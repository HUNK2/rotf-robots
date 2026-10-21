//! CharacterSpawnManager: the enemy spawners of a mission, read from the mission pack's data
//! chunk 0 and run as the game's update rules say.
//!
//! Data and runtime in one module. Everything is read in place from the user's install at
//! runtime (`C:\Games2` or `TF2_GAME_DIR`); nothing from the game is bundled. Sources:
//! `notes\ai-spawning.md` (the first read, and the second read whose corrections win) and
//! `notes\ai-level-data.md` section A2 (the spawner's fields as the data writes them).
//! [game] is read in code, [data] is read from a mission pack, [assumed] and [stand-in] are
//! tagged where they apply. Every assumption is listed in `notes\status.md` under
//! "Phase 3, enemy AI: spawners".
//!
//! The runtime [`Spawner`] does not place characters, route messages or look at the world
//! itself. It returns [`Action`]s and asks the caller through [`SpawnWorld`].

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::formats::hash::crc32;
use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};

/// The type name of a spawner object. [data]
pub const SPAWNER_TYPE: &str = "CharacterSpawnManager";

/// Deepest a mission chunk is walked for spawners (the data nests Instances inside Instances). The
/// same depth as `mission_script` (256): at 64 the walk loses spawners of the deeper chunks.
const MAX_DEPTH: u32 = 256;

/// [stand-in] Logical ids replace the original's retained character object handles.
/// Allocate across spawners rather than packing LuxScriptName: that is a message
/// filter and can be zero or shared. Never reuse an id, including after a reset.
/// [assumed] Reservation matches MissionRun's current id domains: 0 is null, 1 is
/// the player, and the high bit identifies named script objects. See status.md
/// "Spawn identity collisions"; the original numeric id allocator is not ported.
static NEXT_CHARACTER_ID: AtomicU32 = AtomicU32::new(2);

// Message hashes (CRC-32 of the name, [game] checked against the handler's immediates in the
// second read, `notes\ai-spawning.md` section 1). [game]
/// `Enable`. Sets the spawner enabled.
pub const MSG_ENABLE: u32 = 0xf40c_cd04;
/// `Trigger` (the data writes `#d5d636c1`). Any hash that is not Disable or Untrigger enables. [game]
pub const MSG_TRIGGER: u32 = 0xd5d6_36c1;
/// `TRIGGER` (the data writes `TRIGGER`). Enables like any other hash. [game]
pub const MSG_TRIGGER_CAPS: u32 = 0x252e_f147;
/// `Disable`. Runs the done routine: the spawner is disabled. [game]
pub const MSG_DISABLE: u32 = 0x80f8_74cd;
/// `Untrigger`. Optional delete-all, optional reset, then disabled. [game]
pub const MSG_UNTRIGGER: u32 = 0x9caa_723d;

// Spawn point choice, by `spawnHeuristic` (hash of the data's `#xxxxxxxx`). [game] branches
// in `0073c010` and `0117f3a0`.
/// `#3d9b7964`: sequential, most mission spawners. [game]
pub const HEUR_SEQUENTIAL: u32 = 0x3d9b_7964;
/// `#3905834d`: nearest to the reference point, with the distance band. [game]
pub const HEUR_NEAREST: u32 = 0x3905_834d;
/// `#d7ee931a` (`-0x28116ce6`): the FARTHEST usable point from the target object's point (the player
/// [assumed]). No band. [game] `0073c010`: it takes the target path (`007123a0`, LAB_0073c4e6), and the
/// flag bVar5 is clear, so the compare at `0073c7b5`-`0073c7c9` keeps the larger squared distance.
pub const HEUR_FARTHEST_TARGET: u32 = 0xd7ee_931a;
/// `#e38a8eb4`: random order (game LCG, [assumed] as to the points). [game] for the branch.
pub const HEUR_RANDOM: u32 = 0xe38a_8eb4;
/// The NEAREST usable point, from the target path: `#920e4a62` (`-0x6df1b59e`) and `#fc74233f` (`-0x38bdcc1`).
/// [game] `0073c010`: both are in the bVar5 set, which starts the best distance at FLT_MAX and keeps the
/// smaller one (`0073c7c4`-`0073c7c9` and the `comiss` at `0073c7b7`). `920e4a62` has no band (`0117f3a0`
/// does not list it); `fc74233f` does (`0117f3a0`, see `HEUR_RANGE_ON`).
pub const HEUR_NEAREST_SET: [u32; 2] = [0x920e_4a62, 0xfc74_233f];
/// Farthest from the reference point: `#a03bd38a` (`-0x5fc42c76`) and `#febc8345` (`-0x1437cbb`). [stand-in]
/// Their game branches are not the loop read in `0073c010` (`a03bd38a` goes to LAB_0073c075, `febc8345` to the
/// LAB_0073c17c loop), and both are in the bVar5 set, which the read loop uses as NEAREST. Which way they
/// go in their own branches is [unknown]; the port keeps the farthest until they are read.
pub const HEUR_FARTHEST: [u32; 2] = [0xa03b_d38a, 0xfebc_8345];
/// Heuristics the function `0117f3a0` lists with the distance band but whose branch is not read. [game] list, meaning unknown.
pub const HEUR_UNREAD: [u32; 3] = [0xf268_120d, 0xefd0_c9ca, 0xdfb9_9435];
/// The heuristics for which `0117f3a0` returns 1, so the squared distance band applies. [game]
pub const HEUR_RANGE_ON: [u32; 6] = [0xfebc_8345, 0xfc74_233f, 0x3905_834d, 0xf268_120d, 0xefd0_c9ca, 0xdfb9_9435];

/// The point rule a heuristic runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Sequential,
    Nearest,
    Farthest,
    Random,
}

/// The rule for a heuristic. A hash whose branch is not read runs sequentially. [stand-in]
pub fn pick_of(heuristic: u32) -> Pick {
    match heuristic {
        HEUR_SEQUENTIAL => Pick::Sequential,
        HEUR_NEAREST => Pick::Nearest,
        HEUR_RANDOM => Pick::Random,
        HEUR_FARTHEST_TARGET => Pick::Farthest,
        h if HEUR_NEAREST_SET.contains(&h) => Pick::Nearest,
        h if HEUR_FARTHEST.contains(&h) => Pick::Farthest,
        _ => Pick::Sequential,
    }
}

/// Whether the heuristic's branch is one the notes give. The others fall back to sequential. [stand-in]
pub fn heuristic_known(heuristic: u32) -> bool {
    matches!(heuristic, HEUR_SEQUENTIAL | HEUR_NEAREST | HEUR_FARTHEST_TARGET | HEUR_RANDOM) || HEUR_NEAREST_SET.contains(&heuristic) || HEUR_FARTHEST.contains(&heuristic)
}

/// A name for a heuristic, for printing.
pub fn heuristic_name(heuristic: u32) -> &'static str {
    match heuristic {
        HEUR_SEQUENTIAL => "sequential",
        HEUR_NEAREST => "nearest (band)",
        HEUR_FARTHEST_TARGET => "farthest to target",
        HEUR_RANDOM => "random",
        h if HEUR_NEAREST_SET.contains(&h) => "nearest",
        h if HEUR_FARTHEST.contains(&h) => "farthest (stand-in)",
        h if HEUR_UNREAD.contains(&h) => "unread (stand-in: sequential)",
        _ => "not in the notes (stand-in: sequential)",
    }
}

/// characterDefinition name hashes (CRC-32 of the name), as the mission data writes them. [data]
/// `notes\ai-spawn-characters.md` section 1.
pub const DEF_APROT_SCOUT: u32 = 0xec6f_6dfa; // CharDef_AProtScout
pub const DEF_APROT_SNIPER: u32 = 0xb964_f164; // CharDef_AProtSniper
pub const DEF_APROT_WARRIOR: u32 = 0x6839_538b; // CharDef_AProtWarrior
pub const DEF_OPTIMUS: u32 = 0x72b3_20c0; // CharDef_Optimus
pub const DEF_FAKE_DCOM: u32 = 0x382e_9ac3; // Fake_DCOM_DEF (the tutorial's warrior)
pub const DEF_FIRESTORM: u32 = 0xa17e_aaaa; // Char_Def_Firestorm
pub const DEF_BUMBLEBEE: u32 = 0x5280_ac03; // Char_Def_Bumblebee
/// [data] names.tsv (bd6dbb8d, 7754172d, 67f79a59) and CRC-32 of the names (checked in a test).
pub const DEF_DCOM_SCOUT: u32 = 0xbd6d_bb8d; // CharDef_DComScout (Shanghai)
pub const DEF_DCOM_SNIPER: u32 = 0x7754_172d; // CharDef_DComSniper (Shanghai)
pub const DEF_DCOM_WARRIOR: u32 = 0x67f7_9a59; // CharDef_DComWarrior (Shanghai)
pub const DEF_STUNTICON_SNIPER: u32 = 0xf0ad_4aa4; // CharDef_StunticonSniper (Shanghai)
/// The escort mission's variants (`work\survey\escort_spawn_dump.txt`, `notes\mission-escort-characters.md`).
/// Each is its base definition's pack and block with a different `fastAreaRadius` on the definition. [data]
pub const DEF_DCOM_SCOUT_SHORT: u32 = 0x690e_95b8; // CharDef_DComScout_ShortRange (fastAreaRadius 150)
pub const DEF_DCOM_SCOUT_FAR: u32 = 0x3a8e_8404; // CharDef_DComScoutFAR (fastAreaRadius 5000, the ambush)
pub const DEF_DCOM_WARRIOR_SHORT: u32 = 0xfdfb_1b4a; // CharDef_DComWarriorShortRng (fastAreaRadius 150)
/// The escort's convoy truck: a vehicle (block characterAttributesSemiTruck), no pack in the install. [data]
pub const DEF_CONVOY: u32 = 0xd29d_af65; // Convoy_Character_Definition

/// The pack and the `X` of `characterAttributes<X>` that build a record's characterDefinition, for
/// the enemies the install can make. [data] `notes\ai-spawn-characters.md` section 4.
///
/// `None` for: the player's own character and the allies (Optimus, Bumblebee, Firestorm: see
/// `skip_reason`); AProtSniper, AProtWarrior and StunticonSniper (no pack file, see `skip_reason`);
/// any other hash (not in the notes). [assumed] the AProtScout definition loads the pack `aprotscoutmp`
/// (the only AProt pack, the same name; settled by the definition loader `00577620`, unread).
/// [assumed] Fake_DCOM_DEF is DComWarrior (the data root's name; the tutorial's own block
/// `DComWarriorTutorial` is not used). [assumed] the DCom definitions use the pack and the block of
/// their own name (`dcomscout.str` + `characterAttributesDComScout`, and so on); settled by the
/// definition loader `00577620` (unread), or by a recording of a Shanghai spawn.
///
/// [stand-in: overrides not applied] The three escort variants map to their base pack and block. Their
/// definitions override `fastAreaRadius` only (DComScout_ShortRange 150, DComScoutFAR 5000, DComWarriorShortRng
/// 150; base 1000), and `character_inputs` cannot carry that. [data] `notes\mission-escort-characters.md`.
/// [assumed] the variant definitions' link to the base pack (the definition's `attributes` hash is the base block).
pub fn character_inputs(def_hash: u32) -> Option<(&'static str, &'static str)> {
    match def_hash {
        DEF_APROT_SCOUT => Some(("aprotscoutmp", "APrtScout")),
        DEF_FAKE_DCOM => Some(("dcomwarrior", "DComWarrior")),
        // [data] the three packs and blocks exist in the install (names above); [assumed] the name link.
        DEF_DCOM_SCOUT => Some(("dcomscout", "DComScout")),
        DEF_DCOM_SNIPER => Some(("dcomsniper", "DComSniper")),
        DEF_DCOM_WARRIOR => Some(("dcomwarrior", "DComWarrior")),
        // [stand-in: overrides not applied] fastAreaRadius 150 (base 1000) is not carried.
        DEF_DCOM_SCOUT_SHORT => Some(("dcomscout", "DComScout")),
        // [stand-in: overrides not applied] fastAreaRadius 5000 (base 1000) is not carried.
        DEF_DCOM_SCOUT_FAR => Some(("dcomscout", "DComScout")),
        // [stand-in: overrides not applied] fastAreaRadius 150 (base 1000) is not carried.
        DEF_DCOM_WARRIOR_SHORT => Some(("dcomwarrior", "DComWarrior")),
        _ => None,
    }
}

/// Why `character_inputs` gives no pack for a definition, for the spawn log. [data] the packs are the
/// install's `characters\*.str` (`ls`); [assumed] a definition with no pack of its name has no pack.
pub fn skip_reason(def_hash: u32) -> &'static str {
    match def_hash {
        DEF_APROT_SNIPER => "no pack file (the install has no AProtSniper pack; characterAttributesAPrtSniper exists)",
        DEF_APROT_WARRIOR => "no pack file (the install has no AProtWarrior pack; characterAttributesAPrtWarrior exists)",
        DEF_STUNTICON_SNIPER => "no pack file (the install has no Stunticon sniper pack; characterAttributesDStnSniper exists)",
        DEF_OPTIMUS | DEF_FIRESTORM | DEF_BUMBLEBEE => "ally / player character (not an enemy)",
        DEF_CONVOY => "vehicle not buildable yet (block characterAttributesSemiTruck, isVehicle true; no truck pack in characters\\, the model is embedded in the mission chunk)",
        _ => "not in the table (no pack or block known for this definition)",
    }
}

// The field names each object holds, as the data writes them. [data] `notes\ai-level-data.md` A2.
const SPAWNER_FIELDS: &[&str] = &[
    "LuxScriptName",
    "acceptNullTriggerMessages",
    "startEnabled",
    "spawnHeuristic",
    "spawnDelay",
    "spawnOutOfView",
    "maxEnemiesAtOnce",
    "maxEnemiesLifetime",
    "deleteAllCharsOnUnTrigger",
    "resetOnUnTrigger",
    "msgDeleteAll",
    "useDeferredSpawning",
    "maxEnemyDelay",
    "minSpawnRange",
    "maxSpawnRange",
    "trigDone",
    "trigOnSpawn",
    "trigOnDeath",
    "characterSpawnInfo",
];
const RECORD_FIELDS: &[&str] = &[
    "spawnInfoName",
    "characterDefinition",
    "spawnPoints",
    "useSpawnPointAsHomePoint",
    "homePoints",
    "homeRegions",
    "splines",
    "useWayPoints",
    "messageListOnSpawn",
    "radarIconType",
    "radarIconNew",
    "groupID",
    "charDisabled",
    "disableCollision",
    "spawnInVehicleForm",
    "targetPlayerPriorityDistance",
];
const TRIGGER_ARRAY_FIELDS: &[&str] = &["overrideMsgType", "overrideLuxScriptNameSendFilter", "triggerArray"];
/// The `overrideMsgType` values the install writes when a trigger array overrides nothing: `#fe74aaca`
/// (the hunt) and the enum `NOOVERRIDE` (its hash `0x43c0d673`, the tutorial). [data]
pub const OVERRIDE_NONE: [u32; 2] = [0xfe74_aaca, 0x43c0_d673];
const TRIGGER_ENTRY_FIELDS: &[&str] = &["objToSendMsgTo", "msgType", "luxScriptNameSendFilter", "delayMin", "delayMax"];

// ---------------------------------------------------------------------------------------------
// The data. One spawner, as the mission data gives it.
// ---------------------------------------------------------------------------------------------

/// A spawn point: an Instance (`SpwnPt_*`) of a record's `spawnPoints` list. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnPoint {
    /// The Instance's data name, or `#hex` when the table does not spell it.
    pub name: String,
    /// CRC-32 of `name` (the data's label).
    pub hash: u32,
    /// The Instance's translation (row 3 of its matrix), metres, x right, y forward, z up. [data] [assumed] layout.
    pub place: [f32; 3],
    /// The Instance's forward axis (row 1 of its matrix), the facing. [assumed] row 1 is forward.
    pub forward: [f32; 3],
}

impl SpawnPoint {
    /// The facing as a heading in radians, measured from +y towards +x. [assumed] from `forward`.
    pub fn heading(&self) -> f32 {
        self.forward[0].atan2(self.forward[1])
    }
}

/// A characterDefinition: an Instance (`CharDef_*`) or a Ref to one. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Definition {
    pub name: String,
    pub hash: u32,
    /// Whether the decoder read the definition's body (false for a Ref it did not follow). [data]
    pub decoded: bool,
}

/// One `characterSpawnInfo` record: the character a spawner makes and where. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnRecord {
    /// `spawnInfoName`, a name hash. [data]
    pub info_name: u32,
    /// `characterDefinition`, or `None` for a null pointer. [data]
    pub definition: Option<Definition>,
    /// `spawnPoints`, in the data's order. Points whose matrix is not decoded are left out (reported).
    pub points: Vec<SpawnPoint>,
    /// True when `points` is the ONE point the game gives a record whose `spawnPoints` is empty: the
    /// spawner's owner Instance (its position, `SpawnerData::place`). [game] prepare `007380f0` lines
    /// 132-161 pushes one point at record+0x18 when record+0x1c is 0, from the owner pair. [assumed] the
    /// owner pair is the spawner's Instance (see `SpawnerData::place`). homePoints are never spawn points.
    pub points_are_owner: bool,
    /// `useSpawnPointAsHomePoint`. [data]
    pub use_spawn_point_as_home: bool,
    /// `groupID`. [data]
    pub group_id: i64,
    /// `homeRegions`: the AIHomeRegion Instances' name hashes. [data]
    pub home_regions: Vec<u32>,
    /// `messageListOnSpawn`: the message name hashes sent to the character when it spawns, in the
    /// data's order. [data] (the names are the enum values, e.g. `AlertedOn`, `ImmuneToOverdrive`).
    pub messages_on_spawn: Vec<u32>,
}

/// A trigger array (`trigDone`, `trigOnSpawn`, `trigOnDeath`): `{overrideMsgType, overrideLuxScriptNameSendFilter, triggerArray}`. [data]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TriggerArray {
    /// `overrideMsgType`: `None` when false. What it overrides is [assumed] (see `Spawner::send`).
    pub override_msg_type: Option<u32>,
    /// `overrideLuxScriptNameSendFilter`: `None` when false. Meaning [unknown], not used at run time.
    pub override_filter: Option<u32>,
    pub entries: Vec<TriggerEntry>,
}

/// One entry of a trigger array: `{objToSendMsgTo, msgType, luxScriptNameSendFilter, delayMin, delayMax}`. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct TriggerEntry {
    /// The name hash of `objToSendMsgTo`, 0 when it has no name. [data] Routing is not done here (stand-in).
    pub target: u32,
    /// `msgType`: the message hash. [data]
    pub msg_type: u32,
    /// `luxScriptNameSendFilter`, `None` when false. [data] Not used at run time (stand-in).
    pub filter: Option<u32>,
    /// `delayMin`, `delayMax`, seconds. [data] Not used at run time (stand-in).
    pub delay_min: f32,
    pub delay_max: f32,
}

/// Where a spawner sits in its mission pack, in the terms `mission_script` uses. The script finds
/// the same object by [`crate::mission_script::Script::at`] with (`chunk`, `offset`), and its parent
/// by (`chunk`, `parent_offset`). [data] [assumed] the parent is the nearest enclosing typed node
/// of the walk, as `mission_script` takes it (its header test is not repeated here).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    /// The pack's data chunk index the spawner was read from. [data]
    pub chunk: usize,
    /// The spawner's offset in that chunk. `None` when the node is not stored data. [data]
    pub offset: Option<usize>,
    /// The offset of the nearest enclosing typed object in the same chunk, `None` at the root. [assumed]
    pub parent_offset: Option<usize>,
    /// The name hash of that enclosing object (0 when unnamed). [data]
    pub parent_name: u32,
}

/// One CharacterSpawnManager as the mission data gives it. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnerData {
    /// Where it sits: chunk, offset, and its parent (see [`Identity`]).
    pub identity: Identity,
    /// The spawner object's data name, empty when it has none.
    pub name: String,
    /// CRC-32 of `name` (0 when none).
    pub hash: u32,
    /// The translation of the Instance that holds the spawner (its place in the level), when there is one.
    pub place: Option<[f32; 3]>,
    /// `LuxScriptName`: the script name that message targets match (key A). [data]
    pub lux_script_name: u32,
    /// `acceptNullTriggerMessages`. [data] Not checked by `Spawner::message` (the caller matches targets, stand-in).
    pub accept_null_trigger_messages: bool,
    /// `startEnabled`. [data]
    pub start_enabled: bool,
    /// `spawnHeuristic`: the point rule (see [`pick_of`]). [data]
    pub spawn_heuristic: u32,
    /// `spawnDelay`: seconds after a spawn before the next. [data]
    pub spawn_delay: f32,
    /// `spawnOutOfView`: a point is usable only with no straight corridor from the reference. [data] [game]
    pub spawn_out_of_view: bool,
    /// `maxEnemiesAtOnce`. [data]
    pub max_enemies_at_once: i64,
    /// `maxEnemiesLifetime`: the total number of spawns allowed. [data]
    pub max_enemies_lifetime: i64,
    /// `maxEnemyDelay`: the wait after the spawner finds itself at its alive limit. [data]
    pub max_enemy_delay: f32,
    /// `minSpawnRange`, `maxSpawnRange`: the squared distance band's ends, metres. [data]
    pub min_spawn_range: f32,
    pub max_spawn_range: f32,
    /// `deleteAllCharsOnUnTrigger`, `resetOnUnTrigger`. [data]
    pub delete_all_on_untrigger: bool,
    pub reset_on_untrigger: bool,
    /// `msgDeleteAll`: the message that deletes all characters (key B). 0 when none. [data]
    pub msg_delete_all: u32,
    /// `useDeferredSpawning`. [data] Its gate (DAT_00cf5c5f) is not modelled (stand-in).
    pub use_deferred_spawning: bool,
    /// `trigDone`, `trigOnSpawn`, `trigOnDeath`. [data]
    pub trig_done: TriggerArray,
    pub trig_on_spawn: TriggerArray,
    pub trig_on_death: TriggerArray,
    /// `characterSpawnInfo`, in the data's order. [data]
    pub records: Vec<SpawnRecord>,
    /// Everything that did not read as the schema says, with where it was found. Nothing is dropped quietly.
    pub missing: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// The reader. Field access goes through `lxb::Node`; every failure is recorded, not dropped.
// The helpers mirror `ai_data.rs` (its `Reader` is private to that module).
// ---------------------------------------------------------------------------------------------

/// A name hash read as the data writes it: an enum value (its name's hash) or an integer.
fn hash_of(node: Node) -> Option<u32> {
    node.enum_hash().or_else(|| node.int().map(|v| v as u32))
}

fn name_text(file: &DataFile, hash: u32) -> String {
    file.name(hash).map_or_else(|| format!("#{hash:08x}"), str::to_owned)
}

/// The data name and hash of an object, reached through a pointer or a field. `(0, "")` when unnamed.
fn name_of(node: Node) -> (u32, String) {
    match node.label() {
        Some(hash) => (hash, name_text(node.file(), hash)),
        None => (0, String::new()),
    }
}

#[derive(Default)]
struct Reader {
    missing: Vec<String>,
}

impl Reader {
    fn report(&mut self, owner: &str, what: impl AsRef<str>) {
        self.missing.push(format!("{owner}: {}", what.as_ref()));
    }

    /// The named field, or a report that it is absent.
    fn field<'a>(&mut self, node: Node<'a>, owner: &str, name: &str) -> Option<Node<'a>> {
        let found = node.get(name);
        if found.is_none() {
            self.report(owner, format!("no field {name}"));
        }
        found
    }

    /// Reports every field of the object that the schema does not list.
    fn expect_fields(&mut self, node: Node, owner: &str, expected: &[&str]) {
        let file = node.file();
        for (hash, _) in node.fields() {
            if !expected.iter().any(|name| crc32(name.as_bytes()) == hash) {
                let text = name_text(file, hash);
                self.report(owner, format!("unexpected field {text}"));
            }
        }
    }

    fn float(&mut self, node: Node, owner: &str, name: &str) -> f32 {
        let Some(field) = self.field(node, owner, name) else { return 0.0 };
        field.float().unwrap_or_else(|| {
            self.report(owner, format!("{name} is not a number"));
            0.0
        })
    }

    fn int(&mut self, node: Node, owner: &str, name: &str) -> i64 {
        let Some(field) = self.field(node, owner, name) else { return 0 };
        field.int().unwrap_or_else(|| {
            self.report(owner, format!("{name} is not an integer"));
            0
        })
    }

    /// A flag. The data writes it as `true` or `false`, or as the raw value `#00000001` / `#00000000`
    /// (`startEnabled`, `acceptNullTriggerMessages` in the install): nonzero reads as true. [data]
    fn flag(&mut self, node: Node, owner: &str, name: &str) -> bool {
        let Some(field) = self.field(node, owner, name) else { return false };
        match field.enum_name() {
            Some("true") => true,
            Some("false") => false,
            _ => match field.int() {
                Some(v) => v != 0,
                None => {
                    self.report(owner, format!("{name} is not true or false"));
                    false
                }
            },
        }
    }

    fn hash(&mut self, node: Node, owner: &str, name: &str) -> u32 {
        let Some(field) = self.field(node, owner, name) else { return 0 };
        hash_of(field).unwrap_or_else(|| {
            self.report(owner, format!("{name} is not a name hash"));
            0
        })
    }

    /// A hash field that is `false` when there is none.
    fn optional_hash(&mut self, node: Node, owner: &str, name: &str) -> Option<u32> {
        let field = self.field(node, owner, name)?;
        match field.enum_name() {
            Some("false") => None,
            Some("true") => {
                self.report(owner, format!("{name} is true where a hash or false was expected"));
                None
            }
            _ => hash_of(field).or_else(|| {
                self.report(owner, format!("{name} is not a name hash"));
                None
            }),
        }
    }

    /// The items of a list field. An absent field reads as an empty list, and is reported.
    fn list<'a>(&mut self, node: Node<'a>, owner: &str, name: &str) -> Vec<Node<'a>> {
        match self.field(node, owner, name) {
            Some(list) => list.items().collect(),
            None => Vec::new(),
        }
    }

    fn triggers(&mut self, node: Node, owner: &str, name: &str) -> TriggerArray {
        let Some(array) = self.field(node, owner, name) else { return TriggerArray::default() };
        let owner = format!("{owner}.{name}");
        let owner = owner.as_str();
        self.expect_fields(array, owner, TRIGGER_ARRAY_FIELDS);
        // The install writes a sentinel (or 0) when there is no override. [data]
        let override_msg_type = self.optional_hash(array, owner, "overrideMsgType").filter(|h| !OVERRIDE_NONE.contains(h));
        let override_filter = self.optional_hash(array, owner, "overrideLuxScriptNameSendFilter").filter(|h| *h != 0 && !OVERRIDE_NONE.contains(h));
        let mut entries = Vec::new();
        for (i, entry) in self.list(array, owner, "triggerArray").into_iter().enumerate() {
            let owner = format!("{owner}.triggerArray[{i}]");
            let owner = owner.as_str();
            self.expect_fields(entry, owner, TRIGGER_ENTRY_FIELDS);
            let target = self.field(entry, owner, "objToSendMsgTo").and_then(Node::label).unwrap_or(0);
            let msg_type = self.hash(entry, owner, "msgType");
            let filter = self.optional_hash(entry, owner, "luxScriptNameSendFilter");
            let delay_min = self.float(entry, owner, "delayMin");
            let delay_max = self.float(entry, owner, "delayMax");
            entries.push(TriggerEntry { target, msg_type, filter, delay_min, delay_max });
        }
        TriggerArray { override_msg_type, override_filter, entries }
    }

    fn point(&mut self, node: Node, owner: &str) -> Option<SpawnPoint> {
        let (hash, name) = name_of(node);
        let Some(matrix) = node.get("matrix") else {
            self.report(owner, "spawn point has no matrix (not decoded)");
            return None;
        };
        let m = matrix.floats();
        let (Some(place), Some(forward)) = (matrix_row(&m, 3), matrix_row(&m, 1)) else {
            self.report(owner, "spawn point matrix is not 4 rows");
            return None;
        };
        Some(SpawnPoint { name, hash, place, forward })
    }

    fn record(&mut self, node: Node, owner: &str, owner_point: Option<&SpawnPoint>) -> SpawnRecord {
        self.expect_fields(node, owner, RECORD_FIELDS);
        let info_name = self.hash(node, owner, "spawnInfoName");
        let definition = match self.field(node, owner, "characterDefinition") {
            Some(def) => def.label().map(|hash| Definition {
                hash,
                name: name_text(def.file(), hash),
                decoded: def.type_name().is_some(),
            }),
            None => None,
        };
        let mut points = Vec::new();
        for (i, item) in self.list(node, owner, "spawnPoints").into_iter().enumerate() {
            let at = format!("{owner}.spawnPoints[{i}]");
            if let Some(point) = self.point(item, &at) {
                points.push(point);
            }
        }
        // homePoints are read for the home list only; they are never spawn points. [game] the homePoints
        // loop of `0073b2b0` writes a separate list (record+0x2c), and prepare `007380f0` lines 162-215
        // only set the home nodes' handles. [data] the escort's empty-spawnPoints records have a homePoints
        // list of named Instances (SniperPt_*, EnemyPosNode_*, ConvoyNode_* ...), which is NOT read here.
        // [unknown] the list's use as a home point (the walk home) is not ported; nothing reads it yet.
        let (points, points_are_owner) = resolve_points(points, owner_point);
        // [unknown] record 21527 (Enemy_Funneler02's CharacterSpawnManager, the escort's `characterSpawnInfo[0]`)
        // carries a spline (`splines`: 1 spline of 10 points, [data]) and `messageListOnSpawn` SplineGoVehicleOneShot
        // [data]. The spline is not parsed here. Its effect after the spawn (the route the character takes)
        // is unread; the point pick does not use it [game] (`0073c010` reads record+0x18 only).
        let use_spawn_point_as_home = self.flag(node, owner, "useSpawnPointAsHomePoint");
        let group_id = self.int(node, owner, "groupID");
        let home_regions: Vec<u32> = self.list(node, owner, "homeRegions").into_iter().filter_map(Node::label).collect();
        // [data] the list's items are enum values (`hash_of` reads the enum's name hash, as the flags do).
        let messages_on_spawn: Vec<u32> = self.list(node, owner, "messageListOnSpawn").into_iter().filter_map(hash_of).collect();
        SpawnRecord { info_name, definition, points, points_are_owner, use_spawn_point_as_home, group_id, home_regions, messages_on_spawn }
    }
}

/// The spawn points a record ends with. [game] prepare `007380f0` lines 132-161: a record whose
/// spawnPoints is empty gets exactly ONE point, the spawner's owner (its Instance's position). A record
/// with points keeps them. With no owner Instance (no matrix above the spawner in the data) the record
/// keeps none. Returns the points and whether the one point is the owner's. [game] [assumed] the owner
/// pair is the spawner's Instance (`SpawnerData::place`); settle: the attach writer of spawner obj+4/+8, or
/// a recording of the escort's first character.
pub fn resolve_points(points: Vec<SpawnPoint>, owner_point: Option<&SpawnPoint>) -> (Vec<SpawnPoint>, bool) {
    if !points.is_empty() {
        return (points, false);
    }
    match owner_point {
        Some(p) => (vec![p.clone()], true),
        None => (Vec::new(), false),
    }
}

/// The spawner's owner point from its host node in the data walk: the Instance's name, translation (row 3)
/// and facing (row 1). [assumed] the owner is the Instance the spawner object sits in (the nearest matrix above
/// it in the data walk). The walk misses 13 escort spawners, so `mission_run` also gives the owner from the
/// script's parent (see [`SpawnerData::give_owner_point`]).
fn owner_point(host: Node) -> Option<SpawnPoint> {
    let (hash, name) = name_of(host);
    let m = host.get("matrix")?.floats();
    Some(SpawnPoint { name, hash, place: matrix_row(&m, 3)?, forward: matrix_row(&m, 1)? })
}

/// A point from an Instance's name and its matrix (12 or 16 values, row 3 the translation, row 1 the facing).
/// [assumed] the matrix layout (`matrix_row`). Used by the mission loader for a spawner's owner.
pub fn owner_point_from_matrix(name: &str, matrix: &[f32]) -> Option<SpawnPoint> {
    let hash = crc32(name.as_bytes());
    Some(SpawnPoint { name: name.to_owned(), hash, place: matrix_row(matrix, 3)?, forward: matrix_row(matrix, 1)? })
}

impl SpawnerData {
    /// Gives every record that has no spawn points the one owner point (see [`resolve_points`]), and fills
    /// `place` from it when the walk found no Instance. [game] prepare `007380f0` lines 132-161. [assumed] the
    /// owner is the spawner's parent Instance (the caller passes its point; `mission_run::owner_point_of`).
    pub fn give_owner_point(&mut self, owner: Option<SpawnPoint>) {
        for rec in &mut self.records {
            // A record that already has points (its own, or the walk's owner point) is left as it is.
            if rec.points.is_empty() {
                let (points, is_owner) = resolve_points(Vec::new(), owner.as_ref());
                rec.points = points;
                rec.points_are_owner = is_owner;
            }
        }
        if self.place.is_none() {
            self.place = owner.map(|o| o.place);
        }
    }
}

/// Row `r` (first three values) of a 4-row matrix stored as 12 values (4 rows of 3) or 16 (4 of 4). [assumed] layout.
fn matrix_row(m: &[f32], r: usize) -> Option<[f32; 3]> {
    if m.len() != 12 && m.len() != 16 {
        return None;
    }
    let at = r * (m.len() / 4);
    Some([m[at], m[at + 1], m[at + 2]])
}

/// The spawner object at `node`. `host` is the nearest Instance above it that has a matrix (its place).
fn read_spawner(node: Node, host: Option<Node>) -> SpawnerData {
    let (hash, name) = name_of(node);
    let owner = if name.is_empty() { SPAWNER_TYPE.to_owned() } else { name.clone() };
    let owner = owner.as_str();
    let mut r = Reader::default();
    r.expect_fields(node, owner, SPAWNER_FIELDS);
    let lux_script_name = r.hash(node, owner, "LuxScriptName");
    let accept_null_trigger_messages = r.flag(node, owner, "acceptNullTriggerMessages");
    let start_enabled = r.flag(node, owner, "startEnabled");
    let spawn_heuristic = r.hash(node, owner, "spawnHeuristic");
    let spawn_delay = r.float(node, owner, "spawnDelay");
    let spawn_out_of_view = r.flag(node, owner, "spawnOutOfView");
    let max_enemies_at_once = r.int(node, owner, "maxEnemiesAtOnce");
    let max_enemies_lifetime = r.int(node, owner, "maxEnemiesLifetime");
    let max_enemy_delay = r.float(node, owner, "maxEnemyDelay");
    let min_spawn_range = r.float(node, owner, "minSpawnRange");
    let max_spawn_range = r.float(node, owner, "maxSpawnRange");
    let delete_all_on_untrigger = r.flag(node, owner, "deleteAllCharsOnUnTrigger");
    let reset_on_untrigger = r.flag(node, owner, "resetOnUnTrigger");
    let msg_delete_all = r.hash(node, owner, "msgDeleteAll");
    let use_deferred_spawning = r.flag(node, owner, "useDeferredSpawning");
    let trig_done = r.triggers(node, owner, "trigDone");
    let trig_on_spawn = r.triggers(node, owner, "trigOnSpawn");
    let trig_on_death = r.triggers(node, owner, "trigOnDeath");
    // The owner: the spawner's host Instance, found by the walk (`walk`). [assumed] see `owner_point`.
    let owner_pt = host.and_then(owner_point);
    let place = owner_pt.as_ref().map(|p| p.place);
    let mut records = Vec::new();
    for (i, rec) in r.list(node, owner, "characterSpawnInfo").into_iter().enumerate() {
        let at = format!("{owner}.characterSpawnInfo[{i}]");
        records.push(r.record(rec, &at, owner_pt.as_ref()));
    }
    SpawnerData {
        identity: Identity::default(),
        name,
        hash,
        place,
        lux_script_name,
        accept_null_trigger_messages,
        start_enabled,
        spawn_heuristic,
        spawn_delay,
        spawn_out_of_view,
        max_enemies_at_once,
        max_enemies_lifetime,
        max_enemy_delay,
        min_spawn_range,
        max_spawn_range,
        delete_all_on_untrigger,
        reset_on_untrigger,
        msg_delete_all,
        use_deferred_spawning,
        trig_done,
        trig_on_spawn,
        trig_on_death,
        records,
        missing: r.missing,
    }
}

/// Walks a data file and reads every CharacterSpawnManager in it, in the file's order. An object
/// reached by two paths is read once. Spawners are not walked into. The identity's chunk is 0 here;
/// [`load`] sets the chunk of each data chunk it reads.
pub fn read_spawners(file: &DataFile) -> Vec<SpawnerData> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    walk(file.root(), None, None, &mut out, &mut seen, 0);
    out
}

/// `enclosing` is the nearest typed node above `node` in the walk: a spawner's parent.
fn walk<'a>(
    node: Node<'a>,
    host: Option<Node<'a>>,
    enclosing: Option<Node<'a>>,
    out: &mut Vec<SpawnerData>,
    seen: &mut HashSet<(usize, Option<&'a str>)>,
    depth: u32,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let kind = node.type_name();
    if let Some(at) = node.offset() {
        if !seen.insert((at, kind)) {
            return;
        }
    }
    if kind == Some(SPAWNER_TYPE) {
        let mut data = read_spawner(node, host);
        data.identity = Identity {
            chunk: 0,
            offset: node.offset(),
            parent_offset: enclosing.and_then(Node::offset),
            parent_name: enclosing.and_then(Node::label).unwrap_or(0),
        };
        out.push(data);
        return;
    }
    let fields = node.fields();
    let matrix_key = crc32(b"matrix");
    let host = if fields.iter().any(|(hash, _)| *hash == matrix_key) { Some(node) } else { host };
    // A typed node becomes the enclosing node of what is under it.
    let enclosing = if kind.is_some() { Some(node) } else { enclosing };
    for (_, child) in fields {
        walk(child, host, enclosing, out, seen, depth + 1);
    }
    for item in node.items() {
        walk(item, host, enclosing, out, seen, depth + 1);
    }
}

/// Opens a mission pack and returns the spawners of every data chunk, in chunk order and then the
/// walk's order. Each chunk's spawners carry the chunk's index in their [`Identity`]. A chunk that
/// does not parse as a data object is skipped (the other chunks are still read). [data] the
/// escort's 43 spawners are in chunk 8; the hunt's 18 and the tutorial's 19 were in chunk 0.
pub fn load(path: &Path) -> Result<Vec<SpawnerData>, String> {
    let pack = Pack::open(path)?;
    let mut out = Vec::new();
    for chunk in pack.chunks.iter().filter(|c| c.tag == pack::DATA) {
        let Ok(file) = DataFile::parse(pack.data(chunk).to_vec()) else { continue };
        for mut spawner in read_spawners(&file) {
            spawner.identity.chunk = chunk.index;
            out.push(spawner);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// The runtime. A spawner ticks, takes messages, and answers with actions.
// ---------------------------------------------------------------------------------------------

/// Which of the spawner's three trigger arrays a send comes from. [game] the three arrays are
/// trigOnDeath, trigOnSpawn and trigDone (`notes\ai-spawning.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnArray {
    /// trigOnDeath: sent once for each character that died (since the last update).
    Death,
    /// trigOnSpawn: sent after each spawn.
    Spawn,
    /// trigDone: sent once, when the lifetime is used up and no character is alive.
    Done,
}

/// What a spawner asks for. The caller acts on it.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Place a character from `records[record].points[point]`. `id` is the character's id.
    Spawn { record: usize, point: usize, id: u32 },
    /// Send the entry `entry` of the spawner's array `array`, whose message is `msg` (after the
    /// array's override). The target is not routed here: the caller finds the link at that position
    /// in the script (see `mission_triggers`).
    Send { array: SpawnArray, entry: usize, msg: u32 },
    /// Delete all of this spawner's characters (key B, or Untrigger with deleteAllCharsOnUnTrigger).
    DeleteAll,
}

/// What the spawner asks of the world. Implemented by the game.
pub trait SpawnWorld {
    /// Whether the character with this id still exists and is alive. [game] the death test is
    /// the character's own flags (`notes\ai-spawning.md` section 3); the caller maps it here (stand-in).
    fn is_alive(&self, id: u32) -> bool;
    /// The ground under `p` (a sweep down), or `None` when there is none. No hit, no spawn. [game] `00737720`.
    fn ground_at(&self, p: [f32; 3]) -> Option<[f32; 3]>;
    /// The reference point for the nearest, farthest and out-of-view tests. [assumed] the player.
    fn reference_point(&self) -> [f32; 3];
    /// Whether a straight navigation corridor runs from `from` to `to`. [game] `nav_straight_corridor_test` (006f0950).
    fn straight_corridor(&self, from: [f32; 3], to: [f32; 3]) -> bool;
}

/// A spawner's run-time state (the fields of the object `obj` in `notes\ai-spawning.md`).
#[derive(Clone, Debug)]
pub struct Spawner {
    pub data: SpawnerData,
    /// obj+0x5c. Set by `start`, Enable and any non-Disable message; cleared by Disable and done.
    pub enabled: bool,
    /// The living characters' ids (the ring at obj+0x4c).
    pub alive: Vec<u32>,
    /// Characters spawned so far (obj+0x7c).
    pub total: u32,
    /// obj+0x68: the spawn delay timer, seconds.
    pub spawn_timer: f32,
    /// obj+0x84: the alive-limit delay timer, seconds.
    pub max_delay_timer: f32,
    /// The sequential cursor (obj+0x48). Here it indexes the flat list of (record, point) pairs. [stand-in]
    pub cursor: usize,
    /// obj+0x90: the done routine has run.
    pub done: bool,
}

impl Spawner {
    pub fn new(data: SpawnerData) -> Self {
        Self { data, enabled: false, alive: Vec::new(), total: 0, spawn_timer: 0.0, max_delay_timer: 0.0, cursor: 0, done: false }
    }

    /// Start: enabled = startEnabled. [game] `0073ac40`. The caller prepares the characters (`007380f0`, stand-in).
    pub fn start(&mut self) {
        self.enabled = self.data.start_enabled;
    }

    /// Key A: a message addressed to this spawner (the caller has matched its target name, stand-in).
    /// Disable runs the done routine; Untrigger deletes and resets if the flags say so, then disables;
    /// every other hash (Enable, Trigger, TRIGGER, UnTrigger and the rest) enables. [game] `0073ca60`.
    pub fn message(&mut self, hash: u32) -> Vec<Action> {
        if hash == MSG_DISABLE {
            self.enabled = false;
            return Vec::new();
        }
        if hash == MSG_UNTRIGGER {
            let mut out = Vec::new();
            if self.data.delete_all_on_untrigger {
                // [unknown] whether the living ring is also cleared here; it is not, so the next update reports the deaths.
                out.push(Action::DeleteAll);
            }
            if self.data.reset_on_untrigger {
                self.alive.clear();
                self.total = 0;
                self.spawn_timer = 0.0;
                self.max_delay_timer = 0.0;
                self.done = false;
            }
            self.enabled = false;
            return out;
        }
        self.enabled = true;
        Vec::new()
    }

    /// Key B: a message that only deletes all characters when it is `msgDeleteAll`. [game] `0073bf60`.
    /// Any other hash is forwarded to the characters (not modelled here, stand-in). A msgDeleteAll of 0 means none.
    pub fn message_key_b(&mut self, hash: u32) -> Vec<Action> {
        if self.data.msg_delete_all != 0 && hash == self.data.msg_delete_all {
            vec![Action::DeleteAll]
        } else {
            Vec::new()
        }
    }

    /// One tick of `dt` seconds. [game] `0073af20`, steps in order: enabled gate, timers, deaths
    /// (trigOnDeath each), spawn (one at most), done (trigDone once, then disabled).
    pub fn update(&mut self, dt: f32, world: &dyn SpawnWorld, rng: &mut u32) -> Vec<Action> {
        let mut out = Vec::new();
        if !self.enabled {
            return out;
        }
        self.spawn_timer = (self.spawn_timer - dt).max(0.0);
        self.max_delay_timer = (self.max_delay_timer - dt).max(0.0);

        let before = self.alive.len();
        self.alive.retain(|&id| world.is_alive(id));
        for _ in self.alive.len()..before {
            self.send(&mut out, SpawnArray::Death, &self.data.trig_on_death);
        }

        // The deferred-spawning gate (DAT_00cf5c5f, useDeferredSpawning) is treated as open. [stand-in]
        if (self.alive.len() as i64) < self.data.max_enemies_at_once {
            if self.spawn_timer <= 0.0 && self.max_delay_timer <= 0.0 && (self.total as i64) < self.data.max_enemies_lifetime {
                if let Some(spawn) = self.spawn_one(world, rng) {
                    out.push(spawn);
                    self.send(&mut out, SpawnArray::Spawn, &self.data.trig_on_spawn);
                }
            }
        } else {
            self.max_delay_timer = self.data.max_enemy_delay;
        }

        if !self.done && (self.total as i64) >= self.data.max_enemies_lifetime && self.alive.is_empty() {
            self.done = true;
            self.send(&mut out, SpawnArray::Done, &self.data.trig_done);
            // The done routine: disabled. The per-record cleanup (0117f1a0) is not modelled. [stand-in]
            self.enabled = false;
        }
        out
    }

    /// Picks a point and, when the ground is there, makes the spawn: the id joins `alive`, the total
    /// rises, and the delay timer is set. [game] `0073b160` and record slot 3 `00738b50`.
    fn spawn_one(&mut self, world: &dyn SpawnWorld, rng: &mut u32) -> Option<Action> {
        let (flat, record, point) = self.pick(world, rng)?;
        let place = self.data.records[record].points[point].place;
        // No ground hit, no spawn. The point stays picked for the next tick. [stand-in] of the sweep 00737720.
        world.ground_at(place)?;
        if pick_of(self.data.spawn_heuristic) == Pick::Sequential {
            self.cursor = (flat + 1) % self.pair_count();
        }
        let id = self.next_id();
        self.alive.push(id);
        self.total += 1;
        self.spawn_timer = self.data.spawn_delay;
        Some(Action::Spawn { record, point, id })
    }

    /// The point choice. Returns (flat index, record, point). [game] `0073c010`, `00739220`, `007398b0`.
    fn pick(&self, world: &dyn SpawnWorld, rng: &mut u32) -> Option<(usize, usize, usize)> {
        let pairs = self.pairs();
        let n = pairs.len();
        if n == 0 {
            return None;
        }
        let reference = world.reference_point();
        let usable = |p: [f32; 3]| !self.data.spawn_out_of_view || !world.straight_corridor(reference, p);
        let place = |i: usize| {
            let (r, p) = pairs[i];
            self.data.records[r].points[p].place
        };
        match pick_of(self.data.spawn_heuristic) {
            Pick::Sequential => (0..n).map(|k| (self.cursor + k) % n).find(|&i| usable(place(i))).map(|i| (i, pairs[i].0, pairs[i].1)),
            Pick::Random => {
                // The game's LCG shuffles the order, then the first usable point wins. [assumed] over points, not only records.
                let mut order: Vec<usize> = (0..n).collect();
                for i in (1..n).rev() {
                    let j = lcg(rng) as usize % (i + 1);
                    order.swap(i, j);
                }
                order.into_iter().find(|&i| usable(place(i))).map(|i| (i, pairs[i].0, pairs[i].1))
            }
            pick @ (Pick::Nearest | Pick::Farthest) => {
                let band = HEUR_RANGE_ON.contains(&self.data.spawn_heuristic);
                let (lo, hi) = (self.data.min_spawn_range.powi(2), self.data.max_spawn_range.powi(2));
                let mut best: Option<(f32, usize)> = None;
                for i in 0..n {
                    let q = place(i);
                    if !usable(q) {
                        continue;
                    }
                    let d2 = dist2(q, reference);
                    if band && (d2 < lo || d2 > hi) {
                        continue;
                    }
                    let better = match best {
                        None => true,
                        Some((bd, _)) => {
                            if pick == Pick::Nearest {
                                d2 < bd
                            } else {
                                d2 > bd
                            }
                        }
                    };
                    if better {
                        best = Some((d2, i));
                    }
                }
                best.map(|(_, i)| (i, pairs[i].0, pairs[i].1))
            }
        }
    }

    /// Every (record, point) pair in the data's order.
    fn pairs(&self) -> Vec<(usize, usize)> {
        self.data.records.iter().enumerate().flat_map(|(r, rec)| (0..rec.points.len()).map(move |p| (r, p))).collect()
    }

    /// The spawn points over all records (zero: the spawner can never pick one). [data] the count is the mission's list.
    pub fn point_count(&self) -> usize {
        self.data.records.iter().map(|r| r.points.len()).sum()
    }

    fn pair_count(&self) -> usize {
        self.data.records.iter().map(|r| r.points.len()).sum::<usize>().max(1)
    }

    /// A fresh logical identity across all spawners. [stand-in] for an object handle;
    /// exhaustion is explicit instead of wrapping into a live or reserved id.
    fn next_id(&mut self) -> u32 {
        NEXT_CHARACTER_ID.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            if next < 0x8000_0000 { Some(next + 1) } else { None }
        }).expect("spawned character identity space exhausted")
    }

    /// Queues the sends of one of the spawner's arrays, entry by entry. [assumed] an `overrideMsgType`
    /// replaces each entry's msgType.
    fn send(&self, out: &mut Vec<Action>, which: SpawnArray, array: &TriggerArray) {
        for (entry, e) in array.entries.iter().enumerate() {
            out.push(Action::Send { array: which, entry, msg: array.override_msg_type.unwrap_or(e.msg_type) });
        }
    }
}

/// What a spawner would do at its first update after Enable (the `--spawners` summary line, stand-in for
/// the mission script's view). Read from the spawner's data and timers only, not from the world: a point
/// with no ground, or no straight corridor for an out-of-view spawner, is found later by `update`.
#[derive(Clone, Debug, PartialEq)]
pub enum EnableOutcome {
    /// The first update spawns (no timer, a point, lifetime left, a definition with a pack).
    WillSpawn,
    /// A timer still runs (spawn delay or alive-limit delay). [data] the timers start at 0, so this is 0 at Enable.
    Waiting,
    /// No spawn point in any record: `pick` returns nothing, every update, silently.
    NoPoint,
    /// The first record's characterDefinition has no pack here: the spawn is skipped (the reason).
    Skipped(&'static str),
    /// The lifetime is already used up (lifetime 0): the done routine runs at the first update.
    Done,
}

/// The outcome of `s` at Enable. [assumed] the record that is checked is the first one (every mission
/// spawner in the install has one record; `records.len()` is in the `--spawners` line).
pub fn enable_outcome(s: &Spawner) -> EnableOutcome {
    let def_hash = s.data.records.first().and_then(|r| r.definition.as_ref()).map_or(0, |d| d.hash);
    if character_inputs(def_hash).is_none() {
        return EnableOutcome::Skipped(skip_reason(def_hash));
    }
    if s.point_count() == 0 {
        return EnableOutcome::NoPoint;
    }
    if (s.total as i64) >= s.data.max_enemies_lifetime {
        return EnableOutcome::Done;
    }
    if s.spawn_timer > 0.0 || s.max_delay_timer > 0.0 {
        return EnableOutcome::Waiting;
    }
    EnableOutcome::WillSpawn
}

/// The game's LCG (multiplier 0x343fd, increment 0x269ec3), output as the high 15 bits. [assumed] output bits.
fn lcg(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(0x343fd).wrapping_add(0x269_ec3);
    (*state >> 16) & 0x7fff
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A test world: ids alive, ground everywhere or nowhere, a fixed reference, and the points that
    /// have a straight corridor from the reference.
    struct World {
        alive: Vec<u32>,
        ground: bool,
        reference: [f32; 3],
        corridor: Vec<[f32; 3]>,
    }

    impl World {
        fn new() -> Self {
            Self { alive: Vec::new(), ground: true, reference: [0.0; 3], corridor: Vec::new() }
        }
    }

    impl SpawnWorld for World {
        fn is_alive(&self, id: u32) -> bool {
            self.alive.contains(&id)
        }
        fn ground_at(&self, p: [f32; 3]) -> Option<[f32; 3]> {
            self.ground.then_some(p)
        }
        fn reference_point(&self) -> [f32; 3] {
            self.reference
        }
        fn straight_corridor(&self, _from: [f32; 3], to: [f32; 3]) -> bool {
            self.corridor.contains(&to)
        }
    }

    fn point(x: f32, y: f32) -> SpawnPoint {
        SpawnPoint { name: format!("p{x}_{y}"), hash: 0, place: [x, y, 0.0], forward: [0.0, 1.0, 0.0] }
    }

    fn send_to(msg: u32) -> TriggerArray {
        TriggerArray { override_msg_type: None, override_filter: None, entries: vec![TriggerEntry { target: 0, msg_type: msg, filter: None, delay_min: 0.0, delay_max: 0.0 }] }
    }

    const DEATH: u32 = 0x1111_0001;
    const SPAWNED: u32 = 0x1111_0002;
    const DONE: u32 = 0x1111_0003;

    fn spawner(heuristic: u32, points: Vec<SpawnPoint>) -> SpawnerData {
        SpawnerData {
            identity: Identity::default(),
            name: "test".into(),
            hash: 0,
            place: None,
            lux_script_name: 0x1234,
            accept_null_trigger_messages: false,
            start_enabled: true,
            spawn_heuristic: heuristic,
            spawn_delay: 0.0,
            spawn_out_of_view: false,
            max_enemies_at_once: 1,
            max_enemies_lifetime: 1,
            max_enemy_delay: 2.0,
            min_spawn_range: 0.0,
            max_spawn_range: 2000.0,
            delete_all_on_untrigger: false,
            reset_on_untrigger: false,
            msg_delete_all: 0,
            use_deferred_spawning: false,
            trig_done: send_to(DONE),
            trig_on_spawn: send_to(SPAWNED),
            trig_on_death: send_to(DEATH),
            records: vec![SpawnRecord {
                info_name: 0,
                definition: None,
                points,
                points_are_owner: false,
                use_spawn_point_as_home: false,
                group_id: 0,
                home_regions: Vec::new(),
                messages_on_spawn: Vec::new(),
            }],
            missing: Vec::new(),
        }
    }

    /// Runs one update and marks each spawned character alive in the world. Returns the actions.
    fn tick(s: &mut Spawner, w: &mut World, dt: f32, rng: &mut u32) -> Vec<Action> {
        let out = s.update(dt, &*w, rng);
        for a in &out {
            if let Action::Spawn { id, .. } = a {
                w.alive.push(*id);
            }
        }
        out
    }

    fn spawns(actions: &[Action]) -> Vec<(usize, usize)> {
        actions.iter().filter_map(|a| if let Action::Spawn { record, point, .. } = a { Some((*record, *point)) } else { None }).collect()
    }

    #[test]
    fn message_hashes_match_names() {
        // [game] the handler's immediates are the CRC-32 of these names; the second read's check.
        assert_eq!(crc32(b"Enable"), MSG_ENABLE);
        assert_eq!(crc32(b"Disable"), MSG_DISABLE);
        assert_eq!(crc32(b"Trigger"), MSG_TRIGGER);
        assert_eq!(crc32(b"Untrigger"), MSG_UNTRIGGER, "Untrigger: the note says the lower-case spelling; check which one matches");
    }

    #[test]
    fn no_override_sentinels_hash_their_names() {
        assert_eq!(crc32(b"NOOVERRIDE"), OVERRIDE_NONE[1], "the tutorial's NOOVERRIDE enum");
    }

    #[test]
    fn spawn_delay_gates_the_next_spawn() {
        let mut data = spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0), point(1.0, 0.0)]);
        data.max_enemies_at_once = 2;
        data.max_enemies_lifetime = 5;
        data.spawn_delay = 0.5;
        let mut s = Spawner::new(data);
        s.start();
        let (mut w, mut rng) = (World::new(), 7u32);
        assert_eq!(spawns(&tick(&mut s, &mut w, 0.0, &mut rng)), vec![(0, 0)]);
        assert!(spawns(&tick(&mut s, &mut w, 0.3, &mut rng)).is_empty(), "0.2 s left");
        assert_eq!(spawns(&tick(&mut s, &mut w, 0.3, &mut rng)), vec![(0, 1)], "delay has run out");
    }

    #[test]
    fn alive_limit_sets_the_max_delay_timer() {
        let mut data = spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0), point(1.0, 0.0)]);
        data.max_enemies_lifetime = 5;
        let mut s = Spawner::new(data);
        s.start();
        let (mut w, mut rng) = (World::new(), 7u32);
        assert_eq!(spawns(&tick(&mut s, &mut w, 0.0, &mut rng)).len(), 1);
        assert!(spawns(&tick(&mut s, &mut w, 0.0, &mut rng)).is_empty(), "at the alive limit");
        assert_eq!(s.max_delay_timer, 2.0, "maxEnemyDelay is loaded while at the limit");
    }

    #[test]
    fn death_frees_a_slot_and_sends_trigger_on_death() {
        let mut data = spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0), point(1.0, 0.0)]);
        data.max_enemies_lifetime = 5;
        let mut s = Spawner::new(data);
        s.start();
        let (mut w, mut rng) = (World::new(), 7u32);
        tick(&mut s, &mut w, 0.0, &mut rng);
        let id = s.alive[0];
        w.alive.clear();
        let out = tick(&mut s, &mut w, 0.0, &mut rng);
        assert!(s.alive.is_empty() || !s.alive.contains(&id), "the dead id leaves the ring");
        assert!(out.contains(&Action::Send { array: SpawnArray::Death, entry: 0, msg: DEATH }), "trigOnDeath is sent: {out:?}");
    }

    #[test]
    fn unnamed_spawners_keep_independent_lifetimes() {
        // Missing names, identical names, and names sharing their low 16 bits must
        // not give two living characters the same identity.
        for names in [[0, 0], [0x1234, 0x1234], [0xaaaa_1234, 0xbbbb_1234]] {
            let mut a_data = spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0)]);
            let mut b_data = spawner(HEUR_SEQUENTIAL, vec![point(1.0, 0.0)]);
            a_data.lux_script_name = names[0];
            b_data.lux_script_name = names[1];
            let (mut a, mut b) = (Spawner::new(a_data), Spawner::new(b_data));
            a.start();
            b.start();
            let (mut world, mut rng) = (World::new(), 7);
            tick(&mut a, &mut world, 0.0, &mut rng);
            tick(&mut b, &mut world, 0.0, &mut rng);
            let (a_id, b_id) = (a.alive[0], b.alive[0]);
            assert_ne!(a_id, b_id, "characters from script names {names:x?} must have independent ids");
            world.alive.retain(|&id| id == a_id);
            let a_actions = a.update(0.0, &world, &mut rng);
            let b_actions = b.update(0.0, &world, &mut rng);
            assert_eq!(a.alive, vec![a_id]);
            assert!(!a.done);
            assert!(!a_actions.iter().any(|a| matches!(a, Action::Send { array: SpawnArray::Death, .. })));
            assert!(b.alive.is_empty());
            assert!(b.done);
            assert!(b_actions.contains(&Action::Send { array: SpawnArray::Death, entry: 0, msg: DEATH }));
            assert!(b_actions.contains(&Action::Send { array: SpawnArray::Done, entry: 0, msg: DONE }));
        }
    }

    #[test]
    fn character_ids_do_not_repeat_after_65536_allocations() {
        let mut s = Spawner::new(spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0)]));
        let mut ids = HashSet::new();
        for _ in 0..65_537 {
            assert!(ids.insert(s.next_id()), "a reused id would alias an earlier character");
        }
        assert!(!ids.contains(&0) && !ids.contains(&1), "null and the mission player are reserved");
    }

    #[test]
    fn lifetime_exhaustion_gives_done_and_trig_done_once() {
        let mut s = Spawner::new(spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0)]));
        s.start();
        let (mut w, mut rng) = (World::new(), 7u32);
        assert_eq!(spawns(&tick(&mut s, &mut w, 0.0, &mut rng)), vec![(0, 0)]);
        let done = Action::Send { array: SpawnArray::Done, entry: 0, msg: DONE };
        assert!(!tick(&mut s, &mut w, 0.0, &mut rng).contains(&done), "one still alive: not done");
        w.alive.clear();
        let out = tick(&mut s, &mut w, 0.0, &mut rng);
        assert_eq!(out.iter().filter(|a| **a == done).count(), 1, "trigDone once: {out:?}");
        assert!(s.done);
        assert!(!s.enabled, "the done routine disables");
        // Enable again: done is not repeated, and no spawn is left.
        s.message(MSG_ENABLE);
        let out = tick(&mut s, &mut w, 0.0, &mut rng);
        assert!(out.is_empty(), "nothing more after done: {out:?}");
    }

    #[test]
    fn each_key_a_message() {
        let mut s = Spawner::new(spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0)]));
        assert!(s.message(MSG_DISABLE).is_empty());
        assert!(!s.enabled, "Disable");
        assert!(s.message(MSG_ENABLE).is_empty());
        assert!(s.enabled, "Enable");
        s.message(MSG_DISABLE);
        s.message(MSG_TRIGGER);
        assert!(s.enabled, "Trigger enables");
        s.message(MSG_DISABLE);
        s.message(MSG_TRIGGER_CAPS);
        assert!(s.enabled, "TRIGGER enables");
        s.message(MSG_DISABLE);
        s.message(0x0bad_f00d);
        assert!(s.enabled, "any other hash enables");

        // Untrigger: no flags, so just disabled and nothing else kept.
        s.alive.push(9);
        s.total = 3;
        assert!(s.message(MSG_UNTRIGGER).is_empty());
        assert!(!s.enabled && s.alive == [9] && s.total == 3, "no flags: disabled, state kept");

        // Untrigger with delete-all and reset.
        s.data.delete_all_on_untrigger = true;
        s.data.reset_on_untrigger = true;
        s.enabled = true;
        s.done = true;
        assert_eq!(s.message(MSG_UNTRIGGER), vec![Action::DeleteAll]);
        assert!(!s.enabled && s.alive.is_empty() && s.total == 0 && !s.done, "reset clears and disables");
    }

    #[test]
    fn key_b_deletes_only_on_its_own_hash() {
        let mut data = spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0)]);
        data.msg_delete_all = 0xabcd_ef01;
        let mut s = Spawner::new(data);
        assert_eq!(s.message_key_b(0xabcd_ef01), vec![Action::DeleteAll]);
        assert!(s.message_key_b(0x0000_0001).is_empty());
    }

    #[test]
    fn sequential_walks_the_points_in_order() {
        let mut data = spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0), point(10.0, 0.0), point(20.0, 0.0)]);
        data.max_enemies_at_once = 3;
        data.max_enemies_lifetime = 3;
        let mut s = Spawner::new(data);
        s.start();
        let (mut w, mut rng) = (World::new(), 7u32);
        let mut got = Vec::new();
        for _ in 0..3 {
            got.extend(spawns(&tick(&mut s, &mut w, 0.0, &mut rng)));
        }
        assert_eq!(got, vec![(0, 0), (0, 1), (0, 2)]);
    }

    #[test]
    fn nearest_and_farthest_with_the_band() {
        let points = vec![point(10.0, 0.0), point(5.0, 0.0), point(20.0, 0.0)];
        let mut rng = 7u32;
        let w = World::new();

        let s = Spawner::new(spawner(HEUR_NEAREST, points.clone()));
        assert_eq!(s.pick(&w, &mut rng).map(|p| p.2), Some(1), "nearest is the point at 5");

        let mut banded = spawner(HEUR_NEAREST, points.clone());
        banded.min_spawn_range = 6.0;
        let s = Spawner::new(banded);
        assert_eq!(s.pick(&w, &mut rng).map(|p| p.2), Some(0), "the 5 m point is outside the band");

        // [game] d7ee931a keeps the FARTHEST point (the compare at 0073c7b5 with bVar5 clear).
        let s = Spawner::new(spawner(HEUR_FARTHEST_TARGET, points.clone()));
        assert_eq!(s.pick(&w, &mut rng).map(|p| p.2), Some(2), "d7ee931a: farthest is the point at 20");

        // [game] 920e4a62 keeps the NEAREST point (bVar5 set: the best starts at FLT_MAX).
        let s = Spawner::new(spawner(HEUR_NEAREST_SET[0], points.clone()));
        assert_eq!(s.pick(&w, &mut rng).map(|p| p.2), Some(1), "920e4a62: nearest is the point at 5");
        assert!(!heuristic_name(HEUR_NEAREST_SET[0]).contains("farthest"));
        assert_eq!(pick_of(HEUR_FARTHEST_TARGET), Pick::Farthest);
        assert_eq!(pick_of(HEUR_NEAREST_SET[0]), Pick::Nearest);
        assert_eq!(pick_of(HEUR_NEAREST_SET[1]), Pick::Nearest);
    }

    #[test]
    fn an_empty_spawnpoints_record_gets_one_owner_point() {
        // [game] prepare 007380f0 lines 132-161: one point, the owner's, and nothing else.
        let owner = SpawnPoint { name: "Owner".into(), hash: 7, place: [-200.94, 40.68, 28.0], forward: [0.0, 1.0, 0.0] };
        let (pts, is_owner) = resolve_points(Vec::new(), Some(&owner));
        assert_eq!(pts, vec![owner.clone()], "exactly one point, at the owner");
        assert!(is_owner);
        // With spawnPoints the owner is not added.
        let (pts, is_owner) = resolve_points(vec![point(1.0, 2.0)], Some(&owner));
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].place, [1.0, 2.0, 0.0]);
        assert!(!is_owner);
        // No owner Instance: no point (the record cannot spawn).
        let (pts, is_owner) = resolve_points(Vec::new(), None);
        assert!(pts.is_empty() && !is_owner);
    }

    #[test]
    fn a_spawner_with_only_the_owner_point_spawns_at_the_owner() {
        let owner = SpawnPoint { name: "Owner".into(), hash: 7, place: [5.0, 6.0, 0.0], forward: [0.0, 1.0, 0.0] };
        let (pts, is_owner) = resolve_points(Vec::new(), Some(&owner));
        let mut data = spawner(HEUR_FARTHEST_TARGET, pts);
        data.records[0].points_are_owner = is_owner;
        let mut s = Spawner::new(data);
        s.start();
        let (mut w, mut rng) = (World::new(), 7u32);
        let got = tick(&mut s, &mut w, 0.0, &mut rng);
        assert_eq!(spawns(&got), vec![(0, 0)], "one point, so one spawn there");
    }

    #[test]
    fn out_of_view_needs_no_straight_corridor() {
        let points = vec![point(0.0, 5.0), point(0.0, 9.0)];
        let mut data = spawner(HEUR_SEQUENTIAL, points.clone());
        data.spawn_out_of_view = true;
        let s = Spawner::new(data);
        let mut w = World::new();
        w.corridor = vec![points[0].place];
        let mut rng = 7u32;
        assert_eq!(s.pick(&w, &mut rng).map(|p| p.2), Some(1), "point 0 has a corridor, so it is not usable");
        w.corridor.clear();
        assert_eq!(s.pick(&w, &mut rng).map(|p| p.2), Some(0), "no corridor on either: the first is usable");
    }

    #[test]
    fn random_is_deterministic_for_a_seed_and_varies() {
        let points: Vec<SpawnPoint> = (0..8).map(|i| point(i as f32, 0.0)).collect();
        let s = Spawner::new(spawner(HEUR_RANDOM, points));
        let w = World::new();
        let (mut a, mut b) = (5u32, 5u32);
        assert_eq!(s.pick(&w, &mut a), s.pick(&w, &mut b));
        assert_eq!(a, b, "the same seed leaves the same state");
        let mut seen = HashSet::new();
        for seed in 1..40u32 {
            let mut r = seed;
            seen.insert(s.pick(&w, &mut r).map(|p| p.2));
        }
        assert!(seen.len() > 2, "different seeds give different points: {seen:?}");
    }

    #[test]
    fn unread_heuristic_falls_back_to_sequential() {
        assert!(!heuristic_known(HEUR_UNREAD[0]));
        let mut s = Spawner::new(spawner(HEUR_UNREAD[0], vec![point(0.0, 0.0), point(1.0, 0.0)]));
        let w = World::new();
        let mut rng = 1u32;
        assert_eq!(s.pick(&w, &mut rng).map(|p| p.2), Some(0));
        s.cursor = 1;
        assert_eq!(s.pick(&w, &mut rng).map(|p| p.2), Some(1), "the cursor is the sequential one");
    }

    #[test]
    fn no_ground_no_spawn() {
        let mut s = Spawner::new(spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0)]));
        s.start();
        let mut w = World::new();
        w.ground = false;
        let mut rng = 1u32;
        assert!(s.update(0.0, &w, &mut rng).is_empty());
        assert_eq!(s.total, 0);
    }

    fn install_dir() -> Option<PathBuf> {
        let dir = PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into()));
        dir.join("levels").is_dir().then_some(dir)
    }

    #[test]
    fn install_missions_read_without_error() {
        let Some(dir) = install_dir() else { return };
        let tutorial = load(&dir.join("levels/us_west_town/Missions/Tutorial_Autobot/mission.str")).expect("tutorial mission");
        let hunt = load(&dir.join("levels/us_test/missions/usa_east_ltrain_d_hunt/mission.str")).expect("hunt mission");
        assert_eq!(tutorial.len(), 19, "tutorial spawner count (ai-level-data.md A2)");
        // The notes say 14 (ai-level-data.md A6), from a decode cut at depth 64. At depth 200 the
        // Python decoder finds 18 too, so 18 is the count (the notes' 14 is a depth cut, [data]).
        assert_eq!(hunt.len(), 18, "hunt spawner count");
        assert!(hunt.iter().all(|s| !s.start_enabled && s.max_enemies_at_once == 1 && s.max_enemies_lifetime == 1));
    }

    #[test]
    fn install_escort_spawners_are_read_from_every_chunk() {
        let Some(dir) = install_dir() else { return };
        // [data] the escort's 43 CharacterSpawnManager objects are in data chunk 8 (the first
        // version read chunk 0 only and found none). mission_script's install test counts the same
        // 43 in chunk 8.
        let rel = "levels/us_test/missions/usa_east_ltrain_a_escort/mission.str";
        let escort = load(&dir.join(rel)).expect("escort mission");
        assert_eq!(escort.len(), 43, "escort spawner count");
        assert!(escort.iter().all(|s| s.identity.chunk == 8 && s.identity.offset.is_some()));
    }

    #[test]
    fn install_escort_spawners_get_their_points() {
        let Some(dir) = install_dir() else { return };
        // [data] (work/esc_raw.py, chunk 8): 41 of 43 escort spawners have an empty spawnPoints array;
        // 2 have spawnPoints (11 and 8 points). [game] prepare 007380f0 lines 132-161 gives each empty record
        // ONE point, the spawner's owner Instance. homePoints are not used. So every escort spawner that has
        // an owner Instance has one or more points, and the 41 are owner points.
        // [gap] `load` (the walk alone) finds the owner Instance for 28 of the 41 empty records. The other 13
        // sit under no Instance in the data walk, although the script gives each its parent Instance with a matrix:
        // `mission_run::owner_point_of` gives those 13 their point (install test `install_empty_spawnpoints_...`).
        // So the game's `--spawners` path (`load`, targets.rs) has 13 spawners with no point. [unknown] why the walk misses them.
        let escort = load(&dir.join("levels/us_test/missions/usa_east_ltrain_a_escort/mission.str")).expect("escort mission");
        let with_points = escort.iter().filter(|s| s.records.iter().any(|r| !r.points.is_empty())).count();
        let owner_points = escort.iter().filter(|s| s.records.iter().any(|r| r.points_are_owner)).count();
        let with_spawn_points = escort.iter().filter(|s| s.records.iter().any(|r| !r.points.is_empty() && !r.points_are_owner)).count();
        assert_eq!(with_spawn_points, 2, "escort spawners with their own spawnPoints");
        assert_eq!(owner_points, 28, "escort spawners whose one point is the owner's, found by the walk (of 41 empty records)");
        assert_eq!(with_points, 30, "spawners with a point from the walk (28 owner points and 2 spawnPoints)");
        // The hunt and the tutorial were noted as having spawnPoints (ai-spawn-characters.md 139). The walk finds
        // owner points in them too, so some of their records have an empty spawnPoints (the rule applies there).
        // Reported, not asserted: [unknown] how many, until the counts are checked against the data.
        for rel in ["levels/us_test/missions/usa_east_ltrain_d_hunt/mission.str", "levels/us_west_town/Missions/Tutorial_Autobot/mission.str"] {
            let other = load(&dir.join(rel)).expect("mission");
            let n = other.iter().filter(|s| s.records.iter().any(|r| r.points_are_owner)).count();
            eprintln!("{rel}: {n} of {} spawners have an owner point (empty spawnPoints, [game] rule)", other.len());
        }
    }

    #[test]
    fn character_inputs_table_matches_the_definition_names() {
        assert_eq!(crc32(b"CharDef_AProtScout"), DEF_APROT_SCOUT);
        assert_eq!(crc32(b"CharDef_AProtSniper"), DEF_APROT_SNIPER);
        assert_eq!(crc32(b"CharDef_AProtWarrior"), DEF_APROT_WARRIOR);
        assert_eq!(crc32(b"CharDef_Optimus"), DEF_OPTIMUS);
        assert_eq!(crc32(b"Fake_DCOM_DEF"), DEF_FAKE_DCOM);
        assert_eq!(crc32(b"Char_Def_Firestorm"), DEF_FIRESTORM);
        assert_eq!(crc32(b"Char_Def_Bumblebee"), DEF_BUMBLEBEE);
        assert_eq!(crc32(b"CharDef_DComScout"), DEF_DCOM_SCOUT);
        assert_eq!(crc32(b"CharDef_DComSniper"), DEF_DCOM_SNIPER);
        assert_eq!(crc32(b"CharDef_DComWarrior"), DEF_DCOM_WARRIOR);
        assert_eq!(crc32(b"CharDef_StunticonSniper"), DEF_STUNTICON_SNIPER);
    }

    #[test]
    fn character_inputs_maps_the_enemies_and_skips_the_rest() {
        assert_eq!(character_inputs(DEF_APROT_SCOUT), Some(("aprotscoutmp", "APrtScout")));
        assert_eq!(character_inputs(DEF_FAKE_DCOM), Some(("dcomwarrior", "DComWarrior")));
        // [data] Shanghai's DCom definitions: each pack and block exists in the install.
        assert_eq!(character_inputs(DEF_DCOM_SCOUT), Some(("dcomscout", "DComScout")));
        assert_eq!(character_inputs(DEF_DCOM_SNIPER), Some(("dcomsniper", "DComSniper")));
        assert_eq!(character_inputs(DEF_DCOM_WARRIOR), Some(("dcomwarrior", "DComWarrior")));
        // No pack in the install (AProtSniper, AProtWarrior, StunticonSniper), or the player's side (the rest).
        for skipped in [DEF_APROT_SNIPER, DEF_APROT_WARRIOR, DEF_STUNTICON_SNIPER, DEF_OPTIMUS, DEF_FIRESTORM, DEF_BUMBLEBEE, 0, 1] {
            assert_eq!(character_inputs(skipped), None, "{skipped:#010x}");
            assert!(!skip_reason(skipped).is_empty());
        }
        assert!(skip_reason(DEF_APROT_SNIPER).starts_with("no pack file"));
        assert!(skip_reason(DEF_STUNTICON_SNIPER).starts_with("no pack file"));
        assert!(skip_reason(DEF_OPTIMUS).starts_with("ally"));
        assert!(skip_reason(0).starts_with("not in the table"));
    }

    #[test]
    fn character_inputs_escort_variants_and_convoy() {
        assert_eq!(crc32(b"CharDef_DComScout_ShortRange"), DEF_DCOM_SCOUT_SHORT);
        assert_eq!(crc32(b"CharDef_DComScoutFAR"), DEF_DCOM_SCOUT_FAR);
        assert_eq!(crc32(b"CharDef_DComWarriorShortRng"), DEF_DCOM_WARRIOR_SHORT);
        assert_eq!(crc32(b"Convoy_Character_Definition"), DEF_CONVOY);
        // [stand-in: overrides not applied] the variants use their base pack and block.
        assert_eq!(character_inputs(DEF_DCOM_SCOUT_SHORT), Some(("dcomscout", "DComScout")));
        assert_eq!(character_inputs(DEF_DCOM_SCOUT_FAR), Some(("dcomscout", "DComScout")));
        assert_eq!(character_inputs(DEF_DCOM_WARRIOR_SHORT), Some(("dcomwarrior", "DComWarrior")));
        // The convoy truck is a vehicle with no pack: skipped with its reason.
        assert_eq!(character_inputs(DEF_CONVOY), None);
        assert!(skip_reason(DEF_CONVOY).starts_with("vehicle not buildable yet"));
    }

    #[test]
    fn escort_spawners_map_to_a_buildable_character_or_a_reason() {
        let Some(dir) = install_dir() else { return };
        let spawners = load(&dir.join("levels/us_test/missions/usa_east_ltrain_a_escort/mission.str")).expect("escort mission");
        assert_eq!(spawners.len(), 43, "escort spawner count (mission-escort-trace.md, Spawners)");
        let mut buildable = 0;
        let mut skipped: Vec<&str> = Vec::new();
        for s in &spawners {
            for r in &s.records {
                let Some(d) = &r.definition else {
                    skipped.push("no definition");
                    continue;
                };
                if character_inputs(d.hash).is_some() {
                    buildable += 1;
                } else {
                    skipped.push(skip_reason(d.hash));
                }
            }
        }
        // Buildable: DComScout 7, DComSniper 14, DComWarrior 8 (already in the table) + ShortRange 8, ShortRng 4, FAR 1.
        assert_eq!(buildable, 42, "escort spawners that map to a buildable character");
        // Skipped: the convoy's spawner only (the truck, no pack).
        assert_eq!(skipped.len(), 1, "escort spawners skipped: {skipped:?}");
        assert!(skipped[0].starts_with("vehicle not buildable yet"));
    }

    #[test]
    fn messages_on_spawn_read_from_the_hunt_install() {
        // Message names: names.tsv a55f44c1 = AlertedOn, e3c890fb = ImmuneToOverdrive (ai-spawn-characters.md 5).
        assert_eq!(crc32(b"AlertedOn"), 0xa55f_44c1);
        assert_eq!(crc32(b"ImmuneToOverdrive"), 0xe3c8_90fb);
        let Some(dir) = install_dir() else { return };
        let hunt = load(&dir.join("levels/us_test/missions/usa_east_ltrain_d_hunt/mission.str")).expect("hunt mission");
        let records: Vec<&SpawnRecord> = hunt.iter().flat_map(|s| s.records.iter()).collect();
        assert_eq!(records.len(), 18, "one record per hunt spawner");
        let optimus = records.iter().find(|r| r.definition.as_ref().is_some_and(|d| d.hash == DEF_OPTIMUS)).expect("the Optimus record");
        assert_eq!(optimus.messages_on_spawn, vec![crc32(b"AlertedOn"), crc32(b"ImmuneToOverdrive")], "Optimus: AlertedOn, ImmuneToOverdrive");
        let scout = records.iter().find(|r| r.definition.as_ref().is_some_and(|d| d.hash == DEF_APROT_SCOUT)).expect("an AProt scout record");
        assert_eq!(scout.messages_on_spawn, vec![crc32(b"AlertedOn")], "a scout: AlertedOn");
    }

    fn with_definition(mut data: SpawnerData, hash: u32) -> SpawnerData {
        data.records[0].definition = Some(Definition { hash, name: "CharDef".into(), decoded: true });
        data
    }

    #[test]
    fn point_count_sums_the_records_points() {
        let data = spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0), point(1.0, 0.0)]);
        assert_eq!(Spawner::new(data).point_count(), 2);
        assert_eq!(Spawner::new(spawner(HEUR_SEQUENTIAL, Vec::new())).point_count(), 0);
    }

    #[test]
    fn enable_outcome_will_spawn_for_a_loadable_spawner_with_points() {
        let s = Spawner::new(with_definition(spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0)]), DEF_APROT_SCOUT));
        assert_eq!(enable_outcome(&s), EnableOutcome::WillSpawn);
    }

    #[test]
    fn enable_outcome_no_point_is_its_own_reason() {
        // Shanghai_01's DComWarrior and DComScout spawners have no spawn point (the data's list is empty).
        let s = Spawner::new(with_definition(spawner(HEUR_RANDOM, Vec::new()), DEF_APROT_SCOUT));
        assert_eq!(enable_outcome(&s), EnableOutcome::NoPoint);
    }

    #[test]
    fn enable_outcome_skipped_gives_the_reason_of_the_definition() {
        let s = Spawner::new(with_definition(spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0)]), DEF_APROT_WARRIOR));
        assert_eq!(enable_outcome(&s), EnableOutcome::Skipped(skip_reason(DEF_APROT_WARRIOR)));
        // No definition at all is skipped too (hash 0 is not in the table).
        let none = Spawner::new(spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0)]));
        assert!(matches!(enable_outcome(&none), EnableOutcome::Skipped(_)));
    }

    #[test]
    fn enable_outcome_waiting_on_a_timer_and_done_on_lifetime_zero() {
        let mut s = Spawner::new(with_definition(spawner(HEUR_SEQUENTIAL, vec![point(0.0, 0.0)]), DEF_APROT_SCOUT));
        s.spawn_timer = 1.0;
        assert_eq!(enable_outcome(&s), EnableOutcome::Waiting);
        s.spawn_timer = 0.0;
        s.data.max_enemies_lifetime = 0;
        assert_eq!(enable_outcome(&s), EnableOutcome::Done);
    }
}
