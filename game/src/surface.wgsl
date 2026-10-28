// The original's surface shaders for his techniques, written again from the compiled
// pixel shaders in `shaders.lsa` (`work\lsa_disasm.py`; the listings are in
// `work\shaders\asm`): `aniso_paint_x` 0ab3b0, `aniso_paint` 0a83d0, `glass_plus` 09a0d0,
// `cheap_plus` 05cb30, `glass_cheap` 098a20; and the level techniques `dull` 038900,
// `cheap` 052560, `basic` 06b4c0 (see `notes\level-shaders-dull-cheap-basic.md`, ticket LS-8).
// One shader here, the differences as flags.
//
// [game]: the normal from the map, the paint ramp, the mix with the colour texture by its
// alpha, the diffuse and specular terms, the edge light, the reflection and its falloff.
// [game]: the arithmetic is on the textures' stored values (no sRGB decode), as a 2009 Direct3D 9
// title did it; the picture's yellow needs it. [game 0ab3b0_ps] the listing decodes nothing (no pow on
// a texel). The sampler's sRGB state (D3DSAMP_SRGBTEXTURE, state 11) is never set: of the 107
// SetSamplerState sites (device slot 0x114, ls17_samp.py), 102 push an immediate type (1, 2, 3, 5, 6,
// 7, 10, none is 11), the material setter `005a2020` sets 1, 2, 3, 5, 6, 7, 10, 005a2115 pushes type 6
// (MINFILTER), 005809f1 pushes type 4 (BORDERCOLOR) with value 0 (005809b7-005809be, read 2026-10-20), and
// 0058261b is not a sampler call (slot 0xe4, SetRenderState 0x16 CULLMODE).
// [game] with `--mission` (flag RIG): LightDir, LightColor, AmbientColor and the nine
// AmbientIrradiance terms are the level's rig. [stand-in] otherwise (the arena): a two-colour
// ambient, and the sun's colour constant. The sun's shadow is Bevy's (both cases), and the
// paint's `RealtimeSphereMap`, which the original renders from the level, is a stand-in.
// [game LS-37] with `--mission` (flag RIG): the level's OMNI, SPOT and CAPSULE lights are summed here, after the
// sun and before the fog (`point_sum`), from one shared buffer (binding 15). No shadows, as in the game's pass.
// Not ported: the masked directional lights. The ambient cube is evaluated directly (E9 at the mapped
// normal, below); `IrisParams` (exposure) is applied once in `postprocess.wgsl`.
// [game 05cb30_ps:257-265, 038900_ps:203-212, 0ab3b0_ps:321-329; LS-29] the level's fog, with flag RIG:
// lerp(colour, FogColor, t) before the colour conversion, t from the eye distance and the world height
// (`fog_factor` below). Its other passes (the arena's stand-in) have no fog.
// LS-24, flag BLEND2 (`blend2_plus_mask_ssd`: vertex 08d150, pixel 08db60, listings in
// `work\shaders\asm`; the arithmetic is in `notes\level-shaders-bgnd-blend2.md` A.4 to A.6):
// two diffuse layers and two normal maps blended by the mask's weights, the mask on its own uv
// set. Its vertex stage is `blend2_vertex`, used only by BLEND2 pipelines (`surface.rs` specialize).
// [game lsa_list 2026-10-20] the Base pixel shaders of the `_ssd` families (052560, 05cb30,
// 038900, 050250, 0738c0, 070fc0, 098a20, 09a0d0, 089a70, 081790, 08db60, 07b0d0, 055970) have no
// damage sampler; decal_ssd's Base pixel shader 055320 does read them (surface.rs DECAL_SSD, open).
// [stand-in] the exposure of the original is not applied here: `postprocess.wgsl` applies IrisParams
// to the whole frame instead (see the fog note at the end of `fragment`). The fog is applied to this technique too (LS-29, flag RIG).
// [game 2026-10-20] The iris is 00562c70's run-time DAT_00c02ee0 (`glow.wgsl` 14). QUEUED: settles: a recording
// of DAT_00c02ee0 per frame, and of this pixel before and after `postprocess.wgsl` (the exposure against the fog).
// LS-41, flag DIFFUSE2 (`basic_cheap` 0738c0, `basic` 06b4c0 and their `_ssd` forms): a second diffuse layer on
// uv set 1, lerped over the first by a2 = tex2.a * Diffuse_Texture2_Opacity, alpha = max(a1, a2) (the pixel
// stage of `basic_cheap`, `notes\level-shaders-skyline.md` 2.2; `basic`, `notes\level-shaders-dull-cheap-basic.md`
// 3.4 and 3.5). Its lights, specular, normal map and fog are the Surface ones. Its point lights are the shared
// loop (LS-37), not a second mechanism: `basic_cheap`'s own per-draw light slots are not read here.
// LS-41, flag AMBIENT_BOOST (`cheap_plus_boost` 0601e0): Ambient_Boost is added to the ambient (`ambient` below).

#import bevy_pbr::{
    forward_io::FragmentOutput,
    mesh_view_bindings as view_bindings,
    mesh_functions,
    shadows,
    view_transformations::position_world_to_clip,
}

const PAINT: u32 = 1u;
const EDGE: u32 = 2u;
const REFLECT_CUBE: u32 = 4u;
const REFLECT_SPHERE: u32 = 8u;
const FALLOFF: u32 = 16u;
const NORMAL_MAP: u32 = 32u;
const VERTEX_COLOR: u32 = 64u;
const SPECULAR: u32 = 128u;
// [data] Additive blend: colour premultiplied by alpha, output alpha 0 (see surface.rs ADD).
const ADD: u32 = 256u;
// [game] the level's light rig (surface.rs RIG): LightColor, LightDir and the ambient from it.
const RIG: u32 = 512u;
// [game 011790f0, 005a2af0] blendmode 0xbc07610c: opaque with a cut-out. Kill below `cutout.x`
// (the alpha threshold, max(alpharef,1)/255, assets.rs); surviving pixels are written with alpha 1.
// [assumed] the compare is the shader's texkill of GBufferAlphaTest (notes\level-blend-modes.md LS-17).
// The kill rule is read [game 051190, 03a130]: texkill of texel.a * opacity * vertex alpha - T, so a
// texel below T is cut (as written here). What is not read: which variant (AlphaTest or not) a 0xbc07610c
// material is drawn with, and the threshold's upload. settles: the variant pick, as `assets.rs`
// `cutout_threshold` (the one home of that question), then a capture of a chain-link fence.
const CUTOUT: u32 = 1024u;
// [game 08db60, 0b7ca0] blend2_plus_mask_ssd (surface.rs BLEND2). Its pipelines also have the BLEND2
// shader define, which switches on the blend2 code below.
const BLEND2: u32 = 2048u;
// [game 0738c0:84-92, 06b4c0:94-99] DIFFUSE2 (surface.rs DIFFUSE2): the second diffuse layer on uv set 1.
const DIFFUSE2: u32 = 4096u;
// [game 0601e0:106, 244] AMBIENT_BOOST (surface.rs AMBIENT_BOOST): Ambient_Boost is added to the ambient.
const AMBIENT_BOOST: u32 = 8192u;
const UNTEXTURED: u32 = 16384u;
const UNIFORM_SPECULAR: u32 = 32768u;
const VERTEX_WEIGHTS: u32 = 65536u;
const FLAT_BLEND_NORMAL: u32 = 131072u;
const BLEND3: u32 = 262144u;
const FLATCARD: u32 = 524288u;
const UNLIT: u32 = 1048576u;
// [game 089a70:102-113] blend2_mask_ssd: one normal map, no blend of two (surface.rs SINGLE_NORMAL).
const SINGLE_NORMAL: u32 = 2097152u;
// [game 063a70:91-92] cheap_plus_mask_ssd: alpha from layer 2's red (surface.rs ALPHA_LAYER2).
const ALPHA_LAYER2: u32 = 4194304u;
const TRANS_FALLOFF: u32 = 8388608u;

// [assumed] Ordered coverage for the native draw fade; colour/depth/shadow use
// the same per-instance value. The native alpha scheduling remains a fidelity debt.
// OPEN (2026-10-20): the Bayer dither is the port's; the game's fade is not read here. settles: the writer of
// the instance tag's 0x80000000 bit and its 8-bit value (the native draw-fade scheduler, not in this batch),
// and a recording of a robot fading out at distance.
fn lod_coverage(instance: u32, pixel: vec2<f32>) {
    let tag = mesh_functions::get_tag(instance);
    if (tag & 0x80000000u) == 0u { return; }
    let bayer = array<u32,16>(0u,8u,2u,10u,12u,4u,14u,6u,3u,11u,1u,9u,15u,7u,13u,5u);
    let p = vec2<u32>(pixel) & vec2<u32>(3u);
    let threshold = (f32(bayer[p.y * 4u + p.x]) + 0.5) / 16.0;
    if f32(tag & 255u) / 255.0 < threshold { discard; }
}

// The prepass (depth and shadow) of a cut-out part has its own entry point at the end of this file.
// Its scrolling/wind clock is supplied by the material, including in shadow layouts
// that have no globals binding (surface.rs update_material_time).
#import bevy_pbr::prepass_io as pio

struct Surface {
    diffuse_color: vec4<f32>,
    // rgb `Specular_Color`, w `Specular_Power`
    specular: vec4<f32>,
    // rgb `Reflection_Color`, w `Reflection_Falloff`
    reflection: vec4<f32>,
    // rgb `Edge_Color`, w `Edge_Power`
    edge: vec4<f32>,
    // rgb `Paint_Color0`, w `Paint_Pow1`
    paint0: vec4<f32>,
    // rgb `Paint_Color1`, w `Paint_Pow2`
    paint1: vec4<f32>,
    // rgb `Paint_Color2`, w `Normal_Map_HeightScale`
    paint2: vec4<f32>,
    // rgb the sun's colour, w `Diffuse_Texture1_Opacity`
    light: vec4<f32>,
    // rgb the ambient from above, w `Reflection_Falloff_Power`
    sky: vec4<f32>,
    // rgb the ambient from below, w `AlphaBoost`
    ground: vec4<f32>,
    // rgb what the paint reflects overhead
    sky_seen: vec4<f32>,
    uv_x: vec4<f32>,
    uv_y: vec4<f32>,
    uv_z: vec4<f32>,
    uv_t: vec4<f32>,
    // the normal map's transform, and the specular map's
    normal_x: vec4<f32>,
    normal_y: vec4<f32>,
    normal_z: vec4<f32>,
    normal_t: vec4<f32>,
    specular_x: vec4<f32>,
    specular_y: vec4<f32>,
    specular_z: vec4<f32>,
    specular_t: vec4<f32>,
    // [game] flag RIG: rgb LightColor; xyz LightDir in Bevy's space; rgb AmbientColor;
    // AmbientIrradiance's nine float4 in the game's order (x^2, y^2, z^2, xy, yz, zx, x, y, z)
    rig_sun: vec4<f32>,
    rig_dir: vec4<f32>,
    rig_ambient: vec4<f32>,
    irradiance: array<vec4<f32>, 9>,
    // x: the cut-out threshold (flag CUTOUT)
    cutout: vec4<f32>,
    flags: vec4<u32>,
    // blend2 (flag BLEND2): the second diffuse layer's transform (TEXCOORD1), the second normal
    // map's (TEXCOORD1), and `blend`: x normal A is DXT5 (A,G), y normal B is DXT5, z normal A's
    // height scale, w normal B's height scale
    diffuse2_x: vec4<f32>,
    diffuse2_y: vec4<f32>,
    diffuse2_z: vec4<f32>,
    diffuse2_t: vec4<f32>,
    normal_b_x: vec4<f32>,
    normal_b_y: vec4<f32>,
    normal_b_z: vec4<f32>,
    normal_b_t: vec4<f32>,
    blend: vec4<f32>,
    // [game] flag RIG (LS-29): fog_color rgb FogColor, w FogColor.w (the falloff);
    // fog_range FogRange: x near, y slope (1 / (far - near)), z unused, w the layer's height.
    fog_color: vec4<f32>,
    fog_range: vec4<f32>,
    // DIFFUSE2 (LS-41): x Diffuse_Texture2_Opacity. AMBIENT_BOOST (LS-41): rgb Ambient_Boost.
    layer2: vec4<f32>,
    ambient_boost: vec4<f32>,
    diffuse3_x: vec4<f32>,
    diffuse3_y: vec4<f32>,
    diffuse3_z: vec4<f32>,
    diffuse3_t: vec4<f32>,
    motion: vec4<f32>,
    // [game 053510 vs:61-63] x: the material word `depthbias` (signed); not read by the vertex stage yet (LS-54).
    depth_bias: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> surface: Surface;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var diffuse_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var diffuse_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var normal_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var normal_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var specular_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var specular_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var reflection_texture: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var reflection_sampler: sampler;
// blend2 and DIFFUSE2 [LS-24, LS-41]: the second diffuse layer (`diffuse_texture2_texture`); blend2 only: the mask
// (`diffuse_mix_texture`) and the second normal map (`normal_mapb_texture`).
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var diffuse2_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var diffuse2_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var mask_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var mask_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(13) var normal_b_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(14) var normal_b_sampler: sampler;

// [game LS-37, surface.rs GpuLight] one level light in Bevy's space. Record 0 is a placeholder; the lights are
// records 1 .. arrayLength - 1. The colours are before the far fade.
struct PointLight {
    // xyz: the record's position (omni, spot: the Light; capsule: Pc). w: the kind, 0 omni, 1 spot, 2 capsule
    place: vec4<f32>,
    // xyz: spot axis (c10) or capsule segment A (c10). w: capsule 1 / |A|^2
    axis: vec4<f32>,
    // rgb colour (omni c10, spot c11, capsule c11)
    colour: vec4<f32>,
    // rgb capsule colour at u = 1 (c12)
    colour2: vec4<f32>,
    // omni c11, spot c12, capsule c13 (point_light.rs params)
    params: vec4<f32>,
    // xyz the Light's own position (the far fade's distance to the eye), w the far fade's reach
    origin: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(15) var<storage, read> point_lights: array<PointLight>;
@group(#{MATERIAL_BIND_GROUP}) @binding(16) var<storage, read> light_tiles: array<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(17) var diffuse3_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(18) var diffuse3_sampler: sampler;
// The level's ambient occlusion (`ssao.rs`): g is the game's DownsampledSSAOMap green channel.
@group(#{MATERIAL_BIND_GROUP}) @binding(19) var ssao_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(20) var ssao_sampler: sampler;

// What the fragment stage receives: Bevy's `VertexOutput` (forward_io) with the same locations,
// which every technique's vertex stage writes, plus the blend2 mask uv (location 8) that only the
// `blend2_vertex` stage writes. The fields follow Bevy's defines, so the two match.
struct SurfaceVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    @location(6) @interpolate(flat) instance_index: u32,
#endif
#ifdef VERTEX_UVS_A
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_UVS_B
    @location(3) uv_b: vec2<f32>,
#endif
#ifdef VERTEX_TANGENTS
    @location(4) world_tangent: vec4<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(5) color: vec4<f32>,
#endif
#ifdef BLEND2
    @location(8) uv2: vec2<f32>,
    @location(9) uv3: vec2<f32>,
#endif
#ifdef FLATCARD
    @location(10) fog_position: vec3<f32>,
#endif
#ifdef VERTEX_BINORMAL
    @location(12) world_binormal: vec3<f32>,
#endif
}

#ifdef FLATCARD
struct FlatcardVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
#ifdef VERTEX_UVS_A
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(5) color: vec4<f32>,
#endif
    @location(10) anchor: vec3<f32>,
    @location(11) encoded_normal: vec3<f32>,
}

// [game 0b2300/0ad330/0b1340] The anchor is (u,1-v,w). Transform the
// anchor into view space, then add the corner offset and authored sinusoidal
// wind in view XY. The fragment fog input remains the unexpanded anchor.
@vertex
fn flatcard_vertex(v: FlatcardVertex) -> SurfaceVertex {
    var out: SurfaceVertex;
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = v.instance_index;
#endif
    let frame = mesh_functions::get_world_from_local(v.instance_index);
    let anchor = v.anchor * vec3<f32>(1.0, -1.0, 1.0) + vec3<f32>(0.0, 1.0, 0.0);
    let offset = v.position - anchor;
    let origin = mesh_functions::mesh_position_local_to_world(frame, vec4<f32>(0.0,0.0,0.0,1.0));
    let at = mesh_functions::mesh_position_local_to_world(frame, vec4<f32>(anchor,1.0));
    let phase = anchor.x + sign(offset.x) * surface.motion.z * 3.14159012
        + surface.motion.w * surface.motion.x * (fract(origin.x) + 0.5);
    let angle = fract(phase * 0.159154937 + 0.5) * 6.28318548 - 3.14159274;
    let wind = vec2<f32>(sin(angle), cos(angle)) * surface.motion.y;
    let world = at.xyz + view_bindings::view.world_from_view[0].xyz * (offset.x + wind.x)
        + view_bindings::view.world_from_view[1].xyz * (offset.y + wind.y)
        - view_bindings::view.world_from_view[2].xyz * offset.z;
    // [game] The PS receives the anchor (0b2300/0b1340); only clip
    // position includes the camera-space corner and wind expansion.
    out.world_position = at;
    out.world_normal = normalize(mesh_functions::mesh_normal_local_to_world(v.encoded_normal - vec3<f32>(0.0,1.0,0.0), v.instance_index));
    out.position = position_world_to_clip(world);
    out.fog_position = at.xyz;
#ifdef VERTEX_UVS_A
    out.uv = v.uv;
#endif
#ifdef VERTEX_COLORS
    out.color = v.color;
#endif
    return out;
}
#endif

// Bevy's `Vertex` for a rigid mesh, plus the mask uv (location 8, `surface.rs` BLEND2_UV2).
#ifdef BLEND2
struct Blend2Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_b: vec2<f32>,
#ifdef VERTEX_TANGENTS
    @location(4) tangent: vec4<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(5) color: vec4<f32>,
#endif
    @location(8) uv2: vec2<f32>,
    @location(9) uv3: vec2<f32>,
#ifdef VERTEX_BINORMAL
    @location(12) binormal: vec3<f32>,
#endif
}

// [LS-24] blend2 meshes are not skinned or morphed (the technique has no skin form: the entry
// in `notes\level-shaders.md`, table 4, row 8), so this is Bevy's rigid `vertex` (`bevy_pbr`
// mesh.wgsl) with the mask uv added. The mask is passed through untransformed [game 08db60:146].
@vertex
fn blend2_vertex(v: Blend2Vertex) -> SurfaceVertex {
    var out: SurfaceVertex;
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = v.instance_index;
#endif
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(v.normal, v.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef VERTEX_UVS_A
    out.uv = v.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = v.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, v.tangent, v.instance_index);
#endif
#ifdef VERTEX_BINORMAL
    // [data 2026-10-20] the pack's own stream, as 08d150_vs passes it on (dcl_binormal v3): a vector, so w 0.
    out.world_binormal = mesh_functions::mesh_tangent_local_to_world(world_from_local, vec4<f32>(v.binormal, 0.0), v.instance_index).xyz;
#endif
    out.uv2 = v.uv2;
    out.uv3 = v.uv3;
#ifdef VERTEX_COLORS
    out.color = v.color;
#endif
    return out;
}
#endif

// Bevy's space to the game's (z up, y forward), which the cube map is laid out in.
fn game_space(v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(v.x, -v.z, v.y);
}

// [game 0382c0_vs:73-89] the vertex shader's sum over the normal n (in the game's space):
// c0 x^2 + c1 y^2 + c2 z^2 + c3 xy + c4 yz + c5 zx + c6 x + c7 y + c8 z. The listing has no
// clamp (`nrm`, then the sum, then `add o5, c22`), so none here. [unknown] c22 (the listing's
// constant term) has no value found in the game's data, so it is left out.
fn irradiance(n: vec3<f32>) -> vec3<f32> {
    let k = surface.irradiance;
    let e = k[0] * (n.x * n.x) + k[1] * (n.y * n.y) + k[2] * (n.z * n.z)
        + k[3] * (n.x * n.y) + k[4] * (n.y * n.z) + k[5] * (n.z * n.x)
        + k[6] * n.x + k[7] * n.y + k[8] * n.z;
    return e.rgb;
}

// [game 05cb30_ps:257-263, 038900_ps:203-208, 0ab3b0_ps:321-327; LS-29] the fog's blend amount:
// t = saturate((d - near) * slope) * FogColor.w * saturate(1 - (z / height)^2), d the eye-to-pixel
// distance and z the pixel's height in the game's space (up = z; `game_space` gives it from
// Bevy's y). The lit shaders take d as |c3 - v3| (the eye and the interpolated world position).
fn fog_factor(distance_to_eye: f32, game_height: f32) -> f32 {
    let range = surface.fog_range;
    let by_distance = clamp((distance_to_eye - range.x) * range.y, 0.0, 1.0) * surface.fog_color.w;
    let r = game_height / range.w;
    return by_distance * clamp(1.0 - r * r, 0.0, 1.0);
}

// [game LS-37, point_light.rs] The point lights, the arithmetic of `omni_diffuse_specular`,
// `spot_diffuse_specular` and `capsule_diffuse_specular` (Base variants, full values), written line for line.
// No shadows [game]. The listings 008250 (omni), 008e70 (spot) and 009cb0 (capsule) have NO skip branch
// (no if, cmp or texkill): the light is always computed and the saturated ramp makes it zero outside the
// range. With inner <= outer that is zero beyond outer, so a skip there is exact [game]. With inner > outer the
// ramp is 1 beyond outer, so a skip would differ from the game: `mission_lights` counts those lights [assumed
// that the port's skip matches the arithmetic only for inner <= outer]. settles: the count of inverted lights
// in the packs (data), and whether the port's skip is ever taken for one.
// The normal n is normalised here (the forward path); the core uses it as given.

struct PointTerms {
    diffuse: vec3<f32>,
    specular: vec3<f32>,
}

// [game 008250_ps, the omni ramp: `add r0.x, -c11.y, c11.x; rcp r0.x, r0.x; mul_sat r0.x, r0.x, r1.y`] u = sat(rcp(inner - outer) * (value - outer)), no zero guard. At a zero span
// rcp gives +inf: 1 above outer, 0 below it [game]. At value == outer the product is 0 * inf (NaN) and
// saturate's answer for NaN is the hardware's [assumed: 0 here]. Only reached when span == 0 and value
// == outer exactly; for span != 0 the port's clamp is exact. The ps_3_0 mul_sat(0 * inf) answer is the
// GPU's, not in the executable (wave 27 O7). settles: a D3D9 test of mul_sat(0 * inf) on the user's GPU.
fn point_ramp(value: f32, outer: f32, inner: f32) -> f32 {
    let span = inner - outer;
    if span == 0.0 {
        if value > outer { return 1.0; }
        return 0.0;
    }
    return clamp((value - outer) / span, 0.0, 1.0);
}

// [game 008250, 008e70, 009cb0] t * t * (3 - 2t)
fn point_smooth(t: f32) -> f32 {
    return t * t * (3.0 - 2.0 * t);
}

// [game] pow(sat(N.H), SP) * (SP / 30 + 1)
fn point_lobe(n: vec3<f32>, h: vec3<f32>, power: f32) -> f32 {
    return pow(max(clamp(dot(n, h), 0.0, 1.0), 0.000001), power) * (power / 30.0 + 1.0);
}

fn point_unit(v: vec3<f32>) -> vec3<f32> {
    let len = length(v);
    return select(vec3<f32>(0.0), v / len, len > 0.0);
}

// [game point_light.rs far_fade; 0054c31f, 0054c760 (the 0.02 at [00cad454]); LS35.2 gate] The colour's fade from
// D, the distance from the eye to the Light's position, over the reach: 1 - (0.02 D / reach)^2, and no record
// (0 here) from D / reach >= 50. The cut is continuous: the fade is 0 at 50.
fn point_fade(light: PointLight, eye: vec3<f32>) -> f32 {
    let reach = light.origin.w;
    if !(reach > 0.0) {
        return 0.0;
    }
    let ratio = distance(light.origin.xyz, eye) / reach;
    if ratio >= 50.0 {
        return 0.0;
    }
    let k = 0.02 * ratio;
    return 1.0 - k * k;
}

// The diffuse and specular of one light at p (game: point_light.rs *_diffuse_specular). Specular is for the
// technique's own specular term (the caller multiplies it by Specular_Color and the specular map).
fn point_terms(light: PointLight, p: vec3<f32>, n: vec3<f32>, v: vec3<f32>, power: f32, eye: vec3<f32>) -> PointTerms {
    var out: PointTerms;
    out.diffuse = vec3<f32>(0.0);
    out.specular = vec3<f32>(0.0);
    let fade = point_fade(light, eye);
    if fade <= 0.0 {
        return out;
    }
    let kind = u32(light.place.w + 0.5);
    if kind == 0u {
        // OMNI, 008250: L = c9 - P, d2 = |L|^2, t = ramp(d2, outer2, inner2), atten = smooth(t).
        let to_light = light.place.xyz - p;
        let d2 = dot(to_light, to_light);
        if d2 >= light.params.y {
            return out;
        }
        let l = point_unit(to_light);
        let atten = point_smooth(point_ramp(d2, light.params.y, light.params.x));
        let colour = light.colour.rgb * fade;
        let h = point_unit(v + l);
        out.diffuse = atten * clamp(dot(n, l), 0.0, 1.0) * colour;
        out.specular = atten * colour * point_lobe(n, h, power);
    } else if kind == 1u {
        // SPOT, 008e70: radial ramp in d2 (z inner, w outer); cone = smooth(ramp(L^ . D, cos outer, cos inner)).
        // The diffuse and the specular use the AXIS D, not L^ (the quirk the core copies).
        let to_light = light.place.xyz - p;
        let d2 = dot(to_light, to_light);
        if d2 >= light.params.w {
            return out;
        }
        let l = point_unit(to_light);
        let axis = light.axis.xyz;
        let radial = point_smooth(point_ramp(d2, light.params.w, light.params.z));
        let cone = point_smooth(point_ramp(dot(l, axis), light.params.y, light.params.x));
        let atten = radial * cone;
        let colour = light.colour.rgb * fade;
        let h = point_unit(v + axis);
        out.diffuse = atten * clamp(dot(n, axis), 0.0, 1.0) * colour;
        out.specular = atten * colour * point_lobe(n, h, power);
    } else {
        // CAPSULE, 009cb0: the segment from Pc along A; u = sat((P - Pc) . A / |A|^2); the radii lerp by u.
        let a = light.axis.xyz;
        let pc = light.place.xyz;
        // Skip beyond the outer radius: P farther than that from the segment's bounding sphere.
        let centre = pc + 0.5 * a;
        let outer_max = max(light.params.y, light.params.w);
        if distance(p, centre) > 0.5 * length(a) + outer_max {
            return out;
        }
        let rel = p - pc;
        let u = clamp(dot(rel, a) * light.axis.w, 0.0, 1.0);
        let ro = light.params.y + (light.params.w - light.params.y) * u;
        let ri = light.params.x + (light.params.z - light.params.x) * u;
        let q = pc + a * u;
        let qp = q - p;
        let dist2 = dot(qp, qp);
        let atten = point_smooth(point_ramp(dist2, ro * ro, ri * ri));
        let colour = mix(light.colour.rgb, light.colour2.rgb, u) * fade;
        let lq = point_unit(qp);
        out.diffuse = atten * clamp(dot(n, lq), 0.0, 1.0) * colour;
        // The specular at a representative point (009cb0 lines 78-93): M = V - 2 (V.N) N,
        // w = sat((rel . (A.M M - A)) / ((A.M)^2 - |A|^2)), Q2 = Pc + w A, H = normalize(normalize(Q2 - P) + V).
        let m = v - 2.0 * dot(v, n) * n;
        let am = dot(a, m);
        let denom = am * am - dot(a, a);
        var w = 0.0;
        if denom != 0.0 {
            w = clamp(dot(rel, m * am - a) / denom, 0.0, 1.0);
        }
        let q2 = pc + a * w;
        let h = point_unit(point_unit(q2 - p) + v);
        out.specular = atten * colour * point_lobe(n, h, power);
    }
    return out;
}

// The sum over every light of the level (records 1 .. arrayLength - 1).
fn point_sum(p: vec3<f32>, n: vec3<f32>, v: vec3<f32>, power: f32, eye: vec3<f32>, pixel: vec2<f32>) -> PointTerms {
    var sum: PointTerms;
    sum.diffuse = vec3<f32>(0.0);
    sum.specular = vec3<f32>(0.0);
    let light_count = arrayLength(&point_lights);
    var count = light_count - 1u;
    var offset = 0u;
    let tiled = light_tiles[0] != 0u && light_tiles[1] != 0u;
    if tiled {
        let local = max(pixel - vec2<f32>(f32(light_tiles[3]), f32(light_tiles[4])), vec2<f32>(0.0));
        let tile = min(vec2<u32>(local / f32(light_tiles[2])), vec2<u32>(light_tiles[0]-1u, light_tiles[1]-1u));
        let slot = 8u + 2u * (tile.y * light_tiles[0] + tile.x);
        offset = light_tiles[slot];
        count = light_tiles[slot+1u];
    }
    for (var i = 0u; i < count; i += 1u) {
        var index = i + 1u;
        if tiled { index = light_tiles[offset + i]; }
        let t = point_terms(point_lights[index], p, n, v, power, eye);
        sum.diffuse += t.diffuse;
        sum.specular += t.specular;
    }
    return sum;
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

// [game] each texture's UV is X*u + Y*v + Z + T*time with its own X, Y, Z, T: the diffuse
// (`0382c0:66-70`, `05c290:87-91`), the normal and the specular map (`05c290:117-124`).
// [assumed] `time` is Bevy's `globals.time`, as the port has always used it. The game's own clock is
// the `Time` constant (c21 in 08d150_vs and in the other Base vertex listings), filled by the engine
// per frame, and the port's colour pass reads it from Bevy. [game 2026-10-20] the hash 0xcfa6377b
// (CRC-32 of `Time`) is in no immediate of the code (`work\find_imm.py`) and not in the executable's
// bytes. Of the five "Time" texts in the executable four are longer strings ("Decay Time", "By Time",
// a score table header) and the one bare "Time" (00b66318) is read only by 00ae3000 / 00ae3050, the
// C runtime's locale code [game, lead 2026-10-20]: the engine does not bind c21 by that text. It
// binds it by hash: 0058eb00 matches each constant of a shader's table against the 37 engine names at
// 00b346a8 (CRC-32 of the lower-cased name; `time` 6f949845 is entry 20, beside mlocaltoview and
// objectdiffusecolor) and stores (register u16, count u8, entry u8) per match (0058ed28-0058ed4c).
// The records are written at 00580000.c:10577-10590 (4 bytes each, into the table at piVar8[2]); no
// `case 0x14` constant upload exists in the decompile (wave 27 O7 grep), so the reader is likely a table
// indexed by the entry byte. settles: the reader of those 4-byte records (xrefs to the piVar8[2] buffer),
// its entry 20 (which clock it uploads), and that clock's rate against the game's 0.032 s step.
fn transform_uv(x: vec4<f32>, y: vec4<f32>, z: vec4<f32>, t: vec4<f32>, uv: vec2<f32>, time: f32) -> vec2<f32> {
    return x.xy*uv.x + y.xy*uv.y + z.xy + t.xy*time;
}

// [game 08db60:106-112] one normal map's tangent-space vector: x, y = 2v - 1 from (R, G), or from
// (A, G) when the map is DXT5 (`dxt5` above 0.5); z = sqrt(|x^2 + y^2 - 1|) from the unscaled x, y;
// then x and y are times the height scale (lines 113, 120: `mul_pp r3.xy, r2.zwzw, c32.x`).
fn blend2_normal(map: texture_2d<f32>, samp: sampler, uv: vec2<f32>, dxt5: f32, height: f32) -> vec3<f32> {
    let t = textureSample(map, samp, uv);
    let m = vec2<f32>(select(t.r, t.a, dxt5 > 0.5), t.g) * 2.0 - 1.0;
    let z = sqrt(abs(dot(m, m) - 1.0));
    return vec3<f32>(m.x * height, m.y * height, z);
}

#ifndef PREPASS_PIPELINE
// The colour pass's output: Bevy's `FragmentOutput` colour, plus the shifted depth of a material with a
// DepthBias (`SURFACE_DEPTH_BIAS`, set by `specialize` for the colour pass only).
struct SurfaceOutput {
    @location(0) color: vec4<f32>,
#ifdef SURFACE_DEPTH_BIAS
    @builtin(frag_depth) depth: f32,
#endif
}

@fragment
fn fragment(in: SurfaceVertex) -> SurfaceOutput {
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    lod_coverage(in.instance_index, in.position.xy);
#endif
    let flags = surface.flags.x;
#ifdef VERTEX_UVS_A
    let raw_uv = in.uv;
#else
    let raw_uv = vec2<f32>(0.0);
#endif
    let time = view_bindings::globals.time;
    let uv = transform_uv(surface.uv_x, surface.uv_y, surface.uv_z, surface.uv_t, raw_uv, time);
    let normal_uv = transform_uv(surface.normal_x, surface.normal_y, surface.normal_z, surface.normal_t, raw_uv, time);
    let specular_uv = transform_uv(surface.specular_x, surface.specular_y, surface.specular_z, surface.specular_t, raw_uv, time);
    // uv set 1 (TEXCOORD1; LS-24 blend2, LS-41 DIFFUSE2). Zero where the mesh has no set 1.
#ifdef VERTEX_UVS_B
    let raw_uv_b = in.uv_b;
#else
    let raw_uv_b = vec2<f32>(0.0);
#endif
    // LS-41 [game 0738c0:84-92, 06b4c0:94-96]: the second diffuse layer's uv (its own X, Y, Z, T) and its texel.
    // Sampled for every technique (a plain one never reads it); only DIFFUSE2 uses it below.
    let diffuse2_uv = transform_uv(surface.diffuse2_x, surface.diffuse2_y, surface.diffuse2_z, surface.diffuse2_t, raw_uv_b, time);
    let diffuse2_texel = textureSample(diffuse2_texture, diffuse2_sampler, diffuse2_uv);

#ifdef BLEND2
    // [game 08db60:99-102] the mask (TEXCOORD2, no transform) gives the weights R' = R/(R+G) and
    // G' = G/(R+G). [game] There is no guard: a zero sum is an infinite reciprocal (rcp) there too,
    // and the same happens here [unknown what the level data intends for it].
    let mask_uv = select(in.uv2, in.uv3, (flags & BLEND3) != 0u);
    let sampled_mask = textureSample(mask_texture, mask_sampler, mask_uv).rgb;
    var mask = sampled_mask;
#ifdef VERTEX_COLORS
    if (flags & VERTEX_WEIGHTS) != 0u { mask = in.color.rgb; }
#endif
    let mask_sum = mask.r + mask.g + select(0.0, mask.b, (flags & BLEND3) != 0u);
    let weight1 = mask.r * (1.0 / mask_sum);
    let weight2 = mask.g * (1.0 / mask_sum);
    let weight3 = select(0.0, mask.b / mask_sum, (flags & BLEND3) != 0u);
    // [game 08db60:66-70] the second layer and the second normal map use TEXCOORD1, each with its
    // own transform (`diffuse_texture2_*`, `normal_mapb_*`).
    let layer2_uv = transform_uv(surface.diffuse2_x, surface.diffuse2_y, surface.diffuse2_z, surface.diffuse2_t, raw_uv_b, time);
    let normal_b_uv = transform_uv(surface.normal_b_x, surface.normal_b_y, surface.normal_b_z, surface.normal_b_t, raw_uv_b, time);
    // [game 08db60:103-104] D = T1 * R' + T2 * G', rgba: T1 on TEXCOORD0 (`uv`), T2 on TEXCOORD1.
    let layer1 = textureSample(diffuse_texture, diffuse_sampler, uv);
    let layer2 = textureSample(diffuse2_texture, diffuse2_sampler, layer2_uv);
    // [game 090410/090df0] The third diffuse owns TEXCOORD2 and its own UV transform.
    // TEXCOORD3 belongs to the RGB mask, so it cannot share that attribute.
    let layer3_uv = transform_uv(surface.diffuse3_x, surface.diffuse3_y, surface.diffuse3_z, surface.diffuse3_t, in.uv2, time);
    let layer3 = textureSample(diffuse3_texture, diffuse3_sampler, layer3_uv);
    let blended = layer1 * weight1 + layer2 * weight2 + layer3 * weight3;
#endif

    var n = normalize(in.world_normal);
#ifdef BLEND2
#ifdef VERTEX_TANGENTS
    // [game 08db60:106-128] the two normal maps, blended in tangent space by the same weights
    // (`N_ts = R' nA + G' nB`, line 124), then the basis `N = Ng z + T y + B x` with Ng the
    // geometric normal (line 106). [assumed] (the blend2 path: no robot part is blend2, a level-only
    // debt) B is cross(normal, tangent) times the tangent's sign
    // (`assets.rs` mesh) only where the part has no binormal stream: that rebuild is the port's for the
    // other techniques too, whose vertex stages for a binormal are not read. [data 2026-10-20] the packs' streams
    // differ from sign * cross(N, T) on ~1.5% of parts (optimus calf_l / calf_r, some weapons), so a part
    // with a stream uses it (`VERTEX_BINORMAL`, `surface.rs` BINORMAL), as 08d150_vs does. Capture check: the
    // calf and weapon parts (not yet done). [data 2026-10-20, robots] No robot part is blend2, so this block
    // never runs for the robots. Of the robot parts with normals, every one but the 25 `flatcard_emissive`
    // parts has a binormal stream (scratch count over characters\*.str); the rebuild reaches the robots only
    // through those 25 (the other techniques' rebuild is in their own path). OPEN: settles: the vertex listing
    // of flatcard_emissive's Base (not read here) and whether its pixel stage reads the binormal.
    let tangent = normalize(in.world_tangent.xyz);
#ifdef VERTEX_BINORMAL
    // The pack's own binormal stream (`surface.rs` BINORMAL), as the game's shader uses it.
    let binormal = normalize(in.world_binormal);
#else
    let binormal = normalize(cross(n, tangent) * in.world_tangent.w);
#endif
    let normal_a = blend2_normal(normal_texture, normal_sampler, normal_uv, surface.blend.x, surface.blend.z);
    let normal_b = blend2_normal(normal_b_texture, normal_b_sampler, normal_b_uv, surface.blend.y, surface.blend.w);
    // BLEND3 and SINGLE_NORMAL (blend2_mask_ssd, [game 089a70:102-113]) have one normal map.
    let tangent_normal = select(weight1 * normal_a + weight2 * normal_b, normal_a, (flags & (BLEND3 | SINGLE_NORMAL)) != 0u);
    if (flags & FLAT_BLEND_NORMAL) == 0u {
        n = normalize(n * tangent_normal.z + tangent * tangent_normal.y + binormal * tangent_normal.x);
    }
#endif
#else
    if (flags & FLATCARD) != 0u && (flags & NORMAL_MAP) != 0u {
        // [game 0b2a20] Flatcard's bump basis: (-Ny,Nx,0) along map Y,
        // game-up along map X, with no mesh tangent stream.
        let geometric = game_space(n);
        let m = blend2_normal(normal_texture, normal_sampler, normal_uv, surface.blend.x, surface.blend.z);
        let bumped = normalize(geometric * m.z + vec3<f32>(-geometric.y, geometric.x, 0.0) * m.y + vec3<f32>(0.0,0.0,m.x));
        n = vec3<f32>(bumped.x, bumped.z, -bumped.y);
    }
#ifdef VERTEX_TANGENTS
    if (flags & NORMAL_MAP) != 0u && (flags & FLATCARD) == 0u {
        // The map's x goes along the mesh's binormal and its y along its tangent, each
        // times the height scale; z is rebuilt from them.
        let tangent = normalize(in.world_tangent.xyz);
        let binormal = normalize(cross(n, tangent) * in.world_tangent.w);
        // [game 063a70:98, 05cb30:29] x from the map's alpha when Normal_Map_DXT5_NM is set (`blend.x`), as
        // the blend2 path already did; z from the unscaled x, y, then x and y times the height scale.
        let m = blend2_normal(normal_texture, normal_sampler, normal_uv, surface.blend.x, surface.paint2.w);
        n = normalize(n * m.z + tangent * m.y + binormal * m.x);
    }
#endif
#endif

    let to_eye = view_bindings::view.world_position - in.world_position.xyz;
    let v = normalize(to_eye);

    // [stand-in] the sun and its shadow are Bevy's. The original's paint (0ab3b0_ps, read 2026-10-20)
    // DOES sample shadows: ShadowMap0 (s1) at four taps (lines 150-156, offsets from c49 / c53, combined
    // by a compare and `dp4 ... c49.z`), ShadowMap1 (s2) at one tap (line 163, `cmp` against r4.z), with
    // mShadowMap0/1 (c4, c8) projecting the position. The earlier note that it takes no shadow samples
    // was WRONG. The maps are the game's own trapezoid shadow, rendered by a shadow pass that is not
    // ported, so Bevy's map would be sampled in its place. settles: the shadow-generation pass (its
    // trapezoid matrix, `mShadowTrapezoid1`) to port the maps; the filter is in 0ab3b0_ps:150-170.
    var l = vec3<f32>(0.0, 1.0, 0.0);
    var shadow = 1.0;
    if view_bindings::lights.n_directional_lights > 0u {
        let sun = &view_bindings::lights.directional_lights[0];
        l = (*sun).direction_to_light;
        if ((*sun).flags & 1u) != 0u {
            let view_z = dot(vec4<f32>(
                view_bindings::view.view_from_world[0].z,
                view_bindings::view.view_from_world[1].z,
                view_bindings::view.view_from_world[2].z,
                view_bindings::view.view_from_world[3].z
            ), in.world_position);
            shadow = shadows::fetch_directional_shadow(0u, in.world_position, in.world_normal, view_z, in.position.xy);
        }
    }
    var light_color = surface.light.rgb;
    // [stand-in] the arena's two-colour ambient by the normal's height (no level). The arena is a stage
    // with no game ambient of its own; the game's value is the level's rig (flag RIG above). settles: nothing
    // in the executable for the arena; it stays a stage choice unless a level's rig is borrowed.
    var ambient = mix(surface.ground.rgb, surface.sky.rgb, n.y * 0.5 + 0.5);
    if (flags & RIG) != 0u {
        // [game] LightDir and LightColor from the level's rig; the shadow stays Bevy's.
        l = surface.rig_dir.xyz;
        light_color = surface.rig_sun.rgb;
        // [game] The deferred path (the user's VideoQuality 2): ambient = 2 * Cube(N') - 1 + AmbientColor
        // (effects 007650), and each texel of the 32x32 ambient cube is 0.5 * E9(d) + 0.5 for its direction d
        // (effects 011620, the face pass of `UpdateAmbientMap` 01173740 with MajorAxisMask / ScMask / TcMask).
        // So ambient = 2 * Cube(N') - 1 + AmbientColor at the MAPPED normal N' (`n` here, after the normal map).
        // The cube is created as engine format 0x16 (00539ab0 -> 00ac2d80 -> 00986600), and the platform
        // layer's formats are D3DFORMAT values (its pitch switch 00abf280: 0x14 3 bytes, 0x15 / 0x16 4, 0x71 8,
        // 0x74 16, the DXTn codes), so 0x16 is X8R8G8B8: each texel is clamped to [0, 1] and kept in 8 bits,
        // reproduced here. [game, read 2026-10-20; that the descriptor's format reaches CreateCubeTexture
        // unchanged is inferred from the ids matching, not traced] The cube's 32x32 bilinear sampling is not
        // reproduced (the per-pixel E9 stands for it) [assumed: small; not measured].
        let cube_texel = round(clamp(0.5 * irradiance(game_space(n)) + 0.5, vec3(0.0), vec3(1.0)) * 255.0) / 255.0;
        ambient = 2.0 * cube_texel - 1.0 + surface.rig_ambient.rgb;
        // [game 007650] occl = 1 - sat(SSAO.g * SSAOFinalScale) multiplies the ambient (`rig_ambient.w` is the
        // scale, 0 without SSAO). The map is screen-sized: sampled at this pixel's screen position.
        let ssao_uv = in.position.xy / view_bindings::view.viewport.zw;
        let occlusion = textureSampleLevel(ssao_texture, ssao_sampler, ssao_uv, 0.0).g;
        ambient *= 1.0 - saturate(occlusion * surface.rig_ambient.w);
    }
    // LS-41 [game 0601e0:106]: cheap_plus_boost's diffuse sum starts as `add r3.xyz, c32, v7` (Ambient_Boost
    // plus the ambient), and nothing scales r3 before `add r2.xyz, r3, r4` and `mad r0.xyz, r0, r2, r1`
    // (0601e0:244-246): so Ambient_Boost joins the ambient with weight 1, and the albedo multiplies it.
    if (flags & AMBIENT_BOOST) != 0u {
        ambient += surface.ambient_boost.rgb;
    }

    let half_way = normalize(v + l);
    let n_dot_h = max(saturate(dot(n, half_way)), 0.000001);
#ifdef BLEND2
    // [game 08db60:96-97, 129] the specular map is the blended diffuse alpha: there is no
    // specular texture in this shader.
    let specular_map = vec3<f32>(blended.a);
#else
    let sampled_specular = textureSample(specular_texture, specular_sampler, specular_uv).rgb;
    let specular_map = select(sampled_specular, vec3<f32>(1.0), (flags & UNIFORM_SPECULAR) != 0u);
#endif
    let sampled_diffuse = textureSample(diffuse_texture, diffuse_sampler, uv);
    let texel = select(sampled_diffuse, vec4<f32>(1.0), (flags & UNTEXTURED) != 0u);

    var albedo: vec3<f32>;
    var alpha: f32;
#ifdef BLEND2
    // [game 08db60:105, 08d150:107-108] albedo = D * (Diffuse_Color * ObjectDiffuseColor): the vertex
    // stage forms o8 = Diffuse_Color * c23. The port carries that product in `surface.diffuse_color`
    // (`surface.rs` tint, which multiplies the placed Instance's colour in), so this line is the game's.
    // [game 00580660:005808dd] c23 is uploaded per draw from the material record's float4 (crc d5fc1313).
    // [assumed] (level-only: no robot part is blend2) that float4 is the Colorizer tint (as `surface.rs` tint); its writer is not found.
    albedo = blended.rgb * surface.diffuse_color.rgb;
    // [game 08db60:289] alpha = albedo alpha * AlphaBoost (`ground.w`).
    alpha = blended.a * surface.diffuse_color.a * surface.ground.w;
#else
    if (flags & PAINT) != 0u {
        // The paint: dark to its colour to its highlight by how nearly the surface faces
        // half way between eye and sun; the colour texture covers it by its alpha.
        var paint = mix(surface.paint0.rgb, surface.paint1.rgb, pow(n_dot_h, surface.paint0.w));
        paint = mix(paint, surface.paint2.rgb, pow(n_dot_h, surface.paint1.w));
        albedo = mix(paint, texel.rgb, texel.a) * surface.diffuse_color.rgb;
        alpha = surface.ground.w * surface.diffuse_color.a;
    } else if (flags & ALPHA_LAYER2) != 0u {
        // [game 063a70:91-92, 268] cheap_plus_mask_ssd: colour from layer 1 alone; alpha = layer 2's red *
        // Diffuse_Color.a * AlphaBoost (no opacity term).
        albedo = texel.rgb * surface.diffuse_color.rgb;
        alpha = diffuse2_texel.r * surface.diffuse_color.a * surface.ground.w;
    } else if (flags & DIFFUSE2) != 0u {
        // LS-41 [game 0738c0:84-92, 06b4c0:94-99]: a1 = tex1.a * Diffuse_Texture1_Opacity (`light.w`), a2 = tex2.a *
        // Diffuse_Texture2_Opacity (`layer2.x`); colour = lerp(tex1.rgb, tex2.rgb, a2) (`lrp`, unclamped, as the
        // listing); alpha = max(a1, a2) * v4.a * AlphaBoost, with v4.a the diffuse colour's alpha (object colour 1).
        let a1 = texel.a * surface.light.w;
        let a2 = diffuse2_texel.a * surface.layer2.x;
        albedo = mix(texel.rgb, diffuse2_texel.rgb, a2) * surface.diffuse_color.rgb;
        alpha = max(a1, a2) * surface.diffuse_color.a * surface.ground.w;
    } else {
        albedo = texel.rgb * surface.diffuse_color.rgb;
        alpha = texel.a * surface.light.w * surface.diffuse_color.a * surface.ground.w;
    }
#endif
#ifdef VERTEX_COLORS
    // [game] dull_vc: 044aa0_vs / 038900_ps multiply the material, object and
    // vertex colours. The installed skid UV transform is identity [data].
    // [game 044aa0_vs:73, 038900_ps:195] the object's colour c23 is multiplied in (the vertex stage) and
    // the pixel stage reads it too; the port has it in `surface.diffuse_color` (`surface.rs` tint). What
    // c23 holds for a placed Instance: the upload is read [game 00580660:005808dd], the value's writer is not: [assumed] the Colorizer tint (`surface.rs` tint).
    if (flags & VERTEX_COLOR) != 0u {
        albedo *= in.color.rgb;
        alpha *= in.color.a;
    }
#endif

    let power = surface.specular.w;
    let diffuse_light = saturate(dot(n, l)) * light_color * shadow + ambient;
    let specular_light = (power / 30.0 + 1.0) * pow(n_dot_h, power) * shadow * light_color;
    // [game LS-37] the point lights, after the sun and before the fog: their diffuse joins the
    // diffuse light sum and their specular the specular sum, as the sun's do. The level's (flag RIG: none in the
    // arena) and the weapons' muzzle lights (surface.rs FLASH_SLOTS, in the arena too: the game's deferred
    // omni pass lights whatever is in range [assumed: the omni passes are not stencil-masked], so they are not
    // gated on the level's rig).
    var point = PointTerms(vec3<f32>(0.0), vec3<f32>(0.0));
    // [game 0b2a20/0ad870 Base] Flatcards have ambient/boost and the
    // shadowed sun only, with no local-light input in these pixel shaders.
    if (flags & (FLATCARD | UNLIT)) == 0u {
        point = point_sum(in.world_position.xyz, n, v, power, view_bindings::view.world_position, in.position.xy);
    }
    var colour = albedo * (diffuse_light + point.diffuse);
    // [game] the specular term is per technique: `052560:126-140` (cheap), `05cb30:250-252`
    // (cheap_plus); `038900` (dull) and `044aa0` (dull_vc) have none.
    if (flags & SPECULAR) != 0u {
        colour += specular_map * surface.specular.rgb * (specular_light + point.specular);
    }

    let facing = abs(dot(v, n));
    if (flags & EDGE) != 0u {
        colour += pow(saturate(1.0 - facing), surface.edge.w) * light_color * surface.edge.rgb;
    }
    // [game 0675a0_ps:189-200] cheap_plus_fx (flag TRANS_FALLOFF): the alpha times
    // saturate(inTransFalloff + |the view-space normal's xy| * (outTransFalloff - inTransFalloff)), so a layer
    // thins (or thickens) from where it faces the eye to its silhouette. `cutout.y` / `.z` hold the two.
    if (flags & TRANS_FALLOFF) != 0u {
        let nv = (view_bindings::view.view_from_world * vec4<f32>(n, 0.0)).xy;
        alpha *= saturate(surface.cutout.y + length(nv) * (surface.cutout.z - surface.cutout.y));
    }

    if (flags & (REFLECT_CUBE | REFLECT_SPHERE)) != 0u {
        let r = reflect(-to_eye, n);
        var seen: vec3<f32>;
        if (flags & REFLECT_CUBE) != 0u {
            seen = textureSample(reflection_texture, reflection_sampler, game_space(r)).rgb;
        } else {
            // [stand-in] for `RealtimeSphereMap`: sky above, ground below. settles: the pass that fills
            // RealtimeSphereMap (not read; the RealtimeSphereMap paragraph of notes\level-shaders-distortion.md
            // names 005576f0 as the unread lead).
            seen = mix(surface.ground.rgb, surface.sky_seen.rgb, smoothstep(-0.15, 0.25, normalize(r).y));
        }
        var reflected = seen * surface.reflection.rgb * specular_map;
        if (flags & FALLOFF) != 0u {
            reflected *= pow(facing * (surface.reflection.w - 1.0) + 1.0, surface.sky.w);
        }
        colour += saturate(reflected);
    }

    if (flags & UNLIT) != 0u { colour = albedo; }
    // Every texture sample is above this point: a discard cannot precede a derivative op.
    if (flags & CUTOUT) != 0u && alpha < surface.cutout.x {
        discard;
    }

    // [game 0ab3b0_ps:321-329, 05cb30_ps:257-265, 038900_ps:203-212; LS-29] the level's fog is the last
    // step before the colour conversion: colour = lerp(colour, FogColor.rgb, t). The game multiplies by
    // IrisParams.x after it (0ab3b0_ps:329, `mul_pp oC0.xyz, r1, c31.x`). Here the exposure is not in
    // this shader: `postprocess.wgsl` applies iris.x once to the frame. [assumed] that is the same
    // multiply (it is applied after the gamma there, the game's is before the target's clamp).
    // [assumed] the same mix for the additive and blended techniques (their pixel shaders are not read).
    // settles: the additive and blended pixel listings (which names have the ADD flag: surface.rs `ADD`)
    // for the fog lerp. The IrisParams.x writer is found [game 2026-10-20]: 00562c70 stores the run-time iris
    // in DAT_00c02ee0 (see `glow.wgsl` 14); the value itself is a recording question (QUEUED there).
    if (flags & RIG) != 0u {
        var fog_position = in.world_position.xyz;
#ifdef FLATCARD
        fog_position = in.fog_position;
#endif
        let fog_t = fog_factor(distance(view_bindings::view.world_position, fog_position), game_space(fog_position).z);
        colour = mix(colour, surface.fog_color.rgb, fog_t);
    }

    var out: SurfaceOutput;
#ifdef SURFACE_DEPTH_BIAS
    // [game] decal note 7.5: view += normalize(view) * DepthBias * 0.1 in the depth, GBuffer and Base
    // vertex shaders. The same move along the eye ray, done per pixel: DepthBias -1 comes 0.1 m nearer.
    // NARROWED 2026-10-20: for ROBOTS the push is zero (DepthBias word 0 in all robot materials, report-X3
    // RDed3c68). [assumed] view units are metres (the port's world is in metres), for levels only. The format
    // note says distances look like metres (notes\formats.md "Space and units"), not a measured unit. settles: a game length
    // read in the data (a known part size against its mesh) before the 0.1 push of DepthBias is trusted. For robots the
    // DepthBias word is 0 in every material (report-X3, RDed3c68), so the push is zero for them.
    let eye_ray = in.world_position.xyz - view_bindings::view.world_position;
    let moved = in.world_position.xyz + normalize(eye_ray) * surface.depth_bias.x * 0.1;
    let clip = view_bindings::view.clip_from_world * vec4<f32>(moved, 1.0);
    out.depth = clip.z / clip.w;
#endif
    if (flags & ADD) != 0u {
        // Bevy's Add blend is premultiplied (src One, dst OneMinusSrcAlpha): src alpha 0 with the
        // colour times alpha gives dst + colour * alpha, the additive the blend switch sets.
        out.color = vec4<f32>(to_linear(saturate(colour)) * alpha, 0.0);
    } else if (flags & CUTOUT) != 0u {
        // Opaque with a cut-out (AlphaMode::Mask, no blending): the surviving pixel keeps the shader's alpha,
        // as the game's oC0.w = albedo.a * AlphaBoost does; the render target's alpha feeds the bloom's
        // extract (2 * rgb * a, effects 003a80). [assumed: the game's cut-out targets keep that alpha]
        out.color = vec4<f32>(to_linear(saturate(colour)), saturate(alpha));
    } else {
        // [game] AlphaBoost (`ground.w`, 255 / max(X,1) for blendmode 0x3467929c, else 1) scales
        // the alpha; the output is clamped to 1 [assumed]. The listings do not clamp (0ab3b0_ps:330 `mul_pp
        // oC0.w, c43.x, v6.w` has no _sat), so the clamp is the colour target's (a unorm target clamps on
        // write). settles: the format of the colour target the Base pass draws into (its D3D format at its
        // creation, not read here).
        out.color = vec4<f32>(to_linear(saturate(colour)), saturate(alpha));
    }
    return out;
}
#endif

#ifdef PREPASS_PIPELINE
// [game 039ae0] the cut of the depth-only pass: the same texel alpha, opacity, vertex alpha and
// threshold as the colour pass, so the depth of a cut pixel is not written. Its only output is
// the discard; with no normal or motion target the fragment returns nothing.
@fragment
#ifdef CUSTOM_SURFACE_VERTEX
fn fragment(in: SurfaceVertex) {
#else
fn fragment(in: pio::VertexOutput) {
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    lod_coverage(in.instance_index, in.position.xy);
#endif
    let flags = surface.flags.x;
#ifdef VERTEX_UVS_A
    let raw_uv = in.uv;
#else
    let raw_uv = vec2<f32>(0.0);
#endif
    let time = surface.motion.w;
    let uv = transform_uv(surface.uv_x, surface.uv_y, surface.uv_z, surface.uv_t, raw_uv, time);
    let texel = textureSample(diffuse_texture, diffuse_sampler, uv);
    var alpha: f32;
#ifdef BLEND2
    // The cut must use the same layer alpha as the colour pass. Cutting only
    // texture1 would leave invisible depth on mixed grass/soil edges.
    var mask = textureSample(mask_texture, mask_sampler, select(in.uv2, in.uv3, (flags & BLEND3) != 0u)).rgb;
#ifdef VERTEX_COLORS
    if (flags & VERTEX_WEIGHTS) != 0u { mask = in.color.rgb; }
#endif
    let weights = mask / (mask.r + mask.g + select(0.0, mask.b, (flags & BLEND3) != 0u));
    let uv_b = transform_uv(surface.diffuse2_x, surface.diffuse2_y, surface.diffuse2_z, surface.diffuse2_t, in.uv_b, time);
    let uv_c = transform_uv(surface.diffuse3_x, surface.diffuse3_y, surface.diffuse3_z, surface.diffuse3_t, in.uv2, time);
    let b = textureSample(diffuse2_texture, diffuse2_sampler, uv_b).a;
    let c = textureSample(diffuse3_texture, diffuse3_sampler, uv_c).a;
    alpha = (texel.a * weights.r + b * weights.g + select(0.0, c * weights.b, (flags & BLEND3) != 0u))
        * surface.diffuse_color.a * surface.ground.w;
#else
    if (flags & PAINT) != 0u {
        alpha = surface.ground.w * surface.diffuse_color.a;
    } else if (flags & DIFFUSE2) != 0u {
        // LS-41: the same alpha as the colour pass (max of the two layers' alphas), so a cut-out DIFFUSE2 part
        // is cut at the same pixels, using the colour pass's scrolling clock.
#ifdef VERTEX_UVS_B
        let raw_uv_b = in.uv_b;
#else
        let raw_uv_b = vec2<f32>(0.0);
#endif
        let layer2_uv = transform_uv(surface.diffuse2_x, surface.diffuse2_y, surface.diffuse2_z, surface.diffuse2_t, raw_uv_b, time);
        let layer2_texel = textureSample(diffuse2_texture, diffuse2_sampler, layer2_uv);
        if (flags & ALPHA_LAYER2) != 0u {
            // [game 063a70:91-92] cheap_plus_mask_ssd: the colour pass's alpha, layer 2's red.
            alpha = layer2_texel.r * surface.diffuse_color.a * surface.ground.w;
        } else {
            alpha = max(texel.a * surface.light.w, layer2_texel.a * surface.layer2.x) * surface.diffuse_color.a * surface.ground.w;
        }
    } else {
        alpha = texel.a * surface.light.w * surface.diffuse_color.a * surface.ground.w;
    }
#endif
#ifdef VERTEX_COLORS
    if (flags & VERTEX_COLOR) != 0u {
        alpha *= in.color.a;
    }
#endif
    if (flags & CUTOUT) != 0u && alpha < surface.cutout.x {
        discard;
    }
}
#endif
