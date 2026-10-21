//! A robot's weapons' muzzle effects and the nodes they start on: per weapon row, its muzzle
//! template's elements (local space or not, the renderer's facing, the emit actions), and for each
//! weapon model the `MuzzleFlashDummy` / `FireDummy*` nodes with their bind axes in the model's
//! frame (which way a barrel points). Read-only.
//! `cargo run -p tf2-core --example muzzle_peek -- <pack>`

use std::path::Path;

fn main() {
    let pack = std::env::args().nth(1).expect("pack");
    let game = tf2_core::install::dir_string();
    let data = tf2_core::character::load(Path::new(&game), &pack).expect("character");
    let names: std::collections::HashMap<u32, String> = std::fs::read_to_string(r"C:\Bumblebee\export\data\names.tsv")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (h, n) = l.split_once('\t')?;
            Some((u32::from_str_radix(h, 16).ok()?, n.to_owned()))
        })
        .collect();
    let name = |hash: u32| names.get(&hash).cloned().unwrap_or(format!("{hash:08x}"));
    for w in &data.weapon_effects {
        println!("== weapon {}", name(w.name));
        let Some(t) = &w.muzzle_flash else {
            println!("   no muzzle template");
            continue;
        };
        println!("   muzzle {} ({} elements)", name(t.name), t.elements.len());
        for (i, e) in t.elements.iter().enumerate() {
            println!(
                "   [{i}] local {} max {} {:.2}..{:.2} renderer normal {:?} velocity_tracked {} texture {:?}",
                e.local_space, e.max_particles, e.start, e.end, e.renderer.normal, e.renderer.velocity_tracked, e.renderer.texture_path
            );
            for a in &e.actions {
                use tf2_core::particles::Kind;
                match &a.kind {
                    Kind::Burst { slots, .. } | Kind::Source { slots, .. } | Kind::Eject { slots, .. } => {
                        println!("       emits: place {:?} velocity {:?}", slots[0], slots[11]);
                    }
                    other => {
                        let text = format!("{other:?}");
                        println!("       {}", &text[..text.len().min(120)]);
                    }
                }
            }
        }
    }
    // In the weapon stance: where each weapon's dummies point once its model hangs on its bone, and
    // which way that bone's forearm runs (bone place less its parent's).
    let stance = data.animations.find("WeaponSet", "Weapon_Idle");
    let pose = stance.and_then(|r| Some(tf2_core::pose::Rig::new(&data.robot).pose(r.clip.as_ref()?, r.looping, 0.0)));
    for (i, (object, def)) in data.weapon_objects.iter().zip(&data.weapons).enumerate() {
        let (Some(object), Some(pose)) = (object, pose.as_ref()) else { continue };
        let Some(bone) = data.robot.nodes.iter().position(|n| n.name_hash == def.bone) else { continue };
        let parent = data.robot.nodes[bone].parent.unwrap_or(bone);
        let arm = (pose[bone].translation - pose[parent].translation).normalize_or_zero();
        println!(
            "stance: weapon {i} ({}) on {} (arm along {:?}, bone determinant {:.2})",
            name(def.name), data.robot.nodes[bone].name, glam::Vec3::from(arm), pose[bone].matrix3.determinant()
        );
        for (n, node) in object.nodes.iter().enumerate() {
            let keyed: Vec<&str> = data.weapon_animations.get(i).into_iter().flatten()
                .filter(|r| r.clip.as_ref().is_some_and(|c| c.channel(node.name_hash).is_some()))
                .map(|r| r.id.as_str())
                .collect();
            println!("    node {n} {} parent {:?} keyed by {:?}", node.name, node.parent, keyed);
        }
        for (n, node) in object.nodes.iter().enumerate() {
            if node.name.contains("Muzzle") || node.name.contains("FireDummy") {
                let world = pose[bone] * tf2_core::pose::affine(&node.bind);
                let (x, y) = (glam::Vec3::from(world.matrix3.x_axis).normalize(), glam::Vec3::from(world.matrix3.y_axis).normalize());
                println!(
                    "    {} x {x:.2?} (along the arm {:.2}) y {y:.2?} (along the arm {:.2})",
                    node.name, x.dot(arm.into()), y.dot(arm.into())
                );
            }
        }
    }
    for (i, object) in data.weapon_objects.iter().enumerate() {
        let Some(object) = object else { continue };
        let bind: Vec<_> = object.nodes.iter().map(|n| tf2_core::pose::affine(&n.bind)).collect();
        for (n, node) in object.nodes.iter().enumerate() {
            if node.name.contains("Muzzle") || node.name.contains("FireDummy") {
                let m = bind[n];
                println!(
                    "weapon object {i} ({}) node {n} {} at {:?} x {:?} y {:?} z {:?}",
                    data.weapons.get(i).map_or(String::new(), |w| name(w.name)),
                    node.name, m.translation, m.matrix3.x_axis, m.matrix3.y_axis, m.matrix3.z_axis
                );
                // The node as the weapon's own clips key it (its turn and place relative to its parent).
                for r in data.weapon_animations.get(i).into_iter().flatten() {
                    let Some(clip) = r.clip.as_ref() else { continue };
                    let Some(c) = clip.channel(node.name_hash) else { continue };
                    let q = c.rotation.sample(0.0, clip.length, r.looping);
                    let t = c.position.sample(0.0, clip.length, r.looping);
                    let x = q.map(|q| tf2_core::pose::stored_turn(q) * glam::Vec3::X);
                    let y = q.map(|q| tf2_core::pose::stored_turn(q) * glam::Vec3::Y);
                    println!("      clip {}: t {t:?} x {x:?} y {y:?}", r.id);
                }
            }
        }
    }
}
