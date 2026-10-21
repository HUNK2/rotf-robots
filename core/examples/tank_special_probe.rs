//! Megatron (or another pack) in his alt form, the special pressed: the control state, the
//! action clip and its tick, `I_SPECIAL_ON`'s timer and the car mode, update by update.
//! `cargo run -p tf2-core --example tank_special_probe -- [pack:AttributesName]`


use tf2_core::control::button;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet};

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "megatron:Megatron".into());
    let (pack, name) = arg.split_once(':').expect("pack:Name");
    let dir = tf2_core::install::dir();
    let mut t = tuning::load(dir, name).expect("tuning");
    let data = tf2_core::character::load(dir, pack).expect("character");
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing).with_vehicle(data.vehicle.as_ref());
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    let mut s = State::new(&t);
    let arena = Arena::default();
    let bit = 1u64 << button::ATTACK_SPECIAL;
    for i in 0..260 {
        // The vehicle trigger held from update 5; the special pressed at 80.
        let mut input = Input { vehicle: if i >= 5 { 1.0 } else { 0.0 }, ..Default::default() };
        if i == 80 {
            input.buttons_hit = bit;
        }
        if (80..84).contains(&i) {
            input.buttons_down = bit;
        }
        sim::step(&mut s, &input, &t, &arena, 0.032);
        if (78..84).contains(&i) {
            println!("    action {:?}", s.action.as_ref().map(|a| (a.clip, a.tick, a.finished)));
        }
        if i % 4 == 0 || (78..90).contains(&i) {
            println!(
                "{i:3} state {:?} overlay {:?} clip {:?} car {} special_left {:.2}",
                s.control_state(&t),
                s.overlay_state(&t),
                s.action_clip(&t).map(|(set, id, tick)| format!("{set}/{id}@{tick:.0}")),
                s.is_vehicle(),
                s.special_left
            );
        }
    }
}
