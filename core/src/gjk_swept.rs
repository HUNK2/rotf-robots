//! The sphere sweep: a sphere of radius r moved along a segment, against a convex shape.
//!
//! [game] The sphere tests (table 00b8dd38, `notes\collision-tests-emulated.md`) run the same GJK
//! ray cast as the ray tests, with a support that is the shape's support plus the moving sphere's
//! support: out_sum = shape + buf1, buf1 = r * v / |v| (005e2dd0, `notes\collision-support-swept.md`,
//! `work\kernels\support_swept.py`). So the GJK search runs against the shape grown by r (a
//! Minkowski sum), and the centre's hit is the first lambda at which the grown shape is touched.
//!
//! What the references show beyond that (`work\emu\ref\sphere_shape_*.tsv`,
//! `work\emu\gen_refs_sphereshapes.py`, `notes\collision-sphere-sweep.md`):
//! - [game] The hit lambda and the hit/miss verdict are the grown shape's ray cast's.
//! - [game] The reported POINT is the GJK witness: the target's own support points weighted by
//!   the final simplex's determinants (`notes\physics.md` 1.3). With the game's Johnson step in
//!   `gjk.rs` (2026-10-16) it agrees with all five reference files to 0.1 mm, start in contact
//!   included (it was `centre - r * normal` before, 5 to 30 cm off).
//! - [game] The normal is the search direction at the last advance, or the start-in-contact rule
//!   of `gjk::contact_normal`; both agree with the references to 0.03 degrees.

use glam::Vec3;

use crate::gjk::{self, RayHit, Support};

/// [game] 00b34610 (0x358637bd): the moving sphere's support is zero when |v|^2 is at or below this.
const EPS_SWEEP_L2: f32 = 1e-6;

/// [game] 005e2dd0: the moving sphere's support in direction v, `r * v / |v|`, with the same float
/// steps as the game: n2 = (vx*vx + vy*vy) + vz*vz; s = sqrt(n2) as float; k = 1/s as float (taken
/// in 53-bit precision); keff = r * k; out = v * keff. Zero when n2 <= 1e-6 (the game's branch
/// 005e2dfe; keff is then r * 0). [game]
pub fn sphere_point(v: Vec3, radius: f32) -> Vec3 {
    let n2 = (v.x * v.x + v.y * v.y) + v.z * v.z;
    let k: f32 = if n2 <= EPS_SWEEP_L2 {
        0.0
    } else {
        let s = (n2 as f64).sqrt() as f32;
        (1.0f64 / (s as f64)) as f32
    };
    let keff = radius * k;
    Vec3::new(v.x * keff, v.y * keff, v.z * keff)
}

/// A shape grown by a sphere: its support is the shape's support plus the sphere's (the game's
/// out_sum, as the sphere tests' GJK reads it). `shape` is any support in world space.
pub struct Swept<'a, S> {
    pub shape: &'a S,
    pub radius: f32,
}

impl<'a, S: Support> Support for Swept<'a, S> {
    /// [game] out_sum = shape + buf1 (005e21e7-005e2237 style): the shape's point, then the sphere's.
    fn support(&self, v: Vec3) -> Vec3 {
        self.shape.support(v) + sphere_point(v, self.radius)
    }

    /// [game] the witness kept per simplex slot is the target's own support point (the pair
    /// primitive's q, `notes\physics.md` 1.3); the reported point is their weighted sum.
    fn witness_support(&self, v: Vec3) -> Option<Vec3> {
        Some(self.shape.support(v))
    }
}

/// The sphere of `radius` swept from p0 to p1 (the centre's path) against `shape`.
/// `origin` seeds the first search direction, as for `gjk::ray_cast` (the shape's origin).
///
/// Returns the first contact: `lambda` is the centre's distance from p0 along the normalised
/// segment; `point` is the GJK witness on the target (see the module note); `normal` is the GJK
/// search direction at the last advance, normalised; `contact` is the start-in-contact flag. None
/// for a miss.
pub fn sphere_cast<S: Support>(p0: Vec3, p1: Vec3, radius: f32, shape: &S, origin: Vec3) -> Option<RayHit> {
    let swept = Swept { shape, radius };
    gjk::ray_cast(p0, p1, &swept, origin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gjk::Sphere;
    use crate::gjk_shapes::{Capsule, Cone, Cylinder};
    use crate::gjk_world::{Cuboid, Placed};

    const REF_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../work/emu/ref/");

    fn hf(s: &str) -> f32 {
        f32::from_bits(u32::from_str_radix(s, 16).expect("hex word"))
    }

    fn angle_deg(a: Vec3, b: Vec3) -> f32 {
        let (la, lb) = (a.length(), b.length());
        if la == 0.0 || lb == 0.0 {
            return f32::NAN;
        }
        (a.dot(b) / (la * lb)).clamp(-1.0, 1.0).acos().to_degrees()
    }

    #[derive(Default)]
    struct Tally {
        rows: usize,
        agree: usize,
        both_hit: usize,
        contact_agree: usize,
        ordinary: usize,
        lam_ok: usize,
        max_dl: f32,
        angle_sum: f64,
        angle_n: usize,
        angle_max: f32,
        dp_max_all: f32,
        dp_sum_ord: f64,
        dp_max_ord: f32,
        contact_both: usize,
        dp_max_contact: f32,
        contact_angle_sum: f64,
        contact_angle_n: usize,
        contact_angle_max: f32,
        bad_hitmiss: Vec<usize>,
    }

    /// Runs `sphere_cast` over one reference file. `k` = shape floats per row; `make` builds the
    /// shape from those floats and the row's X and Y.
    fn run<S: Support>(name: &str, k: usize, make: impl Fn(&[f32], [f32; 12], [f32; 9]) -> S) {
        let path = format!("{REF_DIR}sphere_shape_{name}.tsv");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                println!("note: {path} not found ({e}); run work\\emu\\gen_refs_sphereshapes.py. {name} skipped.");
                return;
            }
        };
        let mut t = Tally::default();
        for (row, line) in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).enumerate() {
            let c: Vec<&str> = line.split('\t').collect();
            assert_eq!(c.len(), 37 + k, "{name} row {row} has {} columns", c.len());
            let v3 = |i: usize| Vec3::new(hf(c[i]), hf(c[i + 1]), hf(c[i + 2]));
            let p0 = v3(0);
            let p1 = v3(3);
            let radius = hf(c[6]);
            let shape_f: Vec<f32> = (0..k).map(|i| hf(c[7 + i])).collect();
            let mut x = [0.0f32; 12];
            for (i, slot) in x.iter_mut().enumerate() {
                *slot = hf(c[7 + k + i]);
            }
            let mut y = [0.0f32; 9];
            for (i, slot) in y.iter_mut().enumerate() {
                *slot = hf(c[19 + k + i]);
            }
            let ref_hit = c[28 + k] == "1";
            let ref_contact = c[29 + k] == "1";
            let ref_point = v3(30 + k);
            let ref_normal = v3(33 + k);
            let ref_lambda = hf(c[36 + k]);

            let shape = make(&shape_f, x, y);
            let origin = Vec3::new(x[9], x[10], x[11]);
            let ours = sphere_cast(p0, p1, radius, &shape, origin);

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
                let dp = (h.point - ref_point).length();
                t.dp_max_all = t.dp_max_all.max(dp);
                if h.contact && ref_contact {
                    t.contact_both += 1;
                    t.dp_max_contact = t.dp_max_contact.max(dp);
                    let a = angle_deg(h.normal, ref_normal);
                    if a.is_finite() {
                        t.contact_angle_sum += a as f64;
                        t.contact_angle_n += 1;
                        t.contact_angle_max = t.contact_angle_max.max(a);
                    }
                }
                if !h.contact && !ref_contact {
                    t.ordinary += 1;
                    let dl = (h.lambda - ref_lambda).abs();
                    t.max_dl = t.max_dl.max(dl);
                    if dl <= 0.002 {
                        t.lam_ok += 1;
                    }
                    let a = angle_deg(h.normal, ref_normal);
                    if a.is_finite() {
                        t.angle_sum += a as f64;
                        t.angle_n += 1;
                        t.angle_max = t.angle_max.max(a);
                    }
                    t.dp_sum_ord += dp as f64;
                    t.dp_max_ord = t.dp_max_ord.max(dp);
                }
            }
        }
        let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
        println!("== sphere {name}: {} rows", t.rows);
        println!(
            "  hit/miss agree {} / {} ({:.2}%); first disagreeing rows {:?}",
            t.agree,
            t.rows,
            pct(t.agree, t.rows),
            &t.bad_hitmiss[..t.bad_hitmiss.len().min(8)]
        );
        println!("  both hit {}; contact flag agrees {} / {}", t.both_hit, t.contact_agree, t.both_hit);
        println!(
            "  ordinary agreed hits {}: |lambda diff| <= 0.002 {} ({:.2}%); largest |lambda diff| {:e}",
            t.ordinary,
            t.lam_ok,
            pct(t.lam_ok, t.ordinary),
            t.max_dl
        );
        println!(
            "  mean angle between normals (ordinary hits) {:.2} deg, largest {:.2} deg",
            if t.angle_n == 0 { 0.0 } else { t.angle_sum / t.angle_n as f64 },
            t.angle_max
        );
        println!(
            "  largest distance between hit points: all agreed hits {:.4} m; ordinary hits mean {:.4} m, largest {:.4} m; start-in-contact hits ({}) largest {:.4} m",
            t.dp_max_all,
            if t.ordinary == 0 { 0.0 } else { t.dp_sum_ord / t.ordinary as f64 },
            t.dp_max_ord,
            t.contact_both,
            t.dp_max_contact
        );
        println!(
            "  start-in-contact hits: mean angle between normals {:.2} deg, largest {:.2} deg",
            if t.contact_angle_n == 0 { 0.0 } else { t.contact_angle_sum / t.contact_angle_n as f64 },
            t.contact_angle_max
        );
        assert!(
            pct(t.agree, t.rows) >= 99.5,
            "{name}: hit/miss agreement {}/{} ({:.2}%) is below 99.5%",
            t.agree,
            t.rows,
            pct(t.agree, t.rows)
        );
        assert!(
            pct(t.lam_ok, t.ordinary) >= 95.0,
            "{name}: lambda within 0.002 for {}/{} ordinary hits ({:.2}%) is below 95%",
            t.lam_ok,
            t.ordinary,
            pct(t.lam_ok, t.ordinary)
        );
    }

    #[test]
    fn sphere_swept_sphere() {
        run("sphere", 1, |s, x, y| Sphere { radius: s[0], x, y });
    }

    #[test]
    fn sphere_swept_cube() {
        run("cube", 3, |s, x, y| Placed {
            shape: Cuboid { half: Vec3::new(s[0], s[1], s[2]) },
            x,
            y,
        });
    }

    #[test]
    fn sphere_swept_cylinder() {
        run("cylinder", 2, |s, x, y| Placed {
            shape: Cylinder { r: s[0], h: s[1] },
            x,
            y,
        });
    }

    #[test]
    fn sphere_swept_cone() {
        run("cone", 4, |s, x, y| Placed {
            shape: Cone { r: s[0], h: s[1], a: s[2], c: s[3] },
            x,
            y,
        });
    }

    #[test]
    fn sphere_swept_capsule() {
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
            Placed { shape: Capsule { b }, x, y }
        });
    }

    /// Identity cube h = (1.3, 0.7, 2.1), sphere 0.5, sweep down from z = 5: the contact is on the top
    /// face (z = 2.1), the centre stops at z = 2.6 (lambda 2.4) when exact.
    #[test]
    fn sphere_cube_hand_case() {
        let ident = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let x = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
        let cube = Placed { shape: Cuboid { half: Vec3::new(1.3, 0.7, 2.1) }, x, y: ident };
        let h = sphere_cast(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -5.0), 0.5, &cube, Vec3::ZERO)
            .expect("the sphere must hit the cube");
        println!("hand case: lambda {:.6} (exact 2.4), point {:?}, normal {:?}", h.lambda, h.point, h.normal);
        assert!((h.lambda - 2.4).abs() < 0.01, "lambda {}", h.lambda);
        assert!((h.point.z - 2.1).abs() < 0.01, "point {:?}", h.point);
        assert!((h.normal.z - 1.0).abs() < 1e-3, "normal {:?}", h.normal);
        // a sphere swept over the cube's edge region misses when it stays well above
        assert!(sphere_cast(Vec3::new(0.0, 0.0, 9.0), Vec3::new(0.0, 0.0, 4.0), 0.5, &cube, Vec3::ZERO).is_none());
    }

    #[test]
    fn sphere_point_zero_and_unit() {
        // r * v / |v| for a non-zero v, and zero for |v|^2 <= 1e-6
        let p = sphere_point(Vec3::new(0.0, 3.0, 4.0), 2.0);
        assert!((p - Vec3::new(0.0, 1.2, 1.6)).length() < 1e-6, "{p:?}");
        assert_eq!(sphere_point(Vec3::new(1e-4, 0.0, 0.0), 2.0), Vec3::ZERO);
    }
}
