//! A flyer in the empty arena: the flying trigger held, then let go with the weapon button held
//! (or the hover button pressed): the control state, overlay, mode and clip on each change.
//! For checking the way into hover and weapon hover.
//! `cargo run -p tf2-core --example fly_check -- <pack:AttributesName> [weapon|hover]`

use std::path::Path;

use tf2_core::control::button;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet};

fn main() {
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (pack, name) = args[0].split_once(':').expect("pack:Name");
    let how = args.get(1).map_or("weapon", String::as_str);
    let mut t = tuning::load(Path::new(&dir), name).expect("tuning");
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing).with_vehicle(data.vehicle.as_ref());
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    let mut s = State::new(&t);
    let arena = Arena::default();
    let mut last = String::new();
    let weapon = 1u64 << button::WEAPON_MODE;
    let hover = 1u64 << button::TRANSFORM_HOVER;
    for i in 0..200 {
        let flying = (5..80).contains(&i);
        let second = i >= 80;
        let mut input = Input { vehicle: if flying { 1.0 } else { 0.0 }, look: if (10..60).contains(&i) { glam::Vec2::new(0.0, -1.0) } else { glam::Vec2::ZERO }, ..Default::default() };
        if flying {
            input.buttons_down |= 1u64 << button::FLYING_MODE;
        }
        if second {
            let bit = if how == "hover" { hover } else { weapon };
            input.buttons_down |= bit;
            if i == 80 {
                input.buttons_hit |= bit;
            }
            if how == "weapon" {
                input.aim = Some(glam::Vec2::Y);
            }
        }
        sim::step(&mut s, &input, &t, &arena, 0.032);
        let now = format!(
            "{} / {} mode {:?} clip {}",
            s.control_state(&t).unwrap_or("-"),
            s.overlay_state(&t).unwrap_or("-"),
            s.mode,
            s.action_clip(&t).map(|(set, id, _)| format!("{set}/{id}")).or_else(|| s.base_clip(&t).map(|c| format!("{}/{}", c.set, c.id))).unwrap_or("-".into())
        );
        if now != last {
            println!("update {i:3} z {:6.1}: {now}", s.pos.z);
            last = now;
        }
    }
}
