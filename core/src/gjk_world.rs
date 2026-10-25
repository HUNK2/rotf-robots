//! The collision shapes placed in the world (a shape plus its transform) as `gjk::Support`, so
//! `gjk::ray_cast` can be run against them.
//!
//! Frame rule [game]: read in the swept-sphere wrappers (`docs/collision-support-swept.md`,
//! `work\kernels\support_swept.py`), which call the same helpers the ray tests call:
//!   w   = v * Y               (row vector; the same three sums as 005e3570; `direction_to_local`)
//!   p   = local_support(w)    (the shape's own support helper, in the shape's frame)
//!   out = p * X + t           (row vector; `point_to_world`, the order of 005e32a0 005e3350-005e33d7)
//! The cube's local support is read inline in 005e2250: component k = +h_k if 0 <= w_k, else
//! (-0.0) - h_k. This is the order of the swept and pair HELPERS (`world_support`). The RAY
//! tests inline their own (`ray_world_support`) and are compiled with other loop orders too
//! (`gjk::INLINE`): `ray_test` runs them, bit-exact on lambda over every ordinary hit of
//! `work\emu\ref\ray_shape_*.tsv` (2026-10-17, `docs/physics.md` 1.4).

use glam::Vec3;

use crate::gjk::{self, Support};
use crate::gjk_shapes::{capsule_support, cone_support, cylinder_support, pencil_support};

/// A shape's support point in its own frame, for a direction already in that frame.
pub trait LocalSupport {
    fn local_support(&self, v: Vec3) -> Vec3;

    /// The support in the world through Y (direction in) and X (point out). By default the
    /// frame rule of the module note; a shape whose ray test inlines its own arithmetic
    /// overrides it.
    fn world_support(&self, v: Vec3, x: &[f32; 12], y: &[f32; 9]) -> Vec3 {
        point_to_world(self.local_support(direction_to_local(v, y)), x)
    }

    /// The support as the shape's RAY test (table 00b8dd0c) inlines it. [game] The capsule,
    /// cylinder, cone and pencil tests (005c5020, 005c6a50, 005c8480, 005c9eb0) end every row of
    /// both sums on x: w_k = (Y[3+k] vy + Y[6+k] vz) + Y[k] vx and o_k = ((X[3+k] py + X[6+k] pz) +
    /// X[k] px) + X[9+k] (disassembly 005c6dc9-005c6e83 and 005c6e91 on for the cylinder; the
    /// decompile drops the direction rows). With `gjk::INLINE` this makes all four ray files
    /// bit-exact on lambda (`ray_test`).
    fn ray_world_support(&self, v: Vec3, x: &[f32; 12], y: &[f32; 9]) -> Vec3 {
        let w = Vec3::from_array(std::array::from_fn(|k| (y[3 + k] * v.y + y[6 + k] * v.z) + y[k] * v.x));
        let p = self.local_support(w);
        Vec3::from_array(std::array::from_fn(|k| ((x[3 + k] * p.y + x[6 + k] * p.z) + x[k] * p.x) + x[9 + k]))
    }
}

/// A placed shape as its ray test sees it (`LocalSupport::ray_world_support`).
pub struct RayView<'a, S>(pub &'a Placed<S>);

impl<S: LocalSupport> Support for RayView<'_, S> {
    fn support(&self, v: Vec3) -> Vec3 {
        self.0.shape.ray_world_support(v, &self.0.x, &self.0.y)
    }
}

/// [game] the ray test of a cube, capsule, cylinder, cone, pencil or hull (table 00b8dd0c): the
/// GJK ray cast with the add orders these copies are compiled with (`gjk::INLINE`, the same as
/// the inline pair primitives), seeded at the shape's origin; each shape's frame arithmetic is its
/// `ray_world_support` (the hulls': `collision_world::HullLocal`). Not for the sphere and the
/// triangle (005bed70's family, `gjk::ray_cast`). Every ordinary hit of the seven ray files is
/// bit-exact on lambda (tests here and in `collision_world`).
pub fn ray_test<S: LocalSupport>(p0: Vec3, p1: Vec3, s: &Placed<S>) -> Option<gjk::RayHit> {
    gjk::ray_cast_with(p0, p1, &RayView(s), Vec3::new(s.x[9], s.x[10], s.x[11]), &gjk::INLINE)
}

/// A box centred on its frame origin, half extents `half`.
pub struct Cuboid {
    pub half: Vec3,
}

/// [game] 005e2250 (inline corner): component k = +h_k when 0 <= w_k, else (-0.0) - h_k.
fn sign_pick(w: f32, h: f32) -> f32 {
    if 0.0 <= w {
        h
    } else {
        -0.0f32 - h
    }
}

impl LocalSupport for Cuboid {
    fn local_support(&self, w: Vec3) -> Vec3 {
        Vec3::new(sign_pick(w.x, self.half.x), sign_pick(w.y, self.half.y), sign_pick(w.z, self.half.z))
    }

    /// [game] the cube's ray test 005c3670 inlines its support with its own add orders
    /// (005c3a3a-005c3b40, the transform copied to the stack at 005c3880-005c3920 first):
    /// w = ((Y3 vy + Y6 vz) + Y0 vx, (Y4 vy + Y7 vz) + Y1 vx, (Y5 vy + Y8 vz) + Y2 vx), and the
    /// point ((X0 px + X3 py) + X6 pz) + X9, ((X1 px + X4 py) + X7 pz) + X10,
    /// ((X2 px + X5 py) + X8 pz) + X11.
    fn world_support(&self, v: Vec3, x: &[f32; 12], y: &[f32; 9]) -> Vec3 {
        let w = Vec3::new(
            (y[3] * v.y + y[6] * v.z) + y[0] * v.x,
            (y[4] * v.y + y[7] * v.z) + y[1] * v.x,
            (y[5] * v.y + y[8] * v.z) + y[2] * v.x,
        );
        let p = self.local_support(w);
        Vec3::new(
            ((x[0] * p.x + x[3] * p.y) + x[6] * p.z) + x[9],
            ((x[1] * p.x + x[4] * p.y) + x[7] * p.z) + x[10],
            ((x[2] * p.x + x[5] * p.y) + x[8] * p.z) + x[11],
        )
    }

    /// [game] 005c3670 is `world_support` above (its rows end on x, its point on z).
    fn ray_world_support(&self, v: Vec3, x: &[f32; 12], y: &[f32; 9]) -> Vec3 {
        self.world_support(v, x, y)
    }
}

impl LocalSupport for crate::gjk_shapes::Cylinder {
    fn local_support(&self, v: Vec3) -> Vec3 {
        cylinder_support(v, self.r, self.h)
    }
}

impl LocalSupport for crate::gjk_shapes::Cone {
    fn local_support(&self, v: Vec3) -> Vec3 {
        cone_support(v, self.r, self.h, self.a, self.c)
    }
}

impl LocalSupport for crate::gjk_shapes::Pencil {
    fn local_support(&self, v: Vec3) -> Vec3 {
        pencil_support(v, self.r, self.h, self.a, self.c)
    }
}

impl LocalSupport for crate::gjk_shapes::Capsule {
    fn local_support(&self, v: Vec3) -> Vec3 {
        capsule_support(v, &self.b)
    }
}

/// A shape placed by X (3x4: the three column images then the origin, `x[9..12]`) and Y (its
/// inverse's 3x3, row-major).
pub struct Placed<S> {
    pub shape: S,
    pub x: [f32; 12],
    pub y: [f32; 9],
}

/// [game] w = v * A, row vector, in the order of 005e3570 / 005e32a0 (`docs/collision-support-swept.md`).
fn direction_to_local(v: Vec3, a: &[f32; 9]) -> Vec3 {
    let (vx, vy, vz) = (v.x, v.y, v.z);
    let w0 = (a[3] * vy + a[6] * vz) + a[0] * vx;
    let w1 = (a[1] * vx + a[4] * vy) + a[7] * vz;
    let w2 = (a[2] * vx + a[5] * vy) + a[8] * vz;
    Vec3::new(w0, w1, w2)
}

/// [game] out = p * M + t, row vector, order of 005e32a0 005e3350-005e33d7 (t = M[9..12]).
fn point_to_world(p: Vec3, m: &[f32; 12]) -> Vec3 {
    let (px, py, pz) = (p.x, p.y, p.z);
    let o0 = ((m[6] * pz + m[3] * py) + m[0] * px) + m[9];
    let o1 = ((m[7] * pz + m[1] * px) + m[4] * py) + m[10];
    let o2 = ((m[8] * pz + m[2] * px) + m[5] * py) + m[11];
    Vec3::new(o0, o1, o2)
}

impl<S: LocalSupport> Support for Placed<S> {
    fn support(&self, v: Vec3) -> Vec3 {
        self.shape.world_support(v, &self.x, &self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gjk;

    const REF_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../work/emu/ref/");

    fn hf(s: &str) -> f32 {
        f32::from_bits(u32::from_str_radix(s, 16).expect("hex word"))
    }

    /// Per-file counts. "Ordinary" hits are hits where neither side reports a start in contact.
    #[derive(Default)]
    struct Tally {
        rows: usize,
        agree: usize,
        both_hit: usize,
        contact_agree: usize,
        ordinary: usize,
        lam_ok: usize,
        point_ok: usize,
        normal_ok: usize,
        max_dl: f32,
        bad_hitmiss: Vec<usize>,
        /// Ordinary hits whose lambda is bit-identical to the original's.
        exact: usize,
        /// Ordinary hits whose contact point is bit-identical to the original's (all three floats).
        point_exact: usize,
        /// Largest |point diff| over ordinary agreed hits, component-wise.
        max_dp: f32,
        /// Hits that start in contact on both sides, and their normals (within 0.02; bit for bit).
        start_contact: usize,
        start_normal_ok: usize,
        start_normal_exact: usize,
        /// (row, ours, reference) of ordinary hits with |lambda diff| > 0.002.
        outliers: Vec<(usize, f32, f32)>,
    }

    /// Runs `ray_test` over one reference file. `k` = shape floats per row; `make` builds
    /// the placed shape from those floats and the row's X and Y.
    fn run<S: LocalSupport>(name: &str, k: usize, make: impl Fn(&[f32], [f32; 12], [f32; 9]) -> Placed<S>) {
        let path = format!("{REF_DIR}ray_shape_{name}.tsv");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                println!("note: {path} not found ({e}); run work\\emu\\gen_refs_rayshapes.py. {name} skipped.");
                return;
            }
        };
        let mut t = Tally::default();
        for (row, line) in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).enumerate() {
            let c: Vec<&str> = line.split('\t').collect();
            assert_eq!(c.len(), 36 + k, "{name} row {row} has {} columns", c.len());
            let v3 = |i: usize| Vec3::new(hf(c[i]), hf(c[i + 1]), hf(c[i + 2]));
            let p0 = v3(0);
            let p1 = v3(3);
            let shape_f: Vec<f32> = (0..k).map(|i| hf(c[6 + i])).collect();
            let mut x = [0.0f32; 12];
            for (i, slot) in x.iter_mut().enumerate() {
                *slot = hf(c[6 + k + i]);
            }
            let mut y = [0.0f32; 9];
            for (i, slot) in y.iter_mut().enumerate() {
                *slot = hf(c[18 + k + i]);
            }
            let ref_hit = c[27 + k] == "1";
            let ref_contact = c[28 + k] == "1";
            let ref_point = v3(29 + k);
            let ref_normal = v3(32 + k);
            let ref_lambda = hf(c[35 + k]);

            let shape = make(&shape_f, x, y);
            let ours = ray_test(p0, p1, &shape);

            t.rows += 1;
            if ours.is_some() == ref_hit {
                t.agree += 1;
            } else {
                t.bad_hitmiss.push(row);
            }
            if let (Some(h), true) = (ours, ref_hit) {
                t.both_hit += 1;
                if h.contact == ref_contact {
                    t.contact_agree += 1;
                }
                if h.contact && ref_contact {
                    // starts in contact: the normal comes from the walk back (`contact_normal`)
                    t.start_contact += 1;
                    let dn = h.normal - ref_normal;
                    if dn.x.abs() <= 0.02 && dn.y.abs() <= 0.02 && dn.z.abs() <= 0.02 {
                        t.start_normal_ok += 1;
                    }
                    if h.normal.to_array().map(f32::to_bits) == ref_normal.to_array().map(f32::to_bits) {
                        t.start_normal_exact += 1;
                    }
                }
                if !h.contact && !ref_contact {
                    t.ordinary += 1;
                    let dl = (h.lambda - ref_lambda).abs();
                    if dl > t.max_dl {
                        t.max_dl = dl;
                    }
                    if h.lambda.to_bits() == ref_lambda.to_bits() {
                        t.exact += 1;
                    }
                    if dl <= 0.002 {
                        t.lam_ok += 1;
                    } else if t.outliers.len() < 8 {
                        t.outliers.push((row, h.lambda, ref_lambda));
                    }
                    let dp = h.point - ref_point;
                    if dp.x.abs() <= 0.003 && dp.y.abs() <= 0.003 && dp.z.abs() <= 0.003 {
                        t.point_ok += 1;
                    }
                    if h.point.to_array().map(f32::to_bits) == ref_point.to_array().map(f32::to_bits) {
                        t.point_exact += 1;
                    }
                    t.max_dp = t.max_dp.max(dp.abs().max_element());
                    let dn = h.normal - ref_normal;
                    if dn.x.abs() <= 0.02 && dn.y.abs() <= 0.02 && dn.z.abs() <= 0.02 {
                        t.normal_ok += 1;
                    }
                }
            }
        }
        let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
        println!("== {name}: {} rows", t.rows);
        println!(
            "  hit/miss agree {} / {} ({:.2}%); first disagreeing rows {:?}",
            t.agree,
            t.rows,
            pct(t.agree, t.rows),
            &t.bad_hitmiss[..t.bad_hitmiss.len().min(8)]
        );
        println!("  both hit {}; contact flag agrees {} / {}", t.both_hit, t.contact_agree, t.both_hit);
        println!(
            "  ordinary agreed hits {}: |lambda diff| <= 0.002 {} ({:.2}%), point <= 0.003 {} ({:.2}%), normal <= 0.02 {} ({:.2}%)",
            t.ordinary,
            t.lam_ok,
            pct(t.lam_ok, t.ordinary),
            t.point_ok,
            pct(t.point_ok, t.ordinary),
            t.normal_ok,
            pct(t.normal_ok, t.ordinary)
        );
        println!("  largest |lambda diff| over ordinary agreed hits: {:e}", t.max_dl);
        println!("  lambda bit-exact {} / {}; outliers (row, ours, reference) {:?}", t.exact, t.ordinary, t.outliers);
        println!("  point bit-exact {} / {}; largest component |point diff| {:e}", t.point_exact, t.ordinary, t.max_dp);
        println!(
            "  start-in-contact hits {}: normal <= 0.02 {}, normal bit-exact {}",
            t.start_contact, t.start_normal_ok, t.start_normal_exact
        );
        assert_eq!(t.agree, t.rows, "{name}: hit/miss disagreements");
        // [game] with the ray tests' own add orders every ordinary hit's lambda is the game's
        assert_eq!(t.exact, t.ordinary, "{name}: lambda not bit-exact on every ordinary hit");
        // [game] the contact point of these shapes is p0 + dir * lambda (no target-side witness:
        // witness_support is the default None, so simplex.witness() falls back), bit for bit
        assert_eq!(t.point_exact, t.ordinary, "{name}: contact point not bit-exact on every ordinary hit");
        // [game] the start-in-contact normal (`contact_normal`'s walk back) is bit for bit too
        assert_eq!(t.start_normal_exact, t.start_contact, "{name}: start-in-contact normal not bit-exact");
    }

    /// A fitting aid, off unless RAY_FIT=<cube|cylinder|cone|pencil|capsule>: which last-term
    /// choices for the direction and point rows, under either loop order set, make the shape's ray
    /// file bit-exact (it found the orders `ray_world_support` now has, which the disassembly
    /// confirms). `cargo test --release -p tf2-core --lib ray_order_fit -- --nocapture`.
    #[test]
    fn ray_order_fit() {
        let Ok(name) = std::env::var("RAY_FIT") else { return };
        let k = match name.as_str() {
            "cube" => 3,
            "cylinder" => 2,
            "cone" | "pencil" => 4,
            "capsule" => 7,
            _ => unreachable!(),
        };
        let text = std::fs::read_to_string(format!("{REF_DIR}ray_shape_{name}.tsv")).unwrap();
        let rows: Vec<Vec<f32>> = text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .map(|l| l.split('\t').map(hf).collect())
            .collect();
        struct O<'a> {
            s: &'a dyn LocalSupport,
            x: [f32; 12],
            y: [f32; 9],
            dir: [usize; 3],
            pt: [usize; 3],
        }
        fn s3(t: [f32; 3], last: usize) -> f32 {
            let [i, j, k] = match last {
                0 => [1, 2, 0],
                1 => [0, 2, 1],
                _ => [0, 1, 2],
            };
            (t[i] + t[j]) + t[k]
        }
        impl Support for O<'_> {
            fn support(&self, v: Vec3) -> Vec3 {
                let (x, y) = (&self.x, &self.y);
                let w = Vec3::from_array(std::array::from_fn(|k| s3([y[k] * v.x, y[3 + k] * v.y, y[6 + k] * v.z], self.dir[k])));
                let q = self.s.local_support(w);
                Vec3::from_array(std::array::from_fn(|k| s3([x[k] * q.x, x[3 + k] * q.y, x[6 + k] * q.z], self.pt[k]) + x[9 + k]))
            }
        }
        let mut best = Vec::new();
        for (oname, ord) in [("PRIMITIVE", gjk::PRIMITIVE), ("INLINE", gjk::INLINE)] {
            for code in 0..729usize {
                let digit = |i: usize| (code / 3usize.pow(i as u32)) % 3;
                let dir = [digit(0), digit(1), digit(2)];
                let pt = [digit(3), digit(4), digit(5)];
                let (mut exact, mut n) = (0, 0);
                for c in &rows {
                    let v3 = |i: usize| Vec3::new(c[i], c[i + 1], c[i + 2]);
                    let s = &c[6..6 + k];
                    let shape: Box<dyn LocalSupport> = match name.as_str() {
                        "cube" => Box::new(Cuboid { half: Vec3::new(s[0], s[1], s[2]) }),
                        "cylinder" => Box::new(crate::gjk_shapes::Cylinder { r: s[0], h: s[1] }),
                        "cone" => Box::new(crate::gjk_shapes::Cone { r: s[0], h: s[1], a: s[2], c: s[3] }),
                        "pencil" => Box::new(crate::gjk_shapes::Pencil { r: s[0], h: s[1], a: s[2], c: s[3] }),
                        _ => {
                            let mut b = [0.0f32; 9];
                            b[..3].copy_from_slice(&s[..3]);
                            b[4..7].copy_from_slice(&s[3..6]);
                            b[8] = s[6];
                            Box::new(crate::gjk_shapes::Capsule { b })
                        }
                    };
                    let x: [f32; 12] = std::array::from_fn(|i| c[6 + k + i]);
                    let y: [f32; 9] = std::array::from_fn(|i| c[18 + k + i]);
                    let o = O { s: shape.as_ref(), x, y, dir, pt };
                    let h = gjk::ray_cast_with(v3(0), v3(3), &o, Vec3::new(x[9], x[10], x[11]), &ord);
                    if let Some(h) = h
                        && !h.contact {
                        n += 1;
                        exact += (h.lambda.to_bits() == c[35 + k].to_bits()) as usize;
                    }
                }
                best.push((exact, n, oname, dir, pt));
            }
        }
        best.sort_by_key(|b| std::cmp::Reverse(b.0));
        for b in &best[..6] {
            println!("{name}: lambda exact {} / {} with {} dir {:?} pt {:?}", b.0, b.1, b.2, b.3, b.4);
        }
    }

    #[test]
    fn gjk_world_cube() {
        run("cube", 3, |s, x, y| Placed {
            shape: Cuboid { half: Vec3::new(s[0], s[1], s[2]) },
            x,
            y,
        });
    }

    #[test]
    fn gjk_world_cylinder() {
        run("cylinder", 2, |s, x, y| Placed {
            shape: crate::gjk_shapes::Cylinder { r: s[0], h: s[1] },
            x,
            y,
        });
    }

    #[test]
    fn gjk_world_cone() {
        run("cone", 4, |s, x, y| Placed {
            shape: crate::gjk_shapes::Cone { r: s[0], h: s[1], a: s[2], c: s[3] },
            x,
            y,
        });
    }

    #[test]
    fn gjk_world_pencil() {
        run("pencil", 4, |s, x, y| Placed {
            shape: crate::gjk_shapes::Pencil { r: s[0], h: s[1], a: s[2], c: s[3] },
            x,
            y,
        });
    }

    #[test]
    fn gjk_world_capsule() {
        // file floats: +0, +4, +8 (end 1), +0x10, +0x14, +0x18 (end 2), +0x20 (radius)
        run("capsule", 7, |s, x, y| {
            let mut b = [0.0f32; 9];
            b[0] = s[0];
            b[1] = s[1];
            b[2] = s[2];
            b[4] = s[3];
            b[5] = s[4];
            b[6] = s[5];
            b[8] = s[6];
            Placed { shape: crate::gjk_shapes::Capsule { b }, x, y }
        });
    }
}
