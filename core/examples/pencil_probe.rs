//! One step of the avatar's pencil through the game's contact pass (`LevelWorld::move_pencil`,
//! `physics_contact`) on a level, with every contact printed: which primitive, its kind and masks,
//! the record's t, normal and point, and whether the floor rule took it.
//!
//! cargo run --manifest-path C:/Bumblebee/Cargo.toml -p tf2-core --release --example pencil_probe -- x y z dx dy dz [zone]
use glam::Vec3;
use tf2_core::collision_world::WorldShape;
use tf2_core::level_world::{LevelWorld, CHARACTER_MASK};
use tf2_core::pair_sweep::PairShape;
use tf2_core::physics_contact::{contact_pass, AvatarFloor, MovingBody, IDENTITY};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let f = |i: usize| a.get(i).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    let (from, d) = (Vec3::new(f(1), f(2), f(3)), Vec3::new(f(4), f(5), f(6)));
    let zone = a.get(7).map(String::as_str).unwrap_or("levels/shanghai_01/zone.str");
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    let level = LevelWorld::load(std::path::Path::new(&dir), zone).expect("level");
    let (radius, height) = (2.0f32, 5.4f32);
    let gap_part = height * 0.45454544;
    let cylinder = height - gap_part;
    let h = cylinder * 0.5;
    let tip = gap_part * 0.5;
    let shape = PairShape::Pencil { r: radius, h, a: h + tip, c: h / (tip * tip + radius * radius).sqrt() };
    let centre = h + cylinder * 0.45454544;
    let floor = AvatarFloor { centre_above_feet: centre, floor_reach: 0.8 * (cylinder * 0.45454544), wide_tolerance: false };
    let place = |p: Vec3| {
        let mut m = IDENTITY;
        m[9] = p.x;
        m[10] = p.y;
        m[11] = p.z + centre;
        m
    };
    let lo = from.min(from + d) + Vec3::Z * centre - Vec3::new(radius, radius, h + tip) - 0.05;
    let hi = from.max(from + d) + Vec3::Z * centre + Vec3::new(radius, radius, h + tip) + 0.05;
    let mut nodes = level.world.pair_nodes(lo, hi, |i| Some([level.masks[i].send, level.masks[i].receive]));
    println!("{} nodes near the path", nodes.len());
    for n in &nodes {
        let p = &level.world.primitives[n.id];
        let kind = match &p.shape {
            WorldShape::Cuboid(_) => "cuboid",
            WorldShape::Sphere(_) => "sphere",
            WorldShape::Cylinder(_) => "cylinder",
            WorldShape::Cone(_) => "cone",
            WorldShape::Hull(_) => "hull",
            WorldShape::Mesh { .. } => "mesh",
            WorldShape::Unsupported { .. } => "unsupported",
        };
        println!("  node {} {kind} masks {:04x} / {:04x} centre {:?} {}", n.id, n.masks[0], n.masks[1], p.centre, p.source);
    }
    let v = d / 0.032;
    let mut body = MovingBody::new(shape, place(from), place(from + d), v, [CHARACTER_MASK; 2], Some(floor));
    let res = contact_pass(&mut body, &mut nodes);
    println!("passes {}, committed {}", res.passes, res.committed);
    for c in &res.contacts {
        let r = &c.record;
        println!(
            "  id {} t {:.4} lambda {:.4} start-in-contact {} normal {:?} point {:?} face {} floor {} responded {}",
            c.id, r.t, r.lambda, r.contact, r.normal, r.point, r.face as i32, c.floor, c.responded
        );
    }
    let end = res.end(&body, 0.032);
    println!("end feet {:?}, velocity {:?}", Vec3::new(end[9], end[10], end[11] - centre), body.velocity);
    // the stand-in this replaces, for comparison
    let mut p = from + d;
    let hit = tf2_core::sim::World::push_out(&level, &mut p, radius, height);
    println!("stand-in push_out at the unblocked end: hit {hit}, moved to {p:?}");
    if let Some(i) = nodes.iter().position(|n| n.id == 6016) {
        if let WorldShape::Cuboid(c) = &level.world.primitives[nodes[i].id].shape {
            println!("cuboid 6016: half {:?} x {:?}", c.shape.half, c.x);
        }
    }
}
