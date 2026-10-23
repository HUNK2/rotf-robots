//! Devastator's materials: for each mesh his robot object draws, every part's material and its
//! technique (named from `export\data\names.tsv` when found), with the textures it names.
//!
//!   cargo run -p tf2-core --example devastator_materials

use std::collections::BTreeMap;
use std::path::Path;

fn main() {
    let dir = tf2_core::install::dir();
    let data = tf2_core::character::load(dir, "devastator").expect("Devastator's pack");
    let names: std::collections::HashMap<u32, String> = std::fs::read_to_string("C:/Bumblebee/export/data/names.tsv")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (h, n) = l.split_once('\t')?;
            Some((u32::from_str_radix(h, 16).ok()?, n.to_owned()))
        })
        .collect();
    let name = |h: u32| names.get(&h).cloned().unwrap_or_else(|| format!("{h:#010x}"));
    let mut by_technique: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let wanted: std::collections::HashSet<String> = data.robot.renders.iter().map(|r| r.model.to_lowercase()).collect();
    for mesh in data.library.meshes.values().filter(|m| wanted.contains(&m.name.to_lowercase())) {
        for part in &mesh.parts {
            let Some(material) = part.material.and_then(|id| data.library.materials.get(&id)) else { continue };
            let textures: Vec<String> = ["diffuse_texture1_texture", "normal_map_texture", "specular_texture", "reflection_texture"]
                .iter()
                .filter_map(|p| material.texture(p).map(|t| format!("{p}={}", if data.library.textures.contains_key(&t) { "ok" } else { "MISSING" })))
                .collect();
            by_technique.entry(name(material.technique)).or_default().push(format!("{} / {} {}", mesh.name, material.name, textures.join(" ")));
        }
    }
    for (technique, parts) in &by_technique {
        println!("{technique}: {} parts", parts.len());
        for p in parts.iter().take(4) {
            println!("    {p}");
        }
    }
    println!("{} renders, {} meshes found", data.robot.renders.len(), wanted.len());
}
