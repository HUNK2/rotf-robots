//! Writes Bumblebee and checks `bumblebee.json` against the files beside it and the source: every
//! animation, node, file and texture it names is there; each clip carries as many events of each
//! kind as the clip itself; and the change of form starts the vehicle's clips and swaps the models
//! at the ticks the game's own driver does (`core/examples/form_swap_frames`: the car's
//! `Trans_R2V_A` from tick 0, its `Trans_R2V_B` with the robot's part B, the robot hidden at
//! part B's tick 14). Both files carry `format_version`, and `docs/FORMAT.md` names every key
//! and event type the JSON uses. Reads the install; skips without it.

mod common;

use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::Value;

#[test]
fn bumblebee_json_names_only_what_exists() {
    let Some(export) = common::export("bumblebee", "json") else { return };
    let text = std::fs::read_to_string(export.out.join(&export.written.json)).unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let attributes: Value = serde_json::from_str(&std::fs::read_to_string(export.out.join(json["attributes_file"].as_str().unwrap())).unwrap()).unwrap();
    assert!(attributes["attributes"]["baseHealth"].is_number(), "the attribute block has no baseHealth");
    for file in [&json, &attributes] {
        assert_eq!(file["format_version"], rotf_export::FORMAT_VERSION, "format_version");
    }
    let spec = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/FORMAT.md")).unwrap();
    let missing = undocumented(&json, &spec);
    assert!(missing.is_empty(), "docs/FORMAT.md does not name {missing:?}");

    // Each file's animation and node names, read back.
    let mut animations: HashMap<String, HashSet<String>> = HashMap::new();
    let mut nodes: HashMap<String, HashSet<String>> = HashMap::new();
    for (file, _) in &export.written.files {
        let glb = export.read(file);
        animations.insert(file.clone(), glb.document.animations().map(|a| a.name().unwrap().to_owned()).collect());
        nodes.insert(file.clone(), glb.document.nodes().filter_map(|n| n.name().map(str::to_owned)).collect());
    }
    let has_animation = |file: &Value, name: &Value| {
        animations.get(file.as_str().unwrap_or("")).is_some_and(|a| name.as_str().is_some_and(|n| a.contains(n)))
    };
    let mut problems = Vec::new();
    let mut unnamed = 0;
    let mut flagged = 0;
    let robot_file = format!("{}_robot.glb", export.spec.pack);
    for clip in json["clips"].as_array().unwrap() {
        let (file, name) = (&clip["file"], &clip["name"]);
        if !clip["animation"].is_null() && !has_animation(file, &clip["animation"]) {
            problems.push(format!("{name} is not in {file}"));
        }
        for event in clip["events"].as_array().unwrap() {
            let at = format!("{name} tick {} {}", event["tick"], event["type"]);
            // A node the file lacks must be flagged as such, and only then.
            if let Some(node) = event["node"].as_str() {
                let there = nodes[file.as_str().unwrap()].contains(node);
                if there == (event["node_in_file"] == false) {
                    problems.push(format!("{at}: node {node} is {} {file}, flagged {}", if there { "in" } else { "not in" }, event["node_in_file"]));
                }
                flagged += usize::from(!there);
            }
            match event["type"].as_str().unwrap() {
                "model" => {
                    for key in ["vehicle_animation", "vehicle_then"] {
                        if !event[key].is_null() && !event["vehicle_file"].is_null() && !has_animation(&event["vehicle_file"], &event[key]) {
                            problems.push(format!("{at}: {} is not in {}", event[key], event["vehicle_file"]));
                        }
                    }
                }
                "weapon_attach" if !event["weapon_file"].is_null() => {
                    for key in ["weapon_animation", "weapon_then"] {
                        if !event[key].is_null() && !has_animation(&event["weapon_file"], &event[key]) {
                            problems.push(format!("{at}: {} is not in {}", event[key], event["weapon_file"]));
                        }
                    }
                }
                "partial_animation" if !has_animation(&Value::from(robot_file.as_str()), &event["animation"]) => {
                    problems.push(format!("{at}: {} is not in {robot_file}", event["animation"]));
                }
                _ => {}
            }
            unnamed += event.as_object().unwrap().values().filter(|v| v.as_str().is_some_and(|s| s.starts_with('#'))).count();
        }
    }
    for weapon in json["weapons"].as_array().unwrap().iter().chain(json["melee_weapons"].as_array().unwrap()) {
        for key in ["file", "missile_file", "grenade_file", "turret_file"] {
            if let Some(file) = weapon[key].as_str()
                && !animations.contains_key(file)
            {
                problems.push(format!("{}: no file {file}", weapon["weapon"]));
            }
        }
        let start = weapon["start_node"].as_str().unwrap();
        let on = match weapon["hangs_on"].as_str() {
            Some("vehicle") => format!("{}_vehicle.glb", export.spec.pack),
            Some(_) => robot_file.clone(),
            None => continue,
        };
        if !nodes[&on].contains(start) {
            problems.push(format!("{}: no node {start} in {on}", weapon["weapon"]));
        }
    }
    for material in json["materials"].as_array().unwrap() {
        for parameter in material["parameters"].as_array().unwrap() {
            if let Some(path) = parameter["value"].as_str().filter(|p| p.starts_with("textures/"))
                && !export.out.join(path).exists()
            {
                problems.push(format!("{}: no {path}", material["name"]));
            }
        }
    }
    assert!(problems.is_empty(), "{} problems:\n{}", problems.len(), problems.join("\n"));

    // Render sets: each mesh they list is a node of its file by that name (or `<name>.<part>`),
    // and exactly the meshes outside an exclusive group's first set start hidden.
    for entry in json["render_sets"].as_array().unwrap() {
        let file = entry["file"].as_str().unwrap();
        let glb = export.read(file);
        let mut listed = HashMap::new();
        for group in entry["groups"].as_array().unwrap() {
            let exclusive = group["exclusive"].as_bool().unwrap();
            for (i, set) in group["sets"].as_array().unwrap().iter().enumerate() {
                for mesh in set["meshes"].as_array().unwrap() {
                    let shown = listed.entry(mesh.as_str().unwrap()).or_insert(!exclusive);
                    *shown |= i == 0;
                }
            }
        }
        let mut hidden_nodes = 0;
        for (model, shown) in &listed {
            let parts: Vec<_> = glb
                .document
                .nodes()
                .filter(|n| n.mesh().is_some())
                .filter(|n| n.name().is_some_and(|name| name == *model || name.strip_prefix(*model).is_some_and(|rest| rest.starts_with('.') && rest[1..].parse::<u32>().is_ok())))
                .collect();
            assert!(!parts.is_empty(), "{file}: no mesh node named {model}");
            for node in parts {
                let extras: Value = serde_json::from_str(node.extras().as_ref().map_or("{}", |r| r.get())).unwrap();
                assert_eq!(extras["render_set_hidden"] == true, !shown, "{file}: {model}");
                hidden_nodes += usize::from(!shown);
            }
        }
        println!("{file}: {} render-set meshes, {hidden_nodes} mesh nodes start hidden", listed.len());
    }

    // Every event of every clip is written once: counted against the source, kind by kind.
    // By file and name: an object's own clips (a weapon's `Idle`) repeat names across files.
    let entries: HashMap<(&str, &str), &Value> =
        json["clips"].as_array().unwrap().iter().map(|c| ((c["file"].as_str().unwrap(), c["name"].as_str().unwrap()), c)).collect();
    let count = |entry: &Value, kinds: &[&str]| entry["events"].as_array().unwrap().iter().filter(|e| kinds.contains(&e["type"].as_str().unwrap())).count();
    for piece in export.pieces() {
        let file = format!("{}.glb", piece.file);
        for clip in &piece.clips {
            let entry = entries[&(file.as_str(), clip.name.as_str())];
            let c = clip.clip;
            let triggers = c.action_events.iter().filter(|e| matches!(e.action, tf2_core::formats::anim::Action::Trigger { .. })).count();
            let local_triggers = c.trigger_events.iter().filter(|t| !t.on_exit).count();
            let expected = [
                (c.model_events.len(), count(entry, &["model"])),
                (c.sound_events.iter().filter(|e| matches!(e.cue, tf2_core::formats::anim::SoundCue::Play { .. } | tf2_core::formats::anim::SoundCue::Stop { .. })).count(), count(entry, &["sound_play", "sound_stop"])),
                (c.effect_events.len(), count(entry, &["effect_start", "effect_stop", "footstep", "climb_step"])),
                (c.weapon_events.len(), count(entry, &["weapon_attach", "weapon_fire_enabled"])),
                (c.shake_events.len(), count(entry, &["camera_shake"])),
                (c.special_fire_events.len(), count(entry, &["special_fire"])),
                // A local trigger is in both lists: once in the JSON.
                (c.trigger_events.len() + triggers - triggers.min(local_triggers), count(entry, &["trigger"])),
            ];
            for (k, (source, written)) in expected.iter().enumerate() {
                assert_eq!(source, written, "{}: event kind {k}", clip.name);
            }
        }
    }

    println!(
        "{} clips, {} weapons, {} materials; {unnamed} event names given by hash, {flagged} event nodes not in their file",
        entries.len(),
        json["weapons"].as_array().unwrap().len(),
        json["materials"].as_array().unwrap().len()
    );
    // The change of form against the game's driver (Bumblebee's).
    if export.spec.pack != "bumblebee" {
        return;
    }
    let model = |clip: &str| -> Vec<(f64, bool, bool, Option<String>)> {
        entries[&(robot_file.as_str(), clip)]["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "model")
            .map(|e| (e["tick"].as_f64().unwrap(), e["show_robot"].as_bool().unwrap(), e["show_vehicle"].as_bool().unwrap(), e["vehicle_animation"].as_str().map(str::to_owned)))
            .collect()
    };
    let a = model("DriveSet.Trans_R2V_A");
    let b = model("DriveSet.Trans_R2V_B");
    assert_eq!(a[0], (0.0, true, true, Some("VehicleModel.Trans_R2V_A".into())), "{a:?}");
    assert_eq!(b[0], (0.0, true, true, Some("VehicleModel.Trans_R2V_B".into())), "{b:?}");
    assert!(b.contains(&(14.0, false, true, None)), "{b:?}");
}

/// The keys and event types of the JSON that `spec` (docs/FORMAT.md) does not name as `key`: the
/// spec keeps up with the exporter. The game's own data under a weapon's `numbers` is not the
/// format's, nor are the names that key `sounds.events` and `sounds.samples`.
fn undocumented(json: &Value, spec: &str) -> Vec<String> {
    fn walk(v: &Value, keys: &mut BTreeSet<String>) {
        match v {
            Value::Object(map) => {
                for (k, x) in map {
                    keys.insert(k.clone());
                    match (k.as_str(), x) {
                        ("numbers", _) => {}
                        ("events" | "samples", Value::Object(named)) => named.values().for_each(|y| walk(y, keys)),
                        _ => walk(x, keys),
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|x| walk(x, keys)),
            _ => {}
        }
    }
    let mut keys = BTreeSet::new();
    walk(json, &mut keys);
    for clip in json["clips"].as_array().unwrap() {
        keys.extend(clip["events"].as_array().unwrap().iter().map(|e| e["type"].as_str().unwrap().to_owned()));
    }
    keys.into_iter().filter(|k| !spec.contains(&format!("`{k}`"))).collect()
}
