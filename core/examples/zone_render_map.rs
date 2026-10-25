//! A top-down picture of a zone's DRAWN geometry: every vertex of every placed model (the most
//! detailed level only, `lod` 0), transformed by its world matrix (`level_render::load`).
//!
//! cargo run -p tf2-core --release --example zone_render_map -- <zone.str> <out.png> [metres per pixel, default 5]
//!
//! Each pixel is the highest vertex that falls in it (so roofs show over walls). Grey = that
//! vertex's height (black at -20 m, white at 60 m and above); background dark red. The picture
//! bounds are the vertices' own x and y. North (y) is up, as in `zone_map`.

use std::path::Path;

use tf2_core::level_render;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: zone_render_map <zone.str> <out.png> [metres per pixel]");
        std::process::exit(2);
    }
    let step: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(5.0);
    // Optional: leave out any model whose drawn extent (its largest side) is longer than this,
    // metres. Without it the sky domes and the distant backdrop set the picture's size.
    let cap: Option<f32> = args.get(4).and_then(|s| s.parse().ok());
    let level = level_render::load(Path::new(&args[1])).expect("load");
    println!("{} placed models, {} library meshes", level.placed.len(), level.library.meshes.len());

    // Every vertex, transformed: x' = x*w0 + y*w3 + z*w6 + w9 (row vectors, as collision_world does).
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let (mut missing, mut left_out) = (0usize, 0usize);
    for placed in level.placed.iter().filter(|p| p.lod == 0) {
        let Some(mesh) = level.library.meshes.get(&tf2_core::formats::hash::lower33(&placed.model)) else {
            missing += 1;
            continue;
        };
        let w = placed.world;
        let mut model_verts: Vec<[f32; 3]> = Vec::new();
        for part in &mesh.parts {
            for p in &part.positions {
                let [x, y, z] = [p[0], p[1], p[2]];
                model_verts.push([
                    x * w[0] + y * w[3] + z * w[6] + w[9],
                    x * w[1] + y * w[4] + z * w[7] + w[10],
                    x * w[2] + y * w[5] + z * w[8] + w[11],
                ]);
            }
        }
        if let Some(cap) = cap {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in &model_verts {
                for i in 0..3 {
                    lo[i] = lo[i].min(v[i]);
                    hi[i] = hi[i].max(v[i]);
                }
            }
            if (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]) > cap {
                left_out += 1;
                continue;
            }
        }
        verts.append(&mut model_verts);
    }
    println!("{} vertices drawn; {missing} placed models with no mesh; {left_out} left out by the size cap", verts.len());
    if verts.is_empty() {
        eprintln!("no vertices");
        std::process::exit(1);
    }

    let (mut min, mut max) = ([f32::MAX; 2], [f32::MIN; 2]);
    for v in &verts {
        for i in 0..2 {
            min[i] = min[i].min(v[i]);
            max[i] = max[i].max(v[i]);
        }
    }
    let (w, h) = (((max[0] - min[0]) / step) as u32 + 1, ((max[1] - min[1]) / step) as u32 + 1);
    println!("x {:.0}..{:.0}  y {:.0}..{:.0}  picture {w} x {h} at {step} m per pixel", min[0], max[0], min[1], max[1]);

    // Highest z per pixel; untouched pixels stay at NaN (background).
    let mut top = vec![f32::NAN; (w * h) as usize];
    for v in &verts {
        let px = (((v[0] - min[0]) / step) as u32).min(w - 1);
        let py = (((v[1] - min[1]) / step) as u32).min(h - 1);
        let cell = &mut top[(py * w + px) as usize];
        if cell.is_nan() || v[2] > *cell {
            *cell = v[2];
        }
    }

    let mut image = image::RgbImage::new(w, h);
    let mut drawn = 0usize;
    for py in 0..h {
        for px in 0..w {
            // North (y up) at the top of the picture.
            let pixel = image.get_pixel_mut(px, h - 1 - py);
            let z = top[(py * w + px) as usize];
            if z.is_nan() {
                *pixel = image::Rgb([90, 0, 0]);
            } else {
                drawn += 1;
                let shade = (((z + 20.0) / 80.0).clamp(0.0, 1.0) * 255.0) as u8;
                *pixel = image::Rgb([shade, shade, shade]);
            }
        }
    }
    image.save(&args[2]).expect("save");
    println!("{drawn} of {} pixels hold a vertex; saved {}", w * h, args[2]);
}
