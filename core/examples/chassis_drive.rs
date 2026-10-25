//! Bumblebee's car driven flat out in eight directions on a level for eight seconds each, his chassis
//! through the game's contact pass (`World::move_chassis`): where it ends, how high and low it went, how
//! many chassis contacts, and the speed left. A check that the pass neither lets the car through walls
//! nor drops it through the ground, and does not hold it in the open.
//!
//! cargo run -p tf2-core --release --example chassis_drive -- [x y] [zone]
use glam::Vec3;
use tf2_core::level_world::{LevelWorld, NO_FLOOR};
use tf2_core::sim::{self, Input, Mode, State, World};
use tf2_core::tuning;

/// The level without its chassis pass: the car takes the old cylinder push (`--old`), for comparison.
struct Old<'a>(&'a LevelWorld);
impl World for Old<'_> {
    fn ledge(&self, from: Vec3, toward: glam::Vec2, reach: f32) -> Option<tf2_core::climb::Ledge> { self.0.ledge(from, toward, reach) }
    fn wheel_ray(&self, from: Vec3, to: Vec3) -> Option<tf2_core::vehicle::GroundHit> { self.0.wheel_ray(from, to) }
    fn floor(&self, x: f32, y: f32, z: f32) -> f32 { World::floor(self.0, x, y, z) }
    fn push_out(&self, pos: &mut Vec3, radius: f32, height: f32) -> bool { World::push_out(self.0, pos, radius, height) }
    fn move_pencil(&self, from: Vec3, pos: &mut Vec3, vel: &mut Vec3, radius: f32, height: f32) -> bool { World::move_pencil(self.0, from, pos, vel, radius, height) }
    fn front_contact(&self, pos: Vec3, facing: glam::Vec2, radius: f32, height: f32) -> bool { World::front_contact(self.0, pos, facing, radius, height) }
    fn drop_onto(&self, from: Vec3, to_z: f32, radius: f32) -> Option<Vec3> { World::drop_onto(self.0, from, to_z, radius) }
}

/// The level with the contact pass alone for the chassis (`--pass`): no kept contacts, no solver, as before
/// the world step was wired for it.
struct Pass<'a>(&'a LevelWorld);
impl World for Pass<'_> {
    fn ledge(&self, from: Vec3, toward: glam::Vec2, reach: f32) -> Option<tf2_core::climb::Ledge> { self.0.ledge(from, toward, reach) }
    fn wheel_ray(&self, from: Vec3, to: Vec3) -> Option<tf2_core::vehicle::GroundHit> { self.0.wheel_ray(from, to) }
    fn floor(&self, x: f32, y: f32, z: f32) -> f32 { World::floor(self.0, x, y, z) }
    fn push_out(&self, pos: &mut Vec3, radius: f32, height: f32) -> bool { World::push_out(self.0, pos, radius, height) }
    fn move_pencil(&self, from: Vec3, pos: &mut Vec3, vel: &mut Vec3, radius: f32, height: f32) -> bool { World::move_pencil(self.0, from, pos, vel, radius, height) }
    fn move_chassis(&self, centre: Vec3, turn: glam::Quat, next: glam::Quat, half: Vec3, velocity: &mut Vec3, angular: Vec3, dt: f32)
        -> Option<(Vec3, Vec<tf2_core::vehicle::Contact>)> { World::move_chassis(self.0, centre, turn, next, half, velocity, angular, dt) }
    fn chassis_pass(&self) -> bool { true }
    fn front_contact(&self, pos: Vec3, facing: glam::Vec2, radius: f32, height: f32) -> bool { World::front_contact(self.0, pos, facing, radius, height) }
    fn drop_onto(&self, from: Vec3, to_z: f32, radius: f32) -> Option<Vec3> { World::drop_onto(self.0, from, to_z, radius) }
}

fn main() {
    let old = std::env::args().any(|a| a == "--old");
    let pass = std::env::args().any(|a| a == "--pass");
    let a: Vec<String> = std::env::args().filter(|a| a != "--old" && a != "--pass").collect();
    let x: f32 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let y: f32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let zone = a.get(3).map(String::as_str).unwrap_or("levels/shanghai_01/zone.str");
    let dir = tf2_core::install::dir_string();
    let dir = std::path::Path::new(&dir);
    let mut t = tuning::load(dir, "Bumblebee").unwrap();
    let data = tf2_core::character::load(dir, "bumblebee").expect("character");
    t.locomotion = tuning::Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = tuning::Actions::from_animations(&data.animations, &data.robot, &mut t.missing);
    let level = LevelWorld::load(dir, zone).expect("level");
    let world: &dyn World = if old { &Old(&level) } else if pass { &Pass(&level) } else { &level };
    let ground = level.floor(x, y, 300.0);
    assert!(ground > NO_FLOOR, "nothing under ({x}, {y})");
    let started = std::time::Instant::now();
    for k in 0..8 {
        let yaw = k as f32 * std::f32::consts::FRAC_PI_4;
        let mut s = State::new(&t);
        s.pos = Vec3::new(x, y, ground);
        s.yaw = yaw;
        for _ in 0..30 {
            sim::step(&mut s, &Input::default(), &t, world, 0.032);
        }
        let start = s.pos;
        let (mut low, mut high, mut contacts, mut top_speed) = (s.pos.z, s.pos.z, 0usize, 0.0f32);
        let mut drove = 0;
        let mut last: Option<tf2_core::vehicle::Contact> = None;
        for i in 0..250 {
            sim::step(&mut s, &Input { vehicle: 1.0, ..Default::default() }, &t, world, 0.032);
            assert!(s.pos.is_finite() && s.vel.is_finite(), "heading {} step {i}", k * 45);
            low = low.min(s.pos.z);
            high = high.max(s.pos.z);
            contacts += s.vehicle_contacts.len();
            if let Some(c) = s.vehicle_contacts.last() { last = Some(*c); }
            top_speed = top_speed.max(s.vel.length());
            drove += (s.mode == Mode::Drive) as usize;
        }
        let ground_end = level.floor(s.pos.x, s.pos.y, s.pos.z + 3.0);
        println!(
            "heading {:3} deg: went {:6.1} m, end {:8.2} {:8.2} {:6.2} (floor there {:6.2}), z {:6.2}..{:6.2}, {:4} contacts, top {:5.1} m/s, end {:5.1} m/s, {:3} steps driving",
            k * 45,
            (s.pos - start).truncate().length(),
            s.pos.x,
            s.pos.y,
            s.pos.z,
            ground_end,
            low,
            high,
            contacts,
            top_speed,
            s.vel.length(),
            drove
        );
        if let Some(c) = last {
            let key = c.key as usize;
            let what = key.checked_sub(1).and_then(|i| level.world.primitives.get(i)).map(|p| p.source.clone()).unwrap_or_default();
            let what: String = what.chars().take(110).collect();
            println!("    last contact: primitive {} at {:.2?} normal {:.2?}, {what}", key as i64 - 1, c.point, c.normal);
        }
    }
    println!("in {:.2?}", started.elapsed());
}
