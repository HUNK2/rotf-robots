//! Installed Decepticon scout and shared character combat. See docs/damage.md.
use crate::camera::Sight;
use crate::formats::{
    hash::crc32,
    lxb::{DataFile, Node},
    pack::{self, Pack},
};
use crate::melee::{Target, surface};
use crate::{
    ai_attack, ai_bind,
    ai_brain::{self, AiAttr, Ctx, Request, Senses, SelfState},
    ai_data::{self, AiData},
    ai_move::{PathSource, StraightLine},
    ai_tactics::{self, Record as TacticsRecord},
    character, control,
    navmesh::NavMesh,
    navpath::{self, Agent, MeshPath},
    damage::{Actor, Packet},
    sim::{self, Arena, Input, State},
    tuning::{self, Tuning},
    weapons::{self, Fired, Part, Weapon, WeaponTuning},
};
use glam::{Vec2, Vec3};
use std::{cmp::Ordering, path::Path, sync::Arc};

/// The body's turn rate toward a face point (Idle's track), radians a second [assumed]: 240 degrees a
/// second, the scout's turnSpeedAttackControl (global_states.txt line 52911). The game's rate is not
/// read: 00823870, which the first note named as its source, is a heading-error routine that writes
/// nothing (ledger, 2026-10-20), so the rate's source is still open. Settles: the Idle track's turn
/// speed (00823640, the caller of the face test; read it for the value it passes).
const AI_FACE_RATE: f32 = 240.0 * std::f32::consts::PI / 180.0;

/// The sight corridor's agent radius, as a factor of the character radius. The factor 1.1 is [game
/// 00827d37]: 00827ce0 multiplies the STEP reach term (STEP+0x170[STEP+0x244]) by [b6a8d8] = 1.1 before
/// it calls the corridor 006f0950. OPEN: the base here is the robot radius [assumed]; the game's base
/// is that reach term, not the robot radius. Settles: the reach term's value for a scout's form, against
/// its robot radius.
const SIGHT_RADIUS_FACTOR: f32 = 1.1;
/// The sight agent's height, metres [stand-in]: the value `ai_tactics` uses for its sight test. OPEN:
/// 00827ce0 passes no height of its own; the corridor's height argument is read from 006f0950.
/// Settles: the agent height that 006f0950 takes, and where it comes from.
const SIGHT_HEIGHT: f32 = 1.8;
/// The path agent's height when the scout's data has none, metres [assumed] 4 m (the value of the
/// nav_dump agent). The scout's own robot height is used when present [data].
const FALLBACK_AGENT_HEIGHT: f32 = 4.0;
/// How close the character must be to its tactics point to count as at it, metres [assumed] 0.2 m
/// (the follower's own-to-path tolerance). OPEN: the game's 00833cb0 is a point-set test, not a
/// distance. Its "near" perception (a62df181) gives the 3D distance to the point, and the brain's
/// threshold on that value is in its data. Settles: the threshold on perceptionTacticsPointNear in the
/// brain data (brains_full.txt), read against this 0.2 m.
const AT_TACTICS_POINT: f32 = 0.2;
/// The ground probe starts this far above the feet, metres [assumed] 1.5 m: the margin of the rule in
/// the unpublished notes (the game's value is unread). Settle: the game's ground probe for a walker.
pub const GROUND_MARGIN: f32 = 1.5;
/// The level's floor ball sits this far below its probe top (level_world FLOOR_BALL, 0.3 m [data]
/// from that file), so the probe is started at `z + GROUND_MARGIN - sim::STEP - PROBE_BALL`: its top is
/// then `z + GROUND_MARGIN`.
const PROBE_BALL: f32 = 0.3;

/// The collision ground under `(x, y)` for feet at `z`: `floor` (the arena's `floor`) probed from
/// `z + GROUND_MARGIN` going down. None when no floor is found (`NO_FLOOR`). Never probed from a height
/// far above (it finds roofs) or from a mesh floor (it can start under the ground).
pub fn probe_ground(floor: impl Fn(f32, f32, f32) -> f32, x: f32, y: f32, z: f32) -> Option<f32> {
    let found = floor(x, y, z + GROUND_MARGIN - sim::STEP - PROBE_BALL);
    (found > crate::level_world::NO_FLOOR).then_some(found)
}

/// A grounded walker's feet take the collision ground under them, probed from `GROUND_MARGIN` above
/// its feet (`probe_ground`); no floor found: z kept [assumed]. Only grounded
/// feet are moved: when the feet are above the floor under them by more than a step (the sim's own
/// airborne test, sim.rs `step`), the jump or the fall is left to the sim. Nothing is taken from a mesh.
pub fn settle_on_ground(pos: &mut Vec3, floor: impl Fn(f32, f32, f32) -> f32 + Copy) {
    let here = floor(pos.x, pos.y, pos.z);
    if here <= crate::level_world::NO_FLOOR || pos.z > here + sim::STEP {
        return;
    }
    if let Some(z) = probe_ground(floor, pos.x, pos.y, pos.z) {
        pos.z = z;
    }
}

/// The mesh path with the walker's own height. [assumed] A path point keeps its x and y; its z is the
/// mesh floor, which is not a ground height. The walker keeps its own z, which
/// `settle_on_ground` sets from the collision, so the mesh z never reaches the walker's position.
struct OwnHeight(MeshPath);

impl PathSource for OwnHeight {
    fn next_point(&mut self, from: [f32; 3], to: [f32; 3]) -> Option<[f32; 3]> {
        self.0.next_point(from, to).map(|p| [p[0], p[1], from[2]])
    }
    fn new_frame(&mut self) {
        self.0.new_frame();
    }
}

/// Shared collision space, but auto-target searches consider the opposing character.
struct FightWorld<'a> {
    space: &'a Arena,
    foes: &'a [Target],
}
impl sim::World for FightWorld<'_> {
    fn floor(&self, x: f32, y: f32, z: f32) -> f32 {
        self.space.floor(x, y, z)
    }
    fn push_out(&self, pos: &mut Vec3, radius: f32, height: f32) -> bool {
        self.space.push_out(pos, radius, height)
    }
    fn follow_floor(&self, from: Vec3, to: Vec2) -> Option<f32> {
        self.space.follow_floor(from, to)
    }
    // [fix] The car form's ground: the wheel rays and the chassis sweep. Without these two the enemy's
    // car had no ground at all (the trait's defaults are None) and fell; the player's car gets them from
    // `Arena` directly (sim.rs `impl World for Arena`). Found from work/ai_check4_1.log (Shanghai mission).
    // [assumed] The enemy's car takes the same ground as the player's (Phase 3 enemy AI).
    // [stand-in] `separate_targets` is not forwarded here (the car's push-out from target cylinders). OPEN:
    // the game's push-out routine is not read; settles: the car's step that takes separate_targets.
    fn wheel_ray(&self, from: Vec3, to: Vec3) -> Option<crate::vehicle::GroundHit> {
        self.space.wheel_ray(from, to)
    }
    fn vehicle_sweep(&self, centre: Vec3, turn: glam::Quat, half: Vec3, travel: Vec3) -> Option<crate::vehicle::Contact> {
        self.space.vehicle_sweep(centre, turn, half, travel)
    }
    fn move_chassis(&self, centre: Vec3, turn: glam::Quat, next: glam::Quat, half: Vec3, velocity: &mut Vec3, angular: Vec3, dt: f32)
        -> Option<(Vec3, Vec<crate::vehicle::Contact>)> {
        sim::World::move_chassis(self.space, centre, turn, next, half, velocity, angular, dt)
    }
    fn chassis_pass(&self) -> bool {
        sim::World::chassis_pass(self.space)
    }
    // [game] the enemies' bodies take the same world step as the player's, with an AI character's masks
    // and ground probe (0x09; `LevelWorld::step_avatar`). Before this they kept the stand-in push.
    fn step_avatar(&self, body: &mut crate::physics_avatar::AvatarBody, req: &crate::physics_avatar::AvatarRequest)
        -> Option<crate::physics_avatar::FeetStep> {
        sim::World::step_avatar(self.space, body, &crate::physics_avatar::AvatarRequest { ai: true, ..*req })
    }
    fn bodies(&self) -> &[crate::physics_avatar::BodySnapshot] {
        sim::World::bodies(self.space)
    }
    fn step_chassis(&self, body: &mut crate::physics_avatar::AvatarBody, inp: &crate::physics_avatar::ChassisInput, _ai: bool)
        -> Option<(crate::physics_avatar::ChassisOutput, Vec<crate::vehicle::Contact>)> {
        sim::World::step_chassis(self.space, body, inp, true)
    }
    fn ai_bodies(&self) -> bool {
        true
    }
    fn targets(&self) -> &[Target] {
        self.foes
    }
    fn ledge(&self, from: Vec3, toward: Vec2, reach: f32) -> Option<crate::climb::Ledge> {
        self.space.ledge(from, toward, reach)
    }
    fn front_contact(&self, pos: Vec3, facing: Vec2, radius: f32, height: f32) -> bool {
        self.space.front_contact(pos, facing, radius, height)
    }
    fn drop_onto(&self, from: Vec3, to_z: f32, radius: f32) -> Option<Vec3> {
        sim::World::drop_onto(self.space, from, to_z, radius)
    }
    fn surface_contacts(&self, body: &crate::melee::Body) -> Vec<crate::ssd::Contact> {
        sim::World::surface_contacts(self.space, body)
    }
}

pub struct Scout {
    pub character: character::CharacterData,
    pub tuning: Tuning,
    pub gun_index: usize,
    pub gun: WeaponTuning,
    pub brain: Brain,
    pub rig: crate::pose::Rig,
    /// The game's own brain for this character (phase 3). None when it did not load; see `ai_error`.
    pub ai: Option<AiSetup>,
    /// Why `ai` is None. Not fatal: the stand-in brain still runs.
    pub ai_error: Option<String>,
    /// The character's tacticsData block (`ai_tactics::load`) [data]. None when it does not load:
    /// the tactics point is then never picked.
    pub tactics: Option<ai_tactics::TacticsData>,
}

/// What an enemy needs to run the game's own brain (`ai_bind::build`): the loaded AI data, the
/// character's `brainList` as (priority, brain name hash), its AI attributes, and the names the
/// report prints. [data] all of it comes from `bnxglobal.str` (chunk 16 and the attribute block).
pub struct AiSetup {
    pub data: Arc<AiData>,
    pub brains: Vec<(f32, u32)>,
    pub attr: AiAttr,
    names: Arc<Vec<BrainNames>>,
}

/// The names of one brain, for the report only. Goals and entries are listed in the order the
/// controller holds them. [assumed] the controller sorts them stably by descending static
/// priority (`ai_brain::Brain::new`, `Goal::new`), so the same sort gives the same order here.
pub struct BrainNames {
    pub name: String,
    pub goals: Vec<GoalNames>,
}

/// One goal's name and the names of its behaviour entries, in the controller's order.
pub struct GoalNames {
    pub name: String,
    pub behaviours: Vec<String>,
}

fn brain_names(data: &AiData, brains: &[(f32, u32)]) -> Vec<BrainNames> {
    brains
        .iter()
        .filter_map(|&(_, hash)| data.brain_by_hash(hash))
        .map(|root| {
            let mut goals: Vec<&ai_data::GoalRef> = root.goals.iter().collect();
            goals.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap_or(Ordering::Equal));
            BrainNames {
                name: root.name.clone(),
                goals: goals
                    .into_iter()
                    .map(|goal| {
                        let mut behaviours: Vec<&ai_data::BehaviourRef> = goal.goal.behaviours.iter().collect();
                        behaviours.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap_or(Ordering::Equal));
                        GoalNames {
                            name: goal.goal.name.clone(),
                            behaviours: behaviours.into_iter().map(|b| b.behaviour.base().name.clone()).collect(),
                        }
                    })
                    .collect(),
            }
        })
        .collect()
}

/// Loads the game's brain for the character whose `characterAttributes<X>` block is `block`.
/// Fails when the data does not load, the block has no brain list, or a listed brain is not in the
/// data (`ai_bind::build` checks that).
fn load_ai(dir: &Path, block: Node) -> Result<AiSetup, String> {
    let data = ai_data::load(dir)?;
    let mut missing = Vec::new();
    let brains: Vec<(f32, u32)> = ai_data::brains_of(block, &mut missing)
        .iter()
        .map(|b| (b.priority, b.brain))
        .collect();
    if brains.is_empty() {
        return Err("the character has no brainList".to_owned());
    }
    let make: &dyn Fn() -> Box<dyn PathSource> = &|| Box::new(StraightLine);
    ai_bind::build(&data, &brains, make)?;
    let attr = ai_bind::attributes(block);
    let names = Arc::new(brain_names(&data, &brains));
    Ok(AiSetup { data: Arc::new(data), brains, attr, names })
}
#[derive(Clone, Debug)]
pub struct Brain {
    pub sight: f32,
    pub melee_distance: f32,
    pub attack_height: f32,
    pub attack_delay: f32,
    pub ranged_distance: f32,
    pub backpedal_distance: f32,
}
impl Scout {
    pub fn gun_clips(&self) -> crate::gun::Clips<'_> {
        crate::gun::Clips {
            arm: self
                .character
                .animations
                .sets
                .get("WeaponPartialSet")
                .map_or(&[], Vec::as_slice),
            entries: self
                .character
                .animations
                .incoming
                .get("WeaponPartialSet")
                .map_or(&[], Vec::as_slice),
            object: self
                .character
                .weapon_animations
                .get(self.gun_index)
                .map_or(&[], Vec::as_slice),
        }
    }
    /// The Decepticon scout, the enemy of the first missions.
    pub fn load(dir: &Path) -> Result<Self, String> {
        Self::load_as(dir, "dcomscout", "DComScout")
    }

    /// Any character as the enemy: its pack and the `X` of its `characterAttributes<X>`.
    /// [stand-in] the brain is still this module's few numbers, whoever it is.
    pub fn load_as(dir: &Path, pack_name: &str, attributes: &str) -> Result<Self, String> {
        let character = character::load(dir, pack_name)?;
        let mut tuning = tuning::load(dir, attributes)?;
        tuning.locomotion =
            tuning::Locomotion::from_animations(&character.animations, &mut tuning.missing);
        tuning.actions = tuning::Actions::from_animations(
            &character.animations,
            &character.robot,
            &mut tuning.missing,
        )
        .with_vehicle(character.vehicle.as_ref())
        // An enemy robot's blades strike as the player's do (007b2e10). [game]
        .with_melee(&character.melee_weapons);
        tuning.weapon_set = tuning::RuleSet::from_animations(
            &character.animations,
            "WeaponSet",
            &mut tuning.missing,
        );
        tuning.rule_sets = tuning::RuleSet::all(&character.animations);
        let guns = weapons::load(dir, &character.weapons)?;
        let gun_index = guns
            .iter()
            .position(|g| !g.vehicle_mode)
            .ok_or_else(|| format!("{pack_name} has no robot gun"))?;
        let pack = Pack::open(&dir.join("bnxglobal.str"))?;
        let files: Vec<_> = pack
            .of_type(pack::DATA)
            .filter_map(|c| DataFile::parse(pack.data(c).to_vec()).ok())
            .collect();
        let attrs = files
            .iter()
            .find_map(|f| f.root().get(&format!("characterAttributes{attributes}")))
            .ok_or_else(|| format!("characterAttributes{attributes} missing"))?;
        let number = |name| {
            attrs
                .get(name)
                .and_then(Node::float)
                .ok_or_else(|| format!("characterAttributes{attributes} has no {name}"))
        };
        let brain = Brain {
            sight: number("sightRange")?,
            melee_distance: number("attackMeleeDistance")?,
            attack_height: number("attackHeight")?,
            attack_delay: number("attackDelay")?,
            ranged_distance: number("attackRangeDistance")?,
            backpedal_distance: number("backPedalDistance")?,
        };
        let rig = crate::pose::Rig::new(&character.robot);
        // [game] the brain comes from the same attribute block. Failure is not fatal (ai_error).
        let (ai, ai_error) = match load_ai(dir, attrs) {
            Ok(setup) => (Some(setup), None),
            Err(e) => (None, Some(e)),
        };
        let tactics = ai_tactics::load(dir, attributes).ok();
        Ok(Self {
            character,
            tuning,
            gun_index,
            gun: guns[gun_index].clone(),
            brain,
            rig,
            ai,
            ai_error,
            tactics,
        })
    }
}

/// Controller values that persist between steps (`controller+0xcc` attack clock, `+0xe0` dodge clock,
/// `+0xdc` weapons disabled). A fresh `Ctx` each step would lose them. [assumed] the clocks run
/// in the tick: nothing that the notes read advances them, so `Enemy::brain_input` adds dt.
#[derive(Clone, Copy, Debug, Default)]
struct BrainClocks {
    attack: f32,
    dodge: f32,
    weapons_disabled: bool,
}

/// No `Clone` or `Debug`: the game's brain (`Controller`) has neither.
pub struct Enemy {
    pub state: State,
    pub gun: Weapon,
    pub weapon_pose: crate::gun::Gun,
    /// Where the character was placed. [game] NOT the home point: the game has a home point only
    /// when the spawn data gives one (controller+0x44), see `home`.
    pub spawn: Vec3,
    /// The home point (`ctrl+0x38..0x40`, valid when `ctrl+0x44` is set) [game 0081e190, 00830d22].
    /// None unless the spawn data gives one: the scout's spawn records have useSpawnPointAsHomePoint
    /// false and no homePoints [data], so the scout has none. With None, fc1c5893 has no value and
    /// the home goal does not score.
    pub home: Option<[f32; 3]>,
    /// The path agent of this character (`path_agent`), for the nav mesh paths and the sight test.
    agent: Agent,
    /// The level's nav mesh, when `set_nav` gave one. None: straight lines and the stand-in sight test.
    nav: Option<Arc<NavMesh>>,
    /// The tactics record (controller+0x690) [game 00834520]. Only updated with a nav mesh.
    tactics: TacticsRecord,
    /// The tactics type the Tactics behaviour wrote last step (controller+0x694), 0 when none.
    tactics_type: u32,
    melee_wait: f32,
    engaged: bool,
    seed: u32,
    /// [stand-in] Run the game's own brain (`Scout::ai`) instead of the stand-in brain. Off by
    /// default; the game sets it with `--ai-brain`.
    pub use_data_brain: bool,
    brain: Option<ai_brain::Controller>,
    brain_names: Option<Arc<Vec<BrainNames>>>,
    /// Set by the integrator each step when a person drives this character (`Driven`): the brain is
    /// then not run. None for every enemy.
    pub driven: Option<Driven>,
    /// The character's group (`character+0x16c`, the key of the GROUP message broadcast) [stand-in]:
    /// its writer is unread, so every enemy is in group 0 unless the integrator sets it.
    pub group: u32,
    /// Names this enemy broadcast to its group (ARG_GROUP) and that `route_group_messages` has not
    /// delivered yet. Routed after each step (the game must call it every step).
    pub group_outbox: Vec<u32>,
    /// The group members other than this enemy, as the integrator sees them this step (set before
    /// each `step`). Feeds the Escort perceptions: `group_members` and `group_targets` of the brain's
    /// Senses. [stand-in] the game fills it with the convoy only (targets::update); other groups
    /// have no members in this port yet. OPEN: the game's member list is built by 007e9c60's group
    /// registry (007e9080), which is read only to the Destructable branch; settles: the rest of 007e9c60
    /// and where the integrator's group members come from.
    pub group_input: Vec<GroupMember>,
    brain_clocks: BrainClocks,
    /// The buttons the brain held last step (the `held` of its `Ctx`, and the base of `buttons_hit`).
    prev_buttons: u64,
    /// Messages the brain sent to the character in the last step. [stand-in] the state machine's
    /// message input is not wired: nothing reads them yet.
    pub last_messages: Vec<(u32, u32)>,
    /// Rounds this enemy has fired so far (every shot `step` returns). [game] the count is the port's own.
    pub shots: u32,
    last_requests: Vec<Request>,
    last_stick: [f32; 2],
    last_buttons: u64,
    /// Requests with no control button (`request_bit` is None), for the report.
    last_unmapped: Vec<Request>,
    /// [test] The brain's facts at the last step, as `brain_diagnosis` writes them (the spawn check).
    #[cfg(test)]
    pub diag: String,
}
/// Another character of a group, as the integrator sees it this step: the group input of the
/// Escort perceptions (group member distance 712ecdc7, sight 0acb9809, group target 843fc6db).
/// The integrator fills `Enemy::group_input` with the members other than the enemy itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupMember {
    /// The group's name hash (`targetGroupID` / `groupID`; "Escort" is 705cdac2 [data]).
    pub group: u32,
    /// Its position, metres (the integrator's place for it).
    pub pos: Vec3,
    /// Line of sight from the enemy's eye to it, the integrator's test [assumed] (the arena sight).
    pub in_sight: bool,
    /// The member is alive. A dead member is no group target [assumed]. OPEN: 007e9c60 skips some
    /// members by a health test (the Destructable branch, +0x68 > 0); the exact rule for characters is
    /// not read. Settles: the rest of 007e9c60 (after its 0x57325142 branch).
    pub alive: bool,
}

/// The group range: a group member farther than this, flat, is not found [game 0083193f, the
/// 5000 m of ai_brain::Senses::group_members; the search's point is taken as the enemy's own
/// position [assumed]].
const GROUP_RANGE: f32 = 5000.0;

/// The nearest live member of each group in `members`, as (group, flat distance, in sight) from
/// `from`. [assumed] one entry per group: the nearest member is the group target when several are
/// alive (the game's choice is unread). Members beyond `GROUP_RANGE` are not found. OPEN: the 5000 m
/// range is the game's (007e9c60's search radius, [game]); the nearest choice and the flat distance
/// are not read to their comparison. Settles: 007e9c60 after the member loop.
fn nearest_per_group(members: &[GroupMember], from: Vec3) -> Vec<(u32, f32, bool)> {
    nearest_members(members, from).into_iter().map(|(g, d, sight, _)| (g, d, sight)).collect()
}

/// As `nearest_per_group`, with the nearest member's place as the fourth value: (group, flat
/// distance, in sight, place). The place is the group target's point (`Senses::group_target_points`).
fn nearest_members(members: &[GroupMember], from: Vec3) -> Vec<(u32, f32, bool, Vec3)> {
    let mut out: Vec<(u32, f32, bool, Vec3)> = Vec::new();
    for m in members.iter().filter(|m| m.alive) {
        let d = Vec2::new(m.pos.x - from.x, m.pos.y - from.y).length();
        if d > GROUP_RANGE {
            continue;
        }
        match out.iter_mut().find(|e| e.0 == m.group) {
            Some(e) => {
                if d < e.1 {
                    *e = (m.group, d, m.in_sight, m.pos);
                }
            }
            None => out.push((m.group, d, m.in_sight, m.pos)),
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
pub struct Shot {
    pub from: Vec3,
    pub to: Vec3,
    pub target: Option<u32>,
    pub packet: Packet,
}

/// A person's input in place of the brain (the game's second-robot debug view, F3): what is pressed
/// this step and the point its rounds go to. The trigger always counts as the ranged attack.
/// [port] a debug aid with no game counterpart.
#[derive(Clone, Copy, Debug, Default)]
pub struct Driven {
    pub input: Input,
    pub aim: Vec3,
}
impl Enemy {
    /// An enemy placed at `spawn`, with no home point and no nav mesh (`set_nav` gives one).
    pub fn new(id: u32, spawn: Vec3, scout: &Scout) -> Self {
        let mut state = State::new(&scout.tuning);
        state.pos = spawn;
        state.actor = Actor {
            id,
            team: scout.tuning.damage.team,
            player: false,
            character: crc32(b"Swindle"),
            ..Default::default()
        };
        // [game] the character's brain is built from the data and started (`ai_bind::build`).
        // [stand-in] without a nav mesh its paths are straight lines (StraightLine).
        let make: &dyn Fn() -> Box<dyn PathSource> = &|| Box::new(StraightLine);
        let brain = build_brain(scout, make);
        Self {
            state,
            gun: Weapon::default(),
            weapon_pose: Default::default(),
            spawn,
            home: None,
            agent: path_agent(scout),
            nav: None,
            tactics: TacticsRecord::new(),
            tactics_type: 0,
            melee_wait: 0.0,
            engaged: false,
            seed: id.wrapping_mul(0x9e37_79b9).max(1),
            use_data_brain: false,
            driven: None,
            brain,
            brain_names: scout.ai.as_ref().map(|ai| ai.names.clone()),
            group: 0,
            group_outbox: Vec::new(),
            group_input: Vec::new(),
            brain_clocks: BrainClocks::default(),
            prev_buttons: 0,
            last_messages: Vec::new(),
            shots: 0,
            last_requests: Vec::new(),
            last_stick: [0.0, 0.0],
            last_buttons: 0,
            last_unmapped: Vec::new(),
            #[cfg(test)]
            diag: String::new(),
        }
    }
    /// Gives this enemy the level's nav mesh (phase 3, integration). Its brain is built again with
    /// `MeshPath` as the path source (the agent of `path_agent`), and its sight test and tactics
    /// point use the mesh. Call it right after `new` (before a step): the brain's state is rebuilt.
    pub fn set_nav(&mut self, mesh: Arc<NavMesh>, scout: &Scout) {
        let agent = self.agent;
        let path_mesh = Arc::clone(&mesh);
        let make: &dyn Fn() -> Box<dyn PathSource> = &|| Box::new(OwnHeight(MeshPath::new(Arc::clone(&path_mesh), agent)));
        if let Some(brain) = build_brain(scout, make) {
            self.brain = Some(brain);
        }
        self.nav = Some(mesh);
    }
    pub fn target(&self, t: &Tuning) -> Target {
        Target {
            id: self.state.actor.id,
            pos: self.state.pos,
            radius: t.robot_radius,
            height: t.robot_height,
            character: true,
            large: t.damage.large,
            surface: surface::UPPER_BODY,
            dashing: self.state.damage_timers.dash > 0.0,
            health: self.state.health,
            head: None,
        }
    }
    pub fn receive(&mut self, packet: Packet, scout: &Scout) -> crate::damage::Resolved {
        let result = self.state.receive_damage(packet, &scout.tuning);
        if result.accepted {
            self.engaged = true;
            for (_, heat) in self.state.heat_damage.drain(..) {
                self.gun.add_heat(&scout.gun, heat);
            }
        }
        result
    }
    /// [game] Attack behaviours request the controller buttons (008297e0/00822360).
    /// [data] Scout melee has priority .92 over ranged .6, range7 and height10.
    /// [stand-in] Arena steering/engagement and individual attack-delay bookkeeping;
    /// original goals, tactics-point navigation and shared kung-fu slots are not ported. OPEN: the
    /// game's steering and attack-slot code is not read; settles: the enemy's step and its slot
    /// bookkeeping in the game (the attack behaviours' callers, 008297e0/00822360 onward).
    pub fn step(
        &mut self,
        scout: &Scout,
        player: &State,
        player_t: &Tuning,
        arena: &Arena,
        passive: bool,
        dt: f32,
    ) -> Vec<Shot> {
        self.melee_wait = (self.melee_wait - dt).max(0.0);
        self.state.difficulty = player.difficulty;
        let mut world = arena.clone();
        world.targets.retain(|t| t.id != self.state.actor.id);
        // [game] the player's body meets the enemies' through the level's physics (body pairs)
        world.bodies.retain(|b| b.id != self.state.actor.id && b.id != player.actor.id);
        if let Some(b) = (!player.dead()).then(|| player.body_snapshot(player_t, false)).flatten() {
            world.bodies.push(b);
        }
        if !player.dead() {
            world.targets.push(Target {
                id: player.actor.id,
                pos: player.pos,
                radius: if player.is_vehicle() {
                    player_t.vehicle_radius
                } else {
                    player_t.robot_radius
                },
                height: if player.is_vehicle() {
                    player_t.vehicle_height
                } else {
                    player_t.robot_height
                },
                character: true,
                large: player_t.damage.large,
                surface: surface::UPPER_BODY,
                dashing: player.damage_timers.dash > 0.0,
                health: player.health,
                head: None,
            });
        }
        let offset = player.pos - self.state.pos;
        let distance = offset.truncate().length();
        let direction = offset.truncate().normalize_or(Vec2::Y);
        let eye = self.state.pos
            + Vec3::Z * scout.tuning.robot_height * scout.tuning.aiming_height_percentage * 0.01;
        let aim = player.pos
            + Vec3::Z
                * (if player.is_vehicle() {
                    player_t.vehicle_height
                } else {
                    player_t.robot_height
                })
                * player_t.aiming_height_percentage
                * 0.01;
        let visible = arena.sight(eye, aim).is_none();
        let aim = self.driven.map_or(aim, |d| d.aim);
        let (input, ranged) = if let Some(driven) = self.driven {
            (driven.input, true)
        } else if self.use_data_brain && self.brain.is_some() {
            self.brain_input(scout, player, passive, visible, eye, distance, direction, aim.z - player.pos.z, dt)
        } else {
            self.stand_in_input(scout, player, passive, visible, offset, distance, direction)
        };
        let foes: Vec<_> = world
            .targets
            .iter()
            .filter(|t| t.id == player.actor.id)
            .copied()
            .collect();
        sim::step(
            &mut self.state,
            &input,
            &scout.tuning,
            &FightWorld {
                space: &world,
                foes: &foes,
            },
            dt,
        );
        // [assumed] With a nav mesh (a mission's collision), a grounded enemy's feet take the collision
        // ground under them (settle_on_ground). Without one, the sim's own floor stands as before.
        // On a level whose physics now holds the enemies up (`World::step_avatar`), the settle is not run.
        if self.nav.is_some() && arena.level.is_none() {
            settle_on_ground(&mut self.state.pos, |x: f32, y: f32, z: f32| arena.floor(x, y, z));
        }
        let clips = scout.gun_clips();
        let def = &scout.character.weapons[scout.gun_index];
        if self.state.state_class(&scout.tuning) == Some("StateWeapon") && !self.state.dead() {
            self.weapon_pose.take_aim(clips, def, false);
        } else {
            self.weapon_pose.put_away(clips, def);
        }
        let asked = self.state.firing && ranged && !self.state.dead();
        let trigger = asked && !self.weapon_pose.holds_trigger(clips, asked);
        self.weapon_pose.step(clips, def.name, dt);
        let fired = self.gun.step_status(
            &scout.gun,
            trigger,
            self.state.damage_timers.slowed > 0.0,
            &scout.tuning.damage,
            dt,
        );
        let count = match fired {
            Fired::Rounds(n) | Fired::RoundsAndOverheated(n) => n,
            _ => 0,
        };
        let mut shots = Vec::new();
        let muzzle = self.muzzle(scout);
        for _ in 0..count {
            self.weapon_pose.fired(clips, def.name);
            let forward = (aim - muzzle).normalize_or(Vec3::Y);
            let right = forward.cross(Vec3::Z).normalize_or(Vec3::X);
            let up = right.cross(forward);
            let angle = (self.random() * 2.0 - 1.0) * std::f32::consts::PI;
            let radius = (self.random() * 2.0 - 1.0) * scout.gun.general_noise_radius;
            // [game] 007aa210 non-player noise: fixed-radius disc across muzzle-to-aim.
            let at = aim + (right * angle.cos() + up * angle.sin()) * radius;
            let (from, to) = weapons::bullet_line(muzzle, at, &scout.gun);
            let mut landed = arena.sight(from, to).map(|h| (h.point, None));
            for target in &world.targets {
                if let Some(k) = intersect_cylinder(from, to, target) {
                    let point = from + (to - from) * k;
                    if landed.is_none_or(|(p, _)| {
                        p.distance_squared(from) > point.distance_squared(from)
                    }) {
                        landed = Some((point, Some(target.id)));
                    }
                }
            }
            let end = landed.map_or(to, |(p, _)| p);
            let amount = weapons::bullet_damage(
                &scout.gun,
                1.0,
                if player.is_vehicle() {
                    Part::VehicleBody
                } else {
                    Part::UpperBody
                },
                end.distance(muzzle),
                false,
            );
            let packet = Packet {
                source: Some(self.state.actor),
                ..Packet::bullet(&scout.gun, amount, (to - from).normalize_or(Vec3::Y))
            };
            shots.push(Shot {
                from: muzzle,
                to: end,
                target: landed.and_then(|(_, id)| id),
                packet,
            });
        }
        self.shots += shots.len() as u32;
        shots
    }
    /// [stand-in] The arena's own brain: engagement, melee and ranged decisions, and steering. This
    /// is the body `step` had before the game's brain was wired, moved here unchanged. Returns the
    /// input and whether the ranged attack is asked.
    fn stand_in_input(&mut self, scout: &Scout, player: &State, passive: bool, visible: bool, offset: Vec3, distance: f32, direction: Vec2) -> (Input, bool) {
        self.engaged |= distance <= scout.brain.sight && visible;
        let active = !passive && self.engaged && !player.dead() && !self.state.dead();
        let melee = active
            && visible
            && !player.is_vehicle()
            && player.ground_flag()
            && distance <= scout.brain.melee_distance
            && offset.z.abs() <= scout.brain.attack_height;
        let start_melee = melee && self.melee_wait <= 0.0;
        if start_melee {
            self.melee_wait = scout.brain.attack_delay;
        }
        let ranged = active && !melee && visible && distance <= scout.brain.ranged_distance;
        let movement = if active && !melee && !ranged {
            direction
        } else if ranged && distance < scout.brain.backpedal_distance {
            -direction
        } else {
            Vec2::ZERO
        };
        if active && !self.state.dead() && self.state.mode != sim::Mode::Action {
            self.state.yaw = (-direction.x).atan2(direction.y);
        }
        let input = Input {
            stick: movement,
            aim: ranged.then_some(direction),
            buttons_down: if ranged {
                1 << crate::control::button::WEAPON_FIRE
            } else if start_melee {
                1 << crate::control::button::ATTACK_FAST
            } else {
                0
            },
            buttons_hit: if start_melee {
                1 << crate::control::button::ATTACK_FAST
            } else {
                0
            },
            ..Input::default()
        };
        (input, ranged)
    }

    /// [stand-in] The game's own brain drives this enemy (`Scout::ai`, phase 3 wiring). The brain
    /// is ticked once; its requests become the input. Stand-ins, each tagged where it is set: the
    /// foe is the player (the game's foe search 00823220 over all characters is not used), the
    /// sight test is the stand-in's, and the flags with no source in this state stay false.
    /// Returns the input and whether the brain asked for the fire button.
    /// `aim_height` is the player's aiming height above its origin, metres (the `aim` of `step`).
    fn brain_input(&mut self, scout: &Scout, player: &State, passive: bool, visible: bool, eye: Vec3, distance: f32, direction: Vec2, aim_height: f32, dt: f32) -> (Input, bool) {
        let Some(mut controller) = self.brain.take() else {
            return (Input::default(), false);
        };
        let attr = scout.ai.as_ref().map(|ai| ai.attr).unwrap_or_default();
        // [stand-in] the player is the only foe, when alive and on another team. OPEN: the game's foe is
        // the nearest of its target search (00823220 -> 00822c10, type 0x81b1ccb within 5000 m).
        let foe = (!player.dead() && player.actor.team != self.state.actor.team).then_some(ai_brain::Target {
            pos: player.pos.to_array(),
            distance,
            team: player.actor.team,
        });
        let in_vehicle = self.state.is_vehicle();
        // The form values are the game's robotForm numbers: 1 on foot, 2 weapon, 3 the car [game
        // 0071ab70, as control.rs records it]. The port's mapping from its own modes to them is the
        // port's [assumed].
        let form = if in_vehicle {
            3
        } else if self.state.mode == sim::Mode::Strafe {
            2
        } else {
            1
        };
        let max_health = scout.tuning.base_health.max(1.0);
        let aim = player.pos + Vec3::Z * aim_height;
        // The line of sight to the player [game 00827ce0 corridor test with the sight agent] when
        // there is a nav mesh; else the stand-in's sight test (`visible`, from `step`).
        let visible = match &self.nav {
            Some(mesh) => navpath::straight(
                mesh,
                eye,
                aim,
                &Agent::new(SIGHT_RADIUS_FACTOR * scout.tuning.robot_radius, SIGHT_HEIGHT, 0),
            ),
            None => visible,
        };
        // The tactics record (`ai_tactics::update`) [assumed: the driver runs each step with the player
        // as the target]. The tactics type is the one the Tactics behaviour wrote last step, else the
        // character's own tacticsType [assumed]. Without a nav mesh (the test arena) the candidate
        // search is ai_tactics' stand-off stand-in [stand-in]: before 2026-10-20 the FOE destination
        // was the player's own place, which a scout never reaches, so its steer pressed WeaponMode
        // every step and its melee presses fell in StateWeapon.
        if let Some(data) = scout.tactics {
            let mesh = self.nav.clone();
            let tactics_type = if self.tactics_type != 0 { self.tactics_type } else { data.tactics_type };
            let data = ai_tactics::TacticsData { tactics_type, ..data };
            let me = ai_tactics::Me {
                position: self.state.pos,
                // [game 0071a2e0] a walker's camera target is its root origin (C+0xc+0x34..0x3c); the
                // flyer branch (0071a460) is not used for a scout, which does not fly here.
                camera: self.state.pos,
                radius: scout.tuning.robot_radius,
                flying: false,
                flag_3d: false,
                flag_a6: false,
                health: self.state.health,
                health_max: max_health,
                los_height: SIGHT_HEIGHT,
                los_flags: 0,
                home: None,
            };
            // [stand-in] the other attackers are not fed (the group list is not searched here). OPEN.
            let foe_geom = foe.map(|_| ai_tactics::Foe {
                position: player.pos,
                // [game 0071a2e0] the player's camera target is its root origin, as for the scout above.
                camera: player.pos,
            });
            ai_tactics::update(&mut self.tactics, &data, &me, foe_geom.as_ref(), &[], mesh.as_deref(), dt);
        }
        // The tactics point: the record's when it is valid and real. A search that found nothing
        // leaves FLT_MAX with VALID set: that is no point (the scout stays put).
        // [stand-in] a character with no tacticsData: the foe's own point is the FOE destination, as
        // the stand-in brain had it.
        let record = self.tactics;
        let usable = record.usable_point();
        // [assumed] The point's z is the mesh floor, not a height the enemy stands at.
        // It takes the enemy's own z, so the arrival tests of GoToTarget (3D in ai_move.rs) are flat.
        let tactics_point = if scout.tactics.is_some() {
            usable.map(|p| [p.x, p.y, self.state.pos.z])
        } else {
            foe.map(|t| t.pos)
        };
        let mut ctx = Ctx::new();
        ctx.me = SelfState {
            pos: self.state.pos.to_array(),
            aim_origin: eye.to_array(),
            facing: self.state.yaw,
            health: self.state.health,
            form,
            state: self.state.state_class(&scout.tuning).map_or(0, |c| crate::formats::hash::crc32(c.as_bytes())),
            in_vehicle,
            weapon_index: scout.gun_index as i32, // [stand-in] the weapon in hand is the scout's gun
            team: scout.tuning.damage.team,
            special_block: 0.0, // [stand-in] the special object's float is not read here. OPEN.
        };
        let foe_here = foe.is_some();
        let in_sight = foe_here && visible; // the nav corridor test with a mesh, else the stand-in's
        // Distance to the home point, horizontal [assumed]. None without a home point [data]. OPEN: the game's
        // home distance is the 00725400 distance on the c9e0a56c path (its plane is not read).
        let home_distance = self.home.map(|h| Vec2::new(h[0] - self.state.pos.x, h[1] - self.state.pos.y).length());
        // [assumed] flat (x, y): the point's z is the mesh floor, not the enemy's height.
        let flat_to_point = usable.map(|p| Vec2::new(p.x - self.state.pos.x, p.y - self.state.pos.y).length());
        let at_tactics_point = flat_to_point.is_some_and(|d| d <= AT_TACTICS_POINT);
        ctx.senses = Senses {
            foe_distance: foe.map(|t| t.distance),
            foe_visible: in_sight,
            home_distance,
            // [assumed] maximum health is the tuning's base health. OPEN: the game's max-health field is not read.
            health_percent: (100.0 * self.state.health / max_health).clamp(0.0, 100.0),
            in_vehicle_motion: in_vehicle,
            // Values 5d06c0ee, 188e1abe: the grounded byte of the movement step (sim ground_flag).
            on_ground: self.state.ground_flag(),
            // [assumed] the port's StateFall class stands for the fall form (0081e820). OPEN: the game's
            // test compares the form object's class id 0xd7068c31 (0081e820), not a name. crc32("StateFall")
            // is 0x134aa9e2, so this class is not the game's test value. bumblebee-mechanics.md lists
            // 0xd7068c31 as ClimbMode's class id. Settles: the form that 0xd7068c31 names (the class list in
            // 00851a40), and whether the fall form it means is the climb form.
            falling_form: self.state.has_state_class(&scout.tuning, "StateFall"),
            // [stand-in] the hit flag (char+0x14c bit 5) is not set by the damage path yet: the
            // stun timer is the nearest existing hit reaction. Values fbd14a89, 5ada9b7c. [game fbd14a89
            // reads (char+0x14c)>>5; the damage path is not wired to set that bit. OPEN.]
            was_hit: self.state.damage_timers.stunned > 0.0,
            // [assumed] the hit window is open while the stun timer runs (the same window as was_hit): the
            // reaction clock test (state+0xc - char+0x1f0 <= 0) holds inside it. Its window length is the
            // existing hit reaction duration (damage_timers.stunned), not a read value [unknown]. OPEN.
            hit_reaction_over: self.state.damage_timers.stunned > 0.0,
            has_path: false,          // [stand-in] the controller's path record is not ported here. OPEN.
            // [stand-in] the cone is not the sight range: 00831c00 [game] tests the foe's camera point
            // against a cone of radius C+0x140+0x44 (plus the STEP reach term) and half-angle C+0x140+0x48
            // (radians), about the heading STEP+0x25c. The port's fields for +0x44 and +0x48 are not
            // identified, so the sight range stands. Value 6b3394c7. OPEN.
            foe_in_sight_cone: in_sight && distance <= scout.brain.sight,
            // [stand-in] 7b35ef2b [game 00831a5c] is the foe's clip attack type == 0x9ebf35c5 (the flight
            // slam), read through 00718920 on the foe's clip. The port's foe is the player, whose clip is
            // not read here. OPEN.
            foe_attack_fast: false,
            // [stand-in] 218a1421 [game 008314dd] for a player foe: its weapon handle test (O+0x23c vs C+0xc,
            // 00557150 UNREAD) and the weapon in hand (00831c60: vtable +0x8c and +0x94). The port takes the
            // player's firing flag instead. OPEN.
            foe_attack_weapon: foe_here && player.firing,
            // [stand-in] 1529732e (compare site 00831118, unread) reads a float of the foe through 00823220
            // (field unidentified). The player's stun timer stands for it. OPEN.
            foe_stunned: foe_here && player.damage_timers.stunned > 0.0,
            // [stand-in] b85859da [game 00830a76] is the foe's ambush-mode test, 00824430 [game]: true when the
            // foe's behaviour object at +0x10 has type 0x16194d9c (behaviorAmbushMode). The player has no
            // AI behaviour here, so false stands. OPEN: confirm the player's +0x10 is null.
            foe_ambush: false,
            flying_form: false,     // [stand-in] the flying forms are not wired to this enemy (2fe0758f). OPEN.
            // [stand-in] 188e1abe [game 00831306]: grounded gives 0.0 at once; else 00715990 gives the
            // ground value (-FLT_MAX with no world, else the nav walk 006f7fa0 whose output is lost in the
            // decompile). The port's flag stays false. OPEN.
            near_ground: false,
            // [stand-in] 1d489c55 [game 008313d2]: the corridor test 00827ce0 is with a nav mesh, else the
            // stand-in's sight test (`visible`). See `visible` above. OPEN.
            own_line_of_sight: in_sight,
            // [assumed] 100 times the weapon's heat fraction, the gun in hand (26e128c3 [game 008315b7] is
            // 100 x 00828250(C) with slot 0; 00828250 UNREAD, so the unit is assumed). OPEN.
            weapon_overheat_percent: Some(100.0 * self.gun.heat_fraction(&scout.gun)),
            // [stand-in] no home region objects are loaded: the home point is the only home. The game's
            // gate 00824650 gives no value without a region, which is what the port's false does (c9e0a56c).
            // OPEN for loaded regions: 00824700 [game] is 1 when 00824650 is false, else the box test
            // 00824450 (UNREAD).
            has_home_region: false,
            inside_home_region: false, // [stand-in] the home box (00824700) is not ported. OPEN.
            home_point_distance: home_distance,
            // The tactics record's facts (a62df181, c9e0a56c). Without a nav mesh the record holds the
            // stand-off stand-in's point; a character without tacticsData keeps the unset values.
            tactics_point_valid: usable.is_some(),
            tactics_goal_named: usable.is_some() && record.goal == ai_tactics::GOAL_FOE,
            tactics_at_point: at_tactics_point,
            tactics_point_unset: usable.is_none(),
            // [assumed] flat distance to the point. The game's 00830950 is 3D (third read), but the point's z
            // is the mesh floor here, so a 3D distance would mix mesh z with world z.
            tactics_point_distance: flat_to_point,
            // [assumed] the group point is the enemy's own place and the nearest live member is the
            // one fed (see nearest_per_group). The member list is the integrator's (`group_input`).
            group_members: nearest_per_group(&self.group_input, self.state.pos),
            group_targets: nearest_per_group(&self.group_input, self.state.pos),
            // [assumed] the group target's place is its nearest live member's place (the same entry as above). OPEN:
            // the game's 00831b20 point is not read (the point 00725400 measures from is unknown).
            group_target_points: nearest_members(&self.group_input, self.state.pos)
                .into_iter()
                .map(|(g, _, _, p)| (g, p.to_array()))
                .collect(),
            friend_distance: None,     // [stand-in] no friend search (22d75963: 00822c10 UNREAD). OPEN.
            turret_distance: None,     // [stand-in] no turret search is fed here (Targeting TURRET). OPEN.
            // [stand-in] the writer of character+0x26c is NOT FOUND (ai-targeting-idle.md C.4).
            interact_handle: false,
            ..Senses::default() // team_object_distance: unfed.
        };
        ctx.target = foe;
        ctx.attr = attr;
        ctx.home = self.home;
        ctx.tactics_point = tactics_point;
        ctx.tactics_type = self.tactics_type;
        ctx.held = self.prev_buttons;
        // [assumed] the clocks run in the tick; see BrainClocks. OPEN: the game's clock fields are not read.
        self.brain_clocks.attack += dt;
        self.brain_clocks.dodge += dt;
        ctx.attack_clock = self.brain_clocks.attack;
        ctx.dodge_clock = self.brain_clocks.dodge;
        ctx.weapons_disabled = self.brain_clocks.weapons_disabled;
        ctx.random = self.random();
        // [stand-in] in_behaviour, input_blocked, combo_window, can_enter_dash and dash_done have
        // no source read from this state yet, so they stay at their defaults (false). OPEN.
        ctx.stick = [0.0, 0.0];
        ctx.requests.clear();
        controller.tick(&mut ctx, dt);
        #[cfg(test)]
        {
            self.diag = brain_diagnosis(&controller, &ctx);
        }
        self.group_outbox.extend(controller.take_group_messages());
        self.last_messages = ctx.drain();
        let requests = std::mem::take(&mut ctx.requests);
        let mut buttons = 0u64;
        let mut released = 0u64;
        let mut unmapped = Vec::new();
        let (mut fire, mut melee) = (false, false);
        for request in &requests {
            if let Request::Release(id) = *request {
                released |= button_bit(id).map_or(0, |bit| 1 << bit);
                continue;
            }
            match request_bit(*request) {
                Some(bit) => buttons |= 1 << bit,
                None => unmapped.push(*request),
            }
            if let Request::Button(id) = *request {
                fire |= id == ai_attack::BTN_WEAPON_FIRE;
                melee |= id == ai_attack::BTN_ATTACK_FAST || id == ai_attack::BTN_ATTACK_SPECIAL;
            }
        }
        buttons &= !released;
        let mut stick = if passive { [0.0, 0.0] } else { ctx.stick };
        // Guard: a non-finite stick (a NaN or infinite destination reached the steer) is dropped, so the
        // scout stands still for this step. [assumed] the guard is a port safety net, not a game rule.
        if !stick.iter().all(|c| c.is_finite()) {
            eprintln!("[debug] scout {}: non-finite stick {stick:?} dropped (requests {:?}, tactics point {:?})", self.state.actor.id, requests, ctx.tactics_point);
            stick = [0.0, 0.0];
        }
        let (buttons, fire) = if passive { (0, false) } else { (buttons, fire) };
        // Face (Idle's track, 00823640 [game]): turn the body toward the point at a limited rate. The
        // rate is [assumed] (AI_FACE_RATE, OPEN). Mode::Action blocks the turn, as the stand-in did.
        if let Some(point) = ctx.face
            && !self.state.dead() && self.state.mode != sim::Mode::Action {
            let to = Vec2::new(point[0] - self.state.pos.x, point[1] - self.state.pos.y);
            if to.length_squared() > 0.0 {
                let want = (-to.x).atan2(to.y);
                let turn = (want - self.state.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                let step = AI_FACE_RATE * dt;
                self.state.yaw += turn.clamp(-step, step);
            }
        }
        // The aim of a fire or melee request: the AI aim point with its spread (ai_aim_point). Drawn
        // only when the brain asks to fire or melee, so the random sequence is untouched otherwise.
        let aim_dir = match foe {
            Some(t) if fire || melee => {
                let origin = self.muzzle(scout);
                let draws = [self.random() * 2.0 - 1.0, self.random() * 2.0 - 1.0];
                let point = ai_aim_point(t.pos, aim_height, origin, &attr, draws);
                (point - origin).truncate().normalize_or(direction)
            }
            _ => direction,
        };
        // [stand-in] the game's attack facing path is unread (the cone gate is the only part read,
        // ai-targeting-idle.md B.8): while a foe is held and the brain asks to fire or melee, the
        // body faces the aim direction at once, as the stand-in did toward the foe.
        if foe.is_some() && (fire || melee) && !self.state.dead() && self.state.mode != sim::Mode::Action {
            self.state.yaw = (-aim_dir.x).atan2(aim_dir.y);
        }
        let input = Input {
            stick: Vec2::from_array(stick),
            aim: fire.then_some(aim_dir),
            buttons_down: buttons,
            buttons_hit: buttons & !self.prev_buttons,
            ..Input::default()
        };
        self.prev_buttons = buttons;
        self.brain_clocks.attack = ctx.attack_clock;
        self.brain_clocks.dodge = ctx.dodge_clock;
        self.brain_clocks.weapons_disabled = ctx.weapons_disabled;
        self.tactics_type = ctx.tactics_type;
        self.brain = Some(controller);
        self.last_requests = requests;
        self.last_stick = ctx.stick;
        self.last_buttons = buttons;
        self.last_unmapped = unmapped;
        (input, fire)
    }

    /// A group broadcast from another enemy reaches this one's latches [game 007236c0 ARG_GROUP
    /// loop, delivered as (name, ARG_SELF)]. Called by `route_group_messages`, between steps.
    pub fn receive_group_message(&mut self, name: u32) {
        if let Some(controller) = &self.brain {
            controller.deliver(name);
        }
    }

    /// A message sent straight to this character (the spawn record's messageListOnSpawn at spawn).
    /// [assumed] delivered to the brain's latches the same way as a group message; no brain, nothing.
    pub fn deliver_message(&mut self, name: u32) {
        if let Some(controller) = &self.brain {
            controller.deliver(name);
        }
    }

    /// The path source the brain plans with: `"mesh"` when `set_nav` gave a nav mesh, else `"straight"`.
    pub fn path_kind(&self) -> &'static str {
        if self.nav.is_some() { "mesh" } else { "straight" }
    }

    /// The tactics point the brain walks to, when the tactics record holds a valid one (without a nav
    /// mesh, the stand-off stand-in's). A valid record holding FLT_MAX (the search found nothing) is
    /// no point: None. [game] 0081e190.
    pub fn tactics_point(&self) -> Option<Vec3> {
        self.tactics.usable_point()
    }

    /// The brain as it is now, for the checks: the brain, goal and behaviour entry current, with
    /// their names where the data gives them (the report's names come from `Scout::ai`), the time
    /// in the entry, the last requests, stick, buttons, unmapped requests and messages.
    pub fn brain_report(&self) -> String {
        let Some(controller) = &self.brain else {
            return "brain: none (the stand-in brain runs, or the data did not load)".to_owned();
        };
        let names = self.brain_names.as_deref().map_or(&[][..], Vec::as_slice);
        let root = controller.brain.and_then(|b| names.get(b));
        let goal = controller.cur.goal.and_then(|g| root.and_then(|r| r.goals.get(g)));
        let entry = controller.cur.behaviour.and_then(|b| goal.and_then(|g| g.behaviours.get(b)));
        let index = |i: Option<usize>, name: Option<&str>| match (i, name) {
            (Some(i), Some(n)) => format!("{i} ({n})"),
            (Some(i), None) => i.to_string(),
            (None, _) => "none".to_owned(),
        };
        format!(
            "brain {}, goal {}, entry {}, time in entry {:.2}s, requests {:?}, stick {:?}, buttons {:#x}, unmapped {:?}, messages {:?}, form {}",
            index(controller.brain, root.map(|r| r.name.as_str())),
            index(controller.cur.goal, goal.map(|g| g.name.as_str())),
            index(controller.cur.behaviour, entry.map(String::as_str)),
            controller.cur.clock,
            self.last_requests,
            self.last_stick,
            self.last_buttons,
            self.last_unmapped,
            self.last_messages,
            if self.state.is_vehicle() { "car" } else { "robot" },
        )
    }

    pub fn muzzle(&self, scout: &Scout) -> Vec3 {
        let def = &scout.character.weapons[scout.gun_index];
        let playing = self.state.action_clip(&scout.tuning).or_else(|| {
            self.state
                .base_clip(&scout.tuning)
                .map(|c| (c.set, c.id, c.tick))
        });
        let clip = playing.and_then(|(set, id, tick)| {
            scout
                .character
                .animations
                .sets
                .get(set)?
                .iter()
                .find(|r| r.id == id)
                .and_then(|r| r.clip.as_ref().map(|c| (c, tick, r.looping)))
        });
        let nodes = clip.map_or_else(
            || scout.rig.rest(),
            |(c, tick, looping)| scout.rig.pose(c, looping, tick),
        );
        let nodes = match (clip, self.weapon_pose.arm) {
            (_, Some(arm)) => scout
                .gun_clips()
                .arm
                .get(arm.clip)
                .and_then(|r| r.clip.as_ref().map(|over| (r, over)))
                .map_or(nodes.clone(), |(r, over)| {
                    scout.rig.pose_layer(
                        clip.map(|(c, tick, looping)| (c, looping, tick)),
                        over,
                        r.looping,
                        arm.tick,
                        r.additive,
                        arm.weight(),
                    )
                }),
            _ => nodes,
        };
        let arm = scout
            .character
            .robot
            .nodes
            .iter()
            .position(|n| n.name_hash == def.bone)
            .and_then(|i| nodes.get(i));
        let local = arm.map_or(
            Vec3::Z * scout.tuning.robot_height * scout.tuning.aiming_height_percentage * 0.01,
            |a| a.transform_point3(def.muzzle),
        );
        self.state.pos + glam::Quat::from_rotation_z(self.state.yaw) * local
    }
    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// [stand-in] Arena character collision is one cylinder, rather than per-body surfaces.
/// Delivers the group broadcasts of `enemies` (ai-messages.md section 7). Each name a member sent
/// with ARG_GROUP goes to every OTHER enemy with the same `group` (the sender already had it at
/// once, through its own latches). The recipients see it on their next step [assumed]: this runs
/// after the steps of the frame. The game must call it once per step, after every enemy has stepped.
pub fn route_group_messages(enemies: &mut [Enemy]) {
    let mut sent: Vec<(usize, u32, u32)> = Vec::new();
    for (from, enemy) in enemies.iter_mut().enumerate() {
        let group = enemy.group;
        sent.extend(enemy.group_outbox.drain(..).map(|name| (from, group, name)));
    }
    for (from, group, name) in sent {
        for (i, enemy) in enemies.iter_mut().enumerate() {
            if i != from && enemy.group == group {
                enemy.receive_group_message(name);
            }
        }
    }
}

/// [test] The brain's facts after one tick: the senses the perceptions read, the message latches,
/// each brain root's score, and for the current brain each goal's aggregate and entry scores with
/// each entry's usability and score. Read by `spawned_aprot_scout_by_distance` (the spawn check).
#[cfg(test)]
fn brain_diagnosis(controller: &ai_brain::Controller, ctx: &Ctx) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let s = &ctx.senses;
    let _ = writeln!(
        out,
        "    senses: foe_distance {:?}, foe_visible {:?}, own_line_of_sight {:?}, foe_in_sight_cone {:?}, tactics_point_valid {:?}, tactics_at_point {:?}, tactics_point_distance {:?}, health_percent {:?}, on_ground {:?}",
        s.foe_distance, s.foe_visible, s.own_line_of_sight, s.foe_in_sight_cone, s.tactics_point_valid, s.tactics_at_point, s.tactics_point_distance, s.health_percent, s.on_ground
    );
    let latches: Vec<String> = ai_brain::lock_latches(&controller.latches)
        .nodes
        .iter()
        .map(|n| format!("name {:#010x} registered {} flag {} timer {:.2}", n.name_on, n.registered, n.flag, n.timer))
        .collect();
    let _ = writeln!(out, "    latches: [{}]", latches.join("; "));
    for (ri, root) in controller.brains.iter().enumerate() {
        let _ = writeln!(out, "    root {ri}: score {:.3}, current {}", (root.perceptions)(ctx), controller.brain == Some(ri));
        if controller.brain != Some(ri) {
            continue;
        }
        let Some(brain) = &root.brain else { continue };
        for (gi, goal) in brain.goals.iter().enumerate() {
            let (score, pick) = goal.evaluate(ctx, gi, &controller.done);
            let _ = writeln!(out, "      goal {gi} priority {:.2}: aggregate {:.3}, score {:.3}, pick entry {:?}", goal.priority, (goal.scorer)(ctx), score, pick);
            for (bi, entry) in goal.behaviours.iter().enumerate() {
                let usable = entry.usable(ctx, controller.done.contains(&(gi, bi)));
                let score = if usable { ai_brain::Behaviour::score(entry.behaviour.as_ref(), ctx) } else { 0.0 };
                let _ = writeln!(out, "        entry {bi} priority {:.2}: usable {usable}, score {score:.3}", entry.priority);
            }
        }
    }
    out
}

/// The game's brain of one character (`ai_bind::build`), started. None when the data does not build.
fn build_brain(scout: &Scout, make: &dyn Fn() -> Box<dyn PathSource>) -> Option<ai_brain::Controller> {
    let ai = scout.ai.as_ref()?;
    let (mut controller, _) = ai_bind::build(&ai.data, &ai.brains, make).ok()?;
    controller.start(&mut Ctx::new());
    Some(controller)
}

/// The path agent of a scout: its robot radius and height from its data [data]. The height falls
/// back to `FALLBACK_AGENT_HEIGHT` [assumed] when the data has none. Flags 0 [assumed], as nav_dump.
/// OPEN: the game's agent for a character's path is not read here (the fallback and the flags stay
/// assumed). Settles: the path-agent construction for a character in the game (the caller of the path
/// search that takes the robot's radius and height).
fn path_agent(scout: &Scout) -> Agent {
    let height = if scout.tuning.robot_height > 0.0 {
        scout.tuning.robot_height
    } else {
        FALLBACK_AGENT_HEIGHT
    };
    Agent::new(scout.tuning.robot_radius, height, 0)
}

pub fn intersect_cylinder(from: Vec3, to: Vec3, target: &Target) -> Option<f32> {
    let along = to - from;
    let (flat, offset) = (along.truncate(), (from - target.pos).truncate());
    let a = flat.length_squared();
    let b = offset.dot(flat);
    let c = offset.length_squared() - target.radius * target.radius;
    if c <= 0.0 && (0.0..=target.height).contains(&(from.z - target.pos.z)) {
        return Some(0.0);
    }
    let mut best = None;
    if a > 1e-9 && b * b - a * c >= 0.0 {
        let k = (-b - (b * b - a * c).sqrt()) / a;
        if (0.0..=1.0).contains(&k)
            && (0.0..=target.height).contains(&(from.z + along.z * k - target.pos.z))
        {
            best = Some(k);
        }
    }
    for z in [target.pos.z, target.pos.z + target.height] {
        if along.z.abs() > 1e-9 {
            let k = (z - from.z) / along.z;
            if (0.0..=1.0).contains(&k)
                && (from + along * k - target.pos).truncate().length_squared()
                    <= target.radius * target.radius
                && best.is_none_or(|v| k < v)
            {
                best = Some(k);
            }
        }
    }
    best
}

/// The control button (`control::button`, the bit of `Input::buttons_down`) of a game AI button id
/// (`ai_attack`'s BTN_ constants and the enum of docs/state-machine.md). [game] for the ids the
/// behaviours request. None: no control button is known for the id, and it is not pressed.
/// 0x15 is VehicleMode (`control::button::VEHICLE_MODE`) and the weapon mode is 0x18 [game, enum;
/// 0082ac80 raw presses 0x18, settled 2026-10-20]. 0x1c is FlyingMode [game 01185160: the bindings
/// loader stores FlyingMode's binding as entry 0x1c, as control.rs records].
pub fn button_bit(id: u32) -> Option<u32> {
    use control::button as b;
    Some(match id {
        0x00 => b::ACTION,
        0x01 => b::ATTACK_FAST,
        0x02 => b::ATTACK_SPECIAL,
        0x03 => b::ATTACK_VEHICLE,
        0x15 => b::VEHICLE_MODE,
        0x17 => b::WEAPON_FIRE,
        0x18 => b::WEAPON_MODE,
        0x1c => b::FLYING_MODE,
        0x21 => b::TRANSFORM_HOVER,
        0x22 => b::TRANSFORM_JUMP,
        0x23 => b::TRANSFORM_PUNCH,
        0x24 => b::TRANSFORM_SLAM,
        _ => return None,
    })
}

/// The AI's aim point for the data brain path [game 00824c10 step 5, 007b2f00; ai-targeting-idle.md
/// B.6 and B.7c]. The target's point raised by 0.9 x `aim_height`: the game adds 0.9 x a float of the
/// target object (+0x48, [unknown] source), so `aim_height` (the player's aiming height) stands for it
/// [stand-in]. No velocity lead [game: none found]. No clamp to the aim range: attr+0x60c, the
/// maximum aim range from the camera point [game 00824c10 step 6, disasm not checked], has no data
/// name in `AiAttr` yet [stand-in]. OPEN. The spread [game 007b2f00 shape, read 2026-10-20]: the
/// direction from `origin` is turned about z by `draws[0]` times accuracyAngleXY (attr+0x634), then
/// about the horizontal by `draws[1]` times accuracyAngleZ (attr+0x638). The amounts' UNIT is OPEN:
/// the notes disagree (ai-targeting-idle.md 364 says radians, `AiAttr` says degrees [assumed]). The
/// name-to-offset link is [assumed]. `draws` are uniform in [-1, 1). Settles: the conversion, if any,
/// applied to attr+0x634 before it reaches the rotation in 007b2f00 (its calls from 007b2f28 on are not
/// read).
fn ai_aim_point(target: [f32; 3], aim_height: f32, origin: Vec3, attr: &AiAttr, draws: [f32; 2]) -> Vec3 {
    let point = Vec3::from_array(target) + Vec3::Z * (0.9 * aim_height);
    let d = point - origin;
    if d.length_squared() <= 0.0 {
        return point;
    }
    let yawed = glam::Quat::from_axis_angle(Vec3::Z, draws[0] * attr.accuracy_angle_xy.to_radians()) * d;
    let right = yawed.cross(Vec3::Z).normalize_or(Vec3::X);
    let pitch = glam::Quat::from_axis_angle(right, draws[1] * attr.accuracy_angle_z.to_radians());
    origin + pitch * yawed
}

/// The control button a brain request presses, or None. `WeaponMode` presses the weapon mode button
/// 0x18 [game: 0082ac80 raw, `mov edi,0x18` before ai_button_request at 0082ad18; 0x1c and 0x15 are
/// only its "already down" gates]. [stand-in] those gates (no press while FlyingMode or VehicleMode is
/// down this step) and its second press, 0x19 WeaponSwitchUp when the index is not -1 and differs from
/// the weapon in hand (0082ad40..0082ad5f), are not ported here. `WeaponSwitch` names a weapon index, and its
/// button is [unknown] (ai-behaviours-attack.md section 7, item 2), so nothing is pressed. `Dodge`
/// presses the dodge button 0x0c [stand-in]: the direction is not sent. The game's 00829df0 [game,
/// read 2026-10-20] takes a direction 0..3 and calls 00822220 with (first, second) = (-1,0) for 0, (+1,0)
/// for 1, (0,+1) for 2, (0,-1) for 3 (the constants it passes; what 00822220 does with them is not read). It then
/// requests weapon mode (0082ac80) when the form field at +0x31c is 2. OPEN: the port's dodge does not
/// carry that direction; the fix belongs in the dodge's caller (ai_dodge, not in this file).
pub fn request_bit(request: Request) -> Option<u32> {
    match request {
        Request::Button(id) => button_bit(id),
        Request::WeaponMode(_) => Some(control::button::WEAPON_MODE),
        Request::WeaponSwitch(_) => None,
        Request::Dodge(_) => Some(control::button::DODGE),
        // A release presses nothing; brain_input clears its bit (not an unmapped press).
        Request::Release(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::damage::flags;
    use crate::ai_perceive::{Aggregate, CALC_AVG, Link, Timer};

    /// A perception that passes (score 1), as the ai_attack tests build one.
    fn passing() -> Aggregate {
        Aggregate::new(
            CALC_AVG,
            0,
            vec![Link {
                invert: false,
                perception: Box::new(Timer::new(1.0, 36.0, true)),
                message: 0,
                arg: 0,
            }],
        )
    }

    /// A controller with one root and one goal holding one behaviour entry: the test's own brain.
    fn hand_brain(behaviour: Box<dyn ai_brain::Behaviour>) -> ai_brain::Controller {
        let goal = ai_brain::Goal::new(1.0, ai_brain::no_perceptions(), vec![ai_brain::BehaviourEntry::new(1.0, behaviour)]);
        let root = ai_brain::Root {
            perceptions: ai_brain::no_perceptions(),
            persistent: Vec::new(),
            brain: Some(ai_brain::Brain::new(vec![goal])),
        };
        let mut controller = ai_brain::Controller::new(vec![root]);
        controller.start(&mut Ctx::new());
        controller
    }

    #[test]
    fn button_map_places_the_known_ids_and_leaves_the_rest_unplaced() {
        assert_eq!(button_bit(0x17), Some(control::button::WEAPON_FIRE));
        assert_eq!(button_bit(1), Some(control::button::ATTACK_FAST));
        assert_eq!(button_bit(0x24), Some(control::button::TRANSFORM_SLAM));
        assert_eq!(button_bit(0x99), None);
        assert_eq!(request_bit(Request::WeaponSwitch(2)), None);
        assert_eq!(request_bit(Request::WeaponMode(-1)), Some(control::button::WEAPON_MODE));
    }

    #[test]
    fn a_driven_enemy_moves_on_its_input_and_shoots_where_it_is_told() {
        if crate::install::missing() { return; }
        let scout = installed();
        let t = player_tuning();
        let mut player = State::new(&t);
        player.pos = Vec3::new(0.0, 20.0, 0.0);
        // The player behind it: its brain would turn to him, its input walks it the other way.
        let mut enemy = Enemy::new(1, Vec3::ZERO, scout);
        enemy.use_data_brain = true;
        enemy.driven = Some(Driven { input: Input { stick: Vec2::new(0.0, -1.0), ..Default::default() }, aim: Vec3::ZERO });
        for _ in 0..60 {
            enemy.step(scout, &player, &t, &Arena::default(), false, sim::GAME_UPDATE);
        }
        assert!(enemy.state.pos.y < -5.0, "walked to {:?}", enemy.state.pos);
        // Weapon mode and the trigger, aimed at the player's chest: its rounds strike him.
        let chest = player.pos + Vec3::Z * 3.0;
        let toward = (player.pos - enemy.state.pos).truncate().normalize();
        let input = Input { aim: Some(toward), buttons_down: 1 << control::button::WEAPON_FIRE, ..Default::default() };
        enemy.driven = Some(Driven { input, aim: chest });
        let mut struck = 0;
        for _ in 0..90 {
            struck += enemy.step(scout, &player, &t, &Arena::default(), false, sim::GAME_UPDATE).iter().filter(|s| s.target == Some(player.actor.id)).count();
        }
        assert!(enemy.shots > 0 && struck > 0, "{} rounds, {struck} on him", enemy.shots);
    }

    #[test]
    fn data_brain_walks_toward_the_player() {
        if crate::install::missing() { return; }
        let scout = installed();
        let t = player_tuning();
        let mut player = State::new(&t);
        // 150 m: inside distanceForTacticsMax (200) and beyond the scout's distanceFromTarget band
        // (30..100), so the stand-off point is 100 m from him and the scout walks in.
        player.pos = Vec3::new(0.0, 150.0, 0.0);
        let mut enemy = Enemy::new(1, Vec3::ZERO, scout);
        enemy.use_data_brain = true;
        let walk = crate::ai_move::GoToTarget::new(
            crate::ai_move::TargetSpec {
                target_type: crate::ai_move::TARGET_FOE,
                group_id: 0,
                calc: 0,
                range_min: 0.0,
                range_max: 0.0,
            },
            0,
            false,
            -1,
            Box::new(StraightLine),
            passing(),
        );
        enemy.brain = Some(hand_brain(Box::new(walk)));
        let before = (player.pos - enemy.state.pos).truncate().length();
        for _ in 0..100 {
            enemy.step(scout, &player, &t, &Arena::default(), false, sim::GAME_UPDATE);
        }
        let after = (player.pos - enemy.state.pos).truncate().length();
        assert!(after < before - 5.0, "distance {before} -> {after}; {}", enemy.brain_report());
    }

    #[test]
    fn data_brain_ranged_goal_presses_fire_and_shoots() {
        if crate::install::missing() { return; }
        let scout = installed();
        let t = player_tuning();
        let mut player = State::new(&t);
        player.pos = Vec3::new(0.0, 300.0, 0.0);
        let mut enemy = Enemy::new(1, Vec3::ZERO, scout);
        enemy.use_data_brain = true;
        enemy.brain = Some(hand_brain(Box::new(ai_attack::AttackRanged {
            ignore_attack_delay: true,
            ignore_kung_fu: false,
            weapon_index: -1,
            perception: passing(),
            channel: 0,
        })));
        let (mut fire_seen, mut shots) = (false, 0);
        for _ in 0..400 {
            shots += enemy.step(scout, &player, &t, &Arena::default(), false, sim::GAME_UPDATE).len();
            fire_seen |= enemy.last_buttons & (1 << control::button::WEAPON_FIRE) != 0;
        }
        assert!(
            fire_seen,
            "no fire button: {}; state {:?}",
            enemy.brain_report(),
            enemy.state.control_state(&scout.tuning)
        );
        assert!(shots > 0, "fire button pressed but no shot: {}; state {:?}, firing {}, gun {:?}", enemy.brain_report(), enemy.state.control_state(&scout.tuning), enemy.state.firing, enemy.weapon_pose);
    }

    /// The convoy as a group member of Escort (705cdac2), in sight and alive, at `pos`.
    fn escort_member(pos: Vec3) -> GroupMember {
        GroupMember {
            group: crc32(b"Escort"),
            pos,
            in_sight: true,
            alive: true,
        }
    }

    #[test]
    fn group_input_feeds_the_nearest_live_member_per_group() {
        let escort = crc32(b"Escort");
        let member = |pos: Vec3, alive: bool| GroupMember {
            group: escort,
            pos,
            in_sight: true,
            alive,
        };
        let members = [
            member(Vec3::new(0.0, 80.0, 0.0), true),
            member(Vec3::new(0.0, 30.0, 0.0), true),
            member(Vec3::new(0.0, 5.0, 0.0), false),   // dead: no target
            member(Vec3::new(0.0, 6000.0, 0.0), true), // beyond the group range
        ];
        let found = nearest_per_group(&members, Vec3::ZERO);
        assert_eq!(found.len(), 1, "one entry per group: {found:?}");
        assert!((found[0].1 - 30.0).abs() < 1e-4, "the nearest live member: {found:?}");
        assert!(found[0].2, "in sight");
    }

    /// The Escort goal of the installed scout, driven by an AttackEscortOn latch and the convoy as
    /// group member and foe 50 m away. The latch is set by a message (deliver_message), as the spawn
    /// list sets it. Skipped without the install.
    #[test]
    fn escort_latch_picks_the_escort_goal_and_fires_at_the_convoy() {
        if !crate::install::dir().join("bnxglobal.str").is_file() {
            return; // skipped: the install is absent
        }
        let scout = installed();
        let t = player_tuning();
        let mut convoy = State::new(&t);
        convoy.pos = Vec3::new(0.0, 50.0, 0.0);
        let mut enemy = Enemy::new(1, Vec3::ZERO, scout);
        convoy.actor.team = enemy.state.actor.team.wrapping_add(1);
        enemy.use_data_brain = true;
        enemy.deliver_message(crc32(b"AttackEscortOn"));
        enemy.group_input = vec![escort_member(convoy.pos)];
        let (mut escort_seen, mut fire_seen, mut away) = (false, false, 0usize);
        for _ in 0..200 {
            let shots = enemy.step(scout, &convoy, &t, &Arena::default(), false, sim::GAME_UPDATE);
            for s in &shots {
                away += usize::from((s.to - s.from).truncate().dot(Vec2::Y) <= 0.0);
            }
            escort_seen |= current_names(&enemy).is_some_and(|(_, g, e)| g.contains("Escort") || e.contains("Escort"));
            fire_seen |= enemy.last_buttons & (1 << control::button::WEAPON_FIRE) != 0;
        }
        // The objective brain (brainObjectiveAttackEscort, brainList index 2, priority 0.75) wins the brain choice
        // only because its score is the aggregate x priority (ai_bind::weight_root, [game 0082eae0]). Unscaled, it
        // tied brainDComScout (index 1, 0.5) at 1.0 and lost by list order. The undamaged enemy passes its
        // inverted 5ada9b7c link ("5ada9b7c ... settled").
        assert!(escort_seen, "no Escort goal or entry chosen: {}\n{}", enemy.brain_report(), enemy.diag);
        assert!(fire_seen, "no fire button: {}", enemy.brain_report());
        assert_eq!(away, 0, "every shot goes toward the convoy (+y): {}", enemy.brain_report());
    }

    /// The Escort attack through the group perception alone (no objective brain, so no gate of its own): an
    /// AttackRanged whose perception is the group member distance below 60 m (712ecdc7, groupID Escort). With the
    /// convoy as group member 50 m away it fires, toward the convoy (the foe); with no member it does not fire.
    /// Skipped without the install.
    #[test]
    fn escort_attack_fires_at_the_convoy_through_the_group_perception() {
        if !crate::install::dir().join("bnxglobal.str").is_file() {
            return; // skipped: the install is absent
        }
        let scout = installed();
        let t = player_tuning();
        let mut convoy = State::new(&t);
        convoy.pos = Vec3::new(0.0, 50.0, 0.0);
        convoy.actor.team = Enemy::new(1, Vec3::ZERO, scout).state.actor.team.wrapping_add(1);
        let near_member = || Aggregate::new(CALC_AVG, 0, vec![Link {
            invert: false,
            perception: Box::new(crate::ai_perceive::Compare {
                value_type: 0x712e_cdc7,
                group_id: crc32(b"Escort"),
                data: crate::ai_perceive::CompareData {
                    compare_value: 60.0,
                    compare_value_in_behaviour: -1.0,
                    assigned: 0,
                    compare_type: crate::ai_perceive::CMP_BELOW,
                },
            }),
            message: 0,
            arg: 0,
        }]);
        let run = |members: Vec<GroupMember>| {
            let mut enemy = Enemy::new(1, Vec3::ZERO, scout);
            enemy.use_data_brain = true;
            enemy.group_input = members;
            enemy.brain = Some(hand_brain(Box::new(ai_attack::AttackRanged {
                ignore_attack_delay: true,
                ignore_kung_fu: false,
                weapon_index: -1,
                perception: near_member(),
                channel: 0,
            })));
            let (mut fire_seen, mut away, mut shots) = (false, 0usize, 0usize);
            for _ in 0..300 {
                let s = enemy.step(scout, &convoy, &t, &Arena::default(), false, sim::GAME_UPDATE);
                shots += s.len();
                away += s.iter().filter(|s| (s.to - s.from).truncate().dot(Vec2::Y) <= 0.0).count();
                fire_seen |= enemy.last_buttons & (1 << control::button::WEAPON_FIRE) != 0;
            }
            (fire_seen, shots, away, enemy.brain_report())
        };
        let (fire, shots, away, report) = run(vec![escort_member(convoy.pos)]);
        assert!(fire && shots > 0, "the member 50 m away is in the group perception: {report}");
        assert_eq!(away, 0, "every shot goes toward the convoy (+y): {report}");
        let (fire, shots, _, report) = run(Vec::new());
        assert!(!fire && shots == 0, "no member, no group perception, no fire: {report}");
    }

    /// Without the latch the Escort goal is not chosen, even with the convoy in group and in sight: the objective's
    /// REQUIRE message link fails, so its root scores 0 and the brain choice keeps the scout's brains.
    /// The escort_attack test above is the check of the group feed on its own.
    #[test]
    fn escort_goal_needs_the_latch() {
        if !crate::install::dir().join("bnxglobal.str").is_file() {
            return; // skipped: the install is absent
        }
        let scout = installed();
        let t = player_tuning();
        let mut convoy = State::new(&t);
        convoy.pos = Vec3::new(0.0, 50.0, 0.0);
        let mut enemy = Enemy::new(1, Vec3::ZERO, scout);
        convoy.actor.team = enemy.state.actor.team.wrapping_add(1);
        enemy.use_data_brain = true;
        enemy.group_input = vec![escort_member(convoy.pos)];
        let mut escort_seen = false;
        for _ in 0..200 {
            enemy.step(scout, &convoy, &t, &Arena::default(), false, sim::GAME_UPDATE);
            escort_seen |= current_names(&enemy).is_some_and(|(_, g, e)| g.contains("Escort") || e.contains("Escort"));
        }
        assert!(!escort_seen, "the Escort goal without AttackEscortOn: {}", enemy.brain_report());
    }

    /// With the latch but no group member, the enemy falls back: the Escort goal does not score, and
    /// the step runs without a convoy in group (no NaN, no panic).
    #[test]
    fn escort_latch_without_a_member_falls_back() {
        if !crate::install::dir().join("bnxglobal.str").is_file() {
            return; // skipped: the install is absent
        }
        let scout = installed();
        let t = player_tuning();
        let mut convoy = State::new(&t);
        convoy.pos = Vec3::new(0.0, 50.0, 0.0);
        let mut enemy = Enemy::new(1, Vec3::ZERO, scout);
        convoy.actor.team = enemy.state.actor.team.wrapping_add(1);
        enemy.use_data_brain = true;
        enemy.deliver_message(crc32(b"AttackEscortOn"));
        // [game] goalAttackEscort has no goal perceptions (brains_full.txt), so it can be entered with no
        // member. Its Escort behaviours need a member and score 0; behaviorIdleCombat (alert distance on
        // the foe) runs instead. So the check is on the behaviour entry, not on the goal.
        let mut escort_entry_seen = false;
        for _ in 0..200 {
            enemy.step(scout, &convoy, &t, &Arena::default(), false, sim::GAME_UPDATE);
            escort_entry_seen |= current_names(&enemy).is_some_and(|(_, _, e)| e.contains("Escort"));
        }
        assert!(!escort_entry_seen, "an Escort behaviour ran with no member: {}", enemy.brain_report());
        assert!(enemy.state.pos.is_finite(), "the step stays finite");
    }

    #[test]
    fn installed_scout_loads_its_game_brain() {
        if crate::install::missing() { return; }
        let scout = installed();
        let ai = scout.ai.as_ref().unwrap_or_else(|| panic!("no game brain: {:?}", scout.ai_error));
        assert!(!ai.brains.is_empty(), "brainList of characterAttributesDComScout");
        let enemy = Enemy::new(1, Vec3::ZERO, scout);
        assert!(enemy.brain.is_some(), "the controller is built and started");
        assert!(enemy.brain_report().starts_with("brain 0 ("), "{}", enemy.brain_report());
    }

    /// The brain, goal and behaviour entry names the controller is in now (from the names of the
    /// brains it holds, in its order), or None when it is in no goal.
    fn current_names(enemy: &Enemy) -> Option<(String, String, String)> {
        let c = enemy.brain.as_ref()?;
        let names = enemy.brain_names.as_deref()?;
        let root = names.get(c.brain?)?;
        let goal = root.goals.get(c.cur.goal?)?;
        let entry = goal.behaviours.get(c.cur.behaviour?)?;
        Some((root.name.clone(), goal.name.clone(), entry.clone()))
    }

    #[test]
    fn ai_aim_point_has_the_height_offset_and_its_spread() {
        let attr = AiAttr {
            accuracy_angle_xy: 5.0,
            accuracy_angle_z: 3.0,
            ..AiAttr::default()
        };
        let p = ai_aim_point([0.0, 40.0, 0.0], 2.0, Vec3::ZERO, &attr, [0.0, 0.0]);
        assert!((p - Vec3::new(0.0, 40.0, 1.8)).length() < 1e-4, "0.9 x 2 m up, no spread: {p:?}");
        let q = ai_aim_point([0.0, 40.0, 0.0], 2.0, Vec3::ZERO, &attr, [1.0, 0.0]);
        let yaw = q.x.atan2(q.y).to_degrees().abs();
        assert!((yaw - 5.0).abs() < 1e-3, "a full yaw draw is 5 degrees [assumed degrees]: {yaw}");
        assert!((q.length() - Vec3::new(0.0, 40.0, 1.8).length()).abs() < 1e-3, "the spread keeps the distance");
    }

    /// The installed scout's own game brain, as the game builds it (`Enemy::new`: brainList[0] is
    /// started, and the step's re-pick chooses the brain): the player stands 40 m away in plain view
    /// for 20 simulated seconds. Prints a timeline each time the brain, goal or entry changes, with
    /// the buttons and requests of that step, then a summary. Asserts only that it runs and that the
    /// brain selects some goal. Run with --nocapture to see the timeline.
    #[test]
    fn installed_scout_brain_timeline() {
        if !crate::install::dir().join("bnxglobal.str").is_file() {
            return; // skipped: the install is absent
        }
        let scout = installed();
        let t = player_tuning();
        let mut player = State::new(&t);
        player.pos = Vec3::new(0.0, 40.0, 0.0);
        let mut enemy = Enemy::new(1, Vec3::ZERO, scout);
        enemy.use_data_brain = true;
        let steps = (20.0 / sim::GAME_UPDATE).round() as usize;
        let (mut last, mut picked_any, mut time) = (None::<(String, String, String)>, false, 0.0_f32);
        let (mut fire_steps, mut shots, mut stick_steps) = (0usize, 0usize, 0usize);
        // Messages the brain sent (hash, count steps), as the game side gets them.
        let mut messages: std::collections::BTreeMap<u32, usize> = Default::default();
        println!("timeline: time s | brain | goal | entry | distance to player m | buttons | requests");
        for _ in 0..steps {
            shots += enemy.step(scout, &player, &t, &Arena::default(), false, sim::GAME_UPDATE).len();
            time += sim::GAME_UPDATE;
            for (hash, _) in &enemy.last_messages {
                *messages.entry(*hash).or_default() += 1;
            }
            fire_steps += usize::from(enemy.last_buttons & (1 << control::button::WEAPON_FIRE) != 0);
            stick_steps += usize::from(enemy.last_stick != [0.0, 0.0]);
            let now = current_names(&enemy);
            picked_any |= now.is_some();
            if now != last {
                let d = (player.pos - enemy.state.pos).truncate().length();
                let (b, g, e) = now.clone().unwrap_or(("none".into(), "none".into(), "none".into()));
                println!(
                    "  {time:6.2} | {b} | {g} | {e} | {d:.1} | {:#x} | {:?}",
                    enemy.last_buttons, enemy.last_requests
                );
                last = now;
            }
        }
        let end = enemy.brain.as_ref().map(|c| c.cur);
        println!(
            "summary: {steps} steps; fire button in {fire_steps} steps; {shots} shots; stick in {stick_steps} steps; end: {end:?}, requests {:?}",
            enemy.last_requests
        );
        let sent: Vec<String> = messages.iter().map(|(h, n)| format!("{h:#010x} x{n}")).collect();
        println!("messages sent by the brain (hash x steps): {}", sent.join(", "));
        assert!(picked_any, "the brain selects some goal: {end:?}");
    }

    /// Every pack and block the spawn table maps (`ai_spawn::character_inputs`) loads as a scout from the
    /// install, with its brain list. Skipped without the install.
    #[test]
    fn spawn_table_entries_load_as_scouts() {
        let dir = crate::install::dir();
        if !dir.join("bnxglobal.str").is_file() {
            return; // skipped: the install is absent
        }
        for def in [crate::ai_spawn::DEF_APROT_SCOUT, crate::ai_spawn::DEF_FAKE_DCOM, crate::ai_spawn::DEF_DCOM_SCOUT, crate::ai_spawn::DEF_DCOM_SNIPER, crate::ai_spawn::DEF_DCOM_WARRIOR] {
            let (pack, attributes) = crate::ai_spawn::character_inputs(def).expect("in the table");
            let scout = Scout::load_as(dir, pack, attributes).unwrap_or_else(|e| panic!("{pack}:{attributes}: {e}"));
            assert!(scout.ai.is_some(), "{pack}:{attributes} has no game brain: {:?}", scout.ai_error);
            println!("{pack}:{attributes}: team {}, brains {}", scout.tuning.damage.team, scout.ai.as_ref().map_or(0, |ai| ai.brains.len()));
        }
    }

    /// The spawn check (hunt, item 4): an APrtScout built the way `targets::spawn_run` builds one
    /// (`Scout::load_as` aprotscoutmp / APrtScout, `Enemy::new`, then the record's AlertedOn delivered
    /// at spawn, use_data_brain on), with the player at 90, 200 and 400 m on +y. No nav mesh here, so
    /// the sight is the stand-in's (the arena's flat floor, no walls): the hunt's own mesh is not
    /// loaded. Prints the brain diagnosis at steps 1, 30 and 60, once with the AlertedOn and once
    /// without. Asserts nothing about the values. Run with --nocapture. Skipped without the install.
    #[test]
    fn spawned_aprot_scout_by_distance() {
        let dir = crate::install::dir();
        if !dir.join("bnxglobal.str").is_file() {
            return; // skipped: the install is absent
        }
        let scout = Scout::load_as(dir, "aprotscoutmp", "APrtScout").expect("aprot scout");
        let t = player_tuning();
        let alerted = crc32(b"AlertedOn");
        // A 100 m wide wall, 4 m thick, across the line at 120 m: it blocks the sight (not the walk
        // round it, since it is 8 m tall and 100 m wide only at y 120..124 for x -50..50).
        let wall = Arena { boxes: vec![sim::Box3 { min: Vec3::new(-50.0, 120.0, 0.0), max: Vec3::new(50.0, 124.0, 8.0) }], ..Default::default() };
        for (case, alert, walled) in [("AlertedOn at spawn", true, false), ("no AlertedOn", false, false), ("AlertedOn, wall at 120 m", true, true)] {
            for distance in [90.0_f32, 200.0, 400.0] {
                let mut player = State::new(&t);
                // The hunt's player team (the log's "player 2773336647"); the scout's team is its own.
                player.actor.team = 2_773_336_647;
                player.pos = Vec3::new(0.0, distance, 0.0);
                let mut enemy = Enemy::new(4100, Vec3::ZERO, &scout);
                enemy.use_data_brain = true;
                if alert {
                    enemy.deliver_message(alerted);
                }
                let arena = if walled { &wall } else { &Arena::default() };
                println!("== APrtScout at 0 m, player at {distance} m, {case}; team {} (scout {})", player.actor.team, scout.tuning.damage.team);
                for step in 1..=60 {
                    enemy.step(&scout, &player, &t, arena, false, sim::GAME_UPDATE);
                    if matches!(step, 1 | 30 | 60) {
                        println!("  step {step} ({:.2} s): {}", step as f32 * sim::GAME_UPDATE, enemy.brain_report());
                        print!("{}", enemy.diag);
                    }
                }
            }
        }
    }

    /// The installed scout's game brain on the us_test street with its nav mesh, built the way
    /// `tactics_check` builds it (one placement per object). The player stands on the walkable ground
    /// nearest the middle of the mesh, and the scout 80 m away on the same floor. 30 simulated seconds
    /// on the level's collision. Prints the goal and behaviour changes, the tactics point when it
    /// changes, the distance to the player every 5 s, and the shots. Asserts only: no panic, no NaN
    /// position, and the scout moved more than 5 m. Skipped without the install.
    #[test]
    fn installed_scout_brain_on_us_test_mesh() {
        let dir = crate::install::dir();
        if !dir.join("bnxglobal.str").is_file() {
            return; // skipped: the install is absent
        }
        let scout = installed();
        let t = player_tuning();
        let zone = dir.join("levels/us_test/zone.str");
        let (sils, _) = crate::navmesh::load(&zone).expect("us_test silhouettes");
        let mut seen = std::collections::HashSet::new();
        let first: Vec<_> = sils.iter().filter(|s| seen.insert(s.object)).cloned().collect();
        let mesh = Arc::new(crate::navmesh::NavMesh::build(None, &first).expect("us_test nav mesh"));
        // [stand-in] The ground is the arena's flat plane at z 0, not the level's collision: on this
        // street the mesh's floors (-23 m here) and the mission's collision never agree (no walkable
        // triangle of the mesh has a collision floor at its height, measured 2026-10-12), so the
        // collision cannot stand under the mesh. The mesh gives the layout in x and y only.
        let arena = Arena::default();

        // The player: the walkable triangle nearest the middle of the mesh box (as tactics_check).
        let (lo, hi) = mesh.bounds();
        let mid = (lo + hi) * 0.5;
        let walkable = |tri: usize| mesh.tri_ceiling(tri) == f32::MAX && mesh.tri_floor(tri) != f32::MAX;
        let centre_distance = |tri: usize| {
            let ic = mesh.tri_incenter(tri);
            (ic[0] - mid.x).powi(2) + (ic[1] - mid.y).powi(2)
        };
        let Some(tri) = mesh.triangles().filter(|&tri| walkable(tri)).min_by(|&a, &b| centre_distance(a).total_cmp(&centre_distance(b))) else {
            println!("no walkable triangle on the us_test mesh");
            return;
        };
        let ic = mesh.tri_incenter(tri);
        let floor = mesh.tri_floor(tri);
        // The scout: 80 m from the player on walkable ground of the same mesh floor (the first angle that fits).
        let planned = Vec3::new(ic[0], ic[1], floor);
        let attacker = (0..36).find_map(|step| {
            let a = step as f32 * 10.0_f32.to_radians();
            let p = Vec3::new(planned.x + 80.0 * a.cos(), planned.y + 80.0 * a.sin(), floor);
            mesh.locate(p)
                .filter(|&t| walkable(t) && mesh.tri_floor(t) == floor)
                .map(|_| p)
        });
        let Some(attacker) = attacker else {
            println!("no walkable point 80 m from the player on its floor");
            return;
        };
        let target = Vec3::new(planned.x, planned.y, 0.0);
        let attacker = Vec3::new(attacker.x, attacker.y, 0.0);

        let mut player = State::new(&t);
        player.pos = target;
        let mut enemy = Enemy::new(1, attacker, scout);
        enemy.use_data_brain = true;
        enemy.set_nav(Arc::clone(&mesh), scout);
        let start = enemy.state.pos;
        println!(
            "us_test: mesh {} triangles; player at {target:?}, scout at {attacker:?} ({:.1} m apart)",
            mesh.tri_count(),
            (attacker - target).truncate().length()
        );
        println!("timeline: time s | brain | goal | entry | distance to player m | buttons | requests");

        let steps = (30.0 / sim::GAME_UPDATE).round() as usize;
        let (mut last, mut time, mut next_report) = (None::<(String, String, String)>, 0.0_f32, 5.0_f32);
        let (mut shots, mut fire_steps, mut tactics_changes) = (0usize, 0usize, 0usize);
        let mut last_point: Option<Vec3> = None;
        for _ in 0..steps {
            shots += enemy.step(scout, &player, &t, &arena, false, sim::GAME_UPDATE).len();
            time += sim::GAME_UPDATE;
            fire_steps += usize::from(enemy.last_buttons & (1 << control::button::WEAPON_FIRE) != 0);
            let now = current_names(&enemy);
            if now != last {
                let d = (player.pos - enemy.state.pos).truncate().length();
                let (b, g, e) = now.clone().unwrap_or(("none".into(), "none".into(), "none".into()));
                println!("  {time:6.2} | {b} | {g} | {e} | {d:.1} | {:#x} | {:?}", enemy.last_buttons, enemy.last_requests);
                last = now;
            }
            let point = enemy.tactics.valid.then_some(enemy.tactics.point);
            if point != last_point {
                tactics_changes += 1;
                if let Some(p) = point {
                    println!("  {time:6.2} | tactics point {p:?} (goal {:08x}, type {:08x}), {:.1} m from the scout", enemy.tactics.goal, enemy.tactics_type, (p - enemy.state.pos).truncate().length());
                } else {
                    println!("  {time:6.2} | tactics point unset");
                }
                last_point = point;
            }
            if time >= next_report {
                let d = (player.pos - enemy.state.pos).truncate().length();
                println!("  {time:6.2} | distance to player {d:.1} m, shots so far {shots}");
                next_report += 5.0;
            }
        }
        let end = enemy.state.pos;
        println!(
            "summary: {steps} steps; fire button in {fire_steps} steps; {shots} shots; tactics point changes {tactics_changes}; end {end:?}, brain: {}",
            enemy.brain_report()
        );
        assert!(end.is_finite(), "no NaN position: {end:?}");
        assert!(
            (end - start).truncate().length() > 5.0,
            "the scout moved more than 5 m: from {start:?} to {end:?}; {}",
            enemy.brain_report()
        );
    }

    #[test]
    fn settle_takes_the_ground_under_grounded_feet_and_keeps_z_without_one() {
        // Ground z = 0.1 x. Grounded at x = 10 (z 1.0); walking to x = 20 the feet follow the ground to 2.0.
        let sloped = |x: f32, _y: f32, _z: f32| 0.1 * x;
        let mut p = Vec3::new(10.0, 0.0, 1.0);
        assert!((probe_ground(sloped, p.x, p.y, p.z).unwrap() - 1.0).abs() < 1e-5);
        p.x = 20.0;
        settle_on_ground(&mut p, sloped);
        assert!((p.z - 2.0).abs() < 1e-5, "{p:?}");
        // No floor anywhere: z kept.
        let none = |_x: f32, _y: f32, _z: f32| crate::level_world::NO_FLOOR;
        let mut q = Vec3::new(0.0, 0.0, 3.0);
        settle_on_ground(&mut q, none);
        assert_eq!(q.z, 3.0);
        // Airborne (more than a step above the floor under the feet): left to the sim.
        let mut air = Vec3::new(10.0, 0.0, 5.0);
        settle_on_ground(&mut air, sloped);
        assert_eq!(air.z, 5.0);
    }

    /// A walker with a nav mesh walks a sloped ramp (the collision ground, z = 0.28 y) over a mesh whose
    /// floor is the box bottom, -30 m. Its feet must follow the ramp and never the mesh constant.
    /// [stand-in] the walk is the stand-in's straight approach (no mesh path is walked here). Skipped
    /// without the install (Enemy needs the installed scout).
    #[test]
    fn walker_on_a_mesh_keeps_the_ground_z_not_the_mesh_constant() {
        let dir = crate::install::dir();
        if !dir.join("bnxglobal.str").is_file() {
            return; // skipped: the install is absent
        }
        let scout = installed();
        let t = player_tuning();
        let mesh = Arc::new(
            NavMesh::build(Some((Vec3::new(-30.0, -30.0, -30.0), Vec3::new(30.0, 60.0, 30.0))), &[]).expect("empty box builds"),
        );
        let arena = Arena {
            ramps: vec![crate::terrain::Ramp {
                min: Vec2::new(-20.0, 0.0),
                max: Vec2::new(20.0, 50.0),
                base: 0.0,
                height: 0.0,
                slope: Vec2::new(0.0, 0.28),
            }],
            ..Default::default()
        };
        let ground = |y: f32| 0.28 * y;
        // (a) The enemy with the nav mesh, stood on the ramp: its feet stay on the ramp for the steps.
        let mut enemy = Enemy::new(1, Vec3::new(0.0, 5.0, ground(5.0)), scout);
        enemy.set_nav(Arc::clone(&mesh), scout);
        let mut player = State::new(&t);
        player.pos = Vec3::new(0.0, 45.0, ground(45.0));
        for _ in 0..((3.0 / sim::GAME_UPDATE).round() as usize) {
            enemy.step(scout, &player, &t, &arena, false, sim::GAME_UPDATE);
            let p = enemy.state.pos;
            assert!(p.is_finite(), "{p:?}");
            assert!((p.z - ground(p.y)).abs() < 0.05, "feet off the ramp: {p:?}, ramp z {}", ground(p.y));
        }
        // (b) Walking up the ramp: sim::step with a forward stick, and the settle after each step.
        // The stand-in does not walk this far (it engages inside its sight only), so the walk is driven here.
        let mut walker = State::new(&t);
        walker.pos = Vec3::new(0.0, 5.0, ground(5.0));
        let mesh_floor = -30.0_f32;
        let input = Input { stick: Vec2::new(0.0, 1.0), ..Default::default() };
        for _ in 0..((10.0 / sim::GAME_UPDATE).round() as usize) {
            // The ramp ends at y 50; past 45 the walk stops (off its end the feet fall, not settle).
            if walker.pos.y > 45.0 {
                break;
            }
            sim::step(&mut walker, &input, &t, &arena, sim::GAME_UPDATE);
            settle_on_ground(&mut walker.pos, |x: f32, y: f32, z: f32| arena.floor(x, y, z));
            let p = walker.pos;
            assert!(p.is_finite(), "{p:?}");
            assert!((p.z - ground(p.y)).abs() < 0.05 && p.z != mesh_floor, "feet off the ramp: {p:?}, ramp z {}", ground(p.y));
        }
        assert!(walker.pos.y > 8.0, "the walker moved up the ramp: {:?}", walker.pos);
    }

    /// [test] The fault of 2026-10-13: pass 1 sets VALID by distance (the foe is 145 m away, inside
    /// 200 m) while the candidate search finds nothing on this mesh (the foe stands off it), so pass 2
    /// stores FLT_MAX on all axes with VALID set. The scout must read that as no point: no tactics
    /// point, no NaN, and no move toward infinity. Skipped without the install (the scout data).
    #[test]
    fn valid_record_holding_flt_max_is_no_point_and_the_scout_stays_put() {
        if !crate::install::dir().join("bnxglobal.str").is_file() {
            return; // skipped: the install is absent
        }
        let scout = installed();
        let t = player_tuning();
        let mesh = Arc::new(
            NavMesh::build(Some((Vec3::new(-30.0, -30.0, -30.0), Vec3::new(30.0, 60.0, 30.0))), &[]).expect("empty box builds"),
        );
        let arena = Arena::default();
        let mut enemy = Enemy::new(1, Vec3::new(0.0, 5.0, 0.0), scout);
        enemy.use_data_brain = true; // the tactics driver runs inside the data brain's step
        enemy.set_nav(Arc::clone(&mesh), scout);
        let start = enemy.state.pos;
        let mut player = State::new(&t);
        player.pos = Vec3::new(0.0, 150.0, 0.0);
        let mut saw_flt_max = false;
        for _ in 0..((5.0 / sim::GAME_UPDATE).round() as usize) {
            enemy.step(scout, &player, &t, &arena, false, sim::GAME_UPDATE);
            let p = enemy.state.pos;
            assert!(p.is_finite(), "no NaN position: {p:?}");
            assert!(enemy.last_stick.iter().all(|c| c.is_finite()), "no NaN stick: {:?}", enemy.last_stick);
            if enemy.tactics.valid && enemy.tactics.point.x == f32::MAX {
                saw_flt_max = true;
                assert!(enemy.tactics.usable_point().is_none(), "a valid record holding FLT_MAX is no point");
                assert!(enemy.tactics_point().is_none(), "the reader gives no point");
            }
        }
        let end = enemy.state.pos;
        assert!(saw_flt_max, "the search found a point (or no valid record): {:?}, data {}; {}", enemy.tactics, scout.tactics.is_some(), enemy.brain_report());
        assert!(enemy.tactics_point().is_none(), "no tactics point at the end: {}", enemy.brain_report());
        assert!((end - start).truncate().length() < 0.5, "the scout stays put: from {start:?} to {end:?}; {}", enemy.brain_report());
    }

    /// [test] Diagnosis of the idle alternation (2026-10-13): an unalerted DComScout, the foe 600 m
    /// away, 20 s on the data brain. Prints the entry, the form and the requests every 0.5 s. Asserts
    /// only that it runs without NaN. Skipped without the install.
    #[test]
    fn idle_unalerted_scout_form_and_requests_over_20_seconds() {
        if !crate::install::dir().join("bnxglobal.str").is_file() {
            return; // skipped: the install is absent
        }
        let scout = installed();
        let t = player_tuning();
        let arena = Arena::default();
        let mut enemy = Enemy::new(1, Vec3::ZERO, scout);
        enemy.use_data_brain = true;
        let mut player = State::new(&t);
        player.pos = Vec3::new(600.0, 0.0, 0.0);
        let steps = (20.0 / sim::GAME_UPDATE).round() as usize;
        let report_every = (0.5 / sim::GAME_UPDATE).round() as usize;
        let mut time = 0.0_f32;
        let mut entries = Vec::new();
        for i in 0..steps {
            enemy.step(scout, &player, &t, &arena, false, sim::GAME_UPDATE);
            time += sim::GAME_UPDATE;
            assert!(enemy.state.pos.is_finite(), "no NaN position at {time:.2} s");
            let report = enemy.brain_report();
            let entry = report.split("entry ").nth(1).and_then(|s| s.split(',').next()).unwrap_or("?").to_owned();
            entries.push(entry.clone());
            if i % report_every == 0 {
                println!(
                    "idle {time:5.2} s | form {} | in_vehicle {} | {} | requests {:?} | buttons {:#x}",
                    if enemy.state.is_vehicle() { "car" } else { "robot" },
                    enemy.state.is_vehicle(),
                    entry,
                    enemy.last_requests,
                    enemy.last_buttons
                );
                println!("    {}", report);
            }
        }
        let changes = entries.windows(2).filter(|w| w[0] != w[1]).count();
        println!("summary: {steps} steps, {changes} entry changes");
    }

    fn installed() -> &'static Scout {
        static DATA: std::sync::OnceLock<Scout> = std::sync::OnceLock::new();
        DATA.get_or_init(|| Scout::load(crate::install::dir()).expect("installed scout"))
    }
    /// [test] An idle enemy in its car form, on flat ground far from the origin, stays on that ground for
    /// 10 s. Before `FightWorld` forwarded `wheel_ray` and `vehicle_sweep`, the car found no ground and
    /// fell (work/ai_check4_1.log: the Shanghai mission's scouts sank after the vehicle entry).
    #[test]
    fn idle_enemy_in_car_form_stays_on_the_ground() {
        if crate::install::missing() { return; }
        let scout = installed();
        let t = player_tuning();
        let arena = Arena::default();
        let mut enemy = Enemy::new(7, Vec3::new(300.0, 300.0, 0.0), scout);
        for _ in 0..120 {
            sim::step(&mut enemy.state, &Input { vehicle: 1.0, ..Default::default() }, &t, &arena, sim::GAME_UPDATE);
        }
        assert!(enemy.state.is_vehicle(), "the car form is not reached");
        let player = State::new(&t);
        let (mut lowest, mut highest) = (f32::MAX, f32::MIN);
        for _ in 0..313 {
            enemy.step(scout, &player, &t, &arena, true, sim::GAME_UPDATE);
            lowest = lowest.min(enemy.state.pos.z);
            highest = highest.max(enemy.state.pos.z);
        }
        assert!(
            lowest >= -0.5 && highest <= 0.5,
            "z left the ground: {lowest} .. {highest}; vehicle {}, airborne {}",
            enemy.state.is_vehicle(),
            enemy.state.car_airborne()
        );
    }

    /// A new enemy has no mesh (straight paths), no tactics point and no shots: the report's defaults.
    #[test]
    fn a_fresh_enemy_reports_straight_paths_no_tactics_and_no_shots() {
        if crate::install::missing() { return; }
        let enemy = Enemy::new(9, Vec3::ZERO, installed());
        assert_eq!(enemy.path_kind(), "straight");
        assert!(enemy.tactics_point().is_none());
        assert_eq!(enemy.shots, 0);
        assert!(enemy.home.is_none());
    }
    /// [data] The team values the damage tuning reads (`damage::Tuning::read`, `team` as the enum's
    /// stored value). Printed, not asserted: the team relation is not read (ai-spawn-characters.md 2).
    #[test]
    fn team_values_of_the_spawn_and_player_characters() {
        let dir = crate::install::dir();
        if !dir.join("bnxglobal.str").is_file() {
            return;
        }
        let aprot = Scout::load_as(dir, "aprotscoutmp", "APrtScout").expect("aprot scout");
        let player = tuning::load(dir, "Bumblebee").expect("bumblebee tuning");
        let dcom = installed();
        println!("team values: APrtScout {}, Bumblebee {}, DComScout {}", aprot.tuning.damage.team, player.damage.team, dcom.tuning.damage.team);
    }
    fn player_tuning() -> Tuning {
        let dir = crate::install::dir();
        let data = character::load(dir, "bumblebee").unwrap();
        let mut t = tuning::load(dir, "Bumblebee").unwrap();
        t.locomotion = tuning::Locomotion::from_animations(&data.animations, &mut t.missing);
        t.actions = tuning::Actions::from_animations(&data.animations, &data.robot, &mut t.missing)
            .with_vehicle(data.vehicle.as_ref());
        t.weapon_set =
            tuning::RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
        t.rule_sets = tuning::RuleSet::all(&data.animations);
        t
    }
    #[test]
    fn installed_scout_fires_original_gun_and_can_kill_player() {
        if crate::install::missing() { return; }
        let scout = installed();
        let t = player_tuning();
        let mut player = State::new(&t);
        player.health = 4.0; // Two actual gun rounds must kill before an overheat recovery.
        let mut enemy = Enemy::new(1, Vec3::new(0.0, 35.0, 0.0), scout);
        let arena = Arena::default();
        let mut hits = 0;
        for _ in 0..3000 {
            for shot in enemy.step(scout, &player, &t, &arena, false, sim::GAME_UPDATE) {
                assert_eq!(shot.packet.flags, 0x200200);
                assert_eq!(shot.packet.amount, 2.0);
                if shot.target == Some(0) {
                    player.receive_damage(shot.packet, &t);
                    hits += 1;
                }
            }
            sim::step(&mut player, &Input::default(), &t, &arena, sim::GAME_UPDATE);
            if player.dead() {
                break;
            }
        }
        assert!(
            hits > 0,
            "state {:?}, gun {:?}",
            enemy.state.control_state(&scout.tuning),
            enemy.weapon_pose
        );
        assert!(player.dead(), "health {} after {hits} hits", player.health);
        for _ in 0..340 {
            sim::step(&mut player, &Input::default(), &t, &arena, sim::GAME_UPDATE);
        }
        assert!(player.restart_requested);
    }
    #[test]
    fn installed_scout_melee_is_a_real_clip_contact() {
        if crate::install::missing() { return; }
        let scout = installed();
        let t = player_tuning();
        let player = State::new(&t);
        let mut enemy = Enemy::new(1, Vec3::new(0.0, 5.0, 0.0), scout);
        let mut hits = Vec::new();
        for _ in 0..160 {
            enemy.step(
                scout,
                &player,
                &t,
                &Arena::default(),
                false,
                sim::GAME_UPDATE,
            );
            hits.extend(enemy.state.hits.iter().filter(|h| h.target == 0).cloned());
        }
        assert!(
            !hits.is_empty(),
            "state {:?}, pos {:?}",
            enemy.state.control_state(&scout.tuning),
            enemy.state.pos
        );
        assert!(
            hits.iter()
                .all(|h| h.damage > 0.0 && h.flags & flags::BY_MELEE != 0)
        );
    }
    #[test]
    fn emp_stuns_scout_break_budget_and_death_removes_after_clip() {
        if crate::install::missing() { return; }
        let scout = installed();
        let t = player_tuning();
        let player = State::new(&t);
        let mut enemy = Enemy::new(1, Vec3::new(0.0, 35.0, 0.0), scout);
        let emp = Packet {
            source: Some(player.actor),
            flags: flags::STUN,
            stun_time: 2.5,
            ..Default::default()
        };
        enemy.receive(emp, scout);
        for _ in 0..35 {
            assert!(
                enemy
                    .step(
                        scout,
                        &player,
                        &t,
                        &Arena::default(),
                        false,
                        sim::GAME_UPDATE
                    )
                    .is_empty()
            );
        }
        assert_eq!(enemy.state.control_state(&scout.tuning), Some("stateStun"));
        enemy.receive(
            Packet {
                amount: 80.0,
                flags: flags::NO_HIT_REACT,
                ..emp
            },
            scout,
        );
        assert!(enemy.state.damage_timers.stunned > 0.0);
        enemy.receive(
            Packet {
                amount: 0.1,
                flags: flags::NO_HIT_REACT,
                ..emp
            },
            scout,
        );
        assert_eq!(enemy.state.damage_timers.stunned, 0.0);
        enemy.receive(
            Packet {
                amount: 500.0,
                flags: flags::BY_EXPLOSION,
                ..emp
            },
            scout,
        );
        for _ in 0..340 {
            enemy.step(
                scout,
                &player,
                &t,
                &Arena::default(),
                true,
                sim::GAME_UPDATE,
            );
        }
        assert!(enemy.state.dead());
        assert!(enemy.state.restart_requested);
        assert_eq!(enemy.state.health, 0.0);
        assert_eq!(scout.tuning.base_health, 200.0);
        // No invented timed resurrection.
        for _ in 0..400 {
            enemy.step(
                scout,
                &player,
                &t,
                &Arena::default(),
                true,
                sim::GAME_UPDATE,
            );
        }
        assert_eq!(enemy.state.health, 0.0);
    }
    #[test]
    fn cylinder_trace_handles_vertical_and_inside_starts() {
        if crate::install::missing() { return; }
        let target = Enemy::new(1, Vec3::ZERO, installed()).target(&installed().tuning);
        assert_eq!(
            intersect_cylinder(Vec3::Z, Vec3::X * 100.0, &target),
            Some(0.0)
        );
        assert!(intersect_cylinder(Vec3::Z * 100.0, Vec3::ZERO, &target).is_some());
        assert!(
            intersect_cylinder(
                Vec3::new(100.0, 0.0, 100.0),
                Vec3::new(100.0, 0.0, -100.0),
                &target
            )
            .is_none()
        );
    }
    #[test]
    fn bumblebee_emp_does_not_shield_and_disable_blocks_transform() {
        if crate::install::missing() { return; }
        let scout = installed();
        let t = player_tuning();
        let mut player = State::new(&t);
        player.special_left = t.special.duration;
        let attacker = Enemy::new(1, Vec3::Y * 35.0, scout).state.actor;
        let result = player.receive_damage(
            Packet {
                source: Some(attacker),
                amount: t.base_health,
                ..Default::default()
            },
            &t,
        );
        assert_eq!(
            result.health, 0.0,
            "EMP must not grant Optimus's last-health-point protection"
        );
        let mut player = State::new(&t);
        player.receive_damage(
            Packet {
                source: Some(attacker),
                flags: flags::DISABLE_VEHICLE | flags::DISABLE_TRANSFORM,
                stun_time: 0.6,
                ..Default::default()
            },
            &t,
        );
        for _ in 0..15 {
            sim::step(
                &mut player,
                &Input {
                    vehicle: 1.0,
                    ..Default::default()
                },
                &t,
                &Arena::default(),
                0.032,
            );
            assert!(!player.is_vehicle());
        }
        for _ in 0..80 {
            sim::step(
                &mut player,
                &Input {
                    vehicle: 1.0,
                    ..Default::default()
                },
                &t,
                &Arena::default(),
                0.032,
            );
        }
        assert!(player.is_vehicle());
    }
    #[test]
    fn wall_contact_plays_installed_splat_recovery() {
        if crate::install::missing() { return; }
        let t = player_tuning();
        let mut player = State::new(&t);
        player.pos = Vec3::new(0.0, 7.5, 1.0);
        let arena = Arena {
            boxes: vec![sim::Box3 {
                min: Vec3::new(-10.0, 10.0, 0.0),
                max: Vec3::new(10.0, 12.0, 10.0),
            }],
            ..Default::default()
        };
        for _ in 0..4 {
            sim::step(&mut player, &Input::default(), &t, &arena, 0.032);
        }
        player.receive_damage(
            Packet {
                source: Some(Actor {
                    id: 1,
                    team: installed().tuning.damage.team,
                    ..Default::default()
                }),
                amount: 10.0,
                flags: flags::BY_FAST_FLAIL,
                direction: Vec3::Y,
                knockback_speed: 30.0,
                knockback_angle: 0.1,
                ..Default::default()
            },
            &t,
        );
        let mut splat = false;
        let mut recovery = false;
        for _ in 0..250 {
            sim::step(&mut player, &Input::default(), &t, &arena, 0.032);
            splat |= player.damage_type == 0x5060153e;
            recovery |= player
                .action_clip(&t)
                .is_some_and(|(_, id, _)| id == "Recover_WallSplat")
                || player
                    .base_clip(&t)
                    .is_some_and(|c| c.id == "Recover_WallSplat");
        }
        assert!(
            splat && recovery,
            "wall must select its real recovery clip; splat{splat} recovery{recovery}, pos {:?}, kind {:x}, state {:?}, base {:?}",
            player.pos,
            player.damage_type,
            player.control_state(&t),
            player.base
        );
        assert_ne!(player.control_state(&t), Some("stateHitReact"));
    }
    #[test]
    fn slow_changes_motion_and_weapon_cadence_and_heat_then_expires() {
        if crate::install::missing() { return; }
        let scout = installed();
        let t = player_tuning();
        let mut a = State::new(&t);
        let mut b = a.clone();
        b.receive_damage(
            Packet {
                source: Some(Actor {
                    id: 1,
                    team: scout.tuning.damage.team,
                    ..Default::default()
                }),
                flags: flags::SLOW | flags::NO_HIT_REACT,
                slow_time: 2.0,
                ..Default::default()
            },
            &t,
        );
        for _ in 0..30 {
            for s in [&mut a, &mut b] {
                sim::step(
                    s,
                    &Input {
                        stick: Vec2::Y,
                        ..Default::default()
                    },
                    &t,
                    &Arena::default(),
                    0.032,
                );
            }
        }
        assert!(
            b.pos.y < a.pos.y * 0.65 && b.pos.y > a.pos.y * 0.3,
            "slow {} vs {}",
            b.pos.y,
            a.pos.y
        );
        let mut gun_a = Weapon::default();
        let mut gun_b = Weapon::default();
        let (mut n, mut slowed) = (0, 0);
        for _ in 0..50 {
            if matches!(
                gun_a.step_status(&scout.gun, true, false, &t.damage, 0.032),
                Fired::Rounds(_)
            ) {
                n += 1;
            }
            if matches!(
                gun_b.step_status(&scout.gun, true, true, &t.damage, 0.032),
                Fired::Rounds(_)
            ) {
                slowed += 1;
            }
        }
        assert!(slowed < n && gun_b.heat < gun_a.heat);
        for _ in 0..65 {
            sim::step(&mut b, &Input::default(), &t, &Arena::default(), 0.032);
        }
        assert_eq!(b.damage_timers.slowed, 0.0);
        b.receive_damage(
            Packet {
                source: Some(Actor {
                    id: 1,
                    team: scout.tuning.damage.team,
                    ..Default::default()
                }),
                flags: flags::OVERHEAT_ALL,
                heat: 100.0,
                ..Default::default()
            },
            &t,
        );
        assert_eq!(b.heat_damage, vec![(flags::OVERHEAT_ALL, 100.0)]);
    }
    #[test]
    fn difficulty_data_is_used_and_explosion_death_keeps_vertical_root_motion() {
        if crate::install::missing() { return; }
        let t = player_tuning();
        assert_eq!(
            t.damage.difficulties.map(|d| d.enemy_health),
            [0.5, 1.0, 1.5, 2.0]
        );
        let mut player = State::new(&t);
        player.receive_damage(
            Packet {
                source: Some(Actor {
                    id: 1,
                    team: installed().tuning.damage.team,
                    ..Default::default()
                }),
                amount: 500.0,
                flags: flags::BY_EXPLOSION,
                ..Default::default()
            },
            &t,
        );
        let arena = Arena::default();
        sim::step(&mut player, &Input::default(), &t, &arena, 0.032);
        assert!(player.death.as_ref().unwrap().root_z);
        let mut z = 0.0f32;
        for _ in 0..8 {
            let death = player.death.as_ref().unwrap();
            let old = death.tick;
            let clip = death.clip;
            sim::step(&mut player, &Input::default(), &t, &arena, 0.032);
            let death = player.death.as_ref().unwrap();
            assert_eq!(death.clip, clip);
            let moved =
                t.actions.clips[&clip].root_at(death.tick) - t.actions.clips[&clip].root_at(old);
            z = moved.z.max(0.0).max(z);
            assert!(
                (player.vel.z - moved.z / 0.032).abs() < 1e-4,
                "explosion death must use animated Z, not gravity"
            );
        }
        assert!(z > 0.0, "installed explosion clip has vertical travel");
    }
}
