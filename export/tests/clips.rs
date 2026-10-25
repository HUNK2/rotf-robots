//! Writes Bumblebee and plays every animation of every file back as a glTF player does
//! (straight-line blends, spherical for turns): the robot's, the vehicle's, the weapons' and the
//! axe's. Each node's place is compared with the game's own posing (`tf2_core::pose::Rig`, the
//! arithmetic the melee hit bodies are placed by, which the game's drawing shares for every object)
//! at the clip's start, a third of the way in (between ticks), two thirds and the end. Within a
//! millimetre, between ticks too (the exporter fills fast turns with the game's own blend), with
//! room for float rounding on parts parked far off (`allowed`). `ROTF_TEST_CHARACTER` runs it on
//! another character. Reads the install; skips without it.

mod common;

use std::collections::HashMap;

use glam::{Affine3A, Mat4, Quat, Vec3};
use gltf::animation::util::ReadOutputs;

/// One animated property of one node: key times in seconds and values.
struct Curve {
    times: Vec<f32>,
    values: Vec<[f32; 4]>,
}

impl Curve {
    fn at(&self, t: f32, turn: bool) -> [f32; 4] {
        let next = self.times.partition_point(|&k| k <= t);
        if next == 0 {
            return self.values[0];
        }
        if next == self.times.len() {
            return self.values[next - 1];
        }
        let (t0, t1) = (self.times[next - 1], self.times[next]);
        let s = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
        let (a, b) = (self.values[next - 1], self.values[next]);
        if turn {
            Quat::from_array(a).slerp(Quat::from_array(b), s).to_array()
        } else {
            std::array::from_fn(|k| a[k] + (b[k] - a[k]) * s)
        }
    }
}

#[test]
fn bumblebee_animations_pose_every_node_as_the_game_does() {
    let Some(export) = common::export("bumblebee", "clips") else { return };
    // On whole ticks [0] and halfway between them [1]: the worst place error against what it is
    // allowed (`allowed`), the error and where; the worst axis error and where.
    let mut worst_place = [(0.0f32, 0.0f32, String::new()), (0.0f32, 0.0f32, String::new())];
    let mut worst_axis = [(0.0f32, String::new()), (0.0f32, String::new())];
    let mut compared = 0;
    let mut played = 0;
    let mut skipped = 0;
    for piece in export.pieces().iter().filter(|p| !p.clips.is_empty()) {
        let file = format!("{}.glb", piece.file);
        let rig = tf2_core::pose::Rig::new(piece.object);
        let n = piece.object.nodes.len();
        // A node whose rest matrix is sheared, and its children: the nearest turn is a quaternion
        // off unit length, which the port's rig uses as it is and glTF must normalise, so their
        // axes differ by up to the shear (Ratchet's FX_overHeat: 0.002). Their places are compared.
        let bind: Vec<Mat4> = piece.object.nodes.iter().map(|node| rotf_export::model::matrix(&node.bind)).collect();
        let mut sheared = vec![false; n];
        for (i, node) in piece.object.nodes.iter().enumerate() {
            let local = node.parent.map_or(bind[i], |p| bind[p].inverse() * bind[i]);
            let (s, r, t) = local.to_scale_rotation_translation();
            let own = !Mat4::from_scale_rotation_translation(s, r, t).abs_diff_eq(local, 1e-3);
            sheared[i] = own || node.parent.is_some_and(|p| p < i && sheared[p]);
        }
        skipped += sheared.iter().filter(|&&s| s).count();
        let glb = export.read(&file);
        let doc = &glb.document;
        let rest = glb.rest();
        let parent = glb.parents();
        let animations: HashMap<String, gltf::Animation> = doc.animations().map(|a| (a.name().unwrap().to_owned(), a)).collect();
        let mut seen = std::collections::HashSet::new();
        let doubled: Vec<&str> = piece.clips.iter().filter(|c| !seen.insert(&c.name)).map(|c| c.name.as_str()).collect();
        // A clip is left out only when it moves no node of the object.
        let moves = |clip: &rotf_export::model::ClipOut| {
            piece.object.nodes.iter().any(|node| clip.clip.channel(node.name_hash).is_some_and(|c| c.keys_any()))
        };
        let missing: Vec<&str> =
            piece.clips.iter().filter(|c| !animations.contains_key(&c.name) && moves(c)).map(|c| c.name.as_str()).collect();
        assert!(doubled.is_empty() && missing.is_empty(), "{file}: named twice {doubled:?}, not written {missing:?}");

        for clip in piece.clips.iter().filter(|c| animations.contains_key(&c.name)) {
            played += 1;
            let animation = &animations[&clip.name];
            // (node, path) -> curve; path 0 translation, 1 rotation, 2 scale.
            let mut curves: HashMap<(usize, u8), Curve> = HashMap::new();
            for channel in animation.channels() {
                let reader = channel.reader(|_| Some(&glb.blob));
                let times: Vec<f32> = reader.read_inputs().unwrap().collect();
                let (path, values): (u8, Vec<[f32; 4]>) = match reader.read_outputs().unwrap() {
                    ReadOutputs::Translations(v) => (0, v.map(|p| [p[0], p[1], p[2], 0.0]).collect()),
                    ReadOutputs::Rotations(v) => (1, v.into_f32().collect()),
                    ReadOutputs::Scales(v) => (2, v.map(|p| [p[0], p[1], p[2], 0.0]).collect()),
                    ReadOutputs::MorphTargetWeights(_) => continue,
                };
                curves.insert((channel.target().node().index(), path), Curve { times, values });
            }
            let length = clip.clip.length;
            // A looping clip whose last key sits on its end tick wraps to its first key at that very
            // instant in the game, where a glTF player shows the last key and then starts over: the
            // same jump, a tick apart. Its end is compared half a tick before.
            let end = if clip.looping { (length - 0.5).max(0.0) } else { length };
            for tick in [0.0, (length / 3.0).floor() + 0.5, (length * 2.0 / 3.0).floor(), end] {
                let seconds = tick / 60.0;
                let local = |i: usize| {
                    let (mut t, mut r, mut s) = rest[i];
                    if let Some(c) = curves.get(&(i, 0)) {
                        t = Vec3::from_slice(&c.at(seconds, false)[..3]);
                    }
                    if let Some(c) = curves.get(&(i, 1)) {
                        r = Quat::from_array(c.at(seconds, true));
                    }
                    if let Some(c) = curves.get(&(i, 2)) {
                        s = Vec3::from_slice(&c.at(seconds, false)[..3]);
                    }
                    Mat4::from_scale_rotation_translation(s, r, t)
                };
                let game: Vec<Affine3A> = if clip.additive {
                    rig.pose_layer(None, clip.clip, clip.looping, tick, true, 1.0)
                } else {
                    rig.pose(clip.clip, clip.looping, tick)
                };
                for (i, expected) in game.iter().enumerate().take(n) {
                    let got = glb.world(1 + i, &parent, &local);
                    let place = (got.w_axis.truncate() - Vec3::from(expected.translation)).length();
                    let axis = (0..3)
                        .map(|k| (got.col(k).truncate() - Vec3::from(expected.matrix3.col(k))).length())
                        .fold(0.0f32, f32::max);
                    compared += 1;
                    let half = usize::from(tick.fract() != 0.0);
                    let at = || format!("{file} {} tick {tick} node {}", clip.name, piece.object.nodes[i].name);
                    let ratio = place / allowed(Vec3::from(expected.translation).length());
                    if ratio > worst_place[half].0 {
                        worst_place[half] = (ratio, place, at());
                    }
                    if axis > worst_axis[half].0 && !sheared[i] {
                        worst_axis[half] = (axis, at());
                    }
                }
            }
        }
        println!("{file}: {} animations", animations.len());
    }
    println!("{played} animations, {compared} node places compared; axes not compared on {skipped} sheared nodes");
    for (k, label) in ["whole ticks", "half ticks"].iter().enumerate() {
        let ((ratio, place, at), (axis, axis_at)) = (&worst_place[k], &worst_axis[k]);
        println!("  {label}: worst place {place:.2e} m, {:.0}% of allowed ({at}); worst axis {axis:.2e} ({axis_at})", ratio * 100.0);
        assert!(*ratio < 1.0, "a node is {place} m from the game's place ({at})");
        assert!(*axis < 1e-3, "a node's axes are {axis} off the game's ({axis_at})");
    }
}

/// The place error allowed a node `distance` from the object's root: a millimetre, and a fiftieth
/// of a millimetre per metre beyond, for the float rounding of parts the game parks far off
/// (Starscream's thrust flaps sit 2 to 3 km out during his changes of form).
fn allowed(distance: f32) -> f32 {
    1e-3 + 2e-5 * distance
}
