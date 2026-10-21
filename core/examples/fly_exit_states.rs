//! A flyer lifts off on the held flying trigger, lets it go and comes back to the robot: every
//! control state it enters, with its class, `robotForm` and `State::car_mode` after the update.
//! `cargo run -p tf2-core --example fly_exit_states -- <pack:AttributesName> [hold updates]`

use std::path::Path;

use tf2_core::control::button;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet};

fn main() {
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (pack, name) = args[0].split_once(':').expect("pack:Name");
    let hold: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(80);
    let mut t = tuning::load(Path::new(&dir), name).expect("tuning");
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing).with_vehicle(data.vehicle.as_ref());
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    for name in ["stateFlyExitA", "stateFlyExitB"] {
        let Some(def) = t.control.states.iter().find(|d| d.name == name) else { continue };
        println!("{name} anim {:08x} set {:?} mode {:08x} finishAnim {} once {}", def.anim, def.anim_set, def.motion_mode, def.finish_anim, def.play_anim_once);
        for tr in &def.transitions {
            let c = &t.control.connections[tr.connection];
            let to = tr.output.map_or("<back>", |o| t.control.states[o].name.as_str());
            println!("  -> {to} prio {} via {} (ground {} air {} hold {} nohold {} branch {}..{} test {:?})",
                tr.priority, c.name, c.on_ground, c.in_air, c.holding, c.not_holding, c.branch_begin, c.branch_end, c.test);
        }
    }
    let mut s = State::new(&t);
    let arena = Arena::default();
    let mut last = String::new();
    for i in 0..(hold + 200) {
        let held = (5..hold).contains(&i);
        let mut input = Input { vehicle: if held { 1.0 } else { 0.0 }, ..Default::default() };
        // `weapon`: the weapon-mode trigger from 5 updates before the release; `hover`: the jump
        // (hover) button held from then on.
        let how = args.get(2).map_or("", String::as_str);
        // `FLY_CLIMB=1`: the stick pulled back for 25 updates before the release (a jet climbs; a
        // helicopter lifts on the camera stick, so both are pulled).
        // `FLY_CLIMB=<n>` pulls for n updates instead (the stick stays pulled to the release).
        let climb: usize = std::env::var("FLY_CLIMB").ok().map_or(0, |v| v.parse().unwrap_or(25));
        let until = if climb == 25 { hold - 5 } else { hold };
        if climb > 0 && held && i + climb + 5 >= hold && i < until {
            input.stick = glam::Vec2::new(0.0, -1.0);
            input.pad_stick = input.stick;
            input.look = glam::Vec2::new(0.0, -1.0);
        }
        if i + 5 >= hold && i < hold + 120 {
            match how {
                "weapon" => input.aim = Some(glam::Vec2::Y),
                "hover" => input.jump_held = true,
                _ => {}
            }
        }
        if held {
            input.buttons_down |= 1u64 << button::FLYING_MODE;
        }
        sim::step(&mut s, &input, &t, &arena, 0.032);
        if std::env::var("FLY_EXIT_EVERY").is_ok_and(|v| v.split_once('-').is_some_and(|(a, b)| (a.parse().unwrap_or(0)..=b.parse().unwrap_or(0)).contains(&i))) {
            println!("  {i:3} z {:6.2} vel {:+6.2} {:+6.2} {:+6.2} {:?} clip {:?}", s.pos.z, s.vel.x, s.vel.y, s.vel.z, s.mode, s.action_clip(&t));
        }
        let name = s.control_state(&t).unwrap_or("-").to_string();
        if name != last {
            let def = t.control.states.iter().find(|d| d.name == name);
            let (class, form) = def.map_or(("?", 0), |d| (d.class.as_str(), d.robot_form));
            println!("update {i:3} z {:5.1} {:?} -> {name} ({class}, robotForm {form}) car_mode {}", s.pos.z, s.mode, s.car_mode);
            last = name;
        }
    }
    println!("end: {:?} car_mode {}", s.mode, s.car_mode);
}
