// The original's distortion shaders (vertex 731560, pixel 731810, compiled into the
// executable): the particle renderer `Dstr` and any renderer with `IsShimmer`. The heat
// haze over the turbo, a muzzle flash, an explosion.
//
// [game]: the renderer's picture is a map: red and green say which way to push (0.5 is
// no push), alpha how much. With f = the particle's alpha faded by depth,
//   push  = (0.5 - map.rg) x map.a x f
//   place = the pixel's place in the world + A x push.x + B x push.y,
//           A = left x n.y - up x n.x,  B = left x n.y + up x n.x
// (left and up the camera's, n two numbers on the vertex), and the pixel is the picture
// of the scene where that place lands on the screen, blended in with alpha map.a x f.
// The game's vertex 731560_vs (work\shaders\exe) takes its two numbers from the normal's x and y:
// A = left x n.y - up x n.x, B = left x n.y + up x n.x (o3 and o4). The port carries n.x and n.y in the
// colour's red and green (`effects.rs` quads; a normal would be renormalised by Bevy). [assumed] both are
// the particle's radius, as `effects.rs` quads. [game 2026-10-20] The lit quad builder 00971170 writes four corner normals with
// `FUN_00967380`: four constant vectors (x = +-0.70710677 (3f3504f2), y and z = 0.5 (3f000000) or -0.5
// (.rdata 00bef464 = bf000000, read with tools\pe_read.py)), each run through `FUN_00965fd0`, a 3x3 product
// with a basis built from the camera (`aps_camera_vectors` 00a9e0c0 / `mat3_mul_vec3` 005866f0). So the
// lit corners are NOT a radius: their x and y differ per corner. The Dstr vertex's n.x and n.y would be
// those components if the Dstr renderer uses this builder, which is not shown (the BlendMode 3 rule in
// 00acdc00 is one pick; the Dstr renderer's own builder is not found). settles: the caller of 00971170 for a
// Dstr render node (its vertex layout choice), and the basis argument order of 00967380 (the call is at
// 00971170:104 and :189, the decompile loses one argument of 00543ff0).
// [stand-in]: the scene behind is Bevy's copy of the picture taken before the see-through
// things are drawn (smoke and fire behind the haze are not bent), and the blend is done
// here against that copy. The game's pixel 731810 samples its own scene texture (`TextureSampler`,
// s1) at the bent screen position; which texture is bound there is not read. settles: the bind of
// TextureSampler for the Dstr renderer (the caller of the distortion draw, not read).

#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings as view_bindings,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var map_sampler: sampler;

fn screen(p: vec3<f32>) -> vec2<f32> {
    let clip = view_bindings::view.clip_from_world * vec4<f32>(p, 1.0);
    return clip.xy / clip.w * vec2<f32>(0.5, -0.5) + 0.5;
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let d = textureSample(map, map_sampler, in.uv);
    let fade = in.color.a;
    let alpha = saturate(d.a * fade);
    let push = (vec2<f32>(0.5) - d.rg) * d.a * fade;
    // The game's 731560_vs: A = left x n.y - up x n.x, B = left x n.y + up x n.x (o3, o4), with n.x and n.y
    // the two numbers the port carries in the colour's red and green (`effects.rs` quads).
    let n = in.color.rg;
    let left = -view_bindings::view.world_from_view[0].xyz;
    let up = view_bindings::view.world_from_view[1].xyz;
    let a = left * n.y - up * n.x;
    let b = left * n.y + up * n.x;
    let place = in.world_position.xyz + a * push.x + b * push.y;
    let here = textureSampleLevel(view_bindings::view_transmission_texture, view_bindings::view_transmission_sampler, screen(in.world_position.xyz), 0.0);
    let bent = textureSampleLevel(view_bindings::view_transmission_texture, view_bindings::view_transmission_sampler, screen(place), 0.0);
    var out: FragmentOutput;
    out.color = vec4<f32>(mix(here.rgb, bent.rgb, alpha), 1.0);
    return out;
}
