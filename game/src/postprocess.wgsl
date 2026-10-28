// Colour LUT composition: executable722b38/00547190. Final sampling:
// effects.lsa006600 (32 slices in a1024x32 texture, linear RG and manual blue lerp).
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
struct Settings {
    smh: array<vec4<f32>,3>,
    tint: vec4<f32>,
    params: vec4<f32>,
    fade: vec4<f32>,
    flags: vec4<u32>,
    // x IrisParams.x (the iris), y irisLumPower (`LuminancePower` of the luminance pass).
    iris: vec4<f32>,
    // BloomLevel (threshold, 1/threshold, saturation, on) and Brightness (rgb) [game 00546350, 003a80].
    bloom_level: vec4<f32>,
    bloom_brightness: vec4<f32>,
    // Gaussian13 [game 005839a0]: xy offset in source texels, z weight (normalised, times the scale 1.1).
    taps: array<vec4<f32>, 13>,
    // DOFParams (farFocus, nearFocus - nearFalloff, 1/farFalloff, 1/nearFalloff) [game 00551670].
    dof: vec4<f32>,
    // x MaxBlur, y dofKernelTap, z near plane (0.5), w 1 when the soft DOF runs.
    dof2: vec4<f32>,
};
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear_clamp: sampler;
@group(0) @binding(2) var lut: texture_2d<f32>;
@group(0) @binding(3) var<uniform> settings: Settings;
// The bloom (eighth size) for the composite; another pass's unused input elsewhere.
@group(0) @binding(4) var bloom: texture_2d<f32>;
#ifdef MULTISAMPLED
@group(0) @binding(5) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(5) var depth: texture_depth_2d;
#endif

// [game effects 003630, set up by 00551670's soft branch] The depth of field: a five-tap blur (weights 0.2: the centre
// and (-k/2, k), (k, k/2), (k/2, -k), (-k, -k/2) texels, k = dofKernelTap) mixed in by max(nearCoC, farCoC) * MaxBlur,
// farCoC = sat((z - farFocus) / farFalloff), nearCoC = sat(1 - (z - (nearFocus - nearFalloff)) / nearFalloff).
// [assumed] the mix is done on the port's linear scene, not the game's stored values.
@fragment
fn dof(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSampleLevel(source, linear_clamp, in.uv, 0.0);
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    let k = settings.dof2.y;
    let h = 0.5 * k;
    let offsets = array<vec2<f32>, 4>(vec2<f32>(-h, k), vec2<f32>(k, h), vec2<f32>(h, -k), vec2<f32>(-k, -h));
    var blur = scene * 0.2;
    for (var i = 0u; i < 4u; i++) {
        blur += textureSampleLevel(source, linear_clamp, in.uv + offsets[i] * texel, 0.0) * 0.2;
    }
    let size = vec2<f32>(textureDimensions(depth));
    let d = textureLoad(depth, vec2<i32>(min(in.uv * size, size - 1.0)), 0);
    // Bevy's reversed infinite depth: z = near / d. The cleared depth (the sky) is the game camera's far, 1500.
    let z = select(1500.0, settings.dof2.z / d, d > 0.0);
    let far_coc = saturate((z - settings.dof.x) * settings.dof.z);
    let near_coc = saturate(1.0 - (z - settings.dof.y) * settings.dof.w);
    return mix(scene, blur, max(near_coc, far_coc) * settings.dof2.x);
}

fn colour_filter(original: vec3<f32>) -> vec3<f32> {
    var c = saturate((original - 0.5) * settings.params.z + 0.5);
    c = c * settings.params.x + settings.params.w - 1.0;
    let grey = dot(c, vec3<f32>(0.333333313));
    let shadows = saturate(1.0 - 2.0*grey);
    let highlights = saturate(2.0*grey - 1.0);
    let midtones = 1.0 - shadows - highlights;
    let coefficient = (settings.smh[0]*shadows + settings.smh[1]*midtones + settings.smh[2]*highlights) * settings.tint;
    var changed = grey + (c * coefficient.rgb - grey) * coefficient.w * coefficient.w;
    // [assumed] Degenerate black stays black; native rsq0/mul0 depends on D3D hardware.
    let length_squared = dot(changed, changed);
    if length_squared > 0.0 {
        changed = saturate(changed * sqrt(dot(c,c)/length_squared) * settings.params.w);
    } else { changed = vec3<f32>(0.0); }
    return mix(changed, settings.fade.rgb, settings.fade.w);
}

@fragment
fn compose_lut(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<u32>(in.position.xy);
    var c = vec3<f32>(f32(p.x % 32u), f32(p.y), f32(p.x / 32u)) / 31.0;
    if settings.flags.x != 0u { c = textureSampleLevel(source, linear_clamp, in.uv, 0.0).rgb; }
    return vec4<f32>(colour_filter(c), 0.0);
}

// The scene as the game's 8-bit render target holds it: every level shader multiplies its output by
// IrisParams.x [game, e.g. 0b5de0, 094420, 0b7530] and the target clamps it. [assumed] One multiply here, on
// the stored values, stands for all of them (equal for opaque and for blending; Bevy's bloom is before it).
fn exposed(linear: vec3<f32>) -> vec3<f32> {
    return saturate(pow(max(linear, vec3<f32>(0.0)), vec3<f32>(1.0/2.2)) * settings.iris.x);
}

// The iris's luminance target, 4x4 [game 00562c70, 00583420; executable shader 0x71e740]: each texel is the
// power mean (LuminancePower p) of the exposed scene's RED channel over its block, (mean r^p)^(1/p).
// [assumed] One pass of 16x16 point taps per block stands for the game's chain of 16-tap passes
// (64x64, 16x16, 4x4); the power means compose, the 8-bit rounding between stages is not reproduced.
@fragment
fn luminance(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let cell = floor(in.position.xy);
    let p = max(settings.iris.y, 0.01);
    var sum = 0.0;
    for (var j = 0u; j < 16u; j++) {
        for (var i = 0u; i < 16u; i++) {
            let uv = (cell + (vec2<f32>(f32(i), f32(j)) + 0.5) / 16.0) / 4.0;
            let r = exposed(textureSampleLevel(source, linear_clamp, uv, 0.0).rgb).r;
            sum += select(0.0, pow(r, p), r > 0.0);
        }
    }
    let mean = pow(sum / 256.0, 1.0 / p);
    return vec4<f32>(mean, mean, mean, 1.0);
}

// [game effects 003a80, executable 724450] The bloom's hot extract, from the scene to a quarter-size target:
// c = rgb * 2a; c = grey + BloomLevel.z * (c - grey); out = sat(c - (1 - BloomLevel.x)) * BloomLevel.y * Brightness.
// [assumed] the scene's alpha is what the port's materials write (the game's render target alpha).
@fragment
fn bloom_extract(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSampleLevel(source, linear_clamp, in.uv, 0.0);
    let c = exposed(scene.rgb) * 2.0 * saturate(scene.a);
    let grey = dot(c, vec3<f32>(0.33333));
    let s = grey + settings.bloom_level.z * (c - grey);
    let hot = saturate(s - (1.0 - settings.bloom_level.x)) * settings.bloom_level.y * settings.bloom_brightness.rgb;
    return vec4<f32>(saturate(hot), 1.0);
}

// A plain bilinear copy (quarter to eighth) [game 00582b60 / 00582d70, filter_copy: linear, clamp].
@fragment
fn bloom_copy(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return textureSampleLevel(source, linear_clamp, in.uv, 0.0);
}

// [game 005839a0 Gaussian13, executable shader 0x725998] 13 bilinear taps, offsets in source texels.
@fragment
fn bloom_blur(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    var sum = vec4<f32>(0.0);
    for (var i = 0u; i < 13u; i++) {
        let tap = settings.taps[i];
        sum += textureSampleLevel(source, linear_clamp, in.uv + tap.xy * texel, 0.0) * tap.z;
    }
    return saturate(sum);
}

@fragment
fn apply_lut(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSampleLevel(source, linear_clamp, in.uv, 0.0);
    // [assumed] Inverse of Surface's stored-value -> linear2.2 conversion before
    // the original LUT; Bevy's scene domain is not the native D3D render target.
    var c = exposed(scene.rgb);
    // [game effects 006fd0] c = sat(scene + 1.5 * bloom) before the LUT.
    if settings.bloom_level.w > 0.0 {
        c = saturate(c + 1.5 * textureSampleLevel(bloom, linear_clamp, in.uv, 0.0).rgb);
    }
    let blue = c.b * 31.0;
    let rg = c.rg * 31.0 + 0.5;
    let uv0 = vec2<f32>((rg.x + 32.0*floor(blue))/1024.0, rg.y/32.0);
    let uv1 = vec2<f32>((rg.x + 32.0*ceil(blue))/1024.0, rg.y/32.0);
    let a = textureSampleLevel(lut, linear_clamp, uv0, 0.0).rgb;
    let b = textureSampleLevel(lut, linear_clamp, uv1, 0.0).rgb;
    return vec4<f32>(pow(max(mix(a,b,fract(blue)), vec3<f32>(0.0)), vec3<f32>(2.2)), scene.a);
}
