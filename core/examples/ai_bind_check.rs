//! Builds every brain root of the install's AI data (`bnxglobal.str` chunk 16) into the running
//! brain form of `ai_bind`, and prints per brain the goals and perceptions bound and stand-in, the
//! Message latch nodes made, the totals by type, the value kinds (valueTypes) with their data uses
//! and whether the port binds them, and the goal table of brainDComScout with what blocks each
//! stand-in goal. Reads `C:\Games2` (or `TF2_GAME_DIR`). Writes nothing.
//! `cargo run -p tf2-core --example ai_bind_check`. The Message perceptions (toggleOn included) are
//! bound since the latch port (ai-messages.md), so no Message appears among the stand-ins.

use std::collections::BTreeMap;
use std::path::Path;

use tf2_core::ai_bind::{self, Report};
use tf2_core::ai_brain::Latches;
use tf2_core::ai_data::{self as data, AiData, BrainRoot};
// `data` above is the module; the loaded AI data is `ai` in main.
use tf2_core::ai_move::{PathSource, StraightLine};
use tf2_core::ai_perceive::VALUE_KINDS;

/// Compare and Range uses per valueType, over every perception list of the data (brain, goal,
/// behaviour and persistent entries; duplicates across brains count).
fn value_uses(data: &AiData) -> BTreeMap<u32, usize> {
    fn walk(list: &[data::PerceptionRef], uses: &mut BTreeMap<u32, usize>) {
        for pr in list {
            match &pr.perception {
                data::Perception::Compare(c) => *uses.entry(c.value_type).or_default() += 1,
                data::Perception::Range(r) => *uses.entry(r.value_type).or_default() += 1,
                _ => {}
            }
        }
    }
    let mut uses = BTreeMap::new();
    for brain in &data.brains {
        walk(&brain.perceptions, &mut uses);
        for gr in &brain.goals {
            walk(&gr.goal.perceptions, &mut uses);
            for br in &gr.goal.behaviours {
                walk(&br.behaviour.base().perceptions, &mut uses);
            }
        }
        for br in &brain.persistent {
            walk(&br.behaviour.base().perceptions, &mut uses);
        }
    }
    uses
}

/// The stand-in reasons of one list of perceptions, as "name (reason)".
fn perception_blockers(list: &[data::PerceptionRef], out: &mut Vec<String>) {
    for pr in list {
        if let Some(why) = ai_bind::perception_stand_in(pr) {
            let kind = match &pr.perception {
                data::Perception::Compare(c) => format!(" #{:08x}", c.value_type),
                data::Perception::Range(r) => format!(" #{:08x}", r.value_type),
                _ => String::new(),
            };
            out.push(format!("{}{kind} ({why})", pr.perception.base().name));
        }
    }
}

fn main() {
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    let ai = data::load(Path::new(&dir)).expect("load the AI data from bnxglobal.str");
    let make = || -> Box<dyn PathSource> { Box::new(StraightLine) };

    let mut report = Report::default();
    let mut latches = Latches::default();
    let mut built = 0;
    for brain in &ai.brains {
        let _root = ai_bind::build_brain(brain, &make, &mut report, &mut latches);
        built += 1;
    }
    println!("built {built} brain roots of {} in the data", ai.brains.len());
    println!("message latch nodes made: {} (one per Message perception use, all brains)", latches.nodes.len());
    for b in &report.brains {
        println!(
            "  {}: goals {} bound / {} stand-in; perceptions {} bound / {} stand-in; persistent {} bound / {} stand-in",
            b.name, b.goals_bound, b.goals_stand_in, b.perceptions_bound, b.perceptions_stand_in, b.persistent_bound, b.persistent_stand_in
        );
    }
    let goals_stand: usize = report.brains.iter().map(|b| b.goals_stand_in).sum();
    let goals_bound: usize = report.brains.iter().map(|b| b.goals_bound).sum();
    println!("summary: goals {goals_bound} bound / {goals_stand} stand-in (each goal of each brain counted)");
    let perc_bound: usize = report.perceptions.values().map(|c| c.bound).sum();
    let perc_stand: usize = report.perceptions.values().map(|c| c.stand_in).sum();
    println!("summary: perception uses {perc_bound} bound / {perc_stand} stand-in");
    let beh_bound: usize = report.behaviours.values().map(|c| c.bound).sum();
    let beh_stand: usize = report.behaviours.values().map(|c| c.stand_in).sum();
    println!("summary: behaviour uses {beh_bound} bound / {beh_stand} stand-in");

    println!("perceptions by type (uses):");
    for (name, c) in &report.perceptions {
        println!("  {name}: {} bound / {} stand-in", c.bound, c.stand_in);
    }
    println!("behaviours by type (uses):");
    for (name, c) in &report.behaviours {
        println!("  {name}: {} bound / {} stand-in", c.bound, c.stand_in);
    }
    println!("perception stand-in reasons (uses):");
    for (why, n) in &report.reasons {
        println!("  {why}: {n}");
    }

    // The valueTypes: each kind of the port's table, its data uses, and whether it is bound.
    let uses = value_uses(&ai);
    println!("value kinds (valueType, meaning, data uses, bound):");
    let mut bound_kinds = 0;
    for k in VALUE_KINDS.iter() {
        let n = uses.get(&k.hash).copied().unwrap_or(0);
        if k.bound {
            bound_kinds += 1;
        }
        println!("  {:08x}  {}  uses {n}  bound {}", k.hash, k.meaning, if k.bound { "yes" } else { "no" });
    }
    println!("value kinds: {bound_kinds} of {} bound", VALUE_KINDS.len());
    for (hash, n) in &uses {
        if !VALUE_KINDS.iter().any(|k| k.hash == *hash) {
            println!("  {hash:08x}  NOT IN VALUE_KINDS  uses {n}");
        }
    }

    // The data's unique objects of each unported type (ai-data.md section 1 counts them the same way).
    println!("behaviour types with no port, unique objects in the data:");
    for name in ["AIBehaviorMoveOnWayPoints", "AIBehaviorMoveOnSpline", "AIBehaviorTactics", "AIBehaviorTeleport", "AIBehaviorInteract"] {
        println!("  {name}: {}", ai.objects.get(name).copied().unwrap_or(0));
    }

    // Which types the persistent lists hold: the channel check (only WeaponSwitch should be there).
    let mut persistent: BTreeMap<&str, usize> = BTreeMap::new();
    for b in &ai.brains {
        for p in &b.persistent {
            *persistent.entry(p.behaviour.type_name()).or_default() += 1;
        }
    }
    println!("persistent entries by type (all brains): {persistent:?}");

    let scout: &BrainRoot = ai.brain("brainDComScout").expect("brainDComScout is in the data");
    println!("== brainDComScout goal table (goal priority, goal, behaviour entries as priority kind, stand-in):");
    for row in ai_bind::goal_table(scout, &make) {
        let behaviours: Vec<String> = row
            .behaviours
            .iter()
            .map(|b| {
                format!(
                    "{:.2} {} {}",
                    b.priority,
                    b.kind.trim_start_matches("AIBehaviour"),
                    if b.stand_in { "stand-in" } else { "bound" }
                )
            })
            .collect();
        println!(
            "  {:.2}  {}  [{}]  {}",
            row.priority,
            row.name,
            behaviours.join(", "),
            if row.stand_in { "stand-in" } else { "bound" }
        );
    }

    println!("== brainDComScout: what blocks each goal (perceptions, then behaviours):");
    for gr in &scout.goals {
        let mut blockers = Vec::new();
        perception_blockers(&gr.goal.perceptions, &mut blockers);
        for br in &gr.goal.behaviours {
            perception_blockers(&br.behaviour.base().perceptions, &mut blockers);
            if let Some(why) = ai_bind::behaviour_stand_in(&br.behaviour) {
                blockers.push(format!("{} ({why})", br.behaviour.type_name()));
            }
        }
        if blockers.is_empty() {
            println!("  {}: bound, nothing blocks it", gr.goal.name);
        } else {
            println!("  {}: blocked by {}", gr.goal.name, blockers.join("; "));
        }
    }
    let mut persistent_blockers = Vec::new();
    for br in &scout.persistent {
        perception_blockers(&br.behaviour.base().perceptions, &mut persistent_blockers);
        if let Some(why) = ai_bind::behaviour_stand_in(&br.behaviour) {
            persistent_blockers.push(format!("{} ({why})", br.behaviour.type_name()));
        }
    }
    println!("  persistent entries: {}", if persistent_blockers.is_empty() { "bound".to_owned() } else { persistent_blockers.join("; ") });
}
