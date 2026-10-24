//! Blackout's vehicle nodes under and around the rotors: name, parent, bind origin.
//! `cargo run -p tf2-core --example rotor_tree -- [pack]` (default blackout).

use std::path::Path;

fn main() {
    let dir = tf2_core::install::dir_string();
    let pack = std::env::args().nth(1).unwrap_or_else(|| "blackout".into());
    let data = tf2_core::character::load(Path::new(&dir), &pack).expect("pack");
    let object = data.vehicle.as_ref().expect("vehicle");
    for (i, n) in object.nodes.iter().enumerate() {
        let mut depth = 0;
        let mut p = n.parent;
        while let Some(q) = p {
            depth += 1;
            p = object.nodes[q].parent;
        }
        let o = n.bind[3];
        println!("{i:3} {}{} parent {:?} at [{:.2} {:.2} {:.2}]", "  ".repeat(depth), n.name, n.parent.map(|q| object.nodes[q].name.as_str()), o[0], o[1], o[2]);
        if std::env::args().any(|a| a == "--axes") {
            let r = |k: usize| n.bind[k].map(|v| (v * 1000.0).round() / 1000.0);
            println!("      x {:?} y {:?} z {:?}", r(0), r(1), r(2));
        }
    }
}
