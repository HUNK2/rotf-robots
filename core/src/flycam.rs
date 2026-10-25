//! Camera target point for a flying character, read from the game's code
//! (0071a460, flying_vehicle_camera_target_bottom). See docs/flight-camera-exit.md Part A.
//! Pure arithmetic, no engine types.
//!
//! WIRED: `look_drop` is called by `game\src\camera.rs` `target()` for a flying character, so a
//! flyer's look-at is the lower of its root and its holder's bottom. The rest of a flyer's camera
//! is the ordinary DRIVING camera, not a camera of its own: a flying character's `drivingCamera`
//! attribute names a driving-camera settings block whose field set is identical to
//! `bbeeDriveCamera`'s [data, 2026-10-18], and the read class table in `docs/camera.md` has no
//! flight camera. `camera::Target::flying` (the behaviour's byte +0x195, FlightMode) is what
//! that one class does differently for a flyer; it is wired.

use glam::Vec3;

/// The holder's bottom height: `zBottom = holder_height - up_half * up_scale`.
/// [game 0071a460] read at 0x71a46d..0x71a48b: `holder_height` is B+0x134 (`movss xmm0`),
/// `up_half` is [[B+0xa4]+0x54] (`movss xmm1`), `up_scale` is B+0x128 (`mulss`), and the
/// product is subtracted from the height.
/// What the fields are [game, 2026-10-18]: B = the physics holder (owner+0x38), whose current
/// pose is the 12 floats at +0x108 (`docs/physics.md`: the pair sweep starts there, origin at
/// +0x12c), so +0x134 is its origin's z and +0x128 its basis element 8, the up axis's z.
/// `up_half` is the wrapper's +0x54 one dereference from the holder (B+0xa4), which
/// movement_owner_update 00853cb0 writes for the form selected: `*(*(owner+0x38)+0xa4)+0x54 =
/// owner+0x130[form]`, the avatar's half height (`docs/camera.md` audit, 00853a10/006e81e0).
pub fn holder_bottom(holder_height: f32, up_half: f32, up_scale: f32) -> f32 {
    holder_height - up_half * up_scale
}

/// How far a flyer's look-at point drops below its ground-form lift: `target(root, bottom).z -
/// root.z`, with the holder's centre `root.z + up_z * body_offset_z` and its half height
/// `height * 0.5`. [game 0071a460] `up_z` is the holder basis element 8, which is the up
/// axis's z whichever way the 3x3 is stored (the diagonal is shared): `cos(pitch) * cos(roll)`
/// for the flyer's rotation. [game 0071a460] The game's composition is `flyer_point(root,
/// holder_z, up_z, half)`, with `holder_z` the holder body's origin z (B+0x134, a physics pose).
/// [assumed] the port has no holder pose, so it takes `holder_z = root.z + up_z * offsetZ` (the
/// car's look-at composition, 0070abe0). OPEN: 0070abe0 (character_camera_height) takes
/// holderZ - rootZ - wrapper54 * upZ, so holderZ is the holder's committed origin z. Read
/// 2026-10-20: B+0x134 is the 12th float of the holder pose, written by holder_commit_affine
/// 006dc3b0 (`*(param_1+0x134) = in_EAX[11]`; +0x12c..0x134 is the origin, +0x108 the basis).
/// Its caller that matters is physics_world_step 006df7c0 (line 428, the commit branch of the
/// pose pass), which passes the holder's ADVANCED pose (`iVar8+0x198`), a run-time physics
/// result. So the value is not in the executable; OPEN, settles: the physics solver's holder pose
/// for a flyer (`docs/physics.md`, not ported), or a recording of a flying holder's origin z.
/// When this runs [game, 2026-10-18]: 0071a2e0 takes this path when C+0x14c bit 10 is set
/// (the same bit 0071a5c0 tests before answering at all), 0070ab90 is true (DriveMode
/// e2b22eb4 OR FlyMode fb9f3262: its second test), and the vehicle attributes' canFly is
/// set; so in FlyMode for every flyer. Upright the bottom is above the root and this is 0,
/// so an upright flyer is aimed as before; only an inverted one (`up_z` below 0) is lowered.
/// The unknowns are in the unpublished notes, "Phase 2, wave 3".
pub fn look_drop(root: Vec3, up_z: f32, body_offset_z: f32, height: f32) -> f32 {
    let centre = root.z + up_z * body_offset_z;
    let bottom = holder_bottom(centre, height * 0.5, up_z);
    target(root, bottom).z - root.z
}

/// The point the camera looks at for a flyer, from the character's root position and the
/// holder's bottom height.
/// [game 0071a460] x and y are the root's x and y untouched; z is the lower of the root's z
/// and `holder_bottom` (0x71a494 `comiss` root z against the bottom; the root's z is kept
/// when it is not above the bottom, the bottom otherwise).
/// [game 0071a460] No rotation is read: roll and pitch do not move the point.
/// The caller must handle the no-character case (0071a2e0 `character_camera_target_point`
/// uses the root position); this function covers only the flying-character path.
pub fn target(root: Vec3, holder_bottom: f32) -> Vec3 {
    // Written so that a NaN bottom keeps the root z, as the game's `jbe` after `comiss` does.
    let z = if root.z > holder_bottom {
        holder_bottom
    } else {
        root.z
    };
    Vec3::new(root.x, root.y, z)
}

/// The camera target point of a flyer as 0071a460 gives it: the root's x and y, and z the lower of
/// the root's z and `holder_bottom(holder_z, half_height, up_z)`, where `holder_z` is the holder's
/// origin z (B+0x134), `half_height` the avatar's half height ([[B+0xa4]+0x54]) and `up_z` the
/// holder's up z (B+0x128). [game 0071a460, read 2026-10-20] This is the point 007e7830 uses as P
/// for the boundary test (`Fly::boundary_point`). `root` is the character's own origin
/// (`0071a460`'s `K+0x2c` object, +0x34..+0x3c).
pub fn flyer_point(root: Vec3, holder_z: f32, up_z: f32, half_height: f32) -> Vec3 {
    target(root, holder_bottom(holder_z, half_height, up_z))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flyer_point_is_the_root_xy_and_the_lower_of_root_z_and_the_holder_bottom() {
        let root = Vec3::new(1.0, 2.0, 10.0);
        assert_eq!(flyer_point(root, 3.0, 1.0, 0.5), Vec3::new(1.0, 2.0, 2.5), "upright: bottom below root");
        assert_eq!(flyer_point(root, 3.0, -1.0, 0.5), Vec3::new(1.0, 2.0, 3.5), "inverted: bottom is z + half");
        let low = Vec3::new(1.0, 2.0, 1.0);
        assert_eq!(flyer_point(low, 3.0, 1.0, 0.5), low, "a root below the bottom is kept");
    }

    #[test]
    fn root_above_the_holder_bottom_is_lowered_to_it() {
        let t = target(Vec3::new(1.0, 2.0, 10.0), 3.0);
        assert_eq!(t, Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn root_below_the_holder_bottom_is_kept() {
        let t = target(Vec3::new(1.0, 2.0, -1.0), 3.0);
        assert_eq!(t, Vec3::new(1.0, 2.0, -1.0));
    }

    #[test]
    fn xy_is_never_changed() {
        let root = Vec3::new(-4.5, 7.25, 0.5);
        for bottom in [-100.0_f32, 0.0, 0.5, 100.0] {
            let t = target(root, bottom);
            assert_eq!(t.x, root.x);
            assert_eq!(t.y, root.y);
        }
    }

    #[test]
    fn a_nan_bottom_keeps_the_root_height() {
        let t = target(Vec3::new(0.0, 0.0, 4.0), f32::NAN);
        assert_eq!(t.z, 4.0);
    }

    #[test]
    fn holder_bottom_is_height_minus_half_extent_times_scale() {
        assert_eq!(holder_bottom(2.0, 0.5, 1.0), 1.5);
        assert_eq!(holder_bottom(2.0, 0.5, 2.0), 1.0);
    }

    /// Starscream's avatar (height 0.7, offsetZ 0.5): upright the bottom is above the root, so
    /// nothing is lowered; inverted the look-at drops by the gap. [data] [assumed] holder origin
    /// z = root z + up_z * offsetZ (OPEN, see `look_drop`: the game takes the holder's origin z).
    #[test]
    fn look_drop_is_zero_upright_and_lowers_only_when_inverted() {
        let root = Vec3::new(1.0, 2.0, 10.0);
        assert_eq!(look_drop(root, 1.0, 0.5, 0.7), 0.0);
        assert_eq!(look_drop(root, 0.0, 0.5, 0.7), 0.0, "on its side: bottom at the root");
        // Inverted: centre = 10 - 0.5 = 9.5; bottom = 9.5 - 0.35 * (-1) = 9.85, below the root.
        let upside_down = look_drop(root, -1.0, 0.5, 0.7);
        assert!((upside_down - (-0.15)).abs() < 1e-5, "got {upside_down}");
    }
}
