//! The navigation mesh the game builds at mission load from its `NavSilhouette` objects.
//!
//! Read from the game's code (`notes\ai-nav-build.md`, the three reads, the third one
//! corrects the earlier ones): each silhouette outline is inserted into one triangulation of
//! the level's box, vertex by vertex (point location, 1-to-3 or 2-to-4 splits, edge flips),
//! then each outline segment is inserted as a constraint edge (marked cells). After every
//! outline is in, each silhouette sets the floor, ceiling and extra height of the triangles
//! inside it. Triangles are clockwise. Indices are used instead of pointers.
//!
//! Tags used below: `[game]` read in code, `[data]` from the install, `[assumed]` a guess
//! (see `notes\status.md` "Phase 3, navigation mesh"), `[stand-in]` a placeholder.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use glam::Vec3;

use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};
use crate::level_render::{flat32, Mat};
use crate::level_things::each_thing_placement;

/// `Silhouette::flags` bit: `canNotClimb` (loader packing, [game] ai-nav-build "third read" 1).
pub const FLAG_CAN_NOT_CLIMB: u32 = 1;
/// `Silhouette::flags` bit: `canNotLand`.
pub const FLAG_CAN_NOT_LAND: u32 = 2;
/// `Silhouette::flags` bit: `isALayerBoundary`.
pub const FLAG_LAYER_BOUNDARY: u32 = 8;

const FLT_MAX: f32 = f32::MAX;
/// Vertices this close are one vertex. [game] 006fb450: 5 mm.
const VERTEX_REUSE: f64 = 0.005;
/// A point whose distance to an edge's line is below this is on the edge. [game] 006fb450: the
/// line distance |cross(P-A, E)| / |E| < 0.005 m, with no projection-extent test.
const EDGE_LINE_EPS: f64 = 0.005;
/// Tight on-edge band (20 micrometres). Used only by the fallback in `insert_vertex` when the
/// game's 5 mm rule gives a split that inverts a triangle. [assumed] (the fallback is ours).
const TIGHT_EDGE_EPS: f64 = 2e-5;
/// Outline points within this distance (2D) of the chord of their neighbours are dropped.
/// [game] loader 006f9b20 line 99: 1 mm.
const CHORD_DROP: f64 = 0.001;
/// A piece of constraint shorter than this (squared) is not inserted; the existing edge is
/// marked instead. [game] 006fcdd0 lines 173 and 177: 0.001 m^2, i.e. 0.0316 m.
const MIN_PIECE_D2: f64 = 0.001;
/// Below this |denominator| the crossing has no hit. [game] 006fabb0 line 87: 0.001.
const CROSS_DEN_MIN: f64 = 0.001;
/// A ring segment whose length squared is at or below this is not constrained. [game]
/// 006fcdd0 lines 102-103 (squared distance <= 0.05, 0.224 m): the pair is not linked. The
/// port tests it before the walk; the game tests it after its ring-cell search [assumed].
const SHORT_PAIR_D2: f64 = 0.05;
/// Margin added around the silhouettes when the caller gives no bounds. [assumed] 10 m.
const BOUNDS_MARGIN: f32 = 10.0;
/// Bit 0x40 of a triangle's flags: `cross(B-A, C-A) < 0.1` ([game] 006fa960).
const FLAG_WINDING: u8 = 0x40;

/// One silhouette (`NavSilhouette` object) as the loader leaves it. [data] field names
/// `outline`, `height`, `replacementHeight`, `aiBanHeight`, `extraHeight`, `pivotHeight`,
/// `canNotClimb`, `canNotLand`, `canNotWalkOff`, `isALayerBoundary`, `impermanent`,
/// `treat_as_dynamic` (ai-nav-build "third read" 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Silhouette {
    /// Where it came from: pack, chunk, offset, and the placing Instance's label.
    pub name: String,
    /// World-space outline, closed by repeating the first point, at least 4 entries.
    /// Points within 1 mm (2D) of their neighbours' chord are dropped ([game], loader).
    pub points: Vec<[f32; 3]>,
    /// Translation z of the owner's placement, excluding the attachment bind.
    /// [game] 006f46d0 reads owner+0x3c; 006f9b20 discards the bind's z.
    pub base_z: f32,
    /// Top above the bottom (bottom = `base_z - pivot`). [game] 0 becomes FLT_MAX (setter 00701630).
    pub height: f32,
    /// [data] read and stored by the loader, not used by the mesh build.
    pub replacement_height: f32,
    /// Limit above the bottom. [game] 0 becomes FLT_MAX.
    pub ai_ban_height: f32,
    /// Copied into the triangles' extra height when the silhouette sets the floor. [game]
    pub extra_height: f32,
    /// `pivotHeight`: the bottom is `base_z - pivot`. [game] 006f46d0.
    pub pivot: f32,
    /// `FLAG_*` bits. Copied into the triangles' flags bits 0 to 1 by the height flood.
    pub flags: u32,
    /// [data] read and discarded by the loader (line 230), kept here for the record.
    pub can_not_walk_off: bool,
    /// [data] read by the loader; its use in the graph was not found.
    pub treat_as_dynamic: bool,
    /// [data] read by the loader; its use was not found.
    pub impermanent: bool,
    /// Number of outline points as the data has them (including the repeated first point).
    pub raw_points: usize,
    /// The data chunk and the object's offset in it: one object placed twice has two
    /// silhouettes with the same key ([data], `each_thing` walks every placement).
    pub object: (usize, usize),
}

impl Silhouette {
    /// Signed area of the outline (shoelace), positive when counter-clockwise (x right, y forward).
    pub fn signed_area(&self) -> f64 {
        signed_area(&self.points)
    }

    pub fn is_clockwise(&self) -> bool {
        self.signed_area() < 0.0
    }
}

fn signed_area(points: &[[f32; 3]]) -> f64 {
    let n = points.len().saturating_sub(1);
    let mut sum = 0.0;
    for i in 0..n {
        let (a, b) = (points[i], points[i + 1]);
        sum += a[0] as f64 * b[1] as f64 - b[0] as f64 * a[1] as f64;
    }
    sum * 0.5
}

/// Applies the object matrix to one outline point, the way the loader does ([game] lines 43-46):
/// `x' = x*m0 + y*m3 + z*m6 + m9`, `y' = x*m1 + y*m4 + z*m7 + m10`, `z' = x*m2 + y*m5 + z*m8`.
/// The attachment translation z (`m11`) is not applied ([game] lines 74-77 and 101-103).
/// It is not the silhouette's `base_z`: that comes from the separate owner placement.
pub fn transform_point(p: [f32; 3], m: &[f32; 12]) -> [f32; 3] {
    let (x, y, z) = (p[0], p[1], p[2]);
    [
        x * m[0] + y * m[3] + z * m[6] + m[9],
        x * m[1] + y * m[4] + z * m[7] + m[10],
        x * m[2] + y * m[5] + z * m[8],
    ]
}

/// The loader's outline rules: at least 4 points with the first repeated at the end, the
/// matrix applied, and points within 1 mm (2D) of the chord of their neighbours dropped. The
/// first point is never dropped. [game] for the rules; [assumed] that neighbours are the
/// original (not the kept) points, and that the first point is kept.
pub fn outline_in_world(raw: &[[f32; 3]], m: &[f32; 12]) -> Result<Vec<[f32; 3]>, String> {
    if raw.len() < 4 {
        return Err(format!("outline has {} points, needs at least 4", raw.len()));
    }
    if raw[0] != raw[raw.len() - 1] {
        return Err("outline is not closed by repeating its first point".to_owned());
    }
    let world: Vec<[f32; 3]> = raw[..raw.len() - 1].iter().map(|&p| transform_point(p, m)).collect();
    let n = world.len();
    let mut out = vec![world[0]];
    for i in 1..n {
        let (prev, next) = (world[(i + n - 1) % n], world[(i + 1) % n]);
        if dist_to_segment_2d(world[i], prev, next) > CHORD_DROP {
            out.push(world[i]);
        }
    }
    if out.len() < 3 {
        return Err(format!("outline has {} distinct points after the chord rule, needs 3", out.len()));
    }
    out.push(out[0]);
    Ok(out)
}

fn dist_to_segment_2d(p: [f32; 3], a: [f32; 3], b: [f32; 3]) -> f64 {
    let (px, py) = (p[0] as f64, p[1] as f64);
    let (ax, ay) = (a[0] as f64, a[1] as f64);
    let (bx, by) = (b[0] as f64, b[1] as f64);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

fn get_float(thing: Node<'_>, name: &str) -> f32 {
    thing.get(name).and_then(Node::float).unwrap_or(0.0)
}

fn get_flag(thing: Node<'_>, name: &str) -> bool {
    thing.get(name).is_some_and(|n| n.is_true() || n.int().is_some_and(|v| v != 0))
}

/// The setter's rule: a zero height or limit means no height or limit ([game] 00701630).
fn effective(v: f32) -> f32 {
    if v == 0.0 { FLT_MAX } else { v }
}

/// [game] 006f9b20 writes baked outline z = 0 (006fa0b0..006fa160), then
/// 006f46d0 applies the owner matrix. Its height scale is the length of the
/// owner's third row (006f47a2..006f484b), not the attachment's scale.
fn placed_outline(raw: &[[f32; 3]], bind: &Mat, placement: &Mat) -> Result<(Vec<[f32; 3]>, f32, f32), String> {
    let mut points = outline_in_world(raw, &flat32(bind))?;
    let owner = flat32(placement);
    for point in &mut points {
        point[2] = 0.0;
        *point = transform_point(*point, &owner);
        point[2] += owner[11];
    }
    let scale = (owner[6] * owner[6] + owner[7] * owner[7] + owner[8] * owner[8]).sqrt();
    Ok((points, owner[11], scale))
}

/// An open height stays FLT_MAX even when the owner's vertical scale is zero.
/// [game] 006f46d0 tests the sentinel before multiplying (006f48cb..006f48e9).
fn placed_height(value: f32, scale: f32) -> f32 {
    let value = effective(value);
    if value == FLT_MAX { FLT_MAX } else { value * scale }
}

/// Every `NavSilhouette` of one chunk, in the order the data has them ([data] `each_thing`,
/// the same walk the spawn points use). Problems go into `skipped`.
pub fn read_silhouettes(file: &DataFile, skipped: &mut Vec<String>) -> Vec<Silhouette> {
    let mut out = Vec::new();
    each_thing_placement(file, &["NavSilhouette"], &mut |thing, bind, placement, instance| {
        let at = thing.offset().unwrap_or(0);
        let label = instance.map(|i| format!("{i}:")).unwrap_or_default();
        let name = format!("{label}NavSilhouette@{at:#x}");
        let Some(list) = thing.get("outline") else {
            skipped.push(format!("{name}: no outline"));
            return;
        };
        let floats: Vec<Vec<f32>> = list.items().map(|p| p.floats()).collect();
        if floats.iter().any(|f| f.len() != 3) {
            skipped.push(format!("{name}: an outline point is not 3 floats"));
            return;
        }
        let raw: Vec<[f32; 3]> = floats.iter().map(|f| [f[0], f[1], f[2]]).collect();
        let (points, base_z, scale) = match placed_outline(&raw, &bind, &placement) {
            Ok(placed) => placed,
            Err(e) => {
                skipped.push(format!("{name}: {e}"));
                return;
            }
        };
        let mut flags = 0;
        if get_flag(thing, "canNotClimb") {
            flags |= FLAG_CAN_NOT_CLIMB;
        }
        if get_flag(thing, "canNotLand") {
            flags |= FLAG_CAN_NOT_LAND;
        }
        if get_flag(thing, "isALayerBoundary") {
            flags |= FLAG_LAYER_BOUNDARY;
        }
        out.push(Silhouette {
            name,
            points,
            base_z,
            height: placed_height(get_float(thing, "height"), scale),
            replacement_height: get_float(thing, "replacementHeight"),
            ai_ban_height: placed_height(get_float(thing, "aiBanHeight"), scale),
            extra_height: get_float(thing, "extraHeight"),
            pivot: get_float(thing, "pivotHeight") * scale,
            flags,
            can_not_walk_off: get_flag(thing, "canNotWalkOff"),
            treat_as_dynamic: get_flag(thing, "treat_as_dynamic"),
            impermanent: get_flag(thing, "impermanent"),
            raw_points: raw.len(),
            object: (0, at),
        });
    });
    out
}

/// Every `NavSilhouette` of a pack (its data chunks, in pack order), and the problems met.
/// `path` is a pack such as `levels\us_test\zone.str`.
pub fn load(path: &Path) -> Result<(Vec<Silhouette>, Vec<String>), String> {
    let pack = Pack::open(path)?;
    let file_name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for chunk in pack.of_type(pack::DATA) {
        let data = pack.data(chunk);
        if !data.windows(b"NavSilhouette".len()).any(|w| w == b"NavSilhouette") {
            continue;
        }
        let file = DataFile::parse(data.to_vec()).map_err(|e| format!("chunk {}: {e}", chunk.index))?;
        let before = out.len();
        out.extend(read_silhouettes(&file, &mut skipped));
        for s in &mut out[before..] {
            s.name = format!("{file_name}#{}:{}", chunk.index, s.name);
            s.object.0 = chunk.index as usize;
        }
    }
    Ok((out, skipped))
}

// ---------------------------------------------------------------------------------------------
// Exact-enough 2D geometry on f64 (the vertices are f32 rounded to 1 mm, so products are exact).

type P2 = [f64; 2];

fn sub(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}

fn dot(a: P2, b: P2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross(a: P2, b: P2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// Twice the signed area of (a, b, c): negative when clockwise.
fn orient(a: P2, b: P2, c: P2) -> f64 {
    cross(sub(b, a), sub(c, a))
}

fn d2(a: P2, b: P2) -> f64 {
    let d = sub(a, b);
    dot(d, d)
}

fn d2_3(a: [f32; 3], b: [f32; 3]) -> f64 {
    d2(plane(a), plane(b))
}

fn plane(v: [f32; 3]) -> P2 {
    [v[0] as f64, v[1] as f64]
}

fn dist(a: P2, b: P2) -> f64 {
    d2(a, b).sqrt()
}

fn grid_key(q: [f32; 3]) -> (i64, i64) {
    ((q[0] as f64 / VERTEX_REUSE).floor() as i64, (q[1] as f64 / VERTEX_REUSE).floor() as i64)
}

/// The incircle determinant: positive when `d` is inside the circle through `a`, `b`, `c`
/// when they are counter-clockwise (Shewchuk's sign convention).
fn incircle_det(a: P2, b: P2, c: P2, d: P2) -> f64 {
    let (adx, ady) = (a[0] - d[0], a[1] - d[1]);
    let (bdx, bdy) = (b[0] - d[0], b[1] - d[1]);
    let (cdx, cdy) = (c[0] - d[0], c[1] - d[1]);
    let abdet = adx * bdy - bdx * ady;
    let bcdet = bdx * cdy - cdx * bdy;
    let cadet = cdx * ady - adx * cdy;
    let alift = adx * adx + ady * ady;
    let blift = bdx * bdx + bdy * bdy;
    let clift = cdx * cdx + cdy * cdy;
    alift * bcdet + blift * cadet + clift * abdet
}

/// Whether `p` is strictly inside the circumcircle of `a`, `b`, `c` in either orientation.
fn inside_circle(p: P2, a: P2, b: P2, c: P2) -> bool {
    let det = incircle_det(a, b, c, p);
    if orient(a, b, c) > 0.0 { det > 0.0 } else { det < 0.0 }
}

/// Even-odd (crossing number) point in polygon test in x and y. [game] 007017e0: an edge
/// counts when its y ends are on opposite sides of the point (half-open) and the x intercept
/// is to the right of the point; the count's low bit is the answer.
fn point_in_polygon(p: P2, poly: &[P2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// The outline as a polygon: the points without the repeated last one, in f64.
fn polygon_of(points: &[[f32; 3]]) -> Vec<P2> {
    let mut n = points.len();
    if n >= 2 && points[0] == points[n - 1] {
        n -= 1;
    }
    points[..n].iter().map(|&p| plane(p)).collect()
}

// ---------------------------------------------------------------------------------------------
// The mesh.

/// One directed side of a triangle: the cell at `origin`, running to the next cell's origin.
/// `twin` is the cell of the neighbouring triangle that runs the other way, `None` on the
/// box's boundary. `marked` is a constraint (silhouette) edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub origin: usize,
    pub marked: bool,
    pub twin: Option<(usize, usize)>,
}

#[derive(Clone, Debug)]
struct Tri {
    cells: [Cell; 3],
    floor: f32,
    ceiling: f32,
    extra: f32,
    flags: u8,
    incenter: [f32; 2],
    alive: bool,
}

#[derive(Clone, Copy)]
struct NewTri {
    corners: [usize; 3],
    marked: [bool; 3],
    parent: usize,
}

#[derive(Clone, Copy)]
enum Loc {
    Inside(usize),
    Edge(usize, usize),
    Outside,
}

enum Cross {
    Vertex(usize),
    Edge { tri: usize, k: usize, x: [f32; 3] },
    Nothing,
}

/// The triangulated navigation mesh. Triangles are indexed from 0 in creation order; a
/// triangle that a split or flip replaced is dead and skipped by every query.
pub struct NavMesh {
    min: Vec3,
    max: Vec3,
    verts: Vec<[f32; 3]>,
    vert_cell: Vec<(usize, usize)>,
    grid: HashMap<(i64, i64), Vec<usize>>,
    tris: Vec<Tri>,
    hint: usize,
    /// Problems that did not stop the build (degenerate outlines and the like).
    pub warnings: Vec<String>,
    /// Constraint pieces not inserted: the walk found no crossing for them.
    pub skipped_pieces: usize,
    /// Constraint pieces shorter than 0.0316 m: no vertex is made; the crossed edge is marked
    /// and the walk goes on from its far end ([game] 006fcdd0 path at 006fd218; the far-end
    /// choice is [assumed]).
    pub snapped_pieces: usize,
    /// Ring segments not constrained by the short-pair rule.
    pub short_pairs: usize,
    /// Vertex insertions where the game's 5 mm on-edge rule gave a split that inverted a
    /// triangle, so the tight 20 micrometre band was used for that insertion ([assumed]
    /// fallback; see `notes\status.md`).
    pub tight_fallbacks: usize,
}

impl NavMesh {
    /// The seed: the box split into two clockwise triangles along the diagonal from its
    /// minimum to its maximum corner in the 2D sense of the note. [game] 006ef2c0: corners
    /// `v0 = (minx, miny)`, `v1 = (minx, maxy)`, `v2 = (maxx, maxy)`, `v3 = (maxx, miny)`,
    /// z = FLT_MAX; the box sides are marked; floor = box min z; ceiling = FLT_MAX.
    pub fn new(min: Vec3, max: Vec3) -> NavMesh {
        let mut m = NavMesh {
            min,
            max,
            verts: Vec::new(),
            vert_cell: Vec::new(),
            grid: HashMap::new(),
            tris: Vec::new(),
            hint: 0,
            warnings: Vec::new(),
            skipped_pieces: 0,
            snapped_pieces: 0,
            short_pairs: 0,
            tight_fallbacks: 0,
        };
        let v0 = m.add_vertex([min.x, min.y, FLT_MAX]);
        let v1 = m.add_vertex([min.x, max.y, FLT_MAX]);
        let v2 = m.add_vertex([max.x, max.y, FLT_MAX]);
        let v3 = m.add_vertex([max.x, min.y, FLT_MAX]);
        let cell = |origin, marked, twin| Cell { origin, marked, twin };
        let root = |cells| Tri { cells, floor: min.z, ceiling: FLT_MAX, extra: 0.0, flags: 0, incenter: [0.0; 2], alive: true };
        m.tris = vec![
            root([cell(v0, true, None), cell(v1, true, None), cell(v2, false, Some((1, 2)))]),
            root([cell(v2, true, None), cell(v3, true, None), cell(v0, false, Some((0, 2)))]),
        ];
        m.vert_cell[v0] = (0, 0);
        m.vert_cell[v1] = (0, 1);
        m.vert_cell[v2] = (0, 2);
        m.vert_cell[v3] = (1, 1);
        for t in 0..2 {
            let corners = [m.tris[t].cells[0].origin, m.tris[t].cells[1].origin, m.tris[t].cells[2].origin];
            m.tris[t].incenter = m.incenter_of(corners);
            m.set_winding(t);
        }
        m
    }

    /// Builds the mesh of a level: every outline inserted, then every silhouette's heights.
    /// `bounds` is the level box. The game takes min and max from the mission record at
    /// +0xfc and +0x108 ([game] 00756190, ai-nav-build fourth read 5; the field names are
    /// NOT FOUND, so the caller must pass them). `None` takes the silhouettes' box plus a 10 m
    /// margin ([assumed] fallback). An error names the silhouette whose walk did not finish.
    pub fn build(bounds: Option<(Vec3, Vec3)>, sils: &[Silhouette]) -> Result<NavMesh, String> {
        let (min, max) = match bounds {
            Some(b) => b,
            None => {
                let mut lo = [FLT_MAX; 3];
                let mut hi = [-FLT_MAX; 3];
                for s in sils {
                    for p in &s.points {
                        for i in 0..2 {
                            lo[i] = lo[i].min(p[i]);
                            hi[i] = hi[i].max(p[i]);
                        }
                    }
                    lo[2] = lo[2].min(s.base_z - s.pivot);
                    hi[2] = hi[2].max(s.base_z);
                }
                if sils.is_empty() {
                    lo = [-1.0, -1.0, 0.0];
                    hi = [1.0, 1.0, 0.0];
                }
                let m = Vec3::splat(BOUNDS_MARGIN);
                (Vec3::from(lo) - m, Vec3::from(hi) + m)
            }
        };
        let finite = [min, max].iter().all(|v| v.is_finite());
        if !finite || max.x <= min.x || max.y <= min.y {
            return Err(format!("bounds {min:?} .. {max:?} are empty or not finite"));
        }
        let mut mesh = NavMesh::new(min, max);
        for s in sils {
            mesh.insert_outline(&s.points, &s.name)?;
        }
        for s in sils {
            mesh.apply_silhouette(s)?;
        }
        Ok(mesh)
    }

    // ----- queries -------------------------------------------------------------------------

    pub fn bounds(&self) -> (Vec3, Vec3) {
        (self.min, self.max)
    }

    pub fn vertex_count(&self) -> usize {
        self.verts.len()
    }

    pub fn vertex(&self, i: usize) -> [f32; 3] {
        self.verts[i]
    }

    /// The live triangles, in index order.
    pub fn triangles(&self) -> impl Iterator<Item = usize> + '_ {
        self.tris.iter().enumerate().filter(|(_, t)| t.alive).map(|(i, _)| i)
    }

    pub fn tri_count(&self) -> usize {
        self.tris.iter().filter(|t| t.alive).count()
    }

    /// The directed cells that are marked (each constraint edge counts twice, once per side).
    pub fn marked_cells(&self) -> usize {
        self.triangles().map(|t| self.tris[t].cells.iter().filter(|c| c.marked).count()).sum()
    }

    /// The vertex indices of a triangle, in cell order (clockwise).
    pub fn tri_vertices(&self, tri: usize) -> [usize; 3] {
        [self.corner(tri, 0), self.corner(tri, 1), self.corner(tri, 2)]
    }

    /// The triangle across `edge` (edge k runs from cell k's origin to cell k+1's origin).
    pub fn neighbour(&self, tri: usize, edge: usize) -> Option<usize> {
        self.tris[tri].cells[edge % 3].twin.map(|(t, _)| t)
    }

    /// Both halves of a shared edge: neighbour triangle and its reverse edge index.
    /// The ledge sweep rotates through these twins around a vertex. [game] 006f7130.
    pub fn edge_twin(&self, tri: usize, edge: usize) -> Option<(usize, usize)> {
        self.tris[tri].cells[edge % 3].twin
    }

    pub fn edge_marked(&self, tri: usize, edge: usize) -> bool {
        self.tris[tri].cells[edge % 3].marked
    }

    pub fn edge_points(&self, tri: usize, edge: usize) -> ([f32; 3], [f32; 3]) {
        (self.verts[self.corner(tri, edge)], self.verts[self.corner(tri, edge + 1)])
    }

    pub fn tri_floor(&self, tri: usize) -> f32 {
        self.tris[tri].floor
    }

    pub fn tri_ceiling(&self, tri: usize) -> f32 {
        self.tris[tri].ceiling
    }

    #[cfg(test)]
    pub(crate) fn set_tri_ceiling_for_test(&mut self, tri: usize, ceiling: f32) {
        self.tris[tri].ceiling = ceiling;
    }

    pub fn tri_extra(&self, tri: usize) -> f32 {
        self.tris[tri].extra
    }

    /// Bits 0 to 2 are the silhouette's `FLAG_*` bits; bit 6 (0x40) is the winding bit.
    pub fn tri_flags(&self, tri: usize) -> u8 {
        self.tris[tri].flags
    }

    pub fn tri_incenter(&self, tri: usize) -> [f32; 2] {
        self.tris[tri].incenter
    }

    /// The live triangle containing `point` (x and y; the walk is not seeded by the caller).
    /// `None` outside the box. Points on an edge give one of the two triangles.
    pub fn locate(&self, point: Vec3) -> Option<usize> {
        match self.locate_loc(plane([point.x, point.y, 0.0])) {
            Loc::Inside(t) | Loc::Edge(t, _) => Some(t),
            Loc::Outside => None,
        }
    }

    /// Start the climb classifier near its retained cell, with the 5 mm edge band
    /// from 006fb450. [stand-in] The original first samples eight global-LCG vertex
    /// records in 006fb130 and starts at the nearest live cell; this port instead
    /// starts at the retained triangle and does not scan every triangle on failure.
    pub fn locate_seeded(&self, point: Vec3, seed: Option<usize>) -> Option<usize> {
        // A construction hint can name a cell killed by the last split. The original
        // 006fb130 selects a live vertex cell before classifying; never begin at a dead
        // hint. [stand-in] Initial seed choice still omits the original eight LCG samples.
        let seed = seed.filter(|&t| self.tris.get(t).is_some_and(|t| t.alive))
            .or_else(|| self.tris.get(self.hint).filter(|t| t.alive).map(|_| self.hint))
            .or_else(|| self.triangles().next())?;
        match self.locate_walk(plane([point.x, point.y, 0.0]), seed, EDGE_LINE_EPS, false) {
            Loc::Inside(t) | Loc::Edge(t, _) => Some(t),
            Loc::Outside => None,
        }
    }

    /// How many triangles have their incenter inside one of the silhouettes.
    pub fn inside_count(&self, sils: &[Silhouette]) -> usize {
        let polys: Vec<Vec<P2>> = sils.iter().map(|s| polygon_of(&s.points)).collect();
        self.triangles()
            .filter(|&t| {
                let ic = plane([self.tris[t].incenter[0], self.tris[t].incenter[1], 0.0]);
                polys.iter().any(|poly| point_in_polygon(ic, poly))
            })
            .count()
    }

    // ----- helpers that read the structure ------------------------------------------------

    fn corner(&self, t: usize, k: usize) -> usize {
        self.tris[t].cells[k % 3].origin
    }

    fn p2(&self, v: usize) -> P2 {
        plane(self.verts[v])
    }

    fn corner2(&self, t: usize, k: usize) -> P2 {
        self.p2(self.corner(t, k))
    }

    fn add_vertex(&mut self, q: [f32; 3]) -> usize {
        let i = self.verts.len();
        self.verts.push(q);
        self.vert_cell.push((usize::MAX, 0));
        self.grid.entry(grid_key(q)).or_default().push(i);
        i
    }

    /// The existing vertex within 5 mm of `q`, if any.
    fn near_vertex(&self, q: [f32; 3]) -> Option<usize> {
        let (kx, ky) = grid_key(q);
        let mut best: Option<(f64, usize)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(list) = self.grid.get(&(kx + dx, ky + dy)) else { continue };
                for &i in list {
                    let d = d2(plane(self.verts[i]), plane(q));
                    if d <= VERTEX_REUSE * VERTEX_REUSE && best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, i));
                    }
                }
            }
        }
        best.map(|(_, i)| i)
    }

    /// Rounds to 1 mm and clamps to the box. [game] 006fd910 lines 1-17.
    fn snap(&self, p: [f32; 3]) -> Result<[f32; 3], String> {
        if !p.iter().all(|v| v.is_finite()) {
            return Err(format!("point {p:?} is not finite"));
        }
        let r = |v: f32, lo: f32, hi: f32| {
            let rounded = ((v as f64) * 1000.0).round() / 1000.0;
            (rounded as f32).clamp(lo, hi)
        };
        Ok([r(p[0], self.min.x, self.max.x), r(p[1], self.min.y, self.max.y), r(p[2], self.min.z, self.max.z)])
    }

    /// The centre of the inscribed circle (weights are the opposite side lengths).
    fn incenter_of(&self, c: [usize; 3]) -> [f32; 2] {
        let (a, b, cc) = (self.p2(c[0]), self.p2(c[1]), self.p2(c[2]));
        let (la, lb, lc) = (dist(b, cc), dist(cc, a), dist(a, b));
        let s = la + lb + lc;
        let (x, y) = if s > 0.0 {
            ((la * a[0] + lb * b[0] + lc * cc[0]) / s, (la * a[1] + lb * b[1] + lc * cc[1]) / s)
        } else {
            ((a[0] + b[0] + cc[0]) / 3.0, (a[1] + b[1] + cc[1]) / 3.0)
        };
        [x as f32, y as f32]
    }

    fn set_winding(&mut self, t: usize) {
        let c = self.tris[t].cells;
        let o = orient(self.p2(c[0].origin), self.p2(c[1].origin), self.p2(c[2].origin));
        let f = &mut self.tris[t].flags;
        if o < 0.1 {
            *f |= FLAG_WINDING;
        } else {
            *f &= !FLAG_WINDING;
        }
    }

    fn pair(&mut self, x: (usize, usize), y: (usize, usize)) {
        self.tris[x.0].cells[x.1].twin = Some(y);
        self.tris[y.0].cells[y.1].twin = Some(x);
    }

    /// The triangles around vertex `u`, in order, as the cell with origin `u` in each.
    fn fan(&self, u: usize) -> Vec<(usize, usize)> {
        let start = self.vert_cell[u];
        if start.0 == usize::MAX || !self.tris[start.0].alive {
            return Vec::new();
        }
        let cap = self.tris.len() + 4;
        let mut out = vec![start];
        let mut cur = start;
        let mut closed = false;
        for _ in 0..cap {
            match self.tris[cur.0].cells[(cur.1 + 2) % 3].twin {
                Some(n) if n == start => {
                    closed = true;
                    break;
                }
                Some(n) => {
                    out.push(n);
                    cur = n;
                }
                None => break,
            }
        }
        if !closed {
            let mut back = Vec::new();
            cur = start;
            for _ in 0..cap {
                let Some((x, y)) = self.tris[cur.0].cells[cur.1].twin else { break };
                let prev = (x, (y + 1) % 3);
                if prev == start {
                    break;
                }
                back.push(prev);
                cur = prev;
            }
            back.reverse();
            back.extend(out);
            out = back;
        }
        out
    }

    fn find_edge(&self, u: usize, v: usize) -> Option<(usize, usize)> {
        self.fan(u).into_iter().find(|&(t, k)| self.corner(t, k + 1) == v)
    }

    fn mark_edge(&mut self, t: usize, k: usize) {
        self.tris[t].cells[k % 3].marked = true;
        if let Some((u, j)) = self.tris[t].cells[k % 3].twin {
            self.tris[u].cells[j].marked = true;
        }
    }

    // ----- point location ----------------------------------------------------------------

    /// The point location with the game's on-edge rule (5 mm line distance).
    fn locate_loc(&self, p: P2) -> Loc {
        self.locate_with(p, EDGE_LINE_EPS)
    }

    /// The walk from the hint, with a cap; if the walk does not end, every triangle is tried.
    /// An edge is "on" when the point's line distance to it is below `eps` ([game] 006fb450).
    /// There is no projection-extent test: the walk's cell choice does that job [assumed].
    fn locate_with(&self, p: P2, eps: f64) -> Loc {
        self.locate_walk(p, self.hint, eps, true)
    }

    fn locate_walk(&self, p: P2, seed: usize, eps: f64, scan_if_stuck: bool) -> Loc {
        let mut cur = seed;
        if cur >= self.tris.len() || !self.tris[cur].alive {
            if !scan_if_stuck {
                return Loc::Outside;
            }
            match self.tris.iter().position(|t| t.alive) {
                Some(i) => cur = i,
                None => return Loc::Outside,
            }
        }
        let cap = 2 * self.tris.len() + 16;
        for _ in 0..cap {
            let mut step = None;
            let mut on = None;
            for k in 0..3 {
                let (a, b) = (self.corner2(cur, k), self.corner2(cur, k + 1));
                let len = dist(a, b);
                if len == 0.0 {
                    continue;
                }
                // Clockwise triangle: the inside is on the positive side of each edge.
                let s = -orient(a, b, p) / len;
                // [game 006fb450:52-61] Runtime classification chooses the cell by
                // the SIGN before labelling a point inside the 5 mm edge band.
                // That band does not let a climb query start across its cell's edge.
                // The builder retains its existing tolerant insertion walk.
                if s < if scan_if_stuck { -eps } else { 0.0 } {
                    step = Some(k);
                    break;
                }
                if s < eps {
                    on = Some(k);
                }
            }
            if let Some(k) = step {
                match self.tris[cur].cells[k].twin {
                    Some((n, _)) => cur = n,
                    None => return Loc::Outside,
                }
                continue;
            }
            return match on {
                Some(k) => Loc::Edge(cur, k),
                None => Loc::Inside(cur),
            };
        }
        if !scan_if_stuck {
            return Loc::Outside;
        }
        for t in self.triangles() {
            let mut inside = true;
            let mut on = None;
            for k in 0..3 {
                let (a, b) = (self.corner2(t, k), self.corner2(t, k + 1));
                let len = dist(a, b);
                if len == 0.0 {
                    continue;
                }
                let s = -orient(a, b, p) / len;
                if s < -eps {
                    inside = false;
                    break;
                }
                if s < eps {
                    on = Some(k);
                }
            }
            if inside {
                return match on {
                    Some(k) => Loc::Edge(t, k),
                    None => Loc::Inside(t),
                };
            }
        }
        Loc::Outside
    }

    // ----- building ----------------------------------------------------------------------

    /// Inserts a point (rounded and clamped), returning its vertex. A point within 5 mm of a
    /// vertex returns that vertex. [game] 006fd910 with 006fb450 and the splits.
    pub fn insert_vertex(&mut self, p: [f32; 3]) -> Result<usize, String> {
        let q = self.snap(p)?;
        if let Some(i) = self.near_vertex(q) {
            return Ok(i);
        }
        let mut loc = self.locate_loc(plane(q));
        if !self.split_is_sound(loc, plane(q)) {
            // The game's rule gives a split that inverts a triangle: use the tight band for
            // this insertion only ([assumed] guard, counted).
            loc = self.locate_with(plane(q), TIGHT_EDGE_EPS);
            self.tight_fallbacks += 1;
        }
        let id = match loc {
            Loc::Outside => return Err(format!("point {q:?} is outside the mesh")),
            _ => self.add_vertex(q),
        };
        let ids = match loc {
            Loc::Inside(t) => self.split_inside(t, id)?,
            Loc::Edge(t, k) => self.split_edge(t, k, id)?,
            Loc::Outside => unreachable!(),
        };
        let pending = self.opposite_edges(&ids, id);
        self.legalize(pending)?;
        self.hint = ids[0];
        Ok(id)
    }

    /// Inserts a crossing point on a known edge. A point within 5 mm of a vertex reuses it.
    fn insert_on_edge(&mut self, t: usize, k: usize, x: [f32; 3]) -> Result<usize, String> {
        // Not rounded to 1 mm: the point must stay on the crossed edge's line.
        if !x.iter().all(|v| v.is_finite()) {
            return Err(format!("crossing point {x:?} is not finite"));
        }
        let q = [
            x[0].clamp(self.min.x, self.max.x),
            x[1].clamp(self.min.y, self.max.y),
            x[2].clamp(self.min.z, self.max.z),
        ];
        if let Some(i) = self.near_vertex(q) {
            return Ok(i);
        }
        let id = self.add_vertex(q);
        let ids = self.split_edge(t, k, id)?;
        let pending = self.opposite_edges(&ids, id);
        self.legalize(pending)?;
        self.hint = ids[0];
        Ok(id)
    }

    /// Whether splitting at `loc` with the point `p` gives only clockwise triangles (every new
    /// triangle has a negative orientation). Checked before anything is committed.
    fn split_is_sound(&self, loc: Loc, p: P2) -> bool {
        match loc {
            Loc::Outside => true,
            Loc::Inside(t) => (0..3).all(|k| orient(self.corner2(t, k), self.corner2(t, k + 1), p) < 0.0),
            Loc::Edge(t, k) => {
                let (a0, a1, a2) = (self.corner2(t, k), self.corner2(t, k + 1), self.corner2(t, k + 2));
                // A 1-to-2 or 2-to-4 split assumes the point is on the edge. Off the line by more
                // than the tight band, the triangles lose the sliver between the edge and the point
                // (found by the random-point test: a point 0.6 mm inside a box side).
                if orient(a0, a1, p).abs() / dist(a0, a1) >= TIGHT_EDGE_EPS {
                    return false;
                }
                let mut ok = orient(a0, p, a2) < 0.0 && orient(p, a1, a2) < 0.0;
                if let Some((u, j)) = self.tris[t].cells[k % 3].twin {
                    let d = self.corner2(u, j + 2);
                    ok = ok && orient(a1, p, d) < 0.0 && orient(p, a0, d) < 0.0;
                }
                ok
            }
        }
    }

    fn opposite_edges(&self, ids: &[usize], p: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for &id in ids {
            for k in 0..3 {
                if self.corner(id, k) != p && self.corner(id, k + 1) != p {
                    out.push((id, k));
                }
            }
        }
        out
    }

    /// 1-to-3: the point is inside triangle `t`. [game] 006fba10.
    fn split_inside(&mut self, t: usize, p: usize) -> Result<Vec<usize>, String> {
        let cells = self.tris[t].cells;
        let a = [cells[0].origin, cells[1].origin, cells[2].origin];
        let mut outer = HashMap::new();
        for k in 0..3 {
            if let Some((u, j)) = cells[k].twin {
                if u != t {
                    outer.insert((a[k], a[(k + 1) % 3]), (u, j));
                }
            }
        }
        let new: Vec<NewTri> = (0..3)
            .map(|k| NewTri { corners: [a[k], a[(k + 1) % 3], p], marked: [cells[k].marked, false, false], parent: t })
            .collect();
        self.commit(&[t], &new, &outer)
    }

    /// 1-to-2 on a boundary edge, 2-to-4 on an interior edge. [game] 006fc050.
    fn split_edge(&mut self, t: usize, k: usize, p: usize) -> Result<Vec<usize>, String> {
        let (k1, k2) = ((k + 1) % 3, (k + 2) % 3);
        let cells = self.tris[t].cells;
        let (a0, a1, a2) = (cells[k].origin, cells[k1].origin, cells[k2].origin);
        let (m0, m1, m2) = (cells[k].marked, cells[k1].marked, cells[k2].marked);
        let mut outer = HashMap::new();
        let mut killed = vec![t];
        let mut new = Vec::with_capacity(4);
        match cells[k].twin {
            None => {
                for idx in [k1, k2] {
                    if let Some(tw) = cells[idx].twin {
                        outer.insert((cells[idx].origin, cells[(idx + 1) % 3].origin), tw);
                    }
                }
                new.push(NewTri { corners: [a0, p, a2], marked: [m0, false, m2], parent: t });
                new.push(NewTri { corners: [p, a1, a2], marked: [m0, m1, false], parent: t });
            }
            Some((u, j)) => {
                let cu = self.tris[u].cells;
                let (j1, j2) = ((j + 1) % 3, (j + 2) % 3);
                let d = cu[j2].origin;
                let (mu0, mu1, mu2) = (cu[j].marked, cu[j1].marked, cu[j2].marked);
                for idx in [k1, k2] {
                    if let Some(tw) = cells[idx].twin {
                        if tw.0 != u {
                            outer.insert((cells[idx].origin, cells[(idx + 1) % 3].origin), tw);
                        }
                    }
                }
                for idx in [j1, j2] {
                    if let Some(tw) = cu[idx].twin {
                        if tw.0 != t {
                            outer.insert((cu[idx].origin, cu[(idx + 1) % 3].origin), tw);
                        }
                    }
                }
                killed.push(u);
                new.push(NewTri { corners: [a0, p, a2], marked: [m0, false, m2], parent: t });
                new.push(NewTri { corners: [p, a1, a2], marked: [m0, m1, false], parent: t });
                new.push(NewTri { corners: [a1, p, d], marked: [mu0, false, mu2], parent: u });
                new.push(NewTri { corners: [p, a0, d], marked: [mu0, mu1, false], parent: u });
            }
        }
        self.commit(&killed, &new, &outer)
    }

    /// Kills the `killed` triangles, creates `new` (heights and flags copied from each new
    /// triangle's parent), and links every cell: among the new cells, and to the outside
    /// neighbours through `outer`, keyed by the old directed edge.
    fn commit(
        &mut self,
        killed: &[usize],
        new: &[NewTri],
        outer: &HashMap<(usize, usize), (usize, usize)>,
    ) -> Result<Vec<usize>, String> {
        for &t in killed {
            self.tris[t].alive = false;
        }
        let mut ids = Vec::with_capacity(new.len());
        for nt in new {
            let parent = &self.tris[nt.parent];
            let tri = Tri {
                cells: [0, 1, 2].map(|k| Cell { origin: nt.corners[k], marked: nt.marked[k], twin: None }),
                floor: parent.floor,
                ceiling: parent.ceiling,
                extra: parent.extra,
                flags: parent.flags,
                incenter: [0.0; 2],
                alive: true,
            };
            ids.push(self.tris.len());
            self.tris.push(tri);
        }
        for &id in &ids {
            let corners = self.tris_corners(id);
            self.tris[id].incenter = self.incenter_of(corners);
            self.set_winding(id);
        }
        let mut newmap = HashMap::new();
        for &id in &ids {
            for k in 0..3 {
                newmap.insert((self.corner(id, k), self.corner(id, k + 1)), (id, k));
            }
        }
        let mut used = 0usize;
        for &id in &ids {
            for k in 0..3 {
                if self.tris[id].cells[k].twin.is_some() {
                    continue;
                }
                let (a, b) = (self.corner(id, k), self.corner(id, k + 1));
                if let Some(&other) = newmap.get(&(b, a)) {
                    self.pair((id, k), other);
                } else if let Some(&other) = outer.get(&(a, b)) {
                    self.pair((id, k), other);
                    used += 1;
                }
            }
        }
        if used != outer.len() {
            return Err(format!("cavity did not match its outside: {used} of {} outside edges linked", outer.len()));
        }
        for &id in &ids {
            for k in 0..3 {
                let v = self.corner(id, k);
                self.vert_cell[v] = (id, k);
            }
        }
        Ok(ids)
    }

    fn tris_corners(&self, t: usize) -> [usize; 3] {
        [self.corner(t, 0), self.corner(t, 1), self.corner(t, 2)]
    }

    /// Lawson flips on the listed edges (cell refs), each re-testing the outer edges of the
    /// two triangles it makes. Constraint edges never flip. [game] 006fdeb0, 006faa40.
    fn legalize(&mut self, mut stack: Vec<(usize, usize)>) -> Result<(), String> {
        let mut budget = 200 * self.tris.len() + 1000;
        while let Some((t, k)) = stack.pop() {
            if budget == 0 {
                return Err("edge flipping did not settle".to_owned());
            }
            budget -= 1;
            if !self.tris[t].alive || self.tris[t].cells[k].marked {
                continue;
            }
            let Some((u, j)) = self.tris[t].cells[k].twin else { continue };
            let (a, b, c) = (self.corner(t, k), self.corner(t, k + 1), self.corner(t, k + 2));
            let d = self.corner(u, j + 2);
            let (pa, pb, pc, pd) = (self.p2(a), self.p2(b), self.p2(c), self.p2(d));
            // Convexity gate, then the in-circle test on C against the circle through A, B, D.
            let gate = orient(pd, pa, pc) > 0.0 && orient(pc, pb, pd) > 0.0;
            if !(gate && inside_circle(pc, pa, pb, pd)) {
                continue;
            }
            let (tm, um) = (&self.tris[t].cells, &self.tris[u].cells);
            let new = [
                NewTri { corners: [c, a, d], marked: [tm[(k + 2) % 3].marked, um[(j + 1) % 3].marked, false], parent: t },
                NewTri { corners: [c, d, b], marked: [false, um[(j + 2) % 3].marked, tm[(k + 1) % 3].marked], parent: u },
            ];
            let mut outer = HashMap::new();
            for (tri, idx) in [(t, k + 1), (t, k + 2), (u, j + 1), (u, j + 2)] {
                if let Some((x, y)) = self.tris[tri].cells[idx % 3].twin {
                    if x != t && x != u {
                        outer.insert((self.corner(tri, idx), self.corner(tri, idx + 1)), (x, y));
                    }
                }
            }
            let ids = self.commit(&[t, u], &new, &outer)?;
            stack.extend([(ids[0], 0), (ids[0], 1), (ids[1], 1), (ids[1], 2)]);
        }
        Ok(())
    }

    /// The first edge the segment `u -> v` leaves the fan of `u` through (or a vertex it
    /// passes exactly through). [game] the walk of 006fcdd0; the crossing point is 006fabb0's
    /// segment intersection (ai-nav-build fourth read 3), not rounded (006fcdd0 writes it direct).
    fn first_crossing(&self, u: usize, v: usize) -> Cross {
        let (pu, pv) = (self.p2(u), self.p2(v));
        for (t, k) in self.fan(u) {
            let (i1, i2) = (self.corner(t, k + 1), self.corner(t, k + 2));
            let (p1, p2) = (self.p2(i1), self.p2(i2));
            let o = orient(pu, p1, p2);
            if o == 0.0 {
                continue;
            }
            let s = if o > 0.0 { 1.0 } else { -1.0 };
            // Inside the wedge at u of this triangle (boundary rays count).
            if s * orient(pu, p1, pv) < 0.0 || s * orient(pu, pv, p2) < 0.0 {
                continue;
            }
            let (o1, o2) = (orient(pu, p1, pv), orient(pu, pv, p2));
            if o1 == 0.0 && dot(sub(p1, pu), sub(pv, pu)) > 0.0 && d2(pv, pu) > d2(p1, pu) {
                return Cross::Vertex(i1);
            }
            if o2 == 0.0 && dot(sub(p2, pu), sub(pv, pu)) > 0.0 && d2(pv, pu) > d2(p2, pu) {
                return Cross::Vertex(i2);
            }
            let (sv, su) = (orient(p1, p2, pv), orient(p1, p2, pu));
            if sv == 0.0 || su == 0.0 || (sv > 0.0) == (su > 0.0) {
                continue;
            }
            // The segment must cross the opposite edge itself, not pass beside it.
            if orient(pu, pv, p1) * orient(pu, pv, p2) > 0.0 {
                continue;
            }
            // [game] 006fabb0: t = cross(C-A, d2) / cross(d1, d2), no hit when |den| < 0.001,
            // t clamped to 0..1. The point is A + t*d1 in x and y, z lerped.
            let (d1, d2v) = (sub(pv, pu), sub(p2, p1));
            let den = cross(d1, d2v);
            if den.abs() < CROSS_DEN_MIN {
                continue;
            }
            let r = (cross(sub(p1, pu), d2v) / den).clamp(0.0, 1.0);
            let zu = self.verts[u][2] as f64;
            let zv = self.verts[v][2] as f64;
            let x = [
                (pu[0] + r * d1[0]) as f32,
                (pu[1] + r * d1[1]) as f32,
                (zu + r * (zv - zu)) as f32,
            ];
            return Cross::Edge { tri: t, k: (k + 1) % 3, x };
        }
        Cross::Nothing
    }

    /// Makes `u -> v` a chain of constraint edges: crossings are split at the crossing point
    /// and the pieces are handled in turn. Marks the edges. [game] 006fcdd0. Every crossing
    /// takes the split path (crossing vertex, 006fc050): the game's other branch of the accept
    /// test (fourth read 3, "if") is not built, because the read does not say what it does.
    fn constrain(&mut self, u0: usize, v0: usize, name: &str) -> Result<(), String> {
        let mut stack = vec![(u0, v0)];
        let mut budget = 1000 + 64 * (self.verts.len() + self.tris.len());
        while let Some((u, v)) = stack.pop() {
            if budget == 0 {
                return Err(format!("silhouette {name}: the constraint walk did not finish"));
            }
            budget -= 1;
            if u == v {
                continue;
            }
            if let Some((t, k)) = self.find_edge(u, v) {
                self.mark_edge(t, k);
                continue;
            }
            match self.first_crossing(u, v) {
                Cross::Vertex(w) => {
                    stack.push((u, w));
                    stack.push((w, v));
                }
                Cross::Edge { tri, k, x } => {
                    let (pu, pv) = (self.verts[u], self.verts[v]);
                    if d2_3(x, pu) < MIN_PIECE_D2 || d2_3(x, pv) < MIN_PIECE_D2 {
                        // [game] 006fcdd0 at 006fd218: no vertex; the crossed edge and its twin
                        // are marked and the walk goes on from the edge's far end. [assumed]
                        // the far end is the endpoint farther from u.
                        let (i1, i2) = (self.corner(tri, k), self.corner(tri, k + 1));
                        self.mark_edge(tri, k);
                        let far = if d2(self.p2(i1), self.p2(u)) >= d2(self.p2(i2), self.p2(u)) { i1 } else { i2 };
                        self.snapped_pieces += 1;
                        stack.push((far, v));
                        continue;
                    }
                    let w = self.insert_on_edge(tri, k, x)?;
                    stack.push((u, w));
                    stack.push((w, v));
                }
                Cross::Nothing => {
                    self.skipped_pieces += 1;
                    self.warnings.push(format!("silhouette {name}: no crossing found for segment {u}-{v}"));
                }
            }
        }
        Ok(())
    }

    /// Inserts one outline: its vertices, then each ring segment as a constraint. [game]
    /// 006fdb90 (ring walk). Degenerate outlines give a warning and no constraints.
    pub fn insert_outline(&mut self, points: &[[f32; 3]], name: &str) -> Result<(), String> {
        let mut ring: Vec<[f32; 3]> = points.to_vec();
        if ring.len() >= 2 && ring[0] == ring[ring.len() - 1] {
            ring.pop();
        }
        let mut ids: Vec<usize> = Vec::with_capacity(ring.len());
        for p in &ring {
            let id = self.insert_vertex(*p).map_err(|e| format!("silhouette {name}: {e}"))?;
            if ids.last() != Some(&id) {
                ids.push(id);
            }
        }
        while ids.len() > 1 && ids.first() == ids.last() {
            ids.pop();
        }
        if ids.len() < 3 {
            self.warnings.push(format!("silhouette {name}: fewer than 3 distinct outline vertices, not constrained"));
            return Ok(());
        }
        let m = ids.len();
        for i in 0..m {
            let (u, v) = (ids[i], ids[(i + 1) % m]);
            if u == v {
                continue;
            }
            if d2_3(self.verts[u], self.verts[v]) <= SHORT_PAIR_D2 {
                self.short_pairs += 1;
                continue;
            }
            self.constrain(u, v, name)?;
        }
        Ok(())
    }

    /// Sets the floor, ceiling, extra height and flags of the triangles inside the silhouette.
    /// [game] 006f46d0 (ai-nav-build "third read" 5). The inside test is 007017e0's even-odd
    /// test [game]; the point it tests is the triangle's incenter [assumed]. The flood starts at
    /// the centroid's triangle [assumed].
    pub fn apply_silhouette(&mut self, s: &Silhouette) -> Result<usize, String> {
        let poly = polygon_of(&s.points);
        if poly.len() < 3 {
            return Ok(0);
        }
        // The game's centroid: x and y of points 0, 1 and the last, over 3 ([game] setter
        // 00701630 as ai-nav-build "Triangulation (third read)" 1 gives it). "Last" is taken as
        // the last distinct point [assumed].
        let last = poly[poly.len() - 1];
        let c = [(poly[0][0] + poly[1][0] + last[0]) / 3.0, (poly[0][1] + poly[1][1] + last[1]) / 3.0];
        let inside = |mesh: &NavMesh, t: usize| {
            let ic = mesh.tris[t].incenter;
            point_in_polygon([ic[0] as f64, ic[1] as f64], &poly)
        };
        let seed = match self.locate_loc(c) {
            Loc::Inside(t) | Loc::Edge(t, _) if inside(&*self, t) => Some(t),
            _ => self.triangles().find(|&t| inside(&*self, t)),
        };
        let Some(seed) = seed else { return Ok(0) };
        // Scale 1 [assumed]: the game's `k` (local_64) was not found.
        let bottom = s.base_z - s.pivot;
        let top = if s.height >= FLT_MAX { FLT_MAX } else { bottom + s.height };
        let limit = if s.ai_ban_height >= FLT_MAX { FLT_MAX } else { bottom + s.ai_ban_height };
        let mut seen: HashSet<usize> = HashSet::new();
        let mut stack = vec![seed];
        let mut touched = 0;
        while let Some(t) = stack.pop() {
            if !seen.insert(t) || !self.tris[t].alive || !inside(&*self, t) {
                continue;
            }
            self.apply_heights_to(t, s, top, limit);
            touched += 1;
            for k in 0..3 {
                if let Some((u, _)) = self.tris[t].cells[k].twin {
                    if !seen.contains(&u) {
                        stack.push(u);
                    }
                }
            }
        }
        Ok(touched)
    }

    fn apply_heights_to(&mut self, t: usize, s: &Silhouette, top: f32, limit: f32) {
        let tri = &mut self.tris[t];
        // [game] 006f46d0 (fourth read 4): `top < floor` (strict) takes the ceiling-only branch.
        // Its fVar1 >= 0 sign branch is not read [unknown], so only the positive case is ported.
        if top < tri.floor {
            // The floor is already above the top: only the ceiling takes the limit.
            if limit < FLT_MAX {
                tri.ceiling = tri.ceiling.max(limit);
            }
        } else if limit >= FLT_MAX {
            // [game] With top = FLT_MAX (height 0) the floor is set to FLT_MAX literally. What a
            // FLT_MAX floor means for the agent is [assumed]: read as unwalkable, the agent
            // gate (006f04f0) is not checked against it.
            tri.floor = top;
            tri.ceiling = FLT_MAX;
            tri.extra = s.extra_height;
            tri.flags = (tri.flags & FLAG_WINDING) | (s.flags & 0x7) as u8;
        } else {
            // A limit with a real top: the ceiling only [game] 006f4eb5, the ceiling rule
            // as read in 006f46d0 is [assumed] to be a max.
            tri.ceiling = tri.ceiling.max(limit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// xorshift64: deterministic test points.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }

        fn point(&mut self, lo: f64, hi: f64) -> [f32; 3] {
            let x = lo + (hi - lo) * self.next();
            let y = lo + (hi - lo) * self.next();
            [x as f32, y as f32, 0.0]
        }
    }

    fn unit_mesh() -> NavMesh {
        NavMesh::new(Vec3::new(0.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 20.0))
    }

    fn square(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<[f32; 3]> {
        vec![[x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0], [x0, y0, 0.0]]
    }

    fn sil(points: Vec<[f32; 3]>, height: f32) -> Silhouette {
        let raw_points = points.len();
        Silhouette {
            name: "test".into(),
            points,
            base_z: 0.0,
            height,
            replacement_height: 0.0,
            ai_ban_height: FLT_MAX,
            extra_height: 0.0,
            pivot: 0.0,
            flags: 0,
            can_not_walk_off: false,
            treat_as_dynamic: false,
            impermanent: false,
            raw_points,
            object: (0, 0),
        }
    }

    /// Twins mutual, same edge both ways, clockwise, live; the area of the box.
    fn check_invariants(m: &NavMesh) {
        let mut area = 0.0;
        for t in m.triangles() {
            let c = [m.corner2(t, 0), m.corner2(t, 1), m.corner2(t, 2)];
            let o = orient(c[0], c[1], c[2]);
            assert!(o < 0.0, "triangle {t} is not clockwise: {o}");
            area += -o * 0.5;
            for k in 0..3 {
                if let Some((u, j)) = m.tris[t].cells[k].twin {
                    assert!(m.tris[u].alive, "twin of {t}.{k} is dead");
                    assert_eq!(m.tris[u].cells[j].twin, Some((t, k)), "twin not mutual at {t}.{k}");
                    assert_eq!(m.corner(u, j), m.corner(t, k + 1));
                    assert_eq!(m.corner(u, j + 1), m.corner(t, k));
                    assert_eq!(m.tris[u].cells[j].marked, m.tris[t].cells[k].marked, "marks differ across {t}.{k}");
                }
            }
        }
        let (lo, hi) = m.bounds();
        let box_area = ((hi.x - lo.x) * (hi.y - lo.y)) as f64;
        assert!((area - box_area).abs() < 1e-6 * box_area.max(1.0), "area {area} != box {box_area}");
    }

    /// Every ring segment a..b is a chain of marked edges through the vertices on it.
    fn check_chain(m: &NavMesh, a: [f32; 3], b: [f32; 3]) {
        check_chain_gaps(m, a, b, false);
    }

    /// As `check_chain`; with `gaps_reported`, a missing edge is accepted (a skipped piece).
    fn check_chain_gaps(m: &NavMesh, a: [f32; 3], b: [f32; 3], gaps_reported: bool) {
        let (pa, pb) = (plane(a), plane(b));
        let d = sub(pb, pa);
        let len2 = dot(d, d);
        let mut on: Vec<(f64, usize)> = Vec::new();
        for v in 0..m.vertex_count() {
            let p = m.p2(v);
            let rel = sub(p, pa);
            let t = dot(rel, d) / len2;
            if (-1e-4..=1.0 + 1e-4).contains(&t) && cross(d, rel).abs() / len2.sqrt() < 1e-4 {
                on.push((t, v));
            }
        }
        on.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        assert!(on.first().is_some_and(|x| x.0 < 1e-4), "chain of {a:?}..{b:?} does not start at a");
        assert!(on.last().is_some_and(|x| x.0 > 1.0 - 1e-4), "chain of {a:?}..{b:?} does not end at b");
        for w in on.windows(2) {
            let (x, y) = (w[0].1, w[1].1);
            match m.find_edge(x, y) {
                Some((t, k)) => assert!(m.tris[t].cells[k].marked, "edge {x}-{y} on {a:?}..{b:?} is not marked"),
                None => assert!(gaps_reported, "no edge {x}-{y} on {a:?}..{b:?}"),
            }
        }
    }

    #[test]
    fn root_mesh_is_two_clockwise_triangles_over_the_box() {
        let m = unit_mesh();
        assert_eq!(m.tri_count(), 2);
        assert_eq!(m.vertex_count(), 4);
        check_invariants(&m);
        assert_eq!(m.marked_cells(), 4, "the four box sides are marked");
        assert_eq!(m.tri_floor(0), -1.0);
        assert_eq!(m.tri_ceiling(0), FLT_MAX);
    }

    #[test]
    fn random_points_keep_the_mesh_valid_and_delaunay() {
        let mut m = unit_mesh();
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..300 {
            let p = rng.point(0.0, 1.0);
            m.insert_vertex(p).unwrap();
        }
        check_invariants(&m);
        let mut bad = 0;
        for t in m.triangles() {
            for k in 0..3 {
                if m.tris[t].cells[k].marked {
                    continue;
                }
                if let Some((u, j)) = m.tris[t].cells[k].twin {
                    let (a, b, c) = (m.corner2(t, k), m.corner2(t, k + 1), m.corner2(t, k + 2));
                    let d = m.corner2(u, j + 2);
                    if inside_circle(d, a, b, c) {
                        bad += 1;
                    }
                }
            }
        }
        assert_eq!(bad, 0, "{bad} unmarked edges are not Delaunay");
    }

    #[test]
    fn a_point_inside_five_mm_of_a_box_side_uses_the_tight_fallback() {
        let mut m = unit_mesh();
        // 0.6 mm inside the bottom side: on the game's 5 mm edge rule, not on the edge.
        m.insert_vertex([0.5, 0.0006, 0.0]).unwrap();
        assert_eq!(m.tight_fallbacks, 1, "the 5 mm split would drop the sliver");
        check_invariants(&m);
        // A point on the side is 5 mm from the sliver edge the first insertion made as well, so
        // the classifier has two "on" edges; the guard may fall back again. Only the mesh is checked.
        m.insert_vertex([0.3, 0.0, 0.0]).unwrap();
        check_invariants(&m);
    }

    /// The us_test counts for the notes (reads the install; run with --ignored --nocapture).
    #[test]
    #[ignore = "reads the install's levels/us_test; prints the counts for notes/status.md"]
    fn us_test_counts() {
        let (sils, _) = load(crate::install::dir().join("levels/us_test/zone.str").as_path()).unwrap();
        let m = NavMesh::build(None, &sils).unwrap();
        eprintln!(
            "us_test: vertices {} triangles {} marked sides {} short pairs {} not inserted {} snapped {} tight fallbacks {}",
            m.vertex_count(),
            m.tri_count(),
            m.marked_cells(),
            m.short_pairs,
            m.skipped_pieces,
            m.snapped_pieces,
            m.tight_fallbacks
        );
    }

    #[test]
    fn locate_agrees_with_a_brute_force_containment_test() {
        let mut m = unit_mesh();
        let mut rng = Rng(12345);
        for _ in 0..150 {
            let p = rng.point(0.0, 1.0);
            m.insert_vertex(p).unwrap();
        }
        for _ in 0..300 {
            let q = rng.point(0.0, 1.0);
            let qp = plane(q);
            let found = m.locate(Vec3::new(q[0], q[1], 0.0)).expect("inside the box");
            let contains = |t: usize| {
                (0..3).all(|k| {
                    let (a, b) = (m.corner2(t, k), m.corner2(t, k + 1));
                    -orient(a, b, qp) / dist(a, b) >= -2.0 * EDGE_LINE_EPS
                })
            };
            assert!(contains(found), "locate returned a triangle without the point");
            assert!(m.triangles().any(contains), "no triangle contains the point");
        }
    }

    #[test]
    fn a_square_outline_has_all_four_sides_marked() {
        let mut m = unit_mesh();
        let sq = square(0.2, 0.2, 0.6, 0.6);
        m.insert_outline(&sq, "square").unwrap();
        check_invariants(&m);
        for w in sq.windows(2) {
            check_chain(&m, w[0], w[1]);
        }
    }

    #[test]
    fn a_square_crossing_existing_edges_still_has_its_sides_marked() {
        let mut m = unit_mesh();
        // 60 points: the sides cross several edges, and the crossing pieces stay at least
        // 0.0316 m long. With 200 points two crossings on one side fall closer than that and the
        // piece between them is skipped and reported (checked in the next test).
        let mut rng = Rng(777);
        for _ in 0..60 {
            let p = rng.point(0.0, 1.0);
            m.insert_vertex(p).unwrap();
        }
        let sq = square(0.15, 0.1, 0.85, 0.9);
        m.insert_outline(&sq, "square").unwrap();
        check_invariants(&m);
        assert_eq!(m.skipped_pieces, m.warnings.len(), "every skip is a warning");
        assert!(m.vertex_count() > 4 + 60 + 4, "the sides must have added crossing vertices");
        for w in sq.windows(2) {
            check_chain_gaps(&m, w[0], w[1], m.skipped_pieces + m.snapped_pieces > 0);
        }
    }

    #[test]
    fn a_short_crossing_piece_marks_the_existing_edge_and_makes_no_vertex() {
        let mut m = unit_mesh();
        let mut rng = Rng(777);
        for _ in 0..200 {
            let p = rng.point(0.0, 1.0);
            m.insert_vertex(p).unwrap();
        }
        let sq = square(0.15, 0.1, 0.85, 0.9);
        let before = m.vertex_count();
        m.insert_outline(&sq, "square").unwrap();
        check_invariants(&m);
        assert!(m.snapped_pieces > 0, "this seed has short crossing pieces");
        assert!(m.warnings.iter().all(|w| !w.contains("shorter than")), "a snapped piece is not a warning");
        assert!(m.vertex_count() > before, "the outline's own points are added");
        for w in sq.windows(2) {
            check_chain_gaps(&m, w[0], w[1], true);
        }
    }

    #[test]
    fn two_overlapping_outlines_keep_their_sides() {
        let mut m = unit_mesh();
        let a = square(0.2, 0.2, 0.6, 0.6);
        let b = square(0.4, 0.3, 0.8, 0.7);
        m.insert_outline(&a, "a").unwrap();
        m.insert_outline(&b, "b").unwrap();
        check_invariants(&m);
        for w in a.windows(2).chain(b.windows(2)) {
            check_chain(&m, w[0], w[1]);
        }
    }

    #[test]
    fn heights_flood_marks_exactly_the_triangles_inside_a_square() {
        let bounds = (Vec3::new(0.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 20.0));
        let s = sil(square(0.25, 0.2, 0.7, 0.65), 5.0);
        let m = NavMesh::build(Some(bounds), std::slice::from_ref(&s)).unwrap();
        check_invariants(&m);
        let poly = polygon_of(&s.points);
        let mut inside = 0;
        for t in m.triangles() {
            let ic = m.tris[t].incenter;
            let want = point_in_polygon([ic[0] as f64, ic[1] as f64], &poly);
            let got = m.tri_floor(t) == 5.0;
            assert_eq!(got, want, "triangle {t} with incenter {ic:?}");
            if want {
                inside += 1;
                assert_eq!(m.tri_ceiling(t), FLT_MAX);
                assert_eq!(m.tri_flags(t) & 0x7, 0);
            } else {
                assert_eq!(m.tri_floor(t), -1.0);
            }
        }
        assert!(inside > 0);
    }

    #[test]
    fn degenerate_outlines_do_not_panic() {
        let bounds = (Vec3::new(0.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 20.0));
        // Collinear points closed on themselves.
        let line = vec![[0.1, 0.5, 0.0], [0.5, 0.5, 0.0], [0.9, 0.5, 0.0], [0.1, 0.5, 0.0]];
        // Repeated points.
        let repeated = vec![[0.3, 0.3, 0.0], [0.3, 0.3, 0.0], [0.6, 0.3, 0.0], [0.6, 0.3, 0.0], [0.6, 0.6, 0.0], [0.3, 0.3, 0.0]];
        // Outside the bounds: clamped onto the box edge.
        let outside = square(-5.0, -5.0, 7.0, 7.0);
        let sils = [sil(line, 2.0), sil(repeated, 2.0), sil(outside, 2.0)];
        let m = NavMesh::build(Some(bounds), &sils).expect("degenerate input builds");
        check_invariants(&m);
        assert!(m.tri_count() >= 2);
        let empty = NavMesh::build(Some(bounds), &[]).unwrap();
        assert_eq!(empty.tri_count(), 2);
        assert!(NavMesh::build(Some((Vec3::ZERO, Vec3::ZERO)), &[]).is_err());
    }

    #[test]
    fn outline_rules_drop_the_collinear_point_and_apply_the_matrix() {
        // Identity rotation, translation (10, 20, 3).
        let mut m = [0.0f32; 12];
        m[0] = 1.0;
        m[4] = 1.0;
        m[8] = 1.0;
        m[9] = 10.0;
        m[10] = 20.0;
        m[11] = 3.0;
        let raw = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]];
        let out = outline_in_world(&raw, &m).unwrap();
        assert_eq!(out.len(), 5, "the midpoint on the chord is dropped, the ring is closed");
        assert_eq!(out[0], [10.0, 20.0, 0.0], "z of the translation is not applied to the outline");
        assert_eq!(out[4], out[0]);
        assert!(outline_in_world(&raw[..3], &m).is_err());
        assert!(outline_in_world(&[raw[0], raw[1], raw[2], raw[3]], &m).is_err(), "not closed");
    }

    #[test]
    fn attachment_elevation_is_not_added_to_nav_floor() {
        let mut bind = crate::level_render::IDENTITY;
        bind[3] = [10.0, 20.0, 36.0];
        let raw = square(0.0, 0.0, 1.0, 1.0);
        let (points, base_z, scale) = placed_outline(&raw, &bind, &crate::level_render::IDENTITY).unwrap();
        assert_eq!(points[0], [10.0, 20.0, 0.0]);
        let mut s = sil(points, placed_height(36.0, scale));
        s.base_z = base_z;
        let mesh = NavMesh::build(Some((Vec3::new(0.0, 0.0, -1.0), Vec3::new(30.0, 30.0, 100.0))), &[s]).unwrap();
        let tri = mesh.locate(Vec3::new(10.5, 20.5, 0.0)).unwrap();
        assert_eq!(mesh.tri_floor(tri), 36.0, "attachment z must not double the authored height to 72 m");
    }

    #[test]
    fn owner_placement_scales_height_and_pivot() {
        let mut bind = crate::level_render::IDENTITY;
        bind[3] = [10.0, 20.0, 36.0];
        let mut owner = crate::level_render::IDENTITY;
        owner[0][0] = 2.0;
        owner[1][1] = 2.0;
        owner[2][2] = 3.0;
        owner[3] = [100.0, 200.0, 7.0];
        let (points, base_z, scale) = placed_outline(&square(0.0, 0.0, 1.0, 1.0), &bind, &owner).unwrap();
        assert_eq!(points[0], [120.0, 240.0, 7.0]);
        assert_eq!(points[1], [122.0, 240.0, 7.0]);
        assert_eq!((base_z, scale), (7.0, 3.0));
        let mut s = sil(points, placed_height(4.0, scale));
        s.base_z = base_z;
        s.pivot = 1.0 * scale;
        let mesh = NavMesh::build(Some((Vec3::new(100.0, 200.0, -1.0), Vec3::new(130.0, 250.0, 30.0))), &[s]).unwrap();
        let tri = mesh.locate(Vec3::new(121.0, 241.0, 0.0)).unwrap();
        assert_eq!(mesh.tri_floor(tri), 7.0 - 3.0 + 12.0);
        assert_eq!(placed_height(0.0, 0.0), FLT_MAX, "open height is tested before scaling");
    }

    /// [data] Escort zone sample from nav_floor_check: this platform's authored
    /// height is 16 m, placed 0.056 m up. Its attachment bind is also 16 m up.
    #[test]
    fn installed_escort_platform_keeps_its_authored_height() {
        let path = &crate::install::dir().join("levels/us_test/zone.str");
        if !path.is_file() {
            return;
        }
        let (sils, skipped) = load(path).unwrap();
        assert!(skipped.is_empty(), "{skipped:?}");
        let floor = sils.iter()
            .filter(|s| point_in_polygon([134.06, -14.17], &polygon_of(&s.points)))
            .filter(|s| s.ai_ban_height == FLT_MAX && s.height < FLT_MAX)
            .map(|s| s.base_z - s.pivot + s.height)
            .max_by(f32::total_cmp)
            .expect("the recorded platform sample lies inside a silhouette");
        assert!((floor - 16.056).abs() < 0.01, "platform floor {floor}, previously doubled to 32.056");
    }
}
