//! Bumblebee's movement code run on one of the game's own levels (`level_world::LevelWorld`),
//! with no window: he is dropped onto the street, walks, then drives. A check that the
//! level holds him up and stops him, printed every half second.
//!
//! cargo run --manifest-path C:/Bumblebee/Cargo.toml -p tf2-core --release --example level_walk -- [zone, default levels/shanghai_01/zone.str] [x y]
use glam::{Vec2, Vec3};
use tf2_core::level_world::{LevelWorld, NO_FLOOR};
use tf2_core::sim::{self, Input, State, World};
use tf2_core::tuning;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    let dir = std::path::Path::new(&dir);
    let zone = args.get(1).map(String::as_str).unwrap_or("levels/shanghai_01/zone.str");
    let mut t = tuning::load(dir, "Bumblebee").unwrap();
    // On foot his speeds come from his clips: without them he walks on the spot.
    let data = tf2_core::character::load(dir, "bumblebee").expect("character");
    t.locomotion = tuning::Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = tuning::Actions::from_animations(&data.animations, &data.robot, &mut t.missing);
    let started = std::time::Instant::now();
    let level = LevelWorld::load(dir, zone).expect("level");
    println!("{zone}: {} placed primitives, loaded in {:.2?}", level.world.primitives.len(), started.elapsed());

    let x: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let y: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let ground = level.floor(x, y, 300.0);
    assert!(ground > NO_FLOOR, "nothing under ({x}, {y})");
    println!("the floor under ({x}, {y}) is at z = {ground:.3}");

    // The control: the same walk on the flat arena, to tell the level's doing from the input's.
    let mut c = State::new(&t);
    for _ in 0..60 {
        sim::step(&mut c, &Input { stick: Vec2::Y, pad_stick: Vec2::Y, ..Default::default() }, &t, &sim::Arena::default(), 0.032);
    }
    println!("control, flat arena: after 60 walking steps pos {:.2} {:.2}, speed {:.2}", c.pos.x, c.pos.y, c.vel.truncate().length());

    let mut s = State::new(&t);
    s.pos = Vec3::new(x, y, ground + 5.0);
    let phases: [(&str, usize, Input); 3] = [
        ("fall and stand", 60, Input::default()),
        ("walk +y", 250, Input { stick: Vec2::Y, pad_stick: Vec2::Y, ..Default::default() }),
        ("drive", 300, Input { vehicle: 1.0, ..Default::default() }),
    ];
    let started = std::time::Instant::now();
    let mut steps = 0;
    for (name, count, input) in phases {
        println!("-- {name}");
        for i in 0..count {
            sim::step(&mut s, &input, &t, &level, 0.032);
            steps += 1;
            assert!(s.pos.is_finite() && s.vel.is_finite(), "{name}, step {i}");
            if i % 16 == 15 || i + 1 == count {
                let floor = level.floor(s.pos.x, s.pos.y, s.pos.z);
                println!(
                    "  t {:6.2}  pos {:8.2} {:8.2} {:7.3}  speed {:6.2}  floor {:7.3}  {:?}",
                    steps as f32 * 0.032,
                    s.pos.x,
                    s.pos.y,
                    s.pos.z,
                    s.vel.truncate().length(),
                    floor,
                    s.mode
                );
            }
        }
    }
    println!("{steps} steps in {:.2?} ({:.1?} per step)", started.elapsed(), started.elapsed() / steps as u32);
}
