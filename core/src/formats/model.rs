//! Model blobs: an `APKF` blob with `MAT`, `MESH` and sometimes `SKEL` entries.
//!
//! A mesh is a list of parts, one per material. Each part has its own vertex buffer, 16-bit
//! index buffer and Direct3D 9 vertex declaration. Skinned parts also carry a palette that maps
//! the bone numbers in their vertices to bones of the skeleton.

use std::collections::HashMap;

use super::Bytes;
use super::apk::{Blob, Entry};
use super::pack::{self, Pack};
use super::texture::{self, Texture};

const TRIANGLE_LIST: u32 = 4;

const USAGE_POSITION: u8 = 0;
const USAGE_BLEND_WEIGHT: u8 = 1;
const USAGE_BLEND_INDICES: u8 = 2;
const USAGE_NORMAL: u8 = 3;
const USAGE_TEXCOORD: u8 = 5;
const USAGE_TANGENT: u8 = 6;
const USAGE_BINORMAL: u8 = 7;
const USAGE_COLOR: u8 = 10;

const PARAM_TEXTURE: u32 = 3;
const PARAM_FLOAT: u32 = 2;
const PARAM_VECTOR: u32 = 4;

#[derive(Default)]
pub struct Part {
    /// Asset id of the material.
    pub material: Option<u32>,
    /// Skeleton bone for each bone number used by this part's vertices; empty for rigid parts.
    pub palette: Vec<u16>,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Texture coordinate sets 1 and 2 (TEXCOORD usage index 1 and 2), empty when the vertex
    /// declaration has none. Only the blend2 techniques read them (`surface.wgsl`, LS-24).
    pub uvs1: Vec<[f32; 2]>,
    pub uvs2: Vec<[f32; 2]>,
    /// TEXCOORD3: the independent RGB layer-mask UV of blend3_mask.
    pub uvs3: Vec<[f32; 2]>,
    /// Flatcard TEXCOORD1 carries a 3D anchor; TEXCOORD2 carries its encoded normal.
    pub uv1_xyz: Vec<[f32; 3]>,
    pub uv2_xyz: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 3]>,
    pub binormals: Vec<[f32; 3]>,
    /// COLOR0 from the vertex declaration, stored as D3DCOLOR (BGRA bytes) in the zone packs.
    pub colours: Vec<[f32; 4]>,
    pub blend_indices: Vec<[f32; 4]>,
    pub blend_weights: Vec<[f32; 4]>,
    pub indices: Vec<u16>,
}

pub struct Mesh {
    pub name: String,
    /// Centre and radius of the bounding sphere.
    pub sphere: [f32; 4],
    /// Half-extents at MESH+30; +20.xyz is also the local box centre (0058a9c0).
    pub half_extents: [f32; 3],
    pub parts: Vec<Part>,
    /// Asset id of the skeleton, for a skinned mesh.
    pub skeleton: Option<u32>,
    /// Lower-detail meshes and the distance each takes over at.
    pub lods: Vec<(u32, f32)>,
}

pub enum ParamValue {
    Number(f32),
    /// An integer, or the name hash of an enum value.
    Word(u32),
    Vector([f32; 4]),
    /// Asset id of a texture.
    Texture(Option<u32>),
}

pub struct Param {
    pub name: u32,
    /// For a sampler setting, the name of the texture parameter it belongs to.
    pub owner: u32,
    pub value: ParamValue,
}

pub struct Material {
    pub name: String,
    pub technique: u32,
    pub params: Vec<Param>,
}

pub struct Bone {
    /// Lower-case multiply-by-33 hash of the bone's name.
    pub name: u32,
    /// Model space to bone space at rest: four rows of x, y, z (row-vector convention).
    pub inverse_bind: [[f32; 3]; 4],
}

/// Everything a pack's model and texture chunks define, keyed by asset id.
#[derive(Default)]
pub struct Library {
    pub meshes: HashMap<u32, Mesh>,
    pub materials: HashMap<u32, Material>,
    pub skeletons: HashMap<u32, Vec<Bone>>,
    pub textures: HashMap<u32, Texture>,
    pub texture_frames: HashMap<u32, texture::FrameList>,
    /// Chunks that could not be read, with the reason.
    pub problems: Vec<String>,
}

impl Material {
    fn find(&self, name: &str) -> Option<&ParamValue> {
        let hash = super::hash::crc32(name.as_bytes());
        self.params.iter().find(|p| p.name == hash && p.owner == 0).map(|p| &p.value)
    }

    pub fn texture(&self, name: &str) -> Option<u32> {
        match self.find(name)? {
            ParamValue::Texture(id) => *id,
            _ => None,
        }
    }

    pub fn vector(&self, name: &str) -> Option<[f32; 4]> {
        match self.find(name)? {
            ParamValue::Vector(v) => Some(*v),
            _ => None,
        }
    }

    pub fn number(&self, name: &str) -> Option<f32> {
        match self.find(name)? {
            ParamValue::Number(v) => Some(*v),
            _ => None,
        }
    }

    pub fn word(&self, name: &str) -> Option<u32> {
        match self.find(name)? {
            ParamValue::Word(v) => Some(*v),
            _ => None,
        }
    }
}

impl Library {
    pub fn load(pack: &Pack) -> Self {
        let mut library = Self::default();
        for chunk in pack.of_type(pack::TEXTURE) {
            match texture::parse(pack.data(chunk)) {
                Ok(Some(texture)) => {
                    library.textures.insert(texture.id, texture);
                }
                Ok(None) => match texture::parse_frames(pack.data(chunk)) {
                    Ok(Some(frames)) => { library.texture_frames.insert(frames.id, frames); }
                    Ok(None) => {}
                    Err(e) => library.problems.push(format!("IFL chunk {}: {e}", chunk.index)),
                },
                Err(e) => library.problems.push(format!("texture chunk {}: {e}", chunk.index)),
            }
        }
        for chunk in pack.of_type(pack::MODEL) {
            if let Err(e) = library.add_model(pack.data(chunk)) {
                library.problems.push(format!("model chunk {}: {e}", chunk.index));
            }
        }
        library
    }

    /// Takes in another pack's library (a mission's pack over its zone's). An id both have
    /// keeps the later one. The game keeps both: its model accept 00758520 (handler 67876b72,
    /// registered by 00758b50) pushes each entry at the FRONT of the level-set ring (+0x18) with
    /// no id test, so the later pack's entry is first in the ring [game, wave 27 O9].
    /// [assumed: the lookup by id returns the first match, so the later pack wins, as here]
    /// settles: the lookup that walks the level-set +0x18 ring by id (not read).
    pub fn merge(&mut self, other: Library) {
        self.meshes.extend(other.meshes);
        self.materials.extend(other.materials);
        self.skeletons.extend(other.skeletons);
        self.textures.extend(other.textures);
        self.texture_frames.extend(other.texture_frames);
        self.problems.extend(other.problems);
    }

    fn add_model(&mut self, data: &[u8]) -> Result<(), String> {
        let blob = Blob::parse(data)?;
        for entry in &blob.entries {
            let result = match &entry.tag {
                b"MESH" => read_mesh(&blob, entry).map(|mesh| drop(self.meshes.insert(entry.id, mesh))),
                b"MAT\0" => read_material(&blob, entry).map(|mat| drop(self.materials.insert(entry.id, mat))),
                b"SKEL" => read_skeleton(&blob, entry).map(|bones| drop(self.skeletons.insert(entry.id, bones))),
                _ => Ok(()),
            };
            result.map_err(|e| format!("{}: {e}", entry.name))?;
        }
        Ok(())
    }
}

fn id_at(blob: &Blob, pointer_field: usize) -> Result<Option<u32>, String> {
    let Some(target) = blob.image_pointer(pointer_field)? else { return Ok(None) };
    blob.entry_at(target).map(|e| Some(e.id)).ok_or_else(|| format!("pointer at {pointer_field:#x} is not to an entry"))
}

fn floats<const N: usize>(bytes: &[u8], at: usize) -> Result<[f32; N], String> {
    let mut out = [0.0; N];
    for (i, v) in out.iter_mut().enumerate() {
        *v = bytes.f32_at(at + i * 4)?;
    }
    Ok(out)
}

fn read_mesh(blob: &Blob, entry: &Entry) -> Result<Mesh, String> {
    let image = blob.image;
    let base = entry.image_offset;
    let part_count = image.u32_at(base + 0x0c)? as usize;
    let lod_count = image.u32_at(base + 0x18)? as usize;

    let mut parts = Vec::with_capacity(part_count);
    if let Some(table) = blob.image_pointer(base + 0x10)? {
        for i in 0..part_count {
            let part = blob.image_pointer(table + i * 8 + 4)?.ok_or("part table entry without a part")?;
            parts.push(read_part(blob, part)?);
        }
    }
    let mut lods = Vec::with_capacity(lod_count);
    if let Some(table) = blob.image_pointer(base + 0x1c)? {
        for i in 0..lod_count {
            let mesh = id_at(blob, table + i * 8)?.ok_or("level-of-detail entry without a mesh")?;
            lods.push((mesh, image.f32_at(table + i * 8 + 4)?));
        }
    }
    Ok(Mesh {
        name: entry.name.clone(),
        sphere: floats(image, base + 0x20)?,
        half_extents: floats(image, base + 0x30)?,
        parts,
        skeleton: id_at(blob, base + 0x14)?,
        lods,
    })
}

fn read_part(blob: &Blob, at: usize) -> Result<Part, String> {
    let image = blob.image;
    let mut part = Part { material: id_at(blob, at + 0x30)?, ..Default::default() };
    if let Some(palette) = blob.image_pointer(at + 0x34)? {
        for i in 0..image.u32_at(at + 0x38)? as usize {
            part.palette.push(image.u16_at(palette + i * 2)?);
        }
    }
    let vertex_count = image.u32_at(at + 0x44)? as usize;
    let index_count = image.u32_at(at + 0x54)? as usize;
    if image.u32_at(at + 0x58)? != 2 {
        return Err("indices are not 16-bit".into());
    }
    if image.u32_at(at + 0x60)? != TRIANGLE_LIST {
        return Err(format!("primitive type {} is not a triangle list", image.u32_at(at + 0x60)?));
    }
    let layout = blob.image_pointer(at + 0x5c)?.ok_or("part without a vertex layout")?;
    let stride = image.u32_at(layout)? as usize;
    let declaration = blob.image_pointer(layout + 4)?.ok_or("part without a vertex declaration")?;

    let vertices = blob.bulk_pointer(at + 0x3c)?;
    let vertices = blob.bulk.get(vertices..vertices + vertex_count * stride).ok_or("vertex buffer runs past the end")?;
    let indices = blob.bulk_pointer(at + 0x4c)?;
    let indices = blob.bulk.get(indices..indices + index_count * 2).ok_or("index buffer runs past the end")?;
    part.indices = indices.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
    if part.indices.iter().any(|&i| i as usize >= vertex_count) {
        return Err("an index points past the vertex buffer".into());
    }

    // One element per 8 bytes: stream, offset, type, method, usage, usage index.
    for element in 0.. {
        let e = declaration + element * 8;
        if image.u16_at(e)? == 0xff {
            break;
        }
        let offset = image.u16_at(e + 2)? as usize;
        let (kind, usage, usage_index) = (image[e + 4], image[e + 6], image[e + 7]);
        if usage == USAGE_COLOR && usage_index == 0 {
            let width = match kind {
                3 => 16, // FLOAT4
                4 => 4,  // D3DCOLOR, normalized BGRA
                _ => return Err(format!("vertex colour element type {kind} is not handled")),
            };
            if offset + width > stride || stride == 0 {
                return Err("vertex colour runs past the stride".into());
            }
            part.colours = vertices.chunks_exact(stride)
                .map(|v| read_colour(&v[offset..offset + width], kind)).collect();
            continue;
        }
        // Texture coordinate sets 1 and 2 are read too (LS-24), but only when their element
        // is one this reader handles: a set it cannot read is skipped, not an error, so no
        // other mesh's load changes.
        let extra_uv = usage == USAGE_TEXCOORD && matches!(usage_index, 1 | 2 | 3);
        let wanted = (usage_index == 0 || extra_uv)
            && matches!(
                usage,
                USAGE_POSITION
                    | USAGE_NORMAL
                    | USAGE_TANGENT
                    | USAGE_BINORMAL
                    | USAGE_TEXCOORD
                    | USAGE_BLEND_INDICES
                    | USAGE_BLEND_WEIGHT
            );
        if !wanted {
            continue;
        }
        // Types 0..=3 are one to four floats; the elements read here are always stored that way.
        let width = match kind {
            0..=3 => kind as usize + 1,
            _ if extra_uv => continue,
            _ => return Err(format!("vertex element type {kind} for usage {usage} is not handled")),
        };
        if offset + width * 4 > stride {
            if extra_uv {
                continue;
            }
            return Err("vertex element runs past the stride".into());
        }
        let read = |vertex: &[u8]| -> [f32; 4] {
            let mut out = [0.0; 4];
            for (i, v) in out.iter_mut().take(width).enumerate() {
                let b = &vertex[offset + i * 4..offset + i * 4 + 4];
                *v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
            out
        };
        let three = || vertices.chunks_exact(stride).map(|v| read(v)).map(|v| [v[0], v[1], v[2]]).collect();
        let four = || vertices.chunks_exact(stride).map(|v| read(v)).collect();
        match (usage, usage_index) {
            (USAGE_POSITION, 0) => part.positions = three(),
            (USAGE_NORMAL, 0) => part.normals = three(),
            (USAGE_TANGENT, 0) => part.tangents = three(),
            (USAGE_BINORMAL, 0) => part.binormals = three(),
            (USAGE_TEXCOORD, 0) => {
                part.uvs = vertices.chunks_exact(stride).map(|v| read(v)).map(|v| [v[0], v[1]]).collect();
            }
            (USAGE_TEXCOORD, 1) => {
                part.uvs1 = vertices.chunks_exact(stride).map(|v| read(v)).map(|v| [v[0], v[1]]).collect();
                if width >= 3 { part.uv1_xyz = three(); }
            }
            (USAGE_TEXCOORD, 2) => {
                part.uvs2 = vertices.chunks_exact(stride).map(|v| read(v)).map(|v| [v[0], v[1]]).collect();
                if width >= 3 { part.uv2_xyz = three(); }
            }
            (USAGE_TEXCOORD, 3) => {
                part.uvs3 = vertices.chunks_exact(stride).map(|v| read(v)).map(|v| [v[0], v[1]]).collect();
            }
            (USAGE_BLEND_INDICES, 0) => part.blend_indices = four(),
            (USAGE_BLEND_WEIGHT, 0) => part.blend_weights = four(),
            _ => {}
        }
    }
    if part.positions.len() != vertex_count {
        return Err("part has no positions".into());
    }
    Ok(part)
}

fn read_colour(bytes: &[u8], kind: u8) -> [f32; 4] {
    if kind == 4 {
        // [data] D3DCOLOR declarations in deep_six_01, shanghai_01 and us_test. Channel order
        // follows the declaration's D3D9 ARGB word, little-endian BGRA bytes.
        [bytes[2], bytes[1], bytes[0], bytes[3]].map(|v| v as f32 / 255.0)
    } else {
        std::array::from_fn(|i| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
    }
}

fn read_material(blob: &Blob, entry: &Entry) -> Result<Material, String> {
    let image = blob.image;
    let base = entry.image_offset;
    let count = image.u32_at(base + 0x20)? as usize;
    let mut params = Vec::with_capacity(count);
    if let Some(table) = blob.image_pointer(base + 0x24)? {
        for i in 0..count {
            let at = table + i * 16;
            let value = match image.u32_at(at + 12)? & 0xffff {
                PARAM_FLOAT => ParamValue::Number(image.f32_at(at)?),
                PARAM_TEXTURE => ParamValue::Texture(blob.reference(at)),
                PARAM_VECTOR => match blob.image_pointer(at)? {
                    Some(target) => ParamValue::Vector(floats(image, target)?),
                    None => ParamValue::Vector([0.0; 4]),
                },
                _ => ParamValue::Word(image.u32_at(at)?),
            };
            params.push(Param { name: image.u32_at(at + 4)?, owner: image.u32_at(at + 8)?, value });
        }
    }
    Ok(Material { name: entry.name.clone(), technique: image.u32_at(base + 0x14)?, params })
}

fn read_skeleton(blob: &Blob, entry: &Entry) -> Result<Vec<Bone>, String> {
    let image = blob.image;
    let base = entry.image_offset;
    let count = image.u32_at(base + 0x08)? as usize;
    let Some(first) = blob.image_pointer(base + 0x0c)? else { return Ok(Vec::new()) };
    let mut bones = Vec::with_capacity(count);
    for i in 0..count {
        // A bone is 0x90 bytes: a local matrix, the inverse bind matrix, then the name.
        let at = first + i * 0x90;
        let mut inverse_bind = [[0.0; 3]; 4];
        for (row, out) in inverse_bind.iter_mut().enumerate() {
            // The fourth column of each row is not part of the matrix.
            *out = floats(image, at + 0x40 + row * 16)?;
        }
        bones.push(Bone { name: image.u32_at(at + 0x84)?, inverse_bind });
    }
    Ok(bones)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn us_test_ifl_lists_resolve_every_frame_and_keep_repeats() {
        let dir = std::path::PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into()));
        let zone = dir.join("levels/us_test/zone.str");
        if !zone.is_file() { return; }
        let library = Library::load(&Pack::open(&zone).unwrap());
        assert!(library.problems.is_empty(), "{:?}", library.problems);
        assert_eq!(library.texture_frames.len(), 6);
        for sequence in library.texture_frames.values() {
            assert!(!sequence.frames.is_empty());
            for id in &sequence.frames { assert!(library.textures.contains_key(id), "{} frame {id:08x}", sequence.name); }
        }
        let barrier = &library.texture_frames[&0x05d196de];
        assert_eq!(barrier.frames.len(), 8);
        assert_eq!(barrier.frames[3], barrier.frames[5]);
        assert_eq!(barrier.frames[5], barrier.frames[7]);
    }

    #[test]
    fn us_test_three_layer_materials_retain_authored_uv_streams() {
        let dir = std::path::PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into()));
        let zone = dir.join("levels/us_test/zone.str");
        if !zone.is_file() { return; }
        let library = Library::load(&Pack::open(&zone).unwrap());
        let mut count = 0;
        let mut missing_mask_uv = 0;
        for part in library.meshes.values().flat_map(|m| &m.parts) {
            if !part.material.and_then(|id| library.materials.get(&id)).is_some_and(|m| m.technique == super::super::hash::crc32(b"blend3_mask_ssd")) { continue; }
            count += 1;
            for uv in [&part.uvs, &part.uvs1, &part.uvs2] { assert_eq!(uv.len(), part.positions.len()); }
            if part.uvs3.is_empty() { missing_mask_uv += 1; }
            else { assert_eq!(part.uvs3.len(), part.positions.len()); }
        }
        assert_eq!(count, 56);
        // [data] Six planter parts omit TEXCOORD3 in the original declaration.
        // Preserve that fact here instead of manufacturing an authored UV stream.
        assert_eq!(missing_mask_uv, 6);
    }

    #[test]
    fn us_test_flatcards_retain_3d_anchors_and_colour() {
        let dir = std::path::PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into()));
        let zone = dir.join("levels/us_test/zone.str");
        if !zone.is_file() { return; }
        let library = Library::load(&Pack::open(&zone).unwrap());
        let hash = super::super::hash::crc32;
        let mut count = 0;
        for part in library.meshes.values().flat_map(|m| &m.parts) {
            let Some(material) = part.material.and_then(|id| library.materials.get(&id)) else { continue };
            if ![hash(b"flatcard_plus"), hash(b"flatcard_cheap"), hash(b"flatcard_emissive")].contains(&material.technique) { continue; }
            count += 1;
            assert_eq!(part.uv1_xyz.len(), part.positions.len(), "{} anchor", material.name);
            assert_eq!(part.colours.len(), part.positions.len(), "{} colour", material.name);
            if material.technique != hash(b"flatcard_emissive") {
                assert_eq!(part.uv2_xyz.len(), part.positions.len(), "{} normal", material.name);
            }
        }
        assert_eq!(count, 109);
    }

    #[test]
    fn packed_colour_preserves_bgra_channels_and_alpha() {
        // First sky.cylinder01 vertex in deep_six_01, surveyed from its bulk buffer.
        assert_eq!(read_colour(&[0x5e, 0x58, 0x51, 0xff], 4),
            [81.0 / 255.0, 88.0 / 255.0, 94.0 / 255.0, 1.0]);
        assert_eq!(read_colour(&[0, 0, 255, 128], 4), [1.0, 0.0, 0.0, 128.0 / 255.0]);
    }

    #[test]
    fn deep_six_sky_retains_colour_for_every_vertex() {
        let dir = std::path::PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| r"C:\Games2".into()));
        let zone = dir.join(r"levels\deep_six_01\zone.str");
        if !zone.exists() { return; }
        let pack = Pack::open(&zone).unwrap();
        let library = Library::load(&pack);
        let sky = library.meshes.values().find(|m| m.name == "sky.cylinder01").unwrap();
        assert!(!sky.parts.is_empty());
        for part in &sky.parts {
            assert_eq!(part.colours.len(), part.positions.len());
            assert!(!part.colours.is_empty());
            assert_eq!(part.colours[0], [81.0 / 255.0, 88.0 / 255.0, 94.0 / 255.0, 1.0]);
        }
    }

    /// Needs the game install (`TF2_GAME_DIR`, default `C:\Games2`); skipped without it.
    /// `roads.groundpatchd_01` in Shanghai's zone has blend2 parts (`notes\level-shaders-bgnd-blend2.md`):
    /// each reads its texture coordinate sets 0, 1 and 2, all of the vertex count.
    #[test]
    fn blend2_parts_read_three_uv_sets_of_the_vertex_count() {
        let dir = std::path::PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| r"C:\Games2".into()));
        let zone = dir.join(r"levels\shanghai_01\zone.str");
        if !zone.exists() {
            return;
        }
        let pack = Pack::open(&zone).unwrap();
        let library = Library::load(&pack);
        let blend2 = crate::formats::hash::crc32(b"blend2_plus_mask_ssd");
        let mesh = library.meshes.values().find(|m| m.name == "roads.groundpatchd_01").expect("the mesh is in the zone");
        let parts: Vec<&Part> = mesh
            .parts
            .iter()
            .filter(|p| p.material.and_then(|id| library.materials.get(&id)).is_some_and(|m| m.technique == blend2))
            .collect();
        assert!(!parts.is_empty(), "the mesh has blend2 parts");
        for part in parts {
            let count = part.positions.len();
            assert!(count > 0);
            assert_eq!((part.uvs.len(), part.uvs1.len(), part.uvs2.len()), (count, count, count));
        }
    }
}
