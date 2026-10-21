//! Debris: the pieces a damage stage throws when it is built (`lux::Debris`, a component on the Object of a
//! `DetachLink`; the escort barriers' rocks, cars and rubble, and the five pieces of Bumblebee's micro missile's
//! blast object). `notes\mission-level-instances.md` "Debris", `work\weapon-audit\r3-debris-done.md`.
//!
//! [game] 00552930 (the component's ctor, run when the piece's stage is built by 00554830) reads the fields and
//! launches the piece at once:
//! - speed: `speed_min + random(0, speed - speed_min)` (only when speed_min < speed; else `speed`);
//! - direction: from the damage point (the engine's last damage position, DAT_00d16738..40) to the piece, normalised
//!   (left as is when shorter than 1e-8), turned by `random(-spread/2, spread/2)` about a random unit axis
//!   (00551dd0: z = random(-1, 1), angle = random(0, 2 pi));
//! - a second velocity, the damage direction (DAT_00d1672c..34) times `damage_force_speed`;
//! - spin rate: `rot_speed_min + random(0, rot_speed - rot_speed_min)`, its sign random; about the axis `spin_type`
//!   names (0x5b1fe386 / 0xa20af4e1: y; 0xd694e056: z; 0x6483fd62: x; any other: a random unit axis);
//! - start pose: when `random_start_angle` > 0 the piece's matrix is turned by `random(-a, a)` about the spin axis;
//! - place: when `spawn_radius_xy` > 0 it moves by that radius at `theta = random(-pi, pi)` (x - sin theta,
//!   y + cos theta: a fixed distance, not a disc); when `spawn_radius_z` > 0, z moves by `random(-rz, rz)`;
//! - degrees: `rot_speed`, `rot_speed_min`, `spread_angle`, `random_start_angle` are times 0.017453292 (checked in
//!   the disassembly at 00552a85..00552b85); the radii and speeds are used as they are.
//! - `die_condition`: DEBRIS_DIE_REST_TIMER becomes DEBRIS_DIE_SPAWN_TIMER when the parent's +0xb8 bit 0 is set.
//! - `inflict_damage` is kept at +0x60, `die_type` at +0x68, `override_collision` as +0x54 bit 0; the parent object's
//!   handle +0x94 is kept at +0x70 (the damage's source). When die_condition is DEBRIS_DIE_IMPACT or inflict_damage
//!   is above 0 it listens to its body's contacts (key f1e85af0, handler 005564d0 -> 00554090).
//! [game, by absence] `quantity` is read by no code (its hash 9ff31636 is in no instruction): one piece per link.
//!
//! [game] Its life after the throw (the activation 00553840, the messages 00553ab0, the update 00553b70, the die
//! step 00553f40):
//! - activation: its body's masks are edited (message 3680e014, `{id, send set, send clear, take set, take clear,
//!   all, from defaults}`, taken by 006dcda0): it stops SENDING `CAMERA` (and `PUSH` when override_collision is set)
//!   and starts TAKING `CAMERA`. With override_collision, `PUSH` is sent again 0.5 s later (message bb9e4468).
//!   Under DEBRIS_DIE_SPAWN_TIMER with die_time > 0 the die step comes die_time later (message 0135b937); under
//!   DEBRIS_DIE_REST_TIMER it comes die_time after the body first sleeps (00553b70); under DEBRIS_DIE_IMPACT at its
//!   first contact (00554090). Whatever the condition, 12.5 s after the activation it starts to fade (41802d2d).
//! - the die step by die_type: 2709b720 sends its owner a default damage of -1 [stand-in: not ported, the port's
//!   pieces have no owner Destructable], becomes c90410e0 and schedules the die step again after die_time;
//!   c90410e0 starts the fade; 3a07ebf6 starts the fade and stops the contact handler (+0x54 bit 2); any other does
//!   nothing.
//! - the fade: +0x5c starts at 1 and loses 2 a second; it is broadcast as the object's alpha (key 611d70bc); under
//!   0.5 the body sends nothing any more (send clear ffff); at 0 the object is removed (mark_for_removal 005a91e0).
//!   So a fading piece goes 0.5 s after its die step.
//! - its damage (00554090, at each contact, unless +0x54 bit 2): while inflict_damage > 0 (and nothing holds the
//!   piece, +0x50): if the other body SENDS `PUNCH` and the piece TAKES `PUNCH`, the hook `scripted_damage_packet`
//!   00744670 sends the other's object `inflict_damage` (flags 0x400, type 8590b19d, source the piece's +0x70); a
//!   piece that takes `PUNCH` then stops taking it; and inflict_damage becomes 0 WHATEVER it touched. So the damage
//!   is dealt at most once, and only if the very first contact is with a PUNCH sender. [assumed] the contact's first
//!   body is the piece's own (the test reads `*(pair[0]) + 0x9e` against `pair[1] + 0x9c`); for the roster's data the
//!   other order gives the same answer (see `bumblebees_missile_pieces_never_hurt_a_character`).
//! - a contact exists at all only when the two bodies' masks overlap: `(B send & A take) | (B take & A send)` (the
//!   broad phase 006e5e00, `notes\physics.md` 1.13); the solver pushes them apart only where that also meets
//!   `0x4d` (`notes\collision-response.md`, lines 44-49).
//!
//! [data] Bumblebee's missile pieces have the preset `DebrisSmall` (sends nothing, takes `PUSH`); a character's limbs
//! `Character` (sends `PUNCH | WATER`, takes nothing). So a piece never touches a character (no mask meets), the floor
//! (`StaticGround` sends `PUNCH` but the piece does not take it) takes the damage away at the first bounce, and their
//! `inflict_damage` 7 / 10 is never dealt.
//!
//! [stand-in] the flight: gravity 9.81 m/s2 down, the data's damping per 0.032 s step, and a bounce on the level's
//! floor (restitution 0.3, ground friction 0.5) in place of the Lux rigid-body solver (the piece's DynamicsData
//! mass and its Collision shape are not used); the floor bounce counts as a contact with a `StaticGround` body.
//! [stand-in] "the body sleeps" (DEBRIS_DIE_REST_TIMER) is its speed under 0.2 m/s (body_is_sleeping 00556c60 tests the
//! solver's sleep counter). [assumed] the random source is the game's LCG (0x343fd, 0x269ec3) without its clock
//! mixing.

use glam::{Affine3A, Mat3, Quat, Vec3};

use crate::collision_masks::{self as bits, Masks, Presets};
use crate::formats::lxb::Node;
use crate::formats::scene::{self, ObjectDef, PlacedEmitterDef};

pub const DIE_REST_TIMER: u32 = 0xb47a9978;
pub const DIE_SPAWN_TIMER: u32 = 0xb251d249;
pub const DIE_IMPACT: u32 = 0x59968698;
/// The die types the die step 00553f40 knows (their names are in no name table). [game]
pub const DIE_TYPE_HURT_OWNER: u32 = 0x2709b720;
pub const DIE_TYPE_FADE: u32 = 0xc90410e0;
pub const DIE_TYPE_FADE_INERT: u32 = 0x3a07ebf6;
const DEG: f32 = 0.017453292;
/// [stand-in] see the module header.
const GRAVITY: f32 = 9.81;
const RESTITUTION: f32 = 0.3;
const FRICTION: f32 = 0.5;
const STEP: f32 = 0.032;
/// [game 00553840: 0x41480000] every piece starts to fade this long after its activation.
const FADE_AT_LATEST: f32 = 12.5;
/// [game 00553840: 0x3f000000] an override_collision piece sends `PUSH` again after this.
const PUSH_AGAIN_AFTER: f32 = 0.5;
/// [game 00553b70] the fade loses this much alpha a second.
const FADE_RATE: f32 = 2.0;
/// [game, notes\collision-response.md] the bits a contact's masks must meet in.
const RESPONSE: u16 = 0x4d;
/// The level's floor as the stand-in flight touches it: a `StaticGround` body. [data: the preset table]
/// [assumed] the floor a piece lands on is of that preset.
pub const STATIC_GROUND: Masks = Masks {
    send: bits::PUSH | bits::PUNCH | bits::AVATAR_PUSH | bits::GROUND | bits::CAMERA | bits::DETECT,
    receive: 0,
};
/// A character's limb boxes (preset `Character`). [data]
pub const CHARACTER: Masks = Masks { send: bits::PUNCH | bits::WATER, receive: 0 };
/// The packet flags of a piece's damage. [game 00744670: local_44 = 0x400]
pub const DAMAGE_FLAGS: u32 = 0x400;

/// A Debris component's fields. [data] names; angles in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebrisDef {
    pub speed: f32,
    pub speed_min: f32,
    pub rot_speed: f32,
    pub rot_speed_min: f32,
    pub spread: f32,
    pub start_angle: f32,
    pub radius_xy: f32,
    pub radius_z: f32,
    pub force_speed: f32,
    pub die_condition: u32,
    pub die_time: f32,
    pub spin_type: u32,
    /// The DynamicsData's linear and angular damping (per step [assumed]).
    pub linear_damping: f32,
    pub angular_damping: f32,
    /// What the die step does (`DIE_TYPE_*`). [game 00553f40]
    pub die_type: u32,
    /// The damage the piece's first contact may deal. [game 00554090]
    pub inflict_damage: f32,
    /// `override_collision`: the piece sends no `PUSH` for its first 0.5 s. [game 00553840, 00553ab0]
    pub override_collision: bool,
}

fn float(node: Node, name: &str) -> f32 {
    node.get(name).and_then(Node::float).unwrap_or(0.0)
}

fn hash(node: Node, name: &str) -> u32 {
    node.get(name).and_then(|n| n.enum_hash().or_else(|| n.int().map(|v| v as u32))).unwrap_or(0)
}

fn things(object: Node) -> Vec<Node> {
    object.get("attachments").into_iter().flat_map(Node::items).filter_map(|a| a.get("thing")).collect()
}

impl DebrisDef {
    /// The Debris component of an Object, with its Collision's DynamicsData. [data]
    pub fn of_object(object: Node) -> Option<DebrisDef> {
        let things = things(object);
        let d = things.iter().copied().find(|t| t.type_name() == Some("Debris"))?;
        let dynamics = things.iter().copied().filter(|t| t.type_name() == Some("Collision")).find_map(|c| c.get("dynamics"));
        let damping = |name: &str| dynamics.and_then(|n| n.get(name)).and_then(Node::float).unwrap_or(1.0);
        Some(DebrisDef {
            speed: float(d, "speed"),
            speed_min: float(d, "speed_min"),
            rot_speed: float(d, "rot_speed") * DEG,
            rot_speed_min: float(d, "rot_speed_min") * DEG,
            spread: float(d, "spread_angle") * DEG,
            start_angle: float(d, "random_start_angle") * DEG,
            radius_xy: float(d, "spawn_radius_xy"),
            radius_z: float(d, "spawn_radius_z"),
            force_speed: float(d, "damage_force_speed"),
            die_condition: hash(d, "die_condition"),
            die_time: float(d, "die_time"),
            spin_type: hash(d, "spin_type"),
            linear_damping: damping("linearDamping"),
            angular_damping: damping("angularDamping"),
            die_type: hash(d, "die_type"),
            inflict_damage: float(d, "inflict_damage"),
            override_collision: hash(d, "override_collision") != 0,
        })
    }
}

/// The game's random source: its LCG, a value in [0, 1). [assumed] without the clock mixing of 00551d60.
#[derive(Clone, Debug, Default)]
pub struct Rng(pub u32);

impl Rng {
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        ((self.0 >> 16) & 0x7fff) as f32 / 32768.0
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.next() * (hi - lo)
    }
    /// 00551dd0: a uniform unit vector.
    fn unit(&mut self) -> Vec3 {
        let z = self.range(-1.0, 1.0);
        let a = self.range(0.0, std::f32::consts::TAU);
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * a.cos(), r * a.sin(), z)
    }
}

/// A thrown piece: where it is, how it is turned from its placed pose, how it moves, and what its Debris
/// component's timers and contact handler have reached.
#[derive(Clone, Debug)]
pub struct Piece {
    pub pos: Vec3,
    pub turn: Quat,
    pub vel: Vec3,
    /// Spin: axis times rate, radians a second.
    pub spin: Vec3,
    pub age: f32,
    pub rest: f32,
    pub def: DebrisDef,
    /// What its body sends and takes now (`with_masks`; none until then). [game]
    pub masks: Masks,
    /// The damage still to be dealt (`inflict_damage` until the first contact). [game 00554090]
    pub damage: f32,
    /// The fade's alpha (+0x5c): 1 until the fade starts. [game 00553b70]
    pub alpha: f32,
    fading: bool,
    inert: bool,
    pushing: bool,
    die_type: u32,
    die_at: Option<f32>,
    rest_scheduled: bool,
}

/// Throws a piece placed at `at` (its placed matrix's position) away from `damage_point`, pushed along
/// `damage_dir`. [game] 00552930, see the module header. Its body's masks are none until `with_masks`.
pub fn launch(def: &DebrisDef, at: Vec3, damage_point: Vec3, damage_dir: Vec3, rng: &mut Rng) -> Piece {
    let mut speed = def.speed;
    if def.speed_min < def.speed {
        speed = def.speed_min + rng.range(0.0, def.speed - def.speed_min);
    }
    let away = at - damage_point;
    let mut dir = if away.length() > 1e-8 { away.normalize() } else { away };
    if def.spread > 0.0 {
        let angle = rng.range(-def.spread * 0.5, def.spread * 0.5);
        dir = Quat::from_axis_angle(rng.unit(), angle) * dir;
    }
    let vel = dir * speed + damage_dir * def.force_speed;
    let mut rate = def.rot_speed;
    if def.rot_speed_min < def.rot_speed {
        rate = def.rot_speed_min + rng.range(0.0, def.rot_speed - def.rot_speed_min);
        if rng.next() < 0.5 {
            rate = -rate;
        }
    }
    let axis = match def.spin_type {
        0x5b1fe386 | 0xa20af4e1 => Vec3::Y,
        0xd694e056 => Vec3::Z,
        0x6483fd62 => Vec3::X,
        _ => rng.unit(),
    };
    let turn = if def.start_angle > 0.0 { Quat::from_axis_angle(axis, rng.range(-def.start_angle, def.start_angle)) } else { Quat::IDENTITY };
    let mut pos = at;
    if def.radius_xy > 0.0 {
        let theta = rng.range(-std::f32::consts::PI, std::f32::consts::PI);
        pos.x += -theta.sin() * def.radius_xy;
        pos.y += theta.cos() * def.radius_xy;
    }
    if def.radius_z > 0.0 {
        pos.z += rng.range(-def.radius_z, def.radius_z);
    }
    // [game 00553840] the die step's timer starts with the activation under DEBRIS_DIE_SPAWN_TIMER.
    let die_at = (def.die_condition == DIE_SPAWN_TIMER && def.die_time > 0.0).then_some(def.die_time);
    Piece {
        pos, turn, vel, spin: axis * rate, age: 0.0, rest: 0.0, def: *def,
        masks: Masks { send: 0, receive: 0 }, damage: def.inflict_damage, alpha: 1.0,
        fading: false, inert: false, pushing: !def.override_collision, die_type: def.die_type, die_at, rest_scheduled: false,
    }
}

impl Piece {
    /// Its body's masks from its Collision (`base`), as the activation edits them: no `CAMERA` (nor `PUSH` under
    /// override_collision) sent, `CAMERA` taken. [game 00553840]
    pub fn with_masks(mut self, base: Masks) -> Self {
        let mut clear = bits::CAMERA;
        if self.def.override_collision {
            clear |= bits::PUSH;
        }
        self.masks = Masks { send: base.send & !clear, receive: base.receive | bits::CAMERA };
        self
    }

    /// Whether the piece's body and one with masks `other` make a contact at all: the broad phase keeps a pair whose
    /// masks overlap RAW, `(A send & B take) | (A take & B send)` (006e5e00, `notes\physics.md` 1.13). [game]
    /// [assumed] the contact message f1e85af0 (which the melee weapon's PUNCH hits also come by) is sent for every
    /// such pair that touches, not only those whose masks also meet in the response's 0x4d.
    pub fn meets(&self, other: Masks) -> bool {
        (other.send & self.masks.receive) | (other.receive & self.masks.send) != 0
    }

    /// Whether the solver pushes the two apart (the response's masks, `& 0x4d`). [game, collision-response.md]
    pub fn pushes(&self, other: Masks) -> bool {
        ((other.send & self.masks.receive) | (other.receive & self.masks.send)) & RESPONSE != 0
    }

    /// The piece touches a body with masks `other`: nothing when the masks do not meet, else its contact
    /// handler runs (`contact`). Returns the damage the other body takes.
    pub fn touch(&mut self, other: Masks) -> Option<f32> {
        if !self.meets(other) {
            return None;
        }
        self.contact(other)
    }

    /// The contact handler 00554090 (see the module header): the damage at most once, consumed by any contact;
    /// DEBRIS_DIE_IMPACT's die step. Returns the damage the other body takes. [game]
    pub fn contact(&mut self, other: Masks) -> Option<f32> {
        if self.inert {
            return None;
        }
        let mut dealt = None;
        if self.damage > 0.0 {
            if other.send & self.masks.receive & bits::PUNCH != 0 {
                dealt = Some(self.damage);
            }
            // [game] message 3680e014 with take clear 2.
            self.masks.receive &= !bits::PUNCH;
            self.damage = 0.0;
        }
        if self.def.die_condition == DIE_IMPACT {
            self.die();
        }
        dealt
    }

    /// The die step 00553f40. [game]
    fn die(&mut self) {
        if self.fading {
            return;
        }
        match self.die_type {
            DIE_TYPE_HURT_OWNER => {
                // [stand-in] its owner's default damage of -1 (00555ce0) is not sent: no owner here.
                self.die_type = DIE_TYPE_FADE;
                if self.def.die_time > 0.0 {
                    self.die_at = Some(self.age + self.def.die_time);
                }
            }
            DIE_TYPE_FADE => self.fading = true,
            DIE_TYPE_FADE_INERT => {
                self.fading = true;
                self.inert = true;
            }
            _ => {}
        }
    }

    /// Moves the piece by `dt` seconds; `floor(p)` is the level's floor height under a point (None: no floor).
    /// Returns false once the piece is to be removed (its fade has run out).
    pub fn step(&mut self, dt: f32, floor: impl Fn(Vec3) -> Option<f32>) -> bool {
        self.age += dt;
        let k = dt / STEP;
        self.vel.z -= GRAVITY * dt;
        self.vel *= self.def.linear_damping.powf(k);
        self.spin *= self.def.angular_damping.powf(k);
        self.pos += self.vel * dt;
        if self.spin.length_squared() > 0.0 {
            self.turn = (Quat::from_scaled_axis(self.spin * dt) * self.turn).normalize();
        }
        if let Some(z) = floor(self.pos) {
            if self.pos.z < z {
                self.pos.z = z;
                if self.vel.z < 0.0 {
                    self.vel.z = -self.vel.z * RESTITUTION;
                }
                self.vel.x *= 1.0 - FRICTION;
                self.vel.y *= 1.0 - FRICTION;
                self.spin *= 1.0 - FRICTION;
                // [stand-in] the bounce is the solver's contact with the floor (whatever the masks: it did bounce).
                let _ = self.contact(STATIC_GROUND);
            }
        }
        let resting = self.vel.length() < 0.2;
        self.rest = if resting { self.rest + dt } else { 0.0 };
        if !self.pushing && self.age >= PUSH_AGAIN_AFTER {
            self.pushing = true;
            self.masks.send |= bits::PUSH;
        }
        if self.def.die_condition == DIE_REST_TIMER && self.def.die_time > 0.0 && resting && !self.rest_scheduled {
            self.rest_scheduled = true;
            self.die_at = Some(self.age + self.def.die_time);
        }
        if self.die_at.is_some_and(|at| self.age >= at - 1e-6) {
            self.die_at = None;
            self.die();
        }
        if self.age >= FADE_AT_LATEST {
            self.fading = true;
        }
        if self.fading {
            self.alpha -= dt * FADE_RATE;
            if self.alpha < 0.5 {
                self.masks.send = 0;
            }
            if self.alpha <= 0.0 {
                self.alpha = 0.0;
                return false;
            }
        }
        true
    }

    /// Whether its fade has started (its alpha is going down).
    pub fn fading(&self) -> bool {
        self.fading
    }

    /// The piece's matrix: its turn after its placed linear part, at its place.
    pub fn matrix(&self, placed: Mat3) -> (Mat3, Vec3) {
        (Mat3::from_quat(self.turn) * placed, self.pos)
    }
}

/// One piece a projectile's blast object throws: a `DetachLink` of the object its `DamageLink` builds, whose own
/// Object has a Debris (Bumblebee's micro missile: five, `detach3..7`, `MicroMissilesCluster.chunk01..05`). [data]
#[derive(Clone)]
pub struct DebrisPiece {
    pub def: DebrisDef,
    /// The piece's own Object: its RenderThing (the chunk mesh) and nodes.
    pub object: std::sync::Arc<ObjectDef>,
    /// Its own ParticleEmitters (the smoke trail `smk_velocitysmoke_trail_nofire`), each on its node.
    pub emitters: Vec<PlacedEmitterDef>,
    /// Its placed turn, uniform scale and place in the projectile's space (`placement`). [game 005522f0]
    pub turn: Quat,
    pub scale: f32,
    pub at: Vec3,
    /// The damage point the ctor throws it away from: its DetachLink's parent's place (005522f0 writes
    /// DAT_00d16738 with the parent's matrix's position). [game]
    pub damage_point: Vec3,
    /// Its Collision primitive's masks (its preset's, or its own pair). [data] [game: the primitive loader]
    pub masks: Masks,
    /// The largest of its Collision cuboid's `extent`s: the reach round its point the port tests a target with.
    /// [stand-in for the cuboid]
    pub radius: f32,
}

impl std::fmt::Debug for DebrisPiece {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let meshes: Vec<&str> = self.object.renders.iter().map(|r| r.model.as_str()).collect();
        f.debug_struct("DebrisPiece").field("def", &self.def).field("meshes", &meshes).field("emitters", &self.emitters.len())
            .field("turn", &self.turn).field("scale", &self.scale).field("at", &self.at).field("damage_point", &self.damage_point)
            .field("masks", &self.masks).field("radius", &self.radius).finish()
    }
}

fn stored(node: Option<Node>) -> Affine3A {
    let v = node.map(Node::floats).unwrap_or_default();
    if v.len() != 12 {
        return Affine3A::IDENTITY;
    }
    crate::pose::affine(&std::array::from_fn(|r| std::array::from_fn(|c| v[r * 3 + c])))
}

/// A node's bind in its object's space (node 0 the object's origin). As `level_render::bind_of`.
fn bind(object: Node, index: i64) -> Affine3A {
    if index <= 0 {
        return Affine3A::IDENTITY;
    }
    stored(object.get("sub_matrices").and_then(|s| s.at((index - 1) as usize)))
}

/// A turn squared up from three axes (x kept, y made square to it, z = x cross y). [assumed: 005715a0's order]
fn squared(m: Mat3) -> Quat {
    let x = m.x_axis.normalize_or(Vec3::X);
    let y = (m.y_axis - x * m.y_axis.dot(x)).normalize_or(Vec3::Y);
    Quat::from_mat3(&Mat3::from_cols(x, y, x.cross(y))).normalize()
}

/// How a link places the object it builds under its parent `parent` (world): 005522f0. Returns the placed turn,
/// the uniform scale and the place, and the parent's place (the damage point). [game] The scale is the mean
/// length of the link matrix's three rows times the parent's (at least 0.05) and the turn is the two squared-up
/// turns, the link's first. [assumed] the place is the link matrix's position carried by the whole parent
/// (its scale included): the arguments of the two compose_affine calls are not read.
pub fn placement(parent: Affine3A, link: Affine3A) -> (Quat, f32, Vec3, Vec3) {
    let mean = |m: Mat3| (m.x_axis.length() + m.y_axis.length() + m.z_axis.length()) / 3.0;
    let (p, l) = (Mat3::from(parent.matrix3), Mat3::from(link.matrix3));
    let scale = (mean(l) * mean(p)).max(0.05);
    let turn = (squared(p) * squared(l)).normalize();
    (turn, scale, parent.transform_point3(Vec3::from(link.translation)), Vec3::from(parent.translation))
}

/// The pieces of a projectile object (its `obj`): the DetachLinks with a Debris of the object its DamageLink
/// builds, placed in the projectile's space. [data] [assumed] the DamageLink places its object as a DetachLink
/// does (`placement`; the builder 00554830's composition is not audited) on the projectile's node `parent`.
pub fn read_pieces<'a>(projectile: Node<'a>, name_of: &dyn Fn(u32) -> Option<&'a str>, presets: &Presets) -> Vec<DebrisPiece> {
    let mut out = Vec::new();
    for link in things(projectile).into_iter().filter(|t| t.type_name() == Some("DamageLink")) {
        let Some(damage) = link.get("thing") else { continue };
        let parent = link.get("parent").and_then(Node::int).unwrap_or(0);
        let (turn, scale, at, _) = placement(bind(projectile, parent), stored(link.get("matrix")));
        let damage_world = Affine3A::from_scale_rotation_translation(Vec3::splat(scale), turn, at);
        for detach in things(damage).into_iter().filter(|t| t.type_name() == Some("DetachLink")) {
            let Some(object) = detach.get("thing") else { continue };
            let Some(def) = DebrisDef::of_object(object) else { continue };
            let Some(object_def) = scene::read_object(object, name_of) else { continue };
            let parent = detach.get("parent").and_then(Node::int).unwrap_or(0);
            let (turn, scale, at, damage_point) = placement(damage_world * bind(damage, parent), stored(detach.get("matrix")));
            let collision = things(object).into_iter().find(|t| t.type_name() == Some("Collision"));
            let primitive = collision.and_then(|c| c.get("primitives")).and_then(|p| p.at(0));
            let int = |name: &str| primitive.and_then(|p| p.get(name)).and_then(Node::int).unwrap_or(0);
            let masks = presets.resolve(int("maskPreset") as u32, int("sendMask"), int("receiveMask"));
            let extent = primitive.and_then(|p| p.path(&["packet", "extent"])).map(Node::floats).unwrap_or_default();
            let radius = extent.iter().take(3).copied().fold(0.0f32, f32::max);
            out.push(DebrisPiece { def, object: std::sync::Arc::new(object_def), emitters: scene::read_placed_emitters(object), turn, scale, at, damage_point, masks, radius });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rock() -> DebrisDef {
        // [data] the escort barrier's Debris_Rock03 piece (mission.str chunk 8); its die_type, damage and override
        // are set here as Bumblebee's missile pieces have them (synthetic).
        DebrisDef {
            speed: 20.0, speed_min: 8.0, rot_speed: 300.0 * DEG, rot_speed_min: 0.0, spread: 180.0 * DEG, start_angle: 90.0 * DEG,
            radius_xy: 30.0, radius_z: 10.0, force_speed: 0.5, die_condition: DIE_SPAWN_TIMER, die_time: 5.0, spin_type: 0xd2041280,
            linear_damping: 0.9995, angular_damping: 0.9995, die_type: DIE_TYPE_FADE, inflict_damage: 7.0, override_collision: true,
        }
    }

    #[test]
    fn a_piece_flies_at_its_speed_lands_and_goes_after_its_time_and_fade() {
        let mut rng = Rng(1);
        let at = Vec3::new(0.0, 0.0, 1.0);
        for _ in 0..50 {
            let p = launch(&rock(), at, Vec3::ZERO, Vec3::ZERO, &mut rng);
            let s = p.vel.length();
            assert!((8.0 - 1e-3..=20.0 + 1e-3).contains(&s), "speed {s}");
            assert!(((p.pos - at).truncate().length() - 30.0).abs() < 1e-3, "a fixed 30 m away in xy");
            assert!((p.pos.z - at.z).abs() <= 10.0 + 1e-3);
            assert!(p.spin.length() <= 300.0 * DEG + 1e-3);
        }
        let mut p = launch(&rock(), at, Vec3::ZERO, Vec3::ZERO, &mut rng);
        let mut t = 0.0f32;
        while p.step(1.0 / 60.0, |_| Some(0.0)) {
            assert!(p.pos.z >= 0.0);
            assert_eq!(p.fading(), t + 1.0 / 60.0 >= 5.0 - 1e-4, "fading from die_time: {t}");
            t += 1.0 / 60.0;
        }
        assert!((t - 5.5).abs() < 0.05, "removed half a second after die_time: {t}");
    }

    #[test]
    fn the_direction_is_away_from_the_damage_point_without_spread() {
        let def = DebrisDef { spread: 0.0, speed_min: 10.0, speed: 10.0, ..rock() };
        let p = launch(&def, Vec3::new(5.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, &mut Rng(7));
        assert!((p.vel - Vec3::new(10.0, 0.0, 0.5)).length() < 1e-4, "{:?}", p.vel);
    }

    #[test]
    fn the_damage_is_dealt_once_and_only_to_a_punch_sender_by_a_punch_taker() {
        let base = Masks { send: bits::PUSH, receive: bits::PUSH | bits::PUNCH };
        let mut p = launch(&rock(), Vec3::Z, Vec3::ZERO, Vec3::ZERO, &mut Rng(3)).with_masks(base);
        assert_eq!(p.masks, Masks { send: 0, receive: bits::PUSH | bits::PUNCH | bits::CAMERA }, "activation edit");
        assert_eq!(p.touch(CHARACTER), Some(7.0));
        assert_eq!(p.masks.receive & bits::PUNCH, 0, "stops taking PUNCH");
        assert_eq!(p.touch(STATIC_GROUND), None, "once");
        // A first contact with something that does not send PUNCH takes the damage away all the same.
        let mut q = launch(&rock(), Vec3::Z, Vec3::ZERO, Vec3::ZERO, &mut Rng(3)).with_masks(base);
        assert_eq!(q.contact(Masks { send: bits::PUSH, receive: 0 }), None);
        assert_eq!(q.damage, 0.0);
        assert_eq!(q.touch(CHARACTER), None);
        // The override's PUSH comes back after 0.5 s; under 0.5 alpha it sends nothing.
        let mut r = launch(&DebrisDef { die_time: 0.1, ..rock() }, Vec3::Z * 100.0, Vec3::ZERO, Vec3::ZERO, &mut Rng(3)).with_masks(base);
        for _ in 0..30 { r.step(0.016, |_| None); }
        assert!(r.fading() && r.alpha < 0.5 && r.masks.send == 0, "{:?} {}", r.masks, r.alpha);
        let mut s = launch(&DebrisDef { die_time: 0.0, ..rock() }, Vec3::Z * 100.0, Vec3::ZERO, Vec3::ZERO, &mut Rng(3)).with_masks(base);
        for _ in 0..40 { s.step(0.016, |_| None); }
        assert_eq!(s.masks.send & bits::PUSH, bits::PUSH);
        // No die_time and no die step: it still fades 12.5 s on and goes half a second later.
        let mut n = 0;
        while s.step(0.016, |_| None) { n += 1; }
        let t = 0.016 * (40 + n + 1) as f32;
        assert!((t - 13.0).abs() < 0.03, "{t}");
    }

    #[test]
    fn a_hurt_owner_piece_fades_a_die_time_after_its_die_step() {
        let def = DebrisDef { die_type: DIE_TYPE_HURT_OWNER, die_time: 0.5, ..rock() };
        let mut p = launch(&def, Vec3::Z * 100.0, Vec3::ZERO, Vec3::ZERO, &mut Rng(5));
        let mut t = 0.0f32;
        while p.step(0.01, |_| None) { t += 0.01; }
        assert!((t - 1.5).abs() < 0.02, "die step at 0.5, again at 1.0, gone at 1.5: {t}");
    }

    fn install() -> Option<std::path::PathBuf> {
        let dir = std::path::PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into()));
        dir.join("characters").join("bumblebee.str").is_file().then_some(dir)
    }

    #[test]
    fn bumblebees_missile_pieces_never_hurt_a_character() {
        let Some(dir) = install() else { return };
        let data = crate::character::load(&dir, "bumblebee").expect("bumblebee");
        let row = data.weapon_effects.iter().find(|r| !r.debris.is_empty()).expect("a round with debris");
        assert_eq!(row.name as i32, -364875044, "the micro missile");
        let pieces = &row.debris;
        assert_eq!(pieces.len(), 5);
        let damage: Vec<f32> = pieces.iter().map(|p| p.def.inflict_damage).collect();
        assert_eq!(damage, [7.0, 10.0, 10.0, 10.0, 7.0]);
        let speeds: Vec<(f32, f32)> = pieces.iter().map(|p| (p.def.speed_min, p.def.speed)).collect();
        assert_eq!(speeds, [(25.0, 45.0), (10.0, 65.0), (15.0, 45.0), (10.0, 75.0), (50.0, 100.0)]);
        for p in pieces {
            assert_eq!((p.def.die_condition, p.def.die_type, p.def.die_time), (DIE_SPAWN_TIMER, DIE_TYPE_FADE, 0.5));
            assert!(p.def.override_collision);
            assert!((p.def.spread - std::f32::consts::PI).abs() < 1e-5 && (p.def.force_speed - 0.5).abs() < 1e-6);
            // DebrisSmall: sends nothing, takes PUSH.
            assert_eq!(p.masks, Masks { send: 0, receive: bits::PUSH });
            assert!(p.object.renders.iter().any(|r| r.model.starts_with("MicroMissilesCluster.chunk0")), "{p:?}");
            assert_eq!(p.emitters.len(), 1);
            let e = &p.emitters[0];
            assert!(e.auto_start && !e.use_rot && e.delete_when_done && !e.instant_kill && e.node == 1);
            // The links' rows are 0.4 to 0.45 long and the parents' 0.665 x 1.5031 (1.0).
            assert!((0.39..0.46).contains(&p.scale), "{}", p.scale);
            assert!((p.at - p.damage_point).length() > 0.5 && (p.at - p.damage_point).length() < 3.0, "{:?} {:?}", p.at, p.damage_point);
            // Either order of the hook's mask test: neither the piece's take nor its send has PUNCH, and a
            // character and a piece make no contact at all; the first floor bounce takes the damage away.
            let mut piece = launch(&p.def, p.at, p.damage_point, Vec3::Y, &mut Rng(1)).with_masks(p.masks);
            assert_eq!((piece.masks.send | piece.masks.receive) & bits::PUNCH, 0);
            assert!(!piece.meets(CHARACTER));
            assert_eq!(piece.touch(CHARACTER), None);
            assert_eq!(piece.damage, p.def.inflict_damage, "no contact, nothing consumed");
            assert!(piece.meets(STATIC_GROUND));
            assert_eq!(piece.touch(STATIC_GROUND), None);
            assert_eq!(piece.damage, 0.0);
            // Thrown at the blast, it lives its 0.5 s and its 0.5 s fade.
            let mut t = 0.0f32;
            while piece.step(1.0 / 60.0, |_| Some(-100.0)) { t += 1.0 / 60.0; }
            assert!((t - 1.0).abs() < 0.05, "{t}");
        }
    }
}
