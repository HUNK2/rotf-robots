//! Reference runs: a scripted pad played through the reference (`tf2-core`'s `driver`: the
//! movement, the camera, the aim, the weapons and the scouts that fight back, in the Bevy game's
//! order), one line of JSON per update with where the robot is, how fast, which control state,
//! which clip, what the camera sees, what he fired and hit, and what hit him. A native port plays
//! the same script and compares its own numbers with these (`docs/FORMAT.md`, "Reference runs").

use std::cell::RefCell;
use std::path::Path;

use glam::{Vec2, Vec3};
use serde_json::{Value, json};
use tf2_core::arsenal::WeaponSound;
use tf2_core::character::CharacterData;
use tf2_core::control::button;
use tf2_core::driver::{Pad, Robot, Update};
use tf2_core::formats::characterlist::Spec;
use tf2_core::melee::{self, Target};
use tf2_core::sim::{Arena, Box3};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet, Tuning};

use crate::names::{self, Names};

/// The game's update. [game]
pub const UPDATE_SECONDS: f32 = 0.032;

/// The version of the run scripts and run files (`docs/FORMAT.md`, "Reference runs"). 2 since
/// 2026-10-27: the stick is the pad's, relative to the camera, and the camera, the weapons and
/// the scouts run.
pub const RUN_VERSION: u32 = 2;

const JUMP: usize = button::JUMP as usize;

/// The robot's tuning and data as the Bevy game builds them (`game/src/charmenu.rs` in
/// rotf-robots): the attributes, the clips measured into it, the vehicle's wheelbase, and
/// Devastator's fitting.
pub fn load(install: &Path, spec: &Spec) -> Result<(Tuning, CharacterData), String> {
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
        // His vortex and his guns, the boss mission's (`giant::read_vortex`, `giant::read_pods`),
        // as the Bevy game gives them. [assumed port design]
        let vortex = tf2_core::giant::read_vortex(install);
        let pods = tf2_core::giant::read_pods(install);
        if let Some(g) = t.giant.as_mut() {
            g.vortex = vortex.and_then(|def| tf2_core::giant::vortex(&def, &data.library, &data.animations, &data.robot));
            g.pods = pods;
            data.weapon_effects.extend(g.pods.iter().flat_map(|p| p.effects.iter().cloned()));
        }
    }
    Ok((t, data))
}

/// The robot's tuning (`load`).
pub fn tuning(install: &Path, spec: &Spec) -> Result<Tuning, String> {
    load(install, spec).map(|(t, _)| t)
}

/// The weapons' sound calls of one update, by event name, in the order they were made.
struct Recorder<'a> {
    names: &'a Names,
    calls: RefCell<Vec<Value>>,
    next: RefCell<u64>,
}

impl Recorder<'_> {
    fn push(&self, value: Value) {
        self.calls.borrow_mut().push(value);
    }
}

impl WeaponSound for Recorder<'_> {
    fn play_event(&self, id: u32) {
        self.push(json!({"play": self.names.name(id)}));
    }
    fn play_event_at(&self, id: u32, point: Vec3) {
        self.push(json!({"play": self.names.name(id), "at": v3(point)}));
    }
    fn start_kept(&self, id: u32, heat: Option<f32>) -> Option<tf2_core::sound::Handle> {
        let mut next = self.next.borrow_mut();
        *next += 1;
        self.push(json!({"start": self.names.name(id), "handle": *next, "heat": heat.map(num)}));
        Some(tf2_core::sound::Handle::from_number(*next))
    }
    fn is_infinite(&self, _id: u32) -> bool {
        // [port design] A run has no sound banks: a kept sound is taken to loop until it is
        // stopped, so every automatic weapon's fire sound is kept and let go (the banks' own
        // answer is in the robot's `sounds`).
        true
    }
    fn key_off(&self, handle: tf2_core::sound::Handle) {
        self.push(json!({"key_off": handle.number()}));
    }
    fn stop(&self, handle: tf2_core::sound::Handle) {
        self.push(json!({"stop": handle.number()}));
    }
    fn projectile_start(&self, number: u32, weapon: u32, point: Vec3) {
        self.push(json!({"projectile_start": number, "weapon": self.names.name(weapon), "at": v3(point)}));
    }
    fn projectile_start_sounds(&self, number: u32, ids: &[u32], point: Vec3) {
        self.push(json!({"projectile_start": number, "events": ids.iter().map(|&id| self.names.name(id)).collect::<Vec<_>>(), "at": v3(point)}));
    }
    fn projectile_move(&self, _number: u32, _point: Vec3, _speed: f32) {}
    fn projectile_stop(&self, number: u32) {
        self.push(json!({"projectile_stop": number}));
    }
    fn projectile_end(&self, number: u32, weapon: u32, point: Vec3) {
        self.push(json!({"projectile_end": number, "weapon": self.names.name(weapon), "at": v3(point)}));
    }
    fn weapon_impact(&self, weapon: u32, surface: u32, point: Vec3) {
        self.push(json!({"impact": self.names.name(weapon), "surface": self.names.name(surface), "at": v3(point)}));
    }
    fn combat_target(&self, _present: bool) {}
    fn weapon_clip_command(&self, command: tf2_core::gun::SoundCommand) {
        self.push(json!({"clip_command": format!("{command:?}")}));
    }
    fn weapon_object_hidden(&self) {}
}

/// Plays `script` (the JSON of `docs/FORMAT.md`, "Reference runs") for the robot `spec` names:
/// the header line, then one line per update.
pub fn run(install: &Path, spec: &Spec, script: &Value) -> Result<Vec<Value>, String> {
    let (t, data) = load(install, spec)?;
    let mut files = names::data_files(&spec.pack_file(install));
    files.extend(names::data_files(&install.join("bnxglobal.str")));
    let names = Names::load(install, files, &[]);
    let pads = pads(script)?;
    let (mut arena, scouts, passive) = arena(script.get("arena"), &t)?;

    let (mut pos, mut yaw) = (Vec3::ZERO, 0.0);
    if let Some(start) = script.get("start") {
        if let Some(at) = start.get("pos") {
            pos = vec3(at).ok_or("start.pos: three numbers")?;
        }
        if let Some(degrees) = start.get("yaw_degrees") {
            yaw = degrees.as_f64().ok_or("start.yaw_degrees: a number")?.to_radians() as f32;
        }
    }
    let arsenal = tf2_core::arsenal::arm(install, &spec.attributes, &data, t.giant.as_ref());
    let armed = arsenal.is_some();
    let mut robot = Robot::new(&t, &data, arsenal, pos, yaw);
    robot.passive = passive;
    // The scouts are the game's Decepticon scout, or its Autobot scout for a Decepticon: the game
    // lets no hit pass between two of the same side. [port design, as the Bevy game's arena]
    let decepticon = tuning::load(install, "DComScout").map(|s| s.damage.team).ok() == Some(t.damage.team);
    let (scout_pack, scout_attributes) = if decepticon { ("aprotscoutmp", "APrtScout") } else { ("dcomscout", "DComScout") };
    let first = arena.targets.len() as u32 + 1;
    for (n, at) in scouts.iter().enumerate() {
        robot.add_scout(|| tf2_core::combat::Scout::load_as(install, scout_pack, scout_attributes), *at, first + n as u32)?;
    }
    let mut out = vec![json!({
        "format_version": RUN_VERSION,
        "character": spec.display.clone().unwrap_or_else(|| spec.attributes.clone()),
        "pack": spec.pack,
        "attributes": spec.attributes,
        "update_seconds": num(UPDATE_SECONDS),
        "updates": pads.len(),
        "armed": armed,
        "scout": scout_attributes,
        "scouts": robot.scouts.iter().map(|e| json!({"target": e.state.actor.id, "pos": v3(e.state.pos), "health": num(e.state.health)})).collect::<Vec<_>>(),
        "missing": t.missing,
    })];
    for (i, pad) in pads.iter().enumerate() {
        let sound = Recorder { names: &names, calls: RefCell::new(Vec::new()), next: RefCell::new(0) };
        let update = robot.step(pad, &mut arena, Some(&sound), UPDATE_SECONDS);
        out.push(line(i, &robot, &update, &names, sound.calls.into_inner()));
    }
    Ok(out)
}

/// What the robot is after update `i`.
fn line(i: usize, robot: &Robot, update: &Update, names: &Names, sounds: Vec<Value>) -> Value {
    let (s, t) = (&robot.state, robot.t);
    let clip = match s.action_clip(t) {
        Some((set, id, tick)) => json!({"layer": "action", "set": set, "id": id, "tick": num(tick)}),
        None => s.base_clip(t).map_or(Value::Null, |c| json!({"layer": "base", "set": c.set, "id": c.id, "tick": num(c.tick), "blend": num(c.blend)})),
    };
    let named = |hashes: &[u32]| hashes.iter().map(|&h| names.name(h)).collect::<Vec<_>>();
    let view = update.view;
    let weapon = robot.arsenal.as_ref().map_or(Value::Null, |arsenal| {
        let (name, heat, overheated) = arsenal.in_hand_state().unwrap_or_default();
        json!({
            "in_hand": names.name(name),
            "out": arsenal.gun.out,
            "heat": num(heat),
            "overheated_seconds": num(overheated),
            "fired": update.fired.is_some(),
            "muzzle": v3(update.muzzle),
            "lock": {"candidate": arsenal.lock.candidate, "locked": arsenal.lock.locked},
            "projectiles": arsenal.projectiles().map(|(number, _, weapon, grenade)| json!({
                "number": number,
                "weapon": names.name(weapon),
                "grenade": grenade,
                "pos": arsenal.projectile_frame(number).map_or(Value::Null, |f| v3(f.translation.into())),
            })).collect::<Vec<_>>(),
            "struck": update.struck.iter().map(|(target, p)| json!({"target": target, "damage": num(p.amount), "flags": p.flags})).collect::<Vec<_>>(),
            "sounds": sounds,
        })
    });
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
        "camera": {"eye": v3(view.eye), "forward": v3(view.forward), "fov_degrees": num(view.fov), "roll_degrees": num(view.roll.to_degrees())},
        "aim": update.aim.map_or(Value::Null, v3),
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
        "weapon": weapon,
        "taken": update.taken.iter().map(|(from, p, damage)| json!({"from": from, "damage": num(*damage), "flags": p.flags})).collect::<Vec<_>>(),
        "scouts": robot.scouts.iter().map(|e| json!({
            "target": e.state.actor.id,
            "pos": v3(e.state.pos),
            "health": num(e.state.health),
            "dead": e.state.dead(),
        })).collect::<Vec<_>>(),
    })
}

/// The script's steps as one pad per update.
fn pads(script: &Value) -> Result<Vec<Pad>, String> {
    let version = script.get("format_version").and_then(Value::as_u64).ok_or("format_version is missing")?;
    if version != u64::from(RUN_VERSION) {
        return Err(format!("format_version {version}: this exporter reads run scripts of version {RUN_VERSION}"));
    }
    let steps = script.get("steps").and_then(Value::as_array).ok_or("steps: a list")?;
    let mut out = Vec::new();
    for (n, step) in steps.iter().enumerate() {
        let fail = |what: &str| format!("steps[{n}]: {what}");
        let updates = step.get("updates").map_or(Some(1), Value::as_u64).ok_or_else(|| fail("updates: a whole number"))?;
        let pair = |key: &str| step.get(key).map(|v| vec2(v).ok_or_else(|| fail(&format!("{key}: two numbers")))).transpose();
        let number = |key: &str| step.get(key).map(|v| v.as_f64().map(|f| f as f32).ok_or_else(|| fail(&format!("{key}: a number")))).transpose();
        let flag = |key: &str| step.get(key).map(|v| v.as_bool().ok_or_else(|| fail(&format!("{key}: true or false")))).transpose();
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
        let base = Pad {
            stick: pair("stick")?.unwrap_or(Vec2::ZERO),
            look: pair("look")?.unwrap_or(Vec2::ZERO),
            vehicle: number("vehicle")?.unwrap_or(0.0),
            aim: flag("weapon_mode")?.unwrap_or(false),
            brake: number("brake")?.unwrap_or(0.0),
            switch: flag("switch")?.unwrap_or(false),
            ..Default::default()
        };
        let (held, pressed) = (buttons("hold")?, buttons("press")?);
        for k in 0..updates {
            let mut pad = base;
            for &b in held.iter().chain(&pressed) {
                match b {
                    JUMP => pad.jump_held = true,
                    _ => pad.buttons_down |= 1 << b,
                }
            }
            if k == 0 {
                for &b in &pressed {
                    match b {
                        JUMP => pad.jump = true,
                        _ => pad.buttons_hit |= 1 << b,
                    }
                }
            }
            out.push(pad);
        }
    }
    Ok(out)
}

/// The flat ground with the script's boxes and dummies on it, where the scouts stand, and
/// whether they hold their fire.
fn arena(arena: Option<&Value>, t: &Tuning) -> Result<(Arena, Vec<Vec3>, bool), String> {
    let mut out = Arena::default();
    let Some(arena) = arena else { return Ok((out, Vec::new(), false)) };
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
    let mut scouts = Vec::new();
    for (n, scout) in arena.get("scouts").and_then(Value::as_array).into_iter().flatten().enumerate() {
        scouts.push(scout.get("pos").and_then(vec3).ok_or_else(|| format!("arena.scouts[{n}].pos: three numbers"))?);
    }
    let passive = arena.get("passive_scouts").and_then(Value::as_bool).unwrap_or(false);
    Ok((out, scouts, passive))
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
pub(crate) fn num(v: f32) -> Value {
    let v = if v == 0.0 { 0.0 } else { v };
    if v.is_finite() { serde_json::from_str(&v.to_string()).unwrap_or(Value::Null) } else { Value::Null }
}

fn v3(v: Vec3) -> Value {
    json!([num(v.x), num(v.y), num(v.z)])
}
