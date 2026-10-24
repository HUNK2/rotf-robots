//! Which clips play while a robot jumps in weapon mode, without a window: per update the
//! mode, the action clip and the base clip with its tick. For Long Haul folding his arms in
//! a weapon-mode jump (the user's report, 2026-10-25).
//! `cargo run -p tf2-core --example weapon_jump_clips -- [pack:AttributesName] [jump at s] [seconds]`

use std::path::Path;

use glam::Vec2;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet};

fn main() {
    let dir = tf2_core::install::dir_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = args.first().map_or("longhaul:LongHaul", String::as_str);
    let jump_at: f32 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(1.0);
    let seconds: f32 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(3.0);
    let (pack, name) = arg.split_once(':').expect("pack:Name");
    let mut t = tuning::load(Path::new(&dir), name).expect("tuning");
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing);
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    let arena = Arena::default();
    let mut s = State::new(&t);
    let dt = sim::GAME_UPDATE;
    let mut last = String::new();
    for i in 0..(seconds / dt) as usize {
        let now = i as f32 * dt;
        let jump = (now - jump_at).abs() < dt * 0.5;
        let input = Input { aim: Some(Vec2::ZERO), jump, jump_held: jump, ..Default::default() };
        sim::step(&mut s, &input, &t, &arena, dt);
        let action = s.action_clip(&t).map(|(set, id, tick)| format!("{set}/{id}@{tick:.0}"));
        let base = s.base_clip(&t).map(|b| format!("{}/{}", b.set, b.id));
        let line = format!("{:?} action {:?} base {:?}", s.mode, action.as_deref().map(|a| a.split('@').next().unwrap()), base);
        if line != last {
            println!("{now:5.2} z {:5.2} {:?} action {:?} base {:?}", s.pos.z, s.mode, action, base);
            last = line;
        }
    }
}
