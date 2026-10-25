//! The pair matrix (00b8db28, `docs/physics.md` 1): the swept test of two moving shapes.
//!
//! [game] A cell is a W wrapper (005e5030 for sphere x sphere, the others differ only in the call
//! set-up of their primitive) over the pair primitive (00616ed0), which is the same GJK ray cast as
//! the ray tests (`gjk::ray_cast`) run against a PAIR support: the two shapes' supports in the same
//! direction, added, each in its END basis with no translation (the wrapper zeroes it). The cast
//! goes from A0 - B0 (the start origins) along dA - dB. So the moving pair is reduced to a point
//! against the sum of the two shapes. (For a shape that is not symmetric about its origin, the
//! cone, the sum is not the true configuration-space obstacle: that is the game's behaviour.)
//!
//! What is ported (port plan steps 3 and 4): all 36 cells of the six simple shapes (sphere, cube,
//! capsule, cylinder, cone, pencil), the wrapper's whole 0x108 record and the start-in-contact
//! re-cast (005be420) through B's ray test (`gjk_world::ray_test`). Two compiled families of the
//! primitive: the helper cells (00616ed0's add orders, `gjk::PRIMITIVE`) and the eleven inline
//! cells (`is_inline_cell`, `gjk::INLINE`). Checked against the emulator's runs of the whole cell
//! (`work\emu\gen_refs_pairs.py`, `work\emu\ref\pair_*.tsv`, 400 random swept cases per cell,
//! 14,400 rows): hit/miss on every row, and every record field bit-exact on every hit, ordinary
//! or in contact.
//!
//! 2026-10-17: the hulls 7-9 (Hull32, Hull16, Hull8) as rows and columns against the six simple
//! shapes and each other, 45 cells (`PairShape::Hull`, `pair_hull_support`): the helper family,
//! the walk's cache words carried in and out (+0xc0 / +0xc4 in, +0xfc / +0x100 out), the B hull's
//! re-cast starting at unit 1. 18,000 rows, the same result: every field of every hit bit-exact.
//! 2026-10-17, later: the soup row (type 0 as B against types 1-9, `soup_cell`): the selector R0,
//! the W2 wrapper (`pair_cell` with `PairShape::Triangle`), the helper cells (sphere, cube, hulls)
//! and the soup's own inline copies (`SoupInline`). 9,000 rows, every field of every hit bit-exact.
//! Not yet: Hull4 (type 10, its support 005afdf0 is unread).

use glam::Vec3;

use crate::gjk::{self, Support};
use std::cell::Cell;

use crate::gjk_shapes::{hull_support, Capsule, Cone, Cylinder, HullBlock, Pencil};
use crate::gjk_world::{ray_test, Cuboid, LocalSupport, Placed};

/// [game] 005301d0 `inverse_affine3`: the inverse of a packed 3x4 affine (9 basis floats row by
/// row, then the origin), in single precision as written. When |det| <= 1e-8 the adjugate is
/// returned unscaled (the game's guard), translation included.
pub fn inverse_affine3(m: &[f32; 12]) -> [f32; 12] {
    let c0 = m[8] * m[4] - m[7] * m[5];
    let c6 = m[3] * m[7] - m[6] * m[4];
    let c3 = m[6] * m[5] - m[3] * m[8];
    let c1 = m[2] * m[7] - m[1] * m[8];
    let c4 = m[0] * m[8] - m[6] * m[2];
    let c7 = m[6] * m[1] - m[0] * m[7];
    let c2 = m[5] * m[1] - m[4] * m[2];
    let c5 = m[3] * m[2] - m[5] * m[0];
    let c8 = m[4] * m[0] - m[3] * m[1];
    let det = (m[0] * c0 + m[1] * c3) + m[2] * c6;
    let (tx, ty, tz) = (-0.0f32 - m[9], -0.0f32 - m[10], -0.0f32 - m[11]);
    let t0 = (tz * c6 + ty * c3) + tx * c0;
    let t1 = (tz * c7 + ty * c4) + tx * c1;
    let t2 = (tz * c8 + ty * c5) + tx * c2;
    if (-1e-8..=1e-8).contains(&det) {
        return [c0, c1, c2, c3, c4, c5, c6, c7, c8, t0, t1, t2];
    }
    let r = 1.0f32 / det;
    [r * c0, c1 * r, c2 * r, r * c3, c4 * r, c5 * r, r * c6, c7 * r, c8 * r, t0 * r, t1 * r, t2 * r]
}

/// A shape of the pair matrix, with the data its cell reads (the collision packet's floats).
#[derive(Clone, Copy)]
pub enum PairShape<'a> {
    /// Type 1: the radius.
    Sphere { radius: f32 },
    /// Type 2: the half extents.
    Cube { half: Vec3 },
    /// Type 3: the capsule record's nine floats (end 1 at +0x0, end 2 at +0x10, radius at +0x20).
    Capsule { b: [f32; 9] },
    /// Type 4: radius and half height (axis z).
    Cylinder { r: f32, h: f32 },
    /// Type 5: radius, half height, tip height, cosine of the slant (`gjk_shapes::cone_support`).
    Cone { r: f32, h: f32, a: f32, c: f32 },
    /// Type 6: as the cone, the pencil's (`gjk_shapes::pencil_support`).
    Pencil { r: f32, h: f32, a: f32, c: f32 },
    /// Types 7, 8, 9: Hull32, Hull16, Hull8 (`width` 32, 16, 8), the walk's block
    /// (`collision_world::hull_block`). [game] The shape data is a pointer to the block pointer;
    /// the walk's start unit (cache word) is the input record's +0xc0 for A, +0xc4 for B
    /// (`PairInput::tag`). Hull4 (type 10) is not here: its support 005afdf0 is unread.
    Hull { width: u8, block: &'a HullBlock },
    /// Type 0, one triangle of a soup as the soup row's W2 wrappers take it (`soup_cell`).
    /// [game] its support is 005e3710 on the raw vertices: no basis is applied (006d67d0 passes
    /// the direction straight to it), so the vertices are relative to B's origin in world axes.
    Triangle { v: [Vec3; 3] },
}

/// [game] one 64-byte triangle record of the soup row (005e3f50 and kin): vertices (x, y, z, w)
/// at +0x00 / +0x10 / +0x20, the normal at +0x30, a flag at +0x3c (tried only when 0.0).
#[derive(Clone, Copy, Debug)]
pub struct SoupTri {
    pub v: [Vec3; 3],
    pub normal: Vec3,
    pub flag: f32,
}

impl PairShape<'_> {
    /// The shape type number (`docs/collision-shapes.md`), the cell's row or column.
    pub fn kind(&self) -> u32 {
        match self {
            PairShape::Triangle { .. } => 0,
            PairShape::Sphere { .. } => 1,
            PairShape::Cube { .. } => 2,
            PairShape::Capsule { .. } => 3,
            PairShape::Cylinder { .. } => 4,
            PairShape::Cone { .. } => 5,
            PairShape::Pencil { .. } => 6,
            PairShape::Hull { width: 32, .. } => 7,
            PairShape::Hull { width: 16, .. } => 8,
            PairShape::Hull { width: 8, .. } => 9,
            PairShape::Hull { width, .. } => panic!("hull width {width}: only 32, 16 and 8 are ported (Hull4 is unread)"),
        }
    }
}

/// A hull as the pair cells walk it: the block, and the walk's start unit, carried from one
/// support call to the next within the cast as the game carries it in the primitive's record
/// (+0x34 for A, +0x38 for B; call sites 00623f0c, 0069b3ad, 006a80cf).
struct HullWalk<'a> {
    block: &'a HullBlock,
    cache: Cell<u32>,
}

impl LocalSupport for HullWalk<'_> {
    fn local_support(&self, v: Vec3) -> Vec3 {
        let (p, next) = hull_support(self.block, v, self.cache.get());
        self.cache.set(next);
        p
    }

    /// [game] the hull ray tests' frame arithmetic, as `collision_world::HullLocal::ray_world_support`
    /// (005cec94-005cedea; bit-exact on the hull ray files).
    fn ray_world_support(&self, v: Vec3, x: &[f32; 12], y: &[f32; 9]) -> Vec3 {
        let w = Vec3::new(
            (y[0] * v.x + y[6] * v.z) + y[3] * v.y,
            (y[7] * v.z + y[1] * v.x) + y[4] * v.y,
            (y[2] * v.x + y[5] * v.y) + y[8] * v.z,
        );
        let p = self.local_support(w);
        Vec3::new(
            ((x[6] * p.z + x[3] * p.y) + x[0] * p.x) + x[9],
            ((x[4] * p.y + x[1] * p.x) + x[7] * p.z) + x[10],
            ((x[5] * p.y + x[8] * p.z) + x[2] * p.x) + x[11],
        )
    }
}

/// [game] a hull in the pair helpers (006cc760 and its kin, disassembly): the direction into the
/// hull's frame as the pair cube forms it, w = ((Y3 vy + Y6 vz) + Y0 vx, (Y1 vx + Y4 vy) + Y7 vz,
/// (Y2 vx + Y5 vy) + Y8 vz), the walk, and the point back through X:
/// - by default (A hulls in every helper, B hulls against a sphere, capsule, cylinder, cone or
///   pencil: 006cc760, 006cdc70, 006ce990, 006cf3f0, 006d3a10, 006d3eb0, 006d4070):
///   ((X6 pz + X3 py) + X0 px) + X9, ((X1 px + X7 pz) + X4 py) + X10, ((X2 px + X8 pz) + X5 py) + X11;
/// - `b_rows`, the B hull when A is a cube or a hull (006d3bd0, 006d4b50): every row
///   ((X[6+k] pz + X[3+k] py) + X[k] px) + X[9+k].
fn pair_hull_support(h: &Placed<HullWalk>, b_rows: bool, v: Vec3) -> Vec3 {
    let y = &h.y;
    let w = Vec3::new(
        (y[3] * v.y + y[6] * v.z) + y[0] * v.x,
        (y[1] * v.x + y[4] * v.y) + y[7] * v.z,
        (y[2] * v.x + y[5] * v.y) + y[8] * v.z,
    );
    let p = h.shape.local_support(w);
    let x = &h.x;
    if b_rows {
        return Vec3::from_array(std::array::from_fn(|k| ((x[6 + k] * p.z + x[3 + k] * p.y) + x[k] * p.x) + x[9 + k]));
    }
    Vec3::new(
        ((x[6] * p.z + x[3] * p.y) + x[0] * p.x) + x[9],
        ((x[1] * p.x + x[7] * p.z) + x[4] * p.y) + x[10],
        ((x[2] * p.x + x[8] * p.z) + x[5] * p.y) + x[11],
    )
}

/// One shape in its END basis, no translation (the wrapper's descriptor), as a support.
enum Placement<'a> {
    Sphere(gjk::Sphere),
    Cube(Placed<Cuboid>),
    /// [game] capsule, cylinder, cone, pencil: the transform helpers 005e32a0, 005e3140, 005e2fe0,
    /// 005e2e80 (`gjk_world`'s frame rule, bit-exact in `docs/collision-support-swept.md`).
    Capsule(Placed<Capsule>),
    Cylinder(Placed<Cylinder>),
    Cone(Placed<Cone>),
    Pencil(Placed<Pencil>),
    /// The hull and whether its point rows are the B form (`pair_hull_support`).
    Hull(Placed<HullWalk<'a>>, bool),
    /// A soup triangle (raw vertices) and the frame it was placed with (only the re-cast's seed
    /// reads it: 005bed70 starts from EDX's origin).
    Triangle(gjk::Triangle, [f32; 12], [f32; 9]),
}

/// The shape posed by X and Y as the supports above take it; `cache` is a hull's start unit.
fn place<'a>(shape: PairShape<'a>, x: [f32; 12], y: [f32; 9], cache: u32) -> Placement<'a> {
    match shape {
        PairShape::Triangle { v } => Placement::Triangle(gjk::Triangle { a: v[0], b: v[1], c: v[2] }, x, y),
        PairShape::Hull { block, .. } => {
            Placement::Hull(Placed { shape: HullWalk { block, cache: Cell::new(cache) }, x, y }, false)
        }
        PairShape::Sphere { radius } => Placement::Sphere(gjk::Sphere { radius, x, y }),
        PairShape::Cube { half } => Placement::Cube(Placed { shape: Cuboid { half }, x, y }),
        PairShape::Capsule { b } => Placement::Capsule(Placed { shape: Capsule { b }, x, y }),
        PairShape::Cylinder { r, h } => Placement::Cylinder(Placed { shape: Cylinder { r, h }, x, y }),
        PairShape::Cone { r, h, a, c } => Placement::Cone(Placed { shape: Cone { r, h, a, c }, x, y }),
        PairShape::Pencil { r, h, a, c } => Placement::Pencil(Placed { shape: Pencil { r, h, a, c }, x, y }),
    }
}

/// [game] the cube as the pair helpers inline it (006cbf30, 006ccb00, 006cccd0): w = v * Y as
/// the ray tests form it, the corner by the sign of each w_k (`gjk_world`'s rule), and the
/// point back through X with ITS OWN add order, which differs from the ray tests' frame helper
/// in the second and third rows: ((X1 px + X4 py) + X7 pz) + X10, ((X2 px + X5 py) + X8 pz) + X11.
fn pair_cube_support(c: &Placed<Cuboid>, v: Vec3) -> Vec3 {
    let y = &c.y;
    let w = Vec3::new(
        (y[3] * v.y + y[6] * v.z) + y[0] * v.x,
        (y[1] * v.x + y[4] * v.y) + y[7] * v.z,
        (y[2] * v.x + y[5] * v.y) + y[8] * v.z,
    );
    let pick = |w: f32, h: f32| if 0.0 <= w { h } else { -0.0f32 - h };
    let h = c.shape.half;
    let p = Vec3::new(pick(w.x, h.x), pick(w.y, h.y), pick(w.z, h.z));
    let x = &c.x;
    Vec3::new(
        ((x[3] * p.y + x[6] * p.z) + x[0] * p.x) + x[9],
        ((x[1] * p.x + x[4] * p.y) + x[7] * p.z) + x[10],
        ((x[2] * p.x + x[5] * p.y) + x[8] * p.z) + x[11],
    )
}

impl<'a> Placement<'a> {
    fn new(shape: PairShape<'a>, basis: &[f32; 12], cache: u32) -> Self {
        let mut x = *basis;
        x[9] = 0.0;
        x[10] = 0.0;
        x[11] = 0.0;
        let inv = inverse_affine3(&x);
        let mut y = [0.0f32; 9];
        y.copy_from_slice(&inv[..9]);
        place(shape, x, y, cache)
    }

    /// A hull's walk unit after the cast (the primitive's record +0x34 / +0x38, copied to the
    /// output's +0xfc / +0x100).
    fn cache(&self) -> Option<u32> {
        match self {
            Placement::Hull(h, _) => Some(h.shape.cache.get()),
            _ => None,
        }
    }

    fn support(&self, v: Vec3) -> Vec3 {
        match self {
            Placement::Hull(h, b_rows) => pair_hull_support(h, *b_rows, v),
            Placement::Triangle(t, ..) => t.support(v),
            Placement::Sphere(s) => s.support(v),
            Placement::Cube(c) => pair_cube_support(c, v),
            Placement::Capsule(s) => s.support(v),
            Placement::Cylinder(s) => s.support(v),
            Placement::Cone(s) => s.support(v),
            Placement::Pencil(s) => s.support(v),
        }
    }

    fn frame(&self) -> (&[f32; 12], &[f32; 9]) {
        match self {
            Placement::Sphere(s) => (&s.x, &s.y),
            Placement::Cube(s) => (&s.x, &s.y),
            Placement::Capsule(s) => (&s.x, &s.y),
            Placement::Cylinder(s) => (&s.x, &s.y),
            Placement::Cone(s) => (&s.x, &s.y),
            Placement::Pencil(s) => (&s.x, &s.y),
            Placement::Hull(s, _) => (&s.x, &s.y),
            Placement::Triangle(_, x, y) => (x, y),
        }
    }

    /// The shape's own support helper in its frame (the inline family's shapes only).
    fn local(&self, w: Vec3) -> Vec3 {
        match self {
            Placement::Capsule(s) => s.shape.local_support(w),
            Placement::Cylinder(s) => s.shape.local_support(w),
            Placement::Cone(s) => s.shape.local_support(w),
            Placement::Pencil(s) => s.shape.local_support(w),
            Placement::Cube(s) => LocalSupport::local_support(&s.shape, w),
            Placement::Sphere(_) | Placement::Hull(..) | Placement::Triangle(..) => {
                unreachable!("no inline primitive has a sphere, a hull or a triangle (inline_cell)")
            }
        }
    }

    /// [game] the shape's RAY test (table 00b8dd0c): the sphere's is 005bed70's family (`gjk::ray_cast`),
    /// the others `gjk_world::ray_test` (their own add orders, bit-exact on the ray files).
    fn ray_test(&self, p0: Vec3, p1: Vec3) -> Option<gjk::RayHit> {
        match self {
            Placement::Sphere(s) => gjk::ray_cast(p0, p1, s, origin(&s.x)),
            Placement::Cube(s) => ray_test(p0, p1, s),
            Placement::Capsule(s) => ray_test(p0, p1, s),
            Placement::Cylinder(s) => ray_test(p0, p1, s),
            Placement::Cone(s) => ray_test(p0, p1, s),
            Placement::Pencil(s) => ray_test(p0, p1, s),
            Placement::Hull(s, _) => ray_test(p0, p1, s),
            // [game] 005be420 with type 0 and its flag set calls 005bed70 on the triangle itself
            Placement::Triangle(t, x, _) => gjk::ray_cast(p0, p1, t, origin(x)),
        }
    }
}

/// [game] the cells whose primitive inlines both supports (`docs/physics.md` 1.4): 0063a470
/// [3][3], 0063c3a0 [3][4], 0064c9c0 [4][4], 0064e8d0 [4][5], 006507e0 [4][6], 0065dae0 [5][4],
/// 0065f9f0 [5][5], 00661900 [5][6], 0066ec00 [6][4], 00670b10 [6][5], 00672a20 [6][6]. The other
/// cells call a helper (the capsule as B against a cone or pencil, any capsule A but [3][3]).
/// All eleven sum A's point whole and add it to B's, as the disassembly of 0063c3a0 and 0064c9c0
/// shows; the decompile of the nine without a capsule prints A's terms folded into B's point one
/// by one, which is the decompiler re-associating (that form misses half the reference rows).
fn is_inline_cell(row: u32, col: u32) -> bool {
    matches!((row, col), (3, 3) | (3, 4) | (4, 4) | (4, 5) | (4, 6) | (5, 4) | (5, 5) | (5, 6) | (6, 4) | (6, 5) | (6, 6))
}

/// [game] the direction into the shape's frame in all eleven copies: w_k = (Y[k] vx + Y[3+k] vy)
/// + Y[6+k] vz, except B's second row, (Y1 vx + Y7 vz) + Y4 vy.
fn inline_dir(y: &[f32; 9], v: Vec3, b_side: bool) -> Vec3 {
    let row = |k: usize| (y[k] * v.x + y[3 + k] * v.y) + y[6 + k] * v.z;
    let w1 = if b_side { (y[1] * v.x + y[7] * v.z) + y[4] * v.y } else { row(1) };
    Vec3::new(row(0), w1, row(2))
}

/// [game] a point out of the shape's frame, ((X[k] px + X[3+k] py) + X[6+k] pz) + X[9+k].
fn inline_point(x: &[f32; 12], q: Vec3) -> Vec3 {
    Vec3::from_array(std::array::from_fn(|k| ((x[k] * q.x + x[3 + k] * q.y) + x[6 + k] * q.z) + x[9 + k]))
}

/// [game] the inline pair support: A's support (called first) and B's in the same direction,
/// summed B + A; B's point is the witness (0063c3a0 lines 110-114 and 324-326).
struct InlinePair<'a> {
    a: &'a Placement<'a>,
    b: &'a Placement<'a>,
}

impl InlinePair<'_> {
    fn b_point(&self, v: Vec3) -> Vec3 {
        let (x, y) = self.b.frame();
        inline_point(x, self.b.local(inline_dir(y, v, true)))
    }
}

impl Support for InlinePair<'_> {
    fn support(&self, v: Vec3) -> Vec3 {
        let (ax, ay) = self.a.frame();
        let qa = self.a.local(inline_dir(ay, v, false));
        self.b_point(v) + inline_point(ax, qa)
    }

    fn witness_support(&self, v: Vec3) -> Option<Vec3> {
        Some(self.b_point(v))
    }
}

/// [game] the soup row's inline primitives (006be530 capsule, 006c0200 cylinder, 006c1ec0 cone,
/// 006c3b80 pencil; disassembly of 006be530's loop, 006be8c0-006beb52): A's direction rows
/// (Y[3+k] vy + Y[6+k] vz) + Y[k] vx, its point rows ((X[3+k] py + X[6+k] pz) + X[k] px) + X[9+k],
/// the triangle's support on the raw direction; A + triangle, the triangle's point the witness.
struct SoupInline<'a> {
    a: &'a Placement<'a>,
    tri: &'a gjk::Triangle,
}

impl Support for SoupInline<'_> {
    fn support(&self, v: Vec3) -> Vec3 {
        let (x, y) = self.a.frame();
        let w = Vec3::from_array(std::array::from_fn(|k| (y[3 + k] * v.y + y[6 + k] * v.z) + y[k] * v.x));
        let p = self.a.local(w);
        let pa = Vec3::from_array(std::array::from_fn(|k| ((x[3 + k] * p.y + x[6 + k] * p.z) + x[k] * p.x) + x[9 + k]));
        pa + self.tri.support(v)
    }

    fn witness_support(&self, v: Vec3) -> Option<Vec3> {
        Some(self.tri.support(v))
    }
}

/// [game] 005be420 `contact_recast_nearest`, as the W wrappers call it when the primitive
/// started in contact (005e56fe-005e574a): ECX = the record's world contact c (+0xc0), EDX = the
/// record at +0x60 (B at t, its inverse after it), stack: q = A's start origin - c, B's type, B's
/// data. The segment q + c .. c - q (from A's start origin through the contact) goes through B's
/// RAY test (table 00b8dd0c, the ray helpers' arithmetic), seeded at B's origin; the hit's normal
/// replaces the record's. None when the ray misses (the record keeps the primitive's normal).
/// [game 005be420 / 006d7c20, third pass] the re-cast call sites (006d7c20 lines 121 and 205) pass EDX = record
/// +0x60, which 005be420's own header calls "B at t, inverse after it": the Y argument is B's inverse at t through
/// that pointer. [game 005be420 header] settled 2026-10-20: B at t is the 12-float affine at +0x60, so
/// the inverse after it is at +0x60 + 0x30 = +0x90, the port's read. `reference_pair_cells` checks
/// the contact (re-cast) hits of 90 cells bit for bit with this read.
fn recast_normal(b: PairShape, b_at: &[f32; 12], b_inv: &[f32; 12], c: Vec3, q: Vec3) -> Option<Vec3> {
    recast_segment(b, b_at, b_inv, q + c, c - q).map(|(_, n)| n)
}

/// [game] 005be420's ray against B for the segment `from` .. `to` (the re-cast's own endpoints): the nearest
/// hit's point and normal, B at `b_at` with its inverse `b_inv`. Used by `recast_normal` (the contact's
/// own re-cast) and by the refresh's re-cast against a peer body (006d7c20 lines 121 and 205, EDX = B at t).
/// [game] seeded at B's origin, which is b_at's (every ray test seeds at its X's origin). The query's
/// +0x18 / +0x1c are both 1 (005be420), so a hull's walk starts at unit 1.
pub fn recast_segment(b: PairShape, b_at: &[f32; 12], b_inv: &[f32; 12], from: Vec3, to: Vec3) -> Option<(Vec3, Vec3)> {
    let mut y = [0.0f32; 9];
    y.copy_from_slice(&b_inv[..9]);
    place(b, *b_at, y, 1).ray_test(from, to).map(|h| (h.point, h.normal))
}

/// [game] the pair support helper (006cbe80 and its kin): A's support plus B's, in the same
/// direction; B's point is kept as the witness.
struct PairSupport<'a> {
    a: Placement<'a>,
    b: Placement<'a>,
}

impl Support for PairSupport<'_> {
    fn support(&self, v: Vec3) -> Vec3 {
        self.b.support(v) + self.a.support(v)
    }

    fn witness_support(&self, v: Vec3) -> Option<Vec3> {
        Some(self.b.support(v))
    }
}

/// The cell's input record (0xd0 bytes, 006e1bc0): start and end poses of A and B.
#[derive(Clone, Copy, Debug)]
pub struct PairInput {
    pub a0: [f32; 12],
    pub a1: [f32; 12],
    pub b0: [f32; 12],
    pub b1: [f32; 12],
    /// +0xc0 / +0xc4, carried into the output (+0xfc / +0x100) as the primitive's segment ids.
    pub tag: [u32; 2],
    /// +0xc8, the pair index, carried to +0x104.
    pub index: u32,
}

/// The cell's output record (0x108 bytes) on a hit.
#[derive(Clone, Copy, Debug)]
pub struct PairRecord {
    /// +0x00: A at t (its end basis, origin advanced to t); +0x30 its inverse.
    pub a_at: [f32; 12],
    pub a_inv: [f32; 12],
    /// +0x60: B at t; +0x90 its inverse.
    pub b_at: [f32; 12],
    pub b_inv: [f32; 12],
    /// +0xc0: the contact point in the world.
    pub point: Vec3,
    /// +0xcc: the contact normal.
    pub normal: Vec3,
    /// +0xd8: the contact point in A's frame at t; +0xe4: the witness in B's end basis.
    pub point_in_a: Vec3,
    pub point_in_b: Vec3,
    /// +0xf4: the primitive's lambda (metres along dA - dB); +0xf8: t = lambda / |dA - dB|.
    pub lambda: f32,
    pub t: f32,
    pub tag: [u32; 2],
    pub index: u32,
    /// +0xf0: -1 from the wrappers; the soup row's selector writes the hit triangle's index.
    pub face: u32,
    /// The primitive started in contact; the normal is then the 005be420 re-cast's (`recast_normal`).
    pub contact: bool,
}

fn origin(m: &[f32; 12]) -> Vec3 {
    Vec3::new(m[9], m[10], m[11])
}

/// out = p * M + t, rows as 005e5030 sums them for +0xc0 and +0xd8.
fn apply(m: &[f32; 12], p: Vec3) -> Vec3 {
    Vec3::new(
        ((m[3] * p.y + m[6] * p.z) + p.x * m[0]) + m[9],
        ((m[1] * p.x + m[4] * p.y) + m[7] * p.z) + m[10],
        ((m[2] * p.x + m[5] * p.y) + m[8] * p.z) + m[11],
    )
}

/// One cell of the pair matrix for the simple shapes: None for a miss (the game returns 0).
///
/// `a` is the cell's first argument, posed by the record's A poses; `b` the second. [game] The
/// table cell is [type of b][type of a]: the sphere / cube helpers read the cube from the
/// shape list's first entry in cell [1][2] (006cbf30) and from its second in cell [2][1]
/// (006ccb00). Which object the broad phase puts first is decided by the bin insert 0117ba30
/// (unread).
pub fn pair_cell(a: PairShape, b: PairShape, inp: &PairInput) -> Option<PairRecord> {
    // [game] 005e5030: the displacements and their difference
    let da = origin(&inp.a1) - origin(&inp.a0);
    let db = origin(&inp.b1) - origin(&inp.b0);
    let dd = da - db;
    let mut pair = PairSupport { a: Placement::new(a, &inp.a1, inp.tag[0]), b: Placement::new(b, &inp.b1, inp.tag[1]) };
    // [game] a B hull against a cube or a hull sums its point rows in the other form (006d3bd0, 006d4b50)
    if let (Placement::Hull(_, b_rows), 2 | 7..=9) = (&mut pair.b, a.kind()) {
        *b_rows = true;
    }
    let p0 = origin(&inp.a0) - origin(&inp.b0);
    let p1 = p0 + dd;
    // [game] the seed is B's descriptor centre, which the wrapper zeroed
    let h = if let (Placement::Triangle(tri, ..), 3..=6) = (&pair.b, a.kind()) {
        // [game] the soup row's inline copies: their own support sums, the inline family's orders
        gjk::ray_cast_with(p0, p1, &SoupInline { a: &pair.a, tri }, Vec3::ZERO, &gjk::INLINE)
    } else if is_inline_cell(b.kind(), a.kind()) {
        // [game] the inline copies also sum their loop's dot products in other orders (`gjk::INLINE`)
        gjk::ray_cast_with(p0, p1, &InlinePair { a: &pair.a, b: &pair.b }, Vec3::ZERO, &gjk::INLINE)
    } else {
        gjk::ray_cast(p0, p1, &pair, Vec3::ZERO)
    }?;
    let w = h.point;
    // [game] t = lambda / |dd|. [game, by the references] the root is rounded to float before the
    // divide: dividing by the unrounded root misses t by one ulp in a quarter of the rows.
    let n2 = (dd.z * dd.z + dd.x * dd.x) + dd.y * dd.y;
    let t = h.lambda / ((n2 as f64).sqrt() as f32);
    let mut b_basis = inp.b1;
    b_basis[9] = 0.0;
    b_basis[10] = 0.0;
    b_basis[11] = 0.0;
    let ib = inverse_affine3(&b_basis);
    let point_in_b = Vec3::new(
        ((ib[0] * w.x + ib[6] * w.z) + ib[3] * w.y) + ib[9],
        ((ib[1] * w.x + ib[7] * w.z) + ib[4] * w.y) + ib[10],
        ((ib[2] * w.x + ib[8] * w.z) + ib[5] * w.y) + ib[11],
    );
    let mut a_at = inp.a1;
    let mut b_at = inp.b1;
    let (a0, b0) = (origin(&inp.a0), origin(&inp.b0));
    a_at[9] = a0.x + t * da.x;
    a_at[10] = a0.y + da.y * t;
    a_at[11] = a0.z + da.z * t;
    b_at[9] = t * db.x + b0.x;
    b_at[10] = b0.y + db.y * t;
    b_at[11] = b0.z + db.z * t;
    let point = apply(&b_at, point_in_b);
    let a_inv = inverse_affine3(&a_at);
    let b_inv = inverse_affine3(&b_at);
    let point_in_a = apply(&a_inv, point);
    let normal = if h.contact {
        recast_normal(b, &b_at, &b_inv, point, origin(&inp.a0) - point).unwrap_or(h.normal)
    } else {
        h.normal
    };
    Some(PairRecord {
        a_at,
        a_inv,
        b_at,
        b_inv,
        point,
        normal,
        point_in_a,
        point_in_b,
        lambda: h.lambda,
        t,
        // [game] the record's +0x34 / +0x38: a hull's walk unit after the cast, else the input's
        tag: [pair.a.cache().unwrap_or(inp.tag[0]), pair.b.cache().unwrap_or(inp.tag[1])],
        index: inp.index,
        face: u32::MAX,
        contact: h.contact,
    })
}

/// [game] 005bc430: a soup pair's B data, built from the CollisionSoup block and the soup's matrix
/// `x` (the triangle list the soup row's selector walks; 006e5e00 builds one per pair node with
/// +0x32 set, whose B pose is then the identity). Vertices go to the world in the soup ray
/// driver's two loops (`collision_soup::transform_vertices`: the block's 4-wide and remainder add
/// orders). Per face, indices masked 0x7fffffff (31 bits; the ray driver keeps 27): the three
/// vertices (w passed through), the unit cross(b - a, c - a) (raw when its length is 1e-8 or
/// less), flag 0.0. The count is the block's face count as a 16-bit word.
/// [game 005b4d60, read 2026-10-20] the kernel reads each face's three indices into the transformed vertex array
/// with no bound check (the loop compares only the face count), so an out-of-range index in the game reads the
/// stack; a valid soup has none, and that stack read is not reproduced here. settles: nothing further for valid data.
/// [game 005b8fa0, read 2026-10-20] the soup builder does NOT validate the indices: it copies each face's three
/// words raw from the faces view and never compares them with the vertex count, so an index past the vertex list
/// would reach 005bc430 in the game too. [data levels\*\*\*.str, scan 2026-10-20] every CollisionMeshPacket in the
/// install's level packs (2413 packets, 24844 faces, masked 0x7fffffff, against each packet's nVerts) has its
/// indices inside its vertex list and its face count equal to nFaces, so the skip below never fires on shipped
/// data. The skip itself is the port's choice for invalid input (the game's behaviour there is a stack read,
/// not reproduced).
pub fn soup_records(soup: &crate::collision_soup::Soup, x: &[f32; 12]) -> Vec<SoupTri> {
    let v = crate::collision_soup::transform_vertices(&soup.vertices, x);
    let count = soup.triangles.len() as u16 as usize;
    let mut out = Vec::with_capacity(count);
    for tri in soup.triangles.iter().take(count) {
        let idx = tri.map(|w| (w & 0x7fff_ffff) as usize);
        if idx.iter().any(|&i| i >= v.len()) {
            continue;
        }
        let [a, b, c] = idx.map(|i| Vec3::new(v[i][0], v[i][1], v[i][2]));
        let (e1, e2) = (b - a, c - a);
        // [game] 005bc430's face loop, in the decompile's terms
        let nx = e1.y * e2.z - e1.z * e2.y;
        let ny = e1.z * e2.x - e2.z * e1.x;
        let nz = e2.y * e1.x - e1.y * e2.x;
        let root = (((nz * nz + ny * ny) + nx * nx) as f64).sqrt();
        let mut n = Vec3::new(nx, ny, nz);
        if 1e-8 < root {
            let inv = 1.0f32 / root as f32;
            n = Vec3::new(inv * nx, ny * inv, nz * inv);
        }
        out.push(SoupTri { v: [a, b, c], normal: n, flag: 0.0 });
    }
    out
}

/// [game] the soup row's selector R0 (005e3f50 for a sphere A, 005e4100 a cube, 0x1b0 apart;
/// disassembly of 005e3f50): each triangle whose flag WORD is 0 (an integer test, so -0.0 is
/// skipped too) and whose normal points against A's displacement (A's alone, the input's +0x54..
/// minus +0x24..; NaN skips) goes through its W2 wrapper (`pair_cell` with the triangle as B).
/// The hit with the least lambda (+0xf4, strictly less, so the first of equals) is kept, and the
/// selector writes over its copy the triangle's STORED normal (+0xcc, also when the cast started
/// in contact: the W2's re-cast is lost) and the triangle's index as a 16-bit count (+0xf0).
pub fn soup_cell(a: PairShape, tris: &[SoupTri], inp: &PairInput) -> Option<PairRecord> {
    let da = origin(&inp.a1) - origin(&inp.a0);
    let mut best: Option<PairRecord> = None;
    for (i, t) in tris.iter().enumerate() {
        let dot = (t.normal.x * da.x + t.normal.y * da.y) + da.z * t.normal.z;
        if t.flag.to_bits() != 0 || !(0.0 > dot) {
            continue;
        }
        let Some(mut r) = pair_cell(a, PairShape::Triangle { v: t.v }, inp) else { continue };
        if best.map_or(f32::MAX, |b| b.lambda) > r.lambda {
            r.normal = t.normal;
            r.face = i as u16 as u32;
            best = Some(r);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    const REF_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../work/emu/ref/");

    #[test]
    fn inverse_of_a_rotation_and_shift() {
        // 90 degrees about z, origin (1, 2, 3): the inverse maps the origin back to zero
        let m = [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 3.0];
        let inv = inverse_affine3(&m);
        assert_eq!(apply(&inv, Vec3::new(1.0, 2.0, 3.0)), Vec3::ZERO);
        // a singular basis keeps the adjugate unscaled
        let flat = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        assert_eq!(&inverse_affine3(&flat)[..9], &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn spheres_meeting_head_on() {
        // radii 1 and 1, B from z = 3 to z = -3 against A at rest: they touch at t = 1/6
        let id = |o: [f32; 3]| [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, o[0], o[1], o[2]];
        let inp = PairInput {
            a0: id([0.0; 3]),
            a1: id([0.0; 3]),
            b0: id([0.0, 0.0, 3.0]),
            b1: id([0.0, 0.0, -3.0]),
            tag: [0, 0],
            index: 0,
        };
        let s = PairShape::Sphere { radius: 1.0 };
        let r = pair_cell(s, s, &inp).expect("hit");
        assert!((r.t - 1.0 / 6.0).abs() < 1e-3, "t {}", r.t);
        assert!((r.b_at[11] - 2.0).abs() < 1e-2, "B at t {:?}", &r.b_at[9..]);
    }

    fn hw(s: &str) -> u32 {
        u32::from_str_radix(s, 16).expect("hex word")
    }

    /// A hull's block from the reference row's extra columns (nv, x y z w per vertex, np, index link
    /// per pair), starting at `*at`, which is moved past it.
    fn hull_from(c: &[&str], at: &mut usize) -> HullBlock {
        let mut next = || {
            *at += 1;
            c[*at - 1]
        };
        let nv: usize = next().parse().expect("nv");
        let vertices: Vec<[f32; 4]> = (0..nv).map(|_| std::array::from_fn(|_| f32::from_bits(hw(next())))).collect();
        let np: usize = next().parse().expect("np");
        let pairs: Vec<(i64, i64)> = (0..np).map(|_| (next().parse().expect("index"), next().parse().expect("link"))).collect();
        let faces: Vec<Vec<(i64, i64)>> = pairs.chunks(3).map(|f| f.to_vec()).collect();
        crate::collision_world::hull_block(vertices, &faces).expect("a valid hull block")
    }

    /// [game] 005bc430 against the emulator's runs of it whole (`work\emu\gen_refs_soup_build.py`):
    /// every word of every record bit for bit (the w lanes are the vertices' w, passed through).
    #[test]
    fn reference_soup_records() {
        let path = format!("{REF_DIR}soup_build.tsv");
        let Ok(text) = std::fs::read_to_string(&path) else {
            println!("note: {path} absent; skipped");
            return;
        };
        let (mut cases, mut words, mut bad) = (0, 0, 0);
        for line in text.lines().filter(|l| !l.is_empty()) {
            let c: Vec<&str> = line.split('\t').collect();
            let x: [f32; 12] = std::array::from_fn(|k| f32::from_bits(hw(c[k])));
            let a: usize = c[12].parse().unwrap();
            let vertices: Vec<[f32; 4]> = (0..a).map(|i| std::array::from_fn(|k| f32::from_bits(hw(c[13 + 4 * i + k])))).collect();
            let at = 13 + 4 * a;
            let b: usize = c[at].parse().unwrap();
            let triangles: Vec<[u32; 3]> = (0..b).map(|t| std::array::from_fn(|k| c[at + 1 + 3 * t + k].parse().unwrap())).collect();
            let recs = &c[at + 1 + 3 * b..];
            assert_eq!(recs.len(), 16 * b);
            let w_of: Vec<f32> = vertices.iter().map(|v| v[3]).collect();
            let soup = crate::collision_soup::Soup { vertices, triangles: triangles.clone() };
            let ours = soup_records(&soup, &x);
            assert_eq!(ours.len(), b);
            cases += 1;
            for (t, r) in ours.iter().enumerate() {
                let idx = triangles[t].map(|w| (w & 0x7fff_ffff) as usize);
                let mut mine = [0u32; 16];
                for k in 0..3 {
                    mine[4 * k..4 * k + 3].copy_from_slice(&r.v[k].to_array().map(f32::to_bits));
                    mine[4 * k + 3] = w_of[idx[k]].to_bits();
                }
                mine[12..15].copy_from_slice(&r.normal.to_array().map(f32::to_bits));
                mine[15] = r.flag.to_bits();
                for k in 0..16 {
                    words += 1;
                    if mine[k] != hw(recs[16 * t + k]) {
                        bad += 1;
                        if bad <= 5 {
                            println!("case {cases} face {t} word {k}: ours {:e} game {:e}", f32::from_bits(mine[k]), f32::from_bits(hw(recs[16 * t + k])));
                        }
                    }
                }
            }
        }
        println!("{cases} soups, {words} words, {bad} differ");
        assert_eq!(bad, 0);
    }

    /// A soup's triangles from the row's extra columns (the count, then 16 words per record).
    fn soup_from(c: &[&str], at: &mut usize) -> Vec<SoupTri> {
        let n: usize = c[*at].parse().expect("triangle count");
        *at += 1;
        let tris = (0..n)
            .map(|i| {
                let f = |k: usize| f32::from_bits(hw(c[*at + 16 * i + k]));
                let v = |k: usize| Vec3::new(f(k), f(k + 1), f(k + 2));
                SoupTri { v: [v(0), v(4), v(8)], normal: v(12), flag: f(15) }
            })
            .collect();
        *at += 16 * n;
        tris
    }

    fn hull_width(kind: u32) -> Option<u8> {
        match kind {
            7 => Some(32),
            8 => Some(16),
            9 => Some(8),
            _ => None,
        }
    }

    fn shape<'a>(kind: u32, w: &[u32], hull: Option<&'a HullBlock>) -> PairShape<'a> {
        let f = |i: usize| f32::from_bits(w[i]);
        if let (Some(width), Some(block)) = (hull_width(kind), hull) {
            return PairShape::Hull { width, block };
        }
        match kind {
            1 => PairShape::Sphere { radius: f(0) },
            2 => PairShape::Cube { half: Vec3::new(f(0), f(1), f(2)) },
            3 => PairShape::Capsule { b: std::array::from_fn(f) },
            4 => PairShape::Cylinder { r: f(0), h: f(1) },
            5 => PairShape::Cone { r: f(0), h: f(1), a: f(2), c: f(3) },
            6 => PairShape::Pencil { r: f(0), h: f(1), a: f(2), c: f(3) },
            _ => unreachable!(),
        }
    }

    /// The record's 66 words as the game lays them out (words never written stay None).
    fn words(r: &PairRecord) -> [Option<u32>; 66] {
        let mut out = [None; 66];
        let mut put = |at: usize, v: &[f32]| {
            for (i, x) in v.iter().enumerate() {
                out[at / 4 + i] = Some(x.to_bits());
            }
        };
        put(0x00, &r.a_at);
        put(0x30, &r.a_inv);
        put(0x60, &r.b_at);
        put(0x90, &r.b_inv);
        put(0xc0, &r.point.to_array());
        put(0xcc, &r.normal.to_array());
        put(0xd8, &r.point_in_a.to_array());
        put(0xe4, &r.point_in_b.to_array());
        put(0xf4, &[r.lambda, r.t]);
        out[0xf0 / 4] = Some(r.face);
        out[0xfc / 4] = Some(r.tag[0]);
        out[0x100 / 4] = Some(r.tag[1]);
        out[0x104 / 4] = Some(r.index);
        out
    }

    const FIELDS: [(&str, usize, usize); 13] = [
        ("A at t", 0x00, 12),
        ("A inverse", 0x30, 12),
        ("B at t", 0x60, 12),
        ("B inverse", 0x90, 12),
        ("point", 0xc0, 3),
        ("normal", 0xcc, 3),
        ("point in A", 0xd8, 3),
        ("point in B", 0xe4, 3),
        ("-1", 0xf0, 1),
        ("lambda", 0xf4, 1),
        ("t", 0xf8, 1),
        ("tags", 0xfc, 2),
        ("index", 0x104, 1),
    ];

    /// Runs one reference file; returns (rows, hit/miss agreements, per field: hits whose words
    /// all agree bit for bit, and the largest float difference).
    fn run(row: u32, col: u32, contact: bool) -> Option<(usize, usize, usize, Vec<(usize, f32)>)> {
        let path = format!("{REF_DIR}pair_{row}_{col}.tsv");
        let text = std::fs::read_to_string(&path).ok()?;
        let (mut rows, mut agree, mut hits) = (0, 0, 0);
        let mut fields = vec![(0usize, 0.0f32); FIELDS.len()];
        for line in text.lines().filter(|l| !l.is_empty()) {
            let c: Vec<&str> = line.split('\t').collect();
            assert!(c.len() >= 152, "{path}: {} columns", c.len());
            let w: Vec<u32> = c[..84].iter().map(|s| hw(s)).collect();
            let f = |i: usize| f32::from_bits(w[i]);
            let m = |at: usize| -> [f32; 12] { std::array::from_fn(|k| f(32 + at + k)) };
            let inp = PairInput { a0: m(0), a1: m(12), b0: m(24), b1: m(36), tag: [w[80], w[81]], index: w[82] };
            // the hulls follow the record: A's first, then B's
            let mut at = 152;
            let hull_a = hull_width(col).map(|_| hull_from(&c, &mut at));
            let hull_b = hull_width(row).map(|_| hull_from(&c, &mut at));
            let soup = (row == 0).then(|| soup_from(&c, &mut at));
            assert_eq!(at, c.len(), "{path}: columns left after the hulls");
            // [game] cell [row][col]: A is of type col, B of type row (see `pair_cell`)
            let a = shape(col, &w[0..16], hull_a.as_ref());
            let ours = match &soup {
                Some(tris) => soup_cell(a, tris, &inp),
                None => pair_cell(a, shape(row, &w[16..32], hull_b.as_ref()), &inp),
            };
            rows += 1;
            let ref_hit = c[84] == "1";
            if ours.is_some() == ref_hit {
                agree += 1;
            }
            let (Some(r), true) = (ours, ref_hit) else { continue };
            if r.contact != contact {
                continue;
            }
            hits += 1;
            let out: Vec<u32> = c[86..152].iter().map(|s| hw(s)).collect();
            let ow = words(&r);
            for (k, &(_, at, n)) in FIELDS.iter().enumerate() {
                let mut same = true;
                for i in at / 4..at / 4 + n {
                    let ours = ow[i].unwrap();
                    if ours != out[i] {
                        same = false;
                        let d = (f32::from_bits(ours) - f32::from_bits(out[i])).abs();
                        if d.is_finite() && d > fields[k].1 {
                            fields[k].1 = d;
                        }
                    }
                }
                fields[k].0 += same as usize;
            }
        }
        Some((rows, agree, hits, fields))
    }

    /// [game] every cell of the six simple shapes against the emulator's runs of the whole cell
    /// (`work\emu\gen_refs_pairs.py <row> <col> 400`): hit/miss on every row, and every field of
    /// every hit bit for bit, ordinary hits and starts in contact (the 005be420 re-cast) alike.
    #[test]
    fn reference_pair_cells() {
        let mut cells = 0;
        let mut failures = Vec::new();
        // the six simple shapes and the hulls 7-9 (Hull32, Hull16, Hull8); no Hull4, no soup
        for row in 0..=9 {
            for col in 1..=9 {
                let Some((rows, agree, hits, fields)) = run(row, col, false) else {
                    println!("note: pair_{row}_{col}.tsv absent (work\\emu\\gen_refs_pairs.py); skipped");
                    continue;
                };
                let (_, _, c_hits, c_fields) = run(row, col, true).unwrap();
                cells += 1;
                println!("[{row}][{col}] rows {rows}, hit/miss agree {agree}; ordinary hits {hits}, in contact {c_hits}");
                if agree != rows {
                    failures.push(format!("pair [{row}][{col}]: hit/miss agree {agree} / {rows}"));
                }
                for (k, &(name, _, _)) in FIELDS.iter().enumerate() {
                    if fields[k].0 != hits {
                        failures.push(format!("pair [{row}][{col}]: {name} exact on {} / {hits} ordinary hits ({:e})", fields[k].0, fields[k].1));
                    }
                    if c_fields[k].0 != c_hits {
                        failures.push(format!("pair [{row}][{col}]: {name} exact on {} / {c_hits} in contact ({:e})", c_fields[k].0, c_fields[k].1));
                    }
                }
            }
        }
        println!("{cells} cells checked");
        for f in &failures {
            println!("  {f}");
        }
        assert!(failures.is_empty(), "{} disagreements", failures.len());
    }
}
