//! Where he aims in weapon mode, and how the stick turns it (`docs/weapon-mode.md`, "The
//! aim and the stick" and "The aim assist"). In the original the aim is the character's own
//! (a unit vector at owner `+0x260`): the stick turns the AIM (`FUN_0071b990`), his body and
//! the weapon camera then follow it. The camera never reads the stick in weapon mode.
//!
//! In here: the stick's turn with its ramp; the aim assist's slow-down over a target and
//! its pull toward one (`FUN_00709870` and the middle of `FUN_0071b990`); the turn to a
//! target in front of the camera as weapon mode is entered (`r2wLockOn`: `FUN_00715c50`
//! and the other branch of `FUN_0071b990`); and sniping's own rates (`Aim::step_sniping`,
//! for the Audi and Breakaway, the two with a sniper camera).

use glam::{Vec2, Vec3};

use crate::camera::{heading_of, wrap};
use crate::melee::{MeleeTuning, Query, Target};
use crate::tuning::AimTuning;

/// The option "aim sensitivity", 0 to 1 (options `+0x1f44`, the global `00dad8a0`): 0.5 is
/// what the options menu's "defaults" sets (`FUN_008e21a0`, `FUN_008e1550`, which also
/// turn the aim assist on). [game]
const SENSITIVITY: f32 = 0.5;
/// The stick's length squared under which it counts as let go. [game]
const STICK_STILL: f32 = 0.001;
/// Up to this much stick the push is scaled down for fine aiming; past it, it is the
/// stick's length cubed. The two do not meet (0.25 against 0.51 at the border). [game]
const FINE_UP_TO: f32 = 0.8;
const FINE_SCALE: f32 = 0.3125;
/// The rise or drop of the aim at which a clip's offset clip is laid on in full
/// (0x3f91361e, 65 degrees; the clips themselves turn him about that far). [game]
const OFFSET_MOST: f32 = 1.134_464;
/// The aim assist (`FUN_00709870`): the stick's length squared under which it is at rest
/// for the assist, and the one up to which the assist always slows the aim; and how far
/// from the picture's middle (pixels of a 1280 x 720 picture, squared) the target's point
/// may be before a hard push away from it gets out of the assist. [game]
const ASSIST_STILL: f32 = 0.01;
const ASSIST_HARD: f32 = 0.81;
const ASSIST_NEAR: f32 = 576.0;
/// How fast the aim turns to the target taken as weapon mode starts, in both angles
/// (0x4096cbe4: three quarters of a turn a second), and how near (the cosine between the
/// aim and the way to the target) ends it. [game]
const SNAP_RATE: f32 = 4.712_389;
const SNAP_DONE: f32 = 0.9999;
/// The search as weapon mode starts (`FUN_00715c50`): its height limit, the distance a
/// target is best at, and its weights (the query's words 100, 5, and 1, 2.5, 1).
/// [game: 00715c50 + query scorer 00799958..0079997f] Weights are distance=1,
/// yaw=2.5, pitch=1; the pitch and yaw limits are independent.
const SNAP_HEIGHT: f32 = 100.0;
const SNAP_BEST_DIST: f32 = 5.0;
const SNAP_ANGLE_WEIGHT: f32 = 2.5;
const SNAP_DIST_WEIGHT: f32 = 1.0;
const SNAP_PITCH_WEIGHT: f32 = 1.0;

/// The camera as the assist uses it: where it is and its three axes.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: Vec3,
    pub right: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
}

/// The target the aim assist has in its box (the aim-assist object's best, `+0x1c`), as
/// the assist's test looks at it.
#[derive(Clone, Copy, Debug)]
pub struct AssistTarget {
    /// Its `Head` node, or without one (or in car form) its place raised by three quarters
    /// of its height.
    pub point: Vec3,
    /// Where it stands (its object's place), which the pull turns toward.
    pub place: Vec3,
    pub velocity: Vec3,
    /// The squared distance of `point` from the picture's middle, in pixels of a
    /// 1280 x 720 picture (the assist object's `+0x24`).
    pub off_centre: f32,
}

/// What the assist's test says for one update: the aim's speed is to drop to
/// `cameraSpeedReduction`, and there is a target to pull toward (its place).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Assist {
    pub slow: bool,
    pub toward: Option<Vec3>,
}

/// The aim assist's test (`FUN_00709870`), run each update of weapon mode for a player who
/// is not sniping, with the option on. `stick` is the camera stick (x right, y up),
/// `own_velocity` his own. With a target in the box:
///
/// - The stick at rest: slow, with the target.
/// - The stick would turn the aim the way the target is moving across the picture, and
///   no faster than it moves (the turn of one update at the reduced speed against the
///   change of the target's bearing from the camera in one update, his own motion taken
///   off): NOT slow, so the aim can keep up; with the target.
/// - A push of 0.9 or less: slow, with the target.
/// - A harder push with the target's point within 24 pixels of the middle: the same.
/// - A harder push with the target further out: if the push is away from the target (the
///   stick laid in the picture against the way from the target to the middle) there is
///   no slowing and no target; else slow, with the target.
///
/// [game: 0070a990 returns body XYZ velocity] [assumed: the stick laid in the picture is right * x + up * y (the quarter turn `FUN_0070a4b0` makes of the camera's forward was taken to give its up); a push to the right turns the aim to the right]
/// Read 2026-10-20: 00709870 l.113-123 maps the stick (1a0,1a4,1a8) through the camera affine's columns, then rotates it by 0070a4b0 about camera affine row 0 with angle -pi/2 (constant 0xbfc90fdb). Not yet settled: the sign of that turn, and whether row 0 is right or forward. settles: the camera affine's row convention (docs/camera.md) and the sign of the -pi/2 turn in axis_angle_matrix_rowvec 0052e9a0.
pub fn assist(stick: Vec2, camera: &Camera, own_velocity: Vec3, target: Option<&AssistTarget>, t: &AimTuning, dt: f32) -> Assist {
    let Some(target) = target else { return Assist::default() };
    let with = |slow| Assist { slow, toward: Some(target.place) };
    let pushed = stick.length_squared();
    if pushed <= ASSIST_STILL {
        return with(true);
    }
    // The turn the stick asks for in one update at the reduced speed, as a change of
    // heading (positive toward -x, so a push to the right is negative).
    let rate = (t.lowest_sensitivity + (t.highest_sensitivity - t.lowest_sensitivity) * SENSITIVITY).to_radians();
    let side = if stick.x > 0.0 { -1.0 } else { 1.0 };
    let turn = wrap(t.speed_reduction * side * rate * dt);
    // How the target's bearing from the camera changes in one update.
    let moved = (target.velocity - own_velocity) * dt;
    let now = (target.point - camera.eye).normalize_or_zero();
    let next = (target.point + moved - camera.eye).normalize_or_zero();
    let heading = |v: Vec3| heading_of(Vec2::new(v.x, v.y));
    let drift = wrap(heading(next) - heading(now));
    if (turn > 0.0 && drift > 0.0 && drift >= turn) || (turn < 0.0 && drift < 0.0 && drift <= turn) {
        return with(false);
    }
    if pushed <= ASSIST_HARD || target.off_centre <= ASSIST_NEAR {
        return with(true);
    }
    // From the target toward the middle of the picture: forward x (forward x the way to it).
    let to_middle = camera.forward.cross(camera.forward.cross(now)).normalize_or_zero();
    let push = (camera.right * stick.x + camera.up * stick.y).normalize_or_zero();
    if push.dot(to_middle) > 0.0 { Assist::default() } else { with(true) }
}

/// The target the aim turns to as weapon mode starts (`FUN_00715c50`, run while the
/// character's flag 0x100000 is up, which `StateWeapon`'s enter raises): the game's
/// target search from the camera along its forward, within the weapon in hand's aim
/// assist range and `r2wLockOnConeAngle` / `r2wLockOnConeElv` either side, the best of
/// those in clear sight. Characters only. [game] [stand-in: every dummy counts as an
/// enemy that can be targeted; `clear` is the caller's line of sight from the camera]
pub fn snap_target(camera: &Camera, range: f32, targets: &[Target], t: &AimTuning, clear: &dyn Fn(Vec3, Vec3) -> bool) -> Option<u32> {
    let flat = Vec2::new(camera.forward.x, camera.forward.y);
    let query = Query {
        from: camera.eye,
        yaw: heading_of(flat),
        pitch: (-camera.forward.z).atan2(flat.length()),
        range,
        height: SNAP_HEIGHT,
        angle: t.snap_cone.to_radians(),
    };
    let weights = MeleeTuning { best_dist: SNAP_BEST_DIST, angle_weight: SNAP_ANGLE_WEIGHT, dist_weight: SNAP_DIST_WEIGHT, ..Default::default() };
    let mut best: Option<(f32, u32)> = None;
    for target in targets.iter().filter(|target| target.character) {
        let Some(score) = query.score_with_angles(target, &weights,
            t.snap_cone_elevation.to_radians(), SNAP_PITCH_WEIGHT) else { continue };
        if best.is_none_or(|(least, _)| score < least) && clear(camera.eye, assist_point(target)) {
            best = Some((score, target.id));
        }
    }
    best.map(|(_, id)| id)
}

/// The point of a target the assist and the snap look at: its `Head` node, or without
/// one its place raised by three quarters of its height. [game 00709870 l.63-74: Head node 00578440 (hash 0x7c159a2) when present, else the place with 0.75 of character_camera_height 0070abe0]
/// Ported 2026-10-20: `Target::head` (the caller fills it from the posed model's Head node; the arena's
/// scouts in `game/src/targets.rs` animate). A target in car form (+0x31c == 3) takes the no-head point in
/// the game; the port's targets never drive.
pub fn assist_point(target: &Target) -> Vec3 {
    target.head.unwrap_or(target.pos + Vec3::Z * (target.height * 0.75))
}

#[derive(Clone, Copy, Debug)]
pub struct Aim {
    /// The aim's heading (owner `+0x25c`): 0 is +y, positive turns toward -x.
    pub yaw: f32,
    /// How far above level the aim points, radians.
    pub pitch: f32,
    /// How much of the stick's push is let through so far (character `+0x328`).
    ramp: f32,
    /// The share of its speed the aim assist leaves the aim (character `+0x324`): 1, or
    /// on its way to `cameraSpeedReduction` over a target.
    pub slow: f32,
    /// Weapon mode has just started and a target to turn to is still to be looked for
    /// (character flag 0x100000), and the target taken (the handle at `+0x27c`).
    pub snap_wanted: bool,
    pub snap: Option<u32>,
}

impl Default for Aim {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: 0.0, ramp: 0.0, slow: 1.0, snap_wanted: false, snap: None }
    }
}

impl Aim {
    /// Outside weapon mode the aim is handed the body's forward each update
    /// (`0071af5c..0071af62` -> `FUN_00854350`). [game] As weapon mode starts the weapon
    /// state's enter itself puts the aim on the camera's forward (`FUN_0071cad0`). [game]
    pub fn set(&mut self, direction: Vec3) {
        let flat = Vec2::new(direction.x, direction.y);
        if flat.length_squared() > 1e-8 {
            self.yaw = heading_of(flat);
            self.pitch = direction.z.atan2(flat.length());
        }
    }

    /// Weapon mode starts from a state that is neither a weapon state nor the dodge
    /// (`StateWeapon`'s enter `FUN_0087f4a0` -> `FUN_0071cad0`, `FUN_0071ca70`): the aim
    /// is the camera's forward, the assist's share is whole again, the ramp starts at the
    /// camera stick's length as it is, and a target to turn to is wanted. [game]
    pub fn enter(&mut self, camera_forward: Vec3, stick: Vec2) {
        self.set(camera_forward);
        self.slow = 1.0;
        self.ramp = stick.length();
        self.snap_wanted = true;
        self.snap = None;
    }

    /// Weapon mode is over: nothing is wanted or held.
    pub fn leave(&mut self) {
        (self.snap_wanted, self.snap) = (false, None);
    }

    /// One update in weapon mode with no target to turn to and no assist: `step_assisted`
    /// with nothing in the box.
    pub fn step(&mut self, stick: Vec2, t: &AimTuning, dt: f32) {
        self.step_assisted(stick, false, Vec3::ZERO, Assist::default(), t, dt);
    }

    /// One update in weapon mode with no target to turn to (`FUN_0071b990`). `stick` is
    /// the camera stick: x right, y up; `moving` is the other stick being pushed, `body`
    /// where he stands, and `assist` what the assist's test gave.
    ///
    /// - The assist's share goes toward `cameraSpeedReduction` while it says slow, else
    ///   toward 1, by `cameraSpeedAcceleration` a second.
    /// - The push ramps up at the aim acceleration and drops at once; yaw and pitch then
    ///   both turn at `push * share * rate`, shared by the stick's direction.
    /// - The pull: with a target from the assist, the other stick pushed and this one
    ///   under 0.9, the heading goes toward the heading from where he stands to where the
    ///   target stands by `lookAssistTurnRate` (5 degrees a second). The two headings are
    ///   compared as they are, not the short way round. [game]
    ///
    /// Having no target to turn to also takes the "wanted" flag down.
    pub fn step_assisted(&mut self, stick: Vec2, moving: bool, body: Vec3, assist: Assist, t: &AimTuning, dt: f32) {
        self.snap_wanted = false;
        let share = if assist.slow { t.speed_reduction } else { 1.0 };
        let step = t.speed_acceleration * dt;
        self.slow += (share - self.slow).clamp(-step, step);
        let pushed = stick.length_squared() > STICK_STILL;
        if !pushed {
            self.ramp = 0.0;
            if !moving {
                return;
            }
        }
        let length = stick.length();
        if pushed {
            let push = if length > FINE_UP_TO { length.powi(3) } else { length * FINE_SCALE };
            if push > self.ramp {
                self.ramp = (self.ramp + t.acceleration.to_radians() * dt).min(push);
            } else {
                self.ramp = push;
            }
            self.ramp = self.ramp.min(1.0);
        }
        let rate = (t.lowest_sensitivity + (t.highest_sensitivity - t.lowest_sensitivity) * SENSITIVITY).to_radians();
        let turn = self.ramp * self.slow * rate * dt;
        // The signs are the options' "invert" switches, a fresh game's: bits 3 / 4 of the
        // option flags 00dad890 are clear at start (01181b60), so the heading's sign is that of
        // `FUN_00913340` and the pitch's that of `FUN_00913310` with the bit clear [game]; the
        // pitch's sense agrees with the follow camera's in the recording [trace].
        if pushed {
            self.yaw = wrap(self.yaw - stick.x / length * turn);
        }
        if let Some(place) = assist.toward.filter(|_| moving && stick.length_squared() < ASSIST_HARD) {
            let to = heading_of(Vec2::new(place.x - body.x, place.y - body.y));
            let most = t.look_assist_turn_rate.to_radians() * dt;
            if to > self.yaw {
                self.yaw = wrap((self.yaw + most).min(to));
            } else if to < self.yaw {
                self.yaw = wrap((self.yaw - most).max(to));
            }
        }
        if pushed {
            self.pitch = (self.pitch + stick.y / length * turn).clamp(t.min_pitch.to_radians(), t.max_pitch.to_radians());
        }
    }

    /// One update while sniping (`FUN_0071b990` with `camera_is_sniping`): the aim assist is
    /// not asked, so its share goes back toward 1; the push is the stick's length as it is (no
    /// fine-aim scale or cube); the ramp rises at `aimSnipingAcceleration`; the rate is between
    /// the two `aimSniping*Sensitivity` by the option, times `snipe::zoom_rate_share` of the
    /// view's field of view `fov` (degrees); the pitch is kept within the sniping limits. There
    /// is no pull toward a target. [game]
    pub fn step_sniping(&mut self, stick: Vec2, fov: f32, t: &AimTuning, dt: f32) {
        self.snap_wanted = false;
        let step = t.speed_acceleration * dt;
        self.slow += (1.0 - self.slow).clamp(-step, step);
        let s = &t.sniping;
        let pushed = stick.length_squared() > STICK_STILL;
        if !pushed {
            self.ramp = 0.0;
            return;
        }
        let length = stick.length();
        if length > self.ramp {
            self.ramp = (self.ramp + s.acceleration.to_radians() * dt).min(length);
        } else {
            self.ramp = length;
        }
        self.ramp = self.ramp.min(1.0);
        let rate = (s.lowest_sensitivity + (s.highest_sensitivity - s.lowest_sensitivity) * SENSITIVITY).to_radians()
            * crate::snipe::zoom_rate_share(fov, s.full_zoom_share);
        let turn = self.ramp * self.slow * rate * dt;
        self.yaw = wrap(self.yaw - stick.x / length * turn);
        self.pitch = (self.pitch + stick.y / length * turn).clamp(s.min_pitch.to_radians(), s.max_pitch.to_radians());
    }

    /// The modern layout's mouse turns the aim by an angle (`input::mouse_turn`; x right, y
    /// up, radians), with no ramp, curve or assist slow-down, kept in `min_pitch..max_pitch`
    /// (degrees). [port] Not the original's: its mouse is a stick through `step_assisted`.
    pub fn turn(&mut self, turn: Vec2, min_pitch: f32, max_pitch: f32) {
        self.yaw = wrap(self.yaw - turn.x);
        self.pitch = (self.pitch + turn.y).clamp(min_pitch.to_radians(), max_pitch.to_radians());
    }

    /// One update in weapon mode with a target to turn to (`FUN_0071b990`'s other branch):
    /// the stick is not read. `point` is the target's (`assist_point`), `eye` the
    /// camera's place and `body` his own. Once the aim is within a hair of the way from
    /// the camera to the point, the target is let go (and the aim still makes this
    /// update's turn). The heading turns toward the heading of the way from the camera to
    /// the point (its height taken from where he stands, not from the camera), the pitch
    /// toward the pitch of the way from the camera, each by no more than 270 degrees a
    /// second. No pitch limit is applied here. [game]
    pub fn step_snap(&mut self, point: Vec3, eye: Vec3, body: Vec3, dt: f32) {
        let way = Vec3::new(point.x - eye.x, point.y - eye.y, point.z - body.z).normalize_or_zero();
        let yaw_to = heading_of(Vec2::new(way.x, way.y));
        let line = (point - eye).normalize_or_zero();
        if self.snap_wanted && self.direction().dot(line) > SNAP_DONE {
            (self.snap_wanted, self.snap) = (false, None);
        }
        let pitch_to = line.z.atan2(Vec2::new(line.x, line.y).length());
        let most = SNAP_RATE * dt;
        self.yaw = wrap(self.yaw + wrap(yaw_to - self.yaw).clamp(-most, most));
        self.pitch = wrap(self.pitch + wrap(pitch_to - self.pitch).clamp(-most, most));
    }

    /// What the aim hands the animation at the end of each weapon-mode update
    /// (`FUN_0071b990`, its last lines): which of the playing clip's offset clips is laid
    /// on (0 `Down`, 1 `Up`; the `MetaAdditiveAnimation`'s slot `+0x20`) and how much of
    /// it, 0 to 1 (slot `+0x1c`): the aim's rise over 65 degrees, no further. [game]
    pub fn offset(&self) -> (usize, f32) {
        let rise = self.pitch.clamp(-OFFSET_MOST, OFFSET_MOST);
        if rise > 0.0 { (1, rise / OFFSET_MOST) } else { (0, -rise / OFFSET_MOST) }
    }

    /// The aim as a unit vector.
    pub fn direction(&self) -> Vec3 {
        Vec3::new(-self.yaw.sin() * self.pitch.cos(), self.yaw.cos() * self.pitch.cos(), self.pitch.sin())
    }
}

/// The aim layer of a character no player controls, set each update of a weapon state
/// (`StateWeapon`'s update `FUN_0087f800` -> `FUN_0087f900`). [game 0087f900]
///
/// The line D runs to the weapon manager's aim point (`+0x7c..+0x84`) from a point that
/// takes its x and y from the character's camera target (`FUN_0071a2e0`) and its z from
/// the weapon's slot `+0x90` position (disassembly `0087f96a..0087fa08`). Its yaw
/// (`atan2(-x, y)`) and dip (`atan2(-z, |xy|)`) are measured against the aim heading's
/// (`+0x260`); the layer (the animation controller's `+0x100`, type 1) gets the turn
/// (`+0x14`) and the rise (`+0x18`), each wrapped the game's way and kept at the layer's
/// current value (`+0x1c`, `+0x20`) when within 0.05 of it. Returns (turn, rise).
pub fn npc_aim_angles(aim_point: Vec3, camera_target: Vec3, weapon_point: Vec3, heading: Vec3, current: (f32, f32)) -> (f32, f32) {
    let from = Vec3::new(camera_target.x, camera_target.y, weapon_point.z);
    let d = aim_point - from;
    let d = if d.length() > 1e-8 { d / d.length() } else { d };
    let yaw = |v: Vec3| (-v.x).atan2(v.y);
    let dip = |v: Vec3| (-v.z).atan2(v.truncate().length());
    let turn = game_wrap(yaw(d) - yaw(heading));
    let rise = game_wrap(dip(heading) - dip(d));
    let turn = if (turn - current.0).abs() < 0.05 { current.0 } else { turn };
    let rise = if (rise - current.1).abs() < 0.05 { current.1 } else { rise };
    (turn, rise)
}

/// The game's angle wrap: `((a / 2pi + 0.5) - floor(a / 2pi + 0.5) - 0.5) * 2pi`, into
/// [-pi, pi). [game 0087fb30..]
fn game_wrap(a: f32) -> f32 {
    let x = a * 0.159_154_94 + 0.5;
    (x - x.floor() - 0.5) * std::f32::consts::TAU
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 0.032;

    #[test]
    fn an_npc_aims_its_layer_at_the_managers_point_from_the_guns_height() {
        let heading = Vec3::Y;
        // A point 10 ahead, 10 to the left, and level with the gun: a turn of 45 degrees
        // to the left and no rise, whatever the camera target's own height.
        let (turn, rise) = npc_aim_angles(Vec3::new(-10.0, 10.0, 2.0), Vec3::new(0.0, 0.0, 9.0), Vec3::new(0.3, 0.2, 2.0), heading, (0.0, 0.0));
        assert!((turn - std::f32::consts::FRAC_PI_4).abs() < 1e-5, "{turn}");
        assert!(rise.abs() < 1e-6, "{rise}");
        // Above the gun: the rise is positive; within 0.05 of the current one it holds.
        let (_, up) = npc_aim_angles(Vec3::new(0.0, 10.0, 12.0), Vec3::ZERO, Vec3::new(0.0, 0.0, 2.0), heading, (0.0, 0.0));
        assert!((up - std::f32::consts::FRAC_PI_4).abs() < 1e-5, "{up}");
        let (t, r) = npc_aim_angles(Vec3::new(-10.0, 10.0, 2.0), Vec3::ZERO, Vec3::new(0.0, 0.0, 2.0), heading, (0.76, 0.03));
        assert_eq!((t, r), (0.76, 0.03));
        // Behind: wrapped into [-pi, pi).
        let (back, _) = npc_aim_angles(Vec3::new(0.0, -10.0, 2.0), Vec3::ZERO, Vec3::new(0.0, 0.0, 2.0), heading, (0.0, 0.0));
        assert!((back + std::f32::consts::PI).abs() < 1e-5, "{back}");
    }

    /// `AimAssist` as the game's data has it.
    fn tuning() -> AimTuning {
        AimTuning {
            lowest_sensitivity: 85.0,
            highest_sensitivity: 360.0,
            acceleration: 144.0,
            min_pitch: -85.0,
            max_pitch: 85.0,
            speed_reduction: 0.75,
            speed_acceleration: 4.0,
            look_assist_turn_rate: 5.0,
            target_box: [0.025, 0.0444],
            default_max_range: 100.0,
            snap_cone: 30.0,
            snap_cone_elevation: 30.0,
            sniping: crate::snipe::SnipeAim {
                lowest_sensitivity: 18.0,
                highest_sensitivity: 36.0,
                acceleration: 350.0,
                min_pitch: -80.0,
                max_pitch: 80.0,
                full_zoom_share: 0.19,
            },
        }
    }

    #[test]
    fn sniping_turns_at_its_own_rate_scaled_by_the_zoom_with_no_fine_curve() {
        let t = tuning();
        // 350 degrees a second is 6.1 radians: 0.195 an update, so a half push (not scaled down
        // to 0.156 as out of sniping) is let through on the third.
        let mut aim = Aim { slow: 0.75, ..Aim::default() };
        aim.step_sniping(Vec2::new(0.5, 0.0), 15.0, &t, DT);
        assert!((aim.ramp - 350f32.to_radians() * DT).abs() < 1e-6, "{}", aim.ramp);
        // The assist's share went toward 1 (4 a second).
        assert!((aim.slow - (0.75 + 4.0 * DT)).abs() < 1e-6);
        aim.step_sniping(Vec2::new(0.5, 0.0), 15.0, &t, DT);
        aim.step_sniping(Vec2::new(0.5, 0.0), 15.0, &t, DT);
        assert!((aim.ramp - 0.5).abs() < 1e-6, "{}", aim.ramp);
        let yaw = aim.yaw;
        aim.slow = 1.0;
        aim.step_sniping(Vec2::new(0.5, 0.0), 15.0, &t, DT);
        // 27 degrees a second (18 to 36 at the option's 0.5), times the push, at no zoom.
        let turn = 0.5 * 27f32.to_radians() * DT;
        assert!((wrap(yaw - aim.yaw) - turn).abs() < 1e-6);
        // At full zoom, 0.19 of that.
        let yaw = aim.yaw;
        aim.step_sniping(Vec2::new(0.5, 0.0), 5.0, &t, DT);
        assert!((wrap(yaw - aim.yaw) - turn * 0.19).abs() < 1e-6);
        // Its pitch limit is 80 degrees.
        for _ in 0..2000 {
            aim.step_sniping(Vec2::Y, 15.0, &t, DT);
        }
        assert!((aim.pitch - 80f32.to_radians()).abs() < 1e-5);
    }

    #[test]
    fn a_full_push_ramps_up_and_then_turns_at_the_rate_between_the_two_sensitivities() {
        let t = tuning();
        let mut aim = Aim::default();
        // 144 degrees is 2.513 radians: a full push is let through after 0.4 s.
        aim.step(Vec2::X, &t, DT);
        assert!((aim.ramp - 2.513_27 * DT).abs() < 1e-4, "{}", aim.ramp);
        for _ in 0..13 {
            aim.step(Vec2::X, &t, DT);
        }
        assert_eq!(aim.ramp, 1.0);
        let before = aim.yaw;
        aim.step(Vec2::X, &t, DT);
        // Half way between 85 and 360 degrees a second, to the right.
        assert!((wrap(before - aim.yaw) - 222.5f32.to_radians() * DT).abs() < 1e-5);
        assert_eq!(aim.pitch, 0.0);
    }

    #[test]
    fn a_light_push_is_scaled_down_and_letting_go_drops_the_ramp() {
        let t = tuning();
        let mut aim = Aim::default();
        for _ in 0..20 {
            aim.step(Vec2::new(0.0, 0.5), &t, DT);
        }
        assert!((aim.ramp - 0.5 * 0.3125).abs() < 1e-6);
        // Just past the border the push is the length cubed.
        aim.step(Vec2::new(0.0, 0.81), &t, DT);
        assert!(aim.ramp > 0.5 * 0.3125 && aim.ramp <= 0.81f32.powi(3));
        aim.step(Vec2::ZERO, &t, DT);
        assert_eq!(aim.ramp, 0.0);
        assert!(aim.pitch > 0.0, "the stick up aims up");
    }

    #[test]
    fn the_aims_rise_picks_the_offset_clip_and_its_share() {
        let mut aim = Aim::default();
        assert_eq!(aim.offset(), (0, 0.0));
        aim.set(Vec3::new(0.0, 1.0, 1.0));
        let (side, amount) = aim.offset();
        assert_eq!(side, 1, "aiming up lays on the second clip, `Up`");
        assert!((amount - 45.0 / 65.0).abs() < 1e-4, "{amount}");
        aim.set(Vec3::new(0.0, 1.0, -0.5));
        let (side, amount) = aim.offset();
        assert_eq!(side, 0);
        assert!((amount - 0.5f32.atan().to_degrees() / 65.0).abs() < 1e-4, "{amount}");
        // Past 65 degrees the clip is on in full and no more.
        aim.pitch = 85f32.to_radians();
        assert_eq!(aim.offset(), (1, 1.0));
    }

    #[test]
    fn the_pitch_stops_at_the_data_limits_and_the_vector_round_trips() {
        let t = tuning();
        let mut aim = Aim::default();
        for _ in 0..200 {
            aim.step(Vec2::Y, &t, DT);
        }
        assert!((aim.pitch - 85f32.to_radians()).abs() < 1e-6);
        let mut other = Aim::default();
        other.set(Vec3::new(-0.6, 0.0, 0.8));
        assert!((other.direction() - Vec3::new(-0.6, 0.0, 0.8)).length() < 1e-5);
        assert!((other.yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }

    fn camera() -> Camera {
        Camera { eye: Vec3::new(0.0, -10.0, 5.0), right: Vec3::X, forward: Vec3::Y, up: Vec3::Z }
    }

    fn target(off_centre: f32) -> AssistTarget {
        AssistTarget { point: Vec3::new(2.0, 40.0, 5.0), place: Vec3::new(2.0, 40.0, 0.0), velocity: Vec3::ZERO, off_centre }
    }

    #[test]
    fn the_assist_slows_the_aim_over_a_target_and_lets_a_hard_push_away_go() {
        let (t, cam) = (tuning(), camera());
        // Nothing in the box: nothing.
        assert_eq!(assist(Vec2::X, &cam, Vec3::ZERO, None, &t, DT), Assist::default());
        // The stick at rest, a light push, and a hard push with the target near the
        // middle: slow, with the target.
        let near = target(100.0);
        for stick in [Vec2::ZERO, Vec2::new(0.5, 0.0), Vec2::new(-1.0, 0.0)] {
            let got = assist(stick, &cam, Vec3::ZERO, Some(&near), &t, DT);
            assert_eq!((got.slow, got.toward), (true, Some(near.place)), "{stick}");
        }
        // The target is to the right of the middle and far from it: a hard push to the
        // left (away from it) gets out; one to the right (onto it) is slowed.
        let far = target(900.0);
        assert_eq!(assist(Vec2::new(-1.0, 0.0), &cam, Vec3::ZERO, Some(&far), &t, DT), Assist::default());
        assert!(assist(Vec2::new(1.0, 0.0), &cam, Vec3::ZERO, Some(&far), &t, DT).slow);
    }

    #[test]
    fn the_assist_does_not_slow_an_aim_that_is_following_a_faster_target() {
        let (t, cam) = (tuning(), camera());
        // The reduced turn is 0.75 * 222.5 degrees a second: 2.91 radians a second. A
        // target 50 away crossing to the right at 200 changes its bearing by 4 a second.
        let mut crossing = target(100.0);
        crossing.velocity = Vec3::new(200.0, 0.0, 0.0);
        let got = assist(Vec2::new(0.5, 0.0), &cam, Vec3::ZERO, Some(&crossing), &t, DT);
        assert_eq!((got.slow, got.toward.is_some()), (false, true));
        // Pushed the other way it is slowed, and so is a target that crosses slowly.
        assert!(assist(Vec2::new(-0.5, 0.0), &cam, Vec3::ZERO, Some(&crossing), &t, DT).slow);
        crossing.velocity = Vec3::new(20.0, 0.0, 0.0);
        assert!(assist(Vec2::new(0.5, 0.0), &cam, Vec3::ZERO, Some(&crossing), &t, DT).slow);
        // His own motion counts: moving with the target, its bearing does not change.
        crossing.velocity = Vec3::new(200.0, 0.0, 0.0);
        assert!(assist(Vec2::new(0.5, 0.0), &cam, Vec3::new(200.0, 0.0, 0.0), Some(&crossing), &t, DT).slow);
    }

    #[test]
    fn the_slowed_aim_turns_at_three_quarters_and_the_pull_drifts_to_the_target() {
        let t = tuning();
        let mut aim = Aim::default();
        let slow = Assist { slow: true, toward: None };
        // The share falls by 4 a second: 0.25 takes two updates of 0.032.
        aim.step_assisted(Vec2::X, false, Vec3::ZERO, slow, &t, DT);
        assert!((aim.slow - (1.0 - 4.0 * DT)).abs() < 1e-6);
        for _ in 0..20 {
            aim.step_assisted(Vec2::X, false, Vec3::ZERO, slow, &t, DT);
        }
        assert!((aim.slow - 0.75).abs() < 1e-6 && aim.ramp == 1.0);
        let before = aim.yaw;
        aim.step_assisted(Vec2::X, false, Vec3::ZERO, slow, &t, DT);
        assert!((wrap(before - aim.yaw) - 0.75 * 222.5f32.to_radians() * DT).abs() < 1e-5);
        // And comes back when the assist lets go.
        for _ in 0..3 {
            aim.step_assisted(Vec2::X, false, Vec3::ZERO, Assist::default(), &t, DT);
        }
        assert!((aim.slow - 1.0).abs() < 1e-6);

        // The pull: the aim stick at rest and the other pushed, the heading goes toward
        // the target's at 5 degrees a second; with the other stick at rest, nothing.
        let mut aim = Aim::default();
        let pull = Assist { slow: true, toward: Some(Vec3::new(-10.0, 10.0, 0.0)) };
        aim.step_assisted(Vec2::ZERO, false, Vec3::ZERO, pull, &t, DT);
        assert_eq!(aim.yaw, 0.0);
        aim.step_assisted(Vec2::ZERO, true, Vec3::ZERO, pull, &t, DT);
        assert!((aim.yaw - 5f32.to_radians() * DT).abs() < 1e-6, "{}", aim.yaw);
        // It stops at the target's heading (45 degrees to the left).
        for _ in 0..400 {
            aim.step_assisted(Vec2::ZERO, true, Vec3::ZERO, pull, &t, DT);
        }
        assert!((aim.yaw - 45f32.to_radians()).abs() < 1e-5, "{}", aim.yaw);
        // A hard push of the aim stick is not pulled.
        let mut aim = Aim::default();
        aim.step_assisted(Vec2::new(0.0, 1.0), true, Vec3::ZERO, pull, &t, DT);
        assert_eq!(aim.yaw, 0.0);
    }

    fn dummy(id: u32, at: Vec3) -> Target {
        Target { id, pos: at, radius: 1.0, height: 6.0, character: true, large: false, surface: 0, dashing: false, health: f32::MAX, head: None }
    }

    /// 00709870 l.62-74: the Head node's place when the target has one, else 0.75 of its height. [game]
    #[test]
    fn the_assist_point_is_the_head_when_there_is_one() {
        let mut t = dummy(1, Vec3::new(3.0, 4.0, 0.0));
        assert_eq!(assist_point(&t), Vec3::new(3.0, 4.0, 4.5));
        t.head = Some(Vec3::new(3.1, 4.2, 5.6));
        assert_eq!(assist_point(&t), Vec3::new(3.1, 4.2, 5.6));
    }

    #[test]
    fn taking_aim_scores_pitch_separately_from_yaw_and_keeps_both_cone_limits() {
        let mut t = tuning();
        let cam = Camera { eye: Vec3::ZERO, right: Vec3::X, forward: Vec3::Y, up: Vec3::Z };
        let a = 15f32.to_radians();
        let targets = [dummy(1, Vec3::new(10.0 * a.sin(), 10.0 * a.cos(), 0.0)),
            dummy(2, Vec3::new(0.0, 10.0, 10.0 * a.tan()))];
        assert_eq!(snap_target(&cam, 300.0, &targets, &t, &|_,_| true), Some(2),
            "same angular offset costs less in pitch than yaw");
        t.snap_cone_elevation = 10.0;
        assert_eq!(snap_target(&cam, 300.0, &targets, &t, &|_,_| true), Some(1),
            "narrower elevation must not narrow the yaw cone");
    }

    #[test]
    fn taking_aim_turns_to_a_target_in_the_cone_and_then_lets_it_go() {
        let t = tuning();
        let cam = Camera { eye: Vec3::new(0.0, -10.0, 4.5), right: Vec3::X, forward: Vec3::Y, up: Vec3::Z };
        // One 20 degrees to the left of the camera's line, one 50 degrees to the right
        // (outside the cone), one in the cone but out of the range.
        let targets = [
            dummy(1, Vec3::new(-50.0 * 20f32.to_radians().tan(), 40.0, 0.0)),
            dummy(2, Vec3::new(50.0 * 50f32.to_radians().tan(), 40.0, 0.0)),
            dummy(3, Vec3::new(0.0, 400.0, 0.0)),
        ];
        assert_eq!(snap_target(&cam, 300.0, &targets, &t, &|_, _| true), Some(1));
        assert_eq!(snap_target(&cam, 300.0, &targets, &t, &|_, _| false), None, "hidden");
        assert_eq!(snap_target(&cam, 300.0, &targets[1..], &t, &|_, _| true), None);

        let mut aim = Aim::default();
        aim.enter(Vec3::Y, Vec2::new(0.6, 0.0));
        assert!(aim.snap_wanted && aim.ramp == 0.6 && aim.slow == 1.0);
        aim.snap = Some(1);
        let point = assist_point(&targets[0]);
        // 270 degrees a second: 8.64 an update; 20 degrees takes three.
        aim.step_snap(point, cam.eye, Vec3::ZERO, DT);
        assert!((aim.yaw - SNAP_RATE * DT).abs() < 1e-6);
        let mut updates = 1;
        while aim.snap.is_some() && updates < 50 {
            aim.step_snap(point, cam.eye, Vec3::ZERO, DT);
            updates += 1;
        }
        assert_eq!(updates, 4, "three to get there, one more to see it has");
        assert!(aim.direction().dot((point - cam.eye).normalize()) > 0.9999);
        assert!(!aim.snap_wanted);
        // With nothing to turn to, the stick's update takes the flag down.
        let mut aim = Aim::default();
        aim.enter(Vec3::Y, Vec2::ZERO);
        aim.step(Vec2::ZERO, &t, DT);
        assert!(!aim.snap_wanted);
    }
}
