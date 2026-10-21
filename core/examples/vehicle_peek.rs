//! A character's vehicle object under one of its vehicle clips: every node's place at rest and
//! posed (object space), whether the clip moves it, and how many skinned vertices lean on it.
//! Nodes that end up far from their rest are flagged. Read-only.
//! `cargo run -p tf2-core --example vehicle_peek -- <pack> [clip id] [tick]`
//! With no clip id it lists the vehicle clips.

use std::collections::HashMap;
use std::path::Path;

use tf2_core::formats::hash::lower33;
use tf2_core::pose::Rig;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    let pack = args.first().map_or("megatron", String::as_str);
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("load");
    let vehicle = data.vehicle.as_ref().expect("no vehicle object");
    let Some(id) = args.get(1) else {
        for r in &data.vehicle_animations {
            let length = r.clip.as_ref().map_or(0.0, |c| c.length);
            println!("{:<28} looping {:<5} length {length:6.1} channels {}", r.id, r.looping, r.clip.as_ref().map_or(0, |c| c.channels.len()));
        }
        return;
    };
    let tick: f32 = args.get(2).map_or(0.0, |t| t.parse().unwrap());
    let r = data.vehicle_animations.iter().find(|r| &r.id == id).expect("clip");
    let clip = r.clip.as_ref().expect("clip body");
    // Vertices per node: every skinned part's palette entry -> bone -> node by bone hash.
    let mut weight_count: HashMap<usize, usize> = HashMap::new();
    for render in &vehicle.renders {
        let Some(mesh) = data.library.meshes.get(&lower33(&render.model)) else { continue };
        let Some(bones) = mesh.skeleton.and_then(|id| data.library.skeletons.get(&id)) else { continue };
        for part in &mesh.parts {
            for (indices, weights) in part.blend_indices.iter().zip(&part.blend_weights) {
                for k in 0..4 {
                    if weights[k] <= 0.0 {
                        continue;
                    }
                    let Some(&bone) = part.palette.get(indices[k] as usize) else { continue };
                    let Some(bone) = bones.get(bone as usize) else { continue };
                    if let Some(node) = vehicle.nodes.iter().position(|n| n.bone_hash == bone.name) {
                        *weight_count.entry(node).or_default() += 1;
                    }
                }
            }
        }
    }
    // With a fourth argument: the clip's own keys at `tick` for the nodes whose names hold it,
    // beside the node's rest relative to its parent.
    if let Some(filter) = args.get(3) {
        println!("stream kinds: {:?}", clip.stream_kinds);
        let bind: Vec<_> = vehicle.nodes.iter().map(|n| tf2_core::pose::affine(&n.bind)).collect();
        for (i, n) in vehicle.nodes.iter().enumerate().filter(|(_, n)| n.name.contains(filter.as_str())) {
            let local = n.parent.map_or(bind[i], |p| bind[p].inverse() * bind[i]);
            let (s, q, t) = local.to_scale_rotation_translation();
            println!("{i:3} {:<20} rest  t ({:6.2} {:6.2} {:6.2}) q ({:6.3} {:6.3} {:6.3} {:6.3}) s ({:5.2} {:5.2} {:5.2})", n.name, t.x, t.y, t.z, q.x, q.y, q.z, q.w, s.x, s.y, s.z);
            let Some(channel) = clip.channel(n.name_hash) else { continue };
            let p = channel.position.sample(tick, clip.length, r.looping);
            let q = channel.rotation.sample(tick, clip.length, r.looping);
            let s = channel.scale.sample(tick, clip.length, r.looping);
            println!("    {:<20} clip  t {p:?} q {q:?} s {s:?}", "");
        }
        return;
    }
    let rig = Rig::new(vehicle);
    let (pose, rest) = (rig.pose(clip, r.looping, tick), rig.rest());
    for (i, n) in vehicle.nodes.iter().enumerate() {
        let (m, b) = (pose[i].translation, rest[i].translation);
        let far = (m - b).length();
        println!(
            "{i:3} {:<28} parent {:<4} channel {:<5} skinned {:<6} rest ({:7.2} {:7.2} {:7.2}) posed ({:7.2} {:7.2} {:7.2}){}",
            n.name,
            n.parent.map_or("-".into(), |p| p.to_string()),
            clip.channel(n.name_hash).is_some(),
            weight_count.get(&i).copied().unwrap_or(0),
            b.x, b.y, b.z, m.x, m.y, m.z,
            if far > 2.0 { format!("  MOVED {far:.1}") } else { String::new() }
        );
    }
}
