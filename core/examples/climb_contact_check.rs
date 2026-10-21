//! Diagnose the escort facade's climbing contacts without launching either game.
//! `cargo run -p tf2-core --example climb_contact_check`
//! Reads the user's install; prints radial probes and simulated shimmy travel.
//! `--pencil` tries a diagnostic continuous pencil slide; it is not the game's mover.
use glam::{Vec2, Vec3};
use std::{path::PathBuf, sync::Arc};
use tf2_core::{
    climb,
    formats::{hash::crc32, missionlist::MissionList},
    level_world::{CHARACTER_MASK, LevelWorld},
    navmesh::{self, NavMesh},
    sim::World,
    tuning::{self, Actions},
};

// [stand-in] Diagnostic only: compare a continuous pencil cast to the radial proxy.
// Eight slide passes and a 0.01 clearance are trial settings, not a port of
// the original contact solver. This mover is not connected to game movement.
fn sweep_slide(level: &LevelWorld, from: Vec3, wanted: Vec3, radius: f32, height: f32) -> Vec3 {
    let mut pos = from;
    let mut travel = wanted - from;
    for _ in 0..8 {
        if travel.length_squared() < 1e-8 {
            break;
        }
        let hit = level
            .world
            .pencil_cast(pos, pos + travel, radius, height)
            .into_iter()
            .filter(|h| {
                !level.world.primitives[h.primitive].released
                    && level.masks[h.primitive].send & CHARACTER_MASK != 0
                    && travel.dot(h.normal) < -1e-5
            })
            .min_by(|a, b| a.lambda.total_cmp(&b.lambda));
        let Some(hit) = hit else {
            pos += travel;
            break;
        };
        let moved = travel.normalize() * hit.lambda;
        pos += moved + hit.normal * 0.01;
        travel -= moved;
        travel -= hit.normal * travel.dot(hit.normal).min(0.0);
    }
    pos
}

fn lost_ledge_path(level: &LevelWorld, mesh: &NavMesh, from: Vec3, dir: Vec2, reach: f32) {
    // Historical diagnostic: show the former distance-only walker for comparison.
    // The current LevelWorld query also has visited/floor guards and an edge-band fallback.
    println!("former distance-only ray path (not the current ledge query):");
    let Some(mut tri) = mesh.locate(from) else {
        println!("  outside nav mesh");
        return;
    };
    let origin = from.truncate();
    let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
    let mut passed = 0.0;
    for _ in 0..12 {
        let mut exit: Option<(f32, usize, Vec2)> = None;
        for k in 0..3 {
            let (a, b) = mesh.edge_points(tri, k);
            let (a, b) = (Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1]));
            let e = b - a;
            let den = cross(dir, e);
            if den.abs() < 1e-9 {
                continue;
            }
            let s = cross(a - origin, e) / den;
            let u = cross(a - origin, dir) / den;
            if s > passed + 1e-6
                && (-1e-6..=1.0 + 1e-6).contains(&u)
                && exit.is_none_or(|x| s < x.0)
            {
                let mut normal = Vec2::new(-e.y, e.x).normalize_or_zero();
                let ic = Vec2::from_array(mesh.tri_incenter(tri));
                if normal.dot(ic - (a + b) * 0.5) < 0.0 {
                    normal = -normal;
                }
                exit = Some((s, k, normal));
            }
        }
        let Some((s, k, normal)) = exit else {
            println!(
                "  no next intersection in tri {tri}, origin {origin:?}, vertices {:?}",
                mesh.tri_vertices(tri).map(|v| mesh.vertex(v))
            );
            break;
        };
        let Some(far) = mesh.neighbour(tri, k) else {
            println!("  mesh boundary");
            break;
        };
        let hit = origin + dir * s;
        let far_z = mesh.tri_floor(far);
        let beyond = hit - normal * 0.5;
        let top = level.floor(beyond.x, beyond.y, far_z.max(from.z) + 1.0);
        println!(
            "  tri {tri} floor {:.3} -> {far} floor {far_z:.3}, flags {}, at {s:.3}, normal {normal:.3?}, collision top {top:.3}",
            mesh.tri_floor(tri),
            mesh.tri_flags(far)
        );
        if s > reach || far_z.abs() >= 1e30 || top > from.z {
            break;
        }
        passed = s;
        tri = far;
    }
}

fn main() {
    let pencil_trial = std::env::args().any(|a| a == "--pencil");
    let game = PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into()));
    let list = MissionList::load(&game).expect("missions");
    let mission = list
        .by_hash(crc32(b"USA_East_LTrain_A_Escort"))
        .expect("escort");
    let packs: Vec<_> = list
        .pack_paths(mission)
        .iter()
        .map(|p| game.join(p))
        .collect();
    let mut silhouettes = Vec::new();
    for pack in &packs {
        silhouettes.extend(navmesh::load(pack).expect("nav").0);
    }
    let mesh = Arc::new(NavMesh::build(None, &silhouettes).expect("mesh"));
    let level = LevelWorld::load_packs(&game, &packs)
        .expect("collision")
        .with_nav(Some(mesh.clone()));
    let mut t = tuning::load(&game, "Bumblebee").expect("tuning");
    let data = tf2_core::character::load(&game, "bumblebee").expect("character");
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing);

    let radius = t.robot_radius;
    let height = t.robot_height;
    let start = Vec3::new(-219.733 + 9.8 * 0.016, -67.117, 15.299);
    let ball = radius * (std::f32::consts::PI / 12.0).sin() * 1.05;
    let reach = radius - ball;
    println!("pinned position {start:?}, radius {radius}, height {height}, ball {ball}");
    for fraction in [0.35_f32, 0.6, 0.9] {
        let from = start + Vec3::Z * (height * fraction).max(tf2_core::sim::STEP + ball + 0.05);
        for i in 0..12 {
            let angle = i as f32 / 12.0 * std::f32::consts::TAU;
            let dir = Vec3::new(angle.cos(), angle.sin(), 0.0);
            if let Some(h) =
                level.sweep_from_outside(from, from + dir * reach, ball, CHARACTER_MASK)
            {
                let p = h.point - start;
                let front = p.y > 0.0 && p.x * p.x + p.y * p.y < 2.0 * p.y * p.y;
                println!(
                    "probe {fraction:.2}/{i}: short {:.4}, point {:?}, normal {:?}, front {front}, prim {} source {}",
                    reach - h.lambda,
                    h.point,
                    h.normal,
                    h.primitive,
                    level.world.primitives[h.primitive].source
                );
            }
        }
    }
    // Reproduce the active shimmy using its actual installed clips and the current world.
    let continuous = pencil_trial;
    for dt in [0.008_f32, 0.016, 0.032] {
        let mut pos = Vec3::new(-221.457, -66.654, 15.296);
        let mut yaw = 0.0_f32;
        let wall = climb::find(&level, pos, Vec2::Y, radius, height, t.jump.standing.height)
            .expect("wall");
        let mut c = climb::Climb::enter(wall, &mut pos, Vec3::ZERO, &t, radius, true);
        println!("shimmy dt {dt}, continuous {continuous}");
        for step in 0..(4.0 / dt) as usize {
            let facing = Vec2::new(-yaw.sin(), yaw.cos());
            let wall = climb::find(&level, pos, facing, radius, height, t.jump.standing.height);
            let old = pos;
            let front_contact = if continuous {
                level
                    .world
                    .pencil_cast(pos, pos + facing.extend(0.0) * 0.01, radius + 0.01, height)
                    .iter()
                    .any(|h| {
                        level.masks[h.primitive].send & CHARACTER_MASK != 0
                            && climb::contact_ahead(h.point - pos, facing)
                    })
            } else {
                level.front_contact(pos, facing, radius, height)
            };
            let body = climb::Body {
                pos: &mut pos,
                yaw: &mut yaw,
                radius,
                height,
                on_ground: false,
                front_contact,
            };
            let (vel, msg) = climb::update(&mut c, body, Vec2::X, wall, &t, dt);
            let desired = pos + vel * dt;
            let pushed = if continuous {
                pos = sweep_slide(&level, old, desired, radius, height);
                pos.distance(desired) > 0.0001
            } else {
                pos = desired;
                level.push_out(&mut pos, radius, height)
            };
            if step % (0.25 / dt) as usize == 0 || msg.is_some() {
                println!(
                    "step {step}: {:?} pos {pos:.3?}, delta {:.3?}, desired {:.3?}, pushed {pushed}, front {front_contact}, yaw {:.1}, msg {msg:?}",
                    c.phase,
                    pos - old,
                    desired - old,
                    yaw.to_degrees()
                );
            }
            if msg.is_some() {
                let start = old + Vec3::Z * climb::body_offset(height);
                println!(
                    "lost wall at {old:.3?}, raw ledge {:?}",
                    level.ledge(start, facing, (radius + 2.0) * std::f32::consts::SQRT_2)
                );
                lost_ledge_path(
                    &level,
                    &mesh,
                    start,
                    facing,
                    (radius + 2.0) * std::f32::consts::SQRT_2,
                );
                break;
            }
        }
    }
}
