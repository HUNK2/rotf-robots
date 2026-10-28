//! The character's `<pack>.json`: what glTF cannot hold. Every clip's events as the port reads
//! them (`tf2_core::formats::anim`), its attack and its root's travel; where each weapon hangs and
//! what it fires from; the render sets; every material parameter; and the character's attribute
//! block as the game stores it, field by field.

use std::collections::HashSet;
use std::path::Path;

use serde_json::{Map, Value, json};
use tf2_core::character::CharacterData;
use tf2_core::formats::anim::{Action, AttackRef, Clip, EffectEventKind, SoundCue, WeaponEventKind};
use tf2_core::formats::characterlist::Spec;
use tf2_core::formats::lxb::Node;
use tf2_core::formats::model::ParamValue;
use tf2_core::tuning::{Actions, Locomotion};
use tf2_core::weapons::{self, FIRE_DUMMIES, Mount};

use crate::model::{self, ClipOut, Shared, Summary, blend_of};
use crate::names::Names;
use crate::{Piece, PieceKind};

/// A float as the shortest decimal that reads back as the same `f32`, so the JSON says 0.3 and not
/// 0.30000001192092896; null for one that is not finite.
pub fn num(v: f32) -> Value {
    v.to_string().parse::<f64>().ok().and_then(serde_json::Number::from_f64).map_or(Value::Null, Value::Number)
}

fn nums(v: &[f32]) -> Value {
    Value::Array(v.iter().map(|&x| num(x)).collect())
}

/// Rounded to a tenth of a millimetre (or of a thousandth of a radian), for the per-tick lists.
fn fine(v: f32) -> Value {
    num((v * 1e4).round() / 1e4)
}

/// The climbing steps' limbs, in `anim::CLIMB_SIDES`' order.
pub const LIMBS: [&str; 5] = ["left_hand", "right_hand", "left_foot", "right_foot", "slide"];

pub fn character(install: &Path, spec: &Spec, data: &CharacterData, pieces: &[Piece], written: &[(String, Summary)], shared: &mut Shared, names: &Names) -> Value {
    let file_of = |kind: PieceKind, weapon: u32| {
        pieces.iter().find(|p| p.kind == kind && p.weapons.contains(&weapon)).map(|p| format!("{}.glb", p.file))
    };
    let vehicle_file = pieces.iter().find(|p| p.kind == PieceKind::Vehicle).map(|p| format!("{}.glb", p.file));
    let held = |weapon: u32| file_of(PieceKind::Weapon, weapon).or_else(|| file_of(PieceKind::Melee, weapon));
    let mut clips: Vec<Value> = Vec::new();
    let robot_rig = tf2_core::pose::Rig::new(&data.robot);
    for piece in pieces {
        let file = format!("{}.glb", piece.file);
        let nodes: HashSet<&str> = piece.object.nodes.iter().map(|n| n.name.as_str()).collect();
        let left_out = written.iter().find(|(f, _)| *f == file).map_or(&[][..], |(_, s)| &s.other_model_clips[..]);
        for clip in &piece.clips {
            let mut entry = clip_entry(clip, &file, &nodes, !left_out.contains(&clip.name), vehicle_file.as_deref(), &held, names);
            if piece.kind == PieceKind::Robot {
                attack_bodies_entry(&mut entry, &robot_rig, &data.robot, clip, names);
            }
            // The vehicle's animator leaves a node its clip has no channel for where the clip
            // before left it [game 00544e00]; glTF puts it back at rest. (Starscream's `V_Idle`
            // leaves 16 of the jet's nodes unkeyed: alone, it shows the jet folded small.)
            if piece.kind == PieceKind::Vehicle {
                // A channel that keys nothing is not one of these: the node keeps its rest. The
                // glTF has no channel for either, so the names tell them apart.
                let unkeyed: Vec<&str> = piece
                    .object
                    .nodes
                    .iter()
                    .filter(|n| clip.clip.channel(n.name_hash).is_none())
                    .map(|n| n.name.as_str())
                    .collect();
                if !unkeyed.is_empty() {
                    entry["unkeyed_nodes"] = json!(unkeyed.len());
                    entry["unkeyed_node_names"] = json!(unkeyed);
                }
            }
            clips.push(entry);
        }
    }
    let robot_nodes: Vec<u32> = data.robot.nodes.iter().map(|n| n.name_hash).collect();
    let car_nodes: Option<Vec<u32>> = data.vehicle.as_ref().map(|v| v.nodes.iter().map(|n| n.name_hash).collect());
    // Which object the start node is on (`weapons::mount`); null when neither has it, and the
    // weapon hangs nowhere until a clip's `weapon_attach` event names a node.
    let hangs_on = |node: u32| match weapons::mount(node, &robot_nodes, car_nodes.as_deref()) {
        Some(Mount::Robot(_)) => json!("robot"),
        Some(Mount::Car(_)) => json!("vehicle"),
        None => Value::Null,
    };
    // Each weapon's muzzle light (`point_light::load_flash_lights`). [data]
    let flash_lights = tf2_core::point_light::load_flash_lights(install, &spec.pack).unwrap_or_default();
    let weapons: Vec<Value> = data
        .weapons
        .iter()
        .map(|def| {
            let hangs = hangs_on(def.bone);
            let named = |pairs: &[(&str, u32)]| -> Value {
                Value::Object(pairs.iter().filter(|(_, h)| *h != 0).map(|(k, h)| ((*k).to_owned(), json!(names.name(*h)))).collect())
            };
            json!({
                "weapon": names.name(def.name),
                "kind": format!("{:?}", def.kind).to_lowercase(),
                "list": def.list,
                "file": file_of(PieceKind::Weapon, def.name),
                "start_node": names.name(def.bone),
                "hangs_on": hangs,
                "barrel_nodes": (0..def.barrel_count).filter_map(|k| FIRE_DUMMIES.get(def.first_barrel + k)).map(|&h| names.name(h)).collect::<Vec<_>>(),
                "barrels": def.barrels[..def.barrel_count.min(3)].iter().map(|b| nums(&b.to_array())).collect::<Vec<_>>(),
                "muzzle": nums(&def.muzzle.to_array()),
                "missile_file": file_of(PieceKind::Missile, def.name),
                "grenade_file": file_of(PieceKind::Grenade, def.name),
                "turret_file": file_of(PieceKind::Turret, def.name),
                "turret_missile_file": file_of(PieceKind::Missile, tf2_core::character::turret_gun_key(def.name)),
                "always_appear": def.always_appear,
                // The explosion its round makes where it ends (a missile's, a grenade's), and the
                // alternate round's (`chargeMaxProjectileObj`), with no trigger, node or rumble.
                "explosion": def.explosion.as_ref().map_or(Value::Null, |e| crate::mechanics::explosion(e, names)),
                "alt_explosion": def.alt_explosion.as_ref().filter(|_| def.alt_projectile).map_or(Value::Null, |e| crate::mechanics::explosion(e, names)),
                "object_clips": named(&[
                    ("idle", def.idle_clip),
                    ("exit", def.exit_clip),
                    ("exit_overheat", def.exit_overheat_clip),
                    ("overheat", def.overheat_clip),
                    ("charging", def.charging_clip),
                    ("charged", def.charged_clip),
                ]),
                "robot_exit_clip": (def.exit_arm_clip != 0).then(|| names.name(def.exit_arm_clip)),
                "sounds": named(&[
                    ("fire", def.fire_sound),
                    ("overheat", def.overheat_sound),
                    ("refused", def.refused_sound),
                    ("ring_off", def.ring_off_sound),
                    ("idle", def.idle_sound),
                    ("charge_start", def.charge_start_sound),
                    ("charge_loop", def.charge_loop_sound),
                    ("charge_full", def.charge_full_sound),
                    ("charge_fire", def.charge_fire_sound),
                ]),
                // As the overheat lock runs out its kept sound is keyed off (true) or stopped
                // with its fade (`fireOverHeatKeyOff`).
                "overheat_key_off": def.overheat_key_off,
                "numbers_from": names.name(def.attribute),
                "numbers": root_by_hash(names, def.attribute).map_or(Value::Null, |n| value(n, names, 0)),
                "round": crate::rounds::row(data, def.name).map(|r| crate::rounds::round(r, pieces, names)),
                "alt_round": crate::rounds::row(data, tf2_core::character::alt_projectile_key(def.name)).map(|r| crate::rounds::round(r, pieces, names)),
                "muzzle_light": flash_lights.iter().find(|(name, _)| *name == def.name).map(|(_, f)| crate::rounds::flash(f)),
            })
        })
        .collect();
    let melee: Vec<Value> = data
        .melee_weapons
        .iter()
        .map(|m| {
            json!({
                "weapon": names.name(m.name),
                "file": file_of(PieceKind::Melee, m.name),
                "start_node": names.name(m.start_node),
                "hangs_on": hangs_on(m.start_node),
                "exit_clip": (m.exit_clip != 0).then(|| names.name(m.exit_clip)),
            })
        })
        .collect();
    let render_sets: Vec<Value> = pieces
        .iter()
        .filter(|p| !p.object.render_sets.is_empty())
        .map(|p| {
            let groups: Vec<Value> = p
                .object
                .render_sets
                .iter()
                .map(|group| {
                    let sets: Vec<Value> = (0..group.sets.len())
                        .map(|i| {
                            let mut meshes: Vec<&str> =
                                p.object.renders.iter().map(|r| r.model.as_str()).filter(|m| group.contains_model(m, i)).collect();
                            meshes.dedup();
                            json!({ "name": names.name(group.sets[i].name), "meshes": meshes })
                        })
                        .collect();
                    // An exclusive group starts on its first set. [game 00803150]
                    let default = group.exclusive.then(|| group.sets.first().map(|s| names.name(s.name))).flatten();
                    json!({ "exclusive": group.exclusive, "default": default, "sets": sets })
                })
                .collect();
            json!({ "file": format!("{}.glb", p.file), "groups": groups })
        })
        .collect();
    json!({
        "format_version": crate::FORMAT_VERSION,
        "exporter": { "name": "rotf-export", "version": env!("CARGO_PKG_VERSION") },
        "character": {
            "name": spec.display.as_deref().unwrap_or(&spec.pack),
            "pack": spec.pack,
            "attributes": spec.attributes,
            "name_id": names.name(data.name_id),
        },
        "space": {
            "game": "x right, y forward, z up, metres; right-handed",
            "gltf": "y up, facing +Z, metres: each file's root node turns the game's space into glTF's",
            "clip_tick_seconds": 1.0 / 60.0,
            "game_step_seconds": 0.032,
            "event_ticks": "clip ticks from the clip's start",
            "root_travel": "per tick from 0 to the clip's length: metres in the root's own frame at the start (x right, y forward, z up), and degrees turned about z (positive toward -x)",
        },
        "files": pieces.iter().map(|p| json!({
            "file": format!("{}.glb", p.file),
            "kind": p.kind.word(),
            "weapons": p.weapons.iter().map(|&w| names.name(w)).collect::<Vec<_>>(),
            "animations": p.clips.len(),
        })).collect::<Vec<_>>(),
        "clips": clips,
        "weapons": weapons,
        "melee_weapons": melee,
        "render_sets": render_sets,
        "vehicle": data.vehicle.as_ref().map(|car| {
            const WHEELS: [&str; 4] = ["front_left", "front_right", "rear_left", "rear_right"];
            let wheels: Vec<Value> = car
                .nodes
                .iter()
                .filter_map(|n| tf2_core::vehicle::wheel_of_node(n.name_hash).map(|w| json!({ "node": n.name, "wheel": WHEELS[w] })))
                .collect();
            let steering: Vec<&str> = car.nodes.iter().map(|n| n.name.as_str()).filter(|n| n.starts_with("TireGOAL_Front")).collect();
            json!({
                "file": vehicle_file,
                "clips_hold_unkeyed_nodes": "a vehicle clip with unkeyed_nodes leaves those nodes where the clip before it left them, as the game plays it; glTF returns them to rest. Play the change of form (Trans_R2V_A, then Trans_R2V_B) before V_Idle, or hold the last clip's pose for those nodes.",
                "wheels": wheels,
                "steering_nodes": steering,
                "note": "the game moves these in code every frame, on top of the vehicle's clip: each tyre spins by its wheel's travel and is lowered by its wheel's suspension to the ground, and the steering nodes turn with the steering (rotf-robots' game/src/animation.rs). The clips alone leave the tyres where the change of form puts them, off the ground.",
            })
        }),
        "collision": collision(pieces, names),
        "materials": materials(shared, names),
        "summary": summary(install, spec, data),
        "character_sounds": character_sounds(data, names),
        "attributes_file": format!("{}_attributes.json", spec.pack),
        "not_exported": [
            "the voice lines a bank lacks (chatter lines with no sample in audio\\eng\\CHATTER.fsb)",
            "sound events the game's data names that no sound file holds (Jetfire's ANIM_JETFR_BOMB_DROP_RINGOFF)",
            "the clip fields the port does not read (chance, minWalkSpd, maxWalkSpd, minStrafeAng, maxStrafeAng, walkFlags, group_type): how they pick between clips is not read",
            "what each control state class does in code beyond its data (the dodge's direction, the drive and fly states' own steps): docs/state-machine.md and core/src/sim.rs",
            "flight's moving surfaces (flaps, rotors), the wheels' spin and the suspension: the game moves them in code",
        ],
    })
}

/// Every file's collision bodies (`scene::ObjectDef::collisions`), each with the node it rides
/// and the glTF node that stands for it.
fn collision(pieces: &[Piece], names: &Names) -> Value {
    let files: Vec<Value> = pieces
        .iter()
        .filter(|p| !p.object.collisions.is_empty())
        .map(|p| {
            let bodies: Vec<Value> = p
                .object
                .collisions
                .iter()
                .enumerate()
                .map(|(k, c)| {
                    let node = &p.object.nodes[c.node].name;
                    let mut body = model::collision(c, names);
                    body["node"] = json!(node);
                    body["gltf_node"] = json!(model::collision_node(node, k));
                    body
                })
                .collect();
            json!({ "file": format!("{}.glb", p.file), "bodies": bodies })
        })
        .collect();
    json!({
        "note": "each body rides its node (not the node's .frame) and moves with it. translation, rotation (x, y, z, w) and scale place it in the node's space, game axes; a cylinder stands along its own z. radius, half_height and half_extents are as stored; scaled gives them with the scale in, as the port reads them (a sphere and a cylinder's radius take the x scale, a half height the z): that reading is assumed, not read in the game. mask_preset says what the body hits and what hits it; material, on a character, the part struck.",
        "files": files,
    })
}

/// The sound events the character's data names outside its clips: its sound table (the engine,
/// turbo, suspension, a flyer's drive and hover loops) and, by the surface touched, the sounds
/// and effects of its steps, melee hits, climbing and tyres. Each event is in the JSON's `sounds`.
fn character_sounds(data: &CharacterData, names: &Names) -> Value {
    let event = |id: u32| if id == 0 { Value::Null } else { json!(names.name(id)) };
    let preset = |p: &tf2_core::character::ImpactPreset| {
        p.infos
            .iter()
            .map(|info| {
                json!({
                    "surfaces": info.materials.iter().map(|&m| names.name(m)).collect::<Vec<_>>(),
                    "sound": event(info.sound),
                    "effect": info.effect.map(|e| names.name(e)),
                    "effect_flags": info.effect_flags,
                })
            })
            .collect::<Vec<_>>()
    };
    let s = &data.sounds;
    json!({
        "engine": event(s.engine),
        "drift_start": event(s.drift_start),
        "suspension": event(s.suspension),
        "turbo": event(s.turbo),
        "turbo_ready": event(s.turbo_ready),
        "drive": event(s.drive),
        "hover": event(s.hover),
        "by_surface": {
            "note": "the entry for a surface is the first that lists it, else the first that lists the default surface",
            "step": preset(&data.step_impact),
            "melee_hit": preset(&data.melee_impact),
            "climb": data.climb_impact.iter().map(preset).collect::<Vec<_>>(),
            "tyre_skid": preset(&data.tyre_skid),
            "tyre_road": preset(&data.tyre_road),
        },
    })
}

/// The character's attribute block (`characterAttributes<X>` in `bnxglobal.str`) as the game
/// stores it, field by field: its moves' numbers, its avatar bodies, its control state machine
/// (`stateMachineList`) and its computer-run brains (`brainList`). Its own file: most of it is
/// the state machine.
pub fn attributes(spec: &Spec, names: &Names) -> Value {
    let root = format!("characterAttributes{}", spec.attributes);
    let found = names.files().iter().find_map(|f| f.root().get(&root));
    json!({
        "format_version": crate::FORMAT_VERSION,
        "root": root,
        "attributes": found.map_or(Value::Null, |n| value(n, names, 0)),
    })
}

/// A root of the data by its name's hash (a weapon's numbers in `bnxglobal.str`).
fn root_by_hash(names: &Names, hash: u32) -> Option<Node<'_>> {
    (hash != 0).then(|| names.files().iter().find_map(|f| f.root().field(hash))).flatten()
}

/// One clip: its reference's settings (the animation's extras), its attack, its root's travel,
/// and its events in tick order. A clip that moves no node of its object is not in the file
/// (`animation` null), but its events still are.
fn clip_entry(clip: &ClipOut, file: &str, nodes: &HashSet<&str>, in_file: bool, vehicle_file: Option<&str>, held: &dyn Fn(u32) -> Option<String>, names: &Names) -> Value {
    let mut entry = Map::new();
    entry.insert("animation".into(), if in_file { json!(clip.name) } else { Value::Null });
    entry.insert("name".into(), json!(clip.name));
    entry.insert("file".into(), json!(file));
    if let Value::Object(extras) = &clip.extras {
        for (k, v) in extras {
            entry.insert(k.trim_start_matches("rotf_").to_owned(), v.clone());
        }
    }
    if let Some(attack) = &clip.attack {
        entry.insert("attack".into(), attack_entry(attack));
    }
    let path = clip.clip.root_path();
    let turn = clip.clip.root_turn();
    if path.iter().any(|p| p.length() > 1e-3) || turn.iter().any(|t| t.abs() > 1e-4) {
        entry.insert("root_travel".into(), Value::Array(path.iter().map(|p| Value::Array(p.to_array().map(fine).to_vec())).collect()));
        entry.insert("root_turn_degrees".into(), Value::Array(turn.iter().map(|t| fine(t.to_degrees())).collect()));
    }
    entry.insert("events".into(), Value::Array(events(clip.clip, nodes, vehicle_file, held, names)));
    Value::Object(entry)
}

/// The bodies the clip's `attack_hit` events switch on and the nodes they name, posed tick by
/// tick by the clip alone in the robot's own frame (`melee::attack_bodies_and_mounts`, what the
/// reference's hit test uses), and the explosions its `trigger` events set off with their node's
/// path (`melee::clip_blasts`: the ground punch's blasts). A frame is the place (x, y, z) and the
/// turn (x, y, z, w) at each whole tick from 0 to the clip's length.
fn attack_bodies_entry(entry: &mut Value, rig: &tf2_core::pose::Rig, robot: &tf2_core::formats::scene::ObjectDef, clip: &ClipOut, names: &Names) {
    use tf2_core::formats::scene::Shape;
    let frame = |a: &glam::Affine3A| {
        let (_, turn, place) = a.to_scale_rotation_translation();
        Value::Array([place.x, place.y, place.z, turn.x, turn.y, turn.z, turn.w].map(fine).to_vec())
    };
    let (bodies, mounts) = tf2_core::melee::attack_bodies_and_mounts(rig, robot, clip.clip, clip.looping);
    if !mounts.is_empty() {
        entry["attack_bodies"] = Value::Array(bodies.iter().map(|b| {
            let shape = match b.frames.first().map(|f| f.shape) {
                Some(Shape::Sphere { radius }) => json!({ "shape": "sphere", "radius": num(radius) }),
                Some(Shape::Cuboid { half }) => json!({ "shape": "box", "half_extents": nums(&half) }),
                Some(Shape::Cylinder { radius, half_height }) => json!({ "shape": "cylinder", "radius": num(radius), "half_height": num(half_height) }),
                None => Value::Null,
            };
            json!({ "node": names.name(b.node), "foot": b.foot, "body": shape, "frames": b.frames.iter().map(|f| frame(&f.at)).collect::<Vec<_>>() })
        }).collect());
        entry["attack_nodes"] = Value::Array(mounts.iter().map(|m| {
            json!({ "node": names.name(m.node), "frames": m.frames.iter().map(frame).collect::<Vec<_>>() })
        }).collect());
    }
    let blasts = tf2_core::melee::clip_blasts(rig, robot, clip.clip, clip.looping);
    if !blasts.is_empty() {
        entry["blasts"] = Value::Array(blasts.iter().map(|b| {
            let mut v = crate::mechanics::blast(b, names);
            v["centres"] = Value::Array(b.centres.iter().map(|c| Value::Array(c.to_array().map(fine).to_vec())).collect());
            v
        }).collect());
    }
}

fn attack_entry(a: &AttackRef) -> Value {
    let flags = |v: u32| format!("{v:#010x}");
    json!({
        "damage": num(a.damage),
        "multiplayer_damage": num(a.mp_damage),
        "ai_damage": num(a.ai_damage),
        "direction": a.attack_dir,
        "stop_on_hit": a.stop_on_hit,
        "knock_back_speed": num(a.knock_back_speed),
        "knock_back_angle_degrees": num(a.knock_back_angle),
        "slide_speed": num(a.slide_speed),
        "min_range": num(a.min_range),
        "max_range": num(a.max_range),
        "auto_target_turn_speed": num(a.auto_target_turn_speed),
        "turn_speed": num(a.turn_speed),
        "stick_choose_target_max_degrees": num(a.stick_choose_target_max_degrees),
        "auto_target_search_angle_degrees": num(a.auto_target_search_angle),
        "damage_flags": flags(a.include_damage),
        "damage_flags_small": flags(a.include_damage_small),
        "damage_flags_large": flags(a.include_damage_large),
        "damage_flags_excluded": flags(a.exclude_damage),
    })
}

/// The clip's events, each `{"tick", "type", ...}`, in tick order (events at one tick keep the
/// order of their kinds below). A foot or climbing step is in the clip's sound and effect lists
/// both (one event read twice): written once, with its node.
pub fn events(clip: &Clip, nodes: &HashSet<&str>, vehicle_file: Option<&str>, held: &dyn Fn(u32) -> Option<String>, names: &Names) -> Vec<Value> {
    let mut out: Vec<(f32, Value)> = Vec::new();
    let name = |h: u32| names.name(h);
    let opt = |h: u32| (h != 0).then(|| names.name(h));
    let mut push = |time: f32, kind: &str, fields: Value| {
        let mut event = Map::new();
        event.insert("tick".into(), num(time));
        event.insert("type".into(), json!(kind));
        if let Value::Object(fields) = fields {
            event.extend(fields);
        }
        // A node the clip names that its object lacks (a clip shared between weapons): said, not dropped.
        if event.get("node").and_then(Value::as_str).is_some_and(|node| !nodes.contains(node)) {
            event.insert("node_in_file".into(), json!(false));
        }
        out.push((time, Value::Object(event)));
    };
    for e in &clip.model_events {
        let animation = |id: &Option<String>| id.as_ref().map(|id| format!("{}.{id}", Actions::VEHICLE_SET));
        push(e.time, "model", json!({
            "show_robot": e.show_true,
            "show_vehicle": e.show_alternate,
            "vehicle_animation": animation(&e.alternate_clip),
            "vehicle_then": animation(&e.alternate_then),
            "vehicle_file": e.alternate_clip.as_ref().and(vehicle_file),
        }));
    }
    for e in &clip.sound_events {
        match e.cue {
            SoundCue::Play { id, auto_stop, held } => {
                push(e.time, "sound_play", json!({ "event": name(id), "auto_stop": auto_stop, "held": held, "rerun": e.rerun }))
            }
            SoundCue::Stop { id } => push(e.time, "sound_stop", json!({ "event": name(id) })),
            SoundCue::FootStep { .. } | SoundCue::ClimbStep { .. } => {}
        }
    }
    for e in &clip.effect_events {
        match &e.kind {
            EffectEventKind::Spawn { id, template, track, use_rotation, use_scale, auto_stop } => push(e.time, "effect_start", json!({
                "node": name(e.node),
                "effect": name(template.name),
                "id": opt(*id),
                "follows_node": track,
                "use_rotation": use_rotation,
                "use_scale": use_scale,
                "auto_stop": auto_stop,
                "rerun": e.rerun,
            })),
            EffectEventKind::Stop { id } => push(e.time, "effect_stop", json!({ "node": name(e.node), "id": opt(*id) })),
            EffectEventKind::FootStep { side } => push(e.time, "footstep", json!({ "node": name(e.node), "side": name(*side) })),
            // The five sides' names are not in the game's files: named by the limb they are for.
            EffectEventKind::ClimbStep { limb } => push(e.time, "climb_step", json!({ "node": name(e.node), "limb": LIMBS[*limb] })),
        }
    }
    for e in &clip.weapon_events {
        match e.kind {
            WeaponEventKind::Attach { weapon, index, only_if_current, hide, unhide, clip, hide_when_done, then } => {
                push(e.time, "weapon_attach", json!({
                    "node": name(e.node),
                    "weapon": opt(weapon),
                    "index": (weapon == 0).then_some(index),
                    "weapon_file": (weapon != 0).then(|| held(weapon)).flatten(),
                    "only_if_current": only_if_current,
                    "hide": hide,
                    "unhide": unhide,
                    "weapon_animation": opt(clip),
                    "weapon_then": opt(then),
                    "hide_when_done": hide_when_done,
                }))
            }
            WeaponEventKind::EnableFire(on) => push(e.time, "weapon_fire_enabled", json!({ "on": on })),
        }
    }
    for e in &clip.special_fire_events {
        push(e.time, "special_fire", json!({ "node": name(e.node) }));
    }
    for e in &clip.shake_events {
        push(e.time, "camera_shake", json!({ "shake": name(e.name), "positional": e.positional }));
    }
    for e in &clip.action_events {
        let (kind, fields) = match e.action {
            Action::BranchPoint => ("branch_point", json!({})),
            Action::ButtonWindow { open } => ("attack_button_window", json!({ "open": open })),
            Action::FaceAndSlide { slide, face, stick } => ("attack_face_and_slide", json!({ "slide": slide, "face": face, "stick": stick })),
            Action::Hit { on, node } => ("attack_hit", json!({ "on": on, "node": name(node) })),
            // The same event as a trigger message below, when it is a local one: written once.
            Action::Trigger { name: script } => {
                if clip.trigger_events.iter().any(|t| t.name == script && t.time == e.time && !t.on_exit) {
                    continue;
                }
                ("trigger", json!({ "name": name(script) }))
            }
            Action::Traction(v) => ("traction", json!({ "axes": nums(&v) })),
            Action::MotionMode(mode) => ("motion_mode", json!({ "mode": name(mode) })),
            Action::Dash(timer) => ("dash", json!({ "timer": num(timer) })),
            Action::ForceUp(up) => ("force_up", json!({ "up": nums(&up) })),
            Action::PartialAnim { set, id } => {
                ("partial_animation", json!({ "animation": format!("{}.{}", name(set), name(id)), "layer": 1 }))
            }
            Action::FinishMove { start, end, kill } => ("finish_move", json!({ "start_camera": start, "end_camera": end, "kill": kill })),
        };
        push(e.time, kind, fields);
    }
    for e in &clip.trigger_events {
        push(e.time, "trigger", json!({ "name": name(e.name), "message": name(e.message), "on_exit": e.on_exit }));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter().map(|(_, v)| v).collect()
}

/// Every material the files use, with its technique, blend and every parameter: a texture
/// parameter names its file under `textures/` (written here if the glTF did not need it: the
/// specular and reflection maps).
fn materials(shared: &mut Shared, names: &Names) -> Vec<Value> {
    let ids = shared.materials.clone();
    materials_of(&ids, shared, names)
}

/// The materials `ids` name, written as `materials` writes them.
pub fn materials_of(ids: &[u32], shared: &mut Shared, names: &Names) -> Vec<Value> {
    let library = shared.library;
    ids.iter()
        .filter_map(|id| library.materials.get(id))
        .map(|m| {
            let parameters: Vec<Value> = m
                .params
                .iter()
                .map(|p| {
                    let value = match &p.value {
                        ParamValue::Number(v) => num(*v),
                        ParamValue::Word(w) => names.find(*w).map_or_else(|| json!(w), Value::from),
                        ParamValue::Vector(v) => nums(v),
                        ParamValue::Texture(Some(t)) => shared.texture(*t).map_or_else(|| json!(names.name(*t)), |f| json!(f.path())),
                        ParamValue::Texture(None) => Value::Null,
                    };
                    let mut entry = json!({ "name": names.name(p.name), "value": value });
                    if p.owner != 0 {
                        entry["of"] = json!(names.name(p.owner));
                    }
                    entry
                })
                .collect();
            json!({
                "name": m.name,
                "technique": names.name(m.technique),
                "blend": blend_of(m).map(|b| names.name(b)),
                "parameters": parameters,
            })
        })
        .collect()
}

/// The numbers a mod needs most, as the port reads them (`tf2_core::tuning`): the attribute block
/// has them all, by the game's own names. The walk and run speeds are measured on the clips.
fn summary(install: &Path, spec: &Spec, data: &CharacterData) -> Value {
    let tuning = match tf2_core::tuning::load(install, &spec.attributes) {
        Ok(t) => t,
        Err(e) => return json!({ "error": e }),
    };
    let mut missing = Vec::new();
    let locomotion = Locomotion::from_animations(&data.animations, &mut missing);
    let jump = |j: &tf2_core::tuning::JumpKind| json!({ "height": num(j.height), "speed": num(j.speed) });
    json!({
        "health": num(tuning.base_health),
        "health_regen_rate": num(tuning.health_regen_rate),
        "health_regen_delay": num(tuning.health_regen_delay),
        "robot": { "height": num(tuning.robot_height), "radius": num(tuning.robot_radius), "mass": num(tuning.robot_mass) },
        "vehicle": { "height": num(tuning.vehicle_height), "radius": num(tuning.vehicle_radius), "mass": num(tuning.vehicle_mass) },
        "gravity": num(tuning.gravity),
        "walk_speed": num(locomotion.walk_speed),
        "run_speed": num(locomotion.run_speed),
        "strafe_speed": num(tuning.strafe_speed),
        "jumps": {
            "standing": jump(&tuning.jump.standing),
            "running": jump(&tuning.jump.run),
            "high_standing": jump(&tuning.jump.high_standing),
            "long": jump(&tuning.jump.long),
        },
        "drive": {
            "max_speed": num(tuning.drive.max_speed),
            "max_turbo_speed": num(tuning.drive.max_turbo_speed),
            "max_reverse_speed": num(tuning.drive.max_reverse_speed),
            "wheel_radius": num(tuning.drive.render_wheel_radius),
        },
        "units": "metres, seconds, metres per second",
    })
}

/// A data value as JSON, every struct field by the game's own name: numbers, enum and flag names,
/// text, arrays and nested objects (an object reached through a pointer carries its `_type` and
/// its `_name`). Deep chains are cut at 12 levels.
pub fn value(node: Node, names: &Names, depth: u32) -> Value {
    if depth > 12 {
        return json!("...");
    }
    if let Some(name) = node.enum_name() {
        return json!(name);
    }
    let flags = node.flag_names();
    if !flags.is_empty() {
        return json!(flags);
    }
    if let Some(text) = node.text() {
        return json!(text);
    }
    if let Some(i) = node.int() {
        return json!(i);
    }
    if let Some(f) = node.float() {
        return num(f);
    }
    let fields = node.fields();
    if !fields.is_empty() {
        let mut map = Map::new();
        if let (Some(kind), Some(label)) = (node.type_name(), node.label()) {
            map.insert("_type".into(), json!(kind));
            map.insert("_name".into(), json!(names.name(label)));
        }
        for (hash, field) in fields {
            let key = node.file().name(hash).map_or_else(|| names.name(hash), str::to_owned);
            map.insert(key, value(field, names, depth + 1));
        }
        return Value::Object(map);
    }
    if !node.is_empty() {
        return Value::Array(node.items().map(|item| value(item, names, depth + 1)).collect());
    }
    match node.label() {
        Some(label) => json!({ "_link": names.name(label) }),
        None => Value::Null,
    }
}

/// Devastator's `giant` block: his gun pods (each kind's weapon, the bones it is mounted on with
/// their barrels, its model and its rounds) and his vortex funnel, the boss mission's objects as
/// the Bevy game gives them to him. [assumed port design] The rules: `core/src/giant.rs`,
/// `docs/devastator.md`.
pub fn giant(pods: &[tf2_core::giant::Pods], vortex: Option<&tf2_core::giant::VortexDef>, pieces: &[Piece], names: &Names) -> Value {
    let file_of = |kind: PieceKind, weapon: u32| pieces.iter().find(|p| p.kind == kind && p.weapons.contains(&weapon)).map(|p| format!("{}.glb", p.file));
    let pods: Vec<Value> = pods
        .iter()
        .map(|p| {
            let def = &p.def;
            let row = |key: u32| p.effects.iter().find(|r| r.name == key).map(|r| crate::rounds::round(r, pieces, names));
            json!({
                "weapon": names.name(def.name),
                "kind": (["gatling", "missile", "bug_bomb"].get(p.kind).copied().unwrap_or("unknown")),
                "file": file_of(PieceKind::Pod, def.name),
                "mounts": p.mounts.iter().map(|m| json!({
                    "bone": names.name(m.bone),
                    "barrels": m.barrels.iter().map(|b| nums(&b.to_array())).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "fire_sound": (def.fire_sound != 0).then(|| names.name(def.fire_sound)),
                "round": row(def.name),
                "alt_round": row(tf2_core::character::alt_projectile_key(def.name)),
                "muzzle_light": p.flash.as_ref().map(crate::rounds::flash),
            })
        })
        .collect();
    json!({
        "pods": pods,
        "vortex": vortex.map(|v| json!({
            "file": pieces.iter().find(|p| p.kind == PieceKind::Vortex).map(|p| format!("{}.glb", p.file)),
            "clip": v.clip.as_ref().map(|_| "Vortex"),
            "damage": num(v.damage),
            "flags": v.flags,
            "knock_back_speed": num(v.knock_back_speed),
            "knock_back_angle_degrees": num(v.knock_back_angle),
            "sound": (v.sound != 0).then(|| names.name(v.sound)),
            "sound_delay": num(v.sound_delay),
        })),
    })
}
