//! The game's narrow-phase collision routine: a GJK ray cast (`notes\collision-core.md`).
//!
//! [game] The loop, the thresholds, the exits and the hit normal follow the notes'
//! steps 1-13 (005bed70). The support helpers are the transliterations in
//! `work\kernels\support.py` (`sphere_support` = 005e3570, `tri_support` = 005e3710).
//! [game] Johnson's sub-algorithm is the game's (read in 00616ed0, the pair primitive, which
//! `notes\physics.md` 3 finds token-identical to 005bed70's in this step): SOLID's fixed
//! 4-slot simplex and determinant table in double, the first valid subset in descending bit
//! order, and the loop left when none is valid. [game] x87 at 53 bits (see `dx`).

use glam::Vec3;

/// [game] 00b6a404: the segment length squared, the v.w test and the seed test.
const EPS_1E4: f32 = 1e-4;
/// [game] 00b38890: the loop ends when |v|^2 is at or below this (absolute, about 3 cm).
const EPS_DIST2: f32 = 0.001;
/// [game] 005bff95: the iteration limit; running out counts as a hit.
const MAX_ITER: usize = 64;
/// [game] step 13: the contact normal is taken from the last history entry whose |v|^2 > 0.001.
const EPS_WALK: f32 = 0.001;
/// [game] 00b34610: the sphere helper's degenerate-direction threshold on |vA|^2.
const EPS_SPHERE_L2: f32 = 1e-6;

/// The shape's support point for direction v, as the game's helper returns it.
pub trait Support {
    fn support(&self, v: Vec3) -> Vec3;

    /// Optional target-side support point for a Minkowski support pair, the witness the contact
    /// point is weighted from ([game 00616ed0] lines 369-411, `Simplex::witness`). [game emu] Settled
    /// 2026-10-20: the witness weights are bit for bit on every field of the emulator's 90 pair
    /// cells (`pair_sweep::tests::reference_pair_cells`: the witness is Some for those shapes, so
    /// their points come from these weights). The shapes with the default None (the ray tests of
    /// pencil, cube, cone, cylinder, capsule and hull) take the ray's p0 + dir * lambda, bit for
    /// bit (`gjk_world` and `collision_world` hull refs).
    fn witness_support(&self, _v: Vec3) -> Option<Vec3> { None }
}

/// A triangle, support = the game's 005e3710 (`tri_support`).
pub struct Triangle {
    pub a: Vec3,
    pub b: Vec3,
    pub c: Vec3,
}

/// A sphere (ellipsoid), support = the game's 005e3570 `sphere_support(v, A = y, P = radius, B = x)`.
/// `x` is the 3x3 linear part row-major at 0..9 and the translation (the origin) at 9..12.
/// `y` is the 3x3 matrix A row-major.
pub struct Sphere {
    pub radius: f32,
    pub x: [f32; 12],
    pub y: [f32; 9],
}

/// The ray-cast result. `lambda` is metres from P0 along the normalised ray.
#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub point: Vec3,
    pub normal: Vec3,
    pub lambda: f32,
    pub contact: bool,
}

/// [game] order of the dot product (005bed70's uses): z, then y, then x (`fl(a, b, 0)`; the
/// loop takes its orders from `Orders`, this is kept for the tests' printing).
#[cfg(test)]
fn dot_zyx(a: Vec3, b: Vec3) -> f32 {
    (a.z * b.z + a.y * b.y) + a.x * b.x
}

/// One row of the triangle dotted with the direction, summed as 005e3735-005e3760.
fn dot_row(r: Vec3, q: Vec3) -> f32 {
    (r.x * q.x + r.y * q.y) + r.z * q.z
}

impl Support for Triangle {
    /// [game] 005e3710: the vertex with the largest dot; ties go to the LATER row (<=).
    fn support(&self, v: Vec3) -> Vec3 {
        let d0 = dot_row(self.a, v);
        let d1 = dot_row(self.b, v);
        let d2 = dot_row(self.c, v);
        let mask1 = d0 <= d1;
        let m01 = if mask1 { d1 } else { d0 };
        let best01 = if mask1 { self.b } else { self.a };
        if m01 <= d2 {
            self.c
        } else {
            best01
        }
    }
}

impl Support for Sphere {
    fn support(&self, v: Vec3) -> Vec3 {
        sphere_support(v, &self.y, self.radius, &self.x)
    }
}

/// [game] 005e3570, float32 at every step in the disassembly's association order.
/// Returns the point with the last component as the helper's 1.0 dropped.
fn sphere_support(v: Vec3, a: &[f32; 9], p: f32, b: &[f32; 12]) -> Vec3 {
    let (vx, vy, vz) = (v.x, v.y, v.z);
    // w = v * A, row vector (005e3581-005e35e5)
    let w0 = (a[3] * vy + a[6] * vz) + a[0] * vx;
    let w1 = (a[1] * vx + a[4] * vy) + a[7] * vz;
    let w2 = (a[2] * vx + a[5] * vy) + a[8] * vz;
    // squared length (005e35e9-005e3602)
    let l = (w0 * w0 + w2 * w2) + w1 * w1;
    // [game] 005e3606: L <= 1e-6 gives scale 0 (the helper then returns the centre)
    let k: f32 = if l <= EPS_SPHERE_L2 {
        0.0
    } else {
        // 00409a70 stores the sqrt as float; 005e3632 takes 1/s as float
        let s = (l as f64).sqrt() as f32;
        (1.0f64 / (s as f64)) as f32
    };
    let keff = p * k; // 005e365c-005e3665 (*P * k)
    let u0 = w0 * keff;
    let u1 = w1 * keff;
    let u2 = w2 * keff;
    // out = u * B + t (005e367a-005e36e1)
    let ox = ((b[6] * u2 + b[3] * u1) + b[0] * u0) + b[9];
    let oy = ((b[7] * u2 + b[1] * u0) + b[4] * u1) + b[10];
    let oz = ((b[8] * u2 + b[2] * u0) + b[5] * u1) + b[11];
    Vec3::new(ox, oy, oz)
}

/// [game] 00b6b150: the weight sum must exceed this before v is divided by it (f32 1e-8
/// widened to double).
const EPS_DET_SUM: f64 = 1e-8f32 as f64;

/// [game] a dot product of the determinant update, formed on the x87 stack: float products,
/// two adds in the order `o`, no rounding to float in between (006177a0-00617928).
/// [game] 53-bit precision (control word 0x027f, as `work\emu\` runs). Settled 2026-10-20 (`notes\robot-debts\report-U6.md`):
/// the CRT's start-up (00a8bc66) calls 00a8c218, `_controlfp_s(NULL, _PC_53, _MCW_PC)`; no real fninit exists in
/// .text (its 5 byte hits are inside call operands); the 7 precision changes to 24 bits sit in the Scaleform code
/// (00a0ca10 and kin, census platform:scaleform) between save and restore, and every fldcw elsewhere is a rounding
/// save/restore (0xc00). Not read: whether a Scaleform window calls back into physics. Float
/// products are exact in f64, so only the adds round. The emulator references cannot separate 53-bit from
/// 64-bit adds except on rare double-rounding ties, so the gjk_world hits are not evidence for the precision.
fn dx(a: Vec3, b: Vec3, o: [usize; 3]) -> f64 {
    let p = |i: usize| a[i] as f64 * b[i] as f64;
    (p(o[0]) + p(o[1])) + p(o[2])
}

/// The two other components of a three-term sum whose last-added term is `last`. The first
/// add commutes, so a sum of three terms is fixed by which one is added last.
fn others(last: usize) -> [usize; 3] {
    match last {
        0 => [1, 2, 0],
        1 => [0, 2, 1],
        _ => [0, 1, 2],
    }
}

/// `dx` named by the component added last.
fn dl(a: Vec3, b: Vec3, last: usize) -> f64 {
    dx(a, b, others(last))
}

/// A float dot product named by the component added last.
fn fl(a: Vec3, b: Vec3, last: usize) -> f32 {
    let [i, j, k] = others(last);
    (a[i] * b[i] + a[j] * b[j]) + a[k] * b[k]
}

/// The add orders of one compiled copy of the GJK template: each entry names the component
/// (0 x, 1 y, 2 z) or the term added LAST, which is all that fixes a three-term sum.
#[derive(Clone, Copy, Debug)]
pub struct Orders {
    /// |d|^2, the seed's |v|^2 test, v.w, v.dir, the closing |v|^2.
    pub len2: usize,
    pub seed: usize,
    pub vw: usize,
    pub vr: usize,
    pub dist2: usize,
    /// The start-in-contact normal's |walk-back|^2 when two points are left.
    pub contact2: usize,
    /// The three-point updates, the two dot products of each in the order the port writes
    /// them: det[s3][k] (e[j][k].y[j], y[last].e[j][k]), det[s3][j] (e[k][j].y[k],
    /// e[k][j].y[last]), det[s3][last] (e[k][last].y[k], y[j].e[k][last]).
    pub s3: [[usize; 2]; 3],
    /// The four-point block: per det[15][i], the last component of each of its three dot
    /// products (the other slots in increasing order), then which of the three terms is added last.
    pub s4: [([usize; 3], usize); 4],
}

/// [game] 00616ed0 and the ray tests (005bed70 and kin), with their helper calls.
pub const PRIMITIVE: Orders = Orders {
    len2: 0,
    seed: 0,
    vw: 0,
    vr: 0,
    dist2: 0,
    contact2: 1,
    s3: [[1, 2], [0, 2], [1, 2]],
    s4: [([2, 0, 1], 2), ([0, 2, 2], 2), ([1, 1, 2], 2), ([0, 2, 0], 2)],
};

/// [game] the inline-support family of the pair primitives (capsule, cylinder, cone, pencil
/// against each other; read in 0063c3a0 lines 24-422 of its decompile, the locals mapped to
/// the edge / point / det tables by their stack offsets).
pub const INLINE: Orders = Orders {
    len2: 2,
    seed: 1,
    vw: 1,
    vr: 2,
    dist2: 1,
    contact2: 0,
    s3: [[2, 1], [1, 2], [1, 1]],
    s4: [([2, 1, 0], 0), ([2, 2, 2], 0), ([2, 0, 0], 0), ([2, 2, 1], 2)],
};

/// Three terms summed with `last` added last.
fn sum3(t: [f64; 3], last: usize) -> f64 {
    let [i, j, k] = others(last);
    (t[i] + t[j]) + t[k]
}

/// The game's simplex (00616ed0 and 005bed70, `notes\physics.md` 1.3): four FIXED slots, a
/// bit set of the slots in use, and SOLID's determinant table, which lives for the whole
/// cast (entries of subsets without the newest slot are reused from earlier iterations).
struct Simplex {
    /// y = x - p per slot (float).
    y: [Vec3; 4],
    /// The shape's support point p per slot (the start-in-contact normal of 00616ed0 is
    /// built from these).
    p: [Vec3; 4],
    /// The witness per slot, when the shape gives one.
    q: [Option<Vec3>; 4],
    /// edge[i][j] = y[i] - y[j], float.
    edge: [[Vec3; 4]; 4],
    /// det[subset][slot], double.
    det: [[f64; 4]; 16],
    /// The slots in use after the last closest-point step.
    bits: u32,
    /// The slot of the newest point, its bit, and bits | last_bit.
    last: usize,
    last_bit: u32,
    all_bits: u32,
}

/// What the closest-point step did.
enum Closest {
    /// A subset was valid: the new v (bits now names that subset).
    V(Vec3),
    /// [game] no subset is valid, not even the newest point alone: the loop is left with v
    /// and bits as they were (005bed70 / 00616ed0 jump to the end, LAB_00617fed).
    Stuck,
}

impl Simplex {
    fn new() -> Self {
        Simplex {
            y: [Vec3::ZERO; 4],
            p: [Vec3::ZERO; 4],
            q: [None; 4],
            edge: [[Vec3::ZERO; 4]; 4],
            det: [[0.0; 4]; 16],
            bits: 0,
            last: 0,
            last_bit: 1,
            all_bits: 0,
        }
    }

    fn clear(&mut self) {
        self.bits = 0;
    }

    fn count(&self) -> usize {
        self.bits.count_ones() as usize
    }

    /// The slots of `bits`, in slot order.
    fn slots(&self) -> impl Iterator<Item = usize> + '_ {
        (0..4).filter(move |&i| self.bits & (1 << i) != 0)
    }

    /// [game] the new point goes into the LOWEST free slot; the edges to the points in use
    /// and the determinant table are updated (SOLID's compute_det, edge form).
    #[cfg(test)]
    fn add(&mut self, w: Vec3, p: Vec3, q: Option<Vec3>) {
        self.add_with(w, p, q, &PRIMITIVE)
    }

    fn add_with(&mut self, w: Vec3, p: Vec3, q: Option<Vec3>, o: &Orders) {
        let mut last = 0usize;
        let mut last_bit = 1u32;
        while self.bits & last_bit != 0 {
            last_bit <<= 1;
            last += 1;
        }
        self.last = last;
        self.last_bit = last_bit;
        self.y[last] = w;
        self.p[last] = p;
        self.q[last] = q;
        self.all_bits = self.bits | last_bit;
        for i in 0..4 {
            if self.bits & (1 << i) != 0 {
                let e = self.y[i] - self.y[last];
                self.edge[i][last] = e;
                self.edge[last][i] = -e;
            }
        }
        let (y, e) = (self.y, self.edge);
        let det = &mut self.det;
        det[last_bit as usize][last] = 1.0;
        for j in 0..4 {
            let bj = 1u32 << j;
            if self.bits & bj == 0 {
                continue;
            }
            let s2 = (bj | last_bit) as usize;
            // [game] both copies end these two on z
            det[s2][j] = dl(e[last][j], y[last], 2);
            det[s2][last] = dl(e[j][last], y[j], 2);
            for k in 0..j {
                let bk = 1u32 << k;
                if self.bits & bk == 0 {
                    continue;
                }
                let s3 = s2 | bk as usize;
                let lk = (last_bit | bk) as usize;
                let kj = (bk | bj) as usize;
                let [ok, oj, ol] = o.s3;
                det[s3][k] = dl(e[j][k], y[j], ok[0]) * det[s2][j] + dl(y[last], e[j][k], ok[1]) * det[s2][last];
                det[s3][j] = dl(e[k][j], y[k], oj[0]) * det[lk][k] + dl(e[k][j], y[last], oj[1]) * det[lk][last];
                det[s3][last] = dl(e[k][last], y[k], ol[0]) * det[kj][k] + dl(y[j], e[k][last], ol[1]) * det[kj][j];
            }
        }
        if self.all_bits == 15 {
            // [game] the 4-point block (00616ed0: lines 232-257 of work\physics\p_00616ed0.c;
            // the inline family: 0063c3a0 lines 288-311), each copy's add orders in `o.s4`.
            // det[15][i] sums over the other three slots m: e.y[m] * det[15 - bit i][m], with
            // e = e[1][0] for i = 0 and e[0][i] otherwise.
            for i in 0..4 {
                let ed = if i == 0 { e[1][0] } else { e[0][i] };
                let sub = 15 ^ (1usize << i);
                let (lasts, outer) = o.s4[i];
                let mut t = [0.0f64; 3];
                for (n, m) in (0..4).filter(|&m| m != i).enumerate() {
                    t[n] = dl(ed, y[m], lasts[n]) * det[sub][m];
                }
                det[15][i] = sum3(t, outer);
            }
        }
    }

    /// [game] valid(s): every slot of all_bits in s has a positive determinant, and every
    /// slot outside s would not improve on it (det[s | bit][slot] <= 0).
    fn valid(&self, s: u32) -> bool {
        (0..4).all(|i| {
            let b = 1u32 << i;
            if self.all_bits & b == 0 {
                true
            } else if s & b != 0 {
                self.det[s as usize][i] > 0.0
            } else {
                !(self.det[(s | b) as usize][i] > 0.0)
            }
        })
    }

    /// [game] SOLID's closest: the subsets of bits from the highest down, each with the
    /// newest slot added; the FIRST valid one wins. Then the newest slot alone. v is the
    /// determinant-weighted sum of y: the weights rounded to float and the products and
    /// adds in SSE single (00617d7a-00617fa4), the weight sum on the x87 stack, divided
    /// only when it exceeds 1e-8.
    fn closest(&mut self) -> Closest {
        let mut s = self.bits;
        while s != 0 {
            if self.bits & s == s && self.valid(self.last_bit | s) {
                self.bits = self.last_bit | s;
                return Closest::V(self.weighted(&self.y));
            }
            s -= 1;
        }
        if self.valid(self.last_bit) {
            self.bits = self.last_bit;
            return Closest::V(self.y[self.last]);
        }
        Closest::Stuck
    }

    /// sum over bits of det[bits][i] * pts[i] / sum det[bits][i], as the game forms it.
    fn weighted(&self, pts: &[Vec3; 4]) -> Vec3 {
        let row = &self.det[self.bits as usize];
        let mut sum = 0.0f64;
        let mut v = Vec3::ZERO;
        for i in self.slots() {
            sum += row[i];
            v = (row[i] as f32) * pts[i] + v;
        }
        if sum > EPS_DET_SUM {
            v *= (1.0 / sum) as f32;
        }
        v
    }

    /// The witness weighted as the game weights it (after the loop, 00616ed0 lines 369-411),
    /// when every slot in use has one.
    fn witness(&self) -> Option<Vec3> {
        if self.bits == 0 || self.slots().any(|i| self.q[i].is_none()) {
            return None;
        }
        let mut q = [Vec3::ZERO; 4];
        for i in self.slots() {
            q[i] = self.q[i].unwrap_or(Vec3::ZERO);
        }
        let row = &self.det[self.bits as usize];
        let mut sum = 0.0f64;
        let mut v = Vec3::ZERO;
        for i in self.slots() {
            sum += row[i];
            v = (row[i] as f32) * q[i] + v;
        }
        // [game] here the test is sum > 0 (line 406), not > 1e-8
        if sum > 0.0 {
            v *= (1.0 / sum) as f32;
        }
        Some(v)
    }
}

/// [game] 005502f0 / 005571b0: xyz times 1/len (single) when len > 1e-8, else xyz unchanged.
fn normalize_to(v: Vec3, len: f32) -> Vec3 {
    if len > 1e-8 {
        let r = 1.0f32 / len;
        Vec3::new(r * v.x, r * v.y, r * v.z)
    } else {
        v
    }
}

/// [game] 00409a70 followed by `fstp dword`: the root rounded to float.
fn sqrt_f(x: f32) -> f32 {
    (x as f64).sqrt() as f32
}

/// [game] the side rule of both normal cases: -o when walk.o < 0, o when it is > 0; at 0 the
/// walk-back stays only when o is zero.
fn orient(o: Vec3, walk: Vec3) -> Vec3 {
    let d = (walk.y * o.y + walk.z * o.z) + o.x * walk.x;
    if d < 0.0 {
        -o
    } else if d > 0.0 || (o.x * o.x + o.y * o.y) + o.z * o.z != 0.0 {
        o
    } else {
        walk
    }
}

/// [game] the start-in-contact normal by the points left in the simplex (00616ed0 lines
/// 447-535, disassembly 0061856f-00618847). [game emu, 2026-10-20] settled for the ray shapes: the
/// start-in-contact normal is bit for bit on all 903 start-in-contact hits of the emulator's cube,
/// cylinder, cone, pencil and capsule refs (`gjk_world` tests, `start_normal_exact`). The triangle
/// ray test's normal is bit-exact too (`reference_ray_tri`, emulator 005bed70). Not covered: the
/// hull and the swept shapes' start-in-contact branch, and the 2-point walk-back's B103 of 005bed70
/// for the triangle. settles (for those): a start-in-contact reference of 005bed70 (its B103 for the
/// 2-point case, physics.md 152-153: 005571b0 writes the normalised walk-back):
/// - 3 points: the plane of the three SUPPORT points, normalised, on the walk-back's side;
/// - 2 points: the walk-back normalised again. The game also builds dir minus its part along
///   the edge, but 005571b0 then writes the normalised walk-back over it (EAX = that vector,
///   ECX = the walk-back and its length), so the edge plays no part;
/// - 1 or 4 points: -dir.
fn contact_normal(s: &Simplex, walk: Vec3, dir: Vec3, ord: &Orders) -> Vec3 {
    let mut ps = s.slots().map(|i| s.p[i]);
    match s.count() {
        3 => {
            let (a, b, c) = (ps.next().unwrap(), ps.next().unwrap(), ps.next().unwrap());
            let nx = (b.y - a.y) * (c.z - a.z) - (b.z - a.z) * (c.y - a.y);
            let nz = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
            let ny = (c.x - a.x) * (b.z - a.z) - (b.x - a.x) * (c.z - a.z);
            let n = Vec3::new(nx, ny, nz);
            let o = normalize_to(n, sqrt_f((nx * nx + nz * nz) + ny * ny));
            orient(o, walk)
        }
        2 => {
            let o = normalize_to(walk, sqrt_f(fl(walk, walk, ord.contact2)));
            orient(o, walk)
        }
        _ => -dir,
    }
}

/// Ray cast from p0 towards p1 against a convex shape. `origin` seeds the first search
/// direction (for the sphere, the transform's origin x[9..12]; for the triangle, its mean).
/// Returns None for a miss (including a segment shorter than 1 cm).
pub fn ray_cast<S: Support>(p0: Vec3, p1: Vec3, shape: &S, origin: Vec3) -> Option<RayHit> {
    ray_cast_traced(p0, p1, shape, origin, None)
}

/// `ray_cast` as a copy of the template with other add orders compiles it (`INLINE`).
pub fn ray_cast_with<S: Support>(p0: Vec3, p1: Vec3, shape: &S, origin: Vec3, o: &Orders) -> Option<RayHit> {
    ray_cast_core(p0, p1, shape, origin, None, o)
}

/// `ray_cast`, recording at the top of each iteration (simplex size, lambda, v): the
/// values `work\emu\trace_tri.py` prints for the original, for comparing step by step.
pub fn ray_cast_traced<S: Support>(
    p0: Vec3,
    p1: Vec3,
    shape: &S,
    origin: Vec3,
    trace: Option<&mut Vec<(usize, f32, Vec3)>>,
) -> Option<RayHit> {
    ray_cast_core(p0, p1, shape, origin, trace, &PRIMITIVE)
}

fn ray_cast_core<S: Support>(
    p0: Vec3,
    p1: Vec3,
    shape: &S,
    origin: Vec3,
    mut trace: Option<&mut Vec<(usize, f32, Vec3)>>,
    o: &Orders,
) -> Option<RayHit> {
    // [game] step 3: a segment with len^2 < 1e-4 never hits
    let d = p1 - p0;
    let len2 = fl(d, d, o.len2);
    if len2 < EPS_1E4 {
        return None;
    }
    // [game] step 5: len = sqrt(len2), inv = 1/len, dir = d * inv
    let len = (len2 as f64).sqrt() as f32;
    let inv = 1.0f32 / len;
    let dir = d * inv;

    // [game] step 4: seed v = origin - P0, or -d (unnormalised) when |v|^2 <= 1e-4
    let mut v = origin - p0;
    if fl(v, v, o.seed) <= EPS_1E4 {
        v = -d;
    }

    let mut lambda = 0.0f32;
    let mut x = p0;
    let mut simplex = Simplex::new();
    let mut hist_v = [Vec3::ZERO; MAX_ITER];
    let mut hist_f = [0.0f32; MAX_ITER];
    let mut last_adv: Option<usize> = None;
    let mut k_end = 0usize;

    // [game] the float stored beside v: len2 at iteration 0 (`notes\collision-core.md`, row
    // 586), then the |v|^2 the previous iteration ended with.
    let mut dist2 = len2;
    for k in 0..MAX_ITER {
        // [game] step 6: store v and the running distance squared
        hist_v[k] = v;
        hist_f[k] = dist2;
        k_end = k;
        if let Some(t) = trace.as_mut() {
            t.push((simplex.count(), lambda, v));
        }

        let p = shape.support(v);
        let witness = shape.witness_support(v);
        let mut w = x - p;
        let vw = fl(v, w, o.vw);

        // [game] step 6: a full simplex is emptied first
        if simplex.bits == 15 {
            simplex.clear();
        }

        // [game] step 7: advance only when v.w > 1e-4
        if vw > EPS_1E4 {
            // [game] 00616ed0 line 78: (y + z) + x (o.vr = 0); the inline copies end on z
            let vr = fl(v, dir, o.vr);
            // [game] miss when v.dir >= 0
            if vr >= 0.0 {
                return None;
            }
            lambda -= vw / vr;
            // [game] miss when lambda > len
            if lambda > len {
                return None;
            }
            x = p0 + dir * lambda;
            w = x - p;
            simplex.clear();
            last_adv = Some(k);
        }

        // [game] step 8: add w, then the closest point of the simplex is v
        simplex.add_with(w, p, witness, o);
        match simplex.closest() {
            Closest::V(nv) => v = nv,
            Closest::Stuck => break,
        }

        // [game] step 10: end when |v|^2 <= 0.001, or after 64 iterations (for-loop end)
        dist2 = fl(v, v, o.dist2);
        if dist2 <= EPS_DIST2 {
            break;
        }
    }

    // Both ends are a HIT (step 12). The normal is the search direction saved at the
    // iteration of the last advance, normalised (history_v[k] * 1/sqrt(history_f[k])).
    match last_adv {
        Some(k) => {
            let s = (hist_f[k] as f64).sqrt() as f32;
            let inv_n = (1.0f64 / (s as f64)) as f32;
            Some(RayHit {
                point: simplex.witness().unwrap_or(p0 + dir * lambda),
                normal: hist_v[k] * inv_n,
                lambda,
                contact: false,
            })
        }
        None => {
            // [game] start in contact (step 13; 00616ed0 lines 431-446): lambda never
            // advanced. The normal is the newest history entry whose float is > 0.001,
            // divided by the root of that float; the walk stops at iteration 0 whatever its
            // float (len2 there, so the seed is divided by |d|, not by |v|).
            let j = (0..=k_end).rev().find(|&j| hist_f[j] > EPS_WALK).unwrap_or(0);
            let s = (hist_f[j] as f64).sqrt() as f32;
            let inv_n = (1.0f64 / (s as f64)) as f32;
            let walk = hist_v[j] * inv_n;
            Some(RayHit {
                point: simplex.witness().unwrap_or(p0),
                normal: contact_normal(&simplex, walk, dir, o),
                lambda: 0.0,
                contact: true,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simplex_of(pts: &[Vec3]) -> (Simplex, Vec3) {
        let mut s = Simplex::new();
        let mut v = Vec3::ZERO;
        for (i, &p) in pts.iter().enumerate() {
            s.add(p, Vec3::ZERO, Some(Vec3::splat(i as f32)));
            match s.closest() {
                Closest::V(nv) => v = nv,
                Closest::Stuck => panic!("stuck at point {i}"),
            }
        }
        (s, v)
    }

    #[test]
    fn segment_across_the_origin_keeps_both_points_and_their_midpoint() {
        let (s, v) = simplex_of(&[Vec3::new(0.0, 0.0, -100.0), Vec3::new(0.0, 0.0, 100.0)]);
        assert_eq!(s.bits, 0b11);
        assert_eq!(v, Vec3::ZERO);
        // witnesses 0 and 1 weighted half and half
        assert_eq!(s.witness(), Some(Vec3::splat(0.5)));
    }

    #[test]
    fn triangle_around_the_origin_gives_equal_weights() {
        let (s, v) = simplex_of(&[Vec3::X + Vec3::Z, Vec3::Y + Vec3::Z, -Vec3::X - Vec3::Y + Vec3::Z]);
        assert_eq!(s.bits, 0b111);
        assert!(close3(v, Vec3::Z, 1e-6), "v {v:?}");
        assert!(close3(s.witness().unwrap(), Vec3::ONE, 1e-6));
    }

    #[test]
    fn a_point_beyond_the_newest_is_dropped_and_its_slot_reused() {
        // the second point is nearer the origin than the segment between the two: only it stays
        let (mut s, v) = simplex_of(&[Vec3::new(5.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 1.0)]);
        assert_eq!(s.bits, 0b10);
        assert_eq!(v, Vec3::new(0.0, 0.0, 1.0));
        // the next point takes the lowest free slot, 0
        s.add(Vec3::new(0.0, 0.0, -1.0), Vec3::ZERO, None);
        assert_eq!((s.last, s.all_bits), (0, 0b11));
    }

    const REF_TRI: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../work/emu/ref/ray_tri.tsv");
    const REF_SPHERE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../work/emu/ref/ray_sphere.tsv");

    fn tri() -> Triangle {
        Triangle {
            a: Vec3::new(-1.0, -1.0, 0.0),
            b: Vec3::new(2.0, -1.0, 0.0),
            c: Vec3::new(-1.0, 2.0, 0.0),
        }
    }

    fn tri_origin() -> Vec3 {
        // the mean of the three vertices
        Vec3::new(0.0, 0.0, 0.0)
    }

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    fn close3(a: Vec3, b: Vec3, tol: f32) -> bool {
        close(a.x, b.x, tol) && close(a.y, b.y, tol) && close(a.z, b.z, tol)
    }

    // ---- support helpers ----

    #[test]
    fn triangle_support_picks_vertex_and_ties_go_to_later_row() {
        let t = tri();
        assert_eq!(t.support(Vec3::new(1.0, 0.0, 0.0)), t.b);
        assert_eq!(t.support(Vec3::new(-1.0, -1.0, 0.0)), t.a);
        // z = 0 for all: every dot is 0, so the last row c wins
        assert_eq!(t.support(Vec3::new(0.0, 0.0, -1.0)), t.c);
    }

    #[test]
    fn sphere_support_identity_and_degenerate() {
        let ident = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // x = B = identity, translation (1,2,3)
        let x = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 3.0];
        let s = Sphere { radius: 2.0, x, y: ident };
        // S1: v = (0,0,1), r = 2, t = (1,2,3) -> (1,2,5)
        assert!(close3(s.support(Vec3::new(0.0, 0.0, 1.0)), Vec3::new(1.0, 2.0, 5.0), 1e-5));
        // S6: v = 0 -> the centre (degenerate branch)
        assert!(close3(s.support(Vec3::ZERO), Vec3::new(1.0, 2.0, 3.0), 1e-6));
        // S9: a negative radius is not made positive: the reflected point comes back
        let neg = Sphere { radius: -1.0, x: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0], y: ident };
        assert!(close3(neg.support(Vec3::new(0.0, 0.0, 1.0)), Vec3::new(0.0, 0.0, -1.0), 1e-6));
    }

    #[test]
    fn sphere_support_ellipsoid_matches_closed_form() {
        // S3: B = diag(2,1,0.5), A = B^T, v = (1,1,1), r = 1, t = 0
        // expected (4,1,0.25)/sqrt(5.25) = (1.745743, 0.436436, 0.109109)
        let a = [2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.5];
        let x = [2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0];
        let s = Sphere { radius: 1.0, x, y: a };
        let got = s.support(Vec3::new(1.0, 1.0, 1.0));
        assert!(close3(got, Vec3::new(1.745743, 0.436436, 0.109109), 1e-5), "got {got:?}");
    }

    // ---- hand cases ----

    #[test]
    fn hand_cases_triangle() {
        let t = tri();
        let o = tri_origin();

        let h = ray_cast(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -5.0), &t, o)
            .expect("ray down must hit");
        assert!(!h.contact);
        assert!(close(h.lambda, 5.0, 1e-4), "lambda {}", h.lambda);
        assert!(close3(h.normal, Vec3::new(0.0, 0.0, 1.0), 1e-5), "normal {:?}", h.normal);
        assert!(close3(h.point, Vec3::ZERO, 1e-4), "point {:?}", h.point);

        let h = ray_cast(Vec3::new(0.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 5.0), &t, o)
            .expect("ray up must hit");
        assert!(close(h.lambda, 5.0, 1e-4), "lambda {}", h.lambda);
        assert!(close3(h.normal, Vec3::new(0.0, 0.0, -1.0), 1e-5), "normal {:?}", h.normal);

        assert!(ray_cast(Vec3::new(5.0, 5.0, 5.0), Vec3::new(5.0, 5.0, -5.0), &t, o).is_none());
        // stops short of the plane
        assert!(ray_cast(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 1.0), &t, o).is_none());
        // shorter than 1 cm: len^2 = 6.4e-5 < 1e-4
        assert!(ray_cast(Vec3::new(0.0, 0.0, 0.004), Vec3::new(0.0, 0.0, -0.004), &t, o).is_none());

        // in the plane: hits the edge x = -1 at lambda 4, normal -x
        let h = ray_cast(Vec3::new(-5.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 0.0), &t, o)
            .expect("in-plane ray must hit");
        assert!(close(h.lambda, 4.0, 1e-4), "lambda {}", h.lambda);
        assert!(close3(h.normal, Vec3::new(-1.0, 0.0, 0.0), 1e-5), "normal {:?}", h.normal);

        // starts on the triangle: contact, lambda 0
        let h = ray_cast(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -5.0), &t, o)
            .expect("ray from the surface must hit");
        assert!(h.contact);
        assert_eq!(h.lambda, 0.0);
        assert!(close3(h.point, Vec3::ZERO, 0.0));
    }

    // ---- reference files (game's routine run offline) ----

    fn hex(s: &str) -> f32 {
        f32::from_bits(u32::from_str_radix(s, 16).expect("hex column"))
    }

    fn col3(c: &[&str], i: usize) -> Vec3 {
        Vec3::new(hex(c[i]), hex(c[i + 1]), hex(c[i + 2]))
    }

    struct RefRow {
        hit: bool,
        contact: bool,
        point: Vec3,
        normal: Vec3,
        lambda: f32,
    }

    #[derive(Default)]
    struct Tally {
        rows: usize,
        agree: usize,
        both_hit: usize,
        contact_agree: usize,
        nc: usize,
        nc_lambda_ok: usize,
        nc_point_ok: usize,
        nc_normal_ok: usize,
        max_lambda_diff: f32,
        first_mismatch: Vec<(usize, bool, bool)>,
        lambda_outliers: Vec<(usize, f32, f32)>,
        contact_both: usize,
        contact_normal_ok: usize,
    }

    fn tally(t: &mut Tally, row: usize, ours: Option<RayHit>, r: &RefRow) {
        t.rows += 1;
        let ours_hit = ours.is_some();
        if ours_hit == r.hit {
            t.agree += 1;
        } else if t.first_mismatch.len() < 5 {
            t.first_mismatch.push((row, ours_hit, r.hit));
        }
        if let Some(h) = ours {
            if r.hit {
                t.both_hit += 1;
                if h.contact == r.contact {
                    t.contact_agree += 1;
                }
                if h.contact && r.contact {
                    t.contact_both += 1;
                    let dn = h.normal - r.normal;
                    if dn.x.abs() <= 0.02 && dn.y.abs() <= 0.02 && dn.z.abs() <= 0.02 {
                        t.contact_normal_ok += 1;
                    }
                }
                if !h.contact && !r.contact {
                    t.nc += 1;
                    let dl = (h.lambda - r.lambda).abs();
                    if dl > t.max_lambda_diff {
                        t.max_lambda_diff = dl;
                    }
                    if dl <= 0.002 {
                        t.nc_lambda_ok += 1;
                    } else if t.lambda_outliers.len() < 8 {
                        t.lambda_outliers.push((row, h.lambda, r.lambda));
                    }
                    let dp = h.point - r.point;
                    if dp.x.abs() <= 0.003 && dp.y.abs() <= 0.003 && dp.z.abs() <= 0.003 {
                        t.nc_point_ok += 1;
                    }
                    let dn = h.normal - r.normal;
                    if dn.x.abs() <= 0.02 && dn.y.abs() <= 0.02 && dn.z.abs() <= 0.02 {
                        t.nc_normal_ok += 1;
                    }
                }
            }
        }
    }

    fn report(name: &str, t: &Tally) {
        let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
        println!("== {name}: {} rows", t.rows);
        println!(
            "  hit/miss agree {} / {} ({:.2}%); first mismatches (row, ours hit, ref hit) {:?}",
            t.agree,
            t.rows,
            pct(t.agree, t.rows),
            t.first_mismatch
        );
        println!(
            "  both hit {}; contact flag agrees {} / {}",
            t.both_hit, t.contact_agree, t.both_hit
        );
        println!(
            "  non-contact agreed hits {}: |lambda diff| <= 0.002: {} ({:.2}%), point <= 0.003: {} ({:.2}%), normal <= 0.02: {} ({:.2}%)",
            t.nc,
            t.nc_lambda_ok,
            pct(t.nc_lambda_ok, t.nc),
            t.nc_point_ok,
            pct(t.nc_point_ok, t.nc),
            t.nc_normal_ok,
            pct(t.nc_normal_ok, t.nc)
        );
        println!("  largest |lambda diff| over non-contact agreed hits: {:e}", t.max_lambda_diff);
        println!("  first lambda outliers (row, ours, reference): {:?}", t.lambda_outliers);
        println!(
            "  start-in-contact hits {}: normal <= 0.02: {} (reported, not asserted)",
            t.contact_both, t.contact_normal_ok
        );
        assert!(
            t.agree as f64 >= 0.98 * t.rows as f64,
            "{name}: hit/miss agreement {}/{} is below 98%",
            t.agree,
            t.rows
        );
        assert!(
            t.nc_lambda_ok as f64 >= 0.95 * t.nc as f64,
            "{name}: lambda within 0.002 for {}/{} non-contact hits is below 95%",
            t.nc_lambda_ok,
            t.nc
        );
    }

    /// Prints the per-iteration state for the rows named in GJK_TRACE_ROWS (comma
    /// separated), to set beside `python work\emu\trace_tri.py <row>`.
    #[test]
    fn trace_rows() {
        let Ok(rows) = std::env::var("GJK_TRACE_ROWS") else { return };
        let Ok(text) = std::fs::read_to_string(REF_TRI) else { return };
        let lines: Vec<&str> = text.lines().collect();
        for row in rows.split(',').filter_map(|r| r.trim().parse::<usize>().ok()) {
            let c: Vec<&str> = lines[row].split('\t').collect();
            let shape = Triangle { a: col3(&c, 6), b: col3(&c, 9), c: col3(&c, 12) };
            let mut t = Vec::new();
            let hit = ray_cast_traced(col3(&c, 0), col3(&c, 3), &shape, col3(&c, 15), Some(&mut t));
            println!("row {row}: {:?}", hit.map(|h| (h.lambda, h.contact)));
            for (k, (n, lambda, v)) in t.iter().enumerate() {
                println!("  k={k:2} n={n} lambda={lambda:.7} v=({:.7e}, {:.7e}, {:.7e}) f={:.7e}", v.x, v.y, v.z, dot_zyx(*v, *v));
            }
        }
    }

    #[test]
    fn reference_ray_tri() {
        let text = match std::fs::read_to_string(REF_TRI) {
            Ok(s) => s,
            Err(_) => {
                println!("note: {REF_TRI} is absent; skipping the triangle reference test");
                return;
            }
        };
        let mut t = Tally::default();
        for (row, line) in text.lines().filter(|l| !l.is_empty()).enumerate() {
            let c: Vec<&str> = line.split('\t').collect();
            assert_eq!(c.len(), 27, "ray_tri.tsv row {row} has {} columns", c.len());
            let p0 = col3(&c, 0);
            let p1 = col3(&c, 3);
            let shape = Triangle { a: col3(&c, 6), b: col3(&c, 9), c: col3(&c, 12) };
            let origin = col3(&c, 15);
            let r = RefRow {
                hit: c[18] == "1",
                contact: c[19] == "1",
                point: col3(&c, 20),
                normal: col3(&c, 23),
                lambda: hex(c[26]),
            };
            let ours = ray_cast(p0, p1, &shape, origin);
            tally(&mut t, row, ours, &r);
        }
        report("ray_tri.tsv", &t);
    }

    #[test]
    fn reference_ray_sphere() {
        let text = match std::fs::read_to_string(REF_SPHERE) {
            Ok(s) => s,
            Err(_) => {
                println!("note: {REF_SPHERE} is absent; skipping the sphere reference test");
                return;
            }
        };
        let mut t = Tally::default();
        for (row, line) in text.lines().filter(|l| !l.is_empty()).enumerate() {
            let c: Vec<&str> = line.split('\t').collect();
            assert_eq!(c.len(), 37, "ray_sphere.tsv row {row} has {} columns", c.len());
            let p0 = col3(&c, 0);
            let p1 = col3(&c, 3);
            let mut x = [0.0f32; 12];
            for (i, slot) in x.iter_mut().enumerate() {
                *slot = hex(c[7 + i]);
            }
            let mut y = [0.0f32; 9];
            for (i, slot) in y.iter_mut().enumerate() {
                *slot = hex(c[19 + i]);
            }
            let shape = Sphere { radius: hex(c[6]), x, y };
            let origin = Vec3::new(x[9], x[10], x[11]);
            let r = RefRow {
                hit: c[28] == "1",
                contact: c[29] == "1",
                point: col3(&c, 30),
                normal: col3(&c, 33),
                lambda: hex(c[36]),
            };
            let ours = ray_cast(p0, p1, &shape, origin);
            tally(&mut t, row, ours, &r);
        }
        report("ray_sphere.tsv", &t);
    }
}
