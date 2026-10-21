//! The flying form's moving parts: FlyMode's per-step visual routine 0086cbf0
//! (`notes\flight-forces.md` "0086cbf0 flight_surface_anim"), called near the end of every
//! FlyMode update (00864320, after the steering), and the rotor-speed smoothing at the end of
//! the helicopter steering 00867f20 (SM, `notes\flight-helicopter.md`).
//!
//! A jet (`modeFly.helicopter` false) turns its control surfaces by the rates of its own angles:
//! the ailerons and flaperons by the roll rate, the "rutterwings" (the tailplanes) by the pitch
//! rate and the fins by the turn rate, both seen from the body (turned by the roll). A form that
//! flies as a helicopter (Blackout, and the robot flyers) spins its main rotor at a smoothed
//! speed and its tail rotor at a fixed one, and fades the blades it names (`bladeList`) down to
//! `minRotorAlpha`. Nothing here moves the body. [game 0086cbf0, read in disassembly 2026-10-20]

use glam::{Quat, Vec3};

use crate::flight::FlyTuning;

/// Every node 0086cbf0 looks up by name (CRC-32, `FUN_00578440`), for audits.
pub const NODES: [&str; 14] = [
    "Aileron_L", "Aileron_R", "Flaperon_L", "Flaperon_R",
    "Rutterwing_L", "Rutterwing_R", "Rutterwing_L01", "Rutterwing_R01",
    "Vert_StabilizerA_L", "Vert_StabilizerB_L", "Vert_StabilizerA_R", "Vert_StabilizerB_R",
    "Rotor_GOAL", "RearRotor",
];

/// The rate the body rates are smoothed at, per second. [game] 4.4 at 00b6b344 (0x86cfbb).
const RATE_SMOOTHING: f32 = 4.4;
/// The rotor alpha falls this much a second. [game] 2.0 at 00b38838 (0x86cd19).
const ALPHA_FALL: f32 = 2.0;
/// The tail rotor's turn, radians a second, whatever the main rotor does. [game] 15.707964
/// (5 pi) at 00b6b348 (0x86ced0): `rotate_x_matrix(dt * 5 pi)` on RearRotor each step.
const REAR_ROTOR_RATE: f32 = 15.707_964;
/// [game] flymode_init 00863970: mode+0x490 = 1100, +0x494 = 1500, +0x498 = 900 (degrees a
/// second; the floats 0x44898000, 0x44bb8000, 0x44610000). No other FlyMode function writes
/// +0x494 or +0x498 (`decomp_lookup grep`, 2026-10-20): they are constants.
pub const ROTOR_SPEED_START: f32 = 1100.0;
const ROTOR_SPEED_LIFT: f32 = 1500.0;
const ROTOR_SPEED_SINK: f32 = 900.0;

/// How a node's turn is laid on the vehicle clip's pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Apply {
    /// [game] the jets' surfaces: `rotate_x/z_matrix`, `basis_to_affine` (00560fa0, which zeroes
    /// the translation) and `base_pose_set_local` (00545850): the node's local transform is
    /// replaced by the bare turn, whatever the clip or the bind put there.
    Replace(Quat),
    /// [game] the rotors: the node's current local (its BasePose row, `00572110` from
    /// `+0x6c + 0x30 * (index - 1)`) composed with the turn on the node's own side
    /// (`compose_affine` 0052fbf0 with P1 = current, P2 = turn: current * turn in glam's terms),
    /// kept in the BasePose, so the turn adds up step by step.
    After(Quat),
}

/// FlyMode's state for 0086cbf0. The game keeps it in the FlyMode object, which is built once
/// per character (00863970) and survives from one flight to the next: keep this on the
/// character, not in the flight.
#[derive(Clone, Debug, PartialEq)]
pub struct Surfaces {
    /// [game] mode+0x28, +0x2c, +0x30: the angles (pitch, roll, heading = mode+0x1c, +0x20,
    /// +0x24) as the last update left them. 00864320 copies them at its start (step 1); the
    /// port keeps them from the last call, which is the same value. 00863970 zeroes them.
    prev: Vec3,
    /// [game] mode+0x4ec, +0x4f0, +0x4f4: the smoothed rates of pitch, roll and heading,
    /// radians a second. Zero from 00863970 (param_2[0x13b..0x13d]); the enter leaves them.
    pub rates: Vec3,
    /// [game] mode+0x49c: the blades' alpha. 1 on entering (00864d60), down 2 a second to
    /// `minRotorAlpha`.
    pub rotor_alpha: f32,
    /// [game] mode+0x490: the main rotor's speed, degrees a second (SM of 00867f20).
    pub rotor_speed: f32,
    /// The rotors' turns so far, radians: the sum of the game's per-step increments (it keeps
    /// them in the BasePose instead). Main rotor about its y, tail rotor about its x.
    pub rotor_angle: f32,
    pub rear_rotor_angle: f32,
}

impl Default for Surfaces {
    fn default() -> Self {
        Self { prev: Vec3::ZERO, rates: Vec3::ZERO, rotor_alpha: 1.0, rotor_speed: ROTOR_SPEED_START, rotor_angle: 0.0, rear_rotor_angle: 0.0 }
    }
}

/// What one step decided, by node name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pose {
    pub turns: Vec<(&'static str, Apply)>,
    /// The blades' alpha and the blade names (CRC-32, `bladeList`), for a helicopter; the
    /// game sends it to every render instance on each named node or below it (0086ee10 ->
    /// 00588050 -> 00587c70 -> `lod_fade_table_write` 0058b3d0). Empty for a jet.
    pub blade_alpha: Option<f32>,
}

impl Surfaces {
    /// [game] FlyMode's enter 00864d60: `mode+0x49c = 1.0`. Nothing else here is reset.
    pub fn enter(&mut self) {
        self.rotor_alpha = 1.0;
    }

    /// [game] 00867f20's last block SM (00868a86-00868aee): the main rotor's speed eases toward
    /// 1500 while lifting (P >= 0.05), 900 while sinking (P <= -0.05), their mean otherwise, at
    /// rate dt (straight there when dt >= 1). `lift` is the steering's P at its end (the stack
    /// slot +0x7c that N4 reads too, after the turbo scale, the invert and the ground latch):
    /// positive P pushes the helicopter DOWN (`Fly::steer_helicopter`), yet takes the faster
    /// target; the comparison is on `0 - P` (00868a86) and reads that way. Runs only when that
    /// steering runs (a helicopter, or a robot flyer).
    pub fn steer_rotor(&mut self, lift: f32, dt: f32) {
        let x = -lift;
        let target = if x > -0.05 && x < 0.05 {
            (ROTOR_SPEED_SINK + ROTOR_SPEED_LIFT) * 0.5
        } else if x > 0.05 {
            ROTOR_SPEED_SINK
        } else {
            ROTOR_SPEED_LIFT
        };
        self.rotor_speed = if dt < 1.0 { self.rotor_speed + (target - self.rotor_speed) * dt } else { target };
    }

    /// One call of 0086cbf0 with the angles the update ends with (`Fly::pitch`, `roll`,
    /// `heading`: mode+0x1c, +0x20, +0x24), and the turns it leaves (`pose`). Nothing for
    /// dt <= 0 (the game returns at once, and leaves its previous angles alone too).
    pub fn step(&mut self, angles: Vec3, tun: &FlyTuning, dt: f32) -> Pose {
        if dt <= 0.0 {
            return Pose::default();
        }
        let prev = std::mem::replace(&mut self.prev, angles);
        if !tun.helicopter {
            self.fixed_wing(angles, prev, dt);
        } else {
            self.helicopter(tun, dt);
        }
        self.pose(tun)
    }

    /// [game] 0086cf4e-0086d06c (the branch on T+0x84 == 0, the `helicopter` byte): the rates.
    fn fixed_wing(&mut self, angles: Vec3, prev: Vec3, dt: f32) {
        // Each angle's change wrapped to [-pi, pi) (0056f290) over dt (x87, 1/dt kept at full
        // precision; 0086cf4e-0086cfae).
        let inv = 1.0 / dt;
        let raw = Vec3::new(wrap(angles.x - prev.x), wrap(angles.y - prev.y), wrap(angles.z - prev.z)) * inv;
        // Smoothed at k = 4.4 dt when k < 1, else taken whole (0086cfb2-0086d06c).
        let k = dt * RATE_SMOOTHING;
        if k < 1.0 {
            self.rates += (raw - self.rates) * dt * RATE_SMOOTHING;
        } else {
            self.rates = raw;
        }
    }

    /// [game] 0086cd10-0086cec2 (T+0x84 != 0): the alpha and the rotors' increments.
    fn helicopter(&mut self, tun: &FlyTuning, dt: f32) {
        // The alpha falls 2 a second, not below minRotorAlpha (tun+0xdc).
        self.rotor_alpha -= dt * ALPHA_FALL;
        if self.rotor_alpha < tun.min_rotor_alpha {
            self.rotor_alpha = tun.min_rotor_alpha;
        }
        // Main rotor: Ry(-(mode+0x490) * pi * (1/180) * dt) (FUN_0056fbd0, positive Ry; the
        // constants 3.1415927 at 00b387d4 and 0.0055555557 at 00b6b34c), after the current
        // local.
        self.rotor_angle = wrap(self.rotor_angle - self.rotor_speed * std::f32::consts::PI * (1.0 / 180.0) * dt);
        // Tail rotor: Rx(5 pi dt), after the current local.
        self.rear_rotor_angle = wrap(self.rear_rotor_angle + REAR_ROTOR_RATE * dt);
    }

    /// The turns the last step left on the nodes, from the kept state (the game leaves them in
    /// the BasePose rows; the port lays them on each frame's clip pose). [assumed] the summed
    /// rotor turn on top of the clip pose: in the game a clip still driving `Rotor_GOAL` (the
    /// transformation clips do) would overwrite the row each frame and the turn would not add
    /// up; settles: whether the anim controller keeps writing once the clip has ended.
    pub fn pose(&self, tun: &FlyTuning) -> Pose {
        if tun.helicopter {
            // The blade loop (00765b20 copies `bladeList`, tun+0x1d0; each name found in the
            // vehicle object's node-name table +0x74 is sent with the alpha, 0086ee10) comes
            // first: the blades fade even on a model with no Rotor_GOAL. A model with no
            // Rotor_GOAL stops there: RearRotor is not turned either (0086cdd5 jumps to the
            // exit; `apply`).
            return Pose {
                turns: vec![
                    ("Rotor_GOAL", Apply::After(Quat::from_rotation_y(self.rotor_angle))),
                    ("RearRotor", Apply::After(Quat::from_rotation_x(self.rear_rotor_angle))),
                ],
                blade_alpha: Some(self.rotor_alpha),
            };
        }
        let Vec3 { x: pitch, y: roll, z: turn } = self.rates;
        // Turned by the ROLL angle (mode+0x20, kept from 0086cf74 as [esp+0x40]; cos 0040b080,
        // sin 0040b0a0), x87 at 0086d089-0086d0b7:
        //   a = s_pitch cos r - s_turn sin r      (the tailplanes')
        //   b = s_turn cos r + s_pitch sin r      (the fins')
        let (s, c) = self.prev.y.sin_cos();
        let a = pitch * c - turn * s;
        let b = turn * c + pitch * s;
        // The turns and their signs (0086d0c6-0086d557), each `rotate_x_matrix` (0056fb50,
        // positive Rx) or `rotate_z_matrix` (0056fc50, positive Rz); the negated ones are
        // `-0.0 - x` (subss from 00b6a3ac). tun+0xbc aileronScale, +0xc0 elevatorScale,
        // +0xc4 rudderScale (the loader's offsets, `notes\flight-attributes.md`).
        let aileron = tun.aileron_scale * roll;
        let elevator = -tun.elevator_scale * a;
        let rudder = -tun.rudder_scale * b;
        let x = |angle: f32| Apply::Replace(Quat::from_rotation_x(angle));
        let z = |angle: f32| Apply::Replace(Quat::from_rotation_z(angle));
        Pose {
            turns: vec![
                ("Aileron_L", x(aileron)),
                ("Aileron_R", x(-aileron)),
                ("Flaperon_L", x(aileron)),
                ("Flaperon_R", x(-aileron)),
                ("Rutterwing_L", x(elevator)),
                ("Rutterwing_R", x(elevator)),
                ("Rutterwing_L01", x(elevator)),
                ("Rutterwing_R01", x(elevator)),
                ("Vert_StabilizerA_L", z(rudder)),
                ("Vert_StabilizerB_L", z(rudder)),
                ("Vert_StabilizerA_R", z(rudder)),
                ("Vert_StabilizerB_R", z(rudder)),
            ],
            blade_alpha: None,
        }
    }
}

/// [game] 0056f290 wrap_angle_floor: to [-pi, pi).
fn wrap(angle: f32) -> f32 {
    let turns = angle * 0.159_154_94 + 0.5;
    std::f32::consts::TAU * (turns - turns.floor() - 0.5)
}

/// The turns of one step laid on a model's local pose (`names` beside `pose`, rotations and
/// translations of each node's local transform). A Rotor_GOAL that is missing drops RearRotor
/// too, as the game does. Returns how many nodes were turned.
pub fn apply(pose: &Pose, names: &[String], rotation: &mut [Quat], translation: &mut [Vec3]) -> usize {
    let find = |name: &str| names.iter().position(|n| n == name);
    let mut turned = 0;
    for (name, turn) in &pose.turns {
        let Some(i) = find(name) else {
            if *name == "Rotor_GOAL" {
                break;
            }
            continue;
        };
        match turn {
            Apply::Replace(q) => {
                rotation[i] = *q;
                translation[i] = Vec3::ZERO;
            }
            Apply::After(q) => rotation[i] *= *q,
        }
        turned += 1;
    }
    turned
}

/// The nodes a blade alpha reaches: each named blade (CRC-32 of the node name, `bladeList`)
/// and every node below it. [game] 00587c70 walks a render instance's node up its parents
/// (`+0xc` -> `+0x70`, the parent table) and takes the instance when it meets the blade.
pub fn blade_nodes(blades: &[u32], name_hashes: &[u32], parents: &[Option<usize>]) -> Vec<bool> {
    let named: Vec<bool> = name_hashes.iter().map(|h| blades.contains(h)).collect();
    (0..name_hashes.len())
        .map(|mut i| loop {
            if named[i] {
                break true;
            }
            match parents[i] {
                Some(p) => i = p,
                None => break false,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jet() -> FlyTuning {
        FlyTuning { aileron_scale: 0.25, elevator_scale: 0.1, rudder_scale: 0.25, ..Default::default() }
    }

    fn heli() -> FlyTuning {
        FlyTuning { helicopter: true, min_rotor_alpha: 0.25, ..Default::default() }
    }

    fn turn(pose: &Pose, name: &str) -> Apply {
        pose.turns.iter().find(|(n, _)| *n == name).unwrap().1
    }

    fn angle_x(apply: Apply) -> f32 {
        let (Apply::Replace(q) | Apply::After(q)) = apply;
        let (axis, angle) = q.to_axis_angle();
        if angle.abs() < 1e-6 { 0.0 } else { angle * axis.x.signum() }
    }

    #[test]
    fn level_flight_leaves_the_surfaces_still() {
        let tun = jet();
        let mut s = Surfaces { prev: Vec3::new(0.1, 0.0, 1.0), ..Default::default() };
        let p = s.step(Vec3::new(0.1, 0.0, 1.0), &tun, 0.032);
        assert_eq!(s.rates, Vec3::ZERO);
        assert_eq!(angle_x(turn(&p, "Aileron_L")), 0.0);
        // The first flight starts from the zeroed angles of 00863970: the jump to the real
        // heading is one step's rate, smoothed, and dies away while the heading holds.
        let mut s = Surfaces::default();
        s.step(Vec3::new(0.0, 0.0, 1.0), &tun, 0.032);
        assert!((s.rates.z - 4.4).abs() < 1e-3, "{:?}", s.rates);
        for _ in 0..60 {
            s.step(Vec3::new(0.0, 0.0, 1.0), &tun, 0.032);
        }
        assert!(s.rates.z.abs() < 0.01, "{:?}", s.rates);
    }

    #[test]
    fn rates_are_smoothed_at_4_4_and_ailerons_follow_the_roll_rate() {
        let tun = jet();
        let mut s = Surfaces::default();
        let dt = 0.032;
        // Rolling at 1 rad/s, level otherwise.
        let p = s.step(Vec3::new(0.0, dt, 0.0), &tun, dt);
        let k = dt * 4.4;
        assert!((s.rates.y - k).abs() < 1e-5, "{}", s.rates.y);
        let a = 0.25 * k;
        assert!((angle_x(turn(&p, "Aileron_L")) - a).abs() < 1e-5);
        assert!((angle_x(turn(&p, "Aileron_R")) + a).abs() < 1e-5);
        assert!((angle_x(turn(&p, "Flaperon_L")) - a).abs() < 1e-5);
        assert!((angle_x(turn(&p, "Flaperon_R")) + a).abs() < 1e-5);
        // Nothing for the tailplanes or the fins.
        assert_eq!(angle_x(turn(&p, "Rutterwing_L")), 0.0);
    }

    #[test]
    fn a_long_step_takes_the_rate_whole() {
        let tun = jet();
        let mut s = Surfaces::default();
        s.step(Vec3::new(0.5, 0.0, 0.0), &tun, 0.25);
        assert!((s.rates.x - 2.0).abs() < 1e-5);
    }

    #[test]
    fn pitch_and_turn_rates_are_turned_by_the_roll() {
        let tun = jet();
        let dt = 0.25; // k >= 1: rates taken whole.
        // Rolled a quarter turn: a turn of the heading is a pitch of the body.
        let r = std::f32::consts::FRAC_PI_2;
        let mut s = Surfaces { prev: Vec3::new(0.0, r, 0.0), ..Default::default() };
        let p = s.step(Vec3::new(0.0, r, 0.5), &tun, dt);
        // a = s_pitch cos r - s_turn sin r = -2; b = s_turn cos r + s_pitch sin r = 0.
        let elevator = angle_x(turn(&p, "Rutterwing_L"));
        assert!((elevator - (-0.1 * -2.0)).abs() < 1e-5, "{elevator}");
        let Apply::Replace(q) = turn(&p, "Vert_StabilizerA_L") else { panic!() };
        assert!(q.angle_between(Quat::IDENTITY) < 1e-5);
        // Level: the heading's rate goes to the fins, negated (rudderScale 0.25).
        let mut s = Surfaces::default();
        let p = s.step(Vec3::new(0.0, 0.0, 0.5), &tun, dt);
        let Apply::Replace(q) = turn(&p, "Vert_StabilizerB_R") else { panic!() };
        assert!((q.to_euler(glam::EulerRot::XYZ).2 - (-0.25 * 2.0)).abs() < 1e-5);
    }

    #[test]
    fn rates_wrap_across_the_seam() {
        let tun = jet();
        let mut s = Surfaces { prev: Vec3::new(0.0, 0.0, 3.1), ..Default::default() };
        s.step(Vec3::new(0.0, 0.0, -3.1), &tun, 0.25);
        let expected = (std::f32::consts::TAU - 6.2) / 0.25;
        assert!((s.rates.z - expected).abs() < 1e-3, "{}", s.rates.z);
    }

    #[test]
    fn no_step_for_no_time() {
        let mut s = Surfaces { prev: Vec3::ONE, ..Default::default() };
        let p = s.step(Vec3::ZERO, &jet(), 0.0);
        assert!(p.turns.is_empty());
        assert_eq!(s.prev, Vec3::ONE);
    }

    #[test]
    fn rotor_alpha_falls_to_its_floor_and_enter_restores_it() {
        let tun = heli();
        let mut s = Surfaces::default();
        let p = s.step(Vec3::ZERO, &tun, 0.032);
        assert!((p.blade_alpha.unwrap() - (1.0 - 0.064)).abs() < 1e-6);
        for _ in 0..30 {
            s.step(Vec3::ZERO, &tun, 0.032);
        }
        assert_eq!(s.rotor_alpha, 0.25);
        s.enter();
        assert_eq!(s.rotor_alpha, 1.0);
    }

    #[test]
    fn rotors_spin_at_their_rates() {
        let tun = heli();
        let mut s = Surfaces::default();
        let dt = 0.032;
        s.step(Vec3::ZERO, &tun, dt);
        assert!((s.rotor_angle - (-1100.0f32).to_radians() * dt).abs() < 1e-5);
        assert!((s.rear_rotor_angle - 5.0 * std::f32::consts::PI * dt).abs() < 1e-5);
    }

    #[test]
    fn rotor_speed_eases_toward_the_lift_targets() {
        let mut s = Surfaces::default();
        s.steer_rotor(0.0, 0.5);
        assert!((s.rotor_speed - (1100.0 + (1200.0 - 1100.0) * 0.5)).abs() < 1e-3);
        s.steer_rotor(1.0, 1.0);
        assert_eq!(s.rotor_speed, 1500.0);
        s.steer_rotor(-1.0, 2.0);
        assert_eq!(s.rotor_speed, 900.0);
        s.steer_rotor(0.04, 1.0);
        assert_eq!(s.rotor_speed, 1200.0);
    }

    #[test]
    fn apply_replaces_surfaces_and_composes_rotors() {
        let names: Vec<String> = ["Body", "Aileron_L", "Rotor_GOAL", "RearRotor"].iter().map(|s| s.to_string()).collect();
        let mut rot = vec![Quat::from_rotation_z(0.3); 4];
        let mut at = vec![Vec3::ONE; 4];
        let pose = Pose {
            turns: vec![
                ("Aileron_L", Apply::Replace(Quat::from_rotation_x(0.2))),
                ("Rotor_GOAL", Apply::After(Quat::from_rotation_y(0.5))),
                ("RearRotor", Apply::After(Quat::from_rotation_x(0.5))),
            ],
            blade_alpha: None,
        };
        assert_eq!(apply(&pose, &names, &mut rot, &mut at), 3);
        assert_eq!(rot[1], Quat::from_rotation_x(0.2));
        assert_eq!(at[1], Vec3::ZERO);
        assert!(rot[2].angle_between(Quat::from_rotation_z(0.3) * Quat::from_rotation_y(0.5)) < 1e-6);
        assert_eq!(at[2], Vec3::ONE);
        // No Rotor_GOAL: the tail rotor is left alone too.
        let names: Vec<String> = ["Body", "RearRotor"].iter().map(|s| s.to_string()).collect();
        let mut rot = vec![Quat::IDENTITY; 2];
        let mut at = vec![Vec3::ZERO; 2];
        assert_eq!(apply(&pose, &names, &mut rot, &mut at), 0);
        assert_eq!(rot[1], Quat::IDENTITY);
    }

    #[test]
    fn blade_alpha_reaches_the_blades_and_below() {
        let hashes = [1, 2, 3, 4];
        let parents = [None, Some(0), Some(1), Some(0)];
        assert_eq!(blade_nodes(&[2], &hashes, &parents), vec![false, true, true, false]);
    }
}
