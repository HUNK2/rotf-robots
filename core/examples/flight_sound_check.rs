//! Each flyer's flight and hover loops (`soundTable.vehicleDrive`, `hoverSound`) and its turbo,
//! from its own bank: whether the event resolves, how many waveforms it decodes, and how loud
//! three seconds of it render with `speed` swept from 0 to the flyer's top speed (and `pitch` /
//! `roll` at 0). Writes each to `work\flight_sound_<pack>_<kind>.wav` for listening. This
//! renders the sound; it does not prove it sounds right.
//! `cargo run -p tf2-core --example flight_sound_check -- [pack:AttributesName...]`

use std::io::Write;
use std::path::{Path, PathBuf};

use tf2_core::sound::{Library, Mixer};

const RATE: u32 = 48000;
const FLYERS: [&str; 6] = [
    "starscream:Starscream", "jetfire:Jetfire", "firestorm:Firestorm", "blackout:Blackout",
    "optimusflight:OptimusFlight", "megatronflight:MegatronFlight",
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

fn main() {
    let game = PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| r"C:\Games2".into()));
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        args = FLYERS.map(str::to_owned).to_vec();
    }
    for arg in args {
        let (pack, name) = arg.split_once(':').expect("pack:Name");
        let data = tf2_core::character::load(&game, pack).expect("character");
        let t = tf2_core::tuning::load(&game, name).expect("tuning");
        let list = tf2_core::formats::characterlist::CharacterList::load(&game).ok();
        let audio = list
            .as_ref()
            .and_then(|l| l.find(pack))
            .and_then(|e| l_spec(list.as_ref().unwrap(), e))
            .unwrap_or_else(|| pack.to_owned());
        let (library, problems) = Library::load(&[
            game.join(format!(r"characters\eng\{audio}.fev")),
            game.join(r"audio\eng\global.fev"),
        ]);
        for p in problems.iter().take(2) {
            eprintln!("  {p}");
        }
        let top = t.forms.fly.max_turbo_speed.max(t.forms.fly.max_speed).max(1.0);
        println!("== {pack} (bank {audio}): top speed {top}");
        for (kind, id) in [("fly", data.sounds.drive), ("hover", data.sounds.hover), ("turbo", data.sounds.turbo)] {
            if id == 0 {
                println!("  {kind:6} none in the soundTable");
                continue;
            }
            let label = library.name(id).unwrap_or("unnamed").to_owned();
            if !library.has(id) {
                println!("  {kind:6} {label} ({id:08x}): NOT in the bank");
                continue;
            }
            let decoded = library.prepare(id);
            let mut mixer = Mixer::new(RATE);
            let Some(handle) = mixer.play(&library, id, 1.0) else {
                println!("  {kind:6} {label}: does not play");
                continue;
            };
            mixer.set_parameter(handle, "2dPanLevel", 1.0);
            let block = RATE as usize / 100;
            let mut buffer = vec![0.0f32; block * 2];
            let mut samples = Vec::new();
            let (mut sum, mut peak, mut count) = (0.0f64, 0.0f32, 0usize);
            for b in 0..300 {
                let speed = top * b as f32 / 300.0;
                mixer.set_parameter(handle, "speed", speed);
                mixer.set_parameter(handle, "pitch", 0.0);
                mixer.set_parameter(handle, "roll", 0.0);
                buffer.iter_mut().for_each(|s| *s = 0.0);
                mixer.render(&library, &mut buffer);
                for s in &buffer {
                    sum += (*s as f64) * (*s as f64);
                    peak = peak.max(s.abs());
                    count += 1;
                    samples.push((s.clamp(-1.0, 1.0) * 32767.0) as i16);
                }
            }
            let rms = (sum / count.max(1) as f64).sqrt();
            let out = PathBuf::from(format!(r"C:\Bumblebee\work\flight_sound_{pack}_{kind}.wav"));
            write_wav(&out, &samples);
            println!("  {kind:6} {label}: {decoded} waveforms, rms {rms:.4}, peak {peak:.3}{}", if rms < 1e-4 { "  SILENT" } else { "" });
        }
    }
}

fn l_spec(list: &tf2_core::formats::characterlist::CharacterList, entry: &tf2_core::formats::characterlist::Entry) -> Option<String> {
    list.spec(entry).map(|s| s.audio)
}
