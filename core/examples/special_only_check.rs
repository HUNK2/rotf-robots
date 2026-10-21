//! For each robot of the roster: how many of its weapons are specials (`isSpecial`) and how many
//! are not. A robot whose every weapon is special would reach the port's start-pick fallback
//! (`game\src\weapons.rs` `in_hand`), which the game's cycle (007b3ea0) does not have.
//! `cargo run -p tf2-core --example special_only_check -- [game dir]`

use std::path::PathBuf;

use tf2_core::{character, weapons};

const ROSTER: [&str; 13] = [
    "bumblebee", "optimus", "ironhide", "ratchet", "megatron", "longhaul", "audir8", "jetfire", "firestorm",
    "starscream", "blackout", "optimusflight", "megatronflight",
];

fn main() {
    let game_dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(tf2_core::install::dir_string));
    for pack in ROSTER {
        let data = character::load(&game_dir, pack).expect("the character pack");
        let tunings = weapons::load(&game_dir, &data.weapons).expect("the weapon attributes");
        let special = tunings.iter().filter(|t| t.is_special).count();
        println!("{pack:<15} weapons {:>2}  special {special:>2}  ordinary {:>2}", tunings.len(), tunings.len() - special);
    }
}
