//! Each character's `FireDummy` nodes (the barrels a special weapon's rounds leave from,
//! `007a7a40` matches them by name in the owner's node list), with their rest place.
//! `cargo run -p tf2-core --example fire_dummies -- [pack...]`

use std::path::Path;

fn main() {
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    let mut packs: Vec<String> = std::env::args().skip(1).collect();
    if packs.is_empty() {
        packs = ["megatron", "megatronflight", "jetfire", "audir8", "blackout", "ironhide"].map(str::to_owned).to_vec();
    }
    for pack in packs {
        let data = tf2_core::character::load(Path::new(&dir), &pack).expect("load");
        let found: Vec<String> = data
            .robot
            .nodes
            .iter()
            .filter(|n| n.name.starts_with("FireDummy") || n.name.starts_with("FX_") && n.name.contains("Fire"))
            .map(|n| {
                // The bind matrix's translation: the node's rest place in the object. [data]
                let b = &n.bind;
                format!("{} ({:.2}, {:.2}, {:.2})", n.name, b[3][0], b[3][1], b[3][2])
            })
            .collect();
        println!("{pack:15} robot: {}", if found.is_empty() { "none".to_owned() } else { found.join("; ") });
        for (def, object) in data.weapons.iter().zip(&data.weapon_objects) {
            let Some(object) = object else { continue };
            let dummies: Vec<String> = object
                .nodes
                .iter()
                .filter(|n| n.name.to_ascii_lowercase().contains("firedummy") || n.name.to_ascii_lowercase().contains("muzzle"))
                .map(|n| format!("{} ({:.2}, {:.2}, {:.2})", n.name, n.bind[3][0], n.bind[3][1], n.bind[3][2]))
                .collect();
            println!("    weapon {:#010x} {:?}: {} nodes; {}", def.name, def.kind, object.nodes.len(), dummies.join("; "));
        }
    }
}
