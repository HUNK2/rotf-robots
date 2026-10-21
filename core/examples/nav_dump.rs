//! The navigation mesh of a pack's NavSilhouette objects, built the way the game builds it at
//! mission load (`core\src\navmesh.rs`). Prints the counts and the build time.
//! `cargo run --manifest-path C:/Bumblebee/Cargo.toml -p tf2-core --release --example nav_dump -- [pack] [x,y,z x,y,z]`
//! `pack` is a path, or relative to the install; default `levels/us_test/zone.str`.
//! Two points `x,y,z` (arguments with a comma) plan a path on the one-placement-per-object mesh
//! (agent radius 1.0, height 4.0) with `navpath::plan`, and print the records and the length.
//! The first 12 silhouette centres are printed first, to choose the points from.
//! Reads `C:\Games2` (or `TF2_GAME_DIR`). Writes nothing.

use std::path::{Path, PathBuf};
use std::time::Instant;

use glam::Vec3;
use tf2_core::navmesh::{self, NavMesh};
use tf2_core::navpath::{self, Agent};

fn parse_point(text: &str) -> [f32; 3] {
    let parts: Vec<f32> = text.split(',').map(|p| p.trim().parse::<f32>().unwrap_or(f32::NAN)).collect();
    if parts.len() != 3 || parts.iter().any(|v| !v.is_finite()) {
        eprintln!("not a point x,y,z: {text}");
        std::process::exit(2);
    }
    [parts[0], parts[1], parts[2]]
}

fn main() {
    let dir = PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into()));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = args.iter().find(|a| !a.contains(',')).cloned().unwrap_or_else(|| "levels/us_test/zone.str".into());
    let points: Vec<[f32; 3]> = args.iter().filter(|a| a.contains(',')).map(|a| parse_point(a)).collect();
    let path = if Path::new(&arg).is_absolute() { PathBuf::from(&arg) } else { dir.join(&arg) };

    let (sils, skipped) = match navmesh::load(&path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            std::process::exit(1);
        }
    };
    println!("{}", path.display());
    println!("silhouettes read: {}, skipped: {}", sils.len(), skipped.len());
    for line in skipped.iter().take(10) {
        println!("  skipped: {line}");
    }
    let mut objects: std::collections::BTreeMap<(usize, usize), usize> = std::collections::BTreeMap::new();
    for s in &sils {
        objects.insert(s.object, s.raw_points);
    }
    let mut per_chunk: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    for key in objects.keys() {
        *per_chunk.entry(key.0).or_default() += 1;
    }
    println!("distinct objects (chunk, offset): {}, per chunk: {per_chunk:?}", objects.len());
    println!("outline points of the distinct objects: {} in the data", objects.values().sum::<usize>());
    let raw: usize = sils.iter().map(|s| s.raw_points).sum();
    let kept: usize = sils.iter().map(|s| s.points.len() - 1).sum();
    println!("outline points over all placements: {raw} in the data (including the repeated first point), {kept} after the 1 mm chord rule");
    let cw = sils.iter().filter(|s| s.is_clockwise()).count();
    println!("winding: {cw} clockwise, {} counter-clockwise", sils.len() - cw);
    let heights = sils.iter().filter(|s| s.height < f32::MAX).count();
    println!("silhouettes with a height: {heights} (the rest have no top, FLT_MAX)");

    let start = Instant::now();
    let mesh = match NavMesh::build(None, &sils) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("build failed: {e}");
            std::process::exit(1);
        }
    };
    let elapsed = start.elapsed();
    println!("build time: {:.3} s", elapsed.as_secs_f64());
    println!("vertices: {}", mesh.vertex_count());
    println!("triangles: {}", mesh.tri_count());
    println!("marked edge sides: {} (each constraint edge counts on both sides)", mesh.marked_cells());
    println!("ring segments not constrained (length at or below 0.224 m): {}", mesh.short_pairs);
    println!("constraint pieces not inserted (too short or no crossing found): {}", mesh.skipped_pieces);
    let (lo, hi) = mesh.bounds();
    println!("bounds: {lo:?} .. {hi:?}");
    // The same build with one placement per object (the first one the walk finds).
    let mut seen = std::collections::HashSet::new();
    let first: Vec<_> = sils.iter().filter(|s| seen.insert(s.object)).cloned().collect();
    let t1 = Instant::now();
    let one = NavMesh::build(None, &first);
    match &one {
        Ok(m1) => println!(
            "one placement per object: {} silhouettes, {} vertices, {} triangles, build {:.3} s, {} short pairs, {} skipped pieces",
            first.len(),
            m1.vertex_count(),
            m1.tri_count(),
            t1.elapsed().as_secs_f64(),
            m1.short_pairs,
            m1.skipped_pieces
        ),
        Err(e) => println!("one placement per object: build failed: {e}"),
    }
    println!("centres of the first 12 silhouettes (one placement per object):");
    for s in first.iter().take(12) {
        let n = s.points.len().saturating_sub(1).max(1);
        let (sx, sy) = s.points.iter().take(n).fold((0.0f32, 0.0f32), |acc, p| (acc.0 + p[0], acc.1 + p[1]));
        println!("  centre ({:.2}, {:.2}), height {:.2}, {} outline points", sx / n as f32, sy / n as f32, s.height, n);
    }
    if let (Ok(m1), 2) = (&one, points.len()) {
        let agent = Agent::new(1.0, 4.0, 0);
        let (from, to) = (Vec3::from(points[0]), Vec3::from(points[1]));
        let t2 = Instant::now();
        match navpath::plan(m1, from, to, &agent) {
            Some(records) => {
                let length: f32 = records.windows(2).map(|w| w[0].point.distance(w[1].point)).sum();
                println!("path {from} to {to}: {} records, length {length:.2} m, plan {:.4} s", records.len(), t2.elapsed().as_secs_f64());
                for (i, r) in records.iter().enumerate() {
                    let floor = m1.locate(r.point).map(|t| (m1.tri_floor(t), m1.tri_ceiling(t)));
                    println!("  record {i}: {} kind {} triangle floor/ceiling {floor:?}", r.point, r.kind);
                }
            }
            None => println!("path {from} to {to}: none (outside the mesh, or no path found)"),
        }
    } else if !points.is_empty() {
        println!("give exactly two points x,y,z to plan a path");
    }
    let inside = mesh.inside_count(&sils);
    let floor_set = mesh.triangles().filter(|&t| mesh.tri_floor(t) != lo.z).count();
    println!("triangles whose incenter lies inside a silhouette: {inside}");
    println!("triangles with a silhouette floor written: {floor_set}");
    println!("warnings: {}", mesh.warnings.len());
    for w in mesh.warnings.iter().take(10) {
        println!("  {w}");
    }
}
