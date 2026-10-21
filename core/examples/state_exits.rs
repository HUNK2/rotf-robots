//! Which clips a control state starts BY NAME have exit rules of their own (the animation
//! graph's `animExitRules`), which the action player never walked, and which sets a state starts
//! without a clip (entry rules). One line per state; `--all` also lists clips without exits.
//! `cargo run -p tf2-core --example state_exits -- [--all] [pack:AttributesName...]`

use std::collections::HashMap;
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
    let all = args.iter().any(|a| a == "--all");
    args.retain(|a| a != "--all");
    if args.is_empty() {
        args = ROSTER.map(str::to_owned).to_vec();
    }
    for arg in args {
        let (pack, name) = arg.split_once(':').expect("pack:Name");
        let t = tf2_core::tuning::load(Path::new(&dir), name).expect("tuning");
        let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
        let sets = &data.animations.sets;
        let mut names: HashMap<u32, String> = HashMap::new();
        for (set, refs) in sets {
            names.insert(crc32(set.as_bytes()), set.clone());
            for r in refs {
                names.insert(crc32(r.id.as_bytes()), r.id.clone());
            }
        }
        let n = |h: u32| if h == 0 { "-".to_owned() } else { names.get(&h).cloned().unwrap_or(format!("{h:08x}")) };
        println!("== {pack}");
        for state in t.control.states.iter() {
            // The fly and drive states start their own set whatever the data names (0087c320,
            // 0087c7e0, 0087ba10). [game]
            let set = state.anim_set.clone().unwrap_or_else(|| match state.class.as_str() {
                "StateFly" | "StateFlyExit" => "FlySet".to_owned(),
                "StateDrive" => "DriveSet".to_owned(),
                _ => String::new(),
            });
            if state.anim != 0 {
                let r = sets.get(&set).and_then(|refs| refs.iter().find(|r| crc32(r.id.as_bytes()) == state.anim));
                let r = r.or_else(|| sets.values().flat_map(|v| v.iter()).find(|r| crc32(r.id.as_bytes()) == state.anim));
                match r {
                    Some(r) if !r.exits.is_empty() => {
                        let exits: Vec<String> = r
                            .exits
                            .iter()
                            .map(|e| format!("{}{}/{}{}", if e.finish { "fin:" } else { "" }, n(e.to_set), n(e.to_id), if e.rules.is_empty() { String::new() } else { format!("[{}r]", e.rules.len()) }))
                            .collect();
                        println!("  {:34} {:22} {}/{}  EXITS {}", state.name, state.class, set, r.id, exits.join(" "));
                    }
                    Some(r) if all => println!("  {:34} {:22} {}/{}  no exits", state.name, state.class, set, r.id),
                    None => println!("  {:34} {:22} {}/{:08x}  CLIP NOT FOUND", state.name, state.class, set, state.anim),
                    _ => {}
                }
            } else if !set.is_empty() {
                let inc = data.animations.incoming.get(&set).map_or(0, Vec::len);
                println!("  {:34} {:22} set {} (entry rules {})", state.name, state.class, set, inc);
            }
        }
    }
}
