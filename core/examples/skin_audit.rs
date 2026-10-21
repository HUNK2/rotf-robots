//! Every playable robot's skinned mesh parts, checked the way `game\src\assets.rs` `spawn` binds
//! them: each palette entry must name a bone of the mesh's skeleton, each bone a node of the
//! object (by `bone_hash`), and every vertex's bone number must fall inside the palette. Written
//! to test a guess about Megatron's tank treads drawn out in long strips (2026-10-20): it found no
//! problem on any of the 13 packs, and the strips came from the own-scale keys
//! (`formats::anim::Channel::own_scale`) being handed down the tread chain. Read-only.
//! `cargo run -p tf2-core --example skin_audit -- [pack...]`

use std::path::Path;

use tf2_core::formats::hash::lower33;
use tf2_core::formats::scene::ObjectDef;

const ROSTER: [&str; 13] = [
    "bumblebee", "optimus", "ironhide", "ratchet", "megatron", "longhaul", "audir8", "jetfire", "firestorm",
    "optimusflight", "starscream", "blackout", "megatronflight",
];

fn audit(label: &str, object: &ObjectDef, library: &tf2_core::formats::model::Library) -> usize {
    let mut bad = 0;
    for render in &object.renders {
        let Some(mesh) = library.meshes.get(&lower33(&render.model)) else { continue };
        let Some(bones) = mesh.skeleton.and_then(|id| library.skeletons.get(&id)) else { continue };
        for (p, part) in mesh.parts.iter().enumerate() {
            if part.palette.is_empty() {
                continue;
            }
            let no_bone: Vec<u16> = part.palette.iter().copied().filter(|&b| bones.get(b as usize).is_none()).collect();
            let no_node: Vec<String> = part
                .palette
                .iter()
                .filter_map(|&b| bones.get(b as usize))
                .filter(|bone| !object.nodes.iter().any(|n| n.bone_hash == bone.name))
                .map(|bone| format!("{:08x}", bone.name))
                .collect();
            // The highest bone number a vertex uses with a non-zero weight.
            let mut top = 0usize;
            for (indices, weights) in part.blend_indices.iter().zip(&part.blend_weights) {
                for k in 0..4 {
                    if weights[k] > 0.0 {
                        top = top.max(indices[k] as usize);
                    }
                }
            }
            let past = top >= part.palette.len();
            if !no_bone.is_empty() || !no_node.is_empty() || past {
                bad += 1;
                println!(
                    "  {label} {} part {p}: palette {} (skeleton {} bones), top vertex bone {top}{}; palette entries with no bone {:?}; bones with no node {:?}",
                    render.model,
                    part.palette.len(),
                    bones.len(),
                    if past { " PAST THE PALETTE" } else { "" },
                    no_bone,
                    no_node
                );
            }
        }
    }
    bad
}

fn main() {
    let game = tf2_core::install::dir_string();
    let mut packs: Vec<String> = std::env::args().skip(1).collect();
    if packs.is_empty() {
        packs = ROSTER.map(str::to_owned).to_vec();
    }
    for pack in packs {
        let data = match tf2_core::character::load(Path::new(&game), &pack) {
            Ok(d) => d,
            Err(e) => {
                println!("== {pack}: {e}");
                continue;
            }
        };
        println!("== {pack}");
        let mut bad = audit("robot", &data.robot, &data.library);
        if let Some(vehicle) = &data.vehicle {
            bad += audit("vehicle", vehicle, &data.library);
        }
        for (i, object) in data.weapon_objects.iter().enumerate() {
            if let Some(object) = object {
                bad += audit(&format!("weapon{i}"), object, &data.library);
            }
        }
        println!("  {bad} skinned part(s) with a palette problem");
    }
}
