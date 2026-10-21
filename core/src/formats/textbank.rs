//! The game's text banks: `text\<language>\global.lxb` (and a mission's own bank): a data object
//! whose root `text` is an array of `{ key, text }`, `key` being the CRC-32 of the string's key
//! name (`UI_MISSION_TIME`, a mission objective's `descStrId`). [data] (`tools\tf2_lxb.py dump`).
//! The game resolves a key in the mission's bank, then the global one (`text_key_lookup` 005a1760,
//! `notes\mission-hud.md`); `TextBank::with_fallback` does the same.

use std::collections::HashMap;
use std::path::Path;

use super::hash::crc32;
use super::lxb::DataFile;

#[derive(Clone, Debug, Default)]
pub struct TextBank {
    pub strings: HashMap<u32, String>,
}

impl TextBank {
    pub fn parse(file: &[u8]) -> Result<TextBank, String> {
        let data = DataFile::parse(file.to_vec())?;
        let root = data.root().get("text").ok_or("a text bank with no `text` root")?;
        let mut strings = HashMap::new();
        for item in root.items() {
            let Some(key) = item.get("key").and_then(|k| k.int()) else { continue };
            let Some(text) = item.get("text").and_then(|t| t.text()) else { continue };
            strings.insert(key as u32, text);
        }
        Ok(TextBank { strings })
    }

    pub fn load(path: &Path) -> Result<TextBank, String> {
        let file = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        TextBank::parse(&file)
    }

    /// The English global bank of an install. [assumed] the language folder is `eng` (the only one checked).
    pub fn global(game_dir: &Path) -> Result<TextBank, String> {
        TextBank::load(&game_dir.join("text").join("eng").join("global.lxb"))
    }

    /// A string by its key's hash (a `descStrId`, signed or not).
    pub fn by_hash(&self, key: u32) -> Option<&str> {
        self.strings.get(&key).map(String::as_str)
    }

    /// A string by its key's name (`UI_MISSION_TIME`).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.by_hash(crc32(key.as_bytes()))
    }

    /// This bank's strings over `fallback`'s: a mission bank over the global one.
    pub fn with_fallback(mut self, fallback: &TextBank) -> TextBank {
        for (k, v) in &fallback.strings {
            self.strings.entry(*k).or_insert_with(|| v.clone());
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_global_bank_has_the_completion_screens_labels() {
        // Needs the install.
        let Ok(bank) = TextBank::global(crate::install::dir()) else { return };
        assert!(bank.strings.len() > 1000, "{}", bank.strings.len());
        assert_eq!(bank.by_hash(1272520435), Some("This is a test string to make sure everything is working a-ok."));
        for key in ["UI_MISSION_TIME", "UI_TITLE_MISSION_COMPLETE", "UI_GOLD_MEDAL", "UI_OVERALL_ACCURACY", "UI_ENERGON_POINTS"] {
            assert!(bank.get(key).is_some_and(|t| !t.is_empty()), "{key}");
        }
    }
}
