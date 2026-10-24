//! A character's melee weapons from a melee tap into weapon mode, without a window: each
//! update's control state, action clip and, per melee weapon, whether it is shown and which of
//! its object's clips plays at what tick. For Optimus's blades staying out while his guns come
//! out (the user's report, 2026-10-25).
//! `cargo run -p tf2-core --example blade_putaway -- [pack:AttributesName] [aim from update]`

use std::path::Path;

use glam::Vec2;
use tf2_core::control::button;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet};

fn main() {
    let dir = tf2_core::install::dir_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = args.first().map_or("optimus:Optimus", String::as_str);
    let aim_from: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(30);
    let (pack, name) = arg.split_once(':').expect("pack:Name");
    let mut t = tuning::load(Path::new(&dir), name).expect("tuning");
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing)
        .with_vehicle(data.vehicle.as_ref())
        .with_melee(&data.melee_weapons);
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    let name_of = |id: u32| {
        data.melee_weapons.iter().flat_map(|w| &w.clips).find(|r| tf2_core::formats::hash::crc32(r.id.as_bytes()) == id)
            .map_or(format!("{id:08x}"), |r| format!("{} ({:.0} ticks, speed {:.2}, blend {:.2})", r.id, r.clip.as_ref().map_or(0.0, |c| c.length), r.speed, r.crossfade))
    };
    for (i, w) in data.melee_weapons.iter().enumerate() {
        println!("melee weapon {i}: {:08x}, exit clip {}", w.name, name_of(w.exit_clip));
        // Every clip of his that names it in a weapon event (`007b4680` takes them from any clip).
        for (set, refs) in &data.animations.sets {
            for r in refs {
                for e in r.clip.iter().flat_map(|c| &c.weapon_events) {
                    if let tf2_core::formats::anim::WeaponEventKind::Attach { weapon, hide, unhide, clip, hide_when_done, .. } = e.kind
                        && weapon == w.name
                    {
                        println!("  {set}/{} t {:.0}: hide {hide} unhide {unhide} clip {} hide when done {hide_when_done}", r.id, e.time,
                            if clip == 0 { "-".to_owned() } else { name_of(clip) });
                    }
                }
            }
        }
    }
    // How far each blade reaches from its object's origin as its unfold and fold play.
    for w in &data.melee_weapons {
        let Some(object) = &w.object else { continue };
        let rig = tf2_core::pose::Rig::new(object);
        for r in w.clips.iter().filter(|r| r.id.starts_with("Trans")) {
            let Some(clip) = &r.clip else { continue };
            for (name, channel) in clip.names.iter().zip(&clip.channels) {
                let node = object.nodes.iter().find(|n| tf2_core::formats::hash::crc32(n.name.as_bytes()) == *name).map_or("?", |n| n.name.as_str());
                let at = |t: f32| (channel.scale.sample(t, clip.length, false), channel.own_scale.sample(t, clip.length, false),
                    channel.position.sample(t, clip.length, false));
                let scales: Vec<String> = (0..=clip.length as usize).step_by(4)
                    .map(|t| at(t as f32).0.map_or("-".to_owned(), |s| format!("{t}:{:.2}/{:.2}/{:.2}", s[0], s[1], s[2]))).collect();
                println!("  {:08x} {} {node}: scale {}", w.name, r.id, scales.join(" "));
            }
            let reach: Vec<String> = (0..=clip.length as usize).step_by(4).map(|tick| {
                let pose = rig.pose(clip, false, tick as f32);
                let tip = pose.iter().max_by(|a, b| a.translation.length().total_cmp(&b.translation.length())).unwrap();
                let scale = pose.iter().map(|a| a.matrix3.col(0).length()).fold(0.0f32, f32::max);
                format!("{tick}:({:.1},{:.1},{:.1}) s{scale:.2}", tip.translation.x, tip.translation.y, tip.translation.z)
            }).collect();
            println!("{:08x} {} reach by tick: {}", w.name, r.id, reach.join(" "));
        }
    }
    // Weapon events that name no weapon: they go to the manager's slot at their index.
    for (set, refs) in &data.animations.sets {
        for r in refs {
            for e in r.clip.iter().flat_map(|c| &c.weapon_events) {
                if let tf2_core::formats::anim::WeaponEventKind::Attach { weapon, index, hide, unhide, clip, hide_when_done, only_if_current, .. } = e.kind
                    && (weapon == 0 || std::env::var_os("ALL_EVENTS").is_some())
                {
                    let node = data.robot.nodes.iter().find(|n| n.name_hash == e.node).map_or("?", |n| n.name.as_str());
                    println!("event {set}/{} t {:.0} on {node}: weapon {weapon:08x} index {index} hide {hide} unhide {unhide} clip {clip:08x} hide when done {hide_when_done} only if current {only_if_current}", r.id, e.time);
                }
            }
        }
    }
    // A state's ways out, by the machine's data (`STATE=<name>`, the first attack by default).
    let named = std::env::var("STATE").unwrap_or_else(|_| "stateAttack1".into());
    if let Some(state) = t.control.states.iter().find(|s| s.name == named) {
        for tr in &state.transitions {
            let c = &t.control.connections[tr.connection];
            println!("{named} -> {} by {} (priority {}, branch {}..{}, in air {}, on ground {}, can branch {}): {:?}", tr.output.map_or("back", |o| &t.control.states[o].name),
                c.name, tr.priority, c.branch_begin, c.branch_end, c.in_air, c.on_ground, c.attack_can_branch, c.test);
            if let tf2_core::control::Test::All(list) = &c.test {
                for &k in list {
                    let d = &t.control.connections[k];
                    println!("    {k}: {} branch {}..{} in air {} on ground {} z vel {}..{}: {:?}", d.name, d.branch_begin, d.branch_end, d.in_air, d.on_ground,
                        d.min_z_velocity, d.max_z_velocity, d.test);
                }
            }
        }
    }
    for i in [15, 26, 37] {
        if let Some(c) = t.control.connections.get(i) {
            println!("connection {i}: {} branch {}..{} can branch {} adv transform {} on ground {}: {:?}", c.name, c.branch_begin, c.branch_end,
                c.attack_can_branch, c.can_adv_transform, c.on_ground, c.test);
        }
    }
    let arena = Arena::default();
    let mut s = State::new(&t);
    let bit = 1u64 << button::ATTACK_FAST;
    let mut last = String::new();
    for i in 0..120 {
        let tap = i == 5;
        let aim = (i >= aim_from).then_some(Vec2::Y);
        let input = Input { buttons_down: if tap { bit } else { 0 }, buttons_hit: if tap { bit } else { 0 }, aim, ..Default::default() };
        sim::step(&mut s, &input, &t, &arena, 0.032);
        let blades: Vec<String> = s.melee.iter().map(|axe| format!("{}{}", if axe.shown { "shown" } else { "hidden" },
            axe.object.map_or(String::new(), |o| format!(" {} at {:.1}", name_of(o.id), o.tick)))).collect();
        let now = format!("{} {} | {}", s.control_state(&t).unwrap_or("-"),
            s.action_clip(&t).map_or("-".to_owned(), |(set, id, _)| format!("{set}/{id}")), blades.join(" ; "));
        if now != last {
            println!("update {i:3} ({:.3} s): {now}", i as f32 * 0.032);
            last = now;
        }
    }
}
