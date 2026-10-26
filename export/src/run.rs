//! Reference runs: a scripted input played through the reference (`tf2-core`'s `sim::step`), one
//! line of JSON per update with where the robot is, how fast, which control state, which clip and
//! what it set off. A native port plays the same script and compares its own numbers with these
//! (`docs/FORMAT.md`, "Reference runs").

use std::path::Path;

use glam::{Vec2, Vec3};
use serde_json::{Value, json};
use tf2_core::control::button;
use tf2_core::formats::characterlist::Spec;
use tf2_core::melee::{self, Target};
use tf2_core::sim::{self, Arena, Box3, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet, Tuning};

use crate::names::{self, Names};

/// The game's update. [game]
pub const UPDATE_SECONDS: f32 = 0.032;

/// The two buttons the reference takes as their own fields rather than as bits: jump (`jump`,
/// `jump_held`) and turbo (`turbo`, pressed only), as the Bevy game hands them in.
const JUMP: usize = button::JUMP as usize;
const TURBO: usize = 0x16;

/// The robot's tuning as the Bevy game builds it (`game/src/charmenu.rs` in rotf-robots): the
/// attributes, the clips measured into it, the vehicle's wheelbase, and Devastator's fitting.
pub fn tuning(install: &Path, spec: &Spec) -> Result<Tuning, String> {
    let mut t = tuning::load(install, &spec.attributes)?;
    let mut data = tf2_core::character::load(install, &spec.pack)?;
    if data.name_id != 0 {
        t.name_hash = data.name_id;
    }
    let giant = tf2_core::giant::is_giant(&data);
    if giant {
        tf2_core::giant::reroot(&mut data.animations, &data.robot);
    }
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing)
        .with_vehicle(data.vehicle.as_ref())
        .with_melee(&data.melee_weapons);
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    if let Some(vehicle) = &data.vehicle {
        let along = |name: &str| vehicle.nodes.iter().find(|n| n.name == name).map(|n| n.bind[3][1]);
        if let (Some(front), Some(rear)) = (along("Tire_Front_L"), along("Tire_Rear_L")) {
            t.wheelbase = (front - rear).abs();
        }
    }
    if giant {
        tf2_core::giant::fit(&mut t, &data.animations, &data.robot, &data.shockwaves);
    }
    Ok(t)
}

/// Plays `script` (the JSON of `docs/FORMAT.md`, "Reference runs") for the robot `spec` names:
/// the header line, then one line per update.
pub fn run(install: &Path, spec: &Spec, script: &Value) -> Result<Vec<Value>, String> {
    let t = tuning(install, spec)?;
    let mut files = names::data_files(&spec.pack_file(install));
    files.extend(names::data_files(&install.join("bnxglobal.str")));
    let names = Names::load(install, files, &[]);
    let inputs = inputs(script)?;
    let arena = arena(script.get("arena"), &t)?;

    let mut s = State::new(&t);
    if let Some(start) = script.get("start") {
        if let Some(pos) = start.get("pos") {
            s.pos = vec3(pos).ok_or("start.pos: three numbers")?;
        }
        if let Some(yaw) = start.get("yaw_degrees") {
            s.yaw = yaw.as_f64().ok_or("start.yaw_degrees: a number")?.to_radians() as f32;
        }
    }
    let mut out = vec![json!({
        "format_version": crate::FORMAT_VERSION,
        "character": spec.display.clone().unwrap_or_else(|| spec.attributes.clone()),
        "pack": spec.pack,
        "attributes": spec.attributes,
        "update_seconds": num(UPDATE_SECONDS),
        "updates": inputs.len(),
        "missing": t.missing,
    })];
    for (i, input) in inputs.iter().enumerate() {
        sim::step(&mut s, input, &t, &arena, UPDATE_SECONDS);
        out.push(update(i, &s, &t, &names));
    }
    Ok(out)
}

/// What the robot is after update `i`.
fn update(i: usize, s: &State, t: &Tuning, names: &Names) -> Value {
    let clip = match s.action_clip(t) {
        Some((set, id, tick)) => json!({"layer": "action", "set": set, "id": id, "tick": num(tick)}),
        None => s.base_clip(t).map_or(Value::Null, |c| json!({"layer": "base", "set": c.set, "id": c.id, "tick": num(c.tick), "blend": num(c.blend)})),
    };
    let named = |hashes: &[u32]| hashes.iter().map(|&h| names.name(h)).collect::<Vec<_>>();
    json!({
        "update": i,
        "time": num((i + 1) as f32 * UPDATE_SECONDS),
        "pos": v3(s.pos),
        "vel": v3(s.vel),
        "yaw_degrees": num(s.yaw.to_degrees()),
        "pitch_degrees": num(s.pitch.to_degrees()),
        "roll_degrees": num(s.roll.to_degrees()),
        "speed": num(s.speed),
        "mode": format!("{:?}", s.mode),
        "gait": format!("{:?}", s.gait),
        "vehicle": s.is_vehicle(),
        "health": num(s.health),
        "state": s.control_state(t),
        "overlay": s.overlay_state(t),
        "clip": clip,
        "triggers": named(&s.triggers),
        "untriggers": named(&s.untriggers),
        "shakes": s.shakes.iter().map(|shake| names.name(shake.name)).collect::<Vec<_>>(),
        "rumbles": s.rumbles.iter().map(|r| json!({"kind": names.name(r.kind), "duration": num(r.duration), "strength": num(r.strength)})).collect::<Vec<_>>(),
        "hits": s.hits.iter().map(|h| json!({
            "target": h.target,
            "damage": num(h.damage),
            "point": v3(h.point),
            "body": names.name(h.body),
            "knock_back_speed": num(h.knock_back_speed),
            "knock_back_angle_degrees": num(h.knock_back_angle.to_degrees()),
            "flags": h.flags,
        })).collect::<Vec<_>>(),
    })
}

/// The script's steps as one input per update.
fn inputs(script: &Value) -> Result<Vec<Input>, String> {
    let version = script.get("format_version").and_then(Value::as_u64).ok_or("format_version is missing")?;
    if version != u64::from(crate::FORMAT_VERSION) {
        return Err(format!("format_version {version}: this exporter reads {}", crate::FORMAT_VERSION));
    }
    let steps = script.get("steps").and_then(Value::as_array).ok_or("steps: a list")?;
    let mut out = Vec::new();
    for (n, step) in steps.iter().enumerate() {
        let fail = |what: &str| format!("steps[{n}]: {what}");
        let updates = step.get("updates").map_or(Some(1), Value::as_u64).ok_or_else(|| fail("updates: a whole number"))?;
        let pair = |key: &str| step.get(key).map(|v| vec2(v).ok_or_else(|| fail(&format!("{key}: two numbers")))).transpose();
        let number = |key: &str| step.get(key).map(|v| v.as_f64().map(|f| f as f32).ok_or_else(|| fail(&format!("{key}: a number")))).transpose();
        let buttons = |key: &str| -> Result<Vec<usize>, String> {
            let Some(list) = step.get(key) else { return Ok(Vec::new()) };
            let list = list.as_array().ok_or_else(|| fail(&format!("{key}: a list of button names")))?;
            list.iter()
                .map(|b| {
                    let name = b.as_str().unwrap_or_default();
                    button::NAMES.iter().position(|&known| known == name).ok_or_else(|| fail(&format!("{key}: no button {b}")))
                })
                .collect()
        };
        let stick = pair("stick")?.unwrap_or(Vec2::ZERO);
        let base = Input {
            stick,
            steer: number("steer")?.unwrap_or(stick.x),
            pad_stick: pair("pad_stick")?.unwrap_or(stick),
            look: pair("look")?.unwrap_or(Vec2::ZERO),
            vehicle: number("vehicle")?.unwrap_or(0.0),
            brake: number("brake")?.unwrap_or(0.0),
            drift: step.get("drift").and_then(Value::as_bool).unwrap_or(false),
            aim: pair("aim")?,
            ..Default::default()
        };
        let (held, pressed) = (buttons("hold")?, buttons("press")?);
        for k in 0..updates {
            let mut input = base;
            for &b in held.iter().chain(&pressed) {
                match b {
                    JUMP => input.jump_held = true,
                    _ => input.buttons_down |= 1 << b,
                }
            }
            if k == 0 {
                for &b in &pressed {
                    match b {
                        JUMP => input.jump = true,
                        TURBO => input.turbo = true,
                        _ => input.buttons_hit |= 1 << b,
                    }
                }
            }
            out.push(input);
        }
    }
    Ok(out)
}

/// The flat ground, with the script's boxes and targets on it.
fn arena(arena: Option<&Value>, t: &Tuning) -> Result<Arena, String> {
    let mut out = Arena::default();
    let Some(arena) = arena else { return Ok(out) };
    for (n, b) in arena.get("boxes").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let (Some(min), Some(max)) = (b.get("min").and_then(vec3), b.get("max").and_then(vec3)) else {
            return Err(format!("arena.boxes[{n}]: min and max, three numbers each"));
        };
        out.boxes.push(Box3 { min, max });
    }
    for (n, target) in arena.get("targets").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let pos = target.get("pos").and_then(vec3).ok_or_else(|| format!("arena.targets[{n}].pos: three numbers"))?;
        out.targets.push(Target {
            id: n as u32 + 1,
            pos,
            radius: t.robot_radius,
            height: t.robot_height,
            character: true,
            large: false,
            surface: melee::surface::UPPER_BODY,
            dashing: false,
            health: f32::MAX,
            head: None,
        });
    }
    Ok(out)
}

fn vec2(v: &Value) -> Option<Vec2> {
    match v.as_array()?.as_slice() {
        [x, y] => Some(Vec2::new(x.as_f64()? as f32, y.as_f64()? as f32)),
        _ => None,
    }
}

fn vec3(v: &Value) -> Option<Vec3> {
    match v.as_array()?.as_slice() {
        [x, y, z] => Some(Vec3::new(x.as_f64()? as f32, y.as_f64()? as f32, z.as_f64()? as f32)),
        _ => None,
    }
}

/// A float as its shortest decimal, so 0.1 is written 0.1 and not 0.10000000149, and -0 as 0;
/// null if it is not finite.
fn num(v: f32) -> Value {
    let v = if v == 0.0 { 0.0 } else { v };
    if v.is_finite() { serde_json::from_str(&v.to_string()).unwrap_or(Value::Null) } else { Value::Null }
}

fn v3(v: Vec3) -> Value {
    json!([num(v.x), num(v.y), num(v.z)])
}
