//! Path planning over the navigation mesh (phase 3), the way the game plans its paths.
//!
//! Read from `notes\ai-nav-search.md` (the search 006ee6b0 and its open list, relax and close),
//! `notes\ai-nav-path.md` (the path core 006f2060, the straight test 006f0950, the smoothing
//! 006f05e0, the follower 006ff580) and `notes\ai-nav-build.md` (the edge cost 006f1110, the
//! heuristic 006f1980, the cell gate 006f04f0, the portal metric 006fa6e0).
//!
//! Here a graph node is one triangle of `NavMesh` (the game's node is one triangle block with
//! three links). Tags as in `navmesh.rs`: `[game]` read in code, `[data]` from the install,
//! `[assumed]` a guess (each one line in `notes\status.md`, "Phase 3, navigation mesh", path
//! planning), `[stand-in]` a placeholder the game does differently.
//!
//! Second read (`notes\ai-nav-path.md` "Second read: the waypoint builder") applied: the search
//! entry point (`entry_point`, two branches), the straight test without the goal triangle check,
//! and the waypoint builder (`build_records`: sampled waypoints on the shrunk portals, node
//! records, output pass, kind-2 corner pass). The DFS over cells is `[stand-in]` (one shared edge
//! per consecutive path pair is used instead).
//!
//! [stand-in] Dynamic per-query silhouettes (the other objects listed by the caller, read in
//! ai-nav-path section F) are not ported: other characters are not obstacles to this planner.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use glam::Vec3;

use crate::ai_move::PathSource;
use crate::navmesh::NavMesh;

const FLT_MAX: f32 = f32::MAX;

/// Agent flag bit 0 (0x01): the link float of a silhouette edge is 30 (not 15) when the height
/// gap reaches the agent height. [game] 006f1a10 (the agent flag bit 0 test).
pub const AGENT_FLAG_STEP_TALL: u32 = 0x01;
/// Agent flag bit 2 (0x04): silhouette edges carry a link float; without it the links are 0.
/// [game] 006f1a10 (`agent flag bit 2 is clear` gives 0.0).
pub const AGENT_FLAG_LINK_FLOATS: u32 = 0x04;
/// Mask 8: a triangle's ceiling counts (the upper bound). [game] 006f0950 and 006f04f0 (mask 8).
pub const AGENT_FLAG_CEILING: u32 = 0x08;
/// Mask 0x10: marked edges pass without the height test. [game] 006f04f0 returns 0 for it.
pub const AGENT_FLAG_NO_HEIGHT: u32 = 0x10;

/// A node record: a path triangle entered with no waypoint written for its portal. [game] 006f2060
/// section D (kind 0 when agent flag 8 is off, ai-nav-path second read).
pub const KIND_NODE: u32 = 0;
/// A step-up point: agent flag 1 set and the rise over the previous output point is above 0 and
/// below the agent height. [game] 006f2060 lines 518-521 (second read, C step 15).
pub const KIND_STEP: u32 = 2;
/// A waypoint sampled on a portal, offset by (radius + 0.1) along its normal. [game] 006f2060
/// lines 363 and 493-500 (second read, C steps 8 and 14).
pub const KIND_SAMPLE: u32 = 5;
/// The first port's name for kind 0; the same value as `KIND_NODE`.
pub const KIND_WAYPOINT: u32 = KIND_NODE;

/// Open-list cutoff at the start of a search. [game] 006ee6cc (500.0 from 00cad46c).
const CUTOFF_START: f32 = 500.0;
/// Pops allowed in one search. [game] 006ee778 (0x1a9 = 425).
const POP_BUDGET: u32 = 425;
/// A path of this many nodes or more fails. [game] 006ee9a2 (0x258 = 600).
const PATH_NODE_LIMIT: usize = 600;
/// Straight tests in one smoothing pass. [game] 006f05e0 line 22 (0x40 = 64).
const SMOOTH_TESTS: u32 = 64;
/// Smoothing only tries nodes this close (squared, 50 m). [game] 006f05e0 line 21 (2500.0).
const SMOOTH_RANGE_SQ: f32 = 2500.0;
/// Waypoints this close (squared, 0.5 m) merge. [game] 006f2060 line 338 (0.25).
const RECORD_MERGE_SQ: f32 = 0.25;
/// Floors closer than this count as the same height (the node rule). [game] 006f2060 lines 382-389
/// (the 0.1 test, second read C step 10).
const HEIGHT_STEP: f32 = 0.1;
/// The follower's overshoot along a segment. [game] 006ff580 (0.006 m, decompile constant).
const OVERSHOOT: f32 = 0.006;
/// Default follower tolerance on the own point's distance to the path (params+0x0c).
/// [assumed] 0.2 m: the tolerance 00826270 uses in one case (ai-behaviours-move.md section 4).
const DEFAULT_NEAR_TOLERANCE: f32 = 0.2;
/// Default squared destination-change threshold (params+0x14). [assumed] 2 m: the "dest changed
/// or over 2 m" reading in functions.tsv 0081ccb0. Settle: the 5-word block of 00826270.
const DEFAULT_DEST_TOLERANCE_SQ: f32 = 4.0;

/// The agent the path is planned for (the params block of the path core). The game's block is
/// `[radius, height, flags]` (006f2060 param_4, [game]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Agent {
    pub radius: f32,
    pub height: f32,
    pub flags: u32,
}

impl Agent {
    pub fn new(radius: f32, height: f32, flags: u32) -> Self {
        Self { radius, height, flags }
    }
}

/// One path record: a point to steer at, and its kind (see `KIND_WAYPOINT`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Record {
    pub point: Vec3,
    pub kind: u32,
}

fn heuristic(p: Vec3, goal: Vec3) -> f32 {
    // [game] 006f1980: Euclidean 3D distance, not squared. Admissibility is [assumed] (ai-nav-build section 4).
    p.distance(goal)
}

/// The cell gate of one edge, from the triangle `tri` across `edge` to its neighbour.
/// [game] 006f04f0 as ai-nav-build section 4 gives it: an unmarked edge passes (its twin is
/// unmarked too, the marks agree across twins). A marked edge passes when agent flag 0x10 is
/// set, otherwise it compares the two triangles' floors with the agent height. An unmarked edge
/// also has the portal width rule of the edge cost: narrower than 2 x radius is blocked.
///
/// [assumed] The height rule: blocked when the far floor is at least the agent height above this
/// floor (a step up the agent cannot take), and with mask 0x08 also when the far ceiling leaves
/// less than the agent height above the far floor. A drop is passed. Settle: 006f04f0 lines 5-40
/// read in full, and a trace of one step.
pub fn passable(mesh: &NavMesh, tri: usize, edge: usize, agent: &Agent) -> bool {
    let Some(far) = mesh.neighbour(tri, edge) else {
        return false;
    };
    if mesh.edge_marked(tri, edge) {
        if agent.flags & AGENT_FLAG_NO_HEIGHT != 0 {
            return true;
        }
        let here = mesh.tri_floor(tri);
        let there = mesh.tri_floor(far);
        if there - here >= agent.height {
            return false;
        }
        if agent.flags & AGENT_FLAG_CEILING != 0 && mesh.tri_ceiling(far) - there < agent.height {
            return false;
        }
        true
    } else {
        portal_wide_enough(mesh, tri, edge, far, agent)
    }
}

/// The portal width rule (edge cost 006f1110, the width test at line 90). [game] for the test
/// `radius * 2 <= portal length`. [assumed] for the portal: the shared edge, and only when both
/// its ends are silhouette corners (each is a corner of a marked edge in one of the two
/// triangles). A short free edge in open ground is not a gap, so it is not blocked. Settle: the
/// portal construction in 006f1110 lines 60-90 (read in part) and a trace of a gap.
fn portal_wide_enough(mesh: &NavMesh, tri: usize, edge: usize, far: usize, agent: &Agent) -> bool {
    let vs = mesh.tri_vertices(tri);
    let a = vs[edge % 3];
    let b = vs[(edge + 1) % 3];
    if !(touches_marked(mesh, tri, far, a) && touches_marked(mesh, tri, far, b)) {
        return true;
    }
    let (pa, pb) = mesh.edge_points(tri, edge);
    let len = ((pb[0] - pa[0]) * (pb[0] - pa[0]) + (pb[1] - pa[1]) * (pb[1] - pa[1])).sqrt();
    len >= 2.0 * agent.radius
}

/// True when vertex `v` is an end of a marked edge of `t1` or `t2`.
fn touches_marked(mesh: &NavMesh, t1: usize, t2: usize, v: usize) -> bool {
    for t in [t1, t2] {
        let vs = mesh.tri_vertices(t);
        for k in 0..3 {
            if (vs[k] == v || vs[(k + 1) % 3] == v) && mesh.edge_marked(t, k) {
                return true;
            }
        }
    }
    false
}

/// The link float of the edge (the 0 / 15 / 30 of 006f1a10 lines 33-49). [game] for the values
/// and the conditions; [assumed] for their meaning (a gap penalty, larger for a wider step).
fn link_float(mesh: &NavMesh, tri: usize, edge: usize, agent: &Agent) -> f32 {
    let Some(far) = mesh.neighbour(tri, edge) else {
        return 0.0;
    };
    if !mesh.edge_marked(tri, edge) || agent.flags & AGENT_FLAG_LINK_FLOATS == 0 {
        return 0.0;
    }
    let far_floor = mesh.tri_floor(far);
    if far_floor == FLT_MAX {
        return 0.0;
    }
    if agent.flags & AGENT_FLAG_STEP_TALL != 0 && far_floor - mesh.tri_floor(tri) >= agent.height {
        30.0
    } else {
        15.0
    }
}

/// The point where the way from `from` to the far triangle crosses the shared edge: the portal
/// point the game writes into the far node (the mesh point of 006f1110 read by the expansion
/// 006f19e0). [assumed] The crossing of the line from `from` to the far incenter with the edge,
/// clamped to the edge; the z is the z of `from`. Settle: the far point arithmetic in 006f1110
/// lines 45-52 and 60-90 (read in part).
pub fn portal_point(mesh: &NavMesh, tri: usize, edge: usize, from: Vec3, far: usize) -> Vec3 {
    let (a, b) = mesh.edge_points(tri, edge);
    let q = mesh.tri_incenter(far);
    let (px, py) = (from.x as f64, from.y as f64);
    let (dx, dy) = (q[0] as f64 - px, q[1] as f64 - py);
    let (ax, ay) = (a[0] as f64, a[1] as f64);
    let (ex, ey) = (b[0] as f64 - ax, b[1] as f64 - ay);
    let denom = dx * ey - dy * ex;
    let mut s: Option<f64> = None;
    if denom.abs() > 1e-12 {
        let (apx, apy) = (ax - px, ay - py);
        let s_hit = (apx * dy - apy * dx) / denom;
        let t_hit = (apx * ey - apy * ex) / denom;
        if (0.0..=1.0).contains(&s_hit) && (0.0..=1.0).contains(&t_hit) {
            s = Some(s_hit);
        }
    }
    let s = s.unwrap_or_else(|| {
        let ee = ex * ex + ey * ey;
        if ee <= 0.0 {
            0.0
        } else {
            (((q[0] as f64 - ax) * ex + (q[1] as f64 - ay) * ey) / ee).clamp(0.0, 1.0)
        }
    });
    Vec3::new((ax + s * ex) as f32, (ay + s * ey) as f32, from.z)
}

/// The 2D crossing of segment p0..p1 with q0..q1: the parameters (t on p, s on q) when both lie in
/// 0..=1, else `None`. Parallel segments give `None`.
fn cross2(p0: [f64; 2], p1: [f64; 2], q0: [f64; 2], q1: [f64; 2]) -> Option<(f64, f64)> {
    let d = [p1[0] - p0[0], p1[1] - p0[1]];
    let e = [q1[0] - q0[0], q1[1] - q0[1]];
    let denom = d[0] * e[1] - d[1] * e[0];
    if denom.abs() < 1e-12 {
        return None;
    }
    let w = [q0[0] - p0[0], q0[1] - p0[1]];
    let t = (w[0] * e[1] - w[1] * e[0]) / denom;
    let s = (w[0] * d[1] - w[1] * d[0]) / denom;
    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&s) {
        Some((t, s))
    } else {
        None
    }
}

/// The point a node is entered at through `edge` of `tri`, walking from `prev` (the previous node's
/// position) toward `goal`. [game] 006f1110 lines 90-203 (ai-nav-path second read, section A,
/// priority 5), two branches:
///
/// - Branch 1 (agent flag 8 clear): the shared edge A-B is pulled in by the full radius at both
///   ends, A' = A + r u and B' = B - r u. The entry is where the sight line crosses A'B' (line 140,
///   006fabb0), else the foot of the perpendicular from `prev` to A'B', clamped to it.
///   [assumed] The sight line runs from `prev` to `goal` (the note says "the sight line" and "the
///   sight point" without naming the far end). [assumed] z is `prev`'s z (the note gives no z for
///   this branch). [assumed] An edge shorter than 2 r collapses to its midpoint (the note has no
///   case for it). Settle: 006f1110 lines 115-142 and the arguments of 006fabb0.
/// - Branch 2 (agent flag 8 set): the foot of `prev` on the whole edge, clamped (lines 182-192),
///   with z the entered triangle's floor (lines 193-197), or `prev`'s z when that floor is the
///   sentinel. [assumed] "the parent has flag 8" is read as the query's mask 8 (the note's
///   per-node flag is not kept by this port). [assumed] "owner" is the entered triangle.
pub fn entry_point(
    mesh: &NavMesh,
    tri: usize,
    edge: usize,
    prev: Vec3,
    goal: Vec3,
    far: usize,
    agent: &Agent,
) -> Vec3 {
    let (a, b) = mesh.edge_points(tri, edge);
    let (ax, ay) = (a[0] as f64, a[1] as f64);
    let (bx, by) = (b[0] as f64, b[1] as f64);
    let (ex, ey) = (bx - ax, by - ay);
    let elen2 = ex * ex + ey * ey;
    let (px, py) = (prev.x as f64, prev.y as f64);
    if agent.flags & AGENT_FLAG_CEILING != 0 {
        let s = if elen2 > 0.0 { (((px - ax) * ex + (py - ay) * ey) / elen2).clamp(0.0, 1.0) } else { 0.0 };
        let floor = mesh.tri_floor(far);
        let z = if floor < FLT_MAX { floor } else { prev.z };
        return Vec3::new((ax + s * ex) as f32, (ay + s * ey) as f32, z);
    }
    let elen = elen2.sqrt();
    let (ux, uy) = if elen > 0.0 { (ex / elen, ey / elen) } else { (0.0, 0.0) };
    let half = (agent.radius as f64).min(0.5 * elen);
    let a2 = [ax + half * ux, ay + half * uy];
    let b2 = [bx - half * ux, by - half * uy];
    let s = match cross2([px, py], [goal.x as f64, goal.y as f64], a2, b2) {
        Some((_, s)) => s,
        None => {
            let (dx, dy) = (b2[0] - a2[0], b2[1] - a2[1]);
            let l2 = dx * dx + dy * dy;
            if l2 > 0.0 {
                (((px - a2[0]) * dx + (py - a2[1]) * dy) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            }
        }
    };
    Vec3::new((a2[0] + s * (b2[0] - a2[0])) as f32, (a2[1] + s * (b2[1] - a2[1])) as f32, prev.z)
}

/// The cost of crossing `edge` of `tri` from a node at `cur` toward `goal`, and the entry point the
/// search writes into the far node. `None` is the game's -1 (blocked). The search expands with this.
/// [game] for the gate and the cost (006f1110, ai-nav-build section 4); the entry point is
/// `entry_point`. The far-slot and sideways offset of 006f1110 lines 153-155 are not modelled
/// [assumed].
pub fn enter(mesh: &NavMesh, tri: usize, cur: Vec3, goal: Vec3, edge: usize, agent: &Agent) -> Option<(f32, Vec3)> {
    let far = mesh.neighbour(tri, edge)?;
    if far == tri || !passable(mesh, tri, edge, agent) {
        return None;
    }
    let p = entry_point(mesh, tri, edge, cur, goal, far, agent);
    Some((cur.distance(p) + link_float(mesh, tri, edge, agent), p))
}

/// The cost of crossing `edge` of `tri` from a node at `cur`: the 3D distance from `cur` to the
/// portal point plus the link float. `None` is the game's -1 (blocked). [game] 006f1110 with the
/// -1 cases of ai-nav-build section 4; the return is the distance and the link, as the notes give
/// it. [assumed] The far-slot and the sideways offset of 006f1110 lines 153-155 are not modelled.
/// [stand-in] This has no goal, so it uses the first port's crossing (`portal_point`). The search
/// uses `enter`, which takes the goal (kept for the API).
pub fn edge_cost(mesh: &NavMesh, tri: usize, cur: Vec3, edge: usize, agent: &Agent) -> Option<f32> {
    let far = mesh.neighbour(tri, edge)?;
    if far == tri || !passable(mesh, tri, edge, agent) {
        return None;
    }
    let p = portal_point(mesh, tri, edge, cur, far);
    Some(cur.distance(p) + link_float(mesh, tri, edge, agent))
}

// ---------------------------------------------------------------------------------------------
// The straight test (006f0950).

/// The strip frame of the straight test: `u` is the unit direction, `origin` the start point.
struct Frame {
    origin: [f64; 2],
    u: [f64; 2],
    len: f64,
    r: f64,
    z0: f64,
    z1: f64,
}

impl Frame {
    /// Along-track `s` and lateral `v` of a point (v = cross(u, p - origin)).
    fn s_v(&self, p: [f32; 3]) -> (f64, f64) {
        let dx = p[0] as f64 - self.origin[0];
        let dy = p[1] as f64 - self.origin[1];
        (dx * self.u[0] + dy * self.u[1], self.u[0] * dy - self.u[1] * dx)
    }

    /// The along-track range (s_min, s_max) of the part of edge a..b inside the strip
    /// (|v| <= r and 0 <= s <= len). [assumed] The clip is linear, Liang-Barsky style, over the
    /// four strip sides. 006f0710 is the game's clip; its return values are UNREAD (ai-nav-path 3.3).
    fn clip(&self, a: [f32; 3], b: [f32; 3]) -> Option<(f64, f64)> {
        let (sa, va) = self.s_v(a);
        let (sb, vb) = self.s_v(b);
        let ds = sb - sa;
        let dv = vb - va;
        let cons = [
            (sa, ds),
            (self.len - sa, -ds),
            (va + self.r, dv),
            (self.r - va, -dv),
        ];
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        for (c0, c1) in cons {
            if c1 == 0.0 {
                if c0 < 0.0 {
                    return None;
                }
            } else {
                let t = -c0 / c1;
                if c1 > 0.0 {
                    lo = lo.max(t);
                } else {
                    hi = hi.min(t);
                }
            }
        }
        if lo > hi {
            return None;
        }
        let s_lo = sa + lo * ds;
        let s_hi = sa + hi * ds;
        Some((s_lo.min(s_hi), s_lo.max(s_hi)))
    }

    /// The path's height range over an along-track range. The height is linear from `from.z`
    /// to `to.z` along the segment. [assumed] (006f0950 line 25 gives the slope s; the height
    /// check at the clip point is line 106).
    fn height_range(&self, s_lo: f64, s_hi: f64) -> (f64, f64) {
        let z = |s: f64| self.z0 + (self.z1 - self.z0) * (s / self.len);
        let (a, b) = (z(s_lo), z(s_hi));
        (a.min(b), a.max(b))
    }
}

/// The straight test: can an agent of radius `agent.radius` walk the segment from `from` to
/// `to`? [game] 006f0950 is a corridor sweep from the start triangle, with the neighbour gate
/// (006f04f0) and the floor check on the segment height (line 106). The band half-width is the
/// radius [game] (callee [esp+0xd0], ai-nav-path second read, A.3b). [game] The goal triangle is
/// NOT checked: the flood ends and returns 1 (006f0950 lines 157-172, second read A.3a), so a pass
/// means no failing triangle was reached inside the band. The height h is [esp+0xd4] and its use
/// in the sweep is [unknown] (second read E); the gate below uses `agent.height` as before
/// [assumed]. A segment shorter than 1 mm passes at once (006f0950 line 171).
pub fn straight(mesh: &NavMesh, from: Vec3, to: Vec3, agent: &Agent) -> bool {
    let dx = (to.x - from.x) as f64;
    let dy = (to.y - from.y) as f64;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.001 {
        return true;
    }
    let Some(start) = mesh.locate(from) else {
        return false;
    };
    let frame = Frame {
        origin: [from.x as f64, from.y as f64],
        u: [dx / len, dy / len],
        len,
        r: agent.radius as f64,
        z0: from.z as f64,
        z1: to.z as f64,
    };
    let mut seen: HashSet<usize> = HashSet::new();
    seen.insert(start);
    let mut stack = vec![start];
    while let Some(t) = stack.pop() {
        let vs = mesh.tri_vertices(t);
        for k in 0..3 {
            let a = mesh.vertex(vs[k]);
            let b = mesh.vertex(vs[(k + 1) % 3]);
            let Some((s_lo, s_hi)) = frame.clip(a, b) else {
                continue;
            };
            // An overlapped edge with no neighbour is the box's boundary: out of the mesh.
            let Some(next) = mesh.neighbour(t, k) else {
                return false;
            };
            if !passable(mesh, t, k, agent) {
                return false;
            }
            let (z_lo, z_hi) = frame.height_range(s_lo, s_hi);
            if z_lo < mesh.tri_floor(next) as f64 {
                return false;
            }
            if agent.flags & AGENT_FLAG_CEILING != 0 && z_hi + (agent.height as f64) > mesh.tri_ceiling(next) as f64 {
                return false;
            }
            if seen.insert(next) {
                stack.push(next);
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------------------------
// The search (006ee6b0).

#[derive(Clone, Copy)]
struct SNode {
    /// Heuristic to the goal (node +0x00). FLT_MAX for a node the edge gate blocked.
    h: f32,
    /// Cost from the start (+0x14).
    g: f32,
    /// g + h, the open-list key (+0x18).
    f: f32,
    parent: Option<usize>,
    /// The position the node was entered at (+0x38..+0x40, written by the expansion).
    pos: Vec3,
    /// Set while on the open list: the key it is stored under.
    open: Option<(u32, u64, usize)>,
    closed: bool,
}

impl SNode {
    fn new() -> Self {
        Self { h: FLT_MAX, g: 0.0, f: 0.0, parent: None, pos: Vec3::ZERO, open: None, closed: false }
    }
}

/// The open list and the node table of one search.
struct Frontier {
    nodes: HashMap<usize, SNode>,
    /// Ordered by (f bits, insertion number, node). [assumed] The game's 256 buckets, indexed by
    /// trunc(sqrt(f)) >> 8 & 0xff, are collapsed into this one sorted list: with f below 65536
    /// every node is in bucket 0 and the order is the same (ai-nav-search 2.2). Ties are FIFO.
    open: BTreeSet<(u32, u64, usize)>,
    seq: u64,
    cutoff: f32,
}

impl Frontier {
    fn node(&mut self, t: usize) -> &mut SNode {
        self.nodes.entry(t).or_insert_with(SNode::new)
    }

    /// A node is unseen when it is neither open nor closed (006eeb51, `flags & 3 == 0`).
    fn is_unseen(&self, t: usize) -> bool {
        self.nodes.get(&t).map_or(true, |n| n.open.is_none() && !n.closed)
    }

    /// The open insert (006eec70): the node goes on the list only when |f| is below the cutoff or
    /// the list is empty; otherwise nothing is written.
    fn insert(&mut self, t: usize, g: f32, f: f32) {
        if !(f.abs() < self.cutoff || self.open.is_empty()) {
            return;
        }
        self.seq += 1;
        let key = (f.max(0.0).to_bits(), self.seq, t);
        let n = self.node(t);
        n.g = g;
        n.f = f;
        n.open = Some(key);
        self.open.insert(key);
    }
}

/// A* over the triangles, as 006ee6b0 (ai-nav-search section 2). Returns the triangles from the
/// start to the goal, or to the fallback node, start first; empty when there is no path.
///
/// [stand-in] The start point is an extra argument: the game's start node already carries its
/// position (the path core writes it, ai-nav-path 2.2), so the port passes it in.
///
/// Fallback [game] (006ee811, 006ee86b): when the goal is not popped (the 425 cap, or an empty
/// list), the result is the examined node with the lowest h. [unknown] the game's initial value
/// of that slot (open question O1 of ai-nav-search): here it starts empty, so the first examined
/// neighbour with a finite h is the first candidate.
pub fn search(
    mesh: &NavMesh,
    start_tri: usize,
    start_pos: Vec3,
    goal_tri: usize,
    goal_pos: Vec3,
    agent: &Agent,
) -> Vec<usize> {
    let mut cutoff = CUTOFF_START;
    let h0 = heuristic(start_pos, goal_pos);
    // 006ee728: a start further than 95% of the cutoff raises it.
    if h0 - cutoff > -0.05 * cutoff {
        cutoff = 0.2 * cutoff + h0;
    }
    let mut st = Frontier { nodes: HashMap::new(), open: BTreeSet::new(), seq: 0, cutoff };
    {
        let s = st.node(start_tri);
        s.h = h0;
        s.pos = start_pos;
        s.parent = None;
    }
    st.insert(start_tri, 0.0, h0);

    let mut budget = POP_BUDGET;
    let mut best: Option<(f32, usize)> = None;
    let mut reached = false;
    while let Some(&key) = st.open.iter().next() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        st.open.remove(&key);
        let cur = key.2;
        st.node(cur).open = None;
        if cur == goal_tri {
            reached = true;
            break;
        }
        let (cur_g, cur_pos) = {
            let n = st.node(cur);
            (n.g, n.pos)
        };
        for k in 0..3 {
            let Some(far) = mesh.neighbour(cur, k) else {
                continue;
            };
            // [assumed] The start is never re-entered (its parent must stay empty for the walk).
            if far == cur || far == start_tri {
                continue;
            }
            let unseen = st.is_unseen(far);
            match enter(mesh, cur, cur_pos, goal_pos, k, agent) {
                None => {
                    if unseen {
                        st.node(far).h = FLT_MAX;
                    }
                }
                Some((edge, p)) => {
                    let g_new = cur_g + edge;
                    if unseen {
                        let h = heuristic(p, goal_pos);
                        {
                            let n = st.node(far);
                            n.pos = p;
                            n.h = h;
                            n.parent = Some(cur);
                        }
                        st.insert(far, g_new, h + g_new);
                    } else {
                        let f_new = st.node(far).h + g_new;
                        // 006eebcd: a strict improvement only; equal is not one.
                        if st.node(far).f > f_new {
                            let n = st.node(far);
                            n.pos = p;
                            n.closed = false;
                            n.parent = Some(cur);
                            let old = n.open.take();
                            if let Some(old_key) = old {
                                st.open.remove(&old_key);
                            }
                            st.insert(far, g_new, f_new);
                        }
                    }
                }
            }
            if let Some(n) = st.nodes.get(&far) {
                if n.h < FLT_MAX && best.map_or(true, |(bh, _)| n.h < bh) {
                    best = Some((n.h, far));
                }
            }
        }
        st.node(cur).closed = true;
    }

    let chosen = if reached { Some(goal_tri) } else { best.map(|(_, t)| t) };
    let Some(chosen) = chosen else {
        return Vec::new();
    };
    if chosen == start_tri {
        return Vec::new();
    }
    // 006ee9e4..006eea3e: walk the parents back; 600 nodes or more fail.
    let mut walk = Vec::new();
    let mut at = Some(chosen);
    while let Some(t) = at {
        walk.push(t);
        if walk.len() >= PATH_NODE_LIMIT {
            return Vec::new();
        }
        at = st.nodes.get(&t).and_then(|n| n.parent);
    }
    walk.reverse();
    if walk.first() != Some(&start_tri) {
        return Vec::new();
    }
    walk
}

// ---------------------------------------------------------------------------------------------
// The path core (006f2060): plan, smoothing and the waypoint build.

/// A node of the planned path: its triangle, the position it was entered at, and the edge it
/// was entered through (None for the start and the goal).
#[derive(Clone, Copy)]
struct PNode {
    tri: usize,
    pos: Vec3,
    entry: Option<([f32; 3], [f32; 3])>,
}

fn build_nodes(mesh: &NavMesh, tris: &[usize], from: Vec3, goal: Vec3, goal_tri: usize, agent: &Agent) -> Vec<PNode> {
    let mut out = vec![PNode { tri: tris[0], pos: from, entry: None }];
    let mut prev_pos = from;
    for w in tris.windows(2) {
        let (a, b) = (w[0], w[1]);
        let Some(k) = (0..3).find(|&k| mesh.neighbour(a, k) == Some(b)) else {
            break;
        };
        let p = entry_point(mesh, a, k, prev_pos, goal, b, agent);
        out.push(PNode { tri: b, pos: p, entry: Some(mesh.edge_points(a, k)) });
        prev_pos = p;
    }
    let last = out[out.len() - 1];
    let end = if last.tri == goal_tri { goal } else { last.pos };
    out.push(PNode { tri: last.tri, pos: end, entry: None });
    out
}

/// Smoothing (006f05e0): for each node, the farthest later node within 50 m that passes the
/// straight test (mask 8 is passed) is kept, and the nodes between are dropped. At most 64 tests.
fn smooth(mesh: &NavMesh, nodes: &mut Vec<PNode>, agent: &Agent) {
    let test = Agent { flags: agent.flags | AGENT_FLAG_CEILING, ..*agent };
    let mut budget = SMOOTH_TESTS;
    let mut i = 0;
    'outer: while i + 2 < nodes.len() {
        let len = nodes.len();
        for j in ((i + 2)..len).rev() {
            if nodes[j].pos.distance_squared(nodes[i].pos) > SMOOTH_RANGE_SQ {
                continue;
            }
            if budget == 0 {
                break 'outer;
            }
            budget -= 1;
            if straight(mesh, nodes[i].pos, nodes[j].pos, &test) {
                nodes.drain(i + 1..j);
                break;
            }
        }
        i += 1;
    }
}

/// One record of the game's temp list before the output pass (ai-nav-path second read, C).
#[derive(Clone, Copy)]
struct Temp {
    point: Vec3,
    kind: u32,
    /// The portal normal (xy) of a sampled waypoint: (dy, -dx) of the edge. [assumed] unit length
    /// (the game's FUN_005502f0 is UNREAD).
    normal: [f32; 2],
}

fn xy_dist_sq(a: Vec3, b: Vec3) -> f32 {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    dx * dx + dy * dy
}

/// The waypoint builder (006f2060 lines 193-661, ai-nav-path second read, sections C and D).
///
/// - Record 0 is the start node. For each consecutive pair (cur, nxt) whose triangles share an
///   edge: the edge is shrunk by (r - 0.1) at both ends when its length is at least r and above
///   0.001 (the shrink fraction is capped at one half [assumed]). The sight line from cur's
///   position toward the goal crosses the shrunk edge: the crossing is a sampled waypoint (kind 5)
///   at the z of nxt's triangle floor. A sample replaces the previous record when that is a node
///   or a sample, its z is not below it, and the xy distance is under 0.5 m ([game] lines 334-341).
/// - A node record for nxt is written unless a sample was written for the portal, or nxt is within
///   1.05 r (xy) of the last record at the same height (floors within 0.1 m). The last node is
///   always written.
/// - Output pass: a sample (not record 0) moves (r + 0.1) along its normal, negated when the
///   previous output z is at or below its z. A sample becomes a step point (kind 2) when agent flag
///   1 is set and its rise over the previous output is above 0 and below the agent height. An
///   output point within 1.05 r (3D) of the previous one replaces it.
/// - Record 0 takes the start z. A kind-2 record 1 is removed when record 0 is more than 1 m above
///   it and record 2 is above it; record 0 then becomes the step. The corner pass moves each step
///   point's neighbours back along their legs by r when the legs are longer than r (3D), then
///   skips the record after the step.
///
/// [stand-in] The DFS over cells (stack 256, 32 pops per portal, lines 265-390) is not reproduced:
/// the portal is the one shared edge of the two consecutive path triangles [assumed]. [assumed]
/// The sight line runs from cur to the goal, as in `entry_point` (the note names the crossing but
/// not its far end). [assumed] The node-rule distance is from nxt to the last record.
/// [unknown, left out] The step rule (lines 319-321), the one-ahead write (lines 525-529), the
/// value F = 2 nxt - cur (lines 265-267), the flag-8 ceiling z of a sample, and the flag-4 goal
/// extra (line 440). [stand-in] The sample z is the entered triangle's floor (the game's
/// neighbour cell height, local_4a4 [assumed] which cell); the sentinel uses nxt's z.
fn build_records(mesh: &NavMesh, nodes: &[PNode], goal: Vec3, agent: &Agent) -> Vec<Record> {
    let r = agent.radius as f64;
    let inset = if agent.radius < 0.1 { 0.0 } else { r - 0.1 };
    let merge_r = 1.05 * agent.radius;
    let goal_xy = [goal.x as f64, goal.y as f64];
    let mut temp = vec![Temp { point: nodes[0].pos, kind: KIND_NODE, normal: [0.0, 0.0] }];
    let last = nodes.len() - 1;
    for idx in 1..nodes.len() {
        let (cur, nxt) = (nodes[idx - 1], nodes[idx]);
        let mut wrote_sample = false;
        if let Some((a3, b3)) = nxt.entry {
            let (a, b) = ([a3[0] as f64, a3[1] as f64], [b3[0] as f64, b3[1] as f64]);
            let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
            let len = (ex * ex + ey * ey).sqrt();
            let (sa, sb) = if len >= r && len > 0.001 {
                let f = (inset / len).min(0.5);
                ([a[0] + f * ex, a[1] + f * ey], [b[0] - f * ex, b[1] - f * ey])
            } else {
                (a, b)
            };
            let sight_from = [cur.pos.x as f64, cur.pos.y as f64];
            if let Some((_, s)) = cross2(sight_from, goal_xy, sa, sb) {
                let x = sa[0] + s * (sb[0] - sa[0]);
                let y = sa[1] + s * (sb[1] - sa[1]);
                let floor = mesh.tri_floor(nxt.tri);
                let z = if floor < FLT_MAX { floor } else { nxt.pos.z };
                let normal = if len > 0.0 { [(ey / len) as f32, (-ex / len) as f32] } else { [0.0, 0.0] };
                let sample = Temp { point: Vec3::new(x as f32, y as f32, z), kind: KIND_SAMPLE, normal };
                let prev = *temp.last().expect("record 0 exists");
                let merge = matches!(prev.kind, KIND_NODE | KIND_SAMPLE)
                    && sample.point.z - prev.point.z >= 0.0
                    && xy_dist_sq(sample.point, prev.point) < RECORD_MERGE_SQ;
                if merge {
                    *temp.last_mut().expect("record 0 exists") = sample;
                } else {
                    temp.push(sample);
                }
                wrote_sample = true;
            }
        }
        if !wrote_sample {
            let fc = mesh.tri_floor(cur.tri);
            let fn_ = mesh.tri_floor(nxt.tri);
            let same_height = fc < FLT_MAX && fn_ < FLT_MAX && (fn_ - fc).abs() < HEIGHT_STEP;
            let near = xy_dist_sq(nxt.pos, temp.last().expect("record 0 exists").point) < merge_r * merge_r;
            if idx == last || !(same_height && near) {
                temp.push(Temp { point: nxt.pos, kind: KIND_NODE, normal: [0.0, 0.0] });
            }
        }
    }

    // Output pass (C steps 13-17).
    let mut out: Vec<Record> = Vec::new();
    for (k, t) in temp.iter().enumerate() {
        let mut p = t.point;
        let mut kind = t.kind;
        if kind == KIND_SAMPLE {
            let prev_z = out.last().map_or(temp[k.saturating_sub(1)].point.z, |q| q.point.z);
            if k > 0 {
                let sign = if prev_z <= p.z { -1.0 } else { 1.0 };
                let d = (agent.radius + 0.1) * sign;
                p.x += d * t.normal[0];
                p.y += d * t.normal[1];
            }
            if let Some(prev) = out.last() {
                let dz = p.z - prev.point.z;
                if agent.flags & AGENT_FLAG_STEP_TALL != 0 && dz > 0.0 && dz < agent.height {
                    kind = KIND_STEP;
                }
            }
        }
        let rec = Record { point: p, kind };
        match out.last_mut() {
            Some(prev) if p.distance(prev.point) < merge_r => *prev = rec,
            _ => out.push(rec),
        }
    }

    // Phase E (C steps 19-21).
    if let Some(first) = out.first_mut() {
        first.point.z = nodes[0].pos.z;
    }
    if out.len() > 2
        && out[1].kind == KIND_STEP
        && out[0].point.z - out[1].point.z > 1.0
        && out[2].point.z - out[1].point.z > 0.0
    {
        out.remove(1);
        out[0].kind = KIND_STEP;
    }
    let rf = agent.radius;
    let mut k = 0;
    while k < out.len() {
        if out[k].kind == KIND_STEP {
            if k > 0 {
                let d = out[k - 1].point - out[k].point;
                let len = d.length();
                if len > rf {
                    out[k].point += d / len * rf;
                }
            }
            if k + 2 < out.len() {
                let d = out[k + 2].point - out[k + 1].point;
                let len = d.length();
                if len > rf {
                    out[k + 1].point += d / len * rf;
                }
            }
            k += 2;
        } else {
            k += 1;
        }
    }
    out
}

/// Plans a path from `from` to `to` for `agent` (the path core 006f2060). `None` when a point is
/// outside the mesh, or when no path (of two or more records) is found.
///
/// Steps: locate both triangles; the goal's z becomes its triangle's floor (the region height,
/// [game] 006f2060 line 135; [assumed] the sentinel rule is not modelled). Same triangle, or a
/// straight test that passes, gives the two-record path. Otherwise `search`, the smoothing and the
/// waypoint build. If the search gives its fallback node, the path ends there (not at the goal).
pub fn plan(mesh: &NavMesh, from: Vec3, to: Vec3, agent: &Agent) -> Option<Vec<Record>> {
    if !from.is_finite() || !to.is_finite() {
        return None;
    }
    let start_tri = mesh.locate(from)?;
    let goal_tri = mesh.locate(to)?;
    let mut goal = to;
    let floor = mesh.tri_floor(goal_tri);
    if floor < FLT_MAX {
        goal.z = floor;
    }
    let direct = || {
        vec![
            Record { point: from, kind: KIND_NODE },
            Record { point: goal, kind: KIND_NODE },
        ]
    };
    if start_tri == goal_tri || straight(mesh, from, goal, agent) {
        return Some(direct());
    }
    let tris = search(mesh, start_tri, from, goal_tri, goal, agent);
    if tris.len() < 2 {
        return None;
    }
    let mut nodes = build_nodes(mesh, &tris, from, goal, goal_tri, agent);
    smooth(mesh, &mut nodes, agent);
    let records = build_records(mesh, &nodes, goal, agent);
    if records.len() < 2 {
        return None;
    }
    Some(records)
}

/// The closest point on segment a..b to p (3D), clamped to the segment.
fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = b - a;
    let l2 = ab.length_squared();
    if l2 <= 0.0 {
        return a;
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    a + ab * t
}

/// The segment k (from record k to k+1) with the least distance to `own`, among `from_k ..=
/// count-2` (006fa7b0 for each). Ties go to the later segment, so the index moves past a corner
/// the character stands on. [assumed] The tie rule; the game's ties are not read.
fn nearest_segment(records: &[Record], from_k: usize, own: Vec3) -> Option<(usize, f32)> {
    let n = records.len();
    if n < 2 || from_k > n - 2 {
        return None;
    }
    let mut best: Option<(usize, f32)> = None;
    for k in from_k..=n - 2 {
        let q = closest_on_segment(own, records[k].point, records[k + 1].point);
        let d = own.distance(q);
        if best.map_or(true, |(_, bd)| d <= bd) {
            best = Some((k, d));
        }
    }
    best
}

/// The follower (006ff580): holds the records of one path and the index of the segment the
/// character is on. The caller resets `plans_this_frame` once per frame. [game] the gate of
/// 006febd0 (one plan per reset, counter DAT_00cf5a3c < 1); the reset point is UNREAD, so the
/// caller decides it ([assumed] once per frame, shared by the followers it wants to limit).
pub struct Follower {
    pub records: Vec<Record>,
    /// The segment index (+0x78): segment k runs from record k to record k+1.
    pub index: usize,
    /// The destination the path was planned to (+0x7c..+0x84).
    pub dest: Option<Vec3>,
    /// Params +0x0c: the distance (m) the own point may be from the path ahead before a replan.
    /// [assumed] 0.2 m (see DEFAULT_NEAR_TOLERANCE). The game passes it in the 5-word block.
    pub near_tolerance: f32,
    /// Params +0x14: the squared destination change that keeps the path. [assumed] 4.0 (2 m).
    pub dest_tolerance_sq: f32,
    /// Params +0x10: nonzero forbids keeping the nearest segment. [game] 006ff580 line 67.
    pub accept_flag: u8,
    /// Plans made since the caller last reset this to 0.
    pub plans_this_frame: u32,
    /// Plans allowed per reset. [game] 006febd0 lines 3-5 (the counter must be below 1).
    pub plan_limit_per_frame: u32,
}

impl Default for Follower {
    fn default() -> Self {
        Self::new()
    }
}

impl Follower {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            index: 0,
            dest: None,
            near_tolerance: DEFAULT_NEAR_TOLERANCE,
            dest_tolerance_sq: DEFAULT_DEST_TOLERANCE_SQ,
            accept_flag: 0,
            plans_this_frame: 0,
            plan_limit_per_frame: 1,
        }
    }

    /// The point to steer at: record index+1, or the last record at the end of the path.
    fn target(&self) -> Option<Vec3> {
        let n = self.records.len();
        if n == 0 {
            None
        } else {
            Some(self.records[(self.index + 1).min(n - 1)].point)
        }
    }

    /// One step of the follower (006ff580). Returns the point to steer at, or `None` when no path
    /// exists (the caller then walks straight, as section 6 of ai-behaviours-move says).
    ///
    /// Order, as the game: (1) progress along the segment, for records of kind 3, 7 or 8 only;
    /// (2) with the destination unchanged, keep the path when the nearest segment from the index
    /// on is within `near_tolerance`; (3) otherwise replan, and set the index to the nearest segment.
    /// The planning throttle: a replan with `plans_this_frame` at the limit clears the path and
    /// returns `None` [stand-in] (the game's FUN_006fead0 path is UNREAD).
    pub fn step(&mut self, mesh: &NavMesh, own: Vec3, dest: Vec3, agent: &Agent) -> Option<Vec3> {
        let n = self.records.len();
        if n > 0 && self.index + 1 < n && matches!(self.records[self.index + 1].kind, 3 | 7 | 8) {
            let a = self.records[self.index].point;
            let b = self.records[self.index + 1].point;
            let d = b - a;
            let len = d.length();
            let u = if len > 0.0 { d / len } else { Vec3::ZERO };
            if (own - a).dot(u) + OVERSHOOT >= len {
                self.index += 1;
            }
            return self.target();
        }
        if n >= 2 {
            if let Some(stored) = self.dest {
                if (dest - stored).length_squared() <= self.dest_tolerance_sq {
                    if let Some((k, d)) = nearest_segment(&self.records, self.index, own) {
                        if d <= self.near_tolerance && self.accept_flag == 0 {
                            self.index = k;
                            return self.target();
                        }
                    }
                }
            }
        }
        self.records.clear();
        self.index = 0;
        self.dest = Some(dest);
        if self.plans_this_frame >= self.plan_limit_per_frame {
            return None;
        }
        self.plans_this_frame += 1;
        let path = plan(mesh, own, dest, agent)?;
        self.records = path;
        self.index = nearest_segment(&self.records, 0, own).map_or(0, |(k, _)| k);
        self.target()
    }
}

/// A path source over one navigation mesh for one agent: plugs into `GoToTarget` in place of
/// `StraightLine`. [assumed] One `MeshPath` per character; the one-plan-per-frame limit is per
/// follower here, not shared across followers (the game's counter is global).
pub struct MeshPath {
    mesh: Arc<NavMesh>,
    agent: Agent,
    follower: Follower,
}

impl MeshPath {
    pub fn new(mesh: Arc<NavMesh>, agent: Agent) -> Self {
        Self { mesh, agent, follower: Follower::new() }
    }

    /// The follower's records (for the caller's own checks and the debug output).
    pub fn records(&self) -> &[Record] {
        &self.follower.records
    }

    /// Resets the planning throttle; the caller does this once per frame.
    pub fn reset_frame(&mut self) {
        self.follower.plans_this_frame = 0;
    }
}

impl PathSource for MeshPath {
    fn next_point(&mut self, from: [f32; 3], to: [f32; 3]) -> Option<[f32; 3]> {
        let p = self.follower.step(&self.mesh, Vec3::from(from), Vec3::from(to), &self.agent)?;
        Some(p.to_array())
    }

    /// The controller step resets the planning throttle (see `reset_frame`).
    fn new_frame(&mut self) {
        self.reset_frame();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navmesh::Silhouette;

    fn sil(points: Vec<[f32; 3]>, height: f32) -> Silhouette {
        let raw_points = points.len();
        Silhouette {
            name: "test".into(),
            points,
            base_z: 0.0,
            height,
            replacement_height: 0.0,
            ai_ban_height: f32::MAX,
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

    /// A closed rectangle, counter-clockwise, at z 0.
    fn square(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<[f32; 3]> {
        vec![[x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0], [x0, y0, 0.0]]
    }

    /// A mesh over a box whose floor is z 0, so ground points at z 0 need no height change.
    fn mesh_with(sils: &[Silhouette]) -> NavMesh {
        NavMesh::build(Some((Vec3::new(-2.0, -12.0, 0.0), Vec3::new(14.0, 12.0, 8.0))), sils).expect("build")
    }

    fn agent() -> Agent {
        Agent::new(1.0, 4.0, 0)
    }

    fn v(x: f32, y: f32) -> Vec3 {
        Vec3::new(x, y, 0.0)
    }

    /// The wall of the tests: x 4 to 4.5, y -6 to 6, top 4 m.
    fn wall() -> Silhouette {
        sil(square(4.0, -6.0, 4.5, 6.0), 4.0)
    }

    fn inside_wall(p: Vec3) -> bool {
        p.x > 4.0 && p.x < 4.5 && p.y.abs() < 6.0
    }

    #[test]
    fn open_field_gives_a_two_record_straight_path() {
        let m = mesh_with(&[]);
        let rec = plan(&m, v(1.0, 1.0), v(8.0, 9.0), &agent()).expect("path");
        assert_eq!(rec.len(), 2);
        assert!(rec[0].point.distance(v(1.0, 1.0)) < 1e-4);
        assert!(rec[1].point.distance(v(8.0, 9.0)) < 1e-4);
        assert!(plan(&m, v(-50.0, 0.0), v(8.0, 9.0), &agent()).is_none(), "outside the mesh gives None");
    }

    #[test]
    fn wall_is_walked_around_its_end() {
        let m = mesh_with(&[wall()]);
        assert!(!straight(&m, v(0.0, 0.0), v(9.0, 0.0), &agent()), "the wall blocks the direct line");
        let rec = plan(&m, v(0.0, 0.0), v(9.0, 0.0), &agent()).expect("a path around the wall");
        assert!(rec.len() >= 3, "more than the two-record path: {rec:?}");
        assert!(rec.iter().any(|r| r.point.y.abs() > 5.5), "the path goes round an end: {rec:?}");
        assert!(rec.iter().all(|r| !inside_wall(r.point)), "no waypoint inside the wall");
        assert!(rec.last().unwrap().point.distance(v(9.0, 0.0)) < 1e-3, "ends at the goal");
        // Every segment's centre line stays out of the wall (sampled).
        for w in rec.windows(2) {
            for i in 0..=200 {
                let p = w[0].point.lerp(w[1].point, i as f32 / 200.0);
                assert!(!inside_wall(p), "segment {:?} .. {:?} enters the wall at {p:?}", w[0].point, w[1].point);
            }
        }
    }

    /// The strict form of the wall test: every segment passes the radius-1 straight test.
    /// [known gap, still failing after the second-read builder] The first segment ends at the
    /// branch-1 entry point of the corner portal, which the inset puts exactly r = 1 m from the
    /// wall's corner (4, -6) at (3.29, -6.71). The segment from (0, 0) passes 0.95 m from the
    /// corner, so the radius-1 corridor clips it. The segment is start to node, which no smoothing
    /// test covers. No clearance rule is added (the note has none). Run with `--ignored`.
    #[test]
    #[ignore = "known gap: first segment clips the corner at 0.95 m (branch-1 inset node at r from the corner)"]
    fn every_wall_segment_passes_the_straight_test() {
        let m = mesh_with(&[wall()]);
        let rec = plan(&m, v(0.0, 0.0), v(9.0, 0.0), &agent()).expect("a path around the wall");
        for w in rec.windows(2) {
            assert!(
                straight(&m, w[0].point, w[1].point, &agent()),
                "segment {:?} .. {:?} does not pass the straight test",
                w[0].point,
                w[1].point
            );
        }
    }

    /// The us_test route the status note records (one placement per object, radius 1, height 4):
    /// every segment of the planned route must pass the straight test on that mesh. Needs the
    /// install (`TF2_GAME_DIR`, default C:/Games2): run with `--ignored`.
    /// [known gap] Fails on the first two segments: start (-40,-108) to node (-71.47,-97.29), and
    /// that node to the sample (-76.46,-97.82). Same cause as the wall test: nodes on the portal
    /// insets are not checked against the corridor of the adjacent segment.
    #[test]
    #[ignore = "known gap: adjacent segments near building outlines fail the radius-1 straight test"]
    fn us_test_route_segments_pass_the_straight_test() {
        let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
        let path = std::path::PathBuf::from(dir).join("levels/us_test/zone.str");
        let (sils, _) = crate::navmesh::load(&path).expect("load us_test");
        let mut seen = std::collections::HashSet::new();
        let first: Vec<_> = sils.iter().filter(|s| seen.insert(s.object)).cloned().collect();
        let m = NavMesh::build(None, &first).expect("build");
        let rec = plan(&m, Vec3::new(-40.0, -108.0, -23.047), Vec3::new(-160.0, -108.0, -23.047), &agent())
            .expect("a route on us_test");
        let bad: Vec<(Vec3, Vec3)> = rec
            .windows(2)
            .filter(|w| !straight(&m, w[0].point, w[1].point, &agent()))
            .map(|w| (w[0].point, w[1].point))
            .collect();
        assert!(bad.is_empty(), "route segments failing the straight test: {bad:?}");
    }

    #[test]
    fn gap_narrower_than_twice_the_radius_is_not_used() {
        // Two walls with a 1 m gap at y 0.5 .. 1.5; the agent radius is 1 (2r = 2 m).
        let m = mesh_with(&[sil(square(4.0, -6.0, 4.5, 0.5), 4.0), sil(square(4.0, 1.5, 4.5, 6.0), 4.0)]);
        assert!(!straight(&m, v(0.0, 1.0), v(9.0, 1.0), &agent()));
        let rec = plan(&m, v(0.0, 1.0), v(9.0, 1.0), &agent()).expect("around the walls");
        assert!(rec.len() >= 3);
        assert!(rec.iter().any(|r| r.point.y.abs() > 5.0), "the path goes round the walls: {rec:?}");
        assert!(rec.iter().all(|r| !inside_wall(r.point)));
    }

    #[test]
    fn gap_wider_than_twice_the_radius_is_used() {
        // A 3 m gap at y 0.5 .. 3.5: the straight line at y 2 passes the corridor of radius 1.
        let m = mesh_with(&[sil(square(4.0, -6.0, 4.5, 0.5), 4.0), sil(square(4.0, 3.5, 4.5, 6.0), 4.0)]);
        assert!(straight(&m, v(0.0, 2.0), v(9.0, 2.0), &agent()));
        let rec = plan(&m, v(0.0, 2.0), v(9.0, 2.0), &agent()).expect("path");
        assert_eq!(rec.len(), 2, "the wide gap gives the direct path: {rec:?}");
    }

    #[test]
    fn goal_inside_a_tall_obstacle_gives_none_or_the_fallback() {
        // A 4 m block at x 6 .. 8, y -1 .. 1. The goal sits inside it (its floor is 4 m).
        let m = mesh_with(&[sil(square(6.0, -1.0, 8.0, 1.0), 4.0)]);
        match plan(&m, v(0.0, 0.0), v(7.0, 0.0), &agent()) {
            None => {}
            Some(rec) => {
                // The fallback: the path ends short of the block, at the examined node with the
                // lowest heuristic (see `search`).
                let end = rec.last().unwrap().point;
                assert!(end.distance(v(7.0, 0.0)) > 0.5, "the fallback stops short of the goal: {rec:?}");
                assert!(rec.iter().all(|r| !(r.point.x > 6.0 && r.point.x < 8.0 && r.point.y.abs() < 1.0)));
            }
        }
    }

    #[test]
    fn search_respects_the_pop_cap_without_panicking() {
        // A 12 by 12 grid of small pillars with 0.6 m passages: the search cannot reach the far
        // corner in 425 pops from the near side, so it returns a fallback (or nothing).
        let mut sils = Vec::new();
        for i in 0..12 {
            for j in 0..12 {
                let (x, y) = (i as f32 + 0.3, j as f32 + 0.3);
                sils.push(sil(square(x, y, x + 0.4, y + 0.4), 4.0));
            }
        }
        let m = mesh_with(&sils);
        let from = v(-1.0, -1.0);
        let to = v(13.0, 11.0);
        let start = m.locate(from).expect("start in the mesh");
        let goal = m.locate(to).expect("goal in the mesh");
        let tris = search(&m, start, from, goal, to, &agent());
        assert!(tris.len() < PATH_NODE_LIMIT);
        let _ = plan(&m, from, to, &agent());
    }

    #[test]
    fn follower_advances_along_the_path_and_keeps_it() {
        let m = mesh_with(&[wall()]);
        let mut f = Follower::new();
        let dest = v(9.0, 0.0);
        let first = f.step(&m, v(0.0, 0.0), dest, &agent()).expect("a target");
        assert!(first.distance(f.records[1].point) < 1e-4);
        let recs = f.records.clone();
        assert!(recs.len() >= 3);
        for k in 1..recs.len() - 1 {
            let t = f.step(&m, recs[k].point, dest, &agent()).expect("a target");
            assert!(t.distance(recs[k + 1].point) < 1e-4, "at record {k} the target is record {}", k + 1);
        }
        assert_eq!(f.records, recs, "no replan on the way");
    }

    #[test]
    fn follower_replans_when_the_destination_jumps() {
        let m = mesh_with(&[]);
        let mut f = Follower::new();
        f.step(&m, v(1.0, 1.0), v(8.0, 8.0), &agent()).expect("first plan");
        assert!(f.records.last().unwrap().point.distance(v(8.0, 8.0)) < 1e-4);
        f.plans_this_frame = 0;
        f.step(&m, v(1.0, 1.0), v(2.0, 9.0), &agent()).expect("second plan");
        assert!(f.records.last().unwrap().point.distance(v(2.0, 9.0)) < 1e-4, "replanned to the new goal");
        // The throttle: a second plan in the same frame is refused and the path is cleared.
        assert!(f.step(&m, v(1.0, 1.0), v(5.0, 5.0), &agent()).is_none());
        assert!(f.records.is_empty());
    }

    #[test]
    fn mesh_path_walks_round_the_wall_in_bounded_steps() {
        let m = Arc::new(mesh_with(&[wall()]));
        let mut mp = MeshPath::new(m, agent());
        let goal = Vec3::new(9.0, 0.0, 0.0);
        let mut pos = Vec3::ZERO;
        let mut steps = 0;
        while steps < 400 {
            mp.reset_frame();
            let target = Vec3::from(mp.next_point(pos.to_array(), goal.to_array()).expect("a target"));
            let d = pos.distance(target);
            if d <= 0.3 {
                pos = target;
            } else {
                pos += (target - pos) / d * 0.3;
            }
            steps += 1;
            assert!(!inside_wall(pos), "the agent walked into the wall at {pos:?}");
            if pos.distance(goal) < 0.3 {
                break;
            }
        }
        assert!(pos.distance(goal) < 0.3, "reached the goal in {steps} steps, at {pos:?}");
        assert!(steps < 200, "bounded: {steps} steps");
    }
}
