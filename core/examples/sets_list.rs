//! The animation sets of one character's pack and the clips each holds.
//! `cargo run -p tf2-core --example sets_list -- <pack>...` (reads `C:\Games2`).

use std::path::Path;

fn main() {
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    for pack in std::env::args().skip(1) {
        let data = match tf2_core::character::load(Path::new(&dir), &pack) {
            Ok(data) => data,
            Err(e) => {
                println!("{pack}: {e}");
                continue;
            }
        };
        println!("== {pack}: {} sets", data.animations.sets.len());
        let mut names: Vec<_> = data.animations.sets.keys().collect();
        names.sort();
        for name in names {
            let refs = &data.animations.sets[name];
            let ids: Vec<String> = refs.iter().map(|r| format!("{}{}", r.id, if r.clip.is_some() { "" } else { "(no clip)" })).collect();
            println!("  {name} [{}]: {}", refs.len(), ids.join(", "));
        }
    }
}
