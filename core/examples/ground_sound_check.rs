//! Each ground robot's engine and footsteps, from its own data and bank. The engine
//! (`soundTable.vehicleTransmission`): whether the event resolves, how many gears the drive mode's
//! `gearList` gives, and how loud three seconds of it render with `rpm` swept over the gears and
//! `load` at one half. The footsteps: the FootStep attachments (node, side, `heightFromGround`), the
//! `footStepFX` default sound, and, at every FootStep event of every clip, where the named foot is
//! in the object's frame against the ground at z 0: whether the port's old probe (z - h to z + h,
//! until 2026-10-20) and the game's (z - h + 1.5 to z - h - 1.5, 0073f420, now the port's) reach
//! it. Writes the engine to
//! `work\ground_sound_<pack>_engine.wav` for listening. Renders the sound; does not prove it right.
//! `cargo run -p tf2-core --example ground_sound_check -- [pack:AttributesName...]`

use std::io::Write;
use std::path::{Path, PathBuf};

use tf2_core::formats::anim::SoundCue;
use tf2_core::sound::{Library, Mixer};

const RATE: u32 = 48000;
const GROUND: [&str; 7] = [
    "bumblebee:Bumblebee", "optimus:Optimus", "ironhide:Ironhide", "ratchet:Ratchet", "megatron:Megatron",
    "longhaul:LongHaul", "audir8:Audi",
];

fn write_wav(path: &Path, samples: &[i16]) {
    let mut f = std::fs::File::create(path).expect("wav");
    let data = (samples.len() * 2) as u32;
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + data).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&2u16.to_le_bytes()).unwrap();
    f.write_all(&RATE.to_le_bytes()).unwrap();
    f.write_all(&(RATE * 4).to_le_bytes()).unwrap();
    f.write_all(&4u16.to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&data.to_le_bytes()).unwrap();
    for s in samples {
        f.write_all(&s.to_le_bytes()).unwrap();
    }
}

/// Renders `seconds` of an event, calling `set` before each hundredth of a second with the share of
/// the time gone; the RMS and peak, and the samples.
fn render(library: &Library, id: u32, seconds: f32, set: impl Fn(&mut Mixer, tf2_core::sound::Handle, f32)) -> Option<(f64, f32, Vec<i16>)> {
    library.prepare(id);
    let mut mixer = Mixer::new(RATE);
    let handle = mixer.play(library, id, 1.0)?;
    mixer.set_parameter(handle, "2dPanLevel", 1.0);
    let block = RATE as usize / 100;
    let blocks = (seconds * 100.0) as usize;
    let mut buffer = vec![0.0f32; block * 2];
    let mut samples = Vec::new();
    let (mut sum, mut peak, mut count) = (0.0f64, 0.0f32, 0usize);
    for b in 0..blocks {
        set(&mut mixer, handle, b as f32 / blocks as f32);
        buffer.iter_mut().for_each(|s| *s = 0.0);
        mixer.render(library, &mut buffer);
        for s in &buffer {
            sum += (*s as f64) * (*s as f64);
            peak = peak.max(s.abs());
            count += 1;
            samples.push((s.clamp(-1.0, 1.0) * 32767.0) as i16);
        }
    }
    Some(((sum / count.max(1) as f64).sqrt(), peak, samples))
}

fn main() {
    let game = PathBuf::from(tf2_core::install::dir_string());
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        args = GROUND.map(str::to_owned).to_vec();
    }
    let list = tf2_core::formats::characterlist::CharacterList::load(&game).ok();
    for arg in args {
        let (pack, name) = arg.split_once(':').expect("pack:Name");
        let data = tf2_core::character::load(&game, pack).expect("character");
        let t = tf2_core::tuning::load(&game, name).expect("tuning");
        let audio = list
            .as_ref()
            .and_then(|l| l.find(pack).and_then(|e| l.spec(e)))
            .map(|s| s.audio)
            .unwrap_or_else(|| pack.to_owned());
        // The banks `game\src\sound.rs` `start` loads, with its default level.
        let (library, _) = Library::load(&[
            game.join(format!(r"characters\eng\{audio}.fev")),
            game.join(r"levels\us_city_00\eng\mission.fev"),
            game.join(r"audio\eng\global.fev"),
            game.join(r"audio\eng\global_streaming.fev"),
        ]);
        println!("== {pack} (bank {audio})");

        let id = data.sounds.engine;
        let label = library.name(id).unwrap_or("unnamed").to_owned();
        let gears = &t.drive_gears;
        let (low, high) = gears.iter().fold((f32::MAX, 0.0f32), |(l, h), g| (l.min(g.min_rpm), h.max(g.max_rpm)));
        print!("  engine {label} ({id:08x}), {} gears", gears.len());
        if id == 0 || !library.has(id) {
            println!(": NOT in the bank");
        } else if gears.is_empty() {
            println!(": no gears, so no gearbox drives it");
        } else {
            match render(&library, id, 3.0, |m, h, share| {
                m.set_parameter(h, "rpm", low + (high - low) * share);
                m.set_parameter(h, "load", 0.5);
            }) {
                Some((rms, peak, samples)) => {
                    write_wav(&PathBuf::from(format!(r"C:\Bumblebee\work\ground_sound_{pack}_engine.wav")), &samples);
                    println!(": rpm {low:.0}..{high:.0}, rms {rms:.4}, peak {peak:.3}{}", if rms < 1e-4 { "  SILENT" } else { "" });
                }
                None => println!(": does not play"),
            }
        }

        let step = data.step_impact.pick(tf2_core::melee::surface::DEFAULT).map_or(0, |i| i.sound);
        let step_label = library.name(step).unwrap_or("unnamed").to_owned();
        print!("  footstep {step_label} ({step:08x}), {} preset entries", data.step_impact.infos.len());
        if step == 0 || !library.has(step) {
            println!(": NOT in the bank");
        } else {
            let heard = render(&library, step, 1.0, |_, _, _| {}).map_or(0.0, |(rms, _, _)| rms);
            println!(": rms {heard:.4}{}", if heard < 1e-4 { "  SILENT" } else { "" });
        }
        // Clips (the robot's and the car's) whose AnimPlaySoundEvent names an event these banks lack.
        let mut missing: Vec<String> = Vec::new();
        let robot_clips = data.animations.sets.iter().flat_map(|(set, refs)| refs.iter().map(move |r| (set.as_str(), r)));
        for (set, r) in robot_clips.chain(data.vehicle_animations.iter().map(|r| ("vehicle", r))) {
            let Some(clip) = r.clip.as_ref() else { continue };
            for event in &clip.sound_events {
                if let SoundCue::Play { id, .. } = event.cue
                    && !library.has(id)
                {
                    missing.push(format!("{set}/{} t {:.0}: {id:08x}", r.id, event.time));
                }
            }
        }
        if !missing.is_empty() {
            println!("  clip cues missing from the banks: {}", missing.join(", "));
        }
        // Each weapon row's fire sound (`genericWeaponSoundTable.fireSound`) and whether the banks have it.
        for w in &data.weapons {
            let id = w.fire_sound;
            println!("  weapon {:08x}: fire sound {} ({id:08x}){}", w.name, library.name(id).unwrap_or("unnamed"),
                if id == 0 { "  NONE" } else if library.has(id) { "" } else { "  NOT in the bank" });
        }
        // Every event the robot's own clips name, with how many cues name it: whose sounds his data uses.
        let mut named: std::collections::BTreeMap<String, usize> = Default::default();
        for r in data.animations.sets.values().flatten() {
            for event in r.clip.iter().flat_map(|c| &c.sound_events) {
                if let SoundCue::Play { id, .. } = event.cue {
                    *named.entry(library.name(id).map_or(format!("{id:08x}"), str::to_owned)).or_default() += 1;
                }
            }
        }
        let named: Vec<String> = named.iter().map(|(n, k)| format!("{n} x{k}")).collect();
        println!("  robot clip cues: {}", named.join(", "));
        // Cues that start an endless event (a sound loops) without ending it with the clip, and
        // whether any clip has a stop cue for it: one with neither plays until something else stops it.
        let mut endless: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
        let all = data.animations.sets.values().flatten().chain(data.vehicle_animations.iter());
        let cues: Vec<_> = all.flat_map(|r| r.clip.iter().flat_map(|c| c.sound_events.iter().map(|e| e.cue))).collect();
        for cue in &cues {
            if let SoundCue::Play { id, auto_stop: false, .. } = *cue
                && library.has(id) && library.infinite(id)
            {
                let stops = cues.iter().filter(|c| matches!(c, SoundCue::Stop { id: s } if *s == id)).count();
                let entry = endless.entry(library.name(id).unwrap_or("unnamed").to_owned()).or_default();
                entry.0 += 1;
                entry.1 = stops;
            }
        }
        for (n, (k, stops)) in &endless {
            println!("  ENDLESS cue {n} x{k}, stop cues {stops}");
            let id = tf2_core::sound::event_id(n);
            let sets = data.animations.sets.iter().flat_map(|(set, refs)| refs.iter().map(move |r| (set.as_str(), r)));
            for (set, r) in sets.chain(data.vehicle_animations.iter().map(|r| ("vehicle", r))) {
                for e in r.clip.iter().flat_map(|c| &c.sound_events) {
                    match e.cue {
                        SoundCue::Play { id: i, .. } if i == id => println!("    starts in {set}/{} t {:.0}", r.id, e.time),
                        SoundCue::Stop { id: i } if i == id => println!("    stops in {set}/{} t {:.0}", r.id, e.time),
                        _ => {}
                    }
                }
            }
        }
        let object = &data.robot;
        for foot in &object.footsteps {
            println!("    foot {} side {:08x} heightFromGround {:.2}", object.nodes[foot.node].name, foot.side, foot.height);
        }
        let rig = tf2_core::pose::Rig::new(object);
        let (mut events, mut no_foot, mut port_misses, mut game_misses) = (0, 0, 0, 0);
        let mut worst: Vec<String> = Vec::new();
        let mut sets: Vec<_> = data.animations.sets.iter().collect();
        sets.sort_by(|a, b| a.0.cmp(b.0));
        for (set, refs) in sets {
            for r in refs {
                let Some(clip) = r.clip.as_ref() else { continue };
                for event in &clip.sound_events {
                    let SoundCue::FootStep { side } = event.cue else { continue };
                    events += 1;
                    let feet: Vec<_> = object.footsteps.iter().filter(|f| f.side == side).collect();
                    if feet.is_empty() {
                        no_foot += 1;
                        continue;
                    }
                    let pose = rig.pose(clip, r.looping, event.time);
                    for foot in feet {
                        let z = pose[foot.node].translation.z;
                        let h = foot.height;
                        let port = z - h <= 0.0 && 0.0 <= z + h;
                        let game = z - h - 1.5 <= 0.0 && 0.0 <= z - h + 1.5;
                        port_misses += usize::from(!port);
                        game_misses += usize::from(!game);
                        if (!port || !game) && worst.len() < 6 {
                            worst.push(format!("{set}/{} t {:.0} {} z {z:.2} (old {}, game {})", r.id, event.time,
                                object.nodes[foot.node].name, if port { "hits" } else { "MISSES" }, if game { "hits" } else { "MISSES" }));
                        }
                    }
                }
            }
        }
        println!("  {events} FootStep events: {no_foot} name no foot, the old probe misses the ground {port_misses} times, the game's (now the port's) {game_misses}");
        for line in worst {
            println!("    {line}");
        }
    }
}
