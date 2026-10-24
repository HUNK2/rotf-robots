//! One node of a vehicle clip and its parents at a tick: each link's keyed local place, turn
//! and scale, and the model-space place the chain gives. For the 22 m spike in Blackout's
//! `Trans_R2V_A` at ticks 14-16 (2026-10-24).
//! `cargo run -p tf2-core --example node_chain -- <pack> <vehicle clip> <node> <tick>...`
use tf2_core::pose::Rig;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data = tf2_core::character::load(tf2_core::install::dir(), &args[0]).expect("load");
    let vehicle = data.vehicle.as_ref().expect("vehicle");
    let r = data.vehicle_animations.iter().find(|a| a.id == args[1]).expect("clip");
    let clip = r.clip.as_ref().unwrap();
    let rig = Rig::new(vehicle);
    let node = vehicle.nodes.iter().position(|n| n.name == args[2]).expect("node");
    for tick in args[3..].iter().map(|t| t.parse::<f32>().unwrap()) {
        let pose = rig.pose(clip, r.looping, tick);
        println!("tick {tick}:");
        let mut at = Some(node);
        while let Some(i) = at {
            let n = &vehicle.nodes[i];
            let ch = clip.channel(n.name_hash);
            let p = ch.and_then(|c| c.position.sample(tick, clip.length, r.looping));
            let s = ch.and_then(|c| c.scale.sample(tick, clip.length, r.looping));
            let own = ch.and_then(|c| c.own_scale.sample(tick, clip.length, r.looping));
            let m = pose[i].translation;
            println!("  {:<26} keyed pos {:?} scale {:?} own {:?} -> model ({:6.2} {:6.2} {:6.2})", n.name, p, s, own, m.x, m.y, m.z);
            at = n.parent;
        }
    }
}
