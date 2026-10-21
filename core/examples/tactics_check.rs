//! The tactics point of a DComScout on the us_test street (`core\src\ai_tactics.rs`).
//! `cargo run --manifest-path C:/Bumblebee/Cargo.toml -p tf2-core --example tactics_check`
//!
//! The mesh is the one-placement-per-object mesh of `nav_dump` (`levels/us_test/zone.str`). The
//! target is the walkable triangle nearest the middle of the mesh box, and the attacker stands
//! 80 m from it on the walkable ground of the same floor.
//!
//! Two radii (manager ruling 2026-10-12): the flood radius R = min(distanceFromTargetMax, 150) and
//! the wall radius r = 1.65 x the character radius. The character radius is the scout's robot
//! radius `characterAttributesDComScout.avatarDescArray[robot].radius` [data], read through
//! `tuning::load` (`tuning.rs` line 834), the same call `Scout::load` makes.
//! The data is `characterAttributesDComScout.tacticsData` (`[data]`). Writes nothing.
//! Reads the install.

use std::collections::HashSet;
use std::path::PathBuf;

use glam::Vec3;
use tf2_core::ai_tactics::{self as tactics, Foe, Me, Record, TacticsData, GOAL_HOME, WALL_FACTOR};
use tf2_core::navmesh::{self, NavMesh};
use tf2_core::tuning;

fn main() {
    let dir = PathBuf::from(tf2_core::install::dir_string());
    let data = match tactics::load(&dir, "DComScout") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("tacticsData: {e}");
            std::process::exit(1);
        }
    };
    println!("tacticsData (DComScout): type {:08x}, distanceForTacticsMax {}", data.tactics_type, data.distance_for_tactics_max);
    println!(
        "  character distance {}..{}, height {}..{}; target distance {}..{}, height {}..{}",
        data.distance_from_character_min,
        data.distance_from_character_max,
        data.height_from_character_min,
        data.height_from_character_max,
        data.distance_from_target_min,
        data.distance_from_target_max,
        data.height_from_target_min,
        data.height_from_target_max
    );
    let radius = match tuning::load(&dir, "DComScout") {
        Ok(t) => t.robot_radius,
        Err(e) => {
            eprintln!("scout tuning: {e}");
            std::process::exit(1);
        }
    };
    println!("scout character radius (avatarDescArray[robot].radius) = {radius} m [data]");

    let path = dir.join("levels/us_test/zone.str");
    let (sils, _) = match navmesh::load(&path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            std::process::exit(1);
        }
    };
    let mut seen = HashSet::new();
    let first: Vec<_> = sils.iter().filter(|s| seen.insert(s.object)).cloned().collect();
    let mesh = match NavMesh::build(None, &first) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("build failed: {e}");
            std::process::exit(1);
        }
    };
    println!("mesh: one placement per object, {} silhouettes, {} triangles", first.len(), mesh.tri_count());

    // The target: the walkable triangle nearest the middle of the box.
    let (lo, hi) = mesh.bounds();
    let mid = (lo + hi) * 0.5;
    let walkable = |t: usize| mesh.tri_ceiling(t) == f32::MAX && mesh.tri_floor(t) != f32::MAX;
    let Some(tgt_tri) = mesh
        .triangles()
        .filter(|&t| walkable(t))
        .min_by(|&a, &b| {
            let da = (mesh.tri_incenter(a)[0] - mid.x).powi(2) + (mesh.tri_incenter(a)[1] - mid.y).powi(2);
            let db = (mesh.tri_incenter(b)[0] - mid.x).powi(2) + (mesh.tri_incenter(b)[1] - mid.y).powi(2);
            da.total_cmp(&db)
        })
    else {
        println!("no walkable triangle on the mesh");
        return;
    };
    let ic = mesh.tri_incenter(tgt_tri);
    let floor = mesh.tri_floor(tgt_tri);
    let target_pos = Vec3::new(ic[0], ic[1], floor);
    println!("target at {target_pos:?} (floor {floor}, triangle {tgt_tri})");

    // The attacker: 80 m from the target, on walkable ground of the same floor (first angle that fits).
    let mut attacker_pos = None;
    for step in 0..36 {
        let a = step as f32 * 10.0_f32.to_radians();
        let p = Vec3::new(target_pos.x + 80.0 * a.cos(), target_pos.y + 80.0 * a.sin(), floor);
        if let Some(t) = mesh.locate(p) {
            if walkable(t) && mesh.tri_floor(t) == floor {
                attacker_pos = Some(Vec3::new(p.x, p.y, floor));
                break;
            }
        }
    }
    let Some(attacker_pos) = attacker_pos else {
        println!("no walkable point 80 m from the target on its floor");
        return;
    };
    println!("attacker at {attacker_pos:?}, {:.1} m from the target", (attacker_pos - target_pos).truncate().length());

    let flood = tactics::flood_radius(&data);
    let wall = WALL_FACTOR * radius;
    println!("flood radius R = {flood} m [game: min(distanceFromTargetMax, 150)], wall radius r = {wall} m [game: 1.65 x radius]");
    run(&data, &mesh, target_pos, attacker_pos, radius, flood, wall);
}

fn run(data: &TacticsData, mesh: &NavMesh, target_pos: Vec3, attacker_pos: Vec3, radius: f32, flood: f32, wall: f32) {
    let cam = tactics::STAND_IN_CAMERA_HEIGHT;
    let me = Me {
        position: attacker_pos,
        camera: attacker_pos + Vec3::new(0.0, 0.0, cam),
        radius,
        flying: false,
        flag_3d: false,
        flag_a6: false,
        health: 100.0,
        health_max: 100.0,
        los_height: 1.8,
        los_flags: 0,
        home: None,
    };
    let foe = Foe { position: target_pos, camera: target_pos + Vec3::new(0.0, 0.0, cam) };
    let mut rec = Record::new();
    rec.goal = GOAL_HOME;

    let mode = tactics::candidate_mode(&me);
    let raw = tactics::nav_candidates_raw(mesh, target_pos, flood, wall, mode);
    println!("candidates after the flood and the wall rules (before the post pass): {}", raw.len());
    // Keep the owners for the examples, then the post pass.
    let owners: Vec<(Vec3, usize)> = raw.clone();
    let kept = tactics::post_pass(mesh, raw);
    println!("candidates after the post pass: {} (removed {})", kept.len(), owners.len() - kept.len());

    let (count, mut scored) = tactics::scan(&rec, data, &me, Some(&foe), &[], mesh);
    println!("scan: count {count}, passing the gates {}", scored.len());
    scored.sort_by(|a, b| a.total.total_cmp(&b.total));
    for s in scored.iter().take(5) {
        let t = s.terms;
        println!(
            "  {:?} total {:.4}: Dc {:.1}, Hc {:.1}, Dt {:.1}, Ht {:.1}, z9 {:.2}, los {}{}",
            s.point,
            s.total,
            t.dc,
            t.hc,
            t.dt,
            t.ht,
            t.z9,
            s.los,
            if t.goal_keep { ", kept goal x1.5" } else { "" }
        );
    }
    let by_choose = tactics::choose(&scored);
    let by_pick = tactics::pick(&rec, data, &me, Some(&foe), &[], mesh);
    match (by_choose, by_pick) {
        (Some(c), Some(p)) => {
            let s = scored.iter().find(|s| s.point == c.point).copied();
            if let Some(s) = s {
                println!(
                    "winner: {:?}, Dt {:.2}, Dc {:.2}, score {:.4} (a {:.4} b {:.4} c {:.4}, 0.1 term {:.4}), LOS {}",
                    c.point, s.terms.dt, s.terms.dc, s.total, s.terms.a_term, s.terms.b_term, s.terms.c_term, s.terms.hc_term, c.los
                );
            }
            println!("winner (pick, lazy): {:?} los {}; same point: {}", p.point, p.los, c.point == p.point);
        }
        _ => println!("no winner: no candidate passes the gates"),
    }

    let mut r = Record::new();
    r.goal = GOAL_HOME;
    tactics::update(&mut r, data, &me, Some(&foe), &[], Some(mesh), 0.032);
    println!("update (one step of 0.032 s): valid {}, point {:?}, goal {:08x}, los {}", r.valid, r.point, r.goal, r.los);

    // Diagnostics if the post pass removed most candidates: three examples with owner and locate.
    if kept.len() * 2 < owners.len() {
        println!("post pass removed most candidates; examples (point, owner incenter, triangle found by locate):");
        for (p, owner) in owners.iter().filter(|(p, o)| mesh.locate(*p) != Some(*o)).take(3) {
            let ic = mesh.tri_incenter(*owner);
            let found = mesh.locate(*p);
            println!("  point {p:?}, owner {owner} incenter [{:.2}, {:.2}], locate {found:?}", ic[0], ic[1]);
        }
    }
}
