//! Runs the reference's thrown pieces (`debris.rs`) through set launches and prints their flight, for
//! a native port to compare its own with: one JSON line per printed step.
//! `cargo run --release -p tf2-core --example debris_dump -- [steps]`
//!
//! Three throws (`DEFS`): Bumblebee's micro missile chunk as the export gives it (`round.debris[].throw`),
//! one that dies at rest with a set spin axis, a start angle and a spawn radius, and one that dies on its
//! first contact and goes inert. Each throws five pieces from one generator (seed 0x5eed): piece k at
//! (0.3 k, 0.1 k, 2 + 0.2 k), away from (0, 0, 2), pushed along (0, 1, 0). The floor is z = 0. Steps of
//! 0.032 s; every step prints each live piece's place, velocity, turn (x, y, z, w) and alpha.

use glam::Vec3;
use tf2_core::debris::{self, DebrisDef, Rng};

fn defs() -> [DebrisDef; 3] {
    let chunk = DebrisDef {
        speed: 45.0, speed_min: 25.0, rot_speed: 5.2359877, rot_speed_min: 0.0, spread: std::f32::consts::PI,
        start_angle: 0.0, radius_xy: 0.0, radius_z: 0.0, force_speed: 0.5, die_condition: debris::DIE_SPAWN_TIMER,
        die_time: 0.5, spin_type: 3523482240, linear_damping: 0.9995, angular_damping: 0.9995,
        die_type: debris::DIE_TYPE_FADE, inflict_damage: 7.0, override_collision: false,
    };
    let rest = DebrisDef {
        speed: 6.0, speed_min: 6.0, rot_speed: 2.0, rot_speed_min: 1.0, spread: 0.6, start_angle: 0.5,
        radius_xy: 0.4, radius_z: 0.2, force_speed: 2.0, die_condition: debris::DIE_REST_TIMER, die_time: 0.3,
        spin_type: 0xd694e056, linear_damping: 0.98, angular_damping: 0.95, die_type: debris::DIE_TYPE_HURT_OWNER,
        ..chunk
    };
    let impact = DebrisDef {
        speed: 3.0, speed_min: 1.0, spread: 0.0, die_condition: debris::DIE_IMPACT, die_time: 0.0,
        spin_type: 0x6483fd62, die_type: debris::DIE_TYPE_FADE_INERT, ..chunk
    };
    [chunk, rest, impact]
}

fn main() {
    let steps: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(120);
    for (d, def) in defs().iter().enumerate() {
        let mut rng = Rng(0x5eed);
        let mut pieces: Vec<Option<debris::Piece>> = (0..5).map(|k| {
            let k = k as f32;
            Some(debris::launch(def, Vec3::new(0.3 * k, 0.1 * k, 2.0 + 0.2 * k), Vec3::new(0.0, 0.0, 2.0), Vec3::Y, &mut rng))
        }).collect();
        for step in 0..steps {
            for (k, slot) in pieces.iter_mut().enumerate() {
                let Some(p) = slot else { continue };
                let alive = p.step(0.032, |_| Some(0.0));
                let t = p.turn;
                println!("{{\"def\":{d},\"piece\":{k},\"step\":{step},\"pos\":[{},{},{}],\"vel\":[{},{},{}],\"turn\":[{},{},{},{}],\"alpha\":{},\"alive\":{alive}}}",
                    p.pos.x, p.pos.y, p.pos.z, p.vel.x, p.vel.y, p.vel.z, t.x, t.y, t.z, t.w, p.alpha);
                if !alive { *slot = None; }
            }
        }
    }
}
