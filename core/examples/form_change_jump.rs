//! A flyer changes form on the held flying trigger and back on its release, with the stick
//! forward through all of it: every update's control state, mode, position, velocity and yaw,
//! and the largest jump in position or yaw from one update to the next against what the
//! velocity before it predicts. For the "rubber band at the end of the change of form" report.
//! `cargo run -p tf2-core --example form_change_jump -- <pack:AttributesName> [hold updates] [stick y]`

use std::path::Path;

use tf2_core::control::button;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet};

fn main() {
    let dir = tf2_core::install::dir_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (pack, name) = args[0].split_once(':').expect("pack:Name");
    let hold: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(120);
    let stick: f32 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(0.0);
    let mut t = tuning::load(Path::new(&dir), name).expect("tuning");
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing).with_vehicle(data.vehicle.as_ref());
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    let mut s = State::new(&t);
    let arena = Arena::default();
    let dt = 0.032;
    for i in 0..(hold + 150) {
        let held = (5..hold).contains(&i);
        let mut input = Input { vehicle: if held { 1.0 } else { 0.0 }, ..Default::default() };
        input.stick = glam::Vec2::new(0.0, stick);
        input.pad_stick = input.stick;
        if held {
            input.buttons_down |= 1u64 << button::FLYING_MODE;
        }
        let (pos, vel, yaw) = (s.pos, s.vel, s.yaw);
        sim::step(&mut s, &input, &t, &arena, dt);
        let off = s.pos - (pos + vel * dt);
        let turn = (s.yaw - yaw).to_degrees();
        println!(
            "{i:3} {:<16} {:<10} pos {:7.2} {:7.2} {:6.2} vel {:+6.2} {:+6.2} {:+6.2} yaw {:7.2} dyaw {:+6.2} off {:5.2} clip {}",
            s.control_state(&t).unwrap_or("-"),
            format!("{:?}", s.mode).chars().take(10).collect::<String>(),
            s.pos.x, s.pos.y, s.pos.z, s.vel.x, s.vel.y, s.vel.z, s.yaw.to_degrees(), turn, off.length(),
            s.action_clip(&t).map(|(set, id, _)| format!("{set}/{id}")).unwrap_or("-".into()),
        );
    }
}
