//! The contact solver of the world step, its static rows (`docs/physics.md` 1.7): a body against
//! a holder without a body (the level), as the avatar meets it.
//!
//! [game] 006df7c0 lines 171-397 build rows from every refreshed contact's slots that overlap
//! (separation < 0; < -margin between two bodies), 4 iterations on a multi-core machine
//! (DAT_00d3fa2c, 1 on a single core: 00ad6df0) solve them, and 006ead90 writes the velocities back.
//! A row is a sequential impulse along the contact normal with a split-impulse PSEUDO pass: the
//! pseudo velocities (body +0xb8..+0xcc) push the overlap out and are added to the pose by the pose
//! pass, then cleared; the real velocity only loses its approach. Friction rows (two tangents,
//! clamped to friction x the normal impulse) only when the root's +0x1d4 & 3 != 1 (006e92f0).
//!
//! Lanes: the game keeps every quantity as four floats (SSE); scalars are broadcast. The port keeps
//! the four lanes so the arithmetic matches word for word; the fourth lane of a vector is whatever
//! the game had there (zero in the port's own rows).
//!
//! rcpps / rsqrtps (the reciprocal and reciprocal square root estimates) are the processor's own
//! 12-bit values, which differ between makers: the port issues the same instructions
//! (`rcp_estimate`, `rsqrt_estimate`) on the machine the game runs on, so it gets the game's values
//! [game: rcpps at 006ebc09, checked by disassembly; the solver rows 006eb820..006ed8f2]. The emulator's
//! references (Unicorn computes them exactly) are compared within a tolerance for that reason.
//!
//! Not ported here: the two-body rows (006ec0e0, solved by 006e9ef0 / 006ea270), the restitution rows
//! of the average slot (006ed510 / 006eda30; none when an avatar is involved), the joint rows (006eca20 /
//! 006ece30, solved by 006ea940 / 006ea7f0), the islands and the worker tasks.

/// Four lanes.
pub type Lanes = [f32; 4];

const ZERO: Lanes = [0.0; 4];

/// [game] a solver body (0x80 bytes, built by 006eb820 from the body: +0xa0 v, +0xac w, +0xb8 / +0xc4
/// the pseudo pair, +0x28 the inverse mass, the wrapper's world inverse inertia (006e9360)).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SolverBody {
    pub v: Lanes,
    /// The game's angular velocity: the NEGATIVE of the conventional one (`docs/driving.md`).
    pub w: Lanes,
    pub vp: Lanes,
    pub wp: Lanes,
    pub inv_mass: Lanes,
    /// Three rows of four: row i is the world inverse inertia's row i (and one more float).
    pub inv_inertia: [Lanes; 3],
}

impl SolverBody {
    pub fn from_words(w: &[f32]) -> Self {
        let l = |i: usize| [w[i], w[i + 1], w[i + 2], w[i + 3]];
        SolverBody { v: l(0), w: l(4), vp: l(8), wp: l(12), inv_mass: l(16), inv_inertia: [l(20), l(24), l(28)] }
    }

    pub fn words(&self) -> [f32; 32] {
        let mut out = [0.0; 32];
        for (k, lanes) in [self.v, self.w, self.vp, self.wp, self.inv_mass, self.inv_inertia[0], self.inv_inertia[1], self.inv_inertia[2]].iter().enumerate() {
            out[k * 4..k * 4 + 4].copy_from_slice(lanes);
        }
        out
    }

    /// v += inv_mass * p (lanes 0..3); w += (p x r) through the inverse inertia, as every row applies
    /// an impulse.
    fn apply(&mut self, d: Lanes, d3: f32, r: Lanes) {
        for i in 0..3 {
            self.v[i] += self.inv_mass[i] * d[i];
        }
        self.v[3] += d3;
        let c = cross_pr(d, r);
        rotate_add(&mut self.w, c, &self.inv_inertia);
    }

    fn apply_pseudo(&mut self, d: Lanes, d3: f32, r: Lanes) {
        for i in 0..3 {
            self.vp[i] += d[i] * self.inv_mass[i];
        }
        self.vp[3] += d3;
        let c = cross_pr(d, r);
        rotate_add(&mut self.wp, c, &self.inv_inertia);
    }
}

/// (p x r) as the rows sum it.
fn cross_pr(p: Lanes, r: Lanes) -> [f32; 3] {
    [r[2] * p[1] - r[1] * p[2], r[0] * p[2] - r[2] * p[0], r[1] * p[0] - r[0] * p[1]]
}

/// w[j] += (c.y M[1][j] + c.x M[0][j]) + c.z M[2][j], all four lanes.
fn rotate_add(w: &mut Lanes, c: [f32; 3], m: &[Lanes; 3]) {
    for j in 0..4 {
        w[j] += (c[1] * m[1][j] + c[0] * m[0][j]) + c[2] * m[2][j];
    }
}

/// The velocity of the body at r along n: ((r x w) + v) . n, summed (y + x) + z.
fn normal_speed(n: Lanes, r: Lanes, v: Lanes, w: Lanes) -> f32 {
    let x = ((r[1] * w[2] - r[2] * w[1]) + v[0]) * n[0];
    let y = ((r[2] * w[0] - r[0] * w[2]) + v[1]) * n[1];
    let z = ((r[0] * w[1] - r[1] * w[0]) + v[2]) * n[2];
    (y + x) + z
}

/// SSE maxps(a, b) and minps(a, b): b when the comparison fails (a NaN takes b).
fn maxps(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}
fn minps(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

/// rcpps then the game's Newton step: (1 - k r) r + r [game 006ebc09: `rcpps xmm0, xmm1`, then mulps].
fn reciprocal(k: f32) -> f32 {
    let r = rcp_estimate(k);
    (1.0 - k * r) * r + r
}

/// The processor's reciprocal estimate, the instruction the game issues (rcpps) [game 006ebc09 and the
/// other solver rows]. The estimate is the CPU's own (12 bits, it differs between makers), so the port
/// issues the same instruction on the same machine the game runs on and gets the game's value. Off x86,
/// and in the unit tests, it is the exact 1/x rounded to float: the emulator (Unicorn) computes the
/// estimates exactly, so the bit-exact reference tests check the arithmetic order against that, and
/// `cpu_estimates_are_within_their_bound` checks the instruction itself.
pub(crate) fn rcp_estimate(x: f32) -> f32 {
    #[cfg(all(target_arch = "x86_64", not(test)))]
    {
        use std::arch::x86_64::{_mm_cvtss_f32, _mm_rcp_ss, _mm_set_ss};
        // SAFETY: SSE is part of the x86_64 baseline.
        unsafe { _mm_cvtss_f32(_mm_rcp_ss(_mm_set_ss(x))) }
    }
    #[cfg(any(not(target_arch = "x86_64"), test))]
    {
        1.0 / x
    }
}

/// The processor's reciprocal square root estimate (rsqrtps) [game 00579761, 005b7d7e]: as `rcp_estimate`.
pub(crate) fn rsqrt_estimate(x: f32) -> f32 {
    #[cfg(all(target_arch = "x86_64", not(test)))]
    {
        use std::arch::x86_64::{_mm_cvtss_f32, _mm_rsqrt_ss, _mm_set_ss};
        // SAFETY: SSE is part of the x86_64 baseline.
        unsafe { _mm_cvtss_f32(_mm_rsqrt_ss(_mm_set_ss(x))) }
    }
    #[cfg(any(not(target_arch = "x86_64"), test))]
    {
        1.0 / x.sqrt()
    }
}

/// [game] a static row (0x80 bytes): one body against a holder without a body.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StaticRow {
    /// +0x00 the normal (toward the body), +0x10 the arm (slot's A point - the root's origin).
    pub n: Lanes,
    pub r: Lanes,
    /// +0x30 1/k, the inverse of the effective mass along n.
    pub inv_k: Lanes,
    /// +0x40 the pseudo pass's target speed, min(0, separation + margin) x the bias.
    pub bias: Lanes,
    /// +0x50 the accumulated impulse (warm-started from the slot's +0x40), +0x60 the pseudo one.
    pub impulse: Lanes,
    pub pseudo: Lanes,
    /// +0x70 the solver body's index; +0x74 the slot, to write the impulses back (+0x40 / +0x44).
    pub body: u16,
}

/// [game] a friction row (0x70 bytes), paired with a static row.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrictionRow {
    /// +0x00 the contact's friction (+0x1bc).
    pub friction: Lanes,
    pub inv_k1: Lanes,
    pub inv_k2: Lanes,
    /// +0x30 t1: the plane's uphill direction (z less its part along n, normalised; zero on a level
    /// floor); +0x40 t2 = n x t1.
    pub t1: Lanes,
    pub t2: Lanes,
    /// +0x50 / +0x60 the accumulated impulses (from the slot's +0x48 / +0x4c).
    pub i1: Lanes,
    pub i2: Lanes,
}

/// [game] 006e9960: one pass over static rows (velocity, then pseudo velocity).
pub fn solve_static(bodies: &mut [SolverBody], rows: &mut [StaticRow]) {
    for row in rows.iter_mut() {
        solve_normal(&mut bodies[row.body as usize], row);
    }
}

fn solve_normal(b: &mut SolverBody, row: &mut StaticRow) {
    let n = row.n;
    let r = row.r;
    let s = normal_speed(n, r, b.v, b.w);
    let old = row.impulse;
    for i in 0..4 {
        row.impulse[i] = maxps(s * (0.0 - row.inv_k[i]) + old[i], 0.0);
    }
    let d = [(row.impulse[0] - old[0]) * n[0], (row.impulse[1] - old[1]) * n[1], (row.impulse[2] - old[2]) * n[2], 0.0];
    let d3 = b.inv_mass[3] * (row.impulse[3] - old[3]) * n[3];
    b.apply(d, d3, r);
    let sp = normal_speed(n, r, b.vp, b.wp);
    let old = row.pseudo;
    for i in 0..4 {
        row.pseudo[i] = maxps((row.bias[i] - sp) * row.inv_k[i] + old[i], 0.0);
    }
    let d = [(row.pseudo[0] - old[0]) * n[0], (row.pseudo[1] - old[1]) * n[1], (row.pseudo[2] - old[2]) * n[2], 0.0];
    let d3 = (row.pseudo[3] - old[3]) * n[3] * b.inv_mass[3];
    b.apply_pseudo(d, d3, r);
}

/// [game] 006e9b70: one pass over static rows with friction: the normal row as 006e9960, then both
/// tangents from the same point velocity, each clamped to +-friction x the new normal impulse.
pub fn solve_static_friction(bodies: &mut [SolverBody], rows: &mut [StaticRow], friction: &mut [FrictionRow]) {
    for (row, f) in rows.iter_mut().zip(friction.iter_mut()) {
        let b = &mut bodies[row.body as usize];
        solve_normal(b, row);
        let r = row.r;
        let pv = [
            (r[1] * b.w[2] - r[2] * b.w[1]) + b.v[0],
            (r[2] * b.w[0] - r[0] * b.w[2]) + b.v[1],
            (r[0] * b.w[1] - r[1] * b.w[0]) + b.v[2],
        ];
        let s1 = (f.t1[1] * pv[1] + f.t1[0] * pv[0]) + f.t1[2] * pv[2];
        let s2 = (pv[1] * f.t2[1] + pv[0] * f.t2[0]) + pv[2] * f.t2[2];
        let (o1, o2) = (f.i1, f.i2);
        for i in 0..4 {
            let lim = f.friction[i] * row.impulse[i];
            f.i1[i] = minps(maxps((0.0 - s1) * f.inv_k1[i] + o1[i], 0.0 - lim), lim);
            f.i2[i] = minps(maxps((0.0 - s2) * f.inv_k2[i] + o2[i], 0.0 - lim), lim);
        }
        let mut p = [0.0f32; 4];
        for i in 0..4 {
            p[i] = (f.i1[i] - o1[i]) * f.t1[i] + (f.i2[i] - o2[i]) * f.t2[i];
        }
        for i in 0..4 {
            b.v[i] += p[i] * b.inv_mass[i];
        }
        let c = [p[1] * r[2] - p[2] * r[1], p[2] * r[0] - p[0] * r[2], p[0] * r[1] - p[1] * r[0]];
        rotate_add(&mut b.w, c, &b.inv_inertia);
    }
}

/// [game] 00579740: v / |v| (xyz length, all four lanes scaled), zero when |v|^2 is not > 0.
/// rsqrtps then the game's Newton step [game 00579761: `rsqrtps xmm2, xmm1`, 00579740
/// normalize_vector_xyz_sse], the estimate from `rsqrt_estimate`.
fn normalize4(v: Lanes) -> Lanes {
    let l2 = (v[1] * v[1] + v[0] * v[0]) + v[2] * v[2];
    if !(0.0 < l2) {
        return ZERO;
    }
    let y0 = rsqrt_estimate(l2);
    let y = (1.0 - (y0 * l2) * y0) * (0.5 * y0) + y0;
    [y * v[0], y * v[1], y * v[2], y * v[3]]
}

/// What 006eba50 builds for one overlapping slot.
#[derive(Clone, Copy, Debug)]
pub struct BuiltRow {
    pub row: StaticRow,
    pub friction: Option<FrictionRow>,
}

/// [game] 006eba50: the static row (and its friction row) for a slot of A against a holder without a
/// body, and the warm start: A's solver body takes the slot's stored impulses at once.
///
/// `origin` is A's root's +0x12c; `world_a` the slot's +0x24; `n` the normal the world step chose (the
/// slot's, or (0, 0, ground mark) for a ground slot A moves into; times -1 when A is keyframed);
/// `separation` the slot's +0x54; `stored` its +0x40 / +0x48 / +0x4c; `bias` -1440, -3240 (deep) /
/// 60 each, as 006df7c0 makes them; `with_friction` 006e92f0 (root +0x1d4 & 3 != 1).
#[allow(clippy::too_many_arguments)]
pub fn build_static_row(body: &mut SolverBody, index: u16, origin: [f32; 3], world_a: [f32; 3], n: Lanes, separation: f32, stored: [f32; 3], bias: Lanes, margin: f32, friction: f32, with_friction: bool) -> BuiltRow {
    let r = [world_a[0] - origin[0], world_a[1] - origin[1], world_a[2] - origin[2], 0.0];
    let m = body.inv_inertia;
    let k_of = |t: Lanes| -> Lanes {
        let c = [t[2] * r[1] - t[1] * r[2], t[0] * r[2] - t[2] * r[0], t[1] * r[0] - t[0] * r[1]];
        let ic: Vec<f32> = (0..3).map(|j| (c[1] * m[1][j] + c[0] * m[0][j]) + c[2] * m[2][j]).collect();
        let kx = (ic[1] * r[2] - ic[2] * r[1]) * t[0];
        let ky = (ic[2] * r[0] - ic[0] * r[2]) * t[1];
        let kz = (ic[0] * r[1] - ic[1] * r[0]) * t[2];
        let s = (ky + kx) + kz;
        [s + body.inv_mass[0], s + body.inv_mass[1], s + body.inv_mass[2], s + body.inv_mass[3]]
    };
    let k = k_of(n);
    let inv_k = k.map(reciprocal);
    let depth = separation + margin;
    let b = minps(0.0, depth);
    let bias_v = [b * bias[0], b * bias[1], b * bias[2], b * bias[3]];
    let lam = stored[0];
    let mut p = [lam * n[0], lam * n[1], lam * n[2], lam * n[3]];
    let row = StaticRow { n, r, inv_k, bias: bias_v, impulse: [lam; 4], pseudo: ZERO, body: index };
    let friction_row = if with_friction {
        let ax: Lanes = if n[2] <= std::f32::consts::SQRT_2 { [0.0, 0.0, 1.0, 1.0] } else { [1.0, 0.0, 0.0, 0.0] };
        let d = (n[1] * 0.0 + ax[0] * n[0]) + ax[2] * n[2];
        let t = [ax[0] - d * n[0], 0.0 - d * n[1], ax[2] - d * n[2], ax[3] - d * n[3]];
        let t1 = normalize4(t);
        // The fourth lane is the game's 4-wide cross product as it computes it (zero, or NaN
        // when the lane is not finite).
        #[allow(clippy::eq_op)]
        let t2 = [t1[2] * n[1] - t1[1] * n[2], t1[0] * n[2] - t1[2] * n[0], t1[1] * n[0] - t1[0] * n[1], t1[3] * n[3] - t1[3] * n[3]];
        let k1 = k_of(t1);
        let k2 = k_of(t2);
        let inv = |k: Lanes| if k.iter().any(|&x| x != 0.0) { k.map(reciprocal) } else { ZERO };
        let (l1, l2) = (stored[1], stored[2]);
        for i in 0..4 {
            p[i] += t2[i] * l2 + t1[i] * l1;
        }
        Some(FrictionRow { friction: [friction; 4], inv_k1: inv(k1), inv_k2: inv(k2), t1, t2, i1: [l1; 4], i2: [l2; 4] })
    } else {
        None
    };
    for i in 0..4 {
        body.v[i] += body.inv_mass[i] * p[i];
    }
    let c = [p[1] * r[2] - p[2] * r[1], p[2] * r[0] - p[0] * r[2], p[0] * r[1] - p[1] * r[0]];
    rotate_add(&mut body.w, c, &m);
    BuiltRow { row, friction: friction_row }
}

/// [game] a pair row (0x80 bytes): two bodies. +0x00 n (toward A), +0x10 rA, +0x20 rB, +0x30 1/k, +0x40
/// bias, +0x50 impulse, +0x60 pseudo impulse, +0x70 / +0x72 the bodies' indices, +0x74 the slot.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PairRow {
    pub n: Lanes,
    pub ra: Lanes,
    pub rb: Lanes,
    pub inv_k: Lanes,
    pub bias: Lanes,
    pub impulse: Lanes,
    pub pseudo: Lanes,
    pub a: u16,
    pub b: u16,
}

/// The relative velocity along n of two bodies' points: (((rA x wA) + vA) - ((rB x wB) + vB)) . n.
fn pair_speed(n: Lanes, ra: Lanes, rb: Lanes, va: Lanes, wa: Lanes, vb: Lanes, wb: Lanes) -> f32 {
    let x = (((ra[1] * wa[2] - ra[2] * wa[1]) + va[0]) - ((rb[1] * wb[2] - rb[2] * wb[1]) + vb[0])) * n[0];
    let y = (((ra[2] * wa[0] - ra[0] * wa[2]) + va[1]) - ((rb[2] * wb[0] - rb[0] * wb[2]) + vb[1])) * n[1];
    let z = (((ra[0] * wa[1] - ra[1] * wa[0]) + va[2]) - ((rb[0] * wb[1] - rb[1] * wb[0]) + vb[2])) * n[2];
    (y + x) + z
}

fn pair_point_velocity(ra: Lanes, rb: Lanes, a: &SolverBody, b: &SolverBody) -> [f32; 3] {
    [
        ((ra[1] * a.w[2] - ra[2] * a.w[1]) + a.v[0]) - ((rb[1] * b.w[2] - rb[2] * b.w[1]) + b.v[0]),
        ((ra[2] * a.w[0] - ra[0] * a.w[2]) + a.v[1]) - ((rb[2] * b.w[0] - rb[0] * b.w[2]) + b.v[1]),
        ((ra[0] * a.w[1] - ra[1] * a.w[0]) + a.v[2]) - ((rb[0] * b.w[1] - rb[1] * b.w[0]) + b.v[2]),
    ]
}

/// Two distinct bodies of the slice, mutably.
fn two(bodies: &mut [SolverBody], a: usize, b: usize) -> (&mut SolverBody, &mut SolverBody) {
    assert_ne!(a, b, "a pair row joins two bodies");
    if a < b {
        let (lo, hi) = bodies.split_at_mut(b);
        (&mut lo[a], &mut hi[0])
    } else {
        let (lo, hi) = bodies.split_at_mut(a);
        (&mut hi[0], &mut lo[b])
    }
}

/// An impulse d (lanes 0..3, lane 3 already the fourth lane's change) into A, and -d into B.
fn apply_pair(a: &mut SolverBody, b: &mut SolverBody, d: Lanes, ra: Lanes, rb: Lanes, pseudo: bool) {
    let neg = [0.0 - d[0], 0.0 - d[1], 0.0 - d[2], 0.0 - d[3]];
    for (body, p, r) in [(a, d, ra), (b, neg, rb)] {
        let (v, w) = if pseudo { (&mut body.vp, &mut body.wp) } else { (&mut body.v, &mut body.w) };
        for i in 0..4 {
            v[i] += p[i] * body.inv_mass[i];
        }
        let c = cross_pr(p, r);
        rotate_add(w, c, &body.inv_inertia);
    }
}

fn solve_pair_normal(a: &mut SolverBody, b: &mut SolverBody, row: &mut PairRow) {
    let (n, ra, rb) = (row.n, row.ra, row.rb);
    let s = pair_speed(n, ra, rb, a.v, a.w, b.v, b.w);
    let old = row.impulse;
    for i in 0..4 {
        row.impulse[i] = maxps((0.0 - s) * row.inv_k[i] + old[i], 0.0);
    }
    let d = [0, 1, 2, 3].map(|i| (row.impulse[i] - old[i]) * n[i]);
    apply_pair(a, b, d, ra, rb, false);
    let sp = pair_speed(n, ra, rb, a.vp, a.wp, b.vp, b.wp);
    let old = row.pseudo;
    for i in 0..4 {
        row.pseudo[i] = maxps((row.bias[i] - sp) * row.inv_k[i] + old[i], 0.0);
    }
    let d = [0, 1, 2, 3].map(|i| (row.pseudo[i] - old[i]) * n[i]);
    apply_pair(a, b, d, ra, rb, true);
}

/// [game] 006e9ef0: one pass over pair rows (velocity, then pseudo velocity).
pub fn solve_pair(bodies: &mut [SolverBody], rows: &mut [PairRow]) {
    for row in rows.iter_mut() {
        let (a, b) = two(bodies, row.a as usize, row.b as usize);
        solve_pair_normal(a, b, row);
    }
}

/// [game] 006ea270: pair rows with friction: the pair row, then both tangents on the relative point
/// velocity, clamped to +-friction x the new normal impulse.
pub fn solve_pair_friction(bodies: &mut [SolverBody], rows: &mut [PairRow], friction: &mut [FrictionRow]) {
    for (row, f) in rows.iter_mut().zip(friction.iter_mut()) {
        let (a, b) = two(bodies, row.a as usize, row.b as usize);
        solve_pair_normal(a, b, row);
        let pv = pair_point_velocity(row.ra, row.rb, a, b);
        let s1 = (f.t1[1] * pv[1] + f.t1[0] * pv[0]) + f.t1[2] * pv[2];
        let s2 = (f.t2[1] * pv[1] + f.t2[0] * pv[0]) + f.t2[2] * pv[2];
        let (o1, o2) = (f.i1, f.i2);
        for i in 0..4 {
            let lim = f.friction[i] * row.impulse[i];
            f.i1[i] = minps(maxps((0.0 - s1) * f.inv_k1[i] + o1[i], 0.0 - lim), lim);
            f.i2[i] = minps(maxps(o2[i] + (0.0 - s2) * f.inv_k2[i], 0.0 - lim), lim);
        }
        let d = [0, 1, 2, 3].map(|i| (f.i2[i] - o2[i]) * f.t2[i] + (f.i1[i] - o1[i]) * f.t1[i]);
        apply_pair(a, b, d, row.ra, row.rb, false);
    }
}

/// k = (inverse masses) + t . ((I_A^-1 (rA x t)) x rA + (I_B^-1 (rB x t)) x rB), as 006ec0e0 sums it.
fn pair_k(a: &SolverBody, b: &SolverBody, ra: Lanes, rb: Lanes, t: Lanes) -> Lanes {
    let ic = |r: Lanes, m: &[Lanes; 3]| -> [f32; 3] {
        let c = [t[2] * r[1] - t[1] * r[2], t[0] * r[2] - t[2] * r[0], t[1] * r[0] - t[0] * r[1]];
        [0, 1, 2].map(|j| (c[1] * m[1][j] + c[0] * m[0][j]) + c[2] * m[2][j])
    };
    let ia = ic(ra, &a.inv_inertia);
    let ib = ic(rb, &b.inv_inertia);
    let kx = ((ia[1] * ra[2] - ia[2] * ra[1]) + (ib[1] * rb[2] - ib[2] * rb[1])) * t[0];
    let ky = ((ia[2] * ra[0] - ia[0] * ra[2]) + (ib[2] * rb[0] - ib[0] * rb[2])) * t[1];
    let kz = ((ia[0] * ra[1] - ia[1] * ra[0]) + (ib[0] * rb[1] - ib[1] * rb[0])) * t[2];
    let s = (ky + kx) + kz;
    [0, 1, 2, 3].map(|i| s + (b.inv_mass[i] + a.inv_mass[i]))
}

/// What 006ec0e0 builds for one slot overlapping by more than the margin.
#[derive(Clone, Copy, Debug)]
pub struct BuiltPairRow {
    pub row: PairRow,
    pub friction: Option<FrictionRow>,
}

/// [game] 006ec0e0: the pair row (and friction row) for a slot between two bodies, with the warm
/// start applied to both: rA = the slot's A point (+0x24) - A's root origin, rB = its B point (+0x18) -
/// B's. `with_friction` is 006e92f0 of A.
#[allow(clippy::too_many_arguments)]
pub fn build_pair_row(a: &mut SolverBody, b: &mut SolverBody, index: (u16, u16), origin_a: [f32; 3], origin_b: [f32; 3], world_a: [f32; 3], world_b: [f32; 3], n: Lanes, separation: f32, stored: [f32; 3], bias: Lanes, margin: f32, friction: f32, with_friction: bool) -> BuiltPairRow {
    let ra = [world_a[0] - origin_a[0], world_a[1] - origin_a[1], world_a[2] - origin_a[2], 0.0];
    let rb = [world_b[0] - origin_b[0], world_b[1] - origin_b[1], world_b[2] - origin_b[2], 0.0];
    let inv_k = pair_k(a, b, ra, rb, n).map(reciprocal);
    let dmin = minps(0.0, separation + margin);
    let bias_v = bias.map(|x| dmin * x);
    let lam = stored[0];
    let mut p = n.map(|x| lam * x);
    let row = PairRow { n, ra, rb, inv_k, bias: bias_v, impulse: [lam; 4], pseudo: ZERO, a: index.0, b: index.1 };
    let friction_row = if with_friction {
        let ax: Lanes = if n[2] <= std::f32::consts::SQRT_2 { [0.0, 0.0, 1.0, 1.0] } else { [1.0, 0.0, 0.0, 0.0] };
        let d = (ax[1] * n[1] + ax[0] * n[0]) + ax[2] * n[2];
        let t = [0, 1, 2, 3].map(|i| ax[i] - d * n[i]);
        let t1 = normalize4(t);
        #[allow(clippy::eq_op)] // the fourth lane, as above
        let t2 = [t1[2] * n[1] - t1[1] * n[2], t1[0] * n[2] - t1[2] * n[0], t1[1] * n[0] - t1[0] * n[1], t1[3] * n[3] - t1[3] * n[3]];
        let inv = |k: Lanes| if k.iter().any(|&x| x != 0.0) { k.map(reciprocal) } else { ZERO };
        let k1 = pair_k(a, b, ra, rb, t1);
        let k2 = pair_k(a, b, ra, rb, t2);
        let (l1, l2) = (stored[1], stored[2]);
        for i in 0..4 {
            p[i] += t2[i] * l2 + t1[i] * l1;
        }
        Some(FrictionRow { friction: [friction; 4], inv_k1: inv(k1), inv_k2: inv(k2), t1, t2, i1: [l1; 4], i2: [l2; 4] })
    } else {
        None
    };
    for i in 0..4 {
        a.v[i] += p[i] * a.inv_mass[i];
    }
    rotate_add(&mut a.w, cross_pr(p, ra), &a.inv_inertia.clone());
    let q = p.map(|x| 0.0 - x);
    for i in 0..4 {
        b.v[i] += q[i] * b.inv_mass[i];
    }
    rotate_add(&mut b.w, cross_pr(q, rb), &b.inv_inertia.clone());
    BuiltPairRow { row, friction: friction_row }
}

/// [game] a restitution row (0x60 bytes, list +0xbc): +0x00 rA, +0x10 rB, +0x20 n, +0x30 the target
/// separating speed, +0x40 1/k, +0x50 A's index, +0x52 B's (0xffff for a holder without a body).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BounceRow {
    pub ra: Lanes,
    pub rb: Lanes,
    pub n: Lanes,
    pub target: Lanes,
    pub inv_k: Lanes,
    pub a: u16,
    pub b: Option<u16>,
}

/// [game] 006ed510: the bounce row of a contact's AVERAGE slot, built before its slot rows. None unless
/// the approach is faster than 1 (every lane of n . v_rel <= -1) and neither holder is an avatar; the
/// target is e (-n . v_rel) with e = max(A, B +0x214) min(1, -(n . v_rel) / 3). `b` is None for a
/// holder without a body. `restitution` is (A +0x214, B +0x214).
#[allow(clippy::too_many_arguments)]
pub fn build_bounce_row(a: &SolverBody, b: Option<&SolverBody>, index: (u16, u16), origin_a: [f32; 3], origin_b: [f32; 3], world_a: [f32; 3], world_b: [f32; 3], n: Lanes, avatars: bool, restitution: (f32, f32)) -> Option<BounceRow> {
    let ra = [world_a[0] - origin_a[0], world_a[1] - origin_a[1], world_a[2] - origin_a[2], 0.0];
    let rb = [world_b[0] - origin_b[0], world_b[1] - origin_b[1], world_b[2] - origin_b[2], 0.0];
    let point = |r: Lanes, s: &SolverBody| [(s.w[2] * r[1] - s.w[1] * r[2]) + s.v[0], (s.w[0] * r[2] - s.w[2] * r[0]) + s.v[1], (s.w[1] * r[0] - s.w[0] * r[1]) + s.v[2]];
    let (k, rel) = match b {
        None => {
            let m = a.inv_inertia;
            let c = [n[2] * ra[1] - n[1] * ra[2], n[0] * ra[2] - n[2] * ra[0], n[1] * ra[0] - n[0] * ra[1]];
            let ic = [0, 1, 2].map(|j| (c[1] * m[1][j] + c[0] * m[0][j]) + c[2] * m[2][j]);
            let kx = (ic[1] * ra[2] - ic[2] * ra[1]) * n[0];
            let ky = (ic[2] * ra[0] - ic[0] * ra[2]) * n[1];
            let kz = (ic[0] * ra[1] - ic[1] * ra[0]) * n[2];
            let s = (ky + kx) + kz;
            (a.inv_mass.map(|x| s + x), point(ra, a))
        }
        Some(bb) => {
            // [game] summed ((wA x.. + vA) - vB) - (B's rotation part), 006ed510's two-body branch
            let pa = point(ra, a);
            let cb = [bb.w[2] * rb[1] - bb.w[1] * rb[2], bb.w[0] * rb[2] - bb.w[2] * rb[0], bb.w[1] * rb[0] - bb.w[0] * rb[1]];
            (pair_k(a, bb, ra, rb, n), [0, 1, 2].map(|i| (pa[i] - bb.v[i]) - cb[i]))
        }
    };
    let inv_k = k.map(reciprocal);
    let vn = (rel[1] * n[1] + rel[0] * n[0]) + rel[2] * n[2];
    let e = if !(-1.0 < vn) && !avatars {
        let m = minps(1.0, vn * -0.333_333_34);
        let r = if restitution.0 <= restitution.1 { restitution.1 } else { restitution.0 };
        r * m
    } else {
        0.0
    };
    let target = (0.0 - e) * vn;
    if target == 0.0 {
        return None;
    }
    Some(BounceRow { ra, rb, n, target: [target; 4], inv_k, a: index.0, b: b.map(|_| index.1) })
}

/// [game] 006eda30: after the iterations, each bounce row's impulse (1/k target) n, less its part along
/// the normals of the earlier rows of the same body (A index), into A and (when it has one) -P into B.
pub fn apply_bounce(bodies: &mut [SolverBody], rows: &[BounceRow]) {
    for (i, row) in rows.iter().enumerate() {
        let mut p = [0, 1, 2, 3].map(|j| (row.inv_k[j] * row.target[j]) * row.n[j]);
        for prev in &rows[..i] {
            if prev.a == row.a {
                let d = (prev.n[1] * p[1] + prev.n[0] * p[0]) + prev.n[2] * p[2];
                for j in 0..4 {
                    p[j] -= d * prev.n[j];
                }
            }
        }
        let a = &mut bodies[row.a as usize];
        for j in 0..4 {
            a.v[j] += a.inv_mass[j] * p[j];
        }
        rotate_add(&mut a.w, cross_pr(p, row.ra), &a.inv_inertia.clone());
        if let Some(bi) = row.b {
            let q = [0.0 - p[0], 0.0 - p[1], 0.0 - p[2], 0.0 - p[3]];
            let b = &mut bodies[bi as usize];
            for j in 0..4 {
                b.v[j] += b.inv_mass[j] * q[j];
            }
            rotate_add(&mut b.w, cross_pr(q, row.rb), &b.inv_inertia.clone());
        }
    }
}

/// SSE operations as the joint builders use them, lane by lane.
mod sse {
    use super::Lanes;
    pub type Mat = [Lanes; 3];
    pub const Z: Lanes = [0.0; 4];

    pub fn shuf(a: Lanes, b: Lanes, imm: u8) -> Lanes {
        [a[(imm & 3) as usize], a[((imm >> 2) & 3) as usize], b[((imm >> 4) & 3) as usize], b[((imm >> 6) & 3) as usize]]
    }
    pub fn add(a: Lanes, b: Lanes) -> Lanes {
        [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
    }
    pub fn sub(a: Lanes, b: Lanes) -> Lanes {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]
    }
    pub fn mul(a: Lanes, b: Lanes) -> Lanes {
        [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
    }
    pub fn bcast(a: Lanes, i: usize) -> Lanes {
        [a[i]; 4]
    }
    /// shuf(a, 0xc9) shuf(b, 0xd2) - shuf(a, 0xd2) shuf(b, 0xc9): a x b in lanes 0..2.
    pub fn cross4(a: Lanes, b: Lanes) -> Lanes {
        sub(mul(shuf(a, a, 0xc9), shuf(b, b, 0xd2)), mul(shuf(a, a, 0xd2), shuf(b, b, 0xc9)))
    }
    /// [game] 00531aa0: row i of the product = (a_ix B0 + a_iy B1) + a_iz B2.
    pub fn mat_mul(a: &Mat, b: &Mat) -> Mat {
        let row = |r: Lanes| add(add(mul(bcast(r, 0), b[0]), mul(bcast(r, 1), b[1])), mul(bcast(r, 2), b[2]));
        [row(a[0]), row(a[1]), row(a[2])]
    }
    /// [game] 00531b50: the inverse by cofactors over rcpps (and a Newton step) of the determinant.
    pub fn mat_inv(m: &Mat) -> Mat {
        let c0 = cross4(m[1], m[2]);
        let c1 = cross4(m[2], m[0]);
        let c2 = cross4(m[0], m[1]);
        let d = mul(m[0], c0);
        let det = add(add(bcast(d, 1), bcast(d, 0)), bcast(d, 2));
        let r = det.map(crate::physics_solver::rcp_estimate);
        let inv = add(mul(sub([1.0; 4], mul(det, r)), r), r);
        let row = |i: usize| mul(shuf(shuf(bcast(c0, i), bcast(c1, i), 0), bcast(c2, i), 8), inv);
        [row(0), row(1), row(2)]
    }
    /// The skew matrix of r as the builders shuffle it.
    pub fn skew(r: Lanes) -> Mat {
        let (bx, by, bz) = (bcast(r, 0), bcast(r, 1), bcast(r, 2));
        [
            shuf(shuf(Z, sub(Z, bz), 0), by, 8),
            shuf(shuf(bz, Z, 0), sub(Z, bx), 8),
            shuf(shuf(sub(Z, by), bx, 0), Z, 8),
        ]
    }
    /// The inverse mass as a diagonal (lane 3 of the last row keeps it too).
    pub fn diag(m: Lanes) -> Mat {
        [shuf(shuf(m, Z, 0), Z, 8), shuf(shuf(Z, m, 0), Z, 8), shuf(shuf(Z, Z, 0), m, 8)]
    }
    pub fn mat_sub(a: &Mat, b: &Mat) -> Mat {
        [sub(a[0], b[0]), sub(a[1], b[1]), sub(a[2], b[2])]
    }
}

/// [game] a joint's solver fields: +0x30 the anchor in A's frame, +0x70 the other anchor (in the world
/// for a joint to the world, in B's frame for a joint between two bodies), +0x80 the accumulated
/// impulse (written back after the solve), +0x90 the warm start's factor per lane.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Joint {
    pub anchor_a: [f32; 3],
    pub anchor_b: [f32; 3],
    pub impulse: Lanes,
    pub warm: Lanes,
}

/// [game] a joint row (0x80 bytes): +0x00 K^-1 (three rows), +0x30 rA, +0x40 rB (a world joint keeps its
/// target point there), +0x50 the error velocity, +0x60 the accumulated impulse, +0x70 / +0x72 the
/// bodies (A, then B for a pair).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JointRow {
    pub kinv: [Lanes; 3],
    pub ra: Lanes,
    pub rb: Lanes,
    pub err: Lanes,
    pub impulse: Lanes,
    pub a: u16,
    pub b: Option<u16>,
}

/// [game] 006eca20: a joint holding A's anchor to a point in the world. p = A's anchor through A's pose
/// (the root's when it has one), r = p - the root's origin, err = (target - p) x bias (-360 dt per lane),
/// K = m I - [r] I^-1 [r] inverted; the warm start -(impulse x warm) is applied to A.
pub fn build_world_joint(body: &mut SolverBody, index: u16, pose: &[f32; 12], origin: [f32; 3], joint: &Joint, bias: Lanes) -> JointRow {
    use sse::*;
    let (m, l) = (pose, joint.anchor_a);
    let p = [((m[3] * l[1] + m[6] * l[2]) + m[0] * l[0]) + m[9], ((m[4] * l[1] + m[7] * l[2]) + m[1] * l[0]) + m[10], ((m[5] * l[1] + m[8] * l[2]) + m[2] * l[0]) + m[11], 0.0];
    let target = [joint.anchor_b[0], joint.anchor_b[1], joint.anchor_b[2], 0.0];
    let err = mul(sub(target, p), bias);
    let r = sub(p, [origin[0], origin[1], origin[2], 0.0]);
    let s = skew(r);
    let t = mat_mul(&mat_mul(&s, &body.inv_inertia), &s);
    let kinv = mat_inv(&mat_sub(&diag(body.inv_mass), &t));
    let p_warm = mul(joint.warm, joint.impulse);
    let neg = sub(Z, p_warm);
    body.v = add(mul(body.inv_mass, neg), body.v);
    let c = cross4(neg, r);
    rotate_add(&mut body.w, [c[0], c[1], c[2]], &body.inv_inertia.clone());
    JointRow { kinv, ra: r, rb: target, err, impulse: p_warm, a: index, b: None }
}

/// [game] 006ece30: a joint between A's anchor and B's (both in their frames). err = (pB - pA) x bias,
/// K = (mA + mB) I - [rA] IA^-1 [rA] - [rB] IB^-1 [rB] inverted; warm start +P into B, -P into A.
#[allow(clippy::too_many_arguments)]
pub fn build_pair_joint(a: &mut SolverBody, b: &mut SolverBody, index: (u16, u16), pose_a: &[f32; 12], pose_b: &[f32; 12], origin_a: [f32; 3], origin_b: [f32; 3], joint: &Joint, bias: Lanes) -> JointRow {
    use sse::*;
    let (m, l) = (pose_a, joint.anchor_a);
    let pa = [((m[3] * l[1] + m[6] * l[2]) + l[0] * m[0]) + m[9], ((m[1] * l[0] + m[4] * l[1]) + m[7] * l[2]) + m[10], ((m[2] * l[0] + m[5] * l[1]) + m[8] * l[2]) + m[11], 0.0];
    let (m, l) = (pose_b, joint.anchor_b);
    let pb = [((m[6] * l[2] + m[3] * l[1]) + l[0] * m[0]) + m[9], ((m[7] * l[2] + m[4] * l[1]) + m[1] * l[0]) + m[10], ((m[8] * l[2] + m[5] * l[1]) + m[2] * l[0]) + m[11], 0.0];
    let err = mul(sub(pb, pa), bias);
    let ra = sub(pa, [origin_a[0], origin_a[1], origin_a[2], 0.0]);
    let rb = sub(pb, [origin_b[0], origin_b[1], origin_b[2], 0.0]);
    let d = diag(add(a.inv_mass, b.inv_mass));
    let sa = skew(ra);
    let ta = mat_mul(&mat_mul(&sa, &a.inv_inertia), &sa);
    let sb = skew(rb);
    let tb = mat_mul(&mat_mul(&sb, &b.inv_inertia), &sb);
    let kinv = mat_inv(&mat_sub(&mat_sub(&d, &ta), &tb));
    let p = mul(joint.warm, joint.impulse);
    b.v = add(mul(p, b.inv_mass), b.v);
    let c = cross4(p, rb);
    rotate_add(&mut b.w, [c[0], c[1], c[2]], &b.inv_inertia.clone());
    let neg = sub(Z, p);
    a.v = add(mul(a.inv_mass, neg), a.v);
    let c = cross4(neg, ra);
    rotate_add(&mut a.w, [c[0], c[1], c[2]], &a.inv_inertia.clone());
    JointRow { kinv, ra, rb, err, impulse: p, a: index.0, b: Some(index.1) }
}

fn joint_lambda(k: &[Lanes; 3], vel: [f32; 3]) -> Lanes {
    [0, 1, 2, 3].map(|j| (vel[1] * k[1][j] + vel[0] * k[0][j]) + vel[2] * k[2][j])
}

/// [game] 006ea7f0: one pass over joints to the world. The error velocity is met by the body's point
/// velocity, real and pseudo together; the impulse goes into the real velocity only.
pub fn solve_world_joints(bodies: &mut [SolverBody], rows: &mut [JointRow]) {
    for row in rows.iter_mut() {
        let b = &mut bodies[row.a as usize];
        let r = row.ra;
        let vel = [
            // [game] disassembly 006ea805-006ea873: ((r x wp + vp) + (r x w + v)) + err
            (((r[1] * b.wp[2] - r[2] * b.wp[1]) + b.vp[0]) + ((r[1] * b.w[2] - r[2] * b.w[1]) + b.v[0])) + row.err[0],
            (((r[2] * b.wp[0] - r[0] * b.wp[2]) + b.vp[1]) + ((r[2] * b.w[0] - r[0] * b.w[2]) + b.v[1])) + row.err[1],
            (((r[0] * b.wp[1] - r[1] * b.wp[0]) + b.vp[2]) + ((r[0] * b.w[1] - r[1] * b.w[0]) + b.v[2])) + row.err[2],
        ];
        let lam = joint_lambda(&row.kinv, vel);
        for j in 0..4 {
            row.impulse[j] += lam[j];
        }
        let neg = lam.map(|x| 0.0 - x);
        for j in 0..4 {
            b.v[j] += b.inv_mass[j] * neg[j];
        }
        rotate_add(&mut b.w, cross_pr(neg, r), &b.inv_inertia.clone());
    }
}

/// [game] 006ea940: one pass over joints between two bodies: +impulse into B, -impulse into A.
pub fn solve_pair_joints(bodies: &mut [SolverBody], rows: &mut [JointRow]) {
    for row in rows.iter_mut() {
        let bi = row.b.expect("a pair joint has two bodies") as usize;
        let (a, b) = two(bodies, row.a as usize, bi);
        let (ra, rb) = (row.ra, row.rb);
        let pt = |s: &SolverBody, r: Lanes, i: usize| -> (f32, f32) {
            let (j, k) = ((i + 1) % 3, (i + 2) % 3);
            (r[j] * s.w[k] - r[k] * s.w[j], r[j] * s.wp[k] - r[k] * s.wp[j])
        };
        let mut vel = [0.0f32; 3];
        for i in 0..3 {
            let (cb, cbp) = pt(b, rb, i);
            let (ca, cap) = pt(a, ra, i);
            // [game] disassembly 006ea999-006eaa3d
            vel[i] = row.err[i] - ((((cbp + b.vp[i]) + (cb + b.v[i])) - (ca + a.v[i])) - (cap + a.vp[i]));
        }
        let lam = joint_lambda(&row.kinv, vel);
        for j in 0..4 {
            row.impulse[j] += lam[j];
        }
        for j in 0..4 {
            b.v[j] += b.inv_mass[j] * lam[j];
        }
        rotate_add(&mut b.w, cross_pr(lam, rb), &b.inv_inertia.clone());
        let neg = lam.map(|x| 0.0 - x);
        for j in 0..4 {
            a.v[j] += a.inv_mass[j] * neg[j];
        }
        rotate_add(&mut a.w, cross_pr(neg, ra), &a.inv_inertia.clone());
    }
}

/// [game] 006df7c0: the solver's iteration count, DAT_00d3fa2c (00ad6df0: 4 when the machine has more
/// than one processor, else 1).
pub const ITERATIONS: usize = 4;

/// [game] 006df7c0 lines 171-175 (disassembly 006dfc08-006dfc83): the pseudo pass's bias factors,
/// -0.0 - (f C) dt with f = (1/dt) (1/60), the float the world keeps at +0x2819c. C is 1440 for a slot
/// against a static holder, 3240 when the contact is deep (+0x1eb), 720 between two bodies; joints
/// take -360 dt.
pub fn biases(dt: f32) -> (f32, f32, f32) {
    let f = (1.0 / dt) * 0.016_666_668;
    (-0.0 - (f * 1440.0) * dt, -0.0 - (f * 3240.0) * dt, -0.0 - (f * 720.0) * dt)
}

/// What the solve needs of the moving body besides its solver body.
#[derive(Clone, Copy, Debug)]
pub struct StaticSolveBody {
    /// The root's origin (+0x12c), the arms' base.
    pub origin: [f32; 3],
    /// Its linear velocity before the solve, for the ground slots' normal choice.
    pub velocity: [f32; 3],
    /// 006e92f0: the root's +0x1d4 & 3 != 1.
    pub with_friction: bool,
}

/// [game] 006df7c0 lines 205-283 and 384-404 for ONE dynamic body against holders without a body:
/// the rows of every refreshed contact's overlapping slots (separation < 0), each slot's normal or,
/// for a ground slot (+0x3c != 0) the body moves into horizontally (v.xy . n.xy < 0), the vertical
/// (0, 0, mark); the warm start; `ITERATIONS` passes; the impulses back into the slots (+0x40, and
/// the pseudo one at +0x44). The caller writes the body back (006ead90: v clamped to (-1e5, 1e5], the
/// pseudo velocity into +0xb8.., the angular pair unless +0x1d4 bit 0x200000 or 0x40) and the pose
/// pass then moves the centre by (v + pseudo v) dt.
///
/// `contacts` are this body's, each with the body as A and a static B (the caller's rule). Clears each
/// used contact's `refreshed`. The restitution row of the average slot (006ed510) is skipped: it is
/// zero when either holder is an avatar, the only case wired so far.
pub fn solve_against_static(body: &mut SolverBody, info: &StaticSolveBody, contacts: &mut [crate::physics_manifold::Contact], dt: f32) {
    use crate::physics_manifold::{GROUND, NORMAL, SEPARATION, SOLVER, WORLD_A};
    let (normal_bias, deep_bias, _) = biases(dt);
    let mut rows: Vec<StaticRow> = Vec::new();
    let mut friction: Vec<FrictionRow> = Vec::new();
    let mut owners: Vec<(usize, usize)> = Vec::new();
    let (vx, vy) = (info.velocity[0], info.velocity[1]);
    for (ci, k) in contacts.iter_mut().enumerate() {
        if !k.refreshed || k.count == 0 {
            continue;
        }
        for si in 0..k.count {
            let s = k.slots[si];
            if !(s[SEPARATION] < 0.0) {
                continue;
            }
            let mut n = [s[NORMAL], s[NORMAL + 1], s[NORMAL + 2], 0.0];
            if s[GROUND] != 0.0 && vx * n[0] + vy * n[1] < 0.0 {
                n = [0.0, 0.0, s[GROUND], 0.0];
            }
            // [game] times the sign, 1 here (A is not keyframed)
            let n = [n[0] * 1.0, n[1] * 1.0, n[2] * 1.0, n[3]];
            let bias = if k.deep { deep_bias } else { normal_bias };
            let built = build_static_row(body, 0, info.origin, [s[WORLD_A], s[WORLD_A + 1], s[WORLD_A + 2]], n, s[SEPARATION], [s[SOLVER], s[SOLVER + 2], s[SOLVER + 3]], [bias; 4], k.margin, k.friction, info.with_friction);
            rows.push(built.row);
            if let Some(f) = built.friction {
                friction.push(f);
            }
            owners.push((ci, si));
        }
        k.refreshed = false;
    }
    let mut bodies = [*body];
    for _ in 0..ITERATIONS {
        if info.with_friction {
            solve_static_friction(&mut bodies, &mut rows, &mut friction);
        } else {
            solve_static(&mut bodies, &mut rows);
        }
    }
    *body = bodies[0];
    for (row, (ci, si)) in rows.iter().zip(owners) {
        contacts[ci].slots[si][SOLVER] = row.impulse[0];
        contacts[ci].slots[si][SOLVER + 1] = row.pseudo[0];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../work/emu/ref/");

    /// The instructions the game build issues (rcpss / rsqrtss, the scalar forms of rcpps / rsqrtps):
    /// Intel and AMD document a relative error of at most 1.5 x 2^-12.
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn cpu_estimates_are_within_their_bound() {
        use std::arch::x86_64::{_mm_cvtss_f32, _mm_rcp_ss, _mm_rsqrt_ss, _mm_set_ss};
        let bound = 1.5 / 4096.0;
        for &x in &[1.0e-6f32, 0.013, 0.25, 1.0, 3.0, 82.99, 1000.0, 13669.3, 1.0e9] {
            let (rcp, rsqrt) = unsafe { (_mm_cvtss_f32(_mm_rcp_ss(_mm_set_ss(x))), _mm_cvtss_f32(_mm_rsqrt_ss(_mm_set_ss(x)))) };
            assert!(((rcp - 1.0 / x) * x).abs() <= bound, "rcp {x}: {rcp}");
            assert!(((rsqrt - 1.0 / x.sqrt()) * x.sqrt()).abs() <= bound, "rsqrt {x}: {rsqrt}");
            // After the game's Newton step the reciprocal is within a few ulps of the exact one.
            let r = (1.0 - x * rcp) * rcp + rcp;
            assert!(((r - 1.0 / x) * x).abs() < 1.0e-6, "newton {x}: {r}");
        }
    }

    fn rows(name: &str) -> Option<Vec<Vec<String>>> {
        let text = std::fs::read_to_string(format!("{REF}{name}")).ok()?;
        Some(text.lines().filter(|l| !l.is_empty()).map(|l| l.split('\t').map(str::to_owned).collect()).collect())
    }

    struct Cursor<'a> {
        cols: &'a [String],
        at: usize,
    }

    impl Cursor<'_> {
        fn w(&mut self) -> u32 {
            self.at += 1;
            u32::from_str_radix(&self.cols[self.at - 1], 16).unwrap()
        }
        fn f(&mut self) -> f32 {
            f32::from_bits(self.w())
        }
        fn d(&mut self) -> usize {
            self.at += 1;
            self.cols[self.at - 1].parse().unwrap()
        }
        fn fs(&mut self, n: usize) -> Vec<f32> {
            (0..n).map(|_| self.f()).collect()
        }
        fn ws(&mut self, n: usize) -> Vec<u32> {
            (0..n).map(|_| self.w()).collect()
        }
    }

    fn lanes(w: &[f32], i: usize) -> Lanes {
        [w[i], w[i + 1], w[i + 2], w[i + 3]]
    }

    fn static_row(w: &[f32], raw: &[u32]) -> StaticRow {
        StaticRow { n: lanes(w, 0), r: lanes(w, 4), inv_k: lanes(w, 12), bias: lanes(w, 16), impulse: lanes(w, 20), pseudo: lanes(w, 24), body: (raw[28] & 0xffff) as u16 }
    }

    fn friction_row(w: &[f32]) -> FrictionRow {
        FrictionRow { friction: lanes(w, 0), inv_k1: lanes(w, 4), inv_k2: lanes(w, 8), t1: lanes(w, 12), t2: lanes(w, 16), i1: lanes(w, 20), i2: lanes(w, 24) }
    }

    fn eq_lanes(case: usize, what: &str, got: Lanes, want: Lanes, upto: usize) {
        for i in 0..upto {
            assert_eq!(got[i].to_bits(), want[i].to_bits(), "case {case}: {what} lane {i}: {got:?} vs {want:?}");
        }
    }

    fn eq_body(case: usize, got: &SolverBody, want: &SolverBody) {
        for (what, g, w) in [("v", got.v, want.v), ("w", got.w, want.w), ("vp", got.vp, want.vp), ("wp", got.wp, want.wp)] {
            eq_lanes(case, what, g, w, 4);
        }
    }

    fn read_bodies(c: &mut Cursor, n: usize) -> Vec<SolverBody> {
        (0..n).map(|_| SolverBody::from_words(&c.fs(32))).collect()
    }

    fn read_rows(c: &mut Cursor, n: usize) -> Vec<StaticRow> {
        (0..n)
            .map(|_| {
                let raw = c.ws(32);
                let f: Vec<f32> = raw.iter().map(|&x| f32::from_bits(x)).collect();
                static_row(&f, &raw)
            })
            .collect()
    }

    /// 006e9960 against the emulator (`gen_refs_solver.py static`).
    #[test]
    fn reference_static() {
        let Some(cases) = rows("solver_static.tsv") else { return };
        for (case, cols) in cases.iter().enumerate() {
            let mut c = Cursor { cols, at: 0 };
            let iters = c.d();
            let nb = c.d();
            let mut bodies = read_bodies(&mut c, nb);
            let nr = c.d();
            let mut rs = read_rows(&mut c, nr);
            let want_b = read_bodies(&mut c, nb);
            let want_r = read_rows(&mut c, nr);
            for _ in 0..iters {
                solve_static(&mut bodies, &mut rs);
            }
            for i in 0..nb {
                eq_body(case, &bodies[i], &want_b[i]);
            }
            for i in 0..nr {
                eq_lanes(case, "impulse", rs[i].impulse, want_r[i].impulse, 4);
                eq_lanes(case, "pseudo", rs[i].pseudo, want_r[i].pseudo, 4);
            }
        }
    }

    /// 006e9b70 against the emulator (`gen_refs_solver.py friction`).
    #[test]
    fn reference_friction() {
        let Some(cases) = rows("solver_friction.tsv") else { return };
        for (case, cols) in cases.iter().enumerate() {
            let mut c = Cursor { cols, at: 0 };
            let iters = c.d();
            let nb = c.d();
            let mut bodies = read_bodies(&mut c, nb);
            let nr = c.d();
            let mut rs = read_rows(&mut c, nr);
            let mut fr: Vec<FrictionRow> = (0..nr).map(|_| friction_row(&c.fs(28))).collect();
            let want_b = read_bodies(&mut c, nb);
            let want_r = read_rows(&mut c, nr);
            let want_f: Vec<FrictionRow> = (0..nr).map(|_| friction_row(&c.fs(28))).collect();
            for _ in 0..iters {
                solve_static_friction(&mut bodies, &mut rs, &mut fr);
            }
            for i in 0..nb {
                eq_body(case, &bodies[i], &want_b[i]);
            }
            for i in 0..nr {
                eq_lanes(case, "impulse", rs[i].impulse, want_r[i].impulse, 4);
                eq_lanes(case, "pseudo", rs[i].pseudo, want_r[i].pseudo, 4);
                eq_lanes(case, "i1", fr[i].i1, want_f[i].i1, 4);
                eq_lanes(case, "i2", fr[i].i2, want_f[i].i2, 4);
            }
        }
    }

    /// 006eba50 against the emulator (`gen_refs_solver.py build`).
    #[test]
    fn reference_build() {
        let Some(cases) = rows("solver_build.tsv") else { return };
        for (case, cols) in cases.iter().enumerate() {
            let mut c = Cursor { cols, at: 0 };
            let flags = c.d();
            let origin = c.fs(3);
            let mut body = SolverBody::from_words(&c.fs(32));
            let slot = c.fs(22);
            let n = c.fs(4);
            let bias = c.fs(4);
            let margin = c.f();
            let friction = c.f();
            let want_body = SolverBody::from_words(&c.fs(32));
            let n_plain = c.d();
            let plain = read_rows(&mut c, n_plain);
            let n_fr = c.d();
            let with = read_rows(&mut c, n_fr);
            let n_f = c.d();
            let fr: Vec<FrictionRow> = (0..n_f).map(|_| friction_row(&c.fs(28))).collect();
            let with_friction = flags & 3 != 1;
            let built = build_static_row(&mut body, 0, [origin[0], origin[1], origin[2]], [slot[9], slot[10], slot[11]], lanes(&n, 0), slot[21], [slot[16], slot[18], slot[19]], lanes(&bias, 0), margin, friction, with_friction);
            assert_eq!((n_plain, n_fr, n_f), if with_friction { (0, 1, 1) } else { (1, 0, 0) }, "case {case}: lists");
            let want = if with_friction { with[0] } else { plain[0] };
            eq_lanes(case, "n", built.row.n, want.n, 4);
            eq_lanes(case, "r", built.row.r, want.r, 3);
            eq_lanes(case, "inv_k", built.row.inv_k, want.inv_k, 4);
            eq_lanes(case, "bias", built.row.bias, want.bias, 4);
            eq_lanes(case, "impulse", built.row.impulse, want.impulse, 4);
            eq_lanes(case, "pseudo", built.row.pseudo, want.pseudo, 4);
            if let Some(f) = built.friction {
                let w = fr[0];
                for (what, g, x) in [("friction", f.friction, w.friction), ("k1", f.inv_k1, w.inv_k1), ("k2", f.inv_k2, w.inv_k2), ("t1", f.t1, w.t1), ("t2", f.t2, w.t2), ("i1", f.i1, w.i1), ("i2", f.i2, w.i2)] {
                    eq_lanes(case, what, g, x, 4);
                }
            }
            eq_lanes(case, "body v", body.v, want_body.v, 3);
            eq_lanes(case, "body w", body.w, want_body.w, 3);
        }
    }

    fn read_pair_rows(c: &mut Cursor, n: usize) -> Vec<PairRow> {
        (0..n)
            .map(|_| {
                let raw = c.ws(32);
                let f: Vec<f32> = raw.iter().map(|&x| f32::from_bits(x)).collect();
                PairRow { n: lanes(&f, 0), ra: lanes(&f, 4), rb: lanes(&f, 8), inv_k: lanes(&f, 12), bias: lanes(&f, 16), impulse: lanes(&f, 20), pseudo: lanes(&f, 24), a: (raw[28] & 0xffff) as u16, b: (raw[28] >> 16) as u16 }
            })
            .collect()
    }

    fn pair_case(name: &str, with_friction: bool) {
        let Some(cases) = rows(name) else { return };
        for (case, cols) in cases.iter().enumerate() {
            let mut c = Cursor { cols, at: 0 };
            let iters = c.d();
            let nb = c.d();
            let mut bodies = read_bodies(&mut c, nb);
            let nr = c.d();
            let mut rs = read_pair_rows(&mut c, nr);
            let mut fr: Vec<FrictionRow> = if with_friction { (0..nr).map(|_| friction_row(&c.fs(28))).collect() } else { Vec::new() };
            let want_b = read_bodies(&mut c, nb);
            let want_r = read_pair_rows(&mut c, nr);
            let want_f: Vec<FrictionRow> = if with_friction { (0..nr).map(|_| friction_row(&c.fs(28))).collect() } else { Vec::new() };
            for _ in 0..iters {
                if with_friction {
                    solve_pair_friction(&mut bodies, &mut rs, &mut fr);
                } else {
                    solve_pair(&mut bodies, &mut rs);
                }
            }
            for i in 0..nb {
                eq_body(case, &bodies[i], &want_b[i]);
            }
            for i in 0..nr {
                eq_lanes(case, "impulse", rs[i].impulse, want_r[i].impulse, 4);
                eq_lanes(case, "pseudo", rs[i].pseudo, want_r[i].pseudo, 4);
                if with_friction {
                    eq_lanes(case, "i1", fr[i].i1, want_f[i].i1, 4);
                    eq_lanes(case, "i2", fr[i].i2, want_f[i].i2, 4);
                }
            }
        }
    }

    /// 006e9ef0 against the emulator (`gen_refs_solver.py pair`).
    #[test]
    fn reference_pair() {
        pair_case("solver_pair.tsv", false);
    }

    /// 006ea270 against the emulator (`gen_refs_solver.py pairfriction`).
    #[test]
    fn reference_pair_friction() {
        pair_case("solver_pairfriction.tsv", true);
    }

    /// 006ec0e0 against the emulator (`gen_refs_solver.py pairbuild`).
    #[test]
    fn reference_pair_build() {
        let Some(cases) = rows("solver_pairbuild.tsv") else { return };
        for (case, cols) in cases.iter().enumerate() {
            let mut c = Cursor { cols, at: 0 };
            let flags = c.d();
            let o = c.fs(6);
            let mut a = SolverBody::from_words(&c.fs(32));
            let mut b = SolverBody::from_words(&c.fs(32));
            let slot = c.fs(22);
            let n = c.fs(4);
            let bias = c.fs(4);
            let margin = c.f();
            let friction = c.f();
            let want_a = SolverBody::from_words(&c.fs(32));
            let want_b = SolverBody::from_words(&c.fs(32));
            let n_plain = c.d();
            let plain = read_pair_rows(&mut c, n_plain);
            let n_fr = c.d();
            let with = read_pair_rows(&mut c, n_fr);
            let n_f = c.d();
            let fr: Vec<FrictionRow> = (0..n_f).map(|_| friction_row(&c.fs(28))).collect();
            let with_friction = flags & 3 != 1;
            let built = build_pair_row(&mut a, &mut b, (0, 1), [o[0], o[1], o[2]], [o[3], o[4], o[5]], [slot[9], slot[10], slot[11]], [slot[6], slot[7], slot[8]], lanes(&n, 0), slot[21], [slot[16], slot[18], slot[19]], lanes(&bias, 0), margin, friction, with_friction);
            assert_eq!((n_plain, n_fr, n_f), if with_friction { (0, 1, 1) } else { (1, 0, 0) }, "case {case}: lists");
            let want = if with_friction { with[0] } else { plain[0] };
            eq_lanes(case, "n", built.row.n, want.n, 4);
            eq_lanes(case, "ra", built.row.ra, want.ra, 3);
            eq_lanes(case, "rb", built.row.rb, want.rb, 3);
            eq_lanes(case, "inv_k", built.row.inv_k, want.inv_k, 4);
            eq_lanes(case, "bias", built.row.bias, want.bias, 4);
            eq_lanes(case, "impulse", built.row.impulse, want.impulse, 4);
            assert_eq!((built.row.a, built.row.b), (want.a, want.b), "case {case}: indices");
            if let Some(f) = built.friction {
                let w = fr[0];
                for (what, g, x) in [("friction", f.friction, w.friction), ("k1", f.inv_k1, w.inv_k1), ("k2", f.inv_k2, w.inv_k2), ("t1", f.t1, w.t1), ("t2", f.t2, w.t2), ("i1", f.i1, w.i1), ("i2", f.i2, w.i2)] {
                    eq_lanes(case, what, g, x, 4);
                }
            }
            eq_lanes(case, "a v", a.v, want_a.v, 3);
            eq_lanes(case, "a w", a.w, want_a.w, 3);
            eq_lanes(case, "b v", b.v, want_b.v, 3);
            eq_lanes(case, "b w", b.w, want_b.w, 3);
        }
    }

    /// 006ed510 then 006eda30 against the emulator (`gen_refs_solver.py bounce`).
    #[test]
    fn reference_bounce() {
        let Some(cases) = rows("solver_bounce.tsv") else { return };
        let mut made = 0;
        for (case, cols) in cases.iter().enumerate() {
            let mut c = Cursor { cols, at: 0 };
            let two = c.d() != 0;
            let avatar = c.d() != 0;
            let o = c.fs(6);
            let rest = c.fs(2);
            let mut bodies = vec![SolverBody::from_words(&c.fs(32)), SolverBody::from_words(&c.fs(32))];
            let k = c.d();
            let contacts: Vec<Vec<f32>> = (0..k).map(|_| c.fs(9)).collect();
            let count = c.d();
            let want_rows: Vec<Vec<f32>> = (0..count).map(|_| c.fs(24)).collect();
            let want = [SolverBody::from_words(&c.fs(32)), SolverBody::from_words(&c.fs(32))];
            let mut built = Vec::new();
            for s in &contacts {
                let b = if two { Some(&bodies[1]) } else { None };
                if let Some(r) = build_bounce_row(&bodies[0], b, (0, 1), [o[0], o[1], o[2]], [o[3], o[4], o[5]], [s[3], s[4], s[5]], [s[0], s[1], s[2]], [s[6], s[7], s[8], 0.0], avatar, (rest[0], rest[1])) {
                    built.push(r);
                }
            }
            assert_eq!(built.len(), count, "case {case}: rows");
            for (g, w) in built.iter().zip(&want_rows) {
                eq_lanes(case, "target", g.target, lanes(w, 12), 4);
                eq_lanes(case, "inv_k", g.inv_k, lanes(w, 16), 4);
            }
            made += count;
            apply_bounce(&mut bodies, &built);
            for i in 0..2 {
                eq_lanes(case, "v", bodies[i].v, want[i].v, 3);
                eq_lanes(case, "w", bodies[i].w, want[i].w, 3);
            }
        }
        assert!(made > 100, "the references make only {made} bounce rows");
    }

    fn read_joint_rows(c: &mut Cursor, n: usize, pair: bool) -> Vec<JointRow> {
        (0..n)
            .map(|_| {
                let raw = c.ws(32);
                let f: Vec<f32> = raw.iter().map(|&x| f32::from_bits(x)).collect();
                JointRow { kinv: [lanes(&f, 0), lanes(&f, 4), lanes(&f, 8)], ra: lanes(&f, 12), rb: lanes(&f, 16), err: lanes(&f, 20), impulse: lanes(&f, 24), a: (raw[28] & 0xffff) as u16, b: if pair { Some((raw[28] >> 16) as u16) } else { None } }
            })
            .collect()
    }

    fn joint_build_case(name: &str, pair: bool) {
        let Some(cases) = rows(name) else { return };
        for (case, cols) in cases.iter().enumerate() {
            let mut c = Cursor { cols, at: 0 };
            let nb = c.d();
            let mut bodies = read_bodies(&mut c, nb);
            let poses: Vec<[f32; 12]> = (0..nb).map(|_| c.fs(12).try_into().unwrap()).collect();
            let jw = c.fs(52);
            let bias = c.f();
            let joint = Joint { anchor_a: [jw[12], jw[13], jw[14]], anchor_b: [jw[28], jw[29], jw[30]], impulse: lanes(&jw, 32), warm: lanes(&jw, 36) };
            let count = c.d();
            assert_eq!(count, 1, "case {case}");
            let want = read_joint_rows(&mut c, 1, pair)[0];
            let want_b = read_bodies(&mut c, nb);
            let o = |p: &[f32; 12]| [p[9], p[10], p[11]];
            let got = if pair {
                let (a, b) = two(&mut bodies, 0, 1);
                build_pair_joint(a, b, (0, 1), &poses[0], &poses[1], o(&poses[0]), o(&poses[1]), &joint, [bias; 4])
            } else {
                build_world_joint(&mut bodies[0], 0, &poses[0], o(&poses[0]), &joint, [bias; 4])
            };
            for i in 0..3 {
                eq_lanes(case, "kinv", got.kinv[i], want.kinv[i], 3);
            }
            eq_lanes(case, "ra", got.ra, want.ra, 3);
            eq_lanes(case, "rb", got.rb, want.rb, 3);
            eq_lanes(case, "err", got.err, want.err, 3);
            eq_lanes(case, "impulse", got.impulse, want.impulse, 4);
            for i in 0..nb {
                eq_lanes(case, "v", bodies[i].v, want_b[i].v, 3);
                eq_lanes(case, "w", bodies[i].w, want_b[i].w, 3);
            }
        }
    }

    fn joint_solve_case(name: &str, pair: bool) {
        let Some(cases) = rows(name) else { return };
        for (case, cols) in cases.iter().enumerate() {
            let mut c = Cursor { cols, at: 0 };
            let iters = c.d();
            let nb = c.d();
            let mut bodies = read_bodies(&mut c, nb);
            let nr = c.d();
            let mut rs = read_joint_rows(&mut c, nr, pair);
            let want_b = read_bodies(&mut c, nb);
            let want_r = read_joint_rows(&mut c, nr, pair);
            for _ in 0..iters {
                if pair {
                    solve_pair_joints(&mut bodies, &mut rs);
                } else {
                    solve_world_joints(&mut bodies, &mut rs);
                }
            }
            for i in 0..nb {
                eq_body(case, &bodies[i], &want_b[i]);
            }
            for i in 0..nr {
                eq_lanes(case, "impulse", rs[i].impulse, want_r[i].impulse, 4);
            }
        }
    }

    #[test]
    fn reference_world_joint_build() {
        joint_build_case("solver_wjbuild.tsv", false);
    }

    #[test]
    fn reference_pair_joint_build() {
        joint_build_case("solver_pjbuild.tsv", true);
    }

    #[test]
    fn reference_world_joint_solve() {
        joint_solve_case("solver_wjsolve.tsv", false);
    }

    #[test]
    fn reference_pair_joint_solve() {
        joint_solve_case("solver_pjsolve.tsv", true);
    }

    /// A falling avatar 0.2 into a floor: the solve stops the fall, writes the impulse to the slot and
    /// pushes it out with the pseudo velocity; a slot not overlapping makes no row.
    #[test]
    fn a_falling_body_is_stopped_and_lifted() {
        use crate::physics_manifold::{Contact, PairBody, NORMAL, SEPARATION, SOLVER, WORLD_A};
        let floor = PairBody { pose: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0], avatar: false, dynamic: false, velocity: glam::Vec3::ZERO, friction: 0.5 };
        let me = PairBody { avatar: true, dynamic: true, ..floor };
        let mut k = Contact::new(&me, &floor, 0, 0.05, false);
        k.count = 2;
        for (i, sep) in [(0usize, -0.2f32), (1, 0.1)] {
            k.slots[i][NORMAL + 2] = 1.0;
            k.slots[i][SEPARATION] = sep;
            k.slots[i][WORLD_A + 2] = -1.0;
        }
        let mut body = SolverBody { v: [0.0, 0.0, -5.0, 0.0], inv_mass: [0.01; 4], ..Default::default() };
        let info = StaticSolveBody { origin: [0.0; 3], velocity: [0.0, 0.0, -5.0], with_friction: false };
        let mut ks = [k];
        solve_against_static(&mut body, &info, &mut ks, 0.032);
        assert!(body.v[2].abs() < 1e-5, "{:?}", body.v);
        assert!((body.vp[2] - 3.6).abs() < 1e-3, "{:?}", body.vp);
        assert!((ks[0].slots[0][SOLVER] - 500.0).abs() < 0.05, "impulse {}", ks[0].slots[0][SOLVER]);
        assert_eq!(ks[0].slots[1][SOLVER], 0.0);
        assert!(!ks[0].refreshed);
    }

    /// A body sinking 0.2 into a floor at rest: the velocity stays (no approach), the pseudo velocity
    /// pushes it out at 24 per metre past the margin.
    #[test]
    fn an_overlap_is_pushed_out_by_the_pseudo_pass_only() {
        let mut body = SolverBody { inv_mass: [0.01; 4], ..Default::default() };
        let built = build_static_row(&mut body, 0, [0.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 0.0, 1.0, 0.0], -0.2, [0.0; 3], [-24.0; 4], 0.05, 0.0, false);
        let mut rows = [built.row];
        let mut bodies = [body];
        for _ in 0..4 {
            solve_static(&mut bodies, &mut rows);
        }
        assert_eq!(bodies[0].v, [0.0; 4]);
        assert!((bodies[0].vp[2] - 24.0 * 0.15).abs() < 1e-4, "{:?}", bodies[0].vp);
    }
}
