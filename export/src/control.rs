//! The character's control state machine as `<pack>.json`'s `control`: every state with its clip
//! and the transitions out of it (every machine's, as the game joins them), and every connection
//! with what it tests. Read as the port reads it (`tf2_core::control::Graph`); the names the graph
//! keeps as numbers (the buttons, the messages, the environment tests, the states' `canGoToStates`
//! and motion modes) are taken from the data objects themselves.

use serde_json::{Map, Value, json};
use tf2_core::character::CharacterData;
use tf2_core::control::{Connection, Graph, StateDef, Test, button, env};
use tf2_core::formats::hash::crc32;
use tf2_core::formats::lxb::Node;
use tf2_core::tuning::{Actions, Tuning};

use crate::describe::num;
use crate::names::Names;

/// The state the machine falls back to when a finished state has nowhere to go: the character's
/// setup binds it to `stateIdle`. [game 00716d8b..00716d9b]
const START_STATE: &str = "stateIdle";

pub fn control(tuning: &Tuning, data: &CharacterData, names: &Names) -> Value {
    let graph = &tuning.control;
    let mut missing = Vec::new();
    let actions = Actions::from_animations(&data.animations, &data.robot, &mut missing);
    json!({
        "update_seconds": 0.032,
        "start_state": graph.find(START_STATE).map(|_| START_STATE),
        "buttons": button::NAMES.iter().enumerate().map(|(n, name)| json!({ "number": n, "name": name })).collect::<Vec<_>>(),
        "states": graph.states.iter().map(|s| state(graph, s, &actions, data, names)).collect::<Vec<_>>(),
        "connections": graph.connections.iter().map(|c| connection(graph, c, names)).collect::<Vec<_>>(),
    })
}

/// The data object a state or connection was read from, by its name.
fn object<'a>(names: &'a Names, name: &str) -> Option<Node<'a>> {
    let hash = name.strip_prefix('#').and_then(|h| u32::from_str_radix(h, 16).ok()).unwrap_or_else(|| crc32(name.as_bytes()));
    names.files().iter().find_map(|f| f.root().field(hash))
}

/// The motion modes by the class id a state's `motionModeType` stores (`docs/bumblebee-mechanics.md`,
/// "Modes"). [game 00851a40] The first has no name in the executable.
const MOTION_MODES: [(u32, &str); 16] = [
    (0x1ad6_ae68, "FirstMode"),
    (0xd706_8c31, "ClimbMode"),
    (0x517c_0a3b, "DashMode"),
    (0xe2b2_2eb4, "DriveMode"),
    (0xf081_d468, "DriveCompanionMode"),
    (0xfb9f_3262, "FlyMode"),
    (0xc588_2131, "GrabMode"),
    (0x7077_6cef, "HoverMode"),
    (0xbbc9_4a36, "InteractMode"),
    (0x6da1_d12d, "JumpMode"),
    (0x1839_220c, "KnockBackMode"),
    (0x252c_e043, "MoveMode"),
    (0x3c95_6e02, "StrafeMode"),
    (0xe639_2241, "SyncMode"),
    (0x4a15_f3ab, "ThrownMode"),
    (0xcd2f_4b0c, "AttackMode"),
];

fn button_name(number: u32) -> Value {
    button::NAMES.get(number as usize).map_or_else(|| json!(format!("#button{number:02x}")), |name| json!(name))
}

/// The set a state's clip plays from: the state's own `animSetName`; the fly states' `FlySet`
/// and the drive states' `DriveSet` whatever the data names; else the set the clip's name was
/// found in (as `sim::action_rule_clip`). [game 0084ddb0, 0087c320, 0087c7e0, 0087ba10]
fn clip_of(def: &StateDef, actions: &Actions, data: &CharacterData) -> Option<String> {
    if def.anim == 0 {
        return None;
    }
    let by_class = match def.class.as_str() {
        "StateFly" | "StateFlyExit" => Some("FlySet"),
        "StateDrive" => Some("DriveSet"),
        _ => None,
    };
    let found = actions.clips.get(&def.anim).map(|c| c.set.as_str());
    [def.anim_set.as_deref(), by_class, found].into_iter().flatten().find_map(|set| {
        let refs = data.animations.sets.get(set)?;
        let r = refs.iter().find(|r| r.clip.is_some() && crc32(r.id.as_bytes()) == def.anim)?;
        Some(format!("{set}.{}", r.id))
    })
}

fn state(graph: &Graph, def: &StateDef, actions: &Actions, data: &CharacterData, names: &Names) -> Value {
    let node = object(names, &def.name);
    let field = |name: &str| node.and_then(|n| n.get(name));
    let form = match def.robot_form {
        1 => json!("robot"),
        2 => json!("weapon"),
        3 => json!("vehicle"),
        5 => json!("unchanged"),
        n => json!(n),
    };
    let transitions: Vec<Value> = def
        .transitions
        .iter()
        .map(|t| {
            json!({
                "connection": graph.connections[t.connection].name,
                "to": t.output.map(|o| graph.states[o].name.clone()),
                "priority": t.priority,
            })
        })
        .collect();
    json!({
        "name": def.name,
        "class": def.class,
        "overlay": def.overlay,
        "clip": clip_of(def, actions, data),
        "clip_id": (def.anim != 0).then(|| names.name(def.anim)),
        "anim_set": def.anim_set,
        "robot_form": form,
        "finish_anim": def.finish_anim,
        "play_anim_once": def.play_anim_once,
        "finish_on_set_exit": def.finish_on_set_exit,
        "finish_on_special_set_exit": def.finish_on_special_set_exit,
        "allow_fall": def.allow_fall,
        "ranged": def.ranged,
        "fire_button": def.ranged.then(|| button_name(def.fire_button)),
        "can_go_to": field("canGoToStates").map_or_else(Vec::new, |n| n.flag_names()),
        "motion_mode": (def.motion_mode != 0).then(|| {
            MOTION_MODES.iter().find(|(id, _)| *id == def.motion_mode).map_or_else(|| names.name(def.motion_mode), |(_, name)| (*name).to_owned())
        }),
        "transitions": transitions,
    })
}

fn connection(graph: &Graph, c: &Connection, names: &Names) -> Value {
    let node = object(names, &c.name);
    let mut entry = Map::new();
    entry.insert("name".into(), json!(c.name));
    let (kind, fields) = match &c.test {
        Test::None => ("none", json!({})),
        Test::Detect(d) => {
            let stages: Vec<Value> = d
                .stages
                .iter()
                .map(|s| {
                    let l = &s.left;
                    let any = l.amp_min <= 0.0 && l.amp_max >= 100.0 && l.angle_min <= -std::f32::consts::PI && l.angle_max >= std::f32::consts::PI;
                    json!({
                        "after_seconds": num(s.time),
                        "held_seconds": num(s.charge),
                        "held": s.include.iter().map(|&b| button_name(b)).collect::<Vec<_>>(),
                        "not_held": s.exclude.iter().map(|&b| button_name(b)).collect::<Vec<_>>(),
                        "pressed": s.hit.map(button_name),
                        "stick": (!any).then(|| json!({
                            "min": num(l.amp_min),
                            "max": num(l.amp_max),
                            "angle_min_degrees": num(l.angle_min.to_degrees()),
                            "angle_max_degrees": num(l.angle_max.to_degrees()),
                            "against": if l.camera_relative { "camera" } else { "character" },
                        })),
                    })
                })
                .collect();
            ("detector", json!({ "stages": stages, "stays_on_seconds": num(d.stays_on), "hold_with_last_stage": d.hold_with_last_stage }))
        }
        Test::DistToGround { limit, greater } => ("ground_distance", json!({ "limit": num(*limit), "above": greater })),
        Test::TimeInState(seconds) => ("time_in_state", json!({ "seconds": num(*seconds) })),
        Test::Environment(test) => {
            let known = [
                (env::CAN_SWITCH_AVATAR, "CanSwitchAvatar"),
                (env::CLIMBABLE, "Climbable"),
                (env::NOT_HOLD_VIP, "NotHoldVIP"),
                (env::INSIDE_FLIGHT_BOUNDARY, "InsideFlightBoundary"),
            ];
            let name = node
                .and_then(|n| n.get("envTest"))
                .and_then(|n| n.enum_name().map(str::to_owned))
                .or_else(|| names.find(*test))
                .or_else(|| known.iter().find(|(h, _)| h == test).map(|(_, n)| (*n).to_owned()))
                .unwrap_or_else(|| format!("#{test:08x}"));
            ("environment", json!({ "environment": name }))
        }
        Test::Message(message) => {
            // An engine message by its enum's name; a message of the data's own (`msg` 0) by the
            // name of its hash in `msgUserDefined`. [data]
            let engine = node.and_then(|n| n.get("msg")).filter(|m| m.int().unwrap_or(0) != 0);
            let name = engine.and_then(|m| m.enum_name().map(str::to_owned)).unwrap_or_else(|| names.name(*message));
            ("message", json!({ "message": name }))
        }
        Test::All(members) => ("all", json!({ "connections": members.iter().map(|&m| graph.connections[m].name.clone()).collect::<Vec<_>>() })),
    };
    entry.insert("test".into(), json!(kind));
    if let Value::Object(fields) = fields {
        entry.extend(fields);
    }
    if !matches!(c.test, Test::All(_)) {
        let limit = |v: f32| if v == f32::MIN || v == f32::MAX { Value::Null } else { num(v) };
        entry.insert(
            "checks".into(),
            json!({
                "in_air": c.in_air,
                "on_ground": c.on_ground,
                "holding": c.holding,
                "not_holding": c.not_holding,
                "min_z_velocity": limit(c.min_z_velocity),
                "max_z_velocity": limit(c.max_z_velocity),
                "branch_begin": c.branch_begin,
                "branch_end": c.branch_end,
                "attack_can_branch": c.attack_can_branch,
                "can_adv_transform": c.can_adv_transform,
            }),
        );
    }
    Value::Object(entry)
}
