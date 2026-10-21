//! Every character of the character list and every mission of the mission list, with the
//! names `--character` and `--mission` take. Feeds the test launcher (`Play-Test.cmd`).
//!
//! cargo run --manifest-path C:/Bumblebee/Cargo.toml -p tf2-core --example list_roster [-- <game dir>]
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| r"C:\Games2".into()));
    let roster = tf2_core::formats::characterlist::CharacterList::load(&dir).expect("character list");
    let missions = tf2_core::formats::missionlist::MissionList::load(&dir).expect("mission list");
    println!("[characters]");
    for e in &roster.entries {
        let pack = e.file_path.rsplit(['/', '\\']).next().unwrap_or(&e.file_path).to_ascii_lowercase();
        println!("{pack}\t{}", e.name);
    }
    println!("[missions]");
    for m in &missions.missions {
        if let Some(name) = &m.name {
            println!("{name}\t{}", m.category.as_deref().unwrap_or(""));
        }
    }
}
