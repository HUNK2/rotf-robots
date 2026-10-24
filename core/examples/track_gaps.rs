//! Which clip channels leave out a position, turn or scale track, per pack and clip list, and
//! how many of those sit on a node whose rest (bind) has a scale away from 1. The game starts a
//! channel's output at no move, no turn and scale 1 (`0053b520`) and writes the whole of it for
//! every channel (`00544e00`); the port's samplers keep the rest for an unkeyed track. This says
//! where the two differ. Read-only. For Breakaway's gun (the user's report, 2026-10-25).
//! `cargo run -p tf2-core --example track_gaps -- [pack...]`

use std::path::Path;

use tf2_core::formats::anim::AnimRef;
use tf2_core::formats::scene::ObjectDef;

const ROSTER: [&str; 13] = ["bumblebee", "optimus", "ratchet", "ironhide", "megatron", "longhaul", "audir8", "jetfire",
    "firestorm", "starscream", "blackout", "optimusflight", "megatronflight"];

fn survey(what: &str, object: &ObjectDef, clips: &[&AnimRef]) {
    let (mut channels, mut no_pos, mut no_rot, mut no_scale, mut scaled) = (0, 0, 0, 0, Vec::new());
    let (mut off_pos, mut off_rot) = (Vec::<String>::new(), Vec::<String>::new());
    for r in clips {
        let Some(clip) = &r.clip else { continue };
        for (&name, channel) in clip.names.iter().zip(&clip.channels) {
            let Some(node) = object.nodes.iter().find(|n| n.name_hash == name) else { continue };
            channels += 1;
            let b = node.bind;
            let unit = |row: [f32; 3]| { let l = (row[0] * row[0] + row[1] * row[1] + row[2] * row[2]).sqrt().max(1e-6); row.map(|v| v / l) };
            let turned = [b[0], b[1], b[2]].iter().enumerate().any(|(i, &row)| unit(row).iter().enumerate().any(|(j, &v)| (v - if i == j { 1.0 } else { 0.0 }).abs() > 0.01));
            let moved = b[3].iter().any(|v| v.abs() > 0.01);
            if channel.position.sample(0.0, clip.length, false).is_none() {
                no_pos += 1;
                if moved && name != tf2_core::formats::anim::ROOT_CHANNEL {
                    off_pos.push(format!("{}/{}", r.id, node.name));
                }
            }
            if channel.rotation.sample(0.0, clip.length, false).is_none() {
                no_rot += 1;
                if turned && name != tf2_core::formats::anim::ROOT_CHANNEL {
                    off_rot.push(format!("{}/{}", r.id, node.name));
                }
            }
            if channel.scale.sample(0.0, clip.length, false).is_none() {
                no_scale += 1;
                let b = node.bind;
                let s = [b[0], b[1], b[2]].map(|row| (row[0] * row[0] + row[1] * row[1] + row[2] * row[2]).sqrt());
                if s.iter().any(|v| (v - 1.0).abs() > 0.01) {
                    scaled.push(format!("{}/{} {:.2}", r.id, node.name, s[0].min(s[1]).min(s[2])));
                }
            }
        }
    }
    if channels == 0 {
        return;
    }
    scaled.sort();
    scaled.dedup();
    println!("  {what}: {channels} channels; no position {no_pos} ({} on a moved rest), no turn {no_rot} ({} on a turned rest), no scale {no_scale}, of which on a scaled rest {}{}",
        off_pos.len(), off_rot.len(),
        scaled.len(), if scaled.is_empty() { String::new() } else { format!(": {}", scaled.iter().take(6).cloned().collect::<Vec<_>>().join(", ")) });
    for (label, list) in [("moved", &off_pos), ("turned", &off_rot)] {
        if !list.is_empty() {
            println!("    {label}: {}", list.iter().take(6).cloned().collect::<Vec<_>>().join(", "));
        }
    }
}

fn main() {
    let dir = tf2_core::install::dir_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let packs: Vec<&str> = if args.is_empty() { ROSTER.to_vec() } else { args.iter().map(String::as_str).collect() };
    for pack in packs {
        let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
        println!("== {pack}");
        survey("robot", &data.robot, &data.animations.sets.values().flatten().collect::<Vec<_>>());
        if let Some(vehicle) = &data.vehicle {
            survey("vehicle", vehicle, &data.vehicle_animations.iter().collect::<Vec<_>>());
        }
        for (i, (object, clips)) in data.weapon_objects.iter().zip(&data.weapon_animations).enumerate() {
            if let Some(object) = object {
                survey(&format!("weapon {i}"), object, &clips.iter().collect::<Vec<_>>());
            }
        }
        for (i, w) in data.melee_weapons.iter().enumerate() {
            if let Some(object) = &w.object {
                survey(&format!("melee {i}"), object, &w.clips.iter().collect::<Vec<_>>());
            }
        }
    }
}
