//! Writes Bumblebee and checks the sounds against the source: every event of his own sound file
//! is in the JSON's `sounds` (as many as `core/examples/fev_list` counts); every event a clip or
//! his sound table names is there or listed as found nowhere, and those are in no event file the
//! export reads; every WAV is the bank's sample (its length and format) and not silence. Reads
//! the install; skips without it.

mod common;

use std::collections::HashSet;

use serde_json::Value;
use tf2_core::formats::fev::Fev;
use tf2_core::formats::fsb::Bank;

fn strings(value: &Value, into: &mut HashSet<String>) {
    match value {
        Value::String(s) => {
            into.insert(s.clone());
        }
        Value::Array(a) => a.iter().for_each(|v| strings(v, into)),
        Value::Object(o) => o.values().for_each(|v| strings(v, into)),
        _ => {}
    }
}

#[test]
fn bumblebee_sounds_match_the_banks() {
    let Some(export) = common::export("bumblebee", "sounds") else { return };
    let install = tf2_core::install::dir();
    let json: Value = serde_json::from_str(&std::fs::read_to_string(export.out.join(&export.written.json)).unwrap()).unwrap();
    let sounds = &json["sounds"];
    let events = sounds["events"].as_object().unwrap();

    // Every event of his own file.
    let own = Fev::parse(&std::fs::read(export.spec.sound_events(install)).unwrap()).unwrap();
    let missing: Vec<&str> = own.events.iter().map(|e| e.name.as_str()).filter(|n| !events.contains_key(*n)).collect();
    assert!(missing.is_empty(), "{} of {} own events missing: {missing:?}", missing.len(), own.events.len());

    // What the clips and the sound table name is written, or found in no event file read.
    let shared: Vec<Fev> = sounds["files"].as_array().unwrap()[1..]
        .iter()
        .map(|f| Fev::parse(&std::fs::read(install.join(f["file"].as_str().unwrap())).unwrap()).unwrap())
        .collect();
    assert!(shared.len() >= 3, "the shared event files: {}", sounds["files"]);
    let not_found: HashSet<&str> = sounds["not_found"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    for name in &not_found {
        assert!(!own.events.iter().chain(shared.iter().flat_map(|f| &f.events)).any(|e| e.name == *name), "{name} is listed as found nowhere but is in an event file");
    }
    let mut played = HashSet::new();
    for clip in json["clips"].as_array().unwrap() {
        for event in clip["events"].as_array().unwrap() {
            if matches!(event["type"].as_str(), Some("sound_play" | "sound_stop")) {
                played.insert(event["event"].as_str().unwrap().to_owned());
            }
        }
    }
    let mut table = HashSet::new();
    strings(&json["character_sounds"], &mut table);
    let shared_names: HashSet<&str> = shared.iter().flat_map(|f| &f.events).map(|e| e.name.as_str()).collect();
    table.retain(|n| shared_names.contains(n.as_str()) || own.events.iter().any(|e| e.name == *n));
    // The human soldier's data names no clip, step or table sound: his gun's is his only event.
    assert!(!events.is_empty(), "no sound events written");
    for name in played.iter().chain(&table) {
        assert!(events.contains_key(name) || not_found.contains(name.as_str()), "{name} is neither written nor listed as found nowhere");
    }

    // Every sample an event plays is written, and is the bank's sample.
    let samples = sounds["samples"].as_object().unwrap();
    let mut referenced = HashSet::new();
    for event in events.values() {
        for layer in event["layers"].as_array().unwrap() {
            for sound in layer["sounds"].as_array().unwrap() {
                for sample in sound["samples"].as_array().unwrap() {
                    if let Some(file) = sample["file"].as_str() {
                        assert!(samples.contains_key(file), "{file} is played but not in samples");
                        referenced.insert(file.to_owned());
                    }
                }
            }
        }
    }
    // The chatter's lines are played by name, through the voice event.
    referenced.extend(sounds["voices"].as_object().unwrap().values().filter_map(Value::as_str).map(str::to_owned));
    assert_eq!(referenced.len(), samples.len(), "samples nothing plays");
    let mut silent = Vec::new();
    for (file, info) in samples {
        let wav = std::fs::read(export.out.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(&wav[..4], b"RIFF", "{file}");
        let channels = u16::from_le_bytes([wav[22], wav[23]]) as u64;
        let rate = u32::from_le_bytes(wav[24..28].try_into().unwrap()) as u64;
        let bytes = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as u64;
        assert_eq!(wav.len() as u64, 44 + bytes, "{file}: data size");
        let frames = bytes / (2 * channels);
        assert_eq!(frames, info["frames"].as_u64().unwrap(), "{file}: frames");
        assert_eq!((channels, rate), (info["channels"].as_u64().unwrap(), info["rate"].as_u64().unwrap()), "{file}");
        let bank = Bank::open(&install.join(info["bank"].as_str().unwrap())).unwrap();
        let entry = &bank.entries[info["index"].as_u64().unwrap() as usize];
        assert_eq!(entry.name, info["name"].as_str().unwrap(), "{file}");
        if entry.frames > 0 {
            assert_eq!(frames, entry.frames as u64, "{file}: the bank's length");
        }
        let peak = wav[44..].as_chunks::<2>().0.iter().map(|&c| i16::from_le_bytes(c).unsigned_abs()).max().unwrap_or(0);
        // Devastator's mission bank holds a sample named `null`: silence on purpose.
        if peak < 33 && !entry.name.to_lowercase().starts_with("null") {
            silent.push(file.clone());
        }
    }
    assert!(silent.is_empty(), "silent samples: {silent:?}");
    println!("{} events ({} own), {} samples, not found: {not_found:?}", events.len(), own.events.len(), samples.len());
}
