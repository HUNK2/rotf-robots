//! Which clips of the playable robots key scales, and of which kind: the ordinary scale (stream
//! types 3 and 0x41, which children take) and the node's own scale (types 4 and 0x42, which they
//! do not; `formats::anim::Channel::own_scale`). Prints, per pack and clip list, the clips with a
//! key away from one and the nodes it touches. Read-only.
//! `cargo run -p tf2-core --example scale_survey -- [pack...]`

use std::path::Path;

use tf2_core::formats::anim::{AnimRef, Clip};
use tf2_core::formats::scene::ObjectDef;

const ROSTER: [&str; 13] = [
    "bumblebee", "optimus", "ironhide", "ratchet", "megatron", "longhaul", "audir8", "jetfire", "firestorm",
    "optimusflight", "starscream", "blackout", "megatronflight",
];

/// The largest distance of any key of a track from one, sampled at every tick.
fn spread(clip: &Clip, track: impl Fn(f32) -> Option<[f32; 3]>) -> f32 {
    let mut worst = 0.0f32;
    let mut tick = 0.0;
    while tick <= clip.length {
        if let Some(v) = track(tick) {
            worst = worst.max(v.iter().map(|c| (c - 1.0).abs()).fold(0.0, f32::max));
        }
        tick += 1.0;
    }
    worst
}

/// How far the own scales move any node when they are wrongly handed down to the children (the
/// port before 2026-10-20) instead of kept to the node (the game): the largest distance over the
/// clip's ticks, and the node.
fn handed_down_shift(object: &ObjectDef, clip: &Clip, looping: bool) -> (f32, String) {
    use glam::{Affine3A, Quat, Vec3};
    let bind: Vec<Affine3A> = object.nodes.iter().map(|n| tf2_core::pose::affine(&n.bind)).collect();
    let rest: Vec<(Vec3, Quat, Vec3)> = object
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| n.parent.map_or(bind[i], |p| bind[p].inverse() * bind[i]).to_scale_rotation_translation())
        .collect();
    let (mut worst, mut node) = (0.0f32, String::new());
    let mut tick = 0.0;
    while tick <= clip.length {
        let (mut game, mut old): (Vec<Affine3A>, Vec<Affine3A>) = (Vec::new(), Vec::new());
        for (i, n) in object.nodes.iter().enumerate() {
            let (mut s, mut q, mut t) = rest[i];
            let mut own = Vec3::ONE;
            if let Some(c) = clip.channel(n.name_hash).filter(|_| n.name_hash != tf2_core::formats::anim::ROOT_CHANNEL) {
                if let Some(p) = c.position.sample(tick, clip.length, looping) { t = Vec3::from(p); }
                if let Some(r) = c.rotation.sample(tick, clip.length, looping) { q = tf2_core::pose::stored_turn(r); }
                if let Some(v) = c.scale.sample(tick, clip.length, looping) { s = Vec3::from(v); }
                if let Some(v) = c.own_scale.sample(tick, clip.length, looping) { own = Vec3::from(v); }
            }
            let parent = n.parent.filter(|&p| p < i);
            let local = Affine3A::from_scale_rotation_translation(s, q, t);
            game.push(parent.map_or(local, |p| game[p] * local));
            let merged = Affine3A::from_scale_rotation_translation(s * own, q, t);
            old.push(parent.map_or(merged, |p| old[p] * merged));
        }
        for (i, (g, o)) in game.iter().zip(&old).enumerate() {
            let d = g.translation.distance(o.translation);
            if d > worst {
                (worst, node) = (d, object.nodes[i].name.clone());
            }
        }
        tick += 2.0;
    }
    (worst, node)
}

fn survey(label: &str, refs: &[AnimRef], object: Option<&ObjectDef>) {
    for r in refs {
        let Some(clip) = r.clip.as_ref() else { continue };
        let (mut scaled, mut own) = (Vec::new(), Vec::new());
        for (name, channel) in clip.names.iter().zip(&clip.channels) {
            let node = object
                .and_then(|o| o.nodes.iter().find(|n| n.name_hash == *name))
                .map_or(format!("{name:08x}"), |n| n.name.clone());
            let s = spread(clip, |t| channel.scale.sample(t, clip.length, r.looping));
            if s > 0.01 {
                scaled.push(format!("{node} {s:.2}"));
            }
            let o = spread(clip, |t| channel.own_scale.sample(t, clip.length, r.looping));
            if o > 0.01 {
                own.push(format!("{node} {o:.2}"));
            }
        }
        // The aim's offset clips (laid additively, `MetaAdditiveAnimation`): any own scale on them.
        for (k, over) in r.aim_offsets.iter().enumerate() {
            let keyed = over.channels.iter().filter(|c| spread(over, |t| c.own_scale.sample(t, over.length, false)) > 0.01).count();
            if keyed > 0 {
                println!("  {label} {} aim offset {k}: own scale on {keyed} node(s)", r.id);
            }
        }
        if !scaled.is_empty() || !own.is_empty() {
            let shift = object.filter(|_| !own.is_empty()).map(|o| handed_down_shift(o, clip, r.looping));
            println!(
                "  {label} {:<28}{} scale {} node(s) {:?}  own scale {} node(s) {:?}{}",
                r.id,
                if r.additive { " (additive)" } else { "" },
                scaled.len(),
                scaled.iter().take(4).collect::<Vec<_>>(),
                own.len(),
                own.iter().take(4).collect::<Vec<_>>(),
                shift.map_or(String::new(), |(d, n)| format!("  handed down: {n} off by {d:.2} m"))
            );
        }
    }
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
        let mut sets: Vec<_> = data.animations.sets.iter().collect();
        sets.sort_by(|a, b| a.0.cmp(b.0));
        for (set, refs) in sets {
            survey(set, refs, Some(&data.robot));
        }
        survey("vehicle", &data.vehicle_animations, data.vehicle.as_ref());
        for (i, refs) in data.weapon_animations.iter().enumerate() {
            survey(&format!("weapon{i}"), refs, data.weapon_objects.get(i).and_then(Option::as_ref));
        }
    }
}
