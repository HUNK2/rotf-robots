//! Each character's control states and whether the clip each one plays is in his own animation
//! set: a state naming a clip (`animName`) must find it in its set (or any set); a state naming
//! only a set must find that set (its entry rules pick the clip). Prints the states that fail.
//! `cargo run -p tf2-core --example state_clips -- [pack:AttributesName...]`

use std::path::Path;

use tf2_core::formats::hash::crc32;

const ROSTER: [&str; 13] = [
    "bumblebee:Bumblebee", "optimus:Optimus", "ironhide:Ironhide", "ratchet:Ratchet", "megatron:Megatron",
    "longhaul:LongHaul", "audir8:Audi", "jetfire:Jetfire", "firestorm:Firestorm", "starscream:Starscream",
    "blackout:Blackout", "optimusflight:OptimusFlight", "megatronflight:MegatronFlight",
];

fn main() {
    let dir = tf2_core::install::dir_string();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        args = ROSTER.map(str::to_owned).to_vec();
    }
    for arg in args {
        let (pack, name) = arg.split_once(':').expect("pack:Name");
        let t = tf2_core::tuning::load(Path::new(&dir), name).expect("tuning");
        let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
        let sets = &data.animations.sets;
        let has_clip = |set: Option<&str>, id: u32| {
            let in_set = |refs: &Vec<tf2_core::formats::anim::AnimRef>| refs.iter().any(|r| crc32(r.id.as_bytes()) == id && r.clip.is_some());
            set.and_then(|s| sets.get(s)).is_some_and(in_set) || sets.values().any(in_set)
        };
        let (mut total, mut failed) = (0, Vec::new());
        for state in t.control.states.iter() {
            total += 1;
            let ok = if state.anim != 0 {
                has_clip(state.anim_set.as_deref(), state.anim)
            } else if let Some(set) = &state.anim_set {
                sets.contains_key(set)
            } else {
                true
            };
            if !ok {
                failed.push(format!("{} ({:?} {:08x})", state.name, state.anim_set, state.anim));
            }
        }
        println!("{pack:15} {total} states, {} without their clip{}", failed.len(), if failed.is_empty() { String::new() } else { format!(": {}", failed.join(", ")) });
    }
}
