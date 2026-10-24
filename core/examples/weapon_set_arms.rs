//! Which arm nodes each weapon-set clip keys, and whether it carries the aim's Down / Up
//! offsets: a clip that leaves an arm unkeyed shows it at whatever is under it. For Long Haul's
//! and Breakaway's arms in weapon mode (the user's report, 2026-10-25).
//! `cargo run -p tf2-core --example weapon_set_arms -- [pack] [Set] [name filter]`

use std::path::Path;

fn main() {
    let dir = tf2_core::install::dir_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pack = args.first().map_or("longhaul", String::as_str);
    let set = args.get(1).map_or("WeaponSet", String::as_str);
    let filter = args.get(2).map_or("Arm|Hand|Shoulder|Clavicle|Elbow|Wrist|Weapon|Gun", String::as_str);
    let words: Vec<&str> = filter.split('|').collect();
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
    let arms: Vec<(u32, &str)> = data.robot.nodes.iter()
        .filter(|n| words.iter().any(|w| n.name.contains(w)))
        .map(|n| (n.name_hash, n.name.as_str())).collect();
    println!("{pack}: {} arm nodes: {}", arms.len(), arms.iter().map(|a| a.1).collect::<Vec<_>>().join(" "));
    for r in data.animations.sets.get(set).into_iter().flatten() {
        let Some(clip) = &r.clip else { continue };
        let missing: Vec<&str> = arms.iter().filter(|(h, _)| clip.channel(*h).is_none()).map(|a| a.1).collect();
        let offsets: Vec<String> = r.aim_offsets.iter().map(|o| {
            let keyed = arms.iter().filter(|(h, _)| o.channel(*h).is_some()).count();
            format!("{} ch ({keyed} arm)", o.channels.len())
        }).collect();
        println!("{:<28} {:>3} ch, offsets [{}], arm nodes unkeyed: {}", r.id, clip.channels.len(), offsets.join(", "),
            if missing.is_empty() { "none".to_owned() } else { missing.join(" ") });
    }
}
