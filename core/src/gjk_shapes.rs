//! Support helpers of the other collision shapes, for `gjk::ray_cast`.
//!
//! Sources: `notes\collision-support-2.md` (cylinder 005b0180, cone 005b0060, pencil 005aff10,
//! capsule 005e3400) and `notes\collision-support-hull.md` (hulls 005afcd0, 005afbb0, 005afa90).
//! Each helper is an f32 transliteration of its Python copy (`work\kernels\support2.py`,
//! `work\kernels\support_hull.py`): the same operations in the same order, the same branches and
//! comparisons. Where the Python takes a square root or a reciprocal through float64 and rounds
//! back to float32, so does this code.
//!
//! Reference data: `work\emu\gen_refs_shapes.py` runs the original code in the emulator and writes
//! `work\emu\ref\support_<name>.tsv`. The tests at the end compare bit for bit.
//!
//! The `local_support` methods of the shape structs give the helper's point in the shape's own
//! frame. They do NOT implement `gjk::Support`: how the ray test moves v into the shape's frame and
//! the point back out is not read, and guessing it is not allowed.

use glam::Vec3;
use std::cell::Cell;

/// [game] 00b34610: 1e-6, float32 0x358637bd.
const EPS: f32 = 1e-6;

/// [game] 00409a70: `fld; call _CIsqrt; fstp dword`: the square root rounded to float32.
fn sqrt_store(x: f32) -> f32 {
    (x as f64).sqrt() as f32
}

/// [game] `fld1; fdivrp; fmul dword [r]; fstp dword`: float32 of (1/s) * r, with 1/s and the
/// product both in float64 (the x87 stack at 53 bits: [game] 00a8c218 sets _PC_53 at the CRT's start-up, as in
/// gjk.rs `dx`, settled 2026-10-20).
fn recip_times(s: f32, r: f32) -> f32 {
    ((1.0f64 / (s as f64)) * (r as f64)) as f32
}

/// Cylinder along z, FUN_005b0180. `r` = P+0 (radius), `h` = P+4 (half height) [assumed names].
/// Rim point in the direction of (x, y) at height +h or -h by the sign of z; the axis path
/// (x, y near zero) gives the cap centre.
pub fn cylinder_support(v: Vec3, r: f32, h: f32) -> Vec3 {
    let (x, y, z) = (v.x, v.y, v.z);
    // [game] 005b0192-005b01a5: L = x*x + y*y (float32)
    let l = x * x + y * y;
    // [game] 005b01a9: 1e-6 < L takes the rim path
    let (ox, oy) = if EPS < l {
        let s = sqrt_store(l); // 005b0206
        let t = recip_times(s, r); // 005b020b-005b021b
        (x * t, t * y) // 005b0241, 005b0245
    } else {
        (0.0, 0.0) // 005b01db / 005b01f5: xorps
    };
    // [game] z >= 0 (not z < 0, so NaN takes +h): +h; else 0 - h (005b01cf / 005b022e)
    let oz = if !(z < 0.0) { h } else { -0.0f32 - h };
    Vec3::new(ox, oy, oz)
}

/// Cone, FUN_005b0060. Base disc at z = -h with radius `r`; apex at (0, 0, `a`); `c` = P+0xc is the
/// cosine of the half angle. The apex is the support when c*|v| <= z.
/// [game 005b00a5] `fmul [P+0xc]` is the cone's c; [data zone.str#715 CollisionConePacket] the type descriptor
/// puts `radius` at +0, `height14` at +4, `height34` at +8 and `angle` at +0xc, and the loader (005b9ebd) stores
/// the packet block's own address (no copy: 005b9e9a-005b9ebd), so P+0xc is the packet's `angle`. The stored
/// values are the geometric cosine r / sqrt(r^2 + (height14 + height34)^2): 0.955334 for r 5.92142, 0.457942,
/// 1.37383 (0.9553342 computed); 0.952394 for 4.86402, 0.389253, 1.16776 (0.952390 computed, rounding of the inputs).
/// [trace, not settled] the other caller, FUN_005adfd0 (005ae13b / 005ae188 / 005ae1d5 call 005b0060), passes a
/// block at esp+0xa8 / 0xb4 / 0xc0 built in its own frame (005ae0c8-005ae0d1 store and FUN_0052fbf0 call): its
/// +0xc is not traced, so this shows the packet path only. settles: tracing the stores into that frame block
/// (005ae06c-005ae120, the callee 0052fbf0's outputs).
pub fn cone_support(v: Vec3, r: f32, h: f32, a: f32, c: f32) -> Vec3 {
    let (x, y, z) = (v.x, v.y, v.z);
    // [game] 005b0061-005b0091: Q = (y*y + z*z) + x*x (float32)
    let y2 = y * y;
    let z2 = z * z;
    let x2 = x * x;
    let q = (y2 + z2) + x2;
    let nv = sqrt_store(q); // 005b00a0
    // [game] 005b00a5: A = nv * c, exact in float64; 005b00a8-005b00b4: tip when A < z
    let big_a = (nv as f64) * (c as f64);
    if big_a < (z as f64) {
        return Vec3::new(0.0, 0.0, a); // 005b0159-005b0173
    }
    // [game] 005b00cb: L2 = y*y + x*x
    let l2 = y2 + x2;
    let (ox, oy) = if EPS < l2 {
        let rho = sqrt_store(l2); // 005b010b
        let t = recip_times(rho, r); // 005b0110-005b011f
        (x * t, t * y) // 005b012b, 005b0137
    } else {
        (r, 0.0) // 005b00dc-005b00f1: degenerate, the rim point at +r
    };
    Vec3::new(ox, oy, -0.0f32 - h) // 005b00e8 / 005b0143
}

/// Pencil, FUN_005aff10. Base disc at z = +h or -h (by the sign of z) with radius `r`; tip on the
/// axis at +a or -a. `c` = P+0xc as for the cone. [game 006e7e90, third pass] the robot's pencil sets +0x10 =
/// h / sqrt((5/22 H)^2 + r^2) with h = 3/11 H, which is the port's test formula (the same expression). Settled for
/// the pencil; the cone's +0xc is still open. The pencil's +0xc is settled by 006e7e90 (not a stand-in).
pub fn pencil_support(v: Vec3, r: f32, h: f32, a: f32, c: f32) -> Vec3 {
    let (x, y, z) = (v.x, v.y, v.z);
    // [game] 005aff13-005aff79: Q = (y*y + z*z) + x*x, nv = float(sqrt(Q)) (005aff7e)
    let y2 = y * y;
    let z2 = z * z;
    let x2 = x * x;
    let q = (y2 + z2) + x2;
    let nv = sqrt_store(q);
    // [game] 005aff83: A = nv * c (exact); 005aff51: |z| (fabs, exact); tip when A < |z|
    let big_a = (nv as f64) * (c as f64);
    let az = (z as f64).abs();
    if big_a < az {
        // [game] 005b0025-005b0057: tip +a if z >= 0, else 0 - a
        let oz = if !(z < 0.0) { a } else { -0.0f32 - a };
        return Vec3::new(0.0, 0.0, oz);
    }
    // [game] 005affa5-005affa8: L2 = y*y + x*x; 1e-6 < L2 takes the rim path
    let l2 = y2 + x2;
    let t = if EPS < l2 {
        let rho = sqrt_store(l2); // 005affbf
        recip_times(rho, r) // 005affc4-005affcd
    } else {
        0.0 // 005affd9: degenerate, t = +0
    };
    // [game] 005affdc-005afffa: x*t and y*t (a zero t keeps the sign of x, y)
    let ox = x * t;
    let oy = y * t;
    // [game] z >= 0: +h; else 0 - h (005affdf-005affee)
    let oz = if !(z < 0.0) { h } else { -0.0f32 - h };
    Vec3::new(ox, oy, oz)
}

/// Capsule, FUN_005e3400. `b` is the shape's 9 floats: endpoint 1 = (b0, b1, b2), endpoint 2 =
/// (b4, b5, b6), radius b8 [game 005e3400: the game's own support reads the same nine floats,
/// B3 and B7 unused, and adds B8 times v/|v| (0 when |v|^2 <= 1e-6), with the endpoint chosen by
/// the larger dot]. b3 and b7 are not read.
pub fn capsule_support(v: Vec3, b: &[f32; 9]) -> Vec3 {
    let (x, y, z) = (v.x, v.y, v.z);
    let (b0, b1, b2) = (b[0], b[1], b[2]);
    let (b4, b5, b6) = (b[4], b[5], b[6]);
    let b8 = b[8];
    // [game] 005e3411-005e342a: Q3 = (x*x + y*y) + z*z (float32)
    let q3 = ((x * x) + (y * y)) + (z * z);
    // [game] 005e342e: 1e-6 < Q3 takes the normalising path; u = float(1/nv) (005e3450-005e3462)
    let u = if EPS < q3 {
        let nv = sqrt_store(q3);
        ((1.0f64) / (nv as f64)) as f32
    } else {
        0.0 // 005e346e
    };
    // [game] 005e3480-005e3485: W = b8 * u; 005e34a6-005e34b4: X, Y, Z = (x, y, z) * W
    let w = b8 * u;
    let xx = x * w;
    let yy = y * w;
    let zz = z * w;
    // [game] 005e34b8-005e34f8: S1 = (b2*z + b1*y) + b0*x, S2 = (b6*z + b5*y) + b4*x
    let s1 = ((b2 * z) + (b1 * y)) + (b0 * x);
    let s2 = ((b6 * z) + (b5 * y)) + (b4 * x);
    // [game] 005e34fc: S1 > S2 takes endpoint 1 (005e3505-005e354c), else endpoint 2 (005e352d)
    if s1 > s2 {
        Vec3::new(b0 + xx, b1 + yy, b2 + zz)
    } else {
        Vec3::new(xx + b4, b5 + yy, b6 + zz)
    }
}

/// [game 005afcd0, Hull8, read in full 2026-10-20] the game has NO step bound: it climbs while a link
/// gives a strictly greater dot (005afd40-005afde0) and stops when a full pass over the ring finds none
/// (`pbVar4 == pbVar7`, 005afd9f). Each accepted step strictly raises the dot over finitely many corners,
/// so the walk always ends and this constant can never be reached by a finite hull. It stays as a port
/// assert against a bad block (the Python copy raises at the same count). Measured 2026-10-20 (`cargo test
/// support_hull -- --nocapture`, 3,075 walks): longest 31 steps (Hull8, 90 pairs), Hull16 30, Hull32 29.
/// Not read: the Hull16 / Hull32 exit code (005afbb0 / 005afa90 are said to be the same walk).
const HULL_MAX_STEPS: usize = 1_000_000;

/// A convex hull block in the width-independent form (`notes\collision-support-hull.md`).
///
/// Face k has three corners j = 0..3; corner j is pair p = 3k + j: `(vertex index, link)`, where
/// the link is the signed step, in pair units from the end of the pair, to the corner at the same
/// vertex in the neighbouring face. Positions ("units") count pair sizes from the block's cache
/// origin: face k's record is units 4k..4k+3, unit 4k is the unused leading slot and unit
/// 4k+1+j is corner j. This is how the Hull8, Hull16 and Hull32 byte layouts reduce to one form.
/// [game emu, fifth pass] the records start on the grid of their stride with no pad: the emulator's hull blocks
/// (work\emu\gen_refs_rayhull.py, built at a 32-aligned address, records at the origin) and the support / ray
/// references (`support_hull8/16/32`, `ray_shape_hull8/16`) match this layout bit for bit (gjk_shapes and
/// collision_world tests, run 2026-10-20). The loader's own layout is still not read (the emulator's blocks are
/// the builder's, not the loader's): settles: the hull loader (00b8dd94's hull entries 7450-74e0) for a real block.
pub struct HullBlock {
    /// Vertex i as x, y, z, w. The walk multiplies w by 0, so w must be finite (the game fills 1.0).
    pub vertices: Vec<[f32; 4]>,
    /// Corner pairs, p = 3k + j.
    pub pairs: Vec<(u32, i32)>,
}

/// The pair at a unit position. Panics where the game would read outside the block or an unused
/// slot (the Python copy raises `Fault` there).
fn corner_index(unit: i64, pairs: usize) -> usize {
    let slot = unit.rem_euclid(4);
    assert!(slot != 0, "hull walk reached unit {unit}, an unused slot");
    let p = 3 * unit.div_euclid(4) + (slot - 1);
    assert!(p >= 0 && (p as usize) < pairs, "hull walk reached unit {unit}, outside the block");
    p as usize
}

/// Hull support, FUN_005afcd0 (Hull8), FUN_005afbb0 (Hull16), FUN_005afa90 (Hull32). The three
/// differ only in the byte encoding (index width, vertex base, record stride), which the
/// `HullBlock` form drops, so one walk serves all three.
///
/// `start` is the caller's cache (*arg2 in the game), a unit position. Returns the farthest vertex
/// found by the walk and the new cache, the unit of that corner (the game writes (m - F) >> shift).
pub fn hull_support(block: &HullBlock, v: Vec3, start: u32) -> (Vec3, u32) {
    let (p, m, _) = hull_walk(block, v, start);
    (p, m)
}

/// The walk of `hull_support`, also returning its step count (the number of times the loop ran): the
/// tests measure it against the reference blocks (`HULL_MAX_STEPS`).
fn hull_walk(block: &HullBlock, v: Vec3, start: u32) -> (Vec3, u32, usize) {
    let (vx, vy, vz) = (v.x, v.y, v.z);
    let np = block.pairs.len();
    let vertex_at = |unit: i64| block.pairs[corner_index(unit, np)].0 as usize;
    // [game] 005afd15-005afd34: ((y*vy + z*vz) + x*vx) + w*0
    let dot = |vi: usize| -> f32 {
        let [x, y, z, w] = block.vertices[vi];
        let d = (y * vy + z * vz) + x * vx;
        d + w * 0.0
    };
    // [game] 005afcf0-005afcf8: the start position is the cache
    let mut q = start as i64;
    let mut best = dot(vertex_at(q));
    let mut m = q;
    let mut steps = 0usize;
    loop {
        steps += 1;
        assert!(steps <= HULL_MAX_STEPS, "hull walk does not end");
        // [game] 005afd40-005afd4b: candidate = next corner of the record; wrap from the last corner
        let mut c = q + 1;
        if c.rem_euclid(4) == 0 {
            c -= 3;
        }
        let d = dot(vertex_at(c));
        // [game] 005afd84-005afd87: strictly greater (ja) improves
        if d > best {
            best = d;
            q = c;
            // [game] 005afde0: back at m, stop
            if c == m {
                break;
            }
            m = c;
        } else {
            // [game] 005afd89-005afd9d: follow the link, then the same wrap
            let s = block.pairs[corner_index(q, np)].1 as i64;
            let mut t = q + 1 + s;
            if t.rem_euclid(4) == 0 {
                t -= 3;
            }
            q = t;
            if q == m {
                break;
            }
        }
    }
    // [game] 005afd9f-005afdc6: the vertex at m; 005afdc4-005afdca: the cache is m
    let [x, y, z, _] = block.vertices[vertex_at(m)];
    (Vec3::new(x, y, z), m as u32, steps)
}

/// A convex hull as a collision shape. `cache` is the walk's start position, kept between calls
/// as the game keeps it in the caller's record (`local_support` writes it back).
pub struct Hull {
    pub block: HullBlock,
    pub cache: Cell<u32>,
}

impl Hull {
    /// The hull's farthest vertex in direction `v`, in the hull's own frame.
    pub fn local_support(&self, v: Vec3) -> Vec3 {
        let (p, c) = hull_support(&self.block, v, self.cache.get());
        self.cache.set(c);
        p
    }
}

/// Cylinder shape: radius `r`, half height `h`, axis z. Frame: see `cylinder_support`.
pub struct Cylinder {
    pub r: f32,
    pub h: f32,
}

impl Cylinder {
    pub fn local_support(&self, v: Vec3) -> Vec3 {
        cylinder_support(v, self.r, self.h)
    }
}

/// Cone shape (`cone_support`): radius `r`, half height `h`, tip height `a`, cosine `c`.
pub struct Cone {
    pub r: f32,
    pub h: f32,
    pub a: f32,
    pub c: f32,
}

impl Cone {
    pub fn local_support(&self, v: Vec3) -> Vec3 {
        cone_support(v, self.r, self.h, self.a, self.c)
    }
}

/// Pencil shape (`pencil_support`): radius `r`, half height `h`, tip height `a`, cosine `c`.
pub struct Pencil {
    pub r: f32,
    pub h: f32,
    pub a: f32,
    pub c: f32,
}

impl Pencil {
    pub fn local_support(&self, v: Vec3) -> Vec3 {
        pencil_support(v, self.r, self.h, self.a, self.c)
    }
}

/// Capsule shape (`capsule_support`): the 9 floats of the game's capsule record.
pub struct Capsule {
    pub b: [f32; 9],
}

impl Capsule {
    pub fn local_support(&self, v: Vec3) -> Vec3 {
        capsule_support(v, &self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REF_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../work/emu/ref/");

    /// The reference TSV, or None (with a note) when the generator has not been run.
    fn read_ref(name: &str) -> Option<String> {
        let path = format!("{REF_DIR}support_{name}.tsv");
        match std::fs::read_to_string(&path) {
            Ok(t) => Some(t),
            Err(e) => {
                println!("note: {path} not found ({e}); run work\\emu\\gen_refs_shapes.py. Test skipped.");
                None
            }
        }
    }

    /// The data rows of a TSV (header lines start with '#').
    fn rows(text: &str) -> impl Iterator<Item = &str> {
        text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    }

    /// An f32 from its 8 hex digits of bits.
    fn hf(s: &str) -> f32 {
        f32::from_bits(u32::from_str_radix(s, 16).expect("hex word"))
    }

    fn same(p: Vec3, want: [f32; 3]) -> bool {
        p.x.to_bits() == want[0].to_bits() && p.y.to_bits() == want[1].to_bits() && p.z.to_bits() == want[2].to_bits()
    }

    fn report(name: &str, n: usize, bad: &[String]) {
        println!("{name}: {n} cases, {} differ", bad.len());
        for row in bad.iter().take(3) {
            println!("  differing row: {row}");
        }
        assert!(bad.is_empty(), "{name}: {} of {n} cases differ", bad.len());
    }

    #[test]
    fn support_cylinder() {
        let Some(text) = read_ref("cylinder") else { return };
        let (mut n, mut bad) = (0usize, Vec::new());
        for line in rows(&text) {
            let t: Vec<&str> = line.split_whitespace().collect();
            // columns: r h vx vy vz ox oy oz
            let got = cylinder_support(Vec3::new(hf(t[2]), hf(t[3]), hf(t[4])), hf(t[0]), hf(t[1]));
            n += 1;
            if !same(got, [hf(t[5]), hf(t[6]), hf(t[7])]) {
                bad.push(line.to_string());
            }
        }
        report("cylinder_support 005b0180", n, &bad);
    }

    #[test]
    fn support_cone() {
        let Some(text) = read_ref("cone") else { return };
        let (mut n, mut bad) = (0usize, Vec::new());
        for line in rows(&text) {
            let t: Vec<&str> = line.split_whitespace().collect();
            // columns: r h a c vx vy vz ox oy oz
            let got = cone_support(Vec3::new(hf(t[4]), hf(t[5]), hf(t[6])), hf(t[0]), hf(t[1]), hf(t[2]), hf(t[3]));
            n += 1;
            if !same(got, [hf(t[7]), hf(t[8]), hf(t[9])]) {
                bad.push(line.to_string());
            }
        }
        report("cone_support 005b0060", n, &bad);
    }

    #[test]
    fn support_pencil() {
        let Some(text) = read_ref("pencil") else { return };
        let (mut n, mut bad) = (0usize, Vec::new());
        for line in rows(&text) {
            let t: Vec<&str> = line.split_whitespace().collect();
            // columns: r h a c vx vy vz ox oy oz
            let got = pencil_support(Vec3::new(hf(t[4]), hf(t[5]), hf(t[6])), hf(t[0]), hf(t[1]), hf(t[2]), hf(t[3]));
            n += 1;
            if !same(got, [hf(t[7]), hf(t[8]), hf(t[9])]) {
                bad.push(line.to_string());
            }
        }
        report("pencil_support 005aff10", n, &bad);
    }

    #[test]
    fn support_capsule() {
        let Some(text) = read_ref("capsule") else { return };
        let (mut n, mut bad) = (0usize, Vec::new());
        for line in rows(&text) {
            let t: Vec<&str> = line.split_whitespace().collect();
            // columns: B0..B8 (9 words), vx vy vz, ox oy oz
            let mut b = [0f32; 9];
            for (k, slot) in b.iter_mut().enumerate() {
                *slot = hf(t[k]);
            }
            let got = capsule_support(Vec3::new(hf(t[9]), hf(t[10]), hf(t[11])), &b);
            n += 1;
            if !same(got, [hf(t[12]), hf(t[13]), hf(t[14])]) {
                bad.push(line.to_string());
            }
        }
        report("capsule_support 005e3400", n, &bad);
    }

    fn check_hull(width: u32, file: &str) {
        let Some(text) = read_ref(file) else { return };
        let (mut n, mut bad) = (0usize, Vec::new());
        // (steps, pairs) of the longest walk
        let mut most = (0usize, 0usize);
        for line in rows(&text) {
            // columns: nv, nv x (x y z w), np, np x (vertex index, link), cache in, vx vy vz,
            // ox oy oz, cache out
            let t: Vec<&str> = line.split_whitespace().collect();
            let mut k = 0usize;
            let nv: usize = t[k].parse().expect("nv");
            k += 1;
            let mut vertices = Vec::with_capacity(nv);
            for _ in 0..nv {
                vertices.push([hf(t[k]), hf(t[k + 1]), hf(t[k + 2]), hf(t[k + 3])]);
                k += 4;
            }
            let np: usize = t[k].parse().expect("np");
            k += 1;
            let mut pairs = Vec::with_capacity(np);
            for _ in 0..np {
                pairs.push((t[k].parse::<u32>().expect("index"), t[k + 1].parse::<i32>().expect("link")));
                k += 2;
            }
            let start: u32 = t[k].parse().expect("cache in");
            let v = Vec3::new(hf(t[k + 1]), hf(t[k + 2]), hf(t[k + 3]));
            let want = [hf(t[k + 4]), hf(t[k + 5]), hf(t[k + 6])];
            let want_cache: u32 = t[k + 7].parse().expect("cache out");
            let block = HullBlock { vertices, pairs };
            let (got, got_cache, steps) = hull_walk(&block, v, start);
            most = most.max((steps, np));
            n += 1;
            if !same(got, want) || got_cache != want_cache {
                bad.push(line.to_string());
            }
        }
        eprintln!("hull_support, Hull{width}: most walk steps {} with {} pairs (bound {HULL_MAX_STEPS})", most.0, most.1);
        report(&format!("hull_support, Hull{width} ({})", if width == 8 { "005afcd0" } else if width == 16 { "005afbb0" } else { "005afa90" }), n, &bad);
    }

    #[test]
    fn support_hull8() {
        check_hull(8, "hull8");
    }

    #[test]
    fn support_hull16() {
        check_hull(16, "hull16");
    }

    #[test]
    fn support_hull32() {
        check_hull(32, "hull32");
    }

    /// The cone's c (P+0xc, `angle` of shanghai_01 zone.str#715) is the cosine of the half angle:
    /// r / sqrt(r^2 + (height14 + height34)^2), to the packet's six printed digits. [data] values from
    /// the same chunk; the first packet's cosine is 0.9553342 and its stored angle 0.955334.
    #[test]
    fn cone_angle_is_cosine_of_half_angle() {
        for (r, h14, h34, angle) in [
            (5.92142f32, 0.457942f32, 1.37383f32, 0.955334f32),
            (4.86402, 0.389253, 1.16776, 0.952394),
        ] {
            let s = h14 + h34;
            let cos = r / (r * r + s * s).sqrt();
            assert!((cos - angle).abs() < 1e-4, "cos {cos} vs angle {angle}");
        }
    }
}
