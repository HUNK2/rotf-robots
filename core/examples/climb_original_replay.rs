//! Replay the original run1 climbing-up and pull-up reference against installed clips.
//! Run with `cargo run -p tf2-core --example climb_original_replay`.
//! This is a port replay using original-recording measurements, not an original-game run.

use std::path::Path;

use glam::{Vec2, Vec3};
use tf2_core::climb::{self, Body, Climb, Ledge, Phase, Wall, message};
use tf2_core::character;
use tf2_core::tuning::{self, Actions, Locomotion};

const DT: f32 = 0.032;
const RUN1_UP_DISTANCE: (f32, f32) = (7.1, 7.3); // per ~0.775 s [trace]
const RUN1_PULLUP_SECONDS: (f32, f32) = (0.90, 0.98); // 56 clip ticks [trace]

fn main() {
    let dir = tf2_core::install::dir_string();
    let mut t = tuning::load(Path::new(&dir), "Bumblebee").expect("tuning");
    let data = character::load(Path::new(&dir), "bumblebee").expect("character");
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing);
    assert!(t.missing.is_empty(), "missing installed data: {:?}", t.missing);

    // The first run1 wall is 43.3 m high. This axis-aligned replay uses the same measured
    // dimensions and starts at the recorded 23.3 m wall-local height.
    let wall = Wall::from_ledge(
        &Ledge {
            hit: Vec2::ZERO,
            normal: Vec2::NEG_Y,
            bottom: 0.0,
            top: 43.3,
            height_adjustment: 0.0,
            left: 53.5,
            right: 53.5,
            left_corner: None,
            right_corner: None,
            extra: 0.0,
        },
        t.robot_radius,
    );
    let mut pos = wall.world(Vec3::new(53.5, -t.robot_radius, 23.3));
    let mut yaw = 0.0;
    let mut climb = Climb::enter(wall, &mut pos, Vec3::ZERO, &t, t.robot_radius, true);

    let mut elapsed = 0.0;
    let mut entered_up = false;
    let mut up_start_z = 0.0;
    let mut up_start_time = 0.0;
    let mut first_up_done = false;
    let mut pullup_start = None;
    let mut pullup_finished = None;
    let limit = 5.0 / DT;
    for _ in 0..limit as usize {
        let old_phase = climb.phase;
        let old_clip = climb.clip_data(&t).map(|clip| clip.id.as_str()).unwrap_or("?").to_owned();
        let pos_before = pos;
        let body = Body {
            pos: &mut pos,
            yaw: &mut yaw,
            radius: t.robot_radius,
            height: t.robot_height,
            on_ground: false,
            front_contact: false,
        };
        let (root_velocity, state_message) =
            climb::update(&mut climb, body, Vec2::Y, Some(wall), &t, DT);
        pos += root_velocity * DT;
        elapsed += DT;

        let clip = climb.clip_data(&t).map(|clip| clip.id.as_str()).unwrap_or("?");
        if old_phase != climb.phase || old_clip != clip {
            let local = wall.local(pos);
            println!(
                "{elapsed:5.3}s {old_phase:?}/{old_clip} -> {:?}/{clip}, hand={}, phase={:.3}, wall=({:.2},{:.2},{:.2})",
                climb.phase,
                if climb.right_hand { "R" } else { "L" },
                climb.tick / climb.clip_data(&t).map_or(1.0, |data| data.ticks),
                local.x,
                local.y,
                local.z,
            );
        }

        if climb.phase == Phase::Up && !entered_up {
            entered_up = true;
            up_start_z = wall.local(pos_before).z;
            up_start_time = elapsed - DT;
        }
        if entered_up && !first_up_done && clip == "WallR_ClimbUp" && old_clip != clip {
            let local = wall.local(pos);
            let distance = local.z - up_start_z;
            let duration = elapsed - up_start_time;
            println!("run1 up-cycle check: {distance:.3} m in {duration:.3} s [trace 7.1..7.3 m in ~0.775 s]");
            assert!(RUN1_UP_DISTANCE.0 <= distance && distance <= RUN1_UP_DISTANCE.1,
                "first WallR_ClimbUp cycle distance {distance:.3} m differs from run1");
            assert!((0.74..=0.82).contains(&duration), "up-cycle duration {duration:.3} s");
            first_up_done = true;
        }
        if state_message == Some(message::EXIT_PULLUP) && climb.phase == Phase::PullUp && pullup_start.is_none() {
            pullup_start = Some((elapsed, wall.local(pos)));
        }
        if climb.phase == Phase::PullUp && climb.finished {
            pullup_finished = Some(elapsed);
        }
        if pullup_finished.is_some() {
            break;
        }
    }

    assert!(first_up_done, "replay did not complete the measured first up clip");
    let (started, start) = pullup_start.expect("replay did not cross into PullUp");
    let finished = pullup_finished.expect("PullUp clip did not finish");
    let duration = finished - started;
    let end = wall.local(pos);
    println!(
        "run1 pull-up check: {duration:.3} s [trace 0.90..0.98 s], wall z {:.3}->{:.3}, forward {:.3} m",
        start.z,
        end.z,
        end.y - start.y,
    );
    assert!(RUN1_PULLUP_SECONDS.0 <= duration && duration <= RUN1_PULLUP_SECONDS.1,
        "pull-up duration {duration:.3} s differs from run1");
}
