//! The drawn models of a zone that come within a radius of a point: each one's bounding box in
//! the world, its LOD and source, and per part the material's technique, blend mode, diffuse
//! colour, texture opacity and texture. Radius 0 prints a whole-zone summary instead.
//!
//! cargo run --manifest-path C:/Bumblebee/Cargo.toml -p tf2-core --release --example zone_models_near -- <zone.str> <x> <y> <radius>
//!
//! The distance is measured from (x, y) to the model's bounding box footprint in the ground
//! plane (0 inside it). Lines are sorted by bounding-box volume, largest first.
//!
//! Summary (radius 0): counts per name fragment, per material technique, per blend mode, texture
//! status, flat models (smallest box extent under 0.5 m), and LOD-like pairs: placements whose
//! bounding-box centres round to the same metre, where the largest has at least 4 times the
//! triangles of the smallest. [assumed] same-centre is the test for "same place"; a pair split by
//! a rounding boundary is missed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use tf2_core::formats::hash::lower33;
use tf2_core::formats::model::{Library, Mesh, Part};
use tf2_core::level_render::{self, PlacedModel};

/// Name fragments counted in summary mode (matched in the lower-cased model name).
const KEYWORDS: &[&str] = &[
    "lod", "glass", "fade", "vis", "trigger", "occl", "bg", "sky", "shadow", "card", "dome", "proxy", "low", "_lo",
    "decal", "damage", "debris", "broken", "collision", "ui", "hud", "plane", "water", "sign", "light",
];

/// A model's bounding box in the world and its counts.
struct Measure {
    lo: [f32; 3],
    hi: [f32; 3],
    parts: usize,
    verts: usize,
    tris: usize,
}

/// A model vertex p goes to p * axes + position (row vectors; the 4x3 is row by row).
fn world_point(w: &[f32; 12], v: [f32; 3]) -> [f32; 3] {
    let mut out = [0.0f32; 3];
    for c in 0..3 {
        out[c] = v[0] * w[c] + v[1] * w[3 + c] + v[2] * w[6 + c] + w[9 + c];
    }
    out
}

fn measure(mesh: &Mesh, world: &[f32; 12]) -> Measure {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    let (mut verts, mut tris) = (0, 0);
    for part in &mesh.parts {
        verts += part.positions.len();
        tris += part.indices.len() / 3;
        for &p in &part.positions {
            let w = world_point(world, p);
            for c in 0..3 {
                lo[c] = lo[c].min(w[c]);
                hi[c] = hi[c].max(w[c]);
            }
        }
    }
    if verts == 0 {
        lo = [0.0; 3];
        hi = [0.0; 3];
    }
    Measure { lo, hi, parts: mesh.parts.len(), verts, tris }
}

fn size(m: &Measure) -> [f32; 3] {
    [m.hi[0] - m.lo[0], m.hi[1] - m.lo[1], m.hi[2] - m.lo[2]]
}

fn smallest(m: &Measure) -> f32 {
    size(m).iter().cloned().fold(f32::MAX, f32::min)
}

fn volume(m: &Measure) -> f64 {
    size(m).iter().map(|&s| s as f64).product()
}

/// Distance from (x, y) to the box's footprint in the ground plane (0 inside).
fn footprint_distance(m: &Measure, x: f32, y: f32) -> f32 {
    let dx = (m.lo[0] - x).max(0.0).max(x - m.hi[0]);
    let dy = (m.lo[1] - y).max(0.0).max(y - m.hi[1]);
    (dx * dx + dy * dy).sqrt()
}

fn opt_hex(v: Option<u32>) -> String {
    v.map_or("-".into(), |w| format!("{w:#010x}"))
}

fn opt_num(v: Option<f32>) -> String {
    v.map_or("-".into(), |n| format!("{n:.3}"))
}

fn opt_vec(v: Option<[f32; 4]>) -> String {
    v.map_or("-".into(), |c| format!("[{:.3}, {:.3}, {:.3}, {:.3}]", c[0], c[1], c[2], c[3]))
}

fn describe_part(library: &Library, index: usize, part: &Part) -> String {
    let Some(mid) = part.material else {
        return format!("    part {index}: no material");
    };
    let Some(mat) = library.materials.get(&mid) else {
        return format!("    part {index}: material {mid:#010x} not in library");
    };
    let texture = match mat.texture("diffuse_texture1_texture") {
        None => "none (absent or null)".to_string(),
        Some(id) => match library.textures.get(&id) {
            Some(t) => format!("{} ({}x{})", t.name, t.width, t.height),
            None => format!("UNRESOLVED id {id:#010x}"),
        },
    };
    format!(
        "    part {index}: material \"{}\" technique {:#010x} blendmode {} diffuse_color {} diffuse_texture1_opacity {} texture {texture}",
        mat.name,
        mat.technique,
        opt_hex(mat.word("blendmode")),
        opt_vec(mat.vector("diffuse_color")),
        opt_num(mat.number("diffuse_texture1_opacity")),
    )
}

fn near(placed: &[PlacedModel], library: &Library, measured: &[Option<Measure>], x: f32, y: f32, radius: f32) {
    let mut hits: Vec<(usize, &Measure)> = measured
        .iter()
        .enumerate()
        .filter_map(|(i, m)| m.as_ref().filter(|m| footprint_distance(m, x, y) <= radius).map(|m| (i, m)))
        .collect();
    hits.sort_by(|a, b| volume(b.1).total_cmp(&volume(a.1)));
    let unresolved = measured.iter().filter(|m| m.is_none()).count();
    println!("{} placed models within {radius} m (x, y) of ({x}, {y}); {unresolved} placements have no library mesh", hits.len());
    for (i, m) in hits {
        let p = &placed[i];
        let Some(mesh) = library.meshes.get(&lower33(&p.model)) else { continue };
        let s = size(m);
        println!(
            "{}  lod {}  lod chain {}  source {}",
            p.model,
            p.lod,
            mesh.lods.len(),
            p.source
        );
        println!(
            "  parts {} verts {} tris {}  bbox min ({:.2}, {:.2}, {:.2}) max ({:.2}, {:.2}, {:.2})  size ({:.2}, {:.2}, {:.2})  smallest {:.3}  volume {:.0}  dist_xy {:.2}",
            m.parts, m.verts, m.tris, m.lo[0], m.lo[1], m.lo[2], m.hi[0], m.hi[1], m.hi[2], s[0], s[1], s[2], smallest(m), volume(m),
            footprint_distance(m, x, y),
        );
        for (k, part) in mesh.parts.iter().enumerate() {
            println!("{}", describe_part(library, k, part));
        }
    }
}

fn summary(placed: &[PlacedModel], library: &Library, measured: &[Option<Measure>]) {
    let total = placed.len();
    let unresolved = measured.iter().filter(|m| m.is_none()).count();
    let flat_count = measured.iter().flatten().filter(|m| smallest(m) < 0.5).count();
    println!("{total} placed models; {unresolved} without a library mesh; {flat_count} with smallest box extent under 0.5 m");

    println!("\nname fragment: placements (of them flat)");
    for kw in KEYWORDS {
        let (mut n, mut f) = (0usize, 0usize);
        for (p, m) in placed.iter().zip(measured) {
            if p.model.to_ascii_lowercase().contains(kw) {
                n += 1;
                if m.as_ref().is_some_and(|m| smallest(m) < 0.5) {
                    f += 1;
                }
            }
        }
        println!("  {kw:<10} {n:6} ({f} flat)");
    }
    let with_chain = placed
        .iter()
        .filter(|p| library.meshes.get(&lower33(&p.model)).is_some_and(|m| !m.lods.is_empty()))
        .count();
    println!("  (mesh has a LOD chain) {with_chain}");

    // Per technique: parts, and placements that have at least one part with it.
    let mut tech: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
    let mut blend: BTreeMap<String, usize> = BTreeMap::new();
    let (mut mat_missing, mut tex_none, mut tex_unresolved, mut parts_total) = (0usize, 0usize, 0usize, 0usize);
    let mut flat_names: BTreeMap<String, (usize, f32)> = BTreeMap::new();
    for (p, m) in placed.iter().zip(measured) {
        if let Some(m) = m {
            if smallest(m) < 0.5 {
                let e = flat_names.entry(p.model.clone()).or_insert((0, 0.0));
                e.0 += 1;
                e.1 = e.1.max(size(m)[0].max(size(m)[1]).max(size(m)[2]));
            }
        }
        let Some(mesh) = library.meshes.get(&lower33(&p.model)) else { continue };
        let mut seen = BTreeSet::new();
        for part in &mesh.parts {
            parts_total += 1;
            let Some(mat) = part.material.and_then(|mid| library.materials.get(&mid)) else {
                mat_missing += 1;
                continue;
            };
            let e = tech.entry(mat.technique).or_insert((0, 0));
            e.0 += 1;
            if seen.insert(mat.technique) {
                e.1 += 1;
            }
            *blend.entry(opt_hex(mat.word("blendmode"))).or_insert(0) += 1;
            match mat.texture("diffuse_texture1_texture") {
                None => tex_none += 1,
                Some(id) if !library.textures.contains_key(&id) => tex_unresolved += 1,
                Some(_) => {}
            }
        }
    }
    println!("\nparts {parts_total}; parts without a material in the library {mat_missing}; no diffuse texture {tex_none}; texture id unresolved {tex_unresolved}");

    println!("\nmaterial technique: parts (placements)");
    let mut techs: Vec<(u32, (usize, usize))> = tech.into_iter().collect();
    techs.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (t, (parts, places)) in &techs {
        println!("  {t:#010x}  {parts:7} ({places})");
    }

    println!("\nblendmode word: parts");
    for (b, n) in &blend {
        println!("  {b}  {n}");
    }

    println!("\nflat distinct names (smallest extent under 0.5 m), largest extent first, top 40:");
    let mut flat: Vec<(&String, &(usize, f32))> = flat_names.iter().collect();
    flat.sort_by(|a, b| b.1 .1.total_cmp(&a.1 .1));
    for (name, (n, ext)) in flat.iter().take(40) {
        println!("  {ext:9.1} m  x{n:<4} {name}");
    }
    println!("  ({} distinct flat names)", flat_names.len());

    // LOD-like pairs: placements whose bounding-box centres round to the same metre.
    let mut groups: BTreeMap<[i64; 3], Vec<usize>> = BTreeMap::new();
    for (i, m) in measured.iter().enumerate() {
        let Some(m) = m else { continue };
        let key = [0usize, 1, 2].map(|c| ((m.lo[c] + m.hi[c]) * 0.5).round() as i64);
        groups.entry(key).or_default().push(i);
    }
    let tris = |i: usize| measured[i].as_ref().map_or(0, |m| m.tris);
    let mut groups_multi = 0usize;
    let mut far: Vec<([i64; 3], usize, usize)> = Vec::new();
    for (key, members) in &groups {
        if members.len() < 2 {
            continue;
        }
        groups_multi += 1;
        let big = *members.iter().max_by_key(|&&i| tris(i)).unwrap();
        let small = *members.iter().min_by_key(|&&i| tris(i)).unwrap();
        if tris(big) >= 4 * tris(small).max(1) {
            far.push((*key, big, small));
        }
    }
    println!("\ncentre groups (1 m) with two or more placements: {groups_multi}; with the largest at least 4x the smallest's triangles: {}", far.len());
    let mut small_hits: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, _, small) in &far {
        let name = placed[*small].model.to_ascii_lowercase();
        for kw in KEYWORDS {
            if name.contains(kw) {
                *small_hits.entry(kw).or_insert(0) += 1;
            }
        }
    }
    println!("  fragments in the SMALLER member's name: {small_hits:?}");
    for (key, big, small) in far.iter().take(12) {
        println!(
            "  centre ({}, {}, {}): {} {} tris  vs  {} {} tris",
            key[0], key[1], key[2], placed[*big].model, tris(*big), placed[*small].model, tris(*small)
        );
    }
}

/// The LOD chains the library gives the named meshes (`Mesh::lods`): per distinct model name, the
/// named mesh and its first chain mesh, with triangle counts and the chain's distances.
fn lod_chains(placed: &[PlacedModel], library: &Library) {
    let mut seen: BTreeMap<&str, String> = BTreeMap::new();
    let mut chain_names: BTreeSet<&str> = BTreeSet::new();
    for p in placed {
        let Some(mesh) = library.meshes.get(&lower33(&p.model)) else { continue };
        let Some(&(lid, _)) = mesh.lods.first() else { continue };
        chain_names.insert(p.model.as_str());
        if seen.contains_key(p.model.as_str()) {
            continue;
        }
        let named: usize = mesh.parts.iter().map(|q| q.indices.len() / 3).sum();
        let (low_name, low) = match library.meshes.get(&lid) {
            Some(low) => (low.name.clone(), low.parts.iter().map(|q| q.indices.len() / 3).sum::<usize>()),
            None => ("UNRESOLVED".into(), 0),
        };
        let dists: Vec<String> = mesh.lods.iter().map(|&(_, d)| format!("{d}")).collect();
        seen.insert(p.model.as_str(), format!("{named} tris -> {low_name} ({low} tris), chain distances [{}]", dists.join(", ")));
    }
    println!("\nLOD chains: {} distinct model names carry one; chain examples (named mesh -> first chain mesh):", chain_names.len());
    for (name, text) in seen.iter().take(25) {
        println!("  {name}: {text}");
    }
    let low_chain = seen.values().filter(|t| t.contains("low") || t.contains("lod") || t.contains("Low")).count();
    println!("  chain targets whose name holds low / lod: {low_chain} of {}", seen.len());
    // Are any chain targets also placed as models of their own (drawn at full size, not as a LOD)?
    let mut targets: BTreeSet<u32> = BTreeSet::new();
    for p in placed {
        if let Some(mesh) = library.meshes.get(&lower33(&p.model)) {
            targets.extend(mesh.lods.iter().map(|&(id, _)| id));
        }
    }
    let placed_as_target = placed.iter().filter(|p| targets.contains(&lower33(&p.model))).count();
    println!("  placements whose name is also a LOD chain target: {placed_as_target} (distinct chain targets: {})", targets.len());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("usage: zone_models_near <zone.str> <x> <y> <radius>   (radius 0: whole-zone summary)");
        std::process::exit(2);
    }
    let x: f32 = args[2].parse().expect("x");
    let y: f32 = args[3].parse().expect("y");
    let radius: f32 = args[4].parse().expect("radius");
    let level = level_render::load(Path::new(&args[1])).expect("load");
    let library = &level.library;
    let measured: Vec<Option<Measure>> = level
        .placed
        .iter()
        .map(|p| library.meshes.get(&lower33(&p.model)).map(|mesh| measure(mesh, &p.world)))
        .collect();
    if radius <= 0.0 {
        summary(&level.placed, library, &measured);
        lod_chains(&level.placed, library);
    } else {
        near(&level.placed, library, &measured, x, y, radius);
    }
}
