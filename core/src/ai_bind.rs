//! Turns the game's AI data (`ai_data`: what the data says) into a running brain (`ai_brain`:
//! the spine that runs it), with the perceptions of `ai_perceive` and the behaviours of
//! `ai_attack` and `ai_move`. Not wired into sim.rs or the game yet.
//!
//! Tags as in ai_brain.rs. A stand-in is marked `[stand-in]` where it is made and counted in the
//! [`Report`]. [`build`] never fails because of a stand-in: it fails only when a brain the list
//! names is not in the data.
//!
//! How the data maps. Every line marked [assumed] is also logged in notes/status.md, Phase 3.
//! - A brain root becomes one `Root`. Its perceptions are the root's score (`Root::perceptions`,
//!   the brain-choice score of `best_brain`). Its goals become the `Goal`s of one `Brain`, which
//!   `Brain::new` sorts by static priority.
//! - A goal (`goalList` item) becomes an `ai_brain::Goal`: its priority, its perceptions as the
//!   goal's scorer, and `msgEnter` / `msgExit` as its enter and leave messages (argument 0)
//!   [assumed]. Its `behaviorList` becomes its behaviour entries (R1, two levels).
//! - A behaviour entry (`behaviorList` or `persistentBehaviorList` item) carries its own priority,
//!   its `rampUpTime` as the hold hash, its `oneShot` flag (R3), and `msgEnter` / `msgExit` /
//!   `msgFinish` (argument 0) [assumed]. The behaviour's own perceptions are its score's aggregate.
//!   The goal's `rampUpTime` is not used [assumed: R1 puts the hold on the behaviour entry].
//! - A link (one perception of an aggregate) sends `msgTrigger` as its message, with the data's
//!   `msgType` as the argument (ai-messages.md section 1; the field names are from objects.json,
//!   the loader is unread). The link's `invertPriority` is the invert byte (R2): the aggregate
//!   inverts the score after the message is sent.
//! - Persistent behaviours are the brain's `persistentBehaviorList`, as `Persistent` entries.
//!   Their channel is [`WEAPON_CHANNEL`] for WeaponSwitch [assumed]; see that constant.
//! - Compare and Range pass the data's groupID to the value getter, and take an assigned
//!   perception's threshold from `AiAttr` (ai_perceive::assigned). A Compare or Range is a
//!   stand-in when its valueType is not described (ai_perceive::VALUE_KINDS), or its compareType is
//!   not one of the two ported ones.
//! - Targeting is bound for the single-target form and the two-target form (00830140), for the
//!   FOE, TURRET and FRIEND types, from `Ctx::senses`. The other target types are stand-ins
//!   ([`perception_stand_in`]).
//! - A Message perception is bound with its latch node (`ai_brain::Latches`, one per perception).
//!   Its node starts with toggleOn when its brain is entered, and the brain's start and stop
//!   register and clear it (ai-messages.md sections 2 and 4). The link and entry messages carry the
//!   data's msgType as their argument [assumed]; see `ai_brain::ARG_SELF` / `ARG_GROUP`.
//! - Behaviour types with no port (MoveOnSpline, Teleport) become [`Unported`]: score 0, so never
//!   picked [stand-in]. MoveOnWayPoints, Tactics, Interact and Idle with trackTarget are bound
//!   (ai_move, ai_attack); Interact scores 0 until something sets the character's handle
//!   [stand-in].
//! - The brainList priority multiplies the root's score in `build` [game 0082eae0, see `weight_root`]:
//!   the brain choice reads priority x aggregate. It is also kept in [`Report::brain_priorities`].
//!   notes/ai-brain.md "Brain choice (Q2)" item 5 says the priority is not read; that is refuted by
//!   0082eae0 (the entry's score function), and the note should be corrected.
//! - A GoToTarget takes its own perception aggregate (`ai_move::GoToTarget::new`), so no wrapper
//!   is needed. Every other ported behaviour takes an aggregate the same way.

use std::collections::BTreeMap;
use std::path::Path;

use crate::ai_attack;
use crate::ai_brain::{self as brain, AiAttr, Brain, BehaviourEntry, Controller, Ctx, Latch, Latches, Persistent, Root};
use crate::ai_data::{self as data, AiData, BrainRoot};
use crate::ai_move::{self as movement, PathSource};
use crate::ai_perceive::{self as perceive, Aggregate, Link};
use crate::formats::hash::crc32;
use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};

/// The channel of the behaviour types other than WeaponSwitch. [unknown] the game's channels of
/// AttackRanged, AttackMelee, AttackTransform, Idle and Dodge are not in the notes. A channel only
/// matters in the persistent pass, and the data's persistent entries are WeaponSwitch only, which
/// the example checks.
const OTHER_CHANNEL: usize = 0;

/// The channel of the WeaponSwitch entries. [assumed] they share one channel, so the first one in
/// list order that scores runs. Why: the scout's WeaponsOff (disableWeapons, priority 0.10) is a
/// fallback only under that reading. Settled by the channel reader of AIBehaviorWeaponSwitch,
/// which the notes do not give.
pub const WEAPON_CHANNEL: usize = 0;

/// A behaviour type with no port (MoveOnWayPoints, MoveOnSpline, Tactics, Teleport, Interact).
/// [stand-in] Its score is 0, so it is never picked. Its start, update and stop do nothing.
/// `type_hash` is the CRC-32 of the data's type name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unported {
    pub type_hash: u32,
    pub channel: usize,
}

impl brain::Behaviour for Unported {
    fn channel(&self) -> usize {
        self.channel
    }

    fn score(&self, _ctx: &Ctx) -> f32 {
        0.0
    }

    fn start(&mut self, _ctx: &mut Ctx) {}

    fn update(&mut self, _ctx: &mut Ctx, _dt: f32) {}

    fn stop(&mut self, _ctx: &mut Ctx) {}

    fn finished(&self, _ctx: &Ctx) -> bool {
        true
    }

    fn interruptible(&self, _ctx: &Ctx) -> bool {
        true
    }
}

/// How many uses were bound and how many were stand-ins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Count {
    pub bound: usize,
    pub stand_in: usize,
}

/// One brain's outcome: goals and persistent entries that carry a stand-in, and the perceptions
/// of the brain (each use counted). A goal is a stand-in when any part of it is: an Unported
/// behaviour, or a stand-in perception of its own or of its behaviours.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrainReport {
    pub name: String,
    pub goals_bound: usize,
    pub goals_stand_in: usize,
    pub perceptions_bound: usize,
    pub perceptions_stand_in: usize,
    pub persistent_bound: usize,
    pub persistent_stand_in: usize,
}

/// What [`build`] and [`build_brain`] counted. Keys are the data's type names.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub brains: Vec<BrainReport>,
    /// Perception uses by type name (every use: goal, behaviour, persistent, brain).
    pub perceptions: BTreeMap<&'static str, Count>,
    /// Behaviour uses by type name (every use: goal, persistent).
    pub behaviours: BTreeMap<&'static str, Count>,
    /// Why a perception scores 0 for a field the port does not carry, with the number of uses.
    pub reasons: BTreeMap<&'static str, usize>,
    /// The brainList priority of each brain of [`build`], in the list's order. Not applied.
    pub brain_priorities: Vec<f32>,
}

/// One behaviour entry of a goal, for the goal table: its type, static priority and stand-in flag.
#[derive(Clone, Debug, PartialEq)]
pub struct BehaviourRow {
    /// The data's type name of the behaviour.
    pub kind: &'static str,
    pub priority: f32,
    pub stand_in: bool,
}

/// One goal of a brain, for the goal table: its priority, name, and its behaviour entries with
/// their priorities and stand-in flags, in the data's order.
#[derive(Clone, Debug, PartialEq)]
pub struct GoalRow {
    pub priority: f32,
    pub name: String,
    pub behaviours: Vec<BehaviourRow>,
    pub stand_in: bool,
}

fn boxb<T: brain::Behaviour + 'static>(b: T) -> Box<dyn brain::Behaviour> {
    Box::new(b)
}

fn boxp<T: perceive::Perception + 'static>(p: T) -> Box<dyn perceive::Perception> {
    Box::new(p)
}

fn some_hash(v: u32) -> Option<u32> {
    (v != 0).then_some(v)
}

/// Why a perception of the data is a stand-in, or None when the port binds it [stand-in rules of
/// the module note]. The strings are the keys of [`Report::reasons`].
pub fn perception_stand_in(pr: &data::PerceptionRef) -> Option<&'static str> {
    match &pr.perception {
        data::Perception::Compare(c) => {
            if !perceive::value_kind_bound(c.value_type) {
                Some("compare: value kind not described")
            } else if !matches!(c.compare.compare_type, perceive::CMP_ABOVE | perceive::CMP_BELOW) {
                Some("compare: compareType not ported")
            } else {
                None
            }
        }
        data::Perception::Message(_) => None,
        data::Perception::Random(_) | data::Perception::Timer(_) => None,
        data::Perception::Range(r) => {
            (!perceive::value_kind_bound(r.value_type)).then_some("range: value kind not described")
        }
        data::Perception::Targeting(t) => {
            if !perceive::target_type_bound(t.target_data1.target_type) {
                Some("targeting: target type not ported")
            } else if t.compare_with_second_target && !perceive::target_type_bound(t.target_data2.target_type) {
                Some("targeting: second target type not ported")
            } else if !matches!(t.compare.compare_type, perceive::CMP_ABOVE | perceive::CMP_BELOW) {
                Some("targeting: compareType not ported")
            } else {
                None
            }
        }
    }
}

/// Why a behaviour object is a stand-in of its own, or None [stand-in rules of the module note].
pub fn behaviour_stand_in(b: &data::Behaviour) -> Option<&'static str> {
    match b {
        data::Behaviour::MoveOnSpline(_) => Some("MoveOnSpline: no port"),
        data::Behaviour::Teleport(_) => Some("Teleport: no port"),
        _ => None,
    }
}

/// A `weaponIndex` of a WeaponSwitch: true is 1 and false is 0 [assumed] (ai_attack A4); unset
/// is -1, the port's "none".
fn switch_index(w: &data::WeaponIndex) -> i32 {
    match w {
        data::WeaponIndex::Flag(true) => 1,
        data::WeaponIndex::Flag(false) => 0,
        data::WeaponIndex::Hash(_) => -1,
    }
}

fn perceive_compare(c: &data::CompareData) -> perceive::CompareData {
    perceive::CompareData {
        compare_value: c.compare_value,
        compare_value_in_behaviour: c.compare_value_in_behavior,
        assigned: c.assigned_perception.unwrap_or(0),
        compare_type: c.compare_type,
    }
}

fn perceive_spec(t: &data::Target) -> perceive::TargetSpec {
    perceive::TargetSpec {
        target_type: t.target_type,
        group_id: t.target_group_id,
        calc: t.target_calc,
        range_min: t.target_calc_range_min,
        range_max: t.target_calc_range_max,
    }
}

fn movement_spec(t: &data::Target) -> movement::TargetSpec {
    movement::TargetSpec {
        target_type: t.target_type,
        group_id: t.target_group_id,
        calc: t.target_calc,
        range_min: t.target_calc_range_min,
        range_max: t.target_calc_range_max,
    }
}

/// The binding state: the path source factory and the report being filled.
struct Binder<'a> {
    make_path: &'a dyn Fn() -> Box<dyn PathSource>,
    report: &'a mut Report,
    /// The latch nodes of the Message perceptions bound so far (one store per controller).
    latches: &'a mut Latches,
}

impl Binder<'_> {
    fn count_perception(&mut self, name: &'static str, stand_in: bool) {
        let c = self.report.perceptions.entry(name).or_default();
        if stand_in {
            c.stand_in += 1;
        } else {
            c.bound += 1;
        }
    }

    fn count_behaviour(&mut self, name: &'static str, stand_in: bool) {
        let c = self.report.behaviours.entry(name).or_default();
        if stand_in {
            c.stand_in += 1;
        } else {
            c.bound += 1;
        }
    }

    fn reason(&mut self, why: &'static str) {
        *self.report.reasons.entry(why).or_default() += 1;
    }

    /// The perception uses so far: (bound, stand-in).
    fn perception_totals(&self) -> (usize, usize) {
        self.report.perceptions.values().fold((0, 0), |(b, s), c| (b + c.bound, s + c.stand_in))
    }

    /// One perception object for a reference, and whether it is a stand-in ([`perception_stand_in`]).
    /// A stand-in is counted in `reasons`.
    fn perception_object(&mut self, pr: &data::PerceptionRef) -> (Box<dyn perceive::Perception>, bool) {
        let stand_in = perception_stand_in(pr);
        if let Some(why) = stand_in {
            self.reason(why);
        }
        let object: Box<dyn perceive::Perception> = match &pr.perception {
            data::Perception::Compare(c) => boxp(perceive::Compare {
                value_type: c.value_type,
                group_id: c.group_id,
                data: perceive_compare(&c.compare),
            }),
            data::Perception::Message(m) => {
                let id = self.latches.add(Latch {
                    name_on: m.msg_name_on,
                    name_off: m.msg_name_off,
                    timer_init: m.msg_timer,
                    toggle: m.msg_toggle,
                    toggle_on: m.toggle_on,
                    ..Latch::default()
                });
                boxp(perceive::MessagePerception::new(id, m.msg_name_on, m.msg_name_off, m.msg_toggle))
            }
            data::Perception::Random(r) => boxp(perceive::Random { percentage: r.percentage }),
            data::Perception::Range(r) => boxp(perceive::Range {
                value_type: r.value_type,
                group_id: r.group_id,
                min: r.compare_value_min,
                max: r.compare_value_max,
                assigned_min: r.assigned_perception_min.unwrap_or(0),
                assigned_max: r.assigned_perception_max.unwrap_or(0),
            }),
            data::Perception::Targeting(t) => boxp(perceive::Targeting {
                spec: perceive_spec(&t.target_data1),
                spec2: perceive_spec(&t.target_data2),
                compare_with_second: t.compare_with_second_target,
                data: perceive_compare(&t.compare),
            }),
            data::Perception::Timer(t) => boxp(perceive::Timer::new(t.time_on, t.time_off, t.start_on)),
        };
        (object, stand_in.is_some())
    }

    /// One reference as a link: the perception, `msgTrigger` as its message and the data's msgType
    /// as its argument (ai-messages.md section 1 step 1), and the data's invertPriority as the
    /// invert byte (R2).
    fn perception(&mut self, pr: &data::PerceptionRef) -> (Link, bool) {
        let (perception, stand_in) = self.perception_object(pr);
        self.count_perception(pr.perception.type_name(), stand_in);
        (Link { perception, message: pr.msg_trigger, arg: pr.msg_type, invert: pr.invert_priority }, stand_in)
    }

    /// A perception list as an aggregate with the data's calc and condition. Returns whether any
    /// link is a stand-in.
    fn aggregate(&mut self, calc: u32, cond: u32, refs: &[data::PerceptionRef]) -> (Aggregate, bool) {
        let mut links = Vec::with_capacity(refs.len());
        let mut stand_in = false;
        for pr in refs {
            let (link, s) = self.perception(pr);
            stand_in |= s;
            links.push(link);
        }
        (Aggregate::new(calc, cond, links), stand_in)
    }

    /// One behaviour object by its type. Returns it and whether it is a stand-in of its own
    /// (Unported, or Idle with trackTarget, which the port does not track [stand-in]).
    fn behaviour_object(&mut self, b: &data::Behaviour) -> (Box<dyn brain::Behaviour>, bool) {
        let base = b.base();
        let (agg, perception_stand) = self.aggregate(base.priority_calculation, base.priority_condition, &base.perceptions);
        let name = b.type_name();
        let own_stand = behaviour_stand_in(b).is_some();
        let object = match b {
            data::Behaviour::GoToTarget(g) => {
                let path = (self.make_path)();
                boxb(movement::GoToTarget::new(
                    movement_spec(&g.target),
                    g.movement_type,
                    g.attack,
                    g.weapon_index as i32,
                    path,
                    agg,
                ))
            }
            data::Behaviour::AttackRanged(a) => boxb(ai_attack::AttackRanged {
                ignore_attack_delay: a.ignore_attack_delay,
                ignore_kung_fu: a.ignore_kung_fu_fighting,
                weapon_index: a.weapon_index as i32,
                perception: agg,
                channel: OTHER_CHANNEL,
            }),
            data::Behaviour::AttackMelee(m) => boxb(ai_attack::AttackMelee {
                kind: if m.attack_special { ai_attack::BTN_ATTACK_SPECIAL } else { ai_attack::BTN_ATTACK_FAST },
                ignore_attack_delay: m.ignore_attack_delay,
                ignore_kung_fu: m.ignore_kung_fu_fighting,
                perception: agg,
                channel: OTHER_CHANNEL,
            }),
            data::Behaviour::AttackTransform(t) => boxb(ai_attack::AttackTransform {
                kind: t.attack_type,
                ignore_kung_fu: t.ignore_kung_fu_fighting,
                perception: agg,
                channel: OTHER_CHANNEL,
            }),
            data::Behaviour::Idle(i) => boxb(ai_attack::Idle {
                track_target: i.track_target,
                vehicle_mode: i.vehicle_mode,
                weapon_mode: i.weapon_mode,
                perception: agg,
                channel: OTHER_CHANNEL,
            }),
            data::Behaviour::WeaponSwitch(w) => boxb(ai_attack::WeaponSwitch {
                weapon_index: switch_index(&w.weapon_index),
                disable_weapons: w.disable_weapons,
                perception: agg,
                channel: WEAPON_CHANNEL,
            }),
            data::Behaviour::Dodge(d) => boxb(ai_attack::Dodge {
                weights: [d.dodge_left, d.dodge_right, d.dodge_forward, d.dodge_backward],
                dir: 0,
                perception: agg,
                channel: OTHER_CHANNEL,
            }),
            data::Behaviour::MoveOnWayPoints(w) => boxb(movement::MoveOnWayPoints::new(
                w.movement_type,
                w.attack,
                w.weapon_index as i32,
                agg,
            )),
            data::Behaviour::Tactics(t) => boxb(movement::Tactics::new(t.tactics_type, agg)),
            data::Behaviour::Interact(_) => boxb(ai_attack::Interact { perception: agg, channel: 6 }),
            // [stand-in] no port: score 0, never picked. The channels are the vtables' (ai-behaviours-move 10d, 10f).
            data::Behaviour::MoveOnSpline(_) => boxb(Unported { type_hash: crc32(name.as_bytes()), channel: 8 }),
            data::Behaviour::Teleport(_) => boxb(Unported { type_hash: crc32(name.as_bytes()), channel: 11 }),
        };
        self.count_behaviour(name, own_stand);
        (object, own_stand || perception_stand)
    }

    /// A `behaviorList` or `persistentBehaviorList` entry: a behaviour entry with its priority,
    /// hold hash (`rampUpTime`), one-shot flag and messages. The messages' argument is the entry's
    /// msgType [assumed].
    fn behaviour(&mut self, br: &data::BehaviourRef) -> (BehaviourEntry, bool) {
        let (inner, stand_in) = self.behaviour_object(&br.behaviour);
        let mut entry = BehaviourEntry::new(br.priority, inner);
        entry.hold_hash = br.ramp_up_time;
        entry.one_shot = br.one_shot;
        entry.msg_enter = some_hash(br.msg_enter);
        entry.msg_exit = some_hash(br.msg_exit);
        entry.msg_finish = some_hash(br.msg_finish);
        entry.msg_arg = br.msg_type;
        (entry, stand_in)
    }

    /// A `goalList` entry: a goal with the goal's perceptions as its scorer, its messages, and its
    /// behaviour entries (R1).
    fn goal(&mut self, gr: &data::GoalRef) -> (brain::Goal, bool) {
        let g = &gr.goal;
        let (agg, perception_stand) = self.aggregate(g.priority_calculation, g.priority_condition, &g.perceptions);
        let mut behaviours = Vec::with_capacity(g.behaviours.len());
        let mut behaviour_stand = false;
        for br in &g.behaviours {
            let (entry, s) = self.behaviour(br);
            behaviour_stand |= s;
            behaviours.push(entry);
        }
        let mut goal = brain::Goal::new(gr.priority, agg.into_scorer(), behaviours);
        goal.enter_msg = some_hash(gr.msg_enter);
        goal.leave_msg = some_hash(gr.msg_exit);
        // [assumed] the goal's enter and leave messages carry the goalList entry's msgType.
        goal.arg = gr.msg_type;
        (goal, perception_stand || behaviour_stand)
    }

    /// A brain root: its root score, its goals as entries, and its persistent entries.
    fn root(&mut self, b: &BrainRoot) -> Root {
        let (before_bound, before_stand) = self.perception_totals();
        let (agg, _) = self.aggregate(b.priority_calculation, b.priority_condition, &b.perceptions);
        let (mut goals_bound, mut goals_stand_in) = (0, 0);
        let mut entries = Vec::with_capacity(b.goals.len());
        for gr in &b.goals {
            let (entry, stand_in) = self.goal(gr);
            if stand_in {
                goals_stand_in += 1;
            } else {
                goals_bound += 1;
            }
            entries.push(entry);
        }
        let (mut persistent_bound, mut persistent_stand_in) = (0, 0);
        let mut persistent = Vec::with_capacity(b.persistent.len());
        for br in &b.persistent {
            let (behaviour, stand_in) = self.behaviour(br);
            if stand_in {
                persistent_stand_in += 1;
            } else {
                persistent_bound += 1;
            }
            persistent.push(Persistent { entry: behaviour, running: false });
        }
        let (after_bound, after_stand) = self.perception_totals();
        self.report.brains.push(BrainReport {
            name: b.name.clone(),
            goals_bound,
            goals_stand_in,
            perceptions_bound: after_bound - before_bound,
            perceptions_stand_in: after_stand - before_stand,
            persistent_bound,
            persistent_stand_in,
        });
        Root { perceptions: agg.into_scorer(), persistent, brain: Some(Brain::new(entries)) }
    }
}

/// Builds one brain root from its data, and adds its counts to `report`.
/// `latches` gets one node per Message perception of the brain (the controller's store).
pub fn build_brain(
    brain_data: &BrainRoot,
    make_path: &dyn Fn() -> Box<dyn PathSource>,
    report: &mut Report,
    latches: &mut Latches,
) -> Root {
    let mut binder = Binder { make_path, report, latches };
    binder.root(brain_data)
}

/// The goal table of one brain: each goal's priority, name, behaviours and stand-in flags.
pub fn goal_table(brain_data: &BrainRoot, make_path: &dyn Fn() -> Box<dyn PathSource>) -> Vec<GoalRow> {
    let mut scratch = Report::default();
    let mut scratch_latches = Latches::default();
    let mut binder = Binder { make_path, report: &mut scratch, latches: &mut scratch_latches };
    let mut rows = Vec::with_capacity(brain_data.goals.len());
    for gr in &brain_data.goals {
        let (_, stand_in) = binder.goal(gr);
        let mut behaviours = Vec::with_capacity(gr.goal.behaviours.len());
        for br in &gr.goal.behaviours {
            let (_, s) = binder.behaviour(br);
            behaviours.push(BehaviourRow { kind: br.behaviour.type_name(), priority: br.priority, stand_in: s });
        }
        rows.push(GoalRow { priority: gr.priority, name: gr.goal.name.clone(), behaviours, stand_in });
    }
    rows
}

/// The score a brainList entry gives the brain choice: the brain's own aggregate times the entry's
/// priority [game 0082eae0, disasm: `fmul [entry+0x14]` on the score of `entry+0x10`, the brain; 0 when
/// there is no brain]. [assumed] the float at entry+0x14 is the data's `priority` (the loader's store
/// of that field is in 00820d80 / 0082e9a0, not read in full). Without this, a brain with a lower
/// priority ties a higher one at 1.0 and wins by list order (the scout's DComScout at 0.5 beat the
/// Escort brain at 0.75 that way, notes/status.md "Twenty-first pass").
fn weight_root(root: Root, priority: f32) -> Root {
    let inner = root.perceptions;
    Root {
        perceptions: Box::new(move |ctx: &Ctx| inner(ctx) * priority),
        persistent: root.persistent,
        brain: root.brain,
    }
}

/// The brain of a character's brainList, built from the data. `brain_list` holds, in the list's
/// order, each entry's priority and the brain's name hash (`BrainRef::priority`, `BrainRef::brain`).
/// Index 0 of the result is the first brain. The priorities go to [`Report::brain_priorities`]
/// only. Fails when a named brain is not in the data; a stand-in is never an error.
pub fn build(
    data: &AiData,
    brain_list: &[(f32, u32)],
    make_path: &dyn Fn() -> Box<dyn PathSource>,
) -> Result<(Controller, Report), String> {
    let mut report = Report::default();
    let mut latches = Latches::default();
    let mut roots = Vec::with_capacity(brain_list.len());
    for &(priority, hash) in brain_list {
        let brain_data = data.brain_by_hash(hash).ok_or_else(|| format!("brain #{hash:08x} is not in the data"))?;
        let root = build_brain(brain_data, make_path, &mut report, &mut latches);
        roots.push(weight_root(root, priority));
        report.brain_priorities.push(priority);
    }
    Ok((Controller::with_latches(roots, latches), report))
}

/// The AI attributes of a character from its plain `characterAttributes<X>` block (`block`). [data]
/// The keys are the block's own, the same ones `combat::Scout::load_as` reads from it. Field names
/// and offsets: the loader 008289d0 (ai-perceptions third read, A and D). A key the block lacks
/// stays 0.0 [assumed: the loader stores 0.0 for an absent float, third read A].
/// attackRangeFOV is filled into `aim_cone` and dodgeTimer into `dodge_threshold` [assumed]: the
/// names are the loader's, the uses are the older reads' (see the `AiAttr` docs).
/// `auto_facing` is `autoFacing` (the Idle track's turn gate); `accuracy_angle_xy` and
/// `accuracy_angle_z` are `accuracyAngleXY` and `accuracyAngleZ` [assumed: the spread amplitudes of
/// 007b2f00, see AiAttr]. `aim_test`, `weapon_switch`, `idle_aim` and `tactics_default` stay at their
/// default [stand-in]: their names say something else, or none is known.
pub fn attributes(block: Node) -> AiAttr {
    let num = |name: &str| block.get(name).and_then(Node::float).unwrap_or(0.0);
    AiAttr {
        attack_delay: num("attackDelay"),
        aim_cone: num("attackRangeFOV"),
        dodge_threshold: num("dodgeTimer"),
        auto_facing: block.get("autoFacing").and_then(Node::enum_name) == Some("true"),
        accuracy_angle_xy: num("accuracyAngleXY"),
        accuracy_angle_z: num("accuracyAngleZ"),
        alert_distance: num("alertDistance"),
        ambush_distance: num("ambushDistance"),
        attack_counter_distance: num("attackCounterDistance"),
        attack_melee_distance: num("attackMeleeDistance"),
        attack_range_distance: num("attackRangeDistance"),
        attack_range2_distance: num("attackRange2Distance"),
        attack_special_distance: num("attackSpecialDistance"),
        back_pedal_distance: num("backPedalDistance"),
        chase_distance_min: num("chaseDistanceMin"),
        chase_distance_max: num("chaseDistanceMax"),
        dodge_distance: num("dodgeDistance"),
        flee_distance: num("fleeDistance"),
        idle_distance: num("idleDistance"),
        strafe_distance: num("strafeDistance"),
        vehicle_distance: num("vehicleDistance"),
        node_range_max: num("nodeRangeMax"),
        ..AiAttr::default()
    }
}

/// The AI attributes of the character `name` (`DComScout` for `characterAttributesDComScout`),
/// read from `bnxglobal.str` the way `combat::Scout::load_as` reads the same block: the first DATA
/// chunk that has the root.
pub fn load_attributes(dir: &Path, name: &str) -> Result<AiAttr, String> {
    let pack = Pack::open(&dir.join("bnxglobal.str"))?;
    let files: Vec<DataFile> = pack
        .of_type(pack::DATA)
        .filter_map(|c| DataFile::parse(pack.data(c).to_vec()).ok())
        .collect();
    let key = format!("characterAttributes{name}");
    let block = files
        .iter()
        .find_map(|f| f.root().get(&key))
        .ok_or_else(|| format!("{key} missing"))?;
    Ok(attributes(block))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_brain::Request;
    use crate::ai_move::StraightLine;
    use std::path::PathBuf;

    fn install() -> Option<PathBuf> {
        let dir = PathBuf::from(crate::install::dir_string());
        dir.join("bnxglobal.str").is_file().then_some(dir)
    }

    fn straight() -> Box<dyn PathSource> {
        Box::new(StraightLine)
    }

    fn behaviour_base(name: &str) -> data::BehaviourBase {
        data::BehaviourBase {
            name: name.into(),
            hash: crc32(name.as_bytes()),
            perceptions: Vec::new(),
            priority_calculation: perceive::CALC_AVG,
            priority_condition: perceive::COND_REQUIRE,
        }
    }

    fn behaviour_ref(behaviour: data::Behaviour) -> data::BehaviourRef {
        data::BehaviourRef {
            priority: 1.0,
            ramp_up_time: 0,
            ramp_down_time: 0,
            one_shot: false,
            finish_action: 0,
            exit_action: 0,
            msg_enter: 0,
            msg_exit: 0,
            msg_finish: 0,
            msg_type: 0,
            behaviour,
        }
    }

    /// An Idle with weapon mode on: its update pushes a weapon mode request, so a test can see it run.
    fn idle(name: &str) -> data::BehaviourRef {
        behaviour_ref(data::Behaviour::Idle(data::Idle {
            base: behaviour_base(name),
            track_target: false,
            vehicle_mode: false,
            weapon_mode: true,
        }))
    }

    fn timer(name: &str, start_on: bool, invert: bool) -> data::PerceptionRef {
        data::PerceptionRef {
            invert_priority: invert,
            msg_trigger: 0,
            msg_type: 0,
            perception: data::Perception::Timer(data::Timer {
                base: data::PerceptionBase {
                    name: name.into(),
                    hash: crc32(name.as_bytes()),
                    fuzzy: false,
                    min_priority: 0.0,
                    max_priority: 1.0,
                },
                time_on: 1.0,
                time_off: 36.0,
                start_on,
            }),
        }
    }

    fn goal_ref(name: &str, priority: f32, perceptions: Vec<data::PerceptionRef>, behaviours: Vec<data::BehaviourRef>) -> data::GoalRef {
        data::GoalRef {
            priority,
            ramp_up_time: 0,
            ramp_down_time: 0,
            msg_enter: 0,
            msg_exit: 0,
            msg_type: 0,
            goal: data::Goal {
                name: name.into(),
                hash: crc32(name.as_bytes()),
                perceptions,
                priority_calculation: perceive::CALC_AVG,
                priority_condition: perceive::COND_REQUIRE,
                behaviours,
            },
        }
    }

    fn brain_of(name: &str, goals: Vec<data::GoalRef>) -> BrainRoot {
        BrainRoot {
            name: name.into(),
            hash: crc32(name.as_bytes()),
            perceptions: Vec::new(),
            priority_calculation: perceive::CALC_AVG,
            priority_condition: perceive::COND_REQUIRE,
            goals,
            persistent: Vec::new(),
        }
    }

    /// Two goals: the high one (priority 1.0) gated by a timer that holds or not, the low one
    /// (0.5) gated by a timer that holds.
    fn two_goals(high_holds: bool, high_inverted: bool) -> AiData {
        let high = goal_ref("goalHigh", 1.0, vec![timer("perceptionHigh", high_holds, high_inverted)], vec![idle("behaviorHigh")]);
        let low = goal_ref("goalLow", 0.5, vec![timer("perceptionLow", true, false)], vec![idle("behaviorLow")]);
        AiData { brains: vec![brain_of("brainTest", vec![high, low])], ..AiData::default() }
    }

    /// Builds the one brain, starts it, and ticks once. Returns the picked entry index.
    fn run_once(data: &AiData) -> (Option<usize>, Vec<Request>) {
        let make = || -> Box<dyn PathSource> { straight() };
        let hash = crc32(b"brainTest");
        let (mut ctl, _) = build(data, &[(1.0, hash)], &make).expect("build");
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);
        let _ = ctl.tick(&mut ctx, 0.1);
        (ctl.cur.goal, ctx.requests.clone())
    }

    #[test]
    fn tick_picks_the_higher_goal_when_its_perception_holds_else_the_lower() {
        let (picked, requests) = run_once(&two_goals(true, false));
        assert_eq!(picked, Some(0), "the high goal's timer holds: it is picked");
        assert!(!requests.is_empty(), "its Idle pushed a weapon mode request");

        let (picked, requests) = run_once(&two_goals(false, false));
        assert_eq!(picked, Some(1), "the high goal's timer does not hold: the low goal is picked");
        assert!(!requests.is_empty(), "the low goal's Idle ran");
    }

    #[test]
    fn an_inverted_link_scores_zero_so_its_goal_is_not_picked() {
        // The high goal's timer holds; inverted (R2), it scores 0, so the low goal runs.
        let (picked, _) = run_once(&two_goals(true, true));
        assert_eq!(picked, Some(1));
    }

    #[test]
    fn an_inverted_link_passes_when_its_perception_fails() {
        // The high goal's timer does not hold; inverted (R2), it scores 1, so the high goal runs.
        let (picked, _) = run_once(&two_goals(false, true));
        assert_eq!(picked, Some(0));
    }

    #[test]
    fn an_unported_behaviour_scores_zero_and_its_goal_does_nothing() {
        let spline = behaviour_ref(data::Behaviour::MoveOnSpline(data::MoveOnSpline {
            base: behaviour_base("behaviorMoveOnSpline"),
            movement_type: 0,
            use_auto_movement: false,
            attack: false,
            weapon_index: 0,
            spline_movement_type: 0,
            patrol: false,
        }));
        let data = AiData {
            brains: vec![brain_of("brainTest", vec![goal_ref("goalSpline", 1.0, Vec::new(), vec![spline])])],
            ..AiData::default()
        };
        let make = || -> Box<dyn PathSource> { straight() };
        let (mut ctl, report) = build(&data, &[(1.0, crc32(b"brainTest"))], &make).expect("build");
        assert_eq!(report.behaviours.get("AIBehaviorMoveOnSpline"), Some(&Count { bound: 0, stand_in: 1 }));
        assert_eq!(report.brains[0].goals_stand_in, 1);
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);
        let _ = ctl.tick(&mut ctx, 0.1);
        assert!(ctx.requests.is_empty(), "the unported behaviour pushes nothing");
        let goal = &ctl.brains[0].brain.as_ref().expect("brain").goals[0];
        assert_eq!(goal.behaviours[0].behaviour.score(&ctx), 0.0, "the unported behaviour scores 0");
    }

    #[test]
    fn a_build_for_an_unknown_brain_is_an_error() {
        let data = two_goals(true, false);
        let make = || -> Box<dyn PathSource> { straight() };
        assert!(build(&data, &[(1.0, 0x12345678)], &make).is_err());
    }

    /// Every brain root of the install builds, and the scout's goal table has its five goals.
    #[test]
    fn all_59_roots_build_from_the_install() {
        let Some(dir) = install() else { return };
        let data = ai_data_load(&dir);
        let make = || -> Box<dyn PathSource> { straight() };
        let mut report = Report::default();
        let mut latches = Latches::default();
        let roots: Vec<Root> = data.brains.iter().map(|b| build_brain(b, &make, &mut report, &mut latches)).collect();
        assert_eq!(roots.len(), 59, "brain roots");
        assert_eq!(report.brains.len(), 59);
        let scout = data.brain("brainDComScout").expect("brainDComScout");
        assert_eq!(goal_table(scout, &make).len(), 5);
    }

    fn ai_data_load(dir: &std::path::Path) -> AiData {
        data::load(dir).expect("bnxglobal.str chunk 16")
    }

    /// The scout's 15 distances and its attack delay, from its plain block (global_states.txt
    /// 53426..53440 [data]). The third read said the block has none of the keys; it has them.
    #[test]
    fn scout_attributes_come_from_its_block() {
        let Some(dir) = install() else { return };
        let a = load_attributes(&dir, "DComScout").expect("characterAttributesDComScout");
        assert_eq!(a.alert_distance, 300.0);
        assert_eq!(a.ambush_distance, 30.0);
        assert_eq!(a.attack_counter_distance, 7.0);
        assert_eq!(a.attack_melee_distance, 7.0);
        assert_eq!(a.attack_range_distance, 500.0);
        assert_eq!(a.attack_range2_distance, 100.0);
        assert_eq!(a.attack_special_distance, 7.0);
        assert_eq!(a.back_pedal_distance, 15.0);
        assert_eq!(a.chase_distance_min, 25.0);
        assert_eq!(a.chase_distance_max, 2000.0);
        assert_eq!(a.dodge_distance, 100.0);
        assert_eq!(a.flee_distance, 160.0);
        assert_eq!(a.idle_distance, 50.0);
        assert_eq!(a.strafe_distance, 0.0);
        assert_eq!(a.vehicle_distance, 60.0);
        assert_eq!(a.attack_delay, 2.0);
        assert_eq!(a.dodge_threshold, 12.0, "dodgeTimer [assumed use]");
        assert!(a.auto_facing, "autoFacing true (global_states.txt 53418)");
        assert_eq!(a.accuracy_angle_xy, 5.0, "accuracyAngleXY (53420)");
        assert_eq!(a.accuracy_angle_z, 3.0, "accuracyAngleZ (53421)");
        assert!(!a.aim_test && !a.weapon_switch && !a.idle_aim, "[stand-in] left at default");
    }
}
