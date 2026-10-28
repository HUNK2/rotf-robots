//! Input, the movement, the camera and the weapons each frame, and showing the result on
//! the loaded models.

use bevy::input::gamepad::GamepadSettings;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::assets::{ObjectNodes, bevy_to_game, game_to_bevy};
use crate::camera::{self, Rig};
use crate::control::button;
use crate::sim::{self, Input, State};
use crate::sound::SoundOut;
use crate::tuning::Tuning;
use crate::weapons::Arsenal;
use crate::world::ArenaRes;

/// Damage dealt by the test key, to watch health come back.
const TEST_DAMAGE: f32 = 80.0;
/// The longest step the movement is given, in seconds; a hitch is not turned into a leap.
const LONGEST_STEP: f32 = 0.05;

#[derive(Resource)]
pub struct TuningRes(pub Tuning);

/// What the pads and keys ask for, gathered every frame.
#[derive(Resource, Default)]
pub struct Controls {
    /// The filtered pad of this frame, as the game's commands read it (`tf2_core::input`):
    /// the keyboard, the mouse and the pads through the default bindings. Other systems
    /// can test its bits instead of reading keys.
    pub pad: tf2_core::input::Filtered,
    /// Left stick: x right, y forward.
    pub stick: Vec2,
    /// Right stick, for the camera: x right, y up. With the modern layout the mouse's turn
    /// is in here too as a stick (its rate over the full camera rate), for the readers of the
    /// stick that are not the camera or the aim (the helicopter, the animation rules).
    pub look: Vec2,
    /// `look` without the mouse: what the camera and the aim take as a stick.
    pub view_look: Vec2,
    /// The modern layout's mouse turn this frame, radians, x right and y up
    /// (`tf2_core::input::mouse_turn`): the camera and the aim turn by it directly. [port]
    pub mouse_turn: Vec2,
    /// `--classic-controls`: the install's shipped keyboard and mouse bindings, the mouse a
    /// stick, as the original. Without it the port's modern layout (`tf2_core::input::modern_map`).
    pub classic: bool,
    /// `--mouse-sens <degrees per count>` for the modern layout's mouse.
    pub mouse_degrees: f32,
    /// One scripted D-pad Up press for deterministic camera captures.
    pub camera_cycle_at: Option<f32>,
    pub recentre_pressed: bool,
    pub recentre_at: Option<f32>,
    /// `CameraDebugFollowCloser` / `Farther` held (D-pad up and down; I and K, the wheel):
    /// the sniper camera's zoom. [data] `--zoom-in-from <s>` holds the first for captures.
    pub zoom_closer: bool,
    pub zoom_farther: bool,
    pub zoom_in_from: Option<f32>,
    /// The right trigger: car form and throttle; in weapon mode, fire.
    pub vehicle: f32,
    pub brake: f32,
    /// The left trigger: weapon mode on foot, the slide in the car.
    pub aim: bool,
    /// Fire from the mouse, which only counts in weapon mode.
    pub fire: bool,
    pub switch: bool,
    pub jump_held: bool,
    pub jump_pressed: bool,
    /// The left face button: the fast attack, and in weapon mode the dodge (the game binds
    /// `AttackFast` and `Dodge` to the same button).
    pub melee_held: bool,
    pub melee_pressed: bool,
    /// The right face button: climb (and action).
    pub climb_held: bool,
    pub climb_pressed: bool,
    pub damage_pressed: bool,
    pub reset_pressed: bool,
    /// Inputs held or fired without a player, for captures: the vehicle trigger between two
    /// times after start, the stick, a jump at a given time, and weapon mode with the
    /// trigger down from a given time.
    pub vehicle_window: Option<(f32, f32)>,
    pub force_stick: Vec2,
    /// `--stick-from <s>` / `--stick-until <s>`: the forced stick (`force_stick`) applies only
    /// from the first time to the second. Without them it applies throughout.
    pub stick_from: Option<f32>,
    pub stick_until: Option<f32>,
    /// `--look <v>` / `--look-x <v>`: the second stick (`look`) held by a capture, inside the
    /// `--look-from` / `--look-until` window if given, else the `--stick-from` / `--stick-until` window. For a helicopter `look.y` is the lift (-1 climbs,
    /// core/src/flight.rs steer_helicopter) and `look.x` the turn.
    pub force_look: Vec2,
    /// `--look-from <s>` / `--look-until <s>`: a window for the second stick only. Without them
    /// the second stick uses the `--stick-from` / `--stick-until` window, as before.
    pub look_from: Option<f32>,
    pub look_until: Option<f32>,
    pub jump_at: Option<f32>,
    /// A capture holds the jump button from this time on.
    pub jump_hold_from: Option<f32>,
    /// A capture takes all his health at this time (`--die-at`).
    pub die_at: Option<f32>,
    /// Synthetic received hits for checking real reaction clips, separate from self-damage.
    pub hit_at: Option<f32>,
    pub hit_flags: u32,
    pub die_all: bool,
    /// Capture harness: retain the corpse instead of restarting the arena.
    pub keep_dead: bool,
    pub fire_from: Option<f32>,
    /// When a capture lets the trigger go again (`--fire-until`): a missile leaves then.
    pub fire_until: Option<f32>,
    /// Weapon mode without firing from a given time, and taps of the melee button.
    pub aim_from: Option<f32>,
    /// Weapon mode taken and let go every so many seconds from `aim_from` (`--aim-cycle`).
    pub aim_cycle: Option<f32>,
    pub melee_at: Vec<f32>,
    /// The melee button held from a given time.
    pub melee_hold_from: Option<f32>,
    /// And let go at this time, if given.
    pub melee_hold_until: Option<f32>,
    /// Climb held from a given time.
    pub climb_from: Option<f32>,
    /// The camera stick held up (or down) by this much while the scripted aim is on.
    pub force_look_y: f32,
    /// The next weapon button tapped at these times.
    pub switch_at: Vec<f32>,
    /// The top face button: the special ability. And a capture's press of it at a time.
    pub special_held: bool,
    pub special_pressed: bool,
    pub special_at: Option<f32>,
}

#[derive(Component)]
pub struct Player {
    pub state: State,
    /// Weapon mode is on this frame.
    pub aiming: bool,
    pub reset_serial: u32,
    /// He is sniping this frame (`camera_is_sniping`: his sniper camera is in use). While it
    /// lasts his object is hidden (`character_set_sniping` 00721ce0 -> 01176750). [game]
    pub sniping: bool,
    /// `state.since_damage` as the last frame left it: a smaller one now means a hit was taken
    /// since (for `SnipingCameraShake`, 00729120).
    pub seen_since_damage: f32,
    /// The scope's BG loops (`BG_SNIPER_A` / `_D`) while he snipes, kept to be stopped.
    pub scope_loops: Vec<tf2_core::sound::Handle>,
}

impl Player {
    pub fn new(state: State) -> Self {
        Self { state, aiming: false, reset_serial: 0, sniping: false, seen_since_damage: f32::MAX, scope_loops: Vec::new() }
    }
}

#[derive(Component)]
pub struct RobotModel;

#[derive(Component)]
pub struct VehicleModel;

/// In place of `VehicleModel` for a character whose pack has no vehicle object: nothing
/// is drawn, and `animation::animate` has something to drive. [data] The packs with none are
/// megatronflight and optimusflight (`vehicleProxy: None`, 2026-10-20); the rest have one. [stand-in]
/// Same stand-in as `spawn_player` (`game/src/main.rs` l.810), where this object is made: as that.
/// OPEN, settles: the null test on C+0x108 in the game's clip driver for the second object (the
/// same question as main.rs l.810; the driver is not located). Read
/// 2026-10-20: the vehicle object is the pointer at character +0x108 (`flight-camera-exit.md`, [data]; the
/// reader 0071a2e0 checks it through `*(C+0x108)+4`). 0072a550's `+0x108` is a float timer, not that pointer.
/// The null guard in the clip driver (the animation step that gets the vehicle's animator) is not located:
/// settles: the reads of [character+0x108] in 0071a2e0's callers and in the animation step (`fn.py callers`).
#[derive(Component)]
pub struct NoVehicle;

/// The PC input path (`tf2_core::input`): the install's default bindings, the per-action
/// maximum, the bridge to an XInput pad and the engine's pad filter with its 0.25 axial dead
/// zone, the mouse included. [game] [data]
pub struct InputState {
    path: tf2_core::input::InputPath,
    /// The filtered pad's buttons last frame, for presses.
    last: u32,
}

impl InputState {
    /// The shipped bindings (`classic`), or the port's modern keyboard and mouse layout over the
    /// shipped pad bindings.
    fn load(classic: bool) -> Self {
        let dir = tf2_core::install::dir().to_path_buf();
        let map = tf2_core::formats::bxdata::Archive::install(&dir)
            .and_then(|a| tf2_core::input::ActionMap::install_defaults(&a, "Robot"))
            .unwrap_or_else(|e| {
                eprintln!("input: no default bindings ({e}); the pad does nothing");
                Default::default()
            });
        let path = if classic { tf2_core::input::InputPath::new(map) } else { tf2_core::input::InputPath::modern(&map) };
        InputState { path, last: 0 }
    }
}

/// The DirectInput key name (`tf2_core::input::KEYS`) of a key. Keys the shipped bindings do
/// not use still map, for saved custom bindings later.
pub(crate) fn key_code(name: &str) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match name {
        "ESCAPE" => Escape, "1" => Digit1, "2" => Digit2, "3" => Digit3, "4" => Digit4, "5" => Digit5,
        "6" => Digit6, "7" => Digit7, "8" => Digit8, "9" => Digit9, "0" => Digit0, "MINUS" => Minus,
        "EQUALS" => Equal, "BACK" => Backspace, "TAB" => Tab, "Q" => KeyQ, "W" => KeyW, "E" => KeyE, "R" => KeyR,
        "T" => KeyT, "Y" => KeyY, "U" => KeyU, "I" => KeyI, "O" => KeyO, "P" => KeyP, "LBRACKET" => BracketLeft,
        "RBRACKET" => BracketRight, "RETURN" => Enter, "LCONTROL" => ControlLeft, "A" => KeyA, "S" => KeyS,
        "D" => KeyD, "F" => KeyF, "G" => KeyG, "H" => KeyH, "J" => KeyJ, "K" => KeyK, "L" => KeyL,
        "SEMICOLON" => Semicolon, "APOSTROPHE" => Quote, "GRAVE" => Backquote, "LSHIFT" => ShiftLeft,
        "BACKSLASH" => Backslash, "Z" => KeyZ, "X" => KeyX, "C" => KeyC, "V" => KeyV, "B" => KeyB, "N" => KeyN,
        "M" => KeyM, "COMMA" => Comma, "PERIOD" => Period, "SLASH" => Slash, "RSHIFT" => ShiftRight,
        "MULTIPLY" => NumpadMultiply, "LMENU" => AltLeft, "SPACE" => Space, "CAPITAL" => CapsLock, "F1" => F1,
        "F2" => F2, "F3" => F3, "F4" => F4, "F5" => F5, "F6" => F6, "F7" => F7, "F8" => F8, "F9" => F9, "F10" => F10,
        "NUMLOCK" => NumLock, "SCROLL" => ScrollLock, "NUMPAD7" => Numpad7, "NUMPAD8" => Numpad8,
        "NUMPAD9" => Numpad9, "SUBTRACT" => NumpadSubtract, "NUMPAD4" => Numpad4, "NUMPAD5" => Numpad5,
        "NUMPAD6" => Numpad6, "ADD" => NumpadAdd, "NUMPAD1" => Numpad1, "NUMPAD2" => Numpad2, "NUMPAD3" => Numpad3,
        "NUMPAD0" => Numpad0, "DECIMAL" => NumpadDecimal, "OEM_102" => IntlBackslash, "F11" => F11, "F12" => F12,
        "F13" => F13, "F14" => F14, "F15" => F15, "NUMPADEQUALS" => NumpadEqual, "NUMPADENTER" => NumpadEnter,
        "RCONTROL" => ControlRight, "NUMPADCOMMA" => NumpadComma, "DIVIDE" => NumpadDivide,
        "SYSRQ" => PrintScreen, "RMENU" => AltRight, "PAUSE" => Pause, "HOME" => Home, "UP" => ArrowUp,
        "PRIOR" => PageUp, "LEFT" => ArrowLeft, "RIGHT" => ArrowRight, "END" => End, "DOWN" => ArrowDown,
        "NEXT" => PageDown, "INSERT" => Insert, "DELETE" => Delete, "LWIN" => SuperLeft, "RWIN" => SuperRight,
        "APPS" => ContextMenu,
        _ => return None,
    })
}

/// A Bevy pad as the XInput state the game reads. Bevy hands out -1..1 and 0..1; the raw
/// values come back as the XInput reader scales them (`004ec6c0` [game]: triggers byte / 255,
/// sticks positive / 32767 and negative / -32768). [assumed] the Bevy side. Read 2026-10-20: the
/// backend is gilrs's WGI one, not XInput (gilrs-0.11.2 and gilrs-core-0.6.8 default feature `wgi`;
/// no feature here selects `xinput`). Its Gamepad readings give each thumbstick as
/// `trunc(f * i32::MAX)` (gilrs-core windows_wgi/gamepad.rs:478), and gilrs's `axis_value` on the
/// stick's range (i32::MIN..i32::MAX, windows_wgi/gamepad.rs:708-718) returns that as `f` again, so
/// Bevy's stick is the WGI float. How the WGI float relates to the XInput count is the Windows
/// runtime's, not in this tree, so the inverse here is not checked against the count. QUEUED: the
/// executable cannot show it (the float is made by the Windows runtime's WGI, gilrs-core
/// windows_wgi/gamepad.rs:478 and :708-718, and the game's count is not traced here).
/// settles: a recording of a pad's centred stick and of a full deflection, the count the game reads
/// against the value Bevy gives.
pub(crate) fn xinput_pad(pad: &Gamepad) -> tf2_core::input::Pad {
    use tf2_core::input::bit;
    let axis = |a: GamepadAxis| {
        let v = pad.get(a).unwrap_or(0.0).clamp(-1.0, 1.0);
        (if v >= 0.0 { v * 32767.0 } else { v * 32768.0 }).round() as i16
    };
    let trigger = |b: GamepadButton| (pad.get(b).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut buttons = 0u16;
    for (b, mask) in [
        (GamepadButton::DPadUp, bit::DPAD_UP), (GamepadButton::DPadDown, bit::DPAD_DOWN),
        (GamepadButton::DPadLeft, bit::DPAD_LEFT), (GamepadButton::DPadRight, bit::DPAD_RIGHT),
        (GamepadButton::Start, bit::START), (GamepadButton::Select, bit::BACK),
        (GamepadButton::LeftThumb, bit::L_THUMB), (GamepadButton::RightThumb, bit::R_THUMB),
        (GamepadButton::LeftTrigger, bit::L_SHOULDER), (GamepadButton::RightTrigger, bit::R_SHOULDER),
        (GamepadButton::South, bit::A), (GamepadButton::East, bit::B),
        (GamepadButton::West, bit::X), (GamepadButton::North, bit::Y),
    ] {
        if pad.pressed(b) {
            buttons |= mask;
        }
    }
    tf2_core::input::Pad {
        buttons,
        left_trigger: trigger(GamepadButton::LeftTrigger2),
        right_trigger: trigger(GamepadButton::RightTrigger2),
        lx: axis(GamepadAxis::LeftStickX),
        ly: axis(GamepadAxis::LeftStickY),
        rx: axis(GamepadAxis::RightStickX),
        ry: axis(GamepadAxis::RightStickY),
    }
}

/// Bevy's own dead zones, live zones and change thresholds off, so the pad reaches the
/// game's filter as XInput reports it. [game] the original reads XInput unfiltered.
fn raw_pad_settings(settings: &mut GamepadSettings) {
    if let Ok(axis) = bevy::input::gamepad::AxisSettings::new(-1.0, 0.0, 0.0, 1.0, 0.0) {
        settings.default_axis_settings = axis;
        settings.axis_settings.clear();
    }
    settings.default_button_axis_settings = bevy::input::gamepad::ButtonAxisSettings { high: 1.0, low: 0.0, threshold: 0.0 };
    settings.button_axis_settings.clear();
}

#[allow(clippy::too_many_arguments)]
pub fn read_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    wheel: Res<AccumulatedMouseScroll>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    pads: Query<&Gamepad>,
    mut new_pads: Query<&mut GamepadSettings, Added<GamepadSettings>>,
    mut state: Local<Option<InputState>>,
    mut controls: ResMut<Controls>,
    duel: Option<Res<crate::duel::Duel>>,
) {
    // While the second robot's window has focus (F3, `duel.rs`) the devices are its, not his.
    let others = duel.is_some_and(|d| d.focused);
    use tf2_core::input::{bit, Devices, Filtered, Mouse, L2_BIT, R2_BIT};
    for mut settings in &mut new_pads {
        raw_pad_settings(&mut settings);
    }
    let classic = controls.classic;
    let state = state.get_or_insert_with(|| InputState::load(classic));
    let now = time.elapsed_secs();
    let grabbed = cursor.grab_mode == CursorGrabMode::Locked;
    // The mouse as DirectInput reports it: counts since the last poll (y down), the wheel in
    // its 120 a notch, eight buttons. Only while the window holds the mouse. [game 004f09b0]
    // for the 120 a notch. The winit side is read in the cargo registry (a library, not the game):
    // winit 0.30.13 `platform_impl/windows/event_loop.rs` 2560-2592 sends the raw relative counts
    // (`lLastX`/`lLastY`) unscaled as `MouseMotion`, and 1722-1737 sends `WM_MOUSEWHEEL` as
    // `LineDelta(0, wheel / WHEEL_DELTA)`, so a notch is one line; bevy_winit 0.19.0 `state.rs`
    // 452-454 passes the motion through unchanged. Windows never sends a pixel scroll, so the
    // pixel branch below is unused here. [game 004f09b0] [lib winit 0.30.13]
    let mouse = if grabbed && !others {
        let notches = match wheel.unit {
            bevy::input::mouse::MouseScrollUnit::Line => wheel.delta.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => wheel.delta.y / 120.0,
        };
        let mut b = [false; 8];
        for (i, button) in [MouseButton::Left, MouseButton::Right, MouseButton::Middle, MouseButton::Back, MouseButton::Forward]
            .into_iter()
            .enumerate()
        {
            b[i] = buttons.pressed(button);
        }
        Mouse { dx: mouse.delta.x.round() as i32, dy: mouse.delta.y.round() as i32, wheel: (notches * 120.0).round() as i32, buttons: b }
    } else {
        Mouse::default()
    };
    let degrees = if controls.mouse_degrees > 0.0 { controls.mouse_degrees } else { tf2_core::input::MOUSE_DEGREES_DEFAULT };
    let mouse_turn = if classic { Vec2::ZERO } else { tf2_core::input::mouse_turn(&mouse, degrees) };
    let pad_states: Vec<_> = if others { Vec::new() } else { pads.iter().map(xinput_pad).collect() };
    let key_down = |name: &str| !others && key_code(name).is_some_and(|k| keys.pressed(k));
    let devices = Devices {
        key_down: &key_down,
        mouse,
        mouse_divisor: tf2_core::input::MOUSE_DIVISOR_DEFAULT,
        pads: &pad_states,
    };
    let pad = state.path.poll(&devices);
    let down = |bits: u32| pad.down(bits);
    let last = std::mem::replace(&mut state.last, pad.buttons);
    let hit = |bits: u32| pad.down(bits) && last & bits == 0;
    controls.pad = pad;

    let stick_window = controls.stick_from.is_none_or(|from| now >= from) && controls.stick_until.is_none_or(|until| now < until);
    let forced = if stick_window { controls.force_stick } else { Vec2::ZERO };
    // The engine's Y is positive down; here forward is up.
    let stick = forced + Vec2::new(pad.lx, -pad.ly);
    if controls.die_at.is_some_and(|at| now >= at) {
        controls.die_at = None;
        controls.damage_pressed = true;
        controls.die_all = true;
    }
    if controls.jump_at.is_some_and(|at| now >= at) {
        controls.jump_at = None;
        controls.jump_pressed = true;
    }
    let firing = controls.fire_from.is_some_and(|from| now >= from) && controls.fire_until.is_none_or(|until| now < until);
    // `--aim-cycle <s>`: weapon mode on for that long, then off as long, over and over.
    let scripted_aim = controls.aim_from.is_some_and(|from| now >= from
        && controls.aim_cycle.is_none_or(|cycle| (((now - from) / cycle) as u32).is_multiple_of(2)));
    let scripted_melee = controls.melee_at.first().is_some_and(|&at| now >= at);
    if scripted_melee {
        controls.melee_at.remove(0);
    }
    // The engine's Y is positive down; here up is up.
    let mut look = Vec2::new(pad.rx, -pad.ry);
    if scripted_aim || firing {
        look.y += controls.force_look_y;
    }
    // The second stick has its own window when `--look-from` / `--look-until` are given.
    let look_window = if controls.look_from.is_none() && controls.look_until.is_none() {
        stick_window
    } else {
        controls.look_from.is_none_or(|from| now >= from) && controls.look_until.is_none_or(|until| now < until)
    };
    if look_window {
        look += controls.force_look;
    }
    // The commands test the filtered pad's bits, by the `Controller` table [data]: R2 vehicle
    // mode, throttle and fire; L2 weapon mode and the slide; L1 brake; R1 next weapon; the
    // bottom face button (DPAD_R_DOWN) jump and turbo; the left (DPAD_R_LEFT) the fast attack,
    // the dodge and the car's gun; the right (DPAD_R_RIGHT) action and climb; the top
    // (DPAD_R_UP) the special; R3 the camera reset. The keyboard and mouse reach them through
    // the default bindings: Left Shift / left mouse R2, right mouse L2, R L1, T R1, Space,
    // middle mouse, Left Ctrl, F, E. [data]
    let scripted = controls.vehicle_window.is_some_and(|(from, until)| (from..until).contains(&now));
    // The car reads the trigger as `(t + 1) / 2` (`docs/driving.md`). [game]
    let vehicle = if scripted { 1.0 } else { Filtered::trigger01(pad.rt) };
    let brake = if down(bit::L_SHOULDER as u32) { 1.0 } else { 0.0 };
    let aim = down(L2_BIT) || firing || scripted_aim;
    let switch = down(bit::R_SHOULDER as u32) || controls.switch_at.iter().any(|&at| (at..at + 0.1).contains(&now));
    let jump = hit(bit::A as u32);
    let jump_held = down(bit::A as u32);
    let recentre = hit(bit::R_THUMB as u32) || controls.recentre_at.is_some_and(|at| now >= at);
    if recentre {
        controls.recentre_at = None;
    }
    let melee = hit(bit::X as u32) || scripted_melee;
    let melee_held = down(bit::X as u32)
        || controls.melee_hold_from.is_some_and(|from| now >= from && controls.melee_hold_until.is_none_or(|until| now < until));
    let scripted_climb = controls.climb_from.is_some_and(|from| now >= from);
    let climb = hit(bit::B as u32) || (scripted_climb && !controls.climb_held);
    let climb_held = down(bit::B as u32) || scripted_climb;
    let scripted_special = controls.special_at.is_some_and(|at| now >= at);
    if scripted_special {
        controls.special_at = None;
    }
    let special = hit(bit::Y as u32) || scripted_special;
    let special_held = down(bit::Y as u32) || scripted_special;
    // Held to length 1 here: diagonal keys give the engine (1, 1). [assumed] the clamp to length 1 (the game
    // keeps the pad's amplitude, see below). The reader itself is [game 0084fb00]: the input source's virtual
    // at +0x24 (owner +0x20, `docs/bumblebee-mechanics.md` MoveMode), for the player class's pad path. The
    // keyboard path is not read here.
    // Read 2026-10-20: for the player class (vtable 00b6251c, slot 9 = +0x24) that is 0084fb00
    // (pad_left_stick_relative: the controller index at the device's +0x1c, pad block 00d1e85c + index * 0xfc),
    // which keeps the pad's amplitude and clamps nothing; the pad table
    // is written by 005aa890, which clamps each axis to -1..1 on its own, so a full diagonal gives
    // length about 1.41 there. The port clamps to 1 and the game does not, here. MoveMode uses that
    // stick only for its direction and threshold tests (0.01, and 0.9801 for a jump), which an
    // amplitude over 1 does not change. Whether the length reaches a speed in another mode is not
    // read. OPEN, settles: the readers of the input length in the game's walk and strafe modes, and
    // that the human's owner +0x20 is the player class (its constructor not read).
    controls.stick = stick.clamp_length_max(1.0);
    controls.view_look = look.clamp(Vec2::NEG_ONE, Vec2::ONE);
    controls.mouse_turn = mouse_turn;
    // The mouse as a stick for the stick's other readers: its rate over the follow camera's
    // full rate (`userRotSpdZ`, 180 degrees a second). [port]
    let mouse_stick = mouse_turn / (time.delta_secs().max(1e-3) * 180f32.to_radians());
    controls.look = (look + mouse_stick).clamp(Vec2::NEG_ONE, Vec2::ONE);
    controls.vehicle = vehicle;
    controls.brake = brake;
    controls.aim = aim;
    controls.fire = down(R2_BIT) || firing;
    controls.switch = switch;
    controls.jump_held = jump_held || controls.jump_hold_from.is_some_and(|from| now >= from);
    controls.jump_pressed |= jump;
    controls.recentre_pressed |= recentre;
    controls.zoom_closer = down(bit::DPAD_UP as u32) || controls.zoom_in_from.is_some_and(|from| now >= from);
    controls.zoom_farther = down(bit::DPAD_DOWN as u32);
    controls.melee_held = melee_held;
    controls.melee_pressed |= melee;
    controls.climb_held = climb_held;
    controls.climb_pressed |= climb;
    controls.special_held = special_held;
    controls.special_pressed |= special;
    // Test keys, on keys the original leaves free (R is its brake). [port] A debug feature with no
    // game counterpart, not a stand-in for anything the game does (decided 2026-10-20).
    controls.damage_pressed |= keys.just_pressed(KeyCode::KeyH);
    controls.reset_pressed |= keys.just_pressed(KeyCode::F5);
}

/// One frame of Bumblebee: the movement, then the camera that follows it, his weapons and
/// his sounds. The movement and the camera keep the game's own update length inside.
#[allow(clippy::too_many_arguments)]
pub fn tick(
    time: Res<Time>,
    tuning: Res<TuningRes>,
    arena: Res<ArenaRes>,
    occluders: Res<camera::Occluders>,
    sound: Option<Res<SoundOut>>,
    library: Res<crate::animation::AnimLibrary>,
    data: Res<crate::GameData>,
    mut controls: ResMut<Controls>,
    mut rig: ResMut<Rig>,
    mut arsenal: Option<ResMut<Arsenal>>,
    mut player: Single<&mut Player>,
    robot: Single<&ObjectNodes, With<RobotModel>>,
    car_nodes: Option<Single<&ObjectNodes, (With<VehicleModel>, Without<RobotModel>)>>,
    placed: Query<&GlobalTransform>,
    enemies: Query<&crate::targets::EnemyTarget>,
    weapon_models: Query<(&crate::weapons::WeaponModel, &ObjectNodes, Has<ChildOf>)>,
) {
    let player = &mut **player;
    let t = &tuning.0;
    let sound = sound.as_deref();
    let weapon_sound = sound.map(|s| s as &dyn tf2_core::arsenal::WeaponSound);
    if let Some(arsenal)=arsenal.as_mut() {arsenal.receive_heat(std::mem::take(&mut player.state.heat_damage));}
    let dt = time.delta_secs().clamp(0.0, LONGEST_STEP);
    if std::mem::take(&mut controls.reset_pressed) || (player.state.restart_requested && !controls.keep_dead) {
        let difficulty=player.state.difficulty;
        let turbo_upgrades=player.state.turbo_upgrades;
        player.state = State::new(t);
        player.state.difficulty=difficulty;
        player.state.turbo_upgrades=turbo_upgrades;
        player.reset_serial = player.reset_serial.wrapping_add(1);
        let options=rig.camera.options;
        let (orbit,zoom)=(rig.orbit,rig.zoom);
        *rig = Rig::new(&player.state, t, None, None);
        rig.camera.options=options;
        rig.orbit=orbit;rig.zoom=zoom;
        if let Some(sound) = sound {
            sound.silence();
        }
        if let Some(arsenal) = arsenal.as_mut() {
            arsenal.reset(weapon_sound);
        }
    }
    if std::mem::take(&mut controls.damage_pressed) {
        if std::mem::take(&mut controls.die_all) {
            player.state.damage(f32::MAX);
        } else {
            // [port] A debug key supplies an already resolved synthetic hit: a debug feature with no
            // game counterpart, not a stand-in (decided 2026-10-20).
            player.state.receive_hit(TEST_DAMAGE, 0, Vec3::ZERO);
        }
    }
    if controls.hit_at.is_some_and(|at| time.elapsed_secs() >= at) {
        controls.hit_at = None;
        // [stand-in] Capture-only incoming impulse; reaction choice/timing are the game's. OPEN,
        // settles: the impulse a real hit of a scout's box gives (the damage path, `docs/damage.md`).
        player.state.receive_hit(TEST_DAMAGE, controls.hit_flags, Vec3::new(0.0, -18.0, 12.0));
    }

    // On foot the stick is relative to the camera; in the car it steers.
    let forward = Vec2::new(rig.view.forward.x, rig.view.forward.y).normalize_or(Vec2::Y);
    let right = Vec2::new(forward.y, -forward.x);
    let pressed = std::mem::take(&mut controls.jump_pressed);
    let melee = std::mem::take(&mut controls.melee_pressed);
    let climb = std::mem::take(&mut controls.climb_pressed);
    // The left face button is the fast attack, the dodge and, in the car, the car's gun
    // (`AttackFast`, `Dodge` and `AttackVehicle` are all bound to `DPAD_R_LEFT`). [data]
    let both = |down: bool| {
        (down as u64) << button::ATTACK_FAST | (down as u64) << button::DODGE | (down as u64) << button::ATTACK_VEHICLE
    };
    let climbing = |down: bool| (down as u64) << button::CLIMB;
    let special = std::mem::take(&mut controls.special_pressed);
    let special_bits = |down: bool| (down as u64) << button::ATTACK_SPECIAL;
    let vehicle = player.state.is_vehicle();
    // Weapon mode is the original's: held on the left trigger while on foot, and then the
    // right trigger fires instead of changing him into the car. In the car the same trigger
    // is the slide.
    let aiming = controls.aim && !vehicle && arsenal.is_some();
    // Devastator armed with his pods (`tf2_core::giant::Pods`) [assumed port design]: the right
    // trigger fires them whether he aims or not, and changes nothing else (he has no car); his aim
    // turns the camera and the pods, never his body, which walks on its own clips.
    let giant_armed = t.giant.as_ref().is_some_and(|g| !g.pods.is_empty()) && arsenal.is_some();
    let was_aiming = std::mem::replace(&mut player.aiming, aiming);
    // The fire button is on the same trigger as the change of form (the bindings put both
    // on R2); the state machine's firing state reads it. [data]
    let fire = (aiming && (controls.vehicle > 0.1 || controls.fire)) as u64;
    // In weapon mode the camera stick turns his AIM, and his body and the weapon camera
    // follow that (`FUN_0071b990`). Otherwise the aim follows the character object's
    // forward row (`0071af5c..0071af62`). Weapon entry resets it to the camera. [game]
    //
    // As weapon mode starts the aim is put on the camera's forward and, with the aim assist
    // on (the options' default), a target within 30 degrees of the camera's line is looked
    // for and turned to (`r2wLockOn`); after that the stick turns the aim, slowed over a
    // target in the assist's box and pulled toward it while he moves. [game] Taking aim is the weapon state's enter: connectionWeaponMode on WeaponMode (L2), docs/state-machine.md and docs/input.md.
    // [stand-in: the assist's target is the one the box held the frame before, as `seen` passes it to the search (weapons.rs l.1590). The game's search (007079f0, run before the aim in 0071dc20) bumps each candidate's +4, which is a reference count (its four sites, and 00532cc0 when +8 < 0; released by 00820480), not memory of last frame's box; 007088f0 does not write it (wave 28 P6). The dummies stand for enemies (targets.rs:24). settles: how `seen` reaches 007079f0 (its call in 0071dc20) and whether the candidate list (param_1+4) is kept across frames]
    // Sniping (`tf2_core::snipe`; `character_update` 0071dc20, `FUN_00721ce0`): it ends when the
    // weapon in hand has no snipe camera or he is out of `StateWeapon`; `CameraReset` (R3) in
    // weapon mode with a snipe weapon and a sniper camera toggles it. While it lasts the sniper
    // camera replaces the weapon camera, zoomed by D-pad up and down. [game] Going in and out
    // (`character_set_sniping` 00721ce0, its three arguments: on, and re-show the weapon) is
    // below, after the toggle; `SniperScope_Gfx` is drawn by `hud::scope_overlay`.
    {
        use tf2_core::snipe;
        let snipe_weapon = !vehicle && arsenal.as_ref().is_some_and(|a| a.snipe_in_hand());
        let in_weapon_state = player.state.state_class(t) == Some("StateWeapon");
        let sniping = rig.camera.sniper.is_some();
        // A toggle leave is the one that re-shows the rifle (`reshow` 1 at 0071dc20 l.135).
        let mut toggled_leave = false;
        if snipe::must_leave(sniping, snipe_weapon, in_weapon_state) {
            rig.camera.sniper = None;
        } else if snipe::toggles(true, snipe_weapon, t.sniper_camera.is_some(), aiming, controls.recentre_pressed) {
            toggled_leave = sniping;
            rig.camera.sniper = if sniping {
                None
            } else {
                t.sniper_camera.as_ref().map(|(settings, zoom)| tf2_core::camera::Sniper::new(settings.clone(), *zoom))
            };
        }
        if let Some(sniper) = rig.camera.sniper.as_mut() {
            sniper.input = snipe::zoom_input(controls.zoom_closer, controls.zoom_farther);
        }
        let now = rig.camera.sniper.is_some();
        if let Some(arsenal) = arsenal.as_mut() {
            arsenal.sniping = now;
            // Going in hides the weapon in hand (`manager_weapon_in_hand(+0x238, 0)` -> 01176750);
            // a toggle leave re-shows it (`weapon_show` 007a7630). [game 00721ce0] The game
            // calls `weapon_show` only when its third argument (reshow) is 1, so a forced leave
            // does NOT show it there. OPEN, settles: whether the game's weapon-state code shows
            // the rifle again on a forced leave (the callers of 00721ce0 with reshow 0 at
            // 0071dc20 l.49, l.118 were read; not their other callers).
            // [game 00721ce0, 0071dc20 l.49/118/132] A forced leave (`reshow` 0) shows nothing, so the
            // rifle stays hidden while it is still in hand. The next weapon in hand is shown by
            // `manager_take_in_hand` 007b2c60 (the new one +0x24(1)), so the flag clears when no snipe
            // weapon is in hand. [game 007b2c60] [assumed] the rifle not in hand is never drawn anyway:
            // 007b2c60 hides the old weapon (+0x28) only when its `listenToWeaponAnimEvent` is false
            // (weapon slot +0x34 via 0074f300). Read 2026-10-20: the snipe rifles (`snipeCameraEnabled`
            // true) have that flag TRUE in `global_states.txt`: AudiR8_PlasmaLongRifleAttribute (l.79181,
            // the Audi's rifle, `snipe.rs`), Firestorm_LongBeamRifleAttribute (l.87592, the LongBeamRifle
            // family; Breakaway's rifle is not matched by name). So 007b2c60 neither hides nor shows them.
            // Read 2026-10-20 (2nd pass): 0074f300 (`weapon_listen_to_anim_event`) has only two readers,
            // 007b2b40 (take out: sets the weapon's `+0x1c(1)` only when the flag is false or the arg is
            // set) and 007b2c60 (take in hand). Neither is a listen-event handler, so for the snipe rifles
            // the show / hide is the weapon's own animation events (`weapon_show` 007a7630 is slot +0x24,
            // its callers in the events not read). [assumed] the rifle not in hand stays hidden.
            // OPEN, settles: the animation event that calls the weapon's slot +0x24 / +0x28 for a
            // listen-true weapon (find the callers of those slots from the event handlers).
            // 2026-10-20: find_ptr 007a7630 (weapon_show) gives 00b5fbc4 = the gun vtable 00b5fba0 at +0x24,
            // so the slot is confirmed; `decomp_lookup grep "+ 0x24))("` gives ~80 generic sites, too many
            // to read here: narrow it by the weapon-event handler (the WeaponFireSpecialEvent family), not read.
            arsenal.scope_hidden = if now {
                snipe::rifle_hidden(now)
            } else if toggled_leave {
                false
            } else {
                arsenal.scope_hidden && snipe_weapon
            };
        }
        // `player.sniping` is last frame's: a change is a going in or out. A reset of the rig
        // (the debug restart) counts as a leave.
        if now != player.sniping {
            // His animation rate: vtable `+0x1c` = `character_speed_scale_set` 0071f4d0 with 0.2
            // going in, 1.0 going out (00721deb, 00721ec5). [game 0071f4d0] The game sets its
            // scale and rescales the channel speeds (+0x3c, +0x54, +0x5c), which are the
            // AnimController's three layer players (its own setter 00748dc0 has the same body).
            // [game] `State::special_rate` now scales every layer the simulation plays (action,
            // base, hit / fly partials, weapon set; sim.rs `layers_dt`, `weapon_layers`).
            // The vehicle model's clips follow it too (`animation::animate`), and the gun's arm and
            // object players (`Arsenal::anim_rate`, set below each frame, into `gun::Gun::step`).
            // Whether his movement follows the rate beyond the slowed clips' root motion is not
            // settled. Read 2026-10-20: in the decompile, the only reader of `char+0xd8` in the
            // character ranges (0071xxxx-0072xxxx, 0085xxxx-0087xxxx) is its writer 0071f4d0, which
            // rescales the layer speeds (+0x3c, +0x54, +0x5c) when the scale is set. 0072afd0 reads the
            // attribute's +0xd8, 00726100 and 0072656e copy the field, and 00871d50 reads d0..d8 as a
            // flyer's vector. Read 2026-10-20 (2nd pass): the literal forms `0xd8)` and word `[0x36]` over
            // 0071-0087 (decompile) give no movement reader of the character's +0xd8: the other hits are
            // AnimController's own +0xd8 (00748dc0, 00748d50, 00748e40), ActionTrigger (007c0e20, 007c13c0),
            // the trigger volumes (0080f470, 0080c2d0), the hover mode (00871d50), a byte at +0xd8 (00778160)
            // and copies (00726100, 0072656e). OPEN, settles: a disasm-level reader (`[reg+0xd8]` in an
            // expression, not the decompile's literal) of char+0xd8, and whether the layer speed scales the
            // stride or only the root motion (the clips' playback).
            player.state.special_rate = if now { snipe::ANIM_RATE } else { snipe::ANIM_RATE_OUT };
            // `SniperScope_Gfx`'s sounds (bnxglobal.str#23 Activate / Deactivate): going in the
            // zoom one-shot and the BG loop; going out the BG stopped (Untrigger), then the
            // return one-shot. By `auto_PlayerFaction`: 0 Autobot the `_A` set, else `_D`.
            // [data] The two banks hold disjoint sets: a name scan of characters\eng\audir8.fev has
            // only `_D` sniper events, firestorm.fev only `_A`, so asking for both plays only the
            // bank he loaded. The BG loop is kept (`start_kept`) and stopped with its own fade on
            // the Untrigger (its SoundPlayer has `keyOffOnUnTrigger` false). [data] The Untrigger
            // as a non-immediate stop is QUEUED: settles: listening to the sniper going out (the
            // BG loop must fade, not cut).
            if let Some(sound) = sound {
                use tf2_core::formats::hash::crc32;
                if now {
                    for name in ["WPN_SNIPER_ZOOM_A", "WPN_SNIPER_ZOOM_D"] {
                        sound.play_event(crc32(name.as_bytes()));
                    }
                    for name in ["BG_SNIPER_A", "BG_SNIPER_D"] {
                        player.scope_loops.extend(sound.start_kept(crc32(name.as_bytes()), None));
                    }
                } else {
                    for handle in player.scope_loops.drain(..) {
                        sound.stop(handle);
                    }
                    for name in ["ANIM_AUTOBOT_SNIPER_RETURN", "ANIM_DECEPTICON_SNIPER_RETURN"] {
                        sound.play_event(crc32(name.as_bytes()));
                    }
                }
            }
            player.sniping = now;
        }
        // Hit while sniping: `SnipingCameraShake` (cc52c210) at the camera (`damage_reaction_
        // dispatch` 00729120 at 00729e7f-00729ee4, `camera_shake_create` 0091dc70), for the
        // human player with a camera manager; the MP twin when 0088eb10's `+4` > 0 (not here).
        // [game] [assumed] the attacker's `+0x140 -> +0x3e` is 0 and his `+0x148` bit 3 clear
        // (true for the arena's scouts); a hit is seen as `since_damage` going down.
        if now && player.state.since_damage < player.seen_since_damage {
            rig.start_named_shake(tf2_core::formats::hash::crc32(b"SnipingCameraShake"), t);
        }
    }
    let sniping = rig.camera.sniper.is_some();
    let looking = rig.view.forward;
    if aiming {
        // [game] The same yaw/pitch inversion switches also turn weapon aim (0071b990).
        let look=rig.camera.options.stick(controls.view_look);
        // The modern layout's mouse: a direct turn of the aim, which also lets go of a target the
        // aim was turning to. [port]
        let mouse = rig.camera.options.stick(controls.mouse_turn);
        if mouse != Vec2::ZERO && was_aiming {
            (rig.aim.snap_wanted, rig.aim.snap) = (false, None);
            if sniping {
                let s = &t.aim.sniping;
                let share = tf2_core::snipe::zoom_rate_share(rig.view.fov, s.full_zoom_share);
                rig.aim.turn(mouse * share, s.min_pitch, s.max_pitch);
            } else {
                rig.aim.turn(mouse, t.aim.min_pitch, t.aim.max_pitch);
            }
        }
        let right = looking.cross(Vec3::Z).normalize_or(Vec3::X);
        let eye = tf2_core::aim::Camera { eye: rig.view.eye, right, forward: looking, up: right.cross(looking) };
        let targets = &arena.0.targets;
        let clear = |from: Vec3, to: Vec3| tf2_core::camera::Sight::sight(&arena.0, from, to).is_none();
        if !was_aiming {
            rig.aim.enter(looking, look);
        }
        if rig.aim.snap_wanted && rig.aim.snap.is_none() {
            let range = arsenal.as_ref().map_or(t.aim.default_max_range, |a| a.assist_range(&t.aim));
            rig.aim.snap = tf2_core::aim::snap_target(&eye, range, targets, &t.aim, &clear);
        }
        match rig.aim.snap.and_then(|id| targets.iter().find(|target| target.id == id)) {
            Some(target) => rig.aim.step_snap(tf2_core::aim::assist_point(target), eye.eye, player.state.pos, dt),
            // Sniping: its own rates, no assist (`Aim::step_sniping`). [game 0071b990]
            None if sniping => {
                rig.aim.snap = None;
                let fov = rig.view.fov;
                rig.aim.step_sniping(look, fov, &t.aim, dt);
            }
            None => {
                rig.aim.snap = None;
                let held = arsenal.as_ref().and_then(|a| a.assist_target(targets, &rig.view,
                    |id| enemies.iter().find(|e|e.id==id).map_or(Vec3::ZERO,|e|e.combat.state.vel)));
                let assist = tf2_core::aim::assist(look, &eye, player.state.vel, held.as_ref(), &t.aim, dt);
                let moving = controls.stick.length_squared() > 0.001;
                rig.aim.step_assisted(look, moving, player.state.pos, assist, &t.aim, dt);
            }
        }
    } else {
        rig.aim.leave();
        rig.aim.set(player.state.body_forward());
    }
    let aim = aiming.then(|| rig.aim.direction());
    let input = Input {
        stick: right * controls.stick.x + forward * controls.stick.y,
        steer: controls.stick.x,
        // On a wall the stick is the pad's own: up climbs up.
        pad_stick: controls.stick,
        look: controls.look,
        jump: pressed,
        jump_held: controls.jump_held,
        vehicle: if aiming || giant_armed { 0.0 } else { controls.vehicle },
        // The brake is the brake binding only: the drive update reads it as the digital button 0x13 and never
        // reads the stick's y (0085cad0 l.30-48; the stick's x, 0084fcd0, is the steer, l.496-501) [game]. (The
        // port's stick-pulled-back brake, which had no game source, is gone, 2026-10-20.)
        brake: controls.brake,
        // The original puts turbo on the jump button while driving.
        turbo: pressed && vehicle,
        drift: controls.aim,
        // In flight the left trigger is still the weapon-mode button 0x18 to the state machine:
        // `connectionFlytoWeaponHover` leaves stateFly on it (include 0x18, exclude 0x1c) [data]. The
        // flight step reads no aim, so it goes in as the body's heading, without the camera's aim.
        // [assumed] the bit reaches it in flight as on foot; the car is left out (its states' 0x18
        // connections not checked). the unpublished notes "The flyers' weapon hover".
        aim: aim.filter(|_| !giant_armed).map(|aim| Vec2::new(aim.x, aim.y))
            .or_else(|| (controls.aim && player.state.mode == tf2_core::sim::Mode::Fly).then(|| player.state.facing())),
        buttons_down: both(controls.melee_held || melee)
            | climbing(controls.climb_held || climb)
            | special_bits(controls.special_held || special)
            | fire << button::WEAPON_FIRE,
        buttons_hit: both(melee) | climbing(climb) | special_bits(special),
    };
    // Whether his base layer plays the change from the car as this update starts: a
    // weapon state entered straight from it shows the gun at once (`FUN_0087f4a0`).
    let was_unfolding = player.state.action_clip(t).is_some_and(|(set, _, _)| set == "Vehicle2RobotSet");
    // His spawner nodes' world places as drawn last frame: the special's explosion spawners put
    // their object at the node's world place when the Trigger arrives. [game 007fc270, 0056bee0]
    let robot_nodes: &ObjectNodes = &robot;
    // Only the form he is in: the car's spawners hang on the car model, his on his own.
    let on_car = player.state.car_mode;
    let spawners = if on_car { &t.actions.vehicle_spawners } else { &t.actions.spawners };
    player.state.node_places = spawners
        .iter()
        .filter_map(|blast| {
            let node = blast.node;
            let nodes: &ObjectNodes = if on_car { car_nodes.as_deref()? } else { robot_nodes };
            let entity = *nodes.entities.get(nodes.name_hashes.iter().position(|&hash| hash == node)?)?;
            placed.get(entity).ok().map(|at| (node, bevy_to_game(at.translation())))
        })
        .collect();
    // Devastator's shockwaves sit on his thumbs: a ring starts where the drawn node is.
    // [game 00805ed0: the object's place]
    if let Some(giant) = &t.giant {
        let thumbs: Vec<_> = giant.waves.iter().filter_map(|wave| {
            let entity = *robot_nodes.entities.get(robot_nodes.name_hashes.iter().position(|&hash| hash == wave.node)?)?;
            placed.get(entity).ok().map(|at| (wave.node, bevy_to_game(at.translation())))
        }).collect();
        player.state.node_places.extend(thumbs);
    }
    sim::step(&mut player.state, &input, t, &arena.0, dt);
    player.seen_since_damage = player.state.since_damage;

    let target = camera::target(&player.state, t, aim);
    if controls.recentre_pressed {rig.camera.recentre(&target);controls.recentre_pressed=false;}
    let world=tf2_core::camera::Scene {solids:&arena.0,fadeable:&occluders.0};
    // The modern layout's mouse turns the follow and driving cameras by its angle; the weapon
    // camera follows the aim, which the mouse turned above. [port]
    if aim.is_none() && controls.mouse_turn != Vec2::ZERO {
        let turn = rig.camera.options.stick(controls.mouse_turn);
        rig.camera.nudge(turn);
    }
    let view = rig.camera.step(&target, controls.view_look, t, &world, dt);
    rig.view = view;
    // The slam's finisher camera (`tf2_core::camera::finisher`): entered once as the finisher
    // takes its target, then aimed at the pair each frame; gone when the hold ends, which hands
    // back to the follow camera. [game 009192c0, 00918fa0, 009193d0] The two as the camera takes
    // them: bottom at the feet, top their height, reach their body radius (STEP +0x170).
    // [game 0070abe0] the top is the bottom plus `character_camera_height` = form height plus
    // max(holderZ - rootZ - wrapper54*upZ, 0) (ledger, docs/camera-finisher.md l.166-173); the
    // form height is the robot's avatar height, `robot_height` (tuning.rs l.925-926, `avatar[robot]`).
    // [assumed] the holder term is 0 for him on foot (holderZ not read, 006dc3b0 writer). settles:
    // the holder's committed origin z (006dc3b0) against his root z on foot.
    match player.state.finish_target.and_then(|id| arena.0.targets.iter().find(|target| target.id == id)) {
        Some(held) => {
            use tf2_core::camera::finisher::{Finisher, Side};
            let him = Side { bottom: player.state.pos, top: player.state.pos.z + t.robot_height, reach: t.robot_radius };
            let it = Side { bottom: held.pos, top: held.pos.z + held.height, reach: held.radius };
            // [game 009186f0] The entry reads `_time64` (009186f0 l.50-56): the side coin takes its
            // low byte's bit 0 and the `between` term its seconds (00551d60). The port reads the same
            // clock: the seconds since 1970, low 32 bits, at the entry.
            let seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as u32);
            // Its ray helper (00917dc0): the clear distance to the first blocking object, the
            // camera's query with the blocking mask 0x20 (005be890 as 00919800 calls it). [game]
            use tf2_core::camera::Sight;
            let ray = |from: Vec3, to: Vec3| world.sight_mask(from, to, 0x20).map_or((to - from).length(), |hit| (hit.point - from).length());
            let finisher = match rig.finisher {
                Some(finisher) => finisher,
                None => Finisher::enter(&him, &it, &mut rig.finisher_random, seconds, &ray),
            };
            rig.finisher = Some(finisher);
            let pose = finisher.aim(&him, &it, rig.view.forward);
            (rig.view.eye, rig.view.forward, rig.view.fov, rig.view.roll) = (pose.eye, pose.forward, pose.fov, 0.0);
        }
        None => rig.finisher = None,
    }
    rig.shake(&player.state.shakes, t, dt);

    if let Some(arsenal) = arsenal.as_mut() {
        // The gun's arm and object players take his animation rate (0071f4d0). [game]
        arsenal.anim_rate = player.state.special_rate;
        // Shots leave from the weapon's `FireDummy` node [data], the weapon taken to sit on
        // the node it hangs from as that was drawn last frame [assumed: the weapon object
        // is not placed yet; how it is fixed to the node is not read]. If the model has no
        // such node, from a point in front of his chest. [stand-in] OPEN. Read 2026-10-20 (3rd pass):
        // 007a8770 l.9-35 with the shot count `+0x324` below 1 (no FireDummy) fills each shot's slot with
        // the WEAPON OBJECT's own affine (`param_1[3]+0x10..+0x3c`), index 0, and returns 1: the game's
        // no-node origin is the weapon object's placement, not a chest point. The port does not place the
        // weapon object at this point yet, so the chest point stays the stand-in for it. Read 2026-10-20 (2nd pass):
        // the fire virtual `+0xcc` is NOT the reader: the gun vtable 00b5fba0 slot 51 is 007ae010
        // itself. The `+0x324` count (`param_1[0xc9]`) is read by 007a8770, reached from 007ae010 at
        // its decompile l.133 through vtable `+0x4c` (00b5fba0 slot 19), with the shot count and the
        // out-index array. 007a8770 l.9 calls `+0x48` (007a7a40, slot 18), which rebuilds the barrel
        // list and sets the count to 0 when no `FireDummy` name matches. With count < 1, 007a8770
        // (l.10-35) fills each shot's slot with the weapon object's OWN affine (`param_1[3]+0x10..+0x3c`),
        // index 0, and returns 1 (l.244); so the game does not drop the shot. That object origin is
        // `weapon_origin` below. The port's chest point is reached only when the rest-place route
        // (`muzzle_of`, 007b4f20) and the hang node both fail; the game's zero-count point is the
        // object origin, which needs the same hang node. OPEN, settles: which path a barrel-less
        // model takes in the game (007b4f20 before 007a8770, read at the caller of 007ae010's shot
        // count `iVar10`), and the attach code that puts the object on its node (`attach_models`).
        // The chest point is kept until that is read, then it goes. Measured 2026-10-20 (a one-off log in
        // the 13 firing captures, `--fire-from 1.5`): no robot's first weapon reaches the chest point (0 of
        // 13); the weapons switched to later and the specials were not exercised.
        let state = &player.state;
        let chest = state.pos + Vec3::Z * (t.robot_height * 0.7) + state.facing().extend(0.0) * 1.5;
        // The object hangs on its node in the robot, else on the vehicle object's node of that
        // name (every car-form weapon: Cannon_Tip, Back_Window_L, Turbine_L...). [game 007a6380]
        let car_hashes = car_nodes.as_ref().map(|car| car.name_hashes.as_slice());
        let place = |bone: u32, barrel: Vec3| {
            let entity = match tf2_core::weapons::mount(bone, &robot.name_hashes, car_hashes)? {
                tf2_core::weapons::Mount::Robot(node) => robot.entities[node],
                tf2_core::weapons::Mount::Car(node) => car_nodes.as_ref()?.entities[node],
            };
            placed.get(entity).ok().map(|node| bevy_to_game(node.transform_point(barrel)))
        };
        // A barrel as its object is drawn: the node `FireDummy[first + k]` of the model whose list
        // names the weapon, as that model hangs and is posed (`007a7a40` reads the posed object's
        // own node). [game 007a7a40] A model not hung on anything is passed over.
        let on_model = |barrel: crate::weapons::Barrel| {
            weapon_models
                .iter()
                .find(|(model, _, hung)| *hung && model.names.contains(&barrel.weapon))
                .and_then(|(_, nodes, _)| nodes.entities.get(nodes.name_hashes.iter().position(|&h| h == barrel.node)?).copied())
                .and_then(|entity| placed.get(entity).ok())
                .map(|node| bevy_to_game(node.translation()))
        };
        // Else the barrel's rest place on the node the model hangs on (`hang_node`: his cannon's
        // is WeaponAttachment_Arm_R, not its startBoneAttachment). [game 007b4f20] [data]
        let muzzle_of = |barrel: crate::weapons::Barrel| on_model(barrel).or_else(|| place(barrel.hung, barrel.at));
        let muzzle = muzzle_of(arsenal.barrel()).unwrap_or(chest);
        // His other hand's weapon (the child of the one in hand: Optimus's left rifle) fires from
        // its own FireDummy on its own node. [game 007ae010] [data]
        let partner_muzzle = arsenal.partner_barrel().and_then(muzzle_of).unwrap_or(muzzle);
        // The special's rounds leave from the special weapon's own FireDummy on the node it hangs
        // from (Megatron's beam gun, Jetfire's dump), not from the gun in hand. [data] A car-form
        // special's is on the car (Megatron's car beam from Cannon_Tip's FireDummy, the Audi's car
        // mine from Back_Window_L, the car turrets from Windshield_R / Turbine_L, the carpet bomb
        // from Interior). [game 007a6380, 007a7a40] The turret launchers have no drawn model
        // (`main.rs`), so their throw takes the node and the rest place.
        let special_muzzle = arsenal.special_barrel().and_then(muzzle_of).unwrap_or(muzzle);
        // The melee weapons follow the weapon events of the clip his control state plays; the
        // simulation runs them (`sim::step_melee`, as their boxes hit), the arsenal shows them.
        arsenal.tick_axe(&state.melee);
        // The gun is out in the weapon states, and through a dodge out of one. [game]
        let armed = match state.state_class(t) {
            Some("StateWeapon") => true,
            Some("StateDash") => arsenal.gun.out,
            _ => false,
        };
        // The car's weapon hangs on a node of the car; its barrels are that object's
        // `FireDummy` nodes, placed by the node as it was drawn last frame, with the
        // car's own axes for the way they point (`FUN_007a7a40` without
        // `useFireDummyMatrix`). [game] [stand-in for a car with no such node: ahead of
        // its nose]
        let nose = state.body_forward();
        let car_barrels: Vec<Vec3> = arsenal.car_weapon().map_or(Vec::new(), |(bone, barrels)| {
            let node = car_nodes
                .as_ref()
                .and_then(|car| Some(car.entities[car.name_hashes.iter().position(|&hash| hash == bone)?]))
                .and_then(|node| placed.get(node).ok());
            barrels
                .iter()
                .map(|&barrel| node.map_or(state.pos + nose * 3.0 + Vec3::Z, |node| bevy_to_game(node.transform_point(barrel))))
                .collect()
        });
        // The car-gun HUD's axis: the +y row of the car model's `Flipper` node (0xc8ec021e) as it
        // was drawn last frame. [game 007b8420 at 007b94d2-007b9600] None when the model has no such node.
        let flipper = car_nodes
            .as_ref()
            .and_then(|car| Some(car.entities[car.name_hashes.iter().position(|&hash| hash == 0xc8ec_021e)?]))
            .and_then(|node| placed.get(node).ok())
            .map(|node| bevy_to_game(node.transform_point(Vec3::Y)) - bevy_to_game(node.transform_point(Vec3::ZERO)));
        // Devastator's pods ride the car weapons' path (`Arsenal::load_giant`) [assumed port design]:
        // the selected kind's barrels are every pod's FireDummies on the bones they hang from (the
        // boss mission's `BonePositioner`s [data]) as drawn last frame, taken pod by pod so that the
        // rounds hop from pod to pod; they are sent along the camera's line, and the reticle sits on it.
        let pods = t.giant.as_ref().map_or(&[][..], |g| g.pods.as_slice());
        // The gatling fires only from the pods on the side he shoots to (`giant::pods_facing`), so
        // no round crosses his body. [assumed port design]
        let pod_barrels: Vec<Vec3> = pods.get(arsenal.car_current()).filter(|_| giant_armed).map_or(Vec::new(), |p| {
            let placed: Vec<Vec<Vec3>> =
                p.mounts.iter().map(|m| m.barrels.iter().filter_map(|&barrel| place(m.bone, barrel)).collect()).collect();
            let open = if p.kind == tf2_core::giant::GATLING {
                let centres: Vec<Vec3> =
                    placed.iter().map(|b| b.iter().copied().sum::<Vec3>() / b.len().max(1) as f32).collect();
                let open = tf2_core::giant::pods_facing(&centres, view.forward);
                let mask = open.iter().enumerate().fold(0u32, |m, (i, &o)| m | (u32::from(o) << i));
                static SHOWN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);
                if SHOWN.swap(mask, std::sync::atomic::Ordering::Relaxed) != mask && std::env::var_os("TF_DBG").is_some() {
                    let middle = centres.iter().copied().sum::<Vec3>() / centres.len().max(1) as f32;
                    let out: Vec<String> = centres.iter().map(|c| format!("({:.1}, {:.1})", c.x - middle.x, c.y - middle.y)).collect();
                    eprintln!("giant: gatling pods {open:?}, out {}, aim ({:.2}, {:.2}), facing ({:.2}, {:.2})", out.join(" "),
                        view.forward.x, view.forward.y, state.body_forward().x, state.body_forward().y);
                }
                open
            } else if p.kind == tf2_core::giant::BUG_BOMB {
                // The eruption leaves from one vent: the middle of his back launcher's barrels. [assumed port design]
                static SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                if !SHOWN.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    let local = |at: Vec3| {
                        let d = at - state.pos;
                        let f = state.body_forward();
                        format!("({:.1} right, {:.1} ahead, {:.1} up)", d.x * f.y - d.y * f.x, d.x * f.x + d.y * f.y, d.z)
                    };
                    for (m, b) in p.mounts.iter().zip(&placed) {
                        let origin = place(m.bone, Vec3::ZERO);
                        eprintln!("giant: bug-bomb mount {:#010x}: origin {}, barrels {}", m.bone,
                            origin.map_or("-".into(), local), b.iter().map(|&x| local(x)).collect::<Vec<_>>().join(" "));
                    }
                }
                return place(tf2_core::giant::ERUPTION_VENT, Vec3::ZERO)
                    .map(|bone| {
                        let vent = bone - state.body_forward() * tf2_core::giant::ERUPTION_VENT_BACK;
                        tf2_core::giant::vent_spots(vent, state.body_forward(), tf2_core::giant::ERUPTION_BOMBS as usize)
                    })
                    .unwrap_or_default();
            } else {
                vec![true; placed.len()]
            };
            let most = placed.iter().map(Vec::len).max().unwrap_or(0);
            (0..most)
                .flat_map(|k| placed.iter().zip(&open).filter(|(_, o)| **o).filter_map(move |(b, _)| b.get(k).copied()))
                .collect()
        });
        // The way his back launcher points, from its bone to the middle of its barrels as posed: the
        // bug bombs leave along it, so they tilt and turn with him. [assumed port design]
        if let Some(mount) = pods.iter().filter(|p| p.kind == tf2_core::giant::BUG_BOMB)
            .flat_map(|p| p.mounts.iter()).find(|m| m.bone == tf2_core::giant::ERUPTION_VENT && !m.barrels.is_empty())
        {
            let ends: Vec<Vec3> = mount.barrels.iter().filter_map(|&b| place(mount.bone, b)).collect();
            if let (Some(bone), false) = (place(mount.bone, Vec3::ZERO), ends.is_empty()) {
                let middle = ends.iter().copied().sum::<Vec3>() / ends.len() as f32;
                arsenal.vent_axis = (middle - bone).normalize_or(Vec3::Z);
                static SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                if giant_armed && arsenal.car_current() < pods.len() && pods[arsenal.car_current()].kind == tf2_core::giant::BUG_BOMB
                    && !SHOWN.swap(true, std::sync::atomic::Ordering::Relaxed)
                {
                    let (a, f) = (arsenal.vent_axis, state.body_forward());
                    eprintln!("giant: bug-bomb launcher points ({:.2} right, {:.2} ahead, {:.2} up)", a.x * f.y - a.y * f.x, a.x * f.x + a.y * f.y, a.z);
                }
            }
        }
        let car = if giant_armed {
            crate::weapons::Car { mode: true, formed: true, wait: 0.0, barrels: &pod_barrels, forward: view.forward, flipper: None, target_point: view.eye }
        } else {
            crate::weapons::Car {
                mode: state.car_mode,
                formed: state.is_vehicle(),
                wait: t.vehicle_weapon_wait,
                barrels: &car_barrels,
                forward: nose,
                flipper,
                target_point: state.pos,
            }
        };
        if giant_armed {
            arsenal.switch_car(controls.switch, weapon_sound);
        }
        // The special may have started in this step (`sim::start_special`): its weapon's shots
        // that fire at the start. [game]
        arsenal.special_started(state.special_starts, state.car_mode);
        arsenal.type6_special = state.slowing_special(t);
        // Megatron's special (type 4) cancels a charging weapon (`007add20`). [game 007ac370]
        // The branch (007ac370 l.97-106): owner in state `StateAttackSpecial` (class hash
        // -0x23db0e9f) and its special's kind (`+0x140 -> +0xc0`) is 4; then the cancel virtual
        // (+0x140) runs when the weapon's byte at `param_1[0x103]` (+0x40c) is set. The port asks the state, not its timer.
        arsenal.charge_cancel = t.special.kind == 4 && state.state_class(t) == Some("StateAttackSpecial");
        // The thrower's velocity a `tackOnOwnerSpeed` grenade takes on [game 00789b80]: his
        // body's, which in the car is the chassis's (`sim::State::vel`).
        arsenal.owner_velocity = state.vel;
        // His dead bit (+0x148 bit 3, set by character_kill 007204a0): his weapons fire no burst round. [game 007ac370]
        arsenal.owner_dead = state.dead();
        // The in-hand weapon object's origin: it hangs on its node with an identity transform
        // (`attach_models`), so the node's place. [game 0074d610 l.183-184 takes it as the flame
        // packet's "from"]
        let weapon_origin = place(arsenal.barrel().hung, Vec3::ZERO).unwrap_or(muzzle);
        let fired = arsenal.tick(
            armed,
            was_unfolding,
            crate::weapons::gun_clips(&library, &data.0),
            if giant_armed { controls.vehicle > 0.1 || controls.fire } else { state.firing },
            controls.switch,
            muzzle,
            weapon_origin,
            partner_muzzle,
            state.pos,
            view.eye,
            view.forward,
            view.fov,
            &arena.0,
            &arena.0.targets,
            &car,
            &t.aim,
            &t.damage,
            state.damage_timers.slowed>0.0,
            weapon_sound,
            dt,
        );
        // His melee hits restart the hit-mark clock too: the game stamps it for any damaging packet of his. [game 00729120]
        if state.hits.iter().any(|hit| hit.damage > 0.0) {
            arsenal.mark_hit();
        }
        // The special's rounds of this update, from his muzzle and his facing (the car's nose in
        // the car). [game] [stand-in: see `Arsenal::tick_special`] In the car the game takes the
        // turn as the owner's frame (`useFireDummyMatrix` is false for all five car-form specials,
        // [data]; 007a7a40 builds the barrel list from the FireDummy names, l.1-60 read). The port's
        // origin here is `special_muzzle` (l.809: the special barrel's `muzzle_of`, not the car's
        // `car_barrels` list at l.825-834), so the car's special origin is not yet the FireDummy list
        // the game reads. settles: which barrel 007ac370 / 007a7a40 pass for a car-form special
        // (the special barrel's `hung` node in the car), not read here.
        let facing = if state.car_mode { nose } else { state.facing().extend(0.0) };
        // The special clip's `WeaponFireSpecialEvent`s in this update's window press the special
        // (`Arsenal::tick_special_events`), before its rounds are taken. [game 007ba870]
        let special_clip = state.action_clip(t).and_then(|(set, id, tick)| {
            let clip = library.robot.find(set, id)?.clip.as_ref()?;
            Some((clip, set, id, tick))
        });
        arsenal.tick_special_events(special_clip, state.car_mode, &data.0.weapon_animations, dt);
        arsenal.tick_special(special_muzzle, facing, &arena.0, &arena.0.targets, weapon_sound, dt);
        // The deployed turrets (`Arsenal::tick_turrets`). The scouts are (id, team, the point aimed
        // at, alive); the point is the Character branch of 0079e920 (below, at the map). [game]
        // Read 2026-10-20 (0079e920 full body): the turret's target is `0079e920`
        // (fire_turret_target), its candidates from `00799cd0` (target_candidates). The STORED point is
        // the chosen target's root translation (+0x34..+0x3c, its copy at 0079e920 l.~175-178); the
        // `Head` node (0056bee0, hash 07c159a2) is used only for the line-of-sight test when the turret's
        // +0x7c is set, never stored. When the target's Character lookup (0070a3c0, l.~180) is non-null,
        // the stored point is overwritten by 0071a460 plus 0.75 of 0070abe0 (character_camera_height).
        // That Character branch is the scouts' point (at the map below). With no target it is 1000 m along the owner's forward row from a point
        // of vtable +0x90 (not read). `0079f5f0` is the turret's direction (the owner's forward row, or
        // a TurretRotator's cone direction), not a point, and `006e85e0` is only a handle release.
        // The turret's +0x7c (`checkLineOfSight`) only picks the line-of-sight test's point (the Head
        // node), not the stored one.
        // The fifth value is the scout's robot radius, the candidate radius of the turret's band test
        // [assumed: `robot_radius` of its own tuning, 007996c0's `EDI+0x14` not read].
        let scouts: Vec<(u32, u32, Vec3, bool, f32)> = enemies
            .iter()
            // [game 0079e920 l.171-180] a scout is a Character (0070a3c0 finds one), so the turret's point is
            // 0071a460's point plus 0.75 of `character_camera_height` 0070abe0: on foot that is his feet plus
            // 0.75 of his form height. [assumed] the holder terms vanish for an upright scout on foot (0071a460's
            // holder bottom is not below his root, and 0070abe0's holder rise is 0), as the camera takes them.
            .map(|e| (e.id, e.combat.state.actor.team, e.combat.state.pos + Vec3::Z * (0.75 * e.scout.tuning.robot_height),
                !e.combat.state.dead(), e.scout.tuning.robot_radius))
            .collect();
        let owner = crate::weapons::TurretOwner { alive: player.state.health > 0.0, team: player.state.actor.team };
        arsenal.tick_turrets(dt, owner, &arena.0, &arena.0.targets, &scouts, weapon_sound);
        // A round shakes the pad with the weapon's own values, effect type 1. [game]
        if let Some((duration, strength)) = fired {
            player.state.rumbles.push(tf2_core::rumble::Rumble { kind: 1, duration, strength });
        }
        // A charge step's rumble (effect 2 at 0.2, or 3 at 1.0 when full). [game 007addc0]
        if let Some((kind, duration, strength)) = arsenal.charge_rumble {
            player.state.rumbles.push(tf2_core::rumble::Rumble { kind, duration, strength });
        }
        // A blast's (and a ground wave's) ControllerRumble: scaled by his distance from it
        // (scaleType 1), once (playOnce). [game 007dba80, 007dbcb0, 007dbdc0] The distance is from
        // the camera target point 0071a2e0 (007dbdc0 at its head), which is the character's root
        // translation in the car and on foot. [game 0071a2e0] In a flying form (canFly, vehicle
        // motion) that point comes from 0071a460 (read: its z is the lower of the root z and
        // `[B+0x134] - [[B+0xa4]+0x54] * [B+0x128]`), which 0071a2e0 returns (l.9-10). The port's
        // position there is [assumed]. Read 2026-10-20: the call after it, `00562970`, is not a float
        // read (the old label was wrong): it releases the reference at EAX, dropping the count at
        // [ptr+4] and calling 00532c70 at zero (disasm 00562970-005629a4). So the point is 0071a460's
        // alone. OPEN, settles: the holder's world z (B+0x134) and the half-extent [shape+0x54] and up
        // element [B+0x128] (both unnamed), and the port's `flycam::target` (not wired here).
        // The game also gates the rumble on a human pad (`character_pad` 007212f0), not applied here.
        for (at, def) in std::mem::take(&mut arsenal.rumbles) {
            if let Some(strength) = def.strength_at(at.distance(player.state.pos)) {
                if std::env::var_os("TF_DBG").is_some() {
                    eprintln!("rumble start kind {} strength {strength:.2} (blast {:.1} m away)", def.kind, at.distance(player.state.pos));
                }
                player.state.rumbles.push(tf2_core::rumble::Rumble { kind: def.kind, duration: def.duration, strength });
            }
        }
    }
    if let Some(sound) = sound {
        // A trigger message also starts each `SoundPlayer` of the object he is (his own, or
        // the car's) that answers to its name: the special ability's sound, the dodge's
        // jets. [game: 0076d490/0076e500] idtext takes precedence; each player's
        // retrigger flag determines whether its active event is restarted.
        let object = if player.state.car_mode { data.0.vehicle.as_ref() } else { Some(&data.0.robot) };
        for name in &player.state.triggers {
            for (index, definition) in object.iter().flat_map(|o| &o.sound_players).enumerate().filter(|(_, p)| p.script == *name) {
                sound.trigger_event(player.state.car_mode, index, definition);
            }
        }
        // The Untriggers (the special's end, `0072b980`) reach his players of that name in
        // either form, as `message_broadcast_children` reaches all his things. [game]
        for &name in &player.state.untriggers {
            sound.untrigger_name(name);
        }
        // His sounds are heard from where the camera is.
        sound.tick(&player.state, &tuning.0, view.eye, view.forward, time.delta_secs());
    }
}

/// Places the character where the movement has him; the car leans with the ground.
pub fn present(mut player: Single<(&Player, &mut Transform)>) {
    let (player, transform) = &mut *player;
    let state = &player.state;
    let pose=state.car_presentation();
    let (yaw,pitch,roll)=if state.is_vehicle() && !state.is_flying() {pose.angles()} else {(state.yaw,state.pitch,state.roll)};
    transform.translation = game_to_bevy(pose.pos);
    // Pitch lifts the nose and roll the left side, about the car's own axes.
    transform.rotation =
        Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch) * Quat::from_rotation_z(-roll);
    // Test aid: `TF_BODY_LOG=1` prints the drawn place, velocity and angles every frame.
    if std::env::var_os("TF_BODY_LOG").is_some() {
        eprintln!("body {:?} pos {:.2} {:.2} {:.2} vel {:+.2} {:+.2} {:+.2} yaw {:.1} pitch {:.1} roll {:.1}",
            state.mode, pose.pos.x, pose.pos.y, pose.pos.z, state.vel.x, state.vel.y, state.vel.z,
            yaw.to_degrees(), pitch.to_degrees(), roll.to_degrees());
    }
}
