//! A zone's or mission pack's script as data: its objects, the links between them, and their
//! fields. In this repository it is only read, for the stage reverb (`reverb::zone_triggers`):
//! the mission runtime that delivers the links' messages is not part of it.
//!
//! Read from the pack in place (the install found by `install`); nothing from the game is
//! bundled. The object walk and the link fields follow the game's data layout.
//!
//! Tags: [game] read in code, [data] read from a pack or a dump, [assumed] a guess, [stand-in]
//! a placeholder to replace.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use crate::formats::hash::crc32;
use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};

/// Deepest the object walk and the link scan go (mission data nests Instances inside Instances).
/// The python decoder stops at 64 and misses about 4,000 objects of this pack; 256 is the depth at
/// which both readers agree (see the install test).
const MAX_DEPTH: u32 = 256;

/// How deep the node index (`Script::node`) walks a chunk. The same depth the node finder always used
/// (`mission_state::find_nodes`, moved here with the index). [assumed] 256 is the escort's depth; see above.
pub(crate) const WALK_DEPTH: u32 = 256;

/// The type name of an embedded data file (its own object, with roots). [data]
const EMBEDDED_TYPE: &str = "(embedded data)";

/// The sentinel an override field holds when it overrides nothing (`0xfe74aaca`, the array header
/// test in `0080d9f0`, [game] the unpublished notes section 6). The pack also spells the
/// same value as the enum name `NOOVERRIDE` (crc `43c0d673`), which the reader maps to the same
/// meaning [assumed]. [data]
const NO_OVERRIDE: u32 = 0xfe74_aaca;
const NO_OVERRIDE_NAME: u32 = 0x43c0_d673;

// Message names: CRC-32 of the name. [game] immediates in the handlers; [data] the names in the
// pack. Each is checked against crc32 of its name by the test `message_names_are_crc32`.
/// `Enable` (the unpublished notes section 4).
pub const MSG_ENABLE: u32 = 0xf40c_cd04;
/// `Disable`.
pub const MSG_DISABLE: u32 = 0x80f8_74cd;
/// `Trigger` (the pack writes `#d5d636c1`). [data]
pub const MSG_TRIGGER: u32 = 0xd5d6_36c1;
/// `Untrigger` (the pack writes `#9caa723d`). [data]
pub const MSG_UNTRIGGER: u32 = 0x9caa_723d;
/// `Kill`. One handler, 007236c0.
pub const MSG_KILL: u32 = 0xa71b_f957;
/// `Show`. One handler, 008e9910 (unread).
pub const MSG_SHOW: u32 = 0x923c_763f;
/// `Activate`.
pub const MSG_ACTIVATE: u32 = 0x4f10_66fb;
/// `Deactivate`.
pub const MSG_DEACTIVATE: u32 = 0xb16b_d083;

/// The field names of a script object's acceptance. [data] crc `cff8719b` (`LuxScriptName`) and
/// `97308f4a` (`acceptNullTriggerMessages`), `work\mission_slice_graph.txt` names.tsv lines.
const LUX_SCRIPT_NAME: &str = "LuxScriptName";
const ACCEPT_NULL: &str = "acceptNullTriggerMessages";

/// Field names a link record may hold for its target, in the order the reader tries them. [data]
/// `work\mission_graph.py` TARGET_KEYS.
const TARGET_KEYS: [&str; 4] = ["objToSendMsgTo", "msgTarget", "target", "objToSend"];
/// Field names a link record may hold for its message. [data] MESSAGE_KEYS.
const MESSAGE_KEYS: [&str; 3] = ["msgType", "message", "msgToSend"];

/// The CRC-32s of `TARGET_KEYS` and `MESSAGE_KEYS`, computed at compile time.
const TARGET_HASHES: [u32; 4] = [crc32(b"objToSendMsgTo"), crc32(b"msgTarget"), crc32(b"target"), crc32(b"objToSend")];
const MESSAGE_HASHES: [u32; 3] = [crc32(b"msgType"), crc32(b"message"), crc32(b"msgToSend")];
/// Field names a link record may hold for its name filter. [data] FILTER_KEYS. `msgTargetGroup` is
/// the group id of an AIScriptMsgConvert [assumed]; it is read here as a filter as the python does.
const FILTER_KEYS: [&str; 2] = ["luxScriptNameSendFilter", "msgTargetGroup"];

/// The name hash of `Root` (`b6c65665`, the CRC-32 of the name; a test checks it). [game] 0080d7d9
/// compares a link's target name with it and skips the registry lookup (the unpublished notes
/// Q1, [game, disasm]). A Root link is never looked up by name; it resolves to an owner (see [`RootReading`]).
pub const ROOT_NAME: u32 = 0xb6c6_5665;

/// Which object a Root link sends to. [game] 0080d927 stores the trigger array's owner object (array +0x18)
/// as the element's target when the owner handle is live. Who writes that handle is [unknown]
/// (the unpublished notes Q1 "Not settled"), so the two readings below are [assumed].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootReading {
    /// The object whose link site this is (the script object holding the trigger array). [assumed]
    Holder,
    /// That object's registration parent (obj+0xc, the object it sits under). [assumed]
    Parent,
}

/// The reading [`Script::load`] uses: Parent. With instancing the parent of a
/// template's component is the Instance that places it, so Parent is the owning Instance, the reading the
/// data supports [data] [assumed]. Before instancing the parent was the first object that reached the
/// component, and Parent reached 29 of 54 escort Root links against Holder's 5 (the unpublished notes Q1).
pub const DEFAULT_ROOT_READING: RootReading = RootReading::Parent;

/// Identifies one object of a loaded script: an index into [`Script::objects`]. [assumed] the
/// index is chunk order, then the walk's order (the same object is the same id every load).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(pub usize);

/// A message: its name, one argument and the link's name filter. [game] the four-word block of
/// `send_trigger_array` (the unpublished notes 2c) has a sender handle and a filter as
/// well; the filter comes from the link (`luxScriptNameSendFilter`), the sender is passed to the
/// handler as `from`. The argument is 0 for every trigger-array message [assumed] (the sender
/// argument `msg[1]` has no reader yet). A receiver reads the filter from here (SwitchTrigger does).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Msg {
    pub name: u32,
    pub arg: u32,
    pub filter: u32,
}

impl Msg {
    pub const fn new(name: u32) -> Self {
        Self { name, arg: 0, filter: 0 }
    }

    /// A message with its link's name filter.
    pub const fn with_filter(name: u32, filter: u32) -> Self {
        Self { name, arg: 0, filter }
    }
}

/// One record of a trigger array: where a message goes, which message, and after how long.
/// [data] fields `objToSendMsgTo`, `msgType`, `luxScriptNameSendFilter`, `delayMin`, `delayMax`.
/// The array's overrides are already applied to `msg_type` and `name_filter` (see `scan_value`).
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// The target object. `None` when the target is not an object of the pack (see the chunk
    /// report's reason).
    pub target: Option<ObjectId>,
    /// The target's name hash (its header label). 0 when the target is unnamed. [data]
    pub target_name: u32,
    /// The message name hash (`msgType`, or the array's `overrideMsgType`). [data]
    pub msg_type: u32,
    /// The name filter (`luxScriptNameSendFilter`, or the array's override), 0 when none. [data]
    pub name_filter: u32,
    /// `delayMin`, seconds. 0 when the field is absent. [data]
    pub delay_min: f32,
    /// `delayMax`, seconds. 0 when the field is absent. [data]
    pub delay_max: f32,
}

/// The records of one trigger array (a list of links sent together). [data] `triggerArray`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TriggerArray(pub Vec<Link>);

/// Where a script object's data sits: the pack's data chunk index and the object's offset in
/// that chunk. A reader re-opens it with [`Script::chunk`]; the decoded `Node` borrows the chunk
/// and cannot be stored, so the offset stands in for it. [stand-in]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeRef {
    pub chunk: usize,
    pub offset: Option<usize>,
}

/// The link records found under one field path of an object (a list is one site). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct LinkSite {
    /// The field path inside the object, as `work\mission_graph.py` prints it (`a.b[3]`), or
    /// `(root)` when the object is itself a link record.
    pub path: String,
    pub array: TriggerArray,
}

/// One object of a mission pack, as the walk found it. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptObject {
    pub id: ObjectId,
    /// CRC-32 of `type_name`.
    pub type_hash: u32,
    /// The object's type name (`CharacterSpawnManager`, `TrigBox`, ...), `X (extension)` for an
    /// unnamed extension, or `(embedded data)`.
    pub type_name: String,
    /// The object's name hash (its header label). 0 when unnamed (many TrigBox objects are). [data]
    pub name_hash: u32,
    /// The object's name text from its chunk's name table, or `#hex`; empty when unnamed.
    pub name: String,
    /// The object that holds this one: the Instance, Scene or Object whose `things`, `attachments` or
    /// `thing` list holds it (containment, not a link). For a copy, its Instance. None when no such list
    /// holds it. [assumed] the game's parent is the registrant's `obj+0xc` ([game],
    /// the unpublished notes 2); which field the game takes as the holder is [assumed] too.
    pub parent: Option<ObjectId>,
    /// `startEnabled` when the object has the field. [data]
    pub start_enabled: Option<bool>,
    /// `LuxScriptName`: the filter a message must carry to be taken (0 when absent). [data]
    pub lux_script_name: u32,
    /// `acceptNullTriggerMessages`: whether a message with filter 0 is taken (false when absent).
    /// [data]
    pub accept_null: bool,
    pub node: NodeRef,
    /// The link sites of this object (not of its child objects, which have their own entry).
    pub links: Vec<LinkSite>,
    /// The Instance this object is a copy for: a component of a template, copied once per Instance that
    /// places the template. None for an object of the file itself. [assumed]
    pub instance: Option<ObjectId>,
}

/// What the instancing pass found and made. Counts are of the pack's typed
/// objects (`before`: as the walk registered them, embedded data included) and of the instanced list (`after`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Instancing {
    /// Typed `Instance` objects.
    pub instances: usize,
    /// Typed `Object` records.
    pub object_records: usize,
    /// Object records by the number of distinct Instances that hold them in `things`: 0, 1, 2 or more.
    pub referenced_by: [usize; 3],
    /// Distinct (Instance, Object) pairs: an Object held by an Instance in its `things`.
    pub references: usize,
    /// Objects held by at least one Instance (the templates).
    pub templates: usize,
    /// Typed objects inside a template's containment subtree (its components, its pieces), once per record.
    pub components: usize,
    /// Copies made: one per (Instance, component) pair.
    pub copies: usize,
    /// Object records that no Instance holds and that sit in no template, with the types they hold.
    pub orphan_templates: Vec<String>,
    /// Instances reached inside a template subtree. They are not copied (none in the escort).
    pub instances_in_templates: usize,
    /// Typed objects the walk registered (the object list before instancing).
    pub before: usize,
    /// Objects in the instanced list.
    pub after: usize,
    /// Typed objects by type name, as the walk registered them (one per record, embedded data included).
    pub records_by_type: std::collections::BTreeMap<String, usize>,
    /// Link records in the pack (one per record), and the Root and null ones among them.
    pub record_links: usize,
    pub record_root_links: usize,
    pub record_null_links: usize,
}

/// The CRC-32s of the lists that hold a component (containment, not links): `things` (an Instance or Scene),
/// `attachments` (an Object's components) and `thing` (a breakable piece of a DetachLink or DamageLink). [data]
/// the field names; the containment reading is [assumed] from `level_render::carries_placement`. The walk tests
/// every field against them, so they are computed at compile time.
const CONTAIN_HASHES: [u32; 3] = [crc32(b"things"), crc32(b"attachments"), crc32(b"thing")];

/// One object of the instanced list: an object of the file (`instance` None) or a copy of a template's
/// component for one Instance. The list index is the object's id.
struct Entry {
    old: usize,
    instance: Option<usize>,
    template: Option<usize>,
}

/// Builds the instanced list in order: each original (not a template, not a component), and, when an
/// Instance is reached, its children in their order: a template child gives the copies of its components
/// for that Instance, any other child is an original.
struct Emit<'a> {
    is_template: &'a [bool],
    is_component: &'a [bool],
    is_instance: &'a [bool],
    children: &'a [Vec<usize>],
    closure: &'a HashMap<usize, Vec<usize>>,
    entries: Vec<Entry>,
    orig_idx: Vec<Option<usize>>,
    copy_idx: HashMap<(usize, usize), usize>,
}

impl Emit<'_> {
    fn original(&mut self, x: usize) {
        if self.orig_idx[x].is_some() {
            return;
        }
        self.orig_idx[x] = Some(self.entries.len());
        self.entries.push(Entry { old: x, instance: None, template: None });
        if self.is_instance[x] {
            // `children` is a shared borrow, copied out of `self` so the list is read without a clone.
            let children = self.children;
            for &ch in &children[x] {
                if self.is_template[ch] {
                    self.copies(x, ch);
                } else if !self.is_component[ch] {
                    self.original(ch);
                }
            }
        }
    }

    /// The copies of one template's components for one Instance, in walk order.
    fn copies(&mut self, inst: usize, template: usize) {
        // The template's closure is computed once (in `load_with`); it is read here, not copied per Instance.
        let closure = self.closure;
        for &y in closure.get(&template).map_or(&[][..], Vec::as_slice) {
            if self.copy_idx.contains_key(&(y, inst)) {
                continue;
            }
            self.copy_idx.insert((y, inst), self.entries.len());
            self.entries.push(Entry { old: y, instance: Some(inst), template: Some(template) });
        }
    }
}

/// A link whose target is not an object of the pack. [data]
#[derive(Clone, Debug, PartialEq)]
pub struct Unresolved {
    pub from: ObjectId,
    pub path: String,
    pub target_name: u32,
    pub reason: &'static str,
}

/// What the loader found in one data chunk. [data]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChunkReport {
    /// The chunk's index among all the pack's chunks (the python's `chunk N`).
    pub index: usize,
    pub bytes: usize,
    pub objects: usize,
    pub links: usize,
    /// Links whose target is an object of the pack (any chunk).
    pub resolved: usize,
    pub unresolved: Vec<Unresolved>,
    /// Set when the chunk did not parse as a data object; nothing from it is loaded.
    pub error: Option<String>,
}

/// A mission's script: every object of every data chunk, the links between them, and the name
/// index. Built by [`Script::load`].
pub struct Script {
    pub objects: Vec<ScriptObject>,
    /// Name hash to object. When two objects share a name, the first one wins.
    pub by_name: HashMap<u32, ObjectId>,
    pub reports: Vec<ChunkReport>,
    /// Objects whose name hash another object already has (the second is not in `by_name`).
    pub duplicate_names: usize,
    /// Resolved links whose target is in a different chunk from the link's object.
    pub cross_chunk_links: usize,
    /// Links with no name and no pointer (a null target). They send nothing and are not counted as
    /// unresolved. [game] the unpublished notes Q2.
    pub null_links: usize,
    /// Links named Root (no pointer). They are not looked up by name; each resolves to the owner of its
    /// array under [`Script::root_reading`] (`root_unowned` of them have no owner). [game] Q1, [assumed] reading.
    pub root_links: usize,
    /// Root links with no owner under the reading (no registration parent, for [`RootReading::Parent`]).
    pub root_unowned: usize,
    /// The reading the Root links were resolved with.
    pub root_reading: RootReading,
    /// What the instancing pass found and made (see [`Instancing`]).
    pub instancing: Instancing,
    /// Objects by (chunk, offset of the object's data). Identity as the python resolves it.
    at_index: HashMap<(usize, usize), ObjectId>,
    chunks: Vec<(usize, DataFile)>,
    /// Where each typed node sits in its chunk, built on the first [`Script::node`] call (one walk per chunk).
    nodes: OnceLock<NodeIndex>,
}

/// One step of a path through a chunk's tree: a field of a struct (by its position in `fields()`), or an
/// item of a list (by its index).
#[derive(Clone, Copy, Debug)]
enum Step {
    Field(u32),
    Item(u32),
}

/// For each (chunk, offset): the typed nodes stored there (their type name and the path to the node's first visit).
type NodeIndex = HashMap<(usize, usize), Vec<(String, Vec<Step>)>>;

impl Script {
    /// Reads every data chunk of the mission pack and resolves its links.
    ///
    /// `mission_pack_path` is used as it is when absolute, otherwise it is joined onto
    /// `install_dir`. [data] the packs are `levels\<zone>\missions\<mission>\mission.str`.
    pub fn load(install_dir: &Path, mission_pack_path: &Path) -> Result<Script, String> {
        Self::load_with(install_dir, mission_pack_path, DEFAULT_ROOT_READING)
    }

    /// As [`Script::load`], with the reading of Root links given by the caller.
    pub fn load_with(install_dir: &Path, mission_pack_path: &Path, root: RootReading) -> Result<Script, String> {
        let path = if mission_pack_path.is_absolute() {
            mission_pack_path.to_path_buf()
        } else {
            install_dir.join(mission_pack_path)
        };
        // Simple timers, printed to stderr when `MISSION_LOAD_TIMING` is set. Measurement only.
        let timing = std::env::var_os("MISSION_LOAD_TIMING").is_some();
        let mut clock = std::time::Instant::now();
        let mut lap = |what: &str| {
            if timing {
                eprintln!("MISSION_LOAD_TIMING {what}: {:.3} s", clock.elapsed().as_secs_f64());
            }
            clock = std::time::Instant::now();
        };

        let pack = Pack::open(&path)?;
        lap("open and inflate pack");

        let mut chunks: Vec<(usize, DataFile)> = Vec::new();
        let mut reports: Vec<ChunkReport> = Vec::new();
        // The raw bytes of each chunk are kept for the walk's object-header test.
        let mut raw_bytes: Vec<(usize, Vec<u8>)> = Vec::new();
        for chunk in pack.chunks.iter().filter(|c| c.tag == pack::DATA) {
            let bytes = pack.data(chunk).to_vec();
            let size = bytes.len();
            match DataFile::parse(bytes.clone()) {
                Ok(file) => {
                    reports.push(ChunkReport { index: chunk.index, bytes: size, ..ChunkReport::default() });
                    chunks.push((chunk.index, file));
                    raw_bytes.push((chunk.index, bytes));
                }
                Err(e) => reports.push(ChunkReport { index: chunk.index, bytes: size, error: Some(e), ..ChunkReport::default() }),
            }
        }
        lap("decode chunk headers and names");

        // Walk every chunk. `raw[i]` holds the link sites of object i, with targets still
        // unresolved (resolution needs every object of the pack first).
        let mut objects: Vec<ScriptObject> = Vec::new();
        let mut raw: Vec<Vec<RawSite>> = Vec::new();
        // The containment edges (chunk, owner offset, component offset) of every chunk. [data]
        let mut contained: Vec<(usize, usize, usize)> = Vec::new();
        for ((index, file), (_, bytes)) in chunks.iter().zip(raw_bytes.iter()) {
            let started = std::time::Instant::now();
            let before = objects.len();
            let mut walk = Walk { chunk: *index, file, bytes, walked: HashSet::new(), scanned: HashSet::new(), contained: Vec::new() };
            walk.register(file.root(), &mut objects, &mut raw, 0);
            contained.extend(walk.contained.iter().map(|&(owner, child)| (*index, owner, child)));
            if timing {
                eprintln!("MISSION_LOAD_TIMING walk chunk {index}: {} objects in {:.3} s", objects.len() - before, started.elapsed().as_secs_f64());
            }
        }
        let walk_count = objects.len();
        lap("walk every chunk (register, scan links)");

        // Containment: each object's containers, the objects whose `things`, `attachments` or `thing`
        // list holds it, in walk order. [data] Link fields are not containment, so a link target is no
        // one's component (the walk's first visit used to make it one: the unpublished notes).
        let n = objects.len();
        let mut at_old: HashMap<(usize, usize), usize> = HashMap::new();
        let mut by_name_old: HashMap<u32, usize> = HashMap::new();
        for o in &objects {
            if let Some(at) = o.node.offset {
                at_old.entry((o.node.chunk, at)).or_insert(o.id.0);
            }
            if o.name_hash != 0 {
                by_name_old.entry(o.name_hash).or_insert(o.id.0);
            }
        }
        let mut containers: Vec<Vec<usize>> = vec![Vec::new(); n];
        for &(chunk, owner, child) in &contained {
            let (Some(&o), Some(&c)) = (at_old.get(&(chunk, owner)), at_old.get(&(chunk, child))) else { continue };
            if o != c && !containers[c].contains(&o) {
                containers[c].push(o);
            }
        }
        let mut children_old: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (c, holders) in containers.iter().enumerate() {
            for &o in holders {
                children_old[o].push(c);
            }
        }
        lap("containment (containers, children)");

        // Instancing. An Object that an Instance holds (in its `things`) is a template: the Instance places
        // it. Its components (its containment subtree) are copied once per Instance, parented to that
        // Instance, with the Instance's matrix as their placement. [assumed] the reading of the data in
        // the unpublished notes; the game's builder is unread (the unpublished notes 4).
        let is_instance: Vec<bool> = objects.iter().map(|o| o.type_name == "Instance").collect();
        let is_object: Vec<bool> = objects.iter().map(|o| o.type_name == "Object").collect();
        let holders: Vec<Vec<usize>> = containers.iter().map(|c| c.iter().copied().filter(|&h| is_instance[h]).collect()).collect();
        let is_template: Vec<bool> = (0..n).map(|x| is_object[x] && !holders[x].is_empty()).collect();
        let mut is_component = vec![false; n];
        let mut closure: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut instances_in_templates = 0usize;
        for t in (0..n).filter(|&x| is_template[x]) {
            let mut seen: HashSet<usize> = HashSet::new();
            let mut stack: Vec<usize> = children_old[t].clone();
            let mut list: Vec<usize> = Vec::new();
            while let Some(x) = stack.pop() {
                if !seen.insert(x) {
                    continue;
                }
                if is_instance[x] {
                    instances_in_templates += 1;
                    continue;
                }
                list.push(x);
                stack.extend(children_old[x].iter().copied());
            }
            list.sort_unstable();
            for &x in &list {
                is_component[x] = true;
            }
            closure.insert(t, list);
        }
        lap("instancing: template closures");
        let mut emit = Emit {
            is_template: &is_template,
            is_component: &is_component,
            is_instance: &is_instance,
            children: &children_old,
            closure: &closure,
            entries: Vec::new(),
            orig_idx: vec![None; n],
            copy_idx: HashMap::new(),
        };
        for x in 0..n {
            if !is_template[x] && !is_component[x] {
                emit.original(x);
            }
        }
        let Emit { entries, orig_idx, copy_idx, .. } = emit;
        lap("instancing: emit entries and copies");

        // The parent of each object of the instanced list: an original's first container that is itself an
        // original; a copy's container in its own instance (the template's own copy is the Instance).
        let parents: Vec<Option<usize>> = entries
            .iter()
            .map(|e| match e.instance {
                None => containers[e.old].iter().find_map(|&c| orig_idx[c]),
                Some(inst) => containers[e.old].iter().find_map(|&c| if Some(c) == e.template { orig_idx[inst] } else { copy_idx.get(&(c, inst)).copied() }),
            })
            .collect();
        let mut out: Vec<ScriptObject> = Vec::with_capacity(entries.len());
        for (idx, e) in entries.iter().enumerate() {
            let o = &objects[e.old];
            out.push(ScriptObject {
                id: ObjectId(idx),
                type_hash: o.type_hash,
                type_name: o.type_name.clone(),
                name_hash: o.name_hash,
                name: o.name.clone(),
                parent: parents[idx].map(ObjectId),
                start_enabled: o.start_enabled,
                lux_script_name: o.lux_script_name,
                accept_null: o.accept_null,
                node: o.node,
                links: Vec::new(),
                instance: e.instance.and_then(|i| orig_idx[i]).map(ObjectId),
            });
        }
        lap("parents and object list");

        // Indexes: by offset within a chunk (identity, as the python resolves; a copy's offset is its
        // component's, so the first copy answers), by name over the instanced list.
        let mut at_index: HashMap<(usize, usize), ObjectId> = HashMap::new();
        let mut by_name: HashMap<u32, ObjectId> = HashMap::new();
        let mut duplicate_names = 0;
        for o in &out {
            if let Some(at) = o.node.offset {
                at_index.entry((o.node.chunk, at)).or_insert(o.id);
            }
            if o.name_hash != 0 {
                match by_name.entry(o.name_hash) {
                    std::collections::hash_map::Entry::Vacant(e) => {
                        e.insert(o.id);
                    }
                    std::collections::hash_map::Entry::Occupied(_) => duplicate_names += 1,
                }
            }
        }

        lap("indexes (offset, name)");
        // The target of a link, for a sender in instance `inst` (None when it is no copy). A template Object
        // has no receiver of its own, so a link to it resolves to nothing.
        let remap = |t: usize, inst: Option<usize>| -> Option<usize> {
            if is_template[t] {
                None
            } else if is_component[t] {
                inst.and_then(|i| copy_idx.get(&(t, i)).copied())
            } else {
                orig_idx[t]
            }
        };

        // Resolve every link: the target by identity in the same chunk, else by name in the pack, else
        // the owner for a Root link. A copy resolves its links within its own instance.
        let mut cross_chunk_links = 0;
        let mut null_links = 0usize;
        let mut root_links = 0usize;
        let mut root_unowned = 0usize;
        let mut sites_of: Vec<Vec<LinkSite>> = Vec::with_capacity(entries.len());
        for (idx, e) in entries.iter().enumerate() {
            let from = ObjectId(idx);
            let from_chunk = objects[e.old].node.chunk;
            let mut out_sites = Vec::with_capacity(raw[e.old].len());
            for site in &raw[e.old] {
                let mut links = Vec::with_capacity(site.links.len());
                for rl in &site.links {
                    let identity_old = rl.target_offset.and_then(|at| at_old.get(&(from_chunk, at)).copied());
                    let by_identity = identity_old.and_then(|t| remap(t, e.instance));
                    // [game] a null target (no name, no pointer) sends nothing: the resolver skips it and the
                    // sender's null check releases the element (the unpublished notes Q2).
                    let is_null = rl.target_name == 0 && rl.target_offset.is_none();
                    // [game] the name Root is never looked up (Q1): the target is the owner of the array.
                    let is_root = rl.target_name == ROOT_NAME && rl.target_offset.is_none();
                    let owner = match root {
                        RootReading::Holder => Some(idx),
                        RootReading::Parent => parents[idx],
                    };
                    let by_name_hit = if rl.target_name != 0 && !is_root {
                        by_name_old.get(&rl.target_name).and_then(|&t| remap(t, e.instance))
                    } else {
                        None
                    };
                    let root_hit = if is_root { owner } else { None };
                    let target = by_identity.or(by_name_hit).or(root_hit);
                    if is_null {
                        null_links += 1;
                    } else if is_root {
                        root_links += 1;
                        if target.is_none() {
                            root_unowned += 1;
                        }
                    }
                    let target = target.map(ObjectId);
                    match target {
                        Some(t) if out[t.0].node.chunk != from_chunk => cross_chunk_links += 1,
                        Some(_) => {}
                        // Null and Root links are not unresolved: a null sends nothing, a Root has an owner
                        // (or none, counted in `root_unowned`).
                        None if is_null || is_root => {}
                        None => {
                            let reason = if identity_old.is_some_and(|t| is_template[t]) {
                                "target is a template Object (no receiver of its own)"
                            } else {
                                rl.reason.unwrap_or(if rl.target_name == 0 {
                                    "unnamed target, not an object of its chunk"
                                } else {
                                    "name not in the pack"
                                })
                            };
                            if let Some(r) = reports.iter_mut().find(|r| r.index == from_chunk) {
                                r.unresolved.push(Unresolved { from, path: site.path.clone(), target_name: rl.target_name, reason });
                            }
                        }
                    }
                    if let Some(r) = reports.iter_mut().find(|r| r.index == from_chunk) {
                        r.links += 1;
                        if target.is_some() {
                            r.resolved += 1;
                        }
                    }
                    links.push(Link {
                        target,
                        target_name: rl.target_name,
                        msg_type: rl.msg_type,
                        name_filter: rl.name_filter,
                        delay_min: rl.delay_min,
                        delay_max: rl.delay_max,
                    });
                }
                out_sites.push(LinkSite { path: site.path.clone(), array: TriggerArray(links) });
            }
            sites_of.push(out_sites);
        }
        for (o, sites) in out.iter_mut().zip(sites_of) {
            o.links = sites;
        }
        for o in &out {
            if let Some(r) = reports.iter_mut().find(|r| r.index == o.node.chunk) {
                r.objects += 1;
            }
        }
        lap("resolve links");

        let mut records_by_type: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for o in &objects {
            *records_by_type.entry(o.type_name.clone()).or_insert(0) += 1;
        }
        let (mut record_links, mut record_root_links, mut record_null_links) = (0usize, 0usize, 0usize);
        for site in raw.iter().flatten() {
            for rl in &site.links {
                record_links += 1;
                if rl.target_name == 0 && rl.target_offset.is_none() {
                    record_null_links += 1;
                } else if rl.target_name == ROOT_NAME && rl.target_offset.is_none() {
                    record_root_links += 1;
                }
            }
        }

        let mut referenced_by = [0usize; 3];
        for x in (0..n).filter(|&x| is_object[x]) {
            referenced_by[holders[x].len().min(2)] += 1;
        }
        let orphan_templates: Vec<String> = (0..n)
            .filter(|&x| is_object[x] && holders[x].is_empty() && !is_component[x])
            .map(|x| {
                let kinds: Vec<&str> = children_old[x].iter().map(|&c| objects[c].type_name.as_str()).collect();
                format!("Object ({})", if kinds.is_empty() { "empty".to_owned() } else { kinds.join(", ") })
            })
            .collect();
        lap("instancing statistics");
        let instancing = Instancing {
            instances: is_instance.iter().filter(|&&b| b).count(),
            object_records: is_object.iter().filter(|&&b| b).count(),
            referenced_by,
            references: (0..n).filter(|&x| is_object[x]).map(|x| holders[x].len()).sum(),
            templates: is_template.iter().filter(|&&b| b).count(),
            components: is_component.iter().filter(|&&b| b).count(),
            copies: entries.iter().filter(|e| e.instance.is_some()).count(),
            orphan_templates,
            instances_in_templates,
            before: walk_count,
            after: out.len(),
            records_by_type,
            record_links,
            record_root_links,
            record_null_links,
        };

        Ok(Script {
            objects: out,
            by_name,
            reports,
            duplicate_names,
            cross_chunk_links,
            null_links,
            root_links,
            root_unowned,
            root_reading: root,
            at_index,
            chunks,
            instancing,
            nodes: OnceLock::new(),
        })
    }

    /// The object with this id.
    pub fn object(&self, id: ObjectId) -> Option<&ScriptObject> {
        self.objects.get(id.0)
    }

    /// The damage stage an object belongs to: how many DamageLinks hold it (0 the intact Object, 1 the Object of
    /// the first DamageLink's `thing`, 2 the one that stage's DamageLink holds). [data] the nesting; the game builds
    /// a stage only when the stage before it dies (00554830).
    pub fn damage_stage(&self, id: ObjectId) -> u8 {
        let mut stage = 0u8;
        let mut at = self.object(id).and_then(|o| o.parent);
        let mut guard = 0;
        while let Some(p) = at {
            let Some(o) = self.object(p) else { break };
            stage += u8::from(o.type_name == "DamageLink");
            at = o.parent;
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        stage
    }

    /// The object with this name hash.
    pub fn find(&self, name_hash: u32) -> Option<ObjectId> {
        self.by_name.get(&name_hash).copied()
    }

    /// The object stored at this offset of this data chunk. [data] the same identity a spawner's
    /// (chunk, offset) gives (`ai_spawn::SpawnerData::chunk`, `offset`).
    pub fn at(&self, chunk: usize, offset: usize) -> Option<ObjectId> {
        self.at_index.get(&(chunk, offset)).copied()
    }

    /// The decoded data chunk with this pack index, for a reader that needs an object's fields.
    pub fn chunk(&self, index: usize) -> Option<&DataFile> {
        self.chunks.iter().find(|(i, _)| *i == index).map(|(_, f)| f)
    }

    /// The stored node of an object, read from its chunk. The first call walks every chunk once and records
    /// where each typed node sits; a later call only follows that path. It returns the node the walk of the
    /// chunk reaches first for the object's (offset, type), as `mission_state::find_nodes` did with a walk per
    /// call. [assumed, ours] the index is the same walk, kept.
    pub fn node(&self, id: ObjectId) -> Option<Node<'_>> {
        let o = self.object(id)?;
        let at = o.node.offset?;
        let index = self.nodes.get_or_init(|| self.index_nodes());
        let (_, path) = index.get(&(o.node.chunk, at))?.iter().find(|(ty, _)| ty == &o.type_name)?;
        let mut node = self.chunk(o.node.chunk)?.root();
        for step in path {
            node = match *step {
                Step::Field(i) => node.fields().into_iter().nth(i as usize)?.1,
                Step::Item(i) => node.at(i as usize)?,
            };
        }
        Some(node)
    }

    /// The node index of every chunk (see [`Script::node`]).
    fn index_nodes(&self) -> NodeIndex {
        let mut index: NodeIndex = HashMap::new();
        for (chunk, file) in &self.chunks {
            let mut seen: HashSet<(usize, &str)> = HashSet::new();
            let mut path: Vec<Step> = Vec::new();
            index_visit(*chunk, file.root(), 0, &mut path, &mut seen, &mut index);
        }
        index
    }

    /// How many objects have this type name.
    pub fn type_count(&self, type_name: &str) -> usize {
        self.objects.iter().filter(|o| o.type_name == type_name).count()
    }

    /// Every link (resolved or not) in the script.
    pub fn link_count(&self) -> usize {
        self.objects.iter().flat_map(|o| &o.links).map(|s| s.array.0.len()).sum()
    }

    /// A text dump of the whole script, for checking that a loader change alters nothing: the summary
    /// counts, then one line per object in id order (type, name, parent, holder, node, links with their
    /// targets, messages, filters and delays), then the name and offset indexes sorted. Two loads of one
    /// pack must give the same text. [assumed, ours] test and loader-speed tool only; nothing reads it.
    pub fn fingerprint(&self) -> String {
        use std::fmt::Write as _;
        let mut s = String::new();
        let _ = writeln!(s, "objects {}", self.objects.len());
        let mut per_type: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for o in &self.objects {
            *per_type.entry(o.type_name.as_str()).or_insert(0) += 1;
        }
        for (t, n) in &per_type {
            let _ = writeln!(s, "type {t} {n}");
        }
        let _ = writeln!(s, "links {}", self.link_count());
        let _ = writeln!(
            s,
            "cross_chunk {} null {} root {} root_unowned {} duplicate_names {} by_name {} at_index {}",
            self.cross_chunk_links,
            self.null_links,
            self.root_links,
            self.root_unowned,
            self.duplicate_names,
            self.by_name.len(),
            self.at_index.len()
        );
        let _ = writeln!(s, "instancing {:?}", self.instancing);
        for r in &self.reports {
            let _ = writeln!(s, "chunk {} bytes {} objects {} links {} resolved {} unresolved {} error {:?}", r.index, r.bytes, r.objects, r.links, r.resolved, r.unresolved.len(), r.error);
        }
        for o in &self.objects {
            let _ = write!(
                s,
                "obj {} {} {:08x} {:?} parent {:?} inst {:?} start {:?} lux {:08x} accept {} node {}:{:?} type_hash {:08x}",
                o.id.0,
                o.type_name,
                o.name_hash,
                o.name,
                o.parent.map(|p| p.0),
                o.instance.map(|p| p.0),
                o.start_enabled,
                o.lux_script_name,
                o.accept_null,
                o.node.chunk,
                o.node.offset,
                o.type_hash
            );
            for site in &o.links {
                let _ = write!(s, " | {:?} [", site.path);
                for l in &site.array.0 {
                    let _ = write!(
                        s,
                        "({} {:08x} {:08x} {:08x} {:08x} {:08x}) ",
                        l.target.map_or(usize::MAX, |t| t.0),
                        l.target_name,
                        l.msg_type,
                        l.name_filter,
                        l.delay_min.to_bits(),
                        l.delay_max.to_bits()
                    );
                }
                let _ = write!(s, "]");
            }
            let _ = writeln!(s);
        }
        let mut names: Vec<(&u32, &ObjectId)> = self.by_name.iter().collect();
        names.sort_unstable();
        for (h, id) in names {
            let _ = writeln!(s, "name {h:08x} {}", id.0);
        }
        let mut ats: Vec<(&(usize, usize), &ObjectId)> = self.at_index.iter().collect();
        ats.sort_unstable();
        for (k, id) in ats {
            let _ = writeln!(s, "at {}:{} {}", k.0, k.1, id.0);
        }
        s
    }

    /// For each object, how many resolved links name it as their target.
    pub fn incoming_counts(&self) -> Vec<usize> {
        let mut counts = vec![0usize; self.objects.len()];
        for o in &self.objects {
            for site in &o.links {
                for link in &site.array.0 {
                    if let Some(t) = link.target {
                        counts[t.0] += 1;
                    }
                }
            }
        }
        counts
    }
}

// ---------------------------------------------------------------------------------------------
// The walk. Objects are the typed values stored behind a header, with their links. [data]
// ---------------------------------------------------------------------------------------------

/// The node index's walk (the same walk `mission_state::find_nodes` made per call): a node is visited when it
/// is within `WALK_DEPTH`; a typed node already seen at its (offset, type) is not visited again. The walk
/// pushes a node's fields and then its items and pops them last first, so the items are visited from the last
/// to the first, then the fields from the last to the first. The first visit of each typed node is recorded.
fn index_visit<'a>(chunk: usize, node: Node<'a>, depth: u32, path: &mut Vec<Step>, seen: &mut HashSet<(usize, &'a str)>, index: &mut NodeIndex) {
    if depth > WALK_DEPTH {
        return;
    }
    if let (Some(at), Some(ty)) = (node.offset(), node.type_name()) {
        if !seen.insert((at, ty)) {
            return;
        }
        index.entry((chunk, at)).or_default().push((ty.to_owned(), path.clone()));
    }
    let items: Vec<Node<'a>> = node.items().collect();
    for (i, item) in items.into_iter().enumerate().rev() {
        path.push(Step::Item(i as u32));
        index_visit(chunk, item, depth + 1, path, seen, index);
        path.pop();
    }
    let fields = node.fields();
    for (i, (_, child)) in fields.into_iter().enumerate().rev() {
        path.push(Step::Field(i as u32));
        index_visit(chunk, child, depth + 1, path, seen, index);
        path.pop();
    }
}

/// A link as read from the data, before its target is resolved.
struct RawLink {
    /// The target's name hash (its label). 0 when unnamed. [data]
    target_name: u32,
    /// The target's offset in its chunk, when the pointer settles on stored data. [data]
    target_offset: Option<usize>,
    /// Why the target cannot be found from this record alone (`None` when it can). [data]
    reason: Option<&'static str>,
    msg_type: u32,
    name_filter: u32,
    delay_min: f32,
    delay_max: f32,
}

/// One field path's links, before resolution.
struct RawSite {
    path: String,
    links: Vec<RawLink>,
}

/// The walk of one chunk: its decoded data, its raw bytes, and what it has seen. `walked` is for
/// the object walk, `scanned` for the link scan: kept apart, so a struct the scan has read is
/// still walked for its child objects.
struct Walk<'a> {
    chunk: usize,
    file: &'a DataFile,
    bytes: &'a [u8],
    walked: HashSet<(usize, u32)>,
    scanned: HashSet<(usize, u32)>,
    /// (owner offset, component offset) for every component reached through a containment list.
    contained: Vec<(usize, usize)>,
}

impl<'a> Walk<'a> {
    /// The type name of an object: a value stored behind its header, or an embedded data file.
    /// [data] A pointer target has an 8-byte header before its data (see `header_names_type`);
    /// an inline value has a declared type too, but no header, so only the header test tells the
    /// two apart. [assumed] the header test is exact: a false match needs a named type descriptor
    /// at a random offset with the same name. Not measured beyond the escort mission's counts.
    fn object_type(&self, node: Node<'a>) -> Option<String> {
        if !self.is_object(node) {
            return None;
        }
        if node.is_embedded_file() {
            return Some(EMBEDDED_TYPE.to_owned());
        }
        // The label `named_type` gives, built only for a node that is an object.
        node.type_name().map(str::to_owned).or_else(|| node.extension_base().map(|base| format!("{base} (extension)")))
    }

    /// Whether `object_type` names this node, without building the name (the walk asks this of every
    /// node it visits, so it must not allocate). Same tests as `object_type`. [data]
    fn is_object(&self, node: Node<'a>) -> bool {
        if node.is_embedded_file() {
            return true;
        }
        let Some(at) = node.offset() else { return false };
        if let Some(name) = node.type_name() {
            return header_names_type(self.bytes, at, crc32(name.as_bytes()), false);
        }
        node.extension_base().is_some_and(|base| header_names_type(self.bytes, at, crc32(base.as_bytes()), true))
    }

    /// The node's identity in this chunk, when it is stored data: the offset with its type. The label is
    /// the one `named_type` gives (for an unnamed extension, `base (extension)`). [data]
    fn key(node: Node<'a>) -> Option<(usize, u32)> {
        let at = node.offset()?;
        if let Some(name) = node.type_name() {
            Some((at, crc32(name.as_bytes())))
        } else if let Some(base) = node.extension_base() {
            Some((at, crc32(format!("{base} (extension)").as_bytes())))
        } else if node.is_embedded_file() {
            Some((at, u32::MAX))
        } else {
            None
        }
    }

    /// Registers every object under `node` in walk order, with its link sites. Its `parent` is set later,
    /// from the containment edges (`contained`): a link does not make its target a component.
    /// A stored value reached by two paths is walked once per chunk, as the python decodes each
    /// object once; without this the pointer graph is walked again for every path, and cycles
    /// never end.
    fn register(&mut self, node: Node<'a>, objects: &mut Vec<ScriptObject>, raw: &mut Vec<Vec<RawSite>>, depth: u32) {
        if depth > MAX_DEPTH {
            return;
        }
        if let Some(key) = Self::key(node)
            && !self.walked.insert(key) {
            return;
        }
        if let Some(type_name) = self.object_type(node) {
            let id = ObjectId(objects.len());
            let name_hash = node.label().unwrap_or(0);
            let offset = node.offset();
            // One field list for the object's header fields, its links and its children (`fields()` builds a
            // new list on every call, so it is taken once here).
            let fields = node.fields();
            objects.push(ScriptObject {
                id,
                type_hash: crc32(type_name.as_bytes()),
                type_name,
                name_hash,
                name: if name_hash == 0 { String::new() } else { name_text(self.file, name_hash) },
                parent: None,
                start_enabled: find_key(&fields, &["startEnabled"]).map(flag),
                lux_script_name: find_key(&fields, &[LUX_SCRIPT_NAME]).and_then(hash_of).unwrap_or(0),
                accept_null: find_key(&fields, &[ACCEPT_NULL]).is_some_and(flag),
                node: NodeRef { chunk: self.chunk, offset },
                links: Vec::new(),
                instance: None,
            });
            raw.push(self.scan_object(&fields));
            let owner = offset.unwrap_or(0);
            for &(hash, child) in &fields {
                if CONTAIN_HASHES.contains(&hash) {
                    self.collect_contained(child, owner, 0);
                }
                self.register(child, objects, raw, depth + 1);
            }
            for item in node.items() {
                self.register(item, objects, raw, depth + 1);
            }
            return;
        }
        for (_, child) in node.fields() {
            self.register(child, objects, raw, depth + 1);
        }
        for item in node.items() {
            self.register(item, objects, raw, depth + 1);
        }
    }

    /// The typed objects under `node` that a containment list holds: the typed values reached through
    /// untyped structs and lists, and not past them. Embedded data (animation and weapon sets) is no
    /// component. [data] [assumed] the reading of `CONTAIN_HASHES`.
    fn collect_contained(&mut self, node: Node<'a>, owner: usize, depth: u32) {
        if depth > MAX_DEPTH || node.is_embedded_file() {
            return;
        }
        if self.is_object(node) {
            if let Some(at) = node.offset() {
                self.contained.push((owner, at));
            }
            return;
        }
        for (_, child) in node.fields() {
            self.collect_contained(child, owner, depth + 1);
        }
        for item in node.items() {
            self.collect_contained(item, owner, depth + 1);
        }
    }

    /// The link sites under one object, not descending into its child objects. [data] as the
    /// python's `_links_in`. A shared struct is scanned once per chunk: its links belong to the
    /// first object that reaches it. [assumed] the python does the same (a Ref is not walked).
    fn scan_object(&mut self, fields: &[(u32, Node<'a>)]) -> Vec<RawSite> {
        let mut sites = Vec::new();
        // The object may be a link record itself (an AIScriptMsgConvert carries its target and
        // message on its own fields): the python decoder's "(root)" site. [data] 7 such links.
        if is_link(fields) {
            // The python stops here too: a link record has no other sites.
            sites.push(RawSite { path: "(root)".to_owned(), links: vec![read_link(fields)] });
            return sites;
        }
        // One path buffer for the whole walk of this object: `scan_value` appends a field's name and
        // takes it off again, so a path string is only copied when a site is made.
        let mut path = String::new();
        for &(hash, child) in fields {
            path.clear();
            push_name(&mut path, self.file, hash);
            self.scan_value(child, &mut path, &mut sites, 1);
        }
        sites
    }

    /// Scans one value for link sites. `path` is the value's field path; it is restored before this returns.
    fn scan_value(&mut self, v: Node<'a>, path: &mut String, sites: &mut Vec<RawSite>, depth: u32) {
        if depth > MAX_DEPTH || self.is_object(v) {
            return;
        }
        if let Some(key) = Self::key(v)
            && !self.scanned.insert(key) {
            return;
        }
        let fields = v.fields();
        if is_link(&fields) {
            sites.push(RawSite { path: path.clone(), links: vec![read_link(&fields)] });
            return;
        }
        let items: Vec<Node<'a>> = v.items().collect();
        if !items.is_empty() {
            // The links of one list are one site (one trigger array).
            let mut group = Vec::new();
            for (i, item) in items.into_iter().enumerate() {
                if self.is_object(item) {
                    continue;
                }
                let item_fields = item.fields();
                if is_link(&item_fields) {
                    group.push(read_link(&item_fields));
                } else {
                    let len = path.len();
                    path.push_str(&format!("[{i}]"));
                    self.scan_value(item, path, sites, depth + 1);
                    path.truncate(len);
                }
            }
            if !group.is_empty() {
                sites.push(RawSite { path: path.clone(), links: group });
            }
            return;
        }
        // A struct that carries a trigger array's overrides (`overrideMsgType`,
        // `overrideLuxScriptNameSendFilter`): they replace the message and the filter of every
        // link in its `triggerArray`. [game] `0080d9f0` reads header +0x10 (name, sentinel
        // 0xfe74aaca) and header +0x14 (filter, 0 = none) over each element (the unpublished notes
        // section 6); the field names are [data].
        let (msg_over, filter_over) = array_overrides(&fields);
        let before = sites.len();
        let len = path.len();
        for &(hash, child) in &fields {
            path.truncate(len);
            path.push('.');
            push_name(path, self.file, hash);
            self.scan_value(child, path, sites, depth + 1);
        }
        path.truncate(len);
        if msg_over.is_some() || filter_over.is_some() {
            let prefix = format!("{path}.triggerArray");
            for site in sites[before..].iter_mut().filter(|s| s.path.starts_with(&prefix)) {
                for link in &mut site.links {
                    if let Some(m) = msg_over {
                        link.msg_type = m;
                    }
                    if let Some(f) = filter_over {
                        link.name_filter = f;
                    }
                }
            }
        }
    }
}

/// The overrides of a trigger array struct: the message (`None` for the sentinel) and the filter
/// (`None` for 0). [game] 0080d9f0 (section 6 of the unpublished notes); [data] fields.
fn array_overrides(fields: &[(u32, Node<'_>)]) -> (Option<u32>, Option<u32>) {
    let msg = find_key(fields, &["overrideMsgType"]).and_then(hash_of).filter(|&m| m != NO_OVERRIDE && m != NO_OVERRIDE_NAME);
    let filter = find_key(fields, &["overrideLuxScriptNameSendFilter"]).and_then(hash_of).filter(|&f| f != 0);
    (msg, filter)
}

/// Whether the 8 bytes before `at` are an object header whose type descriptor is a named type
/// with name hash `want`. [data] `formats/lxb.rs`: the header is the name label at `at-8` and the
/// type's offset at `at-4` (relative to `at-8`; 8 means untyped); the descriptor is KIND_NAMED
/// (12) with the name hash at +4. For an unnamed extension (`extension`) the descriptor is the
/// extension itself (kind 7) and `want` is its base class name. The KIND constants are private
/// in lxb.rs, so they are repeated here.
fn header_names_type(bytes: &[u8], at: usize, want: u32, extension: bool) -> bool {
    if at < 8 {
        return false;
    }
    let Some(rel) = u32_le(bytes, at - 4) else { return false };
    let rel = rel as i32;
    if rel == 8 {
        return false;
    }
    let Some(ty) = (at as i64 - 8).checked_add(rel as i64).and_then(|t| usize::try_from(t).ok()) else {
        return false;
    };
    if extension {
        return u32_le(bytes, ty) == Some(KIND_EXTENSION) && extension_base(bytes, ty) == Some(want);
    }
    u32_le(bytes, ty) == Some(KIND_NAMED) && u32_le(bytes, ty + 4) == Some(want)
}

/// KIND_NAMED and KIND_EXTENSION in `formats/lxb.rs` (private there, repeated here). [data]
const KIND_NAMED: u32 = 12;
const KIND_EXTENSION: u32 = 7;

/// The name hash of the first named part of the extension at `ty` (the base class), when there is
/// one. [data] as the python's `extension_label` (`tools\tf2_lxb.py`).
fn extension_base(bytes: &[u8], ty: usize) -> Option<u32> {
    let first_rel = u32_le(bytes, ty + 8)? as i32;
    let first = (ty as i64).checked_add(first_rel as i64).and_then(|t| usize::try_from(t).ok())?;
    (u32_le(bytes, first)? == KIND_NAMED).then(|| u32_le(bytes, first + 4)).flatten()
}

fn u32_le(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Whether a struct's fields make it a link record: a target field and a message field. [data]
fn is_link(fields: &[(u32, Node<'_>)]) -> bool {
    has_any(fields, &TARGET_HASHES) && has_any(fields, &MESSAGE_HASHES)
}

/// Whether one of the names (their CRC-32s) is a field here. The same test as `find_key(..).is_some()`,
/// with the hashes given: `is_link` runs on every node the walk visits.
fn has_any(fields: &[(u32, Node<'_>)], hashes: &[u32]) -> bool {
    hashes.iter().any(|h| fields.iter().any(|(f, _)| f == h))
}

fn read_link(fields: &[(u32, Node<'_>)]) -> RawLink {
    let (target_name, target_offset) = match find_key(fields, &TARGET_KEYS) {
        Some(t) => (t.label().unwrap_or(0), t.offset()),
        None => (0, None),
    };
    let reason = if target_name == 0 && target_offset.is_none() {
        Some("target pointer names nothing (null or untyped reference)")
    } else {
        None
    };
    let msg_type = find_key(fields, &MESSAGE_KEYS).and_then(hash_of).unwrap_or(0);
    let name_filter = find_key(fields, &FILTER_KEYS).and_then(hash_of).unwrap_or(0);
    let min = find_key(fields, &["delayMin"]).and_then(Node::float).or_else(|| find_key(fields, &["delay"]).and_then(Node::float));
    let max = find_key(fields, &["delayMax"]).and_then(Node::float).or_else(|| find_key(fields, &["delay"]).and_then(Node::float));
    RawLink {
        target_name,
        target_offset,
        reason,
        msg_type,
        name_filter,
        delay_min: min.unwrap_or(0.0),
        delay_max: max.unwrap_or(0.0),
    }
}

/// The node of the first of `names` that is a field here (in the order of `names`).
fn find_key<'a>(fields: &[(u32, Node<'a>)], names: &[&str]) -> Option<Node<'a>> {
    names.iter().find_map(|name| {
        let hash = crc32(name.as_bytes());
        fields.iter().find(|(h, _)| *h == hash).map(|(_, n)| *n)
    })
}

/// A name hash from an enum value's name, or an integer. [data] as `ai_spawn::hash_of`.
fn hash_of(node: Node<'_>) -> Option<u32> {
    node.enum_hash().or_else(|| node.int().map(|v| v as u32))
}

/// `startEnabled` and the like: an enum `true`/`false`, or a nonzero integer. [data]
fn flag(node: Node<'_>) -> bool {
    match node.enum_name() {
        Some("true") => true,
        Some("false") => false,
        _ => node.int().is_some_and(|v| v != 0),
    }
}

fn name_text(file: &DataFile, hash: u32) -> String {
    file.name(hash).map_or_else(|| format!("#{hash:08x}"), str::to_owned)
}

/// Appends `name_text(file, hash)` to `buf` without making a string of its own.
fn push_name(buf: &mut String, file: &DataFile, hash: u32) {
    match file.name(hash) {
        Some(text) => buf.push_str(text),
        None => buf.push_str(&format!("#{hash:08x}")),
    }
}

