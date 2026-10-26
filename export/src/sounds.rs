//! The character's sounds: every event of its own sound event file (`characters\eng\<x>.fev`)
//! and every event of the shared files (`audio\eng\global.fev`, `global_streaming.fev`) that the
//! character's JSON names, with the samples they play written as WAV under `sounds/<bank>/`.
//!
//! The banks hold MPEG audio, so a WAV is the decoded stream (16-bit), not a lossless original:
//! the game ships none. An event is FMOD's: layers of sounds laid along parameters, each sound
//! playing one of a definition's samples; the JSON keeps that shape so a modder can rebuild it in
//! another engine's mixer.

use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use tf2_core::formats::fev::{Envelope, Event, Fev};
use tf2_core::formats::fsb::Bank;

use crate::describe::num;
use crate::file_safe;

/// The `sounds` block of `<pack>.json`, and what went wrong.
pub struct Written {
    pub json: Value,
    pub events: usize,
    pub samples: usize,
    pub problems: Vec<String>,
}

/// One sound event file and whether all its events are wanted.
struct Source {
    file: String,
    dir: PathBuf,
    fev: Fev,
    all: bool,
}

struct Writer<'a> {
    install: &'a Path,
    out: &'a Path,
    banks: HashMap<PathBuf, Option<Bank>>,
    /// (bank file, index) -> the written path, relative to `out`; `None` when it failed.
    done: HashMap<(PathBuf, usize), Option<String>>,
    used: BTreeSet<String>,
    samples: Map<String, Value>,
    problems: Vec<String>,
}

/// The level whose shared events are written (the port's default sound level).
pub const SHARED_LEVEL: &str = r"levels\us_city_00\eng\mission.fev";

/// The bank of the battle chatter's voice lines.
pub const CHATTER_BANK: &str = r"audio\eng\CHATTER.fsb";

/// Writes the sounds of `own` (the character's .fev) and the events of `shared` that the
/// character's JSON or its attributes name.
pub fn write(install: &Path, own: &Path, shared: &[PathBuf], character: &Value, attributes: &Value, out: &Path) -> Written {
    let mut named = BTreeSet::new();
    strings(character, &mut named);
    strings(attributes, &mut named);
    let mut problems = Vec::new();
    let mut sources = Vec::new();
    for (path, all) in std::iter::once((own, true)).chain(shared.iter().map(|p| (p.as_path(), false))) {
        match std::fs::read(path).map_err(|e| e.to_string()).and_then(|d| Fev::parse(&d)) {
            Ok(fev) => sources.push(Source {
                file: relative(install, path),
                dir: path.parent().map_or_else(PathBuf::new, Path::to_path_buf),
                fev,
                all,
            }),
            Err(e) => problems.push(format!("{}: {e}", path.display())),
        }
    }
    let mut writer = Writer { install, out, banks: HashMap::new(), done: HashMap::new(), used: BTreeSet::new(), samples: Map::new(), problems };
    let mut events = Map::new();
    for source in &sources {
        for event in &source.fev.events {
            if (source.all || named.contains(&event.name)) && !events.contains_key(&event.name) {
                let entry = writer.event(source, event);
                events.insert(event.name.clone(), entry);
            }
        }
    }
    // The battle chatter's lines are samples of `CHATTER.fsb`, each the event of the same name in
    // capitals, played through the `ProgrammerSoundPlayer` event (`mechanics.sound`). [game]
    let chatter = install.join(CHATTER_BANK);
    let wanted: BTreeSet<String> = character["mechanics"]["sound"]["chatter_lines"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|line| line["event"].as_str().map(str::to_ascii_uppercase))
        .collect();
    let mut voices = Map::new();
    if !wanted.is_empty() {
        match Bank::open(&chatter) {
            Ok(bank) => {
                for (index, entry) in bank.entries.iter().enumerate() {
                    let stem = entry.name.rsplit_once('.').map_or(entry.name.as_str(), |(s, _)| s).to_ascii_uppercase();
                    if wanted.contains(&stem) && !voices.contains_key(&stem) {
                        voices.insert(stem, json!(writer.sample(&chatter, index)));
                    }
                }
            }
            Err(e) => writer.problems.push(format!("{}: {e}", chatter.display())),
        }
    }
    let mut played = BTreeSet::new();
    clip_sounds(&character["clips"], &mut played);
    let not_found: Vec<&String> = played.iter().filter(|e| !events.contains_key(*e)).collect();
    let samples = writer.samples.len();
    Written {
        events: events.len(),
        json: json!({
            "folder": "sounds",
            "files": sources.iter().map(|s| json!({ "file": s.file, "all_events": s.all })).collect::<Vec<_>>(),
            "shared_level": SHARED_LEVEL,
            "not_found": not_found,
            "note": "Every event of the character's own sound file, and the shared events its JSON names: \
                     every level has its own copy of those (footsteps, jumps, punches); these are from \
                     shared_level. not_found: events the clips name that no event file holds (the game \
                     plays nothing for them). \
                     Pitch is in octaves (0 is the sample as recorded); volume as FMOD stores it (1 is full). \
                     A layer's sounds are laid along its parameter: each covers start..start+length of the \
                     range, as shares. A sound plays one sample of its list, picked by play_mode. \
                     The WAVs are the banks' MPEG audio decoded; loop points are in frames, end exclusive.",
            "events": events,
            "voices": voices,
            "samples": writer.samples,
        }),
        samples,
        problems: writer.problems,
    }
}

impl Writer<'_> {
    fn event(&mut self, source: &Source, event: &Event) -> Value {
        let parameter = |i: usize| event.parameters.get(i).map(|p| p.name.clone());
        let layers: Vec<Value> = event
            .layers
            .iter()
            .map(|layer| {
                let sounds: Vec<Value> = layer.sounds.iter().map(|sound| self.sound(source, sound)).collect();
                let envelopes: Vec<Value> = layer
                    .envelopes
                    .iter()
                    .map(|e| {
                        json!({
                            "drives": drives(e),
                            "parameter": parameter(e.parameter),
                            "points": e.points.iter().map(|&(x, y)| json!([num(x), num(y)])).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                json!({ "parameter": layer.parameter.and_then(parameter), "sounds": sounds, "envelopes": envelopes })
            })
            .collect();
        json!({
            "file": source.file,
            "category": event.category,
            "volume": num(event.volume),
            "volume_random": num(event.volume_randomisation),
            "pitch_octaves": num(event.pitch * 4.0),
            "pitch_random_octaves": num(event.pitch_randomisation * 4.0),
            "positional": event.mode & 0x10 != 0,
            // FMOD's roll-off bits of the event's mode (`tf2_core::sound::distance_gain`). [game]
            "rolloff": if event.mode & 0x0400_0000 != 0 { "custom" } else if event.mode & 0x0020_0000 != 0 { "linear" } else { "logarithmic" },
            "min_distance": num(event.min_distance),
            "max_distance": num(event.max_distance),
            "max_playbacks": event.max_playbacks,
            "fade_in_ms": event.fade_in_ms,
            "fade_out_ms": event.fade_out_ms,
            "reverb_dry_db": num(event.reverb_dry_db),
            "reverb_wet_db": num(event.reverb_wet_db),
            "loops": event.infinite(),
            "parameters": event.parameters.iter().map(|p| json!({
                "name": p.name,
                "min": num(p.min),
                "max": num(p.max),
                "velocity": num(p.velocity),
                "seek_speed": num(p.seek_speed),
                "primary": p.primary,
                "sustain": p.sustain.iter().map(|&s| num(s)).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "layers": layers,
        })
    }

    fn sound(&mut self, source: &Source, sound: &tf2_core::formats::fev::Sound) -> Value {
        let mut entry = json!({
            "start": num(sound.start),
            "length": num(sound.length),
            "one_shot": sound.one_shot,
            "volume": num(sound.volume),
            "fine_tune": num(sound.fine_tune),
            "fade_in": num(sound.fade_in),
            "fade_out": num(sound.fade_out),
        });
        let Some(def) = source.fev.definitions.get(sound.definition) else {
            self.problems.push(format!("{}: a sound names definition {} of {}", source.file, sound.definition, source.fev.definitions.len()));
            return entry;
        };
        let samples: Vec<Value> = def
            .waves
            .iter()
            .map(|wave| {
                let file = wave.sample.as_ref().and_then(|(bank, index)| self.sample(&source.dir.join(format!("{bank}.fsb")), *index));
                json!({ "file": file, "weight": wave.weight })
            })
            .collect();
        let map = entry.as_object_mut().expect("an object");
        map.insert("definition".into(), json!(def.name));
        map.insert("play_mode".into(), json!(play_mode(def.play_mode)));
        map.insert("definition_volume".into(), json!(num(def.volume)));
        map.insert("definition_volume_floor".into(), json!(num(def.volume_floor)));
        map.insert("definition_pitch_octaves".into(), json!(num(def.pitch * 4.0)));
        map.insert("definition_pitch_random_octaves".into(), json!(num(def.pitch_randomisation * 4.0)));
        map.insert("samples".into(), json!(samples));
        entry
    }

    /// Writes sample `index` of the bank at `path` once and returns its path under `out`.
    fn sample(&mut self, path: &Path, index: usize) -> Option<String> {
        let key = (path.to_path_buf(), index);
        if let Some(done) = self.done.get(&key) {
            return done.clone();
        }
        let written = self.decode(path, index);
        self.done.insert(key, written.clone());
        written
    }

    fn decode(&mut self, path: &Path, index: usize) -> Option<String> {
        let bank = self.banks.entry(path.to_path_buf()).or_insert_with(|| Bank::open(path).ok());
        let Some(bank) = bank.as_ref() else {
            self.problems.push(format!("{}: cannot open the bank", path.display()));
            return None;
        };
        let Some(entry) = bank.entries.get(index) else {
            self.problems.push(format!("{}: no sample {index} of {}", path.display(), bank.entries.len()));
            return None;
        };
        let sample = match bank.sample(index) {
            Ok(s) => s,
            Err(e) => {
                self.problems.push(e);
                return None;
            }
        };
        // Every level's bank is `mission0.fsb`: its folder is named by the level too.
        let mut bank_name = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        if let Some(level) = path.parent().and_then(Path::parent).and_then(Path::file_name).filter(|_| bank_name.to_lowercase().starts_with("mission")) {
            bank_name = format!("{}_{bank_name}", level.to_string_lossy());
        }
        let stem = entry.name.rsplit_once('.').map_or(entry.name.as_str(), |(s, _)| s);
        let stem = format!("sounds/{}/{}", file_safe(&bank_name), file_safe(if stem.is_empty() { &entry.name } else { stem }));
        let mut file = format!("{stem}.wav");
        let mut n = 2;
        while self.used.contains(&file.to_lowercase()) {
            file = format!("{stem}_{n}.wav");
            n += 1;
        }
        self.used.insert(file.to_lowercase());
        let target = self.out.join(&file);
        if let Err(e) = target.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| write_wav(&target, sample.rate, sample.channels, &sample.data)) {
            self.problems.push(format!("{file}: {e}"));
            return None;
        }
        let frames = sample.frames();
        self.samples.insert(
            file.clone(),
            json!({
                "bank": relative(self.install, path),
                "index": index,
                "name": entry.name,
                "rate": sample.rate,
                "channels": sample.channels,
                "frames": frames,
                "seconds": num(frames as f32 / sample.rate.max(1) as f32),
                "loop": if sample.looping { json!([sample.loop_range.start, sample.loop_range.end]) } else { Value::Null },
            }),
        );
        Some(file)
    }
}

/// [game: 0118aa60, as `Definition::play_mode` reads it] how a definition picks its sample.
fn play_mode(mode: u32) -> &'static str {
    match mode {
        0 | 3 => "sequential",
        1 => "random",
        2 => "random_no_repeat",
        4 => "shuffle",
        6 => "shuffle_global",
        5 => "programmer",
        _ => "unknown",
    }
}

/// What an envelope drives: an effect's name, or the layer property its flags name.
fn drives(e: &Envelope) -> String {
    if !e.effect.is_empty() {
        return e.effect.clone();
    }
    match e.flags {
        Envelope::VOLUME => "volume".into(),
        Envelope::PITCH => "pitch".into(),
        Envelope::SPATIAL_BLEND => "spatial_blend".into(),
        f => format!("flags 0x{f:x}"),
    }
}

/// `path` inside the install, with forward slashes.
fn relative(install: &Path, path: &Path) -> String {
    path.strip_prefix(install).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// The events the clips' sound_play and sound_stop events name.
fn clip_sounds(value: &Value, into: &mut BTreeSet<String>) {
    match value {
        Value::Array(a) => a.iter().for_each(|v| clip_sounds(v, into)),
        Value::Object(o) => {
            if let Some(event) = o.get("event").and_then(Value::as_str).filter(|_| matches!(o.get("type").and_then(Value::as_str), Some("sound_play" | "sound_stop"))) {
                into.insert(event.to_owned());
            }
            o.values().for_each(|v| clip_sounds(v, into));
        }
        _ => {}
    }
}

/// Every string value in `value`, for finding the shared events the character's JSON names.
fn strings(value: &Value, into: &mut BTreeSet<String>) {
    match value {
        Value::String(s) => {
            into.insert(s.clone());
        }
        Value::Array(a) => a.iter().for_each(|v| strings(v, into)),
        Value::Object(o) => o.values().for_each(|v| strings(v, into)),
        _ => {}
    }
}

fn write_wav(path: &Path, rate: u32, channels: usize, data: &[f32]) -> std::io::Result<()> {
    let bytes = (data.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels as u16 * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&bytes.to_le_bytes());
    for &s in data {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    std::fs::File::create(path)?.write_all(&out)
}
