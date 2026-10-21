//! One robot clip's keys at one tick for a node and every node above it: the node's rest
//! relative to its parent, and the clip's position, turn, scale and own scale. Read-only.
//! `cargo run -p tf2-core --example chan_peek -- <pack> <Set> <Id> <tick> <node>`

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    let data = tf2_core::character::load(Path::new(&dir), &args[0]).expect("load");
    let r = data.animations.find(&args[1], &args[2]).expect("clip");
    let clip = r.clip.as_ref().expect("clip body");
    let tick: f32 = args[3].parse().unwrap();
    let object = &data.robot;
    let bind: Vec<_> = object.nodes.iter().map(|n| tf2_core::pose::affine(&n.bind)).collect();
    let mut at = object.nodes.iter().position(|n| n.name == args[4]);
    println!("stream kinds: {:?}", clip.stream_kinds);
    let pose = tf2_core::pose::Rig::new(object).pose(clip, r.looping, tick);
    while let Some(i) = at {
        let n = &object.nodes[i];
        let w = pose[i].translation;
        println!("    posed at ({:6.3} {:6.3} {:6.3}), x axis {:?}", w.x, w.y, w.z, pose[i].matrix3.x_axis);
        let local = n.parent.map_or(bind[i], |p| bind[p].inverse() * bind[i]);
        let (s, q, t) = local.to_scale_rotation_translation();
        println!("{i:3} {:<26} rest t ({:6.2} {:6.2} {:6.2}) q ({:6.3} {:6.3} {:6.3} {:6.3}) s ({:5.2} {:5.2} {:5.2})", n.name, t.x, t.y, t.z, q.x, q.y, q.z, q.w, s.x, s.y, s.z);
        if let Some(c) = clip.channel(n.name_hash) {
            println!(
                "    clip t {:?} q {:?} s {:?} own {:?}",
                c.position.sample(tick, clip.length, r.looping),
                c.rotation.sample(tick, clip.length, r.looping),
                c.scale.sample(tick, clip.length, r.looping),
                c.own_scale.sample(tick, clip.length, r.looping)
            );
        }
        at = n.parent;
    }
}
