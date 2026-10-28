//! The game's particle effects, run by `tf2_core::particles` and drawn as the original
//! draws them: the quads of its builders (`particles::Sprite`) with its own particle
//! shaders (`fx.wgsl`).
//!
//! Who starts an effect, and how sure that is:
//! - a melee hit: the impact preset's effect for the surface struck (`FUN_0077cd60`) [game]
//! - a clip's trigger message: the effects on his object that go by that name (the ground
//!   punches' `EffectSpawner` and `TriggerableParticle`, the special's) [game]
//! - a clip's `AnimSpawnEffectEvent` / `AnimStopEffectEvent` (landings, the change of
//!   form, the dodge, the charge, dying) [game]
//! - a clip's `FootStepEvent`: the named attachment probes the ground and selects
//!   its material preset [game/data; synchronous final-pose timing assumed]
//! - a weapon's muzzle flash, its round's impact effect by the surface and a missile's
//!   explosion [data]. Muzzle particles follow the weapon node with rotation and no
//!   scale [game: 007ae010/007aac80]. Impact/explosion placement remains assumed.
//! - the messages a state keeps on (`tf2_core::fxstate`): the exhaust, the headlights, the
//!   turbo's fire, the damage smoke, the health coming back [game: who sends each;
//!   `docs/particles.md` "The senders"]
//!
//! An `EffectSpawner`'s own object (the punches' shock ring, the special's dome) is its
//! mesh from his pack, moved by its own clip and removed by its `TimeoutDeleter`, with the
//! original's `dull_fx_emissive` shader (`glow.wgsl`).
//!
//! Elements of the distortion renderer (`Dstr`: the heat haze of the turbo, a muzzle
//! flash and the missile's explosion) are drawn by `warp.wgsl`. The tyres' dust and skid
//! smoke are started as the vehicle's update starts them [game]; their marks are
//! `skidmarks.rs`. A weapon object's own clip events (the overheat smoke) and the
//! climbing steps' dust are started here too. CollisionFX uses the original contact
//! history and material presets with assumed arena contact types. Car-gun trails use
//! BulletTrace's visual movement. Regeneration, stun and frost follow the damage
//! receiver's clocks (`docs/damage.md`).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use tf2_core::formats::anim::{Clip, EffectEventKind, TICKS_PER_SECOND};
use tf2_core::formats::hash::crc32;
use tf2_core::particles::{Effect, Element, Rng, Sprite, Template};
use tf2_core::pose::stored_turn;
use tf2_core::fxstate;
use tf2_core::sim::Mode;
use tf2_core::sound::tyre_sounds;

use crate::assets::{Builder, Cache, ObjectNodes, bevy_to_game, game_to_bevy, image, model_space_rotation};
use crate::fx::{Fx, Glow, Warp};
use crate::player::{Player, RobotModel, VehicleModel};
use crate::world::ArenaRes;

/// `ATTACH_TO_HIT_OBJECT`, bit 8 of an impact's `effectOnImpactFlags`: the effect is put
/// in the frame of the object that was hit (`FUN_0077cd60`). [game]
const ATTACH_TO_HIT_OBJECT: u32 = 1 << 8;
/// Bit 0 of the same: the effect's z lies along the surface's normal (`FUN_0077c180`).
const Z_ALONG_NORMAL: u32 = 1;



/// The wheels' nodes, in the order of the vehicle update's loop (its fallback names are
/// `Tread_Front_L`, `Tread_Front_R`, `Tread_Back_L`, `Tread_Back_R`). [game: the order]
const TYRES: [u32; 4] = [crc32(b"Tire_Front_L"), crc32(b"Tire_Front_R"), crc32(b"Tire_Rear_L"), crc32(b"Tire_Rear_R")];
/// Those fallback names, wheel by wheel. [game 007347c0]
const TREADS: [u32; 4] = [crc32(b"Tread_Front_L"), crc32(b"Tread_Front_R"), crc32(b"Tread_Back_L"), crc32(b"Tread_Back_R")];

/// What a running effect's place follows.
enum Anchor {
    Fixed,
    /// A target, by its id, and where on it. [stand-in: the offset is kept in the world, so a scout's turn (targets.rs sets its rotation from its yaw) does not turn the effect; the game's ATTACH_TO_HIT_OBJECT attach and whether it takes the object's turn were not read] settles: the game's attach routine for the melee impact flag (the callers of its effect start with that flag), read for whether the turn is followed.
    Target { id: u32, offset: Vec3 },
    /// A node of his, of the robot or of the car, by its number: its place, and its turn
    /// too when the effect takes it.
    Node { car: bool, node: usize, turn: bool },
    /// A node of a projectile in flight (`Arsenal::projectile_frame`) or of a ground wave
    /// (`Arsenal::wave_frame`), by its number, moved in `start`: the place is the frame times the
    /// node's bind, with the node's axes normalised when `rot` (flag 8, 0057d760) and their lengths
    /// kept when `scale`; the translation alone without `rot`. When the projectile goes the emitter
    /// stops and its particles live out, or with `kill` (`instantKill`, flag 0x80) they go too.
    /// [game 0057d0d0, 0057e000]
    Projectile { number: u32, wave: bool, bind: Affine3A, rot: bool, scale: bool, kill: bool },
    Bullet(tf2_core::weapons::BulletVisual),
    Collision {key:u32,offset:Vec3},
    /// A node of another object of his (a weapon's), by its entity.
    Thing { entity: Entity, turn: bool },
    /// Fixed in a node's frame: the place is the node's frame (with its turn) times `offset`, taken
    /// when the effect started (0077d5d0 bit 8: the contact point put into the car's frame with
    /// the inverse of its own matrix, then attached through 0057e820). [game]
    Offset { car: bool, node: usize, offset: Affine3A },
    /// [port design] A place along the front of Grindor's cinematic wave (`Arsenal::energon_frame`),
    /// by the wave's number and the slot, times the emitter's bind (with its scale); when the wave
    /// ends the emitter stops and its particles live out.
    Energon { number: u32, slot: u32, bind: Affine3A },
}

struct Running {
    template: Template,
    effect: Effect,
    anchor: Anchor,
    /// The emitter's frame, in the game's space.
    place: Affine3A,
    /// The `eventId` of the clip event that started it, 0 for none: a stop event's handle.
    id: u32,
    /// The trigger's name, for an effect that is kept going by a state (the exhaust).
    script: u32,
    /// Ends with its clip (autoStop, or a weapon event paired with an explicit stop).
    with_clip: bool,
    /// The object whose clip owns the event id; None is the character's object.
    owner: Option<Entity>,
    /// A tyre's effect, kept going by the vehicle's update: the wheel, whether it is the
    /// skid's (else the rolling one), and the surface it was started for.
    tyre: Option<(usize, bool, u32)>,
    collision: Option<(usize,u32)>,
    /// Started on the car's object by a message (else the robot's).
    on_car: bool,
    /// Removed at once, particles and all.
    gone: bool,
    /// The quads of each element, once it has shown something.
    parts: Vec<Option<(Entity, Handle<Mesh>)>>,
}

/// An `EffectSpawner`'s object while it lives: the punches' shock ring, a mesh its own
/// clip scales up, removed by its `TimeoutDeleter`.
struct Shell {
    root: Entity,
    /// Whose object the effect is on, and which of its effects it is.
    car: bool,
    def: usize,
    age: f32,
    /// Its parts were told not to cast shadows.
    settled: bool,
}

/// A clip event waiting for the next `start`.
enum Pending {
    Spawn { node: u32, id: u32, template: Template, track: bool, turn: bool, with_clip: bool },
    /// The clip was left: the effects it started with `autoStop` end.
    ClipLeft,
    ThingClipLeft { owner: Entity },
    Stop { id: u32 },
    StopOn { owner: Entity, id: u32 },
    Step { side: u32 },
    /// A hand or a foot on the wall, or a slide down it (`anim::CLIMB_SIDES`).
    ClimbStep { limb: usize },
    StepContact { limb: Option<usize>, hit: tf2_core::vehicle::GroundHit },
    /// A spawn event of a clip of another object of his (a weapon's own clip), on that
    /// object's node.
    SpawnOn { owner: Entity, entity: Entity, id: u32, template: Template, track: bool, turn: bool, with_clip: bool },
}

/// What a weapon asks for as it fires (`tf2_core::arsenal`).
pub use tf2_core::arsenal::WeaponFxKind;

/// The handles of what a charged shot and a flamethrower keep going, for `Effects::stop_event`.
/// [data] no `.str` pack under the install (145 packs, levels and shared included, 2026-10-20) holds
/// these four values as a 4-byte id, little or big endian: no game event carries them, so they are
/// private to this file.
const CHARGE: u32 = 0xc4a2_0001;
const CHARGE_READY: u32 = 0xc4a2_0002;
const FLAME_STREAM: u32 = 0xf1a3_0000;
const FLAME_BALL: u32 = 0xf1a3_0010;
const FLAME_IMPACT: u32 = 0xf1a3_0100;

/// What a riding light follows.
enum LightAnchor {
    Projectile(u32),
    Wave(u32),
    Bullet(tf2_core::weapons::BulletVisual),
}

/// A light on a projectile's object: its Light in the object's space, where it is now, and the
/// object's TimeoutDeleter where it has one (the wave's: it fades the light and ends it). [data]
struct Riding {
    anchor: LightAnchor,
    local: tf2_core::point_light::LevelLight,
    light: tf2_core::point_light::LevelLight,
    deleter: Option<tf2_core::point_light::LiveLight>,
    gone: bool,
}

impl Riding {
    fn new(anchor: LightAnchor, local: &tf2_core::point_light::LevelLight, frame: Affine3A, deleter: Option<&tf2_core::point_light::FlashLight>) -> Self {
        let mut riding = Riding { anchor, local: local.clone(), light: local.clone(), deleter: deleter.map(|d| d.spawn(Affine3A::IDENTITY)), gone: false };
        riding.place(frame);
        riding
    }

    fn place(&mut self, frame: Affine3A) {
        self.light.position = frame.transform_point3(self.local.position);
        self.light.direction = frame.transform_vector3(self.local.direction);
    }
}

/// A `destroyEffectSpawn` particle: it starts `start_in` seconds from now (its TRIGGER's delay) and
/// stops `stop_in` from now (its UNTRIGGER's), under its own id. [data] [game 0080daf0]
struct Timed {
    template: Template,
    place: Affine3A,
    start_in: f32,
    stop_in: f32,
    id: u32,
    started: bool,
}

/// The ids of the lingering smoke (`Timed`): the port's own keys, not the game's (the game keeps a
/// spawned object per `destroyEffectSpawn`, with no id) [port]. They share the emitter map's u32 space
/// with the data's eventIds, and the range 0x7d000000..0x7dffffff holds one real eventId (0x7d828ee6,
/// optimus.str): a clash needs 0x828ee6 (8.5 million) lingering smokes in one run first.
const TIMED_ID: u32 = 0x7d00_0000;

/// The character's weapons' muzzle lights, by weapon name (`point_light::load_flash_lights` of his
/// pack). [data] Inserted by `main` with the character data; without it no weapon makes a light.
#[derive(Resource, Default)]
pub struct FlashLights(pub Vec<(u32, tf2_core::point_light::FlashLight)>);

#[derive(Resource)]
pub struct Effects {
    /// The muzzle lights and the blasts' flashes alive, each counting down its TimeoutDeleter.
    /// [game 007aac80, 00776f50]
    flashes: Vec<tf2_core::point_light::LiveLight>,
    /// The lights that ride a projectile, a bullet or a ground wave: components of its object, so
    /// they go with it. [data] [game: the Light is a component of the projectile object]
    riding: Vec<Riding>,
    /// The lingering smoke of the projectiles that ended (`TimedParticle`), waiting to start or stop.
    timed: Vec<Timed>,
    timed_count: u32,
    running: Vec<Running>,
    shells: Vec<Shell>,
    pending: Vec<Pending>,
    /// Footstep and climb-step contacts probed and waiting their one game update (time left, the
    /// limb for a climb, the hit): the dust lands as the sound does (`sound.rs` STEP_DISPATCH_DELAY).
    delayed_steps: Vec<(f32, Option<usize>, tf2_core::vehicle::GroundHit)>,
    cache: Cache,
    /// One generator for every effect, as in the original: the game's `DAT_00d44940` (the LCG step of
    /// `FUN_0095bff0` and the particle domains' readers, 00a9bb40 / 00a9bfa0 / 00a9c3f0 / 00a9c580 /
    /// 00a9c710 / 00acad40). [game 0095bff0] [data: the executable's word at 00d44940 is 0
    /// (`python tools\pe_read.py words`), and no code stores it except that step (`python work\find_imm.py
    /// 00d44940`: every reference is in the readers above), so the first draw is the step from 0. Other
    /// `random_range` users of the game are not checked against this generator here.
    rng: Rng,
    materials: HashMap<(u32, u32, bool, [u8; 7], [u32; 2]), Handle<Fx>>,
    warps: HashMap<u32, Handle<Warp>>,
    /// The level's switch for headlights (`FUN_00815790`). [stand-in: the arena has no
    /// level settings; on, so that they can be seen]
    pub headlights: bool,
    /// The triggers the vehicle model's own clip sent this frame (`animation::animate`): the
    /// jets' take-off clip sends their exhaust. [data]
    pub vehicle_triggers: Vec<u32>,
    /// FlyMode's exhaust latch (`fxstate::kept`'s `fly_exhaust`) and whether the fly brake ran
    /// last frame (its end sends the exhaust again, 007325e0). [game]
    fly_exhaust: bool,
    fly_braking: bool,
    collision_history: tf2_core::collisionfx::History,
    collision_pending: std::collections::BTreeMap<u32,tf2_core::vehicle::Contact>,
    collision_clock: f32,
    /// Capture diagnostics: visual objects created and nonempty bullet sprites submitted.
    pub bullet_visuals_started: u64,
    pub bullet_sprites_submitted: u64,
    pub collision_visuals_started: u64,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            flashes: Vec::new(),
            riding: Vec::new(),
            timed: Vec::new(),
            timed_count: 0,
            running: Vec::new(),
            shells: Vec::new(),
            pending: Vec::new(),
            delayed_steps: Vec::new(),
            cache: Cache::default(),
            rng: Rng(0),
            materials: HashMap::new(),
            warps: HashMap::new(),
            headlights: true,
            vehicle_triggers: Vec::new(),
            fly_exhaust: false,
            fly_braking: false,
            collision_history: Default::default(),collision_pending:Default::default(),collision_clock:0.0,
            bullet_visuals_started:0,bullet_sprites_submitted:0,collision_visuals_started:0,
        }
    }
}

impl Effects {
    /// Everything back to the start for another character (`charmenu.rs`): his effects end at
    /// once, and his cached meshes and materials (his library's) are dropped. Returns the
    /// entities to despawn: the quads drawn and the spawners' objects.
    pub fn reset(&mut self) -> Vec<Entity> {
        let mut gone: Vec<Entity> = self.shells.iter().map(|s| s.root).collect();
        gone.extend(self.running.iter().flat_map(|r| r.parts.iter().flatten().map(|(e, _)| *e)));
        let headlights = self.headlights;
        *self = Self { headlights, ..Self::default() };
        gone
    }

    /// Capture diagnostics for the installed gun's smoke, including particles draining
    /// after a stop. Count simulation state so hidden meshes cannot mask a stuck source.
    pub fn gun_smoke_stats(&self) -> (usize, usize) {
        let mut emitters = 0;
        let mut particles = 0;
        for r in self.running.iter().filter(|r| r.template.name == crc32(b"w_overheat_smoke_small")) {
            emitters += usize::from(!r.effect.stopping());
            particles += (0..r.template.elements.len()).filter_map(|i| r.effect.group(i)).map(|g| g.particles.len()).sum::<usize>();
        }
        (emitters, particles)
    }

    pub fn exhaust_stats(&self) -> (usize, usize) {
        let exhaust = self.running.iter().filter(|r| r.script == fxstate::EXHAUST);
        exhaust.fold((0, 0), |(emitters, particles), r| (
            emitters + usize::from(!r.effect.stopping()),
            particles + (0..r.template.elements.len()).filter_map(|i| r.effect.group(i)).map(|g| g.particles.len()).sum::<usize>()))
    }

    /// Probes from the final animated pose. [game 005a8740, 00740d60, 006dedf0] The probe resolves in
    /// the world step (006dedf0, priority 14), which runs after the IK ring (00740d60, priority 12) has
    /// dispatched the completed FootStep: so the contact is held one game update (`delayed_steps`) and
    /// starts in `start`, as the footstep sound does.
    pub fn steps(&mut self, pose: &[Affine3A], data: &tf2_core::character::CharacterData, forward: Vec3, world: &impl tf2_core::sim::World) {
        let pending = std::mem::take(&mut self.pending);
        for cue in pending {
            let (limb, hits) = match cue {
                Pending::Step { side } => (None, tf2_core::stepfx::footsteps(side, pose, &data.robot.footsteps, world)),
                Pending::ClimbStep { limb } => (Some(limb), tf2_core::stepfx::climbing(limb, pose, &data.robot.climb_steps, forward, world)),
                other => { self.pending.push(other); continue }
            };
            self.delayed_steps.extend(hits.into_iter().map(|hit| (tf2_core::sim::GAME_UPDATE, limb, hit)));
        }
    }

    fn start(&mut self, template: &Template, anchor: Anchor, place: Affine3A, id: u32, script: u32) {
        if template.elements.is_empty() {
            return;
        }
        if matches!(&anchor,Anchor::Bullet(_)) {self.bullet_visuals_started+=1;}
        self.running.push(Running { template: template.clone(), effect: Effect::new(template), anchor, place, id, script, with_clip: false, owner: None, tyre: None, collision:None, on_car: false, gone: false, parts: vec![None; template.elements.len()] });
    }

    fn stop_event(&mut self, id: u32, owner: Option<Entity>) {
        self.running.iter_mut().filter(|r| r.id == id && r.owner == owner).for_each(|r| r.effect.stop());
    }

    /// Devastator's dressing (`giant::dress`) [assumed port design]: a template started at `place`,
    /// following `on`'s place (not its turn) when given, under `id` for `stop_giant` (0: left to end).
    pub fn start_giant(&mut self, template: &Template, on: Option<Entity>, place: Affine3A, id: u32) {
        let anchor = on.map_or(Anchor::Fixed, |entity| Anchor::Thing { entity, turn: false });
        self.start(template, anchor, place, id, 0);
    }

    /// [port design] A piece of Grindor's cinematic wave (`energon::dress`): a template started at
    /// `place` (its scale makes the effect larger, with `Template::scaled`), left to end.
    pub fn start_energon(&mut self, template: &Template, place: Affine3A) {
        self.start(template, Anchor::Fixed, place, 0, 0);
    }

    /// [port design] A copy of the wave object's emitter riding the front of the cinematic wave
    /// `number` at `slot`, from `frame` (its frame now), on its node's bind.
    pub fn start_energon_front(&mut self, emitter: &tf2_core::formats::scene::PlacedEmitterDef, number: u32, slot: u32, frame: Affine3A) {
        let bind = rows_affine(&emitter.bind);
        self.start(&emitter.template, Anchor::Energon { number, slot, bind }, frame * bind, 0, 0);
    }

    /// [port design] A flash light of the wave's (its impact's), at `place`, removed by its deleter.
    pub fn flash(&mut self, light: &tf2_core::point_light::FlashLight, place: Affine3A) {
        self.flashes.push(light.spawn(place));
    }

    /// The dressing's emitters under `id` stop; what they made lives out.
    pub fn stop_giant(&mut self, id: u32) {
        self.stop_event(id, None);
    }

    /// Whether a dressing effect under `id` is still running.
    pub fn giant_running(&self, id: u32) -> bool {
        self.running.iter().any(|r| r.id == id && r.owner.is_none() && !r.effect.stopping())
    }

    /// The weapon object was hidden (put away, scoped, or not in hand). The game's `weapon_hide`
    /// (007a7750) broadcasts 005790a0(1) on the object's listener ring; each emitter's listener
    /// (`particle_emitter_message_trigger`, 0057f600, registered by 0057d370) answers a nonzero
    /// argument with `particle_emitter_stop` (0057e000): no more particles, the ones made live out.
    /// [game 007a7750, 005790a0, 0057f600, 0057d370] Its emitters are the ones this object owns.
    pub fn object_hidden(&mut self, nodes: &ObjectNodes) {
        let Some(&owner) = nodes.entities.first() else { return };
        for r in self.running.iter_mut().filter(|r| r.owner == Some(owner)) {
            r.effect.stop();
        }
    }

    /// The weapon object was shown (`weapon_show` 007a7630 -> 005790a0(0)). The same ring message
    /// with a zero argument goes to `particle_emitter_message_trigger` (0057f600), which triggers
    /// an emitter whose flags2 bit 2 is set: that is `autoStart` (flag 2, as 0057d0d0 sets it), so
    /// each placed emitter with `auto_start` starts again on its node (0057dd30 creates a new system).
    /// [game 007a7630, 005790a0, 0057f600, 0057dd30; flag 2 per 0057d0d0] Only a placed emitter of
    /// the object is triggered here; clip effects are restarted by their clip, not by show.
    fn stop_clip_effects(&mut self, owner: Option<Entity>) {
        for r in self.running.iter_mut().filter(|r| r.with_clip && r.owner == owner) {
            r.with_clip = false;
            r.effect.stop();
        }
    }

    fn start_on(&mut self, template: &Template, anchor: Anchor, place: Affine3A, owner: Entity, id: u32, with_clip: bool) {
        if id != 0 {
            self.stop_event(id, Some(owner));
        }
        if template.elements.is_empty() { return }
        self.start(template, anchor, place, id, 0);
        let started = self.running.last_mut().unwrap();
        started.owner = Some(owner);
        started.with_clip = with_clip;
    }

    /// His clip changed: the effects the old one started with `autoStop` end. The clip's
    /// reference lists their ids for this (`FUN_0070ccd0`, `+0x50`). [game: the list]
    /// [game 00748f00 lines 13-58] The character's clip player walks the OUTGOING reference's
    /// list (it is the reference whose `xfadeTimeOnExit` `+0x48` and `continueTimeOnExit` the same
    /// routine reads) as the new clip is started, before the fade, stopping each id through
    /// 0074ae10; the same walk stops its autoStop and keyOff sounds (005987e0).
    pub fn clip_changed(&mut self) {
        self.pending.push(Pending::ClipLeft);
    }

    /// The object bundle walks its autoStop list on replacement. [game: 007c3870]
    /// The game has NO paired stop on a clip change: 007c2ea0 lists only autoStop spawns, and
    /// 007c3870 stops only those, through 0074ae10. The smoke of an interrupted overheat clip keeps
    /// emitting until the weapon object is hidden: `object_hidden` (game weapon_hide 007a7750).
    /// The paired stop the port had before (2026-10-20 third pass) is removed for that reason.
    pub fn thing_clip_changed(&mut self, nodes: &ObjectNodes) {
        if let Some(&owner) = nodes.entities.first() {
            self.pending.push(Pending::ThingClipLeft { owner });
        }
    }

    /// The same for a clip of another object of his (a weapon's own clip: the overheat
    /// smoke is a spawn event of the gun's overheat clip), on that object's nodes.
    /// [game 007c4260, 007c2ea0] every object's clip bundle is built by the one factory
    /// (anim_ref_bundle_factory, pool 0xf, loader object_clip_bundle_load), so a weapon's clips
    /// carry their effect events the same way. [data bumblebee.txt:541661] the overheat smoke
    /// (`w_overheat_smoke_small`, eventId 898679215) is an AnimStream ANIM_TYPE_EVENT spawn in
    /// that clip, the same stream type as his own. Not traced: the per-object playback call site.
    pub fn thing_clip_events(&mut self, clip: &Clip, from: f32, to: f32, nodes: &ObjectNodes) {
        let Some(&owner) = nodes.entities.first() else { return };
        for event in &clip.effect_events {
            let t = event.time;
            let due = if to >= from { (t > from && t <= to) || (from == 0.0 && t == 0.0 && to > 0.0) } else { t > from || t <= to };
            if !due {
                continue;
            }
            match &event.kind {
                EffectEventKind::Spawn { id, template, track, use_rotation, auto_stop, .. } => {
                    let Some(entity) = nodes.name_hashes.iter().position(|&name| name == event.node).map(|node| nodes.entities[node]) else { continue };
                    // [game 007c2ea0] only an autoStop spawn is ended by a clip change (see thing_clip_changed).
                    self.pending.push(Pending::SpawnOn { owner, entity, id: *id, template: template.clone(), track: *track || *id != 0, turn: *use_rotation, with_clip: *auto_stop && *id != 0 });
                }
                EffectEventKind::Stop { id } => self.pending.push(Pending::StopOn { owner, id: *id }),
                EffectEventKind::FootStep { .. } | EffectEventKind::ClimbStep { .. } => {}
            }
        }
    }

    /// The effect events of his clip between two positions in ticks, kept for the next
    /// `start`. `to` below `from` means the clip went round; a clip that has just started
    /// has `from` at 0 and its events at tick 0 count.
    /// `rerun`: the clip had gone round before this stretch; an event that does not allow
    /// a rerun then stays quiet (`FUN_0053ab90`). [game]
    pub fn clip_events(&mut self, clip: &Clip, from: f32, to: f32, rerun: bool) {
        for event in &clip.effect_events {
            let t = event.time;
            let (due, again) = if to >= from {
                ((t > from && t <= to) || (from == 0.0 && t == 0.0 && to > 0.0), rerun)
            } else if t > from {
                (true, rerun)
            } else {
                (t <= to, true)
            };
            if !due || (again && !event.rerun) {
                continue;
            }
            self.pending.push(match &event.kind {
                EffectEventKind::Spawn { id, template, track, use_rotation, auto_stop, .. } => Pending::Spawn {
                    node: event.node,
                    id: *id,
                    template: template.clone(),
                    track: *track || *id != 0,
                    turn: *use_rotation,
                    with_clip: *auto_stop && *id != 0,
                },
                EffectEventKind::Stop { id } => Pending::Stop { id: *id },
                EffectEventKind::FootStep { side } => Pending::Step { side: *side },
                EffectEventKind::ClimbStep { limb } => Pending::ClimbStep { limb: *limb },
            });
        }
    }
}

/// Direct scene ParticleEmitters start as their factory does (0057d0d0).
pub fn start_level(level: Option<Res<crate::level::LevelModels>>, mut effects: ResMut<Effects>) {
    let Some(level) = level else { return };
    let mut started = 0;
    for emitter in &level.0.emitters {
        // A damage stage's emitters start when the stage is built (`start_stage_emitters`).
        if !emitter.auto_start || emitter.stage > 0 { continue; }
        // [game 0057d0d0] Gate is deterministic from the Object translation, not rand().
        let phase = emitter.object_position.y + emitter.object_position.z;
        if emitter.spawn_chance < 1.0 && emitter.spawn_chance < phase - phase.floor() { continue; }
        let place = emitter_place(&emitter.world, emitter.use_rotation, emitter.use_scale);
        effects.start(&emitter.template, Anchor::Fixed, place, 0, 0);
        started += 1;
    }
    info!("level particles: {started} direct auto-start emitters started from {} retained scene emitters", level.0.emitters.len());
}

/// A placed emitter's frame from its Object's world matrix and its useRotation / useScale flags.
fn emitter_place(w: &[f32; 12], use_rotation: bool, use_scale: bool) -> Affine3A {
    let mut x = Vec3::new(w[0], w[1], w[2]);
    let mut y = Vec3::new(w[3], w[4], w[5]);
    let mut z = Vec3::new(w[6], w[7], w[8]);
    if !use_rotation {
        // [game 0057d760] Scale-only retains the lengths on an identity basis.
        (x, y, z) = if use_scale { (Vec3::X*x.length(), Vec3::Y*y.length(), Vec3::Z*z.length()) }
            else { (Vec3::X, Vec3::Y, Vec3::Z) };
    } else if !use_scale {
        let normalize = |v: Vec3| { let length = v.length(); if length > 1e-8 { v/length } else { v } };
        x = normalize(x); y = normalize(y); z = normalize(z);
    }
    Affine3A::from_mat3_translation(Mat3::from_cols(x,y,z), Vec3::new(w[9],w[10],w[11]))
}

/// A node's frame in the game's space, from where it was drawn last: its place alone, or
/// with its turn (no scale: no effect of his asks for it).
/// [stand-in: the frame is the one drawn last frame] OPEN (read 2026-10-20): the game takes a
/// node's matrix on demand (005509d0 -> 00578360 recomputes a dirty node from its local entry
/// and its parents), so its effects read the pose of the current update. settles: the car's and
/// the robot's per-tick node pose in the port (the same debt as skidmarks.rs).
fn node_frame(nodes: &ObjectNodes, node: usize, turn: bool, placed: &Query<&GlobalTransform>) -> Option<Affine3A> {
    entity_frame(*nodes.entities.get(node)?, turn, placed)
}

/// A robot or vehicle node's frame as this frame left it. `run` comes after `animation::animate`
/// and `player::present` but before `PostUpdate` propagates their transforms, so the node's
/// `GlobalTransform` there is the frame before's: an effect riding a node lagged one frame
/// behind it (Blackout's rotor blur, 0.36 m behind its hub when strafing). This walks the
/// node's own transforms up to the top instead. [port]
fn node_frame_now(nodes: &ObjectNodes, node: usize, turn: bool, transforms: &Query<&mut Transform, Without<Camera3d>>, parents: &Query<&ChildOf>) -> Option<Affine3A> {
    let mut entity = *nodes.entities.get(node)?;
    let mut at = transforms.get(entity).ok()?.compute_affine();
    while let Ok(parent) = parents.get(entity) {
        entity = parent.parent();
        at = transforms.get(entity).ok()?.compute_affine() * at;
    }
    Some(frame_of(at.to_scale_rotation_translation(), turn))
}

fn frame_of((_, rotation, translation): (Vec3, Quat, Vec3), turn: bool) -> Affine3A {
    let place = bevy_to_game(translation);
    if turn {
        Affine3A::from_rotation_translation(model_space_rotation().inverse() * rotation, place)
    } else {
        Affine3A::from_translation(place)
    }
}

fn entity_frame(entity: Entity, turn: bool, placed: &Query<&GlobalTransform>) -> Option<Affine3A> {
    let at = placed.get(entity).ok()?;
    let (_, rotation, translation) = at.to_scale_rotation_translation();
    let place = bevy_to_game(translation);
    Some(if turn {
        Affine3A::from_rotation_translation(model_space_rotation().inverse() * rotation, place)
    } else {
        Affine3A::from_translation(place)
    })
}

/// [game: 007ae010] The named flash node wins, else the successful firing barrel.
fn muzzle_entity(nodes: &ObjectNodes, barrel: usize) -> Option<Entity> {
    let flash = nodes.name_hashes.iter().position(|&h| h == crc32(b"MuzzleFlashDummy"));
    let fire = if barrel == 0 { crc32(b"FireDummy") } else { crc32(format!("FireDummy{barrel:02}").as_bytes()) };
    let index = flash.or_else(|| nodes.name_hashes.iter().position(|&h| h == fire))?;
    nodes.entities.get(index).copied()
}

/// What of a renderer's lighting tells two materials apart (`Fx::new`).
fn lit_key(renderer: &tf2_core::particles::Renderer) -> [u8; 7] {
    if !renderer.lit() {
        return [0; 7];
    }
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
    let (a, d) = (renderer.ambient, renderer.diffuse);
    [byte(a[0]), byte(a[1]), byte(a[2]), byte(d[0]), byte(d[1]), byte(d[2]), byte(renderer.back_lit)]
}

/// A frame at `at` whose x lies along `along` and whose z is as near up as that leaves.
fn frame_along_x(at: Vec3, along: Vec3) -> Affine3A {
    let x = along.try_normalize().unwrap_or(Vec3::X);
    let y = Vec3::Z.cross(x).try_normalize().unwrap_or(Vec3::Y);
    Affine3A::from_mat3_translation(Mat3::from_cols(x, y, x.cross(y)), at)
}

/// A bind's rows (the object's space) as an affine: the game's rows are glam's columns.
fn rows_affine(m: &[[f32; 3]; 4]) -> Affine3A {
    Affine3A::from_cols_array(&[m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2], m[3][0], m[3][1], m[3][2]])
}

/// An emitter's frame on a node of a moving object: `frame * bind`, then its useRot / useScale as
/// a placed emitter's (`emitter_place`, 0057d760). [game]
fn node_place(frame: Affine3A, bind: Affine3A, rot: bool, scale: bool) -> Affine3A {
    let m = frame * bind;
    let (x, y, z, t) = (m.matrix3.x_axis, m.matrix3.y_axis, m.matrix3.z_axis, m.translation);
    emitter_place(&[x.x, x.y, x.z, y.x, y.y, y.z, z.x, z.y, z.z, t.x, t.y, t.z], rot, scale)
}

/// Starts the effects the last step of the movement, his clips and his weapons asked for.
#[allow(clippy::too_many_arguments)]
pub fn start(
    timing: (Res<Time>,Res<crate::player::TuningRes>,Option<Res<FlashLights>>),
    data: Res<crate::GameData>,
    arena: Res<ArenaRes>,
    player: Single<&Player>,
    robot: Single<&ObjectNodes, (With<RobotModel>, Without<VehicleModel>)>,
    car: Option<Single<&ObjectNodes, (With<VehicleModel>, Without<RobotModel>)>>,
    (placed, weapon_models): (Query<&GlobalTransform>, Query<(&crate::weapons::WeaponModel, &ObjectNodes)>),
    arsenal: Option<ResMut<crate::weapons::Arsenal>>,
    mut effects: ResMut<Effects>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surfaces: ResMut<Assets<crate::surface::Surface>>,
    mut glows: ResMut<Assets<Glow>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let (time,tuning,flash_lights)=timing;
    let state = &player.state;
    // A hit of his own body: the impact preset's effect for the surface. A blast's hits
    // do not come this way (`docs/melee.md`). [game]
    for hit in state.hits.iter().filter(|hit| !hit.blast) {
        let Some(info) = data.0.melee_impact.pick(hit.surface) else { continue };
        let Some(template) = &info.template else { continue };
        let target = arena.0.targets.iter().find(|t| t.id == hit.target).filter(|_| info.effect_flags & ATTACH_TO_HIT_OBJECT != 0);
        let anchor = target.map_or(Anchor::Fixed, |t| Anchor::Target { id: t.id, offset: hit.point - t.pos });
        effects.start(template, anchor, Affine3A::from_translation(hit.point), 0, 0);
    }
    // The messages of this update: a clip's triggers, and the car's own.
    let mut triggers = state.triggers.clone();
    triggers.append(&mut effects.vehicle_triggers);
    let driving = state.mode == Mode::Drive;
    // A robot flyer's flying form is the robot itself (`modeFly.robot`): no vehicle object, so
    // its thrusters' effects are on the robot's nodes. [data]
    let in_car = state.is_vehicle() && !state.flies_as_robot(&tuning.0);
    // FlyMode's exhaust (`fxstate::kept`): on from the take-off clip's trigger, off for the brake,
    // on again as the brake ends, off out of the flying form. [game 007325e0, 00865b80] [data]
    let flying = state.mode == Mode::Fly;
    let braking = flying && state.fly_timers.brake_left > 0.0;
    if !flying || braking {
        effects.fly_exhaust = false;
    } else if effects.fly_braking || triggers.contains(&fxstate::EXHAUST) {
        effects.fly_exhaust = true;
    }
    effects.fly_braking = braking;
    // CollisionTrigger's game-update history and CollisionFX's keyed loop lifecycle.
    // The kinds (impact, roll, scrape, end) and their thresholds are the game's: [game 007da7b0]
    // (the ten-update signed history, +7 roll, -7 scrape, impact once at the fresh contact's first
    // update, expiry at .25 s) and [game 007d9e40] (the CollisionFX per kind). The Lux physics type
    // of each contact is the one contact_refresh (006d7c20) gives the listener, from the approach and
    // the tangential speed of that refresh (`collisionfx::physics_type`), and the touch rules are
    // 007dab90's. [game] The contact's normal is taken as out of the surface (see collisionfx::Event).
    for &contact in &state.vehicle_contacts {effects.collision_pending.insert(contact.key,contact);}
    effects.collision_clock+=time.delta_secs();
    let mut collision_events=Vec::new();
    if !driving {
        collision_events=effects.collision_history.clear();
        effects.collision_pending.clear();effects.collision_clock=0.0;
    }
    while driving && effects.collision_clock>=tf2_core::sim::GAME_UPDATE {
        effects.collision_clock-=tf2_core::sim::GAME_UPDATE;
        for (_,contact) in std::mem::take(&mut effects.collision_pending) {
            if data.0.vehicle.as_ref().is_some_and(|object|object.collision_fx.iter().any(|d|d.accepts(&contact))) {
                let kind=tf2_core::collisionfx::physics_type(contact.normal,contact.velocity);
                effects.collision_history.touch(contact,kind);
            }
        }
        collision_events.extend(effects.collision_history.step(tf2_core::sim::GAME_UPDATE));
        let effects=&mut *effects;
        for running in &mut effects.running {
            if let Anchor::Collision {key,ref mut offset}=running.anchor
                && let Some(point)=effects.collision_history.point(key) {
                let delta=point-state.pos-*offset;
                *offset+=delta.clamp_length_max(0.01); // 007d94d0, body-relative contact smoothing
                running.place.translation=(state.pos+*offset).into();
            }
        }
    }
    for event in collision_events {
        for running in effects.running.iter_mut().filter(|r|r.collision.is_some_and(|(_,key)|key==event.key)) {
            running.effect.stop();running.anchor=Anchor::Fixed;
        }
        let Some(kind)=event.kind.index() else {continue;};
        let hard=usize::from(tuning.0.surface_hardness.get(&event.surface).copied().unwrap_or(false));
        for (index,def) in data.0.vehicle.iter().flat_map(|o|o.collision_fx.iter()).enumerate() {
            if !def.accepts_speed(event.speed) {continue;}
            let Some(template)=&def.particles[hard][kind] else {continue;};
            effects.collision_visuals_started+=1;
            let turn=if def.align {tf2_core::collisionfx::normal_rotation(event.normal)} else {Quat::IDENTITY};
            let anchor=if kind==0 {Anchor::Fixed} else {Anchor::Collision {key:event.key,offset:event.point-state.pos}};
            effects.start(template,anchor,Affine3A::from_rotation_translation(turn,event.point),0,0);
            if kind!=0 && let Some(r)=effects.running.last_mut() {r.collision=Some((index,event.key));}
        }
    }
    // The messages a state keeps on (`fxstate::kept`: the exhaust and the headlights in
    // the car, the turbo's fire, the damage smoke, the health coming back): one that went
    // on is triggered, one that went off is told to stop. [game: who sends each]
    // [game: the senders are in docs/particles.md "The senders" (2026-10-05): each message goes to his object AND to the object at `+600` (flight-helpers.md, char+0x258), not read again here]
    // [assumed: a message reaches only the object of the form he is in; the game sends it to the `+600` object too, which the port does not do, so on a change of form the effect is stopped on the old object and started on the new] settles: which object `+600` is in each form (char+0x258 writer, not read) and whether a Trigger that reaches it starts an effect with a visible node there.
    let kept = fxstate::kept(state, &tuning.0, effects.headlights, effects.fly_exhaust);
    for r in effects.running.iter_mut().filter(|r| fxstate::KEPT.contains(&r.script) && !r.effect.stopping()) {
        if !kept.contains(&r.script) || r.on_car != in_car {
            r.effect.stop();
            // A headlight is a spawned object, and its stop deletes the object
            // (`FUN_008158a0`): the glow goes at once, it does not live out. [game]
            r.gone = [fxstate::HEADLIGHT_L, fxstate::HEADLIGHT_R].contains(&r.script);
        }
    }
    for name in kept {
        // A clip's trigger of the same name this frame starts it already (the flight exhaust).
        if !triggers.contains(&name) && !effects.running.iter().any(|r| r.script == name && !r.effect.stopping()) {
            triggers.push(name);
        }
    }
    // Every effect on his object that goes by a message's name, at its node: the car's
    // object while he is the car, the robot's otherwise.
    let (object, nodes) = match (in_car, data.0.vehicle.as_ref(), car.as_deref()) {
        (true, Some(vehicle), Some(nodes)) => (vehicle, *nodes),
        _ => (&data.0.robot, *robot),
    };
    for name in &triggers {
        for (index, def) in object.effects.iter().enumerate().filter(|(_, def)| def.script_name == *name) {
            // A TriggerableParticle with a live system ignores the Trigger (0057dd30). [game]
            // [assumed: "live" is taken as started and not told to stop; the game's handle may
            // stay live while a stopped system's particles drain, which would also block a
            // restart then. settles: when 0057dd30's handle at +0x28 is cleared]
            let live = |r: &Running| r.script == *name && !r.effect.stopping() && r.on_car == in_car
                && matches!(r.anchor, Anchor::Node { node, .. } if node == def.node);
            if def.triggerable && effects.running.iter().any(live) {
                continue;
            }
            let Some(place) = node_frame(nodes, def.node, !def.upright, &placed) else { continue };
            // The spawner's own object, if it has anything to show: put there upright.
            // [stand-in: it stays there; none of his is tied to its node] OPEN: settles: the
            // game's trigger spawn of a proxy object (the effect spawner 0057d0d0 / 0077d5d0 path
            // for a bit-8 effect attaches to the object, see the tyre note below); not read here.
            if let Some(proxy) = def.proxy.as_deref().filter(|proxy| !proxy.renders.is_empty()) {
                let effects = &mut *effects;
                let mut builder = Builder {
                    library: &data.0.library,
                    meshes: &mut meshes,
                    materials: &mut materials,
                    surfaces: &mut surfaces,
                    glows: &mut glows,
                    images: &mut images,
                    bindposes: &mut bindposes,
                    cache: &mut effects.cache,
                };
                let (root, _) = builder.spawn(&mut commands, proxy, "effect");
                let at = game_to_bevy(Vec3::from(place.translation));
                commands.entity(root).insert(Transform::from_translation(at).with_rotation(model_space_rotation()));
                effects.shells.push(Shell { root, car: in_car, def: index, age: 0.0, settled: false });
            }
            let anchor = if def.attach { Anchor::Node { car: in_car, node: def.node, turn: !def.upright } } else { Anchor::Fixed };
            effects.start(&def.template, anchor, place, 0, *name);
            if let Some(started) = effects.running.last_mut().filter(|r| r.script == *name) {
                started.on_car = in_car;
            }
        }
    }
    // The tyres (`FUN_007347c0`, `docs/sound.md` "The tyres and the suspension"): at each
    // wheel, while he rolls faster than 1 m/s on a surface, `vehicleTireRoadFX`'s effect
    // for that surface, and while the tyres skid `vehicleTireSkidFX`'s with it. Each is
    // kept by the wheel, started again when the surface changes and stopped when its
    // condition ends. [game] The presets' flags tie the effect to the car (bit 8). [data]
    // (The port first took "tied" to mean the wheel's node, upright: wrong, see below.) Read 2026-10-20: the
    // caller is vehicle_sound_update 007347c0 (calls at 007350bc, 007350ee, 007353f2, 00735424),
    // which passes a contact struct at esp+0xac to preset_effect_by_surface 0077d5d0. Its bit-8
    // branch (0x100, 0077d5d0 l.26-52) does NOT follow a wheel node: it takes the car object
    // (weak_ref_lock), converts the contact point to the car's frame with the inverse of the
    // car's own matrix (node_world_matrix 011749d0 node 0, inverse_affine3 005301d0), and spawns
    // the emitter attached to the car (0057e820) at that offset. So the game ties it to the car
    // at the contact point, not the wheel node upright. FIXED 2026-10-20 (lead): `Anchor::Offset`,
    // the car's node 0 times the point taken into its frame at the start. The point (struct esp+0xa8) is the tyre
    // node's world translation (00734e05-00734e35: 005509d0 then compose 0052fbf0), or the object's place for
    // node 0 (00734dd5-00734dfa); EBX+0x30 goes into the next field, which this branch does not read [game,
    // lead-checked 2026-10-20].
    let input = if driving { state.tyre_inputs.last() } else { None };
    if let Some(nodes) = car.as_deref() {
        for (wheel, name) in TYRES.into_iter().enumerate() {
            let heard=input.map(|input|tyre_sounds(&input.for_wheel(wheel))).unwrap_or_default();
            // A model with no tyre of that name takes its tread (`TREADS`: Megatron's tank).
            let node = nodes.name_hashes.iter().position(|&n| n == name)
                .or_else(|| nodes.name_hashes.iter().position(|&n| n == TREADS[wheel]));
            let Some(node) = node else { continue };
            for (skid, preset, surface) in [(true, &data.0.tyre_skid, heard.skid), (false, &data.0.tyre_road, heard.road)] {
                let going = effects.running.iter().position(|r| matches!(r.tyre, Some((w, s, _)) if w == wheel && s == skid) && !r.effect.stopping());
                if going.and_then(|i| effects.running[i].tyre).map(|tyre| tyre.2) == surface {
                    continue;
                }
                if let Some(i) = going {
                    effects.running[i].effect.stop();
                }
                let Some(surface) = surface else { continue };
                let Some(template) = preset.pick(surface).and_then(|info| info.template.as_ref()).filter(|t| !t.elements.is_empty()) else { continue };
                // [game 007347c0 at 00734dd5-00734e68, 0077d5d0 l.20-52; lead-checked 2026-10-20, wave 30 R9] at
                // the tyre NODE's world position (the composed node, 005509d0 + 0052fbf0, stored at the struct's
                // point; the ground hit goes into the next field and this branch does not read it), fixed in the
                // car's own frame (node 0, the object's matrix) from then on.
                let tyre = node_frame(nodes, node, false, &placed).map(|f| Vec3::from(f.translation));
                let root = node_frame(nodes, 0, true, &placed);
                let (anchor, place) = match (tyre, root) {
                    (Some(point), Some(root)) => {
                        let place = Affine3A::from_translation(point);
                        (Anchor::Offset { car: true, node: 0, offset: root.inverse() * place }, place)
                    }
                    _ => {
                        let Some(place) = node_frame(nodes, node, false, &placed) else { continue };
                        (Anchor::Node { car: true, node, turn: false }, place)
                    }
                };
                effects.start(template, anchor, place, 0, 0);
                if let Some(started) = effects.running.last_mut() {
                    started.tyre = Some((wheel, skid, surface));
                }
            }
        }
    }
    // The footstep and climb contacts whose one game update has passed start now (`steps`).
    let mut due = Vec::new();
    effects.delayed_steps.retain_mut(|(left, limb, hit)| {
        *left -= time.delta_secs();
        let ready = *left <= 0.0;
        if ready {
            due.push((*limb, *hit));
        }
        !ready
    });
    effects.pending.extend(due.into_iter().map(|(limb, hit)| Pending::StepContact { limb, hit }));
    // His clips' events, on the robot's nodes.
    for event in std::mem::take(&mut effects.pending) {
        let find = |hash: u32| robot.name_hashes.iter().position(|&name| name == hash);
        match event {
            Pending::ClipLeft => effects.stop_clip_effects(None),
            Pending::ThingClipLeft { owner } => effects.stop_clip_effects(Some(owner)),
            Pending::Spawn { node, id, template, track, turn, with_clip } => {
                let Some(node) = find(node) else { continue };
                let Some(place) = node_frame(&robot, node, turn, &placed) else { continue };
                // The object keeps one effect under each id: a new one takes the place of
                // one still going, which is stopped (`FUN_00747ab0`). [game]
                if id != 0 {
                    effects.stop_event(id, None);
                }
                let anchor = if track { Anchor::Node { car: false, node, turn } } else { Anchor::Fixed };
                debug!("clip effect {:08x} at node {node} ({:?})", template.name, place.translation);
                effects.start(&template, anchor, place, id, 0);
                if let Some(started) = effects.running.last_mut().filter(|r| r.id == id) {
                    started.with_clip = with_clip;
                }
            }
            // A stopped effect makes no more and what it has made lives out (`Effect::stop`):
            // [game 0057e000] the stop sets the system's `+0x58` bit 3 (when the emitter's flag
            // 0x80 is clear: the muzzle's 0x3a is; the other spawners' flags are not checked);
            // [game 00a9f190 line 29] with it no new group is made; [game 00a9ff00 lines 49-78]
            // the group update then skips the leading source actions (`+0x14` == 0) and runs the
            // rest, so the particles move and age out.
            Pending::Stop { id } => effects.stop_event(id, None),
            Pending::StopOn { owner, id } => effects.stop_event(id, Some(owner)),
            Pending::SpawnOn { owner, entity, id, template, track, turn, with_clip } => {
                let Some(place) = entity_frame(entity, turn, &placed) else { continue };
                let anchor = if track { Anchor::Thing { entity, turn } } else { Anchor::Fixed };
                effects.start_on(&template, anchor, place, owner, id, with_clip);
            }
            // The character's handler (`FUN_00718720`) starts the limb's climb preset's
            // effect for the surface where the limb's own ray found the wall
            // (`FUN_0073fa20`, through `FUN_0077cd60` as a melee hit does). [game]
            Pending::StepContact { limb, hit } => {
                let preset = match limb { Some(limb) => data.0.climb_impact.get(limb), None => Some(&data.0.step_impact) };
                let Some(info) = preset.and_then(|preset| preset.pick(hit.surface)) else { continue };
                let Some(template) = &info.template else { continue };
                // [data] Step flags0x100 attach to the struck object; his climb flags1
                // put z along normal. Arena surfaces are static, so fixed world placement
                // retains the step effect's attachment position (moving surfaces absent).
                let turn = tf2_core::stepfx::rotation(info.effect_flags, hit.normal);
                effects.start(template, Anchor::Fixed, Affine3A::from_rotation_translation(turn, hit.point), 0, 0);
            }
            // Resolved from the final pose by animation::animate before start runs.
            Pending::Step { .. } | Pending::ClimbStep { .. } => {}
        }
    }
    // What his weapons asked for.
    let mut asked = Vec::new();
    let mut arsenal = arsenal;
    if let Some(arsenal) = arsenal.as_mut() {
        // The projectiles' emitters ride their nodes, and stop making more where the projectile
        // (or the wave) ends: the stop with instantKill clear only stops emission (0057e000), so
        // what they made lives out; with it set, it goes. [game 0057d0d0, 0057e000] [assumed] the
        // object goes at the projectile's end (not after its Destructable's `auto_delete_time`).
        // 007436b0 (destroy) is read: it does not read auto_delete_time. 00785c00 (missile_kill, read in full
        // 2026-10-20) is the end at lifetime: it sends a damage message (00724700) and stops ticking (011767b0);
        // it does not remove the object itself. 007436b0 is reached only through the Destructable vtable slot 1
        // (00b5d94c; no direct caller). auto_delete_time (9bf4465f) is read once, by the Destructable ctor 00742f10
        // into +0x138 (`param_1[0x4e]`, default 0). Its reader (found 2026-10-20): 00743a40 l.195 passes +0x138 to the
        // health take 00554440, which hands it to 005544e0 as the death record's entry [6]; the drain 005546a0 ->
        // 00554830 reads it at l.148, and a positive value skips the branch at l.149-158 (the robots' missiles carry
        // 2): l.163-169 call 00552270(delay) with it (5.0 when none is set and no 12936d90 scene object), which posts
        // message 59ef62fd (unnamed) to the handle at its ESI after that delay (005a9800; census: lux::Debris). OPEN:
        // settles: whether that handle is the dying projectile or what 00554830 just built
        // (`mark_for_removal` 005a91e0 at l.233 is on the other path).
        for r in effects.running.iter_mut() {
            if let Anchor::Energon { number, slot, bind } = r.anchor {
                match arsenal.energon_frame(number, slot) {
                    Some(frame) => r.place = frame * bind,
                    None => {
                        r.effect.stop();
                        r.anchor = Anchor::Fixed;
                    }
                }
                continue;
            }
            let Anchor::Projectile { number, wave, bind, rot, scale, kill } = r.anchor else { continue };
            let frame = if wave { arsenal.wave_frame(number) } else { arsenal.projectile_frame(number) };
            match frame {
                Some(frame) => r.place = node_place(frame, bind, rot, scale),
                None => {
                    r.effect.stop();
                    r.gone = kill;
                    r.anchor = Anchor::Fixed;
                }
            }
        }
        // The lights on them, the same way; a light goes with its object.
        for riding in effects.riding.iter_mut() {
            let frame = match riding.anchor {
                LightAnchor::Projectile(number) => arsenal.projectile_frame(number),
                LightAnchor::Wave(number) => arsenal.wave_frame(number),
                LightAnchor::Bullet(_) => continue,
            };
            match frame {
                Some(frame) => riding.place(frame),
                None => riding.gone = true,
            }
        }
        asked = std::mem::take(&mut arsenal.fx);
    }
    let arsenal = arsenal.as_deref();
    // The lingering smoke: each particle starts at its TRIGGER's delay and stops at its
    // UNTRIGGER's. [game 0080df70] The Untrigger message (9caa723d, which is -0x63558dc3 as the
    // dispatcher's signed compare reads it) goes to vslot 7 (00746fa0), which calls 0057e000 on the
    // emitter at +0x44: emission stops and the particles live out. Trigger goes to vslot 6 (00746f80).
    let dt = time.delta_secs();
    let mut timed = std::mem::take(&mut effects.timed);
    timed.retain_mut(|t| {
        t.start_in -= dt;
        t.stop_in -= dt;
        if !t.started && t.start_in <= 0.0 {
            t.started = true;
            effects.start(&t.template, Anchor::Fixed, t.place, t.id, 0);
        }
        if t.started && t.stop_in <= 0.0 {
            effects.stop_event(t.id, None);
            return false;
        }
        true
    });
    effects.timed.extend(timed);
    for fx in asked {
        // The charged shot and the flame: their templates are the weapon's own (`WeaponFxSpec`),
        // on the weapon object's nodes (`ChargeDummy`, `FireDummy`, the flame's dummies).
        if matches!(fx.kind, WeaponFxKind::ChargeStart | WeaponFxKind::ChargeLevel(_) | WeaponFxKind::ChargeFull | WeaponFxKind::ChargeStop | WeaponFxKind::FlameStream | WeaponFxKind::FlameBall(_) | WeaponFxKind::FlameStop | WeaponFxKind::FlameImpact | WeaponFxKind::FlameImpactLost) {
            let Some(spec) = data.0.weapons.iter().find(|w| w.name == fx.weapon).and_then(|w| w.fx.as_deref()) else { continue };
            let nodes = weapon_models.iter().find(|(model, _)| model.names.contains(&fx.weapon)).map(|(_, nodes)| nodes);
            let named = |name: &[u8]| nodes.and_then(|n| n.name_hashes.iter().position(|&h| h == crc32(name)).and_then(|i| n.entities.get(i).copied()));
            // Each weapon's charge emitters are its own (the weapon's `+0x3fc` and the full
            // charge's), so a child's on its own ChargeDummy does not stop its parent's. [game]
            let (charge_id, ready_id) = (CHARGE ^ fx.weapon, CHARGE_READY ^ fx.weapon);
            match fx.kind {
                // [game 007adae0] the start spawns `chargeId` with scale `minChargeEmissionRate`
                // (`+0x410`; 0.01 where that is not positive, as the step's).
                WeaponFxKind::ChargeStart => {
                    effects.stop_event(charge_id, None);
                    if std::env::var("TF_DBG").is_ok() {
                        eprintln!("charge effect {:#010x}: template {} model {} node {}", fx.weapon, spec.charge.is_some(), nodes.is_some(), named(b"ChargeDummy").is_some());
                    }
                    if let (Some(template), Some(entity)) = (&spec.charge, named(b"ChargeDummy"))
                        && let Some(place) = entity_frame(entity, true, &placed) {
                        let before = effects.running.len();
                        effects.start(template, Anchor::Thing { entity, turn: true }, place, charge_id, 0);
                        if let Some(r) = effects.running.get_mut(before) {
                            r.effect.emission = if spec.charge_emission[0] > 0.0 { spec.charge_emission[0] } else { 0.01 };
                        }
                    }
                }
                // [game 007addc0] scale = (max - min) * level + min; 0.01 where that is not positive.
                WeaponFxKind::ChargeLevel(level) => {
                    let [min, max] = spec.charge_emission;
                    let scale = match (max - min) * level + min { s if s > 0.0 => s, _ => 0.01 };
                    if std::env::var("TF_DBG").is_ok() {
                        let live: usize = effects.running.iter().filter(|r| r.id == charge_id)
                            .map(|r| (0..r.template.elements.len()).filter_map(|i| r.effect.group(i)).map(|g| g.particles.len()).sum::<usize>()).sum();
                        eprintln!("charge level {level:.2} emission {scale:.3} particles {live}");
                    }
                    for r in effects.running.iter_mut().filter(|r| r.id == charge_id && r.owner.is_none() && !r.effect.stopping()) {
                        r.effect.emission = scale;
                    }
                }
                // [game 007b0520] the charge's emitter is stopped first, then the full charge's effect starts.
                WeaponFxKind::ChargeFull => {
                    effects.stop_event(charge_id, None);
                    if let (Some(template), Some(entity)) = (&spec.charge_ready, named(b"ChargeDummy"))
                        && let Some(place) = entity_frame(entity, true, &placed) { effects.start(template, Anchor::Thing { entity, turn: true }, place, ready_id, 0); }
                }
                // [game 007af210 l.376-402] the release stops the emitter in `+0x3fc`. At full charge that
                // slot holds the full charge's effect, not the charge emitter: 007b0520 stops the charge emitter
                // in `+0x3fc`, spawns the ready emitter from the TEMPLATE in `+0x408` (`chargeMaxReadyId`, loaded
                // once by 007ab9e0 l.135-145; `+0x404` is `chargeId`'s), and copies its handle into `+0x3fc`
                // (007b065c `mov esi, ebx` with ebx = this+0x3fc, then 0074af20) [game, raw_disasm 2026-10-20].
                // So the release stops the full charge's sparks, as stopping both ids does here.
                WeaponFxKind::ChargeStop => {
                    effects.stop_event(charge_id, None);
                    effects.stop_event(ready_id, None);
                }
                WeaponFxKind::FlameStream => {
                    effects.stop_event(FLAME_STREAM, None);
                    if let (Some(template), Some(entity)) = (&spec.stream, named(b"FireDummy"))
                        && let Some(place) = entity_frame(entity, true, &placed) { effects.start(template, Anchor::Thing { entity, turn: true }, place, FLAME_STREAM, 0); }
                }
                WeaponFxKind::FlameBall(node) => {
                    let Some((_, template)) = spec.balls.iter().find(|(n, _)| *n == node) else { continue };
                    let Some(entity) = nodes.and_then(|n| n.entities.get(node).copied()) else { continue };
                    let id = FLAME_BALL + node as u32;
                    effects.stop_event(id, None);
                    if let Some(place) = entity_frame(entity, true, &placed) { effects.start(template, Anchor::Thing { entity, turn: true }, place, id, 0); }
                }
                WeaponFxKind::FlameStop => {
                    effects.stop_event(FLAME_STREAM, None);
                    effects.stop_event(FLAME_IMPACT, None);
                    for (node, _) in &spec.balls {
                        effects.stop_event(FLAME_BALL + *node as u32, None);
                    }
                }
                // One emitter, moved to each update's hit (its elements never end).
                WeaponFxKind::FlameImpact => {
                    let Some(template) = &spec.impact else { continue };
                    let place = Affine3A::from_rotation_translation(Quat::from_rotation_arc(Vec3::Z, fx.along), fx.at);
                    match effects.running.iter_mut().find(|r| r.id == FLAME_IMPACT && r.owner.is_none() && !r.effect.stopping()) {
                        Some(running) => running.place = place,
                        None => effects.start(template, Anchor::Fixed, place, FLAME_IMPACT, 0),
                    }
                }
                WeaponFxKind::FlameImpactLost => effects.stop_event(FLAME_IMPACT, None),
                _ => {}
            }
            continue;
        }
        let Some(weapon) = data.0.weapon_effects.iter().find(|w| w.name == fx.weapon) else {
            if std::env::var("TF_DBG").is_ok() {
                eprintln!("weapon fx {:?}: no effects row for {:#010x} (rows {:x?})", fx.kind, fx.weapon, data.0.weapon_effects.iter().map(|w| w.name).collect::<Vec<_>>());
            }
            continue
        };
        match fx.kind {
            WeaponFxKind::Muzzle(barrel) => {
                // [game: 007ae010/007aac80] MuzzleFlashDummy, else the firing
                // barrel. The weapons of one list share its posed object (`WeaponModel::names`:
                // his cannon and launcher, Optimus's right pair...). [game 007b0f10] [data]
                let entity = weapon_models.iter().find(|(model, _)| model.names.contains(&fx.weapon))
                    .and_then(|(_, nodes)| muzzle_entity(nodes, barrel));
                let on_model = entity.and_then(|e| Some((e, entity_frame(e, true, &placed)?)));
                // Test aid: where the flash's node is against where the round leaves.
                if std::env::var_os("TF_MUZZLE_LOG").is_some() {
                    eprintln!("muzzle fx {:#010x} barrel {barrel}: dummy at {:?}, round from ({:.2}, {:.2}, {:.2})", fx.weapon,
                        on_model.map(|(_, p)| p.translation), fx.at.x, fx.at.y, fx.at.z);
                }
                // A weapon with no model of his to sit on (the turret launcher's throw, the deployed
                // turret's gun) flashes where its round leaves, turned along it, and stays there.
                // [assumed] the round's way stands for the barrel node's +y (as for a bullet's
                // emitter, 007832b0), and a flash this short does not need to follow the barrel.
                // The flash object's spawn is weapon_muzzle_effect 007aac80 [game]: it is placed at the weapon
                // record's matrix (param_2 == 0, the owner at weapon +0xc: its +0x10..+0x3c) or the node matrix
                // (005509d0, param_2 != 0). Its callers are weapon_fire 007ae010 through the gun vtable +0x7c
                // (lines 186, 382, 431): 431 passes the MuzzleFlashDummy's node index (found by name 0x0efe3b8b);
                // 186 and 382 pass the fallback indices that 007a8770 (vtable +0x4c, unread) fills, only when the
                // dummy is absent. OPEN: settles: 007a8770 (the fallback indices of a model-less weapon, which
                // decide whether this flash exists at all) and 005509d0's index base (ESI-1) for the node path.
                let (anchor, place) = match on_model {
                    Some((entity, place)) => (Anchor::Thing { entity, turn: true }, place),
                    None => {
                        if std::env::var_os("TF_DBG").is_some() {
                            eprintln!("muzzle fx {:#010x}: no model, flash {} at ({:.2}, {:.2}, {:.2})", fx.weapon,
                                weapon.muzzle_flash.is_some(), fx.at.x, fx.at.y, fx.at.z);
                        }
                        let turn = Quat::from_rotation_arc(Vec3::Y, fx.along.try_normalize().unwrap_or(Vec3::Y));
                        (Anchor::Fixed, Affine3A::from_rotation_translation(turn, fx.at))
                    }
                };
                if let Some(template) = &weapon.muzzle_flash {
                    effects.start(template, anchor, place, 0, 0);
                }
                // [game 007aac80] The same routine builds the weapon's `muzzleFlashObject` at the
                // node's matrix: its Light, removed by its TimeoutDeleter (`point_light::FlashLight`).
                let flash = flash_lights.as_ref().and_then(|lights| lights.0.iter().find(|(name, _)| *name == fx.weapon));
                if let Some((_, flash)) = flash {
                    effects.flashes.push(flash.spawn(place));
                }
            }
            WeaponFxKind::Impact(surface, hit_normal) => {
                let Some(info) = weapon.impact.pick(surface) else { continue };
                let Some(template) = &info.template else { continue };
                // [game 0077d0b7, 007832b0 l.160-165, 00782b30; wave 25 read, ported 2026-10-20] impact_orientation
                // 0077c180 turns the emitter by the HIT's normal (round+0x4c, copied by gun_round_trace from the ray
                // query's hit record and handed on by bullet_damage / material_impact) when flag bit 0 is set (the
                // Z_ALONG_NORMAL path here). The world hit's normal is carried in; [stand-in: a hit on a target's body has no normal here (the port's body test returns none), so it faces the round]
                let normal = hit_normal.try_normalize().unwrap_or_else(|| (-fx.along).try_normalize().unwrap_or(Vec3::Z));
                let turn = if info.effect_flags & Z_ALONG_NORMAL != 0 { Quat::from_rotation_arc(Vec3::Z, normal) } else { Quat::IDENTITY };
                effects.start(template, Anchor::Fixed, Affine3A::from_rotation_translation(turn, fx.at), 0, 0);
            }
            // The projectile object's own emitters, each on its node (Bumblebee's micro missile:
            // five, one per sub-missile), started as the object is made by their autoStart and
            // spawnChance (0057d0d0), and its Lights riding it. [game 007aaeb0: a new instance per
            // shot, so no pool] [data] Without an object (a frame), the single trail at the
            // centre, x along the flight. [assumed, as before]
            WeaponFxKind::Trail(number) => {
                let frame = arsenal.and_then(|a| a.projectile_frame(number));
                match frame {
                    Some(frame) if !weapon.emitters.is_empty() => {
                        let phase = fx.at.y + fx.at.z;
                        // Devastator's erupted bug bomb draws one trail, at its centre. [assumed port design]
                        let lone = arsenal.is_some_and(|a| a.erupted(number));
                        for e in weapon.emitters.iter().filter(|e| e.auto_start).take(if lone { 1 } else { usize::MAX }) {
                            // [game 0057d0d0] the gate is fract(object y + z), not rand().
                            if !lone && e.spawn_chance < 1.0 && e.spawn_chance < phase - phase.floor() { continue; }
                            let bind = if lone { Affine3A::IDENTITY } else { rows_affine(&e.bind) };
                            let anchor = Anchor::Projectile { number, wave: false, bind, rot: e.use_rot, scale: e.use_scale, kill: e.instant_kill };
                            effects.start(&e.template, anchor, node_place(frame, bind, e.use_rot, e.use_scale), 0, 0);
                        }
                    }
                    _ => {
                        if let Some(template) = &weapon.trail {
                            let place = frame.unwrap_or_else(|| frame_along_x(fx.at, fx.along));
                            let anchor = Anchor::Projectile { number, wave: false, bind: Affine3A::IDENTITY, rot: true, scale: false, kill: false };
                            effects.start(template, anchor, place, 0, 0);
                        }
                    }
                }
                if let Some(frame) = frame {
                    for light in &weapon.flight_lights {
                        effects.riding.push(Riding::new(LightAnchor::Projectile(number), light, frame, None));
                    }
                }
            }
            // A ground wave's object: its emitter is its picture (it has no mesh), its Light rides
            // it, faded and ended by the object's TimeoutDeleter. [data] [game 00776f50]
            WeaponFxKind::Wave(number) => {
                let (Some(wave), Some(frame)) = (weapon.wave.as_ref(), arsenal.and_then(|a| a.wave_frame(number))) else { continue };
                if let Some(e) = wave.emitter.as_ref().filter(|e| e.auto_start) {
                    let bind = rows_affine(&e.bind);
                    let anchor = Anchor::Projectile { number, wave: true, bind, rot: e.use_rot, scale: e.use_scale, kill: e.instant_kill };
                    effects.start(&e.template, anchor, node_place(frame, bind, e.use_rot, e.use_scale), 0, 0);
                }
                if let Some(light) = &wave.light {
                    effects.riding.push(Riding::new(LightAnchor::Wave(number), &light.light, frame, Some(light)));
                }
            }
            // A thrown piece's own emitters (Bumblebee's chunks: `smk_velocitysmoke_trail_nofire` on node 1,
            // useRot 0, deleteWhenDone 1), started as its object is made and riding it; where it goes they
            // stop and what they made lives out (instantKill 0). [game 0057d0d0, 0057e000] [data]
            WeaponFxKind::Debris(number, piece) => {
                let (Some(p), Some(frame)) = (weapon.debris.get(piece), arsenal.and_then(|a| a.projectile_frame(number))) else { continue };
                let phase = fx.at.y + fx.at.z;
                for e in p.emitters.iter().filter(|e| e.auto_start) {
                    // [game 0057d0d0] the gate is fract(object y + z), not rand().
                    if e.spawn_chance < 1.0 && e.spawn_chance < phase - phase.floor() { continue; }
                    let bind = rows_affine(&e.bind);
                    let anchor = Anchor::Projectile { number, wave: false, bind, rot: e.use_rot, scale: e.use_scale, kill: e.instant_kill };
                    effects.start(&e.template, anchor, node_place(frame, bind, e.use_rot, e.use_scale), 0, 0);
                }
            }
            // Each `destroyEffectSpawn` particle at the end, at its own delays (drawn from the
            // port's generator), upright: its useRot takes the FX_Dummy's turn, which is the
            // Instance's identity matrix [data]. [assumed] the FX_Dummy stands upright at the
            // round's end (the spawn scene's placement is not read). OPEN, the chain read 2026-10-20: the field
            // (9c488961) is Destructable +0x14c (ctor 00742f10, the only code with the hash), whose address
            // 00743a40 passes to the health take 00554440 (00744063); the death record (005544e0) is drained by
            // 005546a0 into 00554830, which spawns each `spawnScene` of its list (l.73-86, 00575880) at the
            // entry's `matrix` composed with the object's frame and scaled by its mean scale (l.33-64); the list
            // it walks IS the +0x14c pair (00554440's last argument -> 005544e0 param_4 -> entry [2] / [3]).
            // settles: the damage entry's `matrix` in the projectile's data (the FX_Dummy upright or not).
            WeaponFxKind::AfterBlast => {
                for p in &weapon.after_blast {
                    let start_in = p.on.0 + effects.rng.unit() * (p.on.1 - p.on.0);
                    let stop_in = p.off.0 + effects.rng.unit() * (p.off.1 - p.off.0);
                    let place = Affine3A::from_translation(fx.at + p.offset);
                    effects.timed_count = effects.timed_count.wrapping_add(1);
                    let id = TIMED_ID | (effects.timed_count & 0x00ff_ffff);
                    effects.timed.push(Timed { template: p.template.clone(), place, start_in, stop_in, id, started: false });
                }
            }
            WeaponFxKind::ChargeStart | WeaponFxKind::ChargeLevel(_) | WeaponFxKind::ChargeFull | WeaponFxKind::ChargeStop
            | WeaponFxKind::FlameStream | WeaponFxKind::FlameBall(_) | WeaponFxKind::FlameStop
            | WeaponFxKind::FlameImpact | WeaponFxKind::FlameImpactLost => {}
            WeaponFxKind::BulletTrail {end,speed} => {
                if let Some(template)=&weapon.trail {
                    // 007832b0 uses the projectile object's +y row for velocity. [game]
                    let direction=(end-fx.at).normalize_or_zero();
                    let turn=Quat::from_rotation_arc(Vec3::Y,direction);
                    let bullet=tf2_core::weapons::BulletVisual::new(fx.at,end,speed);
                    effects.start(template,Anchor::Bullet(bullet),Affine3A::from_rotation_translation(turn,fx.at),0,0);
                }
                // The bullet object's own Light flies with it and ends with the visual. [data]
                // [assumed] it is placed by the round's +y along its way, as its emitter. OPEN: settles: the bullet
                // Light's frame in the round's own update (its spawn is not read; 007832b0 is the trace only).
                let direction=(end-fx.at).normalize_or_zero();
                let frame=Affine3A::from_rotation_translation(Quat::from_rotation_arc(Vec3::Y,direction),fx.at);
                for light in &weapon.flight_lights {
                    let bullet=tf2_core::weapons::BulletVisual::new(fx.at,end,speed);
                    effects.riding.push(Riding::new(LightAnchor::Bullet(bullet),light,frame,None));
                }
            }
            WeaponFxKind::Blast => {
                if let Some(template) = &weapon.blast {
                    effects.start(template, Anchor::Fixed, Affine3A::from_translation(fx.at), 0, 0);
                }
                // The blast object's Light, removed (and faded) by its TimeoutDeleter. [data]
                // [assumed] placed at the blast's point with no turn (an omni: the turn is irrelevant).
                if let Some(flash) = &weapon.blast_light {
                    effects.flashes.push(flash.spawn(Affine3A::from_translation(fx.at)));
                }
            }
        }
    }
}

/// Runs the effects and rebuilds their quads to face the camera.
#[allow(clippy::too_many_arguments)]
pub fn run(
    time: Res<Time>,
    data: Res<crate::GameData>,
    level: Option<Res<crate::level::LevelModels>>,
    arena: Res<ArenaRes>,
    robot: Single<&ObjectNodes, (With<RobotModel>, Without<VehicleModel>)>,
    car: Option<Single<&ObjectNodes, (With<VehicleModel>, Without<RobotModel>)>>,
    placed: Query<&GlobalTransform>,
    camera: Single<&Transform, (With<Camera3d>, Without<crate::ocean::OceanReflection>, Without<crate::duel::SecondView>)>,
    shell_nodes: Query<&ObjectNodes, (Without<RobotModel>, Without<VehicleModel>)>,
    (children, parents): (Query<&Children>, Query<&ChildOf>),
    part_materials: Query<(Option<&MeshMaterial3d<StandardMaterial>>, Option<&MeshMaterial3d<Glow>>)>,
    mut transforms: Query<&mut Transform, Without<Camera3d>>,
    mut effects: ResMut<Effects>,
    mut commands: Commands,
    (mut meshes, mut materials, mut glows, mut fx_materials, mut warp_materials, mut images): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Glow>>,
        ResMut<Assets<Fx>>,
        ResMut<Assets<Warp>>,
        ResMut<Assets<Image>>,
    ),
    mut dynamic_lights: ResMut<crate::surface::DynamicLights>,
) {
    let dt = time.delta_secs().min(0.05);
    let Effects { flashes, riding, running, shells, rng, materials: made, warps, bullet_sprites_submitted, .. } = &mut *effects;
    // The muzzle lights and the blasts' flashes: each update of their TimeoutDeleters, then what
    // is left is drawn. [game 00776f50]
    flashes.retain_mut(|flash| flash.step(dt));
    // The riding lights: a bullet's moves with its visual and ends with it; a wave's is faded and
    // ended by its object's deleter. (A projectile's and a wave's are placed in `start`.)
    riding.retain_mut(|r| {
        if let LightAnchor::Bullet(ref mut bullet) = r.anchor {
            if bullet.step(dt) { r.light.position = bullet.position; } else { r.gone = true; }
        }
        if let Some(deleter) = r.deleter.as_mut() {
            r.gone |= !deleter.step(dt);
            r.light.colour = deleter.light.colour;
        }
        !r.gone
    });
    if !(flashes.is_empty() && riding.is_empty() && dynamic_lights.0.is_empty()) {
        let all: Vec<_> = flashes.iter().map(|flash| flash.light.clone()).chain(riding.iter().map(|r| r.light.clone())).collect();
        // The port's slots (`surface::FLASH_SLOTS`): the nearest to the eye are kept when there
        // are more. [port] The game's list holds 0x200 (LS35.2).
        let eye = bevy_to_game(camera.translation);
        dynamic_lights.0 = tf2_core::point_light::nearest_lights(&all, eye, crate::surface::FLASH_SLOTS).into_iter().map(|i| all[i].clone()).collect();
        if std::env::var_os("TF_DBG").is_some() && !riding.is_empty() {
            debug!("dynamic lights: {} flashes, {} riding, {} drawn", flashes.len(), riding.len(), dynamic_lights.0.len());
        }
    }
    shells.retain_mut(|shell| {
        let object = if shell.car { data.0.vehicle.as_ref().unwrap_or(&data.0.robot) } else { &data.0.robot };
        let Some(def) = object.effects.get(shell.def) else { return false };
        let (life, fade_start) = def.timeout.unwrap_or((1.0, 1.0));
        shell.age += dt;
        if shell.age >= life {
            commands.entity(shell.root).despawn();
            return false;
        }
        // Its clip moves its nodes, each by its name, from the start once.
        if let (Ok(nodes), Some(clip)) = (shell_nodes.get(shell.root), &def.clip) {
            let tick = (shell.age * TICKS_PER_SECOND).min(clip.length);
            for (i, &hash) in nodes.name_hashes.iter().enumerate() {
                let Some(channel) = clip.channel(hash) else { continue };
                if let Ok(mut transform) = transforms.get_mut(nodes.entities[i]) {
                    *transform = nodes.rest[i];
                    // No move, no turn, scale 1 where unkeyed, as every clip's channel
                    // (`animation::sample`). [game 0053b520]
                    if let Some((p, q, s)) = channel.local(tick, clip.length, false) {
                        *transform = Transform { translation: Vec3::from(p), rotation: stored_turn(q), scale: Vec3::from(s) };
                    }
                }
                // Its own scale sizes its own frame (`ObjectNodes::frames`). [game 00544e00]
                if let (Some(s), Some(&frame)) = (channel.own_scale.sample(tick, clip.length, false), nodes.frames.get(i))
                    && let Ok(mut frame) = transforms.get_mut(frame)
                {
                    frame.scale = Vec3::from(s).max(Vec3::splat(0.001));
                }
            }
        }
        // The deleter's fade: with `fadeStart` >= 0 its `+0x2c` is `time - fadeStart`, and once
        // what is left is under that, the fade is left / `+0x2c` in 0..1 (so linear from
        // `fadeStart` to the end); a negative `fadeStart` leaves `+0x2c` at -1 and there is no
        // fade. [game 00776f50 l.1-30: the fade (remaining / +0x2c, clamped) goes into +0x30 and to 008fc210, a list walk that calls each entry's +0xc with (entry+8, 0, fade)] [assumed: it is applied as the object's own colour's alpha, which the shader multiplies into the material's. The entries' callbacks (008fc210's list) are not read. settles: the function at each list entry's +0xc (the callers of 008fc210 with the object's list, e.g. 007d1f0d, 008beba9), read for whether it sets an alpha.]
        let fade = if fade_start >= 0.0 && life > fade_start { ((life - shell.age) / (life - fade_start)).clamp(0.0, 1.0) } else { 1.0 };
        for part in children.iter_descendants(shell.root) {
            let Ok((standard, glow)) = part_materials.get(part) else { continue };
            if !shell.settled && (standard.is_some() || glow.is_some()) {
                commands.entity(part).insert(NotShadowCaster);
            }
            if let Some(mut material) = standard.and_then(|m| materials.get_mut(&m.0)) {
                material.base_color.set_alpha(fade);
            }
            if let Some(mut material) = glow.and_then(|m| glows.get_mut(&m.0)) {
                material.set_object_alpha(fade);
            }
        }
        shell.settled = true;
        true
    });
    for r in running.iter_mut() {
        match r.anchor {
            Anchor::Fixed | Anchor::Projectile { .. } | Anchor::Collision {..} | Anchor::Energon { .. } => {}
            Anchor::Bullet(ref mut bullet) => {
                if bullet.step(dt) {r.place.translation=bullet.position.into();}
                else {r.gone=true;} // installed emitter instantKill=true; no residual local particles
            }
            Anchor::Thing { entity, turn } => {
                if let Some(place) = entity_frame(entity, turn, &placed) {
                    r.place = place;
                }
            }
            Anchor::Target { id, offset } => {
                if let Some(target) = arena.0.targets.iter().find(|t| t.id == id) {
                    r.place.translation = (target.pos + offset).into();
                }
            }
            Anchor::Node { car: of_car, node, turn } => {
                let nodes = if of_car { car.as_deref().copied() } else { Some(*robot) };
                let now = |nodes: &ObjectNodes| node_frame_now(nodes, node, turn, &transforms, &parents).or_else(|| node_frame(nodes, node, turn, &placed));
                if let Some(place) = nodes.and_then(now) {
                    r.place = place;
                }
            }
            Anchor::Offset { car: of_car, node, offset } => {
                let nodes = if of_car { car.as_deref().copied() } else { Some(*robot) };
                let now = |nodes: &ObjectNodes| node_frame_now(nodes, node, true, &transforms, &parents).or_else(|| node_frame(nodes, node, true, &placed));
                if let Some(frame) = nodes.and_then(now) {
                    r.place = frame * offset;
                }
            }
        }
        if r.gone {continue;}
        r.effect.advance(&r.template, &r.place, dt, rng);
        for (index, element) in r.template.elements.iter().enumerate() {
            let sprites = r.effect.sprites(&r.template, index, &r.place);
            if matches!(&r.anchor,Anchor::Bullet(_)) {*bullet_sprites_submitted+=sprites.len() as u64;}
            let mesh = quads(&sprites, element, &camera, element.local_space.then_some(&r.place));
            match &r.parts[index] {
                // An element with nothing to show keeps its last quads, hidden: a mesh
                // with no vertices upsets the renderer.
                Some((entity, _)) if sprites.is_empty() => {
                    commands.entity(*entity).insert(Visibility::Hidden);
                }
                Some((entity, handle)) => {
                    let _ = meshes.insert(handle, mesh);
                    commands.entity(*entity).insert(Visibility::Inherited);
                }
                // The distortion renderer bends the picture behind it (`warp.wgsl`).
                None if !sprites.is_empty() && element.renderer.distortion => {
                    let texture = element.renderer.texture;
                    let material = warps
                        .entry(texture)
                        .or_insert_with(|| {
                            let map = data.0.library.textures.get(&texture)
                                .or_else(|| level.as_ref().and_then(|l| l.0.library.textures.get(&texture)))
                                .and_then(|t| image(t, false)).map(|i| images.add(i));
                            warp_materials.add(Warp { map })
                        })
                        .clone();
                    let handle = meshes.add(mesh);
                    let entity = commands
                        .spawn((Mesh3d(handle.clone()), MeshMaterial3d(material), Transform::IDENTITY, NoFrustumCulling, NotShadowCaster))
                        .id();
                    r.parts[index] = Some((entity, handle));
                }
                None if !sprites.is_empty() => {
                    let renderer = &element.renderer;
                    let material = made
                        .entry((renderer.texture, renderer.blend_mode, renderer.z_feather > 0.0, lit_key(renderer),
                            [renderer.alpha_fade.0.to_bits(), renderer.alpha_fade.1.to_bits()]))
                        .or_insert_with(|| {
                            // The shader works on the stored values: no decoding.
                            let texture = data.0.library.textures.get(&renderer.texture)
                                .or_else(|| level.as_ref().and_then(|l| l.0.library.textures.get(&renderer.texture)))
                                .and_then(|t| image(t, false)).map(|i| images.add(i));
                            fx_materials.add(Fx::new(renderer, texture))
                        })
                        .clone();
                    let handle = meshes.add(mesh);
                    let entity = commands
                        .spawn((Mesh3d(handle.clone()), MeshMaterial3d(material), Transform::IDENTITY, NoFrustumCulling, NotShadowCaster))
                        .id();
                    r.parts[index] = Some((entity, handle));
                }
                None => {}
            }
        }
    }
    running.retain(|r| {
        let finished = r.gone || r.effect.finished(&r.template);
        if finished {
            for (entity, _) in r.parts.iter().flatten() {
                commands.entity(*entity).despawn();
            }
        }
        !finished
    });
}

/// The quads of one element's particles, in Bevy's space, as the original's builders lay
/// them (`particles::Sprite` says what of this is read and what assumed). Each vertex
/// carries the two frames' places in the picture, the colour, and in its normal how far
/// the picture is on the way to the next frame (`fx.wgsl`).
///
/// `emitter` is the emitter's frame (the game's space) for a `localspace` element, whose
/// rectangle facing (`Normal`) is given in that frame and turns with it. [data] Blackout's rotor
/// blur is such an element: `FX_smoke_exhaust` on his vehicle object, `chopper_blades.dds` on
/// `Rect`s, with `Normal` (0, 0.1, 1) on `FX_smoke_exhaust1` (axes the vehicle's) = `Rotor_GOAL`'s
/// spin axis (0, 0.1, 0.995), and (0, 1, 0) on `FX_smoke_exhaust2` = `RearRotor`'s spin axis
/// (-0.961, 0, -0.275) in the vehicle's frame; read in the world, the tail's disc stood 90 degrees
/// off and the main one stayed level while he leant (`vehicle_fx_dump`, `rotor_tree --axes`).
/// [assumed] a world-space element's `Normal` stays in the world; settles: the reader of the
/// renderer's `Normal` in the fixed-axes builders (render node flag bit 0, `docs/particles.md`
/// "Drawing"), not found.
fn quads(sprites: &[Sprite], element: &Element, camera: &Transform, emitter: Option<&Affine3A>) -> Mesh {
    let renderer = &element.renderer;
    let (left, up, eye, forward) = (-*camera.right(), *camera.up(), camera.translation, *camera.forward());
    let (across, down) = (renderer.frames.0 as f32, renderer.frames.1 as f32);
    let [reach_left, reach_right, reach_top, reach_bottom] = element.extents();
    let mut positions = Vec::with_capacity(sprites.len() * 4);
    let mut uvs = Vec::with_capacity(sprites.len() * 4);
    let mut next_uvs = Vec::with_capacity(sprites.len() * 4);
    let mut colours = Vec::with_capacity(sprites.len() * 4);
    let mut normals = Vec::with_capacity(sprites.len() * 4);
    // A lit element's corners carry true normals, and the frame's fraction in a tangent.
    let lit = renderer.lit();
    // A distortion element's corners carry the particle's size in the colour's red and green.
    let warp = renderer.distortion;
    let mut tangents = Vec::new();
    let mut indices = Vec::with_capacity(sprites.len() * 12);
    for sprite in sprites {
        let pos = game_to_bevy(sprite.pos);
        let to_eye = (eye - pos).normalize_or_zero();
        // The picture's top and its left edge.
        let (top, side) = if let Some(along) = sprite.along {
            let along = game_to_bevy(along);
            (along, to_eye.cross(along).normalize_or(left))
        } else {
            let (l, u) = if renderer.rectangle() && renderer.normal != Vec3::ZERO {
                // A rectangle with a facing of its own lies in the plane across it.
                if let Some(emitter) = emitter {
                    let normal = renderer.normal.normalize();
                    let u = normal.any_orthonormal_vector();
                    let normal = game_to_bevy(emitter.transform_vector3(normal)).normalize();
                    let u = game_to_bevy(emitter.transform_vector3(u)).normalize();
                    (normal.cross(u), u)
                } else {
                    let normal = game_to_bevy(renderer.normal).normalize();
                    let u = normal.any_orthonormal_vector();
                    (normal.cross(u), u)
                }
            } else {
                (left, up)
            };
            let (sin, cos) = sprite.turn.sin_cos();
            (l * sin + u * cos, l * cos - u * sin)
        };
        let (side, top) = (side * sprite.half.0, top * sprite.half.1);
        let corners = [
            pos + top * reach_top + side * reach_left,
            pos - top * reach_bottom + side * reach_left,
            pos + top * reach_top - side * reach_right,
            pos - top * reach_bottom - side * reach_right,
        ];
        // The vertex shader's fade by the depth in front of the camera (`ALPHAFADE`).
        let mut colour = sprite.colour;
        let (near, far) = renderer.alpha_fade;
        // Fx's vertex shader applies this to each corner, as the original does.
        // Warp still uses the existing CPU fade until its vertex shader is ported.
        if warp && far != near {
            colour[3] *= (((pos - eye).dot(forward) - near) / (far - near)).clamp(0.0, 1.0);
        }
        // The frame's cell in the sheet, and the next frame's (the vertex shader's
        // arithmetic: the row is the frame over the sheet's width, whole).
        let cell = |frame: f32| {
            let row = (frame / across).floor();
            (frame - row * across, row)
        };
        let (this, next) = (cell(sprite.frame.floor()), cell(sprite.frame.ceil()));
        let fraction = sprite.frame.fract();
        let first = positions.len() as u32;
        for (corner, (u, v)) in corners.into_iter().zip([(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)]) {
            positions.push(corner.to_array());
            uvs.push([(this.0 + u) / across, (this.1 + v) / down]);
            next_uvs.push([(next.0 + u) / across, (next.1 + v) / down]);
            // A distortion corner's two numbers (`warp.wgsl`, the game's 731560_vs: n.x and n.y) travel in the
            // colour's red and green; the alpha stays in w. [assumed] both are the particle's radius. The lit
            // quad builder 00971170 writes four constant corner normals, not a radius (`warp.wgsl`, read
            // 2026-10-20); the Dstr renderer's own builder is not found, so this stays [assumed].
            if warp {
                let radius = sprite.half.0.max(sprite.half.1);
                colours.push([radius, radius, 0.0, colour[3]]);
            } else {
                colours.push(colour);
            }
            if warp {
                normals.push([fraction, 1.0 - fraction, 0.0]);
            } else if lit {
                // The lit builder (`FUN_00971170`) gives each corner a normal of its own.
                // [assumed: the way to the camera leant toward the corner, so the quad is lit like a ball] OPEN (read 2026-10-20: 00971170 has the camera vectors (aps_camera_vectors 00a9e0c0, or aps_fixed_axes 00a9e2c0), a per-corner loop with cos4_wide 00548390 and quat_sub 00543f10 for the corner frame; the normal stores are past the first 90 lines and the helpers 00543d60/00584ce0 are not read). settles: the normal stores in 00971170 after the corner loop start (`raw_disasm` on the corner loop), and the function that writes the normal into the vertex.
                normals.push((to_eye + (corner - pos).normalize_or_zero()).normalize_or(to_eye).to_array());
                tangents.push([1.0, 0.0, 0.0, fraction]);
            } else {
                normals.push([fraction, 1.0 - fraction, 0.0]);
            }
        }
        // Both ways round: it is seen from either side.
        indices.extend([first, first + 1, first + 2, first + 2, first + 1, first + 3]);
        indices.extend([first, first + 2, first + 1, first + 2, first + 3, first + 1]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, next_uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    if lit {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
    }
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object_nodes(owner: Entity, node: Entity, hash: u32) -> ObjectNodes {
        ObjectNodes {
            entities: vec![owner, node], frames: Vec::new(), names: vec![String::new(); 2], name_hashes: vec![0, hash],
            rest: vec![Transform::IDENTITY; 2], parents: vec![None, Some(0)],
            spine_links: Vec::new(), foot_rig: None,
        }
    }

    #[test]
    fn muzzle_effects_select_the_flash_node_or_each_firing_barrel() {
        let mut world = World::new();
        let root = world.spawn_empty().id();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        let flash = world.spawn_empty().id();
        let mut nodes = object_nodes(root, first, crc32(b"FireDummy"));
        nodes.entities.extend([second, flash]);
        nodes.name_hashes.extend([crc32(b"FireDummy01"), crc32(b"MuzzleFlashDummy")]);
        assert_eq!(muzzle_entity(&nodes, 0), Some(flash));
        assert_eq!(muzzle_entity(&nodes, 1), Some(flash));
        nodes.entities.pop();
        nodes.name_hashes.pop();
        assert_eq!(muzzle_entity(&nodes, 0), Some(first));
        assert_eq!(muzzle_entity(&nodes, 1), Some(second));
        assert_eq!(muzzle_entity(&nodes, 2), None);
    }

    #[test]
    fn installed_gun_smoke_drains_when_its_clip_is_interrupted() {
        if tf2_core::install::missing() { return; }
        let data = tf2_core::character::load(tf2_core::install::dir(), "bumblebee").unwrap();
        let mut world = World::new();
        let owner = world.spawn_empty().id();
        let node = world.spawn_empty().id();
        let mut checked = 0;
        for reference in &data.weapon_animations[0] {
            let Some(clip) = reference.clip.as_ref() else { continue };
            for event in &clip.effect_events {
                let EffectEventKind::Spawn { template, auto_stop, .. } = &event.kind else { continue };
                if template.name != crc32(b"w_overheat_smoke_small") { continue }
                assert!(!auto_stop, "installed smoke uses explicit stops, not autoStop");
                let nodes = object_nodes(owner, node, event.node);
                let mut effects = Effects::default();
                effects.thing_clip_events(clip, 0.0, event.time + 1.0, &nodes);
                for pending in std::mem::take(&mut effects.pending) {
                    let Pending::SpawnOn { owner, entity, id, template, turn, with_clip, .. } = pending else {
                        panic!("expected smoke spawn before its explicit stop");
                    };
                    assert!(!with_clip, "the game ends no autoStop=0 spawn on a clip change (007c2ea0)");
                    effects.start_on(&template, Anchor::Thing { entity, turn }, Affine3A::IDENTITY, owner, id, with_clip);
                }
                assert_eq!(effects.running.len(), 1);
                let dt = 0.032;
                for _ in 0..32 {
                    let r = &mut effects.running[0];
                    r.effect.step(&r.template, &r.place, dt, &mut effects.rng);
                }
                let r = &effects.running[0];
                assert!(r.effect.group(0).is_some_and(|g| !g.particles.is_empty()));
                assert!(!r.effect.finished(&r.template));
                // Cooling, switching or stowing leaves this clip before its stop event.
                effects.thing_clip_changed(&nodes);
                let Pending::ThingClipLeft { owner } = effects.pending.pop().unwrap() else { panic!("clip exit") };
                effects.stop_clip_effects(Some(owner));
                assert!(!effects.running[0].effect.stopping(), "no stop on a clip change: the game sends none");
                // The weapon's hide stops its emitters (weapon_hide 007a7750 -> 0057f600 -> 0057e000).
                effects.object_hidden(&nodes);
                assert!(effects.running[0].effect.stopping());
                assert!(!effects.running[0].effect.finished(&effects.running[0].template), "existing smoke lives out");
                for _ in 0..160 {
                    let r = &mut effects.running[0];
                    r.effect.step(&r.template, &r.place, dt, &mut effects.rng);
                }
                let r = &effects.running[0];
                assert!(r.effect.finished(&r.template), "smoke must finish without a timeout workaround");

                // An explicit stop needs the object's id, even if its channel is absent.
                let smoke_id = r.id;
                let stop = clip.effect_events.iter().find(|e| matches!(e.kind, EffectEventKind::Stop { id } if id == smoke_id)).unwrap();
                let missing_node = object_nodes(owner, node, event.node.wrapping_add(1));
                effects.thing_clip_events(clip, stop.time - 1.0, stop.time, &missing_node);
                assert!(matches!(effects.pending.as_slice(), [Pending::StopOn { owner: o, id }] if *o == owner && *id == smoke_id));
                effects.pending.clear();
                if reference.looping {
                    effects.thing_clip_events(clip, stop.time - 1.0, 1.0, &nodes);
                    assert!(effects.pending.iter().any(|e| matches!(e, Pending::StopOn { owner: o, id } if *o == owner && *id == smoke_id)), "a wrap must process the previous loop's stop key");
                }
                checked += 1;
            }
        }
        assert_eq!(checked, 4, "all installed gun smoke references are covered");
    }

    #[test]
    fn clip_stops_and_event_ids_are_scoped_to_their_object() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        let template = Template { elements: vec![Element { end: f32::MAX, ..Default::default() }], ..Default::default() };
        let mut effects = Effects::default();
        effects.start_on(&template, Anchor::Fixed, Affine3A::IDENTITY, first, 1, true);
        effects.start_on(&template, Anchor::Fixed, Affine3A::IDENTITY, second, 1, true);
        effects.start_on(&template, Anchor::Fixed, Affine3A::IDENTITY, first, 2, false);
        effects.start(&template, Anchor::Fixed, Affine3A::IDENTITY, 1, 0);
        effects.running[3].with_clip = true;
        assert!(effects.running.iter().all(|r| !r.effect.stopping()));
        effects.stop_clip_effects(None);
        assert_eq!(effects.running.iter().map(|r| r.effect.stopping()).collect::<Vec<_>>(), [false, false, false, true]);
        effects.stop_clip_effects(Some(first));
        assert_eq!(effects.running.iter().map(|r| r.effect.stopping()).collect::<Vec<_>>(), [true, false, false, true]);
        effects.stop_event(1, Some(second));
        assert!(effects.running[1].effect.stopping());
        assert!(!effects.running[2].effect.stopping(), "autoStop=false stays alive until its own stop");
    }

    /// Blackout's tail-rotor blur: a local `Rect` with `Normal` (0, 1, 0) lies across its
    /// emitter's y, wherever the emitter is turned; a world element keeps the world's y.
    #[test]
    fn a_local_rectangles_facing_turns_with_its_emitter() {
        let element = Element {
            local_space: true,
            renderer: tf2_core::particles::Renderer { id: u32::from_be_bytes(*b"Rect"), normal: Vec3::Y, ..Default::default() },
            ..Default::default()
        };
        let sprite = Sprite { pos: Vec3::new(3.0, 4.0, 5.0), half: (1.0, 2.0), turn: 0.7, along: None, colour: [1.0; 4], frame: 0.0 };
        let camera = Transform::from_xyz(0.0, 10.0, 30.0);
        // The emitter yawed 90 degrees and rolled 30 (the game's space).
        let emitter = Affine3A::from_rotation_translation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2) * Quat::from_rotation_y(0.5), sprite.pos);
        let facing = |emitter: Option<&Affine3A>| {
            let mesh = quads(&[sprite], &element, &camera, emitter);
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(corners)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
            let centre = game_to_bevy(sprite.pos);
            corners.iter().map(|c| Vec3::from(*c) - centre).collect::<Vec<_>>()
        };
        let turned = game_to_bevy(emitter.transform_vector3(Vec3::Y));
        for corner in facing(Some(&emitter)) {
            assert!(corner.length() > 0.5 && corner.dot(turned).abs() < 1e-4, "{corner} off the emitter's plane");
        }
        for corner in facing(None) {
            assert!(corner.dot(game_to_bevy(Vec3::Y)).abs() < 1e-4, "{corner} off the world's plane");
        }
    }

    /// A node's frame is this frame's transforms, not the `GlobalTransform` propagated the frame
    /// before (left stale here on purpose).
    #[test]
    fn a_node_frame_follows_this_frames_transforms() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let top = world.spawn((Transform::from_xyz(10.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(0.4)), GlobalTransform::IDENTITY)).id();
        let node = world.spawn((Transform::from_xyz(0.0, 2.0, 1.0).with_rotation(Quat::from_rotation_x(0.3)), GlobalTransform::IDENTITY, ChildOf(top))).id();
        let nodes = object_nodes(top, node, 7);
        let frame = world
            .run_system_once(move |t: Query<&mut Transform, Without<Camera3d>>, p: Query<&ChildOf>| node_frame_now(&nodes, 1, true, &t, &p))
            .unwrap()
            .unwrap();
        let expect = (Transform::from_xyz(10.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(0.4)).compute_affine()
            * Transform::from_xyz(0.0, 2.0, 1.0).with_rotation(Quat::from_rotation_x(0.3)).compute_affine())
        .to_scale_rotation_translation();
        assert!((Vec3::from(frame.translation) - bevy_to_game(expect.2)).length() < 1e-5);
        let turn = model_space_rotation().inverse() * expect.1;
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            assert!((frame.transform_vector3(axis) - turn * axis).length() < 1e-5);
        }
    }
}
