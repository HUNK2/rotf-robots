//! A character's weapon objects' own clips: for each, its clips and, for every node a clip has a
//! channel for, its local place and turn at the first tick (`Channel::local`, the game's rule for
//! unkeyed parts). For Ironhide's left cannon smoke (the user's report, 2026-10-25).
//! `cargo run -p tf2-core --example weapon_object_clips -- <pack> [weapon index...]`

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = tf2_core::install::dir_string();
    let data = tf2_core::character::load(Path::new(&dir), &args[0]).expect("character");
    let only: Vec<usize> = args[1..].iter().filter_map(|a| a.parse().ok()).collect();
    for (i, (object, clips)) in data.weapon_objects.iter().zip(&data.weapon_animations).enumerate() {
        if !only.is_empty() && !only.contains(&i) { continue }
        let Some(object) = object else { continue };
        println!("== weapon object {i}: {} clips", clips.len());
        for r in clips {
            let Some(clip) = &r.clip else { continue };
            println!("  {} ({} ticks)", r.id, clip.length);
            for n in &object.nodes {
                let Some(local) = clip.channel(n.name_hash).and_then(|c| c.local(0.0, clip.length, false)) else { continue };
                println!("    {:<20} p {:?} q {:?}", n.name, local.0, local.1);
            }
        }
    }
}
