//! The robot's weapons as the game's weapon manager runs them: the trigger, the heat, what is
//! fired and what it hits, the car's weapons, the special's weapons and turrets, the launcher's
//! lock-on, missiles, grenades, ground waves and the flame. The rules and numbers are
//! `weapons`, `turret`, `gun`, `lockon` and `special_weapon` (the game's code and data); this is
//! the manager that holds them together for one robot, update by update. It moved here from the
//! Bevy game (`game/src/weapons.rs`) on 2026-10-27 so that ports and reference runs have it.
//!
//! No engine types: sounds go out through `WeaponSound` (the host's sound player), effects come
//! back as `WeaponFx` (`Arsenal::fx`), and what is drawn is read back (`projectiles`,
//! `projectile_frame`, `wave_frame`, `debris_pieces`, `energon_waves`). The tracer lines and the
//! hit rings are the port's debug drawing.

use std::path::Path;

use glam::{Affine3A, Mat3, Quat, Vec2, Vec3};

use crate::aim::AssistTarget;
use crate::camera::Sight;
use crate::formats::anim::Clip;
use crate::formats::lxb::DataFile;
use crate::formats::pack::{self, Pack};
use crate::gun::{Axe, Clips, Gun};
use crate::lockon::{self, LockOn, View};
use crate::melee::Target;
use crate::reticle::{self, Reticle, Sprite};
use crate::special_weapon::{self, SpecialFire};
use crate::tuning::AimTuning;
use crate::turret::{self, Burst, FireTimer, LauncherTuning, Rotator, State, Turret};
use crate::weapons::{self, Fired, Kind, Missile, MissileBlast, Part, VehicleWait, Weapon, WeaponDef, WeaponTuning};

/// The sound player the weapons speak to: the game's sound events by id, kept handles, and the
/// projectiles' moving sounds. The Bevy game's `SoundOut` is one; a port gives its own, and a
/// reference run records the calls.
pub trait WeaponSound {
    /// Plays a sound event once, where the listener is.
    fn play_event(&self, id: u32);
    /// Plays a sound event once at a point.
    fn play_event_at(&self, id: u32, point: Vec3);
    /// Starts a sound event and keeps its handle; `heat` is its `heat` parameter.
    fn start_kept(&self, id: u32, heat: Option<f32>) -> Option<crate::sound::Handle>;
    /// Whether the event loops until it is stopped.
    fn is_infinite(&self, id: u32) -> bool;
    /// Lets a kept sound go into its tail (`sound_event_stop`).
    fn key_off(&self, handle: crate::sound::Handle);
    /// Stops a kept sound at once.
    fn stop(&self, handle: crate::sound::Handle);
    /// A projectile's sounds: its start, its place and speed each update, its stop and its end.
    fn projectile_start(&self, number: u32, weapon: u32, point: Vec3);
    /// A projectile whose sounds are these events, not its weapon's (a ground wave's object's).
    fn projectile_start_sounds(&self, number: u32, ids: &[u32], point: Vec3);
    fn projectile_move(&self, number: u32, point: Vec3, speed: f32);
    fn projectile_stop(&self, number: u32);
    fn projectile_end(&self, number: u32, weapon: u32, point: Vec3);
    /// A round met a surface of this material.
    fn weapon_impact(&self, weapon: u32, surface: u32, point: Vec3);
    /// Whether something is locked or aimed at (the music's combat flag).
    fn combat_target(&self, present: bool);
    /// A sound command of the weapon object's clip.
    fn weapon_clip_command(&self, command: crate::gun::SoundCommand);
    /// The weapon object was hidden: its clip's sounds stop.
    fn weapon_object_hidden(&self);
}

/// What a weapon asks for as it fires (`weapons::Arsenal::fx`).
#[derive(Clone, Copy, Debug)]
pub enum WeaponFxKind {
    Muzzle(usize),
    /// A round landed on a surface of this material, with the hit's normal (zero when not known).
    Impact(u32, Vec3),
    /// A missile left: its trail, kept under the missile's number.
    Trail(u32),
    BulletTrail {end:Vec3,speed:f32},
    Blast,
    /// A ground wave was let go (`Arsenal::let_wave_go`), kept under its number
    /// (`Arsenal::wave_frame`): its object's emitter and light ride it.
    Wave(u32),
    /// A piece its blast object threw, by its number and its index in the round's row
    /// (`WeaponEffects::debris`): its own emitters ride it (`Arsenal::projectile_frame`).
    Debris(u32, usize),
    /// A projectile ended: its Destructable's `destroyEffectSpawn` smoke (`TimedParticle`s).
    AfterBlast,
    /// The charged shot (`WeaponFxSpec`): the charge's emitter starts on the object's
    /// `ChargeDummy` when the charge begins; its rate follows the level; the full charge's
    /// effect takes its place; the release stops both. [game 007addc0, 007b0520, 007af210]
    ChargeStart,
    ChargeLevel(f32),
    ChargeFull,
    ChargeStop,
    /// The flame: its stream starts on the object's `FireDummy`; each dummy's looping effect
    /// starts when its sphere comes on (by its node number); all stop at the release or the
    /// overheat. [game 0074d110, 0074e9b0]
    FlameStream,
    FlameBall(usize),
    FlameStop,
    /// The flame's ray met a body this update: its `impactEffect` is put at `at`, turned so its
    /// +z is the body's normal (`along`). [game 0074c200, `align_z_matrix` 0056e7b0]
    FlameImpact,
    /// The ray met nothing (or the flame went out): the impact emitter stops. [game 0074b130: with no hit the flame sends the impact's TriggerableParticle message code 2 (stop, through 007479f0) once, on the change from on to off (flag at +0x8 set); 0057dca0 case 2 is 0057e000, which stops the emitter so its particles live out. Read 2026-10-20]
    FlameImpactLost,
}

#[derive(Clone, Copy, Debug)]
pub struct WeaponFx {
    pub kind: WeaponFxKind,
    /// The weapon's name.
    pub weapon: u32,
    pub at: Vec3,
    /// The way the round flies.
    pub along: Vec3,
}

/// An enemy's collision bodies a gun round strikes (its pack's `Character` primitives: the Head box
/// on the scout's node 22, its UpperBody and LowerBody boxes), posed by this frame's clip in its own
/// space, and the yaw it faces. [game 007832b0] [data] Given to the Arsenal by `animate`.
#[derive(Clone, Debug)]
pub struct TargetParts {
    pub id: u32,
    pub yaw: f32,
    pub bodies: Vec<crate::weapons::PartBody>,
}

impl TargetParts {
    /// The bodies where the enemy stands at `pos` (its feet): turned by its yaw about the up axis
    /// (`transform.rotation` of the owner, Bevy's y, is the game's z) and moved there.
    pub fn placed(&self, pos: Vec3) -> Vec<crate::weapons::PartBody> {
        let place = glam::Affine3A::from_rotation_translation(Quat::from_rotation_z(self.yaw), pos);
        self.bodies.iter().map(|b| crate::weapons::PartBody { part: b.part, body: b.body.moved(&place) }).collect()
    }
}

/// How long a shot's line and a hit's ring stay on screen, in seconds.
pub const TRACER_SECONDS: f32 = 0.08;
/// How far round a ground wave's point it reaches a target: its object's Collision sphere [data]. Each of
/// the three roster waves (blackout, jetfire, optimusflight: the Collision sibling of SnakerProjectile under
/// `spawnOnDestruction`) has a `CollisionSpherePacket` of radius 1.36743 (`tf2_lxb.py dump`), so this
/// fallback (for a row with no sphere or no row) is not reached for the roster: a value for a non-roster
/// row only [port] (the game's reach without a sphere is unread).
const SNAKER_REACH: f32 = 1.0;
pub const IMPACT_SECONDS: f32 = 0.35;

/// The share of the data's heat a round Devastator's pods take (`Arsenal::load_giant`). [assumed port design]
const GIANT_HEAT: f32 = 0.25;

pub struct Held {
    pub def: WeaponDef,
    pub tuning: WeaponTuning,
    pub state: Weapon,
    /// A flamethrower's flame.
    pub flame: weapons::Flame,
    /// The flame's victims (its hit list, weapon `+0x42c`): each flamethrower has its own. [game 0074d610]
    pub flame_hits: weapons::FlameHits,
    /// The flame's spheres lit last update, for its effects and loop (`tick`).
    pub fx_lit: usize,
    /// The flame's ray met a body last update (its impact effect is running).
    pub fx_impact: bool,
    /// The held list's index of its parent (a left-hand weapon: `weaponParent`) or of its one
    /// child (`weaponChildren`), linked at load as the manager's fill does (`007b0f10`).
    /// [game] A child is never the weapon in hand (the switches `007b3ea0` / `007b4080`
    /// skip a weapon with a parent).
    pub parent: Option<usize>,
    pub child: Option<usize>,
    /// A parent's turn with an alternating child (`+0x39c`): 0 the parent fires, 1 the child.
    pub turn: u32,
    /// Its slot in the manager's list. The game numbers the owner's Weapon components in the order
    /// `weapon_manager_fill` 007b0f10 walks them (`iVar12`, l.100-102: the scene table's chain walk
    /// 00578270, stored at the weapon object's `+0x30`), not in the pack's order. The port numbers
    /// them by the pack's list (`enumerate` in `Arsenal::new`), which agrees only if the chain's
    /// order is the pack's (`Arsenal::load`, `enumerate` over the defs). [assumed] The chain walk 00578270
    /// (scene_table_iter, key 0x12936d90) yields the nodes of one key in the table's chain order, and that order
    /// is taken to be the insertion order; the inserter that adds the Weapon components was not found (00587100
    /// and 00578210 are not it). settles: the function that inserts into the Scene hash (writes bucket +0x90 and
    /// size +0x94) for a Weapon component, and whether it appends or prepends in a chain; or a recording of the slot order.
    pub slot: i32,
    /// The sound handles the weapon keeps (`KeptSounds`).
    pub sounds: KeptSounds,
}

/// The sound handles a weapon keeps [game 007a9b40, 007a9ec0, 007aa070]: its fire sound
/// (`+0x37c`) and ring-off (`+0x384`), kept only for an automatic weapon whose event is infinite
/// (else each firing's is fire-and-forget); its overheat sound (`+0x374`); its idle loop (`+0x38c`).
#[derive(Clone, Copy, Debug, Default)]
pub struct KeptSounds {
    fire: Option<crate::sound::Handle>,
    ring: Option<crate::sound::Handle>,
    overheat: Option<crate::sound::Handle>,
    idle: Option<crate::sound::Handle>,
    /// The charged shot's loop (`chargeLoopSound`, ref `+0x440`). [game 007b08a0]
    charge: Option<crate::sound::Handle>,
}

impl KeptSounds {
    /// `charge_sound_start_loop` 007b08a0, at the charge's start: the loop starts only while
    /// none is kept, and is always kept (no infinite test, unlike the fire sound), with the
    /// weapon's heat (`+0x3c8`) on its `heat` parameter. (The one-shot `chargeStartSound` of
    /// the same call is played by the caller.) [game]
    fn charge_start(&mut self, def: &WeaponDef, heat: f32, sound: Option<&dyn WeaponSound>) {
        if def.charge_loop_sound != 0 && self.charge.is_none() {
            self.charge = sound.and_then(|s| s.start_kept(def.charge_loop_sound, Some(heat)));
        }
    }

    /// `charge_loop_stop` 007b0aa0 (slot `+0x150`), from the release (`007af210`, after the
    /// release's sound) and from the cancel (`007add20`): a key-off (`sound_event_stop`
    /// 00595c90), not a fade, so the charge loop runs on past its sustain point into its own
    /// end (`WPN_OPTI_GUN_CHARGE`'s and `WPN_MEGA_BEAM_CHARGE`'s volume falls to 0 along
    /// `param01`; Long Haul's along `param02`). The full charge (`007b0520`) does not stop it.
    /// [game]
    fn charge_stop(&mut self, sound: Option<&dyn WeaponSound>) {
        if let Some(handle) = self.charge.take() {
            sound.inspect(|s| s.key_off(handle));
        }
    }

    /// `gun_fire_sound_start` 007a9b40, once per firing: the fire sound (with the weapon's heat
    /// before this round on its `heat` parameter) and the ring-off (no parameter), each started
    /// only while none is kept, and kept when the weapon is `automaticFiring` and the event is
    /// infinite (`com_query_call` 00775f60 < 0). So the gatling, the Audi's car gun and the flame
    /// start their loop once per pull, and one-shots play each firing. [game]
    fn fire(&mut self, def: &WeaponDef, tuning: &WeaponTuning, heat: f32, sound: Option<&dyn WeaponSound>) {
        let Some(s) = sound else { return };
        for (id, slot, heat) in [(def.fire_sound, &mut self.fire, Some(heat)), (def.ring_off_sound, &mut self.ring, None)] {
            if id != 0 && slot.is_none() {
                let handle = s.start_kept(id, heat);
                if tuning.automatic_firing && s.is_infinite(id) {
                    *slot = handle;
                }
            }
        }
    }

    /// `gun_fire_sound_stop` 007a9d40: a key-off of the kept fire and ring-off sounds
    /// (`sound_event_stop` 00595c90), which lets a sustain loop run on into its tail. [game]
    fn cut(&mut self, sound: Option<&dyn WeaponSound>) {
        for handle in [self.fire.take(), self.ring.take()].into_iter().flatten() {
            sound.inspect(|s| s.key_off(handle));
        }
    }

    /// `weapon_overheat_start` 007a9ec0: the fire sounds are let go first (slot `+0x5c`), then
    /// the overheat sound starts and is kept. [game]
    fn overheat(&mut self, def: &WeaponDef, sound: Option<&dyn WeaponSound>) {
        self.cut(sound);
        if def.overheat_sound != 0 {
            if let Some(old) = self.overheat.take() {
                sound.inspect(|s| s.stop(old));
            }
            self.overheat = sound.and_then(|s| s.start_kept(def.overheat_sound, None));
        }
    }

    /// 007a9fe0, as the lock runs out: the kept overheat sound is keyed off when the weapon's
    /// `fireOverHeatKeyOff` is set, else stopped with its fade (`Event::stop(0)`). [game]
    fn cooled(&mut self, def: &WeaponDef, sound: Option<&dyn WeaponSound>) {
        if let (Some(handle), Some(s)) = (self.overheat.take(), sound) {
            if def.overheat_key_off { s.key_off(handle) } else { s.stop(handle) }
        }
    }

    /// 007aa070: the idle loop starts while none is kept (`weaponIdleSound`). [game]
    fn idle_start(&mut self, def: &WeaponDef, sound: Option<&dyn WeaponSound>) {
        if def.idle_sound != 0 && self.idle.is_none() {
            self.idle = sound.and_then(|s| s.start_kept(def.idle_sound, None));
        }
    }

    /// 007aa110: the idle loop stops with its fade (`Event::stop(0)`). [game]
    fn idle_stop(&mut self, sound: Option<&dyn WeaponSound>) {
        if let Some(handle) = self.idle.take() {
            sound.inspect(|s| s.stop(handle));
        }
    }

    /// Everything stopped at once (a reset), with fades; the fire sounds keyed off.
    fn silence(&mut self, sound: Option<&dyn WeaponSound>) {
        self.cut(sound);
        // [game 007b0aa0] `charge_stop` is the key-off of the kept charge loop (+0x440). [assumed] a reset
        // ends the charge loop that way. The game's callers of the put-away 007b2eb0 are state_weapon_exit
        // 0087f6f0 and state_dash_exit 0087abc0 (and 007b1440, the once-only setup), and 007b2eb0 calls
        // only the slots' vtbl+0x1c(0), +0x28 and +0x110 (no `charge_stop` itself), so a reset reaching the
        // charge loop is not read. settles: the weapon-class vtable's slots +0x1c and +0x28 (the put-away's
        // targets, the class not located) and the reset's caller in the character respawn path (not read).
        // Read 2026-10-20 (wave 24 L2, lead-checked): slot +0x1c is weapon_set_out 007a75a0: with 0 it calls +0x68 and
        // +0x70 (the overheat and idle sound stops) and forwards +0x1c to its child (+0x394, the partner weapon of the same
        // class); slot +0x28 is weapon_hide 007a7750, +0x110 007ab1b0 (the exit clip). None reaches charge_stop 007b0aa0
        // (slot +0x150), so a PUT-AWAY leaves the charge loop to its own key-off; the reset path itself is still the read.
        self.charge_stop(sound);
        self.idle_stop(sound);
        if let Some(handle) = self.overheat.take() {
            sound.inspect(|s| s.stop(handle));
        }
    }
}

pub struct Tracer {
    pub from: Vec3,
    pub to: Vec3,
    pub age: f32,
}

pub struct Impact {
    pub at: Vec3,
    pub age: f32,
    /// A missile's, drawn larger.
    pub explosive: bool,
}

/// The turret specials' weapon names (the CRC-32 of each name, `WeaponDef::name`) [data]: Blackout's
/// (`Blackout_SpecialDeployableTurret`) and Ironhide's (`Ironhide_SpecialDeployableTurret`).
const BLACKOUT_SPECIAL: u32 = 0x5F35_6648;
const IRONHIDE_SPECIAL: u32 = 0xEFFA_D9D4;
/// Ironhide's car form's turret launcher (launchSpeed -50, thrown back); its turret carries the same
/// MissileLauncher, FireTurret and TurretRotator as the foot one's [data, `tf2-core`
/// `ironhides_turret_carries_a_launcher_and_aims_from_its_data`].
const IRONHIDE_CAR_SPECIAL: u32 = 0x1BF3_4BF5;
/// Blackout's car form's turret (`Blackout_VehicleSpecial`'s name, the car weapon of his special) [data].
const BLACKOUT_CAR_SPECIAL: u32 = 0x1FE6_CA2F;
/// `Blackout_TurretGatlingAttribute`, the gun in Blackout's turret: its `gunAttribute` hash
/// (`blackout_turret`'s `GunWeapon`, `notes\special-weapons.md` section 2) [data].
const BLACKOUT_TURRET_GUN: u32 = 454_803_683;
/// `FireTurret` of `blackout_turret`: the band of its loader 0079e250 [data work/survey/weapons/
/// blackout.txt 720724-720728]: `minFireRange` 0, `maxFireRange` 300, `maxFireAngle` and
/// `maxFireElevation` 360 degrees (no limit inside a whole turn), `coneAngleToAllowFire` 15 [data].
/// Each turret's own are read from its object (`turret::read_aiming`); these, with
/// `BLACKOUT_ROTATOR` and `BLACKOUT_FIRE_RATE`, stand for a turret object the reader cannot reach.
const TURRET_BAND: turret::FireBand = turret::FireBand {
    min_range: 0.0,
    max_range: 300.0,
    max_yaw: std::f32::consts::TAU,
    max_pitch: std::f32::consts::TAU,
};
const TURRET_CONE_DEG: f32 = 15.0;
/// Blackout's TurretRotator: maxTurnRateLeftRight 70, turnAccelerationLeftRight 70,
/// maxTurnRateUpDown 50, turnAccelerationUpDown 50, in degrees [data] [game 008139b0].
const BLACKOUT_ROTATOR: ([f32; 2], [f32; 2]) = ([70.0, 50.0], [70.0, 50.0]);
/// Blackout's FireTurret `fireRate` [data].
const BLACKOUT_FIRE_RATE: f32 = 100.0;
/// How far along its barrel a turret's round is aimed when there is no aim point (the trace is cut at
/// the gun's own range). [assumed] The fallback only: a burst that outlives its target keeps this line.
/// The game's point with no candidate is different (owner place + 1000 x row, 0079e920 l.94-103, below);
/// settles: the OPEN point below (the owner's place and row in `TurretOwner`).
/// The round's direction is the aim point, not the barrel: `fire_turret_fire` 0079f0f0 hands the
/// weapon's trigger the FireTurret's aim point `ft+0x80` (`(vtbl+0xc4)(ft+0x80, ...)`), and the round
/// runs from the muzzle towards the point it was sent at (007832b0). [game 0079f0f0, 007832b0]. The
/// barrel follows the same point through the WeaponRotator (`turret_rotator_update` 00813e30 aims at
/// it). [game 0079e920 writes `+0x80`: the nearest candidate's place, or its Head node when `+0x7c`.]
/// With no candidate the game's point is the owner's place plus 1000 x its row at `+0x1c/+0x20/+0x24`
/// [game 0079e920 l.94-103, 2026-10-20]; the port's no-target line is the rotator's facing at this distance.
/// The "owner" there is the object at the FireTurret component's `+0xc`: lux components keep the object they
/// are on at `+0xc` (00577b40), so it is the turret object, its place and its +y row (its rest facing). The port
/// now aims a target-less round at `body.position + rotator.rest x 1000` (2026-10-20; before: muzzle + the live
/// rotator facing). [assumed: the port's turret body place and `rest` are that object's place and +y row] settles: the turret object's frame against the landing's turn (RD2f6c66).
const TURRET_AIM_DISTANCE: f32 = 1000.0;

/// A turret's gun: the weapon of the turret's tree (Blackout's `GunWeapon`, Ironhide's
/// `MissileLauncher`), its numbers, its heat, and its burst (`burstPerFire` rounds,
/// `timeBetweenEachBurst` apart) [data].
#[derive(Clone)]
pub struct TurretGun {
    def: WeaponDef,
    tuning: WeaponTuning,
    state: Weapon,
    rounds: u32,
    gap: f32,
    /// A launcher's place among the special weapons (`weapon_in`'s index), which its missiles in
    /// flight look their numbers up by; None for a gun, whose rounds are traced at once.
    launcher: Option<usize>,
    /// The turret object's node its rounds leave from: the weapon's first `FireDummy`
    /// (`minFireDummyIndex`), 0 when not read. [data]
    pub barrel: u32,
}

/// The turret special's setup: the launcher's numbers, the deploy time after landing, the
/// turret's gun and aiming parts (`FireTurret`, `TurretRotator`), and the launcher's name (whose
/// model pool draws it).
#[derive(Clone)]
struct TurretSetup {
    name: u32,
    launcher: LauncherTuning,
    deploy_time: f32,
    gun: Option<TurretGun>,
    aiming: turret::Aiming,
}

/// What a deployed turret carries beside its body (`turrets`, index for index): its rotator,
/// its fire timer, its gun, and the burst of the gun's rounds in progress.
pub struct Gear {
    /// The launcher that threw it (its model pool, `TurretModel`), and its base's heading: the
    /// throw's, turned half round [game 0078c2e0]; the base keeps it, the `WeaponRotator` bone
    /// turns. [game 0078b8e0] The landing's contact response (read to the end) builds a frame from the
    /// contact normal and the velocity and writes it into the turret object's transform (`+0x10..+0x3c`),
    /// so the base does turn on landing; for a level contact its heading is the throw's horizontal way, which
    /// is what this keeps. Not the same for a sloped landing (not modelled).
    pub weapon: u32,
    pub heading: f32,
    pub rotator: Rotator,
    pub fire: FireTimer,
    /// Its FireTurret's band and `coneAngleToAllowFire` (degrees). [data]
    pub band: turret::FireBand,
    pub cone_deg: f32,
    pub gun: Option<TurretGun>,
    /// Where its gun's `barrel` node was when the turret was last drawn (`draw_turrets`, from the
    /// posed model: the pods turned by the rotator); None before that, or with no model.
    pub muzzle: Option<Vec3>,
    pub burst: Burst,
    pub fired_once: bool,
    /// The clip its gun last started on the turret's object, and the turret clock then: `Fire`
    /// at each firing (weapon_fire 007ae010 l.422 -> slot +0x58 `gun_fire_sound_start` 007a9b40
    /// -> slot +0x11c `weapon_start_fire_clip` 007ab4a0: the GunWeapon's `fireAnimation`, no clip
    /// after it, restarted each time), `Overheat` when it overheats (007a9ec0 -> slot +0x120
    /// 007ab3e0, `overheatAnimation`). The object's clip player holds a finished clip that has
    /// nothing to play after it (007c4170). [game; the slots are the Gun table 00b5fa68's]
    pub clip: Option<(&'static str, f32)>,
}

/// Whose turrets these are, for `Arsenal::tick_turrets`: the owner's team (the enemies are the
/// others) and whether he is alive (a turret goes when he is not). [game]
#[derive(Clone, Copy, Debug)]
pub struct TurretOwner {
    pub alive: bool,
    pub team: u32,
}


/// The next number from 0 to 1 of a small generator.
fn roll(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    (*seed >> 8) as f32 / (1u32 << 24) as f32
}

/// A round that landed on a target in the last tick, for whoever owns the targets.
#[derive(Clone, Copy)]
pub struct Struck {
    pub target: u32,
    pub packet: crate::damage::Packet,
}

/// [port design] One of Grindor's cinematic waves (`crate::energon_wave`): the ring arc a
/// shot of his cannon opens where its missile ends, with a number of its own (from `launched`)
/// and the number of the snaker in its middle, whose look and sound stay and whose own hit is
/// off (the ring hurts in its place), the ground under it (`energon_wave::Ground`: what it climbs
/// and what stops it) and where the missile ended. `energon::dress` dresses it.
pub struct EnergonWave {
    pub number: u32,
    pub snaker: u32,
    pub ring: crate::shockwave::ShockWave,
    pub ground: crate::energon_wave::Ground,
    pub impact: Vec3,
}

/// How far along the line from `from` to `to` (0 to 1) it enters a target's body.
/// [stand-in: the body is one upright cylinder, as for melee; the original traces its
/// collision bodies, each with its own surface (head, upper body...)] settles: the collision
/// shapes of a target with no `TargetParts` (the scene's Character bodies, walked by 007832b0; not read).
fn enters(from: Vec3, to: Vec3, target: &Target) -> Option<f32> {
    let along = to - from;
    let (flat, offset) = (along.truncate(), (from - target.pos).truncate());
    let a = flat.length_squared();
    let mut best: Option<f32> = None;
    if a > 1e-9 {
        let b = offset.dot(flat);
        let c = offset.length_squared() - target.radius * target.radius;
        let root = b * b - a * c;
        if root >= 0.0 {
            let k = (-b - root.sqrt()) / a;
            let z = from.z + along.z * k - target.pos.z;
            if (0.0..=1.0).contains(&k) && (0.0..=target.height).contains(&z) {
                best = Some(k);
            }
        }
    }
    // Its top, from above.
    let top = target.pos.z + target.height;
    if from.z > top && to.z < top {
        let k = (from.z - top) / (from.z - to.z);
        if ((from + along * k) - target.pos).truncate().length() <= target.radius && best.is_none_or(|b| k < b) {
            best = Some(k);
        }
    }
    best
}

/// Where a gun round's line strikes a target: (its first entry, where the hit is taken, the part),
/// as shares of the line. On the target's posed bodies when `targets::animate` gave them
/// (`weapons::part_struck`) [game 007832b0]; else on its cylinder as an upper body [stand-in].
/// [game 007832b0, read to the end 2026-10-20] The game's round is `world_ray_sphere_query` 005be890
/// over the PUNCH mask 2, walking the hits by distance: a hit's part is its body's material (`+0x254`,
/// Head = 0x7c159a2), and a later Head of the same owner takes the hit; the damage is `bullet_damage`
/// 00782b30. The game has no cylinder: a target's hit bodies are its collision shapes, so the cylinder is
/// a stand-in for a target whose collision bodies the port does not build (no model case). The no-model
/// case settles: the target's collision shapes for that case (the scene's Character bodies, not read here).
/// [assumed] the bodies are a frame old (posed by the last `animate`), placed where it stands now.
/// settles: the per-frame order of 007832b0's caller (the gun's trace) against the target's clip
/// update (the `animate` side), read from the weapon tick's caller chain; the cylinder fallback
/// only matters for a target `targets::animate` skipped. [game] `animate` builds a part set for every
/// EnemyModel whose owner is an EnemyTarget (targets.rs 700-702 skip the rest, 762 pushes); so the cylinder
/// is reached only for a target with no such model (not yet spawned, or none at all). Settled as to the
/// rule; the no-model case is still the port's stand-in.
fn round_enters(from: Vec3, to: Vec3, target: &Target, parts: &[TargetParts]) -> Option<(f32, f32, Part)> {
    match parts.iter().find(|p| p.id == target.id) {
        Some(parts) => weapons::part_struck(from, to, &parts.placed(target.pos)).map(|hit| (hit.first, hit.at, hit.part)),
        None => enters(from, to, target).map(|k| (k, k, Part::UpperBody)),
    }
}

/// The id a Destructable's rounds are struck under (`Struck::target`) is `PROP_BASE` plus its object id. The high
/// nibble tells it from an enemy's id, so the enemies' own test (`targets::update`) never matches it. [assumed, ours]
pub const PROP_BASE: u32 = 0x4000_0000;

/// A Destructable the rounds can hit: its place and radius in the game's space. [stand-in] the radius is
/// `crate::destructable::RADIUS_M` (3 m, the template's Collision bounds not read); the place is the
/// runner's, or a stand-in (`mission.rs`). Set by the mission each frame (`Arsenal::props`); a round that
/// lands on one stops there. settles: the destructable template's Collision bounds (destructable.rs
/// `RADIUS_M`); the mission side is dormant (archive\README.md), so this waits for a mission to matter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prop {
    pub id: u32,
    pub pos: Vec3,
    pub radius: f32,
    /// The Instance that holds the Destructable, when the level has its collision (`LevelWorld::owner_is_solid`): the
    /// round then tests that Instance's collision primitives, not the sphere. None: the sphere (stand-in).
    pub owner: Option<crate::collision_world::Owner>,
}

/// A Destructable's collision hit takes the round when it is no farther than the level's own nearest hit plus this
/// much: the Instance's shape and the level's collision are the same surface, so the two distances agree to float
/// error. [assumed] 5 cm, the same slack as `collision_world::RAY_TOL`. settles: no game value
/// exists for a port tolerance; it is ours. The test it guards (owner and sphere agree) is for a mission's
/// Destructable, dormant (archive\README.md).
const PROP_OWNER_TOL: f32 = 0.05;

/// How far along the line from `from` to `to` (0 to 1) it enters a sphere; None when it does not, or when it starts
/// inside (the first entry is the one that counts). [game] the round's test is not read: [stand-in] a sphere.
/// settles: the Destructable's hit test (the round's trace through `00724700`'s target side, not read); dormant
/// with the missions.
fn enters_sphere(from: Vec3, to: Vec3, prop: &Prop) -> Option<f32> {
    let along = to - from;
    let a = along.length_squared();
    if a <= 1e-12 {
        return None;
    }
    let offset = from - prop.pos;
    let b = offset.dot(along);
    let c = offset.length_squared() - prop.radius * prop.radius;
    let root = b * b - a * c;
    if root < 0.0 {
        return None;
    }
    let k = (-b - root.sqrt()) / a;
    (0.0..=1.0).contains(&k).then_some(k)
}

/// The weapon manager's aim point (`weapons::aim_point`) with scouts in the line's way
/// as well as the arena: the original's trace (mask 3) meets characters too. [game]
/// [stand-in: a scout target's body is one upright cylinder]
fn aim_point(eye: Vec3, forward: Vec3, body: Vec3, world: &dyn Sight, targets: &[Target]) -> Vec3 {
    let aim = weapons::aim_point(eye, forward, body, world);
    let from = eye + forward * (body - eye).dot(forward).clamp(0.0, weapons::AIM_REACH);
    let far = eye + forward * weapons::AIM_REACH;
    let mut nearest = (aim.point, aim.distance);
    for target in targets {
        let Some(k) = enters(from, far, target) else { continue };
        let distance = k * (far - from).length();
        if distance < nearest.1 {
            nearest = (from + (far - from) * k, distance);
        }
    }
    nearest.0
}

/// The car's side of a frame of the weapons.
pub struct Car<'a> {
    /// His control mode is the car's (from the start of the change into it), and he is in
    /// the car's form.
    pub mode: bool,
    pub formed: bool,
    /// `vehicleWeaponTimeTilCanFire`.
    pub wait: f32,
    /// Where the barrels of the car's weapon are, and the way the car points, in the
    /// game's space.
    pub barrels: &'a [Vec3],
    pub forward: Vec3,
    /// The `Flipper` node's +y row in the game's space (None: the model has no such node).
    pub flipper: Option<Vec3>,
    /// The camera target point (`0071a2e0`): his root translation in the car. [game]
    pub target_point: Vec3,
}

pub struct Arsenal {
    pub held: Vec<Held>,
    /// The car's weapons (`isVehicleModeWeapon`): in hand while his control mode is the
    /// car's, with the manager's wait before they fire.
    pub car: Vec<Held>,
    car_wait: VehicleWait,
    /// The car weapon that fires: always the first but for Devastator, whose pods ride this path
    /// (`load_giant`) and are switched through with `switch_car`; and its switch's last state.
    car_current: usize,
    car_switch_was_down: bool,
    car_rotates: bool,
    /// Devastator's bug bombs (the car weapon at this index) erupt (`crate::giant::Lob`), and
    /// their missiles' arcs by number. [assumed port design]
    volcano: Option<usize>,
    lobs: std::collections::HashMap<u32, crate::giant::Lob>,
    /// His missiles' top-attack paths by number (`crate::giant::TopAttack`). [assumed port design]
    dives: std::collections::HashMap<u32, crate::giant::TopAttack>,
    /// The way his back launcher points this frame (set by the player from his pose): the bombs leave along it.
    pub vent_axis: Vec3,
    /// His eruptions so far and where the last left (the middle of its launch points), for its
    /// launch blast, smoke and shake (`giant::dress`). [assumed port design]
    pub eruptions: u32,
    pub eruption_at: Vec3,
    /// His firings so far, of any pod kind (`giant::dress` roars at the start of a burst).
    pub giant_firings: u32,
    /// His missiles' and bombs' blasts this update (where, and whether a bomb's), which
    /// `giant::dress` shakes the camera for. [assumed port design]
    pub giant_blasts: Vec<(Vec3, bool)>,
    /// Seconds since his last switch of pod kind: his reticle's weapon icon comes up afresh.
    giant_icon: f32,
    /// Devastator's rounds of this update (from, to, speed), which `giant::tracers` takes.
    pub giant_rounds: Vec<(Vec3, Vec3, f32)>,
    in_car: bool,
    current: usize,
    switch_was_down: bool,
    /// The missiles in flight: the weapon each left, a number of its own (what its
    /// trail is kept under) and the missile.
    pub missiles: Vec<(usize, u32, Missile)>,
    /// A missile's damage scale fixed at its launch, by its number, where it is not 1: the
    /// type-6 special's `starscreamSpecialDamageMultiplier` (00785de0). [game]
    missile_scale: std::collections::HashMap<u32, f32>,
    /// The missiles launched as the weapon's alternate projectile (`chargeMaxProjectileObj`,
    /// `weapons::fire_choice`), by number: their trail, sounds and blast are that object's. [game 007aaeb0]
    alt_rounds: std::collections::HashSet<u32>,
    /// Set each frame by the player: `Some(multiplier)` while his own type-6 special runs
    /// (Starscream's; `sim::State::slowing_special`). Then his rounds carry `SLOW` (bullet_damage
    /// 00782b30) and his missiles' damage words are scaled (00785de0). [game]
    pub type6_special: Option<f32>,
    /// Set each frame by the player: he is in Megatron's special (type 4), which cancels a
    /// charging weapon (`007add20` from `007ac370`). [game 007ac370 l.93-101: the character's state is
    /// StateAttackSpecial (-0x23db0e9f) and the special's type is 4]; the player asks the control state's
    /// class for exactly that (`state_class(t) == Some("StateAttackSpecial")`, player.rs). [data 2026-10-20] The special
    /// states' exits (`export\data\global_states.txt`, connectionListAttackSpecial l.3942) are two
    /// connections: [0] connectionAttackSpecial, a ConnectionControlDetect on the special button (includeHitButton
    /// 0x2, l.1445), and [1] the object at 0x7cd8, which is `connectionEnvInsideFlightBoundary` (a ConnectionEnv
    /// on envTest #f65c039d, `INSIDE_FLIGHT_BOUNDARY`, `notes\flight-boundary.md` section 8; the root's name from
    /// its header in `bnxglobal.str#16`). The object at 0x7d40 (`connectionEnvNotHoldVIP`) is in the list above
    /// it, connectionListAttackJumpGroundPound (l.3939), not in this one. [data] No connection here is a timer or
    /// a put-away. [game 00879700, 2026-10-20] Any exit of StateAttackSpecial runs its exit handler, which puts
    /// the melee weapon away (`melee_weapon_put_away` 007b2d80) only when the state is ordinary (+0x2c 0) with
    /// finishAnim (+0x44); a taken [1] therefore does nothing to the weapon beyond that exit. [assumed] The
    /// priority of [1] among the state's other exits is not read: the list that StateAttackSpecial checks is not
    /// identified (global_states.txt 4728-4736 holds a list of shared connections, not yet tied to this state).
    /// settles: the state's connection list (the StateAttackSpecial exit list in the state data, global_states.txt).
    pub charge_cancel: bool,
    /// The owner is dead (his `+0x148` bit 3): set by the player before `tick`, handed to each weapon
    /// (`Weapon::set_owner_dead`). [game 007ac370 l.41, 007204a0 l.487]
    pub owner_dead: bool,
    /// The pad's rumble of this frame's charge step (`weapons::charge_rumble`), taken by the
    /// player with the round's rumble. [game 007addc0]
    pub charge_rumble: Option<(u32, f32, f32)>,
    /// Set each frame by the player: he is sniping (`camera_is_sniping` 0117e860, `crate::snipe`).
    /// The weapon in hand's snipe clock runs by it (`weapon_is_in_hand` 007a7470); its rounds
    /// are traced from the camera. [game 007ac370, 007832b0]
    pub sniping: bool,
    /// His animation rate (`sim::State::special_rate`, set by the host each frame): the gun's arm
    /// and object players run at it, as every layer player of his AnimController and his weapon
    /// object's do (`character_speed_scale_set` 0071f4d0 -> 00748dc0). [game]
    pub anim_rate: f32,
    /// The rifle is hidden by the scope (`character_set_sniping` 00721ce0: the weapon in hand's
    /// object is hidden going in, re-shown by `weapon_show` 007a7630 on a toggle leave). Set by
    /// the player with `crate::snipe::rifle_hidden`. [game] [assumed] cleared on every
    /// leave, the forced one included. 00721ce0's off path with reshow 0 (the forced leave) calls no
    /// `weapon_show`, so the port's clear relies on the weapon switch that caused the leave showing or
    /// hiding the object itself. [game 2026-10-20] The reshow-0 callers of `character_set_sniping` (raw
    /// disassembly, the pushes before each call): 0071df3a (character_update, leaving StateWeapon without
    /// staying in it), 0071e6ec (character_update, second forced leave), 0071abe8 (character_vehicle_special,
    /// the car form), 00720eab (character_kill), 007157c2 (device_to_ai). The toggle is 0071dfc0 (reshow 1).
    /// The off path with reshow 0 never calls `weapon_show`. [assumed] the object's visibility after a forced
    /// leave is left to the put-away (007b2eb0 from state_weapon_exit 0087f6f0, which only the 0071df3a leave
    /// reaches; the car form and the death leave it to whatever they do). [game 2026-10-20] `weapon_hide` 007a7750 and
    /// `weapon_show` 007a7630 have no direct E8 caller (`work\find_call.py` finds none for either), so they run only
    /// through a weapon object's vtable (slots +0x24 / +0x28). [game] The vtable of the gun class (00b5fba0, and
    /// the same slots in 00b5fa68) gives slot +0x24 = `weapon_show` 007a7630 and slot +0x28 = `weapon_hide` 007a7750
    /// (`pe_read.py vtable`); the same two functions also sit at slots +0x0c / +0x10 of vtable 00b5db00
    /// (checked 2026-10-20, `pe_read.py vtable 0x00b5db00`), so a second class reaches them too, not read
    /// further. [game 007b2eb0, read whole 2026-10-20] The put-away calls each weapon object's slot
    /// +0x1c with 0 (`weapon_set_out` 007a75a0), its +0x28 (`weapon_hide`, when the argument is 0) and its +0x110
    /// (the one in hand, index `+0x74`). So a forced leave that reaches the put-away hides the rifle (unless
    /// weapon_hide's own state test keeps it shown), and the port's clear above is right for that leave. Not read:
    /// the car form's and the death's leave: 0071abe8 is inside character_vehicle_special 0071ab70 and 00720eab inside
    /// character_kill 007204a0 (which hides the body's object, +0x60 bit 0); whether the rifle, the body's child,
    /// is hidden with them is not read. settles: the rifle object's parent link (the weapon in hand's object) and
    /// whether 007204a0's hide reaches it.
    pub scope_hidden: bool,
    /// The thrower's velocity, game space, to be set each frame by the player (his body's, the
    /// car's in the car). A grenade with `tackOnOwnerSpeed` (the Audi's car mine) adds its
    /// component along the throw to `launchSpeed`. [game 00789b80: the owner's first body with
    /// dynamics, `body_linear_velocity` dotted with the throw's direction] Zero until the player
    /// sets it (request to the body package, `notes\standin-pass\weapons.md`).
    pub owner_velocity: Vec3,
    /// The grenades in flight, kept as the missiles are.
    pub grenades: Vec<(usize, u32, weapons::Grenade)>,
    /// Ground waves (`SnakerProjectile`) let go where a missile that carries one ended: the
    /// weapon it came from, a number of its own (from `launched`: what its effects ride), the wave,
    /// the targets it has struck, and its reach round its point (the wave object's Collision sphere).
    snakers: Vec<(usize, u32, weapons::Snaker, Vec<u32>, f32)>,
    /// The ground waves (by number) whose SoundPlayers have been started (`wave_sounds`).
    wave_sounding: std::collections::HashSet<u32>,
    /// [port design] Grindor's cinematic waves alive, and the ring they scale (Devastator's,
    /// `set_energon`): none without it.
    energon: Vec<EnergonWave>,
    energon_ring: Option<crate::shockwave::Def>,
    /// The pieces a round's blast object threw (`crate::debris`, `WeaponEffects::debris`): a number of
    /// its own (from `launched`: what its smoke rides), the round's row key, the piece's index in that row,
    /// its placed turn and uniform scale (game space), and the piece in flight.
    debris: Vec<(u32, u32, usize, Quat, f32, crate::debris::Piece)>,
    debris_rng: crate::debris::Rng,
    launched: u32,
    /// What each round shows and starts as it ends (`CharacterData::weapon_effects`, set by
    /// `set_effects`): its blast rumble and its ground wave's numbers.
    round_fx: Vec<crate::character::WeaponEffects>,
    /// The blast rumbles started this tick, where they started (the round's `blast_rumble`, the
    /// wave's): taken by the player, who scales each by his distance. [game 007dba80, 007dbdc0]
    pub rumbles: Vec<(Vec3, crate::rumble::RumbleDef)>,
    /// The special's shots (`crate::special_weapon`), and the weapons of its two forms as
    /// held objects, which come after the held list (`weapon_in`). `special_slot` is each form's
    /// index (`[foot, car]`), `special_form` the form of the last start, `special_seen` the
    /// starts counted so far (`sim::State::special_starts`). [game 007b3d80]
    special: Option<SpecialFire>,
    pub special_weapons: Vec<Held>,
    special_slot: [Option<usize>; 2],
    special_form: usize,
    special_seen: u32,
    /// The special clip whose fire events were last walked: its id (CRC-32 of its name) and the
    /// tick it had reached (the window's start, as `axe_clip` is for the melee weapon).
    special_clip: Option<(u32, f32)>,
    /// Each special weapon's object (index for index with `special_weapons`), shown, hung and
    /// posed by the special clips' `WeaponAttachmentEvent`s that name it, as the melee weapon's
    /// is by the attack clips' (`Axe`). [data: Megatron's `SpecialAttack_Intro` t=10 unhides
    /// `MegatronSpecialGun` on WeaponAttachment_Hand_R playing `Trans_R2W` then `Idle`,
    /// `SpecialAttack_Exit` t=18 plays `Trans_W2R`, hidden when done] [game 007b4680: an event
    /// that is not "only if in use" counts for its weapon whether or not it is in hand]
    special_objects: Vec<Axe>,
    /// The deployable turret of the special (`TurretLauncher`), when his special throws one.
    /// [game 0078c2e0, 0078b0a0] `turrets` are the deployed ones and `gear` (index for index)
    /// their rotator, fire timer and gun. `turret_clock` is the game time, seconds.
    turret: [Option<TurretSetup>; 2],
    pub turrets: Vec<Turret>,
    pub gear: Vec<Gear>,
    pub turret_clock: f32,
    /// The explosions missiles have made, while their spheres last.
    pub blasts: Vec<MissileBlast>,
    /// The launcher's lock, the targets the aim assist has in its box, and what is drawn
    /// round a target being locked or locked.
    pub lock: LockOn,
    seen: [Option<u32>; 3],
    /// The reticle (`crate::reticle`): its own state, the bracket and the lock's share
    /// found in this frame's search, the seconds since one of his shots hit, the weapons'
    /// icon clock, and the quads of this frame for the HUD to draw.
    reticle: Reticle,
    bracket: Option<reticle::Bracket>,
    lock_shown: f32,
    previous_lock_sound: f32,
    since_hit: f32,
    icon_clock: f32,
    pub sprites: Vec<Sprite>,
    /// A small generator for the scatter of shots.
    seed: u32,
    pub tracers: Vec<Tracer>,
    pub impacts: Vec<Impact>,
    pub rounds: u32,
    pub hits: u32,
    /// The targets the gun's rounds landed on in the last tick.
    pub struck: Vec<Struck>,
    /// The mission's Destructables that rounds can hit, set by `mission::run` each frame (for the next tick). [stand-in]
    /// Only rounds from the gun, the special and the turrets test them; missiles, grenades, flames and melee do not.
    /// settles: the mission runner is dormant (archive\README.md); the Destructable side waits with it.
    pub props: Vec<Prop>,
    /// The enemies' posed collision bodies (Head, UpperBody...), set by `targets::animate` each frame
    /// for the next tick: a round strikes these, and the part picks its multiplier. A target without
    /// an entry is struck on its cylinder as an upper body [stand-in]. [game] `targets::animate` pushes an entry for
    /// every EnemyModel whose owner is an EnemyTarget (targets.rs 700-702, 762), so only a target with no model
    /// lacks one (the arena's scouts have one once spawned).
    pub target_parts: Vec<TargetParts>,
    /// The level's collision, for the props that have an Instance (`Prop::owner`); set by `mission::run` each frame.
    pub level: Option<std::sync::Arc<crate::level_world::LevelWorld>>,
    /// Arena contacts of rounds, kept separately from character damage packets.
    pub surface_impacts: Vec<(u32,crate::camera::Hit)>,
    /// The effects asked for since `effects::start` last took them: a flash where a
    /// round left, its impact, a missile's explosion.
    pub fx: Vec<WeaponFx>,
    /// His weapon layer and the weapon object's own clips.
    pub gun: Gun,
    /// His melee weapons, one per `CharacterData::melee_weapons` (his axe; Optimus's two arm
    /// blades), as the simulation last left them (`sim::State::melee`, `tick_axe`).
    pub axes: Vec<Axe>,
    /// The node each weapon's model was hung on by `attach_models` (weapon name to node name
    /// hash): where its object hangs while it is out, which for Bumblebee's cannon is not its
    /// `startBoneAttachment` (`hang_node`). [game 007b4f20] [data]
    pub hung: std::collections::HashMap<u32, u32>,
}

/// Links each weapon that names a parent to that parent, when the parent names it among its
/// children: the manager's fill (`007b0f10`) gives a weapon with a parent (slot `+0xdc`) to the
/// weapon whose child names (`+0xe0`, `+0xe4`) match it, and that weapon keeps one (`007ab060`
/// stores into a single slot, `+0x394`). [game]
fn link_children(held: &mut [Held]) {
    for child in 0..held.len() {
        let (name, parent) = (held[child].def.name, held[child].tuning.parent);
        if parent == 0 {
            continue;
        }
        let found = held.iter().position(|h| h.def.name == parent && h.tuning.children.contains(&name) && h.child.is_none());
        match found {
            Some(index) => {
                held[index].child = Some(child);
                held[child].parent = Some(index);
                eprintln!("weapon {name:#010x} is the child of {parent:#010x}{}", if held[index].tuning.alternating_fire { ", taking turns" } else { ", firing with it" });
            }
            None => eprintln!("weapon {name:#010x}: its parent {parent:#010x} is not among his weapons"),
        }
    }
}

/// A weapon's first barrel, for finding where its rounds leave (`Arsenal::barrel`): the
/// weapon (its model is the one whose list names it), the barrel node's name in its object,
/// the node of the robot or car the object hangs on, and the barrel's rest place in the
/// object's space.
#[derive(Clone, Copy, Debug)]
pub struct Barrel {
    pub weapon: u32,
    pub node: u32,
    pub hung: u32,
    pub at: Vec3,
}

/// Where the car's weapons start among the slots a round keeps (`weapon_in`): far past the
/// held and special lists, so a special equipped later does not move them.
const CAR_SLOT: usize = 1 << 20;

/// The weapon a round was thrown from: an index of the held list, or past it one of the
/// special's (`Arsenal::special_slot`), or from `CAR_SLOT` on one of the car's.
pub fn weapon_in<'a>(held: &'a [Held], specials: &'a [Held], car: &'a [Held], index: usize) -> &'a Held {
    if index >= CAR_SLOT { &car[index - CAR_SLOT] } else if index < held.len() { &held[index] } else { &specials[index - held.len()] }
}

/// `facing` turned about the up axis by `xy_deg`, then tipped up by `z_deg` (the game's
/// `fireDummy` angles). [game 00570770 + 0052eb40, emulated offline: `work\emu\fire_angle_signs.py`,
/// notes\special-jetfire-mines.md "fireDummy angle signs (emulated)", 2026-10-20] The sign: a positive z tips
/// UP and a positive xy turns LEFT (counter-clockwise about up), as here. [assumed] the facing is the barrel
/// frame's forward: the game tips about the frame's first row and then yaws about the global z (stored R*B),
/// this turns yaw first and tips about the yawed right. The two agree for the frame's forward, not for other
/// facings. settles: the muzzle frame 007a7a40 (`weapon_muzzle_transforms`) for the facing it passes, read
/// on the flame's or Jetfire's barrel.
fn turn(facing: Vec3, xy_deg: f32, z_deg: f32) -> Vec3 {
    let yawed = Quat::from_rotation_z(xy_deg.to_radians()) * facing;
    let right = yawed.cross(Vec3::Z).normalize_or(Vec3::X);
    (Quat::from_axis_angle(right, z_deg.to_radians()) * yawed).normalize_or(yawed)
}

pub const WEAPON_LAYER_SET: &str = "WeaponPartialSet";
/// The special's clips (Intro, Loop, Exit). [data]
const SPECIAL_SET: &str = "SpecialAnimSet";

impl Arsenal {
    pub fn receive_heat(&mut self, packets: impl IntoIterator<Item=(u32,f32)>) {
        for (flags,heat) in packets {
            if flags & crate::damage::flags::OVERHEAT_CURRENT!=0 {
                if self.in_car {for held in &mut self.car {held.state.add_heat(&held.tuning,heat);}}
                else {let held=&mut self.held[self.current]; held.state.add_heat(&held.tuning,heat);}
            } else if flags & crate::damage::flags::OVERHEAT_ALL!=0 {
                for held in &mut self.held {held.state.add_heat(&held.tuning,heat);}
                for held in &mut self.car {held.state.add_heat(&held.tuning,heat);}
            }
        }
    }
    /// His robot-form weapons, or `None` (said on the console) if the install did not give them.
    pub fn load(game_dir: &Path, defs: &[WeaponDef]) -> Option<Arsenal> {
        let tunings = match weapons::load(game_dir, defs) {
            Ok(tunings) => tunings,
            Err(e) => {
                eprintln!("weapons: not loaded ({e})");
                return None;
            }
        };
        for (def, tuning) in defs.iter().zip(&tunings) {
            eprintln!(
                "weapon {:#010x} ({:?}{}): a round every {} s, heat {} a round of {}, damage {}{}{}",
                def.name,
                def.kind,
                if tuning.vehicle_mode { ", car form" } else { "" },
                tuning.rate_of_fire,
                tuning.heat_per_round,
                tuning.heat_capacity,
                if def.kind == Kind::Gun { tuning.damage } else { tuning.explosive_damage },
                def.explosion.map_or(String::new(), |e| format!(", blast radius {} damage {}", e.radius, e.damage)),
                if tuning.missing.is_empty() { String::new() } else { format!("; missing {:?}", tuning.missing) },
            );
        }
        // The car's guns are in the same list (the manager's eight weapons): told apart by
        // `isVehicleModeWeapon`.
        let (car, held): (Vec<Held>, Vec<Held>) = defs
            .iter()
            .zip(tunings)
            .enumerate()
            // A turret launcher is not held: his special throws it (`equip_special`). [game]
            .filter(|(_, (def, _))| def.kind != Kind::Turret)
            .map(|(slot, (def, tuning))| Held {
                def: def.clone(), tuning, state: Weapon::default(), flame: weapons::Flame::default(), flame_hits: Default::default(), fx_lit: 0, fx_impact: false,
                parent: None, child: None, turn: 0, slot: slot as i32, sounds: KeptSounds::default(),
            })
            .partition(|held| held.tuning.vehicle_mode);
        let (mut held, mut car) = (held, car);
        link_children(&mut held);
        link_children(&mut car);
        if held.is_empty() {
            eprintln!("weapons: his pack lists none for the robot");
            return None;
        }
        Some(Arsenal {
            held,
            car,
            car_wait: VehicleWait::default(),
            car_current: 0,
            car_switch_was_down: false,
            car_rotates: false,
            volcano: None,
            lobs: std::collections::HashMap::new(),
            dives: std::collections::HashMap::new(),
            vent_axis: Vec3::Z,
            eruptions: 0,
            eruption_at: Vec3::ZERO,
            giant_firings: 0,
            giant_blasts: Vec::new(),
            giant_icon: 0.0,
            giant_rounds: Vec::new(),
            in_car: false,
            current: 0,
            switch_was_down: false,
            missiles: Vec::new(),
            missile_scale: std::collections::HashMap::new(),
            alt_rounds: std::collections::HashSet::new(),
            type6_special: None,
            charge_cancel: false,
            owner_dead: false,
            charge_rumble: None,
            sniping: false,
            anim_rate: 1.0,
            scope_hidden: false,
            owner_velocity: Vec3::ZERO,
            grenades: Vec::new(),
            snakers: Vec::new(),
            wave_sounding: Default::default(),
            energon: Vec::new(),
            energon_ring: None,
            debris: Vec::new(),
            debris_rng: crate::debris::Rng(0x5eed),
            launched: 0,
            round_fx: Vec::new(),
            rumbles: Vec::new(),
            special: None,
            special_weapons: Vec::new(),
            special_objects: Vec::new(),
            special_slot: [None; 2],
            special_form: 0,
            special_seen: 0,
            special_clip: None,
            turret: [None, None],
            turrets: Vec::new(),
            gear: Vec::new(),
            turret_clock: 0.0,
            blasts: Vec::new(),
            lock: LockOn::default(),
            seen: [None; 3],
            reticle: Reticle::default(),
            bracket: None,
            lock_shown: 0.0,
            previous_lock_sound: 0.0,
            since_hit: f32::MAX,
            icon_clock: 0.0,
            sprites: Vec::new(),
            seed: 0x2545_f491,
            tracers: Vec::new(),
            impacts: Vec::new(),
            rounds: 0,
            hits: 0,
            struck: Vec::new(),
            props: Vec::new(),
            target_parts: Vec::new(),
            level: None,
            surface_impacts: Vec::new(),
            fx: Vec::new(),
            gun: Gun::default(),
            axes: Vec::new(),
            hung: std::collections::HashMap::new(),
        })
        .map(|mut arsenal| {
            // The first weapon in hand is the first that has no parent.
            let first = arsenal.in_hand().next().unwrap_or(0);
            arsenal.current = first;
            // One that always appears is shown from the start (OptimusFlight's). [game 007a7150]
            arsenal.gun.appear(&arsenal.held[first].def);
            arsenal
        })
    }

    /// Devastator's pods (`crate::giant::Pods`) as his weapons [assumed port design]: the game
    /// never arms a player with them, so they ride the car weapons' path, which fires from barrels
    /// placed in the world by the host (`Car::barrels`): heat, rounds, missiles, sounds, the lock and
    /// the reticle as a car gun's. Every pod kind is a car weapon, one firing at a time
    /// (`switch_car`); the held list keeps the gatling, never fired (he has no weapon state), for
    /// what reads the weapon in hand.
    pub fn load_giant(game_dir: &Path, pods: &[crate::giant::Pods]) -> Option<Arsenal> {
        let defs: Vec<WeaponDef> = pods.iter().map(|p| p.def.clone()).collect();
        let mut arsenal = Self::load(game_dir, &defs)?;
        let first = Self::load(game_dir, defs.get(..1)?)?;
        arsenal.car = std::mem::replace(&mut arsenal.held, first.held);
        arsenal.current = 0;
        arsenal.car_rotates = true;
        // [assumed port design] a quarter of the data's heat a round: the boss's gatling overheats in 20
        // rounds (1 s) and locks for 4 s [data], made for an AI that fires bursts; a player holding the
        // trigger gets 4 s of fire from his sixteen barrels.
        for held in &mut arsenal.car {
            held.tuning.heat_per_round *= GIANT_HEAT;
        }
        // [assumed port design] a slow launcher (the missile pods: one round in 3 s [data]) fires a
        // volley, one missile from each of its pods' barrels.
        for (held, pods) in arsenal.car.iter_mut().zip(pods) {
            if held.def.kind == Kind::Launcher && held.tuning.rate_of_fire >= 1.0 {
                let barrels: usize = pods.mounts.iter().map(|m| m.barrels.len()).sum();
                held.tuning.projectiles_per_fire = held.tuning.projectiles_per_fire.max(barrels as u32);
            }
        }
        // [assumed port design] the bug bombs erupt: a firing throws a dozen up, unaimed, to rain
        // round him (`crate::giant::Lob`).
        arsenal.volcano = pods.iter().position(|p| p.kind == crate::giant::BUG_BOMB);
        if let Some(held) = arsenal.volcano.and_then(|i| arsenal.car.get_mut(i)) {
            held.tuning.projectiles_per_fire = crate::giant::ERUPTION_BOMBS;
        }
        // [assumed port design] his pods' numbers balanced for a player (`giant::POD_BALANCE`): the
        // round's damage, the blast's radius and damage, the time between firings; his own blasts
        // never hurt him (the bombs and missiles come down round him).
        for (held, pods) in arsenal.car.iter_mut().zip(pods) {
            let Some(b) = crate::giant::POD_BALANCE.get(pods.kind) else { continue };
            held.tuning.damage = b.damage;
            held.tuning.rate_of_fire = b.interval;
            if b.blast_radius > 0.0 {
                held.tuning.explosive_damage = b.blast_damage;
                held.tuning.explosive_min_damage = b.blast_damage * 0.35;
                for explosion in [&mut held.def.explosion, &mut held.def.alt_explosion].into_iter().flatten() {
                    explosion.radius = b.blast_radius;
                    explosion.damage = b.blast_damage;
                    explosion.min_damage = b.blast_damage * 0.35;
                    explosion.can_damage_self = false;
                }
            }
            eprintln!("giant: pod kind {} balanced: damage {}, every {} s, {} a firing, blast {}; guided {}, lock {} s, hold to lock {}",
                pods.kind, held.tuning.damage, held.tuning.rate_of_fire, held.tuning.projectiles_per_fire,
                held.def.explosion.map_or("none".into(), |e| format!("{} m {}", e.radius, e.damage)), held.tuning.guided, held.tuning.lock_on_time, held.tuning.hold_to_lock_on);
        }
        Some(arsenal)
    }

    /// The missile is one of Devastator's erupted bug bombs (`lobs`): it draws one trail.
    pub fn erupted(&self, number: u32) -> bool {
        self.lobs.contains_key(&number)
    }

    /// The car weapon that fires (`car_current`).
    pub fn car_current(&self) -> usize {
        self.car_current
    }

    /// The car weapon at `index` is locked by its overheat (Devastator's gatling pods smoke while it
    /// is, `giant::dress`).
    pub fn car_overheated(&self, index: usize) -> bool {
        self.car.get(index).is_some_and(|held| held.state.overheated_left > 0.0)
    }

    /// Devastator's weapon switch: each press moves to his next pod kind, cutting the last one's
    /// held sounds. [assumed port design]
    pub fn switch_car(&mut self, down: bool, sound: Option<&dyn WeaponSound>) {
        if down && !self.car_switch_was_down && self.car.len() > 1 {
            self.car[self.car_current].sounds.cut(sound);
            self.car_current = (self.car_current + 1) % self.car.len();
            // The new kind's icon comes up on his reticle, and his pods whir round to it.
            self.giant_icon = 0.0;
            sound.inspect(|s| s.play_event(crate::giant::SWITCH_SOUND));
            eprintln!("giant: weapon {} of {} ({:#010x})", self.car_current + 1, self.car.len(), self.car[self.car_current].def.name);
        }
        self.car_switch_was_down = down;
    }

    /// His special's weapons for this character (`characterAttributes<X>` in `bnxglobal.str`,
    /// `special_weapon::read_forms`), each form's weapon found among his weapons and its numbers
    /// read as `special_weapon::shot_for_form` has them. A form whose weapon is not among his
    /// weapons fires nothing (said on the console). [data] [game 007b3d80]
    pub fn equip_special(&mut self, game_dir: &Path, character: &str, defs: &[WeaponDef], effects: &[crate::character::WeaponEffects],
        turret_parts: &[weapons::TurretParts]) -> Result<(), String> {
        let pack = Pack::open(&game_dir.join("bnxglobal.str"))?;
        let files: Vec<DataFile> =
            pack.of_type(pack::DATA).filter_map(|chunk| DataFile::parse(pack.data(chunk).to_vec()).ok()).collect();
        let attributes = files
            .iter()
            .find_map(|file| file.root().get(&format!("characterAttributes{character}")))
            .ok_or_else(|| format!("bnxglobal.str has no characterAttributes{character}"))?;
        let forms = special_weapon::read_forms(attributes);
        let tunings = weapons::load(game_dir, defs)?;
        let attribute_of = |hash: u32| files.iter().find_map(|file| file.root().field(hash));
        let shots = forms.map(|form| special_weapon::shot_for_form(&form, defs, &tunings, attribute_of));
        let mut slot = [None; 2];
        for form in 0..2 {
            if forms[form].weapon == 0 {
                continue;
            }
            let Some(i) = defs.iter().position(|d| d.name == forms[form].weapon) else {
                eprintln!("special {form}: weapon {:#010x} is not among his weapons", forms[form].weapon);
                continue;
            };
            eprintln!("special {form}: weapon {:#010x} ({:?}), fires {}", defs[i].name, defs[i].kind,
                if forms[form].from_anim { "from its clip's event" } else { "at its start" });
            slot[form] = Some(self.held.len() + self.special_weapons.len());
            self.special_weapons.push(Held { def: defs[i].clone(), tuning: tunings[i].clone(), state: Weapon::default(), flame: weapons::Flame::default(), flame_hits: Default::default(), fx_lit: 0, fx_impact: false, parent: None, child: None, turn: 0, slot: i as i32, sounds: KeptSounds::default() });
            self.special_objects.push(Axe::default());
        }
        // A turret launcher among his special's forms: its launch numbers (`turret::read_launcher`)
        // and the time the landed turret deploys for. [game 0078c2e0] [data] Blackout's turret
        // carries its own gun (`GunWeapon` in `blackout_turret`, `Blackout_TurretGatlingAttribute`,
        // global_states 80540); Ironhide's carries a MissileLauncher (`Ironhide_TurretMissileAttribute`),
        // which his turret's FireTurret fires as Blackout's fires his gun. [data] (`notes\special-weapons.md`
        // "Correction: Ironhide's turret HAS a FireTurret"; the user saw it fire in the game, 2026-10-27)
        self.turret = [None, None];
        for form in 0..2 {
            let Some(at) = defs.iter().position(|d| d.name == forms[form].weapon && d.kind == Kind::Turret) else { continue };
            let def = defs[at].clone();
            let (turret_weapons, aiming) = turret_parts.get(at).map_or((&[][..], None), |p| (p.weapons.as_slice(), p.aiming.clone()));
            let aiming = aiming.unwrap_or_else(|| turret::Aiming {
                band: TURRET_BAND,
                cone_deg: TURRET_CONE_DEG,
                fire_rate: BLACKOUT_FIRE_RATE,
                rotator: Rotator::from_data_degrees(BLACKOUT_ROTATOR.0, BLACKOUT_ROTATOR.1),
            });
            let node = attribute_of(def.attribute).ok_or_else(|| format!("turret launcher {:#010x} has no attribute", def.name))?;
            let launcher = turret::read_launcher(node);
            eprintln!("turret: launcher {:#010x}: launchSpeed {}, zGravity {}, maxTurretsAtOnce {}", def.name,
                launcher.launch_speed, launcher.z_gravity, launcher.max_turrets_at_once);
            // animOnLand: 90 ticks for Blackout, 86 for Ironhide, at 60 ticks a second (`Turret::deploy_time`).
            let (landing_ticks, gun) = match def.name {
                // [data] the car form's turret (`Blackout_VehicleSpecial`, launchSpeed 500) throws the SAME deployed
                // object: its TurretLauncher (work\survey\weapons\blackout.txt l.733925, weaponName 535218735) has
                // `deployableTurretObj: <see object at 0x22fe30>`, the dumper's mark for an object already decoded
                // (tools\tf2_lxb.py `Ref`), and the only deployable turret decoded in his data is the embedded
                // `blackout_turret` (l.702342, the foot launcher's). So its gun, rotator and FireTurret are those.
                BLACKOUT_SPECIAL | BLACKOUT_CAR_SPECIAL => {
                    // Named by its effects row's key (its own weaponName is 0), so its rounds take the
                    // turret gun's muzzle flash, impacts and trail; its fire sound is that row's. [data]
                    let key = crate::character::turret_gun_key(def.name);
                    let fire_sound = effects.iter().find(|w| w.name == key).map_or(0, |w| w.fire_sound);
                    let gun_def = WeaponDef { name: key, attribute: BLACKOUT_TURRET_GUN, fire_sound, ..WeaponDef::test(Kind::Gun) };
                    // Its barrel: the GunWeapon of the one turret object (the car's launcher throws the same). [data]
                    let barrel = turret_parts.iter().flat_map(|p| &p.weapons).find(|d| d.kind == Kind::Gun).map_or(0, WeaponDef::barrel_node);
                    let gun_tuning = weapons::load(game_dir, std::slice::from_ref(&gun_def))?.remove(0);
                    let gun_node = attribute_of(BLACKOUT_TURRET_GUN).ok_or("Blackout_TurretGatlingAttribute is missing")?;
                    let shot = special_weapon::shot(&gun_def, &gun_tuning, gun_node);
                    let gun = TurretGun { def: gun_def, tuning: gun_tuning, state: Weapon::default(), rounds: shot.burst_per_fire, gap: shot.time_between_bursts, launcher: None, barrel };
                    (90.0, Some(gun))
                }
                // [data] ironhide_turret's MissileLauncher (work\survey\weapons\ironhide.txt l.659653): weaponName 0,
                // `Ironhide_TurretMissileAttribute` (rateOfFire 0.8, automaticFiring false, guided, explosive 24),
                // fireSound -278946228, `flyingvehicle_homingmissiles`. Named by its effects row's key, as
                // Blackout's gun, and kept among the special weapons so its missiles in flight find its numbers.
                // [assumed] the car form's turret deploys in the same 86 ticks: its landing clip is not read.
                IRONHIDE_SPECIAL | IRONHIDE_CAR_SPECIAL => {
                    let key = crate::character::turret_gun_key(def.name);
                    let mut gun_def = turret_weapons.iter().find(|d| d.kind == Kind::Launcher).cloned()
                        .ok_or("ironhide_turret has no MissileLauncher")?;
                    gun_def.name = key;
                    let barrel = gun_def.barrel_node();
                    let gun_tuning = weapons::load(game_dir, std::slice::from_ref(&gun_def))?.remove(0);
                    let gun_node = attribute_of(gun_def.attribute).ok_or("Ironhide_TurretMissileAttribute is missing")?;
                    let shot = special_weapon::shot(&gun_def, &gun_tuning, gun_node);
                    let index = self.held.len() + self.special_weapons.len();
                    self.special_weapons.push(Held { def: gun_def.clone(), tuning: gun_tuning.clone(), state: Weapon::default(), flame: weapons::Flame::default(), flame_hits: Default::default(), fx_lit: 0, fx_impact: false, parent: None, child: None, turn: 0, slot: -1, sounds: KeptSounds::default() });
                    self.special_objects.push(Axe::default());
                    eprintln!("turret: {:#010x} carries a missile launcher: rateOfFire {}, fireRate {}", def.name, gun_tuning.rate_of_fire, aiming.fire_rate);
                    let gun = TurretGun { def: gun_def, tuning: gun_tuning, state: Weapon::default(), rounds: shot.burst_per_fire, gap: shot.time_between_bursts, launcher: Some(index), barrel };
                    (86.0, Some(gun))
                }
                _ => (90.0, None),
            };
            self.turret[form] = Some(TurretSetup { name: def.name, launcher, deploy_time: landing_ticks / 60.0, gun, aiming });
        }
        self.special = Some(SpecialFire::new(forms, shots));
        self.special_slot = slot;
        Ok(())
    }

    /// The special started in this step (`sim::start_special`, counted by `starts`); `car` is
    /// the form. A form whose weapon fires at the start fires now (`fires_at_start`). [game
    /// 007b3d80 param 0] A form whose weapon comes from the clip's `WeaponFireSpecialEvent` is
    /// fired by `tick_special_events`, not here.
    pub fn special_started(&mut self, starts: u32, car: bool) {
        // A reset of his state counts its starts from none again.
        if starts < self.special_seen {
            self.special_seen = starts;
        }
        if starts == self.special_seen {
            return;
        }
        self.special_seen = starts;
        let Some(fire) = self.special.as_mut() else { return };
        self.special_form = car as usize;
        if fire.fires_at_start(car) {
            fire.on_fire_event(car);
        }
    }

    /// The `WeaponFireSpecialEvent`s of the special clip he plays in this update: those whose
    /// time is in the window (the clip's previous tick, not counted, up to `tick`), as
    /// `Axe::events` walks the weapon events. Each one presses the special for the form he is
    /// in (`car`). `playing` is the clip's set name, its name and its tick, or `None` when no
    /// action clip plays. [game 007ba870] [data] [assumed] the clip is the one the action layer
    /// plays (`Sim::action_clip`).
    ///
    /// The same window's weapon events show and pose the special weapons' objects
    /// (`special_objects`); `objects` are his weapons' object clips (`weapon_animations`), and
    /// `dt` steps them. `playing` also carries the clip's set.
    pub fn tick_special_events(&mut self, playing: Option<(&Clip, &str, &str, f32)>, car: bool, objects: &[Vec<crate::formats::anim::AnimRef>], dt: f32) {
        // A vehicle-mode weapon's burst (the carpet bomb) goes on only while he is in the drive
        // or fly mode (`SpecialFire::step`). [game 007ac370, 0070ab90] `car` (`State::car_mode`) is
        // DriveMode or FlyMode as `0070ab90` reads them: `sim.rs` sets it on StateDrive and StateFly.
        if let Some(fire) = self.special.as_mut() {
            fire.set_vehicle_motion(car);
        }
        let window = playing.map(|(clip, set, name, tick)| {
            let id = crate::formats::hash::crc32(name.as_bytes());
            let from = self.special_clip.filter(|&(last, at)| last == id && at <= tick).map(|(_, at)| at);
            (clip, set, name, id, from, tick)
        });
        for (held, object) in self.special_weapons.iter().zip(&mut self.special_objects) {
            let was = (object.shown, object.object.map(|o| o.id));
            match window {
                Some((clip, set, _, _, from, tick)) if set == SPECIAL_SET => object.events(clip, from, tick, held.def.name),
                // [assumed] Leaving the special's clips (the special cut short by a hit, a death)
                // puts its object away. [game 007b2eb0, read whole 2026-10-20] The manager's put-away is not
                // the route: it walks the 8 weapon slots, not the specials, and its callers are
                // state_weapon_exit 0087f6f0 and state_dash_exit 0087abc0 only. The special's own exit,
                // state_attack_special_exit 00879700, puts the MELEE weapon away (007b2d80), not this object.
                // A finished special has already hidden it by the exit clip's own event. settles: whether a hit
                // or a death leaves StateAttackSpecial through that exit (00879700, `global_states.txt`
                // connectionListAttackSpecial l.3942) or another route (the damage path's state change, not
                // read), and the special object's put-away slot (vt+0x1c / +0x28 on the special's object).
                _ => object.put_away(held.def.exit_clip),
            }
            let clips = objects.get(held.slot as usize).map_or(&[][..], Vec::as_slice);
            object.step(clips, dt);
            if std::env::var_os("TF_DBG").is_some() && was != (object.shown, object.object.map(|o| o.id)) {
                eprintln!("special object {:#010x}: shown {} on node {:#010x}, clip {:?} (window {:?})", held.def.name, object.shown,
                    object.node, object.object.map(|o| o.id), window.map(|w| (w.2, w.5)));
            }
        }
        let Some((clip, _, name, id, from, tick)) = window else {
            self.special_clip = None;
            return;
        };
        self.special_clip = Some((id, tick));
        let Some(fire) = self.special.as_mut() else { return };
        for event in clip.special_fire_events.iter().filter(|e| e.time <= tick && from.is_none_or(|from| e.time > from)) {
            eprintln!("special: fire event in {name} at tick {}", event.time);
            fire.on_fire_event(car);
        }
    }

    /// The special's rounds of this update (`SpecialFire::step`). Each leaves his muzzle with
    /// its barrel turned by its `fireDummy` angles from his facing, as a gun's shot (traced as
    /// his gun's are) or a grenade thrown on (it then flies and goes off in `tick`). [game]
    /// `muzzle` is the special weapon's own `FireDummy` on its node (`special_mount`). [data]
    /// Every barrel leaves from that first FireDummy. [data] Jetfire's special GrenadeLauncher has
    /// `minFireDummyIndex` 0, `maxFireDummyIndex` 7, `projectiles_per_fire` 8, and its object has
    /// FireDummy..FireDummy07 (`weapons_check jetfire`): their rest places lie within 8 mm of each
    /// other (x 0.00609..0.00689, y 0.01091..0.01105, z -0.016167..-0.016161), so one muzzle is within a
    /// centimetre of each barrel; the eight turns are the per-round angles (`turn`), which are the game's.
    /// Round k of a firing takes the list's entry k (`SpecialFire::burst`). Barrel k's own place is not
    /// carried here (the special's muzzle is one point from `player.rs`).
    pub fn tick_special(&mut self, muzzle: Vec3, facing: Vec3, world: &dyn Sight, targets: &[Target], sound: Option<&dyn WeaponSound>, dt: f32) {
        let Some(fire) = self.special.as_mut() else { return };
        let rounds = fire.step(dt);
        let Some(index) = self.special_slot[self.special_form] else { return };
        for round in rounds {
            let (def, tuning) = {
                let held = weapon_in(&self.held, &self.special_weapons, &self.car, index);
                (held.def.clone(), held.tuning.clone())
            };
            // [game] the data's angles as they are: fireDummyZAngle -40 tips each round 40 degrees
            // DOWN, a ring below him (`notes\special-jetfire-mines.md` "Jetfire's direction settled":
            // 007a8587..007a85bb pass cos and sin with no negation into 00570770, and the grenade is
            // launched along the forward row of the object spawned in the barrel's frame, weapon_fire's
            // slot 0x80 taking `weapon+0x54`). The earlier negation (a ring above him) is withdrawn.
            let way = turn(facing, round.xy_deg, round.z_deg);
            let at = weapons::facing_target(muzzle, way);
            self.rounds += 1;
            match def.kind {
                Kind::Gun => self.shoot(def.name, 0, muzzle, at, &tuning, 1.0, None, world, targets, sound),
                Kind::Grenade => {
                    self.fx.push(WeaponFx { kind: WeaponFxKind::Muzzle(0), weapon: def.name, at: muzzle, along: way });
                    let seed = &mut self.seed;
                    let grenade = weapons::Grenade::launch(muzzle, way, at, self.owner_velocity, &tuning, &mut || roll(seed));
                    self.launched += 1;
                    sound.inspect(|s| s.projectile_start(self.launched, def.name, grenade.position));
                    self.fx.push(WeaponFx { kind: WeaponFxKind::Trail(self.launched), weapon: def.name, at: muzzle, along: way });
                    self.grenades.push((index, self.launched, grenade));
                }
                // The turret launcher throws its turret from the muzzle along his facing
                // (`launch_turret`). The direction is flat along his facing: 0078c2e0 takes the throw's way
                // from the barrel record's `+0x1c..+0x24` row (007a7a40 builds it from the character's frame
                // for a special, which has no FireDummy matrix [game]), so a level frame gives a flat throw.
                // [game 0078c2e0, 007a7a40] The round's barrel turn is not used. [assumed] the launcher's own
                // dummy is the same barrel record the throw takes its way from. settles: the launcher's
                // FireDummy node in the turret's tree (work\survey\weapons\blackout.txt, not read here) against the
                // record 0078c2e0 reads.
                // The throw shows the launcher's own muzzle flash, as any firing does (007aac80 from
                // 007ae010). [game] [data: Blackout's `w_muzzle_flash_blackout_turret`, the car
                // form's `w_muzzle_flash_deva_gunpod`]. OPEN: 007aac80 places the flash at the node its caller
                // passes (param_2 0: the weapon object's own affine at +0xc, l.16-60; else the nav matrix entry), and
                // the port fires it at `Muzzle(0)` (barrel 0), not at a barrel picked by 007a8770. settles: the
                // caller of 007aac80 in 007ae010 with the throw's shot index (not read).
                Kind::Turret => {
                    self.fx.push(WeaponFx { kind: WeaponFxKind::Muzzle(0), weapon: def.name, at: muzzle, along: facing });
                    self.launch_turret(muzzle, facing)
                }
                // [data] no special weapon of the roster is a missile launcher or a flamethrower
                // (Megatron's is a gun, Jetfire's and the Audi's grenade launchers, Ironhide's and
                // Grindor's turret launchers: `notes\character-specials.md`), so nothing to do.
                Kind::Launcher | Kind::Flame => {}
            }
            // Each round's fire sound and ring-off (007a9b40, `KeptSounds::fire`). [game] No special
            // weapon of the roster has an infinite fire sound, so nothing is kept here and no
            // key-off is needed [data: `weapons::sound_tests`].
            let held = if index >= CAR_SLOT { &mut self.car[index - CAR_SLOT] }
                else if index < self.held.len() { &mut self.held[index] }
                else { &mut self.special_weapons[index - self.held.len()] };
            let heat = held.state.heat;
            held.sounds.fire(&def, &tuning, heat, sound);
        }
    }

    /// The turret launcher's throw (`LauncherTuning::launch`, [game 0078c2e0]): one turret from the
    /// muzzle, flat along his facing [game 0078c2e0, 007a7a40] (see the throw's way there),
    /// with the rotator and fire timer its data gives
    /// [data]. Over `maxTurretsAtOnce` the oldest goes [game 0078b0a0].
    fn launch_turret(&mut self, muzzle: Vec3, facing: Vec3) {
        // The form of the press (`special_form`): on foot or in the car. [game 007b3d80]
        let Some(setup) = self.turret[self.special_form].clone() else { return };
        let body = setup.launcher.launch(muzzle, Vec3::new(facing.x, facing.y, 0.0), self.turret_clock, setup.deploy_time);
        eprintln!("turret: launched from ({:.2}, {:.2}, {:.2}) at {:.2} s", muzzle.x, muzzle.y, muzzle.z, self.turret_clock);
        let rest = body.rest_facing;
        self.turrets.push(body);
        self.gear.push(Gear {
            weapon: setup.name,
            // The spawn frame: upright (`align_z_matrix` with (0, 0, 1), its +y the throw's way
            // laid flat), then turned half round about z (`rotate_z_matrix(pi)`, a literal), so the
            // base faces back along the throw. [game 0078c2e0 l.13-41] The reference +y of `align_z_matrix`
            // (0056e7b0) is the source frame: at 0x78c339-0x78c37a the caller copies the affine at [esi+0xc]+0x10
            // into EDI, and align_z reads its rows (the +y row is the second) [game, raw_disasm]. [assumed] for a
            // special that frame is the character's (level on the ground and in the car): not traced to its owner
            // (`[esi+0xc]` is the spawn record's object, `FUN_0078c9d0`'s param_2[1]). [game 0078c9d0, 2026-10-20]
            // 0078c9d0 stores that object at the turret record's `+0x3c` (its owner's handle, param_2[0], at
            // `+0x38`) and calls the object's vtable +0x38 with the spawn record: so the frame is the spawn
            // object's, and that object is the caller's (not read: the spawn callers are 007842a6 in 00784170
            // and 0078c7f4 in 0078c7e0). [game 00784170 callers, 2026-10-20] 00784170 is called only by weapon_fire
            // 007ae010 (two sites) and charge_fire 007af210; 0078c7e0 has no direct caller. The spawn record is built
            // by the vtable +0x80 slot in 007ae010 (l.152 and l.227), whose object argument is not named here.
            // settles: what object the throw's spawn record names for a special (the record built by vt+0x80 in
            // 007ae010, and the caller of 007ae010 for a special's weapon).
            heading: (-facing.x).atan2(facing.y) + std::f32::consts::PI,
            // The turret's TurretRotator (Blackout's 70 / 50 deg/s, Ironhide's 360 / 180) [data] [game
            // 008139b0]. With no target it turns back to the launch's rest facing (`Turret::rest_facing`)
            // [game 00813e30 l.102-106, 008153e0].
            rotator: Rotator { rest, ..setup.aiming.rotator.clone() },
            // FireTurret: fireRate (Blackout's 100, Ironhide's 25), band and cone [data].
            fire: FireTimer::new(setup.aiming.fire_rate),
            band: setup.aiming.band,
            cone_deg: setup.aiming.cone_deg,
            gun: setup.gun.clone(),
            burst: Burst::default(),
            fired_once: false,
            muzzle: None,
            clip: None,
        });
        if let Some(i) = turret::cap_index(&self.turrets, setup.launcher.max_turrets_at_once) {
            self.turrets.remove(i);
            self.gear.remove(i);
            eprintln!("turret: removed by the cap (the oldest; {} at once)", setup.launcher.max_turrets_at_once);
        }
    }

    /// One update of the deployed turrets [game 0078cd50, 0078b3b0, 00813e30, 0079e670, 0079f0f0].
    /// `enemies` are the scouts as (id, team, the point aimed at, present). Each turret lands
    /// (`Turret::step`), then, once Active and with a gun, aims at the nearest visible enemy in
    /// range (`pick_target`); its gun is pulled when it points within the cone of that target
    /// (the cone gates the trigger, [game 0079f547], not the choice of target). A turret goes
    /// when its owner is gone [game 0078cd50 b]. [stand-in] the barrel is a point at its position, not the
    /// gun's muzzle node. [data 2026-10-20] The deployed tree (`blackout_turret`, work\survey\weapons\blackout.txt
    /// 702634) has the rotator bone `turretBoneName` 660760739 = `WeaponRotator` (node 16, parent `TopPlate`);
    /// its subtree holds `MissleRoot`, `FireDummy`, `GunRoot` and the guns `Gun_L_01..03` / `Gun_R_01..03`.
    /// OPEN: which gun node is the gatling's muzzle (the gun object's muzzle attribute, not read in the tree), so the
    /// barrel stays a point. settles: that muzzle node in `blackout_turret`'s gun object, and the WeaponRotator
    /// node's place at run time (the rotator's own pose, not in the data). [data 2026-10-20, blackout.txt 720703-721485]
    /// The gatling's GunWeapon has `minFireDummyIndex` 0 and `maxFireDummyIndex` 14: its rounds leave from one of 15
    /// fire dummies, not from one node, and its FireTurret has `fireRate` 100, `maxFireRange` 300 and
    /// `coneAngleToAllowFire` 15. Which dummy a round takes is not read (weapon_fire 007ae010 only walks the
    /// MuzzleFlashDummy list, l.102-106); so the point stays, and the 15-dummy spread is still unsettled.
    #[allow(clippy::too_many_arguments)]
    pub fn tick_turrets(&mut self, dt: f32, owner: TurretOwner, world: &dyn Sight, targets: &[Target], enemies: &[(u32, u32, Vec3, bool, f32)], sound: Option<&dyn WeaponSound>) {
        self.turret_clock += dt;
        if self.turrets.is_empty() {
            return;
        }
        if !owner.alive {
            eprintln!("turret: removed, its owner is gone ({} of them)", self.turrets.len());
            self.turrets.clear();
            self.gear.clear();
            return;
        }
        let mut turrets = std::mem::take(&mut self.turrets);
        let mut gear = std::mem::take(&mut self.gear);
        for (body, g) in turrets.iter_mut().zip(gear.iter_mut()) {
            let before = body.state;
            // The contact is the arena's surface along this step's path (as a grenade's is). [assumed]
            // [game 0078b860, 0078b3b0 read 2026-10-20] The game's turret contact is the physics contact callback
            // 008cf400 (the Lux solver's, a library function: census net:bd) that calls 0078b860 when the
            // turret's state is 1 or 3, and its in-flight step 0078b3b0 gets the contact normal from there. So the
            // game takes the contact from the solver, not from a surface sweep. [game 0078b3b0, decompile l.31-54,
            // read 2026-10-20] The in-flight step lands the turret only when the contact's normal (the contact's
            // own vector at param_2[2..4], normalised) has a dot with the turret's up row above 0.5; a normal
            // below that leaves it flying. OPEN: the port has no solver contact feed, so the landing test uses the
            // arena's sight normal; settles: the Lux contact solver's contact list for a body (008cf400's
            // arguments, not read), and a CE trace of a turret's landing normal on a slope.
            let contact = if before == State::Flying {
                let to = body.position + body.velocity * dt + Vec3::Z * (0.5 * body.gravity_z * dt * dt);
                world.sight(body.position, to).map(|hit| hit.normal)
            } else {
                None
            };
            body.step(dt, contact);
            match (before, body.state) {
                (State::Flying, State::Deploying) => eprintln!(
                    "turret: landed at ({:.2}, {:.2}, {:.2}) at {:.2} s", body.position.x, body.position.y, body.position.z, self.turret_clock),
                (State::Deploying, State::Active) => eprintln!("turret: active at {:.2} s", self.turret_clock),
                _ => {}
            }
            if body.state != State::Active {
                continue;
            }
            // The only game callers of a weapon's held trigger (slot +0xc4) are FireTurret 0079f0f0, the
            // player's 007b37f0, the special's 007b3c40 and the flamethrower's own 0074cd20 [game, byte scan
            // 2026-10-20, wave 28 P4]. Both turrets carry a FireTurret [data], so 0079f0f0 presses Blackout's
            // GunWeapon and Ironhide's MissileLauncher alike (`automaticFiring` false is no obstacle: the fire
            // timer lets go between its presses). The user saw Ironhide's fire in the game, 2026-10-27.
            let Some(gun) = g.gun.as_mut() else { continue };
            // From its FireDummy as last drawn (`Gear::muzzle`: one frame old, [port design]); [stand-in] 0.5 m over
            // its base before it is drawn, or with no model.
            let muzzle = g.muzzle.unwrap_or(body.position + Vec3::Z * 0.5);
            // The scouts that are not on his team are the enemies, and visible from the muzzle [assumed].
            let candidates: Vec<turret::Candidate> = enemies
                .iter()
                .map(|&(id, team, at, present, radius)| {
                    // [assumed] the turret's enemies are the thrower's hostile faction, the rule of
                    // 0117e6d0; Grindor's FireTurret's `enemyTeam` #951af706 (the Autobots) agrees. [data]
                    let hostile = weapons::thrower_hostile(owner.team, team);
                    (id, u32::from(hostile), at, present && world.sight(muzzle, at).is_none(), radius)
                })
                .collect();
            // The band test of target_score 007996c0, centred on the rest facing: the no-target aim
            // of the game is the same direction [game 0079e920 l.24, 94-103; see `pick_target`].
            let aim_id = turret::pick_target(body.position, g.rotator.rest, &candidates, &g.band, 1);
            let aim = aim_id.and_then(|id| candidates.iter().find(|c| c.0 == id).map(|c| c.2));
            g.rotator.step(dt, body.position, aim);
            let facing = g.rotator.facing();
            // [game 0079f0f0 at 0079f1ea-0079f403, 00815290, 00813700; lead 2026-10-20, wave 30 R6] the fire cone
            // is taken about the TurretRotator's bone: FireTurret finds the rotator (turret_rotator_find 007a0d50,
            // EDI at 0079f205) and 00815290 returns the +y row of its node +0x48, which the rotator's tick
            // 00813700 sets each step to its `turretBoneName` (WeaponRotator) node (008137b1-008137c7); the
            // object's own +y (the rest facing) only when that index is out of range. So the cone follows the
            // turning gun, from the object's place, strict `>`. (The old reading, node 0 = the object, was wrong.)
            // [assumed: the rotator's tick runs before FireTurret in a step, so the facing after `step`]
            let aimed = aim.is_some_and(|p| turret::in_fire_cone(body.position, p, facing, g.cone_deg));
            // FireTurret pulls its trigger each step while it has a target [game 0079e670, 0079f0f0];
            // the gun's own rate and heat decide the shot [game 00b5fba0 weapon_trigger_held].
            let pulled = g.fire.step(dt, aim.is_some());
            let fired = gun.state.step(&gun.tuning, pulled && aimed, dt);
            if matches!(fired, Fired::Rounds(_) | Fired::RoundsAndOverheated(_)) {
                g.burst.start(gun.rounds, gun.gap);
            }
            for _ in 0..g.burst.step(dt) {
                // The round goes to the aim point the weapon is sent, `ft+0x80` [game 0079f0f0]; the barrel's line
                // only where there is none (a burst after its target left).
                // With no candidate 0079e920 l.94-103 writes `ft+0x80` = the FireTurret's own object's place + 1000 x
                // its +y row (the object at the component's +0xc is the turret itself: its rest facing). [game]
                let at = aim.unwrap_or(body.position + g.rotator.rest * TURRET_AIM_DISTANCE);
                if !g.fired_once {
                    g.fired_once = true;
                    eprintln!("turret: fired its first shot at {:.2} s, along ({:.2}, {:.2}, {:.2}), from ({:.2}, {:.2}, {:.2}){}", self.turret_clock,
                        facing.x, facing.y, facing.z, muzzle.x, muzzle.y, muzzle.z, if g.muzzle.is_some() { " (its FireDummy)" } else { " (over its base)" });
                }
                if let Some(index) = gun.launcher {
                    // Ironhide's MissileLauncher: a missile leaves along the barrel's line, sent at the aim point
                    // (`ft+0x80`, as the gun's round) [game 0079f0f0], as a launcher's in hand does. [assumed] it is
                    // locked on the turret's target: the launcher is `guided` [data], and the FireTurret's chosen
                    // candidate is the only target it has (the lock a FireTurret hands its weapon is not read).
                    // settles: what 0079f0f0 passes beside the point to the held trigger (vtbl+0xc4), or a recording.
                    self.fx.push(WeaponFx { kind: WeaponFxKind::Muzzle(0), weapon: gun.def.name, at: muzzle, along: facing });
                    let seed = &mut self.seed;
                    let missile = Missile::launch(muzzle, facing, at, aim_id, &gun.tuning, &mut || roll(seed));
                    self.launched += 1;
                    sound.inspect(|s| s.projectile_start(self.launched, gun.def.name, missile.position));
                    self.fx.push(WeaponFx { kind: WeaponFxKind::Trail(self.launched), weapon: gun.def.name, at: muzzle, along: facing });
                    self.missiles.push((index, self.launched, missile));
                } else {
                    self.shoot(gun.def.name, 0, muzzle, at, &gun.tuning, 1.0, None, world, targets, sound);
                }
                // Its gun's own fire sound, where the turret stands. [data: blackout_turret's GunWeapon,
                // fireSound -278946228] One start per firing, in the call that starts the `Fire` clip
                // (007ae010 -> 007a9b40, see `Gear::clip`). [game]
                sound.inspect(|s| s.play_event_at(gun.def.fire_sound, muzzle));
                g.clip = Some(("Fire", self.turret_clock));
            }
            // The overheat comes after the firing that caused it (007a9ec0 from the heat step). [game]
            if matches!(fired, Fired::RoundsAndOverheated(_)) {
                g.clip = Some(("Overheat", self.turret_clock));
            }
        }
        self.turrets = turrets;
        self.gear = gear;
    }

    /// One frame of the melee weapons: the simulation runs their weapon events, put-away and
    /// clips (`sim::step_melee`, so their boxes hit as they stand), and this takes them for
    /// drawing; one source.
    pub fn tick_axe(&mut self, melee: &[Axe]) {
        self.axes.clear();
        self.axes.extend_from_slice(melee);
    }

    /// A weapon's first barrel as `tick` looks for it: the node its object hangs on (what
    /// `attach_models` hung its model on, `hung`; else its `startBoneAttachment`), the name of
    /// its barrel node (`FireDummy` + `minFireDummyIndex`) and that node's rest place in the
    /// object. [game 007a6380, 007b4f20, 007a7a40] [data]
    fn barrel_of(&self, def: &WeaponDef) -> Barrel {
        Barrel { weapon: def.name, node: def.barrel_node(), hung: self.hung.get(&def.name).copied().unwrap_or(def.bone), at: def.muzzle }
    }

    /// The weapon in hand's first barrel.
    pub fn barrel(&self) -> Barrel {
        self.barrel_of(&self.held[self.current].def)
    }

    /// The weapons that can be in hand (those with no parent, and not the special's), in the
    /// list's order. [game 007b3ea0, 007b4080] The game's next-weapon loop skips every isSpecial (+0x9c) with
    /// no fallback to one (read: an all-special list leaves the hand where it is). The port's
    /// fallback (every one special: the first) never runs for a robot: [data] each of the 13 has two
    /// to five ordinary weapons (`core\examples\special_only_check.rs`). It stays only so that a
    /// list with none still draws something.
    /// A damaging hit of his that is not a round (his melee): the hit-mark clock starts again. [game 00729120
    /// l.8705-8708, 00743a40: the SOURCE's +0x234 +0x30 is stamped with the game time for any damage packet of
    /// amount > 0 (not a flag-0x8000 one), whatever sent it; wave 26 read 2026-10-20]
    pub fn mark_hit(&mut self) {
        self.since_hit = 0.0;
    }

    fn in_hand(&self) -> impl Iterator<Item = usize> + '_ {
        let any = self.held.iter().any(|h| h.parent.is_none() && !h.tuning.is_special);
        (0..self.held.len()).filter(move |&i| self.held[i].parent.is_none() && (!any || !self.held[i].tuning.is_special))
    }

    /// The first barrel of the in-hand weapon's child (his other hand's), if it has one.
    pub fn partner_barrel(&self) -> Option<Barrel> {
        self.held[self.current].child.map(|child| self.barrel_of(&self.held[child].def))
    }

    /// A held weapon as its arm's weapon events see it: name, slot and its child's. [game 007b4680]
    fn hand_of(&self, index: usize) -> crate::gun::Hand {
        let held = &self.held[index];
        let child = held.child.map(|c| (self.held[c].def.name, self.held[c].slot));
        crate::gun::Hand { name: held.def.name, slot: held.slot, child }
    }

    fn hand(&self) -> crate::gun::Hand {
        self.hand_of(self.current)
    }

    /// The names of the weapon in hand and of its child: the weapons whose objects are out.
    pub fn shown_names(&self) -> [u32; 2] {
        let held = &self.held[self.current];
        [held.def.name, held.child.map_or(0, |child| self.held[child].def.name)]
    }

    /// The special weapon of the form last pressed: its first barrel, which its rounds leave
    /// from (`007a7a40` takes the barrels from the weapon's own node list). [data] A car-form
    /// special's node is the car's (`007a6380`). None when the special has no weapon.
    pub fn special_barrel(&self) -> Option<Barrel> {
        let index = self.special_slot[self.special_form]?;
        Some(self.barrel_of(&weapon_in(&self.held, &self.special_weapons, &self.car, index).def))
    }

    fn random(&mut self) -> f32 {
        roll(&mut self.seed)
    }

    /// The car's weapon: the node of the car it hangs from and its barrels in its own
    /// space (`FireDummy`, `FireDummy01`).
    pub fn car_weapon(&self) -> Option<(u32, &[Vec3])> {
        self.car.first().map(|held| (held.def.bone, &held.def.barrels[..held.def.barrel_count]))
    }

    pub fn is_car_weapon(&self, name: u32) -> bool {
        self.car.iter().any(|held| held.def.name == name)
    }

    /// The object state of the special weapon among `names` (a model's weapons), with that
    /// weapon's index in his list (its object clips); None when none of them is a special's.
    pub fn special_object(&self, names: &[u32]) -> Option<(&Axe, usize)> {
        self.special_weapons.iter().zip(&self.special_objects)
            .find(|(held, _)| names.contains(&held.def.name))
            .map(|(held, object)| (object, held.slot as usize))
    }

    /// The weapon in hand has `snipeCameraEnabled` (its slot `+0xa4`). [game 0071dc20]
    pub fn snipe_in_hand(&self) -> bool {
        self.held.get(self.current).is_some_and(|held| held.tuning.snipe.camera_enabled)
    }
    /// The weapon of this name has a `chargeMaxProjectileObj` (`WeaponDef::alt_projectile`).
    fn has_alt_projectile(&self, name: u32) -> bool {
        self.held.iter().chain(&self.car).chain(&self.special_weapons).any(|held| held.def.name == name && held.def.alt_projectile)
    }

    /// How far the aim assist looks with the weapon in hand (the aim-assist object's
    /// `+0x30`, set as a weapon is taken in hand: `FUN_007b2c60`).
    pub fn assist_range(&self, aim: &AimTuning) -> f32 {
        let range = self.held[self.current].tuning.aim_assist_max_range;
        if range > 0.0 { range } else { aim.default_max_range }
    }

    /// The target the aim assist has in its box (its best: the first of its three), as the
    /// assist's test wants it. [game] Speed comes from the simulated character body.
    /// The point is the target's Head node (`aim::assist_point`: `Target::head`, filled from the posed model by
    /// `targets::animate`), else its place raised by 0.75 of its height [game 00709870 l.62-74, 2026-10-20].
    /// The candidate's BOX is another matter: [stand-in] 00709100 takes the place of the candidate's node (`object_node_world_place` 0056bee0: EAX = the
    /// 16-bit word at the candidate's `+0x14`, ECX = its object at `+0xc`, `00709168-00709176`), with its `+0x4c`
    /// (zOffset) added to z, and the top point at `+0x48` (the height) above that. `+0x14` is the lux component's u16
    /// argument, stored by 00577b40 (`00577b57-00577b6f`), from the TargetLockable factory 01196790. 0056bee0: node 0
    /// is the object's own place, any other N is its matrix entry N-1. So the port's point is the game's where that
    /// word is 0 and zOffset is 0 (both from the data record, neither read for the roster). [game 00709100, read whole
    /// 2026-10-20] The box is the game's: `lockon::candidates` makes the same four projected points (sides at the
    /// place +/- `+0x44` along the camera's right, top at `+0x48`) and tests them with FUN_00707980 (read whole:
    /// a strict two-rectangle overlap, x against the aim box's `EDI[2..3]`, y against `EDI[4..5]`). The candidate
    /// needs `+0x53` set, not `lockable_ignored_by_owner` 007bb2c0, and a point ahead of the aim.
    /// 0070916a is not a function of the export (the earlier reference to it was wrong). settles: the value of that
    /// word for each TargetLockable of the roster (the factory record's `+4` / `+0x10` field, `tf2_lxb.py dump`), and
    /// the zOffset of each (`+0x4c`, data).
    pub fn assist_target(&self, targets: &[Target], view: &crate::camera::View,
        velocity: impl Fn(u32)->Vec3) -> Option<AssistTarget> {
        let target = targets.iter().find(|target| Some(target.id) == self.seen[0])?;
        let point = crate::aim::assist_point(target);
        let picture = View::looking(view.eye, view.forward, view.fov, 16.0 / 9.0).project(point)?;
        Some(AssistTarget { point, place: target.pos, velocity: velocity(target.id), off_centre: (picture - lockon::PICTURE * 0.5).length_squared() })
    }

    /// A gun's round: traced from the muzzle towards the point it was sent at over the
    /// weapon's range, landing in the same update (`tracerFirePerRound` is over 0 for his
    /// guns). [game] What it lands on takes the round's damage.
    /// `snipe`: a sniping round's camera place, where its trace starts, and its steady.
    #[allow(clippy::too_many_arguments)]
    fn shoot(&mut self, weapon: u32, barrel: usize, muzzle: Vec3, at: Vec3, tuning: &WeaponTuning, charge: f32, snipe: Option<(Vec3, f32)>, world: &dyn Sight, targets: &[Target], sound: Option<&dyn WeaponSound>) {
        let (from, end) = weapons::bullet_line(snipe.map_or(muzzle, |(eye, _)| eye), at, tuning);
        let world_hit = world.sight(from, end);
        let mut landed = world_hit.map(|hit| (hit.point, None));
        // The part struck: the target's own posed bodies (`round_enters`), its first entry ordering it
        // against the rest, a Head along the line taking the hit. [game 007832b0]
        let mut part = Part::UpperBody;
        for target in targets {
            let Some((first, k, struck)) = round_enters(from, end, target, &self.target_parts) else { continue };
            let near = from + (end - from) * first;
            if landed.is_none_or(|(nearest, _)| (near - from).length_squared() < (nearest - from).length_squared()) {
                landed = Some((from + (end - from) * k, Some(target.id)));
                part = struck;
            }
        }
        // The mission's Destructables: a round that enters one first stops there (no damage to what is behind it).
        // A Destructable of an Instance with collision is hit where the round meets that Instance's primitives
        // (`LevelWorld::ray_of_owner`) [stand-in: every solid primitive takes the round, mission path only]; the
        // others use the sphere [stand-in] (mission path only). The damage is the same packet a character would get. [game 00724700, 2026-10-20] The packet
        // goes to each listener on the target's list (`send_damage_message`: a vtable call per entry, 17 lines).
        // A Destructable's receiver is `destructable_damage_apply` 00743a40 (flags, health, the damage filter,
        // trigOnDestroy), called from 00746350 (the receive entry, not read). [assumed] (mission path only) OPEN: that
        // the Destructable is on the list the round's packet reaches (the listener registration, not read).
        // settles: the listener registration of a Destructable (00746350's entry and its caller), read against
        // this round's target.
        for prop in &self.props {
            let by_shape = match (prop.owner, self.level.as_deref()) {
                (Some(owner), Some(level)) if level.owner_is_solid(owner) => level.ray_of_owner(owner, from, end).map(|h| h.point),
                _ => enters_sphere(from, end, prop).map(|k| from + (end - from) * k),
            };
            let Some(point) = by_shape else { continue };
            let owned = prop.owner.is_some() && self.level.is_some();
            let slack = if owned { PROP_OWNER_TOL } else { 0.0 };
            if landed.is_none_or(|(nearest, _)| (point - from).length() < (nearest - from).length() + slack) {
                landed = Some((point, Some(prop.id)));
                // A Destructable's body is no character part: no multiplier. [game 00782b30 l.55-72: only Head,
                // UpperBody, LowerBody and VehicleBody take one] [data, wave 28 P4: cairo_city zone.str#14,
                // AC_Vent_A_01's Destructable Collision material is Metal_Soft 0xa0683ad8] [assumed: the other
                // Destructable templates (about 206) are none of the four either; one was read]
                part = Part::Other;
            }
        }
        self.fx.push(WeaponFx { kind: WeaponFxKind::Muzzle(barrel), weapon, at: muzzle, along: end - from });
        // While his type-6 special runs the round is the weapon's `chargeMaxProjectileObj`
        // (Starscream's `gatlinggunspecial`: `LargeLaserPreset` impacts), its effects kept under
        // `alt_projectile_key`. [game 007ae010, 007aaeb0] So is a sniping round of full steady
        // (STEADIED); neither sniper rifle has a `chargeMaxProjectileObj`, so theirs stays their
        // own round. [data]
        let steady = snipe.map_or(0.0, |(_, steady)| steady);
        // A full charge's round is the `chargeMaxProjectileObj` too, for a count under 2 (its
        // template 1 needs level >= 1 and N < 2: `007af210` l.123-137). [game] (Optimus's and
        // the Megatrons' guns; the grenade launchers have none.)
        let full_charge = self.held.iter().find(|held| held.def.name == weapon).is_some_and(|held| held.state.charge_full && held.tuning.charge.count < 2);
        let alt = (weapons::fire_choice(self.type6_special.is_some(), steady).1 || full_charge) && self.has_alt_projectile(weapon);
        let round = if alt { crate::character::alt_projectile_key(weapon) } else { weapon };
        if alt && std::env::var("TF_DBG").is_ok() {
            eprintln!("special round: weapon {weapon:#010x} fires its chargeMaxProjectileObj (row {round:#010x})");
        }
        if let Some((point, target)) = landed {
            // The hit's normal for the impact's orientation (0077c180): the world hit's; none for a body hit.
            let hit_normal = if target.is_none() { world_hit.as_ref().map_or(Vec3::ZERO, |h| h.normal) } else { Vec3::ZERO };
            if target.is_none() && let Some(hit)=world_hit {
                self.surface_impacts.push((round,hit));
            }
            // [stand-in: the arena's walls and ground are the default material; the arena is ours, not a game
            // level. settles: the game's surface of a level hit, the collision material of the stage's
            // geometry (`surface.rs` / the level's collision data, not read for a stage here)]
            let surface = target.and_then(|id| targets.iter().find(|t| t.id == id)).map_or(crate::melee::surface::DEFAULT, |t| t.surface);
            // [game 007ae010 -> 00784080 -> 007832b0, read 2026-10-20] The impact lands in the update that fires:
            // weapon_fire calls bullet_trace_setup (its only callers are 007ae010 and charge_fire 007af210), which
            // calls gun_round_trace directly, and its `world_ray_sphere_query` 005be890 returns the sorted hits in
            // the same call that applies the hit. There is no asynchronous probe on this path.
            sound.inspect(|s| s.weapon_impact(round, surface, point));
            self.fx.push(WeaponFx { kind: WeaponFxKind::Impact(surface, hit_normal), weapon: round, at: point, along: end - from });
            self.hits += 1;
            self.impacts.push(Impact { at: point, age: 0.0, explosive: false });
            if let Some(target) = target {
                // The part's `positionalDamageMultiplier` (his rounds: a player's); a Head sets the
                // packet's head-shot bit. [game 00782b30]
                let damage = weapons::bullet_damage(tuning, charge, part, (point - muzzle).length(), true);
                let mut packet = crate::damage::Packet::bullet(tuning, damage, (end-from).normalize_or_zero());
                packet.headshot = part == Part::Head;
                if self.type6_special.is_some() {
                    // [game 00782b30] his type-6 special adds SLOW to the round's flags; the slow
                    // lasts the weapon's own `slowTime` (Starscream's Gatling gun: 5 s).
                    packet.flags |= crate::damage::flags::SLOW;
                }
                self.struck.push(Struck { target, packet });
            }
            // [data] A bullet whose projectile carries an explosion starts it where the round
            // lands, through the missile's blast path. The explosion's own damage is used
            // (`explosive_damage` at -1 keeps the data's). [assumed] whether the bullet's direct
            // damage also lands is not settled by a recording. [game 007832b0, read 2026-10-20] gun_round_trace only
            // FILLS the direct packet (bullet_damage 00782b30 at l.170 and l.201, into the BulletTrace object) and sends
            // nothing itself; the explosion is not in that function. 007891e0 (the grenade's hit) sends its own damage
            // message (00724700) and, per its ledger line, spawns cluster children, not an explosion. settles: the BulletTrace object's hit path (who
            // sends the packet 007832b0 fills, and whether the round's Explosion component spawns in the same hit),
            // read with a Gatling-round example.
            let explosion = self.held.iter().chain(&self.car).chain(&self.special_weapons)
                .find(|held| held.def.name == weapon).and_then(|held| held.def.explosion_for(alt));
            if let Some(def) = explosion {
                // Its effect is the round's blast emitter, as a missile's: Megatron's fusion-arm
                // special has one and no impact rows, so without it nothing showed where the beam
                // landed. [data roster_effects: 101f8f2f, 62c25f02, ea79e9fc; none has a blast
                // sound or scorch]
                self.fx.push(WeaponFx { kind: WeaponFxKind::Blast, weapon: round, at: point, along: Vec3::Z });
                let own = WeaponTuning { explosive_damage: -1.0, explosive_min_damage: -1.0, ..Default::default() };
                let mut blast = MissileBlast::new(def, &own, point);
                let hits = blast.first(targets);
                self.explode(hits);
                self.blasts.push(blast);
            }
            // A full charge's round of OptimusFlight's cannon (`shellcannonbulletsnaker`) carries a
            // Destructable whose `spawnOnDestruction` holds a `SnakerProjectile`: its ground wave
            // slides on from where the round lands, along its way, with the weapon's `snakerInfo`.
            // [data optimusflight 754586-754760] [assumed] it starts where the round lands (when
            // the Destructable is destroyed: the trace's end or its 2 s `auto_delete_time`, is unread).
            // [game 00554830, read in the ledger] A destroyed owner's death record spawns its scenes at the owner's frame
            // when it dies (damage_pending_build), so the wave starts where the Destructable stands at its death. Where
            // it stands then (the trace's end, or after the 2 s `auto_delete_time`, which 00552270 posts) is not read.
            // settles: the Destructable's motion after its hit and the destroy routing (00724700, 00743a40 with
            // 00554440 for the death record, not read against this round).
            if alt && full_charge && let Some(index) = self.held.iter().position(|held| held.def.name == weapon) {
                let held = &self.held[index];
                if let (Some(check), true) = (held.def.alt_snaker_check, held.tuning.snaker.speed > 0.0) {
                    if std::env::var("TF_DBG").is_ok() {
                        eprintln!("charged snaker: weapon {weapon:#010x} at {point:?} (check {check})");
                    }
                    self.let_wave_go(index, round, point, end - from, check, world);
                }
            }
        }
        let endpoint=landed.map_or(end,|(point,_)|point);
        // Devastator's rounds, for his own tracers (`giant::tracers`) [assumed port design].
        if self.car_rotates {
            self.giant_rounds.push((muzzle, endpoint, tuning.bullet_speed));
        }
        // Every gun's round is its projectile object, whose own ParticleEmitter (the effects
        // row's `trail`) flies with it from the muzzle at `bulletSpeed`: held guns and car guns
        // alike, the alternate round with its own row's. [game 007832b0, 00784080] [data] A row
        // without one draws nothing, as in the original. The damage has already landed in the firing update
        // (007ae010 -> 00784080 -> 007832b0, synchronous, see `shoot`); only the emitter travels. [game]
        self.fx.push(WeaponFx {kind:WeaponFxKind::BulletTrail {end:endpoint,speed:tuning.bullet_speed},
            weapon:round,at:muzzle,along:end-from});
        // The old yellow line, kept as a debug aid only (`TF_TRACERS=1`). [stand-in: debug drawing] (debug
        // flag only: no robot's play draws it unless the environment variable is set)
        if std::env::var_os("TF_TRACERS").is_some() {
            self.tracers.push(Tracer { from: muzzle, to:endpoint, age: 0.0 });
        }
    }

    /// One frame. `aiming`: he is in a weapon state, or in the dodge with the gun out (the
    /// gun comes out on entering a weapon state from any state but those two kinds, and
    /// goes away on leaving one for any other: `FUN_0087f4a0`, `FUN_0087f6f0`); `from_car`:
    /// he was still in the change from the car the update before. `firing` is the firing
    /// state's word that the trigger is held (`State::firing`). `muzzle` is where shots leave him, `body` where he stands, `eye`,
    /// `forward` and `fov` (degrees) the camera, all in the game's space. Gives the pad's
    /// rumble for a round that left: seconds and strength, from the weapon's data.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        aiming: bool,
        from_car: bool,
        clips: Clips,
        firing: bool,
        switch: bool,
        muzzle: Vec3,
        origin: Vec3,
        partner_muzzle: Vec3,
        body: Vec3,
        eye: Vec3,
        forward: Vec3,
        fov: f32,
        world: &dyn Sight,
        targets: &[Target],
        car: &Car,
        assist: &AimTuning,
        damage_effects: &crate::damage::Tuning,
        slowed: bool,
        sound: Option<&dyn WeaponSound>,
        dt: f32,
    ) -> Option<(f32, f32)> {
        let mut rumble = None;
        self.struck.clear();
        self.surface_impacts.clear();
        let right = forward.cross(Vec3::Z).normalize_or(Vec3::X);
        let up = right.cross(forward);
        // The manager's wait for the car's weapon starts as his control mode becomes the
        // car's. [game] The firing state is the car's own while that lasts.
        self.car_wait.step(car.mode, car.wait, dt);
        self.in_car = car.mode;
        let car_firing = firing && car.mode;
        let firing = firing && !car.mode;
        // Taking aim pulls the gun out and letting go puts it away; while his arm does
        // either, or changes weapon, the trigger does not reach the weapon. [game]
        if aiming {
            self.gun.take_aim(clips, &self.held[self.current].def, from_car);
        } else {
            self.gun.put_away(clips, &self.held[self.current].def);
        }
        // The switch goes to the next weapon that has no parent. [game 007b3ea0, 007b4080]
        let next = self.in_hand().find(|&i| i > self.current).or_else(|| self.in_hand().next()).unwrap_or(self.current);
        if switch && !self.switch_was_down && next != self.current {
            self.current = next;
            // The weapon now in hand starts its icon's clock from nothing.
            self.icon_clock = 0.0;
            self.gun.switched(clips, self.hand());
        }
        self.switch_was_down = switch;
        let firing = firing && !self.gun.holds_trigger(clips, firing);
        // The aim-assist object's search: what is in the box in the middle of the picture
        // (the launcher's lock box, or for any other weapon the assist's own, 32 pixels
        // square), within the weapon's assist range. The aim assist slows and pulls the
        // aim by its best, and the manager locks the launcher on it while the trigger is
        // held. [game] [stand-in: only the arena hides a target; the picture is taken to
        // be 16:9] settles: the aspect the game's picture uses (the window's; `tick` takes none, so 16:9 stays at 1579 and 2456)
        //
        // In the car the manager's weapon in hand is the car's (`007b43b0` steps `+0x74` to the
        // `isVehicleModeWeapon` one as control mode 3 starts), and its lock runs when that weapon
        // is a guided launcher and the car's weapon may fire (`manager_lock_on` 007b1bb0 l.14-15:
        // control mode 2, OR `FUN_007b44e0`: in DriveMode / FlyMode with the wait `+0xd0` over).
        // [game] So Starscream's, Breakaway's and Grindor's car missiles lock and steer.
        // [game 0070a0d0, 0071dc20] The aim-assist box search (`aim_assist_search` 007079f0: 128 x 128 box, range 350,
        // three best) runs in `0070a0d0`, which `character_update` 0071dc20 calls for every character, gated only by the
        // global DAT_00dadde8: so it runs in the car too, whatever the form. [assumed] from the driving camera with the
        // lock box and range the weapon uses (the camera's input to the search is not read here). [game 005a6d00]
        // DAT_00dadde8 is global state word +0x8 (base DAT_00dadde0): zeroed by global_state_initialize 005a6d00 and
        // read only by 0070a0d0 (l.9); no writer through an absolute address was found. settles: the writers of
        // global state +0x8 (through the state pointer, not found; an option or a per-frame flag).
        {
            let car_may_fire = car.mode && car.formed && self.car_wait.ready();
            // Devastator's erupting bug bombs are thrown unaimed: they lock on nothing. [assumed port design]
            let car_locks = car_may_fire && self.volcano != Some(self.car_current) && self.car.get(self.car_current)
                .is_some_and(|w| lockon::armed_in(false, true, w.def.kind == Kind::Launcher, w.tuning.guided));
            let held = if car_locks { &self.car[self.car_current] } else { &self.held[self.current] };
            let (t, state) = (&held.tuning, &held.state);
            let firing = if car_locks { car_firing } else { firing };
            let view = View { eye, right, forward, up, fov, aspect: 16.0 / 9.0 };
            let locks = car_locks || lockon::armed_in(aiming, false, held.def.kind == Kind::Launcher, t.guided);
            self.seen = if aiming || car_locks {
                let range = if t.aim_assist_max_range > 0.0 { t.aim_assist_max_range } else { assist.default_max_range };
                let facing = (forward * Vec3::new(1.0, 1.0, 0.0)).normalize_or(forward);
                // `FUN_0070a220`, `FUN_0070a2f0`: a launcher with a lock time has its own box.
                let half = if held.def.kind == Kind::Launcher && t.lock_on_time > 0.0 {
                    lockon::lock_box(t)
                } else {
                    Vec2::new(assist.target_box[0] * 640.0, assist.target_box[1] * 360.0)
                };
                lockon::candidates(&view, body, facing, targets, range, lockon::PICTURE * 0.5, half, self.seen[0], |from, to, _| {
                    world.sight(from, to).is_none()
                })
            } else {
                [None; 3]
            };
            if let Some(sound) = sound { sound.combat_target(self.seen[0].is_some()); }
            let pulling = !t.hold_to_lock_on || (firing && state.may_lock(t));
            let was_locked = self.lock.locked;
            self.lock.step(t, locks, pulling, &self.seen, targets, &view, dt);
            if car_locks && self.lock.locked != was_locked && std::env::var("TF_DBG").is_ok() {
                eprintln!("car lock: weapon {:#010x} locked {:?} (seen {:?})", held.def.name, self.lock.locked, self.seen);
            }
            // The bracket round the target being locked or locked, and how far the lock
            // is, for the reticle (`FUN_007b8420`).
            (self.bracket, self.lock_shown) = (None, 0.0);
            for target in targets {
                let locked = self.lock.locked == Some(target.id);
                if locked || (pulling && self.lock.candidate == Some(target.id)) {
                    self.bracket = reticle::bracket(&view, target.pos, target.height, target.radius, locked);
                    self.lock_shown = if locked { 1.0 } else { self.lock.progress(t) };
                }
            }
        }
        // What lets a weapon's burst go on (`Weapon::update`): his weapons are out in the weapon
        // states and through a dodge out of one (`aiming`, the weapon's byte `+0x38`), and the
        // car's weapons go on in the drive or fly mode. [game 007ac370, 007b2b40, 007b2eb0]
        for held in self.held.iter_mut().chain(self.car.iter_mut()) {
            held.state.set_burst_context(aiming, car.mode);
            held.state.set_owner_dead(self.owner_dead);
        }
        // Megatron's special cancels a charge (`007add20`). [game 007ac370]
        self.charge_rumble = None;
        for held in self.held.iter_mut() {
            held.state.set_charge_cancel(self.charge_cancel);
        }
        for index in 0..self.held.len() {
            // A child takes no trigger of its own: its parent's firing fires it (below). A
            // flamethrower's held trigger is passed on to its child (`0074cd20`), so Long Haul's
            // two flames burn together. [game] It leaves his other hand.
            let parent = self.held[index].parent;
            // The parent passes it on only while it may fire itself (0074cd20 step 2, behind
            // slot `+0x8c` 007a8da0: its heat under the capacity), so the pair stops together
            // when the parent's heat runs over: the child keeps no heat (`burn`). [game]
            let trigger = match parent {
                None => firing && index == self.current,
                Some(p) => firing && p == self.current && self.held[index].def.kind == Kind::Flame
                    && self.held[p].state.heat < self.held[p].tuning.heat_capacity,
            };
            let muzzle = if parent.is_some() { partner_muzzle } else { muzzle };
            let def = self.held[index].def.clone();
            let was_locked = self.held[index].state.overheated_left > 0.0;
            let fired = {
                let held = &mut self.held[index];
                // The snipe clock runs only on the weapon in hand (`weapon_is_in_hand` 007a7470,
                // slot `+0x50` with 0, 0, 1). [game]
                held.state.set_sniping(crate::snipe::clock_runs(self.sniping, index == self.current, held.tuning.parent != 0, held.tuning.is_special));
                held.state.step_status(&held.tuning, trigger, slowed, damage_effects,dt)
            };
            // The charged shot's effects, sounds, clips and rumble, from what the charge did
            // (`Weapon::charge_events`), for the weapon and the same for its child, which its
            // parent drives (`007ad1c0` calls the child's slots after its own; the child's own
            // update returns). [game 007ad1c0, 007adae0, 007addc0, 007b0520, 007b08a0, 007b03c0,
            // 007af210, 007b0410, 007add20]
            // - The start: each one's `chargeId` on its own ChargeDummy, the one-shot start sound
            //   and the loop, and the object's Charging / Charged clips.
            // - A step: the emitter follows the level (not on the start's update), the child's
            //   heat by its own numbers, the pad's rumble.
            // - Full: `chargeMaxReadyId` and `chargeMaxReadySound`.
            // - The release: the emitters stop, the release's sound `chargeFireSound` alone (no
            //   fallback; the ordinary fire sound is not played), then the loop's key-off.
            // - The cancel (no shot): the emitters stop and the loop is keyed off (both: the
            //   kept handle of `KeptSounds::charge`, not a fade). [game] Heat reaching the capacity
            //   while held is the RELEASE (007ad1c0 falls through to 007af210, release sound and all);
            //   heat already full at the update's start sends neither event (007ad1c0 l.13-27 only
            //   zeroes the time held), so nothing sounds then (`crate::weapons::Weapon::charge`).
            // Sounds are started only when the data names one (not 0).
            let charge = self.held[index].tuning.charge;
            let events = self.held[index].state.charge_events;
            if charge.has && parent.is_none() && events != Default::default() {
                let child = self.held[index].child.filter(|_| def.kind != Kind::Flame);
                let mut both = vec![(def.name, muzzle)];
                if let Some(c) = child {
                    both.push((self.held[c].def.name, partner_muzzle));
                }
                let play = |id: u32| if id != 0 { sound.inspect(|s| s.play_event(id)); };
                if events.started {
                    for &(name, at) in &both {
                        self.fx.push(WeaponFx { kind: WeaponFxKind::ChargeStart, weapon: name, at, along: forward });
                    }
                    play(def.charge_start_sound);
                    let held = &mut self.held[index];
                    held.sounds.charge_start(&def, held.state.heat, sound);
                    self.gun.charge_started(&def);
                    if let Some(c) = child {
                        self.held[c].state.charging = true;
                    }
                }
                if let Some((level, before)) = events.step {
                    for &(name, at) in &both {
                        self.fx.push(WeaponFx { kind: WeaponFxKind::ChargeLevel(level), weapon: name, at, along: forward });
                    }
                    if let Some(c) = child {
                        let held = &mut self.held[c];
                        held.state.charge_step_heat(&held.tuning, level, before, crate::sim::GAME_UPDATE);
                    }
                    self.charge_rumble = weapons::charge_rumble(&charge, level, crate::sim::GAME_UPDATE);
                }
                if events.full {
                    for &(name, at) in &both {
                        self.fx.push(WeaponFx { kind: WeaponFxKind::ChargeFull, weapon: name, at, along: forward });
                    }
                    play(def.charge_full_sound);
                }
                if events.released.is_some() || events.cancelled {
                    for &(name, at) in &both {
                        self.fx.push(WeaponFx { kind: WeaponFxKind::ChargeStop, weapon: name, at, along: forward });
                    }
                    if let Some(c) = child {
                        self.held[c].state.charging = false;
                    }
                }
                // The release's sound, then the loop's key-off (007af210: slot `+0x148` then
                // `+0x150`); the cancel keys the loop off alone (007add20). The overheat while
                // charging is the release's path (007ad1c0 falls through to 007af210). [game]
                if events.released.is_some() {
                    play(def.charge_fire_sound);
                }
                if events.released.is_some() || events.cancelled {
                    self.held[index].sounds.charge_stop(sound);
                }
            }
            if was_locked && self.held[index].state.overheated_left <= 0.0 {
                self.held[index].sounds.cooled(&def, sound);
                if parent.is_none() {
                    self.gun.cooled(&def);
                }
                // The idle loop's restart at the lock's end (007a9700 l.41-53) is in the idle
                // pass after the gun's step.
            }
            // A flamethrower burns while its trigger is held and it is not overheated. Each
            // contact of a lit sphere hurts the target, at once and then at most once a
            // `1 / damageTicsPerSecond` per target, through this weapon's own hit list (so each
            // of Long Haul's two flames hurts on its own); the list is freed on the stop.
            // [game 0074d610, 0074c200, 0074d110] The weapon's tick (`Flame::step`) no longer
            // deals damage. [game 0074d610] The contact is a lit sphere touching the victim's own
            // posed bodies (`melee::touch`; the physics contact is body against body) [stand-in:
            // the radii are unscaled, see the contact below. [game 0074e730] the dummy's basis grows by dt / its time
            // field, a VISUAL ball (no damage). [game 0074f380] writes an affine into the dummy's transform array
            // under a lock; whether the contact sphere reads that array is unread (its reader: the flame's sphere
            // test, not found). So the radii here stay unscaled: settles: the reader of the dummy's array (obj+0x68, written by 0074f380 from 0117fe70)]. The spheres lie along the aim from
            // the muzzle, as far out as their nodes are from the barrel's [stand-in: the weapon object's own
            // frame is not carried here, so the nodes' places are the aim's line, not the object's nodes.
            // settles: the flame object's affine (+0xc) and its dummy nodes' places, read on the flame's model
            // (the same question as the muzzle dummy below)].
            // [assumed] a contact is reported every update while a sphere overlaps a body, and
            // the list ages before the contacts of that update. Its "rounds" below only heat it, one every
            // `rateOfFire`: the flame's own held-trigger slot 0074cd20 (slot 49) runs the fire step (vtable +0x60)
            // each time its fire timer comes round and reloads it from `weapon_fire_interval` 007a8f20 (rateOfFire,
            // a slow effect's factor applied) [game 0074cd20 step 1, notes\weapon-flamethrower.md 3a]; it does not
            // go through 007ae010. 007a9dd0 is a fire sound's start with a cooldown. [game 0074d610 l.1-60] The handler skips a victim whose
            // list entry has a timer above 0 (`+0x10`), and a packet adds 1 / tics to it. [game 0074c200 l.153-208,
            // read 2026-10-20] The ageing is the flame's own update: an entry whose timer is under 0 is freed, any
            // other loses dt (`FlameHits::age`). settles: the order of the physics contacts against the object
            // update that runs 0074c200 (0074d610 is reached through the listener 0074f000, whose caller 0074b180 is
            // the flame component's constructor, not the dispatcher; the dispatch is not read).
            if def.kind == Kind::Flame {
                let held = &mut self.held[index];
                let burning = trigger && held.state.overheated_left <= 0.0 && held.state.heat < held.tuning.heat_capacity;
                held.flame.step(&held.tuning, burning, dt);
                if !burning {
                    held.flame_hits.clear();
                }
                let lit = held.flame.lit(&def);
                let tuning = held.tuning.clone();
                for _ in 0..held.flame.updates_taken() {
                    // It heats while it burns, by the update (`WeaponState::burn`); a child keeps
                    // none (`weapon_add_heat` 007a90c0 returns for a weapon with a `weaponParent`,
                    // slot `+0xdc` 0074f100). [game 0074c36a]
                    if burning && self.held[index].parent.is_none() {
                        self.held[index].state.burn(&tuning, crate::sim::GAME_UPDATE);
                    }
                    self.held[index].flame_hits.age(crate::sim::GAME_UPDATE);
                    for target in targets {
                        // The victim's bodies where it stands: its posed collision bodies (`targets::animate`),
                        // each a `Character` primitive, so each sends PUNCH [data, `PartBody`]; its cylinder
                        // where no pose was given [stand-in]. [checked 2026-10-20] `targets::animate` (targets.rs
                        // 700-780) pushes a `TargetParts` for every enemy with a model, so the cylinder runs only for
                        // a target with no entry (a scout with no model; a frame before its first pose is not checked).
                        // [assumed] OPEN: the game has no such target in a flame's reach (not read). settles: the
                        // scout list's makeup (targets.rs 700-705) against the candidates of 00799cd0.
                        let bodies: Vec<crate::melee::Body> = match self.target_parts.iter().find(|p| p.id == target.id) {
                            Some(parts) => parts.placed(target.pos).into_iter().map(|part| part.body).collect(),
                            None => vec![crate::melee::Body::of_target(target)],
                        };
                        // The contact point: the point of the victim's body that a lit sphere touches
                        // [game 0074d610: the physics contact of the two bodies]. [stand-in: the physics
                        // contact's point is the same touch, but the sphere's radius is not scaled by the
                        // dummy's growth: 0074e730 scales the dummy's basis, 1 to 1.25 / 1.35 over 0.5 s
                        // [data]; whether the body follows that basis is unread (0117fe70 -> 0074f380).
                        // settles: the physics holder the on-dummy body registers with (0074e8c0 -> 006dcaa0) and
                        // whether its shape is re-read from the dummy's basis each step (0117bbb0 not read)].
                        let contact = def.flame[..lit].iter().find_map(|sphere| {
                            let centre = muzzle + forward * sphere.at.distance(def.muzzle);
                            let ball = crate::melee::Body::new(
                                crate::formats::scene::Shape::Sphere { radius: sphere.radius },
                                Affine3A::from_translation(centre),
                            );
                            bodies.iter().find_map(|body| crate::melee::touch(&ball, body))
                        });
                        let Some(point) = contact else { continue };
                        if !self.held[index].flame_hits.may_hit(target.id) {
                            continue;
                        }
                        // The player's line of sight, muzzle to the contact point: the first
                        // thing on it must be the victim. [game 0074da0b-0074db13; AI shooters
                        // skip it] [game 007aa190] slot 36 is the object's `MuzzleFlashDummy` node, else its
                        // `FireDummy` node, else the object's origin; the flame's `minFireDummyIndex` is 0
                        // [data], so `muzzle` (the drawn object's FireDummy) is slot 36 unless the object has a
                        // MuzzleFlashDummy node [assumed: none on a flame; the model's node list is not checked].
                        // [data longhaul.txt 588893-588912] The flame's data names maxFireDummyIndex 0 and a
                        // muzzleFlashObject of its own, and its dummy objects are named `FlameThrowerDummy`
                        // (591044, 591640, 592062), not MuzzleFlashDummy; the object's node list (007aa190 reads
                        // `object+0x74`) is not in the data. settles: the flame model's node list (for
                        // 0xefe3b8b, read on the flame's model) and 007aa190's slot-36 choice.
                        let reach = (point - muzzle).length();
                        if world.sight(muzzle, point).is_some_and(|hit| (hit.point - muzzle).length() < reach - 1e-3) {
                            continue;
                        }
                        if targets.iter().any(|other| other.id != target.id && enters(muzzle, point, other).is_some_and(|k| k < 1.0)) {
                            continue;
                        }
                        self.hits += 1;
                        // The packet's "from" is the weapon object's origin (`param_2[1]+0xc` +0x34/+0x38),
                        // not a barrel: `origin`, the node the object hangs on with an identity transform
                        // (`attach_models`). [game 0074d610 l.183-184]
                        self.struck.push(Struck { target: target.id, packet: weapons::flame_packet(&tuning, origin, target.pos) });
                        // [assumed] the scout takes it (packet +0x71); the receiver's answer is not fed back.
                        self.held[index].flame_hits.taken(target.id, &tuning);
                        if std::env::var("TF_DBG").is_ok() {
                            eprintln!("flame hit: target {} for {:.1} (weapon {:#010x})", target.id, weapons::Flame::tick_damage(&tuning), def.name);
                        }
                    }
                }
                // The flame's effects and loop: the stream and the loop start with the first
                // sphere; each sphere's looping effect starts as it lights; all stop when it
                // goes out (the release or the overheat). [game 0074d110, 0074e9b0, 007a9b40, 007a9d40]
                let lit_now = self.held[index].flame.lit(&def);
                let was_lit = self.held[index].fx_lit;
                if lit_now != was_lit && std::env::var("TF_DBG").is_ok() {
                    eprintln!("flame: spheres lit {was_lit} -> {lit_now} of {}", def.flame_count);
                }
                if lit_now > was_lit {
                    if was_lit == 0 {
                        self.fx.push(WeaponFx { kind: WeaponFxKind::FlameStream, weapon: def.name, at: muzzle, along: forward });
                        // `flame_set_firing(1)` 0074d110 -> 007a9b40: the flame's loop, kept. [game]
                        let held = &mut self.held[index];
                        let heat = held.state.heat;
                        held.sounds.fire(&def, &held.tuning, heat, sound);
                    }
                    for sphere in &def.flame[was_lit..lit_now] {
                        self.fx.push(WeaponFx { kind: WeaponFxKind::FlameBall(sphere.node), weapon: def.name, at: muzzle, along: forward });
                    }
                } else if lit_now == 0 && was_lit > 0 {
                    self.fx.push(WeaponFx { kind: WeaponFxKind::FlameStop, weapon: def.name, at: muzzle, along: forward });
                    // `flame_set_firing(0)` -> 007a9d40: a key-off, so the flame's tail plays. [game]
                    self.held[index].sounds.cut(sound);
                }
                self.held[index].fx_lit = lit_now;
                // The impact: each update while it burns, a ray from the muzzle to the last lit
                // dummy; the first hit that is not a character takes the impact effect, so the
                // arena's surface does and the scouts never do (`Flame::reach`). [game 0074c200
                // step 6] The level's Destructables (`props`) are not tested [stand-in: the
                // game's invulnerable test on them is not carried].
                let reach = self.held[index].flame.reach(&def);
                let hit = (lit_now > 0 && reach > 0.0)
                    .then(|| world.sight(muzzle, muzzle + forward * reach).map(|h| (h.point, h.normal)))
                    .flatten();
                match hit {
                    Some((at, normal)) => {
                        self.fx.push(WeaponFx { kind: WeaponFxKind::FlameImpact, weapon: def.name, at, along: normal });
                    }
                    None if self.held[index].fx_impact => {
                        self.fx.push(WeaponFx { kind: WeaponFxKind::FlameImpactLost, weapon: def.name, at: muzzle, along: forward });
                    }
                    None => {}
                }
                if hit.is_some() != self.held[index].fx_impact && std::env::var("TF_DBG").is_ok() {
                    eprintln!("flame impact: {} (reach {reach:.1} m{})", if hit.is_some() { "on" } else { "off" },
                        hit.map_or(String::new(), |(at, _)| format!(", at {:.1} m", at.distance(muzzle))));
                }
                self.held[index].fx_impact = hit.is_some();
            }
            let rounds = match fired {
                Fired::Nothing => 0,
                Fired::Refused => {
                    sound.inspect(|s| s.play_event(def.refused_sound));
                    0
                }
                // The overheat's sound comes after this firing's fire sounds (below the rounds).
                Fired::Rounds(rounds) | Fired::RoundsAndOverheated(rounds) => rounds,
            };
            // His arm and the object's clips follow the weapon in hand; a child's object takes
            // them from its parent (`007ab0f0`...). [game]
            // A charged release's rounds are one firing (`007af210`): one recoil message
            // `ab304c20` and one rumble for all of them, and only `chargeFireSound` (above). [game]
            let released = self.held[index].state.charge_events.released.filter(|_| parent.is_none() && rounds > 0);
            if parent.is_none() {
                // `I_WEAPON_INDEX` at the message is the weapon that fires: on an alternating
                // pair's child turn (the parent's counter `+0x39c` is 1) the child's slot, the
                // counter then going back to 0, as the shooters below take it. [game 007b51b0, 007ae010]
                let alternating = self.held[index].tuning.alternating_fire
                    && self.held[index].child.is_some_and(|_| def.kind != Kind::Flame);
                let mut turn = self.held[index].turn;
                for _ in 0..if released.is_some() { rounds.min(1) } else { rounds } {
                    let hand = self.hand_of(index);
                    self.gun.fired_on_turn(clips, hand, alternating && turn != 0);
                    if alternating {
                        turn = if turn == 0 { 1 } else { 0 };
                    }
                }
                if matches!(fired, Fired::RoundsAndOverheated(_)) {
                    self.gun.overheated(clips, &def);
                }
            }
            let (parent_index, parent_def, parent_muzzle) = (index, def, muzzle);
            // A charged release fires the child FIRST with the parent's level (`007ad1c0` tail:
            // the child's `+0x138` before the parent's): its own count, damage multiplier, full
            // flag and heat by its own min / max, in place of a round's heat. For a count of 2
            // or more its count equals the parent's, so the loop below keeps its shape. [game]
            if let (Some(level), Some(c)) = (released, self.held[parent_index].child.filter(|_| parent_def.kind != Kind::Flame)) {
                let held = &mut self.held[c];
                if held.tuning.charge.has {
                    held.state.charge_release(&held.tuning, level);
                }
            }
            for _ in 0..rounds {
                // Who fires this firing (`007ae010`): with a child, the child fires as well
                // (and first), or with `alternatingFire` the two take turns. The parent's heat
                // and rate are spent either way (its step above; the game's tail `007af120`
                // runs on the child's turn too); the child's own fire adds its own heat, with no
                // test of it, and its rounds are not charged (it never charges, `007ad1c0`).
                // [game] A flamethrower's child burns by its own trigger instead (above).
                let child = self.held[parent_index].child.filter(|_| parent_def.kind != Kind::Flame);
                let shooters = match child {
                    None => [Some(parent_index), None],
                    Some(child) if !self.held[parent_index].tuning.alternating_fire => [Some(child), Some(parent_index)],
                    Some(child) => {
                        let turn = self.held[parent_index].turn;
                        self.held[parent_index].turn = if turn == 0 { 1 } else { 0 };
                        [Some(if turn == 0 { parent_index } else { child }), None]
                    }
                };
                for index in shooters.into_iter().flatten() {
                let (def, muzzle) = if index == parent_index { (parent_def.clone(), parent_muzzle) } else { (self.held[index].def.clone(), partner_muzzle) };
                let mut heat_before = self.held[index].state.heat_at_fire;
                if index != parent_index && released.is_none() {
                    let held = &mut self.held[index];
                    heat_before = held.state.heat;
                    held.state.add_heat(&held.tuning, held.tuning.heat_per_round);
                    // [game 007ae010] the fire sound starts per firing with that firing's heat (the port's loop is per
                    // round: a child's round takes its own heat before it; a parent's round takes `heat_at_fire`, the
                    // step's heat before its last round, not per round). [data] the heat parameter drives nothing in the
                    // roster's fire events (r3-sound 1.6), so the difference is not heard. Not changed.
                }
                // A flame's rounds only heat it: its fire sound is the loop started above. [game 007a9b40]
                // A charged release's sound is `chargeFireSound` alone (`007b0410`; slot `+0x58`
                // is never called from `007af210`). [game]
                // Each shooter starts its own (a child plays its own sound, `weapon_fire` 007ae010
                // l.422 is outside the parent / child test), kept per `KeptSounds::fire`. [game]
                // `heat` is the shooter's heat before its round (a parent's round: `heat_at_fire`, see the note
                // above). The parameter drives nothing in the roster's events [data, r3-sound 1.6].
                if def.kind != Kind::Flame && released.is_none() {
                    let held = &mut self.held[index];
                    held.sounds.fire(&def, &held.tuning, heat_before, sound);
                }
                let tuning = &self.held[index].tuning;
                // While sniping a player's rounds are not scattered (`FUN_007aa210`), and the
                // weapon in hand's barrels are the camera's matrix lowered by 1.5 (`007a7a40`).
                // [game]
                let sniping = self.sniping && index == self.current && tuning.snipe.camera_enabled;
                let noise = if sniping { 0.0 } else { tuning.player_noise_radius };
                let shots = tuning.projectiles_per_fire.max(1);
                let muzzle = if sniping { eye - Vec3::Z * crate::snipe::BARREL_DROP } else { muzzle };
                // He shoots at the weapon manager's aim point: what the middle of the
                // picture is on, up to 500 away. [game]
                let target = aim_point(eye, forward, body, world, targets);
                rumble = Some((tuning.rumble_fire_duration, tuning.rumble_fire_strength));
                for _ in 0..shots {
                    let (a, b) = (self.random(), self.random());
                    // The round is sent at the aim point, scattered in the picture. [game]
                    let at = weapons::scattered_point(target, eye, right, up, fov, noise, a, b);
                    let direction = (at - muzzle).normalize_or(forward);
                    self.rounds += 1;
                    match def.kind {
                        // The round is traced from the muzzle towards the scattered point
                        // (the camera only starts a sniping shot's line). [game]
                        Kind::Gun => {
                            let tuning = self.held[index].tuning.clone();
                            // A sniping round: traced from the camera (`007832b0`), its damage
                            // multiplier the snipe ramp (slot `+0x160`, 007ac760), and a full steady
                            // makes it the STEADIED round (`fire_choice`). [game]
                            let state = &self.held[index].state;
                            let (multiplier, snipe) = if sniping {
                                let steady = state.snipe_steady_shot;
                                (crate::snipe::damage_multiplier(steady, &tuning.snipe, tuning.damage), Some((eye, steady)))
                            } else if state.charge_damage != 1.0 {
                                // A charged round's own multiplier (007af210). [game]
                                (state.charge_damage, None)
                            } else {
                                // [game 007ac760, 0079b770] out of sniping, an ordinary round's multiplier is
                                // the type-6 special's while it runs (0079b770 returns the damage tuning's
                                // +0xa8, `starscreamSpecialDamageMultiplier`, in single player), else 1.
                                (self.type6_special.unwrap_or(1.0), None)
                            };
                            if sniping && std::env::var("TF_DBG").is_ok() {
                                eprintln!("sniping round: weapon {:#010x} steady {:.3} damage x{:.3} from {:?}", def.name, state.snipe_steady_shot, multiplier, eye);
                            }
                            self.shoot(def.name, 0, muzzle, at, &tuning, multiplier, snipe, world, targets, sound);
                        }
                        // A missile leaves along the muzzle, which in weapon mode is built
                        // from his aim (`FUN_007a7a40`), sent at the aim point and given
                        // the manager's lock (`FUN_007b36c0`). [game camera.md 566-570: the camera follows
                        // the aim] [assumed: the camera's forward is his aim at the launch; the camera lags
                        // the aim (camera.md +0x170), so the muzzle's own aim (FUN_007a7a40) may differ.
                        // settles: the aim's direction against `view.forward` at the launch]
                        Kind::Launcher => {
                            self.fx.push(WeaponFx { kind: WeaponFxKind::Muzzle(0), weapon: def.name, at: muzzle, along: direction });
                            let seed = &mut self.seed;
                            let missile = Missile::launch(muzzle, forward, at, self.lock.locked, &self.held[index].tuning, &mut || roll(seed));
                            self.launched += 1;
                            // His special's missiles are the launcher's `chargeMaxProjectileObj`
                            // (Starscream's `nullray`). [game 007ae010, 007aaeb0]
                            let alt = weapons::fire_choice(self.type6_special.is_some(), 0.0).1 && def.alt_projectile;
                            let round = if alt { self.alt_rounds.insert(self.launched); crate::character::alt_projectile_key(def.name) } else { def.name };
                            sound.inspect(|s| s.projectile_start(self.launched, round, missile.position));
                            self.fx.push(WeaponFx { kind: WeaponFxKind::Trail(self.launched), weapon: round, at: muzzle, along: forward });
                            self.missiles.push((index, self.launched, missile));
                            if let Some(scale) = self.type6_special {
                                self.missile_scale.insert(self.launched, scale);
                            }
                        }
                        // The flame is run above; a "round" of it only heats the weapon.
                        // A turret launcher is not held (`Arsenal::load`); its special throws it. [game]
                        Kind::Flame | Kind::Turret => {}
                        // A grenade is thrown at the scattered aim point (`FUN_00789b80`), with
                        // the thrower's velocity (it counts only with `tackOnOwnerSpeed`). [game]
                        Kind::Grenade => {
                            self.fx.push(WeaponFx { kind: WeaponFxKind::Muzzle(0), weapon: def.name, at: muzzle, along: direction });
                            let seed = &mut self.seed;
                            let grenade = weapons::Grenade::launch(muzzle, forward, at, self.owner_velocity, &self.held[index].tuning, &mut || roll(seed));
                            self.launched += 1;
                            sound.inspect(|s| s.projectile_start(self.launched, def.name, grenade.position));
                            self.fx.push(WeaponFx { kind: WeaponFxKind::Trail(self.launched), weapon: def.name, at: muzzle, along: direction });
                            self.grenades.push((index, self.launched, grenade));
                        }
                    }
                }
                }
            }
            // The kept fire sounds' end. The round that overheats: `weapon_add_heat` 007a90c0
            // l.75-79 calls the overheat start 007a9ec0 (the fire sounds keyed off, then the
            // overheat sound) and, for the weapon in hand, the idle stop 007aa110; this comes after
            // the round's fire sound as 007ae010 starts the sound before it adds the heat. Else
            // the trigger-up branch of the weapon's update keys them off (`Weapon::sounds_cut`,
            // 007ac370 -> 007a9d40). A child is cut with its parent, whose trigger it fires by (its
            // own update never has the trigger); a flame's loop ends with the flame (above). [game]
            {
                let child = self.held[parent_index].child.filter(|_| parent_def.kind != Kind::Flame);
                let overheated = matches!(fired, Fired::RoundsAndOverheated(_));
                let cut = overheated
                    || (self.held[parent_index].state.sounds_cut && parent_def.kind != Kind::Flame && parent.is_none());
                if overheated {
                    self.held[parent_index].sounds.overheat(&parent_def, sound);
                    if parent.is_none() && parent_index == self.current {
                        self.held[parent_index].sounds.idle_stop(sound);
                    }
                } else if cut {
                    self.held[parent_index].sounds.cut(sound);
                }
                if let Some(c) = child.filter(|_| cut) {
                    self.held[c].sounds.cut(sound);
                }
            }
        }
        // The car's weapons: the trigger reaches one only in the car's form and once the
        // manager's wait has run out (`FUN_007b44e0`); they cool like the others meanwhile.
        // A firing's projectiles leave one from each barrel in turn, each sent at the point
        // 100 ahead of its own barrel along the way the car points (`useFacingAsTarget`),
        // scattered like any player's round, and traced from the barrel. [game 007aab70 l.3-12: the
        // aim point is the barrel's place + 100 x its vector] [assumed: the round is made at the barrel
        // it is aimed from. settles: the round's spawn from its picked slot (007a8770, then 007ae010
        // after l.133) against the aim point]
        for index in 0..self.car.len() {
            let trigger = car_firing && car.formed && self.car_wait.ready() && index == self.car_current;
            let def = self.car[index].def.clone();
            let was_locked = self.car[index].state.overheated_left > 0.0;
            let fired = {
                let held = &mut self.car[index];
                held.state.step_status(&held.tuning, trigger, slowed, damage_effects,dt)
            };
            if was_locked && self.car[index].state.overheated_left <= 0.0 {
                self.car[index].sounds.cooled(&def, sound);
            }
            let firings = match fired {
                Fired::Nothing => 0,
                Fired::Refused => {
                    sound.inspect(|s| s.play_event(def.refused_sound));
                    0
                }
                Fired::Rounds(rounds) | Fired::RoundsAndOverheated(rounds) => rounds,
            };
            if self.car_rotates {
                self.giant_firings = self.giant_firings.wrapping_add(firings);
            }
            // The car's gun's sounds, as a held weapon's (`KeptSounds`): each firing's fire sound
            // and ring-off (the Audi's `WPN_VEH_SHARED_GUN_FIRE_LOOP` starts once and is kept),
            // then the overheat (fire sounds keyed off first) or the trigger-up key-off. [game
            // 007a9b40, 007a90c0, 007a9ec0, 007ac370]
            {
                let held = &mut self.car[index];
                for _ in 0..firings {
                    held.sounds.fire(&def, &held.tuning, held.state.heat_at_fire, sound);
                }
                if matches!(fired, Fired::RoundsAndOverheated(_)) {
                    held.sounds.overheat(&def, sound);
                } else if held.state.sounds_cut {
                    held.sounds.cut(sound);
                }
            }
            for _ in 0..firings {
                let tuning = self.car[index].tuning.clone();
                rumble = Some((tuning.rumble_fire_duration, tuning.rumble_fire_strength));
                for pulled in 0..tuning.projectiles_per_fire.max(1) as usize {
                    // A projectile past the last barrel leaves the first (`FUN_007aab70`).
                    // Devastator's rounds go round all his pods' barrels in turn (`load_giant`) [assumed port design].
                    let shot = if self.car_rotates && !car.barrels.is_empty() { self.rounds as usize % car.barrels.len() } else { pulled };
                    let Some(&barrel) = car.barrels.get(shot).or(car.barrels.first()) else { continue };
                    let target = if tuning.use_facing_as_target && !self.car_rotates {
                        weapons::facing_target(barrel, car.forward)
                    } else {
                        aim_point(eye, forward, body, world, targets)
                    };
                    let (a, b) = (self.random(), self.random());
                    let at = weapons::scattered_point(target, eye, right, up, fov, tuning.player_noise_radius, a, b);
                    self.rounds += 1;
                    let barrel_index = if shot < car.barrels.len() { shot } else { 0 };
                    // The car's weapon makes its own kind of round, as a held one does (the same
                    // `007ae010` and projectile makers): a launcher's missile or a grenade leaves
                    // the barrel along the car. [game 007aab70] [assumed: launched along the car's
                    // forward, as `useFacingAsTarget` aims; the barrel's own vector (gun+0x60+0x30i,
                    // read by 007aab70 l.3-10) is not checked against car.forward. The missile's lock
                    // is the manager's, armed in the car too (`car_locks` in the aim assist, 007b1bb0).
                    // settles: the writer of gun+0x60+0x30i (the barrel's facing vector)]
                    match def.kind {
                        Kind::Gun => self.shoot(def.name, barrel_index, barrel, at, &tuning, self.car[index].state.charge_damage, None, world, targets, sound),
                        Kind::Launcher if self.volcano == Some(index) => {
                            // Devastator's eruption [assumed port design]: the bomb is the launcher's
                            // missile (its model, trail, sounds and blast) flown along its arc.
                            if pulled == 0 {
                                self.eruptions = self.eruptions.wrapping_add(1);
                                self.eruption_at = car.barrels.iter().copied().sum::<Vec3>() / car.barrels.len().max(1) as f32;
                            }
                            let seed = &mut self.seed;
                            let lob = crate::giant::Lob::erupt(barrel, self.vent_axis, body, &mut || roll(seed));
                            let mut missile = Missile::launch(barrel, lob.direction(), lob.landing, None, &tuning, &mut || roll(seed));
                            missile.direction = lob.direction();
                            self.fx.push(WeaponFx { kind: WeaponFxKind::Muzzle(barrel_index), weapon: def.name, at: barrel, along: missile.direction });
                            self.launched += 1;
                            self.lobs.insert(self.launched, lob);
                            sound.inspect(|s| s.projectile_start(self.launched, def.name, missile.position));
                            self.fx.push(WeaponFx { kind: WeaponFxKind::Trail(self.launched), weapon: def.name, at: barrel, along: missile.direction });
                            self.missiles.push((CAR_SLOT + index, self.launched, missile));
                        }
                        Kind::Launcher => {
                            // Devastator's missiles come down where he looks, or on the ground ahead when
                            // his look meets nothing (`giant::missile_point`); they fly a top attack, up
                            // out of his pods and down on it (`giant::TopAttack`). [assumed port design]
                            let at = if self.car_rotates { crate::giant::missile_point(eye, forward, at, body.z) } else { at };
                            let seed = &mut self.seed;
                            let dive = self.car_rotates.then(|| crate::giant::TopAttack::launch(barrel, at, body, &mut || roll(seed)));
                            let leaves = dive.map_or(car.forward, |d| d.direction());
                            self.fx.push(WeaponFx { kind: WeaponFxKind::Muzzle(barrel_index), weapon: def.name, at: barrel, along: leaves });
                            let mut missile = Missile::launch(barrel, leaves, at, self.lock.locked, &tuning, &mut || roll(seed));
                            missile.direction = leaves;
                            self.launched += 1;
                            if let Some(dive) = dive {
                                self.dives.insert(self.launched, dive);
                            }
                            sound.inspect(|s| s.projectile_start(self.launched, def.name, missile.position));
                            self.fx.push(WeaponFx { kind: WeaponFxKind::Trail(self.launched), weapon: def.name, at: barrel, along: leaves });
                            self.missiles.push((CAR_SLOT + index, self.launched, missile));
                        }
                        Kind::Grenade => {
                            let direction = (at - barrel).normalize_or(car.forward);
                            self.fx.push(WeaponFx { kind: WeaponFxKind::Muzzle(barrel_index), weapon: def.name, at: barrel, along: direction });
                            let seed = &mut self.seed;
                            let grenade = weapons::Grenade::launch(barrel, car.forward, at, self.owner_velocity, &tuning, &mut || roll(seed));
                            self.launched += 1;
                            sound.inspect(|s| s.projectile_start(self.launched, def.name, grenade.position));
                            self.fx.push(WeaponFx { kind: WeaponFxKind::Trail(self.launched), weapon: def.name, at: barrel, along: direction });
                            self.grenades.push((CAR_SLOT + index, self.launched, grenade));
                        }
                        Kind::Flame | Kind::Turret => {}
                    }
                }
            }
        }
        // Missiles in flight steer at the middle of their target, and end at the first
        // thing in their way or when their time is up; where one ends its explosion is
        // made (`weapons::MissileBlast`). [assumed: any touch ends it] OPEN: the touch is not the
        // MissileProjectile vtable's (00b5ebc8, `pe_read vtable`): slot 2 is `missile_update` 007847e0,
        // slot 3 `missile_message` 00785a60 (54732c8d -> `missile_kill` 00785c00 at the lifetime only),
        // and slots 5, 11, 17, 23 are the shared 00544340. Where a touch is handled is not found in
        // the vtable. settles: the contact handler of the missile's collision (the body's listeners,
        // `+0x94`/`+0xa4`, and whatever they send on a touch), not read.
        // Read 2026-10-20 (wave 24 + lead): the +0x94 ring (key f1e85af0, 007843a0) only sets +0xb6 (00785af0), which
        // only MissileProjectile_deactivate 00784680 reads, for a multiplayer message; missile_update 007847e0 casts no ray
        // and calls no kill. The robots' missile objects carry a Destructable with strength 1 and destructOnKilled
        // (work\survey\weapons\jetfire.txt l.932-953, spawnOnDestruction = the blast): any damage ends it, so the end on a
        // touch is the Destructable's. settles: what damage a touch deals the missile (the contact damage of its body).
        let mut flying = std::mem::take(&mut self.missiles);
        let mut ended: Vec<(usize, u32, Vec3, Vec3)> = Vec::new();
        flying.retain_mut(|(index, number, missile)| {
            let tuning = &weapon_in(&self.held, &self.special_weapons, &self.car, *index).tuning;
            // A target whose dash clock runs (status +0x94) is not steered at: the missile flies on
            // as it points. [game 007847e0]
            let middle = missile
                .target
                .and_then(|id| targets.iter().find(|target| target.id == id))
                .filter(|target| !target.dashing)
                .map(|target| target.pos + Vec3::Z * (target.height * 0.5));
            let seed = &mut self.seed;
            let stepped = match self.lobs.get_mut(number) {
                // An erupted bug bomb flies its arc, not the missile's guidance. [assumed port design]
                Some(lob) => lob.step(dt).inspect(|_| {
                    missile.position = lob.position;
                    missile.direction = lob.direction();
                }),
                // His missiles fly their top attack, the dive following a locked target. [assumed port design]
                None => match self.dives.get_mut(number) {
                    Some(dive) => {
                        if let Some(at) = middle {
                            dive.track(at);
                        }
                        dive.step(dt).inspect(|_| {
                            missile.position = dive.position;
                            missile.direction = dive.direction();
                        })
                    }
                    None => missile.step(tuning, middle, &mut || roll(seed), dt),
                },
            };
            let Some((from, to)) = stepped else {
                // Its time is up: it is killed where it is (`FUN_00785c00`) and goes off.
                ended.push((*index, *number, missile.position, missile.direction));
                return false;
            };
            sound.inspect(|s| s.projectile_move(*number, missile.position, (to - from).length() / dt.max(f32::EPSILON)));
            let mut landed = world.sight(from, to).map(|hit| (hit.point, None));
            for target in targets {
                let Some(k) = enters(from, to, target) else { continue };
                let point = from + (to - from) * k;
                if landed.is_none_or(|(nearest, _)| (point - from).length_squared() < (nearest - from).length_squared()) {
                    landed = Some((point, Some(target.id)));
                }
            }
            let Some((point, target)) = landed else { return true };
            self.hits += u32::from(target.is_some());
            ended.push((*index, *number, point, missile.direction));
            false
        });
        self.missiles = flying;
        // Grenades fall, and go off on a touch, near a target, or when their time is up
        // (`weapons::Grenade`); what they leave is made as a missile's is.
        let mut thrown = std::mem::take(&mut self.grenades);
        let mut newly_armed: Vec<(usize, u32)> = Vec::new();
        thrown.retain_mut(|(index, number, grenade)| {
            let tuning = &weapon_in(&self.held, &self.special_weapons, &self.car, *index).tuning;
            let touch = |from: Vec3, to: Vec3| {
                // The world, or a target's body (the scouts: characters of the other side, so a
                // proximity grenade goes off on them; `Grenade::step`). [game 007876b0]
                let mut found = world.sight(from, to).map(|hit| (hit.point, hit.normal, false));
                for target in targets {
                    let Some(k) = enters(from, to, target) else { continue };
                    let point = from + (to - from) * k;
                    if found.is_none_or(|(nearest, _, _)| (point - from).length_squared() < (nearest - from).length_squared()) {
                        found = Some((point, (from - to).normalize_or(Vec3::Z), target.character));
                    }
                }
                found
            };
            let nearest = |at: Vec3| {
                targets.iter().map(|target| at.distance(target.pos + Vec3::Z * (target.height * 0.5))).min_by(f32::total_cmp)
            };
            let was_armed = grenade.armed();
            let stepped = grenade.step(tuning, &touch, &nearest, dt);
            if !was_armed && grenade.armed() && tuning.grenade.one_out_at_a_time {
                newly_armed.push((*index, *number));
            }
            match stepped {
                Ok((from, to)) => {
                    sound.inspect(|s| s.projectile_move(*number, grenade.position, (to - from).length() / dt.max(f32::EPSILON)));
                    true
                }
                Err(at) => {
                    ended.push((*index, *number, at, grenade.velocity.normalize_or_zero()));
                    false
                }
            }
        });
        self.grenades = thrown;
        // `oneOutAtATime`: a mine that armed this step expires the weapon's other live ones; they
        // fade out without going off. [game 00786a60 l.6-28, 00789b10]
        for (index, number) in newly_armed {
            self.grenades.retain(|(i, n, _)| {
                let expired = *i == index && *n != number;
                if expired {
                    sound.inspect(|s| s.projectile_stop(*n));
                }
                !expired
            });
        }
        for (index, number, at, way) in ended {
            let held = weapon_in(&self.held, &self.special_weapons, &self.car, index);
            let alt = self.alt_rounds.remove(&number);
            self.lobs.remove(&number);
            self.dives.remove(&number);
            // A missile whose object carries a `SnakerProjectile` (in a Destructable's
            // radial_attack: Grindor's and Jetfire's robot-form energon cannons; their data has
            // no Explosion) lets the ground wave go where it ends, along its way. [data: exactly
            // the roster's launchers with a `snakerInfo` speed] [assumed: it starts at the
            // missile's end, not at its launch, with the missile's way (the wave object's Instance
            // matrix, +0.01 z, is data); `checkGroundHeight` and its life are the wave object's
            // (`let_wave_go`). settles: the spawner of the SnakerProjectile at the missile's kill (`missile_kill`
            // 00785c00 sends the kill and explodes, spawns none; 00785b50 / 00789120 are the snakerInfo checks, not read)]
            let waves = !alt && held.def.kind == Kind::Launcher && held.tuning.snaker.speed > 0.0;
            let round = if alt { crate::character::alt_projectile_key(held.def.name) } else { held.def.name };
            if waves {
                let snaker = held.tuning.snaker;
                // [port design] The cinematic wave opens on the ground under the impact, off a wall
                // at its foot (`energon_wave::start`); the snaker in its middle goes from there too.
                let opens = (round == crate::energon_wave::CANNON && self.energon_ring.is_some())
                    .then(|| crate::energon_wave::start(at, way, world))
                    .flatten();
                let (from, along) = opens.unwrap_or((at, way));
                self.let_wave_go(index, round, from, along, 1.0, world);
                if opens.is_some() {
                    self.open_energon(&snaker, at, from, along, world);
                }
            }
            let held = weapon_in(&self.held, &self.special_weapons, &self.car, index);
            sound.inspect(|s| s.projectile_end(number, round, at));
            self.impacts.push(Impact { at, age: 0.0, explosive: true });
            self.fx.push(WeaponFx { kind: WeaponFxKind::Blast, weapon: round, at, along: Vec3::Z });
            // Devastator's blasts shake his camera by how near they land (`giant::dress`). [assumed port design]
            if self.car_rotates {
                self.giant_blasts.push((at, self.volcano.is_some_and(|v| index == CAR_SLOT + v)));
            }
            // Its Destructable's `destroyEffectSpawn` (the lingering smoke). [game] the projectile's end
            // sends the kill to its own Destructable, and its gates pass it: [data] the missile objects'
            // Destructables have `startEnabled` (flag 0x2, 00743a40 l.4-5) and `destructOnKilled` 0 (so the
            // 0x40 early return, l.157-163, is not taken) (work\survey\weapons\starscream.txt l.639546-639595,
            // blackout.txt l.698291-701213, 723461-723510); in single player 00744ce0 gives 1, so l.191-196 take
            // the health path 00554440 with `destroyEffectSpawn` (+0x14c) into the death record: 00554440 passes
            // its last argument as 005544e0's param_4, stored at the entry's [2] / [3], and 00554830 (drained by
            // 005546a0) spawns each `spawnScene` of the view at entry[3] (l.73-86). [game, read 2026-10-20]
            // [game 00785c00, 00724700, 00743a40, 2026-10-20] The end is a
            // kill packet (amount `+0x34` = 100000, flags 0x10000000, key 0x8590b19d) sent through 00724700 to
            // EVERY listener on the object's list, in list order, so a Destructable and a DamageLink on one object
            // both get it (00724700 has no routing of its own). 00743a40 bypasses its damage filter under 0x10000000
            // and sends the health path (00554440) to 0.
            self.fx.push(WeaponFx { kind: WeaponFxKind::AfterBlast, weapon: round, at, along: way });
            // The damage object's ControllerRumble starts as that object is made. [game 007dba80]
            if let Some(rumble) = self.round_fx.iter().find(|r| r.name == round).and_then(|r| r.blast_rumble) {
                self.rumbles.push((at, rumble));
            }
            // The damage object's DetachLink pieces with a Debris (Bumblebee's micro missile: five chunks)
            // are thrown as that object is built: each away from its link's parent's place (the damage point
            // 005522f0 sets), pushed along the damage direction. [game 005522f0, 00552930, 00553840] [assumed]
            // settles: 00554830 (read): the damage direction is `DAT_00d1672c` = the pending record's `+0x10..+0x18`,
            // set by the hit that destroyed the object; for a projectile's end that hit is the one 00724700 routes (not read).
            // the damage object stands on the projectile's frame at its end (+y along its way, as in flight);
            // the damage direction is the projectile's way (DAT_00d1672c comes from the record of the hit that
            // destroyed it, 00554830, which is not traced for a projectile's end).
            if let Some(row) = self.round_fx.iter().find(|r| r.name == round) {
                let frame = Affine3A::from_rotation_translation(weapons::look_along(way.normalize_or(Vec3::Y)), at);
                let frame_turn = Quat::from_mat3a(&frame.matrix3);
                for (k, p) in row.debris.iter().enumerate() {
                    let piece = crate::debris::launch(&p.def, frame.transform_point3(p.at), frame.transform_point3(p.damage_point),
                        way.normalize_or_zero(), &mut self.debris_rng).with_masks(p.masks);
                    self.launched += 1;
                    self.fx.push(WeaponFx { kind: WeaponFxKind::Debris(self.launched, k), weapon: round, at: piece.pos, along: piece.vel });
                    self.debris.push((self.launched, round, k, (frame_turn * p.turn).normalize(), p.scale, piece));
                }
            }
            // [stand-in] a launcher whose data has no explosion does nothing where it lands. [game 00785c00]
            // missile_kill at the end sends only a kill packet to the missile object's listeners (00724700) and stops
            // ticking; the blast is made by the listeners' rows, not by the kill. So nothing is made here without an
            // Explosion. settles: the listeners' side for a missile with no Explosion row (00724700, not read).
            if let Some(def) = held.def.explosion_for(alt) {
                // The launch scaled the damage words themselves (a -1 "keep the data's" stays
                // negative and so still keeps it). [game 00785de0]
                let tuning = match self.missile_scale.remove(&number) {
                    Some(scale) => WeaponTuning {
                        damage: held.tuning.damage * scale,
                        explosive_damage: held.tuning.explosive_damage * scale,
                        explosive_min_damage: held.tuning.explosive_min_damage * scale,
                        ..held.tuning.clone()
                    },
                    None => held.tuning.clone(),
                };
                let mut blast = MissileBlast::new(def, &tuning, at);
                let hits = blast.first(targets);
                self.explode(hits);
                self.blasts.push(blast);
            }
        }
        let mut blasts = std::mem::take(&mut self.blasts);
        blasts.retain_mut(|blast| {
            let (hits, alive) = blast.step(targets, dt);
            self.explode(hits);
            alive
        });
        self.blasts = blasts;
        // The thrown pieces fly (`crate::debris::Piece::step`: [stand-in] gravity, damping and a floor
        // bounce for the Lux solver) and touch what their masks meet: a character's limb boxes are
        // `Character` [data], so a piece hurts one only if it takes PUNCH, once, at its first contact
        // (00554090, packet 00744670: flags 0x400) [game]. Bumblebee's pieces take only PUSH and CAMERA
        // (`DebrisSmall`), so they pass through the targets and their 7 / 10 is never dealt [game + data].
        // [stand-in] a target is touched when the piece's point is within its upright cylinder widened by the
        // piece's largest extent (the game's contact is the posed bodies, which the flame's touch tests through
        // `target_parts` above; this test does not). settles: a piece's PUNCH contact against those posed bodies
        // for a robot's debris. [game 00554090] On a PUNCH contact (the other body's mask & 2) a debris body with
        // damage `+0x60` > 0 calls the hook (`scripted_damage_packet` 00744670) with that amount and the body's
        // `+0x70` handle as the packet's target (00744670's 4th argument is the target). So the piece's damage goes to
        // the handle at `+0x70`, not to the contacted body: the port's touch rule differs in the target. Not changed
        // (the handle's owner is not read: the port's note says the parent object). settles: what sets `+0x70`.
        // settles for the bounce: QUEUED, a recording of a thrown chunk's landing (the Lux solver's run-time numbers).
        let floor = |p: Vec3| world.sight(p + Vec3::Z * 1.0, p - Vec3::Z * 50.0).map(|hit| hit.point.z);
        let mut thrown = std::mem::take(&mut self.debris);
        thrown.retain_mut(|(_, round, k, _, _, piece)| {
            let alive = piece.step(dt, floor);
            let reach = self.round_fx.iter().find(|r| r.name == *round).and_then(|r| r.debris.get(*k)).map_or(0.0, |p| p.radius);
            for target in targets.iter().filter(|t| t.character) {
                let d = piece.pos - target.pos;
                if d.truncate().length() > target.radius + reach || d.z < -reach || d.z > target.height + reach {
                    continue;
                }
                if let Some(amount) = piece.touch(crate::debris::CHARACTER) {
                    self.struck.push(Struck { target: target.id, packet: crate::damage::Packet {
                        amount, flags: crate::debris::DAMAGE_FLAGS, direction: piece.vel.normalize_or_zero(), ..Default::default()
                    }});
                }
            }
            alive
        });
        self.debris = thrown;
        // The ground waves slide on (`weapons::Snaker`); one that reaches a target sends it the
        // wave's damage with its flags, pushed along its way. [game 0078e950] The reach round
        // its point is its object's Collision sphere (1.36743 [data]). [assumed: that sphere
        // against the target's cylinder stands for the contact test, which is not read; each
        // target is struck once by a wave] settles: 0078e950's reach test (the wave's contact with a body, not read).
        // Its picture and light are drawn by `effects`
        // (`WeaponFxKind::Wave`, `wave_frame`); its SoundPlayers (1444015253 `ANIM_BUMB_CHARGE_LP`,
        // 2106349104 `WPN_BLAC_CAN_IMP`) by `wave_sounds` after the step.
        let mut snakers = std::mem::take(&mut self.snakers);
        snakers.retain_mut(|(index, number, wave, struck, reach)| {
            let t = weapon_in(&self.held, &self.special_weapons, &self.car, *index).tuning.snaker;
            wave.step(&t, world, dt);
            let reach = *reach;
            // [port design] The snaker in the middle of a cinematic wave hurts no one: its ring does.
            let quiet = self.energon.iter().any(|e| e.snaker == *number);
            for target in targets.iter().filter(|_| !quiet) {
                let d = wave.position - target.pos;
                let reaches = d.truncate().length() <= target.radius + reach
                    && d.z >= -reach
                    && d.z <= target.height + reach;
                if !reaches || struck.contains(&target.id) {
                    continue;
                }
                if let Some((amount, push)) = wave.touch_character(&t) {
                    struck.push(target.id);
                    self.hits += 1;
                    self.struck.push(Struck { target: target.id, packet: crate::damage::Packet {
                        amount, flags: t.damage_flags, direction: push.normalize_or_zero(), ..Default::default()
                    }});
                }
            }
            wave.alive
        });
        self.snakers = snakers;
        self.wave_sounds(sound);
        // [port design] Grindor's cinematic waves (`crate::energon_wave`): each ring arc runs on and
        // hurts what it reaches inside it once, with Devastator's query, height check and knock-back
        // (`ShockWave::update`), weakening as it runs; the candidates are the targets its ground lets
        // through (in front of a wall, on the ground it runs on: `Ground::candidates`). Its ground is
        // sampled ahead of the front first; once walls have stopped all of it, it ends (`end_radius`).
        if !self.energon.is_empty() {
            use crate::shockwave::{Candidate, Size};
            let candidates: Vec<Candidate> = targets.iter().map(|t| Candidate {
                id: t.id, position: t.pos, radius: t.radius,
                size: t.character.then_some(if t.large { Size::Large } else { Size::Small }),
            }).collect();
            let (struck, hits) = (&mut self.struck, &mut self.hits);
            for wave in &mut self.energon {
                wave.ground.advance(&wave.ring, world, dt);
                let let_through = wave.ground.candidates(&wave.ring, &candidates);
                wave.ring.update(dt, u32::MAX, None, &let_through, |hit| {
                    struck.push(Struck { target: hit.target, packet: hit.packet });
                    *hits += 1;
                    true
                });
                if wave.ring.rings.first().is_some_and(|r| r.radius >= wave.ground.end_radius(&wave.ring)) {
                    wave.ring.rings.clear();
                }
            }
            self.energon.retain(|wave| wave.ring.active());
        }
        for tracer in &mut self.tracers {
            tracer.age += dt;
        }
        for impact in &mut self.impacts {
            impact.age += dt;
        }
        self.tracers.retain(|tracer| tracer.age < TRACER_SECONDS);
        self.impacts.retain(|impact| impact.age < IMPACT_SECONDS);
        self.gun.step(clips, self.hand(), dt * self.anim_rate);
        for command in self.gun.sound_commands.drain(..) {
            if let Some(sound) = sound { sound.weapon_clip_command(command); }
        }
        // [assumed] a hidden weapon object's held clip sounds end with it (Grindor's servo loop
        // has no stop on the way out). Settles: 00596a00's tie walk, or the original's put-away.
        if !self.gun.shown && let Some(sound) = sound {
            sound.weapon_object_hidden();
        }
        // The idle loop (`weaponIdleSound`, Starscream's Gatling gun and the Audi's SlagRepeater):
        // it starts when the weapon in hand is shown (`weapon_show` 007a7630 -> 007aa070, unless
        // overheat-locked, slot `+0xf8`) or its lock runs out while in hand (007a9700 l.41-53),
        // and stops with its fade on the hide (`weapon_hide` 007a7750), on leaving the weapon
        // states (`weapon_set_out(0)` 007a75a0) and at the overheat (007a90c0 l.75-79, above).
        // [game] Taken here as one condition per weapon: in hand, his weapons out (`aiming`), its
        // object shown (`gun.shown`), not in the car, not locked. [game 007a7630, 007a7750, 007b4680]
        // `gun.shown` is the object's visibility, bit0 of its `+0x60` clear: weapon_show (slot `+0x24`)
        // clears it and starts the idle loop (007aa070, at the show), weapon_hide (slot `+0x28`) sets it.
        // The Attach event's hide (007b4680 l.70-81) sets the bit itself and calls weapon_set_out(0)
        // (slot `+0x1c`) instead of `+0x28`; its unhide calls `+0x24`. The port writes `shown` from the
        // same two flags (core gun.rs), and tests the idle's edge each frame where the game starts it
        // once at the show. [game 2026-10-20] 007a7470 (read in full) tests, in order: the
        // vtable slot +0x9c (isSpecial) is 0; the parent slot +0xdc is null or the second argument is 0; the
        // owner's `+0x31c` is not 1 unless the first argument is nonzero (`param_2 != 0 || owner+0x31c != 1`);
        // then the manager's in-hand weapon 007b5030 is this one (or, with the second argument 0, shares its
        // parent). `find_call` finds no direct `call` to 007a7470 (it may be reached through a vtable),
        // so the form gate's first argument is unread. The port's `wanted` already needs `aiming` (form 2) and
        // not `car.mode`, which covers the foot case the gate excludes for a nonzero first argument.
        // The `+0x31c` values are 1 on foot, 2 weapon, 3 car (state-machine.md l.98). OPEN: the caller that
        // reaches 007a7470 through its vtable slot (the indirect call, RD516162).
        for i in 0..self.held.len() {
            let held = &mut self.held[i];
            let wanted = i == self.current && aiming && self.gun.shown && !car.mode && held.state.overheated_left <= 0.0;
            if wanted {
                held.sounds.idle_start(&held.def, sound);
            } else {
                held.sounds.idle_stop(sound);
            }
        }
        // The reticle. The hit mark's clock is the source's +0x234 +0x30, stamped with the game time by
        // 00729120 (l.8705-8708) and 00743a40 for ANY damage packet of his with amount > 0 [game, wave 26 read
        // 2026-10-20]: here a round or blast that struck (`struck`), and his melee hits (`mark_hit`, player.rs).
        // Port-checked (wave 28 P5): every damage packet of his reaches `struck`: rounds through `shoot`
        // (his guns, the specials and the turrets' guns via `tick_turrets`), flames through `flame`, blasts
        // through `explode` (missiles, grenades, mines), the ground waves and the debris; targets.rs reads it.
        // [game] a weapon's icon clock (its word 0xf6) runs while it is the manager's
        // weapon in hand, and is 0 otherwise (slot `+0x50` = `FUN_007a7470`, asked through
        // `FUN_007b5030`): so only the weapon in hand has an icon, and it comes up afresh
        // on a switch and on leaving the car (in the car the weapon in hand is the car's).
        // [data] his weapons are not special: Bumblebee's three weapons all have `is_special` false
        // (`weapons_check bumblebee`, 2026-10-20), so the vtable slot +0x9c test of 007a7470 passes for each.
        // And have no parent. Slot `+0xdc` is `weapon_parent_of` 0074f100: the parent weapon's name hash
        // from the weapon's attribute `weaponParent` (0 for none) [game 0074f100]. The paired weapons are the
        // data's `weaponParent` / `weaponChildren` pairs, and Bumblebee's three are in none of them
        // (notes/character-weapon-data.md, Paired weapons) [data]. So 007a7470's parent test takes its null
        // branch for each of them. The earlier reading of the parent as a scene link was wrong: the
        // `bumblebee.txt` Objects carry no parent field because the link is an attribute, not a scene link.
        self.since_hit = if self.struck.is_empty() { self.since_hit + dt } else { 0.0 };
        self.icon_clock = if car.mode { 0.0 } else { self.icon_clock + dt };
        let side = |held: &Held, shown_for: f32| reticle::Side {
            kind: held.def.kind,
            heat: held.state.heat,
            capacity: held.tuning.heat_capacity,
            charged_shot: held.tuning.charge.has,
            projectiles: held.tuning.projectiles_per_fire,
            shown_for,
            // `snipeCameraEnabled` gives the sniper's icon (slot `+0xbc`). [game 007b7560]
            sniper: held.tuning.snipe.camera_enabled,
        };
        let view = View { eye, right, forward, up, fov, aspect: 16.0 / 9.0 };
        let car_gun = self.car.get(self.car_current).filter(|_| car.mode && !car.barrels.is_empty());
        self.giant_icon += dt;
        let frame = match car_gun {
            // Devastator's pods aim down the camera's line, as a weapon in hand does: his reticle is the
            // weapon mode's, not the car's two bases, so it shows the pod kind in use (its icon comes
            // up on a switch and stays small beside the middle), his missiles' lock (the bracket and
            // the four lines), the target under it and his hits. The erupting bug bombs, thrown, take
            // the grenade's base and icon. [assumed port design]
            Some(gun) if self.car_rotates => {
                let lobbed = self.volcano == Some(self.car_current);
                let mut pod = side(gun, self.giant_icon);
                if lobbed {
                    pod.kind = Kind::Grenade;
                }
                reticle::Frame {
                    weapon_mode: true,
                    primary: Some(pod),
                    current: Some(pod.kind),
                    hand_charged: Some(false),
                    on_target: !lobbed && self.seen[0].is_some(),
                    lock: self.lock_shown,
                    bracket: self.bracket,
                    since_hit: self.since_hit,
                    ..Default::default()
                }
            }
            // The car's gun: its two bases lie 30 and 50 along the `Flipper` node's +y row (the second row
            // of its world matrix), from the camera target point 0071a2e0, which overwrites the matrix's
            // translation row. [game 007b8420 at 007b94d2-007b9600, read 2026-10-20; 30 = 00b34570]
            // The game uses the row as it is; the port normalises it, which changes nothing: measured
            // 2026-10-20 (TF_DBG probe), Bumblebee's car (72 nodes) and Optimus's (124) both have the
            // node and its row is unit length. A car with no Flipper node falls back to its forward.
            Some(gun) => {
                let from = car.target_point;
                let axis = car.flipper.and_then(Vec3::try_normalize).unwrap_or(car.forward);
                reticle::Frame {
                    primary: Some(side(gun, 0.0)),
                    current: Some(gun.def.kind),
                    hand_charged: Some(gun.tuning.charge.has),
                    since_hit: self.since_hit,
                    car: Some((view.project(from + axis * 30.0), view.project(from + axis * 50.0))),
                    ..Default::default()
                }
            }
            None => reticle::Frame {
                weapon_mode: aiming,
                // [assumed] the HUD's two weapons are the first two that can be in hand. [game 007b8420 l.95-99,
                // 2026-10-20] One call per frame, `reticle_draw_icons` 007b7560 (`this` = the object in EDI at the
                // call; param_1 = the object at the HUD record's `+0x238` +0x3c, checked by its `+0x30` slot), draws
                // TWO icons [game 007b7560, read 2026-10-20]: `this`'s weapon (`this+0xe8`, kind by `this`'s slot
                // +0x88) and param_1's weapon (`param_1+0xe8`, kind by its slot +0x88). So the second side icon is the
                // `+0x238` object's, not a second call's. [game 007b7560, decompile l.73 and l.230-248] The sides: `this`'s
                // icon is the LEFT one (x = 640 - offset; the charge branch l.115 at 550) and param_1's the RIGHT one
                // (x = 640 + offset; l.183-215 at 730 for a full charge with no weapon mode). So the first call's icon is
                // the left side and the second's the right [game]. Which of the two is the port's in-hand first is not
                // settled here: OPEN: settles: the writer of the `+0x238` record and the in-hand order of the manager
                // (hud.md; 007b7f40 reads the same list).
                primary: self.in_hand().next().map(|i| side(&self.held[i], if self.current == i { self.icon_clock } else { 0.0 })),
                secondary: self.in_hand().nth(1).map(|i| side(&self.held[i], if self.current == i { self.icon_clock } else { 0.0 })),
                current: Some(self.held[self.current].def.kind),
                // Which of the two sides is the weapon in hand (`reticle_pick_base` 007b7f40 reads it
                // from the manager): its place in the in-hand list, the first two of it. [game 007b7f40]
                hand_charged: Some(self.held[self.current].tuning.charge.has),
                current_sniper: self.snipe_in_hand(),
                on_target: aiming && self.seen[0].is_some(),
                lock: self.lock_shown,
                bracket: self.bracket,
                since_hit: self.since_hit,
                car: None,
                // The charge bar: the weapon in hand's steady while sniping, else its charge
                // level (-1, no bar, without a charged shot). [game 007b8420 l.709-718]
                charge: {
                    let held = &self.held[self.current];
                    reticle::charge_value(self.sniping, held.state.charge_level(&held.tuning), held.state.snipe_steady(&held.tuning))
                },
            },
        };
        if let Some(id) = reticle::lock_sound(self.previous_lock_sound, frame.lock) {
            sound.inspect(|s| s.play_event(id));
        }
        self.previous_lock_sound = frame.lock;
        self.sprites = self.reticle.draw(&frame, dt);
        rumble
    }

    /// The ground waves' sounds: the snaker object's SoundPlayers (`WaveEffects::sounds`: Blackout's,
    /// Jetfire's and OptimusFlight's `ANIM_BUMB_CHARGE_LP` loop and `WPN_BLAC_CAN_IMP`, both
    /// waitForTrigger 0, playOnce 1) start as the wave is made, follow its point, and stop with
    /// their fade when it is gone (keyOffOnDestruction 0: the loop fades over its 650 ms). [data;
    /// game 0076dcf0] [assumed] their source is the wave's point. 0076dcf0 (`sound_player_initialize`, read
    /// whole 2026-10-20) takes the source ONCE, at init: the world place of node `+0x14` (a 16-bit index, the
    /// player's node) of the object at `+0xc` (`object_node_world_place` 0056bee0, the object's own place for
    /// node 0), stored at `+0x118`. It does not follow the object after that, so a moving wave's sound
    /// position comes from elsewhere (not read). Direct callers of 0076dcf0: 0076f2f0 (VO sequence) and
    /// 0076f3e0 only; the snaker wave's player is not created by either, so which object and node it is given
    /// is unread. The players' nodes 1 and 3 are not carried. settles: the creation path of a snaker wave's
    /// SoundPlayer (its `+0xc` object and `+0x14` node, the vtable or spawn route that builds it), and the
    /// per-step update of `+0x118` (not read). A wave is started here, after the step that made it, so it
    /// sounds from its point one step on.
    fn wave_sounds(&mut self, sound: Option<&dyn WeaponSound>) {
        let Some(s) = sound else { return };
        let mut live = std::collections::HashSet::new();
        for (index, number, wave, _, _) in &self.snakers {
            live.insert(*number);
            let def = &weapon_in(&self.held, &self.special_weapons, &self.car, *index).def;
            if self.wave_sounding.insert(*number) {
                let row = [def.name, crate::character::alt_projectile_key(def.name)].into_iter()
                    .find_map(|key| self.round_fx.iter().find(|r| r.name == key).and_then(|r| r.wave.as_ref()));
                s.projectile_start_sounds(*number, row.map_or(&[][..], |w| &w.sounds[..]), wave.position);
            }
            let speed = weapon_in(&self.held, &self.special_weapons, &self.car, *index).tuning.snaker.speed;
            s.projectile_move(*number, wave.position, if wave.alive { speed } else { 0.0 });
        }
        for gone in self.wave_sounding.iter().filter(|n| !live.contains(n)).copied().collect::<Vec<_>>() {
            s.projectile_stop(gone);
            self.wave_sounding.remove(&gone);
        }
    }

    /// What an explosion's sphere reached takes its damage (no throw: the flags his
    /// missile's explosion carries are not a throwing kind, `Hit::throws`).
    fn explode(&mut self, hits: Vec<weapons::BlastHit>) {
        for hit in hits {
            self.struck.push(Struck { target: hit.target, packet: crate::damage::Packet {
                amount: hit.damage, flags: hit.flags, direction: hit.direction.extend(0.0),
                knockback_speed: hit.knock_back_speed, knockback_angle: hit.knock_back_angle,
                ..Default::default()
            }});
        }
    }

    /// The rounds' effects rows (`CharacterData::weapon_effects`): what a round's end and its
    /// ground wave take from its object (`round_fx`).
    pub fn set_effects(&mut self, rows: &[crate::character::WeaponEffects]) {
        self.round_fx = rows.to_vec();
    }

    /// A ground wave let go at `at` along `way` by the round `round` of the weapon `index`: its
    /// `checkGroundHeight`, its life (its object's TimeoutDeleter: min with `timeTilDies`) and its
    /// reach (its Collision sphere) are the wave object's [data]; `check` where the row has none.
    /// It is numbered as a projectile, its effects ride that number (`WeaponFxKind::Wave`), and its
    /// ControllerRumble starts at once (waitForTrigger 0). [game 007dba80, 00776f50]
    fn let_wave_go(&mut self, index: usize, round: u32, at: Vec3, way: Vec3, check: f32, world: &dyn Sight) {
        let row = self.round_fx.iter().find(|r| r.name == round).and_then(|r| r.wave.as_ref());
        let (check, life, reach, rumble) = match row {
            Some(w) => (w.check_ground_height, w.life, if w.radius > 0.0 { w.radius } else { SNAKER_REACH }, w.rumble),
            None => (check, f32::INFINITY, SNAKER_REACH, None),
        };
        let wave = weapons::Snaker::activate(at, way, check, world).with_life(life);
        if let Some(rumble) = rumble {
            self.rumbles.push((at, rumble));
        }
        self.launched += 1;
        self.fx.push(WeaponFx { kind: WeaponFxKind::Wave(self.launched), weapon: round, at, along: way });
        self.snakers.push((index, self.launched, wave, Vec::new(), reach));
    }

    /// [port design] Devastator's ring for Grindor's cinematic wave (`crate::energon_wave`),
    /// from `CharacterData::energon`: without it his cannon keeps the original's wave alone.
    pub fn set_energon(&mut self, ring: Option<crate::shockwave::Def>) {
        self.energon_ring = ring;
    }

    /// [port design] The cinematic wave of the cannon's snaker just let go (the last one), for a
    /// missile that ended at `impact`: its ring opens on the ground at `at` along `way`
    /// (`energon_wave::start`) and lives as long as the snaker would (`Snaker::life`, min with
    /// `timeTilDies`); its ground is sampled out to its first front.
    fn open_energon(&mut self, snaker: &weapons::SnakerTuning, impact: Vec3, at: Vec3, way: Vec3, world: &dyn Sight) {
        let (Some(base), Some((_, number, middle, _, _))) = (self.energon_ring.as_ref(), self.snakers.last()) else { return };
        let life = middle.life.min(snaker.time_til_dies);
        let mut ring = crate::energon_wave::wave(base, snaker, life);
        if !crate::energon_wave::fire(&mut ring, at, way, 0.0) {
            return;
        }
        ring.ground(0, at.z);
        let Some(mut ground) = crate::energon_wave::Ground::new(&ring) else { return };
        ground.advance(&ring, world, 0.0);
        let snaker = *number;
        self.launched += 1;
        self.energon.push(EnergonWave { number: self.launched, snaker, ring, ground, impact });
    }

    /// [port design] Grindor's cinematic waves alive (`energon::dress`, `shockwaves::run`).
    pub fn energon_waves(&self) -> &[EnergonWave] {
        &self.energon
    }

    /// [port design] The frame copy `slot` of the wave's emitter rides along the front of the
    /// cinematic wave with this number, while it lives (`energon_wave::front_frame`).
    pub fn energon_frame(&self, number: u32, slot: u32) -> Option<Affine3A> {
        self.energon.iter().find(|w| w.number == number).and_then(|w| crate::energon_wave::front_frame(&w.ring, &w.ground, slot))
    }

    /// The frame of the projectile with this number, game space, while it flies: a missile's +y
    /// along its direction, up world z or y above z 0.99 (`weapons::missile_look_along`, as `draw` turns its model
    /// [game 007847e0 l.250-261]); a grenade's own turn (`Grenade::orientation` [game 00786ff0]).
    pub fn projectile_frame(&self, number: u32) -> Option<Affine3A> {
        if let Some((_, _, m)) = self.missiles.iter().find(|(_, n, _)| *n == number) {
            return Some(Affine3A::from_rotation_translation(weapons::missile_look_along(m.direction.normalize_or(Vec3::Y)), m.position));
        }
        if let Some((_, _, g)) = self.grenades.iter().find(|(_, n, _)| *n == number) {
            return Some(Affine3A::from_rotation_translation(g.orientation, g.position));
        }
        // A thrown piece: its spin after its placed turn, its placed uniform scale. [game 005522f0, 00552930]
        self.debris.iter().find(|(n, ..)| *n == number)
            .map(|(_, _, _, placed, scale, p)| Affine3A::from_scale_rotation_translation(Vec3::splat(*scale), (p.turn * *placed).normalize(), p.pos))
    }

    /// The thrown pieces: (number, the round's row key, the piece's index in its row, its frame, its fade's alpha).
    pub fn debris_pieces(&self) -> impl Iterator<Item = (u32, u32, usize, Affine3A, f32)> + '_ {
        self.debris.iter().filter_map(move |(n, round, k, _, _, p)| Some((*n, *round, *k, self.projectile_frame(*n)?, p.alpha)))
    }

    /// The projectiles in flight: (number, the round's effects row key, the weapon's name, grenade).
    pub fn projectiles(&self) -> impl Iterator<Item = (u32, u32, u32, bool)> + '_ {
        let name = |index: usize| weapon_in(&self.held, &self.special_weapons, &self.car, index).def.name;
        let missiles = self.missiles.iter().map(move |(i, n, _)| {
            let weapon = name(*i);
            (*n, if self.alt_rounds.contains(n) { crate::character::alt_projectile_key(weapon) } else { weapon }, weapon, false)
        });
        missiles.chain(self.grenades.iter().map(move |(i, n, _)| (*n, name(*i), name(*i), true)))
    }

    /// The frame of the ground wave with this number while it lives: x = forward x normal,
    /// y = its forward along the surface, z = the surface's normal, at its point.
    pub fn wave_frame(&self, number: u32) -> Option<Affine3A> {
        let (_, _, wave, _, _) = self.snakers.iter().find(|(_, n, w, _, _)| *n == number && w.alive)?;
        let z = wave.normal.normalize_or(Vec3::Z);
        let y = (wave.forward - z * wave.forward.dot(z)).normalize_or(Vec3::Y);
        Some(Affine3A::from_mat3_translation(Mat3::from_cols(y.cross(z), y, z), wave.position))
    }

    /// Where the missile with this number is and the way it flies, while it flies (kept for old
    /// callers; the effects take `projectile_frame`).
    #[allow(dead_code)]
    pub fn missile(&self, number: u32) -> Option<(Vec3, Vec3)> {
        self.missiles.iter().find(|(_, n, _)| *n == number).map(|(_, _, missile)| (missile.position, missile.direction)).or_else(|| {
            self.grenades.iter().find(|(_, n, _)| *n == number).map(|(_, _, g)| (g.position, g.velocity.normalize_or(Vec3::Y)))
        })
    }

    /// The weapon in hand: its name, its heat (0 to its maximum, 100 for most) and the time left
    /// in its overheat (0 when it is not overheated); None with no weapon.
    pub fn in_hand_state(&self) -> Option<(u32, f32, f32)> {
        let held = self.held.get(self.current)?;
        Some((held.def.name, held.state.heat, held.state.overheated_left))
    }

    /// For the readout: which weapon, its heat, and what has been fired.
    pub fn describe(&self) -> String {
        let held = &self.held[self.current];
        let heat = held.state.heat_fraction(&held.tuning);
        let place = self.in_hand().position(|i| i == self.current).unwrap_or(0) + 1;
        let mut line = format!("{:?} ({} of {}), heat {:3.0}%", held.def.kind, place, self.in_hand().count(), heat * 100.0);
        if let Some(child) = held.child {
            line += &format!(", with {:#010x} in the other hand{}", self.held[child].def.name, if held.tuning.alternating_fire { " (taking turns)" } else { "" });
        }
        if held.state.overheated_left > 0.0 {
            line += &format!(" (overheated, locked {:.1} s)", held.state.overheated_left);
        }
        if held.tuning.burst_per_fire > 1 {
            line += &format!(", bursts of {} x {} rounds {:.2} s apart ({} of this one fired)",
                held.tuning.burst_per_fire, held.tuning.projectiles_per_fire, held.tuning.time_between_bursts, held.state.bursts_done);
        }
        if let Some(car) = self.car.get(self.car_current).filter(|_| self.in_car) {
            line = format!("car gun, heat {:3.0}%", car.state.heat_fraction(&car.tuning) * 100.0);
            if self.car_wait.left() > 0.0 {
                line += &format!(" (ready in {:.2} s)", self.car_wait.left());
            }
            if car.state.overheated_left > 0.0 {
                line += &format!(" (overheated, locked {:.1} s)", car.state.overheated_left);
            }
        }
        line += &format!("; {} rounds fired, {} hit", self.rounds, self.hits);
        // The gun and the axe, for captures: shown or not, and the weapon layer's clip
        // (its place in `WeaponPartialSet`), tick and share.
        line += if self.gun.shown { "; gun out" } else { "; gun away" };
        if let Some(arm) = self.gun.arm {
            line += &format!(", arm clip {} at {:.0} ({:.2})", arm.clip, arm.tick, arm.weight());
        }
        if self.axes.iter().any(|axe| axe.shown) {
            line += "; axe out";
        }
        if !self.missiles.is_empty() {
            line += &format!(", {} missiles flying", self.missiles.len());
        }
        if let Some(id) = self.lock.locked {
            line += &format!("; locked on {id}");
        } else if let Some(id) = self.lock.candidate {
            line += &format!("; locking on {id} ({:.2} s)", self.lock.held);
        }
        line
    }

    /// A reset: nothing in flight, nothing hot.
    pub fn reset(&mut self, sound: Option<&dyn WeaponSound>) {
        for held in self.held.iter_mut().chain(&mut self.car) {
            held.state = Weapon::default();
            held.sounds.silence(sound);
        }
        for held in &mut self.special_weapons {
            held.sounds.silence(sound);
        }
        (self.car_wait, self.in_car) = (VehicleWait::default(), false);
        self.missiles.clear();
        self.missile_scale.clear();
        self.alt_rounds.clear();
        self.grenades.clear();
        self.snakers.clear();
        self.energon.clear();
        self.wave_sounds(sound);
        self.rumbles.clear();
        self.blasts.clear();
        (self.lock, self.seen) = (LockOn::default(), [None; 3]);
        self.previous_lock_sound = 0.0;
        (self.reticle, self.bracket, self.lock_shown, self.since_hit, self.icon_clock) = (Reticle::default(), None, 0.0, f32::MAX, 0.0);
        self.sprites.clear();
        self.tracers.clear();
        self.impacts.clear();
        (self.rounds, self.hits) = (0, 0);
        self.gun = Gun::default();
        // [game 007a7150] as at the start (`load`).
        self.gun.appear(&self.held[self.current].def);
        self.axes.iter_mut().for_each(|axe| *axe = Axe::default());
        self.special_clip = None;
        self.special_objects.iter_mut().for_each(|object| *object = Axe::default());
        // A reset takes the deployed turrets away (their owner is back). [assumed] settles: the game's reset
        // path with a deployed turret (0078cd50's owner test covers a death; the reset is unread).
        (self.turrets, self.gear) = (Vec::new(), Vec::new());
    }
}

/// His weapons: the lists his pack names, his special's from his attributes, and what each
/// round's end shows and starts. Devastator's are the boss mission's pods.
pub fn arm(game_dir: &Path, attributes: &str, data: &crate::character::CharacterData, giant: Option<&crate::giant::Giant>) -> Option<Arsenal> {
    // Devastator's pack lists no weapons: his are the boss mission's pods (`Arsenal::load_giant`).
    if let Some(giant) = giant.filter(|g| !g.pods.is_empty()) {
        return Arsenal::load_giant(game_dir, &giant.pods);
    }
    Arsenal::load(game_dir, &data.weapons).map(|mut arsenal| {
        // His special's weapons, from his attributes (`characterAttributes<X>`): none for
        // Bumblebee, whose weapon name is 0. [data]
        if let Err(e) = arsenal.equip_special(game_dir, attributes, &data.weapons, &data.weapon_effects, &data.turret_parts) {
            eprintln!("special: no weapon ({e})");
        }
        // What each round's end shows and starts (its blast rumble, its ground wave). [data]
        arsenal.set_effects(&data.weapon_effects);
        // Grindor's cinematic wave's ring (`energon_wave`) [port design].
        arsenal.set_energon(data.energon.as_ref().map(|d| d.ring.clone()));
        arsenal
    })
}
