//! The level `Camera` component and the picture-in-picture camera that looks through it. [game]
//! `docs/camera.md` "Camera component and picture-in-picture".
//!
//! `Camera` (registrar 00ad9bd0, factory 00780260 -> 007801c0, constructor 00780110, vtable 00b5eb3c) holds
//! only a field of view and registers itself under the key 0xcbdf8007; it has no update. Its users find it
//! by that key among an Instance's things: the picture-in-picture camera's start (007804a0, the scene camera
//! of `PictureInPictureManager`, vtable 00b5eb20) takes the first one's world transform and field of view.
use glam::Vec3;

use super::scripted::FOV_MESSAGE_SCALE;

/// The key a `Camera` component registers under (00780110, 00578ae0). [game]
pub const CAMERA_KEY: u32 = 0xcbdf_8007;

/// The view a picture-in-picture camera starts with when its Instance holds no `Camera`: identity and 60
/// degrees (007804a0 seeds from 00c02990, the identity, and 0x42700000). [game]
pub const DEFAULT_FOV: f32 = 60.0;

/// The `Camera` component's stored field of view from its `fov` field (007801c0): a float field as it is, an
/// integer field converted, a missing field 0; then times 42.971832, the same factor as the scripted camera's
/// FOV message. [game 007801c0] The field is multiplied as stored. [data] The field holds radians: the
/// `fov` of the Camera components in level missions (`levels\cairo_city\missions\cairo_city_a_hunt\mission.str#0`
/// 1.0472 = 60 degrees; `cairo_pyramids_d_boss#2` 1.39626 = 80; `deepsix_01_a_defend#2` 0.785398 = 45),
/// read with `tf2_lxb dump`, checked 2026-10-20 (batch W3).
pub fn component_fov(field: Option<f32>) -> f32 {
    field.unwrap_or(0.0) * FOV_MESSAGE_SCALE
}

/// What the picture-in-picture start takes from its Instance's things: the world rows and position and the
/// field of view of the `Camera` components among them, else identity at the origin and [`DEFAULT_FOV`].
/// 007804a0 visits every element with no early exit and each `Camera` found overwrites the copy, so the LAST
/// one in the Instance's list wins. [game]
pub fn pip_view(cameras: &[([Vec3; 3], Vec3, f32)]) -> ([Vec3; 3], Vec3, f32) {
    cameras.last().copied().unwrap_or(([Vec3::X, Vec3::Y, Vec3::Z], Vec3::ZERO, DEFAULT_FOV))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fov_field_and_defaults() {
        assert_eq!(component_fov(None), 0.0);
        assert!((component_fov(Some(1.0)) - 42.971832).abs() < 1e-4);
        let (rows, pos, fov) = pip_view(&[]);
        assert_eq!((rows[1], pos, fov), (Vec3::Y, Vec3::ZERO, 60.0));
    }
}
