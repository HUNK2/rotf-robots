//! A zone's DRAWN geometry, placed in the world: which model goes where.
//!
//! Placement rule: the same one `collision_world.rs` uses (copied here, not shared):
//! world = (the thing's own matrix, if any) x (the bind of the Object node it rides, absolute in
//! the object's space) x (every enclosing `Instance` matrix). A `RenderThing` is an attachment of
//! an Object and rides the node its `index` names. [data] In the shanghai zone a RenderThing has
//! no matrix of its own; only the node rides (`notes\level-render.md`).
//!
//! Each `RenderThing` holds a `meshes` list, and each entry names one model with its own
//! `lod_distances`. [data] In shanghai every RenderThing has exactly one entry, so `lod` is 0
//! for every placed model. The native draw selector follows the library's stored LOD
//! chain (`Mesh::lods`) in order; geometry counts do not determine near/far ordering.
//!
//! Tags: `[data]` seen in the decoded file, `[assumed]` not settled, see the note.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::collision_world::Owner;
use crate::formats::lxb::{DataFile, Node};
use crate::formats::model::Library;
use crate::formats::pack::{self, Pack};

/// Rows 0..2 are the axes (the images of x, y, z); row 3 the position. Row vectors: p * M.
pub(crate) type Mat = [[f64; 3]; 4];

pub(crate) const IDENTITY: Mat = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0]];

/// The walk stops here (as `collision_world` does).
pub(crate) const MAX_DEPTH: u32 = 80;

/// Placements of one stored RenderThing are told apart by the world matrix rounded to this many
/// steps per metre (as `collision_world`).
pub(crate) const KEY_SCALE: f64 = 1.0e4;

/// One drawn model at one place in the zone.
pub struct PlacedModel {
    /// The model's name as the RenderThing gives it, such as `Tree_Lg_C.Tree_Lg_C_leaves`.
    pub model: String,
    /// The 4x3 row-vector world matrix, flattened row by row (rows 0..2 the axes, row 3 the position).
    pub world: [f32; 12],
    /// The index of the mesh entry within its RenderThing (always
    /// 0 in shanghai, where each RenderThing has one entry). [data] see the module note.
    pub lod: usize,
    /// [game 00586a80] lod_distances[mesh.lods.len()], or FLT_MAX if absent.
    pub draw_distance: f32,
    pub blend_lod: bool,
    /// `"<pack>#<chunk> @ <object offset> node <index> thing <thing offset> mesh <n>"`.
    pub source: String,
    pub object_offset: usize,
    /// The innermost Instance that places it (`collision_world::Owner`), None when no Instance does.
    pub owner: Option<Owner>,
    /// Under a `DamageLink` or `DetachLink`; `deferred_stage` distinguishes future states.
    pub linked: bool,
    /// Under a DamageLink: built only when its pending damage record is drained
    /// (005544e0 -> 005546a0 -> 00554830), unlike intact DetachLink pieces.
    pub deferred_stage: bool,
    /// The damage stage it belongs to: how many DamageLinks are above it (0 the intact Object, 1 the first stage's
    /// Object, 2 the one that stage's own DamageLink builds). [data] the nesting; the game builds a stage when the
    /// Destructable of the stage before it dies (00554440 -> 005544e0 -> 005546a0 -> 00554830).
    pub stage: u8,
    /// Its Object's Debris component (`crate::debris`): thrown when its stage is built. [data]
    pub debris: Option<crate::debris::DebrisDef>,
    /// VisibilityMask values of enclosing Instances; all enclosing masks must accept the view.
    pub visibility_masks: Vec<u32>,
    /// The actual NoShadowTag of this Object (ShadowTag clears it), rather than a name heuristic.
    pub no_shadow: bool,
    /// The Object's NoReflectionTag (ReflectionTag clears it) [data]: [assumed] left out of the sea's reflection
    /// pass (`game\src\ocean.rs`; the tag's runtime 00ad64d0 is unread).
    pub no_reflection: bool,
    pub spin: Option<Spin>,
    pub ifl: Option<crate::ifl::Definition>,
    /// The colour its Instance's `Colorizer` picked (`crate::colorizer`), None without one.
    /// [assumed] it multiplies every material of the Instance (the shader's ObjectDiffuseColor).
    pub tint: Option<[f32; 3]>,
}

/// [game 00809550, 00809700] SpinnyThing speed is degrees/sec; axis is normalised.
#[derive(Clone, Copy, Debug)]
pub struct Spin {
    pub axis: glam::Vec3,
    pub pivot: glam::Vec3,
    pub radians_per_second: f32,
}

/// [game 005a6210] The factory composes each trigRect and stores its inverse.
#[derive(Clone, Debug)]
pub struct VisibilityBox {
    pub channel: u8,
    pub local_from_world: glam::Mat4,
}

/// [game 011796b0, 005a5d30] Empty managers enable every channel. Each channel is the
/// union of its boxes; the authored transform scales a unit cube, with 1.01 slack.
pub fn active_visibility(boxes: &[VisibilityBox], point: glam::Vec3) -> u32 {
    if boxes.is_empty() { return u32::MAX; }
    boxes.iter().filter(|b| b.local_from_world.transform_point3(point).abs().max_element() <= 1.01)
        .fold(0, |bits, b| bits | (1 << b.channel))
}

/// The zone's model library (every MODEL and TEXTURE chunk of the pack) and the placed models.
pub struct LevelRender {
    pub library: Library,
    pub placed: Vec<PlacedModel>,
    pub visibility_boxes: Vec<VisibilityBox>,
    pub emitters: Vec<PlacedEmitter>,
    /// The placed TriggerableParticles (LS-57): started and stopped by a message, not on load.
    pub trig_emitters: Vec<PlacedTrigEmitter>,
}

/// A placed TriggerableParticle (LS-57). [data] fields `LuxScriptName`, `acceptNullTriggerMessages`, `id`
/// (a ParticleTemplate), `useRot`, `useScale`. [game] the loader 00746fc0 and ctor 00746e40
/// (`notes\level-animthing-effects.md` LS-49, LS-53). It is NOT started at load: the trigger path starts it.
pub struct PlacedTrigEmitter {
    pub template: crate::particles::Template,
    pub world: [f32; 12],
    /// `LuxScriptName` as a name hash (the filter a message must carry). [data]
    pub lux_script_name: u32,
    /// `acceptNullTriggerMessages`. [data]
    pub accept_null: bool,
    pub use_rotation: bool,
    pub use_scale: bool,
    /// The innermost placed Instance above it, the one a message to that Instance's children reaches. [assumed]
    pub owner: Option<Owner>,
    pub source: String,
}

/// Direct scene emitters, excluding unbuilt DamageLink branches and template roots.
/// [game 0057d0d0] autoStart/useRot default on, useScale defaults off.
pub struct PlacedEmitter {
    pub template: crate::particles::Template,
    pub world: [f32; 12],
    pub auto_start: bool,
    pub use_rotation: bool,
    pub use_scale: bool,
    pub spawn_chance: f32,
    /// The owning Object's translation, used by the deterministic spawnChance gate.
    pub object_position: glam::Vec3,
    pub source: String,
    /// The Instance that places it and the damage stage it is in (0: there from the load). A stage's emitters start
    /// when the stage is built.
    pub owner: Option<Owner>,
    pub stage: u8,
}

/// One placed mesh entry, with what the survey needs beyond `PlacedModel`. The survey fields are
/// read by the tests only.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Found {
    pub model: String,
    pub world: [f32; 12],
    pub lod: usize,
    pub source: String,
    /// The node of the Object that the thing rides, and how many sub-matrices its Object has.
    pub index: i64,
    pub sub_matrices: usize,
    /// How many mesh entries its RenderThing has, and this entry's `lod_distances`.
    pub meshes: usize,
    pub lod_distances: Vec<f32>,
    pub blend_lod: bool,
    /// Under a `DamageLink` or `DetachLink` of an Object (released when the parent breaks).
    pub linked: bool,
    /// How many DamageLinks are above it (0: not deferred).
    pub deferred_stage: u8,
    /// Its Object's Debris component: a piece its stage throws when built.
    pub debris: Option<crate::debris::DebrisDef>,
    /// The offset of the Object the RenderThing is attached to.
    pub object_offset: usize,
    /// The innermost Instance above it (`Owner`), None when none places it.
    pub owner: Option<Owner>,
    pub visibility_masks: Vec<u32>,
    pub no_shadow: bool,
    pub no_reflection: bool,
    pub spin: Option<Spin>,
    pub ifl: Option<crate::ifl::Definition>,
    pub tint: Option<[f32; 3]>,
}

/// Loads a zone pack (`levels\<zone>\zone.str`): its model library (every MODEL and TEXTURE
/// chunk), and every RenderThing of its data chunks placed in the world.
pub fn load(zone_pack: &Path) -> Result<LevelRender, String> {
    load_at(zone_pack, 0)
}

/// `load` for the pack at `ordinal` in the list its level was loaded from (the owners' keys carry it).
fn load_at(zone_pack: &Path, ordinal: usize) -> Result<LevelRender, String> {
    let pack = Pack::open(zone_pack)?;
    let library = Library::load(&pack);
    let (found, _, visibility_boxes, emitters, trig_emitters) = walk_with_visibility(&pack, zone_pack, ordinal)?;
    let placed = found
        .into_iter()
        .map(|f| {
            let count = library.meshes.get(&crate::formats::hash::lower33(&f.model)).map_or(0,|m|m.lods.len());
            PlacedModel { draw_distance: f.lod_distances.get(count).copied().unwrap_or(f32::MAX),
                model: f.model, world: f.world, lod: f.lod, source: f.source, object_offset: f.object_offset, owner: f.owner, linked: f.linked, deferred_stage: f.deferred_stage > 0, stage: f.deferred_stage, debris: f.debris,
                blend_lod: f.blend_lod, visibility_masks: f.visibility_masks, no_shadow: f.no_shadow, no_reflection: f.no_reflection, spin: f.spin, ifl: f.ifl, tint: f.tint }
        })
        .collect();
    Ok(LevelRender { library, placed, visibility_boxes, emitters, trig_emitters })
}

/// Several packs as one level: a mission's `streamFiles` (the zone, then the mission's own
/// pack). The libraries are merged and the placements follow one another.
pub fn load_packs(packs: &[std::path::PathBuf]) -> Result<LevelRender, String> {
    let mut all = LevelRender { library: Library::default(), placed: Vec::new(), visibility_boxes: Vec::new(), emitters: Vec::new(), trig_emitters: Vec::new() };
    for (ordinal, path) in packs.iter().enumerate() {
        let one = load_at(path, ordinal)?;
        all.library.merge(one.library);
        all.placed.extend(one.placed);
        all.visibility_boxes.extend(one.visibility_boxes);
        all.emitters.extend(one.emitters);
        all.trig_emitters.extend(one.trig_emitters);
    }
    Ok(all)
}

/// Every placed mesh entry of the pack's data chunks, and how many RenderThing placements were
/// skipped as repeats (the same stored thing at the same world matrix, reached by two routes).
#[allow(dead_code)]
pub(crate) fn walk_pack(pack: &Pack, zone_pack: &Path) -> Result<(Vec<Found>, usize), String> {
    walk_pack_at(pack, zone_pack, 0)
}

/// As `walk_pack`, for the pack at `ordinal` in its level's list.
pub(crate) fn walk_pack_at(pack: &Pack, zone_pack: &Path, ordinal: usize) -> Result<(Vec<Found>, usize), String> {
    let (found, repeats, _, _, _) = walk_with_visibility(pack, zone_pack, ordinal)?;
    Ok((found, repeats))
}

type Walked = (Vec<Found>, usize, Vec<VisibilityBox>, Vec<PlacedEmitter>, Vec<PlacedTrigEmitter>);

fn walk_with_visibility(pack: &Pack, zone_pack: &Path, ordinal: usize) -> Result<Walked, String> {
    let file_name = zone_pack.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut found = Vec::new();
    let mut repeats = 0usize;
    let mut visibility_boxes = Vec::new();
    let mut emitters = Vec::new();
    let mut trig_emitters = Vec::new();
    // One pick generator for the pack's Colorizers, in walk order [assumed: the game's load order].
    let mut colour_seed = crate::colorizer::SEED;
    for chunk in pack.of_type(pack::DATA) {
        let file = DataFile::parse(pack.data(chunk).to_vec()).map_err(|e| format!("chunk {}: {e}", chunk.index))?;
        let label = format!("{}#{}", file_name, chunk.index);
        let mut sink = Sink { label: &label, pack: ordinal, chunk: chunk.index, seen: HashSet::new(), out: &mut found, repeats: 0, visited: HashSet::new(),
            visibility_boxes: &mut visibility_boxes, seen_triggers: HashSet::new(), emitters: &mut emitters, seen_emitters: HashSet::new(),
            trig_emitters: &mut trig_emitters, tints: HashMap::new(), colour_seed: &mut colour_seed };
        visit(file.root(), IDENTITY, false, 0, false, None, &[], 0, &mut sink);
        repeats += sink.repeats;
    }
    Ok((found, repeats, visibility_boxes, emitters, trig_emitters))
}

/// The models of a scene embedded in a pack's data chunks (a CharacterDefinition's `data`, as `cargotruck_basepose`
/// in the escort's mission chunk 8): each RenderThing's model and its world matrix (12 floats, as `PlacedModel`)
/// relative to the scene, placed by the same walk as the level's (Instance matrix, `sub_matrices` nodes). [data]
pub fn scene_models(pack_path: &Path, name: &str) -> Result<Vec<(String, [f32; 12])>, String> {
    fn find<'a>(node: Node<'a>, label: u32, depth: u32, seen: &mut HashSet<usize>) -> Option<Node<'a>> {
        if depth > MAX_DEPTH || node.offset().is_some_and(|o| !seen.insert(o)) {
            return None;
        }
        if node.label() == Some(label) {
            return Some(node);
        }
        node.fields().into_iter().map(|(_, n)| n).chain(node.items()).find_map(|n| find(n, label, depth + 1, seen))
    }
    let pack = Pack::open(pack_path)?;
    let label = crate::formats::hash::crc32(name.as_bytes());
    for chunk in pack.of_type(pack::DATA) {
        let file = DataFile::parse(pack.data(chunk).to_vec()).map_err(|e| format!("chunk {}: {e}", chunk.index))?;
        let Some(scene) = find(file.root(), label, 0, &mut HashSet::new()) else { continue };
        let (mut found, mut boxes, mut emitters, mut trig, mut seed) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), crate::colorizer::SEED);
        let label = format!("{name}#{}", chunk.index);
        let mut sink = Sink { label: &label, pack: 0, chunk: chunk.index, seen: HashSet::new(), out: &mut found, repeats: 0, visited: HashSet::new(),
            visibility_boxes: &mut boxes, seen_triggers: HashSet::new(), emitters: &mut emitters, seen_emitters: HashSet::new(),
            trig_emitters: &mut trig, tints: HashMap::new(), colour_seed: &mut seed };
        visit(scene, IDENTITY, false, 0, false, None, &[], 0, &mut sink);
        return Ok(found.into_iter().filter(|f| f.deferred_stage == 0).map(|f| (f.model, f.world)).collect());
    }
    Err(format!("{name}: in no data chunk of {}", pack_path.display()))
}

struct Sink<'a> {
    label: &'a str,
    /// The pack's place in its level's list and its chunk index: with an Instance's offset, the `Owner` key.
    pack: usize,
    chunk: usize,
    /// (RenderThing offset, quantised world matrix, owner): one stored thing once per distinct placement and Instance.
    seen: HashSet<(usize, [i64; 12], Option<Owner>)>,
    out: &'a mut Vec<Found>,
    repeats: usize,
    /// (node offset, its type, quantised world matrix, linked, placed, owner): see `visit`.
    visited: HashSet<(usize, usize, [i64; 12], bool, bool, Option<Owner>)>,
    visibility_boxes: &'a mut Vec<VisibilityBox>,
    seen_triggers: HashSet<(usize, [i64; 12])>,
    emitters: &'a mut Vec<PlacedEmitter>,
    seen_emitters: HashSet<(usize, [i64; 12], Option<Owner>)>,
    trig_emitters: &'a mut Vec<PlacedTrigEmitter>,
    /// Each placed Instance's Colorizer colour, picked when the Instance is first reached.
    tints: HashMap<Owner, [f32; 3]>,
    colour_seed: &'a mut u32,
}

fn affine(m: &Mat) -> glam::Mat4 {
    glam::Mat4::from_cols(glam::Vec3::from_array(m[0].map(|v| v as f32)).extend(0.0),
        glam::Vec3::from_array(m[1].map(|v| v as f32)).extend(0.0),
        glam::Vec3::from_array(m[2].map(|v| v as f32)).extend(0.0),
        glam::Vec3::from_array(m[3].map(|v| v as f32)).extend(1.0))
}

/// A stored 4x3 matrix (12 floats, rows of 3) as f64. None when the node is not 12 floats.
pub(crate) fn matrix_of(node: Node) -> Option<Mat> {
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

/// `first` then `then`: rows 0..2 = first.A * then.A; row 3 = first.t * then.A + then.t.
pub(crate) fn compose(first: &Mat, then: &Mat) -> Mat {
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

/// The bind of node `index` of `obj`, in the object's own space (absolute). Node 0 is the
/// object's origin; node i > 0 is `sub_matrices[i - 1]`. As `collision_world::bind_of`.
pub(crate) fn bind_of(obj: Node, index: i64) -> Mat {
    if index <= 0 {
        return IDENTITY;
    }
    obj.get("sub_matrices").and_then(|s| s.at((index - 1) as usize)).and_then(matrix_of).unwrap_or(IDENTITY)
}

pub(crate) fn flat(m: &Mat) -> [f64; 12] {
    let mut out = [0.0f64; 12];
    for r in 0..4 {
        for c in 0..3 {
            out[r * 3 + c] = m[r][c];
        }
    }
    out
}

pub(crate) fn flat32(m: &Mat) -> [f32; 12] {
    flat(m).map(|v| v as f32)
}

/// Whether the walk goes down through this node: the containers (untyped nodes and lists,
/// `Scene`, `Instance`, `Object`) and the two links that hold the pieces of a breakable
/// Object. Any other component (a trigger, a spawner, a script) may POINT at an Instance or
/// an Object that is placed elsewhere; following the pointer would place it a second time
/// under the wrong matrices, and a mission pack is walked without end that way.
/// [assumed: the list of types; the game builds an Instance's `things` and an Object's
/// attachments (`notes\zone-loading.md` part 2), each component's own builder is unread]
pub(crate) fn carries_placement(node: Node) -> bool {
    matches!(node.type_name(), None | Some("Scene" | "Instance" | "Object" | "DamageLink" | "DetachLink"))
}

/// The walk: every node of the file, carrying the placement (ACC) of the nodes above it.
/// Objects are found wherever they are (under an `Instance`'s `things`, a link, a field); an
/// Object's `RenderThing` attachments are placed here.
#[allow(clippy::too_many_arguments)]
fn visit(node: Node, acc: Mat, linked: bool, deferred_stage: u8, placed: bool, owner: Option<Owner>, masks: &[u32], depth: u32, sink: &mut Sink) {
    if depth > MAX_DEPTH {
        return;
    }
    // A Collision holds no drawn model.
    if node.type_name() == Some("Collision") {
        return;
    }
    if node.type_name() == Some("VisibilityTrigger") {
        let key = flat(&acc).map(|v| (v * KEY_SCALE).round() as i64);
        if !sink.seen_triggers.insert((node.offset().unwrap_or(0), key)) { return; }
        let Some(area @ 1..=32) = node.get("area").and_then(Node::int) else { return; };
        for rect in node.get("trigRects").into_iter().flat_map(Node::items).filter_map(matrix_of) {
            let world = affine(&compose(&rect, &acc));
            if world.determinant().abs() > 1e-8 {
                sink.visibility_boxes.push(VisibilityBox { channel: (area - 1) as u8, local_from_world: world.inverse() });
            }
        }
        return;
    }
    if !carries_placement(node) {
        return;
    }
    // The number of DamageLinks above: 0 intact, 1 the first damage stage, 2 the stage that stage's DamageLink builds...
    let deferred_stage = deferred_stage + u8::from(node.type_name() == Some("DamageLink"));
    // [assumed] An Object is in the world only where an Instance puts it: an Object that
    // is a root of the file is the stored copy its Instances point at (740 of Shanghai's
    // were drawn a second time at the map's origin before this rule).
    let instance = node.get("matrix").and_then(matrix_of).filter(|_| node.get("things").is_some());
    let placed = placed || instance.is_some();
    let acc = instance.map_or(acc, |m| compose(&m, &acc));
    let mut inherited_masks = masks.to_vec();
    if instance.is_some() {
        inherited_masks.extend(node.get("things").into_iter().flat_map(Node::items)
            .filter(|n| n.type_name() == Some("VisibilityMask"))
            .filter_map(|n| n.get("mask").and_then(Node::int).map(|v| v as u32)));
    }
    let masks = inherited_masks.as_slice();
    // [assumed] the innermost placed Instance owns what is under it (`collision_world::Owner`).
    let owner = match (instance.is_some(), node.offset()) {
        (true, Some(at)) => Some((sink.pack, sink.chunk, at)),
        _ => owner,
    };
    // [data] A Colorizer sits in the placed Instance's own `things` (shanghai_01 `TaxiCar_White02`): its
    // colour is picked once for that Instance [game 007db420].
    if let (true, Some(key)) = (instance.is_some(), owner) {
        if !sink.tints.contains_key(&key) {
            let colorizer = node.get("things").into_iter().flat_map(Node::items)
                .find(|n| n.type_name() == Some("Colorizer")).and_then(crate::colorizer::Colorizer::read);
            if let Some(colorizer) = colorizer {
                let colour = colorizer.pick(sink.colour_seed, crate::colorizer::TIME_S);
                sink.tints.insert(key, colour);
            }
        }
    }
    // As `collision_world::visit`: a typed node reached again at the same place and in the
    // same state has nothing new under it (deep_six_01 chunk 13 is walked without end otherwise).
    if let (Some(name), Some(at)) = (node.type_name(), node.offset()) {
        let key = flat(&acc).map(|v| (v * KEY_SCALE).round() as i64);
        if !sink.visited.insert((at, name.as_ptr() as usize, key, linked, placed, owner)) {
            return;
        }
    }
    if let Some(attachments) = node.get("attachments") {
        let object_offset = node.offset().unwrap_or(0);
        for attachment in attachments.items() {
            let Some(thing) = attachment.get("thing") else { continue };
            let index = attachment.get("index").and_then(Node::int).unwrap_or(0);
            let riding = compose(&bind_of(node, index), &acc);
            if placed && thing.type_name() == Some("ParticleEmitter") {
                let key = (thing.offset().unwrap_or(0), flat(&riding).map(|v| (v * KEY_SCALE).round() as i64), owner);
                if sink.seen_emitters.insert(key) {
                    if let Some(template) = thing.get("id").and_then(crate::particles::Template::read) {
                        sink.emitters.push(PlacedEmitter { template, world: flat32(&riding),
                            auto_start: thing.get("autoStart").and_then(Node::int).unwrap_or(1) != 0,
                            use_rotation: thing.get("useRot").and_then(Node::int).unwrap_or(1) != 0,
                            use_scale: thing.get("useScale").and_then(Node::int).unwrap_or(0) != 0,
                            spawn_chance: thing.get("spawnChance").and_then(Node::float).unwrap_or(1.0),
                            object_position: glam::Vec3::from_array(acc[3].map(|v| v as f32)),
                            source: format!("{} @ {object_offset:#x} node {index}", sink.label),
                            owner,
                            stage: deferred_stage,
                        });
                    }
                }
                continue;
            }
            // [LS-57] A placed TriggerableParticle: collected only (not started). Same rule as the emitter above.
            if placed && deferred_stage == 0 && thing.type_name() == Some("TriggerableParticle") {
                let key = (thing.offset().unwrap_or(0), flat(&riding).map(|v| (v * KEY_SCALE).round() as i64), owner);
                if sink.seen_emitters.insert(key) {
                    if let Some(template) = thing.get("id").and_then(crate::particles::Template::read) {
                        sink.trig_emitters.push(PlacedTrigEmitter { template, world: flat32(&riding),
                            lux_script_name: thing.get("LuxScriptName").and_then(|n| n.enum_hash().or_else(|| n.int().map(|v| v as u32))).unwrap_or(0),
                            accept_null: thing.get("acceptNullTriggerMessages").is_some_and(|f| match f.enum_name() {
                                Some("true") => true,
                                Some("false") => false,
                                _ => f.int().is_some_and(|v| v != 0),
                            }),
                            use_rotation: thing.get("useRot").and_then(Node::int).unwrap_or(1) != 0,
                            use_scale: thing.get("useScale").and_then(Node::int).unwrap_or(0) != 0,
                            owner,
                            source: format!("{} @ {object_offset:#x} node {index}", sink.label),
                        });
                    }
                }
                continue;
            }
            if thing.type_name() == Some("RenderThing") {
                if !placed {
                    continue;
                }
                render(thing, node, index, riding, acc, linked, deferred_stage, object_offset, owner, masks, sink);
                continue;
            }
            let linked_here = linked || matches!(thing.type_name(), Some("DamageLink" | "DetachLink"));
            visit(thing, riding, linked_here, deferred_stage, placed, owner, masks, depth + 1, sink);
        }
    }
    for (hash, child) in node.fields() {
        if matches!(node.file().name(hash), Some("attachments" | "sub_matrices" | "matrix" | "hierarchy")) {
            continue;
        }
        visit(child, acc, linked, deferred_stage, placed, owner, masks, depth + 1, sink);
    }
    if node.len() > 0 {
        for child in node.items() {
            visit(child, acc, linked, deferred_stage, placed, owner, masks, depth + 1, sink);
        }
    }
}

/// One RenderThing at one world matrix, under `owner`: each mesh entry is a placed model.
#[allow(clippy::too_many_arguments)]
fn render(thing: Node, object: Node, index: i64, world: Mat, object_world: Mat, linked: bool, deferred_stage: u8, object_offset: usize, owner: Option<Owner>, masks: &[u32], sink: &mut Sink) {
    let thing_offset = thing.offset().unwrap_or(0);
    let key = flat(&world).map(|v| (v * KEY_SCALE).round() as i64);
    if !sink.seen.insert((thing_offset, key, owner)) {
        sink.repeats += 1;
        return;
    }
    let sub_matrices = object.get("sub_matrices").map_or(0, |s| s.len());
    let meshes: Vec<Node> = thing.get("meshes").into_iter().flat_map(Node::items).collect();
    let mut no_shadow = false;
    let mut no_reflection = false;
    let mut blend_lod = false;
    let mut spin = None;
    let mut ifl = None;
    for attachment in object.get("attachments").into_iter().flat_map(Node::items) {
        let Some(component) = attachment.get("thing") else { continue };
        match component.type_name() {
            Some("NoShadowTag") => no_shadow = true,
            Some("ShadowTag") => no_shadow = false,
            Some("NoReflectionTag") => no_reflection = true,
            Some("ReflectionTag") => no_reflection = false,
            Some("BlendLodTag") => blend_lod = true,
            Some("IFLCtrl") => ifl = Some(crate::ifl::Definition::read(component)),
            Some("SpinnyThing") => {
                let axis = component.get("axis").map(Node::floats).unwrap_or_default();
                if axis.len() != 3 { continue; }
                let node = attachment.get("index").and_then(Node::int).unwrap_or(0);
                if node != 0 && node != index { continue; }
                let pivot_frame = affine(&compose(&bind_of(object, node), &object_world));
                // [assumed] rotate the flattened placement about the component's world axis;
                // hierarchy updates with nonuniform scale are not yet compared (status).
                let axis = pivot_frame.transform_vector3(glam::Vec3::new(axis[0], axis[1], axis[2])).normalize_or_zero();
                if axis != glam::Vec3::ZERO {
                    spin = Some(Spin { axis, pivot: pivot_frame.w_axis.truncate(),
                        radians_per_second: component.get("speed").and_then(Node::float).unwrap_or(0.0).to_radians() });
                }
            }
            _ => {}
        }
    }
    for (n, mesh) in meshes.iter().enumerate() {
        let Some(model) = mesh.get("model").and_then(Node::text) else { continue };
        let lod_distances = mesh.get("lod_distances").map(Node::floats).unwrap_or_default();
        sink.out.push(Found {
            model,
            world: flat32(&world),
            lod: n,
            source: format!("{} @ {object_offset:#x} node {index} thing {thing_offset:#x} mesh {n}", sink.label),
            index,
            sub_matrices,
            meshes: meshes.len(),
            lod_distances,
            blend_lod,
            linked,
            deferred_stage,
            object_offset,
            owner,
            visibility_masks: masks.to_vec(),
            no_shadow,
            no_reflection,
            spin,
            ifl,
            tint: owner.and_then(|key| sink.tints.get(&key).copied()),
            debris: crate::debris::DebrisDef::of_object(object),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::hash::lower33;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    #[test]
    fn the_escort_barriers_stage_pieces_carry_their_debris() {
        let pack = crate::install::dir().join("levels/us_test/missions/usa_east_ltrain_a_escort/mission.str");
        if !pack.exists() {
            return;
        }
        let level = load(&pack).unwrap();
        let mut by_stage = BTreeMap::<u8, usize>::new();
        for p in level.placed.iter().filter(|p| p.debris.is_some() && p.model.starts_with("EscortBarrier_")) {
            *by_stage.entry(p.stage).or_default() += 1;
        }
        let rock = level.placed.iter().find(|p| p.model == "EscortBarrier_20M.Debris_Rock03").and_then(|p| p.debris).unwrap();
        println!("barrier debris pieces by stage {by_stage:?}; Debris_Rock03 {rock:?}");
        assert!(by_stage.keys().all(|&s| s > 0), "every piece is in a damage stage");
        assert_eq!((rock.speed, rock.speed_min, rock.die_time, rock.die_condition), (20.0, 8.0, 5.0, crate::debris::DIE_SPAWN_TIMER));
        assert!((rock.spread - std::f32::consts::PI).abs() < 1e-4 && rock.spin_type == 0xd2041280);
    }

    #[test]
    fn the_escorts_truck_is_its_base_poses_models() {
        let pack = crate::install::dir().join("levels/us_test/missions/usa_east_ltrain_a_escort/mission.str");
        if !pack.exists() {
            return;
        }
        let truck = scene_models(&pack, "cargotruck_basepose").unwrap();
        let level = load(&pack).unwrap();
        let in_library = truck.iter().filter(|(m, _)| level.library.meshes.contains_key(&lower33(m))).count();
        let ys: Vec<f32> = truck.iter().map(|(_, w)| w[10]).collect();
        let (lo, hi) = ys.iter().fold((f32::MAX, f32::MIN), |(a, b), &y| (a.min(y), b.max(y)));
        let stray = level.placed.iter().filter(|p| p.model.starts_with("CargoTruck.")).count();
        println!("truck: {} models, {in_library} in the mission library, y {lo:.2}..{hi:.2}; the level walk places {stray} CargoTruck models", truck.len());
        for (m, w) in truck.iter().take(6) {
            println!("  {m} at ({:.2}, {:.2}, {:.2})", w[9], w[10], w[11]);
        }
        assert!(truck.len() >= 10 && in_library == truck.len());
        assert!(truck.iter().all(|(m, _)| m.starts_with("CargoTruck.")), "only the intact truck (no damage stage)");
    }

    #[test]
    fn visibility_channels_use_oriented_boxes_and_union_each_channel() {
        use glam::{Mat4, Quat, Vec3};
        let world = Mat4::from_scale_rotation_translation(Vec3::new(2.0, 4.0, 3.0), Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), Vec3::new(10.0, 20.0, 30.0));
        let boxes = [VisibilityBox { channel: 0, local_from_world: world.inverse() },
            VisibilityBox { channel: 4, local_from_world: Mat4::from_translation(Vec3::new(-50.0, 0.0, 0.0)) }];
        assert_eq!(active_visibility(&[], Vec3::ZERO), u32::MAX);
        assert_eq!(active_visibility(&boxes, Vec3::new(10.0, 20.0, 30.0)), 1);
        assert_eq!(active_visibility(&boxes, world.transform_point3(Vec3::new(1.005, 0.0, 0.0))), 1);
        assert_eq!(active_visibility(&boxes, world.transform_point3(Vec3::new(1.02, 0.0, 0.0))), 0);
        assert_eq!(active_visibility(&boxes, Vec3::new(50.0, 0.0, 0.0)), 1 << 4);
    }

    #[test]
    fn us_test_retains_authored_visibility_shadow_tags_and_spin_speeds() {
        let Some(zone) = install_path(r"levels\us_test\zone.str") else { return };
        let level = load(&zone).unwrap();
        assert_eq!(level.placed.len(), 14629);
        assert_eq!(level.placed.iter().filter(|p| p.deferred_stage).count(), 9316);
        assert!(level.placed.iter().all(|p| p.deferred_stage == p.linked),
            "US_Test's detach pieces are all inside deferred DamageLink stages");
        assert_eq!(level.visibility_boxes.len(), 26);
        assert_eq!(level.placed.iter().filter(|p| !p.visibility_masks.is_empty()).count(), 5782);
        assert_eq!(level.placed.iter().filter(|p| p.no_shadow).count(), 1754);
        let clouds = level.placed.iter().find(|p| p.model.eq_ignore_ascii_case("Sky.USWEST_01_dome")).unwrap();
        let spin = clouds.spin.unwrap();
        assert!((spin.radians_per_second - 0.2f32.to_radians()).abs() < 1e-7);
        assert_eq!(spin.axis, glam::Vec3::Z);
        let fans: Vec<_> = level.placed.iter().filter(|p| p.model == "AC_Vent_B.Vent").collect();
        assert_eq!(fans.len(), 23);
        assert!(fans.iter().all(|p| (p.spin.unwrap().radians_per_second + 60f32.to_radians()).abs() < 1e-7));
    }

    /// The install, when it is there. The tests skip (pass) when it is not.
    fn install_path(relative: &str) -> Option<PathBuf> {
        let dir = PathBuf::from(crate::install::dir_string());
        let path = dir.join(relative);
        path.is_file().then_some(path)
    }

    fn shanghai() -> Option<PathBuf> {
        install_path(r"levels\shanghai_01\zone.str")
    }

    /// Shanghai's zone loads: the library reads without problems, and the placed models and the
    /// distinct model names are the counts measured on the install (survey, 2026-10-11).
    #[test]
    fn shanghai_loads_and_counts() {
        let Some(zone) = shanghai() else { return };
        let level = load(&zone).unwrap();
        let distinct: BTreeSet<&str> = level.placed.iter().map(|p| p.model.as_str()).collect();
        println!("shanghai_01: {} placed models, {} distinct model names, {} meshes in the library, {} library problems",
            level.placed.len(), distinct.len(), level.library.meshes.len(), level.library.problems.len());
        assert_eq!(level.library.problems.len(), 0, "{:?}", level.library.problems);
        assert_eq!(level.placed.len(), 12629);
        assert_eq!(distinct.len(), 1551);
        assert!(level.placed.iter().all(|p| p.lod == 0));
    }

    /// Every placed model's name is a mesh of the library (its hash is the library key and the
    /// name matches). The survey measured 13497 of 13497.
    #[test]
    fn shanghai_names_resolve() {
        let Some(zone) = shanghai() else { return };
        let level = load(&zone).unwrap();
        let resolved = level
            .placed
            .iter()
            .filter(|p| level.library.meshes.get(&lower33(&p.model)).is_some_and(|m| m.name.eq_ignore_ascii_case(&p.model)))
            .count();
        println!("shanghai_01: names resolved in the library: {resolved} of {}", level.placed.len());
        let unresolved: Vec<&str> = level
            .placed
            .iter()
            .filter(|p| !level.library.meshes.contains_key(&lower33(&p.model)))
            .map(|p| p.model.as_str())
            .take(10)
            .collect();
        println!("shanghai_01: first unresolved names: {unresolved:?}");
        assert_eq!(resolved, level.placed.len());
    }

    /// The bounding box of the placement origins (row 3 of each world matrix), printed, and a
    /// loose check against the collision map's range (x -688..840, y -522..689).
    #[test]
    fn shanghai_origin_bounds() {
        let Some(zone) = shanghai() else { return };
        let level = load(&zone).unwrap();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &level.placed {
            for i in 0..3 {
                lo[i] = lo[i].min(p.world[9 + i]);
                hi[i] = hi[i].max(p.world[9 + i]);
            }
        }
        println!("shanghai_01 placement origins: x {:.1}..{:.1}  y {:.1}..{:.1}  z {:.2}..{:.2}", lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]);
        let inside = level
            .placed
            .iter()
            .filter(|p| (-900.0..=1000.0).contains(&p.world[9]) && (-700.0..=850.0).contains(&p.world[10]))
            .count();
        println!("shanghai_01 origins inside x -900..1000, y -700..850: {inside} of {}", level.placed.len());
        assert!(inside * 10 >= level.placed.len() * 9, "{inside} of {}", level.placed.len());
    }

    /// The placement facts of the survey: how many Objects carry RenderThings, how many ride a
    /// node other than their Object's origin, and how many sit under a damage or detach link.
    #[test]
    fn shanghai_placement_facts() {
        let Some(zone) = shanghai() else { return };
        let pack = Pack::open(&zone).unwrap();
        let (found, repeats) = walk_pack(&pack, &zone).unwrap();
        let objects: BTreeSet<(&str, usize)> = found.iter().map(|f| (f.source.split(" @ ").next().unwrap_or(""), f.object_offset)).collect();
        let riding = found.iter().filter(|f| f.index > 0).count();
        let riding_without_nodes = found.iter().filter(|f| f.index > 0 && f.sub_matrices == 0).count();
        let linked = found.iter().filter(|f| f.linked).count();
        let meshes_per_thing: BTreeSet<usize> = found.iter().map(|f| f.meshes).collect();
        let mut per_chunk: BTreeMap<&str, usize> = BTreeMap::new();
        for f in &found {
            *per_chunk.entry(f.source.split(" @ ").next().unwrap_or("")).or_default() += 1;
        }
        println!("shanghai_01 placements per data chunk: {per_chunk:?}");
        println!("shanghai_01: {} Objects carry RenderThings; {} placements ride a node > 0 ({riding_without_nodes} of them with no sub_matrices); {linked} sit under a DamageLink or DetachLink; {repeats} repeats skipped; mesh entries per RenderThing {meshes_per_thing:?}", objects.len(), riding);
        assert_eq!(objects.len(), 1550);
        assert_eq!(found.len(), 12629);
        // 13,923 before the walk stopped re-entering a typed node at the same place (`visit`):
        // those repeats were the same subtrees walked twice.
        assert_eq!(repeats, 0);
        assert_eq!(riding, 1247);
        assert_eq!(riding_without_nodes, 0);
        assert_eq!(linked, 7778);
        assert_eq!(meshes_per_thing, BTreeSet::from([1]));
    }

    /// The escort mission's drawn models and their Instances (`--mission USA_East_LTrain_A_Escort`): how many have an
    /// owner, how many Instances, and how many (stored RenderThing, world matrix) pairs sit under two Instances (the
    /// cases the old keys merged). Needs the install; passes without it.
    #[test]
    fn escort_placed_models_know_their_instance() {
        use crate::formats::hash::crc32;
        use crate::formats::missionlist::MissionList;
        use std::collections::{BTreeMap, BTreeSet};
        let dir = PathBuf::from(crate::install::dir_string());
        if !dir.join("levels").is_dir() {
            return;
        }
        let list = MissionList::load(&dir).expect("mission list");
        let Some(mission) = list.by_hash(crc32(b"USA_East_LTrain_A_Escort")) else { return };
        let packs: Vec<PathBuf> = list.pack_paths(mission).iter().map(|p| dir.join(p)).collect();
        let level = load_packs(&packs).expect("escort level");
        let owners: BTreeSet<(usize, usize, usize)> = level.placed.iter().filter_map(|p| p.owner).collect();
        let owned = level.placed.iter().filter(|p| p.owner.is_some()).count();
        let linked_owned = level.placed.iter().filter(|p| p.owner.is_some() && p.linked).count();
        let mut at: BTreeMap<(String, [u32; 12]), BTreeSet<(usize, usize, usize)>> = BTreeMap::new();
        for p in &level.placed {
            if let Some(o) = p.owner {
                at.entry((p.source.clone(), p.world.map(f32::to_bits))).or_default().insert(o);
            }
        }
        let same_matrix = at.values().filter(|s| s.len() > 1).count();
        println!(
            "escort level: {} placed models, {owned} under {} Instances ({linked_owned} under a link); {same_matrix} (stored thing, matrix) pairs under two Instances",
            level.placed.len(),
            owners.len()
        );
        assert!(owned > 0);
    }

    /// The LOD question, measured: where the library gives the named model a LOD chain
    /// (`Mesh::lods`), how often the chain's first mesh is smaller than the named one, and
    /// whether the RenderThing's `lod_distances` are the chain's distances.
    #[test]
    fn shanghai_lod_chain_against_named_model() {
        let Some(zone) = shanghai() else { return };
        let pack = Pack::open(&zone).unwrap();
        let library = Library::load(&pack);
        let (found, _) = walk_pack(&pack, &zone).unwrap();
        let (mut with_chain, mut resolved, mut named_bigger) = (0usize, 0usize, 0usize);
        let mut exceptions: BTreeSet<&str> = BTreeSet::new();
        let (mut same_distances, mut distances_compared) = (0usize, 0usize);
        for f in &found {
            let Some(mesh) = library.meshes.get(&lower33(&f.model)) else { continue };
            let Some(&(lod_id, _)) = mesh.lods.first() else { continue };
            with_chain += 1;
            let Some(lower) = library.meshes.get(&lod_id) else { continue };
            resolved += 1;
            let named: usize = mesh.parts.iter().map(|q| q.positions.len()).sum();
            let chain: usize = lower.parts.iter().map(|q| q.positions.len()).sum();
            if named > chain {
                named_bigger += 1;
            } else {
                exceptions.insert(f.model.as_str());
            }
            // The RenderThing's lod_distances against the chain's own distances.
            let chain_distances: Vec<f32> = mesh.lods.iter().map(|&(_, d)| d).collect();
            distances_compared += 1;
            if f.lod_distances == chain_distances {
                same_distances += 1;
            }
        }
        println!("shanghai_01: placed models whose mesh has a LOD chain: {with_chain}; first chain mesh resolves: {resolved}; named mesh has more vertices: {named_bigger}");
        println!("shanghai_01: placements whose lod_distances equal the chain's distances: {same_distances} of {distances_compared}");
        println!("shanghai_01: models where the chain's first mesh is not smaller than the named one: {exceptions:?}");
        assert_eq!(with_chain, 2028);
        assert_eq!(resolved, 2028);
        assert_eq!(named_bigger, 1967);
        assert_eq!(same_distances, 237);
    }
}
