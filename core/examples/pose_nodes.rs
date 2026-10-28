//! Prints where a clip puts some of a robot's nodes, in his own frame (`pose::Rig::pose`), for
//! checking a port's posing against the reference.
//!
//!     cargo run --release -p tf2-core --example pose_nodes -- <pack> <set> <clip> <tick> [node...]

use tf2_core::pose::Rig;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!("usage: pose_nodes <pack> <set> <clip> <tick> [node...]");
        return;
    }
    let dir = tf2_core::install::dir();
    let data = tf2_core::character::load(dir, &args[0]).expect("pack");
    let tick: f32 = args[3].parse().expect("tick");
    let r = data.animations.find(&args[1], &args[2]).expect("clip");
    let clip = r.clip.as_ref().expect("keys");
    let rig = Rig::new(&data.robot);
    // TF2_LAYER="<clip> <tick> <weight>": the weapon layer's clip laid over it (pose_layer).
    let pose = match std::env::var("TF2_LAYER") {
        Ok(layer) => {
            let p: Vec<&str> = layer.split_whitespace().collect();
            let o = data.animations.find("WeaponPartialSet", p[0]).expect("layer clip");
            let over = o.clip.as_ref().expect("keys");
            rig.pose_layer(Some((clip, r.looping, tick)), over, o.looping, p[1].parse().unwrap(), o.additive, p[2].parse().unwrap())
        }
        Err(_) => rig.pose(clip, r.looping, tick),
    };
    for (i, node) in data.robot.nodes.iter().enumerate() {
        if args.len() > 4 && !args[4..].contains(&node.name) {
            continue;
        }
        let t = pose[i].translation;
        let (s, _, _) = pose[i].to_scale_rotation_translation();
        println!("{:3} {:24} {:9.4} {:9.4} {:9.4}  scale {:.4} {:.4} {:.4}", i, node.name, t.x, t.y, t.z, s.x, s.y, s.z);
    }
}
