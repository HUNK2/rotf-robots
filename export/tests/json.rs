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
    // The control states: each clip is one of `clips`, each transition names a connection and a
    // state that are there, each connection list's members are there, and the start state is.
    let control = &json["control"];
    let names_of = |key: &str| -> HashSet<&str> { control[key].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect() };
    let (states, connections) = (names_of("states"), names_of("connections"));
    let clip_names: HashSet<&str> = json["clips"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert!(states.contains(control["start_state"].as_str().unwrap()), "no start state");
    for state in control["states"].as_array().unwrap() {
        if let Some(clip) = state["clip"].as_str().filter(|c| !clip_names.contains(c)) {
            problems.push(format!("{}: no clip {clip}", state["name"]));
        }
        for t in state["transitions"].as_array().unwrap() {
            if !connections.contains(t["connection"].as_str().unwrap()) || t["to"].as_str().is_some_and(|s| !states.contains(s)) {
                problems.push(format!("{}: transition {t}", state["name"]));
            }
        }
    }
    for c in control["connections"].as_array().unwrap().iter().filter(|c| c["test"] == "all") {
        if c["connections"].as_array().unwrap().iter().any(|m| !connections.contains(m.as_str().unwrap())) {
            problems.push(format!("{}: a member is missing", c["name"]));
        }
    }
    // The clip rules: each clip and each rule's target clip is one of `clips`, and every rule's
    // variable has its name.
    for set in json["animation_rules"]["sets"].as_array().unwrap() {
        let clips = set["clips"].as_array().unwrap();
        let exits = clips.iter().flat_map(|c| c["exits"].as_array().unwrap());
        for rule in set["entries"].as_array().unwrap().iter().chain(exits) {
            if let Some(clip) = rule["to_clip"].as_str().filter(|c| !clip_names.contains(c)) {
                problems.push(format!("{}: no clip {clip}", set["set"]));
            }
            for r in rule["rules"].as_array().unwrap() {
                if r["variable"].as_str().unwrap().starts_with('#') {
                    problems.push(format!("{}: unnamed variable {}", set["set"], r["variable"]));
                }
            }
        }
        for clip in clips.iter().filter(|c| !clip_names.contains(c["clip"].as_str().unwrap())) {
            problems.push(format!("{}: no clip {}", set["set"], clip["clip"]));
        }
    }
    // The mechanics: every clip they name is one of `clips`, and each jump's launch speed is the
    // one that peaks at its height.
    let mechanics = &json["mechanics"];
    assert!(mechanics["error"].is_null(), "mechanics: {}", mechanics["error"]);
    let named = ["stop_clip", "land_run_clip", "big_land_run_clip"].map(|k| &mechanics["on_foot"][k]);
    for clip in named.into_iter().chain([&mechanics["jump"]["takeoff_clip"]]).filter_map(Value::as_str) {
        if !clip_names.contains(clip) {
            problems.push(format!("mechanics: no clip {clip}"));
        }
    }
    let gravity = mechanics["jump"]["gravity"].as_f64().unwrap();
    for kind in mechanics["jump"]["types"].as_array().unwrap() {
        if let Some(height) = kind["height"].as_f64() {
            let launch = kind["launch_speed"].as_f64().unwrap();
            if (launch - (2.0 * gravity * height).sqrt()).abs() > 1e-3 {
                problems.push(format!("mechanics: {} launches at {launch}", kind["type"]));
            }
        }
    }
    // The car: Bumblebee drives, his inertia is his box's, and the turbo goes faster than the car.
    let car = &mechanics["car"];
    assert!(export.spec.pack != "bumblebee" || car.is_object(), "Bumblebee has no car in mechanics");
    if car.is_object() {
        let n = |k: &str| car[k].as_f64().unwrap_or_else(|| panic!("car.{k} is not a number"));
        let half: Vec<f64> = car["body_half_size"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let yaw = n("mass") / 3.0 * (half[0] * half[0] + half[1] * half[1]);
        if (car["inertia"][2].as_f64().unwrap() - yaw).abs() > 1.0 {
            problems.push(format!("mechanics: car inertia {} against {yaw}", car["inertia"]));
        }
        if n("max_turbo_speed") <= n("max_speed") {
            problems.push("mechanics: the turbo is no faster than the car".into());
        }
    }
    // Flight and hover come together, and only for a robot that flies.
    if mechanics["fly"].is_null() != mechanics["hover"].is_null() {
        problems.push("mechanics: fly and hover disagree on whether he flies".into());
    }
    // Climbing: Bumblebee climbs, every clip it names is one of `clips`, and the test starts
    // where his body shape does.
    let climb = &mechanics["climb"];
    assert!(export.spec.pack != "bumblebee" || climb.is_object(), "Bumblebee has no climb in mechanics");
    if climb.is_object() {
        let hands = climb["clips"].as_object().unwrap().values().flat_map(|v| v.as_array().unwrap().iter());
        let ones = ["catch_clip", "catch_clip_ground", "jump_up_clip", "jump_off_clip"].map(|k| &climb[k]);
        for clip in hands.chain(ones) {
            match clip.as_str() {
                Some(name) if !clip_names.contains(name) => problems.push(format!("mechanics: no clip {name}")),
                None => problems.push(format!("mechanics: a climb clip is missing ({})", climb["clips"])),
                _ => {}
            }
        }
        let n = |k: &str| climb[k].as_f64().unwrap_or_else(|| panic!("climb.{k} is not a number"));
        if (n("body_offset") - n("height") * 24.0 / 121.0).abs() > 1e-4 {
            problems.push(format!("mechanics: climb body_offset {} for height {}", n("body_offset"), n("height")));
        }
    }
    // The dodge: Bumblebee dodges, and every dodge clip named is one of `clips`.
    let dodge = &mechanics["dodge"];
    assert!(export.spec.pack != "bumblebee" || dodge.is_object(), "Bumblebee has no dodge in mechanics");
    if dodge.is_object() {
        let named = ["clips", "hover_clips"].into_iter().flat_map(|k| dodge[k].as_object().unwrap().values());
        for clip in named.filter_map(Value::as_str).filter(|c| !clip_names.contains(c)) {
            problems.push(format!("mechanics: no clip {clip}"));
        }
        if dodge["clips"]["front"].is_null() {
            problems.push("mechanics: a dodge without its front clip".into());
        }
    }
    // Melee, the weapons and damage are there for every robot, and he has health.
    let upgrades = mechanics["melee"]["upgrades"].as_array().map_or(0, Vec::len);
    if upgrades != 3 {
        problems.push(format!("mechanics: {upgrades} melee upgrade rows"));
    }
    if !mechanics["weapons"]["aim"].is_object() {
        problems.push("mechanics: no weapons.aim".into());
    }
    if mechanics["damage"]["base_health"].as_f64().is_none_or(|h| h <= 0.0) {
        problems.push(format!("mechanics: base health {}", mechanics["damage"]["base_health"]));
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

    // Collision bodies: every body of every object, once, each a node of its file by its
    // `gltf_node`, the child of the node it rides, placed as the JSON says.
    let files = json["collision"]["files"].as_array().unwrap();
    for piece in export.pieces() {
        let file = format!("{}.glb", piece.file);
        let entry = files.iter().find(|f| f["file"] == file.as_str());
        let bodies = entry.map_or(&[][..], |e| e["bodies"].as_array().unwrap());
        assert_eq!(bodies.len(), piece.object.collisions.len(), "{file}: collision bodies");
        if bodies.is_empty() {
            continue;
        }
        let glb = export.read(&file);
        for (body, source) in bodies.iter().zip(&piece.object.collisions) {
            let name = body["gltf_node"].as_str().unwrap();
            let node = glb.document.nodes().find(|n| n.name() == Some(name)).unwrap_or_else(|| panic!("{file}: no node {name}"));
            let parent = glb.document.nodes().find(|n| n.children().any(|c| c.index() == node.index())).and_then(|n| n.name().map(str::to_owned));
            assert_eq!(parent.as_deref(), Some(piece.object.nodes[source.node].name.as_str()), "{file}: {name} rides the wrong node");
            assert_eq!(body["node"].as_str(), parent.as_deref(), "{file}: {name}");
            let (t, r, s) = node.transform().decomposed();
            let at = glam::Mat4::from_scale_rotation_translation(s.into(), glam::Quat::from_array(r), t.into());
            let want = rotf_export::model::matrix(&source.matrix);
            assert!(at.abs_diff_eq(want, 1e-4), "{file}: {name} is placed at {at:?}, the data has {want:?}");
            let extras: Value = serde_json::from_str(node.extras().as_ref().unwrap().get()).unwrap();
            assert_eq!(extras["rotf_collision"]["shape"], body["shape"], "{file}: {name}");
        }
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
