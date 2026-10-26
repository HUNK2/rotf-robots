//! The flying vehicle form (jets, the helicopter): its tuning, as the data gives it, and
//! its step (`Fly`).
//!
//! The data: the `modeFly` and `modeHover` blocks of a character's
//! `characterAttributes<X>` object, read by their own field names [data]. The game's loader
//! for the first is 0085aea0 (the block sits at attributes +0x2e0), for the second 0085bf90
//! (+0x560); the offset of every field is in `docs/flight-attributes.md`, so a formula read
//! off the FlyMode code (`tun+0x58`) can be tied to a name (`max_pitch`).
//!
//! The step: FlyMode's update 00864320, as far as it is ported (see `Fly`). What is read of
//! it is in `docs/flight-update.md`, `flight-forces.md`, `flight-helpers.md`,
//! `flight-steering.md`.

use std::path::Path;

use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};
use crate::flybound::{self, Boundary, Steerback};

fn num(node: Node, name: &str) -> f32 {
    node.get(name).and_then(Node::float).unwrap_or(0.0)
}

fn flag(node: Node, name: &str) -> bool {
    node.get(name).is_some_and(|n| n.int() == Some(1) || n.is_true())
}

/// `modeFly`. Units as the code uses them where that is read: angles in degrees
/// (the code multiplies by 0.0174533), speeds in metres a second, lags as rates per second.
/// Fields whose use is not read yet keep the data's name and nothing more is claimed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlyTuning {
    pub l_stick_deadzone: f32,
    /// [game] 00864320: added to the body's velocity along its forward row, times dt;
    /// `turbo_thrust` in its place while the turbo runs.
    pub thrust: f32,
    pub lateral_thrust: f32,
    pub turbo_thrust: f32,
    pub lift: f32,
    pub lift_quick: f32,
    pub max_speed: f32,
    pub max_turbo_speed: f32,
    pub max_turbo_time: f32,
    pub max_turbo_cooldown_time: f32,
    pub turn_speed: f32,
    pub max_turn_speed: f32,
    pub quick_turn_speed: f32,
    pub turn_lag: f32,
    pub turn_restore_lag: f32,
    pub turn_stick_threshold: f32,
    pub pitch_speed: f32,
    pub max_pitch_speed: f32,
    pub quick_pitch_speed: f32,
    pub pitch_lag: f32,
    pub pitch_lag_auto_level: f32,
    pub pitch_restore_lag: f32,
    pub max_pitch: f32,
    pub min_pitch: f32,
    pub max_roll: f32,
    pub max_roll_quick: f32,
    pub pitch_strength: f32,
    pub roll_strength: f32,
    pub roll_quick_strength: f32,
    pub quick_roll_lag: f32,
    pub quick_climb_scale: f32,
    pub quick_climb_lag: f32,
    pub bullet_slow_speed_percent: f32,
    pub helicopter_turbo_turn_factor: f32,
    pub helicopter: bool,
    pub ai_target_height_offset: f32,
    pub ai_approach_target_distance: f32,
    pub ai_approach_target_height: f32,
    pub ai_approach_target_deceleration: f32,
    pub ai_strafe_target_distance: f32,
    pub ai_feeler_length: f32,
    pub stunt_move_button_press_time: f32,
    pub stunt_move_stick_threshold_x: f32,
    pub stunt_move_stick_threshold_y: f32,
    /// The flying form is the robot itself (the "flight" characters) [assumed from the name].
    pub robot: bool,
    pub robot_additive_steer_strength: f32,
    pub can_do_stunts: bool,
    pub aileron_scale: f32,
    pub elevator_scale: f32,
    pub rudder_scale: f32,
    pub brake_duration: f32,
    pub brake_cooldown: f32,
    pub brake_speed_scale: f32,
    pub post_turbo_deceleration: f32,
    pub braking_speed: f32,
    pub min_rotor_alpha: f32,
    /// `bladeList`: the CRC-32 names of the nodes whose meshes fade to the rotor alpha
    /// (`flightsurface`). [game] 0086cbf0 walks the list at tun+0x1d0 (00765b20); that the
    /// loader puts `bladeList` there is `[assumed]` (the only list in the block; Blackout's
    /// 14 names are all rotor parts).
    pub blade_list: Vec<u32>,
    /// [game] the body's extent for the low-speed rules (00866ef0 00867364-00867394 and
    /// 0086b230 0086c49c-0086c504): the game takes `2 * e[-0x10] * |sin roll|` and
    /// `2 * e[-0xc] * |sin pitch|` off the drop ray's distance, where `e` is the table at
    /// `char+0x140` (+0x198 base, +0x194 count; 28-byte rows). The loader 00726710 (l.1335-1488)
    /// fills it from `avatarDescArray` (width at +0xc, length at +0x10), and `e` is one past the
    /// last row, so these are the LAST row's width (roll) and length (pitch): `Forms::read` takes
    /// that row. Level flight is unchanged whatever the extent. (Settled 2026-10-20: the field
    /// is the count, not an index; the earlier reading as `avatarDescArray[1]` agreed only for
    /// the two-row characters.)
    pub extent_roll: f32,
    pub extent_pitch: f32,
}

impl FlyTuning {
    pub fn read(m: Node) -> Self {
        Self {
            l_stick_deadzone: num(m, "lStickDeadzone"),
            thrust: num(m, "thrust"),
            lateral_thrust: num(m, "lateralThrust"),
            turbo_thrust: num(m, "turboThrust"),
            lift: num(m, "lift"),
            lift_quick: num(m, "liftQuick"),
            max_speed: num(m, "maxSpeed"),
            max_turbo_speed: num(m, "maxTurboSpeed"),
            max_turbo_time: num(m, "maxTurboTime"),
            max_turbo_cooldown_time: num(m, "maxTurboCooldownTime"),
            turn_speed: num(m, "turnSpeed"),
            max_turn_speed: num(m, "maxTurnSpeed"),
            quick_turn_speed: num(m, "quickTurnSpeed"),
            turn_lag: num(m, "turnLag"),
            turn_restore_lag: num(m, "turnRestoreLag"),
            turn_stick_threshold: num(m, "turnStickThreshold"),
            pitch_speed: num(m, "pitchSpeed"),
            max_pitch_speed: num(m, "maxPitchSpeed"),
            quick_pitch_speed: num(m, "quickPitchSpeed"),
            pitch_lag: num(m, "pitchLag"),
            pitch_lag_auto_level: num(m, "pitchLagAutoLevel"),
            pitch_restore_lag: num(m, "pitchRestoreLag"),
            max_pitch: num(m, "maxPitch"),
            min_pitch: num(m, "minPitch"),
            max_roll: num(m, "maxRoll"),
            max_roll_quick: num(m, "maxRollQuick"),
            pitch_strength: num(m, "pitchStrength"),
            roll_strength: num(m, "rollStrength"),
            roll_quick_strength: num(m, "rollQuickStrength"),
            quick_roll_lag: num(m, "quickRollLag"),
            quick_climb_scale: num(m, "quickClimbScale"),
            quick_climb_lag: num(m, "quickClimbLag"),
            bullet_slow_speed_percent: num(m, "bulletSlowSpeedPercent"),
            helicopter_turbo_turn_factor: num(m, "helicopterTurboTurnFactor"),
            helicopter: flag(m, "helicopter"),
            ai_target_height_offset: num(m, "aiTargetHeightOffset"),
            ai_approach_target_distance: num(m, "aiApproachTargetDistance"),
            ai_approach_target_height: num(m, "aiApproachTargetHeight"),
            ai_approach_target_deceleration: num(m, "aiApproachTargetDeceleration"),
            ai_strafe_target_distance: num(m, "aiStrafeTargetDistance"),
            ai_feeler_length: num(m, "aiFeelerLength"),
            stunt_move_button_press_time: num(m, "stuntMoveButtonPressTime"),
            stunt_move_stick_threshold_x: num(m, "stuntMoveStickThresholdX"),
            stunt_move_stick_threshold_y: num(m, "stuntMoveStickThresholdY"),
            robot: flag(m, "robot"),
            robot_additive_steer_strength: num(m, "robotAdditiveSteerStrength"),
            can_do_stunts: flag(m, "canDoStunts"),
            aileron_scale: num(m, "aileronScale"),
            elevator_scale: num(m, "elevatorScale"),
            rudder_scale: num(m, "rudderScale"),
            brake_duration: num(m, "brakeDuration"),
            brake_cooldown: num(m, "brakeCooldown"),
            brake_speed_scale: num(m, "brakeSpeedScale"),
            post_turbo_deceleration: num(m, "postTurboDeceleration"),
            braking_speed: num(m, "brakingSpeed"),
            min_rotor_alpha: num(m, "minRotorAlpha"),
            blade_list: m.get("bladeList").into_iter().flat_map(Node::items)
                .filter_map(|b| b.get("bladeName").and_then(Node::int)).map(|h| h as u32).collect(),
            extent_roll: 0.0,
            extent_pitch: 0.0,
        }
    }
}

/// `modeHover`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HoverTuning {
    pub max_hover_time: f32,
    pub move_acceleration: f32,
    pub max_move_speed: f32,
    pub hover_done_particles_pct: f32,
    pub hover_deceleration: f32,
    pub max_lean_degrees: f32,
    pub lean_strength: f32,
}

impl HoverTuning {
    pub fn read(m: Node) -> Self {
        Self {
            max_hover_time: num(m, "maxHoverTime"),
            move_acceleration: num(m, "moveAcceleration"),
            max_move_speed: num(m, "maxMoveSpeed"),
            hover_done_particles_pct: num(m, "hoverDoneParticlesPct"),
            hover_deceleration: num(m, "hoverDeceleration"),
            max_lean_degrees: num(m, "maxLeanDegrees"),
            lean_strength: num(m, "leanStrength"),
        }
    }
}

/// What a character's attributes say of its forms. [data] `canFly` and `canDrive` are the
/// attributes' own flags: a character with `canFly` has the FlyMode and no DriveMode
/// (`docs/bumblebee-mechanics.md`, the mode table).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Forms {
    pub can_fly: bool,
    pub can_drive: bool,
    pub fly: FlyTuning,
    pub hover: HoverTuning,
}

impl Forms {
    pub fn read(attributes: Node) -> Self {
        let mut fly: FlyTuning = attributes.get("modeFly").map(FlyTuning::read).unwrap_or_default();
        // [game 00726710 l.1335-1488] the loader stores the descriptor count at char+0x194 and the
        // 28-byte rows at char+0x198 (fields width +0xc, length +0x10). The low-speed rules read
        // e = base + 28 * count, one past the end (00866ef0 00867338-00867354, 0086c49c-0086c4a8):
        // e[-0x10] is the LAST row's width and e[-0xc] its length. Every flying character has
        // two rows, so that is the vehicle avatar (`avatarDescArray[1]`); a one-row character
        // takes row 0. [data] the rows are `avatarDescArray` in `export\data\global_states.txt`.
        if let Some(avatars) = attributes.get("avatarDescArray")
            && let Some(last) = avatars.len().checked_sub(1).and_then(|i| avatars.at(i)) {
            fly.extent_roll = num(last, "width");
            fly.extent_pitch = num(last, "length");
        }
        Self {
            can_fly: flag(attributes, "canFly"),
            can_drive: flag(attributes, "canDrive"),
            fly,
            hover: attributes.get("modeHover").map(HoverTuning::read).unwrap_or_default(),
        }
    }

    /// From the install, by the `X` of `characterAttributes<X>`.
    pub fn load(game_dir: &Path, character: &str) -> Result<Self, String> {
        let pack = Pack::open(&game_dir.join("bnxglobal.str"))?;
        let name = format!("characterAttributes{character}");
        pack.of_type(pack::DATA)
            .filter_map(|chunk| DataFile::parse(pack.data(chunk).to_vec()).ok())
            .find_map(|file| file.root().get(&name).map(Self::read))
            .ok_or_else(|| format!("bnxglobal.str has no {name}"))
    }
}

/// What the player asks of the flying form in one update.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlyInput {
    /// The left stick: x to the right, y forward, each -1 to 1.
    pub stick: glam::Vec2,
    /// `FlyingAfterburn` (button 0x1a, on the jump button [data]) went DOWN this update.
    /// [game] 00864320 reads it through the input source's slot +0xc, which in the player's
    /// pad reader (vtable 00b6251c) is 0084f980 `pad_button_pressed` (slot +0x8 is
    /// 0084f900, held). A press starts the turbo only when it is ready (`Fly::abilities`).
    pub turbo: bool,
    /// `FlyingBrake` (button 0x1b, the car's brake binding [data]) is down. [game] 00864320
    /// reads its PRESS (slot +0xc as above); `Fly` keeps the last state to find the press.
    pub brake: bool,
    /// The status scale on both speed caps (0086a840 / 0086a950: the slowed multiplier,
    /// else `bulletSlowSpeedPercent` (tun+0x80) while bullet-slowed, else 1.5 while a
    /// type-2 special runs). [game] `None` is 1, nothing slowing it.
    pub cap_scale: Option<f32>,
    /// The option "invert pitch" ([game] 00866ef0 negates the pitch input when it is set).
    pub invert_pitch: bool,
    /// Button 0x1f is down: the tight turn (008676c0 steers in 00866ef0's place). [data]
    /// the binding list calls it `FlyingStuntMode` and puts it on the weapon-mode trigger.
    /// [data] the number 0x1f is the list's order (`docs/state-machine.md` "The data": the
    /// enum is the binding list with the `Flying*` entries after WeaponSwitchUp). That order
    /// is checked on the numbers the state data uses (0x15, 0x17, 0x18, 0x21-0x24, global_states.txt).
    /// 0x1f itself appears in no state, so the number rests on the order alone.
    pub hold: bool,
    /// The other stick, x to the right and y up: the helicopter turns and climbs by it.
    /// [game] 00864320 reads it from the input source's slot +0x28, with the dead zone
    /// and not squared. [game] that slot is the camera's stick: in the player's pad reader
    /// (vtable 00b6251c) slot +0x28 is 0084fc60, which returns the pad table pair
    /// d1e864/d1e868, the pair 009276e0 copies into the camera behaviour's stick
    /// (+0x6c/+0x70). (Slot +0x30, `stick`, is 0084fcd0: the pair d1e85c/d1e860, y negated.)
    /// Its y sign against the port's `look` is not checked here.
    pub look: glam::Vec2,
    /// Stunts are armed (the mode's byte +0x4a1). [game] 008013e0 copies it each update
    /// from whether a control state (hash d73818a4, not named) is enabled.
    /// [assumed] that this is the `FlyingStunt` action shortly after its press (the same tag as `sim.rs`'s
    /// `step_fly`, which arms it from the ACTION or CLIMB press for `stuntMoveButtonPressTime`). Reached by the
    /// playable flyers. Not settled: hash d73818a4 is not in names.tsv, and the CRC-32 of FlyingStunt (a6bf9b9d,
    /// names.tsv), FlyStunt, FlightStunt, StuntMove, StuntMoveButton, FlyingStuntButton, FlyingStuntAction and
    /// others do not give it (2026-10-20 re-check here, and batch C3's 40 data identifiers).
    /// OPEN, settles: the control-state name for d73818a4 (008013e0's view element, `docs/flight-stunts.md` 1.2).
    /// 2026-10-20 tried: CRC-32 and the h*33+c hash of 175 stunt / fly identifiers from export\data and
    /// export\strings.tsv: no match. Not in names.tsv, and -684189532 / 3610777764 appear in no decoded data
    /// file. `tools\tf2_bxdata.py actions` lists only the pad's button names, not controlstates. Next: the
    /// view that 008013e0 walks (its element records, 00573ad0) and the state machine's controlstate table
    /// (`export\data\global_states.txt`, grep for the controlstate of the flight states). `stuntMoveButtonPressTime`
    /// (names.tsv e5eb9f9b) is in the data but read by no code yet.
    pub stunt: bool,
    /// Set when the controller is the AI: 00864320 then runs 008693f0 in place of the
    /// player's steering 00866ef0 (the test is `controller_is_ai`, 00854df0). `stick` still
    /// carries the AI's own stick, shaped the same way. [game]
    pub ai: Option<AiSteer>,
}

/// What the AI's flight steering (008693f0) reads besides the stick. [game]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AiSteer {
    /// The point it flies at, world. [game] mode+0x4dc..0x4e4. Its writer is the AI's side
    /// (not in the flight census, not traced).
    pub target: glam::Vec3,
    /// The probe along the nose hit something. [game] mode+0x4c0, set by 0086c970 when the
    /// probe of length `tun+0xa0` reports a hit; it triples the turn rate.
    /// [stand-in] the caller decides it: that probe is not cast in the port. (An AI flyer's field only:
    /// the player's steering never reads it, the player passes `ai: None`.)
    /// [game 0086c970, read 2026-10-20] The game's rule: when the controller is not dead
    /// (char+0x84 == 0) and mode+0x430 is set, the probe segment starts at the character's root
    /// (+0x34..0x3c) and runs along the forward basis row (+0x1c..0x24) for `tun+0xa0`; it is
    /// "obstacle" when the hit distance mode+0x484 is below FLT_MAX (the readback is 0086c970's
    /// caller's job, not read). The caller casts that ray and sets this field.
    /// OPEN: the readback that writes mode+0x484 is not read; `sim` passes `ai: None` for the
    /// player, so nothing reaches this field yet.
    pub obstacle_ahead: bool,
}

/// A stunt under way (mode+0x520), numbered as the game numbers them. [game] 0086a590
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Stunt {
    #[default]
    None,
    Down,
    Left,
    Right,
    Up,
}

impl Stunt {
    /// The message its start sends the state machine (`FlyStuntDown`...). [game]
    pub fn message(self) -> Option<u32> {
        match self {
            Stunt::None => None,
            Stunt::Down => Some(0x8aa9_abbb),
            Stunt::Left => Some(0xec31_dce8),
            Stunt::Right => Some(0x59e4_a00f),
            Stunt::Up => Some(0xd512_ac49),
        }
    }
}

/// What the flying form's rays found (0086b230), for one update. [game] the game reads
/// each ray's answer an update after it was cast, and so does `Fly` (it keeps `walls` for
/// the next update); the caller casts them from the pose at the start of the update.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Surroundings {
    /// The height of the floor under it.
    pub ground: Option<f32>,
    /// The three rays 35 m along the nose (`Fly::wall_rays`): how far along the hit is, in
    /// metres, and the surface's normal.
    pub walls: [Option<(f32, glam::Vec3)>; 3],
}

/// [game] 0086b230: the length of the rays along the nose.
pub const WALL_RAY: f32 = 35.0;
/// Where the second and third rays start, in the body's frame (x right, y nose, z up): the
/// local points mode+0x52c..0x534 and +0x538..0x540 (`docs/flight-contact.md` 1, rays 2 and
/// 3). [game] Their writer is FlyMode's constructor 00863970, read 2026-10-18:
/// `param_2[0x14b..0x150]` = -5.0, 0, -1.5 and 5.0, 0, -1.5 (0xc0a00000, 0, 0xbfc00000,
/// 0x40a00000, 0, 0xbfc00000): five metres to each side and a metre and a half below.
pub const SIDE_RAYS: [glam::Vec3; 2] = [glam::Vec3::new(-5.0, 0.0, -1.5), glam::Vec3::new(5.0, 0.0, -1.5)];

/// The flying form in normal flight under the player: FlyMode's update 00864320 with its
/// steering 00866ef0, the tight turn 008676c0 while its button is held, and its speed layer
/// 00869df0, as read in `docs/flight-update.md`, `flight-steering.md`, `flight-forces.md`
/// and `flight-common-step.md`.
///
/// Also here (2026-10-12 to 16): the stunts' trigger (0086a590), the helicopter variant
/// (00867f20), the AI's steering (008693f0, `steer_ai`), the boundary's bounce and the wall
/// stop (00868b00, 00868fb0), the walls ahead (0086e020).
/// NOT here (each is read in part and listed in the unpublished notes): the impact states (the
/// control vector mode+0xc..0x14, 00866a70), the probes as the game casts them (0086b230,
/// 0086c970), the control surfaces' animation (0086cbf0), the brake.
/// FlyMode's two timed abilities as the character keeps them: the turbo (mode+0x34) and the
/// brake (mode+0x38) with the turbo's ended byte (+0x24). [game] The FlyMode object is built
/// once (00863970) and its abilities registered with the character's ability list (007235f0),
/// which runs them in every mode; the enter (00864d60) does not touch +0x34 / +0x38, and the
/// exit (00865b80, l.202) only calls the turbo's slot +0x10 (00733290, its effects) with
/// (0, 1). So a cooldown carries over from one flight to the next and runs out on foot.
/// `sim::State::fly_timers` holds them; `Fly` works on a copy during its update.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlyTimers {
    pub turbo_left: f32,
    pub turbo_cooldown: f32,
    pub turbo_ended: bool,
    pub brake_left: f32,
    pub brake_cooldown: f32,
    /// [game] mode+0x4a8 and +0x4ac, the after-turbo slowdown and its speed latch. The enter
    /// (00864d60) writes neither, so they belong to the FlyMode object with the timers and
    /// carry over from one flight to the next (see `Fly::post_turbo`).
    pub post_turbo: bool,
    pub speed_latch: f32,
    /// The turbo's single-player upgrade multipliers (duration, cooldown); `None` is 1, the base.
    /// [game] 00732e60 multiplies the duration at the press (state 1) and the cooldown at the end
    /// (state 2) by upgrade_table_entry 0079d060 for a human player outside multiplayer; the
    /// class is the car's (`driving::turbo_scales`), so the levels are the car's
    /// (`sim::State::turbo_upgrades`). [trace] the 2026-10-21 Starscream capture holds a turbo
    /// 5.0 s from the press, 2.5 s (his maxTurboTime) x 2 (level 3), three times.
    pub upgrade: Option<(f32, f32)>,
}

impl FlyTimers {
    /// One update of both timers (00733180 the turbo, 007324d0 the brake): the time left, then
    /// the whole cooldown. [game] 00732e60 state 2 sets the turbo's +0x24 only when the
    /// character is in DriveMode or FlyMode (00736c80 / 00725890): `in_vehicle`.
    pub fn tick(&mut self, tun: &FlyTuning, in_vehicle: bool, dt: f32) {
        let cooldown = tun.max_turbo_cooldown_time * self.upgrade.map_or(1.0, |u| u.1);
        if crate::driving::step_turbo(&mut self.turbo_left, &mut self.turbo_cooldown, cooldown, dt) && in_vehicle {
            self.turbo_ended = true;
        }
        crate::driving::step_turbo(&mut self.brake_left, &mut self.brake_cooldown, tun.brake_cooldown, dt);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fly {
    pub pos: glam::Vec3,
    pub vel: glam::Vec3,
    /// Radians. [game] mode+0x1c, +0x20, +0x24. Their conventions [game]: 00864320 writes
    /// them from 00570250 (+0x1c = asin of the forward column's z, +0x20 = atan2(-col0.z,
    /// col2.z), +0x24 = atan2(-col1.x, col1.y)) and 00866a70 rebuilds the body with 00570470,
    /// Rz(+0x24) Rx(+0x1c) Ry(+0x20) with positive (right-handed) factors (read in narrow
    /// disassembly, `docs/surface-damage.md`). So pitch above zero is nose up, roll is about
    /// the nose, and forward is (-sin h, cos h) as `sim`'s yaw (`docs/flight-boundary.md` 12.9).
    pub pitch: f32,
    pub roll: f32,
    pub heading: f32,
    /// Radians a second. [game] mode+0x44.
    pub yaw_rate: f32,
    /// [game] mode+0x4b8: 0 on entering, +2 a second up to 1; it scales a pitch input that
    /// is not negative.
    pub ramp: f32,
    /// Seconds of turbo left, and of its cooldown: the mode's own standard turbo (mode+0x34,
    /// built by flymode_init 00863970 through turbo_init 00732da0, vtable 00b5d61c, the
    /// class DriveMode's turbo is) with +0x14 = `maxTurboTime` (tun+0x20) and +0x1c =
    /// `maxTurboCooldownTime` (tun+0x24). [game] 00733180 runs it out (no release ends it),
    /// then the cooldown; 007339a0 says it is ready only when both are spent (state 0).
    pub turbo_left: f32,
    pub turbo_cooldown: f32,
    /// [game] the turbo's byte +0x24: set when its time runs out while in FlyMode (00732e60
    /// state 2, 007330b5 `is_in_car_type`); 00869df0 then brings the speed down from the
    /// turbo cap.
    pub turbo_ended: bool,
    /// [game] mode+0x4a8: the speed after a turbo is being brought down (`speed_latch`).
    /// [game 00864d60] the enter writes neither +0x4a8 nor +0x4ac (its writes are listed in
    /// the unpublished notes), so like the timers they are kept on the character
    /// (`FlyTimers`, set by `set_timers`). A turbo that ended in the last flight with its
    /// slowdown unfinished starts the next one at the turbo cap (00869df0 with the turbo's +0x24
    /// still set).
    pub post_turbo: bool,
    /// [game] mode+0x4ac: the speed latch. After a turbo: the turbo cap, less
    /// `postTurboDeceleration` a second, to the normal cap. In a brake: the speed at the
    /// press, less `brakingSpeed` a second, to `brakeSpeedScale` of the normal cap.
    pub speed_latch: f32,
    /// The turbo's upgrade multipliers, from the character's timers (`FlyTimers::upgrade`).
    pub upgrade: Option<(f32, f32)>,
    /// The brake (mode+0x38, a base timed ability built by 00732430, vtable 00b5d610):
    /// seconds left (`brakeDuration`, tun+0xc8) and of its cooldown (`brakeCooldown`,
    /// tun+0xcc). [game] 007324d0 runs both down; 00732c20 says ready when both are spent.
    pub brake_left: f32,
    pub brake_cooldown: f32,
    brake_was_down: bool,
    /// [game] the movement owner's avatar blend (+0x248): FlyMode's enter switches the
    /// avatar from the robot's pencil to the vehicle's cuboid (00853cb0 with EAX = 1 at
    /// 00865244; the enter has already put its own animation name in owner+0x3fc, so the
    /// drive enter's exemption fc2cb442 does not apply) and that sets it to 1; 00854160
    /// takes dt off each step. While it is above zero the character's camera message
    /// (0071a5c0) asks for camera mode 5e89f579, under which the driving camera's
    /// airborne test (009145b0) is false: `camera::Mode::Transition`.
    pub avatar_blend: f32,
    /// The status scale of this update's caps (`FlyInput::cap_scale`).
    cap_scale: f32,
    /// The rays' answers cast last update, which this update reads (`Surroundings`). [game]
    walls_cast: [Option<(f32, glam::Vec3)>; 3],
    /// [game] mode+0x4c: the hold button was down at the last update.
    pub holding: bool,
    /// [game] mode+0x50: the roll the tight turn is latched to, radians; 0 is none yet.
    pub quick_roll: f32,
    /// [game] mode+0x48: the helicopter's pitch rate while the hold button is down.
    pub pitch_rate: f32,
    /// [game] mode+0x544..0x54c: the push off a wall, already times dt, eased at 3 a second.
    pub wall_push: glam::Vec3,
    /// The helicopter steering's lift P at its end, when that steering ran in this update:
    /// its last block eases the rotor speed by it (`flightsurface::Surfaces::steer_rotor`,
    /// 00867f20 SM). [game]
    pub rotor_lift: Option<f32>,
    /// [game] mode+0x4b4: one of the rays has a wall within 10 m.
    pub near_wall: bool,
    /// The stunt under way, and (mode+0x524) that a roll has yet to set its velocity.
    pub stunt: Stunt,
    stunt_fresh: bool,
    /// FlyMode's control vector (mode+0xc..+0x14), which a clip's `ChangeTractionEvent` writes
    /// (`traction`). Zero in plain flight; while it is not, the stick steers nothing and no stunt
    /// starts (00866ef0, 00867f20, 0086a590 run their bodies only for exactly (0, 0, 0)), and the
    /// clip's root drives the body on the axes it names. [game 00866900] Kept across the mode's
    /// exit and enter (no reset found). [game]
    pub control: glam::Vec3,
    /// The boundary's contact state: `mode+0x55` (outside), the push and the normal, and the
    /// bounce's own fields. Only written by `step_in_bounded` with a boundary. [game] 0086ab90
    pub steer: Steerback,
    /// [game] EnvInsideFlightBoundary is `mode+0x55 == 0`: the release and punch connections
    /// require it. True when no boundary is passed. [game] (see docs/flight-boundary.md 8)
    pub inside_boundary: bool,
    /// The point the boundary test and its N use, when the caller sets it (`flycam::flyer_point`):
    /// the game's P is the character's camera target point (007e7830 l.7 writes it from 0071a2e0).
    /// `None` keeps `pos`, the port's stand-in. [game 007e7830, 0071a460]
    pub boundary_point: Option<glam::Vec3>,
    /// [game] mode+0x4bc: the 3 m variant. The ground latch and the low-speed lift read 3 m and
    /// 2.85 instead of 2 m and 1.85 when it is set (0086b230 0086c514-0086c531, 00866ef0
    /// 008673c2-008673de). The game sets it from the object the drop ray hit: bit 0x80 of its
    /// +0x9e, the object the handle at mode+0x3b8 locks (0086b230 0086bceb-0086bd1f: the weak
    /// ref lock 00544db0, then bit 0x80 of +0x9e, stored to mode+0x4bc unconditionally [game]).
    /// [game] The handle at mode+0x3b8 is the drop probe's hit object. The drop probe is the
    /// flight's embedded probe at mode+0x310 (segment +0x00, hit point +0x30, normal +0x3c..+0x44,
    /// handles +0xa0 / +0xa8, done +0x60, distance +0xb4 = mode+0x3c4), and its results are written
    /// by the probe-result collector 006e2a10 from the physics record (its stores at 006e2a10
    /// l.37-45: the normal's z is +0x44 = mode+0x354; the second handle is +0xa8 = mode+0x3b8).
    /// `sim.rs` casts the drop ray and sets this from the hit's `GroundHit::object_mask` (bit 0x80).
    /// [game 0086b230 l.174-177, read 2026-10-20] The word is settled: the 3 m test reads BYTE +0x9e of the
    /// object the drop probe's second handle locks (probe block +0xa8 = mode+0x3b8, written by 006e2a10 l.24
    /// from the physics record's handle at +4), bit 0x80. `sim.rs` takes it from `GroundHit::object_mask`, which
    /// only the LEVEL hit fills (`level_world.rs` gives the primitive's receive word, `sim.rs` l.354); a hit on
    /// any other object gives 0, so the latch stays off there. [assumed] that the hit object is a level
    /// primitive and that its +0x9e is that receive mask (the same tag as `level_world.rs` move_pencil's note).
    /// OPEN, settles: what the probe's record names as its object (the producer of the physics record, 006e22f0
    /// results and the discrete / swept tests of `docs/physics.md` 1.13, not read here), so whether the hit is a
    /// level primitive or a body. The bit 0x80 is WATER in the mask list of `docs/melee.md`, so the bit's meaning
    /// on a level primitive is also unconfirmed. Reached by the playable flyers on a level.
    pub three_metre_latch: bool,
    /// [game] mode+0x354: the z of the drop probe's hit normal (probe block +0x44, written by
    /// 006e2a10, see `three_metre_latch`). The latch needs it over 0.5 in magnitude
    /// (0086c53e-0086c562, the 0.5 constant at 00b38830). `sim.rs` sets it from the drop ray's hit
    /// normal each update. On a miss there is no ground and the latch is off (no-hit branch
    /// 0086c627-0086c65e) [game], so the value a miss leaves (1.0) is never read.
    pub ground_normal_z: f32,
}

const DEG: f32 = 0.017_453_292;

/// [game] the lag every angle uses: toward the target at `rate` a second, or straight to
/// it when the rate is not positive or the step would overshoot.
fn lag(value: f32, target: f32, rate: f32, dt: f32) -> f32 {
    if rate > 0.0 && rate * dt < 1.0 { value + (target - value) * rate * dt } else { target }
}

/// [game] to [-pi, pi): 2 pi (frac(x / 2 pi + 0.5) - 0.5), `frac` being x - floor(x):
/// 00542f80 is `floor_float_wrapper` (the CRT floor, ledger), the same pattern as
/// 009145b0's own wrap.
fn wrap(angle: f32) -> f32 {
    let turns = angle * 0.159_154_94 + 0.5;
    std::f32::consts::TAU * (turns - turns.floor() - 0.5)
}

/// [game] FUN_0086d5c0: nothing inside the dead zone, the rest stretched to the full range.
fn dead_zone(a: f32, zone: f32) -> f32 {
    if a.abs() < zone { 0.0 } else { a.signum() * (a.abs() - zone) / (1.0 - zone) }
}

/// [game] FUN_0056e4e0: a frame whose forward (the game's row 1, glam's y column) is `dir`,
/// keeping as much of `frame`'s up as it can, and `frame`'s place. Up is `frame`'s up less
/// its part along `dir` while the two are no nearer than |cos| 0.98; past that it is
/// `frame`'s right crossed with `dir`. Right is forward crossed with up. A `dir` shorter than
/// 0.01 gives `frame` back.
pub fn look_at(frame: glam::Mat3, dir: glam::Vec3) -> glam::Mat3 {
    if dir.length_squared() < 0.0001 {
        return frame;
    }
    let f = dir.normalize();
    let along = frame.z_axis.dot(f);
    let up = if along.abs() <= 0.98 { frame.z_axis - f * along } else { frame.x_axis.cross(f) };
    let length = up.length();
    let up = if length > 1e-8 { up / length } else { up };
    glam::Mat3::from_cols(f.cross(up), f, up)
}

/// [game] FUN_0056dfe0 with FUN_0056db30: `from` turned toward `to` by at most `angle`. Both
/// frames become quaternions (0052efb0); when the angle between the quaternions,
/// acos |qa . qb|, is no more than `angle` the result is `to`, else the slerp at
/// angle / that, taken the short way (the sign flip of 0056db30) and normalised (00545630,
/// 005456a0). That angle is HALF the turn between the frames, so `angle` lets the frame turn
/// by up to twice itself. [game] (0056db30's compare of |angle| with acos; checked which
/// argument is which at 0056e3e9-0056e3fe and in the caller 008698d8-008698f3.)
/// [game 0056db30] the slerp's weights are read from its disassembly (0056ddef-0056de45): with
/// the half-angle theta = |acos(qa . qb)| and t = angle / theta, weight A = sin((1 - t) theta) /
/// sin theta and weight B = sin(t theta) / sin theta, which is glam's slerp. Below theta 0.001
/// the game takes the linear form instead (0056dc5c, the same weights without the sines), and
/// normalises the result (00545630 / 005456a0). glam's slerp, which takes the short way too,
/// is used.
pub fn turn_toward(from: glam::Mat3, to: glam::Mat3, angle: f32) -> glam::Mat3 {
    let (qa, qb) = (glam::Quat::from_mat3(&from).normalize(), glam::Quat::from_mat3(&to).normalize());
    let d = qa.dot(qb);
    if d.abs() > 0.999_999 {
        return glam::Mat3::from_quat(qa);
    }
    let between = d.abs().acos();
    if angle.abs() >= between {
        return glam::Mat3::from_quat(qb);
    }
    glam::Mat3::from_quat(qa.slerp(qb, angle.abs() / between).normalize())
}

/// [game] 00570250, a frame to (pitch, roll, heading): pitch = asin of the forward's z, roll =
/// atan2(-right.z, up.z), heading from the forward's level part, the inverse of
/// `Fly::rotation` (Rz Rx Ry). [game] read 2026-10-18 in full: the right and forward columns
/// are normalised, the up column only scaled for its z; when |forward.z| >= 0.9999 (straight up
/// or down) the roll is 0 and the heading atan2(right.y, right.x), otherwise roll
/// atan2(-right.z, up.z) and heading atan2(-forward.x, forward.y); pitch is asin(forward.z).
pub fn angles_of(frame: glam::Mat3) -> (f32, f32, f32) {
    let right = frame.x_axis / frame.x_axis.length();
    let forward = frame.y_axis / frame.y_axis.length();
    let up_z = frame.z_axis.z / frame.z_axis.length();
    let (roll, heading) = if forward.z.abs() >= 0.9999 {
        (0.0, right.y.atan2(right.x))
    } else {
        ((-right.z).atan2(up_z), (-forward.x).atan2(forward.y))
    };
    // The clamp is the port's guard against rounding past 1 (the game takes asin raw).
    (forward.z.clamp(-1.0, 1.0).asin(), roll, heading)
}

impl Fly {
    /// As the game enters the mode: the velocity it had is kept ([game] 00864d60 clamps it
    /// and sets none), angles from the body's matrix, the ramp at 0.
    /// [game] The turbo and brake timers belong to the FlyMode object, which is built once
    /// (00863970) and registered with the character's abilities (007235f0); the enter (00864d60)
    /// writes neither their +0x34 / +0x38 nor the speed latch, so a cooldown carries over from
    /// one flight to the next and keeps ticking on foot. The port's `Fly` lives only while
    /// flying, so these are kept on `sim::State` (`fly_timers`) and put in each step.
    /// The turbo, brake and speed-latch state, to keep on the character between flights (`FlyTimers`).
    pub fn timers(&self) -> FlyTimers {
        FlyTimers {
            turbo_left: self.turbo_left,
            turbo_cooldown: self.turbo_cooldown,
            turbo_ended: self.turbo_ended,
            brake_left: self.brake_left,
            brake_cooldown: self.brake_cooldown,
            post_turbo: self.post_turbo,
            speed_latch: self.speed_latch,
            upgrade: self.upgrade,
        }
    }

    /// Puts the character's turbo, brake and speed-latch state into this flight (`FlyTimers`).
    pub fn set_timers(&mut self, t: FlyTimers) {
        (self.turbo_left, self.turbo_cooldown, self.turbo_ended, self.brake_left, self.brake_cooldown) =
            (t.turbo_left, t.turbo_cooldown, t.turbo_ended, t.brake_left, t.brake_cooldown);
        (self.post_turbo, self.speed_latch, self.upgrade) = (t.post_turbo, t.speed_latch, t.upgrade);
    }

    /// A flight that starts with the timers all spent; `sim` then puts the character's own in
    /// (`set_timers`).
    pub fn enter(pos: glam::Vec3, vel: glam::Vec3, heading: f32) -> Self {
        Self { pos, vel, pitch: 0.0, roll: 0.0, heading, yaw_rate: 0.0, ramp: 0.0, turbo_left: 0.0, turbo_cooldown: 0.0, turbo_ended: false, post_turbo: false, speed_latch: 0.0, upgrade: None, brake_left: 0.0, brake_cooldown: 0.0, brake_was_down: false, avatar_blend: 1.0, cap_scale: 1.0, walls_cast: [None; 3], holding: false, quick_roll: 0.0, pitch_rate: 0.0, wall_push: glam::Vec3::ZERO, near_wall: false, stunt: Stunt::None, stunt_fresh: false, control: glam::Vec3::ZERO, steer: Steerback::default(), inside_boundary: true, boundary_point: None, three_metre_latch: false, ground_normal_z: 1.0, rotor_lift: None }
    }

    /// A `ChangeTractionEvent` while in FlyMode (`motion_control_set` 00854960 -> 00866900, read
    /// in disassembly, `docs/clip-events-traction-modes.md` 2.1). A vector that is not all zero
    /// is only stored. All zero ends the stunt, whatever its kind: the heading is taken again
    /// from where the nose points (00570610: atan2(-x, y) of the nose, kept when it points
    /// straight up or down); a loop (Down, Up) has its pitch and roll cleared, a roll keeps its
    /// decaying roll; a jet (neither the helicopter's `+0x84` nor the robot flyer's `+0xb0`
    /// flag) is sent along that nose at its normal cap (0086a840: maxSpeed times the status
    /// scale); the stunt is over. The jets' stunt clips send [1, 1, 1] at their first key and
    /// [0, 0, 0] at their last (starscream, firestorm FlySet), so a stunt lasts its clip. [game]
    /// [game 00570610: heading is 0046be80(-x, y) of the nose; 0046be80 is the CRT's _CIatan2]
    /// Whether body_add_velocity (00556b40) takes a vector now: it adds only when the body wrapper's
    /// +0x34 float is 0.0 and its +0x38 byte is 0. Those are the z of the control vector and the first
    /// word of the turn vector that holder_control_set 006e8da0 stores at wrapper +0x2c..+0x40 each update,
    /// before the mode's own update (00852220); the turn vector is always zero, and the control is
    /// FlyMode's stored traction (`control`; the owner's impulse and override vectors that replace it have
    /// z 0). So a clip that drives z (a stunt's [1,1,1]) blocks the bounce and the ground push. [game
    /// 00556b40, 006e8da0, 00852220; docs/bumblebee-mechanics.md "How a mode's request reaches the body"]
    /// (The PhysicsOverride loader 0077eb30 writes the same fields; no robot pack has one: a decode of the
    /// 13 packs' chunk 0, 2026-10-20.)
    pub fn body_takes_velocity(&self) -> bool {
        self.control.z == 0.0
    }

    pub fn traction(&mut self, control: glam::Vec3, tun: &FlyTuning) {
        if control != glam::Vec3::ZERO {
            self.control = control;
            return;
        }
        self.control = glam::Vec3::ZERO;
        // The body's nose as it stands, before the loops' angles are cleared (the velocity is
        // the body matrix's row 1, 8669dd-866a4e).
        let nose = self.forward();
        if nose.z.abs() < 0.9999 {
            self.heading = (-nose.x).atan2(nose.y);
        }
        if matches!(self.stunt, Stunt::Down | Stunt::Up) {
            (self.pitch, self.roll) = (0.0, 0.0);
        }
        if !tun.helicopter && !tun.robot {
            self.vel = nose * tun.max_speed * self.cap_scale;
        }
        self.stunt = Stunt::None;
        self.stunt_fresh = false;
    }

    /// The nose: the forward column of `rotation` (roll about the nose leaves it). [game] see `pitch`.
    pub fn forward(&self) -> glam::Vec3 {
        let (sh, ch) = self.heading.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        glam::Vec3::new(-sh * cp, ch * cp, sp)
    }

    /// The body's turn for drawing: heading about z, then pitch about x, then roll about y.
    /// [game] 00570470 builds Rz(+0x24) Rx(+0x1c) Ry(+0x20) (12.9, `pitch`).
    pub fn rotation(&self) -> glam::Quat {
        glam::Quat::from_rotation_z(self.heading) * glam::Quat::from_rotation_x(self.pitch) * glam::Quat::from_rotation_y(self.roll)
    }

    /// The three rays 0086b230 casts along the nose, each from and to: from the body's
    /// place and from the two local points `SIDE_RAYS` taken through the body's frame.
    pub fn wall_rays(&self) -> [(glam::Vec3, glam::Vec3); 3] {
        let reach = self.forward() * WALL_RAY;
        let rot = self.rotation();
        [self.pos, self.pos + rot * SIDE_RAYS[0], self.pos + rot * SIDE_RAYS[1]].map(|from| (from, from + reach))
    }

    /// [game] 0086e020, run after the steering: a wall within 10 m on a ray takes 95% of
    /// the velocity into it each update and pushes the body along itself; two walls that
    /// meet push it up. Checked in disassembly (0086e99b-0086ec12): the strength is
    /// (1 - 0.1 d) * 100 * (1 - c), but never less than 500, so 500 in practice.
    fn meet_walls(&mut self, walls: &[Option<(f32, glam::Vec3)>; 3], stick_y: f32, dt: f32) {
        let near = walls.map(|hit| hit.filter(|(d, n)| *d > 0.0 && *d < 10.0 && n.z.abs() < 0.1));
        self.near_wall = near.iter().any(Option::is_some);
        if !self.near_wall {
            self.wall_push = glam::Vec3::ZERO;
            return;
        }
        let corner = [(0, 1), (0, 2), (1, 2)].iter().any(|&(a, b)| matches!((near[a], near[b]), (Some((_, m)), Some((_, n))) if m.dot(n) <= 0.1));
        let ease = |push: glam::Vec3, target: glam::Vec3| {
            glam::Vec3::new(lag(push.x, target.x, 3.0, dt), lag(push.y, target.y, 3.0, dt), lag(push.z, target.z, 3.0, dt))
        };
        if corner {
            // [game 0086e3c0-0086e4e9] the target is (0, 0, s * 500 * dt), x and y to 0, each eased
            // at 3 a second (lag). The sign s comes from the stick's y (`stick_y`, the pad's
            // pair 00855910 = slot +0x30, raw, no dead zone): below -0.25 it is +1 (up); above
            // 0.25 it is -1 (down) (0086e3d8-0086e402). The option at 0x1f4f (0075e520, default 0)
            // would negate it (0086e415-0086e41c); not modelled, it is off. Between -0.25 and
            // 0.25 the game writes no sign: the slot `local_98` is then the packed flag word of
            // 0086e020 (its bytes 1..3 carry flags), a float near zero (|v| under about 1e-37),
            // so the target is 0 there. [game 0086e020 decompile l.142-196, third pass]
            let sign = if stick_y > 0.25 { -1.0 } else if stick_y < -0.25 { 1.0 } else { 0.0 };
            self.wall_push = ease(self.wall_push, glam::Vec3::new(0.0, 0.0, sign * 500.0 * dt));
            self.vel += self.wall_push;
            self.vel.x *= 0.95;
            self.vel.y *= 0.95;
            return;
        }
        // The nearest wall. [game] two at the same distance give none.
        let Some((d, n)) = near.iter().flatten().copied().min_by(|a, b| a.0.total_cmp(&b.0)) else { return };
        // The ray's direction laid along the wall. [game 0086e020, decompile l.262-285] The game
        // builds R - A, where R is the ray's start A mirrored through the wall's plane (R = A -
        // 2 * tangent(A - B), B the hit point), so R - A = 2 d * tangent(dir), with dir the ray's
        // direction (the nose, for all three rays): the nose projected into the wall, normalised
        // past 1e-8. That is `along` below. (The raw read of 0086e9e3 that suggested H - V was
        // the block for the mirror's x, not a second vector; settled 2026-10-20, second pass.)
        let nose = self.forward();
        let along = nose - n * nose.dot(n);
        let u = if along.length_squared() > 1e-8 { along.normalize() } else { along };
        let c = u.dot(n);
        if c.abs() >= 1.0 {
            return;
        }
        let strength = ((1.0 - 0.1 * d) * 100.0 * (1.0 - c)).max(500.0);
        self.wall_push = ease(self.wall_push, u * strength * dt);
        self.vel += self.wall_push;
        self.vel -= n * (0.95 * self.vel.dot(n));
    }

    /// The two timed abilities of 00864320 step 5, and whether the turbo runs this update.
    /// [game] The timers run out as 00733180 (turbo) and 007324d0 (brake) do: the time
    /// left, then the whole cooldown, then ready. A press of the turbo starts it when ready
    /// (00732e60 state 1, 007339a0); otherwise a press of the brake starts it when ready
    /// (00732c20) and latches the speed it has (mode+0x4ac = |v|). Both buttons are gated by
    /// mode+0x4a0, which flymode_init sets to 1 and only a mission's control-state map
    /// changes (008013e0): always open here.
    /// [game 0071dc20 l.146, 0071eb50] The order is the game's: the presses are read first, in
    /// the mode's update (00864320 step 5, which starts the timers and returns the turbo flag),
    /// and only then does character_update walk the ability list, calling each entry's slot 2
    /// (the brake's is 007324d0, the timed ability update). So the tick is NOT here: it runs
    /// after the whole mode update in `tick_timers`, called by `step_in_bounded`. A press's own
    /// update therefore ticks once (the turbo reads 2.5 - dt after it), and the turbo flag is
    /// taken before that tick. The walker is reached through a vtable and its caller is not
    /// named (OPEN, the place in the character update is from l.146 of 0071dc20).
    /// [game 0079d060, data global_tuning.txt Upgrades] 00732e60 multiplies the times by the
    /// single-player upgrades f3bd1a64 (turbo duration) / 95118079 (cooldown) through
    /// upgrade_table_entry 0079d060, which returns 1.0 for level 0 (its `jle` path, and the
    /// no-entry path, return the constant 1.0 at 00b38834). The tables list levels 1 to 3 only
    /// (f3bd1a64: 1.25 / 1.5 / 2; 95118079: 0.85 / 0.65 / 0.5). The level is the save's: the
    /// port takes the car's selection (`self.upgrade`, `--turbo-upgrade`, 3 by default as the
    /// user's captures show, the 2026-10-07 car and the 2026-10-21 Starscream).
    fn abilities(&mut self, input: &FlyInput, tun: &FlyTuning) -> bool {
        let brake_pressed = input.brake && !self.brake_was_down;
        self.brake_was_down = input.brake;
        if input.turbo && self.turbo_left <= 0.0 && self.turbo_cooldown <= 0.0 {
            self.turbo_left = tun.max_turbo_time * self.upgrade.map_or(1.0, |u| u.0);
        } else if brake_pressed && self.brake_left <= 0.0 && self.brake_cooldown <= 0.0 {
            self.brake_left = tun.brake_duration;
            self.speed_latch = self.vel.length();
        }
        self.turbo_left > 0.0
    }

    /// One update with only a floor under it, when there is one.
    pub fn step(&mut self, input: &FlyInput, tun: &FlyTuning, ground: Option<f32>, dt: f32) {
        self.step_in(input, tun, &Surroundings { ground, ..Default::default() }, dt);
    }

    /// One update, with no flight boundary.
    pub fn step_in(&mut self, input: &FlyInput, tun: &FlyTuning, around: &Surroundings, dt: f32) {
        self.step_in_bounded(input, tun, around, None, dt);
    }

    /// The boundary's part of an update, as the game runs it before FlyMode's update
    /// (007e7830 then 0086ab90, docs/flight-boundary.md 12.3 A1-A5): the position is tested
    /// against the boundary and the contact state is set. Returns true when outside.
    /// P is `boundary_point` when the caller sets it (`flycam::flyer_point`, which is 0071a460:
    /// root x, y and min(root z, holder z - half * upZ)), else `pos`. The game's P is that point
    /// (007e7830 l.7 writes it from 0071a2e0; 0071a460 read 2026-10-20). `sim::step_fly` sets it on
    /// every update for the player's flyer, so `pos` is only the fallback of a caller that does not
    /// set it (the unit tests here).
    /// [game 007e7830 l.27-30, second pass] the push is the local normal rotated into the world by
    /// the boundary's parent matrix (00550930, row-vector convention), the same as
    /// `push_back_direction(a.inverse().linear(), n)` when A^-1 is that matrix (see flybound
    /// `read`). [game 007e75c0] A^-1 is the owner's node matrix: the activate writes A = inverse(M)
    /// (obj+0x64..0x90), so the push frame is M, as 00550930 gives it.
    fn boundary_contact(&mut self, boundary: &Boundary, tun: &FlyTuning) -> bool {
        let point = self.boundary_point.unwrap_or(self.pos);
        let test = boundary.test(point);
        let outside = !test.inside;
        let basis = boundary.a.inverse().linear();
        let push = flybound::push_back_direction(basis, test.normal);
        // [game A3] N points from the body to the boundary's centre: the local origin of
        // the box, taken back to the world through A^-1 and B^-1.
        let centre = boundary.a.inverse().apply(boundary.b.inverse().apply(glam::Vec3::ZERO));
        let normal = flybound::toward_centre(centre, point);
        // The lateral axis is the body's right axis. [game 0086ab90 else branch, disasm 0086ac20-
        // 0086ad78, third pass] its lateral dot is V . (body row 1 rotated by Rz(-pi/2)) = V . right,
        // which is what `push.dot(lateral)` gives. The earlier pairing guess is settled.
        let lateral = self.rotation() * glam::Vec3::X;
        // [game 0086ab90 step 4, docs/flight-boundary.md 12.3 A4] X = *(mode+0x18)+0x30, which is
        // quickTurnSpeed (tun+0x30, flight-attributes.md), not turnSpeed (tun+0x28).
        self.steer.set_contact(push, normal, outside, tun.quick_turn_speed, push.dot(lateral));
        self.inside_boundary = !outside;
        outside
    }

    /// One update with an optional flight boundary (`None` is `step_in`, exactly). The mode's
    /// update runs first and then the ability list ticks the timers (`tick_timers`), as the
    /// game's character update does (0071dc20 l.146, see `abilities`).
    pub fn step_in_bounded(&mut self, input: &FlyInput, tun: &FlyTuning, around: &Surroundings, boundary: Option<&Boundary>, dt: f32) {
        self.step_mode(input, tun, around, boundary, dt);
        self.tick_timers(tun, dt);
    }

    /// The ability list's walk over the turbo and the brake (`FlyTimers::tick`), after the mode's update.
    fn tick_timers(&mut self, tun: &FlyTuning, dt: f32) {
        let mut timers = self.timers();
        timers.tick(tun, true, dt);
        self.set_timers(timers);
    }

    /// The mode's update (00864320) proper: the presses, the boundary, the steering and the cap.
    /// [game] 00864320's branch on `mode+0x55`: outside, the stick steering and the probe
    /// contact are skipped and the bounce 00868b00 runs (`uses_bounce`); a helicopter or a
    /// robot outside gets the wall stop 00868fb0 and then the helicopter steering (its inverted
    /// branch, outside through the top, passes lift P = +1: the helicopter is pushed down).
    fn step_mode(&mut self, input: &FlyInput, tun: &FlyTuning, around: &Surroundings, boundary: Option<&Boundary>, dt: f32) {
        let ground = around.ground;
        self.rotor_lift = None;
        // 0086b230 reads each ray's answer before it casts the next: what this update sees
        // was cast at the start of the last one. [game]
        let walls = std::mem::replace(&mut self.walls_cast, around.walls);
        self.avatar_blend = (self.avatar_blend - dt).max(0.0);
        self.cap_scale = input.cap_scale.unwrap_or(1.0);
        let turbo = self.abilities(input, tun);

        // The boundary test comes before the update reads `mode+0x55` (see `boundary_contact`).
        let outside = boundary.is_some_and(|b| self.boundary_contact(b, tun));

        // [game] 00864320: thrust along the body's forward row, the turbo's while it runs.
        // The helicopter has none of it (0086461d): its own steering pushes it.
        let nose = self.forward();
        if !tun.helicopter {
            self.vel += nose * if turbo { tun.turbo_thrust } else { tun.thrust } * dt;
        }

        // [game] 00864320 builds the stick pair: dead zone, then squared with the sign kept.
        let shape = |a: f32| {
            let a = dead_zone(a, tun.l_stick_deadzone);
            a.abs() * a
        };
        let (lateral, mut pitch_in) = (shape(input.stick.x), shape(input.stick.y));

        // [game] 00864320 l.157-162, outside the boundary and not running a stunt (the stunt
        // check returns first, l.8-10). A jet or the car-form flyer is bounced; a helicopter or
        // a robot gets the wall stop 00868fb0 (below).
        if outside && self.stunt == Stunt::None {
            if flybound::uses_bounce(tun.helicopter, tun.robot) {
                // [game 00868b00] takes the stick vector as its steer. Its velocity change is
                // guarded by the BODY's state (+0x38 == 0 and +0x34 == 0.0, 00868c67-00868c7d),
                // not by a tangent test. Settled 2026-10-20 (lead): those are the wrapper's control z
                // and turn x, which holder_control_set 006e8da0 writes every update (W+0x34 = control.z,
                // W+0x38 = turn.x; the earlier "writes neither" was wrong), so the guard is
                // `body_takes_velocity` [game 00556b40, 006e8da0]. PITCH (mode+0x1c) and ROLL (mode+0x20) are this Fly's own
                // fields, so the bounce ramps from their current values and writes them back
                // (notes 12.9).
                self.steer.pitch = self.pitch;
                self.steer.roll = self.roll;
                let bounced = self.steer.bounce(self.vel, true, glam::Vec3::new(input.stick.x, input.stick.y, 0.0), dt, tun.max_turn_speed);
                if self.body_takes_velocity() {
                    self.vel = bounced;
                }
                self.pitch = self.steer.pitch; // [game 00868b00] mode+0x1c, PITCH
                self.roll = self.steer.roll; // [game 00868b00] mode+0x20, ROLL
                // [game 00868b00] the bounce's rate is the body's yaw rate (mode+0x44, radians a
                // second), and the game integrates it into the heading itself (mode+0x24 -= dt *
                // mode+0x44, then wrapped); this line is that integral, done once.
                self.yaw_rate = self.steer.rate;
                self.heading = wrap(self.heading - self.yaw_rate * dt);
            } else {
                // [game 00864320 l.157-162] a helicopter or a robot outside: the wall stop
                // 00868fb0 (docs/flight-boundary.md 12.7), then the helicopter steering 00867f20
                // with its output as the pair. The body's first world row is R*X (006de7e0 out[0..2],
                // the right axis: [game] 00868fb0 w = (-r1, r0, 0)). The holder's inverse is the
                // stored copy 005e3820 returns (+0x138..+0x164); the port uses R^T of the same
                // rotation. NARROWED 2026-10-20: the writer is 006dc3b0 (holder_commit_affine, +0x138 =
                // its 12-float argument), called from the pose pass 006df7c0 with 0052ff10's inverse of
                // the advanced pose (the same +0x38 / +0x34 guard), and from hit_consumption_loop
                // 006e2c10 (not read). 0052ff10 is the general affine inverse (adjugate / det), so for
                // an orthonormal rotation its 3x3 is R^T [game 0052ff10, 006dc3b0]: the port's value
                // is that inverse. [assumed] the flyer's holder takes the 006df7c0 branch (+0x1d4 bit
                // 0x4000), which is not read; OPEN, settles: that bit's setter for a flying holder, and
                // 006e2c10's write of +0x138 for the same holder.
                // The boost is off outside: 00867f20 takes it as the turbo being active AND
                // mode+0x55 == 0 (00867f77-00867fb0, `docs/flight-helicopter.md` B), so turbo =
                // false in the steering; the cap in `finish` still uses this update's turbo
                // (00869df0 is passed the plain `timed_ability_active`, 00864576). [game]
                let rot = self.rotation();
                // The owner's input point's first float is the turn `a` of the inverted branch
                // (raw `look.x`, no dead zone: 0086905b; the slot +0x28 is the camera stick,
                // 0084fc60, see `FlyInput::look`). [game]
                let ws = flybound::wall_stop(self.steer.normal, self.steer.push.z, rot * glam::Vec3::X, glam::Mat3::from_quat(rot.inverse()), self.vel, input.look.x, dt);
                self.vel = ws.velocity;
                // [game 00868ff4, 00869030-00869041] b is +1, and -1 when the invert-pitch option
                // is set. steer_helicopter flips P on that option itself (its step I, the port's
                // `invert_pitch` line), so the pre-flip value is -1 here: the net P is +1 in every case.
                let lift = if input.invert_pitch { -ws.lift } else { ws.lift };
                self.steer_helicopter(input, tun, ws.out.x, ws.out.y, ws.a, lift, false, ground, dt);
            }
            self.finish(tun, turbo, ground, dt);
            return;
        }

        if tun.helicopter {
            let turn = dead_zone(input.look.x, tun.l_stick_deadzone);
            let lift = dead_zone(input.look.y, tun.l_stick_deadzone);
            // Its steering runs only while the control vector is (0, 0, 0), as 00866ef0's.
            // [game 00867f20, asm 867f00-867f5b]
            if self.control == glam::Vec3::ZERO {
                self.steer_helicopter(input, tun, lateral, pitch_in, turn, lift, turbo, ground, dt);
            }
            self.finish(tun, turbo, ground, dt);
            return;
        }

        // [game] 00866a70: while a stunt runs the stick steers nothing (the control
        // vector is (1, 1, 1)). A roll's own roll angle dies away by 10 a second and its
        // first update puts the velocity along the nose at the speed cap; the loops change
        // nothing in code (`noteslight-stunts.md` 3: their clips show them).
        // The cap is 0086a840's, maxSpeed times the status scale (00866bb2-00866c13). [game]
        // Also while a clip's control vector is set (00866ef0 runs its body only for (0, 0, 0)):
        // the stick steers nothing. [game 00866900]
        if self.stunt != Stunt::None || self.control != glam::Vec3::ZERO {
            if matches!(self.stunt, Stunt::Left | Stunt::Right) {
                self.roll = if 10.0 * dt >= 1.0 { 0.0 } else { self.roll - self.roll * dt * 10.0 };
                if std::mem::take(&mut self.stunt_fresh) {
                    self.vel = self.forward() * tun.max_speed * self.cap_scale;
                }
            }
            self.finish(tun, turbo, ground, dt);
            return;
        }
        // [game] 0086a590, the trigger: armed, the stick past `stuntMoveStickThresholdX` to
        // a side rolls that way; past `stuntMoveStickThresholdY` it loops, up with the
        // stick negative, down with it positive and more than 10 over the ground
        // (mode+0x78). The stick is 00855910's: the input source's slot +0x30 (0084fcd0),
        // the very pair 00864320 steers by, i.e. `input.stick` before its dead zone. [game]
        // (The control vector is zero here: the block above returns otherwise, as 0086a590
        // needs.) [game]
        if input.stunt && tun.can_do_stunts {
            let a = input.stick;
            let (x, y) = (tun.stunt_move_stick_threshold_x, tun.stunt_move_stick_threshold_y);
            self.stunt = if a.y.abs() < a.x.abs() {
                if a.x < -x { Stunt::Left } else if a.x > x { Stunt::Right } else { Stunt::None }
            } else if a.y < -y {
                Stunt::Up
            } else if a.y > y && ground.is_none_or(|g| self.pos.z - g > 10.0) {
                Stunt::Down
            } else {
                Stunt::None
            };
            self.stunt_fresh = matches!(self.stunt, Stunt::Left | Stunt::Right);
        }

        // [game] 00866ef0 step 1: the velocity across the nose loses 95% each update.
        let across = nose.cross(self.vel).cross(nose);
        let length = across.length();
        let n = if length > 1e-8 { across / length } else { across };
        self.vel += n * (-0.95 * n.dot(self.vel));

        // [game] 00864320 (00864955-00864989): with the hold button up, an AI controller steers
        // by 008693f0 instead of 00866ef0. Its step 1 is the damping just done; the hold button
        // takes 008676c0 whoever holds it.
        if let Some(ai) = input.ai.filter(|_| !input.hold) {
            self.holding = false;
            self.steer_ai(&ai, lateral, tun, ground, dt);
            self.meet_walls(&walls, input.stick.y, dt);
            self.finish(tun, turbo, ground, dt);
            return;
        }

        // Steps 2 and 5: the pitch input.
        if input.invert_pitch {
            pitch_in = -pitch_in;
        }
        if pitch_in >= 0.0 {
            pitch_in *= self.ramp;
        }

        // Step 6: close over the ground it is pushed up. [game] 00866ef0 l.73-98: S' from
        // `height_over` (the body's extent taken off by the sines, see there), thr 1.85 or 2.85
        // with `three_metre_latch` (asm 008673c2-008673e6), k = (1 - S'/thr) * 50 for S' < thr,
        // vz += 3 k dt. [game 00556b40, body_add_velocity] the game hands the vector there, and that
        // adds it only when the body's +0x38 byte is 0 and its +0x34 float is 0.0: `body_takes_velocity`
        // (settled and ported 2026-10-20; before, the push always went in).
        if let Some(g) = ground {
            let over = self.height_over(g, tun);
            let near = if self.three_metre_latch { 2.85 } else { 1.85 };
            if over < near && self.body_takes_velocity() {
                self.vel.z += 3.0 * (1.0 - over / near) * 50.0 * dt;
            }
        }

        // Step 7: pitch, toward -maxPitch * input at pitchLagAutoLevel, kept inside
        // maxPitch and inside 0.49 pi.
        let limit = tun.max_pitch * DEG;
        let pitch = wrap(lag(self.pitch, -limit * pitch_in, tun.pitch_lag_auto_level, dt));
        self.pitch = pitch.clamp(-limit, limit).clamp(-0.49 * std::f32::consts::PI, 0.49 * std::f32::consts::PI);
        // [game] 00864320: the hold button picks the steering; its first update clears the
        // latch.
        if input.hold && !self.holding {
            self.quick_roll = 0.0;
        }
        self.holding = input.hold;
        let turn_speed = if input.hold {
            // [game] 008676c0: the roll is latched to the side the stick first leans to, at
            // maxRollQuick, and goes there at quickRollLag; the turn's target is
            // quickTurnSpeed. The rest is 00866ef0's.
            if self.quick_roll == 0.0 && lateral != 0.0 {
                self.quick_roll = lateral.signum() * tun.max_roll_quick * DEG;
            }
            self.roll = lag(self.roll, self.quick_roll, tun.quick_roll_lag, dt);
            tun.quick_turn_speed
        } else {
            // Step 8: roll, toward maxRoll * input at rollStrength.
            self.roll = lag(self.roll, tun.max_roll * DEG * lateral, tun.roll_strength, dt);
            tun.turn_speed
        };
        // Step 9: the yaw rate, toward turnSpeed * input at turnLag, or back to none at
        // turnRestoreLag; an input against the turn stops it at once.
        let rate = if lateral != 0.0 { tun.turn_lag } else { tun.turn_restore_lag };
        if self.yaw_rate * lateral < 0.0 {
            self.yaw_rate = 0.0;
        }
        self.yaw_rate = lag(self.yaw_rate, turn_speed * lateral, rate, dt);
        // Step 11: the heading.
        self.heading = wrap(self.heading - self.yaw_rate * dt);

        self.meet_walls(&walls, input.stick.y, dt);
        self.finish(tun, turbo, ground, dt);
    }

    /// [game] 008693f0, the AI's steering in normal flight (read in disassembly
    /// 008697a0-008699c9; its damping step is the caller's, shared with 00866ef0 step 1).
    /// The body turns toward the target at a capped rate and the angles are read back off the
    /// turned frame; then the roll goes toward maxRoll * the AI's stick at rollStrength, as the
    /// player's step 8. The yaw rate (mode+0x44) is neither written nor used: nothing here
    /// integrates the heading from it.
    fn steer_ai(&mut self, ai: &AiSteer, lateral: f32, tun: &FlyTuning, ground: Option<f32>, dt: f32) {
        let mut to = ai.target - self.pos;
        if to.length() > 0.01 {
            // [game] on the ground (the latch mode+0x74, `on_ground_latch`) it does not aim down.
            // The game's second test (normal z over 0.1, mode+0x90) always passes: the latch
            // already needs a normal z over 0.5.
            if self.on_ground_latch(ground, tun) && to.z < 0.0 {
                to.z = 0.0;
            }
            // [game] pi/4 a second, 3 pi/4 with an obstacle ahead (00b3990c, 00b39910).
            let rate = if ai.obstacle_ahead { 3.0 * std::f32::consts::FRAC_PI_4 } else { std::f32::consts::FRAC_PI_4 };
            let frame = glam::Mat3::from_quat(self.rotation());
            let turned = turn_toward(frame, look_at(frame, to), rate * dt);
            (self.pitch, self.roll, self.heading) = angles_of(turned);
        }
        // [game] 00869927-0086997e: tun+0x60 maxRoll, tun+0x6c rollStrength, the stick's first slot.
        self.roll = lag(self.roll, tun.max_roll * DEG * lateral, tun.roll_strength, dt);
    }

    /// [game] 00867f20, the steering of a form whose data says `helicopter` (the
    /// helicopter, and the two robots that fly as themselves), as numbered in
    /// `docs/flight-helicopter.md`: one stick pushes it along and across its nose and
    /// leans it, the other turns it and lifts it; nothing pushes it when the sticks rest.
    /// `q0` and `q1` are the squared stick, across and along (the game's `Vc`, slots 0 and 1).
    /// `turn` and `lift` are the linear pair (the game's EAX stick, `L` and `P`), dead-zoned by
    /// the caller. [game 00868fb0 step 6] the wall stop passes `(out.x, out.y)` as `q0, q1`, the
    /// scalar `a` as `turn`, and `lift` = `b`: 0 on the normal branch, +1 (or -1 when the invert
    /// option is set, flipped back here) on the inverted branch. [game 00868fb0 12.7, 12.11]
    /// NOT here: the halving of the lean while mode+0x4c1 is set (0086dc00, an approach
    /// drag), the heading stop on contact (008699d0), the value at mode+0x490.
    #[allow(clippy::too_many_arguments)]
    fn steer_helicopter(&mut self, input: &FlyInput, tun: &FlyTuning, mut q0: f32, mut q1: f32, mut turn: f32, mut lift: f32, turbo: bool, ground: Option<f32>, dt: f32) {
        // B: in the turbo it is pushed forward whatever the stick says, the rest weakened.
        if turbo {
            if q1 >= 0.0 {
                q1 = 1.0;
            }
            q0 *= 0.85;
            turn *= 0.85;
            lift *= 0.85;
        }
        if input.invert_pitch {
            lift = -lift;
        }
        // LT: on the ground it goes no lower. [game] the latch mode+0x74 (`on_ground_latch`), set
        // by 0086b230 when the body's height S' is under 2 m (3 m with `three_metre_latch`) on
        // ground whose normal is steep enough; the caller passes that normal (`ground_normal_z`).
        if self.on_ground_latch(ground, tun) && lift >= 0.0 {
            lift = 0.0;
            self.vel.z = 0.0;
        }
        // D1, D2: it slows when the sticks rest.
        if !turbo && q0.hypot(q1) < 0.01 {
            self.vel.x *= 0.95;
            self.vel.y *= 0.95;
        }
        if lift.abs() < 0.01 {
            self.vel.z *= 0.95;
        }
        // H: the hold button stops it and turns it with the first stick.
        let hold = input.hold && !turbo && !tun.robot;
        if hold {
            self.vel *= 0.95;
            turn = 0.5 * q0;
            q0 = 0.0;
        } else {
            // N1 to N4: across the nose, along it (both level), and up or down.
            let nose = self.forward();
            self.vel += glam::Vec3::new(nose.y, -nose.x, 0.0) * (tun.lateral_thrust * q0 * dt);
            let along = glam::Vec3::new(nose.x, nose.y, 0.0);
            let mut push = if turbo { tun.turbo_thrust } else { tun.thrust } * q1 * dt;
            // [game] 00868536-00868586, read 2026-10-18: D = (the body velocity, normalised when
            // longer than 1e-8: [esp+0x38], the buffer 00557000 writes the body from) . W, and
            // `comiss 0, D; jbe` takes the plain push when D >= 0. So the factor applies when
            // the body moves AGAINST the push: it helps the helicopter turn round in a turbo,
            // as its name says. (It was taken the other way round before.)
            if turbo && flybound::unit_or_kept(self.vel).dot(along) < 0.0 {
                push *= tun.helicopter_turbo_turn_factor;
            }
            self.vel += along * push;
            // [game 00867f20, N4 at 0086869b-008686b8: `Vb2 += -lift * P * dt`] P positive pushes
            // the helicopter DOWN (vel.z falls); look.y = -1 (stick up) is P < 0 and climbs.
            // The wall stop's inverted branch passes P = +1 so it descends (docs/flight-boundary.md 12.11).
            self.vel.z -= tun.lift * lift * dt;
        }
        // R: the lean to the side.
        self.roll = lag(self.roll, tun.max_roll * DEG * q0, tun.roll_strength, dt);
        // Y, H2: the turn, kept inside maxTurnSpeed, and the heading.
        let rate = if turn.abs() >= 0.05 { tun.turn_lag } else { tun.turn_restore_lag };
        self.yaw_rate = lag(self.yaw_rate, tun.turn_speed * turn, rate, dt).clamp(-tun.max_turn_speed, tun.max_turn_speed);
        self.heading = wrap(self.heading - self.yaw_rate * dt);
        if hold {
            // RB: the stick turns the nose up and down at a rate, between the limits.
            self.pitch_rate = lag(self.pitch_rate, q1 * std::f32::consts::FRAC_PI_2, 2.0, dt);
            let (low, high) = (tun.min_pitch * DEG, tun.max_pitch * DEG);
            self.pitch = (self.pitch - self.pitch_rate * dt).clamp(low, high);
            if self.pitch == low || self.pitch == high {
                self.pitch_rate = 0.0;
            }
        } else {
            // RA: the nose goes down as it is pushed forward, up as it is pulled back.
            self.pitch_rate = 0.0;
            let limit = if q1 <= 0.0 { tun.max_pitch } else { -tun.min_pitch };
            self.pitch = lag(self.pitch, -limit * DEG * q1, tun.pitch_strength, dt);
        }
        // SM: the rotor speed eases by this P (the slot N4 read, [esp+0x7c]), on the
        // character's `flightsurface::Surfaces`. [game 00868a86-00868aee]
        self.rotor_lift = Some(lift);
    }

    /// The height the low-speed rules read, S' [game]: the drop ray's distance S (00866ef0 reads
    /// it as mode+0x78; 0086b230 stores it there raw, asm 0086c469 and 0086c64a), less the body's
    /// extent by the sines of pitch and roll (00867364-00867394, 0086c4a4-0086c504):
    /// `S' = S - 2 e[-0x10] |sin roll| - 2 e[-0xc] |sin pitch|`. S is the distance of a ray cast
    /// straight down from `pos + 2 * nose` (0086b230 l.343-351), so its origin is `2 sin pitch`
    /// above the place: S = pos.z + 2 sin pitch - ground. Level, S' is `pos.z - ground`.
    /// `ground` is the z of that ray's hit (`sim::step_fly` casts it) [game 0086b230].
    pub fn height_over(&self, ground: f32, tun: &FlyTuning) -> f32 {
        let drop = self.pos.z + 2.0 * self.pitch.sin() - ground;
        drop - 2.0 * (tun.extent_roll * self.roll.sin().abs() + tun.extent_pitch * self.pitch.sin().abs())
    }

    /// The ground latch mode+0x74 [game] (0086b230 l.245-292, asm 0086c514-0086c562): set when
    /// S' is under 2 m (3 m with `three_metre_latch`; S' equal to the threshold is not under it,
    /// the `jbe`) and the ground's normal z is over 0.5 in magnitude. No ground, no latch.
    /// The no-hit branch of 0086b230 (asm 0086c627-0086c65e) tests the raw S with no slope; a
    /// ray that found no ground gives `None` here, so that branch has nothing to test.
    pub fn on_ground_latch(&self, ground: Option<f32>, tun: &FlyTuning) -> bool {
        let Some(g) = ground else { return false };
        let thr = if self.three_metre_latch { 3.0 } else { 2.0 };
        self.height_over(g, tun) < thr && self.ground_normal_z.abs() > 0.5
    }

    /// The end of every update: the speed's cap, the move, the ramp.
    fn finish(&mut self, tun: &FlyTuning, turbo: bool, ground: Option<f32>, dt: f32) {
        self.cap_speed(tun, turbo, dt);
        self.advance(ground, dt);
    }

    /// [game] 00869df0, as the player's flight calls it from 00864320 (00864b0b: its second
    /// argument is the turbo's `timed_ability_active`, its third the brake's, stored at
    /// 00864576 / 0086457f). Branches checked in disassembly:
    /// - turbo (00869f60-00869ff2): the speed is cut to maxTurboSpeed x s (0086a950);
    /// - brake (0086a04a-0086a10c): the latch drops by brakingSpeed (tun+0xd8) x dt, never
    ///   under maxSpeed x s x brakeSpeedScale (tun+0xd0, 0086a094 `mulss` with the cap), and
    ///   the speed is cut to it;
    /// - neither, with the turbo's ended flag (0086a3f9-0086a4f1): beside a wall (mode+0x4b4)
    ///   the flag is dropped and nothing is cut this update; otherwise the latch starts at the
    ///   turbo cap, drops by postTurboDeceleration (tun+0xd4) x dt, and once the normal cap
    ///   is not below it (0086a471 `jb`) it is the normal cap and the flag and latch end. The
    ///   speed is SET to the latch, not only cut;
    /// - otherwise (0086a4ff-0086a563): the speed is cut to maxSpeed x s (0086a840).
    ///
    /// s is `FlyInput::cap_scale`. Not here: the AI's approach drags (mode+0x4c1 / 0x4c2,
    /// switched on only by 0086dc00 for an AI target).
    fn cap_speed(&mut self, tun: &FlyTuning, turbo: bool, dt: f32) {
        let speed = self.vel.length();
        let unit = if speed > 1e-8 { self.vel / speed } else { self.vel };
        let normal = tun.max_speed * self.cap_scale;
        if turbo {
            let cap = tun.max_turbo_speed * self.cap_scale;
            if speed > cap {
                self.vel = unit * cap;
            }
        } else if self.brake_left > 0.0 {
            self.speed_latch -= tun.braking_speed * dt;
            let floor = normal * tun.brake_speed_scale;
            if floor > self.speed_latch {
                self.speed_latch = floor;
            }
            if speed > self.speed_latch {
                self.vel = unit * self.speed_latch;
            }
        } else if self.turbo_ended {
            if self.near_wall {
                self.turbo_ended = false;
                return;
            }
            if !self.post_turbo {
                self.speed_latch = tun.max_turbo_speed * self.cap_scale;
                self.post_turbo = true;
            }
            self.speed_latch -= tun.post_turbo_deceleration * dt;
            if normal >= self.speed_latch {
                self.speed_latch = normal;
                self.post_turbo = false;
                self.turbo_ended = false;
            }
            self.vel = unit * self.speed_latch;
        } else if speed > normal {
            self.vel = unit * normal;
        }
    }

    fn advance(&mut self, ground: Option<f32>, dt: f32) {

        // [game] no gravity in flight. The body step adds the body's gravity force (+0x18)
        // with no flag test (006e96c0), so it is the force itself that is zeroed: FlyMode's
        // enter calls 0071eb80 unconditionally (008658b3), which writes +0x18..+0x20 =
        // mass (+0x24) x (0, 0, 0); the exit calls 0071eca0 (00866185), which puts back
        // mass x (0, 0, -g) with g the global at 00d40b00. HoverMode does the same (enter
        // 00871514, exit 00871657); DashMode's exit restores it too (008589d4). The writers
        // were found by the `+0x28 == 0` guard they share (decomp grep: the jump mode, the
        // grenades, the creation 006e7b60, these two); none runs during flight.
        // [stand-in] the world: here only the floor stops it; `sim.rs` step_fly then pushes the flyer's
        // cylinder out of what it is in (its own stand-in, the same as the one in `step_fly`). OPEN:
        // settles: the game's body contact with level geometry for a flyer (the physics solver,
        // `docs/physics.md`, not ported here).
        self.pos += self.vel * dt;
        if let Some(g) = ground.filter(|&g| self.pos.z < g) {
            self.pos.z = g;
            self.vel.z = self.vel.z.max(0.0);
        }

        // [game] 00864320: the ramp.
        self.ramp = (self.ramp + 2.0 * dt).min(1.0);
    }
}

#[cfg(test)]
mod step_tests {
    use super::*;
    use glam::{Vec2, Vec3};

    const DT: f32 = 0.032;

    /// Starscream's numbers [data], so the tests do not need the install.
    fn starscream() -> FlyTuning {
        FlyTuning {
            thrust: 50.0, turbo_thrust: 75.0, max_speed: 45.0, max_turbo_speed: 65.0, max_turbo_time: 2.5,
            max_turbo_cooldown_time: 5.5, turn_speed: 2.8, turn_lag: 1.5, turn_restore_lag: 4.0, pitch_lag_auto_level: 1.5,
            max_pitch: 65.0, min_pitch: -65.0, max_roll: 70.0, roll_strength: 2.5, post_turbo_deceleration: 20.0,
            max_roll_quick: 90.0, quick_turn_speed: 3.8, quick_roll_lag: 4.0,
            can_do_stunts: true, stunt_move_button_press_time: 0.2, stunt_move_stick_threshold_x: 0.5, stunt_move_stick_threshold_y: 0.9,
            ..Default::default()
        }
    }

    fn fly(input: FlyInput, updates: usize) -> Fly {
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 0.0);
        for _ in 0..updates {
            f.step(&input, &starscream(), Some(0.0), DT);
        }
        f
    }

    /// A clip's traction in flight (00866900): a set vector holds the stick off; the clip's
    /// all-zero last key ends the stunt, the loop's angles cleared, the jet sent along its nose
    /// at the normal cap. [game]
    #[test]
    fn a_zero_traction_ends_the_stunt_along_the_nose() {
        let tun = starscream();
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::new(0.0, 45.0, 0.0), 0.0);
        f.traction(Vec3::ONE, &tun);
        (f.stunt, f.pitch, f.roll, f.heading) = (Stunt::Up, 0.6, 0.2, 0.3);
        // While set, the stick steers nothing: the heading stays.
        let input = FlyInput { stick: Vec2::new(1.0, 0.0), ..Default::default() };
        f.step(&input, &tun, Some(0.0), DT);
        assert_eq!(f.heading, 0.3);
        let nose = f.forward();
        f.traction(Vec3::ZERO, &tun);
        assert_eq!(f.stunt, Stunt::None);
        assert_eq!((f.pitch, f.roll), (0.0, 0.0));
        assert!((f.heading - (-nose.x).atan2(nose.y)).abs() < 1e-6);
        assert!((f.vel - nose * tun.max_speed * f.cap_scale).length() < 1e-4);
        // A roll keeps its roll; a helicopter keeps its velocity.
        let heli = FlyTuning { helicopter: true, ..starscream() };
        let mut h = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::new(3.0, 0.0, 0.0), 0.0);
        (h.stunt, h.roll) = (Stunt::Left, 0.4);
        h.traction(Vec3::ZERO, &heli);
        assert_eq!((h.stunt, h.roll, h.vel), (Stunt::None, 0.4, Vec3::new(3.0, 0.0, 0.0)));
    }

    /// A box boundary, half size 500 m across and 1000 m up, from the us_test block shape
    /// [data] `[500,0,0 | 0,500,0 | 0,0,70 | 0,0,0]`; the height is changed so the flyer at
    /// z = 100 is under the ceiling. A = identity (test only).
    fn box_500() -> Boundary {
        Boundary::read(&[500.0, 0.0, 0.0, 0.0, 500.0, 0.0, 0.0, 0.0, 1000.0, 0.0, 0.0, 0.0], crate::flybound::Affine::IDENTITY, false)
    }

    #[test]
    fn a_jet_flying_out_of_a_box_is_turned_back_inside() {
        let b = box_500();
        let tun = starscream();
        let ground = Surroundings { ground: Some(0.0), ..Default::default() };
        // Just past the +x face (x = 500 is the face), nose on +x: heading -pi/2 gives forward (1,0,0).
        let mut f = Fly::enter(Vec3::new(520.0, 0.0, 100.0), Vec3::new(45.0, 0.0, 0.0), -std::f32::consts::FRAC_PI_2);
        let mut went_out = false;
        let mut back_at = None;
        for i in 1..=(12.0 / DT) as usize {
            f.step_in_bounded(&FlyInput::default(), &tun, &ground, Some(&b), DT);
            went_out |= !f.inside_boundary;
            if went_out && f.inside_boundary {
                back_at = Some(i as f32 * DT);
                break;
            }
        }
        assert!(went_out, "the flyer starts outside, so the contact must be set");
        let t = back_at.expect("the flyer should be back inside within 12 s");
        // The bound is loose on purpose; the measured time is printed (run with --nocapture).
        eprintln!("sideways: back inside at {t:.2} s");
        assert!(t < 12.0, "{t}");
        assert!(!b.outside(f.pos), "{:?}", f.pos);
        assert!(!f.steer.outside);
    }

    /// The helicopter tuning the helicopter test above uses.
    fn heli_tuning() -> FlyTuning {
        FlyTuning {
            helicopter: true, thrust: 30.0, lateral_thrust: 20.0, lift: 15.0, max_speed: 30.0, turn_speed: 2.0,
            max_turn_speed: 1.5, turn_lag: 4.0, turn_restore_lag: 4.0, max_pitch: 25.0, min_pitch: -25.0, max_roll: 25.0,
            roll_strength: 2.0, pitch_strength: 2.0, ..Default::default()
        }
    }

    /// [game 00867f20 SM] Only the helicopter steering hands its lift to the rotor speed.
    #[test]
    fn the_helicopter_steering_hands_its_lift_to_the_rotor() {
        let ground = Surroundings { ground: Some(0.0), ..Default::default() };
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 50.0), Vec3::ZERO, 0.0);
        f.step_in(&FlyInput::default(), &heli_tuning(), &ground, DT);
        assert_eq!(f.rotor_lift, Some(0.0));
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 50.0), Vec3::ZERO, 0.0);
        f.step_in(&FlyInput::default(), &starscream(), &ground, DT);
        assert_eq!(f.rotor_lift, None);
    }

    /// [game 00868fb0, docs/flight-boundary.md 12.7] A helicopter outside the box (not aligned
    /// with the centre) is not reflected: its velocity is multiplied by 0.95 by the wall stop and
    /// by 0.95 again by the helicopter's own damping of the sideways velocity (nothing pushes it),
    /// it turns its nose toward the centre by `a`, and once the nose is within 37 degrees of the
    /// centre (c >= 0.8) it is pushed forward and comes back inside. Run with --nocapture for the trace.
    #[test]
    fn a_helicopter_flying_out_sideways_is_slowed_turned_toward_the_centre_and_comes_back() {
        let b = box_500();
        let tun = heli_tuning();
        let ground = Surroundings { ground: Some(0.0), ..Default::default() };
        // Just past the +x face, nose along +y (heading 0), drifting out at 10 m/s.
        let mut f = Fly::enter(Vec3::new(520.0, 0.0, 100.0), Vec3::new(10.0, 0.0, 0.0), 0.0);
        f.step_in_bounded(&FlyInput::default(), &tun, &ground, Some(&b), DT);
        assert!(!f.inside_boundary, "the helicopter starts outside");
        // One update, nose not toward the centre: 0.95 (wall stop) then 0.95 (the sideways damping).
        assert!((f.vel.x - 10.0 * 0.95 * 0.95).abs() < 1e-3, "{:?}", f.vel);
        assert!(f.heading > 0.0, "the nose turns toward the centre (-x): heading {}", f.heading);
        let mut back_at = None;
        let mut max_dist = 0.0_f32;
        for i in 2..=(40.0 / DT) as usize {
            f.step_in_bounded(&FlyInput::default(), &tun, &ground, Some(&b), DT);
            max_dist = max_dist.max(f.pos.x.abs());
            if i % 30 == 0 {
                eprintln!("t={:5.2} pos=({:7.2},{:6.2}) vel=({:6.2},{:6.2}) heading={:6.1} inside={}", i as f32 * DT, f.pos.x, f.pos.y, f.vel.x, f.vel.y, f.heading / DEG, f.inside_boundary);
            }
            if f.inside_boundary {
                back_at = Some(i as f32 * DT);
                break;
            }
        }
        let t = back_at.expect("the helicopter should come back inside within 40 s");
        eprintln!("helicopter: back inside at {t:.2} s, furthest |x| {max_dist:.2}");
        assert!(t < 40.0, "{t}");
        assert!(!b.outside(f.pos), "{:?}", f.pos);
    }

    /// A helicopter inside the box flies exactly as with no boundary (the wall stop is not reached).
    #[test]
    fn a_helicopter_flying_inside_a_boundary_flies_exactly_as_with_none() {
        let b = box_500();
        let tun = heli_tuning();
        let ground = Surroundings { ground: Some(0.0), ..Default::default() };
        let input = FlyInput { stick: Vec2::new(0.4, 0.8), look: Vec2::new(0.5, -0.3), ..Default::default() };
        let mut plain = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 0.0);
        let mut bounded = plain.clone();
        for _ in 0..80 {
            plain.step_in_bounded(&input, &tun, &ground, None, DT);
            bounded.step_in_bounded(&input, &tun, &ground, Some(&b), DT);
        }
        assert_eq!(plain.pos, bounded.pos);
        assert_eq!(plain.vel, bounded.vel);
        assert_eq!(plain.heading, bounded.heading);
        assert!(bounded.inside_boundary);
    }

    /// Evidence: the unpublished notes "Third check", item 5. US_Test_FreeRoam_A,
    /// Starscream as a jet, the us_test zone's boundary block `[500,0,0 | 0,500,0 | 0,0,70 | 0,0,0]`
    /// on an owner placed at (156.043, -143.19, 68.2943). Stick -1 (climb) for 4 s, then none.
    /// The game: "flight boundary: left" by 10 s, no re-entry to 45 s, height 998 m, pitch +37.
    /// The top exit's bounce (00868b00, notes 12.9) pushes the nose 30 degrees DOWN at 2/s
    /// (PITCH, mode+0x1c) and adds 30 * dt along V, so the thrust no longer holds it up.
    /// Run with --nocapture to see the trace.
    #[test]
    fn a_jet_climbing_out_through_the_top_is_brought_back_inside() {
        use crate::flybound::invert_placement;
        let owner = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 156.043, -143.19, 68.2943];
        let block = [500.0, 0.0, 0.0, 0.0, 500.0, 0.0, 0.0, 0.0, 70.0, 0.0, 0.0, 0.0];
        let b = Boundary::read(&block, invert_placement(&owner), false);
        let tun = starscream();
        let ground = Surroundings { ground: Some(0.0), ..Default::default() };
        let mut f = Fly::enter(Vec3::new(-137.46, -366.85, 0.9), Vec3::ZERO, 0.0);
        let climb = FlyInput { stick: Vec2::new(0.0, -1.0), ..Default::default() };
        let mut left_at: Option<f32> = None;
        let mut back_at: Option<f32> = None;
        let mut max_z = f32::MIN;
        let steps = (45.0 / DT) as usize;
        for i in 1..=steps {
            let t = i as f32 * DT;
            let input = if t <= 4.0 { climb } else { FlyInput::default() };
            f.step_in_bounded(&input, &tun, &ground, Some(&b), DT);
            max_z = max_z.max(f.pos.z);
            if left_at.is_none() && !f.inside_boundary {
                left_at = Some(t);
            }
            if left_at.is_some() && back_at.is_none() && f.inside_boundary {
                back_at = Some(t);
            }
            if i % 31 == 0 {
                eprintln!(
                    "t={:5.2} z={:8.2} pitch={:6.1} roll={:6.1} inside={} V=({:.2},{:.2},{:.2}) N=({:.2},{:.2},{:.2}) rate={:.3} vz={:.1}",
                    t, f.pos.z, f.pitch / DEG, f.roll / DEG, f.inside_boundary,
                    f.steer.push.x, f.steer.push.y, f.steer.push.z,
                    f.steer.normal.x, f.steer.normal.y, f.steer.normal.z,
                    f.steer.rate, f.vel.z,
                );
            }
        }
        let left = left_at.expect("the jet should leave through the top");
        eprintln!("left at {left:.2} s, back at {back_at:?}, final z {:.2}, max z {max_z:.2}", f.pos.z);
        let back = back_at.expect("the jet should be turned back inside within 20 s");
        assert!(back - left <= 20.0, "left {left} back {back}");
        assert!(f.pos.z < 139.0, "{}", f.pos.z);
        assert!(max_z <= 400.0, "the jet must stay bounded over 45 s: max z {max_z}");
    }

    /// The us_test box of the jet test above, as a Boundary (same owner, same block).
    fn us_test_top_box() -> Boundary {
        use crate::flybound::invert_placement;
        let owner = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 156.043, -143.19, 68.2943];
        let block = [500.0, 0.0, 0.0, 0.0, 500.0, 0.0, 0.0, 0.0, 70.0, 0.0, 0.0, 0.0];
        Boundary::read(&block, invert_placement(&owner), false)
    }

    /// [game 00868fb0 inverted branch, docs/flight-boundary.md 12.11] A helicopter climbs out
    /// through the top with the stick up (look.y = -1, P < 0) for 7 s, then is released. Outside,
    /// the wall stop passes P = +1 (lift DOWN) and ignores the stick, so it is pushed down and comes
    /// back inside. The lift value 15 is the test value of `heli_tuning`, not the game's (not located).
    /// Run with --nocapture for the trace.
    #[test]
    fn a_flying_helicopter_climbing_out_through_the_top_comes_back_down() {
        let b = us_test_top_box();
        let tun = heli_tuning();
        let ground = Surroundings { ground: Some(0.0), ..Default::default() };
        let mut f = Fly::enter(Vec3::new(-137.46, -366.85, 0.9), Vec3::ZERO, 0.0);
        let climb = FlyInput { look: Vec2::new(0.0, -1.0), ..Default::default() };
        let mut left_at: Option<f32> = None;
        let mut back_at: Option<f32> = None;
        let mut max_z = f32::MIN;
        for i in 1..=(45.0 / DT) as usize {
            let t = i as f32 * DT;
            let input = if t <= 7.0 { climb } else { FlyInput::default() };
            f.step_in_bounded(&input, &tun, &ground, Some(&b), DT);
            max_z = max_z.max(f.pos.z);
            if left_at.is_none() && !f.inside_boundary {
                left_at = Some(t);
            }
            if left_at.is_some() && back_at.is_none() && f.inside_boundary {
                back_at = Some(t);
            }
            if i % 31 == 0 {
                eprintln!("t={:5.2} z={:8.2} vz={:7.2} inside={}", t, f.pos.z, f.vel.z, f.inside_boundary);
            }
        }
        let left = left_at.expect("the helicopter should leave through the top");
        let back = back_at.expect("the helicopter should be back inside within 45 s");
        eprintln!("helicopter: left at {left:.2} s, back at {back:.2} s, max z {max_z:.2}, final z {:.2}", f.pos.z);
        assert!(back - left <= 30.0, "left {left} back {back}");
        assert!(f.pos.z < 139.0, "{}", f.pos.z);
    }

    /// The same helicopter held climbing with full lift for the whole 45 s. Outside, the stick is
    /// ignored and P = +1, so each excursion above the top is pushed back down. The bound is what
    /// that rule gives: the speed cap (30 m/s) against lift 15 m/s^2 is an overshoot of about 30 m.
    /// The observed trace is printed (run with --nocapture); it is not asserted beyond the bound.
    #[test]
    fn a_flying_helicopter_held_climbing_stays_within_the_overshoot_of_the_top() {
        let b = us_test_top_box();
        let tun = heli_tuning();
        let ground = Surroundings { ground: Some(0.0), ..Default::default() };
        let mut f = Fly::enter(Vec3::new(-137.46, -366.85, 0.9), Vec3::ZERO, 0.0);
        let climb = FlyInput { look: Vec2::new(0.0, -1.0), ..Default::default() };
        let mut exits = 0;
        let mut was_inside = true;
        let mut max_z = f32::MIN;
        for i in 1..=(45.0 / DT) as usize {
            let t = i as f32 * DT;
            f.step_in_bounded(&climb, &tun, &ground, Some(&b), DT);
            max_z = max_z.max(f.pos.z);
            if was_inside && !f.inside_boundary {
                exits += 1;
            }
            was_inside = f.inside_boundary;
            if i % 31 == 0 {
                eprintln!("t={:5.2} z={:8.2} vz={:7.2} inside={}", t, f.pos.z, f.vel.z, f.inside_boundary);
            }
        }
        eprintln!("held climb: {exits} exits through the top, max z {max_z:.2}");
        assert!(exits >= 1, "the helicopter should reach the top");
        assert!(max_z < 139.0 + 40.0, "max z {max_z}");
    }

    #[test]
    fn a_flyer_inside_a_boundary_flies_exactly_as_with_none() {
        let b = box_500();
        let tun = starscream();
        let ground = Surroundings { ground: Some(0.0), ..Default::default() };
        let input = FlyInput { stick: Vec2::new(0.6, 0.2), ..Default::default() };
        let mut plain = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 0.0);
        let mut bounded = plain.clone();
        for _ in 0..80 {
            plain.step_in_bounded(&input, &tun, &ground, None, DT);
            bounded.step_in_bounded(&input, &tun, &ground, Some(&b), DT);
        }
        assert_eq!(plain.pos, bounded.pos);
        assert_eq!(plain.vel, bounded.vel);
        assert_eq!(plain.heading, bounded.heading);
        assert!(bounded.inside_boundary);
        assert!(!bounded.steer.outside);
    }

    #[test]
    fn with_no_boundary_the_inside_flag_stays_set() {
        let f = fly(FlyInput::default(), 10);
        assert!(f.inside_boundary);
        assert!(!f.steer.outside);
    }

    #[test]
    fn level_flight_reaches_the_top_speed_and_holds_it() {
        let f = fly(FlyInput::default(), 100);
        assert!((f.vel.length() - 45.0).abs() < 1e-3, "{}", f.vel.length());
        assert!(f.vel.y > 44.9 && f.vel.x.abs() < 1e-3 && f.vel.z.abs() < 1e-3, "{:?}", f.vel);
        assert!((f.pos.z - 100.0).abs() < 1e-3, "it neither climbs nor sinks: {}", f.pos.z);
    }

    #[test]
    fn the_stick_rolls_and_turns_it_to_that_side() {
        let f = fly(FlyInput { stick: Vec2::new(1.0, 0.0), ..Default::default() }, 100);
        assert!((f.roll - 70.0 * DEG).abs() < 0.01, "{}", f.roll / DEG);
        assert!((f.yaw_rate - 2.8).abs() < 0.05, "{}", f.yaw_rate);
        // To the right: the velocity follows the nose round.
        let half = fly(FlyInput { stick: Vec2::new(1.0, 0.0), ..Default::default() }, 20);
        assert!(half.heading < 0.0 && half.vel.x > 1.0, "{} {:?}", half.heading, half.vel);
        // Half a stick is a quarter of the turn: the stick is squared.
        let soft = fly(FlyInput { stick: Vec2::new(0.5, 0.0), ..Default::default() }, 200);
        assert!((soft.yaw_rate - 0.7).abs() < 0.02, "{}", soft.yaw_rate);
    }

    #[test]
    fn an_armed_stick_starts_a_stunt_and_the_roll_flies_straight() {
        let t = starscream();
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 50.0), Vec3::Y * 20.0, 0.0);
        // Not armed: the stick only banks.
        f.step(&FlyInput { stick: Vec2::new(1.0, 0.0), ..Default::default() }, &t, Some(0.0), DT);
        assert_eq!(f.stunt, Stunt::None);
        f.step(&FlyInput { stick: Vec2::new(1.0, 0.0), stunt: true, ..Default::default() }, &t, Some(0.0), DT);
        assert_eq!(f.stunt, Stunt::Right);
        let rolled = f.roll;
        for _ in 0..30 {
            f.step(&FlyInput { stick: Vec2::new(-1.0, 0.0), ..Default::default() }, &t, Some(0.0), DT);
        }
        assert!(f.roll.abs() < rolled.abs() * 0.1 + 1e-4 && (f.vel.length() - t.max_speed).abs() < 1.0 && f.vel.x.abs() < 1.0, "{} {:?}", f.roll, f.vel);
        // The loops: up at once, down only well over the ground.
        let mut low = Fly::enter(Vec3::new(0.0, 0.0, 5.0), Vec3::Y * 20.0, 0.0);
        low.step(&FlyInput { stick: Vec2::new(0.0, 1.0), stunt: true, ..Default::default() }, &t, Some(0.0), DT);
        assert_eq!(low.stunt, Stunt::None);
        low.step(&FlyInput { stick: Vec2::new(0.0, -1.0), stunt: true, ..Default::default() }, &t, Some(0.0), DT);
        assert_eq!(low.stunt, Stunt::Up);
    }

    #[test]
    fn the_hold_button_latches_the_roll_and_turns_tighter() {
        let right = FlyInput { stick: Vec2::new(1.0, 0.0), hold: true, ..Default::default() };
        let mut f = fly(right, 150);
        assert!((f.roll - 90.0 * DEG).abs() < 0.01, "{}", f.roll / DEG);
        assert!((f.yaw_rate - 3.8).abs() < 0.05, "{}", f.yaw_rate);
        // The stick let go, the button still down: the roll stays where it was latched.
        for _ in 0..100 {
            f.step(&FlyInput { hold: true, ..Default::default() }, &starscream(), Some(0.0), DT);
        }
        assert!((f.roll - 90.0 * DEG).abs() < 0.01 && f.yaw_rate.abs() < 0.01, "{} {}", f.roll / DEG, f.yaw_rate);
        // Let go and pressed again to the other side: a new latch.
        f.step(&FlyInput::default(), &starscream(), Some(0.0), DT);
        for _ in 0..150 {
            f.step(&FlyInput { stick: Vec2::new(-1.0, 0.0), hold: true, ..Default::default() }, &starscream(), Some(0.0), DT);
        }
        assert!((f.roll + 90.0 * DEG).abs() < 0.01, "{}", f.roll / DEG);
    }

    #[test]
    fn a_wall_ahead_stops_it_and_a_slanted_one_slides_it_along() {
        // A wall across the way at y = 200: it never gets there.
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 0.0);
        let wall = |f: &Fly, normal: Vec3, at: f32| {
            f.wall_rays().map(|(from, to)| {
                let (a, b) = (normal.dot(from) - at, normal.dot(to) - at);
                (a > 0.0 && b < 0.0).then(|| (WALL_RAY * a / (a - b), normal))
            })
        };
        for _ in 0..400 {
            let around = Surroundings { ground: Some(0.0), walls: wall(&f, Vec3::NEG_Y, -200.0) };
            f.step_in(&FlyInput::default(), &starscream(), &around, DT);
            assert!(f.pos.y < 200.0, "{}", f.pos.y);
        }
        assert!(f.near_wall && f.vel.length() < 5.0, "{:?}", f.vel);
        // A wall at 45 degrees: it slides along it, and does not cross it.
        let slant = Vec3::new(-1.0, -1.0, 0.0).normalize();
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 0.0);
        for _ in 0..400 {
            let around = Surroundings { ground: Some(0.0), walls: wall(&f, slant, -140.0) };
            f.step_in(&FlyInput::default(), &starscream(), &around, DT);
            assert!(slant.dot(f.pos) > -140.0, "{:?}", f.pos);
        }
        assert!(f.vel.x < -10.0 && f.vel.dot(slant).abs() < 3.0, "{:?}", f.vel);
    }

    /// [game 0086e3c0-0086e402] a corner pushes up (s = +1) when the stick is pulled past -0.25,
    /// and down (s = -1) when it is pushed past 0.25.
    #[test]
    fn a_corner_pushes_up_when_the_stick_is_pulled_and_down_when_pushed() {
        let walls = [Some((5.0, Vec3::Y)), Some((5.0, Vec3::X)), None];
        let mut up = Fly::enter(Vec3::ZERO, Vec3::ZERO, 0.0);
        up.meet_walls(&walls, -0.5, DT);
        assert!(up.wall_push.z > 0.0, "{:?}", up.wall_push);
        let mut down = Fly::enter(Vec3::ZERO, Vec3::ZERO, 0.0);
        down.meet_walls(&walls, 0.5, DT);
        assert!(down.wall_push.z < 0.0, "{:?}", down.wall_push);
    }

    /// [game 0086e020, third pass] with the stick between -0.25 and 0.25 the sign slot holds a
    /// flag word, not ±1, so the corner's vertical target is 0 and no lift is added.
    #[test]
    fn a_corner_with_the_stick_centred_adds_no_vertical_push() {
        let walls = [Some((5.0, Vec3::Y)), Some((5.0, Vec3::X)), None];
        let mut f = Fly::enter(Vec3::ZERO, Vec3::ZERO, 0.0);
        for _ in 0..10 {
            f.meet_walls(&walls, 0.0, DT);
        }
        assert!(f.wall_push.z.abs() < 1e-9, "{:?}", f.wall_push);
    }

    /// [game 007e7830 l.7, 007e78fe] the boundary is tested at the camera target point when the
    /// caller gives it (`boundary_point`), not at the body's position.
    #[test]
    fn the_boundary_is_tested_at_the_camera_target_point_when_given() {
        let b = Boundary { a: crate::flybound::Affine::IDENTITY, b: crate::flybound::Affine::IDENTITY, ellipsoid: false };
        let t = starscream();
        let mut f = Fly::enter(Vec3::ZERO, Vec3::ZERO, 0.0);
        assert!(!f.boundary_contact(&b, &t), "the body is inside");
        f.boundary_point = Some(Vec3::new(5.0, 0.0, 0.0));
        assert!(f.boundary_contact(&b, &t), "the target point is outside");
        assert!(!f.inside_boundary);
    }

    #[test]
    fn the_helicopter_hangs_strafes_turns_and_climbs() {
        let tun = FlyTuning {
            helicopter: true, thrust: 30.0, lateral_thrust: 20.0, lift: 15.0, max_speed: 30.0, turn_speed: 2.0,
            max_turn_speed: 1.5, turn_lag: 4.0, turn_restore_lag: 4.0, max_pitch: 25.0, min_pitch: -25.0, max_roll: 25.0,
            roll_strength: 2.0, pitch_strength: 2.0, ..Default::default()
        };
        let run = |input: FlyInput, updates: usize| {
            let mut f = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 0.0);
            for _ in 0..updates {
                f.step(&input, &tun, Some(0.0), DT);
            }
            f
        };
        // Nothing asked: it stays where it is.
        let f = run(FlyInput::default(), 100);
        assert!(f.vel.length() < 1e-4 && f.pos.distance(Vec3::new(0.0, 0.0, 100.0)) < 1e-3, "{:?}", f.pos);
        // Forward: along the nose, nose down, level.
        let f = run(FlyInput { stick: Vec2::Y, ..Default::default() }, 150);
        assert!((f.vel.y - 30.0).abs() < 0.5 && f.vel.z.abs() < 1e-3 && f.pitch < -20.0 * DEG, "{:?} {}", f.vel, f.pitch / DEG);
        // Let go: it slows to a stop.
        let mut g = f.clone();
        for _ in 0..200 {
            g.step(&FlyInput::default(), &tun, Some(0.0), DT);
        }
        assert!(g.vel.length() < 0.1, "{:?}", g.vel);
        // To the right: sideways, leaning, the heading kept.
        let f = run(FlyInput { stick: Vec2::X, ..Default::default() }, 100);
        assert!(f.vel.x > 10.0 && f.heading == 0.0 && f.roll > 20.0 * DEG, "{:?}", f.vel);
        // The other stick turns it, no faster than maxTurnSpeed, and lifts it.
        let f = run(FlyInput { look: Vec2::new(1.0, -1.0), ..Default::default() }, 30);
        assert!((f.yaw_rate - 1.5).abs() < 1e-4 && f.heading < 0.0 && f.pos.z > 100.0, "{} {:?}", f.yaw_rate, f.pos);
    }

    #[test]
    fn pitch_stops_at_its_limit_and_levels_when_let_go() {
        let mut f = fly(FlyInput { stick: Vec2::new(0.0, -1.0), ..Default::default() }, 200);
        assert!((f.pitch - 65.0 * DEG).abs() < 0.01, "{}", f.pitch / DEG);
        assert!(f.pos.z > 100.0);
        for _ in 0..200 {
            f.step(&FlyInput::default(), &starscream(), Some(0.0), DT);
        }
        assert!(f.pitch.abs() < 0.01, "{}", f.pitch / DEG);
    }

    #[test]
    fn turbo_lifts_the_cap_and_it_comes_back_down() {
        let turbo = FlyInput { turbo: true, ..Default::default() };
        let mut f = fly(FlyInput::default(), 100);
        let mut top: f32 = 0.0;
        for _ in 0..70 {
            f.step(&turbo, &starscream(), Some(0.0), DT);
            top = top.max(f.vel.length());
        }
        assert!((top - 65.0).abs() < 1e-3, "{top}");
        for _ in 0..80 {
            f.step(&FlyInput::default(), &starscream(), Some(0.0), DT);
        }
        assert!((f.vel.length() - 45.0).abs() < 1e-3, "{}", f.vel.length());
    }

    /// [game] 00864320 / 00733180 / 00869df0: one press runs the whole maxTurboTime (letting
    /// go does not end it), a held button does not start it again in its cooldown, and when
    /// it runs out the speed is SET to the turbo cap and comes down at postTurboDeceleration.
    #[test]
    fn with_the_stick_at_rest_it_holds_its_height() {
        // [game] FlyMode's enter zeroes the body's gravity (0071eb80 at 008658b3): level, with
        // no stick, nothing pulls it down, at speed or from a standstill.
        let f = fly(FlyInput::default(), 300);
        assert!((f.pos.z - 100.0).abs() < 1e-3, "{}", f.pos.z);
        assert!(f.vel.z.abs() < 1e-4, "{}", f.vel.z);
        assert!(f.vel.length() > 1.0, "it is flying, not hanging: {}", f.vel.length());
    }

    #[test]
    fn the_timers_tick_on_foot_and_mark_a_turbo_ended_only_in_a_vehicle_mode() {
        let t = starscream();
        let mut on_foot = FlyTimers { turbo_left: 0.1, ..Default::default() };
        let mut in_car = on_foot;
        on_foot.tick(&t, false, 0.2);
        in_car.tick(&t, true, 0.2);
        assert_eq!((on_foot.turbo_left, on_foot.turbo_cooldown), (0.0, 5.5));
        assert!(!on_foot.turbo_ended && in_car.turbo_ended);
        on_foot.tick(&t, false, 0.5);
        assert!((on_foot.turbo_cooldown - 5.0).abs() < 1e-6);
        // A flight puts them in and takes them out unchanged.
        let mut f = Fly::enter(Vec3::ZERO, Vec3::ZERO, 0.0);
        f.set_timers(on_foot);
        assert_eq!(f.timers(), on_foot);
    }

    #[test]
    fn a_turbo_press_runs_its_time_then_the_speed_comes_down_from_the_turbo_cap() {
        let t = starscream();
        let mut f = fly(FlyInput::default(), 100);
        f.step(&FlyInput { turbo: true, ..Default::default() }, &t, Some(0.0), DT);
        // [game 0071dc20 l.146, 0071eb50] The press is read in the mode's update and the ability
        // list ticks after it, so the press's own update already takes one DT off (2.5 - dt).
        assert!((f.turbo_left - (2.5 - DT)).abs() < 1e-6, "{}", f.turbo_left);
        let mut steps = 1;
        while f.turbo_left > 0.0 {
            f.step(&FlyInput::default(), &t, Some(0.0), DT);
            steps += 1;
        }
        // The press counts as the first tick: 2.5 s at 0.032 runs out on the 79th update, the one
        // whose tick sets `turbo_ended` (2.5 / 0.032 = 78.1, so the 79th tick takes it to zero).
        assert_eq!(steps, 79);
        assert!(f.turbo_ended);
        assert!((f.turbo_cooldown - 5.5).abs() < 1e-6);
        // The mode's update reads the ended byte before the tick, so the slowdown starts on the
        // update after the expiry: the latch starts at 65 and loses 20 * dt.
        assert!(!f.post_turbo);
        f.step(&FlyInput::default(), &t, Some(0.0), DT);
        assert!(f.post_turbo);
        assert!((f.vel.length() - (65.0 - 20.0 * DT)).abs() < 1e-3, "{}", f.vel.length());
        // Held through the cooldown: no new turbo.
        for _ in 0..100 {
            f.step(&FlyInput { turbo: true, ..Default::default() }, &t, Some(0.0), DT);
        }
        assert_eq!(f.turbo_left, 0.0);
        assert!(!f.turbo_ended && !f.post_turbo);
        assert!((f.vel.length() - 45.0).abs() < 1e-3);
    }

    /// [game] 00864d60 writes neither mode+0x4a8 nor +0x4ac, so an unfinished slowdown is still
    /// there in the next flight: it is kept with the timers (`FlyTimers`).
    #[test]
    fn the_after_turbo_slowdown_carries_into_the_next_flight() {
        let mut first = Fly::enter(glam::Vec3::ZERO, glam::Vec3::new(0.0, 60.0, 0.0), 0.0);
        first.post_turbo = true;
        first.speed_latch = 65.0;
        let kept = first.timers();
        let mut next = Fly::enter(glam::Vec3::ZERO, glam::Vec3::new(0.0, 60.0, 0.0), 0.0);
        assert!(!next.post_turbo && next.speed_latch == 0.0, "a fresh Fly starts with nothing");
        next.set_timers(kept);
        assert!(next.post_turbo);
        assert_eq!(next.speed_latch, 65.0);
    }

    /// [game] 00869df0 0086a3f9-0086a413: beside a wall the ended flag is simply dropped.
    #[test]
    fn a_wall_beside_it_drops_the_after_turbo_slowdown() {
        let t = starscream();
        let mut f = fly(FlyInput::default(), 10);
        f.turbo_ended = true;
        f.near_wall = true;
        let v = f.vel;
        f.cap_speed(&t, false, DT);
        assert!(!f.turbo_ended && !f.post_turbo);
        assert_eq!(f.vel, v);
    }

    /// [game] 00864320 step 5 and 00869df0 0086a04a-0086a10c: a brake press latches the speed,
    /// which then drops by brakingSpeed a second down to brakeSpeedScale of the cap, for
    /// brakeDuration; then its cooldown; a held trigger is one press.
    #[test]
    fn the_brake_slows_it_to_half_the_cap_for_its_duration() {
        let t = FlyTuning { braking_speed: 20.0, brake_speed_scale: 0.5, brake_duration: 3.0, brake_cooldown: 1.0, ..starscream() };
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 0.0);
        for _ in 0..100 {
            f.step(&FlyInput::default(), &t, Some(0.0), DT);
        }
        assert!((f.vel.length() - 45.0).abs() < 1e-3);
        let brake = FlyInput { brake: true, ..Default::default() };
        f.step(&brake, &t, Some(0.0), DT);
        assert!(f.brake_left > 0.0);
        // The latch was 45 at the press; this update it lost 20 * dt.
        assert!((f.vel.length() - (45.0 - 20.0 * DT)).abs() < 1e-3, "{}", f.vel.length());
        let mut low: f32 = f32::MAX;
        for _ in 0..80 {
            f.step(&brake, &t, Some(0.0), DT);
            low = low.min(f.vel.length());
        }
        assert!((low - 22.5).abs() < 1e-3, "{low}");
        // 3 s in all, then the cooldown, and the trigger still held starts nothing.
        while f.brake_left > 0.0 {
            f.step(&brake, &t, Some(0.0), DT);
        }
        assert!(f.brake_cooldown > 0.0);
        for _ in 0..60 {
            f.step(&brake, &t, Some(0.0), DT);
        }
        assert_eq!(f.brake_left, 0.0);
    }

    /// [game] 0086a840: the normal cap times the status scale.
    #[test]
    fn a_slowed_flyer_has_the_scaled_cap() {
        let f = fly(FlyInput { cap_scale: Some(0.8), ..Default::default() }, 150);
        assert!((f.vel.length() - 36.0).abs() < 1e-3, "{}", f.vel.length());
    }

    /// [game] 00853cb0 / 00854160: entering sets the avatar blend to 1, a second of updates
    /// takes it to 0 (the camera's Transition mode lasts that long).
    #[test]
    fn the_avatar_blend_runs_out_after_a_second() {
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 0.0);
        assert_eq!(f.avatar_blend, 1.0);
        for _ in 0..31 {
            f.step(&FlyInput::default(), &starscream(), Some(0.0), DT);
        }
        assert!(f.avatar_blend > 0.0);
        f.step(&FlyInput::default(), &starscream(), Some(0.0), DT);
        assert_eq!(f.avatar_blend, 0.0);
    }

    #[test]
    fn it_does_not_go_through_the_floor() {
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, 0.0);
        for _ in 0..300 {
            f.step(&FlyInput { stick: Vec2::new(0.0, 1.0), ..Default::default() }, &starscream(), Some(0.0), DT);
            assert!(f.pos.z >= 0.0, "{}", f.pos.z);
        }
    }

    fn ai_at(target: Vec3, stick: Vec2) -> FlyInput {
        FlyInput { stick, ai: Some(AiSteer { target, obstacle_ahead: false }), ..Default::default() }
    }

    #[test]
    fn angles_of_undoes_the_rotation() {
        for (p, r, h) in [(0.3, -0.5, 2.0), (-1.0, 0.2, -2.9), (0.0, 0.0, 0.7)] {
            let mut f = Fly::enter(Vec3::ZERO, Vec3::ZERO, h);
            (f.pitch, f.roll) = (p, r);
            let (p2, r2, h2) = angles_of(glam::Mat3::from_quat(f.rotation()));
            assert!((p - p2).abs() < 1e-4 && (r - r2).abs() < 1e-4 && (wrap(h - h2)).abs() < 1e-4, "{p} {r} {h} -> {p2} {r2} {h2}");
        }
    }

    /// [game] 00863970's local points: the side rays start 5 m to each side, 1.5 m below.
    #[test]
    fn the_side_rays_start_five_metres_out_and_a_metre_and_a_half_down() {
        let f = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 0.0);
        let rays = f.wall_rays();
        assert_eq!(rays[0].0, Vec3::new(0.0, 0.0, 100.0));
        assert!((rays[1].0 - Vec3::new(-5.0, 0.0, 98.5)).length() < 1e-5);
        assert!((rays[2].0 - Vec3::new(5.0, 0.0, 98.5)).length() < 1e-5);
        assert!((rays[2].1 - rays[2].0 - Vec3::new(0.0, 35.0, 0.0)).length() < 1e-4);
    }

    /// [game] 00570250's gimbal branch: nose straight up, the roll is dropped and the heading
    /// is read off the right axis.
    #[test]
    fn angles_of_straight_up_puts_no_roll_and_reads_the_heading_off_the_right_axis() {
        let mut f = Fly::enter(Vec3::ZERO, Vec3::ZERO, 0.8);
        f.pitch = std::f32::consts::FRAC_PI_2;
        let (p, r, h) = angles_of(glam::Mat3::from_quat(f.rotation()));
        assert!((p - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "{p}");
        assert_eq!(r, 0.0);
        assert!((h - 0.8).abs() < 1e-5, "{h}");
    }

    #[test]
    fn look_at_points_the_nose_and_keeps_the_up_and_the_frame_right_handed() {
        let frame = glam::Mat3::IDENTITY;
        let m = look_at(frame, Vec3::new(1.0, 1.0, 0.0));
        assert!((m.y_axis - Vec3::new(1.0, 1.0, 0.0).normalize()).length() < 1e-6);
        assert!((m.z_axis - Vec3::Z).length() < 1e-6);
        assert!((m.determinant() - 1.0).abs() < 1e-5);
        // Straight up: past |cos| 0.98 the up comes from the right axis.
        let up = look_at(frame, Vec3::Z);
        assert!((up.y_axis - Vec3::Z).length() < 1e-6 && (up.determinant() - 1.0).abs() < 1e-5);
        assert_eq!(look_at(frame, Vec3::splat(0.001)), frame);
    }

    #[test]
    fn turn_toward_turns_twice_the_angle_and_stops_on_the_target() {
        let from = glam::Mat3::IDENTITY;
        let to = glam::Mat3::from_rotation_z(1.0);
        // [game] the cap is on the quaternions' angle, half the frames': 0.1 turns 0.2.
        let step = turn_toward(from, to, 0.1);
        let turned = step.y_axis.x.atan2(step.y_axis.y).abs();
        assert!((turned - 0.2).abs() < 1e-4, "{turned}");
        assert!((turn_toward(from, to, 0.6).y_axis - to.y_axis).length() < 1e-5);
    }

    #[test]
    fn the_ai_turns_toward_its_target_at_the_capped_rate_and_then_flies_at_it() {
        let t = starscream();
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::new(0.0, 30.0, 0.0), 0.0);
        let target = Vec3::new(500.0, 0.0, 100.0);
        f.step(&ai_at(target, Vec2::ZERO), &t, None, DT);
        // [game] pi/4 a second on the quaternions turns the nose pi/2 a second.
        let turned = -f.heading;
        assert!((turned - std::f32::consts::FRAC_PI_2 * DT).abs() < 1e-3, "{turned}");
        for _ in 0..200 {
            f.step(&ai_at(target, Vec2::ZERO), &t, None, DT);
        }
        let to = (target - f.pos).normalize();
        assert!(f.forward().dot(to) > 0.99, "nose {:?} to {:?}", f.forward(), to);
        assert!(f.vel.normalize().dot(f.forward()) > 0.99);
        // An obstacle ahead triples the rate.
        let mut g = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 0.0);
        g.step(&FlyInput { ai: Some(AiSteer { target, obstacle_ahead: true }), ..Default::default() }, &t, None, DT);
        assert!((-g.heading - 3.0 * std::f32::consts::FRAC_PI_2 * DT).abs() < 1e-3, "{}", g.heading);
    }

    #[test]
    fn the_ai_rolls_by_its_stick_and_does_not_aim_down_near_the_ground() {
        let t = starscream();
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 0.0);
        for _ in 0..100 {
            f.step(&ai_at(Vec3::new(0.0, 1000.0, 100.0), Vec2::new(1.0, 0.0)), &t, None, DT);
        }
        assert!((f.roll - t.max_roll * DEG).abs() < 1e-3, "{}", f.roll);
        assert_eq!(f.yaw_rate, 0.0);
        // On the ground the target below is taken level: the nose stays level.
        let mut low = Fly::enter(Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, 0.0);
        low.step(&ai_at(Vec3::new(0.0, 100.0, -50.0), Vec2::ZERO), &t, Some(0.0), DT);
        assert!(low.pitch.abs() < 1e-5, "{}", low.pitch);
        let mut high = Fly::enter(Vec3::new(0.0, 0.0, 50.0), Vec3::ZERO, 0.0);
        high.step(&ai_at(Vec3::new(0.0, 100.0, -50.0), Vec2::ZERO), &t, Some(0.0), DT);
        assert!(high.pitch < -0.01, "{}", high.pitch);
    }

    #[test]
    fn the_hold_button_overrides_the_ai_steering() {
        let t = starscream();
        let mut a = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 0.0);
        let mut b = a.clone();
        let target = Vec3::new(500.0, 0.0, 100.0);
        a.step(&FlyInput { hold: true, ai: Some(AiSteer { target, obstacle_ahead: false }), ..Default::default() }, &t, None, DT);
        b.step(&FlyInput { hold: true, ..Default::default() }, &t, None, DT);
        assert_eq!(a, b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    use std::path::PathBuf;

    /// body_add_velocity's guard (00556b40) is the control vector's z and the turn vector's x that
    /// 006e8da0 stores every update: a stunt's [1,1,1] blocks the bounce and the ground push, the
    /// ordinary flight's zero vector lets them through. [game]
    #[test]
    fn the_body_takes_a_velocity_only_while_no_clip_drives_its_z() {
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 0.0);
        assert!(f.body_takes_velocity());
        f.control = Vec3::ONE;
        assert!(!f.body_takes_velocity());
        f.control = Vec3::new(1.0, 1.0, 0.0);
        assert!(f.body_takes_velocity());
    }

    #[test]
    fn the_flyers_fly_and_the_car_drives() {
        // Needs the install.
        let dir = PathBuf::from(crate::install::dir_string());
        if !dir.join("bnxglobal.str").is_file() {
            return;
        }
        let star = Forms::load(&dir, "Starscream").expect("Starscream");
        assert!(star.can_fly && !star.can_drive);
        assert_eq!((star.fly.thrust, star.fly.turbo_thrust, star.fly.max_speed, star.fly.max_turbo_speed), (50.0, 75.0, 45.0, 65.0));
        assert_eq!((star.fly.max_pitch, star.fly.min_pitch, star.fly.max_roll, star.fly.max_roll_quick), (65.0, -65.0, 70.0, 90.0));
        assert!(!star.fly.helicopter && star.fly.can_do_stunts);
        assert_eq!(star.fly.min_rotor_alpha, 0.25);
        let heli = Forms::load(&dir, "Blackout").expect("Blackout");
        assert!(heli.can_fly && heli.fly.helicopter);
        // [data] Blackout's bladeList: Rotor_A..F, four tail-rotor blades and four bases.
        assert_eq!(heli.fly.blade_list.len(), 14);
        assert_eq!(heli.fly.blade_list[0], crate::formats::hash::crc32(b"Rotor_A"));
        assert!(star.fly.blade_list.is_empty());
        let bee = Forms::load(&dir, "Bumblebee").expect("Bumblebee");
        assert!(!bee.can_fly && bee.can_drive);
        for name in ["Firestorm", "Jetfire", "OptimusFlight", "MegatronFlight", "Megatron", "Optimus"] {
            let f = Forms::load(&dir, name).expect(name);
            println!("{name:16} canFly {:5} canDrive {:5} robot {:5} helicopter {:5} thrust {} maxSpeed {} hover {}s", f.can_fly, f.can_drive, f.fly.robot, f.fly.helicopter, f.fly.thrust, f.fly.max_speed, f.hover.max_hover_time);
        }
        // [game 00726710] the extent is the LAST avatar row's width and length (`Forms::read`).
        for name in ["Starscream", "Blackout", "Jetfire"] {
            let f = Forms::load(&dir, name).expect(name);
            println!("{name:16} extent roll (width) {} pitch (length) {}", f.fly.extent_roll, f.fly.extent_pitch);
            assert!(f.fly.extent_roll > 0.0 && f.fly.extent_pitch > 0.0, "{name} has no vehicle extent");
            if name == "Starscream" {
                // [data] characterAttributesStarscream avatarDescArray[1]: width 4, length 4.5.
                assert_eq!((f.fly.extent_roll, f.fly.extent_pitch), (4.0, 4.5));
            }
        }
    }

    /// [game] S' = pos.z + 2 sin pitch - ground - 2 e_roll |sin roll| - 2 e_pitch |sin pitch|
    /// (00866ef0 00867364-00867394; 0086b230 0086c49c-0086c504). Level, it is the plain height.
    #[test]
    fn a_tilted_body_is_measured_down_by_its_extent_and_the_nose_offset() {
        let t = FlyTuning { extent_roll: 1.5, extent_pitch: 3.0, ..Default::default() };
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, 0.0);
        assert!((f.height_over(4.0, &t) - 6.0).abs() < 1e-5, "level: the place less the ground");
        f.pitch = 0.3;
        f.roll = -0.2;
        let (sp, sr) = (0.3f32.sin(), 0.2f32.sin());
        let want = (10.0 + 2.0 * sp - 4.0) - 2.0 * (3.0 * sp + 1.5 * sr);
        assert!((f.height_over(4.0, &t) - want).abs() < 1e-5, "{} vs {want}", f.height_over(4.0, &t));
        // With no extent, only the nose's own offset is left.
        let none = FlyTuning::default();
        assert!((f.height_over(4.0, &none) - (6.0 + 2.0 * sp)).abs() < 1e-5);
    }

    /// [game] the latch's thresholds: under 2 m (3 m with the variant), a steep-enough ground, and at the
    /// threshold itself not under it (0086c514-0086c562: `jbe` when thr <= S').
    #[test]
    fn the_ground_latch_is_under_two_metres_and_three_with_the_variant_on_steep_ground() {
        let t = FlyTuning::default();
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 2.0), Vec3::ZERO, 0.0);
        assert!(!f.on_ground_latch(Some(0.0), &t), "exactly 2 m is not under it");
        f.pos.z = 1.99;
        assert!(f.on_ground_latch(Some(0.0), &t));
        assert!(!f.on_ground_latch(None, &t), "no ground, no latch");
        f.ground_normal_z = 0.4;
        assert!(!f.on_ground_latch(Some(0.0), &t), "a ground with |normal z| <= 0.5 does not latch");
        f.ground_normal_z = 0.9;
        f.three_metre_latch = true;
        assert!(f.on_ground_latch(Some(0.0), &t), "1.99 m is under the 3 m variant");
        f.pos.z = 3.0;
        assert!(!f.on_ground_latch(Some(0.0), &t), "exactly 3 m is not under the variant");
        f.pos.z = 2.5;
        assert!(f.on_ground_latch(Some(0.0), &t));
    }

    /// The extent is what lets a pitched body latch where the same place level would not:
    /// level at 2.5 m is above the latch; nose up 0.3 rad with a 3 m extent it is S' ~ 1.32 m.
    #[test]
    fn a_pitched_body_latches_where_a_level_one_at_the_same_place_does_not() {
        let t = FlyTuning { extent_pitch: 3.0, ..Default::default() };
        let mut f = Fly::enter(Vec3::new(0.0, 0.0, 2.5), Vec3::ZERO, 0.0);
        assert!(!f.on_ground_latch(Some(0.0), &t));
        f.pitch = 0.3;
        let sp = 0.3f32.sin();
        assert!((f.height_over(0.0, &t) - (2.5 + 2.0 * sp - 6.0 * sp)).abs() < 1e-5);
        assert!(f.on_ground_latch(Some(0.0), &t));
    }
}
