// The original's `distortion_vc_emissive` (LS-47, `notes\level-shaders-distortion.md`): the
// invulnerability egg and the other sphere effects on the robots. Its vertex shader is 0b5740 and
// its pixel shader 0b5de0 in `shaders.lsa` (`work\lsa_disasm.py`); the arithmetic below follows the
// listings step by step (read 2026-10-20, the pixel shader's order of operations).
//
// [game 0b5de0_ps] The sphere shows the scene bent by its normal map. The normal n is the normal
// map's (DXT5-NM or not, scaled by Normal_Map_HeightScale) in the tangent frame T, B, N. The mask
// (Diffuse_Texture2, red) times Distortion_Distance is the length of n pushed into the scene; the
// point is moved in view x and y only (the listing drops the view z of the push), projected, and the
// picture is read at the bent point where the scene there is behind the bent point, else at the
// undistorted pixel. The colour is lerped to the fog, and the opacity runs from inTransFalloff at the
// middle of the sphere to outTransFalloff at its rim (the view length of n's x and y).
//
// [stand-in] The picture is Bevy's copy of the scene taken before the translucent things (as
// `warp.wgsl`); the game's RealtimeSphereMap is not read (LS-47 section 6).
// [stand-in] IrisParams is not applied here (as `glow.wgsl` line 14, which owns it): postprocess.wgsl applies the iris to the whole frame.
// B is cross(N, T) times the tangent's sign (the rebuild in `assets.rs` mesh), where the game's 0b5740
// reads the pack's binormal stream (dcl_binormal v3). For these parts the two are the same [data,
// `core\examples\distortion_binormal_check.rs`, 2026-10-20]: the 39 distortion parts of the 13 robots
// (3 each) all have a stream, and its 8060 vertices match the rebuild exactly (largest difference 0).
// [assumed] Time is Bevy's `globals.time` (as `surface.wgsl`'s `transform_uv`, which owns the question; the hash
// 0xcfa6377b is in no immediate of the code, 2026-10-20, and the text "Time" is in the executable, see there).
// settles: the same writer as `surface.wgsl` (c21), then its rate.
// [data 2026-10-20] DepthBias is 0 in all 66 robot materials, so 0b5740's push of the vertex by
// 0.1 * DepthBias along the view ray is zero and is not ported.

#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings as view_bindings,
}
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils
#endif

struct DistortionValues {
    // Normal_Map_X.xy and Normal_Map_Y.xy (the uv set 0 scale, x across and y down the map)
    normal_xy: vec4<f32>,
    // Normal_Map_Z.xy (offset) and Normal_Map_T.xy (scroll per second)
    normal_zt: vec4<f32>,
    // Diffuse_Texture2_X.xy and Diffuse_Texture2_Y.xy (the uv set 1 scale)
    mask_xy: vec4<f32>,
    // Diffuse_Texture2_Z.xy and Diffuse_Texture2_T.xy
    mask_zt: vec4<f32>,
    // Diffuse_Color * ObjectDiffuseColor (the vertex colour is multiplied in as v6)
    colour: vec4<f32>,
    // x Distortion_Distance, y Normal_Map_HeightScale, z Normal_Map_DXT5_NM (1 or 0), w AlphaBoost
    params: vec4<f32>,
    // x inTransFalloff, y outTransFalloff
    falloff: vec4<f32>,
    // rgb FogColor, a the fog strength
    fog_colour: vec4<f32>,
    // x near, y 1 / (far - near), z unused, w the height scale
    fog_range: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> values: DistortionValues;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var normal_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var normal_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var mask: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var mask_sampler: sampler;

// [game 0b5740_vs] uv = X * u + Y * v + Z + T * Time, with the set's own coordinates.
fn distortion_uv(uv: vec2<f32>, xy: vec4<f32>, zt: vec4<f32>, time: f32) -> vec2<f32> {
    return xy.xy * uv.x + xy.zw * uv.y + zt.xy + zt.zw * time;
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let time = view_bindings::globals.time;
    let p = in.world_position.xyz;
    let eye = view_bindings::view.world_from_view[3].xyz;

    // Fog: f = sat((|P - eye| - near) / (far - near)) * FogColor.a * sat(1 - (h / FogRange.w)^2),
    // h the height. The game's z is up; the port's world is y up (as `glow.wgsl`).
    let distance_fog = saturate((length(p - eye) - values.fog_range.x) * values.fog_range.y) * values.fog_colour.a;
    let h = p.y / max(abs(values.fog_range.w), 1e-6);
    let fog = distance_fog * saturate(1.0 - h * h);

    // The normal map. The listing takes x from red (or from alpha for DXT5-NM) and y from green,
    // then z = sqrt(|x * x + y * y - 1|) from the unscaled x and y, and only then scales x and y by
    // Normal_Map_HeightScale.
    let texel = textureSample(normal_map, normal_sampler, distortion_uv(in.uv, values.normal_xy, values.normal_zt, time));
    let raw_x = select(texel.r, texel.a, values.params.z != 0.0) * 2.0 - 1.0;
    let raw_y = texel.g * 2.0 - 1.0;
    let nz = sqrt(abs(raw_x * raw_x + raw_y * raw_y - 1.0));
    let nx = raw_x * values.params.y;
    let ny = raw_y * values.params.y;

    // n = normalize(B * nx + T * ny + N * nz), with the tangent frame of the vertex.
    let normal = normalize(in.world_normal);
    let tangent = normalize(in.world_tangent.xyz);
    let binormal = normalize(cross(normal, tangent) * in.world_tangent.w);
    let n = normalize(binormal * nx + tangent * ny + normal * nz);

    // The mask (Diffuse_Texture2, red) and the push: offset = n * mask.r * Distortion_Distance.
    let mask_r = textureSample(mask, mask_sampler, distortion_uv(in.uv_b, values.mask_xy, values.mask_zt, time)).r;
    let offset = n * (mask_r * values.params.x);

    // The bent point: its view position plus the offset's view x and y (the listing's `mov r0.w, c22.z`
    // drops the view z of the offset), projected by the view's projection.
    let view_p = (view_bindings::view.view_from_world * vec4<f32>(p, 1.0)).xyz;
    let view_offset = (view_bindings::view.view_from_world * vec4<f32>(offset, 0.0)).xyz;
    let bent_view = vec3<f32>(view_p.x + view_offset.x, view_p.y + view_offset.y, view_p.z);
    let bent_clip = view_bindings::view.clip_from_view * vec4<f32>(bent_view, 1.0);
    let bent_uv = bent_clip.xy / bent_clip.w * vec2<f32>(0.5, -0.5) + 0.5;
    // Bevy's depth is reversed (1 near): the game's z_d is 1 - this.
    let bent_depth = bent_clip.z / bent_clip.w;

    // The depth test. The listing takes the scene at the bent point when DepthMap - z_d >= 0 (the
    // scene there is farther than the bent point); in reversed depth that is bent - scene >= 0.
#ifdef DEPTH_PREPASS
    let scene_depth = prepass_utils::prepass_depth(vec4<f32>(bent_uv * view_bindings::view.viewport.zw, 0.0, 0.0), 0u);
    let behind = bent_depth - scene_depth >= 0.0;
#else
    // A view with no depth prepass takes the bent picture always [port]. The game's one camera has
    // `DepthPrepass` (main.rs `spawn_camera`), so the robots' view always runs the test above.
    let behind = true;
#endif

    // The picture: the bent point, or the undistorted pixel (VposParams * pixel, no offset).
    let here_uv = (in.position.xy - view_bindings::view.viewport.xy) / view_bindings::view.viewport.zw;
    let undistorted = textureSampleLevel(view_bindings::view_transmission_texture, view_bindings::view_transmission_sampler, here_uv, 0.0).rgb;
    let bent = textureSampleLevel(view_bindings::view_transmission_texture, view_bindings::view_transmission_sampler, bent_uv, 0.0).rgb;
    let scene = select(undistorted, bent, behind);

    // Colour: lerp(tint * scene, FogColor, f), tint = Diffuse_Color * ObjectDiffuseColor * vertex colour.
    let tint = values.colour * in.color;
    let rgb = mix(tint.rgb * scene, values.fog_colour.rgb, fog);

    // Opacity: the view length of n's x and y runs inTransFalloff -> outTransFalloff, times the tint's
    // alpha and AlphaBoost.
    let n_view = (view_bindings::view.view_from_world * vec4<f32>(n, 0.0)).xy;
    let rim = length(n_view);
    let opacity = saturate(values.falloff.x + (values.falloff.y - values.falloff.x) * rim) * tint.a * values.params.w;

    var out: FragmentOutput;
    out.color = vec4<f32>(rgb, opacity);
    return out;
}
