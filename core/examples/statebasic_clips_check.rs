//! For each robot of the roster: the StateBasic states whose clip (`animName`) is not in the robot's
//! loaded clip table (`Actions::clips`), which the port enters silently (`sim.rs` StateBasic may-enter,
//! [game 0076c530]). The state machine is the global one (`bnxglobal.str`); the clip table is each
//! robot's own animations.
//! `cargo run -p tf2-core --example statebasic_clips_check -- [game dir]`

use std::path::PathBuf;

use tf2_core::{character, tuning, tuning::Actions};

const ROSTER: [&str; 13] = [
    "bumblebee", "optimus", "ironhide", "ratchet", "megatron", "longhaul", "audir8", "jetfire", "firestorm",
    "starscream", "blackout", "optimusflight", "megatronflight",
];

fn main() {
    let game_dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| r"C:\Games2".to_owned()));
    let mut t = tuning::load(&game_dir, "Bumblebee").expect("tuning");
    let basics: Vec<_> = t.control.states.iter().filter(|s| s.class == "StateBasic" && s.anim != 0).cloned().collect();
    println!("{} StateBasic states with a clip (of {} states)", basics.len(), t.control.states.len());
    for pack in ROSTER {
        let data = character::load(&game_dir, pack).expect("the character pack");
        let mut missing = Vec::new();
        t.actions = Actions::from_animations(&data.animations, &data.robot, &mut missing);
        let absent: Vec<String> = basics.iter().filter(|s| !t.actions.clips.contains_key(&s.anim))
            .map(|s| format!("{} ({:08x}{})", s.name, s.anim, s.anim_set.as_deref().map_or(String::new(), |set| format!(" in {set}"))))
            .collect();
        println!("{pack:<15} clips {:>4}  StateBasic clips absent {:>2}: {}", t.actions.clips.len(), absent.len(), absent.join(", "));
    }
}
