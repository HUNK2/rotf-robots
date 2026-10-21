//! One step's contact pass for a moving body against static bodies (`notes\physics.md` 1.5): the
//! loop of 006e1bc0 (pair input), the pair cells (`pair_sweep`) and 006e2c10 (the hits in order of
//! t), repeated until a pass has no hit.
//!
//! [game] What is ported, per hit: the time gate on +0x244, the node's removal (a body pair) or
//! the triangle's flag (a soup pair), the mask term, the normal turned toward A, the avatar's FLOOR
//! override (no commit; the separating test uses (0,0,1)), the commit of A's pose at t, the
//! separating test on the step displacements, and the simple path: the predicted end slid onto
//! the contact plane, the approaching normal velocity removed. The end of the step is the pose pass's
//! rule: a committed body ends at its slid end, another moves by its velocity (`PassResult::end`).
//!
//! Not ported: the contact objects and their generator (006e6a10), the persistent-contact solver
//! that runs after the pass (006df7c0 lines 171-397), the arm terms of 006e92f0 / 006e8f00, and
//! the friction term (0 for an avatar holder, bit 0x1000; the robot's pencil and the car's chassis cube,
//! the two bodies this pass is used for, are both avatars).
//! [assumed] the moving body is the pair's A (the cell is [static type][A type]; the same A/B question as
//! `physics_avatar.rs` discrete_block, RD16376d, which is the discrete pass's): the node's
//! holder order is set by the broad phase (006e5e00 lines 200-300, unread). OPEN (2026-10-20): read 005b6a70
//! (`collision_batch_add`, the queue of 006e2200 / 006e2110): the batch row is the LOWER type, and when typeB <
//! typeA it swaps A and B (axis negated, index -1-i, +0x2c/+0x30 exchanged). The pencil is type 6 and every static
//! type here is lower (soup 0, cube 2, cylinder 4, cone 5), so the game's kernel A is the STATIC body in every
//! static pair, while `contact_pass` calls `pair_cell(body.shape, static)` (line ~194), i.e. the moving body as A.
//! The cell index is the same; the kernel's A/B role and so the MPR start side and the normal sign's source differ.
//! CAUTION (lead, 2026-10-20): 005b6a70 queues the DISCRETE CollisionBatch test (006e2200 / 006e2110, kernel
//! 005b4aa0), which the port runs in `physics_avatar` after the pass (body against body only: pencil against pencil
//! has equal types, no swap; the car's cube, type 2, would be A against a robot). Whether this SWEEP pass (006e5e00's
//! nodes into the pair kernels) goes through the same queue is not shown. settles: the call from 006e5e00's node
//! loop into the sweep kernels (does it reach 005b6a70, or call the pair table directly with the holder order?).
//! [trace, C4 2026-10-20] the sweep side is 006e1bc0 (pair setup) -> bin insert 0117ba30 (row = arg2, col = arg1,
//! cell = DAT_00b8db28[row*11+col], run by 005e39a0). The node's +0x10 holder is the first one read into the bin
//! record (006e1db4-006e1dc6 load its shape from [holder+0x1d8]; 006e1e40-006e1f24 build the records and the bin
//! index from the shape slots). Which holder is +0x10 is set when 006e5e00 builds the node, not read: settles: the
//! writes of node +0x10 / +0x14 in 006e5e00 (lines 200-300).

use glam::Vec3;

use crate::pair_sweep::{pair_cell, soup_cell, PairInput, PairRecord, PairShape, SoupTri};

/// [game] 00c02990: the identity affine, a soup node's B pose (006e1bc0).
pub const IDENTITY: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

/// [game] the mask bits the response acts on (006e2c10 line 47).
const RESPONSE_BITS: u16 = 0x4d;
/// [game] GROUND, the bit of B's +0x9c the floor override wants (line 101).
const GROUND: u16 = 0x10;

/// What a static node is swept against.
pub enum StaticTarget<'a> {
    /// A body pair (+0x32 == 0): the shape and its pose (start = end, the body does not move).
    Shape { shape: PairShape<'a>, pose: [f32; 12] },
    /// A soup pair (+0x32 != 0): the triangle records (`pair_sweep::soup_records`), pose the identity.
    Soup(Vec<SoupTri>),
    /// A body pair whose B has a physics body: its current pose (+0x108) and its prediction (+0x168).
    /// [game] B is swept from one to the other (006e1bc0), takes no floor rule (its +0xa4 is set), and the
    /// response is the two-body one (the generator's caller does it: no simple path). [stand-in] B's pose is
    /// not committed here: the port steps one body at a time and B is not the stepping body. [game 006e2c10 lines
    /// 145-152, read 2026-10-20] the game commits B too at t (holder_commit_affine from the record's +0x90, and
    /// B's +0x1d4 bit 0x4000) when B's body is not keyframed and its +0x34 is 0; the port does not write a peer's
    /// pose. settles: (stand-in stays) a port that steps the peers' poses at t; the two-body branch (lines
    /// 400-580) writes only velocities and the slot, never B's pose.
    Body { shape: PairShape<'a>, pose: [f32; 12], predicted: [f32; 12] },
}

/// One pair node: a static body and the moving body.
pub struct StaticNode<'a> {
    pub target: StaticTarget<'a>,
    /// The static holder's +0x9c and +0x9e.
    pub masks: [u16; 2],
    /// The caller's id for the static body (a `StaticWorld` primitive index).
    pub id: usize,
    /// The hull cache words carried into the input (+0xc0 / +0xc4, from the node's +0x18 / +0x1c).
    /// [assumed] they start at 1 and are not written back by the pass (no write seen in 006e2c10). [game, third
    /// pass] the hull walk does write its cache: 005afdc4-005afdca store the current corner ("the cache is m",
    /// gjk_shapes.rs), so the game's tags change from step to step; this port keeps the start values. Read
    /// 2026-10-20 (005afcd0): the climb takes a start corner and moves only on a STRICTLY greater dot, so the
    /// corner it ends on is the same for any start except where two corners tie exactly on the direction. What
    /// is not settled is the tie case, which the port's fixed start may answer differently. settles: an
    /// emulator run of 005afcd0 on a tied direction with the cache at each corner, against the port's walk.
    pub tag: [u32; 2],
    /// The node is still in the pair list (a body pair leaves it on its first accepted hit).
    pub live: bool,
    /// The holder's +0x244, the least t accepted this pass.
    gate: f32,
}

impl<'a> StaticNode<'a> {
    pub fn new(target: StaticTarget<'a>, masks: [u16; 2], id: usize) -> Self {
        StaticNode { target, masks, id, tag: [1, 1], live: true, gate: f32::MAX }
    }
}

/// The avatar's floor rule (006e2c10 lines 100-138): which avatar body takes it and its two heights.
#[derive(Clone, Copy, Debug)]
pub struct AvatarFloor {
    /// Wrapper +0x54: the centre's height above the feet (63/121 of the height, 006e7b60).
    pub centre_above_feet: f32,
    /// Wrapper +0x50: how far above the feet a contact still counts as floor (24/121 of the height).
    pub floor_reach: f32,
    /// The root's +0x1d4 bit 0x10: the tolerance is 1.0 instead of 0.01.
    pub wide_tolerance: bool,
}

/// The moving body, as the holder and its physics body hold it.
pub struct MovingBody<'a> {
    pub shape: PairShape<'a>,
    /// +0x108: the current pose (9 basis floats row by row, then the origin).
    pub pose: [f32; 12],
    /// +0x168: the predicted pose; the pass slides its origin.
    pub predicted: [f32; 12],
    /// +0x1c8: the step's displacement, predicted origin minus current, written by the prediction.
    pub displacement: Vec3,
    /// The physics body's linear velocity (+0xa0).
    pub velocity: Vec3,
    /// +0x9c and +0x9e.
    pub masks: [u16; 2],
    /// Some for an avatar (holder bit 0x1000) whose root has bit 0x2000 or mask bit 0x800.
    /// [assumed] the player's pencil has one of the two. OPEN (2026-10-20): the executable's `or [reg+0x1d4],
    /// 0x2000` sites are 00851662 (in movement_owner_reset 00851600: sets it on the owner's body whenever that body
    /// is set, STEP+0x38), 00852470 (in movement_owner_step 00852220: sets it on the owner's body unless the state
    /// machine's current state (`call [vtable+0xc]`) returns hash 0xfb9f3262, which is in no name table) and
    /// 00866085 (2026-10-20: inside the owner-release function that starts near 00865fa0 and holds the lock xadd
    /// refcounts of +0x3c / +0x38; it ORs 0x2000 into the owner's body (ecx = [owner+0x38]) with no branch around
    /// it, the same owner-set as 00851662). So the robot's body does get 0x2000 in the ordinary case, but the
    /// holder-level condition is not settled. The `and` at 00865fe9 clears 0x00200000, not 0x2000. settles: the
    /// reader of 0x2000 in 006e2c10 / 006e1bc0 (which gate uses it), the name of hash 0xfb9f3262 (00852470's gate),
    /// and the function that starts before 00865fa0 (the owner-release entry, not named).
    pub floor: Option<AvatarFloor>,
}

impl<'a> MovingBody<'a> {
    /// [game] 006e1bc0 -> 006db1f0's +0x1c8: predicted origin minus current origin.
    pub fn new(shape: PairShape<'a>, pose: [f32; 12], predicted: [f32; 12], velocity: Vec3, masks: [u16; 2], floor: Option<AvatarFloor>) -> Self {
        let displacement = Vec3::new(predicted[9] - pose[9], predicted[10] - pose[10], predicted[11] - pose[11]);
        MovingBody { shape, pose, predicted, displacement, velocity, masks, floor }
    }
}

/// One accepted hit of the pass.
#[derive(Clone, Copy, Debug)]
pub struct PassContact {
    /// The node's `id`.
    pub id: usize,
    pub record: PairRecord,
    /// The floor override took it (no commit).
    pub floor: bool,
    /// The response ran (masks set and not separating): the end was slid and the velocity cut.
    pub responded: bool,
    /// The body's linear velocity when this hit was taken: after the cuts of the hits before it in
    /// the pass, before its own (006e2c10 line 242 hands the hit the holder's point velocity then).
    /// [game 006e2c10]
    pub velocity: Vec3,
}

#[derive(Clone, Debug)]
pub struct PassResult {
    /// A's pose was committed at least once this step (+0x1d4 bit 0x4000).
    pub committed: bool,
    pub contacts: Vec<PassContact>,
    /// How many passes ran (the last one found no hit).
    pub passes: usize,
}

impl PassResult {
    /// [game] the pose pass (006df7c0 lines 405-444): a committed body ends at its slid predicted pose;
    /// another moves from its current pose by its velocity over `dt` (`holder_advance_pose`).
    /// [game 006ddf30, third pass] the centre advances by (v + pseudo v) dt. This pass runs no solver, so the
    /// pseudo term is zero here; the solver's pseudo velocity is carried by `physics_body::advance` in the
    /// avatar step (`physics_avatar::step`), which is where a pushed body takes it. Not done here: 006ddf30 also
    /// advances the basis q, which this translation-only advance does not (open for a solver-less car path,
    /// `level_world::move_chassis`). settles: 006ddf30's q step for that path.
    pub fn end(&self, body: &MovingBody, dt: f32) -> [f32; 12] {
        if self.committed {
            return body.predicted;
        }
        let mut out = body.pose;
        out[9] += body.velocity.x * dt;
        out[10] += body.velocity.y * dt;
        out[11] += body.velocity.z * dt;
        out
    }
}

fn origin(m: &[f32; 12]) -> Vec3 {
    Vec3::new(m[9], m[10], m[11])
}

/// [game] the contact pass for one moving body against static nodes (`notes\physics.md` 1.5).
pub fn contact_pass(body: &mut MovingBody, nodes: &mut [StaticNode]) -> PassResult {
    contact_pass_with(body, nodes, |_, _, _| {})
}

/// `contact_pass` with the contact generator's call (006e6a10, line 201 of 006e2c10): `generate` gets the
/// body (its pose committed at t when the hit committed it), the node and the record (normal toward A)
/// of every accepted hit that responds (masks set, not separating), before the simple path runs. The
/// game hands it the other hits too, with no contact (param_3 = 0, param_8 = 0): nothing is added then.
/// For a node whose B has a body (`StaticTarget::Body`) there is no simple path: `generate` does the
/// two-body response (it may change the body's velocity).
pub fn contact_pass_with(body: &mut MovingBody, nodes: &mut [StaticNode], mut generate: impl FnMut(&mut MovingBody, &StaticNode, &PairRecord)) -> PassResult {
    let mut out = PassResult { committed: false, contacts: Vec::new(), passes: 0 };
    loop {
        out.passes += 1;
        // [game] 006e1bc0: both holders' +0x244 back to FLT_MAX, the input from the current and predicted poses
        let mut gate_a = f32::MAX;
        let mut hits: Vec<(usize, PairRecord)> = Vec::new();
        for (i, node) in nodes.iter_mut().enumerate() {
            if !node.live {
                continue;
            }
            node.gate = f32::MAX;
            let (b0, b1) = match &node.target {
                StaticTarget::Shape { pose, .. } => (*pose, *pose),
                StaticTarget::Soup(_) => (IDENTITY, IDENTITY),
                StaticTarget::Body { pose, predicted, .. } => (*pose, *predicted),
            };
            let inp = PairInput { a0: body.pose, a1: body.predicted, b0, b1, tag: node.tag, index: i as u32 };
            let rec = match &node.target {
                StaticTarget::Shape { shape, .. } | StaticTarget::Body { shape, .. } => pair_cell(body.shape, *shape, &inp),
                StaticTarget::Soup(tris) => soup_cell(body.shape, tris, &inp),
            };
            if let Some(r) = rec {
                hits.push((i, r));
            }
        }
        if hits.is_empty() {
            return out;
        }
        // [game 00584e20] sorted by t: a stable three-pass 11-bit radix sort on the raw float bits, ascending
        // (the buckets are counted upward from 0), so equal t keeps the node order. `total_cmp` gives the same
        // order for the non-negative t of a hit.
        hits.sort_by(|a, b| a.1.t.total_cmp(&b.1.t));
        for (i, mut rec) in hits {
            let node = &mut nodes[i];
            if !(rec.t <= gate_a && rec.t <= node.gate) {
                continue;
            }
            // [game] lines 36-43: a body pair leaves the list; a soup pair flags the triangle (a byte 1)
            let b_body = matches!(node.target, StaticTarget::Body { .. });
            match &mut node.target {
                StaticTarget::Shape { .. } | StaticTarget::Body { .. } => node.live = false,
                StaticTarget::Soup(tris) => {
                    if let Some(t) = tris.get_mut(rec.face as usize) {
                        t.flag = f32::from_bits(1);
                    }
                }
            }
            // [game] lines 45-47
            let m = ((node.masks[0] & body.masks[1]) | (node.masks[1] & body.masks[0])) & RESPONSE_BITS;
            let mut respond = m != 0;
            let mut floor = false;
            if respond {
                // [game] lines 50-59: the normal faces A
                let n = rec.normal;
                let o = origin(&body.pose);
                if (n.z * (o.z - rec.point.z) + n.y * (o.y - rec.point.y)) + (o.x - rec.point.x) * n.x < 0.0 {
                    rec.normal = Vec3::new(-0.0 - n.x, -0.0 - n.y, -0.0 - n.z);
                }
                let mut sep_normal = rec.normal;
                // [game] lines 100-118: the floor override, B static (no body) and sending GROUND
                if let (Some(f), true) = (body.floor, node.masks[0] & GROUND != 0 && !b_body) {
                    let up = (body.pose[6] * body.pose[6] + body.pose[7] * body.pose[7]) + body.pose[8] * body.pose[8];
                    let up = (up as f64).sqrt();
                    let tol = if f.wide_tolerance { 1.0 } else { 0.01 };
                    let az = body.pose[11] as f64;
                    let h = (rec.point.z as f64 - (az - up * f.centre_above_feet as f64)) as f32;
                    if (h as f64) <= up * f.floor_reach as f64 + tol {
                        sep_normal = Vec3::Z;
                        floor = true;
                    }
                }
                if !floor {
                    // [game] lines 139-144: commit A at t; a static B has no body and is not committed
                    out.committed = true;
                    body.pose = rec.a_at;
                }
                // [game] lines 162-169: separating on the step displacements (a static B's is 0); the gates take t
                let d = match &node.target {
                    StaticTarget::Body { pose, predicted, .. } => {
                        let db = Vec3::new(predicted[9] - pose[9], predicted[10] - pose[10], predicted[11] - pose[11]);
                        body.displacement - db
                    }
                    _ => body.displacement,
                };
                if 0.0 <= (d.x * sep_normal.x + d.y * sep_normal.y) + d.z * sep_normal.z {
                    respond = false;
                }
                gate_a = rec.t;
                node.gate = rec.t;
            }
            let velocity = body.velocity;
            if respond {
                generate(body, &nodes[i], &rec);
            }
            if respond && !b_body {
                // [game] lines 277-289: the predicted end slid onto the plane, from the current origin
                let n = rec.normal;
                let o = origin(&body.pose);
                let dv = origin(&body.predicted) - o;
                let s = (n.z * dv.z + n.y * dv.y) + dv.x * n.x;
                body.predicted[9] = (dv.x - s * n.x) + o.x;
                body.predicted[10] = o.y + (dv.y - n.y * s);
                body.predicted[11] = o.z + (dv.z - n.z * s);
                // [game] lines 290-300: the approaching normal velocity removed (friction term 0 for an avatar)
                let v = body.velocity;
                let vn = (n.y * v.y + n.z * v.z) + v.x * n.x;
                if vn < 0.0 {
                    body.velocity = Vec3::new(v.x - vn * n.x, v.y - n.y * vn, v.z - n.z * vn);
                }
            }
            out.contacts.push(PassContact { id: nodes[i].id, record: rec, floor, responded: respond, velocity });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(o: Vec3) -> [f32; 12] {
        let mut m = IDENTITY;
        m[9] = o.x;
        m[10] = o.y;
        m[11] = o.z;
        m
    }

    /// The robot's pencil (height 5.4, radius 2.0, `Tuning`; built as `collision_world::pencil_cast` does).
    fn pencil() -> (PairShape<'static>, AvatarFloor, f32) {
        let height = 5.4f32;
        let radius = 2.0f32;
        let gap = height * 0.45454544;
        let cyl = height - gap;
        let h = cyl * 0.5;
        let tip = gap * 0.5;
        let shape = PairShape::Pencil { r: radius, h, a: h + tip, c: h / (tip * tip + radius * radius).sqrt() };
        let centre = h + cyl * 0.45454544;
        (shape, AvatarFloor { centre_above_feet: centre, floor_reach: height * 24.0 / 121.0, wide_tolerance: false }, centre)
    }

    fn floor_slab() -> StaticTarget<'static> {
        // a 20 x 20 x 1 slab whose top is z = 0
        StaticTarget::Shape { shape: PairShape::Cube { half: Vec3::new(10.0, 10.0, 0.5) }, pose: at(Vec3::new(0.0, 0.0, -0.5)) }
    }

    #[test]
    fn a_wall_stops_and_slides_the_body() {
        let (shape, _, centre) = pencil();
        let start = Vec3::new(0.0, 0.0, centre + 1.0);
        let wall = StaticTarget::Shape { shape: PairShape::Cube { half: Vec3::new(0.5, 10.0, 10.0) }, pose: at(Vec3::new(3.0, 0.0, 5.0)) };
        let mut nodes = vec![StaticNode::new(wall, [0x45, 0x45], 7)];
        let v = Vec3::new(20.0, 10.0, 0.0);
        let mut body = MovingBody::new(shape, at(start), at(start + v * 0.1), v, [0x45, 0x45], None);
        let res = contact_pass(&mut body, &mut nodes);
        assert!(res.committed);
        assert_eq!(res.contacts.len(), 1);
        let c = res.contacts[0];
        assert_eq!(c.id, 7);
        assert!(c.responded && !c.floor);
        let end = origin(&res.end(&body, 0.1));
        // the wall's face is x = 2.5; the pencil's radius is 2.0
        assert!(end.x <= 2.5 - 2.0 + 0.05, "{end:?}");
        assert!((end.y - 1.0).abs() < 0.05, "the slide keeps the motion along the wall: {end:?}");
        assert!(body.velocity.x.abs() < 1e-3 && (body.velocity.y - 10.0).abs() < 1e-3, "{:?}", body.velocity);
        assert!(!nodes[0].live, "a body pair is tested once per step");
    }

    #[test]
    fn a_floor_under_the_feet_cuts_the_fall_without_a_commit() {
        let (shape, floor, centre) = pencil();
        // feet 0.05 above the slab, falling 1 m this step, walking along x
        let start = Vec3::new(0.0, 0.0, centre + 0.05);
        let v = Vec3::new(5.0, 0.0, -10.0);
        let mut nodes = vec![StaticNode::new(floor_slab(), [0x10 | 0x45, 0x45], 0)];
        let mut body = MovingBody::new(shape, at(start), at(start + v * 0.1), v, [0x45, 0x45], Some(floor));
        let res = contact_pass(&mut body, &mut nodes);
        assert_eq!(res.contacts.len(), 1);
        assert!(res.contacts[0].floor && res.contacts[0].responded);
        assert!(!res.committed, "the floor rule does not commit");
        assert_eq!(body.velocity.z, 0.0);
        let end = origin(&res.end(&body, 0.1));
        assert!((end.z - start.z).abs() < 1e-5 && (end.x - 0.5).abs() < 1e-5, "{end:?}");
    }

    #[test]
    fn masks_without_response_bits_pass_through() {
        let (shape, _, centre) = pencil();
        let start = Vec3::new(0.0, 0.0, centre + 1.0);
        let wall = StaticTarget::Shape { shape: PairShape::Cube { half: Vec3::new(0.5, 10.0, 10.0) }, pose: at(Vec3::new(3.0, 0.0, 5.0)) };
        let mut nodes = vec![StaticNode::new(wall, [0x20, 0x20], 0)];
        let v = Vec3::new(20.0, 0.0, 0.0);
        let mut body = MovingBody::new(shape, at(start), at(start + v * 0.1), v, [0x45, 0x45], None);
        let res = contact_pass(&mut body, &mut nodes);
        assert!(!res.committed && !res.contacts[0].responded);
        assert_eq!(body.velocity, v);
    }

    #[test]
    fn a_soup_floor_flags_its_triangle_and_keeps_the_node() {
        let (shape, floor, centre) = pencil();
        let soup = crate::collision_soup::Soup {
            vertices: vec![[-10.0, -10.0, 0.0, 1.0], [10.0, -10.0, 0.0, 1.0], [10.0, 10.0, 0.0, 1.0], [-10.0, 10.0, 0.0, 1.0]],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        };
        let tris = crate::pair_sweep::soup_records(&soup, &IDENTITY);
        let mut nodes = vec![StaticNode::new(StaticTarget::Soup(tris), [0x10 | 0x45, 0x45], 3)];
        let start = Vec3::new(2.0, 1.0, centre + 0.05);
        let v = Vec3::new(0.0, 0.0, -10.0);
        let mut body = MovingBody::new(shape, at(start), at(start + v * 0.1), v, [0x45, 0x45], Some(floor));
        let res = contact_pass(&mut body, &mut nodes);
        assert!(nodes[0].live, "a soup node stays");
        let StaticTarget::Soup(tris) = &nodes[0].target else { unreachable!() };
        let flagged = tris.iter().filter(|t| t.flag.to_bits() == 1).count();
        assert_eq!(flagged, res.contacts.len());
        assert!(res.contacts.iter().all(|c| c.floor));
        assert_eq!(body.velocity.z, 0.0);
    }

    #[test]
    fn a_turned_chassis_cube_stops_at_a_wall() {
        // Bumblebee's car cube (half 1.5 x 3 x 1, `DriveTuning::body_half`), turned 30 degrees about z,
        // driving at 30 m/s along y into a wall whose face is y = 10.
        let half = Vec3::new(1.5, 3.0, 1.0);
        let q = glam::Quat::from_rotation_z(0.5235988);
        let pose = |o: Vec3| {
            let (x, y, z) = (q * Vec3::X, q * Vec3::Y, q * Vec3::Z);
            [x.x, x.y, x.z, y.x, y.y, y.z, z.x, z.y, z.z, o.x, o.y, o.z]
        };
        let wall = StaticTarget::Shape { shape: PairShape::Cube { half: Vec3::new(20.0, 0.5, 5.0) }, pose: at(Vec3::new(0.0, 10.5, 1.0)) };
        let mut nodes = vec![StaticNode::new(wall, [0x45, 0x45], 3)];
        let start = Vec3::new(0.0, 4.0, 1.6);
        let v = Vec3::new(0.0, 30.0, 0.0);
        let mut body = MovingBody::new(PairShape::Cube { half }, pose(start), pose(start + v * 0.032 * 8.0), v, [0x45, 0x45], None);
        let res = contact_pass(&mut body, &mut nodes);
        assert!(res.committed && res.contacts.len() == 1 && res.contacts[0].responded);
        let end = origin(&res.end(&body, 0.032));
        // the turned cube reaches y + (3 cos 30 + 1.5 sin 30) = y + 3.348 ahead of its centre
        let reach = 3.0 * 0.8660254 + 1.5 * 0.5;
        assert!((end.y + reach - 10.0).abs() < 0.05, "{end:?}");
        assert!(body.velocity.y.abs() < 1e-3, "{:?}", body.velocity);
    }
}
