//! Sound: the game's event files and banks, and a mixer that plays events the way the data
//! describes them. No engine or audio device here; `Mixer::render` fills a buffer of floats.
//!
//! Tags as in `sim.rs`: `[data]` comes straight out of the event files, `[assumed]` is a
//! reading of the data that has not been checked against the game's player, `[stand-in]` is
//! ours. FMOD Ex's 3D (stereo speaker levels, roll-off, Doppler) is read from the executable
//! (`fmod_angle`, `stereo_levels`, `distance_gain`, `doppler_pitch`); `Mixer::send` is the
//! reverb send that `crate::reverb` (FMOD's SFX Reverb) runs on.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use glam::Vec3;

use crate::formats::fev::{Definition, Envelope, Event, Fev, Wave};
use crate::formats::fsb::{Bank, Sample};
use crate::formats::hash::crc32;
use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};

/// The id the game's data uses for an event: the CRC-32 of its name.
pub fn event_id(name: &str) -> u32 {
    crc32(name.as_bytes())
}

struct File {
    dir: PathBuf,
    fev: Fev,
    events: Vec<Arc<Event>>,
    /// The header words `Fev` does not keep, one per event (same order as `fev.events`).
    heads: Vec<EventHead>,
}

/// What FMOD's event loader (004bbc80, file version 0x380000) takes from an event's 33-word
/// header beyond what `formats::fev::Event` keeps. [game: 004bbc80, 004be690]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EventHead {
    /// Words 11 and 12: the 2D speaker levels, front left and front right (event +0x120,
    /// +0x124), handed to the channel's setSpeakerMix (00452990) when the event starts. They
    /// are the whole placement of a 2D event and the 2D share of a 3D one (0040b120).
    /// [assumed] (inside FMOD Ex) 004be690 skips that call when either of two flags the event reports
    /// (vtable +0x98 / +0x9c) is set; taken as never set. Read 2026-10-20: a simple event
    /// (FEV kind 16) reports both as 0 (its impl's slots 38 and 39 are 004cc090, which store 0),
    /// so the port is right for those. A complex event (kind 8, impl 004ca020 / 004ca040) reports
    /// the first child of its list at impl+0x18: its +0x30 and +0x3c being nonzero. Those child
    /// fields are written by a loader not yet read, so for complex events this stays open.
    /// The output is taken to be FMOD's stereo speaker mode (the PC's speaker setting decides it
    /// in the game, not read). settles: the writers of the child's +0x30 / +0x3c (after the
    /// complex event's list is built, from FEV kind 8 layers), and the speaker mode setting.
    pub speakers: (f32, f32),
    /// Word 23: the 3D Doppler level (event +0x10c, clamped 0..5 by the loader), handed to
    /// Channel::set3DDopplerLevel (00452c40 -> 00415ea0) for a 3D event.
    pub doppler: f32,
}

impl Default for EventHead {
    /// [game: 004bbc80] the loader's defaults for files too old to carry the words.
    fn default() -> Self {
        Self { speakers: (1.0, 1.0), doppler: 1.0 }
    }
}

/// Finds each event's header in the file's bytes (`u32 kind | u32 length | name\0 | 33
/// words`) and reads the words `EventHead` wants. An event not found keeps the defaults.
fn event_heads(data: &[u8], fev: &Fev) -> Vec<EventHead> {
    let word = |at: usize| data.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let mut from = 0;
    fev.events.iter().map(|event| {
        let needle: Vec<u8> = [&((event.name.len() + 1) as u32).to_le_bytes()[..], event.name.as_bytes(), &[0]].concat();
        // Events are in file order: search on from the last one, then from the start.
        let found = |start: usize| data.get(start..).and_then(|rest| rest.windows(needle.len())
            .position(|w| w == needle.as_slice())).map(|at| start + at)
            .filter(|&at| at >= 4 && matches!(word(at - 4), Some(8 | 16)));
        let Some(at) = found(from).or_else(|| found(0)) else { return EventHead::default() };
        let head = at + needle.len();
        from = head;
        let float = |i: usize| word(head + i * 4).map(f32::from_bits);
        match (float(11), float(12), float(23)) {
            (Some(left), Some(right), Some(doppler)) => EventHead { speakers: (left, right), doppler: doppler.clamp(0.0, 5.0) },
            _ => EventHead::default(),
        }
    }).collect()
}

type SampleKey = (usize, usize, usize);

/// Event files and their banks. Samples are decoded when first asked for and kept.
pub struct Library {
    files: Vec<File>,
    /// Event id to (file, event). The first file to define an event keeps it.
    index: HashMap<u32, (usize, usize)>,
    banks: Mutex<HashMap<PathBuf, Option<Arc<Bank>>>>,
    samples: Mutex<HashMap<SampleKey, Option<Arc<Sample>>>>,
}

impl Library {
    /// The event system's reverb presets by index (`getReverbPresetByIndex`, 004d5e90): every
    /// file's, in load order. [data] only `audio\eng\global.fev` has any (its 20). [assumed] the
    /// game's index counts across the loaded files in their order, so global.fev's are 0..19
    /// whatever is loaded before it (no other file of the install has presets, so only a
    /// mod's could tell). settles: 004d5e90's walk over the projects.
    pub fn reverb_presets(&self) -> Vec<crate::formats::fev::ReverbProperties> {
        self.files.iter().flat_map(|f| f.fev.reverbs.iter().map(|r| r.properties)).collect()
    }

    /// Every category path of the loaded event files (`FX/AMB`...), once each.
    pub fn category_paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self.files.iter().flat_map(|f| f.fev.categories.iter().map(|c| c.path.clone())).collect();
        paths.sort();
        paths.dedup();
        paths
    }

    /// Reads the given event files; their banks are looked for beside each. Returns what
    /// could not be read as text for the log.
    pub fn load(paths: &[PathBuf]) -> (Self, Vec<String>) {
        let mut problems = Vec::new();
        let mut files = Vec::new();
        let mut index = HashMap::new();
        for path in paths {
            let parsed = std::fs::read(path).map_err(|e| e.to_string())
                .and_then(|data| Fev::parse(&data).map(|fev| (event_heads(&data, &fev), fev)));
            match parsed {
                Ok((heads, fev)) => {
                    for (i, event) in fev.events.iter().enumerate() {
                        index.entry(event.id).or_insert((files.len(), i));
                    }
                    let events = fev.events.iter().cloned().map(Arc::new).collect();
                    files.push(File { dir: path.parent().unwrap_or(Path::new(".")).to_owned(), fev, events, heads });
                }
                Err(e) => problems.push(format!("{}: {e}", path.display())),
            }
        }
        (Self { files, index, banks: Mutex::default(), samples: Mutex::default() }, problems)
    }

    pub fn event_count(&self) -> usize {
        self.index.len()
    }

    /// Bind programmer sounds to FSB samples named by the chatter table. The original
    /// registry keys are CRCs of uppercase sample stems (verified against his eventIDs).
    /// The player's programmer event supplies category, mode, volume and fade metadata.
    pub fn add_programmer_bank(&mut self, path: &Path, template: u32, wanted: &[u32]) -> Result<usize, String> {
        let (template_file, template) = self.event(template).ok_or("programmer event template missing")?;
        let template_head = self.head(template.id);
        let bank = Bank::open(path)?;
        let name = path.file_stem().and_then(|s| s.to_str()).ok_or("bank has no name")?.to_owned();
        let mut fev = Fev { project: "chatter bindings".into(), banks: vec![name.clone()], events: Vec::new(),
            definitions: Vec::new(), categories: self.files[template_file].fev.categories.clone(), tail: 0, reverbs: Vec::new() };
        for (index, sample) in bank.entries.iter().enumerate() {
            let stem = sample.name.rsplit_once('.').map_or(sample.name.as_str(), |(stem, _)| stem);
            let id = event_id(&stem.to_ascii_uppercase());
            if !wanted.contains(&id) || self.index.contains_key(&id) { continue; }
            let mut event = (*template).clone();
            let mut sound = event.layers.first().and_then(|l| l.sounds.first()).cloned().ok_or("programmer event has no sound")?;
            sound.definition = fev.definitions.len();
            sound.one_shot = true;
            event.id = id;
            event.name = stem.to_owned();
            event.layers.truncate(1);
            event.layers[0].sounds = vec![sound];
            fev.definitions.push(Definition { name: stem.to_owned(), play_mode: 5,
                volume: 1.0, volume_floor: 1.0, pitch: 0.0,
                pitch_randomisation: 0.0, waves: vec![Wave { weight: 1, sample: Some((name.clone(), index)) }] });
            fev.events.push(event);
        }
        let file = self.files.len();
        let count = fev.events.len();
        for (index, event) in fev.events.iter().enumerate() { self.index.insert(event.id, (file, index)); }
        let events = fev.events.iter().cloned().map(Arc::new).collect();
        let heads = vec![template_head; fev.events.len()];
        self.files.push(File { dir: path.parent().unwrap_or(Path::new(".")).to_owned(), fev, events, heads });
        Ok(count)
    }

    /// Every event's name.
    pub fn names(&self) -> Vec<&str> {
        self.index.values().map(|&(file, event)| self.files[file].events[event].name.as_str()).collect()
    }

    pub fn has(&self, id: u32) -> bool {
        self.index.contains_key(&id)
    }

    /// Whether an event runs until stopped (`Event::infinite`): the game keeps an automatic
    /// weapon's fire-sound handle only then [game 007a9b40]. False for an unknown event.
    pub fn infinite(&self, id: u32) -> bool {
        self.event(id).is_some_and(|(_, event)| event.infinite())
    }

    pub fn name(&self, id: u32) -> Option<&str> {
        self.index.get(&id).map(|&(file, event)| self.files[file].events[event].name.as_str())
    }

    /// An event as the file has it, as text, for looking at.
    pub fn describe(&self, id: u32) -> Option<String> {
        self.event(id).map(|(_, event)| format!("{event:#?}"))
    }

    /// The header words of an event that `Event` does not keep (defaults for one not known).
    pub fn head(&self, id: u32) -> EventHead {
        self.index.get(&id).and_then(|&(file, event)| self.files[file].heads.get(event).copied()).unwrap_or_default()
    }

    fn event(&self, id: u32) -> Option<(usize, Arc<Event>)> {
        self.index.get(&id).map(|&(file, event)| (file, self.files[file].events[event].clone()))
    }

    fn bank(&self, file: usize, name: &str) -> Option<Arc<Bank>> {
        let path = self.files[file].dir.join(format!("{name}.fsb"));
        let mut banks = self.banks.lock().ok()?;
        banks.entry(path.clone()).or_insert_with(|| Bank::open(&path).ok().map(Arc::new)).clone()
    }

    /// One waveform of a sound definition, decoded on first use. `None` for silence, or for
    /// a sample that does not decode (which then stays silent).
    fn wave(&self, file: usize, definition: usize, wave: usize, decode: bool) -> Option<Arc<Sample>> {
        let key = (file, definition, wave);
        if let Some(known) = self.samples.lock().ok()?.get(&key) {
            return known.clone();
        }
        if !decode {
            return None;
        }
        let (bank, index) = self.files[file].fev.definitions.get(definition)?.waves.get(wave)?.sample.clone()?;
        // Decoded outside the lock: it takes a few milliseconds per second of sound.
        let sample = self.bank(file, &bank).and_then(|bank| bank.sample(index).ok()).map(Arc::new);
        self.samples.lock().ok()?.insert(key, sample.clone());
        sample
    }

    /// The length in seconds of an event's first sample, from its bank's header (nothing is decoded). [data] the
    /// header's frame count and rate. [assumed] the first waveform of the first sound stands for the event: the escort's
    /// lines are one sound and one wave each (`notes\mission-voice.md` section 2). None for an event with no sample, or
    /// a bank that does not open. Used only by the dormant mission runner (`mission_run.rs`). The game's own length is
    /// EventI::getLength (004c3a30, vtable 38 of EventI), which returns the event's +0xa8 field; that field is zeroed in
    /// the constructor at 004bd389 and its writer is not read yet. settles: that writer (the loader or the FEV kind 8 /
    /// kind 16 length computation), and a check of this value against it for the escort lines.
    pub fn length_s(&self, id: u32) -> Option<f32> {
        let (file, event) = self.event(id)?;
        let sound = event.layers.iter().flat_map(|layer| &layer.sounds).next()?;
        let (bank, index) = self.files[file].fev.definitions.get(sound.definition)?.waves.first()?.sample.clone()?;
        let bank = self.bank(file, &bank)?;
        let entry = bank.entries.get(index)?;
        (entry.rate > 0 && entry.frames > 0).then(|| entry.frames as f32 / entry.rate as f32)
    }

    /// Decodes everything an event can play, so that playing it later does not have to.
    /// Returns how many waveforms it has.
    pub fn prepare(&self, id: u32) -> usize {
        let Some((file, event)) = self.event(id) else { return 0 };
        let mut count = 0;
        for sound in event.layers.iter().flat_map(|layer| &layer.sounds) {
            let waves = self.files[file].fev.definitions[sound.definition].waves.len();
            for wave in 0..waves {
                count += self.wave(file, sound.definition, wave, true).is_some() as usize;
            }
        }
        count
    }
}

/// Names one playing event, for stopping it or setting its parameters.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Handle(u64);

impl Handle {
    /// Its number, unique within its mixer (`Command`'s).
    pub fn number(self) -> u64 {
        self.0
    }
}

/// One thing a mixer was told, kept while its `tap` is on, so another mixer can be told the same
/// (the game's `--net` link replays another copy's sounds). [port] Handles are the telling mixer's
/// numbers; places are in its listener's frame, as given. `stop_event` is kept as the `Stop` of each
/// instance it stops, and `set_parameter_hash` as the `Parameter` it resolves to.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Play { handle: u64, id: u32, volume: f32 },
    KeyOff(u64),
    Stop(u64),
    Spatial(u64, Vec3),
    StopAll,
    Parameter(u64, String, f32),
    Volume(u64, f32),
    CategoryVolume(String, f32),
    CategoryPitch(String, f32),
}

impl Command {
    /// Appends the commands, little-endian, to `out`.
    pub fn encode(commands: &[Command], out: &mut Vec<u8>) {
        let str = |out: &mut Vec<u8>, s: &str| {
            let b = &s.as_bytes()[..s.len().min(255)];
            out.push(b.len() as u8);
            out.extend_from_slice(b);
        };
        out.extend_from_slice(&(commands.len() as u32).to_le_bytes());
        for c in commands {
            match c {
                Command::Play { handle, id, volume } => {
                    out.push(0);
                    out.extend_from_slice(&handle.to_le_bytes());
                    out.extend_from_slice(&id.to_le_bytes());
                    out.extend_from_slice(&volume.to_le_bytes());
                }
                Command::KeyOff(h) | Command::Stop(h) => {
                    out.push(if matches!(c, Command::KeyOff(_)) { 1 } else { 2 });
                    out.extend_from_slice(&h.to_le_bytes());
                }
                Command::Spatial(h, v) => {
                    out.push(3);
                    out.extend_from_slice(&h.to_le_bytes());
                    for x in v.to_array() {
                        out.extend_from_slice(&x.to_le_bytes());
                    }
                }
                Command::StopAll => out.push(4),
                Command::Parameter(h, name, value) => {
                    out.push(5);
                    out.extend_from_slice(&h.to_le_bytes());
                    str(out, name);
                    out.extend_from_slice(&value.to_le_bytes());
                }
                Command::Volume(h, v) => {
                    out.push(6);
                    out.extend_from_slice(&h.to_le_bytes());
                    out.extend_from_slice(&v.to_le_bytes());
                }
                Command::CategoryVolume(name, v) | Command::CategoryPitch(name, v) => {
                    out.push(if matches!(c, Command::CategoryVolume(..)) { 7 } else { 8 });
                    str(out, name);
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
    }

    /// Reads what `encode` wrote from the start of `bytes`: the commands and the bytes after them,
    /// or None for a short or unknown record.
    pub fn decode(bytes: &[u8]) -> Option<(Vec<Command>, &[u8])> {
        struct Cursor<'a>(&'a [u8]);
        impl<'a> Cursor<'a> {
            fn take(&mut self, n: usize) -> Option<&'a [u8]> {
                let (head, rest) = (self.0.get(..n)?, self.0.get(n..)?);
                self.0 = rest;
                Some(head)
            }
            fn u8(&mut self) -> Option<u8> { Some(self.take(1)?[0]) }
            fn u32(&mut self) -> Option<u32> { Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?)) }
            fn u64(&mut self) -> Option<u64> { Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?)) }
            fn f32(&mut self) -> Option<f32> { Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?)) }
            fn str(&mut self) -> Option<String> {
                let n = self.u8()? as usize;
                String::from_utf8(self.take(n)?.to_vec()).ok()
            }
        }
        let mut c = Cursor(bytes);
        let count = c.u32()? as usize;
        let mut commands = Vec::with_capacity(count.min(4096));
        for _ in 0..count {
            let kind = c.u8()?;
            commands.push(match kind {
                0 => {
                    let handle = c.u64()?;
                    let id = c.u32()?;
                    Command::Play { handle, id, volume: c.f32()? }
                }
                1 => Command::KeyOff(c.u64()?),
                2 => Command::Stop(c.u64()?),
                3 => {
                    let h = c.u64()?;
                    Command::Spatial(h, Vec3::new(c.f32()?, c.f32()?, c.f32()?))
                }
                4 => Command::StopAll,
                5 => {
                    let h = c.u64()?;
                    let name = c.str()?;
                    Command::Parameter(h, name, c.f32()?)
                }
                6 => {
                    let h = c.u64()?;
                    Command::Volume(h, c.f32()?)
                }
                7 | 8 => {
                    let name = c.str()?;
                    let v = c.f32()?;
                    if kind == 7 { Command::CategoryVolume(name, v) } else { Command::CategoryPitch(name, v) }
                }
                _ => return None,
            });
        }
        Some((commands, c.0))
    }
}

struct Voice {
    sample: Arc<Sample>,
    /// Position in the sample, in its own frames.
    at: f64,
    /// Gain at the end of the last block; the next block ramps from it.
    gain: f32,
}

/// One sound of a playing event.
struct Part {
    layer: usize,
    sound: usize,
    voice: Option<Voice>,
    started: bool,
    /// Whether the layer's parameter was inside this sound's span at the last render.
    inside: bool,
    /// This play's share of the randomised volume and pitch.
    gain: f32,
    pitch: f32,
}

struct Instance {
    handle: Handle,
    file: usize,
    event: Arc<Event>,
    parameters: Vec<f32>,
    /// What each parameter was last set to; one with a seek speed is still on its way.
    wanted: Vec<f32>,
    parts: Vec<Part>,
    volume: f32,
    /// [game: 004bf110] Event-level pitch is shared by every sound in the event.
    pitch: f32,
    elapsed: f32,
    stopped_at: Option<(f32, f32)>,
    /// Each layer's two cascaded low-pass stages, left then right. [game: 00469370]
    filters: Vec<[f32; 4]>,
    /// 1 while playing; runs down to 0 once stopped.
    fade: f32,
    stopping: bool,
    fresh: bool,
    /// Where this event's source is from the listener (`Mixer::offset`'s frame); None uses
    /// the mixer's `offset`.
    spatial: Option<Vec3>,
    head: EventHead,
    /// How many sustain points key-offs have released (`EventParameter::keyOff`, which
    /// `sound_event_stop` calls on the primary parameter [game 00595c90]).
    released: usize,
    /// The trace's check of this instance's own output.
    health: Health,
    /// The last render's loudest sample of this instance's own output (master divided out),
    /// and its roll-off then (`Mixer::view`).
    peak: f32,
    rolloff: f32,
}

/// One instance as the sound debug overlay shows it (`Mixer::view`). Not the game's: a debug aid.
#[derive(Clone, Debug)]
pub struct InstanceView {
    pub handle: Handle,
    pub id: u32,
    pub name: String,
    pub category: String,
    /// Mode 0x10: placed in the world (3D); otherwise a 2D sound.
    pub placed: bool,
    /// Where the source is from the listener (x right, y front, z up), and whether the
    /// instance has its own place (`set_spatial`) rather than the mixer's `offset`.
    pub offset: Vec3,
    pub own_place: bool,
    pub elapsed: f32,
    pub stopping: bool,
    /// The event's fade, 1 while playing.
    pub fade: f32,
    /// A running parameter is waiting at a sustain point for a key-off.
    pub held: bool,
    pub key_offs: usize,
    /// Sounds with a voice now, of how many.
    pub sounding: usize,
    pub sounds: usize,
    /// The last render's peak of its own output, master divided out (1 is full scale).
    pub peak: f32,
    pub rolloff: f32,
    pub min_distance: f32,
    pub max_distance: f32,
}

/// Where a running parameter must stop: the next sustain point not yet released, in its own
/// units, or None. [data] sustain points are shares of the range. [assumed] (inside FMOD Ex) every parameter
/// with sustain points holds and is released by the same key-offs as the primary (in the
/// install, the primary is the one that has them). Read 2026-10-20: the game releases only
/// the primary (`sound_event_stop` 00595c90 calls EventParameter::keyOff on it); the key-off
/// code itself (EventParameterI vtable 00b4b4d8, slots 0..13; slot 13 is 004cec90) is platform
/// code that fn.py does not print, so the other parameters' behaviour stays open.
/// settles: the EventParameterI key-off and seek slots (004ce8a0, 004ce900, 004cea60..004cebf0)
/// read with raw_disasm.
/// [assumed] (inside FMOD Ex) only a rising parameter (velocity > 0) holds. Open: 004ce900 (set_value, read 2026-10-20)
/// and 004ce8a0 (get_info) hold no sign test: 004ce900 assigns the normalised value and, while the event
/// is active, sets a pending seek (+0x1c, flag +0x28 bit 0). 004cec50 (read 2026-10-20) has no sign test
/// either: it sets bit 1 at +0x24 of each child of the parameter's list (count at +0x2c of the descriptor
/// at +0x14). A scan of 004cea60..004cebf0 found no float compare against zero (only pointer and flag
/// `test`s). OPEN: settles: the velocity's use in the FEV loader's sustain setup, or the key-off of the
/// sustain points (004cec90 and the seek path), read with raw_disasm in full.
fn sustain_limit(p: &crate::formats::fev::Parameter, released: usize, value: f32) -> Option<f32> {
    if p.velocity <= 0.0 || p.max <= p.min {
        return None;
    }
    let range = p.max - p.min;
    let &s = p.sustain.get(released)?;
    let point = p.min + s * range;
    // Not yet past it (with room for rounding: held exactly at the point stays held).
    (value <= point + range * 1e-5).then_some(point)
}

/// The output limiter's ceiling (full scale is 1) and release time in seconds (`Mixer::limit`).
/// [assumed port design]
const LIMIT_CEILING: f32 = 0.95;
const LIMIT_RELEASE: f32 = 0.25;

pub struct Mixer {
    rate: f32,
    /// Frames rendered so far (the trace prints it as the output's time).
    rendered: u64,
    instances: Vec<Instance>,
    next: u64,
    random: u32,
    /// Where the sounds are from the listener, in metres, in the listener's frame: x across
    /// its look (right), y along it (front), z its up. The default for every event not
    /// given its own place by `set_spatial` (the character's own sounds).
    pub offset: Vec3,
    /// The listener's velocity in the same frame, metres a second. [game: 00771540 /
    /// 00772860] the game derives it from the PLAYER's position (the camera's only when there
    /// is no player), while the listener's position and axes are the camera's.
    pub listener_velocity: Vec3,
    /// FMOD's System 3D settings. [game: 007710f0] the game sets the Doppler scale to 0.3
    /// once the global banks are loaded (`00453590(0.3, rolloff)`), the distance factor is
    /// always 1 (00453590 passes 1.0) and the roll-off scale is 1 (00596430, 00929cb0;
    /// 00929d50, a zoom camera's update, sets it to its field of view / 60 while it runs).
    pub doppler_scale: f32,
    pub rolloff_scale: f32,
    /// The milliseconds FMOD's 3D update divides the listener's motion by (00412c20's
    /// argument: 1..999 ms is used as is, anything else as one second). [assumed] the game
    /// updates FMOD once a 0.032 s tick: who calls System::update with what is not read.
    /// Read 2026-10-20: 00412c20 has one caller, 00414060, which is called from 00412950, 00415a00,
    /// 00416c80 and 00427420 (none read). settles: the FMOD System update driver and its period.
    pub update_ms: u32,
    pub master: f32,
    category_volumes: HashMap<String, f32>,
    category_pitches: HashMap<String, f32>,
    /// One layer's sounds, before its effect.
    gather: Vec<f32>,
    choices: HashMap<(usize, usize, u32, usize, usize), WaveChoice>,
    /// With the trace on: what happened, at the output's time (`take_journal`).
    journal: Vec<(f32, String)>,
    /// The output before the current instance added to it (its own part is split into the
    /// dry output and the reverb send by this; the trace's health check reads it too).
    before: Vec<f32>,
    /// The last render's reverb send: every instance's output at its event's reverb wet level
    /// (`crate::reverb::ReverbSystem` takes it).
    send: Vec<f32>,
    /// While Some, every command is kept here as well (`Command`).
    pub tap: Option<Vec<Command>>,
    /// The output limiter's gain (`limit`), 1 when it is not acting.
    limit_gain: f32,
}

/// With the trace on, each instance's own output is checked over a quarter of a second:
/// a value that is not a number, a level far over full scale, or a mix that is mostly a
/// constant offset (what Megatron's tank engine did with its fine tune read wrongly).
/// Not the game's: a debug aid.
#[derive(Default)]
struct Health {
    frames: usize,
    sum: [f64; 2],
    squares: f64,
    peak: f32,
    bad_numbers: bool,
    /// The fault being reported, and when it began; None while healthy.
    fault: Option<(String, f32)>,
}

impl Health {
    const WINDOW: f32 = 0.25;
    /// Over this many times full scale (before the master volume) is a fault.
    const OVER: f32 = 4.0;

    /// Takes one buffer of the instance's own output (stereo, master divided out). Returns
    /// the fault found when a window closes, `Some("")` when a window closes healthy.
    fn add(&mut self, own: impl Iterator<Item = (f32, f32)>, rate: f32) -> Option<String> {
        for (left, right) in own {
            if !left.is_finite() || !right.is_finite() {
                self.bad_numbers = true;
                continue;
            }
            self.frames += 1;
            self.sum[0] += left as f64;
            self.sum[1] += right as f64;
            self.squares += (left * left + right * right) as f64 * 0.5;
            self.peak = self.peak.max(left.abs()).max(right.abs());
        }
        if (self.frames as f32) < Self::WINDOW * rate && !self.bad_numbers {
            return None;
        }
        let n = self.frames.max(1) as f64;
        let mean = (self.sum[0] / n).abs().max((self.sum[1] / n).abs()) as f32;
        let rms = (self.squares / n).sqrt() as f32;
        let fault = if self.bad_numbers {
            "not a number".to_owned()
        } else if self.peak > Self::OVER {
            format!("peak {:.1} x full scale", self.peak)
        } else if mean > 0.1 && mean > 0.7 * rms {
            format!("offset {mean:.2} of level {rms:.2}")
        } else {
            String::new()
        };
        *self = Health { fault: self.fault.take(), ..Health::default() };
        Some(fault)
    }
}

#[derive(Default)]
struct WaveChoice {
    last: Option<usize>,
    shuffle: Vec<usize>,
}

impl WaveChoice {
    /// [game: 0118aa60] Random-no-repeat advances to the next index if the
    /// weighted pick repeats, rather than drawing again or reweighting the list.
    fn pick(&mut self, definition: &Definition, random: &mut u32) -> usize {
        let count = definition.waves.len();
        if count == 0 { return 0; }
        let draw = |random: &mut u32| {
            *random ^= *random << 13;
            *random ^= *random >> 17;
            *random ^= *random << 5;
            *random
        };
        let chosen = match definition.play_mode {
            0 | 3 => self.last.map_or(0, |last| (last + 1) % count),
            1 | 2 => {
                let total: u64 = definition.waves.iter().map(|w| w.weight as u64).sum();
                if total == 0 { self.last = Some(0); return 0; }
                let mut pick = draw(random) as u64 % total;
                let mut chosen = 0;
                for (i, wave) in definition.waves.iter().enumerate() {
                    chosen = i;
                    if pick < wave.weight as u64 { break; }
                    pick = pick.saturating_sub(wave.weight as u64);
                }
                if definition.play_mode == 2 && self.last == Some(chosen) && count > 1 {
                    chosen = (chosen + 1) % count;
                }
                chosen
            }
            4 | 6 => {
                if self.shuffle.is_empty() {
                    self.shuffle.extend(0..count);
                    for end in (1..count).rev() {
                        let chosen = draw(random) as usize % (end + 1);
                        self.shuffle.swap(end, chosen);
                    }
                    // [game: 004deeb0] Avoid repeating the last entry across bags.
                    if count > 1 && self.shuffle.last().copied() == self.last {
                        let chosen = draw(random) as usize % (count - 1);
                        self.shuffle.swap(count - 1, chosen);
                    }
                }
                self.shuffle.pop().unwrap()
            }
            _ => 0,
        };
        self.last = Some(chosen);
        chosen
    }
}

/// The effect the engine's layer carries, driven by `load`.
const LOWPASS: &str = "FMOD Lowpass Simple";

/// Where a layer's low-pass effect cuts, in hertz, if it has one.
/// [game] 004cc7c0 maps this DSP's envelope logarithmically between the descriptor's
/// 10 and 22000 Hz (00469150, 00bf8878).
fn cutoff(layer: &crate::formats::fev::Layer, share: &impl Fn(usize) -> f32) -> Option<f32> {
    let envelope = layer.envelopes.iter().find(|e| e.effect == LOWPASS)?;
    Some(10.0 * 2200f32.powf(envelope.at(share(envelope.parameter)).clamp(0.0, 1.0)))
}

/// [game: 004c5230] Pitch envelope centre .5 preserves pitch; its ends are
/// four octaves down/up. Disabled envelopes (flag bit0) are not evaluated.
fn layer_pitch(layer: &crate::formats::fev::Layer, share: &impl Fn(usize) -> f32) -> f32 {
    layer.envelopes.iter().filter(|e| e.effect.is_empty() && e.flags == Envelope::PITCH)
        .map(|e| 2f32.powf(e.at(share(e.parameter)) * 8.0 - 4.0)).product()
}

/// [game: 004c5230/00415e20] The 0x404 envelope sets the channel's 3D pan level (ChannelI
/// +0x1b4, clamped 0..1). A channel starts at 1 (00413870 / 00413b80); the event's own
/// pan level word (31) is applied only to an FMOD_SOFTWARE (0x40) event (004be690), and no
/// robot event has that bit, so 1 unless an envelope says otherwise.
fn layer_pan_level(layer: &crate::formats::fev::Layer, share: &impl Fn(usize) -> f32) -> f32 {
    let level: f32 = layer.envelopes.iter()
        .filter(|e| e.effect.is_empty() && e.flags == Envelope::SPATIAL_BLEND)
        .map(|e| e.at(share(e.parameter))).product();
    level.clamp(0.0, 1.0)
}

/// FMOD Ex's direction of a point in whole degrees, 0 ahead, 90 to the right, 0..359.
/// [game: 0042e3a0 (speaker positions), 0040b120 (sources)] both coordinates are scaled by
/// 256 (double at 00b6aa68) and truncated (`_ftol`), then a fixed-point arctangent:
/// 0x324 = pi/4 x 1024, 0x96c = 3pi/4 x 1024, 0xc90 = pi x 1024, integer divisions.
pub fn fmod_angle(right: f32, front: f32) -> i32 {
    let (x, y) = ((right * 256.0) as i32, (front * 256.0) as i32);
    if x == 0 && y == 0 {
        return 0;
    }
    let (fy, fx) = (y.wrapping_shl(10), x.wrapping_shl(10));
    if fy >> 10 != y || fx >> 10 != x {
        return 0;
    }
    let across = fx.wrapping_abs() + 1;
    let mut angle = if fy < 0 {
        0x96c - (((across + fy) / ((across - fy) >> 10)) * 0x324 >> 10)
    } else {
        0x324 - (((fy - across) / ((across + fy) >> 10)) * 0x324 >> 10)
    };
    if fx < 0 {
        angle = -angle;
    }
    let degrees = angle * 0xb4 / 0xc90;
    if degrees < 0 { degrees + 360 } else if degrees > 359 { degrees - 360 } else { degrees }
}

/// FMOD Ex's 3D speaker levels on a stereo output for a source in the direction (`right`,
/// `front`) of the listener (a unit vector's components). [game: 0042c7d0] stereo's
/// speakers are at (-1, 0) and (1, 0): 270 and 90 degrees; [game: 0040b120] the source's
/// angle falls between two neighbouring speakers (sorted by angle, 0042b720) at a share t
/// of the way, and they get sqrt(1 - t) and sqrt(t): ahead or behind is 0.707 on each, hard
/// right is (0, 1). A source exactly at the listener gets sqrt(2) / 2 on each.
pub fn stereo_levels(right: f32, front: f32) -> (f32, f32) {
    if right == 0.0 && front == 0.0 {
        let each = std::f32::consts::SQRT_2 / 2.0;
        return (each, each);
    }
    let angle = fmod_angle(right, front);
    let (left_at, right_at) = (fmod_angle(-1.0, 0.0), fmod_angle(1.0, 0.0));
    // Pairs in angle order: (right speaker, left speaker), then (left, right + 360).
    if (right_at..left_at).contains(&angle) {
        let t = (angle - right_at) as f32 / (left_at - right_at) as f32;
        (t.sqrt(), (1.0 - t).sqrt())
    } else {
        let at = if angle < left_at { angle + 360 } else { angle };
        let t = (at - left_at) as f32 / (right_at + 360 - left_at) as f32;
        ((1.0 - t).sqrt(), t.sqrt())
    }
}

/// Debug aid: `TF2_SOUND_TRACE` set prints every event started and every waveform decoded while
/// it plays (one `Library::prepare` missed).
pub fn tracing() -> bool {
    static TRACE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TRACE.get_or_init(|| std::env::var_os("TF2_SOUND_TRACE").is_some())
}

/// FMOD Ex's distance attenuation of a 3D channel. [game: 00412c20] with no roll-off
/// callback: custom roll-off (0x4000000) follows the sound's own roll-off points, and with
/// none (the event system sets none: its distance shaping is the `(distance)` parameter's
/// envelopes) it is 1. Otherwise the distance is held between min and max; linear
/// (0x200000) is (max - d) / (max - min) (1 when max <= min); logarithmic (the rest,
/// 0x100000) is min / (min + scale (d - min)), with d at least 1e-6. Clamped to 0..1.
pub fn distance_gain(mode: u32, min: f32, max: f32, distance: f32, rolloff_scale: f32) -> f32 {
    let gain = if mode & 0x0400_0000 != 0 {
        1.0
    } else {
        let mut d = if distance >= max { max } else { distance };
        if d < min {
            d = min;
        }
        if mode & 0x0020_0000 != 0 {
            if max - min <= 0.0 { 1.0 } else { (max - d) / (max - min) }
        } else {
            if min < d && rolloff_scale != 1.0 {
                d = rolloff_scale * (d - min) + min;
            }
            min / d.max(1e-6)
        }
    };
    if gain < 0.0 { 0.0 } else if gain > 1.0 { 1.0 } else { gain }
}

/// FMOD Ex's Doppler pitch multiplier. [game: 00412c20] only with one listener and a scale
/// (system Doppler scale x the channel's Doppler level) above 0: the source and the listener
/// are each stepped back by their velocity over the update's seconds (`ms` 1..999, else 1),
/// and the change of distance over that time slows sound of 340 x distance factor (1) m/s:
/// (c - (d - d_before) / t x scale) / c, at least 1e-6. `offset` is the source from the
/// listener, `velocity` the listener's; the game gives every event source a zero velocity
/// (00595250, 00595480, 00595930 -> 00595620).
pub fn doppler_pitch(offset: Vec3, listener_velocity: Vec3, scale: f32, ms: u32) -> f32 {
    if scale <= 0.0 {
        return 1.0;
    }
    let t = if (1..1000).contains(&ms) { ms as f32 / 1000.0 } else { 1.0 };
    let c = 340.0;
    let before = (offset + listener_velocity * t).length();
    ((c - (offset.length() - before) / t * scale) / c).max(1e-6)
}

/// The listener's velocity as the game's audio manager makes it each update. [game:
/// 00772860, from 00771540] (position - last position) / dt; zero when dt <= 0, when the
/// position did not change, or when the velocity changed faster than 1000 m/s/s (a jump).
/// The position is the player character's when there is one (else the camera's).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ListenerMotion {
    last: Vec3,
    velocity: Vec3,
}

impl ListenerMotion {
    pub fn update(&mut self, position: Vec3, dt: f32) -> Vec3 {
        let mut velocity = Vec3::ZERO;
        if dt > 0.0 && position != self.last {
            let inverse = 1.0 / dt;
            velocity = (position - self.last) * inverse;
            if (velocity - self.velocity).length() * inverse > 1000.0 {
                velocity = Vec3::ZERO;
            }
        }
        self.last = position;
        self.velocity = velocity;
        velocity
    }
}

/// How one voice of a layer reaches the two speakers: out left = `ll` in left + `rl` in
/// right, out right = `lr` in left + `rr` in right. [game: 0040ddb0, 0040b120, 0040e690]
/// The channel's volume carries the distance gain blended by the pan level (gain x level +
/// 1 - level); its speaker levels are the 3D levels blended with the event's 2D speaker mix
/// by the same level. A 3D multichannel sample with no spread (every robot event's word 26
/// is 0) has all its channels at the one point, so both inputs feed both 3D levels; its 2D
/// share keeps left to left and right to right [assumed: the speaker-mix matrix 00478fd0 is
/// a jump table not read]. A 2D event is its speaker mix alone.
/// settles: the 2D share's left/right pairing. Read 2026-10-20: 00478fd0 is FMOD's (`platform:fmod`, not in the game's ledger) and dispatches on `ecx` through the jump table at 0047b2b8 (62 words: 0047b2aa, 00478ff7, 0047921e, 004796d6, 00479bf1, 0047a13f, 0047a754, 0047af05, 0047900a, ...); the handlers of entries 1..9 are not yet followed to their stores (`work\raw_disasm.py` over each target).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Matrix {
    ll: f32,
    rl: f32,
    lr: f32,
    rr: f32,
}

fn voice_matrix(placed: bool, level: f32, gain: f32, levels: (f32, f32), speakers: (f32, f32), stereo: bool) -> Matrix {
    if !placed {
        return if stereo { Matrix { ll: speakers.0, rl: 0.0, lr: 0.0, rr: speakers.1 } }
            else { Matrix { ll: speakers.0, rl: 0.0, lr: speakers.1, rr: 0.0 } };
    }
    let volume = gain * level + (1.0 - level);
    let (left, right) = (level * levels.0, level * levels.1);
    let (flat_left, flat_right) = ((1.0 - level) * speakers.0, (1.0 - level) * speakers.1);
    if stereo {
        Matrix { ll: volume * (left + flat_left), rl: volume * left, lr: volume * right, rr: volume * (right + flat_right) }
    } else {
        Matrix { ll: volume * (left + flat_left), rl: 0.0, lr: volume * (right + flat_right), rr: 0.0 }
    }
}

/// [game: 004692a0] FMOD Lowpass Simple's coefficient, including its upper-frequency
/// transition and explicit 22000 Hz bypass.
fn lowpass_coefficient(hertz: f32, rate: f32) -> f32 {
    if hertz >= 22000.0 {
        1.0
    } else if hertz > rate / std::f32::consts::PI {
        (hertz - rate / std::f32::consts::PI) / ((22000.0 - rate / std::f32::consts::PI) * 3.0) + 2.0 / 3.0
    } else {
        let period = 1.0 / rate;
        period / (1.0 / (hertz * std::f32::consts::TAU) + period)
    }
}

/// Adds a layer through the original two cascaded stages (00469370), then its stereo
/// placement. Bypass copies the signal without advancing filter history, as in the game.
/// [assumed] Omit FMOD's alternating denormal-prevention bias: it is not an audible
/// component. Read 2026-10-20: 00469370 swaps the stage's two bias words (+0x12c / +0x130) on
/// every call (0046937e..004693b0), and 004692a0 is the stage they feed. The constant is
/// written at 00469131 (misaligned in a linear read, not decoded yet). settles: that constant
/// (inaudible by construction only if it is below float's denormal range) and a comparison of
/// the stage's output with the game's.
fn settle(out: &mut [f32], gather: &mut [f32], filter: &mut [f32; 4], cutoff: Option<f32>, rate: f32, sides: (f32, f32)) {
    let pass = cutoff.map_or(1.0, |hertz| lowpass_coefficient(hertz, rate));
    for (to, from) in out.chunks_exact_mut(2).zip(gather.chunks_exact(2)) {
        for channel in 0..2 {
            let value = if pass == 1.0 {
                from[channel]
            } else {
                let first = channel * 2;
                filter[first] = (1.0 - pass) * filter[first] + pass * from[channel];
                filter[first + 1] = (1.0 - pass) * filter[first + 1] + pass * filter[first];
                filter[first + 1]
            };
            to[channel] += value * if channel == 0 { sides.0 } else { sides.1 };
        }
    }
}

/// [game: 004c5230] Fade widths are fractions of the sound's span, with fade-in
/// taking precedence if both fades overlap. Curve0 is the cubic Bezier default;
/// 1 is linear, 2 equal-power, and3..7 use the executable's exponential curves.
fn sound_span(sound: &crate::formats::fev::Sound, at: f32) -> f32 {
    let incoming = sound.length * sound.fade_in;
    let outgoing = sound.length * sound.fade_out;
    let curve = |progress: f32, fading_out: bool| {
        let t = progress.clamp(0.0, 1.0);
        match sound.fade_curve {
            1 => if fading_out { 1.0 - t } else { t },
            2 => (if fading_out { 1.0 - t } else { t }).sqrt(),
            3..=7 => {
                let base: f32 = match (sound.fade_curve, fading_out) {
                    (3, _) => 361.0,
                    (4, _) => 5.82843017578125,
                    (5, _) => 2.151270031929016,
                    (6, false) | (7, true) => 1.4514999985694885,
                    _ => 31.9960994720459,
                };
                1.0 - (base.powf(if fading_out { t } else { 1.0 - t }) - 1.0) / (base - 1.0)
            }
            _ => {
                let smooth = t * t * (3.0 - 2.0 * t);
                if fading_out { 1.0 - smooth } else { smooth }
            }
        }
    };
    if incoming > 0.0 && at < sound.start + incoming {
        curve((at - sound.start) / incoming, false)
    } else if outgoing > 0.0 && at > sound.start + sound.length - outgoing {
        curve((at - (sound.start + sound.length - outgoing)) / outgoing, true)
    } else {
        1.0
    }
}

/// [game: 004bf110/004c1330/004df040] Four-octave units, with a full symmetric
/// radius (2*random-1); the earlier adapter halved the authored random spread.
const PITCH_UNIT_OCTAVES: f32 = 4.0;

fn pitch_multiplier(pitch: f32, radius: f32, random: f32) -> f32 {
    2f32.powf((pitch + radius * (2.0 * random - 1.0)) * PITCH_UNIT_OCTAVES)
}

impl Mixer {
    /// `rate` is the output's frames per second.
    pub fn new(rate: u32) -> Self {
        Self { rate: rate as f32, rendered: 0, instances: Vec::new(), next: 1, random: 0x2545_f491, offset: Vec3::ZERO, listener_velocity: Vec3::ZERO, doppler_scale: 0.3, rolloff_scale: 1.0, update_ms: 32, master: 1.0, category_volumes: HashMap::new(), category_pitches: HashMap::new(), gather: Vec::new(), choices: HashMap::new(), journal: Vec::new(), before: Vec::new(), send: Vec::new(), tap: None, limit_gain: 1.0 }
    }

    /// The last `render`'s reverb send, stereo interleaved, as long as its output.
    pub fn send(&self) -> &[f32] {
        &self.send
    }

    fn tapped(&mut self, command: impl FnOnce() -> Command) {
        if let Some(tap) = self.tap.as_mut() {
            tap.push(command());
        }
    }

    /// The output's frames per second.
    pub fn rate(&self) -> f32 {
        self.rate
    }

    /// Seconds of output rendered so far.
    pub fn output_time(&self) -> f32 {
        self.rendered as f32 / self.rate
    }

    /// With the trace on, adds a line to the journal at the output's time (the game's own
    /// marks, such as a change of form, go in here too).
    pub fn note(&mut self, line: impl FnOnce() -> String) {
        if tracing() {
            let at = self.output_time();
            self.journal.push((at, line()));
        }
    }

    /// The journal since the last call: (output seconds, what happened).
    pub fn take_journal(&mut self) -> Vec<(f32, String)> {
        std::mem::take(&mut self.journal)
    }

    fn note_handle(&mut self, handle: Handle, what: &str) {
        if tracing() && let Some(instance) = self.instances.iter().find(|i| i.handle == handle) {
            let line = format!("{what}\t{}\t#{}\t", instance.event.name, handle.0);
            self.note(|| line);
        }
    }

    /// A category's pitch multiplier, applied to it and its children (the audio presets' pitch,
    /// `audio_mix::raw_pitch_multiplier` of their raw sum). 1 leaves it.
    pub fn set_category_pitch(&mut self, category: &str, multiplier: f32) {
        self.tapped(|| Command::CategoryPitch(category.to_owned(), multiplier));
        self.category_pitches.insert(category.to_owned(), multiplier.max(0.01));
    }

    /// Category options multiply the authored gain of the category and its parents.
    pub fn set_category_volume(&mut self, category: &str, volume: f32) {
        self.tapped(|| Command::CategoryVolume(category.to_owned(), volume));
        self.category_volumes.insert(category.to_owned(), volume.max(0.0));
    }

    /// Uniform in 0..1.
    fn random(&mut self) -> f32 {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        (self.random >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn playing(&self) -> usize {
        self.instances.len()
    }

    pub fn is_playing(&self, handle: Handle) -> bool {
        self.instances.iter().any(|i| i.handle == handle && !i.stopping)
    }

    /// Starts an event. Its samples should have been through `Library::prepare`; one that
    /// has not is decoded here, which is slow enough to be heard as a hitch.
    pub fn play(&mut self, library: &Library, id: u32, volume: f32) -> Option<Handle> {
        let (file, event) = library.event(id)?;
        let asked = volume;
        if tracing() {
            eprintln!("sound trace: {} ({id:#010x}) at output {:.2}s", event.name, self.rendered as f32 / self.rate);
        }
        // [game: 004bf110/004c1330] Random event gain/pitch are shared by its
        // layers. Definitions have their own independent random choices.
        let volume = volume * (1.0 - event.volume_randomisation.clamp(0.0, 1.0) * self.random());
        let pitch = pitch_multiplier(event.pitch, event.pitch_randomisation, self.random());
        // [data] an event allows only so many at once; the oldest makes room.
        if event.max_playbacks > 0 {
            let live = self.instances.iter().filter(|i| i.event.id == id && !i.stopping).count();
            if live >= event.max_playbacks as usize
                && let Some(oldest) = self.instances.iter_mut().find(|i| i.event.id == id && !i.stopping)
            {
                oldest.stopping = true;
                let oldest = oldest.handle;
                self.note_handle(oldest, "stolen");
            }
        }
        let mut parts = Vec::new();
        for (layer_index, layer) in event.layers.iter().enumerate() {
            for (sound_index, sound) in layer.sounds.iter().enumerate() {
                let definition = &library.files[file].fev.definitions[sound.definition];
                // [data] volume randomisation takes up to that share off; a definition
                // gives the quietest it may be.
                let floor = definition.volume_floor.clamp(0.0, 1.0);
                let gain = (floor + (1.0 - floor) * self.random()) * definition.volume;
                let pitch = pitch_multiplier(definition.pitch, definition.pitch_randomisation, self.random());
                parts.push(Part { layer: layer_index, sound: sound_index, voice: None, started: false, inside: false, gain, pitch });
            }
        }
        let handle = Handle(self.next);
        self.next += 1;
        let parameters: Vec<f32> = event.parameters.iter().map(|p| p.min).collect();
        let filters = vec![[0.0; 4]; event.layers.len()];
        let wanted = parameters.clone();
        let fade = if event.fade_in_ms > 0 { 0.0 } else { 1.0 };
        self.instances.push(Instance { handle, file, event, parameters, wanted, parts, volume, pitch,
            elapsed: 0.0, stopped_at: None, filters, fade, stopping: false, fresh: true, spatial: None, head: library.head(id),
            released: 0, health: Health::default(), peak: 0.0, rolloff: 1.0 });
        self.note_handle(handle, "start");
        self.tapped(|| Command::Play { handle: handle.0, id, volume: asked });
        Some(handle)
    }

    /// `EventParameter::keyOff` on the primary parameter, as the game's `sound_event_stop`
    /// does [game 00595c90]: the parameter runs on past its next sustain point (into a loop's
    /// tail). No fade. An event without a primary parameter is left as it is (the game calls
    /// nothing then). [assumed] the instance then ends when its sounds end (the game also
    /// marks its wrapper stopped, byte +7, whose effect is unread). settles: FMOD's stop and
    /// end path (004c3c90 / 004c3d90 are the game's stop calls; the instance end is not read).
    pub fn key_off(&mut self, handle: Handle) {
        if let Some(instance) = self.instances.iter_mut().find(|i| i.handle == handle)
            && instance.event.primary().is_some()
        {
            instance.released += 1;
            self.note_handle(handle, "key-off");
            self.tapped(|| Command::KeyOff(handle.0));
        }
    }

    /// An instance's parameter value by name (for tests and debug prints).
    pub fn parameter(&self, handle: Handle, name: &str) -> Option<f32> {
        let instance = self.instances.iter().find(|i| i.handle == handle)?;
        let index = instance.event.parameters.iter().position(|p| p.name == name)?;
        instance.parameters.get(index).copied()
    }

    /// Whether an instance (stopping or not) is still in the mix.
    pub fn is_alive(&self, handle: Handle) -> bool {
        self.instances.iter().any(|i| i.handle == handle)
    }

    /// How many sounds of an instance are sounding now, per (layer, sound) index.
    pub fn sounding(&self, handle: Handle) -> Vec<(usize, usize)> {
        self.instances.iter().find(|i| i.handle == handle)
            .map(|i| i.parts.iter().filter(|p| p.voice.is_some()).map(|p| (p.layer, p.sound)).collect())
            .unwrap_or_default()
    }

    /// Live (not stopping) copies of an event.
    pub fn live(&self, id: u32) -> usize {
        self.instances.iter().filter(|i| i.event.id == id && !i.stopping).count()
    }

    /// Every instance in the mix: its event's name, seconds since it started, whether it is
    /// stopping, how many of its sounds are audible (gain above 0), its key-offs and its parameters (for the trace's "still playing").
    pub fn report(&self) -> Vec<(String, f32, bool, usize, usize, Vec<f32>)> {
        self.instances.iter()
            .map(|i| (i.event.name.clone(), i.elapsed, i.stopping, i.parts.iter().filter(|p| p.voice.as_ref().is_some_and(|v| v.gain > 0.0)).count(), i.released, i.parameters.clone()))
            .collect()
    }

    /// Every instance in the mix as the sound debug overlay shows it, oldest first.
    pub fn view(&self) -> Vec<InstanceView> {
        self.instances.iter().map(|i| {
            let event = &i.event;
            let held = event.parameters.iter().zip(&i.parameters).any(|(p, &value)|
                sustain_limit(p, i.released, value).is_some_and(|point| value >= point - (p.max - p.min) * 1e-4));
            InstanceView {
                handle: i.handle, id: event.id, name: event.name.clone(), category: event.category.clone(),
                placed: event.mode & 0x10 != 0, offset: i.spatial.unwrap_or(self.offset), own_place: i.spatial.is_some(),
                elapsed: i.elapsed, stopping: i.stopping, fade: i.fade, held, key_offs: i.released,
                sounding: i.parts.iter().filter(|p| p.voice.is_some()).count(), sounds: i.parts.len(),
                peak: i.peak, rolloff: i.rolloff, min_distance: event.min_distance, max_distance: event.max_distance,
            }
        }).collect()
    }

    /// Lets an event fade out over its own fade time.
    pub fn stop(&mut self, handle: Handle) {
        if let Some(instance) = self.instances.iter_mut().find(|i| i.handle == handle) {
            if !instance.stopping {
                instance.stopping = true;
                self.note_handle(handle, "stop");
                self.tapped(|| Command::Stop(handle.0));
            }
        }
    }

    /// Gives one event its own place: the source from the listener, in `offset`'s frame (x
    /// right, y front, z up).
    pub fn set_spatial(&mut self, handle: Handle, offset: Vec3) {
        if let Some(instance) = self.instances.iter_mut().find(|i| i.handle == handle) {
            instance.spatial = Some(offset);
            self.tapped(|| Command::Spatial(handle.0, offset));
        }
    }

    /// The handle numbers of every instance not stopping, oldest first (what `--net` sends so the
    /// other copy can end what it missed the stop of).
    pub fn live_numbers(&self) -> Vec<u64> {
        self.instances.iter().filter(|i| !i.stopping).map(|i| i.handle.0).collect()
    }

    /// Every live copy of an event, oldest first.
    pub fn handles_of(&self, id: u32) -> Vec<Handle> {
        self.instances.iter().filter(|i| i.event.id == id).map(|i| i.handle).collect()
    }

    /// Stops every playing copy of an event.
    pub fn stop_event(&mut self, id: u32) {
        let handles: Vec<_> = self.instances.iter().filter(|i| i.event.id == id).map(|i| i.handle).collect();
        for handle in handles {
            self.stop(handle);
        }
    }

    pub fn stop_all(&mut self) {
        self.note(|| "stop all\t\t\t".to_owned());
        self.tapped(|| Command::StopAll);
        for instance in &mut self.instances {
            instance.stopping = true;
        }
    }

    pub fn set_parameter(&mut self, handle: Handle, name: &str, value: f32) {
        if self.is_alive(handle) { self.tapped(|| Command::Parameter(handle.0, name.to_owned(), value)); }
        let Some(instance) = self.instances.iter_mut().find(|i| i.handle == handle) else { return };
        if let Some(index) = instance.event.parameters.iter().position(|p| p.name == name) {
            let p = &instance.event.parameters[index];
            let value = value.clamp(p.min.min(p.max), p.max.max(p.min));
            instance.wanted[index] = value;
            // [data] a parameter with a seek speed glides to what it is set to (in
            // `render`); one without, or an event not yet heard, is there at once.
            if p.seek_speed <= 0.0 || instance.fresh {
                instance.parameters[index] = value;
            }
        }
    }

    /// Original callers can name parameters only by their case-sensitive CRC.
    pub fn set_parameter_hash(&mut self, handle: Handle, hash: u32, value: f32) {
        let name = self.instances.iter().find(|i| i.handle == handle)
            .and_then(|i| i.event.parameters.iter().find(|p| event_id(&p.name) == hash))
            .map(|p| p.name.clone());
        if let Some(name) = name { self.set_parameter(handle, &name, value); }
    }

    pub fn set_volume(&mut self, handle: Handle, volume: f32) {
        if let Some(instance) = self.instances.iter_mut().find(|i| i.handle == handle) {
            instance.volume = volume;
            self.tapped(|| Command::Volume(handle.0, volume));
        }
    }

    /// Adds the next stretch of sound to `out`: stereo, interleaved, `out.len() / 2` frames.
    /// `out` is overwritten.
    /// [assumed port design] The output's limiter, on the whole mix (stereo pairs) just before
    /// the device: where a frame would pass `LIMIT_CEILING` the gain drops at once to keep it
    /// there, and comes back to 1 over `LIMIT_RELEASE` seconds. Below the ceiling the gain is 1
    /// and nothing changes. FMOD's own mix stage is not read; what the port had was a clamp,
    /// heard as crackle when Devastator's pounds stack (peaks 1.8 times full scale, 2026-10-22).
    pub fn limit(&mut self, mixed: &mut [f32]) {
        let release = 1.0 - (-1.0 / (LIMIT_RELEASE * self.rate.max(1.0))).exp();
        for frame in mixed.chunks_exact_mut(2) {
            let peak = frame[0].abs().max(frame[1].abs());
            self.limit_gain += (1.0 - self.limit_gain) * release;
            if peak * self.limit_gain > LIMIT_CEILING {
                self.limit_gain = LIMIT_CEILING / peak;
            }
            frame[0] *= self.limit_gain;
            frame[1] *= self.limit_gain;
        }
    }

    pub fn render(&mut self, library: &Library, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        if frames == 0 {
            return;
        }
        self.rendered += frames as u64;
        let seconds = frames as f32 / self.rate;
        let (rate, master) = (self.rate, self.master);
        let (listener_velocity, doppler_scale, rolloff_scale, update_ms) =
            (self.listener_velocity, self.doppler_scale, self.rolloff_scale, self.update_ms);
        let mut gather = std::mem::take(&mut self.gather);
        gather.resize(out.len(), 0.0);
        let checking = tracing();
        let mut before = std::mem::take(&mut self.before);
        let mut send = std::mem::take(&mut self.send);
        send.clear();
        send.resize(out.len(), 0.0);
        let mut noted = Vec::new();
        let now = self.output_time();
        for instance in &mut self.instances {
            before.clear();
            before.extend_from_slice(out);
            let offset = instance.spatial.unwrap_or(self.offset);
            let distance = offset.length();
            // [game: 0040b120] the direction is the unit offset's right and front parts.
            let direction = offset.normalize_or_zero();
            let levels = stereo_levels(direction.x, direction.y);
            let event = instance.event.clone();
            let released = instance.released;
            for ((value, wanted), parameter) in instance.parameters.iter_mut().zip(instance.wanted.iter_mut()).zip(&event.parameters) {
                let wanted_now = *wanted;
                if parameter.name == "(distance)" {
                    *value = distance.clamp(parameter.min, parameter.max);
                } else if parameter.seek_speed > 0.0 && *value != wanted_now {
                    let wanted = wanted_now;
                    // [game: 004ce900/004cd990] FMOD seeks its normalized value by
                    // seek_speed * elapsed_ms /1000, hence range units here.
                    let step = parameter.seek_speed * (parameter.max - parameter.min).abs() * seconds;
                    *value += (wanted - *value).clamp(-step, step);
                } else if parameter.velocity != 0.0 && !instance.fresh {
                    // [data] a parameter with a velocity runs by itself, and stops at its next
                    // sustain point until a key-off (`key_off`) lets it run on.
                    let limit = sustain_limit(parameter, released, *value);
                    *value = (*value + parameter.velocity * seconds).clamp(parameter.min, parameter.max);
                    if let Some(point) = limit {
                        *value = value.min(point);
                    }
                    // The seek above glides only towards a value the code set: a value the
                    // velocity ran to is not undone by it (else a parameter with both, such as
                    // Ratchet's charge loop's primary `param03` or `WPN_OPTI_GUN_CHARGE`'s
                    // `param02`, was pulled back to its start every other update and never
                    // reached its sustain point or, after a key-off, its end). [assumed] FMOD Ex
                    // seeks only towards `setValue`'s target. settles: the EventParameterI seek
                    // slot (004ce900 is slot 4 of the vtable; read it, not yet done).
                    *wanted = *value;
                }
            }
            let share = |index: usize| {
                let p = &event.parameters[index];
                if p.max > p.min { (instance.parameters[index] - p.min) / (p.max - p.min) } else { 0.0 }
            };
            // [game: 004c0ca0] Event fades are linear elapsed/fade-time ramps.
            // [assumed] (inside FMOD Ex: the caller 004c2d30 -> 004d7110 is FMOD::EventSystemI, and the game's
            // update call into FMOD passes no elapsed time, batch Y6) Apply them on the device sample clock, rather than Lux's
            // EventSystem update clock. This preserves authored durations across
            // different callback sizes. 004c0ca0 takes the elapsed time as an argument, so its
            // clock is whatever its caller passes (not read). settles: that caller's clock
            // and period, and original DSP timestamps.
            if instance.stopping && instance.stopped_at.is_none() {
                instance.stopped_at = Some((instance.elapsed, instance.fade));
            }
            let stopped = instance.stopped_at;
            let elapsed = instance.elapsed;
            let fade_at = |time: f32| match stopped {
                Some((start, gain)) if event.fade_out_ms > 0 =>
                    gain * (1.0 - (time - start) * 1000.0 / event.fade_out_ms as f32).clamp(0.0, 1.0),
                Some(_) => 0.0,
                None if event.fade_in_ms > 0 => (time * 1000.0 / event.fade_in_ms as f32).clamp(0.0, 1.0),
                None => 1.0,
            };
            instance.elapsed += seconds;
            instance.fade = fade_at(instance.elapsed);
            // [data] the event's mode bits are FMOD's: 0x10 a sound placed in the world (0x08
            // one that is not: the interface's), and its roll-off (`distance_gain`).
            let placed = event.mode & 0x10 != 0;
            let rolloff = if placed {
                distance_gain(event.mode, event.min_distance, event.max_distance, distance, rolloff_scale)
            } else {
                1.0
            };
            // [game: 00412c20 / 00415ea0] Doppler: the system's scale x the event's level.
            let doppler = if placed {
                doppler_pitch(offset, listener_velocity, doppler_scale * instance.head.doppler, update_ms)
            } else {
                1.0
            };
            let speakers = instance.head.speakers;
            let parent_of = |parent: &str| parent.is_empty() || event.category == parent
                || event.category.strip_prefix(parent).is_some_and(|tail| tail.starts_with('/'));
            let authored: f32 = library.files[instance.file].fev.categories.iter()
                .filter(|c| parent_of(&c.path)).map(|c| c.volume).product();
            let options: f32 = self.category_volumes.iter().filter(|(c, _)| parent_of(c)).map(|(_, v)| *v).product();
            let category_pitch: f32 = self.category_pitches.iter().filter(|(c, _)| parent_of(c)).map(|(_, v)| *v).product();
            let level = event.volume * instance.volume * master * authored * options;
            // A layer's sounds are gathered apart, so that its effect works on them alone.
            let mut gathered: Option<usize> = None;
            for part in &mut instance.parts {
                if gathered != Some(part.layer) {
                    if let Some(done) = gathered {
                        let cutoff = cutoff(&event.layers[done], &share);
                        settle(out, &mut gather, &mut instance.filters[done], cutoff, rate, (1.0, 1.0));
                    }
                    gather.fill(0.0);
                    gathered = Some(part.layer);
                }
                let layer = &event.layers[part.layer];
                let sound = &layer.sounds[part.sound];
                let x = layer.parameter.filter(|&p| p < event.parameters.len()).map(share);
                // [data] a sound occupies a stretch of its layer's parameter.
                let at = x.unwrap_or(sound.start);
                let inside = at >= sound.start - 1e-4 && at <= sound.start + sound.length + 1e-4;
                part.inside = inside;
                let span = sound_span(sound, at);
                let shaped: f32 = layer
                    .envelopes
                    .iter()
                    .filter(|e| e.effect.is_empty() && e.flags == Envelope::VOLUME && e.parameter < event.parameters.len())
                    .map(|e| e.at(share(e.parameter)))
                    .product();
                let target = if inside { level * sound.volume * part.gain * span * shaped } else { 0.0 };
                if inside && !part.started && !instance.stopping {
                    part.started = true;
                    let definition = &library.files[instance.file].fev.definitions[sound.definition];
                    // [game: 0118aa60] No-repeat/shuffle mode6 share definition
                    // history. [assumed] (inside FMOD Ex: 004cc2c0 is reached from 004cc120 and 004d0d10, FMOD's
                    // instance pool and waveform start, batch Y6) The remaining modes reuse one history
                    // per authored sound in place of FMOD's EventSound pool. settles: the
                    // pool (004cc2c0 allocates an EventSound instance; 0118aa60 is the
                    // no-repeat pick, read in part).
                    let scope = if matches!(definition.play_mode, 2 | 6) { (0, 0, 0) }
                        else { (event.id, part.layer, part.sound) };
                    let key = (instance.file, sound.definition, scope.0, scope.1, scope.2);
                    let chosen = self.choices.entry(key).or_default().pick(definition, &mut self.random);
                    // A waveform not decoded by `Library::prepare` is decoded here, in the output's
                    // callback, which holds the sound back and can starve the device.
                    if tracing() && library.wave(instance.file, sound.definition, chosen, false).is_none() && definition.waves.get(chosen).is_some_and(|w| w.sample.is_some()) {
                        eprintln!("sound trace: {} decoded while playing", event.name);
                    }
                    part.voice = library.wave(instance.file, sound.definition, chosen, true).map(|sample| Voice {
                        sample,
                        at: 0.0,
                        gain: if sound.one_shot { target } else { 0.0 },
                    });
                }
                let Some(voice) = &mut part.voice else { continue };
                // [game: 004c5230] Blend normalized parameter/reference with the
                // authored minimum multiplier, then add the sound's fine tune.
                let mut pitch = part.pitch * instance.pitch * layer_pitch(layer, &share);
                if let (true, Some(x)) = (sound.auto_pitch, x) {
                    let reference = if sound.auto_pitch_reference == 0.0 { 1e-8 } else { sound.auto_pitch_reference };
                    pitch *= sound.auto_pitch_at_min + (1.0 - sound.auto_pitch_at_min) * x / reference;
                }
                // [game: 004c5230] Fine tune is HERTZ: the sample's default frequency
                // (`Sound::getDefaults`, vtable +0x48) times the multipliers, plus fine tune, goes
                // to the channel's setFrequency (00414bf0). It was added to the multiplier, so
                // Megatron's tank engine (`VEH_MEGA_TANK_RPM`, a sound tuned -10660 Hz) ran its
                // sample backwards off its start and the read extrapolated a growing offset that
                // pinned the output for a second as he changed form.
                // [assumed] The event's own pitch (`instance.pitch`) is among the multipliers
                // before fine tune; the category's pitch and the Doppler act on the channel after.
                // settles: what 004c5230's `param_2 + 0x64` (its other factor) holds.
                let rate_hz = voice.sample.rate as f32;
                pitch = (rate_hz * pitch + sound.fine_tune) / rate_hz.max(1.0);
                pitch *= category_pitch * doppler;
                // [assumed] Below zero hertz FMOD Ex plays the channel backwards (00414bf0
                // restarts it when the sign turns); the port holds it at zero instead: in the
                // data seen, a sound tuned that far down is outside its span (silent) there.
                // settles: 00413da0 and the system's frequency limits (`+0x48` / `+0x44`).
                pitch = pitch.max(0.0);
                let step = voice.sample.rate as f64 / rate as f64 * pitch as f64;
                let (channels, total) = (voice.sample.channels.max(1), voice.sample.frames());
                // [game: 0040b120 / 0040ddb0] placement and attenuation at the channel, before
                // the layer's effect (FMOD's channel levels feed its channel group's DSP).
                let matrix = voice_matrix(placed, layer_pan_level(layer, &share), rolloff, levels, speakers, channels > 1);
                let data = &voice.sample.data;
                let (from, ramp) = (voice.gain, (target - voice.gain) / frames as f32);
                let mut ended = total == 0;
                // [data] Keep every authored frame. The FSB loop endpoints, converted
                // to an exclusive end, define the period; no invented seam crossfade.
                let region = if sound.one_shot { 0..total } else { voice.sample.loop_range.clone() };
                let lap = region.len().max(1) as f64;
                if !ended && (from != 0.0 || target != 0.0) {
                    for frame in 0..frames {
                        if voice.at >= region.end as f64 {
                            if sound.one_shot {
                                ended = true;
                                break;
                            }
                            // Wrap before reading this frame; skipping it inserted a
                            // silent output frame at every audible loop boundary.
                            voice.at = region.start as f64 + (voice.at - region.end as f64) % lap;
                        }
                        let whole = voice.at as usize;
                        let t = (voice.at - whole as f64) as f32;
                        let gain = (from + ramp * frame as f32) * fade_at(elapsed + frame as f32 / rate);
                        let read = |frame: usize, channel: usize| {
                            let next = if frame + 1 < region.end { frame + 1 }
                                else if sound.one_shot { frame } else { region.start };
                            data[frame * channels + channel] * (1.0 - t) + data[next * channels + channel] * t
                        };
                        let left = read(whole, 0);
                        let right = if channels > 1 { read(whole, 1) } else { 0.0 };
                        gather[frame * 2] += (matrix.ll * left + matrix.rl * right) * gain;
                        gather[frame * 2 + 1] += (matrix.lr * left + matrix.rr * right) * gain;
                        voice.at += step;
                    }
                } else if !ended && !sound.one_shot {
                    // A silent loop keeps its place in step with the others.
                    voice.at += step * frames as f64;
                    if voice.at >= region.end as f64 {
                        voice.at = region.start as f64 + (voice.at - region.end as f64) % lap;
                    }
                } else if !ended {
                    // A silent one-shot (its parameter has left its span) runs on unheard and
                    // ends, so that it does not hold its event in the mix for ever (Long Haul's
                    // `WPN_FIRE_FLAME` start stayed after every pull). [assumed] FMOD Ex lets a
                    // started one-shot run out rather than freezing it; nothing audible changes.
                    // settles: FMOD's EventSound update for a sound outside its span.
                    voice.at += step * frames as f64;
                    ended = voice.at >= region.end as f64;
                }
                voice.gain = target;
                if ended {
                    part.voice = None;
                }
            }
            if let Some(done) = gathered {
                let cutoff = cutoff(&event.layers[done], &share);
                settle(out, &mut gather, &mut instance.filters[done], cutoff, rate, (1.0, 1.0));
            }
            instance.fresh = false;
            // The trace's health check of what this instance added (master divided out).
            if checking {
                let scale = if master > 0.0 { 1.0 / master } else { 0.0 };
                let own = out.chunks_exact(2).zip(before.chunks_exact(2))
                    .map(|(o, b)| ((o[0] - b[0]) * scale, (o[1] - b[1]) * scale));
                if let Some(found) = instance.health.add(own, rate) {
                    let name = &instance.event.name;
                    let handle = instance.handle.0;
                    match (&instance.health.fault, found.is_empty()) {
                        (None, false) => {
                            eprintln!("sound fault: {name} #{handle} {found} at output {now:.2}s");
                            noted.push((now, format!("fault\t{name}\t#{handle}\t{found}")));
                            instance.health.fault = Some((found, now));
                        }
                        (Some((_, since)), true) => {
                            eprintln!("sound fault over: {name} #{handle} after {:.2}s", now - since);
                            noted.push((now, format!("fault over\t{name}\t#{handle}\t")));
                            instance.health.fault = None;
                        }
                        _ => {}
                    }
                }
            }
            // [game: 004c7be0 / 004c7b70] the event's reverb wet and dry levels become its
            // channels' reverb Room and Direct (dB x 100, in millibels). [data] every event of
            // the install has both at 0 dB. [assumed] the send is taken after the event's
            // volume, fades, roll-off and panning, and both channels go to the reverb (whose
            // input sums them, 0047f320). settles: FMOD's channel-to-reverb connection (the
            // ChannelI reverb properties' use).
            let (wet, dry) = (crate::reverb::decibels(event.reverb_wet_db), crate::reverb::decibels(event.reverb_dry_db));
            let mut peak = 0.0f32;
            for ((o, b), s) in out.iter_mut().zip(&before).zip(send.iter_mut()) {
                let own = *o - *b;
                *s += own * wet;
                if dry != 1.0 {
                    *o = *b + own * dry;
                }
                peak = peak.max((own * dry).abs());
            }
            // The debug overlay's meter (`view`).
            instance.peak = if master > 0.0 { peak / master } else { 0.0 };
            instance.rolloff = rolloff;
        }
        self.gather = gather;
        self.before = before;
        self.send = send;
        self.journal.extend(noted);
        let alive_before: Vec<(Handle, String)> = if checking {
            self.instances.iter().map(|i| (i.handle, i.event.name.clone())).collect()
        } else {
            Vec::new()
        };
        self.instances.retain(|instance| {
            if instance.stopping && instance.fade <= 0.0 {
                return false;
            }
            // Over when nothing sounds and nothing more can start: a sound not yet started
            // only will if a parameter is still running towards it.
            let sounding = instance.parts.iter().any(|p| p.voice.is_some());
            let running = instance
                .event
                .parameters
                .iter()
                .zip(&instance.parameters)
                .any(|(p, &value)| p.velocity > 0.0 && value < p.max);
            let waiting = instance.parts.iter().any(|p| !p.started) && running && !instance.stopping;
            // A parameter held at a sustain point is `running` (below its max), so a held
            // event stays. Once keyed off and run out, a loop left behind its span plays no
            // more: the event is over when its one-shots (the tail) end. [assumed] FMOD ends
            // such an event then (its `lengthms` is -1 but nothing can sound again); applied
            // only after a key-off so that loops driven by code (the engine) are untouched.
            // settles: the event end path in FMOD (EventI's length field +0xa8 and the sound-end
            // check, not read).
            // A loop whose span runs to the very end of its parameter (Ratchet's and Long
            // Haul's charge loops: 0..1 on their primary) is still inside it once the
            // parameter has run out; its volume envelope has taken it to 0 there, and with
            // nothing running it can never sound again, so it counts as left behind too.
            // [assumed] FMOD ends such an event (silent, keyed off, run out); settle with its
            // docs. `voice.gain` is the last render's target, 0 exactly when silent. settles:
            // the same FMOD end path as the line above.
            let behind = |p: &Part| !instance.event.layers[p.layer].sounds[p.sound].one_shot
                && (!p.inside || p.voice.as_ref().is_some_and(|v| v.gain == 0.0));
            let finished = instance.released > 0 && !running
                && instance.parts.iter().all(|p| p.voice.is_none() || behind(p));
            (sounding || waiting) && !finished
        });
        if checking {
            for (handle, name) in alive_before {
                if !self.instances.iter().any(|i| i.handle == handle) {
                    self.journal.push((now, format!("end\t{name}\t#{}\t", handle.0)));
                }
            }
            // Clipping: the mix over full scale before the output's limiter (`limit`), which the dump (taken after it) cannot show.
            // Judged at volume 1 (the master divided out), so a quiet test run still shows it.
            let full = if master > 0.0 { master } else { 1.0 };
            let peak = out.iter().fold(0f32, |m, v| m.max(v.abs())) / full;
            let over = out.chunks_exact(2).filter(|f| f[0].abs().max(f[1].abs()) > full).count();
            if over > 0 {
                self.journal.push((now, format!("clip\t\t\t{over} frames over full scale at volume 1, peak {peak:.2}")));
            }
        }
        self.limit(out);
    }
}

/// One gear: the revolutions it runs between and its ratio. `<Name>Gears` in `bnxglobal.str`.
#[derive(Clone, Copy, Debug)]
pub struct Gear {
    pub min_rpm: f32,
    pub max_rpm: f32,
    pub ratio: f32,
}

/// Reads a gear table's rows. The game's characters find theirs by the hash in their attributes'
/// `modeDrive.gearList` (`tuning::load`), not by the name. [game: 007a2300 -> 007a4790]
pub fn read_gears(table: Node) -> Vec<Gear> {
    let number = |gear: Node, field: &str| gear.get(field).and_then(Node::float).unwrap_or(0.0);
    table
        .items()
        .map(|gear| Gear { min_rpm: number(gear, "minRPM"), max_rpm: number(gear, "maxRPM"), ratio: number(gear, "ratio") })
        .filter(|gear| gear.ratio > 0.0 && gear.max_rpm > 0.0)
        .collect()
}

/// Reads a gear table by its name (`BumblebeeGears`) out of `bnxglobal.str`.
pub fn load_gears(game_dir: &Path, character: &str) -> Result<Vec<Gear>, String> {
    let pack = Pack::open(&game_dir.join("bnxglobal.str"))?;
    let name = format!("{character}Gears");
    for chunk in pack.of_type(pack::DATA) {
        let Ok(file) = DataFile::parse(pack.data(chunk).to_vec()) else { continue };
        let Some(table) = file.root().get(&name) else { continue };
        return Ok(read_gears(table));
    }
    Err(format!("bnxglobal.str has no {name}"))
}

/// What the drive mode's wheel set shows the gearbox in one of the game's updates (the wheel
/// set's fields, `notes\driving.md`). [game]
#[derive(Clone, Copy, Debug, Default)]
pub struct GearboxInput {
    /// The body's velocity.
    pub velocity: [f32; 3],
    /// The filtered trigger, 0 to 1 (wheel set `+0x24`).
    pub trigger: f32,
    /// Stick left/right (`+0x1c`).
    pub stick_x: f32,
    /// The brake is slowing him (`+0x14` over 0).
    pub braking: bool,
    /// The slide is held (`+0x20`).
    pub skid: bool,
    pub reversing: bool,
    /// Braked to a stop and waiting to reverse (`+0x22`).
    pub stopped_by_brake: bool,
    /// Any wheel is on the ground.
    pub grounded: bool,
    /// A turbo is running: the throttle counts as full.
    pub turbo: bool,
}

/// The gearbox behind the engine event's `rpm` and `load` parameters, as the game runs it
/// once an update for a human player's car (`FUN_007a48e0`, called through `FUN_00731f80`
/// by the vehicle's sound update `FUN_007347c0`, which sends `+0x18` as `rpm` and `+0x30`
/// as `load`). [game]
///
/// The revolutions do not come from the speed: they are `trigger * revs * ratio * 600`, with
/// `revs` a number that climbs while he is not slowing down and falls while he is, and a
/// change of gear whenever that leaves the gear's range. The speed only says which of the
/// two it is.
#[derive(Clone, Debug)]
pub struct Gearbox {
    gears: Vec<Gear>,
    /// `engineTorque * 2 / (frontWheelRadius * mass)` and the same with 7 (`FUN_007a2300`).
    up_rate: f32,
    down_rate: f32,
    max_speed: f32,
    turning_velocity_delta: f32,
    pub gear: usize,
    pub rpm: f32,
    pub load: f32,
    /// `+0x58`.
    revs: f32,
    /// The revolutions when the wheels were last down (`+0x20`).
    grounded_rpm: f32,
    was_grounded: bool,
    was_reversing: bool,
    last_speed: f32,
    /// The stick, eased (`+0x54`).
    stick: f32,
    pub shifted_up: bool,
    pub shifted_down: bool,
}

impl Gearbox {
    /// 007a4fe0: rev progress within the current gear, used by suspension weight transfer. [game]
    pub fn progress(&self) -> f32 {
        let Some(current)=self.gears.get(self.gear) else {return 0.0;};
        if self.gear==0 {return self.rpm/current.max_rpm;}
        let below=self.gears[self.gear-1];
        let low=current.ratio*below.max_rpm/below.ratio;
        let span=current.max_rpm-low;
        if span>0.0 {(self.rpm-low)/span} else {0.0}
    }
    /// `mass` is the vehicle body's; the tuning values are the drive mode's.
    pub fn new(gears: Vec<Gear>, engine_torque: f32, front_wheel_radius: f32, mass: f32, max_speed: f32, turning_velocity_delta: f32) -> Self {
        let per = engine_torque / (front_wheel_radius * mass).max(1e-6);
        Self {
            gears,
            up_rate: per * 2.0,
            down_rate: per * 7.0,
            max_speed,
            turning_velocity_delta,
            gear: 0,
            rpm: 0.0,
            load: 0.5,
            revs: 0.0,
            grounded_rpm: 0.0,
            was_grounded: false,
            was_reversing: false,
            last_speed: 0.0,
            stick: 0.0,
            shifted_up: false,
            shifted_down: false,
        }
    }

    /// The same gearbox as the drive mode builds it on entry: first gear, nothing turning.
    pub fn fresh(self) -> Self {
        let (per, max_speed, delta) = (self.up_rate / 2.0, self.max_speed, self.turning_velocity_delta);
        Self { up_rate: per * 2.0, down_rate: per * 7.0, ..Self::new(self.gears, 1.0, 1.0, 1.0, max_speed, delta) }
    }

    /// One of the game's updates of `dt` seconds.
    pub fn update(&mut self, input: &GearboxInput, dt: f32) {
        if self.gears.is_empty() {
            return;
        }
        let [mut vx, mut vy, vz] = input.velocity;
        let throttle = if input.turbo { 1.0 } else { input.trigger };
        // At full throttle and not sliding, the speed it looks at is held under the top
        // speed less a share for the turn of the stick.
        if throttle > 0.9 && !input.skid {
            self.stick = if dt < 1.0 { self.stick + (input.stick_x - self.stick) * dt } else { input.stick_x };
            let limit = self.max_speed - self.stick.abs() * self.turning_velocity_delta;
            let flat = (vx * vx + vy * vy).sqrt();
            if limit < flat && flat > 1e-8 {
                (vx, vy) = (vx / flat * limit, vy / flat * limit);
            }
        }
        let speed = (vx * vx + vy * vy + vz * vz).sqrt();
        let slowing = speed - self.last_speed < -0.01;
        self.last_speed = speed;
        self.revs = self.revs.clamp(1.0, 1e9);
        let before = self.rpm;
        (self.shifted_up, self.shifted_down) = (false, false);
        let (grounded, was_grounded) = (input.grounded, std::mem::replace(&mut self.was_grounded, input.grounded));
        if grounded && !was_grounded {
            self.rpm = self.grounded_rpm;
        }
        let first = self.gears[0];
        if input.stopped_by_brake {
            (self.gear, self.rpm, self.load, self.revs) = (0, first.min_rpm, 0.5, 1.0);
            return;
        }
        let gear = self.gears[self.gear.min(self.gears.len() - 1)];
        let turning = |revs: f32| throttle * revs * gear.ratio * 600.0;
        if !input.turbo && input.braking && !input.skid && !input.reversing {
            self.revs = self.down_rate / self.revs.sqrt() * dt;
            self.rpm = turning(self.revs).max(first.min_rpm);
            self.load = 0.25;
            return;
        }
        if input.reversing {
            self.was_reversing = true;
            self.revs += self.up_rate / self.revs.sqrt() * dt;
            self.rpm = turning(self.revs).min(gear.max_rpm);
        } else if std::mem::take(&mut self.was_reversing) {
            (self.gear, self.rpm, self.revs) = (0, first.min_rpm, 1.0);
            return;
        }
        if (!slowing && !input.reversing) || !grounded {
            // In the air the engine runs to the top of its gear.
            self.rpm = if grounded {
                self.revs += self.up_rate / self.revs.sqrt() * dt;
                turning(self.revs)
            } else {
                gear.max_rpm
            };
            if self.rpm > gear.max_rpm {
                if !grounded || input.skid {
                    self.rpm = gear.max_rpm;
                } else {
                    self.shifted_up = true;
                    self.gear += 1;
                    if self.gear < self.gears.len() {
                        self.rpm = self.gears[self.gear].min_rpm;
                    } else {
                        // Out of gears: the revs fall back to where the gear before the
                        // last one ran out, and climb again.
                        self.gear = self.gears.len() - 1;
                        let below = self.gears[self.gears.len().saturating_sub(2)];
                        self.revs = below.max_rpm / (below.ratio * 600.0);
                        self.rpm = turning(self.revs);
                    }
                }
            }
        } else if !input.reversing {
            if !input.skid && self.gear > 0 {
                self.revs -= self.down_rate / self.revs.sqrt() * dt * 0.5;
            }
            self.rpm = turning(self.revs);
            if self.rpm < gear.min_rpm {
                self.shifted_down = true;
                self.rpm = match self.gear.checked_sub(1) {
                    Some(lower) => {
                        self.gear = lower;
                        self.gears[lower].max_rpm
                    }
                    None => first.min_rpm,
                };
            }
        }
        // The load is how fast the revolutions are changing, settling back to a half. A
        // division of nothing by nothing counts as 0, as the game's comparison has it.
        let moved = self.rpm / before - 1.0 + self.load;
        let load = if moved > 0.0 { moved.min(1.0) } else { 0.0 };
        self.load = (0.5 - load) * dt * 4.0 + load;
        if grounded {
            self.grounded_rpm = self.rpm;
        }
    }
}

/// What the vehicle's sound update (`FUN_007347c0`) reads of the car in one of the game's
/// updates, for the tyres and the suspension. [game]
#[derive(Clone, Copy, Debug, Default)]
pub struct TyreInput {
    /// The length of the body's velocity.
    pub speed: f32,
    /// The slide's button is held and the mode lets him slide (button 0x14, mode `+0x1f5`).
    pub drift: bool,
    /// The brake's amount, 0 to 1 (mode `+0x40`).
    pub brake: f32,
    pub reversing: bool,
    /// Braked to a stop and waiting to reverse (mode `+0x4e`).
    pub waiting_to_reverse: bool,
    /// Stick left / right.
    pub stick_x: f32,
    /// The speed the trigger allows right now (`FUN_0085f8b0`).
    pub cap: f32,
    /// Any wheel is on the ground.
    pub grounded: bool,
    /// The material of the ground under the wheel (wheel `+0x98`), 0 for none.
    pub surface: u32,
    /// The most a rear wheel's spring shortened in this update, as a share of
    /// `suspensionLength` (wheel `(+0x70 - +0x7c) / +0x80`).
    pub compression: f32,
    /// Independent original wheel contacts for effects/marks. [game]
    pub wheels: [TyreWheel; 4],
    pub per_wheel: bool,
    pub sound_wheel: usize,
    pub suspension_wheel: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TyreWheel {
    pub grounded: bool,
    pub was_grounded: bool,
    pub point: glam::Vec3,
    pub surface: u32,
    pub compression: f32,
}

impl TyreInput {
    pub fn for_wheel(&self, index: usize) -> Self {
        if !self.per_wheel {return *self;}
        let w=self.wheels[index];
        Self {grounded:w.grounded,surface:w.surface,compression:w.compression,..*self}
    }
}

/// What the tyres and the suspension should sound like after an update.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TyreSounds {
    /// The surface the skid loop is for (`vehicleTireSkidFX` picks the event), if it plays.
    pub skid: Option<u32>,
    /// The same for the rolling loop (`vehicleTireRoadFX`).
    pub road: Option<u32>,
    /// A thump of the suspension, with the force the code works out for it.
    pub suspension: Option<f32>,
}

/// The parameter the code gives the suspension's force to, by the hash of this name. The
/// event's own parameter is spelt `ImpactForce` and the game's lookup compares CRC-32s of
/// the names as they are (`FUN_00594d90`), so in the game the force reaches nothing and the
/// thump always plays with the parameter where it starts. [game] The port does the same.
pub const IMPACT_FORCE: &str = "impactForce";

/// The skid test of `FUN_007347c0` (`00734a53` to `00734b42`). [game]
pub fn skidding(input: &TyreInput) -> bool {
    (input.drift && input.speed > 1.0)
        || (input.brake > 0.5 && !input.reversing && !input.waiting_to_reverse)
        || (input.stick_x.abs() >= 0.9 && input.speed >= input.cap * 0.85)
}

/// The tyres' part of `FUN_007347c0`. [game]
///
/// The tyres skid while the slide is held above 1 m/s, or the brake is over half down (not
/// reversing or waiting to), or the stick is hard over (0.9) at 0.85 of the speed the
/// trigger allows or more. With a wheel on the ground and the body faster than 1 m/s the
/// rolling loop for the surface plays, and the skid loop with it while skidding; a surface
/// of 0 plays neither. Both loops are started with the body's speed as `speed` and follow
/// it (`FUN_0077d890`, `FUN_00595770`). The game plays each at ONE rear wheel, the one on
/// the side the car is moving to, and starts it again when that wheel's surface changes.
///
/// A rear spring that shortened by more than 0.05 of its length in the update plays
/// `vehicleSuspension` and asks for its `impactForce` to be that share x 10 (see
/// `IMPACT_FORCE`: the event has no parameter of that spelling).
pub fn tyre_sounds(input: &TyreInput) -> TyreSounds {
    let moving = input.speed > 1.0;
    let skidding = skidding(input);
    let on_ground = input.grounded && input.surface != 0 && moving;
    TyreSounds {
        skid: (on_ground && skidding).then_some(input.surface),
        road: on_ground.then_some(input.surface),
        suspension: (input.grounded && input.compression > 0.05).then_some(input.compression * 10.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The limiter leaves a mix under its ceiling alone, holds a loud one at the ceiling, and
    /// gives the gain back over its release.
    #[test]
    fn the_limiter_holds_loud_stacks_under_full_scale() {
        let mut mixer = Mixer::new(48_000);
        let mut quiet = vec![0.5, -0.4, 0.9, 0.1];
        mixer.limit(&mut quiet);
        assert_eq!(quiet, vec![0.5, -0.4, 0.9, 0.1]);
        let mut loud: Vec<f32> = (0..4800).flat_map(|i| { let v = 1.8 * (i as f32 * 0.05).sin(); [v, -v] }).collect();
        mixer.limit(&mut loud);
        assert!(loud.iter().all(|v| v.abs() <= LIMIT_CEILING + 1e-6));
        let mut after = vec![0.5f32; 2 * 48_000];
        mixer.limit(&mut after);
        assert!(after[0] < 0.5 * 0.7, "the gain comes back over the release, not at once: {}", after[0]);
        assert!((after[after.len() - 1] - 0.5).abs() < 0.01, "and is back after a second");
    }

    #[test]
    fn tapped_commands_survive_the_wire() {
        let commands = vec![
            Command::Play { handle: 7, id: 0xdead_beef, volume: 0.5 },
            Command::KeyOff(7),
            Command::Stop(7),
            Command::Spatial(9, Vec3::new(1.0, -2.0, 3.5)),
            Command::StopAll,
            Command::Parameter(9, "rpm".to_owned(), 0.25),
            Command::Volume(9, 0.8),
            Command::CategoryVolume("sfx".to_owned(), 0.7),
            Command::CategoryPitch("sfx/robot".to_owned(), 1.1),
        ];
        let mut out = Vec::new();
        Command::encode(&commands, &mut out);
        out.extend_from_slice(b"rest");
        let (back, rest) = Command::decode(&out).unwrap();
        assert_eq!(back, commands);
        assert_eq!(rest, b"rest");
        assert!(Command::decode(&out[..out.len() - 6]).is_none());
        // a mixer taps only once asked, and only what it did
        let mut mixer = Mixer::new(48_000);
        mixer.stop(Handle(3));
        mixer.set_category_volume("sfx", 0.5);
        mixer.tap = Some(Vec::new());
        mixer.stop(Handle(3));
        mixer.set_category_volume("sfx", 0.5);
        assert_eq!(mixer.tap.unwrap(), vec![Command::CategoryVolume("sfx".to_owned(), 0.5)]);
    }

    #[test]
    fn gearbox_replays_recorded_vehicle_updates() {
        if crate::install::missing() { return; }
        let dir=crate::install::dir_string();
        let gears=load_gears(Path::new(&dir),"Bumblebee").expect("installed gears");
        let mut errors=Vec::new();
        // Offline comparison against a new user capture; default regression stays fixed.
        let capture=std::env::var("TF2_GEARBOX_CAPTURE").ok()
            .map(|path| std::fs::read_to_string(path).expect("gearbox capture fixture"));
        let fixture=capture.as_deref().unwrap_or(include_str!("../fixtures/vehicle_20261007_gearbox.csv"));
        let (mut gear_mismatches,mut load_error,mut revs_error)=(0usize,0.0f32,0.0f32);
        for line in fixture.lines().filter(|line| !line.trim().is_empty()) {
            let r:Vec<f32>=line.split(',').map(|v|v.parse().unwrap()).collect();
            let mut g=Gearbox::new(gears.clone(),15000.0,1.6,5000.0,35.0,3.0);
            (g.gear,g.rpm,g.load,g.revs)=(r[2] as usize,r[3],r[4],r[5]);
            g.grounded_rpm=r[6];g.was_grounded=r[7]!=0.0;g.was_reversing=r[8]!=0.0;
            g.last_speed=r[9];g.stick=r[10];
            g.update(&GearboxInput { velocity:[r[11],r[12],r[13]],trigger:r[14],stick_x:r[15],
                braking:r[16]!=0.0,skid:r[17]!=0.0,reversing:r[18]!=0.0,
                stopped_by_brake:r[19]!=0.0,grounded:r[20]!=0.0,turbo:r[21]!=0.0 },r[1]);
            errors.push((g.rpm-r[23]).abs());
            gear_mismatches+=usize::from(g.gear!=r[22] as usize);
            load_error=load_error.max((g.load-r[24]).abs());
            revs_error=revs_error.max((g.revs-r[25]).abs());
        }
        errors.sort_by(|a,b|a.total_cmp(b));
        let p95=errors[errors.len()*95/100];
        eprintln!("{} recorded gearbox updates: rpm median{},p95{},max{}",errors.len(),errors[errors.len()/2],p95,errors.last().unwrap());
        eprintln!("gear mismatches{gear_mismatches}; max load error{load_error}; max revs error{revs_error}");
        assert_eq!(gear_mismatches,0,"recorded gear mismatch");
        assert!(load_error<0.0001,"recorded load mismatch");
        assert!(revs_error<0.0001,"recorded revs mismatch");
        assert!(*errors.last().unwrap()<0.02,"recorded RPM mismatch: {errors:?}");
    }

    #[test]
    fn the_tyres_skid_on_the_slide_the_brake_and_a_hard_turn_at_speed() {
        const PAVE: u32 = 0x37f7_18bf;
        let rolling = TyreInput { speed: 20.0, cap: 40.0, grounded: true, surface: PAVE, ..Default::default() };
        let heard = tyre_sounds(&rolling);
        assert_eq!((heard.skid, heard.road, heard.suspension), (None, Some(PAVE), None));
        assert_eq!(tyre_sounds(&TyreInput { drift: true, ..rolling }).skid, Some(PAVE));
        assert_eq!(tyre_sounds(&TyreInput { brake: 0.6, ..rolling }).skid, Some(PAVE));
        assert_eq!(tyre_sounds(&TyreInput { brake: 0.5, ..rolling }).skid, None, "the brake has to be over half");
        assert_eq!(tyre_sounds(&TyreInput { brake: 1.0, reversing: true, ..rolling }).skid, None);
        assert_eq!(tyre_sounds(&TyreInput { brake: 1.0, waiting_to_reverse: true, ..rolling }).skid, None);
        // The stick hard over only skids near the speed the trigger allows.
        assert_eq!(tyre_sounds(&TyreInput { stick_x: -1.0, ..rolling }).skid, None);
        assert_eq!(tyre_sounds(&TyreInput { stick_x: -1.0, speed: 34.0, ..rolling }).skid, Some(PAVE));
        assert_eq!(tyre_sounds(&TyreInput { stick_x: 0.8, speed: 40.0, ..rolling }).skid, None);
        // Slow, in the air or on no surface: nothing.
        let quiet = TyreSounds::default();
        assert_eq!(tyre_sounds(&TyreInput { speed: 1.0, drift: true, ..rolling }), quiet);
        assert_eq!(tyre_sounds(&TyreInput { grounded: false, drift: true, ..rolling }), quiet);
        assert_eq!(tyre_sounds(&TyreInput { surface: 0, drift: true, ..rolling }), quiet);
        // The suspension's thump: over a twentieth of the spring in one update, times ten.
        assert_eq!(tyre_sounds(&TyreInput { compression: 0.05, ..rolling }).suspension, None);
        assert_eq!(tyre_sounds(&TyreInput { compression: 0.2, ..rolling }).suspension, Some(2.0));
        assert_eq!(crc32(IMPACT_FORCE.as_bytes()), 0x3283_b7ff);
        assert_ne!(crc32(b"ImpactForce"), 0x3283_b7ff, "the event's own spelling is not the one the code asks for");
    }

    fn game_dir() -> PathBuf {
        PathBuf::from(crate::install::dir_string())
    }

    fn bumblebee() -> Library {
        let (library, problems) = Library::load(&[game_dir().join(r"characters\eng\bumblebee.fev")]);
        assert!(problems.is_empty(), "{problems:?}");
        library
    }

    fn loudness(buffer: &[f32]) -> f32 {
        (buffer.iter().map(|v| v * v).sum::<f32>() / buffer.len().max(1) as f32).sqrt()
    }

    #[test]
    fn bumblebees_events_are_found_by_the_ids_his_data_uses() {
        if crate::install::missing() { return; }
        let library = bumblebee();
        assert_eq!(library.event_count(), 38);
        // Ids as they appear in his animation events and sound table.
        assert_eq!(library.name(430740649), Some("ANIM_BUMB_TRANS_ROB_B"));
        assert_eq!(library.name(19899842), Some("VEH_BUMBLEBEE_CAMARO_RPM"));
        assert_eq!(library.name((-422524421i32) as u32), Some("VEH_BUMBLEBEE_CAMARO_REVS"));
    }

    #[test]
    fn installed_chatter_table_binds_decodable_radio_samples() {
        if crate::install::missing() { return; }
        let game = game_dir();
        let data = crate::character::load(&game, "bumblebee").unwrap();
        let (mut library, errors) = Library::load(&[game.join(r"audio\eng\global_streaming.fev")]);
        assert!(errors.is_empty(), "{errors:?}");
        let wanted: Vec<_> = data.chatter.iter().map(|l| l.event).collect();
        assert_eq!(library.add_programmer_bank(&game.join(r"audio\eng\CHATTER.fsb"), event_id("ProgrammerSoundPlayer"), &wanted).unwrap(), 109);
        let missing: Vec<_> = wanted.iter().filter(|&&id| !library.has(id)).collect();
        assert!(missing.is_empty(), "unbound chatter IDs: {missing:x?}");
        for sub in [(crate::chatter::ATTACK, crate::chatter::GENERIC, crate::chatter::ENEMY),
            (crate::chatter::INJURED, crate::chatter::CHARACTER, crate::chatter::SELF),
            (crate::chatter::DIED, crate::chatter::CHARACTER, crate::chatter::SELF)] {
            let line = data.chatter.iter().find(|l| (l.action, l.category, l.subcategory) == sub).unwrap();
            assert_eq!(library.prepare(line.event), 1);
            let mut mixer = Mixer::new(48000);
            mixer.play(&library, line.event, 1.0).unwrap();
            let mut out = vec![0.0; 960];
            let mut peak = 0.0f32;
            for _ in 0..300 {
                mixer.render(&library, &mut out);
                peak = peak.max(loudness(&out));
                if mixer.playing() == 0 { break; }
            }
            assert!(peak > 0.001, "silent radio line {}", library.name(line.event).unwrap());
            assert_eq!(mixer.playing(), 0, "radio line must finish");
        }
    }

    #[test]
    fn category_tree_gains_and_runtime_options_multiply_without_touching_other_buses() {
        if crate::install::missing() { return; }
        let mut library = bumblebee();
        let id = event_id("VEH_BUMBLEBEE_CAMARO_RPM");
        let (file, event) = library.event(id).unwrap();
        let category = event.category.clone();
        library.prepare(id);
        let render = |library: &Library, options: &[(&str, f32)]| {
            let mut mixer = Mixer::new(48000);
            for &(category, volume) in options { mixer.set_category_volume(category, volume); }
            let handle = mixer.play(library, id, 1.0).unwrap();
            mixer.set_parameter(handle, "rpm", 850.0);
            mixer.set_parameter(handle, "load", 1.0);
            let mut out = vec![0.0; 4800];
            mixer.render(library, &mut out);
            loudness(&out)
        };
        let baseline = render(&library, &[]);
        assert!(baseline > 0.001);
        assert!((render(&library, &[("", 0.5), (&category, 0.5)]) / baseline - 0.25).abs() < 1e-5);
        assert_eq!(render(&library, &[("UI", 0.0)]), baseline);
        let authored = library.files[file].fev.categories.iter_mut().find(|c| c.path == category).unwrap();
        authored.volume *= 0.5;
        assert!((render(&library, &[]) / baseline - 0.5).abs() < 1e-5);
    }

    #[test]
    fn player_engine_spatial_envelope_keeps_its_authored_2d_share() {
        if crate::install::missing() { return; }
        let library = bumblebee();
        let (_, event) = library.event(event_id("VEH_BUMBLEBEE_CAMARO_RPM")).unwrap();
        let pan = event.parameters.iter().position(|p| p.name == "2dPanLevel").unwrap();
        let layer = &event.layers[0];
        let share = |index| if index == pan { 1.0 } else { 0.0 };
        // The human player's engine: 2dPanLevel 1 gives a 3D pan level of 0.4.
        let level = layer_pan_level(layer, &share);
        assert!((level - 0.4).abs() < 1e-5, "{level}");
        // Hard right at a quarter of the volume: the channel volume is 0.25 x 0.4 + 0.6, the
        // levels 0.4 x (0, 1) + 0.6 x the event's speaker mix (1, 1).
        let m = voice_matrix(true, level, 0.25, (0.0, 1.0), (1.0, 1.0), false);
        assert!((m.ll - 0.7 * 0.6).abs() < 1e-5 && (m.lr - 0.7 * 1.0).abs() < 1e-5, "{m:?}");
        // A 2D event is its speaker mix, whatever the distance.
        assert_eq!(voice_matrix(false, level, 0.0, (0.0, 0.0), (1.0, 1.0), false), Matrix { ll: 1.0, rl: 0.0, lr: 1.0, rr: 0.0 });
    }

    #[test]
    fn fmod_angles_put_the_stereo_speakers_at_270_and_90() {
        // [game: 0042c7d0] stereo speakers at (-1, 0) and (1, 0).
        assert_eq!(fmod_angle(-1.0, 0.0), 270);
        assert_eq!(fmod_angle(1.0, 0.0), 90);
        assert_eq!(fmod_angle(0.0, 1.0), 0);
        assert_eq!(fmod_angle(0.0, -1.0), 180);
        // The fixed-point arctangent is coarse between the axes (45 degrees comes out 45).
        let diagonal = std::f32::consts::FRAC_1_SQRT_2;
        assert_eq!(fmod_angle(diagonal, diagonal), 45);
        assert_eq!(fmod_angle(-diagonal, -diagonal), 225);
        // Within a few degrees of the true angle everywhere.
        for degrees in 0..360 {
            let r = (degrees as f32).to_radians();
            let got = fmod_angle(r.sin(), r.cos());
            let error = ((got - degrees + 540) % 360 - 180).abs();
            assert!(error <= 5, "{degrees} -> {got}");
        }
        // Too small to register at 1/256: straight ahead.
        assert_eq!(fmod_angle(0.003, -0.003), 0);
    }

    #[test]
    fn stereo_levels_are_constant_power_between_the_speakers() {
        let each = std::f32::consts::FRAC_1_SQRT_2;
        let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4;
        assert!(near(stereo_levels(0.0, 1.0), (each, each)));
        assert!(near(stereo_levels(0.0, -1.0), (each, each)));
        assert!(near(stereo_levels(1.0, 0.0), (0.0, 1.0)));
        assert!(near(stereo_levels(-1.0, 0.0), (1.0, 0.0)));
        assert!(near(stereo_levels(0.0, 0.0), (each, each)));
        // 45 degrees right: a quarter of the way from left (270) round to right (450).
        let d = std::f32::consts::FRAC_1_SQRT_2;
        assert!(near(stereo_levels(d, d), (0.25f32.sqrt(), 0.75f32.sqrt())), "{:?}", stereo_levels(d, d));
        for degrees in 0..360 {
            let r = (degrees as f32).to_radians();
            let (l, rr) = stereo_levels(r.sin(), r.cos());
            assert!((l * l + rr * rr - 1.0).abs() < 1e-4, "{degrees}");
        }
    }

    #[test]
    fn distance_gain_follows_fmods_three_rolloffs() {
        const LOG: u32 = 0x18_0010;
        const LINEAR: u32 = 0x28_0010;
        const CUSTOM: u32 = 0x408_0010;
        assert_eq!(distance_gain(LOG, 40.0, 10000.0, 10.0, 1.0), 1.0);
        assert!((distance_gain(LOG, 40.0, 10000.0, 80.0, 1.0) - 0.5).abs() < 1e-6);
        // Held at max distance.
        assert!((distance_gain(LOG, 40.0, 100.0, 1000.0, 1.0) - 0.4).abs() < 1e-6);
        // A roll-off scale stretches the distance past min.
        assert!((distance_gain(LOG, 40.0, 10000.0, 80.0, 2.0) - 40.0 / 120.0).abs() < 1e-6);
        // Min 0 on a logarithmic event is silent, as in FMOD.
        assert_eq!(distance_gain(LOG, 0.0, 150.0, 5.0, 1.0), 0.0);
        assert!((distance_gain(LINEAR, 45.0, 10000.0, 5022.5, 1.0) - 0.5).abs() < 1e-6);
        assert_eq!(distance_gain(LINEAR, 45.0, 10000.0, 20.0, 1.0), 1.0);
        assert_eq!(distance_gain(LINEAR, 45.0, 45.0, 20.0, 1.0), 1.0);
        // Custom: the event's (distance) envelopes do the work.
        assert_eq!(distance_gain(CUSTOM, 0.0, 150.0, 500.0, 1.0), 1.0);
    }

    #[test]
    fn doppler_raises_the_pitch_of_what_the_listener_moves_towards() {
        let ahead = Vec3::new(0.0, 20.0, 0.0);
        // Towards it at 34 m/s with the game's scale 0.3: 1 + 34 x 0.3 / 340 = 1.03.
        let up = doppler_pitch(ahead, Vec3::new(0.0, 34.0, 0.0), 0.3, 32);
        assert!((up - 1.03).abs() < 1e-4, "{up}");
        let down = doppler_pitch(ahead, Vec3::new(0.0, -34.0, 0.0), 0.3, 32);
        assert!((down - 0.97).abs() < 1e-4, "{down}");
        // Across the line to it: almost nothing (second order in the step).
        assert!((doppler_pitch(ahead, Vec3::new(34.0, 0.0, 0.0), 0.3, 32) - 1.0).abs() < 1e-3);
        // A Doppler level of 0 (some events) or no update time worth using.
        assert_eq!(doppler_pitch(ahead, Vec3::new(0.0, 34.0, 0.0), 0.0, 32), 1.0);
        // Out of 1..999 ms the step is a whole second: across the line it then counts.
        let slow = doppler_pitch(ahead, Vec3::new(34.0, 0.0, 0.0), 0.3, 0);
        let expected = 1.0 + ((400.0f32 + 34.0 * 34.0).sqrt() - 20.0) * 0.3 / 340.0;
        assert!((slow - expected).abs() < 1e-5, "{slow} {expected}");
    }

    #[test]
    fn listener_motion_is_the_step_over_dt_and_drops_jumps() {
        let mut motion = ListenerMotion::default();
        // The first update from the origin to 0.32 m along x in 0.032 s: 10 m/s, a change of
        // 10 m/s in 0.032 s (312 m/s/s) is not a jump.
        let first = motion.update(Vec3::new(0.32, 0.0, 0.0), 0.032);
        assert!((first - Vec3::new(10.0, 0.0, 0.0)).length() < 1e-4, "{first}");
        assert_eq!(motion.update(Vec3::new(0.32, 0.0, 0.0), 0.032), Vec3::ZERO);
        // A teleport of 50 m: 1562 m/s in one step, far over 1000 m/s/s.
        assert_eq!(motion.update(Vec3::new(50.0, 0.0, 0.0), 0.032), Vec3::ZERO);
        assert_eq!(motion.update(Vec3::new(51.0, 0.0, 0.0), 0.0), Vec3::ZERO);
    }

    #[test]
    fn event_heads_carry_the_speaker_mix_and_doppler_level() {
        if crate::install::missing() { return; }
        let (library, problems) = Library::load(&[game_dir().join(r"characters\eng\optimus.fev")]);
        assert!(problems.is_empty(), "{problems:?}");
        // [data] his left and right gun shots are 2D events panned hard to one side.
        assert_eq!(library.head(event_id("WPN_OPTIMUS_LASER_FIRE_L")).speakers, (1.0, 0.0));
        assert_eq!(library.head(event_id("WPN_OPTIMUS_LASER_FIRE_R")).speakers, (0.0, 1.0));
        assert_eq!(library.head(event_id("VEH_OPTIMUS_MACK_BOOST")).speakers, (0.707, 0.707));
        let dopplers: Vec<f32> = library.names().iter().map(|n| library.head(event_id(n)).doppler).collect();
        assert!(dopplers.iter().all(|d| [0.0, 1.0, 5.0].contains(d)), "{dopplers:?}");
        assert!(dopplers.contains(&0.0) && dopplers.contains(&1.0));
        let library = bumblebee();
        let head = library.head(event_id("VEH_BUMBLEBEE_CAMARO_RPM"));
        assert_eq!((head.speakers, head.doppler), ((1.0, 1.0), 1.0));
    }

    #[test]
    fn a_hard_left_2d_shot_is_heard_only_on_the_left() {
        if crate::install::missing() { return; }
        let (library, problems) = Library::load(&[game_dir().join(r"characters\eng\optimus.fev")]);
        assert!(problems.is_empty(), "{problems:?}");
        let id = event_id("WPN_OPTIMUS_LASER_FIRE_L");
        assert!(library.prepare(id) > 0);
        let mut mixer = Mixer::new(48000);
        mixer.play(&library, id, 1.0).unwrap();
        let mut buffer = vec![0.0; 19200];
        mixer.render(&library, &mut buffer);
        let one = |channel: usize| loudness(&buffer.iter().skip(channel).step_by(2).copied().collect::<Vec<_>>());
        assert!(one(0) > 0.005 && one(1) < 1e-6, "{} {}", one(0), one(1));
        // Its reverb levels are 0 dB, as every event's: the send is the output itself.
        assert_eq!(mixer.send(), &buffer[..]);
    }

    #[test]
    fn the_debug_view_shows_each_instance_its_place_level_and_stop() {
        let (library, problems) = Library::load(&[game_dir().join(r"characters\eng\optimus.fev")]);
        assert!(problems.is_empty(), "{problems:?}");
        let id = event_id("WPN_OPTIMUS_LASER_FIRE_L");
        library.prepare(id);
        let mut mixer = Mixer::new(48000);
        mixer.master = 0.5;
        let handle = mixer.play(&library, id, 1.0).unwrap();
        mixer.set_spatial(handle, Vec3::new(3.0, 4.0, 0.0));
        let mut buffer = vec![0.0; 19200];
        mixer.render(&library, &mut buffer);
        let view = mixer.view();
        assert_eq!(view.len(), 1);
        let v = &view[0];
        assert_eq!((v.handle, v.id, v.name.as_str()), (handle, id, "WPN_OPTIMUS_LASER_FIRE_L"));
        assert!(v.own_place && (v.offset.length() - 5.0).abs() < 1e-5 && !v.stopping);
        // The meter is the instance's own output with the master divided out.
        let loudest = buffer.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(v.peak > 0.0 && (v.peak * mixer.master - loudest).abs() < 1e-5, "{} {loudest}", v.peak);
        assert!((v.elapsed - 0.2).abs() < 1e-4 && v.sounds > 0);
        mixer.stop(handle);
        assert!(mixer.view()[0].stopping);
    }

    #[test]
    fn authored_pitch_and_random_radius_preserve_four_octave_units() {
        if crate::install::missing() { return; }
        // Original 004bf110/004c1330: .25 is one octave; randomness is a
        // symmetric radius, not the earlier half-width approximation.
        assert_eq!(pitch_multiplier(0.25, 0.0, 0.5), 2.0);
        assert_eq!(pitch_multiplier(-0.25, 0.0, 0.5), 0.5);
        assert_eq!(pitch_multiplier(0.0, 0.125, 0.5), 1.0);
        assert!((pitch_multiplier(0.0, 0.125, 0.0) - 0.5f32.sqrt()).abs() < 1e-6);
        assert!((pitch_multiplier(0.0, 0.125, 1.0) - 2.0f32.sqrt()).abs() < 1e-6);
        let (library, errors) = Library::load(&[game_dir().join(r"audio\eng\global.fev")]);
        assert!(errors.is_empty(), "{errors:?}");
        let id = event_id("UI_REGEN_START");
        let (_, event) = library.event(id).unwrap();
        assert!((event.pitch - 0.245).abs() < 1e-6, "the installed regeneration cue carries pitch");
        let mut mixer = Mixer::new(48000);
        let handle = mixer.play(&library, id, 1.0).unwrap();
        let instance = mixer.instances.iter().find(|i| i.handle == handle).unwrap();
        assert!((instance.pitch - 2f32.powf(0.98)).abs() < 1e-6);
    }

    #[test]
    fn pitch_envelope_is_neutral_at_its_centre_and_ignores_disabled_points() {
        let envelope = Envelope { effect: String::new(), flags: Envelope::PITCH,
            points: vec![(0.0, 0.0), (1.0, 1.0)], parameter: 0 };
        let mut layer = crate::formats::fev::Layer { parameter: None,
            sounds: vec![], envelopes: vec![envelope] };
        assert_eq!(layer_pitch(&layer, &|_| 0.5), 1.0);
        assert_eq!(layer_pitch(&layer, &|_| 0.0), 1.0 / 16.0);
        assert_eq!(layer_pitch(&layer, &|_| 1.0), 16.0);
        layer.envelopes[0].flags |= 1;
        assert_eq!(layer_pitch(&layer, &|_| 0.0), 1.0);
    }

    #[test]
    fn sample_choice_modes_preserve_no_repeat_sequence_and_shuffle_rules() {
        let waves = (0..3).map(|i| Wave { weight: if i == 0 { 1 } else { 0 }, sample: None }).collect();
        let mut definition = Definition { name: "choice fixture".into(), play_mode: 2,
            volume: 1.0, volume_floor: 1.0, pitch: 0.0, pitch_randomisation: 0.0, waves };
        let mut random = 0x2545_f491;
        let mut choice = WaveChoice::default();
        let picks: Vec<_> = (0..6).map(|_| choice.pick(&definition, &mut random)).collect();
        // Original advances a repeated weighted pick by one, even if that next
        // waveform's weight is zero. Redrawing would incorrectly repeat index0.
        assert_eq!(picks, [0, 1, 0, 1, 0, 1]);
        definition.play_mode = 3;
        let mut choice = WaveChoice::default();
        assert_eq!((0..6).map(|_| choice.pick(&definition, &mut random)).collect::<Vec<_>>(), [0, 1, 2, 0, 1, 2]);
        definition.play_mode = 4;
        let mut choice = WaveChoice::default();
        let picks: Vec<_> = (0..90).map(|_| choice.pick(&definition, &mut random)).collect();
        for bag in picks.chunks(3) {
            let mut bag = bag.to_vec();
            bag.sort_unstable();
            assert_eq!(bag, [0, 1, 2]);
        }
        assert!(picks.windows(2).all(|w| w[0] != w[1]), "shuffle cycle boundaries must not repeat");
    }

    #[test]
    fn event_fades_keep_their_duration_across_device_callback_sizes() {
        if crate::install::missing() { return; }
        let mut library = bumblebee();
        let id = event_id("ANIM_BUMB_TRANS_ROB_B");
        let (file, index) = library.index[&id];
        let event = Arc::make_mut(&mut library.files[file].events[index]);
        event.fade_in_ms = 100;
        event.fade_out_ms = 200;
        library.prepare(id);
        let render = |block_frames: usize| {
            let mut mixer = Mixer::new(48000);
            mixer.master = 0.1;
            let handle = mixer.play(&library, id, 1.0).unwrap();
            let mut out = Vec::new();
            // Stop partway through fade-in: retain the current level without a pop.
            for (frames, stopping) in [(960, false), (12000, true)] {
                if stopping { mixer.stop(handle); }
                let mut left = frames;
                while left > 0 {
                    let count = left.min(block_frames);
                    let mut buffer = vec![0.0; count * 2];
                    mixer.render(&library, &mut buffer);
                    out.extend(buffer);
                    left -= count;
                }
            }
            assert_eq!(mixer.playing(), 0);
            out
        };
        let small = render(240);
        let large = render(2400);
        let max_error = small.iter().zip(&large).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(max_error < 2e-6, "callback-dependent fade: {max_error}");
        assert!(loudness(&small[..960 * 2]) > 0.00001);
        assert!(small[(960 + 9600) * 2..].iter().all(|v| v.abs() < 1e-7), "authored 200ms tail must finish");
    }

    #[test]
    fn a_one_shot_plays_its_sample_and_ends() {
        if crate::install::missing() { return; }
        let library = bumblebee();
        let id = event_id("ANIM_BUMB_TRANS_ROB_B");
        assert_eq!(library.prepare(id), 1);
        let mut mixer = Mixer::new(48000);
        mixer.play(&library, id, 1.0).unwrap();
        let mut heard = 0.0f32;
        let mut blocks = 0;
        let mut buffer = vec![0.0; 960];
        while mixer.playing() > 0 && blocks < 1000 {
            mixer.render(&library, &mut buffer);
            heard = heard.max(loudness(&buffer));
            blocks += 1;
        }
        // The sample is 0.91 s long; a block is 10 ms. Pitch randomisation moves it a little.
        assert!((80..=100).contains(&blocks), "played for {blocks} blocks");
        assert!(heard > 0.01, "peak block loudness {heard}");
    }

    #[test]
    fn the_engine_moves_between_its_three_samples_with_the_revolutions() {
        if crate::install::missing() { return; }
        let library = bumblebee();
        let id = event_id("VEH_BUMBLEBEE_CAMARO_RPM");
        assert_eq!(library.prepare(id), 3);
        let mut mixer = Mixer::new(48000);
        let handle = mixer.play(&library, id, 1.0).unwrap();
        mixer.set_parameter(handle, "load", 1.0);
        let mut buffer = vec![0.0; 9600];
        let mut sounding = |mixer: &mut Mixer, rpm: f32| {
            mixer.set_parameter(handle, "rpm", rpm);
            for _ in 0..3 {
                mixer.render(&library, &mut buffer);
            }
            let parts = &mixer.instances[0].parts;
            let gains: Vec<bool> = parts.iter().map(|p| p.voice.as_ref().is_some_and(|v| v.gain > 0.0)).collect();
            (gains, loudness(&buffer))
        };
        // Idle, medium and high in the order the event lists them.
        let (at_idle, loud) = sounding(&mut mixer, 850.0);
        assert_eq!(at_idle, [true, false, false]);
        assert!(loud > 0.005, "idle loudness {loud}");
        assert_eq!(sounding(&mut mixer, 3000.0).0, [false, true, false]);
        assert_eq!(sounding(&mut mixer, 4500.0).0, [false, true, true]);
        assert_eq!(sounding(&mut mixer, 8000.0).0, [false, false, true]);
        // It loops: still there after ten seconds, and gone soon after a stop.
        for _ in 0..50 {
            mixer.render(&library, &mut buffer);
        }
        assert_eq!(mixer.playing(), 1);
        mixer.stop(handle);
        for _ in 0..3 {
            mixer.render(&library, &mut buffer);
        }
        assert_eq!(mixer.playing(), 0);
    }

    /// The trace's per-sound check: an offset like the tank engine's, a level far over full
    /// scale and a non-number are faults; an ordinary loud tone is not.
    #[test]
    fn the_trace_health_check_names_offsets_overs_and_non_numbers() {
        let rate = 1000.0;
        let check = |signal: &dyn Fn(usize) -> f32| {
            let mut health = Health::default();
            health.add((0..300).map(|i| (signal(i), signal(i))), rate).unwrap()
        };
        assert_eq!(check(&|i| (i as f32 * 0.3).sin() * 0.9), "");
        assert!(check(&|i| -54.0 + (i as f32 * 0.3).sin()).starts_with("peak"));
        assert!(check(&|i| -0.6 + (i as f32 * 0.3).sin() * 0.1).starts_with("offset"));
        assert_eq!(check(&|i| if i == 7 { f32::NAN } else { 0.0 }), "not a number");
        // A window not yet full says nothing.
        assert!(Health::default().add((0..10).map(|_| (0.0, 0.0)), rate).is_none());
    }

    /// Fine tune is hertz added to the channel's frequency [game 004c5230]: Megatron's tank
    /// engine has a sound tuned -10660 Hz, which as a multiplier ran its sample backwards and
    /// pinned the output at full scale as he changed form.
    #[test]
    fn fine_tune_is_hertz_and_the_tank_engine_stays_centred() {
        if crate::install::missing() { return; }
        let (library, problems) = Library::load(&[game_dir().join(r"characters\eng\megatron.fev")]);
        assert!(problems.is_empty(), "{problems:?}");
        let id = event_id("VEH_MEGA_TANK_RPM");
        let (_, event) = library.event(id).unwrap();
        assert!(event.layers.iter().flat_map(|l| &l.sounds).any(|s| s.fine_tune == -10660.0));
        library.prepare(id);
        let mut mixer = Mixer::new(48000);
        let handle = mixer.play(&library, id, 1.0).unwrap();
        mixer.set_parameter(handle, "load", 1.0);
        let mut buffer = vec![0.0; 4800];
        for step in 0..=60 {
            mixer.set_parameter(handle, "rpm", step as f32 * 100.0);
            // Half a second: the engine's throb is low enough that 50 ms is not centred.
            let (mut sum, mut peak) = (0.0, 0f32);
            for _ in 0..10 {
                mixer.render(&library, &mut buffer);
                sum += buffer.iter().sum::<f32>();
                peak = buffer.iter().fold(peak, |m, v| m.max(v.abs()));
            }
            let mean = sum / (buffer.len() * 10) as f32;
            assert!(mean.abs() < 0.05 && peak < 1.5, "rpm {}: mean {mean} peak {peak}", step * 100);
        }
    }

    #[test]
    fn the_engines_layer_is_duller_at_no_load() {
        if crate::install::missing() { return; }
        let library = bumblebee();
        let id = event_id("VEH_BUMBLEBEE_CAMARO_RPM");
        let (_, event) = library.event(id).unwrap();
        let load = event.parameters.iter().position(|p| p.name == "load").unwrap();
        let at = |value: f32| cutoff(&event.layers[0], &|index| if index == load { value } else { 0.0 }).unwrap();
        // [data] the envelope: 0.894 at no load, 1 from half load up.
        assert!((at(0.0) - 10.0 * 2200f32.powf(0.893617)).abs() < 1.0);
        assert!(at(0.0) < 10000.0 && at(1.0) > 21999.0);
        // The filter: the fastest wobble there is goes through a 1 kHz cut much smaller, a
        // steady level whole, and an open filter changes nothing.
        let wobble: Vec<f32> = (0..2000).map(|i| if (i / 2) % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let through = |cutoff: Option<f32>, from: &[f32]| {
            let (mut out, mut gather, mut filter) = (vec![0.0; from.len()], from.to_vec(), [0.0; 4]);
            settle(&mut out, &mut gather, &mut filter, cutoff, 48000.0, (1.0, 1.0));
            out
        };
        assert!(loudness(&through(Some(1000.0), &wobble)) < 0.1);
        assert_eq!(through(None, &wobble), wobble);
        let steady = vec![1.0; 2000];
        assert!((through(Some(1000.0), &steady)[1999] - 1.0).abs() < 1e-3);
    }

    #[test]
    fn engine_crossfades_use_the_authored_overlap_not_the_whole_rpm_range() {
        if crate::install::missing() { return; }
        let library = bumblebee();
        let (_, event) = library.event(event_id("VEH_BUMBLEBEE_CAMARO_RPM")).unwrap();
        let sounds = &event.layers[0].sounds;
        assert_eq!(sounds.iter().map(|s| s.fade_curve).collect::<Vec<_>>(), [2, 0, 0]);
        // Original FEV: idle/medium overlap1438..1845, medium/high3847..5298.
        // Full medium at3000 was wrongly attenuated by the previous interpretation.
        assert_eq!(sound_span(&sounds[1], 3000.0 / 8800.0), 1.0);
        assert_eq!(sound_span(&sounds[0], 850.0 / 8800.0), 1.0);
        assert_eq!(sound_span(&sounds[2], 8000.0 / 8800.0), 1.0);
        for rpm in [4000.0, 4500.0, 5000.0] {
            let sum = sound_span(&sounds[1], rpm / 8800.0) + sound_span(&sounds[2], rpm / 8800.0);
            assert!((sum - 1.0).abs() < 2e-5, "overlap gain at{rpm}: {sum}");
        }
        // The idle's equal-power fade, checked at the middle of its authored overlap.
        let mid = sounds[0].start + sounds[0].length * (1.0 - sounds[0].fade_out * 0.5);
        assert!((sound_span(&sounds[0], mid) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
        library.prepare(event.id);
        let mut mixer = Mixer::new(48000);
        let handle = mixer.play(&library, event.id, 1.0).unwrap();
        mixer.set_parameter(handle, "rpm", 3000.0);
        mixer.set_parameter(handle, "load", 1.0);
        let mut buffer = vec![0.0; 960];
        mixer.render(&library, &mut buffer);
        let instance = &mixer.instances[0];
        let medium = &instance.parts[1];
        let expected = event.volume * 0.81 * medium.gain;
        assert!((medium.voice.as_ref().unwrap().gain - expected).abs() < 1e-6);
    }

    #[test]
    fn engine_lowpass_has_the_original_two_stage_impulse_and_upper_transition() {
        // Numerical reference from004692a0/00469370: at1000Hz/48000Hz alpha=.11574828.
        let alpha = lowpass_coefficient(1000.0, 48000.0);
        assert!((alpha - 0.11574828).abs() < 1e-7);
        let mut out = vec![0.0; 8];
        let mut gather = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let mut filter = [0.0; 4];
        settle(&mut out, &mut gather, &mut filter, Some(1000.0), 48000.0, (1.0, 1.0));
        assert!((out[0] - 0.013397664).abs() < 1e-7);
        assert!((out[2] - 0.023693822).abs() < 1e-7);
        assert_eq!([out[1], out[3], out[5], out[7]], [0.0; 4]);
        assert!((lowpass_coefficient(48000.0 / std::f32::consts::PI, 48000.0) - 2.0 / 3.0).abs() < 1e-6);
        assert!(lowpass_coefficient(20000.0, 48000.0) > 0.89);
        assert_eq!(lowpass_coefficient(22000.0, 48000.0), 1.0);
        let history = filter;
        settle(&mut out, &mut gather, &mut filter, Some(22000.0), 48000.0, (1.0, 1.0));
        assert_eq!(filter, history, "the original bypass leaves filter memory alone");
    }

    #[test]
    fn an_audible_engine_loop_wrap_does_not_drop_an_output_frame() {
        if crate::install::missing() { return; }
        let library = bumblebee();
        let id = event_id("VEH_BUMBLEBEE_CAMARO_RPM");
        library.prepare(id);
        let mut mixer = Mixer::new(48000);
        let handle = mixer.play(&library, id, 1.0).unwrap();
        mixer.set_parameter(handle, "rpm", 850.0); // Idle has constant1x pitch.
        mixer.set_parameter(handle, "load", 1.0);
        mixer.render(&library, &mut [0.0; 2]);
        let voice = mixer.instances[0].parts[0].voice.as_mut().unwrap();
        mixer.rate = voice.sample.rate as f32; // Exactly one source frame per output frame.
        let total = voice.sample.frames();
        voice.at = (total - 2) as f64;
        mixer.render(&library, &mut [0.0; 8]);
        assert_eq!(mixer.instances[0].parts[0].voice.as_ref().unwrap().at, 2.0);
    }

    #[test]
    fn a_placed_sound_leans_to_its_side_and_an_interface_sound_does_not() {
        if crate::install::missing() { return; }
        let library = bumblebee();
        let id = event_id("ANIM_BUMB_TRANS_ROB_B");
        library.prepare(id);
        let heard = |side: f32| {
            let mut mixer = Mixer::new(48000);
            mixer.offset = Vec3::new(side, 0.0, 0.0);
            mixer.play(&library, id, 1.0).unwrap();
            let mut buffer = vec![0.0; 19200];
            mixer.render(&library, &mut buffer);
            let one = |channel: usize| loudness(&buffer.iter().skip(channel).step_by(2).copied().collect::<Vec<_>>());
            (one(0), one(1))
        };
        let (left, right) = heard(1.0);
        assert!(left < 1e-4 && right > 0.01, "{left} {right}");
        let (left, right) = heard(-1.0);
        assert!(right < 1e-4 && left > 0.01, "{left} {right}");
        // The interface's sounds have no place: the same on both sides, at any distance.
        let (shared, problems) = Library::load(&[game_dir().join(r"levels\us_city_00\eng\mission.fev")]);
        assert!(problems.is_empty(), "{problems:?}");
        let id = event_id("UI_BOOST_READY");
        assert!(shared.prepare(id) > 0);
        let mut mixer = Mixer::new(48000);
        mixer.offset = Vec3::new(500.0, 0.0, 0.0);
        mixer.play(&shared, id, 1.0).unwrap();
        let mut buffer = vec![0.0; 19200];
        mixer.render(&shared, &mut buffer);
        let one = |channel: usize| loudness(&buffer.iter().skip(channel).step_by(2).copied().collect::<Vec<_>>());
        assert!(one(0) > 0.005 && (one(0) - one(1)).abs() < one(0) * 0.5, "{} {}", one(0), one(1));
    }

    #[test]
    fn a_change_of_gear_glides_at_the_events_seek_speed() {
        if crate::install::missing() { return; }
        let library = bumblebee();
        let id = event_id("VEH_BUMBLEBEE_CAMARO_RPM");
        library.prepare(id);
        let mut mixer = Mixer::new(48000);
        let handle = mixer.play(&library, id, 1.0).unwrap();
        // Before it is first heard a value is taken at once.
        mixer.set_parameter(handle, "rpm", 6000.0);
        let mut buffer = vec![0.0; 960];
        mixer.render(&library, &mut buffer);
        assert_eq!(mixer.instances[0].parameters[0], 6000.0);
        // [data] seek speed 1: the whole 8800 in a second, so 88 in a block of 10 ms, and
        // the drop to second gear's 3000 takes a third of a second.
        mixer.set_parameter(handle, "rpm", 3000.0);
        mixer.render(&library, &mut buffer);
        assert!((mixer.instances[0].parameters[0] - 5912.0).abs() < 0.5);
        for _ in 0..40 {
            mixer.render(&library, &mut buffer);
        }
        assert_eq!(mixer.instances[0].parameters[0], 3000.0);
    }

    #[test]
    fn an_event_makes_room_for_itself() {
        if crate::install::missing() { return; }
        let library = bumblebee();
        let id = event_id("VEH_BUMBLEBEE_CAMARO_REVS");
        let mut mixer = Mixer::new(48000);
        let first = mixer.play(&library, id, 1.0).unwrap();
        // This event allows one at a time.
        let second = mixer.play(&library, id, 1.0).unwrap();
        assert!(!mixer.is_playing(first));
        assert!(mixer.is_playing(second));
    }

    #[test]
    fn gears_carry_the_revolutions_up_through_the_speed_range() {
        if crate::install::missing() { return; }
        let gears = load_gears(&game_dir(), "Bumblebee").unwrap();
        assert_eq!(gears.len(), 6);
        assert_eq!((gears[0].min_rpm, gears[0].max_rpm, gears[5].max_rpm), (1000.0, 6000.0, 8800.0));
        assert!((gears[0].ratio - 2.97).abs() < 1e-4);
        // His numbers: engineTorque 15000, front wheels 1.6, a body of 5000, maxSpeed 36.
        let mut engine = Gearbox::new(gears, 15000.0, 1.6, 5000.0, 36.0, 3.0);
        assert_eq!((engine.up_rate, engine.down_rate), (3.75, 13.125));
        let dt = 0.032;
        let pulling = |speed: f32| GearboxInput { velocity: [0.0, speed, 0.0], trigger: 1.0, grounded: true, ..Default::default() };
        // Pulling away: first gear from 1782 (revs 1 x 2.97 x 600) up to its 6000, which
        // takes (2/3)(3.367^1.5 - 1) / 3.75 = 0.92 s, then second from its 3000.
        engine.update(&pulling(0.0), dt);
        assert!((engine.rpm - 2.97 * 600.0 * (1.0 + 3.75 * dt)).abs() < 1.0, "{}", engine.rpm);
        let mut updates = 1;
        while !engine.shifted_up {
            // The load sits over a half while the revolutions rise.
            assert!(engine.rpm <= 6000.0 && engine.gear == 0 && engine.load > 0.5);
            engine.update(&pulling(updates as f32), dt);
            updates += 1;
        }
        assert!((27..=31).contains(&updates), "first gear lasted {updates} updates");
        assert_eq!((engine.gear, engine.rpm), (1, 3000.0));
        // The change up halves the revolutions, and the load drops with them.
        assert!(engine.load < 0.3, "{}", engine.load);
        // Held flat out it runs through every gear and then saws in the last: back to
        // 8250 / 0.8 x 0.63 = 6497 each time it passes 8800.
        let mut lowest = f32::MAX;
        for _ in 0..2000 {
            engine.update(&pulling(36.0), dt);
            assert!(engine.rpm <= 8800.0);
            if engine.gear == 5 && engine.shifted_up {
                lowest = lowest.min(engine.rpm);
            }
        }
        assert_eq!(engine.gear, 5);
        assert!((lowest - 6496.9).abs() < 1.0, "{lowest}");
        // The trigger let go while he slows: down through the gears to first's minimum.
        let coasting = |speed: f32| GearboxInput { velocity: [0.0, speed, 0.0], grounded: true, ..Default::default() };
        for i in 0..10 {
            engine.update(&coasting(36.0 - i as f32), dt);
        }
        assert_eq!((engine.gear, engine.rpm), (0, 1000.0));
        // Braking holds first's minimum at a quarter load; stopped by the brake, a half.
        engine.update(&GearboxInput { braking: true, ..pulling(20.0) }, dt);
        assert_eq!((engine.rpm, engine.load), (1000.0, 0.25));
        engine.update(&GearboxInput { stopped_by_brake: true, ..pulling(0.0) }, dt);
        assert_eq!((engine.gear, engine.rpm, engine.load), (0, 1000.0, 0.5));
        // Off the ground the engine runs to the top of its gear.
        engine.update(&GearboxInput { grounded: false, ..pulling(10.0) }, dt);
        assert_eq!(engine.rpm, 6000.0);
        // Standing with the trigger up: nothing turns, and nothing is not a number.
        for _ in 0..5 {
            engine.update(&coasting(0.0), dt);
        }
        assert_eq!(engine.rpm, 0.0);
        assert!(engine.load.is_finite());
    }

    /// Renders `seconds` in 10 ms blocks; returns the loudest block.
    fn run(mixer: &mut Mixer, library: &Library, seconds: f32) -> f32 {
        let mut buffer = vec![0.0; 960];
        let mut heard = 0.0f32;
        for _ in 0..(seconds * 100.0).round() as usize {
            mixer.render(library, &mut buffer);
            heard = heard.max(loudness(&buffer));
        }
        heard
    }

    /// (started, sounding) of the instance's loops and of its one-shots placed after `from`.
    fn loop_and_tail(mixer: &Mixer, handle: Handle, from: f32) -> ((bool, bool), (bool, bool)) {
        let instance = mixer.instances.iter().find(|i| i.handle == handle).expect("alive");
        let mut loops = (false, false);
        let mut tail = (false, false);
        for p in &instance.parts {
            let sound = &instance.event.layers[p.layer].sounds[p.sound];
            let audible = p.voice.as_ref().is_some_and(|v| v.gain > 0.0);
            if !sound.one_shot {
                loops = (loops.0 || p.started, loops.1 || audible);
            } else if sound.start >= from {
                tail = (tail.0 || p.started, tail.1 || audible);
            }
        }
        (loops, tail)
    }

    /// A fire loop held at its sustain point, then keyed off: the tail plays after the key-off,
    /// never before. [data] `WPN_BLAC_GAT_FIRE_LP`: loop 0-0.415, spin-down 0.415-1 on param00
    /// (velocity 5), sustain at 0.40915.
    #[test]
    fn a_sustain_loop_holds_until_its_key_off_then_plays_its_tail() {
        if crate::install::missing() { return; }
        let (library, problems) = Library::load(&[game_dir().join(r"characters\eng\blackout.fev")]);
        assert!(problems.is_empty(), "{problems:?}");
        let id = event_id("WPN_BLAC_GAT_FIRE_LP");
        assert!(library.infinite(id));
        assert!(library.prepare(id) > 0);
        let (_, event) = library.event(id).unwrap();
        let p = &event.parameters[event.primary().unwrap()];
        let held = p.min + p.sustain[0] * (p.max - p.min);
        let mut mixer = Mixer::new(48000);
        let handle = mixer.play(&library, id, 1.0).unwrap();
        let heard = run(&mut mixer, &library, 1.0);
        assert!((mixer.parameter(handle, "param00").unwrap() - held).abs() < 1e-5, "held at the sustain point");
        assert!(heard > 0.001, "the loop sounds: {heard}");
        let ((loop_started, loop_sounding), (tail_started, _)) = loop_and_tail(&mixer, handle, 0.41);
        assert!(loop_started && loop_sounding, "the loop still sounds after a second");
        assert!(!tail_started, "no spin-down before the key-off");
        // Held longer: still nothing of the tail.
        run(&mut mixer, &library, 2.0);
        assert!(!loop_and_tail(&mixer, handle, 0.41).1.0);
        mixer.key_off(handle);
        run(&mut mixer, &library, 0.2);
        assert!((mixer.parameter(handle, "param00").unwrap() - p.max).abs() < 1e-5, "runs on to its max");
        let ((_, loop_sounding), (tail_started, _)) = loop_and_tail(&mixer, handle, 0.41);
        assert!(tail_started, "the spin-down starts after the key-off");
        assert!(!loop_sounding, "the loop is left behind");
        let mut seconds = 0.0;
        while mixer.is_alive(handle) && seconds < 10.0 {
            run(&mut mixer, &library, 0.1);
            seconds += 0.1;
        }
        assert!(!mixer.is_alive(handle), "the event ends after its tail");
    }

    #[test]
    fn the_flame_holds_its_loop_at_its_sustain_point() {
        if crate::install::missing() { return; }
        let (library, problems) = Library::load(&[game_dir().join(r"characters\eng\longhaul.fev")]);
        assert!(problems.is_empty(), "{problems:?}");
        let id = event_id("WPN_FIRE_FLAME");
        assert!(library.infinite(id));
        library.prepare(id);
        let (_, event) = library.event(id).unwrap();
        let primary = event.primary().unwrap();
        let p = &event.parameters[primary];
        assert!((p.sustain[0] - 0.35727).abs() < 1e-4, "{:?}", p.sustain);
        let held = p.min + p.sustain[0] * (p.max - p.min);
        let mut mixer = Mixer::new(48000);
        let handle = mixer.play(&library, id, 1.0).unwrap();
        run(&mut mixer, &library, 1.5);
        assert!((mixer.parameter(handle, &p.name).unwrap() - held).abs() < 1e-5);
        let ((_, loop_sounding), (tail_started, _)) = loop_and_tail(&mixer, handle, 0.6);
        assert!(loop_sounding, "the flame's loop is heard while held");
        assert!(!tail_started);
        mixer.key_off(handle);
        run(&mut mixer, &library, 0.3);
        assert!((mixer.parameter(handle, &p.name).unwrap() - p.max).abs() < 1e-5);
        assert!(loop_and_tail(&mixer, handle, 0.6).1.0, "the tail after the key-off");
    }

    /// [game 00595c90] a key-off needs a parameter: an event without one goes on until stopped,
    /// and a stop fades it over its fade-out (`ANIM_BUMB_CHARGE_LP`: 650 ms).
    #[test]
    fn a_key_off_leaves_an_event_without_parameters_and_a_stop_fades_it() {
        if crate::install::missing() { return; }
        let (library, problems) = Library::load(&[game_dir().join(r"audio\eng\global.fev")]);
        assert!(problems.is_empty(), "{problems:?}");
        let id = event_id("ANIM_BUMB_CHARGE_LP");
        assert!(library.infinite(id));
        let (_, event) = library.event(id).unwrap();
        assert!(event.parameters.is_empty());
        assert_eq!(event.fade_out_ms, 650);
        library.prepare(id);
        let mut mixer = Mixer::new(48000);
        let handle = mixer.play(&library, id, 1.0).unwrap();
        run(&mut mixer, &library, 0.5);
        mixer.key_off(handle);
        run(&mut mixer, &library, 2.0);
        assert!(mixer.is_playing(handle), "a key-off does nothing to it");
        mixer.stop(handle);
        run(&mut mixer, &library, 0.6);
        assert!(mixer.is_alive(handle), "still fading");
        run(&mut mixer, &library, 0.1);
        assert!(!mixer.is_alive(handle), "gone after 650 ms");
    }

    /// Ratchet's and Long Haul's charge loops, keyed off: their primary parameter (`param03` /
    /// `param02`) runs to its max, where the layer's volume envelope on it is 0, but the loop's
    /// layer sits on a parameter that never moves (`2dPanLevel`), so the loop stays inside its
    /// span. Silent and run out, the instance is over and leaves the mix. The gatling's held
    /// loop and an un-keyed loop stay (the other tests here). [data; end rule assumed]
    #[test]
    fn a_keyed_off_charge_loop_left_silent_inside_its_span_ends() {
        if crate::install::missing() { return; }
        let dir = game_dir();
        for (pack, name) in [("ratchet", "ANIM_RATC_GRE_CHARG_LP"), ("longhaul", "ANIM_LONG_GRE_CHRG_LP")] {
            let (library, problems) = Library::load(&[dir.join(format!(r"characters\eng\{pack}.fev"))]);
            assert!(problems.is_empty(), "{problems:?}");
            let id = event_id(name);
            assert!(library.infinite(id) && library.prepare(id) > 0, "{name}");
            let (_, event) = library.event(id).unwrap();
            let primary = event.primary().unwrap();
            let mut mixer = Mixer::new(48000);
            let handle = mixer.play(&library, id, 1.0).unwrap();
            assert!(run(&mut mixer, &library, 2.0) > 0.01, "{name}: heard while held");
            assert!(run(&mut mixer, &library, 1.0) > 0.01 && mixer.is_alive(handle), "{name}: held at its sustain point");
            mixer.key_off(handle);
            // The primary runs from its sustain point to its max at its velocity (1 / s and
            // 2 / s): over within a second, the loop silent from there.
            let p = &event.parameters[primary];
            let mut ended = None;
            for k in 0..150 {
                run(&mut mixer, &library, 0.01);
                if !mixer.is_alive(handle) {
                    ended = Some(k as f32 * 0.01);
                    break;
                }
                assert!(mixer.parameter(handle, &p.name).unwrap() <= p.max);
            }
            let ended = ended.unwrap_or_else(|| panic!("{name}: still in the mix 1.5 s after its key-off"));
            assert!(ended < 1.0, "{name}: ended {ended} s after its key-off");
        }
    }

    /// One-shots are untouched: the gun's shot has no sustain and ends by itself.
    #[test]
    fn a_one_shot_fire_sound_is_not_infinite_and_ends_alone() {
        if crate::install::missing() { return; }
        let library = bumblebee();
        let id = event_id("WPN_BUMB_GUN_FIRE");
        assert_eq!(event_id("heat"), 0xba44_e6f8, "the fire sound's parameter [game 007a9b40]");
        assert!(library.has(id));
        assert!(!library.infinite(id));
        library.prepare(id);
        let mut mixer = Mixer::new(48000);
        let handle = mixer.play(&library, id, 1.0).unwrap();
        run(&mut mixer, &library, 5.0);
        assert!(!mixer.is_alive(handle));
    }
}
