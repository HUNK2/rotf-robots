//! Devastator as a player: the walking giant of the Cairo City boss mission, made playable.
//!
//! The game never lets anyone play him. His machine (`stateMachineDevastator`) is driven by his
//! brain and the mission script (`brainDevastator`, `goalDevastatorBuildingAttack`), which send
//! it messages: `TurnLeft` / `TurnRight`, `DevastatorAttack` (the hydraulic pound) and
//! `AttackSpecial` (the vortex). [data global_states.txt 6484] This module is the port's
//! player binding in their place: the pad's buttons and the stick become those messages.
//! Everything else he does (the clips, their sounds, shakes and effects, the walk, the
//! shockwaves his pound's Trigger events set off) is his own data. `docs/devastator.md`.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_4;

use glam::{Quat, Vec2, Vec3};

use crate::control::{Graph, Test};
use crate::formats::anim::{Action, AnimSet, Channel, Clip, Track, ROOT_CHANNEL, TICKS_PER_SECOND};
use crate::formats::hash::crc32;
use crate::footik::{FootRig, Leg, PROBE};
use crate::formats::model::Library;
use crate::formats::scene::ObjectDef;
use crate::pose::{affine, Rig};
use crate::shockwave::Def;
use crate::tuning::{Actions, CameraTuning, Tuning};

/// How far the stick must be pushed before a push off his facing turns him on the spot. [assumed]
/// The brain's turn test is not read; half the stick's travel keeps a resting thumb from turning
/// him. Settles: `brainDevastator`'s goal code, or play.
pub const TURN_STICK: f32 = 0.5;
/// How far off his facing the stick must point to turn him on the spot rather than walk (his
/// walk turns him at `modeMove.turnSpeed`). [assumed] His turn clips each turn him 90 degrees
/// [data `devastator_clips`], so a push under half of that walks. Settles: as above.
pub const TURN_ANGLE: f32 = FRAC_PI_4;
/// The node his clips are re-rooted on (`reroot`): the middle of his spine, the point his turns
/// pivot about. [assumed port reference point] Over `TurnLeft` it ends 3.5 m from where it
/// started while the root crosses 22.5 m of his body; the hips swing 28 m. [data
/// `devastator_clips --turn TurnLeft --body`] Settles: nothing in the game, which never follows
/// him with a camera; play.
pub const CENTRE: &str = "Spine_03";
/// The height the port's camera numbers are made for, against which his own is scaled: an
/// ordinary robot's. [stand-in] the game has no player camera for him (`cameraDevastator` does not
/// exist); his model is 60.6 m tall.
const ORDINARY_HEIGHT: f32 = 6.0;
/// Seconds he blends into his stand over (`fit`). [assumed port design] About a third of his
/// walk's stride (four limbs in 4 s). Settles: play.
pub const IDLE_BLEND: f32 = 0.6;

/// One of his `ShockWave` attachments.
#[derive(Clone, Debug)]
pub struct Wave {
    pub def: Def,
    /// The name hash of the node it sits on: the drawn model's place for it.
    pub node: u32,
    /// That node's place at each whole tick of each clip whose Trigger events name it, in his own
    /// frame (keyed by the clip's name hash): the place when nothing is drawn.
    pub centres: HashMap<u32, Vec<Vec3>>,
}

impl Wave {
    pub fn centre(&self, clip: u32, tick: f32) -> Option<Vec3> {
        let centres = self.centres.get(&clip)?;
        centres.get((tick.max(0.0).round() as usize).min(centres.len().checked_sub(1)?)).copied()
    }
}

/// The player binding: the message each button sends, and his shockwaves.
#[derive(Clone, Debug, Default)]
pub struct Giant {
    pub attack: Option<u32>,
    pub special: Option<u32>,
    pub turn_left: Option<u32>,
    pub turn_right: Option<u32>,
    pub waves: Vec<Wave>,
    /// The port's planting of his legs and arms (`limbs`); the game has none on him.
    pub limbs: Option<FootRig>,
    /// The root motion of every clip of his, by the CRC-32 of its set's name and its own: the
    /// base layer's clips (`IdleStandSet`, `WalkSet`) move him by it (`sim`), as the actions'
    /// clips do. [assumed port design] see `roots`.
    pub roots: HashMap<(u32, u32), Root>,
    /// Radians a second his walk turns him toward the stick: the rate his own turn clips turn
    /// him at (`TurnLeft`, 90 degrees in 4 s). [assumed port design] his `modeMove.turnSpeed` is
    /// 360 degrees a second [data], the shared robot walk's, which spins a 60 m body with every
    /// limb sliding; his brain walks him only along its waypoints. Settles: play.
    pub walk_turn: f32,
    /// His vortex (`vortex`), when the mission's funnel was found.
    pub vortex: Option<Vortex>,
    /// How high his body's cylinder starts above his feet (`body`): what is lower he strides
    /// over, his hands and feet planted on it by the limb IK. 0 when his model gave no ankles.
    pub stride: f32,
    /// His guns (`read_pods`): empty when the boss mission's pods were not read.
    pub pods: Vec<Pods>,
    /// His smoke, sparks, bursts and fire (`read_dressing`).
    pub dressing: Dressing,
}

/// A clip's root motion: where the root is at each tick in its frame at the start
/// (`Clip::root_path`), and how far it has turned (`Clip::root_turn`).
#[derive(Clone, Debug, Default)]
pub struct Root {
    pub path: Vec<Vec3>,
    pub turn: Vec<f32>,
    pub looping: bool,
    /// The ticks a hand or foot of his lands at: the clip's effects on `Hand_L`, `Foot_R`,
    /// `Hand_R`, `Foot_L` (his walk's at 42, 94, 170, 212: the dust at each footfall). [data]
    pub steps: Vec<f32>,
}

/// The nodes he walks on (`limbs`), whose effects in a clip are the dust of a landing.
const LIMBS: [&[u8]; 4] = [b"Foot_L", b"Foot_R", b"Hand_L", b"Hand_R"];

/// The shake his landing limbs start, at his place. [assumed port design] His clips have none on
/// the walk (the boss is never followed by a camera); this is the one the robots' plain jump
/// landing starts (`docs/camera.md` "Shakes"), 47 m full and 90 m gone, so his scaled camera
/// feels each step less the further out it is. Settles: play.
pub const FOOTFALL_SHAKE: u32 = crc32(b"SoftCameraShake");

/// [assumed port design] His health while played. First 6400; balanced 2026-10-23 (the user: "he's
/// pretty weak"): three scouts took 1800 of the 6400 in 28 s (64 a second: every 6.8 round lands on
/// a 60 m target), so he fell in under two minutes. With `GIANT_ARMOUR` the same squad takes 26 a
/// second, about 7.5 minutes for the 12000; Megatron has 3600 and Optimus 3000 [data].
pub const GIANT_HEALTH: f32 = 12000.0;
/// [assumed port design] The share of every hit he takes: his plating shrugs off small arms (a
/// scout's 6.8 round does 2.7).
pub const GIANT_ARMOUR: f32 = 0.4;
/// [assumed port design] His pound's rings' damage (the boss's is 25 [data]) and his vortex's
/// KillBox's (50 [data]): a scout has 225 [data], so a ring reaching one takes two thirds of it and
/// the funnel kills in two touches.
pub const POUND_DAMAGE: f32 = 150.0;
pub const VORTEX_DAMAGE: f32 = 120.0;

/// [assumed port design] His pods' numbers while played (the boss's [data] were set for an AI
/// pecking at a small player): per kind (`Pods::kind`) the round's damage, its blast's radius and
/// damage (none for the gatling), and the seconds between firings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PodBalance {
    pub damage: f32,
    pub blast_radius: f32,
    pub blast_damage: f32,
    pub interval: f32,
}

/// The data's: gatling 2 a round every 0.05 s; missiles 10, blast 7 m 20, every 3 s; bug bombs 15,
/// blast 5 m 32, every 0.075 s (held, a dozen a pull every 0.075 s rained hundreds). Now: the
/// gatling 9 a round (180 a second: a scout in 1.3 s); a missile volley every 2 s, each blast 15 m
/// and 140 (a scout in two); an eruption every 1.2 s, each of its 12 bombs a 12 m blast of 100.
pub const POD_BALANCE: [PodBalance; 3] = [
    PodBalance { damage: 9.0, blast_radius: 0.0, blast_damage: 0.0, interval: 0.05 },
    PodBalance { damage: 60.0, blast_radius: 15.0, blast_damage: 140.0, interval: 2.0 },
    PodBalance { damage: 40.0, blast_radius: 12.0, blast_damage: 100.0, interval: 1.2 },
];

/// A hit as his armour lets it through (`GIANT_ARMOUR`). [assumed port design]
pub fn armour(mut packet: crate::damage::Packet) -> crate::damage::Packet {
    packet.amount *= GIANT_ARMOUR;
    packet
}

/// [assumed port design] Sounds of the boss mission's bank [data] that nothing of his cues, given
/// to him (the audit of 2026-10-23, `docs/devastator.md`): the gatling's overheat (the pods' data
/// has none; the bank's heavy machine gun's), his blasts sized for him (the data's missiles end in
/// `WPN_MIS_SM_EXPLO`, the bombs in `EXP_OPTIMUS_MISSLE`), the eruption's launch, a pod blowing, its
/// sparks, his roar (`DEV_ROAR`, whose sound is `/ANIM_DEVA_ROAR_02`'s four samples [data]; only his
/// stagger played it), and the death cinematic's sound bed over his fall.
pub const OVERHEAT_SOUND: u32 = crc32(b"WPN_HMG_OVERHEAT");
pub const MISSILE_BLAST_SOUND: u32 = crc32(b"EXP_HUGE");
pub const BOMB_BLAST_SOUND: u32 = crc32(b"EXP_LRG");
pub const ERUPTION_SOUND: u32 = crc32(b"EXP_FIREBALL");
pub const POD_BLOW_SOUND: u32 = crc32(b"EXP_LRG");
pub const SPARK_SOUND: u32 = crc32(b"DEST_PHONELINE_SPARK");
pub const ROAR_SOUND: u32 = crc32(b"DEV_ROAR");
pub const DEATH_SOUND: u32 = crc32(b"Devastator_Death_Cinematic_fx");
/// [assumed port design] His ambience (the user, 2026-10-23: "he needs some ambience"); the bank has
/// no idle loop of his own, so a body of construction trucks gets: a diesel truck's engine
/// (`VEH_IRON_TRUCK_RPM`, rpm 0..8000 and load [data]) kept on him, idling at `ENGINE_RPM[0]` and
/// rising to `ENGINE_RPM[1]` with his walk and his attacks; the wind at his height
/// (`ANIM_SHARED_WIND_LP` at `WIND_SPEED` of its 0..35); and now and then while he stands, a
/// hydraulic lift of his own or an air hiss (`IDLE_CREAKS`), every `IDLE_CREAK_EVERY` seconds.
pub const ENGINE_SOUND: u32 = crc32(b"VEH_IRON_TRUCK_RPM");
pub const ENGINE_RPM: [f32; 2] = [700.0, 2400.0];
pub const WIND_SOUND: u32 = crc32(b"ANIM_SHARED_WIND_LP");
pub const WIND_SPEED: f32 = 14.0;
pub const IDLE_CREAKS: [u32; 3] = [crc32(b"ANIM_DEVA_HYD_LIFT_A"), crc32(b"ANIM_DEVA_HYD_LIFT_B"), crc32(b"DEST_AIR_HISS")];
pub const IDLE_CREAK_EVERY: [f32; 2] = [3.0, 8.0];
/// [assumed port design] His roar beyond his staggers (the user: more of it "as he's firing and using
/// his abilities"): at the windup of his pound and his vortex, and at the start of a burst of fire
/// (a firing after `ROAR_BURST_GAP` seconds without one) with chance `ROAR_FIRE_CHANCE`; never within
/// `ROAR_REST` seconds of the last, except his breaking pods' and his death's.
pub const ROAR_REST: f32 = 5.0;
pub const ROAR_BURST_GAP: f32 = 1.5;
pub const ROAR_FIRE_CHANCE: f32 = 0.6;

/// [assumed port design] His switch of pod kind: a Decepticon's servo fidget (the boss bank's) as his
/// pods turn over. And the shakes his blasts start where they land (scaled to his camera, see
/// `scale_cameras`): a bomb's soft, a missile's medium.
pub const SWITCH_SOUND: u32 = crc32(b"ANIM_DECPTICON_SRVO_FDG");
pub const BOMB_BLAST_SHAKE: u32 = crc32(b"SoftCameraShake");
pub const MISSILE_BLAST_SHAKE: u32 = crc32(b"MediumCameraShake");

pub const SOUNDS: [u32; 15] = [
    SWITCH_SOUND,
    OVERHEAT_SOUND, MISSILE_BLAST_SOUND, BOMB_BLAST_SOUND, ERUPTION_SOUND, SPARK_SOUND, ROAR_SOUND, DEATH_SOUND,
    crc32(b"FIRE_LG"), ENGINE_SOUND, WIND_SOUND, IDLE_CREAKS[0], IDLE_CREAKS[1], IDLE_CREAKS[2], crc32(b"DEVA_REAC_EXERT"),
];
/// [assumed port design] The eruption's kick: a shake as the dozen bombs leave.
pub const ERUPTION_SHAKE: u32 = crc32(b"MediumCameraShake");
/// [assumed port design] A pod blowing: the hard shake of a stagger.
pub const POD_BLOW_SHAKE: u32 = crc32(b"MedHardCameraShake");

/// The boss mission's effects that dress him [data: their names and templates; assumed port design:
/// where and when]: each pod's own damage smoke (`smk_deva_gunpod`, its `DMGSMOKE`
/// TriggerableParticle on node 1, an endless 3-a-second smoke), its spark shower (`dest_sparkshower`,
/// `DMGFX` on node 10, a burst), the burst of its `DamageLink`'s broken object as it is destroyed
/// (`r_devastator_damaged`: flash, fireball, ring, sparks), and his animation set's burning plume
/// (`smk_plumesmokelarge`, a 60 s fire and smoke).
#[derive(Clone, Debug, Default)]
pub struct Dressing {
    pub pod_smoke: Option<crate::particles::Template>,
    pub sparks: Option<crate::particles::Template>,
    pub pod_blast: Option<crate::particles::Template>,
    pub plume: Option<crate::particles::Template>,
}

/// The pod nodes his pod smoke and sparks sit on (the attachments' `index`). [data]
pub const POD_SMOKE_NODE: usize = 1;
pub const POD_SPARK_NODE: usize = 10;

/// The node of a pod of `kind` that its effect on `node` sits on: the gatling and missile pods carry
/// the smoke and sparks there [data]; the bug-bomb launchers have neither, and their four nodes past
/// the root are launch dummies 20 to 40 m out in front of him (measured 2026-10-23: pod 7's node 3 sat
/// 41 m ahead and 11 m up, in the air), so theirs go on the launcher's root. [assumed port design]
pub fn pod_effect_node(kind: usize, node: usize) -> usize {
    if kind == BUG_BOMB { 0 } else { node }
}

/// Reads `Dressing` out of his pack (the boss mission's), each template by its name.
pub fn read_dressing(game_dir: &std::path::Path) -> Dressing {
    use crate::formats::lxb::DataFile;
    use crate::formats::pack::{self, Pack};
    let mut out = Dressing::default();
    let Ok(pack) = Pack::open(&crate::formats::characterlist::pack_file(game_dir, "devastator")) else { return out };
    for chunk in pack.of_type(pack::DATA) {
        let Ok(file) = DataFile::parse(pack.data(chunk).to_vec()) else { continue };
        let find = |name: &str| {
            find_deep(file.root(), crc32(name.as_bytes()), &mut std::collections::HashSet::new())
                .and_then(crate::particles::Template::read)
        };
        for (slot, name) in [
            (&mut out.pod_smoke, "smk_deva_gunpod"),
            (&mut out.sparks, "dest_sparkshower"),
            (&mut out.pod_blast, "r_devastator_damaged"),
            (&mut out.plume, "smk_plumesmokelarge"),
        ] {
            if slot.is_none() {
                *slot = find(name);
            }
        }
    }
    out
}

/// [assumed port design] His damage shows as his pods breaking, as the boss's do (each pod is a
/// 500-strength Destructable that smokes and sparks when hit and bursts into its broken object
/// [data]; the port does not hurt his pods apart from him): below each share of his health in
/// `DAMAGE_STAGES` another third of his pods blow and smoke from then on, and below the last he
/// burns. He mends (`GIANT_REGEN_DELAY`), so a stage is left only `DAMAGE_MEND` above its share.
pub const DAMAGE_STAGES: [f32; 3] = [0.75, 0.5, 0.25];
pub const DAMAGE_MEND: f32 = 0.05;

/// The damage stage (how many of `DAMAGE_STAGES` he is under) for his health's share `health`,
/// from the stage he was at.
pub fn damage_stage(health: f32, was: usize) -> usize {
    let mut stage = DAMAGE_STAGES.iter().filter(|&&share| health < share).count();
    while stage < was && health < DAMAGE_STAGES[stage] + DAMAGE_MEND {
        stage += 1;
    }
    stage
}

/// The stage at which pod `index` breaks (1 to `DAMAGE_STAGES.len()`): the pods taken in turns,
/// so each stage breaks some of every kind.
pub fn pod_breaks_at(index: usize) -> usize {
    index % DAMAGE_STAGES.len() + 1
}

/// [assumed port design] Where his missiles come down: the aim's point, or, when the aim's line
/// meets nothing within `MISSILE_REACH` (from his 50 m eye a level look never meets the ground),
/// the ground under the line at that reach. His missiles always land.
pub const MISSILE_REACH: f32 = 350.0;

pub fn missile_point(eye: Vec3, forward: Vec3, aim: Vec3, ground: f32) -> Vec3 {
    if (aim - eye).length() <= MISSILE_REACH {
        return aim;
    }
    let far = eye + forward.normalize_or(Vec3::Y) * MISSILE_REACH;
    Vec3::new(far.x, far.y, far.z.min(ground))
}
/// [assumed port design] Seconds unhurt before he starts to mend (Bumblebee's is 1.5 [data]).
const GIANT_REGEN_DELAY: f32 = 4.0;
/// [assumed port design] The share of his health that, taken within about `STAGGER_MEMORY`
/// seconds, staggers him; one is held off for `STAGGER_REST` after another.
pub const STAGGER_SHARE: f32 = 0.025;
const STAGGER_MEMORY: f32 = 1.5;
const STAGGER_REST: f32 = 2.5;
/// [assumed port design] The sway a stagger sets going about his feet: the lean's top
/// (radians), its swing's frequency (rad/s, a slow 60 m body) and damping ratio.
const STAGGER_LEAN: f32 = 0.045;
const SWAY_FREQUENCY: f32 = 2.4;
const SWAY_DAMPING: f32 = 0.35;
/// [assumed port design] What a stagger adds to its clip, whose own cues carry the heave and the
/// steps (`ANIM_DEVA_LEAN`, `_FALL`, `_POUND`, `_STEP_LAND` [data]): his grunt (`DEVA_REAC_EXERT`,
/// the boss mission's [data], which only his pound cues) and a hard shake. When only the body
/// sways (`Stagger::kick`) the heave plays too (`STAGGER_SWAY_SOUND`).
pub const STAGGER_SOUND: u32 = crc32(b"DEVA_REAC_EXERT");
pub const STAGGER_SWAY_SOUND: u32 = crc32(b"ANIM_DEVA_LEAN");
pub const STAGGER_SHAKE: u32 = crc32(b"MedHardCameraShake");
/// [assumed port design] His fall (`DeathSet/Death`, 720 ticks, which cues `ANIM_DEVA_FALL` at
/// 274 and no shake [data]): the ticks where its dust bursts (the 78.9 m effect template) say a
/// part of him meets the ground, each with a shake; the last with the building-collapse one.
pub const FALL_IMPACTS: [(f32, u32); 3] = [
    (178.0, crc32(b"MassiveCameraShake")),
    (296.0, crc32(b"MassiveCameraShake")),
    (300.0, crc32(b"BuildingCollapseCameraShake")),
];

/// [assumed port design] His answer to being hit, since he has no hit reacts of his own: the
/// damage of the last moments adds up, and past `STAGGER_SHARE` of his health he staggers,
/// leaning away from the push and swaying back on a slow damped spring. The lean is (pitch,
/// roll) in radians about his feet: pitch lifts his front (back from a push from ahead), roll
/// lifts his left (over to the right).
#[derive(Clone, Copy, Debug, Default)]
pub struct Stagger {
    pressure: f32,
    rest: f32,
    pub lean: Vec2,
    sway: Vec2,
    /// How many staggers only swayed him (`kick`), for the game's sound.
    pub kicks: u32,
}

impl Stagger {
    /// One frame: `hit` is the damage taken since the last. True when he staggers now.
    pub fn step(&mut self, hit: f32, maximum: f32, dt: f32) -> bool {
        self.pressure = self.pressure * (-dt / STAGGER_MEMORY).exp() + hit.max(0.0);
        self.rest = (self.rest - dt).max(0.0);
        let staggered = self.rest <= 0.0 && maximum > 0.0 && self.pressure >= maximum * STAGGER_SHARE;
        if staggered {
            self.pressure = 0.0;
            self.rest = STAGGER_REST;
        }
        let spring = -self.lean * SWAY_FREQUENCY * SWAY_FREQUENCY - self.sway * 2.0 * SWAY_DAMPING * SWAY_FREQUENCY;
        self.sway += spring * dt;
        self.lean += self.sway * dt;
        staggered
    }

    /// Sets the sway going away from `push` (the last hit's way, world) for `yaw`: for a
    /// stagger whose clip cannot play (he is in his pound or his vortex), so the body still
    /// gives under it. A kick whose free swing peaks at about `STAGGER_LEAN`.
    pub fn kick(&mut self, push: Vec3, yaw: f32) {
        let (pushed_ahead, pushed_right) = local_push(push, yaw);
        self.sway += Vec2::new(-pushed_ahead, pushed_right) * STAGGER_LEAN * SWAY_FREQUENCY;
        self.kicks = self.kicks.wrapping_add(1);
    }
}

/// The way a hit pushes him, in his own frame: (ahead, right), a unit length; straight back
/// when the hit carries no direction.
fn local_push(push: Vec3, yaw: f32) -> (f32, f32) {
    let forward = Vec2::new(-yaw.sin(), yaw.cos());
    let right = Vec2::new(yaw.cos(), yaw.sin());
    let flat = push.truncate().normalize_or(-forward);
    (flat.dot(forward), flat.dot(right))
}

/// [assumed port design] Which of his four hit reacts a stagger plays [data: `PlayAnimSet`
/// `HitReactMissilePod`, `HitReactMissilePodLt`, `HitReactArmLt`, `HitReactArmRt`; the game's
/// brain picks one by the part shot, which the port does not track]: by the side the push comes
/// from. From ahead he reels back (`HitReactMissilePod`, which carries him 17.6 m back); from his
/// left or right the arm on that side; from behind the left pod's.
pub fn react_clip(push: Vec3, yaw: f32) -> u32 {
    let (ahead, right) = local_push(push, yaw);
    if ahead <= -right.abs() {
        crc32(b"HitReactMissilePod")
    } else if right >= ahead.abs() {
        crc32(b"HitReactArmLt")
    } else if -right >= ahead.abs() {
        crc32(b"HitReactArmRt")
    } else {
        crc32(b"HitReactMissilePodLt")
    }
}

/// What `sim` keeps of his root motion between steps: the base clip (serial) and tick it was last
/// taken at and the velocity it gave, and the velocity of the clip before the playing one, which
/// fades over the playing one's blend (`left` of `blend` seconds).
#[derive(Clone, Copy, Debug, Default)]
pub struct RootMotion {
    pub serial: u32,
    pub tick: f32,
    pub velocity: Vec2,
    pub carry: Vec2,
    pub left: f32,
    pub blend: f32,
}

impl Root {
    pub fn new(clip: &Clip, looping: bool) -> Self {
        let limbs = LIMBS.map(crc32);
        let steps = clip.effect_events.iter().filter(|e| limbs.contains(&e.node)).map(|e| e.time).collect();
        Self { path: clip.root_path(), turn: clip.root_turn(), looping, steps }
    }

    /// Whether a limb lands after tick `from` and by tick `to` (a loop's `to` before `from`: it
    /// came round).
    pub fn lands(&self, from: f32, to: f32) -> bool {
        self.steps.iter().any(|&t| if to >= from { t > from && t <= to } else { self.looping && (t > from || t <= to) })
    }

    /// The clip's length in ticks.
    pub fn ticks(&self) -> f32 {
        self.path.len().saturating_sub(1) as f32
    }

    fn at(&self, tick: f32) -> (Vec3, f32) {
        let Some(last) = self.path.len().checked_sub(1) else { return (Vec3::ZERO, 0.0) };
        let tick = tick.clamp(0.0, last as f32);
        let from = tick.floor() as usize;
        let (to, f) = ((from + 1).min(last), tick - from as f32);
        let turn = |i: usize| self.turn.get(i).copied().unwrap_or(0.0);
        (self.path[from].lerp(self.path[to], f), turn(from) + (turn(to) - turn(from)) * f)
    }

    /// The root's travel from tick `from` to tick `to`, in the frame it had at `from` (x right,
    /// y forward, z up), and how far it turned (radians toward -x, as `sim`'s yaw). A loop whose
    /// `to` is before `from` came round: through its end and on from its start.
    pub fn travel(&self, from: f32, to: f32) -> (Vec3, f32) {
        let span = |a: f32, b: f32| {
            let ((pa, ta), (pb, tb)) = (self.at(a), self.at(b));
            (Quat::from_rotation_z(-ta) * (pb - pa), tb - ta)
        };
        if to < from && self.looping {
            let (first, turned) = span(from, self.ticks());
            let (then, more) = span(0.0, to);
            return (first + Quat::from_rotation_z(turned) * then, turned + more);
        }
        span(from, to.max(from))
    }
}

/// The boss mission's vortex: a placed `VortexObj` Instance the mission's script triggers, not a
/// part of him. Its object carries the funnel model (`Devastator_Vortex.Vortex_OfficialVersion`, a
/// tube 2.02 long along its +y, pointed at y = 0, up to 0.67 across), the funnel's own 520-tick
/// clip, a `KillBox` on the funnel's hull (damage 50, DAMAGE_BY_PLAYER | EXPLOSION, knock-back 15
/// at 35 degrees), and a `SoundPlayer` a `ForwardTrigger` starts 5 s after the trigger. [data
/// `work\devastator\mission_data_1.txt` 544905, `devastator_clips --vortex`]
pub struct VortexDef {
    pub object: std::sync::Arc<ObjectDef>,
    pub clip: Option<Clip>,
    pub damage: f32,
    /// The KillBox's `damageType`, raw.
    pub flags: u32,
    pub knock_back_speed: f32,
    pub knock_back_angle: f32,
    /// The `SoundPlayer`'s event id (0: none), and the `ForwardTrigger`'s delay before it starts.
    pub sound: u32,
    pub sound_delay: f32,
}

/// His vortex as the port plays it: the mission's funnel (`VortexDef`) put on him. [assumed port
/// design] In the game the funnel is a placed object the script triggers; here it starts when his
/// own `AttackSpecial` starts drawing dust into his head (the clip's first effect on `Head`, tick
/// 86 [data]) and plays its own clip from there (520 ticks: it grows from 1 to 50 times its size
/// by tick 40, spins, and shrinks back from tick 460 [data]). Its point sits on his head and it
/// runs along the head's +y, the line the clip's dust comes in along from 100 m out [data: the
/// effect's source disc at (20, 100, 0) in the head's frame], so at 50 times it reaches as far.
#[derive(Clone, Debug, Default)]
pub struct Vortex {
    /// The clip it plays in (by its name's CRC-32) and the tick it starts at.
    pub clip: u32,
    pub start: f32,
    /// His head: its node's name hash, and its place and turn in his own frame at each whole tick
    /// of that clip (as `Wave::centres`).
    pub head_node: u32,
    pub head: Vec<(Vec3, Quat)>,
    /// The funnel's own clip at each whole tick: its scale and its spin about its axis (its +y).
    pub scale: Vec<f32>,
    pub spin: Vec<Quat>,
    /// The funnel model: its name, its length along +y and its radius at each tenth of that, in
    /// model units (times `scale` in the world). [data `devastator_clips --vortex`]
    pub model: String,
    pub length: f32,
    pub profile: Vec<f32>,
    /// What its KillBox hands what it touches. [data]
    pub damage: f32,
    pub flags: u32,
    pub knock_back_speed: f32,
    pub knock_back_angle: f32,
    pub sound: u32,
    pub sound_delay: f32,
    /// The mission's funnel object, for the host to draw (`Funnel` places its render node).
    pub object: Option<VortexObject>,
}

/// `VortexDef::object`, shared with the host.
#[derive(Clone)]
pub struct VortexObject(pub std::sync::Arc<ObjectDef>);

impl std::fmt::Debug for VortexObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VortexObject({} nodes)", self.0.nodes.len())
    }
}

/// The funnel where it stands at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Funnel {
    /// Its point (on his head), its turn (its +y the axis, with its spin), and its scale.
    pub tip: Vec3,
    pub turn: Quat,
    pub scale: f32,
    /// The tick of its own clip.
    pub tick: f32,
}

impl Funnel {
    pub fn axis(&self) -> Vec3 {
        self.turn * Vec3::Y
    }

    /// Where a ball of `radius` at `point` is in the funnel: how far along it (0 at the point, 1
    /// at the open end) and how far off its axis; None when it is not inside.
    pub fn holds(&self, vortex: &Vortex, point: Vec3, radius: f32) -> Option<(f32, f32)> {
        let length = vortex.length * self.scale;
        let offset = point - self.tip;
        let along = offset.dot(self.axis());
        if length <= 0.0 || along > length {
            return None;
        }
        let off = (offset - self.axis() * along).length();
        // Within a tenth of its length of the point, on any side (the head moves on from what
        // the drag brought in): held there. [assumed port design]
        if offset.length() <= radius + length * 0.1 {
            return Some(((along / length).max(0.0), off));
        }
        if along < 0.0 {
            return None;
        }
        let at = along / length;
        let wide = vortex.radius(at) * self.scale;
        (off <= wide + radius).then_some((at, off))
    }
}

impl Vortex {
    /// The funnel's radius in model units `at` a part of its length (0 its point, 1 its open end):
    /// from nothing at the point out to the widest its model gets by then, straight between the
    /// middles of the tenths `profile` measures. [stand-in] for its KillBox's hull (14 points, convex,
    /// not read): the model is drawn with the same scale and spin, so what it hits is what is seen.
    pub fn radius(&self, at: f32) -> f32 {
        let bands = self.profile.len() as f32;
        let mut knots = vec![(0.0, 0.0)];
        let mut widest = 0.0f32;
        for (i, &r) in self.profile.iter().enumerate() {
            widest = widest.max(r);
            // A tenth with no ring in it (the first holds only the point's vertex) is no knot.
            if widest > 0.0 {
                knots.push(((i as f32 + 0.5) / bands, widest));
            }
        }
        let at = at.clamp(0.0, 1.0);
        knots.windows(2).find(|w| at <= w[1].0).map_or(widest, |w| w[0].1 + (w[1].1 - w[0].1) * (at - w[0].0) / (w[1].0 - w[0].0))
    }

    /// The funnel at his clip's `tick`, with him placed at `placed`: None outside its time.
    pub fn at(&self, placed: glam::Affine3A, clip: u32, tick: f32) -> Option<Funnel> {
        let own = tick - self.start;
        let last = self.scale.len().checked_sub(1)?;
        if clip != self.clip || !(0.0..=last as f32).contains(&own) {
            return None;
        }
        let i = (own.round() as usize).min(last);
        let (head, turn) = self.head.get((tick.round().max(0.0) as usize).min(self.head.len().checked_sub(1)?)).copied()?;
        let (_, placed_turn, _) = placed.to_scale_rotation_translation();
        // Level, the way the head looks: the mission places its funnel level (identity turn, on
        // the ground [data]); along the head's own +y, 25 degrees down, most of it is underground.
        let looks = (placed_turn * turn * Vec3::Y).truncate();
        let looks = if looks.length_squared() > 1e-6 { looks.normalize() } else { (placed_turn * Vec3::Y).truncate().normalize_or(Vec2::Y) };
        let level = Quat::from_rotation_z(Vec2::Y.angle_to(looks));
        Some(Funnel { tip: placed.transform_point3(head), turn: level * self.spin[i], scale: self.scale[i], tick: own })
    }
}

/// His vortex from the mission's (`read_vortex`), his clips and the funnel's model.
pub fn vortex(def: &VortexDef, library: &Library, animations: &AnimSet, robot: &ObjectDef) -> Option<Vortex> {
    let special = animations.find("PlayAnimSet", "AttackSpecial")?;
    let clip = special.clip.as_ref()?;
    let head_node = crc32(b"Head");
    let start = clip.effect_events.iter().filter(|e| e.node == head_node).map(|e| e.time).fold(f32::MAX, f32::min);
    let head_index = robot.nodes.iter().position(|n| n.name_hash == head_node)?;
    let rig = Rig::new(robot);
    let head = (0..=clip.length.round() as usize)
        .map(|tick| {
            let (_, turn, at) = rig.pose(clip, special.looping, tick as f32)[head_index].to_scale_rotation_translation();
            (at, turn)
        })
        .collect();
    let render = def.object.renders.first()?;
    let funnel = def.object.nodes.get(render.node)?.name_hash;
    let own = def.clip.as_ref()?;
    let channel = own.channel(funnel)?;
    let (mut scale, mut spin) = (Vec::new(), Vec::new());
    for tick in 0..=own.length.round() as usize {
        scale.push(channel.scale.sample(tick as f32, own.length, false).map_or(1.0, |s| s[0]));
        // Stored for row vectors: the conjugate is the turn for column vectors.
        spin.push(channel.rotation.sample(tick as f32, own.length, false).map_or(Quat::IDENTITY, |q| Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize().conjugate()));
    }
    let mesh = library.meshes.values().find(|m| m.name.eq_ignore_ascii_case(&render.model))?;
    let points = || mesh.parts.iter().flat_map(|p| &p.positions);
    let length = points().map(|p| p[1]).fold(0.0, f32::max);
    let profile = (0..10)
        .map(|band| {
            let (from, to) = (length * band as f32 / 10.0, length * (band + 1) as f32 / 10.0);
            points().filter(|p| (from..=to).contains(&p[1])).map(|p| p[0].hypot(p[2])).fold(0.0, f32::max)
        })
        .collect();
    Some(Vortex {
        clip: crc32(special.id.as_bytes()),
        start: if start == f32::MAX { 0.0 } else { start },
        head_node,
        head,
        scale,
        spin,
        model: render.model.clone(),
        length,
        profile,
        damage: def.damage.max(VORTEX_DAMAGE),
        flags: def.flags,
        knock_back_speed: def.knock_back_speed,
        knock_back_angle: def.knock_back_angle,
        sound: def.sound,
        sound_delay: def.sound_delay,
        object: Some(VortexObject(def.object.clone())),
    })
}

/// Reads `VortexDef` out of his pack (the boss mission's).
pub fn read_vortex(game_dir: &std::path::Path) -> Option<VortexDef> {
    use crate::formats::lxb::{DataFile, Node};
    use crate::formats::pack::{self, Pack};
    let pack = Pack::open(&crate::formats::characterlist::pack_file(game_dir, "devastator")).ok()?;
    for chunk in pack.of_type(pack::DATA) {
        let Ok(file) = DataFile::parse(pack.data(chunk).to_vec()) else { continue };
        let found = crate::character::find_label(file.root(), crc32(b"VortexObj"), 0, &mut std::collections::HashSet::new());
        let Some(object) = found.and_then(|i| i.get("things")?.at(0)) else { continue };
        let things: Vec<Node> = object.get("attachments").into_iter().flat_map(Node::items).filter_map(|a| a.get("thing")).collect();
        let of = |kind: &str| things.iter().copied().find(|t| t.type_name() == Some(kind));
        let kill = of("KillBox");
        let number = |node: Option<Node>, path: &[&str]| node.and_then(|n| n.path(path)).and_then(Node::float).unwrap_or(0.0);
        let clip = things
            .iter()
            .filter(|t| t.type_name() == Some("AnimBundle"))
            .flat_map(|b| b.get("records").into_iter().flat_map(Node::items))
            .find_map(|r| crate::formats::anim::read_clip(&file, r.get("anim")?));
        let delay = of("ForwardTrigger").and_then(|f| f.path(&["ForwardTo", "triggerArray"])?.at(0)?.get("delayMin")).and_then(Node::float);
        return Some(VortexDef {
            object: std::sync::Arc::new(crate::formats::scene::read_object(object, |hash| file.name(hash))?),
            clip,
            damage: number(kill, &["damage"]),
            flags: kill.and_then(|k| k.get("damageType")).and_then(Node::int).unwrap_or(0) as u32,
            knock_back_speed: number(kill, &["specialCaseDamageStuff", "knockBackSpeed"]),
            knock_back_angle: number(kill, &["specialCaseDamageStuff", "knockBackAngle"]),
            sound: of("SoundPlayer").and_then(|p| p.get("id")).and_then(Node::int).unwrap_or(0) as u32,
            sound_delay: delay.unwrap_or(0.0),
        });
    }
    None
}

/// One of his weapons as the port arms him: the boss mission's pods of one kind, each hung on a
/// bone of his. [data `work\devastator\boss1.txt`] In the mission they are placed objects the
/// script fixes to his attach points with a `BonePositioner` each (`AttachPt_GunPod01..04`,
/// `AttachPt_MissilePod01..03`, `AttachPt_Vortex`, `AttachPt_BugBombLauncher`): the four gatling
/// pods (`GunWeapon`, `Devastator_GatlingAttribute`), the three missile pods (`MissileLauncher`,
/// `DevastatorMissileAttribute`) and the two bug-bomb launchers (`MissileLauncher`,
/// `DevastatorBugBombAttribute`), fired by their `FireTurret`s at whoever is in range. [assumed port
/// design] the player fires them: one kind at a time, every pod of that kind in turn.
#[derive(Clone, Debug)]
pub struct Pods {
    /// Which kind (`POD_KINDS`): `GATLING`, `MISSILE` or `BUG_BOMB`.
    pub kind: usize,
    /// The pods' weapon (the first pod's: the others of a kind carry the same). [data]
    pub def: crate::weapons::WeaponDef,
    pub mounts: Vec<Mount>,
    /// The pod's own object (node tree and model, `HandGun.Pole` for a gatling pod), to draw on
    /// each mount; `None` where it did not read.
    pub object: Option<VortexObject>,
    /// Its rows of what its rounds show (`character::object_weapon_effects`): muzzle flash, impacts,
    /// trail, blast. [data]
    pub effects: Vec<crate::character::WeaponEffects>,
    /// Its weapon's muzzle light (`muzzleFlashObject`: `muzzleflash_orange_big`, a 15 m omni). [data]
    pub flash: Option<crate::point_light::FlashLight>,
}

/// One pod: the bone of his it hangs on (CRC-32 of its name) and its barrels (its `FireDummy`
/// nodes from `minFireDummyIndex` to `maxFireDummyIndex`) in its own frame. [data]
#[derive(Clone, Debug, Default)]
pub struct Mount {
    pub bone: u32,
    pub barrels: Vec<Vec3>,
}

/// The pod kinds (`Pods::kind`), in `POD_KINDS`' order.
pub const GATLING: usize = 0;
pub const MISSILE: usize = 1;
pub const BUG_BOMB: usize = 2;

/// The bug bombs' eruption [assumed port design, the user's ask of 2026-10-22: "like a volcano"]: a
/// firing throws `ERUPTION_BOMBS` bombs nearly straight up out of the launchers, unaimed, and each
/// falls on a random spot of the ring `ERUPTION_RING` (metres from his root, over his 25.6 m body
/// radius) around him. The boss's bombs are `MissileLauncher` rounds sent at a target [data]; none of
/// this is the game's.
pub const ERUPTION_BOMBS: u32 = 12;
/// The eruption's one vent: on the bone `AttachPt_BugBombLauncher` (CRC-32) of the launcher
/// on his back (`ERUPTION_VENT_BACK` behind it); the other, on `AttachPt_Vortex` at his mouth, stays still. Each bomb draws one
/// trail, not the game's three (its object is a cluster of three, each with its own emitter). [assumed
/// port design, the user's ask of 2026-10-23: one spawn point]
pub const ERUPTION_VENT: u32 = 0xbfed_9a58;
/// Metres the vent sits behind that bone, along his facing: the bone is on his centre line about
/// 50 m up, and the launcher's barrels' middle 14 m over it, on the axis he turns about, so a vent
/// there floated over his head and did not swing as he turned (the user, 2026-10-23). [assumed port
/// design]
pub const ERUPTION_VENT_BACK: f32 = 10.0;
/// The bombs leave from `ERUPTION_BOMBS` spots spread over his back round the vent, not one point
/// (the user, 2026-10-23): a disc this wide (metres) across his back, sunflower-spread so the spots
/// are even, the disc turned with his facing. [assumed port design]
pub const ERUPTION_VENT_SPREAD: f32 = 14.0;

/// The bombs' launch spots round `vent`: `count` points of a disc of radius `ERUPTION_VENT_SPREAD`
/// lying across his back (his right and his facing), each a little higher toward the middle as his
/// back's hump is. [assumed port design]
pub fn vent_spots(vent: Vec3, forward: Vec3, count: usize) -> Vec<Vec3> {
    let ahead = forward.truncate().normalize_or(Vec2::Y);
    let right = Vec2::new(ahead.y, -ahead.x);
    let golden = std::f32::consts::PI * (3.0 - 5.0f32.sqrt());
    (0..count)
        .map(|i| {
            let r = ERUPTION_VENT_SPREAD * ((i as f32 + 0.5) / count as f32).sqrt();
            let a = i as f32 * golden;
            let across = right * (r * a.cos()) + ahead * (r * a.sin() * 0.6);
            vent + across.extend(-0.15 * r)
        })
        .collect()
}
pub const ERUPTION_RING: [f32; 2] = [38.0, 120.0];
/// Metres the bombs climb over the launcher, drawn per bomb, and the fall's pull (m/s²): heavier
/// than the world's so the 60 m giant's rocks drop fast once they turn. [assumed port design]
const ERUPTION_APEX: [f32; 2] = [40.0, 95.0];
const ERUPTION_GRAVITY: f32 = 40.0;
/// Seconds a bomb that missed the ground keeps falling past its landing time before it goes off.
const ERUPTION_SPARE: f32 = 3.0;

/// The lowest the launcher's axis may point for a launch (its z): one tipped past it is lifted to
/// it, so a bomb never leaves into the ground. [assumed port design]
const ERUPTION_AXIS_LOWEST: f32 = 0.25;

/// One erupted bug bomb in flight [assumed port design]: it leaves the vent along the launcher's own
/// axis (the user, 2026-10-23: it tilts and turns with his body), curves over at its height and
/// comes down on its spot of the ring. The path is a cubic curve: from the vent, a control point
/// along the axis at the climb's height, one over the spot at that height, and the spot; it is
/// flown evenly in `flight`, the time a thrown rock of that climb takes under `ERUPTION_GRAVITY`.
/// Past the spot it falls on as a thrown body.
#[derive(Clone, Copy, Debug)]
pub struct Lob {
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    /// Seconds it takes to reach its spot, and the spot.
    pub flight: f32,
    pub landing: Vec3,
    /// The curve's four points.
    path: [Vec3; 4],
}

impl Lob {
    /// A bomb from the vent `launcher` leaving along `axis` (the launcher's out axis), falling on a
    /// spot of the ring round `root` (his feet, the ground's height), drawn by `random` (0 to 1):
    /// its bearing, its distance (even over the ring's area) and the height it climbs.
    pub fn erupt(launcher: Vec3, axis: Vec3, root: Vec3, random: &mut dyn FnMut() -> f32) -> Self {
        let bearing = random() * std::f32::consts::TAU;
        let [inner, outer] = ERUPTION_RING;
        let reach = (inner * inner + (outer * outer - inner * inner) * random()).sqrt();
        let landing = root + Vec3::new(bearing.cos(), bearing.sin(), 0.0) * reach;
        let climb = ERUPTION_APEX[0] + (ERUPTION_APEX[1] - ERUPTION_APEX[0]) * random();
        let mut axis = axis.normalize_or(Vec3::Z);
        if axis.z < ERUPTION_AXIS_LOWEST {
            axis = (axis.truncate().normalize_or_zero() * (1.0 - ERUPTION_AXIS_LOWEST * ERUPTION_AXIS_LOWEST).sqrt())
                .extend(ERUPTION_AXIS_LOWEST);
        }
        // The control points' height that puts the curve's peak at the climb: about a third over it
        // (exactly, for a spot level with the vent), set by a few corrections against the sampled peak.
        let path_at = |top: f32| [launcher, launcher + axis * ((top - launcher.z) / axis.z), landing.truncate().extend(top), landing];
        let peak = |p: &[Vec3; 4]| (0..=64).map(|i| Self::at(p, i as f32 / 64.0).z).fold(f32::MIN, f32::max);
        let mut top = launcher.z + climb * 4.0 / 3.0;
        for _ in 0..4 {
            top += (launcher.z + climb - peak(&path_at(top))) * 4.0 / 3.0;
        }
        let [_, out, over, _] = path_at(top);
        let up = (2.0 * ERUPTION_GRAVITY * climb).sqrt();
        let fall = (launcher.z + climb - landing.z).max(0.0);
        let flight = up / ERUPTION_GRAVITY + (2.0 * fall / ERUPTION_GRAVITY).sqrt();
        let path = [launcher, out, over, landing];
        let velocity = Self::along(&path, 0.0) / flight;
        Self { position: launcher, velocity, age: 0.0, flight, landing, path }
    }

    /// The curve's way (its derivative) at `s`.
    fn along(p: &[Vec3; 4], s: f32) -> Vec3 {
        let r = 1.0 - s;
        3.0 * r * r * (p[1] - p[0]) + 6.0 * r * s * (p[2] - p[1]) + 3.0 * s * s * (p[3] - p[2])
    }

    fn at(p: &[Vec3; 4], s: f32) -> Vec3 {
        let r = 1.0 - s;
        p[0] * (r * r * r) + p[1] * (3.0 * r * r * s) + p[2] * (3.0 * r * s * s) + p[3] * (s * s * s)
    }

    /// Moves it on by `dt` and returns the stretch covered, or `None` once it is long past its
    /// landing time (it fell through a hole).
    pub fn step(&mut self, dt: f32) -> Option<(Vec3, Vec3)> {
        let from = self.position;
        self.age += dt;
        if self.age > self.flight + ERUPTION_SPARE {
            return None;
        }
        if self.age <= self.flight {
            let s = self.age / self.flight;
            self.position = Self::at(&self.path, s);
            self.velocity = Self::along(&self.path, s) / self.flight;
        } else {
            self.position += self.velocity * dt - Vec3::Z * (0.5 * ERUPTION_GRAVITY * dt * dt);
            self.velocity.z -= ERUPTION_GRAVITY * dt;
        }
        Some((from, self.position))
    }

    pub fn direction(&self) -> Vec3 {
        self.velocity.normalize_or(Vec3::NEG_Z)
    }
}

/// Metres his missiles climb over the higher of their pod and their point, drawn per missile, and
/// what each metre of range to the point adds. [assumed port design]
pub const TOP_ATTACK_CLIMB: [f32; 2] = [45.0, 70.0];
pub const TOP_ATTACK_CLIMB_PER_RANGE: f32 = 0.15;
/// How far the launch leans off straight up, out from his body and toward the point (the
/// horizontal part of the launch's way, before it is made a unit).
const TOP_ATTACK_LEAN_OUT: f32 = 0.3;
const TOP_ATTACK_LEAN_TO: f32 = 0.2;
/// The share of the range the dive's control point is pulled back from over the point toward the
/// launch: 0 dives straight down at the end, more comes in at a slant.
const TOP_ATTACK_DIVE_BACK: f32 = 0.12;
/// The missile's mean speed over its path (m/s), the least time of a flight, and the ease: the path
/// is flown as `(age / flight)^ease`, slow out of the pod and fastest in the dive.
const TOP_ATTACK_SPEED: f32 = 105.0;
const TOP_ATTACK_LEAST_FLIGHT: f32 = 1.6;
const TOP_ATTACK_EASE: f32 = 1.6;
/// Seconds one that reached its point without touching anything flies on before it goes off.
const TOP_ATTACK_SPARE: f32 = 2.0;

/// One of his missiles flying a top attack (the user, 2026-10-23: like a Javelin, it flies up first
/// and comes down on what it is sent at, so none crosses his body) [assumed port design]. It leaves
/// its pod near straight up, leaning out from his body, climbs to its height, turns over and dives
/// on its point. The path is a cubic curve as the bomb's (`Lob`): the pod, a control point up the
/// launch's way at the climb's height, one at that height a little short of over the point, and the
/// point. A locked missile's point follows its target (`track`), so it comes down on it as it moves;
/// the curve's end moves the missile by the cube of its progress, smoothly. Past the point it flies on
/// straight.
#[derive(Clone, Copy, Debug)]
pub struct TopAttack {
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    /// Seconds it takes to reach its point.
    pub flight: f32,
    path: [Vec3; 4],
}

impl TopAttack {
    /// A missile from the pod at `pod` sent at `point`; `root` is his feet (the launch leans away
    /// from over them), `random` (0 to 1) draws its climb.
    pub fn launch(pod: Vec3, point: Vec3, root: Vec3, random: &mut dyn FnMut() -> f32) -> Self {
        let out = (pod - root).truncate().normalize_or_zero();
        let to = (point - pod).truncate();
        let range = to.length();
        let axis = (out * TOP_ATTACK_LEAN_OUT + to.normalize_or_zero() * TOP_ATTACK_LEAN_TO).extend(1.0).normalize();
        let climb = TOP_ATTACK_CLIMB[0] + (TOP_ATTACK_CLIMB[1] - TOP_ATTACK_CLIMB[0]) * random() + TOP_ATTACK_CLIMB_PER_RANGE * range;
        // The control points' height that puts the curve's peak at the climb, as the bomb's.
        let peak_at = pod.z.max(point.z) + climb;
        let path_at = |top: f32| [pod, pod + axis * ((top - pod.z) / axis.z), (point.truncate() - to * TOP_ATTACK_DIVE_BACK).extend(top), point];
        let peak = |p: &[Vec3; 4]| (0..=64).map(|i| Lob::at(p, i as f32 / 64.0).z).fold(f32::MIN, f32::max);
        let mut top = peak_at + climb / 3.0;
        for _ in 0..5 {
            top += (peak_at - peak(&path_at(top))) * 4.0 / 3.0;
        }
        let path = path_at(top);
        let length: f32 = (1..=32)
            .map(|i| Lob::at(&path, i as f32 / 32.0).distance(Lob::at(&path, (i - 1) as f32 / 32.0)))
            .sum();
        let flight = (length / TOP_ATTACK_SPEED).max(TOP_ATTACK_LEAST_FLIGHT);
        let mut missile = Self { position: pod, velocity: Vec3::ZERO, age: 0.0, flight, path };
        missile.velocity = missile.way(0.0);
        missile
    }

    /// Moves its point to `point` (its target's middle as it is now).
    pub fn track(&mut self, point: Vec3) {
        if self.age < self.flight {
            self.path[3] = point;
        }
    }

    /// Its point.
    pub fn point(&self) -> Vec3 {
        self.path[3]
    }

    /// Its velocity at progress `u` (0 to 1 of the flight's time) along the eased curve.
    fn way(&self, u: f32) -> Vec3 {
        // s = u^e, so ds/dt = e u^(e-1) / flight; the first instant takes a small u so it is not still.
        let u = u.max(0.02);
        let s = u.powf(TOP_ATTACK_EASE);
        Lob::along(&self.path, s) * (TOP_ATTACK_EASE * u.powf(TOP_ATTACK_EASE - 1.0) / self.flight)
    }

    /// Moves it on by `dt` and returns the stretch covered, or `None` once it is past its spare time.
    pub fn step(&mut self, dt: f32) -> Option<(Vec3, Vec3)> {
        let from = self.position;
        self.age += dt;
        if self.age > self.flight + TOP_ATTACK_SPARE {
            return None;
        }
        if self.age <= self.flight {
            let u = self.age / self.flight;
            self.position = Lob::at(&self.path, u.powf(TOP_ATTACK_EASE));
            self.velocity = self.way(u);
        } else {
            self.position += self.velocity * dt;
        }
        Some((from, self.position))
    }

    pub fn direction(&self) -> Vec3 {
        self.velocity.normalize_or(Vec3::NEG_Z)
    }
}

/// The gatling pods that may fire along `aim` (the user's ask of 2026-10-22: only the miniguns on
/// the side he shoots to, so no round or trail crosses his own body or pods). [assumed port design]
/// Each pod's side is its centre's way out from the middle of all of them, across the ground; a pod
/// fires when that way is on the shot's half (cosine over `POD_SIDE`), so a shot ahead takes the front
/// pods, one to his left the left pods. If none qualifies, the pod facing the shot best fires. A
/// shot straight up or down lets them all fire.
pub const POD_SIDE: f32 = 0.0;
pub fn pods_facing(centres: &[Vec3], aim: Vec3) -> Vec<bool> {
    let Some(way) = aim.truncate().try_normalize() else { return vec![true; centres.len()] };
    if centres.is_empty() {
        return Vec::new();
    }
    let middle = centres.iter().copied().sum::<Vec3>() / centres.len() as f32;
    let facing: Vec<f32> = centres.iter().map(|c| (*c - middle).truncate().normalize_or_zero().dot(way)).collect();
    let mut open: Vec<bool> = facing.iter().map(|&f| f >= POD_SIDE).collect();
    if !open.contains(&true) {
        let best = facing.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map_or(0, |(i, _)| i);
        open[best] = true;
    }
    open
}

/// His weapons' pods, by kind, and the placed objects each kind's pods are. [data]
const POD_KINDS: [&[&str]; 3] = [
    &["Devastator_GunPod01", "Devastator_GunPod02", "Devastator_GunPod03", "Devastator_GunPod04"],
    &["Devastator_MissilePodProxied01", "Devastator_MissilePodProxied02", "Devastator_MissilePodProxied03"],
    &["BugBomb_Launcher_Front", "BugBomb_Launcher_Back"],
];

/// His pods (`Pods`) from the boss mission's data: each kind whose placed objects are found and
/// carry a weapon and a `BonePositioner`.
pub fn read_pods(game_dir: &std::path::Path) -> Vec<Pods> {
    use crate::formats::lxb::{DataFile, Node};
    use crate::formats::pack::{self, Pack};
    let Ok(pack) = Pack::open(&crate::formats::characterlist::pack_file(game_dir, "devastator")) else { return Vec::new() };
    let int = |node: Option<Node>| node.and_then(Node::int).unwrap_or(0);
    let presets = crate::collision_masks::Presets::load(game_dir).unwrap_or_default();
    for chunk in pack.of_type(pack::DATA) {
        let Ok(file) = DataFile::parse(pack.data(chunk).to_vec()) else { continue };
        let mut kinds = Vec::new();
        for (list, names) in POD_KINDS.iter().enumerate() {
            let mut pods: Option<Pods> = None;
            for name in names.iter() {
                let found = file.root().get(name).or_else(|| find_deep(file.root(), crc32(name.as_bytes()), &mut std::collections::HashSet::new()));
                let Some(instance) = found else { continue };
                let Some(object) = instance.get("things").and_then(|t| t.at(0)) else { continue };
                let attached: Vec<Node> = object.get("attachments").into_iter().flat_map(Node::items).filter_map(|a| a.get("thing")).collect();
                let things = instance.get("things").into_iter().flat_map(Node::items);
                // Its bone: the BonePositioner among the instance's things or the object's attachments.
                let bone = things.chain(attached.iter().copied())
                    .find(|t| t.type_name() == Some("BonePositioner"))
                    .map(|p| int(p.get("boneName")) as u32);
                let weapon = attached.iter().copied().find(|&t| crate::weapons::is_weapon(t));
                let (Some(bone), Some(weapon)) = (bone, weapon) else { continue };
                let (first, last) = (int(weapon.get("minFireDummyIndex")).max(0) as usize, int(weapon.get("maxFireDummyIndex")).max(0) as usize);
                let barrels: Vec<Vec3> = (first..=last).filter_map(|i| crate::weapons::fire_dummy(Some(object), i)).collect();
                let pods = pods.get_or_insert_with(|| {
                    let mut def = crate::weapons::object_weapons(list, Some(object)).into_iter().next().expect("the weapon found above");
                    let mut effects = crate::character::object_weapon_effects(Some(object), &|hash| file.name(hash), &presets);
                    // The pods carry only the remote fire sound (`fireSoundRemote`: the gatling's
                    // `WPN_DEVASTATOR_GUN_FIRE`), the one heard from another's weapon; it is theirs
                    // to sound. [data] [assumed port design: played as the player's own]
                    if def.fire_sound == 0 {
                        def.fire_sound = int(weapon.path(&["genericWeaponSoundTable", "fireSoundRemote"])) as u32;
                    }
                    // His blasts sound his size (`MISSILE_BLAST_SOUND`, `BOMB_BLAST_SOUND`). [assumed port design]
                    let blast = match list {
                        MISSILE => Some(MISSILE_BLAST_SOUND),
                        BUG_BOMB => Some(BOMB_BLAST_SOUND),
                        _ => None,
                    };
                    for row in &mut effects {
                        row.fire_sound = def.fire_sound;
                        if let Some(blast) = blast.filter(|_| !row.blast_sounds.is_empty()) {
                            row.blast_sounds = vec![blast];
                        }
                    }
                    // The gatling overheats aloud (`OVERHEAT_SOUND`). [assumed port design]
                    if list == GATLING && def.overheat_sound == 0 {
                        def.overheat_sound = OVERHEAT_SOUND;
                    }
                    Pods {
                        kind: list,
                        def,
                        mounts: Vec::new(),
                        object: crate::formats::scene::read_object(object, |hash| file.name(hash)).map(|o| VortexObject(std::sync::Arc::new(o))),
                        effects,
                        flash: weapon.get("muzzleFlashObject").and_then(crate::point_light::read_flash_light),
                    }
                });
                pods.mounts.push(Mount { bone, barrels });
            }
            kinds.extend(pods);
        }
        if !kinds.is_empty() {
            return kinds;
        }
    }
    Vec::new()
}

/// `character::find_label` without its depth limit: the bug-bomb launchers sit past it, at the end
/// of trigger chains in the boss mission's script objects.
fn find_deep<'a>(node: crate::formats::lxb::Node<'a>, label: u32, seen: &mut std::collections::HashSet<usize>) -> Option<crate::formats::lxb::Node<'a>> {
    if node.offset().is_some_and(|o| !seen.insert(o)) {
        return None;
    }
    if node.label() == Some(label) {
        return Some(node);
    }
    node.fields().into_iter().map(|(_, n)| n).chain(node.items()).find_map(|n| find_deep(n, label, seen))
}

/// What the pad asks of him this update.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ask {
    pub messages: Vec<u32>,
    /// He turns on the spot: the machine must not see the stick (it would walk him).
    pub hide_stick: bool,
}

impl Giant {
    /// The messages his machine's connections wait for, by their connections' names. [data]
    pub fn from_graph(graph: &Graph) -> Self {
        let message = |name: &str| {
            graph.connections.iter().find(|c| c.name == name).and_then(|c| match c.test {
                Test::Message(m) => Some(m),
                _ => None,
            })
        };
        Self {
            attack: message("connectionMessageDevastatorAttack"),
            special: message("connectionMessageAttackSpecial"),
            turn_left: message("connectionMessageTurnLeft"),
            turn_right: message("connectionMessageTurnRight"),
            waves: Vec::new(),
            limbs: None,
            roots: HashMap::new(),
            walk_turn: 0.0,
            vortex: None,
            stride: 0.0,
            pods: Vec::new(),
            dressing: Dressing::default(),
        }
    }

    /// The pad as messages. `idle`: he stands in `stateIdle`, the one state the turns leave from.
    /// [assumed port binding] the melee button pounds and the special button starts the vortex,
    /// on the press; a push of the stick past `TURN_STICK` and more than `TURN_ANGLE` off his
    /// `facing` turns him toward it, left when it points to his left.
    pub fn ask(&self, idle: bool, facing: Vec2, stick: Vec2, melee: bool, special: bool) -> Ask {
        let mut ask = Ask::default();
        if melee && let Some(m) = self.attack {
            ask.messages.push(m);
        }
        if special && let Some(m) = self.special {
            ask.messages.push(m);
        }
        if idle && stick.length() >= TURN_STICK {
            let off = facing.angle_to(stick);
            if off.abs() > TURN_ANGLE {
                // A positive angle from his facing is toward his left (`angle_to` is
                // counter-clockwise, and x is right, y forward).
                let turn = if off > 0.0 { self.turn_left } else { self.turn_right };
                if let Some(m) = turn {
                    ask.messages.push(m);
                    ask.hide_stick = true;
                }
            }
        }
        ask
    }
}

/// Moves his clips' root onto his chest (`CENTRE`), keeping every node where the clip puts it.
/// Call on his clips as read, before anything is measured on them (`fit`, the tuning's sets).
///
/// His clips are made in two frames [data `devastator_clips --frames`]: walking, the root sits
/// under his chest, 21 m ahead of his hips (`Walk`, and the start of the turns, the pounds and
/// the hit reacts); standing, it sits by his hind feet, 5 m behind his hips (`Idle`, `Attack1`,
/// `AttackSpecial`, and the end of all those). Where a clip goes from one to the other the root
/// jumps across his body while he stays put: `Idle2Attack1` ticks 52..56 by 35.2 m, the turns
/// all through (`TurnLeft` carries it 22.5 m sideways as he pivots on the spot). The game moves
/// his object by the root, rigidly in its own frame (`00544e00` root mode 0), which is the same
/// motion; the port's position is what its camera follows and where a clip that follows starts,
/// so here it is put on his body instead: the root becomes the chest's point on the ground
/// (turned as the root is), and each of the root's children is moved back by as much, so the
/// model is where the clip puts it at every tick. [assumed port reference point] Settles:
/// nothing to settle in the game (the root is only ever his object's place there); play.
pub fn reroot(animations: &mut AnimSet, robot: &ObjectDef) {
    let (Some(centre), Some(root)) = (robot.nodes.iter().position(|n| n.name == CENTRE), robot.nodes.iter().position(|n| n.name_hash == ROOT_CHANNEL))
    else {
        return;
    };
    let rig = Rig::new(robot);
    let to_root = affine(&robot.nodes[root].bind).inverse();
    // Each child of the root with its place at rest, used where a clip does not move it.
    let children: Vec<(u32, Vec3)> = robot
        .nodes
        .iter()
        .filter(|n| n.parent == Some(root))
        .map(|n| (n.name_hash, (to_root * affine(&n.bind)).translation.into()))
        .collect();
    for (set, refs) in animations.sets.iter_mut() {
        if set == Actions::VEHICLE_SET {
            continue;
        }
        for r in refs.iter_mut() {
            let looping = r.looping;
            if let Some(clip) = r.clip.as_mut() {
                reroot_clip(clip, &rig, looping, centre, &children);
            }
        }
    }
}

fn reroot_clip(clip: &mut Clip, rig: &Rig, looping: bool, centre: usize, children: &[(u32, Vec3)]) {
    let ticks = clip.length.round().max(1.0) as usize;
    let mut times: Vec<f32> = (0..=ticks).map(|t| (t as f32).min(clip.length.max(0.0))).collect();
    times.dedup();
    // The chest's place on the ground with the root held, as the port draws him.
    let offsets: Vec<Vec3> = times
        .iter()
        .map(|&t| {
            let at = rig.pose(clip, looping, t)[centre].translation;
            Vec3::new(at.x, at.y, 0.0)
        })
        .collect();
    let length = clip.length;
    let root = clip.channel(ROOT_CHANNEL);
    let keyed = root.filter(|c| c.position.sample(0.0, length, false).is_some());
    let (start, end) = (Vec3::from(clip.start[3]), Vec3::from(clip.end[3]));
    // Stored for row vectors: the conjugate is the root's turn for column vectors.
    let turn = |t: f32| {
        root.and_then(|c| c.rotation.sample(t, length, false))
            .map_or(Quat::IDENTITY, |q| Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize().conjugate())
    };
    // The root's place as `Clip::root_path` reads it (the start-to-end line where it is not keyed).
    let place = |t: f32| match keyed.and_then(|c| c.position.sample(t, length, false)) {
        Some(p) => Vec3::from(p),
        None => start + (end - start) * if length > 0.0 { t / length } else { 0.0 },
    };
    let rooted: Vec<[f32; 3]> = times.iter().zip(&offsets).map(|(&t, &h)| (place(t) + turn(t) * h).into()).collect();
    let moved: Vec<(u32, Vec<[f32; 3]>)> = children
        .iter()
        .map(|&(name, rest)| {
            let track = clip.channel(name).map(|c| &c.position);
            let at = |t: f32| track.and_then(|tr| tr.sample(t, length, looping)).map_or(rest, Vec3::from);
            (name, times.iter().zip(&offsets).map(|(&t, &h)| (at(t) - h).into()).collect())
        })
        .collect();
    let (first, last) = (offsets[0], *offsets.last().unwrap_or(&Vec3::ZERO));
    let (turn_first, turn_last) = (turn(0.0), turn(length));
    clip.start[3] = (start + turn_first * first).into();
    clip.end[3] = (end + turn_last * last).into();
    channel_mut(clip, ROOT_CHANNEL).position = Track::from_keys(times.clone(), rooted);
    for (name, positions) in moved {
        channel_mut(clip, name).position = Track::from_keys(times.clone(), positions);
    }
}

/// A clip's channel for a node, made empty where the clip has none.
fn channel_mut(clip: &mut Clip, name: u32) -> &mut Channel {
    let i = match clip.names.iter().position(|&n| n == name) {
        Some(i) => i,
        None => {
            clip.names.push(name);
            clip.channels.push(Channel::default());
            clip.names.len() - 1
        }
    };
    &mut clip.channels[i]
}

/// Whether a loaded character is Devastator, the one this binding is for: his pack's `nameID`.
/// Not "his object has `ShockWave`s": `optimusflight` and `megatronflight` carry one each too
/// (no damage, nothing drawn) [data], and taking that for him made both walk-only giants with
/// his pods in place of their own guns and no HUD (the user's report, 2026-10-25).
pub fn is_giant(data: &crate::character::CharacterData) -> bool {
    data.name_id == crc32(b"Devastator")
}

/// Makes the loaded character a playable Devastator: his machine's extra sets as actions, the
/// jump off, the camera scaled to his size, and his shockwaves. Called only for him
/// (`is_giant`), on clips already `reroot`ed.
pub fn fit(t: &mut Tuning, animations: &AnimSet, robot: &ObjectDef, shockwaves: &[(usize, Def)]) {
    // His turns play `TurnSet`, his special `PlayAnimSet`: sets no other robot plays as an
    // action. [data]
    t.actions = std::mem::take(&mut t.actions).with_sets(animations, robot, &["TurnSet", "PlayAnimSet"]);
    // He has no JumpSet: the shared jump states would start nothing and float him. [assumed]
    // Settles: whether his machine list holds the jump states at all (his brain never sends one).
    t.control.set_enabled(crc32(b"StateJump"), None, false);
    // [assumed port design] He can be hurt and killed. The boss is `invincible` with 100000
    // health [data]: the mission ends him by script, never by damage. Played, he takes damage
    // like anyone over `GIANT_HEALTH` (his regeneration's curve scales with it [game]); nothing
    // knocks him about (`immovable`); and the man-sized hit reacts and stun, which play
    // `HitReactionSet` (he has none [data]), are off: `Stagger` answers hits instead.
    t.damage.invincible = false;
    t.damage.immovable = true;
    t.base_health = GIANT_HEALTH;
    // His regeneration starts at once after a hit [data: healthRegenDelay 0]: under a squad's
    // fire it restarted (and its `UI_REGEN_START` sounded) with every round. It waits
    // `GIANT_REGEN_DELAY` unhurt instead. [assumed port design]
    t.health_regen_delay = t.health_regen_delay.max(GIANT_REGEN_DELAY);
    t.control.set_enabled(crc32(b"StateHitReact"), None, false);
    t.control.set_enabled(crc32(b"StateStun"), None, false);
    // His body against the world and as others' target, from his model (`body`), before the
    // cameras: their look point is set against his height.
    // Others aim at his height times his `aimingHeightPercentage`, 3500 of his 1 m: 35 m up, his
    // chest [data]; that point is kept as his height changes.
    let stride = body(animations, robot).map_or(0.0, |(radius, height, stride)| {
        t.aiming_height_percentage *= t.robot_height / height;
        (t.robot_radius, t.robot_height) = (radius, height);
        stride
    });
    scale_cameras(t, model_height(robot));
    // He settles into his stand over `IDLE_BLEND`, not his Idle's 0.1 s [data]: he has no clip
    // from the walk to the stand, and a 60 m body snapping into it reads as weightless; his root
    // motion runs down over the same time (`sim::giant_root_motion`). [assumed port design]
    let idle = t.rule_sets.get_mut(&crc32(b"IdleStandSet")).and_then(|set| set.clips.iter_mut().find(|c| c.id == crc32(b"Idle")));
    if let Some(idle) = idle {
        idle.crossfade = idle.crossfade.max(IDLE_BLEND);
    }
    let mut giant = Giant::from_graph(&t.control);
    giant.waves = waves(animations, robot, shockwaves);
    for wave in &mut giant.waves {
        wave.def.damage = wave.def.damage.max(POUND_DAMAGE);
    }
    giant.roots = roots(animations);
    giant.walk_turn = animations
        .find("TurnSet", "TurnLeft")
        .and_then(|r| {
            let clip = r.clip.as_ref()?;
            let turned = clip.root_turn().last()?.abs();
            (clip.length > 0.0).then(|| turned / (clip.length / r.speed.max(1e-3) / TICKS_PER_SECOND))
        })
        .unwrap_or(t.moving.turn_speed.to_radians());
    giant.stride = stride;
    t.giant = Some(giant);
}

/// His body as the upright cylinder every character is to the world and to others' rounds:
/// (radius, height, stride). His attributes' avatar is 1 m by 1 m [data], a scripted boss's
/// placeholder under a 50 m model, so walls stopped only the point under his chest and rounds
/// missed all of him. [assumed port design] Measured in `Idle` at tick 0, on his re-rooted
/// clips: the radius is his four limbs' mean distance from his root across the ground (18 to 33
/// m, 25.6), so he stands in it to his wrists and his widest reach pokes out; the height is
/// his model's (`model_height`, 60.6 m, the bind pose's top; about 50 in his stand); and the cylinder starts at his ankles' height (the
/// mean of `Foot_L` and `Foot_R`, about 9 m), so what is lower passes between his legs, as a
/// scout does. Settles: play; the game never moves him into anything.
pub fn body(animations: &AnimSet, robot: &ObjectDef) -> Option<(f32, f32, f32)> {
    let idle = animations
        .find("IdleStandSet", "Idle")
        .or_else(|| animations.sets.values().flatten().find(|r| r.id == "Idle"))?;
    let pose = Rig::new(robot).pose(idle.clip.as_ref()?, idle.looping, 0.0);
    let at = |name: &str| {
        let index = robot.nodes.iter().position(|n| n.name_hash == crc32(name.as_bytes()))?;
        pose.get(index).map(|place| Vec3::from(place.translation))
    };
    let limbs = ["Foot_L", "Foot_R", "Hand_L", "Hand_R"].map(at);
    let [Some(foot_l), Some(foot_r), Some(hand_l), Some(hand_r)] = limbs else { return None };
    let radius = [foot_l, foot_r, hand_l, hand_r].iter().map(|p| p.truncate().length()).sum::<f32>() / 4.0;
    Some((radius, model_height(robot), (foot_l.z + foot_r.z) / 2.0))
}

/// Every clip's root motion, by set and clip. His walk's root surges and checks with each step
/// (0.12 to 0.27 m a tick on his chest), and his limbs are planted only while he moves with it:
/// at the walk's even speed (`Clip::forward_speed`, 10.96 m/s) his planted hands slip up to 0.23 m
/// a tick, against 0.02 with the clip's own root path (`devastator_clips --reroot --slide Walk`).
/// The robots' walk keeps the even speed [trace: Bumblebee's, `step_pace`]; his clips are the
/// whole of his motion in the game (`00544e00` root mode 0), so here they move him. [assumed
/// port design]
fn roots(animations: &AnimSet) -> HashMap<(u32, u32), Root> {
    animations
        .sets
        .iter()
        .flat_map(|(set, refs)| refs.iter().map(move |r| (set, r)))
        .filter_map(|(set, r)| Some(((crc32(set.as_bytes()), crc32(r.id.as_bytes())), Root::new(r.clip.as_ref()?, r.looping))))
        .collect()
}

/// His limbs for the port's foot IK (`footik::FootIk`): both legs (Thigh, Calf, Foot) and both
/// arms (Arm, Forearm, Hand), each sole measured where it stands in `Idle` at tick 0, on his
/// clips already `reroot`ed; the probe scaled by his height over an ordinary robot's.
/// [assumed] The game puts no foot IK on him: his pack holds no FootIK object, only one SpineIK
/// (`devastator_clips --ik`), like the playable robots'. The arms are limbs because he walks on
/// his fingertips (lowest in `Walk` and `Idle`: `devastator_clips --limbs`). settles: nothing in
/// the game; a recording of him on a slope would show whether his hands and feet follow it at all.
pub fn limbs(animations: &AnimSet, robot: &ObjectDef, library: &Library) -> Option<FootRig> {
    let idle = animations
        .find("IdleStandSet", "Idle")
        .or_else(|| animations.sets.values().flatten().find(|r| r.id == "Idle"))?;
    let pose = Rig::new(robot).pose(idle.clip.as_ref()?, idle.looping, 0.0);
    let node = |name: &str| robot.nodes.iter().position(|n| n.name_hash == crc32(name.as_bytes()));
    let legs = ["Foot_L", "Foot_R", "Hand_L", "Hand_R"]
        .iter()
        .map(|name| Leg::planted(robot, library, &pose, node(name)?, SOLE_REACH))
        .collect::<Option<Vec<_>>>()?;
    Some(FootRig {
        legs,
        body: node("Main")?,
        probe: PROBE * (model_height(robot) / ORDINARY_HEIGHT).max(1.0),
        weight: Some(WEIGHT),
    })
}

/// His limbs' and body's heavy planting (`footik::Weight`) [assumed port design; the user, 2026-10-23:
/// "weightless when he's pushing his feet up over a small obstacle"]: a limb takes about half a second
/// to rise onto a block (a quarter to come down), looks 0.35 s ahead while it swings, his body rises a
/// third of his limbs' mean lift with a slight bob, and a limb landing up there sinks it by a third of
/// its speed.
pub const WEIGHT: crate::footik::Weight = crate::footik::Weight {
    rise: 9.0,
    fall: 16.0,
    lead: 0.35,
    lead_reach: 6.0,
    heave: 0.35,
    body_rate: 5.0,
    body_damping: 0.55,
    settle: 0.35,
};

/// How far above a limb's lowest mesh point the points that carry it reach: his fingertips lie
/// within 0.8 m of each other in `Idle` (`devastator_clips --limbs`). [assumed]
const SOLE_REACH: f32 = 1.0;

/// The top of his bind pose: how tall he is.
pub fn model_height(robot: &ObjectDef) -> f32 {
    robot.nodes.iter().map(|n| n.bind[3][2]).fold(0.0, f32::max)
}

/// The behemoth camera [assumed port design: the game has no camera for a playable Devastator; the
/// user asked for a custom one, 2026-10-22]. Built on the ordinary robot camera (`robotCamera` and its
/// distance levels [data]), so it follows, lags and collides as theirs do, with a giant's framing:
/// - distances scaled by his height over an ordinary robot's, then brought in to `NEAR` of that
///   (about 95 m in place of 165): he fills the picture's middle third instead of a quarter;
/// - the look point at `LOOK` of his height (his collar, about 41 m) and `SHOULDER` of his height
///   to his right, so the picture looks past his shoulder rather than at his back;
/// - the camera held a little below that point (`ELEVATION` degrees, down to `LOWEST`), so it
///   looks up at him;
/// - the player's turning of it slowed to `TURN` of the ordinary rate, and the view widened by
///   `WIDER` degrees.
fn scale_cameras(t: &mut Tuning, height: f32) {
    const NEAR: f32 = 0.62;
    const LOOK: f32 = 0.68;
    const SHOULDER: f32 = 0.18;
    const ELEVATION: f32 = -5.0;
    const LOWEST: f32 = -18.0;
    const TURN: f32 = 0.6;
    const WIDER: f32 = 5.0;
    let scale = (height / ORDINARY_HEIGHT).max(1.0) * NEAR;
    let look = height * LOOK - t.robot_height;
    let fit = |c: &mut CameraTuning| {
        c.min_dist *= scale;
        c.max_dist *= scale;
        c.face_extra_dist *= scale;
        c.climb_extra_dist *= scale;
        c.max_speed_dist_add *= scale;
        c.side_offset = c.side_offset * scale + height * SHOULDER;
        c.ahead_offset *= scale;
        c.height_offset += look;
        c.default_elevation = ELEVATION;
        c.min_elevation = c.min_elevation.min(LOWEST);
        c.user_rot_speed_z *= TURN;
        c.user_rot_speed_x *= TURN;
        c.fov += WIDER;
    };
    fit(&mut t.robot_camera);
    t.robot_camera_levels.iter_mut().for_each(fit);
    // His aim (L2, his pods' guns): the weapon camera over his right shoulder, at `AIM_LOOK` of his
    // height, `AIM_SIDE` of it to the side and its distances scaled to `AIM_NEAR` (about 45 m back),
    // turning at `TURN` of the ordinary rate: a gunner's view past the giant's shoulder.
    const AIM_NEAR: f32 = 0.38;
    const AIM_LOOK: f32 = 0.95;
    const AIM_SIDE: f32 = 0.36;
    let aim_scale = (height / ORDINARY_HEIGHT).max(1.0) * AIM_NEAR;
    let aim_look = height * AIM_LOOK - t.robot_height;
    let fit_aim = |c: &mut CameraTuning| {
        c.min_dist *= aim_scale;
        c.max_dist *= aim_scale;
        c.side_offset = c.side_offset * aim_scale + height * AIM_SIDE;
        c.ahead_offset *= aim_scale;
        c.height_offset += aim_look;
        c.user_rot_speed_z *= TURN;
        c.user_rot_speed_x *= TURN;
    };
    fit_aim(&mut t.weapon_camera);
    t.weapon_camera_levels.iter_mut().for_each(fit_aim);
    // The shakes that have a place reach as far as his camera stands off: their distances [data] are
    // for a camera 5 m from a 6 m robot (`SoftCameraShake` whole to 47 m, gone at 90 m), and his camera
    // stands about 105 m from him, so his footfalls' shakes never reached it (found 2026-10-23).
    // Scaled by his camera's own scale, they are whole to about 290 m.
    for def in t.shakes.values_mut().filter(|d| d.max_dist < 1.0e6) {
        def.full_dist *= scale;
        def.max_dist *= scale;
    }
}

/// His `ShockWave`s, each with its node's path through the clips that trigger it. [data]
fn waves(animations: &AnimSet, robot: &ObjectDef, shockwaves: &[(usize, Def)]) -> Vec<Wave> {
    let rig = Rig::new(robot);
    shockwaves
        .iter()
        .filter_map(|(index, def)| {
            let node = robot.nodes.get(*index)?;
            let mut centres = HashMap::new();
            for r in animations.sets.values().flatten() {
                let Some(clip) = &r.clip else { continue };
                let named = clip.action_events.iter().any(|e| matches!(e.action, Action::Trigger { name } if name == def.name));
                if !named {
                    continue;
                }
                let path = (0..=clip.length.round().max(1.0) as usize)
                    .map(|tick| rig.pose(clip, r.looping, tick as f32)[*index].translation.into())
                    .collect();
                centres.insert(crc32(r.id.as_bytes()), path);
            }
            Some(Wave { def: def.clone(), node: node.name_hash, centres })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn giant() -> Giant {
        Giant { attack: Some(1), special: Some(2), turn_left: Some(3), turn_right: Some(4), ..Giant::default() }
    }

    #[test]
    fn buttons_send_their_messages() {
        let g = giant();
        assert_eq!(g.ask(true, Vec2::Y, Vec2::ZERO, true, false).messages, vec![1]);
        assert_eq!(g.ask(false, Vec2::Y, Vec2::ZERO, false, true).messages, vec![2]);
    }

    #[test]
    fn root_travel_is_in_the_frame_it_starts_from_and_comes_round_a_loop() {
        use std::f32::consts::FRAC_PI_2;
        // Forward 1 m a tick for 4 ticks while turning a quarter round to the left.
        let root = Root {
            path: (0..=4).map(|i| Vec3::new(0.0, i as f32, 0.0)).collect(),
            turn: (0..=4).map(|i| FRAC_PI_2 * i as f32 / 4.0).collect(),
            looping: true,
            steps: vec![0.5, 2.5],
        };
        // A limb lands in a step that passes its tick, round the loop too.
        assert!(root.lands(2.0, 3.0) && !root.lands(3.0, 3.5) && root.lands(3.5, 1.0));
        let (moved, turned) = root.travel(2.0, 3.0);
        // Turned an eighth at tick 2, forward in the start frame is ahead and to the right.
        assert!((moved - Vec3::new(0.5f32.sqrt(), 0.5f32.sqrt(), 0.0)).length() < 1e-5, "{moved}");
        assert!((turned - FRAC_PI_2 / 4.0).abs() < 1e-6);
        // Round the loop: 3 -> 4, then 0 -> 1, the second part in the frame the first ended in.
        let (moved, turned) = root.travel(3.0, 1.0);
        let (a, ta) = root.travel(3.0, 4.0);
        let (b, tb) = root.travel(0.0, 1.0);
        assert!((moved - (a + Quat::from_rotation_z(ta) * b)).length() < 1e-5);
        assert!((turned - (ta + tb)).abs() < 1e-6);
        // A clip that does not loop stays at its end.
        let once = Root { looping: false, ..root };
        assert_eq!(once.travel(3.0, 1.0).0, Vec3::ZERO);
        assert!(!once.lands(3.5, 1.0));
    }

    #[test]
    fn a_push_off_his_facing_turns_him_toward_it() {
        let g = giant();
        // Facing +y: -x is his left.
        let left = g.ask(true, Vec2::Y, Vec2::NEG_X, false, false);
        assert_eq!(left, Ask { messages: vec![3], hide_stick: true });
        let right = g.ask(true, Vec2::Y, Vec2::X, false, false);
        assert_eq!(right.messages, vec![4]);
        // Ahead, or a push too slight, walks; walking, he never turns on the spot.
        assert!(g.ask(true, Vec2::Y, Vec2::new(0.3, 1.0), false, false).messages.is_empty());
        assert!(g.ask(true, Vec2::Y, Vec2::NEG_X * 0.3, false, false).messages.is_empty());
        assert!(g.ask(false, Vec2::Y, Vec2::NEG_X, false, false).messages.is_empty());
    }
}
