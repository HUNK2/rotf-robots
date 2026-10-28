// The original's sky shaders, written again from the compiled ones in `shaders.lsa`
// (`work\shaders\asm`): `sky` (vertex 0b7390, pixel 0b7530) and `sky_vc` (vertex 0b76d0,
// pixel 0b7890). LS-25, notes `notes\level-sky.md` section 4.
//
// [game 0b7390:138-145] the clip position is the object's local-to-screen matrix on the vertex
// (no camera term, no fog) with clip z = w * K, K = c2.z + c3.z (the device-to-screen constants).
// [assumed] K = far: 0 in Bevy's reversed depth (sky.rs `specialize` takes the GreaterEqual test).
// [game 0b7390:146] uv = the vertex's uv, untransformed; no time term.
// [game 0b7530] colour = texel.rgb * IrisParams.x; [game 0b7890:170-171] for sky_vc, times the
// vertex colour's rgb first, and then times IrisParams.x.
// [game 0b7530:159-160] alpha = bloom texture's green * AlphaBoost (and times the vertex alpha for
// sky_vc, 0b7890:172-174). AlphaBoost is 1 (sky.rs, LS-21). LS26: the bloom texture is bound now;
// before, the alpha was the diffuse texel's alpha, 1 for a DXT1 picture, so the horizon-light
// cylinder was opaque teal. [stand-in] IrisParams.x is 1 (sky.rs). No fog, no lighting, no tonemapping.
// Colour space: as surface.wgsl, the final colour goes through `to_linear` (the same sRGB
// decode), so sky and level agree [game-port convention, surface.wgsl].

#import bevy_pbr::{
    forward_io::FragmentOutput,
    mesh_functions,
    view_transformations::position_world_to_clip,
    mesh_view_bindings as view_bindings,
}

const VERTEX_COLOUR: u32 = 1u;
const BACKDROP: u32 = 2u;
const ADD: u32 = 4u;
// [assumed] K = far in reversed depth: clip z = 0 (sky.rs).
const SKY_DEPTH: f32 = 0.0;

struct SkyValues {
    // x: IrisParams.x, the exposure [stand-in] 1
    exposure: vec4<f32>,
    // x: VERTEX_COLOUR for sky_vc
    flags: vec4<u32>,
    // x: AlphaBoost (c1 of 0b7530; 1 for this blend, sky.rs)
    boost: vec4<f32>,
    fog_colour: vec4<f32>,
    fog_range: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: SkyValues;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var diffuse_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var diffuse_sampler: sampler;
// s1 = Bloom_Texture [game 0b7530:159]; its green channel is the alpha.
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var bloom_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var bloom_sampler: sampler;

// Bevy's rigid vertex inputs, as surface.wgsl's blend2 vertex: position at 0, uv at 2 and the
// colour at 5 (only when the mesh has them; Bevy's shader defines say which).
struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
#ifdef VERTEX_UVS_A
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(5) colour: vec4<f32>,
#endif
}

struct SkyVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(2) world_position: vec3<f32>,
#ifdef VERTEX_COLORS
    @location(1) colour: vec4<f32>,
#endif
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@vertex
fn vertex(v: Vertex) -> SkyVertex {
    var out: SkyVertex;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    let clip = position_world_to_clip(world.xyz);
    out.world_position = world.xyz;
    // [game 0b7390:138-145] clip z = w * K. [assumed] K = far (SKY_DEPTH): the sky is behind all
    // geometry and is not cut by the camera's far plane (the x, y and w of the clip are kept).
    out.position = vec4<f32>(clip.x, clip.y, clip.w * SKY_DEPTH, clip.w);
#ifdef VERTEX_UVS_A
    // [game 0b7390:146] the uv is passed through with no transform.
    out.uv = v.uv;
#else
    out.uv = vec2<f32>(0.0);
#endif
#ifdef VERTEX_COLORS
    // [game 0b7890:170] the vertex colour, passed to the pixel stage (`mov o2, v2`, 0b76d0).
    out.colour = v.colour;
#endif
    return out;
}

@fragment
fn fragment(in: SkyVertex) -> FragmentOutput {
    let texel = textureSample(diffuse_texture, diffuse_sampler, in.uv);
    // [game 0b7530:158] rgb = T.rgb * IrisParams.x
    var colour = texel.rgb;
    // [game 0b7530:159-160] a = B.g * AlphaBoost, B sampled at the same uv
    let bloom = textureSample(bloom_texture, bloom_sampler, in.uv);
    var alpha = bloom.g * sky.boost.x;
    if (sky.flags.x & BACKDROP) != 0u {
        // [game 0b7ca0] Background alpha comes from the diffuse texture, not a bloom mask.
        alpha = texel.a * sky.boost.x;
    }
#ifdef VERTEX_COLORS
    if (sky.flags.x & VERTEX_COLOUR) != 0u {
        // [game 0b7890:170] T.rgb * V.rgb, then [game 0b7890:171] times IrisParams.x
        colour *= in.colour.rgb;
        // [game 0b7890:173] the alpha times the vertex alpha
        alpha *= in.colour.a;
    }
#endif
    if (sky.flags.x & BACKDROP) != 0u && sky.fog_colour.w != 0.0 {
        // [game 0b7ca0:32-46] Distance ramp times height falloff, using game z = Bevy y.
        let d = length(in.world_position - view_bindings::view.world_position);
        let distance_fog = saturate((d - sky.fog_range.x) * sky.fog_range.y) * sky.fog_colour.w;
        let h = in.world_position.y / sky.fog_range.w;
        let amount = distance_fog * saturate(1.0 - h * h);
        colour = mix(colour, sky.fog_colour.rgb, amount);
    }
    colour *= sky.exposure.x;

    var out: FragmentOutput;
    let rgb = to_linear(max(colour, vec3<f32>(0.0)));
    if (sky.flags.x & ADD) != 0u {
        out.color = vec4<f32>(rgb * saturate(alpha), 0.0);
    } else {
        out.color = vec4<f32>(rgb, saturate(alpha));
    }
    return out;
}
