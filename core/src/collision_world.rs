//! A level's static collision: every primitive of a zone pack placed in the world, and a
//! ray cast against all of them.
//!
//! Placement rule (`docs/collision-world.md`; survey by the porter, 2026-10-11, from
//! `work\survey\collision\`):
//! - A Collision rides a node of an Object (`parent`). A node's `bind` is ABSOLUTE in the
//!   Object's own space, not chained to its parent node. [data] Bumblebee's humanoid skeleton:
//!   absolute z runs -0.05 .. 5.42 m (feet on 0, head near 5.3); chained, -7.6 .. 15.1 m.
//! - A primitive's world matrix is `primitive.matrix`, then `bind(parent node)`, then ACC
//!   (row vectors: p * M1 * M2 ...). ACC is the composition of every enclosing placement: each
//!   `matrix` of an `Instance` (a root entry, or one inside a `Scene`; outermost last), and for
//!   an Object nested in another Object's attachment, the bind of the attachment's node `index`.
//! - One stored Collision is placed once per distinct ACC. [data] Shanghai's 871 stored
//!   Collisions are placed 14,708 times in all: each building is instanced (`docs/collision-world.md`).
//! - Primitives under a `DamageLink` or `DetachLink` are kept, flagged `released` (they are
//!   debris and damage variants released when the parent breaks) [assumed: not checked against
//!   the game's release rule].
//!
//! Cuboid `extent` is HALF the size (stored value used as the half extent; [data] the resting
//! peak in `docs/collision-world.md`). Cylinder `height12` and cone heights are used as the
//! game's helper uses them (`gjk_shapes`), [game]/[assumed] see the note.
//!
//! Tags: `[data]` seen in the decoded file, `[game]` from a note with an address, `[assumed]`.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use glam::Vec3;

use crate::collision_soup::Soup;
use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};
use crate::gjk::{self, RayHit, Support, Triangle};
use crate::gjk_shapes;
use crate::gjk_swept;
use crate::gjk_world::{ray_test, Cuboid, LocalSupport, Placed};

/// Rows 0..2 are the axes (the images of x, y, z); row 3 the position. Row vectors: p * M.
type Mat = [[f64; 3]; 4];

/// Side of one cell of the xy grid, metres. [stand-in] (see `Grid`).
const GRID_CELL: f32 = 16.0;
/// A box wider than this in x or y stays out of the grid's cells (see `Grid::build`).
const GRID_LARGE: f32 = 2048.0;

/// Added to every primitive's box on every side, metres, so a box never cuts a hit short.
const BOX_MARGIN: f32 = 0.01;

/// Added to the ray's own box (and to its xy rectangle in the grid), metres. `gjk::ray_cast`
/// ends when |v|^2 is at or below 0.001 (`EPS_DIST2`, about 3 cm), so it can report a hit up to
/// that far off the shape's surface; the ray side is widened by more than that, so no hit the
/// brute loop returns is cut away. [assumed] the bound; the test below is what checks it.
const RAY_TOL: f32 = 0.05;

const IDENTITY: Mat = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0]];

/// The walk stops here (as `formats::collision` does).
const MAX_DEPTH: u32 = 80;

/// Placements of one stored Collision are told apart by ACC rounded to this many steps per metre.
const KEY_SCALE: f64 = 1.0e4;

/// The shape of one placed primitive, with its world transform as the ray tests take it.
pub enum WorldShape {
    /// `CollisionCuboidPacket`: `half` is the stored extent (half size) [data, see module note].
    Cuboid(Placed<Cuboid>),
    /// `CollisionSpherePacket`; `x` is the world transform (the sphere's own `x` / `y`).
    Sphere(gjk::Sphere),
    /// `CollisionCylinderPacket`: `r` = radius, `h` = height12 (used as the half height).
    Cylinder(Placed<gjk_shapes::Cylinder>),
    /// `CollisionConePacket`: base at -height14 with radius `radius`, apex at +height34, `c` = angle.
    Cone(Placed<gjk_shapes::Cone>),
    /// `CollisionMeshPacket`: the triangle soup in its own space, placed by `x` (the 3x4 world).
    Mesh { soup: Soup, x: [f32; 12] },
    /// `CollisionHull8Packet` / `CollisionHull16Packet`, placed through the hull walk (see `HullLocal`).
    /// [data] Hull8 and Hull16 are the only hulls in the level zone.
    Hull(Placed<HullLocal>),
    /// A Hull32, a capsule, a primitive with no packet, or a hull whose links fail `hull_block`:
    /// counted, not ray-tested.
    Unsupported { type_name: String },
}

/// The Instance a placed primitive belongs to: (the pack's place in the list the level was loaded from, the
/// pack's chunk index, the Instance's offset in that chunk). The script names an Instance by its chunk and offset
/// (`mission_script::NodeRef`); the pack place keeps two packs' chunk 3 apart. [assumed] the mission pack's place is
/// the one `mission.rs` finds by name.
pub type Owner = (usize, usize, usize);

/// One placed copy of a stored primitive.
pub struct WorldPrimitive {
    pub shape: WorldShape,
    /// The innermost Instance above the primitive's Collision, or None when no Instance places it.
    pub owner: Option<Owner>,
    /// `"<pack>#<chunk> @ <collision offset> primitive <n>"`: the same for every placement of
    /// the stored primitive, so distinct sources are distinct stored primitives.
    pub source: String,
    /// `material` and `maskPreset`: CRC-32 name hashes as stored.
    pub material: u32,
    pub mask_preset: u32,
    pub send_mask: i64,
    pub receive_mask: i64,
    /// The Collision has a `DynamicsData` (a movable body) [data]. Included, flagged.
    pub has_dynamics: bool,
    /// Under a `DamageLink` or `DetachLink` of its Object: released when the parent breaks.
    /// [assumed] (an addition to the ticket's struct, which has no such field).
    pub released: bool,
    /// The world position of the primitive's origin (row 3 of its world matrix).
    pub centre: Vec3,
}

/// One ray hit. `primitive` indexes `StaticWorld::primitives`; `triangle` is set for a mesh.
#[derive(Clone, Copy, Debug)]
pub struct WorldHit {
    pub primitive: usize,
    pub point: Vec3,
    pub normal: Vec3,
    pub lambda: f32,
    pub contact: bool,
    pub triangle: Option<u32>,
}

/// An axis-aligned box. The empty box (min above max) overlaps nothing.
#[derive(Clone, Copy, Debug)]
struct Aabb {
    min: Vec3,
    max: Vec3,
}

impl Aabb {
    fn empty() -> Aabb {
        Aabb { min: Vec3::splat(f32::INFINITY), max: Vec3::splat(f32::NEG_INFINITY) }
    }

    fn include(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    fn overlaps(&self, o: &Aabb) -> bool {
        self.min.cmple(o.max).all() && o.min.cmple(self.max).all()
    }
}

/// The box of a shape that has a support function: its support points along +-x, +-y, +-z.
fn support_box<S: Support>(s: &S) -> Aabb {
    let mut b = Aabb::empty();
    for d in [Vec3::X, Vec3::new(-1.0, 0.0, 0.0), Vec3::Y, Vec3::new(0.0, -1.0, 0.0), Vec3::Z, Vec3::new(0.0, 0.0, -1.0)] {
        b.include(s.support(d));
    }
    b
}

/// The box of a mesh: every vertex carried by `x`, with the same affine sums as
/// `Soup::ray_cast` (`collision_soup.rs`). Every vertex is used, a superset of the referenced ones.
fn mesh_box(soup: &Soup, x: &[f32; 12]) -> Aabb {
    let mut b = Aabb::empty();
    for v in &soup.vertices {
        let [vx, vy, vz, w] = *v;
        b.include(Vec3::new(
            x[0] * vx + x[3] * vy + x[6] * vz + x[9] * w,
            x[1] * vx + x[4] * vy + x[7] * vz + x[10] * w,
            x[2] * vx + x[5] * vy + x[8] * vz + x[11] * w,
        ));
    }
    b
}

/// The widened world box of one primitive (empty for an unsupported one).
fn primitive_box(shape: &WorldShape) -> Aabb {
    let b = match shape {
        WorldShape::Cuboid(s) => support_box(s),
        WorldShape::Sphere(s) => support_box(s),
        WorldShape::Cylinder(s) => support_box(s),
        WorldShape::Cone(s) => support_box(s),
        WorldShape::Hull(s) => hull_box(s),
        WorldShape::Mesh { soup, x } => mesh_box(soup, x),
        WorldShape::Unsupported { .. } => return Aabb::empty(),
    };
    if b.is_empty() {
        return b;
    }
    Aabb { min: b.min - Vec3::splat(BOX_MARGIN), max: b.max + Vec3::splat(BOX_MARGIN) }
}

/// [stand-in] A uniform xy grid of `GRID_CELL` cells over the boxes of all primitives: each
/// cell lists the primitives whose box overlaps it. The game's own structure is 16 buckets each
/// with a box and a tree (005b3620 / 005b2630), not read yet; this grid only skips work and must
/// return exactly what the brute loop returns.
struct Grid {
    /// The xy extent of all boxes (min, max).
    lo: [f32; 2],
    hi: [f32; 2],
    nx: usize,
    ny: usize,
    /// `cells[iy * nx + ix]`: the primitive indices, ascending.
    cells: Vec<Vec<u32>>,
    /// The primitives too wide for the cells: every query tries them.
    large: Vec<u32>,
}

/// The cell column or row of `v`, clamped into 0..n. The same map for boxes and for rays, so a
/// box that meets a ray's xy rectangle always shares a cell with it.
fn cell_index(v: f32, lo: f32, n: usize) -> usize {
    let c = ((v - lo) / GRID_CELL).floor();
    c.max(0.0).min((n - 1) as f32) as usize
}

impl Grid {
    fn build(boxes: &[Aabb]) -> Grid {
        // A box wider than `GRID_LARGE` (or not finite) is kept out of the cells and offered to
        // every query: one such primitive (deep_six_01 has them) would otherwise make the grid
        // as wide as itself and fill millions of cells.
        let large_one = |b: &Aabb| {
            !(b.min.is_finite() && b.max.is_finite()) || b.max.x - b.min.x > GRID_LARGE || b.max.y - b.min.y > GRID_LARGE
        };
        let large: Vec<u32> = (0..boxes.len()).filter(|&i| !boxes[i].is_empty() && large_one(&boxes[i])).map(|i| i as u32).collect();
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for b in boxes.iter().filter(|b| !b.is_empty() && !large_one(b)) {
            lo = lo.min(b.min);
            hi = hi.max(b.max);
        }
        if lo.x > hi.x || lo.y > hi.y {
            return Grid { lo: [0.0; 2], hi: [0.0; 2], nx: 0, ny: 0, cells: Vec::new(), large };
        }
        let nx = ((hi.x - lo.x) / GRID_CELL).floor() as usize + 1;
        let ny = ((hi.y - lo.y) / GRID_CELL).floor() as usize + 1;
        let mut cells: Vec<Vec<u32>> = vec![Vec::new(); nx * ny];
        for (i, b) in boxes.iter().enumerate() {
            if b.is_empty() || large_one(b) {
                continue;
            }
            let x0 = cell_index(b.min.x, lo.x, nx);
            let x1 = cell_index(b.max.x, lo.x, nx);
            let y0 = cell_index(b.min.y, lo.y, ny);
            let y1 = cell_index(b.max.y, lo.y, ny);
            for iy in y0..=y1 {
                for ix in x0..=x1 {
                    cells[iy * nx + ix].push(i as u32);
                }
            }
        }
        Grid { lo: [lo.x, lo.y], hi: [hi.x, hi.y], nx, ny, cells, large }
    }

    /// The unique primitive indices in the cells that the xy rectangle from `min` to `max` covers,
    /// ascending.
    fn candidates(&self, min: Vec3, max: Vec3) -> Vec<u32> {
        let (xmin, xmax) = (min.x, max.x);
        let (ymin, ymax) = (min.y, max.y);
        let mut out = self.large.clone();
        if self.nx != 0 && !(xmax < self.lo[0] || xmin > self.hi[0] || ymax < self.lo[1] || ymin > self.hi[1]) {
            let x0 = cell_index(xmin, self.lo[0], self.nx);
            let x1 = cell_index(xmax, self.lo[0], self.nx);
            let y0 = cell_index(ymin, self.lo[1], self.ny);
            let y1 = cell_index(ymax, self.lo[1], self.ny);
            for iy in y0..=y1 {
                for ix in x0..=x1 {
                    out.extend_from_slice(&self.cells[iy * self.nx + ix]);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

pub struct StaticWorld {
    pub primitives: Vec<WorldPrimitive>,
    /// The widened world box of each primitive, by index (empty for an unsupported one).
    boxes: Vec<Aabb>,
    grid: Grid,
}

impl StaticWorld {
    /// Loads every data chunk of a zone pack (`levels\<zone>\zone.str`) and places its Collisions.
    pub fn load(zone_pack: &Path) -> Result<StaticWorld, String> {
        StaticWorld::load_packs(&[zone_pack.to_path_buf()])
    }

    /// Several packs as one world: a mission's `streamFiles` (the zone, then the mission's pack).
    pub fn load_packs(packs: &[std::path::PathBuf]) -> Result<StaticWorld, String> {
        let mut primitives = Vec::new();
        for (ordinal, path) in packs.iter().enumerate() {
            let pack = Pack::open(path)?;
            let file_name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            for chunk in pack.of_type(pack::DATA) {
                let file = DataFile::parse(pack.data(chunk).to_vec()).map_err(|e| format!("chunk {}: {e}", chunk.index))?;
                let label = format!("{}#{}", file_name, chunk.index);
                let mut sink = Sink { label: &label, pack: ordinal, chunk: chunk.index, seen: HashSet::new(), visited: HashSet::new(), out: &mut primitives };
                visit(file.root(), IDENTITY, None, None, false, false, 0, &mut sink);
            }
        }
        Ok(StaticWorld::from_primitives(primitives))
    }

    /// Builds the boxes and the grid over the given primitives.
    pub(crate) fn from_primitives(primitives: Vec<WorldPrimitive>) -> StaticWorld {
        let boxes: Vec<Aabb> = primitives.iter().map(|p| primitive_box(&p.shape)).collect();
        let grid = Grid::build(&boxes);
        StaticWorld { primitives, boxes, grid }
    }

    /// Every hit of the segment p0 -> p1 against every primitive, in primitive order (no mask
    /// filtering). A mesh gives one hit per triangle it meets. Same result as `ray_cast_brute`;
    /// only primitives whose box meets the segment are ray-tested.
    pub fn ray_cast(&self, p0: Vec3, p1: Vec3) -> Vec<WorldHit> {
        let tol = Vec3::splat(RAY_TOL);
        let seg = Aabb { min: p0.min(p1) - tol, max: p0.max(p1) + tol };
        let mut hits = Vec::new();
        for i in self.grid.candidates(seg.min, seg.max) {
            let i = i as usize;
            if !self.boxes[i].overlaps(&seg) {
                continue;
            }
            cast_one(i, &self.primitives[i].shape, p0, p1, &mut hits);
        }
        hits
    }

    /// As `ray_cast`, against the primitives listed in `indices` only (`LevelWorld` lists one Instance's).
    pub fn ray_cast_some(&self, indices: &[usize], p0: Vec3, p1: Vec3) -> Vec<WorldHit> {
        let tol = Vec3::splat(RAY_TOL);
        let seg = Aabb { min: p0.min(p1) - tol, max: p0.max(p1) + tol };
        let mut hits = Vec::new();
        for &i in indices {
            if !self.boxes[i].overlaps(&seg) {
                continue;
            }
            cast_one(i, &self.primitives[i].shape, p0, p1, &mut hits);
        }
        hits
    }

    /// Every hit of the segment p0 -> p1 against every primitive, in primitive order (no broad
    /// phase, no mask filtering). A mesh gives one hit per triangle it meets.
    pub fn ray_cast_brute(&self, p0: Vec3, p1: Vec3) -> Vec<WorldHit> {
        let mut hits = Vec::new();
        for (i, prim) in self.primitives.iter().enumerate() {
            cast_one(i, &prim.shape, p0, p1, &mut hits);
        }
        hits
    }

    /// The hit with the smallest lambda. [assumed] a convenience; the game's own choice of
    /// hit is another ticket.
    pub fn nearest(&self, p0: Vec3, p1: Vec3) -> Option<WorldHit> {
        self.ray_cast(p0, p1).into_iter().min_by(|a, b| a.lambda.total_cmp(&b.lambda))
    }

    /// Every hit of a sphere of `radius` swept along the centre's path p0 -> p1, in primitive order
    /// (no mask filtering). Same grid as `ray_cast`, widened by the radius: a primitive's box meets
    /// the swept sphere's path only if its box, widened by the radius, meets the segment's box.
    /// Cuboid, Sphere, Cylinder and Cone use `gjk_swept::sphere_cast`; a mesh is swept per triangle
    /// ([assumed], see `sweep_mesh`). `lambda` is the centre's distance from p0; `point` is on the
    /// target's surface (see `gjk_swept`).
    pub fn sphere_cast(&self, p0: Vec3, p1: Vec3, radius: f32) -> Vec<WorldHit> {
        let tol = Vec3::splat(RAY_TOL + radius);
        let seg = Aabb { min: p0.min(p1) - tol, max: p0.max(p1) + tol };
        let mut hits = Vec::new();
        for i in self.grid.candidates(seg.min, seg.max) {
            let i = i as usize;
            if !self.boxes[i].overlaps(&seg) {
                continue;
            }
            sweep_one(i, &self.primitives[i].shape, p0, p1, radius, &mut hits);
        }
        hits
    }

    /// Sweep the upright avatar pencil, with positions at its feet.
    /// [game] Dimensions and centre: 006e7e90 / 006e7b60, documented in
    /// `docs/character-ground-rule.md` "The slot mapping". The support helper
    /// is the existing emulated 005aff10 port.
    /// [stand-in] This convex Minkowski cast uses the ray GJK kernel instead
    /// of the original swept-pair dispatcher and contact-manifold generator.
    pub fn pencil_cast(&self, p0: Vec3, p1: Vec3, radius: f32, height: f32) -> Vec<WorldHit> {
        let gap_part = height * 0.45454544;
        let cylinder = height - gap_part;
        let h = cylinder * 0.5;
        let tip = gap_part * 0.5;
        let pencil = gjk_shapes::Pencil { r: radius, h, a: h + tip,
            c: h / (tip * tip + radius * radius).sqrt() };
        let centre = Vec3::Z * (h + cylinder * 0.45454544);
        let (p0, p1) = (p0 + centre, p1 + centre);
        let extent = Vec3::new(radius, radius, pencil.a) + Vec3::splat(RAY_TOL);
        let seg = Aabb { min: p0.min(p1) - extent, max: p0.max(p1) + extent };
        let mut hits = Vec::new();
        for i in self.grid.candidates(seg.min, seg.max) {
            let i = i as usize;
            if self.boxes[i].overlaps(&seg) {
                pencil_one(i, &self.primitives[i].shape, p0, p1, &pencil, &mut hits);
            }
        }
        hits
    }
}

impl StaticWorld {
    /// The pair nodes (`physics_contact`) of the primitives whose boxes meet the box `lo`..`hi`, in
    /// primitive order. `masks` gives a primitive's holder masks (+0x9c, +0x9e), or None to leave it
    /// out. [game] a mesh's node takes its triangles moved into the world (005bc430,
    /// `pair_sweep::soup_records`) with the identity pose; the other shapes keep their pose.
    /// [stand-in] the box overlap replaces the game's broad phase (006e5e00, its cull partly read).
    /// Unsupported primitives (Hull32 among them: no level zone has one) are left out.
    pub fn pair_nodes(&self, lo: Vec3, hi: Vec3, mut masks: impl FnMut(usize) -> Option<[u16; 2]>) -> Vec<crate::physics_contact::StaticNode<'_>> {
        use crate::pair_sweep::PairShape;
        use crate::physics_contact::{StaticNode, StaticTarget};
        let want = Aabb { min: lo, max: hi };
        let mut out = Vec::new();
        for i in self.grid.candidates(lo, hi) {
            let i = i as usize;
            if !self.boxes[i].overlaps(&want) {
                continue;
            }
            let Some(m) = masks(i) else { continue };
            let shape = |shape, pose| StaticTarget::Shape { shape, pose };
            let target = match &self.primitives[i].shape {
                WorldShape::Cuboid(s) => shape(PairShape::Cube { half: s.shape.half }, s.x),
                WorldShape::Sphere(s) => shape(PairShape::Sphere { radius: s.radius }, s.x),
                WorldShape::Cylinder(s) => shape(PairShape::Cylinder { r: s.shape.r, h: s.shape.h }, s.x),
                WorldShape::Cone(s) => shape(PairShape::Cone { r: s.shape.r, h: s.shape.h, a: s.shape.a, c: s.shape.c }, s.x),
                WorldShape::Hull(s) => shape(PairShape::Hull { width: s.shape.width, block: &s.shape.block }, s.x),
                WorldShape::Mesh { soup, x } => StaticTarget::Soup(crate::pair_sweep::soup_records(soup, x)),
                WorldShape::Unsupported { .. } => continue,
            };
            out.push(StaticNode::new(target, m, i));
        }
        out
    }
}

struct PencilGrown<'a, S> { target: &'a S, pencil: &'a gjk_shapes::Pencil }
impl<S: Support> Support for PencilGrown<'_, S> {
    fn support(&self, v: Vec3) -> Vec3 {
        // [game 006d02a0] The native pair support adds cube and pencil support
        // in the SAME direction. The pencil is symmetric, but subtracting
        // support(-v) changes the zero-Z tie and the resulting contact witness.
        self.target.support(v) + self.pencil.local_support(v)
    }

    fn witness_support(&self, v: Vec3) -> Option<Vec3> {
        Some(self.target.support(v))
    }
}

fn pencil_hit<S: Support>(p0: Vec3, p1: Vec3, target: &S, seed: Vec3,
    pencil: &gjk_shapes::Pencil) -> Option<RayHit> {
    gjk::ray_cast(p0, p1, &PencilGrown {target, pencil}, seed)
}

fn pencil_one(index: usize, shape: &WorldShape, p0: Vec3, p1: Vec3,
    pencil: &gjk_shapes::Pencil, hits: &mut Vec<WorldHit>) {
    macro_rules! cast { ($s:expr) => {
        if let Some(h) = pencil_hit(p0, p1, $s, origin(&$s.x), pencil) {
            hits.push(hit(index, h, None));
        }
    }; }
    match shape {
        WorldShape::Cuboid(s) => cast!(s),
        WorldShape::Sphere(s) => cast!(s),
        WorldShape::Cylinder(s) => cast!(s),
        WorldShape::Cone(s) => cast!(s),
        WorldShape::Hull(s) => {reset_hull_cache(s); cast!(s);},
        WorldShape::Mesh {soup, x} => {
            // [stand-in] Sweep each placed triangle; the original soup pair
            // traversal and manifold assembly remain unported.
            let v = place_vertices(&soup.vertices, x);
            for (ti, tri) in soup.triangles.iter().enumerate() {
                let idx = tri.map(|w| (w & SOUP_INDEX_MASK) as usize);
                if idx.iter().any(|&i| i >= v.len()) {continue;}
                let target = Triangle {a:v[idx[0]], b:v[idx[1]], c:v[idx[2]]};
                if let Some(h) = pencil_hit(p0, p1, &target, origin(x), pencil) {
                    // Retain the convex sweep normal at edges and vertices.
                    // Replacing it with a triangle face loses the pencil's
                    // rounded contact with facade protrusions.
                    hits.push(hit(index, h, Some(ti as u32)));
                }
            }
        },
        WorldShape::Unsupported {..} => {}
    }
}

/// [assumed] The walk's start unit at the start of each ray test or sweep: the corner at unit 1
/// (face 0, corner 0). The game seeds it from the query record's +0x18 (`docs/collision-support-hull.md`
/// section 1), whose meaning is not read. Any valid unit gives the same farthest vertex except on exact ties.
const HULL_CACHE_START: u32 = 1;

/// A convex hull in its own frame, as `Placed` needs it. `cache` is the walk's start unit: it is
/// carried from one support call to the next within a test (as the game carries it in the caller's
/// record) and set to `HULL_CACHE_START` before each test (`cast_one`, `sweep_one`). Atomic, so the
/// world stays `Sync`.
pub struct HullLocal {
    /// 8 or 16: the packet's index width (the walk is the same for both).
    pub width: u8,
    pub block: gjk_shapes::HullBlock,
    cache: AtomicU32,
}

impl HullLocal {
    pub fn new(width: u8, block: gjk_shapes::HullBlock) -> HullLocal {
        HullLocal { width, block, cache: AtomicU32::new(HULL_CACHE_START) }
    }
}

impl LocalSupport for HullLocal {
    fn local_support(&self, v: Vec3) -> Vec3 {
        let (p, next) = gjk_shapes::hull_support(&self.block, v, self.cache.load(Ordering::Relaxed));
        self.cache.store(next, Ordering::Relaxed);
        p
    }

    /// [game] the hull ray tests (005cb8e0, 005cd1a0, 005cea60, 005d0320, token-identical) read
    /// Y and X in place with their own add orders (005cec94-005cedea in 005cea60):
    /// w = ((Y0 vx + Y6 vz) + Y3 vy, (Y7 vz + Y1 vx) + Y4 vy, (Y2 vx + Y5 vy) + Y8 vz) and
    /// o = (((X6 pz + X3 py) + X0 px) + X9, ((X4 py + X1 px) + X7 pz) + X10, ((X5 py + X8 pz) + X2 px) + X11).
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

/// Builds the walk's block from a decoded hull: `vertices` as x, y, z, w, and `faces` as each
/// face's stored edges `(index, link)`, its three corners in order (corner j of face k is pair 3k + j).
/// [data] `index` is a vertex index; [assumed, checked below] `link` is the signed step, in pairs from
/// the end of the pair, to the corner at the same vertex in the neighbouring face
/// (`docs/collision-support-hull.md`). Every link is checked here, as the walk follows it (the
/// wrap off the unused slot included): a link that misses, a face without three corners, or an index
/// past the vertices gives None, and the hull stays unsupported.
pub fn hull_block(vertices: Vec<[f32; 4]>, faces: &[Vec<(i64, i64)>]) -> Option<gjk_shapes::HullBlock> {
    if vertices.is_empty() || faces.is_empty() {
        return None;
    }
    let mut pairs: Vec<(u32, i32)> = Vec::with_capacity(3 * faces.len());
    for face in faces {
        if face.len() != 3 {
            return None;
        }
        for &(index, link) in face {
            if index < 0 || index as usize >= vertices.len() || link < i32::MIN as i64 || link > i32::MAX as i64 {
                return None;
            }
            pairs.push((index as u32, link as i32));
        }
    }
    let block = gjk_shapes::HullBlock { vertices, pairs };
    for p in 0..block.pairs.len() {
        // Pair p is corner j of face k: at unit 4k + 1 + j (unit 4k is the unused slot).
        let (k, j) = ((p / 3) as i64, (p % 3) as i64);
        let mut t = 4 * k + 1 + j + 1 + block.pairs[p].1 as i64;
        if t.rem_euclid(4) == 0 {
            t -= 3;
        }
        if t < 0 {
            return None;
        }
        let q = 3 * t.div_euclid(4) + (t.rem_euclid(4) - 1);
        if q < 0 || q as usize >= block.pairs.len() || block.pairs[q as usize].0 != block.pairs[p].0 {
            return None;
        }
    }
    Some(block)
}

/// The box of a hull: its vertices carried by the placement `x`, with the same row-vector sums as
/// `gjk_world`'s point_to_world [assumed: the hull's own transform is not read]. Widened by the caller.
fn hull_box(h: &Placed<HullLocal>) -> Aabb {
    let x = &h.x;
    let mut b = Aabb::empty();
    for v in &h.shape.block.vertices {
        let [px, py, pz, _] = *v;
        b.include(Vec3::new(
            ((x[6] * pz + x[3] * py) + x[0] * px) + x[9],
            ((x[7] * pz + x[1] * px) + x[4] * py) + x[10],
            ((x[8] * pz + x[2] * px) + x[5] * py) + x[11],
        ));
    }
    b
}

/// Sets the hull's walk to its start unit before a test (see `HULL_CACHE_START`).
fn reset_hull_cache(s: &Placed<HullLocal>) {
    s.shape.cache.store(HULL_CACHE_START, Ordering::Relaxed);
}

fn origin(x: &[f32; 12]) -> Vec3 {
    Vec3::new(x[9], x[10], x[11])
}

fn hit(index: usize, h: RayHit, triangle: Option<u32>) -> WorldHit {
    WorldHit { primitive: index, point: h.point, normal: h.normal, lambda: h.lambda, contact: h.contact, triangle }
}

fn cast_one(index: usize, shape: &WorldShape, p0: Vec3, p1: Vec3, hits: &mut Vec<WorldHit>) {
    match shape {
        // [game] the cube, cylinder, cone and hull ray tests have their own add orders
        // (`gjk_world::ray_test`, bit-exact on their ray files); the sphere's is 005bed70's family
        WorldShape::Cuboid(s) => {
            if let Some(h) = ray_test(p0, p1, s) {
                hits.push(hit(index, h, None));
            }
        }
        WorldShape::Sphere(s) => {
            if let Some(h) = gjk::ray_cast(p0, p1, s, origin(&s.x)) {
                hits.push(hit(index, h, None));
            }
        }
        WorldShape::Cylinder(s) => {
            if let Some(h) = ray_test(p0, p1, s) {
                hits.push(hit(index, h, None));
            }
        }
        WorldShape::Cone(s) => {
            if let Some(h) = ray_test(p0, p1, s) {
                hits.push(hit(index, h, None));
            }
        }
        WorldShape::Hull(s) => {
            reset_hull_cache(s);
            if let Some(h) = ray_test(p0, p1, s) {
                hits.push(hit(index, h, None));
            }
        }
        WorldShape::Mesh { soup, x } => {
            for h in soup.ray_cast(p0, p1, x) {
                hits.push(WorldHit {
                    primitive: index,
                    point: h.point,
                    normal: h.normal,
                    lambda: h.lambda,
                    contact: h.contact,
                    triangle: Some(h.triangle),
                });
            }
        }
        WorldShape::Unsupported { .. } => {}
    }
}

/// [game] the soup's triangle index mask (`collision_soup.rs` INDEX_MASK, private there, copied).
const SOUP_INDEX_MASK: u32 = 0x7ff_ffff;

fn sweep_one(index: usize, shape: &WorldShape, p0: Vec3, p1: Vec3, radius: f32, hits: &mut Vec<WorldHit>) {
    match shape {
        WorldShape::Cuboid(s) => {
            if let Some(h) = gjk_swept::sphere_cast(p0, p1, radius, s, origin(&s.x)) {
                hits.push(hit(index, h, None));
            }
        }
        WorldShape::Sphere(s) => {
            if let Some(h) = gjk_swept::sphere_cast(p0, p1, radius, s, origin(&s.x)) {
                hits.push(hit(index, h, None));
            }
        }
        WorldShape::Cylinder(s) => {
            if let Some(h) = gjk_swept::sphere_cast(p0, p1, radius, s, origin(&s.x)) {
                hits.push(hit(index, h, None));
            }
        }
        WorldShape::Cone(s) => {
            if let Some(h) = gjk_swept::sphere_cast(p0, p1, radius, s, origin(&s.x)) {
                hits.push(hit(index, h, None));
            }
        }
        WorldShape::Hull(s) => {
            reset_hull_cache(s);
            if let Some(h) = gjk_swept::sphere_cast(p0, p1, radius, s, origin(&s.x)) {
                hits.push(hit(index, h, None));
            }
        }
        WorldShape::Mesh { soup, x } => sweep_mesh(index, soup, x, p0, p1, radius, hits),
        WorldShape::Unsupported { .. } => {}
    }
}

/// [assumed] The game's sphere-against-soup driver is not read. Each triangle of the soup, placed by
/// `x`, is swept as a triangle grown by the radius (`gjk_swept::sphere_cast` per triangle), in the
/// soup's triangle order. The vertex placement is the one of `collision_soup.rs` (`transform_vertices`,
/// private there, copied here with the same two orderings), and the normal's orientation is the
/// soup driver's rule, so a mesh hit is reported with the same fields as a ray hit.
fn sweep_mesh(index: usize, soup: &Soup, x: &[f32; 12], p0: Vec3, p1: Vec3, radius: f32, hits: &mut Vec<WorldHit>) {
    let v = place_vertices(&soup.vertices, x);
    let seed = origin(x);
    for (ti, tri) in soup.triangles.iter().enumerate() {
        let idx = tri.map(|w| (w & SOUP_INDEX_MASK) as usize);
        if idx.iter().any(|&i| i >= v.len()) {
            continue;
        }
        let (a, b, c) = (v[idx[0]], v[idx[1]], v[idx[2]]);
        let Some(h) = gjk_swept::sphere_cast(p0, p1, radius, &Triangle { a, b, c }, seed) else { continue };
        // Face normal as collision_soup.rs: cross(e1, e2), normalised when longer than 1e-8,
        // then oriented toward the hit's normal.
        let (e1, e2) = (b - a, c - a);
        let mut n = Vec3::new(e1.y * e2.z - e1.z * e2.y, e1.z * e2.x - e2.z * e1.x, e2.y * e1.x - e1.y * e2.x);
        let ln = ((n.z * n.z + n.y * n.y) + n.x * n.x).sqrt();
        if ln > 1e-8 {
            n *= 1.0f32 / ln;
        }
        let dot = (h.normal.z * n.z + h.normal.y * n.y) + n.x * h.normal.x;
        let oriented = if dot > 0.0 { n } else { -n };
        hits.push(WorldHit {
            primitive: index,
            point: h.point,
            normal: oriented,
            lambda: h.lambda,
            contact: h.contact,
            triangle: Some(ti as u32),
        });
    }
}

/// [game] `collision_soup.rs` transform_vertices, copied: vertices 0 .. 4*(n/4) with the first
/// order, the rest with the second (the game's two loops round differently).
fn place_vertices(vertices: &[[f32; 4]], x: &[f32; 12]) -> Vec<Vec3> {
    let (p0, p1, p2, p3, p4, p5) = (x[0], x[1], x[2], x[3], x[4], x[5]);
    let (p6, p7, p8, p9, p10, p11) = (x[6], x[7], x[8], x[9], x[10], x[11]);
    let n4 = 4 * (vertices.len() / 4);
    vertices
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let [vx, vy, vz, w] = *v;
            if i < n4 {
                Vec3::new(
                    p0 * vx + p3 * vy + p6 * vz + p9 * w,
                    p1 * vx + p4 * vy + p7 * vz + p10 * w,
                    p2 * vx + p5 * vy + p8 * vz + p11 * w,
                )
            } else {
                Vec3::new(
                    ((p3 * vy + p6 * vz) + p9 * w) + p0 * vx,
                    ((p4 * vy + p7 * vz) + p10 * w) + p1 * vx,
                    ((p5 * vy + p8 * vz) + p11 * w) + p2 * vx,
                )
            }
        })
        .collect()
}

/// `first` then `then`: rows 0..2 = first.A * then.A; row 3 = first.t * then.A + then.t.
fn compose(first: &Mat, then: &Mat) -> Mat {
    let mut m = [[0.0f64; 3]; 4];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = (0..3).map(|k| first[i][k] * then[k][j]).sum();
        }
    }
    for j in 0..3 {
        m[3][j] = (0..3).map(|k| first[3][k] * then[k][j]).sum::<f64>() + then[3][j];
    }
    m
}

/// A stored 4x3 matrix (12 floats, rows of 3) as f64. None when the node is not 12 floats.
fn matrix_of(node: Node) -> Option<Mat> {
    let v = node.floats();
    if v.len() != 12 {
        return None;
    }
    let mut m = IDENTITY;
    for (r, row) in m.iter_mut().enumerate() {
        for (c, slot) in row.iter_mut().enumerate() {
            *slot = v[r * 3 + c] as f64;
        }
    }
    Some(m)
}

/// The bind of node `index` of `obj`, in the object's own space (absolute, see the module note).
/// Node 0 is the object's origin; node i > 0 is `sub_matrices[i - 1]`.
fn bind_of(obj: Node, index: i64) -> Mat {
    if index <= 0 {
        return IDENTITY;
    }
    obj.get("sub_matrices").and_then(|s| s.at((index - 1) as usize)).and_then(matrix_of).unwrap_or(IDENTITY)
}

/// The inverse of the 3x3 part, row-major, rounded to f32 (computed in f64). None when singular.
fn inverse3(a: &Mat) -> Option<[f32; 9]> {
    let m = [[a[0][0], a[0][1], a[0][2]], [a[1][0], a[1][1], a[1][2]], [a[2][0], a[2][1], a[2][2]]];
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let mut y = [0.0f32; 9];
    for i in 0..3 {
        for j in 0..3 {
            // Adjugate: inverse[i][j] = cofactor(j, i) / det, written with the cyclic indices.
            let cof = m[(j + 1) % 3][(i + 1) % 3] * m[(j + 2) % 3][(i + 2) % 3]
                - m[(j + 1) % 3][(i + 2) % 3] * m[(j + 2) % 3][(i + 1) % 3];
            y[i * 3 + j] = (cof / det) as f32;
        }
    }
    Some(y)
}

fn flat(m: &Mat) -> [f64; 12] {
    let mut out = [0.0f64; 12];
    for r in 0..4 {
        for c in 0..3 {
            out[r * 3 + c] = m[r][c];
        }
    }
    out
}

fn flat32(m: &Mat) -> [f32; 12] {
    flat(m).map(|v| v as f32)
}

/// The placed shape of one `Collision` primitive (`packet` is its typed shape packet).
fn world_shape(packet: Option<Node>, x: [f32; 12], y: [f32; 9]) -> WorldShape {
    let Some(packet) = packet else {
        return WorldShape::Unsupported { type_name: "(no packet)".to_owned() };
    };
    let num = |name: &str| packet.get(name).and_then(Node::float).unwrap_or(0.0);
    match packet.type_name().unwrap_or("") {
        "CollisionCuboidPacket" => {
            let e = packet.get("extent").map(Node::floats).unwrap_or_default();
            let at = |i: usize| e.get(i).copied().unwrap_or(0.0);
            WorldShape::Cuboid(Placed { shape: Cuboid { half: Vec3::new(at(0), at(1), at(2)) }, x, y })
        }
        "CollisionSpherePacket" => WorldShape::Sphere(gjk::Sphere { radius: num("radius"), x, y }),
        "CollisionCylinderPacket" => WorldShape::Cylinder(Placed {
            shape: gjk_shapes::Cylinder { r: num("radius"), h: num("height12") },
            x,
            y,
        }),
        "CollisionConePacket" => WorldShape::Cone(Placed {
            shape: gjk_shapes::Cone { r: num("radius"), h: num("height14"), a: num("height34"), c: num("angle") },
            x,
            y,
        }),
        "CollisionMeshPacket" => {
            let vertices: Vec<[f32; 4]> = packet
                .get("verts")
                .map(|n| {
                    n.items()
                        .map(|v| {
                            let f = v.floats();
                            [f.first().copied().unwrap_or(0.0), f.get(1).copied().unwrap_or(0.0), f.get(2).copied().unwrap_or(0.0), f.get(3).copied().unwrap_or(1.0)]
                        })
                        .collect()
                })
                .unwrap_or_default();
            let triangles: Vec<[u32; 3]> = packet
                .get("faces")
                .map(|n| {
                    n.items()
                        .map(|f| {
                            let w = f.ints();
                            std::array::from_fn(|i| w.get(i).copied().unwrap_or(0) as u32)
                        })
                        .collect()
                })
                .unwrap_or_default();
            WorldShape::Mesh { soup: Soup { vertices, triangles }, x }
        }
        name @ ("CollisionHull8Packet" | "CollisionHull16Packet") => {
            let width: u8 = if name == "CollisionHull8Packet" { 8 } else { 16 };
            let vertices: Vec<[f32; 4]> = packet
                .get("verts")
                .map(|n| {
                    n.items()
                        .map(|v| {
                            let f = v.floats();
                            [f.first().copied().unwrap_or(0.0), f.get(1).copied().unwrap_or(0.0), f.get(2).copied().unwrap_or(0.0), f.get(3).copied().unwrap_or(1.0)]
                        })
                        .collect()
                })
                .unwrap_or_default();
            let faces: Vec<Vec<(i64, i64)>> = packet
                .get("faces")
                .map(|n| {
                    n.items()
                        .map(|f| {
                            f.get("edges")
                                .map(|e| {
                                    e.items()
                                        .map(|edge| {
                                            (
                                                edge.get("index").and_then(Node::int).unwrap_or(0),
                                                edge.get("offset").and_then(Node::int).unwrap_or(0),
                                            )
                                        })
                                        .collect()
                                })
                                .unwrap_or_default()
                        })
                        .collect()
                })
                .unwrap_or_default();
            match hull_block(vertices, &faces) {
                Some(block) => WorldShape::Hull(Placed { shape: HullLocal::new(width, block), x, y }),
                None => WorldShape::Unsupported { type_name: format!("{name} (links fail hull_block)") },
            }
        }
        other => WorldShape::Unsupported { type_name: other.to_owned() },
    }
}

struct Sink<'a> {
    label: &'a str,
    /// The pack's place in the list it was loaded from, and its chunk index ([`Owner`]).
    pack: usize,
    chunk: usize,
    /// (Collision offset, quantised ACC, owner): one stored Collision once per distinct placement and Instance.
    seen: HashSet<(usize, [i64; 12], Option<Owner>)>,
    /// (node offset, its type, quantised ACC, released, the Object above, owner): see `visit`.
    visited: HashSet<(usize, usize, [i64; 12], bool, bool, usize, Option<Owner>)>,
    out: &'a mut Vec<WorldPrimitive>,
}

/// The walk: every node of the file, carrying the ACC of the placements above it, and the innermost Instance above
/// (`owner`, [`Owner`]).
#[allow(clippy::too_many_arguments)]
fn visit(node: Node, acc: Mat, obj: Option<Node>, owner: Option<Owner>, released: bool, placed: bool, depth: u32, sink: &mut Sink) {
    if depth > MAX_DEPTH {
        return;
    }
    // As `level_render::visit`: an Object is in the world only where an Instance puts it,
    // and the walk goes down through the containers only (`level_render::carries_placement`).
    let instance = node.get("matrix").and_then(matrix_of).filter(|_| node.get("things").is_some());
    let placed = placed || instance.is_some();
    let acc = instance.map_or(acc, |m| compose(&m, &acc));
    // [assumed] the innermost placed Instance owns what is under it.
    let owner = match (instance.is_some(), node.offset()) {
        (true, Some(at)) => Some((sink.pack, sink.chunk, at)),
        _ => owner,
    };
    if node.type_name() == Some("Collision") {
        if let (Some(obj), true) = (obj, placed) {
            record(node, obj, &acc, released, owner, sink);
        }
        return;
    }
    if !crate::level_render::carries_placement(node) {
        return;
    }
    // A typed node reached again at the same place, under the same Object and in the same
    // state, has nothing new under it. Without this a chunk whose objects share or loop
    // back to one another is walked without end (deep_six_01 chunk 13).
    if let (Some(name), Some(at)) = (node.type_name(), node.offset()) {
        let key = flat(&acc).map(|v| (v * KEY_SCALE).round() as i64);
        let under = obj.and_then(Node::offset).unwrap_or(usize::MAX);
        if !sink.visited.insert((at, name.as_ptr() as usize, key, released, placed, under, owner)) {
            return;
        }
    }
    let is_object = node.get("hierarchy").is_some() && node.get("attachments").is_some();
    let obj = if is_object { Some(node) } else { obj };
    if is_object {
        for attachment in node.get("attachments").into_iter().flat_map(Node::items) {
            let Some(thing) = attachment.get("thing") else { continue };
            if thing.type_name() == Some("Collision") {
                visit(thing, acc, obj, owner, released, placed, depth + 1, sink);
                continue;
            }
            let index = attachment.get("index").and_then(Node::int).unwrap_or(0);
            let child = if index > 0 { compose(&bind_of(node, index), &acc) } else { acc };
            let linked = released || matches!(thing.type_name(), Some("DamageLink" | "DetachLink"));
            visit(thing, child, obj, owner, linked, placed, depth + 1, sink);
        }
    }
    for (hash, child) in node.fields() {
        if matches!(node.file().name(hash), Some("hierarchy" | "sub_matrices" | "matrix" | "primitives" | "attachments")) {
            continue;
        }
        visit(child, acc, obj, owner, released, placed, depth + 1, sink);
    }
    if !node.is_empty() {
        for child in node.items() {
            visit(child, acc, obj, owner, released, placed, depth + 1, sink);
        }
    }
}

/// One stored Collision under `obj` at ACC, owned by `owner`: its primitives, placed.
fn record(col: Node, obj: Node, acc: &Mat, released: bool, owner: Option<Owner>, sink: &mut Sink) {
    let at = col.offset();
    if let Some(at) = at {
        let key = flat(acc).map(|v| (v * KEY_SCALE).round() as i64);
        if !sink.seen.insert((at, key, owner)) {
            return;
        }
    }
    let at = at.unwrap_or(0);
    let node = bind_of(obj, col.get("parent").and_then(Node::int).unwrap_or(0));
    let has_dynamics = col.get("dynamics").and_then(Node::type_name).is_some();
    for (qi, prim) in col.get("primitives").into_iter().flat_map(Node::items).enumerate() {
        let m = prim.get("matrix").and_then(matrix_of).unwrap_or(IDENTITY);
        let world = compose(&compose(&m, &node), acc);
        let x = flat32(&world);
        let source = format!("{} @ {at:#x} primitive {qi}", sink.label);
        let int = |name: &str| prim.get(name).and_then(Node::int).unwrap_or(0);
        let shape = match inverse3(&world) {
            Some(y) => world_shape(prim.get("packet"), x, y),
            // A singular placement has no inverse: counted as unsupported, never ray-tested.
            None => WorldShape::Unsupported { type_name: "degenerate placement".to_owned() },
        };
        sink.out.push(WorldPrimitive {
            shape,
            owner,
            source,
            material: int("material") as u32,
            mask_preset: int("maskPreset") as u32,
            send_mask: int("sendMask"),
            receive_mask: int("receiveMask"),
            has_dynamics,
            released,
            centre: Vec3::new(x[9], x[10], x[11]),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pencil_test_box(min: Vec3, max: Vec3) -> StaticWorld {
        let centre = (min+max)*0.5;
        StaticWorld::from_primitives(vec![WorldPrimitive {
            shape: WorldShape::Cuboid(Placed {shape: Cuboid {half: (max-min)*0.5},
                x: [1.,0.,0.,0.,1.,0.,0.,0.,1.,centre.x,centre.y,centre.z],
                y: [1.,0.,0.,0.,1.,0.,0.,0.,1.]}),
            owner: None, source: String::new(), material: 0, mask_preset: 0,
            send_mask: 0xffff, receive_mask: 0xffff, has_dynamics: false,
            released: false, centre,
        }])
    }

    #[test]
    fn pencil_sweep_stops_one_radius_before_a_vertical_face() {
        let world = pencil_test_box(Vec3::new(0.,-100.,-100.), Vec3::new(20.,100.,100.));
        let hits = world.pencil_cast(Vec3::new(-5.,0.,10.), Vec3::new(5.,0.,10.), 2.,5.4);
        let h = hits.first().expect("pencil hit");
        assert!((h.lambda-3.).abs()<0.1, "{h:?}");
        assert!(h.normal.dot(Vec3::NEG_X)>0.99, "{h:?}");
        assert!(h.point.x.abs()<0.1, "contact must be on the wall: {h:?}");
        assert!(h.point.y.abs()<0.1 && (10.0..=15.6).contains(&h.point.z), "witness should stay within the swept pencil span: {h:?}");
        assert!(!h.contact);
        assert!(world.pencil_cast(Vec3::new(-5.,0.,10.), Vec3::new(-3.,0.,10.),2.,5.4).is_empty());
    }

    #[test]
    fn pencil_sweep_preserves_a_convex_corner_contact_normal() {
        let world = pencil_test_box(Vec3::new(0.,0.,-100.), Vec3::new(20.,20.,100.));
        let from = Vec3::new(-3.,-3.,10.);
        let h = world.pencil_cast(from, Vec3::new(3.,3.,10.),2.,5.4).first().copied().expect("corner hit");
        let expected = Vec3::new(-1.,-1.,0.).normalize();
        assert!(h.normal.dot(expected)>0.99, "{h:?}");
        assert!((h.lambda-(from.truncate().length()-2.)).abs()<0.1, "{h:?}");
        assert!(h.point.truncate().length()<0.1, "corner contact: {h:?}");
        assert!((10.0..=15.6).contains(&h.point.z), "corner witness should remain within the swept pencil span: {h:?}");
    }

    #[test]
    fn pencil_cube_witness_matches_original_pair_cell_reference() {
        // Offline original reference: pair cell [6][2], H=5.4, r=1, cube
        // half-extents (0.5,100,100), cube centre (5,0,0), pencil origin
        // (0,0,0)->(10,0,0). Output P0=(4.4930053,0.0022002,-7.7e-8),
        // TOI=0.349301. The feet API is shifted so its derived pencil centre
        // matches the pair record's origin.
        let height = 5.4;
        let centre_offset = height * (63.0 / 121.0);
        let world = pencil_test_box(Vec3::new(4.5,-100.,-100.), Vec3::new(5.5,100.,100.));
        let h = world.pencil_cast(
            Vec3::new(0.,0.,-centre_offset), Vec3::new(10.,0.,-centre_offset), 1., height,
        ).into_iter().next().expect("original pair reference hits");
        assert!((h.lambda / 10.0 - 0.349301).abs() < 0.01, "{h:?}");
        assert!((h.point.x - 4.4930053).abs() < 0.02, "target witness x: {h:?}");
        assert!((h.point.y - 0.0022002).abs() < 0.02, "target witness y: {h:?}");
        assert!(h.point.z.abs() < 0.02, "target witness z: {h:?}");
    }

    #[test]
    fn pencil_cube_corner_witness_matches_original_pair_cell_reference() {
        // Same original pair cell and shape, with a diagonal sweep to a cube corner.
        // Original output P0=(4.5,4.5,0), TOI=0.379289.
        let height = 5.4;
        let centre_offset = height * (63.0 / 121.0);
        let world = pencil_test_box(Vec3::new(4.5,4.5,-100.), Vec3::new(5.5,5.5,100.));
        let h = world.pencil_cast(
            Vec3::new(0.,0.,-centre_offset), Vec3::new(10.,10.,-centre_offset), 1., height,
        ).into_iter().next().expect("original pair reference hits");
        assert!((h.lambda / (200.0f32).sqrt() - 0.379289).abs() < 0.01, "{h:?}");
        assert!((h.point.x - 4.5).abs() < 0.02 && (h.point.y - 4.5).abs() < 0.02, "target corner witness: {h:?}");
        assert!(h.point.z.abs() < 0.02, "target corner witness z: {h:?}");
    }
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// The install, when it is there. The tests skip (pass) when it is not.
    fn install_path(relative: &str) -> Option<PathBuf> {
        let dir = PathBuf::from(crate::install::dir_string());
        let path = dir.join(relative);
        path.is_file().then_some(path)
    }

    fn kind_name(shape: &WorldShape) -> String {
        match shape {
            WorldShape::Cuboid(_) => "Cuboid".into(),
            WorldShape::Sphere(_) => "Sphere".into(),
            WorldShape::Cylinder(_) => "Cylinder".into(),
            WorldShape::Cone(_) => "Cone".into(),
            WorldShape::Mesh { .. } => "Mesh".into(),
            WorldShape::Hull(h) => format!("Hull{}", h.shape.width),
            WorldShape::Unsupported { type_name } => {
                if type_name.contains("Hull8") {
                    "Hull8".into()
                } else if type_name.contains("Hull16") {
                    "Hull16".into()
                } else {
                    format!("Unsupported:{type_name}")
                }
            }
        }
    }

    #[test]
    fn compose_applies_first_then_second() {
        // A scale of 2 on x, then a translation of (1, 0, 0): the point (1, 1, 0) goes to (3, 1, 0).
        let mut scale = IDENTITY;
        scale[0][0] = 2.0;
        let mut shift = IDENTITY;
        shift[3][0] = 1.0;
        let m = compose(&scale, &shift);
        let p = [1.0f64, 1.0, 0.0];
        let q: [f64; 3] = std::array::from_fn(|j| (0..3).map(|k| p[k] * m[k][j]).sum::<f64>() + m[3][j]);
        assert!((q[0] - 3.0).abs() < 1e-12 && (q[1] - 1.0).abs() < 1e-12 && q[2].abs() < 1e-12, "{q:?}");
    }

    #[test]
    fn inverse3_round_trips() {
        let mut a = IDENTITY;
        a[0] = [2.0, 0.5, 0.0];
        a[1] = [0.1, 3.0, 0.2];
        a[2] = [0.0, -0.4, 1.5];
        let y = inverse3(&a).expect("invertible");
        for i in 0..3 {
            for j in 0..3 {
                let v: f32 = (0..3).map(|k| a[i][k] as f32 * y[k * 3 + j]).sum();
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((v - want).abs() < 1e-5, "A * inv(A) [{i}][{j}] = {v}");
            }
        }
        assert!(inverse3(&[[0.0; 3]; 4]).is_none());
    }

    /// Shanghai's zone: the decoder's counts of stored primitives (`collision.rs` tests, identity
    /// walk: 871 Collisions, 1791 primitives). Placed copies are more: one per distinct placement.
    #[test]
    fn shanghai_load_counts_and_bounds() {
        let Some(zone) = install_path(r"levels\shanghai_01\zone.str") else { return };
        let world = StaticWorld::load(&zone).unwrap();

        let mut stored: BTreeMap<String, usize> = BTreeMap::new();
        let mut placed: BTreeMap<String, usize> = BTreeMap::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for p in &world.primitives {
            let kind = kind_name(&p.shape);
            *placed.entry(kind.clone()).or_insert(0) += 1;
            if seen.insert(p.source.as_str()) {
                *stored.entry(kind).or_insert(0) += 1;
            }
        }
        println!("shanghai_01 zone.str: stored primitives by shape (distinct sources):");
        for (k, v) in &stored {
            println!("  {k:<22} stored {v:>5}  placed {:>6}", placed.get(k).copied().unwrap_or(0));
        }
        let total_stored: usize = stored.values().sum();
        let total_placed: usize = placed.values().sum();
        println!("  total                  stored {total_stored:>5}  placed {total_placed:>6}");
        assert_eq!(total_stored, 1753);
        assert_eq!(stored.get("Cuboid").copied(), Some(1571));
        assert_eq!(stored.get("Cylinder").copied(), Some(137));
        assert_eq!(stored.get("Sphere").copied(), Some(15));
        assert_eq!(stored.get("Mesh").copied(), Some(17));
        assert_eq!(stored.get("Cone").copied(), Some(3));
        assert_eq!(stored.get("Hull8").copied().unwrap_or(0) + stored.get("Hull16").copied().unwrap_or(0), 10);
        assert_eq!(world.primitives.iter().filter(|p| matches!(&p.shape, WorldShape::Unsupported { type_name } if type_name == "degenerate placement")).count(), 0);

        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in &world.primitives {
            for i in 0..3 {
                let v = [p.centre.x, p.centre.y, p.centre.z][i];
                lo[i] = lo[i].min(v);
                hi[i] = hi[i].max(v);
            }
        }
        println!("placed centres: x {:.1}..{:.1}  y {:.1}..{:.1}  z {:.2}..{:.2}", lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]);
        let missions = crate::formats::missionlist::MissionList::load(Path::new(&crate::install::dir_string())).unwrap();
        let m = missions
            .missions
            .iter()
            .find(|m| m.stream_files.iter().any(|s| s.eq_ignore_ascii_case("Levels/Shanghai_01/zone")) && m.name.as_deref() == Some("Shanghai_01_FreeRoam_A"))
            .expect("Shanghai_01_FreeRoam_A");
        println!("mission {:?} world_min {:?} world_max {:?}", m.name, m.world_min, m.world_max);
        if m.world_min == [0, 0, 0] && m.world_max == [0, 0, 0] {
            // missionlist.rs reads worldMin / worldMax as zeros for this mission (the list dump
            // has values): the bounds comparison is not made here. See docs/collision-world.md.
            println!("mission bounds read as zeros: the comparison is skipped (missionlist.rs suspect)");
            return;
        }
        let inside = world
            .primitives
            .iter()
            .filter(|p| (0..3).all(|i| {
                let v = [p.centre.x, p.centre.y, p.centre.z][i];
                v >= m.world_min[i] as f32 && v <= m.world_max[i] as f32
            }))
            .count();
        println!("placements whose centre is inside the mission bounds: {inside} of {total_placed}");
    }

    /// Downward rays over the mission's box, from 200 m above to 200 m below. Asserts only that some hit.
    #[test]
    fn shanghai_downward_grid() {
        let Some(zone) = install_path(r"levels\shanghai_01\zone.str") else { return };
        let world = StaticWorld::load(&zone).unwrap();
        const STEP: f32 = 20.0;
        let (x0, x1, y0, y1) = (-108.0f32, 602.0f32, -273.0f32, 428.0f32);
        let mut rays = 0usize;
        let mut hit_rays = 0usize;
        let mut z_hist: BTreeMap<i32, usize> = BTreeMap::new();
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        let mut x = x0;
        while x <= x1 {
            let mut y = y0;
            while y <= y1 {
                rays += 1;
                let p0 = Vec3::new(x, y, 200.0);
                let p1 = Vec3::new(x, y, -200.0);
                if let Some(h) = world.nearest(p0, p1) {
                    hit_rays += 1;
                    *z_hist.entry((h.point.z / 5.0).floor() as i32 * 5).or_insert(0) += 1;
                    *kinds.entry(kind_name(&world.primitives[h.primitive].shape)).or_insert(0) += 1;
                }
                y += STEP;
            }
            x += STEP;
        }
        println!("downward grid (step {STEP} m): {rays} rays, {hit_rays} hit something");
        println!("nearest hit z (5 m bins, lower edge: count):");
        for (k, v) in &z_hist {
            println!("  {k:>5}: {v}");
        }
        println!("nearest hit kinds: {kinds:?}");
        assert!(hit_rays > 0);
    }

    /// Hand check: an axis-aligned ground-like cuboid (half z about 4, centre z about -4), ray from
    /// above at its centre. The top face must be at centre z + half z (half-extent reading).
    #[test]
    fn shanghai_hand_check_ground_slab_top_face() {
        let Some(zone) = install_path(r"levels\shanghai_01\zone.str") else { return };
        let world = StaticWorld::load(&zone).unwrap();
        let mut chosen = None;
        for (i, p) in world.primitives.iter().enumerate() {
            let WorldShape::Cuboid(s) = &p.shape else { continue };
            let x = &s.x;
            let axis_aligned = x[2].abs() < 1e-5 && x[5].abs() < 1e-5 && x[6].abs() < 1e-5 && x[7].abs() < 1e-5 && (x[8] - 1.0).abs() < 1e-5;
            if p.released || p.has_dynamics || !axis_aligned {
                continue;
            }
            if (s.shape.half.z - 4.0).abs() < 0.1 && (x[11] + 4.0).abs() < 0.05 {
                chosen = Some(i);
                break;
            }
        }
        let i = chosen.expect("an axis-aligned slab with half z 4 at z -4");
        let p = &world.primitives[i];
        let WorldShape::Cuboid(s) = &p.shape else { unreachable!() };
        let top = s.x[11] + s.shape.half.z * s.x[8].abs();
        println!("slab {i}: {} centre ({:.3}, {:.3}, {:.3}) half {:?}; predicted top z {:.4}",
            p.source, s.x[9], s.x[10], s.x[11], s.shape.half, top);
        let c = Vec3::new(s.x[9], s.x[10], s.x[11]);
        let got = {
            let mut one = Vec::new();
            cast_one(i, &p.shape, c + Vec3::new(0.0, 0.0, 100.0), c - Vec3::new(0.0, 0.0, 100.0), &mut one);
            one.into_iter().min_by(|a, b| a.lambda.total_cmp(&b.lambda)).expect("the ray hits the slab")
        };
        println!("ray from above hit at z {:.4}, normal {:?}, lambda {:.3}", got.point.z, got.normal, got.lambda);
        assert!((got.point.z - top).abs() < 0.01, "hit z {} vs top {}", got.point.z, top);
        assert!((got.point.x - c.x).abs() < 0.01 && (got.point.y - c.y).abs() < 0.01);
    }

    /// A 64-bit LCG (no `rand` crate): the same segments every run.
    struct Lcg(u64);

    impl Lcg {
        /// A number in [lo, hi).
        fn range(&mut self, lo: f32, hi: f32) -> f32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let unit = ((self.0 >> 40) as f32) / (1u64 << 24) as f32;
            lo + (hi - lo) * unit
        }
    }

    /// The grid's `ray_cast` must return exactly what `ray_cast_brute` returns, for the same
    /// segments: same hits, same primitive, triangle and bit-identical lambda, same order.
    #[test]
    fn grid_matches_brute_force_shanghai() {
        let Some(zone) = install_path(r"levels\shanghai_01\zone.str") else { return };
        let world = StaticWorld::load(&zone).unwrap();
        let mut rng = Lcg(0x5eed_0001);
        let mut segs: Vec<(Vec3, Vec3)> = Vec::new();
        // 300 vertical rays, z 400 -> -100.
        for _ in 0..300 {
            let x = rng.range(-600.0, 800.0);
            let y = rng.range(-500.0, 650.0);
            segs.push((Vec3::new(x, y, 400.0), Vec3::new(x, y, -100.0)));
        }
        // 200 slanted segments, each at most 150 m (the end is pulled toward the start).
        for _ in 0..200 {
            let a = Vec3::new(rng.range(-600.0, 800.0), rng.range(-500.0, 650.0), rng.range(-20.0, 200.0));
            let b = Vec3::new(rng.range(-600.0, 800.0), rng.range(-500.0, 650.0), rng.range(-20.0, 200.0));
            let d = b - a;
            let len = d.length();
            let b = if len > 150.0 { a + d * (150.0 / len) } else { b };
            segs.push((a, b));
        }
        // 100 short horizontal segments of 10 m at z = 2.
        for _ in 0..100 {
            let x = rng.range(-600.0, 790.0);
            let y = rng.range(-500.0, 650.0);
            segs.push((Vec3::new(x, y, 2.0), Vec3::new(x + 10.0, y, 2.0)));
        }

        let t = std::time::Instant::now();
        let grid: Vec<Vec<WorldHit>> = segs.iter().map(|&(a, b)| world.ray_cast(a, b)).collect();
        let grid_time = t.elapsed();
        let t = std::time::Instant::now();
        let brute: Vec<Vec<WorldHit>> = segs.iter().map(|&(a, b)| world.ray_cast_brute(a, b)).collect();
        let brute_time = t.elapsed();
        println!("600 ray_cast calls: {grid_time:?}; 600 ray_cast_brute calls: {brute_time:?}");

        let mut total_hits = 0usize;
        for (si, (g, b)) in grid.iter().zip(brute.iter()).enumerate() {
            total_hits += b.len();
            assert_eq!(g.len(), b.len(), "segment {si} {:?}: hit count differs", segs[si]);
            for (gh, bh) in g.iter().zip(b.iter()) {
                let same = gh.primitive == bh.primitive && gh.triangle == bh.triangle && gh.lambda.to_bits() == bh.lambda.to_bits();
                assert!(
                    same,
                    "segment {si} {:?}: primitive {} ({}) vs brute primitive {} ({}), lambda {} vs {}",
                    segs[si],
                    gh.primitive,
                    kind_name(&world.primitives[gh.primitive].shape),
                    bh.primitive,
                    kind_name(&world.primitives[bh.primitive].shape),
                    gh.lambda,
                    bh.lambda
                );
            }
        }
        println!("grid and brute agree on {} segments, {total_hits} hits in all", segs.len());
    }

    /// A sphere of radius 1 dropped from z = 50 to z = -10 over (300, 300). The first contact is on
    /// the ground near z = 0, and the centre stops about 1 above it.
    #[test]
    fn shanghai_sphere_drop_stops_on_ground() {
        let Some(zone) = install_path(r"levels\shanghai_01\zone.str") else { return };
        let world = StaticWorld::load(&zone).unwrap();
        let p0 = Vec3::new(300.0, 300.0, 50.0);
        let p1 = Vec3::new(300.0, 300.0, -10.0);
        let hits = world.sphere_cast(p0, p1, 1.0);
        let h = hits.iter().min_by(|a, b| a.lambda.total_cmp(&b.lambda)).expect("the sphere hits something");
        let dir = (p1 - p0).normalize();
        let centre = p0 + dir * h.lambda;
        println!(
            "sphere r=1 drop at (300, 300): {} hits in all; nearest: primitive {} ({}), point {:?}, normal {:?}, lambda {:.5}, centre {:?}",
            hits.len(),
            h.primitive,
            kind_name(&world.primitives[h.primitive].shape),
            h.point,
            h.normal,
            h.lambda,
            centre
        );
        assert!(h.point.z.abs() < 0.05, "contact at z {} (wanted about 0)", h.point.z);
        assert!((centre.z - 1.0).abs() < 0.05, "centre stops at z {} (wanted about 1)", centre.z);
    }

    /// For 200 random segments, `sphere_cast` with radius 0.001 must hit the same primitives as
    /// `ray_cast` in at least 99% of them. Prints the mismatches.
    #[test]
    fn shanghai_sphere_cast_tiny_radius_matches_ray_cast() {
        let Some(zone) = install_path(r"levels\shanghai_01\zone.str") else { return };
        let world = StaticWorld::load(&zone).unwrap();
        let mut rng = Lcg(0x5eed_0002);
        let mut agree = 0usize;
        let mut mismatches: Vec<(usize, Vec<usize>, Vec<usize>)> = Vec::new();
        for si in 0..200 {
            let a = Vec3::new(rng.range(-600.0, 800.0), rng.range(-500.0, 650.0), rng.range(-20.0, 200.0));
            let b = Vec3::new(rng.range(-600.0, 800.0), rng.range(-500.0, 650.0), rng.range(-20.0, 200.0));
            let d = b - a;
            let len = d.length();
            let b = if len > 150.0 { a + d * (150.0 / len) } else { b };
            let mut ray: Vec<usize> = world.ray_cast(a, b).iter().map(|h| h.primitive).collect();
            ray.sort_unstable();
            ray.dedup();
            let mut swept: Vec<usize> = world.sphere_cast(a, b, 0.001).iter().map(|h| h.primitive).collect();
            swept.sort_unstable();
            swept.dedup();
            if ray == swept {
                agree += 1;
            } else {
                mismatches.push((si, ray, swept));
            }
        }
        println!("sphere_cast (radius 0.001) vs ray_cast: {agree} / 200 segments hit the same primitives");
        for (si, ray, swept) in mismatches.iter().take(30) {
            println!("  mismatch segment {si}: ray_cast primitives {ray:?}, sphere_cast primitives {swept:?}");
        }
        assert!(agree >= 198, "sphere_cast (radius 0.001) agrees with ray_cast on {agree} / 200 segments (below 99%)");
    }

    const REF_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../work/emu/ref/");

    /// For a hull that fails the support check: the largest plane violation (a convex hull has none,
    /// with the faces oriented outward) and the vertices whose corners do not form ONE fan under the
    /// links (a closed convex surface has one fan per vertex).
    fn hull_diagnosis(block: &gjk_shapes::HullBlock) -> String {
        let v: Vec<[f64; 3]> = block.vertices.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
        let c: [f64; 3] = std::array::from_fn(|i| v.iter().map(|q| q[i]).sum::<f64>() / v.len() as f64);
        let mut worst = f64::MIN;
        for k in 0..block.pairs.len() / 3 {
            let (a, b, d) = (v[block.pairs[3 * k].0 as usize], v[block.pairs[3 * k + 1].0 as usize], v[block.pairs[3 * k + 2].0 as usize]);
            let e1: [f64; 3] = std::array::from_fn(|i| b[i] - a[i]);
            let e2: [f64; 3] = std::array::from_fn(|i| d[i] - a[i]);
            let mut n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-30);
            let sign = if (0..3).map(|i| n[i] * (c[i] - a[i])).sum::<f64>() > 0.0 { -1.0 } else { 1.0 };
            n = n.map(|x| sign * x / len);
            for q in &v {
                worst = worst.max((0..3).map(|i| n[i] * (q[i] - a[i])).sum::<f64>());
            }
        }
        // Fans: follow each corner's link (the walk's own step, with the wrap) and count the cycles
        // per vertex.
        let np = block.pairs.len();
        let mut seen = vec![false; np];
        let mut cycles = vec![0usize; v.len()];
        for start in 0..np {
            if seen[start] {
                continue;
            }
            let vert = block.pairs[start].0 as usize;
            cycles[vert] += 1;
            let mut p = start;
            while !seen[p] {
                seen[p] = true;
                let mut t = 4 * (p / 3) as i64 + 1 + (p % 3) as i64 + 1 + block.pairs[p].1 as i64;
                if t.rem_euclid(4) == 0 {
                    t -= 3;
                }
                p = (3 * t.div_euclid(4) + (t.rem_euclid(4) - 1)) as usize;
            }
        }
        let multi: Vec<usize> = (0..v.len()).filter(|&i| cycles[i] != 1).collect();
        // Winding: as stored (no flip), a face whose normal points at the centre is inward.
        let mut inward = 0usize;
        // Links: a link from corner (a, face k, next vertex b) must land on a corner at a whose
        // previous vertex (in its face) is b, the neighbour across the edge a-b.
        let mut link_mismatch = 0usize;
        for k in 0..np / 3 {
            let idx: [usize; 3] = std::array::from_fn(|j| block.pairs[3 * k + j].0 as usize);
            let (a, b, d) = (v[idx[0]], v[idx[1]], v[idx[2]]);
            let e1: [f64; 3] = std::array::from_fn(|i| b[i] - a[i]);
            let e2: [f64; 3] = std::array::from_fn(|i| d[i] - a[i]);
            let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            if (0..3).map(|i| n[i] * (c[i] - a[i])).sum::<f64>() > 0.0 {
                inward += 1;
            }
            for j in 0..3 {
                let p = 3 * k + j;
                let next = block.pairs[3 * k + (j + 1) % 3].0;
                let t_unit = {
                    let mut t = 4 * k as i64 + 1 + j as i64 + 1 + block.pairs[p].1 as i64;
                    if t.rem_euclid(4) == 0 {
                        t -= 3;
                    }
                    t
                };
                let q = (3 * t_unit.div_euclid(4) + (t_unit.rem_euclid(4) - 1)) as usize;
                let (qk, qj) = (q / 3, q % 3);
                let prev_of_target = block.pairs[3 * qk + (qj + 2) % 3].0;
                if prev_of_target != next {
                    link_mismatch += 1;
                }
            }
        }
        format!(
            "{} vertices, {} faces, largest plane violation {worst:.3e}, vertices without one fan: {multi:?}, \
             faces wound inward (as stored): {inward}, links whose target's previous vertex is not this corner's next: {link_mismatch}",
            v.len(),
            np / 3
        )
    }

    fn hf(s: &str) -> f32 {
        f32::from_bits(u32::from_str_radix(s, 16).expect("hex word"))
    }

    /// Dot of a vertex (x, y, z) with a direction, in float64.
    fn dot64(p: [f32; 3], v: Vec3) -> f64 {
        p[0] as f64 * v.x as f64 + p[1] as f64 * v.y as f64 + p[2] as f64 * v.z as f64
    }

    /// The 26 directions: the 6 axes, the 8 cube corners and the 12 edge diagonals.
    fn directions26() -> Vec<Vec3> {
        let mut d = Vec::new();
        for s in [-1.0f32, 1.0] {
            d.push(Vec3::new(s, 0.0, 0.0));
            d.push(Vec3::new(0.0, s, 0.0));
            d.push(Vec3::new(0.0, 0.0, s));
        }
        for a in [-1.0f32, 1.0] {
            for b in [-1.0f32, 1.0] {
                for c in [-1.0f32, 1.0] {
                    d.push(Vec3::new(a, b, c));
                }
            }
        }
        for a in [-1.0f32, 1.0] {
            for b in [-1.0f32, 1.0] {
                d.push(Vec3::new(a, b, 0.0));
                d.push(Vec3::new(a, 0.0, b));
                d.push(Vec3::new(0.0, a, b));
            }
        }
        assert_eq!(d.len(), 26);
        d
    }

    /// Every stored hull of Shanghai's zone: its block builds (every link lands on a corner of the
    /// same vertex), and for each of the 26 directions and EVERY corner as the start, the walk's
    /// answer has the largest dot product over the vertices. Prints how many hulls pass.
    #[test]
    fn hull_support_is_farthest_on_real_hulls() {
        let Some(zone) = install_path(r"levels\shanghai_01\zone.str") else { return };
        let mut stored = 0usize;
        let mut built = 0usize;
        let mut passed = 0usize;
        let mut calls = 0usize;
        let mut bad_calls = 0usize;
        let mut failed_labels: Vec<String> = Vec::new();
        for c in crate::formats::collision::load(&zone).unwrap() {
            for p in &c.primitives {
                let crate::formats::collision::Shape::Hull { vertices, faces, .. } = &p.shape else { continue };
                stored += 1;
                let edges: Vec<Vec<(i64, i64)>> = faces.iter().map(|f| f.edges.clone()).collect();
                let Some(block) = hull_block(vertices.clone(), &edges) else {
                    println!("  hull {stored} ({}): hull_block refused it", c.object_path);
                    continue;
                };
                built += 1;
                let n_faces = block.pairs.len() / 3;
                let mut ok = true;
                let mut worst_gap = (0.0f64, String::new());
                for v in directions26() {
                    let want = block.vertices.iter().map(|q| dot64([q[0], q[1], q[2]], v)).fold(f64::MIN, f64::max);
                    let tol = 1e-6 * (1.0 + want.abs());
                    for k in 0..n_faces {
                        for j in 0..3 {
                            let start = (4 * k + 1 + j) as u32;
                            let (got, _) = gjk_shapes::hull_support(&block, v, start);
                            calls += 1;
                            let gap = want - dot64([got.x, got.y, got.z], v);
                            if gap > tol {
                                bad_calls += 1;
                                ok = false;
                                if gap > worst_gap.0 {
                                    worst_gap = (gap, format!("direction {v:?} start unit {start}: got {got:?}, want dot {want:.6}"));
                                }
                            }
                        }
                    }
                }
                if !ok {
                    println!("  worst gap {:.6e}: {}", worst_gap.0, worst_gap.1);
                    failed_labels.push(c.object_path.clone());
                }
                if ok {
                    passed += 1;
                } else {
                    println!("  hull {stored} ({}): support is NOT the farthest vertex in some case: {}", c.object_path, hull_diagnosis(&block));
                }
            }
        }
        println!(
            "real hulls (shanghai_01): {stored} stored, {built} built by hull_block, {passed} pass the farthest-vertex check \
             (26 directions x every start: {calls} calls, {bad_calls} not farthest)"
        );
        assert_eq!(stored, 10, "the level's stored hulls (6 Hull8, 4 Hull16)");
        assert_eq!(built, stored, "every stored hull builds");
        // The known failure (docs/collision-hull-placed.md): DirtPile_A_01 has a vertex in the middle
        // of a flat facet, all of whose neighbours lie in that plane, and the walk's strict tie rule
        // keeps it on that plateau for the straight-up direction. Any other failing hull is a test failure.
        const PLATEAU_HULLS: &[&str] = &["#715/DirtPile_A_01/things[0]"];
        let unexpected: Vec<&String> = failed_labels.iter().filter(|l| !PLATEAU_HULLS.iter().any(|k| l.contains(k))).collect();
        assert!(unexpected.is_empty(), "hulls that fail the support check, other than the known plateau hull: {unexpected:?}");
        assert_eq!(failed_labels.len(), PLATEAU_HULLS.len(), "the known plateau hull is the only failure");
        println!("  ({} hull(s) fail, the known plateau case; see docs/collision-hull-placed.md)", failed_labels.len());
        assert!(passed + failed_labels.len() == built);
    }

    /// The emulator's hull ray references (`work\emu\gen_refs_rayhull.py`, `ref\ray_shape_hull8.tsv`
    /// and `hull16`): `gjk::ray_cast` over the same hull block, placement and start unit, against
    /// the original ray tests. Asserts hit/miss agreement of at least 99%.
    fn check_hull_refs(width: u8, name: &str) {
        let path = format!("{REF_DIR}ray_shape_{name}.tsv");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                println!("note: {path} not found ({e}); run work\\emu\\gen_refs_rayhull.py. {name} skipped.");
                return;
            }
        };
        let (mut rows, mut agree, mut not_built) = (0usize, 0usize, 0usize);
        let (mut both_hit, mut contact_agree, mut ordinary) = (0usize, 0usize, 0usize);
        let (mut lam_ok, mut point_ok, mut normal_ok) = (0usize, 0usize, 0usize);
        let mut max_dl = 0.0f32;
        let mut exact = 0usize;
        let mut point_exact = 0usize;
        let mut bad_rows = Vec::new();
        for (row, line) in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).enumerate() {
            let mut it = line.split_whitespace();
            let mut word = || it.next().expect("a field");
            let p0 = Vec3::new(hf(word()), hf(word()), hf(word()));
            let p1 = Vec3::new(hf(word()), hf(word()), hf(word()));
            let nv: usize = word().parse().expect("nv");
            let vertices: Vec<[f32; 4]> = (0..nv).map(|_| [hf(word()), hf(word()), hf(word()), hf(word())]).collect();
            let np: usize = word().parse().expect("np");
            let pairs: Vec<(i64, i64)> = (0..np).map(|_| (word().parse().expect("index"), word().parse().expect("link"))).collect();
            let cache: u32 = word().parse().expect("cache");
            let xf: [f32; 12] = std::array::from_fn(|_| hf(word()));
            let yf: [f32; 9] = std::array::from_fn(|_| hf(word()));
            let ref_hit = word() == "1";
            let ref_contact = word() == "1";
            let ref_point = Vec3::new(hf(word()), hf(word()), hf(word()));
            let ref_normal = Vec3::new(hf(word()), hf(word()), hf(word()));
            let ref_lambda = hf(word());

            rows += 1;
            let faces: Vec<Vec<(i64, i64)>> = pairs.chunks(3).map(|c| c.to_vec()).collect();
            let Some(block) = hull_block(vertices, &faces) else {
                not_built += 1;
                bad_rows.push(row);
                continue;
            };
            let shape = HullLocal::new(width, block);
            shape.cache.store(cache, Ordering::Relaxed);
            let placed = Placed { shape, x: xf, y: yf };
            let ours = ray_test(p0, p1, &placed);
            if ours.is_some() == ref_hit {
                agree += 1;
            } else {
                bad_rows.push(row);
                println!(
                    "  row {row} disagrees: ours {:?}; emulator hit {ref_hit} contact {ref_contact} point {ref_point:?} lambda {ref_lambda}; cache {cache}",
                    ours.map(|h| (h.contact, h.lambda))
                );
            }
            if let (Some(h), true) = (ours, ref_hit) {
                both_hit += 1;
                if h.contact == ref_contact {
                    contact_agree += 1;
                }
                if !h.contact && !ref_contact {
                    ordinary += 1;
                    let dl = (h.lambda - ref_lambda).abs();
                    max_dl = max_dl.max(dl);
                    exact += (h.lambda.to_bits() == ref_lambda.to_bits()) as usize;
                    if dl <= 0.002 {
                        lam_ok += 1;
                    }
                    let dp = h.point - ref_point;
                    if dp.x.abs() <= 0.003 && dp.y.abs() <= 0.003 && dp.z.abs() <= 0.003 {
                        point_ok += 1;
                    }
                    point_exact += (h.point.to_array().map(f32::to_bits) == ref_point.to_array().map(f32::to_bits)) as usize;
                    let dn = h.normal - ref_normal;
                    if dn.x.abs() <= 0.02 && dn.y.abs() <= 0.02 && dn.z.abs() <= 0.02 {
                        normal_ok += 1;
                    }
                }
            }
        }
        let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
        println!("== hull{width} ray vs emulator ({name}): {rows} rows, {not_built} refused by hull_block");
        println!("  point bit-exact {point_exact} / {ordinary} ordinary hits");
        println!("  hit/miss agree {agree} / {rows} ({:.2}%); first disagreeing rows {:?}", pct(agree, rows), &bad_rows[..bad_rows.len().min(8)]);
        println!("  both hit {both_hit}; contact flag agrees {contact_agree} / {both_hit}");
        println!(
            "  ordinary agreed hits {ordinary}: |lambda diff| <= 2 mm {lam_ok} ({:.2}%), point <= 3 mm {point_ok} ({:.2}%), normal <= 0.02 {normal_ok} ({:.2}%); largest |lambda diff| {max_dl:e}; lambda bit-exact {exact} / {ordinary}",
            pct(lam_ok, ordinary),
            pct(point_ok, ordinary),
            pct(normal_ok, ordinary)
        );
        assert_eq!(agree, rows, "hull{width}: hit/miss disagreements");
        // [game] the hull ray tests' own add orders (`HullLocal::ray_world_support`, `gjk::INLINE`)
        assert_eq!(exact, ordinary, "hull{width}: lambda not bit-exact on every ordinary hit");
        // [game] the hull has no witness_support either, so its point is the ray's p0 + dir * lambda
        assert_eq!(point_exact, ordinary, "hull{width}: contact point not bit-exact on every ordinary hit");
    }

    #[test]
    fn hull8_ray_matches_emulator_refs() {
        check_hull_refs(8, "hull8");
    }

    #[test]
    fn hull16_ray_matches_emulator_refs() {
        check_hull_refs(16, "hull16");
    }

    /// Shanghai's hull placements: how many are placed and how many stay unsupported (was 64 hull
    /// placements, all unsupported). A ray down at the centre of each placed hull's box, from just
    /// above to just below it, hits that hull.
    #[test]
    fn shanghai_hull_placements_are_placed_and_hit() {
        let Some(zone) = install_path(r"levels\shanghai_01\zone.str") else { return };
        let world = StaticWorld::load(&zone).unwrap();
        let mut placed_hulls = 0usize;
        let mut unsupported_total = 0usize;
        let mut unsupported_hull = 0usize;
        let mut centre_hits = 0usize;
        let mut centre_miss = Vec::new();
        for (i, p) in world.primitives.iter().enumerate() {
            match &p.shape {
                WorldShape::Hull(_) => {
                    placed_hulls += 1;
                    let b = primitive_box(&p.shape);
                    let c = (b.min + b.max) * 0.5;
                    let mut one = Vec::new();
                    cast_one(i, &p.shape, Vec3::new(c.x, c.y, b.max.z + 1.0), Vec3::new(c.x, c.y, b.min.z - 1.0), &mut one);
                    if one.iter().any(|h| h.primitive == i) {
                        centre_hits += 1;
                    } else {
                        centre_miss.push((i, p.source.clone(), c));
                    }
                }
                WorldShape::Unsupported { type_name } => {
                    unsupported_total += 1;
                    if type_name.contains("Hull") {
                        unsupported_hull += 1;
                    }
                }
                _ => {}
            }
        }
        println!(
            "shanghai_01: hull placements now ray-tested {placed_hulls}; Unsupported placements {unsupported_total} of which hulls {unsupported_hull} (was 64 hull placements)"
        );
        println!("  box-centre downward rays hitting their own hull: {centre_hits} / {placed_hulls}");
        for (i, src, c) in centre_miss.iter().take(10) {
            println!("  miss: primitive {i} ({src}) centre {c:?}");
        }
        assert_eq!(unsupported_hull, 0, "hull placements left unsupported");
        assert!(centre_miss.is_empty(), "{} hull placements not hit by a ray at their box centre", centre_miss.len());
    }
}
