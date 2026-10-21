//! Bumblebee's movement, as a deterministic fixed-step routine with no engine types in it.
//!
//! Space is the game's: x right, y forward, z up, distances as the game's data gives them.
//! Every tuning number comes from the install (see `tuning.rs`). How those numbers are *applied*
//! is only partly read from the original so far, and each rule below says which it is:
//!
//!   [game]     behaviour read from the original's movement code (see notes/bumblebee-mechanics.md)
//!   [data]     follows directly from the character's own data (clips, rules, tuning)
//!   [assumed]  a plausible reading of a data value whose use has not been read yet
//!   [stand-in] invented so the test is playable; to be replaced
//!   [trace]    measured on the running game (see notes/live-trace.md)

use glam::{Affine3A, EulerRot, Quat, Vec2, Vec3};

use crate::animrules::{self, Base, Value};
use crate::climb::{self, Climb, Ledge, Wall};
use crate::control::{Facts, Pad, Runner, button, env};
use crate::formats::anim::Action as ClipAction;
use crate::formats::hash::crc32;
use crate::formats::scene::Shape;
use crate::melee::{self, Hit, Target};
use crate::rumble::{self, Rumble};
use crate::sound::{GearboxInput, TyreInput};
use crate::tuning::{ActionClip, JumpKind, Locomotion, Tuning};

/// Stick deflection (squared) below which the original treats the stick as centred. [game]
const STICK_DEAD_SQUARED: f32 = 0.01;
/// Speed (squared) under which leaving vehicle form gives the high standing jump. [game]
const VEHICLE_EXIT_SLOW_SQUARED: f32 = 3.0;
/// Out of FlyMode, JumpMode takes the long jump's type (with no launch) at this ground speed
/// squared or more, else the fall (`25.0 <= fVar13`, 008740e0 l.111). [game]
const FLY_EXIT_LONG_SQUARED: f32 = 25.0;
/// Stick deflection (squared) above which a running jump takes off at exactly the run speed. [game]
const FULL_STICK_SQUARED: f32 = 0.9801;
/// The standing jump's height as a multiple of the running jump's. [game]
const STANDING_JUMP_SCALE: f32 = 1.2;
/// The per-update air-control shares are applied this many times a second: the game's update
/// on PC is 0.032 s. [trace]
const CONTROL_FRAME_RATE: f32 = 31.25;
/// A ledge this low is walked up instead of blocking. [stand-in]
const STEP_HEIGHT: f32 = 0.6;
/// The brake counts as pressed above this. The brake is the digital bit 0x13 only (0 or 1,
/// [game 0085cad0 l.30-48]; the stick-back share was removed 2026-10-20), so any threshold in (0, 1)
/// reads it the same; 0.25 is the engine's axial dead zone [game 005aa890 / 005aa700].
const STICK_DEAD: f32 = 0.25;
/// The R2 bit (vehicle mode, flying mode, fire) is down once the trigger is touched at all: the
/// trigger filter (`005aa700` / `005aa890`) ORs 0x800 when RT > -0.999 on its -1..1 range, which
/// is `(t + 1) / 2 > 0.0005` on the 0..1 value `Input::vehicle` carries (raw byte 1 and up).
/// [game, `notes\input.md` "Filter triggers"]
const R2_DOWN: f32 = 0.0005;
/// The length of one of the game's updates on PC, in seconds. [trace]
pub const GAME_UPDATE: f32 = 0.032;
/// The vehicle body's mass and its resistance to turning about the vertical, read from the
/// running game's physics body (5417 and 16670 about the other two axes). [trace]
pub const VEHICLE_MASS: f32 = 5000.0;
/// The state machine's "on the ground" flag (owner `+0x26c`) trails the body: in run 1 every
/// landing stays in the jump mode for the update it touches down and the one after, and
/// leaves it on the second. So the flag the machine reads changes once the body's contact has
/// held for one of the game's updates. [trace]
const GROUND_FLAG_LAG: f32 = GAME_UPDATE - 1e-4;
/// The jump state may not be left until the jump mode has run this long (`FUN_0087e380`). [game]
const JUMP_LEAVE_TIME: f32 = 0.1;
/// MoveMode's share of the way to what the clips ask for, per update, while the ground flag is
/// clear (`FUN_00852220`); on the ground it is 1. [game]
const MOVE_AIR_CONTROL: f32 = 0.1;
/// The stick lines up with his motion for the car (`FUN_0087bcf0`): cosine of the angle. [game]
const DRIVE_LINE_UP: f32 = 0.9;
/// The unfold runs out to the run if the stick is held past this when it starts
/// (`FUN_00863670`). [game]
const UNFOLD_RUN_STICK: f32 = 0.05;
/// In the air the stick pushes the car this hard, and a body tipped further than this is
/// turned back toward level. [game]
const CAR_AIR_PUSH: f32 = 10.0;
/// The car rumbles the pad when it touches something faster than this (`FUN_00860870`),
/// and when it comes down later than this into the drive mode (`00b345ec`). [game]
const CAR_KNOCK_SPEED: f32 = 5.0;
const CAR_LANDING_AFTER: f32 = 0.1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JumpType {
    Standing,
    Moving,
    HighStanding,
    Long,
    /// Walked off an edge: airborne without a launch.
    Fall,
    /// Off a wall (type dfb7146d): straight up it or away from it.
    Climb,
}

/// The motion mode: what moves the body this update. The control state machine chooses it
/// (each state sets one when entered); the names are the original's mode classes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Standing (the unnamed first mode, class 1ad6ae68): the clips move him, nothing turns
    /// him. `stateIdle` sets it.
    Stand,
    /// MoveMode: turns him toward the stick; the clips move him.
    Move,
    /// StrafeMode (class 3c956e02), weapon mode on foot: he faces his aim and the weapon
    /// set's clips move him the way the stick points. `stateWeaponMode` sets it.
    Strafe,
    Jump(JumpType),
    Drive,
    /// FlyMode (class fb9f3262): the flying form, `flight::Fly`. The fly states set it.
    Fly,
    /// The change back out of the car (the drive companion mode, class f081d468): he slows
    /// at the exit rate and turns toward the stick at the exit's own turn speed.
    Unfold,
    /// HoverMode (class 70776cef): the robot held in the air, `step_hover`. The hover
    /// states (`StateBasic` with that motion mode) set it.
    Hover,
    /// On a wall (`notes\climbing.md`).
    Climb,
    /// One of the control states that move him by their clip: the attacks (the attack mode),
    /// the dodge (the dash mode), the ground punch's landing.
    Action,
}

/// What the stick asks of the robot on the ground; it picks the clip, and the clip sets the pace.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Gait {
    #[default]
    Idle,
    Walk,
    Run,
}

/// The clip that sets the pace on the ground. [trace]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Pace {
    #[default]
    Idle,
    Walk,
    Run,
    /// Run to idle.
    Stop,
    /// The landing that goes straight into a run, and the one after a long fall.
    LandRun,
    BigLandRun,
}

/// One clip in the cross-fade: how far it has faded in over what was there, and how long it
/// has played.
#[derive(Clone, Copy, Debug)]
struct Layer {
    pace: Pace,
    weight: f32,
    time: f32,
}

/// One clip of the weapon set in its cross-fade: how far it has faded in and where it is.
/// The bottom one can be what he was doing before instead: a control state's clip playing
/// on (`action`), or with `clip` 0 the velocity he had, in his own frame.
#[derive(Clone, Copy, Debug)]
struct RootLayer {
    clip: u32,
    action: bool,
    weight: f32,
    blend: f32,
    tick: f32,
    carry: Vec2,
    elapsed_ticks: f32,
    overrun: f32,
}

/// StrafeMode and the weapon set it drives through three animation variables.
#[derive(Clone, Debug, Default)]
pub struct Strafe {
    /// `F_STRAFE_ANG`, degrees, positive to the left: moving, the stick against the aim;
    /// standing, the aim against his body. [game]
    pub angle: f32,
    /// `I_IS_STRAFING`: set by the mode's enter, cleared by its exit. [game]
    pub strafing: bool,
    /// The aim heading (owner `+0x25c`) and how far his spine is twisted toward it while
    /// he stands (the mode's `+0x24`), radians. [game]
    pub aim_yaw: f32,
    pub twist: f32,
    /// The weapon set's clip that leads, by the CRC-32 of its id, and its tick.
    pub clip: u32,
    pub tick: f32,
    layers: Vec<RootLayer>,
    /// The mode has not had an update since its enter.
    entering: bool,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Input {
    /// Stick direction in world space, length up to 1.
    pub stick: Vec2,
    /// Stick left/right as the pad gives it, for steering the car.
    pub steer: f32,
    /// The stick as the pad gives it, x right and y up, for climbing (the input source's
    /// slot `+0x30`, which the car steers by too). [game]
    pub pad_stick: Vec2,
    /// The other stick as the pad gives it (the camera's): the helicopter turns and
    /// climbs by it. [game 0050b6b0] This is the pad's right thumb: `input_actions_to_xinput_state`
    /// writes the XInput right stick (state +0x0c / +0x0e) from the action values, and
    /// `game\src\player.rs` takes `pad.rx, -pad.ry`. No source slot `+0x28` is relied on.
    pub look: Vec2,
    /// Jump button went down this step.
    pub jump: bool,
    /// Jump button is down.
    pub jump_held: bool,
    /// Vehicle-mode trigger, 0 to 1. Held: car form and throttle. Released: back to robot.
    pub vehicle: f32,
    /// Brake, 0 to 1.
    pub brake: f32,
    /// Turbo button went down this step.
    pub turbo: bool,
    /// The slide (drift) button is down.
    pub drift: bool,
    /// Weapon mode: the direction he aims along the ground. `None` when not aiming.
    pub aim: Option<Vec2>,
    /// Buttons the fields above do not cover, by the game's numbers (`control::button`):
    /// down now, and went down this step. Melee, dodge and climb come this way.
    pub buttons_down: u64,
    pub buttons_hit: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Box3 {
    pub min: Vec3,
    pub max: Vec3,
}

/// What the movement needs to know about its surroundings. The Bevy test answers from a flat
/// arena of boxes and slopes; a host game answers from its own collision.
pub trait World {
    /// Vehicle queries use the world's actual ray/contact geometry, without walking steps.
    fn wheel_ray(&self, _from: Vec3, _to: Vec3) -> Option<crate::vehicle::GroundHit> { None }
    fn vehicle_sweep(&self, _centre: Vec3, _turn: Quat, _half: Vec3, _travel: Vec3) -> Option<crate::vehicle::Contact> { None }
    /// One step of the car's chassis cube (centre `centre`, turned `turn` now and `next` at the end, moving
    /// by `*velocity`) through the world's own contact pass: the end centre and the responded contacts, the
    /// velocity cut. None: the world has no pass and the chassis takes `vehicle_sweep` (`vehicle.rs`).
    /// `angular` (conventional, world axes) gives each contact the chassis point's velocity at its own hit.
    fn move_chassis(&self, _centre: Vec3, _turn: Quat, _next: Quat, _half: Vec3, _velocity: &mut Vec3, _angular: Vec3, _dt: f32)
        -> Option<(Vec3, Vec<crate::vehicle::Contact>)> { None }
    /// The driving car meets this world's walls through `move_chassis`, so the stand-in cylinder push is off.
    fn chassis_pass(&self) -> bool { false }
    /// Existing arena target separation, independent of the static vehicle solver.
    fn separate_targets(&self,_pos:&mut Vec3,_radius:f32,_height:f32) {}
    fn surface_contacts(&self, _body: &melee::Body) -> Vec<crate::ssd::Contact> { Vec::new() }
    /// Height of the surface to stand on at this spot, for feet at height `z`: the highest
    /// surface no more than a step above the feet.
    fn floor(&self, x: f32, y: f32, z: f32) -> f32;
    /// Optional continuous support along a grounded move, across ramps and their joins.
    fn follow_floor(&self, _from: Vec3, _to: Vec2) -> Option<f32> { None }
    /// Moves an upright cylinder out of anything it cannot step onto. Returns true if it hit.
    fn push_out(&self, pos: &mut Vec3, radius: f32, height: f32) -> bool;
    /// One on-foot step of the avatar's pencil from `from` to `*pos` against the walls; may move `*pos`
    /// and cut `*vel`. Returns true if a wall was hit. By default the stand-in `push_out`; a level answers
    /// with the game's contact pass (`LevelWorld::move_pencil`).
    fn move_pencil(&self, _from: Vec3, pos: &mut Vec3, _vel: &mut Vec3, radius: f32, height: f32) -> bool {
        self.push_out(pos, radius, height)
    }
    /// One world step of the robot's body on foot through the game's physics (`physics_avatar`): from
    /// `feet` with the velocity the mode left, `controlled` the mode's blended x and y. None: the world has
    /// no physics and the movement settles him itself (floor probes and `move_pencil`). The owner's ground test
    /// follows. The other characters' and cars' bodies (`bodies`) take part.
    fn step_avatar(&self, _body: &mut crate::physics_avatar::AvatarBody, _req: &crate::physics_avatar::AvatarRequest)
        -> Option<crate::physics_avatar::FeetStep> { None }
    /// The other characters' and cars' bodies as their snapshots at the start of this frame's steps.
    fn bodies(&self) -> &[crate::physics_avatar::BodySnapshot] { &[] }
    /// The car's chassis through the world step with its kept contacts (`physics_avatar::step_chassis`).
    /// None: the world has no physics (then `move_chassis`, then `vehicle_sweep`).
    fn step_chassis(&self, _body: &mut crate::physics_avatar::AvatarBody, _inp: &crate::physics_avatar::ChassisInput, _ai: bool)
        -> Option<(crate::physics_avatar::ChassisOutput, Vec<crate::vehicle::Contact>)> { None }
    /// The characters this world moves are AI ones (the enemies' world in `combat`).
    fn ai_bodies(&self) -> bool { false }
    /// A body contact within the forward cone, for the climb's wall pullback gate.
    /// Hosts without contact geometry answer false.
    fn front_contact(&self, _pos: Vec3, _facing: Vec2, _radius: f32, _height: f32) -> bool { false }
    /// For the climbable test: the first step up along `toward` (a unit vector) from `from`,
    /// within `reach`, to ground higher than `from.z`. The original answers it from its
    /// level's 2.5D ledge mesh; a world without one has nothing to climb.
    fn ledge(&self, _from: Vec3, _toward: Vec2, _reach: f32) -> Option<Ledge> {
        None
    }
    /// [assumed] Existing worlds query the player's character unless the host supplies AI control.
    /// Preserves the current player-only climb host; per-controller integration is still pending.
    fn climb_player_controlled(&self) -> bool { true }
    /// Carry the query seed through worlds that provide authored navigation.
    fn ledge_query(&self, from: Vec3, toward: Vec2, reach: f32,
        context: crate::level_world::ClimbQueryContext) -> crate::level_world::ClimbQueryResult {
        crate::level_world::ClimbQueryResult { ledge: self.ledge(from, toward, reach), context }
    }
    /// For the jump state's ledge catch: a ball of `radius` let straight down from `from` to
    /// height `to_z`, and where it first touches. The original sweeps its collision
    /// (`FUN_005be5a0`, mask 0x5d); a world that does not answer catches nothing.
    fn drop_onto(&self, _from: Vec3, _to_z: f32, _radius: f32) -> Option<Vec3> {
        None
    }
    /// What his attacks can go for and hit. A world with nothing to fight has none.
    fn targets(&self) -> &[Target] {
        &[]
    }
}

/// How high a surface can be above the feet and still be stepped onto.
pub const STEP: f32 = STEP_HEIGHT;

/// [stand-in] Flat ground at height zero with authored solid boxes and planar ramps.
#[derive(Clone, Default, Debug)]
pub struct Arena {
    pub boxes: Vec<Box3>,
    pub ramps: Vec<crate::terrain::Ramp>,
    /// Things to hit, each standing as an upright cylinder he cannot walk through.
    pub targets: Vec<Target>,
    /// One of the game's own levels (`level_world`). When set, it is the ground: the flat
    /// plane at height zero is not there, and the level answers beside the boxes and ramps.
    pub level: Option<std::sync::Arc<crate::level_world::LevelWorld>>,
    /// The characters' and cars' bodies on a level (`physics_avatar::BodySnapshot`), refreshed by the host each
    /// frame: they meet each other through the level's physics (`World::step_avatar`); a target with a body
    /// is no longer pushed out as a cylinder there.
    pub bodies: Vec<crate::physics_avatar::BodySnapshot>,
}

impl World for Arena {
    fn front_contact(&self, pos: Vec3, facing: Vec2, radius: f32, height: f32) -> bool {
        // [stand-in] The arena cylinder's closest box contact; real level contact
        // queries are delegated below, rather than inferred from a nav wall.
        self.boxes.iter().any(|b| {
            if b.max.z <= pos.z + STEP_HEIGHT || b.min.z >= pos.z + height {return false;}
            let near = pos.truncate().clamp(b.min.truncate(), b.max.truncate());
            let offset = near - pos.truncate();
            offset.length_squared() <= (radius + 0.01).powi(2)
                && climb::contact_ahead(offset.extend(0.0), facing)
        }) || self.level.as_ref().is_some_and(|level| level.front_contact(pos, facing, radius, height))
    }
    fn separate_targets(&self,pos:&mut Vec3,radius:f32,height:f32) {
        // [stand-in] Retain the existing target-cylinder separation. On a level a target with a physics body
        // meets him through the body pairs instead (`physics_avatar`), so it is skipped there.
        for target in &self.targets {
            if self.level.is_some() && self.bodies.iter().any(|b| b.id == target.id) {
                continue;
            }
            if target.pos.z >= pos.z + height || pos.z >= target.pos.z + target.height {continue;}
            let away=Vec2::new(pos.x-target.pos.x,pos.y-target.pos.y);
            let (distance,apart)=(away.length(),radius+target.radius);
            if distance<apart && distance>1e-4 {
                let push=away/distance*(apart-distance);
                pos.x+=push.x;pos.y+=push.y;
            }
        }
    }
    fn wheel_ray(&self, from: Vec3, to: Vec3) -> Option<crate::vehicle::GroundHit> {
        // The hit object's +0x9e word, where the level's soup is what was hit (the arena's own
        // floor, boxes and ramps have none: 0).
        let level_hit=self.level.as_ref().and_then(|level| level.ray_from_outside(from,to,
            crate::level_world::CHARACTER_MASK | crate::collision_masks::CAMERA).map(|h|(h.point,level.receive_mask(h.primitive))));
        crate::camera::Sight::sight(self,from,to).map(|hit|crate::vehicle::GroundHit {
            point:hit.point, normal:hit.normal, surface:melee::surface::DEFAULT,
            object_mask:level_hit.filter(|(p,_)| p.distance_squared(hit.point)<1e-6).map_or(0,|(_,m)|m),
        })
    }
    fn vehicle_sweep(&self, centre: Vec3, turn: Quat, half: Vec3, travel: Vec3) -> Option<crate::vehicle::Contact> {
        let mut found=self.ramps.iter().enumerate().filter_map(|(i,r)|r.vehicle_sweep(centre,turn,half,travel)
            .map(|hit|crate::vehicle::Contact {key:(self.boxes.len()+i+1) as u32,..hit})).collect::<Vec<_>>();
        for (i,b) in self.boxes.iter().enumerate() {
            let vertices=(0..8).map(|i|Vec3::new(if i&1==0 {b.min.x} else {b.max.x},
                if i&2==0 {b.min.y} else {b.max.y}, if i&4==0 {b.min.z} else {b.max.z})).collect::<Vec<_>>();
            if let Some(hit)=crate::terrain::sweep_cuboid(centre,turn,half,travel,&vertices,
                &[Vec3::X,Vec3::Y,Vec3::Z],&[Vec3::X,Vec3::Y,Vec3::Z]) {found.push(crate::vehicle::Contact {key:i as u32+1,..hit});}
        }
        let axes=[turn*Vec3::X,turn*Vec3::Y,turn*Vec3::Z];
        let bottom=centre.z-half.x*axes[0].z.abs()-half.y*axes[1].z.abs()-half.z*axes[2].z.abs();
        // On a level the chassis takes the contact pass instead (`move_chassis`).
        if self.level.is_none() && (bottom<0.0 || (travel.z<0.0 && bottom+travel.z<=0.0)) {
            let fraction=if bottom<0.0 {0.0} else {bottom/-travel.z};
            found.push(crate::vehicle::Contact {key:0,fraction,point:Vec3::new(centre.x,centre.y,0.0),normal:Vec3::Z,
                depth:(-bottom).max(0.0),surface:melee::surface::DEFAULT,velocity:Vec3::ZERO});
        }
        found.into_iter().min_by(|a,b|a.fraction.total_cmp(&b.fraction).then(b.depth.total_cmp(&a.depth)))
    }
    fn move_chassis(&self, centre: Vec3, turn: Quat, next: Quat, half: Vec3, velocity: &mut Vec3, angular: Vec3, dt: f32)
        -> Option<(Vec3, Vec<crate::vehicle::Contact>)> {
        // [game] on a level the chassis meets the level through the contact pass; the arena's own boxes
        // and ramps are not built on a level (`main.rs`), so they are not swept here.
        self.level.as_ref().map(|level| level.move_chassis(centre, turn, next, half, velocity, angular, dt))
    }
    fn chassis_pass(&self) -> bool { self.level.is_some() }
    fn surface_contacts(&self, body: &melee::Body) -> Vec<crate::ssd::Contact> {
        let mut contacts = crate::ssd::contacts(body, &self.boxes);
        for (i, ramp) in self.ramps.iter().enumerate() {
            if let Some((point, normal)) = ramp.contact(body) {
                contacts.push(crate::ssd::Contact { surface: self.boxes.len() + i + 1, point, normal });
            }
        }
        contacts
    }
    fn floor(&self, x: f32, y: f32, z: f32) -> f32 {
        Arena::floor(self, x, y, z)
    }

    fn push_out(&self, pos: &mut Vec3, radius: f32, height: f32) -> bool {
        Arena::push_out(self, pos, radius, height)
    }

    fn move_pencil(&self, from: Vec3, pos: &mut Vec3, vel: &mut Vec3, radius: f32, height: f32) -> bool {
        match &self.level {
            // [game] the level's walls through the contact pass; the arena's own boxes keep their push
            Some(level) => {
                let hit = self.push_out_local(pos, radius, height);
                level.move_pencil(from, pos, vel, radius, height) | hit
            }
            None => Arena::push_out(self, pos, radius, height),
        }
    }

    fn step_avatar(&self, body: &mut crate::physics_avatar::AvatarBody, req: &crate::physics_avatar::AvatarRequest)
        -> Option<crate::physics_avatar::FeetStep> {
        // [game] on a level the robot's body takes the level's physics, the other bodies with it; the arena's own
        // boxes and the targets without a body keep their push
        let level = self.level.as_ref()?;
        let others: Vec<_> = self.bodies.iter().filter(|b| b.id != req.id).copied().collect();
        let mut out = level.step_avatar(body, req, &others);
        self.push_out_local(&mut out.feet, req.radius, req.height);
        Some(out)
    }

    fn step_chassis(&self, body: &mut crate::physics_avatar::AvatarBody, inp: &crate::physics_avatar::ChassisInput, ai: bool)
        -> Option<(crate::physics_avatar::ChassisOutput, Vec<crate::vehicle::Contact>)> {
        let others: Vec<_> = self.bodies.iter().filter(|b| b.id != inp.id).copied().collect();
        self.level.as_ref().map(|level| level.step_chassis(body, inp, ai, &others))
    }

    fn bodies(&self) -> &[crate::physics_avatar::BodySnapshot] {
        &self.bodies
    }

    fn follow_floor(&self, from: Vec3, to: Vec2) -> Option<f32> {
        Arena::follow_floor(self, from, to)
    }

    fn ledge(&self, from: Vec3, toward: Vec2, reach: f32) -> Option<Ledge> {
        Arena::ledge(self, from, toward, reach)
    }

    fn ledge_query(&self, from: Vec3, toward: Vec2, reach: f32,
        context: crate::level_world::ClimbQueryContext) -> crate::level_world::ClimbQueryResult {
        if let Some(level) = &self.level {
            level.ledge_query(from, toward, reach, context)
        } else {
            crate::level_world::ClimbQueryResult { ledge: Arena::ledge(self, from, toward, reach), context }
        }
    }

    /// The highest top (or the ground) under the spot between the two heights. [stand-in:
    /// the ball's width is ignored, so it touches straight under its centre] settles: the game's drop query with a radius (its callers, not read).
    fn drop_onto(&self, from: Vec3, to_z: f32, _radius: f32) -> Option<Vec3> {
        let z = self
            .boxes
            .iter()
            .filter(|b| from.x >= b.min.x && from.x <= b.max.x && from.y >= b.min.y && from.y <= b.max.y)
            .map(|b| b.max.z)
            .chain(self.ramps.iter().filter(|r| r.contains(from.truncate())).map(|r| r.top(from.truncate())))
            .chain(match &self.level {
                Some(level) => World::drop_onto(&**level, from, to_z, _radius).map(|p| p.z),
                None => Some(0.0),
            })
            .filter(|&z| z <= from.z && z >= to_z)
            .fold(f32::MIN, f32::max);
        (z > f32::MIN).then(|| Vec3::new(from.x, from.y, z))
    }

    fn targets(&self) -> &[Target] {
        &self.targets
    }
}

impl Arena {
    /// Height of the surface to stand on at this spot, for feet at height `z`.
    pub fn floor(&self, x: f32, y: f32, z: f32) -> f32 {
        self.boxes
            .iter()
            .filter(|b| x >= b.min.x && x <= b.max.x && y >= b.min.y && y <= b.max.y && b.max.z <= z + STEP_HEIGHT)
            .map(|b| b.max.z)
            .chain(self.ramps.iter().filter(|r| r.contains(Vec2::new(x, y)))
                .map(|r| r.top(Vec2::new(x, y))).filter(|&top| top <= z + STEP_HEIGHT))
            .fold(self.level.as_ref().map_or(0.0, |level| World::floor(&**level, x, y, z)), f32::max)
    }

    /// [stand-in] Follow continuous arena support in substeps shorter than a step height.
    /// Only ramp traversals use this; a discontinuous drop still launches/falls normally.
    /// This prevents a fast car treating the rise in one frame as a vertical wall.
    pub fn follow_floor(&self, from: Vec3, to: Vec2) -> Option<f32> {
        let start = from.truncate();
        let low = start.min(to);
        let high = start.max(to);
        if !self.ramps.iter().any(|r| low.cmple(r.max).all() && high.cmpge(r.min).all())
            || (self.floor(from.x, from.y, from.z) - from.z).abs() > 0.01 {
            return None;
        }
        let steps = (start.distance(to) / (STEP_HEIGHT * 0.25)).ceil().max(1.0) as usize;
        let mut z = from.z;
        let mut on_ramp = false;
        for i in 1..=steps {
            let p = start.lerp(to, i as f32 / steps as f32);
            for r in self.ramps.iter().filter(|r| r.contains(p)) {
                if r.base < z + STEP_HEIGHT && r.top(p) > z + STEP_HEIGHT { return None; }
                on_ramp |= (r.top(p) - z).abs() <= STEP_HEIGHT;
            }
            let next = self.floor(p.x, p.y, z);
            if (next - z).abs() > STEP_HEIGHT { return None; }
            z = next;
        }
        on_ramp.then_some(z)
    }

    /// Moves an upright cylinder out of any box it cannot step onto. Returns true if it hit one.
    pub fn push_out(&self, pos: &mut Vec3, radius: f32, height: f32) -> bool {
        let mut hit = self.push_out_local(pos, radius, height);
        if let Some(level) = &self.level {
            hit |= World::push_out(&**level, pos, radius, height);
        }
        hit
    }

    /// `push_out` against the arena's own boxes, ramps and targets only (not the level).
    fn push_out_local(&self, pos: &mut Vec3, radius: f32, height: f32) -> bool {
        // Two bodies do not share a spot: he is moved back to touching. That is not a wall,
        // so it does not count as a hit. [stand-in for the collision between two characters]
        self.separate_targets(pos,radius,height);
        let mut hit = false;
        // [stand-in] The upright cylinder meets the ramp's vertical sides at their local
        // height. The planar top is handled by floor/follow_floor, not a bounding box.
        let ramp_sides = self.ramps.iter().filter_map(|r| {
            let p = pos.truncate().clamp(r.min, r.max);
            let top = r.top(p);
            (top > pos.z + STEP_HEIGHT && r.base < pos.z + height)
                .then_some(Box3 { min: r.min.extend(r.base), max: r.max.extend(top) })
        }).collect::<Vec<_>>();
        for b in self.boxes.iter().chain(&ramp_sides) {
            if b.max.z <= pos.z + STEP_HEIGHT || b.min.z >= pos.z + height {
                continue;
            }
            let nearest = Vec2::new(pos.x.clamp(b.min.x, b.max.x), pos.y.clamp(b.min.y, b.max.y));
            let away = Vec2::new(pos.x, pos.y) - nearest;
            let distance = away.length();
            if distance >= radius {
                continue;
            }
            hit = true;
            if distance > 1e-4 {
                let push = away / distance * (radius - distance);
                pos.x += push.x;
                pos.y += push.y;
            } else {
                // Centre is inside the box: leave by the nearest side.
                let sides = [
                    (pos.x - b.min.x, Vec2::NEG_X),
                    (b.max.x - pos.x, Vec2::X),
                    (pos.y - b.min.y, Vec2::NEG_Y),
                    (b.max.y - pos.y, Vec2::Y),
                ];
                let (depth, normal) = sides.into_iter().fold(sides[0], |a, s| if s.0 < a.0 { s } else { a });
                pos.x += normal.x * (depth + radius);
                pos.y += normal.y * (depth + radius);
            }
        }
        hit
    }

    /// The ledge mesh the original climbs by, made from the boxes: every box stands on the
    /// ground, so each of its sides is a step from the ground to its top. The first side the
    /// line crosses into ground above `from.z` is the wall; it runs on across the same side
    /// of other boxes in line with it and touching it. The original also lets a wall run past
    /// a corner that turns by less than 45 degrees; boxes have none. [stand-in: boxes for
    /// the ledge mesh; the rules are the original's, `notes\climbing.md`] settles: a level's ledge (the level answers, see above); the arena boxes stay a test stand-in.
    pub fn ledge(&self, from: Vec3, toward: Vec2, reach: f32) -> Option<Ledge> {
        // On a level the level answers, from its nav mesh (`level_world.rs`); the boxes are the test arena's.
        if let Some(level) = &self.level {
            return level.ledge(from, toward, reach);
        }
        let origin = Vec2::new(from.x, from.y);
        let inside = |b: &Box3, p: Vec2| p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y;
        let rises = |b: &Box3| b.max.z > from.z && b.min.z <= from.z;
        // Where the line enters each box, by the slab test, and through which side.
        let mut nearest: Option<(f32, Vec2, usize)> = None;
        for (i, b) in self.boxes.iter().enumerate() {
            if !rises(b) || inside(b, origin) {
                continue;
            }
            let (mut enter, mut leave, mut normal) = (0.0f32, reach, Vec2::ZERO);
            let mut missed = false;
            for (o, d, lo, hi, axis) in [(origin.x, toward.x, b.min.x, b.max.x, Vec2::X), (origin.y, toward.y, b.min.y, b.max.y, Vec2::Y)] {
                if d.abs() < 1e-6 {
                    missed |= o < lo || o > hi;
                    continue;
                }
                let (near, far, side) = if d > 0.0 { ((lo - o) / d, (hi - o) / d, -axis) } else { ((hi - o) / d, (lo - o) / d, axis) };
                if near > enter {
                    enter = near;
                    normal = side;
                }
                leave = leave.min(far);
            }
            if missed || enter > leave || normal == Vec2::ZERO || nearest.is_some_and(|(at, _, _)| at <= enter) {
                continue;
            }
            nearest = Some((enter, normal, i));
        }
        let (at, normal, first) = nearest?;
        let hit = origin + toward * at;
        let beyond = hit - normal * 0.01;
        let top = self.boxes.iter().filter(|b| inside(b, beyond) && b.min.z <= from.z).map(|b| b.max.z).fold(f32::MIN, f32::max);
        let near = hit + normal * 0.01;
        let bottom = self.floor(near.x, near.y, from.z - STEP_HEIGHT);
        // The side's line, and how far it runs along it (measured along the wall's x axis,
        // to his right as he faces it), joined across sides in line with it.
        let along = Vec2::new(-normal.y, normal.x);
        let side_of = |b: &Box3| -> (f32, f32, f32) {
            // (where the side is across the line, its two ends along it)
            let corner_lo = if normal.x != 0.0 { Vec2::new(if normal.x < 0.0 { b.min.x } else { b.max.x }, b.min.y) } else { Vec2::new(b.min.x, if normal.y < 0.0 { b.min.y } else { b.max.y }) };
            let corner_hi = if normal.x != 0.0 { Vec2::new(corner_lo.x, b.max.y) } else { Vec2::new(b.max.x, corner_lo.y) };
            let (a, c) = (corner_lo.dot(along), corner_hi.dot(along));
            (corner_lo.dot(normal), a.min(c), a.max(c))
        };
        let (plane, mut lo, mut hi) = side_of(&self.boxes[first]);
        loop {
            let mut grew = false;
            for b in self.boxes.iter().filter(|b| rises(b)) {
                let (p, a, c) = side_of(b);
                if (p - plane).abs() < 1e-3 && a <= hi + 1e-3 && c >= lo - 1e-3 && (a < lo - 1e-3 || c > hi + 1e-3) {
                    (lo, hi) = (lo.min(a), hi.max(c));
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        let s = hit.dot(along);
        Some(Ledge { hit, normal, bottom, top, height_adjustment: 0.0, left: s - lo, right: hi - s, left_corner: None, right_corner: None, extra: 0.0 })
    }
}

#[derive(Clone, Debug)]
pub struct State {
    pub pos: Vec3,
    pub vel: Vec3,
    /// Facing: 0 looks along +y, positive turns toward -x.
    pub yaw: f32,
    pub mode: Mode,
    pub gait: Gait,
    /// Forward speed along the facing while on the ground; negative when the car reverses.
    pub speed: f32,
    pub turbo_left: f32,
    pub turbo_cooldown: f32,
    /// Player's duration/cooldown upgrade levels;0 is the base tuning. [game]
    pub turbo_upgrades: [u8; 2],
    /// [game: 00732e60] Either timed turbo started during this step (sound event).
    pub turbo_started: bool,
    /// [game: 00733290/00733ae0] Last independent turbo FX on/off message. A timer
    /// keeps running after DriveMode exit, while this is switched off by that exit.
    pub turbo_fx_on: bool,
    /// Front wheel angle in radians, for drawing.
    pub steer_angle: f32,
    pub health: f32,
    pub since_damage: f32,
    pub damage_timers: crate::damage::Runtime,
    /// Receiving-side flags4/5, consumed by the host's weapon manager.
    pub heat_damage: Vec<(u32,f32)>,
    pub actor: crate::damage::Actor,
    pub difficulty: crate::damage::Difficulty,
    /// [game] Single-player death completion asks the host to restart the mission.
    pub restart_requested: bool,
    regen_delay: f32,
    /// Height of the last launch point and the highest point reached since, for the readout.
    pub launch_z: f32,
    pub apex_z: f32,
    /// The clips cross-fading on the ground, oldest first. Empty means standing.
    layers: Vec<Layer>,
    /// StrafeMode's own values and the weapon set's clips.
    pub strafe: Strafe,
    /// The clip the base layer's rules have picked while he stands, walks, jumps or falls
    /// (`animrules::Base`); empty while a control state plays a clip of its own or the weapon
    /// set plays.
    pub base: Base,
    /// `C_JUMP_TYPE`: what the jump mode last stored, which stays after it ends (the landing's
    /// entry rules read it). [game]
    jump_type: u32,
    /// Seconds spent going down since leaving the ground, and seconds since the launch.
    fall_time: f32,
    air_time: f32,
    /// Seconds in the jump mode (its `+0x2c`).
    jump_time: f32,
    /// The jump mode's slow motion while the weapon set plays: its time factor (`+0x28`, 1
    /// when there is none), the factor the body's gravity was last scaled by (`+0x24` over
    /// `+0x20`), and the height the slow motion began at (`+0x30`). [game]
    pub dilation: f32,
    gravity_scale: f32,
    dilation_from: Option<f32>,
    /// Whether the body touched the ground at the end of the last step, and for how long
    /// that has been so; the state machine's ground flag follows it late (`GROUND_FLAG_LAG`).
    touching: bool,
    touching_for: f32,
    /// The unfold started with the stick held: it runs out to the run (`+0x1c`). [game]
    unfold_run: bool,
    /// The car body's tilt, radians: nose up, and left side up.
    pub pitch: f32,
    pub roll: f32,
    /// Seconds left of the character's timer that a melee attack starts (his `+0x234`
    /// object's `+0x108`, 4.0). While it runs the camera is told to stand off
    /// (`FUN_0071a5c0`, the message's second flag byte, bit 0). The routine read to set it
    /// is the melee hit's (`FUN_007189b0`); the recording has the camera told from the
    /// update an attack state starts, hit or no hit, and that is what is done here. [trace]
    pub fight: f32,
    car: Car,
    /// The robot's physics body on a level: its holder's contacts, kept between steps (`physics_avatar`).
    pub avatar_body: crate::physics_avatar::AvatarBody,
    /// The car form's chassis holder: its contacts, kept between steps (`physics_avatar::step_chassis`).
    pub chassis_body: crate::physics_avatar::AvatarBody,
    /// The flying form while `mode` is `Fly`.
    pub fly: Option<crate::flight::Fly>,
    /// FlyMode's turbo and brake timers, kept between flights and run in every mode, as the
    /// game's FlyMode object (built once, 00863970) keeps them. [game] `flight::FlyTimers`.
    pub fly_timers: crate::flight::FlyTimers,
    /// FlyMode's moving parts (0086cbf0): the control surfaces' smoothed rates and the rotors,
    /// kept between flights as the FlyMode object keeps them. [game] `flightsurface`.
    pub flight_surfaces: crate::flightsurface::Surfaces,
    /// Seconds hovered since the hover began (its timer's `+0x18`).
    pub hover_time: f32,
    /// Seconds stunts stay armed after the stunt button's press, and seconds the stunt
    /// under way has run.
    stunt_armed: f32,
    stunt_time: f32,
    /// HoverMode's control vector (mode+0xc), from a clip's `ChangeTractionEvent` (00854960): on
    /// the axes it names the clip's root drives the body (`step_hover`); HoverMode's own update
    /// never reads it (0086f960). [game] FlyMode's is `flight::Fly::control`.
    hover_control: Vec3,
    /// The control state machine, the state it was in before this one (`+0x20`, which the
    /// fall, jump and weapon states look at) [game], and the clip the state is playing, if
    /// it plays one of its own.
    runner: Runner,
    previous_state: Option<usize>,
    pub action: Option<ActionState>,
    /// A state clip that ended in this update and was left: its leftover time in seconds and its
    /// root travel up to its end, for the clip the next state starts (`start_clip`). Lasts the
    /// update only.
    clip_handover: Option<(f32, Vec3)>,
    /// Seconds before another dodge may start, and the extra pull of a ground punch.
    dash_cooldown: f32,
    punch_gravity: f32,
    /// The climb mode, while he is on a wall.
    pub climb: Option<Climb>,
    /// The climb mode's top target and facing there (`+0x2c`, `+0x38`) as the last climb left
    /// them: the mode object outlives the climb, and the jump state reads them. [game]
    climb_top: Vec3,
    climb_top_facing: Vec2,
    /// Owner byte `+0x3f8`: set by the climb state's exit (`FUN_0087a820`) when Climb is
    /// still down, cleared by the body's step once it is up (`FUN_008524b0`, its pad test
    /// with button 0xb); while set, the climbable test fails (`FUN_0084cee0`). So a climb
    /// left with the button held is not taken up again until it is let go. [game]
    climb_regrab_lock: bool,
    /// What the climb mode told the state machine this update, for the next to act on.
    messages: Vec<u32>,
    /// The levels of the three melee upgrades (`melee::UPGRADES`); nothing raises them yet.
    pub melee_levels: [u32; 3],
    /// The hits that landed in the last step, for whoever owns the targets to act on.
    pub hits: Vec<Hit>,
    pub surface_hits: Vec<crate::ssd::Hit>,
    surface_hit_list: Vec<usize>,
    /// What the car's updates showed its gearbox in the last step, one entry an update, for
    /// whoever owns the engine's sound. [game]
    pub gearbox_inputs: Vec<GearboxInput>,
    pub vehicle_gearbox: Option<crate::sound::Gearbox>,
    /// The same for the tyres' and the suspension's sounds (`sound::tyre_sounds`). [game]
    pub tyre_inputs: Vec<TyreInput>,
    /// Vehicle convex contacts generated by this frame's physics, for CollisionFX.
    pub vehicle_contacts: Vec<crate::vehicle::Contact>,
    /// The targets the attack state he is in has hit already (owner `+0x228`): a target is
    /// hit once per attack state, and the list is emptied when the state is left
    /// (`FUN_00878be0`). [game]
    hit_list: Vec<u32>,
    /// His melee weapons (one per `t.actions.melee`): shown or not, the node each is linked
    /// to and its object's clip, following the weapon events of the clip he plays (`step_melee`).
    /// [game 007b4680, 007b2d80]
    pub melee: Vec<crate::gun::Axe>,
    /// The clip whose weapon events they last followed (key and tick), and whether he was in
    /// an attack state.
    melee_clip: Option<(u32, f32)>,
    melee_attacking: bool,
    /// The explosions his clips have set off that are still there.
    pub blasts: Vec<Blast>,
    /// The world places of his spawner nodes as last drawn (node name CRC-32, place), filled by
    /// whoever draws him before each step; empty without a drawn model. The special's spawners
    /// put their object at the node's world place when the Trigger arrives (ObjectSpawnerOnBone
    /// 007fc270: object_node_world_place 0056bee0 at 007fc31a, read 007fc325-007fc33a) [game, wave 28 P2].
    pub node_places: Vec<(u32, Vec3)>,
    /// The camera shakes and pad rumbles started in the last step, for whoever owns the
    /// camera and the pad.
    pub shakes: Vec<ShakeStart>,
    pub rumbles: Vec<Rumble>,
    /// The trigger messages his clip sent in the last step, by the `LuxScriptName` each is
    /// for: whoever draws starts the effects on his object that go by that name.
    pub triggers: Vec<u32>,
    /// The `Untrigger` messages (9caa723d) sent to his own things in the last step, by name: the
    /// stop of what a trigger started. [game 0072b980: when the special's duration runs out,
    /// Untrigger goes to BOTH self-trigger names, the walk's and the car's, whichever form he is
    /// in; the Lux dispatch `0080df70` turns it into the thing's Off, vslot 7] Whoever draws and
    /// plays stops the things of that name; the repair zone is switched off by its own clock.
    pub untriggers: Vec<u32>,
    /// The OptimusFlight special's mark (bit 29 of `+0x14c`, set by `00730e90` at the special's
    /// start, cleared at its end). [game] What it does (read 2026-10-18): every same-team
    /// character's damage update (`0072a550`, 0072ab8d) looks for a character with the mark
    /// (`00730f40`) other than itself and, within `optimusFlightSpecialDamageTransferRadius` (200,
    /// 00d40b50), turns its own bit 30 on (`007310b0`, `FX_heal_OptimusF_special`); a hit on a
    /// bit-30 character (`00719830`, 0071993e..007199d4) sends `optimusFlightSpecialDamageTransfer
    /// Multiplier` (0.75, 00d40b4c) of it to the marked one and keeps the rest
    /// (`damage::flight_transfer`). The marked one is never its own receiver. Only allies are
    /// covered, and the test arena has none, so nothing reads this yet.
    pub transfer_source: bool,
    /// The firing state (`stateWeaponModeAttack`, an overlay state) is running and its fire
    /// button was down at its last update: it told the weapon manager "trigger held"
    /// (`FUN_00878740` -> `FUN_007b37f0`). When this goes false the trigger was let go
    /// (`FUN_007b3a20`) or the state ended. [game]
    pub firing: bool,
    /// The special ability: seconds before it may be used again (his damage object's
    /// `+0x84`; the whole wait is its `+0x88`) and seconds it still counts as on
    /// (`+0x8c`, `specialAbilityDuration`). [game]
    pub special_cooldown: f32,
    pub special_left: f32,
    /// How many times the special has started (`start_special`): the game sees a new start by
    /// a change here, since a start may send no trigger (Megatron's has no script). [game]
    pub special_starts: u32,
    /// The type-coded special (types 0 to 3, `special_types::Active`) while it runs; None for
    /// the other specials and when none runs. Its timer is `step`ped in the update. [game 0072afd0]
    pub special: Option<crate::special_types::Active>,
    /// The clip-rate scale the special's `SpeedScale` steps set (the DComScout's 1.5 for its 5 s,
    /// then 1.0). [game 0071f4d0 -> 00748dc0] The game's scale sets every layer player's rate
    /// (+0x3c, +0x54, +0x5c) and the vehicle object's clips too. [game] Applied to every layer the
    /// port plays: the action clip (`play_action`), the base layer, the hit and fly partials
    /// (`layers_dt`), the weapon set (`weapon_layers`), the gun's arm and object players
    /// (`game\src\weapons.rs` `Arsenal::anim_rate`) and the vehicle model's clips
    /// (`game\src\animation.rs`).
    pub special_rate: f32,
    /// FlyMode's elapsed time in the mode (its `mode+0x4a4`): zeroed by the enter (00864d60 l.210),
    /// `+= dt` in the update (`step_fly`). When it reaches `advTransformDelay` the flight's
    /// can-advance-transform flag is up (`notes\flight-update.md` step 20, [game 00864320]).
    pub fly_clock: f32,
    /// The flight boundary of the level's zone, read at load (`level_things` / `flybound::Boundary::read`);
    /// None in the test arena; set only for a zone's level (`game\src\main.rs` start.flight_boundary). Used by
    /// `step_fly` while he flies. [data] [assumed] the placement it is read
    /// with is main.rs's `flight_boundary`. Read 2026-10-20: activation (007e75c0) inverts the owner
    /// node's world matrix (the registered handle at obj+0xc, rows at +0x10..+0x3c), which the port's
    /// placement does not do: it inverts each_thing world (bind composed). The frames agree only if that
    /// is the owner's world matrix. settles: the zone's caller of the factory 007e7780 / loader 011882b0
    /// (the owner handle it passes; not read) and the zone's flight boundary as the game tests it
    /// (`envTest` #f65c039d, notes/flight-boundary.md).
    pub flight_boundary: Option<crate::flybound::Boundary>,
    /// His control mode is the car's (character `+0x31c` is 3): from the enter of a drive
    /// state (`FUN_0087ba10`, so from the START of the change into the car) to the enter
    /// of a state whose `robotForm` puts him back on foot, or of a weapon state. The
    /// weapon manager takes the car's weapon in hand, and starts its wait, when this
    /// changes (`FUN_007b43b0`). [game]
    pub car_mode: bool,
    /// He has no health left; DeathSet's entry and exit rules run on the base layer. [game]
    pub death: Option<Death>,
    /// Animation damage kind supplied by the receiving side (`C_DAMAGE_TYPE`). [game]
    pub damage_type: u32,
    /// Character's special-idle countdown, initialized to 8 (00cad478). [game]
    pub idle_delay: f32,
    /// The additive layer above the weapon arm, started by stateHitReactOverlay. [game]
    pub hit_partial: Base,
    /// `V2RPartialSet`'s layer (layer 1), started by FlyMode's enter and exit where the
    /// character's `specialAbilityVehicleWeaponFireFromAnim` is set (`Tuning::
    /// vehicle_special_from_anim`): `Trans_R2V_Partial` going in, `Trans_V2R_Partial` coming out,
    /// each ending into `emptyAnimSet`. [game 00864d60, 00865b80] [data notes\animation-rules.md:254: the layer starts only when the flag is set, and no roster robot has both the flag and the set (MegatronFlight's flag is false, Blackout's set is missing), so nothing is drawn]
    pub fly_partial: Base,
    /// The clip events his layers have passed that act on his motion and layers, queued and
    /// carried out together once a step (`apply_clip_effects`). [game: a fired event is queued
    /// (0053ab90) and the queue is sent later in the frame (00544a00)]
    clip_effects: Vec<ClipAction>,
    /// The base layer's clip (serial) and tick when its events were last queued, and the weapon
    /// set's leading clip and tick likewise.
    base_seen: (u32, f32),
    weapon_seen: (u32, f32),
    /// Counts the weapon set's clip starts (`strafe_start`), for `weapon_seen`.
    weapon_serial: u32,
    /// `PlayCharPartialAnim`'s asks, (set, clip) by CRC-32, for the weapon arm's layer
    /// (`WeaponPartialSet`), which the host's gun plays (`gun::Gun::play_partial`). [game 007baa00]
    pub partial_requests: Vec<(u32, u32)>,
    /// The target the slam's finisher holds (`FinishMoveEvent`, `finish_move`), by id: the host's
    /// finisher camera frames him and it while this is set. [game 0072df10]
    pub finish_target: Option<u32>,
    hit_pending: Option<bool>,
    hit_full: bool,
    hit_flyback: bool,
    hit_start_z: f32,
    hit_last: Option<(u32, u32, f32)>,
    /// Damage object's +0x108, set to 4 by full hit-reaction enter. [game]
    hit_cooldown: f32,
    stun_pending: bool,
    stun_full: bool,
    /// The control state in force is a weapon state whose motion mode is HoverMode
    /// (`stateWeaponModeHover`): it runs on `Mode::Hover` with the weapon set over it
    /// (`weapon_hover`, `step_hover`). [game 008713c0, 0087f4a0; data global_states.txt 1094]
    hover_weapon: bool,
    hit_direction: Vec3,
    wall_splatted: bool,
}

/// The base layer's clip as the picture needs it: the set and clip by name, the tick, the
/// seconds it fades in over, and a count that goes up each time a clip starts.
#[derive(Clone, Copy, Debug)]
pub struct BaseClip<'a> {
    pub set: &'a str,
    pub id: &'a str,
    pub tick: f32,
    pub blend: f32,
    pub serial: u32,
}

/// The death clip playing: its id by CRC-32, its tick, where it started from and facing.
#[derive(Clone, Debug)]
pub struct Death {
    pub clip: u32,
    pub tick: f32,
    last_tick: f32,
    heading: f32,
    /// StateDeath's elapsed time, used by its 3s/10s completion clocks.
    pub age: f32,
    /// [game] BasicMotionMode enters with (1,1,1) for explosion deaths.
    pub root_z: bool,
}

/// A camera shake to start: the CRC-32 of its settings' name, and where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShakeStart {
    pub name: u32,
    pub at: Vec3,
}

/// An explosion in the world: the ground punch's blast. An `Explosion` on a spawned object
/// whose sphere grows and hits what touches it, each thing once (`FUN_007e5f00`,
/// `FUN_007e60f0`). [game]
#[derive(Clone, Debug)]
pub struct Blast {
    /// The clip that set it off and which of the clip's blasts it is.
    pub clip: u32,
    index: usize,
    /// Set off by a script and not by a clip (the special ability's shockwave): `index` is
    /// then into his own object's spawners (`Actions::spawners`), or with `true` the car's.
    script: Option<bool>,
    /// Seconds still to wait before it starts, while that is not negative; then seconds
    /// since it started.
    delay: f32,
    age: f32,
    /// Its middle, and its sphere's radius now; nothing touches it before it has started.
    pub centre: Vec3,
    pub radius: f32,
    pub live: bool,
    /// What it has hit already (its list at `+0xb8`).
    hit: Vec<u32>,
    surfaces: Vec<usize>,
}

/// A control state that plays a clip of its own: an attack, a dodge, the ground punch, the
/// halves of the change of form.
#[derive(Clone, Debug)]
pub struct ActionState {
    /// The control state (an index into `Tuning::control`) and its clip, by the CRC-32 of the
    /// clip's id; the clip's tick, and whether a non-looping clip has run out.
    pub state: usize,
    pub clip: u32,
    pub tick: f32,
    pub finished: bool,
    /// The tick the step before, for the root motion in between.
    last_tick: f32,
    /// Events of the clip already passed.
    events_done: usize,
    /// Branch points passed, counting from 1 as the original does. [game]
    branch: i32,
    /// The attack button window, and whether the attack button went down inside it; when the
    /// window shuts on a press, the next hit may chain. [game]
    window_open: bool,
    pressed_in_window: bool,
    can_branch: bool,
    /// What the attack mode may do now (`AttackFaceAndSlideEvent`). [game]
    slide: bool,
    face: bool,
    stick: bool,
    /// He is attacking (owner `+0xc8` is 1): set by an `AttackCollEvent` that switches a
    /// body on, taken off by one that switches any off (`FUN_0074af60`). [game]
    pub hitting: bool,
    /// The bodies switched on, by the CRC-32 of their node's name. [game]
    pub live: Vec<u32>,
    /// The trigger messages the clip has sent that nothing has acted on yet, by the
    /// `LuxScriptName` they are for.
    triggers: Vec<u32>,
    /// The attack mode's target (its `+0x20`): looked for once and then kept, until an
    /// attack that hit a character is over. And whether this attack has hit one (`+0x18`).
    /// [game]
    pub target: Option<u32>,
    has_hit: bool,
    /// The velocity there was when the clip started, faded out over the clip's blend time.
    carry: Vec2,
    fade: f32,
    fade_time: f32,
    /// The heading a dodge keeps, and the one a clip that turns its root turns from.
    heading: f32,
    /// Started on the wall (the two jumps off it).
    pub from_climb: bool,
    /// The punch out of the car (`stateDriveAttackAirPunch`, in the first mode): its clip
    /// lifts him 3.08 before `stateGroundPunchFall` takes over. The first mode's control lets the
    /// clip's height through [game 00856cc0, data; settled 2026-10-20]: the state's motionModeType is
    /// 1ad6ae68 (global_states.txt), BasicMotionMode's enter sets (1,1,1), and the clip
    /// Trans_Vehicle2Robot2GroundPunch keys [1,1,1] itself at time 0 (notes\clip-events-traction-motion.md
    /// section 2), which 00852220 hands the holder (the impulse case (0.1,0.1,0) only while knocked).
    /// Out of flight the same holds for Trans_V2R_GroundPunch (`begin_action`).
    pub lifted: bool,
    /// The clip's events that act on his motion and layers (`Action::is_motion`), passed and not
    /// yet carried out (`apply_clip_effects`).
    effects: Vec<ClipAction>,
    /// Ticks the clip ran past its end in the update it finished in; 0 in any other update.
    overshoot: f32,
    /// The root travel, in its own frame, of the clip this one took over from on the update that
    /// clip ended: that update's root motion (`ActionState::root_travel`). Cleared by the next
    /// update's `play_action`.
    carried: Vec3,
}

impl ActionState {
    /// The root's travel over this update in the clip's own frame: from the last tick to this one,
    /// plus the end of the clip it took over from, when that ended this update. A loop that came
    /// round counts for nothing.
    fn root_travel(&self, data: &ActionClip) -> Vec3 {
        let own = if self.tick >= self.last_tick { data.root_at(self.tick) - data.root_at(self.last_tick) } else { Vec3::ZERO };
        own + self.carried
    }
}

/// A car frame for presentation only; gameplay continues to use State's current pose.
#[derive(Clone, Copy, Debug)]
pub struct CarPresentation {
    pub pos: Vec3,
    pub rotation: Quat,
    pub velocity: Vec3,
    pub camera_velocity: Vec3,
    pub steer_angle: f32,
    pub wheels: [crate::vehicle::Wheel; 4],
}

impl CarPresentation {
    /// Yaw, pitch, roll in the same order as vehicle::rotation.
    pub fn angles(&self) -> (f32, f32, f32) { self.rotation.to_euler(EulerRot::ZXY) }

    fn interpolate(self, next: Self, alpha: f32) -> Self {
        let mut wheels=self.wheels;
        for (wheel,new) in wheels.iter_mut().zip(next.wheels) {
            wheel.render_spin=wheel.render_spin+(new.render_spin-wheel.render_spin)*alpha;
            // Contact changes are discrete. Keep the older contact through this interval;
            // interpolate the contact plane only while both frames have wheel support.
            if wheel.grounded && new.grounded {
                wheel.point=wheel.point.lerp(new.point,alpha);
                wheel.normal=wheel.normal.lerp(new.normal,alpha).normalize_or_zero();
            }
        }
        Self { pos:self.pos.lerp(next.pos,alpha), rotation:self.rotation.slerp(next.rotation,alpha),
            velocity:self.velocity.lerp(next.velocity,alpha),
            camera_velocity:self.camera_velocity.lerp(next.camera_velocity,alpha),
            steer_angle:self.steer_angle+(next.steer_angle-self.steer_angle)*alpha,wheels }
    }
}

/// What the car form keeps between updates.
#[derive(Clone, Copy, Default, Debug)]
struct Car {
    chassis: crate::vehicle::Chassis,
    /// Time not yet used up by whole updates of the car's rules.
    clock: f32,
    previous_presentation: Option<CarPresentation>,
    /// Turning rate about the vertical, radians a second, positive toward -x like `yaw`.
    yaw_rate: f32,
    /// How fast each wheel spins, radians a second: front left, front right, rear left, rear right.
    spin: [f32; 4],
    /// The speed being brought down while the brake is held, and after a turbo.
    braking_from: Option<f32>,
    after_turbo: Option<f32>,
    brake_held: f32,
    reversing: bool,
    /// The turbo button went down since the last update.
    turbo_asked: bool,
    /// The speed after the caps and before the wheels pushed.
    capped: f32,
    /// No wheel is on the ground.
    airborne: bool,
    /// [game] Drift turbo's charge/ready/active states, and DriveMode +0x204.
    drift_turbo: crate::driving::DriftTurbo,
    filtered_trigger: f32,
    sliding: bool,
    /// Time in the drive mode (its `+0x1fc`), counted in its updates. [game]
    time: f32,
}

impl State {
    /// The clip that leads the pace on the ground right now.
    pub fn pace(&self) -> Pace {
        self.layers.last().map_or(Pace::Idle, |layer| layer.pace)
    }

    pub fn new(tuning: &Tuning) -> Self {
        Self {
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            yaw: 0.0,
            mode: Mode::Stand,
            gait: Gait::Idle,
            speed: 0.0,
            turbo_left: 0.0,
            turbo_cooldown: 0.0,
            turbo_upgrades: [0; 2],
            turbo_started: false,
            turbo_fx_on: false,
            steer_angle: 0.0,
            health: tuning.base_health,
            since_damage: f32::MAX,
            damage_timers: crate::damage::Runtime::default(),
            heat_damage: Vec::new(),
            actor: crate::damage::Actor { id: 0, team: tuning.damage.team, player: true, character: crc32(tuning.name.as_bytes()), ..Default::default() },
            difficulty: tuning.damage.difficulties[1],
            restart_requested: false,
            regen_delay: tuning.health_regen_delay,
            launch_z: 0.0,
            apex_z: 0.0,
            layers: Vec::new(),
            strafe: Strafe::default(),
            base: Base::default(),
            jump_type: 0,
            fall_time: 0.0,
            air_time: 0.0,
            jump_time: 0.0,
            touching: true,
            touching_for: f32::MAX,
            unfold_run: false,
            pitch: 0.0,
            roll: 0.0,
            fight: 0.0,
            car: Car::default(),
            avatar_body: Default::default(),
            chassis_body: Default::default(),
            fly: None,
            fly_timers: Default::default(),
            flight_surfaces: Default::default(),
            hover_time: 0.0,
            stunt_armed: 0.0,
            stunt_time: 0.0,
            hover_control: Vec3::ZERO,
            runner: Runner::default(),
            previous_state: None,
            action: None,
            clip_handover: None,
            dash_cooldown: 0.0,
            punch_gravity: 0.0,
            dilation: 1.0,
            gravity_scale: 1.0,
            dilation_from: None,
            climb: None,
            climb_top: Vec3::ZERO,
            climb_top_facing: Vec2::ZERO,
            climb_regrab_lock: false,
            messages: Vec::new(),
            melee_levels: [0; 3],
            hits: Vec::new(),
            surface_hits: Vec::new(),
            surface_hit_list: Vec::new(),
            gearbox_inputs: Vec::new(),
            vehicle_gearbox: None,
            tyre_inputs: Vec::new(),
            vehicle_contacts: Vec::new(),
            hit_list: Vec::new(),
            melee: vec![crate::gun::Axe::default(); tuning.actions.melee.len()],
            melee_clip: None,
            melee_attacking: false,
            blasts: Vec::new(),
            node_places: Vec::new(),
            shakes: Vec::new(),
            rumbles: Vec::new(),
            triggers: Vec::new(),
            untriggers: Vec::new(),
            transfer_source: false,
            firing: false,
            special_cooldown: 0.0,
            special_left: 0.0,
            special_starts: 0,
            special: None,
            special_rate: 1.0,
            fly_clock: 0.0,
            flight_boundary: None,
            car_mode: false,
            death: None,
            damage_type: 0,
            idle_delay: 8.0,
            hit_partial: Base::default(),
            fly_partial: Base::default(),
            clip_effects: Vec::new(),
            base_seen: (0, 0.0),
            weapon_seen: (0, 0.0),
            weapon_serial: 0,
            partial_requests: Vec::new(),
            finish_target: None,
            hit_pending: None,
            hit_full: false,
            hit_flyback: false,
            hit_start_z: 0.0,
            hit_last: None,
            hit_cooldown: 0.0,
            stun_pending: false,
            stun_full: false,
            hover_weapon: false,
            hit_direction: Vec3::ZERO,
            wall_splatted: false,
        }
    }

    pub fn dead(&self) -> bool {
        self.death.is_some()
    }

    /// The clip a control state or the climb is playing: set, id and tick.
    pub fn action_clip<'a>(&self, t: &'a Tuning) -> Option<(&'a str, &'a str, f32)> {
        if let Some(death) = &self.death {
            let clip = t.actions.clips.get(&death.clip)?;
            return Some((&clip.set, &clip.id, death.tick));
        }
        if let Some(climb) = &self.climb {
            let clip = climb.clip_data(t)?;
            return Some((&clip.set, &clip.id, climb.tick));
        }
        if self.mode == Mode::Strafe {
            let clip = t.weapon_set.clip(self.strafe.clip)?;
            return Some(("WeaponSet", &clip.name, self.strafe.tick));
        }
        if self.weapon_air() || self.weapon_hover() {
            if let Some(clip) = t.weapon_set.clip(self.strafe.clip) {
                return Some(("WeaponSet", &clip.name, self.strafe.tick));
            }
        }
        let action = self.action.as_ref()?;
        let clip = t.actions.clips.get(&action.clip)?;
        Some((action_set_name(t, action.state, action.clip).unwrap_or(&clip.set), &clip.id, action.tick))
    }

    /// The clip the base layer's own rules picked, when no control state, climb, death or
    /// weapon clip is playing in its place: standing, walking, running, jumping, falling and
    /// the landings. [game: the sets' entry and exit rules, `animrules::Base`]
    pub fn base_clip<'a>(&self, t: &'a Tuning) -> Option<BaseClip<'a>> {
        if self.action_clip(t).is_some() {
            return None;
        }
        let clip = self.base.current(&t.rule_sets)?;
        let set = t.rule_sets.get(&self.base.set)?;
        Some(BaseClip { set: &set.set_name, id: &clip.name, tick: self.base.tick, blend: self.base.blend, serial: self.base.serial })
    }

    /// His collision bodies that can hit right now, where they are in the world: the ones
    /// the clip has switched on, at the clip's tick, each with the node the events name it by
    /// and whether it is on a foot. [game] Then the boxes of each melee weapon linked to a node
    /// the clip has switched on, on that node as the clip has it (`007b2e10`: every box of
    /// every melee slot whose node `+0x4c` is the event's), named by that node and never a
    /// foot. [game 00747e60, 007b2e10] [data] Shown or not: hiding is the render's alone
    /// (`005790a0` / `00579100`). [assumed: the hide messages' only listeners found are
    /// render-side (`fn.py callers 008034a0 / 0057f630`, partial)] settles: a full caller search of the hide setters `005790a0` / `00579100` for any listener that is not render-side. Which weapon a node switches
    /// is checked as it touches, not set as a bit at the event.
    /// [data: test `each_melee_weapon_links_to_one_node`: no roster pack links a melee weapon to two nodes, so the two agree]
    pub fn attack_bodies(&self, t: &Tuning) -> Vec<(u32, bool, melee::Body)> {
        let Some(action) = self.action.as_ref().filter(|a| a.hitting) else { return Vec::new() };
        let Some(data) = t.actions.clips.get(&action.clip) else { return Vec::new() };
        let placed = Affine3A::from_rotation_translation(Quat::from_rotation_z(self.yaw), self.pos);
        let mut out: Vec<(u32, bool, melee::Body)> = data
            .bodies
            .iter()
            .filter(|body| action.live.contains(&body.node))
            .filter_map(|body| body.at(action.tick).map(|frame| (body.node, body.foot, frame.moved(&placed))))
            .collect();
        for (blade, axe) in t.actions.melee.iter().zip(&self.melee) {
            let Some(link) = blade.link(axe.node, |node| t.actions.robot_nodes.contains(&node)) else { continue };
            if !action.live.contains(&link) {
                continue;
            }
            let Some(mount) = data.mounts.iter().find(|m| m.node == link).and_then(|m| m.at(action.tick)) else { continue };
            let on = placed * *mount;
            out.extend(blade.bodies(axe.object).iter().map(|b| (link, false, b.moved(&on))));
        }
        out
    }

    /// The clip whose weapon events his melee weapons follow, by its key, and its tick: the
    /// one `action_clip` names, when it is one of `t.actions` (the weapon set's clips carry
    /// none for a melee weapon [data: test `no_weapon_set_clip_moves_a_melee_weapon`]).
    fn melee_clip<'a>(&self, t: &'a Tuning) -> Option<(u32, &'a ActionClip, f32)> {
        if let Some(death) = &self.death {
            return t.actions.clips.get(&death.clip).map(|c| (death.clip, c, death.tick));
        }
        if let Some(climb) = &self.climb {
            return climb.clip_data(t).map(|c| (climb.clip, c, climb.tick));
        }
        if self.mode == Mode::Strafe || ((self.weapon_air() || self.weapon_hover()) && t.weapon_set.clip(self.strafe.clip).is_some()) {
            return None;
        }
        let action = self.action.as_ref()?;
        t.actions.clips.get(&action.clip).map(|c| (action.clip, c, action.tick))
    }

    /// The class of the control state he is in (`StateAttack`, `StateIdle`...).
    pub fn state_class<'a>(&self, t: &'a Tuning) -> Option<&'a str> {
        self.runner.state.and_then(|state| t.control.states.get(state)).map(|def| def.class.as_str())
    }

    /// Includes the two active overlay levels, as the battle chatter checks do.
    pub fn has_state_class(&self, t: &Tuning, class: &str) -> bool {
        [self.runner.state, self.runner.overlay, self.runner.nested].into_iter().flatten()
            .any(|state| t.control.states.get(state).is_some_and(|def| def.class == class))
    }

    /// In the air with the weapon set playing (the weapon states' jump and fall): the jump
    /// mode then keeps him facing his aim and slows time near the top. [game]
    pub fn weapon_air(&self) -> bool {
        matches!(self.mode, Mode::Jump(_)) && self.strafe.clip != 0
    }

    /// In the hover weapon state (`stateWeaponModeHover`): HoverMode holds him in the air and the
    /// weapon set plays over it. [game, data]
    pub fn weapon_hover(&self) -> bool {
        self.mode == Mode::Hover && self.hover_weapon
    }

    /// The fade the weapon set's leading clip came in with, while that set plays: its own
    /// blend time, or the one the clip before it asked for on its way out. [data]
    pub fn weapon_fade(&self) -> Option<f32> {
        let playing = self.mode == Mode::Strafe || self.weapon_air() || self.weapon_hover();
        self.strafe.layers.last().filter(|layer| playing && !layer.action && layer.clip == self.strafe.clip).map(|layer| layer.blend)
    }

    /// The name of the overlay state running on top of the control state, if any.
    pub fn overlay_state<'a>(&self, t: &'a Tuning) -> Option<&'a str> {
        self.runner.overlay.and_then(|state| t.control.states.get(state)).map(|state| state.name.as_str())
    }

    /// The name of the control state being run, for the readout.
    pub fn control_state<'a>(&self, t: &'a Tuning) -> Option<&'a str> {
        self.runner.state.and_then(|state| t.control.states.get(state)).map(|state| state.name.as_str())
    }

    /// Changing back to the robot after leaving the car.
    pub fn unfolding(&self) -> bool {
        self.mode == Mode::Unfold
    }

    /// On the ground as the state machine sees it: the body's contact, once it has held for
    /// an update. [trace]
    pub fn ground_flag(&self) -> bool {
        if self.touching_for >= GROUND_FLAG_LAG { self.touching } else { !self.touching }
    }

    /// The car is off the ground.
    pub fn car_airborne(&self) -> bool {
        self.mode == Mode::Drive && self.car.airborne
    }

    pub fn car_wheels(&self) -> &[crate::vehicle::Wheel; 4] { &self.car.chassis.wheels }

    fn car_snapshot(&self) -> CarPresentation {
        CarPresentation { pos:self.pos,rotation:crate::vehicle::rotation(self.yaw,self.pitch,self.roll),
            velocity:self.vel,camera_velocity:self.camera_velocity(),steer_angle:self.steer_angle,
            wheels:self.car.chassis.wheels }
    }

    /// [assumed] Interpolate the completed car frames, one 32ms physics step behind,
    /// for the body, wheel models and camera together. The original render scheduler is
    /// unread; this adapter removes fixed-step display jitter without changing physics.
    /// History is reset on drive entry/reset. See notes/status.md vehicle assumption 8.
    /// settles: a recording of the car at display rate (its body, wheels and camera against the frames shown).
    pub fn car_presentation(&self) -> CarPresentation {
        let current=self.car_snapshot();
        if self.mode!=Mode::Drive {return current;}
        self.car.previous_presentation.map_or(current,|previous|
            previous.interpolate(current,(self.car.clock/GAME_UPDATE).clamp(0.0,1.0)))
    }

    /// The car is sliding on its slide button.
    pub fn sliding(&self) -> bool {
        self.mode == Mode::Drive && self.car.sliding
    }

    pub fn facing(&self) -> Vec2 {
        Vec2::new(-self.yaw.sin(), self.yaw.cos())
    }

    /// His node's world place as last drawn (`node_places`), if the drawing gave it.
    pub fn node_place(&self, node: u32) -> Option<Vec3> {
        self.node_places.iter().find(|(n, _)| *n == node).map(|&(_, at)| at)
    }

    /// [game: 0071af5c] The character object's forward row, used for aim outside
    /// weapon mode. Roll does not change the nose direction; car pitch does.
    pub fn body_forward(&self) -> Vec3 {
        let pitch = if self.is_vehicle() { self.pitch } else { 0.0 };
        (self.facing() * pitch.cos()).extend(pitch.sin())
    }

    /// The velocity the camera is told about. In the car that is the speed after the caps
    /// (35, or 43 in turbo), not the body's, which runs a little past them: in the recording
    /// the driving camera's view only widens in turbo. [trace]
    pub fn camera_velocity(&self) -> Vec3 {
        if self.mode == Mode::Drive {
            let flat = Vec2::new(self.vel.x, self.vel.y).clamp_length_max(self.car.capped);
            Vec3::new(flat.x, flat.y, self.vel.z)
        } else {
            self.vel
        }
    }

    pub fn is_vehicle(&self) -> bool {
        matches!(self.mode, Mode::Drive | Mode::Fly)
    }

    /// His body as other bodies see it this frame (`physics_avatar::BodySnapshot`): the robot's pencil, or the
    /// car's chassis cube while driving; None in the flying form (not ported as a body). `ai` picks the
    /// holder masks. [game] the sizes and masses are the avatar descriptors' (`Tuning`).
    pub fn body_snapshot(&self, t: &Tuning, ai: bool) -> Option<crate::physics_avatar::BodySnapshot> {
        use crate::level_world::{AI_CHARACTER_MASK, CHARACTER_MASK};
        use crate::physics_avatar::{AvatarRequest, BodySnapshot, SnapshotShape};
        let masks = [if ai { AI_CHARACTER_MASK } else { CHARACTER_MASK }; 2];
        match self.mode {
            Mode::Fly | Mode::Hover => None,
            Mode::Drive => {
                let turn = crate::vehicle::rotation(self.yaw, self.pitch, self.roll);
                let inv = crate::vehicle::inertia(&t.drive).recip();
                let cols = [Vec3::X, Vec3::Y, Vec3::Z].map(|e| crate::vehicle::angular_acceleration(turn, e, inv));
                Some(BodySnapshot {
                    id: self.actor.id,
                    shape: SnapshotShape::Cube { half: t.drive.body_half },
                    centre: self.pos + turn * Vec3::Z * t.drive.body_offset_z,
                    rotation: turn,
                    velocity: self.vel,
                    angular: self.car.chassis.angular,
                    inv_mass: 1.0 / crate::vehicle::mass(&t.drive),
                    inv_inertia: [Vec3::new(cols[0].x, cols[1].x, cols[2].x), Vec3::new(cols[0].y, cols[1].y, cols[2].y),
                        Vec3::new(cols[0].z, cols[1].z, cols[2].z)],
                    masks,
                })
            }
            _ => Some(
                AvatarRequest {
                    feet: self.pos,
                    velocity: self.vel,
                    controlled: self.vel,
                    gravity: t.gravity,
                    radius: t.robot_radius,
                    height: t.robot_height,
                    mass: t.robot_mass,
                    facing: self.facing(),
                    in_move_mode: self.mode == Mode::Move,
                    ai,
                    id: self.actor.id,
                    dt: 0.0,
                }
                .snapshot(masks),
            ),
        }
    }

    pub fn is_flying(&self) -> bool {
        self.mode == Mode::Fly
    }

    /// Flying, as a character whose flying form is the robot itself (`modeFly.robot`, true for
    /// OptimusFlight and MegatronFlight only): the robot model stays drawn and plays `FlySet`'s
    /// `Fly_*` clips, whose channels carry the back thrusters and wings that are part of the
    /// robot object. Their packs have no vehicle object at all. [data]
    pub fn flies_as_robot(&self, t: &Tuning) -> bool {
        self.mode == Mode::Fly && t.forms.fly.robot
    }

    /// His type-6 special is running (Starscream's): type 6 and the special's timer (damage
    /// object `+0x8c`, `special_left`) above 0. [game 00721c10; the same test inline in
    /// bullet_damage 00782b30, missile_launch 00785de0, weapon_fire 007ae010] While it runs his
    /// rounds slow what they hit and his missiles do `starscreamSpecialDamageMultiplier` times
    /// the damage: the multiplier is the value.
    pub fn slowing_special(&self, t: &Tuning) -> Option<f32> {
        (t.special.kind == 6 && self.special_left > 0.0).then_some(t.damage.starscream_special_multiplier)
    }

    pub fn damage(&mut self, amount: f32) {
        if self.death.is_some() {
            return;
        }
        self.health = (self.health - amount).max(0.0);
        self.since_damage = 0.0;
        self.damage_timers.stop_regen(self.regen_delay);
    }

    /// A received hit, with its already-scaled damage, game damage flags and resolved
    /// impulse. Receiving-side scaling/collision stays with the caller. [game: reaction
    /// kind FUN_0072ddf0; full vs overlay FUN_00729120, single-player, no immunities]
    pub fn receive_hit(&mut self, amount: f32, flags: u32, impulse: Vec3) {
        if self.dead() { return; }
        self.damage(amount);
        self.hit_direction = impulse;
        self.damage_type = if self.health <= 0.0 && flags & melee::damage::BY_EXPLOSION != 0 { 0xf712_8ccc }
            else { animation_damage_kind(flags) };
        if flags & melee::damage::THROWS != 0 {
            self.vel = impulse;
        }
        let flail = flags & (melee::damage::BY_FAST_FLAIL | melee::damage::BY_STRONG_FLAIL) != 0;
        self.hit_pending = Some(flags & 0x17_0100 != 0 && (self.hit_cooldown <= 0.0 || flail));
    }

    /// Every gameplay hit uses the shared receiver. `receive_hit` is only the capture
    /// harness for already resolved damage/impulse. [game: 00719830 / 00729120]
    pub fn receive_damage(&mut self, packet: crate::damage::Packet, t: &Tuning) -> crate::damage::Resolved {
        use crate::damage::{self, flags};
        // The type-coded special's shield (types 0 and 1) [game 00719830, 00721b90, 00721bd0]: the
        // factor only in its own branch (ground for type 0, flight for type 1) and not for a hit
        // with the PREDAMAGE flag (bit 23). `resolve` clamps the health to 1.0 while it is passed.
        // [game 00719830 l.104-129] The branch is chosen by the ACTIVE special's type, not by the
        // form: `ground_shield_active` (type 0) is tested with no form test, and the flight block
        // runs on `flight_shield_active` (type 1). So the flight branch here is that special's kind.
        // The two blocks are separate `if`s in 00719830 (l.104-150) and both can run in one hit:
        // the ground block on type 0, then the flight block on type 1 OR damage bit 27 (`0x0800_0000`,
        // `flags::SPIRIT_LINK`), each multiplying the amount unless bit 23 is set. With only bit 27
        // and no type-1 special the flight block still multiplies, by the tuning
        // `optimusFlightSpecialDamageMultiplier` (getter 0079b630). Multiplayer is not read here.
        // Through `special_types::incoming_damage_flags` / `health_floor_flags`, which follow the
        // receiver: bit 28 skips both blocks, bit 23 exempts the multipliers but not the floor, and
        // the floor holds while either special runs or the hit has bit 27 (l.216-220). [game 00719830]
        // `resolve` takes the factor and applies the floor of 1 while it is passed.
        let active = self.special.as_ref();
        let factor = crate::special_types::incoming_damage_flags(active, &t.damage.special_multipliers, 1.0, packet.flags, false);
        let context_shield = crate::special_types::health_floor_flags(active, packet.flags).map(|_| factor);
        // The hit reactions and the heat damage are held back by a running shield special. [game 00729120]
        // 00729120 (called from 00719830 l.310 for every hit) runs its reaction block only when
        // `ground_shield_active` (00721b90) and `flight_shield_active` (00721bd0) are both false. Those
        // test the ability's type and duration only: bit 23 (PREDAMAGE) and bit 27 do not gate it.
        // FIXED: the earlier gate also skipped a PREDAMAGE hit, which the game does not do.
        let shield = active.filter(|a| a.is_on() && matches!(a.kind, crate::special_types::SpecialType::Optimus
            | crate::special_types::SpecialType::OptimusFlight)).map(|_| factor);
        let resolved = damage::resolve(packet, damage::Context { actor: self.actor, health: self.health,
            vehicle: self.is_vehicle(), shield: context_shield, enabled: !self.dead() && self.health > 0.0,
            reject: false, invincible: t.damage.invincible, controller_accepts: true, external_health: false,
            difficulty: self.difficulty, immune_to_overdrive:false, double_damage:false }, &t.damage);
        if !resolved.accepted { return resolved; }
        self.health = resolved.health.min(t.base_health);
        self.since_damage = 0.0;
        self.hit_direction = packet.direction;
        self.damage_timers.receive(packet, shield.is_some(), t.health_regen_delay, &t.damage);
        if shield.is_none() && packet.heat>0.0 && packet.flags & (flags::OVERHEAT_CURRENT|flags::OVERHEAT_ALL)!=0 {self.heat_damage.push((packet.flags,packet.heat));}
        if let Some(velocity) = resolved.velocity && !t.damage.immovable { self.vel = velocity; }
        self.damage_type = if self.health <= 0.0 { resolved.death_kind } else { animation_damage_kind(resolved.flags) };
        let immune = t.damage.reaction_immunity;
        if shield.is_none() && packet.flags & flags::NO_HIT_REACT == 0 {
            if packet.flags & flags::STUN != 0 && immune & flags::STUN == 0 && self.damage_timers.stunned > 0.0 {
                self.stun_pending = true;
            } else {
                let flail = packet.flags & (flags::BY_FAST_FLAIL | flags::BY_STRONG_FLAIL) != 0;
                self.hit_pending = Some(packet.flags & 0x17_0100 != 0 && immune & flags::BY_MELEE == 0 &&
                    !self.is_vehicle() && (self.hit_cooldown <= 0.0 || flail));
            }
        }
        resolved
    }
}

/// The executable's damage flags -> animation variable. [game: FUN_0072ddf0]
pub fn animation_damage_kind(flags: u32) -> u32 {
    if flags & (1 << 17) != 0 { 0xfea0_bd86 }
    else if flags & (1 << 16) != 0 { 0x6a58_53c5 }
    else if flags & (1 << 18) != 0 { 0xd475_6749 }
    else if flags & (1 << 20) != 0 { 0x37b0_d2e1 }
    else if flags & 0x8_0200 != 0 { 0xe968_e103 }
    else if flags & (1 << 8) != 0 { 0xd475_6749 }
    else { 0 }
}

fn yaw_of(direction: Vec2) -> f32 {
    (-direction.x).atan2(direction.y)
}

/// Turns `yaw` toward `target` by at most `max_step`, the short way round.
fn turn_toward(yaw: f32, target: f32, max_step: f32) -> f32 {
    let mut delta = (target - yaw) % std::f32::consts::TAU;
    if delta > std::f32::consts::PI {
        delta -= std::f32::consts::TAU;
    } else if delta < -std::f32::consts::PI {
        delta += std::f32::consts::TAU;
    }
    yaw + delta.clamp(-max_step, max_step)
}

/// Speed, control share and turn speed of a jump type. [game]
fn jump_kind(tuning: &Tuning, kind: JumpType) -> JumpKind {
    match kind {
        JumpType::Standing | JumpType::Fall => tuning.jump.standing,
        JumpType::Moving => tuning.jump.run,
        JumpType::HighStanding => tuning.jump.high_standing,
        JumpType::Long => tuning.jump.long,
        // Its own speed, the standing jump's turn, and no hold on it in the air. [game]
        JumpType::Climb => JumpKind {
            height: tuning.jump.climb_off_height,
            speed: tuning.jump.climb_off_speed,
            control: 0.0,
            turn_speed: tuning.jump.standing.turn_speed,
        },
    }
}

/// How high a jump type goes. The original does not use each type's own height: the running
/// jump uses the run height, the standing jump the run height and a fifth more, and both jumps
/// out of vehicle form the long jump's height. [game]
pub fn jump_height(tuning: &Tuning, kind: JumpType) -> f32 {
    match kind {
        JumpType::Standing | JumpType::Fall => tuning.jump.run.height * STANDING_JUMP_SCALE,
        JumpType::Moving => tuning.jump.run.height,
        JumpType::HighStanding | JumpType::Long => tuning.jump.long.height,
        // Every jump that starts in the climb set is this type, the one up the wall included
        // (its own height, `climbJumpUpHeight`, belongs to a type whose start is not read;
        // Bumblebee's two are both 15). [game]
        JumpType::Climb => tuning.jump.climb_off_height,
    }
}

pub fn step(s: &mut State, input: &Input, t: &Tuning, arena: &dyn World, dt: f32) {
    s.hits.clear();
    s.surface_hits.clear();
    s.shakes.clear();
    s.rumbles.clear();
    s.vehicle_contacts.clear();
    s.triggers.clear();
    s.untriggers.clear();
    s.turbo_started = false;
    s.fight = (s.fight - dt).max(0.0);
    advance(s, input, t, arena, dt);
    step_melee(s, t, dt);
    // The original's hits are contacts found by the physics step, which follows the modes.
    land_hits(s, t, arena);
    blast_hits(s, t, arena, dt);
    hit_rumbles(s, t);
}

/// Starts one of the pad's preset rumbles by its number (`FUN_00717c40` -> `FUN_007dc0a0`).
/// [game] He is always a local player's here, which is the original's condition.
fn preset_rumble(s: &mut State, t: &Tuning, number: usize) {
    if let Some(p) = t.rumble_presets.get(number) {
        s.rumbles.push(Rumble { kind: p.kind, duration: p.duration, strength: p.strength });
    }
}

/// The attacker's side of a hit that a target took (`FUN_00717d80`, called from the
/// target's damage routine): a hit whose flags have `DAMAGE_BY_MELEE` rumbles his pad, the
/// strong preset while the clip he is playing is the charge or the slam out of the car,
/// else the fast one. [game] The ground punch's blast carries the melee flag too, so each
/// target it reaches does the same, in place of the blast's own rumble that was running.
/// The blast's damage record names him as the attacker [game, read 2026-10-20]: 007e60f0 puts the live
/// handle of the explosion's +0x68 (weak_ref_lock at 007e6432-007e643c) into the record's attacker slot
/// (007e693b-007e6960); 007e5140 zeroes +0x68, and the explosion's owner routine 007e6af0 sets it, in its
/// character-owner branch (character_object_lookup 0070a3c0), to the owner character's handle
/// (handle_copy_into_esi 0074af20 at 007e6dd7-007e6ddd). (ObjectSpawnerOnBone 007fc270 only places
/// the object: a 12-float matrix to +0x88..+0xb4; its 0x3b83126f is the float 0.004 at 00b38894.)
/// [stand-in: the dummies take every hit] The game's refusals, read (wave 27 O12): 00719830 returns
/// before any damage when the receiver's flag word +0x14c bit 10 is clear, the hit is null, or bit 11 is
/// set; an invincible receiver (00724180) skips to 0071a260 unless the source is its own player.
/// settles: the scouts' flag word +0x14c (bits 10 and 11) and invincibility as the game builds them.
/// [data] His pack supplies neither soundTable field406e38ba nor0ae3b908, so the
/// optional attacker-hit sounds at+284/+288 are both zero (notes/melee.md).
fn hit_rumbles(s: &mut State, t: &Tuning) {
    let strong = s
        .action
        .as_ref()
        .is_some_and(|a| a.clip == crc32(b"AttackFast_3_ChargeAttk") || a.clip == crc32(b"Trans_Vehicle2RobotSlam"));
    for _ in 0..s.hits.iter().filter(|hit| hit.flags & melee::damage::BY_MELEE != 0).count() {
        preset_rumble(s, t, if strong { rumble::preset::STRONG_HIT_ATTACKER } else { rumble::preset::FAST_HIT_ATTACKER });
    }
}

/// The explosions his clips set off (`ClipBlast`): the ground punch's blast. Each update of
/// one (`FUN_007e5f00`): while its delay has not run out, that counts down and nothing
/// else happens; then its age goes up and its sphere is sized by it, and once the age was
/// past `explosion_time` both before and after the update it is gone. What its sphere
/// touches is hit, once each (`FUN_007e60f0`): the damage by the distance from its middle,
/// the blow flat from its middle to where the target stands, the throw its own. With
/// `attachToBone` it rides the node it was put at. [game] The melee upgrades and Optimus's
/// scalings are not applied to it: they sit in the fist's routine (`FUN_007189b0`). [game]
/// [game 007e5f00: the update the delay runs out clears the owner's bit 1, stamps the start time and adds no age, so the blast stays at `start_scale` until the next update.]
/// [assumed: the touch (007e60f0) runs on that update too, not read; a target standing at its very middle is thrown nowhere (the original takes the contact's normal there).] [stand-in: the touch,
/// and the target's side of it, as for the fists; once the clip that set it off is left
/// it stays where it was, where the original's stays on the hand] settles: the blast's contact routine `007e60f0` (its normal and its later frames, `l.113-145`), read in part only.
fn blast_hits(s: &mut State, t: &Tuning, arena: &dyn World, dt: f32) {
    let placed = Affine3A::from_rotation_translation(Quat::from_rotation_z(s.yaw), s.pos);
    let playing = s.action.as_ref().map(|a| (a.clip, a.tick));
    let node_places = s.node_places.clone();
    let mut blasts = std::mem::take(&mut s.blasts);
    blasts.retain_mut(|blast| {
        let data = match blast.script {
            None => t.actions.clips.get(&blast.clip).and_then(|clip| clip.blasts.get(blast.index)),
            Some(false) => t.actions.spawners.get(blast.index),
            Some(true) => t.actions.vehicle_spawners.get(blast.index),
        };
        let Some(data) = data else { return false };
        let def = &data.explosion;
        if blast.delay >= 0.0 {
            blast.delay -= dt;
            if blast.delay >= 0.0 {
                return true;
            }
            blast.live = true;
        } else {
            let before = blast.age;
            blast.age += dt;
            if blast.age > def.time && before > def.time {
                return false;
            }
        }
        blast.radius = def.radius * if blast.age > 0.0 { def.scale(blast.age, dt) } else { def.start_scale };
        if blast.script.is_some() {
            // A script's explosion rides his own node, wherever he goes. [game:
            // `attachToBone`]
            if data.attach {
                blast.centre = node_places.iter().find(|(node, _)| *node == data.node).map_or_else(|| placed.transform_point3(data.centre(0.0)), |&(_, at)| at);
            }
        } else if let Some((_, tick)) = playing.filter(|&(clip, _)| data.attach && clip == blast.clip) {
            blast.centre = placed.transform_point3(data.centre(tick));
        }
        let sphere = melee::Body { shape: Shape::Sphere { radius: blast.radius }, at: Affine3A::from_translation(blast.centre) };
        if !data.ssd.is_empty() {
            for contact in arena.surface_contacts(&sphere) {
                if !blast.surfaces.contains(&contact.surface) {
                    blast.surfaces.push(contact.surface);
                    s.surface_hits.push(crate::ssd::Hit { contact, infos:Some(data.ssd.clone()) });
                }
            }
        }
        let (damage, min_damage) = def.owned_damage(t.ground_pound_damage, t.ground_pound_damage_weak);
        for target in arena.targets() {
            if blast.hit.contains(&target.id) {
                continue;
            }
            let Some(point) = melee::touch(&sphere, &melee::Body::of_target(target)) else { continue };
            s.hits.push(Hit {
                target: target.id,
                clip: blast.clip,
                damage: def.damage_at(damage, min_damage, point.distance(blast.centre), blast.radius),
                point,
                body: data.node,
                direction: Vec2::new(target.pos.x - blast.centre.x, target.pos.y - blast.centre.y).normalize_or_zero(),
                knock_back_speed: def.knock_back_speed,
                knock_back_angle: def.knock_back_angle.to_radians(),
                flags: def.damage_flags,
                surface: target.surface,
                blast: true,
                stun_time: def.owned_stun(t.name_hash, t.special.duration),
            });
            blast.hit.push(target.id);
        }
        true
    });
    // The triggers the clip sent this update: each spawner by that name puts its object at
    // its node (`FUN_007fc270`). [game]
    if let Some(action) = s.action.as_mut() {
        let clip = action.clip;
        for name in action.triggers.drain(..) {
            s.triggers.push(name);
            let Some(data) = t.actions.clips.get(&clip) else { continue };
            for (index, blast) in data.blasts.iter().enumerate().filter(|(_, b)| b.name == name) {
                let centre = placed.transform_point3(blast.centre(action.tick));
                // The explosion starts its camera shake where it is (`FUN_007e5d30`), and
                // the object's `TriggerOnSpawn` its rumble, by each player's distance from
                // the object (`FUN_007dbdc0`). [game] The player's place there is his camera target
                // point (`0071a2e0`), which on foot is his root: where he stands.
                // [assumed: the rumble runs as the object is put there, not later; the caller of
                // `007dbdc0` is not read, and his clips' explosion delays are not checked]
                if blast.explosion.camera_shake != 0 {
                    s.shakes.push(ShakeStart { name: blast.explosion.camera_shake, at: centre });
                }
                if let Some(rumble) = &blast.rumble
                    && let Some(strength) = rumble.strength_at(centre.distance(s.pos))
                {
                    s.rumbles.push(Rumble { kind: rumble.kind, duration: rumble.duration, strength });
                }
                blasts.push(Blast {
                    clip,
                    index,
                    script: None,
                    delay: blast.explosion.delay,
                    age: 0.0,
                    centre,
                    radius: blast.explosion.radius * blast.explosion.start_scale,
                    live: false,
                    hit: Vec::new(),
                    surfaces: Vec::new(),
                });
            }
        }
    }
    s.blasts = blasts;
}

/// While he is attacking, each target one of his switched-on bodies touches is hit, once
/// per attack state (`FUN_00719540` -> `FUN_007189b0`): the damage is the clip's, the blow
/// goes the way its `attack_dir` says and carries its throw. The bodies are his own, where
/// the clip has them at this tick; a foot's touch counts only against a character. [game]
/// The damage is scaled by the melee upgrade the clip belongs to (`FUN_0079d060`). [game]
/// [stand-in: the target's side of the touch, one upright cylinder] The other scalings are
/// not his: Optimus's special ability (`FUN_0079b680`, `FUN_0079b6d0`) and the difficulty's
/// `scaleEnemyDamage` for attackers no player runs. The hit carries the clip's damage type
/// flags for the target's size and the surface it struck, by which whoever owns the sound
/// picks the hit's sound from his impact preset (`FUN_0072d9c0`). [game]
fn land_hits(s: &mut State, t: &Tuning, arena: &dyn World) {
    let Some(action) = s.action.as_ref().filter(|a| a.hitting) else { return };
    let Some(data) = t.actions.clips.get(&action.clip) else { return };
    let (clip, facing) = (action.clip, s.facing());
    let bodies = s.attack_bodies(t);
    // [game] Non-character surfaces count for hand bodies; feet count only on characters.
    // [stand-in] The host supplies arena contacts rather than the original level bodies.
    for (_, _, body) in bodies.iter().filter(|(_, foot, _)| !foot) {
        for contact in arena.surface_contacts(body) {
            if !s.surface_hit_list.contains(&contact.surface) {
                s.surface_hit_list.push(contact.surface);
                s.surface_hits.push(crate::ssd::Hit { contact, infos:None });
            }
        }
    }
    let Some(attack) = data.attack else { return };
    for target in arena.targets() {
        if s.hit_list.contains(&target.id) {
            continue;
        }
        let theirs = melee::Body::of_target(target);
        let touched = bodies
            .iter()
            .filter(|(_, foot, _)| target.character || !foot)
            .find_map(|(node, _, mine)| melee::touch(mine, &theirs).map(|point| (*node, point)));
        let Some((body, point)) = touched else { continue };
        s.hits.push(Hit {
            target: target.id,
            clip,
            // He is a player's, and this is not multiplayer.
            // The owner's special scales its melee (types 0 and 1, [game 0079b680, 0079b6d0]).
            damage: attack.damage_for(s.actor.player, false) * if s.actor.player { t.melee.upgrade_scale(clip, s.melee_levels) * s.special.as_ref().map_or(1.0, |a| crate::special_types::melee_multiplier(a, false)) } else { s.difficulty.enemy_damage },
            point,
            body,
            direction: melee::attack_direction(attack.attack_dir, facing, s.pos, target.pos),
            knock_back_speed: attack.knock_back_speed,
            knock_back_angle: attack.knock_back_angle.to_radians(),
            flags: attack.flags_for(target.character.then_some(target.large as u8)),
            surface: target.surface,
            blast: false,
            stun_time: 0.0,
        });
        s.hit_list.push(target.id);
    }
}

/// One update of his melee weapons, each by itself (`007b4680` gives an event to the slot of
/// its name): the weapon events of the clip he plays since the last update (from its start
/// when the clip is new), the put-away of every one when he leaves an attack state for a
/// state that is not one (`00878be0` -> `007b2d80` -> slot `+0x28`, `007a67b0`), and each
/// object's clip moved on. [game] Run after the modes and before the hits, so the hit test
/// sees this update's links and blade poses. Its `attackRanged` flag skips the put-away for
/// firing overlays (as the game crate's axe did before this moved here).
fn step_melee(s: &mut State, t: &Tuning, dt: f32) {
    if s.melee.len() != t.actions.melee.len() {
        s.melee.resize(t.actions.melee.len(), crate::gun::Axe::default());
    }
    if t.actions.melee.is_empty() {
        return;
    }
    let playing = s.melee_clip(t);
    if let Some((key, data, tick)) = playing {
        let from = s.melee_clip.filter(|&(last, at)| last == key && at <= tick).map(|(_, at)| at);
        let window = |e: &&crate::formats::anim::WeaponEvent| e.time <= tick && from.is_none_or(|from| e.time > from);
        if data.weapon_events.iter().any(|e| window(&e)) {
            let clip = crate::formats::anim::Clip { weapon_events: data.weapon_events.clone(), ..Default::default() };
            for (axe, blade) in s.melee.iter_mut().zip(&t.actions.melee) {
                axe.events(&clip, from, tick, blade.name);
            }
        }
    }
    s.melee_clip = playing.map(|(key, _, tick)| (key, tick));
    let attacking = s.state_class(t) == Some("StateAttack");
    if std::mem::replace(&mut s.melee_attacking, attacking) && !attacking {
        for (axe, blade) in s.melee.iter_mut().zip(&t.actions.melee) {
            axe.put_away(blade.exit_clip);
        }
    }
    for (axe, blade) in s.melee.iter_mut().zip(&t.actions.melee) {
        axe.step(&blade.clips.0, dt);
    }
}

/// StateDeath's enter: BasicMotionMode with XY control (except explosion deaths), then
/// DeathSet's entry walk. Form 3 is the control mode, including the transform's start.
/// [game: FUN_0087b070 / FUN_0070cae0]
fn die(s: &mut State, input: &Input, t: &Tuning) {
    let from = playing_clip(s, t);
    let fallback = crc32(if s.car_mode { b"Death_V2R".as_slice() } else { b"Death".as_slice() });
    s.base = Base { serial: s.base.serial.wrapping_add(1), ..Base::default() };
    base_enter(s, input, t, crc32(b"DeathSet"), from);
    let clip = s.base.playing().map_or(fallback, |(_, id)| id);
    s.death = Some(Death { clip, tick: s.base.tick, last_tick: s.base.tick, heading: s.yaw, age: 0.0, root_z: s.damage_type == 0xf712_8ccc });
    (s.mode, s.action, s.climb, s.runner.state, s.runner.overlay) = (Mode::Stand, None, None, None, None);
    (s.firing, s.speed, s.turbo_left, s.dilation, s.gravity_scale) = (false, 0.0, 0.0, 1.0, 1.0);
    s.turbo_fx_on = false;
    s.layers.clear();
    s.strafe = Strafe::default();
    s.hit_partial = Base::default();
    s.hit_pending = None;
    s.hit_full = false;
    s.stun_full = false;
    s.stun_pending = false;
    if let Some(index) = t.control.states.iter().position(|def| def.class == "StateDeath") {
        s.runner.enter(&t.control, index);
    }
}

/// [game] Ordinary death controls XY root motion, explosion death controls XYZ.
/// BasicMotionMode enter 00856cc0 and StateDeath enter 0087b070.
fn step_death(s: &mut State, input: &Input, t: &Tuning, arena: &dyn World, dt: f32) {
    let old = s.base.playing();
    let before = s.base.tick;
    let motion_scale=if s.damage_timers.slowed>0.0 {t.damage.slow_multiplier} else {1.0};
    base_step(s, input, t, dt*motion_scale*s.special_rate);
    let Some(death) = s.death.as_mut() else { return };
    death.age += dt;
    if let Some((_, clip)) = s.base.playing() {
        death.clip = clip;
        death.last_tick = if old == s.base.playing() { before } else { 0.0 };
        death.tick = s.base.tick;
    }
    let Some(data) = t.actions.clips.get(&death.clip) else { return };
    if s.base.playing().is_none() {
        death.last_tick = death.tick;
        death.tick = (death.tick + dt * 60.0 * data.speed).min(data.ticks);
    }
    let moved = if dt > 1e-6 { (data.root_at(death.tick) - data.root_at(death.last_tick)) / dt } else { Vec3::ZERO };
    let facing = Vec2::new(-death.heading.sin(), death.heading.cos());
    let right = Vec2::new(facing.y, -facing.x);
    let flat = right * moved.x + facing * moved.y;
    (s.vel.x, s.vel.y) = (flat.x, flat.y);
    s.pos.x += s.vel.x * dt;
    s.pos.y += s.vel.y * dt;
    let floor = arena.floor(s.pos.x, s.pos.y, s.pos.z);
    if death.root_z {
        s.vel.z = moved.z;
        s.pos.z = (s.pos.z + s.vel.z * dt).max(floor);
    } else if s.pos.z > floor + 0.01 {
        s.vel.z -= t.gravity * dt;
        s.pos.z = (s.pos.z + s.vel.z * dt).max(floor);
    } else {
        s.pos.z = floor;
        s.vel.z = 0.0;
    }
    let finished = data.ticks <= death.tick && !data.looping;
    let age = death.age;
    arena.push_out(&mut s.pos, t.robot_radius, t.robot_height);
    touch(s, s.pos.z <= floor + 0.01, dt);
    s.restart_requested = crate::damage::death_finished(age, s.pos.z <= floor + 0.01, finished, false, false);
}

fn advance(s: &mut State, input: &Input, t: &Tuning, arena: &dyn World, dt: f32) {
    // The clip events his layers passed in the last update are carried out first: the game's
    // characters update at update-list priority 5 (0071e930 walks them, slot 2 = 0071dc20: the
    // state machine, the rules, the body step), the animation controllers advance and send their
    // queued events afterwards at priority 11 (AnimCtrl::UpdateAll 00544b60 -> 00544a00), so an
    // event acts on the body from the next update. [game 005a8740, 00ad7670, 00ad5190]
    apply_clip_effects(s, input, t, arena);
    // Character update lowers this until negative; it is not reset by locomotion or damage.
    // [game: FUN_00713300 / FUN_0071dc20; the idle's own rules also require 8s unharmed]
    if s.idle_delay >= 0.0 { s.idle_delay -= dt; }
    s.hit_cooldown = (s.hit_cooldown - dt).max(0.0);
    s.since_damage += dt;
    if let Some(packet) = s.damage_timers.step(dt) { s.receive_damage(packet, t); }
    s.damage_timers.regenerate(&mut s.health, t.base_health, t.health_regen_rate > 0.0,
        t.health_regen_delay, &t.damage, dt);
    if s.health <= 0.0 && s.death.is_none() {
        die(s, input, t);
    }
    if s.death.is_some() {
        step_death(s, input, t, arena, dt);
        return;
    }
    // [game: 00733180/0085ed50] The standard turbo stays registered on the character
    // when DriveMode exits. Its FX stop, but its active/cooldown timers keep running.
    if crate::driving::step_turbo(&mut s.turbo_left, &mut s.turbo_cooldown,
        crate::driving::turbo_times(&t.drive, s.turbo_upgrades).1, dt) {
        s.car.after_turbo = Some(t.drive.max_turbo_speed);
        s.turbo_fx_on = false;
    }
    s.dash_cooldown = (s.dash_cooldown - dt).max(0.0);
    // His damage object's update (`FUN_0072a550`) runs the special's two clocks down.
    // [game] When the second goes from above 0 to 0 or below (`0072a550` line 201), the end
    // (`FUN_0072b980`) first sends `Untrigger` to the walk's and the car's self-trigger names,
    // each when its `...SelfTrigger` is on (the tuning folds that in as a 0 name), whatever the
    // form; then the type's own end (`special_types`). Bumblebee's shockwave, particles and ring
    // have ended themselves by then; the CombatRepair zone is a thing the Untrigger switches off
    // (vslot 7 through `0080df70`; `repair.rs`). A duration of 0 never starts the clock, so its
    // end never runs.
    s.special_cooldown = (s.special_cooldown - dt).max(0.0);
    if s.special_left > 0.0 {
        s.special_left -= dt;
        if s.special_left <= 0.0 {
            s.special_left = 0.0;
            for name in [t.special.script, t.special.vehicle_script] {
                if name != 0 {
                    s.untriggers.push(name);
                }
            }
        }
    }
    // The type-coded special's timer and its end list (`special_types::Active::step`). [game 0072b980]
    if let Some(end) = s.special.as_mut().and_then(|active| active.step(dt)) {
        for effect in end {
            special_effect(s, effect);
        }
    }

    // The state machine first, then the motion mode it leaves him in, in the same update: in
    // run 1 the update that starts a run already moves him, and the one that starts a jump
    // already applies the jump's air control. [trace]
    control(s, input, t, arena, dt);
    // The body step's end clears the climb lock when Climb is not down (00852220 l.76-79, read
    // 2026-10-20: after character_update's machine, 0071dc20). [game]
    if input.buttons_down >> button::CLIMB & 1 == 0 {
        s.climb_regrab_lock = false;
    }
    action_exits(s, input, t);
    // [game] 0072a550 sets controller speed and animation delta308 to slowMultiplier-1.
    let motion_scale=if s.damage_timers.slowed>0.0 {t.damage.slow_multiplier} else {1.0};
    let animation_dt=dt*motion_scale;
    // His animation rate (`special_rate`: 0.2 while sniping, a type-2 special's 1.5) scales every
    // layer's player, not only the action clip's: `character_speed_scale_set` 0071f4d0 is the
    // AnimController's setter (00748dc0), each layer player's rate +0x1c = base rate x layer
    // factor x the scale. [game 0071f4d0, 00748dc0, 00748d50, 00748e40]
    let layers_dt = animation_dt * s.special_rate;
    base_step(s, input, t, layers_dt);
    let mut partial = std::mem::take(&mut s.hit_partial);
    let base = playing_clip(s, t);
    partial.update_partial(&t.rule_sets, base, &|variable| rule_value(s, input, variable));
    partial.advance(&t.rule_sets, layers_dt);
    s.hit_partial = partial;
    // `V2RPartialSet`'s layer: the same turn of the controller (exit rules, then the clip plays
    // on), until its clip ends into `emptyAnimSet`. [game 0084e3c0]
    if s.fly_partial.playing().is_some() {
        let mut layer = std::mem::take(&mut s.fly_partial);
        let base = playing_clip(s, t);
        layer.update_partial(&t.rule_sets, base, &|variable| rule_value(s, input, variable));
        layer.advance(&t.rule_sets, layers_dt);
        s.fly_partial = layer;
    }
    // The body's velocity before the mode's control blend: what the game's control step keeps under a
    // ceiling (`physics_avatar`).
    let vel_before_mode = s.vel;
    // FlyMode's turbo and brake are on the character's ability list, which runs them in every
    // mode (`flight::FlyTimers`); in flight `step_fly` runs them. [game] The turbo is the car's
    // class, with the car's upgrade levels (00732e60, `FlyTimers::upgrade`).
    s.fly_timers.upgrade = Some(crate::driving::turbo_scales(&t.drive, s.turbo_upgrades));
    if s.mode != Mode::Fly {
        s.fly_timers.tick(&t.forms.fly, s.mode == Mode::Drive, dt);
    }
    match s.mode {
        Mode::Stand | Mode::Move => step_move(s, input, t, dt),
        Mode::Strafe => step_strafe(s, input, t, animation_dt),
        Mode::Jump(kind) => step_jump(s, input, t, kind, dt),
        Mode::Drive => step_drive(s, input, t, arena, dt),
        Mode::Fly => {
            step_fly(s, input, t, arena, dt);
            return;
        }
        Mode::Unfold => step_unfold(s, input, t, dt),
        Mode::Hover => {
            step_hover(s, input, t, arena, dt);
            return;
        }
        Mode::Action if s.hit_full || s.stun_full => step_hit_reaction(s, t, dt),
        Mode::Action => step_action(s, input, t, arena, animation_dt),
        Mode::Climb => step_climb(s, input, t, arena, animation_dt),
    }
    if motion_scale!=1.0 && !s.hit_flyback {
        s.vel.x*=motion_scale; s.vel.y*=motion_scale;
        if s.mode==Mode::Climb {s.vel.z*=motion_scale;}
    }
    if s.mode==Mode::Drive {
        arena.separate_targets(&mut s.pos,t.vehicle_radius,t.vehicle_height);
        s.car.airborne=!s.car.chassis.grounded();
        s.speed=s.vel.truncate().dot(s.facing());
        touch(s,!s.car.airborne,dt);
        return;
    }
    if s.mode == Mode::Climb || s.action.as_ref().is_some_and(|a| a.from_climb || a.lifted) {
        // On the wall the clips and the climb mode move him and nothing pulls him down; the
        // ground and the wall still hold him. [game: the climb's control vector is (1, 1, 1)]
        // The same for the two jumps off it [game 00856cc0, data; settled 2026-10-20]: stateClimbJumpUp and
        // stateClimbJumpOff are BasicMotionMode states (motionModeType 1ad6ae68), whose enter sets (1,1,1),
        // and Wall_JumpUp / Wall_JumpOff carry no ChangeTractionEvent (notes\clip-events-traction-motion.md
        // section 2), so the clip drives all three axes to its end; the same holds for the `lifted` legs
        // (their clips key [1,1,1]).
        s.pos += s.vel * dt;
        arena.push_out(&mut s.pos, t.robot_radius, t.robot_height);
        s.pos.z = s.pos.z.max(arena.floor(s.pos.x, s.pos.y, s.pos.z));
        let floor = arena.floor(s.pos.x, s.pos.y, s.pos.z);
        touch(s, s.pos.z <= floor + 0.01, dt);
        return;
    }
    // In the jump mode the body's gravity carries the mode's slow motion. [game]
    let gravity = t.gravity * if matches!(s.mode, Mode::Jump(_)) { s.gravity_scale } else { 1.0 } + s.punch_gravity;

    // [game] on a level the robot's body takes the world step of the game's physics (`physics_avatar`,
    // `notes\physics.md` 1.9): gravity, the control step's planes, the prediction, the contact pass with
    // its floors and walls, the kept contacts' refresh, the solver and the pose pass. Nothing snaps him
    // to a floor: the pass and the solver hold him up. The mode's blend along x and y is the control's
    // target; the velocity it started from is what a ceiling leaves. Then the owner's ground test (008524b0)
    // decides the ground flag from the kept contacts and the ground probe, and may push or land him.
    if !s.is_vehicle() {
        let entering = Vec3::new(vel_before_mode.x, vel_before_mode.y, s.vel.z);
        let mut body = std::mem::take(&mut s.avatar_body);
        // [game] STEP+0x3e4 is MoveMode's flag (set by 00875eb0, cleared by 00875ec0). The state
        // machine's Move enter starts MoveMode and Idle's enter starts only the first mode, which
        // does not set it (notes/state-machine.md, "Idle enter" / "Move enter").
        let req = crate::physics_avatar::AvatarRequest {
            feet: s.pos,
            velocity: entering,
            controlled: s.vel,
            gravity,
            radius: t.robot_radius,
            height: t.robot_height,
            mass: t.robot_mass,
            facing: s.facing(),
            in_move_mode: s.mode == Mode::Move,
            ai: arena.ai_bodies(),
            id: s.actor.id,
            dt,
        };
        let stepped = arena.step_avatar(&mut body, &req);
        s.avatar_body = body;
        if let Some(out) = stepped {
            s.pos = out.feet;
            s.vel = out.velocity;
            if !out.standing && s.vel.z < 0.0 {
                s.fall_time += dt;
            }
            if let (Some(normal), true) = (out.wall, s.hit_full && s.hit_flyback && !s.wall_splatted) {
                // [game] the contact's own normal for the wall splat (it was the stand-in push's direction)
                let character = arena.targets().iter().any(|target| (s.pos - target.pos).truncate().length() <= t.robot_radius + target.radius + 0.01);
                let finished = s.base.current(&t.rule_sets).is_some_and(|c| !c.looping && s.base.tick >= c.ticks);
                let flat = normal.truncate().normalize_or_zero().extend(0.0);
                if crate::damage::wall_splat(flat, s.facing().extend(0.0), finished, character, false) {
                    s.wall_splatted = true;
                    s.damage_type = 0x5060_153e;
                    s.yaw = yaw_of(flat.truncate());
                    s.hit_flyback = false;
                    s.hit_last = None;
                    s.vel = Vec3::ZERO;
                }
            }
            if out.wall.is_some() && s.mode != Mode::Jump(JumpType::Fall) {
                // [game 006e2c10 l.246-258, `notes\physics.md` 1.5 step 3] The contact pass has already taken
                // the into-wall part of the velocity off (`out.velocity` is the solver's). There is no
                // scrub on top of it in the game, so the speed is what the body keeps. FIXED 2026-10-20
                // (was a 0.5 scrub with no game counterpart).
                s.speed = s.vel.truncate().dot(s.facing());
            }
            s.apex_z = s.apex_z.max(s.pos.z);
            touch(s, out.standing, dt);
            tilt(s, t, arena, dt);
            return;
        }
    }

    // Integrate, then settle against the arena. In the air the height follows the exact arc for
    // steady gravity, so a jump peaks where its launch speed says whatever the step length.
    let airborne = matches!(s.mode, Mode::Jump(_))
        || s.car_airborne()
        || (s.mode != Mode::Drive && s.pos.z > arena.floor(s.pos.x, s.pos.y, s.pos.z) + STEP_HEIGHT)
        || (s.mode != Mode::Drive && s.vel.z > 0.0);
    let previous_feet = s.pos;
    s.pos.x += s.vel.x * dt;
    s.pos.y += s.vel.y * dt;
    if airborne {
        s.pos.z += s.vel.z * dt - 0.5 * gravity * dt * dt;
        s.vel.z -= gravity * dt;
        if s.vel.z < 0.0 {
            s.fall_time += dt;
        }
    }
    if !airborne && s.vel.z <= 0.0 {
        if let Some(z) = arena.follow_floor(previous_feet, s.pos.truncate()) {
            s.pos.z = z;
        }
    } else if s.vel.z <= 0.0 {
        // Sweep down before side collision so a falling body cannot skip a ramp top.
        // [stand-in] The arena's centre-only drop query also serves this landing sweep; on a level the
        // arena's `drop_onto` also chains in `LevelWorld::drop_onto` (level_world.rs, a ball sweep on
        // LEDGE_MASK, not centre-only). settles: the game's floor contact for a landing (the contact planes of body_control_step 006db490 and the contact scan 008524b0); not read.
        if let Some(hit) = arena.drop_onto(Vec3::new(s.pos.x, s.pos.y, previous_feet.z), s.pos.z,
            if s.is_vehicle() { t.vehicle_radius } else { t.robot_radius }) {
            s.pos.z = hit.z;
        }
    }
    let (radius, height) = if s.is_vehicle() { (t.vehicle_radius, t.vehicle_height) } else { (t.robot_radius, t.robot_height) };
    let before_push = s.pos;
    // [game] on a level the robot's pencil meets the walls through the contact pass (`World::move_pencil`,
    // `physics_contact`), and so does the driving car's chassis (`World::move_chassis`, in `step_drive`);
    // a car-sized body elsewhere keeps the stand-in push.
    let pushed = if s.is_vehicle() {
        !(s.mode == Mode::Drive && arena.chassis_pass()) && arena.push_out(&mut s.pos, radius, height)
    } else {
        arena.move_pencil(previous_feet, &mut s.pos, &mut s.vel, radius, height)
    };
    if pushed && s.hit_full && s.hit_flyback && !s.wall_splatted {
        // [stand-in] Arena push supplies the contact normal (off a level; on a level `move_pencil`
        // moves the body and this normal is the delta of the total push); a level host must provide
        // the actual physics contact flags and body. Character contacts are excluded. settles: the contact normal the game's contact scan gives a push (008524b0; the planes in 006db490); not read.
        let normal = (s.pos - before_push).normalize_or_zero();
        let character = arena.targets().iter().any(|target| (s.pos - target.pos).truncate().length() <= radius + target.radius + 0.01);
        let finished = s.base.current(&t.rule_sets).is_some_and(|c| !c.looping && s.base.tick >= c.ticks);
        if crate::damage::wall_splat(normal, s.facing().extend(0.0), finished, character, false) {
            s.wall_splatted = true;
            s.damage_type = 0x5060_153e;
            s.yaw = yaw_of(normal.truncate());
            s.hit_flyback = false;
            s.hit_last = None;
            s.vel = Vec3::ZERO;
        }
    }
    // The car touching something at a speed over 5: the drive mode's contact handler
    // `FUN_00860870` reads the body's speed, all three axes, at l.106-111 (after its own
    // character projection, l.94-102, which only a non-flagged character reaches) and rumbles
    // 0xc when it is over 5 and the contact is kind 3 (l.110). [game 00860870 l.106-111] The
    // port's `s.vel` is not yet scrubbed here (the scrub is below), so it is the speed before
    // the bounce; for a wall the game's read is also the body's velocity at the contact (no
    // character projection; where the game's own bounce falls in its update is not read). Kind 3 is the
    // contact state code of 006d7c20 (approaching faster than 0.5 along the normal, `collisionfx::
    // physics_type`), the listener record's +0x3c [game, settled 2026-10-20; vehicle.rs `head_on`].
    // [stand-in: the arena's push for the contact, its normal the push direction]
    let push_normal = (s.pos - before_push).normalize_or_zero();
    if pushed && s.mode == Mode::Drive && s.vel.length() > CAR_KNOCK_SPEED
        && crate::collisionfx::physics_type(push_normal, s.vel) == 3 {
        preset_rumble(s, t, rumble::preset::VEHICLE_HIT_WALL);
    }
    if pushed && s.mode != Mode::Jump(JumpType::Fall) && s.is_vehicle() {
        // Hitting a wall scrubs the speed that was going into it. [stand-in] settles: the game's wall contact from its contact scan (008524b0) and the speed it leaves; not read.
        // (Reach: the driving car on a level is not pushed here, since `chassis_pass` takes its walls in `move_chassis`; this runs for the arena's push and for a non-Drive vehicle form.)
        s.speed *= 0.5;
        if s.mode == Mode::Drive {
            s.vel.x *= 0.5;
            s.vel.y *= 0.5;
        }
    } else if pushed && s.mode != Mode::Jump(JumpType::Fall) {
        // A robot on foot: the game's contact pass takes the into-wall velocity off whole (`V -= k n`
        // with k = vn, friction 0 for an avatar: [game 006e2c10 l.246-258]), with no scrub on top. On a level
        // `move_pencil` has done that to `s.vel` already; the arena's push does not touch the velocity, so the
        // cut is taken here with the push's normal [stand-in: the arena's push normal, not a contact's]. FIXED
        // 2026-10-20 (was a 0.5 scrub with no game counterpart).
        let normal = (s.pos - before_push).normalize_or_zero();
        let vn = s.vel.dot(normal);
        if vn < 0.0 {
            s.vel -= vn * normal;
        }
        s.speed = s.vel.truncate().dot(s.facing());
    }
    let floor = arena.floor(s.pos.x, s.pos.y, s.pos.z);
    s.apex_z = s.apex_z.max(s.pos.z);
    match s.mode {
        // Whatever the mode, the body comes down on the ground and stays on it; which mode he
        // is in after a landing or after running off an edge is the state machine's to say
        // (`connectionOnGround`, `connectionFall`), not the body's. [game]
        _ if airborne => {
            if s.pos.z <= floor && s.vel.z <= 0.0 {
                s.pos.z = floor;
                s.vel.z = 0.0;
            }
        }
        _ if s.pos.z > floor + STEP_HEIGHT => {
            // Ran off an edge: he falls from the next update on, in whatever mode he is in.
        }
        _ => {
            // [stand-in] A step up is climbed at 12 a second instead of in one frame. settles: the game's step-up in the contact scan (008524b0): its height and its time; not read.
            s.pos.z = floor.min(s.pos.z + 12.0 * dt);
            s.vel.z = 0.0;
        }
    }
    let contact = match s.mode {
        Mode::Drive => !s.car.airborne,
        _ => s.pos.z <= floor + 0.01 && s.vel.z <= 0.0,
    };
    touch(s, contact, dt);
    tilt(s, t, arena, dt);
}

/// Keeps the body's contact with the ground, for the state machine's ground flag. Time going
/// down is counted from the moment he leaves the ground (`animRuleLongFall`). [trace]
fn touch(s: &mut State, contact: bool, dt: f32) {
    if contact == s.touching {
        s.touching_for += dt;
    } else {
        s.touching = contact;
        s.touching_for = 0.0;
        if !contact {
            s.fall_time = 0.0;
        }
    }
}

/// Out of vehicle form the rendered root is upright at once, with no blend. Every exit that
/// leaves a tilted body stands it up in its own update: DriveMode's exit (0085ed50 l.154-175, in
/// `set_mode`) and FlyMode's (00865b80). [game] The only other writer of the roll and pitch on foot
/// is ForceUpEvent (`clip_force_up`), whose six uses are [0, 0, 1] [data], so nothing is left to decay.
fn tilt(s: &mut State, _t: &Tuning, _arena: &dyn World, _dt: f32) {
    (s.pitch, s.roll) = (0.0, 0.0);
}

/// The pace on the ground for this step. The body moves at the root motion of the clips that
/// are playing, and clips cross-fade in a stack: each new one fades in, straight-line over its
/// own blend time, on top of whatever was there, which keeps fading and playing underneath.
/// Checked against the recorded game update by update (`notes\live-trace.md`): starts, stops
/// from a run and from a walk, and both landings. [trace]
fn step_pace(s: &mut State, walk: &Locomotion, dt: f32) -> f32 {
    let curve = |pace| match pace {
        Pace::Stop => Some(&walk.stop),
        Pace::LandRun => Some(&walk.land_run),
        Pace::BigLandRun => Some(&walk.big_land_run),
        _ => None,
    };
    let fade = |pace| match pace {
        Pace::Idle => walk.idle_blend,
        Pace::Walk => walk.walk_blend,
        Pace::Run => walk.run_blend,
        other => curve(other).map_or(0.0, |c| c.crossfade),
    };
    let top = s.pace();
    let finished = s.layers.last().is_some_and(|l| curve(l.pace).is_some_and(|c| l.time >= c.duration()));
    let moving = if s.gait == Gait::Run { Pace::Run } else { Pace::Walk };

    if finished {
        // A one-off that has played out hands over at once: the stop to standing, a landing
        // to the walk or run.
        s.layers.clear();
        if s.gait != Gait::Idle {
            s.layers.push(Layer { pace: moving, weight: 1.0, time: 0.0 });
        }
    } else {
        let next = match (s.gait, top) {
            (Gait::Idle, Pace::Idle | Pace::Stop) => None,
            // Only the run stops through the run-to-idle clip; anything else fades to idle.
            (Gait::Idle, Pace::Run) => Some(Pace::Stop),
            (Gait::Idle, _) => Some(Pace::Idle),
            // A landing plays out before the walk or run takes over.
            (_, Pace::LandRun | Pace::BigLandRun) => None,
            (_, top) if top == moving => None,
            _ => Some(moving),
        };
        if let Some(pace) = next {
            s.layers.push(Layer { pace, weight: 0.0, time: 0.0 });
        }
    }

    let mut speed = 0.0;
    for layer in &mut s.layers {
        let blend = fade(layer.pace);
        layer.weight = if blend > 0.0 { (layer.weight + dt / blend).min(1.0) } else { 1.0 };
        let own = match layer.pace {
            Pace::Idle => 0.0,
            Pace::Walk => walk.walk_speed,
            Pace::Run => walk.run_speed,
            other => curve(other).map_or(0.0, |c| c.speed(layer.time, layer.time + dt)),
        };
        layer.time += dt;
        speed += (own - speed) * layer.weight;
    }
    // Whatever lies under a clip that has fully faded in no longer counts.
    if let Some(full) = s.layers.iter().rposition(|l| l.weight >= 1.0) {
        s.layers.drain(..full);
    }
    if s.layers.len() == 1 && s.layers[0].pace == Pace::Idle {
        s.layers.clear();
    }
    speed
}

fn gait_of(deflection: f32, walk: &Locomotion) -> Gait {
    if deflection > walk.run_stick {
        Gait::Run
    } else if deflection > walk.move_stick {
        Gait::Walk
    } else {
        Gait::Idle
    }
}

/// Standing (the first mode) and MoveMode. Neither translates him: the clip that plays does,
/// by its root motion. The animation rules pick walk or run from the stick, and each clip has
/// one speed. [data] MoveMode turns him toward the stick; standing does not. [game]
fn step_move(s: &mut State, input: &Input, t: &Tuning, dt: f32) {
    let moving = s.mode == Mode::Move;
    let deflection = input.stick.length().min(1.0);
    let pushing = deflection * deflection > STICK_DEAD_SQUARED;
    if moving && pushing {
        s.yaw = turn_toward(s.yaw, yaw_of(input.stick), t.moving.turn_speed.to_radians() * dt);
    }
    let walk = &t.locomotion;
    s.gait = if moving { gait_of(deflection, walk) } else { Gait::Idle };
    let wanted = if let Some(moved) = clip_motion(s, t, dt) {
        // A state that plays its own clip in this mode (the first half of the change into the
        // car, while the stick does not yet line up): that clip's root moves him. [game]
        s.layers.clear();
        moved
    } else {
        s.speed = step_pace(s, walk, dt);
        s.facing() * s.speed
    };
    let horizontal = Vec2::new(s.vel.x, s.vel.y);
    let blended = if moving && !s.ground_flag() {
        // MoveMode off the ground follows the clips only a tenth of the way each update. [game]
        horizontal.lerp(wanted, 1.0 - (1.0 - MOVE_AIR_CONTROL).powf(dt * CONTROL_FRAME_RATE))
    } else {
        wanted
    };
    (s.vel.x, s.vel.y) = (blended.x, blended.y);
}

/// Standing in weapon mode his spine twists toward the aim no further than this; past it
/// his body turns by the rest. And the least squared stick StrafeMode counts as pushed.
/// [game]
const STRAFE_TWIST_MOST: f32 = 0.794_124_84;
const STRAFE_STICK_SQUARED: f32 = 0.0001;
const WEAPON_SET: u32 = crc32(b"WeaponSet");
/// The sets the control states start on the base layer, whose clips `animrules::Base` picks.
/// [game: `notes\state-machine.md`: Idle enter plays IdleStandSet, Move's WalkSet, Jump's
/// JumpSet (after the jump mode), Fall's FallSet (before it)]
const IDLE_SET: u32 = crc32(b"IdleStandSet");
const WALK_SET: u32 = crc32(b"WalkSet");
const JUMP_SET: u32 = crc32(b"JumpSet");
const FALL_SET: u32 = crc32(b"FallSet");
const DASH_SET: u32 = crc32(b"DashSet");
const FLY_SET: u32 = crc32(b"FlySet");

/// An angle brought into half a turn either side of zero, as the game does it.
fn wrapped(angle: f32) -> f32 {
    let turns = angle / std::f32::consts::TAU + 0.5;
    (turns - turns.floor() - 0.5) * std::f32::consts::TAU
}

/// What the animation rules of the weapon set ask about him. [game: `F_L_STICK_AMP` is the
/// stick's length, `F_L_STICK_DIR` the stick against the aim in degrees (`FUN_0070c380`);
/// `F_STRAFE_ANG` and `I_IS_STRAFING` are what StrafeMode last wrote; integer getter
/// `FUN_0070cae0` reads `I_IN_AIR` / `I_ON_GROUND` from the controller's ground flag]
fn rule_value(s: &State, input: &Input, variable: u32) -> Option<Value> {
    use crate::animrules::var;
    let stick = input.stick;
    Some(match variable {
        var::L_STICK_AMP => Value::Float(stick.length()),
        // [game] in FlyMode the aim heading (owner `+0x260`) the stick is measured against is
        // the body's own heading: outside weapon mode character_aim_update 0071af10 copies the
        // character's forward row (+0x1c) into it through aim_set_direction 00854350.
        var::L_STICK_DIR => {
            let reference = if s.mode == Mode::Fly { s.yaw } else { s.strafe.aim_yaw };
            Value::Float(wrapped(yaw_of(stick) - reference).to_degrees())
        }
        // [game 0070c380] `F_R_STICK_AMP` is the camera stick's length. `F_R_STICK_DIR` is its
        // direction against the same heading, negated for a helicopter or robot flyer with the
        // invert option (+0x1f4e) on. Read 2026-10-18: 007112b0 takes the camera stick as the
        // pad gives it (input slot +0x28, y positive down, `notes\input.md`) through the
        // CHARACTER's own frame, w = s.x * X row + s.y * Y row (transform +0x10 / +0x1c), not
        // the camera's; the heading of w less the aim heading (+0x260/+0x264, the same reference
        // as `F_L_STICK_DIR`) is the value. The invert option defaults off (0075e520 writes
        // +0x1f4e = 0 with the other option defaults). It makes FlySet's rules read as the
        // flight does: pushed up (climb) 180 = Fly_Up, pulled down 0 = Fly_Down, sideways 90 =
        // Fly_Idle.
        var::R_STICK_AMP => Value::Float(input.look.length()),
        var::R_STICK_DIR => {
            let frame = Quat::from_rotation_z(s.yaw) * Quat::from_rotation_x(s.pitch) * Quat::from_rotation_y(s.roll);
            let w = frame * input.look.extend(0.0);
            let reference = if s.mode == Mode::Fly { s.yaw } else { s.strafe.aim_yaw };
            Value::Float(wrapped(yaw_of(w.truncate()) - reference).to_degrees())
        }
        var::STRAFE_ANG => Value::Float(s.strafe.angle),
        var::IS_STRAFING => Value::Int(s.strafe.strafing as i32),
        var::FALL_DUR => Value::Float(s.fall_time),
        var::Z_VEL => Value::Float(s.vel.z),
        var::IN_AIR => Value::Int(!s.ground_flag() as i32),
        var::ON_GROUND => Value::Int(s.ground_flag() as i32),
        // What the base layer's rules (idle, walk, jump, fall) ask besides. [game: the jump
        // mode writes `C_JUMP_TYPE`; `F_TIME_SINCE_DAMAGED` is now minus the last hit's time]
        // Nothing is carried, so the interaction type is none: 0 is NoInteract's crc (bumblebee.txt
        // 22791-22815; VIP is 0xe081c00e, Flippable -1225938910) [data]. The non-none types are
        // asked by InteractSet (the set of StateInteract and StateSync, which `control` refuses: the
        // may-enter table) and by the base sets' VIP-carry transitions (IdleStandSet bumblebee.txt
        // 115965, WalkSet 138772 and 141847, starscream IdleStandSet 18895), which need a carry only
        // StateInteract's pickup starts; with 0 they never pass, as in the game for a robot carrying
        // nothing. [data, read 2026-10-20 in bumblebee.txt and starscream.txt]
        var::JUMP_TYPE => Value::Hash(s.jump_type),
        var::INTERACT_TYPE => Value::Hash(0),
        // [game 0070c380] `F_STUN_TIME` is the stun clock of the character's damage object
        // (`+0x234` -> `+0x54`, the one `0072a550` runs down; 0 with no such object).
        var::STUN_TIME => Value::Float(s.damage_timers.stunned),
        // [game 0070cae0] `I_SPECIAL_ON`: the ability object's timer (`+0x8c`) above 0.
        var::SPECIAL_ON => Value::Int((s.special_left > 0.0) as i32),
        // [game 0070cae0, 008713c0, 00871580] the getter's own answer is 0; HoverMode's enter
        // writes the variable 1 and its exit 0, so it is 1 while that mode runs. The
        // weapon-hover state runs on StrafeMode here but is HoverMode in the original.
        var::IS_HOVERING => Value::Int((s.mode == Mode::Hover || s.hover_weapon) as i32),
        // [game 0070c900, 00864d60, 00865b80] the getter's default is 0 and FlyMode's enter and
        // exit both write 0; no code in the executable writes a stunt kind into it (the six
        // `VEHICLE_STUNT_*` hashes occur nowhere in the executable): the stunts reach the
        // animation through the state machine's `stateFlyStunt*` states, so the fly set's
        // exits on these hashes never hold. [data] Checked 2026-10-20 against the decoded packs: the
        // six kind hashes occur in bumblebee.txt (22834-22864, 23992-24037) and starscream.txt
        // (16955-16985, 18113-18122) only as the crcValue of the six `animRuleVehicleStunt*` rules
        // and the rule keys that use them; no event or state in those dumps, nor in global_states.txt
        // or the flight survey dumps, carries a kind into C_VEHICLE_STUNT. objects.tsv was not
        // scanned (grep aborted on its 2 GB size), so this is settled for the text dumps only.
        var::VEHICLE_STUNT => Value::Hash(0),
        var::SPECIAL_IDLE_ON => Value::Int((s.idle_delay < 0.0) as i32),
        var::TIME_SINCE_DAMAGED => Value::Float(s.since_damage),
        var::TIME_IN_ANIM => Value::Float(if s.strafe.clip != 0 {
            s.strafe.layers.last().map_or(0.0, |layer| layer.elapsed_ticks / 60.0)
        } else { s.base.seconds() }),
        var::CHAR_FORM => Value::Int(if s.car_mode { 3 } else if s.mode == Mode::Strafe || s.weapon_air() || s.weapon_hover() { 2 } else { 1 }),
        var::DAMAGE_TYPE => Value::Hash(s.damage_type),
        _ => return None,
    })
}

/// What the base layer's rules read: the same variables, with the playing clip's own time
/// (the controller is taken out of the state while it walks them).
fn base_value(s: &State, input: &Input, seconds: f32, variable: u32) -> Option<Value> {
    if variable == animrules::var::TIME_IN_ANIM {
        return Some(Value::Float(seconds));
    }
    rule_value(s, input, variable)
}

/// A control state starts one of the base layer's sets: its entry rules pick the clip,
/// coming from the set and clip that were playing (`from`). [game]
fn base_enter(s: &mut State, input: &Input, t: &Tuning, set: u32, from: (u32, u32)) {
    let mut base = std::mem::take(&mut s.base);
    let seconds = base.seconds();
    base.enter(&t.rule_sets, set, from, &|variable| base_value(s, input, seconds, variable));
    s.base = base;
}

/// The animation controller's update for the base layer: the playing clip's exit rules, then
/// the clip goes on playing. [game: the walk before the mode's own update, as the weapon
/// set's is]
fn base_step(s: &mut State, input: &Input, t: &Tuning, dt: f32) {
    if s.base.playing().is_none() {
        return;
    }
    let mut base = std::mem::take(&mut s.base);
    let seconds = base.seconds();
    base.update(&t.rule_sets, &|variable| base_value(s, input, seconds, variable));
    base.advance(&t.rule_sets, dt);
    s.base = base;
    // The motion events the base layer's clip passed this step (a clip just started passes
    // those at its first tick too): queued for `apply_clip_effects`. [game]
    queue_layer_events(s, t);
}

/// Queues the motion events the base layer's clip passed since the last look (`base_seen`: the
/// clip's serial and tick then; a clip started since, here or by a state's enter, passes from
/// its first tick).
fn queue_layer_events(s: &mut State, t: &Tuning) {
    let seen = std::mem::replace(&mut s.base_seen, (s.base.serial, s.base.tick));
    let Some(clip) = s.base.current(&t.rule_sets) else { return };
    let from = (seen.0 == s.base.serial).then_some(seen.1);
    if from == Some(s.base.tick) {
        return;
    }
    s.clip_effects.extend(clip.motion_between(from, s.base.tick));
}

/// `C_JUMP_TYPE`'s value for each jump the mode picks (`notes\bumblebee-mechanics.md`,
/// "JumpMode"). [game]
fn jump_type_hash(kind: JumpType) -> u32 {
    match kind {
        JumpType::Standing | JumpType::Fall => 0xaa18_5eeb,
        JumpType::Moving => 0x0ec5_1d8e,
        JumpType::HighStanding => 0xf431_2e7f,
        JumpType::Long => 0x225a_6ca8,
        JumpType::Climb => 0xdfb7_146d,
    }
}

/// Starts a clip of the weapon set on top of what is playing; `blend` is the fade the clip
/// being left asks for, if it does. [game: a clip with `usePrevAnimFrame` starts at the
/// time the one before had reached; the fade is the new clip's `xfadeTime`, or the old
/// one's `xfadeTimeOnExit` when that names the new clip or none; keeping the previous
/// frame wins over continuing the old clip's overrun. FUN_00748f00]
fn strafe_start(s: &mut State, t: &Tuning, id: u32) {
    let Some(clip) = t.weapon_set.clip(id) else { return };
    let old = t.weapon_set.clip(s.strafe.clip);
    let handed = old.filter(|old| old.exit_crossfade >= 0.0 && (old.exit_crossfade_to == 0 || old.exit_crossfade_to == id));
    let blend = handed.map_or(if clip.crossfade < 0.0 { DEFAULT_CROSSFADE } else { clip.crossfade }, |old| old.exit_crossfade);
    let previous = s.strafe.layers.last();
    let elapsed = if clip.keeps_time { previous.map_or(s.strafe.tick, |layer| layer.elapsed_ticks) }
        else if old.is_some_and(|old| old.continue_time) { previous.map_or(0.0, |layer| layer.overrun) * clip.speed }
        else { 0.0 };
    let tick = if clip.looping { elapsed % clip.ticks.max(1.0) } else { elapsed.min(clip.ticks) };
    s.strafe.layers.push(RootLayer { clip: id, action: false, weight: 0.0, blend, tick, carry: Vec2::ZERO,
        elapsed_ticks: if clip.looping { elapsed } else { elapsed.min(clip.ticks) },
        overrun: if clip.looping { 0.0 } else { (elapsed-clip.ticks).max(0.0) } });
    (s.strafe.clip, s.strafe.tick) = (id, tick);
    s.weapon_serial = s.weapon_serial.wrapping_add(1);
}

/// The clip the weapon set's entry rules pick coming from `from_set` and `from_id`, if one
/// fits (`FUN_0084ddb0` with no clip named, `FUN_0070acc0`). [game]
fn strafe_entry(s: &mut State, input: &Input, t: &Tuning, from_set: u32, from_id: u32) {
    let get = |variable: u32| rule_value(s, input, variable);
    let picked = animrules::entry(&t.weapon_set.incoming, from_set, from_id, &get);
    if let Some((_, id)) = picked.filter(|&(set, id)| set == WEAPON_SET && id != 0 && id != s.strafe.clip) {
        strafe_start(s, t, id);
    }
}

/// StrafeMode (`FUN_00875ef0`), for a character whose data leaves `useCodeDrivenStrafing`
/// off, as Bumblebee's does: the mode moves nothing itself. With the stick pushed it turns
/// his body to the aim at once and writes `F_STRAFE_ANG`, the stick against the aim, which
/// the weapon set's rules turn into one of eight run clips; each travels 20 a second along
/// a root turned its own way. Standing, `F_STRAFE_ANG` is the aim against his body and his
/// spine twists by it, to 45.5 degrees at most; past that his body turns by the rest (the
/// rules start a turning clip at 45, which turns him first). [game]
///
/// Around it, each update: the leading clip's exit rules are walked BEFORE the mode's
/// update, so they see the angle the update before wrote (standing it is the twist, about
/// zero, so every start is one update of the forward clip: run 1 at 493.86 reads 2.13
/// ahead, then 4.39 at 29 degrees); then the clips cross-fade in a stack as on foot, each
/// fading in over its blend time on what was there, and their blended root motion is his
/// velocity (run 1: 20.00 steady, every start, stop and change of direction to 0.05).
/// [trace]
///
/// The update the mode is entered leaves `F_STRAFE_ANG` as its enter wrote it (the stick
/// against the facing he had) while his body already turns to the aim: run 1 at 489.79
/// runs on for two updates along the new facing before the sideways clip starts. [trace]
/// The cause is the update order [game]: the state machine runs first in an update, then the
/// mode (`notes\state-machine.md` "Run 1 order"; `advance` calls `control` before `base_step`),
/// so the rules read the angle the mode wrote on the update before, and the entering update
/// does not write it (`step_strafe`'s `entering`, which is the game's enter value).
///
/// Code-driven strafing is not built, and no playable robot takes it [game, read 2026-10-20]: the
/// flag `+0x20` is `FUN_00876fe0`, which is 1 for a character no player controls
/// (`character_input_is_player` 00711210 false) and otherwise the data's `useCodeDrivenStrafing`,
/// true only in seven vehicle attribute blocks (MilitaryMissileTruck, MilitaryTank,
/// MilitaryVehicle, NESTHummer, NESTHummerEscort, PoliceCar, PoliceTruck: global_states.txt
/// 63244-67585). Its update, for the enemy side only: `FUN_00875ef0` l.160-197, the heading turned
/// toward the stick's at most `DAT_00ded044` = 10.471975 rad (600 degrees) a second, speed from
/// `FUN_008770d0`, then root mode 2. The twist smoothing (`FUN_00876950`) is for uncontrolled
/// characters too. No character flag zeroes the stick (the special attack's state does).
fn step_strafe(s: &mut State, input: &Input, t: &Tuning, dt: f32) {
    let entering = std::mem::take(&mut s.strafe.entering);
    if !entering {
        weapon_rules(s, input, t);
    }

    // The mode's own update.
    let aim_yaw = input.aim.filter(|aim| aim.length_squared() > 1e-6).map_or(s.strafe.aim_yaw, yaw_of);
    s.strafe.aim_yaw = aim_yaw;
    if input.stick.length_squared() >= STRAFE_STICK_SQUARED {
        if !entering {
            s.strafe.angle = wrapped(yaw_of(input.stick) - aim_yaw).to_degrees();
        }
        s.yaw = aim_yaw;
        s.strafe.twist = 0.0;
    } else {
        let off = wrapped(aim_yaw - s.yaw);
        s.strafe.angle = off.to_degrees();
        if off.abs() > STRAFE_TWIST_MOST {
            s.yaw = wrapped(s.yaw + off - STRAFE_TWIST_MOST.copysign(off));
        }
        s.strafe.twist = off.clamp(-STRAFE_TWIST_MOST, STRAFE_TWIST_MOST);
    }

    let (moved, turned) = weapon_layers(s, t, dt);
    // A turning clip turns him (`Weapon_IdleTurnL` and `R`, 67.5 degrees). [data]
    // Its turn is blended by its fade like its travel [game 0053d600 l.145-178, settled 2026-10-20]:
    // BlendAnimation::BlendOutputs gives every bone, the root among them, one weight w (+0x30):
    // translation w / 1-w (l.154-161), the turn through 00542ff0 with the same w (l.162-167), scale
    // likewise (l.168-178). (For a yaw-only root the slerp of the turn is the lerp of its angle.)
    s.yaw = wrapped(s.yaw + turned);
    let facing = s.facing();
    let right = Vec2::new(facing.y, -facing.x);
    let v = right * moved.x + facing * moved.y;
    (s.vel.x, s.vel.y) = (v.x, v.y);
    s.speed = moved.length();
    s.gait = gait_of(input.stick.length().min(1.0), &t.locomotion);
}

/// The animation controller's turn for the weapon set (`FUN_0084e3c0`): the leading clip's
/// exit rules are walked, and the first that fits names the clip to start, or none, which
/// walks the set's entry rules from the clip being left. [game]
fn weapon_rules(s: &mut State, input: &Input, t: &Tuning) {
    let Some(clip) = t.weapon_set.clip(s.strafe.clip) else { return };
    let finished = !clip.looping && s.strafe.tick >= clip.ticks;
    let get = |variable: u32| rule_value(s, input, variable);
    if let Some(exit) = animrules::exit(&clip.exits, clip.branch(s.strafe.tick), finished, &get).filter(|exit| exit.to_set == WEAPON_SET) {
        match exit.to_id {
            0 => strafe_entry(s, input, t, WEAPON_SET, clip.id),
            id if id != clip.id => strafe_start(s, t, id),
            _ => {}
        }
    }
}

/// Plays the weapon set's clips on by a step, cross-fading down their stack, and gives
/// their blended root motion: his velocity in his own frame, and his turn. [data] [trace]
fn weapon_layers(s: &mut State, t: &Tuning, dt: f32) -> (Vec2, f32) {
    let (mut moved, mut turned) = (Vec2::ZERO, 0.0);
    // The clips play at his animation rate (`special_rate`, 0071f4d0 / 00748dc0); the root
    // motion they give is per second of the game's time, so slower clips move him slower. [game]
    let rate = s.special_rate;
    for layer in &mut s.strafe.layers {
        layer.weight = if layer.blend > 0.0 { (layer.weight + dt / layer.blend).min(1.0) } else { 1.0 };
        let ticks = dt * rate * crate::formats::anim::TICKS_PER_SECOND;
        let (own, turn) = match (layer.action, t.weapon_set.clip(layer.clip), t.actions.clips.get(&layer.clip)) {
            (false, Some(clip), _) => {
                let to = layer.tick + ticks * clip.speed;
                let (travel, turn) = clip.travel(layer.tick, to);
                if clip.looping { layer.elapsed_ticks += ticks * clip.speed; }
                else {
                    layer.elapsed_ticks = (layer.elapsed_ticks + ticks * clip.speed).min(clip.ticks);
                    layer.overrun += (to - clip.ticks).max(0.0);
                }
                layer.tick = if clip.looping { to % clip.ticks.max(1.0) } else { to.min(clip.ticks) };
                (Vec2::new(travel.x, travel.y) / dt.max(1e-6), turn)
            }
            (true, _, Some(clip)) => {
                let to = (layer.tick + ticks * clip.speed).min(clip.ticks);
                let travel = clip.root_at(to) - clip.root_at(layer.tick);
                layer.tick = to;
                (Vec2::new(travel.x, travel.y) / dt.max(1e-6), 0.0)
            }
            _ => (layer.carry, 0.0),
        };
        moved += (own - moved) * layer.weight;
        turned += (turn - turned) * layer.weight;
    }
    if let Some(top) = s.strafe.layers.last() {
        s.strafe.tick = top.tick;
    }
    // The leading clip's motion events passed this step: its landings switch the motion mode
    // to StrafeMode, `Weapon_Fall` to JumpMode (`ChangeMotionModeEvent`). [game] [data]
    let seen = std::mem::replace(&mut s.weapon_seen, (s.weapon_serial, s.strafe.tick));
    if let Some(clip) = t.weapon_set.clip(s.strafe.clip) {
        let from = (seen.0 == s.weapon_serial).then_some(seen.1);
        if from != Some(s.strafe.tick) {
            s.clip_effects.extend(clip.motion_between(from, s.strafe.tick));
        }
    }
    // Whatever lies under a clip that has fully faded in no longer counts.
    if let Some(full) = s.strafe.layers.iter().rposition(|l| l.weight >= 1.0) {
        s.strafe.layers.drain(..full);
    }
    (moved, turned)
}

/// The set and clip playing on the base layer, as a set's entry rules ask where he comes
/// from: the weapon set's clip while it plays, a control state's own clip, else the set
/// the state he is in starts (the clip there is the chooser's and is not kept: no entry
/// rule of the weapon set names one of those sets' clips). [game: which set each state
/// class starts]
fn playing_clip(s: &State, t: &Tuning) -> (u32, u32) {
    if s.strafe.clip != 0 {
        return (WEAPON_SET, s.strafe.clip);
    }
    if let Some((a, clip)) = s.action.as_ref().and_then(|a| Some((a, t.actions.clips.get(&a.clip)?))) {
        let set = action_rule_clip(t, a.state, a.clip).map_or_else(|| crc32(clip.set.as_bytes()), |(set, _)| set);
        return (set, crc32(clip.id.as_bytes()));
    }
    // What the base layer's rules have been playing (idle, walk, jump, fall and the landings).
    if let Some(playing) = s.base.playing() {
        return playing;
    }
    let class = s.runner.state.and_then(|state| t.control.states.get(state)).map(|d| d.class.as_str());
    let set: &[u8] = match class {
        Some("StateJump") => b"JumpSet",
        Some("StateFall") => b"FallSet",
        Some("StateMove") => b"WalkSet",
        Some("StateIdle") => b"IdleStandSet",
        _ => return (0, 0),
    };
    (crc32(set), 0)
}

/// StrafeMode's enter (`FUN_00876990`) and the weapon set's start that follows it in the
/// weapon state's enter: `I_IS_STRAFING` goes to 1; unless the mode before was the dash or
/// the jump the aim heading becomes his facing; with the stick pushed `F_STRAFE_ANG` is the
/// stick against that; the twist is cleared. Then the set's entry rules pick its first
/// clip, coming from what plays: out of the weapon states' jump and fall that is the weapon
/// set's own air clip (`Weapon_Fall` leads to the landings), which goes on fading out
/// underneath. [game] [assumed: from the ground, what cross-fades out underneath is the
/// clip of the control state being left playing on (the dodge), else the velocity he had,
/// in his own frame, held (the run clip plays on in the original, at a steady 24)] settles: the StrafeMode enter's blend (`00852150` switches to it), or the run 1 trace at 489.79 (`notes\weapon-mode.md` l.150-170, read: the run's 24 fades under the forward clip).
fn enter_strafe(s: &mut State, input: &Input, t: &Tuning, old: Mode) {
    let from_air = matches!(old, Mode::Jump(_));
    // The state being left is still the runner's while the next one is entered.
    let dash = s.runner.state.and_then(|state| t.control.states.get(state)).is_some_and(|d| d.class == "StateDash");
    let facing = s.facing();
    let had = Vec2::new(s.vel.x, s.vel.y);
    let carry = if from_air { Vec2::ZERO } else { Vec2::new(had.dot(Vec2::new(facing.y, -facing.x)), had.dot(facing)) };
    let aim_yaw = if from_air || dash { input.aim.map_or(s.strafe.aim_yaw, yaw_of) } else { s.yaw };
    let (from_set, from_id) = playing_clip(s, t);
    let old = std::mem::take(&mut s.strafe);
    s.strafe = Strafe { strafing: true, aim_yaw, entering: true, ..Strafe::default() };
    if input.stick.length_squared() >= STRAFE_STICK_SQUARED {
        s.strafe.angle = wrapped(yaw_of(input.stick) - aim_yaw).to_degrees();
    }
    if old.clip != 0 {
        (s.strafe.clip, s.strafe.tick, s.strafe.layers) = (old.clip, old.tick, old.layers);
    } else {
        // A control state's clip (the dodge) plays on underneath from where it had got to.
        let (clip, tick) = s.action.as_ref().map_or((0, 0.0), |a| (a.clip, a.tick));
        s.strafe.layers.push(RootLayer { clip, action: clip != 0, weight: 1.0, blend: 0.0, tick, carry, elapsed_ticks:tick, overrun:0.0 });
    }
    strafe_entry(s, input, t, from_set, from_id);
}

/// The velocity the playing state clip's root motion gives over this step, in the world, if
/// a state clip is playing. [data]
fn clip_motion(s: &State, t: &Tuning, dt: f32) -> Option<Vec2> {
    let action = s.action.as_ref()?;
    let data = t.actions.clips.get(&action.clip)?;
    let moved = if dt > 1e-6 { action.root_travel(data) / dt } else { Vec3::ZERO };
    let facing = s.facing();
    let right = Vec2::new(facing.y, -facing.x);
    Some(right * moved.x + facing * moved.y)
}

/// The change back out of the car (the drive companion mode, `FUN_00863070`). It takes the
/// body's speed along the ground; while that is over the exit's least speed it comes down by
/// the exit's deceleration (and stops at zero, not at the least speed, which is why run 1
/// settles at 23.7 and not 24); the stick turns him at the exit's turn speed; and the body
/// is sent along his facing at that speed. Run or stop is chosen once, by the stick, when the
/// mode starts. The clip's root motion does not move him. [game] [trace]
fn step_unfold(s: &mut State, input: &Input, t: &Tuning, dt: f32) {
    let d = &t.drive;
    let (least, slowing, turn) = if s.unfold_run {
        (d.exit_to_run_min_speed, d.exit_to_run_deceleration, d.exit_to_run_turn_speed)
    } else {
        (d.exit_to_idle_min_speed, d.exit_to_idle_deceleration, d.exit_to_idle_turn_speed)
    };
    let mut speed = Vec2::new(s.vel.x, s.vel.y).length();
    if speed > least {
        speed = (speed - slowing * dt).max(0.0);
    }
    if input.stick.length_squared() > STICK_DEAD_SQUARED {
        s.yaw = turn_toward(s.yaw, yaw_of(input.stick), turn.to_radians() * dt);
    }
    let v = s.facing() * speed;
    (s.vel.x, s.vel.y) = (v.x, v.y);
    s.speed = speed;
    s.gait = Gait::Idle;
}

fn step_jump(s: &mut State, input: &Input, t: &Tuning, kind: JumpType, dt: f32) {
    // The ground punch's fall: each update the body's pull grows by the punch's acceleration,
    // up to its limit, with no time step in it (so it was tied to the frame rate). [game]
    let punching = s.action.as_ref().is_some_and(|a| t.control.states[a.state].name.contains("GroundPunch"));
    if punching {
        let most = (t.ground_punch_speed_limit - t.gravity).max(0.0);
        s.punch_gravity = (s.punch_gravity + t.ground_punch_acceleration * dt / GAME_UPDATE).min(most);
    }
    let weapon = s.weapon_air();
    // The slow motion of a jump with the weapon set playing (`FUN_00872fe0`). The mode keeps
    // a time factor (`+0x28`). The body's gravity is the factor times its own, and when that
    // changes the vertical speed is rescaled so that what is left of the rise or fall keeps
    // its height; the mode's own step is dt times the factor's root. Both use the factor as
    // the update before left it. So up and down he moves as if time ran at the factor's
    // root: half speed at Bumblebee's 0.25. [game]
    if s.dilation != s.gravity_scale {
        s.vel.z *= (s.dilation / s.gravity_scale).sqrt();
        s.gravity_scale = s.dilation;
    }
    let slowed = s.dilation.sqrt();
    // The factor goes down while he is near the top: with no height noted yet, while he
    // rises or falls slower than the start speed, and the height he is at is then noted;
    // with one noted, while he is above it, whatever his speed. Else it goes back up to 1.
    // [game] [game: the other way in is `FUN_00721b50`, a state of class dc24f161 with value
    // 4 on the character's `+0x314`; not Bumblebee's weapon states, not built]
    let dilating = weapon
        && match s.dilation_from {
            None => s.vel.z.abs() < t.jump.weapon_dilation_start,
            Some(from) => s.pos.z > from,
        };
    if dilating {
        s.dilation_from.get_or_insert(s.pos.z);
        s.dilation = (s.dilation - t.jump.weapon_dilation_rate * dt).max(t.jump.weapon_dilation);
    } else {
        s.dilation = (s.dilation + t.jump.weapon_dilation_rate * dt).min(1.0);
    }
    if weapon {
        // The animation controller's turn comes before the mode's, as on foot.
        weapon_rules(s, input, t);
    }

    let jump = jump_kind(t, kind);
    let stick = input.stick.clamp_length_max(1.0);
    // What the stick asks for is kept up to date in the air, so the landing knows it.
    s.gait = gait_of(stick.length(), &t.locomotion);
    let horizontal = Vec2::new(s.vel.x, s.vel.y);
    // What the mode asks for each frame: the stick times the jump type's speed, turning toward
    // the stick at the type's turn speed. The long jump with the stick centred asks for its
    // speed along the way he is already going. [game]
    let asked = if stick.length_squared() > STICK_DEAD_SQUARED {
        if !weapon {
            s.yaw = turn_toward(s.yaw, yaw_of(stick), jump.turn_speed.to_radians() * dt * slowed);
        }
        stick * jump.speed
    } else if kind == JumpType::Long {
        horizontal.try_normalize().map_or(Vec2::ZERO, |direction| direction * jump.speed)
    } else {
        // Stick centred: the mode moves nothing, so the speed he took off with bleeds away. [trace]
        Vec2::ZERO
    };
    if weapon {
        // With the weapon set playing the mode does not turn him to the stick: it rebuilds
        // his matrix from the aim heading each update, stick or none, so he jumps sideways
        // and backwards facing his aim. [game]
        s.strafe.aim_yaw = input.aim.filter(|aim| aim.length_squared() > 1e-6).map_or(s.strafe.aim_yaw, yaw_of);
        s.yaw = s.strafe.aim_yaw;
    }
    // The clip's own root motion counts as well: the take-off clip pushes him forward, then
    // the in-air clip drifts a little. Seen in the recording as the extra gain in the first
    // five updates of a running jump and the creep forward in a standing one. [trace]
    // With the weapon set it is that set's clips' (next to nothing: the rise drifts half a
    // unit forward). [data]
    // The clips play on at their own rate in the slow motion [game, settled 2026-10-20]: the slow motion is
    // JumpMode's own factor (mode+0x28, 00872fe0: gravity times it, dt times its root), while 00544b60 steps the
    // animation controllers by the GLOBAL timescale 00dadea4, which 007797b0 sets each frame to the measured frame
    // ratio (elapsed x 60, smoothed by 0.3, clamped 0..3, stored at game state +0xc4 = 00dadea4; 1.0 only on its first
    // call): a real-time clock that the jump's factor does not touch. (Corrected 2026-10-20: an earlier text said 1.0.)
    let root = if weapon {
        let (moved, _) = weapon_layers(s, t, dt);
        let facing = s.facing();
        Vec2::new(facing.y, -facing.x) * moved.x + facing * moved.y
    } else {
        let along = match kind {
            JumpType::Standing | JumpType::Moving => {
                let takeoff = &t.locomotion.takeoff;
                if s.air_time < takeoff.duration() {
                    takeoff.speed(s.air_time, s.air_time + dt)
                } else {
                    t.locomotion.in_air_speed
                }
            }
            _ => 0.0,
        };
        s.facing() * along
    };
    s.air_time += dt;
    s.jump_time += dt;
    // What the mode asks for is a step of speed times its own, slowed, dt. [game]
    let wanted = asked * slowed + root;
    // Each of the game's updates the body's velocity moves the jump type's control share of the
    // way to what is asked. Read from the code and confirmed update by update in the recording
    // (a running jump climbs from 24 to 33 m/s in the air exactly this way). [game] [trace]
    let share = 1.0 - (1.0 - jump.control).powf(dt * CONTROL_FRAME_RATE);
    let blended = horizontal.lerp(wanted, share);
    (s.vel.x, s.vel.y) = (blended.x, blended.y);
}

/// FlyMode's enter (`00864d60`, step 10) and exit (`00865b80`, step 11) start `V2RPartialSet`'s
/// clip on layer 1 (`animation_set_start(0xa1c4e1d0, clip, 1, 0)`) when the character's
/// `specialAbilityVehicleWeaponFireFromAnim` is set (attributes `+0x390`,
/// `Tuning::vehicle_special_from_anim`). A character without the set (all but MegatronFlight)
/// starts nothing: the set lookup fails. [game]
fn fly_partial_start(s: &mut State, t: &Tuning, clip: &[u8]) {
    if !t.vehicle_special_from_anim {
        return;
    }
    let from = s.fly_partial.playing().unwrap_or((animrules::EMPTY_SET, 0));
    s.fly_partial.play(&t.rule_sets, crc32(b"V2RPartialSet"), crc32(clip), from);
}

/// Changes the motion mode, with what leaving the old one and entering the new one do. A mode
/// that is already running is not entered again (`FUN_00852150`). [game]
/// The length of the flyer's drop ray, metres [game 00b6b3b4 (the constant 35.0), 0086b230 uses
/// it for the drop probe (`notes\flight-contact.md` l.63, l.75)].
const FLY_DROP_RAY: f32 = 35.0;

/// One update of the flying form (`flight::Fly`: FlyMode's update 00864320 as far as it
/// is ported; what is left out is listed there). The world it sees: the floor under it, the
/// three rays along the nose (answered by `World::wheel_ray`) and the zone's flight boundary.
/// [stand-in] its body against the world: the robot's cylinder pushed out (below). settles: the flying body's contacts in the game (a physics body through 006db490); not read.
fn step_fly(s: &mut State, input: &Input, t: &Tuning, arena: &dyn World, dt: f32) {
    let Some(mut fly) = s.fly.take() else { return };
    s.fly_clock += dt;
    // [data] the binding list has a `FlyingStunt` action on the action button, and the
    // fly block a `stuntMoveButtonPressTime` no read code uses. [assumed] the press arms
    // the stunts for that long (see `flight::FlyInput::stunt`). Against it [game]: the enable
    // byte mode+0x4a1 is 1 from the constructor (00863970) and only a level's control state
    // 0xd73818a4 changes it (008013e0). The trigger 0086a590 gates on: no stunt running, inside
    // the flight boundary (mode+0x55), that byte, a zero control vector, the state object being
    // `stateFly` (0x085e3ca2, the plain flying state), and its caller 00866a70 on the hold button
    // being up (mode+0x4c, `notes\flight-forces.md`). Its stick is the input source's slot +0x30
    // through 00855910, the slot the steering reads, so as read a hard sideways stick would roll
    // with no button: that contradicts the step-ending return of `notes\flight-stunts.md` 2.4.
    // settles: what slot +0x30 of the player's input source returns in flight (a stunt vector
    // gated by stuntMoveButtonPressTime, or the plain stick), or a recording of a jet's stunts.
    s.stunt_armed = if input.buttons_hit & (1 << button::ACTION | 1 << button::CLIMB) != 0 {
        t.forms.fly.stunt_move_button_press_time
    } else {
        (s.stunt_armed - dt).max(0.0)
    };
    let stunt_before = fly.stunt;
    // [data] the binding list puts `FlyingAfterburn` on the jump button (the car's turbo)
    // and `FlyingStuntMode` on the drift and weapon-mode trigger.
    // The stick is the pad's own (the input source's slot +0x30, as 00864320 reads it).
    // [game] 00864320 reads FlyingAfterburn and FlyingBrake as PRESSES (input slot +0xc,
    // 0084f980); the brake's press is found by `Fly` from the trigger's state. Both caps take
    // the same status scale as the car's (0086a840 / 0086a950 against 0085f8b0).
    let cap_scale = crate::driving::cap_scale(s.damage_timers.slowed > 0.0, t.damage.slow_multiplier,
        s.damage_timers.bullet_slowed > 0.0, t.forms.fly.bullet_slow_speed_percent,
        s.special.as_ref().is_some_and(|a| matches!(a.kind, crate::special_types::SpecialType::Scout) && a.left > 0.0));
    let fly_input = crate::flight::FlyInput { stick: input.pad_stick, turbo: input.jump, brake: input.brake > STICK_DEAD, cap_scale: Some(cap_scale), invert_pitch: false, hold: input.drift, look: input.look, stunt: s.stunt_armed > 0.0, ai: None };
    // The rays along the nose (0086b230), answered by the world's ray.
    let walls = fly.wall_rays().map(|(from, to)| {
        arena.wheel_ray(from, to).map(|hit| (hit.point.distance(from), hit.normal))
    });
    // The drop ray (0086b230 l.343-351): straight down from `pos + 2 * nose`, 35 m [game]. Its hit is the
    // ground the low-speed rules measure S from (`Fly::height_over`, mode+0x78) and its normal's z the latch's
    // slope test (`Fly::ground_normal_z`, mode+0x354). A miss is no ground: the latch is off (no-hit branch
    // 0086c627-0086c65e), so the normal kept on a miss is never read. The hit object's +0x9e bit 0x80 sets the
    // 3 m latch (`three_metre_latch`, mode+0x4bc, 0086bceb-0086bd1f) [game]: `GroundHit::object_mask` is that
    // word (a level primitive's receive mask; 0 for the arena's own floor and boxes, and for a miss).
    let drop_origin = fly.pos + 2.0 * fly.forward();
    let drop = arena.wheel_ray(drop_origin, drop_origin - Vec3::Z * FLY_DROP_RAY);
    fly.ground_normal_z = drop.as_ref().map_or(1.0, |hit| hit.normal.z);
    fly.three_metre_latch = drop.as_ref().is_some_and(|hit| hit.object_mask & 0x80 != 0);
    let around = crate::flight::Surroundings { ground: drop.map(|hit| hit.point.z), walls };
    // The zone's flight boundary (`step_in_bounded`; None in the test arena). [game 0086ab90]
    let was_inside = fly.inside_boundary;
    // The turbo and brake run on the character's own timers (`State::fly_timers`). [game]
    fly.set_timers(s.fly_timers);
    let turbo_before = fly.turbo_left;
    // The boundary is tested at the flyer's camera target point (`flycam::flyer_point`, which is
    // 0071a460 for a flyer: the root's x and y, and the lower of the root's z and the holder's
    // bottom). [game 0071a460, 007e7830 l.7] The root is the body here (`s.pos`, the same origin
    // `car_presentation` gives the camera); the holder's z is the body's z. [data] The half height
    // is the vehicle avatar's (`avatarDescArray[1]`, the flyers' last row; every flyer has two).
    // [assumed] the body and the rotation as they stand before this update's motion: the order of
    // the boundary test within the game's update is not read. OPEN settles: the caller of the
    // per-update check 007e7830 (through the boundary's vtable +8; unread, notes/flight-boundary.md
    // section 12.3 and its open item 2) and where that caller sits against FlyMode's update 00864320.
    // Part 2 (2026-10-20): the boundary object's vtable is 00b60908 (stored by the constructors 007e74f0 /
    // 007e7540, the factories 007e7780 and 01196e50 via 00adbf70 / 00adbfd0); slot +8 is 007e7830. No direct
    // caller of 007e7830, and the owner's list dispatcher was not found by a grep of `+ 8)` calls. Not
    // settled in this part: a trace of the boundary object's update, or the owner's update list.
    // Part 3 (lead, 2026-10-20): the vtable is 00b60908 (slot 0 dtor 007e74f0, slot 2 = 007e7830,
    // slot 4 = 007e75c0 the activation); the ctor's 00578550 links the object into the game state's
    // ring at +0x28 (00dade08, 005794e0); 005a9d00 only clears that ring. settles: the walker of the
    // state's +0x28 ring (reached through the 005a9cc0 pointer, so no immediate) and its priority
    // against the characters' 5.
    let up_z = (fly.rotation() * glam::Vec3::Z).z;
    fly.boundary_point = Some(crate::flycam::flyer_point(s.pos, fly.pos.z, up_z, t.vehicle_height * 0.5));
    fly.step_in_bounded(&fly_input, &t.forms.fly, &around, s.flight_boundary.as_ref(), dt);
    s.fly_timers = fly.timers();
    // The rotor speed (the helicopter steering's last block) and 0086cbf0, the moving parts,
    // late in the update on the angles the steering left. [game 00867f20 SM, 00864320 step 21]
    if let Some(lift) = fly.rotor_lift {
        s.flight_surfaces.steer_rotor(lift, dt);
    }
    s.flight_surfaces.step(Vec3::new(fly.pitch, fly.roll, fly.heading), &t.forms.fly, dt);
    // [game] 00864320: a turbo it starts plays rumble preset 0xe (`play_preset_rumble`), the
    // car's turbo preset.
    if turbo_before <= 0.0 && fly.turbo_left > 0.0 {
        preset_rumble(s, t, rumble::preset::VEHICLE_TURBO);
    }
    if was_inside != fly.inside_boundary {
        eprintln!("flight boundary: {}", if fly.inside_boundary { "re-entered" } else { "left" });
    }
    // A stunt's start tells the state machine, whose stunt state plays its clip (0086a590); the
    // clip's traction ([1, 1, 1] at its first key) holds the stick off and its root drives the jet;
    // its [0, 0, 0] last key ends the stunt (`flight::Fly::traction`, 00866900). [game] [data: the
    // stunt clips of the two jets that can stunt, starscream and firestorm, end so]
    if fly.stunt != stunt_before {
        s.messages.extend(fly.stunt.message());
        s.stunt_time = 0.0;
    } else if fly.stunt != crate::flight::Stunt::None {
        s.stunt_time += dt;
    }
    // While the control vector is set the body's control step pins the velocity on each axis it
    // names to the clip's: v += c (wanted - v), wanted the clip's root motion over the update
    // (holder control 00852220 -> 006db490). [game] The flight's own step has moved him already, so
    // the position takes the difference too. [assumed: the clip's root frame is turned by his
    // heading only, as the root motion on foot is] settles: the flyer's root-frame turn in the game (the fly step's clip root, not located), or a `Vehicle_RollLeft` recording. (`Vehicle_RollLeft` slides him 50 m to his
    // left in a second; `Vehicle_LoopFull` climbs 30 m and comes back 3 m ahead) [data]
    if fly.control != Vec3::ZERO {
        if let Some(wanted) = action_root_velocity(s, t, fly.heading, dt) {
            let pinned = fly.vel + fly.control * (wanted - fly.vel);
            fly.pos += (pinned - fly.vel) * dt;
            fly.vel = pinned;
        }
    }
    // [stand-in] the body itself against the world (the game's is a physics body with its
    // own contacts): the robot's cylinder pushed out of what it is in (the same stand-in as the one in `step_fly`'s
    // header above). settles: the flying body's contacts in the game (006db490 with a flight holder); not read.
    if arena.push_out(&mut fly.pos, t.robot_radius, t.robot_height) {
        fly.vel = (fly.pos - s.pos) / dt.max(1e-6);
    }
    (s.pos, s.vel) = (fly.pos, fly.vel);
    (s.yaw, s.pitch, s.roll) = (fly.heading, fly.pitch, fly.roll);
    s.speed = fly.vel.length();
    s.fly = Some(fly);
}

/// The action clip's root travel over this update as a velocity in the world, turned by
/// `heading`: what the body's control step pulls toward (006db490's `(animated - origin) / dt`).
fn action_root_velocity(s: &State, t: &Tuning, heading: f32, dt: f32) -> Option<Vec3> {
    let action = s.action.as_ref()?;
    let data = t.actions.clips.get(&action.clip)?;
    if dt <= 1e-6 || action.tick < action.last_tick {
        return None;
    }
    let local = action.root_travel(data) / dt;
    Some(Quat::from_rotation_z(heading) * local)
}

/// HoverMode's update (`FUN_0086f960`, `notes\flight-hover.md`). [game] With the stick at
/// rest the whole velocity, height included, is multiplied by `hoverDeceleration` each
/// update (no time step in it); with the stick, `moveAcceleration` along it and the speed
/// cut to `maxMoveSpeed`. The player has no way up or down in it (that part is the AI's
/// alone), and no lean. Its timer (`FUN_00735a00`) sends `HoverDone` once `maxHoverTime`
/// (where not negative) has run.
/// [game 0071eb80, called by HoverMode enter 00871514] nothing pulls him down while he hovers:
/// the enter zeroes the body's gravity, and the body's flag 0x200000 (set by that enter too)
/// only gates the angular write-back (006ead90), not gravity. The stick is the camera's;
/// he faces his aim while aiming and keeps his heading otherwise (the original turns
/// him toward the camera's target, `FUN_0071a2e0`).
/// [stand-in] the body against the world, as in flight: the floor and `push_out`. settles: the hover body's contacts (006db490) with its gravity zeroed (0071eb80, settled); not read.
/// Not in: the homing variant (`FUN_00870aa0`), the water test, `FX_hover`.
fn step_hover(s: &mut State, input: &Input, t: &Tuning, arena: &dyn World, dt: f32) {
    let h = &t.forms.hover;
    if input.stick.length() <= 0.01 {
        s.vel *= h.hover_deceleration.powf(dt / GAME_UPDATE);
    } else {
        s.vel += input.stick.extend(0.0) * h.move_acceleration * dt;
        s.vel = s.vel.clamp_length_max(h.max_move_speed);
    }
    if let Some(aim) = input.aim.filter(|aim| aim.length_squared() > 1e-6) {
        s.yaw = yaw_of(aim);
    }
    if s.hover_weapon {
        // The hover weapon state: the weapon set's clips play over HoverMode as over the weapon
        // jump, the animation controller's turn first, and their root moves nothing: the mode's
        // update owns the body [game 0084e3c0, 0086f960], and the set's one hover clip,
        // `Hover_Weapon_Idle`, keeps its root at the origin [data: `clip_root`, Starscream,
        // Jetfire, Blackout]. He faces the aim, which HoverMode's heading in StateWeapon is
        // (0086f960 step 10, owner +0x260). [game]
        weapon_rules(s, input, t);
        s.strafe.aim_yaw = s.yaw;
        let _ = weapon_layers(s, t, dt);
    }
    // The body's control step: on the axes the clip's traction names (`Trans_V2R_Hover`'s
    // [0, 0, 1] until its [0, 0, 0] last key) the clip's root drives the velocity, v += c (wanted
    // - v). [game 00852220 -> 006db490; 0086f960 never reads the vector] Only while a clip with a
    // root plays: with no clip the vector is left alone. [data, settled 2026-10-20] Neither the no-clip
    // origin nor the frame the root is turned into can show: the only traction a HoverMode receives in
    // the 13 packs is Trans_V2R_Hover's [0,0,1], ended by its own [0,0,0] key before the clip ends
    // (notes\clip-events-traction-motion.md section 2; Starscream's two [1,0,1] keys, starscream.txt
    // 730622 / 731009, are the jet proxy's RollLeft / RollRight, played in flight stunts, FlyMode), and a
    // turn about z leaves the z axis the vector drives unchanged.
    if s.hover_control != Vec3::ZERO {
        if let Some(wanted) = action_root_velocity(s, t, s.yaw, dt) {
            s.vel += s.hover_control * (wanted - s.vel);
        }
    }
    let before = s.pos;
    s.pos += s.vel * dt;
    let floor = arena.floor(s.pos.x, s.pos.y, before.z);
    if s.pos.z < floor {
        (s.pos.z, s.vel.z) = (floor, s.vel.z.max(0.0));
    }
    if arena.push_out(&mut s.pos, t.robot_radius, t.robot_height) {
        s.vel = (s.pos - before) / dt.max(1e-6);
    }
    s.speed = s.vel.truncate().length();
    s.gait = Gait::Idle;
    s.hover_time += dt;
    if h.max_hover_time >= 0.0 && s.hover_time > h.max_hover_time {
        s.messages.push(HOVER_DONE);
    }
}

fn set_mode(s: &mut State, input: &Input, t: &Tuning, mode: Mode) {
    if mode == Mode::Fly && (s.mode != Mode::Fly || s.fly.is_none()) {
        // FlyMode's enter (00864d60): the velocity he has is kept. [game] Only when the mode
        // changes, as the doc above says of every mode (`FUN_00852150`); before, a repeated
        // request re-entered it and reset its timers and its avatar blend.
        s.fly = Some(crate::flight::Fly::enter(s.pos, s.vel, s.yaw));
        s.flight_surfaces.enter();
        s.fly_clock = 0.0;
        fly_partial_start(s, t, b"Trans_R2V_Partial");
    } else if s.mode == Mode::Fly {
        // Its exit (00865b80) leaves him the velocity and stands him upright. [game]
        s.fly = None;
        (s.pitch, s.roll) = (0.0, 0.0);
        if mode != Mode::Fly {
            fly_partial_start(s, t, b"Trans_V2R_Partial");
        }
    }
    if s.mode == Mode::Drive && mode != Mode::Drive {
        // DriveMode's exit (0085ed50 l.154-175) calls align_z_matrix on the car's own root affine
        // with z = (0, 0, 1): he is stood up in the same update, his heading kept, with no blend.
        // [game 0085ed50]
        (s.pitch, s.roll) = (0.0, 0.0);
    }
    if mode == s.mode {
        return;
    }
    let old = s.mode;
    let landed = matches!(old, Mode::Jump(_));
    if landed && mode == Mode::Move {
        // The jump mode's exit into MoveMode caps the speed along the ground at the
        // walk-run boundary, keeping its direction (run 1 shows the 5.0). [game] [trace]
        let flat = Vec2::new(s.vel.x, s.vel.y).clamp_length_max(t.moving.walk_run_speed);
        (s.vel.x, s.vel.y) = (flat.x, flat.y);
    }
    if landed && mode == Mode::Strafe {
        // Into StrafeMode the jump mode's exit does the same with the strafe speed. [game]
        let flat = Vec2::new(s.vel.x, s.vel.y).clamp_length_max(t.strafe_speed);
        (s.vel.x, s.vel.y) = (flat.x, flat.y);
    }
    if old == Mode::Drive {
        // [game: 0085ed50] Drift turbo is removed; standard turbo is retained.
        s.car.drift_turbo = crate::driving::DriftTurbo::default();
        s.turbo_fx_on = false;
    }
    if old == Mode::Strafe {
        // StrafeMode's exit (`FUN_00876ea0`): `I_IS_STRAFING` and `F_STRAFE_ANG` go to 0
        // and the spine's twist is taken off. The weapon set's clips are not the mode's:
        // they play on until a state starts another set (`enter_state`). [game]
        let old = std::mem::take(&mut s.strafe);
        s.strafe = Strafe { aim_yaw: old.aim_yaw, clip: old.clip, tick: old.tick, layers: old.layers, ..Strafe::default() };
    }
    if landed {
        // The jump mode's exit puts the body's gravity back and its time factor to 1
        // (`FUN_00874850`). [game]
        s.dilation = 1.0;
        s.gravity_scale = 1.0;
    }
    s.mode = mode;
    match mode {
        Mode::Strafe => {
            s.layers.clear();
            enter_strafe(s, input, t, old);
        }
        Mode::Stand | Mode::Move => {
            // BasicMotionMode's enter sets the root control to (1, 1, 1) [game 00856cc0]: on the
            // update it starts his height follows the clip, whose root does not move in z [data:
            // the flyers' `IdleStandSet/Idle`, `clip_root`], so a rise he brings in ends there; the
            // idle state's update hands gravity back at (1, 1, 0) once he is off the ground
            // (0087d790). Out of flight the fall state's JumpMode starts as a high standing jump
            // from the companion (008740e0 l.100-104) and the idle state follows one update later
            // on the lagging ground flag: without this he rose 14 m in the standing mode.
            if mode == Mode::Stand {
                s.vel.z = 0.0;
            }
            s.speed = Vec2::new(s.vel.x, s.vel.y).dot(s.facing()).max(0.0);
            if landed || old == Mode::Climb {
                s.layers.clear();
                if mode == Mode::Move {
                    // Landing into the walk set plays the landing that runs on; after a fall
                    // of a second or more, the big one (`animRuleLongFall`). [data] [trace]
                    // The walk set's entry rules have picked the landing by now (the state
                    // starts the set before the mode): the big one for a fall of a second
                    // or more, or of three quarters of one after a long jump. [game]
                    let long_fall = match s.base.current(&t.rule_sets) {
                        Some(clip) if s.base.set == WALK_SET => clip.name == "BigJump_LandRun",
                        _ => t.locomotion.long_fall > 0.0 && s.fall_time >= t.locomotion.long_fall,
                    };
                    let pace = if long_fall { Pace::BigLandRun } else { Pace::LandRun };
                    s.layers.push(Layer { pace, weight: 0.0, time: 0.0 });
                }
            } else if old == Mode::Unfold {
                // Out of the change back to the robot the walk or run carries on from the
                // speed he has, not from a standstill. [trace]
                s.layers.clear();
                let walk = &t.locomotion;
                if mode == Mode::Move {
                    let (pace, own) = if gait_of(input.stick.length(), walk) == Gait::Run {
                        (Pace::Run, walk.run_speed)
                    } else {
                        (Pace::Walk, walk.walk_speed)
                    };
                    s.layers.push(Layer { pace, weight: (s.speed / own.max(0.1)).clamp(0.0, 1.0), time: 0.0 });
                }
            } else if old != Mode::Stand && old != Mode::Move {
                s.layers.clear();
            }
        }
        Mode::Drive => {
            s.layers.clear();
            // The wheels start out rolling with the ground, so nothing grabs or pushes at first.
            let forward = Vec2::new(s.vel.x, s.vel.y).dot(s.facing());
            let d = &t.drive;
            s.vehicle_gearbox=Some(crate::sound::Gearbox::new(t.drive_gears.clone(),d.engine_torque,
                d.front_wheel_radius,crate::vehicle::mass(d),d.max_speed,d.turning_velocity_delta));
            let roll = |radius: f32| forward / radius.max(0.1);
            s.car = Car {
                spin: [roll(d.front_wheel_radius), roll(d.front_wheel_radius), roll(d.rear_wheel_radius), roll(d.rear_wheel_radius)],
                // The update the car mode starts does not push: run 1 reads 0.000 on it and
                // the car moves from the next (t 433.81). [trace]
                clock: -GAME_UPDATE,
                // [game: 0085e130] Enter starts the trigger filter at full, not zero.
                filtered_trigger: 1.0,
                ..Car::default()
            };
        }
        Mode::Unfold => {
            s.layers.clear();
            s.unfold_run = input.stick.length() > UNFOLD_RUN_STICK;
            s.speed = Vec2::new(s.vel.x, s.vel.y).length();
        }
        // HoverMode's enter (`FUN_008713c0`) starts its timer afresh. [game]
        Mode::Hover => {
            s.layers.clear();
            s.hover_time = 0.0;
        }
        Mode::Jump(_) | Mode::Action | Mode::Climb | Mode::Fly => s.layers.clear(),
    }
}

/// JumpMode's enter (`FUN_008740e0`): which jump, from what was playing, the mode before and
/// the stick, and the launch when there is one. `set` is the set playing as the mode starts:
/// the fall state starts its set first, the jump state after. [game]
fn enter_jump_mode(s: &mut State, input: &Input, t: &Tuning, set: Option<&str>, on_ground: bool) {
    if matches!(s.mode, Mode::Jump(_)) {
        return;
    }
    let old = s.mode;
    let horizontal = Vec2::new(s.vel.x, s.vel.y);
    let stick_squared = input.stick.length_squared();
    let kind = match set {
        // A jump that starts while the climb set plays: vertical speed cleared, a push along
        // his facing on top of what he was moving at, the climb jump's height. [game]
        Some("ClimbSet") => {
            let push = s.facing() * CLIMB_JUMP_PUSH;
            s.vel = Vec3::new(s.vel.x + push.x, s.vel.y + push.y, 0.0);
            Some(JumpType::Climb)
        }
        Some("GroundPunchSet") => None,
        // Out of the car on the ground: the high standing jump from a crawl, the long jump
        // otherwise, keeping the car's speed.
        _ if matches!(old, Mode::Drive | Mode::Unfold) && on_ground => Some(if horizontal.length_squared() <= VEHICLE_EXIT_SLOW_SQUARED {
            JumpType::HighStanding
        } else {
            JumpType::Long
        }),
        // Out of FlyMode (the player's: the port's robot always is): LONG_JUMP at a ground speed
        // squared of 25 or more, else the fall; neither launches (008740e0 l.111-118, both jump to
        // the no-launch label). [game]
        _ if old == Mode::Fly => {
            let long = horizontal.length_squared() >= FLY_EXIT_LONG_SQUARED;
            set_mode(s, input, t, Mode::Jump(if long { JumpType::Long } else { JumpType::Fall }));
            s.jump_type = jump_type_hash(if long { JumpType::Long } else { JumpType::Fall });
            s.dilation = 1.0;
            s.gravity_scale = 1.0;
            s.dilation_from = None;
            s.jump_time = 0.0;
            s.air_time = 0.0;
            s.launch_z = s.pos.z;
            s.apex_z = s.pos.z;
            return;
        }
        // Falling, or off the ground: no launch.
        Some("FallSet") => None,
        _ if !on_ground => None,
        _ if stick_squared <= STICK_DEAD_SQUARED => Some(JumpType::Standing),
        _ => {
            // A running jump at full stick takes off at exactly MoveMode's top speed, whatever
            // point of the stride the run clip was at.
            if old == Mode::Move && stick_squared > FULL_STICK_SQUARED && horizontal.length_squared() > 1e-4 {
                let v = s.facing() * t.moving.max_speed;
                (s.vel.x, s.vel.y) = (v.x, v.y);
            }
            // Out of StrafeMode likewise at the strafe speed, the way he was going. [game]
            if old == Mode::Strafe && stick_squared > FULL_STICK_SQUARED && horizontal.length_squared() > 1e-4 && t.strafe_speed > 0.1 {
                let v = horizontal.normalize() * t.strafe_speed;
                (s.vel.x, s.vel.y) = (v.x, v.y);
            }
            Some(JumpType::Moving)
        }
    };
    set_mode(s, input, t, Mode::Jump(kind.unwrap_or(JumpType::Fall)));
    // The animation variable the mode writes, which outlives it. [game: the ground punch's
    // fall is type 2494da83]
    s.jump_type = if set == Some("GroundPunchSet") { 0x2494_da83 } else { jump_type_hash(kind.unwrap_or(JumpType::Fall)) };
    // The mode's time factor and the height its slow motion began at start afresh. [game]
    s.dilation = 1.0;
    s.gravity_scale = 1.0;
    s.dilation_from = None;
    s.jump_time = 0.0;
    s.air_time = 0.0;
    s.launch_z = s.pos.z;
    s.apex_z = s.pos.z;
    if let Some(kind) = kind {
        // Straight up against gravity at the speed that peaks at the jump's height.
        s.vel.z = (2.0 * t.gravity * jump_height(t, kind)).sqrt();
    }
}

/// The motion mode classes the control states name. [game]
/// What a melee attack sets the character's fight timer to (`State::fight`). [game]
const FIGHT_TIME: f32 = 4.0;
const ATTACK_MODE: u32 = 0xcd2f_4b0c;
const JUMP_MODE: u32 = 0x6da1_d12d;
/// The unnamed first mode: the clips move him.
const FIRST_MODE: u32 = 0x1ad6_ae68;
/// HoverMode (`notes\flight-hover.md`).
const HOVER_MODE: u32 = 0x7077_6cef;
/// StrafeMode, weapon mode on foot.
const STRAFE_MODE: u32 = 0x3c95_6e02;
/// DriveMode, the car (`notes\bumblebee-mechanics.md`).
const DRIVE_MODE: u32 = 0xe2b2_2eb4;
/// The message its timer sends when `maxHoverTime` has run (`FUN_00735a00`). [game]
const HOVER_DONE: u32 = 0xb587_f609;
const CLIMB_MODE: u32 = 0xd706_8c31;
const UNFOLD_MODE: u32 = 0xf081_d468;
/// A jump that starts while the climb set plays is pushed this hard along his facing. [game]
const CLIMB_JUMP_PUSH: f32 = 20.0;
/// Blend time for clips whose own is negative ("none given"): the clip starter
/// `FUN_00748f00` takes 0.1 for any blend time under 0. [game]
const DEFAULT_CROSSFADE: f32 = 0.1;

/// The control state machine decides every change of what he is doing: standing, moving,
/// jumping, falling, the change of form both ways, weapon mode, the attacks, the dodge, the
/// climb. Each update it runs the current state's own update, then takes the transition the
/// graph allows, and the state it enters sets the motion mode. [game: the graph, the
/// detectors, the choice, and the enter and update of the states named in `enter_state`]
/// The StateBasic hover states (stateHover, stateHoverEnter: global_states 665, 678) are built and
/// enter when the connections ask; StateWeapon's hover (stateWeaponModeHover, 1094 and 8709) runs on
/// HoverMode with the weapon set over it (`State::weapon_hover`). [stand-in: the carrying states (StateInteract, refused) are
/// not built] The refusal hides a real path: statePickUpHuman sits in stateMachinePickUpHuman
/// (global_states 10424), machine [7] of stateMachineListAudi (12189, 13 machines), entered through
/// connectionEnvPickUpHuman's env test a21955a2, which passes when the interact target is of type
/// INTERACT_TYPE_VIP 0xe081c00e (0084cee0 l.42-62, wave 28 P1). The playable Audi's
/// attributes (50721) point at object 0x9420, which IS the root stateMachineListAudi (bnxglobal.str#16
/// root table, data offset 0x9420) [data, 2026-10-20]: the playable Audi has the pickup. It needs a VIP
/// interact target, and VIPs are mission objects (missions are out of scope since 2026-10-18), so the
/// arena and stages never offer one. settles: statePickUpHuman's enter and update (0087dbb0's family),
/// to be built if a VIP is ever placed on a stage.
fn control(s: &mut State, input: &Input, t: &Tuning, arena: &dyn World, dt: f32) {
    let graph = &t.control;
    let messages = std::mem::take(&mut s.messages);
    if s.runner.state.is_none() {
        // He starts standing: the character's set-up (`00716890`, 00716cbf..00716d9b) hands the
        // machine (`00850a70`, owner +0x314) `stateIdle`, or `stateDrive` (canDrive, attr+0xa4) /
        // `stateFly` (canFly, attr+0xa5) when a local flag (esp+0x17, its source not read) asks
        // for a start in the vehicle form. [game] The port always starts on foot.
        let Some(idle) = graph.find("stateIdle") else { return };
        s.runner.enter(graph, idle);
        base_enter(s, input, t, IDLE_SET, (0, 0));
    }
    if let Some(full) = s.hit_pending.take() {
        let permission = if full { 1 << 1 } else { 1 << 2 };
        let allowed = s.runner.state.is_some_and(|index| graph.states[index].can_go_to & permission != 0);
        if allowed {
            if full {
                if let Some(next) = graph.find("stateHitReact") {
                    let on_ground = s.ground_flag();
                    enter_state(s, input, t, next, None, on_ground);
                    s.previous_state = s.runner.state;
                    s.runner.enter(graph, next);
                }
            } else {
                // The character's animation message handler starts layer 2 immediately;
                // the control overlay is requested for the next update. [game]
                let mut layer = std::mem::take(&mut s.hit_partial);
                let from = layer.playing().unwrap_or((animrules::EMPTY_SET, 0));
                layer.enter_partial(&t.rule_sets, crc32(b"HitReactPartialSet"), from, playing_clip(s, t), &|v| rule_value(s, input, v));
                s.hit_partial = layer;
                s.runner.pending = graph.find("stateHitReactOverlay");
            }
        }
    }
    if std::mem::take(&mut s.stun_pending) && s.damage_timers.stunned > 0.0 &&
        s.runner.state.is_some_and(|index| graph.states[index].can_go_to & (1 << 3) != 0) &&
        let Some(next) = graph.find("stateStun") {
        let on_ground = s.ground_flag();
        enter_state(s, input, t, next, None, on_ground);
        s.previous_state = s.runner.state;
        s.runner.enter(graph, next);
    }
    let Some(current) = s.runner.state else { return };
    let climbing = s.climb.is_some();

    // The pad as the detectors see it. The movement's own inputs are folded in under the
    // game's button numbers. [data export\data\global_states.txt: all 206 `cameraRelative: true` stick detectors have the full AngleMin -360 / AngleMax 360 range (grep), and the two restricted ones (120..240) are `cameraRelative: false`, so the stick's frame changes no match; the roster's other packs carry no such flag (work\survey\weapons, grep)]
    let facing = s.facing();
    let right = Vec2::new(facing.y, -facing.x);
    let mut pad = Pad { down: input.buttons_down, hit: input.buttons_hit, left: input.stick, left_character: Vec2::ZERO };
    pad.left_character = Vec2::new(input.stick.dot(right), input.stick.dot(facing));
    pad.set(button::JUMP, input.jump_held || input.jump, input.jump);
    // The jump out of the car is on the jump button in the game's bindings. [data]
    pad.set(button::TRANSFORM_JUMP, input.jump_held || input.jump, input.jump);
    pad.set(button::TRANSFORM_HOVER, input.jump_held || input.jump, input.jump);
    // The two attacks out of the car: the slam is on the melee button, the punch on the
    // action (climb) button. [data]
    let held = |b: u32| (input.buttons_down >> b & 1 != 0, input.buttons_hit >> b & 1 != 0);
    let (slam, punch) = (held(button::ATTACK_FAST), held(button::CLIMB));
    // The climb lock (owner +0x3f8) is read here as the machine sees it. It is cleared after the
    // machine, in `update`, where the body step clears it (00852220's end, with Climb not down). [game]
    let climb_down = punch.0;
    let regrab_locked = s.climb_regrab_lock;
    pad.set(button::TRANSFORM_SLAM, slam.0, slam.1);
    pad.set(button::TRANSFORM_PUNCH, punch.0, punch.1);
    pad.set(button::WEAPON_MODE, input.aim.is_some(), false);
    pad.set(button::VEHICLE_MODE, input.vehicle > R2_DOWN, false);
    // The flying form is on the same trigger as the car: the `Controller` bindings give
    // `FlyingMode` and `VehicleMode` the same entry, 25394345 (R2). [data global_tuning.txt]
    pad.set(button::FLYING_MODE, input.vehicle > R2_DOWN, false);

    s.clip_handover = None;
    if s.action.is_some() {
        play_action(s, t, &pad, dt);
        special_chain(s, t);
    }
    s.runner.update(graph, &pad, dt);
    let on_ground = s.ground_flag();
    // The drive states' own update: until the car mode runs, the first half of the change
    // into the car keeps MoveMode while the stick does not line up with his motion. [game]
    // The fly exit state's update (`FUN_0087c910`): once the jump mode it started has him
    // on the ground, the drive companion mode takes over (`notes\flight-camera-exit.md`
    // B2, B5). [game]
    if graph.states[current].class == "StateFlyExit" && matches!(s.mode, Mode::Jump(_)) && on_ground {
        set_mode(s, input, t, Mode::Unfold);
    }
    if graph.states[current].class == "StateDrive" && s.mode != Mode::Drive {
        drive_or_move(s, input, t);
    }
    // The jump state's: after the jump up off a wall, the catch onto the ledge above. [game]
    if graph.states[current].class == "StateJump" && s.previous_state.is_some_and(|p| graph.states[p].name == "stateClimbJumpUp") {
        ledge_catch(s, t, arena, true);
    }
    // The idle state's (`FUN_0087d790`) sets the first mode's control to (1, 1, 1) while he
    // is on the ground (height over it, owner `+0x334`, under 0.01) and nothing is pushing
    // him (`+0x34c`), else (1, 1, 0): standing, the body's height follows the animated
    // one, so gravity builds no speed. The body here keeps `vel.z` at zero on the ground in
    // every mode, which comes to the same; nothing pushes him yet. [game]

    let floor = arena.floor(s.pos.x, s.pos.y, s.pos.z);
    let branch = if s.stun_full {
        if s.damage_timers.stunned <= 0.0 { 100 } else { 1 }
    } else if s.hit_full {
        let done = if s.hit_flyback { s.base.set != crc32(b"HitReactionSet") }
            else { s.base.current(&t.rule_sets).is_some_and(|c| !c.looping && s.base.tick >= c.ticks) || s.base.set != crc32(b"HitReactionSet") };
        if done { 100 } else { 1 }
    } else { match (&s.climb, &s.action) {
        (Some(climb), _) => if climb.finished { 100 } else { 1 },
        (None, action) => action.as_ref().map_or(1, |a| if a.finished { 100 } else { a.branch }),
    }};
    let can_branch = s.action.as_ref().is_some_and(|a| a.can_branch);
    // The climbable test, from where he is along his facing. [game]
    let wall = if climbing || s.mode == Mode::Drive {
        None
    } else {
        climb::find(arena, s.pos, s.facing(), t.robot_radius, t.robot_height, t.jump.standing.height)
    };
    // EnvInsideFlightBoundary is mode+0x55 == 0 while he flies; TRUE when he is not flying.
    // It gates connectionListFlyRelease and connectionListFlyToAirPunch. [game 0084d34f]
    // (notes/flight-boundary.md section 8 and A6)
    let inside_boundary = s.fly.as_ref().is_none_or(|f| f.inside_boundary);
    let environment = |test: u32| match test {
        env::CLIMBABLE => wall.is_some() && !regrab_locked,
        env::INSIDE_FLIGHT_BOUNDARY => inside_boundary,
        // Nothing to carry or fly out of yet. [stand-in] settles: the two envTests' functions (CAN_SWITCH_AVATAR, and NOT_HOLD_VIP 0x14fd46e2, set in FUN_0084cee0 per notes/state-machine.md); level and mission scope, not read.
        env::CAN_SWITCH_AVATAR | env::NOT_HOLD_VIP => true,
        _ => false,
    };
    let facts = Facts {
        on_ground,
        z_velocity: s.vel.z,
        holding: false,
        branch,
        attack_can_branch: can_branch,
        // Character flag 0x20, which the drive mode's update (`FUN_0085cad0`) sets once it
        // has run `advTransformDelay` and its enter and exit clear. [game]
        // In flight: FlyMode's update sets the same flag (bit 0 of `+0x150`, flag 0x20) once its
        // own clock `mode+0x4a4` reaches `advTransformDelay` (00d40b74) [game 00864320,
        // `notes\flight-update.md` step 20]. The clock is `State::fly_clock` (`+= dt` in `step_fly`).
        // [game] The order is the state machine first, then the mode, in the same update
        // (`notes\state-machine.md` "Run 1 order"; `advance` calls `control` before `base_step`).
        // The mode bumps the clock and sets the flag in its update (00864320 step 20), so the
        // state machine reads the flag in the NEXT update, which is what `fly_clock` gives here.
        can_adv_transform: (s.mode == Mode::Drive && s.car.time >= t.transform_delay)
            || (s.mode == Mode::Fly && s.fly_clock >= t.transform_delay),
        ground_distance: s.pos.z - floor,
        env: &environment,
        messages: &messages,
    };
    // Whether a state agrees to be entered (slot 11 of its table). [game for the dash and
    // the drive; the others have none, or are not built]
    let (dash_ready, special_ready) = (s.dash_cooldown <= 0.0, s.special_cooldown <= 0.0);
    // [game 0087c550] the drive / fly may-enter reads only the DISABLE_VEHICLE timer (+0x48). Read
    // 2026-10-20: 0087c550 reads `+0x234 -> +0x48` and nothing else. DISABLE_TRANSFORM's timer
    // (+0x4c) is counted down by 0072a550 (l.105-109) and read by its effect 0072c340 only to see
    // whether the effect is already on. No other reader of +0x4c was found, so it refuses nothing here.
    let vehicle_enabled = s.damage_timers.disabled_vehicle <= 0.0;
    // [game] StateAttack animName0 starts CombatSet through its entry rules (008788b0).
    let generic_attack = t.rule_sets.get(&crc32(b"CombatSet")).and_then(|set| {
        let (from_set,from_id)=playing_clip(s,t);
        animrules::entry(&set.incoming,from_set,from_id,&|v|rule_value(s,input,v))
    }).map(|(_,id)|id).filter(|id|t.actions.clips.contains_key(id));
    let may_enter = |state: usize| {
        let def = &graph.states[state];
        if def.overlay {
            return match def.class.as_str() {
                // The two firing states, on foot and in the car: an attack state's own
                // test (slot 11, `FUN_0076c530`) is always yes. [game]
                "StateAttack" => def.ranged,
                // The special (`FUN_008799c0`): not while its cooldown runs (his damage
                // object's `+0x84`). [game] Left out: its refusal in a multiplayer mode
                // that switches specials off.
                "StateAttackSpecial" => special_ready,
                "StateHitReact" => true,
                _ => false,
            };
        }
        match def.class.as_str() {
            "StateClimb" => wall.is_some(),
            // The pull-up: the climb mode again, so it only follows the climb.
            "StateBasic" if def.motion_mode == CLIMB_MODE => climbing,
            "StateAttack" => if def.anim==0 {generic_attack.is_some()} else {t.actions.clips.contains_key(&def.anim)},
            // [game 0076c530] StateBasic's own may-enter (slot 11 of 00b62a0c) is always yes, and so is
            // StateFall's (00b62b4c): the hover states (stateHover, stateHoverEnter) enter whenever the
            // game's connections ask. [game] No clip test: a clip the set lacks plays nothing (00879ef0
            // calls 0084ddb0, which returns at 01193520 with nothing started), and the enter still does
            // its mode switch (begin_action does it for a missing clip too). Not ported: the enter's
            // (1,1,0) for allowFallInBasicMotionMode, which only statePlayCustomAnimExit3 and
            // statePlayMPMeteoroidPopout set in global_states.txt [data] (`allow_fall` is unused in
            // this file); both are PlayAnimSet states of the mission scripts. A clip the robot's table lacks
            // enters silently, as in the game. [data, `core\examples\statebasic_clips_check.rs`, 2026-10-20]
            // Of the 27 StateBasic states with a clip, the ones absent from a robot's table are, on all 13:
            // the mission scripts' PlayAnimSet (12) and CSAInteractSet (2) states, and the other form's
            // states (the ground robots lack stateHover / stateHoverEnter / stateFlyTransAirPunch /
            // stateDriveExitRun; the flyers lack the drive exits, A and B too for the two with no car).
            "StateBasic" => true,
            "StateDash" => dash_ready && t.actions.named("DodgeFront").is_some(),
            // Its own test (`FUN_0087c550`) refuses while his damage object's
            // "vehicle disabled" timer (`+0x234` -> `+0x48`) runs: a hit flagged
            // `DISABLE_VEHICLE` starts it. [game]
            "StateDrive" => vehicle_enabled,
            // [game] StateFly's slot 11 is the drive state's own `0087c550` (table 00b62b8c);
            // StateFlyExit's is `0076c530`, always yes (table 00b62bd0) (`notes\flight-states.md`
            // table). `can_fly` is the port's guard, not the game's: only a flyer's machine has them.
            "StateFly" => vehicle_enabled && t.forms.can_fly,
            "StateFlyExit" => t.forms.can_fly,
            // `StateWeapon` (`FUN_0087fcf0`) refuses only while sniping, and only where the
            // data sets `canEnterOnlyIfNotSniping` (the weapon jump). No sniping yet. [game] It reads
            // no damage timer, so DISABLE_TRANSFORM does not refuse it.
            "StateIdle" | "StateMove" | "StateFall" | "StateJump" => true,
            // An ordinary special state (Megatron's gun, `stateAttackSpecialMegatron`, is not an
            // overlay): the same cooldown test as the overlay one above. [game] the class, not the
            // list, gives the table: both are `StateAttackSpecial` (table 00b629cc), whose
            // may-enter `008799c0` reads nothing of the overlay flag. Found 2026-10-12: without
            // this arm the press was refused and the special never started.
            "StateAttackSpecial" => special_ready,
            "StateWeapon" => true,
            "StateStun" => true,
            "StateHitReact" => true,
            // [data] What reaches here (global_states.txt): StateInteract (stateCarFlip,
            // statePickUpHuman) and StateSync's stateVIPHeld, which need world objects or the
            // mission (out of scope); StateDeath (entered by the damage path, not a connection);
            // and the grab and throw between characters, StateGrab / StateSync stateGrabbed /
            // StateThrown. The hover states are StateBasic and StateFall and are taken above.
            // [stand-in] the grab and throw are not built, so they are refused here (reached only if a grab message
            // enters the state: no robot pack names it, so no playable robot reaches this refusal [data:
            // global_states.txt 9461 is the machine's only occurrence; no `stateMachineGrabThrow` in
            // export\data\bumblebee.txt, starscream.txt or work\survey\weapons\*.txt; 2026-10-20]; the
            // message route is OPEN below). Read 2026-10-20:
            // StateGrab's and StateThrown's may-enter (slot 11 of vtables 00b62c10 and 00b62e58) is
            // 0076c530, always yes [game], so the refusal is only the unbuilt state. The machine
            // stateMachineGrabThrow is in no character's list in bnxglobal.str#16 (0087ca80, the ledger:
            // unused in the shipped data) [data]. OPEN: the message route into StateGrab (00850c80 maps
            // a grab message to it) is not read, so whether a shipped move sends one is unsettled; StateSync's
            // table is not located; the grab's and throw's modes (c5882131, 4a15f3ab) are not ported.
            _ => false,
        }
    };
    // Whether the current state lets him go (slot 8): the jump state only once the jump mode
    // has run a tenth of a second, and the fall state the same when it followed weapon mode's
    // jump. [game]
    let jump_young = matches!(s.mode, Mode::Jump(_)) && s.jump_time <= JUMP_LEAVE_TIME;
    // A plain state (`StateBasic`, not an overlay): with `finishOnAnimSetExit` it may be left,
    // and is done, once the playing set is no longer its own; else with `finishAnim` it may
    // be left only once its clip has finished (slot 8, `FUN_0087a0a0`), and with
    // `playAnimOnce` it is done when its clip has finished (slot 9, `FUN_0087a1b0`), which
    // sends it to the default state when no transition wins. A finished clip is `branch`
    // 100, as the connections see it. [game]
    let current_def = &graph.states[current];
    let basic = current_def.class == "StateBasic" && !current_def.overlay;
    let basic_left_set = current_def.finish_on_set_exit && current_def.anim_set != playing_set(s, t);
    let basic_done = basic && if current_def.finish_on_set_exit { basic_left_set } else { current_def.play_anim_once && branch == 100 };
    let may_leave = match graph.states[current].class.as_str() {
        "StateBasic" if basic => if current_def.finish_on_set_exit { basic_left_set } else { !current_def.finish_anim || branch == 100 },
        // An ordinary special with `finishOnSpecialAnimSetExit` (+0x60) waits until the layer has
        // left `SpecialAnimSet` (00879990): its last clip finished into another set
        // (`special_ended`). Megatron's tank special leaves by an always-on
        // `connectionIsFinished`, so this is what holds him in it. [game 00879990] [data]
        "StateAttackSpecial" if !current_def.overlay && current_def.finish_on_special_set_exit => special_ended(s, t, true),
        "StateStun" => s.damage_timers.stunned <= 0.0,
        "StateJump" => !jump_young,
        "StateFall" => !(jump_young && s.previous_state.is_some_and(|p| graph.states[p].name == "stateWeaponModeJump")),
        "StateHitReact" if s.hit_flyback => s.base.set != crc32(b"HitReactionSet") || s.pos.z < s.hit_start_z - 2.5,
        _ => true,
    };
    let basic_hit_waiting = s.hit_full && !s.hit_flyback && branch != 100;
    let allowed = |state: usize| {
        // A basic reaction refuses a fall until its clip finishes; other outgoing states
        // are allowed. Flybacks can also leave after a 2.5m drop. [game: FUN_0087d2b0]
        let basic_hit_can_fall = !basic_hit_waiting || graph.states[state].class != "StateFall";
        may_leave && basic_hit_can_fall && may_enter(state)
    };

    // The overlay state (`FUN_00878370`, at the start of a state's own transition step):
    // the one asked for last update takes its place and has its update. The ranged attack
    // state's (`FUN_00878740`) gives its fire button to the weapon manager: down, "trigger
    // held"; let go this update, "released" (which `weapons::Weapon` sees as `firing` going
    // false). Then its own transitions: `connectionReleaseWeaponModeAttack` ends it. [game]
    //
    // The special is an overlay state too (`stateAttackSpecial`), on top of an ordinary
    // state or of a firing state: its enter starts the ability (`FUN_00879110` ->
    // `FUN_0072afd0`), and being an overlay state with no clip it is done at once (its
    // slot 9, `FUN_00879950`), so it is left at the start of the next update. [game]
    let entered = s.runner.swap_overlay(graph);
    for state in [entered.overlay, entered.nested].into_iter().flatten() {
        if graph.states[state].class == "StateAttackSpecial" {
            start_special(s, t);
            if s.car_mode {
                start_vehicle_special(s, input, t, state);
            }
        }
    }
    s.firing = s.runner.overlay.is_some_and(|overlay| {
        let def = &graph.states[overlay];
        (def.ranged && pad.is_down(def.fire_button)) || (def.class == "StateHitReact" &&
            (s.car_mode || s.mode == Mode::Strafe || s.weapon_air() || s.weapon_hover()) &&
            pad.is_down(if s.car_mode { button::ATTACK_VEHICLE } else { button::WEAPON_FIRE }))
    });
    let done = |state: usize| graph.states[state].class == "StateAttackSpecial" ||
        (graph.states[state].class == "StateHitReact" && s.hit_partial.current(&t.rule_sets).is_none_or(|c| !c.looping && s.hit_partial.tick >= c.ticks));
    s.runner.update_overlay(graph, &pad, &facts, &may_enter, &done, dt);

    let Some(transition) = s.runner.pick(graph, &facts, &allowed) else {
        // Nothing to go to: a transition into an overlay state is looked for, if none is
        // running; it is entered at the start of the next update. A state's "may leave" is
        // not asked for these (`FUN_00878230`). [game]
        if let Some(over) = s.runner.pick_overlay(graph, &facts, &may_enter) {
            s.runner.ask_overlay(over);
        } else if basic_done || ((s.hit_full || s.stun_full || special_ended(s, t, graph.states[current].class == "StateAttackSpecial")) && branch == 100) {
            // A completed ordinary state requests the owner's default state (+0x318).
            // Character setup binds that default to stateIdle, even when another state
            // starts first. [game: FUN_00877d90, FUN_00850a70, 00716d8b..00716d9b]
            if let Some(next) = graph.find("stateIdle") {
                if current_def.name == "stateClimbPullup" {
                    ledge_catch(s, t, arena, false);
                }
                enter_state(s, input, t, next, wall, on_ground);
                s.previous_state = Some(current);
                s.runner.enter(graph, next);
            }
        }
        return;
    };
    let Some(next) = transition.output else { return };
    if graph.states[current].class == "StateAttack" {
        // The attack state's exit forgets whom it hit (`FUN_00878be0`). [game]
        s.hit_list.clear();
        s.surface_hit_list.clear();
    }
    // A plain state's exit (`FUN_0087a5d0`): leaving the pull-up, the ledge catch. [game]
    if current_def.name == "stateClimbPullup" {
        ledge_catch(s, t, arena, false);
    }
    // The climb state's exit (`FUN_0087a820`): Climb still down locks the climbable test.
    // [game]
    if current_def.class == "StateClimb" && climb_down {
        s.climb_regrab_lock = true;
    }
    enter_state(s, input, t, next, wall, on_ground);
    s.previous_state = Some(current);
    s.runner.enter(graph, next);
}

/// The special ability starts (`FUN_0072afd0`, called by the special state's enter): the
/// cooldown is set to `specialAbilityCooldown` (`FUN_0072bc40`), the script the character's
/// attributes name for the form he is in is sent to himself as a trigger message, and the
/// ability counts as on for `specialAbilityDuration`. [game] What the script sets off is
/// whatever on his own object goes by that name. [data] Bumblebee's (`specialAttack`, and
/// `specialAttackVehicle` in the car): an `ObjectSpawnerOnBone` that puts a stun shockwave
/// on him (an `Explosion` of no damage, flag `DAMAGE_STUN`, `stunTime` 2.5, a sphere of 15
/// growing from half size over 0.25 s), a `TriggerableParticle`
/// (`r_attack_emp_stun_bumblebee`), an `EffectSpawner` (`r_empstun_bumblebee`, a ring mesh)
/// and a `SoundPlayer`; the last three are for whoever draws and plays, through
/// `State::triggers`. Not in: the types 0 to 5 of other characters (his is -1), the weapon
/// a special may fire (`specialAbilityWeaponFireName`, his 0), the single player upgrade
/// that shortens the cooldown (`FUN_0079d060`; levels all 0), the HUD's messages.
fn start_special(s: &mut State, t: &Tuning) {
    s.special_starts += 1;
    s.special_cooldown = t.special.cooldown;
    if t.special.duration > 0.0 {
        s.special_left = t.special.duration;
    }
    let (script, spawners) =
        if s.car_mode { (t.special.vehicle_script, &t.actions.vehicle_spawners) } else { (t.special.script, &t.actions.spawners) };
    // The type-coded specials (0 to 3) [game 0072afd0]: their own start list and timer. Their
    // self trigger is sent by the list (`Send`), so it is not pushed again here.
    let typed = crate::special_types::SpecialType::from_kind(t.special.kind).map(|kind| {
        crate::special_types::Active::start(kind, &t.special, script, t.damage.special_multipliers)
    });
    match typed {
        Some((active, start)) => {
            s.special = Some(active);
            for effect in start {
                special_effect(s, effect);
            }
        }
        None if script != 0 => s.triggers.push(script),
        None => {}
    }
    if script == 0 {
        return;
    }
    let placed = Affine3A::from_rotation_translation(Quat::from_rotation_z(s.yaw), s.pos);
    for (index, blast) in spawners.iter().enumerate().filter(|(_, b)| b.name == script) {
        let centre = s.node_place(blast.node).unwrap_or_else(|| placed.transform_point3(blast.centre(0.0)));
        if blast.explosion.camera_shake != 0 {
            s.shakes.push(ShakeStart { name: blast.explosion.camera_shake, at: centre });
        }
        if let Some(rumble) = &blast.rumble
            && let Some(strength) = rumble.strength_at(centre.distance(s.pos))
        {
            s.rumbles.push(Rumble { kind: rumble.kind, duration: rumble.duration, strength });
        }
        s.blasts.push(Blast {
            clip: 0,
            index,
            script: Some(s.car_mode),
            delay: blast.explosion.delay,
            age: 0.0,
            centre,
            radius: blast.explosion.radius * blast.explosion.start_scale,
            live: false,
            hit: Vec::new(),
            surfaces: Vec::new(),
        });
    }
}

/// Carries out one step of a type-coded special's list (`special_types::Effect`, types 0 to 3).
/// [game 0072afd0, 0072b980, 0072e540] The steps the sim has no state for are stand-ins.
fn special_effect(s: &mut State, effect: crate::special_types::Effect) {
    use crate::special_types::Effect;
    match effect {
        // `stun_effect(0)`, `flashbang_effect(0)`, `slow_clear` [game 0072e540]
        Effect::ClearStun => {
            s.damage_timers.stunned = 0.0;
            s.stun_pending = false;
        }
        Effect::ClearFlashbang => s.damage_timers.flashbang = 0.0,
        Effect::ClearSlow => s.damage_timers.slowed = 0.0,
        // The mark of the flight shield's damage transfer (`State::transfer_source`). [game
        // 00730e90, 00730f40, 0072a550, 00719830] The end clears it only when the marked one
        // `00730f40` finds is he himself, which in a world of one is always.
        Effect::DashFlag(on) => s.transfer_source = on,
        Effect::SpeedScale(factor) => s.special_rate = factor,
        // A message with Trigger: the clip triggers the game spawns from, the same list.
        Effect::Send(name) => s.triggers.push(name),
        // `OptimusSpecial` is not an effect: 0072afd0 / 0072b980 call `audio_preset_play` /
        // `audio_preset_stop` (00771230 / 011832a0) with it on the audio manager (01194410) for a
        // human player. [game] `game\src\sound.rs` plays the preset while `State::special` runs,
        // so there is nothing to spawn or stop here.
        Effect::Spawn(_) | Effect::StopSpawned => {}
        // The end's Untrigger to the self-trigger name: sent above for both names, at the same
        // moment, by the countdown in `advance` (as `0072b980` does, for every type), so not
        // pushed twice. [game]
        Effect::Off(_) => {}
    }
}

/// What entering each kind of state does to the motion. [game: the enter of `StateIdle`
/// (`FUN_0087d830`), `StateMove` (`FUN_0087e4d0`), `StateJump` (`FUN_0087e210`), `StateFall`
/// (`FUN_0087c070`), `StateDrive` (`FUN_0087ba10`), `StateBasic` (`FUN_00879ef0`), and of the
/// attack, dash and climb states, and `StateWeapon` (`FUN_0087f4a0`): its `motionModeType`
/// first, then `WeaponSet`; coming from anything but a dash or another weapon state the gun
/// is drawn (`WEAPON_ACTION_PULL_OUT_GUN` on `WeaponPartialSet`, or at once after the change
/// from the car), and its exit (`FUN_0087f6f0`) puts it away again unless a dash or weapon
/// state follows]
fn enter_state(s: &mut State, input: &Input, t: &Tuning, next: usize, wall: Option<Wall>, on_ground: bool) {
    let def = &t.control.states[next];
    // The slam's finisher lets its target go with the state. [data work\packdump: all 25 `FinishMoveEvent`s also carry `startCamera`, so 007314d0 takes the start branch (0072df10) and never the `endCamera` one (0072e080)]
    // The hold ends when he is no longer in a StateAttack state [game 0072a550 l.207-211, settled 2026-10-20]: each
    // update the damage timers call finish_move_unlink 0072e080 when the link flag (+0x14c bit 21) is set and the
    // current state's class (char+0x314's state, vt+0x30) is not StateAttack (3e5bcf4a); a chain into another attack
    // state keeps it. (Before: the port let it go on every state change.)
    if def.class != "StateAttack" {
        s.finish_target = None;
    }
    if s.hit_full && def.class != "StateHitReact" && s.damage_timers.stunned > 0.0 { s.stun_pending = true; }
    if def.class != "StateStun" { s.stun_full = false; }
    let playing = playing_set(s, t);
    // The set and clip the new one comes from, for the entry rules of the set it starts.
    let from = playing_clip(s, t);
    // Exiting an ordinary state exits its overlays too. [game: FUN_00878310]
    s.hit_partial = Base::default();
    if def.class != "StateHitReact" {
        s.hit_full = false;
        s.hit_last = None;
    }
    // The control mode: a drive state's enter makes it the car's (`FUN_0087ba10`), a
    // weapon state's the weapon's (`FUN_0087f4a0`), and an attack or plain state's is its
    // `robotForm` unless that is 5. A fly exit's enter and the fall's put him back on foot
    // (`0087c7e0` and `0087c070` call `0071ab70(obj, 1)`, control mode 1), whatever their
    // `robotForm`: without it a flyer kept the car's gun and sights in robot form. [game]
    match def.class.as_str() {
        "StateDrive" | "StateFly" => s.car_mode = true,
        "StateWeapon" | "StateFlyExit" | "StateFall" => s.car_mode = false,
        "StateAttack" | "StateBasic" | "StateAttackSpecial" if def.robot_form != 5 => s.car_mode = def.robot_form == 3,
        _ => {}
    }
    match def.class.as_str() {
        "StateHitReact" => {
            stop_clip(s, t);
            base_enter(s, input, t, crc32(b"HitReactionSet"), from);
            s.mode = Mode::Action;
            s.hit_full = true;
            s.hit_flyback = matches!(s.damage_type, 0x6a58_53c5 | 0xfea0_bd86);
            s.hit_start_z = s.pos.z;
            s.hit_last = None;
            s.hit_cooldown = 4.0;
            s.wall_splatted = false;
            s.car_mode = false;
            if s.hit_direction.truncate().length_squared() > 1e-8 { s.yaw = yaw_of(-s.hit_direction.truncate()); }
        }
        "StateStun" => {
            stop_clip(s, t);
            base_enter(s, input, t, crc32(b"StunSet"), from);
            s.mode = Mode::Action;
            s.stun_full = true;
            s.car_mode = false;
            s.hit_flyback = false;
            s.hit_last = None;
            s.firing = false;
        }
        "StateClimb" => begin_climb(s, input, t, wall.expect("entered only with a wall")),
        // The climb mode plays the pull-up on.
        "StateBasic" if def.motion_mode == CLIMB_MODE => {}
        "StateAttack" | "StateDash" | "StateBasic" => begin_action(s, input, t, next, on_ground),
        // An ordinary special (not an overlay: `stateAttackSpecialMegatron`) starts the ability
        // as its overlay twin does (`start_special`, from `swap_overlay`'s path), then plays its
        // clip from its set's rules (`begin_action`). Its weapon fires on the clip's event
        // (`tick_special_events`). [game: the enter's FUN_0072afd0 call, as the overlay state's]
        // [game 00879110] the class's enter ends with `0072afd0` for every state of the class,
        // overlay or not (after the ordinary state's set start and its `robotForm`).
        "StateAttackSpecial" => {
            start_special(s, t);
            begin_action(s, input, t, next, on_ground);
        }
        "StateDrive" => {
            // Plays its half of the change in the drive set; on the ground it picks the car
            // mode, or MoveMode for the first half while the stick does not line up. [game]
            stop_clip(s, t);
            if def.anim != 0 {
                start_clip(s, input, t, next, def.anim);
            } else {
                // The hub (`stateDrive`, `animName` 0) starts the drive set and its entry rules
                // pick the clip: `Vehicle_Idle` for every ground robot ("always"). [game 0087ba10
                // -> 0084ddb0 with no clip] [data]
                base_enter(s, input, t, crc32(b"DriveSet"), from);
            }
            if on_ground {
                drive_or_move(s, input, t);
            }
        }
        // A fly state's enter (`FUN_0087c320`): its clip in the fly set, and FlyMode made
        // the motion mode whether he is on the ground or not. [game] (`notes\flight-states.md`)
        "StateFly" => {
            stop_clip(s, t);
            if def.anim != 0 {
                start_clip(s, input, t, next, def.anim);
            } else {
                // The hub (`animName` 0) starts the fly set and its entry rules pick the clip:
                // `Fly_Idle` and the `Fly_*` directions for a robot flyer. [game 0087c320] [data]
                // A stunt is over when the machine is back at the hub (`step_fly`'s assumption),
                // ended here at once rather than at the next `step_fly`. The stunt clip itself
                // comes from the stunt STATE (its `animName`); `C_VEHICLE_STUNT` is 0 as FlyMode
                // writes it (`rule_value`), so `Vehicle_Idle`'s exits into the stunt clips never
                // fire and cannot play a finished stunt twice. [game 0070c900] The writes are 0:
                // FlyMode enter 00864d60 and exit 00865b80 both store 0, and the six
                // VEHICLE_STUNT_* hashes occur nowhere in the executable [game 0070c900, 2026-10-20].
                // The game's own end is built: FlyMode's traction vector all zero (00866900) ends the
                // stunt, `flight::Fly::traction` from the stunt clip's last ChangeTractionEvent [game]
                // (`clip_traction`). The extra clear here at the hub return changes nothing [game, data;
                // settled 2026-10-20]: a stunt state can be left early only for Death or Stun (the four
                // stateFlyStunt* canGoToStates, global_states.txt), the stun's enter 0087eaf0 switches to
                // BasicMotionMode, whose switch runs FlyMode's exit 00865b80 (mode+0x520 = 0), and a death
                // does not come back to the hub; so the game's stunt is already cleared when the hub returns.
                if let Some(fly) = s.fly.as_mut() {
                    fly.stunt = crate::flight::Stunt::None;
                }
                base_enter(s, input, t, FLY_SET, from);
            }
            if s.mode != Mode::Fly {
                set_mode(s, input, t, Mode::Fly);
            }
        }
        // Leaving flight: its clip, and the jump mode with no launch. [game: the enter
        // starts JumpMode; the state's update hands him to the drive companion mode once
        // he is on the ground, see `step`]
        "StateFlyExit" => {
            stop_clip(s, t);
            if def.anim != 0 {
                start_clip(s, input, t, next, def.anim);
            }
            enter_jump_mode(s, input, t, Some("FallSet"), on_ground);
        }
        // The set starts before the mode does (`FUN_0087d830`, `FUN_0087e4d0`), so the mode's
        // enter sees the clip the rules picked.
        "StateIdle" => {
            stop_clip(s, t);
            base_enter(s, input, t, IDLE_SET, from);
            set_mode(s, input, t, Mode::Stand);
        }
        "StateMove" => {
            stop_clip(s, t);
            base_enter(s, input, t, WALK_SET, from);
            set_mode(s, input, t, Mode::Move);
        }
        // The jump mode starts before the jump set does, so it still sees what was playing.
        "StateJump" => {
            stop_clip(s, t);
            enter_jump_mode(s, input, t, playing.as_deref(), on_ground);
            base_enter(s, input, t, JUMP_SET, from);
        }
        // The fall set starts first: no launch.
        "StateFall" => {
            stop_clip(s, t);
            base_enter(s, input, t, FALL_SET, from);
            enter_jump_mode(s, input, t, Some("FallSet"), on_ground);
        }
        "StateWeapon" => match def.motion_mode {
            // The weapon states' jump and fall: the jump mode, then the weapon set, whose
            // entry rules pick the clip from what was playing: a take-off out of the set's
            // own clips (to the left or right with the stick that way against the aim),
            // `Weapon_Fall` out of the jump and fall sets or when already going down. [game]
            JUMP_MODE => {
                let (from_set, from_id) = playing_clip(s, t);
                stop_clip(s, t);
                enter_jump_mode(s, input, t, playing.as_deref(), on_ground);
                strafe_entry(s, input, t, from_set, from_id);
            }
            // Weapon mode on foot: StrafeMode, then the weapon set; the clip of the state
            // being left is still there for the set to fade in over. [game]
            // The hover weapon state (`stateWeaponModeHover`): its motionModeType is HoverMode [data
            // global_states.txt 1094], so he hangs in the air with the gun, moved by HoverMode's
            // update (`step_hover`), and the weapon set plays over it as it does over the weapon
            // jump. The mode comes before the weapon set (`FUN_0087f4a0` l.12-13), so HoverMode's
            // enter has written `I_IS_HOVERING` 1 by the time the set's entry rules are walked;
            // from the plain hover the mode is already running and is not entered again
            // (`FUN_00852150`), so its timer runs on. [game] (Before 2026-10-20 the port ran this
            // state on StrafeMode: gravity and the ground's steps, so the weapon hover fell.)
            HOVER_MODE => {
                let (from_set, from_id) = playing_clip(s, t);
                stop_clip(s, t);
                set_mode(s, input, t, Mode::Hover);
                s.hover_weapon = true;
                s.strafe.aim_yaw = input.aim.filter(|aim| aim.length_squared() > 1e-6).map_or(s.yaw, yaw_of);
                strafe_entry(s, input, t, from_set, from_id);
            }
            _ => {
                s.hover_weapon = false;
                set_mode(s, input, t, Mode::Strafe);
                stop_clip(s, t);
            }
        },
        _ => {}
    }
    // Only the hover weapon state keeps the variable; any other state's mode has exited
    // HoverMode (the dash's rules above still saw it: its set starts before its mode).
    if !(def.class == "StateWeapon" && def.motion_mode == HOVER_MODE) {
        s.hover_weapon = false;
    }
    if def.class != "StateWeapon" {
        // Every other state starts a set of its own, which ends the weapon set's clips.
        (s.strafe.clip, s.strafe.tick) = (0, 0.0);
        s.strafe.layers.clear();
    }
    if s.mode != Mode::Strafe && !(def.class == "StateAttackSpecial" && def.motion_mode == STRAFE_MODE) {
        // StrafeMode's exit (`FUN_00876ea0`) for the special that ran in it without the port's
        // `Mode::Strafe`: `I_IS_STRAFING` and `F_STRAFE_ANG` to 0, the twist off. [game]
        (s.strafe.strafing, s.strafe.angle, s.strafe.twist) = (false, 0.0, 0.0);
    }
    // A fly state whose clip the character's set does not have starts nothing, and what was
    // playing plays on [game 0084ddb0: the explicit clip's lookup fails and it returns] (the
    // robot flyers have no Trans_R2V_A / B, so their EnterA / B keep the clip before).
    let fly_hub = def.class == "StateFly" && (def.anim == 0 || !t.actions.clips.contains_key(&def.anim));
    let drive_hub = def.class == "StateDrive" && def.anim == 0;
    if !fly_hub && !drive_hub && !matches!(def.class.as_str(), "StateIdle" | "StateMove" | "StateJump" | "StateFall" | "StateHitReact" | "StateStun") {
        // A set of the states that play a clip of their own, or the weapon set, is not the
        // base controller's to run. (The count of clips started goes on, so the next clip is
        // told from the last one by `queue_layer_events`.)
        s.base = Base { serial: s.base.serial.wrapping_add(1), ..Base::default() };
    }
}

/// HitReactionSet's full-body clips drive BasicMotionMode. Flybacks keep the supplied
/// velocity in KnockBackMode until grounded, then use XY root motion for recovery.
/// [game: FUN_0087cd10, FUN_00875100, 00875088..008750c9]
fn step_hit_reaction(s: &mut State, t: &Tuning, dt: f32) {
    let Some(data) = s.base.current(&t.rule_sets) else { return };
    // KnockBackMode hands the ground axes to the clip only once he is grounded (STEP +0x26c)
    // AND his body's vertical speed is 0.01 or less (00874fe2-00875019: `body_linear_velocity`
    // 00556c00 through 00725560, `comiss 0.01, vz`); until then the throw's velocity flies him.
    // [game] (It read as a height test before: on the hit's first update he still stood on the
    // ground, so the throw lost its push along the ground and never reached a wall.)
    if s.hit_flyback && !(s.ground_flag() && s.vel.z <= 0.01) {
        s.hit_last = Some((s.base.set, s.base.clip, s.base.tick));
        return;
    }
    let from = s.hit_last.filter(|&(set, id, _)| (set, id) == (s.base.set, s.base.clip)).map_or(0.0, |(_, _, tick)| tick);
    let moved = data.travel(from, s.base.tick).0 / dt.max(1e-6);
    let facing = s.facing();
    let right = Vec2::new(facing.y, -facing.x);
    let flat = right * moved.x + facing * moved.y;
    (s.vel.x, s.vel.y) = (flat.x, flat.y);
    s.hit_last = Some((s.base.set, s.base.clip, s.base.tick));
}

/// The set playing now, as JumpMode's enter looks at it: the climb's, or the state clip's.
fn playing_set(s: &State, t: &Tuning) -> Option<String> {
    if s.climb.is_some() {
        return Some("ClimbSet".to_owned());
    }
    let action = s.action.as_ref()?;
    t.actions.clips.get(&action.clip).map(|clip| clip.set.clone())
}

/// The first half of the change into the car (`Trans_R2V_A` and its running version) keeps
/// MoveMode, which turns him toward the stick, until the stick lines up with the way he is
/// moving (`FUN_0087bcf0`: within about 26 degrees, or the stick centred); everything else in
/// the drive states is the car mode. [game]
fn drive_or_move(s: &mut State, input: &Input, t: &Tuning) {
    let first_half = s.action.as_ref().is_some_and(|a| a.clip == crc32(b"Trans_R2V_A") || a.clip == crc32(b"Trans_R2V_Run_A"));
    let stick = input.stick;
    let lined_up = stick.length() <= 0.01 || {
        let moving = Vec2::new(s.vel.x, s.vel.y);
        let along = if moving.length() > 1e-8 { moving.normalize() } else { moving };
        stick.normalize().dot(along) >= DRIVE_LINE_UP
    };
    let mode = if first_half && !lined_up { Mode::Move } else { Mode::Drive };
    set_mode(s, input, t, mode);
}

/// Onto the wall: the climb mode's enter. [game]
fn begin_climb(s: &mut State, input: &Input, t: &Tuning, wall: Wall) {
    stop_clip(s, t);
    // The set's entry rule reads `I_IN_AIR`; taken to be the state machine's ground flag.
    let in_air = !s.ground_flag();
    s.climb = Some(Climb::enter(wall, &mut s.pos, s.vel, t, t.robot_radius, in_air));
    set_mode(s, input, t, Mode::Climb);
    s.gait = Gait::Idle;
    s.speed = 0.0;
}

/// Ends the clip of the state being left, and the climb: the dash mode's exit starts the
/// dodge's cooldown, and the ground punch's extra pull goes. [game]
fn stop_clip(s: &mut State, t: &Tuning) {
    if let Some(mut action) = s.action.take() {
        // Events it passed are queued already in the game (0053ab90): they still go out though
        // the clip is left (a stunt clip's last key ends the stunt as its state is left).
        s.clip_effects.append(&mut action.effects);
        // Ended in this update: what the next state's clip takes over (`start_clip`).
        if action.finished && action.overshoot > 0.0 {
            if let Some(data) = t.actions.clips.get(&action.clip) {
                let seconds = action.overshoot / (crate::formats::anim::TICKS_PER_SECOND * data.speed * s.special_rate).max(1e-6);
                s.clip_handover = Some((seconds, action.root_travel(data)));
            }
        }
        if t.control.states[action.state].class == "StateDash" {
            s.dash_cooldown = t.dash_cooldown;
        }
    }
    if let Some(climb) = s.climb.take() {
        (s.climb_top, s.climb_top_facing) = (climb.top, climb.top_facing);
    }
    s.punch_gravity = 0.0;
}

/// How far past the top target, in radii, the ledge is looked for; how far under it his feet
/// may be; and the least the ball must drop before what it touches counts. [game]
const CATCH_REACH_RADII: f32 = 2.25;
const CATCH_BELOW: f32 = 1.5;
const CATCH_LEAST_DROP: f32 = 0.25;
/// How square to the top's facing he must be, and how long that facing must be to count as
/// set (the climb mode's enter zeroes it). [game]
const CATCH_FACING: f32 = 0.98;
const CATCH_FACING_SET: f32 = 0.9;

/// The jump state's own update (`FUN_0087dde0`), run only when the state before was
/// `stateClimbJumpUp`: once he is coming down, still under the top the climb mode aimed for,
/// no further than his radius past it and facing as he did at the top, a ball of his radius
/// is let down 2.25 radii beyond the top; if it lands on something within 1.5 above his feet
/// he is put there, facing as at the top, with no speed (`FUN_00854790`, `FUN_00855940`).
/// The climb mode only sets the target when he goes over the top, so this follows the
/// "exit jump-up" of a wall with a ledge above it. [game] True if it caught him.
/// The reach above is his height (owner `+0x160`) plus how far his body's foot stands over
/// his own position when it does (`FUN_0070abe0`, the sum the camera's look-at height also
/// uses). The body here never leaves his position, so that part is zero. [game]
/// The same catch runs as `stateClimbPullup` is left (the exit of every `StateBasic`,
/// `FUN_0087a5d0`, calls `FUN_0087a200`, which acts only for that state): there it does not
/// ask that he be coming down (`falling` false). [game] Its hit test (disassembly 0087a502..
/// 0087a561): hit z above his z (`jbe` out), his z above hit z - 1.5, and |hit z - start z| over
/// 0.25 (`00409a50`, then `fcompi` against 0.25 at 00b387f8); there is NO "under his z + h"
/// bound, unlike the jump's, so the pull-up leaves it out. Then `00854790` and `00855940` with
/// a zero vector, as the jump's catch. [game]
fn ledge_catch(s: &mut State, t: &Tuning, arena: &dyn World, falling: bool) -> bool {
    let (top, facing) = (s.climb_top, s.climb_top_facing);
    if (falling && s.vel.z >= 0.0) || facing.length_squared() <= CATCH_FACING_SET || top.z <= s.pos.z {
        return false;
    }
    let (radius, height) = (t.robot_radius, t.robot_height);
    let past = (Vec2::new(s.pos.x, s.pos.y) - Vec2::new(top.x, top.y)).dot(facing);
    if past >= radius || s.facing().dot(facing) <= CATCH_FACING {
        return false;
    }
    let over = Vec2::new(top.x, top.y) + facing * (radius * CATCH_REACH_RADII);
    let from = Vec3::new(over.x, over.y, top.z + height);
    let Some(hit) = arena.drop_onto(from, s.pos.z, radius) else { return false };
    let feet = s.pos.z;
    // The pull-up's test has no "under his z + h" bound (below); the jump's has.
    if hit.z <= feet || hit.z - CATCH_BELOW >= feet || (falling && hit.z >= feet + height) || (hit.z - from.z).abs() <= CATCH_LEAST_DROP {
        return false;
    }
    s.pos = hit;
    s.yaw = yaw_of(facing);
    s.vel = Vec3::ZERO;
    s.speed = 0.0;
    true
}

/// Starts a control state that plays its own clip, and the motion mode it names: the attack
/// and dash modes (the clip's root motion moves him), the jump mode (air attacks, the ground
/// punch's fall, the leap out of the car), the unfold. [game]
fn begin_action(s: &mut State, input: &Input, t: &Tuning, state: usize, on_ground: bool) {
    let def = &t.control.states[state];
    let from_climb = s.climb.is_some();
    // A BasicMotionMode clip out of the car (Trans_Vehicle2Robot2GroundPunch) or out of flight (the jets'
    // Trans_V2R_GroundPunch, up to 8.9 m of root z) keys [1,1,1]: the clip lifts him. [data, game 00856cc0]
    let lifted = def.motion_mode == FIRST_MODE && matches!(s.mode, Mode::Drive | Mode::Fly);
    // The attack mode is not started again while it runs (`FUN_00852150`), so a chain of
    // attacks keeps its target, unless the attack before hit a character: then the mode
    // lets the target go once the hit list is emptied. [game]
    let kept = s
        .action
        .as_ref()
        .filter(|a| t.control.states[a.state].motion_mode == ATTACK_MODE && !a.has_hit)
        .and_then(|a| a.target)
        .filter(|_| def.motion_mode == ATTACK_MODE);
    stop_clip(s, t);
    let mut heading = s.yaw;
    let clip = if def.class == "StateDash" {
        // The dodge set's incoming rules pick the clip from the stick against the aim
        // (`F_L_STICK_DIR`, degrees, positive to the left). [data]
        heading = input.aim.map_or(s.yaw, yaw_of);
        let mut angle = (yaw_of(input.stick) - heading).to_degrees() % 360.0;
        if angle > 180.0 {
            angle -= 360.0;
        } else if angle < -180.0 {
            angle += 360.0;
        }
        // The set's entry rules are walked as the controller walks them: front within 45
        // degrees (from -45 up to, not including, 45), back from 135 round either way, left
        // and right between. [game] Without the set loaded, the fallback below takes the same
        // edges: front from -45 up to 45, back from 135 round either way, right from -135 up to
        // -45, left from 45 up to 135 ([data bumblebee.txt DashSet incRules, animRuleList*
        // and animRuleForwardMin/Max, animRuleLeftMin/Max, animRuleRightMin/Max, animRuleBackward*:
        // the same edges with the same comparisons; the fallback reads them as the symmetric
        // dodge_forward / dodge_back). SETTLED.
        let by_rules = t.rule_sets.get(&DASH_SET).and_then(|set| {
            let get = |variable: u32| match variable {
                animrules::var::L_STICK_DIR => Some(Value::Float(angle)),
                _ => rule_value(s, input, variable),
            };
            animrules::entry(&set.incoming, 0, 0, &get).filter(|&(to_set, _)| to_set == DASH_SET).map(|(_, id)| id)
        });
        let a = &t.actions;
        let id = by_rules.unwrap_or_else(|| {
            crc32(
                if angle >= -a.dodge_forward && angle < a.dodge_forward {
                    "DodgeFront"
                } else if angle >= a.dodge_back || angle < -a.dodge_back {
                    "DodgeBack"
                } else if angle < 0.0 {
                    "DodgeRight"
                } else {
                    "DodgeLeft"
                }
                .as_bytes(),
            )
        });
        s.yaw = heading;
        // The dodge's enter sets his dash clock to half a second (status +0x94), which homing
        // missiles read. [game 0087a960 l.27]
        s.damage_timers.dash = 0.5;
        id
    } else if def.class=="StateAttack" && def.anim==0 {
        let (from_set,from_id)=playing_clip(s,t);
        t.rule_sets.get(&crc32(b"CombatSet")).and_then(|set|animrules::entry(&set.incoming,from_set,from_id,&|v|rule_value(s,input,v))).map_or(0,|(_,id)|id)
    } else if def.class=="StateAttackSpecial" && def.anim==0 {
        // An ordinary special with no clip of its own (Megatron's, `SpecialAnimSet`): its clip
        // is picked by its set's entry rules, as the attack's is by `CombatSet`'s: the class's
        // enter calls `animation_set_start` (`0084ddb0`) with its set `+0x48` and clip `+0x4c`,
        // which walks the set's entry rules when the clip is 0. [game 00879110]
        let (from_set,from_id)=playing_clip(s,t);
        t.rule_sets.get(&crc32(b"SpecialAnimSet")).and_then(|set|animrules::entry(&set.incoming,from_set,from_id,&|v|rule_value(s,input,v))).map_or(0,|(_,id)|id)
    } else if def.class=="StateBasic" && def.anim==0 {
        // A plain state with no clip of its own names a set (`animSetName`): its enter walks that
        // set's entry rules for the clip (`animation_set_start` 0084ddb0 with clip 0). [game 00879ef0]
        // Of the StateBasic objects in global_states.txt only statePutDownHuman has this (InteractSet,
        // a carried human: out of scope); stateClimbPullup has neither, and takes the climb mode. [data]
        let (from_set,from_id)=playing_clip(s,t);
        def.anim_set.as_ref().and_then(|name|t.rule_sets.get(&crc32(name.as_bytes()))).and_then(|set|animrules::entry(&set.incoming,from_set,from_id,&|v|rule_value(s,input,v))).map_or(0,|(_,id)|id)
    } else {
        def.anim
    };
    // [game 00879ef0] A StateBasic's enter does its mode switch whether or not its clip is there:
    // animation_set_start starts nothing for a clip the set lacks (01193520), and the enter goes on
    // to mode_switch_by_type and the (1,1,0) control. Every other class still stops here.
    let started = start_clip(s, input, t, state, clip);
    if !started && def.class != "StateBasic" {
        return;
    }
    if let Some(action) = s.action.as_mut() {
        action.heading = heading;
        action.from_climb = from_climb;
        action.lifted = lifted;
        action.target = kept;
    }
    if def.motion_mode == ATTACK_MODE {
        s.fight = FIGHT_TIME;
    }
    match def.motion_mode {
        // The set starts before the mode, so the jump mode sees the state's own set. [game]
        JUMP_MODE => {
            let set = playing_set(s, t);
            enter_jump_mode(s, input, t, set.as_deref(), on_ground);
        }
        UNFOLD_MODE => set_mode(s, input, t, Mode::Unfold),
        HOVER_MODE => set_mode(s, input, t, Mode::Hover),
        // DriveMode: Megatron's tank special (`stateAttackSpecialMegatronVehicle`, robotForm 3):
        // he goes on driving while the special set's clips play on the hidden robot, whose model
        // events run the tank's `Siege_In` / `Siege_IdleCharge` / `Siege_Out`. Already in the
        // mode, the switch does nothing (00852150). [game 00852150] [data]
        DRIVE_MODE => {
            if s.mode != Mode::Drive {
                set_mode(s, input, t, Mode::Drive);
            }
        }
        // [game 00879ef0] With no clip playing, the first mode is the standing one, not an
        // action's: nothing moves him but the mode itself.
        _ if !started && def.motion_mode == FIRST_MODE => {
            set_mode(s, input, t, Mode::Stand);
            s.gait = Gait::Idle;
        }
        _ => {
            set_mode(s, input, t, Mode::Action);
            s.gait = Gait::Idle;
        }
    }
}

/// Starts a state's clip. False if the character has no such clip. [data]
fn start_clip(s: &mut State, _input: &Input, t: &Tuning, state: usize, clip: u32) -> bool {
    let Some(data) = t.actions.clips.get(&clip) else { return false };
    // The blend is the animRef's of the set the state starts the clip in: the table of clips is
    // keyed by name alone, and `Trans_V2R_Hover` of `Vehicle2RobotSet` (the hover-enter state's)
    // blends in over 0 where `HoverSet`'s copy, which the table holds, asks 0.1. [data]
    let crossfade = action_rule_clip(t, state, clip).map_or(data.crossfade, |(_, found)| found.crossfade);
    let fade_time = if crossfade < 0.0 { DEFAULT_CROSSFADE } else { crossfade };
    let heading = s.yaw;
    // [trace] A clip that ended in this update hands its leftover time to the one the next state
    // starts: the new clip begins that far in, its root over the leftover is not counted, and the
    // update's root motion is the old clip's travel up to its end. The 2026-10-21 Starscream
    // capture's take-off (Trans_R2V_A into Trans_R2V_B through stateFlyEnterA / B): the update A
    // ends rises 0.108 m (A's last 0.44 ticks: 0.106), B's root then runs 1.5 ticks in, and the
    // climb totals 6.90 m (4.24 + 3.10 - B's first 1.52 ticks, 0.42 = 6.92; with B's leftover
    // counted it would be 7.34, with A's end dropped 6.66). [assumed] the same for every state that
    // ends on its clip: only this one transition is measured. settles: a recording of another
    // clip-end state change (an attack combo's last hit into idle, a jump's land) with the root.
    let replaced = s.action.as_ref().filter(|old| old.finished && old.overshoot > 0.0).and_then(|old| {
        let old_data = t.actions.clips.get(&old.clip)?;
        Some((old.overshoot / (crate::formats::anim::TICKS_PER_SECOND * old_data.speed * s.special_rate).max(1e-6), old.root_travel(old_data)))
    });
    let (start_tick, carried) = s.clip_handover.take().or(replaced)
        .map(|(seconds, travel)| ((seconds * crate::formats::anim::TICKS_PER_SECOND * data.speed * s.special_rate).min(data.ticks), travel))
        .unwrap_or((0.0, Vec3::ZERO));
    s.action = Some(ActionState {
        state,
        clip,
        tick: start_tick,
        finished: false,
        last_tick: start_tick,
        overshoot: 0.0,
        carried,
        events_done: 0,
        branch: 1,
        window_open: false,
        pressed_in_window: false,
        can_branch: false,
        slide: false,
        face: false,
        stick: false,
        hitting: false,
        live: Vec::new(),
        triggers: Vec::new(),
        target: None,
        has_hit: false,
        carry: Vec2::new(s.vel.x, s.vel.y),
        fade: if fade_time > 0.0 { 0.0 } else { 1.0 },
        fade_time,
        heading,
        from_climb: false,
        lifted: false,
        effects: Vec::new(),
    });
    // The clip's events at its first tick count at once.
    pass_events(s.action.as_mut().unwrap(), data, &Pad::default());
    true
}

/// Moves the clip on and handles what its events say. [game]
fn play_action(s: &mut State, t: &Tuning, pad: &Pad, dt: f32) {
    let Some(action) = s.action.as_mut() else { return };
    let Some(data) = t.actions.clips.get(&action.clip) else { return };
    action.last_tick = action.tick;
    action.tick += dt * crate::formats::anim::TICKS_PER_SECOND * data.speed * s.special_rate;
    action.carried = Vec3::ZERO;
    action.overshoot = 0.0;
    if action.tick >= data.ticks {
        if data.looping {
            pass_events(action, data, pad);
            action.tick %= data.ticks.max(1.0);
            action.events_done = 0;
            action.branch = 1;
        } else {
            // Only the update it ends in: a clip that has ended stays at its end.
            if !action.finished {
                action.overshoot = action.tick - data.ticks;
            }
            action.tick = data.ticks;
            action.finished = true;
        }
    }
    pass_events(action, data, pad);
    // The attack state notes a press of the attack button while the window is open. [game]
    if action.window_open && !action.pressed_in_window && pad.was_hit(button::ATTACK_FAST) {
        action.pressed_in_window = true;
    }
    action.fade = (action.fade + dt / action.fade_time.max(1e-3)).min(1.0);
    // Its passed motion events join the queue now (0053ab90), so leaving the clip loses none.
    let effects = std::mem::take(&mut action.effects);
    s.clip_effects.extend(effects);
}

/// The car's special plays the car model's clip named by `specialAbilityVehicleAnim` (`Special`
/// for the Audi and Blackout) while he stays in the car: the clip's `WeaponFireSpecialEvent`
/// fires the mine or the turret (`tick_special_events`). [data] The clip is not in any set of
/// the robot, so it is started from the car's own table (`Actions::vehicle_clip_key`).
/// The car keeps driving meanwhile: in control mode 3 the start (`0072afd0` lines 22-32) sends
/// the clip as a `PlayAnim` event (`007247f0`, registry key 0805bf75) to the car's clip bundle,
/// `animOnDone` = `V_Idle`, crossfade -1, and changes no mode; the overlay state itself plays no
/// clip [game 00879110]. So the clip ends him back to the car's idle (`special_chain`). [game] Nothing is started when the attribute is 0 or the
/// clip is not there (Megatron's car gun, Jetfire's car). [data]
fn start_vehicle_special(s: &mut State, input: &Input, t: &Tuning, state: usize) {
    let anim = t.special.vehicle_anim;
    if anim == 0 {
        return;
    }
    let key = crate::tuning::Actions::vehicle_clip_key(anim);
    if !t.actions.clips.contains_key(&key) {
        return;
    }
    stop_clip(s, t);
    start_clip(s, input, t, state, key);
}

/// Whether an ordinary special has ended: its clip is in `SpecialAnimSet` and has finished,
/// which is its exit from the set (`finishOnSpecialAnimSetExit`). [data] [game 0070d540]
fn special_ended(s: &State, t: &Tuning, in_special: bool) -> bool {
    in_special && s.action.as_ref().is_some_and(|a| a.finished && t.actions.clips.get(&a.clip).is_some_and(|c| c.set == "SpecialAnimSet"))
}

/// The ordinary special's clips hand over by their set's exit rules (`SpecialAnimSet`, read
/// as the controller walks them, `FUN_0070d540`). `SpecialAttack_Intro` ends into
/// `SpecialAttack_Idle` (its exit has no rules); Idle loops while the ability is on, and its
/// exit to `SpecialAttack_Exit` holds when `I_SPECIAL_ON` is 0 (`FUN_0070cae0`: the timer is
/// not above 0); Exit finishes by leaving the set (to clip 0), which ends the state
/// (`finishOnSpecialAnimSetExit`, read in `play_action`'s caller). [data: megatron.str
/// SpecialAnimSet, animRef 0, 2, 6] [game 0070cae0, 0070d540] The timer is `special_left`:
/// `0070cae0` (line 56) reads status `+0x8c`, which `0072afd0` sets to `specialAbilityDuration`
/// (2.75 for Megatron) and `0072a550` runs down. [game]
fn special_chain(s: &mut State, t: &Tuning) {
    let Some(action) = s.action.as_ref() else { return };
    let Some(data) = t.actions.clips.get(&action.clip) else { return };
    if data.set == crate::tuning::Actions::VEHICLE_SET {
        // The car's special ends with its clip (`animOnDone` V_Idle): the car is driving as
        // before. [game 0072afd0 line 30]
        if action.finished {
            stop_clip(s, t);
        }
        return;
    }
    if data.set != "SpecialAnimSet" {
        return;
    }
    let Some(clip) = t.rule_sets.get(&crc32(b"SpecialAnimSet")).and_then(|set| set.clips.iter().find(|c| c.id == action.clip)) else { return };
    let (on, strafe_angle) = (s.special_left > 0.0, s.strafe.angle);
    // `SpecialAttack_Idle` also turns on `F_STRAFE_ANG` (IdleTurnL at 45 or more, R under -45),
    // which the special's StrafeMode writes (`step_action`). [data] [game 00875ef0]
    let get = |variable: u32| match variable {
        animrules::var::SPECIAL_ON => Some(Value::Int(on as i32)),
        animrules::var::STRAFE_ANG => Some(Value::Float(strafe_angle)),
        _ => None,
    };
    let Some(exit) = animrules::exit(&clip.exits, action.branch, action.finished, &get) else { return };
    if exit.to_id != 0 {
        switch_action_clip(s, t, exit.to_id);
    }
}

/// A clip started by name hands over to another by its exit rules: the control state goes on
/// with the new clip as its own, from its first tick, faded in over the new clip's own blend
/// time. [game: the controller's walk, `FUN_0084e3c0` / `FUN_0070d540`, starts the named clip
/// on the layer the old one played on]
fn switch_action_clip(s: &mut State, t: &Tuning, next_id: u32) {
    let Some(next) = t.actions.clips.get(&next_id) else { return };
    let fade_time = if next.crossfade < 0.0 { DEFAULT_CROSSFADE } else { next.crossfade };
    let Some(action) = s.action.as_mut() else { return };
    action.clip = next_id;
    action.tick = 0.0;
    action.last_tick = 0.0;
    action.finished = false;
    action.events_done = 0;
    action.branch = 1;
    action.window_open = false;
    action.pressed_in_window = false;
    action.can_branch = false;
    action.slide = false;
    action.face = false;
    action.stick = false;
    action.hitting = false;
    action.live.clear();
    action.triggers.clear();
    action.fade = if fade_time > 0.0 { 0.0 } else { 1.0 };
    action.fade_time = fade_time;
    pass_events(action, next, &Pad::default());
}

/// The set a control state's clip plays in, and the clip as the rules run it. The state's own
/// `animSetName` where it names one; the fly states start `FlySet` and the drive states
/// `DriveSet` whatever the data names (`0087c320`, `0087c7e0`, `0087ba10`); else the set the
/// clip's name was found in. The table of clips is keyed by name alone, and two sets can hold a
/// clip of one name with different exit rules (`Vehicle2RobotSet` and `HoverSet` both have
/// `Trans_V2R_Hover`; only the second leaves into `Hover_Idle`), so the state's set decides.
/// [game: `animation_set_start`, `0084ddb0`, looks the clip up in the set it is given]
fn action_rule_clip<'a>(t: &'a Tuning, state: usize, clip: u32) -> Option<(u32, &'a crate::tuning::RuleClip)> {
    let def = t.control.states.get(state)?;
    let data = t.actions.clips.get(&clip)?;
    let named = def.anim_set.as_deref().map(|name| crc32(name.as_bytes()));
    let by_class = match def.class.as_str() {
        "StateFly" | "StateFlyExit" => Some(FLY_SET),
        "StateDrive" => Some(crc32(b"DriveSet")),
        _ => None,
    };
    [named, by_class, Some(crc32(data.set.as_bytes()))]
        .into_iter()
        .flatten()
        .find_map(|set| t.rule_sets.get(&set)?.clip(clip).map(|found| (set, found)))
}

/// The name of the set a control state's clip plays from, as `action_rule_clip` finds it. The
/// action table is keyed by the clip's name alone, and a later set's copy of a name wins there
/// (the flyers' `HoverSet/Trans_V2R_Hover` over `Vehicle2RobotSet/Trans_V2R_Hover`, which
/// `stateHoverEnter` names). The game's layer keeps the set the state named
/// (`animation_set_start` 0084ddb0 stores the set `anim_set_find_by_id` found), so that is what
/// the next entry walk comes from and what the weapon state's enter tests for the change from
/// the car (`FUN_0087f4a0`). [game] [data: the two copies hold the same clip and events, only
/// their blend and exits differ, set_dump 2026-10-21]
fn action_set_name(t: &Tuning, state: usize, clip: u32) -> Option<&'static str> {
    let (set, _) = action_rule_clip(t, state, clip)?;
    crate::tuning::Actions::SETS.into_iter().find(|name| crc32(name.as_bytes()) == set)
}

/// Whether a set is one whose clips the action player plays (`Actions::SETS`): an exit rule
/// into any other set is the base layer's, or the state machine's, to carry out.
fn plays_as_action(set: u32) -> bool {
    crate::tuning::Actions::SETS.iter().any(|name| crc32(name.as_bytes()) == set)
}

/// The animation controller's turn for a clip a control state started BY NAME (`start_clip`):
/// its exit rules are walked as for every clip the controller plays (`FUN_0084e3c0`, the first
/// entry that fits by `FUN_0070d540`), and a clip they name takes its place; with none named
/// the named set's entry rules pick one, coming from this clip. [game] It runs after the
/// state machine has looked (as `base_step` and the weapon set's walk do for theirs), so the
/// update a clip finishes in is the update the machine sees it finished in: a stunt state
/// leaves on the finished clip before the exit walk could replace it with `Vehicle_Idle`.
/// [game 0071dc20, settled 2026-10-20] character_update calls the state object's slot 1 (the machine,
/// char+0x314) first, then animation_rules_update 0084e3c0 (the exit walks of layers 0-2), then the
/// STEP's update 00852220: the machine sees the finished clip before the exit walk replaces it.
/// `SpecialAnimSet` and the car model's clips are `special_chain`'s, which runs before the
/// machine. Of the roster only the jets' stunt clips (`Vehicle_Roll*`, `Vehicle_Loop*`: finished
/// into `Vehicle_Idle`) have rules in a set the action player plays; the exit into another
/// set (a landing into `IdleStandSet`) ends the state through its own connections.
fn action_exits(s: &mut State, input: &Input, t: &Tuning) {
    let Some(action) = s.action.as_ref() else { return };
    let Some(data) = t.actions.clips.get(&action.clip) else { return };
    if data.set == "SpecialAnimSet" || data.set == crate::tuning::Actions::VEHICLE_SET {
        return;
    }
    let Some((set, clip)) = action_rule_clip(t, action.state, action.clip) else { return };
    if clip.exits.is_empty() {
        return;
    }
    let get = |variable: u32| rule_value(s, input, variable);
    let Some(exit) = animrules::exit(&clip.exits, action.branch, action.finished, &get) else { return };
    let to_set = if exit.to_set == 0 { set } else { exit.to_set };
    let next = if exit.to_id != 0 {
        Some((to_set, exit.to_id))
    } else {
        t.rule_sets.get(&to_set).and_then(|rules| animrules::entry(&rules.incoming, set, clip.id, &get))
    };
    let next = next
        .filter(|&(next_set, id)| id != 0 && id != action.clip && (next_set == 0 || plays_as_action(next_set)) && t.actions.clips.contains_key(&id))
        .map(|(_, id)| id);
    if let Some(next) = next {
        switch_action_clip(s, t, next);
    }
}

fn pass_events(action: &mut ActionState, data: &ActionClip, pad: &Pad) {
    while let Some(event) = data.events.get(action.events_done).filter(|e| e.time <= action.tick) {
        action.events_done += 1;
        match event.action {
            ClipAction::BranchPoint => action.branch += 1,
            ClipAction::ButtonWindow { open: true } => {
                action.window_open = true;
                action.pressed_in_window = pad.was_hit(button::ATTACK_FAST);
            }
            ClipAction::ButtonWindow { open: false } => {
                if action.pressed_in_window {
                    action.can_branch = true;
                }
                action.window_open = false;
                action.pressed_in_window = false;
            }
            ClipAction::FaceAndSlide { slide, face, stick } => (action.slide, action.face, action.stick) = (slide, face, stick),
            ClipAction::Hit { on, node } => {
                action.hitting = on;
                action.live.retain(|&live| live != node);
                if on {
                    action.live.push(node);
                }
            }
            ClipAction::Trigger { name } => action.triggers.push(name),
            motion => action.effects.push(motion),
        }
    }
}

/// Carries out the motion events his layers passed (the action clip's, the base layer's, the
/// weapon set's leading clip's), in the order queued. [game: each is its registry run handler;
/// a fired event is queued (0053ab90) and sent later in the frame (00544a00)] Called at the
/// start of an update (`advance`), for every layer's events of the update before: the game's
/// characters step at update-list priority 5 and the animation controllers send their queues at
/// 11, after every controller has advanced (00544b60). [game 005a8740, 0071e930, 00544b60]
fn apply_clip_effects(s: &mut State, input: &Input, t: &Tuning, arena: &dyn World) {
    if let Some(action) = s.action.as_mut() {
        s.clip_effects.append(&mut action.effects);
    }
    for effect in std::mem::take(&mut s.clip_effects) {
        match effect {
            ClipAction::Traction(control) => clip_traction(s, t, Vec3::from(control)),
            ClipAction::MotionMode(class) => clip_motion_mode(s, input, t, class),
            // [game 00731400] set, not raised: the event's value replaces the clock.
            ClipAction::Dash(time) => s.damage_timers.dash = time,
            ClipAction::ForceUp(up) => clip_force_up(s, Vec3::from(up)),
            // [game 007baa00] a set and a clip both given, else nothing. The game also starts it only
            // when the scene's first Weapon (00745f60) has `+0x60` bit 3 clear, and the Character has
            // `+0xfc`. This port keeps no scene Weapon object here, so that bit is taken as clear.
            // [assumed] Read 2026-10-20: 00721350 (`FUN_00721350`, char in EAX; on = param 1, off = 0) sets
            // bit 3 (|8) of the `+0x60` word of the object at char+0x258 (`+600`) on the on-call and clears it
            // on the off-call; the on-call also sets char+0x14c bit 15, zeroes the body's velocity (when its
            // +0xa4 is set) and hides the character (0071cfa0 with 0). The bit's meaning is not read: it is
            // set at 007d3b65, 007d3d5a, 007d4040, 007d6130, 007d6328 and cleared at 007d4e0c, 007d6402,
            // 007d6535 (raw_disasm scan, not read). settles: the class of the object at char+0x258 and what
            // those writers do; and whether char+0x258 is the scene's first Weapon (00745f60).
            // Read (lead, 2026-10-20): the writers are cutscene code: 007d37f0 is cinema_play (it marks every
            // cinematic root it plays `+0x60 | 8`), 007d5d60 CinemaTrigger, 007d4ab0 its message handler. So bit
            // 3 is "in a cinematic", and cutscenes are out of the robots' scope. 00721350 itself is a listener the
            // character's ctor 00713300 links at char+0x3d4 (callback thunk 00725110, 007136cc); the signal that
            // fires it is not found. settles: the sender of char+0x3d4's signal (a cutscene hide or a play path).
            ClipAction::PartialAnim { set, id } if set != 0 && id != 0 => s.partial_requests.push((set, id)),
            ClipAction::FinishMove { start, end, kill } => finish_move(s, t, arena, start, end, kill),
            // A rule layer's trigger (the base layer's, the weapon set's): to his object by name,
            // as the action clip's are (`pass_events`). [game 0053ab90, 00724940]
            ClipAction::Trigger { name } => s.triggers.push(name),
            _ => {}
        }
    }
}

/// `FinishMoveEvent` (007314d0), in the car-to-robot slam (`Trans_Vehicle2RobotSlam`): for a
/// player only. `startCamera` holds the finish target (0072df10 -> 0072e100): the nearest
/// character in a cone round his facing (45 degrees either side; 90 up or down, so any height)
/// within 6 + 2 x his body radius (STEP `+0x170[slot]`), whose health is at or under the playing
/// clip's attack damage (00717c80), so only an enemy the slam would kill. Else `endCamera`
/// lets it go (0072e080) and `killDude` lands a melee hit on it (0072dfd0 -> 007189b0) at its
/// camera target point (0071a2e0: the root for a character on foot). [game] [stand-in: the
/// finisher camera the hold starts (009192c0 -> camera behaviour 00917b40, `notes\
/// camera-finisher.md`) is the host's to do from `State::finish_target`, not built; settles: camera behaviour 00917b40 and the hold's camera frames, a recording of the hold] While a
/// target is held, HIS `+0x14c` bit 21 is set (0072df10 sets it on the attacker), which stops
/// the stick turning him (`attack_aim`). [game] [assumed: the reach and the cone from his feet;
/// the kill carries the clip's own attack numbers as a fist's hit would; the hold also ends
/// when the slam's state is left: every roster `FinishMoveEvent` carries `startCamera`, so
/// the `endCamera` branch that would end it is never taken (`work\packdump`); settles: the
/// state exit 00879700 and the clearing of the hold, not read]
fn finish_move(s: &mut State, t: &Tuning, arena: &dyn World, start: bool, end: bool, kill: bool) {
    if !s.actor.player {
        return;
    }
    let attack = s.action.as_ref().and_then(|a| t.actions.clips.get(&a.clip).map(|data| (a.clip, data.attack)));
    if start {
        let Some((_, Some(attack))) = attack else { return };
        let threshold = attack.damage_for(true, false);
        let (facing, reach) = (s.facing(), 6.0 + 2.0 * t.robot_radius);
        s.finish_target = arena
            .targets()
            .iter()
            .filter(|target| target.character && target.health <= threshold)
            .filter_map(|target| {
                let to = target.pos - s.pos;
                let flat = to.truncate();
                let ahead = flat.length_squared() < 1e-8 || facing.angle_to(flat).abs() <= std::f32::consts::FRAC_PI_4;
                (ahead && to.length() <= reach).then_some((target.id, to.length()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id);
        return;
    }
    if end {
        s.finish_target = None;
    }
    if kill {
        let Some(id) = s.finish_target else { return };
        let Some((clip, Some(attack))) = attack else { return };
        let Some(target) = arena.targets().iter().find(|target| target.id == id) else { return };
        let facing = s.facing();
        s.hits.push(Hit {
            target: id,
            clip,
            damage: attack.damage_for(true, false),
            point: target.pos,
            body: 0,
            direction: melee::attack_direction(attack.attack_dir, facing, s.pos, target.pos),
            knock_back_speed: attack.knock_back_speed,
            knock_back_angle: attack.knock_back_angle.to_radians(),
            flags: attack.flags_for(Some(target.large as u8)),
            surface: target.surface,
            blast: false,
            stun_time: 0.0,
        });
    }
}

/// `ChangeTractionEvent` (00712960 -> motion_control_set 00854960): the current motion mode's
/// control vector, how much of each axis the clip drives. [game] Only the modes that take it:
/// in the port's terms the death (BasicMotionMode, 0087b070), where the vertical axis follows
/// the root while z is 1 and falls under gravity while it is 0 (`Death_Explode_Fall` gives
/// [1, 1, 0] in all 13 packs [data]); and standing (BasicMotionMode), where with a body the
/// setter also stops him along the ground, keeping his vertical speed [game 00854960, read by
/// hand from 008549c7-008549f8: assumed until checked again]. FlyMode's (00866900) is
/// `flight::Fly::traction`: stored while set, the all-zero vector ends a stunt; the stunt loops
/// and rolls carry [1,1,1] at their start and [0,0,0] at their end [data]. HoverMode's is stored
/// (`hover_control`) and drives the body through `step_hover` (`Trans_V2R_Hover`'s [0,0,1]).
/// ClimbMode's (`+0x18 == 6`, the pull-up) is not applied: no climb clip of the 13 packs has a
/// traction event [data] (`notes\clip-events-traction-modes.md`). The port's hit reactions and stun run their own motion (`step_hit_reaction`)
/// and do not take it either. The hit clips have no traction event in the 93 uses read
/// (notes/clip-events-traction-motion.md section 2, [data]); the stun's StunStart carries [1,1,0].
/// [game, settled 2026-10-20] StateStun's enter is 0087eaf0 (vtable 00b62dd8, slot 5): it switches to
/// BasicMotionMode (1ad6ae68) and calls motion_control_set(1,1,0); 00712960 (the StunStart event)
/// writes the same vector again. 00854960 then zeroes the body's ground velocity, keeping vz, when the
/// body (STEP+0x38) exists and the override STEP+0x3d4 is clear; that override is written only by
/// 00854ad0, whose callers are 007efa90 (a vtable 00b60c04 handler, not the stun) and 0072d030 (the
/// damage timers clearing it), so a stun runs with it clear. Each update the owner then hands
/// (1,1,0) to the holder: the clip's root drives the ground axes, gravity the vertical one. The
/// port's `Mode::Action` with `step_hit_reaction` gives the same, as the measured test shows
/// (`a_stun_drops_the_ground_speed_for_the_clips_and_keeps_falling`: 24 m/s running to the clip's
/// 0 on the first update; vz -3.36, -4.48, -5.6 in the air), so the mode name differs and nothing else.
fn clip_traction(s: &mut State, t: &Tuning, control: Vec3) {
    if let Some(death) = s.death.as_mut() {
        death.root_z = control.z != 0.0;
        return;
    }
    // [game 00866900] the FlyMode branch (`flight::Fly::traction`): stored while set (the stick
    // and the stunt trigger wait, the clip's root drives the body: `step_fly`); all zero ends a
    // running stunt along the nose.
    if s.mode == Mode::Fly {
        if let Some(fly) = s.fly.as_mut() {
            fly.traction(control, &t.forms.fly);
        }
        return;
    }
    // [game 00854960] HoverMode's vector is stored; its update never reads it, the body's control
    // step does (`step_hover`).
    if s.mode == Mode::Hover {
        s.hover_control = control;
        return;
    }
    if s.mode == Mode::Stand {
        (s.vel.x, s.vel.y) = (0.0, 0.0);
    }
}

/// `ChangeMotionModeEvent` (00712a20 -> mode_switch_by_type 00852150): the first of his modes of
/// that class becomes the motion mode; nothing if it is already, or if he has none of it.
/// [game] The roster's data names three (`work\packdump`): StrafeMode in the weapon set's
/// landings and runs, JumpMode in `Weapon_Fall`, the first mode in `Hit_WallFront_2_Fall`.
/// [data] The port's hit reactions, stun and death are the game's first mode under another
/// name (`Mode::Action`, the death's own step), so the first mode leaves those as they are.
/// JumpMode from a clip: [game, settled 2026-10-20] the request branch's three hashes are the OLD
/// mode's class (vt+0xc of param_2): f081d468 (the car's companion, `UNFOLD_MODE`) or e2b22eb4
/// (DriveMode) on the ground give HIGH_STANDING / LONG_JUMP by speed (l.100-107), FlyMode fb9f3262 for
/// a player gives LONG_JUMP at speed^2 >= 25 or else a fall (l.111-118); `FUN_007106f0` is only the
/// animation-variable lookup that writes C_JUMP_TYPE afterwards (l.132-138). `enter_jump_mode` has this
/// table; the clip's call now passes the weapon set (no set name) instead of FallSet. History: read 2026-10-20 (008740e0,
/// JumpMode's enter, the table of `notes\bumblebee-mechanics.md`): the game takes the set from the
/// layer that is playing (`iVar6`). FallSet, or an owner without the flag +0x26c, gives type
/// 0xaa185eeb with no request test; any other set goes to the request branch (the requests from
/// the old mode's hash, the flag +0x26c, the speed test), which the port does not have. So the port's
/// constant `FallSet` is right for a fall clip only. Read again 2026-10-20 (part 2): the JumpMode
/// events sit in `Weapon_Fall`, which is a weapon-set clip, not FallSet (weapon-mode.md l.76). So the game
/// takes the request branch (008740e0 l.104-123, `iVar6` not FallSet) whenever the owner's +0x26c is
/// set, i.e. grounded: HIGH_STANDING or LONG_JUMP by the request, else a fall when the speed is tiny and
/// a jump type otherwise. (Those "requests" are the old mode's class, read above; fixed 2026-10-20.)
fn clip_motion_mode(s: &mut State, input: &Input, t: &Tuning, class: u32) {
    let mode = match class {
        FIRST_MODE => Mode::Stand,
        STRAFE_MODE => Mode::Strafe,
        HOVER_MODE => Mode::Hover,
        JUMP_MODE => {
            // [game 008740e0 l.75, l.99-124] the set is the playing layer's: for `Weapon_Fall`, the
            // only clip that names JumpMode, the weapon set (not ClimbSet, GroundPunchSet or FallSet),
            // so the mode's own table decides from the old mode, the ground flag and the stick.
            let on_ground = s.ground_flag();
            enter_jump_mode(s, input, t, None, on_ground);
            return;
        }
        // [data] none other is named by the roster's clips: the 68 ChangeMotionModeEvent uses in
        // the 13 dumps carry only the three values above (notes/clip-events-traction-motion.md section 3).
        _ => return,
    };
    if mode == Mode::Stand && (s.death.is_some() || s.mode == Mode::Action) {
        return;
    }
    set_mode(s, input, t, mode);
}

/// `ForceUpEvent` (00712ad0): his body is turned so its z is `up` (align_z_matrix 0056e7b0),
/// keeping his heading; no push. [game] With the port's frame (yaw, then pitch about x, then
/// roll about y) the body's z in the heading's frame is (sin roll, -sin pitch cos roll,
/// cos pitch cos roll). All six uses are [0, 0, 1], which stands him upright. [data]
fn clip_force_up(s: &mut State, up: Vec3) {
    let Some(up) = up.try_normalize() else { return };
    let local = Quat::from_rotation_z(-s.yaw) * up;
    s.roll = local.x.clamp(-1.0, 1.0).asin();
    s.pitch = (-local.y).atan2(local.z);
}

/// The attack and dash modes: the clip's root motion moves him, faded in over what he was
/// doing; the attacks turn toward the stick while their clip allows it, the dodge keeps the
/// heading it started with. [game]
fn step_action(s: &mut State, input: &Input, t: &Tuning, arena: &dyn World, dt: f32) {
    let Some(action) = s.action.as_ref() else {
        s.mode = Mode::Move;
        return;
    };
    let def = &t.control.states[action.state];
    if def.class == "StateDash" {
        s.yaw = action.heading;
    } else if def.motion_mode == ATTACK_MODE {
        attack_aim(s, input, t, arena, dt);
    } else if def.class == "StateAttackSpecial" && def.motion_mode == STRAFE_MODE {
        // Megatron's special runs in StrafeMode (`stateAttackSpecialMegatron`, motion 3c956e02):
        // the mode's update with the stick taken as zero, as it is while a StateAttackSpecial of
        // value 4 runs, so its standing branch: `F_STRAFE_ANG` is his aim against his body, his
        // body turned by what is past 45.5 degrees, the spine twisted by the rest. Its set's
        // rules turn that into `SpecialAttack_IdleTurnL` / `R` (at 45 either way). [game
        // 00875ef0] [data] The port keeps the special's own clip playing (its `Mode::Action`)
        // rather than the weapon set StrafeMode would otherwise lead: no weapon set starts in
        // this state. [assumed: the mode's enter bookkeeping (aim heading from his facing)
        // is the only part of the enter that matters here] settles: the StrafeMode enter's other writes, read from its code (not located; `notes\weapon-mode.md` l.150-170).
        let aim_yaw = input.aim.filter(|aim| aim.length_squared() > 1e-6).map_or(s.strafe.aim_yaw, yaw_of);
        s.strafe.aim_yaw = aim_yaw;
        s.strafe.strafing = true;
        let off = wrapped(aim_yaw - s.yaw);
        s.strafe.angle = off.to_degrees();
        if off.abs() > STRAFE_TWIST_MOST {
            s.yaw = wrapped(s.yaw + off - STRAFE_TWIST_MOST.copysign(off));
        }
        s.strafe.twist = off.clamp(-STRAFE_TWIST_MOST, STRAFE_TWIST_MOST);
    }
    let Some(action) = s.action.as_ref() else { return };
    let Some(data) = t.actions.clips.get(&action.clip) else { return };
    // Root motion over this step, from the clip's own frame to the world's. A loop that came
    // round counts for nothing on that step.
    let moved = if dt > 1e-6 { action.root_travel(data) / dt } else { Vec3::ZERO };
    // A clip whose root turns turns him with it, and its travel is in the frame it started
    // in (only `Wall_JumpOff` turns: 124 degrees). [assumed: the original moves the body by
    // the root's whole transform; the dodges, whose root sits turned and never turns, agree] settles: the game's root-motion code (the body's move by the clip root, not located) with a turning clip such as `Wall_JumpOff`.
    let mut facing = s.facing();
    if data.turn.iter().any(|turn| turn.abs() > 1e-3) {
        s.yaw = action.heading + data.turn_at(action.tick);
        facing = Vec2::new(-action.heading.sin(), action.heading.cos());
    }
    let right = Vec2::new(facing.y, -facing.x);
    let own = right * moved.x + facing * moved.y;
    let mut v = action.carry.lerp(own, action.fade);
    // `stop_on_hit` (the charge attack): once it has hit a character, on the ground, the
    // animation's root mode goes to 2 (`FUN_00855af0`: the clip's root no longer moves the
    // body) and the hit stops him dead (`FUN_00717d80`: a melee hit that is not a knock-back
    // kind zeroes his velocity). Taken back when the attack state ends. [game]
    if def.motion_mode == ATTACK_MODE
        && action.has_hit
        && s.ground_flag()
        && data.attack.is_some_and(|attack| attack.stop_on_hit)
    {
        v = Vec2::ZERO;
    }
    (s.vel.x, s.vel.y) = (v.x, v.y);
    if action.from_climb || action.lifted {
        // Off the wall the clip lifts him too: [game 00856cc0] a BasicMotionMode state's control is
        // (1,1,1), and the body step (006db490) moves the body's z toward the clip's root z per axis.
        // The climb's jumps: ClimbSet animRef[4] (Wall_JumpUp, 12 ticks, +3.58 z) and animRef[5] (18
        // ticks, +0.57 z, probably Wall_JumpOff: its name is not in the dump) [data]. [data, settled
        // 2026-10-20] Every BasicMotionMode state's clip with root z, over the 13 packs: the climb's two jumps,
        // stateDriveAttackAirPunch (Trans_Vehicle2Robot2GroundPunch, up to 7.2 m, keys [1,1,1]) = the `lifted`
        // leg out of the car, stateFlyTransAirPunch (Trans_V2R_GroundPunch, up to 8.9 m, keys [1,1,1]) = the
        // `lifted` leg out of flight (added 2026-10-20; test `the_jets_punch_out_of_flight_is_lifted_by_its_clip`),
        // and the ground punches (down 1.5 m at most). (The car's Trans_V2R_A / _B play in the unfold mode
        // f081d468, not here.) [assumed: the ground punches' downward root z on the floor ends as gravity does,
        // held by the floor] settles: a ground punch landing on a slope or a ledge in the game.
        s.vel.z = moved.z;
    }
    s.speed = v.dot(s.facing());
    if def.motion_mode == ATTACK_MODE && !s.ground_flag() {
        // Off the ground the attack mode lets him rise no faster than this. [game]
        s.vel.z = s.vel.z.min(ATTACK_RISE_MOST);
    }
}

/// The attack mode turns to a target only when it is more than this off it (`FUN_00717770`),
/// and caps his upward speed at this while he is off the ground. [game]
const ATTACK_TURN_LEAST: f32 = 0.017_453_292;
const ATTACK_RISE_MOST: f32 = 2.0;
/// The slide wants the target further than this (squared), and the stick counts as held for
/// choosing a target from this deflection. [game]
const SLIDE_LEAST_SQUARED: f32 = 0.1;
const AIM_STICK_LEAST: f32 = 0.001;

/// The attack mode's own part of its update (`FUN_00855af0`), before the clip moves him.
/// Until the attack has hit a character, and while the clip allows a slide or a turn
/// (`AttackFaceAndSlideEvent`): it looks along the stick, or along his facing with no stick,
/// for a target (`melee::Query`), once, and keeps it; while the target is further than
/// touching (the two radii) it turns him to it at `auto_target_turn_speed` and slides him
/// toward it at `slideSpeed`, by moving the body itself, as long as the target is between
/// `minRange` and `maxRange` away. Otherwise, while the clip allows stick movement, he
/// turns toward the stick at `turn_speed`; not after a hit if the clip says `stop_on_hit`.
/// [game] Not read, so not here: character flag 0x15, which also stops the stick's turn;
/// (`stop_on_hit` itself is done in `step_action`);
/// the test that leaves a target alone in one of its states (`FUN_007258d0`); the attack
/// mode in the air (its `+0x2a`), whose slide keeps to his height: his air attack has no
/// slide speed.
fn attack_aim(s: &mut State, input: &Input, t: &Tuning, arena: &dyn World, dt: f32) {
    use std::f32::consts::PI;
    let Some(action) = s.action.as_ref() else { return };
    let Some(attack) = t.actions.clips.get(&action.clip).and_then(|clip| clip.attack) else { return };
    let (slide, face, stick) = (action.slide, action.face, action.stick);
    let targets = arena.targets();
    let find = |id: u32| targets.iter().find(|target| target.id == id);
    let has_hit = action.has_hit || s.hit_list.iter().any(|&id| find(id).is_some_and(|target| target.character));
    // A target that is gone is looked for again.
    let mut target = action.target.filter(|&id| find(id).is_some());
    let mut turned = false;
    if !has_hit && (slide || face) {
        let mut direction = s.facing();
        // While the slam's finisher holds a target his `+0x14c` bit 21 is set (0072df10, cleared
        // by 0072e080), which stops the stick turning him in the attack mode. [game] (melee.md)
        if input.stick.length() >= AIM_STICK_LEAST && s.finish_target.is_none() {
            direction = input.stick.normalize();
            // A stick further round than the clip allows is taken at the limit. As the
            // original computes it, the limit is put on the side AWAY from the stick, which
            // looks like a slip of theirs; kept as it is. Only the slam out of the car has a
            // limit under half a turn (110 degrees). [game]
            let most = attack.stick_choose_target_max_degrees.to_radians().clamp(0.0, PI);
            let off = melee::wrap(s.yaw - yaw_of(direction));
            if most < PI && off.abs() > most {
                let yaw = melee::wrap(if off > 0.0 { s.yaw + most } else { s.yaw - most });
                direction = Vec2::new(-yaw.sin(), yaw.cos());
            }
        }
        if target.is_none() {
            let angle = attack.auto_target_search_angle.to_radians().clamp(0.0, PI);
            target = melee::Query::new(s.pos, direction.extend(0.0), attack.max_range, angle, &t.melee).best(targets, &t.melee);
        }
        if let Some(found) = target.and_then(find) {
            let d = found.pos - s.pos;
            let flat = Vec2::new(d.x, d.y);
            let touching = t.robot_radius + if found.character { found.radius } else { 0.0 };
            if flat.length_squared() > touching * touching {
                if attack.auto_target_turn_speed > 0.0 && face {
                    let off = melee::wrap(yaw_of(flat) - s.yaw);
                    if off.abs() > ATTACK_TURN_LEAST {
                        let most = attack.auto_target_turn_speed.to_radians() * dt;
                        s.yaw += off.clamp(-most, most);
                    }
                    turned = true;
                }
                let away = d.length_squared();
                let in_range = away > attack.min_range * attack.min_range && away < attack.max_range * attack.max_range;
                if attack.slide_speed > 0.0 && slide && in_range && away > SLIDE_LEAST_SQUARED {
                    let away = away.sqrt();
                    // A whole step even if it ends inside touching distance; the bodies'
                    // own collision parts them again. [game]
                    s.pos += d / away * (attack.slide_speed * dt).min(away);
                }
            }
        }
    }
    let may_turn = stick && attack.turn_speed > 0.0 && !(attack.stop_on_hit && has_hit);
    if !turned && may_turn && input.stick.length_squared() > STICK_DEAD_SQUARED {
        s.yaw = turn_toward(s.yaw, yaw_of(input.stick), attack.turn_speed.to_radians() * dt);
    }
    if let Some(action) = s.action.as_mut() {
        (action.target, action.has_hit) = (target, has_hit);
    }
}

/// The climb mode's update, and the climbable test run again from where he is. [game]
fn step_climb(s: &mut State, input: &Input, t: &Tuning, arena: &dyn World, dt: f32) {
    let Some(mut climb) = s.climb.take() else {
        s.mode = Mode::Jump(JumpType::Fall);
        return;
    };
    let on_ground = s.pos.z <= arena.floor(s.pos.x, s.pos.y, s.pos.z) + 0.01;
    let wall = climb::find(arena, s.pos, s.facing(), t.robot_radius, t.robot_height, t.jump.standing.height);
    let front_contact = arena.front_contact(s.pos, s.facing(), t.robot_radius, t.robot_height);
    let body = climb::Body { pos: &mut s.pos, yaw: &mut s.yaw, radius: t.robot_radius, height: t.robot_height, on_ground, front_contact };
    let (velocity, message) = climb::update(&mut climb, body, input.pad_stick, wall, t, dt);
    s.vel = velocity;
    s.speed = 0.0;
    s.gait = Gait::Idle;
    s.messages.extend(message);
    // Every update, by what he is doing on the wall after it (`FUN_00856d00`): up and the
    // shimmies start preset 9, down preset 10. [game] Preset 9 lasts 0 seconds, so going
    // up or sideways is silent and going down hums; that is what the data gives. [data]
    match climb.phase {
        climb::Phase::Up | climb::Phase::ShimLeft | climb::Phase::ShimRight => preset_rumble(s, t, rumble::preset::CLIMB_MOVED),
        climb::Phase::Down => preset_rumble(s, t, rumble::preset::CLIMB_SLIDING),
        _ => {}
    }
    s.climb = Some(climb);
}

/// The car mode. When he leaves it, and how (the unfold, the leap out with the jump button
/// held as the trigger goes up), is the state machine's (`stateDrive`'s transitions). [game]
fn step_drive(s: &mut State, input: &Input, t: &Tuning, arena: &dyn World, dt: f32) {
    // The game's rules are written per update and several depend on the update's length (the
    // wheel angle has it built in, and the engine pushes for one update past the speed cap),
    // so they are run in whole updates of the game's own length whatever the caller's step.
    // Wheel queries, drive limits, spring accumulation and body integration form
    // one physics update. Splitting integration at the render rate changes the
    // spring damping and clips the response between forces. [game/trace]
    s.car.turbo_asked |= input.turbo;
    s.car.clock += dt;
    s.gearbox_inputs.clear();
    s.tyre_inputs.clear();
    if s.car.chassis.wheels[0].rest_length<=0.0 {
        s.car.chassis.probe(s.pos,s.vel,crate::vehicle::rotation(s.yaw,s.pitch,s.roll),
            t.vehicle_height,&t.drive,arena,GAME_UPDATE);
        s.car.airborne=!s.car.chassis.grounded();
    }
    while s.car.clock >= GAME_UPDATE {
        s.car.clock -= GAME_UPDATE;
        s.car.previous_presentation=Some(s.car_snapshot());
        let mut angles=Vec3::new(s.pitch,s.roll,s.yaw);
        s.car.chassis.prepare_frame(&mut s.pos,&mut angles,&t.drive);
        (s.pitch,s.roll,s.yaw)=(angles.x,angles.y,angles.z);
        let was_grounded=s.car.chassis.grounded();
        s.car.chassis.probe(s.pos,s.vel,crate::vehicle::rotation(s.yaw,s.pitch,s.roll),
            t.vehicle_height,&t.drive,arena,GAME_UPDATE);
        s.car.airborne=!s.car.chassis.grounded();
        if !was_grounded && !s.car.airborne && s.car.time>CAR_LANDING_AFTER {
            preset_rumble(s,t,rumble::preset::VEHICLE_HIT_GROUND);
        }
        drive_update(s, input, t, GAME_UPDATE);
        drift_step(s, input, t, GAME_UPDATE);
        s.car.chassis.finish_step(&mut s.pos,&mut s.vel,GAME_UPDATE);
        let speed_before=s.vel.length();
        let mut angles=Vec3::new(s.pitch,s.roll,s.yaw);
        let contacts=s.car.chassis.advance_kept(&mut s.chassis_body,s.actor.id,&mut s.pos,&mut s.vel,&mut angles,
            t.vehicle_height,t.gravity,&t.drive,arena,GAME_UPDATE);
        (s.pitch,s.roll,s.yaw)=(angles.x,angles.y,angles.z);
        s.car.yaw_rate=s.car.chassis.angular.z;
        // A head-on hit stops the wheels (`FUN_00860870` -> `FUN_007a38e0`). [game] Both tests below take
        // only a contact of kind 3: the listener record's +0x3c is the contact state code 006d7c20 writes
        // (3 = approaching faster than 0.5 along the normal, `collisionfx::physics_type`; the record handed
        // to the listeners at l.409-426 carries the normal at +8 and the code at +0x3c, the two fields
        // 00860870 reads). [game, settled 2026-10-20]
        let forward=crate::vehicle::rotation(s.yaw,s.pitch,s.roll)*Vec3::Y;
        let approaching=|c: &crate::vehicle::Contact| crate::collisionfx::physics_type(c.normal,c.velocity)==3;
        let stop=contacts.iter().any(|c|approaching(c) && crate::vehicle::head_on(c.normal,forward,
            t.drive.collision_forward_dot));
        let knock=contacts.iter().any(approaching);
        s.vehicle_contacts.extend(contacts);
        if stop {
            s.car.chassis.stop_wheels();
            s.car.spin=[0.0;4];
        }
        // [game 00860870 l.105-111] rumble 0xc when the body's speed (all three axes) is over 5 and the
        // contact is kind 3; no wall test of its own.
        if speed_before>CAR_KNOCK_SPEED && knock {
            preset_rumble(s,t,rumble::preset::VEHICLE_HIT_WALL);
        }
    }
    s.speed = Vec2::new(s.vel.x, s.vel.y).dot(s.facing());
}

/// The drift turbo's own update (`00733e10`, slot 2 of its vtable 00b5d630), AFTER the car's
/// update, on the velocity the car wrote back [game, wave 29 Q11]: drive_enter 0085e130 puts the
/// drift object on the character's ability list (char+0x378, ability_register 007235f0 at
/// 0085eb5c-0085eb70), and ability_list_walk 0071eb50 runs it from character_update 0071dc20 l.146,
/// after the movement step (DriveMode 0085cad0 via 00852220) at l.84. It takes dt only and reads the
/// car itself. The boost needs lateral motion, not just a fast slide; its active state continues if
/// the drift button is pressed again, even in the air. [game 00733e10] It reads the button from the
/// owner's controller (owner+0x100, vtable +8, id 0x14 at 0073400e-0073401f: the drift button,
/// notes\driving.md) and the lateral speed as fabs(dot(velocity, right)) with the right row of
/// character_transform 00725a70, against minDriftTurboSpeed (`DriftTurbo::step` takes the absolute
/// value); 00732ce0 is only the ability-active test (wave 30 R5).
fn drift_step(s: &mut State, input: &Input, t: &Tuning, h: f32) {
    let d = &t.drive;
    let facing = s.facing();
    let right = Vec2::new(facing.y, -facing.x);
    let drift = s.car.drift_turbo.step(Vec2::new(s.vel.x, s.vel.y).dot(right), input.drift, s.turbo_left > 0.0, d, h);
    if drift.started {
        s.turbo_started = true;
        s.turbo_fx_on = true;
        preset_rumble(s, t, rumble::preset::VEHICLE_TURBO);
    }
    if drift.ended {
        s.car.after_turbo = Some(d.max_drift_turbo_speed);
        s.turbo_fx_on = false;
    }
}

/// One update of the car, as the drive mode and its wheel set do it (`notes\driving.md`),
/// with independent 3D wheel forces; caps/spin response checked against run1. [game/trace]
fn drive_update(s: &mut State, input: &Input, t: &Tuning, h: f32) {
    let d = &t.drive;
    let facing = s.facing();
    let mut velocity = Vec2::new(s.vel.x, s.vel.y);
    let set_speed = |velocity: &mut Vec2, speed: f32| {
        if velocity.length() > 1e-4 {
            *velocity = velocity.normalize() * speed.max(0.0);
        }
    };
    let grounded = !s.car.airborne;
    s.car.time += h;
    let braking = input.brake > STICK_DEAD && grounded;
    // Both caps end with the status scale (`0085f8b0`, `0085fc80`). [game]
    let cap_scale = crate::driving::cap_scale(s.damage_timers.slowed > 0.0, t.damage.slow_multiplier,
        s.damage_timers.bullet_slowed > 0.0, d.bullet_slow_speed_percent,
        s.special.as_ref().is_some_and(|a| matches!(a.kind, crate::special_types::SpecialType::Scout) && a.left > 0.0));

    // [game: 0085cd56..0085cda0] Player trigger filtering happens before the caps.
    s.car.filtered_trigger = crate::driving::filter_trigger(s.car.filtered_trigger,
        input.vehicle.clamp(0.0, 1.0), d.trigger_filter_strength, h);
    // The drift turbo as its own step left it last update (`drift_step`, after this one). [game]
    let right = Vec2::new(facing.y, -facing.x);
    let slide_boost = s.car.drift_turbo.active();

    // The slide: while its button is held with the wheels down, every tyre keeps only a
    // little of its grip and the car is slowed along its own motion. [game]
    let sliding = input.drift && grounded;
    if sliding {
        let speed = velocity.length();
        set_speed(&mut velocity, (speed - d.skid_out_deceleration * h).max(0.0));
    }
    s.car.sliding = sliding;

    // Turbo: runs its time, then waits out its cooldown. [game: 00733180/00732e60]
    if std::mem::take(&mut s.car.turbo_asked) && s.turbo_left <= 0.0 && s.turbo_cooldown <= 0.0 {
        s.turbo_left = crate::driving::turbo_times(d, s.turbo_upgrades).0;
        s.turbo_started = true;
        s.turbo_fx_on = true;
        // The turbo starting rumbles the pad (`FUN_0085cad0`). [game]
        preset_rumble(s, t, rumble::preset::VEHICLE_TURBO);
    }
    let boosting = s.turbo_left > 0.0;

    // Reverse: the brake held at a stop for a moment; over when the brake is up and he is
    // no longer rolling backward.
    let forward_speed = velocity.dot(facing);
    let mut stopped_by_brake = false;
    if !braking {
        s.car.brake_held = 0.0;
        if forward_speed >= 0.0 {
            s.car.reversing = false;
        }
    }

    // The speed caps, first that applies. The trigger scales the top speed, not the push.
    let speed = velocity.length();
    if !braking || s.car.reversing {
        s.car.braking_from = None;
    }
    if boosting {
        s.car.after_turbo = None;
        if speed > d.max_turbo_speed * cap_scale {
            set_speed(&mut velocity, d.max_turbo_speed * cap_scale);
        }
    } else if slide_boost {
        if speed > d.max_drift_turbo_speed {
            set_speed(&mut velocity, d.max_drift_turbo_speed);
        }
    } else if s.car.reversing {
        if speed > d.max_reverse_speed {
            set_speed(&mut velocity, d.max_reverse_speed);
        }
    } else if braking {
        let from = s.car.braking_from.unwrap_or(speed);
        let now = (from - d.braking_deceleration * h).max(0.0);
        s.car.braking_from = Some(now);
        set_speed(&mut velocity, now);
        if now <= 0.0 {
            // Braked to a stop: the engine is cut, and holding on a moment longer reverses.
            velocity = Vec2::ZERO;
            s.car.spin = [0.0; 4];
            stopped_by_brake = true;
            s.car.brake_held += h;
            if s.car.brake_held >= d.min_reverse_brake_time {
                s.car.reversing = true;
            }
        }
    } else {
        let normal = crate::driving::allowed_speed(d, s.car.filtered_trigger, true) * cap_scale;
        let cap = match s.car.after_turbo {
            Some(ramp) if ramp - d.post_turbo_deceleration * h > normal => {
                let ramp = ramp - d.post_turbo_deceleration * h;
                s.car.after_turbo = Some(ramp);
                ramp
            }
            _ => {
                s.car.after_turbo = None;
                normal
            }
        };
        if speed > cap {
            set_speed(&mut velocity, cap);
        }
    }

    s.car.capped = velocity.length();
    // What the wheel set shows the gearbox this update (`FUN_007a48e0`). [game]
    // [game] The trigger is filtered; turbo is either cee630d9 (standard) or ea8020ae
    // (drift), whose identities are confirmed in DriveMode's constructor 0085c2f0.
    // [assumed] (`notes\sound.md`, 3 and 5): brake amount and the velocity's sampling
    // order relative to the vehicle sound update. The velocity read here is the body's at the start
    // of this update, before the caps (the capped one stays in `velocity` until the write-back below;
    // `capped` is its length). The game's sound object (vtable 00b5d64c slot 0, 007347c0) has no direct
    // caller: 0085cad0 writes the capped velocity through 00557000 (cleaned l.143-405) before its
    // wheel_pre step (007a2860). settles: whether that sound object updates before or after this mode
    // update, from the owner that holds the sound object's vtable. Part 3 (2026-10-20): 00b5d64c is not a
    // primary vtable. It is the interior table at +0x14 of the 12-entry vtable 00b5d638 (slot 5 = 007347c0,
    // the sound update; slot 0 = 00733e10, the drift turbo's update; 00b5d650 is its slot 6). [data 00b5d638]
    // No literal store of 00b5d638 is in the executable (scan of 00401000-00b00000), so the owner's
    // constructor is not located; the open part is the update order (RDb2a18c).
    s.gearbox_inputs.push(GearboxInput {
        velocity: [s.vel.x, s.vel.y, s.vel.z],
        trigger: s.car.filtered_trigger,
        stick_x: input.steer,
        braking: braking && !s.car.reversing,
        skid: sliding,
        reversing: s.car.reversing,
        stopped_by_brake,
        grounded,
        turbo: boosting || slide_boost,
    });
    if let Some(gearbox)=s.vehicle_gearbox.as_mut() {
        gearbox.update(s.gearbox_inputs.last().unwrap(),h);
    }
    // What the same update shows the tyres' and the suspension's sounds (`FUN_007347c0`,
    // `notes\sound.md` "The tyres and the suspension"). [game]
    // [game: 0085f8b0, dt=0] The tyre sound's cap skips the trigger, even if filtered.
    // The brake is digital [game 0085cad0 l.30-48]: button 0x13 (VehicleBrake: pad L1, keyboard R, mouse none)
    // read through the input object's vtable +8, so its amount is 1.0 or 0 and `braking = input.brake >
    // STICK_DEAD` is that bit; 0x14 (VehicleDrift: pad L2, mouse right button) is the skid flag with a wheel
    // down (and the drive mode's +0x1f5); the brake flag is 0x13 with a wheel down, or a kind-4 vehicle special.
    // The stick never reaches the brake: vtable +0x30 (0084fcd0) feeds the drive force and the steer (l.49-51,
    // l.496-501), and the reverse starts from the brake held at a stop (notes\driving.md "Reverse"). The port's
    // old stick-pulled-back brake had no game source and was removed from game\src\player.rs on 2026-10-20.
    // Contact/material and shortening are independent for all four wheels. [game]
    let wheels=s.car.chassis.wheels.map(|w|crate::sound::TyreWheel {
        grounded:w.grounded,was_grounded:w.was_grounded,point:w.point,surface:w.surface,
        compression:(w.previous_length-w.length)/w.rest_length.max(0.01),
    });
    let sound_wheel=if velocity.dot(right)<0.0 {2} else {3};
    let suspension_wheel=if wheels[2].compression>wheels[3].compression {2} else {3};
    s.tyre_inputs.push(TyreInput {
        speed: s.vel.length(),
        drift: input.drift,
        brake: input.brake,
        reversing: s.car.reversing,
        waiting_to_reverse: stopped_by_brake,
        stick_x: input.steer,
        cap: crate::driving::allowed_speed(d, s.car.filtered_trigger, false) * cap_scale,
        grounded,
        surface: if wheels[sound_wheel].grounded {wheels[sound_wheel].surface} else {0},
        compression: wheels[suspension_wheel].compression,
        wheels,per_wheel:true,sound_wheel,suspension_wheel,
    });

    // What the engine is asked for.
    let throttle = if stopped_by_brake {
        0.0
    } else if boosting {
        d.throttle * d.turbo_throttle_boost
    } else if slide_boost {
        d.throttle * d.drift_turbo_boost_factor
    } else if s.car.reversing {
        -d.throttle
    } else if braking {
        if forward_speed >= 0.0 { d.throttle } else { -d.throttle }
    } else if forward_speed < 0.0 {
        d.post_reverse_throttle
    } else {
        d.throttle
    };

    // Steering: the front wheels are turned by this much, an angle with the update's length
    // in it. Fast at a standstill, slower at speed.
    let pace = (velocity.length() / d.max_speed.max(1.0)).clamp(0.0, 1.0);
    let steer = (d.max_turn_speed - (d.max_turn_speed - d.turn_speed) * pace) * -input.steer.clamp(-1.0, 1.0) * h;
    // 007a10d0 rotates physical tyre axes by NEGATIVE mode steer;008601d0
    // multiplies that mode steer by -3 for the visual. Keep the mode value,
    // rather than exposing the already-negated physical angle to animation. [game]
    s.steer_angle = -steer;

    // Each wheel: the engine spins it up, and the tyre pushes on the body by how far its rim
    // and the ground under it disagree. Sideways slip is opposed outright; forward push comes
    // only from the wheel spinning faster than the ground, so it builds over about a second.
    let turn=crate::vehicle::rotation(s.yaw,s.pitch,s.roll);
    // 007a34c0 aligns the wheel-force frame to the support plane before 007a2860.
    // The spring anchors use the restored body's own orientation in physics. [game]
    let support=crate::vehicle::support_rotation(turn,s.car.chassis.normal);
    let mut velocity3=velocity.extend(s.vel.z);
    s.car.chassis.render_step(velocity3,turn,stopped_by_brake,boosting||slide_boost,h);
    s.car.chassis.angular.z=s.car.yaw_rate;
    let (side_grip, forward_grip) = if sliding { (d.wheel_drift_factor, d.drift_steering_factor) } else { (1.0, 1.0) };
    if !grounded {
        // 007a2860 clears torque for airborne wheels; no fictitious engine spin-up.
        velocity3+=(turn*Vec3::X*input.steer.clamp(-1.0,1.0)+turn*Vec3::Y*input.stick.y)*CAR_AIR_PUSH*h;
        s.car.yaw_rate -= s.car.yaw_rate * h;
        s.car.chassis.angular.z=s.car.yaw_rate;
        s.car.chassis.damp_angular(d);
        // 00862000's airborne recovery precedes world physics. [game]
        for angle in [&mut s.pitch,&mut s.roll] {
            if angle.abs()>0.17453294 {*angle-=angle.signum()*(0.17453294*h).min(angle.abs());}
        }
        s.vel=velocity3;
        return;
    }
    let up=turn*Vec3::Z;
    if up.z>0.9 && velocity3.dot(up)>0.0 {velocity3-=up*velocity3.dot(up)*0.95;}
    for (index,mount) in crate::vehicle::mounts(d).into_iter().enumerate() {
        if !s.car.chassis.wheels[index].grounded {continue;}
        let spin=&mut s.car.spin[index];
        let radius=if index<2 {d.front_wheel_radius} else {d.rear_wheel_radius};
        let radius=radius.max(0.1);
        let arm=support*mount;
        let at_wheel=velocity3+s.car.chassis.angular.cross(arm);
        let wheel_turn=support*Quat::from_rotation_z(if index<2 {steer} else {0.0});
        let rolling=wheel_turn*Vec3::Y;
        let sideways=wheel_turn*Vec3::X;
        let slip = at_wheel - rolling * (*spin * radius);
        let lateral = slip.dot(sideways);
        let lateral = if lateral.abs() < 0.01 { 0.0 } else { lateral };
        let along = slip.dot(rolling);
        let push = -sideways * lateral * d.wheel_friction * side_grip - rolling * along * forward_grip;
        *spin += (radius * along + d.engine_torque * throttle) / (radius * radius) * h;
        if !stopped_by_brake {s.car.chassis.add_force(push,arm);}
        s.car.chassis.wheels[index].spin=*spin;
    }
    s.car.chassis.transfer_weight(velocity3,support,d,stopped_by_brake,s.car.reversing,
        s.vehicle_gearbox.as_ref().map_or(0.0,crate::sound::Gearbox::progress));
    s.car.chassis.damp_angular(d);
    s.car.yaw_rate=s.car.chassis.angular.z;
    s.vel=velocity3;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::{DriveTuning, JumpTuning, Locomotion, MoveTuning, RootCurve};

    const DT: f32 = 1.0 / 60.0;

    /// The numbers Bumblebee has in the game's data, so the rules can be checked without the install.
    fn tuning() -> Tuning {
        let kind = |height, speed, control, turn_speed| JumpKind { height, speed, control, turn_speed };
        let mut t = Tuning {
            base_health: 320.0,
            health_regen_rate: 39.0,
            health_regen_delay: 1.5,
            robot_height: 5.4,
            robot_radius: 2.0,
            vehicle_height: 1.0,
            vehicle_radius: 2.5,
            wheelbase: 3.36,
            gravity: 35.0,
            transform_delay: 0.25,
            moving: MoveTuning { max_speed: 24.0, turn_speed: 360.0, walk_run_speed: 5.0 },
            locomotion: Locomotion {
                walk_speed: 5.61,
                run_speed: 24.0,
                move_stick: 0.01,
                run_stick: 0.7,
                idle_blend: 0.2,
                walk_blend: 0.25,
                run_blend: 0.25,
                // The `Root` channel of Run2Idle and Jump_LandRun, read every second tick.
                stop: every_second_tick(0.1, &[1.20, 1.70, 2.31, 2.97, 3.58, 4.05, 4.37, 4.56, 4.65, 4.68, 4.69]),
                land_run: every_second_tick(0.1, &[0.99, 1.02, 1.53, 1.97, 2.38, 2.80, 3.21, 3.63, 4.04, 4.46, 4.88]),
                big_land_run: RootCurve::new(0.0, (0..=28).map(|tick| 6.18 * tick as f32 / 28.0).collect()),
                takeoff: RootCurve::new(0.0, (0..=10).map(|tick| 0.99 * tick as f32 / 10.0).collect()),
                in_air_speed: 0.31,
                long_fall: 1.0,
            },
            jump: JumpTuning {
                standing: kind(6.0, 15.0, 0.1, 120.0),
                run: kind(10.0, 35.0, 0.038, 120.0),
                high_standing: kind(10.0, 35.0, 0.1, 65.0),
                long: kind(14.0, 40.0, 0.025, 65.0),
                climb_up_height: 15.0,
                climb_off_height: 15.0,
                climb_off_speed: 30.0,
                weapon_dilation_start: 5.0,
                weapon_dilation: 0.25,
                weapon_dilation_rate: 4.0,
            },
            climb: crate::tuning::ClimbTuning {
                pull_up_dist: 8.0,
                edge_limit_dist: 3.0,
                wall_offset: 0.0,
                arrival_deceleration: 50.0,
            },
            drive: DriveTuning {
                max_speed: 35.0,
                trigger_filter_strength: 1.0,
                turning_velocity_delta: 3.0,
                max_turbo_speed: 43.0,
                max_turbo_time: 2.4,
                max_turbo_cooldown_time: 3.2,
                throttle: 12.0,
                turbo_throttle_boost: 2.0,
                post_turbo_deceleration: 20.0,
                braking_deceleration: 15.0,
                max_reverse_speed: 15.0,
                post_reverse_throttle: 30.0,
                exit_to_run_deceleration: 35.0,
                turn_speed: 4.0,
                max_turn_speed: 8.0,
                engine_torque: 15000.0,
                wheel_friction: 5000.0,
                front_axle_length: 2.0,
                rear_axle_length: 2.0,
                long_axle_length: 5.0,
                front_wheel_radius: 1.6,
                rear_wheel_radius: 2.0,
                max_drift_turbo_speed: 38.0,
                min_reverse_brake_time: 0.05,
                max_angular_vel_z: 4.0,
                max_angular_vel_x: 0.35,
                max_angular_vel_y: 0.35,
                exit_to_run_min_speed: 24.0,
                exit_to_run_turn_speed: 115.0,
                exit_to_idle_deceleration: 35.0,
                exit_to_idle_min_speed: 0.0,
                exit_to_idle_turn_speed: 115.0,
                wheel_drift_factor: 0.05,
                drift_steering_factor: 0.4,
                skid_out_deceleration: 40.0,
                min_drift_time: 0.2,
                min_drift_turbo_speed: 20.0,
                drift_turbo_time: 2.0,
                drift_turbo_boost_factor: 2.0,
                suspension_length: 1.0,
                suspension_k: 100000.0,
                suspension_damp: 0.5,
                suspension_wt_scale: 0.85,
                angular_vel_damp_x: 0.75,
                angular_vel_damp_y: 0.75,
                body_half: Vec3::new(1.5,3.0,1.0),
                body_offset_z: 1.6,
                render_wheel_radius: 0.4,
                min_render_wheel_offset: -0.1,
                max_render_wheel_offset: 0.1,
                collision_forward_dot: -0.85,
                bullet_slow_speed_percent: 0.8,
                ..Default::default()
            },
            ..Default::default()
        };
        common_graph(&mut t);
        t
    }

    /// Bumblebee's graph for standing, moving, jumping, falling, weapon mode and the change of
    /// form both ways, copied from `bnxglobal.str#16` (`work\bb_machines.txt`, machines Common,
    /// DriveAdvance and Weapon), with the change-of-form clips' lengths and branch points.
    fn common_graph(t: &mut Tuning) {
        use crate::control::{Detector, Graph, Stage, StateDef, StickRange, Test, Transition};
        use crate::formats::anim::{Action as A, ActionEvent};
        let stick = |amp_min: f32, amp_max: f32| StickRange {
            amp_min,
            amp_max,
            angle_min: -std::f32::consts::TAU,
            angle_max: std::f32::consts::TAU,
            camera_relative: true,
        };
        let stage = |include: &[u32], exclude: &[u32], hit: Option<u32>, amp: (f32, f32)| Stage {
            time: 0.0,
            charge: 0.0,
            include: include.to_vec(),
            exclude: exclude.to_vec(),
            hit,
            left: stick(amp.0, amp.1),
        };
        let any = (-1.0, 100.0);
        let detect = |stages: Vec<Stage>, stays_on: f32| Test::Detect(Detector { stages, stays_on, hold_with_last_stage: false });
        let mut g = Graph::default();
        let mv = g.add_connection("connectionMove", detect(vec![stage(&[], &[], None, (0.18, 2.0))], 1e5));
        let release_move = g.add_connection("connectionReleaseMove", detect(vec![stage(&[], &[], None, (0.0, 0.15))], 1e5));
        let jump = g.add_connection("connectionJump", detect(vec![stage(&[], &[], Some(button::JUMP), any)], 0.1));
        g.connections[jump].on_ground = true;
        let fall = g.add_connection("connectionFall", Test::DistToGround { limit: 3.0, greater: true });
        g.connections[fall].in_air = true;
        g.connections[fall].max_z_velocity = -5.0;
        let on_ground = g.add_connection("connectionOnGround", detect(vec![], 1e5));
        g.connections[on_ground].on_ground = true;
        let move_on_ground = g.add_connection("connectionMoveOnGround", detect(vec![stage(&[], &[], None, (0.25, 2.0))], 0.05));
        g.connections[move_on_ground].on_ground = true;
        let delay = g.add_connection("connectionJumpSwitchDelay", Test::TimeInState(0.2));
        let exit_jump = g.add_connection("connectionListExitJump", Test::All(vec![delay, on_ground]));
        let exit_jump_move = g.add_connection("connectionListExitJumpMove", Test::All(vec![delay, move_on_ground]));
        let switch = g.add_connection("connectionEnvCanSwitchAvatar", Test::Environment(env::CAN_SWITCH_AVATAR));
        let vehicle = [button::VEHICLE_MODE];
        let weapon = [button::WEAPON_MODE];
        let drive = g.add_connection("connectionDrive", detect(vec![stage(&vehicle, &weapon, None, any)], 1e5));
        let drive_run = g.add_connection("connectionDriveRun", detect(vec![stage(&vehicle, &weapon, None, (0.11, 10.0))], 1e5));
        let drive_light = g.add_connection("connectionDriveLight", detect(vec![stage(&vehicle, &[], None, any)], 1e5));
        let release = g.add_connection("connectionReleaseDrive", detect(vec![stage(&[], &vehicle, None, (-1.0, 10.0))], 1e5));
        let release_run = g.add_connection("connectionReleaseDriveToRun", detect(vec![stage(&[], &vehicle, None, (0.11, 10.0))], 1e5));
        let jump_drive = g.add_connection("connectionJumpDrive", detect(vec![stage(&[button::TRANSFORM_JUMP], &vehicle, None, any)], 1e5));
        g.connections[jump_drive].on_ground = true;
        g.connections[jump_drive].can_adv_transform = true;
        let done = g.add_connection("connectionWaitForAnimDone", Test::None);
        (g.connections[done].branch_begin, g.connections[done].branch_end) = (100, 100);
        let second = g.add_connection("connectionWaitForSecondAnimBranch", Test::None);
        (g.connections[second].branch_begin, g.connections[second].branch_end) = (2, 2);
        let no_input = g.add_connection("connectionNoInput", detect(vec![stage(&[], &[], None, any)], 1e5));
        g.connections[no_input].branch_begin = 100;
        let weapon_mode = g.add_connection("connectionWeaponMode", detect(vec![stage(&weapon, &[], None, any)], 1e5));
        let release_weapon = g.add_connection("connectionReleaseWeapon", detect(vec![stage(&[], &weapon, None, any)], 1e5));
        let fire = [button::WEAPON_FIRE];
        let fire_held = g.add_connection("connectionWeaponModeAttack", detect(vec![stage(&fire, &[], None, any)], 1e5));
        let fire_let_go = g.add_connection("connectionReleaseWeaponModeAttack", detect(vec![stage(&[], &fire, None, any)], 1e5));
        let mut all = |name: &str, members: Vec<usize>| g.add_connection(name, Test::All(members));
        let list_drive = all("connectionListDrive", vec![drive, switch]);
        let list_drive_run = all("connectionListDriveRun", vec![drive_run, switch]);
        let list_release = all("connectionListDriveRelease", vec![release, switch]);
        let list_release_run = all("connectionListDriveReleaseRun", vec![release_run, switch]);
        let list_jump_drive = all("connectionListJumpDrive", vec![jump_drive, switch]);
        let light_done = all("connectionListDriveLightAndAnimDone", vec![drive_light, done]);
        let release_done = all("connectionListDriveReleaseAndAnimDone", vec![release, switch, done]);
        let release_run_done = all("connectionListDriveReleaseRunAndAnimDone", vec![release_run, switch, done]);
        let release_second = all("connectionListDriveReleaseAndSecondAnimBranch", vec![release, switch, second]);
        let release_run_second = all("connectionListDriveReleaseRunAndSecondAnimBranch", vec![release_run, switch, second]);
        let drive_done = all("connectionListDriveAndAnimDone", vec![drive, switch, done]);
        let drive_run_done = all("connectionListDriveRunAndAnimDone", vec![drive_run, switch, done]);
        let move_second = all("connectionListMoveAndSecondAnimBranch", vec![mv, second]);
        let idle_done = all("connectionListNoInputAndAnimDone", vec![no_input, done]);
        let fall_second = all("connectionListFallAndSecondAnimBranch", vec![fall, second]);
        let weapon_done = all("connectionListWeaponAndAnimDone", vec![weapon_mode, done]);

        let mut state = |name: &str, class: &str, anim: &str, motion: u32| {
            g.add_state(StateDef {
                name: name.into(),
                class: class.into(),
                anim: if anim.is_empty() { 0 } else { crc32(anim.as_bytes()) },
                motion_mode: motion,
                ..Default::default()
            })
        };
        let idle = state("stateIdle", "StateIdle", "", 0);
        let mov = state("stateMove", "StateMove", "", 0);
        let jmp = state("stateJump", "StateJump", "", 0);
        let jump_adv = state("stateJumpAdv", "StateJump", "", 0);
        let fal = state("stateFall", "StateFall", "", 0);
        let weapon_state = state("stateWeaponMode", "StateWeapon", "", 0x3c95_6e02);
        let weapon_jump = state("stateWeaponModeJump", "StateWeapon", "", JUMP_MODE);
        let weapon_fall = state("stateWeaponModeFall", "StateWeapon", "", JUMP_MODE);
        let car = state("stateDrive", "StateDrive", "", 0);
        let enter_a = state("stateDriveEnterA", "StateDrive", "Trans_R2V_A", 0);
        let enter_run_a = state("stateDriveEnterRunA", "StateDrive", "Trans_R2V_Run_A", 0);
        let enter_b = state("stateDriveEnterB", "StateDrive", "Trans_R2V_B", 0);
        let enter_run_b = state("stateDriveEnterRunB", "StateDrive", "Trans_R2V_Run_B", 0);
        let exit_a = state("stateDriveExitA", "StateBasic", "Trans_V2R_A", UNFOLD_MODE);
        let exit_run_a = state("stateDriveExitRunA", "StateBasic", "Trans_V2R_Run_A", UNFOLD_MODE);
        let exit_b = state("stateDriveExitB", "StateBasic", "Trans_V2R_B", UNFOLD_MODE);
        let exit_run_b = state("stateDriveExitRunB", "StateBasic", "Trans_V2R_Run_B", UNFOLD_MODE);
        let exit_jump_state = state("stateDriveExitJump", "StateBasic", "Trans_V2R_Jump", JUMP_MODE);

        let to = |connection: usize, output: usize, priority: i32| Transition { connection, output: Some(output), priority };
        let enter_drive = |a: usize, run_a: usize| vec![to(list_drive, a, 5), to(list_drive_run, run_a, 6)];
        g.states[idle].transitions = vec![to(mv, mov, 4), to(jump, jmp, 1), to(fall, fal, 0), to(weapon_mode, weapon_state, 2)];
        g.states[idle].transitions.extend(enter_drive(enter_a, enter_run_a));
        g.states[mov].transitions = vec![
            to(release_move, idle, 1),
            to(jump, jmp, 3),
            to(fall, fal, 2),
            to(weapon_mode, weapon_state, 0),
            to(list_drive, enter_a, 4),
            to(list_drive_run, enter_run_a, 5),
        ];
        for j in [jmp, jump_adv] {
            g.states[j].transitions = vec![
                to(exit_jump, idle, 1),
                to(exit_jump_move, mov, 2),
                to(list_drive, enter_a, 0),
                to(list_drive_run, enter_run_a, 1),
                to(weapon_mode, weapon_fall, 0),
            ];
        }
        g.states[fal].transitions = vec![
            to(move_on_ground, mov, 3),
            to(on_ground, idle, 2),
            to(list_drive, enter_a, 0),
            to(list_drive_run, enter_run_a, 1),
            to(weapon_mode, weapon_fall, 0),
        ];
        // The firing state, as the data has it: an overlay state on top of the weapon states.
        let firing = g.add_state(StateDef {
            name: "stateWeaponModeAttack".into(),
            class: "StateAttack".into(),
            overlay: true,
            ranged: true,
            fire_button: button::WEAPON_FIRE,
            ..Default::default()
        });
        g.states[firing].transitions = vec![Transition { connection: fire_let_go, output: None, priority: 0 }];
        g.states[weapon_state].transitions =
            vec![to(release_weapon, idle, 1), to(fire_held, firing, 3), to(fall, weapon_fall, 4), to(jump, weapon_jump, 5)];
        g.states[weapon_jump].transitions = vec![to(release_weapon, fal, 1), to(exit_jump, weapon_state, 2), to(fire_held, firing, 3)];
        g.states[weapon_fall].transitions = vec![to(release_weapon, fal, 1), to(on_ground, weapon_state, 2), to(fire_held, firing, 3)];
        g.states[car].transitions = vec![to(list_release, exit_a, 0), to(list_release_run, exit_run_a, 1), to(list_jump_drive, exit_jump_state, 2)];
        // The car's gun and the special, as the data has them: overlay states too
        // (machines DriveAttacksAdvance and GenericSpecial).
        let car_fire = [button::ATTACK_VEHICLE];
        let car_fire_held = g.add_connection("connectionWeaponModeVehicleAttack", detect(vec![stage(&car_fire, &[], None, any)], 1e5));
        let car_fire_let_go = g.add_connection("connectionReleaseVehicleWeapon", detect(vec![stage(&[], &car_fire, None, any)], 1e5));
        let special_pressed = g.add_connection("connectionAttackSpecial", detect(vec![stage(&[], &[], Some(button::ATTACK_SPECIAL), any)], 0.5));
        let overlay = |name: &str, class: &str, fire_button: u32| StateDef {
            name: name.into(),
            class: class.into(),
            overlay: true,
            ranged: class == "StateAttack",
            fire_button,
            robot_form: 5,
            ..Default::default()
        };
        let vehicle_fire = g.add_state(overlay("stateWeaponModeVehicleAttack", "StateAttack", button::ATTACK_VEHICLE));
        g.states[vehicle_fire].transitions = vec![Transition { connection: car_fire_let_go, output: None, priority: 0 }];
        g.states[car].transitions.push(to(car_fire_held, vehicle_fire, 5));
        let special = g.add_state(overlay("stateAttackSpecial", "StateAttackSpecial", 0));
        for under in [idle, mov, weapon_state, firing, car, vehicle_fire] {
            g.states[under].transitions.push(to(special_pressed, special, 10));
        }
        for (a, b) in [(enter_a, enter_b), (enter_run_a, enter_run_b)] {
            g.states[a].transitions = vec![
                to(done, b, 0),
                to(light_done, b, 1),
                to(release_done, exit_b, 2),
                to(release_run_done, exit_run_b, 3),
                to(list_jump_drive, exit_jump_state, 4),
            ];
        }
        for b in [enter_b, enter_run_b] {
            g.states[b].transitions = vec![
                to(done, car, 0),
                to(light_done, car, 1),
                to(release_second, exit_a, 2),
                to(release_run_second, exit_run_a, 3),
                to(list_jump_drive, exit_jump_state, 4),
            ];
        }
        g.states[exit_a].transitions = vec![to(drive_done, enter_b, 1), to(drive_run_done, enter_run_b, 2), to(done, exit_b, 0)];
        g.states[exit_run_a].transitions = vec![to(drive_done, enter_b, 1), to(drive_run_done, enter_run_b, 2), to(done, exit_run_b, 0)];
        for b in [exit_b, exit_run_b] {
            g.states[b].transitions = vec![
                to(move_second, mov, 2),
                to(drive_done, enter_a, 1),
                to(drive_run_done, enter_run_a, 1),
                to(idle_done, idle, 0),
                to(fall_second, fal, 3),
                to(weapon_done, weapon_state, 4),
            ];
        }
        g.states[exit_jump_state].transitions = vec![to(idle_done, jump_adv, 0), to(weapon_done, weapon_fall, 1)];
        t.control = g;

        // The clips: length in ticks, root travel forward, branch point. [data]
        let clips = [
            ("DriveSet", "Trans_R2V_A", 26.0, 3.5, None),
            ("DriveSet", "Trans_R2V_B", 72.0, 0.06, Some(40.0)),
            ("DriveSet", "Trans_R2V_Run_A", 34.0, 11.17, None),
            ("DriveSet", "Trans_R2V_Run_B", 76.0, 2.03, Some(32.0)),
            ("Vehicle2RobotSet", "Trans_V2R_A", 10.0, 0.0, None),
            ("Vehicle2RobotSet", "Trans_V2R_B", 50.0, 1.63, None),
            ("Vehicle2RobotSet", "Trans_V2R_Run_A", 16.0, 6.39, None),
            ("Vehicle2RobotSet", "Trans_V2R_Run_B", 44.0, 22.08, None),
            ("Vehicle2RobotSet", "Trans_V2R_Jump", 42.0, 0.0, None),
        ];
        for (set, id, ticks, travel, branch) in clips {
            let n = ticks as usize;
            t.actions.clips.insert(
                crc32(id.as_bytes()),
                ActionClip {
                    set: set.into(),
                    id: id.into(),
                    ticks,
                    speed: 1.0,
                    crossfade: 0.0,
                    looping: false,
                    path: (0..=n).map(|tick| Vec3::Y * (travel * tick as f32 / ticks)).collect(),
                    turn: Vec::new(),
                    events: branch.map(|time| vec![ActionEvent { time, action: A::BranchPoint }]).unwrap_or_default(),
                    attack: None,
                    bodies: Vec::new(),
                    mounts: Vec::new(),
                    weapon_events: Vec::new(),
                    blasts: Vec::new(),
                },
            );
        }
    }

    fn every_second_tick(crossfade: f32, samples: &[f32]) -> RootCurve {
        let mut forward = Vec::new();
        for pair in samples.windows(2) {
            forward.push(pair[0] - samples[0]);
            forward.push((pair[0] + pair[1]) / 2.0 - samples[0]);
        }
        forward.push(samples[samples.len() - 1] - samples[0]);
        RootCurve::new(crossfade, forward)
    }

    /// The game's own update length on the PC it was recorded on (`notes\live-trace.md`). Its
    /// updates ran from 31 to 33 ms, so the replays below agree to about 0.3, not exactly.
    const GAME_DT: f32 = 0.032;

    /// Steps at the game's rate and returns the ground speed after each update.
    fn speeds(s: &mut State, input: Input, t: &Tuning, updates: usize) -> Vec<f32> {
        let arena = Arena::default();
        (0..updates)
            .map(|_| {
                step(s, &input, t, &arena, GAME_DT);
                s.speed
            })
            .collect()
    }

    fn assert_close(got: &[f32], recorded: &[f32], within: f32) {
        assert_eq!(got.len(), recorded.len());
        for (i, (g, r)) in got.iter().zip(recorded).enumerate() {
            assert!((g - r).abs() <= within, "update {i}: {g:.2} here, {r:.2} in the game\n{got:.2?}\n{recorded:?}");
        }
    }

    const FORWARD: Input =
        Input {
            stick: Vec2::Y,
            steer: 0.0,
            pad_stick: Vec2::Y,
            look: Vec2::ZERO,
            jump: false,
            jump_held: false,
            vehicle: 0.0,
            brake: 0.0,
            turbo: false,
            drift: false,
            aim: None,
            buttons_down: 0,
            buttons_hit: 0,
        };

    #[test]
    fn leaving_the_car_on_the_ground_slows_to_a_run_or_a_stop_as_recorded() {
        let t = tuning();
        let gas = Input { vehicle: 1.0, ..Default::default() };
        // Stick held (run 1, t 433.8): 1.15 off an update down to the run's speed, held
        // there for the rest of the 1.03 s, then the run itself at 24.
        let mut s = State::new(&t);
        run(&mut s, gas, &t, 4.0);
        let before = s.speed;
        let got = speeds(&mut s, FORWARD, &t, 40);
        assert_eq!(s.mode, Mode::Move);
        // Run 1 (t 435.78, 442.14): 1.14 off every update from the first, until it is under
        // the run's 24; there it holds (23.7 in the game, from where it started), and the run
        // takes over when the two clips' 60 ticks are out, 32 updates on.
        for i in 1..10 {
            assert!((got[i - 1] - got[i] - 1.12).abs() < 0.03, "{before:.2} then {got:.2?}");
        }
        assert!((before - got[0] - 1.12).abs() < 0.03, "{before:.2} then {got:.2?}");
        assert!((22.8..24.0).contains(&got[20]) && got[30] == got[20], "{got:.2?}");
        assert!((got[33] - 24.0).abs() < 0.01 && !s.unfolding(), "{got:.2?}");
        assert!((got[39] - 24.0).abs() < 0.01 && s.pace() == Pace::Run, "{got:.2?}");
        // Stick centred: on down to nothing, and standing after.
        let mut s = State::new(&t);
        run(&mut s, gas, &t, 4.0);
        let got = speeds(&mut s, Input::default(), &t, 40);
        assert!(got[20] < got[10] - 10.0 && got[39] == 0.0 && s.pace() == Pace::Idle, "{got:.2?}");
    }

    /// Ground that climbs along y.
    struct Slope(f32);

    impl World for Slope {
        fn wheel_ray(&self,from:Vec3,to:Vec3)->Option<crate::vehicle::GroundHit> {
            let n=Vec3::new(0.0,-self.0,1.0).normalize();
            let fraction=-n.dot(from)/n.dot(to-from);
            (fraction>=0.0 && fraction<=1.0).then_some(crate::vehicle::GroundHit {
                point:from.lerp(to,fraction),normal:n,surface:melee::surface::DEFAULT,object_mask:0,
            })
        }
        fn floor(&self, _: f32, y: f32, _: f32) -> f32 {
            (y * self.0).max(0.0)
        }

        fn push_out(&self, _: &mut Vec3, _: f32, _: f32) -> bool {
            false
        }
    }

    #[test]
    fn arena_ramps_support_fast_driving_launching_and_landing() {
        let t = tuning();
        let arena = Arena { ramps: vec![crate::terrain::Ramp {
            min: Vec2::new(-20.0, 0.0), max: Vec2::new(20.0, 50.0),
            base: 0.0, height: 0.0, slope: Vec2::new(0.0, 0.28),
        }], ..Default::default() };
        let gas = Input { vehicle: 1.0, ..Default::default() };
        let mut s = State::new(&t);
        s.pos.y = -20.0;
        let (mut pitched, mut launched, mut landed) = (false, false, false);
        for _ in 0..700 {
            step(&mut s, &gas, &t, &arena, GAME_UPDATE);
            if (10.0..45.0).contains(&s.pos.y) {
                // Suspension permits root movement; the old exact plane snap is gone.
                assert!((s.pos.z - 0.28 * s.pos.y).abs() < t.drive.suspension_length, "{:?}", s.pos);
                assert!(!s.car_airborne());
                pitched |= s.pitch > 0.15;
            }
            if s.car_airborne() && s.pos.y > 50.0 {
                launched |= s.pos.z >= 14.0-t.drive.suspension_length && s.vel.z > 0.0;
            }
            landed |= launched && !s.car_airborne() && s.pos.z.abs()<t.drive.suspension_length;
        }
        assert!(pitched && launched && landed, "pitch {pitched}, launch {launched}, land {landed}");
    }

    #[test]
    fn arena_hill_has_continuous_support_up_and_down_in_both_forms() {
        let t = tuning();
        let arena = Arena { ramps: vec![
            crate::terrain::Ramp { min: Vec2::new(-20.0, 0.0), max: Vec2::new(20.0, 40.0),
                base: 0.0, height: 0.0, slope: Vec2::new(0.0, 0.2) },
            crate::terrain::Ramp { min: Vec2::new(-20.0, 40.0), max: Vec2::new(20.0, 80.0),
                base: 0.0, height: 8.0, slope: Vec2::new(0.0, -0.2) },
        ], ..Default::default() };
        for vehicle in [0.0, 1.0] {
            let mut s = State::new(&t);
            s.pos.y = -10.0;
            let input = Input { vehicle, stick: Vec2::Y, ..Default::default() };
            let mut crossed_crest = false;
            for _ in 0..500 {
                step(&mut s, &input, &t, &arena, GAME_UPDATE);
                if (2.0..78.0).contains(&s.pos.y) {
                    let expected = if s.pos.y < 40.0 { s.pos.y * 0.2 } else { (80.0 - s.pos.y) * 0.2 };
                    if vehicle==0.0 {
                        assert!((s.pos.z-expected).abs()<1e-3 && s.touching);
                    } else {
                        // A sprung car may unload at the crest and carry upward velocity.
                        assert!(s.pos.z>expected-t.drive.suspension_length && s.pos.z<expected+5.0,
                            "vehicle {vehicle}: {:?}",s.pos);
                    }
                    crossed_crest |= s.pos.y > 40.0;
                }
            }
            assert!(crossed_crest && s.pos.y > 80.0 && s.pos.z.abs()<0.2);
        }
    }

    #[test]
    fn ramp_sides_block_entry_but_the_low_edge_is_walkable() {
        let arena = Arena { ramps: vec![crate::terrain::Ramp {
            min: Vec2::ZERO, max: Vec2::new(20.0, 40.0),
            base: 0.0, height: 0.0, slope: Vec2::new(0.0, 0.25),
        }], ..Default::default() };
        let mut high = Vec3::new(10.0, 40.5, 0.0);
        assert!(arena.push_out(&mut high, 1.0, 3.0));
        assert!((high.y - 41.0).abs() < 1e-5);
        let mut low = Vec3::new(10.0, -0.5, 0.0);
        assert!(!arena.push_out(&mut low, 1.0, 3.0));
        assert!(arena.follow_floor(high, Vec2::new(10.0, 35.0)).is_none());
        assert!(arena.follow_floor(Vec3::new(10.0, -1.0, 0.0), Vec2::new(10.0, 5.0)).is_some());
        let landed = arena.drop_onto(Vec3::new(10.0, 20.0, 8.0), 2.0, 1.0).unwrap();
        assert_eq!(landed.z, 5.0);
    }

    #[test]
    fn banked_arena_surface_rolls_the_car() {
        let t = tuning();
        let arena = Arena { ramps: vec![crate::terrain::Ramp {
            min: Vec2::new(-20.0, -50.0), max: Vec2::new(20.0, 500.0),
            base: 0.0, height: 0.0, slope: Vec2::new(0.15, 0.0),
        }], ..Default::default() };
        let mut s = State::new(&t);
        s.pos.z = 3.0;
        for _ in 0..120 {
            step(&mut s, &Input { vehicle: 1.0, ..Default::default() }, &t, &arena, DT);
        }
        assert!((s.roll + 0.15_f32.atan()).abs() < 0.02, "roll {}", s.roll);
        assert!(s.pitch.abs() < 0.01 && !s.car_airborne());
    }

    #[test]
    fn car_presentation_moves_on_every_render_frame_at_steady_speed() {
        let t=tuning();let arena=Arena::default();
        // Include irregular frames and a hitch spanning multiple physics updates.
        // A direct display of State.pos pauses, then jumps, in every sequence here.
        for frames in [vec![1.0/60.0],vec![1.0/120.0],vec![1.0/144.0],
            vec![0.007,0.021,0.012,0.045,0.009]] {
            let mut s=State::new(&t);
            let input=Input {vehicle:1.0,..Default::default()};
            for _ in 0..200 {step(&mut s,&input,&t,&arena,GAME_UPDATE);}
            let mut previous=s.car_presentation();
            let mut raw_previous=s.pos;
            let mut raw_holds=0;
            for frame in 0..300 {
                let dt=frames[frame%frames.len()];
                step(&mut s,&input,&t,&arena,dt);
                let pose=s.car_presentation();
                let expected=s.vel.y*dt;
                let moved=pose.pos.y-previous.pos.y;
                assert!((moved-expected).abs()<0.002,
                    "dt {dt}, display {moved}, expected {expected}");
                assert!((pose.wheels[0].render_spin-previous.wheels[0].render_spin).abs()>0.01,
                    "tyre paused while body moved, dt {dt}");
                assert!((s.pos-pose.pos).length()<=s.vel.length()*GAME_UPDATE+0.01,
                    "presentation must stay within one completed physics interval");
                raw_holds+=usize::from(s.pos==raw_previous);
                raw_previous=s.pos;previous=pose;
            }
            assert!(raw_holds>50,"test must reproduce the old held-frame bug");
        }
    }

    #[test]
    fn car_presentation_keeps_turns_continuous_and_discards_old_drive_history() {
        let t=tuning();let arena=Arena::default();let mut s=State::new(&t);
        let mut pose=s.car_presentation();
        let mut crossed_heading_wrap=false;
        let mut last_yaw=s.yaw;
        for i in 0..2400 {
            let input=Input {vehicle:1.0,steer:if i<400 {0.0} else if i<1500 {1.0} else {-1.0},
                ..Default::default()};
            step(&mut s,&input,&t,&arena,1.0/120.0);
            let next=s.car_presentation();
            if i>400 {
                assert!(pose.rotation.angle_between(next.rotation)<0.05,"display heading jumped");
                assert!((next.pos-pose.pos).length()<0.4,"display root jumped");
                crossed_heading_wrap|=(s.yaw-last_yaw).abs()>6.0;
            }
            last_yaw=s.yaw;pose=next;
        }
        assert!(crossed_heading_wrap,"must exercise the quaternion heading seam");
        set_mode(&mut s,&Input::default(),&t,Mode::Stand);
        assert_eq!(s.car_presentation().pos,s.pos);
        s.pos=Vec3::new(1000.0,2000.0,0.0);
        set_mode(&mut s,&Input::default(),&t,Mode::Drive);
        assert!(s.car.previous_presentation.is_none());
        assert_eq!(s.car_presentation().pos,s.pos,"re-entry must not reuse the old car position");
        let reset=State::new(&t);
        assert_eq!(reset.car_presentation().pos,Vec3::ZERO);
    }

    #[test]
    fn sharp_turn_suspension_stays_within_render_travel_in_both_directions() {
        let t=tuning();let arena=Arena::default();
        // Installed model's tyre centres [data], rounded to millimetres. These differ
        // from the physics mounts; testing only mounts misses the visible penetration.
        let centres=[Vec3::new(-0.980,2.200,0.444),Vec3::new(0.964,2.200,0.444),
            Vec3::new(-1.010,-1.156,0.444),Vec3::new(0.994,-1.155,0.445)];
        for dt in [GAME_UPDATE,DT,1.0/120.0] {
            for direction in [-1.0,1.0] {
                let mut s=State::new(&t);
                for i in 0..(12.0/dt) as usize {
                    let time=i as f32*dt;
                    let steer=if time<3.0 {0.0} else if time<8.0 {direction} else {-direction};
                    step(&mut s,&Input {vehicle:1.0,steer,..Default::default()},&t,&arena,dt);
                    if s.mode!=Mode::Drive || time<2.0 {continue;}
                    assert!(s.roll.abs()<0.2,"dt {dt}, t {time}, roll {}",s.roll);
                    // Check both the authoritative physics and the interpolated model.
                    for pose in [s.car_snapshot(),s.car_presentation()] {
                        let turn=pose.rotation;let up=turn*Vec3::Z;
                        for (index,(w,centre)) in pose.wheels.iter().zip(centres).enumerate() {
                            let point=pose.pos+turn*centre;
                            let offset=crate::vehicle::Chassis::render_offset(w,point.z,up.z,&t.drive);
                            let wheel_turn=turn*Quat::from_rotation_z(if index<2 {-3.0*pose.steer_angle} else {0.0});
                            let bottom=point.z-up.z*offset-t.drive.render_wheel_radius*
                                (1.0-(wheel_turn*Vec3::X).z.powi(2)).sqrt();
                            // Installed .4 radius and +/- .1 travel. The original capture's
                            // flat-contact nominal clearance p05 is -.0417m; shallow overlap
                            // exists in the original. Reject the old .24m penetration. [trace]
                            assert!(bottom> -0.045,"dt {dt}, t {time}, tyre {index} bottom {bottom}, roll {}",s.roll);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_car_leans_to_a_slope_and_flies_off_a_ledge() {
        let t = tuning();
        let gas = Input { vehicle: 1.0, ..Default::default() };
        let mut s = State::new(&t);
        s.pos.y = -20.0;
        for _ in 0..180 {
            step(&mut s, &gas, &t, &Slope(0.2), DT);
        }
        // Nose up by the slope's own angle, and no faster than the body may tip.
        assert!(s.pos.z > 5.0 && (s.pitch - 0.2f32.atan()).abs() < 0.02 && s.roll.abs() < 0.01, "{} {}", s.pitch, s.roll);
        // Off the edge of a box: in the air, then down on the ground again and level.
        let arena = Arena { boxes: vec![Box3 { min: Vec3::new(-50.0, -50.0, 0.0), max: Vec3::new(50.0, 60.0, 8.0) }], ..Default::default() };
        let mut s = State::new(&t);
        s.pos.z = 8.0;
        let mut flew = 0;
        for _ in 0..400 {
            step(&mut s, &gas, &t, &arena, DT);
            flew += s.car_airborne() as usize;
        }
        // Independent wheels lose/regain support before the old centre-only touchdown.
        assert!(flew>0, "never left wheel contact");
        assert!(s.mode == Mode::Drive && s.pos.z.abs()<0.2 && s.pos.y > 60.0 && s.speed > 30.0, "{:?}", s.pos);
    }

    #[test]
    fn the_car_tells_its_tyres_what_to_sound_like() {
        use crate::sound::tyre_sounds;
        let t = tuning();
        let gas = Input { vehicle: 1.0, ..Default::default() };
        // Off a ledge 8 high: rolling on the way, silent in the air, one thump coming down.
        let arena = Arena { boxes: vec![Box3 { min: Vec3::new(-50.0, -50.0, 0.0), max: Vec3::new(50.0, 60.0, 8.0) }], ..Default::default() };
        let mut s = State::new(&t);
        s.pos.z = 8.0;
        let (mut rolled, mut silent, mut thumps) = (0, 0, Vec::new());
        for _ in 0..400 {
            step(&mut s, &gas, &t, &arena, DT);
            for heard in s.tyre_inputs.iter().map(tyre_sounds) {
                assert_eq!(heard.skid, None, "nothing here makes them skid");
                rolled += heard.road.is_some() as usize;
                silent += heard.road.is_none() as usize;
                thumps.extend(heard.suspension);
            }
        }
        assert!(rolled > 100 && silent > 15, "{rolled} rolling, {silent} silent");
        // Thumps now use actual rear shortening; unloading/reloading can cause more
        // than the old invented single touchdown thump. Every cue obeys the code gate.
        assert!(!thumps.is_empty() && thumps.iter().all(|force|*force>0.5),"{thumps:?}");
        // The slide and the brake make them skid; plain driving does not.
        // (A step shorter than the game's update may hold no update: the last one that did.)
        let mut heard = |input: Input, seconds: f32| {
            let mut last = None;
            for _ in 0..(seconds / DT).round() as usize {
                step(&mut s, &input, &t, &arena, DT);
                last = s.tyre_inputs.last().map(tyre_sounds).or(last);
            }
            last.unwrap()
        };
        assert!(heard(Input { drift: true, ..gas }, 0.3).skid.is_some());
        let rolling = heard(gas, 1.0);
        assert!(rolling.skid.is_none() && rolling.road.is_some());
        assert!(heard(Input { brake: 1.0, ..gas }, 0.2).skid.is_some());
    }

    #[test]
    fn the_slide_slows_him_swings_the_tail_but_forward_speed_alone_earns_no_boost() {
        let t = tuning();
        let gas = Input { vehicle: 1.0, ..Default::default() };
        let turn = Input { steer: 1.0, ..gas };
        let mut gripping = State::new(&t);
        run(&mut gripping, gas, &t, 4.0);
        let mut sliding = gripping.clone();
        run(&mut gripping, turn, &t, 0.3);
        run(&mut sliding, Input { drift: true, ..turn }, &t, 0.3);
        assert!(sliding.sliding());
        // Slowed along his own motion at 40 a second, less what the spun-up wheels still
        // give through the little forward grip they keep (0.4 of about 81).
        let lost = Vec2::new(gripping.vel.x, gripping.vel.y).length() - Vec2::new(sliding.vel.x, sliding.vel.y).length();
        assert!((2.0..6.0).contains(&lost), "lost {lost}");
        // All four tyres lose their sideways grip, so he carries on much the way he was
        // going: the path bends far less than with grip.
        let bend = |s: &State| Vec2::new(s.vel.x, s.vel.y).normalize().x.abs();
        assert!(bend(&sliding) < bend(&gripping) * 0.5, "{} against {}", bend(&sliding), bend(&gripping));
        // [game: 00733e10] This short turn never produces enough sideways velocity to
        // charge. The old guessed rule wrongly awarded a boost for forward speed.
        assert!(!sliding.car.drift_turbo.active());
        run(&mut sliding, gas, &t, 1.5);
        assert!(sliding.speed < 39.0, "{}", sliding.speed);
    }

    #[test]
    fn leaving_the_car_keeps_standard_turbo_and_its_cooldown_ticking() {
        let t = tuning();
        let gas = Input { vehicle: 1.0, ..Default::default() };
        let mut s = State::new(&t);
        run(&mut s, gas, &t, 2.0);
        run(&mut s, Input { turbo: true, ..gas }, &t, 0.2);
        assert!(s.turbo_left > 2.0);
        assert!(s.turbo_fx_on);
        let left = s.turbo_left;
        step(&mut s, &Input::default(), &t, &Arena::default(), GAME_DT);
        assert_eq!(s.mode, Mode::Unfold);
        assert!(!s.turbo_fx_on);
        assert!((s.turbo_left - (left - GAME_DT)).abs() < 1e-5);
        run(&mut s, Input::default(), &t, 2.5);
        assert_eq!(s.turbo_left, 0.0);
        assert!(s.turbo_cooldown > 2.5, "{}", s.turbo_cooldown);
    }

    #[test]
    fn the_drive_update_uses_lateral_charge_and_reports_drift_turbo_start() {
        let t = tuning();
        let mut s = State::new(&t);
        s.mode = Mode::Drive;
        let gas = Input { vehicle: 1.0, drift: true, ..Default::default() };
        // Hold a qualifying sideways velocity, like the external physics body sampled
        // by the original ability. A plain forward slide is checked above separately.
        // The drift steps after the car's update, on the velocity it wrote back (`drift_step`).
        for _ in 0..7 {
            s.vel = Vec3::new(25.0, 0.0, 0.0);
            drive_update(&mut s, &gas, &t, GAME_DT);
            s.vel = Vec3::new(25.0, 0.0, 0.0);
            drift_step(&mut s, &gas, &t, GAME_DT);
        }
        assert!(!s.turbo_started);
        let release = Input { drift: false, ..gas };
        drive_update(&mut s, &release, &t, GAME_DT);
        drift_step(&mut s, &release, &t, GAME_DT);
        assert!(s.car.drift_turbo.active());
        assert!(s.turbo_started && s.turbo_fx_on);
        // The car's next update sees the drift running.
        drive_update(&mut s, &release, &t, GAME_DT);
        assert!(crate::fxstate::kept(&s, &t, false, false).contains(&crate::fxstate::TURBO));
        assert!(s.gearbox_inputs.last().unwrap().turbo);
    }

    /// FlyMode's and HoverMode's messages (`fxstate::kept`): the exhaust only once the take-off
    /// clip has sent it (the latch), the brake in its place while it runs, the turbo while the
    /// fly turbo runs; the hover thrusters while hovering. [game 007325e0, 00733290, 00735bb0]
    #[test]
    fn the_flying_forms_keep_their_exhaust_brake_turbo_and_hover_effects() {
        use crate::fxstate::{kept, BRAKE, EXHAUST, HOVER, TURBO};
        let t = tuning();
        let mut s = State::new(&t);
        s.mode = Mode::Fly;
        assert!(!kept(&s, &t, false, false).contains(&EXHAUST), "not before the clip sends it");
        assert!(kept(&s, &t, false, true).contains(&EXHAUST));
        s.fly_timers.brake_left = 0.2;
        let braking = kept(&s, &t, false, true);
        assert!(braking.contains(&BRAKE) && !braking.contains(&EXHAUST));
        s.fly_timers.brake_left = 0.0;
        s.fly_timers.turbo_left = 1.0;
        assert!(kept(&s, &t, false, true).contains(&TURBO));
        s.mode = Mode::Hover;
        let hover = kept(&s, &t, false, true);
        assert!(hover.contains(&HOVER) && !hover.contains(&EXHAUST) && !hover.contains(&TURBO));
    }

    #[test]
    fn drive_filter_feeds_the_gearbox_but_does_not_reduce_the_tyre_sound_cap() {
        let mut t = tuning();
        t.drive.trigger_filter_strength = 10.0;
        let mut s = State::new(&t);
        s.mode = Mode::Drive;
        drive_update(&mut s, &Input { vehicle: 0.5, ..Default::default() }, &t, GAME_DT);
        assert!((s.gearbox_inputs.last().unwrap().trigger - 0.16).abs() < 1e-5);
        assert_eq!(s.tyre_inputs.last().unwrap().cap, t.drive.max_speed);
    }

    #[test]
    fn drive_entry_starts_with_full_trigger_then_filters_partial_throttle() {
        let t = tuning();
        let mut s = State::new(&t);
        let gas = Input { vehicle: 1.0, ..Default::default() };
        run(&mut s, gas, &t, 2.0);
        assert_eq!(s.car.filtered_trigger, 1.0);
        drive_update(&mut s, &Input { vehicle: 0.5, ..gas }, &t, GAME_DT);
        assert!((s.car.filtered_trigger - 0.984).abs() < 1e-6);
    }

    #[test]
    fn letting_the_trigger_go_with_jump_held_leaps_out_of_the_car() {
        let t = tuning();
        let arena = Arena::default();
        let mut s = State::new(&t);
        let gas = Input { vehicle: 1.0, ..Default::default() };
        run(&mut s, gas, &t, 3.0);
        step(&mut s, &Input { turbo: true, jump_held: true, ..gas }, &t, &arena, DT);
        run(&mut s, Input { jump_held: true, ..gas }, &t, 1.5);
        let before = Vec2::new(s.vel.x, s.vel.y).length();
        step(&mut s, &Input { jump_held: true, ..Default::default() }, &t, &arena, DT);
        assert_eq!(s.mode, Mode::Jump(JumpType::Long));
        // t 651.96 in run 1: 46.6 along the ground and 30.2 up one update after leaving.
        assert!((Vec2::new(s.vel.x, s.vel.y).length() - before).abs() < 0.5 && before > 44.0, "{before}");
        assert!((s.vel.z - 31.3).abs() < 0.7, "{}", s.vel.z);
    }

    fn half_stick() -> Input {
        Input { stick: Vec2::Y * 0.5, ..Default::default() }
    }

    // The four tests below replay sequences from the recorded game, one value per update.

    #[test]
    fn run_start_matches_the_recording() {
        let t = tuning();
        // The stick crosses the walk zone for one update (t 120.36 in run 1).
        let mut s = State::new(&t);
        let mut got = speeds(&mut s, half_stick(), &t, 1);
        got.extend(speeds(&mut s, FORWARD, &t, 9));
        assert_close(&got, &[0.72, 4.32, 7.73, 10.96, 14.03, 16.93, 19.65, 22.22, 24.0, 24.0], 0.3);
        // Straight to full stick (t 132.72).
        let mut s = State::new(&t);
        assert_close(&speeds(&mut s, FORWARD, &t, 4), &[3.1, 6.2, 9.3, 12.5], 0.3);
    }

    #[test]
    fn stop_from_a_run_matches_the_recording() {
        let t = tuning();
        let mut s = State::new(&t);
        run(&mut s, FORWARD, &t, 1.0);
        // Stick let go at once (t 173.8): the run-to-idle clip's own root motion.
        let got = speeds(&mut s, Input::default(), &t, 12);
        assert_close(&got, &[21.1, 20.1, 19.8, 18.5, 14.7, 10.2, 6.2, 3.3, 1.3, 0.1, 0.0, 0.0], 0.6);
        assert_eq!(s.pace(), Pace::Idle);
        // Stick passing through the walk zone for two updates first (t 130.48): no
        // run-to-idle, the idle clip fades in over the run fading to a walk.
        let mut s = State::new(&t);
        run(&mut s, FORWARD, &t, 1.0);
        let mut got = speeds(&mut s, half_stick(), &t, 2);
        got.extend(speeds(&mut s, Input::default(), &t, 7));
        assert_close(&got, &[21.62, 19.22, 14.08, 9.72, 6.12, 3.32, 1.31, 0.13, 0.0], 0.3);
    }

    #[test]
    fn stop_from_a_walk_matches_the_recording() {
        let t = tuning();
        let mut s = State::new(&t);
        // t 171.06: four updates into a walk, then the stick is let go.
        let mut got = speeds(&mut s, half_stick(), &t, 4);
        got.extend(speeds(&mut s, Input::default(), &t, 7));
        assert_close(&got, &[0.7, 1.4, 2.1, 2.8, 2.9, 2.8, 2.5, 1.9, 1.0, 0.1, 0.0], 0.3);
    }

    #[test]
    fn landing_into_a_run_matches_the_recording() {
        let t = tuning();
        let arena = Arena::default();
        let mut s = State::new(&t);
        run(&mut s, FORWARD, &t, 1.0);
        step(&mut s, &Input { jump: true, ..FORWARD }, &t, &arena, GAME_DT);
        while s.mode != Mode::Move {
            step(&mut s, &FORWARD, &t, &arena, GAME_DT);
        }
        // t 125.6 and a dozen like it: the landing clip's root motion from the update the
        // jump mode ends (two after touching down), then the run at once.
        let mut got = vec![s.speed];
        got.extend(speeds(&mut s, FORWARD, &t, 11));
        assert_eq!(s.pace(), Pace::Run);
        assert_close(&got[..6], &[0.3, 9.5, 12.9, 12.6, 12.4, 12.4], 0.6);
        assert_eq!(got[11], 24.0);
    }

    #[test]
    fn running_jump_gains_speed_in_the_air_as_recorded() {
        let t = tuning();
        let arena = Arena::default();
        let mut s = State::new(&t);
        run(&mut s, FORWARD, &t, 1.0);
        step(&mut s, &Input { jump: true, ..FORWARD }, &t, &arena, GAME_DT);
        // t 125.77: off the ground at exactly the run speed (24.00, seen mid-update), and the
        // jump's air control already in by the end of that update; faster every update after.
        let mut got = vec![Vec2::new(s.vel.x, s.vel.y).length()];
        let mut updates = 1;
        while s.mode != Mode::Move {
            step(&mut s, &FORWARD, &t, &arena, GAME_DT);
            got.push(Vec2::new(s.vel.x, s.vel.y).length());
            updates += 1;
        }
        assert_close(&got[..9], &[24.52, 25.19, 25.88, 26.50, 26.95, 27.26, 27.54, 27.81, 28.08], 0.3);
        // In the game the jump mode ran for 48 updates (t 125.774 to 127.328), the last two
        // on the ground, and came down at 32.9. Full stick here gives 33.7; the recorded gains
        // late in the jump fit a stick held a little short of full, which is the player's
        // hand and not a rule.
        assert!((47..=50).contains(&updates), "{updates} updates in the jump mode");
        assert!((got[got.len() - 3] - 32.9).abs() < 1.0, "{:.2?}", &got[got.len() - 5..]);
    }

    #[test]
    fn standing_jump_creeps_forward_and_then_slows_as_recorded() {
        let t = tuning();
        let arena = Arena::default();
        let mut s = State::new(&t);
        step(&mut s, &Input { jump: true, ..Default::default() }, &t, &arena, GAME_DT);
        // t 370.6: from rest the take-off clip carries him forward to about 2.4, then the
        // speed falls by about a tenth each update.
        let got = speeds_in_air(&mut s, &t, 16);
        let peak = got.iter().copied().fold(0.0, f32::max);
        assert!((peak - 2.45).abs() < 0.5, "peak {peak:.2}\n{got:.2?}");
        assert!((got[15] - 1.2).abs() < 0.4, "{got:.2?}");
    }

    fn speeds_in_air(s: &mut State, t: &Tuning, updates: usize) -> Vec<f32> {
        let arena = Arena::default();
        (0..updates)
            .map(|_| {
                step(s, &Input::default(), t, &arena, GAME_DT);
                Vec2::new(s.vel.x, s.vel.y).length()
            })
            .collect()
    }

    fn run(s: &mut State, input: Input, t: &Tuning, seconds: f32) {
        let arena = Arena::default();
        for _ in 0..(seconds / DT).round() as usize {
            step(s, &input, t, &arena, DT);
        }
    }

    /// A `Death` clip like his: 100 ticks, the root going 10.8 back.
    fn with_death_clip(t: &mut Tuning) {
        let path = (0..=100).map(|tick| Vec3::new(0.0, -10.8 * tick as f32 / 100.0, 0.0)).collect();
        let clip = ActionClip { set: "DeathSet".into(), id: "Death".into(), ticks: 100.0, speed: 1.0, path, ..Default::default() };
        t.actions.clips.insert(crc32(b"Death"), clip);
    }

    fn installed_animation_tuning() -> Tuning {
        let dir = std::path::Path::new("C:/Games2");
        let mut t = crate::tuning::load(dir, "Bumblebee").unwrap();
        let data = crate::character::load(dir, "bumblebee").unwrap();
        t.actions = crate::tuning::Actions::from_animations(&data.animations, &data.robot, &mut t.missing);
        t.locomotion = crate::tuning::Locomotion::from_animations(&data.animations, &mut t.missing);
        t.weapon_set = crate::tuning::RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
        t.rule_sets = crate::tuning::RuleSet::all(&data.animations);
        t
    }

    #[test]
    fn installed_partial_hit_plays_above_movement_and_does_not_interrupt_fire() {
        let t = installed_animation_tuning();
        let mut s = State::new(&t);
        let input = Input { stick: Vec2::Y, aim: Some(Vec2::Y), buttons_down: 1 << button::WEAPON_FIRE, ..Input::default() };
        run(&mut s, input, &t, 0.6);
        assert!(s.firing);
        s.receive_hit(15.0, 0, Vec3::ZERO);
        run(&mut s, input, &t, 0.1);
        assert_eq!(s.hit_partial.clip, crc32(b"HitFront"));
        assert!(s.firing);
        assert_eq!(s.mode, Mode::Strafe);
        run(&mut s, input, &t, 0.6);
        assert!(s.hit_partial.playing().is_none());
        assert!(s.firing);
    }

    #[test]
    fn installed_flybacks_recover_and_release_the_control_state() {
        let t = installed_animation_tuning();
        for (flag, fly, recover) in [(melee::damage::BY_FAST_FLAIL, "Flyback_Fast", "Recover_Fast"),
            (melee::damage::BY_STRONG_FLAIL, "Flyback_Slow", "Recover_Slow")] {
            let mut s = State::new(&t);
            run(&mut s, Input::default(), &t, 0.1);
            s.receive_hit(30.0, flag, Vec3::new(0.0, -18.0, 12.0));
            run(&mut s, Input::default(), &t, 0.15);
            assert_eq!(s.base.clip, crc32(fly.as_bytes()));
            assert_eq!(s.control_state(&t), Some("stateHitReact"));
            let mut recovered = false;
            for _ in 0..300 {
                step(&mut s, &Input::default(), &t, &Arena::default(), DT);
                recovered |= s.base.clip == crc32(recover.as_bytes());
            }
            assert!(recovered, "{fly} never reached {recover}");
            assert_eq!(s.control_state(&t), Some("stateIdle"));
            assert!(!s.hit_full && s.health > 0.0);
        }
    }

    #[test]
    fn installed_air_death_and_vehicle_death_follow_their_exit_rules() {
        let t = installed_animation_tuning();
        let mut s = State::new(&t);
        s.pos.z = 10.0;
        s.touching = false;
        s.damage(f32::MAX);
        run(&mut s, Input::default(), &t, 0.1);
        assert_eq!(s.death.as_ref().unwrap().clip, crc32(b"Death_Explode_Fall"));
        run(&mut s, Input::default(), &t, 2.0);
        assert_eq!(s.death.as_ref().unwrap().clip, crc32(b"Death_Explode_HitGround"));
        assert_eq!(s.control_state(&t), Some("stateDeath"));
        let mut s = State::new(&t);
        s.car_mode = true;
        s.damage(f32::MAX);
        step(&mut s, &Input::default(), &t, &Arena::default(), DT);
        assert_eq!(s.death.as_ref().unwrap().clip, crc32(b"Death_V2R"));
        run(&mut s, Input::default(), &t, 3.0);
        assert_eq!(s.death.as_ref().unwrap().clip, crc32(b"Death_Explode_HitGround"));
    }

    #[test]
    fn installed_weapon_landings_hand_their_overrun_to_running_clips() {
        let t=installed_animation_tuning();
        let land=t.weapon_set.clips.iter().find(|c|c.continue_time).unwrap();
        let mut s=State::new(&t);
        s.strafe.strafing=true;
        strafe_start(&mut s,&t,land.id);
        weapon_layers(&mut s,&t,land.ticks/(60.0*land.speed)+0.01);
        let previous=*s.strafe.layers.last().unwrap();
        assert!(previous.overrun>0.0);
        weapon_rules(&mut s,&Input { stick:Vec2::Y,aim:Some(Vec2::Y),..Input::default() },&t);
        assert_ne!(s.strafe.clip,land.id);
        let next=t.weapon_set.clip(s.strafe.clip).unwrap();
        let elapsed=if next.keeps_time { previous.elapsed_ticks } else { previous.overrun*next.speed };
        assert!(s.strafe.tick>0.0);
        assert!((s.strafe.tick - if next.looping { elapsed%next.ticks } else { elapsed.min(next.ticks) }).abs()<1e-4);
    }

    #[test]
    fn installed_hit_reaction_fall_permission_obeys_the_drop_and_clip_gate() {
        let t = installed_animation_tuning();
        for (flag, early_fall) in [(melee::damage::BY_FAST_FLAIL, true), (melee::damage::BY_KNOCKBACK, false)] {
            let mut s = State::new(&t);
            run(&mut s, Input::default(), &t, 0.1);
            s.pos.z = 30.0;
            s.touching = false;
            s.touching_for = 1.0;
            s.receive_hit(10.0, flag, Vec3::new(0.0,-18.0,0.0));
            run(&mut s, Input::default(), &t, 0.12);
            assert_eq!(s.control_state(&t), Some("stateHitReact"));
            // Drop more than 2.5m while the reaction clip still plays.
            s.pos.z = 26.0;
            step(&mut s, &Input::default(), &t, &Arena::default(), DT);
            assert_eq!(s.control_state(&t), Some(if early_fall { "stateFall" } else { "stateHitReact" }));
        }
    }

    /// The climb state's exit with Climb held locks the climbable test until the button is
    /// let go (`FUN_0087a820`, owner `+0x3f8`). [game]
    #[test]
    fn installed_a_climb_left_with_climb_held_is_not_taken_up_until_it_is_let_go() {
        let t = installed_animation_tuning();
        let arena = Arena { boxes: vec![Box3 { min: Vec3::new(-10.0, 10.0, 0.0), max: Vec3::new(10.0, 30.0, 40.0) }], ..Default::default() };
        let climb = Input { buttons_down: 1 << button::CLIMB, buttons_hit: 1 << button::CLIMB, ..Default::default() };
        let held = Input { buttons_down: 1 << button::CLIMB, ..Default::default() };
        let start = |lock: bool| {
            let mut s = State::new(&t);
            s.pos = Vec3::new(0.0, 10.0 - t.robot_radius - 0.5, 0.0);
            for _ in 0..4 {
                step(&mut s, &Input::default(), &t, &arena, DT);
            }
            // As a climb left with the button down leaves it.
            s.climb_regrab_lock = lock;
            s
        };
        // Unlocked, Climb held at the wall starts the climb.
        let mut s = start(false);
        step(&mut s, &climb, &t, &arena, DT);
        for _ in 0..3 {
            step(&mut s, &held, &t, &arena, DT);
        }
        assert_eq!(s.control_state(&t), Some("stateClimb"));
        // Locked, it does not, however long it is held.
        let mut s = start(true);
        step(&mut s, &climb, &t, &arena, DT);
        for _ in 0..10 {
            step(&mut s, &held, &t, &arena, DT);
        }
        assert_ne!(s.control_state(&t), Some("stateClimb"));
        assert!(s.climb_regrab_lock);
        // Let go once and pressed again: it does.
        step(&mut s, &Input::default(), &t, &arena, DT);
        assert!(!s.climb_regrab_lock);
        step(&mut s, &climb, &t, &arena, DT);
        for _ in 0..3 {
            step(&mut s, &held, &t, &arena, DT);
        }
        assert_eq!(s.control_state(&t), Some("stateClimb"));
    }

    #[test]
    fn health_gone_he_falls_back_along_the_death_clip_and_stays_down() {
        let mut t = tuning();
        with_death_clip(&mut t);
        let mut s = State::new(&t);
        let start = s.pos;
        s.damage(t.base_health);
        run(&mut s, Input { stick: Vec2::Y, jump: true, ..Input::default() }, &t, 0.5);
        assert!(s.dead());
        assert_eq!(s.action_clip(&t).map(|(set, id, _)| (set, id)), Some(("DeathSet", "Death")));
        run(&mut s, Input { stick: Vec2::Y, ..Input::default() }, &t, 3.0);
        // He went 10.8 backwards (he faces +y) whatever the stick said, and nothing healed him.
        assert!((s.pos.y - start.y + 10.8).abs() < 0.3, "{:?}", s.pos);
        assert_eq!((s.health, s.vel.length()), (0.0, 0.0));
        s.damage(50.0);
        assert!(s.dead() && s.health == 0.0);
    }

    #[test]
    fn a_hit_that_leaves_health_does_not_kill() {
        let t = tuning();
        let mut s = State::new(&t);
        s.damage(t.base_health - 1.0);
        run(&mut s, Input::default(), &t, 0.2);
        assert!(!s.dead() && s.health > 0.0);
    }

    fn jump_apex(s: &mut State, held: Input, t: &Tuning) -> f32 {
        let arena = Arena::default();
        step(s, &Input { jump: true, ..held }, t, &arena, DT);
        for _ in 0..600 {
            step(s, &held, t, &arena, DT);
            if !matches!(s.mode, Mode::Jump(_)) {
                break;
            }
        }
        s.apex_z - s.launch_z
    }

    #[test]
    fn standing_jump_peaks_a_fifth_above_the_run_jump_height() {
        let t = tuning();
        let mut s = State::new(&t);
        let apex = jump_apex(&mut s, Input::default(), &t);
        assert!((apex - 12.0).abs() < 0.2, "apex {apex}");
    }

    #[test]
    fn running_jump_uses_the_run_jump_and_lands_at_walk_speed() {
        let t = tuning();
        let mut s = State::new(&t);
        let forward = Input { stick: Vec2::Y, ..Default::default() };
        run(&mut s, forward, &t, 1.0);
        assert_eq!(s.gait, Gait::Run);
        assert!((s.speed - t.locomotion.run_speed).abs() < 0.01);
        // Half stick is under the run rule's threshold: the walk clip's pace.
        run(&mut s, Input { stick: Vec2::Y * 0.5, ..Default::default() }, &t, 1.0);
        assert_eq!(s.gait, Gait::Walk);
        assert!((s.speed - t.locomotion.walk_speed).abs() < 0.01);
        run(&mut s, forward, &t, 1.0);
        let arena = Arena::default();
        step(&mut s, &Input { jump: true, ..forward }, &t, &arena, DT);
        assert_eq!(s.mode, Mode::Jump(JumpType::Moving));
        // Off at MoveMode's top speed; the jump's first update of air control is already in.
        assert!((Vec2::new(s.vel.x, s.vel.y).length() - t.moving.max_speed).abs() < 0.5);
        for _ in 0..600 {
            step(&mut s, &Input::default(), &t, &arena, DT);
            if !matches!(s.mode, Mode::Jump(_)) {
                break;
            }
        }
        assert_eq!(s.mode, Mode::Stand);
        assert!((s.apex_z - t.jump.run.height).abs() < 0.3, "apex {}", s.apex_z);
        assert!(s.speed <= t.moving.walk_run_speed + 1e-3);
    }

    #[test]
    fn robot_turns_at_the_move_turn_speed() {
        let t = tuning();
        let mut s = State::new(&t);
        // Stick hard left asks for a quarter turn; a fifth of a second at 360 degrees a second gets 72.
        run(&mut s, Input { stick: Vec2::NEG_X, ..Default::default() }, &t, 0.2);
        assert!((s.yaw.to_degrees() - 72.0).abs() < 0.5, "yaw {}", s.yaw.to_degrees());
    }

    /// The robot flyers' FlySet entry rules (OptimusFlight, MegatronFlight) read the camera stick:
    /// `F_R_STICK_DIR` beyond 95 either way is Fly_Up, inside 85 Fly_Down, between Fly_Idle. With
    /// the port's `look` (y = -1 pushed up, which climbs), up must read past 95 and down inside 85.
    #[test]
    fn the_camera_stick_reads_as_the_robot_flyers_rules_expect() {
        let t = tuning();
        let mut s = State::new(&t);
        s.mode = Mode::Fly;
        let dir = |look: Vec2| match rule_value(&s, &Input { look, ..Default::default() }, animrules::var::R_STICK_DIR) {
            Some(Value::Float(v)) => v,
            other => panic!("{other:?}"),
        };
        assert!(dir(Vec2::new(0.0, -1.0)).abs() >= 95.0, "climb is Fly_Up");
        assert!(dir(Vec2::new(0.0, 1.0)).abs() < 85.0, "dive is Fly_Down");
        let side = dir(Vec2::new(1.0, 0.0)).abs();
        assert!((85.0..95.0).contains(&side), "sideways is Fly_Idle: {side}");
        // The left stick in flight is measured against his own heading, not a stale aim.
        s.yaw = 1.0;
        s.strafe.aim_yaw = -2.0;
        let ahead = Input { stick: s.facing(), ..Default::default() };
        match rule_value(&s, &ahead, animrules::var::L_STICK_DIR) {
            Some(Value::Float(v)) => assert!(v.abs() < 0.01, "{v}"),
            other => panic!("{other:?}"),
        }
        // [game 007112b0] the camera stick goes through his own frame and the heading comes
        // off again: turned, the same push reads the same.
        let dir_of = |s: &State, look: Vec2| match rule_value(s, &Input { look, ..Default::default() }, animrules::var::R_STICK_DIR) {
            Some(Value::Float(v)) => v,
            other => panic!("{other:?}"),
        };
        let side = dir_of(&s, Vec2::new(1.0, 0.0));
        assert!((side.abs() - 90.0).abs() < 0.01, "{side}");
        // Nose up, the forward part shrinks by cos(pitch): a diagonal push reads wider.
        s.pitch = 1.0;
        let diagonal = dir_of(&s, Vec2::new(1.0, 1.0)).abs();
        assert!((diagonal - (1.0f32).atan2(1.0f32.cos()).to_degrees()).abs() < 0.01, "{diagonal}");
    }

    /// [game] FlyMode's turbo and brake live on the FlyMode object, built once (00863970):
    /// a cooldown started in one flight runs on foot and is still there at the next.
    #[test]
    fn the_flight_turbo_cooldown_carries_over_between_flights_and_runs_on_foot() {
        let mut t = tuning();
        (t.forms.fly.max_turbo_time, t.forms.fly.max_turbo_cooldown_time) = (0.5, 2.0);
        (t.forms.fly.brake_duration, t.forms.fly.brake_cooldown) = (0.3, 1.0);
        let arena = Arena::default();
        let mut s = State::new(&t);
        let fly = |s: &mut State, input: &Input| {
            if s.mode != Mode::Fly {
                set_mode(s, input, &t, Mode::Fly);
                s.mode = Mode::Fly;
            }
            step_fly(s, input, &t, &arena, GAME_UPDATE);
        };
        s.pos.z = 50.0;
        fly(&mut s, &Input { jump: true, ..Default::default() });
        // [game 0071dc20 l.146, 0071eb50] the press's own update ticks the turbo once after the
        // press is read: 0.5 - one game update (0.468), not 0.5.
        assert!((s.fly_timers.turbo_left - (0.5 - GAME_UPDATE)).abs() < 1e-6, "{:?}", s.fly_timers);
        for _ in 0..20 {
            fly(&mut s, &Input::default());
        }
        assert_eq!(s.fly_timers.turbo_left, 0.0);
        assert!(s.fly_timers.turbo_cooldown > 1.5, "{:?}", s.fly_timers);
        // He lands: the flight is gone, the timers are not, and they run on foot.
        set_mode(&mut s, &Input::default(), &t, Mode::Move);
        s.mode = Mode::Move;
        assert!(s.fly.is_none());
        let before = s.fly_timers.turbo_cooldown;
        for _ in 0..10 {
            step(&mut s, &Input::default(), &t, &arena, GAME_UPDATE);
        }
        assert_ne!(s.mode, Mode::Fly);
        let ran = before - s.fly_timers.turbo_cooldown;
        assert!((ran - 10.0 * GAME_UPDATE).abs() < 1e-4, "{ran}");
        // The next flight starts with that cooldown: a press does nothing.
        fly(&mut s, &Input { jump: true, ..Default::default() });
        assert_eq!(s.fly.as_ref().unwrap().turbo_left, 0.0);
        assert!(s.fly_timers.turbo_cooldown > 0.0 && s.fly_timers.turbo_cooldown < before);
    }

    /// [game 00732e60] [trace] the flight turbo is the car's turbo class, so the upgrade levels
    /// stretch it: level 3 doubles the time (the 2026-10-21 Starscream capture: 2.5 s held 5.0 s
    /// from the press) and halves the cooldown.
    #[test]
    fn the_flight_turbo_takes_the_upgrade_levels() {
        let mut t = tuning();
        (t.forms.fly.max_turbo_time, t.forms.fly.max_turbo_cooldown_time) = (2.5, 3.0);
        t.drive.turbo_upgrades = [[1.25, 1.5, 2.0], [0.85, 0.65, 0.5]];
        let arena = Arena::default();
        let mut s = State::new(&t);
        s.turbo_upgrades = [3, 3];
        step(&mut s, &Input::default(), &t, &arena, GAME_UPDATE);
        assert_eq!(s.fly_timers.upgrade, Some((2.0, 0.5)));
        set_mode(&mut s, &Input::default(), &t, Mode::Fly);
        s.mode = Mode::Fly;
        s.pos.z = 50.0;
        step_fly(&mut s, &Input { jump: true, ..Default::default() }, &t, &arena, GAME_UPDATE);
        assert!((s.fly_timers.turbo_left - (5.0 - GAME_UPDATE)).abs() < 1e-5, "{:?}", s.fly_timers);
        while s.fly_timers.turbo_left > 0.0 {
            step_fly(&mut s, &Input::default(), &t, &arena, GAME_UPDATE);
        }
        assert!((s.fly_timers.turbo_cooldown - 1.5).abs() < 1e-5, "{:?}", s.fly_timers);
        // No upgrades: the character's own times.
        let mut s = State::new(&t);
        step(&mut s, &Input::default(), &t, &arena, GAME_UPDATE);
        assert_eq!(s.fly_timers.upgrade, Some((1.0, 1.0)));
    }

    /// Starscream's special (type 6) has no case in the type switch; what it does is read by the
    /// weapons while its timer runs (00721c10 and the same test in 00782b30 / 00785de0).
    #[test]
    fn the_type_six_special_is_on_only_while_its_timer_runs() {
        let mut t = tuning();
        t.special.kind = 6;
        t.damage.starscream_special_multiplier = 2.0;
        let mut s = State::new(&t);
        assert_eq!(s.slowing_special(&t), None);
        s.special_left = 5.0;
        assert_eq!(s.slowing_special(&t), Some(2.0));
        t.special.kind = -1;
        assert_eq!(s.slowing_special(&t), None, "Bumblebee's timer does not do it");
    }

    #[test]
    fn car_tops_out_and_turbo_runs_its_time() {
        let t = tuning();
        let mut s = State::new(&t);
        let gas = Input { vehicle: 1.0, ..Default::default() };
        run(&mut s, gas, &t, 6.0);
        assert_eq!(s.mode, Mode::Drive);
        // The engine pushes for one update past the cap of 35: the game settles at 37.6.
        assert!((s.speed - 37.6).abs() < 0.3, "speed {}", s.speed);
        let arena = Arena::default();
        step(&mut s, &Input { turbo: true, ..gas }, &t, &arena, DT);
        run(&mut s, gas, &t, 2.0);
        // Cap 43 in turbo; the game holds 48.1.
        assert!((s.speed - 48.1).abs() < 0.4, "speed {}", s.speed);
        run(&mut s, gas, &t, 1.5);
        // 1.1 s after the turbo ran out the game was at 39.1 and still easing down.
        assert!((s.speed - 39.1).abs() < 0.8, "speed {}", s.speed);
        assert!(s.turbo_cooldown > 0.0);
    }

    /// The car's preset rumbles: the turbo as it starts, and a wall met at over 5.
    #[test]
    fn the_car_rumbles_the_pad_for_its_turbo_and_its_knocks() {
        use crate::rumble::{Preset, preset};
        let mut t = tuning();
        t.rumble_presets = (0..15).map(|n| Preset { kind: 1, duration: 0.25, strength: n as f32 }).collect();
        let started = |s: &State, n: usize| s.rumbles.iter().filter(|r| r.strength == n as f32).count();
        let mut s = State::new(&t);
        let gas = Input { vehicle: 1.0, ..Default::default() };
        let arena = Arena::default();
        let mut early = 0;
        for _ in 0..180 {
            step(&mut s, &gas, &t, &arena, DT);
            early += s.rumbles.len();
        }
        assert_eq!((s.mode, early), (Mode::Drive, 0));
        // The turbo: once, on the update it starts, and not again while it runs.
        let mut turbos = 0;
        for _ in 0..30 {
            step(&mut s, &Input { turbo: true, ..gas }, &t, &arena, DT);
            turbos += started(&s, preset::VEHICLE_TURBO);
        }
        assert_eq!(turbos, 1);
        // Into a wall at speed: the wall preset.
        let ahead = s.pos + s.facing().extend(0.0) * 60.0;
        let arena = Arena { boxes: vec![Box3 { min: ahead - Vec3::new(40.0, 5.0, 0.0), max: ahead + Vec3::new(40.0, 5.0, 20.0) }], ..Arena::default() };
        let mut knocks = 0;
        for _ in 0..240 {
            step(&mut s, &gas, &t, &arena, DT);
            knocks += started(&s, preset::VEHICLE_HIT_WALL);
        }
        assert!(knocks >= 1, "no knock; at {} speed {}", s.pos, s.speed);
    }

    #[test]
    fn car_pulls_away_smoothly_and_reaches_recorded_cap() {
        let t = tuning();
        let mut s = State::new(&t);
        let gas = Input { vehicle: 1.0, ..Default::default() };
        let arena = Arena::default();
        step(&mut s, &gas, &t, &arena, GAME_DT);
        // Flat arena start. Run1 t621.66 was on changing/sloped ground (support
        // normal xy .0087,-.0048 rising to .0528,-.0415), not this arena. The old
        // unseeded speed-curve comparison rewarded discarding horizontal spring
        // force. Recorded spring/velocity/omega replay now lives in vehicle.rs.
        let got = speeds(&mut s, gas, &t, 45);
        assert!(got[..36].windows(2).all(|v|v[1]>v[0]));
        assert!(got[0]>=0.0 && got[0]<1.0 && got[1]>0.0);
        assert!((got[44] - 37.0).abs() < 0.5, "{:.2}", got[44]);
    }

    #[test]
    fn car_brakes_in_a_straight_line_and_reverses() {
        let t = tuning();
        let mut s = State::new(&t);
        let gas = Input { vehicle: 1.0, ..Default::default() };
        run(&mut s, gas, &t, 4.0);
        let braking = Input { brake: 1.0, ..gas };
        let before = s.speed;
        run(&mut s, braking, &t, 1.0);
        // t 421.31 in run 1: 37.61, and 25.17 a second later. The cap comes down by
        // brakingDeceleration (15 a second) while the engine keeps pushing a little past it.
        assert!((before - s.speed - 12.4).abs() < 0.8, "lost {}", before - s.speed);
        run(&mut s, braking, &t, 4.0);
        assert!(s.speed < -10.0 && s.speed >= -t.drive.max_reverse_speed - 3.0, "reverse {}", s.speed);
    }

    #[test]
    fn car_turns_toward_the_stick_and_straightens() {
        let t = tuning();
        let mut s = State::new(&t);
        let gas = Input { vehicle: 1.0, ..Default::default() };
        run(&mut s, gas, &t, 4.0);
        let yaw = s.yaw;
        run(&mut s, Input { steer: 1.0, ..gas }, &t, 1.0);
        // Stick right turns him clockwise. At top speed the game turned at about 1 radian a
        // second with the stick hard over (t 644.9: 0.75 rising to 1.47, then settling).
        let turned = yaw - s.yaw;
        assert!((0.6..1.6).contains(&turned), "turned {turned}");
        run(&mut s, gas, &t, 1.5);
        assert!(s.car.yaw_rate.abs() < 0.05, "still turning at {}", s.car.yaw_rate);
        assert!(s.speed > 30.0);
    }

    #[test]
    fn jumping_out_of_the_car_at_speed_is_the_long_jump() {
        let t = tuning();
        let arena = Arena::default();
        let gas = Input { vehicle: 1.0, ..Default::default() };
        // Let go first and jump after: the unfold has no way into a jump. [game: the graph]
        let mut s = State::new(&t);
        run(&mut s, gas, &t, 4.0);
        step(&mut s, &Input::default(), &t, &arena, DT);
        assert_eq!(s.mode, Mode::Unfold);
        step(&mut s, &Input { jump: true, jump_held: true, ..Default::default() }, &t, &arena, DT);
        assert_eq!(s.mode, Mode::Unfold);
        // Jump held as the trigger goes up: the long jump.
        let mut s = State::new(&t);
        run(&mut s, gas, &t, 4.0);
        step(&mut s, &Input { jump: true, jump_held: true, ..Default::default() }, &t, &arena, DT);
        assert_eq!(s.mode, Mode::Jump(JumpType::Long));
        // With the stick centred the long jump still drives toward its own speed, and it
        // peaks at the long jump's height.
        let before = Vec2::new(s.vel.x, s.vel.y).length();
        for _ in 0..600 {
            step(&mut s, &Input::default(), &t, &arena, DT);
            if s.vel.z <= 0.0 {
                break;
            }
        }
        assert!(Vec2::new(s.vel.x, s.vel.y).length() > before);
        assert!((s.apex_z - t.jump.long.height).abs() < 0.3, "apex {}", s.apex_z);
    }

    #[test]
    fn walls_block_and_low_boxes_can_be_stood_on() {
        let t = tuning();
        let arena = Arena { boxes: vec![Box3 { min: Vec3::new(-5.0, 10.0, 0.0), max: Vec3::new(5.0, 20.0, 4.0) }], ..Default::default() };
        let mut s = State::new(&t);
        for _ in 0..240 {
            step(&mut s, &Input { stick: Vec2::Y, ..Default::default() }, &t, &arena, DT);
        }
        assert!(s.pos.y <= 10.0 - t.robot_radius + 0.05, "y {}", s.pos.y);
        assert_eq!(arena.floor(0.0, 15.0, 4.0), 4.0);
        assert_eq!(arena.floor(0.0, 15.0, 0.0), 0.0);
    }

    /// Coming down just under a ledge after the jump up off a wall, he is put on it; not
    /// while still rising, not from further below, not without a top set by the climb.
    #[test]
    fn the_jump_up_off_a_wall_catches_the_ledge_above() {
        let t = tuning();
        let arena = Arena { boxes: vec![Box3 { min: Vec3::new(-10.0, 10.0, 0.0), max: Vec3::new(10.0, 30.0, 20.0) }], ..Default::default() };
        let under = |z: f32, vz: f32| {
            let mut s = State::new(&t);
            s.pos = Vec3::new(0.0, 10.0 - t.robot_radius, z);
            s.vel = Vec3::new(0.0, 0.0, vz);
            s.climb_top = Vec3::new(0.0, s.pos.y, 20.0);
            s.climb_top_facing = Vec2::Y;
            s
        };
        let mut s = under(19.0, -3.0);
        assert!(ledge_catch(&mut s, &t, &arena, true));
        let on_top = Vec3::new(0.0, 10.0 - t.robot_radius + 2.25 * t.robot_radius, 20.0);
        assert!((s.pos - on_top).length() < 1e-4, "{:?}", s.pos);
        assert_eq!(s.vel, Vec3::ZERO);

        assert!(!ledge_catch(&mut under(19.0, 3.0), &t, &arena, true), "still rising");
        // Leaving the pull-up (`FUN_0087a200`), rising does not matter.
        assert!(ledge_catch(&mut under(19.0, 3.0), &t, &arena, false), "the pull-up's exit");
        assert!(!ledge_catch(&mut under(18.4, -3.0), &t, &arena, true), "more than 1.5 below");
        assert!(!ledge_catch(&mut under(20.5, -3.0), &t, &arena, true), "above the top");
        let mut turned = under(19.0, -3.0);
        turned.yaw = 0.5;
        assert!(!ledge_catch(&mut turned, &t, &arena, true), "not facing as at the top");
        let mut unset = under(19.0, -3.0);
        unset.climb_top_facing = Vec2::ZERO;
        assert!(!ledge_catch(&mut unset, &t, &arena, true), "the climb set no top");
    }

    /// A small copy of the attack chain's shape: a press from standing starts a hit; a press
    /// inside the hit's button window chains a second when the window shuts; a hit that runs
    /// out goes back to standing.
    fn with_attack_chain(mut t: Tuning) -> Tuning {
        use crate::control::{Detector, Stage, StateDef, StickRange, Test, Transition};
        use crate::formats::anim::{Action as A, ActionEvent};
        let any = StickRange { amp_min: -1.0, amp_max: 100.0, angle_min: -7.0, angle_max: 7.0, camera_relative: true };
        let press = Stage { time: 0.0, charge: 0.0, include: vec![], exclude: vec![], hit: Some(button::ATTACK_FAST), left: any };
        let mut g = std::mem::take(&mut t.control);
        let attack = g.add_connection("attack", Test::Detect(Detector { stages: vec![press], stays_on: 0.5, hold_with_last_stage: false }));
        g.connections[attack].on_ground = true;
        let chain = g.add_connection("chain", Test::None);
        g.connections[chain].attack_can_branch = true;
        let done = g.add_connection("done", Test::None);
        (g.connections[done].branch_begin, g.connections[done].branch_end) = (100, 100);
        let state = |name: &str, class: &str, anim: &str| StateDef {
            name: name.into(),
            class: class.into(),
            anim: if anim.is_empty() { 0 } else { crc32(anim.as_bytes()) },
            motion_mode: ATTACK_MODE,
            ..Default::default()
        };
        let idle = g.find("stateIdle").unwrap();
        let one = g.add_state(state("stateAttack1", "StateAttack", "Hit1"));
        let two = g.add_state(state("stateAttack2", "StateAttack", "Hit2"));
        g.states[idle].transitions.push(Transition { connection: attack, output: Some(one), priority: 3 });
        g.states[one].transitions = vec![
            Transition { connection: chain, output: Some(two), priority: 2 },
            Transition { connection: done, output: Some(idle), priority: 0 },
        ];
        g.states[two].transitions = vec![Transition { connection: done, output: Some(idle), priority: 0 }];
        t.control = g;
        // 60 ticks carrying the root 6 forward; the window from tick 10 to 36.
        let event = |time, action| ActionEvent { time, action };
        let clip = |id: &str| ActionClip {
            set: "CombatSet".into(),
            id: id.into(),
            ticks: 60.0,
            speed: 1.0,
            crossfade: 0.0,
            looping: false,
            path: (0..=60).map(|tick| Vec3::Y * (tick as f32 / 10.0)).collect(),
            turn: Vec::new(),
            events: vec![
                event(10.0, A::ButtonWindow { open: true }),
                event(36.0, A::ButtonWindow { open: false }),
                event(38.0, A::BranchPoint),
            ],
            attack: None,
            bodies: Vec::new(),
            mounts: Vec::new(),
            weapon_events: Vec::new(),
            blasts: Vec::new(),
        };
        for id in ["Hit1", "Hit2"] {
            t.actions.clips.insert(crc32(id.as_bytes()), clip(id));
        }
        t
    }

    #[test]
    fn a_press_attacks_and_a_press_in_the_window_chains_the_next_hit() {
        let t = with_attack_chain(tuning());
        let arena = Arena::default();
        let tap = Input { buttons_down: 1 << button::ATTACK_FAST, buttons_hit: 1 << button::ATTACK_FAST, ..Default::default() };
        let attack_state = |s: &State| s.control_state(&t).map(str::to_owned);

        // One hit on its own: the clip's root carries him 6 forward, then he stands again.
        let mut s = State::new(&t);
        step(&mut s, &tap, &t, &arena, DT);
        assert_eq!(s.mode, Mode::Action);
        assert_eq!(attack_state(&s).as_deref(), Some("stateAttack1"));
        for _ in 0..70 {
            step(&mut s, &Input::default(), &t, &arena, DT);
        }
        assert_eq!(s.mode, Mode::Stand);
        assert!((s.pos.y - 6.0).abs() < 0.3, "travelled {}", s.pos.y);

        // A press inside the window: the second hit starts when the window shuts (tick 36).
        let mut s = State::new(&t);
        step(&mut s, &tap, &t, &arena, DT);
        for _ in 0..20 {
            step(&mut s, &Input::default(), &t, &arena, DT);
        }
        step(&mut s, &tap, &t, &arena, DT);
        let mut chained_at = None;
        for i in 0..40 {
            step(&mut s, &Input::default(), &t, &arena, DT);
            if chained_at.is_none() && attack_state(&s).as_deref() == Some("stateAttack2") {
                chained_at = Some(22 + i);
            }
        }
        assert_eq!(chained_at, Some(36), "chained at update {chained_at:?}");

        // A press before the window opens does not count.
        let mut s = State::new(&t);
        step(&mut s, &tap, &t, &arena, DT);
        step(&mut s, &tap, &t, &arena, DT);
        for _ in 0..70 {
            step(&mut s, &Input::default(), &t, &arena, DT);
        }
        assert_eq!(s.mode, Mode::Stand);
        assert!((s.pos.y - 6.0).abs() < 0.3, "travelled {}", s.pos.y);
    }

    /// The attack chain with what the first hit's clip carries in the install: the turn and
    /// the slide allowed from the start to tick 18, the hit live from tick 10 to 20.
    fn with_melee(t: Tuning) -> Tuning {
        use crate::formats::anim::{Action as A, ActionEvent, AttackRef};
        use crate::formats::scene::Shape;
        const HAND: u32 = crc32(b"Hand_R");
        let mut t = with_attack_chain(t);
        // The attack is taken before a move, so a press with the stick held attacks.
        let idle = t.control.find("stateIdle").unwrap();
        let first = t.control.find("stateAttack1");
        for transition in t.control.states[idle].transitions.iter_mut().filter(|tr| tr.output == first) {
            transition.priority = 100;
        }
        t.melee = crate::melee::MeleeTuning {
            ang_range: 90.0,
            best_dist: 1.5,
            angle_weight: 1.0,
            dist_weight: 0.5,
            match_weight: 0.75,
            distance: 25.0,
            height: 5.0,
            upgrades: [[1.25, 1.5, 2.0]; 3],
        };
        for clip in t.actions.clips.values_mut() {
            clip.attack = Some(AttackRef {
                damage: 30.0,
                mp_damage: 35.0,
                ai_damage: -1.0,
                slide_speed: 60.0,
                max_range: 20.0,
                auto_target_turn_speed: 900.0,
                turn_speed: 900.0,
                stick_choose_target_max_degrees: 180.0,
                auto_target_search_angle: 90.0,
                knock_back_speed: 24.0,
                knock_back_angle: 12.0,
                ..Default::default()
            });
            let event = |time, action| ActionEvent { time, action };
            clip.events.extend([
                event(0.0, A::FaceAndSlide { slide: true, face: true, stick: true }),
                event(10.0, A::Hit { on: true, node: HAND }),
                event(18.0, A::FaceAndSlide { slide: false, face: false, stick: false }),
                event(20.0, A::Hit { on: false, node: HAND }),
            ]);
            // A hand out in front at chest height all through, the size of his own.
            let hand = melee::Body::new(Shape::Sphere { radius: 1.83 }, Affine3A::from_translation(Vec3::new(0.0, 2.5, 3.5)));
            clip.bodies = vec![melee::AttackBody { node: HAND, foot: false, frames: vec![hand; clip.ticks as usize + 1] }];
            clip.events.sort_by(|a, b| a.time.total_cmp(&b.time));
        }
        t
    }

    /// Every clip also sets off the ground punch's blast at tick 8, with the numbers of
    /// Bumblebee's `genericgroundpound`, from a hand held out in front.
    fn with_blast(mut t: Tuning) -> Tuning {
        use crate::formats::anim::{Action as A, ActionEvent};
        use crate::formats::scene::ExplosionDef;
        const NAME: u32 = crc32(b"FX_groundpunch");
        (t.ground_pound_damage, t.ground_pound_damage_weak) = (100.0, 60.0);
        for clip in t.actions.clips.values_mut() {
            clip.events.push(ActionEvent { time: 8.0, action: A::Trigger { name: NAME } });
            clip.events.sort_by(|a, b| a.time.total_cmp(&b.time));
            clip.blasts = vec![melee::ClipBlast {
                ssd: Vec::new(),
                name: NAME,
                node: crc32(b"Hand_R"),
                attach: true,
                explosion: ExplosionDef {
                    radius: 18.0,
                    start_scale: 0.5,
                    time: 0.15,
                    delay: 0.0,
                    damage: 65.0,
                    min_damage: 0.0,
                    fall_off: -1.0,
                    pound_override: true,
                    damage_player: true,
                    can_damage_self: false,
                    damage_flags: melee::damage::BY_MELEE | melee::damage::BY_EXPLOSION | melee::damage::BY_STRONG_FLAIL,
                    knock_back_speed: 35.0,
                    knock_back_angle: 3.0,
                    camera_shake: crc32(b"MassiveCameraShake"),
                    stun_time: 0.0,
                    slow_time: 0.0,
                    dot_time: 0.0,
                    heat: 0.0,
                },
                rumble: Some(crate::rumble::RumbleDef {
                    kind: 1,
                    duration: 0.7,
                    strength: 0.55,
                    scale_type: 1,
                    distance_near: 1.0,
                    distance_far: 25.0,
                }),
                centres: vec![Vec3::new(0.0, 2.5, 3.5); clip.ticks as usize + 1],
            }];
        }
        t
    }

    /// `stop_on_hit` (the slam out of the car): the clip's root stops moving him once the
    /// attack has hit a character.
    #[test]
    fn a_clip_that_stops_on_a_hit_carries_him_no_further() {
        let travel = |stop: bool| {
            let mut t = with_melee(tuning());
            for clip in t.actions.clips.values_mut() {
                let attack = clip.attack.as_mut().unwrap();
                (attack.stop_on_hit, attack.slide_speed) = (stop, 0.0);
            }
            let arena = Arena { targets: vec![enemy(1, 0.0, 7.0)], ..Default::default() };
            let tap = Input { buttons_down: 1 << button::ATTACK_FAST, buttons_hit: 1 << button::ATTACK_FAST, ..Default::default() };
            let mut s = State::new(&t);
            step(&mut s, &tap, &t, &arena, DT);
            let (mut hits, mut at_hit) = (0, 0.0);
            for _ in 0..70 {
                step(&mut s, &Input::default(), &t, &arena, DT);
                if !s.hits.is_empty() {
                    (hits, at_hit) = (hits + s.hits.len(), s.pos.y);
                }
            }
            assert_eq!(hits, 1);
            (s.pos.y, at_hit)
        };
        let (free, _) = travel(false);
        let (stopped, at_hit) = travel(true);
        assert!(free > stopped + 1.0, "{free} against {stopped}");
        assert!((stopped - at_hit).abs() < 0.25, "hit at {at_hit}, ended at {stopped}");
    }

    #[test]
    fn the_blast_grows_hits_each_thing_in_reach_once_and_goes() {
        let t = with_blast(with_melee(tuning()));
        // Both behind him, where no fist goes: one within the blast's 18, one beyond it.
        let arena = Arena { targets: vec![enemy(1, 0.0, -10.0), enemy(2, 0.0, -30.0)], ..Default::default() };
        let mut s = State::new(&t);
        let mut hits = Vec::new();
        let mut radii = Vec::new();
        step(&mut s, &TAP, &t, &arena, DT);
        for _ in 0..70 {
            step(&mut s, &Input::default(), &t, &arena, DT);
            hits.extend(s.hits.iter().copied());
            radii.extend(s.blasts.iter().filter(|b| b.live).map(|b| b.radius));
        }
        assert_eq!(hits.len(), 1, "{hits:?}");
        let hit = hits[0];
        // The data's 65 is over 5, so the damage is his `groundPoundDamage`.
        assert_eq!((hit.target, hit.damage, hit.knock_back_speed, hit.blast), (1, 100.0, 35.0, true));
        assert!((hit.knock_back_angle - 3f32.to_radians()).abs() < 1e-6);
        assert!(hit.direction.dot(Vec2::NEG_Y) > 0.99, "{:?}", hit.direction);
        // From half its size to all of it, then gone.
        assert_eq!(radii.first().copied(), Some(9.0));
        assert!(radii.windows(2).all(|w| w[1] >= w[0]) && radii.last().copied() == Some(18.0), "{radii:?}");
        assert!(s.blasts.is_empty());
        // 0.15 s of growing: the update that starts it, nine that grow it, one more at full size.
        assert_eq!(radii.len(), 11, "{radii:?}");
    }

    #[test]
    fn a_blast_marks_each_world_surface_once_without_hurting_a_target() {
        let mut t=with_blast(with_melee(tuning()));
        let info=crate::ssd::Info { radius:7.0,frames:4,noise_ratio:0.406,..Default::default() };
        for clip in t.actions.clips.values_mut() { for blast in &mut clip.blasts { blast.ssd=vec![info]; } }
        let arena=Arena::default(); let mut s=State::new(&t); let mut surfaces=Vec::new();
        step(&mut s,&TAP,&t,&arena,DT);
        for _ in 0..70 {
            step(&mut s,&Input::default(),&t,&arena,DT);
            surfaces.extend(s.surface_hits.iter().filter(|h|h.infos.is_some()).map(|h|h.contact.surface));
            assert!(s.hits.is_empty());
        }
        assert_eq!(surfaces,vec![0]);
    }

    #[test]
    fn a_hand_marks_a_wall_once_per_attack_even_without_an_attack_damage_record() {
        struct Wall;
        impl World for Wall {
            fn floor(&self,_:f32,_:f32,_:f32)->f32 { 0.0 }
            fn push_out(&self,_:&mut Vec3,_:f32,_:f32)->bool { false }
            fn surface_contacts(&self,_:&melee::Body)->Vec<crate::ssd::Contact> {
                vec![crate::ssd::Contact { surface:1,point:Vec3::Y*4.0,normal:-Vec3::Y }]
            }
        }
        let mut t=with_melee(tuning());
        for clip in t.actions.clips.values_mut() { clip.attack=None; }
        let mut s=State::new(&t);
        for _ in 0..2 {
            let mut hits=0;
            step(&mut s,&TAP,&t,&Wall,DT);
            for _ in 0..80 {
                step(&mut s,&Input::default(),&t,&Wall,DT);
                hits+=s.surface_hits.len();
            }
            assert_eq!(hits,1);
        }
    }

    #[test]
    fn the_blast_shakes_the_camera_and_the_pad_once_as_it_is_set_off() {
        let t = with_blast(with_melee(tuning()));
        let arena = Arena::default();
        let mut s = State::new(&t);
        let (mut shakes, mut rumbles) = (Vec::new(), Vec::new());
        step(&mut s, &TAP, &t, &arena, DT);
        for _ in 0..70 {
            step(&mut s, &Input::default(), &t, &arena, DT);
            shakes.extend(s.shakes.iter().copied());
            rumbles.extend(s.rumbles.iter().copied());
        }
        // The shake by its name, at the hand: 2.5 ahead of him and 3.5 up.
        assert_eq!(shakes.len(), 1, "{shakes:?}");
        assert_eq!(shakes[0].name, crc32(b"MassiveCameraShake"));
        assert!((shakes[0].at.z - 3.5).abs() < 1e-4 && shakes[0].at.distance(s.pos) < 6.0, "{:?}", shakes[0]);
        // The rumble weakened by the hand's distance from him: 0.55 * (25 - d) / 24.
        assert_eq!(rumbles.len(), 1, "{rumbles:?}");
        let d = Vec3::new(0.0, 2.5, 3.5).length();
        assert_eq!((rumbles[0].kind, rumbles[0].duration), (1, 0.7));
        assert!((rumbles[0].strength - 0.55 * (25.0 - d) / 24.0).abs() < 1e-4, "{:?}", rumbles[0]);
    }

    fn enemy(id: u32, x: f32, y: f32) -> Target {
        Target { id, pos: Vec3::new(x, y, 0.0), radius: 2.0, height: 5.4, character: true, large: false, surface: melee::surface::UPPER_BODY, dashing: false, health: f32::MAX, head: None }
    }
    #[test]
    fn new_vehicle_solver_retains_arena_target_separation() {
        let t=tuning();let mut s=State::new(&t);
        let world=Arena {targets:vec![enemy(1,0.0,20.0)],..Default::default()};
        for _ in 0..300 {
            step(&mut s,&Input {vehicle:1.0,..Default::default()},&t,&world,DT);
            // Check separation throughout contact. Spring/tyre forces can steer past
            // the cylinder; a final y-only bound wrongly requires parking forever.
            assert!(s.pos.truncate().distance(Vec2::new(0.0,20.0))>=t.vehicle_radius+2.0-0.001,"{:?}",s.pos);
        }
    }

    const TAP: Input = Input {
        stick: Vec2::ZERO,
        steer: 0.0,
        pad_stick: Vec2::ZERO,
        look: Vec2::ZERO,
        jump: false,
        jump_held: false,
        vehicle: 0.0,
        brake: 0.0,
        turbo: false,
        drift: false,
        aim: None,
        buttons_down: 1 << button::ATTACK_FAST,
        buttons_hit: 1 << button::ATTACK_FAST,
    };

    #[test]
    fn an_attack_turns_to_its_target_slides_up_to_it_and_hits_it_once() {
        let t = with_melee(tuning());
        let arena = Arena { targets: vec![enemy(7, 6.0, 12.0)], ..Default::default() };
        let mut s = State::new(&t);
        let mut hits = Vec::new();
        step(&mut s, &TAP, &t, &arena, DT);
        for _ in 0..70 {
            step(&mut s, &Input::default(), &t, &arena, DT);
            if s.action.is_some() {
                assert_eq!(s.action.as_ref().unwrap().target, Some(7));
            }
            hits.extend(s.hits.iter().copied());
        }
        // He faced +y; the target is 26.6 degrees to his right.
        let to_target = Vec2::new(6.0, 12.0) - Vec2::new(s.pos.x, s.pos.y);
        assert!(s.facing().dot(to_target.normalize()) > 0.99, "facing {:?}", s.facing());
        // Up to it and no further: the two radii apart.
        assert!((to_target.length() - 4.0).abs() < 0.05, "{} from it", to_target.length());
        assert_eq!(hits.len(), 1, "{hits:?}");
        let hit = hits[0];
        assert_eq!((hit.target, hit.damage, hit.knock_back_speed), (7, 30.0, 24.0));
        assert!((hit.knock_back_angle - 12f32.to_radians()).abs() < 1e-6);
        // The blow goes his way (attack_dir 0), which by then is toward the target.
        assert!(hit.direction.dot(Vec2::new(6.0, 12.0).normalize()) > 0.95, "{:?}", hit.direction);
    }

    #[test]
    fn each_attack_of_a_chain_hits_and_a_target_out_of_range_is_left_alone() {
        let t = with_melee(tuning());
        let arena = Arena { targets: vec![enemy(1, 0.0, 9.0)], ..Default::default() };
        let mut s = State::new(&t);
        let mut hits = Vec::new();
        let run = |s: &mut State, input: &Input, updates: usize, hits: &mut Vec<Hit>| {
            for _ in 0..updates {
                step(s, input, &t, &arena, DT);
                hits.extend(s.hits.iter().copied());
            }
        };
        run(&mut s, &TAP, 1, &mut hits);
        run(&mut s, &Input::default(), 20, &mut hits);
        run(&mut s, &TAP, 1, &mut hits);
        run(&mut s, &Input::default(), 90, &mut hits);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(s.mode, Mode::Stand);

        // 40 away with a range of 20: no target, no turn, no slide, no hit.
        let far = Arena { targets: vec![enemy(1, 20.0, 35.0)], ..Default::default() };
        let mut s = State::new(&t);
        step(&mut s, &TAP, &t, &far, DT);
        for _ in 0..70 {
            step(&mut s, &Input::default(), &t, &far, DT);
            assert!(s.hits.is_empty());
        }
        assert!(s.yaw.abs() < 1e-6 && (s.pos.y - 6.0).abs() < 0.3 && s.pos.x.abs() < 1e-4, "{:?} {}", s.pos, s.yaw);
    }

    #[test]
    fn the_stick_picks_between_two_targets_and_turns_him_when_there_is_none() {
        let t = with_melee(tuning());
        // One ahead and one to the left, both in range; the stick held left picks the left.
        let arena = Arena { targets: vec![enemy(1, 0.0, 10.0), enemy(2, -10.0, 0.0)], ..Default::default() };
        let left = Input { stick: Vec2::NEG_X, ..Default::default() };
        let mut s = State::new(&t);
        step(&mut s, &TAP, &t, &arena, DT);
        step(&mut s, &Input::default(), &t, &arena, DT);
        assert_eq!(s.action.as_ref().unwrap().target, Some(1));
        let mut s = State::new(&t);
        step(&mut s, &Input { stick: Vec2::NEG_X, ..TAP }, &t, &arena, DT);
        step(&mut s, &left, &t, &arena, DT);
        assert_eq!(s.action.as_ref().unwrap().target, Some(2));

        // Nothing to fight: the stick turns him, at the clip's turn speed.
        let empty = Arena::default();
        let mut s = State::new(&t);
        step(&mut s, &Input { stick: Vec2::NEG_X, ..TAP }, &t, &empty, DT);
        for _ in 0..5 {
            step(&mut s, &left, &t, &empty, DT);
        }
        assert!((s.yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "yaw {}", s.yaw);
    }

    /// The ground half of Bumblebee's weapon set as the game's data has it
    /// (`cargo run -p tf2-core --example rules_dump -- WeaponSet`): the idle, the two
    /// turns on the spot, and the eight runs with their `Prev` twins, each 50 ticks and
    /// 16.67 along a root turned its own way; their exit rules and the set's entry rules.
    fn with_weapon_set(mut t: Tuning) -> Tuning {
        use crate::animrules::var;
        use crate::formats::anim::{ExitRule, IncomingRule, Rule};
        use crate::tuning::{RuleClip, RuleSet};
        let float = |variable, logic, float| Rule { variable, logic, float, ..Default::default() };
        let pushed = float(var::L_STICK_AMP, 3, 0.01);
        let centred = float(var::L_STICK_AMP, 4, 0.01);
        let strafing = Rule { variable: var::IS_STRAFING, logic: 0, int: 1, ..Default::default() };
        // Name, the degrees its root is turned to the left, and the angles it is picked in.
        let ways: [(&str, f32, &[(f32, f32)]); 8] = [
            ("Fwd", 0.0, &[(-22.5, 22.5)]),
            ("FwdL", 45.0, &[(22.5, 67.5)]),
            ("Left", 90.0, &[(67.5, 112.5)]),
            ("BackL", 135.0, &[(112.5, 157.5)]),
            ("Back", 180.0, &[(157.5, 1e9), (-1e9, -157.5)]),
            ("BackR", -135.0, &[(-157.5, -112.5)]),
            ("Right", -90.0, &[(-112.5, -67.5)]),
            ("FwdR", -45.0, &[(-67.5, -22.5)]),
        ];
        let within = |&(low, high): &(f32, f32)| {
            let mut rules = vec![pushed];
            if low > -1e8 {
                rules.push(float(var::STRAFE_ANG, 3, low));
            }
            if high < 1e8 {
                rules.push(float(var::STRAFE_ANG, 4, high));
            }
            rules
        };
        let id = |name: &str| crc32(name.as_bytes());
        let to = |to_id, rules| ExitRule { to_set: WEAPON_SET, to_id, rules, ..Default::default() };
        let mut clips = vec![RuleClip {
            id: id("Weapon_Idle"),
            name: "Weapon_Idle".into(),
            ticks: 84.0,
            speed: 1.0,
            crossfade: 0.25,
            looping: true,
            exit_crossfade: -1.0,
            path: vec![Vec3::ZERO; 85],
            turn: vec![0.0; 85],
            exits: vec![
                to(0, vec![pushed]),
                to(id("Weapon_IdleTurnL"), vec![float(var::STRAFE_ANG, 3, 45.0)]),
                to(id("Weapon_IdleTurnR"), vec![float(var::STRAFE_ANG, 4, -45.0)]),
            ],
            ..Default::default()
        }];
        for (name, sign) in [("Weapon_IdleTurnL", 1.0f32), ("Weapon_IdleTurnR", -1.0)] {
            clips.push(RuleClip {
                id: id(name),
                name: name.into(),
                ticks: 20.0,
                speed: 1.0,
                crossfade: 0.1,
                exit_crossfade: -1.0,
                path: vec![Vec3::ZERO; 21],
                turn: (0..=20).map(|tick| sign * 67.5f32.to_radians() * tick as f32 / 20.0).collect(),
                exits: vec![to(0, vec![pushed]), ExitRule { to_set: WEAPON_SET, finish: true, ..Default::default() }],
                ..Default::default()
            });
        }
        let mut incoming = vec![IncomingRule { to_set: WEAPON_SET, to_id: id("Weapon_Idle"), rules: vec![centred, strafing], ..Default::default() }];
        for (way, turned, ranges) in ways {
            let along = Vec2::from_angle(turned.to_radians()).rotate(Vec2::Y);
            for prefix in ["Weapon_Run", "Weapon_PrevRun"] {
                let name = format!("{prefix}{way}");
                let mut exits = vec![to(id("Weapon_Idle"), vec![centred])];
                for (other, _, ranges) in ways.iter().filter(|w| w.0 != way) {
                    exits.extend(ranges.iter().map(|range| to(id(&format!("Weapon_PrevRun{other}")), within(range))));
                }
                clips.push(RuleClip {
                    id: id(&name),
                    name,
                    ticks: 50.0,
                    speed: 1.0,
                    crossfade: 0.3,
                    looping: true,
                    keeps_time: prefix == "Weapon_PrevRun",
                    continue_time: false,
                    exit_crossfade: -1.0,
                    exit_crossfade_to: 0,
                    path: (0..=50).map(|tick| (along * (16.667 * tick as f32 / 50.0)).extend(0.0)).collect(),
                    turn: vec![0.0; 51],
                    branches: Vec::new(),
                    exits,
                    motion: Vec::new(),
                });
            }
            for range in ranges {
                let to_id = id(&format!("Weapon_Run{way}"));
                incoming.push(IncomingRule { to_set: WEAPON_SET, to_id, rules: within(range), ..Default::default() });
            }
        }
        // The air half: three take-offs and their rises, the fall, and the landings out of
        // it: on the spot, or running on one of four ways, each of those left only once its
        // branch point has passed. The same again after a long fall.
        let int = |variable, int| Rule { variable, logic: 0, int, ..Default::default() };
        let not_strafing = int(var::IS_STRAFING, 0);
        let falling = vec![not_strafing, int(var::IN_AIR, 1), float(var::Z_VEL, 5, -0.01)];
        let still = |name: &str, ticks: usize, crossfade: f32, looping: bool, exits: Vec<ExitRule>| RuleClip {
            id: id(name),
            name: name.into(),
            ticks: ticks as f32,
            speed: 1.0,
            crossfade,
            looping,
            exit_crossfade: -1.0,
            path: vec![Vec3::ZERO; ticks + 1],
            turn: vec![0.0; ticks + 1],
            exits,
            ..Default::default()
        };
        let finished = |to_id| ExitRule { to_set: WEAPON_SET, to_id, finish: true, ..Default::default() };
        for side in ["", "_L", "_R"] {
            let rising = format!("Weapon_JumpRising{side}");
            let exits = vec![to(id("Weapon_Fall"), falling.clone()), finished(id(&rising))];
            clips.push(still(&format!("Weapon_JumpTakeOff{side}"), 10, 0.0, false, exits));
            clips.push(still(&rising, 30, 0.1, false, vec![to(id("Weapon_Fall"), falling.clone())]));
        }
        clips.push(still("Weapon_Fall", 56, 0.3, true, Vec::new()));
        let from_fall = |to_id, rules| IncomingRule { from_set: WEAPON_SET, from_id: id("Weapon_Fall"), to_set: WEAPON_SET, to_id, rules, ..Default::default() };
        let mut air = Vec::new();
        for (prefix, long) in [("Weapon_FallBigLand", true), ("Weapon_FallLand", false)] {
            let after = |mut rules: Vec<Rule>| {
                if long {
                    rules.insert(0, float(var::FALL_DUR, 3, 1.0));
                }
                rules
            };
            clips.push(still(prefix, 50, 0.03, false, vec![to(0, vec![pushed]), finished(0)]));
            air.push(from_fall(id(prefix), after(vec![centred, strafing])));
            // Way, the degrees its root is turned, its length and branch point, the angles
            // it is picked in, and the angles it is left in before its branch point.
            let ways: [(&str, f32, usize, f32, &[(f32, f32)], [(u8, f32); 2]); 4] = [
                ("Fwd", 0.0, 26, 20.0, &[(-45.0, 45.0)], [(3, 22.5), (4, -22.5)]),
                ("Back", 180.0, 26, 20.0, &[(-1e9, -100.0), (100.0, 1e9)], [(4, 157.5), (3, -157.5)]),
                ("Left", 90.0, 30, 24.0, &[(45.0, 135.0)], [(4, 67.5), (3, 112.5)]),
                ("Right", -90.0, 30, 24.0, &[(-135.0, -45.0)], [(3, -67.5), (4, -112.5)]),
            ];
            for (way, turned, ticks, branch, ranges, early) in ways {
                let name = format!("{prefix}2Run{way}");
                let along = Vec2::from_angle(turned.to_radians()).rotate(Vec2::Y);
                let mut exits: Vec<ExitRule> = early.iter().map(|&(logic, angle)| to(0, vec![pushed, float(var::STRAFE_ANG, logic, angle)])).collect();
                exits.push(ExitRule { to_set: WEAPON_SET, branch_point: 2, ..Default::default() });
                clips.push(RuleClip {
                    path: (0..=ticks).map(|tick| (along * (7.0 * tick as f32 / ticks as f32)).extend(0.0)).collect(),
                    exit_crossfade: 0.07,
                    exit_crossfade_to: id(&format!("Weapon_Run{way}")),
                    branches: vec![branch],
                    ..still(&name, ticks, 0.0, false, exits)
                });
                for range in ranges {
                    let mut rules = within(range);
                    rules.insert(0, strafing);
                    air.push(from_fall(id(&name), after(rules)));
                }
            }
        }
        for set in ["JumpSet", "FallSet"] {
            air.push(IncomingRule { from_set: id(set), to_set: WEAPON_SET, to_id: id("Weapon_Fall"), ..Default::default() });
        }
        air.push(IncomingRule { to_set: WEAPON_SET, to_id: id("Weapon_Fall"), rules: falling, ..Default::default() });
        let sideways = |low, high| vec![not_strafing, pushed, float(var::L_STICK_DIR, 3, low), float(var::L_STICK_DIR, 4, high)];
        let take_off = |name: &str, rules| IncomingRule { from_set: WEAPON_SET, to_set: WEAPON_SET, to_id: id(name), rules, ..Default::default() };
        air.push(take_off("Weapon_JumpTakeOff_L", sideways(45.0, 135.0)));
        air.push(take_off("Weapon_JumpTakeOff_R", sideways(-135.0, -45.0)));
        air.push(take_off("Weapon_JumpTakeOff", vec![not_strafing]));
        air.extend(incoming);
        t.strafe_speed = 20.0;
        t.weapon_set = RuleSet { name: WEAPON_SET, set_name: "WeaponSet".into(), clips, incoming: air };
        t
    }

    /// Steps at the game's rate and returns, after each update, his ground speed and the way
    /// he is going measured from his aim, in degrees to the left.
    fn strafes(s: &mut State, input: Input, t: &Tuning, updates: usize) -> Vec<(f32, f32)> {
        let arena = Arena::default();
        (0..updates)
            .map(|_| {
                step(s, &input, t, &arena, GAME_DT);
                let v = Vec2::new(s.vel.x, s.vel.y);
                (v.length(), wrapped(yaw_of(v) - s.strafe.aim_yaw).to_degrees())
            })
            .collect()
    }

    fn assert_strafes(got: &[(f32, f32)], recorded: &[(f32, f32)]) {
        assert_eq!(got.len(), recorded.len());
        for (i, (g, r)) in got.iter().zip(recorded).enumerate() {
            let off = wrapped((g.1 - r.1).to_radians()).to_degrees().abs();
            assert!((g.0 - r.0).abs() <= 0.5 && (off <= 2.5 || r.0 < 0.1), "update {i}: {g:.1?} here, {r:?} in the game\n{got:.1?}");
        }
    }

    /// Steps until the named control state is the one running; gives the weapon set's
    /// clips seen on the way, in order, and the number of updates.
    fn until_state(s: &mut State, input: Input, t: &Tuning, state: &str) -> (Vec<String>, usize) {
        let arena = Arena::default();
        let (mut seen, mut updates) = (Vec::<String>::new(), 0);
        while s.control_state(t) != Some(state) {
            step(s, &input, t, &arena, GAME_DT);
            updates += 1;
            assert!(updates < 400, "never reached {state}: {:?}, {seen:?}", s.control_state(t));
            if let Some((_, clip, _)) = s.action_clip(t).filter(|_| s.control_state(t) != Some(state)) {
                if seen.last().map(String::as_str) != Some(clip) {
                    seen.push(clip.to_owned());
                }
            }
        }
        (seen, updates)
    }

    #[test]
    fn a_jump_in_weapon_mode_goes_sideways_facing_the_aim_and_hangs_at_the_top() {
        let t = with_weapon_set(tuning());
        let arena = Arena::default();
        let aiming = Input { aim: Some(Vec2::Y), ..Default::default() };
        let left = Input { stick: Vec2::NEG_X, ..aiming };
        let mut s = State::new(&t);
        run(&mut s, left, &t, 1.0);
        assert_eq!(s.action_clip(&t).map(|c| c.1), Some("Weapon_RunLeft"));
        step(&mut s, &Input { jump: true, jump_held: true, ..left }, &t, &arena, GAME_DT);
        assert_eq!(s.control_state(&t), Some("stateWeaponModeJump"));
        // The stick is to the left of his aim: the take-off to the left, and no longer
        // strafing as the rules see it.
        assert_eq!(s.action_clip(&t).map(|c| c.1), Some("Weapon_JumpTakeOff_L"));
        assert!(!s.strafe.strafing && s.strafe.angle == 0.0);
        assert_eq!(s.mode, Mode::Jump(JumpType::Moving));
        // Launched with the running jump's speed (the body has had one update of gravity).
        let launch = (2.0 * t.gravity * t.jump.run.height).sqrt();
        assert!((s.vel.z - (launch - t.gravity * GAME_DT)).abs() < 0.01, "{}", s.vel.z);

        // Up and over: he never turns to the stick, the factor goes down to the data's
        // quarter near the top, and comes back before he lands.
        let (mut slowest, mut turned, mut airborne) = (1.0f32, 0.0f32, 1);
        let mut seen = vec!["Weapon_JumpTakeOff_L".to_owned()];
        while s.control_state(&t) == Some("stateWeaponModeJump") {
            step(&mut s, &left, &t, &arena, GAME_DT);
            airborne += 1;
            assert!(airborne < 400);
            slowest = slowest.min(s.dilation);
            turned = turned.max(s.yaw.abs());
            let clip = s.action_clip(&t).map(|c| c.1.to_owned()).unwrap();
            if seen.last() != Some(&clip) {
                seen.push(clip);
            }
        }
        assert!(turned < 1e-4, "he turned by {turned}");
        assert_eq!(slowest, t.jump.weapon_dilation);
        // The height is the plain jump's: slowing the time does not change the arc.
        assert!((s.apex_z - s.launch_z - t.jump.run.height).abs() < 0.2, "{}", s.apex_z - s.launch_z);
        // The plain jump is up and down in 1.51 s; the top of this one is stretched by a
        // quarter of a second.
        let plain = 2.0 * launch / t.gravity;
        let longer = airborne as f32 * GAME_DT - plain;
        assert!((0.2..0.35).contains(&longer), "{airborne} updates, {longer} s longer");
        assert_eq!(seen, ["Weapon_JumpTakeOff_L", "Weapon_JumpRising_L", "Weapon_Fall", "Weapon_FallLand2RunLeft"]);

        // Down in weapon mode again, strafing, on the landing that runs on to the left; it
        // is left once its branch point (tick 24 of 30) has passed, not at once.
        assert_eq!(s.mode, Mode::Strafe);
        assert!(s.strafe.strafing && s.dilation == 1.0);
        let mut landing = 1;
        while s.action_clip(&t).map(|c| c.1) == Some("Weapon_FallLand2RunLeft") {
            step(&mut s, &left, &t, &arena, GAME_DT);
            landing += 1;
            assert!(landing < 100);
        }
        let ticks = landing as f32 * GAME_DT * 60.0;
        assert!((24.0..28.0).contains(&ticks), "left after {ticks} ticks");
        assert_eq!(s.action_clip(&t).map(|c| c.1), Some("Weapon_RunLeft"));
        run(&mut s, left, &t, 0.3);
        let v = Vec2::new(s.vel.x, s.vel.y);
        assert!((v.length() - 20.0).abs() < 0.1 && (v.x + 20.0).abs() < 0.2, "{v:?}");
    }

    #[test]
    fn a_standing_jump_in_weapon_mode_lands_on_the_spot() {
        let t = with_weapon_set(tuning());
        let arena = Arena::default();
        let aiming = Input { aim: Some(Vec2::Y), ..Default::default() };
        let mut s = State::new(&t);
        run(&mut s, aiming, &t, 0.5);
        step(&mut s, &Input { jump: true, jump_held: true, ..aiming }, &t, &arena, GAME_DT);
        assert_eq!(s.mode, Mode::Jump(JumpType::Standing));
        assert_eq!(s.action_clip(&t).map(|c| c.1), Some("Weapon_JumpTakeOff"));
        let (seen, _) = until_state(&mut s, aiming, &t, "stateWeaponMode");
        assert_eq!(seen, ["Weapon_JumpTakeOff", "Weapon_JumpRising", "Weapon_Fall"]);
        // The hang stretches the way down from 0.83 s to 1.06 s, which is past the second
        // the set's rules count as a long fall, so he lands on the spot with the big
        // landing. Only two updates past: no recording says the original lands so.
        assert!((1.0..1.1).contains(&s.fall_time), "{}", s.fall_time);
        assert_eq!(s.action_clip(&t).map(|c| c.1), Some("Weapon_FallBigLand"));
        // It plays out (50 ticks), then the idle.
        run(&mut s, aiming, &t, 0.7);
        assert_eq!(s.action_clip(&t).map(|c| c.1), Some("Weapon_FallBigLand"));
        run(&mut s, aiming, &t, 0.3);
        assert_eq!(s.action_clip(&t).map(|c| c.1), Some("Weapon_Idle"));
    }

    #[test]
    fn the_firing_state_runs_on_top_of_weapon_mode_while_the_fire_button_is_down() {
        let t = with_weapon_set(tuning());
        let arena = Arena::default();
        let aiming = Input { aim: Some(Vec2::Y), ..Default::default() };
        let fire = Input { buttons_down: 1 << button::WEAPON_FIRE, ..aiming };
        let mut s = State::new(&t);
        // Out of weapon mode the button does nothing.
        run(&mut s, Input { buttons_down: fire.buttons_down, ..Default::default() }, &t, 0.2);
        assert!(!s.firing && s.overlay_state(&t).is_none());
        run(&mut s, aiming, &t, 0.5);
        assert_eq!(s.control_state(&t), Some("stateWeaponMode"));
        // The update the button goes down asks for the state; the next enters it and gives
        // the weapon the held trigger. Weapon mode goes on under it.
        step(&mut s, &fire, &t, &arena, GAME_DT);
        assert!(!s.firing && s.overlay_state(&t).is_none());
        step(&mut s, &fire, &t, &arena, GAME_DT);
        assert!(s.firing);
        assert_eq!(s.overlay_state(&t), Some("stateWeaponModeAttack"));
        assert_eq!(s.control_state(&t), Some("stateWeaponMode"));
        run(&mut s, fire, &t, 0.5);
        assert!(s.firing && s.mode == Mode::Strafe);
        // Let go: that update the state passes the release on and asks to end; the next
        // update it is gone.
        step(&mut s, &aiming, &t, &arena, GAME_DT);
        assert!(!s.firing);
        assert_eq!(s.overlay_state(&t), Some("stateWeaponModeAttack"));
        step(&mut s, &aiming, &t, &arena, GAME_DT);
        assert!(s.overlay_state(&t).is_none());
        // A jump while firing: the weapon state's exit ends the firing state, and the jump
        // state takes it up again while the button stays down.
        run(&mut s, fire, &t, 0.2);
        assert!(s.firing);
        step(&mut s, &Input { jump: true, jump_held: true, ..fire }, &t, &arena, GAME_DT);
        assert_eq!(s.control_state(&t), Some("stateWeaponModeJump"));
        assert!(s.overlay_state(&t).is_none());
        run(&mut s, fire, &t, 0.2);
        assert!(s.firing && s.control_state(&t) == Some("stateWeaponModeJump"));
        // Weapon mode let go while firing: no firing left.
        run(&mut s, Input { buttons_down: fire.buttons_down, ..Default::default() }, &t, 0.2);
        assert!(!s.firing && s.overlay_state(&t).is_none());
    }

    #[test]
    fn the_cars_gun_is_a_firing_state_on_top_of_the_drive_state() {
        let t = tuning();
        let arena = Arena::default();
        let driving = Input { vehicle: 1.0, ..Default::default() };
        let fire = Input { buttons_down: 1 << button::ATTACK_VEHICLE, ..driving };
        let mut s = State::new(&t);
        // On foot the button does not fire.
        run(&mut s, Input { buttons_down: fire.buttons_down, ..Default::default() }, &t, 0.2);
        assert!(!s.firing && !s.car_mode);
        // The control mode is the car's from the first update of the change.
        step(&mut s, &driving, &t, &arena, DT);
        step(&mut s, &driving, &t, &arena, DT);
        assert!(s.car_mode);
        assert_eq!(s.control_state(&t), Some("stateDriveEnterA"));
        run(&mut s, driving, &t, 2.0);
        assert_eq!(s.control_state(&t), Some("stateDrive"));
        // The button down asks for the state, the next update enters it.
        step(&mut s, &fire, &t, &arena, DT);
        assert!(!s.firing);
        step(&mut s, &fire, &t, &arena, DT);
        assert!(s.firing && s.is_vehicle());
        assert_eq!(s.overlay_state(&t), Some("stateWeaponModeVehicleAttack"));
        assert_eq!(s.control_state(&t), Some("stateDrive"));
        run(&mut s, fire, &t, 0.5);
        assert!(s.firing && s.mode == Mode::Drive);
        // Let go: it ends, and it is not taken up again with the button up (leaving the
        // state resets the connection it came by).
        step(&mut s, &driving, &t, &arena, DT);
        assert!(!s.firing);
        step(&mut s, &driving, &t, &arena, DT);
        assert!(s.overlay_state(&t).is_none());
        run(&mut s, driving, &t, 0.3);
        assert!(s.overlay_state(&t).is_none() && !s.firing);
        // Out of the car the control mode is his own again as the unfold starts.
        run(&mut s, Input::default(), &t, 0.2);
        assert!(!s.car_mode);
    }

    /// His special as the data has it: the script on his own object sets off a shockwave
    /// that stuns for 2.5 s and does no damage, a sphere of 15 from half size over 0.25 s.
    fn with_special(mut t: Tuning) -> Tuning {
        use crate::formats::scene::ExplosionDef;
        const SCRIPT: u32 = crc32(b"specialAttack");
        t.special = crate::tuning::SpecialTuning {
            cooldown: 23.5,
            duration: 5.0,
            kind: -1,
            icon: 0,
            script: SCRIPT,
            vehicle_script: crc32(b"specialAttackVehicle"),
            vehicle_anim: 0,
        };
        t.actions.spawners = vec![melee::ClipBlast {
            ssd: Vec::new(),
            name: SCRIPT,
            node: 0,
            attach: true,
            explosion: ExplosionDef {
                radius: 15.0,
                start_scale: 0.5,
                time: 0.25,
                delay: 0.0,
                damage: 0.0,
                min_damage: 0.0,
                fall_off: -1.0,
                pound_override: false,
                damage_player: true,
                can_damage_self: false,
                damage_flags: 0,
                knock_back_speed: 0.0,
                knock_back_angle: 0.0,
                camera_shake: 0,
                stun_time: 2.5,
                slow_time: 0.0,
                dot_time: 0.0,
                heat: 0.0,
            },
            rumble: None,
            centres: vec![Vec3::new(0.0, 0.0, 2.7)],
        }];
        t
    }

    /// A type-0 special (Optimus) through the real start and timer: a 100 hit loses 50 while it
    /// runs and 100 after it ends. [data] duration 10, shield 0.5 [game 0072afd0, 00721b90]
    #[test]
    fn optimus_special_halves_a_ground_hit_for_its_ten_seconds_then_it_is_full() {
        let mut t = tuning();
        t.base_health = 1000.0;
        t.special = crate::tuning::SpecialTuning { cooldown: 29.4, duration: 10.0, kind: 0, icon: 0, script: 0, vehicle_script: 0, vehicle_anim: 0 };
        let mut s = State::new(&t);
        s.health = 400.0;
        let hit = || crate::damage::Packet { amount: 100.0, ..Default::default() };
        start_special(&mut s, &t);
        assert!(s.special.as_ref().is_some_and(|a| a.is_on()));
        let before = s.health;
        s.receive_damage(hit(), &t);
        assert!((before - s.health - 50.0).abs() < 1e-3, "during: lost {}", before - s.health);
        run(&mut s, Input::default(), &t, 10.5);
        assert!(s.special.as_ref().is_some_and(|a| !a.is_on()), "the timer ends at ten seconds");
        let before = s.health;
        s.receive_damage(hit(), &t);
        assert!((before - s.health - 100.0).abs() < 1e-3, "after: lost {}", before - s.health);
    }

    /// The ground shield is the type-0 special's whatever the form: the flight branch of 00719830 is
    /// chosen by the active special's type (`ground_shield_active` has no form test), so an Optimus
    /// shield holds in the flying form too [game 00719830 l.104-129, 00721b90].
    #[test]
    fn the_ground_shield_holds_in_the_flying_form_too() {
        let mut t = tuning();
        t.base_health = 1000.0;
        t.special = crate::tuning::SpecialTuning { cooldown: 29.4, duration: 10.0, kind: 0, icon: 0, script: 0, vehicle_script: 0, vehicle_anim: 0 };
        let mut s = State::new(&t);
        s.health = 400.0;
        start_special(&mut s, &t);
        s.mode = Mode::Fly;
        let before = s.health;
        s.receive_damage(crate::damage::Packet { amount: 100.0, ..Default::default() }, &t);
        assert!((before - s.health - 50.0).abs() < 1e-3, "flying: lost {}", before - s.health);
    }

    /// Damage bit 27 with no type-1 special still runs the flight block: the amount is multiplied by
    /// `optimusFlightSpecialDamageMultiplier` and the health stops at 1 [game 00719830 l.128-150, 00719830 l.216-222].
    #[test]
    fn damage_bit_27_forces_the_flight_shield_without_a_special() {
        let mut t = tuning();
        t.base_health = 1000.0;
        let mut s = State::new(&t);
        s.health = 400.0;
        s.receive_damage(crate::damage::Packet { amount: 100.0, flags: crate::damage::flags::SPIRIT_LINK, ..Default::default() }, &t);
        assert!((400.0 - s.health - 50.0).abs() < 1e-3, "lost {}", 400.0 - s.health);
    }

    /// The flyer's advance-transform clock (`mode+0x4a4`): zeroed by FlyMode's enter, counted by
    /// each update [game 00864d60 l.210, 00864320 step 20].
    #[test]
    fn the_flyers_advance_transform_clock_counts_from_its_enter() {
        let t = tuning();
        let arena = Arena::default();
        let mut s = State::new(&t);
        set_mode(&mut s, &Input::default(), &t, Mode::Fly);
        s.mode = Mode::Fly;
        assert_eq!(s.fly_clock, 0.0);
        for _ in 0..10 {
            step_fly(&mut s, &Input::default(), &t, &arena, GAME_UPDATE);
        }
        assert!((s.fly_clock - 10.0 * GAME_UPDATE).abs() < 1e-4, "clock {}", s.fly_clock);
        set_mode(&mut s, &Input::default(), &t, Mode::Drive);
        set_mode(&mut s, &Input::default(), &t, Mode::Fly);
        s.mode = Mode::Fly;
        assert_eq!(s.fly_clock, 0.0, "a new enter zeroes the clock");
    }

    /// The flyer's boundary is tested at its camera target point, not its body: the root's x and y
    /// and the lower of the root's z and the holder's bottom [game 0071a460; `flycam::flyer_point`].
    /// Upright, the bottom is the body's z less the vehicle avatar's half height, so the point is lower
    /// than the body. `step_fly` sets it before the boundary is tested.
    #[test]
    fn the_flyers_boundary_point_is_its_camera_target_point() {
        let t = tuning();
        let arena = Arena::default();
        let mut s = State::new(&t);
        set_mode(&mut s, &Input::default(), &t, Mode::Fly);
        s.mode = Mode::Fly;
        s.pos = Vec3::new(1.0, 2.0, 30.0);
        s.fly.as_mut().unwrap().pos = s.pos;
        let up_z = (s.fly.as_ref().unwrap().rotation() * glam::Vec3::Z).z;
        let half = t.vehicle_height * 0.5;
        let want = crate::flycam::flyer_point(s.pos, s.pos.z, up_z, half);
        assert!(up_z > 0.99 && half > 0.0, "upright, with a real avatar height");
        assert!((want.z - (30.0 - half * up_z)).abs() < 1e-4, "the holder's bottom: {}", want.z);
        assert!((want.x - 1.0).abs() < 1e-6 && (want.y - 2.0).abs() < 1e-6, "the root's x and y");
        step_fly(&mut s, &Input::default(), &t, &arena, GAME_UPDATE);
        let got = s.fly.as_ref().unwrap().boundary_point.expect("step_fly sets the point");
        assert!((got - want).length() < 1e-4, "point {got:?} vs {want:?}");
        assert!(got.z < 30.0, "lower than the body");
    }

    /// The drop ray's hit normal is the flyer's ground normal (`Fly::ground_normal_z`, mode+0x354):
    /// a slope gives its z, and a ray that finds nothing keeps the flat value [game 0086b230;
    /// stand-in for the miss].
    #[test]
    fn the_flyers_drop_ray_gives_its_ground_normal() {
        let t = tuning();
        let mut s = State::new(&t);
        set_mode(&mut s, &Input::default(), &t, Mode::Fly);
        s.mode = Mode::Fly;
        s.pos = Vec3::new(0.0, 0.0, 10.0);
        s.fly.as_mut().unwrap().pos = s.pos;
        // The plane z = 0.5 y: its normal is (0, -0.5, 1) normalised.
        step_fly(&mut s, &Input::default(), &t, &Slope(0.5), GAME_UPDATE);
        let want = Vec3::new(0.0, -0.5, 1.0).normalize().z;
        let got = s.fly.as_ref().unwrap().ground_normal_z;
        assert!((got - want).abs() < 1e-4, "slope normal z {got} vs {want}");
        s.pos = Vec3::new(0.0, 0.0, 10.0);
        s.fly.as_mut().unwrap().pos = s.pos;
        step_fly(&mut s, &Input::default(), &t, &Arena::default(), GAME_UPDATE);
        assert_eq!(s.fly.as_ref().unwrap().ground_normal_z, 1.0, "a miss keeps the flat value");
    }

    /// A stunt ends on the all-zero traction vector of its clip, not on the 0.2 s rule: a nonzero vector
    /// keeps it, and the zero one zeroes pitch and roll for an up loop and sets the velocity along the
    /// nose at the cap [game 00866900].
    #[test]
    fn a_zero_traction_vector_ends_a_stunt_in_flight() {
        let mut t = tuning();
        t.forms.fly.max_speed = 60.0;
        let arena = Arena::default();
        let mut s = State::new(&t);
        set_mode(&mut s, &Input::default(), &t, Mode::Fly);
        s.mode = Mode::Fly;
        {
            let fly = s.fly.as_mut().unwrap();
            fly.stunt = crate::flight::Stunt::Up;
            fly.pitch = 0.5;
            fly.roll = 0.3;
        }
        clip_traction(&mut s, &t, Vec3::new(1.0, 1.0, 1.0));
        step_fly(&mut s, &Input::default(), &t, &arena, GAME_UPDATE);
        assert_eq!(s.fly.as_ref().unwrap().stunt, crate::flight::Stunt::Up, "a nonzero vector only stores");
        assert_eq!(s.fly.as_ref().unwrap().control, Vec3::ONE);
        clip_traction(&mut s, &t, Vec3::ZERO);
        let fly = s.fly.as_ref().unwrap();
        assert_eq!(fly.stunt, crate::flight::Stunt::None, "the zero vector ends it");
        assert_eq!(fly.control, Vec3::ZERO);
        assert_eq!((fly.pitch, fly.roll), (0.0, 0.0), "an up loop levels pitch and roll");
        assert!((fly.vel.length() - 60.0).abs() < 1e-3, "speed {} at the cap", fly.vel.length());
        step_fly(&mut s, &Input::default(), &t, &arena, GAME_UPDATE);
    }

    /// A jet's stunt runs on its clip: the roll's [1, 1, 1] first key pins the jet to the clip's
    /// root (Starscream's `Vehicle_RollLeft` slides 50 m to his left in a second), the stick does
    /// nothing meanwhile, and the [0, 0, 0] last key ends it. [game 00866900, 006db490] [data]
    #[test]
    fn a_jets_roll_follows_its_clip_and_ends_on_its_last_key() {
        let dir = std::path::Path::new("C:/Games2");
        if !dir.exists() {
            return;
        }
        let mut t = crate::tuning::load(dir, "Starscream").unwrap();
        let data = crate::character::load(dir, "starscream").unwrap();
        t.actions = crate::tuning::Actions::from_animations(&data.animations, &data.robot, &mut t.missing);
        t.rule_sets = crate::tuning::RuleSet::all(&data.animations);
        let arena = Arena::default();
        let mut s = State::new(&t);
        set_mode(&mut s, &Input::default(), &t, Mode::Fly);
        s.mode = Mode::Fly;
        s.pos.z = 100.0;
        s.fly.as_mut().unwrap().pos.z = 100.0;
        s.fly.as_mut().unwrap().stunt = crate::flight::Stunt::Left;
        let roll = crc32(b"Vehicle_RollLeft");
        let Some(state) = t.control.states.iter().position(|d| d.anim == roll) else { return };
        assert!(start_clip(&mut s, &Input::default(), &t, state, roll));
        apply_clip_effects(&mut s, &Input::default(), &t, &arena);
        assert_eq!(s.fly.as_ref().unwrap().control, Vec3::ONE, "the first key sets it");
        let start = s.pos;
        let mut ended = false;
        for _ in 0..40 {
            // The stick held hard right: it must not steer while the clip drives.
            let input = Input { stick: Vec2::new(1.0, 0.0), pad_stick: Vec2::new(1.0, 0.0), ..Default::default() };
            play_action(&mut s, &t, &Pad::default(), GAME_UPDATE);
            step_fly(&mut s, &input, &t, &arena, GAME_UPDATE);
            apply_clip_effects(&mut s, &input, &t, &arena);
            if s.fly.as_ref().unwrap().stunt == crate::flight::Stunt::None {
                ended = true;
                break;
            }
        }
        assert!(ended, "the last key ends the stunt");
        // Heading 0 faces +y: his left is -x. The clip slides him about 50 m that way.
        let moved = s.pos - start;
        assert!(moved.x < -40.0, "slid {moved}");
    }

    /// The special's end sends Untrigger to both self-trigger names once, in the step its
    /// duration runs out, whatever form he is in [game 0072a550 line 201, 0072b980 lines 9-22];
    /// OptimusFlight's mark is on only while its special runs [game 00730e90].
    #[test]
    fn the_specials_end_untriggers_both_self_trigger_names_once() {
        let mut t = with_special(tuning());
        t.base_health = 1000.0;
        let mut s = State::new(&t);
        s.health = 1000.0;
        start_special(&mut s, &t);
        assert_eq!(s.triggers, [crc32(b"specialAttack")]);
        let world = Arena::default();
        let mut sent = Vec::new();
        let mut steps = 0;
        while (steps as f32) * DT < 6.0 {
            step(&mut s, &Input::default(), &t, &world, DT);
            if !s.untriggers.is_empty() {
                sent.push((steps, s.untriggers.clone()));
            }
            steps += 1;
        }
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0].1, [crc32(b"specialAttack"), crc32(b"specialAttackVehicle")]);
        // The step that takes the 5 s clock to zero or below, within float rounding.
        let due = 5.0f32 / DT;
        assert!((sent[0].0 as f32 + 1.0 - due).abs() <= 1.0, "{} vs {due}", sent[0].0);

        let mut t = tuning();
        t.base_health = 1000.0;
        t.special = crate::tuning::SpecialTuning { cooldown: 29.4, duration: 10.0, kind: 1, ..Default::default() };
        let mut s = State::new(&t);
        s.health = 1000.0;
        start_special(&mut s, &t);
        assert!(s.transfer_source && s.untriggers.is_empty());
        run(&mut s, Input::default(), &t, 10.5);
        assert!(!s.transfer_source);
    }

    #[test]
    fn the_special_sets_off_his_script_stuns_what_is_near_and_then_waits_out_its_cooldown() {
        let t = with_special(with_weapon_set(tuning()));
        let arena = Arena { targets: vec![enemy(1, 0.0, 10.0), enemy(2, 0.0, 40.0)], ..Default::default() };
        let press = Input { buttons_down: 1 << button::ATTACK_SPECIAL, buttons_hit: 1 << button::ATTACK_SPECIAL, ..Default::default() };
        let mut s = State::new(&t);
        step(&mut s, &Input::default(), &t, &arena, DT);
        // The press asks for the state; the next update enters it, which starts the
        // ability; the one after, the state is gone again.
        step(&mut s, &press, &t, &arena, DT);
        assert!(s.triggers.is_empty() && s.special_cooldown == 0.0);
        step(&mut s, &Input::default(), &t, &arena, DT);
        assert_eq!(s.triggers, [crc32(b"specialAttack")]);
        assert_eq!(s.overlay_state(&t), Some("stateAttackSpecial"));
        assert!((s.special_cooldown - 23.5).abs() < 1e-4 && (s.special_left - 5.0).abs() < 1e-4);
        assert_eq!(s.special_starts, 1);
        assert_eq!(s.control_state(&t), Some("stateIdle"), "he goes on with what he was doing");
        let mut hits: Vec<melee::Hit> = s.hits.clone();
        step(&mut s, &Input::default(), &t, &arena, DT);
        assert!(s.overlay_state(&t).is_none() && s.triggers.is_empty());
        hits.extend(s.hits.iter().copied());
        for _ in 0..20 {
            step(&mut s, &Input::default(), &t, &arena, DT);
            hits.extend(s.hits.iter().copied());
        }
        // The one 10 away is stunned once, with no damage and no throw; the one 40 away
        // is out of the sphere's 15.
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!((hits[0].target, hits[0].damage, hits[0].stun_time, hits[0].blast), (1, 0.0, 2.5, true));
        assert!(!hits[0].throws() && s.blasts.is_empty());
        // Pressed again inside the cooldown: nothing.
        step(&mut s, &press, &t, &arena, DT);
        for _ in 0..5 {
            step(&mut s, &Input::default(), &t, &arena, DT);
            assert!(s.triggers.is_empty() && s.overlay_state(&t).is_none());
        }
        // After it: again. The ability's own five seconds are long over.
        run(&mut s, Input::default(), &t, 23.5);
        assert_eq!((s.special_cooldown, s.special_left), (0.0, 0.0));
        step(&mut s, &press, &t, &arena, DT);
        step(&mut s, &Input::default(), &t, &arena, DT);
        assert_eq!(s.triggers, [crc32(b"specialAttack")]);
    }

    #[test]
    fn the_special_runs_on_top_of_the_firing_state_without_ending_it() {
        let t = with_special(with_weapon_set(tuning()));
        let arena = Arena::default();
        let fire = Input { aim: Some(Vec2::Y), buttons_down: 1 << button::WEAPON_FIRE, ..Default::default() };
        let press = Input { buttons_down: fire.buttons_down | 1 << button::ATTACK_SPECIAL, buttons_hit: 1 << button::ATTACK_SPECIAL, ..fire };
        let mut s = State::new(&t);
        run(&mut s, fire, &t, 1.0);
        assert!(s.firing && s.overlay_state(&t) == Some("stateWeaponModeAttack"));
        step(&mut s, &press, &t, &arena, DT);
        assert!(s.triggers.is_empty());
        step(&mut s, &fire, &t, &arena, DT);
        assert_eq!(s.triggers, [crc32(b"specialAttack")]);
        assert!(s.firing && s.overlay_state(&t) == Some("stateWeaponModeAttack"));
        for _ in 0..10 {
            step(&mut s, &fire, &t, &arena, DT);
            assert!(s.firing && s.triggers.is_empty());
        }
        assert!(s.runner.nested.is_none());
    }

    #[test]
    fn aiming_in_a_plain_jump_starts_the_weapon_fall_and_letting_go_ends_it() {
        let t = with_weapon_set(tuning());
        let arena = Arena::default();
        let mut s = State::new(&t);
        step(&mut s, &Input { jump: true, jump_held: true, ..Default::default() }, &t, &arena, GAME_DT);
        run(&mut s, Input::default(), &t, 0.3);
        assert_eq!(s.control_state(&t), Some("stateJump"));
        assert!(s.action_clip(&t).is_none() && !s.weapon_air());
        // The aim is to his left: the weapon set comes in on its fall clip, rising or not,
        // and the jump mode has him face the aim from the first update.
        let aiming = Input { aim: Some(Vec2::NEG_X), ..Default::default() };
        step(&mut s, &aiming, &t, &arena, GAME_DT);
        assert_eq!(s.control_state(&t), Some("stateWeaponModeFall"));
        assert_eq!(s.action_clip(&t).map(|c| c.1), Some("Weapon_Fall"));
        assert!(s.vel.z > 0.0 && (s.yaw.to_degrees() - 90.0).abs() < 0.01, "{}", s.yaw.to_degrees());
        // The trigger let go: the plain fall, the weapon set's clip gone, and the time
        // factor on its way back to 1.
        run(&mut s, aiming, &t, 0.4);
        assert!(s.dilation < 1.0);
        step(&mut s, &Input::default(), &t, &arena, GAME_DT);
        assert_eq!(s.control_state(&t), Some("stateFall"));
        assert!(s.action_clip(&t).is_none() && !s.weapon_air());
        run(&mut s, Input::default(), &t, 0.3);
        assert_eq!(s.dilation, 1.0);
    }

    #[test]
    fn strafing_from_a_stand_starts_ahead_for_an_update_as_recorded() {
        let t = with_weapon_set(tuning());
        let aiming = Input { aim: Some(Vec2::Y), ..Default::default() };
        let mut s = State::new(&t);
        run(&mut s, aiming, &t, 0.5);
        assert_eq!(s.mode, Mode::Strafe);
        assert_eq!(s.action_clip(&t).map(|c| c.1), Some("Weapon_Idle"));
        // Run 1, t 490.31 and 493.86 (the stick to the left) with 494.37 for the end: one
        // update of the forward clip, then the left one fading in over it.
        let left = Input { stick: Vec2::NEG_X, ..aiming };
        let got = strafes(&mut s, left, &t, 12);
        let recorded = [
            (2.13, 0.0),
            (4.39, 29.4),
            (6.64, 40.5),
            (8.71, 48.0),
            (10.57, 54.6),
            (12.32, 61.1),
            (14.01, 67.7),
            (15.61, 74.1),
            (17.38, 80.9),
            (19.34, 88.0),
            (20.0, 90.0),
            (20.0, 90.0),
        ];
        assert_strafes(&got, &recorded);
        assert!(s.yaw.abs() < 1e-4, "he keeps facing his aim: {}", s.yaw);
        assert_eq!(s.action_clip(&t).map(|c| c.1), Some("Weapon_PrevRunLeft"));
        // t 494.63: the stick through the middle to the other side. The idle fades in for
        // two updates, the forward clip for one, then the right one.
        let mut got = strafes(&mut s, aiming, &t, 2);
        got.extend(strafes(&mut s, Input { stick: Vec2::X, ..aiming }, &t, 4));
        assert_strafes(&got, &[(17.43, 90.0), (14.87, 90.0), (11.16, 78.8), (6.04, 50.5), (5.09, -5.5), (7.46, -38.7)]);
    }

    #[test]
    fn going_into_weapon_mode_at_a_run_carries_on_for_two_updates_as_recorded() {
        let t = with_weapon_set(tuning());
        let mut s = State::new(&t);
        run(&mut s, FORWARD, &t, 1.0);
        assert!((s.speed - 24.0).abs() < 0.1);
        // Run 1, t 489.79: running, the aim 88.6 degrees to his left, the stick as it was.
        let aim = Vec2::from_angle(88.6f32.to_radians()).rotate(Vec2::Y);
        let got = strafes(&mut s, Input { aim: Some(aim), ..FORWARD }, &t, 4);
        assert_eq!(s.mode, Mode::Strafe);
        assert_strafes(&got, &[(23.57, 0.0), (23.13, 0.0), (20.34, -6.2), (17.94, -14.1)]);
        assert!((s.yaw - 88.6f32.to_radians()).abs() < 1e-3, "he turned to the aim at once: {}", s.yaw);
    }

    #[test]
    fn standing_he_twists_to_the_aim_and_turns_on_the_spot_past_45_degrees() {
        let t = with_weapon_set(tuning());
        let ahead = Input { aim: Some(Vec2::Y), ..Default::default() };
        let mut s = State::new(&t);
        run(&mut s, ahead, &t, 0.5);
        // The aim 30 degrees to the left: his body stays, the spine twists by it.
        let aim = |degrees: f32| Input { aim: Some(Vec2::from_angle(degrees.to_radians()).rotate(Vec2::Y)), ..Default::default() };
        run(&mut s, aim(30.0), &t, 0.5);
        assert!(s.yaw.abs() < 1e-4 && (s.strafe.twist.to_degrees() - 30.0).abs() < 0.01, "{} {}", s.yaw, s.strafe.twist);
        assert!((s.strafe.angle - 30.0).abs() < 0.01);
        // 60 degrees: the turning clip takes him 67.5 round, and the twist comes back.
        let arena = Arena::default();
        step(&mut s, &aim(60.0), &t, &arena, GAME_DT);
        assert!((s.yaw.to_degrees() - 14.5).abs() < 0.01, "the body gives what is past 45.5 at once: {}", s.yaw.to_degrees());
        step(&mut s, &aim(60.0), &t, &arena, GAME_DT);
        assert_eq!(s.action_clip(&t).map(|c| c.1), Some("Weapon_IdleTurnL"));
        run(&mut s, aim(60.0), &t, 1.0);
        assert_eq!(s.action_clip(&t).map(|c| c.1), Some("Weapon_Idle"));
        // How much of the clip's 67.5 he gets while it fades in is our blend, not checked.
        let turned = s.yaw.to_degrees();
        assert!(turned > 60.0 && turned < 83.0, "{turned}");
        assert!((s.strafe.twist.to_degrees() - (60.0 - turned)).abs() < 0.01, "{}", s.strafe.twist.to_degrees());
        // Letting the aim go leaves the mode and takes the twist off.
        run(&mut s, Input::default(), &t, 0.2);
        assert_eq!(s.mode, Mode::Stand);
        assert!(s.strafe.twist == 0.0 && !s.strafe.strafing);
    }
}

/// A melee weapon's own box as a hitting body (Optimus's left arm blade).
#[cfg(test)]
mod blade_hit_tests {
    use super::*;

    fn optimus() -> Tuning {
        let dir = std::path::Path::new("C:/Games2");
        let mut t = crate::tuning::load(dir, "Optimus").unwrap();
        let data = crate::character::load(dir, "optimus").unwrap();
        t.locomotion = crate::tuning::Locomotion::from_animations(&data.animations, &mut t.missing);
        t.actions = crate::tuning::Actions::from_animations(&data.animations, &data.robot, &mut t.missing)
            .with_vehicle(data.vehicle.as_ref())
            .with_melee(&data.melee_weapons);
        t.weapon_set = crate::tuning::RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
        t.rule_sets = crate::tuning::RuleSet::all(&data.animations);
        t
    }

    /// Two taps of the melee button, then his state at the first update of `AttackFast_2`
    /// with its left blade switched on.
    fn in_swing_two(t: &Tuning) -> State {
        let arena = Arena::default();
        let mut s = State::new(t);
        let bit = 1u64 << button::ATTACK_FAST;
        for i in 0..200 {
            let tap = [5, 20].contains(&i);
            let input = Input { buttons_down: if tap { bit } else { 0 }, buttons_hit: if tap { bit } else { 0 }, ..Default::default() };
            step(&mut s, &input, t, &arena, 0.032);
            if s.attack_bodies(t).iter().any(|(node, _, _)| *node == crc32(b"WeaponAttachment_Arm_L")) {
                let (_, id, tick) = s.action_clip(t).unwrap();
                assert_eq!(id, "AttackFast_2", "at {tick}");
                assert!((20.0..=28.0).contains(&tick), "{tick}");
                return s;
            }
        }
        panic!("the left blade never went live: {:?}", s.action_clip(t));
    }

    #[test]
    fn optimus_hits_with_his_left_blade_once_per_attack_state() {
        let t = optimus();
        let mut s = in_swing_two(&t);
        let arm_l = crc32(b"WeaponAttachment_Arm_L");
        assert_eq!(s.melee.len(), 2);
        assert_eq!(s.melee[1].node, arm_l);
        // Here the blade is folding (its object plays `Trans_W2R`, from swing 1's t=32 event, at
        // tick 28.8) and its box, at half (1.27, 1, 0.33), lies inside his live Hand_L sphere
        // (radius 2.34, 0.6 from the box's centre), so the limbs' bodies are taken out of this
        // clip for the touch: only the blade can hit.
        assert_eq!(s.melee[1].object.map(|o| o.id), Some(crc32(b"Trans_W2R")));
        let mut t = t.clone();
        t.actions.clips.get_mut(&crc32(b"AttackFast_2")).unwrap().bodies.clear();
        let bodies = s.attack_bodies(&t);
        assert_eq!(bodies.len(), 1, "{bodies:?}");
        let (_, _, blade) = bodies[0];
        let Shape::Cuboid { half } = blade.shape else { panic!("{:?}", blade.shape) };
        let p = blade.at.transform_point3(Vec3::new(half[0] * 0.8, 0.0, 0.0));
        let target = Target { id: 7, pos: p - Vec3::Z * 0.1, radius: 0.1, height: 0.2, character: true, large: false, surface: melee::surface::UPPER_BODY, dashing: false, health: f32::MAX, head: None };
        let arena = Arena { targets: vec![target], ..Default::default() };
        land_hits(&mut s, &t, &arena);
        assert_eq!(s.hits.len(), 1);
        let hit = &s.hits[0];
        assert_eq!(hit.body, arm_l);
        let attack = t.actions.named("AttackFast_2").unwrap().attack.unwrap();
        let expect = attack.damage_for(true, false) * t.melee.upgrade_scale(hit.clip, s.melee_levels);
        assert!((hit.damage - expect).abs() < 1e-4, "{} {expect}", hit.damage);
        s.hits.clear();
        land_hits(&mut s, &t, &arena);
        assert!(s.hits.is_empty(), "hit twice in one attack state");
    }
}

/// The animation rule variables the roster's data reads, answered by `rule_value`
/// (2026-10-20): each test loads the installed character and shows the rule choosing the clip.
#[cfg(test)]
mod rule_variable_tests {
    use super::*;

    fn installed(pack: &str, name: &str) -> Option<Tuning> {
        let dir = std::path::Path::new("C:/Games2");
        if !dir.exists() {
            return None;
        }
        let mut t = crate::tuning::load(dir, name).unwrap();
        let data = crate::character::load(dir, pack).unwrap();
        t.locomotion = crate::tuning::Locomotion::from_animations(&data.animations, &mut t.missing);
        t.actions = crate::tuning::Actions::from_animations(&data.animations, &data.robot, &mut t.missing).with_vehicle(data.vehicle.as_ref());
        t.weapon_set = crate::tuning::RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
        t.rule_sets = crate::tuning::RuleSet::all(&data.animations);
        Some(t)
    }

    fn name_of(s: &State, t: &Tuning) -> String {
        s.base.current(&t.rule_sets).map_or_else(String::new, |c| c.name.clone())
    }

    /// StunSet's clips leave on `F_STUN_TIME` under half a second: the stun clock of the damage
    /// object. Before it was answered, every update read 0 and StunStart left at once.
    #[test]
    fn stun_set_stays_while_the_stun_runs_and_exits_under_half_a_second() {
        for (pack, name) in [("bumblebee", "Bumblebee"), ("optimusflight", "OptimusFlight"), ("starscream", "Starscream")] {
            let Some(t) = installed(pack, name) else { return };
            let mut s = State::new(&t);
            let input = Input::default();
            s.damage_timers.stunned = 3.0;
            base_enter(&mut s, &input, &t, crc32(b"StunSet"), (0, 0));
            assert_eq!(name_of(&s, &t), "StunStart", "{pack}");
            assert_eq!(rule_value(&s, &input, animrules::var::STUN_TIME), Some(Value::Float(3.0)));
            // StunStart finishes into the loop and the loop goes on while the clock is high.
            for _ in 0..400 {
                base_step(&mut s, &input, &t, 0.032);
            }
            assert_eq!(name_of(&s, &t), "StunLoop", "{pack}: still stunned");
            s.damage_timers.stunned = 0.4;
            base_step(&mut s, &input, &t, 0.032);
            assert_eq!(name_of(&s, &t), "StunExit", "{pack}: under 0.5 s the loop exits");
        }
    }

    /// A stun that runs out during the opening clip leaves it at any time (`StunStart`'s exit).
    #[test]
    fn stun_start_leaves_at_any_time_once_the_clock_is_under_half_a_second() {
        let Some(t) = installed("bumblebee", "Bumblebee") else { return };
        let mut s = State::new(&t);
        let input = Input::default();
        s.damage_timers.stunned = 0.3;
        base_enter(&mut s, &input, &t, crc32(b"StunSet"), (0, 0));
        base_step(&mut s, &input, &t, 0.032);
        assert_eq!(name_of(&s, &t), "StunExit");
    }

    /// StateStun's enter (0087eaf0) switches to BasicMotionMode and sets its control to (1, 1, 0):
    /// 00854960 then zeroes the body's ground speed and keeps its vertical speed (the override at
    /// STEP+0x3d4 is only set by 007efa90, never by the stun), and every update hands (1, 1, 0) to the
    /// holder: the clip's root drives the ground axes, the vertical one falls. The port's stun runs
    /// `step_hit_reaction` in `Mode::Action`; this checks it gives the same: the run's speed is gone on
    /// the first update (the ground speed is the clip's), and a stun taken in the air keeps falling.
    #[test]
    fn a_stun_drops_the_ground_speed_for_the_clips_and_keeps_falling() {
        const DT: f32 = 0.032;
        let Some(t) = installed("bumblebee", "Bumblebee") else { return };
        let arena = Arena::default();
        let mut s = State::new(&t);
        let forward = Input { stick: Vec2::Y, ..Default::default() };
        for _ in 0..60 { step(&mut s, &forward, &t, &arena, DT); }
        let running = Vec2::new(s.vel.x, s.vel.y).length();
        s.damage_timers.stunned = 3.0;
        s.stun_pending = true;
        step(&mut s, &Input::default(), &t, &arena, DT);
        assert!(s.stun_full && s.mode == Mode::Action, "the stun did not start");
        let data = s.base.current(&t.rule_sets).unwrap();
        let clip = data.travel(0.0, s.base.tick).0 / DT;
        let ground = Vec2::new(s.vel.x, s.vel.y).length();
        assert!(running > 5.0 && (ground - clip.length()).abs() < 1e-3, "running {running}, stunned {ground}, clip {}", clip.length());
        // In the air: the vertical speed is the body's, under gravity.
        let mut s = State::new(&t);
        s.pos.z += 20.0;
        s.vel.z = 0.0;
        for _ in 0..3 { step(&mut s, &Input::default(), &t, &arena, DT); }
        s.damage_timers.stunned = 3.0;
        s.stun_pending = true;
        let before = s.vel.z;
        step(&mut s, &Input::default(), &t, &arena, DT);
        let after = s.vel.z;
        step(&mut s, &Input::default(), &t, &arena, DT);
        assert!(after < before && s.vel.z < after, "vz {before} -> {after} -> {}", s.vel.z);
    }

    /// The roster's BasicMotionMode clips whose root moves in z are the climb's two jumps, the punch out
    /// of the car, the jets' punch out of flight (up to 8.9 m) and the ground punches (down 1.5 m at most,
    /// into the floor). The ones that lift him are the ones `lifted` / `from_climb` take: out of the car or
    /// the wall, and, since 2026-10-20, out of flight. [data] [game 00856cc0: BasicMotionMode's (1,1,1)]
    #[test]
    fn the_jets_punch_out_of_flight_is_lifted_by_its_clip() {
        let Some((t, mut s)) = flying_jet("jetfire", "Jetfire") else { return };
        let punch = t.control.find("stateFlyTransAirPunch").unwrap();
        let rise = t.actions.clips.get(&t.control.states[punch].anim)
            .map(|clip| clip.path.iter().fold(0.0f32, |a, p| a.max(p.z))).unwrap();
        assert!(rise > 8.0, "Trans_V2R_GroundPunch rises {rise}");
        let on_ground = s.ground_flag();
        enter_state(&mut s, &Input::default(), &t, punch, None, on_ground);
        s.previous_state = s.runner.state;
        s.runner.enter(&t.control, punch);
        assert!(s.action.as_ref().is_some_and(|a| a.lifted), "the punch out of flight is not lifted");
        let (start, arena) = (s.pos.z, Arena::default());
        let mut top = start;
        let held = Input { vehicle: 1.0, buttons_down: 1 << button::FLYING_MODE, ..Default::default() };
        for _ in 0..40 {
            step(&mut s, &held, &t, &arena, GAME_UPDATE);
            top = top.max(s.pos.z);
        }
        assert!(top - start > 4.0, "rose {} of the clip's {rise}", top - start);
    }

    fn dash_clip(s: &State, t: &Tuning, angle: f32) -> String {
        let input = Input::default();
        let get = |variable: u32| match variable {
            animrules::var::L_STICK_DIR => Some(Value::Float(angle)),
            _ => rule_value(s, &input, variable),
        };
        let set = &t.rule_sets[&DASH_SET];
        let (_, id) = animrules::entry(&set.incoming, 0, 0, &get).expect("a dodge");
        set.clip(id).unwrap().name.clone()
    }

    /// DashSet's hover dodges need `I_IS_HOVERING == 1`: the hover mode, and the weapon-hover
    /// state that runs on the strafe mode here. On the ground the ordinary dodge plays.
    #[test]
    fn a_flyers_dodge_is_the_hover_dodge_only_while_it_hovers() {
        for (pack, name) in [("starscream", "Starscream"), ("jetfire", "Jetfire"), ("optimusflight", "OptimusFlight"), ("blackout", "Blackout")] {
            let Some(t) = installed(pack, name) else { return };
            let mut s = State::new(&t);
            assert_eq!(dash_clip(&s, &t, 0.0), "DodgeFront", "{pack} on the ground");
            assert_eq!(dash_clip(&s, &t, 90.0), "DodgeLeft");
            assert_eq!(dash_clip(&s, &t, -170.0), "DodgeBack");
            s.mode = Mode::Hover;
            assert_eq!(dash_clip(&s, &t, 0.0), "Hover_DodgeFront", "{pack} hovering");
            assert_eq!(dash_clip(&s, &t, 90.0), "Hover_DodgeLeft");
            assert_eq!(dash_clip(&s, &t, -90.0), "Hover_DodgeRight");
            assert_eq!(dash_clip(&s, &t, 170.0), "Hover_DodgeBack");
            s.hover_weapon = true;
            assert_eq!(dash_clip(&s, &t, 0.0), "Hover_DodgeFront", "{pack} in the weapon-hover state");
        }
    }

    /// A jet in the air, the flying trigger held, on the hub `stateFly`.
    /// JumpMode out of FlyMode (008740e0 l.111-118): the long jump's type at a ground speed squared of
    /// 25 or more, the fall's below, and no launch either way, even with the fall state's FallSet.
    #[test]
    fn jump_mode_out_of_flight_takes_the_long_type_when_fast_and_never_launches() {
        let Some((t, flying)) = flying_jet("starscream", "Starscream") else { return };
        for (speed, want) in [(10.0, JumpType::Long), (4.0, JumpType::Fall)] {
            let mut s = flying.clone();
            assert_eq!(s.mode, Mode::Fly);
            s.vel = Vec3::new(speed, 0.0, -1.0);
            let on_ground = s.ground_flag();
            enter_jump_mode(&mut s, &Input::default(), &t, Some("FallSet"), on_ground);
            assert_eq!(s.mode, Mode::Jump(want), "speed {speed}");
            assert_eq!(s.jump_type, jump_type_hash(want));
            assert_eq!(s.vel.z, -1.0, "speed {speed}: no launch");
        }
    }

    fn flying_jet(pack: &str, name: &str) -> Option<(Tuning, State)> {
        let t = installed(pack, name)?;
        let mut s = State::new(&t);
        let arena = Arena::default();
        let held = Input { vehicle: 1.0, buttons_down: 1 << button::FLYING_MODE, ..Default::default() };
        for _ in 0..200 {
            step(&mut s, &held, &t, &arena, GAME_UPDATE);
            if s.control_state(&t) == Some("stateFly") && s.pos.z > 1.0 {
                return Some((t, s));
            }
        }
        panic!("{pack} never reached the fly hub: {:?}", s.control_state(&t));
    }

    /// The flying trigger let go with the weapon-mode trigger held, high up: the jet changes to the
    /// robot (`stateHoverEnter`) and goes on to `stateWeaponModeHover`, whose motion mode is HoverMode
    /// [data 1094]. He hangs there with the weapon set's `Hover_Weapon_Idle` playing and fires; he
    /// does not fall (before 2026-10-20 the state ran on StrafeMode and dropped to the ground).
    #[test]
    fn the_weapon_hover_hangs_in_the_air_with_the_gun() {
        let Some((t, mut s)) = flying_jet("starscream", "Starscream") else { return };
        let arena = Arena::default();
        let climb = Input {
            vehicle: 1.0, buttons_down: 1 << button::FLYING_MODE,
            stick: Vec2::new(0.0, -1.0), pad_stick: Vec2::new(0.0, -1.0), ..Default::default()
        };
        for _ in 0..25 {
            step(&mut s, &climb, &t, &arena, GAME_UPDATE);
        }
        assert!(s.pos.z > 15.0, "climbed to {}", s.pos.z);
        let aim = Input { aim: Some(Vec2::Y), ..Default::default() };
        let mut reached = None;
        for i in 0..80 {
            step(&mut s, &aim, &t, &arena, GAME_UPDATE);
            if s.control_state(&t) == Some("stateWeaponModeHover") {
                reached = Some(i);
                break;
            }
        }
        assert!(reached.is_some(), "never reached the weapon hover: {:?}", s.control_state(&t));
        assert!(s.weapon_hover() && s.mode == Mode::Hover, "{:?}", s.mode);
        let height = s.pos.z;
        let fire = Input { aim: Some(Vec2::Y), buttons_down: 1 << button::WEAPON_FIRE, ..Default::default() };
        let mut fired = false;
        for _ in 0..60 {
            step(&mut s, &fire, &t, &arena, GAME_UPDATE);
            fired |= s.firing;
        }
        assert_eq!(s.control_state(&t), Some("stateWeaponModeHover"));
        assert!((s.pos.z - height).abs() < 1.0 && s.pos.z > 10.0, "{height} -> {}", s.pos.z);
        assert_eq!(s.action_clip(&t).map(|(set, id, _)| (set, id)), Some(("WeaponSet", "Hover_Weapon_Idle")));
        assert!(fired, "the hover weapon state never fired");
    }

    /// Letting go of the flight: the fly exit's enter puts him back on foot (`0087c7e0`, control
    /// mode 1), so the car's gun and sights go; and the fall state's high standing jump out of the
    /// companion ends at the idle state's BasicMotionMode enter (00856cc0, (1, 1, 1)), a hop of
    /// about a metre, not the 14 m he rose in the standing mode before. Grindor and a jet.
    #[test]
    fn leaving_flight_puts_him_back_on_foot_without_a_leap() {
        for (pack, name) in [("blackout", "Blackout"), ("starscream", "Starscream")] {
            let Some((t, mut s)) = flying_jet(pack, name) else { return };
            assert!(s.car_mode, "{pack}");
            let arena = Arena::default();
            let mut exited = false;
            let mut landed = false;
            let mut highest_after = 0.0f32;
            for _ in 0..250 {
                step(&mut s, &Input::default(), &t, &arena, GAME_UPDATE);
                let state = s.control_state(&t).unwrap_or("");
                if state.starts_with("stateFlyExit") {
                    exited = true;
                    assert!(!s.car_mode, "{pack}: {state} keeps the car's control mode");
                }
                if exited && s.mode == Mode::Unfold && s.pos.z < 0.05 {
                    landed = true;
                }
                if landed {
                    highest_after = highest_after.max(s.pos.z);
                }
            }
            assert!(exited && landed, "{pack}: exited {exited} landed {landed}");
            assert!(!s.car_mode && s.mode == Mode::Stand, "{pack}: {:?} car_mode {}", s.mode, s.car_mode);
            assert!(highest_after < 2.0, "{pack}: rose {highest_after} after landing");
        }
    }

    /// A stunt is a control state that starts its clip BY NAME (`Vehicle_RollLeft`, set FlySet),
    /// and the clip has an exit rule of its own: finished, to `Vehicle_Idle`. The state machine
    /// sees the clip finished and takes the stunt state back to the hub, whose entry walk lands
    /// on `Vehicle_Idle` and stays there (nothing plays the roll twice). Held in the stunt state
    /// (the machine refused to leave), the action player's exit walk puts `Vehicle_Idle` in the
    /// clip's place, as the controller does for every clip it plays.
    #[test]
    fn a_jets_stunt_clip_leaves_by_its_exit_rule_and_the_hub_takes_over() {
        for (pack, name, message, state, clip) in [
            ("starscream", "Starscream", 0xec31_dce8u32, "stateFlyStuntLeft", "Vehicle_RollLeft"),
            ("starscream", "Starscream", 0xd512_ac49, "stateFlyStuntUp", "Vehicle_LoopFull"),
            ("firestorm", "Firestorm", 0x59e4_a00f, "stateFlyStuntRight", "Vehicle_RollRight"),
        ] {
            let Some((t, mut s)) = flying_jet(pack, name) else { return };
            let arena = Arena::default();
            let held = Input { vehicle: 1.0, buttons_down: 1 << button::FLYING_MODE, ..Default::default() };
            // The rules read from the stunt state's own set.
            let index = t.control.find(state).unwrap();
            let (set, found) = action_rule_clip(&t, index, crc32(clip.as_bytes())).expect("the clip is in a set");
            assert_eq!(set, FLY_SET, "{pack} {state}: the fly states start FlySet");
            assert!(found.exits.iter().any(|e| e.finish && e.to_id == crc32(b"Vehicle_Idle")), "{pack} {clip} finishes into Vehicle_Idle");

            // Case 1: the machine leaves the finished stunt for the hub on its own.
            let mut left = s.clone();
            left.messages.push(message);
            step(&mut left, &held, &t, &arena, GAME_UPDATE);
            assert_eq!(left.control_state(&t), Some(state), "{pack}: the message starts the stunt state");
            assert_eq!(left.action_clip(&t).map(|c| c.1), Some(clip));
            let mut came_back = false;
            for _ in 0..600 {
                step(&mut left, &held, &t, &arena, GAME_UPDATE);
                if left.control_state(&t) == Some("stateFly") {
                    came_back = true;
                    break;
                }
            }
            assert!(came_back, "{pack}: {state} never returned to the hub");
            assert!(left.action.is_none(), "the hub plays no clip of its own");
            for _ in 0..90 {
                assert_eq!(left.base_clip(&t).map(|c| c.id), Some("Vehicle_Idle"), "{pack}: the hub's clip after {clip}");
                step(&mut left, &held, &t, &arena, GAME_UPDATE);
                assert_eq!(left.control_state(&t), Some("stateFly"), "{pack}: no second stunt");
            }

            // Case 2: the machine cannot leave (no vehicle form while disabled); the exit walk
            // alone hands the finished clip over.
            s.messages.push(message);
            step(&mut s, &held, &t, &arena, GAME_UPDATE);
            assert_eq!(s.control_state(&t), Some(state));
            s.damage_timers.disabled_vehicle = 1.0e6;
            let mut handed = false;
            for _ in 0..600 {
                step(&mut s, &held, &t, &arena, GAME_UPDATE);
                if s.action_clip(&t).map(|c| c.1) == Some("Vehicle_Idle") {
                    handed = true;
                    break;
                }
            }
            assert!(handed, "{pack}: {clip} never handed over to Vehicle_Idle by its exit rule");
            assert_eq!(s.control_state(&t), Some(state), "still the stunt state: the machine could not leave");
        }
    }

    /// A stunt ends when the machine is back at the hub, before the hub's walks read it.
    #[test]
    fn re_entering_the_fly_hub_ends_the_stunt() {
        let Some((t, mut s)) = flying_jet("starscream", "Starscream") else { return };
        s.fly.as_mut().unwrap().stunt = crate::flight::Stunt::Left;
        let hub = t.control.find("stateFly").unwrap();
        let on_ground = s.ground_flag();
        enter_state(&mut s, &Input::default(), &t, hub, None, on_ground);
        assert_eq!(s.fly.as_ref().unwrap().stunt, crate::flight::Stunt::None);
    }

    /// Hover. The states name their clips, so HoverSet's entry rules are not what starts them:
    /// `stateHoverEnter` plays `Trans_V2R_Hover` of `Vehicle2RobotSet` (no exit rules), and
    /// `stateHover` plays `Hover_Idle` of `HoverSet`. HoverSet's own `Trans_V2R_Hover` (a
    /// different animRef, finished into `Hover_Idle`) is what its entry walk picks, from
    /// anything but the weapon set; from the weapon set the walk gives `Hover_Idle`. Both
    /// walks run through `animrules::Base` on the real data.
    #[test]
    fn hover_states_play_their_named_clips_and_the_sets_rules_are_the_graphs() {
        for (pack, name) in [("starscream", "Starscream"), ("jetfire", "Jetfire"), ("blackout", "Blackout"), ("optimusflight", "OptimusFlight"),
            ("megatronflight", "MegatronFlight"), ("firestorm", "Firestorm")] {
            let Some(t) = installed(pack, name) else { return };
            let hover_set = crc32(b"HoverSet");
            let (enter, hover) = (t.control.find("stateHoverEnter").unwrap(), t.control.find("stateHover").unwrap());
            let (set, clip) = action_rule_clip(&t, enter, crc32(b"Trans_V2R_Hover")).unwrap();
            assert_eq!(set, crc32(b"Vehicle2RobotSet"), "{pack}: the enter state names Vehicle2RobotSet");
            assert!(clip.exits.is_empty(), "{pack}: that copy has no exit rules");
            let (set, clip) = action_rule_clip(&t, hover, crc32(b"Hover_Idle")).unwrap();
            assert_eq!((set, clip.exits.len()), (hover_set, 0), "{pack}");
            // The enter state blends in as ITS copy of the clip asks (0), not HoverSet's (0.1).
            let mut s = State::new(&t);
            assert!(start_clip(&mut s, &Input::default(), &t, enter, crc32(b"Trans_V2R_Hover")));
            assert_eq!(s.action.as_ref().map(|a| a.fade_time), Some(0.0), "{pack}");
            // And it plays as Vehicle2RobotSet's, for the drawing, for the next entry walk's
            // "from", and for the weapon state's test of the change from the car (`0087f4a0`):
            // the action table alone would give HoverSet's copy.
            assert_eq!(s.action_clip(&t).map(|(set, id, _)| (set, id)), Some(("Vehicle2RobotSet", "Trans_V2R_Hover")), "{pack}");
            assert_eq!(playing_clip(&s, &t), (crc32(b"Vehicle2RobotSet"), crc32(b"Trans_V2R_Hover")), "{pack}");
            assert!(start_clip(&mut s, &Input::default(), &t, hover, crc32(b"Hover_Idle")));
            assert_eq!(s.action_clip(&t).map(|(set, _, _)| set), Some("HoverSet"), "{pack}");
            assert_eq!(s.action.as_ref().map(|a| (a.fade_time * 100.0).round()), Some(10.0), "{pack}: HoverSet's Hover_Idle");

            // HoverSet's entry walk and its exit walk, through the controller.
            let input = Input::default();
            let get = |variable: u32| rule_value(&State::new(&t), &input, variable);
            let mut layer = Base::default();
            assert!(layer.enter(&t.rule_sets, hover_set, (WEAPON_SET, crc32(b"Hover_Weapon_Idle")), &get));
            assert_eq!(layer.clip, crc32(b"Hover_Idle"), "{pack}: from the weapon set, straight to the idle");
            let mut layer = Base::default();
            assert!(layer.enter(&t.rule_sets, hover_set, (IDLE_SET, 0), &get));
            assert_eq!((layer.set, layer.clip), (hover_set, crc32(b"Trans_V2R_Hover")), "{pack}");
            layer.advance(&t.rule_sets, 0.1);
            layer.update(&t.rule_sets, &get);
            assert_eq!(layer.clip, crc32(b"Trans_V2R_Hover"), "{pack}: not finished yet");
            layer.advance(&t.rule_sets, 30.0);
            layer.update(&t.rule_sets, &get);
            assert_eq!(layer.clip, crc32(b"Hover_Idle"), "{pack}: finished into the idle");
        }
    }

    /// The robot flyers' hub starts FlySet with no clip named; the change into the flying form
    /// is picked by its entry rules (`Trans_RobotRun2Vehicle` with the stick pushed,
    /// `Trans_Robot2Vehicle` without), and it finishes into `Fly_Idle`. Their `DriveSet` holds
    /// the same two entries, but their machines have no `StateDrive`, so nothing starts it.
    #[test]
    fn the_robot_flyers_hub_enters_flyset_by_the_entry_rules() {
        for (pack, name) in [("optimusflight", "OptimusFlight"), ("megatronflight", "MegatronFlight")] {
            let Some(t) = installed(pack, name) else { return };
            assert!(t.control.states.iter().all(|state| state.class != "StateDrive"), "{pack} has no drive state");
            let drive = &t.rule_sets[&crc32(b"DriveSet")];
            let fly = &t.rule_sets[&FLY_SET];
            let names = |set: &crate::tuning::RuleSet, stick: f32| {
                let get = |variable: u32| (variable == animrules::var::L_STICK_AMP).then_some(Value::Float(stick));
                let (_, id) = animrules::entry(&set.incoming, IDLE_SET, crc32(b"Idle"), &get).unwrap();
                set.clip(id).unwrap().name.clone()
            };
            for (stick, expect) in [(0.0, "Trans_Robot2Vehicle"), (1.0, "Trans_RobotRun2Vehicle")] {
                assert_eq!(names(fly, stick), expect, "{pack} FlySet, stick {stick}");
                assert_eq!(names(drive, stick), expect, "{pack} DriveSet, stick {stick}");
            }
            let arena = Arena::default();
            for (stick, expect) in [(Vec2::ZERO, "Trans_Robot2Vehicle"), (Vec2::Y, "Trans_RobotRun2Vehicle")] {
                let mut s = State::new(&t);
                let held = Input { vehicle: 1.0, stick, buttons_down: 1 << button::FLYING_MODE, ..Default::default() };
                step(&mut s, &Input::default(), &t, &arena, GAME_UPDATE);
                for _ in 0..3 {
                    step(&mut s, &held, &t, &arena, GAME_UPDATE);
                }
                assert_eq!(s.control_state(&t), Some("stateFly"), "{pack}");
                assert_eq!(s.base_clip(&t).map(|c| (c.set, c.id)), Some(("FlySet", expect)), "{pack}, stick {stick}");
                for _ in 0..120 {
                    step(&mut s, &held, &t, &arena, GAME_UPDATE);
                }
                assert!(s.base_clip(&t).is_some_and(|c| c.id != expect), "{pack}: the change finished into a Fly_ clip");
            }
        }
    }

    /// FlyMode's enter and exit start `V2RPartialSet`'s partial clips on layer 1, but only where
    /// `specialAbilityVehicleWeaponFireFromAnim` is set. Of the roster that is the Audi, Blackout,
    /// Ironhide and the ground Megatron; MegatronFlight, the only pack with the set, has it clear,
    /// so the layer never plays in the shipped game's data.
    #[test]
    fn the_v2r_partial_layer_plays_only_where_the_flag_is_set() {
        let Some(mut t) = installed("megatronflight", "MegatronFlight") else { return };
        let flagged: Vec<&str> = [("audir8", "Audi"), ("blackout", "Blackout"), ("ironhide", "Ironhide"), ("megatron", "Megatron"),
            ("megatronflight", "MegatronFlight"), ("optimusflight", "OptimusFlight"), ("starscream", "Starscream"), ("bumblebee", "Bumblebee")]
            .into_iter()
            .filter(|(pack, name)| installed(pack, name).is_some_and(|t| t.vehicle_special_from_anim))
            .map(|(pack, _)| pack)
            .collect();
        assert_eq!(flagged, ["audir8", "blackout", "ironhide", "megatron"]);
        assert!(!t.vehicle_special_from_anim);

        let (input, arena) = (Input::default(), Arena::default());
        let mut s = State::new(&t);
        set_mode(&mut s, &input, &t, Mode::Fly);
        assert!(s.fly_partial.playing().is_none(), "MegatronFlight: flag clear, nothing starts");

        // With the flag forced on: the enter plays Trans_R2V_Partial, which ends into emptyAnimSet;
        // the exit plays Trans_V2R_Partial.
        t.vehicle_special_from_anim = true;
        let mut s = State::new(&t);
        set_mode(&mut s, &input, &t, Mode::Fly);
        s.mode = Mode::Fly;
        assert_eq!(s.fly_partial.playing(), Some((crc32(b"V2RPartialSet"), crc32(b"Trans_R2V_Partial"))));
        for _ in 0..400 {
            step_fly(&mut s, &input, &t, &arena, GAME_UPDATE);
            let mut layer = std::mem::take(&mut s.fly_partial);
            layer.update_partial(&t.rule_sets, (0, 0), &|v| rule_value(&s, &input, v));
            layer.advance(&t.rule_sets, GAME_UPDATE);
            s.fly_partial = layer;
        }
        assert!(s.fly_partial.playing().is_none(), "the partial ends into emptyAnimSet");
        set_mode(&mut s, &input, &t, Mode::Move);
        assert_eq!(s.fly_partial.playing(), Some((crc32(b"V2RPartialSet"), crc32(b"Trans_V2R_Partial"))));

        // A pack without the set: the set lookup fails and nothing starts, as in the game.
        let Some(t) = installed("blackout", "Blackout") else { return };
        assert!(t.vehicle_special_from_anim);
        let mut s = State::new(&t);
        set_mode(&mut s, &input, &t, Mode::Fly);
        assert!(s.fly_partial.playing().is_none(), "Blackout has the flag but not V2RPartialSet");
    }

    /// Every clip a control state of the roster starts by name, in a set the action player
    /// plays, that has exit rules: these are the ones `action_exits` (and `special_chain` for
    /// `SpecialAnimSet`) must walk, and their targets must be clips the player can start.
    #[test]
    fn the_rosters_named_clips_with_exit_rules_are_the_jets_stunts_and_the_specials() {
        let mut with_exits: Vec<String> = Vec::new();
        for (pack, name) in [("bumblebee", "Bumblebee"), ("optimus", "Optimus"), ("ironhide", "Ironhide"), ("ratchet", "Ratchet"),
            ("megatron", "Megatron"), ("longhaul", "LongHaul"), ("audir8", "Audi"), ("jetfire", "Jetfire"), ("firestorm", "Firestorm"),
            ("starscream", "Starscream"), ("blackout", "Blackout"), ("optimusflight", "OptimusFlight"), ("megatronflight", "MegatronFlight")] {
            let Some(t) = installed(pack, name) else { return };
            for (index, def) in t.control.states.iter().enumerate() {
                let started_by_name = matches!(def.class.as_str(), "StateAttack" | "StateDash" | "StateBasic" | "StateAttackSpecial" | "StateDrive" | "StateFly" | "StateFlyExit");
                if !started_by_name || def.anim == 0 {
                    continue;
                }
                let Some((set, clip)) = action_rule_clip(&t, index, def.anim) else { continue };
                if clip.exits.is_empty() {
                    continue;
                }
                with_exits.push(format!("{pack}:{}:{}", def.name, clip.name));
                for exit in &clip.exits {
                    let to_set = if exit.to_set == 0 { set } else { exit.to_set };
                    if exit.to_id != 0 && plays_as_action(to_set) {
                        assert!(t.actions.clips.contains_key(&exit.to_id), "{pack} {}: the exit's clip is not in the table", clip.name);
                    }
                }
            }
        }
        with_exits.sort();
        assert_eq!(
            with_exits,
            ["firestorm:stateFlyStuntDown:Vehicle_LoopBack", "firestorm:stateFlyStuntLeft:Vehicle_RollLeft", "firestorm:stateFlyStuntRight:Vehicle_RollRight",
                "firestorm:stateFlyStuntUp:Vehicle_LoopFull", "starscream:stateFlyStuntDown:Vehicle_LoopBack", "starscream:stateFlyStuntLeft:Vehicle_RollLeft",
                "starscream:stateFlyStuntRight:Vehicle_RollRight", "starscream:stateFlyStuntUp:Vehicle_LoopFull"],
            "update this list if a pack gains a named clip with exit rules"
        );
    }

    /// WeaponSet's `Hover_Weapon_Idle` entry holds only while hovering.
    #[test]
    fn the_weapon_set_starts_its_hover_idle_only_while_hovering() {
        let Some(t) = installed("starscream", "Starscream") else { return };
        let mut s = State::new(&t);
        let input = Input::default();
        let pick = |s: &State| {
            let get = |variable: u32| rule_value(s, &input, variable);
            let (_, id) = animrules::entry(&t.weapon_set.incoming, 0, 0, &get).expect("an idle");
            t.weapon_set.clip(id).unwrap().name.clone()
        };
        assert_ne!(pick(&s), "Hover_Weapon_Idle", "on the ground");
        s.hover_weapon = true;
        assert_eq!(pick(&s), "Hover_Weapon_Idle", "in the weapon-hover state");
        s.hover_weapon = false;
        s.mode = Mode::Hover;
        assert_eq!(pick(&s), "Hover_Weapon_Idle", "hovering");
    }

    /// FlySet's `Vehicle_Idle` exits on the stunt hashes. Unanswered they compared as 0 against a
    /// hash of 0 and held, which sent the jets into a bank at once; the variable is 0 as the
    /// game's FlyMode writes it, so the clip stays.
    #[test]
    fn the_jets_vehicle_idle_does_not_bank_or_roll_on_its_own() {
        for (pack, name) in [("jetfire", "Jetfire"), ("firestorm", "Firestorm"), ("starscream", "Starscream")] {
            let Some(t) = installed(pack, name) else { return };
            let mut s = State::new(&t);
            let input = Input::default();
            assert_eq!(rule_value(&s, &input, animrules::var::VEHICLE_STUNT), Some(Value::Hash(0)));
            base_enter(&mut s, &input, &t, FLY_SET, (0, 0));
            assert_eq!(name_of(&s, &t), "Vehicle_Idle", "{pack}");
            for _ in 0..60 {
                base_step(&mut s, &input, &t, 0.032);
            }
            assert_eq!(name_of(&s, &t), "Vehicle_Idle", "{pack}: no stunt hash is ever the variable's value");
            // The exits the data has are on stunt hashes, none of them 0.
            let idle = t.rule_sets[&FLY_SET].clip(crc32(b"Vehicle_Idle")).unwrap();
            let stunts: Vec<u32> = idle.exits.iter().flat_map(|e| e.rules.iter()).filter(|r| r.variable == animrules::var::VEHICLE_STUNT).map(|r| r.hash).collect();
            assert!(!stunts.is_empty() && stunts.iter().all(|&h| h != 0), "{pack}: {stunts:x?}");
        }
    }

    /// `I_SPECIAL_ON` through `rule_value` is the ability timer above 0.
    #[test]
    fn the_special_variable_follows_the_ability_timer() {
        let Some(t) = installed("megatron", "Megatron") else { return };
        let mut s = State::new(&t);
        let input = Input::default();
        assert_eq!(rule_value(&s, &input, animrules::var::SPECIAL_ON), Some(Value::Int(0)));
        s.special_left = 1.5;
        assert_eq!(rule_value(&s, &input, animrules::var::SPECIAL_ON), Some(Value::Int(1)));
        // Megatron's SpecialAnimSet: the idle clip loops while it is on and exits when it is off.
        let set = &t.rule_sets[&crc32(b"SpecialAnimSet")];
        let idle = set.clip(crc32(b"SpecialAttack_Idle")).unwrap();
        let exits_with = |s: &State| {
            let get = |variable: u32| rule_value(s, &input, variable);
            animrules::exit(&idle.exits, 1, false, &get).map(|e| e.to_id)
        };
        assert_eq!(exits_with(&s), None);
        s.special_left = 0.0;
        assert_eq!(exits_with(&s), Some(crc32(b"SpecialAttack_Exit")));
    }
}

/// The clip events that act on his motion and layers (`ClipAction::is_motion`), read from the
/// installed packs and carried out by `apply_clip_effects`.
#[cfg(test)]
mod clip_event_tests {
    use super::*;

    fn installed(pack: &str, name: &str) -> Option<Tuning> {
        let dir = std::path::Path::new("C:/Games2");
        if !dir.exists() {
            return None;
        }
        let mut t = crate::tuning::load(dir, name).unwrap();
        let data = crate::character::load(dir, pack).unwrap();
        t.locomotion = crate::tuning::Locomotion::from_animations(&data.animations, &mut t.missing);
        t.actions = crate::tuning::Actions::from_animations(&data.animations, &data.robot, &mut t.missing).with_vehicle(data.vehicle.as_ref());
        t.weapon_set = crate::tuning::RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
        t.rule_sets = crate::tuning::RuleSet::all(&data.animations);
        Some(t)
    }

    fn motion_of<'a>(t: &'a Tuning, set: &str, clip: &str) -> &'a [crate::formats::anim::ActionEvent] {
        &t.rule_sets[&crc32(set.as_bytes())].clip(crc32(clip.as_bytes())).unwrap().motion
    }

    /// Each event kind is read from the packs with the values the decoded data shows
    /// (`work\packdump`): the explosion's fall [1, 1, 0], a weapon landing's StrafeMode, the
    /// stunt's dash clock, the flyers' upright, Ratchet's charging arm. [data]
    #[test]
    fn the_motion_events_are_read_from_the_installed_clips() {
        let Some(bee) = installed("bumblebee", "Bumblebee") else { return };
        assert!(motion_of(&bee, "DeathSet", "Death_Explode_Fall").iter().any(|e| e.action == ClipAction::Traction([1.0, 1.0, 0.0])));
        let strafe = bee.weapon_set.clips.iter().filter(|c| c.motion.iter().any(|e| e.action == ClipAction::MotionMode(STRAFE_MODE))).count();
        assert!(strafe > 0, "a weapon clip switches to StrafeMode");
        assert!(motion_of(&bee, "WeaponSet", "Weapon_Fall").iter().any(|e| e.action == ClipAction::MotionMode(JUMP_MODE)));
        let Some(star) = installed("starscream", "Starscream") else { return };
        assert!(motion_of(&star, "FlySet", "Vehicle_RollLeft").iter().any(|e| e.action == ClipAction::Dash(1.0)));
        let Some(jet) = installed("jetfire", "Jetfire") else { return };
        let up = jet.rule_sets.values().flat_map(|set| &set.clips).flat_map(|c| &c.motion).filter(|e| e.action == ClipAction::ForceUp([0.0, 0.0, 1.0])).count();
        assert_eq!(up, 1, "Jetfire's one ForceUpEvent");
        // `PlayCharPartialAnim` is on the weapon object's clips, not his own (`gun::Gun::step`).
        let Some(data) = crate::character::load(std::path::Path::new("C:/Games2"), "longhaul").ok() else { return };
        let charging = ClipAction::PartialAnim { set: crc32(b"WeaponPartialSet"), id: crc32(b"Weapon_Charging") };
        let keys: Vec<f32> = data.weapon_animations.iter().flatten().filter_map(|r| r.clip.as_ref())
            .flat_map(|c| &c.action_events).filter(|e| e.action == charging).map(|e| e.time).collect();
        assert!(keys.contains(&108.0) && keys.contains(&214.0), "Long Haul's WeaponB_Charging asks at 108 and 214: {keys:?}");
    }

    /// The explosion's fall hands the height to gravity: its traction's z is 0 (00854960), so
    /// the death no longer follows the root's height there. [game] [data]
    #[test]
    fn the_explosion_fall_takes_its_height_from_gravity() {
        let Some(t) = installed("bumblebee", "Bumblebee") else { return };
        let mut s = State::new(&t);
        let input = Input::default();
        s.pos.z = 10.0;
        s.touching = false;
        s.damage(f32::MAX);
        let mut fell_by_gravity = false;
        for _ in 0..60 {
            step(&mut s, &input, &t, &Arena::default(), 0.032);
            let Some(death) = s.death.as_ref() else { continue };
            if death.clip == crc32(b"Death_Explode_Fall") {
                fell_by_gravity |= !death.root_z;
            }
        }
        assert!(fell_by_gravity, "Death_Explode_Fall's [1, 1, 0] reached the death");
    }

    /// The other effects as `apply_clip_effects` carries them out. [game 00731400, 00712ad0,
    /// 007baa00, 00712a20]
    #[test]
    fn dash_upright_partial_and_mode_events_act_on_him() {
        let Some(t) = installed("bumblebee", "Bumblebee") else { return };
        let mut s = State::new(&t);
        let input = Input::default();
        s.clip_effects.push(ClipAction::Dash(1.33));
        (s.pitch, s.roll) = (0.4, -0.3);
        s.clip_effects.push(ClipAction::ForceUp([0.0, 0.0, 1.0]));
        s.clip_effects.push(ClipAction::PartialAnim { set: 7, id: 9 });
        apply_clip_effects(&mut s, &input, &t, &Arena::default());
        assert_eq!(s.damage_timers.dash, 1.33);
        assert!(s.pitch.abs() < 1e-6 && s.roll.abs() < 1e-6);
        assert_eq!(s.partial_requests, vec![(7, 9)]);
        // The clock runs down with the others (0072a550).
        s.damage_timers.step(1.0);
        assert!((s.damage_timers.dash - 0.33).abs() < 1e-5);
        // A tilted up: z along it, the heading kept.
        s.yaw = 0.7;
        clip_force_up(&mut s, Vec3::new(0.3, -0.2, 0.9));
        let frame = Quat::from_rotation_z(s.yaw) * Quat::from_rotation_x(s.pitch) * Quat::from_rotation_y(s.roll);
        assert!((frame * Vec3::Z - Vec3::new(0.3, -0.2, 0.9).normalize()).length() < 1e-5);
        // A mode event switches the motion mode; the mode already current does nothing.
        s.mode = Mode::Move;
        s.clip_effects.push(ClipAction::MotionMode(STRAFE_MODE));
        apply_clip_effects(&mut s, &input, &t, &Arena::default());
        assert_eq!(s.mode, Mode::Strafe);
        s.clip_effects.push(ClipAction::MotionMode(STRAFE_MODE));
        apply_clip_effects(&mut s, &input, &t, &Arena::default());
        assert_eq!(s.mode, Mode::Strafe);
    }

    /// Megatron's tank special stays in the tank and runs its set's chain: Intro, ChargeIdle
    /// while the ability is on, Exit, then back to `stateDrive` (`finishOnSpecialAnimSetExit`
    /// holds the always-on `connectionIsFinished`, 00879990). [game] [data]
    #[test]
    fn megatrons_tank_special_runs_its_chain_in_the_tank() {
        let Some(t) = installed("megatron", "Megatron") else { return };
        let mut s = State::new(&t);
        let arena = Arena::default();
        let bit = 1u64 << button::ATTACK_SPECIAL;
        let mut seen = Vec::new();
        for i in 0..260 {
            let mut input = Input { vehicle: if i >= 5 { 1.0 } else { 0.0 }, ..Default::default() };
            if i == 80 {
                (input.buttons_hit, input.buttons_down) = (bit, bit);
            }
            step(&mut s, &input, &t, &arena, 0.032);
            if i >= 80 {
                assert!(s.is_vehicle(), "update {i}: still in the tank");
            }
            if let Some((_, id, _)) = s.action_clip(&t).filter(|(set, _, _)| *set == "SpecialAnimSet") {
                if seen.last() != Some(&id.to_owned()) {
                    seen.push(id.to_owned());
                }
            }
        }
        assert_eq!(seen, ["SpecialAttackVehicle_Intro", "SpecialAttackVehicle_ChargeIdle", "SpecialAttackVehicle_Exit"]);
        assert_eq!(s.control_state(&t), Some("stateDrive"));
    }

    /// The drive hub (`stateDrive`, no clip named) walks DriveSet's entry rules: `Vehicle_Idle`.
    /// [game 0087ba10 -> 0084ddb0] [data]
    #[test]
    fn the_drive_hub_plays_the_drive_sets_entry_clip() {
        let Some(t) = installed("bumblebee", "Bumblebee") else { return };
        let mut s = State::new(&t);
        let arena = Arena::default();
        let mut idle = false;
        for i in 0..120 {
            let input = Input { vehicle: if i >= 5 { 1.0 } else { 0.0 }, ..Default::default() };
            step(&mut s, &input, &t, &arena, 0.032);
            idle |= s.control_state(&t) == Some("stateDrive") && s.base.playing() == Some((crc32(b"DriveSet"), crc32(b"Vehicle_Idle")));
        }
        assert!(idle);
    }

    /// His animation rate scales the base layer's clock as it does the action clip's
    /// (0071f4d0 / 00748dc0). [game]
    #[test]
    fn the_animation_rate_slows_the_base_layer() {
        let Some(t) = installed("bumblebee", "Bumblebee") else { return };
        let input = Input::default();
        let ticks = |rate: f32| {
            let mut s = State::new(&t);
            s.special_rate = rate;
            base_enter(&mut s, &input, &t, IDLE_SET, (0, 0));
            for _ in 0..10 {
                step(&mut s, &input, &t, &Arena::default(), 0.032);
            }
            s.base.elapsed_ticks
        };
        let (full, slow) = (ticks(1.0), ticks(0.2));
        assert!(full > 0.0 && (slow - 0.2 * full).abs() < 0.05 * full, "{full} {slow}");
    }

    /// The slam's finisher holds the nearest enemy ahead that its damage would kill and lands
    /// the kill on it; a healthy one ahead and a weak one behind are passed over (0072e100,
    /// 0072dfd0). [game]
    #[test]
    fn the_slam_finisher_picks_a_weak_enemy_ahead_and_kills_it() {
        let Some(t) = installed("bumblebee", "Bumblebee") else { return };
        let slam = crc32(b"Trans_Vehicle2RobotSlam");
        let Some(state) = t.control.states.iter().position(|d| d.anim == slam) else { return };
        let damage = t.actions.clips[&slam].attack.expect("the slam is an attack").damage_for(true, false);
        let mut s = State::new(&t);
        s.actor.player = true;
        let input = Input::default();
        assert!(start_clip(&mut s, &input, &t, state, slam));
        let target = |id: u32, y: f32, health: f32| Target {
            id, pos: Vec3::new(0.0, y, 0.0), radius: 2.0, height: 5.4, character: true, large: false,
            surface: melee::surface::UPPER_BODY, dashing: false, health, head: None,
        };
        // He faces +y.
        let arena = Arena { targets: vec![target(1, 5.0, damage * 4.0), target(2, -3.0, 1.0), target(3, 7.0, damage)], ..Arena::default() };
        s.yaw = 0.0;
        finish_move(&mut s, &t, &arena, true, false, false);
        assert_eq!(s.finish_target, Some(3));
        finish_move(&mut s, &t, &arena, false, false, true);
        assert_eq!(s.hits.last().map(|h| (h.target, h.damage)), Some((3, damage)));
        // A computer-run attacker does not finish.
        let mut ai = State::new(&t);
        ai.actor.player = false;
        assert!(start_clip(&mut ai, &input, &t, state, slam));
        finish_move(&mut ai, &t, &arena, true, false, false);
        assert_eq!(ai.finish_target, None);
    }

    /// Megatron's on-foot special turns with his aim: StrafeMode's standing update writes
    /// `F_STRAFE_ANG`, and `SpecialAttack_Idle` hands over to `IdleTurnL` past 45. [game 00875ef0]
    #[test]
    fn megatrons_special_turns_with_his_aim() {
        let Some(t) = installed("megatron", "Megatron") else { return };
        let mut s = State::new(&t);
        let arena = Arena::default();
        let bit = 1u64 << button::ATTACK_SPECIAL;
        let mut clips = Vec::new();
        for i in 0..120 {
            // The special at 5; from 60 the aim is swung 60 degrees to his left.
            let aim = if i >= 60 { Vec2::new(-(60f32.to_radians().sin()), 60f32.to_radians().cos()) } else { Vec2::Y };
            let mut input = Input { aim: Some(aim), ..Default::default() };
            if i == 5 {
                (input.buttons_hit, input.buttons_down) = (bit, bit);
            }
            step(&mut s, &input, &t, &arena, 0.032);
            if let Some((_, id, _)) = s.action_clip(&t) {
                if clips.last() != Some(&id.to_owned()) {
                    clips.push(id.to_owned());
                }
            }
        }
        assert!(clips.iter().any(|c| c == "SpecialAttack_IdleTurnL"), "{clips:?}");
    }

    /// The dodge's enter sets the dash clock to half a second (0087a960). [game]
    #[test]
    fn the_dodge_sets_the_dash_clock() {
        let Some(t) = installed("bumblebee", "Bumblebee") else { return };
        let mut s = State::new(&t);
        let dash = t.control.states.iter().position(|d| d.class == "StateDash").unwrap();
        let input = Input { stick: Vec2::new(0.0, 1.0), ..Input::default() };
        begin_action(&mut s, &input, &t, dash, true);
        assert_eq!(s.damage_timers.dash, 0.5);
    }
}
