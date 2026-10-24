//! One clip's local keys for the named nodes, as the port decodes them: position, rotation (as
//! stored, x y z w) and scale at a few ticks, and which stream types and key classes the clip
//! holds. For the gun nodes Long Haul's jump turns across his chest (the user's report,
//! 2026-10-25). `cargo run -p tf2-core --example channel_peek -- <pack> <Set> <Id> <node>...`

use std::path::Path;

fn main() {
    let dir = tf2_core::install::dir_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data = tf2_core::character::load(Path::new(&dir), &args[0]).expect("character");
    let r = data.animations.find(&args[1], &args[2]).expect("clip");
    let clip = r.clip.as_ref().expect("clip file");
    println!("{} ticks; streams {:?}", clip.length, clip.stream_kinds);
    for name in &args[3..] {
        let hash = tf2_core::formats::hash::crc32(name.as_bytes());
        let index = clip.names.iter().position(|&h| h == hash);
        println!("-- {name}: channel {index:?}");
        let Some(channel) = clip.channel(hash) else { continue };
        for tick in [0.0, clip.length * 0.25, clip.length * 0.5, clip.length] {
            let f = |v: Option<[f32; 3]>| v.map_or("-".to_owned(), |v| format!("({:+.3} {:+.3} {:+.3})", v[0], v[1], v[2]));
            let q = channel.rotation.sample(tick, clip.length, false).map_or("-".to_owned(), |q| format!("({:+.3} {:+.3} {:+.3} {:+.3})", q[0], q[1], q[2], q[3]));
            println!("  {tick:5.1}: p {} q {q} s {}", f(channel.position.sample(tick, clip.length, false)), f(channel.scale.sample(tick, clip.length, false)));
        }
    }
}
