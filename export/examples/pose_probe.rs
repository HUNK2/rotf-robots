//! Where an exported animation and the game's pose part: for one node of one written file, at one
//! tick, every node of its chain with the game's local place, turn and scale (`Channel::local`,
//! the root rule of `pose::Rig`) beside the glTF's (its keys played back, spherical for turns).
//! `cargo run -p rotf-export --example pose_probe -- <pack> <file stem: robot|vehicle> <animation> <tick> <node>`

use std::collections::HashMap;

use glam::{Quat, Vec3};
use gltf::animation::util::ReadOutputs;
use tf2_core::formats::anim::ROOT_CHANNEL;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (pack, which, wanted, tick, node_name) = (&a[0], &a[1], &a[2], a[3].parse::<f32>().unwrap(), &a[4]);
    let install = tf2_core::install::dir();
    let roster = tf2_core::formats::characterlist::CharacterList::load(install).unwrap();
    let spec = rotf_export::spec(&roster, pack);
    let out = std::env::temp_dir().join("rotf-pose-probe");
    rotf_export::export(install, &spec, &out).unwrap();
    let data = tf2_core::character::load(install, &spec.pack).unwrap();
    let names = rotf_export::names::Names::load(install, rotf_export::names::data_files(&spec.pack_file(install)), &[]);
    let pieces = rotf_export::pieces(&data, &spec.pack, &names);
    let piece = pieces.iter().find(|p| p.file == format!("{}_{which}", spec.pack)).expect("no such file");
    let clip = piece.clips.iter().find(|c| &c.name == wanted).expect("no such animation");
    let object = piece.object;

    let bytes = std::fs::read(out.join(format!("{}.glb", piece.file))).unwrap();
    let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
    let blob = gltf.blob.as_deref().unwrap();
    let animation = gltf.document.animations().find(|x| x.name() == Some(wanted)).unwrap();
    let mut curves: HashMap<(usize, u8), (Vec<f32>, Vec<[f32; 4]>)> = HashMap::new();
    for channel in animation.channels() {
        let reader = channel.reader(|_| Some(blob));
        let times: Vec<f32> = reader.read_inputs().unwrap().map(|t| t * 60.0).collect();
        let (path, values): (u8, Vec<[f32; 4]>) = match reader.read_outputs().unwrap() {
            ReadOutputs::Translations(v) => (0, v.map(|p| [p[0], p[1], p[2], 0.0]).collect()),
            ReadOutputs::Rotations(v) => (1, v.into_f32().collect()),
            ReadOutputs::Scales(v) => (2, v.map(|p| [p[0], p[1], p[2], 0.0]).collect()),
            ReadOutputs::MorphTargetWeights(_) => continue,
        };
        curves.insert((channel.target().node().index(), path), (times, values));
    }
    let at = |i: usize, path: u8| -> Option<([f32; 4], (f32, f32))> {
        let (times, values) = curves.get(&(1 + i, path))?;
        let next = times.partition_point(|&k| k <= tick);
        if next == 0 || next == times.len() {
            let k = next.saturating_sub(1);
            return Some((values[k], (times[k], times[k])));
        }
        let (t0, t1) = (times[next - 1], times[next]);
        let s = ((tick - t0) / (t1 - t0)).clamp(0.0, 1.0);
        let (x, y) = (values[next - 1], values[next]);
        let v = if path == 1 { Quat::from_array(x).slerp(Quat::from_array(y), s).to_array() } else { std::array::from_fn(|k| x[k] + (y[k] - x[k]) * s) };
        Some((v, (t0, t1)))
    };
    let mut chain = Vec::new();
    let mut i = object.nodes.iter().position(|n| &n.name == node_name).expect("no such node");
    loop {
        chain.push(i);
        match object.nodes[i].parent {
            Some(p) => i = p,
            None => break,
        }
    }
    chain.reverse();
    let rig = tf2_core::pose::Rig::new(object);
    let world = rig.pose(clip.clip, clip.looping, tick);
    let local = |i: usize| -> glam::Mat4 {
        let t = at(i, 0).map_or(Vec3::ZERO, |(v, _)| Vec3::new(v[0], v[1], v[2]));
        let r = at(i, 1).map_or(Quat::IDENTITY, |(v, _)| Quat::from_array(v));
        let s = at(i, 2).map_or(Vec3::ONE, |(v, _)| Vec3::new(v[0], v[1], v[2]));
        glam::Mat4::from_scale_rotation_translation(s, r, t)
    };
    let mut gltf_world = glam::Mat4::IDENTITY;
    for &i in &chain {
        // Nodes the animation does not touch keep the file's rest.
        let node = gltf.document.nodes().nth(1 + i).unwrap();
        let (t, r, s) = node.transform().decomposed();
        let rest = glam::Mat4::from_scale_rotation_translation(Vec3::from(s), Quat::from_array(r), Vec3::from(t));
        let has = (0..3).any(|p| curves.contains_key(&(1 + i, p)));
        if has {
            // Per-path: a path the animation lacks keeps the rest's.
            let (rs, rr, rt) = rest.to_scale_rotation_translation();
            let t = at(i, 0).map_or(rt, |(v, _)| Vec3::new(v[0], v[1], v[2]));
            let r = at(i, 1).map_or(rr, |(v, _)| Quat::from_array(v));
            let s = at(i, 2).map_or(rs, |(v, _)| Vec3::new(v[0], v[1], v[2]));
            gltf_world *= glam::Mat4::from_scale_rotation_translation(s, r, t);
        } else {
            gltf_world *= rest;
        }
        let g = glam::Mat4::from(world[i]);
        let axis = (0..3).map(|k| (gltf_world.col(k) - g.col(k)).truncate().length()).fold(0.0f32, f32::max);
        println!("{}: world axis difference {axis:.2e}, place {:.2e}", object.nodes[i].name, (gltf_world.w_axis - g.w_axis).truncate().length());
    }
    let _ = local;
    for i in chain {
        let node = &object.nodes[i];
        let game = clip.clip.channel(node.name_hash).and_then(|c| c.local(tick, clip.clip.length, clip.looping));
        println!("{} ({i}){}", node.name, if node.name_hash == ROOT_CHANNEL { " root" } else { "" });
        if let Some((p, q, s)) = game {
            let q = Quat::from_xyzw(-q[0], -q[1], -q[2], q[3]);
            println!("   game  t {:?} r {:?} s {:?}", Vec3::from(p), q, Vec3::from(s));
        } else {
            println!("   game  rest");
        }
        for (path, label) in [(0u8, "t"), (1, "r"), (2, "s")] {
            if let Some((v, span)) = at(i, path) {
                println!("   gltf  {label} {:?} between ticks {:.4}..{:.4}", &v[..if path == 1 { 4 } else { 3 }], span.0, span.1);
            }
        }
        if let Some(c) = clip.clip.channel(node.name_hash) {
            let near = |times: &[f32]| times.iter().copied().filter(|t| (t - tick).abs() < 3.0).collect::<Vec<_>>();
            println!("   keys near: t {:?} r {:?} s {:?} own {:?}", near(c.position.times()), near(c.rotation.times()), near(c.scale.times()), near(c.own_scale.times()));
        }
    }
    let i = object.nodes.iter().position(|n| &n.name == node_name).unwrap();
    println!("game world place of {node_name}: {:?} ({:.1} m from the root)", world[i].translation, Vec3::from(world[i].translation).length());
    let _ = std::fs::remove_dir_all(&out);
}
