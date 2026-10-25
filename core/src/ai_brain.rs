//! The enemy AI brain spine (phase 3): the brain tick, think, select, the goal and behaviour
//! switches and the persistent behaviour pass, as engine-free Rust. Read from the game's code;
//! where the notes disagree, the unpublished notes wins: "The tick, settled" and "Manager rulings:
//! goals, behaviour entries, invert, one-shot" (R1 to R3) win over the earlier sections. Not wired
//! into sim.rs or the game yet. The concrete perceptions and behaviours are written by other
//! tickets; this file holds only the traits they implement, the data they fill, and the logic.
//!
//!   [game addr]  read in the game's code at that address
//!   [data]       a value from the game's data
//!   [assumed]    our reading; logged in the unpublished notes under Phase 3
//!   [stand-in]   a placeholder, to be replaced
//!   [unread]     a call the game makes that is not read yet: a tagged no-op here
//!   [unknown]    not found
//!
//! Two levels (R1). A brain holds its goals (the data's goalList), sorted by static priority. A
//! goal holds its behaviour entries (the data's behaviorList), sorted the same way. The brain walks
//! its goals, and inside a goal the same walk runs over its behaviour entries. The controller's
//! current is (goal index, behaviour entry index). Mapping to the game's names: `Root` is the brain
//! root (`root+0x10` is the thinking brain, `root+0x20` the persistent list). `Controller` is the
//! character's AI (`controller+8` is the current brain, `+0xc` the goal, `+0x10` the behaviour
//! entry, `+0xc0` the clock).

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::ai_move::{TARGET_FOE, TARGET_FRIEND, TARGET_TURRET};

/// TARGET_GROUP, the target type of a group lookup (the targetGroupID names the group) [data: the
/// hash #fdae89ce, the unpublished notes; the type's name is [assumed] from the hash alone].
/// Its distance is `Senses::group_target_distance`.
pub const TARGET_GROUP: u32 = 0xfdae_89ce;

/// Channels a persistent behaviour can claim: the game's local table is 13 bytes [game 00820e10].
pub const CHANNELS: usize = 13;

/// The one-shot behaviour entries that have finished, as (goal index, behaviour entry index) of
/// the current brain [assumed] (R3). Cleared when the brain changes and on start.
pub type Done = BTreeSet<(usize, usize)>;

/// A message to the character: (message hash, argument). [assumed] the argument is one 32-bit
/// word, as the character message handler 007236c0 takes it; the game's handler is not read.
pub type Message = (u32, u32);

/// The message argument that means "deliver to this character's own controller" (handler 007236c0,
/// ARG 0xcc0d1c75) [game].
pub const ARG_SELF: u32 = 0xcc0d_1c75;

/// The message argument of a GROUP broadcast (handler 007236c0, ARG 0x2a1bd058): the name goes to
/// every character with the sender's group, the sender included [game, ai-messages.md section 7].
pub const ARG_GROUP: u32 = 0x2a1b_d058;

/// One latch node of a Message perception (P+0x2c list, node+0x10 flag, node+0x14 timer) [game
/// 0082f200, 0082f350, 0082f3c0]. The port keys a node by the perception's index in `Latches::nodes`
/// (the controller is the only key the port needs: one store per controller).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Latch {
    /// P+0x1c msgNameOn.
    pub name_on: u32,
    /// P+0x20 msgNameOff.
    pub name_off: u32,
    /// P+0x24 msgTimer, the time an ON lasts when the timer runs [game 0082f350].
    pub timer_init: f32,
    /// P+0x28 msgToggle: an OFF name clears the latch.
    pub toggle: bool,
    /// P+0x29 toggleOn: the flag a node starts with on entry [game 0082f200].
    pub toggle_on: bool,
    /// The node exists: the brain that holds the perception is running [game 00822430, 00822500].
    pub registered: bool,
    /// node+0x10: the latch [game].
    pub flag: bool,
    /// node+0x14: the time left on an ON [game 0082f3c0].
    pub timer: f32,
}

/// The latch store of one controller, and the group outbox of its step.
#[derive(Clone, Debug, Default)]
pub struct Latches {
    /// Indexed by the Message perception's id.
    pub nodes: Vec<Latch>,
    /// Names this controller broadcast to its group (ARG_GROUP) since the outbox was last taken.
    /// The integrator delivers them to the other members (`combat::route_group_messages`).
    pub outbox: Vec<u32>,
}

/// A shared handle to a controller's latch store. Shared by the controller and the `Ctx` of each
/// step, so a score (which takes `&Ctx`) reads the latches a send changed earlier in the same walk.
pub type LatchStore = Arc<Mutex<Latches>>;

/// Locks a latch store. A poisoned lock still gives the data (the store has no invariant to break).
pub fn lock_latches(store: &LatchStore) -> MutexGuard<'_, Latches> {
    store.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Latches {
    /// Adds the node of one Message perception and returns its id. Starts unregistered.
    pub fn add(&mut self, latch: Latch) -> usize {
        self.nodes.push(latch);
        self.nodes.len() - 1
    }

    /// Enter (00822430 enters every brain of the list; each Message perception of the brain enters
    /// with 0082f200): each node is created with its flag = toggleOn [game 0082f200]. [assumed] the
    /// timer starts at 0: the game's enter does not write node+0x14 [unknown].
    pub fn enter_all(&mut self) {
        for n in &mut self.nodes {
            n.registered = true;
            n.flag = n.toggle_on;
            n.timer = 0.0;
        }
    }

    /// Leave (00822500): the nodes are removed, so every latch is gone [game 0082f2e0].
    pub fn leave_all(&mut self) {
        for n in &mut self.nodes {
            n.registered = false;
            n.flag = false;
            n.timer = 0.0;
        }
    }

    /// The event walk of one name (0082f350, per registered Message perception) [game]: msgNameOn
    /// sets the flag and the timer; with msgToggle, msgNameOff clears both. Any other name does
    /// nothing. The name is not compared with the perception's own name.
    pub fn deliver(&mut self, name: u32) {
        for n in self.nodes.iter_mut().filter(|n| n.registered) {
            if name == n.name_on {
                n.flag = true;
                n.timer = n.timer_init;
            } else if n.toggle && name == n.name_off {
                n.flag = false;
                n.timer = 0.0;
            }
        }
    }

    /// The timer tick, once per engine step [game 0082f3c0]: a node that is on and has a timer
    /// (toggle 0, or msgTimer above 0) counts down; at 0 or less it goes off. [assumed] the tick runs
    /// once per controller step, at the start of `Controller::tick`; the game's order is [unknown].
    pub fn tick(&mut self, dt: f32) {
        for n in self.nodes.iter_mut().filter(|n| n.registered && n.flag) {
            if !n.toggle || n.timer_init > 0.0 {
                n.timer -= dt;
                if n.timer <= 0.0 {
                    n.flag = false;
                    n.timer = 0.0;
                }
            }
        }
    }
}

/// A button or weapon request a behaviour makes for the character. The caller reads them after
/// the step. The button ids are the state-machine enum [game] (docs/state-machine.md line 35;
/// the unpublished notes section 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Press a button (`ai_button_request` 00822360): the button id.
    Button(u32),
    /// Weapon mode request (`ai_weapon_mode_request` 0082ac80) with a weapon index, -1 for none.
    /// [unknown] which button it presses, 0x15 or 0x18 (ai-behaviours-attack.md section 7, item 1).
    WeaponMode(i32),
    /// A switch to a weapon index. [unknown] the button id the game presses (section 7, item 2).
    WeaponSwitch(i32),
    /// The dodge direction sent to the character (`00829df0`, unread): 0 left, 1 right, 2
    /// forward, 3 backward [assumed] (ai-behaviours-attack.md section 6 and A5).
    Dodge(u8),
    /// Release a button id: the bit is not pressed this step [game 0082ab50 clears bit 0x18 of the
    /// device bitmap, ai-targeting-idle.md B.2]. The caller clears it after the other requests.
    Release(u32),
}

/// What the character is doing, as the perceptions and behaviours read it. Filled by the caller.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SelfState {
    /// Character position, metres [game].
    pub pos: [f32; 3],
    /// The camera target point (`0071a2e0`), the origin of the aim cone test [game]. How the game
    /// computes it is [unread]: the caller supplies it.
    pub aim_origin: [f32; 3],
    /// Heading in radians (`char+0x104 +0x25c`) [game]; the facing is (-sin, cos) of it.
    pub facing: f32,
    /// Health [unknown] units. No ported perception reads it yet.
    pub health: f32,
    /// Form value (`char+0x31c`; 2 is checked by WeaponSwitch) [game].
    pub form: i32,
    /// Current state hash (state machine `+0x18`); 0 means no current state [game].
    pub state: u32,
    /// In a vehicle (`0070ab90`) [game].
    pub in_vehicle: bool,
    /// The weapon index in hand (weapon manager `+0x74`) [game].
    pub weapon_index: i32,
    /// Team id (`character+0x50`) [assumed].
    pub team: u32,
    /// `char+0x234 +0x84`, the special object's float: above 0 blocks the melee special [game].
    pub special_block: f32,
}

/// The foe the controller holds (`ctrl+0x68`), as the perceptions and behaviours see it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Target {
    /// Target position, metres [game].
    pub pos: [f32; 3],
    /// Distance from the character, metres [assumed] (the scalar of 00725400, ai-perceptions 5).
    pub distance: f32,
    /// Team id (`character+0x50`) [assumed].
    pub team: u32,
}

/// What the character senses this step, read by the perceptions' values (the value getter
/// 00830360, ai_perceive::value). The integrator fills it before each step; the perceptions only
/// read it. Default: nothing sensed (no foe, no group target, no home point, no team object, not
/// in vehicle motion) and full health.
#[derive(Debug, Clone, PartialEq)]
pub struct Senses {
    /// Distance from the character to the nearest foe, metres [assumed] (the FOE search 00823220,
    /// range 0..5000; ai-perceptions "Manager rulings"). None: no foe. Values da9bec13.
    pub foe_distance: Option<f32>,
    /// Line of sight to that foe [game 00827ce0 test, assumed]. Values e056d012.
    pub foe_visible: bool,
    /// Group targets as (groupID, distance metres, line of sight), one entry per group the
    /// integrator found. Values 843fc6db and 0cf5b934 look up their group here. [assumed] the
    /// entry is the nearest live member of the group (the port's `combat::Enemy::step` feeds it from
    /// `group_input`; which member the game picks when several are alive is unread).
    pub group_targets: Vec<(u32, f32, bool)>,
    /// The place of each group target, as (groupID, world point), the same entries as `group_targets`.
    /// The destination of a group-targeted movement walks here (ai_move `destination`), not to the
    /// enemy's own place [game: a movement's point is its target's point, ai-behaviours-move section 10,
    /// assumed for the group type]. A group with no entry has no point; the movement then stays at its own place.
    pub group_target_points: Vec<(u32, [f32; 3])>,
    /// Distance to the home point, metres (`ctrl+0x38..0x40`); None when there is no home point
    /// [game 00830d22]. Values fc1c5893.
    pub home_distance: Option<f32>,
    /// Health as a percent of maximum, 0..100 [game 00831664, third read B]. Values 27bb0c2e.
    /// [assumed] the default 100 (a fresh character) when nothing is set.
    pub health_percent: f32,
    /// The character is in vehicle motion (`0070ab90`) [game]. Values 621332f0.
    pub in_vehicle_motion: bool,
    /// Distance to the nearest team object (the team type 0x01e2fc40 search), metres [assumed]
    /// (ai-perceptions third read, row 192301ae). None: none found. Values 192301ae.
    pub team_object_distance: Option<f32>,
    /// The character is on the ground: the grounded byte of the movement step (`STEP+0x26c`)
    /// [game 00831780]. Values 5d06c0ee and 188e1abe. Fed by the caller's ground flag.
    pub on_ground: bool,
    /// The character is in the fall form (`0081e820`) [game 0081e820]. [assumed] the port's
    /// `StateFall` class stands for it (the game reads the form object's vtable). Values 5d06c0ee.
    pub falling_form: bool,
    /// Bit 5 of `character+0x14c`, the hit flag [game 00830c6c; the name "hit" is [assumed]].
    /// Values fbd14a89 and 5ada9b7c.
    pub was_hit: bool,
    /// The hit reaction clock test of 5ada9b7c: `state+0xc - char+0x1f0 <= 0` [game 00831799]. True
    /// means the test holds, which is INSIDE the hit window (the name says "over" and is kept from the
    /// first port): with the hit flag, 5ada9b7c gives 0.001 and an inverted DamagedRecently fails.
    /// The state's clock and `char+0x1f0` are [unknown] names; `+0x1f0` read as the window's end is
    /// [assumed]. Fed by the enemy's stun window (combat.rs brain_input). Values 5ada9b7c.
    pub hit_reaction_over: bool,
    /// The controller has a path record (`controller+0x1ac` non-zero) [game 008307e3]. [assumed]
    /// the name "has a path". Values 8fd8eb5d.
    pub has_path: bool,
    /// The nearest foe is inside the sight cone of the character [game 00831929, 00831c00,
    /// 00723c30]. Values 6b3394c7.
    pub foe_in_sight_cone: bool,
    /// The foe's attack type is the flight slam (`0x9ebf35c5`) [game 00831a5c; the name "fast" is
    /// [assumed] and does not match the data]. Values 7b35ef2b.
    pub foe_attack_fast: bool,
    /// The foe's weapon attack test (the player branch of 218a1421) [game 008314dd; the branch
    /// is UNVERIFIED]. Values 218a1421.
    pub foe_attack_weapon: bool,
    /// The foe's hit reaction float at `+0x54` is above 0 [game 008312ac; the field name is
    /// [unknown]]. Values 1529732e.
    pub foe_stunned: bool,
    /// The foe is in ambush mode (`00824430`) [game 00830a76; the name is [assumed]]. Values
    /// b85859da. With a foe present the value is 0.0 when this is false.
    pub foe_ambush: bool,
    /// The character is in a flying form (`00736c10`) [game 0083172a; the name "hovering" is
    /// [assumed]]. Values 2fe0758f.
    pub flying_form: bool,
    /// The ground height test of 188e1abe passes [game 00831306; the height `00715990` is unread].
    /// Values 188e1abe.
    pub near_ground: bool,
    /// The line of sight of 1d489c55: the nav corridor test `00827ce0` with mask 0x18 [game
    /// 008313d2]. It is NOT the plain sight test. Values 1d489c55.
    pub own_line_of_sight: bool,
    /// Weapon heat as a percent, 100 times `00828250(char)` with slot 0 [game 008315b7; the
    /// fraction's meaning is [assumed]]. None: no weapon. Values 26e128c3.
    pub weapon_overheat_percent: Option<f32>,
    /// A home region exists (the `00824650` gate: a home-region member with node byte +0x98 set)
    /// [game 0083097c]. False: c9e0a56c has no value. Values c9e0a56c.
    pub has_home_region: bool,
    /// The camera target point is inside the home box (`00824700`; its box `00824450` is unread)
    /// [game 0083097c]. Values c9e0a56c.
    pub inside_home_region: bool,
    /// Distance from the home point to the character, metres [game 00824610, 00725400]. None: no
    /// home point. Values c9e0a56c.
    pub home_point_distance: Option<f32>,
    /// The tactics record R is present and its `+4` byte is non-zero (`controller+0x690`; the
    /// record's name is [assumed]) [game 0083097c]. Values c9e0a56c.
    pub tactics_point_valid: bool,
    /// The tactics record's goal type is the one the game names `0xa8e780b1` (name [unknown])
    /// [game 0083097c]. Values c9e0a56c.
    pub tactics_goal_named: bool,
    /// The character is at the tactics point (`00833cb0`, which gives 0.0) [game 008308b6].
    /// Values a62df181 and c9e0a56c.
    pub tactics_at_point: bool,
    /// The tactics point is unset (`00833d30`: `+8`, `+0xc`, `+0x10` all FLT_MAX) or the record is
    /// absent [game 008308b6]. Values a62df181 and c9e0a56c. Default false; unset is no value.
    pub tactics_point_unset: bool,
    /// Distance from the tactics point to the character, metres [game 00830950, 3D]. Values
    /// a62df181 and c9e0a56c.
    pub tactics_point_distance: Option<f32>,
    /// Group members as (groupID, distance metres, line of sight), one per group the integrator
    /// found within 5000 m of the group's point [game 0083193f, 007e9c60]. Values 712ecdc7 (the
    /// distance) and 0acb9809 (the sight). [assumed] the point and the sight test are unread; the
    /// port takes the group point as the enemy's own position and feeds the nearest live member
    /// of each group (`combat::Enemy::step`, from `group_input`) [assumed].
    pub group_members: Vec<(u32, f32, bool)>,
    /// Distance to the nearest friend, metres [game 008315d5; the friend definition is [unknown]].
    /// Values 22d75963. [assumed] the branch of 00822c10 is NOT VERIFIED.
    pub friend_distance: Option<f32>,
    /// Distance to the nearest turret, metres, for the TARGET_TURRET type of a Targeting [stand-in]:
    /// no turret search is fed to the port yet, so the integrator leaves it None.
    pub turret_distance: Option<f32>,
    /// `character+0x26c` holds a handle: something grabbable is registered on the character
    /// [game 0081e750, 0070c900 read it]. [stand-in] its writer is NOT FOUND (ai-targeting-idle.md
    /// C.4), so the integrator sets it false and the Interact score is 0.
    pub interact_handle: bool,
}

impl Default for Senses {
    fn default() -> Self {
        Self {
            foe_distance: None,
            foe_visible: false,
            group_targets: Vec::new(),
            group_target_points: Vec::new(),
            home_distance: None,
            health_percent: 100.0,
            in_vehicle_motion: false,
            team_object_distance: None,
            on_ground: false,
            falling_form: false,
            was_hit: false,
            hit_reaction_over: false,
            has_path: false,
            foe_in_sight_cone: false,
            foe_attack_fast: false,
            foe_attack_weapon: false,
            foe_stunned: false,
            foe_ambush: false,
            flying_form: false,
            near_ground: false,
            own_line_of_sight: false,
            weapon_overheat_percent: None,
            has_home_region: false,
            inside_home_region: false,
            home_point_distance: None,
            tactics_point_valid: false,
            tactics_goal_named: false,
            tactics_at_point: false,
            tactics_point_unset: false,
            tactics_point_distance: None,
            group_members: Vec::new(),
            friend_distance: None,
            turret_distance: None,
            interact_handle: false,
        }
    }
}

impl Senses {
    /// The distance a Targeting spec of `target_type` (and `group`) picks, None when there is none.
    /// FOE is the foe distance, TURRET the turret distance, FRIEND the friend distance
    /// (ai-perceptions 5). GROUP (TARGET_GROUP) is the group target of `group` [data type hash; the
    /// lookup itself is [assumed], see `group_targets`]. [stand-in] every other type (team, home,
    /// waypoint, special) has no distance here, so None.
    pub fn target_distance(&self, target_type: u32, group: u32) -> Option<f32> {
        match target_type {
            TARGET_FOE => self.foe_distance,
            TARGET_TURRET => self.turret_distance,
            TARGET_FRIEND => self.friend_distance,
            TARGET_GROUP => self.group_target_distance(group),
            _ => None,
        }
    }

    /// The distance of the group target of `group`, None when no group target was found for it.
    pub fn group_target_distance(&self, group: u32) -> Option<f32> {
        self.group_targets.iter().find(|g| g.0 == group).map(|g| g.1)
    }

    /// The world point of the group target of `group`, None when no group target was found for it.
    pub fn group_target_point(&self, group: u32) -> Option<[f32; 3]> {
        self.group_target_points.iter().find(|g| g.0 == group).map(|g| g.1)
    }

    /// True when the group target of `group` is in line of sight.
    pub fn group_target_visible(&self, group: u32) -> bool {
        self.group_targets.iter().any(|g| g.0 == group && g.2)
    }

    /// The distance of the nearest group member of `group`, None when there is none (712ecdc7).
    pub fn group_member_distance(&self, group: u32) -> Option<f32> {
        self.group_members.iter().find(|g| g.0 == group).map(|g| g.1)
    }

    /// True when the nearest group member of `group` is in line of sight (0acb9809).
    pub fn group_member_visible(&self, group: u32) -> bool {
        self.group_members.iter().any(|g| g.0 == group && g.2)
    }
}

/// The AI attribute block (`character+0x140`), read from the plain `characterAttributes<X>` block
/// (`ai_bind::attributes`). The offsets are the notes'; names from the loader 008289d0 (third
/// read, A and D of the unpublished notes).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AiAttr {
    /// +0x3d: Idle aims at the target [game 0081e570]. [stand-in] not filled: name unknown.
    pub idle_aim: bool,
    /// +0x5f8 = attackDelay: the attack delay threshold, against `Ctx::attack_clock` [game 00824a00].
    pub attack_delay: f32,
    /// +0x608 = attackRangeFOV: the aim cone, used through cos [game 0081bf10]. [assumed] radians;
    /// the scout's value is 10, which is more than a half-turn in radians, so the unit is doubtful.
    pub aim_cone: f32,
    /// +0x610 = useSpecialFlightMode (third read D; it is NOT a weapon-switch enable). Not filled:
    /// the weapon-switch use stays false [stand-in].
    pub weapon_switch: bool,
    /// +0x620 = dodgeTimer (third read D; the older note called it the dodge threshold). A dodge
    /// needs it at or below `Ctx::dodge_clock` [game 0081c4f0, older read] [assumed] the same use.
    pub dodge_threshold: f32,
    /// +0x62c = autoFacing (third read D; it is NOT the aim test). Not filled: the aim test stays
    /// off [stand-in]. ai_attack's AttackRanged still uses this field as its cone gate; that use is
    /// the older reading and is left as it was.
    pub aim_test: bool,
    /// +0x62c, read as `autoFacing` [data: scout true, global_states.txt line 53418]. The turn gate
    /// of the Idle track (00823640 "attr+0x62c must be set") [game]. Field offset is the notes'.
    pub auto_facing: bool,
    /// accuracyAngleXY [data: scout 5, global_states.txt line 53420]: the yaw spread amplitude of
    /// a shot (007b2f00, the rotation about z by a random draw times +0x634). [assumed] that this
    /// attribute is the +0x634 one, and that its unit is degrees (5 radians would be 286 degrees).
    pub accuracy_angle_xy: f32,
    /// accuracyAngleZ [data: scout 3, global_states.txt line 53421]: the pitch spread amplitude
    /// (007b2f00, +0x638). [assumed] as for accuracy_angle_xy; degrees.
    pub accuracy_angle_z: f32,
    /// The tactics type Tactics' stop restores into `Ctx::tactics_type` (controller+0x694 from the
    /// character's +0x140 block field +0x590) [game 0081fec0]. [stand-in] 0: that field's name is NOT
    /// FOUND in the data.
    pub tactics_default: u32,
    /// The 15 distance attributes, one per assignedPerception hash (ai_perceive::assigned) [game
    /// 008250a0; names and offsets from the loader 008289d0, third read A].
    pub alert_distance: f32,
    pub ambush_distance: f32,
    pub attack_counter_distance: f32,
    pub attack_melee_distance: f32,
    pub attack_range_distance: f32,
    pub attack_range2_distance: f32,
    pub attack_special_distance: f32,
    pub back_pedal_distance: f32,
    pub chase_distance_min: f32,
    pub chase_distance_max: f32,
    pub dodge_distance: f32,
    pub flee_distance: f32,
    pub idle_distance: f32,
    pub strafe_distance: f32,
    pub vehicle_distance: f32,
    /// +0x630 = nodeRangeMax [game 008289d0; data: 500 in all 86 blocks]: the far end of
    /// MoveOnWayPoints' first waypoint search (0081fc60).
    pub node_range_max: f32,
}

/// What a behaviour and a perception may touch. The caller fills the inputs before a step and
/// reads `requests` after it. A score takes `&Ctx`, so a link's message goes out through `emit`.
#[derive(Debug, Default)]
pub struct Ctx {
    /// Outgoing messages to the character (`send`, or `emit` from a score).
    pub messages: RefCell<Vec<Message>>,
    pub me: SelfState,
    /// The one target the controller holds (`ctrl+0x68`); None when there is no foe. Behaviours
    /// read it; the perceptions read `senses` instead.
    pub target: Option<Target>,
    /// What the character senses this step; the perceptions' values read it.
    pub senses: Senses,
    pub attr: AiAttr,
    /// The step length, seconds (the dt the tick takes).
    pub dt: f32,
    /// A uniform draw in [0, 1), filled by the caller for each evaluation. Perceptions scale it
    /// to 0..100 and Dodge to its weight total [assumed] the game's random_range_with_clock is
    /// uniform and these scalings match it.
    pub random: f32,
    /// The latch store of the controller that runs this step (set by `Controller::tick`). Sends
    /// deliver into it at once, and the Message perceptions read it when they score.
    pub latches: LatchStore,
    /// `controller+0xec`: the message value the Message score reads [game 0082f180]. The writer
    /// (0082c5b0) only sets it for an entry that is not the controller's current group entry, and
    /// the scouts' entries have msgEnter 0 there, so it stays 0 for them [assumed].
    pub ctrl_word: u32,
    /// `controller+0xc4` is nonzero: the controller is in a behaviour with its own threshold
    /// [assumed].
    pub in_behaviour: bool,
    /// `controller+0xcc`: the attack clock. Reset by each attack request [assumed].
    pub attack_clock: f32,
    /// `controller+0xe0`: the dodge clock or threshold [assumed]. Dodge needs `attr.dodge_threshold`
    /// at or below it.
    pub dodge_clock: f32,
    /// `controller+0xdc`: weapons are disabled (WeaponSwitch sets it) [game].
    pub weapons_disabled: bool,
    /// Bit b is set when button b is held (`ai_button_down` 008221e0) [game].
    pub held: u64,
    /// `FUN_0091cef0` [unread]: true when the AI may not press buttons now.
    pub input_blocked: bool,
    /// `007219e0` [game]: in StateAttack with a combo window open.
    pub combo_window: bool,
    /// `FUN_008781f0(StateDash)` [unread]: the character may start a dash now.
    pub can_enter_dash: bool,
    /// The dash end test `0074ad50` with the dash state's sub-flag [unknown sense]: the caller
    /// fills it.
    pub dash_done: bool,
    /// The home point (`ctrl+0x38..0x40`, valid when `ctrl+0x44` is set) used by the HOME_POINT and
    /// HOME_REGION targets [game 0081e190, 0081d770]. None: no home point [game].
    pub home: Option<[f32; 3]>,
    /// The path records of MoveOnWayPoints, in order, as world points. [stand-in] used only when
    /// `nodes` is None: the integrator fills them (convoy.rs). With `nodes`, MoveOnWayPoints follows
    /// the level's waypoint nodes as the game does (ai_node, 0082dbe0 and 0082dbb0).
    pub waypoints: Vec<[f32; 3]>,
    /// The level's AI nodes (the global node rings 00c06d48 / 00c06d54), shared by every
    /// character. None: the level's nodes are not loaded; MoveOnWayPoints falls back to
    /// `waypoints` and GoToTarget takes no node route.
    pub nodes: Option<std::sync::Arc<crate::ai_node::AiNodeGraph>>,
    /// Output of Tactics: the tactics type written to `controller+0x694` [game 0081fea0]. Read by
    /// no port yet. Tactics' stop restores `AiAttr::tactics_default`.
    pub tactics_type: u32,
    /// The tactics point of the character's tactics record (`controller+0x690`), when the record is
    /// valid; None otherwise. [game 0081e190] FOE destinations use it, else the own position.
    /// [stand-in] with no nav mesh the integrator puts the foe's point here (`Enemy::brain_input`).
    pub tactics_point: Option<[f32; 3]>,
    /// Output of Idle's track: the point the body turns toward (the track's turn, 00823640 [game]).
    /// None: no turn this step.
    pub face: Option<[f32; 3]>,
    /// The stick a movement behaviour writes this step: a WORLD-space horizontal direction, with
    /// magnitude 0 or 1 [game 0082a8a0 writes a unit direction]. The game writes it rotated into
    /// the character's frame at device+4/+8 by 00822260; that frame conversion is the integrator's
    /// job, not this crate's. The integrator clears it each tick. `Controller::tick` does not
    /// clear it, so a value written in a tick stays until the integrator clears it.
    pub stick: [f32; 2],
    /// The requests the behaviours push this step.
    pub requests: Vec<Request>,
}

impl Ctx {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues a message for the character and delivers it [game 007236c0, ai-messages.md section 1].
    pub fn send(&mut self, hash: u32, arg: u32) {
        self.messages.get_mut().push((hash, arg));
        self.dispatch(hash, arg);
    }

    /// Queues a message from a score, which takes `&Ctx` (the link send of 0082e570 [game]), and
    /// delivers it at once: a perception scored later in the same walk sees the change.
    pub fn emit(&self, hash: u32, arg: u32) {
        self.messages.borrow_mut().push((hash, arg));
        self.dispatch(hash, arg);
    }

    /// The character message handler's arg gate [game 007236c0, ai-messages.md section 1 step 2].
    /// ARG_SELF: the name goes to this controller's latches. ARG_GROUP: the same, and the name is
    /// put in the outbox so the other members of the group get (name, ARG_SELF) after this step.
    /// Any other arg, and name 0, does nothing to the latches (the game side still gets it).
    /// [assumed] the group delivery to the other members is one step late (the integrator routes
    /// the outbox after the step); the game's order is [unknown].
    fn dispatch(&self, name: u32, arg: u32) {
        if name == 0 {
            return;
        }
        match arg {
            ARG_SELF => lock_latches(&self.latches).deliver(name),
            ARG_GROUP => {
                let mut store = lock_latches(&self.latches);
                store.deliver(name);
                store.outbox.push(name);
            }
            _ => {}
        }
    }

    /// Takes the queued messages in send order.
    pub fn drain(&mut self) -> Vec<Message> {
        std::mem::take(self.messages.get_mut())
    }

    /// True when button `button` is held (`ai_button_down` 008221e0) [game].
    pub fn is_held(&self, button: u32) -> bool {
        button < 64 && self.held & (1u64 << button) != 0
    }
}

/// A behaviour: the object a behaviour entry runs. The vtable slots are noted.
pub trait Behaviour: Send + Sync {
    /// Channel index, below `CHANNELS` (vtable +0x34) [game 00820e40].
    fn channel(&self) -> usize;
    /// Score (vtable +0x10): 0 means the behaviour does not pass now [game 00820e40, 0081b4e0].
    fn score(&self, ctx: &Ctx) -> f32;
    /// Vtable +0x14. The game's enter is not read per behaviour; empty by default [unknown].
    fn enter(&mut self, _ctx: &mut Ctx) {}
    /// Vtable +0x18. Empty by default [unknown].
    fn leave(&mut self, _ctx: &mut Ctx) {}
    /// Vtable +0x20: starts the behaviour [game 00822620, 00820e40].
    fn start(&mut self, ctx: &mut Ctx);
    /// Vtable +0x1c: one step of the behaviour, with the step length [game 0081b4e0, 00820e40].
    fn update(&mut self, ctx: &mut Ctx, dt: f32);
    /// Vtable +0x24: stops the behaviour [game 0081b540, 00820e40].
    fn stop(&mut self, ctx: &mut Ctx);
    /// Vtable +0x28: true when the behaviour has finished [game 0081b4e0, 0081b540].
    fn finished(&self, ctx: &Ctx) -> bool;
    /// Vtable +0x30: false keeps the current behaviour entry [game 00821450].
    fn interruptible(&self, ctx: &Ctx) -> bool;
    /// Vtable +0x2c: the target is still valid. The game's answer per class is not read [unknown];
    /// the default is true [assumed]. Used by the one-shot gate (R3).
    fn target_valid(&self, _ctx: &Ctx) -> bool {
        true
    }
    /// Called once per controller step before the brain runs [assumed]: the path planner's
    /// per-step throttle is reset here. Empty for the behaviours that have nothing to reset.
    fn new_frame(&mut self) {}
}

/// The perception aggregate of an entry or a root (vtable +0x10 = `0082e7e0`). 1 with no
/// perceptions, 0 when rejected [game 0082e7e0, as noted in the unpublished notes].
pub type Scorer = Box<dyn Fn(&Ctx) -> f32 + Send + Sync>;

/// The scorer for an entry with no perceptions: always 1 [game 0082e7e0].
pub fn no_perceptions() -> Scorer {
    Box::new(|_ctx: &Ctx| 1.0)
}

/// A behaviour entry: one behaviour of a goal's list, or a persistent entry [game 00821450,
/// 0081b4e0]. The data's fields: `priority`, `rampUpTime`, `oneShot`, `msgEnter`, `msgExit`,
/// `msgFinish` (R1).
pub struct BehaviourEntry {
    /// `entry+0x14`: static priority; the list is sorted by it, highest first [game 00821450].
    pub priority: f32,
    /// `entry+0x10`: the behaviour object.
    pub behaviour: Box<dyn Behaviour>,
    /// `entry+0x1c`: a name hash, read by `hold_time` [data rampUpTime] [game 0082eb10].
    pub hold_hash: u32,
    /// `entry+0x20`: a one-shot entry runs to its end once and is then not usable (R3) [data].
    pub one_shot: bool,
    /// Message sent when the entry starts (data msgEnter).
    pub msg_enter: Option<u32>,
    /// Message sent when the entry stops (data msgExit).
    pub msg_exit: Option<u32>,
    /// Message sent once per run, when the behaviour reports done (data msgFinish); the
    /// once-per-run rule is the port's, see `finished_sent`.
    pub msg_finish: Option<u32>,
    /// The argument of the three messages above: the entry's data msgType [assumed] (the game's
    /// entry-send arg pushes in 00822620 / 00822770 are not yet read; ai-messages.md section 8).
    pub msg_arg: u32,
    /// The finish message of this run is sent (or was not needed). Reset in `start` [assumed]: the
    /// game's own guard on the done message is unread, so a finished behaviour would otherwise
    /// resend it each step.
    finished_sent: bool,
}

impl BehaviourEntry {
    /// An entry with no hold, not one-shot, and no messages.
    pub fn new(priority: f32, behaviour: Box<dyn Behaviour>) -> Self {
        Self {
            priority,
            behaviour,
            hold_hash: 0,
            one_shot: false,
            msg_enter: None,
            msg_exit: None,
            msg_finish: None,
            msg_arg: 0,
            finished_sent: false,
        }
    }

    /// R3: a one-shot entry is usable while it is not done and its target is valid [assumed]: the
    /// game's `00824190` checks the target only for entries in `ctrl+0xb0`, and that list's
    /// membership rule is unread, so the check is applied to every one-shot entry here.
    pub fn usable(&self, ctx: &Ctx, done: bool) -> bool {
        !self.one_shot || (!done && self.behaviour.target_valid(ctx))
    }

    /// Start (vtable +0x20), then the enter message if set.
    pub fn start(&mut self, ctx: &mut Ctx) {
        self.finished_sent = false;
        self.behaviour.start(ctx);
        if let Some(m) = self.msg_enter {
            ctx.send(m, self.msg_arg);
        }
    }

    /// One step (vtable +0x1c). When the behaviour reports done, the finish message goes out once
    /// for this run [assumed, see `finished_sent`].
    pub fn update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.behaviour.update(ctx, dt);
        if !self.finished_sent && self.behaviour.finished(ctx) {
            self.finished_sent = true;
            if let Some(m) = self.msg_finish {
                ctx.send(m, self.msg_arg);
            }
        }
    }

    /// Stop (vtable +0x24), then the exit message if set. [assumed] the goal-end finish message of
    /// 0081b540 step 5 is not sent again here: `update` already sent it for this run.
    pub fn stop(&mut self, ctx: &mut Ctx) {
        self.behaviour.stop(ctx);
        if let Some(m) = self.msg_exit {
            ctx.send(m, self.msg_arg);
        }
    }
}

/// A goal: a brain entry (the data's goalList entry). It holds its perception aggregate, its enter
/// and leave messages, and its behaviour entries, sorted by static priority (R1) [game 00821450,
/// 0082c320].
pub struct Goal {
    /// `entry+0x14`: static priority of the goal in its brain [game 00821450].
    pub priority: f32,
    /// The goal's perception aggregate (`entry.vtable+0x10`, `0082e7e0`) [game].
    pub scorer: Scorer,
    /// Message sent when the goal becomes current (data msgEnter) [game 00822770].
    pub enter_msg: Option<u32>,
    /// Message sent when the goal stops being current (data msgExit) [game 00822770].
    pub leave_msg: Option<u32>,
    /// The argument of the goal's enter and leave messages: the goalList entry's data msgType
    /// [assumed] (the game's arg is `entry+0x28` per 00822770; the mapping to msgType is not read).
    pub arg: u32,
    /// The behaviour entries, highest static priority first.
    pub behaviours: Vec<BehaviourEntry>,
}

impl Goal {
    /// A goal with no messages and argument 0. Sorts the behaviour entries by descending static
    /// priority [assumed] the game's loader sorts them (not read); a stable sort.
    pub fn new(priority: f32, scorer: Scorer, mut behaviours: Vec<BehaviourEntry>) -> Self {
        behaviours.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap_or(Ordering::Equal));
        Self {
            priority,
            scorer,
            enter_msg: None,
            leave_msg: None,
            arg: 0,
            behaviours,
        }
    }

    /// The goal's score and the behaviour entry it selects (R1 steps 1 to 4). `goal_index` is this
    /// goal's place in its brain, for the done set.
    /// 1. Gate: some behaviour entry is usable (not one-shot, or one-shot and not done and valid).
    ///    None usable: score 0.
    /// 2. A = the goal's perception aggregate. A == 0: score 0.
    /// 3. The walk over the behaviour entries, as SELECT walks the goals: stop when an entry's
    ///    static priority is `<=` the best score so far; score = the entry's behaviour score; keep
    ///    the highest above 0. [assumed] an entry that is not usable is skipped in the walk, so a
    ///    finished one-shot is never run again (R3). None selected: score 0.
    /// 4. Otherwise the score is A, and the selected entry is returned.
    pub fn evaluate(&self, ctx: &Ctx, goal_index: usize, done: &Done) -> (f32, Option<usize>) {
        let usable = |bi: usize| self.behaviours[bi].usable(ctx, done.contains(&(goal_index, bi)));
        if !(0..self.behaviours.len()).any(&usable) {
            return (0.0, None);
        }
        let a = (self.scorer)(ctx);
        if a <= 0.0 {
            return (0.0, None);
        }
        let mut best: Option<usize> = None;
        let mut best_score = 0.0_f32;
        for (bi, entry) in self.behaviours.iter().enumerate() {
            if entry.priority <= best_score {
                break;
            }
            if !usable(bi) {
                continue;
            }
            let score = entry.behaviour.score(ctx);
            if score > best_score {
                best_score = score;
                best = Some(bi);
            }
        }
        match best {
            Some(bi) => (a, Some(bi)),
            None => (0.0, None),
        }
    }
}

/// A persistent entry: a behaviour that runs by channel, outside the goal list. The game's
/// persistent list nodes hold an entry whose `+0x10` is the behaviour (driver step 7a; the settled
/// tick uses the same walk) [game 00820e10].
pub struct Persistent {
    pub entry: BehaviourEntry,
    /// `node+0xc`: started and not yet stopped [game 00820e10].
    pub running: bool,
}

/// A brain: the goals, sorted by static priority, highest first (`brain+0x34`) [game 00821450].
pub struct Brain {
    pub goals: Vec<Goal>,
}

/// What SELECT decided: keep the current behaviour entry (it cannot be interrupted), or the best
/// (goal index, behaviour entry index), or None when no goal scored above 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Keep,
    Best(Option<(usize, usize)>),
}

impl Brain {
    /// Builds a brain, sorting the goals by descending static priority. [assumed] the game's
    /// loader sorts them (not read): a stable sort, so list order holds on ties.
    pub fn new(mut goals: Vec<Goal>) -> Self {
        goals.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap_or(Ordering::Equal));
        Self { goals }
    }

    /// SELECT [game 00821450], two levels (R1). `current` is the controller's (goal, behaviour
    /// entry) now running. Steps:
    /// 1. If that behaviour entry is not interruptible, keep it (`Pick::Keep`).
    /// 2. Otherwise walk the goals in order. Stop at the first goal whose static priority is `<=`
    ///    the best score so far. Each goal is scored by `Goal::evaluate`. Take the highest score
    ///    above 0; on a tie the earlier goal keeps the win. [assumed] the best score starts at 0,
    ///    and a static priority is an upper bound of the score (the early stop relies on it).
    pub fn select(&self, current: Option<(usize, usize)>, ctx: &Ctx, done: &Done) -> Pick {
        if let Some((g, b)) = current
            && let Some(entry) = self.goals.get(g).and_then(|goal| goal.behaviours.get(b))
                && !entry.behaviour.interruptible(ctx) {
            return Pick::Keep;
        }
        let mut best: Option<(usize, usize)> = None;
        let mut best_score = 0.0_f32;
        for (gi, goal) in self.goals.iter().enumerate() {
            if goal.priority <= best_score {
                break;
            }
            let (score, bi) = goal.evaluate(ctx, gi, done);
            if score > best_score
                && let Some(bi) = bi {
                best_score = score;
                best = Some((gi, bi));
            }
        }
        Pick::Best(best)
    }
}

/// A brain root: the persistent list and the brain (`root+0x20`, `root+0x10`) [game 00820e10].
/// `perceptions` is the score the brain-choice pass reads (`00827a90`, vtable +0x10 of the
/// brainList entry). [assumed] that is this root's own scorer, since the settled tick says the
/// root wraps the brain; settled by the object at the brainList entry's vtable.
pub struct Root {
    pub perceptions: Scorer,
    pub persistent: Vec<Persistent>,
    /// `root+0x10`: the thinking brain. None: the tick runs the persistent pass and returns 0.
    pub brain: Option<Brain>,
}

/// The goal state the controller keeps for the current brain: `controller+0xc` (the goal),
/// `+0x10` (the behaviour entry of that goal, as its index) and `+0xc0` (the time in this entry).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Current {
    pub goal: Option<usize>,
    pub behaviour: Option<usize>,
    pub clock: f32,
}

/// The character's AI controller. `brains` is the character's brainList (its roots) [data].
pub struct Controller {
    pub brains: Vec<Root>,
    /// `controller+8`: the current brain, an index into `brains`.
    pub brain: Option<usize>,
    /// `controller+0xc`, `+0x10`, `+0xc0`.
    pub cur: Current,
    /// One-shot behaviour entries that finished in the current brain (R3) [assumed]. Cleared by
    /// `start` and on a brain change.
    pub done: Done,
    /// The latch store of this controller's Message perceptions (shared with each step's `Ctx`).
    pub latches: LatchStore,
}

/// The hold time of a behaviour entry, by its name hash [game 0082eb10]. Values as the game gives
/// them. The names behind the hashes are not resolved [unknown].
pub fn hold_time(hash: u32) -> f32 {
    match hash {
        0x63df2be6 => 0.2,
        0x65210a65 => 0.5,
        0xd7a6b14b => 1.0,
        0x7540327c => 1.5,
        0x73be13ff => 2.0,
        0x4c6393c4 => 0.0, // tested explicitly in the game, and 0 [game 0082eb10]
        _ => 0.0,
    }
}

/// 00827a90 (ai_best_brain) [game]: the root with the highest score. Starts from -1.0, so the
/// first root wins ties and a score of 0 still counts. None when there are no roots.
pub fn best_brain(roots: &[Root], ctx: &Ctx) -> Option<usize> {
    let mut best = -1.0_f32;
    let mut pick = None;
    for (i, root) in roots.iter().enumerate() {
        let score = (root.perceptions)(ctx);
        if score > best {
            best = score;
            pick = Some(i);
        }
    }
    pick
}

fn goal_ref(root: &Root, g: usize) -> Option<&Goal> {
    root.brain.as_ref()?.goals.get(g)
}

fn entry_ref(root: &Root, g: usize, b: usize) -> Option<&BehaviourEntry> {
    goal_ref(root, g)?.behaviours.get(b)
}

fn entry_mut(root: &mut Root, g: usize, b: usize) -> Option<&mut BehaviourEntry> {
    root.brain.as_mut()?.goals.get_mut(g)?.behaviours.get_mut(b)
}

/// The persistent pass, step 1 of the settled tick [game 00820e10, disasm checked]. The
/// claimed table lives on the stack and is zeroed each tick. For each entry in list order:
/// if its channel is not yet claimed and its score is not 0, start it once, update it, and
/// claim the channel. Otherwise, if it was running, stop it. So at most one persistent
/// behaviour runs per channel per tick. [assumed] a channel index at or above 13 never
/// claims (not seen in the game; the data is not checked).
fn persistent_pass(list: &mut [Persistent], ctx: &mut Ctx, dt: f32) {
    let mut claimed = [false; CHANNELS];
    for p in list.iter_mut() {
        let ch = p.entry.behaviour.channel();
        let free = ch < CHANNELS && !claimed[ch];
        if free && p.entry.behaviour.score(ctx) != 0.0 {
            if !p.running {
                p.entry.start(ctx);
                p.running = true;
            }
            p.entry.update(ctx, dt);
            claimed[ch] = true;
        } else if p.running {
            p.entry.stop(ctx);
            p.running = false;
        }
    }
}

impl Current {
    /// The (goal, behaviour entry) pair, when both are set.
    fn pair(&self) -> Option<(usize, usize)> {
        Some((self.goal?, self.behaviour?))
    }

    /// 00822770 (group entry change, here the goal change) [game]: if the goal changes, drop the
    /// current behaviour entry (`set_behaviour(None)`), send the old goal's leave message and the
    /// new goal's enter message (each with the goal's argument), and store the new goal.
    pub fn set_goal(&mut self, root: &mut Root, ctx: &mut Ctx, new: Option<usize>) {
        if new == self.goal {
            return;
        }
        self.set_behaviour(root, ctx, None);
        if let Some(old) = self.goal
            && let Some(g) = goal_ref(root, old)
                && let Some(m) = g.leave_msg {
            ctx.send(m, g.arg);
        }
        if let Some(n) = new
            && let Some(g) = goal_ref(root, n)
                && let Some(m) = g.enter_msg {
            ctx.send(m, g.arg);
        }
        self.goal = new;
    }

    /// 00822620 (ai_goal_change, here the behaviour entry change) [game]: if the entry changes,
    /// stop the old one (`BehaviourEntry::stop`), start the new one (`BehaviourEntry::start`),
    /// store it, and zero the clock. `new` is the entry's index in the current goal.
    pub fn set_behaviour(&mut self, root: &mut Root, ctx: &mut Ctx, new: Option<usize>) {
        if new == self.behaviour {
            return;
        }
        if let Some(old) = self.behaviour.take()
            && let Some(g) = self.goal
                && let Some(entry) = entry_mut(root, g, old) {
            entry.stop(ctx);
        }
        if let (Some(g), Some(n)) = (self.goal, new)
            && let Some(entry) = entry_mut(root, g, n) {
            entry.start(ctx);
            self.behaviour = Some(n);
        }
        self.clock = 0.0;
    }

    /// Runs the current behaviour entry for one step (`0081b4e0`, ai_goal_run) [game]: update,
    /// and a finished one-shot entry joins the done set (R3). Returns true when an entry ran.
    fn run_current(&self, root: &mut Root, ctx: &mut Ctx, dt: f32, done: &mut Done) -> bool {
        let (Some(g), Some(b)) = (self.goal, self.behaviour) else {
            return false;
        };
        let Some(entry) = entry_mut(root, g, b) else {
            return false;
        };
        entry.update(ctx, dt);
        if entry.one_shot && entry.behaviour.finished(ctx) {
            done.insert((g, b));
        }
        true
    }

    /// THINK, the brain's vtable +0x1c [game 00821410], run by the settled tick when the entry is
    /// not held: select (00821450), switch the goal and the behaviour entry, then run it if one is
    /// current. Returns true when an entry ran.
    fn think(&mut self, root: &mut Root, ctx: &mut Ctx, dt: f32, done: &mut Done) -> bool {
        let pick = match root.brain.as_ref() {
            Some(brain) => brain.select(self.pair(), ctx, done),
            None => return false,
        };
        let (goal, behaviour) = match pick {
            Pick::Keep => (self.goal, self.behaviour),
            Pick::Best(Some((g, b))) => (Some(g), Some(b)),
            Pick::Best(None) => (None, None),
        };
        self.set_goal(root, ctx, goal);
        self.set_behaviour(root, ctx, behaviour);
        self.run_current(root, ctx, dt, done)
    }

    /// The brain tick 00820e10 [game, settled], for one root. Steps:
    /// 1. the persistent pass;
    /// 2. no brain in the root: return false;
    /// 3. a current behaviour entry whose time has not passed its hold (`clock <= hold`,
    ///    `comiss; jbe`) and which is still usable (R3) [assumed] is kept without a new select:
    ///    run it if its behaviour scores above 0 (return true), else return false and nothing runs
    ///    this step;
    /// 4. otherwise THINK.
    ///
    /// The clock is not advanced here; `Controller::tick` advances it.
    fn tick_root(&mut self, root: &mut Root, ctx: &mut Ctx, dt: f32, done: &mut Done) -> bool {
        persistent_pass(&mut root.persistent, ctx, dt);
        if root.brain.is_none() {
            return false;
        }
        if let (Some(g), Some(b)) = (self.goal, self.behaviour)
            && let Some(entry) = entry_ref(root, g, b) {
            let held = self.clock <= hold_time(entry.hold_hash) && entry.usable(ctx, done.contains(&(g, b)));
            if held {
                let scores = entry.behaviour.score(ctx) != 0.0;
                return scores && self.run_current(root, ctx, dt, done);
            }
        }
        self.think(root, ctx, dt, done)
    }
}

impl Controller {
    /// A controller over a character's brainList, with no brain chosen yet and no latches.
    pub fn new(brains: Vec<Root>) -> Self {
        Self::with_latches(brains, Latches::default())
    }

    /// A controller over a character's brainList, with the latch nodes its binder made (one per
    /// Message perception of every brain, ai_bind::build).
    pub fn with_latches(brains: Vec<Root>, latches: Latches) -> Self {
        Self {
            brains,
            brain: None,
            cur: Current::default(),
            done: Done::new(),
            latches: Arc::new(Mutex::new(latches)),
        }
    }

    /// Start (00822430) [game]: every brain of the list is entered, so each Message perception
    /// registers (its latch starts at toggleOn); the first root becomes the current brain. The
    /// enter calls on each brain's objects (00820b70, the `+0x28` list) are [unread] and not made
    /// here. The done set is cleared [assumed] (R3).
    pub fn start(&mut self, ctx: &mut Ctx) {
        ctx.latches = Arc::clone(&self.latches);
        lock_latches(&self.latches).enter_all();
        self.done.clear();
        let first = if self.brains.is_empty() { None } else { Some(0) };
        self.set_brain(ctx, first);
    }

    /// Stop (00822500) [game]: brain = none, then every brain is left, which removes the latches.
    /// [assumed] the goal and the behaviour entry are dropped through the same path as a brain change
    /// (`set_brain(None)`), which the notes do not say; the clear of the two lists (+0xa4, +0xb0)
    /// and the leave calls (00820bd0) are [unread].
    pub fn stop(&mut self, ctx: &mut Ctx) {
        ctx.latches = Arc::clone(&self.latches);
        self.set_brain(ctx, None);
        lock_latches(&self.latches).leave_all();
    }

    /// Delivers a message name to this controller's latches, as the handler does for ARG_SELF
    /// [game 007236c0 to 00823ea0 to 0082f350]. Used for a group broadcast from another character.
    /// The controller-event names (BrainOn, HoverOff, ...) are not applied [stand-in].
    pub fn deliver(&self, name: u32) {
        if name != 0 {
            lock_latches(&self.latches).deliver(name);
        }
    }

    /// Resets the per-step state of every behaviour (`Behaviour::new_frame`): the path planners'
    /// throttle, so one step may plan again [assumed: the game's counter reset point is unread].
    pub fn new_frame(&mut self) {
        for root in &mut self.brains {
            for p in &mut root.persistent {
                p.entry.behaviour.new_frame();
            }
            if let Some(brain) = &mut root.brain {
                for goal in &mut brain.goals {
                    for entry in &mut goal.behaviours {
                        entry.behaviour.new_frame();
                    }
                }
            }
        }
    }

    /// Takes the names this controller broadcast to its group in the steps since the last take
    /// (ARG_GROUP). The caller delivers them to the other members.
    pub fn take_group_messages(&self) -> Vec<u32> {
        std::mem::take(&mut lock_latches(&self.latches).outbox)
    }

    /// Set brain (00827b90) [game]: if the brain changes, clear the goal (which drops the behaviour
    /// entry, `set_goal(None)`), store the new brain, and clear the done set [assumed] (R3): the
    /// one-shot indices belong to the brain they were made in.
    pub fn set_brain(&mut self, ctx: &mut Ctx, new: Option<usize>) {
        if new == self.brain {
            return;
        }
        if let Some(old) = self.brain
            && let Some(root) = self.brains.get_mut(old) {
            self.cur.set_goal(root, ctx, None);
        }
        self.brain = new;
        self.done.clear();
    }

    /// The per-step re-pick [game 008271d0 step 3, 00827a90, 00827b90]. Only when a brain is
    /// already set and `allowed` is true. `allowed` stands for the gate 008225f0 (DAT_00b8d7e9
    /// set and character+0x148 bit 3 clear), which is [unread]: the caller computes it.
    pub fn repick(&mut self, ctx: &mut Ctx, allowed: bool) {
        if self.brain.is_none() || !allowed {
            return;
        }
        let best = best_brain(&self.brains, ctx);
        self.set_brain(ctx, best);
    }

    /// One step of the brain tick for the current brain (00820e10, settled), then the clock.
    /// Returns true when a behaviour entry ran.
    /// Order: the latch timer tick (once per step, [assumed]), the re-pick (step 3 of 008271d0, see
    /// `repick`), then the brain tick. [assumed] the clock `controller+0xc0` advances by dt once per
    /// tick, at the end, as the driver notes (008271d0 writes it last; not checked in disassembly).
    pub fn tick(&mut self, ctx: &mut Ctx, dt: f32) -> bool {
        ctx.dt = dt;
        ctx.latches = Arc::clone(&self.latches);
        lock_latches(&self.latches).tick(dt);
        self.new_frame();
        // [assumed] the re-pick runs every step with its gate open: the gate 008225f0 is unread
        // (the unpublished notes "Brain choice"), and the notes give no rate for the re-pick.
        self.repick(ctx, true);
        let ran = match self.brain {
            Some(bi) if bi < self.brains.len() => {
                self.cur.tick_root(&mut self.brains[bi], ctx, dt, &mut self.done)
            }
            _ => false,
        };
        self.cur.clock += dt;
        ran
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A thread-safe cell for the test fakes (Behaviour and Scorer must be Send + Sync). Same
    /// `get` / `set` as `std::cell::Cell` for a Copy value.
    #[derive(Debug, Default)]
    pub struct Shared<T>(Mutex<T>);

    impl<T: Copy> Shared<T> {
        fn new(v: T) -> Self {
            Shared(Mutex::new(v))
        }
        fn get(&self) -> T {
            *self.0.lock().unwrap()
        }
        fn set(&self, v: T) {
            *self.0.lock().unwrap() = v;
        }
    }

    type Log = Arc<Mutex<Vec<String>>>;
    type Knob = Arc<Shared<f32>>;
    type Flag = Arc<Shared<bool>>;
    type Count = Arc<Shared<u32>>;

    fn knob(v: f32) -> Knob {
        Arc::new(Shared::new(v))
    }

    fn flag(v: bool) -> Flag {
        Arc::new(Shared::new(v))
    }

    fn count() -> Count {
        Arc::new(Shared::new(0))
    }

    fn seen(log: &Log) -> Vec<String> {
        log.lock().unwrap().clone()
    }

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// A perception scorer that counts its calls, so tests can see what SELECT scored.
    fn scorer(v: &Knob, calls: &Count) -> Scorer {
        let (v, calls) = (Arc::clone(v), Arc::clone(calls));
        Box::new(move |_ctx: &Ctx| {
            calls.set(calls.get() + 1);
            v.get()
        })
    }

    /// A fake behaviour that logs its calls as "name.start", "name.update", and so on.
    struct Fake {
        name: &'static str,
        channel: usize,
        score: Knob,
        interruptible: bool,
        finished: Flag,
        log: Log,
    }

    impl Fake {
        fn note(&self, what: &str) {
            self.log.lock().unwrap().push(format!("{}.{}", self.name, what));
        }
    }

    impl Behaviour for Fake {
        fn channel(&self) -> usize {
            self.channel
        }
        fn score(&self, _ctx: &Ctx) -> f32 {
            self.score.get()
        }
        fn start(&mut self, _ctx: &mut Ctx) {
            self.note("start");
        }
        fn update(&mut self, _ctx: &mut Ctx, _dt: f32) {
            self.note("update");
        }
        fn stop(&mut self, _ctx: &mut Ctx) {
            self.note("stop");
        }
        fn finished(&self, _ctx: &Ctx) -> bool {
            self.finished.get()
        }
        fn interruptible(&self, _ctx: &Ctx) -> bool {
            self.interruptible
        }
    }

    fn fake(
        name: &'static str,
        channel: usize,
        score: &Knob,
        interruptible: bool,
        finished: &Flag,
        log: &Log,
    ) -> Box<dyn Behaviour> {
        Box::new(Fake {
            name,
            channel,
            score: Arc::clone(score),
            interruptible,
            finished: Arc::clone(finished),
            log: Arc::clone(log),
        })
    }

    /// A behaviour that counts its score calls and has a fixed score; it logs nothing.
    struct Scored {
        value: f32,
        calls: Count,
    }

    impl Behaviour for Scored {
        fn channel(&self) -> usize {
            0
        }
        fn score(&self, _ctx: &Ctx) -> f32 {
            self.calls.set(self.calls.get() + 1);
            self.value
        }
        fn start(&mut self, _ctx: &mut Ctx) {}
        fn update(&mut self, _ctx: &mut Ctx, _dt: f32) {}
        fn stop(&mut self, _ctx: &mut Ctx) {}
        fn finished(&self, _ctx: &Ctx) -> bool {
            false
        }
        fn interruptible(&self, _ctx: &Ctx) -> bool {
            true
        }
    }

    fn scored(value: f32, calls: &Count) -> Box<dyn Behaviour> {
        Box::new(Scored {
            value,
            calls: Arc::clone(calls),
        })
    }

    fn be(priority: f32, b: Box<dyn Behaviour>) -> BehaviourEntry {
        BehaviourEntry::new(priority, b)
    }

    fn goal(priority: f32, s: Scorer, behaviours: Vec<BehaviourEntry>) -> Goal {
        Goal::new(priority, s, behaviours)
    }

    fn root_of(goals: Vec<Goal>) -> Root {
        Root {
            perceptions: no_perceptions(),
            persistent: Vec::new(),
            brain: Some(Brain::new(goals)),
        }
    }

    /// Compile-time check: the controller (and so the game's Enemy component) can cross threads.
    #[test]
    fn controller_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Controller>();
    }

    /// A controller with two latch nodes and no brain: start registers them (flag = toggleOn),
    /// a delivered name sets or clears them, stop removes them, and a GROUP send goes to the outbox.
    #[test]
    fn latches_follow_the_brain_run_and_messages() {
        let mut store = Latches::default();
        store.add(Latch { name_on: 0x11, name_off: 0x22, toggle: true, ..Latch::default() });
        store.add(Latch { name_on: 0x33, name_off: 0x44, toggle: true, toggle_on: true, ..Latch::default() });
        let mut ctl = Controller::with_latches(Vec::new(), store);
        let node = |ctl: &Controller, i: usize| lock_latches(&ctl.latches).nodes[i];
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);
        assert!(!node(&ctl, 0).flag && node(&ctl, 0).registered, "entered: off");
        assert!(node(&ctl, 1).flag, "entered: toggleOn starts on");
        ctl.deliver(0x11);
        assert!(node(&ctl, 0).flag, "a delivered msgNameOn sets the latch");
        ctl.deliver(0x22);
        assert!(!node(&ctl, 0).flag, "a delivered msgNameOff clears it (toggle)");
        ctx.latches = Arc::clone(&ctl.latches);
        ctx.emit(0x11, ARG_GROUP);
        assert!(node(&ctl, 0).flag, "a GROUP send also delivers to self");
        assert_eq!(ctl.take_group_messages(), vec![0x11], "and is in the outbox");
        assert!(ctl.take_group_messages().is_empty(), "the outbox is taken once");
        ctx.emit(0x11, 0x1234);
        assert_eq!(ctx.drain().len(), 2, "every send reaches the game side");
        ctl.stop(&mut ctx);
        assert!(!node(&ctl, 0).registered && !node(&ctl, 1).flag, "stop leaves and clears");
    }

    #[test]
    fn highest_score_wins_and_lower_priority_is_not_scored() {
        let log = Log::default();
        let (sa, sb, ca, cb) = (knob(0.7), knob(0.9), count(), count());
        let ga = goal(1.0, scorer(&sa, &ca), vec![be(1.0, fake("A", 0, &knob(1.0), true, &flag(false), &log))]);
        let gb = goal(0.5, scorer(&sb, &cb), vec![be(1.0, fake("B", 1, &knob(1.0), true, &flag(false), &log))]);
        let mut ctl = Controller::new(vec![root_of(vec![ga, gb])]);
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);
        assert!(ctl.tick(&mut ctx, 0.1));
        assert_eq!(ctl.cur.goal, Some(0), "A scores 0.7 and wins");
        assert_eq!(ctl.cur.behaviour, Some(0));
        assert_eq!(ca.get(), 1);
        assert_eq!(cb.get(), 0, "0.5 <= 0.7: the lower goal is never scored");
        assert_eq!(seen(&log), lines(&["A.start", "A.update"]));
    }

    #[test]
    fn lower_priority_goal_wins_when_it_scores_higher() {
        let log = Log::default();
        let (sa, sb, ca, cb) = (knob(0.3), knob(0.8), count(), count());
        let ga = goal(1.0, scorer(&sa, &ca), vec![be(1.0, fake("A", 0, &knob(1.0), true, &flag(false), &log))]);
        let gb = goal(0.5, scorer(&sb, &cb), vec![be(1.0, fake("B", 1, &knob(1.0), true, &flag(false), &log))]);
        let mut ctl = Controller::new(vec![root_of(vec![ga, gb])]);
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);
        ctl.tick(&mut ctx, 0.1);
        assert_eq!(ctl.cur.goal, Some(1), "B scores 0.8 and wins");
        assert_eq!(cb.get(), 1, "0.5 > 0.3: B is scored");
    }

    #[test]
    fn two_level_selection_picks_the_goal_then_the_behaviour_entry() {
        let (c0, c1, c2) = (count(), count(), count());
        let g0 = goal(
            1.0,
            no_perceptions(),
            vec![be(1.0, scored(0.0, &c0)), be(0.5, scored(0.6, &c1))],
        );
        let g1 = goal(0.5, no_perceptions(), vec![be(1.0, scored(0.8, &c2))]);
        let mut ctl = Controller::new(vec![root_of(vec![g0, g1])]);
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);
        assert!(ctl.tick(&mut ctx, 0.1));
        assert_eq!(ctl.cur.goal, Some(0), "goal 0 wins with its entry 1 (0.6)");
        assert_eq!(ctl.cur.behaviour, Some(1), "entry 0 scored 0, entry 1 scored 0.6");
        assert_eq!(c2.get(), 0, "goal 1 is not scored: 0.5 <= the goal 0 score of 1.0");
    }

    #[test]
    fn early_stop_inside_a_goal() {
        let (cx, cy) = (count(), count());
        let g = goal(
            1.0,
            no_perceptions(),
            vec![be(1.0, scored(0.9, &cx)), be(0.5, scored(0.3, &cy))],
        );
        let done = Done::new();
        let ctx = Ctx::new();
        assert_eq!(g.evaluate(&ctx, 0, &done), (1.0, Some(0)));
        assert_eq!(cx.get(), 1);
        assert_eq!(cy.get(), 0, "0.5 <= 0.9: the lower entry is never scored");
    }

    #[test]
    fn goal_with_a_zero_perception_scores_zero() {
        let cb = count();
        let zero = knob(0.0);
        let g = goal(1.0, scorer(&zero, &count()), vec![be(1.0, scored(1.0, &cb))]);
        let ctx = Ctx::new();
        assert_eq!(g.evaluate(&ctx, 0, &Done::new()), (0.0, None), "A == 0: score 0");
        assert_eq!(cb.get(), 0, "the behaviour is not scored");
    }

    #[test]
    fn goal_whose_behaviours_all_score_zero_is_not_selected() {
        let (c0, c1) = (count(), count());
        let ga = goal(1.0, no_perceptions(), vec![be(1.0, scored(0.0, &c0))]);
        let gb = goal(0.5, no_perceptions(), vec![be(1.0, scored(1.0, &c1))]);
        let ctx = Ctx::new();
        let done = Done::new();
        assert_eq!(ga.evaluate(&ctx, 0, &done), (0.0, None), "every entry scores 0: score 0");
        let mut ctl = Controller::new(vec![root_of(vec![ga, gb])]);
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);
        ctl.tick(&mut ctx, 0.1);
        assert_eq!(ctl.cur.goal, Some(1), "goal 0 is not selected");
    }

    #[test]
    fn uninterruptible_entry_is_kept() {
        let log = Log::default();
        let (sa, sb, ca, cb) = (knob(1.0), knob(0.0), count(), count());
        // The goal A is current and its entry cannot be interrupted; the goal B, with the higher
        // priority, scores 0 at first.
        let gb = goal(1.0, scorer(&sb, &cb), vec![be(1.0, fake("B", 1, &knob(1.0), true, &flag(false), &log))]);
        let ga = goal(0.5, scorer(&sa, &ca), vec![be(1.0, fake("A", 0, &knob(1.0), false, &flag(false), &log))]);
        let mut ctl = Controller::new(vec![root_of(vec![gb, ga])]);
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);
        ctl.tick(&mut ctx, 0.1);
        assert_eq!(ctl.cur.goal, Some(1), "A is chosen while B scores 0");
        assert_eq!(cb.get(), 1);

        sb.set(1.0);
        ctl.tick(&mut ctx, 0.1);
        assert_eq!(ctl.cur.goal, Some(1), "A cannot be interrupted, so it is kept");
        assert_eq!(cb.get(), 1, "SELECT returned early: B was not scored again");
        assert_eq!(seen(&log), lines(&["A.start", "A.update", "A.update"]));
    }

    #[test]
    fn switching_goal_sends_messages_in_order_and_zeroes_clock() {
        let log = Log::default();
        let (sa, sb) = (knob(1.0), knob(1.0));
        let (ca, cb) = (count(), count());
        let mut ea = be(1.0, fake("A", 0, &knob(1.0), true, &flag(false), &log));
        ea.msg_enter = Some(10);
        ea.msg_exit = Some(13);
        let mut eb = be(1.0, fake("B", 1, &knob(1.0), true, &flag(false), &log));
        eb.msg_enter = Some(11);
        let mut ga = goal(1.0, scorer(&sa, &ca), vec![ea]);
        ga.enter_msg = Some(1);
        ga.leave_msg = Some(2);
        ga.arg = 7;
        let mut gb = goal(0.5, scorer(&sb, &cb), vec![eb]);
        gb.enter_msg = Some(3);
        gb.leave_msg = Some(4);
        gb.arg = 7;
        let mut ctl = Controller::new(vec![root_of(vec![ga, gb])]);
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);

        ctl.tick(&mut ctx, 0.1);
        assert_eq!(ctx.drain(), vec![(1, 7), (10, 0)], "enter goal A, then A's entry starts");

        sa.set(0.0);
        ctl.tick(&mut ctx, 0.1);
        assert_eq!(
            ctx.drain(),
            vec![(13, 0), (2, 7), (3, 7), (11, 0)],
            "A's entry exits, goal A leaves, goal B enters, B's entry starts"
        );
        assert_eq!(
            seen(&log),
            lines(&["A.start", "A.update", "A.stop", "B.start", "B.update"])
        );
        assert!((ctl.cur.clock - 0.1).abs() < 1e-6, "clock zeroed on the switch, then dt added");
    }

    #[test]
    fn held_entry_is_kept_for_its_hold_then_replaced() {
        let log = Log::default();
        let (sa, sb) = (knob(1.0), knob(0.0));
        let (ca, cb) = (count(), count());
        // A's entry has a 1 s hold. B has the higher priority and appears at the second step.
        let mut ea = be(1.0, fake("A", 0, &knob(1.0), true, &flag(false), &log));
        ea.hold_hash = 0xd7a6b14b;
        let gb = goal(1.0, scorer(&sb, &cb), vec![be(1.0, fake("B", 1, &knob(1.0), true, &flag(false), &log))]);
        let ga = goal(0.5, scorer(&sa, &ca), vec![ea]);
        let mut ctl = Controller::new(vec![root_of(vec![gb, ga])]);
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);

        ctl.tick(&mut ctx, 0.5); // step 1: no entry yet, select A
        assert_eq!(ctl.cur.goal, Some(1));
        sb.set(1.0);

        ctl.tick(&mut ctx, 0.5); // clock 0.5 <= 1.0: kept
        assert_eq!(ctl.cur.goal, Some(1));
        assert_eq!(cb.get(), 1, "held: B not scored");

        ctl.tick(&mut ctx, 0.5); // clock 1.0 <= 1.0: kept
        assert_eq!(ctl.cur.goal, Some(1));
        assert_eq!(cb.get(), 1);

        ctl.tick(&mut ctx, 0.5); // clock 1.5 > 1.0: think, B wins
        assert_eq!(ctl.cur.goal, Some(0));
        assert_eq!(cb.get(), 2);
    }

    #[test]
    fn held_entry_with_zero_score_does_not_run_and_nothing_else_runs() {
        let log = Log::default();
        let (sa, sb) = (knob(1.0), knob(1.0));
        let (ca, cb) = (count(), count());
        let behaviour_score = knob(1.0);
        let mut ea = be(1.0, fake("A", 0, &behaviour_score, true, &flag(false), &log));
        ea.hold_hash = 0xd7a6b14b;
        let ga = goal(1.0, scorer(&sa, &ca), vec![ea]);
        let gb = goal(0.5, scorer(&sb, &cb), vec![be(1.0, fake("B", 1, &knob(1.0), true, &flag(false), &log))]);
        let mut ctl = Controller::new(vec![root_of(vec![ga, gb])]);
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);

        assert!(ctl.tick(&mut ctx, 0.5));
        let before = seen(&log).len();
        behaviour_score.set(0.0);
        assert!(!ctl.tick(&mut ctx, 0.5), "held entry scores 0: nothing runs");
        assert_eq!(seen(&log).len(), before, "A.update not called this step");
        assert_eq!(cb.get(), 0, "no select, so B is not scored");
        assert_eq!(ctl.cur.goal, Some(0), "the goal is kept");
        assert_eq!(ctl.cur.behaviour, Some(0), "the entry is kept");
    }

    #[test]
    fn one_shot_entry_runs_once_then_is_not_selected() {
        let log = Log::default();
        let finished = flag(true);
        let mut ea = be(1.0, fake("A", 0, &knob(1.0), true, &finished, &log));
        ea.one_shot = true;
        let ga = goal(1.0, no_perceptions(), vec![ea]);
        let gb = goal(0.5, no_perceptions(), vec![be(1.0, fake("B", 1, &knob(1.0), true, &flag(false), &log))]);
        let mut ctl = Controller::new(vec![root_of(vec![ga, gb])]);
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);

        ctl.tick(&mut ctx, 0.1);
        assert_eq!(ctl.cur.goal, Some(0), "A runs first");
        assert!(ctl.done.contains(&(0, 0)), "A finished: it is marked done");

        ctl.tick(&mut ctx, 0.1);
        assert_eq!(ctl.cur.goal, Some(1), "A is done, so its goal is not usable; B runs");
        assert_eq!(
            seen(&log),
            lines(&["A.start", "A.update", "A.stop", "B.start", "B.update"])
        );
    }

    #[test]
    fn persistent_pass_runs_one_behaviour_per_channel() {
        let log = Log::default();
        let (p1, p2, p3) = (knob(1.0), knob(0.0), knob(0.0));
        let root = Root {
            perceptions: no_perceptions(),
            persistent: vec![
                Persistent {
                    entry: be(1.0, fake("P1", 2, &p1, true, &flag(false), &log)),
                    running: false,
                },
                Persistent {
                    entry: be(1.0, fake("P2", 2, &p2, true, &flag(false), &log)),
                    running: false,
                },
                Persistent {
                    entry: be(1.0, fake("P3", 5, &p3, true, &flag(false), &log)),
                    running: false,
                },
            ],
            brain: Some(Brain::new(Vec::new())),
        };
        let mut ctl = Controller::new(vec![root]);
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);

        ctl.tick(&mut ctx, 0.1);
        assert_eq!(seen(&log), lines(&["P1.start", "P1.update"]));

        p1.set(0.0);
        p2.set(1.0);
        ctl.tick(&mut ctx, 0.1);
        assert_eq!(
            seen(&log),
            lines(&["P1.start", "P1.update", "P1.stop", "P2.start", "P2.update"]),
            "P1 stops when it stops passing; P2 takes channel 2"
        );

        ctl.tick(&mut ctx, 0.1);
        assert_eq!(
            seen(&log).len(),
            6,
            "P2 is already running: only its update runs"
        );
        assert_eq!(seen(&log)[5], "P2.update");
    }

    #[test]
    fn finished_behaviour_sends_done_message_once_per_run() {
        let log = Log::default();
        let finished = flag(true);
        let mut ea = be(1.0, fake("A", 0, &knob(1.0), true, &finished, &log));
        ea.msg_finish = Some(77);
        let mut ctl = Controller::new(vec![root_of(vec![goal(1.0, no_perceptions(), vec![ea])])]);
        let mut ctx = Ctx::new();
        ctl.start(&mut ctx);

        assert!(ctl.tick(&mut ctx, 0.1));
        assert_eq!(ctx.drain(), vec![(77, 0)]);

        assert!(ctl.tick(&mut ctx, 0.1), "the finished entry keeps running");
        assert_eq!(ctx.drain(), Vec::new(), "the done message is not resent in the same run");
    }

    #[test]
    fn start_and_brain_change_clear_the_done_set() {
        let mut ctl = Controller::new(vec![root_of(Vec::new()), root_of(Vec::new())]);
        let mut ctx = Ctx::new();
        ctl.done.insert((0, 0));
        ctl.start(&mut ctx);
        assert!(ctl.done.is_empty(), "start clears it");
        ctl.done.insert((0, 1));
        ctl.set_brain(&mut ctx, Some(1));
        assert!(ctl.done.is_empty(), "a brain change clears it");
    }

    #[test]
    fn hold_times_match_the_game() {
        assert_eq!(hold_time(0x63df2be6), 0.2);
        assert_eq!(hold_time(0x65210a65), 0.5);
        assert_eq!(hold_time(0xd7a6b14b), 1.0);
        assert_eq!(hold_time(0x7540327c), 1.5);
        assert_eq!(hold_time(0x73be13ff), 2.0);
        assert_eq!(hold_time(0x4c6393c4), 0.0);
        assert_eq!(hold_time(0x12345678), 0.0);
    }

    #[test]
    fn best_brain_takes_highest_and_first_on_tie() {
        let root = |s: f32| Root {
            perceptions: Box::new(move |_ctx: &Ctx| s),
            persistent: Vec::new(),
            brain: None,
        };
        let ctx = Ctx::new();
        assert_eq!(best_brain(&[root(0.0), root(0.0)], &ctx), Some(0), "0 still counts");
        assert_eq!(best_brain(&[root(0.2), root(0.9), root(0.9)], &ctx), Some(1));
        assert_eq!(best_brain(&[], &ctx), None);
    }
}
