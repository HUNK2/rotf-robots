//! Devastator's vortex (`tf2_core::giant::Vortex`, `sim::vortex_pull`): the boss mission's funnel
//! object drawn where his state puts it (`State::vortex`: its point on his head, its turn with its
//! spin, its scale), and its `SoundPlayer`'s sound. [assumed port design] as the core's: in the
//! game the funnel is a placed object the mission's script triggers; here it is put on him. Its
//! nodes are drawn at rest, all identity (their binds are, but for the render node's 4 mm offset,
//! which is dropped), so the funnel drawn is the hull the core drags and hits with. The sound
//! starts `sound_delay` (5 s [data]) into the funnel's own clip [assumed: that its `ForwardTrigger`
//! and its `AnimBundle` start on the same trigger] at its point, and is stopped when it ends.

use std::sync::Arc;

use bevy::prelude::*;
use tf2_core::formats::scene::ObjectDef;

use crate::assets::{game_to_bevy, model_space_rotation, ObjectNodes};
use crate::player::{Player, TuningRes};

/// The funnel's drawn object, and the object it was made from (a swap of robot makes it again).
#[derive(Component)]
pub struct VortexModel(Arc<ObjectDef>);

/// The funnel's sound while it plays, and whether the event was found at all.
#[derive(Default)]
pub struct Sounding {
    handle: Option<tf2_core::sound::Handle>,
    missing: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    data: Res<crate::GameData>,
    tuning: Res<TuningRes>,
    player: Single<&Player>,
    sound: Option<Res<crate::sound::SoundOut>>,
    mut models: Query<(Entity, &VortexModel, &mut Transform, &mut Visibility, &ObjectNodes)>,
    mut nodes: Query<&mut Transform, Without<VortexModel>>,
    mut sounding: Local<Sounding>,
    mut cache: Local<crate::assets::Cache>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surfaces: ResMut<Assets<crate::surface::Surface>>,
    mut glows: ResMut<Assets<crate::fx::Glow>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
) {
    let vortex = tuning.0.giant.as_ref().and_then(|g| g.vortex.as_ref());
    let object = vortex.and_then(|v| v.object.as_ref()).map(|o| &o.0);
    // One model, of this robot's funnel: another robot's (or none) goes.
    let mut have = false;
    for (entity, model, ..) in &models {
        if object.is_some_and(|o| Arc::ptr_eq(o, &model.0)) {
            have = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
    let (Some(vortex), Some(object)) = (vortex, object) else { return };
    if !have {
        let mut builder = crate::assets::Builder {
            library: &data.0.library,
            meshes: &mut meshes,
            materials: &mut materials,
            surfaces: &mut surfaces,
            glows: &mut glows,
            images: &mut images,
            bindposes: &mut bindposes,
            cache: &mut cache,
        };
        let (model, _) = builder.spawn(&mut commands, object, "vortex");
        commands.entity(model).insert((VortexModel(Arc::clone(object)), Visibility::Hidden));
        // It shows from the next frame, once its nodes exist.
        return;
    }
    let funnel = player.state.vortex;
    for (_, _, mut transform, mut visibility, object_nodes) in &mut models {
        match funnel {
            Some(f) => {
                transform.translation = game_to_bevy(f.tip);
                transform.rotation = model_space_rotation() * f.turn;
                transform.scale = Vec3::splat(f.scale);
                *visibility = Visibility::Inherited;
                for &node in &object_nodes.entities {
                    if let Ok(mut t) = nodes.get_mut(node)
                        && *t != Transform::IDENTITY {
                        *t = Transform::IDENTITY;
                    }
                }
            }
            None => *visibility = Visibility::Hidden,
        }
    }
    let Some(sound) = sound else { return };
    match funnel {
        Some(f) if f.tick >= vortex.sound_delay * tf2_core::formats::anim::TICKS_PER_SECOND => match sounding.handle {
            Some(handle) => sound.move_to(handle, f.tip),
            None if !sounding.missing && vortex.sound != 0 => {
                sounding.handle = sound.start_kept_at(vortex.sound, None, f.tip);
                if sounding.handle.is_none() {
                    sounding.missing = true;
                    eprintln!("vortex: its sound {:08x} is in none of his loaded sound files", vortex.sound);
                }
            }
            None => {}
        },
        // The funnel is gone: its sound stops (with its own fade).
        _ if funnel.is_none() => {
            if let Some(handle) = sounding.handle.take() {
                sound.stop(handle);
            }
        }
        _ => {}
    }
}
