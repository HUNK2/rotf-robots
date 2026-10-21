//! The collision mask presets: what each named kind of body sends and takes.
//!
//! [data] `bnxglobal.str` data chunk 2, list `presets` (name hash, `sendMask`,
//! `receiveMask`); the game's loader is 00757c00. A primitive names a preset
//! (`maskPreset`) and only falls back on its own `sendMask` / `receiveMask` when the
//! preset is not found (`notes\melee.md`, the primitive loader at 005b95ab). The table
//! as text: `python work\preset_table.py`.
//!
//! Nothing here says how a QUERY's mask is matched against a body: that is the tree
//! walk 005b2630, written up in `notes\collision-world-query.md`.

use std::collections::HashMap;
use std::path::Path;

use crate::formats::lxb::DataFile;
use crate::formats::pack::{self, Pack};

/// The mask bits, named as the data object's flag type names them [data].
pub const PUSH: u16 = 0x1;
pub const PUNCH: u16 = 0x2;
pub const AVATAR_PUSH: u16 = 0x4;
pub const AI_PUSH: u16 = 0x8;
pub const GROUND: u16 = 0x10;
pub const CAMERA: u16 = 0x20;
pub const PLAYER_PUSH: u16 = 0x40;
pub const WATER: u16 = 0x80;
pub const DETECT: u16 = 0x100;
pub const SLIPPERY: u16 = 0x200;
pub const AWAKE_STATIC: u16 = 0x400;
pub const SPECIAL_BODY_PUSH: u16 = 0x1000;

/// One preset: what a body of this kind sends and what it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Masks {
    pub send: u16,
    pub receive: u16,
}

/// The preset table, by the CRC-32 of the preset's name.
#[derive(Clone, Debug, Default)]
pub struct Presets {
    by_name: HashMap<u32, Masks>,
}

impl Presets {
    /// Reads the `presets` list of a decoded data object (`bnxglobal.str` chunk 2).
    pub fn from_lxb(file: &DataFile) -> Result<Self, String> {
        let list = file.root().get("presets").ok_or("no presets in the data object")?;
        let mut by_name = HashMap::new();
        for p in list.items() {
            let name = p.get("name").and_then(|n| n.int()).ok_or("a preset has no name")? as u32;
            let mask = |field: &str| p.get(field).and_then(|n| n.int()).unwrap_or(0) as u16;
            by_name.insert(name, Masks { send: mask("sendMask"), receive: mask("receiveMask") });
        }
        Ok(Self { by_name })
    }

    /// Reads the table from the install's `bnxglobal.str`, data chunk 2.
    pub fn load(game_dir: &Path) -> Result<Self, String> {
        let pack = Pack::open(&game_dir.join("bnxglobal.str"))?;
        let chunk = pack
            .chunks
            .iter()
            .find(|c| c.index == 2 && c.tag == pack::DATA)
            .ok_or("bnxglobal.str has no data chunk 2")?;
        Self::from_lxb(&DataFile::parse(pack.data(chunk).to_vec())?)
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    pub fn get(&self, preset: u32) -> Option<Masks> {
        self.by_name.get(&preset).copied()
    }

    /// A primitive's masks: its preset's, or its own stored pair when the preset is not
    /// in the table [game: the primitive loader, `notes\melee.md`].
    pub fn resolve(&self, preset: u32, own_send: i64, own_receive: i64) -> Masks {
        self.get(preset).unwrap_or(Masks { send: own_send as u16, receive: own_receive as u16 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::hash::crc32;
    use std::path::PathBuf;

    fn install() -> Option<PathBuf> {
        let dir = PathBuf::from(crate::install::dir_string());
        dir.join("bnxglobal.str").is_file().then_some(dir)
    }

    #[test]
    fn presets_are_read() {
        // Needs the install. Values from `python work\preset_table.py`.
        let Some(dir) = install() else { return };
        let presets = Presets::load(&dir).expect("presets");
        assert_eq!(presets.len(), 63);
        let get = |name: &str| presets.get(crc32(name.as_bytes())).unwrap_or_else(|| panic!("no preset {name}"));
        assert_eq!(get("StaticGround"), Masks { send: PUSH | PUNCH | AVATAR_PUSH | GROUND | CAMERA | DETECT, receive: 0 });
        assert_eq!(get("StaticObj_SeeThroughFade"), Masks { send: PUSH | PUNCH | AVATAR_PUSH | DETECT, receive: CAMERA });
        assert_eq!(get("Character"), Masks { send: PUNCH | WATER, receive: 0 });
        assert_eq!(get("Attack"), Masks { send: WATER | AWAKE_STATIC, receive: DETECT });
        assert_eq!(get("NoPush_SeeThroughFade"), Masks { send: 0, receive: CAMERA });
        assert_eq!(get("StaticAIPushOnlyObj"), Masks { send: AI_PUSH, receive: 0 });
        // A name that is no preset falls back on the primitive's own pair.
        assert_eq!(presets.resolve(0, 37, 128), Masks { send: 37, receive: 128 });
    }
}
