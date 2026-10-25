//! The game's own list of playable characters: `characters` in `bnxglobal.str` data chunk 5
//! (the unpublished notes section 1). 24 entries in the install. An entry joins a
//! display name to the pack under `characters\`, to the `characterAttributes<X>` object of
//! chunk 16 (by the CRC-32 of that object's name) and to the pack whose sound bank it uses.
//!
//! Read from the data object's own field names (`[data]`). The menu ratings, string ids and
//! unlock ids are not read yet: nothing uses them before the front end exists.
//!
//! Not in this list `[data]`: `dcomscout`, `dcomsniper`, `dcomwarrior`, `adrian`,
//! `bumblebeeinvert`. Those are reached by pack name (`Spec::by_pack`).

use std::path::{Path, PathBuf};

use super::hash::crc32;
use super::lxb::{DataFile, Node};
use super::pack::{self, Pack};

/// One entry of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The display name, in capitals (`BUMBLEBEE`, `POWERED-UP OPTIMUS`).
    pub name: String,
    /// The entry's id. A spawn point's `attributes` is compared with `attributes` (below), not
    /// with this [data: survey-levels]. The `usableCharactersIds` of global_characters.txt are list
    /// ids [data]: `1295997219` IRONHIDE, `1170348905` OPTIMUS PRIME, `-1517799451` BUMBLEBEE; `0` is
    /// empty; `-390335762` names no entry in the list (OPEN: settles: the game's usable-id compare, or
    /// what that id is, not read).
    pub id: u32,
    /// CRC-32 of the name of its `characterAttributes<X>` object, and of the multiplayer one.
    pub attributes: u32,
    pub attributes_mp: u32,
    /// 0 human, 1 Autobot, 2 Decepticon. [data] global_characters.txt: one 0, twelve 1 and
    /// eleven 2, which is the names' split (one human, 12 Autobot entries, 11 Decepticon).
    pub faction: u32,
    /// The pack, as stored: `characters/bumblebee` (no extension; mixed case in places).
    pub file_path: String,
    /// The pack whose sound bank it uses when that is not its own (the paint-overs and
    /// Gold Megatron name their base character); empty otherwise.
    pub audio_file: String,
    /// Hashes that resolve to `SA_<NAME>`, `W1_<NAME>`, `W2_<NAME>` (the names the HUD shows).
    pub ability_name: u32,
    pub weapon_names: [u32; 2],
}

/// What it takes to start one character: the pack, the suffix of its attributes object and
/// the pack its sound bank is named after.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    /// The pack's file name under `characters\`, lower case, no extension.
    pub pack: String,
    /// `X` of `characterAttributes<X>`.
    pub attributes: String,
    /// The pack whose `characters\eng\<name>.fev` holds its sound events.
    pub audio: String,
    /// The list's display name, when the character is in the list.
    pub display: Option<String>,
}

impl Spec {
    /// A character that is not in the list (the enemies), or one named by hand. Without
    /// `attributes`, the object is guessed as the pack name with a capital. [data, checked 2026-10-20
    /// against bnxglobal's chunk 16 (61 `characterAttributes*` names)]: that guess is right for
    /// Bumblebee, Optimus, OptimusFlight, Ironhide, Ratchet, Starscream, Megatron, Jetfire, Blackout
    /// and Firestorm, but NOT for `dcomscout` (`DComScout`), `longhaul` (`LongHaul`), `megatronflight`
    /// (`MegatronFlight`) or `audir8` (`Audi`, no name match at all). Every roster pack that is an
    /// entry takes its object from the entry's hash (`CharacterList::spec`), so the guess only reaches
    /// packs that are not entries; `CharacterList::spec_by_pack` looks the name up in the data first.
    /// [assumed] `adrian`, `bumblebeeinvert` and `soldiermale` have no such object, so the guess stands
    /// for them (not a roster pack; OPEN: settles: which attributes object, if any, the game gives them,
    /// from the spawn point or the state that starts them).
    pub fn by_pack(pack: &str, attributes: Option<&str>) -> Self {
        let pack = pack.to_ascii_lowercase();
        let attributes = attributes.map_or_else(|| pack[..1].to_ascii_uppercase() + &pack[1..], str::to_owned);
        Self { audio: pack.clone(), pack, attributes, display: None }
    }

    pub fn pack_file(&self, game_dir: &Path) -> PathBuf {
        pack_file(game_dir, &self.pack)
    }

    /// The character's sound events.
    pub fn sound_events(&self, game_dir: &Path) -> PathBuf {
        match elsewhere(&self.audio) {
            Some(e) => game_dir.join(e.sounds),
            None => game_dir.join("characters").join("eng").join(format!("{}.fev", self.audio)),
        }
    }
}

/// A character the install keeps outside `characters\`: a boss whose Character exists only in his
/// mission's pack, as the embedded scene (`data`) of the CharacterDefinition a CharacterSpawnManager
/// spawns. The scene has the shape of a character pack's data object (a root whose `things` hold the
/// robot Object and the Character), so it is read the same way. [data]
pub struct Elsewhere {
    /// The name `--character` and `Spec::pack` give him.
    pub pack: &'static str,
    /// The pack that holds his data, models and textures, under the install.
    pub file: &'static str,
    /// The embedded scene's name (its object label is the CRC-32 of it).
    pub scene: &'static str,
    /// The sound events his clips and Character name.
    pub sounds: &'static str,
}

/// Devastator: `CharDef_Devastator` under `Devastator_CranialCircuits` in the Cairo City boss
/// mission's data chunk 1; his model blob (572b354e) and animation set (`devastator_animset`, 14
/// clips) are in the same pack, his sounds in the mission's events. Not in the game's list and in
/// no `characters\` pack. [data 2026-10-22, the unpublished notes "Devastator"]
pub const ELSEWHERE: &[Elsewhere] = &[Elsewhere {
    pack: "devastator",
    file: r"levels\cairo_city\missions\cairo_city_a_boss\mission.str",
    scene: "devastator_basepose",
    sounds: r"levels\cairo_city\missions\cairo_city_a_boss\eng\mission.fev",
}];

pub fn elsewhere(pack: &str) -> Option<&'static Elsewhere> {
    ELSEWHERE.iter().find(|e| e.pack.eq_ignore_ascii_case(pack))
}

/// The pack that holds a character: `characters\<pack>.str`, or the mission pack of one kept elsewhere.
pub fn pack_file(game_dir: &Path, pack: &str) -> PathBuf {
    match elsewhere(pack) {
        Some(e) => game_dir.join(e.file),
        None => game_dir.join("characters").join(format!("{pack}.str")),
    }
}

#[derive(Clone, Debug, Default)]
pub struct CharacterList {
    pub entries: Vec<Entry>,
    /// The names of every `characterAttributes*` root of the install, to turn an entry's
    /// hash back into the name `tuning::load` takes.
    attribute_names: Vec<String>,
}

fn int(node: Option<Node>) -> u32 {
    node.and_then(Node::int).unwrap_or(0) as u32
}

fn read(c: Node) -> Entry {
    Entry {
        name: c.get("name").and_then(Node::text).unwrap_or_default(),
        id: int(c.get("id")),
        attributes: int(c.get("characterAttributes")),
        attributes_mp: int(c.get("characterAttributesMP")),
        faction: int(c.get("faction")),
        file_path: c.get("filePath").and_then(Node::text).unwrap_or_default(),
        audio_file: c.get("audioFile").and_then(Node::text).unwrap_or_default(),
        ability_name: int(c.get("abilityName")),
        weapon_names: [int(c.get("weapon1Name")), int(c.get("weapon2Name"))],
    }
}

/// `characters/megatronGold` to `megatrongold`.
fn pack_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_ascii_lowercase()
}

impl CharacterList {
    /// Reads the list and the attribute names from the install's `bnxglobal.str`.
    pub fn load(game_dir: &Path) -> Result<Self, String> {
        let pack = Pack::open(&game_dir.join("bnxglobal.str"))?;
        let files: Vec<DataFile> =
            pack.of_type(pack::DATA).filter_map(|chunk| DataFile::parse(pack.data(chunk).to_vec()).ok()).collect();
        let list = files.iter().find_map(|f| f.root().get("characters")).ok_or("bnxglobal.str has no characters list")?;
        let attribute_names = files
            .iter()
            .flat_map(|f| f.root().field_names())
            .filter(|n| n.starts_with("characterAttributes"))
            .collect();
        Ok(Self { entries: list.items().map(read).collect(), attribute_names })
    }

    /// The entry by its display name (any case), else the first whose pack is `name`.
    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))
            .or_else(|| self.entries.iter().find(|e| pack_name(&e.file_path).eq_ignore_ascii_case(name)))
    }

    pub fn by_id(&self, id: u32) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// The entry whose attributes object hashes to `hash` (what a `PlayerSpawnPoint`'s
    /// `attributes` field holds: [data] `#a130b578` is `characterAttributesOptimus`, survey-levels).
    pub fn by_attributes(&self, hash: u32) -> Option<&Entry> {
        self.entries.iter().find(|e| e.attributes == hash)
    }

    /// `X` of the `characterAttributes<X>` object whose name hashes to `hash`.
    pub fn attributes_name(&self, hash: u32) -> Option<&str> {
        self.attribute_names.iter().find(|n| crc32(n.as_bytes()) == hash).map(|n| &n["characterAttributes".len()..])
    }

    /// What it takes to start an entry. `None` when its attributes object is not in the
    /// install's data.
    pub fn spec(&self, entry: &Entry) -> Option<Spec> {
        let pack = pack_name(&entry.file_path);
        let audio = if entry.audio_file.is_empty() { pack.clone() } else { pack_name(&entry.audio_file) };
        Some(Spec { pack, attributes: self.attributes_name(entry.attributes)?.to_owned(), audio, display: Some(entry.name.clone()) })
    }

    /// What it takes to start a pack that is not an entry (`dcomscout`): its attributes object is
    /// the `characterAttributes<X>` of the install's data whose `X` is the pack's name in any case
    /// (`DComScout`, `Firestorm`). [data] Only when no such object exists does `Spec::by_pack`'s
    /// capital guess stand.
    pub fn spec_by_pack(&self, pack: &str) -> Spec {
        let attributes = self
            .attribute_names
            .iter()
            .map(|n| &n["characterAttributes".len()..])
            .find(|x| x.eq_ignore_ascii_case(pack));
        Spec::by_pack(pack, attributes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game_dir() -> Option<PathBuf> {
        let dir = PathBuf::from(crate::install::dir_string());
        dir.join("bnxglobal.str").is_file().then_some(dir)
    }

    #[test]
    fn a_pack_that_is_not_an_entry_finds_its_attributes_by_name() {
        // Needs the install.
        let Some(dir) = game_dir() else { return };
        let list = CharacterList::load(&dir).expect("list");
        assert_eq!(list.spec_by_pack("dcomscout").attributes, "DComScout");
        assert_eq!(list.spec_by_pack("firestorm").attributes, "Firestorm");
        assert_eq!(list.spec_by_pack("ironhide").attributes, "Ironhide");
    }

    #[test]
    fn every_entry_resolves_to_a_pack_and_attributes() {
        // Needs the install.
        let Some(dir) = game_dir() else { return };
        let list = CharacterList::load(&dir).expect("list");
        assert_eq!(list.entries.len(), 24);
        let bee = list.spec(list.find("bumblebee").expect("bumblebee")).expect("spec");
        assert_eq!((bee.pack.as_str(), bee.attributes.as_str(), bee.audio.as_str()), ("bumblebee", "Bumblebee", "bumblebee"));
        let mut unresolved = Vec::new();
        for entry in &list.entries {
            let Some(spec) = list.spec(entry) else {
                println!("{:24} attributes {:#010x} / MP {:#010x}: no such object", entry.name, entry.attributes, entry.attributes_mp);
                unresolved.push(entry.name.clone());
                continue;
            };
            println!("{:24} {:#010x} {:?}", entry.name, entry.id, spec);
            assert!(spec.pack_file(&dir).is_file(), "{}", spec.pack);
            assert!(spec.sound_events(&dir).is_file(), "{}", spec.audio);
        }
        println!("unresolved: {unresolved:?}");
    }
}
