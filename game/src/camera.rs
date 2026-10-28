//! The game's three cameras (`tf2_core::camera`: follow, driving and weapon), shown
//! through Bevy's. Their rules were read from the original and
//! checked against a recording; this file only feeds them and places Bevy's camera.
//!
//! In weapon mode the stick turns his aim (`tf2_core::aim`) and the weapon camera follows
//! that. The mouse is the original's: a second camera stick (see
//! `MOUSE_COUNTS_FOR_FULL_STICK`).
//!
//! The camera shakes (`tf2_core::shake`) are put on the view as it is drawn; the view the
//! stick and the weapons go by is the one without them.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use tf2_core::aim::Aim;
use tf2_core::camera::{Camera, Mode, Target, View};
use tf2_core::flycam;
use tf2_core::formats::anim::Clip;
use tf2_core::shake::{Jolt, Shakes};

use crate::assets::game_to_bevy;
use crate::player::{Controls, Player, TuningRes};
use crate::sim::{ShakeStart, State};
use crate::tuning::Tuning;

/// The mouse is a stick in the original: each direction of it is a control that reads
/// counts moved this frame over this divisor, from 0 to 1 (`FUN_004f0e30`; the divisor
/// `DAT_00bf361c` is 10 in the executable; options rewrite it as
/// trunc((1.05 - SETTING_AIM_MOUSE) * 20), checked at008e8bb0). So it turns the cameras and the aim at their stick rates, and
/// more than this many counts in a frame is the stick held over. [game]
// The original's value, kept as the reference for its mouse; the modern layout does not use it.
#[allow(dead_code)]
pub const MOUSE_COUNTS_FOR_FULL_STICK: f32 = 10.0;

/// Scene importers attach this only to collision objects carrying the original fade
/// flag. Bounds are in game/world space; a moving object updates them with its pose.
/// Materials must be unique to the object, as opacity is per object in Lux. [game]
#[derive(Component)]
pub struct Fadeable {
    pub bounds: crate::sim::Box3,
    pub alpha: f32,
    pub alpha_mode: AlphaMode,
}

#[derive(Resource, Default)]
pub struct Occluders(pub Vec<tf2_core::camera::Fadeable>);

pub fn collect_occluders(query: Query<(Entity, &Fadeable)>, mut occluders: ResMut<Occluders>) {
    occluders.0.clear();
    occluders
        .0
        .extend(query.iter().map(|(entity, o)| tf2_core::camera::Fadeable {
            id: entity.to_bits(),
            bounds: o.bounds,
        }));
}

pub fn fade_occluders(
    rig: Res<Rig>,
    query: Query<(Entity, &Fadeable, &MeshMaterial3d<crate::ssd::Material>)>,
    mut materials: ResMut<Assets<crate::ssd::Material>>,
    mut report_frames: Local<u32>,
) {
    *report_frames += 1;
    for (entity, original, handle) in &query {
        if let Some(mut material) = materials.get_mut(&handle.0) {
            let alpha = rig.camera.fades.alpha(entity.to_bits());
            if *report_frames == 60 && std::env::args().any(|a| a == "--camera-fade-test") {
                eprintln!(
                    "camera fade fixture: alpha={alpha:.3}, eye={:?}, forward={:?}",
                    rig.view.eye, rig.view.forward
                );
            }
            material.base.base_color = material.base.base_color.with_alpha(original.alpha * alpha);
            material.base.alpha_mode = if alpha < 1.0 {
                AlphaMode::Blend
            } else {
                original.alpha_mode
            };
        }
    }
}

/// The length of an update of the shakes. The original runs them once a frame with the
/// frame's own length (`camera_shake_update` 0091f5d0, docs/camera.md "Shakes"), and how they
/// look depends on how often that is. The port steps them in fixed 0.032 s updates.
/// [assumed: 0.032 s, the cameras' recorded-update convention. settles: a recording of the frame durations and a shake's age (trace)]
const SHAKE_UPDATE: f32 = 0.032;

#[derive(Resource)]
pub struct Rig {
    pub camera: Camera,
    /// Where he aims: turned by the stick in weapon mode, the camera's direction otherwise.
    pub aim: Aim,
    /// Degrees the drawn picture is swung round him, for captures (`--orbit`), about the point
    /// `orbit_pivot` ahead of the camera (`--orbit-pivot`, 11 by default; negative: that height
    /// over his feet, looked at).
    pub orbit: f32,
    pub orbit_pivot: f32,
    /// How many times the drawn picture is magnified, for captures (`--zoom`); 1 is the game's.
    pub zoom: f32,
    /// The view to draw this frame from, in the game's space, before the shakes.
    pub view: View,
    pub levels: [usize; 3],
    /// The slam's finisher camera while the finisher holds a target (`sim::State::finish_target`),
    /// with the random stream its entry tosses its coin from. [game 009192c0 -> 00917b40]
    pub finisher: Option<tf2_core::camera::finisher::Finisher>,
    pub finisher_random: tf2_core::camera::spectator::Random,
    /// The camera shakes running, the time not yet given to them, and what they do to the
    /// view until their next update.
    shakes: Shakes,
    shake_time: f32,
    jolt: Jolt,
}

/// What the camera follows, from the movement's state; `aim` is his aim in weapon mode,
/// which hands him to the weapon camera and raises the point looked at by the character's
/// `cameraWeaponTargetHeightOffset`. [game]
pub fn target(state: &State, tuning: &Tuning, aim: Option<Vec3>) -> Target {
    let vehicle = state.is_vehicle();
    let flying = state.is_flying();
    let pose = state.car_presentation();
    let (yaw, pitch, _) = pose.angles();
    let up = pose.rotation * Vec3::Z;
    let right = pose.rotation * Vec3::X;
    let aim = aim.filter(|_| !vehicle);
    Target {
        position: pose.pos,
        height: if vehicle {
            // [game] 0070abe0: form height plus the bottom of the physics holder
            // above the character root. Replaces the fixed run1 rise of1.12.
            let lift = tuning.vehicle_height
                + (up.z * (tuning.drive.body_offset_z - tuning.vehicle_height * 0.5)).max(0.0);
            // A flyer's look-at is the lower of its root and its holder's bottom (0071a460).
            if flying {
                lift + flycam::look_drop(pose.pos, up.z, tuning.drive.body_offset_z, tuning.vehicle_height)
            } else {
                lift
            }
        } else {
            tuning.robot_height
                + if aim.is_some() {
                    tuning.weapon_look_height
                } else {
                    0.0
                }
        },
        facing: if vehicle {
            Vec2::new(-yaw.sin(), yaw.cos())
        } else {
            state.facing()
        },
        velocity: pose.camera_velocity,
        motion: pose.velocity,
        vehicle,
        climbing: state.climb.is_some(),
        aim,
        radius: if vehicle {
            tuning.vehicle_radius
        } else {
            tuning.robot_radius
        },
        pitch: if vehicle { pitch } else { 0.0 },
        roll: if vehicle {
            (-right.z).clamp(-1.0, 1.0).asin()
        } else {
            0.0
        },
        fighting: state.fight > 0.0,
        mode: if state.dead() {
            Mode::Fast
        }
        // [game] character dead flag requests7d110862
        // [game] 0071a5c0: while the movement owner's avatar blend (+0x248) is above zero the
        // message asks for 5e89f579 in place of the character's own mode; the death request
        // above still overrides it (00922580, bit 0x200). Only FlyMode's enter starts that
        // blend for a character the port can play: the car's enter is exempt (owner+0x3fc =
        // fc2cb442) and the robot's camera never reads the mode. `flight::Fly::avatar_blend`.
        else if state.fly.as_ref().is_some_and(|fly| flying && fly.avatar_blend > 0.0) {
            Mode::Transition
        } else if state.control_state(tuning) == Some("stateGroundPunchWeakFall") {
            Mode::GroundPunch
        } else {
            Mode::Normal
        },
        ground_distance: pose
            .wheels
            .iter()
            .map(|w| w.probe_distance)
            .fold(100.0, f32::min),
        body_height: vehicle.then_some(up.z * tuning.drive.body_offset_z),
        // [game] The camera's byte +0x195 is `character_in_flight_mode` (007256e0: the motion
        // mode type is fb9f3262, FlightMode), copied into the behaviour by the driving camera's
        // slot 17 (00916030: `+0x195 = *param_2 >> 7`). `sim::Mode::Fly` is that same class, so
        // this is the flag, not an approximation of it. It gates the elevation and eye rules of
        // 009145b0 (`core\src\camera.rs`). Wired 2026-10-18; `docs/camera.md` "A flyer's camera
        // is the driving camera".
        flying,
    }
}

impl Rig {
    /// Behind him at the camera's own distance and elevation; or, for captures, looking
    /// along `yaw` degrees (0 is +y, positive turns toward -x) from `elevation` degrees up.
    pub fn new(state: &State, tuning: &Tuning, yaw: Option<f32>, elevation: Option<f32>) -> Self {
        let target = target(state, tuning, None);
        let index = |levels: &[tf2_core::tuning::CameraTuning],
                     current: &tf2_core::tuning::CameraTuning| {
            levels
                .iter()
                .position(|s| s.min_dist == current.min_dist && s.max_dist == current.max_dist)
                .unwrap_or(0)
        };
        let levels = [
            index(&tuning.robot_camera_levels, &tuning.robot_camera),
            index(&tuning.weapon_camera_levels, &tuning.weapon_camera),
            index(&tuning.drive_camera_levels, &tuning.drive_camera),
        ];
        let mut camera = Camera::default();
        camera.reset(&target, tuning);
        if yaw.is_some() || elevation.is_some() {
            let settings = &tuning.robot_camera;
            let yaw = yaw.unwrap_or(0.0).to_radians();
            let elevation = elevation.unwrap_or(settings.default_elevation).to_radians();
            let look_at = target.position + Vec3::Z * (target.height + settings.height_offset);
            let back = Vec2::new(yaw.sin(), -yaw.cos());
            let eye =
                look_at + (back * elevation.cos()).extend(elevation.sin()) * settings.max_dist;
            camera = Camera::from_view(
                View {
                    eye,
                    forward: (look_at - eye).normalize(),
                    fov: settings.fov,
                    roll: 0.0,
                },
                &target,
                tuning,
            );
            // The smallest turn the camera takes as the player's own, so it keeps this
            // elevation instead of settling back to its default.
            camera.nudge(Vec2::new(0.0, 0.0015));
        }
        let view = camera.latest();
        let mut aim = Aim::default();
        aim.set(state.body_forward());
        Self {
            camera,
            aim,
            orbit: 0.0,
            orbit_pivot: 11.0,
            zoom: 1.0,
            view,
            levels,
            finisher: None,
            finisher_random: tf2_core::camera::spectator::Random { state: 0 },
            shakes: Shakes::default(),
            shake_time: 0.0,
            jolt: Jolt::default(),
        }
    }

    /// Starts the shakes the movement asked for this frame, each by its name in the game's
    /// list (a name the game does not have starts nothing, as in the original), and gives
    /// the running ones the frame's time.
    pub fn shake(&mut self, starts: &[ShakeStart], tuning: &Tuning, dt: f32) {
        for start in starts {
            if let Some(def) = tuning.shakes.get(&start.name) {
                self.shakes.start(*def, Some(start.at));
            }
        }
        self.run_shakes(dt);
    }

    /// Starts one of the game's named shakes wherever the camera is (`camera_shake_create`
    /// 0091dc70, no place); a name the list lacks starts nothing. [game]
    pub fn start_named_shake(&mut self, name: u32, tuning: &Tuning) {
        if let Some(def) = tuning.shakes.get(&name) {
            self.shakes.start(*def, None);
        }
    }

    /// Starts one of the game's named shakes at a place, felt by how near the camera is (its
    /// `shakeFullDist` / `shakeMaxDist`); a name the list lacks starts nothing.
    pub fn start_named_shake_at(&mut self, name: u32, tuning: &Tuning, at: Vec3) {
        if let Some(def) = tuning.shakes.get(&name) {
            self.shakes.start(*def, Some(at));
        }
    }

    /// The shakes a clip of his asks for in the stretch just played (`AnimCameraShakeEvent`,
    /// `FUN_007d0ff0`): at his place, or wherever the camera is. The original takes node_world_matrix (011749d0) at the index FUN_00578440 gives for the handed object (007d0ff0, the positional branch).
    /// [assumed: the handed object is his own, so its node 0 is his place. settles: the caller of 007d0ff0 that passes the object (the event registry's handler, 0053aa20)]
    pub fn clip_shakes(&mut self, clip: &Clip, from: f32, to: f32, at: Vec3, tuning: &Tuning) {
        for event in clip.shakes_between(from, to) {
            if let Some(def) = tuning.shakes.get(&event.name) {
                self.shakes.start(*def, event.positional.then_some(at));
            }
        }
    }

    fn run_shakes(&mut self, dt: f32) {
        if !self.shakes.any() {
            (self.shake_time, self.jolt) = (0.0, Jolt::default());
            return;
        }
        self.shake_time += dt;
        while self.shake_time >= SHAKE_UPDATE {
            self.shake_time -= SHAKE_UPDATE;
            self.jolt = self
                .shakes
                .step_with_clock(self.view.eye, SHAKE_UPDATE, || {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as u32
                });
        }
    }
}

pub fn grab_cursor(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut cursor: Single<(&mut CursorOptions, &Window), With<PrimaryWindow>>,
) {
    let (cursor, window) = &mut *cursor;
    // A click in another window (the second robot's, F3) is not his.
    if buttons.just_pressed(MouseButton::Left) && window.focused {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
    if keys.just_pressed(KeyCode::Escape) {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

/// D-pad Up sends a signed-1 level step; camera options are shared across forms in
/// the user's capture (57.867s L1->L0;96.147s L0->L2). [trace/game]
/// C is the keyboard equivalent. Camera Options also support separate per-form levels.
pub fn cycle_distance(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut controls: ResMut<Controls>,
    time: Res<Time>,
    mut tuning: ResMut<TuningRes>,
    mut rig: ResMut<Rig>,
    player: Single<&Player>,
) {
    let scripted = controls
        .camera_cycle_at
        .is_some_and(|at| time.elapsed_secs() >= at);
    if scripted {
        controls.camera_cycle_at = None;
    }
    // While sniping D-pad up is the zoom: `character_camera_message` 0071a5c0 l.44-53 asks the
    // input source for action 4 (`CameraDebugFollowCloser`, controller index 4 in 01185160) and
    // sets the zoom to +1 while it is held; action 5 (`CameraDebugFollowFarther`) gives -1 [game].
    // [assumed] The distance step is then skipped (the sniper gate here). Read 2026-10-20:
    // `CameraSwitchBack` is controller index 9 (01185160), and 0071a5c0 l.86-93 sets the message
    // bit 0x20 (`param_1[0] |= 0x20`) when controller 9 reads set and controller 8 reads clear
    // (not gated by `camera_is_sniping`, which only gates the zoom at l.45-64). So the sniper gate
    // is not in the character's message. settles: the camera's own read of bit 0x20 of the block
    // after the dispatch 0091cf90: 00921f60 and 00927f60 read no `and ..,0x20` (raw_disasm
    // 00921f60-00922300 and 00927f60-00928200 checked 2026-10-20); the reader is in vt+0x44's class.
    // Read 2026-10-20 (3rd pass), chain now identified: the character registers ring node
    // `character+0x3c4` on `ca291e26` (00713300 at 007136a9, ring-add 011945f0 via thunk 00724dd0
    // at 00713ecd); its callback at node +0xc is 00725100, a thunk to 0071a5c0 (this message). The
    // ring is dispatched by 0091cf90 from four camera behaviours: 00919de0 (spectator), 00921f60
    // (follow start), 00922580 (camera_target_request), 00927f60 (weapon start); 0091a6f7 reads
    // only bit 1 of the block after it. In 00922580 the `& 0x20` test (00922732) reads the block
    // passed to the camera's vt+0x44 (`lea eax,[esp]`, the same buffer, overwritten by that call),
    // so the bit set here is not seen there unless vt+0x44 forwards it. Not settled: the class of
    // vt+0x44 and the sniper branch of 00921f60 / 00927f60 (not read). `settles: the body of
    // vt+0x44 (the camera class's slot 17) and the `[esp+0x20]`-style bit tests after the dispatch
    // in 00921f60 and 00927f60.`
    if rig.camera.sniper.is_some() {
        return;
    }
    if !scripted
        && !keys.just_pressed(KeyCode::KeyC)
        && !pads
            .iter()
            .any(|pad| pad.just_pressed(GamepadButton::DPadUp))
    {
        return;
    }
    let t = &mut tuning.0;
    let vehicle = player.state.is_vehicle();
    let weapon = !vehicle && controls.aim;
    let form = if vehicle {
        2
    } else if weapon {
        1
    } else {
        0
    };
    let options = rig.camera.options;
    options.cycle(&mut rig.levels, form, -1);
    if let Some(s) = t.robot_camera_levels.get(rig.levels[0]) {
        t.robot_camera = s.clone();
    }
    if let Some(s) = t.weapon_camera_levels.get(rig.levels[1]) {
        t.weapon_camera = s.clone();
    }
    if let Some(s) = t.drive_camera_levels.get(rig.levels[2]) {
        t.drive_camera = s.clone();
    }
    let aim = weapon.then(|| rig.aim.direction());
    let target = target(&player.state, t, aim);
    rig.camera.settings_changed(&target, t);
}

/// Puts Bevy's camera where the game's is. The game's field of view is the picture's height.
pub fn apply(rig: Res<Rig>, mut camera: Single<(&mut Transform, &mut Projection), (With<Camera3d>, Without<crate::ocean::OceanReflection>, Without<crate::duel::SecondView>)>,
    player: Option<Single<&crate::player::Player>>) {
    let (transform, projection) = &mut *camera;
    let mut shaken = rig.jolt.apply(&rig.view);
    // For captures only: the picture is taken from round to one side of where the game's
    // camera is, about the point `orbit_pivot` ahead of it; the game's own view is left alone.
    if rig.orbit != 0.0 {
        let turn = Quat::from_rotation_z(rig.orbit.to_radians());
        let mut pivot = shaken.eye + shaken.forward * rig.orbit_pivot;
        // A negative pivot is the height over his feet to swing round instead, looking at it.
        if let Some(player) = player.filter(|_| rig.orbit_pivot < 0.0) {
            pivot = player.state.pos + Vec3::Z * -rig.orbit_pivot;
            shaken.forward = (pivot - shaken.eye).try_normalize().unwrap_or(shaken.forward);
            shaken.up = Vec3::Z;
        }
        shaken.eye = pivot + turn * (shaken.eye - pivot);
        shaken.forward = turn * shaken.forward;
        shaken.up = turn * shaken.up;
    }
    **transform = Transform::from_translation(game_to_bevy(shaken.eye))
        .looking_to(game_to_bevy(shaken.forward), game_to_bevy(shaken.up));
    if let Projection::Perspective(perspective) = &mut **projection {
        // Down to the sniper camera's 5 degrees (`zoomFOV`). [data]
        let fov = rig.view.fov.clamp(1.0, 120.0).to_radians();
        perspective.fov = if rig.zoom == 1.0 {
            fov
        } else {
            2.0 * ((fov * 0.5).tan() / rig.zoom).atan()
        };
    }
}
