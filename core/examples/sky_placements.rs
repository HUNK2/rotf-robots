//! Prints a zone's `sky.` placements as the level renderer loads them: position, scale row lengths,
//! draw distance, LOD, visibility masks, spin, and the zone's visibility mask at a point.
//! `cargo run -p tf2-core --example sky_placements -- <zone.str> [x y z]`
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let level = tf2_core::level_render::load(Path::new(&args[1])).expect("load");
    for p in level.placed.iter().filter(|p| p.model.to_lowercase().starts_with("sky.")) {
        let w = &p.world;
        let len = |i: usize| (w[i] * w[i] + w[i + 1] * w[i + 1] + w[i + 2] * w[i + 2]).sqrt();
        println!("{} at ({:.1}, {:.1}, {:.1}) scale ({:.1}, {:.1}, {:.1}) draw {} lod {} masks {:?} spin {} no_shadow {}",
            p.model, w[9], w[10], w[11], len(0), len(3), len(6), p.draw_distance, p.lod, p.visibility_masks, p.spin.is_some(), p.no_shadow);
    }
    if args.len() >= 5 {
        let v: Vec<f32> = args[2..5].iter().map(|s| s.parse().unwrap()).collect();
        let at = glam::Vec3::new(v[0], v[1], v[2]);
        println!("visibility boxes {}, active mask at {at}: {:#x}", level.visibility_boxes.len(),
            tf2_core::level_render::active_visibility(&level.visibility_boxes, at));
    }
}
