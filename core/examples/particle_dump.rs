//! Steps some of Bumblebee's particle templates through the reference and prints their particles,
//! for a native port to compare its own particle system with: each named template from the
//! game's generator at 0, at a place turned 30 degrees about z and moved to (1, 2, 3), stepped at
//! 1/60 s; every 10th step prints each particle's place, radius, alpha and frame as one JSON line.
//! `cargo run --release -p tf2-core --example particle_dump -- <steps> <template>...`

use std::collections::HashSet;

use glam::{Affine3A, Quat, Vec3};
use tf2_core::formats::hash::crc32;
use tf2_core::formats::lxb::{DataFile, Node};
use tf2_core::formats::pack::{DATA, Pack};
use tf2_core::particles::{Effect, Rng, Template};

fn walk<'a>(node: Node<'a>, seen: &mut HashSet<(usize, Option<&'a str>)>, out: &mut Vec<Node<'a>>) {
    if let Some(at) = node.offset()
        && !seen.insert((at, node.type_name()))
    {
        return;
    }
    if node.type_name() == Some("ParticleTemplate") {
        out.push(node);
        return;
    }
    for (_, field) in node.fields() {
        walk(field, seen, out);
    }
    for item in node.items() {
        walk(item, seen, out);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let steps: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(120);
    let wanted: Vec<String> = args.collect();
    let pack = Pack::open(&tf2_core::install::dir().join("characters").join("bumblebee.str")).expect("pack");
    let files: Vec<DataFile> = pack.of_type(DATA).filter_map(|c| DataFile::parse(pack.data(c).to_vec()).ok()).collect();
    let mut found = Vec::new();
    for file in &files {
        walk(file.root(), &mut HashSet::new(), &mut found);
    }
    let place = Affine3A::from_rotation_translation(Quat::from_rotation_z(30f32.to_radians()), Vec3::new(1.0, 2.0, 3.0));
    for name in &wanted {
        let Some(template) = found.iter().filter(|n| n.label() == Some(crc32(name.as_bytes()))).find_map(|n| Template::read(*n)) else {
            eprintln!("no template {name}");
            continue;
        };
        let mut effect = Effect::new(&template);
        let mut rng = Rng(0);
        for step in 1..=steps {
            effect.step(&template, &place, 1.0 / 60.0, &mut rng);
            if step % std::env::var("EVERY").ok().and_then(|s| s.parse().ok()).unwrap_or(10usize) != 0 {
                continue;
            }
            let mut ps = Vec::new();
            for e in 0..template.elements.len() {
                for p in effect.group(e).map(|g| g.particles.as_slice()).unwrap_or(&[]) {
                    ps.push(format!("[{},{},{},{},{},{},{},{},{}]", e, p.pos.x, p.pos.y, p.pos.z, p.radius, p.alpha, p.frame, p.age, p.lifetime));
                }
            }
            println!("{{\"t\":\"{name}\",\"step\":{step},\"ps\":[{}]}}", ps.join(","));
        }
    }
}
