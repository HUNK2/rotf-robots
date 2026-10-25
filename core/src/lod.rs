//! Draw-side LOD: 0058a9c0, 00588740 and 0058aab0. See the unpublished notes.
use glam::{Mat4, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    /// Index0 is the named mesh; index n+1 is chain[n].
    pub primary: usize,
    pub secondary: Option<usize>,
    pub primary_fade: f32,
    pub secondary_fade: f32,
}

/// Distance from the eye to the mesh's local box, measured back in WORLD units.
/// [game] The native SIMD lanes all have the same multiplier for one draw view.
pub fn distance(world: Mat4, centre: Vec3, half_extents: Vec3, eye: Vec3, scale: f32) -> f32 {
    let local_eye = world.inverse().transform_point3(eye);
    let nearest = local_eye.clamp(centre - half_extents, centre + half_extents);
    world.transform_point3(nearest).distance(eye) * scale
}

/// [game 00585880] Reference field of view is45 degrees; orthographic uses1.
pub fn perspective_scale(fov_radians: f32) -> f32 {
    (std::f32::consts::FRAC_PI_8).tan() / (fov_radians * 0.5).tan()
}

pub fn select(thresholds: &[f32], distance: f32, limit: f32, blend: bool) -> Option<Selection> {
    // Strictly below the authored limit; equality is culled, as cmpltps at0058ab18.
    if distance.partial_cmp(&limit) != Some(std::cmp::Ordering::Less) { return None; }
    let mut level = 0;
    while level < thresholds.len() && distance > thresholds[level] { level += 1; }
    let fade = 1.0 - ((distance - (limit - limit * 0.1)) / (limit * 0.1)).clamp(0.0, 1.0);
    let mut out = Selection { primary: level, secondary: None, primary_fade: fade, secondary_fade: 0.0 };
    if blend && level > 0 {
        let crossed = thresholds[level - 1];
        let previous = if level > 1 { thresholds[level - 2] } else { 0.0 };
        let q = (distance - crossed) / ((crossed - previous) * 0.2);
        if q < 0.5 {
            out.primary = level - 1;
            out.secondary = Some(level);
            out.secondary_fade = 2.0 * q * fade;
        } else if q < 1.0 {
            out.secondary = Some(level - 1);
            out.secondary_fade = (2.0 - 2.0 * q) * fade;
        }
    }
    Some(out)
}
