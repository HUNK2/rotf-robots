//! How far each ground robot's hands and feet reach in front of him in his climbing clips, against
//! the stand-off the climb holds him at (his `radius`, `climb.rs` `enter`, [game]): a limb whose
//! reach is well short of the radius cannot touch the wall. Prints, per clip of the ClimbSet, the
//! largest forward reach (y in the object's frame, which faces +y) of each hand and foot over the
//! clip. Read-only.
//! `cargo run -p tf2-core --example climb_reach -- [pack:AttributesName...]`

use std::path::Path;

const GROUND: [&str; 7] = [
    "bumblebee:Bumblebee", "optimus:Optimus", "ironhide:Ironhide", "ratchet:Ratchet", "megatron:Megatron",
    "longhaul:LongHaul", "audir8:Audi",
];

fn main() {
    let game = tf2_core::install::dir_string();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        args = GROUND.map(str::to_owned).to_vec();
    }
    for arg in args {
        let (pack, name) = arg.split_once(':').expect("pack:Name");
        let data = tf2_core::character::load(Path::new(&game), pack).expect("character");
        let t = tf2_core::tuning::load(Path::new(&game), name).expect("tuning");
        println!("== {pack}: radius {:.2}, height {:.2}", t.robot_radius, t.robot_height);
        let object = &data.robot;
        let limbs: Vec<(usize, &str)> = ["Hand_L", "Hand_R", "Foot_L", "Foot_R"]
            .iter()
            .filter_map(|n| Some((object.nodes.iter().position(|node| node.name == *n)?, *n)))
            .collect();
        let rig = tf2_core::pose::Rig::new(object);
        let Some(refs) = data.animations.sets.get("ClimbSet") else {
            println!("   no ClimbSet");
            continue;
        };
        for r in refs.iter().filter(|r| r.id.contains("Idle") || r.id.contains("ClimbUp") || r.id.contains("Shimmy")) {
            let Some(clip) = r.clip.as_ref() else { continue };
            let mut reach = vec![f32::MIN; limbs.len()];
            let mut tick = 0.0;
            while tick <= clip.length {
                let pose = rig.pose(clip, r.looping, tick);
                for (k, (node, _)) in limbs.iter().enumerate() {
                    reach[k] = reach[k].max(pose[*node].translation.y);
                }
                tick += 2.0;
            }
            let text: Vec<String> = limbs.iter().zip(&reach).map(|((_, n), y)| format!("{n} {y:.2}")).collect();
            // The root channel's place at the start, which the pose above leaves out.
            let root = clip
                .channel(tf2_core::formats::anim::ROOT_CHANNEL)
                .and_then(|c| c.position.sample(0.0, clip.length, r.looping));
            println!("   {:<22} {}  root {:?}", r.id, text.join("  "), root);
        }
    }
}
