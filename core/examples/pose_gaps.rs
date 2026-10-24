//! Where the game's rule for a channel's missing tracks moves a part: in each pose clip (not
//! the offset clips) of the robot, the car, the weapon objects and the melee weapons, the
//! channels that key something but no turn on a node whose rest turns it, or no position on one
//! whose rest moves it, by how much (local to the parent). The game starts every channel at no
//! move and no turn (0053b520) and writes it whole (00544e00); a channel that keys nothing at all
//! keeps its rest in the port [assumed]. For Long Haul's and Breakaway's forearms (the user's
//! report, 2026-10-25). `cargo run -p tf2-core --example pose_gaps -- [degrees] [metres] [pack...]`
//! (`MOST=<n>` lists that many of each).

use std::path::Path;

use glam::Affine3A;
use tf2_core::formats::anim::AnimRef;
use tf2_core::formats::scene::ObjectDef;

fn survey(what: &str, object: &ObjectDef, clips: &[(&str, &AnimRef)], min_turn: f32, min_move: f32) {
    let nodes = &object.nodes;
    let world: Vec<Affine3A> = nodes.iter().map(|n| tf2_core::pose::affine(&n.bind)).collect();
    let local: Vec<(glam::Vec3, glam::Quat, glam::Vec3)> = nodes.iter().enumerate().map(|(i, n)| match n.parent {
        Some(p) => (world[p].inverse() * world[i]).to_scale_rotation_translation(),
        None => world[i].to_scale_rotation_translation(),
    }).collect();
    let (mut turns, mut moves) = (Vec::new(), Vec::new());
    for (set, r) in clips.iter().filter(|(_, r)| !r.additive) {
        let Some(clip) = &r.clip else { continue };
        for (&name, channel) in clip.names.iter().zip(&clip.channels) {
            if name == tf2_core::formats::anim::ROOT_CHANNEL || !channel.keys_any() { continue }
            let Some(i) = nodes.iter().position(|n| n.name_hash == name) else { continue };
            let (_, rot, pos) = local[i];
            let angle = rot.angle_between(glam::Quat::IDENTITY).to_degrees();
            if channel.rotation.sample(0.0, clip.length, false).is_none() && angle > min_turn {
                turns.push((angle, format!("{set}/{}/{}", r.id, nodes[i].name)));
            }
            if channel.position.sample(0.0, clip.length, false).is_none() && pos.length() > min_move {
                moves.push((pos.length(), format!("{set}/{}/{}", r.id, nodes[i].name)));
            }
        }
    }
    if turns.is_empty() && moves.is_empty() {
        return;
    }
    turns.sort_by(|a, b| b.0.total_cmp(&a.0));
    moves.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut nodes_turned: Vec<&str> = turns.iter().map(|t| t.1.rsplit('/').next().unwrap()).collect();
    nodes_turned.sort();
    nodes_turned.dedup();
    println!("  {what}: {} turn gaps over {min_turn} deg (nodes {}), {} position gaps over {min_move} m", turns.len(), nodes_turned.join(" "), moves.len());
    let most: usize = std::env::var("MOST").ok().and_then(|m| m.parse().ok()).unwrap_or(8);
    for (a, what) in turns.iter().take(most) { println!("   turn {a:6.1} deg  {what}"); }
    for (d, what) in moves.iter().take(most) { println!("   move {d:6.2} m    {what}"); }
}

fn main() {
    let dir = tf2_core::install::dir_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let min_turn: f32 = args.first().and_then(|a| a.parse().ok()).unwrap_or(5.0);
    let min_move: f32 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(0.05);
    let packs: Vec<&str> = if args.len() > 2 { args[2..].iter().map(String::as_str).collect() } else {
        vec!["bumblebee", "optimus", "ratchet", "ironhide", "megatron", "longhaul", "audir8", "jetfire",
            "firestorm", "starscream", "blackout", "optimusflight", "megatronflight", "devastator"]
    };
    for pack in packs {
        let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
        println!("== {pack}");
        let sets: Vec<(&str, &AnimRef)> = data.animations.sets.iter().flat_map(|(s, refs)| refs.iter().map(move |r| (s.as_str(), r))).collect();
        survey("robot", &data.robot, &sets, min_turn, min_move);
        if let Some(vehicle) = &data.vehicle {
            survey("vehicle", vehicle, &data.vehicle_animations.iter().map(|r| ("car", r)).collect::<Vec<_>>(), min_turn, min_move);
        }
        for (i, (object, clips)) in data.weapon_objects.iter().zip(&data.weapon_animations).enumerate() {
            if let Some(object) = object {
                survey(&format!("weapon {i}"), object, &clips.iter().map(|r| ("weapon", r)).collect::<Vec<_>>(), min_turn, min_move);
            }
        }
        for (i, w) in data.melee_weapons.iter().enumerate() {
            if let Some(object) = &w.object {
                survey(&format!("melee {i}"), object, &w.clips.iter().map(|r| ("melee", r)).collect::<Vec<_>>(), min_turn, min_move);
            }
        }
    }
}
