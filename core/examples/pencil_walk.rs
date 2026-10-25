//! Bumblebee walked in eight directions on a level for five seconds each, his walls through the
//! game's contact pass (`World::move_pencil`): where he ends, how high, and how fast he still went.
//! A check that the pass neither lets him through walls nor holds him in the open.
//!
//! cargo run -p tf2-core --release --example pencil_walk -- [x y] [zone]
use glam::{Vec2, Vec3};
use tf2_core::level_world::{LevelWorld, NO_FLOOR};
use tf2_core::sim::{self, Input, State, World};
use tf2_core::tuning;

/// The level as the enemies' world sees it (`--ai`): an AI character's masks and ground probe.
struct Ai<'a>(&'a LevelWorld);
impl World for Ai<'_> {
    fn floor(&self, x: f32, y: f32, z: f32) -> f32 { World::floor(self.0, x, y, z) }
    fn push_out(&self, pos: &mut Vec3, radius: f32, height: f32) -> bool { World::push_out(self.0, pos, radius, height) }
    fn step_avatar(&self, body: &mut tf2_core::physics_avatar::AvatarBody, req: &tf2_core::physics_avatar::AvatarRequest)
        -> Option<tf2_core::physics_avatar::FeetStep> {
        World::step_avatar(self.0, body, &tf2_core::physics_avatar::AvatarRequest { ai: true, ..*req })
    }
    fn ai_bodies(&self) -> bool { true }
}

fn main() {
    let ai = std::env::args().any(|a| a == "--ai");
    let a: Vec<String> = std::env::args().filter(|a| a != "--ai").collect();
    let x: f32 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let y: f32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let zone = a.get(3).map(String::as_str).unwrap_or("levels/shanghai_01/zone.str");
    let dir = tf2_core::install::dir_string();
    let dir = std::path::Path::new(&dir);
    let mut t = tuning::load(dir, "Bumblebee").unwrap();
    let data = tf2_core::character::load(dir, "bumblebee").expect("character");
    t.locomotion = tuning::Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = tuning::Actions::from_animations(&data.animations, &data.robot, &mut t.missing);
    println!("masses: robot {} vehicle {}; missing {:?}", t.robot_mass, t.vehicle_mass, t.missing.iter().filter(|m| m.contains("mass")).collect::<Vec<_>>());
    let level = LevelWorld::load(dir, zone).expect("level");
    let world: &dyn World = if ai { &Ai(&level) } else { &level };
    let ground = level.floor(x, y, 300.0);
    assert!(ground > NO_FLOOR, "nothing under ({x}, {y})");
    let started = std::time::Instant::now();
    for k in 0..8 {
        let angle = k as f32 * std::f32::consts::FRAC_PI_4;
        let stick = Vec2::new(angle.sin(), angle.cos());
        let mut s = State::new(&t);
        s.pos = Vec3::new(x, y, ground);
        for _ in 0..30 {
            sim::step(&mut s, &Input::default(), &t, world, 0.032);
        }
        let start = s.pos;
        if k == 0 {
            println!("after 30 idle steps: feet {:.4} above the floor probe's {ground:.4}, velocity {:?}", s.pos.z - ground, s.vel);
        }
        let (mut stopped, mut max_z) = (0, s.pos.z);
        for _ in 0..156 {
            sim::step(&mut s, &Input { stick, pad_stick: stick, ..Default::default() }, &t, world, 0.032);
            assert!(s.pos.is_finite() && s.vel.is_finite());
            stopped += (s.vel.truncate().length() < 0.5) as usize;
            max_z = max_z.max(s.pos.z);
        }
        let went = (s.pos - start).truncate();
        println!(
            "heading {:3} deg: went {:6.2} m (along {:6.2}), end {:8.2} {:8.2} {:6.2}, highest {:6.2}, {:3} of 156 steps under 0.5 m/s, mode {:?}",
            k * 45,
            went.length(),
            went.dot(stick),
            s.pos.x,
            s.pos.y,
            s.pos.z,
            max_z,
            stopped,
            s.mode
        );
    }
    println!("in {:.2?}", started.elapsed());
}
