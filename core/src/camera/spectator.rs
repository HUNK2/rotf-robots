//! `CameraSpectator`: a level component that hands the game camera to an automatic "spectator" director which
//! frames a target with a run of generated shots (placed in 5 levels). [game] `notes\camera.md`
//! "CameraSpectator".
//!
//! Component: registrar 00ae25e0 (factory 0091c660), constructor 0091c080, loader 0091c1f0, vtable 00b65ed8
//! (slot 3 message 0091cda0, slot 6 Trigger 0091c6d0, slot 7 Untrigger 0091cd80, end 0091cac0; ring c4b725fd
//! retargets, 0091cdc0). Behaviour: constructor 00919510, vtable 00b65f00, key 0xd63697a6, priority 8; slot 0
//! picks a new shot (00919de0), slot 3 the update (0091b770), slot 4 the aim (0091bc60); helpers 00919b70 (the
//! point looked at) and 00919800 (how far a line is clear).
use glam::{Mat3, Vec3};

/// The behaviour's key; also the key the Trigger looks for to refuse a second spectator. [game]
pub const KEY: u32 = 0xd636_97a6;
/// The key of another cinematic behaviour the Trigger removes before it starts. [game] which class: [unknown].
pub const OTHER_CINEMATIC_KEY: u32 = 0x768c_e890;
/// The message the Trigger schedules after `timeout` and that ends the spectator (slot 3). [game]
pub const TIMEOUT_MESSAGE: u32 = 0x7437_ef48;
/// The field of view each shot is framed for (written at the start of every shot), degrees. [game]
pub const SHOT_FOV: f32 = 50.0;

/// The loader's fields and defaults (0091c1f0). `gameSpeed` is stored times 0.01. [game]
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub disable_player: bool,
    pub hide_player: bool,
    pub towards_this: bool,
    pub pan_up: bool,
    pub rotate_around: bool,
    pub ignore_radius: f32,
    pub target_height: f32,
    pub timeout: f32,
    pub cut_time_min: f32,
    pub cut_time_max: f32,
    pub cam_ahead_dist: f32,
    pub cut_move_dist: f32,
    pub game_speed: f32,
    pub max_ground_height: f32,
    pub min_target_height: f32,
    pub max_target_height: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            disable_player: false,
            hide_player: false,
            towards_this: false,
            pan_up: false,
            rotate_around: false,
            ignore_radius: 0.0,
            target_height: 0.0,
            timeout: -1.0,
            cut_time_min: -1.0,
            cut_time_max: -1.0,
            cam_ahead_dist: 10.0,
            cut_move_dist: -1.0,
            game_speed: 1.0,
            max_ground_height: 1000.0,
            min_target_height: -1000.0,
            max_target_height: 1000.0,
        }
    }
}

/// What the Trigger asks of the world (0091c6d0), in order. [game]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Start {
    /// Push the behaviour (its first shot is picked as it starts).
    CameraOn,
    /// `gameSpeed` outside 0.99..1.01: the game speed request under the key `CameraSpectator`, with 1.5 passed
    /// beside it (007504b0 / 007581f0). [assumed] 1.5 is a blend time in seconds.
    GameSpeed(f32),
    /// `disablePlayer` without `hidePlayer`: the player's input lock level raised to at least 2.
    LockPlayer,
    /// `hidePlayer`: the player hidden (00721590(player, 1)).
    HidePlayer,
    /// `timeout` above 1e-7: [`TIMEOUT_MESSAGE`] to itself after that many seconds.
    EndAfter(f32),
}

/// What the Trigger does, given whether a spectator or the other cinematic behaviour runs already.
/// A running spectator makes it do nothing; the other one is removed first. [game] 0091c6d0.
pub fn start(s: &Settings, spectator_running: bool) -> Vec<Start> {
    if spectator_running {
        return Vec::new();
    }
    let mut out = vec![Start::CameraOn];
    if s.game_speed < 0.99 || 1.01 < s.game_speed {
        out.push(Start::GameSpeed(s.game_speed));
    }
    if s.hide_player {
        out.push(Start::HidePlayer);
    } else if s.disable_player {
        out.push(Start::LockPlayer);
    }
    if 1e-7 < s.timeout {
        out.push(Start::EndAfter(s.timeout));
    }
    out
}

/// What the director looks at: the target object (0x7c / retargeted at 0x80). [game] 00919b70.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// No live target: the camera's own position, height 2. [game]
    None { camera_pos: Vec3 },
    /// A character: its camera height, the flyer-aware bottom point (0071a460), and its STEP record's reach
    /// (`+0x170[+0x244]`, 0 without a record). [game]
    Character { bottom: Vec3, height: f32, reach: f32 },
    /// Any other object: its position; the height is `targetHeight` when above 0.01, else 5. [game]
    Object { pos: Vec3 },
}

/// The point looked at, the target's height and its radius: the middle of bottom and top, plus the drifting
/// look offset. Radius 0.25 unless a character's reach replaces it. [game] 00919b70.
pub fn look_point(target: Target, target_height: f32, look: Vec3) -> (Vec3, f32, f32) {
    let (bottom, height, radius) = match target {
        Target::None { camera_pos } => return (look + camera_pos, 2.0, 0.25),
        Target::Character { bottom, height, reach } => (bottom, height, reach),
        Target::Object { pos } => (pos, if target_height > 0.01 { target_height } else { 5.0 }, 0.25),
    };
    let top = Vec3::new(bottom.x, bottom.y, bottom.z + height);
    (look + (bottom + top) * 0.5, height, radius)
}

/// The world as the director sees it.
pub trait World {
    /// The distance from `from` along the segment to `to` of the nearest BLOCKING object (collision flag bit 5
    /// of `+0x9c`), a thin ray (radius 0, mask 0x20). [game] 005be890 as 00919800 calls it.
    fn first_block(&self, from: Vec3, to: Vec3) -> Option<f32>;
    /// Whether a sphere of `radius` swept from `from` to `to` (mask 0xffff) meets a blocking object. Objects
    /// that only fade are not blocking (they are faded, 0091ecb0: [stand-in] not done by this port).
    fn sphere_blocked(&self, from: Vec3, to: Vec3, radius: f32) -> bool;
}

/// How far the line from `from` to `to` is clear (00919800). With `skip` above 0.01 the line starts that far
/// along (a target shorter than `skip` returns its own length); a block returns its distance plus `skip`,
/// no block the length from the moved start. [game]
pub fn clear(world: &dyn World, from: Vec3, to: Vec3, skip: f32) -> f32 {
    let mut start = from;
    if skip > 0.01 {
        let d = to - from;
        let len = d.length();
        if len < skip {
            return len;
        }
        start = from + d * (skip / len);
    }
    match world.first_block(start, to) {
        Some(hit) => hit + skip,
        None => (to - start).length(),
    }
}

/// The game's shared random numbers (00551d60 `between`, 0091d090 `coin`): the C library's LCG mixed with the
/// clock's seconds. [game] [stand-in] a stream of the spectator's own, as `shake.rs` does: the original shares
/// the LCG with other systems.
#[derive(Debug, Clone)]
pub struct Random {
    pub state: u32,
}

impl Random {
    fn step(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(0x343fd).wrapping_add(0x26_9ec3);
        self.state
    }
    pub fn between(&mut self, low: f32, high: f32, seconds: u32) -> f32 {
        let s = self.step();
        let share = f32::from_bits(((s >> 8) ^ seconds.wrapping_shl(8)) & 0x7f_ff00 | 0x3f80_0000) - 1.0;
        low + (high - low) * share
    }
    pub fn coin(&mut self, seconds: u32) -> bool {
        (self.step() >> 16 ^ seconds) & 1 != 0
    }
}

/// What a new shot needs besides the target. [game] the sources are named on each field.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShotInputs {
    /// The target object's position (+0x34) and forward row (+0x1c).
    pub target_pos: Vec3,
    pub target_forward: Vec3,
    /// The velocity of the target's physics body, when it has one (the dd566c80 child with a body).
    pub body_velocity: Option<Vec3>,
    /// The position of the object the component sits on (behaviour +0x20), when it lives (rotateAround).
    pub own_position: Option<Vec3>,
    /// `towardsThis`: the component's position, kept at the Trigger (behaviour +0x2c).
    pub towards: Option<Vec3>,
    /// panUp: the vector the camera's parent behaviour returns from slot 7 and its field of view, when the
    /// parent chain has a behaviour other than key 0xfc36eb23. [game] [unknown] what that vector means.
    pub parent: Option<(Vec3, f32)>,
    /// panUp: the target character's answer to the camera target message (ca291e26): the sum of its two
    /// height fields when its flag bit 0 is set. [game]
    pub message_height: Option<f32>,
    /// Clock seconds for the random numbers.
    pub seconds: u32,
}

/// The behaviour (0x19 pool object). [game] offsets in brackets.
#[derive(Debug, Clone)]
pub struct Spectator {
    pub settings: Settings,
    pub eye: Vec3,          // +0x38
    pub eye_velocity: Vec3, // +0x44
    pub look: Vec3,         // +0x6c, offset added to the target's middle
    pub look_velocity: Vec3,
    pub fov: f32,        // +0x14
    pub fov_rate: f32,   // +0xc4
    pub yaw_rate: f32,   // +0xc8, radians per second round the target
    pub shot_fov: f32,   // +0x28
    pub time: f32,       // +0xa4
    pub travel: f32,     // +0xb0, how far the target moved this shot
    pub previous: Vec3,  // +0x60
    pub shot_timer: f32, // +0xa0
    pub side: bool,      // +0x84
    /// +0x58 / +0x5c: pitch and heading from the camera to the point looked at, kept at each cut (0091b680).
    pub angles: (f32, f32),
    /// The camera's rows (right, forward, up) after the last aim.
    pub rows: [Vec3; 3],
    pub random: Random,
}

impl Spectator {
    /// 00919510 then the Trigger's copies. The side starts random (`(lcg byte 2 ^ time) & 1`).
    /// The behaviour's timeout (+0xc0): the component's when above 1e-6, else the constructor's 1. [game]
    fn timeout(&self) -> f32 {
        if self.settings.timeout > 1e-6 { self.settings.timeout } else { 1.0 }
    }

    /// The behaviour's cut-time window (+0x98, +0x9c): copied only when cutTimeMax is above 0, else the
    /// constructor's -1, -1. [game]
    fn cut_window(&self) -> (f32, f32) {
        if self.settings.cut_time_max > 0.0 { (self.settings.cut_time_min, self.settings.cut_time_max) } else { (-1.0, -1.0) }
    }

    pub fn new(settings: Settings, random: Random, seconds: u32) -> Self {
        let mut random = random;
        let side = (random.step() >> 16) as u8 ^ seconds as u8;
        Self {
            settings,
            eye: Vec3::ZERO,
            eye_velocity: Vec3::ZERO,
            look: Vec3::ZERO,
            look_velocity: Vec3::ZERO,
            fov: 60.0,
            fov_rate: 0.0,
            yaw_rate: 0.0,
            shot_fov: 60.0,
            time: 0.0,
            travel: 0.0,
            previous: Vec3::ZERO,
            shot_timer: -1.0,
            side: side & 1 != 0,
            angles: (0.0, 0.0),
            rows: [Vec3::X, Vec3::Y, Vec3::Z],
            random,
        }
    }

    /// Slot 0 (00919de0): a new shot. Returns false (no shot) without a live target, as the original returns
    /// early. `target` is None then.
    pub fn new_shot(&mut self, target: Option<Target>, inputs: &ShotInputs, world: &dyn World) -> bool {
        let Some(target) = target else { return false };
        let s = self.settings.clone();
        self.shot_fov = SHOT_FOV;
        self.look = Vec3::ZERO;
        let (centre, height, radius) = look_point(target, s.target_height, self.look);

        // The direction the shot is built round: the target's forward; its body's level velocity when faster
        // than 1.5; towards the component when `towardsThis` and more than 1 away. [game]
        let mut dir = normalise(inputs.target_forward);
        if let Some(v) = inputs.body_velocity {
            let level = (v.x * v.x + v.y * v.y).sqrt();
            if level > 1.5 {
                dir = if level > 1e-8 { Vec3::new(v.x / level, v.y / level, 0.0) } else { Vec3::new(v.x, v.y, 0.0) };
            }
        }
        if let Some(t) = inputs.towards {
            let d = t - centre;
            let len = d.length();
            if len > 1.0 {
                dir = if len > 1e-8 { d / len } else { d };
            }
        }
        let side_dir = normalise(Vec3::new(-dir.y, dir.x, 0.0));

        if s.pan_up {
            self.pan_up_shot(centre, dir, height, radius, inputs, world);
        } else if s.rotate_around {
            self.rotate_around_shot(centre, height, inputs, world);
        } else {
            self.plain_shot(centre, dir, side_dir, height, radius, inputs, world);
        }
        self.finish_cut(target, centre);
        true
    }

    /// The rotateAround shot (0091a2f9..0091a63d). The eye is the component's object if it lives, else the
    /// target plus random (-10..10, -10..10, 7.5). The original then probes from the target to a point one
    /// metre on the far side of it (`eye - dir * (distance + 1)`), which leaves the eye where it is in every
    /// case; kept as found. A random turn of -25..25 degrees per second round the target; no timed cut.
    fn rotate_around_shot(&mut self, centre: Vec3, _height: f32, inputs: &ShotInputs, world: &dyn World) {
        let sec = inputs.seconds;
        let rx = self.random.between(-10.0, 10.0, sec);
        let ry = self.random.between(-10.0, 10.0, sec);
        let mut eye = Vec3::new(centre.x + rx, centre.y + ry, centre.z + 7.5);
        if let Some(p) = inputs.own_position {
            eye = p;
        }
        let d = eye - centre;
        let dist = d.length();
        let dn = normalise(d);
        let probe = clear(world, centre, eye - dn * (dist + 1.0), self.settings.ignore_radius);
        let back = (probe - 1.0).max(0.0);
        self.shot_timer = f32::MAX;
        self.look_velocity = Vec3::ZERO;
        self.eye_velocity = Vec3::ZERO;
        let limit = f32::from_bits(0x3edf_66f3); // 0.436332, 25 degrees [game]
        self.yaw_rate = self.random.between(-limit, limit, sec);
        self.eye = eye - dn * back;
    }

    /// The panUp shot (0091a642..0091aa0f): from beside the target, low, rising for `timeout` while the view
    /// widens from half its field to all of it.
    fn pan_up_shot(&mut self, centre: Vec3, dir: Vec3, height: f32, radius: f32, inputs: &ShotInputs, world: &dyn World) {
        let mut fov_start = 60.0;
        let mut ahead = radius.max(1.75) * 2.0;
        let mut extra = height * 0.25;
        let mut rise = height;
        if let Some((v, fov)) = inputs.parent {
            ahead = v.y;
            extra = v.z;
            fov_start = fov;
            if let Some(h) = inputs.message_height {
                rise = h;
            }
        }
        let out = clear(world, centre, centre + dir * (ahead + 1.0), self.settings.ignore_radius);
        let along = (out - 1.0).max(0.0);
        let beside = centre + dir * along;
        let down = clear(world, beside, beside - Vec3::new(0.0, 0.0, height), 0.0);
        let low = beside.z - (down - 1.5);
        let timeout = self.timeout();
        let speed = if timeout > 1e-4 {
            (extra + rise) / timeout
        } else {
            self.random.between(1.0, 5.0, inputs.seconds)
        };
        // The way up: from (x, y, low) to (x, y, low + height), normalised. [game]
        let way = normalise(Vec3::new(0.0, 0.0, height));
        self.eye_velocity = way * speed;
        self.look_velocity = self.eye_velocity * 0.4;
        self.fov = fov_start * 0.5;
        self.fov_rate = (fov_start - fov_start * 0.5) / timeout;
        self.eye = Vec3::new(beside.x, beside.y, low);
        self.shot_timer = f32::MAX;
    }

    /// The plain shot (0091aa14..0091b162): ahead of the target by `camAheadDist` (or behind when there is no
    /// room), off to the clearer side at a distance that frames it in [`SHOT_FOV`], at a random height
    /// between the ground and the height limits, maybe dollying back, cut again after cutTimeMin..Max.
    #[allow(clippy::too_many_arguments)]
    fn plain_shot(&mut self, centre: Vec3, dir: Vec3, side: Vec3, height: f32, radius: f32, inputs: &ShotInputs, world: &dyn World) {
        let s = &self.settings;
        let sec = inputs.seconds;
        // +0x89 is always set by the Trigger, so the ahead distance is camAheadDist. [game]
        let ahead = s.cam_ahead_dist;
        let span = (height / (self.shot_fov * 0.017_453_292).tan() * 1.5).max(10.0);
        let base = if ahead > radius {
            let reach = ahead + 1.0;
            let mut off = (clear(world, centre, centre + dir * reach, s.ignore_radius) - 1.0).max(0.0);
            if radius * 1.5 > off {
                let back = (clear(world, centre, centre - dir * reach, s.ignore_radius) - 1.0).max(0.0);
                off = -back;
            }
            centre + dir * off
        } else {
            centre + dir * radius
        };
        let reach = span + 1.0;
        let plus = clear(world, base, base + side * reach, 0.0);
        let minus = clear(world, base, base - side * reach, 0.0);
        let room = (span * 0.25).max(1.0);
        let flip = if plus > room && minus > room { self.side } else { plus <= minus };
        self.side = !flip;
        let (side, free) = if flip { (-side, minus) } else { (side, plus) };
        let out = span.min((free - 1.0).max(1.0));
        let at = base + side * out;
        let ground = clear(world, at, at - Vec3::new(0.0, 0.0, 2.0), 0.0) - 2.0;
        // The second probe's end is (x, y, height): the target's HEIGHT used as a world z. [game] kept as found.
        let up = clear(world, at, Vec3::new(at.x, at.y, height), 0.0).max(s.min_target_height + centre.z + height);
        let room_up = (s.max_ground_height - ground).min(up);
        let top = (s.max_target_height + centre.z + height).min(room_up);
        let z = at.z + self.random.between(-ground, top, sec);
        let eye = Vec3::new(at.x, at.y, z);
        let mut goal = eye + side * -20.0;
        if self.random.coin(sec) {
            let level = normalise_xy(dir);
            goal = Vec3::new(eye.x + level.x * -20.0, eye.y + level.y * -20.0, eye.z);
        }
        if clear(world, eye, goal, 0.0) > 5.0 {
            let d = normalise(goal - eye);
            let speed = -self.random.between(1.0, 5.0, sec);
            self.eye_velocity = d * speed;
        } else {
            self.eye_velocity = Vec3::ZERO;
        }
        self.eye = eye;
        let (lo, hi) = self.cut_window();
        self.shot_timer = self.random.between(lo, hi, sec);
    }

    /// The tail of every shot (0091b16f..0091b5ee): aim at the point looked at, keep the angles, reset the
    /// target's travel. The camera's FOV is set to the shot FOV and its FOV rate cleared; the update writes
    /// the behaviour's own FOV over it next step. [game]
    fn finish_cut(&mut self, target: Target, centre: Vec3) {
        if let Some(rows) = aim_rows(self.eye, centre) {
            self.rows = rows;
        }
        let (t, _, _) = look_point(target, self.settings.target_height, self.look);
        let d = t - self.eye;
        self.angles = (-(-d.z).atan2((d.x * d.x + d.y * d.y).sqrt()), (-d.x).atan2(d.y));
        self.travel = 0.0;
        self.previous = centre;
    }

    /// Slot 3 (0091b770). Returns true when the shot should be cut (the caller then calls [`Self::new_shot`],
    /// as the original calls its slot 0 there).
    pub fn update(&mut self, dt: f32, target: Target, world: &dyn World) -> bool {
        self.time += dt;
        self.look += self.look_velocity * dt;
        self.fov += self.fov_rate * dt;
        let (t, _, _) = look_point(target, self.settings.target_height, self.look);
        self.travel += (t - self.previous).length();
        self.previous = t;
        let turn = self.yaw_rate * dt;
        if turn.abs() > 1.745_329_2e-8 {
            self.eye = t + Mat3::from_rotation_z(turn) * (self.eye - t);
        }
        self.eye += self.eye_velocity * dt;
        let r = self.settings.ignore_radius;
        let dist = (self.eye - t).length();
        let start = if r > 0.01 { t + (self.eye - t) * (r / dist) } else { t };
        let blocked = r < dist && world.sphere_blocked(start, self.eye, 0.5);
        let (cut_min, _) = self.cut_window();
        if 0.0 < cut_min {
            self.shot_timer -= dt;
        }
        let moved = self.settings.cut_move_dist > 0.0 && self.travel > self.settings.cut_move_dist;
        blocked || moved || (0.0 < cut_min && self.shot_timer <= 0.0)
    }

    /// Slot 4 (0091bc60): the camera looks from the eye at the point looked at; rows kept when degenerate.
    pub fn aim(&mut self, target: Target) {
        let (t, _, _) = look_point(target, self.settings.target_height, self.look);
        if let Some(rows) = aim_rows(self.eye, t) {
            self.rows = rows;
        }
    }
}

/// Right = forward x up(z), up = right x forward, all three unit within 0.001 or nothing. [game] 0091bc60,
/// 0091b16f.
fn aim_rows(eye: Vec3, at: Vec3) -> Option<[Vec3; 3]> {
    let f = normalise(at - eye);
    let r = normalise(Vec3::new(f.y, -f.x, 0.0));
    let u = r.cross(f);
    let unit = |v: Vec3| (v.length() - 1.0).abs() <= 0.001;
    (unit(r) && unit(f) && unit(u)).then_some([r, f, u])
}

fn normalise(v: Vec3) -> Vec3 {
    let len = v.length();
    if len > 1e-8 { v / len } else { v }
}

fn normalise_xy(v: Vec3) -> Vec3 {
    let len = (v.x * v.x + v.y * v.y).sqrt();
    if len > 1e-8 { Vec3::new(v.x / len, v.y / len, 0.0) } else { Vec3::new(v.x, v.y, 0.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Open;
    impl World for Open {
        fn first_block(&self, _: Vec3, _: Vec3) -> Option<f32> {
            None
        }
        fn sphere_blocked(&self, _: Vec3, _: Vec3, _: f32) -> bool {
            false
        }
    }

    /// A wall across the x axis at x = 2 (blocks any line that crosses it).
    struct Wall;
    impl World for Wall {
        fn first_block(&self, from: Vec3, to: Vec3) -> Option<f32> {
            if (from.x - 2.0) * (to.x - 2.0) < 0.0 {
                let t = (2.0 - from.x) / (to.x - from.x);
                Some((to - from).length() * t)
            } else {
                None
            }
        }
        fn sphere_blocked(&self, from: Vec3, to: Vec3, _: f32) -> bool {
            self.first_block(from, to).is_some()
        }
    }

    #[test]
    fn clear_line_with_skip_and_block() {
        assert_eq!(clear(&Open, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), 0.0), 10.0);
        assert_eq!(clear(&Open, Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 2.0), 1.0, "shorter than the skip");
        assert!((clear(&Open, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), 2.0) - 8.0).abs() < 1e-5, "no block: from the moved start");
        assert!((clear(&Wall, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), 1.0) - 2.0).abs() < 1e-5, "block: hit plus skip");
    }

    #[test]
    fn look_point_kinds() {
        let (t, h, r) = look_point(Target::Object { pos: Vec3::new(1.0, 2.0, 3.0) }, 0.0, Vec3::ZERO);
        assert_eq!((t, h, r), (Vec3::new(1.0, 2.0, 5.5), 5.0, 0.25));
        let (t, h, r) = look_point(Target::Character { bottom: Vec3::ZERO, height: 4.0, reach: 1.2 }, 0.0, Vec3::X);
        assert_eq!((t, h, r), (Vec3::new(1.0, 0.0, 2.0), 4.0, 1.2));
        assert_eq!(look_point(Target::None { camera_pos: Vec3::Z }, 9.0, Vec3::ZERO), (Vec3::Z, 2.0, 0.25));
    }

    #[test]
    fn trigger_actions() {
        let s = Settings { hide_player: true, disable_player: true, game_speed: 0.5, timeout: 4.0, ..Settings::default() };
        assert_eq!(start(&s, false), vec![Start::CameraOn, Start::GameSpeed(0.5), Start::HidePlayer, Start::EndAfter(4.0)]);
        assert!(start(&s, true).is_empty());
        assert_eq!(start(&Settings::default(), false), vec![Start::CameraOn]);
    }

    fn inputs() -> ShotInputs {
        ShotInputs { target_pos: Vec3::ZERO, target_forward: Vec3::Y, seconds: 0, ..ShotInputs::default() }
    }

    #[test]
    fn plain_shot_frames_the_target_from_the_side_and_looks_at_it() {
        let mut sp = Spectator::new(Settings::default(), Random { state: 1 }, 0);
        let target = Target::Object { pos: Vec3::ZERO };
        assert!(sp.new_shot(Some(target), &inputs(), &Open));
        // Ahead 10 along +y, then off to one side by the 50-degree framing distance (at least 10).
        assert!((sp.eye.y - 10.0).abs() < 1e-4, "{:?}", sp.eye);
        assert!((sp.eye.x.abs() - 10.0).abs() < 1e-4, "{:?}", sp.eye);
        let to_target = (Vec3::new(0.0, 0.0, 2.5) - sp.eye).normalize();
        assert!((sp.rows[1] - to_target).length() < 1e-5);
        assert_eq!(sp.travel, 0.0);
    }

    #[test]
    fn a_wall_on_one_side_sends_the_camera_to_the_other() {
        let mut sp = Spectator::new(Settings::default(), Random { state: 7 }, 0);
        let target = Target::Object { pos: Vec3::ZERO };
        sp.new_shot(Some(target), &inputs(), &Wall);
        assert!(sp.eye.x < 0.0, "the +x side is walled at x = 2: {:?}", sp.eye);
    }

    #[test]
    fn rotate_around_orbits_and_pan_up_rises() {
        let s = Settings { rotate_around: true, ..Settings::default() };
        let mut sp = Spectator::new(s, Random { state: 3 }, 0);
        let target = Target::Object { pos: Vec3::ZERO };
        let ins = ShotInputs { own_position: Some(Vec3::new(10.0, 0.0, 2.5)), ..inputs() };
        sp.new_shot(Some(target), &ins, &Open);
        assert_eq!(sp.eye, Vec3::new(10.0, 0.0, 2.5));
        assert!(sp.yaw_rate.abs() <= 0.4364);
        sp.yaw_rate = 0.1;
        assert!(!sp.update(1.0, target, &Open));
        assert!((sp.eye - Vec3::new(10.0 * 0.1f32.cos(), 10.0 * 0.1f32.sin(), 2.5)).length() < 1e-4);

        let s = Settings { pan_up: true, timeout: 2.0, ..Settings::default() };
        let mut sp = Spectator::new(s, Random { state: 3 }, 0);
        sp.new_shot(Some(target), &inputs(), &Open);
        assert!((sp.fov - 30.0).abs() < 1e-5 && (sp.fov_rate - 15.0).abs() < 1e-5);
        let z0 = sp.eye.z;
        sp.update(1.0, target, &Open);
        assert!(sp.eye.z > z0 && (sp.fov - 45.0).abs() < 1e-4);
    }

    #[test]
    fn the_target_moving_past_cut_move_dist_asks_for_a_cut() {
        let s = Settings { cut_move_dist: 3.0, ..Settings::default() };
        let mut sp = Spectator::new(s, Random { state: 3 }, 0);
        sp.new_shot(Some(Target::Object { pos: Vec3::ZERO }), &inputs(), &Open);
        assert!(!sp.update(0.1, Target::Object { pos: Vec3::new(2.0, 0.0, 0.0) }, &Open));
        assert!(sp.update(0.1, Target::Object { pos: Vec3::new(4.0, 0.0, 0.0) }, &Open));
    }
}
