//! A bare Bevy scene (a turning cube, a light, a camera; none of the game's code) that logs every frame
//! over 15 ms, to tell the game's hitches from the machine's or the engine's (2026-10-23, `--fps`).
//! `cargo run -p bumblebee --example bare_hitch -- [seconds]`; vsync off, full speed unfocused.
use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy::winit::WinitSettings;

#[derive(Resource)]
struct Until(f32);

fn main() {
    let seconds = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(30.0);
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { resolution: (1280, 720).into(), present_mode: PresentMode::AutoNoVsync, ..default() }),
            ..default()
        }))
        .insert_resource(WinitSettings::continuous())
        .insert_resource(Until(seconds))
        .add_systems(Startup, setup)
        .add_systems(Update, (turn, report))
        .run();
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.spawn((Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))), MeshMaterial3d(materials.add(Color::srgb(0.8, 0.7, 0.2))), Transform::default()));
    commands.spawn((PointLight { shadow_maps_enabled: true, ..default() }, Transform::from_xyz(4.0, 8.0, 4.0)));
    commands.spawn((Camera3d::default(), Transform::from_xyz(-2.5, 4.5, 9.0).looking_at(Vec3::ZERO, Vec3::Y)));
}

fn turn(time: Res<Time>, mut cubes: Query<&mut Transform, With<Mesh3d>>) {
    for mut t in &mut cubes {
        t.rotate_y(time.delta_secs());
    }
}

fn report(time: Res<Time>, until: Res<Until>, mut window: Local<(f32, u32, f32)>, mut exit: MessageWriter<AppExit>) {
    let dt = time.delta_secs();
    window.0 += dt;
    window.1 += 1;
    window.2 = window.2.max(dt);
    if dt > 0.015 && time.elapsed_secs() > 1.0 {
        info!("hitch {:.1} ms at {:.3} s", dt * 1000.0, time.elapsed_secs());
    }
    if window.0 >= 2.0 {
        info!("fps {:.1} ({:.2} ms a frame, slowest {:.1} ms)", window.1 as f32 / window.0, window.0 * 1000.0 / window.1 as f32, window.2 * 1000.0);
        *window = (0.0, 0, 0.0);
    }
    if time.elapsed_secs() > until.0 {
        exit.write(AppExit::Success);
    }
}
