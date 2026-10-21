//! One character stood in the empty arena, the special button held: whether the special starts,
//! the trigger messages it sends, the blasts it makes, its timer and the type-6 state.
//! `cargo run -p tf2-core --example special_check -- <pack>:<AttributesName>...`

use std::collections::HashMap;
use std::path::Path;

use tf2_core::control::button;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet};

fn main() {
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    let names: HashMap<u32, String> = std::fs::read_to_string("C:/Bumblebee/export/data/names.tsv")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (h, n) = l.split_once('\t')?;
            Some((u32::from_str_radix(h, 16).ok()?, n.to_owned()))
        })
        .collect();
    let name_of = |h: u32| names.get(&h).cloned().unwrap_or(format!("{h:08x}"));
    for arg in std::env::args().skip(1) {
        let (pack, name) = arg.split_once(':').expect("pack:Name");
        let mut t = tuning::load(Path::new(&dir), name).expect("tuning");
        let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
        t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
        t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing).with_vehicle(data.vehicle.as_ref());
        t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
        t.rule_sets = RuleSet::all(&data.animations);
        println!(
            "== {pack}: kind {} cooldown {} duration {} script {} spawners named so {}",
            t.special.kind,
            t.special.cooldown,
            t.special.duration,
            name_of(t.special.script),
            t.actions.spawners.iter().filter(|s| s.name == t.special.script).count()
        );
        let mut s = State::new(&t);
        let arena = Arena::default();
        let mut blasts = 0;
        for i in 0..90 {
            let held = (10..40).contains(&i);
            let bit = 1u64 << button::ATTACK_SPECIAL;
            let input = Input { buttons_down: if held { bit } else { 0 }, buttons_hit: if i == 10 { bit } else { 0 }, ..Default::default() };
            let starts = s.special_starts;
            sim::step(&mut s, &input, &t, &arena, 0.032);
            blasts = blasts.max(s.blasts.len());
            if s.special_starts != starts || !s.triggers.is_empty() {
                println!(
                    "  update {i}: starts {} left {:.2} state {:?} overlay {:?} triggers {:?} blasts {} type6 {:?}",
                    s.special_starts,
                    s.special_left,
                    s.control_state(&t),
                    s.overlay_state(&t),
                    s.triggers.iter().map(|&h| name_of(h)).collect::<Vec<_>>(),
                    s.blasts.len(),
                    s.slowing_special(&t)
                );
            }
        }
        println!("  after: starts {} left {:.2} most blasts {blasts} type6 {:?}", s.special_starts, s.special_left, s.slowing_special(&t));
    }
}
