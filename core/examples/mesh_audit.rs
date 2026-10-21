//! Every playable robot's mesh and material sets, audited the way Optimus's were
//! (`notes\optimus-mesh-visibility.md`): for the robot and the vehicle object, the
//! RenderSetEnabler groups and how many of each set's links resolve to a drawn mesh, the meshes
//! drawn by default, and the suspicious ones drawn by default at the same time as their twin
//! (an `_SFX`, damage or LOD copy of another part outside any group), and meshes whose model is
//! missing from the pack's library. Read-only.
//! `cargo run -p tf2-core --example mesh_audit -- [pack...]`

use std::collections::HashMap;
use std::path::Path;

use tf2_core::formats::hash::lower33;
use tf2_core::formats::scene::ObjectDef;

const ROSTER: [&str; 13] = [
    "bumblebee", "optimus", "ironhide", "ratchet", "megatron", "longhaul", "audir8", "jetfire", "firestorm",
    "starscream", "blackout", "optimusflight", "megatronflight",
];

fn audit(label: &str, object: &ObjectDef, library: &tf2_core::formats::model::Library) -> usize {
    let mut problems = 0;
    let local = |model: &str| model.rsplit('.').next().unwrap_or(model).to_owned();
    // Default visibility: a part of an exclusive group shows only in set 0. [game 00803150]
    let drawn: Vec<&str> = object
        .renders
        .iter()
        .map(|r| r.model.as_str())
        .filter(|m| object.render_sets.iter().all(|g| !g.exclusive || !g.sets.iter().enumerate().skip(1).any(|(i, _)| g.contains_model(m, i)) || g.contains_model(m, 0)))
        .collect();
    let missing: Vec<&str> = object.renders.iter().map(|r| r.model.as_str()).filter(|m| !library.meshes.contains_key(&lower33(m))).collect();
    print!("  {label}: {} meshes, {} drawn by default", object.renders.len(), drawn.len());
    for group in &object.render_sets {
        let sets: Vec<String> = group
            .sets
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let found = object.renders.iter().filter(|r| group.contains_model(&r.model, i)).count();
                if found != s.nodes.len() {
                    problems += 1;
                }
                format!("{:08x} {found}/{}", s.name, s.nodes.len())
            })
            .collect();
        print!("; group{} [{}]", if group.exclusive { " exclusive" } else { "" }, sets.join(", "));
    }
    println!();
    if !missing.is_empty() {
        problems += 1;
        println!("    MISSING from the library ({}): {}", missing.len(), missing.iter().take(8).cloned().collect::<Vec<_>>().join(", "));
    }
    // Materials: every part of every drawn mesh names a material the library has, and every
    // texture parameter of it names a texture the pack holds (or a shared one, counted apart).
    let (mut parts, mut no_material, mut textures, mut absent) = (0, 0, 0, Vec::new());
    for m in &drawn {
        let Some(mesh) = library.meshes.get(&lower33(m)) else { continue };
        for part in &mesh.parts {
            parts += 1;
            let Some(material) = part.material.and_then(|id| library.materials.get(&id)) else {
                no_material += 1;
                continue;
            };
            for param in &material.params {
                if let tf2_core::formats::model::ParamValue::Texture(Some(id)) = &param.value {
                    textures += 1;
                    if !library.textures.contains_key(id) && !library.texture_frames.contains_key(id) {
                        absent.push(format!("{}:{id:08x}", material.name));
                    }
                }
            }
        }
    }
    absent.sort();
    absent.dedup();
    println!("    {parts} parts, {no_material} without a material; {textures} texture uses, {} not in the pack", absent.len());
    if no_material > 0 {
        problems += 1;
    }
    if !absent.is_empty() {
        println!("      not in the pack (shared or missing): {}", absent.iter().take(6).cloned().collect::<Vec<_>>().join(", "));
    }
    // Twins drawn together: a default-drawn part whose name, less a variant suffix, is another
    // default-drawn part.
    let names: HashMap<String, &str> = drawn.iter().map(|m| (local(m).to_ascii_lowercase(), *m)).collect();
    let mut twins = Vec::new();
    for m in &drawn {
        let l = local(m).to_ascii_lowercase();
        for suffix in ["_sfx", "_damaged", "_damage", "_dmg", "_broken", "_lod1", "_lod2", "_low", "_glow", "_special"] {
            if let Some(base) = l.strip_suffix(suffix) {
                if names.contains_key(base) {
                    twins.push(format!("{m} with {}", names[base]));
                }
            }
        }
    }
    if !twins.is_empty() {
        problems += 1;
        println!("    TWINS drawn together ({}): {}", twins.len(), twins.iter().take(8).cloned().collect::<Vec<_>>().join("; "));
    }
    problems
}

fn main() {
    let dir = tf2_core::install::dir_string();
    let mut packs: Vec<String> = std::env::args().skip(1).collect();
    if packs.is_empty() {
        packs = ROSTER.map(str::to_owned).to_vec();
    }
    for pack in packs {
        let data = tf2_core::character::load(Path::new(&dir), &pack).expect("load");
        println!("== {pack}");
        let mut problems = audit("robot", &data.robot, &data.library);
        if let Some(vehicle) = &data.vehicle {
            problems += audit("vehicle", vehicle, &data.library);
        }
        println!("  {}", if problems == 0 { "clean".to_owned() } else { format!("{problems} problem(s)") });
    }
}
