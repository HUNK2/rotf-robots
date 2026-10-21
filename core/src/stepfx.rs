//! FootStep/ClimbStep contacts shared by their visual and sound cues.
//! [game: 0073f520,0073fa20,00718720] Probe the named component, then select its
//! impact preset by the contact material. [game 0073f420] The game's probe is SUBMITTED from the bone's
//! world place read at the FootStep event itself (`object_node_world_place` 0056bee0, ray from z - height
//! +1.5 to -1.5, mask 0x11, probe_submit 006e0a30, queued and resolved later). [assumed] Synchronous
//! final-pose probes replace Lux's asynchronous callbacks (existing sound timing debt in status.md).
//! The cue's one-update delay is the consumers' (`effects.rs` and `sound.rs` `delayed_steps`), from the
//! order of the world update [game 005a8740, 00740d60, 006dedf0]. settles: the pose at the event's dispatch
//! against the final pose used here: find the caller of 0073f420 (not found by `fn.py callers` or find_ptr:
//! the FootStep event's dispatch) and whether the event fires before or after this update's pose blend.
//! [game 0077cd60, 0077bff0] The impact preset by the contact material is the game's: the
//! material registry (005bbc00) gives the surface's preset table, 0077bff0 picks the entry
//! that lists the impact hash (ported as `ImpactPreset::pick` in character.rs), and a miss
//! falls back on `0 - Default` (37f718bf). The contact material's source (FUN_0072d9c0, the
//! surface's hash) is not read here: OPEN, settles: 0072d9c0 and the probe's material field.

use glam::{Affine3A, Mat3, Quat, Vec3};
use crate::{formats::{anim::CLIMB_SIDES, scene::FootstepDef}, sim::World, vehicle::GroundHit};

pub fn footsteps(side: u32, pose: &[Affine3A], feet: &[FootstepDef], world: &impl World) -> Vec<GroundHit> {
    // From 1.5 m above the foot's ground (the bone less `heightFromGround`) to 1.5 m below it.
    // [game 0073f420] The port's earlier z + h to z - h ended where a standing foot's ground is, so
    // the ground of a foot bone a little above its height was out of reach: Long Haul's probe
    // missed in 105 of his 132 FootStep events, Ironhide's in 123 of 146 (`ground_sound_check`).
    feet.iter().filter(|foot| foot.side == side).filter_map(|foot| {
        let ground = Vec3::from(pose.get(foot.node)?.translation) - Vec3::Z * foot.height;
        world.wheel_ray(ground + Vec3::Z * 1.5, ground - Vec3::Z * 1.5)
    }).collect()
}

/// Bumblebee's installed four ClimbStep components have offsetFromWall=0 and
/// spawnSlideEffectOnThisBone=true. Slide therefore probes all four. [data]
pub fn climbing(limb: usize, pose: &[Affine3A], steps: &[(usize, u32)], forward: Vec3, world: &impl World) -> Vec<GroundHit> {
    let Some(&side) = CLIMB_SIDES.get(limb) else { return Vec::new() };
    let direction = forward.normalize_or_zero() * 1.5;
    steps.iter().filter(|(_, s)| limb == 4 || *s == side).filter_map(|&(node, _)| {
        let point = Vec3::from(pose.get(node)?.translation);
        world.wheel_ray(point - direction, point + direction)
    }).collect()
}

/// [game: 0077c180, flags1] +z follows the contact normal; +y uses projected
/// world up, falling back to +y when almost parallel (absolute dot >0.999).
/// Bumblebee's footstep flags0x100 instead preserve the hit object's axes; the
/// arena's static world-frame attachment is represented by identity.
pub fn rotation(flags: u32, normal: Vec3) -> Quat {
    if flags & 1 == 0 { return Quat::IDENTITY }
    let z = normal.normalize_or_zero();
    let reference = if z.z.abs() > 0.999 { Vec3::Y } else { Vec3::Z };
    let x = reference.cross(z).normalize_or_zero();
    let y = z.cross(x).normalize_or_zero();
    Quat::from_mat3(&Mat3::from_cols(x, y, z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Arena, Box3};

    #[test]
    fn foot_contact_uses_the_named_component_and_requires_ground_in_probe_range() {
        let side = 0x439eb7a4;
        let feet = [FootstepDef { node: 0, side, height: 1.5 }];
        let mut pose = [Affine3A::from_translation(Vec3::new(4.0, 5.0, 0.2))];
        let arena = Arena::default();
        let hit = footsteps(side, &pose, &feet, &arena);
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].point, Vec3::new(4.0, 5.0, 0.0));
        assert_eq!(hit[0].normal, Vec3::Z);
        assert!(footsteps(0, &pose, &feet, &arena).is_empty());
        // A foot bone a little above its height from the ground (Long Haul's at 1.66 for 1.64)
        // still finds it. [game 0073f420]
        pose[0].translation.z = 1.6;
        assert_eq!(footsteps(side, &pose, &feet, &arena).len(), 1);
        // More than 1.5 m above its height, the ground is out of reach.
        pose[0].translation.z = 3.1;
        assert!(footsteps(side, &pose, &feet, &arena).is_empty());
    }

    #[test]
    fn climb_contact_hits_wall_surface_and_slide_reaches_each_component() {
        let steps: Vec<_> = CLIMB_SIDES[..4].iter().enumerate().map(|(i, &s)| (i, s)).collect();
        let pose = [Affine3A::from_translation(Vec3::new(0.0, 0.0, 3.0)); 4];
        let arena = Arena { boxes: vec![Box3 { min: Vec3::new(-2.0, 1.0, 0.0), max: Vec3::new(2.0, 2.0, 6.0) }], ..Default::default() };
        let hits = climbing(0, &pose, &steps, Vec3::Y, &arena);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].point, Vec3::new(0.0, 1.0, 3.0));
        assert_eq!(hits[0].normal, Vec3::NEG_Y);
        assert_eq!(climbing(4, &pose, &steps, Vec3::Y, &arena).len(), 4);
        assert!(climbing(0, &pose, &steps, Vec3::X, &arena).is_empty());
        assert!(climbing(5, &pose, &steps, Vec3::Y, &arena).is_empty());
    }

    #[test]
    fn climbing_dust_preserves_world_up_twist_instead_of_using_collision_fx_axes() {
        for normal in [Vec3::X, Vec3::NEG_Y, Vec3::Z] {
            let q = rotation(1, normal);
            assert!((q * Vec3::Z).abs_diff_eq(normal, 1e-6));
            assert!((q * Vec3::Y).abs_diff_eq(if normal == Vec3::Z { Vec3::Y } else { Vec3::Z }, 1e-6));
        }
        assert_eq!(rotation(0x100, Vec3::X), Quat::IDENTITY);
    }
}
