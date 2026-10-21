//! The slam's finisher camera: the camera behaviour the `FinishMoveEvent`'s `startCamera` pushes
//! (`009192c0` -> constructor `00917b40`, vtable 00b65e98, key 0x768ce890, priority 7), framing
//! the attacker and the target he holds. Read in `notes\camera-finisher.md`. A cut in (the
//! camera's rate set to 0), a fixed eye chosen once to one side of the pair, the look-at point
//! following both every frame, no roll; it lasts until `endCamera` (`009193d0`). [game]

use glam::Vec3;

use super::spectator::Random;

/// One of the two characters as the camera takes them (`009180a0`): the bottom point, the top
/// height and the reach offset. [game]
#[derive(Clone, Copy, Debug)]
pub struct Side {
    /// x, y and the bottom z (the object's +0x34..+0x3c; for a character, 0071a460).
    pub bottom: Vec3,
    /// The top height: the bottom plus `character_camera_height` (0070abe0).
    pub top: f32,
    /// The STEP record's `+0x170[+0x244]` (his body radius, `notes\character-height-fields.md`).
    pub reach: f32,
}

impl Side {
    fn centre(&self) -> Vec3 {
        Vec3::new(self.bottom.x, self.bottom.y, 0.5 * (self.bottom.z + self.top))
    }
}

/// The focus between the two, the unit direction from him to the target, the higher top, and the
/// pair's size (`009180a0`): F = the centres' midpoint + half the direction times (reach of the
/// target less his); the size, which it returns in xmm0, is the larger of the horizontal span
/// (the distance between them plus the two reaches) and the vertical one (the higher top less the
/// lower bottom). [game 009180a0, disassembly 009185ff-00918682] [assumed: the two values added to the horizontal distance are the two reaches] Read 2026-10-20: the tail is sqrt(X+Y) + [esp+0x18] + [esp+0x4c] (xmm0 then the fadd), and slot 0x18 is written from 0071a460 (bottom z) and 0070abe0 (height) at 0x918188 and 0x9181a6, not from a reach, so the reach reading is doubtful. settles: the writes of slots 0x4c, 0x54 and 0x18 in 009180a0 (raw_disasm 0x918145-0x918359) to name the two terms.
fn pair(him: &Side, it: &Side) -> (Vec3, Vec3, f32, f32) {
    let (h, t) = (him.centre(), it.centre());
    let dir = (t - h).try_normalize().unwrap_or(Vec3::Y);
    let focus = 0.5 * (h + t) + 0.5 * dir * (it.reach - him.reach);
    let top = him.top.max(it.top);
    let flat = (t - h).truncate().length() + him.reach + it.reach;
    let size = flat.max(top - him.bottom.z.min(it.bottom.z));
    (focus, dir, top, size)
}

/// The camera's look: eye, unit forward and up, field of view in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub eye: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub fov: f32,
}

/// The finisher camera once entered: its eye (+0x3c) and field of view (+0x28) are kept for
/// its life. [game 00918fa0 writes neither]
#[derive(Clone, Copy, Debug)]
pub struct Finisher {
    pub eye: Vec3,
    pub fov: f32,
}

/// The entry field of view (`+0x28` = 50.0). [game 009186f0]
const FOV: f32 = 50.0;

impl Finisher {
    /// Its entry (slot 0, `009186f0`; disassembly read in `notes\camera-finisher-eye.md`). `ray`
    /// is the camera's ray helper 00917dc0: the distance from the first point to the first
    /// blocking object on the segment to the second, else the segment's length. [game]
    /// - hw = half the pair's size (`pair`); D = max(hw / tan 50, 15).
    /// - The side: rays from the focus D + 1 to either side; the longer clear one wins; within
    ///   0.01 of each other, a coin of the random stream (`Random::coin`, drawn only then).
    /// - The point P = F + S * min(max(free - 1, 1), D).
    /// - Lifted by a random between -(the clear way down to max(lower bottom - 1, P.z - 22),
    ///   less 2) and the clear way up to the higher top; then slid along the pair by a random
    ///   between -(the clear way back, 2 hw) and the clear way forward (hw / 2).
    /// - The field of view widened to atan(hw / d) when tan 50 < hw / d, d the eye's distance.
    /// [game 009186f0, 00551d60, 00466960 = atan] [stand-in: the random stream is the camera's
    /// own, `Random`; the game's is shared] settles: the state's global address in 00551d60 and its readers, to decide whether the port can share one stream; a run-time value, so also a recording of the stream at a finisher's entry.
    pub fn enter(him: &Side, it: &Side, random: &mut Random, seconds: u32, ray: &dyn Fn(Vec3, Vec3) -> f32) -> Self {
        let (focus, dir, top, size) = pair(him, it);
        let half_width = 0.5 * size;
        let tan = FOV.to_radians().tan();
        let reach = (half_width / tan).max(15.0);
        let hlen = glam::Vec2::new(dir.x, dir.y).length();
        let mut side = Vec3::new(dir.y, -dir.x, 0.0);
        if hlen > 1e-8 {
            side /= hlen;
        }
        let r1 = ray(focus, focus + side * (reach + 1.0));
        let r2 = ray(focus, focus - side * (reach + 1.0));
        let (side, free) = if (r1 - r2).abs() >= 0.01 {
            if r2 > r1 { (-side, r2) } else { (side, r1) }
        } else if random.coin(seconds) {
            (-side, r2)
        } else {
            (side, r1)
        };
        let p = focus + side * (free - 1.0).max(1.0).min(reach);
        let floor = (him.bottom.z.min(it.bottom.z) - 1.0).max(p.z - 22.0);
        let down = ray(p, Vec3::new(p.x, p.y, floor)) - 2.0;
        let up = ray(p, Vec3::new(p.x, p.y, top));
        let p = p + Vec3::Z * random.between(-down, up, seconds);
        let back = ray(p, p - dir * (2.0 * half_width));
        let ahead = ray(p, p + dir * (0.5 * half_width));
        let eye = p + dir * random.between(-back, ahead, seconds);
        let d = (focus - eye).length().max(1e-6);
        let fov = if tan < half_width / d { (half_width / d).atan().to_degrees() } else { FOV };
        Self { eye, fov }
    }

    /// Every frame (slot 4, `00918fa0`): the focus from the two where they are now, snapped 2
    /// along the last forward when it comes within 0.32 of the eye; forward to it, the right
    /// level with the horizon, up their cross. The eye and the field of view stay. [game]
    pub fn aim(&self, him: &Side, it: &Side, previous_forward: Vec3) -> Pose {
        let (mut focus, _, _, _) = pair(him, it);
        if (focus - self.eye).length_squared() < 0.1 {
            focus = self.eye + 2.0 * previous_forward;
        }
        let forward = (focus - self.eye).try_normalize().unwrap_or(previous_forward);
        let right = Vec3::new(forward.y, -forward.x, 0.0).try_normalize().unwrap_or(Vec3::X);
        Pose { eye: self.eye, forward, up: right.cross(forward), fov: self.fov }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(x: f32, y: f32) -> Side {
        Side { bottom: Vec3::new(x, y, 0.0), top: 5.4, reach: 1.5 }
    }

    /// Open ground: no ray is blocked.
    fn open(from: Vec3, to: Vec3) -> f32 {
        (to - from).length()
    }

    #[test]
    fn in_the_open_the_eye_is_fifteen_to_the_side_and_stays_while_the_look_follows() {
        let (him, it) = (side(0.0, 0.0), side(0.0, 6.0));
        let camera = Finisher::enter(&him, &it, &mut Random { state: 7 }, 3, &open);
        // Size max(6 + 1.5 + 1.5, 5.4) = 9: hw 4.5, D = max(4.5 / tan 50, 15) = 15. Both side rays
        // clear (16 each): the coin; P is min(max(16 - 1, 1), 15) = 15 to that side of the focus
        // (0, 3, 2.7); the lift and the slide are draws within the clear ways.
        let off = camera.eye - Vec3::new(0.0, 3.0, 2.7);
        assert!((off.x.abs() - 15.0).abs() < 1e-4, "{off}");
        // 15 m and more away: tan 50 > 4.5 / d, no widening.
        assert!((camera.fov - 50.0).abs() < 1e-4);
        let pose = camera.aim(&him, &side(0.0, 10.0), Vec3::Y);
        assert_eq!(pose.eye, camera.eye);
        assert!(pose.up.z > 0.0 && pose.forward.cross(pose.up).z.abs() < 1e-5, "no roll");
        let focus = Vec3::new(0.0, 5.0, 2.7);
        assert!((pose.forward - (focus - camera.eye).normalize()).length() < 1e-5);
    }

    #[test]
    fn a_wall_on_one_side_puts_the_eye_on_the_other() {
        let (him, it) = (side(0.0, 0.0), side(0.0, 6.0));
        // A wall 3 m out along +x: the ray that way stops there, so the eye goes to -x.
        let wall = |from: Vec3, to: Vec3| {
            if to.x > from.x + 3.0 { (3.0 - from.x) / (to.x - from.x) * (to - from).length() } else { (to - from).length() }
        };
        let camera = Finisher::enter(&him, &it, &mut Random { state: 1 }, 0, &wall);
        assert!(camera.eye.x < -14.0, "{}", camera.eye);
    }
}
