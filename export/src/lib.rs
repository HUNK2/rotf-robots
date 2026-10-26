//! Writes a robot from the user's own install of Transformers: Revenge of the Fallen as standard
//! files for modding tools: glTF for the models, PNG and DDS for the textures, JSON for what glTF
//! cannot hold. The files are made on the user's machine from the user's copy of the game and are
//! not to be shared.
//!
//! What is written so far: the robot form, the vehicle form, every weapon, projectile and melee
//! weapon object, each with its clips as animations, and `<pack>.json` (clip events, attacks, root
//! travel, weapon mounts, render sets, collision bodies, materials, the character's attributes), and
//! the sounds: every sample its events play as WAV, and the events themselves in the JSON.

pub mod describe;
pub mod glb;
pub mod model;
pub mod names;
pub mod sounds;
pub mod texture;

use std::path::Path;

use tf2_core::character::CharacterData;
use tf2_core::formats::anim::{AnimRef, AnimSet};
use tf2_core::formats::characterlist::{CharacterList, Spec};
use tf2_core::formats::scene::ObjectDef;
use tf2_core::tuning::Actions;
use tf2_core::weapons::Kind;

use model::ClipOut;
use names::Names;

/// The version of the JSON files' format (`docs/FORMAT.md`), written as their `format_version`.
/// It goes up when a key is removed or renamed, or changes its type, unit or meaning; new keys,
/// event types and values leave it as it is.
pub const FORMAT_VERSION: u32 = 1;

/// The character `name` takes, as the game's `--character` does: a display name or a pack, or
/// `pack:Attributes` for one the list does not name.
pub fn spec(roster: &CharacterList, name: &str) -> Spec {
    match name.split_once(':') {
        None => roster.find(name).and_then(|e| roster.spec(e)).unwrap_or_else(|| roster.spec_by_pack(name)),
        Some((pack, attributes)) => Spec::by_pack(pack, Some(attributes)),
    }
}

/// Every character `--character` takes, as (name, pack): the game's list (whose entries name some
/// characters twice, as their single and multiplayer selves: once here), then those kept outside
/// it (Devastator).
pub fn roster(list: &CharacterList) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for entry in &list.entries {
        if let Some(spec) = list.spec(entry).filter(|s| !out.iter().any(|(n, p)| *n == entry.name && *p == s.pack)) {
            out.push((entry.name.clone(), spec.pack));
        }
    }
    for e in tf2_core::formats::characterlist::ELSEWHERE {
        out.push((e.pack.to_uppercase(), e.pack.to_owned()));
    }
    out
}

/// What a run wrote, for the report.
pub struct Written {
    pub files: Vec<(String, model::Summary)>,
    /// The JSON's file name.
    pub json: String,
    /// Sound events and samples written.
    pub sounds: (usize, usize),
    /// Textures and sounds that could not be written in full.
    pub problems: Vec<String>,
}

/// What a glb holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    Robot,
    Vehicle,
    /// A weapon list's object, which every weapon of the list shares.
    Weapon,
    Grenade,
    Missile,
    Turret,
    Melee,
}

impl PieceKind {
    pub fn word(self) -> &'static str {
        match self {
            PieceKind::Robot => "robot",
            PieceKind::Vehicle => "vehicle",
            PieceKind::Weapon => "weapon",
            PieceKind::Grenade => "grenade",
            PieceKind::Missile => "missile",
            PieceKind::Turret => "turret",
            PieceKind::Melee => "melee",
        }
    }
}

/// One glb of the character: its object, its clips, and the weapons (by name hash) it is the
/// model of.
pub struct Piece<'a> {
    /// The file's stem: `<pack>_robot`, `<pack>_weapon_<name>`...
    pub file: String,
    pub kind: PieceKind,
    pub object: &'a ObjectDef,
    pub clips: Vec<ClipOut<'a>>,
    pub weapons: Vec<u32>,
}

/// Writes the character under `out`: a glb per piece, the textures, the JSON and the README.
pub fn export(install: &Path, spec: &Spec, out: &Path) -> Result<Written, String> {
    let data = tf2_core::character::load(install, &spec.pack)?;
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    std::fs::write(out.join("README.txt"), readme(spec)).map_err(|e| e.to_string())?;
    let mut files = names::data_files(&spec.pack_file(install));
    files.extend(names::data_files(&install.join("bnxglobal.str")));
    // The character's own events, then the shared ones: every level carries its own copy of the
    // shared events (footsteps, jumps, landings, punches); the port reads us_city_00's, and so
    // does this. Grindor's wave plays Devastator's.
    let mut sound_events = vec![spec.sound_events(install), install.join(sounds::SHARED_LEVEL)];
    sound_events.extend([install.join(r"audio\eng\global.fev"), install.join(r"audio\eng\global_streaming.fev")]);
    if data.energon.is_some() {
        sound_events.extend(tf2_core::energon_wave::sound_events(install));
    }
    let mut names = Names::load(install, files, &sound_events);
    let pieces = pieces(&data, &spec.pack, &names);
    for piece in &pieces {
        for node in &piece.object.nodes {
            names.add(&node.name);
        }
        for clip in &piece.clips {
            names.add(clip.name.rsplit('.').next().unwrap_or(&clip.name));
        }
    }
    for (set, refs) in &data.animations.sets {
        names.add(set);
        for r in refs {
            names.add(&r.id);
        }
    }
    let mut shared = model::Shared::new(&data.library, out)?;
    let mut written = Vec::new();
    for piece in &pieces {
        let summary = model::write(piece.object, &mut shared, &piece.clips, &piece.file, &names)?;
        written.push((format!("{}.glb", piece.file), summary));
    }
    let mut json = describe::character(install, spec, &data, &pieces, &written, &mut shared, &names);
    let json_file = format!("{}.json", spec.pack);
    let attributes = describe::attributes(spec, &names);
    let sounds = sounds::write(install, &sound_events[0], &sound_events[1..], &json, &attributes, out);
    json["sounds"] = sounds.json;
    for (file, value) in [(json_file.clone(), json), (format!("{}_attributes.json", spec.pack), attributes)] {
        let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
        std::fs::write(out.join(&file), text).map_err(|e| format!("{file}: {e}"))?;
    }
    let mut problems = shared.problems;
    problems.extend(sounds.problems);
    Ok(Written { files: written, json: json_file, sounds: (sounds.events, sounds.samples), problems })
}

/// The character's glbs: the robot, the vehicle, one per weapon list (the turret launcher's is
/// thrown, not held: its turret is written instead), each grenade, missile and turret model, and
/// each melee weapon.
pub fn pieces<'a>(data: &'a CharacterData, pack: &str, names: &Names) -> Vec<Piece<'a>> {
    let mut out = vec![Piece { file: format!("{pack}_robot"), kind: PieceKind::Robot, object: &data.robot, clips: robot_clips(&data.animations), weapons: Vec::new() }];
    if let Some(vehicle) = &data.vehicle {
        let clips = clips_of(Some(Actions::VEHICLE_SET), &data.vehicle_animations);
        out.push(Piece { file: format!("{pack}_vehicle"), kind: PieceKind::Vehicle, object: vehicle, clips, weapons: Vec::new() });
    }
    let add = |out: &mut Vec<Piece<'a>>, kind: PieceKind, name: u32, object: &'a ObjectDef, clips: Vec<ClipOut<'a>>, weapons: Vec<u32>| {
        let stem = format!("{pack}_{}_{}", kind.word(), file_safe(&names.name(name)));
        let mut file = stem.clone();
        let mut n = 2;
        while out.iter().any(|p| p.file == file) {
            file = format!("{stem}_{n}");
            n += 1;
        }
        out.push(Piece { file, kind, object, clips, weapons });
    };
    let mut lists = Vec::new();
    for (i, def) in data.weapons.iter().enumerate() {
        if def.kind == Kind::Turret || lists.contains(&def.list) {
            continue;
        }
        let Some(object) = &data.weapon_objects[i] else { continue };
        lists.push(def.list);
        let shared: Vec<u32> = data.weapons.iter().filter(|d| d.list == def.list && d.kind != Kind::Turret).map(|d| d.name).collect();
        add(&mut out, PieceKind::Weapon, def.name, object, clips_of(None, &data.weapon_animations[i]), shared);
    }
    for (i, def) in data.weapons.iter().enumerate() {
        if let Some(object) = &data.grenade_objects[i] {
            add(&mut out, PieceKind::Grenade, def.name, object, Vec::new(), vec![def.name]);
        }
        if let Some(object) = &data.missile_objects[i] {
            add(&mut out, PieceKind::Missile, def.name, object, Vec::new(), vec![def.name]);
        }
        if let Some(object) = &data.turret_objects[i] {
            add(&mut out, PieceKind::Turret, def.name, object, clips_of(None, &data.turret_clips[i]), vec![def.name]);
        }
    }
    for melee in &data.melee_weapons {
        if let Some(object) = &melee.object {
            add(&mut out, PieceKind::Melee, melee.name, object, clips_of(None, &melee.clips), vec![melee.name]);
        }
    }
    // A clip of the robot's sets that moves no node of the robot goes with the first object it
    // does move, under its own `<set>.<id>`: the vehicle's idle in `DriveSet` goes with the
    // vehicle, Megatron's tank special with his tank's cannon.
    let moves = |object: &ObjectDef, clip: &ClipOut| object.nodes.iter().any(|n| clip.clip.channel(n.name_hash).is_some_and(|c| c.keys_any()));
    let robot = std::mem::take(&mut out[0].clips);
    for clip in robot {
        let to = if moves(&data.robot, &clip) { 0 } else { (1..out.len()).find(|&k| moves(out[k].object, &clip)).unwrap_or(0) };
        out[to].clips.push(clip);
    }
    out
}

fn file_safe(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

/// Every clip of the robot's sets, sets by name and clips in their stored order, named
/// `<set>.<id>`; a weapon-set clip's two aim clips follow it as `.aim_down` and `.aim_up`. The
/// vehicle model's own set (`VehicleModel`) goes with the vehicle.
pub fn robot_clips(animations: &AnimSet) -> Vec<ClipOut<'_>> {
    let mut sets: Vec<_> = animations.sets.iter().filter(|(set, _)| set.as_str() != Actions::VEHICLE_SET).collect();
    sets.sort_by(|a, b| a.0.cmp(b.0));
    sets.into_iter().flat_map(|(set, refs)| clips_of(Some(set), refs)).collect()
}

/// A list of clip references as animations, named `<set>.<id>`, or `<id>` for an object's own
/// list (a weapon's), each followed by its aim clips.
pub fn clips_of<'a>(set: Option<&str>, refs: &'a [AnimRef]) -> Vec<ClipOut<'a>> {
    let mut out = Vec::new();
    for r in refs {
        let Some(clip) = &r.clip else { continue };
        let name = set.map_or_else(|| r.id.clone(), |set| format!("{set}.{}", r.id));
        let extras = serde_json::json!({
            "rotf_set": set,
            "rotf_id": r.id,
            "looping": r.looping,
            "speed": describe::num(r.speed),
            "crossfade": describe::num(r.crossfade),
            "additive": r.additive,
            "length_ticks": describe::num(clip.length),
        });
        out.push(ClipOut { name: name.clone(), clip, looping: r.looping, additive: r.additive, attack: r.attack, extras });
        for (aim, which) in r.aim_offsets.iter().zip(["aim_down", "aim_up"]) {
            let extras = serde_json::json!({ "rotf_set": set, "rotf_id": r.id, "aim": which, "additive": true, "length_ticks": describe::num(aim.length) });
            out.push(ClipOut { name: format!("{name}.{which}"), clip: aim, looping: false, additive: true, attack: None, extras });
        }
    }
    out
}

fn readme(spec: &Spec) -> String {
    format!(
        "{name}, written by rotf-export from your own install of Transformers: Revenge of the Fallen.\n\
         \n\
         These files are made from your copy of the game. Use them in your own mods on your own\n\
         machine. Do not share, upload or publish them, or files made from them: they belong to the\n\
         game's owners. A mod you share should tell its users to run rotf-export themselves.\n\
         \n\
         {pack}_robot.glb      the robot: meshes, skeleton, skins, materials, and every clip as an\n\
         \x20                    animation named <set>.<id> (glTF 2.0)\n\
         {pack}_vehicle.glb    the vehicle form and its own clips (VehicleModel.<id>)\n\
         {pack}_weapon_*.glb   each weapon's model and its clips; _missile_, _grenade_, _turret_\n\
         \x20                    the projectile and thrown models; _melee_ the melee weapons\n\
         {pack}.json           what glTF cannot hold: every clip's events (sounds, effects, the\n\
         \x20                    change of form, weapon moves, attack windows), its attack and its\n\
         \x20                    root travel; where each weapon hangs; render sets; the collision\n\
         \x20                    bodies; every material parameter; the character's attributes as\n\
         \x20                    the game stores them.\n\
         \x20                    Its format: docs/FORMAT.md in github.com/HUNK2/rotf-robots-sdk\n\
         textures/            the textures as PNG; textures/dds/ the same as the game stores them\n\
         sounds/              every sample the character's sound events play, as WAV, by bank;\n\
         \x20                    the events themselves (layers, parameters, picks) are the JSON's\n\
         \x20                    \"sounds\", keyed by the names the clip events use\n\
         \n\
         Space: the glTF is y up, facing +Z, in metres; the game is z up, facing +Y. Each file's root\n\
         node turns one into the other. Every bone has a child '.frame' node: the skins bind to those,\n\
         as the game's drawing does, so a bone's own scale can stay off its children. A weapon hangs on\n\
         the '.frame' of the node the JSON names: put its root's children there, without the root.\n\
         Materials: glTF's metallic-roughness is an approximation of the game's shaders; the JSON has\n\
         each material's technique and parameters, and the specular and reflection maps. The vertex\n\
         colour is kept as the attribute _COLOR0: most techniques read it as blend weights, not a tint.\n\
         Animations: the game's own keys (it plays clips at 60 ticks a second). The root stays put,\n\
         as the game draws it: the travel is the character's movement, given per tick in the JSON.\n\
         Additive clips (and the weapon set's .aim_down / .aim_up) are written laid over the rest pose.\n\
         In Blender, set the scene to 60 fps before importing, so a frame is a game tick.\n\
         The vehicle's clips: a node a clip does not key keeps the previous clip's pose in the game,\n\
         but goes back to rest in glTF. A jet's V_Idle alone shows it folded; play Trans_R2V_A and\n\
         Trans_R2V_B first (the JSON gives each such clip's unkeyed_nodes).\n\
         Render sets (Optimus's special-attack meshes): a mesh outside its group's first set starts\n\
         hidden. Its node says so with KHR_node_visibility and the custom property render_set_hidden;\n\
         Blender does not read the first, so hide the objects that carry the second.\n\
         Collision bodies (hit boxes, attack bodies) are empty nodes named <node>.collision.<n> on\n\
         the node they ride; the shape is their custom property rotf_collision (the JSON's\n\
         \"collision\" gives the same).\n\
         Names the game's files do not spell out are written as #xxxxxxxx (the CRC-32 of the name).\n\
         An event whose node its file lacks (a clip shared between two weapons) says \"node_in_file\":\n\
         false. A few weapon and wing parts have sheared rest matrices, which glTF cannot hold: they\n\
         are written as the nearest move, turn and scale, as the port draws them (the run lists them).\n",
        name = spec.display.as_deref().unwrap_or(&spec.pack),
        pack = spec.pack,
    )
}
