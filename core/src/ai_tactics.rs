//! Where an enemy decides to stand: the tactics point (phase 3, enemy AI).
//!
//! Read from the unpublished notes (three reads; the later reads correct the earlier ones): the
//! tactics data block (loader 008340d0), the record and its update rule (00833d70), the driver
//! (00834520), the candidate pick (00835330), the nav candidates (006f6830), the candidate score
//! (00835b10), the other-attacker gate (00835f20), the sight endpoints (00833a60), the LOS test
//! (00827ce0, through `navpath::straight`) and the record write (00833b70).
//!
//! Not ported: the slot pick (00832ea0 and its score 00833200), because the slot geometry is not
//! found; the cover list (0071a4d0) and the fallback 008358f0 are stand-ins.
//!
//! Tags as in `navmesh.rs`: `[game]` read in code (the address is given where it matters),
//! `[data]` from the install, `[assumed]` a guess (each one line in the unpublished notes, "Phase 3,
//! enemy AI: tactics point"), `[stand-in]` a placeholder the game does differently, `[unknown]`
//! not found. Every unset point is `FLT_MAX` on all three axes, as in the game.

use std::collections::{HashSet, VecDeque};
use std::path::Path;

use glam::Vec3;

use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};
use crate::navmesh::NavMesh;
use crate::navpath::{self, Agent};

const FLT_MAX: f32 = f32::MAX;

/// tacticsType 98f87bfb: the candidate search (scouts, Bumblebee, Ironhide, LongHaul, Megatron,
/// Optimus, Ratchet). [data] ai_char_blocks.tsv row 136; [game] the driver branch 00834520.
pub const TYPE_CANDIDATE_SEARCH: u32 = 0x98f87bfb;
/// tacticsType 2b1ca766: the target's camera point or the home point (Blackout, Jetfire, warriors).
/// [data]; the driver branch [game], the exact choice is [stand-in].
pub const TYPE_TARGET_CAMERA: u32 = 0x2b1ca766;
/// tacticsType 596a409b: slots (Starscream, TheFallen, the aerial and stun scouts). [data]
pub const TYPE_SLOT: u32 = 0x596a409b;
/// tacticsType f579762c: the candidate search with the sniper weights (Audi, Breakaway, snipers).
/// [data]
pub const TYPE_SNIPER: u32 = 0xf579762c;
/// tacticsType 8af46033: only MP AAerScoutMP. [data] The name is not found. Its driver branch is
/// not read: it takes the attacker's camera point, as any type the driver does not name [game].
pub const TYPE_MP_AERIAL_SCOUT: u32 = 0x8af46033;

/// Goal type FOE (R+0x4c). [game] 00835150, 00833a60; the hash is the one in the code.
pub const GOAL_FOE: u32 = 0xa8e780b1;
/// Goal type HOME_REGION (R+0x4c). [game] 00835330 tests it.
pub const GOAL_HOME: u32 = 0x081b1ccb;
/// No goal (the reset value). [game] 00834520 reset.
pub const GOAL_NONE: u32 = 0xc4dc849a;

/// Wall radius r = this x the character radius. [game] 006f6830 (the stack float r = 1.65 x radius,
/// manager ruling 2026-10-12; the same 1.65 is read at 00835330). It is the wall rules' radius only.
pub const WALL_FACTOR: f32 = 1.65;
/// Sight radius = this x the character radius. [game] 00827ce0 (1.1), the half-width of the sight
/// corridor (manager ruling 2026-10-12).
const SIGHT_RADIUS_FACTOR: f32 = 1.1;
/// The flood radius cap and the z lift cap of the sight start. [game] 00835330 (150) and 006f6830.
const LIFT_MAX: f32 = 150.0;
/// Added to a candidate score when its sight test fails. [game] 00835330.
const LOS_PENALTY: f32 = 1000.0;
/// A candidate equal to the current goal point scores this times lower. [game] 00835b10 (1.5).
const GOAL_KEEP: f32 = 1.5;
/// The weight of the height from the attacker: a constant, not a field. [game] 00835b10 (0.1).
const HC_WEIGHT: f32 = 0.1;
/// Extra distance for the other-attacker gate. [game] 00835f20 (3.0).
const OTHER_MARGIN: f32 = 3.0;
/// Hold time of a goal point that is not "held" (R+0x20 zero). [game] 00833d70 (30 s constant).
const HOLD_TIME: f32 = 30.0;
/// Age under which the damage rule holds the point. [game] 00833d70 (3.0 s).
const DAMAGE_HOLD_TIME: f32 = 3.0;
/// 5 m, squared. [game] 00833d70.
const NEAR_SQ: f32 = 25.0;
/// The sniper's low point test on z. [game] 00835b10 (constant 0xb338b4, 9.8).
const SNIPER_LOW_Z: f32 = 9.8;
/// The 2b1ca766 stand-in distance test, in metres. [stand-in] the 5 m test of the first read is not
/// traced; only its threshold is known.
const TARGET_CAMERA_TEST_M: f32 = 5.0;
/// Camera height above the root, for the stand-in camera point. [stand-in] 0071a2e0 is not read here.
pub const STAND_IN_CAMERA_HEIGHT: f32 = 1.5;

/// The tactics block of one character (`characterAttributes<X>.tacticsData`). [game] loader
/// 008340d0 (the fields at char attribute +0x590 and up); [data] the values in the install.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TacticsData {
    /// tacticsType hash. [game] view_scalar; the default when absent is not found [unknown].
    pub tactics_type: u32,
    /// Gate: the driver evaluates a goal only when the target is nearer than this. [game] 00834520.
    pub distance_for_tactics_max: f32,
    pub distance_from_character_min: f32,
    pub distance_from_character_max: f32,
    pub height_from_character_min: f32,
    pub height_from_character_max: f32,
    pub distance_from_target_min: f32,
    pub distance_from_target_max: f32,
    pub height_from_target_min: f32,
    pub height_from_target_max: f32,
    /// Seconds. [game] 00833d70 (+0x28).
    pub recalc_pos_time: f32,
    /// Seconds. [game] 00833d70 (+0x2c).
    pub recalc_pos_time_los: f32,
    /// The raw value (metres) is squared by the loader. [game] 008340d0; used as squared.
    pub recalc_pos_target_dist_sq: f32,
    /// The raw value (metres) is squared by the loader. [game] 008340d0; used as squared.
    pub recalc_pos_target_movement_dist_sq: f32,
    /// The raw value times 0.01: a fraction of health. [game] 008340d0 and 00833d70 (+0x38).
    pub recalc_pos_damage: f32,
}

/// Reads a `tacticsData` block (the node itself, not the character block). [game] 008340d0.
/// A float the block lacks is 0.0 [game: the loader stores 0.0 for an absent float, second read].
pub fn read(block: Node) -> TacticsData {
    let num = |name: &str| block.get(name).and_then(Node::float).unwrap_or(0.0);
    let squared = |name: &str| {
        let v = num(name);
        v * v
    };
    TacticsData {
        // [data] The field is not an enum in the install (`enum_hash` finds nothing), so the stored
        // integer is the hash. The enum name hash is the fallback [assumed] for another layout.
        tactics_type: block
            .get("tacticsType")
            .and_then(|n| n.int().map(|v| v as u32).or_else(|| n.enum_hash()))
            .unwrap_or(0),
        distance_for_tactics_max: num("distanceForTacticsMax"),
        distance_from_character_min: num("distanceFromCharacterMin"),
        distance_from_character_max: num("distanceFromCharacterMax"),
        height_from_character_min: num("heightFromCharacterMin"),
        height_from_character_max: num("heightFromCharacterMax"),
        distance_from_target_min: num("distanceFromTargetMin"),
        distance_from_target_max: num("distanceFromTargetMax"),
        height_from_target_min: num("heightFromTargetMin"),
        height_from_target_max: num("heightFromTargetMax"),
        recalc_pos_time: num("recalcPosTime"),
        recalc_pos_time_los: num("recalcPosTimeLOS"),
        recalc_pos_target_dist_sq: squared("recalcPosTargetDist"),
        recalc_pos_target_movement_dist_sq: squared("recalcPosTargetMovementDist"),
        recalc_pos_damage: num("recalcPosDamage") * 0.01,
    }
}

/// The tacticsData of the plain `characterAttributes<X>` block (`X` is `DComScout`, say) in
/// `bnxglobal.str`, read the way `ai_bind::load_attributes` reads the block. [data] The scouts keep
/// it there (second read, section 1).
pub fn load(dir: &Path, character_name: &str) -> Result<TacticsData, String> {
    let pack = Pack::open(&dir.join("bnxglobal.str"))?;
    let files: Vec<DataFile> = pack
        .of_type(pack::DATA)
        .filter_map(|c| DataFile::parse(pack.data(c).to_vec()).ok())
        .collect();
    let key = format!("characterAttributes{character_name}");
    let attrs = files.iter().find_map(|f| f.root().get(&key)).ok_or_else(|| format!("{key} missing"))?;
    let block = attrs.get("tacticsData").ok_or_else(|| format!("{key} has no tacticsData"))?;
    Ok(read(block))
}

/// The tactics record of one controller (controller +0x690). Fields named by the game's offsets
/// as the notes give them (second and third reads).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Record {
    /// R+0x04: a goal point is set. [game] 00834520.
    pub valid: bool,
    /// R+0x08: the goal point; `FLT_MAX` when none. [game] 00833b70.
    pub point: Vec3,
    /// R+0x14: the goal point before the last change. [game] 00833b70.
    pub previous: Vec3,
    /// R+0x30: anchor. [assumed] set at a pick to the target camera point (own position when
    /// there is no target). Its writer in the game is not found [unknown].
    pub anchor: Vec3,
    /// R+0x3c: the second point, set at a pick: the target camera point for HOME_REGION, the home
    /// point for FOE, `FLT_MAX` otherwise. [game] 00833b70.
    pub second: Vec3,
    /// R+0x20: the "held" byte. Cleared by a pick [game] 00833b70; its setter is not found [unknown].
    pub hold: bool,
    /// R+0x24: health at the time of the pick. [assumed] its writer is not found [unknown].
    pub health_at_pick: f32,
    /// R+0x28: the time the damage rule counts from. [assumed] set at a pick (its writer is not found).
    pub damage_clock: f32,
    /// R+0x2c: time stamp of the last pick (game time). [game] 00833b70 (DAT_00daddec).
    pub stamp: f32,
    /// R+0x48: a slot owner was displaced, or an event fired. [game] 00833200, 00823ea0 (set);
    /// cleared by a pick and by a reset.
    pub displaced: bool,
    /// R+0x4c: the goal type hash (GOAL_*). [game]
    pub goal: u32,
    /// R+0x50: the goal type before the last update. [game] the field exists; the writer is the driver.
    pub previous_goal: u32,
    /// R+0x54: the point came from a slot. [game] 00832ea0 sets it; 00834520 clears it.
    pub from_slot: bool,
    /// The sight flag of the last accepted pick (the third argument of 00835330). [game]
    pub los: bool,
    /// The game time this record has seen: the sum of the `dt` passed to `update`. [assumed]
    /// the game clock is the sum of the driver's time steps; the game reads DAT_00daddec.
    pub clock: f32,
}

impl Default for Record {
    fn default() -> Self {
        Self::new()
    }
}

impl Record {
    /// The record as the game leaves it before its first update. [game] (reset values)
    pub fn new() -> Self {
        Self {
            valid: false,
            point: unset(),
            previous: unset(),
            anchor: unset(),
            second: unset(),
            hold: false,
            health_at_pick: 0.0,
            damage_clock: 0.0,
            stamp: 0.0,
            displaced: false,
            goal: GOAL_NONE,
            previous_goal: GOAL_NONE,
            from_slot: false,
            los: false,
            clock: 0.0,
        }
    }

    /// The goal point a reader may walk to: the record is VALID and the point is real (see
    /// `is_real_point`). None otherwise: the reader then uses the scout's own position. [game] 0081e190.
    pub fn usable_point(&self) -> Option<Vec3> {
        (self.valid && is_real_point(self.point)).then_some(self.point)
    }

    /// The reset of 00834520 when the gate fails: not valid, points FLT_MAX, flag 0, no goal,
    /// not from a slot. [game]
    pub fn reset(&mut self) {
        self.valid = false;
        self.point = unset();
        self.second = unset();
        self.displaced = false;
        self.goal = GOAL_NONE;
        self.from_slot = false;
    }

    /// Whether the record must be picked again. [game] 00833d70, in the order of the second read
    /// (section 4). `target` is the fetched target, if any.
    ///
    /// [assumed] The rules that compare a target distance (rule 4's target test, rule 5's two
    /// target tests and the LOS timer) read the recalcPos fields as the notes name them; their
    /// direction and operands are not traced in full. The 3D squared distance is used.
    pub fn needs_update(&self, data: &TacticsData, me: &Me, target: Option<&Foe>) -> bool {
        // Rule 1 [game]: a 2b1ca766 record re-picks at every call unless its goal is FOE.
        if data.tactics_type == TYPE_TARGET_CAMERA && self.goal != GOAL_FOE {
            return true;
        }
        // Rule 2 [game]: displaced, unset, or outside the home region.
        if self.displaced || !is_set(self.point) || !home_test(me, self.point) {
            return true;
        }
        // Rule 3 [game]: a slot type with no slot point.
        if data.tactics_type == TYPE_SLOT && !self.from_slot && self.goal != GOAL_FOE {
            return true;
        }
        // Rule 4 [game] for the FOE goal; the target test is [assumed].
        if self.goal == GOAL_FOE {
            if me.home.is_none() {
                return true;
            }
            return match target {
                Some(t) => t.camera.distance_squared(self.anchor) > data.recalc_pos_target_dist_sq,
                None => false,
            };
        }
        // Rule 5 [game] for HOME_REGION with a target and the held byte. The three tests are
        // in the read's order; the LOS timer and the first test are [assumed].
        if self.goal == GOAL_HOME && self.hold
            && let Some(t) = target {
            if t.camera.distance_squared(self.anchor) > data.recalc_pos_target_dist_sq {
                return true;
            }
            if !self.los && self.clock - self.stamp > data.recalc_pos_time_los {
                return true;
            }
            return t.camera.distance_squared(self.second) > data.recalc_pos_target_movement_dist_sq;
        }
        // Rule 6 [game]: not held; the point holds for 30 s from the last pick.
        if !self.hold {
            return self.clock - self.stamp > HOLD_TIME;
        }
        // Rule 7 [game] for a held point with no rule 5 target: the damage and time rule.
        let damage = safe_div(self.health_at_pick - me.health, me.health_max);
        let age = self.clock - self.damage_clock;
        if damage <= data.recalc_pos_damage && age < data.recalc_pos_time {
            if age <= DAMAGE_HOLD_TIME {
                return false;
            }
            return self.anchor.distance_squared(me.position) > NEAR_SQ;
        }
        // The read gives no value for this branch [assumed]: pick again.
        true
    }

    /// The write of 00833b70. [game] The point is copied as it is; a point equal to the current one
    /// only refreshes the stamp. [assumed] the anchor is the target camera point (see the field).
    /// VALID is not written here: pass 1 owns it (R5, the unpublished notes "Fourth read"), so a point
    /// stored while VALID is 0 is never offered (R2).
    fn set_point(&mut self, point: Vec3, second: Vec3, anchor: Vec3, me: &Me, los: bool) {
        self.displaced = false;
        self.los = los;
        if point == self.point {
            self.stamp = self.clock;
            return;
        }
        self.previous = self.point;
        self.point = point;
        self.second = second;
        self.anchor = anchor;
        self.hold = false;
        self.stamp = self.clock;
        self.damage_clock = self.clock;
        self.health_at_pick = me.health;
    }
}

/// The character this record belongs to, as the pick and the gates read it. Built by the caller.
pub struct Me {
    /// The root position (the driver's distance gate and the damage rule). [assumed] source.
    pub position: Vec3,
    /// The camera point (`0071a2e0`). [stand-in] the caller supplies it; the game's camera point
    /// is not read here.
    pub camera: Vec3,
    /// The character's radius for its current form: the STEP object float at +0x170 + 4 x index
    /// (char+0x104). [game] the index rule and its uses (manager ruling 2026-10-12); the name
    /// "radius" is the manager's reading [assumed]. The value is the caller's: the scout's is
    /// `characterAttributesDComScout.avatarDescArray[robot].radius` [data] (see `tuning.rs` 834).
    pub radius: f32,
    /// Flying form (00828310 true). [game] the flag only; its source is the caller's.
    pub flying: bool,
    /// char+0x140 +0x3d. [game] the candidate mode and the score's k flag.
    pub flag_3d: bool,
    /// char+0xa6 (the second flag of the candidate mode). [game] 00835330.
    pub flag_a6: bool,
    pub health: f32,
    pub health_max: f32,
    /// The agent height of the sight test. [stand-in] the game's sight block is not read here.
    pub los_height: f32,
    /// The sight mask (char+0x140 +0x4c4). [stand-in] meaning and value are not read here.
    pub los_flags: u32,
    /// The home region, when the character has one. None is 00824650 false.
    pub home: Option<HomeRegion>,
}

/// The home region of a character: its point (00824610) and a membership test (00824700).
pub struct HomeRegion {
    /// The home point. [game] 0056bee0 via 00824610.
    pub point: Vec3,
    /// Whether a point is inside the region. [game] 00824700 (4x4 affine box, tolerance 1.01);
    /// the box is the caller's here [stand-in].
    pub contains: Box<dyn Fn(Vec3) -> bool>,
}

/// The target fetched by 00823220: its root position and its camera point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Foe {
    pub position: Vec3,
    pub camera: Vec3,
}

/// Another attacker that the candidate must keep away from. [game] 00835f20.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OtherAttacker {
    /// Its record point, or None when its record is unset (then its camera point is used).
    pub record: Option<Vec3>,
    pub camera: Vec3,
    /// Its radius (see `Me::radius`). [game] 00835f20.
    pub radius: f32,
}

/// The chosen point: the candidate and its sight flag. [game] 00835330.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pick {
    pub point: Vec3,
    pub los: bool,
}

/// The score terms of one candidate (00835b10 for the scouts' weights). `total` is `FLT_MAX` when
/// a gate fails.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Terms {
    /// Horizontal distance to the attacker's camera point.
    pub dc: f32,
    /// Height above the attacker's camera point.
    pub hc: f32,
    /// Horizontal distance to the scoring target point.
    pub dt: f32,
    /// Height above the scoring target point.
    pub ht: f32,
    pub a_term: f32,
    pub hc_term: f32,
    pub b_term: f32,
    pub c_term: f32,
    pub z9: f32,
    /// The candidate equals the current goal point (the x1.5 of 00835b10).
    pub goal_keep: bool,
    pub total: f32,
}

/// One candidate with its score and sight result (all the gate-passing candidates of `scan`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scored {
    pub point: Vec3,
    pub terms: Terms,
    pub los: bool,
    /// The score after the sight penalty.
    pub total: f32,
}

fn unset() -> Vec3 {
    Vec3::splat(FLT_MAX)
}

fn is_set(p: Vec3) -> bool {
    p.x != FLT_MAX
}

/// Whether a point is a real goal point: finite, and no axis at the game's `FLT_MAX` unset value.
/// A pass-2 search that finds nothing stores `FLT_MAX` on all three axes while VALID is already set
/// (pass 1 sets VALID by distance alone), so every reader of the record must use this, or a scout
/// walks toward infinity. [game] 0081e190 falls back to the scout's own camera point for such a
/// record (the unpublished notes "Fourth read", section 2).
pub fn is_real_point(p: Vec3) -> bool {
    p.is_finite() && p.x != FLT_MAX && p.y != FLT_MAX && p.z != FLT_MAX
}

fn safe_div(a: f32, b: f32) -> f32 {
    if b == 0.0 {
        0.0
    } else {
        a / b
    }
}

fn horizontal(a: Vec3, b: Vec3) -> f32 {
    ((a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)).sqrt()
}

/// The home test: no home region, or the point inside it. [game] 00824650 false OR 00824700(0).
fn home_test(me: &Me, p: Vec3) -> bool {
    me.home.as_ref().is_none_or(|h| (h.contains)(p))
}

/// The normalised band value of a height. [game] 00835b10 (both branches of the sign test):
/// positive heights over the max (or the span when the min is above zero), negative over the min
/// (or over the span from the max).
fn band_fraction(h: f32, lo: f32, hi: f32) -> f32 {
    if h > 0.0 {
        if lo <= 0.0 {
            safe_div(h, hi)
        } else {
            safe_div(h - lo, hi - lo)
        }
    } else if hi >= 0.0 {
        safe_div(h, lo)
    } else {
        safe_div(h - hi, lo - hi)
    }
}

/// The weights (a, b, c) of the score per tacticsType. [game] 00835b10 (type branch at the top).
fn weights(tactics_type: u32) -> (f32, f32, f32) {
    match tactics_type {
        TYPE_CANDIDATE_SEARCH => (0.1, 0.5, 0.3),
        TYPE_SNIPER => (0.1, 0.3, 0.5),
        _ => (0.2, 0.6, 0.1),
    }
}

/// The candidate mode of 00835330: 0xe flying, 0xb when both flags are clear, else 8. [game]
pub fn candidate_mode(me: &Me) -> u32 {
    if me.flying {
        0xe
    } else if !me.flag_3d && !me.flag_a6 {
        0xb
    } else {
        8
    }
}

/// Whether a point is kept away from another attacker. [game] 00835f20: the other's record point
/// (or its camera point when unset) closer than the two radii plus 3.0 (`W_att + W_other + 3.0`
/// in the ruling's reading; the radii are `Me::radius` and `OtherAttacker::radius`).
fn near_other_attacker(p: Vec3, me: &Me, others: &[OtherAttacker]) -> bool {
    others.iter().any(|o| {
        let q = o.record.unwrap_or(o.camera);
        q.distance(p) < me.radius + o.radius + OTHER_MARGIN
    })
}

/// The score of one candidate with its terms. [game] 00835b10 for the candidate search; the
/// sniper's z9 and its height term are the third read's.
pub fn score_terms(p: Vec3, t: Vec3, data: &TacticsData, me: &Me, rec: &Record, others: &[OtherAttacker]) -> Terms {
    let mut out = Terms {
        dc: horizontal(p, me.camera),
        hc: p.z - me.camera.z,
        dt: horizontal(p, t),
        ht: p.z - t.z,
        a_term: 0.0,
        hc_term: 0.0,
        b_term: 0.0,
        c_term: 0.0,
        z9: 0.0,
        goal_keep: false,
        total: FLT_MAX,
    };
    let gates = out.dc >= data.distance_from_character_min
        && out.dc <= data.distance_from_character_max
        && out.hc >= data.height_from_character_min
        && out.hc <= data.height_from_character_max
        && out.dt >= data.distance_from_target_min
        && out.dt <= data.distance_from_target_max
        && out.ht >= data.height_from_target_min
        && out.ht <= data.height_from_target_max;
    if !gates || !home_test(me, p) || near_other_attacker(p, me, others) {
        return out;
    }
    let (a, b, c) = weights(data.tactics_type);
    out.a_term = a * safe_div(out.dc, data.distance_from_character_max);
    out.hc_term = HC_WEIGHT * band_fraction(out.hc, data.height_from_character_min, data.height_from_character_max);
    out.b_term = b * (1.0 - safe_div(out.dt, data.distance_from_target_max));
    let ht = band_fraction(out.ht, data.height_from_target_min, data.height_from_target_max);
    if data.tactics_type == TYPE_SNIPER {
        // [game] z9 0.5 below 9.8; the target height term is 0.5 (1 - Ht) above the target, (1 - Ht) below.
        out.z9 = if p.z < SNIPER_LOW_Z { 0.5 } else { 0.0 };
        out.c_term = c * if out.ht > 0.0 { 0.5 * (1.0 - ht) } else { 1.0 - ht };
    } else {
        out.c_term = c * ht;
    }
    let mut total = out.a_term + out.hc_term + out.b_term + out.c_term + out.z9;
    out.goal_keep = rec.point == p;
    if out.goal_keep {
        total *= GOAL_KEEP;
    }
    out.total = total;
    out
}

/// The score alone (FLT_MAX when a gate fails). [game] 00835b10.
pub fn score(p: Vec3, t: Vec3, data: &TacticsData, me: &Me, rec: &Record, others: &[OtherAttacker]) -> f32 {
    score_terms(p, t, data, me, rec, others).total
}

/// Flood radius R = min(distanceFromTargetMax, 150). [game] 006f6830 (the XMM0 argument; R x R is
/// the 2D admission test's xmm5 at 006f6a1d / 006f6a34). Used only by the flood admission.
pub fn flood_radius(data: &TacticsData) -> f32 {
    data.distance_from_target_max.min(LIFT_MAX)
}

/// The candidates of 006f6830 before the post pass: each point with its owner triangle, in the
/// game's order. [game] `flood` is R (the flood's 2D admission test) and `wall` is r (the wall
/// rules), the two floats of the call (manager ruling 2026-10-12). The z special case
/// (`z == nav+0x514`) is not ported [unknown].
pub fn nav_candidates_raw(mesh: &NavMesh, target: Vec3, flood: f32, wall: f32, mode: u32) -> Vec<(Vec3, usize)> {
    let Some(seed) = mesh.locate(target) else {
        return Vec::new();
    };
    let t = [target.x, target.y];
    let r2 = flood * flood;
    let mut visited: HashSet<usize> = HashSet::new();
    visited.insert(seed);
    let mut order: Vec<usize> = Vec::new();
    let mut queue: VecDeque<usize> = VecDeque::new();
    queue.push_back(seed);
    while let Some(b) = queue.pop_front() {
        order.push(b);
        for k in 0..3 {
            let Some(n) = mesh.neighbour(b, k) else {
                continue;
            };
            if visited.contains(&n) {
                continue;
            }
            let floor = mesh.tri_floor(n);
            if floor == FLT_MAX || mesh.tri_ceiling(n) != FLT_MAX {
                continue;
            }
            // Same floor, or (mode bit 0x2) a floor change onto a triangle without flag bit 0.
            let same = floor == mesh.tri_floor(b);
            if !same && !(mode & 0x2 != 0 && mesh.tri_flags(n) & 0x1 == 0) {
                continue;
            }
            // One origin of the shared cell pair within r of the target, in 2D.
            let (a, c) = mesh.edge_points(b, k);
            let near = [a, c].iter().any(|p| {
                let dx = p[0] - t[0];
                let dy = p[1] - t[1];
                dx * dx + dy * dy < r2
            });
            if !near {
                continue;
            }
            visited.insert(n);
            queue.push_back(n);
        }
    }

    // Candidate points on the marked sides of each visited triangle, in queue order. The wall
    // rules use the wall radius r. [game] 006f6830 (r is the stack float).
    let mut cands: Vec<(Vec3, usize)> = Vec::new();
    for &b in &order {
        let floor = mesh.tri_floor(b);
        for k in 0..3 {
            if !mesh.edge_marked(b, k) {
                continue;
            }
            let (a3, c3) = mesh.edge_points(b, k);
            let (a, c) = ([a3[0], a3[1]], [c3[0], c3[1]]);
            let (dx, dy) = (c[0] - a[0], c[1] - a[1]);
            let len = (dx * dx + dy * dy).sqrt();
            if len <= wall {
                continue;
            }
            let d = [dx / len, dy / len];
            // Right-hand normal: the walkable side of a clockwise triangle. [game] local_7c, local_78.
            let p = [d[1] * wall, -d[0] * wall];
            let mut pts: Vec<[f32; 2]> = Vec::with_capacity(2);
            if len < 2.0 * wall {
                pts.push([(a[0] + c[0]) * 0.5 + p[0], (a[1] + c[1]) * 0.5 + p[1]]);
            } else {
                pts.push([a[0] + d[0] * wall + p[0], a[1] + d[1] * wall + p[1]]);
                pts.push([c[0] - d[0] * wall + p[0], c[1] - d[1] * wall + p[1]]);
            }
            for q in pts {
                let point = Vec3::new(q[0], q[1], floor);
                // Dedupe: the same owner within r among the last two records. [game] (loop to count-3)
                let n = cands.len();
                let dup = cands[n.saturating_sub(2)..].iter().any(|c| c.1 == b && c.0.distance(point) < wall);
                if !dup {
                    cands.push((point, b));
                }
            }
        }
    }
    cands
}

/// The post pass of 006f6830: a candidate whose owner triangle still holds it is kept; otherwise it
/// is swap-removed (the order changes as the read code's does). [game] the loop; the swap-remove is
/// as read. Returns the points.
pub fn post_pass(mesh: &NavMesh, mut cands: Vec<(Vec3, usize)>) -> Vec<Vec3> {
    let mut i = 0;
    while i < cands.len() {
        if mesh.locate(cands[i].0) == Some(cands[i].1) {
            i += 1;
        } else {
            cands.swap_remove(i);
        }
    }
    cands.into_iter().map(|c| c.0).collect()
}

/// The nav candidates of 006f6830: the flood (radius `flood`), the wall rules (radius `wall`) and
/// the post pass. [game] unless marked.
pub fn nav_candidates(mesh: &NavMesh, target: Vec3, flood: f32, wall: f32, mode: u32) -> Vec<Vec3> {
    post_pass(mesh, nav_candidates_raw(mesh, target, flood, wall, mode))
}

/// The sight test of one candidate's pick: the start is the candidate lifted by the clamped target
/// distance max, the end is the target camera point (stand-in for 00833a60 flag 1, whose cover list
/// 0071a4d0 is unread). [game] 00835330 and 00827ce0; the end point is [stand-in].
fn sight(mesh: &NavMesh, p: Vec3, lift: f32, end: Vec3, agent: &Agent) -> bool {
    let start = Vec3::new(p.x, p.y, p.z + lift);
    navpath::straight(mesh, start, end, agent)
}

/// The scoring target of a candidate search for a record's goal, or None when the search cannot
/// run. [game] 00835330: FOE takes the home point (needs a home region); HOME_REGION takes the target
/// camera point.
fn search_target(rec: &Record, me: &Me, target: Option<&Foe>) -> Option<Vec3> {
    match rec.goal {
        GOAL_FOE => me.home.as_ref().map(|h| h.point),
        GOAL_HOME => target.map(|t| t.camera),
        _ => None,
    }
}

/// Every candidate of the search that passes the gates, scored with its sight result (all of
/// them; `pick` does the same in the game's lazy order). Returns the candidate count with the list.
pub fn scan(rec: &Record, data: &TacticsData, me: &Me, target: Option<&Foe>, others: &[OtherAttacker], mesh: &NavMesh) -> (usize, Vec<Scored>) {
    let Some(t) = search_target(rec, me, target) else {
        return (0, Vec::new());
    };
    let lift = data.distance_from_target_max.min(LIFT_MAX);
    let cands = nav_candidates(mesh, t, flood_radius(data), WALL_FACTOR * me.radius, candidate_mode(me));
    let agent = sight_agent(me);
    let count = cands.len();
    let mut out = Vec::new();
    for p in cands {
        let terms = score_terms(p, t, data, me, rec, others);
        if terms.total == FLT_MAX {
            continue;
        }
        let los = sight(mesh, p, lift, t, &agent);
        let total = if los { terms.total } else { terms.total + LOS_PENALTY };
        out.push(Scored { point: p, terms, los, total });
    }
    (count, out)
}

/// The agent of the sight test: radius 1.1 x the character radius. [game] 00827ce0; height and mask
/// [stand-in].
fn sight_agent(me: &Me) -> Agent {
    Agent::new(SIGHT_RADIUS_FACTOR * me.radius, me.los_height, me.los_flags)
}

/// The choice of 00835330 over scored candidates in their order: the lowest total, strictly lower
/// wins (the first of equal totals stays). A failed sight adds 1000 (`Scored::total` has it).
pub fn choose(scored: &[Scored]) -> Option<Pick> {
    let mut best = FLT_MAX;
    let mut out = None;
    for s in scored {
        if s.total < best {
            best = s.total;
            out = Some(Pick { point: s.point, los: s.los });
        }
    }
    out
}

/// The candidate search of 00835330, in the game's lazy order: a candidate whose score is not
/// below the best so far is skipped before its sight test. [game] Same result as `choose` over
/// `scan`. Returns None when nothing passes (FLT_MAX in the game).
pub fn pick(rec: &Record, data: &TacticsData, me: &Me, target: Option<&Foe>, others: &[OtherAttacker], mesh: &NavMesh) -> Option<Pick> {
    let t = search_target(rec, me, target)?;
    let lift = data.distance_from_target_max.min(LIFT_MAX);
    let cands = nav_candidates(mesh, t, flood_radius(data), WALL_FACTOR * me.radius, candidate_mode(me));
    let agent = sight_agent(me);
    let mut best = FLT_MAX;
    let mut out = None;
    for p in cands {
        let s = score(p, t, data, me, rec, others);
        if s >= best {
            continue;
        }
        let los = sight(mesh, p, lift, t, &agent);
        let s = if los { s } else { s + LOS_PENALTY };
        if s < best {
            best = s;
            out = Some(Pick { point: p, los });
        }
    }
    out
}

/// The home point, or the camera point when there is no home region. [game] 00824610.
fn home_or_camera(me: &Me) -> Vec3 {
    me.home.as_ref().map_or(me.camera, |h| h.point)
}

/// The point of the 596a409b branch when no slot is taken: the target's position when the home
/// test passes. [game] 00832ea0 (the fallback). [stand-in] The slot geometry is not found, so no
/// slot point is ever made.
fn slot_stand_in(me: &Me, target: Option<&Foe>) -> Option<Vec3> {
    let t = target?;
    if home_test(me, t.position) {
        Some(t.position)
    } else {
        None
    }
}

/// The point of the 2b1ca766 branch: the target's camera point when the distance test passes,
/// else the home point or the camera point. [game] the branch shape (first read section 2);
/// the 5 m test is [stand-in] (its threshold is in the read, its operands are not traced).
fn target_camera_stand_in(me: &Me, target: Option<&Foe>) -> Vec3 {
    match target {
        Some(t) if horizontal(t.camera, me.camera) > TARGET_CAMERA_TEST_M => t.camera,
        _ => home_or_camera(me),
    }
}

/// [stand-in] The candidate search's point when there is no nav mesh (the test arena). The game
/// scores nav-mesh candidates around the target by the distanceFromTarget band (min..max) and the
/// other terms (`score_terms`); without candidates this keeps only the band: the point on the
/// horizontal bearing from the target to the character, at the character's own distance clamped into
/// [distance_from_target_min, distance_from_target_max], at the character's own height. So a
/// character already inside the band stands where it is, one too near backs off, one too far closes
/// in. No target: no point. Settle: build the arena a nav mesh (the real search then runs).
fn standoff_stand_in(data: &TacticsData, me: &Me, target: Option<&Foe>) -> Option<Vec3> {
    let t = target?.position;
    let away = (me.position - t).truncate();
    let d = away.length();
    let dir = if d > 1e-3 { away / d } else { glam::Vec2::Y };
    let lo = data.distance_from_target_min.max(0.0);
    let want = d.clamp(lo, data.distance_from_target_max.max(lo));
    let p = t.truncate() + dir * want;
    Some(Vec3::new(p.x, p.y, me.position.z))
}

/// Pass 1 of the driver (00834520, decompile lines 6-100): the VALID byte (R+0x04). [game, disasm]
/// Set when the target's camera point is nearer than distanceForTacticsMax (200) to the scout's
/// camera point, or when the goal is FOE (gate 0x834755..0x834777, `ja 0x834779` accepts; then
/// `cmp goal, FOE`). Otherwise a VALID record is cleared (points FLT_MAX, goal NONE; reject path at
/// 0x8347ac) and a record that was never valid is left alone. [game]
/// [assumed] The distance is 3D between the two camera points: the game's helper 00725400 is seen
/// only as a sqrt, its dimension is not read. The camera points are the note's reading (the earlier
/// code used the root positions).
fn pass_one_valid(record: &mut Record, data: &TacticsData, me: &Me, target: Option<&Foe>) {
    let in_range = target.is_some_and(|t| t.camera.distance(me.camera) < data.distance_for_tactics_max);
    if in_range || record.goal == GOAL_FOE {
        record.valid = true;
    } else if record.valid {
        record.reset();
    }
}

/// The driver for one controller (00834520), run once per time step with `dt`. [game] pass 1 (the
/// VALID byte, `pass_one_valid`) runs first (R5), then pass 2 (the candidate pick and the point).
/// Pass 2 runs whether or not VALID is set; a point stored while VALID is 0 is never offered
/// (R2: `Record::valid` gates every reader). [game] The goal policy, the time step and the sniper
/// fallback are [assumed] or [stand-in] as marked. The game runs it from a priority-4 update list
/// [game, ai-tactics second read section 4]; its rate in seconds is not found [unknown].
/// `mesh` None (the test arena has no level, so no nav mesh): the candidate search is replaced by
/// `standoff_stand_in` [stand-in]; everything else (pass 1, the recalc rules, the write) is the same.
pub fn update(record: &mut Record, data: &TacticsData, me: &Me, target: Option<&Foe>, others: &[OtherAttacker], mesh: Option<&NavMesh>, dt: f32) {
    // [assumed] The driver runs only for a non-zero time step (the game tests XMM0).
    if dt == 0.0 {
        return;
    }
    record.clock += dt;
    // [assumed] Goal policy: a target makes it HOME_REGION; no target with a home region makes it
    // FOE (stand at home). The writer of R+0x4c in the game is not found [unknown].
    record.previous_goal = record.goal;
    record.goal = match (target, me.home.is_some()) {
        (Some(_), _) => GOAL_HOME,
        (None, true) => GOAL_FOE,
        (None, false) => GOAL_NONE,
    };
    // R5: pass 1 before pass 2, each update.
    pass_one_valid(record, data, me, target);
    if !record.needs_update(data, me, target) {
        return;
    }
    record.from_slot = false;
    let anchor = target.map_or(me.position, |t| t.camera);
    let (point, los) = match data.tactics_type {
        TYPE_SLOT => (slot_stand_in(me, target), true),
        TYPE_TARGET_CAMERA => (Some(target_camera_stand_in(me, target)), true),
        TYPE_CANDIDATE_SEARCH | TYPE_SNIPER => match mesh.and_then(|mesh| pick(record, data, me, target, others, mesh)) {
            Some(p) => (Some(p.point), p.los),
            None if mesh.is_none() => (standoff_stand_in(data, me, target), true),
            // [stand-in] The sniper's fallback 008358f0 is not ported; the home or camera point.
            None if data.tactics_type == TYPE_SNIPER => (Some(home_or_camera(me)), true),
            None => (None, true),
        },
        // Any other type: the attacker's camera point [game, first read section 2].
        _ => (Some(me.camera), true),
    };
    let second = match record.goal {
        GOAL_HOME => target.map_or(unset(), |t| t.camera),
        GOAL_FOE => me.home.as_ref().map_or(unset(), |h| h.point),
        _ => unset(),
    };
    let point = point.unwrap_or_else(unset);
    record.set_point(point, second, anchor, me, los);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navmesh::Silhouette;
    use std::path::PathBuf;

    fn install() -> Option<PathBuf> {
        let dir = PathBuf::from(crate::install::dir_string());
        dir.join("bnxglobal.str").is_file().then_some(dir)
    }

    /// The scout's block as the second read gives it (characterAttributesDComScout).
    fn dcom_scout() -> TacticsData {
        TacticsData {
            tactics_type: TYPE_CANDIDATE_SEARCH,
            distance_for_tactics_max: 200.0,
            distance_from_character_min: 10.0,
            distance_from_character_max: 200.0,
            height_from_character_min: -100.0,
            height_from_character_max: 100.0,
            distance_from_target_min: 30.0,
            distance_from_target_max: 100.0,
            height_from_target_min: -25.0,
            height_from_target_max: 15.0,
            recalc_pos_time: 15.0,
            recalc_pos_time_los: 5.0,
            recalc_pos_target_dist_sq: 20.0 * 20.0,
            recalc_pos_target_movement_dist_sq: 150.0 * 150.0,
            recalc_pos_damage: 0.5,
        }
    }

    fn me_at(camera: Vec3) -> Me {
        Me {
            position: camera - Vec3::new(0.0, 0.0, STAND_IN_CAMERA_HEIGHT),
            camera,
            radius: 10.0,
            flying: false,
            flag_3d: false,
            flag_a6: false,
            health: 100.0,
            health_max: 100.0,
            los_height: 1.8,
            los_flags: 0,
            home: None,
        }
    }

    fn goal_record(goal: u32) -> Record {
        Record { goal, ..Record::new() }
    }

    #[test]
    fn reads_dcom_scout_from_install() {
        let Some(dir) = install() else {
            eprintln!("ai_tactics: no install, skipped");
            return;
        };
        let d = load(&dir, "DComScout").expect("characterAttributesDComScout has a tacticsData block");
        let want = dcom_scout();
        assert_eq!(d.tactics_type, TYPE_CANDIDATE_SEARCH, "tacticsType");
        assert_eq!(d.distance_for_tactics_max, want.distance_for_tactics_max);
        assert_eq!(d.distance_from_character_min, want.distance_from_character_min);
        assert_eq!(d.distance_from_character_max, want.distance_from_character_max);
        assert_eq!(d.height_from_character_min, want.height_from_character_min);
        assert_eq!(d.height_from_character_max, want.height_from_character_max);
        assert_eq!(d.distance_from_target_min, want.distance_from_target_min);
        assert_eq!(d.distance_from_target_max, want.distance_from_target_max);
        assert_eq!(d.height_from_target_min, want.height_from_target_min);
        assert_eq!(d.height_from_target_max, want.height_from_target_max);
        assert_eq!(d.recalc_pos_time, 15.0);
        assert_eq!(d.recalc_pos_time_los, 5.0);
        assert_eq!(d.recalc_pos_target_dist_sq, 400.0, "recalcPosTargetDist 20 stored squared");
        assert_eq!(d.recalc_pos_target_movement_dist_sq, 22500.0, "recalcPosTargetMovementDist 150 stored squared");
        assert!((d.recalc_pos_damage - 0.5).abs() < 1e-6, "recalcPosDamage 50 x 0.01");
    }

    #[test]
    fn needs_update_timers() {
        let d = dcom_scout();
        let me = me_at(Vec3::new(0.0, 0.0, 1.5));
        // Not held: the point holds for 30 s from the pick (rule 6).
        let mut r = goal_record(GOAL_HOME);
        r.point = Vec3::new(40.0, 0.0, 0.0);
        r.stamp = 0.0;
        r.clock = 10.0;
        assert!(!r.needs_update(&d, &me, None), "10 s after the pick holds");
        r.clock = 31.0;
        assert!(r.needs_update(&d, &me, None), "31 s after the pick fires");
        // Held: recalcPosTime and the damage fraction (rule 7). Age 1 s, no damage: holds.
        r.hold = true;
        r.damage_clock = 0.0;
        r.clock = 1.0;
        assert!(!r.needs_update(&d, &me, None), "held point inside recalcPosTime holds");
        // Age over 3 s and within recalcPosTime: holds while the anchor is within 5 m.
        r.clock = 4.0;
        r.anchor = Vec3::new(0.0, 0.0, 1.5);
        assert!(!r.needs_update(&d, &me, None), "anchor 0 m from own position holds");
        r.anchor = Vec3::new(10.0, 0.0, 1.5);
        assert!(r.needs_update(&d, &me, None), "anchor 10 m away fires");
        // Past recalcPosTime: fires.
        r.anchor = Vec3::new(0.0, 0.0, 1.5);
        r.clock = 20.0;
        assert!(r.needs_update(&d, &me, None), "past recalcPosTime fires");
        // A 2b1ca766 record always fires unless its goal is FOE.
        let warrior = TacticsData { tactics_type: TYPE_TARGET_CAMERA, ..d };
        let mut w = goal_record(GOAL_HOME);
        w.point = Vec3::new(40.0, 0.0, 0.0);
        w.hold = false;
        assert!(w.needs_update(&warrior, &me, None), "2b1ca766 re-picks every call");
    }

    #[test]
    fn needs_update_target_moved() {
        let d = dcom_scout();
        let me = me_at(Vec3::new(0.0, 0.0, 1.5));
        let target = Foe { position: Vec3::new(100.0, 0.0, 0.0), camera: Vec3::new(100.0, 0.0, 1.5) };
        let mut r = goal_record(GOAL_HOME);
        r.point = Vec3::new(60.0, 0.0, 1.5);
        r.hold = true;
        r.los = true;
        r.second = target.camera;
        r.anchor = target.camera;
        r.clock = 1.0;
        r.stamp = 0.0;
        // Target still where it was: holds.
        assert!(!r.needs_update(&d, &me, Some(&target)));
        // Target moved 10 m (100 m squared 10^2 = 100 is inside the 150 m movement limit): holds.
        let moved = Foe { position: target.position, camera: target.camera + Vec3::new(10.0, 0.0, 0.0) };
        assert!(!r.needs_update(&d, &me, Some(&moved)), "10 m is inside recalcPosTargetMovement 150 m");
        // Target moved 200 m: fires (movement test against the stored second point).
        let far = Foe { position: target.position, camera: target.camera + Vec3::new(200.0, 0.0, 0.0) };
        assert!(r.needs_update(&d, &me, Some(&far)), "200 m is outside recalcPosTargetMovement 150 m");
    }

    #[test]
    fn band_gates_and_order() {
        let d = dcom_scout();
        let me = me_at(Vec3::new(0.0, 0.0, 1.5));
        let t = Vec3::new(100.0, 0.0, 1.5);
        let rec = Record::new();
        // Too near the target (Dt 5 < 30): rejected.
        assert_eq!(score(Vec3::new(95.0, 0.0, 1.5), t, &d, &me, &rec, &[]), FLT_MAX);
        // Too far from the target (Dt 150 > 100): rejected.
        assert_eq!(score(Vec3::new(-50.0, 0.0, 1.5), t, &d, &me, &rec, &[]), FLT_MAX);
        // Inside the band: finite.
        assert!(score(Vec3::new(40.0, 0.0, 1.5), t, &d, &me, &rec, &[]) < FLT_MAX);
        // Of two points inside, the one farther from the target wins (Dt 70 beats Dt 40).
        let far_from_target = Vec3::new(30.0, 0.0, 1.5); // Dt 70, Dc 30
        let near_to_target = Vec3::new(60.0, 0.0, 1.5); // Dt 40, Dc 60
        assert!(score(far_from_target, t, &d, &me, &rec, &[]) < score(near_to_target, t, &d, &me, &rec, &[]));
        // Same target distance (70): the one nearer to self wins.
        let near_self = Vec3::new(30.0, 0.0, 1.5); // Dc 30
        let other = Vec3::new(100.0 - 70.0 * 0.5, 70.0 * 0.866_025_4, 1.5); // Dt 70, Dc about 89
        assert!((horizontal(other, t) - 70.0).abs() < 1e-3);
        assert!(score(near_self, t, &d, &me, &rec, &[]) < score(other, t, &d, &me, &rec, &[]));
        // The goal point keeps its place: x 1.5 on its score.
        let mut kept = Record::new();
        kept.point = near_self;
        let s_kept = score(near_self, t, &d, &me, &kept, &[]);
        let s_plain = score(near_self, t, &d, &me, &rec, &[]);
        assert!((s_kept - 1.5 * s_plain).abs() < 1e-5);
    }

    #[test]
    fn other_attacker_excludes_a_disc() {
        let d = dcom_scout();
        let me = me_at(Vec3::new(0.0, 0.0, 1.5));
        let t = Vec3::new(100.0, 0.0, 1.5);
        let other = OtherAttacker { record: Some(Vec3::new(30.0, 0.0, 1.5)), camera: Vec3::ZERO, radius: 10.0 };
        let rec = Record::new();
        // Exclusion 10 + 10 + 3 = 23 m: a point 5 m from the other's record is rejected.
        assert_eq!(score(Vec3::new(30.0, 5.0, 1.5), t, &d, &me, &rec, &[other]), FLT_MAX);
        // A point 40 m from it is kept.
        assert!(score(Vec3::new(30.0, 40.0, 1.5), t, &d, &me, &rec, &[other]) < FLT_MAX);
        // An unset record uses the camera point instead.
        let unset_other = OtherAttacker { record: None, camera: Vec3::new(30.0, 0.0, 1.5), radius: 10.0 };
        assert_eq!(score(Vec3::new(30.0, 5.0, 1.5), t, &d, &me, &rec, &[unset_other]), FLT_MAX);
    }

    #[test]
    fn sight_penalty_adds_but_can_win() {
        let p1 = Vec3::new(10.0, 0.0, 0.0);
        let p2 = Vec3::new(20.0, 0.0, 0.0);
        let terms = Terms { dc: 0.0, hc: 0.0, dt: 0.0, ht: 0.0, a_term: 0.0, hc_term: 0.0, b_term: 0.0, c_term: 0.0, z9: 0.0, goal_keep: false, total: 0.2 };
        let blocked = Scored { point: p1, terms, los: false, total: 0.2 + LOS_PENALTY };
        let clear = Scored { point: p2, terms: Terms { total: 0.5, ..terms }, los: true, total: 0.5 };
        // A blocked candidate alone is still chosen (no rejection), with its flag 0.
        assert_eq!(choose(&[blocked]), Some(Pick { point: p1, los: false }));
        // A clear candidate with a higher score beats a blocked one.
        assert_eq!(choose(&[blocked, clear]), Some(Pick { point: p2, los: true }));
        // Nothing passes: None.
        assert_eq!(choose(&[]), None);
    }

    /// An open square 0..100 with one long wall at x 49.5..50.5, y 20..80 (clockwise outline).
    fn square_with_wall() -> NavMesh {
        let wall = Silhouette {
            name: "test wall".into(),
            points: vec![[49.5, 20.0, 0.0], [49.5, 80.0, 0.0], [50.5, 80.0, 0.0], [50.5, 20.0, 0.0], [49.5, 20.0, 0.0]],
            base_z: 0.0,
            height: 3.0,
            replacement_height: 0.0,
            ai_ban_height: 0.0,
            extra_height: 0.0,
            pivot: 0.0,
            flags: 0,
            can_not_walk_off: false,
            treat_as_dynamic: false,
            impermanent: false,
            raw_points: 5,
            object: (0, 0),
        };
        NavMesh::build(Some((Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 100.0, 10.0))), &[wall]).expect("test mesh builds")
    }

    #[test]
    fn candidates_lie_off_the_wall_on_the_walkable_side() {
        let mesh = square_with_wall();
        let r = 16.5; // radius 10 x WALL_FACTOR 1.65; flood and wall both 16.5 here (test values)
        // The target sits 10.8 m from the wall's south end (49.5, 20): the flood admits only
        // triangles with a shared-edge end within r of the target, so the target must be near the wall.
        let cands = nav_candidates(&mesh, Vec3::new(40.0, 25.0, 0.0), r, r, 0xb);
        assert!(!cands.is_empty(), "the flood from (40, 25) finds the wall's sides");
        // The marked sides are the wall's faces and the box's sides (the root triangles mark the box).
        // Every candidate is r from one of those lines, on the walkable side, outside the wall.
        let near = |a: f32, b: f32| (a - b).abs() < 0.05;
        for c in &cands {
            assert!(c.x < 49.5 || c.x > 50.5, "{c:?} lies inside the wall");
            let on_line = near(c.x, 49.5 - r)
                || near(c.x, 50.5 + r)
                || near(c.x, r)
                || near(c.x, 100.0 - r)
                || near(c.y, r)
                || near(c.y, 100.0 - r);
            assert!(on_line, "{c:?} is not r off a marked side");
            assert_eq!(c.z, 0.0, "z is the owner triangle's floor");
        }
        // The wall's west face, at r on the target's side, gives the two end points of its long side.
        let west: Vec<&Vec3> = cands.iter().filter(|c| near(c.x, 49.5 - r)).collect();
        assert!(!west.is_empty(), "a west-face candidate exists: {cands:?}");
        for c in west {
            assert!(near(c.y, 20.0 + r) || near(c.y, 80.0 - r), "{c:?} is not at an end of the long side");
        }
    }

    #[test]
    fn flood_and_wall_radii_are_separate() {
        let mesh = square_with_wall();
        let target = Vec3::new(40.0, 25.0, 0.0);
        // The flood radius only admits triangles; the wall radius only places points. With a
        // wider flood the same wall rules can only add candidates, and each one is still r off a side.
        let narrow = nav_candidates_raw(&mesh, target, 16.5, 16.5, 0xb);
        let wide = nav_candidates_raw(&mesh, target, 100.0, 16.5, 0xb);
        assert!(wide.len() >= narrow.len(), "the wider flood admits at least as many owners");
        let narrow_points = nav_candidates(&mesh, target, 16.5, 16.5, 0xb);
        let wall_only = nav_candidates(&mesh, target, 100.0, 16.5, 0xb);
        assert!(wall_only.len() >= narrow_points.len());
        // A smaller wall radius moves every wall point, so the sides are not the same points.
        let small_wall = nav_candidates(&mesh, target, 100.0, 5.0, 0xb);
        assert!(small_wall.iter().any(|c| (c.x - (49.5 - 5.0)).abs() < 0.05), "the west face is at 5 m with wall radius 5");
        assert!(!small_wall.iter().any(|c| (c.x - (49.5 - 16.5)).abs() < 0.05), "no point at the 16.5 m offset");
    }

    #[test]
    fn flood_radius_is_capped_at_150() {
        let mut d = TacticsData { distance_from_target_max: 100.0, ..read_empty() };
        assert_eq!(flood_radius(&d), 100.0);
        d.distance_from_target_max = 400.0;
        assert_eq!(flood_radius(&d), 150.0);
    }

    fn read_empty() -> TacticsData {
        TacticsData {
            tactics_type: 0,
            distance_for_tactics_max: 0.0,
            distance_from_character_min: 0.0,
            distance_from_character_max: 0.0,
            height_from_character_min: 0.0,
            height_from_character_max: 0.0,
            distance_from_target_min: 0.0,
            distance_from_target_max: 0.0,
            height_from_target_min: 0.0,
            height_from_target_max: 0.0,
            recalc_pos_time: 0.0,
            recalc_pos_time_los: 0.0,
            recalc_pos_target_dist_sq: 0.0,
            recalc_pos_target_movement_dist_sq: 0.0,
            recalc_pos_damage: 0.0,
        }
    }

    #[test]
    fn candidate_mode_follows_flags() {
        let mut me = me_at(Vec3::ZERO);
        assert_eq!(candidate_mode(&me), 0xb);
        me.flag_a6 = true;
        assert_eq!(candidate_mode(&me), 8);
        me.flag_a6 = false;
        me.flying = true;
        assert_eq!(candidate_mode(&me), 0xe);
    }

    #[test]
    fn update_resets_when_the_target_is_too_far() {
        let mesh = square_with_wall();
        let d = dcom_scout();
        let me = me_at(Vec3::new(10.0, 50.0, 1.5));
        let far = Foe { position: Vec3::new(900.0, 0.0, 0.0), camera: Vec3::new(900.0, 0.0, 1.5) };
        let mut r = goal_record(GOAL_HOME);
        r.point = Vec3::new(40.0, 50.0, 0.0);
        r.valid = true;
        update(&mut r, &d, &me, Some(&far), &[], Some(&mesh), 0.032);
        assert!(!r.valid);
        assert_eq!(r.point, unset());
        assert_eq!(r.goal, GOAL_NONE);
    }

    /// The target's camera point at (40, 25, 1.5) on the wall mesh. A scout at x -100 is 140 m from
    /// it, at x -180 is 220 m (the 200 m gate of pass 1).
    fn foe_on_mesh() -> Foe {
        Foe { position: Vec3::new(40.0, 25.0, 0.0), camera: Vec3::new(40.0, 25.0, 1.5) }
    }

    #[test]
    fn valid_within_200m() {
        let mesh = square_with_wall();
        let d = dcom_scout();
        let foe = foe_on_mesh();
        let mut r = Record::new();
        update(&mut r, &d, &me_at(Vec3::new(-100.0, 25.0, 1.5)), Some(&foe), &[], Some(&mesh), 0.032);
        assert!(r.valid, "140 m is inside distanceForTacticsMax 200");
        assert_eq!(r.goal, GOAL_HOME);
    }

    #[test]
    fn invalid_beyond_200m_stores_a_point_but_is_not_valid() {
        let mesh = square_with_wall();
        // Wider character reach so the candidates of the target's side pass the scout's own gate.
        let d = TacticsData { distance_from_character_max: 400.0, ..dcom_scout() };
        let foe = foe_on_mesh();
        let mut r = Record::new();
        update(&mut r, &d, &me_at(Vec3::new(-180.0, 25.0, 1.5)), Some(&foe), &[], Some(&mesh), 0.032);
        assert!(!r.valid, "220 m is beyond 200 m: VALID stays 0");
        assert!(is_set(r.point), "pass 2 still stores a point (R2: it is not offered)");
    }

    #[test]
    fn becomes_valid_when_the_scout_comes_within_range() {
        let mesh = square_with_wall();
        let d = TacticsData { distance_from_character_max: 400.0, ..dcom_scout() };
        let foe = foe_on_mesh();
        let mut r = Record::new();
        update(&mut r, &d, &me_at(Vec3::new(-180.0, 25.0, 1.5)), Some(&foe), &[], Some(&mesh), 0.032);
        assert!(!r.valid);
        update(&mut r, &d, &me_at(Vec3::new(-100.0, 25.0, 1.5)), Some(&foe), &[], Some(&mesh), 0.032);
        assert!(r.valid, "the scout is now 140 m away");
    }

    #[test]
    fn cleared_when_the_scout_leaves_range() {
        let mesh = square_with_wall();
        let d = TacticsData { distance_from_character_max: 400.0, ..dcom_scout() };
        let foe = foe_on_mesh();
        let mut r = Record::new();
        update(&mut r, &d, &me_at(Vec3::new(-100.0, 25.0, 1.5)), Some(&foe), &[], Some(&mesh), 0.032);
        assert!(r.valid);
        update(&mut r, &d, &me_at(Vec3::new(-180.0, 25.0, 1.5)), Some(&foe), &[], Some(&mesh), 0.032);
        assert!(!r.valid, "a valid record is cleared when the scout leaves range");
        assert_eq!(r.goal, GOAL_NONE, "the reject path resets the goal to NONE [game]");
        assert_eq!(r.point, unset(), "and clears the points");
    }

    #[test]
    fn goal_foe_is_valid_whatever_the_distance() {
        let mesh = square_with_wall();
        let d = dcom_scout();
        let home = HomeRegion { point: Vec3::new(10.0, 10.0, 0.0), contains: Box::new(|_| true) };
        let me = Me { home: Some(home), ..me_at(Vec3::new(-900.0, 25.0, 1.5)) };
        let mut r = Record::new();
        // No target and a home region: the goal is FOE, and pass 1 accepts it at any distance.
        update(&mut r, &d, &me, None, &[], Some(&mesh), 0.032);
        assert_eq!(r.goal, GOAL_FOE);
        assert!(r.valid, "goal FOE overrides the distance gate");
    }

    #[test]
    fn without_a_mesh_the_point_keeps_the_target_distance_band() {
        let d = dcom_scout(); // distanceFromTarget 30..100
        let foe = Foe { position: Vec3::ZERO, camera: Vec3::new(0.0, 0.0, STAND_IN_CAMERA_HEIGHT) };
        let point_from = |x: f32| {
            let me = me_at(Vec3::new(x, 0.0, STAND_IN_CAMERA_HEIGHT));
            let mut r = Record::new();
            update(&mut r, &d, &me, Some(&foe), &[], None, 0.032);
            assert!(r.valid, "the target is within distanceForTacticsMax");
            r.usable_point().expect("a stand-off point")
        };
        // Too near: backs off along the same bearing to the band's minimum.
        assert!(point_from(4.0).distance(Vec3::new(30.0, 0.0, 0.0)) < 1e-4);
        // Inside the band: stands where it is.
        assert!(point_from(50.0).distance(Vec3::new(50.0, 0.0, 0.0)) < 1e-4);
        // Too far: closes in to the band's maximum.
        assert!(point_from(150.0).distance(Vec3::new(100.0, 0.0, 0.0)) < 1e-4);
    }
}
