//! The game's mission table: `MissionList` in `bnxglobal.str` data chunk 5 (the unpublished notes
//! "Lead 1"). One entry per mission, 132 in the install. The join from a mission to its objects
//! is by pack path: `stream_files` names the zone's `zone` pack and the mission's own `mission`
//! pack, and `pack_paths` gives them with `.str` on (the original formats `%s.str`).
//!
//! Each entry is read from the data object's own field names (`[data]`), in snake_case. The
//! list stores each mission's `name` as text; `name_hash` is its CRC-32 (`formats::hash::crc32`),
//! which is what `nextMission` and `requiredMission` hold.
//!
//! Not decoded (left out, as the rule for shapes that do not decode cleanly):
//! - `checkPoints`: empty in all 132 entries, so its element shape is unknown. `[data]`
//! - `characterLineUp`: an array of structs (`charId`, `unlockId`, `unlockReqStrId`). Those are
//!   plain numbers, but the list is a struct array, and this module keeps to flat fields. `[data]`
//!
//! The enum fields `animSkill`, `leaderIcon` and `nightMission` carry values with no option
//! name in the table, so they are kept as the raw value. `[data]`

use std::path::Path;

use super::hash::crc32;
use super::lxb::{DataFile, Node};
use super::pack::{self, Pack};

/// One mission entry. Hashes are CRC-32 of the name (0 means none).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mission {
    /// CRC-32 of `name` (case sensitive); 0 when the entry has no name. [data] for the text,
    /// the hash is computed here.
    pub name_hash: u32,
    /// `name`: the mission's name text. [data]
    pub name: Option<String>,
    /// `category` (an enum: `Shanghai`, `US`, `Cairo`, `TestLevels`...). [data]
    pub category: Option<String>,
    /// `nextMission`: the mission it returns to (a War Room hub). [data]
    pub next_mission: u32,
    /// `requiredMission`: the mission that must be done first. [data]
    pub required_mission: u32,
    /// `streamFiles`: pack paths without `.str`, in file order, e.g. `Levels/Shanghai_01/zone`. [data]
    pub stream_files: Vec<String>,
    /// `audioFiles`: pack paths without `.str`, in file order. [data]
    pub audio_files: Vec<String>,
    /// `introMovie`, e.g. `Movies\D1_SHAN_CO`; `None` when the entry has none. [data]
    pub intro_movie: Option<String>,
    /// `deathHeight`. [data]
    pub death_height: i32,
    /// `animSkill`: raw enum value (no option name in the table). [data]
    pub anim_skill: u32,
    /// `loadScreen`: a name hash, 0 for none. [data]
    pub load_screen: u32,
    /// `maxTimePlatinum`, `maxTimeGold`, `maxTimeSilver`, `maxTimeBronze`. [data]
    pub max_time_platinum: i32,
    pub max_time_gold: i32,
    pub max_time_silver: i32,
    pub max_time_bronze: i32,
    /// `accBonusAmt`: five amounts, best first. [data]
    pub acc_bonus_amt: Vec<i32>,
    /// `accBonusReq`: five accuracy thresholds, best first. [data]
    pub acc_bonus_req: Vec<f32>,
    /// `bo1Bonus`, `bo2Bonus`. [data]
    pub bo1_bonus: i32,
    pub bo2_bonus: i32,
    /// `tfLogoAllDestroyedBonus`, `tfLogoTimeBonus`. [data]
    pub tf_logo_all_destroyed_bonus: i32,
    pub tf_logo_time_bonus: i32,
    /// `defaultHiScore`. [data]
    pub default_hi_score: i32,
    /// `unlockableId`: a name hash, 0 for none. [data]
    pub unlockable_id: u32,
    /// `nameStrId`, `descriptionStrId`: text ids (hashes). [data]
    pub name_str_id: u32,
    pub description_str_id: u32,
    /// `leaderIcon`: raw enum value. [data]
    pub leader_icon: u32,
    /// `bonusObj1StrId`, `bonusObj2StrId`: text ids (hashes). [data]
    pub bonus_obj1_str_id: u32,
    pub bonus_obj2_str_id: u32,
    /// `leaderBoard`: -1 for none, as stored. [data]
    pub leader_board: i32,
    /// `nightMission`: raw enum value, 0 or 1 in the list. [data] [assumed: 1 means a night
    /// mission, from the field name only]
    pub night_mission: u32,
    /// `worldMin`, `worldMax`: the mission's bounds. [data]
    pub world_min: [i32; 3],
    pub world_max: [i32; 3],
    /// `aiCharacterToleranceWorldHeightMax`. [data]
    pub ai_character_tolerance_world_height_max: i32,
    /// `usableCharactersIds`: character name hashes; 0 pads the unused slots. [data]
    pub usable_characters_ids: Vec<u32>,
}

/// One `characters` entry (the same data object's root list). [data] fields; [game] the parser 00750940 keeps
/// them in the options object's list +0x2034, searched by `id` (0075b900): the loading screen's `LB_BACKGROUND`
/// is the entry's `name` (008d5f20).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharacterEntry {
    /// `name`, e.g. `BUMBLEBEE`, `OPTIMUS PRIME`. [data]
    pub name: String,
    /// `id`: CRC-32 of the character's key (`BUMBLEBEE` a58837e5, `OPTIMUS` 45c21b69). [data]
    pub id: u32,
    /// `nameStrId`. [data]
    pub name_str_id: u32,
}

/// One zone of a campaign (`campaigns[n].zones`). [data] fields; [game] 00750940 inserts each into the options
/// object's list +0x204c (0075b6c0), searched by `id` (0075ba70); the loading screen's first top line is its
/// `nameStrId` (008d5f20).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Zone {
    pub id: u32,
    pub name_str_id: u32,
    pub desc_str_id: u32,
    /// `missionIds`: CRC-32 of the missions' names. [data]
    pub mission_ids: Vec<u32>,
}

/// The whole table, in file order (entry `[n]` is `missions[n]`), with the same object's characters and zones.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionList {
    pub missions: Vec<Mission>,
    pub characters: Vec<CharacterEntry>,
    pub zones: Vec<Zone>,
}

/// An integer field, or a float field read as one: the medal times and bonus amounts are stored as floats
/// (`maxTimePlatinum` 300.0, `bo1Bonus` 15000.0) and read 0 when only integers were taken (corrected 2026-10-16).
fn int(node: Option<Node>) -> i64 {
    node.and_then(|n| n.int().or_else(|| n.float().map(|f| f as i64))).unwrap_or(0)
}

fn texts(node: Option<Node>) -> Vec<String> {
    node.map(|n| n.items().filter_map(Node::text).collect()).unwrap_or_default()
}

fn text(node: Option<Node>) -> Option<String> {
    node.and_then(Node::text)
}

fn enum_text(node: Option<Node>) -> Option<String> {
    node.and_then(Node::enum_name).map(str::to_owned)
}

fn ints(node: Option<Node>) -> Vec<i64> {
    node.map(|n| n.items().filter_map(|i| i.int().or_else(|| i.float().map(|f| f as i64))).collect()).unwrap_or_default()
}

/// Three integers stored as an array, as a struct of three fields, or as floats: the first
/// reading that yields values is used (the first port read arrays only and returned zeros).
fn vec3(node: Option<Node>) -> [i32; 3] {
    let mut v = ints(node);
    if let (true, Some(n)) = (v.is_empty(), node) {
        v = n.fields().into_iter().filter_map(|(_, c)| c.int().or_else(|| c.float().map(|f| f as i64))).collect();
        if v.is_empty() {
            v = n.floats().into_iter().map(|f| f as i64).collect();
        }
    }
    std::array::from_fn(|i| v.get(i).copied().unwrap_or(0) as i32)
}

fn read(m: Node) -> Mission {
    let name = text(m.get("name"));
    Mission {
        name_hash: name.as_deref().map_or(0, |t| crc32(t.as_bytes())),
        name,
        category: enum_text(m.get("category")),
        next_mission: int(m.get("nextMission")) as u32,
        required_mission: int(m.get("requiredMission")) as u32,
        stream_files: texts(m.get("streamFiles")),
        audio_files: texts(m.get("audioFiles")),
        intro_movie: text(m.get("introMovie")),
        death_height: int(m.get("deathHeight")) as i32,
        anim_skill: int(m.get("animSkill")) as u32,
        load_screen: int(m.get("loadScreen")) as u32,
        max_time_platinum: int(m.get("maxTimePlatinum")) as i32,
        max_time_gold: int(m.get("maxTimeGold")) as i32,
        max_time_silver: int(m.get("maxTimeSilver")) as i32,
        max_time_bronze: int(m.get("maxTimeBronze")) as i32,
        acc_bonus_amt: ints(m.get("accBonusAmt")).into_iter().map(|v| v as i32).collect(),
        acc_bonus_req: m.get("accBonusReq").map(Node::floats).unwrap_or_default(),
        bo1_bonus: int(m.get("bo1Bonus")) as i32,
        bo2_bonus: int(m.get("bo2Bonus")) as i32,
        tf_logo_all_destroyed_bonus: int(m.get("tfLogoAllDestroyedBonus")) as i32,
        tf_logo_time_bonus: int(m.get("tfLogoTimeBonus")) as i32,
        default_hi_score: int(m.get("defaultHiScore")) as i32,
        unlockable_id: int(m.get("unlockableId")) as u32,
        name_str_id: int(m.get("nameStrId")) as u32,
        description_str_id: int(m.get("descriptionStrId")) as u32,
        leader_icon: int(m.get("leaderIcon")) as u32,
        bonus_obj1_str_id: int(m.get("bonusObj1StrId")) as u32,
        bonus_obj2_str_id: int(m.get("bonusObj2StrId")) as u32,
        leader_board: int(m.get("leaderBoard")) as i32,
        night_mission: int(m.get("nightMission")) as u32,
        world_min: vec3(m.get("worldMin")),
        world_max: vec3(m.get("worldMax")),
        ai_character_tolerance_world_height_max: int(m.get("aiCharacterToleranceWorldHeightMax")) as i32,
        usable_characters_ids: ints(m.get("usableCharactersIds")).into_iter().map(|v| v as u32).collect(),
    }
}

impl MissionList {
    /// Reads the `MissionList` root of a decoded data object (`bnxglobal.str` chunk 5).
    pub fn from_lxb(file: &DataFile) -> Result<Self, String> {
        let list = file.root().get("MissionList").ok_or("no MissionList in the data object")?;
        let missions = list.get("missions").ok_or("MissionList has no missions")?;
        let root = file.root();
        let characters = root
            .get("characters")
            .map(|l| {
                l.items()
                    .map(|c| CharacterEntry {
                        name: text(c.get("name")).unwrap_or_default(),
                        id: int(c.get("id")) as u32,
                        name_str_id: int(c.get("nameStrId")) as u32,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let zones = root
            .get("campaigns")
            .map(|l| {
                l.items()
                    .filter_map(|c| c.get("zones"))
                    .flat_map(|z| z.items().collect::<Vec<_>>())
                    .map(|z| Zone {
                        id: int(z.get("id")) as u32,
                        name_str_id: int(z.get("nameStrId")) as u32,
                        desc_str_id: int(z.get("descStrId")) as u32,
                        mission_ids: ints(z.get("missionIds")).into_iter().map(|v| v as u32).collect(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self { missions: missions.items().map(read).collect(), characters, zones })
    }

    /// The character whose `id` is `id`.
    pub fn character(&self, id: u32) -> Option<&CharacterEntry> {
        self.characters.iter().find(|c| c.id == id)
    }

    /// The zone that lists the mission named `name_hash`. [assumed] this is the engine's current zone (options
    /// +0x1f70, copied from +0x211c when a hub is left, 007539d0) when a mission starts: the zone the player picked
    /// it from, and every mission is listed by one zone only (not checked).
    pub fn zone_of(&self, name_hash: u32) -> Option<&Zone> {
        self.zones.iter().find(|z| z.mission_ids.contains(&name_hash))
    }

    /// Reads the list from the install's `bnxglobal.str`, data chunk 5.
    pub fn load(game_dir: &Path) -> Result<Self, String> {
        let pack = Pack::open(&game_dir.join("bnxglobal.str"))?;
        let chunk = pack
            .chunks
            .iter()
            .find(|c| c.index == 5 && c.tag == pack::DATA)
            .ok_or("bnxglobal.str has no data chunk 5")?;
        Self::from_lxb(&DataFile::parse(pack.data(chunk).to_vec())?)
    }

    /// The mission whose name hashes to `h`.
    pub fn by_hash(&self, h: u32) -> Option<&Mission> {
        self.missions.iter().find(|m| m.name_hash == h)
    }

    /// The pack files a mission streams, each with `.str` appended (`%s.str`).
    pub fn pack_paths(&self, m: &Mission) -> Vec<String> {
        m.stream_files.iter().map(|f| format!("{f}.str")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The install, when it is there. The tests skip (pass) when it is not.
    fn install() -> Option<PathBuf> {
        let dir = PathBuf::from(crate::install::dir_string());
        dir.join("bnxglobal.str").is_file().then_some(dir)
    }

    #[test]
    fn world_bounds_are_read() {
        // Needs the install. Values from `tools\tf2_lxb.py dump bnxglobal.str#5`.
        let Some(dir) = install() else { return };
        let list = MissionList::load(&dir).unwrap();
        let m = list.missions.iter().find(|m| m.name.as_deref() == Some("Shanghai_01_FreeRoam_A")).unwrap();
        assert_eq!((m.world_min, m.world_max), ([-108, -273, -73], [602, 428, 226]));
    }

    #[test]
    fn the_list_has_132_missions_and_each_name_hashes_to_its_hash() {
        // Needs the install.
        let Some(dir) = install() else { return };
        let list = MissionList::load(&dir).unwrap();
        assert_eq!(list.missions.len(), 132);
        for m in &list.missions {
            let name = m.name.as_deref().unwrap_or_else(|| panic!("entry without a name: hash {:08x}", m.name_hash));
            assert_eq!(crc32(name.as_bytes()), m.name_hash, "{name}");
        }
    }

    #[test]
    fn every_required_and_next_mission_resolves() {
        // Needs the install.
        let Some(dir) = install() else { return };
        let list = MissionList::load(&dir).unwrap();
        for m in &list.missions {
            for h in [m.required_mission, m.next_mission] {
                if h != 0 {
                    assert!(list.by_hash(h).is_some(), "{:?} points at {h:08x}, which is no entry", m.name);
                }
            }
        }
    }

    #[test]
    fn the_shanghai_construction_a_csa_mission_has_its_zone_and_its_own_pack() {
        // Needs the install.
        let Some(dir) = install() else { return };
        let list = MissionList::load(&dir).unwrap();
        let m = list.by_hash(crc32(b"Shanghai_01_Construction_A_CSA")).expect("entry");
        assert_eq!(m.stream_files, ["Levels/Shanghai_01/zone", "Levels/Shanghai_01/Missions/Shanghai_01_Construction_A_CSA/mission"]);
        assert_eq!(m.required_mission, crc32(b"Shanghai_03_Waterfront_A_Hunt"));
        assert_eq!(m.next_mission, crc32(b"WarRoom_Autobot"));
        assert_eq!(list.pack_paths(m).len(), 2);
        assert!(list.pack_paths(m)[0].ends_with("zone.str"));
    }

    #[test]
    fn every_stream_file_of_every_mission_is_on_disk() {
        // Needs the install. Reports the count of missing packs; does not fail on them.
        let Some(dir) = install() else { return };
        let list = MissionList::load(&dir).unwrap();
        let mut total = 0;
        let mut missing = Vec::new();
        for m in &list.missions {
            for path in list.pack_paths(m) {
                total += 1;
                // Windows file names are case-insensitive, so `exists` matches the names as written.
                if !dir.join(&path).is_file() {
                    missing.push(path);
                }
            }
        }
        println!("stream files: {total} checked, {} missing", missing.len());
        for path in &missing {
            println!("missing: {path}");
        }
    }
}
