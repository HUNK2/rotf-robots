//! Prints one block of a character's `mechanics` (or all of it) without writing the export.
//! `cargo run -p rotf-export --example mechanics_peek -- <name> [block]`

use rotf_export::names::{self, Names};
use tf2_core::formats::characterlist::CharacterList;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let install = tf2_core::install::dir();
    let roster = CharacterList::load(install).expect("the character list");
    let spec = rotf_export::spec(&roster, &args[0]);
    let tuning = rotf_export::run::tuning(install, &spec).expect("the tuning");
    let data = tf2_core::character::load(install, &spec.pack).expect("the character");
    let mut files = names::data_files(&spec.pack_file(install));
    let pack_files = files.len();
    files.extend(names::data_files(&install.join("bnxglobal.str")));
    let names = Names::load(install, files, &[spec.sound_events(install)]);
    let all = rotf_export::mechanics::mechanics(&tuning, &data, &names, pack_files, &spec.attributes);
    let shown = args.get(1).map_or(&all, |block| &all[block.as_str()]);
    println!("{}", serde_json::to_string_pretty(shown).unwrap());
}
