//! A character's weapon objects: each node's bind place and parent, and each render's node, mesh
//! and bounding sphere (centre in the mesh's own space), so a muzzle dummy can be set against the
//! barrel it should sit at. For Ironhide's left cannon smoke (the user's report, 2026-10-25).
//! `cargo run -p tf2-core --example weapon_model_bounds -- <pack> [weapon index...]`

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pack = args.first().expect("pack");
    let only: Vec<usize> = args[1..].iter().filter_map(|a| a.parse().ok()).collect();
    let game = tf2_core::install::dir_string();
    let data = tf2_core::character::load(Path::new(&game), pack).expect("character");
    for (i, object) in data.weapon_objects.iter().enumerate() {
        if !only.is_empty() && !only.contains(&i) {
            continue;
        }
        let Some(object) = object else { continue };
        let name = data.weapons.get(i).map_or(0, |w| w.name);
        println!("== weapon object {i} ({name:#010x})");
        for (n, node) in object.nodes.iter().enumerate() {
            let b = node.bind;
            println!("  node {n} {} parent {:?} at ({:.2}, {:.2}, {:.2}) x ({:.2}, {:.2}, {:.2})", node.name, node.parent,
                b[3][0], b[3][1], b[3][2], b[0][0], b[0][1], b[0][2]);
        }
        // With `POSE=<clip>@<tick>`: each node's place and scale in the object as the port's rig poses it.
        if let Ok(pose) = std::env::var("POSE") {
            let (clip_id, tick) = pose.split_once('@').map_or((pose.as_str(), 0.0), |(c, t)| (c, t.parse().unwrap_or(0.0)));
            let clips = data.weapon_animations.get(i).map_or(&[][..], Vec::as_slice);
            if let Some(clip) = clips.iter().find(|r| r.id == clip_id).and_then(|r| r.clip.as_ref()) {
                let rig = tf2_core::pose::Rig::new(object);
                let (rest, posed) = (rig.rest(), rig.pose(clip, false, tick));
                // The other rule: a keyed node's unkeyed scale is 1 (the game's output start, 0053b520).
                let mut unit = Vec::<glam::Affine3A>::new();
                for (n, node) in object.nodes.iter().enumerate() {
                    let b = node.bind;
                    let m = glam::Mat3::from_cols(glam::Vec3::from(b[0]), glam::Vec3::from(b[1]), glam::Vec3::from(b[2]));
                    let parent_inv = node.parent.map(|p| {
                        let pb = object.nodes[p].bind;
                        glam::Affine3A::from_mat3_translation(glam::Mat3::from_cols(pb[0].into(), pb[1].into(), pb[2].into()), pb[3].into()).inverse()
                    }).unwrap_or(glam::Affine3A::IDENTITY);
                    let local = parent_inv * glam::Affine3A::from_mat3_translation(m, b[3].into());
                    let (mut scale, mut rotation, mut position) = local.to_scale_rotation_translation();
                    if let Some(channel) = clip.channel(node.name_hash) {
                        if let Some(p) = channel.position.sample(tick, clip.length, false).filter(|_| n != 0) { position = p.into(); }
                        if let Some(q) = channel.rotation.sample(tick, clip.length, false).filter(|_| n != 0) { rotation = glam::Quat::from_xyzw(-q[0], -q[1], -q[2], q[3]); }
                        scale = channel.scale.sample(tick, clip.length, false).map_or(glam::Vec3::ONE, Into::into);
                    }
                    let l = glam::Affine3A::from_scale_rotation_translation(scale, rotation, position);
                    let world = node.parent.map_or(l, |p| unit[p] * l);
                    unit.push(world);
                }
                for (n, node) in object.nodes.iter().enumerate() {
                    let keyed = clip.channel(node.name_hash).is_some();
                    let (a, b, c) = (rest[n], posed[n], unit[n]);
                    println!("  {clip_id}@{tick} node {n} {}{}: rest ({:.2}, {:.2}, {:.2}) s{:.2} -> ({:.2}, {:.2}, {:.2}) s{:.2}; scale-1 rule ({:.2}, {:.2}, {:.2}) s{:.2}", node.name,
                        if keyed { " keyed" } else { "" }, a.translation.x, a.translation.y, a.translation.z, a.matrix3.x_axis.length(),
                        b.translation.x, b.translation.y, b.translation.z, b.matrix3.x_axis.length(),
                        c.translation.x, c.translation.y, c.translation.z, c.matrix3.x_axis.length());
                }
            } else {
                println!("  no clip {clip_id} (clips: {:?})", clips.iter().map(|r| &r.id).collect::<Vec<_>>());
            }
        }
        for r in &object.renders {
            let hash = tf2_core::formats::hash::lower33(&r.model);
            let mesh = data.library.meshes.get(&hash);
            println!("  render on node {} {}: {}", r.node, r.model, mesh.map_or("mesh not in the library".to_owned(), |m| format!(
                "sphere ({:.2}, {:.2}, {:.2}) r {:.2}, half ({:.2}, {:.2}, {:.2}), skinned {}", m.sphere[0], m.sphere[1], m.sphere[2], m.sphere[3],
                m.half_extents[0], m.half_extents[1], m.half_extents[2], m.skeleton.is_some())));
        }
    }
}
