//! Every sound cue in a character's weapon objects' own clips (their `weapon_animations`), by
//! weapon, clip and tick, named from his bank and the shared level's: what a gun's model plays
//! as it unfolds, fires, switches and overheats. Read-only. For Jetfire's and Breakaway's missing
//! audio (the user's report, 2026-10-25).
//! `cargo run -p tf2-core --example weapon_clip_sounds -- <pack>`

use std::path::Path;

use tf2_core::formats::anim::SoundCue;

fn main() {
    let pack = std::env::args().nth(1).expect("pack");
    let dir = tf2_core::install::dir_string();
    let data = tf2_core::character::load(Path::new(&dir), &pack).expect("character");
    let names: std::collections::HashMap<u32, String> = std::env::var("TF2_NAMES_TSV").and_then(|p| std::fs::read_to_string(p).map_err(|_| std::env::VarError::NotPresent)).unwrap_or_default()
        .lines().filter_map(|l| { let (h, n) = l.split_once('\t')?; Some((u32::from_str_radix(h, 16).ok()?, n.to_owned())) }).collect();
    let name = |id: u32| names.get(&id).cloned().unwrap_or(format!("{id:08x}"));
    for (i, clips) in data.weapon_animations.iter().enumerate() {
        let weapon = data.weapons.get(i).map_or(0, |w| w.name);
        for r in clips {
            let cues: Vec<String> = r.clip.iter().flat_map(|c| &c.sound_events).map(|e| match e.cue {
                SoundCue::Play { id, auto_stop, held } => format!("t{:.0} play {}{}{}", e.time, name(id), if auto_stop { " autostop" } else { "" }, if held { " held" } else { "" }),
                SoundCue::Stop { id } => format!("t{:.0} stop {}", e.time, name(id)),
                _ => format!("t{:.0} other", e.time),
            }).collect();
            if !cues.is_empty() {
                println!("weapon {i} ({weapon:#010x}) {}: {}", r.id, cues.join(", "));
            }
        }
    }
}
