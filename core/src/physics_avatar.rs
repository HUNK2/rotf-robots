//! One world step of an avatar body against static bodies, the parts of 006df7c0 in their order
//! (`notes\physics.md` 1.9): gravity and damping (006e96c0), the force integration (006de230), the control
//! step with its contact planes (006db490), the prediction (006db1f0), the contact pass with the contact
//! generator (006e2c10 / 006e6a10), the refresh of the body's contacts (006e7550 / 006d7c20), the
//! solver (006df7c0 lines 171-404) and the pose pass (lines 405-444).
//!
//! [game] the persistent part is the holder's contact list (+0x1e4) and the pair nodes' contact
//! pointers (node +0xc); `AvatarBody` keeps both between steps. Everything else of the body is rebuilt
//! from the caller's position and velocity each step: the solver extras are cleared after the pose pass
//! and the avatar does not turn (see the assumptions on `step`).

use glam::Vec3;

use crate::pair_sweep::{PairRecord, PairShape};
use crate::physics_body::{advance, gravity_and_damping, integrate_forces, predict_holder, BodyForces, BodyState};
use crate::physics_contact::{contact_pass_with, AvatarFloor, MovingBody, PassResult, StaticNode, StaticTarget, IDENTITY};
use crate::physics_manifold::{clamp_velocity, control_velocity, ClearGround, Contact, ListedContact, PairBody, Slot, GROUND, LOCAL_A, LOCAL_B, NORMAL, SEPARATION, WORLD_A, WORLD_B};
use crate::physics_solver::SolverBody;

/// [game] 006d7280: +0x1e4, 0.05 unless either shape is of type 0xe (neither the pencil nor a level's).
const MARGIN: f32 = 0.05;

/// Node ids at and above this are other bodies (`PEER_NODE + actor id`); below, `StaticWorld` primitives.
pub const PEER_NODE: usize = 1 << 40;

/// What the body keeps of a contact besides the object: which body it is with.
#[derive(Clone, Copy, Debug)]
pub struct ContactOwner {
    /// The other body's node id: a `StaticWorld` primitive, or `PEER_NODE + actor id` for a body.
    pub node: usize,
    /// A static B's pose (+0x108), for the refresh: a soup's is the identity (006e1bc0). A body's is
    /// taken from its snapshot at each step.
    pub b_pose: [f32; 12],
}

/// The avatar holder's persistent physics state.
#[derive(Clone, Debug, Default)]
pub struct AvatarBody {
    /// The holder's +0x1e4 list. [game] newest first: 006d7280 links a new contact at the head.
    pub contacts: Vec<Contact>,
    pub owners: Vec<ContactOwner>,
    /// The pair nodes' +0xc: per static body, the feature of the contact its node points to. Cleared
    /// with the contact (0117c3c0).
    node_contact: Vec<(usize, u32)>,
    /// The owner's movement step record's ground fields and its ground probe (008524b0, 00853420).
    pub ground: GroundRecord,
}

/// Another body as the stepping one sees it: its snapshot as the frame's steps begin.
/// [stand-in for the one world step of all bodies] the port steps the bodies one after another; each takes
/// every other body as a moving B from its snapshot, does the pair's response and solve from its own side
/// (A) and keeps only its own share, so both sides of a pair hold a contact for it (as both holders' lists
/// do in the game) and nothing passes between them. With equal snapshots each side's share is the one the
/// joint step gives it; the solve's iterations are each side's own.
#[derive(Clone, Copy, Debug)]
pub struct BodySnapshot {
    /// Its actor id (the pair node's id is `PEER_NODE + id`).
    pub id: u32,
    pub shape: SnapshotShape,
    /// Its holder origin (the body's centre) and rotation.
    pub centre: Vec3,
    pub rotation: glam::Quat,
    /// Velocity and conventional angular velocity.
    pub velocity: Vec3,
    pub angular: Vec3,
    pub inv_mass: f32,
    /// World inverse inertia rows: the pencil's diagonal for a robot (`shape_inverse_inertia`), zero for a cube.
    pub inv_inertia: [Vec3; 3],
    /// The holder's +0x9c / +0x9e.
    pub masks: [u16; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapshotShape {
    /// The robot form's pencil: radius and height (`pencil_avatar`).
    Pencil { radius: f32, height: f32 },
    /// The car's cube: half extents.
    Cube { half: Vec3 },
}

impl BodySnapshot {
    fn pair_shape(&self) -> PairShape<'static> {
        match self.shape {
            SnapshotShape::Pencil { radius, height } => pencil_avatar(radius, height).0,
            SnapshotShape::Cube { half } => PairShape::Cube { half },
        }
    }

    /// Its pose now and its prediction (006ddad0: centre + v dt, the rotation stepped by w dt).
    fn poses(&self, dt: f32) -> ([f32; 12], [f32; 12]) {
        let next = crate::vehicle::integrate_rotation(self.rotation, self.angular, dt);
        (affine(self.rotation, self.centre), affine(next, self.centre + self.velocity * dt))
    }

    fn solver_body(&self) -> SolverBody {
        let lane = |v: Vec3| [v.x, v.y, v.z, 0.0];
        SolverBody {
            v: lane(self.velocity),
            w: lane(-self.angular),
            inv_mass: [self.inv_mass; 4],
            inv_inertia: [lane(self.inv_inertia[0]), lane(self.inv_inertia[1]), lane(self.inv_inertia[2])],
            ..Default::default()
        }
    }

    /// Its pair node in the stepping body's pass.
    pub fn node(&self, dt: f32) -> StaticNode<'static> {
        let (pose, predicted) = self.poses(dt);
        StaticNode::new(StaticTarget::Body { shape: self.pair_shape(), pose, predicted }, self.masks, PEER_NODE + self.id as usize)
    }
}

impl AvatarBody {
    fn find(&self, node: usize, feature: u32) -> Option<usize> {
        (0..self.contacts.len()).find(|&i| self.owners[i].node == node && self.contacts[i].feature == feature)
    }

    fn node_pointer(&self, node: usize) -> Option<usize> {
        let &(_, f) = self.node_contact.iter().find(|(n, _)| *n == node)?;
        self.find(node, f)
    }

    fn clear_ground(&mut self, cg: ClearGround) {
        if cg.a {
            for k in &mut self.contacts {
                k.clear_ground(cg.upto);
            }
        }
    }

    /// [game] the generator's contact (006e6a10): the node's +0xc, else 006d7230 (pair and feature), else a
    /// new one (006d7280) at the head of the list; the node then points to it.
    fn contact_for(&mut self, node: usize, feature: u32, mine: &PairBody, other: &PairBody, b_pose: [f32; 12], both_dynamic: bool) -> usize {
        let i = match self.node_pointer(node).or_else(|| self.find(node, feature)) {
            Some(i) => i,
            None => {
                self.contacts.insert(0, Contact::new(mine, other, feature, MARGIN, both_dynamic));
                self.owners.insert(0, ContactOwner { node, b_pose });
                0
            }
        };
        if !self.node_contact.iter().any(|(n, _)| *n == node) {
            self.node_contact.push((node, self.contacts[i].feature));
        }
        i
    }

    /// [game] 006e78c0 and 0117c3c0: the contact leaves the lists and its node's pointer is cleared.
    fn destroy(&mut self, i: usize) {
        let (node, feature) = (self.owners[i].node, self.contacts[i].feature);
        self.node_contact.retain(|&(n, f)| !(n == node && f == feature));
        self.contacts.remove(i);
        self.owners.remove(i);
    }
}

/// [game] the block 006e2c10 builds for the generator (lines 171-187): +0x00 the point in A's frame
/// (record +0xd8), +0x0c in B's (+0xe4), +0x18 and +0x24 the record's point, +0x30 its normal (turned
/// toward A), +0x3c 0, +0x54 0. +0x44 is not written (stack data): 0 here.
pub fn hit_block(rec: &PairRecord) -> Slot {
    let mut b: Slot = [0.0; 22];
    let put = |b: &mut Slot, at: usize, v: Vec3| {
        b[at] = v.x;
        b[at + 1] = v.y;
        b[at + 2] = v.z;
    };
    put(&mut b, LOCAL_A, rec.point_in_a);
    put(&mut b, LOCAL_B, rec.point_in_b);
    put(&mut b, WORLD_B, rec.point);
    put(&mut b, WORLD_A, rec.point);
    put(&mut b, NORMAL, rec.normal);
    b
}

/// The discrete test's shape for a pair shape: the robot's pencil and the car's cube (the others have no
/// discrete support ported yet).
fn posed(shape: PairShape, pose: [f32; 12]) -> Option<crate::mpr::Posed> {
    use crate::mpr::{MprShape, Posed};
    let s = match shape {
        PairShape::Pencil { r, h, a, c } => MprShape::Pencil { r, h, a, c },
        PairShape::Cube { half } => MprShape::Cube { half },
        _ => return None,
    };
    Some(Posed::new(s, pose))
}

/// [game] the block 006e22f0 builds from a discrete result for the generator, seen from this body (the
/// contact's A): +0x00 this body's point in its frame, +0x0c the other's in the other's frame (through the
/// holders' inverses +0x138, each axis in the game's order: the node's A and B differ in y and z), +0x18
/// the other's point, +0x24 this body's, +0x30 the normal toward this body (the record's is from the
/// batch's B toward its A), +0x3c 0, +0x54 the separation ((P - Q) . n, x then z then y).
/// [assumed] the node's A is the batch's A (`i_am_a` is the caller's batch order). OPEN (2026-10-20, read
/// 006e22f0 lines 1-162): the block's A-slot holder is the contact's A (+0x1c0, from contact_init 006d7130) when
/// the node already has a contact (flip: uVar10 = 1 when the node's +0x31 flag is set, its contact is present, and
/// contact+0x1c0 != node+0x10), else the node's +0x10 holder (the generator gets node+0x10 as A and +0x14 as B,
/// line 153). With no contact the batch's A is node+0x10 (006e2200 queues the holders in node order and swaps them
/// on the type order, notes 1.13), so the batch order is the game's for an uncontested pair. The port's contacts
/// are always A = the stepping body (`refresh(&mine, ..)`; `Contact` has no A flag), so the flip never arises here:
/// this block's A-slot is the stepping body's point whatever `i_am_a` is, and `i_am_a` picks only the record side
/// and the order of the y/z sums (node_a / node_b). The disagreement is a new contact made when the other body is
/// node+0x10: the game holds it as A, the port as the stepping body. That depends on RD7c3933's holder order.
/// settles: RD7c3933 (a recording of the holder addresses); then a per-contact A flag if that order differs.
fn discrete_block(rec: &crate::mpr::MprRecord, i_am_a: bool, mine: &crate::mpr::Posed, other: &crate::mpr::Posed) -> Slot {
    let node_a = |m: &[f32; 12], p: Vec3| {
        Vec3::new(
            ((m[3] * p.y + m[6] * p.z) + m[0] * p.x) + m[9],
            ((m[1] * p.x + m[7] * p.z) + m[4] * p.y) + m[10],
            ((m[5] * p.y + m[8] * p.z) + m[2] * p.x) + m[11],
        )
    };
    let node_b = |m: &[f32; 12], p: Vec3| {
        Vec3::new(
            ((m[3] * p.y + m[6] * p.z) + m[0] * p.x) + m[9],
            ((m[1] * p.x + m[4] * p.y) + m[7] * p.z) + m[10],
            ((m[2] * p.x + m[5] * p.y) + m[8] * p.z) + m[11],
        )
    };
    let (p, q, n) = if i_am_a {
        (rec.pa, rec.pb, rec.normal)
    } else {
        (rec.pb, rec.pa, Vec3::new(-0.0 - rec.normal.x, -0.0 - rec.normal.y, -0.0 - rec.normal.z))
    };
    let (lp, lq) = if i_am_a {
        (node_a(&mine.inverse, p), node_b(&other.inverse, q))
    } else {
        (node_b(&mine.inverse, p), node_a(&other.inverse, q))
    };
    let mut s: Slot = [0.0; 22];
    let put = |s: &mut Slot, at: usize, v: Vec3| {
        s[at] = v.x;
        s[at + 1] = v.y;
        s[at + 2] = v.z;
    };
    put(&mut s, LOCAL_A, lp);
    put(&mut s, LOCAL_B, lq);
    put(&mut s, WORLD_B, q);
    put(&mut s, WORLD_A, p);
    put(&mut s, NORMAL, n);
    s[SEPARATION] = ((p.x - q.x) * n.x + (p.z - q.z) * n.z) + (p.y - q.y) * n.y;
    s
}

/// [game] 006e6a10 lines 41-56 for an avatar A (holder bit 0x1000, its root taking the floor rule) and a
/// B with no body sending GROUND: the block's ground mark +1 when A's world point (+0x2c) lies no more
/// than the floor reach (006e94e0) + 0.01 above A's feet (origin z - 006e9550).
pub fn ground_mark(block: &mut Slot, origin_z: f32, floor: &AvatarFloor) {
    let h = (block[WORLD_A + 2] as f64 - (origin_z as f64 - floor.centre_above_feet as f64)) as f32;
    if (h as f64) <= floor.floor_reach as f64 + 0.01f32 as f64 {
        block[GROUND] = 1.0;
    }
}

/// [game] 006e6a10 lines 103-117 for an avatar A: a normal with |nz| < 0.9 loses its z (and is
/// normalised horizontally, 005571b0: divided by the length when it is over 1e-8) when the contact is a
/// ceiling (+0x1ec), both roots are avatars, or nz < -0.001.
pub fn flatten_steep(block: &mut Slot, ceiling: bool, both_avatars: bool) {
    let nz = block[NORMAL + 2];
    if nz.abs() < 0.9 && (ceiling || both_avatars || nz < -0.001) {
        block[NORMAL + 2] = 0.0;
        let (x, y, z) = (block[NORMAL], block[NORMAL + 1], block[NORMAL + 2]);
        let len = ((z as f64 * z as f64 + y as f64 * y as f64 + x as f64 * x as f64) as f32 as f64).sqrt() as f32;
        if len > 1e-8 {
            block[NORMAL] = x / len;
            block[NORMAL + 1] = y / len;
            block[NORMAL + 2] = z / len;
        }
    }
}

/// [game] the robot's pencil (006e7e90 for its sizes, 006e7b60 for the wrapper heights): the shape, its
/// floor rule (wrapper +0x54 the centre 63/121 of the height above the feet, +0x50 the reach 24/121) and
/// the centre's height above the feet.
pub fn pencil_avatar(radius: f32, height: f32) -> (PairShape<'static>, AvatarFloor) {
    let gap_part = height * 0.45454544;
    let cylinder = height - gap_part;
    let h = cylinder * 0.5;
    let tip = gap_part * 0.5;
    let shape = PairShape::Pencil { r: radius, h, a: h + tip, c: h / (tip * tip + radius * radius).sqrt() };
    let centre = h + cylinder * 0.45454544;
    (shape, AvatarFloor { centre_above_feet: centre, floor_reach: 0.8 * (cylinder * 0.45454544), wide_tolerance: false })
}

/// [game 006d9190 through 006e8e20, read 2026-10-20 (fifth pass of the robot debts)] The inverse inertia diagonal
/// of the robot's pencil with mass `mass`: the body's +0xc..+0x14 that 006e8e20 writes from the mass record, the
/// record's inertia diagonal being rec[0] = rec[4] and rec[8] (`pencil_mass_properties`, read from the disassembly).
/// The pencil is a cylinder of length `l` and radius `r` with two cones of length `g`: volumes C = pi r^2 g / 3 (each
/// cone) and Y = pi r^2 l (the cylinder), V = 2C + Y, masses mc = mass Y / V and mk = mass C / V.
/// rec[0] = mc (l^2/3 + r^2/4) + 2 mk (0.1 g^2 + 0.15 r^2); rec[8] = mc r^2 / 2 + 0.6 mk r^2.
/// The record's parallel-axis term for the cones (mk (l + g/4)^2, computed at 006d9300) is never stored, so it is
/// left out here too. [game] the body's own frame is the holder's, with no offset (006e8e20 copies the diagonal).
pub fn pencil_inverse_inertia(r: f32, l: f32, g: f32, mass: f32) -> [f32; 3] {
    let pi = std::f32::consts::PI;
    let cone = pi * r * r * g / 3.0;
    let cylinder = pi * r * r * l;
    let volume = 2.0 * cone + cylinder;
    let mc = mass * cylinder / volume;
    let mk = mass * cone / volume;
    let ixx = mc * (l * l / 3.0 + r * r / 4.0) + 2.0 * mk * (0.1 * g * g + 0.15 * r * r);
    let izz = mc * r * r * 0.5 + 0.6 * mk * r * r;
    let inv = |i: f32| if i > 0.0 { 1.0 / i } else { 0.0 };
    [inv(ixx), inv(ixx), inv(izz)]
}

/// The pencil's inverse inertia for a shape from `pencil_avatar`: its cylinder is 2h long and each cone is
/// 2(a - h) long (`pencil_avatar`: the tip is half the gap). Zero for any other shape.
pub fn shape_inverse_inertia(shape: PairShape, mass: f32) -> [f32; 3] {
    match shape {
        PairShape::Pencil { r, h, a, .. } => pencil_inverse_inertia(r, 2.0 * h, 2.0 * (a - h), mass),
        _ => [0.0; 3],
    }
}

/// What one step of the robot on foot gives back to the movement.
#[derive(Clone, Copy, Debug)]
pub struct FeetStep {
    pub feet: Vec3,
    pub velocity: Vec3,
    /// A wall's normal (the first responding contact with a static body that was not a floor).
    pub wall: Option<Vec3>,
    /// The ground test's +0x26c after the step.
    pub standing: bool,
}

/// One on-foot world step asked of a world (`sim::World::step_avatar`).
#[derive(Clone, Copy, Debug)]
pub struct AvatarRequest {
    /// Where the step starts, and the velocity the mode left (before the control step).
    pub feet: Vec3,
    pub velocity: Vec3,
    /// The mode's blended velocity along x and y (the control step's target).
    pub controlled: Vec3,
    pub gravity: f32,
    /// The robot form's avatar descriptor: height, radius, mass.
    pub radius: f32,
    pub height: f32,
    pub mass: f32,
    /// His forward row (the ground test's cone axis), and MoveMode is the mode (STEP+0x3e4).
    pub facing: glam::Vec2,
    pub in_move_mode: bool,
    /// An AI character (its masks and its ground probe), and its actor id.
    pub ai: bool,
    pub id: u32,
    pub dt: f32,
}

impl AvatarRequest {
    /// 1 / mass; 1 when no mass was given (a tuning without avatar descriptors, as in tests).
    pub fn inv_mass(&self) -> f32 {
        if self.mass > 0.0 { 1.0 / self.mass } else { 1.0 }
    }

    /// The snapshot others see of this body as it starts the step.
    pub fn snapshot(&self, masks: [u16; 2]) -> BodySnapshot {
        let (shape, floor) = pencil_avatar(self.radius, self.height);
        let centre = self.feet + Vec3::Z * floor.centre_above_feet;
        let d = shape_inverse_inertia(shape, 1.0 / self.inv_mass());
        BodySnapshot {
            id: self.id,
            shape: SnapshotShape::Pencil { radius: self.radius, height: self.height },
            centre,
            rotation: glam::Quat::IDENTITY,
            velocity: self.velocity,
            angular: Vec3::ZERO,
            inv_mass: self.inv_mass(),
            inv_inertia: [Vec3::X * d[0], Vec3::Y * d[1], Vec3::Z * d[2]],
            masks,
        }
    }
}

/// One step's input for the avatar.
pub struct AvatarInput<'a> {
    pub shape: PairShape<'a>,
    pub floor: AvatarFloor,
    /// The holder's +0x9c / +0x9e.
    pub masks: [u16; 2],
    /// The holder's origin (the body's centre) at the start of the step.
    pub centre: Vec3,
    /// The body's velocity entering the step: what the mode left it (its vertical part, a launch
    /// included) before the control step.
    pub velocity: Vec3,
    /// The velocity the control step asks for (the mode's blend toward its wanted velocity, along x and y).
    pub controlled: Vec3,
    /// The gravity acceleration (positive down) and the linear damping per step (+0x2c).
    pub gravity: f32,
    pub damping: f32,
    /// The body's inverse mass (+0x28: 1 / the avatar descriptor's mass, 006e8e20).
    pub inv_mass: f32,
    /// Its actor id (its own snapshot is left out of the bodies it meets).
    pub id: u32,
    pub dt: f32,
}

#[derive(Clone, Debug)]
pub struct AvatarOutput {
    /// The holder's origin after the pose pass.
    pub centre: Vec3,
    pub velocity: Vec3,
    pub pass: PassResult,
    /// The first responding contact of the pass that is not a floor: its normal (a wall).
    pub wall: Option<Vec3>,
}

/// [game] One world step of the avatar (006df7c0, `notes\physics.md` 1.9) against `nodes`.
/// `recast(node, from, to)` is 005be420 against that static body: the nearest hit's point and normal.
///
/// [game 006e8e20, third pass 2026-10-20] the avatar body's +0x28 is 1/mass, the mass being the avatar
/// descriptor's +0x14 (data, `Tuning::robot_mass`; the port's `AvatarBody::inv_mass` is 1/robot_mass, and
/// `step` takes the caller's `inv_mass`). The test value 0.001 in the unit tests is the tests' own.
/// [game, fifth pass 2026-10-20] A ground robot does NOT turn when pushed. The earlier reading ("a ground robot
/// has only 0x1000") missed the side effect of `pencil_avatar_create` (006e7b60): its `holder_control_set`
/// call (006e8da0, ECX = the body, edi = iVar3) sets bit 0x1 and then bit 0x40 (no 0x20 on a walker: 0x20 is
/// only written by the flight stunts, 0086eea0). With 0x40: `holder_apply_impulse` (006e8f00) zeroes w after
/// each impulse, and `solver_write_back` (006ead90) does not write w back to the body. So the body's w, and
/// the basis that 006ddf30 advances by w (its q step reads +0xac..+0xb4 and the pseudo words), stay at zero.
/// The port's pass keeps the solver's w only inside the solve, and `step` returns the centre and velocity alone.
/// What the inverse inertia still does for a robot: the solver rows' effective mass (006eba50 k, 006e9ef0 and
/// the pair response's k_eff) takes the pencil's diagonal (`shape_inverse_inertia`, from 006d9190 and 006e8e20),
/// so an off-centre contact moves the robot less than a point mass would. settles: nothing further for the turn;
/// the diagonal's frame (the record is about the pencil's own origin, with the cones' parallel-axis term unstored)
/// settles against a recording of a walker's push off-centre, and the 0x40 bit against a robot's flag word at run time.
/// Its holder origin is the body's centre, and it has no children. [game 006dd050] node +0x31 (contacts allowed) is 1 when either
/// holder has a body; the avatar has one, so every pair with it has the flag. Friction is +0x218 and restitution
/// +0x214 [game, read 2026-10-20: 006d7280 / 006d7c20 take max(+0x218) into +0x1bc; 006e5795 and 006e2c10 lines
/// 246-258 and 552-566 take max(+0x214) with min(1, -vn/3) as a bounce]. An avatar body's +0x218 and +0x214 are
/// both 0 (006e0ee0 lines 15-16). Static level bodies take +0x218 = desc+0x30, +0x214 = desc+0x34 (006e10d0).
/// [game 006e0ee0] the avatar's friction (+0x218) is 0. [assumed] the static bodies' are 0 (the level's desc+0x30 in the game; unused: the
/// controlled root, +0x1d4 bits 0 alone, gets no friction rows, 006e92f0). [stand-in] the control
/// step's blend is the caller's (`controlled`): the control vector is taken as (1, 1, 0) toward it, so
/// only the ceiling rule and the planes are the game's here. [game 006db490, third pass] the control vector is the
/// body's +0x2c..+0x34, written by `holder_control_set` (006e8da0) from the owner's per-mode choice (008522 /
/// 854960: basic mode's vector at mode+0xc..0x14, the owner's +0x3d8, or (0.1, 0.1, 0) on the +0x34c branch). The
/// (1, 1, 0) here is the walking mode's value as the trace read it, not the general rule. settles: the other
/// modes' vectors (the +0x34c and +0x3d4 branches of 00852220, not ported) against a recording of a stop or a climb.
pub fn step(body: &mut AvatarBody, inp: &AvatarInput, nodes: Vec<StaticNode>, bodies: &[BodySnapshot],
    recast: impl FnMut(usize, Vec3, Vec3) -> Option<(Vec3, Vec3)>) -> AvatarOutput {
    let dt = inp.dt;
    let entering = inp.velocity;
    let mut state = BodyState {
        scale: [1.0; 3],
        centre: inp.centre.to_array(),
        q: [0.0, 0.0, 0.0, 1.0],
        v: entering.to_array(),
        ..Default::default()
    };
    // [game] 006e96c0 and 006de230: gravity is the force mass x g (006e7b60), so the acceleration is g
    let mass = 1.0 / inp.inv_mass;
    let mut forces = BodyForces { gravity: [0.0, 0.0, -inp.gravity * mass], inv_mass: inp.inv_mass, damping: [inp.damping, 1.0], ..Default::default() };
    gravity_and_damping(&mut state, &mut forces);
    integrate_forces(&mut state, &forces, dt);
    // [game] 006db490: the control step over the holder's contact list
    let listed: Vec<ListedContact> = body
        .contacts
        .iter()
        .zip(&body.owners)
        .map(|(k, o)| ListedContact {
            this_is_a: true,
            count: k.count,
            normal: Vec3::new(k.slots[0][NORMAL], k.slots[0][NORMAL + 1], k.slots[0][NORMAL + 2]),
            ground: k.slots[0][GROUND],
            ceiling: k.ceiling,
            other_dynamic: o.node >= PEER_NODE,
        })
        .collect();
    let v = Vec3::from_array(state.v);
    let animated = inp.centre + Vec3::new(inp.controlled.x, inp.controlled.y, 0.0) * dt;
    if let Some(v) = control_velocity(dt, Vec3::new(1.0, 1.0, 0.0), v, inp.centre, animated, &listed, true) {
        state.v = v.to_array();
    }
    // [game] 006db1f0
    let held = predict_holder(&state, state.centre, dt);
    let mut pose = IDENTITY;
    pose[9..12].copy_from_slice(&state.centre);
    let mut moving = MovingBody::new(inp.shape, pose, held.predicted, Vec3::from_array(state.v), inp.masks, Some(inp.floor));
    let v = moving.velocity;
    // [game 006e8e20] the body's inverse inertia is the pencil's (`shape_inverse_inertia`): the solver rows' effective
    // mass takes it (006eba50 k, 006e9ef0), while the angular velocity it would give is never kept (see `step`'s doc).
    let d = shape_inverse_inertia(inp.shape, mass);
    let sb = SolverBody {
        v: [v.x, v.y, v.z, 0.0],
        inv_mass: [inp.inv_mass; 4],
        inv_inertia: [[d[0], 0.0, 0.0, 0.0], [0.0, d[1], 0.0, 0.0], [0.0, 0.0, d[2], 0.0]],
        ..Default::default()
    };
    let (pass, sb) = pass_and_solve(body, &mut moving, sb, nodes, inp.id, bodies, recast, dt);
    let v = clamp_velocity(Vec3::new(sb.v[0], sb.v[1], sb.v[2]));
    let centre = if pass.committed {
        Vec3::new(moving.predicted[9], moving.predicted[10], moving.predicted[11])
    } else {
        let mut s = BodyState { centre: [moving.pose[9], moving.pose[10], moving.pose[11]], v: v.to_array(), vp: [sb.vp[0], sb.vp[1], sb.vp[2]], ..state };
        advance(&mut s, dt);
        Vec3::from_array(s.centre)
    };
    let wall = pass.contacts.iter().find(|c| c.responded && !c.floor && c.id < PEER_NODE).map(|c| c.record.normal);
    AvatarOutput { centre, velocity: v, pass, wall }
}

fn v3(l: [f32; 4]) -> Vec3 {
    Vec3::new(l[0], l[1], l[2])
}

/// [game] 006e9070 / 006e9160 through 006e8f00: an impulse `p` at the arm `r` into a solver body: v += p
/// (inverse mass), w += I^-1 (p x r) (the game's angular sign).
fn apply_impulse(b: &mut SolverBody, p: Vec3, r: Vec3) {
    let m = b.inv_mass[0];
    b.v = [b.v[0] + p.x * m, b.v[1] + p.y * m, b.v[2] + p.z * m, b.v[3]];
    let t = p.cross(r);
    let rows = &b.inv_inertia;
    for i in 0..3 {
        b.w[i] += v3(rows[i]).dot(t);
    }
}

/// [game] 005be420 against another body, for the refresh's re-cast (006d7c20 lines 121 and 205): the segment
/// `from` .. `to` through the peer's own shape at `b_pose` (its pose as the frame began, the refresh's B pose;
/// its inverse is taken from it, as 005be420 reads the record's B at t and its inverse).
fn peer_recast(peer: &BodySnapshot, b_pose: &[f32; 12], from: Vec3, to: Vec3) -> Option<(Vec3, Vec3)> {
    crate::pair_sweep::recast_segment(peer.pair_shape(), b_pose, &crate::pair_sweep::inverse_affine3(b_pose), from, to)
}

/// [game] 006d7c20 lines 190-213: the same call as `peer_recast`, against the stepping body's own shape (its
/// pencil or cube at `pose`, its inverse taken from it), for the refresh's side_is_a branch.
fn own_recast(shape: PairShape, pose: &[f32; 12], from: Vec3, to: Vec3) -> Option<(Vec3, Vec3)> {
    crate::pair_sweep::recast_segment(shape, pose, &crate::pair_sweep::inverse_affine3(pose), from, to)
}

/// [game] The middle of 006df7c0 for one avatar body, shared by the robot and the car: the pass with the
/// generator on each responding hit (006e2c10 / 006e6a10), with the other bodies (`bodies`, all but `me`)
/// as moving B bodies and their two-body response (006e2c10 lines 400-632); the refresh and destroy of
/// 006e7550; and the solver: static rows (006eba50) and pair rows (006ec0e0), the pair rows first in each
/// of the `ITERATIONS` (006e9ef0, then 006e9960), no friction rows (the controlled roots, 006e92f0). `sb`
/// is the body's solver body as it enters (its velocity after the pass is taken from `moving`); returned
/// with the solved velocities. The other bodies' solver bodies are copies: their changes are dropped (each
/// body does its own side, `BodySnapshot`).
#[allow(clippy::too_many_arguments)]
fn pass_and_solve(body: &mut AvatarBody, moving: &mut MovingBody, sb: SolverBody, mut nodes: Vec<StaticNode>, me: u32,
    bodies: &[BodySnapshot], mut recast: impl FnMut(usize, Vec3, Vec3) -> Option<(Vec3, Vec3)>, dt: f32)
    -> (PassResult, SolverBody) {
    use crate::physics_solver::{biases, build_pair_row, build_static_row, solve_pair, solve_static, ITERATIONS};
    let peers: Vec<BodySnapshot> = bodies.iter().filter(|b| b.id != me).copied().collect();
    // [game] 006e5e00: every pair with another body goes on the discrete list (tested below at the poses
    // after the pass); it is swept as well only when this body moves farther this step than 0.8 of its
    // extent along the motion (006dcf00) or than sqrt(1.4), and moves at all (|d|^2 > 0.0001).
    // [assumed] the gate is the stepping body's: the game gates a node by the first holder of its world
    // list that reaches it, whose order between two bodies is not known. Read 2026-10-20 (006e5e00 lines 83-140):
    // the list holder's own |d|^2 (fVar17, not the relative displacement) and its radius are the test, the other
    // holder's radius is taken only when it has a shape and no body; so this port is exact when the stepping body
    // is the first list holder to reach the node. OPEN (2026-10-20): the order of the world list is read: 00556d20
    // (body_wake_active_insert, its only caller 00556f20 body_set_sleep_counter) puts a body that wakes at the HEAD
    // (DAT_00cf59ec; +0x25c = the next, older holder; +0x258 the previous), and 00556ca0 unlinks it. So the first
    // holder to reach a node is the one woken most recently, and this port has no wake order to take it from.
    // settles: a wake sequence per body in `BodySnapshot` (set where body_set_sleep_counter is ported) and the other
    // holder's +0x1c8 and radius (006dcf00) in the gate, so that the gate takes the more recently woken holder's test.
    let d = moving.displacement;
    let d2 = (d.x * d.x + d.y * d.y) + d.z * d.z;
    let swept = posed(moving.shape, moving.pose).map_or(true, |p| {
        let r = crate::mpr::sweep_radius(&p, d);
        0.0001 < d2 && (r * r < d2 || 1.4 < d2)
    });
    if swept {
        nodes.extend(peers.iter().map(|p| p.node(dt)));
    }
    // The other bodies' solver bodies, at 1.. (0 is this body).
    let mut solver: Vec<SolverBody> = std::iter::once(sb).chain(peers.iter().map(BodySnapshot::solver_body)).collect();
    let peer_index = |node: usize| peers.iter().position(|p| PEER_NODE + p.id as usize == node);
    // [game] a contact with a body that is gone (no snapshot) goes with it (its holder frees it)
    let mut i = 0;
    while i < body.contacts.len() {
        let n = body.owners[i].node;
        if n >= PEER_NODE && !bodies.iter().any(|b| PEER_NODE + b.id as usize == n) {
            body.destroy(i);
            continue;
        }
        if let Some(k) = peer_index(n) {
            body.owners[i].b_pose = peers[k].poses(dt).0;
        }
        i += 1;
    }
    // [game] 006e2c10 with 006e6a10 for each responding hit
    let floor = moving.floor.expect("an avatar body takes the floor rule");
    let mut wa = Vec3::new(solver[0].w[0], solver[0].w[1], solver[0].w[2]);
    let moving_shape = moving.shape;
    let pass = {
        let solver = &mut solver;
        contact_pass_with(moving, &mut nodes, |a, node, rec| {
            let mut block = hit_block(rec);
            let peer = peer_index(node.id);
            if node.masks[0] & 0x10 != 0 && peer.is_none() {
                ground_mark(&mut block, a.pose[11], &floor);
            }
            // node +0xc, else 006d7230, else a new contact (006d7280) at the head of the list
            let feature = rec.face;
            let mine = PairBody { pose: a.pose, avatar: true, dynamic: true, velocity: a.velocity, friction: 0.0 };
            let (b_pose, other) = match (&node.target, peer) {
                (StaticTarget::Body { pose, .. }, Some(k)) => {
                    (*pose, PairBody { pose: *pose, avatar: true, dynamic: true, velocity: peers[k].velocity, friction: 0.0 })
                }
                (StaticTarget::Shape { pose, .. } | StaticTarget::Body { pose, .. }, _) => {
                    (*pose, PairBody { pose: *pose, avatar: false, dynamic: false, velocity: Vec3::ZERO, friction: 0.0 })
                }
                (StaticTarget::Soup(_), _) => (IDENTITY, PairBody { pose: IDENTITY, avatar: false, dynamic: false, velocity: Vec3::ZERO, friction: 0.0 }),
            };
            let i = body.contact_for(node.id, feature, &mine, &other, b_pose, peer.is_some());
            // [game] both roots are avatars between two characters (or a character and the car)
            flatten_steep(&mut block, body.contacts[i].ceiling, peer.is_some());
            let cg = body.contacts[i].insert(&block, true, peer.is_some());
            body.clear_ground(cg);
            let Some(k) = peer else { return };
            // [game] 006e2c10 lines 400-580, the two-body response: the impulse that stops the approach of the
            // two contact points along the (generator's) normal, +P into A at its arm, -P into B at its.
            // [game 006ed510] the solver's bounce row (the average slot's restitution, +0x214) is not built when
            // either holder is an avatar. [game 006e2c10 lines 552-566] this response does use +0x214 (e = max of
            // A's and B's +0x214, no avatar gate), but an avatar body's +0x214 is 0 from its creation
            // (006e0ee0 lines 15-16), so e = 0 for characters. The car is the same holder (fifth pass): its bit
            // 0x1000 is set once by pencil_avatar_create (006e7b60) and never cleared (drive_enter 0085e130 and
            // drive_exit 0085ed50 touch 0x200000 and 0x40 only), so the car is an avatar and e = 0 for it too.
            // No friction impulses for a controlled root (006e92f0).
            let mut n = Vec3::new(block[NORMAL], block[NORMAL + 1], block[NORMAL + 2]);
            if body.contacts[i].ground_side {
                n.z = 0.0;
            }
            let point = rec.point;
            let ra = point - Vec3::new(a.pose[9], a.pose[10], a.pose[11]);
            let pb = peers[k].poses(dt).0;
            let rb = point - Vec3::new(pb[9], pb[10], pb[11]);
            let b = &solver[k + 1];
            let (wb, vb) = (v3(b.w), v3(b.v));
            let rel = (a.velocity + ra.cross(wa)) - (vb + rb.cross(wb));
            let vn = rel.dot(n);
            let arm_term = |s: &SolverBody, r: Vec3| {
                let t = r.cross(n);
                let it = Vec3::new(v3(s.inv_inertia[0]).dot(t), v3(s.inv_inertia[1]).dot(t), v3(s.inv_inertia[2]).dot(t));
                it.cross(r).dot(n)
            };
            let k_eff = arm_term(&solver[0], ra) + arm_term(b, rb) + (solver[0].inv_mass[0] + b.inv_mass[0]);
            let p = ((-0.0 - vn) * (1.0 / k_eff)).max(0.0);
            let impulse = n * p;
            let mut self_body = solver[0];
            self_body.v = [a.velocity.x, a.velocity.y, a.velocity.z, 0.0];
            self_body.w = [wa.x, wa.y, wa.z, 0.0];
            let wb_before = solver[k + 1].w;
            apply_impulse(&mut self_body, impulse, ra);
            a.velocity = v3(self_body.v);
            apply_impulse(&mut solver[k + 1], -impulse, rb);
            // [game 006e8f00, called for A and for B at 006e2c10 lines 573 and 580] bit 0x40 or 0x200000: the
            // impulse is linear only (00556f80). A robot (0x40, holder_control_set at creation, 006e7b60) then has
            // its w zeroed; the car (0x200000) keeps the w it had. The pencil is the robot, the cube the car.
            let is_robot = |s: PairShape| matches!(s, PairShape::Pencil { .. });
            wa = if is_robot(moving_shape) { Vec3::ZERO } else { wa };
            solver[k + 1].w = if matches!(peers[k].shape, SnapshotShape::Pencil { .. }) { [0.0; 4] } else { wb_before };
        })
    };
    // [game] 006e2200, the CollisionBatch (005b6a70 / 005b7150, kernel 005b4aa0) and 006e22f0: the discrete
    // test of each pair with another body, this body at its pose after the pass (+0x108, committed or not),
    // the other as the frame began; an overlap goes to the generator with the feature -1 and leaves the
    // push to the solver's pair rows (no commit, no impulse here).
    let me_posed = posed(moving.shape, moving.pose);
    for p in &peers {
        if (moving.masks[0] & p.masks[1]) | (moving.masks[1] & p.masks[0]) == 0 {
            continue;
        }
        let pose = p.poses(dt).0;
        let (Some(a), Some(b)) = (me_posed, posed(p.pair_shape(), pose)) else { continue };
        // [game] the batch's A is the lower shape type (005b6a70 swaps the pair otherwise).
        // [assumed] between two bodies of one type, the lower actor id. The game's is the node's +0x10, and 006dd050
        // [game] orders the two holders by ADDRESS (lower first, when both have a body; 006dd050 lines 1-30, read
        // 2026-10-20). QUEUED: the addresses are heap addresses of the run, not in the executable: settles: a
        // recording (Cheat Engine, read-only) of each robot holder's address at creation, against its actor id.
        let i_am_a = a.shape.kind() < b.shape.kind() || (a.shape.kind() == b.shape.kind() && me < p.id);
        let Some(rec) = (if i_am_a { crate::mpr::pair_test(&a, &b) } else { crate::mpr::pair_test(&b, &a) }) else { continue };
        let block = discrete_block(&rec, i_am_a, &a, &b);
        let node = PEER_NODE + p.id as usize;
        let mine = PairBody { pose: moving.pose, avatar: true, dynamic: true, velocity: moving.velocity, friction: 0.0 };
        let other = PairBody { pose, avatar: true, dynamic: true, velocity: p.velocity, friction: 0.0 };
        let i = body.contact_for(node, u32::MAX, &mine, &other, pose, true);
        let mut block = block;
        flatten_steep(&mut block, body.contacts[i].ceiling, true);
        let cg = body.contacts[i].insert(&block, true, true);
        body.clear_ground(cg);
    }
    // [game] 006e7550: each contact not yet refreshed (+0x1e9 clear), then destroyed when empty and slot 0
    // separates by more than 6
    let mine = PairBody { pose: moving.pose, avatar: true, dynamic: true, velocity: moving.velocity, friction: 0.0 };
    // [game] 006d7c20 lines 190-213: the side_is_a re-cast is against A's own shape (holder +0x1d8, its type from
    // vtable +0x10: 6 for the pencil, 0081e620; its data from +0x8: 005bbe70 = this+4) at the B holder's pose
    // (EDX = [contact+0x1c4]+0x108, see `peer_recast` above). Read 2026-10-20 (006d8178, 006d842a, 006d86d7).
    let my_shape = moving.shape;
    let mut i = 0;
    while i < body.contacts.len() {
        if !body.contacts[i].refreshed {
            body.contacts[i].refreshed = true;
            let owner = body.owners[i];
            let peer = peer_index(owner.node);
            let other = match peer {
                Some(k) => PairBody { pose: owner.b_pose, avatar: true, dynamic: true, velocity: v3(solver[k + 1].v), friction: 0.0 },
                None => PairBody { pose: owner.b_pose, avatar: false, dynamic: false, velocity: Vec3::ZERO, friction: 0.0 },
            };
            // [game 006d7c20 lines 121 and 205] the refresh re-casts a slot through 005be420 when its drift fails,
            // against the other holder at t (EDX = record +0x60, the B pose `owner.b_pose` here); 006e7550 only
            // calls the refresh per contact. A peer is cast on its own shape (`peer_recast`, the same cast as a
            // static's, bit-exact: `pair_sweep::recast_segment`). The side_is_a branch (006d7c20 lines 190-213) casts
            // the stepping body's own shape (`own_recast`, type and data from its vtable +0x10 and +0x8), and the game
            // passes EDX = [contact+0x1c4]+0x108 there too (006d8178 / 006d842a load ebx = the B holder; 006d86d7 pushes
            // edx = ebx+0x108, edi = ebx+0x138): the other holder's pose, not A's. [game 006d86ea] so `owner.b_pose`.
            let cg = body.contacts[i].refresh(&mine, &other, |side_is_a, c, d| {
                if side_is_a {
                    return own_recast(my_shape, &owner.b_pose, c + d, c - d);
                }
                match peer {
                    Some(k) => peer_recast(&peers[k], &owner.b_pose, c + d, c - d),
                    None if owner.node < PEER_NODE => recast(owner.node, c + d, c - d),
                    None => None,
                }
            });
            body.clear_ground(cg);
            if body.contacts[i].count == 0 && 6.0 < body.contacts[i].slots[0][SEPARATION] {
                body.destroy(i);
                continue;
            }
        }
        i += 1;
    }
    // [game] the solver's rows (006df7c0 lines 205-283)
    let v = moving.velocity;
    solver[0].v = [v.x, v.y, v.z, solver[0].v[3]];
    solver[0].w = [wa.x, wa.y, wa.z, 0.0];
    let (normal_bias, deep_bias, pair_bias) = biases(dt);
    let origin = [moving.pose[9], moving.pose[10], moving.pose[11]];
    let mut statics = Vec::new();
    let mut pairs = Vec::new();
    let mut owners_s = Vec::new();
    let mut owners_p = Vec::new();
    for (ci, k) in body.contacts.iter_mut().enumerate() {
        if !k.refreshed || k.count == 0 {
            continue;
        }
        let peer = peer_index(body.owners[ci].node);
        for si in 0..k.count {
            let s = k.slots[si];
            let sep = s[SEPARATION];
            let mut n = [s[NORMAL], s[NORMAL + 1], s[NORMAL + 2], 0.0];
            let stored = [s[crate::physics_manifold::SOLVER], s[crate::physics_manifold::SOLVER + 2], s[crate::physics_manifold::SOLVER + 3]];
            match peer {
                None => {
                    if !(sep < 0.0) {
                        continue;
                    }
                    let vv = v3(solver[0].v);
                    if s[GROUND] != 0.0 && vv.x * n[0] + vv.y * n[1] < 0.0 {
                        n = [0.0, 0.0, s[GROUND], 0.0];
                    }
                    let bias = if k.deep { deep_bias } else { normal_bias };
                    let built = build_static_row(&mut solver[0], 0, origin, [s[WORLD_A], s[WORLD_A + 1], s[WORLD_A + 2]], n, sep, stored, [bias; 4], k.margin, k.friction, false);
                    statics.push(built.row);
                    owners_s.push((ci, si));
                }
                Some(p) => {
                    if !(sep < -k.margin) {
                        continue;
                    }
                    let rel = v3(solver[0].v) - v3(solver[p + 1].v);
                    if s[GROUND] != 0.0 && rel.x * n[0] + rel.y * n[1] < 0.0 {
                        n = [0.0, 0.0, s[GROUND], 0.0];
                    }
                    let pb = body.owners[ci].b_pose;
                    let (head, tail) = solver.split_at_mut(p + 1);
                    let built = build_pair_row(&mut head[0], &mut tail[0], (0, p as u16 + 1), origin, [pb[9], pb[10], pb[11]],
                        [s[WORLD_A], s[WORLD_A + 1], s[WORLD_A + 2]], [s[WORLD_B], s[WORLD_B + 1], s[WORLD_B + 2]], n, sep, stored,
                        [pair_bias; 4], k.margin, k.friction, false);
                    pairs.push(built.row);
                    owners_p.push((ci, si));
                }
            }
        }
        k.refreshed = false;
    }
    for _ in 0..ITERATIONS {
        solve_pair(&mut solver, &mut pairs);
        solve_static(&mut solver, &mut statics);
    }
    use crate::physics_manifold::SOLVER;
    for (row, (ci, si)) in statics.iter().zip(owners_s) {
        body.contacts[ci].slots[si][SOLVER] = row.impulse[0];
        body.contacts[ci].slots[si][SOLVER + 1] = row.pseudo[0];
    }
    for (row, (ci, si)) in pairs.iter().zip(owners_p) {
        body.contacts[ci].slots[si][SOLVER] = row.impulse[0];
        body.contacts[ci].slots[si][SOLVER + 1] = row.pseudo[0];
    }
    (pass, solver[0])
}

/// One world step of the car's chassis cube (the vehicle avatar, 00853a10 / 006e81e0) against static bodies.
/// The car's own code has already integrated gravity, the springs and the tyre forces (`vehicle::Chassis::
/// advance`, compared with the recordings); this is the rest of 006df7c0 for it.
pub struct ChassisInput {
    pub half: Vec3,
    /// The chassis centre (the holder origin) and its rotation now, and the rotation the prediction reaches.
    pub centre: Vec3,
    pub turn: glam::Quat,
    pub next: glam::Quat,
    /// The body's velocity and its CONVENTIONAL angular velocity (world axes; the game stores the negative).
    pub velocity: Vec3,
    pub angular: Vec3,
    pub inv_mass: f32,
    /// The world inverse inertia (rows), as the car's integration applies it.
    pub inv_inertia: [Vec3; 3],
    /// The character's actor id (its own snapshot is left out of the bodies it meets).
    pub id: u32,
    pub dt: f32,
}

#[derive(Clone, Debug)]
pub struct ChassisOutput {
    pub centre: Vec3,
    /// The rotation after the pose pass: `next` when the pass committed, else the advance from the solved
    /// angular velocity with its pseudo part.
    pub rotation: glam::Quat,
    pub velocity: Vec3,
    pub angular: Vec3,
    pub pass: PassResult,
}

fn affine(q: glam::Quat, o: Vec3) -> [f32; 12] {
    let (x, y, z) = (q * Vec3::X, q * Vec3::Y, q * Vec3::Z);
    [x.x, x.y, x.z, y.x, y.y, y.z, z.x, z.y, z.z, o.x, o.y, o.z]
}

/// [game] 006df7c0 from the prediction on, for the car's chassis: the prediction (centre + v dt at `next`),
/// the pass, the refresh, the solver with the cube's arms (the angular rows use the inverse inertia), the
/// clamp, and the pose pass (committed: the slid end at `next`; else centre + (v + pseudo v) dt and the
/// rotation advanced by the solved w + pseudo w, 006ddf30, `vehicle::integrate_rotation`).
/// [game] the cube and its floor heights (wrapper +0x54 = 0.5, +0x50 = 0.9 of the half height, 006e81e0), as
/// `LevelWorld::move_chassis`. [assumed] the car's holder masks are the robot's (A-PH8), its root takes the
/// floor rule (A-PH9), and it gets no friction rows (its root is the controlled one: 006e92f0); the drive
/// form's control step (006db490) is not run (its control vector is unread: the car's vector is written by
/// `holder_control_set` (006e8da0), whose callers are 006e7b60, 007e8b10, 00852220, 00854960 and 00945d80; the car's
/// drive form is not among the direct writers). settles: the car's holder masks as 006df7c0 sets them (A-PH8),
/// and the drive form's control writer (`fn.py callers 00852220` and the drive mode's own writer).
pub fn step_chassis(body: &mut AvatarBody, inp: &ChassisInput, masks: [u16; 2], nodes: Vec<StaticNode>, bodies: &[BodySnapshot],
    recast: impl FnMut(usize, Vec3, Vec3) -> Option<(Vec3, Vec3)>) -> ChassisOutput {
    let dt = inp.dt;
    let (velocity, angular) = (inp.velocity, inp.angular);
    let floor = AvatarFloor { centre_above_feet: inp.half.z * 0.5, floor_reach: inp.half.z * 0.9, wide_tolerance: false };
    let end = Vec3::new(inp.centre.x + velocity.x * dt, inp.centre.y + velocity.y * dt, inp.centre.z + velocity.z * dt);
    let mut moving = MovingBody::new(PairShape::Cube { half: inp.half }, affine(inp.turn, inp.centre), affine(inp.next, end), velocity, masks, Some(floor));
    let lane = |v: Vec3| [v.x, v.y, v.z, 0.0];
    let sb = SolverBody {
        v: lane(velocity),
        w: lane(-angular),
        inv_mass: [inp.inv_mass; 4],
        inv_inertia: [lane(inp.inv_inertia[0]), lane(inp.inv_inertia[1]), lane(inp.inv_inertia[2])],
        ..Default::default()
    };
    let (pass, sb) = pass_and_solve(body, &mut moving, sb, nodes, inp.id, bodies, recast, dt);
    let v = clamp_velocity(Vec3::new(sb.v[0], sb.v[1], sb.v[2]));
    let angular = -Vec3::new(sb.w[0], sb.w[1], sb.w[2]);
    let (centre, rotation) = if pass.committed {
        (Vec3::new(moving.predicted[9], moving.predicted[10], moving.predicted[11]), inp.next)
    } else {
        let vp = Vec3::new(sb.vp[0], sb.vp[1], sb.vp[2]);
        let wp = -Vec3::new(sb.wp[0], sb.wp[1], sb.wp[2]);
        let o = Vec3::new(moving.pose[9], moving.pose[10], moving.pose[11]);
        (o + (v + vp) * dt, crate::vehicle::integrate_rotation(inp.turn, angular + wp, dt))
    };
    ChassisOutput { centre, rotation, velocity: v, angular, pass }
}

/// The movement step record's fields the owner's ground test (008524b0) reads and writes
/// (`notes\character-ground-test.md`, `notes\character-height-fields.md`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundRecord {
    /// +0x26c: on the ground (what the state machine's `checkOnGround` reads).
    pub grounded: bool,
    /// +0x334: the height above the ground past 1.25 when not grounded (1000 at construction).
    pub excess: f32,
    /// +0x340..+0x348: the probe's hit normal as last copied.
    pub last_normal: [f32; 3],
    /// +0x34c: the slippery push ran; +0x34d: a contact point lies in the cone (its reader is unread).
    pub pushed: bool,
    pub cone: bool,
    /// The ground probe embedded at +0x270 (00853420 submits it, the world step answers it): +0x2d0 done,
    /// +0x324 the hit distance (FLT_MAX: no hit within 10 m), +0x2ac..+0x2b4 the hit normal; +0x33c its radius.
    pub probe_done: bool,
    pub probe_distance: f32,
    pub probe_normal: [f32; 3],
    pub probe_radius: f32,
}

impl Default for GroundRecord {
    /// [game] 008511f0 and the probe's initialiser 0073a370: +0x334 = 1000, +0x324 = FLT_MAX, no result yet.
    /// [game 008511f0, third pass] +0x26c starts clear: the constructor stores the byte 0 at +0x26c.
    fn default() -> Self {
        GroundRecord {
            grounded: false,
            excess: 1000.0,
            last_normal: [0.0; 3],
            pushed: false,
            cone: false,
            probe_done: false,
            probe_distance: f32::MAX,
            probe_normal: [0.0; 3],
            probe_radius: 0.0,
        }
    }
}

/// [game] +0x330: the probe starts this far above the character (008511f0, never written again).
pub const PROBE_LIFT: f32 = 1.25;
/// [game] 00853420: the probe's length, down from its start.
pub const PROBE_LENGTH: f32 = 10.0;

/// What the ground test needs of the owner besides the record.
#[derive(Clone, Copy, Debug)]
pub struct GroundOwner {
    /// The holder's origin (+0x12c) and up row (+0x120), the avatar wrapper's +0x54 (the centre's height above
    /// the feet, so P - U gap is the feet).
    pub origin: [f32; 3],
    pub up: [f32; 3],
    pub gap: f32,
    /// The body's linear velocity (+0xa0).
    pub velocity: [f32; 3],
    /// The vector at STEP+0x2c's +0x1c: the cone test's axis, the forward row [game 008524b0] (the
    /// ledger's note on the character ground test, whose port `ground_test` is bit-exact against the
    /// emulator in `gen_refs_ground.py`'s 1000 cases).
    pub cone: [f32; 3],
    /// STEP+0x3e4: MoveMode is the mode (its enter 00875eb0 sets it, its exit 00875ec0 clears it).
    pub in_move_mode: bool,
    /// The probe hit a body with physics (+0x318 / +0x31c, the held handles).
    pub held: bool,
    /// R1 = STEP+0x140[slot] (24/121 of the form's height), R2 = STEP+0x170[slot] (the height).
    pub r1: f32,
    pub r2: f32,
}

fn clamp_component(c: f32) -> f32 {
    // [game] the clamp of 008524b0 and 00557000: (-100000, 100000], -100000 taking a NaN
    if !(-100_000.0 < c) {
        -100_000.0
    } else if 100_000.0 <= c {
        100_000.0
    } else {
        c
    }
}

/// [game] 008524b0 for a character on foot (not the drive form): the contact points under the owner, the
/// slippery push, the lowest point, the cone flag, and the grounded decision with its landing correction.
/// `contacts` is the holder's +0x1e4 list in order: (the other body sends SLIPPERY 0x200, the slot count,
/// the slots). Returns the body velocity after the push and the landing set.
pub fn ground_test<'a>(rec: &mut GroundRecord, o: &GroundOwner, contacts: impl IntoIterator<Item = (bool, usize, &'a [Slot; 5])>) -> [f32; 3] {
    rec.pushed = false;
    rec.cone = false;
    let g = o.gap;
    let bz = o.origin[2] - o.up[2] * g;
    let bx = o.origin[0] - o.up[0] * g;
    let by = o.origin[1] - o.up[1] * g;
    let mut points: Vec<[f32; 3]> = Vec::with_capacity(10);
    let mut normal: Option<[f32; 3]> = None;
    for (slippery, count, slots) in contacts {
        for s in slots.iter().take(count) {
            if points.len() < 10 {
                points.push([s[WORLD_A] - bx, s[WORLD_A + 1] - by, s[WORLD_A + 2] - bz]);
            }
            if normal.is_none() && slippery {
                // [game] the slot's normal moved off the vertical: (x - x k, y - y k, -z k), k = x^2 + y^2
                let (x, y, z) = (s[NORMAL], s[NORMAL + 1], s[NORMAL + 2]);
                let k = (x * x + y * y) + z * 0.0;
                normal = Some([x - x * k, y - y * k, -0.0 - z * k]);
            }
        }
    }
    let mut v = o.velocity;
    if let Some(n) = normal {
        let (sx, sy, sz) = (n[0] + 0.0, n[1] + 0.0, n[2] + 0.0);
        let b8 = (sx * sx + sz * sz) + sy * sy;
        if 0.01 < b8 {
            let len = (b8 as f64).sqrt();
            let (mut ux, mut uy, mut uz) = (sx, sy, sz);
            if 1e-8 < len {
                let inv = 1.0 / len as f32;
                ux = inv * sx;
                uy = sy * inv;
                uz = sz * inv;
            }
            v = [clamp_component(ux * 5.0 + v[0]), clamp_component(v[1] + uy * 5.0), clamp_component(v[2] + uz * 5.0)];
            rec.pushed = true;
        }
    }
    let (mut min_z, mut min_r) = (f32::MAX, f32::MAX);
    let [ca, cb, cc] = o.cone;
    for p in &points {
        let [x, y, z] = *p;
        if z < min_z {
            min_z = z;
            min_r = ((x * x + y * y) as f64).sqrt() as f32;
        }
        let dot = (y * cb + cc * z) + x * ca;
        if 0.0 < dot && x * x + y * y < dot * dot * 2.0 {
            rec.cone = true;
        }
    }
    if !rec.probe_done {
        return v;
    }
    let h = rec.probe_radius + rec.probe_distance;
    let enter = !o.in_move_mode || !rec.grounded || o.held || PROBE_LIFT + 2.5 < h || (o.r1 * 0.3 <= min_z && min_z != f32::MAX);
    if enter {
        if h <= PROBE_LIFT + 0.01 || (min_z < o.r1 * 0.9 && min_r < o.r2 * 0.9) {
            rec.grounded = true;
            rec.excess = 0.0;
        } else {
            rec.grounded = false;
            rec.excess = h - PROBE_LIFT;
        }
    } else {
        rec.grounded = true;
        rec.excess = 0.0;
        rec.last_normal = rec.probe_normal;
        let gain = h - PROBE_LIFT;
        let n = rec.last_normal;
        if 0.0 < gain && 0.05 < n[2] {
            // [game] the landing: follow the ground's slope and pull down by 30 times the height gained
            let d = ((n[1] * v[1] + n[2] * 0.0) + v[0] * n[0]) / n[2] + gain * 30.0;
            let mut vn = -0.0 - d;
            if vn < 0.0 {
                vn *= 0.8;
            }
            if 1.0 < v[2] && 20.0 < v[2] - vn {
                vn = 1.0;
            }
            if vn <= v[2] {
                v[2] = vn;
            }
            v = clamp_velocity(Vec3::from_array(v)).to_array();
        }
    }
    rec.last_normal = rec.probe_normal;
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_contact::StaticTarget;

    /// The robot's pencil (height 5.4, radius 2.0), as `LevelWorld::move_pencil` builds it.
    fn pencil() -> (PairShape<'static>, AvatarFloor, f32) {
        let (shape, floor) = pencil_avatar(2.0, 5.4);
        (shape, floor, floor.centre_above_feet)
    }

    fn at(o: Vec3) -> [f32; 12] {
        let mut m = IDENTITY;
        m[9..12].copy_from_slice(&o.to_array());
        m
    }

    fn slab() -> StaticNode<'static> {
        let t = StaticTarget::Shape { shape: PairShape::Cube { half: Vec3::new(50.0, 50.0, 0.5) }, pose: at(Vec3::new(0.0, 0.0, -0.5)) };
        StaticNode::new(t, [0x10 | 0x45, 0x45], 1)
    }

    fn wall() -> StaticNode<'static> {
        let t = StaticTarget::Shape { shape: PairShape::Cube { half: Vec3::new(0.5, 50.0, 20.0) }, pose: at(Vec3::new(10.5, 0.0, 10.0)) };
        StaticNode::new(t, [0x45, 0x45], 2)
    }

    fn input(shape: PairShape<'static>, floor: AvatarFloor, centre: Vec3, velocity: Vec3) -> AvatarInput<'static> {
        AvatarInput { shape, floor, masks: [0x45; 2], centre, velocity, controlled: velocity, gravity: 60.0, damping: 0.9995,
            inv_mass: 0.001, id: 1, dt: 0.032 }
    }

    /// Two robots coasting at each other on the slab (the control's target is their own velocity, so only
    /// the physics changes it), stepped one after the other from the frame's snapshots as the hosts do: the
    /// two-body response stops both approaches alike, and each keeps a contact for the pair.
    #[test]
    fn two_coasting_bodies_stop_each_other() {
        let (shape, floor, c) = pencil();
        let (mut a, mut b) = (AvatarBody::default(), AvatarBody::default());
        let (mut pa, mut pb) = (Vec3::new(-6.0, 0.0, c), Vec3::new(6.0, 0.0, c));
        let (mut va, mut vb) = (Vec3::new(6.0, 0.0, 0.0), Vec3::new(-6.0, 0.0, 0.0));
        let snap = |id: u32, centre: Vec3, v: Vec3| BodySnapshot {
            id,
            shape: SnapshotShape::Pencil { radius: 2.0, height: 5.4 },
            centre,
            rotation: glam::Quat::IDENTITY,
            velocity: v,
            angular: Vec3::ZERO,
            inv_mass: 0.001,
            inv_inertia: [Vec3::ZERO; 3],
            masks: [0x45; 2],
        };
        for _ in 0..60 {
            let (sa, sb) = (snap(1, pa, va), snap(2, pb, vb));
            let out = step(&mut a, &input(shape, floor, pa, va), vec![slab()], &[sa, sb], |_, _, _| None);
            (pa, va) = (out.centre, out.velocity);
            let mut inp = input(shape, floor, pb, vb);
            inp.id = 2;
            let out = step(&mut b, &inp, vec![slab()], &[sa, sb], |_, _, _| None);
            (pb, vb) = (out.centre, out.velocity);
        }
        assert!(pa.x < pb.x, "{pa:?} {pb:?}");
        assert!(pb.x - pa.x > 3.0, "they stop about touching (two radii of 2.0): {pa:?} {pb:?}");
        assert!(va.x.abs() < 0.5 && vb.x.abs() < 0.5, "{va:?} {vb:?}");
        assert!((pa.x + pb.x).abs() < 0.1, "equal masses meet in the middle: {pa:?} {pb:?}");
        assert!(a.owners.iter().any(|o| o.node == PEER_NODE + 2));
        assert!(b.owners.iter().any(|o| o.node == PEER_NODE + 1));
    }

    /// [game 006d9190 / 006e8e20] The pencil's inverse inertia diagonal at unit sizes, worked by hand: with
    /// l = g = r = m = 1 the volumes are C = pi/3 (each cone) and Y = pi, so V = 5 pi/3, mc = 3/5, mk = 1/5;
    /// Ixx = 3/5 (1/3 + 1/4) + 2 (1/5) (0.1 + 0.15) = 0.45 and Izz = 3/5 (1/2) + 0.6 (1/5) = 0.3 + 0.12 = 0.42.
    #[test]
    fn pencil_inverse_inertia_matches_the_mass_record() {
        let d = pencil_inverse_inertia(1.0, 1.0, 1.0, 1.0);
        assert!((d[0] - 1.0 / 0.45).abs() < 1e-4, "{d:?}");
        assert_eq!(d[0], d[1]);
        assert!((d[2] - 1.0 / 0.42).abs() < 1e-4, "{d:?}");
        // The robot's pencil: the snapshot carries the same diagonal, and the shape helper reads its cylinder and cones.
        let req = AvatarRequest { feet: Vec3::ZERO, velocity: Vec3::ZERO, controlled: Vec3::ZERO, gravity: 60.0, radius: 2.0,
            height: 5.4, mass: 1000.0, facing: glam::Vec2::X, in_move_mode: false, ai: false, id: 1, dt: 0.032 };
        let snap = req.snapshot([0x45; 2]);
        let (shape, _) = pencil_avatar(2.0, 5.4);
        let want = shape_inverse_inertia(shape, 1000.0);
        // The snapshot's mass is 1 / (1 / 1000): the same diagonal to a few ulps.
        assert!((snap.inv_inertia[0].x - want[0]).abs() <= 1e-6 * want[0], "{:?} {want:?}", snap.inv_inertia);
        assert!((snap.inv_inertia[2].z - want[2]).abs() <= 1e-6 * want[2], "{:?} {want:?}", snap.inv_inertia);
        assert!(want[0] > 0.0 && want[2] > 0.0, "{want:?}");
    }

    /// Two robots WALKING at each other (the ground control (1, 1, 0) asks for 6 m/s every step, [trace]).
    /// At 0.19 m a step they are never swept against each other (006dcf00: 0.8 of the 2 m radius); the
    /// discrete test (005b4aa0) gives the pair a contact each step and the solver's pair rows hold them
    /// apart, a little overlapped, standing still (`notes\physics.md` 1.13). Before the broad phase was read
    /// they were swept, committed to their predictions and walked through each other.
    #[test]
    fn walkers_hold_each_other_apart() {
        let (shape, floor, c) = pencil();
        let (mut a, mut b) = (AvatarBody::default(), AvatarBody::default());
        let (mut pa, mut pb) = (Vec3::new(-6.0, 0.0, c), Vec3::new(6.0, 0.0, c));
        let (mut va, mut vb) = (Vec3::ZERO, Vec3::ZERO);
        let snap = |id: u32, centre: Vec3, v: Vec3| BodySnapshot { id, shape: SnapshotShape::Pencil { radius: 2.0, height: 5.4 }, centre,
            rotation: glam::Quat::IDENTITY, velocity: v, angular: Vec3::ZERO, inv_mass: 0.001, inv_inertia: [Vec3::ZERO; 3], masks: [0x45; 2] };
        let mut last = (pa, pb);
        for _ in 0..80 {
            last = (pa, pb);
            let (sa, sb) = (snap(1, pa, va), snap(2, pb, vb));
            let mut inp = input(shape, floor, pa, va);
            inp.controlled = Vec3::new(6.0, 0.0, 0.0);
            let out = step(&mut a, &inp, vec![slab()], &[sa, sb], |_, _, _| None);
            (pa, va) = (out.centre, out.velocity);
            let mut inp = input(shape, floor, pb, vb);
            inp.id = 2;
            inp.controlled = Vec3::new(-6.0, 0.0, 0.0);
            let out = step(&mut b, &inp, vec![slab()], &[sa, sb], |_, _, _| None);
            (pb, vb) = (out.centre, out.velocity);
        }
        let _ = (va, vb);
        assert!(pa.x < pb.x, "they stay on their sides: {pa:?} {pb:?}");
        let gap = pb.x - pa.x;
        assert!(gap > 3.0 && gap < 4.0, "touching, a little overlapped (two radii of 2.0): {gap}");
        assert!((pa.x + pb.x).abs() < 0.01, "equal masses meet in the middle: {pa:?} {pb:?}");
        assert!((pa - last.0).length() < 1e-3 && (pb - last.1).length() < 1e-3, "standing still: {pa:?} {:?}", last.0);
        assert!(a.owners.iter().any(|o| o.node == PEER_NODE + 2) && b.owners.iter().any(|o| o.node == PEER_NODE + 1));
    }

    /// Dropped from 3 m, the body lands on the slab, keeps a ground-marked contact and stays put.
    #[test]
    fn a_dropped_body_lands_and_rests() {
        let (shape, floor, c) = pencil();
        let mut body = AvatarBody::default();
        let mut centre = Vec3::new(0.0, 0.0, c + 3.0);
        let mut v = Vec3::ZERO;
        for _ in 0..60 {
            let nodes = vec![slab()];
            let out = step(&mut body, &input(shape, floor, centre, v), nodes, &[], |_, _, _| None);
            centre = out.centre;
            v = out.velocity;
        }
        let feet = centre.z - c;
        assert!(feet > -0.2 && feet < 0.05, "feet {feet}");
        assert!(v.z.abs() < 2.0, "{v:?}");
        assert_eq!(body.contacts.len(), 1);
        assert!(body.contacts[0].count >= 1);
        // and it stays there
        let rest = centre;
        for _ in 0..30 {
            let nodes = vec![slab()];
            let out = step(&mut body, &input(shape, floor, centre, v), nodes, &[], |_, _, _| None);
            centre = out.centre;
            v = out.velocity;
        }
        assert!((centre - rest).length() < 0.05, "{rest:?} -> {centre:?}");
    }

    /// Walking into a wall on the slab: the wall stops the body along x, the slab holds it up.
    #[test]
    fn a_walking_body_stops_at_a_wall() {
        let (shape, floor, c) = pencil();
        let mut body = AvatarBody::default();
        let mut centre = Vec3::new(0.0, 0.0, c);
        let mut v = Vec3::ZERO;
        let mut walled = false;
        for _ in 0..90 {
            let nodes = vec![slab(), wall()];
            let mut inp = input(shape, floor, centre, v);
            inp.controlled = Vec3::new(15.0, 3.0, 0.0);
            let out = step(&mut body, &inp, nodes, &[], |_, _, _| None);
            walled |= out.wall.is_some();
            centre = out.centre;
            v = out.velocity;
        }
        assert!(walled);
        assert!(centre.x <= 10.0 - 2.0 + 0.1, "{centre:?}");
        assert!(centre.x > 7.0, "{centre:?}");
        assert!(centre.y > 3.0, "slides along the wall: {centre:?}");
        assert!((centre.z - c).abs() < 0.3, "{centre:?}");
    }

    /// 008524b0 run whole in the emulator (`work\emu\gen_refs_ground.py`, 1,000 cases): the velocity, the
    /// flags, +0x334 and +0x340 bit-exact.
    #[test]
    fn reference_ground_test() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../work/emu/ref/ground_test.tsv");
        let Ok(text) = std::fs::read_to_string(path) else { return };
        let mut cases = 0;
        for line in text.lines().filter(|l| !l.is_empty()) {
            let cols: Vec<&str> = line.split('\t').collect();
            let w: Vec<f32> = cols[..23].iter().map(|s| f32::from_bits(u32::from_str_radix(s, 16).unwrap())).collect();
            let f3 = |i: usize| [w[i], w[i + 1], w[i + 2]];
            let (origin, up, gap) = (f3(0), f3(3), w[6]);
            let (velocity, cone) = (f3(7), f3(10));
            let (dist, radius) = (w[13], w[14]);
            let pnormal = f3(15);
            let (r1, r2) = (w[18], w[19]);
            let prev = f3(20);
            let d = |i: usize| cols[i].parse::<usize>().unwrap();
            let (gate, mv, grounded, n) = (d(23), d(24), d(25), d(26));
            let mut at = 27;
            let mut list: Vec<(bool, usize, [Slot; 5])> = Vec::new();
            let hx = |s: &str| f32::from_bits(u32::from_str_radix(s, 16).unwrap());
            for _ in 0..n {
                let (slippery, count) = (d(at) == 1, d(at + 1));
                at += 2;
                let mut slots = [[0.0f32; 22]; 5];
                for slot in slots.iter_mut().take(4) {
                    for j in 0..3 {
                        slot[WORLD_A + j] = hx(cols[at + j]);
                        slot[NORMAL + j] = hx(cols[at + 3 + j]);
                    }
                    at += 6;
                }
                list.push((slippery, count, slots));
            }
            let want_v = [hx(cols[at]), hx(cols[at + 1]), hx(cols[at + 2])];
            let (want_g, want_p, want_c) = (d(at + 3) == 1, d(at + 4) == 1, d(at + 5) == 1);
            let want_excess = hx(cols[at + 6]);
            let want_last = [hx(cols[at + 7]), hx(cols[at + 8]), hx(cols[at + 9])];
            let mut rec = GroundRecord {
                grounded: grounded == 1,
                excess: 0.0,
                last_normal: prev,
                pushed: false,
                cone: false,
                probe_done: gate == 1,
                probe_distance: dist,
                probe_normal: pnormal,
                probe_radius: radius,
            };
            let owner = GroundOwner { origin, up, gap, velocity, cone, in_move_mode: mv == 1, held: false, r1, r2 };
            let v = ground_test(&mut rec, &owner, list.iter().map(|(s, c, k)| (*s, *c, k)));
            let bits = |a: [f32; 3]| a.map(f32::to_bits);
            assert_eq!(bits(v), bits(want_v), "velocity, case {cases}: {line}");
            assert_eq!((rec.grounded, rec.pushed, rec.cone), (want_g, want_p, want_c), "flags, case {cases}");
            if gate == 1 {
                assert_eq!(rec.excess.to_bits(), want_excess.to_bits(), "excess, case {cases}");
            }
            assert_eq!(bits(rec.last_normal), bits(want_last), "+0x340, case {cases}");
            cases += 1;
        }
        assert_eq!(cases, 1000);
    }

    /// The refresh's re-cast against a peer (006d7c20 lines 121 and 205): the segment is cast on the peer's own
    /// pencil at its frame pose, and misses where the peer is not. [game 006d7c20 lines 121 and 205] the drift test
    /// and the closure are driven through `Contact::refresh` by `a_drifted_slot_is_recast_through_refresh` below
    /// (the step's own closure, not a full `step`).
    #[test]
    fn a_peer_is_recast_on_its_own_shape() {
        let (_, _, c) = pencil();
        let peer = BodySnapshot { id: 2, shape: SnapshotShape::Pencil { radius: 2.0, height: 5.4 }, centre: Vec3::new(6.0, 0.0, c),
            rotation: glam::Quat::IDENTITY, velocity: Vec3::ZERO, angular: Vec3::ZERO, inv_mass: 0.001, inv_inertia: [Vec3::ZERO; 3],
            masks: [0x45; 2] };
        let (b_pose, _) = peer.poses(0.032);
        let (p, n) = peer_recast(&peer, &b_pose, Vec3::new(10.0, 0.0, c), Vec3::new(2.0, 0.0, c)).expect("the segment crosses the pencil");
        assert!((p.x - 8.0).abs() < 0.05 && p.y.abs() < 0.05, "the side of the pencil at its radius: {p:?}");
        assert!(n.x > 0.99, "the side's normal points out of the peer: {n:?}");
        assert!(peer_recast(&peer, &b_pose, Vec3::new(10.0, 10.0, c), Vec3::new(2.0, 10.0, c)).is_none());
    }

    /// A slot whose B point has slid sideways past the drift threshold is re-cast by `Contact::refresh` with the
    /// step's closure (`peer_recast` on the peer's pencil): the slot takes the peer's surface point and normal when
    /// the cast hits, and is dropped when it misses (006d7c20 lines 121 and 205, 006d882a for the drop).
    #[test]
    fn a_drifted_slot_is_recast_through_refresh() {
        let (_, _, c) = pencil();
        let peer = BodySnapshot { id: 2, shape: SnapshotShape::Pencil { radius: 2.0, height: 5.4 }, centre: Vec3::new(6.0, 0.0, c),
            rotation: glam::Quat::IDENTITY, velocity: Vec3::ZERO, angular: Vec3::ZERO, inv_mass: 0.001, inv_inertia: [Vec3::ZERO; 3],
            masks: [0x45; 2] };
        let (b_pose, _) = peer.poses(0.032);
        let origin = Vec3::new(b_pose[9], b_pose[10], b_pose[11]);
        let a = PairBody { pose: IDENTITY, avatar: true, dynamic: true, velocity: Vec3::ZERO, friction: 0.0 };
        let b = PairBody { pose: b_pose, avatar: true, dynamic: true, velocity: Vec3::ZERO, friction: 0.0 };
        // One slot: A's point on the pencil's side (x = 8), B's point 0.5 off it in y (drift > threshold).
        let run = |wa: Vec3, wb: Vec3| {
            let mut k = Contact::new(&a, &b, u32::MAX, 0.05, true);
            let s = &mut k.slots[0];
            s[LOCAL_A..LOCAL_A + 3].copy_from_slice(&wa.to_array());
            s[LOCAL_B..LOCAL_B + 3].copy_from_slice(&(wb - origin).to_array());
            s[NORMAL..NORMAL + 3].copy_from_slice(&[1.0, 0.0, 0.0]);
            k.count = 1;
            let mut calls = 0;
            k.refresh(&a, &b, |side_is_a, from, d| {
                calls += 1;
                assert!(!side_is_a);
                peer_recast(&peer, &b_pose, from + d, from - d)
            });
            (k, calls)
        };
        // Hit: the cast from (11, 0.0, c) to (5, 0.0, c) crosses the side at (8, 0, c); B moves onto it.
        let (k, calls) = run(Vec3::new(8.0, 0.0, c), Vec3::new(8.0, 0.5, c));
        assert_eq!(calls, 1, "the drift test sends the slot to the cast");
        assert_eq!(k.count, 1, "a hit keeps the slot");
        let wb = Vec3::new(k.slots[0][WORLD_B], k.slots[0][WORLD_B + 1], k.slots[0][WORLD_B + 2]);
        assert!((wb - Vec3::new(8.0, 0.0, c)).length() < 0.05, "B takes the peer's surface point: {wb:?}");
        // Miss: the same drift at y = 10, where the peer is not: the slot is dropped.
        let (k, calls) = run(Vec3::new(8.0, 10.0, c), Vec3::new(8.0, 10.5, c));
        assert_eq!(calls, 1);
        assert_eq!(k.count, 0, "a miss drops the slot");
    }

    /// The refresh's side_is_a re-cast (006d7c20 lines 190-213): the segment is cast on the stepping body's own
    /// pencil at its pose, by the same call as a peer's, and misses where the body is not.
    #[test]
    fn a_stepping_body_is_recast_on_its_own_shape() {
        let (shape, _, c) = pencil();
        let pose = at(Vec3::new(6.0, 0.0, c));
        let (p, n) = own_recast(shape, &pose, Vec3::new(10.0, 0.0, c), Vec3::new(2.0, 0.0, c)).expect("the segment crosses the pencil");
        assert!((p.x - 8.0).abs() < 0.05 && p.y.abs() < 0.05, "the side of the pencil at its radius: {p:?}");
        assert!(n.x > 0.99, "the side's normal points out of the body: {n:?}");
        assert!(own_recast(shape, &pose, Vec3::new(10.0, 10.0, c), Vec3::new(2.0, 10.0, c)).is_none());
        let peer = BodySnapshot { id: 2, shape: SnapshotShape::Pencil { radius: 2.0, height: 5.4 }, centre: Vec3::new(6.0, 0.0, c),
            rotation: glam::Quat::IDENTITY, velocity: Vec3::ZERO, angular: Vec3::ZERO, inv_mass: 0.001, inv_inertia: [Vec3::ZERO; 3],
            masks: [0x45; 2] };
        let (b_pose, _) = peer.poses(0.032);
        let from = Vec3::new(10.0, 0.0, c);
        let to = Vec3::new(2.0, 0.0, c);
        assert_eq!(own_recast(shape, &b_pose, from, to), peer_recast(&peer, &b_pose, from, to), "the same cast as a peer's");
        // [game 006d8178 / 006d86d7] the side_is_a call's EDX is the B holder's pose: the stepping body's own pose,
        // here far off, does not decide the cast; the other holder's pose does.
        let far = at(Vec3::new(-20.0, 0.0, c));
        assert!(own_recast(shape, &far, from, to).is_none(), "A's own pose is not the cast's pose");
        assert!(own_recast(shape, &b_pose, from, to).is_some(), "the cast is at the B holder's pose");
    }

    #[test]
    fn a_steep_downward_normal_is_flattened() {
        let mut b: Slot = [0.0; 22];
        b[NORMAL] = 0.6;
        b[NORMAL + 2] = -0.8;
        flatten_steep(&mut b, false, false);
        assert_eq!((b[NORMAL], b[NORMAL + 1], b[NORMAL + 2]), (1.0, 0.0, 0.0));
        let mut b: Slot = [0.0; 22];
        b[NORMAL] = 0.6;
        b[NORMAL + 2] = 0.8;
        flatten_steep(&mut b, false, false);
        assert_eq!(b[NORMAL + 2], 0.8, "an upward slope keeps its normal");
    }
}
