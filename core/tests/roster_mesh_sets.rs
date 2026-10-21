//! The roster's mesh and material sets (`core\examples\mesh_audit.rs` is the read-out): for every
//! playable robot, robot and vehicle object, each RenderSetEnabler link resolves to one drawn mesh,
//! every mesh is in the pack's library, every part of it has a material, and every texture the
//! materials name is in the pack. Needs the install.

use tf2_core::formats::hash::lower33;
use tf2_core::formats::model::{Library, ParamValue};
use tf2_core::formats::scene::ObjectDef;

/// The 13 robots and the 5 reskins (which use their base's attribute block and bank [data]).
const ROSTER: [&str; 18] = [
    "bumblebee", "optimus", "ironhide", "ratchet", "megatron", "longhaul", "audir8", "jetfire", "firestorm",
    "starscream", "blackout", "optimusflight", "megatronflight",
    "ironhidepaintover", "optimusgold", "starscreampaintover", "megatrongold", "longhaulyellow",
];

fn check(pack: &str, label: &str, object: &ObjectDef, library: &Library) {
    for group in &object.render_sets {
        for (i, set) in group.sets.iter().enumerate() {
            let found = object.renders.iter().filter(|r| group.contains_model(&r.model, i)).count();
            assert_eq!(found, set.nodes.len(), "{pack} {label}: set {:08x} links", set.name);
        }
    }
    for render in &object.renders {
        let mesh = library.meshes.get(&lower33(&render.model)).unwrap_or_else(|| panic!("{pack} {label}: {} not in the library", render.model));
        for part in &mesh.parts {
            let material = part
                .material
                .and_then(|id| library.materials.get(&id))
                .unwrap_or_else(|| panic!("{pack} {label}: a part of {} has no material", render.model));
            for param in &material.params {
                if let ParamValue::Texture(Some(id)) = &param.value {
                    assert!(
                        library.textures.contains_key(id) || library.texture_frames.contains_key(id),
                        "{pack} {label}: {} names texture {id:08x}, not in the pack",
                        material.name
                    );
                }
            }
        }
    }
}

#[test]
fn every_robot_has_complete_mesh_sets_and_materials() {
    let dir = std::path::PathBuf::from(tf2_core::install::dir_string());
    if !dir.join("characters/bumblebee.str").exists() {
        return;
    }
    for pack in ROSTER {
        let data = tf2_core::character::load(&dir, pack).unwrap();
        check(pack, "robot", &data.robot, &data.library);
        if let Some(vehicle) = &data.vehicle {
            check(pack, "vehicle", vehicle, &data.library);
        }
    }
}
