//! The persistent contact object of a body pair and the avatar's control step over its contacts
//! (`notes\physics.md` 1.6).
//!
//! [game] A contact object (500 bytes, allocated by 006d7280, set up by 006d7130) belongs to one
//! pair of holders and one feature (a soup triangle's index, or -1). It holds up to four SLOTS of
//! 0x58 bytes (+0x1b8 the count) and, at index `count`, the slots' average. The contact pass's
//! generator (006e6a10) adds the hit as a slot (006d7830); after the pass every contact is
//! refreshed once (006d7c20, from 006e7550): world points from the stored local ones, the
//! separation along the normal, slots dropped when they separate or slide apart. The control step
//! (006db490) turns the contacts with static bodies into planes the controlled velocity may not
//! enter (006dd900, 006dd800, 006dd5b0).
//!
//! Slot words (`Slot`, 22 floats): +0x00 the point in A's frame, +0x0c the point in B's frame,
//! +0x18 B's point in the world, +0x24 A's point in the world, +0x30 the normal (from B toward A),
//! +0x3c the avatar ground mark (+1 / -1 from 006e6a10, else 0), +0x40..+0x50 the solver's words,
//! +0x54 the separation n . (A's point - B's point), negative when they overlap.
//!
//! Kept literal on purpose: a slot removed by the refresh is replaced by the last slot, and the
//! average then accumulates on that index (the sums made before the removal are dropped), as the
//! game does. Not ported: the refresh's wake calls on removal (00556ca0 / 00556d20, the sleep lists),
//! its listener calls with the state code (holder +0x2c lists, 006d816d), and the solver that reads
//! +0x40..+0x54 (006df7c0 lines 171-397).

use glam::Vec3;

/// One slot's 22 words.
pub type Slot = [f32; 22];

/// Word indices of a slot.
pub const LOCAL_A: usize = 0;
pub const LOCAL_B: usize = 3;
pub const WORLD_B: usize = 6;
pub const WORLD_A: usize = 9;
pub const NORMAL: usize = 12;
pub const GROUND: usize = 15;
/// +0x40, +0x44, +0x48, +0x4c, +0x50: the solver's words.
pub const SOLVER: usize = 16;
pub const SEPARATION: usize = 21;

/// [game] +0x1f0 as 006d7130 sets it (0x4e01902e, about 5.4e8).
pub const FRESH_1F0: f32 = 543_293_312.0;

fn v3(s: &Slot, at: usize) -> Vec3 {
    Vec3::new(s[at], s[at + 1], s[at + 2])
}

fn put(s: &mut Slot, at: usize, v: Vec3) {
    s[at] = v.x;
    s[at + 1] = v.y;
    s[at + 2] = v.z;
}

/// What the contact functions read of a holder.
#[derive(Clone, Copy, Debug)]
pub struct PairBody {
    /// +0x108: the current affine (basis rows, then the origin).
    pub pose: [f32; 12],
    /// +0x1d4 bit 0x1000: an avatar's holder.
    pub avatar: bool,
    /// +0xa4 != 0: the holder has a physics body (a static one has none).
    pub dynamic: bool,
    /// The body's linear velocity (+0xa0), zero without a body.
    pub velocity: Vec3,
    /// +0x218, the friction term the contact takes the larger of.
    pub friction: f32,
}

/// The contact object.
#[derive(Clone, Debug)]
pub struct Contact {
    /// Four slots and the average at index `count` (+0x00..+0x1b4).
    pub slots: [Slot; 5],
    /// +0x1b8.
    pub count: usize,
    /// +0x1bc: the larger friction of the two holders, 0 between two avatars.
    pub friction: f32,
    /// +0x1c8: the feature (a soup triangle's index; -1 from the body-pair wrappers).
    pub feature: u32,
    /// +0x1e4: 0.05, or 0.1 when either holder's shape is of type 0xe (006d7280).
    pub margin: f32,
    /// +0x1e8: both holders have a body that is not keyframed (006d7280).
    pub both_dynamic: bool,
    /// +0x1e9: set by 006d7130 and by 006e7550 when it refreshes the contact.
    pub refreshed: bool,
    /// +0x1ea: read by the generator's ground rule and the refresh's friction (writer unread).
    pub ground_side: bool,
    /// +0x1eb: a slot overlaps by more than 0.8 (006d9030).
    pub deep: bool,
    /// +0x1ec: an avatar has this contact's normal pointing down onto it by more than 0.2 (006d9030):
    /// a ceiling. The control step then gives the avatar no control.
    pub ceiling: bool,
    /// +0x1f0.
    pub word_1f0: f32,
}

/// What 006d9030 asks of the holders' other contacts: clear their ground marks (slots 0..=count,
/// `count` being this contact's) for A's list, B's list, or neither.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClearGround {
    pub a: bool,
    pub b: bool,
    /// The slot bound (this contact's count) the game uses for every contact of the list.
    pub upto: usize,
}

impl Contact {
    /// [game] 006d7130 and 006d7280: a new contact between A and B for `feature`.
    pub fn new(a: &PairBody, b: &PairBody, feature: u32, margin: f32, both_dynamic: bool) -> Self {
        let mut friction = if b.friction < a.friction { a.friction } else { b.friction };
        if a.avatar && b.avatar {
            friction = 0.0;
        }
        Contact {
            slots: [[0.0; 22]; 5],
            count: 0,
            friction,
            feature,
            margin,
            both_dynamic,
            refreshed: true,
            ground_side: false,
            deep: false,
            ceiling: false,
            word_1f0: FRESH_1F0,
        }
    }

    /// [game] 006d7650: the slot whose A point lies nearest `p` within margin + 0.01, or None.
    pub fn matching_slot(&self, p: Vec3) -> Option<usize> {
        let r = self.margin + 0.01;
        let mut best = r * r;
        let mut found = None;
        for i in 0..self.count {
            let s = &self.slots[i];
            let dx = s[0] - p.x;
            let dy = s[1] - p.y;
            let dz = s[2] - p.z;
            let d = (dx * dx + dz * dz) + dy * dy;
            if d < best {
                found = Some(i);
                best = d;
            }
        }
        found
    }

    /// [game] 006d73f0: with four slots, the one to replace by `p` (A's point): the choice that
    /// leaves the largest |(p - s_j) x (s_k - s_l)|^2, as the game pairs them.
    pub fn slot_to_replace(&self, p: Vec3) -> usize {
        let s = |i: usize| v3(&self.slots[i], LOCAL_A);
        let (s0, s1, s2, s3) = (s(0), s(1), s(2), s(3));
        let cross2 = |a: Vec3, b: Vec3| {
            let cx = a.z * b.y - a.y * b.z;
            let cy = b.z * a.x - a.z * b.x;
            let cz = a.y * b.x - b.y * a.x;
            (cz * cz + cy * cy) + cx * cx
        };
        let r = [
            cross2(s3 - s2, p - s1),
            cross2(s3 - s2, p - s0),
            cross2(s3 - s1, p - s0),
            cross2(s2 - s1, p - s0),
        ];
        let mut k = (r[0] < r[1]) as usize;
        if r[k] < r[2] {
            k = 2;
        }
        if r[k] < r[3] {
            k = 3;
        }
        k
    }

    /// [game] 006d7830: add the generator's block as a slot (or over the matching one, or over the
    /// one 006d73f0 picks when four are held), rebuild the average, then 006d9030.
    pub fn insert(&mut self, block: &Slot, a_avatar: bool, b_avatar: bool) -> ClearGround {
        let mut block = *block;
        block[SOLVER + 4] = 0.0;
        let idx = match self.matching_slot(v3(&block, LOCAL_A)) {
            Some(i) => i,
            None if self.count == 4 => self.slot_to_replace(v3(&block, LOCAL_A)),
            None => {
                self.count += 1;
                self.count - 1
            }
        };
        self.slots[idx] = block;
        let c = self.count;
        if c == 0 {
            // [game] the branch for no slot zeroes these words of slot 1 (006d7b98); unreachable
            // after an add, kept as written.
            for w in [0usize, 1, 2, 3, 4, 5, 12, 13, 14] {
                self.slots[1][w] = 0.0;
            }
        } else {
            let first = self.slots[0];
            let avg = &mut self.slots[c];
            for w in [WORLD_A, WORLD_A + 1, WORLD_A + 2, WORLD_B, WORLD_B + 1, WORLD_B + 2, NORMAL, NORMAL + 1, NORMAL + 2] {
                avg[w] = first[w];
            }
            for i in 1..c {
                let s = self.slots[i];
                let avg = &mut self.slots[c];
                for w in [WORLD_A, WORLD_A + 1, WORLD_A + 2, WORLD_B, WORLD_B + 1, WORLD_B + 2, NORMAL, NORMAL + 1, NORMAL + 2] {
                    avg[w] = s[w] + avg[w];
                }
            }
            let avg = &mut self.slots[c];
            if c > 1 {
                let inv = 1.0 / c as f32;
                for w in [WORLD_A, WORLD_A + 1, WORLD_A + 2, WORLD_B, WORLD_B + 1, WORLD_B + 2] {
                    avg[w] *= inv;
                }
                normalize(avg);
            }
            avg[SOLVER] = 0.0;
            avg[SOLVER + 2] = 0.0;
            avg[SOLVER + 3] = 0.0;
            avg[SEPARATION] = separation(avg);
        }
        if c > 1 {
            self.slots[c][SOLVER + 4] = self.slots[c - 1][SOLVER + 4];
            self.slots[c - 1][SOLVER + 4] = 0.0;
        } else {
            self.slots[0][SOLVER + 4] = 0.0;
            self.slots[1][SOLVER + 4] = 0.0;
        }
        self.crush_flags(a_avatar, b_avatar)
    }

    /// [game] 006d9030: +0x1eb when a slot overlaps by more than 0.8 (an avatar on either side);
    /// +0x1ec when an avatar A has the normal pointing down at it (nz < -0.2) or an avatar B up
    /// (nz > 0.2), slot 0's normal; that side's contacts then lose their ground marks.
    pub fn crush_flags(&mut self, a_avatar: bool, b_avatar: bool) -> ClearGround {
        self.deep = false;
        self.ceiling = false;
        let mut out = ClearGround { upto: self.count, ..Default::default() };
        if a_avatar || b_avatar {
            for i in 0..self.count {
                if self.slots[i][SEPARATION] < -0.8 {
                    self.deep = true;
                }
            }
        }
        if self.count != 0 {
            let nz = self.slots[0][NORMAL + 2];
            if a_avatar && nz < -0.2 {
                self.ceiling = true;
                out.a = true;
            }
            if b_avatar && nz > 0.2 {
                self.ceiling = true;
                out.b = true;
            }
        }
        out
    }

    /// The ground marks' clear 006d9030 runs over a holder's list (slots 0..=upto of each contact).
    pub fn clear_ground(&mut self, upto: usize) {
        for i in 0..=upto.min(4) {
            self.slots[i][GROUND] = 0.0;
        }
    }

    /// [game] 006d7c20: world points, separations, drops and the average, once per step after the
    /// pass. `recast(side_is_a, c, d)` is 005be420 against that side's shape (its current pose):
    /// the segment c + d .. c - d, returning the nearest hit's point and normal. It is only called
    /// for a slot whose points slid apart sideways by more than the threshold.
    pub fn refresh(&mut self, a: &PairBody, b: &PairBody, mut recast: impl FnMut(bool, Vec3, Vec3) -> Option<(Vec3, Vec3)>) -> ClearGround {
        if self.count == 0 {
            return ClearGround::default();
        }
        let keep_friction = (!self.ground_side || !a.avatar || !b.dynamic) && (!b.avatar || !a.dynamic);
        self.friction = if keep_friction {
            if b.friction < a.friction { a.friction } else { b.friction }
        } else {
            0.0
        };
        for i in (0..self.count).rev() {
            let s = &mut self.slots[i];
            let wa = world_a(&a.pose, v3(s, LOCAL_A));
            let wb = world_b(&b.pose, v3(s, LOCAL_B));
            put(s, WORLD_A, wa);
            put(s, WORLD_B, wb);
            s[SEPARATION] = sep_refresh(s);
        }
        let c = self.count;
        let saved = (self.slots[c][SOLVER], self.slots[c][SOLVER + 2], self.slots[c][SOLVER + 3]);
        for w in [WORLD_A, WORLD_A + 1, WORLD_A + 2, WORLD_B, WORLD_B + 1, WORLD_B + 2, NORMAL, NORMAL + 1, NORMAL + 2] {
            self.slots[c][w] = 0.0;
        }
        let r = self.margin + 0.01;
        let thr = r * r + 0.01;
        let limit = if a.dynamic && b.dynamic { self.margin * 0.24 } else { self.margin * 4.0 };
        for i in (0..self.count).rev() {
            let s = self.slots[i];
            let sep = s[SEPARATION];
            let n = v3(&s, NORMAL);
            let wa = v3(&s, WORLD_A);
            let wb = v3(&s, WORLD_B);
            let mut keep = None;
            if !(limit < sep) {
                let q = Vec3::new(wa.x - n.x * sep, wa.y - n.y * sep, wa.z - n.z * sep);
                let d = Vec3::new(wb.x - q.x, wb.y - q.y, wb.z - q.z);
                if thr < (d.z * d.z + d.y * d.y) + d.x * d.x {
                    keep = self.recast_slot(i, a, b, q, &mut recast);
                } else {
                    keep = Some((wa, wb, n));
                }
            }
            match keep {
                Some((ka, kb, kn)) => {
                    let avg = &mut self.slots[self.count];
                    for (at, v) in [(WORLD_A, ka), (WORLD_B, kb), (NORMAL, kn)] {
                        avg[at] = v.x + avg[at];
                        avg[at + 1] = v.y + avg[at + 1];
                        avg[at + 2] = v.z + avg[at + 2];
                    }
                }
                None => {
                    // [game] 006d882a: both holders' sleep counters are cleared and they are woken
                    // (not ported), then the last slot moves into this one.
                    self.count -= 1;
                    self.slots[i] = self.slots[self.count];
                }
            }
        }
        let c = self.count;
        if c > 1 {
            let inv = 1.0 / c as f32;
            let avg = &mut self.slots[c];
            for w in [WORLD_A, WORLD_A + 1, WORLD_A + 2, WORLD_B, WORLD_B + 1, WORLD_B + 2] {
                avg[w] *= inv;
            }
        }
        if c != 0 {
            let avg = &mut self.slots[c];
            normalize(avg);
            avg[SOLVER] = saved.0;
            avg[SOLVER + 2] = saved.1;
            avg[SOLVER + 3] = saved.2;
            avg[SEPARATION] = separation_end(avg);
            avg[SOLVER + 4] = 0.0;
            avg[GROUND] = 0.0;
        }
        self.crush_flags(a.avatar, b.avatar)
    }

    /// [game] 006d7c20's re-cast (lines 120-229): against B from A's point projected onto B's plane,
    /// unless B moves and its sideways speed is the larger, then against A from B's point. The slot
    /// takes the hit's point (and normal) when it lies within the threshold of the projected point.
    fn recast_slot(&mut self, i: usize, a: &PairBody, b: &PairBody, q: Vec3, recast: &mut impl FnMut(bool, Vec3, Vec3) -> Option<(Vec3, Vec3)>) -> Option<(Vec3, Vec3, Vec3)> {
        let r = self.margin + 0.01;
        let thr = r * r + 0.01;
        let s = &mut self.slots[i];
        let n = v3(s, NORMAL);
        let sep = s[SEPARATION];
        let n3 = n * 3.0;
        let against_b = if !b.dynamic {
            true
        } else if a.dynamic {
            let tangent = |v: Vec3| {
                let d = (n.z * v.z + n.y * v.y) + n.x * v.x;
                Vec3::new(v.x - n.x * d, v.y - n.y * d, v.z - n.z * d)
            };
            let ta = tangent(a.velocity);
            let tb = tangent(b.velocity);
            (tb.z * tb.z + tb.y * tb.y) + tb.x * tb.x <= (ta.z * ta.z + ta.y * ta.y) + ta.x * ta.x
        } else {
            false
        };
        if against_b {
            let (p, hn) = recast(false, q, n3)?;
            let d = p - q;
            if thr < (d.z * d.z + d.y * d.y) + d.x * d.x {
                return None;
            }
            put(s, WORLD_B, p);
            put(s, NORMAL, hn);
            Some((v3(s, WORLD_A), p, hn))
        } else {
            let wb = v3(s, WORLD_B);
            let c = Vec3::new(wb.x + n.x * sep, wb.y + n.y * sep, wb.z + n.z * sep);
            let (p, hn) = recast(true, c, -n3)?;
            let d = p - c;
            if thr < (d.x * d.x + d.z * d.z) + d.y * d.y {
                return None;
            }
            put(s, WORLD_A, p);
            put(s, NORMAL, -hn);
            Some((p, wb, -hn))
        }
    }
}

/// [game] 006d7c20: A's local point through A's pose, summed as the refresh sums it.
fn world_a(m: &[f32; 12], p: Vec3) -> Vec3 {
    Vec3::new(
        ((m[0] * p.x + p.z * m[6]) + m[3] * p.y) + m[9],
        ((m[4] * p.y + m[7] * p.z) + m[1] * p.x) + m[10],
        ((m[2] * p.x + p.y * m[5]) + p.z * m[8]) + m[11],
    )
}

/// [game] 006d7c20: B's local point through B's pose.
fn world_b(m: &[f32; 12], p: Vec3) -> Vec3 {
    Vec3::new(
        ((m[0] * p.x + m[3] * p.y) + p.z * m[6]) + m[9],
        ((m[7] * p.z + m[4] * p.y) + m[1] * p.x) + m[10],
        ((m[2] * p.x + p.z * m[8]) + p.y * m[5]) + m[11],
    )
}

/// [game] the average's separation in 006d7830.
fn separation(s: &Slot) -> f32 {
    let (a, b, n) = (v3(s, WORLD_A), v3(s, WORLD_B), v3(s, NORMAL));
    (n.z * (a.z - b.z) + n.y * (a.y - b.y)) + n.x * (a.x - b.x)
}

/// [game] a slot's separation in 006d7c20's first loop.
fn sep_refresh(s: &Slot) -> f32 {
    let (a, b, n) = (v3(s, WORLD_A), v3(s, WORLD_B), v3(s, NORMAL));
    (n.x * (a.x - b.x) + n.z * (a.z - b.z)) + n.y * (a.y - b.y)
}

/// [game] the average's separation at the end of 006d7c20.
fn separation_end(s: &Slot) -> f32 {
    let (a, b, n) = (v3(s, WORLD_A), v3(s, WORLD_B), v3(s, NORMAL));
    (n.z * (a.z - b.z) + n.y * (a.y - b.y)) + (a.x - b.x) * n.x
}

/// [game] the average normal's normalisation (006d7830, 006d7c20): unchanged when |n| <= 1e-8.
fn normalize(s: &mut Slot) {
    let n = v3(s, NORMAL);
    let len = ((n.z * n.z + n.y * n.y) + n.x * n.x).sqrt();
    if len > 1e-8 {
        let inv = 1.0 / len;
        put(s, NORMAL, Vec3::new(inv * n.x, n.y * inv, n.z * inv));
    }
}

/// The control step's plane record (a 0x90-byte local of 006db490).
#[derive(Clone, Copy, Debug, Default)]
pub struct Planes {
    /// +0x00: an orthonormal basis of the contact normals added (006dd900), +0x60 its count.
    pub basis: [Vec3; 3],
    pub count: usize,
    /// +0x48: per basis vector, the largest component a later normal had along it (its own length
    /// for the first).
    pub reach: [f32; 3],
    /// +0x24 / +0x64: a second set, empty in the control step.
    pub second: [Vec3; 3],
    pub second_count: usize,
}

impl Planes {
    /// [game] 006dd900: Gram-Schmidt the normal against the basis; a residue longer than
    /// sqrt(0.001) joins it (three at most).
    pub fn add(&mut self, n: Vec3) {
        let mut r = n;
        for i in 0..self.count {
            let e = self.basis[i];
            let d = (e.z * r.z + e.y * r.y) + r.x * e.x;
            if self.reach[i] < d {
                self.reach[i] = d;
            }
            r.x -= e.x * d;
            r.y -= e.y * d;
            r.z -= e.z * d;
        }
        if self.count < 3 {
            let l2 = (r.z * r.z + r.y * r.y) + r.x * r.x;
            if l2 > 0.001 {
                let len = l2.sqrt();
                let inv = 1.0 / len;
                self.basis[self.count] = Vec3::new(inv * r.x, inv * r.y, r.z * inv);
                self.reach[self.count] = len;
                self.count += 1;
            }
        }
    }

    /// [game] 006dd800 then 006dd5b0: the velocity with every component into a basis vector removed
    /// (one-sided: only where v . e < 0), the second set first.
    pub fn project(&mut self, v: Vec3) -> Vec3 {
        let mut v = v;
        for i in 0..self.second_count {
            v = cut(v, self.second[i]);
        }
        for i in 0..self.count {
            v = cut(v, self.basis[i]);
        }
        if self.count == 0 && self.second_count != 0 {
            self.count = self.second_count;
        }
        self.reach = [0.0; 3];
        v
    }
}

fn cut(v: Vec3, e: Vec3) -> Vec3 {
    let d = (v.y * e.y + v.z * e.z) + e.x * v.x;
    if d < 0.0 {
        Vec3::new(v.x - e.x * d, v.y - e.y * d, v.z - e.z * d)
    } else {
        v
    }
}

/// A contact as the control step sees it from one holder's list.
#[derive(Clone, Copy, Debug)]
pub struct ListedContact {
    /// This holder is the contact's A (+0x1c0); else its B, and the normal is turned round.
    pub this_is_a: bool,
    /// +0x1b8.
    pub count: usize,
    /// Slot 0's normal and ground mark.
    pub normal: Vec3,
    pub ground: f32,
    /// +0x1ec.
    pub ceiling: bool,
    /// The other holder has a body.
    pub other_dynamic: bool,
}

/// [game] 00557000: each component kept in (-100000, 100000] (the low bound DAT_00cae3ec, -100000
/// in the image; a NaN takes it).
pub fn clamp_velocity(v: Vec3) -> Vec3 {
    let c = |x: f32| {
        if -100_000.0 < x {
            if 100_000.0 <= x { 100_000.0 } else { x }
        } else {
            -100_000.0
        }
    };
    Vec3::new(c(v.x), c(v.y), c(v.z))
}

/// [game] 006db490 with holder bit 5 clear (the linear part): the body's new velocity.
///
/// wanted = (animated origin - body origin) / dt; v' = v + control (wanted - v), per axis; control
/// is zero when any contact of the list is a ceiling (+0x1ec). The result is projected off the
/// planes of the contacts with static holders, slot 0's normal turned toward this holder; those
/// are used only when at least one of them has no ground mark (+0x3c == 0): a body touching only
/// floors keeps its velocity. `avatar` is the holder's bit 0x1000 (the planes are built only then).
/// None when the step does not run (no dt).
pub fn control_velocity(dt: f32, control: Vec3, velocity: Vec3, origin: Vec3, animated: Vec3, contacts: &[ListedContact], avatar: bool) -> Option<Vec3> {
    if dt == 0.0 {
        return None;
    }
    let inv = 1.0 / dt;
    let wanted = Vec3::new((animated.x - origin.x) * inv, (animated.y - origin.y) * inv, (animated.z - origin.z) * inv);
    let mut c = control;
    if contacts.iter().any(|k| k.ceiling) {
        c = Vec3::new(c.x * 0.0, c.y * 0.0, c.z * 0.0);
    }
    let gap = Vec3::new((wanted.x - velocity.x) * c.x, (wanted.y - velocity.y) * c.y, (wanted.z - velocity.z) * c.z);
    let mut planes = Planes::default();
    if avatar {
        let live = |k: &&ListedContact| k.count != 0 && !k.other_dynamic;
        let any_free = contacts.iter().filter(live).any(|k| k.ground == 0.0);
        if any_free {
            for k in contacts.iter().filter(live) {
                let n = if k.this_is_a { k.normal } else { Vec3::new(-0.0 - k.normal.x, -0.0 - k.normal.y, -0.0 - k.normal.z) };
                planes.add(n);
            }
        }
    }
    let v = Vec3::new(velocity.x + gap.x, velocity.y + gap.y, velocity.z + gap.z);
    Some(clamp_velocity(planes.project(v)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../work/emu/ref/");

    fn rows(name: &str) -> Option<Vec<Vec<String>>> {
        let text = std::fs::read_to_string(format!("{REF}{name}")).ok()?;
        Some(text.lines().filter(|l| !l.is_empty()).map(|l| l.split('\t').map(str::to_owned).collect()).collect())
    }

    fn word(s: &str) -> u32 {
        u32::from_str_radix(s, 16).unwrap()
    }

    fn fl(s: &str) -> f32 {
        f32::from_bits(word(s))
    }

    struct Cursor<'a> {
        cols: &'a [String],
        at: usize,
    }

    impl Cursor<'_> {
        fn f(&mut self) -> f32 {
            self.at += 1;
            fl(&self.cols[self.at - 1])
        }
        fn w(&mut self) -> u32 {
            self.at += 1;
            word(&self.cols[self.at - 1])
        }
        fn d(&mut self) -> i64 {
            self.at += 1;
            self.cols[self.at - 1].parse().unwrap()
        }
        fn v(&mut self) -> Vec3 {
            Vec3::new(self.f(), self.f(), self.f())
        }
        fn words(&mut self, n: usize) -> Vec<u32> {
            (0..n).map(|_| self.w()).collect()
        }
    }

    fn bits(v: Vec3) -> [u32; 3] {
        [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
    }

    /// The contact's 125 words as the game lays them out (the fields the port keeps).
    fn contact_from(words: &[u32]) -> Contact {
        let mut slots = [[0.0f32; 22]; 5];
        for (i, s) in slots.iter_mut().enumerate() {
            for (j, x) in s.iter_mut().enumerate() {
                *x = f32::from_bits(words[i * 22 + j]);
            }
        }
        let bytes = words[122];
        Contact {
            slots,
            count: words[110] as usize,
            friction: f32::from_bits(words[111]),
            feature: words[114],
            margin: f32::from_bits(words[121]),
            both_dynamic: bytes & 0xff != 0,
            refreshed: (bytes >> 8) & 0xff != 0,
            ground_side: (bytes >> 16) & 0xff != 0,
            deep: (bytes >> 24) & 0xff != 0,
            ceiling: words[123] & 0xff != 0,
            word_1f0: f32::from_bits(words[124]),
        }
    }

    fn check_contact(case: usize, got: &Contact, want: &[u32]) {
        let w = contact_from(want);
        assert_eq!(got.count, w.count, "case {case}: count");
        for i in 0..5 {
            for j in 0..22 {
                assert_eq!(got.slots[i][j].to_bits(), w.slots[i][j].to_bits(), "case {case}: slot {i} word {j} ({} vs {})", got.slots[i][j], w.slots[i][j]);
            }
        }
        assert_eq!(got.friction.to_bits(), w.friction.to_bits(), "case {case}: friction");
        assert_eq!((got.deep, got.ceiling), (w.deep, w.ceiling), "case {case}: flags");
    }

    /// 006dd900 / 006dd800 / 006dd5b0 against the emulator (`gen_refs_manifold.py planes`).
    #[test]
    fn reference_planes() {
        let Some(rows) = rows("manifold_planes.tsv") else { return };
        for (case, cols) in rows.iter().enumerate() {
            let mut c = Cursor { cols, at: 0 };
            let k = c.d() as usize;
            let normals: Vec<Vec3> = (0..k).map(|_| c.v()).collect();
            let v = c.v();
            let rec = c.words(36);
            let want = c.v();
            let mut p = Planes::default();
            for n in &normals {
                p.add(*n);
            }
            assert_eq!(p.count as u32, rec[24], "case {case}: count");
            for i in 0..3 {
                assert_eq!(bits(p.basis[i]), [rec[i * 3], rec[i * 3 + 1], rec[i * 3 + 2]], "case {case}: basis {i}");
                assert_eq!(p.reach[i].to_bits(), rec[18 + i], "case {case}: reach {i}");
            }
            assert_eq!(bits(p.project(v)), bits(want), "case {case}: projected");
        }
    }

    /// 006d7830 with 006d7650, 006d73f0 and 006d9030 (`gen_refs_manifold.py insert`).
    #[test]
    fn reference_insert() {
        let Some(rows) = rows("manifold_insert.tsv") else { return };
        for (case, cols) in rows.iter().enumerate() {
            let mut c = Cursor { cols, at: 0 };
            let fa = c.d();
            let fb = c.d();
            let before = c.words(125);
            let block: Vec<f32> = (0..22).map(|_| c.f()).collect();
            let after = c.words(125);
            let mut k = contact_from(&before);
            let mut blk = [0.0f32; 22];
            blk.copy_from_slice(&block);
            // the driver's holders list this contact alone, so a clear reaches it
            let clear = k.insert(&blk, fa & 1 != 0, fb & 1 != 0);
            if clear.a || clear.b {
                k.clear_ground(clear.upto);
            }
            check_contact(case, &k, &after);
        }
    }

    fn body(c: &mut Cursor) -> PairBody {
        let flags = c.d();
        let mut pose = [0.0f32; 12];
        for x in pose.iter_mut() {
            *x = c.f();
        }
        let velocity = c.v();
        let _omega = c.v();
        let friction = c.f();
        let dynamic = flags & 2 != 0;
        PairBody { pose, avatar: flags & 1 != 0, dynamic, velocity: if dynamic { velocity } else { Vec3::ZERO }, friction }
    }

    /// 006d7c20 without the re-cast (`gen_refs_manifold.py refresh`).
    #[test]
    fn reference_refresh() {
        let Some(rows) = rows("manifold_refresh.tsv") else { return };
        for (case, cols) in rows.iter().enumerate() {
            let mut c = Cursor { cols, at: 0 };
            let a = body(&mut c);
            let b = body(&mut c);
            let before = c.words(125);
            let after = c.words(125);
            let mut k = contact_from(&before);
            let clear = k.refresh(&a, &b, |_, _, _| panic!("case {case}: the reference has no re-cast"));
            if clear.a || clear.b {
                k.clear_ground(clear.upto);
            }
            check_contact(case, &k, &after);
        }
    }

    /// 006db490 whole, bit 5 clear (`gen_refs_manifold.py control`).
    #[test]
    fn reference_control() {
        let Some(rows) = rows("manifold_control.tsv") else { return };
        for (case, cols) in rows.iter().enumerate() {
            let mut c = Cursor { cols, at: 0 };
            let dt = c.f();
            let control = c.v();
            let v = c.v();
            let origin = c.v();
            let animated = c.v();
            let k = c.d() as usize;
            let contacts: Vec<ListedContact> = (0..k)
                .map(|_| {
                    let this_is_a = c.d() != 0;
                    let count = c.d() as usize;
                    let normal = c.v();
                    let ground = c.f();
                    let ceiling = c.d() != 0;
                    let other_dynamic = c.d() != 0;
                    ListedContact { this_is_a, count, normal, ground, ceiling, other_dynamic }
                })
                .collect();
            let want = c.v();
            let got = control_velocity(dt, control, v, origin, animated, &contacts, true).unwrap();
            assert_eq!(bits(got), bits(want), "case {case}: {got} vs {want}");
        }
    }

    fn static_floor() -> PairBody {
        PairBody { pose: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0], avatar: false, dynamic: false, velocity: Vec3::ZERO, friction: 0.5 }
    }

    #[test]
    fn a_slot_that_separates_is_dropped() {
        let floor = static_floor();
        let mut a = PairBody { pose: floor.pose, avatar: true, dynamic: true, velocity: Vec3::ZERO, friction: 0.0 };
        let mut k = Contact::new(&a, &floor, 0, 0.05, false);
        let mut s = [0.0f32; 22];
        put(&mut s, NORMAL, Vec3::Z);
        k.insert(&s, true, false);
        assert_eq!(k.count, 1);
        a.pose[11] = 0.1;
        k.refresh(&a, &floor, |_, _, _| None);
        assert_eq!(k.count, 1, "0.1 above the floor is within 4 margins");
        assert!((k.slots[0][SEPARATION] - 0.1).abs() < 1e-6);
        a.pose[11] = 0.3;
        k.refresh(&a, &floor, |_, _, _| None);
        assert_eq!(k.count, 0, "0.3 above is past 4 x 0.05");
    }

    #[test]
    fn floors_alone_leave_the_velocity_and_a_wall_cuts_it() {
        let floor = ListedContact { this_is_a: true, count: 1, normal: Vec3::Z, ground: 1.0, ceiling: false, other_dynamic: false };
        let wall = ListedContact { this_is_a: true, count: 1, normal: Vec3::X, ground: 0.0, ceiling: false, other_dynamic: false };
        let v = Vec3::new(-5.0, 0.0, -3.0);
        let free = control_velocity(0.032, Vec3::ZERO, v, Vec3::ZERO, Vec3::ZERO, &[floor], true).unwrap();
        assert_eq!(free, v);
        let cut = control_velocity(0.032, Vec3::ZERO, v, Vec3::ZERO, Vec3::ZERO, &[floor, wall], true).unwrap();
        assert_eq!(cut, Vec3::ZERO, "with a wall both planes apply");
    }
}
