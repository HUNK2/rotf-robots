//! For each of the 13 playable packs: the effects on the robot and vehicle objects that answer to a
//! message (`EffectSpawner` / `TriggerableParticle`, by `LuxScriptName`), the plain `ParticleEmitter`
//! attachments (auto-start or not), and the sound players waiting for a trigger -- each message name
//! resolved through `export\data\names.tsv`. Shows which names a form's effects wait for, to set
//! against what the port sends (`fxstate::kept`, the clips' triggers).
//! `cargo run -p tf2-core --example form_fx_names [pack...]` (reads the install).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use tf2_core::formats::scene::ObjectDef;

const PACKS: [&str; 13] = [
    "bumblebee", "optimus", "ratchet", "ironhide", "megatron", "longhaul", "audir8", "jetfire", "firestorm",
    "starscream", "blackout", "optimusflight", "megatronflight",
];

fn object(label: &str, o: &ObjectDef, names: &HashMap<u32, String>) {
    let name = |h: u32| names.get(&h).cloned().unwrap_or_else(|| format!("#{h:08x}"));
    let node = |i: usize| o.nodes.get(i).map_or("?".to_owned(), |n| n.name.clone());
    let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in &o.effects {
        by_name.entry(name(e.script_name)).or_default().push(format!("{}{}", node(e.node), if e.template.elements.is_empty() { "(empty)" } else { "" }));
    }
    println!("  {label}: {} triggered effects, {} placed emitters, {} sound players",
        o.effects.len(), o.emitters.len(), o.sound_players.len());
    for (n, nodes) in &by_name {
        println!("    fx  {n:<28} x{} at {}", nodes.len(), nodes.join(", "));
    }
    for e in &o.emitters {
        println!("    emit {:08x} at {} auto_start {} elements {}", e.template.name, node(e.node), e.auto_start, e.template.elements.len());
    }
    for s in o.sound_players.iter().filter(|s| s.wait_for_trigger) {
        println!("    snd {:<28} at {}", name(s.script), node(s.node));
    }
}

fn main() {
    let dir = tf2_core::install::dir_string();
    let mut names = HashMap::new();
    if let Ok(text) = std::fs::read_to_string(std::env::var("TF2_NAMES").unwrap_or_else(|_| "export/data/names.tsv".into())) {
        for line in text.lines() {
            if let Some((h, n)) = line.split_once('\t')
                && let Ok(h) = u32::from_str_radix(h, 16) {
                names.entry(h).or_insert_with(|| n.to_owned());
            }
        }
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let packs: Vec<&str> = if args.is_empty() { PACKS.to_vec() } else { args.iter().map(String::as_str).collect() };
    for pack in packs {
        let data = match tf2_core::character::load(Path::new(&dir), pack) {
            Ok(d) => d,
            Err(e) => {
                println!("== {pack}: {e}");
                continue;
            }
        };
        println!("== {pack}");
        object("robot", &data.robot, &names);
        match &data.vehicle {
            Some(v) => object("vehicle", v, &names),
            None => println!("  vehicle: none"),
        }
        // Every `AnimSendTriggerEvent` a robot or vehicle clip sends, by set and clip.
        let name = |h: u32| names.get(&h).cloned().unwrap_or_else(|| format!("#{h:08x}"));
        let mut sets: Vec<_> = data.animations.sets.iter().map(|(s, r)| (s.as_str(), r)).collect();
        sets.push(("<vehicle model>", &data.vehicle_animations));
        sets.sort_by_key(|(s, _)| *s);
        for (set, refs) in sets {
            for r in refs {
                let Some(clip) = &r.clip else { continue };
                let sent: Vec<String> = clip.action_events.iter().filter_map(|e| match e.action {
                    tf2_core::formats::anim::Action::Trigger { name: n } => Some(format!("{}@{:.0}", name(n), e.time)),
                    _ => None,
                }).collect();
                if !sent.is_empty() {
                    println!("    clip {set}/{} sends {}", r.id, sent.join(", "));
                }
            }
        }
    }
}
