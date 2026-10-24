//! One car clip's keys along the chain from the root to a node: each link's local place, turn
//! and scale at the ticks asked, as the port samples them (`Channel::local`). For Grindor's rear
//! rotor housing flying out for a frame of `Trans_V2R_B` (the user's report, 2026-10-25).
//! `cargo run -p tf2-core --example car_chain -- <pack> <clip> <node> <tick>...`

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = tf2_core::install::dir_string();
    let data = tf2_core::character::load(Path::new(&dir), &args[0]).expect("character");
    let car = data.vehicle.as_ref().expect("car");
    let r = data.vehicle_animations.iter().find(|r| r.id == args[1]).expect("clip");
    let clip = r.clip.as_ref().expect("clip file");
    let ticks: Vec<f32> = args[3..].iter().filter_map(|a| a.parse().ok()).collect();
    let mut chain = vec![car.nodes.iter().position(|n| n.name == args[2]).expect("node")];
    while let Some(p) = car.nodes[*chain.last().unwrap()].parent {
        chain.push(p);
    }
    for &i in chain.iter().rev() {
        let n = &car.nodes[i];
        let channel = clip.channel(n.name_hash);
        println!("{:<26} rest at {:?}", n.name, n.bind[3]);
        for &t in &ticks {
            match channel.and_then(|c| c.local(t, clip.length, false)) {
                Some((p, q, s)) => println!("   {t:5.1}: p ({:+8.3} {:+8.3} {:+8.3}) q ({:+.2} {:+.2} {:+.2} {:+.2}) s ({:.3} {:.3} {:.3})",
                    p[0], p[1], p[2], q[0], q[1], q[2], q[3], s[0], s[1], s[2]),
                None => println!("   {t:5.1}: rest"),
            }
        }
    }
}
