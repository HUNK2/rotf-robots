//! Runs the reference's ribbon trails (`ribbon.rs`) and skid marks (`skidmark.rs`) through set
//! sequences and prints what they draw, for a native port to compare its own with: one JSON line
//! per printed step. The sequences are formulas a port can repeat exactly (below).
//! `cargo run --release -p tf2-core --example trail_dump -- [steps]`
//!
//! Ribbon `r` of `RIBBONS` (links, trails, subdivisions, trigger speed, enabled at the start): at
//! step i (from 0) the time is t = 0.02 i, the update's dt is 0.012 + 0.008 (i mod 4), and link k
//! is at (2 sin(12t) + 0.3 k + 0.1 r, 20t + 0.05 k, 0.5 cos(8t) + 0.1 k) while i mod 50 < 30, else held
//! where it was at the last moving step (so the trigger speed tells). `Trigger` is sent before
//! steps 10 and 70, the stop before step 45. The eye is at (2, -5, 1). Every 5th step prints the
//! mesh's even vertices (each odd one is the same with the normal turned round): place and uv.
//!
//! Skid mark: his first tyre's (`point_length` 2, `point_width` 0.3, colour 0.75 grey). At step i
//! the tyre is at (3 sin(0.05 i), 0.9 i, 0.3), the contact's height 0.1 sin(0.1 i); it skids while
//! i mod 60 < 40; the wheel is off the ground while i mod 90 is 50 to 54. The message is sent
//! (`trigger` / `stop`), then `update` runs with the wheel's ground now and at the step before.
//! Every 5th step prints the quads: each corner's place, uv and alpha.

use glam::Vec3;
use tf2_core::ribbon::{Def, STOP, TRIGGER, Trail};
use tf2_core::skidmark::{SkidMark, SkidMarkDef};

const RIBBONS: [(usize, usize, usize, f32, bool); 3] = [(2, 8, 4, 0.5, false), (3, 8, 4, 0.5, false), (4, 12, 4, 0.0, true)];

fn main() {
    let steps: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(120);
    let eye = Vec3::new(2.0, -5.0, 1.0);
    for (r, &(links, trails, subdivisions, trigger_speed, enabled)) in RIBBONS.iter().enumerate() {
        let def = Def { name: 1, links: (0..links as u32).collect(), material: 0, trails, subdivisions, trigger_speed, enabled };
        let mut trail = Trail::new(&def);
        let mut held = Vec::new();
        for i in 0..steps {
            if i == 10 || i == 70 {
                trail.message(TRIGGER);
            }
            if i == 45 {
                trail.message(STOP);
            }
            let t = 0.02 * i as f32;
            let dt = 0.012 + 0.008 * (i % 4) as f32;
            if i % 50 < 30 || held.is_empty() {
                held = (0..links)
                    .map(|k| Vec3::new(2.0 * (12.0 * t).sin() + 0.3 * k as f32 + 0.1 * r as f32, 20.0 * t + 0.05 * k as f32, 0.5 * (8.0 * t).cos() + 0.1 * k as f32))
                    .collect();
            }
            trail.update(&def, held.clone(), dt);
            if i % 5 == 4 {
                let (vertices, _) = trail.mesh(&def, eye);
                let out: Vec<String> = vertices
                    .iter()
                    .step_by(2)
                    .map(|v| format!("[{},{},{},{},{}]", v.position.x, v.position.y, v.position.z, v.uv[0], v.uv[1]))
                    .collect();
                println!("{{\"ribbon\":{r},\"step\":{i},\"vertices\":[{}]}}", out.join(","));
            }
        }
    }
    let def = SkidMarkDef { node: 0, tyre: 0, point_length: 2.0, point_width: 0.3, lifetime: 10.0, colour: [0.75; 4], material: 0 };
    let mut mark = SkidMark::default();
    let mut was_on_ground = true;
    for i in 0..steps {
        let f = i as f32;
        let tyre = Vec3::new(3.0 * (0.05 * f).sin(), 0.9 * f, 0.3);
        let contact = 0.1 * (0.1 * f).sin();
        let on_ground = !(50..55).contains(&(i % 90));
        if i % 60 < 40 {
            mark.trigger(tyre, contact, f);
        } else {
            mark.stop();
        }
        mark.update(&def, tyre, contact, on_ground, was_on_ground, f);
        was_on_ground = on_ground;
        if i % 5 == 4 {
            let out: Vec<String> = mark
                .quads(&def)
                .iter()
                .flatten()
                .map(|v| format!("[{},{},{},{},{},{}]", v.pos.x, v.pos.y, v.pos.z, v.uv[0], v.uv[1], v.colour[3]))
                .collect();
            println!("{{\"skid\":0,\"step\":{i},\"corners\":[{}]}}", out.join(","));
        }
    }
}
