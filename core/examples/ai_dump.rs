//! The AI brains of one or more characters as the install's `bnxglobal.str` data chunk 16 gives
//! them, and the totals over every brain. `cargo run -p tf2-core --example ai_dump -- [Name...]`
//! (`Name` is the character without `characterAttributes`; default `DComScout APrtScout`).
//! Reads `C:\Games2` (or `TF2_GAME_DIR`). Writes nothing.

use std::path::Path;

use tf2_core::ai_data::{self, Behaviour, BrainRoot};
use tf2_core::formats::lxb::DataFile;
use tf2_core::formats::pack::{self, Pack};

fn main() {
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    let pack = Pack::open(&Path::new(&dir).join("bnxglobal.str")).expect("open bnxglobal.str");
    let chunk = pack
        .chunks
        .iter()
        .find(|c| c.index == ai_data::AI_CHUNK && c.tag == pack::DATA)
        .expect("bnxglobal.str has no chunk 16");
    let file = DataFile::parse(pack.data(chunk).to_vec()).expect("parse chunk 16");

    let all = ai_data::read_brains(&file);
    let mut missing = all.missing.clone();
    let blocks = ai_data::read_attributes(&file, &mut missing);
    println!("AI attribute blocks (AI_characterAttributes*): {}", blocks.len());

    let mut names: Vec<String> = std::env::args().skip(1).collect();
    if names.is_empty() {
        names = vec!["DComScout".into(), "APrtScout".into()];
    }
    for character in &names {
        let attr = format!("characterAttributes{character}");
        let Some(node) = file.root().get(&attr) else {
            println!("== {attr}: not a root of chunk 16");
            continue;
        };
        let mut own = Vec::new();
        let refs = ai_data::brains_of(node, &mut own);
        println!("== {attr}: {} brainList entries", refs.len());
        for r in &refs {
            let name = file.name(r.brain).map_or_else(|| format!("#{:08x}", r.brain), str::to_owned);
            let Some(root) = all.brain_by_hash(r.brain) else {
                println!("  {name} (priority {:.2}): NOT FOUND among the brain roots", r.priority);
                continue;
            };
            let t = root.totals();
            println!(
                "  {name} priority {:.2} (ramp {:#010x}/{:#010x}): {} goals, {} perceptions, {} behaviours",
                r.priority, r.ramp_up_time, r.ramp_down_time, t.goals, t.perceptions, t.behaviours
            );
            print_goals(root);
            println!("    persistent: {}", describe_persistent(root));
        }
        missing.extend(own);
    }

    let mut goals = 0;
    let mut perceptions = 0;
    let mut behaviours = 0;
    for brain in &all.brains {
        let t = brain.totals();
        goals += t.goals;
        perceptions += t.perceptions;
        behaviours += t.behaviours;
    }
    println!("== totals over all {} brain roots ({} other roots in chunk 16)", all.brains.len(), all.other_roots);
    println!("  unique objects by type: {}", all.objects.values().sum::<usize>());
    for (kind, n) in &all.objects {
        println!("    {kind}: {n}");
    }
    println!("  types: {}", all.objects.len());
    println!("  goal and behaviour uses across brains (each use counted): goals {goals}, perceptions {perceptions}, behaviours {behaviours}");
    println!("== missing: {} (reader) + {} (attribute lists)", all.missing.len(), missing.len());
    for line in all.missing.iter().chain(missing.iter()).take(40) {
        println!("  {line}");
    }
}

fn print_goals(root: &BrainRoot) {
    for (i, g) in root.goals.iter().enumerate() {
        let perceptions: Vec<&str> = g.goal.perceptions.iter().map(|p| p.perception.type_name()).collect();
        let behaviours: Vec<&str> = g.goal.behaviours.iter().map(|b| b.behaviour.type_name()).collect();
        println!(
            "    goal[{i}] {} priority {:.2}: perceptions [{}], behaviours [{}]",
            g.goal.name,
            g.priority,
            perceptions.join(", "),
            behaviours.join(", ")
        );
    }
}

fn describe_persistent(root: &BrainRoot) -> String {
    let parts: Vec<String> = root
        .persistent
        .iter()
        .map(|b| {
            let kind = match &b.behaviour {
                Behaviour::WeaponSwitch(w) => format!("{:?}", w.weapon_index),
                other => other.type_name().to_owned(),
            };
            format!("{:.2} {kind}", b.priority)
        })
        .collect();
    parts.join("; ")
}
