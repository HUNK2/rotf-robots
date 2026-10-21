//! The animation graph of every robot, set by set: how many clips, entry rules and exit rules
//! each set has, which rule variables they read, and which of those `sim::rule_value` does not
//! answer yet. `cargo run -p tf2-core --example animgraph_audit -- [pack ...] [--sets]`
//! (`--sets` prints one line per set; without it, one summary per character and the union).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use tf2_core::formats::hash::crc32;

const ROSTER: [&str; 13] = [
    "bumblebee", "optimus", "ironhide", "ratchet", "megatron", "longhaul", "audir8", "jetfire", "firestorm", "starscream",
    "blackout", "optimusflight", "megatronflight",
];

const VARIABLES: [&str; 31] = [
    "F_STUN_TIME", "F_RANDOM", "I_IN_AIR", "I_SPECIAL_ON", "F_R_STICK_AMP", "F_TIME_SINCE_DAMAGED", "C_JUMP_TYPE",
    "F_Z_VEL", "I_IS_STRAFING", "C_VEHICLE_STUNT", "I_WEAPON_INDEX", "C_INTERACT_TYPE", "C_CHAR_MESSAGE",
    "C_DAMAGE_TYPE", "F_L_STICK_DIR", "I_ATTK_BTN_HELD", "I_SPECIAL_IDLE_ON", "I_ON_GROUND", "F_TIME_IN_ANIM",
    "F_FALL_DUR", "F_R_STICK_DIR", "I_ATTK_BRANCH", "I_IS_HOVERING", "I_PREV_CHAR_FORM", "I_CHAR_FORM",
    "C_INTERACT_ACTION_TYPE", "C_WEAPON_ACTION", "F_L_STICK_AMP", "F_STRAFE_ANG", "C_WEAPON_EQUIP", "F_HP_PERCENT",
];

/// What `sim::rule_value` answers today (kept by hand: update it with that function), and
/// the two the weapon layer answers for itself (`gun::Gun::value`: `C_WEAPON_ACTION`,
/// `I_WEAPON_INDEX`).
const ANSWERED: [&str; 23] = [
    "F_L_STICK_AMP", "F_L_STICK_DIR", "F_R_STICK_AMP", "F_R_STICK_DIR", "F_STRAFE_ANG", "I_IS_STRAFING", "F_FALL_DUR",
    "F_Z_VEL", "I_IN_AIR", "I_ON_GROUND", "C_JUMP_TYPE", "C_INTERACT_TYPE", "I_SPECIAL_IDLE_ON", "F_TIME_SINCE_DAMAGED",
    "F_TIME_IN_ANIM", "I_CHAR_FORM", "C_DAMAGE_TYPE", "F_STUN_TIME", "I_SPECIAL_ON", "I_IS_HOVERING", "C_VEHICLE_STUNT",
    "C_WEAPON_ACTION", "I_WEAPON_INDEX",
];

fn main() {
    let dir = tf2_core::install::dir_string();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let per_set = args.iter().any(|a| a == "--sets");
    args.retain(|a| a != "--sets");
    if args.is_empty() {
        args = ROSTER.map(str::to_owned).to_vec();
    }
    let names: HashMap<u32, &str> = VARIABLES.iter().map(|n| (crc32(n.as_bytes()), *n)).collect();
    let answered: BTreeSet<&str> = ANSWERED.into_iter().collect();
    let mut union: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut unanswered_union: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for pack in &args {
        let data = match tf2_core::character::load(Path::new(&dir), pack) {
            Ok(d) => d,
            Err(e) => {
                println!("{pack}: cannot load: {e}");
                continue;
            }
        };
        let a = &data.animations;
        let mut sets: Vec<&String> = a.sets.keys().collect();
        sets.sort();
        let (mut clips, mut exits, mut incoming) = (0, 0, 0);
        for set in &sets {
            let refs = &a.sets[*set];
            let inc = a.incoming.get(*set).map_or(0, Vec::len);
            let ex: usize = refs.iter().map(|r| r.exits.len()).sum();
            let mut vars = BTreeSet::new();
            for r in refs {
                for e in &r.exits {
                    vars.extend(e.rules.iter().map(|r| r.variable));
                }
            }
            for i in a.incoming.get(*set).into_iter().flatten() {
                vars.extend(i.rules.iter().map(|r| r.variable));
            }
            let vars: BTreeSet<String> =
                vars.into_iter().map(|v| names.get(&v).map_or_else(|| format!("#{v:08x}"), |n| n.to_string())).collect();
            let missing: Vec<&String> = vars.iter().filter(|v| !answered.contains(v.as_str())).collect();
            for m in &missing {
                unanswered_union.entry((*m).clone()).or_default().insert(format!("{pack}:{set}"));
            }
            union.entry((*set).clone()).or_default().insert(pack.clone());
            clips += refs.len();
            exits += ex;
            incoming += inc;
            if per_set {
                println!(
                    "{pack:15} {set:28} clips {:3} entry {:3} exit {:3}  vars [{}]  unanswered [{}]",
                    refs.len(),
                    inc,
                    ex,
                    vars.iter().cloned().collect::<Vec<_>>().join(" "),
                    missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")
                );
            }
        }
        println!("{pack:15} sets {:3} clips {clips:4} entry rules {incoming:4} exit rules {exits:4}", sets.len());
    }
    println!("\nsets across the roster (set: characters):");
    for (set, packs) in &union {
        println!("  {set:28} {:2}  {}", packs.len(), packs.iter().cloned().collect::<Vec<_>>().join(" "));
    }
    println!("\nvariables the rules read that rule_value does not answer:");
    for (var, wher) in &unanswered_union {
        let sets: BTreeSet<&str> = wher.iter().filter_map(|w| w.split_once(':').map(|(_, s)| s)).collect();
        println!("  {var:24} {} places; sets {}", wher.len(), sets.into_iter().collect::<Vec<_>>().join(" "));
    }
}
