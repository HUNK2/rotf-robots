//! Everything a vertical line meets in a zone's collision, top to bottom: what is at a spot.
//!
//! cargo run --manifest-path C:/Bumblebee/Cargo.toml -p tf2-core --release --example zone_probe -- <zone under the install> <x> <y>
use glam::Vec3;
use tf2_core::collision_world::WorldShape;
use tf2_core::level_world::LevelWorld;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: zone_probe <zone under the install, e.g. levels/shanghai_01/zone.str> <x> <y>");
        std::process::exit(2);
    }
    let dir = tf2_core::install::dir_string();
    let level = LevelWorld::load(std::path::Path::new(&dir), &args[1]).expect("level");
    let (x, y): (f32, f32) = (args[2].parse().expect("x"), args[3].parse().expect("y"));
    let mut hits = level.world.ray_cast(Vec3::new(x, y, 2000.0), Vec3::new(x, y, -2000.0));
    hits.sort_by(|a, b| a.lambda.total_cmp(&b.lambda));
    println!("     z   kind      preset    send  recv  released  size                 source");
    for h in hits {
        let p = &level.world.primitives[h.primitive];
        let m = level.masks[h.primitive];
        let (kind, size) = match &p.shape {
            WorldShape::Cuboid(c) => ("cuboid", format!("{:.1} {:.1} {:.1}", c.shape.half.x * 2.0, c.shape.half.y * 2.0, c.shape.half.z * 2.0)),
            WorldShape::Sphere(_) => ("sphere", String::new()),
            WorldShape::Cylinder(_) => ("cylinder", String::new()),
            WorldShape::Cone(_) => ("cone", String::new()),
            WorldShape::Mesh { .. } => ("mesh", String::new()),
            WorldShape::Hull(_) => ("hull", String::new()),
            WorldShape::Unsupported { .. } => ("other", String::new()),
        };
        println!(
            "{:7.2}  {kind:8}  {:08x}  {:04x}  {:04x}  {:8}  {size:20} {}",
            h.point.z, p.mask_preset, m.send, m.receive, p.released, p.source
        );
    }
}
