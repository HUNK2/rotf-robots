//! Each character's melee from its own data, without a window: three taps of the melee button
//! against a target standing in front of him. Prints the attack states and clips that play, the
//! ticks at which the clip switches hit bodies on, the hits and their damage, and how far he
//! travelled. For settling the "melee UNCLEAR" verdicts of the 2026-10-12 captures, which could
//! not see the swing.
//! `cargo run -p tf2-core --example melee_check -- [pack:AttributesName...] [--at <metres>]`

use std::path::Path;

use glam::Vec3;
use tf2_core::control::button;
use tf2_core::melee::Target;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet};

const ROSTER: [&str; 13] = [
    "bumblebee:Bumblebee", "optimus:Optimus", "ironhide:Ironhide", "ratchet:Ratchet", "megatron:Megatron",
    "longhaul:LongHaul", "audir8:Audi", "jetfire:Jetfire", "firestorm:Firestorm", "starscream:Starscream",
    "blackout:Blackout", "optimusflight:OptimusFlight", "megatronflight:MegatronFlight",
];

fn main() {
    let dir = tf2_core::install::dir_string();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut at = 6.0;
    if let Some(i) = args.iter().position(|a| a == "--at") {
        at = args[i + 1].parse().expect("metres");
        args.drain(i..i + 2);
    }
    if args.is_empty() {
        args = ROSTER.map(str::to_owned).to_vec();
    }
    for arg in args {
        let (pack, name) = arg.split_once(':').expect("pack:Name");
        let mut t = tuning::load(Path::new(&dir), name).expect("tuning");
        let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
        t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
        t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing)
            .with_vehicle(data.vehicle.as_ref())
            .with_melee(&data.melee_weapons);
        t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
        t.rule_sets = RuleSet::all(&data.animations);
        let arena = Arena {
            targets: vec![Target { id: 1, pos: Vec3::new(0.0, at, 0.0), radius: 2.0, height: 5.4, character: true, large: false, surface: tf2_core::melee::surface::UPPER_BODY, dashing: false, health: f32::MAX, head: None }],
            ..Default::default()
        };
        let attack_clips = t.actions.clips.values().filter(|c| c.attack.is_some()).count();
        let bodies: usize = t.actions.clips.values().map(|c| c.bodies.len()).sum();
        println!("== {pack}: {attack_clips} attack clips, {bodies} hit bodies in all, robot height {} radius {}", t.robot_height, t.robot_radius);
        let mut s = State::new(&t);
        let bit = 1u64 << button::ATTACK_FAST;
        let mut last = String::new();
        let (mut hits, mut damage) = (0usize, 0.0f32);
        let mut live_ticks = Vec::new();
        let mut top_speed = 0.0f32;
        for i in 0..140 {
            let tap = [5, 20, 35].contains(&i);
            let input = Input { buttons_down: if tap { bit } else { 0 }, buttons_hit: if tap { bit } else { 0 }, ..Default::default() };
            sim::step(&mut s, &input, &t, &arena, 0.032);
            top_speed = top_speed.max(s.vel.truncate().length());
            let now = format!(
                "{} {}",
                s.control_state(&t).unwrap_or("-"),
                s.action_clip(&t).map_or("-".to_owned(), |(set, id, _)| format!("{set}/{id}"))
            );
            if now != last {
                println!("  update {i:3}: {now}");
                last = now;
            }
            if let Some(action) = s.action.as_ref().filter(|a| a.hitting && !a.live.is_empty()) {
                live_ticks.push(action.tick as i32);
            }
            for hit in &s.hits {
                hits += 1;
                damage += hit.damage;
            }
        }
        live_ticks.dedup();
        let span = match (live_ticks.first(), live_ticks.last()) {
            (Some(a), Some(b)) => format!("{a}..{b}"),
            _ => "never".to_owned(),
        };
        println!(
            "  hit bodies live at ticks {span}; hits {hits}, damage {damage:.1}; travelled {:.2} m, top speed {top_speed:.1} (move max {:.1})",
            s.pos.y, t.moving.max_speed
        );
    }
}
