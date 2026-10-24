//! One clip's arm chains in the model's space (forward +y, up +z), without a window: each
//! node's place and where its three axes point, and, from the hand on, the line a gun on the
//! weapon node would fire along. With `AIM=<-1..1>` the clip's Down (negative) or Up offset
//! is laid on by that share, as the aim lays it. For Breakaway's rifle pointing down and Long
//! Haul's arms in weapon mode (the user's report, 2026-10-25).
//! `cargo run -p tf2-core --example arm_chain -- <pack> <Set> <Id> [tick] [side L|R]`

use std::path::Path;

fn main() {
    let dir = tf2_core::install::dir_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pack = args.first().map_or("firestorm", String::as_str);
    let set = args.get(1).map_or("WeaponSet", String::as_str);
    let id = args.get(2).map_or("Weapon_Idle", String::as_str);
    let tick: f32 = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(0.0);
    let side = args.get(4).map_or("R", String::as_str);
    let aim: f32 = std::env::var("AIM").ok().and_then(|a| a.parse().ok()).unwrap_or(0.0);
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
    let r = data.animations.find(set, id).expect("clip");
    let clip = r.clip.as_ref().expect("clip file");
    let rig = tf2_core::pose::Rig::new(&data.robot);
    let pose = match r.aim_offsets.get(if aim > 0.0 { 1 } else { 0 }).filter(|_| aim != 0.0) {
        Some(over) => rig.pose_offset(clip, r.looping, tick, over, aim.abs()),
        None => rig.pose(clip, r.looping, tick),
    };
    // The chain from the root to the deepest node whose name ends in this side.
    let suffix = format!("_{side}");
    // `NODES=a,b` names the chains' ends instead.
    let named: Vec<String> = std::env::var("NODES").map(|n| n.split(',').map(str::to_owned).collect()).unwrap_or_default();
    let ends: Vec<usize> = data.robot.nodes.iter().enumerate()
        .filter(|(_, n)| if named.is_empty() { n.name.ends_with(&suffix) && (n.name.contains("Weapon") || n.name.contains("Hand")) } else { named.contains(&n.name) })
        .map(|(i, _)| i).collect();
    for end in ends {
        let mut chain = vec![end];
        while let Some(p) = data.robot.nodes[*chain.last().unwrap()].parent {
            chain.push(p);
        }
        chain.reverse();
        println!("-- {} ({} keyed: {})", data.robot.nodes[end].name, set, id);
        for &i in &chain {
            let n = &data.robot.nodes[i];
            let m = pose[i].matrix3;
            let d = |v: glam::Vec3A| { let v = v.normalize(); format!("({:+.2} {:+.2} {:+.2})", v.x, v.y, v.z) };
            println!("  {:<28} {} at ({:+.2} {:+.2} {:+.2}) x {} y {} z {}", n.name,
                if clip.channel(n.name_hash).is_some() { "k" } else { " " },
                pose[i].translation.x, pose[i].translation.y, pose[i].translation.z, d(m.x_axis), d(m.y_axis), d(m.z_axis));
        }
    }
}
