//! Every motion clip event (`Action::is_motion`) in a pack's animation sets, by set and clip.
//! `cargo run -p tf2-core --example clip_motion_events -- <pack>`
use std::path::Path;

fn main() {
    let pack = std::env::args().nth(1).unwrap_or_else(|| "bumblebee".into());
    let data = tf2_core::character::load(Path::new("C:/Games2"), &pack).expect("load");
    let mut sets: Vec<_> = data.animations.sets.iter().collect();
    sets.sort_by_key(|(name, _)| name.as_str());
    for (set, refs) in sets {
        for r in refs {
            let Some(clip) = &r.clip else { continue };
            for e in clip.action_events.iter().filter(|e| e.action.is_motion()) {
                println!("{set:20} {:28} {:6.1} {:?}", r.id, e.time, e.action);
            }
        }
    }
}
