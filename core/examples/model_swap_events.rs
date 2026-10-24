//! The model events of a character's change-of-form clips: at which tick of each robot clip the
//! robot and the vehicle model are shown or hidden, and which vehicle clip starts. Read-only. For
//! whether Grindor's rotor housing spike (vehicle `Trans_V2R_B` ticks 30-32) can be seen at all.
//! `cargo run -p tf2-core --example model_swap_events -- <pack> [clip name filter]`

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pack = args.first().expect("pack");
    let filter = args.get(1).map_or("Trans", String::as_str);
    let dir = tf2_core::install::dir_string();
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
    for (set, refs) in &data.animations.sets {
        for r in refs.iter().filter(|r| r.id.contains(filter)) {
            let Some(clip) = &r.clip else { continue };
            for e in &clip.model_events {
                println!("{set}/{} ({:.0} ticks, speed {:.2}) t {:.0}: robot {} vehicle {} starts {:?} then {:?}", r.id, clip.length, r.speed, e.time,
                    e.show_true, e.show_alternate, e.alternate_clip, e.alternate_then);
            }
        }
    }
}
