//! A flyer in the empty arena with the flying trigger held and the stick pushed: the fly angles and
//! the control-surface turns / rotor state of `flightsurface` (0086cbf0) every few updates.
//! `cargo run -p tf2-core --example surface_fly -- <pack:AttributesName> [pitch|roll|lift]`

use std::path::Path;

use tf2_core::control::button;
use tf2_core::flightsurface::Apply;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet};

fn main() {
    let dir = tf2_core::install::dir_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (pack, name) = args[0].split_once(':').expect("pack:Name");
    let how = args.get(1).map_or("roll", String::as_str);
    let mut t = tuning::load(Path::new(&dir), name).expect("tuning");
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing).with_vehicle(data.vehicle.as_ref());
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    let mut s = State::new(&t);
    let arena = Arena::default();
    for i in 0..160 {
        let mut input = Input { vehicle: if i >= 5 { 1.0 } else { 0.0 }, ..Default::default() };
        if i >= 5 {
            input.buttons_down |= 1u64 << button::FLYING_MODE;
        }
        // Lift first, then the test input.
        if (10..50).contains(&i) {
            input.look = glam::Vec2::new(0.0, -1.0);
        }
        if (70..100).contains(&i) {
            match how {
                "pitch" => { input.stick = glam::Vec2::new(0.0, -1.0); input.pad_stick = input.stick; }
                "lift" => input.look = glam::Vec2::new(0.0, 1.0),
                _ => { input.stick = glam::Vec2::new(1.0, 0.0); input.pad_stick = input.stick; input.steer = 1.0; }
            }
        }
        sim::step(&mut s, &input, &t, &arena, 0.032);
        if i % 5 != 0 || s.fly.is_none() {
            continue;
        }
        let fly = s.fly.as_ref().unwrap();
        let f = &s.flight_surfaces;
        let pose = f.pose(&t.forms.fly);
        let turns: Vec<String> = pose.turns.iter().take(6).map(|(n, a)| {
            let q = match a { Apply::Replace(q) | Apply::After(q) => *q };
            let (axis, ang) = q.to_axis_angle();
            format!("{n} {:+.1}deg@{:.0},{:.0},{:.0}", ang.to_degrees() * axis.dot(glam::Vec3::ONE).signum(), axis.x, axis.y, axis.z)
        }).collect();
        println!(
            "update {i:3} z {:6.1} p/r/h {:+.3} {:+.3} {:+.3} rates {:+.3} {:+.3} {:+.3} alpha {:.2} rpm {:.0} rotor {:+.2} | {}",
            s.pos.z, fly.pitch, fly.roll, fly.heading, f.rates.x, f.rates.y, f.rates.z, f.rotor_alpha, f.rotor_speed, f.rotor_angle, turns.join("  ")
        );
    }
}
