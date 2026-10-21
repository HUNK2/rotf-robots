//! The missile launcher's lock-on: which targets are in the box in the middle of the
//! picture, and how holding the trigger turns one into the lock.
//!
//! Two pieces of the original, both read from disassembly (`notes\weapon-mode.md` "The
//! lock-on and the missile"):
//!
//! - The character's aim-assist object (owner `+0x23c`; update `FUN_0070a0d0` ->
//!   `FUN_007079f0`, a target's test `FUN_007088f0`) keeps up to three targets whose
//!   picture overlaps a box about a point of a 1280 x 720 picture: `candidates`.
//! - The weapon manager's `FUN_007b1bb0` runs the lock on them: `LockOn`.

use glam::{Vec2, Vec3};

use crate::melee::Target;
use crate::weapons::WeaponTuning;

/// The picture the box is measured in, whatever the window's size (`FUN_007088f0` scales
/// what it projects by 1280 / width and 720 / height). [game]
pub const PICTURE: Vec2 = Vec2::new(1280.0, 720.0);

/// The camera, as the search uses it.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub eye: Vec3,
    pub right: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    /// The field of view top to bottom, in degrees, and the picture's width over its height.
    pub fov: f32,
    pub aspect: f32,
}

impl View {
    /// A camera at `eye` looking along `forward`, level.
    pub fn looking(eye: Vec3, forward: Vec3, fov: f32, aspect: f32) -> Self {
        let forward = forward.normalize_or(Vec3::Y);
        let right = forward.cross(Vec3::Z).normalize_or(Vec3::X);
        Self { eye, right, forward, up: right.cross(forward), fov, aspect }
    }

    /// Where a point is in the 1280 x 720 picture, or `None` behind the camera.
    /// [assumed: y runs down the picture, as Direct3D's pixels do]. 2026-10-20 read: the game's
    /// `project_point_screen` 00a98da0 divides its clip x and y by w (l.15-17) and applies no y
    /// flip of its own; the sign of y comes from the transform it is handed (00584ce0 and
    /// 0057ab60, not read). OPEN, settles: the view-projection's y row (00584ce0), or where the
    /// bracket is drawn on screen (007b8420).
    pub fn project(&self, point: Vec3) -> Option<Vec2> {
        let d = point - self.eye;
        let depth = d.dot(self.forward);
        if depth <= 1e-4 {
            return None;
        }
        let tan = (self.fov.to_radians() * 0.5).tan();
        let x = d.dot(self.right) / (depth * tan * self.aspect);
        let y = d.dot(self.up) / (depth * tan);
        Some(PICTURE * 0.5 * Vec2::new(1.0 + x, 1.0 - y))
    }
}

/// Half the size of the box a launcher locks in: `missileLockOnBoxWidth` / 1280 * 640 and
/// `missileLockOnBoxHeight` / 720 * 360 (`FUN_0070a220`, `FUN_0070a2f0`), so the box is as
/// many pixels across as the data says: his is 128 x 128. [game] (Any other weapon's box
/// is the aim assist's `targetBoxPixelWidth` and `Height`: not ported.)
pub fn lock_box(t: &WeaponTuning) -> Vec2 {
    Vec2::new(t.lock_box[0], t.lock_box[1]) * 0.5
}

/// The point of a target the search looks at for a clear line: its `Head` node, or for
/// one without (or in car form) its place + 0.75 of its height. [game 0079e920 l.171-178 for the no-head point; Head branch as 00709870 l.63-74]
/// `Target::head` carries the Head node's place (filled for the arena's scouts from their posed model,
/// `game/src/targets.rs` animate, 2026-10-20); a target without it takes the no-head point.
pub fn aim_at(target: &Target) -> Vec3 {
    crate::aim::assist_point(target)
}

/// The targets in the box, best first, at most three (`FUN_007079f0`). [game]
///
/// A target counts (`FUN_007088f0`) if it is ahead of the camera and ahead of him (the
/// way to it from `own` against `facing`, both over 0 as a dot product), nearer the
/// camera than `range`, and its picture overlaps the box: the picture being, across, its
/// place moved its radius along the camera's right both ways, and, up and down, its place
/// and its place + its height. Its score is the squared distance in the picture from the
/// box's middle to the point halfway across and 0.8 of the way from the least y to the
/// most. The three with the lowest scores are kept, but `main` (the one that was best the
/// update before) first; then any whose point (`aim_at`) is not in clear sight of the
/// camera is taken out again, leaving its place empty (`clear(from, to, target)`).
///
/// `centre` is the middle of the box: the picture's middle in weapon mode (640, 360).
/// [assumed: which of the two sizes read from a character is its height and which its
/// radius, by how they are used; the two "place" routines `FUN_0071a2e0` and
/// `FUN_0071a460` give the same point] Not in: targets that are not characters
/// (`FUN_00709100`), the test `thunk_FUN_0117e6d0` (taken to be "an enemy"), and the
/// target's flag `+0x148` bit 3.
#[allow(clippy::too_many_arguments)]
pub fn candidates(
    view: &View,
    own: Vec3,
    facing: Vec3,
    targets: &[Target],
    range: f32,
    centre: Vec2,
    half: Vec2,
    main: Option<u32>,
    clear: impl Fn(Vec3, Vec3, &Target) -> bool,
) -> [Option<u32>; 3] {
    let mut seen: Vec<(f32, &Target)> = Vec::new();
    for target in targets {
        let from_eye = target.pos - view.eye;
        if from_eye.normalize_or_zero().dot(view.forward) <= 0.0 {
            continue;
        }
        if (target.pos - own).normalize_or_zero().dot(facing.normalize_or_zero()) <= 0.0 {
            continue;
        }
        if from_eye.length_squared() >= range * range {
            continue;
        }
        let side = view.right * target.radius;
        let (Some(foot), Some(top), Some(left), Some(right)) = (
            view.project(target.pos),
            view.project(target.pos + Vec3::Z * target.height),
            view.project(target.pos - side),
            view.project(target.pos + side),
        ) else {
            continue;
        };
        let (x0, x1) = (left.x.min(right.x), left.x.max(right.x));
        let (y0, y1) = (foot.y.min(top.y), foot.y.max(top.y));
        // The two rectangles overlap (`FUN_00707980`).
        let across = x0.max(centre.x - half.x) < x1.min(centre.x + half.x);
        let down = y0.max(centre.y - half.y) < y1.min(centre.y + half.y);
        if !(across && down) {
            continue;
        }
        let point = Vec2::new((x0 + x1) * 0.5, y0 + (y1 - y0) * 0.8);
        seen.push(((point - centre).length_squared(), target));
    }
    seen.sort_by(|a, b| {
        let first = |t: &Target| main != Some(t.id);
        first(a.1).cmp(&first(b.1)).then(a.0.total_cmp(&b.0))
    });
    let mut kept = [None; 3];
    for (slot, (_, target)) in kept.iter_mut().zip(seen) {
        if clear(view.eye, aim_at(target), target) {
            *slot = Some(target.id);
        }
    }
    kept
}

/// The weapon manager's lock (`FUN_007b1bb0`). [game]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LockOn {
    /// The target being locked on (manager `+0xc0`) and the one locked (`+0xb8`): what a
    /// missile is given at its launch.
    pub candidate: Option<u32>,
    pub locked: Option<u32>,
    /// Seconds the candidate has been held in the box (`+0xc8`), and seconds left of the
    /// lock's stay (`+0xcc`).
    pub held: f32,
    pub stay: f32,
    /// Time not yet used up by whole updates of the game's.
    clock: f32,
}

impl LockOn {
    /// How near the lock is, 0 to 1, for whatever draws it.
    pub fn progress(&self, t: &WeaponTuning) -> f32 {
        if t.lock_on_time <= 0.0 { 1.0 } else { (self.held / t.lock_on_time).clamp(0.0, 1.0) }
    }

    /// A step of any length, run in whole updates of the game's.
    pub fn step(&mut self, t: &WeaponTuning, armed: bool, pulling: bool, seen: &[Option<u32>; 3], targets: &[Target], view: &View, dt: f32) {
        self.clock += dt;
        while self.clock >= crate::sim::GAME_UPDATE {
            self.clock -= crate::sim::GAME_UPDATE;
            self.update(t, armed, pulling, seen, targets, view, crate::sim::GAME_UPDATE);
        }
    }

    /// One update of the game's.
    ///
    /// - `armed`: the weapon in hand is a launcher, `guided`, and he is in weapon mode
    ///   (control mode 2). If not: no candidate, no lock, the time held 0.
    /// - A lock time of 0 or less: the lock is the aim assist's best target, at once.
    /// - `pulling`: for a launcher with `mustHoldFireTriggerToLockOn`, the trigger is down
    ///   and the weapon may lock (`Weapon::may_lock`); for any other, always. Without it
    ///   NOTHING changes: the lock and the time held stay as they are, which is how the
    ///   missile let go of still has its target, and why pulling again on the same target
    ///   locks at once.
    /// - While the candidate is still one of `seen`: the time held goes up, and at
    ///   `missileCrosshairLockOnTime` or over it is the lock, with
    ///   `lockOnBracketStayOnTime` to stay.
    /// - When it is not (or there is none): the candidate is the first of `seen` that is
    ///   alive. The lock is dropped if it is dead, its stay has run out, there is a new
    ///   candidate that is not it, or it is further off the camera's forward than 0.33 of
    ///   the field of view (the whole of it for a car's weapon; the way to the target's
    ///   place, the angle in degrees as the camera gives its field of view). With no lock
    ///   the time held goes back to 0.
    /// - Last, every update: the stay runs down.
    ///
    /// `targets` are the living ones. The field of view is `scene_camera_fov` 00557aa0: the
    /// active camera's float +0x94 (camera at global +0x454, when +0x464 bit 0 is set and +0x468
    /// > 0), else 60 degrees (00b34630) [game, wave 28 P2; `FUN_0070a220` is the assist box, not
    /// this]. [assumed: +0x94 is the camera's top-to-bottom field, as for the scatter] settles:
    /// the meaning of the camera's +0x94 (notes\camera.md). Not in: `breakLockOnAfterFire` (his is false; where it
    /// is used is not read). Whether it is armed in the car is `armed_in`.
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, t: &WeaponTuning, armed: bool, pulling: bool, seen: &[Option<u32>; 3], targets: &[Target], view: &View, dt: f32) {
        let alive = |id: u32| targets.iter().find(|target| target.id == id);
        if !armed {
            (self.candidate, self.locked, self.held) = (None, None, 0.0);
        } else if t.lock_on_time <= 0.0 {
            self.locked = seen.iter().flatten().copied().next();
        } else if pulling {
            match self.candidate.filter(|id| seen.contains(&Some(*id))) {
                Some(id) => {
                    self.held += dt;
                    if self.held >= t.lock_on_time {
                        self.locked = Some(id);
                        self.stay = t.lock_bracket_stay;
                    }
                }
                None => {
                    self.candidate = seen.iter().flatten().copied().find(|&id| alive(id).is_some());
                    if let Some(id) = self.locked {
                        let keep = alive(id).is_some_and(|target| {
                            let ahead = (target.pos - view.eye).normalize_or_zero().dot(view.forward);
                            self.stay > 0.0
                                && self.candidate.is_none_or(|new| new == id)
                                && ahead >= (view.fov * keep_share(t)).to_radians().cos()
                        });
                        if !keep {
                            self.locked = None;
                        }
                    }
                    if self.locked.is_none() {
                        self.held = 0.0;
                    }
                }
            }
        }
        if self.stay > 0.0 {
            self.stay -= dt;
        }
    }
}

/// The share of the camera's field of view within which a lock is kept (`007b1bb0` step 5):
/// 0.33 of it, the whole of it for an `isVehicleModeWeapon` (slot `+0xac`). [game]
pub fn keep_share(t: &WeaponTuning) -> f32 {
    if t.vehicle_mode { 1.0 } else { 0.33 }
}

/// Whether the manager's lock runs for the weapon in hand (`manager_lock_on` 007b1bb0
/// l.14-15): a `guided` launcher, held in weapon mode (control mode 2) or, as the car's
/// weapon in hand, while the car's weapon may fire (`FUN_007b44e0`: DriveMode or FlyMode and
/// the manager's wait `+0xd0` over). In the car the weapon in hand IS the car's
/// (`007b43b0` moves `+0x74` to the `isVehicleModeWeapon` one as control mode 3 starts).
/// [game]
pub fn armed_in(weapon_mode: bool, car_weapon_may_fire: bool, launcher: bool, guided: bool) -> bool {
    launcher && guided && (weapon_mode || car_weapon_may_fire)
}

/// The team hashes a character's `+0x244` holds (not in `names.tsv`): the Autobot side (the
/// protoforms, Bumblebee), the Decepticon side (the DCom group), and the NPC team KillBox counts
/// apart (`notes\ai-spawn-characters.md`, `damage_objects.rs`). [data]
pub const TEAM_AUTOBOT: u32 = 0x951a_f706;
pub const TEAM_DECEPTICON: u32 = 0xa54d_c247;
pub const TEAM_NPC: u32 = 0xea29_1c1d;

/// A `TargetLockable` component: what a missile can lock onto that is not a character (a
/// turret, a barrier part). Loader `FUN_007badf0` (census label PlayCharPartialAnim is wrong).
/// [game 007badf0]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lockable {
    /// `radius` (+0x44), `height` (+0x48), `zOffset` (+0x4c); 0 when absent.
    pub radius: f32,
    pub height: f32,
    pub z_offset: f32,
    /// `targetableByAutobots` (+0x50), `targetableByDecepticons` (+0x51): on when absent.
    pub by_autobots: bool,
    pub by_decepticons: bool,
    /// `onlyMakeCrosshairTurnRed` (+0x52): on when absent. [game 007badf0] (the loader reads the
    /// field by hash `e17fe329` and defaults it to 1). [game 007b1bb0 l.91] The reader is the
    /// missile lock's re-pick in `manager_lock_on` (l.74-108, run when the locked target is lost):
    /// of the 3 candidates (`+0x23c`), a CHARACTER is passed over while invincible (00724180) and
    /// any other object while its `+0x52` is set; the first left is taken [game 007b1bb0]. So the
    /// flag keeps a non-character from being locked (its crosshair may only turn red). The port's
    /// lock-on takes characters only (`candidates`: "Not in: targets that are not characters"),
    /// so the flag has no object to act on until non-character candidates are built.
    pub crosshair_only: bool,
    /// `startEnabled` (+0x53): on when absent.
    pub start_enabled: bool,
}

/// Reads a `TargetLockable` node. [game 007badf0]
pub fn read_lockable(node: crate::formats::lxb::Node) -> Lockable {
    let num = |name| node.get(name).and_then(crate::formats::lxb::Node::float).unwrap_or(0.0);
    let flag = |name| node.get(name).map_or(true, crate::formats::lxb::Node::is_true);
    Lockable {
        radius: num("radius"),
        height: num("height"),
        z_offset: num("zOffset"),
        by_autobots: flag("targetableByAutobots"),
        by_decepticons: flag("targetableByDecepticons"),
        crosshair_only: flag("onlyMakeCrosshairTurnRed"),
        start_enabled: flag("startEnabled"),
    }
}

impl Lockable {
    /// Whether an owner of `team` leaves this lockable alone, in a single-player game
    /// (`FUN_007bb2c0`, true = not its target). An Autobot or the NPC team needs
    /// `targetableByAutobots`, a Decepticon `targetableByDecepticons`; any other team leaves
    /// it alone. A grenade's proximity contact with a non-character goes off only when this
    /// says no (`FUN_007876b0`). The multiplayer branch (same team number = left alone) is not
    /// ported. [game 007bb2c0]
    pub fn ignored_by(&self, team: u32) -> bool {
        match team {
            TEAM_AUTOBOT | TEAM_NPC => !self.by_autobots,
            TEAM_DECEPTICON => !self.by_decepticons,
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = crate::sim::GAME_UPDATE;

    #[test]
    fn a_lockable_is_left_alone_by_the_side_it_is_not_targetable_by() {
        let l = Lockable { radius: 2.0, height: 1.0, z_offset: 0.0, by_autobots: false, by_decepticons: true, crosshair_only: false, start_enabled: true };
        assert!(l.ignored_by(TEAM_AUTOBOT));
        assert!(l.ignored_by(TEAM_NPC));
        assert!(!l.ignored_by(TEAM_DECEPTICON));
        assert!(l.ignored_by(0x1234_5678));
    }

    fn launcher() -> WeaponTuning {
        WeaponTuning {
            lock_on_time: 0.4,
            guided: true,
            hold_to_lock_on: true,
            lock_bracket_stay: 1.0,
            lock_box: [128.0, 128.0],
            aim_assist_max_range: 350.0,
            ..Default::default()
        }
    }

    fn dummy(id: u32, pos: Vec3) -> Target {
        Target { id, pos, radius: 1.5, height: 5.5, character: true, large: false, surface: 0, dashing: false, health: f32::MAX, head: None }
    }

    fn view() -> View {
        View::looking(Vec3::new(0.0, -10.0, 4.0), Vec3::Y, 55.0, 16.0 / 9.0)
    }

    fn see(view: &View, targets: &[Target], main: Option<u32>) -> [Option<u32>; 3] {
        candidates(view, Vec3::ZERO, Vec3::Y, targets, 350.0, PICTURE * 0.5, lock_box(&launcher()), main, |_, _, _| true)
    }

    #[test]
    fn the_box_takes_what_overlaps_it_nearest_its_middle_first() {
        let view = view();
        let targets = [
            dummy(1, Vec3::new(7.0, 60.0, 0.0)),
            dummy(2, Vec3::new(1.0, 60.0, 0.0)),
            // Well off to the side, out of range, and behind: none of them counts.
            dummy(3, Vec3::new(40.0, 60.0, 0.0)),
            dummy(4, Vec3::new(0.0, 400.0, 0.0)),
            dummy(5, Vec3::new(0.0, -40.0, 0.0)),
        ];
        // 64 pixels of 640 at 60 away and this view: about 3.5 either side of the middle.
        assert_eq!(see(&view, &targets, None), [Some(2), Some(1), None]);
        // The one that was best stays first.
        assert_eq!(see(&view, &targets, Some(1)), [Some(1), Some(2), None]);
        // One out of sight leaves its place empty.
        let hidden = candidates(&view, Vec3::ZERO, Vec3::Y, &targets, 350.0, PICTURE * 0.5, lock_box(&launcher()), None, |_, _, t| t.id != 2);
        assert_eq!(hidden, [None, Some(1), None]);
    }

    #[test]
    fn a_guided_car_launcher_is_armed_while_the_car_weapon_may_fire() {
        // Weapon mode, or the car's weapon in hand and free to fire; never a gun or an
        // unguided launcher. [game 007b1bb0 l.14-15]
        assert!(armed_in(true, false, true, true));
        assert!(armed_in(false, true, true, true));
        assert!(!armed_in(false, false, true, true));
        assert!(!armed_in(false, true, false, true));
        assert!(!armed_in(true, true, true, false));
    }

    #[test]
    fn a_car_weapon_keeps_its_lock_across_the_whole_view() {
        // 30 degrees off a 55-degree view: past a third of it (18.3), within the whole.
        let view = view();
        let (s, c) = 30f32.to_radians().sin_cos();
        let off = view.eye + Vec3::new(s, c, 0.0) * 60.0;
        let targets = [dummy(1, Vec3::new(0.0, 60.0, 0.0)), dummy(2, Vec3::new(off.x, off.y, 0.0))];
        let run = |t: &WeaponTuning| {
            let mut lock = LockOn::default();
            for _ in 0..15 {
                lock.update(t, true, true, &[Some(2), None, None], &targets, &view, DT);
            }
            assert_eq!(lock.locked, Some(2));
            // Out of the box: the stay keeps it, unless it is too far off the forward.
            lock.update(t, true, true, &[None; 3], &targets, &view, DT);
            lock.locked
        };
        assert_eq!(run(&launcher()), None);
        assert_eq!(run(&WeaponTuning { vehicle_mode: true, ..launcher() }), Some(2));
    }

    #[test]
    fn holding_the_trigger_on_a_target_locks_after_the_lock_time() {
        let (t, view) = (launcher(), view());
        let targets = [dummy(1, Vec3::new(0.0, 60.0, 0.0))];
        let seen = see(&view, &targets, None);
        let mut lock = LockOn::default();
        // Not pulling: nothing.
        lock.update(&t, true, false, &seen, &targets, &view, DT);
        assert_eq!(lock, LockOn::default());
        // The first update takes the candidate; 0.4 s needs 13 more of 0.032.
        for _ in 0..13 {
            lock.update(&t, true, true, &seen, &targets, &view, DT);
        }
        assert_eq!((lock.candidate, lock.locked), (Some(1), None));
        lock.update(&t, true, true, &seen, &targets, &view, DT);
        assert_eq!(lock.locked, Some(1));
        // Let go: the lock is still there for the missile.
        lock.update(&t, true, false, &seen, &targets, &view, DT);
        assert_eq!(lock.locked, Some(1));
        // Out of weapon mode it is gone.
        lock.update(&t, false, false, &seen, &targets, &view, DT);
        assert_eq!((lock.candidate, lock.locked, lock.held), (None, None, 0.0));
    }

    #[test]
    fn a_lock_outlasts_its_target_leaving_the_box_by_the_stay_time() {
        let (t, view) = (launcher(), view());
        let targets = [dummy(1, Vec3::new(0.0, 60.0, 0.0)), dummy(2, Vec3::new(30.0, 60.0, 0.0))];
        let seen = [Some(1), None, None];
        let mut lock = LockOn::default();
        for _ in 0..15 {
            lock.update(&t, true, true, &seen, &targets, &view, DT);
        }
        assert_eq!(lock.locked, Some(1));
        // Nothing in the box: it stays for a second (it is 0 off the forward here).
        let none = [None; 3];
        for _ in 0..30 {
            lock.update(&t, true, true, &none, &targets, &view, DT);
        }
        assert_eq!(lock.locked, Some(1));
        for _ in 0..3 {
            lock.update(&t, true, true, &none, &targets, &view, DT);
        }
        assert_eq!((lock.locked, lock.held), (None, 0.0));

        // Another target in the box takes it away at once.
        let mut lock = LockOn::default();
        for _ in 0..15 {
            lock.update(&t, true, true, &seen, &targets, &view, DT);
        }
        lock.update(&t, true, true, &[Some(2), None, None], &targets, &view, DT);
        assert_eq!((lock.candidate, lock.locked, lock.held), (Some(2), None, 0.0));

        // And so does the camera turning well away from it: a third of 55 degrees.
        let mut lock = LockOn::default();
        for _ in 0..15 {
            lock.update(&t, true, true, &seen, &targets, &view, DT);
        }
        let away = View::looking(view.eye, Vec3::new(20f32.to_radians().sin(), 20f32.to_radians().cos(), 0.0), 55.0, 16.0 / 9.0);
        lock.update(&t, true, true, &none, &targets, &away, DT);
        assert_eq!(lock.locked, None);
    }
}
