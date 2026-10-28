//! The helicopter's blades fading as its rotor spins up: the blade loop of FlyMode's visual
//! step 0086cbf0 (`tf2_core::flightsurface`). The game sends the rotor alpha (mode+0x49c, 1 on
//! entering the flight, down 2 a second to `minRotorAlpha`) to every render instance on each
//! node of `bladeList` or below it (0086ee10 -> 00588050 -> 00587c70), which writes it as that
//! instance's fade (`lod_fade_table_write` 0058b3d0), the same per-instance fade a level piece's
//! LOD fade writes. [game]
//!
//! The port draws that fade as the level's LOD fade is drawn: the mesh's tag carries it and the
//! surface and glow shaders cover that share of the pixels (`lod_coverage`, a 4x4 Bayer screen
//! door). [stand-in] the game's draw-time reader of the fade table is not read
//! (the unpublished notes "Not read"); settles: that reader, or a capture of Blackout's
//! blades in flight. A blade drawn by a `StandardMaterial` cannot take the tag and stays solid.

use bevy::mesh::MeshTag;
use bevy::prelude::*;

use crate::assets::ObjectNodes;
use crate::fx::Glow;
use crate::player::{Player, RobotModel, TuningRes, VehicleModel};
use crate::surface::Surface;

/// A blade's mesh whose material has been given its own fade-enabled copy.
#[derive(Component)]
pub struct BladeFade;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn blade_fade(
    mut commands: Commands,
    player: Single<&Player>,
    tuning: Res<TuningRes>,
    vehicle: Option<Single<&ObjectNodes, (With<VehicleModel>, Without<RobotModel>)>>,
    children: Query<&Children>,
    mut parts: Query<(Option<&MeshMaterial3d<Surface>>, Option<&MeshMaterial3d<Glow>>, Option<&mut MeshTag>, Has<BladeFade>), With<Mesh3d>>,
    mut surfaces: ResMut<Assets<Surface>>,
    mut glows: ResMut<Assets<Glow>>,
) {
    let blades = &tuning.0.forms.fly.blade_list;
    let Some(nodes) = vehicle else { return };
    if blades.is_empty() {
        return;
    }
    // The fade table keeps the last value written: the alpha stays where the last flight left
    // it (the vehicle is hidden in robot form anyway). [game]
    let alpha = player.state.flight_surfaces.rotor_alpha;
    let tag = 0x8000_0000 | (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
    // The named nodes only: `iter_descendants` reaches everything below them.
    for (i, entity) in nodes.entities.iter().enumerate() {
        if !blades.contains(&nodes.name_hashes[i]) {
            continue;
        }
        for part in children.iter_descendants(*entity) {
            let Ok((surface, glow, current, ready)) = parts.get_mut(part) else { continue };
            if !ready {
                let mut entity = commands.entity(part);
                if let Some(m) = surface.and_then(|h| surfaces.get(&h.0)).cloned() {
                    let mut m = m;
                    m.enable_lod_fade();
                    entity.insert(MeshMaterial3d(surfaces.add(m)));
                } else if let Some(m) = glow.and_then(|h| glows.get(&h.0)).cloned() {
                    let mut m = m;
                    m.enable_lod_fade();
                    entity.insert(MeshMaterial3d(glows.add(m)));
                }
                entity.insert((BladeFade, MeshTag(tag)));
                continue;
            }
            if let Some(mut current) = current
                && current.0 != tag {
                current.0 = tag;
            }
        }
    }
}
