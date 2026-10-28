// The original's `ocean_water2` (deep_six_01's sea), written again from the compiled shaders in
// `shaders.lsa` (`work\shaders\asm`): vertex 0a0c30, pixel 0a1d90. Decoded line by line in
// `notes\level-shaders-ocean.md` sections 1 and 2; ocean.rs fills the values.
//
// The arithmetic is done in the GAME's space (x right, y forward, z up): the waves, the normal maps and
// the reflection vectors are laid out in it. `game_space` / `bevy_space` convert (as surface.wgsl).
//
// [stand-in] the shadow factor is Bevy's sun shadow (the game: two cascades and its filter), as every
// ported technique. [stand-in] no IrisParams (exposure): the port applies none here, as surface.wgsl.
// The reflection is the port's own mirrored camera (ocean.rs `follow`); without it (flag REFLECTION off:
// `--no-ocean-reflection`, or the eye under the sea) the reflection is black and "nothing drawn" everywhere.

#import bevy_pbr::{
    forward_io::FragmentOutput,
    mesh_view_bindings as view_bindings,
    mesh_functions,
    shadows,
    view_transformations::position_world_to_clip,
}

const RIG: u32 = 1u;
const REFLECTION: u32 = 2u;
// The port's reflection camera is a proper rotation (no mirror), so its picture is the mirror image
// turned left to right: sampled at 1 - u. A port flag, not the game's Ocean_Distortion_Flip_Horiz.
const MIRRORED_X: u32 = 4u;

const TWO_PI: f32 = 6.28318548;
const PI: f32 = 3.14159274;

struct OceanValues {
    // Wave_Period0..3, Wave_Wavelength0..3, Wave_Amp0..3, Wave_Phase0..3 (degrees), Wave_Dir0..3 (degrees),
    // Wave_Enable0..3 [data]
    wave_period: vec4<f32>,
    wave_length: vec4<f32>,
    wave_amp: vec4<f32>,
    wave_phase: vec4<f32>,
    wave_dir: vec4<f32>,
    wave_enable: vec4<f32>,
    // x Stormy, y Time_Scale, z Vertex_Fade_Dist_Softness, w Vertex_Fade_Dist_Thresh
    wave_misc: vec4<f32>,
    // x Wrap_Dist
    wrap: vec4<f32>,
    // TexCoord_X.xy, TexCoord_Y.xy; TexCoord_Z.xy, TexCoord_T.xy
    texcoord_xy: vec4<f32>,
    texcoord_zt: vec4<f32>,
    // per normal map i: (Normal_Map{i}_X.xy, _Y.xy) and (_Z.xy, _T.xy)
    normal_xy: array<vec4<f32>, 4>,
    normal_zt: array<vec4<f32>, 4>,
    // Normal_Map_DXT5_NM0..3, Normal_Map_HeightScale0..3
    dxt5: vec4<f32>,
    height_scale: vec4<f32>,
    // Diffuse_Color * ObjectDiffuseColor (the vertex shader's o2)
    diffuse: vec4<f32>,
    // rgb Specular_Color, w Specular_Power
    specular: vec4<f32>,
    // rgb Reflection_Color, w AlphaBoost
    reflection: vec4<f32>,
    // x Ocean_Fresnel_Max, y _Min, z _Pow, w Snell_Amount
    fresnel: vec4<f32>,
    // Ocean_Use_Diffuse, _Reflection, _Specular, _Water
    use_: vec4<f32>,
    // Ocean_Distortion_Border, _Max, _Aspect_Ratio, _Scale
    distortion: vec4<f32>,
    // x Ocean_Distortion_Flip_Horiz, y _Flip_Vert
    flip: vec4<f32>,
    // Ocean_Show_Fresnel_Mask, _Specular, _Reflection, _Diffuse; show2.x Ocean_Show_Water
    show: vec4<f32>,
    show2: vec4<f32>,
    // mReflectWorldView's two rows (game space), c12 and c13 of 0a1d90
    reflect_view: array<vec4<f32>, 2>,
    // The level's rig in the game's space: LightColor, LightDir, AmbientColor, the nine irradiance terms
    rig_sun: vec4<f32>,
    rig_dir: vec4<f32>,
    rig_ambient: vec4<f32>,
    irradiance: array<vec4<f32>, 9>,
    fog_color: vec4<f32>,
    fog_range: vec4<f32>,
    flags: vec4<u32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> ocean: OceanValues;
// s5..s8 Normal_Map_Texture0..3, s9 Fresnel_Texture, s3 ReflectionMap [game 0a1d90]
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var normal0_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var normal0_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var normal1_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var normal1_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var normal2_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var normal2_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var normal3_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var normal3_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var fresnel_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var fresnel_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var reflection_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var reflection_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
}

struct OceanVertex {
    @builtin(position) position: vec4<f32>,
    // o1: the world position with the wave height, in the game's space
    @location(0) world_game: vec3<f32>,
    // o3, o4: the four normal-map uvs
    @location(1) uv01: vec4<f32>,
    @location(2) uv23: vec4<f32>,
}

// Bevy's space to the game's (z up, y forward), and back (assets::game_to_bevy).
fn game_space(v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(v.x, -v.z, v.y);
}

fn bevy_space(v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(v.x, v.z, -v.y);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.0031308));
}

// [game 0a0c30] the listing's range reduction before each `sincos`: frac(x / 2pi + 0.5) * 2pi - pi.
fn wrap_angle(x: f32) -> f32 {
    return fract(x * 0.159154937 + 0.5) * TWO_PI - PI;
}

// [game 0a0c30] one wave's height: d = (cos, sin) of Wave_Dir (degrees, reduced as `frac(Dir/360 + 0.5)`),
// phase = (W.xy . d) * 2pi / Wavelength + t * 2pi / Period + Phase * pi / 180.
fn wave(i: u32, w: vec2<f32>, t: f32) -> f32 {
    let theta = fract(ocean.wave_dir[i] * 0.00277777552 + 0.5) * TWO_PI - PI;
    let d = vec2<f32>(cos(theta), sin(theta));
    let phase = dot(w, d) * (6.28318024 / ocean.wave_length[i])
        + t * (6.28318024 / ocean.wave_period[i])
        + ocean.wave_phase[i] * 0.0174532775;
    return ocean.wave_enable[i] * ocean.wave_amp[i] * sin(wrap_angle(phase));
}

@vertex
fn vertex(v: Vertex) -> OceanVertex {
    var out: OceanVertex;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    // W = mLocalToWorld * v0, in the game's space.
    let w = game_space(world.xyz);
    let time = view_bindings::globals.time;
    // t = Time * Time_Scale
    let t = time * ocean.wave_misc.y;
    // [game] the listing sums wave 1 first, then 0, 2, 3.
    var h = wave(1u, w.xy, t);
    h += wave(0u, w.xy, t);
    h += wave(2u, w.xy, t);
    h += wave(3u, w.xy, t);
    // Stormy: h' = h + Stormy * (1 - 2|h| - h)
    h += ocean.wave_misc.x * (1.0 - 2.0 * abs(h) - h);
    // The fade with distance: the eye's z is compared with 0, not with W.z [game: `mov r4.w, c65.z`].
    let s = max(ocean.wave_misc.z, 1e-7);
    let cam = game_space(view_bindings::view.world_position);
    let dist = length(vec3<f32>(w.x - cam.x, w.y - cam.y, 0.0 - cam.z));
    let fade = saturate(0.5 * (s + ocean.wave_misc.w - dist) / s);
    let p = vec3<f32>(w.x, w.y, w.z + h * fade);
    out.world_game = p;
    out.position = position_world_to_clip(bevy_space(p));
    // uv = TexCoord_X * (W.x / Wrap) + TexCoord_Y * (W.y / Wrap) + TexCoord_Z + TexCoord_T * Time (raw Time)
    let uv0 = w.xy / ocean.wrap.x;
    let uv = ocean.texcoord_xy.xy * uv0.x + ocean.texcoord_xy.zw * uv0.y + ocean.texcoord_zt.xy + ocean.texcoord_zt.zw * time;
    var maps: array<vec2<f32>, 4>;
    for (var i = 0u; i < 4u; i++) {
        maps[i] = ocean.normal_xy[i].xy * uv.x + ocean.normal_xy[i].zw * uv.y + ocean.normal_zt[i].xy + ocean.normal_zt[i].zw * time;
    }
    out.uv01 = vec4<f32>(maps[0], maps[1]);
    out.uv23 = vec4<f32>(maps[2], maps[3]);
    return out;
}

// [game 0a1d90] one normal map's xy: 2 tex.xy - 1, or (2 tex.a - 1, 2 tex.g - 1) when its DXT5_NM is > 0.
fn normal_xy(tex: vec4<f32>, dxt5: f32) -> vec2<f32> {
    let m = tex * 2.0 - 1.0;
    return select(m.xy, vec2<f32>(m.w, m.y), dxt5 > 0.0);
}

// [game 0382c0_vs:73-89] the ambient irradiance at n (game space), as surface.wgsl `irradiance`.
fn irradiance(n: vec3<f32>) -> vec3<f32> {
    let k = ocean.irradiance;
    let e = k[0] * (n.x * n.x) + k[1] * (n.y * n.y) + k[2] * (n.z * n.z)
        + k[3] * (n.x * n.y) + k[4] * (n.y * n.z) + k[5] * (n.z * n.x)
        + k[6] * n.x + k[7] * n.y + k[8] * n.z;
    return e.rgb;
}

@fragment
fn fragment(in: OceanVertex) -> FragmentOutput {
    let flags = ocean.flags.x;
    let p = in.world_game;
    let cam = game_space(view_bindings::view.world_position);

    // 1. The shadow factor S. [stand-in] Bevy's sun shadow, at the displaced position.
    var shadow = 1.0;
    let bevy_p = vec4<f32>(bevy_space(p), 1.0);
    if view_bindings::lights.n_directional_lights > 0u {
        let sun = &view_bindings::lights.directional_lights[0];
        if ((*sun).flags & 1u) != 0u {
            let view_z = dot(vec4<f32>(
                view_bindings::view.view_from_world[0].z,
                view_bindings::view.view_from_world[1].z,
                view_bindings::view.view_from_world[2].z,
                view_bindings::view.view_from_world[3].z
            ), bevy_p);
            shadow = shadows::fetch_directional_shadow(0u, bevy_p, vec3<f32>(0.0, 1.0, 0.0), view_z, in.position.xy);
        }
    }
    // LightDir and LightColor: the level's rig (game space). [stand-in] without a rig (the arena) a light
    // from straight up, black.
    var l = vec3<f32>(0.0, 0.0, 1.0);
    var light_color = vec3<f32>(0.0);
    var ambient = vec3<f32>(0.0);
    if (flags & RIG) != 0u {
        l = ocean.rig_dir.xyz;
        light_color = ocean.rig_sun.rgb;
    }

    // 2. V = normalize(cam - P); H = normalize(V + LightDir).
    let v = normalize(cam - p);
    let h = normalize(v + l);

    // 3. The normal: the four maps' xy times their height scales, summed in the order 1, 0, 2, 3;
    // N = normalize(n.x, n.y, 1) in world space (the sea is flat: tangent space = world, z up).
    var n2 = normal_xy(textureSample(normal1_texture, normal1_sampler, in.uv01.zw), ocean.dxt5.y) * ocean.height_scale.y;
    n2 += normal_xy(textureSample(normal0_texture, normal0_sampler, in.uv01.xy), ocean.dxt5.x) * ocean.height_scale.x;
    n2 += normal_xy(textureSample(normal2_texture, normal2_sampler, in.uv23.xy), ocean.dxt5.z) * ocean.height_scale.z;
    n2 += normal_xy(textureSample(normal3_texture, normal3_sampler, in.uv23.zw), ocean.dxt5.w) * ocean.height_scale.w;
    let n = normalize(vec3<f32>(n2, 1.0));

    // 4. spec = pow(sat(N.H), Power) * (Power / 30 + 1) * S * LightColor * Specular_Color
    let power = ocean.specular.w;
    let spec = pow(saturate(dot(n, h)), power) * (power * 0.0333333351 + 1.0) * shadow * light_color * ocean.specular.rgb;

    // 5. I = normalize(P - cam), invd = 1 / |P - cam|; R = reflect(I, N), R_flat = (I.x, I.y, -I.z);
    // off = ((R - R_flat) . mReflectWorldView[0], (R - R_flat) . mReflectWorldView[1]) * invd * Scale.
    let to_p = p - cam;
    let d = length(to_p);
    let invd = 1.0 / d;
    let i = to_p * invd;
    let r = i - 2.0 * dot(i, n) * n;
    let r_flat = vec3<f32>(i.x, i.y, -i.z);
    var off = vec2<f32>(dot(r - r_flat, ocean.reflect_view[0].xyz), dot(r - r_flat, ocean.reflect_view[1].xyz))
        * invd * ocean.distortion.w;

    // 6. The screen uv (vPos * VposParams.xy + VposParams.zw); the offset fades to 0 at the screen's edge
    // over Ocean_Distortion_Border, is flipped by the Flip switches, cut to Ocean_Distortion_Max, x times the
    // aspect ratio, and shortened so that the end stays inside [0, 1].
    let uv = (in.position.xy - view_bindings::view.viewport.xy) / view_bindings::view.viewport.zw;
    let edge = abs(uv - 0.5) * 2.0;
    let m = max(edge.x, edge.y);
    off *= 1.0 - saturate(1.0 + (m - 1.0) / ocean.distortion.x);
    off = vec2<f32>(select(off.x, -off.x, ocean.flip.x > 0.0), select(off.y, -off.y, ocean.flip.y > 0.0));
    let len = length(off);
    if len > ocean.distortion.y {
        off *= ocean.distortion.y / len;
    }
    off.x *= ocean.distortion.z;
    // [assumed] a zero component (0/0 in the listing's rcp) does not limit k.
    let reach = saturate(uv + off) - uv;
    let kx = select(1e30, reach.x / off.x, off.x != 0.0);
    let ky = select(1e30, reach.y / off.y, off.y != 0.0);
    let k = min(min(kx, ky), 1.0);
    let uv_r = uv + off * k;

    // 7. Rc = (depth == 0 ? refl + spec * Ocean_Use_Specular : refl) * Reflection_Color: the specular shows only
    // where the reflection pass drew nothing.
    var refl = vec3<f32>(0.0);
    var drawn = false;
    if (flags & REFLECTION) != 0u {
        var at = uv_r;
        if (flags & MIRRORED_X) != 0u {
            at.x = 1.0 - at.x;
        }
        let texel = textureSampleLevel(reflection_texture, reflection_sampler, at, 0.0);
        // The port's picture is an sRGB target holding `to_linear` of the level's colours (as the screen does);
        // the game's arithmetic is on the colours before that, so they are encoded back.
        refl = to_srgb(texel.rgb);
        // [assumed] "nothing drawn" (the game's ReflectionDepthMap == 0) is the reflection image's alpha 0.
        drawn = texel.a > 0.0;
    }
    let spec_used = spec * ocean.use_.z;
    let rc = select(refl + spec_used, refl, drawn) * ocean.reflection.rgb;

    // 8. Refraction T = lerp(I, -N, Snell_Amount); F = lerp(Fresnel_Min, Fresnel_Max, pow(1 - sat(I . -N), Pow));
    // the water colour Wc = Fresnel_Texture(u = sat(-T.z / |T|), v = 0).
    let refr = mix(i, -n, ocean.fresnel.w);
    let f = pow(1.0 - saturate(dot(i, -n)), ocean.fresnel.z);
    let fresnel = mix(ocean.fresnel.y, ocean.fresnel.x, f);
    let size = vec2<f32>(textureDimensions(fresnel_texture, 0));
    // [assumed] the game's sampler clamps Fresnel_Texture: the lookup is kept on the texel centres (a 1D table
    // by angle has no meaning wrapped round, and the port's images repeat).
    let fu = clamp(saturate(-refr.z / length(refr)), 0.5 / size.x, 1.0 - 0.5 / size.x);
    let water = textureSampleLevel(fresnel_texture, fresnel_sampler, vec2<f32>(fu, 0.5 / size.y), 0.0).rgb;

    // 9. E = 2 AmbientMap(N) + AmbientColor - 1, which the port takes as E9(N) + AmbientColor (surface.wgsl);
    // D = (sat(N . LightDir) * LightColor * S + E) * colour * Ocean_Use_Diffuse.
    if (flags & RIG) != 0u {
        ambient = irradiance(n) + ocean.rig_ambient.rgb;
    }
    let diffuse = (saturate(dot(n, l)) * light_color * shadow + ambient) * ocean.diffuse.rgb * ocean.use_.x;
    let water_used = water * ocean.use_.w;
    let base = water_used + diffuse;
    let rc_used = rc * ocean.use_.y;
    var colour = mix(base, rc_used, fresnel);

    // 10. The debug switches, in the listing's order.
    if ocean.show.x > 0.0 { colour = vec3<f32>(fresnel); }
    if ocean.show.y > 0.0 { colour = spec_used; }
    if ocean.show.z > 0.0 { colour = rc_used; }
    if ocean.show.w > 0.0 { colour = diffuse; }
    if ocean.show2.x > 0.0 { colour = water_used; }

    // 11. Fog: t = sat((|P - cam| - FogRange.x) * FogRange.y) * FogColor.w * sat(1 - (P.z / FogRange.w)^2).
    if (flags & RIG) != 0u {
        let z = p.z / ocean.fog_range.w;
        let t = saturate((d - ocean.fog_range.x) * ocean.fog_range.y) * ocean.fog_color.w * saturate(1.0 - z * z);
        colour = mix(colour, ocean.fog_color.rgb, t);
    }

    var out: FragmentOutput;
    // Alpha = AlphaBoost * the colour's alpha. The colour is converted as surface.wgsl does (`to_linear`).
    out.color = vec4<f32>(to_linear(saturate(colour)), saturate(ocean.reflection.w * ocean.diffuse.a));
    return out;
}
