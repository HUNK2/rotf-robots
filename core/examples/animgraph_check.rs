//! Which parts of each robot's animation graph the port reaches. Two halves:
//!
//! STATIC (the data alone): every exit and entry target (set, clip) of every set resolves to a clip
//! that exists and has its animation file; every control state's `animName` is in the set it names;
//! every `VehicleModel` clip (the car or jet model's own clips) is named by some robot clip's
//! `AlternateModelEvent` (or by the special's `specialAbilityVehicleAnim`), and every name such an
//! event gives exists.
//!
//! DYNAMIC (a scripted headless session per robot through `sim::step`): idle, locomotion, jumps,
//! falls, dodges, melee, the ground punch, weapon mode with strafing and firing, climbing, the car
//! (or flight, hover and stunts for the flyers), the special, hits, stun and every kind of death.
//! Every (set, clip) that plays on the base layer, the control-state layer, the weapon set, the hit
//! overlay (layer 2) or the weapon-arm layer (`WeaponPartialSet`) is recorded. The arm layer is
//! driven here by a small stand-in for the weapon manager (`tf2_core::gun::Gun`; the game's is in
//! `game\src\weapons.rs`): it takes aim when an aim is held, fires every 0.25 s while the firing
//! state runs, overheats after 24 rounds, switches weapon once. That layer's coverage therefore
//! says what the data and `Gun` reach under that script, not what the game's manager does.
//!
//! `cargo run -p tf2-core --example animgraph_check -- [pack ...] [--why] [--reached]
//! [--static-only] [--log <segment>]` (no pack: all 13; a pack is `bumblebee` or
//! `bumblebee:Bumblebee`). `--why` says, for each unreached clip, which control state names it
//! (and whether the script entered it) and which entry or exit rules target it, with their
//! conditions; `--reached` lists, per set, the clips reached and the segment that first reached
//! each; `--log <segment>` prints each change of control state and clip while a segment (a
//! substring of its name: idle, locomotion, jumps, falls, dodges, melee, ground punch, weapon
//! mode, climb, special, hits, wall splat, car, fly, hover, fly exits, fly hits, fly deaths,
//! deaths) runs.
//!
//! The script is the checker's own guess at inputs: a clip it does not reach may be one the port
//! cannot reach, or one this script does not ask for. `--why` and `--log` are for telling which.
//!
//! The static half is also run by `core\tests\roster_animgraph.rs`, which includes this file.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use glam::{Vec2, Vec3};
use tf2_core::animrules::EMPTY_SET;
use tf2_core::character::CharacterData;
use tf2_core::control::button;
use tf2_core::damage::{Packet, flags};
use tf2_core::formats::hash::crc32;
use tf2_core::gun::{Clips, Gun, Hand};
use tf2_core::melee::Target;
use tf2_core::sim::{self, Arena, Box3, Input, Mode, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet, Tuning};
use tf2_core::weapons::WeaponDef;

pub const ROSTER: [&str; 13] = [
    "bumblebee:Bumblebee", "optimus:Optimus", "ironhide:Ironhide", "ratchet:Ratchet", "megatron:Megatron",
    "longhaul:LongHaul", "audir8:Audi", "jetfire:Jetfire", "firestorm:Firestorm", "starscream:Starscream",
    "blackout:Blackout", "optimusflight:OptimusFlight", "megatronflight:MegatronFlight",
];

/// Sets the robots' own mechanics do not need (the task's scope): counted apart, never flagged.
pub const OUT_OF_SCOPE: [&str; 6] = ["InteractSet", "CSAInteractSet", "PlayAnimSet", "GrabSet", "FaceSet", "WarRoomSet"];

/// The car (or jet) model's own clips, put in the animation set under this name by `character::load`.
pub const CAR_SET: &str = Actions::VEHICLE_SET;

const DT: f32 = 0.032;

// ---------------------------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------------------------

/// Hash to name, from `export\data\names.tsv` when it is there.
pub struct Names(HashMap<u32, String>);

impl Names {
    pub fn load() -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../export/data/names.tsv");
        let map = std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let (h, n) = l.split_once('\t')?;
                Some((u32::from_str_radix(h, 16).ok()?, n.to_owned()))
            })
            .collect();
        Self(map)
    }

    pub fn name(&self, local: &HashMap<u32, String>, hash: u32) -> String {
        local.get(&hash).or_else(|| self.0.get(&hash)).cloned().unwrap_or_else(|| format!("#{hash:08x}"))
    }
}

// ---------------------------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------------------------

pub fn load_character(dir: &Path, arg: &str) -> Result<(String, Tuning, CharacterData), String> {
    let (pack, name) = arg.split_once(':').ok_or("pack:Name")?;
    let mut t = tuning::load(dir, name)?;
    let data = tf2_core::character::load(dir, pack)?;
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing)
        .with_vehicle(data.vehicle.as_ref())
        .with_melee(&data.melee_weapons);
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    Ok((pack.to_owned(), t, data))
}

// ---------------------------------------------------------------------------------------------
// Static checks
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct StaticReport {
    /// A target or name that does not resolve: the data and the port disagree.
    pub errors: Vec<String>,
    /// Oddities that are not wrong by themselves (dead-end entries, a clip found in another set...).
    pub warnings: Vec<String>,
    /// Things `tuning` could not find while loading (`Tuning::missing`).
    pub tuning_missing: Vec<String>,
    pub sets: usize,
    pub clips: usize,
    pub in_scope_clips: usize,
    pub out_of_scope_clips: usize,
    pub exits: usize,
    pub entries: usize,
    pub states: usize,
    /// The control states among the errors (so the dynamic half can say whether a session ever
    /// entered one).
    pub state_errors: Vec<String>,
    pub car_clips: usize,
    /// Car clips no robot clip's model event (or the special) plays, and the ones that are named.
    pub car_unplayed: Vec<String>,
    pub car_named: BTreeSet<String>,
}

/// A static problem: an error, or (in a set or state outside the task's scope, or one that needs
/// an ability the character lacks) a tagged warning.
fn fail(r: &mut StaticReport, tag: impl Into<Option<&'static str>>, msg: impl Into<String>) {
    let msg = msg.into();
    match tag.into() {
        Some(tag) => r.warnings.push(format!("({tag}) {msg}")),
        None => r.errors.push(msg),
    }
}

pub fn static_check(t: &Tuning, data: &CharacterData, names: &Names) -> StaticReport {
    let a = &data.animations;
    let mut r = StaticReport { tuning_missing: t.missing.clone(), sets: a.sets.len(), ..Default::default() };
    let mut local: HashMap<u32, String> = HashMap::new();
    for (set, refs) in &a.sets {
        local.insert(crc32(set.as_bytes()), set.clone());
        for c in refs {
            local.insert(crc32(c.id.as_bytes()), c.id.clone());
        }
    }
    local.insert(EMPTY_SET, "emptyAnimSet".into());
    let set_by_hash: HashMap<u32, &str> = a.sets.keys().map(|k| (crc32(k.as_bytes()), k.as_str())).collect();
    let find = |set: &str, id: u32| a.sets.get(set).and_then(|refs| refs.iter().find(|c| crc32(c.id.as_bytes()) == id));
    let nm = |h: u32| names.name(&local, h);

    // Sets, clips, unnamed clips, duplicates.
    let mut sets: Vec<&String> = a.sets.keys().collect();
    sets.sort();
    for set in &sets {
        let refs = &a.sets[*set];
        let car = set.as_str() == CAR_SET;
        let out = OUT_OF_SCOPE.contains(&set.as_str());
        let mut ids = BTreeSet::new();
        for c in refs {
            r.clips += 1;
            if car {
                r.car_clips += 1;
            } else if out {
                r.out_of_scope_clips += c.clip.is_some() as usize;
            } else {
                r.in_scope_clips += c.clip.is_some() as usize;
            }
            if c.id == "?" {
                r.warnings.push(format!("{set}: a clip whose name is not in the data's name table"));
            }
            if !ids.insert(c.id.clone()) {
                r.warnings.push(format!("{set}/{}: two clips with this id", c.id));
            }
        }
    }

    // Target resolution, for exits and entries.
    let target = |r: &mut StaticReport, origin: String, kind: &str, cur_set: &str, to_set: u32, to_id: u32| {
        if to_set == EMPTY_SET {
            return;
        }
        let oos = OUT_OF_SCOPE.contains(&cur_set).then_some("out of scope");
        let set_name = if to_set == 0 {
            cur_set
        } else if let Some(s) = set_by_hash.get(&to_set) {
            *s
        } else {
            fail(r, oos, format!("{origin}: {kind} -> set {} is not in this character's animation set", nm(to_set)));
            return;
        };
        if to_id == 0 {
            if a.incoming.get(set_name).is_none_or(Vec::is_empty) {
                r.warnings.push(format!("{origin}: {kind} -> set {set_name} with no clip named, and that set has no entry rules"));
            }
            return;
        }
        match find(set_name, to_id) {
            None => fail(r, oos, format!("{origin}: {kind} -> {set_name}/{} is not a clip of that set", nm(to_id))),
            Some(c) if c.clip.is_none() => fail(r, oos, format!("{origin}: {kind} -> {set_name}/{} has no animation file", c.id)),
            Some(_) => {}
        }
    };
    for set in &sets {
        if set.as_str() == CAR_SET {
            continue;
        }
        for c in &a.sets[*set] {
            for (i, e) in c.exits.iter().enumerate() {
                r.exits += 1;
                target(&mut r, format!("{set}/{} exit {i}", c.id), "exit", set, e.to_set, e.to_id);
            }
        }
        for (i, e) in a.incoming.get(*set).into_iter().flatten().enumerate() {
            r.entries += 1;
            target(&mut r, format!("{set} entry {i}"), "entry", set, e.to_set, e.to_id);
            // The filters: an entry that names a set or clip nothing has can never fit.
            for (what, fs, fi) in [("from", e.from_set, e.from_id), ("base", e.base_set, e.base_id)] {
                if fs != 0 && fs != EMPTY_SET {
                    match set_by_hash.get(&fs) {
                        None => r.warnings.push(format!("{set} entry {i}: {what}-filter set {} is not in this character", nm(fs))),
                        Some(fset) if fi != 0 && find(fset, fi).is_none() => {
                            r.warnings.push(format!("{set} entry {i}: {what}-filter clip {fset}/{} does not exist", nm(fi)))
                        }
                        _ => {}
                    }
                } else if fs == 0 && fi != 0 && !a.sets.values().flatten().any(|c| crc32(c.id.as_bytes()) == fi) {
                    r.warnings.push(format!("{set} entry {i}: {what}-filter clip {} is in no set", nm(fi)));
                }
            }
        }
    }

    // Control states' clips. The control graph is shared by every robot, so a state may need an
    // ability this one lacks (flight): those are tagged and not counted as errors.
    for state in &t.control.states {
        r.states += 1;
        let named = state.anim_set.as_deref();
        let tag: Option<&'static str> = if named.is_some_and(|s| OUT_OF_SCOPE.contains(&s)) {
            Some("out of scope")
        } else if !t.forms.can_fly && (matches!(named, Some("FlySet" | "HoverSet")) || state.name.contains("Fly") || state.name.contains("Hover")) {
            Some("flight state, character cannot fly")
        } else if !t.forms.can_drive && state.name.starts_with("stateDrive") {
            Some("drive state, character cannot drive")
        } else if !t.forms.fly.can_do_stunts && state.name.contains("Stunt") {
            Some("stunt state, character cannot do stunts")
        } else {
            None
        };
        let before = (r.errors.len(), r.warnings.len());
        if let Some(set) = named.filter(|s| !a.sets.contains_key(*s)) {
            fail(&mut r, tag, format!("state {}: names set {set}, which this character does not have", state.name));
            if r.errors.len() > before.0 {
                r.state_errors.push(state.name.clone());
            }
            continue;
        }
        if state.anim != 0 {
            let in_named = named.is_some_and(|s| find(s, state.anim).is_some_and(|c| c.clip.is_some()));
            if !in_named {
                let elsewhere: Vec<&str> = sets
                    .iter()
                    .filter(|s| find(s, state.anim).is_some_and(|c| c.clip.is_some()))
                    .map(|s| s.as_str())
                    .collect();
                if elsewhere.is_empty() {
                    fail(&mut r, tag, format!("state {}: clip {} ({:?}) is in no set", state.name, nm(state.anim), named));
                    if r.errors.len() > before.0 {
                        r.state_errors.push(state.name.clone());
                    }
                } else if named.is_some() {
                    r.warnings.push(format!(
                        "state {}: clip {} is not in its set {:?}, only in {}",
                        state.name,
                        nm(state.anim),
                        named,
                        elsewhere.join(", ")
                    ));
                }
            }
        }
    }

    // The car model's clips and who plays them.
    let car_ids: BTreeSet<String> = a.sets.get(CAR_SET).into_iter().flatten().map(|c| c.id.clone()).collect();
    let mut named: BTreeSet<String> = BTreeSet::new();
    for set in &sets {
        if set.as_str() == CAR_SET {
            continue;
        }
        for c in &a.sets[*set] {
            let Some(clip) = &c.clip else { continue };
            for ev in &clip.model_events {
                for id in [&ev.alternate_clip, &ev.alternate_then].into_iter().flatten() {
                    if car_ids.contains(id) {
                        named.insert(id.clone());
                    } else {
                        let tag = car_ids.is_empty().then_some("character has no car model");
                        fail(&mut r, tag, format!("{set}/{}: model event starts car clip {id}, which the car model does not have", c.id));
                    }
                }
            }
        }
    }
    if t.special.vehicle_anim != 0 {
        match a.sets.get(CAR_SET).into_iter().flatten().find(|c| crc32(c.id.as_bytes()) == t.special.vehicle_anim) {
            Some(c) => {
                named.insert(c.id.clone());
            }
            None => r.warnings.push(format!("special: specialAbilityVehicleAnim {} is not a car clip", nm(t.special.vehicle_anim))),
        }
    }
    r.car_unplayed = car_ids.difference(&named).cloned().collect();
    r.car_named = named;
    r
}

// ---------------------------------------------------------------------------------------------
// Dynamic coverage
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Seen {
    pub first_phase: String,
    pub updates: u32,
    pub max_tick: f32,
}

#[derive(Default)]
pub struct Coverage {
    pub seen: BTreeMap<(String, String), Seen>,
    pub panics: Vec<String>,
    pub updates: u64,
    /// Control states (and overlay states) that were entered, with the segment that first did.
    pub states: BTreeMap<String, String>,
    /// Control states that name a clip's set and were seen playing it from another set.
    pub mismatches: BTreeSet<String>,
    /// `--log <segment>`: print each change of control state / clip while that segment runs.
    pub log_phase: Option<String>,
    last_log: String,
}

impl Coverage {
    fn note(&mut self, set: &str, id: &str, tick: f32, phase: &str) {
        let e = self.seen.entry((set.to_owned(), id.to_owned())).or_insert_with(|| Seen { first_phase: phase.to_owned(), ..Default::default() });
        e.updates += 1;
        e.max_tick = e.max_tick.max(tick);
    }
}

struct Session<'a> {
    t: &'a Tuning,
    clips: Clips<'a>,
    weapons: &'a [WeaponDef],
    phase: &'static str,
    s: State,
    arena: Arena,
    gun: Gun,
    shot_timer: f32,
    shots: u32,
    current: usize,
    cooling: f32,
    cov: &'a mut Coverage,
}

fn still() -> Input {
    Input::default()
}

/// The stick as the game's input builder gives it with the camera looking along +y.
fn walk(v: Vec2) -> Input {
    Input { stick: v, pad_stick: v, steer: v.x, ..Default::default() }
}

fn bit(b: u32) -> u64 {
    1u64 << b
}

fn press(mut i: Input, b: u32) -> Input {
    i.buttons_down |= bit(b);
    i.buttons_hit |= bit(b);
    i
}

fn hold(mut i: Input, b: u32) -> Input {
    i.buttons_down |= bit(b);
    i
}

fn jump_press(mut i: Input) -> Input {
    i.jump = true;
    i.jump_held = true;
    i
}

fn jump_hold(mut i: Input) -> Input {
    i.jump_held = true;
    i
}

/// Weapon mode: the aim along `aim`, the slide button as the game holds it with the aim, the stick.
fn aimed(stick: Vec2, aim: Vec2) -> Input {
    Input { stick, pad_stick: stick, steer: stick.x, aim: Some(aim), drift: true, ..Default::default() }
}

fn firing(i: Input) -> Input {
    hold(i, button::WEAPON_FIRE)
}

/// The game's melee button is also the dodge and the car's gun (`player.rs`).
fn melee_all(i: Input, hit: bool) -> Input {
    let mut i = i;
    let all = bit(button::ATTACK_FAST) | bit(button::DODGE) | bit(button::ATTACK_VEHICLE);
    i.buttons_down |= all;
    if hit {
        i.buttons_hit |= all;
    }
    i
}

fn toward(degrees: f32) -> Vec2 {
    Vec2::from_angle(degrees.to_radians()).rotate(Vec2::Y)
}

impl<'a> Session<'a> {
    fn fresh(&mut self) {
        self.s = State::new(self.t);
        self.arena = Arena::default();
        self.gun = Gun::default();
        self.shot_timer = 0.0;
        self.shots = 0;
        self.current = 0;
        self.cooling = 0.0;
    }

    fn target(&self, pos: Vec3, large: bool) -> Target {
        Target {
            id: 1,
            pos,
            radius: self.t.robot_radius,
            height: self.t.robot_height,
            character: true,
            large,
            surface: tf2_core::melee::surface::UPPER_BODY,
            dashing: false,
            health: f32::MAX,
            head: None,
        }
    }

    fn step(&mut self, input: &Input) {
        let t = self.t;
        let unfolding = self.s.action_clip(t).is_some_and(|(set, _, _)| set == "Vehicle2RobotSet");
        sim::step(&mut self.s, input, t, &self.arena, DT);
        self.cov.updates += 1;
        // The weapon manager's stand-in, for the arm layer.
        // [stand-in] The hand's slot: the weapon in hand (0 or 1), and 2 or 3 on every other shot,
        // the list's second entries (the game's slots for the weapons' children are not known here).
        if let Some(def) = self.weapons.get(self.current).or(self.weapons.first()).filter(|_| !self.s.dead()) {
            let aiming = input.aim.is_some() && !self.s.is_vehicle();
            if aiming {
                self.gun.take_aim(self.clips, def, unfolding);
            } else {
                self.gun.put_away(self.clips, def);
            }
            let trigger = self.s.firing && !self.gun.holds_trigger(self.clips, self.s.firing);
            self.shot_timer -= DT;
            if trigger && self.shot_timer <= 0.0 {
                let slot = self.current as i32 + 2 * (self.shots % 2) as i32;
                self.gun.fired(self.clips, Hand { name: def.name, slot, child: None });
                self.shot_timer = 0.25;
                self.shots += 1;
                if self.shots.is_multiple_of(24) {
                    self.gun.overheated(self.clips, def);
                    self.cooling = 2.0;
                }
            }
            if self.cooling > 0.0 {
                self.cooling -= DT;
                if self.cooling <= 0.0 {
                    self.gun.cooled(def);
                }
            }
            self.gun.step(self.clips, def.name, DT);
            self.gun.sound_commands.clear();
        }
        self.record();
    }

    fn record(&mut self) {
        let t = self.t;
        let phase = self.phase;
        for name in [self.s.control_state(t), self.s.overlay_state(t)].into_iter().flatten() {
            if !self.cov.states.contains_key(name) {
                self.cov.states.insert(name.to_owned(), phase.to_owned());
            }
        }
        if let Some((set, id, tick)) = self.s.action_clip(t) {
            self.cov.note(set, id, tick, phase);
            // A control state that names its clip's set, playing the same clip from another set.
            if let Some(def) = self.s.control_state(t).and_then(|n| t.control.states.iter().find(|d| d.name == n))
                && def.anim != 0
                && def.anim == crc32(id.as_bytes())
                && let Some(named) = def.anim_set.as_deref()
                && named != set
            {
                self.cov.mismatches.insert(format!("{}: names {named}/{id}, the port plays {set}/{id}", def.name));
            }
        } else if let Some(c) = self.s.base_clip(t) {
            self.cov.note(c.set, c.id, c.tick, phase);
        }
        if let (Some(c), Some(set)) = (self.s.hit_partial.current(&t.rule_sets), t.rule_sets.get(&self.s.hit_partial.set)) {
            self.cov.note(&set.set_name, &c.name, self.s.hit_partial.tick, phase);
        }
        if let Some(arm) = self.gun.arm
            && let Some(r) = self.clips.arm.get(arm.clip)
        {
            self.cov.note("WeaponPartialSet", &r.id, arm.tick, phase);
        }
        if self.cov.log_phase.as_deref().is_some_and(|p| phase.contains(p)) {
            let leading = self
                .s
                .action_clip(t)
                .map(|(set, id, _)| format!("{set}/{id}"))
                .or_else(|| self.s.base_clip(t).map(|c| format!("{}/{}", c.set, c.id)))
                .unwrap_or_else(|| "-".into());
            let arm = self.gun.arm.and_then(|a| self.clips.arm.get(a.clip)).map_or("-", |r| r.id.as_str());
            let now = format!(
                "{} / {} {:?} {leading} [arm {arm}, firing {}, partial {}]",
                self.s.control_state(t).unwrap_or("-"),
                self.s.overlay_state(t).unwrap_or("-"),
                self.s.mode,
                self.s.firing,
                self.s.hit_partial.playing().is_some()
            );
            if now != self.cov.last_log {
                println!("      [{phase} #{}] {now}  pos ({:.1}, {:.1}, {:.1}) vel ({:.1}, {:.1}, {:.1})", self.cov.updates, self.s.pos.x, self.s.pos.y, self.s.pos.z, self.s.vel.x, self.s.vel.y, self.s.vel.z);
                self.cov.last_log = now;
            }
        }
    }

    fn run(&mut self, input: Input, n: usize) {
        for _ in 0..n {
            self.step(&input);
        }
    }

    /// One update with the press, then `n - 1` with the same input held (no new hit).
    fn tap(&mut self, input: Input, n: usize) {
        self.step(&input);
        let mut held = input;
        held.buttons_hit = 0;
        held.jump = false;
        self.run(held, n.saturating_sub(1));
    }

    fn hit(&mut self, amount: f32, flag: u32) {
        self.s.receive_hit(amount, flag, Vec3::new(0.0, -18.0, 12.0));
    }

    fn stun(&mut self) {
        let p = Packet { amount: 1.0, flags: flags::STUN, direction: Vec3::new(0.0, -1.0, 0.0), stun_time: 2.0, ..Default::default() };
        self.s.receive_damage(p, self.t);
    }

    fn kill(&mut self, flag: u32) {
        self.s.receive_hit(1.0e9, flag, Vec3::new(0.0, -10.0, 4.0));
    }

    fn headshot(&mut self) {
        let p = Packet { amount: 1.0e9, direction: Vec3::new(0.0, -1.0, 0.0), headshot: true, ..Default::default() };
        self.s.receive_damage(p, self.t);
    }

    fn switch_weapon(&mut self) {
        if self.weapons.len() > 1 {
            self.current = (self.current + 1) % 2;
            let name = self.weapons[self.current].name;
            self.gun.switched(self.clips, Hand { name, slot: self.current as i32, child: None });
        }
    }

    fn platform(&mut self, top: f32) {
        self.arena = Arena { boxes: vec![Box3 { min: Vec3::new(-60.0, -60.0, -10.0), max: Vec3::new(60.0, 30.0, top) }], ..Default::default() };
        self.s.pos = Vec3::new(0.0, 0.0, top);
    }

    fn wall(&mut self) {
        self.arena = Arena { boxes: vec![Box3 { min: Vec3::new(-50.0, 30.0, 0.0), max: Vec3::new(57.0, 70.0, 43.3) }], ..Default::default() };
    }
}

type Segment = fn(&mut Session);

fn seg_idle(x: &mut Session) {
    x.fresh();
    x.run(still(), 900);
}

fn seg_locomotion(x: &mut Session) {
    x.fresh();
    x.run(still(), 20);
    x.run(walk(Vec2::Y * 0.5), 40);
    x.run(still(), 30);
    x.run(walk(Vec2::Y), 60);
    x.run(still(), 50);
    x.run(walk(Vec2::Y * 0.3), 20);
    x.run(walk(Vec2::Y), 40);
    x.run(walk(Vec2::NEG_X), 30);
    x.run(walk(Vec2::NEG_Y), 30);
    x.run(walk(Vec2::X), 30);
    x.run(walk(Vec2::Y), 30);
    x.run(still(), 60);
    x.run(walk(Vec2::Y * 0.1), 20);
    x.run(still(), 30);
    for d in [Vec2::X, Vec2::NEG_X, Vec2::NEG_Y, Vec2::new(1.0, 1.0).normalize(), Vec2::new(-1.0, -1.0).normalize()] {
        x.run(walk(d), 25);
        x.run(still(), 40);
        x.run(walk(d * 0.5), 25);
        x.run(still(), 40);
    }
}

fn seg_jumps(x: &mut Session) {
    x.fresh();
    x.run(still(), 30);
    x.tap(jump_press(still()), 5);
    x.run(still(), 90);
    x.tap(jump_press(still()), 40);
    x.run(still(), 90);
    x.run(walk(Vec2::Y), 40);
    x.tap(jump_press(walk(Vec2::Y)), 6);
    x.run(walk(Vec2::Y), 90);
    x.run(still(), 60);
    x.run(walk(Vec2::Y), 30);
    x.tap(jump_press(walk(Vec2::Y)), 6);
    x.run(still(), 100);
    x.run(walk(Vec2::Y * 0.5), 30);
    x.tap(jump_press(walk(Vec2::Y * 0.5)), 6);
    x.run(walk(Vec2::Y * 0.5), 90);
    x.run(still(), 40);
    x.run(walk(Vec2::Y), 30);
    x.tap(jump_press(walk(Vec2::Y)), 40);
    x.run(walk(Vec2::Y), 90);
    x.run(still(), 40);
    // Jump, turn in the air, land with the stick the other way.
    x.run(walk(Vec2::Y), 30);
    x.tap(jump_press(walk(Vec2::Y)), 6);
    x.run(walk(Vec2::NEG_Y), 100);
    x.run(still(), 50);
    // Jump again in the air (double jump, if there is one).
    x.tap(jump_press(still()), 10);
    x.tap(jump_press(still()), 10);
    x.run(still(), 100);
}

fn seg_falls(x: &mut Session) {
    for (i, top) in [6.0f32, 14.0, 24.0, 45.0, 90.0].into_iter().enumerate() {
        for (j, stick) in [Vec2::Y, Vec2::Y * 0.5, Vec2::ZERO].into_iter().enumerate() {
            x.fresh();
            x.platform(top);
            let _ = (i, j);
            x.run(walk(stick), 70);
            x.run(still(), 120);
            // Straight on in the fall, then the other way.
            x.platform(top);
            x.s.vel = Vec3::ZERO;
            x.run(walk(stick), 40);
            x.run(walk(Vec2::NEG_Y), 20);
            x.run(still(), 100);
        }
    }
    // Off the edge in weapon mode, running or standing over it, the stick held or let go in the
    // air, for each drop, and landing with the stick any way round the aim.
    for top in [6.0f32, 14.0, 24.0, 45.0, 90.0] {
        // All four diagonals: plain `Weapon_RunFwdL` / `Weapon_RunBackR` are reached only by the
        // entry walk of a landing with the stick in their sectors (the unpublished notes).
        let diagonals = [Vec2::new(1.0, 1.0), Vec2::new(-1.0, -1.0), Vec2::new(-1.0, 1.0), Vec2::new(1.0, -1.0)].map(Vec2::normalize);
        for stick in [Vec2::Y, Vec2::NEG_Y, Vec2::X, Vec2::NEG_X, Vec2::ZERO].into_iter().chain(diagonals) {
            for let_go in [false, true] {
                x.fresh();
                // A 40 x 40 island; he starts 12 in from the edge he will cross (standing: over it).
                x.arena = Arena { boxes: vec![Box3 { min: Vec3::new(-20.0, -20.0, -10.0), max: Vec3::new(20.0, 20.0, top) }], ..Default::default() };
                x.s.pos = if stick == Vec2::ZERO { Vec3::new(21.0, 0.0, top) } else { (stick.normalize() * 8.0).extend(top) };
                x.run(aimed(stick, Vec2::Y), 30);
                x.run(aimed(if let_go { Vec2::ZERO } else { stick }, Vec2::Y), 90);
                x.run(aimed(Vec2::ZERO, Vec2::Y), 60);
            }
        }
    }
    x.fresh();
    x.platform(30.0);
    x.run(aimed(Vec2::Y, Vec2::Y), 60);
    x.run(aimed(Vec2::ZERO, Vec2::Y), 120);
    // Jump with a running start off the edge.
    x.fresh();
    x.platform(30.0);
    x.run(walk(Vec2::Y), 38);
    x.tap(jump_press(walk(Vec2::Y)), 6);
    x.run(walk(Vec2::Y), 100);
    x.run(still(), 60);
}

fn seg_dodge(x: &mut Session) {
    let dirs: [(&str, Vec2); 8] = [
        ("front", Vec2::Y),
        ("right", Vec2::X),
        ("left", Vec2::NEG_X),
        ("back", Vec2::NEG_Y),
        ("front-right", Vec2::new(1.0, 1.0).normalize()),
        ("front-left", Vec2::new(-1.0, 1.0).normalize()),
        ("back-right", Vec2::new(1.0, -1.0).normalize()),
        ("back-left", Vec2::new(-1.0, -1.0).normalize()),
    ];
    for (_, d) in dirs {
        // On foot, stick held with the dodge button.
        x.fresh();
        x.run(still(), 10);
        x.tap(press(walk(d), button::DODGE), 3);
        x.run(still(), 70);
        // The dodge while running.
        x.fresh();
        x.run(walk(Vec2::Y), 30);
        x.tap(press(walk(d), button::DODGE), 3);
        x.run(still(), 70);
        // In weapon mode: aimed along +y, the stick any way round the aim.
        x.fresh();
        x.run(aimed(Vec2::ZERO, Vec2::Y), 10);
        x.tap(press(aimed(d, Vec2::Y), button::DODGE), 3);
        x.run(aimed(Vec2::ZERO, Vec2::Y), 70);
        // The dodge twice in a row (the cooldown), and the second after it.
        x.fresh();
        x.tap(press(walk(d), button::DODGE), 3);
        x.run(still(), 5);
        x.tap(press(walk(d), button::DODGE), 3);
        x.run(still(), 40);
        x.tap(press(walk(d), button::DODGE), 3);
        x.run(still(), 60);
    }
    // The dodge in the air.
    x.fresh();
    x.tap(jump_press(still()), 6);
    x.tap(press(walk(Vec2::X), button::DODGE), 3);
    x.run(still(), 100);
}

fn seg_melee(x: &mut Session) {
    for (large, distance) in [(false, 6.0f32), (true, 6.0), (false, 16.0), (false, 40.0)] {
        for spacing in [10usize, 14, 20, 28] {
            x.fresh();
            x.arena.targets = vec![x.target(Vec3::new(0.0, distance, 0.0), large)];
            x.run(still(), 5);
            for _ in 0..6 {
                x.tap(press(still(), button::ATTACK_FAST), 3);
                x.run(still(), spacing.saturating_sub(3));
            }
            x.run(still(), 80);
        }
    }
    // Without a target, with the game's all-in-one button, and moving.
    x.fresh();
    for _ in 0..5 {
        x.tap(melee_all(still(), true), 3);
        x.run(melee_all(still(), false), 2);
        x.run(still(), 12);
    }
    x.run(still(), 80);
    x.fresh();
    x.arena.targets = vec![x.target(Vec3::new(0.0, 20.0, 0.0), false)];
    x.run(walk(Vec2::Y), 20);
    x.tap(press(walk(Vec2::Y), button::ATTACK_FAST), 3);
    x.run(walk(Vec2::Y), 10);
    x.tap(press(walk(Vec2::Y), button::ATTACK_FAST), 3);
    x.run(still(), 90);
    // Held (the charge) and let go, at a target and without.
    for tg in [true, false] {
        x.fresh();
        if tg {
            x.arena.targets = vec![x.target(Vec3::new(0.0, 12.0, 0.0), false)];
        }
        x.tap(press(still(), button::ATTACK_FAST), 3);
        x.run(hold(still(), button::ATTACK_FAST), 15);
        x.run(still(), 40);
        x.tap(press(still(), button::ATTACK_FAST), 3);
        x.run(hold(still(), button::ATTACK_FAST), 60);
        x.run(still(), 90);
        // Three taps then a hold: the charged finisher.
        for _ in 0..2 {
            x.tap(press(still(), button::ATTACK_FAST), 3);
            x.run(still(), 12);
        }
        x.tap(press(still(), button::ATTACK_FAST), 3);
        x.run(hold(still(), button::ATTACK_FAST), 50);
        x.run(still(), 100);
    }
    // Melee after a dodge, in the air, and in weapon mode.
    x.fresh();
    x.tap(press(walk(Vec2::X), button::DODGE), 3);
    x.run(still(), 8);
    x.tap(press(still(), button::ATTACK_FAST), 3);
    x.run(still(), 80);
    x.fresh();
    x.tap(jump_press(still()), 6);
    x.tap(press(still(), button::ATTACK_FAST), 3);
    x.run(still(), 120);
    x.fresh();
    x.run(aimed(Vec2::ZERO, Vec2::Y), 10);
    x.tap(press(aimed(Vec2::ZERO, Vec2::Y), button::ATTACK_FAST), 3);
    x.run(aimed(Vec2::ZERO, Vec2::Y), 60);
}

fn seg_punch(x: &mut Session) {
    // Jump, then the attack held in the air: the ground punch; and a short tap in the air.
    for (hold_for, start) in [(40usize, Vec2::ZERO), (4, Vec2::ZERO), (40, Vec2::Y), (40, Vec2::NEG_Y)] {
        x.fresh();
        x.arena.targets = vec![x.target(Vec3::new(0.0, 8.0, 0.0), false)];
        x.run(walk(start), 20);
        x.tap(jump_press(walk(start)), 3);
        x.run(walk(start), 5);
        x.run(hold(walk(start), button::ATTACK_FAST), hold_for);
        x.run(still(), 120);
    }
    // From height.
    for top in [20.0f32, 60.0] {
        x.fresh();
        x.platform(top);
        x.run(walk(Vec2::Y), 45);
        x.run(hold(walk(Vec2::Y), button::ATTACK_FAST), 80);
        x.run(still(), 120);
    }
    // The all-in-one button in the air, as the game sends it.
    x.fresh();
    x.tap(jump_press(still()), 3);
    x.run(still(), 5);
    x.run(melee_all(still(), false), 40);
    x.run(still(), 120);
}

fn seg_weapon(x: &mut Session) {
    let aim = Vec2::Y;
    x.fresh();
    x.run(still(), 10);
    x.run(aimed(Vec2::ZERO, aim), 40);
    // Strafing every 45 degrees round the aim, then stopped, then the next.
    for k in 0..8 {
        let d = toward(k as f32 * 45.0);
        x.run(aimed(d, aim), 25);
        x.run(aimed(Vec2::ZERO, aim), 12);
        // Long enough for the clip that leads in to give way to the steady one.
        x.run(aimed(d, aim), 90);
        x.run(aimed(Vec2::ZERO, aim), 20);
    }
    for k in 0..8 {
        let d = toward(k as f32 * 45.0);
        x.run(aimed(d * 0.5, aim), 25);
        x.run(aimed(Vec2::ZERO, aim), 12);
        x.run(aimed(d * 0.5, aim), 90);
        x.run(aimed(Vec2::ZERO, aim), 20);
    }
    // Every 15 degrees, held.
    for k in 0..24 {
        x.run(aimed(toward(k as f32 * 15.0), aim), 40);
    }
    x.run(aimed(Vec2::ZERO, aim), 20);
    // Round the full circle without stopping.
    for k in 0..16 {
        x.run(aimed(toward(k as f32 * 22.5), aim), 8);
    }
    x.run(aimed(Vec2::ZERO, aim), 20);
    // Turning the aim on the spot, both ways, and far.
    for deg in [60.0f32, -60.0, 120.0, -120.0, 180.0, 0.0] {
        x.run(aimed(Vec2::ZERO, toward(deg)), 40);
    }
    // Moving while the aim swings.
    for deg in [90.0f32, 0.0, -90.0, 0.0] {
        x.run(aimed(Vec2::Y, toward(deg)), 30);
    }
    x.run(aimed(Vec2::ZERO, aim), 20);
    // Out of weapon mode and back.
    x.run(still(), 40);
    x.run(aimed(Vec2::ZERO, aim), 30);
    x.run(walk(Vec2::Y), 30);
    x.run(aimed(Vec2::Y, aim), 30);
    x.run(still(), 40);

    // Firing: standing, then strafing each way, a long burst to overheat, a tap, a switch.
    x.fresh();
    x.run(aimed(Vec2::ZERO, aim), 40);
    x.run(firing(aimed(Vec2::ZERO, aim)), 80);
    x.run(aimed(Vec2::ZERO, aim), 20);
    for k in 0..8 {
        x.run(firing(aimed(toward(k as f32 * 45.0), aim)), 25);
    }
    x.run(aimed(Vec2::ZERO, aim), 20);
    x.run(firing(aimed(Vec2::ZERO, aim)), 400);
    x.run(aimed(Vec2::ZERO, aim), 120);
    x.switch_weapon();
    x.run(aimed(Vec2::ZERO, aim), 40);
    x.run(firing(aimed(Vec2::ZERO, aim)), 100);
    x.run(aimed(Vec2::ZERO, aim), 30);
    x.switch_weapon();
    x.run(firing(aimed(Vec2::Y, aim)), 60);
    x.run(still(), 60);

    // Firing as the weapon is drawn, and the trigger pulled during the pull-out.
    x.fresh();
    x.run(firing(aimed(Vec2::ZERO, aim)), 80);
    x.run(still(), 60);

    // Jumps in weapon mode: strafing left, standing, forward, then the landing run into a strafe.
    x.fresh();
    x.run(aimed(Vec2::NEG_X, aim), 20);
    x.tap(jump_press(aimed(Vec2::NEG_X, aim)), 6);
    x.run(aimed(Vec2::NEG_X, aim), 80);
    x.run(aimed(Vec2::ZERO, aim), 20);
    x.tap(jump_press(aimed(Vec2::ZERO, aim)), 6);
    x.run(aimed(Vec2::ZERO, aim), 90);
    x.run(aimed(Vec2::Y, aim), 20);
    x.tap(jump_press(aimed(Vec2::Y, aim)), 6);
    x.run(aimed(Vec2::Y, aim), 90);
    x.run(aimed(Vec2::NEG_Y, aim), 20);
    x.tap(jump_press(aimed(Vec2::NEG_Y, aim)), 6);
    x.run(aimed(Vec2::NEG_Y, aim), 90);
    x.run(aimed(Vec2::X, aim), 20);
    x.tap(jump_press(aimed(Vec2::X, aim)), 6);
    x.run(aimed(Vec2::X, aim), 90);
    x.tap(jump_press(aimed(Vec2::ZERO, aim)), 40);
    x.run(aimed(Vec2::ZERO, aim), 90);
    // Firing in the air; leaving weapon mode in the air.
    x.tap(jump_press(firing(aimed(Vec2::Y, aim))), 6);
    x.run(firing(aimed(Vec2::Y, aim)), 40);
    x.run(still(), 90);
    x.tap(jump_press(aimed(Vec2::Y, aim)), 6);
    x.run(walk(Vec2::Y), 90);
    x.run(still(), 40);
    // Into weapon mode in the air.
    x.tap(jump_press(walk(Vec2::Y)), 6);
    x.run(aimed(Vec2::Y, aim), 90);
    x.run(still(), 40);
}

fn seg_climb(x: &mut Session) {
    let climb = |stick: Vec2, pad: Vec2| Input { stick, pad_stick: pad, buttons_down: bit(button::CLIMB), ..Default::default() };
    // Run at the wall and jump at it with Climb held; up, over the top.
    x.fresh();
    x.wall();
    for i in 0..30 {
        let mut input = climb(Vec2::Y, Vec2::Y);
        if i == 0 {
            input.buttons_hit = bit(button::CLIMB);
        }
        input.jump = i == 20;
        input.jump_held = (20..24).contains(&i);
        x.step(&input);
    }
    for _ in 0..250 {
        x.step(&climb(Vec2::ZERO, Vec2::Y));
        if x.s.mode == Mode::Move {
            break;
        }
    }
    x.run(still(), 60);
    // Catch from the ground, climb a varying height (the hand that leads changes with it), shimmy
    // right and left, down to the ground.
    for (dir, up) in [Vec2::X, Vec2::NEG_X].into_iter().flat_map(|d| [12usize, 18, 24, 30, 36, 42, 55, 70].map(|u| (d, u))) {
        x.fresh();
        x.wall();
        x.s.pos = Vec3::new(0.0, 27.0, 0.0);
        x.step(&press(Input::default(), button::CLIMB));
        for _ in 0..up {
            x.step(&climb(Vec2::ZERO, Vec2::Y));
        }
        for _ in 0..40 {
            x.step(&climb(Vec2::ZERO, dir));
        }
        for _ in 0..60 {
            x.step(&climb(Vec2::ZERO, Vec2::new(dir.x, 1.0).normalize()));
        }
        for _ in 0..80 {
            x.step(&climb(Vec2::ZERO, Vec2::NEG_Y));
            if x.s.climb.is_none() {
                break;
            }
        }
        x.run(still(), 20);
    }
    // Up a little and jump off: away from the wall, up the wall, to each side.
    for pad in [Vec2::NEG_Y, Vec2::Y, Vec2::X, Vec2::NEG_X, Vec2::ZERO] {
        x.fresh();
        x.wall();
        x.s.pos = Vec3::new(0.0, 27.0, 0.0);
        x.step(&press(Input::default(), button::CLIMB));
        for _ in 0..20 {
            x.step(&climb(Vec2::ZERO, Vec2::Y));
        }
        let off = Input { stick: pad, pad_stick: pad, jump: true, jump_held: true, ..Default::default() };
        x.step(&off);
        for _ in 0..100 {
            x.step(&Input { stick: pad, pad_stick: pad, ..Default::default() });
            if x.s.mode == Mode::Move {
                break;
            }
        }
        x.run(still(), 40);
    }
    // Stop at different heights (the hand that leads changes), idle on the wall, then go on over
    // the top: the idle and the pull-up of each hand.
    for pause_at in [3usize, 5, 7, 9, 11, 13, 15, 17, 20, 23, 26] {
        x.fresh();
        x.wall();
        x.s.pos = Vec3::new(0.0, 27.0, 0.0);
        x.step(&press(Input::default(), button::CLIMB));
        for _ in 0..pause_at {
            x.step(&climb(Vec2::ZERO, Vec2::Y));
        }
        for _ in 0..25 {
            x.step(&climb(Vec2::ZERO, Vec2::ZERO));
        }
        for _ in 0..400 {
            x.step(&climb(Vec2::ZERO, Vec2::Y));
            if x.s.climb.is_none() && x.s.mode != Mode::Climb {
                break;
            }
        }
        x.run(still(), 60);
    }
    // Drop off by letting go of the button on the way up.
    x.fresh();
    x.wall();
    x.s.pos = Vec3::new(0.0, 27.0, 0.0);
    x.step(&press(Input::default(), button::CLIMB));
    for _ in 0..30 {
        x.step(&climb(Vec2::ZERO, Vec2::Y));
    }
    x.run(still(), 120);
    // Catch and leave the stick at rest: the catch clip plays until it finishes (the Catch phase
    // leaves on the finish or a push, `00856d00` l.120-137), from the ground and from a jump at
    // the wall. Every catch above pushes the stick on its first update, so the catch was left in
    // the update it started (the unpublished notes).
    x.fresh();
    x.wall();
    x.s.pos = Vec3::new(0.0, 27.0, 0.0);
    x.step(&press(Input::default(), button::CLIMB));
    for _ in 0..60 {
        x.step(&climb(Vec2::ZERO, Vec2::ZERO));
    }
    x.run(still(), 40);
    x.fresh();
    x.wall();
    for i in 0..30 {
        let mut input = climb(Vec2::Y, Vec2::Y);
        if i == 0 {
            input.buttons_hit = bit(button::CLIMB);
        }
        input.jump = i == 20;
        input.jump_held = (20..24).contains(&i);
        x.step(&input);
        if x.s.mode == Mode::Climb {
            break;
        }
    }
    for _ in 0..60 {
        x.step(&climb(Vec2::ZERO, Vec2::ZERO));
    }
    x.run(still(), 40);
}

fn seg_car(x: &mut Session) {
    let gas = Input { vehicle: 1.0, ..Default::default() };
    let steer = |v: f32| Input { vehicle: 1.0, steer: v, stick: Vec2::new(v, 0.0), pad_stick: Vec2::new(v, 0.0), ..Default::default() };
    // Standing into the car, drive on, steer both ways, drift, brake to a stop, reverse.
    x.fresh();
    x.run(gas, 100);
    x.run(steer(-1.0), 40);
    x.run(steer(1.0), 40);
    x.run(gas, 30);
    x.run(Input { drift: true, ..steer(1.0) }, 40);
    x.run(Input { drift: true, ..steer(-1.0) }, 40);
    x.run(gas, 30);
    x.tap(Input { turbo: true, jump: true, jump_held: true, ..gas }, 20);
    x.run(gas, 60);
    x.run(Input { brake: 1.0, ..gas }, 80);
    x.run(Input { brake: 1.0, vehicle: 1.0, ..Default::default() }, 80);
    x.run(gas, 40);
    // The car's gun (the melee button in the car).
    x.run(melee_all(gas, true), 60);
    x.run(gas, 30);
    // Let go with nothing held: the plain unfold, standing.
    x.run(still(), 100);
    // From a run, the unfold runs out with the stick held.
    x.fresh();
    x.run(gas, 100);
    x.run(Input { stick: Vec2::Y, pad_stick: Vec2::Y, ..Default::default() }, 60);
    x.run(still(), 40);
    // From a run on foot into the car.
    x.fresh();
    x.run(walk(Vec2::Y), 30);
    x.run(Input { stick: Vec2::NEG_X, vehicle: 1.0, steer: -1.0, ..Default::default() }, 60);
    x.run(Input { jump_held: true, jump: true, ..Default::default() }, 1);
    x.run(Input { jump_held: true, ..Default::default() }, 80);
    x.run(still(), 60);
    // The leap out (jump on release), the slam (melee), the punch (action).
    for (name, out) in [
        ("leap", Input { jump: true, jump_held: true, ..Default::default() }),
        ("slam", melee_all(still(), true)),
        ("punch", press(still(), button::CLIMB)),
    ] {
        let _ = name;
        x.fresh();
        x.arena.targets = vec![x.target(Vec3::new(0.0, 104.0, 0.0), false)];
        x.run(gas, 100);
        x.step(&out);
        let mut held = out;
        held.buttons_hit = 0;
        held.jump = false;
        x.run(held, 60);
        x.run(still(), 150);
    }
    // The car in the air (off a platform), landing, and the unfold in the air.
    x.fresh();
    x.platform(30.0);
    x.run(gas, 80);
    x.run(gas, 80);
    x.run(still(), 100);
    // Out of the car and into weapon mode at once.
    x.fresh();
    x.run(gas, 100);
    x.run(aimed(Vec2::ZERO, Vec2::Y), 60);
    x.run(firing(aimed(Vec2::ZERO, Vec2::Y)), 50);
    x.run(still(), 40);
    // The car's special.
    x.fresh();
    x.run(gas, 100);
    x.tap(press(gas, button::ATTACK_SPECIAL), 40);
    x.run(gas, 120);
    x.run(still(), 120);
}

fn seg_special(x: &mut Session) {
    // On foot: pressed, held a second, let go; again after the wait; while moving; in weapon mode.
    x.fresh();
    x.run(still(), 5);
    x.tap(press(still(), button::ATTACK_SPECIAL), 40);
    x.run(still(), 200);
    x.tap(press(still(), button::ATTACK_SPECIAL), 10);
    x.run(still(), 400);
    x.fresh();
    x.run(walk(Vec2::Y), 20);
    x.tap(press(walk(Vec2::Y), button::ATTACK_SPECIAL), 40);
    x.run(walk(Vec2::Y), 100);
    x.run(still(), 100);
    x.fresh();
    x.run(aimed(Vec2::ZERO, Vec2::Y), 30);
    x.tap(press(aimed(Vec2::ZERO, Vec2::Y), button::ATTACK_SPECIAL), 40);
    x.run(aimed(Vec2::ZERO, Vec2::Y), 120);
    x.fresh();
    x.arena.targets = vec![x.target(Vec3::new(0.0, 6.0, 0.0), false)];
    x.tap(press(still(), button::ATTACK_SPECIAL), 100);
    x.run(still(), 200);
    // In the air.
    x.fresh();
    x.tap(jump_press(still()), 6);
    x.tap(press(still(), button::ATTACK_SPECIAL), 40);
    x.run(still(), 150);
    // The aim swung 60 degrees left, then right, while the special holds: Megatron's special
    // runs in StrafeMode, whose `F_STRAFE_ANG` turns it (`SpecialAttack_IdleTurnL` / `R`).
    let left = Vec2::new(-(60f32.to_radians().sin()), 60f32.to_radians().cos());
    let right = Vec2::new(60f32.to_radians().sin(), 60f32.to_radians().cos());
    // Only the aim changes: `aimed` also holds the weapon-mode trigger, which is not wanted here.
    let aim = |to: Vec2| Input { aim: Some(to), ..still() };
    x.fresh();
    x.run(still(), 5);
    x.tap(press(still(), button::ATTACK_SPECIAL), 60);
    // Inside the ability's 2.75 s (the Intro alone is 1.47 s).
    x.run(aim(left), 14);
    x.run(aim(Vec2::Y), 6);
    x.run(aim(right), 14);
    x.run(still(), 60);
}

fn seg_hits(x: &mut Session) {
    use tf2_core::melee::damage as d;
    let kinds: [(&str, u32); 9] = [
        ("partial", 0),
        ("melee", d::BY_MELEE),
        ("fast flail", d::BY_FAST_FLAIL),
        ("strong flail", d::BY_STRONG_FLAIL),
        ("knockback", d::BY_KNOCKBACK),
        ("partial knockback", d::BY_PARTIAL_KNOCKBACK),
        ("explosion", d::BY_EXPLOSION),
        ("projectile", flags::PROJECTILE),
        ("melee + knockback", d::BY_MELEE | d::BY_KNOCKBACK),
    ];
    for (_, flag) in kinds {
        // Standing; running; in weapon mode firing; in the air; during melee; while climbing is
        // left out.
        x.fresh();
        x.run(still(), 10);
        x.hit(15.0, flag);
        x.run(still(), 220);
        // A second hit inside the cooldown, and one after.
        x.hit(15.0, flag);
        x.run(still(), 10);
        x.hit(15.0, flag);
        x.run(still(), 220);

        x.fresh();
        x.run(walk(Vec2::Y), 30);
        x.hit(15.0, flag);
        x.run(walk(Vec2::Y), 20);
        x.run(still(), 200);

        x.fresh();
        x.run(firing(aimed(Vec2::Y, Vec2::Y)), 30);
        x.hit(15.0, flag);
        x.run(firing(aimed(Vec2::Y, Vec2::Y)), 40);
        x.run(aimed(Vec2::ZERO, Vec2::Y), 200);

        x.fresh();
        x.run(aimed(Vec2::ZERO, Vec2::Y), 30);
        x.hit(15.0, flag);
        x.run(aimed(Vec2::ZERO, Vec2::Y), 200);

        x.fresh();
        x.tap(jump_press(still()), 6);
        x.run(still(), 4);
        x.hit(15.0, flag);
        x.run(still(), 220);

        x.fresh();
        x.tap(press(still(), button::ATTACK_FAST), 3);
        x.run(still(), 5);
        x.hit(15.0, flag);
        x.run(still(), 220);

        x.fresh();
        x.tap(press(walk(Vec2::X), button::DODGE), 3);
        x.run(still(), 3);
        x.hit(15.0, flag);
        x.run(still(), 220);

        // The car ignores it; the state after is the interesting part.
        x.fresh();
        x.run(Input { vehicle: 1.0, ..Default::default() }, 100);
        x.hit(15.0, flag);
        x.run(Input { vehicle: 1.0, ..Default::default() }, 30);
        x.run(still(), 120);
    }
    // Stun, standing and moving and in weapon mode.
    x.fresh();
    x.run(still(), 10);
    x.stun();
    x.run(still(), 200);
    x.fresh();
    x.run(walk(Vec2::Y), 30);
    x.stun();
    x.run(still(), 200);
    x.fresh();
    x.run(aimed(Vec2::ZERO, Vec2::Y), 30);
    x.stun();
    x.run(aimed(Vec2::ZERO, Vec2::Y), 200);
    x.fresh();
    x.run(still(), 10);
    x.stun();
    x.run(still(), 10);
    x.stun();
    x.run(still(), 200);
    // Stunned in the car (driving, and stopped), and in the air.
    for drive in [100usize, 5] {
        x.fresh();
        x.run(Input { vehicle: 1.0, ..Default::default() }, drive);
        x.stun();
        x.run(Input { vehicle: 1.0, ..Default::default() }, 60);
        x.run(still(), 150);
    }
    x.fresh();
    x.tap(jump_press(still()), 6);
    x.run(still(), 5);
    x.stun();
    x.run(still(), 200);
    // A hit while stunned.
    x.fresh();
    x.stun();
    x.run(still(), 10);
    x.hit(10.0, d::BY_FAST_FLAIL);
    x.run(still(), 250);
}

/// Thrown back against a wall: the flybacks, the knockbacks, at several distances and impulses.
fn seg_splat(x: &mut Session) {
    use tf2_core::melee::damage as d;
    for flag in [d::BY_FAST_FLAIL, d::BY_STRONG_FLAIL, d::BY_KNOCKBACK, d::BY_PARTIAL_KNOCKBACK] {
        for wall in [2.0f32, 4.0, 8.0, 14.0, 24.0] {
            for impulse in [Vec3::new(0.0, -18.0, 12.0), Vec3::new(0.0, -30.0, 4.0), Vec3::new(0.0, -10.0, 20.0)] {
                x.fresh();
                x.arena = Arena {
                    boxes: vec![Box3 { min: Vec3::new(-50.0, -wall - 6.0, 0.0), max: Vec3::new(50.0, -wall, 40.0) }],
                    ..Default::default()
                };
                x.run(still(), 10);
                x.s.receive_hit(15.0, flag, impulse);
                x.run(still(), 200);
            }
        }
    }
}

fn seg_deaths(x: &mut Session) {
    use tf2_core::melee::damage as d;
    // On the ground by each kind of last hit.
    for flag in [0u32, d::BY_MELEE, d::BY_EXPLOSION, d::BY_FAST_FLAIL, d::BY_STRONG_FLAIL, d::BY_KNOCKBACK, flags::INSTANT_KILL] {
        x.fresh();
        x.run(still(), 10);
        x.kill(flag);
        x.run(still(), 400);
    }
    x.fresh();
    x.run(still(), 10);
    x.headshot();
    x.run(still(), 400);
    // Running, in weapon mode firing, in melee, in a dodge, at the top of a jump and on its way
    // down, from a ledge, on a wall, in the car, with the hit throwing him into a wall.
    x.fresh();
    x.run(walk(Vec2::Y), 30);
    x.kill(0);
    x.run(walk(Vec2::Y), 20);
    x.run(still(), 400);
    x.fresh();
    x.run(firing(aimed(Vec2::Y, Vec2::Y)), 40);
    x.kill(d::BY_EXPLOSION);
    x.run(firing(aimed(Vec2::Y, Vec2::Y)), 40);
    x.run(still(), 400);
    x.fresh();
    x.run(aimed(Vec2::ZERO, Vec2::Y), 30);
    x.kill(0);
    x.run(aimed(Vec2::ZERO, Vec2::Y), 400);
    x.fresh();
    x.tap(press(still(), button::ATTACK_FAST), 3);
    x.run(still(), 4);
    x.kill(0);
    x.run(still(), 400);
    x.fresh();
    x.tap(press(walk(Vec2::X), button::DODGE), 3);
    x.kill(0);
    x.run(still(), 400);
    for (up, flag) in [(4usize, 0u32), (14, 0), (14, d::BY_EXPLOSION), (30, d::BY_FAST_FLAIL), (50, 0)] {
        x.fresh();
        x.tap(jump_press(still()), 6);
        x.run(jump_hold(still()), up);
        x.kill(flag);
        x.run(still(), 300);
    }
    x.fresh();
    x.platform(60.0);
    x.run(walk(Vec2::Y), 70);
    x.kill(0);
    x.run(still(), 400);
    x.fresh();
    x.wall();
    x.s.pos = Vec3::new(0.0, 27.0, 0.0);
    x.step(&press(Input::default(), button::CLIMB));
    for _ in 0..30 {
        x.step(&Input { pad_stick: Vec2::Y, buttons_down: bit(button::CLIMB), ..Default::default() });
    }
    x.kill(0);
    x.run(still(), 400);
    // In the car: driving, and the same with the last hit an explosion.
    for flag in [0u32, d::BY_EXPLOSION] {
        x.fresh();
        x.run(Input { vehicle: 1.0, ..Default::default() }, 100);
        x.kill(flag);
        x.run(Input { vehicle: 1.0, ..Default::default() }, 20);
        x.run(still(), 400);
    }
    // Into the car, hit during the change.
    x.fresh();
    x.run(Input { vehicle: 1.0, ..Default::default() }, 6);
    x.kill(0);
    x.run(still(), 400);
    // Having been stunned, having been knocked back.
    x.fresh();
    x.stun();
    x.run(still(), 10);
    x.kill(0);
    x.run(still(), 400);
    x.fresh();
    x.hit(10.0, d::BY_FAST_FLAIL);
    x.run(still(), 12);
    x.kill(0);
    x.run(still(), 400);
}

// --- the flyers ---

fn fly(look: Vec2, pad: Vec2) -> Input {
    Input { vehicle: 1.0, look, pad_stick: pad, stick: pad, ..Default::default() }
}

/// Into the flying form and up: the jets climb on the pad stick pulled back (y = -1), the
/// robot flyers and helicopters on the other stick pushed up (measured, 2026-10-20: each alone
/// climbs only its own kind; both together climb all of them).
fn up_into_the_air(x: &mut Session) {
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 12);
    x.run(fly(Vec2::new(0.0, -1.0), Vec2::new(0.0, -1.0)), 30);
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 10);
}

fn seg_fly(x: &mut Session) {
    x.fresh();
    up_into_the_air(x);
    // Steering both ways, climbing, diving, the tight turn.
    // All four diagonals: the robot flyers' `Fly_FwdL` and `Fly_BackR` sit on (-1, 1) and (1, -1).
    for pad in [Vec2::NEG_X, Vec2::X, Vec2::new(0.0, -1.0), Vec2::new(0.0, 1.0), Vec2::new(-1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0), Vec2::new(1.0, -1.0), Vec2::ZERO] {
        x.run(fly(Vec2::ZERO, pad), 40);
        x.run(fly(Vec2::ZERO, Vec2::ZERO), 20);
    }
    for k in 0..32 {
        let look = Vec2::from_angle((k as f32 * 11.25).to_radians()).rotate(Vec2::Y);
        x.run(fly(look, Vec2::ZERO), 40);
        x.run(fly(Vec2::ZERO, Vec2::ZERO), 10);
    }
    x.run(Input { drift: true, ..fly(Vec2::ZERO, Vec2::new(-1.0, 0.0)) }, 40);
    x.run(Input { drift: true, ..fly(Vec2::ZERO, Vec2::new(1.0, 0.0)) }, 40);
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 20);
    // Turbo, brake, both.
    x.tap(Input { jump: true, jump_held: true, ..fly(Vec2::ZERO, Vec2::ZERO) }, 30);
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 60);
    x.run(Input { brake: 1.0, ..fly(Vec2::ZERO, Vec2::ZERO) }, 40);
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 40);
    x.run(Input { brake: 1.0, ..fly(Vec2::ZERO, Vec2::ZERO) }, 60);
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 60);
    // Stunts: the action button, then the stick: left, right, up, down.
    for pad in [Vec2::NEG_X, Vec2::X, Vec2::new(0.0, -1.0), Vec2::new(0.0, 1.0)] {
        up_into_the_air(x);
        x.run(fly(Vec2::ZERO, Vec2::ZERO), 20);
        x.tap(press(fly(Vec2::ZERO, pad), button::ACTION), 4);
        x.run(fly(Vec2::ZERO, pad), 10);
        x.run(fly(Vec2::ZERO, Vec2::ZERO), 80);
        x.tap(press(fly(Vec2::ZERO, pad), button::CLIMB), 4);
        x.run(fly(Vec2::ZERO, Vec2::ZERO), 90);
    }
    // Firing while flying, the special in the air.
    x.run(firing(Input { aim: Some(Vec2::Y), ..fly(Vec2::ZERO, Vec2::ZERO) }), 80);
    x.tap(press(fly(Vec2::ZERO, Vec2::ZERO), button::ATTACK_SPECIAL), 40);
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 150);
    x.tap(press(fly(Vec2::ZERO, Vec2::ZERO), button::ATTACK_FAST), 4);
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 60);
    // Let go of the trigger: the change back, from the air.
    x.run(still(), 200);
    // Land: fly low and let go on the ground.
    x.fresh();
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 12);
    x.run(still(), 120);
    x.fresh();
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 30);
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 30);
    x.run(still(), 150);
    // Flying out with the stick held, and from a run.
    x.fresh();
    x.run(walk(Vec2::Y), 30);
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 80);
    x.run(walk(Vec2::Y), 120);
    x.run(still(), 40);
    // The stick still held on the trigger's first update: `Trans_RobotRun2Vehicle` (FlySet's and
    // DriveSet's entry with `F_L_STICK_AMP` >= 0.01).
    x.fresh();
    x.run(walk(Vec2::Y), 30);
    x.run(fly(Vec2::ZERO, Vec2::Y), 80);
    x.run(still(), 150);
    // Weapon mode on release in the air (weapon hover) and on the ground.
    x.fresh();
    up_into_the_air(x);
    x.tap(press(Input { aim: Some(Vec2::Y), ..still() }, button::WEAPON_MODE), 6);
    x.run(Input { aim: Some(Vec2::Y), ..still() }, 80);
    x.run(still(), 150);
}

/// Up in the air, the trigger let go with the hover button (or the weapon button): hovering.
fn enter_hover(x: &mut Session, weapon: bool) {
    x.fresh();
    up_into_the_air(x);
    if weapon {
        x.tap(press(Input { aim: Some(Vec2::Y), ..still() }, button::WEAPON_MODE), 6);
    } else {
        x.tap(press(Input { jump: true, jump_held: true, ..still() }, button::TRANSFORM_HOVER), 6);
    }
}

fn hov(weapon: bool, pad: Vec2, look: Vec2) -> Input {
    Input { look, pad_stick: pad, stick: pad, aim: weapon.then_some(Vec2::Y), ..Default::default() }
}

/// Each action starts a hover of its own: the hover does not last long (it ends in the hover fall).
fn seg_hover(x: &mut Session) {
    let dirs = [Vec2::Y, Vec2::NEG_Y, Vec2::X, Vec2::NEG_X, Vec2::new(1.0, 1.0).normalize(), Vec2::new(-1.0, 1.0).normalize(), Vec2::new(1.0, -1.0).normalize(), Vec2::new(-1.0, -1.0).normalize()];
    for weapon in [false, true] {
        // Stationary, then moving every way, the camera stick every way.
        enter_hover(x, weapon);
        x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 40);
        for pad in dirs {
            x.run(hov(weapon, pad, Vec2::ZERO), 25);
            x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 10);
        }
        for look in dirs {
            x.run(hov(weapon, Vec2::ZERO, look), 25);
            x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 10);
        }
        x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 300);
        x.run(still(), 150);
        // The dodge in each direction, and twice.
        for d in dirs {
            enter_hover(x, weapon);
            x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 15);
            x.tap(press(hov(weapon, d, Vec2::ZERO), button::DODGE), 4);
            x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 60);
            x.tap(press(hov(weapon, d, Vec2::ZERO), button::DODGE), 4);
            x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 40);
            x.run(still(), 150);
        }
        // Turbo and brake, firing, a strafe while firing, melee, the special.
        enter_hover(x, weapon);
        x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 10);
        x.tap(Input { jump: true, jump_held: true, ..hov(weapon, Vec2::ZERO, Vec2::ZERO) }, 20);
        x.run(Input { brake: 1.0, ..hov(weapon, Vec2::ZERO, Vec2::ZERO) }, 20);
        x.run(firing(Input { aim: Some(Vec2::Y), ..hov(weapon, Vec2::ZERO, Vec2::ZERO) }), 60);
        x.run(firing(Input { aim: Some(Vec2::Y), ..hov(weapon, Vec2::X, Vec2::ZERO) }), 30);
        x.run(still(), 150);
        enter_hover(x, weapon);
        x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 10);
        x.tap(press(hov(weapon, Vec2::ZERO, Vec2::ZERO), button::ATTACK_FAST), 4);
        x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 40);
        x.run(still(), 150);
        enter_hover(x, weapon);
        x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 10);
        x.tap(press(hov(weapon, Vec2::ZERO, Vec2::ZERO), button::ATTACK_SPECIAL), 40);
        x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 100);
        x.run(still(), 150);
        // Hover back to flight (the trigger again), and out to the robot (jump / weapon / melee / action).
        enter_hover(x, weapon);
        x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 15);
        x.run(fly(Vec2::ZERO, Vec2::ZERO), 60);
        x.run(still(), 150);
        for out in [
            Input { jump: true, jump_held: true, ..hov(weapon, Vec2::ZERO, Vec2::ZERO) },
            press(hov(weapon, Vec2::ZERO, Vec2::ZERO), button::ATTACK_FAST),
            press(hov(weapon, Vec2::ZERO, Vec2::ZERO), button::CLIMB),
            press(hov(weapon, Vec2::ZERO, Vec2::ZERO), button::WEAPON_MODE),
            press(hov(weapon, Vec2::ZERO, Vec2::ZERO), button::TRANSFORM_HOVER),
        ] {
            enter_hover(x, weapon);
            x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 15);
            x.tap(out, 6);
            x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 40);
            x.run(still(), 150);
        }
    }
    // Hover from standing, hover down to the ground, hover landing.
    x.fresh();
    x.tap(press(Input { jump: true, jump_held: true, ..still() }, button::TRANSFORM_HOVER), 6);
    x.run(hov(false, Vec2::ZERO, Vec2::ZERO), 40);
    x.run(hov(false, Vec2::ZERO, Vec2::new(0.0, 1.0)), 80);
    x.run(still(), 150);
    // A hit and a stun while hovering.
    use tf2_core::melee::damage as d;
    for weapon in [false, true] {
        for flag in [0u32, d::BY_FAST_FLAIL, d::BY_STRONG_FLAIL, d::BY_KNOCKBACK, d::BY_PARTIAL_KNOCKBACK, d::BY_EXPLOSION] {
            enter_hover(x, weapon);
            x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 15);
            x.hit(15.0, flag);
            x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 200);
        }
        enter_hover(x, weapon);
        x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 15);
        x.stun();
        x.run(hov(weapon, Vec2::ZERO, Vec2::ZERO), 150);
    }
}

/// Leaving the flying form from the air in each way the buttons give: nothing, the hover button
/// (the jump), the melee button (the slam), the action button (the punch), the weapon button,
/// the trigger again; with the stick held and not, high and low.
fn seg_fly_exits(x: &mut Session) {
    for (name, button_) in [("nothing", None), ("melee", Some(button::ATTACK_FAST)), ("action", Some(button::CLIMB)), ("weapon", Some(button::WEAPON_MODE)), ("hover", Some(button::TRANSFORM_HOVER)), ("special", Some(button::ATTACK_SPECIAL)), ("dodge", Some(button::DODGE))] {
        let _ = name;
        for (climb, stick) in [(true, Vec2::ZERO), (true, Vec2::Y), (false, Vec2::ZERO)] {
            x.fresh();
            if climb {
                up_into_the_air(x);
            } else {
                x.run(fly(Vec2::ZERO, Vec2::ZERO), 30);
            }
            let mut release = Input { stick, pad_stick: stick, ..Default::default() };
            if let Some(b) = button_ {
                release = press(release, b);
                if b == button::WEAPON_MODE {
                    release.aim = Some(Vec2::Y);
                }
                if b == button::TRANSFORM_HOVER {
                    release.jump = true;
                    release.jump_held = true;
                }
            }
            x.tap(release, 6);
            let mut held = release;
            held.buttons_hit = 0;
            held.jump = false;
            x.run(held, 40);
            x.run(Input { stick, pad_stick: stick, ..Default::default() }, 150);
            x.run(still(), 60);
        }
    }
    // Out of a hover into the slam / punch while falling is the hover's own; the car-less
    // flyers' leap: the jump held for a second after the release.
    x.fresh();
    up_into_the_air(x);
    x.tap(Input { jump: true, jump_held: true, ..still() }, 40);
    x.run(still(), 150);
}

fn seg_fly_hits(x: &mut Session) {
    use tf2_core::melee::damage as d;
    for flag in [0u32, d::BY_MELEE, d::BY_FAST_FLAIL, d::BY_STRONG_FLAIL, d::BY_KNOCKBACK, d::BY_PARTIAL_KNOCKBACK, d::BY_EXPLOSION] {
        x.fresh();
        up_into_the_air(x);
        x.hit(15.0, flag);
        x.run(fly(Vec2::ZERO, Vec2::ZERO), 40);
        x.run(still(), 200);
    }
    x.fresh();
    up_into_the_air(x);
    x.stun();
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 60);
    x.run(still(), 150);
}

fn seg_fly_deaths(x: &mut Session) {
    use tf2_core::melee::damage as d;
    for flag in [0u32, d::BY_EXPLOSION, d::BY_FAST_FLAIL] {
        x.fresh();
        up_into_the_air(x);
        x.kill(flag);
        x.run(fly(Vec2::ZERO, Vec2::ZERO), 20);
        x.run(still(), 400);
    }
    x.fresh();
    up_into_the_air(x);
    x.headshot();
    x.run(still(), 400);
    // Hovering, and in the change.
    x.fresh();
    up_into_the_air(x);
    x.tap(press(Input { jump: true, jump_held: true, ..still() }, button::TRANSFORM_HOVER), 6);
    x.run(still(), 30);
    x.kill(0);
    x.run(still(), 400);
    x.fresh();
    x.run(fly(Vec2::ZERO, Vec2::ZERO), 5);
    x.kill(0);
    x.run(still(), 400);
}

pub fn run_dynamic(t: &Tuning, data: &CharacterData, log_phase: Option<String>) -> Coverage {
    let mut cov = Coverage { log_phase, ..Default::default() };
    let empty_set: &[tf2_core::formats::anim::AnimRef] = &[];
    let empty_entries: &[tf2_core::formats::anim::IncomingRule] = &[];
    let clips = Clips {
        arm: data.animations.sets.get("WeaponPartialSet").map_or(empty_set, Vec::as_slice),
        entries: data.animations.incoming.get("WeaponPartialSet").map_or(empty_entries, Vec::as_slice),
        object: data.weapon_animations.first().map_or(empty_set, Vec::as_slice),
    };
    let mut segments: Vec<(&'static str, Segment)> = vec![
        ("idle", seg_idle),
        ("locomotion", seg_locomotion),
        ("jumps", seg_jumps),
        ("falls", seg_falls),
        ("dodges", seg_dodge),
        ("melee", seg_melee),
        ("ground punch", seg_punch),
        ("weapon mode", seg_weapon),
        ("climb", seg_climb),
        ("special", seg_special),
        ("hits", seg_hits),
        ("wall splat", seg_splat),
    ];
    if t.forms.can_drive {
        segments.push(("car", seg_car));
    }
    if t.forms.can_fly {
        segments.push(("fly", seg_fly));
        segments.push(("hover", seg_hover));
        segments.push(("fly exits", seg_fly_exits));
        segments.push(("fly hits", seg_fly_hits));
        segments.push(("fly deaths", seg_fly_deaths));
    }
    segments.push(("deaths", seg_deaths));
    for (name, segment) in segments {
        let mut x = Session {
            t,
            clips,
            weapons: &data.weapons,
            phase: name,
            s: State::new(t),
            arena: Arena::default(),
            gun: Gun::default(),
            shot_timer: 0.0,
            shots: 0,
            current: 0,
            cooling: 0.0,
            cov: &mut cov,
        };
        let result = catch_unwind(AssertUnwindSafe(|| segment(&mut x)));
        if let Err(e) = result {
            let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            cov.panics.push(format!("segment '{name}' panicked: {msg}"));
        }
    }
    cov
}

/// The car clips a set of reached robot clips plays through their model events (and the chain
/// each names to follow it), and the car clips reached directly (the car's special).
pub fn car_played(data: &CharacterData, cov: &Coverage) -> BTreeSet<String> {
    let mut played = BTreeSet::new();
    for ((set, id), seen) in &cov.seen {
        if set == CAR_SET {
            played.insert(id.clone());
            continue;
        }
        let Some(clip) = data.animations.sets.get(set).and_then(|refs| refs.iter().find(|r| &r.id == id)).and_then(|r| r.clip.as_ref()) else {
            continue;
        };
        for ev in clip.model_events.iter().filter(|e| e.time <= seen.max_tick) {
            played.extend(ev.alternate_clip.iter().cloned());
            played.extend(ev.alternate_then.iter().cloned());
        }
    }
    played
}

// ---------------------------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------------------------

const VARIABLES: [&str; 31] = [
    "F_STUN_TIME", "F_RANDOM", "I_IN_AIR", "I_SPECIAL_ON", "F_R_STICK_AMP", "F_TIME_SINCE_DAMAGED", "C_JUMP_TYPE",
    "F_Z_VEL", "I_IS_STRAFING", "C_VEHICLE_STUNT", "I_WEAPON_INDEX", "C_INTERACT_TYPE", "C_CHAR_MESSAGE",
    "C_DAMAGE_TYPE", "F_L_STICK_DIR", "I_ATTK_BTN_HELD", "I_SPECIAL_IDLE_ON", "I_ON_GROUND", "F_TIME_IN_ANIM",
    "F_FALL_DUR", "F_R_STICK_DIR", "I_ATTK_BRANCH", "I_IS_HOVERING", "I_PREV_CHAR_FORM", "I_CHAR_FORM",
    "C_INTERACT_ACTION_TYPE", "C_WEAPON_ACTION", "F_L_STICK_AMP", "F_STRAFE_ANG", "C_WEAPON_EQUIP", "F_HP_PERCENT",
];

fn describe_rules(rules: &[tf2_core::formats::anim::Rule], local: &HashMap<u32, String>, names: &Names) -> String {
    if rules.is_empty() {
        return "always".into();
    }
    let vars: HashMap<u32, &str> = VARIABLES.iter().map(|n| (crc32(n.as_bytes()), *n)).collect();
    const LOGIC: [&str; 6] = ["==", "!=", ">", ">=", "<", "<="];
    rules
        .iter()
        .map(|r| {
            let var = vars.get(&r.variable).copied().map_or_else(|| format!("#{:08x}", r.variable), str::to_owned);
            let value = match var.get(..2) {
                Some("F_") => format!("{}", r.float),
                Some("I_") => format!("{}", r.int),
                Some("C_") => names.name(local, r.hash),
                _ => format!("{} / {} / {}", r.float, r.int, names.name(local, r.hash)),
            };
            format!("{var} {} {value}", LOGIC[(r.logic as usize).min(5)])
        })
        .collect::<Vec<_>>()
        .join(" and ")
}

/// What would play an unreached clip: the control states that name it and whether the script
/// entered them, and the entry and exit rules that target it, with their conditions.
fn why_unreached(t: &Tuning, data: &CharacterData, cov: &Coverage, names: &Names, set: &str, id: &str) -> String {
    let a = &data.animations;
    let mut local: HashMap<u32, String> = HashMap::new();
    for (s, refs) in &a.sets {
        local.insert(crc32(s.as_bytes()), s.clone());
        for c in refs {
            local.insert(crc32(c.id.as_bytes()), c.id.clone());
        }
    }
    local.insert(EMPTY_SET, "emptyAnimSet".into());
    let (set_h, id_h) = (crc32(set.as_bytes()), crc32(id.as_bytes()));
    let mut parts: Vec<String> = Vec::new();
    let states: Vec<String> = t
        .control
        .states
        .iter()
        .filter(|s| s.anim == id_h && s.anim_set.as_deref().is_none_or(|x| x == set))
        .map(|s| format!("{}{}", s.name, if cov.states.contains_key(&s.name) { "" } else { " (never entered)" }))
        .collect();
    if !states.is_empty() {
        parts.push(format!("state {}", states.join(", ")));
    }
    let mut rules: Vec<String> = Vec::new();
    for (s2, list) in &a.incoming {
        for e in list {
            let to_set = if e.to_set == 0 { crc32(s2.as_bytes()) } else { e.to_set };
            if e.to_id == id_h && to_set == set_h {
                let from = if e.from_set == 0 && e.from_id == 0 {
                    String::new()
                } else {
                    format!(" from {}/{}", if e.from_set == 0 { "any".into() } else { names.name(&local, e.from_set) }, if e.from_id == 0 { "any".into() } else { names.name(&local, e.from_id) })
                };
                rules.push(format!("entry of {s2}{from} when {}", describe_rules(&e.rules, &local, names)));
            }
        }
    }
    for (s2, refs) in &a.sets {
        for c in refs {
            for e in &c.exits {
                let to_set = if e.to_set == 0 { crc32(s2.as_bytes()) } else { e.to_set };
                if e.to_id == id_h && to_set == set_h {
                    let when = if e.finish { "finished" } else { "at any time" };
                    rules.push(format!("exit of {s2}/{} ({when}) when {}", c.id, describe_rules(&e.rules, &local, names)));
                }
            }
        }
    }
    let more = rules.len().saturating_sub(3);
    rules.truncate(3);
    parts.extend(rules);
    if more > 0 {
        parts.push(format!("and {more} more rules"));
    }
    if parts.is_empty() {
        "no control state and no entry or exit rule targets it".into()
    } else {
        parts.join("; ")
    }
}

struct Summary {
    pack: String,
    errors: usize,
    warnings: usize,
    reached: usize,
    total: usize,
    out_reached: usize,
    out_total: usize,
    car_played: usize,
    car_total: usize,
    panics: usize,
}

fn main() {
    let dir = tf2_core::install::dir_string();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let show_reached = args.iter().any(|a| a == "--reached");
    let static_only = args.iter().any(|a| a == "--static-only");
    let why = args.iter().any(|a| a == "--why");
    let log_phase = args.iter().position(|a| a == "--log").and_then(|i| args.get(i + 1).cloned());
    if let Some(i) = args.iter().position(|a| a == "--log") {
        args.drain(i..(i + 2).min(args.len()));
    }
    args.retain(|a| !a.starts_with("--"));
    let list: Vec<String> = if args.is_empty() {
        ROSTER.map(str::to_owned).to_vec()
    } else {
        args.iter().map(|a| ROSTER.iter().find(|r| r.starts_with(&format!("{a}:"))).map_or(a.clone(), |r| (*r).to_owned())).collect()
    };
    let names = Names::load();
    let mut summaries = Vec::new();
    for arg in &list {
        let (pack, t, data) = match load_character(Path::new(&dir), arg) {
            Ok(x) => x,
            Err(e) => {
                println!("== {arg}: cannot load: {e}");
                continue;
            }
        };
        let r = static_check(&t, &data, &names);
        println!(
            "== {pack} ({}): {} sets, {} clips ({} in scope, {} out of scope, {} car model), {} exit rules, {} entry rules, {} control states",
            arg.split_once(':').map_or("", |p| p.1),
            r.sets,
            r.clips,
            r.in_scope_clips,
            r.out_of_scope_clips,
            r.car_clips,
            r.exits,
            r.entries,
            r.states
        );
        println!("  STATIC: {} errors, {} warnings, tuning could not find {} things", r.errors.len(), r.warnings.len(), r.tuning_missing.len());
        for e in &r.errors {
            println!("    ERROR {e}");
        }
        for w in &r.warnings {
            println!("    warn  {w}");
        }
        for m in &r.tuning_missing {
            println!("    tuning missing: {m}");
        }
        println!(
            "    car model clips: {} total, {} named by a model event or the special, {} named by none{}",
            r.car_clips,
            r.car_named.len(),
            r.car_unplayed.len(),
            if r.car_unplayed.is_empty() { String::new() } else { format!(": {}", r.car_unplayed.join(", ")) }
        );
        let mut summary = Summary {
            pack: pack.clone(),
            errors: r.errors.len(),
            warnings: r.warnings.len(),
            reached: 0,
            total: r.in_scope_clips,
            out_reached: 0,
            out_total: r.out_of_scope_clips,
            car_played: 0,
            car_total: r.car_clips,
            panics: 0,
        };
        if !static_only {
            let cov = run_dynamic(&t, &data, log_phase.clone());
            // The in-scope universe: every clip with an animation file in a set not out of scope and not the car's.
            let mut universe: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            let mut out_universe: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for (set, refs) in &data.animations.sets {
                let list: Vec<&str> = refs.iter().filter(|c| c.clip.is_some()).map(|c| c.id.as_str()).collect();
                if set == CAR_SET {
                    continue;
                }
                if OUT_OF_SCOPE.contains(&set.as_str()) {
                    out_universe.insert(set, list);
                } else {
                    universe.insert(set, list);
                }
            }
            let reached = |set: &str, id: &str| cov.seen.contains_key(&(set.to_owned(), id.to_owned()));
            let (mut got, mut all) = (0usize, 0usize);
            let mut unreached: Vec<(String, usize, usize, Vec<String>)> = Vec::new();
            for (set, ids) in &universe {
                let uniq: BTreeSet<&str> = ids.iter().copied().collect();
                let miss: Vec<String> = uniq.iter().filter(|id| !reached(set, id)).map(|s| s.to_string()).collect();
                got += uniq.len() - miss.len();
                all += uniq.len();
                if !miss.is_empty() {
                    unreached.push((set.to_string(), uniq.len() - miss.len(), uniq.len(), miss));
                }
            }
            let (mut out_got, mut out_all) = (0usize, 0usize);
            for (set, ids) in &out_universe {
                let uniq: BTreeSet<&str> = ids.iter().copied().collect();
                out_got += uniq.iter().filter(|id| reached(set, id)).count();
                out_all += uniq.len();
            }
            let played = car_played(&data, &cov);
            let car_ids: BTreeSet<String> = data.animations.sets.get(CAR_SET).into_iter().flatten().map(|c| c.id.clone()).collect();
            let car_got = played.intersection(&car_ids).count();
            summary.reached = got;
            summary.total = all;
            summary.out_reached = out_got;
            summary.out_total = out_all;
            summary.car_played = car_got;
            summary.car_total = car_ids.len();
            summary.panics = cov.panics.len();
            println!(
                "  DYNAMIC: reached {got}/{all} in-scope clips ({:.0}%), {} unreached; out of scope {out_got}/{out_all} (not flagged); car model clips played {car_got}/{}; {} updates",
                if all == 0 { 0.0 } else { 100.0 * got as f32 / all as f32 },
                all - got,
                car_ids.len(),
                cov.updates
            );
            for p in &cov.panics {
                println!("    PANIC {p}");
            }
            for m in &cov.mismatches {
                println!("    set mismatch: state {m}");
            }
            let all_states: Vec<&str> = t.control.states.iter().map(|s| s.name.as_str()).collect();
            let never: Vec<&str> = all_states.iter().copied().filter(|s| !cov.states.contains_key(*s)).collect();
            println!("    control states entered {}/{}; never entered: {}", all_states.len() - never.len(), all_states.len(), never.join(" "));
            if !r.state_errors.is_empty() {
                let entered: Vec<&String> = r.state_errors.iter().filter(|s| cov.states.contains_key(*s)).collect();
                println!(
                    "    state errors: {} of {} states with a static error were entered by the script{}",
                    entered.len(),
                    r.state_errors.len(),
                    if entered.is_empty() { String::new() } else { format!(": {}", entered.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")) }
                );
            }
            if !unreached.is_empty() {
                println!("    unreached, by set:");
            }
            for (set, g, n, miss) in &unreached {
                println!("      {set} ({g}/{n}): {}", miss.join(" "));
                if why {
                    for id in miss {
                        println!("        {id}: {}", why_unreached(&t, &data, &cov, &names, set, id));
                    }
                }
            }
            let unplayed_car: Vec<&String> = car_ids.difference(&played).collect();
            if !unplayed_car.is_empty() {
                println!("    car model clips not played: {}", unplayed_car.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "));
            }
            if show_reached {
                println!("    reached (set/clip: first segment, updates):");
                for ((set, id), seen) in &cov.seen {
                    println!("      {set}/{id}: {} x{}", seen.first_phase, seen.updates);
                }
            }
        }
        summaries.push(summary);
    }
    println!("\nSUMMARY");
    println!(
        "{:<15} {:>7} {:>8} {:>13} {:>10} {:>12} {:>12} {:>7}",
        "character", "errors", "warnings", "reached/total", "unreached", "out of scope", "car clips", "panics"
    );
    for s in &summaries {
        println!(
            "{:<15} {:>7} {:>8} {:>13} {:>10} {:>12} {:>12} {:>7}",
            s.pack,
            s.errors,
            s.warnings,
            format!("{}/{}", s.reached, s.total),
            s.total - s.reached,
            format!("{}/{}", s.out_reached, s.out_total),
            format!("{}/{}", s.car_played, s.car_total),
            s.panics
        );
    }
}
