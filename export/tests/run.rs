//! Plays `runs/basics.json`, `weapons.json` and `fight.json` through the reference for Bumblebee:
//! a run comes out the same twice, has a line per update, goes through the states the script asks
//! for, fires, hits and is hit where the script means it to, and `docs/FORMAT.md` names every key
//! it writes. A script with a button the game does not have, or of the old version, is refused.
//! Reads the install; skips without it.

use std::collections::BTreeSet;

use serde_json::{Value, json};

fn bumblebee() -> Option<tf2_core::formats::characterlist::Spec> {
    if tf2_core::install::missing() {
        return None;
    }
    let roster = tf2_core::formats::characterlist::CharacterList::load(tf2_core::install::dir()).unwrap();
    Some(rotf_export::spec(&roster, "bumblebee"))
}

/// Plays a script twice (the runs must be the same), checks its header and update numbers and
/// that FORMAT.md names every key, and gives the lines.
fn play(script: &str) -> Option<Vec<Value>> {
    let spec = bumblebee()?;
    let install = tf2_core::install::dir();
    let script: Value = serde_json::from_str(script).unwrap();
    let lines = rotf_export::run::run(install, &spec, &script).unwrap();
    assert_eq!(lines, rotf_export::run::run(install, &spec, &script).unwrap(), "two runs differ");

    let header = &lines[0];
    assert_eq!(header["format_version"], rotf_export::run::RUN_VERSION);
    assert_eq!(header["update_seconds"], json!(0.032));
    let updates: u64 = script["steps"].as_array().unwrap().iter().map(|s| s["updates"].as_u64().unwrap_or(1)).sum();
    assert_eq!(header["updates"], updates);
    assert_eq!(lines.len() as u64, updates + 1);
    for (i, line) in lines[1..].iter().enumerate() {
        assert_eq!(line["update"], i, "update numbers");
    }

    let spec_text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/FORMAT.md")).unwrap();
    let mut keys = BTreeSet::new();
    for line in &lines {
        collect(line, &mut keys);
    }
    let missing: Vec<_> = keys.into_iter().filter(|k| !spec_text.contains(&format!("`{k}`"))).collect();
    assert!(missing.is_empty(), "docs/FORMAT.md does not name {missing:?}");
    Some(lines)
}

#[test]
fn the_basics_run_goes_through_its_states() {
    let Some(lines) = play(include_str!("../runs/basics.json")) else { return };
    let states: Vec<&str> = lines[1..].iter().map(|l| l["state"].as_str().unwrap_or("-")).collect();
    for state in ["stateIdle", "stateMove", "stateJump", "stateAttack1", "stateAttack2", "stateAttack3", "stateDriveEnterA", "stateDrive", "stateDriveExitRunA"] {
        assert!(states.contains(&state), "never in {state}");
    }
    assert_eq!(states.last(), Some(&"stateMove"), "the run does not end running");
    assert!(lines[1..].iter().any(|l| l["vehicle"] == true), "never in the car");
    assert!(lines.last().unwrap()["pos"][1].as_f64().unwrap() > 200.0, "he did not go far north");
    // The camera follows him: behind him and above as he stands, still behind him at the end.
    let (first, last) = (&lines[1], lines.last().unwrap());
    assert!(first["camera"]["eye"][1].as_f64().unwrap() < -5.0 && first["camera"]["eye"][2].as_f64().unwrap() > 3.0);
    assert!(last["camera"]["eye"][1].as_f64().unwrap() < last["pos"][1].as_f64().unwrap());
}

#[test]
fn the_weapons_run_fires_locks_on_and_drives_with_the_gun() {
    let Some(lines) = play(include_str!("../runs/weapons.json")) else { return };
    let weapon = |l: &Value| l["weapon"].clone();
    let hands: BTreeSet<String> = lines[1..].iter().filter_map(|l| weapon(l)["in_hand"].as_str().map(str::to_owned)).collect();
    assert_eq!(hands, BTreeSet::from(["ForwardLauncher".to_owned(), "SolarPlasmaCannon".to_owned()]));
    let fired = |car: bool| lines[1..].iter().filter(|l| l["vehicle"] == car && weapon(l)["fired"] == true).count();
    assert!(fired(false) > 10 && fired(true) > 3, "on foot {}, in the car {}", fired(false), fired(true));
    assert!(lines[1..].iter().any(|l| !weapon(l)["lock"]["locked"].is_null()), "the launcher never locked on");
    assert!(lines[1..].iter().any(|l| !weapon(l)["projectiles"].as_array().unwrap().is_empty()), "no missile flew");
    assert!(lines[1..].iter().any(|l| weapon(l)["heat"].as_f64().unwrap() > 50.0), "the gun never got hot");
    assert!(lines[1..].iter().any(|l| !l["aim"].is_null()), "never in weapon mode");
    assert_eq!(lines.last().unwrap()["scouts"][0]["dead"], true, "the first scout lives");
}

#[test]
fn the_fight_run_hits_and_is_hit() {
    let Some(lines) = play(include_str!("../runs/fight.json")) else { return };
    let taken: f64 = lines[1..].iter().flat_map(|l| l["taken"].as_array().unwrap()).map(|t| t["damage"].as_f64().unwrap()).sum();
    assert!(taken > 10.0, "he took {taken}");
    assert!(lines[1..].iter().any(|l| !l["hits"].as_array().unwrap().is_empty()), "no melee hit landed");
    assert!(lines[1..].iter().any(|l| !l["weapon"]["struck"].as_array().unwrap().is_empty()), "no round hit");
    assert_eq!(lines.last().unwrap()["scouts"][0]["dead"], true, "the scout lives");
}

#[test]
fn a_button_the_game_lacks_is_refused() {
    let Some(spec) = bumblebee() else { return };
    let script = json!({"format_version": 2, "steps": [{"press": ["Punch"]}]});
    let error = rotf_export::run::run(tf2_core::install::dir(), &spec, &script).unwrap_err();
    assert!(error.contains("no button"), "{error}");
    let old = json!({"format_version": 1, "steps": [{"updates": 1}]});
    assert!(rotf_export::run::run(tf2_core::install::dir(), &spec, &old).unwrap_err().contains("version"));
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
