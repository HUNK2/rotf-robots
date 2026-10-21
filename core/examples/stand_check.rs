//! One character stood in the empty arena for a few updates: which control state, mode and
//! clip the movement code gives it. For finding why a character shows no clip.
//! `cargo run -p tf2-core --example stand_check -- <pack>:<AttributesName>...`

use std::path::Path;

use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet};

fn main() {
    let dir = tf2_core::install::dir_string();
    for arg in std::env::args().skip(1) {
        let (pack, name) = arg.split_once(':').expect("pack:Name");
        let mut t = tuning::load(Path::new(&dir), name).expect("tuning");
        let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
        t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
        t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing).with_vehicle(data.vehicle.as_ref());
        t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
        t.rule_sets = RuleSet::all(&data.animations);
        println!("== {pack}: missing {:?}", t.missing);
        let mut s = State::new(&t);
        let arena = Arena::default();
        for i in 0..4 {
            sim::step(&mut s, &Input::default(), &t, &arena, 0.032);
            println!(
                "  update {i}: mode {:?}, pos {:?}, action clip {:?}, base clip {:?}",
                s.mode,
                s.pos,
                s.action_clip(&t).map(|(set, id, tick)| format!("{set}/{id} at {tick:.0}")),
                s.base_clip(&t).map(|c| format!("{}/{} at {:.0}", c.set, c.id, c.tick)),
            );
        }
    }
}
