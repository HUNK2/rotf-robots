//! The animation sets' clip-selection rules as `<pack>.json`'s `animation_rules`: each set's entry
//! rules (which clip a set starts on, coming from which clip) and each clip's exit rules (when it
//! hands over, and to which clip), with every comparison by the variable's name. Read as the port
//! reads them (`tf2_core::formats::anim`, run by `tf2_core::animrules`).

use serde_json::{Value, json};
use tf2_core::character::CharacterData;
use tf2_core::formats::anim::{AnimRef, Rule};
use tf2_core::formats::hash::crc32;

use crate::describe::num;
use crate::names::Names;

/// Every variable a rule may read, by name: `F_` floats, `I_` integers, `C_` hashes
/// (`docs/animation-rules.md`, "Variables"). [game 0070c380, 0070c900, 0070cae0]
pub const VARIABLES: [&str; 27] = [
    "F_L_STICK_AMP", "F_R_STICK_AMP", "F_L_STICK_DIR", "F_R_STICK_DIR", "F_STRAFE_ANG", "F_Z_VEL",
    "F_FALL_DUR", "F_TIME_IN_ANIM", "F_TIME_SINCE_DAMAGED", "F_HP_PERCENT", "F_STUN_TIME", "F_RANDOM",
    "I_IS_HOVERING", "I_WEAPON_INDEX", "I_SPECIAL_ON", "I_SPECIAL_IDLE_ON", "I_IS_STRAFING",
    "I_ATTK_BRANCH", "I_ATTK_BTN_HELD", "I_PREV_CHAR_FORM", "I_CHAR_FORM", "I_IN_AIR", "I_ON_GROUND",
    "C_JUMP_TYPE", "C_VEHICLE_STUNT", "C_INTERACT_TYPE", "C_CHAR_MESSAGE",
];

/// The hash variables not in `VARIABLES` that the code sets (`docs/animation-rules.md`).
pub const MORE_VARIABLES: [&str; 4] = ["C_DAMAGE_TYPE", "C_WEAPON_ACTION", "C_WEAPON_EQUIP", "C_INTERACT_ACTION_TYPE"];

pub fn animation_rules(data: &CharacterData, names: &Names) -> Value {
    let anims = &data.animations;
    let mut sets: Vec<&String> = anims.sets.keys().chain(anims.incoming.keys()).collect();
    sets.sort();
    sets.dedup();
    // A set and clip by their hashes, the clip by its name in `clips` when the robot has it.
    let set_name = |hash: u32| names.name(hash);
    let clip_name = |set: &str, id: u32| -> Option<String> {
        let r = anims.sets.get(set)?.iter().find(|r| r.clip.is_some() && crc32(r.id.as_bytes()) == id)?;
        Some(format!("{set}.{}", r.id))
    };
    let target = |own: &str, to_set: u32, to_id: u32| {
        let set = if to_set == 0 { own.to_owned() } else { set_name(to_set) };
        json!({
            "to_set": set,
            "to_clip": (to_id != 0).then(|| clip_name(&set, to_id)).flatten(),
            "to_clip_id": (to_id != 0).then(|| names.name(to_id)),
        })
    };
    let entries = |set: &str| -> Vec<Value> {
        anims.incoming.get(set).into_iter().flatten().map(|inc| {
            let mut entry = target(set, inc.to_set, inc.to_id);
            let any = |hash: u32| (hash != 0).then(|| names.name(hash));
            entry["from_set"] = json!(any(inc.from_set));
            entry["from_clip_id"] = json!(any(inc.from_id));
            entry["base_set"] = json!(any(inc.base_set));
            entry["base_clip_id"] = json!(any(inc.base_id));
            entry["rules"] = rules(&inc.rules, names);
            entry
        }).collect()
    };
    let clip = |set: &str, r: &AnimRef| -> Value {
        let exits: Vec<Value> = r.exits.iter().map(|exit| {
            let mut entry = target(set, exit.to_set, exit.to_id);
            entry["finish"] = json!(exit.finish);
            entry["branch_point"] = json!(exit.branch_point);
            entry["rules"] = rules(&exit.rules, names);
            entry
        }).collect();
        json!({
            "clip": format!("{set}.{}", r.id),
            "keeps_time": r.keeps_time,
            "continue_time": r.continue_time,
            "exit_events": r.exit_events,
            "exit_crossfade": (r.exit_crossfade >= 0.0).then(|| num(r.exit_crossfade)),
            "exit_crossfade_to": (r.exit_crossfade >= 0.0 && r.exit_crossfade_to != 0).then(|| names.name(r.exit_crossfade_to)),
            "exits": exits,
        })
    };
    json!({
        "default_crossfade": num(tf2_core::animrules::DEFAULT_CROSSFADE),
        "sets": sets.iter().map(|set| json!({
            "set": set,
            "entries": entries(set),
            "clips": anims.sets.get(set.as_str()).into_iter().flatten().filter(|r| r.clip.is_some()).map(|r| clip(set, r)).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// A rule list: each rule's variable, comparison, and the value of the variable's own type.
fn rules(list: &[Rule], names: &Names) -> Value {
    Value::Array(
        list.iter()
            .map(|rule| {
                let variable = names.name(rule.variable);
                let compare = match rule.logic {
                    1 => "!=",
                    2 => ">",
                    3 => ">=",
                    4 => "<",
                    5 => "<=",
                    _ => "==",
                };
                let value = match variable.get(..2) {
                    Some("F_") => num(rule.float),
                    Some("I_") => json!(rule.int),
                    Some("C_") => json!((rule.hash != 0).then(|| names.name(rule.hash))),
                    _ => json!({ "float": num(rule.float), "int": rule.int, "hash": names.name(rule.hash) }),
                };
                json!({ "variable": variable, "compare": compare, "value": value })
            })
            .collect(),
    )
}
