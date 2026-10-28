//! The marks his tyres leave (`tf2_core::skidmark`, the game's `TireSkidMark`): told to
//! start and stop as the vehicle's update tells them, a point put down at each tyre's node
//! every `skidPointLength`, and the strip drawn as the game builds it.
//!
//! [game/data] `dull_vc`: texture x material x vertex colour, diffuse lighting and alpha
//! blending. The light rig remains the surface renderer's stand-in.
//! [stand-in] The tyre's node uses its last rendered transform. Triggers and independent
//! wheel contacts use each game update. OPEN (third pass, read 2026-10-20): the game's update (0080afa0)
//! takes the tyre point's place from 0080bc30, which reads the node's world matrix through 005509d0.
//! That getter returns the 3x4 entry of the object's world table (+0x68) after a dirty test (bit in
//! +0x50) that calls 00578360: a copy of the node's local entry (+0x6c) and a walk up its parents, so
//! the game reads the node's matrix as it is at that moment, recomputed on demand, not the last drawn
//! one. The port's GlobalTransform is the last propagated transform. Replacing it needs the car's
//! per-tick pose (the animation's pose, not in this file). settles: that per-tick pose in the port.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use tf2_core::sim::Mode;
use tf2_core::skidmark::{self, SkidMark};

use crate::assets::{ObjectNodes, bevy_to_game, game_to_bevy, image};
use crate::player::{Player, RobotModel, VehicleModel};

struct Shown {
    mark: SkidMark,
    strip: Option<(Entity, Handle<Mesh>)>,
}

#[derive(Resource, Default)]
pub struct SkidMarks {
    marks: Vec<Shown>,
    material: Option<Handle<crate::surface::Surface>>,
}

pub fn run(
    time: Res<Time>,
    data: Res<crate::GameData>,
    player: Single<&Player>,
    car: Option<Single<&ObjectNodes, (With<VehicleModel>, Without<RobotModel>)>>,
    placed: Query<&GlobalTransform>,
    mut marks: ResMut<SkidMarks>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<crate::surface::Surface>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(vehicle), Some(nodes)) = (data.0.vehicle.as_ref(), car.as_deref()) else { return };
    let defs = &vehicle.skid_marks;
    let marks = &mut *marks;
    marks.marks.resize_with(defs.len(), || Shown { mark: SkidMark::default(), strip: None });
    let state = &player.state;
    let now = time.elapsed_secs();
    // Where each mark's tyre is: the node its `tireName` names.
    let tyre = |def: &skidmark::SkidMarkDef| {
        let node = nodes.name_hashes.iter().position(|&name| name == def.tyre)?;
        let at = nodes.entities.get(node).and_then(|&e| placed.get(e).ok())?;
        Some(bevy_to_game(at.translation()))
    };
    if state.mode == Mode::Drive {
        for input in &state.tyre_inputs {
            for (shown,def) in marks.marks.iter_mut().zip(defs) {
            // [data/game] Attachments are rear L/R then front R/L; wheel physics is
            // front L/R then rear L/R. Bind by tireName, never attachment order, through the
            // game's name-to-wheel map (`vehicle::wheel_of_node`): Megatron's marks name his
            // treads (`Tread_Back_L`...) and laid nothing when only the tyres' names counted.
            let Some(wheel)=tf2_core::vehicle::wheel_of_node(def.tyre) else {continue;};
            let wheel_input=input.for_wheel(wheel);
            let contact=input.wheels[wheel].point.z;
            match skidmark::message(&wheel_input) {
                Some(true) => {
                        if let Some(at) = tyre(def) {
                            shown.mark.trigger(at, contact, now);
                        }
                }
                Some(false) => shown.mark.stop(),
                None => {}
            }
            if let Some(at)=tyre(def) {
                shown.mark.update(def,at,contact,wheel_input.grounded,
                    input.wheels[wheel].was_grounded,now);
            }
            }
        }
    } else {
        // Out of the car nothing new skids: [game 0085ed50] the exit sends the stop (9caa723d) to TireSkidMark,
        // whose stop (FUN_007ca780, vtable +0x1c) only clears its going flag. [game 0080bd10, 0080bb30] the
        // marks laid stay: the class has no time fade (skidPointLifetime is read by nothing, docs/particles.md)
        // and a point goes only when a new one takes it, so the drawn marks stay where they are.
        marks.marks.iter_mut().for_each(|shown| shown.mark.stop());
    }

    for (shown, def) in marks.marks.iter_mut().zip(defs) {
        let quads = shown.mark.quads(def);
        if quads.is_empty() {
            if let Some((entity, _)) = &shown.strip {
                commands.entity(*entity).insert(Visibility::Hidden);
            }
            continue;
        }
        let mut positions = Vec::with_capacity(quads.len() * 4);
        let mut uvs = Vec::with_capacity(quads.len() * 4);
        let mut colours = Vec::with_capacity(quads.len() * 4);
        let mut indices = Vec::with_capacity(quads.len() * 6);
        for quad in &quads {
            let first = positions.len() as u32;
            for corner in quad {
                positions.push(game_to_bevy(corner.pos).to_array());
                uvs.push(corner.uv);
                colours.push(corner.colour);
            }
            indices.extend([first, first + 1, first + 2, first + 2, first + 3, first]);
        }
        // The game's normal: straight up.
        let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_indices(Indices::U32(indices));
        match &shown.strip {
            Some((entity, handle)) => {
                let _ = meshes.insert(handle, mesh);
                commands.entity(*entity).insert(Visibility::Inherited);
            }
            None => {
                let material = marks
                    .material
                    .get_or_insert_with(|| {
                        let library = &data.0.library;
                        let source = library.materials.get(&def.material);
                        let texture = source
                            .and_then(|m| m.texture("diffuse_texture1_texture"))
                            .and_then(|id| library.textures.get(&id))
                            .and_then(|t| image(t, false))
                            .map(|i| images.add(i));
                        // [data] Installed Bumblebee skid MAT uses dull_vc, Normal blend,
                        // identity UV transform and white diffuse colour.
                        materials.add(crate::surface::build(source.expect("skid material"),
                            [texture,None,None,None],AlphaMode::Blend).expect("dull_vc shader"))
                    })
                    .clone();
                let handle = meshes.add(mesh);
                let entity = commands
                    .spawn((Mesh3d(handle.clone()), MeshMaterial3d(material), Transform::IDENTITY, NoFrustumCulling, NotShadowCaster))
                    .id();
                shown.strip = Some((entity, handle));
            }
        }
    }
}
