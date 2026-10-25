//! Measures whether the nav mesh's heights agree with the mission's collision ground (the known risk of
//! the unpublished notes for climbing), then counts the climbable walls of a
//! mission with the climb's own test (`climb::find` on `LevelWorld::ledge_from_mesh`) and lists test spots near
//! the spawn. Writes nothing.
//! `cargo run -p tf2-core --example ledge_check -- [MissionName]`
//! Default: `USA_East_LTrain_A_Escort`. Reads the install.
//!
//! The mesh is built as `game/src/main.rs` `build_nav` builds it: every pack's silhouettes, one `NavMesh::build`.
//! The collision is `LevelWorld::load_packs` on the same packs, with the mesh given by `with_nav`.
//! Bumblebee's radius, height and jump height are read from his tuning (`tuning::load`), as the game does.
//!
//! A directed marked edge (triangle `t`, edge `k`, neighbour `n`) is seen from `t`: `t` the near side, `n` the far side.

use std::path::PathBuf;
use std::sync::Arc;

use glam::{Vec2, Vec3};
use tf2_core::climb;
use tf2_core::formats::hash::crc32;
use tf2_core::formats::missionlist::MissionList;
use tf2_core::level_world::{LevelWorld, NO_FLOOR};
use tf2_core::navmesh::{self, NavMesh, Silhouette, FLAG_CAN_NOT_CLIMB};
use tf2_core::sim::World;

/// The mesh's open top is f32::MAX (FLT_MAX in navmesh.rs).
const OPEN: f32 = 1.0e30;
const SPAWN: [f32; 2] = [-215.12, -84.0];

fn finite(z: f32) -> bool {
    z.abs() < OPEN
}

fn stats(name: &str, errs: &mut [f32]) {
    if errs.is_empty() {
        println!("{name}: none");
        return;
    }
    errs.sort_by(|a, b| a.abs().total_cmp(&b.abs()));
    let n = errs.len();
    let within = errs.iter().filter(|e| e.abs() <= 0.25).count();
    println!(
        "{name}: n={n}, median |err| {:.3} m, worst |err| {:.3} m, within 0.25 m {within}/{n} ({:.1}%)",
        errs[n / 2].abs(),
        errs[n - 1].abs(),
        100.0 * within as f32 / n as f32
    );
}

fn main() {
    let game = PathBuf::from(tf2_core::install::dir_string());
    let name = std::env::args().nth(1).unwrap_or_else(|| "USA_East_LTrain_A_Escort".into());
    let list = MissionList::load(&game).expect("mission list");
    let mission = list.by_hash(crc32(name.as_bytes())).unwrap_or_else(|| panic!("no mission {name}"));
    let packs: Vec<PathBuf> = list.pack_paths(mission).iter().map(|p| game.join(p)).collect();
    let tuning = tf2_core::tuning::load(&game, "Bumblebee").expect("tuning");

    let mut sils: Vec<Silhouette> = Vec::new();
    for pack in &packs {
        let (found, _) = navmesh::load(pack).expect("nav silhouettes");
        sils.extend(found);
    }
    let mesh: NavMesh = NavMesh::build(None, &sils).expect("nav mesh");
    let mesh = Arc::new(mesh);
    let level = LevelWorld::load_packs(&game, &packs).expect("collision").with_nav(Some(mesh.clone()));

    let tris: Vec<usize> = mesh.triangles().collect();
    let constant = tris.iter().map(|&t| mesh.tri_floor(t)).filter(|z| finite(*z)).fold(f32::MAX, f32::min);
    println!("{name}: {} live triangles, {} silhouettes, open-ground floor constant {constant:.3}", tris.len(), sils.len());

    // 1. Triangle floors against the collision ground under each incenter. The open-ground constant is below the
    //    ground, so those are probed from 1000 (the spawn check's start, which can catch a roof).
    let step = (tris.len() / 500).max(1);
    let (mut on_constant, mut on_other, mut open, mut no_ground, mut sampled) = (Vec::new(), Vec::new(), 0usize, 0usize, 0usize);
    for &t in tris.iter().step_by(step).take(500) {
        sampled += 1;
        let f = mesh.tri_floor(t);
        if !finite(f) {
            open += 1;
            continue;
        }
        let [x, y] = mesh.tri_incenter(t);
        let on_open = (f - constant).abs() < 1e-3;
        let g = level.floor(x, y, if on_open { 1000.0 } else { f + 2.0 });
        if g <= NO_FLOOR {
            no_ground += 1;
            continue;
        }
        if on_open {
            on_constant.push(f - g);
        } else {
            on_other.push(f - g);
        }
    }
    println!("sample of {sampled} triangles: {open} open tops, {no_ground} with no collision under them");
    stats("floor = open-ground constant, error mesh minus collision", &mut on_constant);
    stats("floor = a silhouette top, error mesh minus collision", &mut on_other);

    // 2. Directed marked edges: the far floor against the collision top just beyond the edge, and the near floor
    //    against the collision ground just in front of it (both probes from the far floor plus 1 m).
    let mut directed = 0usize;
    let mut top_err = Vec::new();
    let mut bottom_err = Vec::new();
    for &t in &tris {
        for k in 0..3 {
            if !mesh.edge_marked(t, k) {
                continue;
            }
            let Some(n) = mesh.neighbour(t, k) else { continue };
            directed += 1;
            let far_floor = mesh.tri_floor(n);
            if !finite(far_floor) || mesh.tri_flags(n) as u32 & FLAG_CAN_NOT_CLIMB != 0 {
                continue;
            }
            let (a, b) = mesh.edge_points(t, k);
            let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            let mut perp = [-dy / len, dx / len];
            let [ix, iy] = mesh.tri_incenter(t);
            if perp[0] * (ix - mid[0]) + perp[1] * (iy - mid[1]) < 0.0 {
                perp = [-perp[0], -perp[1]];
            }
            let near_pt = [mid[0] + perp[0] * 0.5, mid[1] + perp[1] * 0.5];
            let far_pt = [mid[0] - perp[0] * 0.5, mid[1] - perp[1] * 0.5];
            let top = level.floor(far_pt[0], far_pt[1], far_floor + 1.0);
            let ground = level.floor(near_pt[0], near_pt[1], far_floor + 1.0);
            if top > NO_FLOOR {
                top_err.push(far_floor - top);
            }
            if ground > NO_FLOOR && finite(mesh.tri_floor(t)) {
                bottom_err.push(mesh.tri_floor(t) - ground);
            }
        }
    }
    println!("marked directed edges: {directed} (each physical edge is seen from both sides)");
    stats("far floor minus collision top just beyond the edge", &mut top_err);
    stats("near floor minus collision ground just in front of it", &mut bottom_err);

    // 3. Climbable walls: from each directed marked edge's near side, 1 m out, facing the wall, the climb's own
    //    test. Listed by distance from the escort spawn.
    let (radius, height, min_height) = (tuning.robot_radius, tuning.robot_height, tuning.jump.standing.height);
    println!("Bumblebee: radius {radius:.3}, height {height:.3}, jump height {min_height:.3}");
    let mut walls: Vec<(f32, [f32; 3], f32, [f32; 2], f32)> = Vec::new();
    let mut corners = Vec::new();
    for &t in &tris {
        for k in 0..3 {
            if !mesh.edge_marked(t, k) {
                continue;
            }
            let Some(n) = mesh.neighbour(t, k) else { continue };
            let far_floor = mesh.tri_floor(n);
            if !finite(far_floor) {
                continue;
            }
            let (a, b) = mesh.edge_points(t, k);
            let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            let mut perp = [-dy / len, dx / len];
            let [ix, iy] = mesh.tri_incenter(t);
            if perp[0] * (ix - mid[0]) + perp[1] * (iy - mid[1]) < 0.0 {
                perp = [-perp[0], -perp[1]];
            }
            let stand = [mid[0] + perp[0] * 1.0, mid[1] + perp[1] * 1.0];
            // The ground under the stand point, from the spawn check's start (it can catch a roof overhead).
            let ground = level.floor(stand[0], stand[1], 1000.0);
            if ground <= NO_FLOOR {
                continue;
            }
            let facing = [-perp[0], -perp[1]];
            let pos = Vec3::new(stand[0], stand[1], ground);
            if let Some(wall) = climb::find(&level, pos, Vec2::new(facing[0], facing[1]), radius, height, min_height) {
                let d = ((mid[0] - SPAWN[0]).powi(2) + (mid[1] - SPAWN[1]).powi(2)).sqrt();
                let foot = [mid[0] + perp[0] * 0.5, mid[1] + perp[1] * 0.5, ground];
                walls.push((d, foot, ground, facing, wall.height));
                if wall.width > wall.base_width {
                    corners.push((d, pos, facing, wall));
                }
            }
        }
    }
    println!("climbable walls (climb::find from 1 m in front of a marked edge): {} directed edges", walls.len());
    let within: Vec<_> = walls.iter().filter(|w| w.0 <= 30.0).collect();
    println!("within 30 m of the spawn ({:.2}, {:.2}): {}", SPAWN[0], SPAWN[1], within.len());
    walls.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut picked: Vec<&(f32, [f32; 3], f32, [f32; 2], f32)> = Vec::new();
    for w in walls.iter().filter(|w| w.0 <= 30.0) {
        let apart = picked.iter().all(|p| ((p.1[0] - w.1[0]).powi(2) + (p.1[1] - w.1[1]).powi(2)).sqrt() >= 5.0);
        if apart {
            picked.push(w);
        }
        if picked.len() == 3 {
            break;
        }
    }
    for (i, w) in picked.iter().enumerate() {
        println!(
            "spot {}: foot of the wall ({:.3}, {:.3}, {:.3}); stand 1 m in front ({:.3}, {:.3}, {:.3}), facing ({:.3}, {:.3}); wall height {:.3} m; {:.1} m from the spawn",
            i + 1,
            w.1[0],
            w.1[1],
            w.1[2],
            w.1[0] - w.3[0] * 0.5,
            w.1[1] - w.3[1] * 0.5,
            w.2,
            w.3[0],
            w.3[1],
            w.4,
            w.0
        );
    }
    corners.sort_by(|a, b| a.0.total_cmp(&b.0));
    println!("shallow-corner wall queries: {}; climb edge limit {:.3} m", corners.len(), tuning.climb.edge_limit_dist);
    let mut selected = Vec::<Vec3>::new();
    for (distance, pos, facing, wall) in &corners {
        if selected.iter().any(|p| p.distance(*pos) < 5.0) {
            continue;
        }
        selected.push(*pos);
        let local = wall.local(*pos);
        let yaw = (-facing[0].atan2(facing[1])).to_degrees();
        println!(
            "corner spot {}: --start {:.3},{:.3},{:.3}; ground {:.3}; local x {:.3}; base width {:.3}; width {:.3}; extensions L {:.3} R {:.3}; wall height {:.3}; {:.1} m from spawn",
            selected.len(), pos.x, pos.y, yaw, pos.z, local.x, wall.base_width, wall.width,
            wall.left_extension, wall.width - wall.base_width - wall.left_extension, wall.height, distance,
        );
        if selected.len() == 3 { break; }
    }
}
