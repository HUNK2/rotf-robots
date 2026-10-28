//! Names for the hashes the game's data keeps in place of names (the CRC-32 of the name): from the
//! data's own name tables, the sound events, the shader techniques and the names the export already
//! holds (nodes, sets, clips). A hash nothing names is written `#xxxxxxxx`.

use std::collections::HashMap;
use std::path::Path;

use tf2_core::formats::fev::Fev;
use tf2_core::formats::hash::crc32;
use tf2_core::formats::lxb::DataFile;
use tf2_core::formats::pack::{self, Pack};

/// The material parameters the port's code reads by name (`game/src/assets.rs`, `surface.rs`,
/// `fx.rs`): the names the JSON can give a material's parameters. The others are written by hash.
pub const MATERIAL_PARAMETERS: [&str; 56] = [
    "alphaboost",
    "alpharef",
    "ambient_boost",
    "blendmode",
    "bloom_texture",
    "cullmode",
    "depthbias",
    "diffuse_color",
    "diffuse_mix_texture",
    "diffuse_texture1_opacity",
    "diffuse_texture1_t",
    "diffuse_texture1_texture",
    "diffuse_texture1_x",
    "diffuse_texture1_y",
    "diffuse_texture1_z",
    "diffuse_texture2_opacity",
    "diffuse_texture2_texture",
    "diffuse_texture3_texture",
    "distortion_distance",
    "edge_color",
    "edge_power",
    "fresnel_texture",
    "intransfalloff",
    "motion_distort",
    "motion_speed",
    "motion_strength",
    "normal_map_dxt5_nm",
    "normal_map_heightscale",
    "normal_map_t",
    "normal_map_texture",
    "normal_map_texture0",
    "normal_map_texture1",
    "normal_map_texture2",
    "normal_map_texture3",
    "normal_map_x",
    "normal_map_y",
    "normal_map_z",
    "normal_mapb_texture",
    "outtransfalloff",
    "paint_color0",
    "paint_color1",
    "paint_color2",
    "paint_pow1",
    "paint_pow2",
    "reflection_color",
    "reflection_falloff",
    "reflection_falloffpower",
    "reflectionmap_texture",
    "specular_color",
    "specular_map_t",
    "specular_map_texture",
    "specular_map_x",
    "specular_map_y",
    "specular_map_z",
    "specular_power",
    "zfeather_distance",
];

pub struct Names {
    known: HashMap<u32, String>,
    files: Vec<DataFile>,
}

impl Names {
    /// The data files' name tables, and the names of the techniques, the sound events and the
    /// material parameters. A file that cannot be read gives no names and no error: a name the
    /// export cannot find is written by its hash.
    pub fn load(install: &Path, files: Vec<DataFile>, sound_events: &[std::path::PathBuf]) -> Self {
        let mut names = Self { known: HashMap::new(), files };
        for name in MATERIAL_PARAMETERS.iter().copied().chain(["Normal", "Additive"]) {
            names.add(name);
        }
        for archive in ["shaders.lsa", "effects.lsa"] {
            for technique in techniques(&install.join(archive)) {
                names.add(&technique);
            }
        }
        // The shaders' constant tables spell out most material parameters (`specular_power`,
        // `reflection_color`); the executable a few enum values the data stores by hash (a
        // footstep's `LEFT` and `RIGHT`). Every run of name characters is taken: a hash nothing
        // asks for is never looked at.
        for file in ["shaders.lsa", "effects.lsa", "Transformers2.exe"] {
            if let Ok(data) = std::fs::read(install.join(file)) {
                for word in data.split(|&b| !(b.is_ascii_alphanumeric() || b == b'_' || b == b'.')).filter(|w| (3..=64).contains(&w.len())) {
                    let word: String = word.iter().map(|&b| b as char).collect();
                    names.add(&word);
                }
            }
        }
        for path in sound_events {
            let Some(fev) = std::fs::read(path).ok().and_then(|data| Fev::parse(&data).ok()) else { continue };
            for event in &fev.events {
                names.known.insert(event.id, event.name.clone());
            }
        }
        names
    }

    pub fn add(&mut self, name: &str) {
        self.known.entry(crc32(name.as_bytes())).or_insert_with(|| name.to_owned());
    }

    pub fn find(&self, hash: u32) -> Option<String> {
        if let Some(name) = self.known.get(&hash) {
            return Some(name.clone());
        }
        self.files.iter().find_map(|file| file.name(hash)).map(str::to_owned)
    }

    /// The name, or `#xxxxxxxx` when nothing names the hash.
    pub fn name(&self, hash: u32) -> String {
        self.find(hash).unwrap_or_else(|| format!("#{hash:08x}"))
    }

    /// The data files the names came from, for reading objects by root name.
    pub fn files(&self) -> &[DataFile] {
        &self.files
    }
}

/// Every data chunk of a pack, those that parse.
pub fn data_files(path: &Path) -> Vec<DataFile> {
    let Ok(pack) = Pack::open(path) else { return Vec::new() };
    pack.of_type(pack::DATA).filter_map(|chunk| DataFile::parse(pack.data(chunk).to_vec()).ok()).collect()
}

/// The technique names of a shader archive (`.lsa`): a header of six words (magic, version, shader
/// count, shader table, entry count, entry table), entries of 0xa0 bytes with a 64-byte name at
/// +0x20. [data] (the layout `work\lsa_list.py` reads in the dev tree)
fn techniques(path: &Path) -> Vec<String> {
    let Ok(data) = std::fs::read(path) else { return Vec::new() };
    let word = |at: usize| data.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let (Some(count), Some(table)) = (word(16), word(20)) else { return Vec::new() };
    (0..count.min(4096))
        .filter_map(|i| {
            let name = data.get(table + i * 0xa0 + 0x20..table + i * 0xa0 + 0x60)?;
            let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
            (!name.is_empty()).then(|| name.iter().map(|&b| b as char).collect())
        })
        .collect()
}
