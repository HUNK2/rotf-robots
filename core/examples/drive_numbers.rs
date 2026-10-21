//! The car numbers of each driving character, as the install has them: the avatar's vehicle
//! mass, engine torque, top speeds and wheel radii. For checking what is shared and what is not.
//! `cargo run -p tf2-core --example drive_numbers -- [AttributesName...]`

use std::path::Path;

use tf2_core::sim::{self, Arena, Input, State};

fn main() {
    let dir = tf2_core::install::dir_string();
    let mut names: Vec<String> = std::env::args().skip(1).collect();
    if names.is_empty() {
        names = ["Bumblebee", "Optimus", "Ironhide", "Ratchet", "Megatron", "LongHaul", "Audi"].map(str::to_owned).to_vec();
    }
    for name in names {
        let t = tf2_core::tuning::load(Path::new(&dir), &name).expect("tuning");
        let d = &t.drive;
        // Held throttle on the flat empty arena: the speed it settles at after 6 s.
        let mut s = State::new(&t);
        let arena = Arena::default();
        let gas = Input { vehicle: 1.0, ..Default::default() };
        let mut speeds = Vec::new();
        for i in 0..(6.0 / 0.032) as usize {
            sim::step(&mut s, &gas, &t, &arena, 0.032);
            if i % 31 == 30 { speeds.push(format!("{:.1}", s.speed)); }
        }
        println!(
            "{name:10} vehicle_mass {:7.0} torque {:7.0} max {:5.1} turbo {:5.1} wheel f {:.2} r {:.2}  speed by second {}",
            t.vehicle_mass, d.engine_torque, d.max_speed, d.max_turbo_speed, d.front_wheel_radius, d.rear_wheel_radius, speeds.join(" ")
        );
    }
}
