//! A jet held in flight with a fixed stick: its speed every 0.32 s. For checking the speed caps.
//! `cargo run -p tf2-core --example fly_speed_probe -- <pack:AttributesName> <stick y> [turbo at update]
//! [hover at update]` (a turbo update past 250 is none; with a hover update, every update round it).

use std::path::Path;

use tf2_core::control::button;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet};

fn main() {
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (pack, name) = args[0].split_once(':').expect("pack:Name");
    let stick_y: f32 = args.get(1).map_or(0.0, |a| a.parse().expect("stick y"));
    let turbo_at: Option<usize> = args.get(2).map(|a| a.parse().expect("update"));
    let mut t = tuning::load(Path::new(&dir), name).expect("tuning");
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing).with_vehicle(data.vehicle.as_ref());
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    // FLY_PROBE_CLIPS=1: the take-off clips (length, speed, root height per tick, events) first.
    if std::env::var("FLY_PROBE_CLIPS").is_ok() {
        for (hash, c) in &t.actions.clips {
            if c.id.contains("Trans") || c.id.contains("Fly") || c.id.contains("Takeoff") {
                println!("clip {hash:08x} {}/{} ticks {} speed {} crossfade {} looping {}", c.set, c.id, c.ticks, c.speed, c.crossfade, c.looping);
                println!("  z {:?}", c.path.iter().map(|p| (p.z * 100.0).round() / 100.0).collect::<Vec<_>>());
                println!("  events {:?}", c.events);
            }
        }
    }
    let mut s = State::new(&t);
    // The game's default upgrade level (`--turbo-upgrade`, 3), as in the 2026-10-21 capture.
    s.turbo_upgrades = [3; 2];
    let arena = Arena::default();
    let hover_at: Option<usize> = args.get(3).map(|a| a.parse().expect("update"));
    // FLY_PROBE_EVERY=1 prints every update (the take-off).
    let every = std::env::var("FLY_PROBE_EVERY").ok().and_then(|v| v.parse().ok())
        .unwrap_or(if hover_at.is_some() { 1 } else { 10 });
    for i in 0..250 {
        let hovering = hover_at.is_some_and(|h| i >= h);
        let mut input = Input { vehicle: if hovering { 0.0 } else { 1.0 }, ..Default::default() };
        if hovering {
            // As fly_check's `hover`: the trigger let go, the hover button pressed and held.
            input.buttons_down |= 1u64 << button::TRANSFORM_HOVER;
            if hover_at == Some(i) {
                input.buttons_hit |= 1u64 << button::TRANSFORM_HOVER;
            }
        } else {
            input.buttons_down |= 1u64 << button::FLYING_MODE;
        }
        // With a hover update, the stick is held only in hover (the flight before it level).
        if i >= 20 && hover_at.map_or(true, |_| hovering) {
            input.pad_stick = glam::Vec2::new(0.0, stick_y);
            if hovering {
                // Hover moves by the world-space stick; the jet faces +y in the empty arena.
                input.stick = input.pad_stick;
            }
        }
        if turbo_at == Some(i) {
            input.jump = true;
        }
        sim::step(&mut s, &input, &t, &arena, 0.032);
        if i % every == 0 && hover_at.map_or(true, |h| i + 3 >= h && i <= h + 60) {
            println!("t {:5.2} mode {:?} speed {:6.2} vz {:6.2} z {:7.2} horiz {:6.2} | {} clip {:08x} tick {:6.2} control {:?}",
                i as f32 * 0.032, std::mem::discriminant(&s.mode), s.vel.length(), s.vel.z, s.pos.z,
                s.vel.truncate().length(), s.control_state(&t).unwrap_or("-"),
                s.action.as_ref().map_or(0, |a| a.clip), s.action.as_ref().map_or(-1.0, |a| a.tick),
                s.fly.as_ref().map(|f| f.control));
        }
    }
}
