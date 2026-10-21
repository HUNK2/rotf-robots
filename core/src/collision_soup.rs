//! Ray against a triangle mesh (`lux::CollisionSoup`): the driver FUN_005bcb30 over
//! `gjk::ray_cast` (`notes\collision-ray-soup.md`, with the corrections the porter's
//! ticket lists: the hit verdict, the origin seed, the hit point, the normal rule).
//!
//! [game] addresses are the instruction addresses from the note. The triangle test is
//! `crate::gjk::ray_cast` (a port of 005bed70), not the game's bytes.

use crate::gjk::{self, RayHit, Triangle};
use glam::Vec3;

/// [game] word 0 and word 1 of the block: vertex count A and triangle count B; vertices
/// (4 f32 each) at +0x10, triangles (3 u32 each) at +0x10 + 16*A.
const HEADER: usize = 0x10;
/// [game] 005bd073, 005bd0ad, 005bd0e8: index = word & 0x7ffffff (low 27 bits).
const INDEX_MASK: u32 = 0x7ff_ffff;
/// [game] 00b345d8: normalise only when the cross product's length is above 1e-8.
const EPS_NORM: f32 = 1e-8;

/// A triangle mesh as the block stores it. `vertices` are the block's vec4s (local space);
/// `triangles` are the raw words, not masked (the index mask is applied when they are used).
pub struct Soup {
    pub vertices: Vec<[f32; 4]>,
    pub triangles: Vec<[u32; 3]>,
}

/// One hit record (the driver's result entry, the fields the caller needs).
#[derive(Clone, Copy, Debug)]
pub struct SoupHit {
    /// Triangle index in the block's triangle order (record +0x24).
    pub triangle: u32,
    /// Hit point (record +0x00..+0x08).
    pub point: Vec3,
    /// Oriented face normal (record +0x18..+0x20).
    pub normal: Vec3,
    /// Distance in metres from P0 along the ray (record +0x28).
    pub lambda: f32,
    /// The triangle test's contact flag (AL on return).
    pub contact: bool,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_bits(u32_at(b, at))
}

/// [game] 005bcb55..005bcee5 (4-wide loop, vertices 0 .. 4*(A/4)-1) and 005bcf7e..005bd03a
/// (remainder). The two loops round differently, so the order is chosen by index. The
/// matrix `x` is the 3x4 affine: x[0..3] image of the x axis, x[3..6] of y, x[6..9] of z,
/// x[9..12] origin. w is passed through.
pub(crate) fn transform_vertices(vertices: &[[f32; 4]], x: &[f32; 12]) -> Vec<[f32; 4]> {
    let (p0, p1, p2, p3, p4, p5) = (x[0], x[1], x[2], x[3], x[4], x[5]);
    let (p6, p7, p8, p9, p10, p11) = (x[6], x[7], x[8], x[9], x[10], x[11]);
    let a = vertices.len();
    let n4 = 4 * (a / 4);
    let mut out = Vec::with_capacity(a);
    for (i, v) in vertices.iter().enumerate() {
        let [vx, vy, vz, w] = *v;
        let o = if i < n4 {
            // [game] 005bcc06..005bccb9: ((p0*x + p3*y) + p6*z) + p9*w, and so on
            [
                p0 * vx + p3 * vy + p6 * vz + p9 * w,
                p1 * vx + p4 * vy + p7 * vz + p10 * w,
                p2 * vx + p5 * vy + p8 * vz + p11 * w,
            ]
        } else {
            // [game] 005bcf7e..005bd01a: ((p3*y + p6*z) + p9*w) + p0*x, and so on
            [
                ((p3 * vy + p6 * vz) + p9 * w) + p0 * vx,
                ((p4 * vy + p7 * vz) + p10 * w) + p1 * vx,
                ((p5 * vy + p8 * vz) + p11 * w) + p2 * vx,
            ]
        };
        out.push([o[0], o[1], o[2], w]);
    }
    out
}

impl Soup {
    /// Reads a block (word 0 = A, word 1 = B, vertices, triangles). None if the slice is
    /// shorter than the block says.
    pub fn from_block(bytes: &[u8]) -> Option<Soup> {
        if bytes.len() < HEADER {
            return None;
        }
        let a = u32_at(bytes, 0) as usize;
        let b = u32_at(bytes, 4) as usize;
        let tri_start = HEADER.checked_add(16usize.checked_mul(a)?)?;
        let end = tri_start.checked_add(12usize.checked_mul(b)?)?;
        if bytes.len() < end {
            return None;
        }
        let vertices = (0..a)
            .map(|i| {
                let at = HEADER + 16 * i;
                [f32_at(bytes, at), f32_at(bytes, at + 4), f32_at(bytes, at + 8), f32_at(bytes, at + 12)]
            })
            .collect();
        let triangles = (0..b)
            .map(|t| {
                let at = tri_start + 12 * t;
                [u32_at(bytes, at), u32_at(bytes, at + 4), u32_at(bytes, at + 8)]
            })
            .collect();
        Some(Soup { vertices, triangles })
    }

    /// The driver FUN_005bcb30. `x` is the 3x4 transform. Returns every hit, in triangle
    /// order (the game does not choose the nearest).
    pub fn ray_cast(&self, p0: Vec3, p1: Vec3, x: &[f32; 12]) -> Vec<SoupHit> {
        let v = transform_vertices(&self.vertices, x);
        // [game] 005bd254..: the search seed is the transform's origin (correction (b)).
        let origin = Vec3::new(x[9], x[10], x[11]);
        let mut hits = Vec::new();

        for (ti, tri) in self.triangles.iter().enumerate() {
            // [assumed] an index past the vertex list is skipped; the original reads
            // past the array there, and no test has such a triangle.
            let idx = tri.map(|w| (w & INDEX_MASK) as usize);
            if idx.iter().any(|&i| i >= v.len()) {
                continue;
            }
            let a = v[idx[0]];
            let b = v[idx[1]];
            let c = v[idx[2]];

            // [game] 005bd12a..005bd179: edges e1 = b - a, e2 = c - a
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];

            // [game] 005bd185..005bd1b5: N = cross(e1, e2), unnormalised
            let mut n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e2[2] * e1[0],
                e2[1] * e1[0] - e1[1] * e2[0],
            ];
            // [game] 005bd1a8..005bd1cc: len2 = (Nz*Nz + Ny*Ny) + Nx*Nx; ln = sqrt(len2) as f32
            let len2 = (n[2] * n[2] + n[1] * n[1]) + n[0] * n[0];
            let ln = len2.sqrt();
            // [game] 005bd1ed..005bd24e: normalise only if ln > 1e-8
            if ln > EPS_NORM {
                let inv = 1.0f32 / ln;
                n = [inv * n[0], n[1] * inv, n[2] * inv];
            }

            let tri_shape = Triangle {
                a: Vec3::new(a[0], a[1], a[2]),
                b: Vec3::new(b[0], b[1], b[2]),
                c: Vec3::new(c[0], c[1], c[2]),
            };
            // The verdict is the triangle test's (the count went up): a hit iff it returns Some.
            if let Some(RayHit { point, normal: e, lambda, contact }) = gjk::ray_cast(p0, p1, &tri_shape, origin) {
                // [game] 005bd2a0..005bd2fd: dot = (E.z*N.z + E.y*N.y) + N.x*E.x; keep N if dot > 0,
                // else store -0.0 - N (step 9). E is the triangle test's vector.
                let dot = (e.z * n[2] + e.y * n[1]) + n[0] * e.x;
                let oriented = if dot > 0.0 {
                    Vec3::new(n[0], n[1], n[2])
                } else {
                    Vec3::new(-0.0 - n[0], -0.0 - n[1], -0.0 - n[2])
                };
                hits.push(SoupHit {
                    triangle: ti as u32,
                    point,
                    normal: oriented,
                    lambda,
                    contact,
                });
            }
        }
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_f32(out: &mut Vec<u8>, v: f32) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    /// Block: A = 4 corners of a 2x2 square at z = 0, B = 2 triangles along the diagonal 0-2.
    fn square_block() -> Vec<u8> {
        let mut out = Vec::new();
        for w in [4u32, 2, 0, 0] {
            out.extend_from_slice(&w.to_le_bytes());
        }
        for v in [[-1.0f32, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]] {
            for c in v {
                push_f32(&mut out, c);
            }
            push_f32(&mut out, 1.0);
        }
        for t in [[0u32, 1, 2], [0, 2, 3]] {
            for w in t {
                out.extend_from_slice(&w.to_le_bytes());
            }
        }
        out
    }

    const IDENTITY: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

    #[test]
    fn from_block_reads_a_hand_built_square() {
        let soup = Soup::from_block(&square_block()).expect("block reads");
        assert_eq!(soup.vertices.len(), 4);
        assert_eq!(soup.vertices[2], [1.0, 1.0, 0.0, 1.0]);
        assert_eq!(soup.triangles, vec![[0, 1, 2], [0, 2, 3]]);
        // too short for the triangles it claims
        assert!(Soup::from_block(&square_block()[..0x10 + 64 + 20]).is_none());
        assert!(Soup::from_block(&[0u8; 8]).is_none());
    }

    #[test]
    fn ray_straight_down_onto_square_hits_once() {
        let soup = Soup::from_block(&square_block()).unwrap();
        // (0.25, 0.125) lies inside triangle 0 (y < x), not on the diagonal.
        let p0 = Vec3::new(0.25, 0.125, 5.0);
        let p1 = Vec3::new(0.25, 0.125, -5.0);
        let hits = soup.ray_cast(p0, p1, &IDENTITY);
        assert_eq!(hits.len(), 1, "exactly one hit: {hits:?}");
        let h = hits[0];
        assert_eq!(h.triangle, 0);
        assert!((h.lambda - 5.0).abs() < 1e-3, "lambda {} should be the height 5", h.lambda);
        assert!((h.point - Vec3::new(0.25, 0.125, 0.0)).length() < 1e-3, "point {:?}", h.point);
        // Face normal of triangle 0 is (0,0,1) (cross of (2,0,0) and (2,2,0)). The
        // flip rule keeps it when the triangle test's normal agrees, so the sign is
        // (0,0,1) for a ray from above whose test normal points back up.
        println!("straight-down normal {:?} lambda {} point {:?}", h.normal, h.lambda, h.point);
        assert!(h.normal.z.abs() > 1.0 - 1e-4 && h.normal.x.abs() < 1e-4 && h.normal.y.abs() < 1e-4,
            "normal {:?} should be +-(0,0,1)", h.normal);
    }

    #[test]
    fn reference_cases_from_the_emulator() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../work/emu/ref/ray_soup.tsv");
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => {
                println!("reference file absent ({path}): skipped");
                return;
            }
        };
        let hex = |s: &str| f32::from_bits(u32::from_str_radix(s, 16).expect("hex"));
        let dec = |s: &str| s.parse::<i64>().expect("decimal");
        let v3 = |s: &[&str]| Vec3::new(hex(s[0]), hex(s[1]), hex(s[2]));
        fn take<'a>(f: &'a [&'a str], k: &mut usize, n: usize) -> &'a [&'a str] {
            let s = &f[*k..*k + n];
            *k += n;
            s
        }

        let mut cases = 0usize;
        let mut same_list = 0usize;
        let mut compared = 0usize;
        let mut lambda_ok = 0usize;
        let mut point_ok = 0usize;
        let mut normal_ok = 0usize;
        let mut shown = 0usize;
        let mut hist = [0usize; 3];

        for line in text.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
            let f: Vec<&str> = line.split('\t').collect();
            let mut k = 0usize;
            let p0 = v3(take(&f, &mut k, 3));
            let p1 = v3(take(&f, &mut k, 3));
            let xs: Vec<f32> = take(&f, &mut k, 12).iter().map(|s| hex(s)).collect();
            let x: [f32; 12] = xs.try_into().unwrap();
            take(&f, &mut k, 9); // Y, not read by the driver
            let a = dec(take(&f, &mut k, 1)[0]) as usize;
            let b = dec(take(&f, &mut k, 1)[0]) as usize;
            let mut vertices: Vec<[f32; 4]> = Vec::with_capacity(a);
            for _ in 0..a {
                let s = take(&f, &mut k, 4);
                vertices.push([hex(s[0]), hex(s[1]), hex(s[2]), hex(s[3])]);
            }
            let mut triangles: Vec<[u32; 3]> = Vec::with_capacity(b);
            for _ in 0..b {
                let s = take(&f, &mut k, 3);
                triangles.push([dec(s[0]) as u32, dec(s[1]) as u32, dec(s[2]) as u32]);
            }
            let n = dec(take(&f, &mut k, 1)[0]) as usize;
            let mut want: Vec<(u32, Vec3, Vec3, f32)> = Vec::with_capacity(n);
            for _ in 0..n {
                let s = take(&f, &mut k, 8);
                want.push((dec(s[0]) as u32, v3(&s[1..4]), v3(&s[4..7]), hex(s[7])));
            }

            cases += 1;
            let soup = Soup { vertices, triangles };
            let got = soup.ray_cast(p0, p1, &x);
            hist[n.min(2)] += 1;

            let got_ids: Vec<u32> = got.iter().map(|h| h.triangle).collect();
            let want_ids: Vec<u32> = want.iter().map(|w| w.0).collect();
            if got_ids != want_ids {
                if shown < 5 {
                    shown += 1;
                    println!("case {cases}: triangle lists differ: port {got_ids:?} original {want_ids:?}");
                }
                continue;
            }
            same_list += 1;
            for (g, w) in got.iter().zip(want.iter()) {
                compared += 1;
                if (g.lambda - w.3).abs() <= 0.002 {
                    lambda_ok += 1;
                }
                let dp = g.point - w.1;
                if dp.x.abs() <= 0.003 && dp.y.abs() <= 0.003 && dp.z.abs() <= 0.003 {
                    point_ok += 1;
                }
                let dn = g.normal - w.2;
                if dn.x.abs() <= 1e-4 && dn.y.abs() <= 1e-4 && dn.z.abs() <= 1e-4 {
                    normal_ok += 1;
                }
            }
        }

        println!("reference cases: {cases} (hits per case: 0 = {}, 1 = {}, 2+ = {})", hist[0], hist[1], hist[2]);
        println!("identical triangle lists: {same_list} of {cases} ({:.2}%)",
            100.0 * same_list as f64 / cases.max(1) as f64);
        println!("hits compared in identical cases: {compared}");
        println!("  lambda within 0.002: {lambda_ok} ({:.2}%)", 100.0 * lambda_ok as f64 / compared.max(1) as f64);
        println!("  point within 0.003 per component: {point_ok} ({:.2}%)", 100.0 * point_ok as f64 / compared.max(1) as f64);
        println!("  normal within 1e-4 per component: {normal_ok} ({:.2}%)", 100.0 * normal_ok as f64 / compared.max(1) as f64);

        assert!(same_list as f64 >= 0.97 * cases as f64, "identical triangle lists {same_list}/{cases}");
        assert!(lambda_ok as f64 >= 0.93 * compared as f64, "lambda within 0.002: {lambda_ok}/{compared}");
    }
}
