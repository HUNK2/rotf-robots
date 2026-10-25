//! Prints what a character pack holds beside the robot: the vehicle object and its clips, each
//! weapon (kind, name, bone, list) with its object's models, and the melee weapons.
//! `cargo run -p rotf-export --example parts_peek -- <pack>`

use tf2_core::formats::scene::ObjectDef;

fn models(object: Option<&ObjectDef>) -> String {
    object.map_or("-".into(), |o| {
        let mut m: Vec<&str> = o.renders.iter().map(|r| r.model.as_str()).collect();
        m.dedup();
        format!("{} nodes, root {:?}, models {m:?}", o.nodes.len(), o.nodes.first().map(|n| &n.name))
    })
}

fn main() {
    let pack = std::env::args().nth(1).unwrap_or_else(|| "bumblebee".into());
    let install = tf2_core::install::dir();
    let data = tf2_core::character::load(install, &pack).unwrap();
    let node_name = |hash: u32| {
        data.robot.nodes.iter().chain(data.vehicle.iter().flat_map(|v| &v.nodes)).find(|n| n.name_hash == hash).map_or(format!("{hash:08x}"), |n| n.name.clone())
    };
    let packfile = tf2_core::formats::pack::Pack::open(&tf2_core::formats::characterlist::pack_file(install, &pack)).unwrap();
    let files: Vec<_> = packfile
        .of_type(tf2_core::formats::pack::DATA)
        .filter_map(|c| tf2_core::formats::lxb::DataFile::parse(packfile.data(c).to_vec()).ok())
        .collect();
    let names: Vec<u32> = data.weapons.iter().map(|w| w.name).chain(data.melee_weapons.iter().map(|m| m.name)).collect();
    for hash in names {
        println!("{hash:08x} -> {:?}", files.iter().find_map(|f| f.name(hash)));
    }
    println!("vehicle: {}", models(data.vehicle.as_ref()));
    println!("vehicle clips: {:?}", data.vehicle_animations.iter().map(|r| &r.id).collect::<Vec<_>>());
    for (i, w) in data.weapons.iter().enumerate() {
        println!(
            "weapon {i}: {:?} name {:08x} bone {} list {} | object {} | grenade {} | turret {} ({} clips) | missile {} | clips {:?}",
            w.kind,
            w.name,
            node_name(w.bone),
            w.list,
            models(data.weapon_objects[i].as_ref()),
            models(data.grenade_objects[i].as_ref()),
            models(data.turret_objects[i].as_ref()),
            data.turret_clips[i].len(),
            models(data.missile_objects[i].as_ref()),
            data.weapon_animations[i].iter().map(|r| &r.id).collect::<Vec<_>>()
        );
    }
    for m in &data.melee_weapons {
        println!(
            "melee {:08x} start {} | object {} | clips {:?}",
            m.name,
            node_name(m.start_node),
            models(m.object.as_ref()),
            m.clips.iter().map(|r| &r.id).collect::<Vec<_>>()
        );
    }
}
