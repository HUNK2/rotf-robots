//! The flight control-surface and rotor nodes (`flightsurface::NODES`) in a character's vehicle
//! and robot objects: parent, place relative to the parent, and the bind axes. For checking which
//! flyers carry the nodes 0086cbf0 turns, and how they hang.
//! `cargo run -p tf2-core --example surface_nodes -- <pack>...`

use std::path::Path;

fn main() {
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    for pack in std::env::args().skip(1) {
        let data = match tf2_core::character::load(Path::new(&dir), &pack) {
            Ok(d) => d,
            Err(e) => {
                println!("{pack}: {e}");
                continue;
            }
        };
        for (label, object) in [("robot", Some(&data.robot)), ("vehicle", data.vehicle.as_ref())] {
            let Some(object) = object else { continue };
            let found: Vec<_> = object.nodes.iter().enumerate()
                .filter(|(_, n)| tf2_core::flightsurface::NODES.iter().any(|s| s.eq_ignore_ascii_case(&n.name)))
                .collect();
            println!("{pack} {label}: {} of {} nodes", found.len(), object.nodes.len());
            for (_, n) in found {
                let b = n.bind;
                let parent = n.parent.map(|p| &object.nodes[p]);
                let local = parent.map(|p| {
                    let d = [b[3][0] - p.bind[3][0], b[3][1] - p.bind[3][1], b[3][2] - p.bind[3][2]];
                    // Into the parent's axes (rows of its bind).
                    let a = p.bind;
                    [0, 1, 2].map(|i| a[i][0] * d[0] + a[i][1] * d[1] + a[i][2] * d[2])
                });
                println!(
                    "  {:20} parent {:20} local {:?}  x {:?} y {:?} z {:?}",
                    n.name,
                    parent.map_or("-", |p| p.name.as_str()),
                    local.map(|l| l.map(|v| (v * 100.0).round() / 100.0)),
                    b[0].map(|v| (v * 100.0).round() / 100.0),
                    b[1].map(|v| (v * 100.0).round() / 100.0),
                    b[2].map(|v| (v * 100.0).round() / 100.0),
                );
            }
        }
        // Which vehicle clips drive these nodes (a clip channel would overwrite the BasePose
        // row the game composes the rotors onto), and which blades the model has.
        let hashes: Vec<u32> = tf2_core::flightsurface::NODES.iter().map(|n| tf2_core::formats::hash::crc32(n.as_bytes())).collect();
        for r in &data.vehicle_animations {
            let Some(clip) = &r.clip else { continue };
            let driven: Vec<_> = tf2_core::flightsurface::NODES.iter().zip(&hashes)
                .filter(|(_, h)| clip.names.contains(h)).map(|(n, _)| *n).collect();
            if !driven.is_empty() {
                println!("  clip {} drives {:?}", r.id, driven);
            }
        }
        let name: String = pack[..1].to_uppercase() + &pack[1..];
        let forms = tf2_core::flight::Forms::load(Path::new(&dir), &name).ok();
        if let (Some(forms), Some(vehicle)) = (forms, data.vehicle.as_ref()) {
            let hashes: Vec<u32> = vehicle.nodes.iter().map(|n| n.name_hash).collect();
            let parents: Vec<_> = vehicle.nodes.iter().map(|n| n.parent).collect();
            let reached = tf2_core::flightsurface::blade_nodes(&forms.fly.blade_list, &hashes, &parents);
            let found = forms.fly.blade_list.iter().filter(|b| hashes.contains(b)).count();
            let meshes = vehicle.renders.iter().filter(|r| reached[r.node]).count();
            println!("  bladeList {} names, {} on the model, {} nodes and {} renders reached", forms.fly.blade_list.len(), found, reached.iter().filter(|r| **r).count(), meshes);
        }
    }
}
