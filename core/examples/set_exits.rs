//! Every clip with exit rules in the sets the action player plays (`Actions::SETS`), per
//! character: set/clip and where each exit goes. `cargo run -p tf2-core --example set_exits -- [pack...]`

use std::path::Path;

use tf2_core::formats::hash::crc32;
use tf2_core::tuning::Actions;

fn main() {
    let dir = tf2_core::install::dir_string();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        args = ["bumblebee", "optimus", "ironhide", "ratchet", "megatron", "longhaul", "audir8", "jetfire", "firestorm", "starscream", "blackout", "optimusflight", "megatronflight"].map(str::to_owned).to_vec();
    }
    for pack in args {
        let data = tf2_core::character::load(Path::new(&dir), &pack).expect("character");
        let mut names = std::collections::HashMap::new();
        for (set, refs) in &data.animations.sets {
            names.insert(crc32(set.as_bytes()), set.clone());
            for r in refs {
                names.insert(crc32(r.id.as_bytes()), r.id.clone());
            }
        }
        let n = |h: u32| if h == 0 { "-".to_owned() } else { names.get(&h).cloned().unwrap_or(format!("{h:08x}")) };
        println!("== {pack}");
        for set in Actions::SETS {
            let Some(refs) = data.animations.sets.get(set) else { continue };
            for r in refs.iter().filter(|r| !r.exits.is_empty()) {
                let exits: Vec<String> = r
                    .exits
                    .iter()
                    .map(|e| format!("{}{}{}/{}{}", if e.finish { "fin:" } else { "" }, if e.branch_point > 0 { format!("bp{}:", e.branch_point) } else { String::new() }, n(e.to_set), n(e.to_id), if e.rules.is_empty() { String::new() } else { format!("[{}r]", e.rules.len()) }))
                    .collect();
                println!("  {set}/{}  {}", r.id, exits.join(" "));
            }
        }
    }
}
