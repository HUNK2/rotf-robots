// The original's unlit surface shaders for the meshes that glow, written again from the
// compiled ones in `shaders.lsa` (`work\shaders\asm`): `emissive` (pixel 094420: his eyes,
// the weapon's lights) and `dull_fx_emissive` (vertex 043aa0, pixel 043f90: the ground
// punches' shock rings).
//
// [game]: the texture's place is `X u + Y v + Z + T time`; the colour is the texel times
// `Diffuse_Color` times the object's own colour; `dull_fx_emissive` adds
// `(1 - |V.N|)^Edge_Power * LightColor * Edge_Color` and fades the alpha between
// `inTransFalloff` where the surface faces the camera and `outTransFalloff` where it is
// edge on; the alpha is the texel's times `Diffuse_Texture1_Opacity` times the colour's
// times `AlphaBoost`.
// [assumed]: arithmetic on stored texture values (as `surface.wgsl`); `AlphaBoost` 1.
// With a mission the sun colour and fog come from the level rig. emissive_vc multiplies the
// authored COLOR0 stream (0459c0/094420). [stand-in] IrisParams (exposure) is still 1 in this shader: the
// game multiplies by c6 (IrisParams.x) in 094420_ps:51, and `postprocess.wgsl` applies iris.x to the whole
// frame, so the value is the frame's. [game 2026-10-20] The writer of IrisParams.x is 00562c70
// (iris_exposure_update): each game step it stores the iris in DAT_00c02ee0, the IrisParams block that
// 005a50f0 hands to the shaders as slot 23, from the luminance that 00581a00 (iris_histogram_readback) reads
// back. 00583420 writes no iris (it binds a filter pass). The value is a run-time luminance, so the
// executable does not hold it. QUEUED: settles: a recording of DAT_00c02ee0 per frame (a read-only Cheat
// Engine read, `notes\live-trace.md`), and whether the frame-wide multiply in `postprocess.wgsl` matches the
// per-pixel one of 094420_ps:51.

#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings as view_bindings,
    mesh_functions,
}
#import bevy_pbr::prepass_io as pio
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#import bevy_pbr::view_transformations::{position_ndc_to_world, frag_coord_to_ndc}
#endif

const ADD: u32 = 1u;
const EDGE: u32 = 2u;
const VERTEX_COLOUR: u32 = 4u;
const RIG: u32 = 8u;
const CUTOUT: u32 = 16u;
const SOFT: u32 = 32u;
const MASK: u32 = 64u;
const BARRIER: u32 = 128u;

struct Glow {
    // `Diffuse_Color` times the object's colour
    colour: vec4<f32>,
    // xy `Diffuse_Texture1_X`, zw `Diffuse_Texture1_Y`
    uv_xy: vec4<f32>,
    // xy `Diffuse_Texture1_Z`, zw `Diffuse_Texture1_T`
    uv_zt: vec4<f32>,
    // rgb `Edge_Color`, w `Edge_Power`
    edge: vec4<f32>,
    // x `inTransFalloff`, y `outTransFalloff`, z `Diffuse_Texture1_Opacity`, w `AlphaBoost`
    falloff: vec4<f32>,
    // rgb `LightColor`
    light: vec4<f32>,
    flags: vec4<u32>,
    fog_colour: vec4<f32>,
    fog_range: vec4<f32>,
    cutout: vec4<f32>,
    mask_uv_xy: vec4<f32>,
    mask_uv_zt: vec4<f32>,
    dynamic: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> glow: Glow;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var diffuse_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var diffuse_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var mask_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var mask_sampler: sampler;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2));
}

// [assumed] Same ordered coverage as Surface; native draw-alpha scheduling is owed. settles: the same
// as `surface.wgsl` lod_coverage (the native fade scheduler and a recording of a fading part).
fn lod_coverage(instance: u32, pixel: vec2<f32>) {
    let tag = mesh_functions::get_tag(instance);
    if (tag & 0x80000000u) == 0u { return; }
    let bayer = array<u32,16>(0u,8u,2u,10u,12u,4u,14u,6u,3u,11u,1u,9u,15u,7u,13u,5u);
    let p = vec2<u32>(pixel) & vec2<u32>(3u);
    if f32(tag & 255u) / 255.0 < (f32(bayer[p.y*4u+p.x]) + 0.5) / 16.0 { discard; }
}

#ifndef PREPASS_PIPELINE
// The colour pass's output, plus the shifted depth of a material with a DepthBias (`GLOW_DEPTH_BIAS`).
struct GlowOutput {
    @location(0) color: vec4<f32>,
#ifdef GLOW_DEPTH_BIAS
    @builtin(frag_depth) depth: f32,
#endif
}

// [game 0459c0] view += normalize(view) * DepthBias * 0.1, done per pixel along the eye ray (as
// surface.wgsl). NARROWED 2026-10-20: for ROBOTS the push is zero: the DepthBias word is 0 in all 1,131 robot
// material words (report-X3, RDed3c68; re-checked on optimus.str with work\mat_dump.py), so the unit does not
// matter for them. [assumed] metres for the level case only, the unit `surface.wgsl`'s fragment owns. settles
// (levels only): a game length in the data (a known part size against its mesh).
fn biased_depth(world: vec3<f32>) -> f32 {
    let moved = world + normalize(world - view_bindings::view.world_position) * glow.cutout.w * 0.1;
    let clip = view_bindings::view.clip_from_world * vec4<f32>(moved, 1.0);
    return clip.z / clip.w;
}

@fragment
fn fragment(in: VertexOutput) -> GlowOutput {
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    lod_coverage(in.instance_index, in.position.xy);
#endif
    var uv = vec2<f32>(0.0);
#ifdef VERTEX_UVS_A
    uv = glow.uv_xy.xy * in.uv.x + glow.uv_xy.zw * in.uv.y + glow.uv_zt.xy + glow.uv_zt.zw * view_bindings::globals.time;
#endif
    var texel = textureSample(diffuse_texture, diffuse_sampler, uv);
    // [game095b40] Barrier uses both full RGBA samples on independently transformed
    // TEXCOORD0, sums RGB, and fades summed opacities by distance to the PLAYER.
    if (glow.flags.x & BARRIER) != 0u {
        var raw = vec2<f32>(0.0);
#ifdef VERTEX_UVS_A
        raw = in.uv;
#endif
        let second_uv = glow.mask_uv_xy.xy*raw.x + glow.mask_uv_xy.zw*raw.y + glow.mask_uv_zt.xy + glow.mask_uv_zt.zw*glow.dynamic.x;
        let second = textureSample(mask_texture,mask_sampler,second_uv);
        let fade = saturate(distance(in.world_position.xyz,glow.dynamic.yzw)*glow.falloff.x+glow.falloff.y);
        let alpha = (texel.a*glow.falloff.z + second.a*glow.cutout.z)*fade*glow.falloff.w;
        var out: GlowOutput;
#ifdef GLOW_DEPTH_BIAS
        out.depth = biased_depth(in.world_position.xyz);
#endif
        let rgb = to_linear(saturate(texel.rgb+second.rgb));
        if (glow.flags.x & ADD) != 0u { out.color=vec4<f32>(rgb*alpha,0.0); }
        else { out.color=vec4<f32>(rgb,alpha); }
        return out;
    }
    // [game 049a20:51-54] The red channel of texture 2 replaces texture 1's alpha,
    // before material/object/vertex tint. It is not multiplied by texture 1's opacity.
    var mask_uv = glow.mask_uv_zt.xy + glow.mask_uv_zt.zw * view_bindings::globals.time;
#ifdef VERTEX_UVS_B
    mask_uv += glow.mask_uv_xy.xy * in.uv_b.x + glow.mask_uv_xy.zw * in.uv_b.y;
#endif
    let mask = textureSample(mask_texture, mask_sampler, mask_uv).r;
    if (glow.flags.x & MASK) != 0u { texel.a = mask; }
    else { texel.a *= glow.falloff.z; }
    texel *= glow.colour;
#ifdef VERTEX_COLORS
    if (glow.flags.x & VERTEX_COLOUR) != 0u { texel *= in.color; }
#endif
    var colour = texel.rgb;
    var alpha = texel.a;

    if (glow.flags.x & EDGE) != 0u {
        let n = normalize(in.world_normal);
        let v = normalize(view_bindings::view.world_position - in.world_position.xyz);
        let rim = pow(saturate(1.0 - abs(dot(v, n))), glow.edge.w);
        colour += rim * glow.light.rgb * glow.edge.rgb;
        // How far the normal lies across the camera's line: 0 facing it, 1 edge on.
        let back = normalize(view_bindings::view.world_from_view[2].xyz);
        let along = dot(n, back);
        let across = sqrt(max(1.0 - along * along, 0.0));
        alpha *= saturate(across * (glow.falloff.y - glow.falloff.x) + glow.falloff.x);
    }
#ifdef DEPTH_PREPASS
    if (glow.flags.x & SOFT) != 0u {
        // [game 04af60] Reconstruct the opaque surface and fade by its world distance from
        // the light shaft, divided by ZFeather_Distance. The original uses mScreenToWorld;
        // Bevy's inverse projection and reversed depth provide the same world-position input.
        let depth = prepass_depth(in.position, 0u);
        if depth > 0.0 && glow.cutout.y > 0.0 {
            let behind = position_ndc_to_world(frag_coord_to_ndc(vec4<f32>(in.position.xy, depth, 1.0)));
            alpha *= saturate(length(behind - in.world_position.xyz) / glow.cutout.y);
        }
        // [game 04af60:76-84] The listing's fade is mul_sat(length(recon - particle) * rcp(ZFeather)):
        // an infinite reconstructed distance (rcp of a zero w) and a zero feather both saturate to 1,
        // so the existing alpha is kept, as this guard does. [assumed] The guard itself is the port's:
        // a cleared depth's w is not known statically (mScreenToWorld, c0..c3, is a run-time value),
        // so whether the game's sky gives inf or NaN (0/0, where saturate's answer is the hardware's)
        // is not settled. settles: the value of w at a cleared depth (c0..c3 at the sky, a run-time
        // read or the setter of mScreenToWorld), and a recording of a soft particle against the sky.
    }
#endif
    alpha = saturate(alpha * glow.falloff.w);
    if (glow.flags.x & CUTOUT) != 0u && alpha < glow.cutout.x { discard; }
    if (glow.flags.x & RIG) != 0u && glow.fog_colour.w != 0.0 {
        // [game 094420:34-49] Fog is applied to emissive meshes before exposure (still 1).
        let d = length(in.world_position.xyz - view_bindings::view.world_position);
        let distance_fog = saturate((d - glow.fog_range.x) * glow.fog_range.y) * glow.fog_colour.w;
        let h = in.world_position.y / glow.fog_range.w;
        colour = mix(colour, glow.fog_colour.rgb, distance_fog * saturate(1.0 - h * h));
    }

    let lit = to_linear(saturate(colour));
    var out: GlowOutput;
#ifdef GLOW_DEPTH_BIAS
    out.depth = biased_depth(in.world_position.xyz);
#endif
    if (glow.flags.x & ADD) != 0u {
        out.color = vec4<f32>(lit * alpha, 0.0);
    } else if (glow.flags.x & CUTOUT) != 0u {
        out.color = vec4<f32>(lit, 1.0);
    } else {
        out.color = vec4<f32>(lit, alpha);
    }
    return out;
}
#else
@fragment
fn fragment(in: pio::VertexOutput) {
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    lod_coverage(in.instance_index, in.position.xy);
#endif
    var uv = glow.uv_zt.xy + glow.uv_zt.zw*glow.dynamic.x;
#ifdef VERTEX_UVS_A
    // Shared main/depth scrolling clock, including LOD fade fragment pipelines.
    uv += glow.uv_xy.xy * in.uv.x + glow.uv_xy.zw * in.uv.y;
#endif
    let texel = textureSample(diffuse_texture, diffuse_sampler, uv);
    var raw_mask_uv = vec2<f32>(0.0);
#ifdef VERTEX_UVS_B
    raw_mask_uv = in.uv_b;
#endif
    let mask_uv = glow.mask_uv_xy.xy*raw_mask_uv.x + glow.mask_uv_xy.zw*raw_mask_uv.y + glow.mask_uv_zt.xy + glow.mask_uv_zt.zw*glow.dynamic.x;
    let mask = textureSample(mask_texture,mask_sampler,mask_uv).r;
    var alpha = select(texel.a*glow.falloff.z, mask, (glow.flags.x & MASK) != 0u) * glow.colour.a * glow.falloff.w;
#ifdef VERTEX_COLORS
    if (glow.flags.x & VERTEX_COLOUR) != 0u { alpha *= in.color.a; }
#endif
    if (glow.flags.x & CUTOUT) != 0u && alpha < glow.cutout.x { discard; }
}
#endif
