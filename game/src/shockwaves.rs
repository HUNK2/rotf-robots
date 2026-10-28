//! Devastator's pound rings (`tf2_core::shockwave`, `tf2_core::giant`): each living ring drawn as
//! its flat band (`ShockWave::mesh`, the ring draw `FUN_00806380`) in his `devastator.fx_shockwave`
//! material, faded by the ring's alpha. [game] [data] The material is `emissive`, additive.
//! Grindor's cinematic waves (`tf2_core::energon_wave`, [port design]) are arcs of the same ring
//! in the same material (borrowed from Devastator's pack at load), drawn here alike.
//! [assumed] the ring's fade, which the game writes into the vertex colour (1, 1, 1, alpha), is
//! laid on as the object's alpha: the port's `emissive` path does not read vertex colour. Settles:
//! whether the game's `emissive` technique reads COLOR0 (`docs/shaders.md`).

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use tf2_core::shockwave;

use crate::assets::{game_to_bevy, image};
use crate::fx::Glow;
use crate::player::Player;

/// The ring meshes made so far (reused from frame to frame), and the material as built.
#[derive(Default)]
pub struct Rings {
    shown: Vec<(Entity, Handle<Mesh>, Handle<Glow>)>,
    base: Option<Glow>,
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    data: Res<crate::GameData>,
    player: Single<&Player>,
    arsenal: Option<Res<crate::weapons::Arsenal>>,
    mut state: Local<Rings>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<Glow>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Rings { shown, base } = &mut *state;
    let mut used = 0;
    // His rings flat, Grindor's laid on the ground they run over (`energon_wave::mesh`).
    let mut bands = Vec::new();
    for wave in &player.state.shockwaves {
        for ring in &wave.rings {
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            wave.mesh(ring, &mut vertices, &mut indices);
            bands.push((wave, ring, vertices, indices));
        }
    }
    for energon in arsenal.iter().flat_map(|a| a.energon_waves()) {
        for ring in &energon.ring.rings {
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            tf2_core::energon_wave::mesh(&energon.ring, ring, &energon.ground, &mut vertices, &mut indices);
            bands.push((&energon.ring, ring, vertices, indices));
        }
    }
    for (wave, ring, vertices, indices) in bands {
        if vertices.is_empty() {
            continue;
        }
        let mesh = band_mesh(&vertices, indices);
        let alpha = wave.alpha(ring);
        if let Some((entity, mesh_handle, material)) = shown.get(used) {
            let _ = meshes.insert(mesh_handle, mesh);
            if let Some(mut m) = materials.get_mut(material) {
                m.set_object_alpha(alpha);
            }
            commands.entity(*entity).insert(Visibility::Inherited);
        } else {
            if base.is_none() {
                *base = wave.def.material.and_then(|id| build(&data, id, &mut images));
                if base.is_none() {
                    eprintln!("shockwave: material {:?} not drawable (not loaded, or not a glow technique)", wave.def.material);
                }
            }
            let Some(made) = base.as_ref() else { return };
            let mut material = made.clone();
            material.set_object_alpha(alpha);
            let (mesh_handle, material) = (meshes.add(mesh), materials.add(material));
            let entity = commands
                .spawn((
                    Mesh3d(mesh_handle.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::IDENTITY,
                    NoFrustumCulling,
                    NotShadowCaster,
                ))
                .id();
            shown.push((entity, mesh_handle, material));
        }
        used += 1;
    }
    for (entity, _, _) in shown.iter().skip(used) {
        commands.entity(*entity).insert(Visibility::Hidden);
    }
}

/// His material as the trails build theirs (`trails::draw`): its colour texture, additive by its
/// `blendmode`.
fn build(data: &crate::GameData, id: u32, images: &mut Assets<Image>) -> Option<Glow> {
    let source = data.0.library.materials.get(&id)?;
    let texture = Glow::texture(source)
        .flatten()
        .and_then(|t| data.0.library.textures.get(&t))
        .and_then(|t| image(t, false))
        .map(|i| images.add(i));
    let alpha = match source.word("blendmode") {
        Some(v) if v == tf2_core::formats::hash::crc32(b"Additive") => AlphaMode::Add,
        Some(v) if v == tf2_core::formats::hash::crc32(b"Normal") => AlphaMode::Blend,
        _ => AlphaMode::Opaque,
    };
    Glow::build(source, texture, alpha)
}

fn band_mesh(vertices: &[shockwave::Vertex], indices: Vec<u32>) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices.iter().map(|v| game_to_bevy(v.position).to_array()).collect::<Vec<_>>());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vertices.iter().map(|v| game_to_bevy(v.normal).to_array()).collect::<Vec<_>>());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vertices.iter().map(|v| v.uv.to_array()).collect::<Vec<_>>());
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
