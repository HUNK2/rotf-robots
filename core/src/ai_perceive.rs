//! The AI perceptions (phase 3): Compare, Targeting, Message, Range, Random and Timer, and the
//! aggregate that combines one entry's links (0082e7e0, with the link evaluator 0082e570).
//! Source: notes/ai-perceptions.md, and its "Manager rulings" section, which wins over the rest.
//!
//! Tags as in ai_brain.rs. Perceptions read the world only through `Ctx`. A score takes `&Ctx`,
//! so the one side effect of scoring (a link's message to the character) goes through `Ctx::emit`.
//! Not wired into sim.rs or the game yet.

use crate::ai_brain::{lock_latches, AiAttr, Ctx, Scorer};
use crate::ai_move::{TARGET_FOE, TARGET_FRIEND, TARGET_TURRET};

/// AVG: the mean of the links that scored above 0 [game 0082e7e0; hash d1cad841, the shipped data].
pub const CALC_AVG: u32 = 0xd1cad841;
/// MAX: the best link score [game 0082e7e0; hash 68e4023e, unused by the shipped data].
pub const CALC_MAX: u32 = 0x68e4023e;
/// MIN: the lowest link score above 0 [game 0082e7e0; hash 6b17e271, unused by the shipped data].
pub const CALC_MIN: u32 = 0x6b17e271;
/// REQUIRE: a link at 0 or below rejects the whole entry [game 0082e7e0; hash 90d88c38].
pub const COND_REQUIRE: u32 = 0x90d88c38;

/// compareType: V above T [game 0082ae90; hash d89693aa]. [assumed] the boolean reading.
pub const CMP_ABOVE: u32 = 0xd89693aa;
/// compareType: V below T, the distance reading [game 0082ae90; hash 2156da7b].
pub const CMP_BELOW: u32 = 0x2156da7b;

/// A perception: the object a link holds (its vtable slot +0x10 is `score`).
pub trait Perception: Send + Sync {
    /// The raw score [game]: 0 fails. The booleans score 0 or 1; Random and Range say so too.
    fn score(&self, ctx: &Ctx) -> f32;
    /// Enter (vtable +0x14). Only the Message perception latches [game 0082e8f0 walk].
    fn enter(&mut self, _ctx: &Ctx) {}
    /// Leave (vtable +0x18). Only the Message perception unlatches.
    fn leave(&mut self, _ctx: &Ctx) {}
}

/// One valueType of the data (the cases of the value getter 00830360) [game, data].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueKind {
    pub hash: u32,
    /// What the value is, from the getter's branch (notes/ai-perceptions.md, third read B and the
    /// second read A). "[unknown]" where the branch is not read.
    pub meaning: &'static str,
    /// True when this port computes the value. False: a [stand-in] with no value (score 0).
    pub bound: bool,
}

/// The 26 valueTypes the shipped data uses in Compare and Range (usage counts [data]). All 26 are
/// bound (fifth read, ai-perceptions.md "Value kinds (fourth read)" and "(fifth read)"): the value
/// mechanism is ported for each, and the facts it reads are in `Senses`. Where a fact is fed by a
/// stand-in in `combat.rs` (brain_input), the `Senses` field says so. The `[assumed]` kinds are
/// the ones the reads mark UNVERIFIED or doubtful; see their arms in `value`.
pub const VALUE_KINDS: [ValueKind; 26] = [
    ValueKind { hash: 0xda9bec13, meaning: "distance to the nearest foe, m", bound: true },
    ValueKind { hash: 0xe056d012, meaning: "line of sight to the nearest foe (1 or no value)", bound: true },
    ValueKind { hash: 0x621332f0, meaning: "in vehicle motion (1 or no value)", bound: true },
    ValueKind { hash: 0x843fc6db, meaning: "distance to the nearest group target, m", bound: true },
    ValueKind { hash: 0x0cf5b934, meaning: "line of sight to the group target (1 or no value)", bound: true },
    ValueKind { hash: 0x5d06c0ee, meaning: "OnGround: 1 when grounded and not in the fall form (fifth read 1)", bound: true },
    ValueKind { hash: 0xc9e0a56c, meaning: "OutsideHomeRegion: 0 inside, else a distance m, four gates (fourth read 2)", bound: true },
    ValueKind { hash: 0xfc1c5893, meaning: "distance to the home point, m (no value without one)", bound: true },
    ValueKind { hash: 0xfbd14a89, meaning: "MyselfHasBeenHit: 1 when bit 5 of char+0x14c is set (fourth read 3)", bound: true },
    ValueKind { hash: 0x6b3394c7, meaning: "FoeWithinSight: 1 when the foe is in the sight cone (fifth read 1)", bound: true },
    ValueKind { hash: 0x712ecdc7, meaning: "group member distance m (fifth read 4) [assumed]", bound: true },
    ValueKind { hash: 0x8fd8eb5d, meaning: "HasPath: 1 when the controller has a path record (fourth read 6)", bound: true },
    ValueKind { hash: 0xa62df181, meaning: "distance to the tactics point m, 0 at the point (fifth read 5)", bound: true },
    ValueKind { hash: 0x7b35ef2b, meaning: "FoeAttackFast: 1 when the foe's attack type is the flight slam (fifth read 2) [assumed]", bound: true },
    ValueKind { hash: 0x218a1421, meaning: "FoeAttackWeapon: 1 for the foe's weapon attack (fifth read 3) [assumed]", bound: true },
    ValueKind { hash: 0x27bb0c2e, meaning: "health as a percent of maximum, 0..100", bound: true },
    ValueKind { hash: 0x188e1abe, meaning: "MyselfNearGround: 0 when grounded or near the ground (fifth read 6)", bound: true },
    ValueKind { hash: 0x1d489c55, meaning: "MyselfLineOfSight: 1 on the nav corridor test (fifth read 7)", bound: true },
    ValueKind { hash: 0x192301ae, meaning: "distance to the nearest team object, m [assumed]", bound: true },
    ValueKind { hash: 0x0acb9809, meaning: "AttackObjLineOfSight: 1 when the group member is in sight (fifth read 8) [assumed]", bound: true },
    ValueKind { hash: 0x22d75963, meaning: "FriendInRange: distance m to the nearest friend (fifth read 9) [assumed]", bound: true },
    ValueKind { hash: 0xb85859da, meaning: "FoeAmbushMode: 1 or 0 with a foe, no value without (fifth read 10)", bound: true },
    ValueKind { hash: 0x5ada9b7c, meaning: "Range: 0.001 when hit and the reaction clock is over (fourth read 8, fifth read note)", bound: true },
    ValueKind { hash: 0x26e128c3, meaning: "MyselfOverheatFullWeapon1: 100 times the weapon's heat fraction (fifth read 11) [assumed]", bound: true },
    ValueKind { hash: 0x2fe0758f, meaning: "MyselfHovering: 1 in a flying form (fifth read 12) [assumed]", bound: true },
    ValueKind { hash: 0x1529732e, meaning: "FoeStunned: 1 when the foe's hit reaction float is above 0 (fifth read 13)", bound: true },
];

/// True when the port computes this valueType (a [stand-in] otherwise).
pub fn value_kind_bound(value_type: u32) -> bool {
    VALUE_KINDS.iter().any(|k| k.hash == value_type && k.bound)
}

/// The assigned threshold of an assignedPerception hash: the attribute the hash names, read from
/// `attr` (FUN_008250a0 [game]: a float load per hash, from the AI attribute block at character+0x140).
/// An unknown hash gives 0.0, as the game does [game]. The hash-to-field map is the third read's
/// table A (the loader's field names).
pub fn assigned(hash: u32, attr: &AiAttr) -> f32 {
    match hash {
        0x37f13d56 => attr.attack_range_distance,
        0xbf7ab96a => attr.attack_counter_distance,
        0x846c8fb6 => attr.flee_distance,
        0x9c096a7c => attr.dodge_distance,
        0xb748d69a => attr.attack_range2_distance,
        0xde5d12f8 => attr.idle_distance,
        0xec1954c8 => attr.strafe_distance,
        0x2908a6ef => attr.alert_distance,
        0x595f2a5a => attr.ambush_distance,
        0x3e19fbef => attr.vehicle_distance,
        0x439e2241 => attr.chase_distance_min,
        0x463cf345 => attr.back_pedal_distance,
        0x7320a9ff => attr.attack_special_distance,
        0x7339f804 => attr.attack_melee_distance,
        0x7f931d18 => attr.chase_distance_max,
        _ => 0.0,
    }
}

/// The value getter 00830360 (VALUE) for a valueType hash [game]. `group_id` is the perception's
/// groupID, passed to the group kinds. None is "no value" (FLT_MAX in the game), which scores 0.
/// The kinds that are not bound return None: a [stand-in].
pub fn value(value_type: u32, group_id: u32, ctx: &Ctx) -> Option<f32> {
    let s = &ctx.senses;
    match value_type {
        // da9bec13: the distance to the nearest foe [game 00823220 search, manager ruling].
        0xda9bec13 => s.foe_distance,
        // e056d012: line of sight to that foe: 1.0 when seen, else no value [assumed float; the
        // data compares it with V > 0].
        0xe056d012 => s.foe_distance.and(s.foe_visible.then_some(1.0)),
        // 843fc6db: the distance to the group target of the perception's groupID [game 0083068b..].
        0x843fc6db => s.group_target_distance(group_id),
        // 0cf5b934: 1.0 when the group target is in line of sight, else no value [game 00830efa..].
        0x0cf5b934 => s.group_target_visible(group_id).then_some(1.0),
        // fc1c5893: the distance to the home point [game 00830d22..]; no value with no home point.
        0xfc1c5893 => s.home_distance,
        // 27bb0c2e: health as a percent of maximum [game 00831664..].
        0x27bb0c2e => Some(s.health_percent),
        // 621332f0: 1.0 in vehicle motion, else no value [game 00831853..].
        0x621332f0 => s.in_vehicle_motion.then_some(1.0),
        // 192301ae: the distance to the nearest team object [game 0083144e..; assumed type].
        0x192301ae => s.team_object_distance,
        // 5d06c0ee (OnGround): 1 when grounded and not in the fall form, else no value [game
        // 00831780..00831841, fifth read 1 and fourth read 1]. The fall form test is [assumed].
        0x5d06c0ee => (s.on_ground && !s.falling_form).then_some(1.0),
        // c9e0a56c (OutsideHomeRegion) [game 0083097c..00830a73; fourth read 2]. The gates in order:
        // no home region: no value. Without a tactics record (or its +4 byte, or its goal name), the
        // home box decides: inside gives 0.0, else the home point's distance. With the tactics record
        // and its name: at the point gives 0.0, unset gives no value, else the tactics point's distance.
        0xc9e0a56c => {
            if !s.has_home_region {
                return None;
            }
            if !(s.tactics_point_valid && s.tactics_goal_named) {
                return if s.inside_home_region { Some(0.0) } else { s.home_point_distance };
            }
            if s.tactics_at_point {
                Some(0.0)
            } else if s.tactics_point_unset {
                None
            } else {
                s.tactics_point_distance
            }
        }
        // fbd14a89 (MyselfHasBeenHit): 1 when bit 5 of char+0x14c is set, else no value [game
        // 00830c6c..00830c90, fourth read 3]. The flag's name is [assumed].
        0xfbd14a89 => s.was_hit.then_some(1.0),
        // 6b3394c7 (FoeWithinSight): 1 when the nearest foe is in the sight cone [game 00831929..
        // 00831a0b, fifth read 1]. No foe: no value.
        0x6b3394c7 => s.foe_distance.and(s.foe_in_sight_cone.then_some(1.0)),
        // 712ecdc7 (group member distance) [game 0083193f..0083196e, fifth read 4]. [assumed] the
        // point (00555e90) and the return of 00831b20 are unverified.
        0x712ecdc7 => s.group_member_distance(group_id),
        // 8fd8eb5d (HasPath): 1 when the controller's path record is set [game 008307e3..008307f2,
        // fourth read 6]. [assumed] the name "has a path".
        0x8fd8eb5d => s.has_path.then_some(1.0),
        // a62df181 (distance to the tactics point) [game 008308b6..00830963, fifth read 5]: at the
        // point gives 0.0; unset (or no record) gives no value; else the 3D distance.
        0xa62df181 => {
            if s.tactics_at_point {
                Some(0.0)
            } else if s.tactics_point_unset {
                None
            } else {
                s.tactics_point_distance
            }
        }
        // 7b35ef2b (FoeAttackFast): 1 when the foe's attack type is the flight slam [game
        // 00831a5c..00831aad, fifth read 2]. [assumed] the name "fast" does not match the data and
        // the clip's attack type path is UNVERIFIED. No foe: no value.
        0x7b35ef2b => s.foe_distance.and(s.foe_attack_fast.then_some(1.0)),
        // 218a1421 (FoeAttackWeapon) [game 008314dd..00831591, fifth read 3]. The player branch is
        // UNVERIFIED (the input test's return). [assumed] the fact is the weapon test. No foe: no value.
        0x218a1421 => s.foe_distance.and(s.foe_attack_weapon.then_some(1.0)),
        // 188e1abe (MyselfNearGround) [game 00831306..008313c7, fifth read 6]: 0.0 when grounded or
        // at the ground; else no value. The height test's inputs are [stand-in] (see combat.rs).
        0x188e1abe => (s.on_ground || s.near_ground).then_some(0.0),
        // 1d489c55 (MyselfLineOfSight) [game 008313d2..0083144b, fifth read 7]: 1 on the nav corridor
        // test to the foe. No foe: no value.
        0x1d489c55 => s.foe_distance.and(s.own_line_of_sight.then_some(1.0)),
        // 0acb9809 (AttackObjLineOfSight) [game 00830e43..00830e93, fifth read 8]: 1 when the group
        // member is in line of sight. The rest of the branch is UNREAD [assumed].
        0x0acb9809 => s.group_member_visible(group_id).then_some(1.0),
        // 22d75963 (FriendInRange) [game 008315d5..0083161e, fifth read 9]: the nearest friend's
        // distance. The 00822c10 branch is NOT VERIFIED and the friend definition is [unknown].
        0x22d75963 => s.friend_distance,
        // b85859da (FoeAmbushMode) [game 00830a76..00830ac4, fifth read 10]: 1 or 0 with a foe, no
        // value without one. The ambush test (00824430) is [assumed].
        0xb85859da => s.foe_distance.map(|_| if s.foe_ambush { 1.0 } else { 0.0 }),
        // 5ada9b7c (Range) [game 00831799..00831841, settled 2026-10-13 in notes/mission-attack-escort-trace.md]:
        // no value (FLT_MAX) unless bit 5 of char+0x14c is set (was_hit) and state+0xc - char+0x1f0 <= 0
        // (hit_reaction_over: inside the hit window); then 0.001. Range 0..15 scores 1 for it, and the
        // brain-level link is inverted, so an undamaged enemy passes and a hit one is rejected.
        0x5ada9b7c => (s.was_hit && s.hit_reaction_over).then_some(0.001),
        // 26e128c3 (MyselfOverheatFullWeapon1) [game 008315b7..008315d2, fifth read 11]: 100 times the
        // heat fraction; no value without a weapon. [assumed] the fraction's meaning.
        0x26e128c3 => s.weapon_overheat_percent,
        // 2fe0758f (MyselfHovering) [game 0083172a..0083172f, fifth read 12]: 1 in a flying form.
        0x2fe0758f => s.flying_form.then_some(1.0),
        // 1529732e (FoeStunned) [game 008312ac..00831303, fifth read 13]: 1 when the foe's hit
        // reaction float is above 0. No foe: no value.
        0x1529732e => s.foe_distance.and(s.foe_stunned.then_some(1.0)),
        // [stand-in] any other valueType: not in VALUE_KINDS. No value.
        _ => None,
    }
}

/// The compare data of a Compare, Targeting (sub-object `+0x24` / `+0x4c`), filled from the data's
/// `compareData` [game 0082adb0].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompareData {
    /// sub+0x4: compareValue [game].
    pub compare_value: f32,
    /// sub+0x8: compareValueInBehavior. -1.0 means unused [game 0082ae90].
    pub compare_value_in_behaviour: f32,
    /// sub+0xc: assignedPerception hash, 0 for none [game].
    pub assigned: u32,
    /// sub+0x10: compareType hash [game].
    pub compare_type: u32,
}

/// PRED 0082ae90 [game]: the compare of a value V against the threshold T.
pub fn predicate(data: &CompareData, ctx: &Ctx, v: f32) -> bool {
    // T: the assigned attribute when assignedPerception is set (FUN_008250a0 [game]; third read,
    // "compareValue is the threshold only where assignedPerception is false").
    // [assumed] the assigned threshold ignores compareValueInBehavior: the game's T is read only
    // as FUN_008250a0's result.
    let t = if data.assigned != 0 {
        assigned(data.assigned, &ctx.attr)
    } else if ctx.in_behaviour && data.compare_value_in_behaviour != -1.0 {
        // T is compareValueInBehavior when the controller is in a behaviour and that value is set
        // (not -1.0) [game 0082ae90, disasm checked]; else compareValue.
        data.compare_value_in_behaviour
    } else {
        data.compare_value
    };
    match data.compare_type {
        CMP_ABOVE => v > t,
        CMP_BELOW => v < t,
        _ => false,
    }
}

/// PerceptionCompare (56 objects in the data) [game 0082ed70]: 1 when the value passes the
/// predicate, else 0. No value scores 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Compare {
    pub value_type: u32,
    /// The data's groupID, passed to the value getter (the group kinds use it) [game].
    pub group_id: u32,
    pub data: CompareData,
}

impl Perception for Compare {
    fn score(&self, ctx: &Ctx) -> f32 {
        match value(self.value_type, self.group_id, ctx) {
            Some(v) if predicate(&self.data, ctx, v) => 1.0,
            _ => 0.0,
        }
    }
}

/// PerceptionRange (2 objects) [game 0082fb10]: 1 when lower < V < upper, both strict.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub value_type: u32,
    /// The data's groupID, passed to the value getter [game].
    pub group_id: u32,
    /// P+0x24 compareValueMin.
    pub min: f32,
    /// P+0x2c compareValueMax.
    pub max: f32,
    /// P+0x28 assignedPerceptionMin, 0 for none.
    pub assigned_min: u32,
    /// P+0x30 assignedPerceptionMax, 0 for none.
    pub assigned_max: u32,
}

impl Perception for Range {
    fn score(&self, ctx: &Ctx) -> f32 {
        // An assigned bound replaces its compareValue bound (FUN_008250a0 [game]; [assumed] the
        // same rule as Compare).
        let lower = if self.assigned_min != 0 { assigned(self.assigned_min, &ctx.attr) } else { self.min };
        let upper = if self.assigned_max != 0 { assigned(self.assigned_max, &ctx.attr) } else { self.max };
        match value(self.value_type, self.group_id, ctx) {
            Some(v) if v > lower && v < upper => 1.0,
            _ => 0.0,
        }
    }
}

/// PerceptionRandom (5 objects) [game 0082f890]: a draw of 0..100 against the percentage. The
/// draw is `ctx.random` (a unit draw) times 100; the caller fills it once per evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Random {
    /// P+0x1c percentage [data]. [assumed] percent units (0.5 means a 0.5 percent chance).
    pub percentage: f32,
}

impl Perception for Random {
    fn score(&self, ctx: &Ctx) -> f32 {
        let r = ctx.random * 100.0;
        if r <= self.percentage {
            1.0
        } else {
            0.0
        }
    }
}

/// PerceptionTimer (1 object) [game 00831f90]: the flag set at load from startOn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timer {
    /// P+0x1c timeOn [data]. Not used by any code read [unknown].
    pub time_on: f32,
    /// P+0x20 timeOff [data]. Not used by any code read [unknown].
    pub time_off: f32,
    /// P+0x2c: 1.0 when startOn was set. The writer after the load is [unknown] (unread), so the
    /// flag stays what the load set.
    flag: bool,
}

impl Timer {
    pub fn new(time_on: f32, time_off: f32, start_on: bool) -> Self {
        Self {
            time_on,
            time_off,
            flag: start_on,
        }
    }
}

impl Perception for Timer {
    fn score(&self, _ctx: &Ctx) -> f32 {
        if self.flag {
            1.0
        } else {
            0.0
        }
    }
}

/// One target specification of a Targeting perception (the 0x18-byte record of 0082bd20) [game].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetSpec {
    /// targetType hash (0xae97c318 field).
    pub target_type: u32,
    /// targetGroupID hash.
    pub group_id: u32,
    /// targetCalc hash.
    pub calc: u32,
    /// targetCalcRangeMin, metres.
    pub range_min: f32,
    /// targetCalcRangeMax, metres. 0 when absent in the data [data]; the game's handling of that
    /// case is [unknown].
    pub range_max: f32,
}

/// The target choice 008228d0 for one spec [game 00830140 step 2a]: the distance of the object the
/// spec picks, when it is inside the spec's range (both inclusive, the 00822c10 accept test [game]).
/// The distance is `Senses::target_distance` (FOE, TURRET, FRIEND). [stand-in] every other type
/// (the team, home, waypoint and special types) has no port here: no distance, so None.
fn choose(spec: &TargetSpec, ctx: &Ctx) -> Option<f32> {
    let d = ctx.senses.target_distance(spec.target_type, spec.group_id)?;
    (spec.range_min <= d && d <= spec.range_max).then_some(d)
}

/// True when this port scores a Targeting of this target type (`choose`): FOE, TURRET, FRIEND and
/// GROUP (the group lookup, fed from the Escort group input). [assumed] the TARGET_FRIEND hash
/// (`ai_move::TARGET_FRIEND`) follows the naming of the others.
pub fn target_type_bound(target_type: u32) -> bool {
    matches!(target_type, TARGET_FOE | TARGET_TURRET | TARGET_FRIEND | crate::ai_brain::TARGET_GROUP)
}

/// PerceptionTargeting (2 objects) [game 00830140].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Targeting {
    /// Target spec 1 (P+0x1c).
    pub spec: TargetSpec,
    /// Target spec 2 (P+0x34), used when compare_with_second is set.
    pub spec2: TargetSpec,
    /// P+0x60 compareWithSecondTarget.
    pub compare_with_second: bool,
    /// The compare data (P+0x4c).
    pub data: CompareData,
}

impl Targeting {
    /// True when every target type this perception uses is ported (`target_type_bound`).
    pub fn bound(&self) -> bool {
        target_type_bound(self.spec.target_type) && (!self.compare_with_second || target_type_bound(self.spec2.target_type))
    }
}

impl Perception for Targeting {
    /// 00830140 [game, disasm checked, ai-targeting-idle.md A.1]. Single target: the predicate on
    /// the first target's distance. Two targets (`compare_with_second`): O2 missing scores 0; O1
    /// missing with O2 present scores 1 at once; both present compare V = d(O2) - d(O1) with the
    /// predicate. A target type with no port scores 0 [stand-in].
    fn score(&self, ctx: &Ctx) -> f32 {
        if !self.bound() {
            return 0.0;
        }
        if !self.compare_with_second {
            return match choose(&self.spec, ctx) {
                Some(d) if predicate(&self.data, ctx, d) => 1.0,
                _ => 0.0,
            };
        }
        match (choose(&self.spec, ctx), choose(&self.spec2, ctx)) {
            (_, None) => 0.0,
            (None, Some(_)) => 1.0,
            (Some(d1), Some(d2)) => {
                if predicate(&self.data, ctx, d2 - d1) {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

/// PerceptionMessage (36 objects in the data) [game 0082f180, 0082f200, 0082f2e0]. Its latch is a
/// node in the controller's store (`ai_brain::Latches`, the node at `id`): it is set by the
/// delivery of messages (`Ctx::send` / `Ctx::emit`) and is alive while its brain is registered.
#[derive(Debug, Clone, PartialEq)]
pub struct MessagePerception {
    /// The index of this perception's latch node in the controller's store.
    pub id: usize,
    /// P+0x1c msgNameOn.
    pub name_on: u32,
    /// P+0x20 msgNameOff.
    pub name_off: u32,
    /// P+0x28 msgToggle: the off word wins when set.
    pub toggle: bool,
}

impl MessagePerception {
    /// `id` is the node the binder made (`Latches::add`); `timer` and `toggle_on` are kept there.
    pub fn new(id: usize, name_on: u32, name_off: u32, toggle: bool) -> Self {
        Self {
            id,
            name_on,
            name_off,
            toggle,
        }
    }
}

impl Perception for MessagePerception {
    /// 0082f180 [game]: no node (not registered) gives 0. Then the controller word (ctrl+0xec, when
    /// set) decides: the on word gives 1, and with msgToggle the off word gives 0. Else the flag.
    fn score(&self, ctx: &Ctx) -> f32 {
        let flag = {
            let store = lock_latches(&ctx.latches);
            match store.nodes.get(self.id) {
                Some(node) if node.registered => node.flag,
                _ => return 0.0,
            }
        };
        let w = ctx.ctrl_word;
        if w != 0 {
            if w == self.name_on {
                return 1.0;
            }
            if self.toggle && w == self.name_off {
                return 0.0;
            }
        }
        if flag {
            1.0
        } else {
            0.0
        }
    }
}

/// One link of an entry: a perception, and the message it sends to the character when it scores
/// (`link+0x18` name, `+0x1c` argument; 0 = none) [game 0082e570]. The link's fuzzy byte
/// (`link+0x14`) is not ported: no shipped perception sets it [stand-in: unused by the data].
pub struct Link {
    pub perception: Box<dyn Perception>,
    pub message: u32,
    pub arg: u32,
    /// The link's invert byte (`link+0x14`, the data's invertPriority) [game 0082e570 disasm
    /// 0082e5a5..0082e5f1, notes/ai-brain.md R2]: the score is inverted after its message is sent.
    pub invert: bool,
}

impl Link {
    /// The link's score after its invert byte (R2) [game]: with the byte set, 1 gives 0, 0 gives 1,
    /// and any other score gives 1 - S, clamped to 0..1. Without it, the score is unchanged.
    pub fn inverted(&self, raw: f32) -> f32 {
        if !self.invert {
            return raw;
        }
        if raw == 1.0 {
            0.0
        } else if raw == 0.0 {
            1.0
        } else {
            (1.0 - raw).clamp(0.0, 1.0)
        }
    }
}

/// The perception aggregate of an entry (0082e7e0) [game]: AVG, MAX or MIN over the links that
/// score above 0, with REQUIRE making any link at 0 or below reject the entry.
pub struct Aggregate {
    pub links: Vec<Link>,
    /// G+0x20: the calculation hash (CALC_AVG, CALC_MAX, CALC_MIN; anything else scores 0).
    pub calc: u32,
    /// G+0x24: the condition hash (COND_REQUIRE, or none).
    pub condition: u32,
}

impl Aggregate {
    pub fn new(calc: u32, condition: u32, links: Vec<Link>) -> Self {
        Self {
            links,
            calc,
            condition,
        }
    }

    /// The enter walk (0082e8f0) [game]: each link's perception enters.
    pub fn enter(&mut self, ctx: &Ctx) {
        for link in &mut self.links {
            link.perception.enter(ctx);
        }
    }

    /// The leave walk (vtable +0x18; its caller is [unknown]).
    pub fn leave(&mut self, ctx: &Ctx) {
        for link in &mut self.links {
            link.perception.leave(ctx);
        }
    }

    /// The aggregate score 0082e7e0 [game, disasm checked]:
    /// 1. no links: 1.
    /// 2. each link scores; a score above 0 with a message sends it (`Ctx::emit`).
    /// 3. a score at 0 or below: with REQUIRE, the entry is 0 at once; otherwise the link is skipped.
    /// 4. no link counted: 0. AVG is the mean; MAX and MIN the best; any other calc is 0.
    pub fn score(&self, ctx: &Ctx) -> f32 {
        if self.links.is_empty() {
            return 1.0;
        }
        let mut best = if self.calc == CALC_MIN { f32::MAX } else { 0.0 };
        let mut sum = 0.0_f32;
        let mut count = 0_u32;
        for link in &self.links {
            // The message goes out on the raw score; the invert byte applies after it (R2).
            let raw = link.perception.score(ctx);
            if raw != 0.0 && link.message != 0 {
                ctx.emit(link.message, link.arg);
            }
            let s = link.inverted(raw);
            if s <= 0.0 {
                if self.condition == COND_REQUIRE {
                    return 0.0;
                }
                continue;
            }
            count += 1;
            match self.calc {
                CALC_AVG => sum += s,
                CALC_MAX => {
                    if s > best {
                        best = s;
                    }
                }
                CALC_MIN => {
                    if best > s {
                        best = s;
                    }
                }
                _ => {}
            }
        }
        if count == 0 {
            return 0.0;
        }
        if self.calc == CALC_AVG {
            sum / count as f32
        } else {
            best
        }
    }

    /// The aggregate as the brain's scorer (`Entry::scorer`).
    pub fn into_scorer(self) -> Scorer {
        Box::new(move |ctx: &Ctx| self.score(ctx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A perception with a fixed graded score, for the aggregate tests.
    struct Graded(f32);

    impl Perception for Graded {
        fn score(&self, _ctx: &Ctx) -> f32 {
            self.0
        }
    }

    fn link(p: impl Perception + 'static) -> Link {
        Link {
            perception: Box::new(p),
            message: 0,
            arg: 0,
            invert: false,
        }
    }

    #[test]
    fn inverted_link_flips_its_score_after_the_message() {
        let mut ctx = Ctx::new();
        let mut on = link(Graded(1.0));
        on.invert = true;
        on.message = 42;
        on.arg = 7;
        let mut half = link(Graded(0.25));
        half.invert = true;
        let mut zero = link(Graded(0.0));
        zero.invert = true;
        assert_eq!(Aggregate::new(CALC_AVG, 0, vec![on]).score(&ctx), 0.0, "1 inverts to 0");
        assert_eq!(Aggregate::new(CALC_AVG, 0, vec![zero]).score(&ctx), 1.0, "0 inverts to 1");
        assert_eq!(Aggregate::new(CALC_AVG, 0, vec![half]).score(&ctx), 0.75, "1 - 0.25");
        assert_eq!(ctx.drain(), vec![(42, 7)], "the message goes out on the raw score 1");
    }

    /// A context with one foe at `d`, in line of sight (the ctrl+0x68 target and the senses).
    fn ctx_with_distance(d: f32) -> Ctx {
        let mut ctx = Ctx::new();
        ctx.target = Some(crate::ai_brain::Target {
            pos: [0.0; 3],
            distance: d,
            team: 2,
        });
        ctx.senses.foe_distance = Some(d);
        ctx.senses.foe_visible = true;
        ctx
    }

    fn cmp(value_type: u32, value: f32, behaviour: f32, assigned: u32, kind: u32) -> Compare {
        Compare {
            value_type,
            group_id: 0,
            data: CompareData {
                compare_value: value,
                compare_value_in_behaviour: behaviour,
                assigned,
                compare_type: kind,
            },
        }
    }

    fn spec(target_type: u32, min: f32, max: f32) -> TargetSpec {
        TargetSpec {
            target_type,
            group_id: 0,
            calc: 0,
            range_min: min,
            range_max: max,
        }
    }

    #[test]
    fn compare_distance_is_below_threshold() {
        let ctx = ctx_with_distance(3.0);
        assert_eq!(cmp(0xda9bec13, 5.0, -1.0, 0, CMP_BELOW).score(&ctx), 1.0);
        assert_eq!(cmp(0xda9bec13, 2.0, -1.0, 0, CMP_BELOW).score(&ctx), 0.0);
        assert_eq!(cmp(0xda9bec13, 2.0, -1.0, 0, CMP_ABOVE).score(&ctx), 1.0);
    }

    #[test]
    fn compare_uses_behaviour_threshold_only_in_a_behaviour() {
        let mut ctx = ctx_with_distance(3.0);
        let c = cmp(0xda9bec13, 5.0, 2.0, 0, CMP_BELOW);
        assert_eq!(c.score(&ctx), 1.0, "not in a behaviour: compareValue 5");
        ctx.in_behaviour = true;
        assert_eq!(c.score(&ctx), 0.0, "in a behaviour: compareValueInBehavior 2");
        let unused = cmp(0xda9bec13, 5.0, -1.0, 0, CMP_BELOW);
        assert_eq!(unused.score(&ctx), 1.0, "-1 means unused: compareValue");
    }

    #[test]
    fn compare_scores_zero_for_no_value_and_unknown_compare_type() {
        let ctx = ctx_with_distance(3.0);
        assert_eq!(cmp(0x12345678, 2.0, -1.0, 0, CMP_ABOVE).score(&ctx), 0.0, "[stand-in] value kind: no value");
        assert_eq!(cmp(0xda9bec13, 2.0, -1.0, 0, 0x12345678).score(&ctx), 0.0, "unknown compareType");
        assert_eq!(cmp(0xda9bec13, 5.0, -1.0, 0, CMP_BELOW).score(&Ctx::new()), 0.0, "no foe: no value");
    }

    #[test]
    fn value_kinds_present_and_no_value() {
        // da9bec13: distance, or no value without a foe.
        let mut c = Ctx::new();
        c.senses.foe_distance = Some(7.5);
        assert_eq!(value(0xda9bec13, 0, &c), Some(7.5));
        assert_eq!(value(0xda9bec13, 0, &Ctx::new()), None);
        // e056d012: 1 in line of sight to the foe, else no value.
        c.senses.foe_visible = true;
        assert_eq!(value(0xe056d012, 0, &c), Some(1.0));
        c.senses.foe_visible = false;
        assert_eq!(value(0xe056d012, 0, &c), None);
        // 843fc6db: the group target's distance for the groupID, else no value.
        c.senses.group_targets = vec![(0, 12.0, false), (5, 3.0, true)];
        assert_eq!(value(0x843fc6db, 0, &c), Some(12.0));
        assert_eq!(value(0x843fc6db, 5, &c), Some(3.0));
        assert_eq!(value(0x843fc6db, 9, &c), None);
        // 0cf5b934: 1 when the group target is in line of sight, else no value.
        assert_eq!(value(0x0cf5b934, 5, &c), Some(1.0));
        assert_eq!(value(0x0cf5b934, 0, &c), None, "group 0 is not in sight");
        // fc1c5893: the home distance, no value without a home point.
        c.senses.home_distance = Some(4.0);
        assert_eq!(value(0xfc1c5893, 0, &c), Some(4.0));
        assert_eq!(value(0xfc1c5893, 0, &Ctx::new()), None);
        // 27bb0c2e: the health percent (always a value).
        c.senses.health_percent = 45.0;
        assert_eq!(value(0x27bb0c2e, 0, &c), Some(45.0));
        assert_eq!(value(0x27bb0c2e, 0, &Ctx::new()), Some(100.0), "a fresh character is at 100");
        // 621332f0: 1 in vehicle motion, else no value.
        c.senses.in_vehicle_motion = true;
        assert_eq!(value(0x621332f0, 0, &c), Some(1.0));
        c.senses.in_vehicle_motion = false;
        assert_eq!(value(0x621332f0, 0, &c), None);
        // 192301ae: the team object's distance, no value without one.
        c.senses.team_object_distance = Some(9.0);
        assert_eq!(value(0x192301ae, 0, &c), Some(9.0));
        assert_eq!(value(0x192301ae, 0, &Ctx::new()), None);
    }

    #[test]
    fn all_26_value_kinds_are_bound() {
        assert_eq!(VALUE_KINDS.len(), 26);
        assert!(VALUE_KINDS.iter().all(|k| k.bound), "every shipped valueType is bound");
        assert!(VALUE_KINDS.iter().all(|k| value_kind_bound(k.hash)));
        assert_eq!(value(0x12345678, 0, &ctx_with_distance(3.0)), None, "an unknown hash: no value");
    }

    /// A context with the foe at 4 m, in sight, and nothing else sensed.
    fn foe_ctx() -> Ctx {
        ctx_with_distance(4.0)
    }

    #[test]
    fn on_ground_is_one_when_grounded_and_not_falling() {
        let mut c = foe_ctx();
        c.senses.on_ground = true;
        assert_eq!(value(0x5d06c0ee, 0, &c), Some(1.0));
        c.senses.falling_form = true;
        assert_eq!(value(0x5d06c0ee, 0, &c), None, "the fall form: no value");
        c.senses.falling_form = false;
        c.senses.on_ground = false;
        assert_eq!(value(0x5d06c0ee, 0, &c), None, "in the air: no value");
    }

    #[test]
    fn was_hit_is_one_when_hit_else_no_value() {
        let mut c = Ctx::new();
        assert_eq!(value(0xfbd14a89, 0, &c), None);
        c.senses.was_hit = true;
        assert_eq!(value(0xfbd14a89, 0, &c), Some(1.0));
        // 5ada9b7c: 0.001 only with a hit AND inside the hit window (hit_reaction_over true).
        assert_eq!(value(0x5ada9b7c, 0, &c), None, "hit but outside the hit window");
        c.senses.hit_reaction_over = true;
        assert_eq!(value(0x5ada9b7c, 0, &c), Some(0.001));
        c.senses.was_hit = false;
        assert_eq!(value(0x5ada9b7c, 0, &c), None, "no hit: no value");
    }

    #[test]
    fn foe_within_sight_needs_a_foe_in_the_cone() {
        let mut c = foe_ctx();
        assert_eq!(value(0x6b3394c7, 0, &c), None, "in the cone is not set");
        c.senses.foe_in_sight_cone = true;
        assert_eq!(value(0x6b3394c7, 0, &c), Some(1.0));
        assert_eq!(value(0x6b3394c7, 0, &Ctx::new()), None, "no foe: no value");
    }

    #[test]
    fn has_path_is_one_when_the_controller_has_a_path() {
        let mut c = Ctx::new();
        assert_eq!(value(0x8fd8eb5d, 0, &c), None);
        c.senses.has_path = true;
        assert_eq!(value(0x8fd8eb5d, 0, &c), Some(1.0));
    }

    #[test]
    fn foe_attack_kinds_need_a_foe() {
        let mut c = foe_ctx();
        assert_eq!(value(0x7b35ef2b, 0, &c), None);
        assert_eq!(value(0x218a1421, 0, &c), None);
        c.senses.foe_attack_fast = true;
        c.senses.foe_attack_weapon = true;
        assert_eq!(value(0x7b35ef2b, 0, &c), Some(1.0));
        assert_eq!(value(0x218a1421, 0, &c), Some(1.0));
        assert_eq!(value(0x7b35ef2b, 0, &Ctx::new()), None, "no foe: no value");
        assert_eq!(value(0x218a1421, 0, &Ctx::new()), None, "no foe: no value");
    }

    #[test]
    fn near_ground_is_zero_when_grounded_or_near_else_no_value() {
        let mut c = Ctx::new();
        assert_eq!(value(0x188e1abe, 0, &c), None, "in the air and not near the ground");
        c.senses.on_ground = true;
        assert_eq!(value(0x188e1abe, 0, &c), Some(0.0), "grounded gives 0.0, a value");
        c.senses.on_ground = false;
        c.senses.near_ground = true;
        assert_eq!(value(0x188e1abe, 0, &c), Some(0.0), "near the ground gives 0.0");
    }

    #[test]
    fn own_line_of_sight_is_one_with_a_foe_in_the_corridor_test() {
        let mut c = foe_ctx();
        assert_eq!(value(0x1d489c55, 0, &c), None);
        c.senses.own_line_of_sight = true;
        assert_eq!(value(0x1d489c55, 0, &c), Some(1.0));
        assert_eq!(value(0x1d489c55, 0, &Ctx::new()), None, "no foe: no value");
    }

    #[test]
    fn group_member_distance_and_sight_follow_the_group() {
        let mut c = Ctx::new();
        c.senses.group_members = vec![(5, 3.0, true), (7, 8.0, false)];
        assert_eq!(value(0x712ecdc7, 5, &c), Some(3.0), "group 5's nearest member");
        assert_eq!(value(0x712ecdc7, 9, &c), None, "no member of group 9");
        assert_eq!(value(0x0acb9809, 5, &c), Some(1.0), "group 5's member is in sight");
        assert_eq!(value(0x0acb9809, 7, &c), None, "group 7's member is not in sight");
    }

    #[test]
    fn friend_distance_is_the_value_or_no_value() {
        let mut c = Ctx::new();
        assert_eq!(value(0x22d75963, 0, &c), None);
        c.senses.friend_distance = Some(4.5);
        assert_eq!(value(0x22d75963, 0, &c), Some(4.5));
    }

    #[test]
    fn ambush_is_zero_or_one_with_a_foe_and_no_value_without() {
        let mut c = foe_ctx();
        assert_eq!(value(0xb85859da, 0, &c), Some(0.0), "a foe not in ambush: 0.0, a value");
        c.senses.foe_ambush = true;
        assert_eq!(value(0xb85859da, 0, &c), Some(1.0));
        assert_eq!(value(0xb85859da, 0, &Ctx::new()), None, "no foe: no value");
    }

    #[test]
    fn weapon_overheat_is_the_percent_or_no_value() {
        let mut c = Ctx::new();
        assert_eq!(value(0x26e128c3, 0, &c), None, "no weapon: no value");
        c.senses.weapon_overheat_percent = Some(40.0);
        assert_eq!(value(0x26e128c3, 0, &c), Some(40.0));
    }

    #[test]
    fn flying_form_is_one_in_a_flying_form() {
        let mut c = Ctx::new();
        assert_eq!(value(0x2fe0758f, 0, &c), None);
        c.senses.flying_form = true;
        assert_eq!(value(0x2fe0758f, 0, &c), Some(1.0));
    }

    #[test]
    fn foe_stunned_is_one_when_the_foe_is_stunned() {
        let mut c = foe_ctx();
        assert_eq!(value(0x1529732e, 0, &c), None);
        c.senses.foe_stunned = true;
        assert_eq!(value(0x1529732e, 0, &c), Some(1.0));
        assert_eq!(value(0x1529732e, 0, &Ctx::new()), None, "no foe: no value");
    }

    #[test]
    fn home_region_distance_follows_the_gates() {
        let mut c = Ctx::new();
        assert_eq!(value(0xc9e0a56c, 0, &c), None, "no home region: no value");
        c.senses.has_home_region = true;
        c.senses.home_point_distance = Some(6.0);
        assert_eq!(value(0xc9e0a56c, 0, &c), Some(6.0), "outside the box: the home distance");
        c.senses.inside_home_region = true;
        assert_eq!(value(0xc9e0a56c, 0, &c), Some(0.0), "inside the box: 0.0");
        c.senses.inside_home_region = false;
        c.senses.home_point_distance = None;
        assert_eq!(value(0xc9e0a56c, 0, &c), None, "no home point: no value");
    }

    #[test]
    fn home_region_uses_the_tactics_point_when_it_is_named() {
        let mut c = Ctx::new();
        c.senses.has_home_region = true;
        c.senses.home_point_distance = Some(6.0);
        c.senses.tactics_point_valid = true;
        c.senses.tactics_goal_named = true;
        c.senses.tactics_point_distance = Some(2.5);
        assert_eq!(value(0xc9e0a56c, 0, &c), Some(2.5), "the tactics point's distance");
        c.senses.tactics_at_point = true;
        assert_eq!(value(0xc9e0a56c, 0, &c), Some(0.0), "at the tactics point: 0.0");
        c.senses.tactics_at_point = false;
        c.senses.tactics_point_unset = true;
        assert_eq!(value(0xc9e0a56c, 0, &c), None, "unset tactics point: no value");
    }

    #[test]
    fn tactics_point_distance_is_zero_at_the_point_and_no_value_when_unset() {
        let mut c = Ctx::new();
        assert_eq!(value(0xa62df181, 0, &c), None, "no record: no value");
        c.senses.tactics_point_distance = Some(2.5);
        assert_eq!(value(0xa62df181, 0, &c), Some(2.5));
        c.senses.tactics_at_point = true;
        assert_eq!(value(0xa62df181, 0, &c), Some(0.0), "at the point: 0.0");
        c.senses.tactics_at_point = false;
        c.senses.tactics_point_unset = true;
        assert_eq!(value(0xa62df181, 0, &c), None, "unset: no value");
    }

    /// The 0.0 values are values, not "no value": a Compare sees them (predicate 0.0 < T and
    /// 0.0 > T). Checked for each kind that returns 0.0.
    #[test]
    fn compare_of_zero_valued_kinds_uses_the_zero_value() {
        // c9e0a56c inside the home box: 0.0.
        let mut home = Ctx::new();
        home.senses.has_home_region = true;
        home.senses.inside_home_region = true;
        assert_eq!(cmp(0xc9e0a56c, 5.0, -1.0, 0, CMP_BELOW).score(&home), 1.0, "0.0 < 5: a value");
        assert_eq!(cmp(0xc9e0a56c, 0.5, -1.0, 0, CMP_BELOW).score(&home), 1.0, "0.0 < 0.5");
        assert_eq!(cmp(0xc9e0a56c, 0.0, -1.0, 0, CMP_ABOVE).score(&home), 0.0, "0.0 > 0.0 is false");
        assert_eq!(cmp(0xc9e0a56c, -1.0, -1.0, 0, CMP_ABOVE).score(&home), 1.0, "0.0 > -1: a value");
        assert_eq!(cmp(0xc9e0a56c, 5.0, -1.0, 0, CMP_BELOW).score(&Ctx::new()), 0.0, "no value: 0");
        // a62df181 at the tactics point: 0.0.
        let mut at = Ctx::new();
        at.senses.tactics_at_point = true;
        assert_eq!(cmp(0xa62df181, 0.5, -1.0, 0, CMP_BELOW).score(&at), 1.0);
        assert_eq!(cmp(0xa62df181, 0.0, -1.0, 0, CMP_ABOVE).score(&at), 0.0);
        // 188e1abe grounded: 0.0.
        let mut ground = Ctx::new();
        ground.senses.on_ground = true;
        assert_eq!(cmp(0x188e1abe, 0.5, -1.0, 0, CMP_BELOW).score(&ground), 1.0);
        assert_eq!(cmp(0x188e1abe, 0.0, -1.0, 0, CMP_ABOVE).score(&ground), 0.0);
        // b85859da with a foe not in ambush: 0.0.
        let not_ambush = foe_ctx();
        assert_eq!(cmp(0xb85859da, 0.5, -1.0, 0, CMP_BELOW).score(&not_ambush), 1.0);
        assert_eq!(cmp(0xb85859da, 0.5, -1.0, 0, CMP_ABOVE).score(&not_ambush), 0.0, "0.0 is not above 0.5");
        assert_eq!(cmp(0xb85859da, 0.5, -1.0, 0, CMP_BELOW).score(&Ctx::new()), 0.0, "no foe: no value");
    }

    #[test]
    fn range_of_the_point_kinds_is_strict() {
        // A Range on 1d489c55 (1.0) has the same strict bounds: 1.0 is inside (0.5, 1.5).
        let mut c = foe_ctx();
        c.senses.own_line_of_sight = true;
        let r = Range {
            value_type: 0x1d489c55,
            group_id: 0,
            min: 0.5,
            max: 1.5,
            assigned_min: 0,
            assigned_max: 0,
        };
        assert_eq!(r.score(&c), 1.0);
        let r0 = Range { min: 1.0, ..r };
        assert_eq!(r0.score(&c), 0.0, "the lower bound is strict");
    }

    #[test]
    fn assigned_threshold_comes_from_the_attribute() {
        // alertDistance (0x2908a6ef) is 2000 for the scout's block; the foe at 1500 is inside it.
        let mut ctx = ctx_with_distance(1500.0);
        ctx.attr.alert_distance = 2000.0;
        let below = |assigned: u32| Compare {
            value_type: 0xda9bec13,
            group_id: 0,
            data: CompareData {
                compare_value: 1.0,
                compare_value_in_behaviour: -1.0,
                assigned,
                compare_type: CMP_BELOW,
            },
        };
        assert_eq!(below(0x2908a6ef).score(&ctx), 1.0, "1500 < alertDistance 2000");
        ctx.senses.foe_distance = Some(2500.0);
        assert_eq!(below(0x2908a6ef).score(&ctx), 0.0, "2500 is not below 2000");
        assert_eq!(assigned(0x2908a6ef, &ctx.attr), 2000.0);
        assert_eq!(assigned(0x9c096a7c, &ctx.attr), 0.0, "dodgeDistance is unset here");
        assert_eq!(assigned(0x12345678, &ctx.attr), 0.0, "an unknown hash gives 0.0 [game]");
        // An unknown assigned hash makes T 0.0: 1500 is not below 0.
        ctx.senses.foe_distance = Some(1500.0);
        assert_eq!(below(0x12345678).score(&ctx), 0.0, "T is 0.0 for an unknown hash");
    }

    #[test]
    fn group_id_is_routed_to_the_group_kinds() {
        let mut ctx = Ctx::new();
        ctx.senses.group_targets = vec![(0, 10.0, true), (5, 3.0, false)];
        let c = |group_id: u32| Compare {
            value_type: 0x843fc6db,
            group_id,
            data: CompareData {
                compare_value: 5.0,
                compare_value_in_behaviour: -1.0,
                assigned: 0,
                compare_type: CMP_BELOW,
            },
        };
        assert_eq!(c(5).score(&ctx), 1.0, "group 5 is at 3, below 5");
        assert_eq!(c(0).score(&ctx), 0.0, "group 0 is at 10, not below 5");
        assert_eq!(c(9).score(&ctx), 0.0, "no group 9: no value");
    }

    #[test]
    fn range_is_strict_at_both_ends() {
        let ctx = ctx_with_distance(5.0);
        let r = |min: f32, max: f32| Range {
            value_type: 0xda9bec13,
            group_id: 0,
            min,
            max,
            assigned_min: 0,
            assigned_max: 0,
        };
        assert_eq!(r(1.0, 6.0).score(&ctx), 1.0);
        assert_eq!(r(5.0, 6.0).score(&ctx), 0.0, "the lower bound is strict");
        assert_eq!(r(1.0, 5.0).score(&ctx), 0.0, "the upper bound is strict");
    }

    #[test]
    fn range_with_assigned_bounds_reads_the_attributes() {
        // chaseDistanceMin 25 and chaseDistanceMax 2000 (the scout's block).
        let mut ctx = ctx_with_distance(100.0);
        ctx.attr.chase_distance_min = 25.0;
        ctx.attr.chase_distance_max = 2000.0;
        let chase = Range {
            value_type: 0xda9bec13,
            group_id: 0,
            min: 0.0,
            max: 0.0,
            assigned_min: 0x439e2241,
            assigned_max: 0x7f931d18,
        };
        assert_eq!(chase.score(&ctx), 1.0, "25 < 100 < 2000");
        ctx.senses.foe_distance = Some(10.0);
        assert_eq!(chase.score(&ctx), 0.0, "10 is inside the minimum");
        ctx.senses.foe_distance = Some(2500.0);
        assert_eq!(chase.score(&ctx), 0.0, "2500 is past the maximum");
    }

    #[test]
    fn targeting_foe_checks_range_and_compare() {
        let mut ctx = ctx_with_distance(4.0);
        let data = CompareData {
            compare_value: 5.0,
            compare_value_in_behaviour: -1.0,
            assigned: 0,
            compare_type: CMP_BELOW,
        };
        let inside = Targeting {
            spec: spec(TARGET_FOE, 0.0, 10.0),
            spec2: spec(0, 0.0, 0.0),
            compare_with_second: false,
            data,
        };
        assert_eq!(inside.score(&ctx), 1.0, "foe at 4, in 0..10, and 4 < 5");

        let outside = Targeting {
            spec: spec(TARGET_FOE, 5.0, 10.0),
            ..inside
        };
        assert_eq!(outside.score(&ctx), 0.0, "foe at 4 is below the range 5..10");

        ctx.senses.foe_distance = None;
        assert_eq!(inside.score(&ctx), 0.0, "no foe");
    }

    #[test]
    fn targeting_turret_uses_the_turret_distance() {
        let mut ctx = Ctx::new();
        ctx.senses.turret_distance = Some(4.0);
        let data = CompareData {
            compare_value: 5.0,
            compare_value_in_behaviour: -1.0,
            assigned: 0,
            compare_type: CMP_BELOW,
        };
        let t = Targeting {
            spec: TargetSpec {
                target_type: TARGET_TURRET,
                group_id: 7,
                calc: 0,
                range_min: 0.0,
                range_max: 10.0,
            },
            spec2: spec(0, 0.0, 0.0),
            compare_with_second: false,
            data,
        };
        assert_eq!(t.score(&ctx), 1.0, "turret at 4");
        ctx.senses.turret_distance = None;
        assert_eq!(t.score(&ctx), 0.0, "no turret");
    }

    #[test]
    fn targeting_other_types_score_zero() {
        let ctx = ctx_with_distance(4.0);
        let data = CompareData {
            compare_value: 5.0,
            compare_value_in_behaviour: -1.0,
            assigned: 0,
            compare_type: CMP_BELOW,
        };
        let team = Targeting {
            spec: spec(0x65d870d3, 0.0, 10.0),
            spec2: spec(0, 0.0, 0.0),
            compare_with_second: false,
            data,
        };
        assert_eq!(team.score(&ctx), 0.0, "[stand-in] team type has no port");
        assert!(!target_type_bound(0x65d870d3));
        let second_unbound = Targeting {
            spec: spec(TARGET_FOE, 0.0, 10.0),
            compare_with_second: true,
            ..team
        };
        assert_eq!(second_unbound.score(&ctx), 0.0, "[stand-in] second spec type unbound: 0");
    }

    #[test]
    fn two_target_form_compares_the_second_distance_with_the_first() {
        // perceptionTurretNearerThanFoe shape: target 1 foe, target 2 turret, V = d(turret) - d(foe) < 0.
        let data = CompareData {
            compare_value: 0.0,
            compare_value_in_behaviour: -1.0,
            assigned: 0,
            compare_type: CMP_BELOW,
        };
        let t = Targeting {
            spec: spec(TARGET_FOE, 0.0, 50.0),
            spec2: spec(TARGET_TURRET, 0.0, 50.0),
            compare_with_second: true,
            data,
        };
        let mut ctx = Ctx::new();
        ctx.senses.foe_distance = Some(10.0);
        ctx.senses.turret_distance = Some(4.0);
        assert_eq!(t.score(&ctx), 1.0, "turret 4 nearer than foe 10");
        ctx.senses.turret_distance = Some(20.0);
        assert_eq!(t.score(&ctx), 0.0, "turret 20 is farther than foe 10");
        ctx.senses.turret_distance = None;
        assert_eq!(t.score(&ctx), 0.0, "no second target: 0");
        ctx.senses.turret_distance = Some(20.0);
        ctx.senses.foe_distance = None;
        assert_eq!(t.score(&ctx), 1.0, "no first target, second present: 1 at once");
        assert!(t.bound());
    }

    #[test]
    fn random_compares_its_percent_draw() {
        let mut ctx = Ctx::new();
        ctx.random = 0.3;
        assert_eq!(Random { percentage: 50.0 }.score(&ctx), 1.0, "draw 30 <= 50");
        assert_eq!(Random { percentage: 20.0 }.score(&ctx), 0.0, "draw 30 > 20");
    }

    #[test]
    fn timer_scores_its_start_flag() {
        let ctx = Ctx::new();
        assert_eq!(Timer::new(1.0, 36.0, true).score(&ctx), 1.0);
        assert_eq!(Timer::new(1.0, 36.0, false).score(&ctx), 0.0);
    }

    /// A controller store with one Message node, registered or not, and a Ctx that reads it.
    fn message_ctx(name_on: u32, name_off: u32, timer: f32, toggle: bool, toggle_on: bool, enter: bool) -> Ctx {
        let mut ctx = Ctx::new();
        let mut store = crate::ai_brain::Latches::default();
        store.add(crate::ai_brain::Latch {
            name_on,
            name_off,
            timer_init: timer,
            toggle,
            toggle_on,
            ..Default::default()
        });
        if enter {
            store.enter_all();
        }
        ctx.latches = std::sync::Arc::new(std::sync::Mutex::new(store));
        ctx
    }

    #[test]
    fn message_latch_needs_a_registered_brain() {
        let m = MessagePerception::new(0, 0x11, 0x22, true);
        let ctx = message_ctx(0x11, 0x22, 0.0, true, false, false);
        assert_eq!(m.score(&ctx), 0.0, "not registered: no node");
        let ctx = message_ctx(0x11, 0x22, 0.0, true, true, true);
        assert_eq!(m.score(&ctx), 1.0, "entered with toggleOn: the flag starts on");
    }

    #[test]
    fn delivery_is_seen_at_once_by_a_later_score() {
        let m = MessagePerception::new(0, 0x11, 0x22, true);
        let ctx = message_ctx(0x11, 0x22, 0.0, true, false, true);
        assert_eq!(m.score(&ctx), 0.0, "registered, flag off");
        ctx.emit(0xAA, crate::ai_brain::ARG_SELF);
        assert_eq!(m.score(&ctx), 0.0, "a name that is not msgNameOn");
        ctx.emit(0x11, crate::ai_brain::ARG_SELF);
        assert_eq!(m.score(&ctx), 1.0, "msgNameOn sent to self: seen by the next score");
        ctx.emit(0x22, crate::ai_brain::ARG_SELF);
        assert_eq!(m.score(&ctx), 0.0, "msgNameOff with toggle clears it");
    }

    #[test]
    fn other_args_do_not_latch_and_group_is_outboxed() {
        let m = MessagePerception::new(0, 0x11, 0x22, true);
        let mut ctx = message_ctx(0x11, 0x22, 0.0, true, false, true);
        ctx.emit(0x11, 0x1234_5678);
        assert_eq!(m.score(&ctx), 0.0, "an arg that is not SELF or GROUP does nothing");
        ctx.emit(0x11, crate::ai_brain::ARG_GROUP);
        assert_eq!(m.score(&ctx), 1.0, "GROUP delivers to self too");
        let outbox = crate::ai_brain::lock_latches(&ctx.latches).outbox.clone();
        assert_eq!(outbox, vec![0x11], "and the name waits in the outbox for the group");
        assert_eq!(ctx.drain().len(), 2, "both sends are kept for the game side");
    }

    #[test]
    fn toggle_off_word_needs_msg_toggle_and_timer_runs_per_step() {
        let m = MessagePerception::new(0, 0x11, 0x22, false);
        let ctx = message_ctx(0x11, 0x22, 0.0, false, false, true);
        ctx.emit(0x11, crate::ai_brain::ARG_SELF);
        ctx.emit(0x22, crate::ai_brain::ARG_SELF);
        assert_eq!(m.score(&ctx), 1.0, "no msgToggle: an off name does nothing");
        // Toggle 0 with msgTimer 0.5: the on lasts 0.5 s of steps, then goes off.
        let ctx = message_ctx(0x11, 0x22, 0.5, false, false, true);
        ctx.emit(0x11, crate::ai_brain::ARG_SELF);
        {
            let mut store = crate::ai_brain::lock_latches(&ctx.latches);
            store.tick(0.032);
        }
        assert_eq!(m.score(&ctx), 1.0, "0.5 - 0.032 is still on");
        for _ in 0..16 {
            crate::ai_brain::lock_latches(&ctx.latches).tick(0.032);
        }
        assert_eq!(m.score(&ctx), 0.0, "timed out after 17 steps");
    }

    #[test]
    fn average_skips_zero_links() {
        let ctx = Ctx::new();
        let a = Aggregate::new(
            CALC_AVG,
            0,
            vec![link(Graded(0.5)), link(Graded(0.0)), link(Graded(1.0))],
        );
        assert_eq!(a.score(&ctx), 0.75);
    }

    #[test]
    fn require_rejects_on_any_zero_link() {
        let ctx = Ctx::new();
        let zero = Aggregate::new(
            CALC_AVG,
            COND_REQUIRE,
            vec![link(Graded(0.5)), link(Graded(0.0))],
        );
        assert_eq!(zero.score(&ctx), 0.0);
        let all = Aggregate::new(
            CALC_AVG,
            COND_REQUIRE,
            vec![link(Graded(0.5)), link(Graded(1.0))],
        );
        assert_eq!(all.score(&ctx), 0.75);
    }

    #[test]
    fn max_and_min_pick_the_extreme_link() {
        let ctx = Ctx::new();
        let links = || vec![link(Graded(0.2)), link(Graded(0.9)), link(Graded(0.5))];
        assert_eq!(Aggregate::new(CALC_MAX, 0, links()).score(&ctx), 0.9);
        assert_eq!(Aggregate::new(CALC_MIN, 0, links()).score(&ctx), 0.2);
    }

    #[test]
    fn no_links_is_one_and_no_passing_link_is_zero() {
        let ctx = Ctx::new();
        assert_eq!(Aggregate::new(CALC_AVG, 0, vec![]).score(&ctx), 1.0);
        assert_eq!(Aggregate::new(CALC_AVG, 0, vec![link(Graded(0.0))]).score(&ctx), 0.0);
        assert_eq!(
            Aggregate::new(0x12345678, 0, vec![link(Graded(0.5))]).score(&ctx),
            0.0,
            "an unknown calc leaves best at 0"
        );
    }

    #[test]
    fn a_scoring_link_sends_its_message() {
        let mut ctx = Ctx::new();
        let mut on = link(Graded(1.0));
        on.message = 42;
        on.arg = 7;
        let mut off = link(Graded(0.0));
        off.message = 43;
        off.arg = 8;
        let a = Aggregate::new(CALC_AVG, 0, vec![on, off]);
        assert_eq!(a.score(&ctx), 1.0);
        assert_eq!(ctx.drain(), vec![(42, 7)], "a zero score sends nothing");
    }

    #[test]
    fn into_scorer_runs_the_aggregate() {
        let s = Aggregate::new(CALC_AVG, 0, vec![link(Graded(0.4))]).into_scorer();
        assert_eq!(s(&Ctx::new()), 0.4);
    }
}
