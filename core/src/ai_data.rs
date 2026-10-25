//! The game's AI data: the brains, goals, perceptions and behaviours that `bnxglobal.str` data
//! chunk 16 holds, and the attribute blocks that name a brain (`brainList`).
//!
//! Data only. Everything is read in place from the user's install at runtime;
//! nothing from the game is bundled and no behaviour is run here. [data] Field names are the
//! data's own names, in snake_case. A hash is the CRC-32 of a data name, kept as a `u32` (the
//! data's `#xxxxxxxx`); the doc comment gives the data name it stands for where known. [data]
//! Hashes the survey could not name (the unpublished notes section 10) stay numbers.
//!
//! Sources: the unpublished notes, `work\survey\ai\schema.tsv` (every object field), and
//! `work\survey\ai\objects.json` (the list entries). [data] The list entries carry wrapper
//! fields (`invertPriority`, `msgTrigger`, `msgType`, `pPerception`, `pGoal`, `pBehavior`,
//! `priority`, `rampUpTime`, ...) that `schema.tsv` does not list; they are read here.
//!
//! Anything the data holds that these structs do not expect (an unknown type, a field that
//! is not listed, a missing field, a name not in the table) goes into a `missing` list with
//! where it was found. Nothing is dropped quietly.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use crate::formats::hash::crc32;
use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};

/// The data chunk of `bnxglobal.str` that holds the AI objects and the attribute blocks. [data]
pub const AI_CHUNK: usize = 16;

/// The type name of a brain root. [data] The roots carry no type name of their own in most
/// cases; the survey reads them as `AIBrainNormal` by their use (the unpublished notes section 1).
pub const BRAIN_TYPE: &str = "AIBrainNormal";
/// The type name of a goal. [data]
pub const GOAL_TYPE: &str = "AIGoal";

// The field names each object and list entry holds, as the data writes them. [data] schema.tsv
// for the objects; objects.json for the wrapper entries of the lists.
const BRAIN_FIELDS: &[&str] = &["perceptionList", "priorityCalculation", "priorityCondition", "goalList", "persistentBehaviorList"];
const GOAL_FIELDS: &[&str] = &["perceptionList", "priorityCalculation", "priorityCondition", "behaviorList"];
const BRAIN_LIST_FIELDS: &[&str] = &["priority", "rampUpTime", "rampDownTime", "pBrain"];
const GOAL_REF_FIELDS: &[&str] = &["priority", "rampUpTime", "rampDownTime", "pGoal", "msgEnter", "msgExit", "msgType"];
const BEHAVIOUR_REF_FIELDS: &[&str] = &[
    "priority", "rampUpTime", "rampDownTime", "pBehavior", "oneShot", "finishAction", "exitAction", "msgEnter", "msgExit", "msgFinish", "msgType",
];
const PERCEPTION_REF_FIELDS: &[&str] = &["invertPriority", "msgTrigger", "msgType", "pPerception"];
const BEHAVIOUR_COMMON: &[&str] = &["perceptionList", "priorityCalculation", "priorityCondition"];
const TARGET_FIELDS: &[&str] = &["targetType", "targetGroupID", "targetCalc", "targetCalcRangeMin", "targetCalcRangeMax"];
const COMPARE_DATA_FIELDS: &[&str] = &["compareValue", "compareValueInBehavior", "assignedPerception", "compareType"];

const COMPARE_FIELDS: &[&str] = &["fuzzy", "minPriority", "maxPriority", "valueType", "groupID", "compareData"];
const MESSAGE_FIELDS: &[&str] = &["fuzzy", "minPriority", "maxPriority", "msgNameOn", "msgNameOff", "msgTimer", "msgToggle", "toggleOn"];
const RANDOM_FIELDS: &[&str] = &["fuzzy", "minPriority", "maxPriority", "percentage"];
const RANGE_FIELDS: &[&str] = &[
    "fuzzy", "minPriority", "maxPriority", "valueType", "groupID", "compareValueMin", "compareValueMax", "assignedPerceptionMin", "assignedPerceptionMax",
];
const TARGETING_FIELDS: &[&str] = &["fuzzy", "minPriority", "maxPriority", "targetData1", "targetData2", "compareData", "compareWithSecondTarget"];
const TIMER_FIELDS: &[&str] = &["fuzzy", "minPriority", "maxPriority", "timeOn", "timeOff", "startOn"];

const GO_TO_TARGET_FIELDS: &[&str] = &["movementType", "useAutoMovement", "attack", "weaponIndex", "targetData"];
const ATTACK_RANGED_FIELDS: &[&str] = &["ignoreAttackDelay", "ignoreKungFuFighting", "weaponIndex", "targetData"];
const ATTACK_MELEE_FIELDS: &[&str] = &["ignoreAttackDelay", "ignoreKungFuFighting", "weaponIndex", "attackSpecial"];
const ATTACK_TRANSFORM_FIELDS: &[&str] = &["ignoreAttackDelay", "ignoreKungFuFighting", "weaponIndex", "attackType", "targetData"];
const IDLE_FIELDS: &[&str] = &["trackTarget", "vehicleMode", "weaponMode"];
const WEAPON_SWITCH_FIELDS: &[&str] = &["weaponIndex", "disableWeapons"];
const MOVE_ON_WAY_POINTS_FIELDS: &[&str] = &["movementType", "useAutoMovement", "attack", "weaponIndex"];
const MOVE_ON_SPLINE_FIELDS: &[&str] = &["movementType", "useAutoMovement", "attack", "weaponIndex", "splineMovementType", "patrol"];
const TACTICS_FIELDS: &[&str] = &["tacticsType"];
const DODGE_FIELDS: &[&str] = &["dodgeLeft", "dodgeRight", "dodgeForward", "dodgeBackward"];
const TELEPORT_FIELDS: &[&str] = &["targetData"];
const INTERACT_FIELDS: &[&str] = &[];

// ---------------------------------------------------------------------------------------------
// The structs. Each mirrors one object of the data; a list entry mirrors its wrapper.
// ---------------------------------------------------------------------------------------------

/// A brain's entry in a character's `brainList`: `{priority, rampUpTime, rampDownTime, pBrain}`. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct BrainRef {
    /// `priority`: 0 to 1 (the scout's own brain is 0.5, `brainList` index 1). [data]
    /// Which entry runs is not in the data: the highest active one is [assumed] (the unpublished notes section 2).
    pub priority: f32,
    /// `rampUpTime` (hash, `#63df2be6` in most entries). [data]
    pub ramp_up_time: u32,
    /// `rampDownTime` (hash, `#63df2be6` in most entries). [data]
    pub ramp_down_time: u32,
    /// `pBrain`: the CRC-32 of the brain's name, which is its root's name in chunk 16. [data]
    pub brain: u32,
}

/// An `AI_characterAttributes<X>` block: the AI variant of a character's attributes. [data]
/// Only the `brainList` is mirrored here; the block's other 133 keys are read by the character
/// loader, not by this module (the unpublished notes section 8).
#[derive(Clone, Debug, PartialEq)]
pub struct AiCharacterAttributes {
    /// The block's data name, `AI_characterAttributes<X>`.
    pub name: String,
    /// CRC-32 of `name`.
    pub hash: u32,
    pub brains: Vec<BrainRef>,
}

/// A brain root (`brain*` in chunk 16): its perceptions, its goals and its persistent behaviours. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct BrainRoot {
    /// The root's data name, e.g. `brainDComScout`.
    pub name: String,
    /// CRC-32 of `name`.
    pub hash: u32,
    /// `perceptionList`: perceptions of the brain itself (empty in most brains). [data]
    pub perceptions: Vec<PerceptionRef>,
    /// `priorityCalculation`: `#d1cad841` in every brain. [data] Meaning unnamed.
    pub priority_calculation: u32,
    /// `priorityCondition`: `#90d88c38` in most, `#cf330a51` in 2. [data] Meaning unnamed.
    pub priority_condition: u32,
    /// `goalList`: the goals in the data's order (not sorted by priority in the data). [data]
    pub goals: Vec<GoalRef>,
    /// `persistentBehaviorList`: behaviours checked on every update, outside any goal. [data]
    pub persistent: Vec<BehaviourRef>,
}

/// A goal's entry in a brain's `goalList`: `{priority, rampUpTime, rampDownTime, pGoal, msgEnter, msgExit, msgType}`. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct GoalRef {
    pub priority: f32,
    pub ramp_up_time: u32,
    pub ramp_down_time: u32,
    /// `msgEnter`, `msgExit`: 0 or a message name's hash. [data]
    pub msg_enter: u32,
    pub msg_exit: u32,
    /// `msgType`: `#cc0d1c75` in most, `#2a1bd058` in a few. [data] Meaning unnamed.
    pub msg_type: u32,
    pub goal: Goal,
}

/// A goal (`AIGoal`): perceptions that gate it, and the behaviours it runs. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Goal {
    /// The object's data name, e.g. `goalScoutP`.
    pub name: String,
    /// CRC-32 of `name`.
    pub hash: u32,
    /// `perceptionList`. [data]
    pub perceptions: Vec<PerceptionRef>,
    /// `priorityCalculation`, `priorityCondition`: as on a brain. [data]
    pub priority_calculation: u32,
    pub priority_condition: u32,
    /// `behaviorList`: the behaviours of the goal, in the data's order. [data]
    pub behaviours: Vec<BehaviourRef>,
}

/// A behaviour's entry in a goal's `behaviorList` or a brain's `persistentBehaviorList`. [data]
/// `{priority, rampUpTime, rampDownTime, pBehavior, oneShot, finishAction, exitAction, msgEnter, msgExit, msgFinish, msgType}`.
#[derive(Clone, Debug, PartialEq)]
pub struct BehaviourRef {
    pub priority: f32,
    pub ramp_up_time: u32,
    pub ramp_down_time: u32,
    /// `oneShot`: true in 13 of 280 entries. [data]
    pub one_shot: bool,
    /// `finishAction`, `exitAction`: `#b962ede1` in every entry. [data] Meaning unnamed.
    pub finish_action: u32,
    pub exit_action: u32,
    /// `msgEnter`, `msgExit`, `msgFinish`: 0 or a message name's hash. [data]
    pub msg_enter: u32,
    pub msg_exit: u32,
    pub msg_finish: u32,
    /// `msgType`: `#cc0d1c75` in most, `#2a1bd058` in 6. [data]
    pub msg_type: u32,
    pub behaviour: Behaviour,
}

/// A perception's entry in a `perceptionList`: `{invertPriority, msgTrigger, msgType, pPerception}`. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct PerceptionRef {
    /// `invertPriority`: true in 76 of 355 entries. [data] What inversion does is not in the data.
    pub invert_priority: bool,
    /// `msgTrigger`: 0 or a message name's hash. [data]
    pub msg_trigger: u32,
    /// `msgType`: `#cc0d1c75` in most, `#2a1bd058` in 2. [data]
    pub msg_type: u32,
    pub perception: Perception,
}

/// The fields every perception object has. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct PerceptionBase {
    pub name: String,
    pub hash: u32,
    /// `fuzzy`: false in every object. [data]
    pub fuzzy: bool,
    /// `minPriority` (0) and `maxPriority` (1) in every object. [data] Their weight is not in the data.
    pub min_priority: f32,
    pub max_priority: f32,
}

/// The six perception types. [data] (`AIPerceptionState` is not in the data; the unpublished notes section 1.)
#[derive(Clone, Debug, PartialEq)]
pub enum Perception {
    Compare(Compare),
    Message(Message),
    Random(Random),
    Range(Range),
    Targeting(Targeting),
    Timer(Timer),
}

/// `compareData` of a compare: a value to test against, and the kind of test. [data]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompareData {
    /// `compareValue`: mostly 0 (35 objects), then 15, 20, 35, 55. Its unit is not in the data.
    pub compare_value: f32,
    /// `compareValueInBehavior`: -1 in 57 entries, 25 in one. [data] Meaning unnamed.
    pub compare_value_in_behavior: f32,
    /// `assignedPerception`: the perception it hands its result to; `None` for `false`. [data]
    pub assigned_perception: Option<u32>,
    /// `compareType`: `#2156da7b` (40 entries, all distance-type) or `#d89693aa` (18, boolean-type).
    /// [assumed] "less than" and "equal" (the unpublished notes section 4); not named in the data.
    pub compare_type: u32,
}

/// A `targetData` block: which target a behaviour or perception looks at. [data]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Target {
    /// `targetType`: a `TARGET_` enum value's name hash (`TARGET_FOE`, `TARGET_HOME_POINT`, ... or unnamed). [data]
    pub target_type: u32,
    /// `targetGroupID`: 0, or the hash of a group name (`AttackObj`, `Escort`). [data]
    pub target_group_id: u32,
    /// `targetCalc`: `#d812eb39` in every target that has one. [data] Meaning unnamed.
    pub target_calc: u32,
    /// `targetCalcRangeMin`, `targetCalcRangeMax`: 0 and 5000 in every target. [data]
    pub target_calc_range_min: f32,
    pub target_calc_range_max: f32,
}

/// `AIPerceptionCompare`: compares a value with `compareData.compareValue`. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Compare {
    pub base: PerceptionBase,
    /// `valueType`: a hash of what is compared; 25 distinct values, unnamed except as the perception names suggest. [data]
    pub value_type: u32,
    /// `groupID`: 0, or the hash of `AttackObj` or `Escort`. [data]
    pub group_id: u32,
    pub compare: CompareData,
}

/// `AIPerceptionMessage`: on while a named message is received, off on another. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub base: PerceptionBase,
    /// `msgNameOn`, `msgNameOff`: message name hashes. [data] The names resolve through `names.tsv`.
    pub msg_name_on: u32,
    pub msg_name_off: u32,
    /// `msgTimer`: 0 in every object. [data]
    pub msg_timer: f32,
    /// `msgToggle`: true in every object. [data]
    pub msg_toggle: bool,
    /// `toggleOn`: true in 2 of 36. [data]
    pub toggle_on: bool,
}

/// `AIPerceptionRandom`: true with a chance `percentage`. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Random {
    pub base: PerceptionBase,
    /// `percentage`: 0.5 (3), 1 (1), 3.3 (1). [data] The unit (0 to 1 or a weight) is not in the data.
    pub percentage: f32,
}

/// `AIPerceptionRange`: compares a value against a min and a max. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Range {
    pub base: PerceptionBase,
    pub value_type: u32,
    pub group_id: u32,
    pub compare_value_min: f32,
    pub compare_value_max: f32,
    /// `assignedPerceptionMin`, `assignedPerceptionMax`: `None` for `false`. [data]
    pub assigned_perception_min: Option<u32>,
    pub assigned_perception_max: Option<u32>,
}

/// `AIPerceptionTargeting`: compares two targets, optionally with `compareData`. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Targeting {
    pub base: PerceptionBase,
    pub target_data1: Target,
    pub target_data2: Target,
    pub compare: CompareData,
    /// `compareWithSecondTarget`: true in both objects. [data]
    pub compare_with_second_target: bool,
}

/// `AIPerceptionTimer`: on after `timeOn` seconds, off after `timeOff` (the one object: 1 and 36). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Timer {
    pub base: PerceptionBase,
    pub time_on: f32,
    pub time_off: f32,
    /// `startOn`: true in the one object. [data]
    pub start_on: bool,
}

/// The fields every behaviour object has, besides its own parameters. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct BehaviourBase {
    pub name: String,
    pub hash: u32,
    /// `perceptionList` of the behaviour: perceptions that gate it. [data]
    pub perceptions: Vec<PerceptionRef>,
    pub priority_calculation: u32,
    pub priority_condition: u32,
}

/// A `weaponIndex` that is either a flag (`true` or `false`) or a hash (`#ffffffff` is what the
/// data holds when it is unset, 1 WeaponSwitch and every GoToTarget and AttackRanged). [data]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WeaponIndex {
    Flag(bool),
    Hash(u32),
}

/// The twelve behaviour types. [data] (`AIBehaviorMessage` is not in the data.)
#[derive(Clone, Debug, PartialEq)]
pub enum Behaviour {
    GoToTarget(GoToTarget),
    AttackRanged(AttackRanged),
    AttackMelee(AttackMelee),
    AttackTransform(AttackTransform),
    Idle(Idle),
    WeaponSwitch(WeaponSwitch),
    MoveOnWayPoints(MoveOnWayPoints),
    MoveOnSpline(MoveOnSpline),
    Tactics(Tactics),
    Dodge(Dodge),
    Teleport(Teleport),
    Interact(Interact),
}

/// `AIBehaviorGoToTarget` (36 objects). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct GoToTarget {
    pub base: BehaviourBase,
    /// `movementType`: `MOVEMENT_NORMAL` (17), `MOVEMENT_STRAFE` (15), `MOVEMENT_VEHICLE` (4); names are CRC matches. [data guess]
    pub movement_type: u32,
    pub use_auto_movement: bool,
    pub attack: bool,
    /// `weaponIndex`: `#ffffffff` in all 36. [data]
    pub weapon_index: u32,
    pub target: Target,
}

/// `AIBehaviorAttackRanged` (24 objects). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct AttackRanged {
    pub base: BehaviourBase,
    /// `ignoreAttackDelay`: true in all 24. [data]
    pub ignore_attack_delay: bool,
    pub ignore_kung_fu_fighting: bool,
    pub weapon_index: u32,
    pub target: Target,
}

/// `AIBehaviorAttackMelee` (5 objects). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct AttackMelee {
    pub base: BehaviourBase,
    pub ignore_attack_delay: bool,
    pub ignore_kung_fu_fighting: bool,
    pub weapon_index: u32,
    /// `attackSpecial`: true in 2 of 5. [data]
    pub attack_special: bool,
}

/// `AIBehaviorAttackTransform` (9 objects). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct AttackTransform {
    pub base: BehaviourBase,
    pub ignore_attack_delay: bool,
    pub ignore_kung_fu_fighting: bool,
    pub weapon_index: u32,
    /// `attackType`: `#00000021` (7), `#00000022` (1), `#00000023` (1). [data] An integer enum; the names are not in the data.
    pub attack_type: u32,
    pub target: Target,
}

/// `AIBehaviorIdle` (17 objects). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Idle {
    pub base: BehaviourBase,
    pub track_target: bool,
    pub vehicle_mode: bool,
    pub weapon_mode: bool,
}

/// `AIBehaviorWeaponSwitch` (16 objects). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponSwitch {
    pub base: BehaviourBase,
    /// `weaponIndex`: `true` (7), `false` (8), or the unset hash (1). [data] What `true` and `false` select is [assumed] weapon 1 and 2.
    pub weapon_index: WeaponIndex,
    /// `disableWeapons`: true in 1 of 16. [data]
    pub disable_weapons: bool,
}

/// `AIBehaviorMoveOnWayPoints` (8 objects). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct MoveOnWayPoints {
    pub base: BehaviourBase,
    pub movement_type: u32,
    pub use_auto_movement: bool,
    pub attack: bool,
    pub weapon_index: u32,
}

/// `AIBehaviorMoveOnSpline` (3 objects). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct MoveOnSpline {
    pub base: BehaviourBase,
    pub movement_type: u32,
    pub use_auto_movement: bool,
    pub attack: bool,
    pub weapon_index: u32,
    /// `splineMovementType`: `#3b4b6b8e` in all 3. [data] Meaning unnamed.
    pub spline_movement_type: u32,
    /// `patrol`: false in all 3. [data]
    pub patrol: bool,
}

/// `AIBehaviorTactics` (5 objects). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Tactics {
    pub base: BehaviourBase,
    /// `tacticsType`: `#2b1ca766` (3), `#f579762c` (1), `#596a409b` (1). [data] Unnamed.
    pub tactics_type: u32,
}

/// `AIBehaviorDodge` (2 objects). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Dodge {
    pub base: BehaviourBase,
    /// `dodgeLeft`, `dodgeRight` (0 to 25), `dodgeForward` (0 to 100), `dodgeBackward` (0). [data] Units not in the data.
    pub dodge_left: f32,
    pub dodge_right: f32,
    pub dodge_forward: f32,
    pub dodge_backward: f32,
}

/// `AIBehaviorTeleport` (1 object). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Teleport {
    pub base: BehaviourBase,
    pub target: Target,
}

/// `AIBehaviorInteract` (1 object): no fields beyond the common three. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Interact {
    pub base: BehaviourBase,
}

impl Behaviour {
    /// The behaviour's own fields and its common ones.
    pub fn base(&self) -> &BehaviourBase {
        match self {
            Behaviour::GoToTarget(b) => &b.base,
            Behaviour::AttackRanged(b) => &b.base,
            Behaviour::AttackMelee(b) => &b.base,
            Behaviour::AttackTransform(b) => &b.base,
            Behaviour::Idle(b) => &b.base,
            Behaviour::WeaponSwitch(b) => &b.base,
            Behaviour::MoveOnWayPoints(b) => &b.base,
            Behaviour::MoveOnSpline(b) => &b.base,
            Behaviour::Tactics(b) => &b.base,
            Behaviour::Dodge(b) => &b.base,
            Behaviour::Teleport(b) => &b.base,
            Behaviour::Interact(b) => &b.base,
        }
    }

    /// The data's type name of this behaviour (`AIBehaviorGoToTarget`, ...).
    pub fn type_name(&self) -> &'static str {
        match self {
            Behaviour::GoToTarget(_) => "AIBehaviorGoToTarget",
            Behaviour::AttackRanged(_) => "AIBehaviorAttackRanged",
            Behaviour::AttackMelee(_) => "AIBehaviorAttackMelee",
            Behaviour::AttackTransform(_) => "AIBehaviorAttackTransform",
            Behaviour::Idle(_) => "AIBehaviorIdle",
            Behaviour::WeaponSwitch(_) => "AIBehaviorWeaponSwitch",
            Behaviour::MoveOnWayPoints(_) => "AIBehaviorMoveOnWayPoints",
            Behaviour::MoveOnSpline(_) => "AIBehaviorMoveOnSpline",
            Behaviour::Tactics(_) => "AIBehaviorTactics",
            Behaviour::Dodge(_) => "AIBehaviorDodge",
            Behaviour::Teleport(_) => "AIBehaviorTeleport",
            Behaviour::Interact(_) => "AIBehaviorInteract",
        }
    }
}

impl Perception {
    /// The fields every perception has.
    pub fn base(&self) -> &PerceptionBase {
        match self {
            Perception::Compare(p) => &p.base,
            Perception::Message(p) => &p.base,
            Perception::Random(p) => &p.base,
            Perception::Range(p) => &p.base,
            Perception::Targeting(p) => &p.base,
            Perception::Timer(p) => &p.base,
        }
    }

    /// The data's type name of this perception (`AIPerceptionCompare`, ...).
    pub fn type_name(&self) -> &'static str {
        match self {
            Perception::Compare(_) => "AIPerceptionCompare",
            Perception::Message(_) => "AIPerceptionMessage",
            Perception::Random(_) => "AIPerceptionRandom",
            Perception::Range(_) => "AIPerceptionRange",
            Perception::Targeting(_) => "AIPerceptionTargeting",
            Perception::Timer(_) => "AIPerceptionTimer",
        }
    }
}

impl Goal {
    /// Its perceptions, and those of its behaviours, counted where they are used.
    fn perception_count(&self) -> usize {
        self.perceptions.len() + self.behaviours.iter().map(|b| b.behaviour.base().perceptions.len()).sum::<usize>()
    }
}

impl BrainRoot {
    /// How many goals, perceptions and behaviours the brain uses, each counted at every place it
    /// is written (a goal shared by two brains counts twice). Perceptions are those of the brain,
    /// its goals and their behaviours, and its persistent behaviours.
    pub fn totals(&self) -> BrainTotals {
        let goals = self.goals.len();
        let perceptions = self.perceptions.len()
            + self.goals.iter().map(|g| g.goal.perception_count()).sum::<usize>()
            + self.persistent.iter().map(|b| b.behaviour.base().perceptions.len()).sum::<usize>();
        let behaviours = self.goals.iter().map(|g| g.goal.behaviours.len()).sum::<usize>() + self.persistent.len();
        BrainTotals { goals, perceptions, behaviours }
    }
}

/// The counts [`BrainRoot::totals`] gives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BrainTotals {
    pub goals: usize,
    pub perceptions: usize,
    pub behaviours: usize,
}

/// Everything read from chunk 16's brains.
#[derive(Clone, Debug, Default)]
pub struct AiData {
    /// Every brain root whose name starts with `brain`, in the file's order. [data] 59 in the install (survey).
    pub brains: Vec<BrainRoot>,
    /// Unique objects read, counted by type name (an object reached by two paths counts once).
    /// An object with no type name is counted as `AIBrainNormal` when it is a brain and as
    /// `untyped` otherwise. [data] The survey: 364 objects in 20 types.
    pub objects: BTreeMap<String, usize>,
    /// Roots that are not brains (the attribute blocks and anything else in chunk 16).
    pub other_roots: usize,
    /// Everything that did not read as the schema says, with where it was found.
    pub missing: Vec<String>,
}

impl AiData {
    /// The brain whose name hashes to `hash` (the CRC-32 of its name).
    pub fn brain_by_hash(&self, hash: u32) -> Option<&BrainRoot> {
        self.brains.iter().find(|b| b.hash == hash)
    }

    /// The brain with this data name.
    pub fn brain(&self, name: &str) -> Option<&BrainRoot> {
        self.brain_by_hash(crc32(name.as_bytes()))
    }
}

// ---------------------------------------------------------------------------------------------
// The reader. Field access goes through `lxb::Node`; every failure is recorded, not dropped.
// ---------------------------------------------------------------------------------------------

/// A name hash read as the data writes it: an enum value (its name's hash) or an integer.
fn hash_of(node: Node) -> Option<u32> {
    node.enum_hash().or_else(|| node.int().map(|v| v as u32))
}

fn name_text(file: &DataFile, hash: u32) -> String {
    file.name(hash).map_or_else(|| format!("#{hash:08x}"), str::to_owned)
}

#[derive(Default)]
struct Reader {
    missing: Vec<String>,
    /// Objects already counted, by type name and data position.
    seen: HashSet<(String, usize)>,
    counts: BTreeMap<String, usize>,
}

impl Reader {
    fn report(&mut self, owner: &str, what: impl AsRef<str>) {
        self.missing.push(format!("{owner}: {}", what.as_ref()));
    }

    /// Counts an object once, under its type name.
    fn count(&mut self, kind: &str, node: Node) {
        if let Some(at) = node.offset()
            && self.seen.insert((kind.to_owned(), at)) {
            *self.counts.entry(kind.to_owned()).or_default() += 1;
        }
    }

    /// The named field, or a report that it is absent.
    fn field<'a>(&mut self, node: Node<'a>, owner: &str, name: &str) -> Option<Node<'a>> {
        let found = node.get(name);
        if found.is_none() {
            self.report(owner, format!("no field {name}"));
        }
        found
    }

    /// Reports every field of the object that the schema does not list.
    fn expect_fields(&mut self, node: Node, owner: &str, expected: &[&str]) {
        let file = node.file();
        for (hash, _) in node.fields() {
            if !expected.iter().any(|name| crc32(name.as_bytes()) == hash) {
                let text = name_text(file, hash);
                self.report(owner, format!("unexpected field {text}"));
            }
        }
    }

    /// Reports a type name the schema does not allow. An untyped object has the name `""`.
    fn check_type(&mut self, node: Node, owner: &str, allowed: &[&str]) -> bool {
        let found = node.type_name().unwrap_or("");
        if allowed.contains(&found) {
            return true;
        }
        self.report(owner, format!("type {found:?}, expected one of {allowed:?}"));
        false
    }

    /// The object's data name: its header's label. Reports a name the table does not spell out.
    fn name_of(&mut self, node: Node, owner: &str) -> (u32, String) {
        let Some(hash) = node.label() else {
            self.report(owner, "object has no name");
            return (0, String::new());
        };
        let file = node.file();
        if file.name(hash).is_none() {
            self.report(owner, format!("name #{hash:08x} is not in the table"));
        }
        (hash, name_text(file, hash))
    }

    fn float(&mut self, node: Node, owner: &str, name: &str) -> f32 {
        let Some(field) = self.field(node, owner, name) else { return 0.0 };
        field.float().unwrap_or_else(|| {
            self.report(owner, format!("{name} is not a number"));
            0.0
        })
    }

    fn flag(&mut self, node: Node, owner: &str, name: &str) -> bool {
        let Some(field) = self.field(node, owner, name) else { return false };
        match field.enum_name() {
            Some("true") => true,
            Some("false") => false,
            _ => {
                self.report(owner, format!("{name} is not true or false"));
                false
            }
        }
    }

    fn hash(&mut self, node: Node, owner: &str, name: &str) -> u32 {
        let Some(field) = self.field(node, owner, name) else { return 0 };
        hash_of(field).unwrap_or_else(|| {
            self.report(owner, format!("{name} is not a name hash"));
            0
        })
    }

    /// A hash field that is `false` when there is none.
    fn optional_hash(&mut self, node: Node, owner: &str, name: &str) -> Option<u32> {
        let field = self.field(node, owner, name)?;
        match field.enum_name() {
            Some("false") => None,
            Some("true") => {
                self.report(owner, format!("{name} is true where a hash or false was expected"));
                None
            }
            _ => hash_of(field).or_else(|| {
                self.report(owner, format!("{name} is not a name hash"));
                None
            }),
        }
    }

    /// The items of a list field. An absent field reads as an empty list, and is reported.
    fn list<'a>(&mut self, node: Node<'a>, owner: &str, name: &str) -> Vec<Node<'a>> {
        match self.field(node, owner, name) {
            Some(list) => list.items().collect(),
            None => Vec::new(),
        }
    }

    /// The object a pointer field names. Reports a null pointer, or one the table cannot follow.
    fn pointer<'a>(&mut self, node: Node<'a>, owner: &str, name: &str) -> Option<Node<'a>> {
        let target = self.field(node, owner, name)?;
        if target.offset().is_none() {
            self.report(owner, format!("{name} is null or does not name an object"));
            return None;
        }
        Some(target)
    }

    /// The perceptions named in a `perceptionList`, in order.
    fn perception_list(&mut self, node: Node, owner: &str) -> Vec<PerceptionRef> {
        let mut out = Vec::new();
        for (i, entry) in self.list(node, owner, "perceptionList").into_iter().enumerate() {
            let owner = format!("{owner}.perceptionList[{i}]");
            out.extend(self.perception_ref(entry, &owner));
        }
        out
    }

    fn perception_ref(&mut self, entry: Node, owner: &str) -> Option<PerceptionRef> {
        self.expect_fields(entry, owner, PERCEPTION_REF_FIELDS);
        let invert_priority = self.flag(entry, owner, "invertPriority");
        let msg_trigger = self.hash(entry, owner, "msgTrigger");
        let msg_type = self.hash(entry, owner, "msgType");
        let target = self.pointer(entry, owner, "pPerception")?;
        let perception = self.perception(target, owner)?;
        Some(PerceptionRef { invert_priority, msg_trigger, msg_type, perception })
    }

    fn perception_base(&mut self, node: Node, owner: &str) -> PerceptionBase {
        let (hash, name) = self.name_of(node, owner);
        PerceptionBase {
            name,
            hash,
            fuzzy: self.flag(node, owner, "fuzzy"),
            min_priority: self.float(node, owner, "minPriority"),
            max_priority: self.float(node, owner, "maxPriority"),
        }
    }

    /// The perception object itself, by its type name.
    fn perception(&mut self, node: Node, owner: &str) -> Option<Perception> {
        let kind = node.type_name().unwrap_or("untyped");
        self.count(kind, node);
        let (_, name) = self.name_of(node, owner);
        let owner = format!("{owner} perception {name}");
        let owner = owner.as_str();
        match kind {
            "AIPerceptionCompare" => {
                self.expect_fields(node, owner, COMPARE_FIELDS);
                let base = self.perception_base(node, owner);
                let value_type = self.hash(node, owner, "valueType");
                let group_id = self.hash(node, owner, "groupID");
                let compare = self.compare_data(node, owner);
                Some(Perception::Compare(Compare { base, value_type, group_id, compare }))
            }
            "AIPerceptionMessage" => {
                self.expect_fields(node, owner, MESSAGE_FIELDS);
                let base = self.perception_base(node, owner);
                Some(Perception::Message(Message {
                    base,
                    msg_name_on: self.hash(node, owner, "msgNameOn"),
                    msg_name_off: self.hash(node, owner, "msgNameOff"),
                    msg_timer: self.float(node, owner, "msgTimer"),
                    msg_toggle: self.flag(node, owner, "msgToggle"),
                    toggle_on: self.flag(node, owner, "toggleOn"),
                }))
            }
            "AIPerceptionRandom" => {
                self.expect_fields(node, owner, RANDOM_FIELDS);
                let base = self.perception_base(node, owner);
                Some(Perception::Random(Random { base, percentage: self.float(node, owner, "percentage") }))
            }
            "AIPerceptionRange" => {
                self.expect_fields(node, owner, RANGE_FIELDS);
                let base = self.perception_base(node, owner);
                Some(Perception::Range(Range {
                    base,
                    value_type: self.hash(node, owner, "valueType"),
                    group_id: self.hash(node, owner, "groupID"),
                    compare_value_min: self.float(node, owner, "compareValueMin"),
                    compare_value_max: self.float(node, owner, "compareValueMax"),
                    assigned_perception_min: self.optional_hash(node, owner, "assignedPerceptionMin"),
                    assigned_perception_max: self.optional_hash(node, owner, "assignedPerceptionMax"),
                }))
            }
            "AIPerceptionTargeting" => {
                self.expect_fields(node, owner, TARGETING_FIELDS);
                let base = self.perception_base(node, owner);
                let target_data1 = self.target(node, owner, "targetData1");
                let target_data2 = self.target(node, owner, "targetData2");
                let compare = self.compare_data(node, owner);
                Some(Perception::Targeting(Targeting {
                    base,
                    target_data1,
                    target_data2,
                    compare,
                    compare_with_second_target: self.flag(node, owner, "compareWithSecondTarget"),
                }))
            }
            "AIPerceptionTimer" => {
                self.expect_fields(node, owner, TIMER_FIELDS);
                let base = self.perception_base(node, owner);
                Some(Perception::Timer(Timer {
                    base,
                    time_on: self.float(node, owner, "timeOn"),
                    time_off: self.float(node, owner, "timeOff"),
                    start_on: self.flag(node, owner, "startOn"),
                }))
            }
            other => {
                self.report(owner, format!("unknown perception type {other:?}"));
                None
            }
        }
    }

    /// The `compareData` struct of a compare or a targeting perception.
    fn compare_data(&mut self, node: Node, owner: &str) -> CompareData {
        let Some(data) = self.field(node, owner, "compareData") else { return CompareData::default() };
        let owner = format!("{owner}.compareData");
        let owner = owner.as_str();
        self.expect_fields(data, owner, COMPARE_DATA_FIELDS);
        CompareData {
            compare_value: self.float(data, owner, "compareValue"),
            compare_value_in_behavior: self.float(data, owner, "compareValueInBehavior"),
            assigned_perception: self.optional_hash(data, owner, "assignedPerception"),
            compare_type: self.hash(data, owner, "compareType"),
        }
    }

    /// A `targetData` struct, read under the field `name`.
    fn target(&mut self, node: Node, owner: &str, name: &str) -> Target {
        let Some(data) = self.field(node, owner, name) else { return Target::default() };
        let owner = format!("{owner}.{name}");
        let owner = owner.as_str();
        self.expect_fields(data, owner, TARGET_FIELDS);
        Target {
            target_type: self.hash(data, owner, "targetType"),
            target_group_id: self.hash(data, owner, "targetGroupID"),
            target_calc: self.hash(data, owner, "targetCalc"),
            target_calc_range_min: self.float(data, owner, "targetCalcRangeMin"),
            target_calc_range_max: self.float(data, owner, "targetCalcRangeMax"),
        }
    }

    fn behaviour_base(&mut self, node: Node, owner: &str) -> BehaviourBase {
        let (hash, name) = self.name_of(node, owner);
        BehaviourBase {
            name,
            hash,
            perceptions: self.perception_list(node, owner),
            priority_calculation: self.hash(node, owner, "priorityCalculation"),
            priority_condition: self.hash(node, owner, "priorityCondition"),
        }
    }

    /// The behaviour object itself, by its type name.
    fn behaviour(&mut self, node: Node, owner: &str) -> Option<Behaviour> {
        let kind = node.type_name().unwrap_or("untyped");
        self.count(kind, node);
        let (_, name) = self.name_of(node, owner);
        let owner = format!("{owner} behaviour {name}");
        let owner = owner.as_str();
        let own: &[&str] = match kind {
            "AIBehaviorGoToTarget" => GO_TO_TARGET_FIELDS,
            "AIBehaviorAttackRanged" => ATTACK_RANGED_FIELDS,
            "AIBehaviorAttackMelee" => ATTACK_MELEE_FIELDS,
            "AIBehaviorAttackTransform" => ATTACK_TRANSFORM_FIELDS,
            "AIBehaviorIdle" => IDLE_FIELDS,
            "AIBehaviorWeaponSwitch" => WEAPON_SWITCH_FIELDS,
            "AIBehaviorMoveOnWayPoints" => MOVE_ON_WAY_POINTS_FIELDS,
            "AIBehaviorMoveOnSpline" => MOVE_ON_SPLINE_FIELDS,
            "AIBehaviorTactics" => TACTICS_FIELDS,
            "AIBehaviorDodge" => DODGE_FIELDS,
            "AIBehaviorTeleport" => TELEPORT_FIELDS,
            "AIBehaviorInteract" => INTERACT_FIELDS,
            other => {
                self.report(owner, format!("unknown behaviour type {other:?}"));
                return None;
            }
        };
        let mut all: Vec<&str> = BEHAVIOUR_COMMON.to_vec();
        all.extend_from_slice(own);
        self.expect_fields(node, owner, &all);
        let base = self.behaviour_base(node, owner);
        Some(match kind {
            "AIBehaviorGoToTarget" => Behaviour::GoToTarget(GoToTarget {
                movement_type: self.hash(node, owner, "movementType"),
                use_auto_movement: self.flag(node, owner, "useAutoMovement"),
                attack: self.flag(node, owner, "attack"),
                weapon_index: self.hash(node, owner, "weaponIndex"),
                target: self.target(node, owner, "targetData"),
                base,
            }),
            "AIBehaviorAttackRanged" => Behaviour::AttackRanged(AttackRanged {
                ignore_attack_delay: self.flag(node, owner, "ignoreAttackDelay"),
                ignore_kung_fu_fighting: self.flag(node, owner, "ignoreKungFuFighting"),
                weapon_index: self.hash(node, owner, "weaponIndex"),
                target: self.target(node, owner, "targetData"),
                base,
            }),
            "AIBehaviorAttackMelee" => Behaviour::AttackMelee(AttackMelee {
                ignore_attack_delay: self.flag(node, owner, "ignoreAttackDelay"),
                ignore_kung_fu_fighting: self.flag(node, owner, "ignoreKungFuFighting"),
                weapon_index: self.hash(node, owner, "weaponIndex"),
                attack_special: self.flag(node, owner, "attackSpecial"),
                base,
            }),
            "AIBehaviorAttackTransform" => Behaviour::AttackTransform(AttackTransform {
                ignore_attack_delay: self.flag(node, owner, "ignoreAttackDelay"),
                ignore_kung_fu_fighting: self.flag(node, owner, "ignoreKungFuFighting"),
                weapon_index: self.hash(node, owner, "weaponIndex"),
                attack_type: self.hash(node, owner, "attackType"),
                target: self.target(node, owner, "targetData"),
                base,
            }),
            "AIBehaviorIdle" => Behaviour::Idle(Idle {
                track_target: self.flag(node, owner, "trackTarget"),
                vehicle_mode: self.flag(node, owner, "vehicleMode"),
                weapon_mode: self.flag(node, owner, "weaponMode"),
                base,
            }),
            "AIBehaviorWeaponSwitch" => Behaviour::WeaponSwitch(WeaponSwitch {
                weapon_index: self.weapon_index(node, owner),
                disable_weapons: self.flag(node, owner, "disableWeapons"),
                base,
            }),
            "AIBehaviorMoveOnWayPoints" => Behaviour::MoveOnWayPoints(MoveOnWayPoints {
                movement_type: self.hash(node, owner, "movementType"),
                use_auto_movement: self.flag(node, owner, "useAutoMovement"),
                attack: self.flag(node, owner, "attack"),
                weapon_index: self.hash(node, owner, "weaponIndex"),
                base,
            }),
            "AIBehaviorMoveOnSpline" => Behaviour::MoveOnSpline(MoveOnSpline {
                movement_type: self.hash(node, owner, "movementType"),
                use_auto_movement: self.flag(node, owner, "useAutoMovement"),
                attack: self.flag(node, owner, "attack"),
                weapon_index: self.hash(node, owner, "weaponIndex"),
                spline_movement_type: self.hash(node, owner, "splineMovementType"),
                patrol: self.flag(node, owner, "patrol"),
                base,
            }),
            "AIBehaviorTactics" => Behaviour::Tactics(Tactics { tactics_type: self.hash(node, owner, "tacticsType"), base }),
            "AIBehaviorDodge" => Behaviour::Dodge(Dodge {
                dodge_left: self.float(node, owner, "dodgeLeft"),
                dodge_right: self.float(node, owner, "dodgeRight"),
                dodge_forward: self.float(node, owner, "dodgeForward"),
                dodge_backward: self.float(node, owner, "dodgeBackward"),
                base,
            }),
            "AIBehaviorTeleport" => Behaviour::Teleport(Teleport { target: self.target(node, owner, "targetData"), base }),
            _ => Behaviour::Interact(Interact { base }),
        })
    }

    /// `weaponIndex` of a WeaponSwitch: a flag, or a hash when unset.
    fn weapon_index(&mut self, node: Node, owner: &str) -> WeaponIndex {
        let Some(field) = self.field(node, owner, "weaponIndex") else { return WeaponIndex::Hash(0) };
        match field.enum_name() {
            Some("true") => WeaponIndex::Flag(true),
            Some("false") => WeaponIndex::Flag(false),
            _ => WeaponIndex::Hash(self.hash(node, owner, "weaponIndex")),
        }
    }

    /// A `behaviorList` or `persistentBehaviorList` entry: the wrapper and its behaviour.
    fn behaviour_ref(&mut self, entry: Node, owner: &str) -> Option<BehaviourRef> {
        self.expect_fields(entry, owner, BEHAVIOUR_REF_FIELDS);
        let priority = self.float(entry, owner, "priority");
        let ramp_up_time = self.hash(entry, owner, "rampUpTime");
        let ramp_down_time = self.hash(entry, owner, "rampDownTime");
        let one_shot = self.flag(entry, owner, "oneShot");
        let finish_action = self.hash(entry, owner, "finishAction");
        let exit_action = self.hash(entry, owner, "exitAction");
        let msg_enter = self.hash(entry, owner, "msgEnter");
        let msg_exit = self.hash(entry, owner, "msgExit");
        let msg_finish = self.hash(entry, owner, "msgFinish");
        let msg_type = self.hash(entry, owner, "msgType");
        let target = self.pointer(entry, owner, "pBehavior")?;
        let behaviour = self.behaviour(target, owner)?;
        Some(BehaviourRef {
            priority,
            ramp_up_time,
            ramp_down_time,
            one_shot,
            finish_action,
            exit_action,
            msg_enter,
            msg_exit,
            msg_finish,
            msg_type,
            behaviour,
        })
    }

    /// A goal object (`AIGoal`), with its perceptions and behaviours.
    fn goal(&mut self, node: Node, owner: &str) -> Goal {
        self.count(GOAL_TYPE, node);
        let (hash, name) = self.name_of(node, owner);
        let owner = format!("{owner} goal {name}");
        let owner = owner.as_str();
        self.expect_fields(node, owner, GOAL_FIELDS);
        let perceptions = self.perception_list(node, owner);
        let priority_calculation = self.hash(node, owner, "priorityCalculation");
        let priority_condition = self.hash(node, owner, "priorityCondition");
        let mut behaviours = Vec::new();
        for (i, entry) in self.list(node, owner, "behaviorList").into_iter().enumerate() {
            let owner = format!("{owner}.behaviorList[{i}]");
            behaviours.extend(self.behaviour_ref(entry, &owner));
        }
        Goal { name, hash, perceptions, priority_calculation, priority_condition, behaviours }
    }

    /// A `goalList` entry: the wrapper and its goal.
    fn goal_ref(&mut self, entry: Node, owner: &str) -> Option<GoalRef> {
        self.expect_fields(entry, owner, GOAL_REF_FIELDS);
        let priority = self.float(entry, owner, "priority");
        let ramp_up_time = self.hash(entry, owner, "rampUpTime");
        let ramp_down_time = self.hash(entry, owner, "rampDownTime");
        let msg_enter = self.hash(entry, owner, "msgEnter");
        let msg_exit = self.hash(entry, owner, "msgExit");
        let msg_type = self.hash(entry, owner, "msgType");
        let target = self.pointer(entry, owner, "pGoal")?;
        if !self.check_type(target, owner, &[GOAL_TYPE]) {
            return None;
        }
        let goal = self.goal(target, owner);
        Some(GoalRef { priority, ramp_up_time, ramp_down_time, msg_enter, msg_exit, msg_type, goal })
    }

    /// A brain root: its perceptions, goals and persistent behaviours.
    fn brain(&mut self, node: Node, name: String, hash: u32) -> BrainRoot {
        let owner = name.as_str();
        // A root holds its fields directly; most carry no type name, the rest are AIBrainNormal.
        self.count(node.type_name().unwrap_or(BRAIN_TYPE), node);
        self.check_type(node, owner, &["", BRAIN_TYPE]);
        self.expect_fields(node, owner, BRAIN_FIELDS);
        let perceptions = self.perception_list(node, owner);
        let priority_calculation = self.hash(node, owner, "priorityCalculation");
        let priority_condition = self.hash(node, owner, "priorityCondition");
        let mut goals = Vec::new();
        for (i, entry) in self.list(node, owner, "goalList").into_iter().enumerate() {
            let owner = format!("{owner}.goalList[{i}]");
            goals.extend(self.goal_ref(entry, &owner));
        }
        let mut persistent = Vec::new();
        for (i, entry) in self.list(node, owner, "persistentBehaviorList").into_iter().enumerate() {
            let owner = format!("{owner}.persistentBehaviorList[{i}]");
            persistent.extend(self.behaviour_ref(entry, &owner));
        }
        BrainRoot { name: name.clone(), hash, perceptions, priority_calculation, priority_condition, goals, persistent }
    }
}

/// Reads every brain of a chunk-16 data file. Every problem goes into `missing`.
pub fn read_brains(file: &DataFile) -> AiData {
    let mut r = Reader::default();
    let mut brains = Vec::new();
    let mut other_roots = 0;
    for (hash, node) in file.root().fields() {
        match file.name(hash) {
            Some(name) if name.starts_with("brain") => {
                if crc32(name.as_bytes()) != hash {
                    r.report(name, "root name does not hash to its key");
                }
                brains.push(r.brain(node, name.to_owned(), hash));
            }
            Some(_) => other_roots += 1,
            None => r.report("root", format!("#{hash:08x} has no name in the table")),
        }
    }
    AiData { brains, objects: r.counts, other_roots, missing: r.missing }
}

/// Reads the AI attribute blocks (`AI_characterAttributes<X>` roots) of a chunk-16 data file.
pub fn read_attributes(file: &DataFile, missing: &mut Vec<String>) -> Vec<AiCharacterAttributes> {
    let mut out = Vec::new();
    for (hash, node) in file.root().fields() {
        let Some(name) = file.name(hash) else { continue };
        if !name.starts_with("AI_characterAttributes") {
            continue;
        }
        let brains = brains_of(node, missing);
        out.push(AiCharacterAttributes { name: name.to_owned(), hash, brains });
    }
    out
}

/// The brains a character attribute block lists in its `brainList`, in the data's order.
/// Works on a `characterAttributes<X>` or an `AI_characterAttributes<X>` object. Problems go into `missing`.
pub fn brains_of(attributes: Node, missing: &mut Vec<String>) -> Vec<BrainRef> {
    let mut r = Reader::default();
    let mut out = Vec::new();
    for (i, entry) in r.list(attributes, "brainList", "brainList").into_iter().enumerate() {
        let owner = format!("brainList[{i}]");
        let owner = owner.as_str();
        r.expect_fields(entry, owner, BRAIN_LIST_FIELDS);
        let priority = r.float(entry, owner, "priority");
        let ramp_up_time = r.hash(entry, owner, "rampUpTime");
        let ramp_down_time = r.hash(entry, owner, "rampDownTime");
        let Some(target) = r.pointer(entry, owner, "pBrain") else { continue };
        let Some(brain) = target.label() else {
            r.report(owner, "pBrain has no name");
            continue;
        };
        out.push(BrainRef { priority, ramp_up_time, ramp_down_time, brain });
    }
    missing.append(&mut r.missing);
    out
}

/// Opens `bnxglobal.str` in the install and reads its AI data chunk ([`AI_CHUNK`]).
pub fn load(game_dir: &Path) -> Result<AiData, String> {
    let pack = Pack::open(&game_dir.join("bnxglobal.str"))?;
    let chunk = pack
        .chunks
        .iter()
        .find(|c| c.index == AI_CHUNK && c.tag == pack::DATA)
        .ok_or("bnxglobal.str has no AI data chunk 16")?;
    let file = DataFile::parse(pack.data(chunk).to_vec())?;
    Ok(read_brains(&file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::hash::crc32;
    use std::path::PathBuf;

    fn install() -> Option<PathBuf> {
        let dir = PathBuf::from(crate::install::dir_string());
        dir.join("bnxglobal.str").is_file().then_some(dir)
    }

    fn base(name: &str) -> PerceptionBase {
        PerceptionBase { name: name.into(), hash: crc32(name.as_bytes()), fuzzy: false, min_priority: 0.0, max_priority: 1.0 }
    }

    // A small hand-built brain (no game file): one goal with two perceptions, and two
    // behaviours of two different types. The brain has no perceptions and no persistent entries.
    fn perception_ref(perception: Perception) -> PerceptionRef {
        PerceptionRef { invert_priority: false, msg_trigger: 0, msg_type: crc32(b"msgtype"), perception }
    }

    fn behaviour_base(name: &str, perceptions: Vec<PerceptionRef>) -> BehaviourBase {
        BehaviourBase {
            name: name.into(),
            hash: crc32(name.as_bytes()),
            perceptions,
            priority_calculation: 1,
            priority_condition: 2,
        }
    }

    fn behaviour_ref(behaviour: Behaviour) -> BehaviourRef {
        BehaviourRef {
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

    fn fixture_brain() -> BrainRoot {
        let compare = Perception::Compare(Compare {
            base: base("perceptionCompare"),
            value_type: 1,
            group_id: 0,
            compare: CompareData { compare_value: 5.0, compare_value_in_behavior: -1.0, assigned_perception: None, compare_type: 2 },
        });
        let timer = Perception::Timer(Timer { base: base("perceptionTimer"), time_on: 1.0, time_off: 36.0, start_on: true });
        let goal_perceptions = vec![perception_ref(compare), perception_ref(timer)];
        let message = perception_ref(Perception::Message(Message {
            base: base("perceptionMessage"),
            msg_name_on: 7,
            msg_name_off: 8,
            msg_timer: 0.0,
            msg_toggle: true,
            toggle_on: false,
        }));
        let go_to = Behaviour::GoToTarget(GoToTarget {
            base: behaviour_base("behaviorGoTo", vec![]),
            movement_type: 3,
            use_auto_movement: true,
            attack: false,
            weapon_index: u32::MAX,
            target: Target { target_type: 4, target_group_id: 0, target_calc: 5, target_calc_range_min: 0.0, target_calc_range_max: 5000.0 },
        });
        let idle = Behaviour::Idle(Idle {
            base: behaviour_base("behaviorIdle", vec![message]),
            track_target: true,
            vehicle_mode: false,
            weapon_mode: true,
        });
        let goal = Goal {
            name: "goalFixture".into(),
            hash: crc32(b"goalFixture"),
            perceptions: goal_perceptions,
            priority_calculation: 1,
            priority_condition: 2,
            behaviours: vec![behaviour_ref(go_to), behaviour_ref(idle)],
        };
        BrainRoot {
            name: "brainFixture".into(),
            hash: crc32(b"brainFixture"),
            perceptions: vec![],
            priority_calculation: 1,
            priority_condition: 2,
            goals: vec![GoalRef {
                priority: 1.0,
                ramp_up_time: 0,
                ramp_down_time: 0,
                msg_enter: 0,
                msg_exit: 0,
                msg_type: 0,
                goal,
            }],
            persistent: vec![],
        }
    }

    #[test]
    fn one_goal_with_two_perceptions_and_two_behaviour_types_counts_as_written() {
        let brain = fixture_brain();
        assert_eq!(brain.totals(), BrainTotals { goals: 1, perceptions: 3, behaviours: 2 });
        let goal = &brain.goals[0].goal;
        assert_eq!(goal.perceptions.len(), 2);
        assert_eq!(goal.perceptions[0].perception.type_name(), "AIPerceptionCompare");
        assert_eq!(goal.perceptions[1].perception.type_name(), "AIPerceptionTimer");
        let types: Vec<&str> = goal.behaviours.iter().map(|b| b.behaviour.type_name()).collect();
        assert_eq!(types, ["AIBehaviorGoToTarget", "AIBehaviorIdle"]);
    }

    #[test]
    fn a_brain_list_of_two_keeps_its_order_and_its_priorities() {
        let list = vec![
            BrainRef { priority: 1.0, ramp_up_time: 1, ramp_down_time: 1, brain: crc32(b"brainCommon") },
            BrainRef { priority: 0.5, ramp_up_time: 1, ramp_down_time: 1, brain: crc32(b"brainDComScout") },
        ];
        let attributes = AiCharacterAttributes { name: "AI_characterAttributesFixture".into(), hash: 0, brains: list.clone() };
        assert_eq!(attributes.brains, list);
        assert_eq!(attributes.brains[1].priority, 0.5);
        let data = AiData {
            brains: vec![BrainRoot { name: "brainCommon".into(), hash: crc32(b"brainCommon"), ..fixture_brain() }, fixture_brain()],
            ..AiData::default()
        };
        assert!(data.brain("brainCommon").is_some());
        assert!(data.brain_by_hash(attributes.brains[1].brain).is_none(), "brainDComScout is not in this fixture");
        assert_eq!(data.brain("brainFixture").map(|b| b.totals().goals), Some(1));
    }

    #[test]
    fn the_type_names_match_the_data() {
        let b = Behaviour::Interact(Interact { base: behaviour_base("behaviorInteract", vec![]) });
        assert_eq!(b.type_name(), "AIBehaviorInteract");
        let p = Perception::Random(Random { base: base("perceptionRandom"), percentage: 0.5 });
        assert_eq!(p.type_name(), "AIPerceptionRandom");
    }

    /// The install's brains against the survey: 59 roots, the scout's brain as `brains_full.txt` has it.
    #[test]
    fn the_install_brains_match_the_survey() {
        let Some(dir) = install() else { return };
        let data = load(&dir).expect("bnxglobal.str chunk 16");
        assert_eq!(data.brains.len(), 59, "brain roots");
        let scout = data.brain("brainDComScout").expect("brainDComScout");
        assert_eq!(scout.goals.len(), 5);
        assert_eq!(scout.persistent.len(), 7);
        assert!(data.missing.is_empty(), "missing: {:?}", &data.missing[..data.missing.len().min(8)]);
    }
}
