//! The AI movement behaviour GoToTarget (phase 3). It walks the character toward a destination by
//! writing a horizontal stick direction into `Ctx::stick`, and pushes the attack-while-moving
//! requests. Source: notes/ai-behaviours-move.md (sections 0 to 6 and its assumptions A1 to A6).
//! Tags as in ai_brain.rs. Not wired into sim.rs or the game yet.
//!
//! The far-target node route for targets over 200 m (section 1 step 7) and MoveOnWayPoints' node
//! chain run on `Ctx::nodes` (ai_node, notes/ai-nodes.md) when the level's nodes are loaded.
//!
//! Not ported here (each is marked where it is left out):
//!   - the path core (006f2060, 006ee6b0, 006ff580): `PathSource`, with a straight line for now;
//!   - the lead distance for the moving-target types (section 1 step 5);
//!   - the flyer and waypoint MOVE branches 0082a050 and 00829ea0, and the auto-movement gate
//!     00825db0 (partly read): the MOVE kind is never chosen here;
//!   - the climb rule of the steer (ctrl+0xc8, section 3a step 1), and char+0x310;
//!   - the perception score of the entry (0081d650 tail, ledger only): `score` is a stand-in;
//!   - the HOME_REGION gate FUN_00824650 (section 2c).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::ai_attack::BTN_WEAPON_FIRE;
use crate::ai_brain::{Behaviour, Ctx, Request, TARGET_GROUP};
use crate::ai_node::{self, NodeSearch, CALC_NEAREST, NODE_JUMP, NODE_WAYPOINT};
use crate::ai_perceive::Aggregate;

/// TARGET_FOE [data guess, notes/ai-behaviours-move.md section 8 item 2]: the foe's point.
pub const TARGET_FOE: u32 = 0x081b1ccb;
/// TARGET_HOME_REGION, -0x57187f4f as a 32-bit word [data guess, section 8 item 2].
pub const TARGET_HOME_REGION: u32 = 0xa8e780b1;
/// TARGET_HOME_POINT [data guess, section 8 item 2]: the home point.
pub const TARGET_HOME_POINT: u32 = 0x55f255e0;
/// TARGET_TURRET [data guess, section 8 item 2]. Not named in the code here: it takes the own
/// position fallback like any other type without a point (section 2d).
pub const TARGET_TURRET: u32 = 0x2919a3ec;
/// TARGET_FRIEND, the CRC-32 of the name "TARGET_FRIEND" [assumed]: the same convention as the three
/// guesses above (their CRCs match the names); no data name or use settles it (names.tsv has none).
pub const TARGET_FRIEND: u32 = 0xb51570a8;

/// The MOVE sentinel returned by ai_goto_kind [game 0081eb50]. Not chosen by this port (see the
/// module note on the auto-movement gate).
pub const KIND_MOVE: u32 = 0xb926e5d7;
/// The attack movement kind 0x770a2b36 [game 0081d298 path, 0082a8a0 step 6]. [unknown] its name.
pub const KIND_ATTACK: u32 = 0x770a2b36;
/// The second attack movement kind 0xe0f54be7 [game 0082a8a0 step 6]. [unknown] its name.
pub const KIND_ATTACK_ALT: u32 = 0xe0f54be7;

/// The arrival ring, metres [game 0081dbc0].
const RING: f32 = 2.0;
/// The arrival ring while this behaviour is the current node, metres: hysteresis [game 0081dbc0;
/// the meaning of "current node" is [assumed], notes section 5 and A2].
const RING_CURRENT: f32 = 1.6;
/// The steer writes nothing inside this distance, squared: 0.2 m [game 0082a8a0 step 3, 0.04].
const STOP_SQ: f32 = 0.04;
/// The direct branch counts the point as reached inside this distance, squared: 0.5 m [game
/// 0081ccb0 lines 262-273, 0.25].
const DIRECT_REACHED_SQ: f32 = 0.25;
/// The path is asked for when the character is more than this far, squared: 2.0 m [game 0081ccb0
/// line 134, 4.0].
const PATH_FAR_SQ: f32 = 4.0;

/// The path source: the next point to steer at on the way from `from` to `to`. The game's path
/// query (ctrl+0xf0, notes section 4) is behind this trait.
pub trait PathSource: Send + Sync {
    /// The next waypoint, or None when no path is found (section 6: a straight walk then).
    fn next_point(&mut self, from: [f32; 3], to: [f32; 3]) -> Option<[f32; 3]>;
    /// Called once per controller step (`Behaviour::new_frame`). Empty by default.
    fn new_frame(&mut self) {}
}

/// A straight line to the destination. [stand-in] the game's path core is not ported: every
/// query returns the destination itself, so there is no obstacle avoidance.
pub struct StraightLine;

impl PathSource for StraightLine {
    fn next_point(&mut self, _from: [f32; 3], to: [f32; 3]) -> Option<[f32; 3]> {
        Some(to)
    }
}

/// The target spec of the behaviour (`beh+0x34`, notes section 8 item 2). The offsets are
/// [assumed] from the loader (A1). Types of `calc` and the range units are [unknown].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetSpec {
    /// targetType, `beh+0x38`: a name hash, see the TARGET_ constants [data].
    pub target_type: u32,
    /// targetGroupID, `beh+0x3c` [data]. Not used by this port.
    pub group_id: u32,
    /// targetCalc, `beh+0x40` [data]. Not used by this port.
    pub calc: u32,
    /// rangeMin, `beh+0x44` [data]. Not used by this port. [unknown] units.
    pub range_min: f32,
    /// rangeMax, `beh+0x48` [data]. Not used by this port. [unknown] units.
    pub range_max: f32,
}

/// GoToTarget (vtable 00b61c54) [game]. Channel 4 and always interruptible (section 0).
pub struct GoToTarget {
    /// movementType, `beh+0x30` [data]: the kind the attack part checks.
    pub movement_type: u32,
    /// The attack flag, `beh+0x29` [data]. [assumed] it gates the attack-while-moving requests:
    /// its reader (ai_goto_attack_ready 0081eb90) is not read (A9).
    pub attack: bool,
    /// weaponIndex, `beh+0x2c`, -1 for none [data]: the weapon mode request uses it.
    pub weapon_index: i32,
    /// The target spec (`beh+0x34`).
    pub target_data: TargetSpec,
    /// The behaviour's own perception aggregate (its score outside the ring) [game 0081d650 tail;
    /// the links are the data's behaviour perceptionList].
    pub perception: Aggregate,
    /// True when this behaviour is the controller's current node: the arrival ring is 1.6 m then.
    /// The caller sets it (A2). It is a field here, not a Ctx flag, so Ctx stays as it was.
    pub current_node: bool,
    path: Box<dyn PathSource>,
    /// The sticky in-ring result (`beh+0x60`, FINISHED 0081d700 [game]). Set by `finished`; once
    /// true it is returned without recomputing. Reset in `start` [assumed], see A7.
    arrived: AtomicBool,
    /// The destination of the last path request (`ctrl` copy of the point, section 1 step 6).
    /// Cleared by `stop`, which drops the path (A8).
    last_dest: Option<[f32; 3]>,
    /// `controller+0x1a8`: the node of the far route steered at last (ai_node::far_pick) [game].
    far_node: Option<usize>,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// The squared 3D length. [assumed] the distances are 3D (the note says 2D or 3D is not settled;
/// A10).
fn len_sq(v: [f32; 3]) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

/// The shared attack gate 00824a00 [game, decompile]. The same test as ai_attack's private
/// `shared_attack_gate`, repeated here because that one is private. The ignore flag is passed as
/// false [assumed]: the argument of the call from the steer is not read (A11).
fn attack_gate(ctx: &Ctx) -> bool {
    if ctx.attr.attack_delay > ctx.attack_clock {
        return false;
    }
    if ctx.weapons_disabled {
        return false;
    }
    match ctx.target {
        Some(t) => (0.0..=5000.0).contains(&t.distance),
        None => false,
    }
}

/// The attack-while-moving requests of ai_goto_steer 0082a8a0 step 6 [game]: a weapon mode request
/// with the weapon index, then, when the attack gate passes, a fire request. The fire request follows
/// ai_attack's fire rule (refused when input is blocked, in a vehicle or with weapons disabled; not
/// pressed again while held). [assumed] the kind passed to ai_attack_request 008297e0 from here is
/// the weapon fire button 0x17 (ai_attack's ranged kind): not read (A12).
fn attack_while_moving(ctx: &mut Ctx, weapon_index: i32) {
    ctx.requests.push(Request::WeaponMode(weapon_index));
    if !attack_gate(ctx) {
        return;
    }
    if ctx.input_blocked || ctx.me.in_vehicle || ctx.weapons_disabled {
        return;
    }
    if ctx.is_held(BTN_WEAPON_FIRE) {
        return;
    }
    ctx.requests.push(Request::Button(BTN_WEAPON_FIRE));
}

/// The attack movement kinds of ai_goto_steer step 6 (KIND_ATTACK, KIND_ATTACK_ALT).
fn is_attack_kind(movement_type: u32) -> bool {
    movement_type == KIND_ATTACK || movement_type == KIND_ATTACK_ALT
}

impl GoToTarget {
    /// A GoToTarget with its target spec, movement kind, attack flag, weapon index, path source and
    /// perception aggregate.
    pub fn new(
        target_data: TargetSpec,
        movement_type: u32,
        attack: bool,
        weapon_index: i32,
        path: Box<dyn PathSource>,
        perception: Aggregate,
    ) -> Self {
        Self {
            movement_type,
            attack,
            weapon_index,
            target_data,
            perception,
            current_node: false,
            path,
            arrived: AtomicBool::new(false),
            last_dest: None,
            far_node: None,
        }
    }

    /// The far-target branch of 0081ccb0 [game], run when the path is asked: with the level's
    /// nodes and a destination farther than 200 m (400 m in a car), the A* node route 0082cfd0 and
    /// its first node farther than 100 m (400 m in a car) from the character, starting at the node
    /// picked last. That node's place is steered at instead of the destination (in a car with the
    /// destination's height [assumed]: the game keeps a stored z, local_70, whose source is not
    /// settled). Otherwise the remembered node is cleared and the destination is kept.
    /// [assumed] "in a car" (is_in_car_type 00725890) is `SelfState::in_vehicle`.
    fn far_target(&mut self, ctx: &Ctx, own: [f32; 3], dest: [f32; 3]) -> [f32; 3] {
        let Some(nodes) = ctx.nodes.as_ref() else { return dest };
        let in_car = ctx.me.in_vehicle;
        let limit = if in_car { ai_node::FAR_CAR } else { ai_node::FAR_TARGET_FOOT };
        if nodes.is_empty() || len_sq(sub(dest, own)).sqrt() <= limit {
            self.far_node = None;
            return dest;
        }
        let route = nodes.route(own, dest);
        if route.nodes.is_empty() {
            self.far_node = None;
            return dest;
        }
        match nodes.far_pick(&route.nodes, self.far_node, own, in_car) {
            Some(n) => {
                self.far_node = Some(n);
                let p = nodes.pos(n);
                if in_car {
                    [p[0], p[1], dest[2]]
                } else {
                    p
                }
            }
            None => {
                self.far_node = None;
                dest
            }
        }
    }

    /// The destination point (section 2d). FOE is the tactics point when the tactics record is valid
    /// (`Ctx::tactics_point`), else the own position [game rule, ai-behaviours-move.md section 10;
    /// the integrator fills the point]. HOME_POINT and HOME_REGION are `Ctx::home`. Other types take
    /// the own position (A4). None when the point does not exist: that counts as arrived (section 5).
    fn destination(&self, ctx: &Ctx) -> Option<[f32; 3]> {
        match self.target_data.target_type {
            TARGET_FOE => Some(ctx.tactics_point.unwrap_or(ctx.me.pos)),
            // [stand-in] HOME_REGION also needs FUN_00824650 (section 2c), not read: a set home
            // point is taken as valid.
            TARGET_HOME_POINT | TARGET_HOME_REGION => ctx.home,
            // The group target's place (targetGroupID, the group's nearest live member) [assumed: the
            // point of a group target is its own place, as FOE's is the tactics point]. With no group
            // target of that group, the own position (the A4 fallback), as the other types.
            TARGET_GROUP => Some(ctx.senses.group_target_point(self.target_data.group_id).unwrap_or(ctx.me.pos)),
            _ => Some(ctx.me.pos),
        }
    }

    /// The arrival test 0081dbc0 [game]: arrived when inside the ring (2.0 m, or 1.6 m as the
    /// current node). No valid point counts as arrived [game: the early exits return 1].
    fn in_ring(&self, ctx: &Ctx) -> bool {
        match self.destination(ctx) {
            None => true,
            Some(p) => {
                let r = if self.current_node { RING_CURRENT } else { RING };
                len_sq(sub(p, ctx.me.pos)) < r * r
            }
        }
    }

    /// ai_goto_steer 0082a8a0 [game] for one point. Inside 0.2 m it writes nothing and returns
    /// before the requests (step 3). Otherwise the attack requests run for the attack kinds (step
    /// 6, with the attack flag as A9 says), then the unit horizontal direction to the point is
    /// written (steps 4 and 7). A zero horizontal part writes no direction. The stick is WORLD
    /// space; the frame conversion of 00822260 is the integrator's job.
    fn steer(&self, ctx: &mut Ctx, point: [f32; 3]) {
        steer_to(ctx, point, is_attack_kind(self.movement_type) && self.attack, self.weapon_index);
    }
}

/// The steer of 0082a8a0 for one point, shared by GoToTarget and MoveOnWayPoints [game]: nothing
/// inside 0.2 m; the attack requests when `attack_requests`; then the unit horizontal direction.
fn steer_to(ctx: &mut Ctx, point: [f32; 3], attack_requests: bool, weapon_index: i32) {
    let d = sub(point, ctx.me.pos);
    if len_sq(d) <= STOP_SQ {
        return;
    }
    if attack_requests {
        attack_while_moving(ctx, weapon_index);
    }
    let h2 = d[0] * d[0] + d[1] * d[1];
    if h2 > 0.0 {
        let h = h2.sqrt();
        ctx.stick = [d[0] / h, d[1] / h];
    }
}

/// AIBehaviorMoveOnWayPoints (8 objects in the data) [game, ai-behaviours-move.md 10b]. It walks the
/// path records the integrator fills in `Ctx::waypoints`, one at a time: the current record is
/// `index`, and it advances once a step when the character is within 0.5 m of it [game 0081f980
/// step 2]. Score 0 with no record; finished when the records are used up. Channel 9.
pub struct MoveOnWayPoints {
    /// movementType, `beh+0x30` [data].
    pub movement_type: u32,
    /// The attack flag (`beh+0x29`) [data]; see GoToTarget.
    pub attack: bool,
    /// weaponIndex, `beh+0x2c`, -1 for none [data].
    pub weapon_index: i32,
    /// The behaviour's perception aggregate [data links].
    pub perception: Aggregate,
    /// [stand-in] without `Ctx::nodes`: the current record as an index into `Ctx::waypoints`, set to
    /// 0 by start.
    index: usize,
    /// With `Ctx::nodes`: the current record (`controller+0x1ac`), a node index, NO_NODE when none.
    /// Atomic because the score advances it (0081fb40 writes controller+0x1ac).
    node: AtomicUsize,
}

/// No node held (controller+0x1ac == 0).
const NO_NODE: usize = usize::MAX;
/// MoveOnWayPoints takes the next node within 0.5 m (squared 0.25). [game 0081fb40, 0081f980]
const NODE_REACHED_SQ: f32 = 0.25;

impl MoveOnWayPoints {
    pub fn new(movement_type: u32, attack: bool, weapon_index: i32, perception: Aggregate) -> Self {
        Self {
            movement_type,
            attack,
            weapon_index,
            perception,
            index: 0,
            node: AtomicUsize::new(NO_NODE),
        }
    }

    fn current(&self) -> Option<usize> {
        let n = self.node.load(Ordering::Relaxed);
        (n != NO_NODE).then_some(n)
    }

    fn set_current(&self, n: Option<usize>) {
        self.node.store(n.unwrap_or(NO_NODE), Ordering::Relaxed);
    }
}

impl Behaviour for MoveOnWayPoints {
    fn channel(&self) -> usize {
        9
    }

    /// Score 0081fb40 [game]: 0 with no record; else, within 0.5 m of the record and when it is not
    /// a jump node, the record moves on to the next linked waypoint (which may be none); then the
    /// perception aggregate. [stand-in] the vehicle branch (in a car, farther than 0.5 m, a spline
    /// band test 0.9 < f < 1.9 through 00825940 and 0059c140) is not ported: a car moves on only
    /// within 0.5 m. Without `Ctx::nodes` the stand-in record list `Ctx::waypoints` is used.
    fn score(&self, ctx: &Ctx) -> f32 {
        let Some(nodes) = ctx.nodes.as_ref() else {
            if ctx.waypoints.is_empty() {
                return 0.0;
            }
            return self.perception.score(ctx);
        };
        let Some(cur) = self.current() else { return 0.0 };
        if len_sq(sub(nodes.pos(cur), ctx.me.pos)) < NODE_REACHED_SQ && nodes.nodes[cur].data.flags & NODE_JUMP == 0 {
            self.set_current(nodes.next_waypoint(cur));
        }
        self.perception.score(ctx)
    }

    fn enter(&mut self, ctx: &mut Ctx) {
        self.perception.enter(ctx);
    }

    fn leave(&mut self, ctx: &mut Ctx) {
        self.perception.leave(ctx);
    }

    /// Start 0081fc60 [game]: the record is the nearest active waypoint node within nodeRangeMax
    /// (`AiAttr::node_range_max`, 500 in the data) of the character (0082dbe0, CALC_NEAREST, band 0
    /// to nodeRangeMax, required NODE_WAYPOINT). [assumed] the search point, the camera target point
    /// 0071a2e0, is the own place. The writes to controller+0x6a8 and +0x68c are not ported.
    /// Without `Ctx::nodes`: [stand-in] index 0 of `Ctx::waypoints`.
    fn start(&mut self, ctx: &mut Ctx) {
        self.index = 0;
        if let Some(nodes) = ctx.nodes.as_ref() {
            let spec = NodeSearch { calc: CALC_NEAREST, min: 0.0, max: ctx.attr.node_range_max };
            self.set_current(nodes.find(ctx.me.pos, spec, NODE_WAYPOINT, 0, None));
        }
    }

    /// Update 0081f980 [game]. With `Ctx::nodes`: at a jump node and within 0.5 m, the next linked
    /// waypoint becomes the record and is steered at (0082a810: the steer with kind 0xe0f54be7);
    /// none: nothing. Otherwise (any node farther, or not a jump node) the steer at the record.
    /// [stand-in] the jump steer's cone test 00723c30 (200 m, 15 degrees), its StateJump return and
    /// the device flag 0x40 it sets are not ported. Without `Ctx::nodes` [stand-in]: within 0.5 m
    /// of the current point of `Ctx::waypoints`, the next one; then the steer at it.
    fn update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let own = ctx.me.pos;
        if let Some(nodes) = ctx.nodes.clone() {
            let Some(cur) = self.current() else { return };
            let here = nodes.pos(cur);
            if nodes.nodes[cur].data.flags & NODE_JUMP != 0 && len_sq(sub(here, own)) < NODE_REACHED_SQ {
                let Some(next) = nodes.next_waypoint(cur) else { return };
                steer_to(ctx, nodes.pos(next), self.attack, self.weapon_index);
                self.set_current(Some(next));
                return;
            }
            steer_to(ctx, here, is_attack_kind(self.movement_type) && self.attack, self.weapon_index);
            return;
        }
        if let Some(&here) = ctx.waypoints.get(self.index) {
            if len_sq(sub(here, own)) < DIRECT_REACHED_SQ {
                self.index += 1;
            }
        }
        if let Some(&point) = ctx.waypoints.get(self.index) {
            steer_to(ctx, point, is_attack_kind(self.movement_type) && self.attack, self.weapon_index);
        }
    }

    /// Stop 0081eb00 [game for the flags it resets]: the record is kept, so a later start restarts.
    fn stop(&mut self, _ctx: &mut Ctx) {}

    /// Finished 0081fd10 [game]: 1 when the record is cleared, here when no record is left.
    fn finished(&self, ctx: &Ctx) -> bool {
        if ctx.nodes.is_some() {
            return self.current().is_none();
        }
        self.index >= ctx.waypoints.len()
    }

    /// Target-valid 0081fd30 [game]: 1 when a record is held.
    fn target_valid(&self, ctx: &Ctx) -> bool {
        if ctx.nodes.is_some() {
            return self.current().is_some();
        }
        self.index < ctx.waypoints.len()
    }

    fn interruptible(&self, _ctx: &Ctx) -> bool {
        true
    }
}

/// AIBehaviorTactics (5 objects in the data) [game, ai-behaviours-move.md 10c]. Its update writes the
/// tactics type into `Ctx::tactics_type` (controller+0x694); its stop restores the default from
/// `AiAttr::tactics_default`. It does not steer and does not compute a tactics point. Channel 10.
pub struct Tactics {
    /// tacticsType [data]. Its name is not known (`ai_data::Tactics`).
    pub tactics_type: u32,
    pub perception: Aggregate,
}

impl Tactics {
    pub fn new(tactics_type: u32, perception: Aggregate) -> Self {
        Self { tactics_type, perception }
    }
}

impl Behaviour for Tactics {
    fn channel(&self) -> usize {
        10
    }

    /// Score 0082e7e0 [game]: the perception aggregate.
    fn score(&self, ctx: &Ctx) -> f32 {
        self.perception.score(ctx)
    }

    fn enter(&mut self, ctx: &mut Ctx) {
        self.perception.enter(ctx);
    }

    fn leave(&mut self, ctx: &mut Ctx) {
        self.perception.leave(ctx);
    }

    /// Start 00580aa0: no-op [game].
    fn start(&mut self, _ctx: &mut Ctx) {}

    /// Update 0081fea0 [game]: writes the tactics type.
    fn update(&mut self, ctx: &mut Ctx, _dt: f32) {
        ctx.tactics_type = self.tactics_type;
    }

    /// Stop 0081fec0 [game]: restores the character's default (`AiAttr::tactics_default`).
    fn stop(&mut self, ctx: &mut Ctx) {
        ctx.tactics_type = ctx.attr.tactics_default;
    }

    /// Finished 0076c530: always 1 [game].
    fn finished(&self, _ctx: &Ctx) -> bool {
        true
    }

    /// Target-valid 0076c530: always 1 [game].
    fn target_valid(&self, _ctx: &Ctx) -> bool {
        true
    }

    fn interruptible(&self, _ctx: &Ctx) -> bool {
        true
    }
}

impl Behaviour for GoToTarget {
    fn channel(&self) -> usize {
        4
    }

    fn new_frame(&mut self) {
        self.path.new_frame();
    }

    /// The score (vtable 0081d650) [game for the ring; the tail's use of the aggregate is [assumed]
    /// from the ledger]: 0 inside the arrival ring, else the behaviour's perception aggregate.
    fn score(&self, ctx: &Ctx) -> f32 {
        if self.in_ring(ctx) {
            0.0
        } else {
            self.perception.score(ctx)
        }
    }

    fn enter(&mut self, ctx: &mut Ctx) {
        self.perception.enter(ctx);
    }

    fn leave(&mut self, ctx: &mut Ctx) {
        self.perception.leave(ctx);
    }

    /// START 0081d7b0 [game for the flags it sets; the clear of the arrived result is [assumed]
    /// (A7): the game's start does not show one].
    fn start(&mut self, _ctx: &mut Ctx) {
        self.arrived.store(false, Ordering::Relaxed);
    }

    /// UPDATE 0081ccb0 [game], the parts ported. Order as the note gives it:
    /// 1. an arrived result (sticky) returns with no path and no steer (step 4);
    /// 2. no point (no valid target) returns: arrived (step 5);
    /// 3. the path is asked only when the destination changed or the character is more than 2.0 m
    ///    away (step 7). A waypoint is steered at. When no path is asked, or none is found, the
    ///    direct branch steers at the destination, and it counts as reached inside 0.5 m (step 8,
    ///    section 6).
    fn update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if self.arrived.load(Ordering::Relaxed) {
            return;
        }
        let Some(dest) = self.destination(ctx) else {
            return;
        };
        let own = ctx.me.pos;
        let changed = self.last_dest != Some(dest);
        self.last_dest = Some(dest);
        let path_asked = changed || len_sq(sub(dest, own)) > PATH_FAR_SQ;
        // [stand-in] the lead distance for the moving-target types (step 5, A3) is not ported: the
        // point is the target's own point. The far-target node route (step 7) needs `Ctx::nodes`.
        let aim = if path_asked { self.far_target(ctx, own, dest) } else { dest };
        let waypoint = if path_asked {
            self.path.next_point(own, aim)
        } else {
            None
        };
        match waypoint {
            Some(p) => self.steer(ctx, p),
            None => {
                if len_sq(sub(aim, own)) <= DIRECT_REACHED_SQ {
                    return;
                }
                self.steer(ctx, aim);
            }
        }
    }

    /// STOP 0081d8a0 [game for the flags it sets]. [assumed] the path is dropped (its node list is
    /// cleared there, section 2b), so the next start asks for a new one (A8). The stick is not
    /// touched: the integrator clears it.
    fn stop(&mut self, _ctx: &mut Ctx) {
        self.last_dest = None;
        self.far_node = None;
    }

    /// FINISHED 0081d700 [game]: the in-ring result is computed once (when not yet set) and then
    /// kept (sticky). No valid target counts as arrived.
    fn finished(&self, ctx: &Ctx) -> bool {
        if !self.arrived.load(Ordering::Relaxed) {
            self.arrived.store(self.in_ring(ctx), Ordering::Relaxed);
        }
        self.arrived.load(Ordering::Relaxed)
    }

    /// Vtable +0x30 is `mov al,1; ret 4` [game 0076c530]: always interruptible.
    fn interruptible(&self, _ctx: &Ctx) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_brain::Target;
    use std::sync::atomic::AtomicU32;
    use std::sync::Arc;

    fn spec(target_type: u32) -> TargetSpec {
        TargetSpec {
            target_type,
            group_id: 0,
            calc: 0,
            range_min: 0.0,
            range_max: 0.0,
        }
    }

    fn walker(target_type: u32) -> GoToTarget {
        GoToTarget::new(spec(target_type), 0, false, -1, Box::new(StraightLine), pass())
    }

    /// An aggregate with no links: score 1.
    fn pass() -> Aggregate {
        Aggregate::new(crate::ai_perceive::CALC_AVG, 0, Vec::new())
    }

    /// A foe at `pos`. The FOE destination is the tactics point, which these tests set to the foe's
    /// point (the integrator's stand-in when there is no nav mesh).
    fn foe_at(ctx: &mut Ctx, pos: [f32; 3]) {
        let d = len_sq(sub(pos, ctx.me.pos)).sqrt();
        ctx.target = Some(Target {
            pos,
            distance: d,
            team: 0,
        });
        ctx.tactics_point = Some(pos);
    }

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5
    }

    /// Counts the path requests it receives.
    struct Counting(Arc<AtomicU32>);

    impl PathSource for Counting {
        fn next_point(&mut self, _from: [f32; 3], to: [f32; 3]) -> Option<[f32; 3]> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Some(to)
        }
    }

    /// A path source that finds nothing.
    struct NoPath;

    impl PathSource for NoPath {
        fn next_point(&mut self, _from: [f32; 3], _to: [f32; 3]) -> Option<[f32; 3]> {
            None
        }
    }

    /// A group-targeted movement (targetType TARGET_GROUP, targetGroupID Escort) walks to the group
    /// target's place, not to the enemy's own place. With no target of that group it stays put.
    #[test]
    fn group_target_destination_is_the_group_targets_place() {
        let escort = crate::formats::hash::crc32(b"Escort");
        let mut ctx = Ctx::new();
        ctx.senses.group_target_points = vec![(escort, [0.0, 50.0, 0.0])];
        let mut g = GoToTarget::new(
            TargetSpec { target_type: TARGET_GROUP, group_id: escort, calc: 0, range_min: 0.0, range_max: 0.0 },
            0,
            false,
            -1,
            Box::new(StraightLine),
            pass(),
        );
        assert_eq!(g.destination(&ctx), Some([0.0, 50.0, 0.0]), "the convoy's place, not the enemy's");
        assert!(!g.in_ring(&ctx), "50 m away is outside the ring");
        g.update(&mut ctx, 0.032);
        assert!(near(ctx.stick, [0.0, 1.0]), "walks toward +y, got {:?}", ctx.stick);

        let none = Ctx::new();
        let other = GoToTarget::new(
            TargetSpec { target_type: TARGET_GROUP, group_id: escort, calc: 0, range_min: 0.0, range_max: 0.0 },
            0,
            false,
            -1,
            Box::new(StraightLine),
            pass(),
        );
        assert_eq!(other.destination(&none), Some([0.0, 0.0, 0.0]), "no group target: the own place (A4)");
    }

    #[test]
    fn steers_along_the_unit_horizontal_direction_and_ignores_height() {
        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [3.0, 4.0, 7.0]);
        let mut g = walker(TARGET_FOE);
        g.update(&mut ctx, 0.032);
        assert!(near(ctx.stick, [0.6, 0.8]), "got {:?}", ctx.stick);

        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [-3.0, 4.0, -2.0]);
        let mut g = walker(TARGET_FOE);
        g.update(&mut ctx, 0.032);
        assert!(near(ctx.stick, [-0.6, 0.8]), "got {:?}", ctx.stick);
    }

    #[test]
    fn writes_no_stick_inside_zero_point_two_metres() {
        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [0.1, 0.1, 0.1]); // squared 0.03 <= 0.04
        let mut g = walker(TARGET_FOE);
        g.update(&mut ctx, 0.032);
        assert_eq!(ctx.stick, [0.0, 0.0]);

        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [0.15, 0.0, 0.15]); // squared 0.045 > 0.04: steers
        let mut g = walker(TARGET_FOE);
        g.update(&mut ctx, 0.032);
        assert!(near(ctx.stick, [1.0, 0.0]), "got {:?}", ctx.stick);
    }

    #[test]
    fn arrival_ring_is_two_metres() {
        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [1.9, 0.0, 0.0]);
        assert!(walker(TARGET_FOE).finished(&ctx));

        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [2.1, 0.0, 0.0]);
        assert!(!walker(TARGET_FOE).finished(&ctx));

        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [2.0, 0.0, 0.0]); // radius^2 <= dist^2 is not arrived
        assert!(!walker(TARGET_FOE).finished(&ctx));
    }

    #[test]
    fn current_node_has_a_one_point_six_metre_ring() {
        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [1.8, 0.0, 0.0]);
        let g = walker(TARGET_FOE);
        assert!(g.finished(&ctx), "2.0 m ring: arrived");

        let mut g = walker(TARGET_FOE);
        g.current_node = true;
        assert!(!g.finished(&ctx), "1.6 m ring: not arrived at 1.8 m");

        foe_at(&mut ctx, [1.5, 0.0, 0.0]);
        let mut g = walker(TARGET_FOE);
        g.current_node = true;
        assert!(g.finished(&ctx), "1.5 m is inside 1.6 m");
    }

    #[test]
    fn arrival_is_sticky_until_start() {
        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [1.0, 0.0, 0.0]);
        let mut g = walker(TARGET_FOE);
        assert!(g.finished(&ctx));

        foe_at(&mut ctx, [10.0, 0.0, 0.0]);
        assert!(g.finished(&ctx), "sticky: the foe moved away, still arrived");
        assert_eq!(g.score(&ctx), 1.0, "score is outside the ring now");

        g.start(&mut ctx);
        assert!(!g.finished(&ctx), "start clears the result (A7)");
    }

    #[test]
    fn no_valid_target_counts_as_arrived() {
        let ctx = Ctx::new();
        let g = walker(TARGET_FOE);
        assert!(g.finished(&ctx), "FOE with no target");

        let ctx = Ctx::new();
        let g = walker(TARGET_HOME_POINT);
        assert!(g.finished(&ctx), "HOME_POINT with no home point");

        let mut ctx = Ctx::new();
        ctx.home = Some([50.0, 0.0, 0.0]);
        let g = walker(TARGET_HOME_POINT);
        assert!(!g.finished(&ctx), "HOME_POINT far away is not arrived");

        // With no point the update does nothing: no stick.
        let mut g = walker(TARGET_FOE);
        g.update(&mut ctx, 0.032);
        assert_eq!(ctx.stick, [0.0, 0.0]);
    }

    #[test]
    fn foe_destination_is_the_tactics_point_else_the_own_position() {
        // No valid tactics point: the destination is the own position, so no steer and arrived.
        let mut ctx = Ctx::new();
        ctx.target = Some(Target { pos: [10.0, 0.0, 0.0], distance: 10.0, team: 0 });
        let mut g = walker(TARGET_FOE);
        g.update(&mut ctx, 0.032);
        assert_eq!(ctx.stick, [0.0, 0.0], "no tactics point: the own position, no steer");
        assert!(walker(TARGET_FOE).finished(&ctx), "the own position is inside the ring");

        // A valid tactics point 5 m away: steer to it, the foe's point is not used.
        ctx.tactics_point = Some([0.0, 5.0, 0.0]);
        let mut g = walker(TARGET_FOE);
        g.update(&mut ctx, 0.032);
        assert!(near(ctx.stick, [0.0, 1.0]), "steer to the tactics point: {:?}", ctx.stick);
    }

    #[test]
    fn path_is_asked_only_when_destination_changed_or_far() {
        let calls = Arc::new(AtomicU32::new(0));
        let mut g = GoToTarget::new(
            spec(TARGET_FOE),
            0,
            false,
            -1,
            Box::new(Counting(Arc::clone(&calls))),
            pass(),
        );
        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [1.5, 0.0, 0.0]);

        g.update(&mut ctx, 0.032);
        assert_eq!(calls.load(Ordering::Relaxed), 1, "first step: the destination is new");

        g.update(&mut ctx, 0.032);
        assert_eq!(calls.load(Ordering::Relaxed), 1, "same destination and 1.5 m: no path asked");

        ctx.me.pos = [-1.0, 0.0, 0.0]; // now 2.5 m away
        g.update(&mut ctx, 0.032);
        assert_eq!(calls.load(Ordering::Relaxed), 2, "more than 2.0 m away: a path is asked");
    }

    #[test]
    fn direct_branch_counts_as_reached_inside_half_a_metre() {
        // No path found: the direct branch. At 0.4 m it is reached (0.16 <= 0.25): no stick.
        let mut g = GoToTarget::new(spec(TARGET_FOE), 0, false, -1, Box::new(NoPath), pass());
        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [0.4, 0.0, 0.0]);
        g.update(&mut ctx, 0.032);
        assert_eq!(ctx.stick, [0.0, 0.0]);

        // At 1.0 m it is not reached: steers.
        let mut g = GoToTarget::new(spec(TARGET_FOE), 0, false, -1, Box::new(NoPath), pass());
        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [1.0, 0.0, 0.0]);
        g.update(&mut ctx, 0.032);
        assert!(near(ctx.stick, [1.0, 0.0]), "got {:?}", ctx.stick);
    }

    #[test]
    fn attack_kind_with_attack_flag_requests_weapon_mode_and_fire() {
        let mut g = GoToTarget::new(spec(TARGET_FOE), KIND_ATTACK_ALT, true, 2, Box::new(StraightLine), pass());
        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [5.0, 0.0, 0.0]);
        g.update(&mut ctx, 0.032);
        assert_eq!(
            ctx.requests,
            vec![Request::WeaponMode(2), Request::Button(BTN_WEAPON_FIRE)]
        );
        assert!(near(ctx.stick, [1.0, 0.0]), "it still steers");
    }

    #[test]
    fn fire_is_not_pressed_again_while_held() {
        let mut g = GoToTarget::new(spec(TARGET_FOE), KIND_ATTACK, true, 2, Box::new(StraightLine), pass());
        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [5.0, 0.0, 0.0]);
        ctx.held = 1u64 << BTN_WEAPON_FIRE;
        g.update(&mut ctx, 0.032);
        assert_eq!(ctx.requests, vec![Request::WeaponMode(2)]);
    }

    #[test]
    fn no_attack_requests_without_the_attack_flag_or_attack_kind() {
        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [5.0, 0.0, 0.0]);
        let mut g = GoToTarget::new(spec(TARGET_FOE), KIND_ATTACK, false, 2, Box::new(StraightLine), pass());
        g.update(&mut ctx, 0.032);
        assert!(ctx.requests.is_empty(), "attack flag off");

        let mut ctx = Ctx::new();
        foe_at(&mut ctx, [5.0, 0.0, 0.0]);
        let mut g = GoToTarget::new(spec(TARGET_FOE), 0, true, 2, Box::new(StraightLine), pass());
        g.update(&mut ctx, 0.032);
        assert!(ctx.requests.is_empty(), "not an attack kind");
    }

    #[test]
    fn channel_and_interruptible_match_the_game() {
        let g = walker(TARGET_FOE);
        assert_eq!(g.channel(), 4);
        assert!(g.interruptible(&Ctx::new()));
    }

    use crate::ai_node::{AiNodeData, AiNodeGraph, NODE_PATH};

    fn wp(instance: u32, pos: [f32; 3], flags: u32, next: Option<u32>) -> AiNodeData {
        AiNodeData {
            instance,
            pos,
            flags: flags | NODE_PATH,
            radius: 150.0,
            height: 50.0,
            auto_connections: next.into_iter().collect(),
            ..AiNodeData::default()
        }
    }

    /// Three waypoint nodes chained 1 -> 2 -> 3 (the convoy's shape), and a far one past the range.
    fn waypoint_chain() -> Arc<AiNodeGraph> {
        Arc::new(AiNodeGraph::new(vec![
            wp(1, [10.0, 0.0, 0.0], NODE_WAYPOINT, Some(2)),
            wp(2, [20.0, 0.0, 0.0], NODE_WAYPOINT, Some(3)),
            wp(3, [30.0, 0.0, 0.0], NODE_WAYPOINT, None),
            wp(4, [9.0, 600.0, 0.0], NODE_WAYPOINT, None),
        ]))
    }

    #[test]
    fn waypoints_follow_the_node_chain() {
        let mut ctx = Ctx::new();
        ctx.nodes = Some(waypoint_chain());
        ctx.attr.node_range_max = 500.0;
        let mut m = MoveOnWayPoints::new(0, false, -1, pass());
        assert_eq!(m.score(&ctx), 0.0, "no record before start");
        m.start(&mut ctx);
        assert!(m.target_valid(&ctx) && !m.finished(&ctx));
        m.update(&mut ctx, 0.032);
        assert!(near(ctx.stick, [1.0, 0.0]), "steers at the nearest waypoint (10, 0, 0)");
        // Within 0.5 m of node 1, the score moves the record on to node 2, then 3, then none.
        ctx.me.pos = [9.8, 0.0, 0.0];
        assert_eq!(m.score(&ctx), 1.0);
        ctx.me.pos = [19.9, 0.0, 0.0];
        m.score(&ctx);
        ctx.me.pos = [29.9, 0.0, 0.0];
        m.score(&ctx);
        assert!(m.finished(&ctx) && !m.target_valid(&ctx));
        // Farther than 0.5 m: no move on.
        let mut m = MoveOnWayPoints::new(0, false, -1, pass());
        ctx.me.pos = [0.0; 3];
        m.start(&mut ctx);
        ctx.me.pos = [9.0, 0.0, 0.0];
        m.score(&ctx);
        assert!(!m.finished(&ctx));
    }

    #[test]
    fn waypoint_start_honours_node_range_max() {
        let mut ctx = Ctx::new();
        ctx.nodes = Some(waypoint_chain());
        ctx.me.pos = [9.0, 600.0, 0.0];
        ctx.attr.node_range_max = 5.0;
        let mut m = MoveOnWayPoints::new(0, false, -1, pass());
        m.start(&mut ctx);
        assert!(m.target_valid(&ctx), "node 4 is 0 m away");
        ctx.me.pos = [9.0, 300.0, 0.0];
        m.start(&mut ctx);
        assert!(!m.target_valid(&ctx), "nothing within 5 m");
    }

    #[test]
    fn jump_node_moves_on_in_the_update() {
        let mut ctx = Ctx::new();
        ctx.nodes = Some(Arc::new(AiNodeGraph::new(vec![
            wp(1, [10.0, 0.0, 0.0], NODE_WAYPOINT | crate::ai_node::NODE_JUMP, Some(2)),
            wp(2, [10.0, 10.0, 0.0], NODE_WAYPOINT, None),
        ])));
        ctx.attr.node_range_max = 500.0;
        let mut m = MoveOnWayPoints::new(0, false, -1, pass());
        m.start(&mut ctx);
        ctx.me.pos = [10.0, 0.3, 0.0];
        // The score does not move on from a jump node.
        m.score(&ctx);
        m.update(&mut ctx, 0.032);
        assert!(near(ctx.stick, [0.0, 1.0]), "steers at the next node: {:?}", ctx.stick);
        assert!(m.target_valid(&ctx));
    }

    #[test]
    fn far_destination_steers_at_a_route_node() {
        // Path nodes every 60 m along x, radius 70: a chain. The foe (tactics point) is 300 m away.
        let data = (0..6)
            .map(|i| AiNodeData {
                instance: i + 1,
                pos: [60.0 * i as f32, 0.0, 0.0],
                flags: NODE_PATH,
                radius: 70.0,
                height: 50.0,
                ..AiNodeData::default()
            })
            .collect();
        let mut ctx = Ctx::new();
        ctx.nodes = Some(Arc::new(AiNodeGraph::new(data)));
        ctx.tactics_point = Some([300.0, 50.0, 0.0]);
        let mut g = walker(TARGET_FOE);
        g.update(&mut ctx, 0.032);
        // The first route node over 100 m away is (120, 0, 0): straight along +x.
        assert!(near(ctx.stick, [1.0, 0.0]), "{:?}", ctx.stick);
        assert_eq!(g.far_node, Some(2));
        // Within 200 m: the destination itself.
        ctx.tactics_point = Some([0.0, 150.0, 0.0]);
        g.update(&mut ctx, 0.032);
        assert!(near(ctx.stick, [0.0, 1.0]), "{:?}", ctx.stick);
        assert_eq!(g.far_node, None);
    }
}
