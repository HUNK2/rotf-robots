//! A top-down picture of a zone's collision, by casting rays straight down through
//! `collision_world::StaticWorld`. A check that the level is where it should be.
//!
//! cargo run -p tf2-core --release --example zone_map -- <zone.str> <out.png> [metres per pixel, default 5]
//!
//! Each pixel is the nearest hit of a ray from z = +400 to z = -100, skipping primitives
//! flagged `released` (destructible pieces) and, [assumed] until the masks are read, hits
//! above z = 150 (ceilings and volumes that are not ground). Grey = height (black -20 m,
//! white 60 m and up); red = no hit. Also prints, per mask preset, how many placed
//! primitives there are and how often each is the nearest hit.

use std::collections::BTreeMap;
use std::path::Path;

use glam::Vec3;
use tf2_core::collision_world::StaticWorld;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: zone_map <zone.str> <out.png> [metres per pixel]");
        std::process::exit(2);
    }
    let step: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(5.0);
    let world = StaticWorld::load(Path::new(&args[1])).expect("load");
    let solid: Vec<usize> = (0..world.primitives.len()).filter(|&i| !world.primitives[i].released).collect();
    println!("{} placed primitives, {} not released", world.primitives.len(), solid.len());

    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for &i in &solid {
        min = min.min(world.primitives[i].centre);
        max = max.max(world.primitives[i].centre);
    }
    let (w, h) = (((max.x - min.x) / step) as u32 + 1, ((max.y - min.y) / step) as u32 + 1);
    println!("x {:.0}..{:.0}  y {:.0}..{:.0}  picture {w} x {h} at {step} m per pixel", min.x, max.x, min.y, max.y);

    let mut placed: BTreeMap<u32, usize> = BTreeMap::new();
    for &i in &solid {
        *placed.entry(world.primitives[i].mask_preset).or_default() += 1;
    }
    let mut nearest: BTreeMap<u32, (usize, f64)> = BTreeMap::new();
    let mut image = image::RgbImage::new(w, h);
    let mut misses = 0usize;
    for py in 0..h {
        for px in 0..w {
            let (x, y) = (min.x + px as f32 * step, min.y + py as f32 * step);
            let hit = world
                .ray_cast(Vec3::new(x, y, 400.0), Vec3::new(x, y, -100.0))
                .into_iter()
                .filter(|hit| !world.primitives[hit.primitive].released && hit.point.z < 150.0)
                .min_by(|a, b| a.lambda.total_cmp(&b.lambda));
            // North (y up) at the top of the picture.
            let pixel = image.get_pixel_mut(px, h - 1 - py);
            match hit {
                Some(hit) => {
                    let entry = nearest.entry(world.primitives[hit.primitive].mask_preset).or_default();
                    entry.0 += 1;
                    entry.1 += hit.point.z as f64;
                    let shade = (((hit.point.z + 20.0) / 80.0).clamp(0.0, 1.0) * 255.0) as u8;
                    *pixel = image::Rgb([shade, shade, shade]);
                }
                None => {
                    misses += 1;
                    *pixel = image::Rgb([160, 0, 0]);
                }
            }
        }
    }
    image.save(&args[2]).expect("save");
    println!("{} rays, {} without a hit; saved {}", w * h, misses, args[2]);
    println!("mask preset   placed   nearest   mean z of those hits");
    for (preset, count) in &placed {
        let (n, z) = nearest.get(preset).copied().unwrap_or((0, 0.0));
        println!("  {preset:08x}  {count:7}  {n:8}   {}", if n > 0 { format!("{:.2}", z / n as f64) } else { "-".into() });
    }
}
