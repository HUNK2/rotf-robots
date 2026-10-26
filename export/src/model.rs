//! A character's object (its node tree and the meshes on it) as a glTF scene, built the way the
//! game builds it for drawing (`game/src/assets.rs` `spawn`): a node per object node, a "frame"
//! child per node for the node's own scale (what the skins bind to), rigid parts on their node's
//! frame, one skin per skinned part with a joint per palette entry.

use std::collections::HashMap;
use std::path::Path;

use glam::{Mat4, Quat, Vec3};
use serde_json::{Map, Value, json};
use tf2_core::formats::anim::{AttackRef, Blend, Clip, ROOT_CHANNEL, Track};
use tf2_core::formats::hash::{crc32, lower33};
use tf2_core::formats::model::{Library, Material, Part};
use tf2_core::formats::scene::{CollisionDef, ObjectDef, Shape};

use crate::describe::num;
use crate::glb::{ARRAY_BUFFER, ELEMENT_ARRAY_BUFFER, Glb};
use crate::names::Names;
use crate::texture;

/// What one written file holds, for the report.
#[derive(Default, Debug)]
pub struct Summary {
    pub nodes: usize,
    pub parts: usize,
    pub skins: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub materials: usize,
    pub textures: usize,
    pub animations: usize,
    /// Clips in the list that move no node of this model: `pieces` gives a robot clip to the object
    /// it moves, so these move none the pack has (Optimus's face-part clips name nodes no object of
    /// his has; Megatron's tank-special clips key no node, only events).
    pub other_model_clips: Vec<String>,
    pub problems: Vec<String>,
}

/// A clip to write as an animation.
pub struct ClipOut<'a> {
    /// Its name in the file: `<set>.<id>`.
    pub name: String,
    pub clip: &'a Clip,
    pub looping: bool,
    /// Its keys are offsets laid over the pose under it (`AnimRef::additive`, the weapon set's
    /// aim clips). They are written laid over the rest pose, so an engine that takes an additive
    /// clip as "pose minus reference pose" gets the offsets back.
    pub additive: bool,
    /// The attack its reference carries (a melee clip's damage, ranges and turning), for the JSON.
    pub attack: Option<AttackRef>,
    pub extras: Value,
}

/// The game's ticks (1/60 s) as glTF's seconds.
const TICK: f32 = 1.0 / 60.0;

/// A track's keys as the game plays them, with a key added at the clip's end when the last one
/// is before it (the end value by the game's own sampling, which blends round to the first key
/// on a looping clip), and a time that repeats nudged on: glTF wants times that rise. None for a
/// track with no key.
fn keys<T: Blend>(track: &Track<T>, length: f32, looping: bool) -> Option<Vec<(f32, T)>> {
    if !track.is_keyed() {
        return None;
    }
    let mut keys: Vec<(f32, T)> = track.times().iter().copied().zip(track.values().iter().copied()).collect();
    if keys.last().is_some_and(|&(t, _)| t < length) {
        keys.push((length, track.sample(length, length, looping)?));
    }
    for k in 1..keys.len() {
        if keys[k].0 <= keys[k - 1].0 {
            keys[k].0 = keys[k - 1].0 + 1e-4;
        }
    }
    Some(keys)
}

/// The widest turn left between two rotation keys.
const TURN_STEP: f32 = 3.0 * std::f32::consts::PI / 180.0;

/// Rotation keys with the game's own blend filled in where two keys are more than `TURN_STEP`
/// apart. The game blends turns along a straight line, then normalises (`Blend for [f32; 4]`);
/// glTF players blend along the sphere. The two agree at the keys and drift apart between them
/// as the turn grows (2 cm at Bumblebee's hand in `AttackFast_2`); keys sampled by the game's rule
/// every few degrees keep them together. A step (two keys at one time) is left a step.
fn even_turns(track: &Track<[f32; 4]>, keys: Vec<(f32, [f32; 4])>, length: f32, looping: bool) -> Vec<(f32, [f32; 4])> {
    let mut out = Vec::with_capacity(keys.len());
    for pair in keys.windows(2) {
        let ((t0, a), (t1, b)) = (pair[0], pair[1]);
        out.push((t0, a));
        let dot = (0..4).map(|k| a[k] * b[k]).sum::<f32>().abs().min(1.0);
        let angle = 2.0 * dot.acos();
        if angle > TURN_STEP && t1 - t0 > 1e-3 {
            let parts = (angle / TURN_STEP).ceil() as usize;
            for j in 1..parts {
                let t = t0 + (t1 - t0) * j as f32 / parts as f32;
                if let Some(q) = track.sample(t, length, looping) {
                    out.push((t, q));
                }
            }
        }
    }
    out.extend(keys.last().copied());
    out
}

/// The game's space (x right, y forward, z up) into glTF's (y up, the model facing +Z): what the
/// root node turns. The game's own drawing turns by the first half only (`model_space_rotation`,
/// facing -Z, Bevy's forward); glTF's convention is that a model faces +Z.
pub fn root_turn() -> Quat {
    Quat::from_rotation_y(std::f32::consts::PI) * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

/// The file's matrices are for row vectors: three axis rows and a position row.
pub fn matrix(rows: &[[f32; 3]; 4]) -> Mat4 {
    Mat4::from_cols(
        Vec3::from(rows[0]).extend(0.0),
        Vec3::from(rows[1]).extend(0.0),
        Vec3::from(rows[2]).extend(0.0),
        Vec3::from(rows[3]).extend(1.0),
    )
}

/// The glTF node of collision body `index` of an object, riding node `node`.
pub fn collision_node(node: &str, index: usize) -> String {
    format!("{node}.collision.{index}")
}

/// A collision body as the JSON and its glTF node give it: its shape as stored (`sphere`, `box`,
/// `cylinder` along its z), its place in its node's space as a move, turn and scale, the sizes
/// with that scale in them as the port reads them (`tf2_core::melee::Body::new`), its mask preset
/// and its material. The loader squares the matrix's axes up and keeps their lengths, so the move,
/// turn and scale are the whole matrix.
pub fn collision(c: &CollisionDef, names: &Names) -> Value {
    let (scale, rotation, translation) = matrix(&c.matrix).to_scale_rotation_translation();
    let size = scale.abs();
    let mut body = Map::new();
    let scaled = match c.shape {
        Shape::Sphere { radius } => {
            body.insert("shape".into(), json!("sphere"));
            body.insert("radius".into(), num(radius));
            json!({ "radius": num(radius * size.x) })
        }
        Shape::Cuboid { half } => {
            body.insert("shape".into(), json!("box"));
            body.insert("half_extents".into(), json!(half.map(num)));
            json!({ "half_extents": (Vec3::from(half) * size).to_array().map(num) })
        }
        Shape::Cylinder { radius, half_height } => {
            body.insert("shape".into(), json!("cylinder"));
            body.insert("radius".into(), num(radius));
            body.insert("half_height".into(), num(half_height));
            json!({ "radius": num(radius * size.x), "half_height": num(half_height * size.z) })
        }
    };
    let name = |h: u32| if h == 0 { Value::Null } else { json!(names.name(h)) };
    body.insert("translation".into(), json!(translation.to_array().map(num)));
    body.insert("rotation".into(), json!(rotation.normalize().to_array().map(num)));
    body.insert("scale".into(), json!(scale.to_array().map(num)));
    body.insert("scaled".into(), scaled);
    body.insert("mask_preset".into(), name(c.mask_preset));
    body.insert("material".into(), name(c.material));
    Value::Object(body)
}

// A material's blend word (`game/src/assets.rs` `blend_of`, where the values are tagged): Normal and
// Additive by name, two unnamed words folded onto them, the cut-out and the boosted blend.
const BLEND_NORMAL: u32 = crc32(b"Normal");
const BLEND_ADDITIVE: u32 = crc32(b"Additive");
const BLEND_CUTOUT: u32 = 0xbc07_610c;
const BLEND_BOOST: u32 = 0x3467_929c;

pub fn blend_of(source: &Material) -> Option<u32> {
    source.word("blendmode").map(|blend| match blend {
        0xd630_0862 => BLEND_NORMAL,
        0xcdac_d688 => BLEND_ADDITIVE,
        other => other,
    })
}

/// A texture as written under `textures/`: its file stem, and whether it has a PNG (a cube map
/// has only its DDS).
#[derive(Clone)]
pub struct TextureFile {
    pub stem: String,
    pub png: bool,
}

impl TextureFile {
    /// Its path from the output folder: the PNG, or the DDS when there is none.
    pub fn path(&self) -> String {
        if self.png { format!("textures/{}.png", self.stem) } else { self.dds() }
    }

    pub fn dds(&self) -> String {
        format!("textures/dds/{}.dds", self.stem)
    }
}

/// What the files of one character share: the pack's library, the output folder, every texture
/// written so far (each once), and the materials the files use, in the order first used.
pub struct Shared<'a> {
    pub library: &'a Library,
    out: &'a Path,
    written: HashMap<u32, Option<TextureFile>>,
    stems: HashMap<String, u32>,
    pub materials: Vec<u32>,
    pub problems: Vec<String>,
}

impl<'a> Shared<'a> {
    pub fn new(library: &'a Library, out: &'a Path) -> Result<Self, String> {
        std::fs::create_dir_all(out.join("textures").join("dds")).map_err(|e| format!("{}: {e}", out.display()))?;
        Ok(Self { library, out, written: HashMap::new(), stems: HashMap::new(), materials: Vec::new(), problems: Vec::new() })
    }

    /// The texture by id, written the first time it is asked for: the DDS as the game stores it,
    /// and a PNG of it. An animated texture (an IFL) gives its first frame. None when the pack has
    /// no such texture.
    pub fn texture(&mut self, id: u32) -> Option<TextureFile> {
        if let Some(found) = self.written.get(&id) {
            return found.clone();
        }
        let library = self.library;
        let source = library
            .textures
            .get(&id)
            .or_else(|| library.texture_frames.get(&id).and_then(|f| f.frames.first()).and_then(|first| library.textures.get(first)));
        let made = source.map(|t| self.write_texture(t));
        self.written.insert(id, made.clone());
        made
    }

    fn write_texture(&mut self, t: &tf2_core::formats::texture::Texture) -> TextureFile {
        let stem: String = t
            .name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&t.name)
            .trim_end_matches(".dds")
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' })
            .collect();
        let stem = match self.stems.get(&stem) {
            Some(&other) if other != t.id => format!("{stem}_{:08x}", t.id),
            _ => stem,
        };
        self.stems.insert(stem.clone(), t.id);
        let file = TextureFile { stem, png: false };
        let dds = self.out.join(file.dds());
        if let Err(e) = std::fs::write(&dds, texture::dds(t)) {
            self.problems.push(format!("{}: {e}", dds.display()));
        }
        // A cube map (the reflection maps) has no single picture: its DDS is the file.
        let Some(rgba) = texture::rgba(t) else {
            if t.faces == 1 {
                self.problems.push(format!("{}: its pixels are short of its size; written as DDS only", file.stem));
            }
            return file;
        };
        let png = self.out.join("textures").join(format!("{}.png", file.stem));
        if let Err(e) = image::save_buffer(&png, &rgba, t.width, t.height, image::ExtendedColorType::Rgba8) {
            self.problems.push(format!("{}: {e}", png.display()));
            return file;
        }
        TextureFile { png: true, ..file }
    }
}

struct Writer<'s, 'a> {
    shared: &'s mut Shared<'a>,
    glb: Glb,
    materials: HashMap<u32, usize>,
    textures: HashMap<u32, Option<usize>>,
    summary: Summary,
}

/// Writes `object` and `clips` as `<out>/<file>.glb`, its textures under `<out>/textures/`.
pub fn write(object: &ObjectDef, shared: &mut Shared, clips: &[ClipOut], file: &str, names: &Names) -> Result<Summary, String> {
    let library = shared.library;
    let out = shared.out;
    let mut w = Writer { shared, glb: Glb::default(), materials: HashMap::new(), textures: HashMap::new(), summary: Summary::default() };
    let n = object.nodes.len();
    let world: Vec<Mat4> = object.nodes.iter().map(|node| matrix(&node.bind)).collect();
    let turn = root_turn();
    let root = w.glb.node(json!({ "name": file, "rotation": [turn.x, turn.y, turn.z, turn.w] }));
    // Nodes 1..=n are the object's, n+1..=2n their frames.
    let mut rest = Vec::with_capacity(n);
    for (i, node) in object.nodes.iter().enumerate() {
        let local = match node.parent {
            Some(parent) => world[parent].inverse() * world[i],
            None => world[i],
        };
        let (scale, rotation, translation) = local.to_scale_rotation_translation();
        rest.push((scale, rotation.normalize(), translation));
        // glTF cannot hold a shear. The port's drawing and posing take the same nearest move, turn
        // and scale (`game/src/assets.rs` `spawn`, `core/src/pose.rs` `Rig::new`); whether the
        // original keeps the shear is not known.
        let back = Mat4::from_scale_rotation_translation(scale, rotation, translation);
        let off = (back - local).to_cols_array().iter().fold(0.0f32, |a, v| a.max(v.abs()));
        if off > 1e-3 {
            w.summary.problems.push(format!(
                "node {i} ({}): its rest matrix is sheared ({off:.3} off); written as the nearest move, turn and scale, as the port draws it",
                node.name
            ));
        }
        w.glb.node(json!({
            "name": node.name,
            "translation": translation.to_array(),
            "rotation": rotation.normalize().to_array(),
            "scale": scale.to_array(),
        }));
    }
    for node in &object.nodes {
        w.glb.node(json!({ "name": format!("{}.frame", node.name) }));
    }
    let node_index = |i: usize| 1 + i;
    let frame_index = |i: usize| 1 + n + i;
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); 1 + 2 * n];
    for (i, node) in object.nodes.iter().enumerate() {
        children[node.parent.map_or(root, node_index)].push(node_index(i));
        children[node_index(i)].push(frame_index(i));
    }
    // Each collision body as an empty node on the node it rides, not on its frame: the game poses
    // the bodies by the node without its own scale (`tf2_core::melee`). Its shape is in `extras`.
    for (k, c) in object.collisions.iter().enumerate() {
        let body = collision(c, names);
        let at = w.glb.node(json!({
            "name": collision_node(&object.nodes[c.node].name, k),
            "translation": body["translation"],
            "rotation": body["rotation"],
            "scale": body["scale"],
            "extras": { "rotf_collision": body },
        }));
        children.push(Vec::new());
        children[node_index(c.node)].push(at);
    }

    for render in &object.renders {
        let Some(mesh) = library.meshes.get(&lower33(&render.model)) else {
            w.summary.problems.push(format!("no mesh named {}", render.model));
            continue;
        };
        let skeleton = mesh.skeleton.and_then(|id| library.skeletons.get(&id));
        // A mesh in an exclusive render-set group but not in its first set starts hidden
        // [game 00803150]: Optimus's special-attack meshes.
        let hidden = object.render_sets.iter().any(|g| {
            g.exclusive && (0..g.sets.len()).any(|i| g.contains_model(&render.model, i)) && !g.contains_model(&render.model, 0)
        });
        let mut extras = json!({ "model": render.model });
        if hidden {
            extras["render_set_hidden"] = json!(true);
            if !w.glb.extensions_used.contains(&"KHR_node_visibility") {
                w.glb.extensions_used.push("KHR_node_visibility");
            }
        }
        let mesh_node = |name: &str, mesh_index: usize| {
            let mut node = json!({ "name": name, "mesh": mesh_index, "extras": extras });
            if hidden {
                node["extensions"] = json!({ "KHR_node_visibility": { "visible": false } });
            }
            node
        };
        for (k, part) in mesh.parts.iter().enumerate() {
            // Named as the object names the model (`Optimus.Base_SFX`), as the JSON's render
            // sets do; a mesh of several parts gets one node a part.
            let name = if mesh.parts.len() == 1 { render.model.clone() } else { format!("{}.{k}", render.model) };
            let skinned = !part.palette.is_empty() && skeleton.is_some();
            let Some(primitive) = w.primitive(part, skinned, &name) else { continue };
            w.glb.meshes.push(json!({ "name": name, "primitives": [primitive] }));
            let mesh_index = w.glb.meshes.len() - 1;
            w.summary.parts += 1;
            match skeleton.filter(|_| skinned) {
                None => {
                    let at = w.glb.node(mesh_node(&name, mesh_index));
                    children.push(Vec::new());
                    children[frame_index(render.node)].push(at);
                }
                Some(bones) => {
                    // One joint per palette entry, so the bone numbers in the vertices index it
                    // directly; a bone the skeleton or the object lacks is kept on the root with
                    // no offset, so the numbering holds.
                    let mut joints = Vec::new();
                    let mut inverse = Vec::new();
                    for &bone in &part.palette {
                        let found = bones.get(bone as usize).and_then(|b| {
                            Some((object.nodes.iter().position(|node| node.bone_hash == b.name)?, b))
                        });
                        match found {
                            Some((node, b)) => {
                                joints.push(frame_index(node));
                                inverse.extend_from_slice(&matrix(&b.inverse_bind).to_cols_array());
                            }
                            None => {
                                w.summary.problems.push(format!("{name}: bone {bone} is not on the object"));
                                joints.push(root);
                                inverse.extend_from_slice(&Mat4::IDENTITY.to_cols_array());
                            }
                        }
                    }
                    let matrices = w.glb.floats(&inverse, 16, "MAT4", None, false);
                    w.glb.skins.push(json!({ "name": name, "joints": joints, "inverseBindMatrices": matrices, "skeleton": root }));
                    let skin = w.glb.skins.len() - 1;
                    w.summary.skins += 1;
                    // glTF places a skinned mesh by its joints, not by its node.
                    let mut node = mesh_node(&name, mesh_index);
                    node["skin"] = json!(skin);
                    let at = w.glb.node(node);
                    children.push(Vec::new());
                    children[root].push(at);
                }
            }
        }
    }
    for (i, list) in children.into_iter().enumerate() {
        if !list.is_empty() {
            w.glb.nodes[i]["children"] = json!(list);
        }
    }
    for clip in clips {
        match w.animation(object, &rest, clip) {
            Some(animation) => w.glb.animations.push(animation),
            None => w.summary.other_model_clips.push(clip.name.clone()),
        }
    }
    w.summary.animations = w.glb.animations.len();
    w.summary.nodes = w.glb.nodes.len();
    w.summary.materials = w.glb.materials.len();
    w.summary.textures = w.glb.images.len();
    let path = out.join(format!("{file}.glb"));
    w.glb.write(&path, root).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(w.summary)
}

impl Writer<'_, '_> {
    /// One clip as a glTF animation, by the rule the game poses by (`core/src/pose.rs` `Rig::pose`,
    /// `game/src/animation.rs` `sample`): a node the clip has a channel for takes the channel's
    /// keys, and no move, no turn and scale 1 for the parts it does not key; a channel that keys
    /// nothing, and a node with no channel, keep the rest; the root keeps its rest place and turn
    /// (its travel is movement) and takes the scale; a node's own scale goes on its frame,
    /// never under 0.001. The game's own keys are written, linearly blended between, as it plays
    /// them; turns are stored for row vectors and written conjugated.
    fn animation(&mut self, object: &ObjectDef, rest: &[(Vec3, Quat, Vec3)], out: &ClipOut) -> Option<Value> {
        let clip = out.clip;
        let n = object.nodes.len();
        let (length, looping, additive) = (clip.length, out.looping, out.additive);
        let mut channels = Vec::new();
        let mut samplers = Vec::new();
        let mut inputs: HashMap<Vec<u32>, usize> = HashMap::new();
        let mut add = |w: &mut Self, node: usize, path: &str, times: Vec<f32>, values: Vec<f32>, kind: &str| {
            let key: Vec<u32> = times.iter().map(|t| t.to_bits()).collect();
            let input = *inputs.entry(key).or_insert_with(|| {
                let seconds: Vec<f32> = times.iter().map(|t| t * TICK).collect();
                w.glb.floats(&seconds, 1, "SCALAR", None, true)
            });
            let width = if kind == "VEC4" { 4 } else { 3 };
            let output = w.glb.floats(&values, width, kind, None, false);
            samplers.push(json!({ "input": input, "output": output, "interpolation": "LINEAR" }));
            channels.push(json!({ "sampler": samplers.len() - 1, "target": { "node": node, "path": path } }));
        };
        let split3 = |k: Vec<(f32, [f32; 3])>, f: &dyn Fn(Vec3) -> Vec3| -> (Vec<f32>, Vec<f32>) {
            (k.iter().map(|&(t, _)| t).collect(), k.iter().flat_map(|&(_, v)| f(Vec3::from(v)).to_array()).collect())
        };
        for (i, node) in object.nodes.iter().enumerate() {
            let Some(channel) = clip.channel(node.name_hash).filter(|c| c.keys_any()) else { continue };
            let root = node.name_hash == ROOT_CHANNEL;
            if root && additive {
                continue;
            }
            let (rest_s, rest_r, rest_t) = rest[i];
            let at = 1 + i;
            if !root {
                match keys(&channel.position, length, looping) {
                    Some(k) => {
                        let (times, values) = split3(k, &|v| if additive { rest_t + v } else { v });
                        add(self, at, "translation", times, values, "VEC3");
                    }
                    None if !additive => add(self, at, "translation", vec![0.0], vec![0.0; 3], "VEC3"),
                    None => {}
                }
                match keys(&channel.rotation, length, looping).map(|k| even_turns(&channel.rotation, k, length, looping)) {
                    Some(k) => {
                        let mut previous = Quat::IDENTITY;
                        let mut values = Vec::with_capacity(k.len() * 4);
                        for (j, &(_, q)) in k.iter().enumerate() {
                            let turn = Quat::from_xyzw(-q[0], -q[1], -q[2], q[3]);
                            let mut turn = if additive { rest_r * turn } else { turn }.normalize();
                            // The same turn either sign; kept on one side for importers that
                            // blend keys component by component.
                            if j > 0 && turn.dot(previous) < 0.0 {
                                turn = -turn;
                            }
                            previous = turn;
                            values.extend_from_slice(&turn.to_array());
                        }
                        add(self, at, "rotation", k.iter().map(|&(t, _)| t).collect(), values, "VEC4");
                    }
                    None if !additive => add(self, at, "rotation", vec![0.0], vec![0.0, 0.0, 0.0, 1.0], "VEC4"),
                    None => {}
                }
            }
            match keys(&channel.scale, length, looping) {
                Some(k) => {
                    let (times, values) = split3(k, &|v| if additive { rest_s * v } else { v });
                    add(self, at, "scale", times, values, "VEC3");
                }
                None if !additive => add(self, at, "scale", vec![0.0], vec![1.0; 3], "VEC3"),
                None => {}
            }
            if let Some(k) = keys(&channel.own_scale, length, looping) {
                let (times, values) = split3(k, &|v| v.max(Vec3::splat(0.001)));
                add(self, 1 + n + i, "scale", times, values, "VEC3");
            }
        }
        if channels.is_empty() {
            return None;
        }
        Some(json!({ "name": out.name, "channels": channels, "samplers": samplers, "extras": out.extras }))
    }

    fn primitive(&mut self, part: &Part, skinned: bool, name: &str) -> Option<Value> {
        let count = part.positions.len();
        if count == 0 || part.indices.is_empty() {
            return None;
        }
        if !part.indices.len().is_multiple_of(3) || part.indices.iter().any(|&i| i as usize >= count) {
            self.summary.problems.push(format!("{name}: its triangles do not fit its vertices; left out"));
            return None;
        }
        let flat = |v: &[[f32; 3]]| v.iter().flatten().copied().collect::<Vec<f32>>();
        let mut attributes = serde_json::Map::new();
        let positions = self.glb.floats(&flat(&part.positions), 3, "VEC3", Some(ARRAY_BUFFER), true);
        attributes.insert("POSITION".into(), json!(positions));
        if part.normals.len() == count {
            let normals: Vec<[f32; 3]> = part.normals.iter().map(|&n| Vec3::from(n).try_normalize().unwrap_or(Vec3::Z).to_array()).collect();
            attributes.insert("NORMAL".into(), json!(self.glb.floats(&flat(&normals), 3, "VEC3", Some(ARRAY_BUFFER), false)));
            if part.tangents.len() == count && part.binormals.len() == count {
                // glTF rebuilds the bitangent as cross(normal, tangent) times the sign in w, as the
                // game's drawing does.
                let tangents: Vec<f32> = (0..count)
                    .flat_map(|i| {
                        let (n, t, b) = (Vec3::from(normals[i]), Vec3::from(part.tangents[i]), Vec3::from(part.binormals[i]));
                        let t = t.try_normalize().unwrap_or(Vec3::X);
                        let sign = if n.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 };
                        [t.x, t.y, t.z, sign]
                    })
                    .collect();
                attributes.insert("TANGENT".into(), json!(self.glb.floats(&tangents, 4, "VEC4", Some(ARRAY_BUFFER), false)));
            }
        }
        for (key, uvs) in [("TEXCOORD_0", &part.uvs), ("TEXCOORD_1", &part.uvs1)] {
            if uvs.len() == count {
                let flat: Vec<f32> = uvs.iter().flatten().copied().collect();
                attributes.insert(key.into(), json!(self.glb.floats(&flat, 2, "VEC2", Some(ARRAY_BUFFER), false)));
            }
        }
        // The vertex colour as the game stores it, under an application name rather than
        // COLOR_0: most of the game's techniques read it as blend weights or a mask, not a tint,
        // and a COLOR_0 would tint the base colour in every importer.
        if part.colours.len() == count {
            let flat: Vec<f32> = part.colours.iter().flatten().copied().collect();
            attributes.insert("_COLOR0".into(), json!(self.glb.floats(&flat, 4, "VEC4", Some(ARRAY_BUFFER), false)));
        }
        if skinned && part.blend_indices.len() == count && part.blend_weights.len() == count {
            let slots = part.palette.len();
            let mut joints = Vec::with_capacity(count * 4);
            let mut weights = Vec::with_capacity(count * 4);
            let mut stray = false;
            for (index, weight) in part.blend_indices.iter().zip(&part.blend_weights) {
                let mut w = *weight;
                let mut j = [0u16; 4];
                for k in 0..4 {
                    let at = index[k] as usize;
                    if at < slots {
                        j[k] = at as u16;
                    } else {
                        stray |= w[k] > 0.0;
                        w[k] = 0.0;
                    }
                    w[k] = w[k].max(0.0);
                }
                let sum: f32 = w.iter().sum();
                let w = if sum > 0.0 { w.map(|v| v / sum) } else { [1.0, 0.0, 0.0, 0.0] };
                joints.extend_from_slice(&j);
                weights.extend_from_slice(&w);
            }
            if stray {
                self.summary.problems.push(format!("{name}: some vertices name bones past the palette; their weight was dropped"));
            }
            attributes.insert("JOINTS_0".into(), json!(self.glb.shorts(&joints, 4, "VEC4", Some(ARRAY_BUFFER))));
            attributes.insert("WEIGHTS_0".into(), json!(self.glb.floats(&weights, 4, "VEC4", Some(ARRAY_BUFFER), false)));
        }
        let indices = self.glb.shorts(&part.indices, 1, "SCALAR", Some(ELEMENT_ARRAY_BUFFER));
        self.summary.vertices += count;
        self.summary.triangles += part.indices.len() / 3;
        let mut primitive = json!({ "attributes": attributes, "indices": indices, "mode": 4 });
        if let Some(material) = part.material.and_then(|id| self.material(id)) {
            primitive["material"] = json!(material);
        }
        Some(primitive)
    }

    /// The material as glTF's metallic-roughness one: the colour texture and tint, the normal map,
    /// the blend. RotF's own technique and its parameters go in `extras` for a faithful port.
    fn material(&mut self, id: u32) -> Option<usize> {
        if let Some(&found) = self.materials.get(&id) {
            return Some(found);
        }
        let library = self.shared.library;
        let source = library.materials.get(&id)?;
        if !self.shared.materials.contains(&id) {
            self.shared.materials.push(id);
        }
        let colour = source.vector("diffuse_color").unwrap_or([1.0; 4]);
        let opacity = source.number("diffuse_texture1_opacity").unwrap_or(1.0).clamp(0.0, 1.0);
        // [assumed] RotF is a specular workflow; these are the numbers the game's own fallback
        // drawing uses for a technique it does not port (`game/src/assets.rs`), not the original's look.
        let mut pbr = json!({ "baseColorFactor": [colour[0], colour[1], colour[2], 1.0], "metallicFactor": 0.35, "roughnessFactor": 0.5 });
        if let Some(t) = source.texture("diffuse_texture1_texture").and_then(|t| self.texture(t)) {
            pbr["baseColorTexture"] = json!({ "index": t });
        }
        let blend = blend_of(source);
        let mut material = json!({
            "name": source.name,
            "pbrMetallicRoughness": pbr,
            "extras": {
                "rotf_technique": format!("{:08x}", source.technique),
                "rotf_blend": blend.map(|b| format!("{b:08x}")),
            },
        });
        if let Some(t) = source.texture("normal_map_texture").and_then(|t| self.texture(t)) {
            material["normalTexture"] = json!({ "index": t });
        }
        match blend {
            Some(BLEND_ADDITIVE) => {
                // glTF has no additive blend: blended and unlit is the nearest.
                material["alphaMode"] = json!("BLEND");
                material["pbrMetallicRoughness"]["baseColorFactor"][3] = json!(opacity);
                material["extensions"] = json!({ "KHR_materials_unlit": {} });
                if !self.glb.extensions_used.contains(&"KHR_materials_unlit") {
                    self.glb.extensions_used.push("KHR_materials_unlit");
                }
            }
            Some(BLEND_NORMAL) | Some(BLEND_BOOST) => {
                material["alphaMode"] = json!("BLEND");
                material["pbrMetallicRoughness"]["baseColorFactor"][3] = json!(opacity);
            }
            Some(BLEND_CUTOUT) => {
                material["alphaMode"] = json!("MASK");
                material["alphaCutoff"] = json!(source.word("alpharef").unwrap_or(1).max(1) as f32 / 255.0);
            }
            _ => {}
        }
        self.glb.materials.push(material);
        let index = self.glb.materials.len() - 1;
        self.materials.insert(id, index);
        Some(index)
    }

    /// The texture by id as a glTF texture over its PNG (`Shared::texture`), once per file.
    fn texture(&mut self, id: u32) -> Option<usize> {
        if let Some(&found) = self.textures.get(&id) {
            return found;
        }
        let made = self.shared.texture(id).filter(|file| file.png).map(|file| {
            self.glb.images.push(json!({ "name": file.stem, "uri": file.path() }));
            self.glb.textures.push(json!({ "source": self.glb.images.len() - 1, "sampler": 0 }));
            self.glb.textures.len() - 1
        });
        self.textures.insert(id, made);
        made
    }
}
