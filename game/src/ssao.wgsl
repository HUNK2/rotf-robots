// The level's ambient occlusion, as the game's deferred path makes it (`ssao.rs`).
// [game] effects.lsa 003ce0 (the SSAO pass, constants from 0059d3f0) and 005090 (the bilateral blur, run twice).
// Output: x the code of the normal's octant (the blur's edge test), y the occlusion (the composite reads .g).
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Params {
    view_from_clip: mat4x4<f32>,
    world_from_view: mat4x4<f32>,
    // x near, y far (the game camera's, 0.5 / 1500), z SSAOSampleRange, w SSAOSampleRangeFar
    camera: vec4<f32>,
    // xy the depth texture's size in texels; zw this pass's uv step of one blur tap (0 for the SSAO pass)
    size: vec4<f32>,
};
@group(0) @binding(0) var<uniform> p: Params;
#ifdef MULTISAMPLED
@group(0) @binding(1) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(1) var depth: texture_depth_2d;
#endif
@group(0) @binding(2) var source: texture_2d<f32>;

fn depth_at(uv: vec2<f32>) -> f32 {
    // [game] point sampling, clamped (0059d3f0: sampler states 3,3,1,1,1).
    let texel = vec2<i32>(clamp(uv, vec2<f32>(0.0), vec2<f32>(0.99999)) * p.size.xy);
    return textureLoad(depth, texel, 0);
}

// The view-space position at a uv (Bevy's view: x right, y up, looking down -z).
fn view_at(uv: vec2<f32>) -> vec3<f32> {
    let d = max(depth_at(uv), 1e-7);
    let h = p.view_from_clip * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    return h.xyz / h.w;
}

@fragment
fn ssao(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    if depth_at(uv) <= 0.0 {
        return vec4<f32>(0.0); // the sky: nothing in front, no occlusion
    }
    let centre = view_at(uv);
    let z = -centre.z;
    // [game 003ce0] t = sat(d * 3.378), d the D3D depth of this pixel (DepthConversionParams (3.378, 0)); the
    // tap radius in uv is lerp(SSAOSampleRangeFar, SSAOSampleRange, t).
    let near = p.camera.x;
    let far = p.camera.y;
    let d3d = far / (far - near) * (1.0 - near / z);
    let t = saturate(d3d * 3.378378);
    let radius = mix(p.camera.w, p.camera.z, t);
    // [assumed] The normal is rebuilt from the depth (the game reads the G-buffer's mapped normal): the
    // flatter of the two neighbours on each axis.
    let step = 1.0 / p.size.xy;
    let px = view_at(uv + vec2<f32>(step.x, 0.0)) - centre;
    let nx = centre - view_at(uv - vec2<f32>(step.x, 0.0));
    let py = view_at(uv + vec2<f32>(0.0, step.y)) - centre;
    let ny = centre - view_at(uv - vec2<f32>(0.0, step.y));
    let dx = select(nx, px, abs(px.z) < abs(nx.z));
    let dy = select(ny, py, abs(py.z) < abs(ny.z));
    var n = normalize(cross(dy, dx));
    // [game 003ce0] the normal is turned to face the camera (`mov r5.w, -r5_abs.z`: D3D's view looks down +z,
    // Bevy's down -z, so here z is made positive).
    n = normalize(vec3<f32>(n.x, n.y, abs(n.z)));
    // [game] the code: the world normal's octant, 0.5 x + 0.25 y + 0.125 z for the positive components.
    let world_n = (p.world_from_view * vec4<f32>(n, 0.0)).xyz;
    let code = dot(select(vec3<f32>(0.0), vec3<f32>(1.0), world_n > vec3<f32>(0.0)), vec3<f32>(0.5, 0.25, 0.125));
    // [game 003ce0] eight taps at radius * these offsets (in uv).
    let taps = array<vec2<f32>, 8>(vec2<f32>(0.0, 0.9), vec2<f32>(0.5, 0.0), vec2<f32>(-0.5, 0.0), vec2<f32>(0.0, -0.9),
        vec2<f32>(1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(-1.0, -1.0));
    var occlusion = 0.0;
    var weights = 0.0;
    for (var i = 0u; i < 8u; i++) {
        let s = view_at(uv + taps[i] * radius);
        let v = s - centre;
        // [game] w = sat(DepthalM * (z_centre - z_tap) + DepthalB), DepthalM -0.125, DepthalB 0.5 (0059d3f0).
        let w = saturate(-0.125 * (z - (-s.z)) + 0.5);
        occlusion += max(dot(n, normalize(v)), 0.0) * w;
        weights += w;
    }
    // [game] y = sat(occlusion / weights * SSAOOccluderScale + SSAOBias), OccluderScale 1, Bias 0 (0059d3f0).
    let ao = select(0.0, saturate(occlusion / weights), weights > 0.0);
    return vec4<f32>(code, ao, 0.0, 0.0);
}

// [game 005090] The blur: the centre and six taps at -3..3 steps of 1.845 screen pixels (0059d3f0's Offset01,
// Offset24, Offset56); a tap counts only when its octant code equals the centre's. Run across, then down.
@fragment
fn blur(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let centre = textureSampleLevel(source, source_sampler, in.uv, 0.0);
    var sum = centre.y;
    var count = 1.0;
    for (var k = -3; k <= 3; k++) {
        if k == 0 { continue; }
        let tap = textureSampleLevel(source, source_sampler, in.uv + p.size.zw * f32(k), 0.0);
        if abs(tap.x - centre.x) < 0.5 / 255.0 {
            sum += tap.y;
            count += 1.0;
        }
    }
    return vec4<f32>(centre.x, saturate(sum / count), 0.0, 0.0);
}

@group(0) @binding(3) var source_sampler: sampler;
