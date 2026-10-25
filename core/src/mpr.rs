//! The discrete pair test of the physics step (`docs/physics.md` 1.13): the CollisionBatch kernel
//! 005b4aa0 over 005b3c50, a Minkowski portal refinement (XenoCollide) on the difference B - A, with the
//! shapes' world supports of table 00b8dd94.
//!
//! [game] The broad phase (006e5e00) puts every kept pair node on a discrete list; only a pair whose
//! body moves far for its size this step is swept as well (`sweep_radius`). The discrete test runs at
//! the holders' current poses (+0x108, inverse +0x138) after the sweep's hits are consumed, and its
//! result goes to the contact generator (006e22f0 -> 006e6a10), which leaves the push to the solver.
//!
//! Every operation is in the game's order (SSE, lane by lane): a dot is (y + x) + z; `0 - v` is
//! written as such (it is +0 where v is 0); no w lane reaches an xyz result, so the port keeps three.
//! rsqrtps is the processor's own estimate: the port issues the same instruction (`rsqrt`, through
//! `physics_solver::rsqrt_estimate`) on the machine the game runs on [game 005b7d7e, pencil support 005b7cf0].
//! The emulator's references (`work\emu\gen_refs_mpr.py`, `tests::reference_mpr`; Unicorn computes it
//! exactly, A-PH11) are compared within a tolerance for that reason. Other rsqrtps sites in 005b7cf0-005b8650
//! (byte scan: 005b7fd7, 005b81db, 005b8375) go through the same helper.

use glam::Vec3;

use crate::pair_sweep::inverse_affine3;

/// A shape the discrete test supports here, with its data as the collision packet holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MprShape {
    /// Type 2: the half extents.
    Cube { half: Vec3 },
    /// Type 6: radius, half height, tip height, cosine of the slant (`gjk_shapes::pencil_support`).
    Pencil { r: f32, h: f32, a: f32, c: f32 },
}

impl MprShape {
    /// [game] the shape's type (its vtable +0x10), the batch's row and column.
    pub fn kind(&self) -> u32 {
        match self {
            MprShape::Cube { .. } => 2,
            MprShape::Pencil { .. } => 6,
        }
    }

    /// The local support of the batch's table (00b8dd94): 005b8440's cube part, 005b7cf0 for the pencil.
    fn local_support(&self, l: Vec3) -> Vec3 {
        match *self {
            // [game] 005b8440: the half extent by the sign of each component (cmpltps l < 0: -0 takes +)
            MprShape::Cube { half } => Vec3::new(
                if l.x < 0.0 { -0.0 - half.x } else { half.x },
                if l.y < 0.0 { -0.0 - half.y } else { half.y },
                if l.z < 0.0 { -0.0 - half.z } else { half.z },
            ),
            // [game] 005b7cf0 (the SSE pencil, not 005aff10)
            MprShape::Pencil { r, h, a, c } => {
                let (x, y, z) = (l.x, l.y, l.z);
                let q = (y * y + x * x) + z * z;
                let s = q.sqrt();
                if !(z.abs() <= s * c) {
                    return Vec3::new(0.0, 0.0, if z < 0.0 { 0.0 - a } else { a });
                }
                let ll = x * x + y * y;
                let e = rsqrt(ll);
                let t = ((1.0 - (ll * e) * e) * (e * 0.5) + e) * r;
                Vec3::new(x * t, y * t, if z < 0.0 { 0.0 - h } else { h })
            }
        }
    }
}

/// rsqrtps, the processor's estimate [game 005b7d7e, pencil support 005b7cf0]: the same instruction on the
/// same machine (`physics_solver::rsqrt_estimate`).
fn rsqrt(x: f32) -> f32 {
    crate::physics_solver::rsqrt_estimate(x)
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    (a.y * b.y + a.x * b.x) + a.z * b.z
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
}

fn neg(v: Vec3) -> Vec3 {
    Vec3::new(0.0 - v.x, 0.0 - v.y, 0.0 - v.z)
}

/// [game] 00579740: v / |v|, zero when |v|^2 is not above 0 (as `physics_solver`'s copy).
fn normalize(v: Vec3) -> Vec3 {
    let l2 = (v.y * v.y + v.x * v.x) + v.z * v.z;
    if !(0.0 < l2) {
        return Vec3::ZERO;
    }
    let y0 = rsqrt(l2);
    let y = (1.0 - (y0 * l2) * y0) * (0.5 * y0) + y0;
    Vec3::new(y * v.x, y * v.y, y * v.z)
}

/// A shape at a holder's pose: +0x108 and its inverse +0x138.
#[derive(Clone, Copy, Debug)]
pub struct Posed {
    pub shape: MprShape,
    pub world: [f32; 12],
    pub inverse: [f32; 12],
}

impl Posed {
    /// [game] the holder's inverse is 0052ff10's, the same cofactors as 005301d0 (`inverse_affine3`)
    /// without its 1e-8 guard (a holder's pose is never singular). The reference test checks it.
    pub fn new(shape: MprShape, world: [f32; 12]) -> Self {
        Posed { shape, world, inverse: inverse_affine3(&world) }
    }

    fn origin(&self) -> Vec3 {
        Vec3::new(self.world[9], self.world[10], self.world[11])
    }

    /// [game] the world support of 00b8dd94 (005b7c20, 005b8440): d into the shape's frame through the
    /// inverse rows, ((y R1 + x R0) + z R2) + 0 R3, the local support (w 1), and back out through the
    /// world rows, ((y W1 + x W0) + z W2) + 1 W3.
    fn support(&self, d: Vec3) -> Vec3 {
        let m = &self.inverse;
        let lane = |j: usize| ((d.y * m[3 + j] + d.x * m[j]) + d.z * m[6 + j]) + 0.0 * m[9 + j];
        let s = self.shape.local_support(Vec3::new(lane(0), lane(1), lane(2)));
        let w = &self.world;
        let out = |j: usize| ((s.y * w[3 + j] + s.x * w[j]) + s.z * w[6 + j]) + w[9 + j];
        Vec3::new(out(0), out(1), out(2))
    }
}

/// The 0x48-byte result record of one discrete test (005b4aa0 / 005b3c50).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MprRecord {
    /// +0x00 / +0x0c: A's and B's contact points (world).
    pub pa: Vec3,
    pub pb: Vec3,
    /// +0x18: the separation along the normal, as a vector ((pB - pA) . n) n.
    pub depth: Vec3,
    /// +0x24: the normal (from B toward A).
    pub normal: Vec3,
    /// +0x30: written only when the first portal ray is collinear with the centres (the depth there).
    pub collinear_depth: Option<f32>,
    /// +0x44: the hit (also the refinement's second-pass flag).
    pub hit: bool,
}

/// [game] 005b3c50: the portal search and refinement. `n0` the first direction, `ca` / `cb` the two
/// interior points, `v0` the difference's interior point (cB - cA). On the first contact found (the record's
/// hit flag clear) the test runs again from the contact (cA' = pA, cB' = pB, v0' = (pB - pA) - n, along n);
/// on the second the points are moved onto the portal's plane through v1.
fn portal(a: &Posed, b: &Posed, n0: Vec3, ca: Vec3, cb: Vec3, v0: Vec3, rec: &mut MprRecord) -> bool {
    let mut n0 = n0;
    if ((n0.y * n0.y + n0.x * n0.x) + n0.z * n0.z).abs() < 1e-5 {
        n0 = Vec3::new(0.0, 0.0, 0.001);
    }
    // A's support against -n, B's along n
    let sup = |n: Vec3| {
        let pa = a.support(neg(n));
        let pb = b.support(n);
        (pa, pb, pb - pa)
    };
    let (mut a1, mut b1, mut v1) = sup(n0);
    if dot(v1, n0) <= 0.0 {
        return false;
    }
    let mut n = cross(v1, v0);
    if dot(n, n) < 1e-5 {
        // the ray from v0 runs through v1: the contact is v1's
        rec.hit = true;
        let nn = normalize(v1 - v0);
        rec.normal = nn;
        rec.pa = a1;
        rec.pb = b1;
        let d = b1 - a1;
        let depth = ((d.z * nn.z) + (d.y * nn.y)) + d.x * nn.x;
        rec.collinear_depth = Some(depth);
        rec.depth = Vec3::new(depth * nn.x, nn.y * depth, nn.z * depth);
        return true;
    }
    let (mut a2, mut b2, mut v2) = sup(n);
    if dot(v2, n) <= 1e-5 {
        return false;
    }
    n = cross(v1 - v0, v2 - v0);
    if 0.0 < dot(n, v0) {
        std::mem::swap(&mut a1, &mut a2);
        std::mem::swap(&mut b1, &mut b2);
        std::mem::swap(&mut v1, &mut v2);
        n = neg(n);
    }
    let mut outer = 0xff;
    loop {
        let (mut a3, mut b3, mut v3) = sup(n);
        if dot(n, v3) <= 0.0 {
            return false;
        }
        if dot(cross(v1, v3), v0) < 0.0 {
            (a2, b2, v2) = (a3, b3, v3);
            n = cross(v1 - v0, v3 - v0);
        } else if dot(cross(v3, v2), v0) < 0.0 {
            (a1, b1, v1) = (a3, b3, v3);
            n = cross(v3 - v0, v2 - v0);
        } else {
            // the portal (v1, v2, v3) holds the origin ray: refine it
            let mut hit = false;
            let mut count = 199;
            loop {
                let m = cross(v2 - v1, v3 - v1);
                let l2 = (m.y * m.y + m.x * m.x) + m.z * m.z;
                if l2 <= 1e-5 {
                    return false;
                }
                if !hit {
                    hit = 0.0 <= dot(m, v1);
                }
                let e = rsqrt(l2);
                let s = (1.0 - (e * l2) * e) * (e * 0.5) + e;
                let nn = if 0.0 < l2 { Vec3::new(s * m.x, s * m.y, s * m.z) } else { Vec3::ZERO };
                let (a4, b4, v4) = sup(nn);
                if dot(v4 - v3, nn) <= 0.001 || 0.0 <= 0.0 - dot(v4, nn) {
                    if !hit {
                        return false;
                    }
                    return contact(a, b, ca, cb, v0, nn, [(a1, b1, v1), (a2, b2, v2), (a3, b3, v3)], rec);
                }
                let c = cross(v4, v0);
                let replace_v1 = if 0.0 < dot(v1, c) {
                    if 0.0 < dot(v2, c) {
                        true
                    } else {
                        (a3, b3, v3) = (a4, b4, v4);
                        false
                    }
                } else if 0.0 < dot(v3, c) {
                    (a2, b2, v2) = (a4, b4, v4);
                    false
                } else {
                    true
                };
                if replace_v1 {
                    (a1, b1, v1) = (a4, b4, v4);
                }
                count -= 1;
                if count <= 0 {
                    break;
                }
            }
        }
        outer -= 1;
        if outer <= 0 {
            return false;
        }
    }
}

/// [game] 005b3c50 lines 356-485: the contact on the final portal (barycentric weights of the origin's
/// projection), then the second pass or the points moved onto the portal plane.
#[allow(clippy::too_many_arguments)]
fn contact(a: &Posed, b: &Posed, ca: Vec3, cb: Vec3, v0: Vec3, nn: Vec3, p: [(Vec3, Vec3, Vec3); 3], rec: &mut MprRecord) -> bool {
    let [(a1, b1, v1), (a2, b2, v2), (a3, b3, v3)] = p;
    let mut w0 = dot(cross(v1, v2), v3);
    let mut w1 = dot(cross(v3, v2), v0);
    let mut w2 = dot(cross(v0, v1), v3);
    let mut w3 = dot(cross(v2, v1), v0);
    let mut sum = ((w0 + w1) + w2) + w3;
    if sum <= 0.0 {
        w0 = 0.0;
        w1 = dot(cross(v2, v3), nn);
        w2 = dot(cross(v3, v1), nn);
        w3 = dot(cross(v1, v2), nn);
        sum = (w2 + w1) + w3;
    }
    let r = 1.0 / sum;
    let inv = (1.0 - r * sum) * r + r;
    let pa = (((a1 * w1 + ca * w0) + a2 * w2) + a3 * w3) * inv;
    let pb = (((cb * w0 + b1 * w1) + b2 * w2) + b3 * w3) * inv;
    if !rec.hit {
        rec.hit = true;
        let ok = portal(a, b, nn, pa, pb, (pb - pa) - nn, rec);
        if !ok {
            rec.hit = false;
        }
        return ok;
    }
    rec.normal = nn;
    let da = pa - a1;
    let db = pb - b1;
    let k = dot(da, nn);
    let pa = (da - nn * k) + a1;
    let k2 = dot(db, nn);
    let pb = (db - nn * k2) + b1;
    rec.pa = pa;
    rec.pb = pb;
    let k3 = dot(pb - pa, nn);
    rec.depth = nn * k3;
    true
}

/// [game] 005b4aa0: the discrete test of A against B at their poses, the interior points their origins
/// (w 1) and the first direction cA - cB. Some(record) on a hit.
pub fn pair_test(a: &Posed, b: &Posed) -> Option<MprRecord> {
    let ca = a.origin();
    let cb = b.origin();
    let v0 = cb - ca;
    let mut rec = MprRecord::default();
    portal(a, b, neg(v0), ca, cb, v0, &mut rec).then_some(rec)
}

/// [game] 006dcf00: the radius the broad phase gates a pair's sweep on, for a holder moving by `d`
/// this step: 0.8 of the shape's extent from its centre along d (005b7260: the world support along
/// unit d, minus the origin, dotted with d); a cube's is at most its least half extent times its scale
/// (0056fa90, the basis rows' lengths). [game 0056fa90, third pass] each row length is 00409a70 of a sum of three
/// products passed as a float argument, so the sum is rounded to float before the square root, as here.
/// [game 0056fa90, 00409a70] settled 2026-10-20 (the row sums are float, as 00409a70's argument is;
/// `nav_sqrt_wrap` passes its float argument to CIsqrt): the port's f32 sums and f64 root match it.
pub fn sweep_radius(p: &Posed, d: Vec3) -> f32 {
    let u = normalize(d);
    let s = p.support(u) - p.origin();
    let r = dot(s, u) * 0.8;
    match p.shape {
        MprShape::Cube { half } => {
            let w = &p.world;
            let row = |k: usize| (((w[3 * k] * w[3 * k] + w[3 * k + 1] * w[3 * k + 1]) + w[3 * k + 2] * w[3 * k + 2]) as f64).sqrt() as f32;
            let mut least = row(0) * half.x;
            if row(1) * half.y <= least {
                least = row(1) * half.y;
            }
            if row(2) * half.z <= least {
                least = row(2) * half.z;
            }
            if r < least { r } else { least }
        }
        MprShape::Pencil { .. } => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(w: &str) -> f32 {
        f32::from_bits(u32::from_str_radix(w, 16).unwrap())
    }

    fn words(v: &[f32]) -> Vec<u32> {
        v.iter().map(|x| x.to_bits()).collect()
    }

    fn shape(kind: u32, d: &[f32]) -> MprShape {
        match kind {
            2 => MprShape::Cube { half: Vec3::new(d[0], d[1], d[2]) },
            6 => MprShape::Pencil { r: d[0], h: d[1], a: d[2], c: d[3] },
            _ => unreachable!(),
        }
    }

    /// The 0x48-byte record 005b4aa0 leaves, from the port's result (the buffer was 0xee bytes).
    fn image(rec: Option<MprRecord>, info: [u32; 3]) -> Vec<u32> {
        let mut out = vec![0xeeee_eeeeu32; 18];
        out[0xd] = 0xffff_ffff;
        out[0xe] = info[2];
        out[0xf] = info[0];
        out[0x10] = info[1];
        out[0x11] = 0xeeee_0000 | rec.is_some() as u32;
        if let Some(r) = rec {
            let put = |o: &mut Vec<u32>, at: usize, v: Vec3| o[at..at + 3].copy_from_slice(&words(&[v.x, v.y, v.z]));
            put(&mut out, 0, r.pa);
            put(&mut out, 3, r.pb);
            put(&mut out, 6, r.depth);
            put(&mut out, 9, r.normal);
            if let Some(d) = r.collinear_depth {
                out[0xc] = d.to_bits();
            }
        }
        out
    }

    #[test]
    fn reference_mpr() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../work/emu/ref");
        let mut total = 0;
        for (ka, kb) in [(6u32, 6u32), (2, 6), (2, 2)] {
            let path = format!("{dir}/mpr_{ka}_{kb}.tsv");
            let Ok(text) = std::fs::read_to_string(&path) else {
                eprintln!("no references at {path}");
                continue;
            };
            let (mut cases, mut hits, mut bad, mut bad_inverse) = (0, 0, 0, 0);
            for line in text.lines().filter(|l| !l.is_empty()) {
                let c: Vec<&str> = line.split('\t').collect();
                let fl = |i: usize, n: usize| -> Vec<f32> { c[i..i + n].iter().map(|w| f(w)).collect() };
                let (da, db) = (fl(0, 4), fl(4, 4));
                let m = |i: usize| -> [f32; 12] { fl(i, 12).try_into().unwrap() };
                let (aw, ai, bw, bi) = (m(8), m(20), m(32), m(44));
                let info = [56, 57, 58].map(|i| u32::from_str_radix(c[i], 16).unwrap());
                let ret = c[59];
                let want: Vec<u32> = c[61..79].iter().map(|w| u32::from_str_radix(w, 16).unwrap()).collect();
                let a = Posed::new(shape(ka, &da), aw);
                let b = Posed::new(shape(kb, &db), bw);
                if words(&a.inverse) != words(&ai) || words(&b.inverse) != words(&bi) {
                    bad_inverse += 1;
                }
                let a = Posed { inverse: ai, ..a };
                let b = Posed { inverse: bi, ..b };
                let rec = pair_test(&a, &b);
                cases += 1;
                hits += rec.is_some() as usize;
                let got = image(rec, info);
                if got != want || (ret == "1") != rec.is_some() {
                    bad += 1;
                    if bad <= 3 {
                        eprintln!("[{ka}][{kb}] differs:\n want {want:08x?}\n got  {got:08x?}");
                    }
                }
            }
            eprintln!("mpr [{ka}][{kb}]: {cases} cases, {hits} hits, {bad} differ, {bad_inverse} inverses differ");
            assert_eq!(bad, 0, "[{ka}][{kb}]");
            assert_eq!(bad_inverse, 0, "[{ka}][{kb}] 0052ff10");
            total += cases;
        }
        if total == 0 {
            eprintln!("no MPR references (made locally by the emulator): skipped");
        }
    }
}
