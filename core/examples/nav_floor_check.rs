//! Compares the nav mesh's triangle floors with the mission's collision ground heights.
//! `cargo run --manifest-path C:/Bumblebee/Cargo.toml -p tf2-core --example nav_floor_check -- [zone.str | MissionName ...]`
//! Default: `levels/us_test/zone.str` and `Shanghai_01_Construction_A_CSA`.
//! The mesh is the one-placement-per-object mesh of the zone (as `combat.rs` and `tactics_check`
//! build it). The collision is the mission's packs as `--mission` loads them (`level_world`), and
//! the zone alone for comparison. Reads the install. Writes nothing.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use glam::Vec3;
use tf2_core::formats::hash::crc32;
use tf2_core::formats::missionlist::MissionList;
use tf2_core::level_things;
use tf2_core::level_world::{LevelWorld, NO_FLOOR};
use tf2_core::navmesh::{self, NavMesh, Silhouette};
use tf2_core::sim::World;

const FLT_MAX: f32 = f32::MAX;

fn norm(s: &str) -> String {
    s.to_ascii_lowercase().replace('\\', "/").trim_end_matches(".str").to_string()
}

struct Case {
    label: String,
    zone: PathBuf,
    packs: Vec<PathBuf>,
    /// The mission record's world_min / world_max ([data], the game's mesh box per navmesh.rs:463).
    world: Option<([i32; 3], [i32; 3])>,
}

fn resolve(game: &Path, list: &MissionList, arg: &str) -> Option<Case> {
    if arg.contains('/') || arg.ends_with(".str") {
        let zone = game.join(arg);
        let key = norm(arg);
        let mission = list.missions.iter().find(|m| m.stream_files.iter().any(|f| norm(f) == key));
        let (packs, label) = match mission {
            Some(m) => (
                list.pack_paths(m).iter().map(|p| game.join(p)).collect::<Vec<PathBuf>>(),
                format!("{arg} (mission {:?}, streamFiles {:?})", m.name, m.stream_files),
            ),
            None => (vec![zone.clone()], format!("{arg} (no mission lists it; zone only)")),
        };
        let world = mission.map(|m| (m.world_min, m.world_max));
        return Some(Case { label, zone, packs, world });
    }
    let mission = list.by_hash(crc32(arg.as_bytes()))?;
    let packs: Vec<PathBuf> = list.pack_paths(mission).iter().map(|p| game.join(p)).collect();
    let zone = mission
        .stream_files
        .iter()
        .find_map(|f| {
            let n = norm(f);
            n.ends_with("/zone").then(|| game.join(format!("{n}.str")))
        })
        .unwrap_or_else(|| packs[0].clone());
    Some(Case { label: format!("mission {arg}, streamFiles {:?}", mission.stream_files), zone, packs, world: Some((mission.world_min, mission.world_max)) })
}

/// Crossing-number test in x and y.
fn point_in(poly: &[[f32; 3]], x: f32, y: f32) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi, xj, yj) = (poly[i][0], poly[i][1], poly[j][0], poly[j][1]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn first_per_object(sils: &[Silhouette]) -> Vec<Silhouette> {
    let mut seen = HashSet::new();
    sils.iter().filter(|s| seen.insert(s.object)).cloned().collect()
}

fn report(game: &Path, c: &Case) {
    println!("==================================================================");
    println!("case: {}", c.label);
    println!("zone: {}", c.zone.display());
    println!("collision packs: {:?}", c.packs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>());

    let mut all = Vec::new();
    for p in &c.packs {
        match navmesh::load(p) {
            Ok((s, sk)) => {
                println!("silhouettes in {}: {} read, {} skipped", p.display(), s.len(), sk.len());
                all.extend(s);
            }
            Err(e) => {
                println!("{}: {e}", p.display());
                return;
            }
        }
    }
    let zone_sils = match navmesh::load(&c.zone) {
        Ok((s, _)) => s,
        Err(e) => {
            println!("{}: {e}", c.zone.display());
            return;
        }
    };
    let primary_sils = first_per_object(&zone_sils);
    let mesh = match NavMesh::build(None, &primary_sils) {
        Ok(m) => m,
        Err(e) => {
            println!("build failed: {e}");
            return;
        }
    };
    println!("mesh variants (triangle counts; the integrator's 3208 should match one):");
    let variants: Vec<(&str, Vec<Silhouette>)> = vec![
        ("zone, all placements", zone_sils.clone()),
        ("zone, first per object (PRIMARY)", primary_sils.clone()),
        ("collision packs, all placements (game build_nav)", all.clone()),
        ("collision packs, first per object", first_per_object(&all)),
    ];
    for (name, sils) in &variants {
        match NavMesh::build(None, sils) {
            Ok(m) => println!("  {name}: {} silhouettes, {} triangles", sils.len(), m.tri_count()),
            Err(e) => println!("  {name}: build failed: {e}"),
        }
    }

    // (a) bounds
    let (lo, hi) = mesh.bounds();
    println!("(a) mesh bounds x {:.3}..{:.3}  y {:.3}..{:.3}  z {:.3}..{:.3}", lo.x, hi.x, lo.y, hi.y, lo.z, hi.z);
    println!("    mesh root floor (box min z, what open ground without a silhouette gets) = {:.3}", lo.z);
    println!("    mission record world_min {:?} world_max {:?} [data] (the game's box per navmesh.rs:463, not passed to build)", c.world.map(|w| w.0), c.world.map(|w| w.1));
    let sil_min_z = primary_sils.iter().map(|s| s.base_z - s.pivot).fold(f32::MAX, f32::min);
    println!("    lowest silhouette bottom (base_z - pivot) = {sil_min_z:.3}; minus the 10 m margin = {:.3}", sil_min_z - 10.0);
    let level = match LevelWorld::load_packs(game, &c.packs) {
        Ok(l) => l,
        Err(e) => {
            println!("collision load failed: {e}");
            return;
        }
    };
    let zone_level = match LevelWorld::load_packs(game, std::slice::from_ref(&c.zone)) {
        Ok(l) => l,
        Err(e) => {
            println!("zone-only collision load failed: {e}");
            return;
        }
    };
    let cen: Vec<Vec3> = level.world.primitives.iter().map(|p| p.centre).collect();
    let cmin = cen.iter().fold(Vec3::splat(f32::MAX), |a, &b| a.min(b));
    let cmax = cen.iter().fold(Vec3::splat(f32::MIN), |a, &b| a.max(b));
    println!(
        "    collision: {} primitives; primitive centres x {:.3}..{:.3}  y {:.3}..{:.3}  z {:.3}..{:.3}",
        cen.len(),
        cmin.x,
        cmax.x,
        cmin.y,
        cmax.y,
        cmin.z,
        cmax.z
    );
    println!("    zone-only collision: {} primitives", zone_level.world.primitives.len());

    // (b) spawn
    match level_things::spawn_points(&c.packs) {
        Ok(pts) if !pts.is_empty() => {
            let p = pts[0].pos;
            println!("(b) spawn point {} of {}: ({:.3}, {:.3}, {:.3})", pts[0].name, pts.len(), p.x, p.y, p.z);
            println!(
                "    collision floor: from z+2 {:.3}, from z 1000 {:.3}; zone-only from z 1000 {:.3}",
                level.floor(p.x, p.y, p.z + 2.0),
                level.floor(p.x, p.y, 1000.0),
                zone_level.floor(p.x, p.y, 1000.0)
            );
            match mesh.locate(p) {
                Some(t) => println!("    mesh triangle there {t}: floor {:.3}, ceiling {:.3}, incenter {:?}", mesh.tri_floor(t), mesh.tri_ceiling(t), mesh.tri_incenter(t)),
                None => println!("    spawn point is outside the mesh"),
            }
        }
        Ok(_) => println!("(b) no spawn points"),
        Err(e) => println!("(b) spawn points: {e}"),
    }

    // (c) histogram of floors
    let walkable = |t: usize| mesh.tri_ceiling(t) == FLT_MAX && mesh.tri_floor(t) != FLT_MAX;
    let mut flt = 0usize;
    let mut hist: BTreeMap<i64, usize> = BTreeMap::new();
    let mut walk_count = 0usize;
    for t in mesh.triangles() {
        let f = mesh.tri_floor(t);
        if f == FLT_MAX {
            flt += 1;
            continue;
        }
        *hist.entry((f * 2.0).round() as i64).or_default() += 1;
        if walkable(t) {
            walk_count += 1;
        }
    }
    println!("(c) triangles: {} total, floor FLT_MAX: {flt}, walkable (ceiling FLT_MAX, floor set): {walk_count}", mesh.tri_count());
    let mut top: Vec<(i64, usize)> = hist.iter().map(|(&k, &v)| (k, v)).collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    println!("    top 10 floor values (rounded to 0.5 m): {}", top.iter().take(10).map(|(k, n)| format!("{:.1} m x{n}", *k as f32 / 2.0)).collect::<Vec<_>>().join(", "));
    println!("    distinct floor values: {}", hist.len());
    let root_count = mesh.triangles().filter(|&t| walkable(t) && mesh.tri_floor(t) == lo.z).count();
    println!("    walkable triangles at the root constant {:.3}: {root_count}; set by a silhouette top: {}", lo.z, walk_count - root_count);

    // agreement over all walkable triangles
    let (mut agree2, mut agree1000, mut none_count) = (0usize, 0usize, 0usize);
    let walk: Vec<usize> = mesh.triangles().filter(|&t| walkable(t)).collect();
    for &t in &walk {
        let ic = mesh.tri_incenter(t);
        let mf = mesh.tri_floor(t);
        let a = level.floor(ic[0], ic[1], mf + 2.0);
        let b = level.floor(ic[0], ic[1], 1000.0);
        if a <= NO_FLOOR + 1.0 || b <= NO_FLOOR + 1.0 {
            none_count += 1;
        }
        if (a - mf).abs() <= 0.3 {
            agree2 += 1;
        }
        if (b - mf).abs() <= 0.3 {
            agree1000 += 1;
        }
    }
    println!(
        "    walkable triangles with collision floor within 0.3 m of mesh floor: {agree2} of {} (cast from mesh floor + 2), {agree1000} of {} (cast from z 1000); collision reports no floor: {none_count}",
        walk.len(),
        walk.len()
    );

    // (d) sample
    let n = 20.min(walk.len());
    println!("(d) {n} walkable triangles spread over the mesh (incenter, mesh floor/ceiling, collision ground, raw hits, setters):");
    for i in 0..n {
        let t = walk[i * walk.len() / n];
        let ic = mesh.tri_incenter(t);
        let (mf, mc) = (mesh.tri_floor(t), mesh.tri_ceiling(t));
        let a = level.floor(ic[0], ic[1], mf + 2.0);
        let b = level.floor(ic[0], ic[1], 1000.0);
        let z = zone_level.floor(ic[0], ic[1], 1000.0);
        let mut raw: Vec<f32> = level.world.ray_cast(Vec3::new(ic[0], ic[1], 1000.0), Vec3::new(ic[0], ic[1], -1000.0)).iter().map(|h| h.point.z).collect();
        raw.sort_by(|a, b| b.total_cmp(a));
        raw.truncate(4);
        let setters: Vec<String> = primary_sils
            .iter()
            .filter(|s| s.height < FLT_MAX && point_in(&s.points, ic[0], ic[1]))
            .map(|s| {
                let top = s.base_z - s.pivot + s.height;
                let tag = if (top - mf).abs() < 1e-3 { " <- sets floor" } else { "" };
                format!("base {:.2} pivot {:.2} h {:.2} top {:.2}{tag}", s.base_z, s.pivot, s.height, top)
            })
            .take(3)
            .collect();
        let ceil = if mc == FLT_MAX { "max".to_string() } else { format!("{mc:.3}") };
        println!(
            "  tri {t:5} ic ({:8.2},{:8.2}) floor {mf:8.3} ceil {ceil} | coll from floor+2 {a:9.3} | coll from 1000 {b:9.3} | zone-only {z:9.3} | raw hits z {raw:?} | sils: {setters:?}",
            ic[0], ic[1]
        );
    }
}

fn main() {
    let game = PathBuf::from(tf2_core::install::dir_string());
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        args = vec!["levels/us_test/zone.str".into(), "Shanghai_01_Construction_A_CSA".into()];
    }
    let list = match MissionList::load(&game) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("mission list: {e}");
            std::process::exit(1);
        }
    };
    for arg in &args {
        match resolve(&game, &list, arg) {
            Some(c) => report(&game, &c),
            None => println!("{arg}: no such mission"),
        }
    }
}
