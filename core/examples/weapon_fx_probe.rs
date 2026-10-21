//! Per playable pack: each weapon's type and every particle effect found under it (fields
//! named like an effect that read as a template, and `ParticleEmitter` things), with the
//! path to it -- to see which weapons' effects `character::read_weapon_effects` (guns and
//! missile launchers only) leaves out.
//! `cargo run -p tf2-core --example weapon_fx_probe [pack...]` (reads the install).

use std::path::Path;

use tf2_core::formats::lxb::{DataFile, Node};
use tf2_core::formats::pack::{self, Pack};
use tf2_core::particles::Template;

const PACKS: [&str; 13] = [
    "bumblebee", "optimus", "ratchet", "ironhide", "megatron", "longhaul", "audir8", "jetfire", "firestorm",
    "starscream", "blackout", "optimusflight", "megatronflight",
];

fn walk(file: &DataFile, node: Node, path: String, depth: usize, out: &mut Vec<String>) {
    if depth > 9 {
        return;
    }
    if node.type_name() == Some("ParticleEmitter") {
        if let Some(t) = node.get("id").and_then(Template::read) {
            out.push(format!("{path} emitter {:08x} {}", t.name, file.name(t.name).unwrap_or("?")));
        }
    }
    for (name, child) in node.fields() {
        let key = file.name(name).unwrap_or("?").to_string();
        let lower = key.to_lowercase();
        if lower.contains("effect") || lower.contains("fx") || lower.contains("particle") {
            if let Some(t) = Template::read(child) {
                let ends: Vec<String> = t.elements.iter().map(|e| format!("{}..{}", e.start, e.end)).collect();
                out.push(format!("{path}.{key} {:08x} {} elements {ends:?}", t.name, file.name(t.name).unwrap_or("?")));
                continue;
            }
        }
        walk(file, child, format!("{path}.{key}"), depth + 1, out);
    }
    for (i, item) in node.items().enumerate() {
        let ty = item.get("thing").and_then(|t| t.type_name()).or(item.type_name()).unwrap_or("");
        walk(file, item, format!("{path}[{i}{}{ty}]", if ty.is_empty() { "" } else { ":" }), depth + 1, out);
    }
}

fn main() {
    let dir = tf2_core::install::dir_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let packs: Vec<&str> = if args.is_empty() { PACKS.to_vec() } else { args.iter().map(String::as_str).collect() };
    for name in packs {
        let pack = Pack::open(&Path::new(&dir).join("characters").join(format!("{name}.str"))).expect("pack");
        let chunk = pack.of_type(pack::DATA).next().expect("data");
        let file = DataFile::parse(pack.data(chunk).to_vec()).expect("parse");
        println!("== {name}");
        for (_, root) in file.root().fields() {
            for thing in root.get("things").into_iter().flat_map(|t| t.items()) {
                if thing.type_name() != Some("Character") {
                    continue;
                }
                for (i, weapon) in thing.get("weapons").into_iter().flat_map(|w| w.items()).enumerate() {
                    let kinds: Vec<&str> = weapon
                        .path(&["obj", "attachments"])
                        .into_iter()
                        .flat_map(|a| a.items())
                        .filter_map(|a| a.get("thing").and_then(|t| t.type_name()))
                        .collect();
                    // A grenade's projectile: what its `obj` and `damage` objects hold.
                    for launcher in weapon.path(&["obj", "attachments"]).into_iter().flat_map(|a| a.items()).filter_map(|a| a.get("thing")) {
                        let Some(g) = launcher.get("grenadeProjectileObj") else { continue };
                        for (field, child) in g.fields() {
                            let types: Vec<String> = child
                                .get("attachments")
                                .into_iter()
                                .flat_map(|a| a.items())
                                .filter_map(|a| a.get("thing"))
                                .map(|t| {
                                    let inner = t.get("thing").and_then(|i| i.type_name()).map(|n| format!("->{n}")).unwrap_or_default();
                                    format!("{}{inner}", t.type_name().unwrap_or("?"))
                                })
                                .collect();
                            println!("  grenade.{} {:?}", file.name(field).unwrap_or("?"), types);
                        }
                    }
                    let mut out = Vec::new();
                    walk(&file, weapon, String::new(), 0, &mut out);
                    println!("  weapon {i} {kinds:?}: {} effects", out.len());
                    for line in out {
                        println!("    {line}");
                    }
                }
            }
        }
    }
}
