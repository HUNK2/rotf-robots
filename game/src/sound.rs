//! Bumblebee's sounds on the default output device.
//!
//! What plays and when is the game's data: his clips carry sound cues (`AnimPlaySoundEvent`,
//! `AnimStopSoundEvent`, `FootStepEvent`), his `soundTable` names the engine and turbo
//! events, and the event files say which samples those are (`tf2_core::sound`). The samples
//! are decoded from the install when the test starts.
//!
//! The engine's revolutions and load are the game's gearbox (`tf2_core::sound::Gearbox`,
//! `FUN_007a48e0`). [game]
//! Footsteps probe the named animated foot and choose the material preset. Sources have
//! independent positions, heard through FMOD Ex's stereo 3D as read from the executable
//! (`tf2_core::sound`: speaker levels, roll-off, pan level, Doppler); the listener is the
//! camera, its velocity the player's motion (00771540). [game] Every mixer's reverb send goes
//! through FMOD's system reverb (`tf2_core::reverb`), set by the `--mission` stage's
//! ReverbTrigger (`set_stage`); the arena has none and stays dry, as a zone without one does.
//! Tyre and suspension cues use their selected rear wheel's position. Shared
//! sounds (steps, jumps, landings, turbo) come from one level's event file.
//! A melee hit sounds by the surface it struck: his `meleeHitSparkPreset` names an event per
//! surface material (`FUN_0072d9c0` -> `FUN_0077cd60`). [game] [data] The surface is the
//! target's, and the dummies' is a stand-in.
//! The tyres' rolling and skid loops and the suspension's thump are the game's rules
//! (`tf2_core::sound::tyre_sounds`, `FUN_007347c0`). [game] Stand-ins: the ground is the
//! default surface everywhere. Actual rear-wheel spring compression drives the thump.
//! Collision reactions use his SoundPhysicsReactor presets; combat chatter uses his
//! installed radio lines, frequency, history and delay. Gameplay sounds only: the user
//! excluded music/ambience. `vehicleDriftStart` is in his data but no code
//! of the game's reads it.

use std::path::Path;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use bevy::prelude::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tf2_core::character::{CharacterData, ImpactPreset, Sounds};
use tf2_core::formats::anim::{Clip, SoundCue};
use tf2_core::sim::State;
use tf2_core::sound::{Handle, IMPACT_FORCE, Library, Mixer, TyreSounds, tyre_sounds};
use tf2_core::tuning::Tuning;
use tf2_core::{collisionfx, sim};
use tf2_core::chatter;

use crate::animation::AnimLibrary;

/// The level whose event file gives the shared sounds, unless `TF2_SOUND_LEVEL` names another.
const DEFAULT_LEVEL: &str = "us_city_00";

/// Whose table a clip's held sound is kept in (`SoundCue::Play`'s `held`): the robot's own
/// (his clips and the weapon arm's partial set), the weapon object's, or another character's.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Table {
    Robot,
    WeaponObject,
    Other(u32),
}

struct Playing {
    mixer: Mixer,
    /// Sounds a clip started that end with it.
    with_clip: Vec<Handle>,
    /// Loops a robot clip started that only a Stop cue later in the same clip ends, by event
    /// id (`until_stop`).
    until_stop: Vec<(u32, Handle)>,
    /// The held clip sounds by table and event id, so a cue of an id still playing reuses it.
    held: HashMap<(Table, u32), Handle>,
    weapon_clips: [Vec<Handle>; 2],
    engine: Option<Handle>,
    sounds: Sounds,
    melee_impact: ImpactPreset,
    tyre_skid: ImpactPreset,
    tyre_road: ImpactPreset,
    /// The tyres' two loops, each with the surface it was started for.
    skid: Option<(Handle, u32)>,
    road: Option<(Handle, u32)>,
    was_vehicle: bool,
    /// His speed at the last tick, in metres per second.
    speed: f32,
    turbo_left: f32,
    turbo_cooldown: f32,
    /// Cues that named an event no loaded file has, so each is reported once.
    unknown: Vec<u32>,
    sources: Vec<(Handle, Source)>,
    position: Vec3,
    regenerating: bool,
    collision_defs: Vec<collisionfx::SoundDef>,
    collision_original: Vec<collisionfx::SoundDef>,
    collision_history: collisionfx::History,
    collision_pending: BTreeMap<u32, tf2_core::vehicle::Contact>,
    collision_clock: f32,
    collision_loops: BTreeMap<(usize, u32), Handle>,
    hardness: std::collections::HashMap<u32, bool>,
    chatter: Option<chatter::Speaker>,
    voice: Option<Handle>,
    /// The robot's `Head` node, where his voice lines sound (`component_sources`).
    head_node: Option<usize>,
    pending_voice: Option<u32>,
    voice_delay: f32,
    low_health_spoken: bool,
    death_spoken: bool,
    chatter_clock: f32,
    random: tf2_core::chatter::GameRandom,
    pending_steps: Vec<u32>,
    /// Footstep and climb-step sounds probed and waiting their one update (`STEP_DISPATCH_DELAY`):
    /// time left, sound, place.
    delayed_steps: Vec<(f32, u32, Vec3)>,
    step_impact: ImpactPreset,
    /// The listener: the camera's position and axes (`Ear`), and the player's motion that
    /// gives its velocity. [game: 00771540]
    listener: Ear,
    listener_motion: tf2_core::sound::ListenerMotion,
    combat_target: bool,
    trace: bool,
    triggered: BTreeMap<(bool, usize), Handle>,
    triggered_once: std::collections::BTreeSet<(bool, usize)>,
    trigger_definitions: [Vec<tf2_core::formats::scene::SoundPlayerDef>; 2],
    kept_sounds: std::collections::BTreeSet<(bool, usize)>,
    projectile_audio: BTreeMap<u32, (Vec<u32>, Vec<u32>)>,
    projectiles: BTreeMap<u32, Vec<Handle>>,
    weapon_impacts: BTreeMap<u32, ImpactPreset>,
    pending_climb_steps: Vec<usize>,
    pending_spawns: Vec<u32>,
    climb_impact: Vec<ImpactPreset>,
    /// The flyer's loop: its handle and event id (`vehicleDrive` in FlyMode, `hoverSound` in
    /// HoverMode), and whether the turbo was running at the last tick.
    flight_loop: Option<(Handle, u32)>,
    flight_turbo: bool,
    /// Seconds to the next "still playing" print (`TF2_SOUND_TRACE`).
    trace_clock: f32,
    /// The trace's last control state, to journal its changes.
    trace_mode: String,
    /// The audio presets, their per-category mix, each category name's paths in the banks,
    /// and whether `OptimusSpecial` is playing (`presets_tick`).
    presets: Vec<tf2_core::audio_mix::AudioPreset>,
    mix: tf2_core::audio_mix::Mix,
    category_paths: HashMap<String, Vec<String>>,
    optimus_preset: bool,
    /// Seconds the current loop has played, and the whole second last printed (`TF_DBG`).
    flight_clock: f32,
    flight_second: i32,
    flight_trace: bool,
    /// Other characters' (the enemies') clip sounds, by their id (`other_clip_cues`).
    others: HashMap<u32, OtherSounds>,
    /// Their probed footsteps waiting their update, as `delayed_steps`, with the stepper's speed.
    other_steps: Vec<(f32, u32, Vec3, f32)>,
    /// FMOD's system reverb (`tf2_core::reverb`): every mixer's send goes through it.
    reverb: tf2_core::reverb::ReverbSystem,
    /// The event system's reverb presets by index (`Library::reverb_presets`).
    reverb_presets: Vec<tf2_core::formats::fev::ReverbProperties>,
    /// The stage's ReverbTrigger waiting for its TriggerOnSpawn link's delay: seconds, preset.
    reverb_pending: Option<(f32, u32)>,
    reverb_send: Vec<f32>,
    /// Output seconds to the trace's next wet-level line.
    reverb_clock: f32,
    /// The trace's last `reverb` line (output seconds), and whether a change came since.
    reverb_noted_at: f32,
    reverb_unnoted: bool,
}

/// The stage (`--mission`'s zone folder) whose ReverbTrigger sets the reverb: `set_stage`.
static STAGE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Names the stage's zone folder (`levels\<name>`) before `start`, for its reverb.
pub fn set_stage(level: &str) {
    let _ = STAGE.set(level.to_owned());
}

/// The trace's test aids for the reverb (not the game's):
/// - `TF2_SOUND_REVERB=off` no reverb; `=<index>` that preset at once instead of the stage's.
/// - `TF2_SOUND_REVERB_AREA=x,y,z,min,max,preset` a ReverbArea of that preset at that world
///   point, as the game's ReverbArea would make one (no pack of the install has one). With the
///   stage's ambient (or Off) around it, the reverb follows the listener in and out.
fn reverb_aids(playing: &mut Playing) -> bool {
    let presets = playing.reverb_presets.clone();
    if let Ok(area) = std::env::var("TF2_SOUND_REVERB_AREA") {
        let v: Vec<f32> = area.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        match (v.as_slice(), v.get(5).and_then(|&p| presets.get(p as usize))) {
            ([x, y, z, min, max, _], Some(properties)) => {
                eprintln!("sound: reverb area of preset {} at ({x}, {y}, {z}), {min}..{max} m (TF2_SOUND_REVERB_AREA)", v[5]);
                playing.reverb.areas.push(tf2_core::reverb::Reverb3d {
                    position: Vec3::new(*x, *y, *z),
                    min_distance: *min,
                    max_distance: *max,
                    properties: *properties,
                    active: true,
                });
            }
            _ => eprintln!("sound: TF2_SOUND_REVERB_AREA wants x,y,z,min,max,preset (0..{}): {area}", presets.len().saturating_sub(1)),
        }
    }
    match std::env::var("TF2_SOUND_REVERB").ok().as_deref() {
        Some("off") => {
            eprintln!("sound: reverb off (TF2_SOUND_REVERB)");
            playing.reverb.areas.clear();
            true
        }
        Some(index) => match index.parse::<u32>() {
            Ok(preset) => {
                eprintln!("sound: reverb preset {preset} (TF2_SOUND_REVERB)");
                playing.reverb.ambient = presets.get(preset as usize).copied();
                true
            }
            Err(_) => false,
        },
        None => false,
    }
}

/// Reads the stage's zone for its ReverbTrigger, on a thread (a zone takes about a second).
/// [game] the trigger fires when its TriggerOnSpawn link does, at the zone's spawn plus the
/// link's delay. [assumed] the port's spawn is when the zone has been read: the stage is
/// already playing by then, so the reverb starts up to a second late. settles: nothing to
/// settle, a port artefact (reading the zone with the level would remove it).
fn read_stage(game_dir: &Path, playing: Arc<Mutex<Playing>>) {
    let Some(level) = STAGE.get().cloned() else { return };
    let game_dir = game_dir.to_owned();
    std::thread::spawn(move || {
        let pack = format!("levels/{level}/zone.str");
        let script = match tf2_core::mission_script::Script::load(&game_dir, Path::new(&pack)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("sound: stage {level}: no reverb ({e})");
                return;
            }
        };
        let triggers = tf2_core::reverb::zone_triggers(&script);
        let mut playing = playing.lock().unwrap_or_else(|e| e.into_inner());
        let fired: Vec<_> = triggers.iter().filter_map(|t| t.at_spawn.map(|at| (at, t.preset))).collect();
        eprintln!("sound: stage {level}: reverb triggers {triggers:?}");
        // [assumed] a zone has at most one fired at spawn (every zone of the install has one
        // trigger); were there more, the last would win.
        if let Some(&(at, preset)) = fired.iter().max_by(|a, b| a.0.total_cmp(&b.0)) {
            playing.reverb_pending = Some((at, preset));
        }
    });
}

/// One other character's clip sounds: every handle its clips started with its event (a Stop
/// cue ends only its own copies), those its current clip ends with it (`autoStop`), and the
/// footstep cues waiting for its pose's probe.
#[derive(Default)]
struct OtherSounds {
    handles: Vec<(Handle, u32)>,
    with_clip: Vec<Handle>,
    pending_steps: Vec<u32>,
}

impl Playing {
    /// Adds the system reverb of every mixer's send to the output. [game] FMOD has one reverb
    /// for all its channels. With the trace on, returns its journal lines: `reverb` when the
    /// morph's result changes, `reverb wet` with the wet signal's RMS every half second.
    fn render_reverb(&mut self, out: &mut [f32], start: f32, tracing: bool) -> Vec<(f32, String)> {
        let mut send = std::mem::take(&mut self.reverb_send);
        send.clear();
        send.extend_from_slice(self.mixer.send());
        send.resize(out.len(), 0.0);
        let changed = self.reverb.render(&send, out);
        self.reverb_send = send;
        let mut lines = Vec::new();
        if !tracing {
            return lines;
        }
        // A morph changes the properties every callback while the listener moves: one line a
        // quarter second is enough to follow it.
        self.reverb_unnoted |= changed;
        if self.reverb_unnoted && (start - self.reverb_noted_at >= 0.25 || self.reverb_noted_at == 0.0)
            && let Some(p) = self.reverb.applied {
            self.reverb_unnoted = false;
            self.reverb_noted_at = start.max(1e-6);
            let line = format!("room {} room_hf {} decay {:.2} reflections {} reverb {} density {:.0}", p.room, p.room_hf, p.decay_time, p.reflections, p.reverb, p.density);
            eprintln!("sound reverb: {line} (listener at {:.1?})", self.reverb.listener);
            lines.push((start, format!("reverb\t\t\t{line}")));
        }
        self.reverb_clock -= out.len() as f32 / 2.0 / self.mixer.rate();
        if self.reverb_clock <= 0.0 {
            self.reverb_clock = 0.5;
            let (rms, _) = self.reverb.take_wet_rms();
            let master = if self.mixer.master > 0.0 { self.mixer.master } else { 1.0 };
            let l = self.reverb.listener;
            lines.push((start, format!("reverb wet\t\t\trms {:.5} at ({:.1}, {:.1}, {:.1})", rms / master, l.x, l.y, l.z)));
        }
        lines
    }
}

enum Source { Body(Vec3), Fixed(Vec3), Moving(Vec3, f32) }

/// One sound in the mix as the debug overlay lists it (`SoundOut::debug_view`). Not the game's.
pub struct DebugSound {
    /// Who started it: "player", "enemy 3", "projectile 12", "engine"...
    pub owner: String,
    /// The event has a sound that loops for ever: it ends only when stopped.
    pub infinite: bool,
    pub view: tf2_core::sound::InstanceView,
}

pub struct DebugView {
    pub sounds: Vec<DebugSound>,
    /// The system reverb in effect, or "off".
    pub reverb: String,
    pub master: f32,
    /// Seconds of sound output so far.
    pub output_time: f32,
}

/// CRC-32 of `Head`: the node a character's voice is tied to. [game: 007224a0] the voice
/// slot looks the skeleton's names up for 07c159a2 and ties the event there (the root when
/// there is none).
const HEAD: u32 = 0x07c1_59a2;

/// CRC-32 of `heat`: the fire sound's parameter [game 007a9b40, `event_set_parameter`].
const HEAT: u32 = 0xba44_e6f8;

/// The listener's position and axes. [game: 00771540 -> 00598050] the game hands FMOD the
/// camera matrix's look (row 1) as forward and its up (row 2), the position its translation;
/// FMOD measures a source across (right) and along (front) them (0040b120). Here the camera's
/// right is its look across the world's up (no roll), and its up is right across look.
#[derive(Clone, Copy)]
struct Ear {
    eye: Vec3,
    right: Vec3,
    front: Vec3,
    up: Vec3,
}

impl Default for Ear {
    fn default() -> Self {
        Self { eye: Vec3::ZERO, right: Vec3::X, front: Vec3::Y, up: Vec3::Z }
    }
}

impl Ear {
    fn new(eye: Vec3, forward: Vec3) -> Self {
        let front = forward.normalize_or(Vec3::Y);
        let right = front.cross(Vec3::Z).normalize_or(Vec3::X);
        Self { eye, right, front, up: right.cross(front) }
    }

    /// A world vector in the listener's frame (x right, y front, z up): `Mixer::offset`'s.
    fn local(&self, world: Vec3) -> Vec3 {
        Vec3::new(world.dot(self.right), world.dot(self.front), world.dot(self.up))
    }
}

#[derive(Resource, Clone)]
pub struct SoundOut {
    library: Arc<Library>,
    playing: Arc<Mutex<Playing>>,
    /// Set by `shut_down`: the output thread closes its stream and ends (the debug menu's swap of
    /// robot, `charmenu.rs`, starts a new output for the new one).
    stopped: Arc<std::sync::atomic::AtomicBool>,
}
/// The weapons (`tf2_core::arsenal`) speak to this output.
impl tf2_core::arsenal::WeaponSound for SoundOut {
    fn play_event(&self, id: u32) { SoundOut::play_event(self, id) }
    fn play_event_at(&self, id: u32, point: Vec3) { SoundOut::play_event_at(self, id, point) }
    fn start_kept(&self, id: u32, heat: Option<f32>) -> Option<Handle> { SoundOut::start_kept(self, id, heat) }
    fn is_infinite(&self, id: u32) -> bool { SoundOut::is_infinite(self, id) }
    fn key_off(&self, handle: Handle) { SoundOut::key_off(self, handle) }
    fn stop(&self, handle: Handle) { SoundOut::stop(self, handle) }
    fn projectile_start(&self, number: u32, weapon: u32, point: Vec3) { SoundOut::projectile_start(self, number, weapon, point) }
    fn projectile_start_sounds(&self, number: u32, ids: &[u32], point: Vec3) { SoundOut::projectile_start_sounds(self, number, ids, point) }
    fn projectile_move(&self, number: u32, point: Vec3, speed: f32) { SoundOut::projectile_move(self, number, point, speed) }
    fn projectile_stop(&self, number: u32) { SoundOut::projectile_stop(self, number) }
    fn projectile_end(&self, number: u32, weapon: u32, point: Vec3) { SoundOut::projectile_end(self, number, weapon, point) }
    fn weapon_impact(&self, weapon: u32, surface: u32, point: Vec3) { SoundOut::weapon_impact(self, weapon, surface, point) }
    fn combat_target(&self, present: bool) { SoundOut::combat_target(self, present) }
    fn weapon_clip_command(&self, command: tf2_core::gun::SoundCommand) { SoundOut::weapon_clip_command(self, command) }
    fn weapon_object_hidden(&self) { SoundOut::weapon_object_hidden(self) }
}

impl SoundOut {
    /// Ends this output: its stream is closed within a fifth of a second.
    pub fn shut_down(&self) {
        self.stopped.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Loads the event files, decodes what Bumblebee can ask for and starts the output. `None`
/// (with the reason printed) leaves the test silent.
pub fn start(game_dir: &Path, spec: &tf2_core::formats::characterlist::Spec, enemy: &tf2_core::formats::characterlist::Spec, data: &CharacterData, animations: &AnimLibrary, tuning: &Tuning, scout: &tf2_core::combat::Scout) -> Option<SoundOut> {
    let volume = std::env::var("TF2_SOUND_VOLUME").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
    if volume <= 0.0 {
        eprintln!("sound: off (TF2_SOUND_VOLUME is 0)");
        return None;
    }
    let level = std::env::var("TF2_SOUND_LEVEL").unwrap_or_else(|_| DEFAULT_LEVEL.to_owned());
    let began = Instant::now();
    let mut files = vec![
        spec.sound_events(game_dir),
        game_dir.join("levels").join(&level).join(r"eng\mission.fev"),
        game_dir.join(r"audio\eng\global.fev"),
        game_dir.join(r"audio\eng\global_streaming.fev"),
        enemy.sound_events(game_dir),
    ];
    // Grindor's cinematic wave plays sounds of Devastator's bank (`energon_wave::SOUNDS`) [port design].
    if data.energon.is_some() {
        files.extend(tf2_core::energon_wave::sound_events(game_dir));
    }
    let (mut library, problems) = Library::load(&files);
    for problem in &problems {
        eprintln!("sound: {problem}");
    }
    let chatter_ids: Vec<_> = data.chatter.iter().map(|l| l.event).collect();
    match library.add_programmer_bank(&game_dir.join(r"audio\eng\CHATTER.fsb"), tf2_core::sound::event_id("ProgrammerSoundPlayer"), &chatter_ids) {
        Ok(count) => eprintln!("sound: {count} of {} chatter table entries bound to his installed radio samples", data.chatter.len()),
        Err(e) => eprintln!("sound: chatter unavailable ({e})"),
    }
    let chatter = match chatter::Tuning::load(game_dir) {
        Ok(tuning) => Some(chatter::Speaker::new(data.chatter.clone(), tuning)),
        Err(e) => { eprintln!("sound: {e}"); None }
    };
    // Everything his clips and his sound table can ask for, decoded now rather than in play.
    let mut wanted = data_ids(data);
    let sets = animations.robot.sets.values().flatten();
    for clip in sets.chain(&animations.vehicle).chain(data.weapon_animations.iter().flatten())
        .filter_map(|reference| reference.clip.as_ref()) {
        wanted.extend(clip.sound_events.iter().filter_map(|event| match event.cue {
            SoundCue::Play { id, .. } => Some(id),
            _ => None,
        }));
    }
    for weapon in &scout.character.weapons {
        wanted.extend([weapon.fire_sound, weapon.overheat_sound, weapon.refused_sound, weapon.ring_off_sound]);
    }
    // The enemy's own clip cues and footsteps (`other_clip_cues`), from its event file.
    wanted.extend(data_ids(&scout.character));
    for clip in scout.character.animations.sets.values().flatten().filter_map(|r| r.clip.as_ref()) {
        wanted.extend(clip.sound_events.iter().filter_map(|event| match event.cue {
            SoundCue::Play { id, .. } => Some(id),
            _ => None,
        }));
    }
    wanted.extend(&chatter_ids);
    wanted.sort_unstable();
    wanted.dedup();
    wanted.retain(|&id| id != 0);
    let found = wanted.iter().filter(|&&id| library.has(id)).count();
    for &id in wanted.iter().filter(|&&id| !library.has(id)) { eprintln!("sound: unresolved authored cue {id:#010x}"); }
    let waves: usize = wanted.iter().map(|&id| library.prepare(id)).sum();
    eprintln!(
        "sound: {} events in the files; his data names {}, {found} of them found, {waves} samples decoded in {:.2} s (shared sounds from level {level})",
        library.event_count(),
        wanted.len(),
        began.elapsed().as_secs_f32(),
    );
    let out = SoundOut {
        library: Arc::new(library),
        playing: Arc::new(Mutex::new(Playing::new(data, tuning, chatter))),
        stopped: Default::default(),
    };
    // The audio presets and, for each category name they use, its paths in these banks.
    match tf2_core::audio_mix::load_presets(game_dir) {
        Ok(presets) => {
            let mut names: Vec<&str> = presets.iter().flat_map(|p| p.entries.iter().map(|e| e.category.as_str())).collect();
            names.sort();
            names.dedup();
            let paths = out.library.category_paths();
            let mut playing = out.playing.lock().unwrap_or_else(|e| e.into_inner());
            playing.category_paths = names
                .iter()
                .map(|n| (n.to_string(), paths.iter().filter(|p| p.rsplit('/').next() == Some(*n)).cloned().collect()))
                .collect();
            let resolved = playing.category_paths.values().filter(|p| !p.is_empty()).count();
            eprintln!("sound: {} audio presets, {resolved} of their {} categories found in the banks", presets.len(), names.len());
            playing.mix = tf2_core::audio_mix::Mix::new(&names);
            playing.presets = presets;
        }
        Err(e) => eprintln!("sound: no audio presets ({e})"),
    }
    {
        let mut playing = out.playing.lock().unwrap_or_else(|e| e.into_inner());
        playing.reverb_presets = out.library.reverb_presets();
        let named = reverb_aids(&mut playing);
        eprintln!("sound: {} reverb presets", playing.reverb_presets.len());
        drop(playing);
        if !named {
            read_stage(game_dir, out.playing.clone());
        }
    }
    // The stream lives on a thread of its own, which keeps it for as long as the test runs.
    let (library, playing, stopped) = (out.library.clone(), out.playing.clone(), out.stopped.clone());
    std::thread::spawn(move || {
        if let Err(e) = output(library, playing, volume, stopped) {
            eprintln!("sound: no output ({e})");
        }
    });
    Some(out)
}

/// What a character's data (not his clips) can ask for: his sound table, impacts, weapons and
/// collision sounds.
fn data_ids(data: &CharacterData) -> Vec<u32> {
    let sounds = data.sounds;
    let mut wanted = vec![sounds.engine, sounds.drift_start, sounds.suspension, sounds.turbo, sounds.turbo_ready, sounds.drive, sounds.hover];
    wanted.extend(data.step_impact.infos.iter().map(|info| info.sound));
    wanted.extend(data.climb_impact.iter().flat_map(|p| &p.infos).map(|info| info.sound));
    wanted.extend(std::iter::once(&data.robot).chain(data.vehicle.iter())
        .flat_map(|o| &o.sound_players).map(|p| p.id));
    wanted.extend(std::iter::once(&data.robot).chain(data.vehicle.iter())
        .flat_map(|o| &o.spawn_sound_players).flat_map(|s| &s.2).copied());
    wanted.extend(data.weapon_effects.iter().flat_map(|w| w.flight_sounds.iter().chain(&w.blast_sounds)).copied());
    // A ground wave's own SoundPlayers (the snaker object's) [data].
    wanted.extend(data.weapon_effects.iter().filter_map(|w| w.wave.as_ref()).flat_map(|w| &w.sounds).copied());
    wanted.extend(data.weapon_effects.iter().flat_map(|w| &w.impact.infos).map(|i| i.sound));
    // Each row's fire sound: the deployed turret's gun is in no weapon list below. [data]
    wanted.extend(data.weapon_effects.iter().map(|w| w.fire_sound));
    for weapon in &data.weapons {
        wanted.extend([weapon.fire_sound, weapon.overheat_sound, weapon.refused_sound]);
        // The ring-off and the idle loop (`genericWeaponSoundTable`) [data].
        wanted.extend([weapon.ring_off_sound, weapon.idle_sound]);
        // A charged shot's sounds (`gunWeaponSoundTable`) [data]: its start, loop, full and release.
        wanted.extend([weapon.charge_start_sound, weapon.charge_loop_sound, weapon.charge_full_sound, weapon.charge_fire_sound]);
    }
    wanted.extend(data.melee_impact.infos.iter().map(|info| info.sound));
    wanted.extend(data.tyre_skid.infos.iter().chain(&data.tyre_road.infos).map(|info| info.sound));
    wanted.extend(data.vehicle.iter().flat_map(|o| &o.collision_sounds)
        .flat_map(|d| d.choices.iter().flatten()).map(|c| c.id));
    wanted.extend(["UI_LOCK_ON", "UI_LOCKED_ON", "UI_REGEN_START"].map(tf2_core::sound::event_id));
    wanted.extend(&data.extra_sounds);
    wanted
}

/// Fills a device buffer of any channel count from the mixer's stereo.
fn render<T: cpal::SizedSample + cpal::FromSample<f32>>(
    out: &mut [T],
    channels: usize,
    mixed: &mut Vec<f32>,
    library: &Library,
    playing: &Mutex<Playing>,
) {
    let frames = out.len() / channels;
    mixed.clear();
    mixed.resize(frames * 2, 0.0);
    let asked = Instant::now();
    let tracing = tf2_core::sound::tracing();
    let (waited, rate, master, start, mut journal) = {
        let mut playing = playing.lock().unwrap_or_else(|e| e.into_inner());
        let waited = asked.elapsed();
        let start = playing.mixer.output_time();
        playing.mixer.render(library, mixed);
        let reverb = playing.render_reverb(mixed, start, tracing);
        let mut journal = Vec::new();
        if tracing {
            journal = playing.mixer.take_journal();
            journal.extend(reverb);
        }
        (waited, playing.mixer.rate(), playing.mixer.master, start, journal)
    };
    // The trace's output check: a callback that waited on the game's lock, or took longer
    // than a quarter of its own buffer, is where the device can run dry (a gap in the sound).
    if tracing {
        let buffer = frames as f32 / rate;
        let took = asked.elapsed().as_secs_f32();
        if took > buffer * 0.25 {
            let line = format!("callback took {:.1} ms (lock wait {:.1} ms) for a {:.1} ms buffer", took * 1e3, waited.as_secs_f32() * 1e3, buffer * 1e3);
            eprintln!("sound output: {line}");
            journal.push((start, format!("slow output\t\t\t{line}")));
        }
        Dump::get(rate).write(mixed, master, &journal);
    }
    for (frame, pair) in out.chunks_mut(channels).zip(mixed.chunks(2)) {
        for (channel, sample) in frame.iter_mut().enumerate() {
            let value = match (channels, channel) {
                (1, _) => (pair[0] + pair[1]) * 0.5,
                (_, 0) => pair[0],
                (_, 1) => pair[1],
                _ => 0.0,
            };
            *sample = <T as cpal::Sample>::from_sample(value.clamp(-1.0, 1.0));
        }
    }
}

/// The trace's recording. With `TF2_SOUND_TRACE=1` and `TF2_SOUND_DUMP=<file.wav>`:
/// - `<file.wav>`: everything mixed, stereo 32-bit float at the device's rate, at volume 1
///   (the master volume divided out, so a quiet run still records at full level; values over
///   1 are kept, so clipping and offsets stay visible). Its sizes are rewritten after every
///   buffer, so the file plays even when the game is killed.
/// - `<file.wav>.events.tsv`: the journal, one line per happening at its output time:
///   `seconds  kind  event  #instance  detail`. Kinds: start, stop, key-off, stolen, end,
///   fault / fault over (the mixer's per-instance check), clip, form, state, slow output.
///
/// `python work\sound_dump.py <file.wav>` reads both. Not the game's: a debug aid.
struct Dump {
    file: Option<(std::fs::File, std::fs::File, u64)>,
}

impl Dump {
    fn get(rate: f32) -> std::sync::MutexGuard<'static, Dump> {
        static DUMP: std::sync::OnceLock<Mutex<Dump>> = std::sync::OnceLock::new();
        DUMP.get_or_init(|| Mutex::new(Dump::open(rate))).lock().unwrap_or_else(|e| e.into_inner())
    }

    fn open(rate: f32) -> Dump {
        use std::io::Write;
        let Some(path) = std::env::var_os("TF2_SOUND_DUMP") else { return Dump { file: None } };
        let path = std::path::PathBuf::from(path);
        let mut events = path.clone().into_os_string();
        events.push(".events.tsv");
        let (Ok(mut wav), Ok(mut tsv)) = (std::fs::File::create(&path), std::fs::File::create(&events)) else {
            eprintln!("sound dump: cannot create {}", path.display());
            return Dump { file: None };
        };
        let rate = rate as u32;
        let mut head = Vec::with_capacity(44);
        head.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
        head.extend_from_slice(&16u32.to_le_bytes());
        head.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
        head.extend_from_slice(&2u16.to_le_bytes());
        head.extend_from_slice(&rate.to_le_bytes());
        head.extend_from_slice(&(rate * 8).to_le_bytes());
        head.extend_from_slice(&8u16.to_le_bytes());
        head.extend_from_slice(&32u16.to_le_bytes());
        head.extend_from_slice(b"data\0\0\0\0");
        let _ = wav.write_all(&head);
        let _ = writeln!(tsv, "seconds\tkind\tevent\tinstance\tdetail");
        eprintln!("sound dump: recording to {} ({rate} Hz) and its .events.tsv", path.display());
        Dump { file: Some((wav, tsv, 0)) }
    }

    fn write(&mut self, mixed: &[f32], master: f32, journal: &[(f32, String)]) {
        use std::io::{Seek, SeekFrom, Write};
        let Some((wav, tsv, written)) = &mut self.file else { return };
        let scale = if master > 0.0 { 1.0 / master } else { 0.0 };
        let bytes: Vec<u8> = mixed.iter().flat_map(|v| (v * scale).to_le_bytes()).collect();
        let _ = wav.write_all(&bytes);
        *written += bytes.len() as u64;
        let data = (*written).min(u32::MAX as u64 - 36) as u32;
        let _ = wav.seek(SeekFrom::Start(4)).and_then(|_| wav.write_all(&(data + 36).to_le_bytes()));
        let _ = wav.seek(SeekFrom::Start(40)).and_then(|_| wav.write_all(&data.to_le_bytes()));
        let _ = wav.seek(SeekFrom::End(0));
        for (at, line) in journal {
            let _ = writeln!(tsv, "{at:.3}\t{line}");
        }
    }
}

/// Opens the default device at its own rate and feeds it until the test ends.
fn output(library: Arc<Library>, playing: Arc<Mutex<Playing>>, volume: f32, stopped: Arc<std::sync::atomic::AtomicBool>) -> Result<(), String> {
    let device = cpal::default_host().default_output_device().ok_or("no output device")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let channels = supported.channels() as usize;
    let rate = supported.sample_rate();
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    {
        let mut playing = playing.lock().unwrap_or_else(|e| e.into_inner());
        playing.mixer = Mixer::new(rate);
        playing.mixer.master = volume;
        let (ambient, areas) = (playing.reverb.ambient, playing.reverb.areas.clone());
        playing.reverb = tf2_core::reverb::ReverbSystem::new(rate as f32);
        playing.reverb.ambient = ambient;
        playing.reverb.areas = areas;
        for (option, categories) in [
            ("TF2_SOUND_FX_VOLUME", &["FX", "FX_LOUD"][..]),
            ("TF2_SOUND_UI_VOLUME", &["UI"][..]),
            ("TF2_SOUND_VOICE_VOLUME", &["VO", "VO_LOUD"][..]),
        ] {
            if let Some(gain) = std::env::var(option).ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite()) {
                for category in categories {
                    playing.mixer.set_category_volume(category, gain);
                }
            }
        }
        playing.engine = None;
        playing.with_clip.clear();
        playing.until_stop.clear();
        playing.weapon_clips.iter_mut().for_each(Vec::clear);
        playing.sources.clear();
        playing.skid = None;
        playing.road = None;
        playing.collision_loops.clear();
    }
    let problem = |e| eprintln!("sound: {e}");
    let mut mixed = Vec::new();
    let stream = match format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| render(out, channels, &mut mixed, &library, &playing),
            problem,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            move |out: &mut [i16], _: &cpal::OutputCallbackInfo| render(out, channels, &mut mixed, &library, &playing),
            problem,
            None,
        ),
        other => return Err(format!("the device wants {other:?} samples")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    eprintln!("sound: output started ({rate} Hz, {channels} channels)");
    while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
        std::thread::park_timeout(std::time::Duration::from_millis(200));
    }
    drop(stream);
    eprintln!("sound: output stopped");
    Ok(())
}

/// How long a probed footstep or climb step waits before it sounds: one game update, the IK ring
/// (priority 12) dispatching what the world step (priority 14) resolved the update before.
/// [game 005a8740, 01174360] (`SoundOut::footsteps`)
const STEP_DISPATCH_DELAY: f32 = sim::GAME_UPDATE;

/// The sound cues of `clip` due between two positions in ticks (`clip_cues` says how). [game]
fn due_cues(clip: &Clip, from: f32, to: f32, rerun: bool) -> impl Iterator<Item = &tf2_core::formats::anim::SoundEvent> {
    clip.sound_events.iter().filter(move |event| {
        let t = event.time;
        let (due, again) = if to >= from {
            ((t > from && t <= to) || (from == 0.0 && t == 0.0 && to > 0.0), rerun)
        } else if t > from {
            (true, rerun)
        } else {
            (t <= to, true)
        };
        due && !(again && !event.rerun)
    })
}

impl Playing {
    /// Plays the probed steps whose update has come. [game 00740d60 -> 0073f520]
    fn dispatch_steps(&mut self, library: &Library, dt: f32) {
        let mut due = Vec::new();
        self.delayed_steps.retain_mut(|(left, id, point)| {
            *left -= dt;
            let ready = *left <= 0.0;
            if ready {
                due.push((*id, *point));
            }
            !ready
        });
        for (id, point) in due {
            self.play_at(library, id, point, false);
        }
    }

    /// `dispatch_steps` for the other characters' steps (counted down by their own update,
    /// `others_tick`, so they do not wait on the player's).
    fn dispatch_other_steps(&mut self, library: &Library, dt: f32) {
        let mut due = Vec::new();
        self.other_steps.retain_mut(|(left, id, point, speed)| {
            *left -= dt;
            let ready = *left <= 0.0;
            if ready {
                due.push((*id, *point, *speed));
            }
            !ready
        });
        for (id, point, speed) in due {
            // A step is the stepper's own sound, but no clip or Stop cue ends it: it is not kept
            // with its owner's handles.
            if let Some(handle) = self.play(library, id)
                && let Some((_, source)) = self.sources.iter_mut().find(|(h, _)| *h == handle) {
                *source = Source::Moving(point, speed);
            }
        }
    }

    fn new(data: &CharacterData, tuning: &Tuning, chatter: Option<chatter::Speaker>) -> Self {
        Self {
            mixer: Mixer::new(48000), with_clip: Vec::new(), until_stop: Vec::new(), held: HashMap::new(), weapon_clips: Default::default(), engine: None, sounds: data.sounds,
            melee_impact: data.melee_impact.clone(), tyre_skid: data.tyre_skid.clone(), tyre_road: data.tyre_road.clone(),
            skid: None, road: None, was_vehicle: false, speed: 0.0, turbo_left: 0.0, turbo_cooldown: 0.0,
            unknown: Vec::new(), sources: Vec::new(), position: Vec3::ZERO, regenerating: false,
            collision_defs: data.vehicle.iter().flat_map(|o| &o.collision_sounds).cloned().collect(),
            collision_original: data.vehicle.iter().flat_map(|o| &o.collision_sounds).cloned().collect(),
            collision_history: collisionfx::History::default(), collision_pending: BTreeMap::new(),
            collision_clock: 0.0, collision_loops: BTreeMap::new(), hardness: tuning.surface_hardness.clone(),
            chatter, voice: None, pending_voice: None, voice_delay: 0.0, low_health_spoken: false,
            death_spoken: false, chatter_clock: 0.0, random: tf2_core::chatter::GameRandom(0x2545_f491), pending_steps: Vec::new(), delayed_steps: Vec::new(),
            step_impact: data.step_impact.clone(), listener: Ear::default(), listener_motion: Default::default(), combat_target: false,
            head_node: data.robot.nodes.iter().position(|n| n.name_hash == HEAD),
            trace: std::env::var("TF2_SOUND_TRACE").is_ok_and(|v| v == "1"),
            triggered: BTreeMap::new(),
            triggered_once: std::collections::BTreeSet::new(),
            trigger_definitions: [data.robot.sound_players.clone(), data.vehicle.as_ref().map_or_else(Vec::new, |o| o.sound_players.clone())],
            kept_sounds: std::collections::BTreeSet::new(),
            projectile_audio: data.weapon_effects.iter().map(|w| (w.name, (w.flight_sounds.clone(), w.blast_sounds.clone()))).collect(),
            projectiles: BTreeMap::new(),
            weapon_impacts: data.weapon_effects.iter().map(|w| (w.name, w.impact.clone())).collect(),
            pending_climb_steps: Vec::new(), pending_spawns: Vec::new(), climb_impact: data.climb_impact.clone(),
            flight_loop: None, flight_turbo: false, trace_clock: 0.0, trace_mode: String::new(), flight_clock: 0.0, flight_second: -1,
            flight_trace: std::env::var("TF_DBG").is_ok(),
            presets: Vec::new(), mix: tf2_core::audio_mix::Mix::new(&[]), category_paths: HashMap::new(), optimus_preset: false,
            others: HashMap::new(), other_steps: Vec::new(),
            reverb: tf2_core::reverb::ReverbSystem::new(48000.0), reverb_presets: Vec::new(), reverb_pending: None,
            reverb_send: Vec::new(), reverb_clock: 0.0, reverb_noted_at: 0.0, reverb_unnoted: false,
        }
    }

    /// Stops an event's copies that are not another character's: the player's Stop cue ends his
    /// own sound, not an enemy's copy of a shared one. [assumed] (see `other_clip_cues`)
    fn stop_own_event(&mut self, id: u32) {
        for handle in self.mixer.handles_of(id) {
            if !self.others.values().any(|o| o.handles.iter().any(|&(h, _)| h == handle)) {
                self.mixer.stop(handle);
            }
        }
    }

    /// A clip's play cue. A held one whose id the table still holds, playing, is that sound
    /// again; nothing new starts. [game 005984d0: the query 0059aa00 finds it, the start is
    /// skipped and the held handle is put back by 0059aac0] Without this a loop whose clip is
    /// left before its stop key stacked a copy on every pass (Grindor's `ANIM_BLAC_SRVO_LP`).
    /// [assumed] the query does not find a sound that has ended or been stopped (its stop
    /// 005987e0 takes it out of the table through 0059ac20; the listener that answers the
    /// query, message 0x74e62405, is not read); settles: reading that listener.
    fn play_cue(&mut self, library: &Library, table: Table, id: u32, held: bool, start: impl FnOnce(&mut Self, &Library) -> Option<Handle>) -> Option<Handle> {
        if held && let Some(&handle) = self.held.get(&(table, id)) && self.mixer.is_playing(handle) {
            return Some(handle);
        }
        let handle = start(self, library)?;
        if held {
            self.held.insert((table, id), handle);
        }
        Some(handle)
    }

    /// Starts one of another character's sounds where it stands, moving at `speed`.
    fn play_other(&mut self, library: &Library, owner: u32, id: u32, point: Vec3, speed: f32) -> Option<Handle> {
        let handle = self.play(library, id)?;
        if let Some((_, source)) = self.sources.iter_mut().find(|(h, _)| *h == handle) {
            *source = Source::Moving(point, speed);
        }
        self.others.entry(owner).or_default().handles.push((handle, id));
        Some(handle)
    }

    /// The audio presets (`audio_mix`): the Optimus specials play `OptimusSpecial` while their
    /// timer runs and stop it at the end [game 0072afd0 / 0072b980 cases 0 and 1 through 01194410
    /// (the audio manager) and 00771230 / 011832a0]; each category's volume and pitch then go to
    /// the mixer on every path whose last name is the preset's category (the game compares names).
    fn presets_tick(&mut self, state: &State, dt: f32) {
        use tf2_core::special_types::SpecialType;
        let on = state.special.as_ref().is_some_and(|a| a.is_on() && matches!(a.kind, SpecialType::Optimus | SpecialType::OptimusFlight));
        if on != self.optimus_preset {
            self.optimus_preset = on;
            if let Some(preset) = tf2_core::audio_mix::preset(&self.presets, tf2_core::special_types::OPTIMUS_SPECIAL) {
                if on { self.mix.play(preset, None) } else { self.mix.stop(preset) }
            }
        }
        for category in &mut self.mix.categories {
            let Some(out) = category.update(dt) else { continue };
            for path in self.category_paths.get(&category.name).into_iter().flatten() {
                self.mixer.set_category_volume(path, out.volume);
                self.mixer.set_category_pitch(path, tf2_core::audio_mix::raw_pitch_multiplier(out.pitch));
            }
        }
    }

    fn trigger(&mut self, library: &Library, key: (bool, usize), definition: &tf2_core::formats::scene::SoundPlayerDef) {
        if definition.once && self.triggered_once.contains(&key) { return; }
        if let Some(&old) = self.triggered.get(&key) {
            if self.mixer.is_playing(old) && !definition.retrigger { return; }
            self.mixer.stop(old);
        }
        if let Some(handle) = self.play(library, definition.id) {
            self.triggered.insert(key, handle);
            self.triggered_once.insert(key);
        }
    }

    /// `sound_player_untrigger` 0076e450: an active player's event is keyed off
    /// (`sound_event_stop` 00595c90) when its `keyOffOnUnTrigger` is on, else stopped with its
    /// fade (vtable +0x10 with 0), and the player is no longer active. [game] (Optimus's
    /// `specialAttack` player, `WPN_OPTI_SPEC_FIRE`, holds at a sustain point until this key-off:
    /// without it the loop never left.)
    fn untrigger(&mut self, key: (bool, usize)) {
        let Some(handle) = self.triggered.remove(&key) else { return };
        let key_off = self.trigger_definitions[key.0 as usize].get(key.1).is_some_and(|d| d.key_off_on_untrigger);
        if key_off { self.mixer.key_off(handle) } else { self.mixer.stop(handle) }
    }

    /// [game] SoundPlayer receives the same persistent Trigger/UnTrigger messages
    /// as particles. In particular, Camaro boost audio follows turbo_fx_on, including
    /// independent drift/standard stops and DriveMode exit, rather than a new timer.
    fn kept(&mut self, library: &Library, state: &State, tuning: &Tuning) {
        // No Bumblebee headlight sound players; no playable pack has a sound player on
        // `FX_smoke_exhaust` (`core\examples\form_fx_names.rs`), so the flight exhaust latch is moot here.
        let names = tf2_core::fxstate::kept(state, tuning, false, false);
        let wanted = &names;
        let desired: std::collections::BTreeSet<_> = self.trigger_definitions.iter().enumerate()
            .flat_map(|(form, defs)| defs.iter().enumerate().filter_map(move |(index, d)| wanted.contains(&d.script).then_some((form != 0, index))))
            .collect();
        let ends: Vec<_> = self.kept_sounds.difference(&desired).copied().collect();
        for key in ends {
            self.untrigger(key);
        }
        let starts: Vec<_> = desired.difference(&self.kept_sounds).copied().collect();
        for key in starts {
            let definition = self.trigger_definitions[key.0 as usize][key.1].clone();
            self.trigger(library, key, &definition);
        }
        self.kept_sounds = desired;
    }
    fn chatter(&mut self, library: &Library, state: &State, tuning: &Tuning, dt: f32) {
        self.chatter_clock += dt;
        // Match the game's update cadence; probability is a per-update trial.
        while self.chatter_clock >= sim::GAME_UPDATE {
            self.chatter_clock -= sim::GAME_UPDATE;
            if let Some(id) = self.pending_voice {
                self.voice_delay -= sim::GAME_UPDATE;
                if self.voice_delay <= 0.0 {
                    self.pending_voice = None;
                    self.voice = self.play(library, id);
                }
                continue;
            }
            let Some(speaker) = &mut self.chatter else { return };
            speaker.step(sim::GAME_UPDATE);
            let low = state.health / tuning.base_health.max(1e-5) < speaker.tuning.low_health;
            if !low { self.low_health_spoken = false; }
            if self.voice.is_some_and(|h| self.mixer.is_playing(h)) || state.car_mode { continue; }
            // [game: 007cb180, raw007cbb44..77 /007cb887..b0]
            // Self-injury below the authored low-health threshold; weapon fire and
            // special attack use the generic attack category. Self death is once.
            let special = state.has_state_class(tuning, "StateAttackSpecial");
            let request = if state.dead() { (!self.death_spoken).then_some((chatter::DIED, chatter::CHARACTER, chatter::SELF)) }
                else if low && !self.low_health_spoken { Some((chatter::INJURED, chatter::CHARACTER, chatter::SELF)) }
                else if state.firing && state.state_class(tuning) == Some("StateWeapon") && self.combat_target { Some((chatter::ATTACK, chatter::GENERIC, chatter::ENEMY)) }
                else if special { Some((chatter::ATTACK, chatter::GENERIC, chatter::GENERIC_SUB)) }
                else { None };
            let Some((action, category, sub)) = request else { continue };
            // The game's generators, with the clock (2026-10-20, `tf2_core::chatter::GameRandom`; the port had
            // its own xorshift): random_range 011904f0 for the line, random_range_with_clock 00551d60 for the
            // frequency test and the delay, the wall clock's seconds as `_time64`. [game]
            // [stand-in: the chatter's own generator state; the game's is shared with all callers]
            // So its sequence, like the camera shake's, cannot be reproduced. QUEUED, settles: a recording
            // of the chatter choices, compared by their spread.
            // [game: 007cc720, 007cd430] The roll's ranges are the game's own: the line pick is
            // random_range 011904f0 over [0, n), the frequency test random_range_with_clock(0, 1) <=
            // the line's frequency, and the speak delay random_range_with_clock(min, max). Only the
            // random source differs from the original (see above).
            // The game's generators with the clock's seconds (`_time64`) mixed in, as 011904f0 / 00551d60. [game]
            let seconds = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as u32);
            let random = std::cell::RefCell::new(&mut self.random);
            let Some(line) = speaker.choose_with(action, category, [sub, 0],
                &mut |n| random.borrow_mut().range(n, seconds()),
                &mut || random.borrow_mut().between(0.0, 1.0, seconds())) else { continue };
            let [min, max] = speaker.tuning.delay;
            self.voice_delay = random.borrow_mut().between(min, max, seconds());
            // [game: raw007cd794..be] Queue playback for game clock + speak delay.
            if library.has(line.event) {
                self.pending_voice = Some(line.event);
                self.low_health_spoken |= action == chatter::INJURED;
                self.death_spoken |= action == chatter::DIED;
            }
        }
    }
    fn play(&mut self, library: &Library, id: u32) -> Option<Handle> {
        let handle = self.mixer.play(library, id, 1.0);
        if let Some(handle) = handle {
            if self.trace { eprintln!("sound cue: {}", library.name(id).unwrap_or("unnamed")); }
            self.sources.push((handle, Source::Body(Vec3::ZERO)));
            // [data] the walking hydraulics are louder the faster he goes: their events
            // carry a `speed` parameter (0 to 20). Events without one ignore this.
            self.mixer.set_parameter(handle, "speed", self.speed);
            // [game] 00734630/007336f0/00733800 set5913246c to1 for
            // the human player's vehicle events; the FEV names it2dPanLevel.
            if [self.sounds.engine, self.sounds.turbo, self.sounds.turbo_ready].contains(&id) {
                self.mixer.set_parameter(handle, "2dPanLevel", 1.0);
            }
        }
        if handle.is_none() && id != 0 && !self.unknown.contains(&id) {
            self.unknown.push(id);
            eprintln!("sound: no loaded event file has event {id:#010x}");
        }
        handle
    }

    fn play_at(&mut self, library: &Library, id: u32, point: Vec3, attached: bool) -> Option<Handle> {
        let handle = self.play(library, id)?;
        self.place(handle, point, attached);
        Some(handle)
    }

    fn place(&mut self, handle: Handle, point: Vec3, attached: bool) {
        if let Some((_, source)) = self.sources.iter_mut().find(|(h, _)| *h == handle) {
            *source = if attached { Source::Body(point - self.position) } else { Source::Fixed(point) };
        }
    }

    fn spatial(&mut self) {
        let ear = self.listener;
        self.sources.retain(|(handle, _)| self.mixer.is_playing(*handle));
        for (handle, source) in &self.sources {
            let point = match source { Source::Body(offset) => self.position + *offset, Source::Fixed(point) | Source::Moving(point, _) => *point };
            let speed = match source { Source::Body(_) => Some(self.speed), Source::Moving(_, speed) => Some(*speed), _ => None };
            if let Some(speed) = speed {
                // [game: 0076e100/00595770] Attached events follow body speed every update.
                self.mixer.set_parameter(*handle, "speed", speed);
                self.mixer.set_parameter_hash(*handle, 0xd9a785a4, speed);
            }
            self.mixer.set_spatial(*handle, ear.local(point - ear.eye));
        }
    }

    /// [game 00866760] FlyMode's loop: the character's `vehicleDrive` event starts when the
    /// flight starts and stops when it ends; in HoverMode its `hoverSound` plays instead:
    /// [game] HoverMode's enter (008713c0) starts `soundTable.hoverSound` (character +0x2b0)
    /// through 00871920 when it is not already playing, ties it to the body (00595770) and
    /// sets 2dPanLevel 1 for the player; the exit (00871580 -> 00871a30) stops it (event
    /// vtable +0x10 with 0, a fade-out stop) and drops the handle (mode +0x1c). Each tick: `speed` is the body's speed (00595930), and in flight `pitch` and `roll`
    /// (00864b66, 00864ba1) as the game forms them. The turbo's event plays when the turbo
    /// starts (007336f0). The ids come from the character's data (`soundTable`), never named here.
    fn flight_loop_tick(&mut self, library: &Library, state: &State, tuning: &Tuning, dt: f32) {
        // [game 00864b66] the game's radian-to-degree factor, as written there.
        const DEG: f32 = 0.017_453_292;
        let wanted = match state.mode {
            sim::Mode::Fly => self.sounds.drive,
            sim::Mode::Hover => self.sounds.hover,
            _ => 0,
        };
        if let Some((handle, id)) = self.flight_loop
            && (id != wanted || !self.mixer.is_playing(handle)) {
            self.mixer.stop(handle);
            self.flight_loop = None;
        }
        if self.flight_loop.is_none() && wanted != 0
            && let Some(handle) = self.play(library, wanted) {
            // [game] the human player's events have 2dPanLevel 1 (00865951 / 00725a40), as
            // the car's do: the enter pushes `fld1` and hash 5913246c to 005956e0 when
            // is_human_player (disassembly 00865937-00865957).
            self.mixer.set_parameter(handle, "2dPanLevel", 1.0);
            self.flight_loop = Some((handle, wanted));
            self.flight_clock = 0.0;
            self.flight_second = -1;
            if self.flight_trace { eprintln!("flight sound: started {}", library.name(wanted).unwrap_or("unnamed")); }
        }
        // [game] FlyMode's update starts the turbo with turbo_change_state(mode+0x34, 1)
        // (00864320 -> 00732e60), the same call the car's turbo makes, and that plays
        // soundTable.vehicleTurbo (007336f0). Here: on the rise of the flight turbo.
        let turbo = state.mode == sim::Mode::Fly && state.fly.as_ref().is_some_and(|fly| fly.turbo_left > 0.0);
        if turbo && !self.flight_turbo {
            let id = self.sounds.turbo;
            self.play(library, id);
        }
        self.flight_turbo = turbo;
        let Some((handle, id)) = self.flight_loop else { return };
        let speed = state.vel.length();
        self.mixer.set_parameter(handle, "speed", speed);
        let mut pitch_value = 0.0;
        let mut roll_value = 0.0;
        if let (sim::Mode::Fly, Some(fly)) = (state.mode, state.fly.as_ref()) {
            let fly_tuning = &tuning.forms.fly;
            // [game 00864b66] pitch = mode+0x1c over maxPitch (attr+0x58) when positive, over
            // minPitch (attr+0x5c) when negative, in degrees (disassembly 00864b26-00864b63).
            // [data] every flyer's minPitch is negative (-20 / -30, `global_states.txt`), so a
            // nose-down attitude gives a positive value too: the game's parameter is never
            // below 0, and the -1..0 half of VEH_F22's `pitch` envelopes is never reached.
            // (That `fly.pitch` has mode+0x1c's sign is the flight port's reading.)
            let p = fly.pitch;
            pitch_value = if p > 0.0 { p / (fly_tuning.max_pitch * DEG) }
                else if p < 0.0 { p / (fly_tuning.min_pitch * DEG) }
                else { 0.0 };
            // [game 00864ba1] roll = mode+0x20 over maxRollQuick (attr+0x64), degrees. Sign kept.
            roll_value = fly.roll / (fly_tuning.max_roll_quick * DEG);
            self.mixer.set_parameter(handle, "pitch", pitch_value);
            self.mixer.set_parameter(handle, "roll", roll_value);
        }
        self.flight_clock += dt;
        let second = self.flight_clock.floor() as i32;
        if self.flight_trace && second != self.flight_second && self.flight_clock < 7.0 {
            self.flight_second = second;
            eprintln!("flight sound t={:.1}s {} speed={speed:.2} pitch={pitch_value:.3} roll={roll_value:.3}", self.flight_clock, library.name(id).unwrap_or("unnamed"));
        }
    }

    fn collisions(&mut self, library: &Library, state: &State, dt: f32) {
        let driving = state.mode == sim::Mode::Drive;
        let mut events = Vec::new();
        if !driving {
            events = self.collision_history.clear();
            self.collision_pending.clear();
            self.collision_clock = 0.0;
        } else {
            for contact in &state.vehicle_contacts { self.collision_pending.insert(contact.key, *contact); }
            self.collision_clock += dt;
        }
        while driving && self.collision_clock >= sim::GAME_UPDATE {
            self.collision_clock -= sim::GAME_UPDATE;
            for (_, contact) in std::mem::take(&mut self.collision_pending) {
                if self.collision_defs.iter().any(|d| d.filter.accepts(&contact)) {
                    // [assumed] Same arena contact adaptation as CollisionFX: first
                    // contact is push3, repeated contact slide5; Lux dispatcher unread.
                    // settles: a recording of the Lux contact physics types (1 roll, 2/5 sliding,
                    // 3/4 initial push, set per contact by the solver at run time; the solver
                    // itself is 006e6a10's neighbourhood, not read here).
                    let kind = if self.collision_history.contains(contact.key) { 5 } else { 3 };
                    self.collision_history.touch(contact, kind);
                }
            }
            events.extend(self.collision_history.step(sim::GAME_UPDATE));
        }
        let turn = tf2_core::vehicle::rotation(state.yaw, state.pitch, state.roll);
        for event in events {
            let keys: Vec<_> = self.collision_loops.keys().filter(|(_, key)| *key == event.key).copied().collect();
            for key in keys {
                if let Some(handle) = self.collision_loops.remove(&key) { self.mixer.stop(handle); }
            }
            if event.kind == collisionfx::Kind::End { continue; }
            let hard = self.hardness.get(&event.surface).copied().unwrap_or(false);
            let bottom = (turn.inverse() * event.normal).normalize_or_zero().z > 0.7;
            for index in 0..self.collision_defs.len() {
                if !self.collision_defs[index].filter.accepts_speed(event.speed) { continue; }
                let Some(choice) = self.collision_defs[index].select(event.kind, hard, bottom) else { continue; };
                if let Some(handle) = self.play_at(library, choice.id, event.point, choice.attached) {
                    if event.kind == collisionfx::Kind::Impact {
                        // [game: 0077f620] Case-sensitive impactForce hash, retained.
                        // impactForce = max(n . (v_b - v_a), contact+5c) [game 0077f620, raw
                        // 0077fc00..0077fc89]: `collisionfx::Event::impact` (the moving body as A,
                        // the static world as B, no impulse floor: see its tags).
                        self.mixer.set_parameter(handle, IMPACT_FORCE, event.impact);
                    } else {
                        self.collision_loops.insert((index, event.key), handle);
                    }
                }
            }
        }
        for (&(_, key), &handle) in &self.collision_loops {
            self.mixer.set_parameter(handle, "speed", state.vel.length());
            if let Some(point) = self.collision_history.point(key)
                && let Some((_, source)) = self.sources.iter_mut().find(|(h, _)| *h == handle)
                    && let Source::Body(offset) = source {
                *offset += (point - state.pos - *offset).clamp_length_max(0.01);
            }
        }
    }

    /// Keeps one of the tyres' loops (the skid's or the rolling one) playing for `surface`:
    /// started when wanted, started again when the surface changes, stopped when not.
    /// [game] `FUN_007347c0`
    fn tyre_loop(&mut self, library: &Library, skid: bool, surface: Option<u32>) {
        let current = if skid { self.skid } else { self.road };
        let next = match (current, surface) {
            (Some((handle, was)), Some(now)) if was == now && self.mixer.is_playing(handle) => current,
            (current, wanted) => {
                if let Some((handle, _)) = current {
                    self.mixer.stop(handle);
                }
                wanted.and_then(|surface| {
                    let preset = if skid { &self.tyre_skid } else { &self.tyre_road };
                    let id = preset.pick(surface).map_or(0, |info| info.sound);
                    self.play(library, id).map(|handle| (handle, surface))
                })
            }
        };
        if skid {
            self.skid = next;
        } else {
            self.road = next;
        }
    }
}

impl SoundOut {
    /// [assumed] The arena's best aim-assist target supplies the original chatter
    /// component's combat-target presence. Read 2026-10-20 (4th pass): 007cb180 takes its target
    /// from the owner's controller handle (ai_handle_get 007259c0, +0x108/+0x10c) at l.214-245 and
    /// tests it for presence (`iStack_40 != 0`, the StateWeapon branch around l.245-300); its
    /// +0x31c test (weapon mode == 2, weapon-mode.md l.626) is a second test on that same target,
    /// not the presence. So the game's flag is the AI controller's target, not aim assist.
    /// settles: the AI brain's target selection that fills the controller
    /// handle for the player, and the port's weapon mode test of that target (not ported here).
    pub fn combat_target(&self, present: bool) { self.playing().combat_target = present; }

    fn playing(&self) -> MutexGuard<'_, Playing> {
        self.playing.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Plays one event by id, for things that are not clip cues (the weapons).
    pub fn play_event(&self, id: u32) {
        let started = self.playing().play(&self.library, id);
        if std::env::var("TF_DBG").is_ok() && id != 0 {
            eprintln!("sound start: {} ({})", self.library.name(id).unwrap_or("unnamed"), if started.is_some() { "playing" } else { "not in the files" });
        }
    }

    /// Starts an event at the body and returns its handle, for a sound the caller keeps
    /// (a fire loop, a ring-off, a weapon's idle loop). `heat`: the weapon's heat before this
    /// round, set on the event's `heat` parameter (CRC 0xba44e6f8) at the start, as
    /// `gun_fire_sound_start` does for the fire sound [game 007a9b40]; events without the
    /// parameter ignore it. None when no loaded file has the event.
    pub fn start_kept(&self, id: u32, heat: Option<f32>) -> Option<Handle> {
        let mut playing = self.playing();
        let handle = playing.play(&self.library, id);
        if let (Some(handle), Some(heat)) = (handle, heat) {
            playing.mixer.set_parameter_hash(handle, HEAT, heat);
        }
        if std::env::var("TF_DBG").is_ok() && id != 0 {
            eprintln!("sound start: {} ({})", self.library.name(id).unwrap_or("unnamed"), if handle.is_some() { "kept" } else { "not in the files" });
        }
        handle
    }

    /// `sound_event_stop` [game 00595c90]: a key-off on the event's primary parameter, which
    /// runs on past its sustain point into the tail (a gatling's spin-down). Not a fade: an
    /// event without a primary parameter goes on (use `stop` for it). A handle that has
    /// ended is ignored.
    pub fn key_off(&self, handle: Handle) {
        let mut playing = self.playing();
        if std::env::var("TF_DBG").is_ok() && playing.mixer.is_alive(handle) {
            eprintln!("sound keyoff: {handle:?}");
        }
        playing.mixer.key_off(handle);
    }

    /// Stops one kept event with its own fade-out (`Event::stop` non-immediate, as the idle
    /// sound's stop 007aa110 calls it [game; the FMOD name is assumed]).
    pub fn stop(&self, handle: Handle) {
        let mut playing = self.playing();
        if std::env::var("TF_DBG").is_ok() && playing.mixer.is_alive(handle) {
            eprintln!("sound stop: {handle:?}");
        }
        playing.mixer.stop(handle);
    }

    /// Whether an event runs until stopped (any sound loops): the game keeps an automatic
    /// weapon's fire-sound handle only then [game 007a9b40, `com_query_call` 00775f60
    /// lengthms < 0; assumed = loop mode 0].
    pub fn is_infinite(&self, id: u32) -> bool {
        self.library.infinite(id)
    }

    /// [game: 0076d490/0076e500] Respect each component's retrigger/once flags.
    /// Other copies from clip cues have separate handles.
    pub fn trigger_event(&self, car: bool, index: usize, definition: &tf2_core::formats::scene::SoundPlayerDef) {
        let mut playing = self.playing();
        let key = (car, index);
        playing.trigger(&self.library, key, definition);
    }

    /// An Untrigger reaches every SoundPlayer of that name (`untrigger_name`). [game]
    pub fn untrigger_name(&self, name: u32) {
        let mut playing = self.playing();
        let keys: Vec<_> = playing.triggered.keys().copied()
            .filter(|&(car, index)| playing.trigger_definitions[car as usize].get(index).is_some_and(|d| d.script == name))
            .collect();
        for key in keys {
            playing.untrigger(key);
        }
    }

    /// [game: 0076e100] (batch C3, 2026-10-20: 0076deb0's only direct caller is 00770520, whose only
    /// caller is vo_sequence_update 0076f3e0, a mission voice path. Whether a robot's component
    /// sources are updated by 0076deb0 at all is OPEN: settles: the vtable slot or walk that reaches
    /// 0076deb0 for a robot's SoundPlayer, read with `find_ptr 0076deb0`.)
    /// [game: 0076e100] Component sounds follow their attachment's world matrix,
    /// including bone motion/turning. [assumed] As with foot/spawn cues, use the
    /// frame's final animated pose; original callback order settles its timing.
    /// Second pass (2026-10-20): 0076e100 (sound_player_source_update) reads the owner's world
    /// matrix at its +0x10 record, and is called only from 0076deb0 (sound_player_update), itself
    /// called from 00770520 (not yet named). Whether that runs before or after the pose is written
    /// is the order the port does not match yet. Third pass (2026-10-20): 0076deb0 is the robot
    /// SoundPlayer's vtable slot 2 (00b5e5c4, the base class; its registry 0076d320 and ctor 0076cb90
    /// store it). 00770520 is slot 2 of the derived class's vtable 00b5e774 (2026-10-20 correction:
    /// the destructor 00770570 stores &PTR_FUN_00b5e774, and 00770520 sits at 00b5e77c, not slot 3
    /// of 00b5e770), owner-gated by sound_flag_test 0x10, else FUN_00775310, and calls 0076deb0 after
    /// its gate. Its own caller (a virtual walk, slot 2 through a `+8` call) is not found, so the
    /// frame order is still open. settles: the walker that calls these slots, read with
    /// fn.py callers on the walk function.
    /// settles: the order of the game's component callbacks against the frame's pose (0076e100
    /// is the attachment follower; its callers are not read yet).
    pub fn component_sources(&self, car: bool, world: &[bevy::math::Affine3A]) {
        let mut playing = self.playing();
        let updates: Vec<_> = playing.triggered.iter().filter(|(key, _)| key.0 == car)
            .filter_map(|(key, handle)| {
                let definition = playing.trigger_definitions[car as usize].get(key.1)?;
                let point = world.get(definition.node)?.translation.into();
                Some((*handle, point))
            }).collect();
        let mut updates = updates;
        // [game: 007224a0] his voice line follows his Head node (robot form only here).
        if let (false, Some(voice), Some(head)) = (car, playing.voice, playing.head_node)
            && let Some(at) = world.get(head) { updates.push((voice, at.translation.into())); }
        let speed = playing.speed;
        for (handle, point) in updates {
            if let Some((_, source)) = playing.sources.iter_mut().find(|(h, _)| *h == handle) {
                *source = Source::Moving(point, speed);
            }
        }
        playing.spatial();
    }

    /// A one-shot at its world source, including the scout's gun rather than Bumblebee.
    pub fn play_event_at(&self, id: u32, point: Vec3) {
        let mut playing = self.playing();
        playing.play_at(&self.library, id, point, false);
        playing.spatial();
    }

    /// `start_kept` for a sound of someone else's (an enemy's gun): started at its world source,
    /// with `heat` on its `heat` parameter. `move_to` keeps it on its source.
    pub fn start_kept_at(&self, id: u32, heat: Option<f32>, point: Vec3) -> Option<Handle> {
        let mut playing = self.playing();
        let handle = playing.play_at(&self.library, id, point, false);
        if let (Some(handle), Some(heat)) = (handle, heat) {
            playing.mixer.set_parameter_hash(handle, HEAT, heat);
        }
        playing.spatial();
        handle
    }

    /// Sets a kept event's named parameter (Devastator's engine's `rpm` and `load`, `giant::dress`).
    pub fn set_param(&self, handle: Handle, name: &str, value: f32) {
        self.playing().mixer.set_parameter(handle, name, value);
    }

    /// Sets a kept sound's volume (Grindor's wave's roll fading out, `energon::dress`).
    pub fn set_volume(&self, handle: Handle, volume: f32) {
        self.playing().mixer.set_volume(handle, volume);
    }

    /// Moves a placed sound to a new world source (a kept enemy loop following its gun).
    pub fn move_to(&self, handle: Handle, point: Vec3) {
        let mut playing = self.playing();
        playing.place(handle, point, false);
        playing.spatial();
    }

    /// [game: 0076dcf0] Anonymous projectile players start at object creation.
    /// [assumed] Source is the simulated missile centre: its model-node offset is
    /// absent from this projectile adapter. Original source telemetry settles it.
    /// Second pass (2026-10-20): 0076dcf0 (sound_player_initialize) places the player at the
    /// owner object's node (0056bee0 with EAX = the player's node index, word +0x14, object = owner
    /// +0xc), so the original source is that node's world point, not the object's centre. The node
    /// index is data on each flight sound's SoundPlayer; the port's weapon effects keep only the
    /// event ids, and the projectile adapter has no model node transform. OPEN for that data.
    /// Third pass (2026-10-20): 0076dcf0 reads the node index itself (movsx of word +0x14 into EAX,
    /// raw 0076dd19; not set by its caller). 0056bee0 then gives the owner's own origin (+0x34..0x3c)
    /// when that index is 0 (raw 0056befb..0056bf06), else the node's instance matrix composed with
    /// 0052fbf0 (raw 0056bf08..0056bf2a). So the source is the missile's origin for a node-0 cue,
    /// which is the centre only if the missile's origin is its centre. Fourth pass (2026-10-20): word +0x14
    /// is written by the component base constructor 00577b40 (its u16 argument, from the factory record:
    /// the attachment's `index`) [game]. [data] Starscream's `homingmissiles` has its top-level SoundPlayer
    /// at index 3 (work\survey\weapons\starscream.txt l.643577) and the nested ones at 0, so its flight
    /// cue sits on node 3 of the missile's hierarchy, not at its origin. OPEN: which SoundPlayer carries
    /// the start cue, and node 3's place in the missile model (the port has no projectile node transform).
    pub fn projectile_start(&self, number: u32, weapon: u32, point: Vec3) {
        let mut playing = self.playing();
        let ids = playing.projectile_audio.get(&weapon).map(|p| p.0.clone()).unwrap_or_default();
        let handles = ids.into_iter().filter_map(|id| playing.play_at(&self.library, id, point, false)).collect();
        playing.projectiles.insert(number, handles);
        playing.spatial();
    }

    /// A projectile's players given by id rather than by a weapon row (a ground wave's
    /// SoundPlayers, `WaveEffects::sounds`): started at `point` and kept under `number` as
    /// `projectile_start`'s are, so `projectile_move` and `projectile_stop` reach them. [game
    /// 0076dcf0: a SoundPlayer with waitForTrigger 0 starts at its object's creation]
    pub fn projectile_start_sounds(&self, number: u32, ids: &[u32], point: Vec3) {
        let mut playing = self.playing();
        let handles: Vec<Handle> = ids.iter().filter_map(|&id| playing.play_at(&self.library, id, point, false)).collect();
        if std::env::var("TF_DBG").is_ok() {
            let names: Vec<&str> = ids.iter().map(|&id| self.library.name(id).unwrap_or("unnamed")).collect();
            eprintln!("sound start: projectile {number}: {names:?} ({} playing)", handles.len());
        }
        playing.projectiles.insert(number, handles);
        playing.spatial();
    }

    /// Each missile has its own handles and position; other missiles keep playing.
    pub fn projectile_move(&self, number: u32, point: Vec3, speed: f32) {
        let mut playing = self.playing();
        let handles = playing.projectiles.get(&number).cloned().unwrap_or_default();
        for (handle, source) in &mut playing.sources {
            if handles.contains(handle) { *source = Source::Moving(point, speed); }
        }
        playing.spatial();
    }

    /// Object destruction stops flight players and starts its detached players once.
    /// [assumed] Detached sound sources use the endpoint of the simulated missile;
    /// the original DetachLink matrices/source positions would settle their offsets.
    /// A projectile that goes without ending: its sounds stop and nothing plays (a mine expired
    /// by `oneOutAtATime`, 00789b10). Second pass (2026-10-20): 00789b10 (sticky_expire_fadeout)
    /// does not silence the mine: it broadcasts fadeOut (a80e5e89) and Trigger (d5d636c1) to the
    /// mine's children, then clears the proximity flag. The port's `Mixer::stop` (core sound.rs 804) lets
    /// each event fade over its own fade-out time, so the flight players fade here by their events'
    /// fade, not by the game's fadeOut message, and the Trigger's sounds (the children's own players)
    /// are not started. OPEN (see the fade and the Trigger's sound players). settles: the mine's child
    /// objects and their SoundPlayers' fadeOut / Trigger response (not read here), the DetachLink
    /// matrices, and the mine expiry path at 00789b10 (00786a60 arms the grenade at launch).
    pub fn projectile_stop(&self, number: u32) {
        let mut playing = self.playing();
        let Some(handles) = playing.projectiles.remove(&number) else { return };
        for handle in handles { playing.mixer.stop(handle); }
    }

    pub fn projectile_end(&self, number: u32, weapon: u32, point: Vec3) {
        let mut playing = self.playing();
        let Some(handles) = playing.projectiles.remove(&number) else { return };
        for handle in handles { playing.mixer.stop(handle); }
        let ids = playing.projectile_audio.get(&weapon).map(|p| p.1.clone()).unwrap_or_default();
        for id in ids { playing.play_at(&self.library, id, point, false); }
        playing.spatial();
    }

    /// [game: 0077cd60] The bullet's material impact preset supplies its sound too.
    pub fn weapon_impact(&self, weapon: u32, surface: u32, point: Vec3) {
        let mut playing = self.playing();
        let id = playing.weapon_impacts.get(&weapon).and_then(|p| p.pick(surface)).map_or(0, |i| i.sound);
        playing.play_at(&self.library, id, point, false);
        playing.spatial();
    }

    /// The cues of `clip` between two positions in ticks. `to` below `from` means the clip
    /// went round; a clip that has just started has `from` at 0 and its cues at tick 0 play.
    /// `rerun`: the clip had gone round before this stretch; a cue that does not allow a
    /// rerun (`allowReRun` 0) then stays quiet (`FUN_0053ab90`). [game]
    pub fn clip_cues(&self, clip: &Clip, from: f32, to: f32, rerun: bool) {
        if clip.sound_events.is_empty() {
            return;
        }
        let mut playing = self.playing();
        for event in due_cues(clip, from, to, rerun) {
            let t = event.time;
            if playing.trace && let SoundCue::Play { id, .. } = event.cue {
                eprintln!("sound clip cue: {} at tick {t:.1} of {:.1}, window {from:.2}..{to:.2}{}", self.library.name(id).unwrap_or("unnamed"), clip.length, if rerun { " rerun" } else { "" });
            }
            match event.cue {
                SoundCue::Play { id, auto_stop, held } => {
                    let handle = playing.play_cue(&self.library, Table::Robot, id, held, |p, l| p.play(l, id));
                    if let (Some(handle), true) = (handle, auto_stop)
                        && !playing.with_clip.contains(&handle)
                    {
                        playing.with_clip.push(handle);
                    }
                    let stopped_later = clip.sound_events.iter().any(|e| e.time >= t && e.cue == SoundCue::Stop { id });
                    if let (Some(handle), false, true) = (handle, auto_stop, stopped_later)
                        && !playing.until_stop.contains(&(id, handle))
                    {
                        playing.until_stop.push((id, handle));
                    }
                }
                SoundCue::Stop { id } => {
                    playing.until_stop.retain(|&(i, _)| i != id);
                    playing.stop_own_event(id)
                }
                SoundCue::FootStep { side } => playing.pending_steps.push(side),
                SoundCue::ClimbStep { limb } => playing.pending_climb_steps.push(limb),
            }
        }
    }

    /// Another character's (an enemy's) clip cues, as `clip_cues` for the player, played where
    /// it stands. [game] every character's clips play their cues through its own object's
    /// sound player, which places them at that object (the same path as the player's: the
    /// clip-event walk 007c3870 -> 005987e0, the object's sounds tied to its body by 00595770);
    /// before this the port played none of an enemy's clip sounds (only its gun).
    /// A Stop cue ends that character's own copies only, and `changed` (a new clip) ends the
    /// sounds its last clip asked to end with it (`autoStop`). [assumed] the Stop cue's scope
    /// is the object that played it, as the autoStop list is (anim_ref_load 0070ccd0 keeps it
    /// per object, +0x5c); settles: reading 005987e0's stop branch. Footstep cues wait for
    /// `other_steps`; climb cues are dropped (an enemy never climbs).
    #[allow(clippy::too_many_arguments)]
    pub fn other_clip_cues(&self, owner: u32, clip: &Clip, from: f32, to: f32, rerun: bool, changed: bool, point: Vec3, speed: f32) {
        let mut guard = self.playing();
        let playing = &mut *guard;
        if changed && let Some(other) = playing.others.get_mut(&owner) {
            for handle in std::mem::take(&mut other.with_clip) {
                playing.mixer.stop(handle);
            }
        }
        for event in due_cues(clip, from, to, rerun) {
            match event.cue {
                SoundCue::Play { id, auto_stop, held } => {
                    if playing.trace {
                        eprintln!("sound other cue: {owner} {} at tick {:.1}", self.library.name(id).unwrap_or("unnamed"), event.time);
                    }
                    let handle = playing.play_cue(&self.library, Table::Other(owner), id, held, |p, l| p.play_other(l, owner, id, point, speed));
                    if let (Some(handle), true) = (handle, auto_stop) {
                        let with_clip = &mut playing.others.entry(owner).or_default().with_clip;
                        if !with_clip.contains(&handle) {
                            with_clip.push(handle);
                        }
                    }
                }
                SoundCue::Stop { id } => {
                    let own: Vec<_> = playing.others.get(&owner).map_or_else(Vec::new, |o| o.handles.iter().filter(|&&(_, i)| i == id).map(|&(h, _)| h).collect());
                    for handle in own {
                        playing.mixer.stop(handle);
                    }
                }
                SoundCue::FootStep { side } => playing.others.entry(owner).or_default().pending_steps.push(side),
                SoundCue::ClimbStep { .. } => {}
            }
        }
        playing.spatial();
    }

    /// Another character's footstep cues against its posed world nodes, through its own
    /// `footStepFX` preset, sounding one update later where the foot met the ground: the
    /// player's `footsteps` path for an enemy. [game 0073f420, 0073f520, 00740d60]
    pub fn other_steps(&self, owner: u32, world: &[bevy::math::Affine3A], definitions: &[tf2_core::formats::scene::FootstepDef], preset: &ImpactPreset, arena: &impl sim::World, speed: f32) {
        let mut guard = self.playing();
        let playing = &mut *guard;
        let Some(other) = playing.others.get_mut(&owner) else { return };
        let sides = std::mem::take(&mut other.pending_steps);
        for side in sides {
            for hit in tf2_core::stepfx::footsteps(side, world, definitions, arena) {
                let Some(id) = preset.pick(hit.surface).map(|i| i.sound) else { continue };
                playing.other_steps.push((STEP_DISPATCH_DELAY, id, hit.point, speed));
            }
        }
    }

    /// Moves another character's sounds with it, and counts its steps down (once a frame,
    /// before its cues).
    pub fn others_tick(&self, dt: f32) {
        let mut playing = self.playing();
        playing.dispatch_other_steps(&self.library, dt);
    }

    /// Another character's sounds follow it: its body's place and speed. [game 00595770]
    pub fn follow(&self, owner: u32, point: Vec3, speed: f32) {
        let mut guard = self.playing();
        let playing = &mut *guard;
        let Some(other) = playing.others.get(&owner) else { return };
        let mut alive = Vec::new();
        for &(handle, id) in &other.handles {
            if playing.mixer.is_playing(handle) {
                alive.push((handle, id));
            }
        }
        for (handle, _) in &alive {
            if let Some((_, source)) = playing.sources.iter_mut().find(|(h, _)| h == handle) {
                *source = Source::Moving(point, speed);
            }
        }
        let other = playing.others.get_mut(&owner).unwrap();
        other.with_clip.retain(|h| alive.iter().any(|(a, _)| a == h));
        other.handles = alive;
        playing.spatial();
    }

    /// Another character is gone (removed or reset): its loops stop; its one-shots play out
    /// where it was.
    pub fn other_gone(&self, owner: u32) {
        let mut guard = self.playing();
        let playing = &mut *guard;
        playing.held.retain(|&(table, _), _| table != Table::Other(owner));
        let Some(other) = playing.others.remove(&owner) else { return };
        for (handle, id) in other.handles {
            if self.library.infinite(id) {
                playing.mixer.stop(handle);
            }
        }
    }

    /// The characters that have sounds here.
    pub fn others(&self) -> Vec<u32> {
        self.playing().others.keys().copied().collect()
    }

    /// Every sound in the mix with who started it, for the sound debug overlay (`sounddebug.rs`).
    pub fn debug_view(&self) -> DebugView {
        let playing = self.playing();
        let p = &*playing;
        let mut owners: HashMap<Handle, String> = HashMap::new();
        let mut own = |handle: Handle, owner: String| { owners.entry(handle).or_insert(owner); };
        for (&id, other) in &p.others {
            for &(h, _) in &other.handles { own(h, format!("enemy {id}")); }
            for &h in &other.with_clip { own(h, format!("enemy {id} clip")); }
        }
        for (&number, handles) in &p.projectiles {
            for &h in handles { own(h, format!("projectile {number}")); }
        }
        let singles = [(p.engine, "engine"), (p.flight_loop.map(|f| f.0), "flight loop"), (p.skid.map(|s| s.0), "tyre skid"),
            (p.road.map(|r| r.0), "tyre road"), (p.voice, "voice")];
        for (handle, owner) in singles {
            if let Some(h) = handle { own(h, owner.to_owned()); }
        }
        for &h in &p.with_clip { own(h, "clip".to_owned()); }
        for &h in p.weapon_clips.iter().flatten() { own(h, "weapon clip".to_owned()); }
        for &h in p.triggered.values() { own(h, "trigger".to_owned()); }
        for &h in p.collision_loops.values() { own(h, "collision".to_owned()); }
        for (h, source) in &p.sources {
            own(*h, match source { Source::Body(_) => "player", Source::Fixed(_) => "fixed point", Source::Moving(..) => "moving point" }.to_owned());
        }
        let mut sounds = Vec::new();
        for view in p.mixer.view() {
            let owner = owners.get(&view.handle).cloned().unwrap_or_else(|| "player".to_owned());
            let infinite = self.library.infinite(view.id);
            sounds.push(DebugSound { owner, infinite, view });
        }
        let reverb = match p.reverb.applied {
            Some(r) => format!("room {} mB, decay {:.2} s, reverb {} mB", r.room, r.decay_time, r.reverb),
            None => "off".to_owned(),
        };
        DebugView { sounds, reverb, master: p.mixer.master, output_time: p.mixer.output_time() }
    }

    /// Weapon arm/object cues use their own autoStop lists, independent of locomotion.
    pub fn weapon_clip_command(&self, command: tf2_core::gun::SoundCommand) {
        use tf2_core::gun::SoundCommand;
        let mut playing = self.playing();
        match command {
            SoundCommand::Changed { layer } => {
                for handle in std::mem::take(&mut playing.weapon_clips[layer]) {
                    playing.mixer.stop(handle);
                }
            }
            SoundCommand::Cue { layer, cue: SoundCue::Play { id, auto_stop, held } } => {
                // Layer 0 is the arm's partial set (the robot's clips), layer 1 the weapon object.
                let table = if layer == 0 { Table::Robot } else { Table::WeaponObject };
                if playing.trace {
                    eprintln!("sound weapon cue: layer {layer} {}{}", self.library.name(id).unwrap_or("unnamed"), if held { " held" } else { "" });
                }
                if let (Some(handle), true) = (playing.play_cue(&self.library, table, id, held, |p, l| p.play(l, id)), auto_stop)
                    && !playing.weapon_clips[layer].contains(&handle)
                {
                    playing.weapon_clips[layer].push(handle);
                }
            }
            SoundCommand::Cue { cue: SoundCue::Stop { id }, .. } => playing.stop_own_event(id),
            SoundCommand::Cue { cue: SoundCue::FootStep { side }, .. } => playing.pending_steps.push(side),
            SoundCommand::Cue { cue: SoundCue::ClimbStep { limb }, .. } => playing.pending_climb_steps.push(limb),
        }
    }

    /// The weapon object is hidden: its held clip sounds stop. [assumed] (see `weapons.rs`)
    pub fn weapon_object_hidden(&self) {
        let mut guard = self.playing();
        let playing = &mut *guard;
        let mut gone = Vec::new();
        playing.held.retain(|&(table, _), &mut handle| {
            let keep = table != Table::WeaponObject;
            if !keep { gone.push(handle); }
            keep
        });
        for handle in gone {
            playing.mixer.stop(handle);
        }
    }

    /// The robot's clip changed to `next`: sounds the old one asked to end with it stop.
    ///
    /// [assumed port design] So does a loop the old clip started that only a Stop cue later in
    /// that clip ends (autoStop 0 [data]), unless `next` cues the same event, which then holds it
    /// on (`play_cue`). Optimus's, Ratchet's and Ironhide's `BigJump_InAir` / `BigJump_Fall`
    /// start `ANIM_SHARED_WIND_LP` at tick 0 and stop it at 56 / 54; a landing within 0.9 s of
    /// the fall left the wind on, and its `speed` envelope (0.37 at 6.4 m/s) made it heard
    /// through the walk after (the user's report, 2026-10-25). As read, the original keeps such
    /// a loop (anim_ref_load 0070ccd0 lists it in neither +0x5c nor +0x68; 005984d0 holds it in
    /// the object's `FXEventQ<lux::Sound>` until a Stop finds it); what else empties that queue
    /// is not read. Settles: reading the queue's other users, or a recording of one of the three
    /// landing a short fall out of the car's leap.
    pub fn clip_changed(&self, next: Option<&Clip>) {
        let mut guard = self.playing();
        let playing = &mut *guard;
        for handle in std::mem::take(&mut playing.with_clip) {
            playing.mixer.stop(handle);
        }
        let cued = |id: u32| next.is_some_and(|c| c.sound_events.iter().any(|e| matches!(e.cue, SoundCue::Play { id: i, .. } if i == id)));
        for (id, handle) in std::mem::take(&mut playing.until_stop) {
            if cued(id) {
                playing.until_stop.push((id, handle));
            } else {
                playing.mixer.stop(handle);
            }
        }
    }

    /// Resolve each named foot's cue against its current animated world position, and play it one
    /// update later. The probe: footstep_probe 0073f420 submits a segment from the bone (z -
    /// heightFromGround +1.5 to -1.5, mask 0x11) to the world's queue (probe_submit 006e0a30);
    /// the world step 006dedf0 (priority 14) resolves it; the FootStep, in the IK ring 00c05d68
    /// (footstep_factory 0073f5f0), is updated by ik_update_all 00740d60 (priority 12), whose
    /// foot_step_update 0073f520 dispatches the completed contact. World::Update 005a8740 walks the
    /// update list (game state 00dadde0 + 0x48 = 00dade28) along node[1] in ascending priority, as
    /// 01174360 inserts, so the dispatch (12) comes before the resolve (14) in an update: the cue
    /// lands one update after its probe resolves. [game 005a8740, 01174360, 00740d60, 006dedf0]
    /// The probe is submitted in the update that resolves it: the characters update at priority 5
    /// (00ad7670 registers 0071eb10 -> 0071e930, which calls each character's 0071dc20), before the
    /// world step at 14, so the lag is exactly one update [game 00ad7670]; the port probes at the
    /// current pose, as the game's segment is taken at submission.
    pub fn footsteps(&self, world: &[bevy::math::Affine3A], definitions: &[tf2_core::formats::scene::FootstepDef], arena: &impl sim::World, dt: f32) {
        let mut playing = self.playing();
        // What earlier updates probed sounds first; this update's probes wait. [game 005a8740]
        playing.dispatch_steps(&self.library, dt);
        for side in std::mem::take(&mut playing.pending_steps) {
            for hit in tf2_core::stepfx::footsteps(side, world, definitions, arena) {
                let Some(id) = playing.step_impact.pick(hit.surface).map(|i| i.sound) else { continue };
                playing.delayed_steps.push((STEP_DISPATCH_DELAY, id, hit.point));
            }
        }
        playing.spatial();
    }

    /// [game: 0073fa20] ClimbStep probes 1.5m either side of the limb, along the
    /// object's forward row, and the contact's material selects its preset.
    /// climbstep_event_forward 0073ff10 hands the step to footstep_event 0073f780, so a climb cue
    /// goes through the same probe queue as the footstep and lands one update late, as
    /// `footsteps` (its tags). [game 0073ff10, 0073f780]
    pub fn climb_steps(&self, world: &[bevy::math::Affine3A], steps: &[(usize, u32)], forward: Vec3, arena: &impl sim::World, dt: f32) {
        // The shared queue is counted down once a frame, by `footsteps` (called first). `_dt` is kept
        // so the two calls read alike.
        let _ = dt;
        let mut playing = self.playing();
        let pending = std::mem::take(&mut playing.pending_climb_steps);
        for limb in pending {
            for hit in tf2_core::stepfx::climbing(limb, world, steps, forward, arena) {
                let id = playing.climb_impact.get(limb).and_then(|p| p.pick(hit.surface)).map_or(0, |i| i.sound);
                playing.delayed_steps.push((STEP_DISPATCH_DELAY, id, hit.point));
            }
        }
        playing.spatial();
    }

    /// [game: 0076dcf0/007fc270] A spawner's anonymous SoundPlayer starts with its
    /// object at the named bone. [assumed] Use the frame's final animated bone pose,
    /// matching our foot/climb probe ordering. Read 2026-10-20: the game reads the bone at the
    /// spawner's Trigger, not at a probe pass: object_spawner_on_bone_trigger 007fc270 calls
    /// object_node_world_place 0056bee0 for the node when the Trigger arrives, and places the
    /// spawned object there; 0056bee0 reads the object's stored matrix table (object +0x68, entry
    /// N-1; node 0 is the object place +0x34), so the game takes the LAST WRITTEN pose at that
    /// moment. The spawned SoundPlayer's sound_player_initialize 0076dcf0 reads its node again at
    /// creation. The port's `pending_spawns` are filled from the clip's triggers (`state.triggers`,
    /// animation.rs l.302 clip_events, which runs before the pose sample at l.334-344) and flushed
    /// here with the final pose of this frame. So the port differs if the game's Trigger dispatch
    /// precedes its pose write (then the game uses the previous frame's pose). settles: the order
    /// of the animation update's Trigger dispatch against the pose write, and the writer of the
    /// object's matrix table (the caller of 007fc270's Trigger path, not read).
    pub fn spawn_cues(&self, world: &[bevy::math::Affine3A], definitions: &[(usize, u32, Vec<u32>)]) {
        let mut playing = self.playing();
        for name in std::mem::take(&mut playing.pending_spawns) {
            for (node, _, ids) in definitions.iter().filter(|(_, script, _)| *script == name) {
                let Some(point) = world.get(*node).map(|at| Vec3::from(at.translation)) else { continue };
                for &id in ids { playing.play_at(&self.library, id, point, false); }
            }
        }
        playing.spatial();
    }

    /// Once a frame: the engine, the turbo, and where the listener (the camera, at `eye`
    /// looking along `forward`) is from him.
    pub fn tick(&self, state: &State, tuning: &Tuning, eye: Vec3, forward: Vec3, dt: f32) {
        let mut guard = self.playing();
        let playing = &mut *guard;
        // [game: 00771540 -> 00598050] FMOD's listener is the camera: its position, its look
        // (matrix row 1) and its up (row 2).
        let ear = Ear::new(eye, forward);
        playing.listener = ear;
        playing.mixer.offset = ear.local(state.pos - eye);
        // [game: 00771540] the reverb's listener is FMOD's: the camera.
        playing.reverb.listener = eye;
        if let Some((left, preset)) = playing.reverb_pending.as_mut() {
            *left -= dt;
            if *left <= 0.0 {
                let preset = *preset;
                playing.reverb_pending = None;
                let presets = std::mem::take(&mut playing.reverb_presets);
                playing.reverb.trigger(&presets, preset);
                eprintln!("sound: the stage's ReverbTrigger sets preset {preset}{}", if preset == 0 { " (does nothing)" } else { "" });
                playing.reverb_presets = presets;
            }
        }
        // [game: 00771540 / 00772860] its velocity is the player's motion over the update.
        let velocity = playing.listener_motion.update(state.pos, dt);
        playing.mixer.listener_velocity = ear.local(velocity);
        playing.speed = state.vel.length();
        playing.position = state.pos;
        playing.kept(&self.library, state, tuning);
        if state.damage_timers.regenerating && !playing.regenerating {
            playing.play(&self.library, tf2_core::sound::event_id("UI_REGEN_START"));
        }
        playing.regenerating = state.damage_timers.regenerating;
        playing.presets_tick(state, dt);
        playing.collisions(&self.library, state, dt);
        playing.chatter(&self.library, state, tuning, dt);
        playing.pending_spawns.extend(&state.triggers);
        // [game] each hit of his own body plays his melee preset's sound for the surface
        // struck (`FUN_007189b0` -> `FUN_0072d9c0`), with his body's speed as the event's
        // `speed` (a second parameter, hash d9a785a4, gets the same; its name is not known).
        // An explosion's hits do not come this way. Not there: the preset's effect
        // (`r_hit_melee`, tied to the object hit) and the mark on the surface.
        for hit in state.hits.iter().filter(|hit| !hit.blast) {
            if let Some(id) = playing.melee_impact.pick(hit.surface).map(|info| info.sound) {
                playing.play_at(&self.library, id, hit.point, false);
            }
        }
        let vehicle = state.is_vehicle();
        if vehicle && playing.engine.is_none_or(|handle| !playing.mixer.is_playing(handle)) {
            let id = playing.sounds.engine;
            playing.engine = playing.play(&self.library, id);
        } else if !vehicle && let Some(handle) = playing.engine.take() {
            playing.mixer.stop(handle);
        }
        playing.flight_loop_tick(&self.library, state, tuning, dt);
        // The trace's "still playing": once a second, every instance older than 4 s, with the
        // player's form, to find sounds that never leave.
        if tf2_core::sound::tracing() {
            playing.trace_clock -= dt;
            if playing.trace_clock <= 0.0 {
                playing.trace_clock += 1.0;
                for (name, age, stopping, sounding, released, values) in playing.mixer.report() {
                    if age > 4.0 {
                        eprintln!("sound still: {name} {age:.1}s{} audible {sounding} keyoffs {released} parameters {values:?} (vehicle {vehicle})", if stopping { " stopping" } else { "" });
                    }
                }
                // Where the placed sounds are from the listener (x right, y front, z up).
                let ear = playing.listener;
                let places: Vec<String> = playing.sources.iter().map(|(_, source)| {
                    let (kind, point) = match source { Source::Body(o) => ("body", playing.position + *o), Source::Fixed(p) => ("fixed", *p), Source::Moving(p, _) => ("moving", *p) };
                    let at = ear.local(point - ear.eye);
                    format!("{kind} ({:.1} {:.1} {:.1})", at.x, at.y, at.z)
                }).collect();
                eprintln!("sound places: ear at {:.1?} facing {:.2?}: {}", ear.eye, ear.front, places.join(", "));
            }
        }
        // [game] The sound reads the same gearbox that the wheel set updated.
        // A separate render-frame gearbox can miss the start of a transformation
        // and diverge when drive updates precede the visible vehicle form.
        if let (true, Some(handle), Some(gearbox)) = (vehicle, playing.engine, state.vehicle_gearbox.as_ref()) {
            playing.mixer.set_parameter(handle, "rpm", gearbox.rpm);
            playing.mixer.set_parameter(handle, "load", gearbox.load);
        }
        // [game] the tyres and the suspension (`FUN_007347c0`, `tyre_sounds`): a rolling
        // loop and, while they skid, a skid loop, each picked by the ground's surface from
        // his `vehicleTireRoadFX` / `vehicleTireSkidFX`, and `vehicleSuspension` when a
        // rear spring is knocked short. Out of the car both loops stop.
        let mut heard = state.tyre_inputs.last().map(tyre_sounds);
        if !vehicle {
            heard = Some(TyreSounds::default());
        }
        if let Some(heard) = heard {
            playing.tyre_loop(&self.library, true, heard.skid);
            playing.tyre_loop(&self.library, false, heard.road);
        }
        // [game] both loops follow the body's speed (`FUN_00595770` ties the event's
        // `speed` to the object it is on).
        for (handle, _) in playing.skid.into_iter().chain(playing.road) {
            playing.mixer.set_parameter(handle, "speed", playing.speed);
            if let Some(input)=state.tyre_inputs.last().filter(|input|input.per_wheel) {
                playing.place(handle, input.wheels[input.sound_wheel].point, true);
            }
        }
        for input in state.tyre_inputs.iter().filter(|_| vehicle) {
            let Some(force)=tyre_sounds(input).suspension else {continue;};
            let id = playing.sounds.suspension;
            // The force goes to a parameter the event does not have, as in the game
            // (`IMPACT_FORCE`): the mixer ignores a name it does not find.
            if let Some(handle) = playing.play(&self.library, id) {
                playing.mixer.set_parameter(handle, IMPACT_FORCE, force);
                if input.per_wheel {
                    playing.place(handle, input.wheels[input.suspension_wheel].point, true);
                }
            }
        }
        // [data] the sound table's turbo event when it starts, and its "ready" when the
        // cooldown runs out.
        // [game: 00732e60] Both standard and drift turbo play this event on start.
        if vehicle && state.turbo_started {
            let id = playing.sounds.turbo;
            playing.play(&self.library, id);
        }
        if vehicle && playing.was_vehicle && state.turbo_cooldown <= 0.0 && playing.turbo_cooldown > 0.0 {
            let id = playing.sounds.turbo_ready;
            playing.play(&self.library, id);
        }
        playing.turbo_left = state.turbo_left;
        playing.turbo_cooldown = state.turbo_cooldown;
        if tf2_core::sound::tracing() {
            if vehicle != playing.was_vehicle {
                eprintln!("sound form: vehicle {vehicle} at output {:.2}s", playing.mixer.output_time());
                playing.mixer.note(|| format!("form\t\t\t{}", if vehicle { "vehicle" } else { "robot" }));
            }
            let mode = format!("{:?}", state.mode);
            if mode != playing.trace_mode {
                playing.mixer.note(|| format!("state\t\t\t{mode}"));
                playing.trace_mode = mode;
            }
        }
        playing.was_vehicle = vehicle;
        playing.spatial();
    }

    /// Everything fades out (a reset).
    pub fn silence(&self) {
        let mut playing = self.playing();
        playing.mixer.stop_all();
        playing.with_clip.clear();
        playing.until_stop.clear();
        playing.weapon_clips.iter_mut().for_each(Vec::clear);
        playing.engine = None;
        playing.skid = None;
        playing.road = None;
        playing.was_vehicle = false;
        playing.regenerating = false;
        playing.sources.clear();
        playing.collision_defs = playing.collision_original.clone();
        playing.collision_history.clear();
        playing.collision_pending.clear();
        playing.collision_loops.clear();
        playing.collision_clock = 0.0;
        playing.voice = None;
        playing.pending_voice = None;
        playing.combat_target = false;
        playing.voice_delay = 0.0;
        playing.low_health_spoken = false;
        playing.death_spoken = false;
        playing.chatter_clock = 0.0;
        playing.pending_steps.clear();
        playing.delayed_steps.clear();
        playing.others.clear();
        playing.other_steps.clear();
        playing.triggered.clear();
        playing.triggered_once.clear();
        playing.kept_sounds.clear();
        playing.projectiles.clear();
        playing.pending_climb_steps.clear();
        playing.pending_spawns.clear();
        if let Some(speaker) = &mut playing.chatter { speaker.clear(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_weapon_equip_stow_and_both_switch_directions_render_audio() {
        use tf2_core::gun::{Clips, Gun, SoundCommand};
        let Some((sound, _, data)) = offline() else { return };
        let clips = Clips {
            arm: &data.animations.sets["WeaponPartialSet"],
            entries: &data.animations.incoming["WeaponPartialSet"],
            object: &data.weapon_animations[0],
        };
        let weapons = &data.weapons[..2]; // CharacterData stores primary/secondary before the car's guns.
        let mut gun = Gun::default();
        let run = |gun: &mut Gun, weapon: &tf2_core::weapons::WeaponDef, expected: &str| {
            sound.silence();
            let mut ids = Vec::new();
            let mut peak = 0.0_f32;
            for _ in 0..100 {
                gun.step(clips, weapon.name, sim::GAME_UPDATE);
                for command in gun.sound_commands.drain(..) {
                    if let SoundCommand::Cue { cue: SoundCue::Play { id, .. }, .. } = command { ids.push(id); }
                    sound.weapon_clip_command(command);
                }
                let mut samples = [0.0; 3072];
                sound.playing().mixer.render(&sound.library, &mut samples);
                peak = samples.iter().map(|s| s.abs()).fold(peak, f32::max);
            }
            assert!(ids.contains(&tf2_core::sound::event_id(expected)), "{expected}: {ids:x?}");
            assert!(peak > 0.001, "{expected} must produce audible samples");
        };
        gun.take_aim(clips, &weapons[0], false);
        run(&mut gun, &weapons[0], "ANIM_BUMB_WEPN_R2W_A");
        gun.switched(clips, weapons[1].name);
        run(&mut gun, &weapons[1], "ANIM_BUMB_WEPN_A2B");
        gun.switched(clips, weapons[0].name);
        run(&mut gun, &weapons[0], "ANIM_BUMB_WEPN_B2A");
        gun.put_away(clips, &weapons[0]);
        run(&mut gun, &weapons[0], "ANIM_BUMB_WEPN_W2R_A");
        gun.take_aim(clips, &weapons[1], false);
        run(&mut gun, &weapons[1], "ANIM_BUMB_WEPN_R2W_B");
    }

    #[test]
    fn weapon_auto_stop_is_scoped_to_its_clip_player() {
        use tf2_core::gun::SoundCommand;
        let Some((sound, _, _)) = offline() else { return };
        let cue = SoundCue::Play { id: tf2_core::sound::event_id("ANIM_BUMB_WEPN_R2W_A"), auto_stop: true, held: false };
        sound.weapon_clip_command(SoundCommand::Cue { layer: 1, cue });
        let handle = sound.playing().weapon_clips[1][0];
        sound.clip_changed(None);
        sound.weapon_clip_command(SoundCommand::Changed { layer: 0 });
        assert!(sound.playing().mixer.is_playing(handle), "base and arm changes cannot cancel the weapon object sound");
        sound.weapon_clip_command(SoundCommand::Changed { layer: 1 });
        assert!(!sound.playing().mixer.is_playing(handle));
        assert!(sound.playing().weapon_clips[1].is_empty());
    }

    /// Bumblebee's sounds read from the install; None (the test skips) without it.
    fn offline() -> Option<(SoundOut, Tuning, CharacterData)> {
        if tf2_core::install::missing() {
            return None;
        }
        let game = tf2_core::install::dir();
        let data = tf2_core::character::load(game, "bumblebee").unwrap();
        let tuning = tf2_core::tuning::load(game, "Bumblebee").unwrap();
        let (mut library, errors) = Library::load(&[
            game.join(r"characters\eng\bumblebee.fev"), game.join(r"levels\us_city_00\eng\mission.fev"),
            game.join(r"audio\eng\global.fev"), game.join(r"audio\eng\global_streaming.fev"),
        ]);
        assert!(errors.is_empty(), "{errors:?}");
        let ids: Vec<_> = data.chatter.iter().map(|l| l.event).collect();
        library.add_programmer_bank(&game.join(r"audio\eng\CHATTER.fsb"), tf2_core::sound::event_id("ProgrammerSoundPlayer"), &ids).unwrap();
        let speaker = chatter::Speaker::new(data.chatter.clone(), chatter::Tuning::load(game).unwrap());
        let playing = Playing::new(&data, &tuning, Some(speaker));
        Some((SoundOut { library: Arc::new(library), playing: Arc::new(Mutex::new(playing)), stopped: Default::default() }, tuning, data))
    }

    #[test]
    fn regeneration_is_an_edge_and_chatter_waits_before_playing_then_resets() {
        let Some((sound, tuning, _)) = offline() else { return };
        let mut state = State::new(&tuning);
        let tick = |state: &State| sound.tick(state, &tuning, Vec3::Y * -10.0, Vec3::Y, sim::GAME_UPDATE);
        state.damage_timers.regenerating = true;
        tick(&state);
        assert_eq!(sound.playing().mixer.playing(), 1);
        tick(&state);
        assert_eq!(sound.playing().mixer.playing(), 1, "regeneration start must not repeat every frame");
        sound.silence();
        sound.playing().mixer.render(&sound.library, &mut [0.0; 9600]);
        state.damage_timers.regenerating = false;
        state.health = tuning.base_health * 0.1;
        for _ in 0..100 {
            tick(&state);
            if sound.playing().pending_voice.is_some() { break; }
        }
        let delay = sound.playing().voice_delay;
        assert!((0.3..=1.5).contains(&delay));
        assert!(sound.playing().pending_voice.is_some());
        assert_eq!(sound.playing().mixer.playing(), 0, "queued speech is silent until its delay");
        for _ in 0..(delay / sim::GAME_UPDATE).floor() as usize { tick(&state); }
        assert_eq!(sound.playing().mixer.playing(), 0);
        tick(&state);
        assert_eq!(sound.playing().mixer.playing(), 1);
        assert!(sound.playing().voice.is_some());
        sound.silence();
        let playing = sound.playing();
        assert!(playing.pending_voice.is_none() && playing.voice.is_none());
        assert!(!playing.low_health_spoken && !playing.death_spoken);
    }

    #[test]
    fn collision_scraping_loop_expires_and_unfolding_clears_contact_audio() {
        let Some((sound, tuning, _)) = offline() else { return };
        sound.playing().chatter = None;
        let mut state = State::new(&tuning);
        state.mode = sim::Mode::Drive;
        state.car_mode = true;
        state.vehicle_contacts.push(tf2_core::vehicle::Contact { key: 77, fraction: 0.0, point: Vec3::Y,
            normal: Vec3::X, depth: 0.0, surface: tf2_core::melee::surface::DEFAULT, velocity: Vec3::Y * 20.0 });
        let tick = |state: &State| sound.tick(state, &tuning, Vec3::Y * -10.0, Vec3::Y, sim::GAME_UPDATE);
        for _ in 0..8 { tick(&state); }
        let handle = *sound.playing().collision_loops.values().next().expect("scrape starts after seven slide updates");
        assert!(sound.playing().mixer.is_playing(handle));
        tick(&state);
        assert_eq!(sound.playing().collision_loops.values().next(), Some(&handle), "steady scrape must retain the same loop");
        state.vehicle_contacts.clear();
        for _ in 0..10 { tick(&state); }
        assert!(sound.playing().collision_loops.is_empty());
        assert!(!sound.playing().mixer.is_playing(handle));
        state.mode = sim::Mode::Stand;
        state.car_mode = false;
        tick(&state);
        assert!(sound.playing().collision_pending.is_empty());
        assert!(sound.playing().collision_history.clear().is_empty());
    }

    #[test]
    fn a_footstep_requires_its_named_foot_to_probe_ground_and_consumes_the_cue() {
        let Some((sound, tuning, data)) = offline() else { return };
        let arena = crate::world::layout(&tuning);
        let mut world = vec![bevy::math::Affine3A::IDENTITY; data.robot.nodes.len()];
        let foot = &data.robot.footsteps[0];
        // A standing foot's bone is a little above the floor: the game's probe (0073f420) starts
        // 1.5 m above its ground estimate, which for Bumblebee (heightFromGround 1.5) is the bone itself.
        world[foot.node].translation.z = 0.1;
        sound.playing().pending_steps.push(foot.side);
        sound.footsteps(&world, &data.robot.footsteps, &arena, 0.0);
        assert_eq!(sound.playing().mixer.playing(), 0, "probed now, dispatched one update later");
        sound.footsteps(&world, &data.robot.footsteps, &arena, sim::GAME_UPDATE);
        assert_eq!(sound.playing().mixer.playing(), 1);
        sound.footsteps(&world, &data.robot.footsteps, &arena, sim::GAME_UPDATE);
        assert_eq!(sound.playing().mixer.playing(), 1, "the same cue is consumed once");
        world[foot.node].translation.z = 100.0;
        sound.playing().pending_steps.push(foot.side);
        sound.footsteps(&world, &data.robot.footsteps, &arena, sim::GAME_UPDATE);
        sound.footsteps(&world, &data.robot.footsteps, &arena, sim::GAME_UPDATE);
        assert_eq!(sound.playing().mixer.playing(), 1, "a midair probe must not invent a ground sound");
    }

    /// [game 0076e450] An Untrigger keys a `keyOffOnUnTrigger` player's event off, stops any
    /// other's with its fade, and leaves players of other names alone.
    #[test]
    fn untrigger_keys_off_or_stops_the_named_players_only() {
        let Some((sound, _, _)) = offline() else { return };
        // A loop with a primary parameter (a key-off of an event without one does nothing, as in the game).
        let id = tf2_core::sound::event_id("ANIM_SHARED_WIND_LP");
        let def = |script, key_off| tf2_core::formats::scene::SoundPlayerDef { node: 0, script, id,
            wait_for_trigger: true, once: false, retrigger: true, key_off_on_untrigger: key_off };
        sound.playing().trigger_definitions[0] = vec![def(7, true), def(8, false), def(9, true)];
        for index in 0..3 {
            let d = sound.playing().trigger_definitions[0][index].clone();
            sound.trigger_event(false, index, &d);
        }
        sound.untrigger_name(7);
        sound.untrigger_name(8);
        let report = sound.playing().mixer.report();
        assert_eq!(report.len(), 3);
        // (stopping, key-offs) in start order: keyed off, faded, untouched.
        let states: Vec<_> = report.iter().map(|r| (r.2, r.4)).collect();
        assert_eq!(states, [(false, 1), (true, 0), (false, 0)]);
        assert_eq!(sound.playing().triggered.len(), 1, "only the untouched player stays active");
    }

    #[test]
    fn retrigger_replaces_the_components_handle_without_stopping_an_unrelated_cue() {
        let Some((sound, _, _)) = offline() else { return };
        let id = tf2_core::sound::event_id("WPN_BUMBLEBEE_SPEC_FIRE");
        let mut definition = tf2_core::formats::scene::SoundPlayerDef { node: 0, script: 1, id, wait_for_trigger: true, once: false, retrigger: true, key_off_on_untrigger: false };
        sound.trigger_event(false, 0, &definition);
        let first = sound.playing().triggered[&(false, 0)];
        let other = sound.playing().play(&sound.library, tf2_core::sound::event_id("UI_LOCK_ON")).unwrap();
        sound.trigger_event(false, 0, &definition);
        let next = sound.playing().triggered[&(false, 0)];
        definition.retrigger = false;
        sound.trigger_event(false, 0, &definition);
        let playing = sound.playing();
        assert_eq!(playing.triggered[&(false, 0)], next, "turbo's allowRetrigger=false retains its active event");
        assert_ne!(first, next);
        assert!(!playing.mixer.is_playing(first));
        assert!(playing.mixer.is_playing(next) && playing.mixer.is_playing(other));
    }

    #[test]
    fn camaro_boost_sound_follows_fx_messages_and_stops_on_mode_exit() {
        let Some((sound, tuning, data)) = offline() else { return };
        sound.playing().chatter = None;
        let mut state = State::new(&tuning);
        state.mode = sim::Mode::Drive;
        state.car_mode = true;
        state.turbo_fx_on = true;
        let tick = |state: &State| sound.tick(state, &tuning, Vec3::Y * -10.0, Vec3::Y, sim::GAME_UPDATE);
        let index = data.vehicle.as_ref().unwrap().sound_players.iter().position(|p| p.script == tf2_core::fxstate::TURBO).unwrap();
        tick(&state);
        let handle = sound.playing().triggered[&(true, index)];
        assert!(sound.playing().mixer.is_playing(handle));
        tick(&state);
        assert_eq!(sound.playing().triggered[&(true, index)], handle);
        state.turbo_fx_on = false;
        tick(&state);
        assert!(!sound.playing().mixer.is_playing(handle));
        state.turbo_fx_on = true;
        tick(&state);
        let next = sound.playing().triggered[&(true, index)];
        assert_ne!(handle, next);
        state.mode = sim::Mode::Unfold;
        tick(&state);
        assert!(!sound.playing().mixer.is_playing(next));
    }

    #[test]
    fn component_sound_follows_its_bone_turn_without_moving_other_forms_or_cues() {
        let Some((sound, _, _)) = offline() else { return };
        let definition = tf2_core::formats::scene::SoundPlayerDef { node: 1,
            script: 1, id: tf2_core::sound::event_id("WPN_BUMBLEBEE_SPEC_FIRE"),
            wait_for_trigger: true, once: false, retrigger: true, key_off_on_untrigger: false };
        sound.playing().trigger_definitions[0][0] = definition.clone();
        sound.trigger_event(false, 0, &definition);
        let handle = sound.playing().triggered[&(false, 0)];
        let other = sound.playing().play(&sound.library, definition.id).unwrap();
        let root = bevy::math::Affine3A::from_rotation_translation(
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), Vec3::new(10.0, 20.0, 3.0));
        let bone = root * bevy::math::Affine3A::from_translation(Vec3::new(2.0, 0.0, 1.0));
        sound.component_sources(false, &[root, bone]);
        let point = Vec3::new(10.0, 22.0, 4.0);
        {
            let playing = sound.playing();
            let source = &playing.sources.iter().find(|(h, _)| *h == handle).unwrap().1;
            assert!(matches!(source, Source::Moving(at, _) if at.distance(point) < 1e-5));
            assert!(matches!(playing.sources.iter().find(|(h, _)| *h == other).unwrap().1, Source::Body(_)),
                "an unrelated animation cue must keep its own source");
        }
        // Hidden/other-form node arrays cannot move this robot component.
        sound.component_sources(true, &[bevy::math::Affine3A::from_translation(Vec3::splat(300.0)); 2]);
        // Missing nodes cannot relocate a valid source to the character centre.
        sound.component_sources(false, &[root]);
        let playing = sound.playing();
        assert!(matches!(&playing.sources.iter().find(|(h, _)| *h == handle).unwrap().1,
            Source::Moving(at, _) if at.distance(point) < 1e-5));
    }

    #[test]
    fn missiles_keep_independent_flight_handles_and_explode_once_at_their_endpoint() {
        let Some((sound, _, data)) = offline() else { return };
        let weapon = data.weapons.iter().find(|w| w.kind == tf2_core::weapons::Kind::Launcher).unwrap().name;
        let first_point = Vec3::new(1.0, 5.0, 3.0);
        let second_point = Vec3::new(-4.0, 10.0, 3.0);
        sound.projectile_start(1, weapon, first_point);
        sound.projectile_start(2, weapon, second_point);
        let first = sound.playing().projectiles[&1][0];
        let second = sound.playing().projectiles[&2][0];
        let endpoint = Vec3::new(20.0, 30.0, 3.0);
        sound.projectile_move(1, endpoint, 80.0);
        {
            let playing = sound.playing();
            assert!(matches!(playing.sources.iter().find(|(h, _)| *h == first).unwrap().1,
                Source::Moving(point, speed) if point == endpoint && speed == 80.0));
            assert!(matches!(playing.sources.iter().find(|(h, _)| *h == second).unwrap().1,
                Source::Fixed(point) if point == second_point));
        }
        sound.projectile_end(1, weapon, endpoint);
        let count = sound.playing().mixer.playing();
        assert_eq!(count, 3, "one remaining flight loop, explosion, and the authored stop fade of the ended loop");
        sound.projectile_end(1, weapon, endpoint);
        {
            let playing = sound.playing();
            assert_eq!(playing.mixer.playing(), count, "ending one missile again must not repeat the explosion");
            assert!(!playing.mixer.is_playing(first) && playing.mixer.is_playing(second));
            assert!(!playing.projectiles.contains_key(&1));
            assert!(playing.sources.iter().any(|(h, s)| *h != second && matches!(s, Source::Fixed(point) if *point == endpoint)));
        }
        sound.silence();
        assert!(sound.playing().projectiles.is_empty());
    }

    #[test]
    fn spawned_sound_uses_the_animated_bone_and_consumes_its_trigger() {
        let Some((sound, _, data)) = offline() else { return };
        let (node, script, ids) = &data.robot.spawn_sound_players[0];
        let point = Vec3::new(3.0, 6.0, 2.0);
        let mut world = vec![bevy::math::Affine3A::IDENTITY; data.robot.nodes.len()];
        world[*node].translation = point.into();
        sound.playing().pending_spawns.push(*script);
        sound.spawn_cues(&world, &data.robot.spawn_sound_players);
        assert_eq!(sound.playing().mixer.playing(), ids.len());
        assert!(sound.playing().sources.iter().all(|(_, s)| matches!(s, Source::Fixed(at) if *at == point)));
        sound.spawn_cues(&world, &data.robot.spawn_sound_players);
        assert_eq!(sound.playing().mixer.playing(), ids.len(), "a spawner pulse plays once");
    }

    #[test]
    fn climbing_sound_requires_the_limbs_wall_probe_and_consumes_the_cue() {
        let Some((sound, _, data)) = offline() else { return };
        let mut world = vec![bevy::math::Affine3A::IDENTITY; data.robot.nodes.len()];
        let side = tf2_core::formats::anim::CLIMB_SIDES[0];
        let node = data.robot.climb_steps.iter().find(|(_, s)| *s == side).unwrap().0;
        world[node].translation = Vec3::new(0.0, 0.0, 3.0).into();
        let arena = sim::Arena { boxes: vec![sim::Box3 { min: Vec3::new(-2.0, 1.0, 0.0), max: Vec3::new(2.0, 2.0, 6.0) }], ..Default::default() };
        // A frame: the shared queue is counted down by `footsteps`, then the climb probes queue.
        let frame = |world: &[bevy::math::Affine3A]| {
            sound.footsteps(world, &data.robot.footsteps, &arena, sim::GAME_UPDATE);
            sound.climb_steps(world, &data.robot.climb_steps, Vec3::Y, &arena, sim::GAME_UPDATE);
        };
        sound.playing().pending_climb_steps.push(0);
        frame(&world);
        assert_eq!(sound.playing().mixer.playing(), 0, "probed now, dispatched one update later");
        frame(&world);
        assert_eq!(sound.playing().mixer.playing(), 1);
        frame(&world);
        assert_eq!(sound.playing().mixer.playing(), 1, "a step cue is consumed once");
        world[node].translation.z = 100.0;
        sound.playing().pending_climb_steps.push(0);
        frame(&world);
        frame(&world);
        assert_eq!(sound.playing().mixer.playing(), 1, "without a wall contact no material cue plays");
    }

    /// Left and right energy of `seconds` of the mix.
    fn sides(sound: &SoundOut, seconds: f32) -> (f32, f32) {
        let mut playing = sound.playing();
        let mut buffer = vec![0.0; 2 * 1024];
        let (mut left, mut right) = (0.0, 0.0);
        for _ in 0..(seconds * 48000.0 / 1024.0) as usize {
            playing.mixer.render(&sound.library, &mut buffer);
            for pair in buffer.as_chunks::<2>().0 {
                left += pair[0] * pair[0];
                right += pair[1] * pair[1];
            }
        }
        (left, right)
    }

    /// An enemy's clip cue sounds where the enemy stands: off to the listener's right it is
    /// louder on the right, and it moves with the enemy (`follow`). [game 007c3870 -> 005987e0]
    #[test]
    fn another_characters_cue_is_placed_at_it_and_follows_it() {
        let Some((sound, _, _)) = offline() else { return };
        let mut clip = Clip { length: 30.0, ..Default::default() };
        let id = tf2_core::sound::event_id("AUTOBOT_SM_STEP_DEFAULT");
        assert!(sound.library.has(id));
        sound.library.prepare(id);
        clip.sound_events.push(tf2_core::formats::anim::SoundEvent { time: 0.0, cue: SoundCue::Play { id, auto_stop: false, held: false }, rerun: false });
        sound.playing().listener = Ear::new(Vec3::ZERO, Vec3::Y);
        sound.other_clip_cues(7, &clip, 0.0, 1.0, false, true, Vec3::new(8.0, 4.0, 0.0), 0.0);
        let (left, right) = sides(&sound, 0.25);
        assert!(right > 2.0 * left && right > 0.0, "a cue on the right: left {left} right {right}");
        sound.playing().mixer.stop_all();
        sides(&sound, 0.5);
        sound.other_clip_cues(7, &clip, 0.0, 1.0, false, true, Vec3::new(8.0, 4.0, 0.0), 0.0);
        sound.follow(7, Vec3::new(-8.0, 4.0, 0.0), 0.0);
        let (left, right) = sides(&sound, 0.25);
        assert!(left > 2.0 * right && left > 0.0, "it followed its owner to the left: left {left} right {right}");
    }

    /// The never-ending enemy gun: an automatic gun's infinite fire loop is started once and
    /// kept at the muzzle through a burst, keyed off when the trigger comes up (`sounds_cut`)
    /// and then leaves the mix; an overheat cuts it and keeps the overheat sound until the gun
    /// cools; an enemy that is gone lets go of all of them. Before, every shot started a new
    /// copy of the loop and none was ever ended. [game 007a9b40, 007a9d40, 007a9ec0, 007a9fe0]
    #[test]
    fn an_enemys_gun_loop_is_kept_through_a_burst_and_ends_after_it() {
        // The Decepticon scout's gun (`WPN_SWIND_GUN_FIRE`), from its own pack and event file.
        let game = tf2_core::install::dir();
        let Some((_, player_tuning, data)) = offline() else { return };
        let scout = tf2_core::combat::Scout::load(game).unwrap();
        let (library, errors) = Library::load(&[game.join(r"characters\eng\dcomscout.fev")]);
        assert!(errors.is_empty(), "{errors:?}");
        let sound = SoundOut { library: Arc::new(library), playing: Arc::new(Mutex::new(Playing::new(&data, &player_tuning, None))), stopped: Default::default() };
        let (def, tuning) = (&scout.character.weapons[scout.gun_index], &scout.gun);
        assert_eq!(sound.library.name(def.fire_sound), Some("WPN_SWIND_GUN_FIRE"));
        assert!(tuning.automatic_firing && sound.is_infinite(def.fire_sound), "an automatic gun with a looping fire sound");
        sound.library.prepare(def.fire_sound);
        sound.library.prepare(def.overheat_sound);
        let muzzle = Vec3::new(10.0, 5.0, 1.0);
        let live = |id| sound.playing().mixer.handles_of(id).len();
        let keyed_off = |id| sound.playing().mixer.report().iter().any(|r| Some(r.0.as_str()) == sound.library.name(id) && r.4 > 0);
        let (mut kept, mut gun) = (crate::targets::EnemyGunSounds::default(), tf2_core::weapons::Weapon::default());
        for _ in 0..10 {
            kept.fire(&sound, def, tuning, 0.5, muzzle);
            kept.after_step(&sound, def, &gun, muzzle);
            sides(&sound, 0.05);
        }
        assert_eq!(live(def.fire_sound), 1, "one loop for the whole burst");
        gun.sounds_cut = true;
        kept.after_step(&sound, def, &gun, muzzle);
        assert!(keyed_off(def.fire_sound), "the trigger coming up keys the loop off");
        sides(&sound, 10.0);
        assert_eq!(live(def.fire_sound), 0, "and it leaves the mix");
        // An overheat in a burst.
        gun.sounds_cut = false;
        kept.fire(&sound, def, tuning, 0.5, muzzle);
        gun.overheated_left = 2.0;
        kept.after_step(&sound, def, &gun, muzzle);
        assert!(keyed_off(def.fire_sound), "the overheat cuts the fire loop");
        if def.overheat_sound != 0 {
            assert_eq!(live(def.overheat_sound), 1, "and keeps its overheat sound");
            kept.after_step(&sound, def, &gun, muzzle);
            assert_eq!(live(def.overheat_sound), 1, "once");
            gun.overheated_left = 0.0;
            kept.after_step(&sound, def, &gun, muzzle);
        }
        sides(&sound, 10.0);
        assert_eq!(live(def.fire_sound) + live(def.overheat_sound), 0, "all of it ends once the gun cools");
        // A gone enemy lets go mid-burst.
        kept.fire(&sound, def, tuning, 0.5, muzzle);
        kept.let_go(&sound);
        sides(&sound, 10.0);
        assert_eq!(live(def.fire_sound), 0, "a gone enemy's loop ends");
    }

    /// A held clip sound (`trackSound` on, as Grindor's `ANIM_BLAC_SRVO_LP` in his weapon
    /// object's Idle_WeaponA) is started once per table while it plays: a weapon object clip
    /// left before its stop key and started again reuses it instead of stacking a copy. Once
    /// stopped, the next cue starts it anew; an unheld cue still starts a copy each time.
    #[test]
    fn a_held_clip_sound_is_reused_while_it_plays() {
        use tf2_core::gun::SoundCommand;
        let Some((sound, _, data)) = offline() else { return };
        let id = data.sounds.engine;
        assert!(sound.library.has(id) && sound.library.infinite(id));
        let live = || {
            let playing = sound.playing();
            playing.mixer.handles_of(id).into_iter().filter(|&h| playing.mixer.is_playing(h)).count()
        };
        let held = SoundCommand::Cue { layer: 1, cue: SoundCue::Play { id, auto_stop: false, held: true } };
        for _ in 0..3 {
            sound.weapon_clip_command(SoundCommand::Changed { layer: 1 });
            sound.weapon_clip_command(held);
        }
        assert_eq!(live(), 1, "three passes of the clip, one loop");
        sound.weapon_clip_command(SoundCommand::Cue { layer: 1, cue: SoundCue::Stop { id } });
        assert_eq!(live(), 0, "its stop key ends it");
        sound.weapon_clip_command(held);
        assert_eq!(live(), 1, "and the next pass starts it again");
        // The robot's own table is another: the same id there starts its own copy.
        sound.weapon_clip_command(SoundCommand::Cue { layer: 0, cue: SoundCue::Play { id, auto_stop: false, held: true } });
        assert_eq!(live(), 2);
        sound.weapon_clip_command(SoundCommand::Cue { layer: 1, cue: SoundCue::Play { id, auto_stop: false, held: false } });
        assert_eq!(live(), 3, "an unheld cue is not looked up");
    }

    /// [assumed port design] The fall's wind (`ANIM_SHARED_WIND_LP`, held, autoStop 0, its Stop
    /// later in the same clip [data]) goes on into the next clip that cues it too, and ends when
    /// a clip that does not takes over before the Stop: a landing within the fall left it on.
    #[test]
    fn a_loop_left_before_its_stop_cue_ends_unless_the_next_clip_cues_it() {
        let Some((sound, _, _)) = offline() else { return };
        let id = tf2_core::sound::event_id("ANIM_SHARED_WIND_LP");
        assert!(sound.library.has(id) && sound.library.infinite(id));
        let event = |time, cue| tf2_core::formats::anim::SoundEvent { time, cue, rerun: false };
        let wind = SoundCue::Play { id, auto_stop: false, held: true };
        let in_air = Clip { length: 56.0, sound_events: vec![event(0.0, wind), event(56.0, SoundCue::Stop { id })], ..Default::default() };
        let fall = Clip { length: 54.0, sound_events: vec![event(0.0, wind), event(54.0, SoundCue::Stop { id })], ..Default::default() };
        let walk = Clip { length: 48.0, ..Default::default() };
        let live = || {
            let playing = sound.playing();
            playing.mixer.handles_of(id).into_iter().filter(|&h| playing.mixer.is_playing(h)).count()
        };
        // The leap: the in-air clip starts it, the fall takes it over at its apex.
        sound.clip_changed(Some(&in_air));
        sound.clip_cues(&in_air, 0.0, 10.0, false);
        assert_eq!(live(), 1);
        sound.clip_changed(Some(&fall));
        sound.clip_cues(&fall, 0.0, 10.0, false);
        assert_eq!(live(), 1, "the fall holds the same wind on, no second copy");
        // A landing at tick 30 of the fall, before its Stop at 54.
        sound.clip_changed(Some(&walk));
        assert_eq!(live(), 0, "the walk does not cue the wind: it ends with the fall");
        // A fall that runs to its Stop ends it there, as before.
        sound.clip_changed(Some(&fall));
        sound.clip_cues(&fall, 0.0, 55.0, false);
        assert_eq!(live(), 0);
        assert!(sound.playing().until_stop.is_empty());
    }

    /// A Stop cue ends its own character's copies only: the player's Stop leaves an enemy's
    /// copy of the same event, and the enemy's leaves the player's. A clip change ends the
    /// owner's autoStop sounds, and a character that is gone stops its loops. [assumed]
    #[test]
    fn stop_cues_and_clip_changes_are_scoped_to_their_owner() {
        let Some((sound, _, data)) = offline() else { return };
        // A loop, so that only a stop ends it.
        let id = data.sounds.engine;
        assert!(sound.library.has(id) && sound.library.infinite(id));
        let cue = |cue| Clip { length: 30.0, sound_events: vec![tf2_core::formats::anim::SoundEvent { time: 0.0, cue, rerun: false }], ..Default::default() };
        let (start, stop) = (cue(SoundCue::Play { id, auto_stop: true, held: false }), cue(SoundCue::Stop { id }));
        let at = Vec3::new(5.0, 0.0, 0.0);
        let playing = |h| sound.playing().mixer.is_playing(h);
        sound.clip_cues(&start, 0.0, 1.0, false);
        let mine = *sound.playing().with_clip.last().unwrap();
        sound.other_clip_cues(3, &start, 0.0, 1.0, false, true, at, 0.0);
        let theirs = sound.playing().others[&3].handles[0].0;
        sound.clip_cues(&stop, 0.0, 1.0, false);
        assert!(!playing(mine), "the player's Stop ends his copy");
        assert!(playing(theirs), "and not the enemy's");
        sound.clip_cues(&start, 0.0, 1.0, false);
        let mine = *sound.playing().with_clip.last().unwrap();
        sound.other_clip_cues(3, &stop, 0.0, 1.0, false, false, at, 0.0);
        assert!(!playing(theirs), "the enemy's Stop ends its copy");
        assert!(playing(mine), "and not the player's");
        // A new clip ends the owner's last autoStop sounds, not the player's.
        sound.other_clip_cues(3, &start, 0.0, 1.0, false, true, at, 0.0);
        let theirs = sound.playing().others[&3].handles.last().unwrap().0;
        sound.other_clip_cues(3, &Clip::default(), 0.0, 1.0, false, true, at, 0.0);
        assert!(!playing(theirs), "a clip change ends the enemy's autoStop sound");
        assert!(playing(mine), "and not the player's");
        // A character that is gone stops its loops.
        sound.other_clip_cues(4, &cue(SoundCue::Play { id, auto_stop: false, held: false }), 0.0, 1.0, false, true, at, 0.0);
        let kept = sound.playing().others[&4].handles[0].0;
        sound.other_gone(4);
        assert!(!playing(kept), "a gone character's loop stops");
        assert!(playing(mine) && !sound.others().contains(&4));
    }
}
