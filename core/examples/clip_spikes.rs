//! One-frame spikes in a character's clips: every node of the robot and vehicle models whose place
//! (in the model's space, posed by the port's rig) at a half tick stands more than a threshold away
//! from the midpoint of its places half a tick before and after. A part that flies off for a frame
//! of a change of form shows here. For Grindor's piece "detached" for a frame as he changes back
//! (the user's report, 2026-10-25; the 2026-10-24 rotor housing spike in `Trans_R2V_A`).
//! `cargo run -p tf2-core --example clip_spikes -- <pack> [metres] [clip name filter]`

use std::path::Path;

use tf2_core::formats::anim::AnimRef;
use tf2_core::formats::scene::ObjectDef;
use tf2_core::pose::Rig;

fn scan(what: &str, object: &ObjectDef, clips: &[&AnimRef], threshold: f32, filter: &str) {
    let rig = Rig::new(object);
    for r in clips.iter().filter(|r| r.id.contains(filter)) {
        let Some(clip) = &r.clip else { continue };
        let at = |t: f32| rig.pose(clip, false, t.clamp(0.0, clip.length));
        let steps = (clip.length * 2.0) as usize;
        let mut worst: Vec<(f32, String, f32)> = Vec::new();
        for k in 1..steps {
            let t = k as f32 * 0.5;
            let (before, now, after) = (at(t - 0.5), at(t), at(t + 0.5));
            for (n, node) in object.nodes.iter().enumerate() {
                let mid = (before[n].translation + after[n].translation) * 0.5;
                let off = (now[n].translation - mid).length();
                if off > threshold {
                    worst.push((t, node.name.clone(), off));
                }
            }
        }
        worst.sort_by(|a, b| b.2.total_cmp(&a.2));
        for (t, name, off) in worst.iter().take(6) {
            println!("{what} {} tick {t:.1}: {name} {off:.2} m off its neighbours' midpoint", r.id);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pack = args.first().expect("pack");
    let threshold: f32 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(1.0);
    let filter = args.get(2).map_or("", String::as_str);
    let dir = tf2_core::install::dir_string();
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
    scan("robot", &data.robot, &data.animations.sets.values().flatten().collect::<Vec<_>>(), threshold, filter);
    if let Some(vehicle) = &data.vehicle {
        scan("vehicle", vehicle, &data.vehicle_animations.iter().collect::<Vec<_>>(), threshold, filter);
    }
}
