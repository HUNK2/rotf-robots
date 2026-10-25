//! The nodes one clip keys, and which of the character's objects has a node of that name.
//! `cargo run -p rotf-export --example clip_nodes -- <pack> <set> <id>`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let install = tf2_core::install::dir();
    let data = tf2_core::character::load(install, &args[0]).unwrap();
    let r = data.animations.find(&args[1], &args[2]).expect("no such clip");
    let clip = r.clip.as_ref().unwrap();
    let mut objects: Vec<(String, &tf2_core::formats::scene::ObjectDef)> = vec![("robot".into(), &data.robot)];
    objects.extend(data.vehicle.iter().map(|v| ("vehicle".into(), v)));
    for (i, o) in data.weapon_objects.iter().enumerate() {
        objects.extend(o.iter().map(|o| (format!("weapon{i}"), o)));
    }
    for (i, o) in data.turret_objects.iter().enumerate() {
        objects.extend(o.iter().map(|o| (format!("turret{i}"), o)));
    }
    for (i, m) in data.melee_weapons.iter().enumerate() {
        objects.extend(m.object.iter().map(|o| (format!("melee{i}"), o)));
    }
    for o in data.robot.effects.iter().filter_map(|e| e.proxy.as_deref()) {
        objects.push(("robot effect proxy".into(), o));
    }
    for (name, channel) in clip.names.iter().zip(&clip.channels) {
        if !channel.keys_any() {
            continue;
        }
        let found: Vec<String> = objects
            .iter()
            .filter_map(|(label, o)| o.nodes.iter().find(|n| n.name_hash == *name).map(|n| format!("{label}:{}", n.name)))
            .collect();
        println!("{name:08x} {found:?}");
    }
}
