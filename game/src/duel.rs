//! F3: a second robot in the same arena, driven by a person in a second window. [port] A debug
//! aid with no game counterpart (the user asked for it, 2026-10-21, to try the robots against
//! each other).
//!
//! The second robot is an `EnemyTarget` whose brain is replaced by input (`combat::Driven`), so it
//! has the enemies' model, animation, gun, damage and death, and the player's weapons and melee
//! already strike it. Its own follow and weapon camera (`camera::Rig`) draws into the second
//! window. Whichever window has focus takes the keyboard, the mouse and the pads, through the
//! install's default bindings, as `player::read_input` reads them.
//!
//! Not in (it is the enemies' path, not the player's): the weapons beyond its first robot gun, the
//! special, the car's model and gun (it changes form, but the robot model is drawn), the HUD, its
//! sounds beyond the gun's, the camera shakes and the post-processing in its window.
//! `--p2 <name>` says who it is (a roster name or pack, as `--character`).

use std::path::PathBuf;
use std::sync::Arc;

use bevy::camera::RenderTarget;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, WindowRef};

use crate::assets::{self, game_to_bevy};
use crate::camera::{self, Rig};
use crate::player::Player;
use crate::control::button;
use crate::sim::Input;
use crate::targets::{EnemyTarget, NavRes};
use crate::world::ArenaRes;
use tf2_core::combat::{Driven, Scout};
use tf2_core::formats::characterlist::Spec;

/// The second robot's actor id: clear of the arena's scouts (1..) and the spawners' enemies.
const ID: u32 = 9001;
/// Where it starts: this far in front of the player, facing him.
const START: f32 = 20.0;

/// The second robot, its window and its camera, while F3 has them on.
#[derive(Resource)]
pub struct Duel {
    game_dir: PathBuf,
    spec: Spec,
    scout: Option<Arc<Scout>>,
    robot: Option<Entity>,
    window: Option<Entity>,
    camera: Option<Entity>,
    text: Option<Entity>,
    rig: Option<Rig>,
    /// The second window has focus: the input is the second robot's, and the player gets none.
    pub focused: bool,
    input: Option<tf2_core::input::InputPath>,
    last: u32,
    aiming: bool,
}

impl Duel {
    pub fn new(game_dir: PathBuf, spec: Spec) -> Self {
        Self {
            game_dir, spec, scout: None, robot: None, window: None, camera: None, text: None, rig: None,
            focused: false, input: None, last: 0, aiming: false,
        }
    }
}

/// The second window's camera: the systems that work on the main camera leave it out.
#[derive(Component)]
pub struct SecondView;

/// The second robot's owner entity.
#[derive(Component)]
pub struct Driven2;

#[derive(Component)]
pub struct SecondText;

/// The second robot's vehicle model.
#[derive(Component)]
pub struct P2Vehicle;

/// Swaps the second robot's robot model for its vehicle model while it is in vehicle form.
/// [stand-in] the vehicle model stays in its rest pose: no wheel spin, steering, suspension or lean,
/// and no vehicle clips for the change of form (the player's `VehicleAnimator` path is not run for
/// it). settles: driving a `VehicleAnimator` from the enemy's state as `animation::animate` does.
/// A flyer whose flying form is the robot itself (`flies_as_robot`) keeps the robot drawn.
pub fn forms(
    robots: Query<(&EnemyTarget, &Children), With<Driven2>>,
    mut robot_models: Query<&mut Visibility, (With<crate::targets::EnemyModel>, Without<P2Vehicle>)>,
    mut vehicle_models: Query<&mut Visibility, With<P2Vehicle>>,
) {
    let Ok((enemy, children)) = robots.single() else { return };
    let s = &enemy.combat.state;
    let car = s.is_vehicle() && !s.flies_as_robot(&enemy.scout.tuning);
    for child in children.iter() {
        if let Ok(mut v) = vehicle_models.get_mut(child) {
            *v = if car { Visibility::Inherited } else { Visibility::Hidden };
        } else if let Ok(mut v) = robot_models.get_mut(child) {
            *v = if car && !s.dead() { Visibility::Hidden } else { Visibility::Inherited };
        }
    }
}

/// F3 turns the second robot and its window on and off. The first time, its pack is read here
/// (a pause of a second or two).
#[allow(clippy::too_many_arguments)]
pub fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut duel: ResMut<Duel>,
    mut arena: ResMut<ArenaRes>,
    nav: Res<NavRes>,
    player: Single<&Player>,
    windows: Query<(), With<Window>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surfaces: ResMut<Assets<crate::surface::Surface>>,
    mut glows: ResMut<Assets<crate::fx::Glow>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut started: Local<bool>,
) {
    // `--duel` turns it on at the start, for captures.
    let at_start = !std::mem::replace(&mut *started, true) && std::env::args().any(|a| a == "--duel");
    // Its window closed by hand counts as F3 off.
    let closed = duel.window.is_some_and(|w| windows.get(w).is_err());
    if !keys.just_pressed(KeyCode::F3) && !closed && !at_start {
        return;
    }
    if duel.robot.is_some() {
        for entity in [duel.robot.take(), duel.camera.take(), duel.text.take(), duel.window.take()].into_iter().flatten() {
            if let Ok(mut e) = commands.get_entity(entity) {
                e.despawn();
            }
        }
        duel.rig = None;
        duel.focused = false;
        arena.0.targets.retain(|t| t.id != ID);
        arena.0.bodies.retain(|b| b.id != ID);
        println!("duel: second robot off");
        return;
    }
    if closed {
        return;
    }
    if duel.scout.is_none() {
        let started = std::time::Instant::now();
        match Scout::load_as(&duel.game_dir, &duel.spec.pack, &duel.spec.attributes) {
            Ok(scout) => {
                println!("duel: loaded {} (pack {}, attributes {}) in {:.2} s", duel.spec.display.as_deref().unwrap_or(&duel.spec.pack), duel.spec.pack, duel.spec.attributes, started.elapsed().as_secs_f32());
                duel.scout = Some(Arc::new(scout));
            }
            Err(e) => {
                eprintln!("duel: could not load {}: {e}", duel.spec.pack);
                return;
            }
        }
    }
    let scout = Arc::clone(duel.scout.as_ref().expect("loaded above"));
    let me = &player.state;
    let spawn = me.pos + me.facing().extend(0.0) * START;
    let mut combat = crate::targets::new_enemy(ID, spawn, &scout, &nav);
    combat.state.yaw = me.yaw + std::f32::consts::PI;
    let start = combat.state.clone();
    let target = combat.target(&scout.tuning);
    let robot = commands
        .spawn((
            Transform::from_translation(game_to_bevy(spawn)),
            Visibility::Inherited,
            Driven2,
            EnemyTarget {
                id: ID,
                combat,
                health: scout.tuning.base_health,
                down_for: 0.0,
                reset_serial: player.reset_serial,
                scout: Arc::clone(&scout),
                spawner: None,
                held: false,
            },
        ))
        .id();
    let mut cache = assets::Cache::default();
    let mut builder = assets::Builder {
        library: &scout.character.library,
        meshes: &mut meshes,
        materials: &mut materials,
        surfaces: &mut surfaces,
        glows: &mut glows,
        images: &mut images,
        bindposes: &mut bindposes,
        cache: &mut cache,
    };
    crate::targets::spawn_models(&mut commands, &mut builder, robot, &scout);
    // Its car (or jet) model, shown in place of the robot while it is in vehicle form (`forms`). [port]
    if let Some(vehicle) = &scout.character.vehicle {
        let (model, _) = builder.spawn(&mut commands, vehicle, "p2 vehicle");
        commands.entity(model).insert((P2Vehicle, Visibility::Hidden));
        commands.entity(robot).add_child(model);
    }
    let name = duel.spec.display.clone().unwrap_or_else(|| duel.spec.pack.clone());
    let window = commands
        .spawn(Window { title: format!("Player 2: {name} (F3 closes)"), resolution: (1280, 720).into(), ..default() })
        .id();
    let t = &scout.tuning;
    let camera = commands
        .spawn((
            Camera3d::default(),
            DepthPrepass,
            Projection::Perspective(PerspectiveProjection { fov: t.robot_camera.fov.to_radians(), near: 0.5, far: 1500.0, ..default() }),
            Transform::default(),
            RenderTarget::Window(WindowRef::Entity(window)),
            // The sky domes' layer, as the main view has it (`ocean::setup`).
            bevy::camera::visibility::RenderLayers::from_layers(&[0, crate::ocean::NOT_REFLECTED]),
            SecondView,
        ))
        .id();
    if let Some(look) = crate::world::camera_look() {
        commands.entity(camera).insert(look);
    }
    let text = commands
        .spawn((
            Text::new(""),
            Node { position_type: PositionType::Absolute, top: px(12), left: px(12), ..default() },
            UiTargetCamera(camera),
            SecondText,
        ))
        .id();
    duel.rig = Some(Rig::new(&start, t, None, None));
    duel.robot = Some(robot);
    duel.window = Some(window);
    duel.camera = Some(camera);
    duel.text = Some(text);
    if let Some(target) = arena.0.targets.iter().position(|t| t.id == ID) {
        arena.0.targets.remove(target);
    }
    arena.0.targets.push(target);
    println!("duel: second robot on: {name} at ({:.1}, {:.1}); click its window to control it, F3 to close", spawn.x, spawn.y);
}

/// The second robot's input of this frame, from the devices while its window has focus (none
/// otherwise), turned into the movement's input as `player::tick` turns the player's.
#[allow(clippy::too_many_arguments)]
pub fn drive(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    wheel: Res<AccumulatedMouseScroll>,
    pads: Query<&Gamepad>,
    mut windows: Query<(Entity, &mut Window, &mut CursorOptions)>,
    mut duel: ResMut<Duel>,
    mut arena: ResMut<ArenaRes>,
    nav: Res<NavRes>,
    player: Single<&Player>,
    mut robots: Query<(&mut EnemyTarget, &mut Visibility), With<Driven2>>,
) {
    use tf2_core::input::{bit, Devices, Filtered, Mouse, L2_BIT, R2_BIT};
    let duel = &mut *duel;
    let (Some(window), Some(rig)) = (duel.window, duel.rig.as_mut()) else {
        duel.focused = false;
        return;
    };
    let Ok((mut robot, mut visibility)) = robots.single_mut() else { return };
    let Ok((_, win, _)) = windows.get(window) else { return };
    duel.focused = win.focused;
    // F4 hands the focus to the other window. A window without focus lets go of the mouse, so the
    // pointer can reach the other one.
    let swap = keys.just_pressed(KeyCode::F4);
    for (entity, mut win, mut cursor) in &mut windows {
        if swap && (entity == window) != duel.focused {
            win.focused = true;
        }
        let own = entity == window;
        let grab = own && duel.focused && buttons.just_pressed(MouseButton::Left);
        if grab {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        } else if (swap || !win.focused || (own && keys.just_pressed(KeyCode::Escape))) && cursor.grab_mode != CursorGrabMode::None {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }
    let Ok((_, _, cursor)) = windows.get(window) else { return };
    let scout = Arc::clone(&robot.scout);
    let t = &scout.tuning;
    // Dead and gone: back at its start, as the player is after his restart.
    if robot.combat.state.restart_requested {
        let spawn = robot.combat.spawn;
        let yaw = robot.combat.state.yaw;
        robot.combat = crate::targets::new_enemy(ID, spawn, &scout, &nav);
        robot.combat.state.yaw = yaw;
        *visibility = Visibility::Inherited;
        *rig = Rig::new(&robot.combat.state, t, None, None);
        arena.0.targets.retain(|t| t.id != ID);
        arena.0.targets.push(robot.combat.target(t));
        println!("duel: second robot back at its start");
    }
    // Opposite sides, whatever their data says, so each can hurt the other (the game filters hits
    // between the same team, `damage::receive`). [port]
    if robot.combat.state.actor.team == player.state.actor.team {
        robot.combat.state.actor.team = player.state.actor.team.wrapping_add(1);
    }
    let path = duel.input.get_or_insert_with(|| {
        let map = tf2_core::formats::bxdata::Archive::install(&duel.game_dir)
            .and_then(|a| tf2_core::input::ActionMap::install_defaults(&a, "Robot"))
            .unwrap_or_default();
        tf2_core::input::InputPath::new(map)
    });
    let on = duel.focused;
    let grabbed = on && cursor.grab_mode == CursorGrabMode::Locked;
    let mouse = if grabbed {
        let mut b = [false; 8];
        for (i, button) in [MouseButton::Left, MouseButton::Right, MouseButton::Middle, MouseButton::Back, MouseButton::Forward].into_iter().enumerate() {
            b[i] = buttons.pressed(button);
        }
        let notches = match wheel.unit {
            bevy::input::mouse::MouseScrollUnit::Line => wheel.delta.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => wheel.delta.y / 120.0,
        };
        Mouse { dx: mouse.delta.x.round() as i32, dy: mouse.delta.y.round() as i32, wheel: (notches * 120.0).round() as i32, buttons: b }
    } else {
        Mouse::default()
    };
    let pad_states: Vec<_> = if on { pads.iter().map(crate::player::xinput_pad).collect() } else { Vec::new() };
    let key_down = |name: &str| on && crate::player::key_code(name).is_some_and(|k| keys.pressed(k));
    let pad = path.poll(&Devices { key_down: &key_down, mouse, mouse_divisor: tf2_core::input::MOUSE_DIVISOR_DEFAULT, pads: &pad_states });
    let last = std::mem::replace(&mut duel.last, pad.buttons);
    let down = |bits: u32| pad.down(bits);
    let hit = |bits: u32| pad.down(bits) && last & bits == 0;

    let state = &robot.combat.state;
    let dt = time.delta_secs().min(0.05);
    // Capture aids: `--p2-stick <forward>` holds its stick, `--p2-fire` its weapon mode and trigger.
    let forced = |flag: &str| std::env::args().skip_while(|a| a != flag).nth(1).and_then(|v| v.parse::<f32>().ok());
    let forced_fire = std::env::args().any(|a| a == "--p2-fire");
    let stick = (Vec2::new(pad.lx, -pad.ly) + Vec2::Y * forced("--p2-stick").unwrap_or(0.0)).clamp_length_max(1.0);
    let look = Vec2::new(pad.rx, -pad.ry).clamp(Vec2::NEG_ONE, Vec2::ONE);
    let vehicle = state.is_vehicle();
    let trigger = if forced_fire { 1.0 } else { Filtered::trigger01(pad.rt) };
    let aiming = (down(L2_BIT) || forced_fire) && !vehicle;
    let was_aiming = std::mem::replace(&mut duel.aiming, aiming);
    let fire = aiming && (trigger > 0.1 || down(R2_BIT));
    // The aim as the player's, without the assist's target. [port]
    let looking = rig.view.forward;
    if aiming {
        let look = rig.camera.options.stick(look);
        let right = looking.cross(Vec3::Z).normalize_or(Vec3::X);
        let eye = tf2_core::aim::Camera { eye: rig.view.eye, right, forward: looking, up: right.cross(looking) };
        if !was_aiming {
            rig.aim.enter(looking, look);
        }
        let assist = tf2_core::aim::assist(look, &eye, state.vel, None, &t.aim, dt);
        rig.aim.step_assisted(look, stick.length_squared() > 0.001, state.pos, assist, &t.aim, dt);
    } else {
        rig.aim.leave();
        rig.aim.set(state.body_forward());
    }
    let aim = aiming.then(|| rig.aim.direction());
    let forward = Vec2::new(looking.x, looking.y).normalize_or(Vec2::Y);
    let right = Vec2::new(forward.y, -forward.x);
    let jump = hit(bit::A as u32);
    let melee = hit(bit::X as u32);
    let climb = hit(bit::B as u32);
    let special = hit(bit::Y as u32);
    let both = |on: bool| (on as u64) << button::ATTACK_FAST | (on as u64) << button::DODGE | (on as u64) << button::ATTACK_VEHICLE;
    let input = Input {
        stick: right * stick.x + forward * stick.y,
        steer: stick.x,
        pad_stick: stick,
        look,
        jump,
        jump_held: down(bit::A as u32),
        // `--p2-car` holds the vehicle trigger, for captures.
        vehicle: if aiming { 0.0 } else if std::env::args().any(|a| a == "--p2-car") { 1.0 } else { trigger },
        brake: if down(bit::L_SHOULDER as u32) { 1.0 } else { 0.0 },
        turbo: jump && vehicle,
        drift: down(L2_BIT),
        aim: aim.map(|a| Vec2::new(a.x, a.y)),
        buttons_down: both(down(bit::X as u32))
            | (down(bit::B as u32) as u64) << button::CLIMB
            | (down(bit::Y as u32) as u64) << button::ATTACK_SPECIAL
            | (fire as u64) << button::WEAPON_FIRE,
        buttons_hit: both(melee) | (climb as u64) << button::CLIMB | (special as u64) << button::ATTACK_SPECIAL,
    };
    // Its rounds go to the point the camera's line meets, as the player's do (`aim_point`).
    let at = tf2_core::weapons::aim_point(rig.view.eye, aim.unwrap_or(looking), state.pos, &arena.0).point;
    // `--p2-fire` aims at his chest, not along the camera (the test has no one to raise the aim).
    let at = if forced_fire { player.state.pos + Vec3::Z * 3.0 } else { at };
    if std::env::var("DUEL_DBG").is_ok() {
        eprintln!(
            "duel {:.2}: pos {:.1?} state {:?} firing {} shots {} aim {:?} at {:.1?} fire {} health {:.0} / him {:.0}",
            time.elapsed_secs(), state.pos, state.control_state(t), state.firing, robot.combat.shots, aim.map(|a| a.truncate()), at, fire, state.health, player.state.health
        );
    }
    robot.combat.driven = Some(Driven { input, aim: at });
}

/// The second camera follows the second robot after it has moved, and its window shows its health.
pub fn follow(
    time: Res<Time>,
    mut duel: ResMut<Duel>,
    arena: Res<ArenaRes>,
    occluders: Res<camera::Occluders>,
    robots: Query<&EnemyTarget, With<Driven2>>,
    mut camera: Query<(&mut Transform, &mut Projection), With<SecondView>>,
    mut text: Query<&mut Text, With<SecondText>>,
    mut commands: Commands,
    mut clock: Local<f32>,
) {
    // `--duel-shot <png> [--duel-shot-after <s>]`: saves the second window once, for captures.
    let flag = |name: &str| std::env::args().skip_while(|a| a != name).nth(1);
    *clock += time.delta_secs();
    let after = flag("--duel-shot-after").and_then(|v| v.parse().ok()).unwrap_or(3.0);
    if let (Some(path), Some(window)) = (flag("--duel-shot"), duel.window)
        && *clock >= after && *clock - time.delta_secs() < after {
        use bevy::render::view::screenshot::{save_to_disk, Screenshot};
        commands.spawn(Screenshot::window(window)).observe(save_to_disk(path));
    }
    let duel = &mut *duel;
    let (Some(rig), Ok(robot)) = (duel.rig.as_mut(), robots.single()) else { return };
    let t = &robot.scout.tuning;
    let state = &robot.combat.state;
    let dt = time.delta_secs().min(0.05);
    let look = robot.combat.driven.map_or(Vec2::ZERO, |d| d.input.look);
    let aim = duel.aiming.then(|| rig.aim.direction());
    let target = camera::target(state, t, aim);
    let world = tf2_core::camera::Scene { solids: &arena.0, fadeable: &occluders.0 };
    rig.view = rig.camera.step(&target, look, t, &world, dt);
    if let Ok((mut transform, mut projection)) = camera.single_mut() {
        // Rolled as `shake::Jolt::apply` rolls the view (the car's lean), with no shake.
        let view = &rig.view;
        let level = view.forward.cross(Vec3::Z).normalize_or(Vec3::X);
        let right = level * view.roll.cos() - level.cross(view.forward) * view.roll.sin();
        *transform = Transform::from_translation(game_to_bevy(view.eye))
            .looking_to(game_to_bevy(view.forward), game_to_bevy(right.cross(view.forward)));
        if let Projection::Perspective(p) = &mut *projection {
            p.fov = rig.view.fov.clamp(1.0, 120.0).to_radians();
        }
    }
    if let Ok(mut text) = text.single_mut() {
        let name = duel.spec.display.as_deref().unwrap_or(&duel.spec.pack);
        let hint = if duel.focused { "" } else { "  (click here to control)" };
        text.0 = format!("{name}  health {:.0} / {:.0}{hint}", state.health.max(0.0), t.base_health);
    }
}
