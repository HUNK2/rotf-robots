//! The animation sets of one character's pack and the clips each holds.
//! `cargo run -p tf2-core --example sets_list -- <pack>...` (reads the install).

use std::path::Path;

fn main() {
    let dir = tf2_core::install::dir_string();
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
