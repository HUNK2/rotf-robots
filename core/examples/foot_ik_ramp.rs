//! A robot walking up and over a joined pair of wedges (the arena's green hill: 8 m up over 40 m
//! and down again) with the port's foot IK, without a window: per update his height, the base
//! clip, each foot's lift as the IK leaves it and the body's lowering. The base clip alone is
//! posed (no blends or layers), so this is the IK's own answer to the ground. For Breakaway's
//! and Starscream's legs on the wedges (the user's report, 2026-10-25).
//! `cargo run -p tf2-core --example foot_ik_ramp -- [pack:AttributesName] [slope] [seconds]`

use std::path::Path;

use glam::{Affine3A, Quat, Vec2, Vec3};
use tf2_core::footik::{accumulate, FootIk, FootRig};
use tf2_core::formats::anim::ROOT_CHANNEL;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::terrain::Ramp;
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet};

fn main() {
    let dir = tf2_core::install::dir_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = args.first().map_or("firestorm:Firestorm", String::as_str);
    let slope: f32 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(0.2);
    let seconds: f32 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(4.0);
    let (pack, name) = arg.split_once(':').expect("pack:Name");
    let mut t = tuning::load(Path::new(&dir), name).expect("tuning");
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing);
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    let rig = FootRig::read(&data.robot, &data.library).expect("foot rig");
    let parents: Vec<Option<usize>> = data.robot.nodes.iter().enumerate().map(|(i, n)| n.parent.filter(|&p| p < i)).collect();
    let binds: Vec<Affine3A> = data.robot.nodes.iter().map(|n| tf2_core::pose::affine(&n.bind)).collect();
    let rest: Vec<(Vec3, Quat, Vec3)> = (0..binds.len()).map(|i| match parents[i] {
        Some(p) => (binds[p].inverse() * binds[i]).to_scale_rotation_translation(),
        None => binds[i].to_scale_rotation_translation(),
    }).collect();
    let hashes: Vec<u32> = data.robot.nodes.iter().map(|n| n.name_hash).collect();
    let length = 8.0 / slope;
    let arena = Arena {
        ramps: vec![
            Ramp { min: Vec2::new(-14.0, 10.0), max: Vec2::new(14.0, 10.0 + length), base: 0.0, height: 0.0, slope: Vec2::new(0.0, slope) },
            Ramp { min: Vec2::new(-14.0, 10.0 + length), max: Vec2::new(14.0, 10.0 + 2.0 * length), base: 0.0, height: 8.0, slope: Vec2::new(0.0, -slope) },
        ],
        ..Default::default()
    };
    let mut s = State::new(&t);
    let mut ik = FootIk::default();
    let dt = sim::GAME_UPDATE;
    let input = Input { stick: Vec2::Y, ..Default::default() };
    println!("{pack}: hill of slope {slope} from y 10 to {:.0}, crest at {:.0}", 10.0 + 2.0 * length, 10.0 + length);
    for i in 0..(seconds / dt) as usize {
        sim::step(&mut s, &input, &t, &arena, dt);
        let Some(base) = s.base_clip(&t) else { continue };
        let Some(clip) = data.animations.find(base.set, base.id).and_then(|r| r.clip.as_ref().map(|c| (r, c))) else { continue };
        let (r, clip) = clip;
        let mut local: Vec<Affine3A> = rest.iter().zip(&hashes).map(|(&(scale, rotation, position), &name)| {
            let (mut sc, mut ro, mut po) = (scale, rotation, position);
            if let Some(channel) = clip.channel(name).filter(|_| name != ROOT_CHANNEL) {
                if let Some(p) = channel.position.sample(base.tick, clip.length, r.looping) { po = Vec3::from(p); }
                if let Some(q) = channel.rotation.sample(base.tick, clip.length, r.looping) { ro = Quat::from_xyzw(-q[0], -q[1], -q[2], q[3]); }
                sc = channel.scale.sample(base.tick, clip.length, r.looping).map_or(Vec3::ONE, Vec3::from);
            }
            Affine3A::from_scale_rotation_translation(sc, ro, po)
        }).collect();
        let root = Affine3A::from_rotation_translation(Quat::from_rotation_z(s.yaw), s.pos);
        let active = matches!(s.mode, sim::Mode::Stand | sim::Mode::Move) && s.ground_flag() && s.vel.z <= 0.0;
        ik.apply(&rig, &mut local, &parents, root, active, dt, &arena);
        let world = accumulate(&local, &parents);
        let feet: Vec<String> = rig.legs.iter().zip(ik.lifts()).map(|(leg, lift)| {
            let foot = root * world[leg.foot];
            format!("lift {lift:+.2} foot z {:.2}", foot.translation.z)
        }).collect();
        println!("{i:3} y {:6.2} z {:5.2} {} {:>14} t {:5.1} | body {:+.2} | {}", s.pos.y, s.pos.z, if active { "ik " } else { "off" },
            base.id, base.tick, ik.body_lift(), feet.join(" | "));
    }
}
