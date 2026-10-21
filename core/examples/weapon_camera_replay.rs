//! Replays recorded weapon-camera stretches through `tf2_core::camera` and prints how far its eye
//! ends up from the recorded one. The rows come from `work\weapon_camera_export.py` (columns in its
//! docstring); each stretch starts from the recorded view.
//! `cargo run -p tf2-core --example weapon_camera_replay -- <AttributesName> <file.csv> [stretch]`

use std::path::Path;

use glam::{Vec2, Vec3};
use tf2_core::camera::{Camera, Open, Target, View};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    let mut tuning = tf2_core::tuning::load(Path::new(&dir), &args[0]).expect("tuning");
    let only: Option<usize> = args.get(2).map(|a| a.parse().expect("stretch"));
    let rows: Vec<Vec<f32>> = std::fs::read_to_string(&args[1])
        .expect("read")
        .lines()
        .map(|line| line.split(',').map(|v| v.parse().unwrap()).collect())
        .collect();
    let target_of = |r: &[f32], height_offset: f32, swings: bool| {
        let aim = Vec3::new(r[6], r[7], r[8]);
        // The recorded look height (+0x20) already has the settings' heightOffset in it, and
        // when the point swings on his radius the update has lowered it by radius * (1 -
        // |aim.xy|) for the floor test and left it so (`FUN_009284e0`) [game]: put that back.
        let unit = aim.normalize_or(Vec3::Y);
        let lowered = if swings { r[9] * (1.0 - unit.truncate().length()) } else { 0.0 };
        Target {
            position: Vec3::new(r[2], r[3], r[4]),
            height: r[5] + lowered - height_offset,
            facing: aim.truncate().normalize_or(Vec2::Y),
            aim: Some(aim),
            radius: r[9],
            ..Default::default()
        }
    };
    let (mut all_sum, mut all_count) = (0.0f32, 0u32);
    let mut start = 0;
    while start < rows.len() {
        let stretch = rows[start][0];
        let end = rows[start..].iter().position(|r| r[0] != stretch).map_or(rows.len(), |n| start + n);
        let part = &rows[start..end];
        start = end;
        if only.is_some_and(|s| s as f32 != stretch) || part.len() < 2 {
            continue;
        }
        // The recorded settings' maxDist names the level.
        let Some(level) = tuning.weapon_camera_levels.iter().position(|s| (s.max_dist - part[0][10]).abs() < 0.01) else {
            println!("stretch {stretch}: no level with maxDist {}", part[0][10]);
            continue;
        };
        tuning.weapon_camera = tuning.weapon_camera_levels[level].clone();
        let offset = tuning.weapon_camera.height_offset;
        let first = &part[0];
        let s = &tuning.weapon_camera;
        let swings = s.ahead_offset.abs() < 0.1 && first[9] * 0.25 > s.side_offset;
        let view = View {
            eye: Vec3::new(first[11], first[12], first[13]),
            forward: Vec3::new(first[14], first[15], first[16]),
            fov: tuning.weapon_camera.fov,
            roll: 0.0,
        };
        let mut camera = Camera::from_view(view, &target_of(first, offset, swings), &tuning);
        // A fresh camera has none of the point's side and ahead offset the recorded one already
        // had, and the distance it takes at the start keeps that error (the weapon camera moves
        // its distance only outside [minDist, maxDist]). So the offset is let settle for WARM
        // seconds (default 1), the camera is put back on the recorded view, and only then counted.
        let warm: f32 = std::env::var("WARM").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
        let mut counted = 0usize;
        let (mut sum, mut worst, mut worst_t) = (0.0f32, 0.0f32, 0.0f32);
        let (mut away_sum, mut up_sum) = (0.0f32, 0.0f32);
        for pair in part.windows(2) {
            let (last, row) = (&pair[0], &pair[1]);
            let got = camera.step(&target_of(row, offset, swings), Vec2::ZERO, &tuning, &Open, row[1] - last[1]);
            if row[1] - first[1] < warm {
                camera.reseat(View {
                    eye: Vec3::new(row[11], row[12], row[13]),
                    forward: Vec3::new(row[14], row[15], row[16]),
                    fov: got.fov,
                    roll: 0.0,
                });
                continue;
            }
            counted += 1;
            let eye = Vec3::new(row[11], row[12], row[13]);
            let at = Vec3::new(row[2], row[3], row[4]);
            let error = (got.eye - eye).length();
            sum += error;
            away_sum += (got.eye - at).length() - (eye - at).length();
            up_sum += (got.eye - at).z - (eye - at).z;
            if error > worst {
                (worst, worst_t) = (error, row[1]);
            }
            if only.is_some() {
                let (distance, elevation, offset, _) = camera.internals();
                println!(
                    "{:8.3} aim.z {:5.2} off {:5.2} | recorded {:5.2} away {:5.2} up | here {:5.2} away {:5.2} up \
                     (distance {:5.2}, elevation {:5.1}, offset {:5.2} {:5.2} {:5.2}; look {:5.2} up; eye pitch {:5.1} here, {:5.1} recorded)",
                    row[1], row[8], error, (eye - at).length(), (eye - at).z, (got.eye - at).length(), (got.eye - at).z,
                    distance, elevation.to_degrees(), offset.x, offset.y, offset.z,
                    row[5],
                    (got.eye - at - Vec3::Z * row[5]).z.atan2((got.eye - at).truncate().length()).to_degrees(),
                    (eye - at - Vec3::Z * row[5]).z.atan2((eye - at).truncate().length()).to_degrees(),
                );
            }
        }
        if counted == 0 {
            continue;
        }
        let n = counted as f32;
        all_sum += sum;
        all_count += counted as u32;
        println!(
            "stretch {stretch:3} level {level} {:7.2}-{:7.2} s: eye off by {:.2} on average, {:.2} at worst ({:.2} s); \
             farther by {:+.2}, higher by {:+.2} on average",
            part[0][1], part[part.len() - 1][1], sum / n, worst, worst_t, away_sum / n, up_sum / n
        );
    }
    println!("{all_count} samples: eye off by {:.2} on average", all_sum / all_count.max(1) as f32);
}
