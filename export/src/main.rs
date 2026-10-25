//! rotf-export: a robot from your own install as glTF, PNG and DDS, for modding tools.
//!
//!   cargo run --release -p rotf-export -- --list
//!   cargo run --release -p rotf-export -- --character <name> [--out <folder>] [--game-dir <install>]
//!
//! `<name>` is what the game's `--character` takes (`--list` prints them). The install is found as
//! the game finds it; `--game-dir` names it when it is not. The output folder defaults to
//! `exported/<pack>` under the current folder.

use std::path::PathBuf;

use tf2_core::formats::characterlist::CharacterList;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fail = |message: String| -> ! {
        eprintln!("{message}");
        std::process::exit(1);
    };
    let install = match flag(&args, "--game-dir").map(PathBuf::from) {
        Some(dir) if tf2_core::install::is_install(&dir) => dir,
        Some(dir) => fail(format!("{} has no {}: that is not the game's install folder.", dir.display(), tf2_core::install::MARKER)),
        None => tf2_core::install::find().filter(|d| tf2_core::install::is_install(d)).unwrap_or_else(|| {
            fail(format!("The game's install was not found. Pass --game-dir <folder>, the folder that holds {}.", tf2_core::install::MARKER))
        }),
    };
    let roster = CharacterList::load(&install).unwrap_or_else(|e| fail(format!("Could not read the character list: {e}")));
    if args.iter().any(|a| a == "--list") {
        // The list names some characters twice (their single and multiplayer entries).
        let mut seen = std::collections::HashSet::new();
        for entry in &roster.entries {
            if let Some(spec) = roster.spec(entry).filter(|s| seen.insert((entry.name.clone(), s.pack.clone()))) {
                println!("{:<24} pack {}", entry.name, spec.pack);
            }
        }
        return;
    }
    let Some(name) = flag(&args, "--character") else {
        fail("Name a robot: --character <name> (--list prints them).".into());
    };
    let spec = rotf_export::spec(&roster, &name);
    let out = flag(&args, "--out").map_or_else(|| PathBuf::from("exported").join(&spec.pack), PathBuf::from);
    let written = rotf_export::export(&install, &spec, &out).unwrap_or_else(|e| fail(format!("{name}: {e}")));
    println!("{} (pack {}) -> {}", spec.display.as_deref().unwrap_or(&spec.pack), spec.pack, out.display());
    for (file, s) in &written.files {
        println!(
            "  {file}: {} nodes, {} parts ({} skinned), {} vertices, {} triangles, {} materials, {} textures, {} animations",
            s.nodes, s.parts, s.skins, s.vertices, s.triangles, s.materials, s.textures, s.animations
        );
        if !s.other_model_clips.is_empty() {
            println!("    {} clips move no node of any model in the pack (written as events only, in the JSON): {}", s.other_model_clips.len(), s.other_model_clips.join(", "));
        }
        for problem in &s.problems {
            println!("    note: {problem}");
        }
    }
    println!("  {}: clip events, weapon mounts, render sets, materials, attributes", written.json);
    println!("  sounds/: {} events, {} samples as WAV", written.sounds.0, written.sounds.1);
    for problem in &written.problems {
        println!("  note: {problem}");
    }
    println!("Made from your own copy of the game: keep these files to yourself (README.txt).");
}
