//! AINode: the level's AI nodes (phase 3). A node is an attachment of an Instance in a zone or
//! mission pack (3,106 objects in the install); the game keeps every node
//! in one global list and the path nodes in a second, links them once after the mission's nav
//! build, and uses them three ways:
//!   - a nearest / farthest search with required and excluded type flags (`find`, 0082dbe0), used by
//!     MoveOnWayPoints (waypoint nodes) and Teleport (teleport nodes);
//!   - an A* route over the path nodes (`route`, 0082cfd0), used by GoToTarget when its destination
//!     is over 200 m away (400 m in a car) to pick an intermediate node (`far_pick`);
//!   - the next linked waypoint (`next_waypoint`, 0082dbb0), which MoveOnWayPoints follows.
//!
//! Source: the unpublished notes (this port's read, 2026-10-16). Tags: [game] read in the code, [data]
//! read from a pack, [assumed] and [stand-in] where they apply; every assumption is also in
//! the unpublished notes "Phase 3, AI nodes". Read only from the user's install at run time.

use std::collections::HashSet;
use std::path::Path;

use crate::formats::hash::crc32;
use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};

/// The AINode class name (type hash 242cda35). [game] 0082c610.
pub const AINODE_TYPE: &str = "AINode";

/// Type flags at node+0x40. [game] the loader 0082c950.
pub const NODE_EXIT: u32 = 0x1;
pub const NODE_JUMP: u32 = 0x2;
pub const NODE_PATH: u32 = 0x4;
pub const NODE_SNIPER: u32 = 0x8;
pub const NODE_TELEPORT: u32 = 0x10;
pub const NODE_WAYPOINT: u32 = 0x20;
/// nodeSize flags, same word. [game] 0082c950.
pub const SIZE_EXTRA_SMALL: u32 = 0x40;
pub const SIZE_SMALL: u32 = 0x80;
pub const SIZE_MEDIUM: u32 = 0x100;
pub const SIZE_LARGE: u32 = 0x200;
pub const SIZE_EXTRA_LARGE: u32 = 0x400;

/// The search kinds (a target spec's `targetCalc`, spec+0xc). [game] 0082dbe0: d812eb39 keeps the
/// NEAREST node in the band, 69f1cb68 the FARTHEST. The names are not in names.tsv.
pub const CALC_NEAREST: u32 = 0xd812eb39;
pub const CALC_FARTHEST: u32 = 0x69f1cb68;

/// The band of the route's two node searches: 0 to 5000 m. [game] 0082cfd0 (0x459c4000).
pub const ROUTE_RANGE: f32 = 5000.0;
/// GoToTarget asks for a node route when the destination is farther than this on foot. [game]
/// 0081ccb0 (200.0).
pub const FAR_TARGET_FOOT: f32 = 200.0;
/// The route's intermediate node must be farther than this from the character on foot. [game]
/// 0081ccb0 (100.0).
pub const FAR_NODE_FOOT: f32 = 100.0;
/// Both distances in a car (`is_in_car_type` 00725890): the global DAT_00cad468, 400 in the file.
/// [game] for the use; [assumed] the global keeps its file value (it sits in .data, writer unread).
pub const FAR_CAR: f32 = 400.0;
/// The tolerance of the cylinder test 0056f7d0: x^2 + y^2 <= 1.01 and |z| <= 1.01 in the node's
/// unit frame. [game]
const CYLINDER_TOL: f32 = 1.01;

/// One AINode as the data writes it. [data] the fields of the AINode attachment; the place is the
/// host Instance's translation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AiNodeData {
    /// The host Instance's name hash and name (`ConvoyNode_00B`, `NavNode01`...).
    pub instance: u32,
    pub name: String,
    /// The node's place. [game] the loader takes the owner's world translation (owner+0x34..0x3c,
    /// 0082c950); [assumed] that is the host Instance's matrix row 3 (no parent above the
    /// Instance), as the spawner reader assumes.
    pub pos: [f32; 3],
    /// nodeGroupID (node+0x38). [data] 0 in the examples; the group join 007e9950 is not ported.
    pub group_id: u32,
    /// nodeRadius (+0x44) and nodeHeight (+0x48), metres. [data]
    pub radius: f32,
    pub height: f32,
    /// The type and size flags (+0x40). [game] 0082c950.
    pub flags: u32,
    /// connections (+0x4c), disconnections (+0x58), autoConnections (+0x64): the linked Instances'
    /// name hashes in the data's order. [data]
    pub connections: Vec<u32>,
    pub disconnections: Vec<u32>,
    pub auto_connections: Vec<u32>,
    /// Fields that were absent or of an unexpected kind.
    pub missing: Vec<String>,
}

/// The nodeSize enum value's flag. [game] 0082c950: five names; any other value sets none (the
/// install also writes `#00000000` and other hashes, which set none).
pub fn size_flag(hash: u32) -> u32 {
    match hash {
        0x05713d6d => SIZE_EXTRA_SMALL,
        0x821128a3 => SIZE_SMALL,
        0x368b8231 => SIZE_MEDIUM,
        0xae3feb0e => SIZE_LARGE,
        0x295ffec0 => SIZE_EXTRA_LARGE,
        _ => 0,
    }
}

/// A node at run time.
#[derive(Clone, Debug)]
pub struct AiNode {
    pub data: AiNodeData,
    /// node+0x3c: the active byte. [game] the ctor sets 1 (0082c610); its other writers are not read,
    /// so it stays true here [assumed].
    pub active: bool,
    /// node+0x90: the character that holds the node (Teleport's reservation), None when free.
    /// [game] read by 0082dbe0; its writer (the teleport) is not ported.
    pub held_by: Option<u32>,
    /// node+0x70: the links `build_links` made, in insertion order (node indices). [game] 0082e230.
    pub links: Vec<usize>,
}

/// The search record of 0082dbe0 (a target spec): the kind and the distance band. [game]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeSearch {
    /// spec+0xc: CALC_NEAREST or CALC_FARTHEST; any other kind finds nothing.
    pub calc: u32,
    /// spec+0x10 and +0x14, metres: the node's distance must lie in [min, max] (compared squared).
    pub min: f32,
    pub max: f32,
}

/// A route from 0082cfd0: the path nodes from the start node to the goal node, and the point the
/// function returns (the node after the start, or the own point when there is no route). [game]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Route {
    /// controller+0x19c in route order, start first. Empty when there is no route.
    pub nodes: Vec<usize>,
    pub next: [f32; 3],
}

/// Every node of a level in the global list's order, and the path list. [game] 0082c610 appends
/// each new node to the ring 00c06d48; 0082c950 appends path nodes to 00c06d54.
#[derive(Clone, Debug, Default)]
pub struct AiNodeGraph {
    pub nodes: Vec<AiNode>,
    /// Indices of the nodes with NODE_PATH, in list order (the ring 00c06d54).
    pub path: Vec<usize>,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len_sq(v: [f32; 3]) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    len_sq(sub(a, b)).sqrt()
}

/// A* node states (node+0x88). [game] 0082cfd0: 0 unvisited, 1 closed, 2 open; the ctor's -1 reads
/// as unvisited.
#[derive(Clone, Copy, PartialEq)]
enum State {
    Unvisited,
    Closed,
    Open,
}

impl AiNodeGraph {
    /// The graph of `data` in the given order, linked as the mission load does it. [assumed] the
    /// data walk's order is the game's creation order (the list order decides ties in the searches).
    pub fn new(data: Vec<AiNodeData>) -> Self {
        let nodes: Vec<AiNode> = data
            .into_iter()
            .map(|d| AiNode { data: d, active: true, held_by: None, links: Vec::new() })
            .collect();
        let path = (0..nodes.len()).filter(|&i| nodes[i].data.flags & NODE_PATH != 0).collect();
        let mut g = Self { nodes, path };
        g.build_links();
        g
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn pos(&self, i: usize) -> [f32; 3] {
        self.nodes[i].data.pos
    }

    /// The first node attached to the Instance `hash`. [game] the link targets are resolved by
    /// object and then its AINode component (005784b0, 005781b0); [assumed] the first AINode of the
    /// Instance in list order when it has several.
    pub fn by_instance(&self, hash: u32) -> Option<usize> {
        self.nodes.iter().position(|n| n.data.instance == hash)
    }

    /// 0082e230 [game]: `a` links to `b` unless the link is already there; appended at the end.
    fn link(&mut self, a: usize, b: usize) {
        if !self.nodes[a].links.contains(&b) {
            self.nodes[a].links.push(b);
        }
    }

    /// The mission load's link pass (00756190 after the nav build): for each path node in path
    /// order, its links are cleared and rebuilt by 0082de30. [game]
    ///   - A node without NODE_WAYPOINT links to every other path node that is not a waypoint and
    ///     whose place is inside its vertical cylinder: radius `nodeRadius`, half height
    ///     `nodeHeight`, both scaled by the 1.01 tolerance of 0056f7d0 (x^2 + y^2 <= 1.01 and
    ///     |z| <= 1.01 in the unit frame). Its connection lists are not read.
    ///   - A waypoint node links to its autoConnections, then its connections, then drops its
    ///     disconnections. Links are one way (a to b).
    ///
    /// Nodes that are not path nodes get no links. [game]
    pub fn build_links(&mut self) {
        for &i in &self.path.clone() {
            self.nodes[i].links.clear();
            let d = self.nodes[i].data.clone();
            if d.flags & NODE_WAYPOINT == 0 {
                // [assumed] a zero radius or height (a singular frame) links nothing: the inverse of
                // 0056f540 on a singular matrix is not read.
                if d.radius == 0.0 || d.height == 0.0 {
                    continue;
                }
                for &j in &self.path.clone() {
                    if j == i || self.nodes[j].data.flags & NODE_WAYPOINT != 0 {
                        continue;
                    }
                    let v = sub(self.nodes[j].data.pos, d.pos);
                    let (x, y, z) = (v[0] / d.radius, v[1] / d.radius, v[2] / d.height);
                    if x * x + y * y <= CYLINDER_TOL && z.abs() <= CYLINDER_TOL {
                        self.link(i, j);
                    }
                }
            } else {
                for h in d.auto_connections.iter().chain(d.connections.iter()) {
                    if let Some(j) = self.by_instance(*h) {
                        self.link(i, j);
                    }
                }
                for h in &d.disconnections {
                    if let Some(j) = self.by_instance(*h) {
                        self.nodes[i].links.retain(|&k| k != j);
                    }
                }
            }
        }
    }

    /// 0082dbe0 [game]: over every node in list order, the active ones whose flags hold all of
    /// `required` and none of `excluded`, and (when `holder` is given) that no other character
    /// holds; of those whose squared distance from `from` lies in [min^2, max^2], the nearest
    /// (CALC_NEAREST, strict: the first of equals wins) or the farthest (CALC_FARTHEST). Any other
    /// kind returns None.
    pub fn find(&self, from: [f32; 3], spec: NodeSearch, required: u32, excluded: u32, holder: Option<u32>) -> Option<usize> {
        let mut best = match spec.calc {
            CALC_NEAREST => f32::MAX,
            _ => -1.0,
        };
        let mut found = None;
        for (i, n) in self.nodes.iter().enumerate() {
            let flags = n.data.flags;
            if flags & excluded != 0 || flags & required != required || !n.active {
                continue;
            }
            if let (Some(me), Some(h)) = (holder, n.held_by)
                && h != me {
                continue;
            }
            let d = len_sq(sub(n.data.pos, from));
            if d < spec.min * spec.min || d > spec.max * spec.max {
                continue;
            }
            let better = match spec.calc {
                CALC_NEAREST => d < best,
                CALC_FARTHEST => d > best,
                _ => false,
            };
            if better {
                best = d;
                found = Some(i);
            }
        }
        found
    }

    /// The nearest active path node that is not a waypoint, within 5000 m of `p`. [game] the two
    /// searches of 0082cfd0 (required NODE_PATH, excluded NODE_WAYPOINT, CALC_NEAREST, 0 to 5000).
    fn route_end(&self, p: [f32; 3]) -> Option<usize> {
        self.find(p, NodeSearch { calc: CALC_NEAREST, min: 0.0, max: ROUTE_RANGE }, NODE_PATH, NODE_WAYPOINT, None)
    }

    /// 0082cfd0 [game]: an A* over the path nodes from the node nearest `own` to the node nearest
    /// `dest`. Waypoint and inactive path nodes are closed from the start; the cost is the 3D
    /// distance between node places, the estimate the 3D distance to the goal node. The open node
    /// with the least cost plus estimate is expanded next (the first opened wins a tie). On success
    /// the route runs start to goal and `next` is the node after the start; when either end is
    /// missing, the ends are the same node, or the goal is not reached, the route is empty and
    /// `next` is `own`. [assumed] `own` is the character's camera target point 0071a2e0 (its own
    /// place on foot).
    pub fn route(&self, own: [f32; 3], dest: [f32; 3]) -> Route {
        let none = Route { nodes: Vec::new(), next: own };
        let Some(start) = self.route_end(own) else { return none };
        let Some(goal) = self.route_end(dest) else { return none };
        if start == goal {
            return none;
        }
        let n = self.nodes.len();
        let mut state = vec![State::Unvisited; n];
        let mut g = vec![0.0f32; n];
        let mut h = vec![0.0f32; n];
        let mut parent: Vec<Option<usize>> = vec![None; n];
        for &i in &self.path {
            let node = &self.nodes[i];
            if node.data.flags & NODE_WAYPOINT != 0 || !node.active {
                state[i] = State::Closed;
            }
        }
        state[start] = State::Closed;
        let goal_pos = self.pos(goal);
        let mut open: Vec<usize> = Vec::new();
        let mut cur = start;
        loop {
            for &nb in &self.nodes[cur].links {
                match state[nb] {
                    State::Closed => {}
                    State::Open => {
                        let cost = dist(self.pos(cur), self.pos(nb)) + g[cur];
                        if cost < g[nb] {
                            g[nb] = cost;
                            parent[nb] = Some(cur);
                        }
                    }
                    State::Unvisited => {
                        state[nb] = State::Open;
                        h[nb] = dist(self.pos(nb), goal_pos);
                        parent[nb] = Some(cur);
                        g[nb] = dist(self.pos(nb), self.pos(cur)) + g[cur];
                        open.push(nb);
                    }
                }
            }
            let mut pick: Option<(usize, f32)> = None;
            for (k, &o) in open.iter().enumerate() {
                let f = g[o] + h[o];
                if f < pick.map_or(f32::MAX, |p| p.1) {
                    pick = Some((k, f));
                }
            }
            let Some((k, _)) = pick else { return none };
            cur = open.remove(k);
            state[cur] = State::Closed;
            if cur == goal {
                break;
            }
        }
        let mut nodes = vec![goal];
        let mut at = goal;
        while let Some(p) = parent[at] {
            nodes.push(p);
            if p == start {
                break;
            }
            at = p;
        }
        nodes.reverse();
        let next = self.pos(nodes[1]);
        Route { nodes, next }
    }

    /// 0082dbb0 [game]: the first node linked from `i` (in link order) that is an active waypoint.
    /// No distance test.
    pub fn next_waypoint(&self, i: usize) -> Option<usize> {
        self.nodes[i]
            .links
            .iter()
            .copied()
            .find(|&j| self.nodes[j].data.flags & NODE_WAYPOINT != 0 && self.nodes[j].active)
    }

    /// GoToTarget's pick over the node route (0081ccb0, the branch after 0082cfd0) [game]: from the
    /// remembered node's place in the route (or the start when it is not in it, or none is
    /// remembered), the first node farther than 100 m (400 m in a car) from `own`. None: no node is
    /// far enough, and the remembered node is cleared.
    pub fn far_pick(&self, route: &[usize], remembered: Option<usize>, own: [f32; 3], in_car: bool) -> Option<usize> {
        let limit = if in_car { FAR_CAR } else { FAR_NODE_FOOT };
        let from = remembered.and_then(|r| route.iter().position(|&n| n == r)).unwrap_or(0);
        route[from.min(route.len())..].iter().copied().find(|&n| dist(self.pos(n), own) > limit)
    }
}

// ---------------------------------------------------------------------------------------------
// The data reader.
// ---------------------------------------------------------------------------------------------

/// The fields of an AINode in the install. [data]
const AINODE_FIELDS: &[&str] = &[
    "nodeGroupID",
    "nodeRadius",
    "nodeHeight",
    "nodeSize",
    "nodeTypePath",
    "nodeTypeExit",
    "nodeTypeJump",
    "nodeTypeSniper",
    "nodeTypeTeleport",
    "nodeTypeWayPoint",
    "connections",
    "disconnections",
    "autoConnections",
];

const MAX_DEPTH: u32 = 64;

/// A name hash as the data writes it: an enum value or an integer.
fn hash_of(node: Node) -> Option<u32> {
    node.enum_hash().or_else(|| node.int().map(|v| v as u32))
}

/// A flag written as `true` / `false` or as a raw `#00000001` / `#00000000`. [data]
fn flag_of(node: Node) -> Option<bool> {
    match node.enum_name() {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => node.int().map(|v| v != 0),
    }
}

/// The translation of a 12- or 16-value matrix (row 3). [assumed] layout, as `ai_spawn`.
fn translation(m: &[f32]) -> Option<[f32; 3]> {
    if m.len() != 12 && m.len() != 16 {
        return None;
    }
    let at = 3 * (m.len() / 4);
    Some([m[at], m[at + 1], m[at + 2]])
}

/// The host Instance and its name hash: its label, or the root key it sits under (the zone writes
/// `NavNode01:` as a root entry whose Instance has no label of its own). [data]
type Host<'a> = Option<(Node<'a>, u32)>;

fn read_node(node: Node, host: Host) -> AiNodeData {
    let mut d = AiNodeData::default();
    if let Some((h, name)) = host {
        d.instance = name;
        d.name = h.file().name(name).map_or_else(String::new, str::to_owned);
        match h.get("matrix").map(Node::floats).as_deref().and_then(translation) {
            Some(p) => d.pos = p,
            None => d.missing.push("host matrix".into()),
        }
    } else {
        d.missing.push("no host Instance".into());
    }
    let file = node.file();
    for (hash, _) in node.fields() {
        if !AINODE_FIELDS.iter().any(|n| crc32(n.as_bytes()) == hash) {
            d.missing.push(format!("unexpected field {}", file.name(hash).map_or_else(|| format!("#{hash:08x}"), str::to_owned)));
        }
    }
    let num = |name: &str, missing: &mut Vec<String>| match node.get(name).and_then(Node::float) {
        Some(v) => v,
        None => {
            missing.push(format!("{name} absent or not a number"));
            0.0
        }
    };
    d.radius = num("nodeRadius", &mut d.missing);
    d.height = num("nodeHeight", &mut d.missing);
    d.group_id = node.get("nodeGroupID").and_then(hash_of).unwrap_or(0);
    if let Some(s) = node.get("nodeSize").and_then(hash_of) {
        d.flags |= size_flag(s);
    }
    for (name, bit) in [
        ("nodeTypePath", NODE_PATH),
        ("nodeTypeExit", NODE_EXIT),
        ("nodeTypeJump", NODE_JUMP),
        ("nodeTypeSniper", NODE_SNIPER),
        ("nodeTypeTeleport", NODE_TELEPORT),
        ("nodeTypeWayPoint", NODE_WAYPOINT),
    ] {
        match node.get(name).and_then(flag_of) {
            Some(true) => d.flags |= bit,
            Some(false) => {}
            None => d.missing.push(format!("{name} absent or not a flag")),
        }
    }
    let refs = |name: &str| -> Vec<u32> {
        node.get(name).map(|l| l.items().filter_map(Node::label).collect()).unwrap_or_default()
    };
    d.connections = refs("connections");
    d.disconnections = refs("disconnections");
    d.auto_connections = refs("autoConnections");
    d
}

/// Every AINode of a data file in the walk's order; `host` is the nearest node above it with a
/// matrix (its Instance). An object reached twice is read once, except that a node first reached
/// without an Instance above it (the zone lists its node objects at the root, `obj1` ...) is read
/// again when the walk reaches it through its Instance (`NavNode01` -> things -> attachments); it
/// keeps its first place in the order. [data] us_test zone chunk 826.
pub fn read_nodes(file: &DataFile) -> Vec<AiNodeData> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut at_index = std::collections::HashMap::new();
    walk(file.root(), 0, None, &mut out, &mut seen, &mut at_index, 0);
    out
}

/// `key` is the field name hash `node` sits under (0 for list items and the root).
#[allow(clippy::too_many_arguments)]
fn walk<'a>(
    node: Node<'a>,
    key: u32,
    host: Host<'a>,
    out: &mut Vec<AiNodeData>,
    seen: &mut HashSet<(usize, Option<&'a str>, Option<usize>)>,
    at_index: &mut std::collections::HashMap<usize, usize>,
    depth: u32,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let kind = node.type_name();
    if kind == Some(AINODE_TYPE) {
        if let Some(&i) = node.offset().and_then(|at| at_index.get(&at)) {
            let unhosted = out[i].missing.iter().any(|m| m == "no host Instance");
            if unhosted && host.is_some() {
                out[i] = read_node(node, host);
            }
            return;
        }
        if let Some(at) = node.offset() {
            at_index.insert(at, out.len());
        }
        out.push(read_node(node, host));
        // The connection lists hold Instances that carry their own nodes: walk them too (below).
    } else if let Some(at) = node.offset() {
        // Keyed by the host too: the zone also lists the Objects that hold its nodes at the root,
        // where the walk meets them first without their Instance.
        if !seen.insert((at, kind, host.and_then(|(h, _)| h.offset()))) {
            return;
        }
    }
    let fields = node.fields();
    let matrix_key = crc32(b"matrix");
    let host = if fields.iter().any(|(h, _)| *h == matrix_key) { Some((node, node.label().unwrap_or(key))) } else { host };
    for (hash, child) in fields {
        walk(child, hash, host, out, seen, at_index, depth + 1);
    }
    for item in node.items() {
        walk(item, 0, host, out, seen, at_index, depth + 1);
    }
}

/// The AINodes of every data chunk of a pack, in chunk order. A chunk that does not parse is skipped.
pub fn load(path: &Path) -> Result<Vec<AiNodeData>, String> {
    let pack = Pack::open(path)?;
    let mut out = Vec::new();
    for chunk in pack.chunks.iter().filter(|c| c.tag == pack::DATA) {
        let Ok(file) = DataFile::parse(pack.data(chunk).to_vec()) else { continue };
        out.extend(read_nodes(&file));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(instance: u32, pos: [f32; 3], flags: u32, radius: f32) -> AiNodeData {
        AiNodeData { instance, pos, flags, radius, height: 50.0, ..AiNodeData::default() }
    }

    const NEAR: NodeSearch = NodeSearch { calc: CALC_NEAREST, min: 0.0, max: 5000.0 };

    #[test]
    fn size_flags_match_the_loader() {
        assert_eq!(size_flag(crc32(b"LARGE")), SIZE_LARGE);
        assert_eq!(size_flag(crc32(b"SMALL")), SIZE_SMALL);
        assert_eq!(size_flag(crc32(b"MEDIUM")), SIZE_MEDIUM);
        assert_eq!(size_flag(crc32(b"EXTRA_SMALL")), SIZE_EXTRA_SMALL);
        assert_eq!(size_flag(crc32(b"EXTRA_LARGE")), SIZE_EXTRA_LARGE);
        assert_eq!(size_flag(0), 0);
    }

    #[test]
    fn search_kinds_and_flags() {
        let g = AiNodeGraph::new(vec![
            node(1, [10.0, 0.0, 0.0], NODE_WAYPOINT, 0.0),
            node(2, [20.0, 0.0, 0.0], NODE_WAYPOINT, 0.0),
            node(3, [5.0, 0.0, 0.0], NODE_TELEPORT, 0.0),
        ]);
        assert_eq!(g.find([0.0; 3], NEAR, NODE_WAYPOINT, 0, None), Some(0));
        let far = NodeSearch { calc: CALC_FARTHEST, ..NEAR };
        assert_eq!(g.find([0.0; 3], far, NODE_WAYPOINT, 0, None), Some(1));
        // The band is inclusive and squared; 10 m is outside [11, 30].
        let band = NodeSearch { calc: CALC_NEAREST, min: 11.0, max: 30.0 };
        assert_eq!(g.find([0.0; 3], band, NODE_WAYPOINT, 0, None), Some(1));
        // Excluded flags, and the two kinds that never pick.
        assert_eq!(g.find([0.0; 3], NEAR, 0, NODE_WAYPOINT, None), Some(2));
        assert_eq!(g.find([0.0; 3], NodeSearch { calc: 0x1f6dbc20, ..NEAR }, 0, 0, None), None);
        assert_eq!(g.find([0.0; 3], NodeSearch { calc: 0x98243379, ..NEAR }, 0, 0, None), None);
    }

    #[test]
    fn nearest_tie_keeps_the_first_and_holders_are_skipped() {
        let mut g = AiNodeGraph::new(vec![node(1, [3.0, 0.0, 0.0], NODE_TELEPORT, 0.0), node(2, [-3.0, 0.0, 0.0], NODE_TELEPORT, 0.0)]);
        assert_eq!(g.find([0.0; 3], NEAR, NODE_TELEPORT, 0, None), Some(0));
        g.nodes[0].held_by = Some(7);
        assert_eq!(g.find([0.0; 3], NEAR, NODE_TELEPORT, 0, Some(9)), Some(1));
        assert_eq!(g.find([0.0; 3], NEAR, NODE_TELEPORT, 0, Some(7)), Some(0));
        // No holder given: the reservation is not looked at.
        assert_eq!(g.find([0.0; 3], NEAR, NODE_TELEPORT, 0, None), Some(0));
    }

    #[test]
    fn auto_links_use_the_cylinder() {
        // Radius 100, height 50: (100, 0, 0) is inside with the 1.01 tolerance, (101, 0, 0) not
        // (1.0201 > 1.01), (0, 0, 50.4) inside, (0, 0, 51) outside. The others have a 0.4 m
        // radius, so node 2 does not reach node 3 1 m away.
        let g = AiNodeGraph::new(vec![
            node(1, [0.0; 3], NODE_PATH, 100.0),
            node(2, [100.0, 0.0, 0.0], NODE_PATH, 0.4),
            node(3, [101.0, 0.0, 0.0], NODE_PATH, 0.4),
            node(4, [0.0, 0.0, 50.4], NODE_PATH, 0.4),
            node(5, [0.0, 0.0, 51.0], NODE_PATH, 0.4),
            node(6, [10.0, 0.0, 0.0], NODE_PATH | NODE_WAYPOINT, 0.4),
            node(7, [10.0, 10.0, 0.0], 0, 0.4),
        ]);
        assert_eq!(g.nodes[0].links, vec![1, 3]);
        // A link is one way.
        assert!(g.nodes[1].links.is_empty());
        // A node outside the path list gets no links.
        assert!(g.nodes[6].links.is_empty());
    }

    #[test]
    fn waypoint_links_follow_the_lists() {
        let mut a = node(1, [0.0; 3], NODE_PATH | NODE_WAYPOINT, 150.0);
        a.auto_connections = vec![2];
        a.connections = vec![3, 2];
        a.disconnections = vec![3];
        let g = AiNodeGraph::new(vec![a, node(2, [500.0, 0.0, 0.0], NODE_WAYPOINT, 0.0), node(3, [600.0, 0.0, 0.0], NODE_WAYPOINT, 0.0)]);
        assert_eq!(g.nodes[0].links, vec![1]);
        assert_eq!(g.next_waypoint(0), Some(1));
        assert_eq!(g.next_waypoint(1), None);
    }

    /// A chain 0 - 1 - 2 - 3 along x, each node linking to its neighbours (50 m apart, radius 60).
    fn chain() -> AiNodeGraph {
        AiNodeGraph::new(vec![
            node(1, [0.0, 0.0, 0.0], NODE_PATH, 60.0),
            node(2, [50.0, 0.0, 0.0], NODE_PATH, 60.0),
            node(3, [100.0, 0.0, 0.0], NODE_PATH, 60.0),
            node(4, [150.0, 0.0, 0.0], NODE_PATH, 60.0),
        ])
    }

    #[test]
    fn route_runs_start_to_goal() {
        let g = chain();
        let r = g.route([1.0, 0.0, 0.0], [149.0, 0.0, 0.0]);
        assert_eq!(r.nodes, vec![0, 1, 2, 3]);
        assert_eq!(r.next, [50.0, 0.0, 0.0]);
        // Same start and goal: no route, the own point.
        let r = g.route([1.0, 0.0, 0.0], [2.0, 0.0, 0.0]);
        assert!(r.nodes.is_empty());
        assert_eq!(r.next, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn route_prefers_the_cheaper_branch() {
        // 0 links to 1 (a side node, 72 m) and 2 (60 m); only 2 links on to 3. The side node is
        // opened and never expanded past: its estimate is worse.
        let g = AiNodeGraph::new(vec![
            node(1, [0.0, 0.0, 0.0], NODE_PATH, 80.0),
            node(2, [40.0, 60.0, 0.0], NODE_PATH, 80.0),
            node(3, [60.0, 0.0, 0.0], NODE_PATH, 80.0),
            node(4, [120.0, 0.0, 0.0], NODE_PATH, 80.0),
        ]);
        let r = g.route([0.0; 3], [120.0, 0.0, 0.0]);
        assert_eq!(r.nodes, vec![0, 2, 3]);
    }

    #[test]
    fn route_fails_when_the_goal_is_cut_off() {
        let g = AiNodeGraph::new(vec![node(1, [0.0; 3], NODE_PATH, 60.0), node(2, [500.0, 0.0, 0.0], NODE_PATH, 60.0)]);
        let r = g.route([0.0; 3], [500.0, 0.0, 0.0]);
        assert!(r.nodes.is_empty());
        assert_eq!(r.next, [0.0; 3]);
    }

    #[test]
    fn far_pick_starts_at_the_remembered_node() {
        let g = AiNodeGraph::new(vec![
            node(1, [0.0, 0.0, 0.0], NODE_PATH, 0.0),
            node(2, [150.0, 0.0, 0.0], NODE_PATH, 0.0),
            node(3, [300.0, 0.0, 0.0], NODE_PATH, 0.0),
            node(4, [500.0, 0.0, 0.0], NODE_PATH, 0.0),
        ]);
        let route = [0, 1, 2, 3];
        assert_eq!(g.far_pick(&route, None, [0.0; 3], false), Some(1));
        assert_eq!(g.far_pick(&route, Some(2), [0.0; 3], false), Some(2));
        // In a car the limit is 400 m.
        assert_eq!(g.far_pick(&route, None, [0.0; 3], true), Some(3));
        // A remembered node that is not on the route: from the start.
        assert_eq!(g.far_pick(&[0, 1], Some(3), [0.0; 3], false), Some(1));
        assert_eq!(g.far_pick(&[0], None, [0.0; 3], false), None);
    }
}
