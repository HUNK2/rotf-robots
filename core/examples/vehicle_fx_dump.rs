//! A pack's vehicle-object effects in detail: node, each element's renderer, texture, local space,
//! normal, velocity tracking and actions. For checking what an "exhaust" on a flyer really draws
//! (Blackout's `FX_smoke_exhaust` sits on his rotor hubs).
//! `cargo run -p tf2-core --example vehicle_fx_dump -- [pack]` (default blackout).

use std::path::Path;

fn main() {
    let dir = tf2_core::install::dir_string();
    let pack = std::env::args().nth(1).unwrap_or_else(|| "blackout".into());
    let data = tf2_core::character::load(Path::new(&dir), &pack).expect("pack");
    let Some(object) = data.vehicle.as_ref() else { return };
    for def in &object.effects {
        let t = &def.template;
        println!(
            "node {} {} script {:08x} attach {} template {:08x} elements {}",
            def.node, object.nodes.get(def.node).map_or("-", |n| n.name.as_str()), def.script_name, def.attach, t.name, t.elements.len()
        );
        for e in &t.elements {
            let r = &e.renderer;
            println!(
                "   {} tex {} local {} normal {:?} tracked {} blend {} tint {:?} max {} start {} end {}",
                String::from_utf8_lossy(&r.id.to_be_bytes()), r.texture_path, e.local_space, r.normal, r.velocity_tracked, r.blend_mode, r.tint, e.max_particles, e.start, e.end
            );
            for a in &e.actions {
                println!("      {:?}", a.kind);
            }
        }
    }
}
