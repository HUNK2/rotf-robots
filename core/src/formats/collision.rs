//! The `Collision` objects of a pack: every primitive's shape packet, placement and masks.
//!
//! Where it lives (`notes\collision-data.md`): a data chunk of the pack holds `Object`s
//! (root entries of the chunk included). An Object's
//! `attachments[i].thing` is a `Collision`; the Collision's `primitives` list holds one
//! `CollisionBatchPrimitive` per shape, and each has a typed `packet` (`CollisionCuboidPacket`,
//! ...). Level geometry is in several data chunks of a zone pack, not in chunk 0 [data].
//!
//! Not converted here: sizes stay as stored, `matrix` as stored (4 rows of 3 floats; row 3 is
//! the translation, so points are row vectors, `[data]` from a mesh with identity axes and a
//! non-zero row 3).
//!
//! One stored object, once: objects are shared heavily (a level's `scene` list and the `obj*`
//! roots refer to each other; the walk reaches the same Collision through many paths, 248
//! times over for Bumblebee's pack and 22k times for Shanghai's zone, for 38 and 871 distinct
//! objects). `collisions` keeps the first occurrence of each stored Collision, identified by
//! where it sits in the file (`lxb::Node::offset`; added by the manager, 2026-10-11, replacing
//! the porter's content-key stand-in, which gave 56 and 813).
//!
//! Tags: `[data]` seen in the decoded file, `[game]` from `notes\collision-loader.md`,
//! `[assumed]` interpretation with the reason stated.

use std::collections::HashSet;
use std::path::Path;

use super::lxb::{DataFile, Node};
use super::pack::{self, Pack};

/// The walk stops at this depth, as `work\tf2_lxb.py` does (`depth=64`).
const MAX_DEPTH: u32 = 64;

/// One hull face: `pad` and `edges` as stored. Each edge is `(index, offset)`.
/// [data] the field names; [assumed] what `index` and `offset` mean (not read in the loader).
#[derive(Clone, Debug, PartialEq)]
pub struct HullFace {
    pub pad: Vec<i64>,
    pub edges: Vec<(i64, i64)>,
}

/// The shape of one primitive, from its packet. Sizes and vertices are as stored.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// `CollisionCuboidPacket`, field `extent`: 4 floats, the last one 0 in every level file [data].
    Cuboid { extent: [f32; 4] },
    /// `CollisionSpherePacket`, field `radius` [data].
    Sphere { radius: f32 },
    /// `CollisionCylinderPacket`: every float field of the packet, by name (`radius`, `height12`) [data].
    Cylinder { fields: Vec<(String, f32)> },
    /// `CollisionConePacket`: every float field (`radius`, `height14`, `height34`, `angle`) [data].
    Cone { fields: Vec<(String, f32)> },
    /// `CollisionCapsulePacket`: no level data uses it [game, notes\collision-loader.md]. Floats as stored.
    Capsule { fields: Vec<(String, f32)> },
    /// `CollisionMeshPacket` (the soup). `declared_*` are the packet's `nVerts` / `nFaces`.
    /// Face words are stored raw: the low 27 bits are the vertex index, the top 5 bits are flags
    /// [data: a flagged word such as 0x80000002 appears]. Vertices are `x, y, z, w` [data].
    Mesh { declared_vertices: i64, declared_faces: i64, vertices: Vec<[f32; 4]>, faces: Vec<[u32; 3]> },
    /// `CollisionHull8Packet` / `Hull16Packet` / `Hull32Packet`: `width` is 8, 16 or 32 from the type name.
    /// Hull4 has no packet in the loader [game]; none in the level data.
    Hull { width: u8, declared_vertices: i64, declared_faces: i64, vertices: Vec<[f32; 4]>, faces: Vec<HullFace> },
    /// A packet type not decoded (or a primitive with no packet). Counted, never dropped.
    Other { type_name: String },
}

/// One primitive: a `CollisionBatchPrimitive`.
#[derive(Clone, Debug, PartialEq)]
pub struct Primitive {
    pub shape: Shape,
    /// `matrix`: 4 rows of 3 floats, as stored [data]. Rows 0 to 2 are the axes, row 3 the
    /// translation (identity axes with a non-zero row 3 in every checked sample) [data].
    pub matrix: [[f32; 3]; 4],
    /// `material`: the CRC-32 name hash as stored, and its text when the file's name table has it [data].
    pub material: u32,
    pub material_name: Option<String>,
    /// `maskPreset`: the CRC-32 name hash as stored, and its text [data].
    pub mask_preset: u32,
    pub mask_preset_name: Option<String>,
    /// `sendMask`, `receiveMask`: stored integers [data]. Their bit meaning is not decoded [assumed unused].
    pub send_mask: i64,
    pub receive_mask: i64,
}

/// One `Collision` object, as attached to an Object.
#[derive(Clone, Debug, PartialEq)]
pub struct Collision {
    /// Where the Object sits in the data object tree, e.g. `#14/obj2/attachments[0]/thing`.
    /// `load` puts the chunk index in front. Not stable across files.
    pub object_path: String,
    /// `true` when the Object's node carries the type name `Object`. `false` only for an
    /// Object-shaped value with no type name (none in the checked packs: every root entry is typed).
    pub object_typed: bool,
    /// The Collision's `parent`: an index into the Object's `hierarchy.names` [data].
    pub parent_node: Option<usize>,
    /// That node's name (CRC-32, resolved through the file's name table when it has the name).
    pub parent_name: Option<String>,
    /// `dynamics` is a `DynamicsData` (a physics body) when true [data].
    pub has_dynamics: bool,
    pub primitives: Vec<Primitive>,
}

/// A Collision found by the walk, with the Object it hangs off.
struct Hit<'a> {
    object: Node<'a>,
    /// The Object's name: its header's name, or for a root entry the root's own name.
    label: u32,
    collision: Node<'a>,
    path: String,
}

/// An Object: a node whose type is `Object` or, for a root entry the reader cannot type by name,
/// one with the Object's `hierarchy` and `attachments` fields. The type name is in the data for
/// the root entries too (Rust reads it; `work\collision_dump.py` does not, it tags pointers only).
fn is_object(node: Node) -> bool {
    match node.type_name() {
        Some(name) => name == "Object",
        None => node.get("hierarchy").is_some() && node.get("attachments").is_some(),
    }
}

/// `root_name` is set for a value that is a direct field of the file's root: its name is the
/// object's name when the header carries none (a root entry has no header name in `Node`).
fn walk<'a>(node: Node<'a>, root_name: Option<u32>, path: &mut String, depth: u32, visited: &mut HashSet<usize>, out: &mut Vec<Hit<'a>>) {
    if depth > MAX_DEPTH {
        return;
    }
    // A typed node reached a second time has nothing new under it (the first occurrence of
    // a Collision is the one kept); without this deep_six_01's chunk 13 is walked without end.
    if let (Some(_), Some(at)) = (node.type_name(), node.offset()) {
        if !visited.insert(at) {
            return;
        }
    }
    if is_object(node) {
        let label = node.label().or(root_name).unwrap_or(0);
        if let Some(attachments) = node.get("attachments") {
            for attachment in attachments.items() {
                if let Some(thing) = attachment.get("thing") {
                    if thing.type_name() == Some("Collision") {
                        out.push(Hit { object: node, label, collision: thing, path: path.clone() });
                    }
                }
            }
        }
    }
    let file = node.file();
    for (hash, child) in node.fields() {
        let len = path.len();
        path.push('/');
        match file.name(hash) {
            Some(name) => path.push_str(name),
            None => path.push_str(&format!("#{hash:08x}")),
        }
        let is_root_field = depth == 0;
        walk(child, is_root_field.then_some(hash), path, depth + 1, visited, out);
        path.truncate(len);
    }
    if node.len() > 0 {
        for (i, child) in node.items().enumerate() {
            let len = path.len();
            path.push_str(&format!("[{i}]"));
            walk(child, None, path, depth + 1, visited, out);
            path.truncate(len);
        }
    }
}

fn fixed<const N: usize>(node: Option<Node>) -> [f32; N] {
    let v = node.map(Node::floats).unwrap_or_default();
    std::array::from_fn(|i| v.get(i).copied().unwrap_or(0.0))
}

fn int_or(node: Option<Node>) -> Option<i64> {
    node.and_then(Node::int)
}

fn float_fields(packet: Node, file: &DataFile) -> Vec<(String, f32)> {
    packet
        .fields()
        .into_iter()
        .filter_map(|(hash, node)| {
            let value = node.float()?;
            let name = file.name(hash).map_or_else(|| format!("#{hash:08x}"), str::to_owned);
            Some((name, value))
        })
        .collect()
}

fn vertices(node: Option<Node>) -> Vec<[f32; 4]> {
    node.map(|n| n.items().map(|v| fixed::<4>(Some(v))).collect()).unwrap_or_default()
}

fn mesh_faces(node: Option<Node>) -> Vec<[u32; 3]> {
    node.map(|n| {
        n.items()
            .map(|f| {
                let w = f.ints();
                std::array::from_fn(|i| w.get(i).copied().unwrap_or(0) as u32)
            })
            .collect()
    })
    .unwrap_or_default()
}

fn hull_faces(node: Option<Node>) -> Vec<HullFace> {
    node.map(|n| {
        n.items()
            .map(|f| HullFace {
                pad: f.get("pad").map(Node::ints).unwrap_or_default(),
                edges: f
                    .get("edges")
                    .map(|e| {
                        e.items()
                            .map(|edge| (int_or(edge.get("index")).unwrap_or(0), int_or(edge.get("offset")).unwrap_or(0)))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect()
    })
    .unwrap_or_default()
}

fn shape(packet: Node, file: &DataFile) -> Shape {
    let type_name = packet.type_name().unwrap_or("");
    let declared_vertices = int_or(packet.get("nVerts")).unwrap_or(0);
    let declared_faces = int_or(packet.get("nFaces")).unwrap_or(0);
    match type_name {
        "CollisionCuboidPacket" => Shape::Cuboid { extent: fixed::<4>(packet.get("extent")) },
        "CollisionSpherePacket" => match packet.get("radius").and_then(Node::float) {
            Some(radius) => Shape::Sphere { radius },
            None => Shape::Other { type_name: type_name.to_owned() },
        },
        "CollisionCylinderPacket" => Shape::Cylinder { fields: float_fields(packet, file) },
        "CollisionConePacket" => Shape::Cone { fields: float_fields(packet, file) },
        "CollisionCapsulePacket" => Shape::Capsule { fields: float_fields(packet, file) },
        "CollisionMeshPacket" => Shape::Mesh {
            declared_vertices,
            declared_faces,
            vertices: vertices(packet.get("verts")),
            faces: mesh_faces(packet.get("faces")),
        },
        "CollisionHull8Packet" | "CollisionHull16Packet" | "CollisionHull32Packet" => Shape::Hull {
            width: match type_name {
                "CollisionHull8Packet" => 8,
                "CollisionHull16Packet" => 16,
                _ => 32,
            },
            declared_vertices,
            declared_faces,
            vertices: vertices(packet.get("verts")),
            faces: hull_faces(packet.get("faces")),
        },
        other => Shape::Other { type_name: other.to_owned() },
    }
}

fn primitive(pr: Node, file: &DataFile) -> Primitive {
    let shape = match pr.get("packet") {
        Some(packet) if packet.type_name().is_some() => shape(packet, file),
        _ => Shape::Other { type_name: "(no packet)".to_owned() },
    };
    let matrix_values = pr.get("matrix").map(Node::floats).unwrap_or_default();
    let matrix = std::array::from_fn(|r| std::array::from_fn(|c| matrix_values.get(r * 3 + c).copied().unwrap_or(0.0)));
    // `material` and `maskPreset` are stored as CRC-32 name hashes (an integer in the data, read
    // as one), resolved through the file's name table [data: `Default`, `Character`, `LowerBody`].
    let material = int_or(pr.get("material")).unwrap_or(0) as u32;
    let mask_preset = int_or(pr.get("maskPreset")).unwrap_or(0) as u32;
    Primitive {
        shape,
        matrix,
        material,
        material_name: file.name(material).map(str::to_owned),
        mask_preset,
        mask_preset_name: file.name(mask_preset).map(str::to_owned),
        send_mask: int_or(pr.get("sendMask")).unwrap_or(0),
        receive_mask: int_or(pr.get("receiveMask")).unwrap_or(0),
    }
}

fn primitives(collision: Node, file: &DataFile) -> Vec<Primitive> {
    collision.get("primitives").map(|list| list.items().map(|pr| primitive(pr, file)).collect()).unwrap_or_default()
}

/// The name of the node a Collision's `parent` points at, from the Object's `hierarchy`.
fn parent_name(object: Node, index: usize, file: &DataFile) -> Option<String> {
    let hash = object.get("hierarchy")?.get("names")?.at(index)?.int()?;
    Some(file.name(hash as u32).map_or_else(|| format!("#{:08x}", hash as u32), str::to_owned))
}

/// Every Collision of one data object (one chunk), with the stand-in dedupe (see the module note).
/// Walks every path from the roots, so a shared object is reached more than once; the first
/// occurrence is kept.
pub fn collisions(file: &DataFile) -> Vec<Collision> {
    let mut hits = Vec::new();
    let mut path = String::new();
    walk(file.root(), None, &mut path, 0, &mut HashSet::new(), &mut hits);

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for hit in hits {
        let parent = int_or(hit.collision.get("parent"));
        let has_dynamics = hit.collision.get("dynamics").and_then(Node::type_name).is_some();
        let prims = primitives(hit.collision, file);
        // Identity = where the Collision is stored (`Node::offset`): one stored object is
        // kept once however many paths reach it.
        let _ = hit.label;
        if let Some(at) = hit.collision.offset() {
            if !seen.insert(at) {
                continue;
            }
        }
        let parent_node = parent.and_then(|p| usize::try_from(p).ok());
        out.push(Collision {
            object_path: hit.path,
            object_typed: hit.object.type_name().is_some(),
            parent_node,
            parent_name: parent_node.and_then(|i| parent_name(hit.object, i, file)),
            has_dynamics,
            primitives: prims,
        });
    }
    out
}

/// Every Collision in every data chunk of a `.str` pack. `object_path` starts with `#<chunk>`.
pub fn load(pack_path: &Path) -> Result<Vec<Collision>, String> {
    let pack = Pack::open(pack_path)?;
    let mut out = Vec::new();
    for chunk in pack.of_type(pack::DATA) {
        let file = DataFile::parse(pack.data(chunk).to_vec()).map_err(|e| format!("chunk {}: {e}", chunk.index))?;
        for mut c in collisions(&file) {
            c.object_path = format!("#{}{}", chunk.index, c.object_path);
            out.push(c);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The install, when it is there. The tests skip (pass) when the pack is not.
    fn pack_at(relative: &str) -> Option<PathBuf> {
        let dir = PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| r"C:\Games2".into()));
        let path = dir.join(relative);
        path.is_file().then_some(path)
    }

    fn count_by_type(cols: &[Collision]) -> std::collections::BTreeMap<&'static str, usize> {
        let mut m = std::collections::BTreeMap::new();
        for c in cols {
            for p in &c.primitives {
                let name = match &p.shape {
                    Shape::Cuboid { .. } => "Cuboid",
                    Shape::Sphere { .. } => "Sphere",
                    Shape::Cylinder { .. } => "Cylinder",
                    Shape::Cone { .. } => "Cone",
                    Shape::Capsule { .. } => "Capsule",
                    Shape::Mesh { .. } => "Mesh",
                    Shape::Hull { width: 8, .. } => "Hull8",
                    Shape::Hull { width: 16, .. } => "Hull16",
                    Shape::Hull { .. } => "Hull32",
                    Shape::Other { .. } => "Other",
                };
                *m.entry(name).or_insert(0) += 1;
            }
        }
        m
    }

    /// Exact expectation: 38 Collisions and 41 primitives in Bumblebee's pack, by object identity
    /// (`work\survey\collision\count_distinct.py`, Python). `work\collision_dump.py` prints 32: it
    /// does not see the 6 root entries. Ignored until `lxb::Node` has an offset (module note).
    #[test]
    fn bumblebee_counts_match_the_identity_walk() {
        let Some(path) = pack_at(r"characters\bumblebee.str") else { return };
        let cols = load(&path).unwrap();
        assert_eq!(cols.len(), 38);
        assert_eq!(cols.iter().map(|c| c.primitives.len()).sum::<usize>(), 41);
    }

    #[test]
    fn bumblebee_named_primitives_have_their_stored_shapes() {
        // Needs the install.
        let Some(path) = pack_at(r"characters\bumblebee.str") else { return };
        let cols = load(&path).unwrap();
        assert!(cols.iter().all(|c| c.object_typed));
        assert!(cols.iter().flat_map(|c| &c.primitives).all(|p| !matches!(p.shape, Shape::Other { .. })));

        // Hand_L (parent 34): a sphere of radius 0.932079 at (0.155, 0, 0), on Character masks.
        let hand = cols.iter().find(|c| c.parent_name.as_deref() == Some("Hand_L")).expect("Hand_L");
        assert_eq!(hand.primitives.len(), 1);
        let Shape::Sphere { radius } = hand.primitives[0].shape else { panic!("Hand_L is not a sphere") };
        assert!((radius - 0.932_079).abs() < 1e-5);
        // collision_dump.py rounds to 3 places: 0.155.
        assert!((hand.primitives[0].matrix[3][0] - 0.155).abs() < 1e-3 && hand.primitives[0].matrix[3][1].abs() < 1e-6);
        // The name table's text for this hash is "0 - Default" [data].
        assert_eq!(hand.primitives[0].material_name.as_deref(), Some("0 - Default"));

        // Foot_L (parent 20): a cuboid, extent (0.4176, 0.7400, 0.5395, 0), with rotated axes.
        let foot = cols.iter().find(|c| c.parent_name.as_deref() == Some("Foot_L")).expect("Foot_L");
        let Shape::Cuboid { extent } = foot.primitives[0].shape else { panic!("Foot_L is not a cuboid") };
        assert!((extent[0] - 0.417_620_0).abs() < 1e-5 && extent[3] == 0.0);
        assert_eq!(foot.primitives[0].material_name.as_deref(), Some("LowerBody"));
        assert!((foot.primitives[0].matrix[0][1] + 0.681).abs() < 1e-3);
    }

    #[test]
    fn shanghai_zone_prints_the_primitive_table() {
        // Needs the install. Prints the table; checks that every packet type is decoded.
        let Some(path) = pack_at(r"levels\shanghai_01\zone.str") else { return };
        let cols = load(&path).unwrap();
        let table = count_by_type(&cols);
        println!("shanghai_01 zone.str: {} Collisions; primitives by shape:", cols.len());
        for (k, v) in &table {
            println!("  {k:<10} {v}");
        }
        println!("  total      {}", table.values().sum::<usize>());
        assert!(!table.contains_key("Other"), "undecoded shapes: {:?}", table);
    }

    /// Exact expectation (`work\survey\collision\count_distinct.py` and `count_collisions.py`, Python,
    /// by object identity; the per-type numbers add up to the survey's Z-SH counts). Ignored until
    /// `lxb::Node` has an offset (module note): the stand-in key gives a different total.
    #[test]
    fn shanghai_zone_counts_match_the_identity_walk() {
        let Some(path) = pack_at(r"levels\shanghai_01\zone.str") else { return };
        let cols = load(&path).unwrap();
        let table = count_by_type(&cols);
        assert_eq!(cols.len(), 871);
        assert_eq!(table.values().sum::<usize>(), 1791);
        assert_eq!(table["Cuboid"], 1579);
        assert_eq!(table["Cylinder"], 141);
        assert_eq!(table["Sphere"], 41);
        assert_eq!(table["Mesh"], 17);
        assert_eq!(table["Hull8"], 6);
        assert_eq!(table["Hull16"], 4);
        assert_eq!(table["Cone"], 3);
    }

    #[test]
    fn shanghai_meshes_have_declared_counts_and_valid_face_indices() {
        // Needs the install. Expected from `count_labelkey_types.py`: 17 meshes, 114 faces with the
        // top 5 bits set, and no face index at or past the vertex count.
        let Some(path) = pack_at(r"levels\shanghai_01\zone.str") else { return };
        let cols = load(&path).unwrap();
        let mut meshes = 0;
        let mut flagged_faces = 0;
        for c in &cols {
            for p in &c.primitives {
                let Shape::Mesh { declared_vertices, declared_faces, vertices, faces } = &p.shape else { continue };
                meshes += 1;
                assert_eq!(vertices.len() as i64, *declared_vertices, "{}", c.object_path);
                assert_eq!(faces.len() as i64, *declared_faces, "{}", c.object_path);
                for face in faces {
                    for &w in face {
                        assert!(((w & 0x07ff_ffff) as usize) < vertices.len(), "{}: face index past {} vertices", c.object_path, vertices.len());
                    }
                    if face.iter().any(|&w| w & 0xf800_0000 != 0) {
                        flagged_faces += 1;
                    }
                }
            }
        }
        println!("shanghai_01 meshes: {meshes}; faces with the top 5 bits set: {flagged_faces}");
        assert_eq!(meshes, 17);
        assert_eq!(flagged_faces, 114);
    }
}
