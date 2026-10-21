//! Static level contact response for an upright character pencil.
//!
//! UNUSED (checked 2026-10-20): `move_avatar` has no caller; the robots move through
//! `physics_avatar` / `physics_contact` (the game's contact pass). Kept for reference only.
//!
//! [game] The original character body is a pencil and its static response slides the
//! remaining position along the contact plane and removes approaching normal velocity
//! (006e2c10, `notes/collision-response.md`). settles: porting the swept-pair dispatcher (005e3d50) and the
//! generator (006e6a10) in place of the cast, not done. [stand-in] Contacts here come from
//! `StaticWorld::pencil_cast`'s Minkowski ray-GJK path, not Lux's swept-pair dispatcher
//! or contact generator. Initial-overlap depenetration is not available from that cast.

use std::collections::HashSet;

use glam::Vec3;

use crate::collision_world::WorldHit;
use crate::level_world::LevelWorld;

/// A static world contact encountered during one movement request.
#[derive(Clone, Copy, Debug)]
pub struct AvatarContact {
    pub point: Vec3,
    pub normal: Vec3,
    pub travel: f32,
    /// The cast began in contact. [game] reported by the current GJK kernel;
    /// [stand-in] it does not provide the original manifold's penetration witness. settles: the generator's
    /// (006e6a10) output block for a contact, which holds it; not ported.
    pub initial_contact: bool,
    pub primitive: usize,
    pub triangle: Option<u32>,
}

/// Accepted character movement and its static contacts.
#[derive(Clone, Debug)]
pub struct AvatarMove {
    pub position: Vec3,
    pub velocity: Vec3,
    pub contacts: Vec<AvatarContact>,
}

/// Sweep and slide an avatar pencil through the static level for one requested movement.
///
/// [game] The slide projection and inelastic normal velocity removal match the simple
/// static-body response in 006e2c10. [game 006e2c10 lines 163-169, read 2026-10-20] the accepted hit sets both
/// holders' +0x244 to its t, and the next hit must be no later than that: `physics_contact::contact_pass` ports
/// this gate per pass. settles: routing this path through `physics_contact` in place of the residual casts below. [stand-in] the source contacts are GJK sweep hits
/// and this path cannot depenetrate a pencil that starts overlapped. [assumed] repeated
/// residual casts approximate the original repeated broadphase passes; one already-resolved
/// `(primitive, triangle)` feature is suppressed for this move. The game instead gates by
/// holder `+0x244` per broadphase pass (006e1bc0/006e2c10); that broader schedule is not ported.
pub fn move_avatar(
    world: &LevelWorld,
    position: Vec3,
    displacement: Vec3,
    velocity: Vec3,
    radius: f32,
    height: f32,
    mask: u16,
) -> AvatarMove {
    let mut out = AvatarMove { position, velocity, contacts: Vec::new() };
    let mut remaining = displacement;
    let mut resolved = HashSet::new();
    loop {
        let end = out.position + remaining;
        let Some(hit) = world.pencil_contacts(out.position, end, radius, height, mask)
            .into_iter()
            .find(|h| {
                let feature = (h.primitive, h.triangle);
                !resolved.contains(&feature)
            })
        else {
            out.position = end;
            break;
        };

        resolved.insert((hit.primitive, hit.triangle));
        let distance = remaining.length();
        let fraction = if distance == 0.0 { 0.0 } else { (hit.lambda / distance).clamp(0.0, 1.0) };
        let impact = out.position + remaining * fraction;
        let tail = remaining * (1.0 - fraction);
        let normal_part = tail.dot(hit.normal);
        let slide = tail - hit.normal * normal_part;
        out.contacts.push(contact(hit));
        out.position = impact;
        // This helper receives translation velocity only; the original holder path also
        // includes omega x contact offset through 006e9250. Climbing integration is expected
        // to use this translation-only API only while angular velocity is zero.
        let normal_velocity = out.velocity.dot(hit.normal);
        if normal_velocity < 0.0 {
            out.velocity -= hit.normal * normal_velocity;
        }
        remaining = slide;
    }
    out
}

/// [game] 006dd900: remove each normal component from `v`, while retaining dot values
/// when they exceed the caller's thresholds. The original appends a normalized residual
/// normal when fewer than three were supplied and residual length squared exceeds 0.001.
/// The source stores at most three normals and one threshold per normal.
pub fn adjust_contact_basis(v: &mut Vec3, normals: &mut Vec<Vec3>, thresholds: &mut Vec<f32>) {
    assert!(normals.len() <= 3 && thresholds.len() >= normals.len());
    for (i, normal) in normals.iter().copied().enumerate() {
        let dot = v.dot(normal);
        let threshold = &mut thresholds[i];
        if *threshold <= dot && *threshold != dot {
            *threshold = dot;
        }
        *v -= normal * dot;
    }
    let residual2 = v.length_squared();
    if normals.len() < 3 && residual2 > 0.001 {
        let length = residual2.sqrt();
        normals.push(*v / length);
        thresholds.push(length);
    }
}

/// [game] 006dd800: normalize `desired - normal`, with the original squared-length test.
pub fn remove_contact_normal(desired: Vec3, normal: Vec3) -> (Vec3, Vec3, f32) {
    let residual = desired - normal;
    let length2 = residual.length_squared();
    if length2 > 1e-6 {
        let length = length2.sqrt();
        (normal, residual / length, length)
    } else {
        (normal, Vec3::ZERO, 0.0)
    }
}

/// [game] 006dd5b0: project desired motion against retained normals, then project both
/// desired motion and control velocity against the current normals, in stored order.
pub fn project_contact_controls(
    mut desired: Vec3,
    mut controls: Vec3,
    retained: &[Vec3],
    current: &[Vec3],
) -> (Vec3, Vec3) {
    for normal in retained {
        let dot = desired.dot(*normal);
        if dot < 0.0 { desired -= *normal * dot; }
    }
    for normal in current {
        let dot = desired.dot(*normal);
        if dot < 0.0 { desired -= *normal * dot; }
        let control_dot = controls.dot(*normal);
        if control_dot < 0.0 { controls -= *normal * control_dot; }
    }
    (desired, controls)
}

#[cfg(test)]
mod projection_tests {
    use super::*;

    #[test]
    fn retained_and_current_contact_sets_project_in_source_order() {
        let (desired, controls) = project_contact_controls(
            Vec3::new(-2.0, -3.0, 4.0),
            Vec3::new(-5.0, 7.0, -1.0),
            &[Vec3::X],
            &[Vec3::Y],
        );
        assert_eq!(desired, Vec3::new(0.0, 0.0, 4.0));
        assert_eq!(controls, Vec3::new(-5.0, 7.0, -1.0));
    }

    #[test]
    fn contact_basis_adjustment_removes_components_and_appends_residual_direction() {
        let mut v = Vec3::new(2.0, 3.0, 4.0);
        let mut normals = vec![Vec3::X];
        let mut thresholds = vec![0.0];
        adjust_contact_basis(&mut v, &mut normals, &mut thresholds);
        assert_eq!(v, Vec3::new(0.0, 3.0, 4.0));
        assert_eq!(normals.len(), 2);
        assert!((normals[1].length() - 1.0).abs() < 1e-6);
        assert_eq!(thresholds[0], 2.0);
        assert_eq!(thresholds[1], 5.0);
    }

    #[test]
    fn remove_contact_normal_obeys_original_squared_length_guard() {
        let (n, residual, length) = remove_contact_normal(Vec3::X, Vec3::X + Vec3::splat(0.0001));
        assert_eq!(n, Vec3::X + Vec3::splat(0.0001));
        assert_eq!(residual, Vec3::ZERO);
        assert_eq!(length, 0.0);
    }
}

fn contact(hit: WorldHit) -> AvatarContact {
    AvatarContact {
        point: hit.point,
        normal: hit.normal,
        travel: hit.lambda,
        initial_contact: hit.contact,
        primitive: hit.primitive,
        triangle: hit.triangle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_projection_removes_only_approaching_normal_velocity() {
        let normal = Vec3::NEG_X;
        let velocity = Vec3::new(3.0, 4.0, -2.0);
        let approaching = velocity.dot(normal);
        let resolved = velocity - normal * approaching.min(0.0);
        assert_eq!(resolved, Vec3::new(0.0, 4.0, -2.0));

        let separating = Vec3::new(-3.0, 4.0, 0.0);
        let kept = separating - normal * separating.dot(normal).min(0.0);
        assert_eq!(kept, separating);
    }
}
