//! `CameraTracker`: a level component that puts its scene object where the game camera is, every update,
//! keeping as much of the camera's turn as its fields allow (sky domes and other things hung on the view).
//! [game] `notes\camera.md` "CameraTracker".
//!
//! Registrar 00adb550 (factory 007d1ba0), loader 01187090, constructor 007d1110 (vtable 00b603ec: slot 2 the
//! update 007d11d0, slot 4 the activation 007d1160). Placed 11 times in 7 levels (`notes\phase1-audit.md`).
use glam::{Quat, Vec3};

/// The loader's flag word. Pitch, yaw and roll are KEEP bits: set when `ignore*` is missing or false
/// [game] (01187090: `ebx = 1` when ignorePitch is absent or zero, then `or 2`, `or 4`). Height is an IGNORE
/// bit, set when `ignoreHeight` is true; the PC build computes its 8 through an obfuscated sequence
/// (0118715a..011871c1: (0x2f80861a - 0x037da540 + 0x39a1886e) ^ ror(0x7339a088, 13) ^ 0x61e7f08d = 8).
pub mod flag {
    pub const KEEP_PITCH: u32 = 1;
    pub const KEEP_YAW: u32 = 2;
    pub const KEEP_ROLL: u32 = 4;
    pub const IGNORE_HEIGHT: u32 = 8;
}

/// Rows of a packed affine: the object's x (right), y (forward) and z (up) axes in the world.
pub type Rows = [Vec3; 3];

const IDENTITY: Rows = [Vec3::X, Vec3::Y, Vec3::Z];

#[derive(Debug, Clone, PartialEq)]
pub struct CameraTracker {
    pub flags: u32,
    /// `offset` (+0x2c), in the frame the tracker keeps. The loader makes no object without it [game].
    pub offset: Vec3,
    /// +0x38: the object's rotation when activated (slot 4 copies its affine and zeroes the translation).
    pub start: Rows,
}

impl CameraTracker {
    /// The fields as the loader reads them; `None` is a missing field.
    pub fn new(offset: Vec3, ignore_pitch: Option<bool>, ignore_yaw: Option<bool>, ignore_roll: Option<bool>, ignore_height: Option<bool>, start: Rows) -> Self {
        let mut flags = 0;
        if !ignore_pitch.unwrap_or(false) {
            flags |= flag::KEEP_PITCH;
        }
        if !ignore_yaw.unwrap_or(false) {
            flags |= flag::KEEP_YAW;
        }
        if !ignore_roll.unwrap_or(false) {
            flags |= flag::KEEP_ROLL;
        }
        if ignore_height.unwrap_or(false) {
            flags |= flag::IGNORE_HEIGHT;
        }
        Self { flags, offset, start }
    }

    /// The object's rows and position for this camera (007d11d0).
    ///
    /// - None of pitch, yaw, roll kept: the frame is the start rotation. All three: the camera's rows.
    ///   Otherwise the frame starts at identity and turns by the camera's heading (yaw kept), then by its
    ///   pitch (pitch kept); roll alone leaves identity.
    /// - Position: the camera's plus `offset` in that frame; `ignoreHeight` sets the height to 0 (world z = 0,
    ///   not the camera's height) [game].
    /// - The object's rows are the start rotation expressed in the frame (`compose_affine(frame, start)`), so
    ///   with nothing kept the start rotation is applied twice. Kept as found [game]; harmless for the usual
    ///   unrotated object.
    pub fn place(&self, camera: Rows, camera_pos: Vec3) -> (Rows, Vec3) {
        let frame = match self.flags & 7 {
            0 => self.start,
            7 => camera,
            keep => {
                let mut f = IDENTITY;
                if keep & flag::KEEP_YAW != 0 {
                    f = turn(heading(camera, keep & flag::KEEP_PITCH != 0), f);
                }
                if keep & flag::KEEP_PITCH != 0 {
                    f = turn(pitch(camera), f);
                }
                f
            }
        };
        let mut pos = camera_pos + frame[0] * self.offset.x + frame[1] * self.offset.y + frame[2] * self.offset.z;
        if self.flags & flag::IGNORE_HEIGHT != 0 {
            pos.z = 0.0;
        }
        let rows = self.start.map(|r| frame[0] * r.x + frame[1] * r.y + frame[2] * r.z);
        (rows, pos)
    }
}

/// `compose_affine(Q, frame)` with Q built from a row quaternion: each row turned by Q. The row quaternion is
/// conjugated for glam (`CLAUDE.md`). [game] 0052f1c0, 00560fa0, 0052fbf0.
fn turn(row_quat: Quat, frame: Rows) -> Rows {
    let q = row_quat.conjugate();
    frame.map(|r| q * r)
}

/// The camera's level forward: forward with its height dropped and normalised (kept when shorter than 1e-8);
/// when forward is straight up or down, minus the camera's up. [game] 007d11d0.
fn level_forward(camera: Rows) -> Vec3 {
    let f = camera[1];
    if f.x == 0.0 && f.y == 0.0 {
        return -camera[2];
    }
    let len = (f.x * f.x + f.y * f.y).sqrt();
    if len > 1e-8 { Vec3::new(f.x / len, f.y / len, 0.0) } else { Vec3::new(f.x, f.y, 0.0) }
}

/// The heading turn: a row quaternion about z from half-angle square roots of the level forward's y, its
/// sign from the level forward's x. With pitch kept and the camera upside down (up.z < 0) the level forward
/// is reversed first. [game] 007d12f4..007d1515.
fn heading(camera: Rows, keep_pitch: bool) -> Quat {
    let sign = if keep_pitch && camera[2].z < 0.0 { -1.0 } else { 1.0 };
    let h = level_forward(camera);
    let (s, c) = (h.x * sign, h.y * sign);
    let half = c * 0.5;
    let mut w = (half + 0.5).max(0.0).sqrt();
    let z = (0.5 - half).max(0.0).sqrt();
    if s <= 0.0 {
        w = -w;
    }
    Quat::from_xyzw(0.0, 0.0, z, w)
}

/// The pitch turn: about forward x level-forward, by the half angles of their dot product (both negated when
/// the camera is upside down). [game] 007d1557..007d184a.
fn pitch(camera: Rows) -> Quat {
    let f = camera[1];
    let h = level_forward(camera);
    let upside_down = camera[2].z < 0.0;
    let mut d = f.dot(h);
    if upside_down {
        d = -d;
    }
    let half = d * 0.5;
    let w = (half + 0.5).max(0.0).sqrt();
    let mut s = (0.5 - half).max(0.0).sqrt();
    if upside_down {
        s = -s;
    }
    let axis = f.cross(h);
    let len = axis.length();
    let axis = if len > 1e-8 { axis / len } else { axis };
    Quat::from_xyzw(axis.x * s, axis.y * s, axis.z * s, w)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(yaw: f32, pitch_down: f32) -> Rows {
        let q = Quat::from_rotation_z(yaw) * Quat::from_rotation_x(-pitch_down);
        [q * Vec3::X, q * Vec3::Y, q * Vec3::Z]
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn loader_bits() {
        let t = CameraTracker::new(Vec3::ZERO, None, Some(true), Some(false), Some(true), IDENTITY);
        assert_eq!(t.flags, flag::KEEP_PITCH | flag::KEEP_ROLL | flag::IGNORE_HEIGHT);
    }

    #[test]
    fn keeping_everything_follows_the_camera_and_its_offset() {
        let cam = camera(0.7, 0.3);
        let t = CameraTracker::new(Vec3::new(0.0, 2.0, 0.0), None, None, None, None, IDENTITY);
        let (rows, pos) = t.place(cam, Vec3::new(5.0, 6.0, 7.0));
        assert!(close(rows[1], cam[1]));
        assert!(close(pos, Vec3::new(5.0, 6.0, 7.0) + cam[1] * 2.0));
    }

    #[test]
    fn yaw_only_takes_the_heading_and_drops_the_pitch() {
        let cam = camera(0.7, 0.4);
        let t = CameraTracker::new(Vec3::ZERO, Some(true), None, Some(true), None, IDENTITY);
        let (rows, _) = t.place(cam, Vec3::ZERO);
        let level = Vec3::new(cam[1].x, cam[1].y, 0.0).normalize();
        assert!(close(rows[1], level), "{:?} vs {level:?}", rows[1]);
        assert!(close(rows[2], Vec3::Z));
    }

    #[test]
    fn yaw_and_pitch_rebuild_the_cameras_forward() {
        let cam = camera(-2.0, 0.5);
        let t = CameraTracker::new(Vec3::ZERO, None, None, Some(true), None, IDENTITY);
        let (rows, _) = t.place(cam, Vec3::ZERO);
        assert!(close(rows[1], cam[1]), "{:?} vs {:?}", rows[1], cam[1]);
    }

    #[test]
    fn nothing_kept_uses_the_start_rotation_twice_and_height_can_be_zeroed() {
        let start = camera(0.5, 0.0);
        let t = CameraTracker::new(Vec3::new(1.0, 0.0, 3.0), Some(true), Some(true), Some(true), Some(true), start);
        let (rows, pos) = t.place(camera(2.0, 0.2), Vec3::new(1.0, 1.0, 9.0));
        assert!(close(rows[1], camera(1.0, 0.0)[1]), "start twice: 0.5 + 0.5 rad");
        assert_eq!(pos.z, 0.0);
        assert!(close(Vec3::new(pos.x, pos.y, 0.0), Vec3::new(1.0 + start[0].x, 1.0 + start[0].y, 0.0)));
    }
}
