//! Plays `runs/basics.json` through the reference for Bumblebee: the run comes out the same twice,
//! has a line per update, goes through the states the script asks for, and `docs/FORMAT.md` names
//! every key it writes. A script with a button the game does not have is refused. Reads the
//! install; skips without it.

use std::collections::BTreeSet;

use serde_json::{Value, json};

fn bumblebee() -> Option<tf2_core::formats::characterlist::Spec> {
    if tf2_core::install::missing() {
        return None;
    }
    let roster = tf2_core::formats::characterlist::CharacterList::load(tf2_core::install::dir()).unwrap();
    Some(rotf_export::spec(&roster, "bumblebee"))
}

#[test]
fn the_basics_run_goes_through_its_states() {
    let Some(spec) = bumblebee() else { return };
    let install = tf2_core::install::dir();
    let script: Value = serde_json::from_str(include_str!("../runs/basics.json")).unwrap();
    let lines = rotf_export::run::run(install, &spec, &script).unwrap();
    assert_eq!(lines, rotf_export::run::run(install, &spec, &script).unwrap(), "two runs differ");

    let header = &lines[0];
    assert_eq!(header["format_version"], rotf_export::FORMAT_VERSION);
    assert_eq!(header["update_seconds"], json!(0.032));
    let updates: u64 = script["steps"].as_array().unwrap().iter().map(|s| s["updates"].as_u64().unwrap_or(1)).sum();
    assert_eq!(header["updates"], updates);
    assert_eq!(lines.len() as u64, updates + 1);
    for (i, line) in lines[1..].iter().enumerate() {
        assert_eq!(line["update"], i, "update numbers");
    }

    let states: Vec<&str> = lines[1..].iter().map(|l| l["state"].as_str().unwrap_or("-")).collect();
    for state in ["stateIdle", "stateMove", "stateJump", "stateAttack1", "stateAttack2", "stateAttack3", "stateDriveEnterA", "stateDrive", "stateDriveExitRunA"] {
        assert!(states.contains(&state), "never in {state}");
    }
    assert_eq!(states.last(), Some(&"stateMove"), "the run does not end running");
    assert!(lines[1..].iter().any(|l| l["vehicle"] == true), "never in the car");
    assert!(lines.last().unwrap()["pos"][1].as_f64().unwrap() > 200.0, "he did not go far north");

    let spec_text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/FORMAT.md")).unwrap();
    let mut keys = BTreeSet::new();
    for line in &lines {
        collect(line, &mut keys);
    }
    let missing: Vec<_> = keys.into_iter().filter(|k| !spec_text.contains(&format!("`{k}`"))).collect();
    assert!(missing.is_empty(), "docs/FORMAT.md does not name {missing:?}");
}

#[test]
fn a_button_the_game_lacks_is_refused() {
    let Some(spec) = bumblebee() else { return };
    let script = json!({"format_version": 1, "steps": [{"press": ["Punch"]}]});
    let error = rotf_export::run::run(tf2_core::install::dir(), &spec, &script).unwrap_err();
    assert!(error.contains("no button"), "{error}");
}

fn collect(v: &Value, keys: &mut BTreeSet<String>) {
    match v {
        Value::Object(map) => {
            for (k, x) in map {
                keys.insert(k.clone());
                collect(x, keys);
            }
        }
        Value::Array(items) => items.iter().for_each(|x| collect(x, keys)),
        _ => {}
    }
}
