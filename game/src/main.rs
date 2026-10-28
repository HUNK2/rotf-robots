//! A standalone Bevy sandbox for testing the decoded Transformers: Revenge of the Fallen data.
//!
//! It reads the game's packs from the install at runtime and ships none of them. Usage:
//!
//!   bumblebee                             run the sandbox as Bumblebee
//!   bumblebee     --check [pack]          load a character pack and report, no window
//!   bumblebee     --shot <file.png> [--after <s>] [--yaw <deg>] [--elev <deg>] [--no-hud]
//!                 [--clip <Set/Id@tick>] [--stick <0..1>] [--jump-at <s>] [--die-at <s>]
//!                 [--vehicle | --vehicle-from <s> [--vehicle-until <s>]] [--fire-from <s> [--fire-until <s>]]
//!                 [--stick-x <-1..1>] [--aim-from <s>] [--melee-at <s>,<s>...]
//!                 [--melee-hold-from <s>] [--special-at <s>]
//!                 [--hit-at <s> [--hit-flags <hex>]]
//!                 [--enemy-at <x,y>] [--stick-from <s>] [--stick-until <s>] [--look-from <s>] [--look-until <s>]
//!                 [--climb-from <s>] [--start <x,y,yaw>] [--bodies] [--spawners [name]] (with --mission)
//!                                         render for a moment, save one frame, and quit;
//!                                         the second and third lines hold a pose or feed
//!                                         input meanwhile
//!   ... [--robot-camera <0..2>] [--drive-camera <0..2>] [--no-sound]
//!                                         the game's camera distance levels; silence
//!   ... [--scouts]                        put the three scouts in the arena (it starts empty)
//!
//!   ... [--game-dir <folder>]             the game's install folder (the one that holds
//!                                         bnxglobal.str); saved for later runs
//!   ... [--pick-game-dir]                 choose the install folder in a dialog; saved too
//!
//! Without `--game-dir` the install is found by `tf2_core::install`: `TF2_GAME_DIR`, the saved
//! folder, the installer's registry entry, then this program's folder and the working folder.
//! When none of those holds it, a dialog asks for the folder and saves it (not for `--check`).

mod animation;
mod trails;
mod assets;
mod camera;
mod charmenu;
mod check;
mod duel;
mod effects;
mod skidmarks;
mod shockwaves;
mod energon;
mod vortex;
mod giant;
mod sky;
mod ocean;
mod ssd;
mod flightparts;
mod fx;
mod hud;
mod level;
mod player;
mod rumble;
mod sound;
mod sounddebug;
mod speedblur;
mod postprocess;
mod ssao;
mod surface;
mod switcher;
mod texture_upscale;
mod targets;
mod weapons;
mod world;

// The decoders, the movement, the cameras, the sound mixer and the weapon rules live in
// `tf2-core`, which has no engine types; these imports keep the `crate::formats`,
// `crate::sim`... paths used across the test.
use tf2_core::{character, control, formats, sim, tuning};

use std::path::PathBuf;

use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::animation::{Animator, VehicleAnimator};
use crate::character::CharacterData;
use crate::player::{Controls, Player, RobotModel, TuningRes, VehicleModel};

use tf2_core::formats::characterlist::Spec as CharacterSpec;

/// `--fps`: the seconds since `main` began at each step of the start (a performance aid).
pub fn startup_mark(step: &str) {
    static START: std::sync::LazyLock<(std::time::Instant, bool)> =
        std::sync::LazyLock::new(|| (std::time::Instant::now(), std::env::args().any(|a| a == "--fps")));
    if START.1 {
        eprintln!("startup: {step} at {:.2} s", START.0.elapsed().as_secs_f32());
    }
}

fn first_frame_mark(mut seen: Local<bool>) {
    if !std::mem::replace(&mut *seen, true) {
        startup_mark("first frame");
    }
}
/// The character started when `--character` names none.
const CHARACTER_PACK: &str = "bumblebee";
const CHARACTER_NAME: &str = "Bumblebee";
/// Seconds to let the scene settle before a capture, and to wait for the file after it.
const SHOT_DELAY: f32 = 2.5;
const SHOT_TIMEOUT: f32 = 10.0;

#[derive(Resource)]
struct GameData(CharacterData);

/// A clip (set, id) and the tick to hold the robot at.
#[derive(Resource)]
struct HeldClip(String, String, f32);

/// How he starts.
#[derive(Resource)]
struct Start(sim::State);

#[derive(Resource)]
struct Shot {
    path: String,
    delay: f32,
    taken_at: Option<f32>,
    fx_stats: bool,
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

/// Every node of the given class in a data tree (depth-first, bounded). [data]
fn find_type<'a>(node: formats::lxb::Node<'a>, class: &str, depth: u32, out: &mut Vec<formats::lxb::Node<'a>>) {
    if depth > 48 {
        return;
    }
    if node.type_name() == Some(class) {
        out.push(node);
        return;
    }
    for (_, child) in node.fields() {
        find_type(child, class, depth + 1, out);
    }
    for child in node.items() {
        find_type(child, class, depth + 1, out);
    }
}

/// The CombatRepair heal areas of a character's pack: each `HealAreaEffect` object, with its
/// Lux script name, `healRadius` and slot (`specialHealingType` true is slot 1, Ratchet). [data]
/// The bool is read as its stored integer, 1 for true: `heal_area_effect_init` 007ebf00 reads
/// `specialHealingType` through `view_scalar` 00573c20 into its slot, 1 for Ratchet [game 007ebf00, 00573c20].
fn heal_areas(pack_path: &std::path::Path) -> Vec<targets::HealArea> {
    let mut out = Vec::new();
    let Ok(pack) = formats::pack::Pack::open(pack_path) else { return out };
    for chunk in pack.of_type(formats::pack::DATA) {
        let Ok(file) = formats::lxb::DataFile::parse(pack.data(chunk).to_vec()) else { continue };
        let mut found = Vec::new();
        find_type(file.root(), "HealAreaEffect", 0, &mut found);
        for thing in found {
            let script = thing.get("LuxScriptName").and_then(|n| n.int());
            let radius = thing.get("healRadius").and_then(|n| n.float());
            let healing = thing.get("specialHealingType").and_then(|n| n.int());
            match (script, radius, healing) {
                (Some(script), Some(radius), Some(healing)) => {
                    out.push(targets::HealArea { script: script as u32, radius, slot: usize::from(healing != 0) });
                }
                _ => eprintln!("repair: a HealAreaEffect in {} lacks a field", pack_path.display()),
            }
        }
    }
    out
}

/// The heal fields of a character's attributes, `characterAttributes<name>` in bnxglobal.str.
/// [data] The default (no heal) when the attributes are not found.
fn read_rates(game_dir: &std::path::Path, attributes: &str) -> tf2_core::repair::Rates {
    let Ok(pack) = formats::pack::Pack::open(&game_dir.join("bnxglobal.str")) else { return Default::default() };
    for chunk in pack.of_type(formats::pack::DATA) {
        let Ok(file) = formats::lxb::DataFile::parse(pack.data(chunk).to_vec()) else { continue };
        if let Some(node) = file.root().get(&format!("characterAttributes{attributes}")) {
            return tf2_core::repair::Rates::read(node);
        }
    }
    Default::default()
}

/// The navigation mesh of a mission's packs: the silhouettes of each pack (`navmesh::load`), built
/// as one mesh (`NavMesh::build`). The counts and the build time are logged. [assumed] the bounds
/// are the silhouettes' box plus 10 m. [assumed] The mission record's world_min/world_max (missionlist.rs,
/// decoded) are not used: for us_test they do not contain the silhouettes (x -282..607 against mesh
/// -415..596; y -563..279 against -640..341), so their index order or meaning is not settled
///. None when a pack
/// does not load or the build fails (logged): the enemies then walk straight lines.
fn build_nav(packs: &[PathBuf]) -> Option<std::sync::Arc<tf2_core::navmesh::NavMesh>> {
    let started = std::time::Instant::now();
    let mut sils = Vec::new();
    for pack in packs {
        match tf2_core::navmesh::load(pack) {
            Ok((found, skipped)) => {
                if !skipped.is_empty() {
                    eprintln!("nav: {} silhouettes skipped in {}", skipped.len(), pack.display());
                }
                sils.extend(found);
            }
            Err(e) => {
                eprintln!("nav: {} did not load ({e}); no nav mesh", pack.display());
                return None;
            }
        }
    }
    match tf2_core::navmesh::NavMesh::build(None, &sils) {
        Ok(mesh) => {
            eprintln!(
                "nav: {} silhouettes, {} vertices, {} triangles, built in {:.2} s",
                sils.len(),
                mesh.vertex_count(),
                mesh.tri_count(),
                started.elapsed().as_secs_f64()
            );
            Some(std::sync::Arc::new(mesh))
        }
        Err(e) => {
            eprintln!("nav: build failed ({e}); no nav mesh");
            None
        }
    }
}

/// The flight boundary of a level's zone: the first `FlightBoundary` or `FlightBoundaryEllipsoid`
/// found, its `boundary` block read with `Boundary::read`. [data] A is the inverse of the owner's
/// placement, which `each_thing` gives as its `world` placement (`invert_placement`; the
/// direction is checked by `flybound::tests::a_is_the_inverse_of_the_owner_placement_not_the_placement`).
/// [assumed] the placement `each_thing` gives is the owner's (the thing rides its node at the
/// attachment's bind), in the same 4x3 layout as the block. The 2026-10-12 capture (Starscream
/// never turning back) predates the flight wiring; on 2026-10-18 he leaves through the top and is
/// brought back level near 136 m, the box top this placement gives (owner z 68.29 + 70) [port
/// capture, not compared]. [game 007e75c0] The writer of A is 007e75c0 (the class's activate, vtable
/// slot 4, read 2026-10-20): it writes `obj+0x64..+0x90` = the inverse (0052ff10) of M, where M is
/// the registered owner's 3x4 matrix at `obj+0xc`+0x10..+0x3c, when `obj+0x14` (the u16 type id that
/// the factory takes from its record's +0x10, 007e7780 l.7-9) is 0. The port's A is that inverse.
/// The loader 011882b0 writes B (`invert_placement(block)`, 0056f2d0 then 00572110). OPEN, narrowed:
/// (1) `each_thing`'s `world` is `compose(bind, owner)` (level_things.rs), so the port's A carries the
/// attachment's bind, and 007e75c0's M is the owner node's own matrix; whether that node's matrix
/// already includes the bind is not read. (2) The `obj+0x14 == 0` branch depends on the record the
/// caller builds for the factory (its +0x10 for FlightBoundary); the other branch (nav instance
/// matrix, 005509d0) is not in the port. Read 2026-10-20 (2nd pass): the only registration of the
/// factory is registrar 00adbf70 (`_DAT_00d41650` = 007e7780, class `fdc81a37`); the record's +0x10
/// is written by the generic thing builder 005557e0 / 00576050, which composes the owner's placement
/// with the link matrix "when +0x14 is nonzero" (ledger). Who builds the FlightBoundary record and
/// with which type id is NOT read. So the branch for this boundary is unknown: its `world` is the
/// bind-composed one in the port. Settles: the record builder for a FlightBoundary thing (its type id
/// at +0x10, 005557e0's caller chain), the node matrix of a level's boundary owner against
/// `each_thing`'s world (the bind), or a recording of a jet at the us_test ceiling.
fn flight_boundary(packs: &[PathBuf]) -> Option<tf2_core::flybound::Boundary> {
    let mut found = None;
    for path in packs {
        let Ok(pack) = formats::pack::Pack::open(path) else { continue };
        for chunk in pack.of_type(formats::pack::DATA) {
            let Ok(file) = formats::lxb::DataFile::parse(pack.data(chunk).to_vec()) else { continue };
            tf2_core::level_things::each_thing(&file, &["FlightBoundary", "FlightBoundaryEllipsoid"], &mut |thing, owner, _| {
                if found.is_some() {
                    return;
                }
                let Some(values) = thing.get("boundary").map(|n| n.floats()) else { return };
                let Ok(block) = <[f32; 12]>::try_from(values.as_slice()) else { return };
                let ellipsoid = thing.type_name() == Some("FlightBoundaryEllipsoid");
                let a = tf2_core::flybound::invert_placement(&owner);
                eprintln!("flight boundary: owner placement {owner:?}");
                found = Some(tf2_core::flybound::Boundary::read(&block, a, ellipsoid));
            });
        }
    }
    found
}

/// Asks for the install folder (`asked`: by `--pick-game-dir`, rather than because it was not
/// found), until a folder that holds `tf2_core::install::MARKER` is picked (a folder one level
/// above it is accepted too) or the dialog is cancelled.
fn pick_install(asked: bool) -> Option<PathBuf> {
    let marker = tf2_core::install::MARKER;
    let why = if asked { "Choose where the game is installed." } else { "Transformers: Revenge of the Fallen was not found on this PC." };
    let welcome = format!("{why}\n\n\
        Pick the folder you installed the PC game into (the one that holds {marker}). \
        It is remembered for later runs. Nothing in it is changed.");
    let go = rfd::MessageDialog::new().set_level(rfd::MessageLevel::Info).set_title("Find the game")
        .set_description(welcome).set_buttons(rfd::MessageButtons::OkCancel).show();
    if go != rfd::MessageDialogResult::Ok {
        return None;
    }
    loop {
        let picked = rfd::FileDialog::new().set_title(format!("The game's install folder (the one that holds {marker})")).pick_folder()?;
        let below = std::fs::read_dir(&picked).into_iter().flatten().flatten().map(|e| e.path());
        if let Some(dir) = std::iter::once(picked.clone()).chain(below).find(|d| tf2_core::install::is_install(d)) {
            return Some(dir);
        }
        let again = rfd::MessageDialog::new().set_level(rfd::MessageLevel::Warning).set_title("Not the game's folder")
            .set_description(format!("{} has no {marker}. Pick the folder the game is installed in.", picked.display()))
            .set_buttons(rfd::MessageButtons::OkCancel).show();
        if again != rfd::MessageDialogResult::Ok {
            return None;
        }
    }
}

fn main() {
    startup_mark("main");
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `--pick-game-dir` and `--game-dir <folder>` are taken out of the arguments, so `--check`
    // stays first.
    let pick = args.iter().position(|a| a == "--pick-game-dir").map(|i| args.remove(i)).is_some();
    if let Some(i) = args.iter().position(|a| a == "--game-dir") {
        let Some(dir) = args.get(i + 1).map(PathBuf::from) else {
            eprintln!("--game-dir needs the folder that holds {}.", tf2_core::install::MARKER);
            std::process::exit(1);
        };
        args.drain(i..i + 2);
        if !tf2_core::install::is_install(&dir) {
            eprintln!("{} has no {}: that is not the game's install folder.", dir.display(), tf2_core::install::MARKER);
            std::process::exit(1);
        }
        match tf2_core::install::remember(&dir) {
            Ok(file) => eprintln!("install: {} (saved in {})", dir.display(), file.display()),
            Err(e) => eprintln!("install: {} (not saved: {e})", dir.display()),
        }
        tf2_core::install::set(dir);
    } else if pick || (tf2_core::install::find().is_none() && args.first().map(String::as_str) != Some("--check")) {
        // Asked for, or nothing found and nothing set (`TF2_GAME_DIR` would have been): ask,
        // unless the run is the windowless check. Cancelled, the usual lookup goes on.
        if let Some(dir) = pick_install(pick) {
            match tf2_core::install::remember(&dir) {
                Ok(file) => eprintln!("install: {} (saved in {})", dir.display(), file.display()),
                Err(e) => eprintln!("install: {} (not saved: {e})", dir.display()),
            }
            tf2_core::install::set(dir);
        }
    }
    let args = args;
    let game_dir = tf2_core::install::dir().to_path_buf();
    if !tf2_core::install::is_install(&game_dir) {
        eprintln!("The game's install was not found{}.", if game_dir.as_os_str().is_empty() { String::new() } else { format!(" at {}", game_dir.display()) });
        eprintln!("Run once with --game-dir <folder>, the folder that holds {}; it is remembered.", tf2_core::install::MARKER);
        std::process::exit(1);
    }
    let texture_scale = texture_upscale::factor(&args).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    if args.first().map(String::as_str) == Some("--check") {
        let pack = args.get(1).map_or(CHARACTER_PACK, String::as_str);
        if let Err(e) = check::run(&game_dir, pack) {
            eprintln!("check failed: {e}");
            std::process::exit(1);
        }
        return;
    }

    // `--character optimus` (or `ironhide:Ironhide`: the pack under `characters\`, then
    // the name its data objects carry, by default the pack's with a capital): another
    // character through the same code. Without the flag he is Bumblebee (`CHARACTER_PACK` /
    // `CHARACTER_NAME`, the port's default start) [port]; a mission starts as its spawn point's
    // character (below). Checked 2026-10-20: outside tests, those two constants are the only
    // Bumblebee names left in the production code (the unpublished notes is the 2026-10-12 survey
    // the roster work has since cleared; per-character state is the unpublished notes).
    // The game's own list of characters (`formats::characterlist`) says which pack, which
    // attributes object and which sound bank a character has: `--character` is looked up
    // there first, by display name ("optimus prime") or by pack ("optimus"). What is not
    // in the list (the enemies) is named by pack, with the attributes name after a colon
    // (`dcomscout:DComScout`).
    let roster = tf2_core::formats::characterlist::CharacterList::load(&game_dir).unwrap_or_else(|e| {
        eprintln!("character list: {e}");
        Default::default()
    });
    // A mission is started as the character its player spawn point names ([data] the
    // point's `attributes` is the hash of a `characterAttributes<X>` name; [assumed] that
    // is who the game starts when the player picks nobody else), unless `--character` says.
    // OPEN, settles: the mission start code that reads the spawn point's `attributes` when no
    // character is picked (not read).
    let mission_character = flag_value(&args, "--mission").filter(|_| flag_value(&args, "--character").is_none()).and_then(|name| {
        let list = tf2_core::formats::missionlist::MissionList::load(&game_dir).ok()?;
        let mission = list.by_hash(tf2_core::formats::hash::crc32(name.as_bytes()))?;
        let packs: Vec<PathBuf> = list.pack_paths(mission).iter().map(|p| game_dir.join(p)).collect();
        let point = tf2_core::level_things::spawn_points(&packs).ok()?.into_iter().next()?;
        roster.by_attributes(point.attributes).map(|e| e.name.clone())
    });
    let character = flag_value(&args, "--character").or(mission_character);
    let spec = match character.as_deref().map(|c| c.split_once(':').unwrap_or((c, ""))) {
        None => roster.find(CHARACTER_PACK).and_then(|e| roster.spec(e)).unwrap_or_else(|| CharacterSpec::by_pack(CHARACTER_PACK, Some(CHARACTER_NAME))),
        Some((name, "")) => roster.find(name).and_then(|e| roster.spec(e)).unwrap_or_else(|| roster.spec_by_pack(name)),
        Some((pack, name)) => CharacterSpec::by_pack(pack, Some(name)),
    };
    eprintln!("character: {} (pack {}, attributes {}, sounds {})", spec.display.as_deref().unwrap_or("not in the list"), spec.pack, spec.attributes, spec.audio);
    // `--p2 <name>`: the second robot F3 brings in (`duel.rs`), named as `--character` names him.
    // Optimus when not said, Bumblebee when he is Optimus.
    let p2_name = flag_value(&args, "--p2").unwrap_or_else(|| if spec.pack == "optimus" { "bumblebee" } else { "optimus" }.to_owned());
    let p2_spec = match p2_name.split_once(':') {
        None => roster.find(&p2_name).and_then(|e| roster.spec(e)).unwrap_or_else(|| roster.spec_by_pack(&p2_name)),
        Some((pack, name)) => CharacterSpec::by_pack(pack, Some(name)),
    };
    let (character_pack, character_name) = (spec.pack.clone(), spec.attributes.clone());
    // The same loading as the robot menu's swap (`charmenu.rs`).
    let charmenu::Loaded { tuning, data, animations } = charmenu::load(&game_dir, &spec, texture_scale, &args).unwrap_or_else(|e| {
        eprintln!("Could not read the game data under {}: {e}", game_dir.display());
        eprintln!("Run with --game-dir <folder>, the folder that holds {}.", tf2_core::install::MARKER);
        std::process::exit(1);
    });

    let number = |flag: &str| flag_value(&args, flag).and_then(|v| v.parse::<f32>().ok());
    let level = |flag: &str, default: usize| number(flag).map_or(default, |v| v as usize);
    // `--start x,y,yaw` puts him somewhere else to begin with (yaw in degrees, 0 along +y).
    let mut start = sim::State::new(&tuning);
    // [trace] The user's2026-10-07 capture has4.8s turbo and1.6s cooldown,
    // matching level3 of both installed upgrade tables. Save import is still absent.
    let turbo_upgrade = level("--turbo-upgrade", 3).min(3) as u8;
    start.turbo_upgrades = [turbo_upgrade; 2];
    // [stand-in] Arena difficulty selector; importing the original save is not implemented.
    // QUEUED, settles: a recording of the difficulty the game starts on (its save or options).
    let difficulty=flag_value(&args,"--difficulty").unwrap_or_else(||"medium".into());
    let difficulty_index=match difficulty.to_ascii_lowercase().as_str() {"easy"=>0,"medium"=>1,"hard"=>2,"expert"=>3,_=>panic!("--difficulty must be easy, medium, hard or expert")};
    start.difficulty=tuning.damage.difficulties[difficulty_index];
    if let Some(spec) = flag_value(&args, "--start") {
        let v: Vec<f32> = spec.split(',').filter_map(|p| p.parse().ok()).collect();
        if let [x, y, yaw] = v[..] {
            start.pos = Vec3::new(x, y, 0.0);
            start.yaw = yaw.to_radians();
        }
    }
    // `--level levels/shanghai_01/zone.str`: one of the game's own zones in place of the
    // arena (its collision only: `tf2_core::level_world`). He starts on whatever is highest
    // under the `--start` spot. [stand-in] the arena's scouts still spawn where the arena
    // had them.
    // Its drawn models (`level.rs`), unless `--no-level-models`.
    //
    // `--mission Shanghai_01_Construction_A_CSA`: one of the game's missions by its name in
    // the mission list (`formats::missionlist`): every pack its `streamFiles` names (the
    // zone, then the mission's own pack) as one level, and he starts at the mission pack's
    // `PlayerSpawnPoint` unless `--start` says otherwise. [stand-in] nothing of the mission
    // itself runs: no script, no enemies of its own, no objectives; the spawn point's own
    // character (Optimus in the packs looked at) is not who is spawned.
    let mut level_packs: Vec<std::path::PathBuf> = flag_value(&args, "--level").map(|zone| game_dir.join(zone)).into_iter().collect();
    let mut level_name: Option<String> = None;
    if let Some(name) = flag_value(&args, "--mission") {
        let list = tf2_core::formats::missionlist::MissionList::load(&game_dir).expect("Could not load the mission list");
        let mission = list.by_hash(tf2_core::formats::hash::crc32(name.as_bytes())).unwrap_or_else(|| panic!("--mission: no mission named {name} (the name is case sensitive)"));
        level_packs = list.pack_paths(mission).iter().map(|p| game_dir.join(p)).collect();
        level_name = mission.stream_files.iter().find_map(|f| f.to_ascii_lowercase().strip_suffix("/zone").and_then(|f| f.strip_prefix("levels/")).map(str::to_owned));
        let points = tf2_core::level_things::spawn_points(&level_packs).expect("Could not read the mission's spawn points");
        match points.first().filter(|_| flag_value(&args, "--start").is_none()) {
            Some(point) => {
                eprintln!("mission {name}: starting at {} {:?}", point.name, point.pos);
                // Who the mission means to be played, said and not yet acted on.
                let named = roster.by_attributes(point.attributes).or_else(|| roster.by_id(point.attributes)).map(|e| e.name.as_str());
                let usable: Vec<&str> = mission.usable_characters_ids.iter().filter_map(|&id| roster.by_id(id).map(|e| e.name.as_str())).collect();
                eprintln!("mission {name}: the spawn point's character is {named:?} ({:#010x}); the list's usable characters: {usable:?} of {}", point.attributes, mission.usable_characters_ids.len());
                start.pos = point.pos;
                start.yaw = point.yaw;
            }
            None => eprintln!("mission {name}: {} spawn points, starting at --start", points.len()),
        }
    }
    let level_models = (!level_packs.is_empty() && !args.iter().any(|a| a == "--no-level-models")).then(|| {
        std::sync::Arc::new(tf2_core::level_render::load_packs(&level_packs).expect("Could not load the level's models"))
    });
    // `--mission`: the mission's nav mesh, built once here (phase 3). None when it does not build. The level
    // walks it for the climb's walls (`LevelWorld::with_nav`), so it is built before the level.
    let nav = flag_value(&args, "--mission").is_some().then(|| build_nav(&level_packs)).flatten();
    let level = (!level_packs.is_empty()).then(|| {
        let level = tf2_core::level_world::LevelWorld::load_packs(&game_dir, &level_packs).expect("Could not load the level's collision").with_nav(nav.clone());
        // Test aid: choose a floor below this height for captures in multi-storey areas.
        let probe_height = number("--start-height").unwrap_or_else(|| if level_name.is_some() && flag_value(&args, "--start").is_none() { start.pos.z + 2.0 } else { 1000.0 });
        let floor = sim::World::floor(&level, start.pos.x, start.pos.y, probe_height);
        assert!(floor > tf2_core::level_world::NO_FLOOR, "--level: nothing to stand on under --start");
        start.pos.z = floor;
        std::sync::Arc::new(level)
    });
    // `--mission <name> --spawners [name]`: the mission's enemy spawners (phase 3). [stand-in] they are
    // enabled by a stand-in message 1 s after the load, not by mission scripting. `[name]` takes only
    // the spawners whose name contains it. Their data: `mission.str` chunk 0 (`ai_spawn::load`).
    let spawner_res = if args.iter().any(|a| a == "--spawners") {
        if flag_value(&args, "--mission").is_none() {
            eprintln!("--spawners needs --mission: ignored");
            None
        } else {
            let filter = flag_value(&args, "--spawners").filter(|v| !v.starts_with("--"));
            let mut found = Vec::new();
            for pack in level_packs.iter().filter(|p| p.file_name().is_some_and(|n| n == "mission.str")) {
                match tf2_core::ai_spawn::load(pack) {
                    Ok(spawners) => found.extend(spawners),
                    Err(e) => eprintln!("spawner: {} did not load ({e})", pack.display()),
                }
            }
            if found.is_empty() {
                eprintln!("spawners: none found in the mission's packs");
            }
            Some(targets::SpawnRes::new(&game_dir, found, filter))
        }
    } else {
        None
    };
    // The level's flight boundary (none in the test arena). [data]
    start.flight_boundary = flight_boundary(&level_packs);
    // Printed for the checkers: where he starts and which way he faces (forward is (-sin yaw, cos yaw)). [data]
    println!("spawn: pos ({:.2}, {:.2}, {:.2}) yaw {:.1} deg forward ({:.3}, {:.3})", start.pos.x, start.pos.y, start.pos.z, start.yaw.to_degrees(), -start.yaw.sin(), start.yaw.cos());
    let mut rig = camera::Rig::new(&start, &tuning, number("--yaw"), number("--elev"));
    rig.camera.options.sensitivity=number("--camera-sensitivity").unwrap_or(0.5).clamp(0.0,1.0);
    rig.camera.options.invert_yaw=args.iter().any(|a|a=="--invert-camera-x");
    rig.camera.options.invert_pitch=args.iter().any(|a|a=="--invert-camera-y");
    rig.camera.options.separate_distances=args.iter().any(|a|a=="--separate-camera-distances");
    rig.camera.options.disable_stick=args.iter().any(|a|a=="--disable-camera-stick");
    rig.camera.options.widescreen = !args.iter().any(|a|a=="--camera-4-3");
    rig.orbit = number("--orbit").unwrap_or(0.0);
    rig.orbit_pivot = number("--orbit-pivot").unwrap_or(11.0);
    rig.zoom = number("--zoom").unwrap_or(1.0).max(0.1);
    // `--enemy dcomwarrior:DComWarrior`: who stands in the arena as the enemy (the scout
    // when not said). [stand-in] three of them, with the scout's stand-in brain.
    // [assumed, test aid] a Decepticon player gets the Autobot scout (`aprotscoutmp:APrtScout`): the
    // game filters hits between the same team (`damage::receive`, 0072...: team equal and no
    // 0x4200_0000 flag), so against the Decepticon scout half the roster could neither hurt nor be hurt.
    let dcom_team = tf2_core::tuning::load(&game_dir, "DComScout").map(|t| t.damage.team).ok();
    let enemy = match flag_value(&args, "--enemy").as_deref().map(|c| c.split_once(':').unwrap_or((c, ""))) {
        None if dcom_team == Some(tuning.damage.team) => {
            println!("enemies: {} is on the scouts' side; the arena uses the Autobot scout", tuning.name);
            CharacterSpec::by_pack("aprotscoutmp", Some("APrtScout"))
        }
        None => CharacterSpec::by_pack("dcomscout", Some("DComScout")),
        Some((name, "")) => roster.find(name).and_then(|e| roster.spec(e)).unwrap_or_else(|| CharacterSpec::by_pack(name, None)),
        Some((pack, name)) => CharacterSpec::by_pack(pack, Some(name)),
    };
    let scout = tf2_core::combat::Scout::load_as(&game_dir, &enemy.pack, &enemy.attributes).unwrap_or_else(|e| panic!("Could not load the enemy {}: {e}", enemy.pack));
    if let Some(level) = &level_name {
        sound::set_stage(level);
    }
    startup_mark("enemy loaded");
    let sound = if args.iter().any(|a| a == "--no-sound") { None } else { sound::start(&game_dir, &spec, &enemy, &data, &animations, &tuning, &scout) };
    startup_mark("sound started");
    let arsenal = charmenu::arm(&game_dir, &character_name, &data, tuning.giant.as_ref());
    // The robot menu (F2): the roster, swapped in the running game.
    let robot_menu = charmenu::Menu::new(&game_dir, texture_scale, &args, &roster, &enemy, switcher::index_of(character.as_deref()));
    startup_mark("weapons and robot menu");
    // [assumed] with spawners (`--spawners`) the arena's three default scouts are not made unless `--enemy-at`
    // asks for the first one: the mission's own spawners make the enemies (phase 3, the first fault of the brief).
    // The arena starts empty (user, 2026-10-21): `--scouts` brings back the three scouts.
    let arena_enemies = (spawner_res.is_none() && args.iter().any(|a| a == "--scouts")) || flag_value(&args, "--enemy-at").is_some();
    if !arena_enemies {
        println!("enemies: the arena's default scouts are not created (--scouts makes them)");
    }
    // `--enemy-at x,y`, one enemy per flag occurrence [assumed]. Parsed here in order.
    let enemy_at: Vec<(f32, f32)> = args
        .windows(2)
        .filter(|w| w[0] == "--enemy-at")
        .filter_map(|w| {
            let (x, y) = w[1].split_once(',')?;
            Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?))
        })
        .collect();
    // [assumed] With --mission the arena's three fixed scouts are not made (check 4 of 2026-10-13: they were
    // removed "no floor" on the level). Only the enemies placed by --enemy-at are made: one per occurrence,
    // ids 1.. in the order given. Without --mission the arena keeps its scouts as before.
    let mission_mode = flag_value(&args, "--mission").is_some();
    let mut enemy_layout = if mission_mode {
        let template = targets::layout(&scout.tuning).into_iter().next();
        enemy_at
            .iter()
            .zip(1u32..)
            .filter_map(|(&(x, y), id)| {
                let mut t = template?;
                t.id = id;
                t.pos = Vec3::new(x, y, 0.0);
                Some(t)
            })
            .collect()
    } else if arena_enemies {
        targets::layout(&scout.tuning)
    } else {
        Vec::new()
    };
    if mission_mode {
        println!("enemies: {} placed by --enemy-at (the mission makes no fixed scouts)", enemy_layout.len());
    } else if let (Some(&(x, y)), Some(first)) = (enemy_at.first(), enemy_layout.first_mut()) {
        // Without --mission: the first arena enemy stands at the given position instead of its fixed one.
        first.pos = Vec3::new(x, y, 0.0);
        println!("enemy: first enemy placed at ({x:.2}, {y:.2})");
    }
    // Four independent reads of the install, side by side: one after another they took 1.6 s of
    // the start (each opens and inflates its own pack; performance audit, 2026-10-23).
    let level_zone = level_name.as_deref().unwrap_or(world::LEVEL);
    let (heal, ssd_textures, turbo_blur, look) = std::thread::scope(|s| {
        let heal = s.spawn(|| {
            let heal = targets::HealConfig {
                areas: heal_areas(&game_dir.join("characters").join(format!("{character_pack}.str"))),
                user: read_rates(&game_dir, &character_name),
                enemy: read_rates(&game_dir, &enemy.attributes),
            };
            startup_mark("  heal rates read");
            heal
        });
        let ssd = s.spawn(|| ssd::load(&game_dir).inspect(|_| startup_mark("  surface damage read")));
        let blur = s.spawn(|| speedblur::load(&game_dir, args.iter().any(|a| a == "--no-speed-blur"),
            args.iter().any(|a| a == "--speed-blur-report")).inspect(|_| startup_mark("  turbo blur read")));
        // The arena's light and look: a level's own environment (`world::LEVEL`), unless asked not to.
        // A mission is lit by its own zone's default environment; the zone's environments are for
        // EnvironmentTrigger blends (`postprocess::EnvironmentBlend`).
        let look = s.spawn(|| (!args.iter().any(|a| a == "--no-level-look")).then(|| {
            let zone = game_dir.join("levels").join(level_zone).join("zone.str");
            let look = (tf2_core::formats::environment::load(&game_dir, level_zone), tf2_core::environment_blend::zone_specs(&zone));
            startup_mark("  level look read");
            look
        }));
        (heal.join().expect("reading the heal rates panicked"), ssd.join().expect("reading surface damage panicked"),
            blur.join().expect("reading the turbo blur panicked"), look.join().expect("reading the level look panicked"))
    });
    let encounter = targets::Encounter {
        scout: std::sync::Arc::new(scout),
        passive: args.iter().any(|a|a=="--passive-enemies"),
        report: args.iter().any(|a|a=="--combat-report"),
        // The game's own brain by default (user, 2026-10-20); `--stand-in-brain` brings back the old one.
        ai_brain: !args.iter().any(|a|a=="--stand-in-brain"),
        heal,
    };
    let ssd_textures = ssd_textures.expect("Could not load the original surface damage textures");
    let turbo_blur = turbo_blur.expect("Could not load original turbo blur data");
    if let Some((environment, specs)) = look {
        world::set_look(environment.inspect_err(|e| eprintln!("No level look: {e}")).ok());
        world::set_env_specs(specs.inspect_err(|e| eprintln!("No environment specs: {e}")).unwrap_or_default());
    }
    startup_mark("heal rates, surface damage, blur and level look");
    let controls = Controls {
        // `--vehicle` holds the trigger throughout; the two times bound it instead.
        vehicle_window: (args.iter().any(|a| a == "--vehicle") || number("--vehicle-from").is_some())
            .then(|| (number("--vehicle-from").unwrap_or(0.0), number("--vehicle-until").unwrap_or(f32::MAX))),
        force_stick: Vec2::new(number("--stick-x").unwrap_or(0.0), number("--stick").unwrap_or(0.0)),
        stick_from: number("--stick-from"),
        stick_until: number("--stick-until"),
        force_look: Vec2::new(number("--look-x").unwrap_or(0.0), number("--look").unwrap_or(0.0)),
        look_from: number("--look-from"),
        look_until: number("--look-until"),
        jump_at: flag_value(&args, "--jump-at").and_then(|v| v.parse().ok()),
        jump_hold_from: number("--jump-hold-from"),
        die_at: number("--die-at"),
        keep_dead: args.iter().any(|a|a=="--keep-dead"),
        hit_at: number("--hit-at"),
        hit_flags: flag_value(&args, "--hit-flags").and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()).unwrap_or(0),
        fire_from: number("--fire-from"),
        fire_until: number("--fire-until"),
        aim_from: number("--aim-from"),
        aim_cycle: number("--aim-cycle"),
        // Taps of the melee button, at times given as `0.5,1.1,1.7`.
        melee_at: flag_value(&args, "--melee-at")
            .map(|v| v.split(',').filter_map(|t| t.parse().ok()).collect())
            .unwrap_or_default(),
        melee_hold_from: number("--melee-hold-from"),
        melee_hold_until: number("--melee-hold-until"),
        climb_from: number("--climb-from"),
        force_look_y: number("--look-y").unwrap_or(0.0),
        // Taps of the weapon switch, at times given as `1,1.4`.
        switch_at: flag_value(&args, "--switch-at")
            .map(|v| v.split(',').filter_map(|t| t.parse().ok()).collect())
            .unwrap_or_default(),
        camera_cycle_at: number("--camera-cycle-at"),
        recentre_at: number("--recentre-at"),
        zoom_in_from: number("--zoom-in-from"),
        special_at: number("--special-at"),
        classic: args.iter().any(|a| a == "--classic-controls"),
        mouse_degrees: number("--mouse-sens").unwrap_or(tf2_core::input::MOUSE_DEGREES_DEFAULT),
        ..Default::default()
    };
    // `--clip Set/Id@tick` holds the robot in one frame of one clip.
    let held = flag_value(&args, "--clip").and_then(|spec| {
        let (name, tick) = spec.split_once('@').unwrap_or((&spec, "0"));
        let (set, id) = name.split_once('/')?;
        Some(HeldClip(set.to_owned(), id.to_owned(), tick.parse().unwrap_or(0.0)))
    });

    // Read once: the run conditions below are polled every frame.
    let fps = args.iter().any(|a| a == "--fps");
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Transformers RotF data test (Bevy)".into(),
            resolution: (1280, 720).into(),
            // `--no-vsync`: frames as fast as they come, so `--fps` gives the true frame time (a performance aid).
            present_mode: if args.iter().any(|a| a == "--no-vsync") { bevy::window::PresentMode::AutoNoVsync } else { bevy::window::PresentMode::AutoVsync },
            ..default()
        }),
        ..default()
    }))
    .add_plugins(surface::plugin)
    .add_plugins(sky::plugin)
    .add_plugins(ocean::plugin)
    .add_plugins(fx::plugin)
    .add_plugins(ssd::plugin)
    .add_plugins(speedblur::plugin)
    .add_plugins(postprocess::plugin)
    .add_plugins(ssao::plugin);
    startup_mark("plugins");
    app.init_resource::<UpdateClock>()
    .add_systems(First, update_clock_start.run_if(move || fps))
    .add_systems(Last, update_clock_end.run_if(move || fps))
    .add_systems(Update, (fps_report, first_frame_mark).run_if(move || fps))
    // `--fps` measures at full speed with the window unfocused too (Bevy's default slows an unfocused
    // window to 60 updates a second, which reads as a 16.7 ms frame).
    .insert_resource(if args.iter().any(|a| a == "--fps") { bevy::winit::WinitSettings::continuous() } else { bevy::winit::WinitSettings::default() })
    .add_systems(Update, switcher::run.after(player::tick))
    // F3: a second robot in a second window (`duel.rs`).
    .insert_resource(duel::Duel::new(game_dir.clone(), p2_spec))
    .add_systems(Update, (
        duel::toggle.before(duel::drive),
        duel::drive.after(player::read_input).before(targets::update),
        duel::follow.after(targets::update).before(targets::animate),
        duel::forms.after(targets::update),
    ))
    .insert_resource(robot_menu)
    .add_systems(Startup, charmenu::spawn)
    .add_systems(Update, (
        charmenu::apply.before(player::read_input),
        charmenu::run.after(camera::grab_cursor).before(player::tick),
        charmenu::draw.after(charmenu::run),
    ))
    .insert_resource(turbo_blur)
    .insert_resource(ssd_textures)
    .init_resource::<ssd::Marks>()
    .insert_resource(ssd::Report(args.iter().any(|a|a=="--ssd-report")))
    .add_systems(Update,ssd::run.after(player::tick))
    .insert_resource(world::ArenaRes(match level {
        Some(level) => sim::Arena { targets: enemy_layout, level: Some(level), ..Default::default() },
        None => sim::Arena { targets: enemy_layout, ..world::layout(&tuning) },
    }))
    .insert_resource(encounter)
    .insert_resource(targets::NavRes(nav))
    .insert_resource(TuningRes(tuning))
    .insert_resource(GameData(data))
    .insert_resource(animations)
    .init_resource::<trails::Trails>()
    .add_systems(PostUpdate,trails::run.after(bevy::transform::TransformSystems::Propagate))
    // The projectiles' ribbons ride the arsenal's frames, not posed nodes.
    .add_systems(PostUpdate,trails::run_projectiles)
    // Devastator's pound rings (none for anyone else).
    .add_systems(PostUpdate,shockwaves::run)
    // His vortex's funnel and sound (none for anyone else).
    .add_systems(PostUpdate,vortex::run.before(bevy::transform::TransformSystems::Propagate))
    // A giant's sun shadow reaches past his far camera (`giant.rs`).
    .add_systems(Update,(giant::shadows,giant::fill,giant::pods,giant::tracers,giant::reactions,giant::hurt_aid))
    // His dressing: overheat smoke, the eruption's launch, his damage and his death's sound (`giant::dress`).
    .add_systems(Update,giant::dress.after(player::tick))
    // Grindor's cinematic wave's impact and front (none for anyone else) [port design].
    .add_systems(Update,energon::dress.after(player::tick).before(effects::run))
    .insert_resource(rig)
    .init_resource::<camera::Occluders>()
    .add_systems(Update,camera::fade_occluders.after(player::tick))
    .insert_resource(controls)
    .insert_resource(Start(start))
    .add_systems(Startup, (ssd::setup, world::spawn, targets::spawn, spawn_player, spawn_camera).chain())
    .add_systems(Startup, effects::start_level.after(spawn_player))
    .insert_resource(targets::ShowBodies(args.iter().any(|a| a == "--bodies")))
    .add_systems(Update, (targets::draw_bodies, targets::update).after(player::tick).before(player::present))
    .add_systems(Update, targets::animate.after(targets::update))
    // The one despawn of an enemy: after the spawners, the cull and the animation (see targets::remove_marked).
    .add_systems(Update, targets::remove_marked.after(targets::update).after(targets::animate))
    .init_resource::<effects::Effects>()
    .init_resource::<skidmarks::SkidMarks>()
    .add_systems(Update, skidmarks::run.after(player::tick))
    .add_systems(Update, assets::update_render_sets.after(player::tick))
    .add_systems(Update, assets::animate_ifls.after(player::tick))
    .add_systems(Update, flightparts::blade_fade.after(player::tick))
    .add_systems(Update, (effects::start.after(animation::animate), effects::run.after(effects::start).after(camera::apply)))
    .init_resource::<rumble::PadRumble>()
    .add_systems(Update, rumble::play.after(player::tick))
    .add_systems(
        Update,
        (player::read_input, camera::grab_cursor, camera::cycle_distance, camera::collect_occluders, player::tick, player::present, animation::animate, camera::apply).chain(),
    );
    if let Some(models) = level_models {
        app.insert_resource(level::LevelModels(models)).add_systems(Startup, level::spawn)
            .add_systems(Update, level::update.after(camera::apply));
    }
    if let Some(spawners) = spawner_res {
        app.insert_resource(spawners).add_systems(Update, targets::spawn_run.after(targets::update).before(targets::remove_marked));
    }
    // With a mission loaded, an enemy with no floor under its placement (not a spawned one, which is held), or one that falls far below it, is marked for removal.
    if level_name.is_some() {
        app.add_systems(Update, targets::cull_unplaced.after(targets::update).before(targets::remove_marked));
    }
    if let Some(sound) = sound {
        app.insert_resource(sound);
    }
    // The weapon systems are always there and skip while the robot has no arsenal: the robot
    // menu (F2) takes it away when it swaps to one with no weapons (Devastator), and back.
    if let Some(arsenal) = arsenal {
        app.insert_resource(arsenal);
    }
    {
        let armed = bevy::ecs::schedule::common_conditions::resource_exists::<weapons::Arsenal>;
        app.add_systems(Startup, weapons::attach_models.after(spawn_player).run_if(armed))
            .add_systems(Update, (weapons::draw, weapons::draw_turrets, weapons::show_models, weapons::show_axe, weapons::show_special_models).after(player::tick).run_if(armed))
            // More projectile models when every one is carrying a projectile (no game limit). [port]
            .init_resource::<weapons::PoolDemand>()
            .init_resource::<weapons::PoolCache>()
            .add_systems(Update, weapons::grow_pools.after(weapons::draw).run_if(armed))
            // The pieces a blast object throws (Bumblebee's missile's chunks), drawn with their own models.
            .init_resource::<weapons::DebrisDemand>()
            .add_systems(Update, (weapons::draw_debris.after(player::tick), weapons::grow_debris.after(weapons::draw_debris).after(weapons::grow_pools)).run_if(armed));
    }
    if let Some(held) = held {
        app.insert_resource(held);
    }
    // Each weapon's muzzle light, the flash object's omni Light [game 007aac80].
    app.insert_resource(effects::FlashLights(
        tf2_core::point_light::load_flash_lights(&game_dir, &character_pack).unwrap_or_default(),
    ));
    if !args.iter().any(|a| a == "--no-hud") {
        if let Some(source) = hud::ReticleSource::load(&game_dir) {
            app.insert_resource(source);
        }
        if let Some(flash) = hud::FlashHud::load(&game_dir) {
            app.insert_resource(flash);
        }
        app.add_systems(Startup, hud::spawn).add_systems(
            Update,
            (hud::toggle_help, hud::update, hud::health, hud::draw_reticle.after(player::tick), hud::draw_meters.after(player::tick), hud::scope_overlay.after(player::tick)),
        );
    }
    // F8: every sound playing (`sounddebug.rs`).
    app.insert_resource(sounddebug::SoundDebug::new(args.iter().any(|a| a == "--sound-debug")))
        .add_systems(Startup, sounddebug::spawn)
        .add_systems(Update, sounddebug::run.after(player::tick));
    if let Some(path) = flag_value(&args, "--shot") {
        let delay = flag_value(&args, "--after").and_then(|v| v.parse().ok()).unwrap_or(SHOT_DELAY);
        app.insert_resource(Shot { path, delay, taken_at: None,fx_stats:args.iter().any(|a|a=="--fx-stats") })
            .add_systems(Update, take_shot.after(effects::run));
    }
    startup_mark("app built");
    app.run();
}

fn spawn_player(
    mut commands: Commands,
    data: Res<GameData>,
    start: Res<Start>,
    held: Option<Res<HeldClip>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surfaces: ResMut<Assets<surface::Surface>>,
    mut glows: ResMut<Assets<fx::Glow>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let mut cache = assets::Cache::default();
    let mut builder = assets::Builder {
        library: &data.0.library,
        meshes: &mut meshes,
        materials: &mut materials,
        surfaces: &mut surfaces,
        glows: &mut glows,
        images: &mut images,
        bindposes: &mut bindposes,
        cache: &mut cache,
    };
    let (robot, rest) = builder.spawn(&mut commands, &data.0.robot, "robot");
    let mut animator = Animator::new(&rest);
    animator.forced = held.map(|h| (h.0.clone(), h.1.clone(), h.2));
    commands.entity(robot).insert((RobotModel, animator));
    // Each weapon's own model; `weapons::attach_models` hangs it on its node, whose space
    // is already the game's, so the root's turn into Bevy's space is taken off again.
    // ONE model per weapon list: the weapons of a list share its object (`WeaponDef::list`),
    // as his cannon and launcher do, or Optimus's right rifle and cannon. [game] [data]
    let mut spawned_lists = Vec::new();
    for (index, (def, object)) in data.0.weapons.iter().zip(&data.0.weapon_objects).enumerate() {
        // A turret launcher's object is not hung on his hand: his special throws the turret. [game]
        if def.kind == tf2_core::weapons::Kind::Turret || spawned_lists.contains(&def.list) {
            continue;
        }
        let Some(object) = object else { continue };
        spawned_lists.push(def.list);
        let (model, _) = builder.spawn(&mut commands, object, "weapon");
        let (names, indices) = data.0.weapons.iter().enumerate()
            .filter(|(_, d)| d.list == def.list && d.kind != tf2_core::weapons::Kind::Turret)
            .map(|(i, d)| (d.name, i))
            .unzip();
        let hung = weapons::WeaponModel { name: def.name, names, indices, bone: def.bone, weapon: index };
        commands.entity(model).insert((hung, Transform::IDENTITY, Visibility::Hidden));
    }
    // Each grenade weapon's projectile model, a pool per weapon, hidden until `weapons::draw`
    // flies a grenade with it. OPEN: `read_grenade_objects` takes the weapon's `grenadeProjectileObj`
    // [data], the field the explosion reads. The launch `GrenadeProjectile_launch` 00789b80 instances
    // no object (it sets the record's velocity, arc and body; read 2026-10-20). `FUN_0078adc0` is
    // NOT the create (it sets +0x1e0 = 8 on a list). Settles: the projectile-record factory that
    // reads `grenadeProjectileObj`, not read.
    for (def, object) in data.0.weapons.iter().zip(&data.0.grenade_objects) {
        let Some(object) = object else { continue };
        for slot in 0..weapons::GRENADE_POOL {
            let (model, _) = builder.spawn(&mut commands, object, "grenade");
            commands.entity(model).insert((weapons::GrenadeModel { name: def.name, slot, carrying: None }, Visibility::Hidden));
        }
    }
    // Each turret launcher's deployed-turret model and each missile launcher's missile model, a
    // pool per weapon, hidden until `weapons::draw_turrets` / `weapons::draw` use them. [data]
    for (index, (def, object)) in data.0.weapons.iter().zip(&data.0.turret_objects).enumerate() {
        let Some(object) = object else { continue };
        for slot in 0..weapons::TURRET_POOL {
            let (model, _) = builder.spawn(&mut commands, object, "turret");
            commands.entity(model).insert((weapons::TurretModel { name: def.name, weapon: index, slot }, Visibility::Hidden));
        }
    }
    // A deployed turret's launcher (Ironhide's) is no weapon of his list: its missiles are named by the
    // turret's row key (`turret_gun_key`), as the arsenal fires them. [data]
    let turret_launchers = data.0.weapons.iter().zip(&data.0.turret_parts)
        .filter(|(_, parts)| parts.missile.is_some())
        .map(|(def, _)| tf2_core::character::turret_gun_key(def.name));
    let names: Vec<u32> = data.0.weapons.iter().map(|def| def.name).chain(turret_launchers).collect();
    for name in names {
        let Some(object) = data.0.missile_object(name) else { continue };
        for slot in 0..weapons::MISSILE_POOL {
            let (model, _) = builder.spawn(&mut commands, object, "missile");
            commands.entity(model).insert((weapons::MissileModel { name, slot, carrying: None }, Visibility::Hidden));
        }
    }
    // Each melee weapon's model (Optimus's and the Audi's two blades); `weapons::show_axe`
    // hangs it on the node its events name. [game 007b4680: every slot] [data]
    for (index, weapon) in data.0.melee_weapons.iter().enumerate() {
        let Some(object) = weapon.object.as_ref() else { continue };
        let (model, _) = builder.spawn(&mut commands, object, "axe");
        commands.entity(model).insert((weapons::AxeModel { index, hung: 0 }, Transform::IDENTITY, Visibility::Hidden));
    }
    let player = commands
        .spawn((Player::new(start.0.clone()), Transform::default(), Visibility::default()))
        .add_child(robot)
        .id();
    if let Some(vehicle) = &data.0.vehicle {
        let (vehicle, _) = builder.spawn(&mut commands, vehicle, "vehicle");
        commands.entity(vehicle).insert((VehicleModel, VehicleAnimator::default(), Visibility::Hidden));
        commands.entity(player).add_child(vehicle);
    } else {
        // A character with no second object: an empty stand-in so the systems that drive the
        // robot's and the vehicle's clips together still run. [data] The Character's
        // `transformModel.vehicleProxy` (the loader's second object, core `character.rs` 460) is
        // `None` in megatronflight.str and optimusflight.str (`tf2_lxb.py dump <pack>#0`, 2026-10-20);
        // starscream, jetfire, firestorm and blackout have one (`bumblebeevehicle`-style embedded
        // object). [data characters/soldiermale.str #0: `vehicleProxy: None`, `tf2_lxb.py dump`, 2026-10-20]
        // the human soldier (`soldiermale`, the HUMAN row of the character list) has no vehicle object
        // too. Whether this spawn is ever given him is not read here.
        // [stand-in] OPEN, settles: the game's null test on the vehicle object pointer C+0x108 in the
        // clip driver that drives the second object: `fn.py callers 0071a2e0` and the reads of
        // [character+0x108] in its callers (not located as the driver). Read 2026-10-20: 0071a2e0
        // reads `*(C+0x108)+4` only after 0070ab90 says the character is in vehicle motion.
        let nodes = assets::ObjectNodes { entities: Vec::new(), frames: Vec::new(), names: Vec::new(), name_hashes: Vec::new(), rest: Vec::new(), parents: Vec::new(), spine_links: Vec::new(), foot_rig: None };
        commands.spawn((player::NoVehicle, nodes, VehicleAnimator::default(), Visibility::Hidden));
    }
}

/// The one camera; apart from `spawn_player` because the robot menu spawns another robot
/// under the same camera (`charmenu::apply`).
fn spawn_camera(mut commands: Commands, tuning: Res<TuningRes>) {
    let camera = commands.spawn((
        Camera3d::default(),
        // The scene's depth, for the particles that fade where they near what is behind
        // them (`fx.wgsl`).
        DepthPrepass,
        Projection::Perspective(PerspectiveProjection {
            fov: tuning.0.robot_camera.fov.to_radians(),
            // [game] base camera constructor00781a00.
            near: 0.5,
            far: 1500.0,
            ..default()
        }),
        Transform::from_xyz(0.0, 6.0, 14.0).looking_at(Vec3::new(0.0, 3.0, 0.0), Vec3::Y),
    )).id();
    if let Some(look) = world::camera_look() {
        commands.entity(camera).insert(look);
    }
}

/// `--fps`: when the main schedule began this frame, and how long the last frame's took (from
/// `First` to `Last`), so a hitch says whether it was the game's update or the rendering.
#[derive(Resource, Default)]
struct UpdateClock {
    began: Option<std::time::Instant>,
    last: f32,
}

fn update_clock_start(mut clock: ResMut<UpdateClock>) {
    clock.began = Some(std::time::Instant::now());
}

fn update_clock_end(mut clock: ResMut<UpdateClock>) {
    if let Some(began) = clock.began.take() {
        clock.last = began.elapsed().as_secs_f32();
    }
}

/// The process's page faults so far (Windows; 0 elsewhere), for `--fps`.
#[cfg(windows)]
fn page_faults() -> u32 {
    #[repr(C)]
    #[derive(Default)]
    struct Counters { cb: u32, page_faults: u32, rest: [usize; 8] }
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, size: u32) -> i32;
    }
    let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, ..Default::default() };
    // SAFETY: a plain query into a local struct of the size it is told.
    if unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) } != 0 { c.page_faults } else { 0 }
}
#[cfg(not(windows))]
fn page_faults() -> u32 { 0 }

/// `--fps`: the average frame rate and frame time every two seconds, and the slowest frame among
/// them (a performance aid: a hitch hides in the average); each frame over 15 ms or with over 5000
/// page faults is logged. The mean "update" is the main world's First..Last (the game's own systems
/// and Bevy's; the render world runs beside it).
fn fps_report(time: Res<Time>, clock: Res<UpdateClock>, mut window: Local<(f32, u32, f32, f32)>, mut faults: Local<u32>) {
    window.0 += time.delta_secs();
    window.1 += 1;
    window.2 = window.2.max(time.delta_secs());
    window.3 += clock.last;
    let now = page_faults();
    let new = now.wrapping_sub(*faults);
    *faults = now;
    if (time.delta_secs() > 0.015 || new > 5000) && time.elapsed_secs() > 1.0 {
        info!("hitch {:.1} ms at {:.3} s (the frame's update {:.1} ms, {new} page faults)", time.delta_secs() * 1000.0, time.elapsed_secs(), clock.last * 1000.0);
    }
    if window.0 >= 2.0 {
        info!("fps {:.1} ({:.1} ms a frame, update {:.2} ms, slowest {:.1} ms; {now} page faults so far)", window.1 as f32 / window.0, window.0 * 1000.0 / window.1 as f32, window.3 * 1000.0 / window.1 as f32, window.2 * 1000.0);
        *window = (0.0, 0, 0.0, 0.0);
    }
}

fn take_shot(time: Res<Time>, fx: Res<effects::Effects>, player: Single<&player::Player>, arsenal: Option<Res<weapons::Arsenal>>, rig: Option<Res<camera::Rig>>, mut commands: Commands, mut shot: ResMut<Shot>, mut exit: MessageWriter<AppExit>) {
    let now = time.elapsed_secs();
    match shot.taken_at {
        None if now >= shot.delay => {
            let state = &player.state;
            eprintln!("capture pose: t={now:.3} pos ({:.3}, {:.3}, {:.3}) yaw {:.3} deg",
                state.pos.x, state.pos.y, state.pos.z, state.yaw.to_degrees());
            eprintln!("capture state: {:?}, health {:.0}; weapon: {}", state.mode, state.health,
                arsenal.as_ref().map_or_else(|| "none loaded".to_owned(), |a| a.describe()));
            if let Some(rig) = &rig {
                // The eye against him: how far, how high above his root, and the view's pitch.
                let (eye, forward) = (rig.view.eye, rig.view.forward);
                let away = eye - state.pos;
                eprintln!("capture camera: eye ({:.2}, {:.2}, {:.2}), {:.2} m from him ({:.2} flat, {:.2} up), view pitch {:.1} deg",
                    eye.x, eye.y, eye.z, away.length(), away.truncate().length(), away.z, forward.z.clamp(-1.0, 1.0).asin().to_degrees());
            }
            if let Some(climb) = &state.climb {
                let local = climb.wall.local(state.pos);
                let inward = climb.wall.inward();
                eprintln!("capture climb: {:?}; local ({:.3}, {:.3}, {:.3}); inward ({:.6}, {:.6}); base width {:.3}; width {:.3}; extensions L {:.3} R {:.3}; height {:.3}",
                    climb.phase, local.x, local.y, local.z, inward.x, inward.y,
                    climb.wall.base_width, climb.wall.width, climb.wall.left_extension,
                    climb.wall.width - climb.wall.base_width - climb.wall.left_extension, climb.wall.height);
            }
            for wave in &state.shockwaves {
                // A giant's pound rings (`tf2_core::giant`): where each living ring is and how big.
                let rings: Vec<String> = wave.rings.iter().map(|r| format!("({:.1}, {:.1}, {:.1}) r {:.1} a {:.2}",
                    r.centre.x, r.centre.y, r.centre.z, r.radius, wave.alpha(r))).collect();
                eprintln!("capture shockwave {:08x}: {} rings {}", wave.def.name, rings.len(), rings.join("; "));
            }
            for wave in arsenal.as_ref().map_or(&[][..], |a| a.energon_waves()) {
                // Grindor's cinematic waves (`tf2_core::energon_wave`): the arc's centre, radius, band and fade.
                for r in &wave.ring.rings {
                    let (inner, outer) = wave.ring.band(r.radius);
                    // Its ground along the arc's middle: how far it runs there, and the front's place on it.
                    let mid = r.arc.map_or(0.0, |a| a.angle());
                    let front = wave.ground.point(mid, outer);
                    eprintln!("capture energon wave {}: centre ({:.1}, {:.1}, {:.1}) r {:.1} band {:.1}..{:.1} a {:.2} strength {:.2} \
                        | middle reach {:.1} front ({:.1}, {:.1}, {:.1}) open {} ends at r {:.1}",
                        wave.number, r.centre.x, r.centre.y, r.centre.z, r.radius, inner, outer, wave.ring.alpha(r), wave.ring.strength(r.radius),
                        wave.ground.reach(mid), front.x, front.y, front.z, wave.ground.open(), wave.ground.end_radius(&wave.ring));
                }
            }
            if let Some(f) = &state.vortex {
                // His vortex (`tf2_core::giant::Funnel`): its point, axis, scale and own tick, and what it drags.
                let axis = f.axis();
                eprintln!("capture vortex: tip ({:.1}, {:.1}, {:.1}) axis ({:.2}, {:.2}, {:.2}) scale {:.1} tick {:.0}; pulling {:?}",
                    f.tip.x, f.tip.y, f.tip.z, axis.x, axis.y, axis.z, f.scale, f.tick, state.pulls);
            }
            if shot.fx_stats {
                eprintln!("vehicle FX: {} bullet visuals, {} bullet sprites submitted, {} collision visuals",
                    fx.bullet_visuals_started,fx.bullet_sprites_submitted,fx.collision_visuals_started);
                let (emitters, particles) = fx.gun_smoke_stats();
                eprintln!("gun smoke: {emitters} emitting effects, {particles} remaining particles");
                let (emitters, particles) = fx.exhaust_stats();
                eprintln!("car exhaust: {emitters} emitting effects, {particles} remaining particles");
            }
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(shot.path.clone()));
            shot.taken_at = Some(now);
        }
        // Quit once the file is there, or give up waiting for it.
        Some(at) if std::path::Path::new(&shot.path).exists() || now - at > SHOT_TIMEOUT => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}
