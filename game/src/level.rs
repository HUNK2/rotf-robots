//! One of the game's own levels, drawn: every placed model of its packs (a zone alone, or
//! a mission's zone and mission pack: `tf2_core::level_render`), with the materials the
//! character's models use.
//!
//! Visibility masks, object shadow tags, texture animation and SpinnyThing are applied.
//! Mesh chains and authored draw limits follow the native LOD selector. DamageLink stages are hidden
//! at load; activating their replacement geometry/collision on damage is still owed.

use std::sync::Arc;

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use tf2_core::formats::hash::lower33;
use tf2_core::level_render::LevelRender;

use crate::{assets, fx, surface};

/// The zone's models and where they stand. Present only with `--level`.
#[derive(Resource)]
pub struct LevelModels(pub Arc<LevelRender>);

#[derive(Component)]
pub struct MapPresentation {
    masks: Vec<u32>,
    spin: Option<tf2_core::level_render::Spin>,
    base: Transform,
    deferred_stage: bool,
    lod: Option<(Arc<MapLod>, usize)>,
}

struct MapLod {
    centre: Vec3,
    half_extents: Vec3,
    thresholds: Vec<f32>,
    limit: f32,
    blend: bool,
}

/// [game] Visibility masks and SpinnyThing update independently of the owning Instance's
/// destruction visibility. A shown child inherits its owner's visibility.
pub fn update(
    level: Option<Res<LevelModels>>,
    time: Res<Time>,
    camera: Query<(&Transform, &Projection), (With<Camera3d>, Without<MapPresentation>, Without<crate::ocean::OceanReflection>, Without<crate::duel::SecondView>)>,
    mut placed: Query<(&MapPresentation, &mut Transform, &mut Visibility, &Children)>,
    mut tags: Query<&mut bevy::mesh::MeshTag>,
) {
    let Some(level) = level else { return };
    let Ok((camera, projection)) = camera.single() else { return };
    // [assumed] The input to the visibility manager is the draw camera's world position.
    // 005a7935 passes a local pointer in EDX; see the unpublished notes.
    let active = tf2_core::level_render::active_visibility(&level.0.visibility_boxes, assets::bevy_to_game(camera.translation));
    let eye = assets::bevy_to_game(camera.translation);
    let scale = match projection { Projection::Perspective(p) => tf2_core::lod::perspective_scale(p.fov), _ => 1.0 };
    for (settings, mut transform, mut visibility, children) in &mut placed {
        // [game 00554180, 00554830] DamageLink's Object is deferred until replacement: a staged Instance draws its
        // intact models (nothing on a stage breaks it).
        let stage_shown = !settings.deferred_stage;
        let mut shown = stage_shown && settings.masks.iter().all(|mask| mask & active != 0);
        if let Some(spin) = settings.spin {
            let angle = (spin.radians_per_second * time.elapsed_secs() + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            let turn = Quat::from_axis_angle(spin.axis, angle);
            transform.translation = spin.pivot + turn * (settings.base.translation - spin.pivot);
            transform.rotation = turn * settings.base.rotation;
        }
        if let Some((lod, index)) = &settings.lod {
            let distance = tf2_core::lod::distance(transform.to_matrix(), lod.centre, lod.half_extents, eye, scale);
            let selection = tf2_core::lod::select(&lod.thresholds, distance, lod.limit, lod.blend);
            let fade = selection.map_or(0.0, |s| if s.primary == *index { s.primary_fade }
                else if s.secondary == Some(*index) { s.secondary_fade } else { 0.0 });
            shown &= fade > 0.0;
            // [assumed] Screen-door coverage represents native per-instance draw fade.
            // Native blend/depth scheduling is not yet reproduced; see status.md.
            let value = 0x8000_0000 | (fade.clamp(0.0, 1.0) * 255.0).round() as u32;
            for child in children.iter() {
                if let Ok(mut tag) = tags.get_mut(child) && tag.0 != value { tag.0 = value; }
            }
        }
        let next = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != next { *visibility = next; }
    }
}

pub fn spawn(
    mut commands: Commands,
    level: Option<Res<LevelModels>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surfaces: ResMut<Assets<surface::Surface>>,
    mut glows: ResMut<Assets<fx::Glow>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let Some(level) = level else { return };
    let mut cache = assets::Cache::default();
    let mut builder = assets::Builder {
        library: &level.0.library,
        meshes: &mut meshes,
        materials: &mut materials,
        surfaces: &mut surfaces,
        glows: &mut glows,
        images: &mut images,
        bindposes: &mut bindposes,
        cache: &mut cache,
    };
    // The `Sky.` models are drawn since 2026-10-13 by the `Sky` material (`sky.rs`,
    // the unpublished notes "LS-25": forced to the far plane, so the 35 km domes are not
    // clipped at 1500).
    // Barrier cards now use their native distance fade, two texture layers and authored
    // visibility/draw limits. --hide-barriers is only a diagnostic comparison switch.
    // Backdrop shadow casting follows each Object's actual NoShadowTag/ShadowTag.
    let hide_barriers = std::env::args().any(|a| a == "--hide-barriers");
    let lower = |name: &str| name.to_ascii_lowercase();
    let hidden = |name: &str| hide_barriers && lower(name).starts_with("barrier_card");
    let shown: Vec<_> = level.0.placed.iter().filter(|p| !hidden(&p.model)).collect();
    let mut object_clocks = std::collections::HashMap::new();
    // [assumed] A separate CRT-style stream seeded at1 supplies map random-start draws.
    // The original shares rand() with gameplay; its consumed startup stream is unknown.
    let mut random_state = 1u32;
    let mut requests = Vec::new();
    let mut presentation = Vec::new();
    let no_lod = std::env::args().any(|a| a == "--no-level-lod");
    for p in &shown {
        let clock = p.ifl.map(|definition| *object_clocks.entry((p.owner, p.object_offset)).or_insert_with(|| {
            let draw = if definition.random_start {
                random_state = random_state.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
                ((random_state >> 16) & 0x7fff) as i32
            } else { 0 };
            tf2_core::ifl::Clock::new(definition, draw)
        }));
        let id = lower33(&p.model);
        let mesh = level.0.library.meshes.get(&id);
        let lod = mesh.filter(|m| !no_lod && (!m.lods.is_empty() || p.draw_distance < f32::MAX))
            .map(|m| Arc::new(MapLod { centre: Vec3::from_slice(&m.sphere[..3]), half_extents: m.half_extents.into(),
                thresholds: m.lods.iter().map(|(_, d)| *d).collect(), limit: p.draw_distance, blend: p.blend_lod }));
        let ids = std::iter::once(id).chain(mesh.into_iter().flat_map(|m| m.lods.iter().map(|(id, _)| *id)).filter(|_| lod.is_some()));
        for (index, id) in ids.enumerate() {
            requests.push((id, p.world, !p.no_shadow, p.owner, clock, lod.is_some(), p.tint));
            presentation.push((*p, lod.as_ref().map(|l| (l.clone(), index))));
        }
    }
    let spawned = builder.spawn_placed(&mut commands, "level", requests.into_iter());
    for ((model, lod), entity) in presentation.into_iter().zip(&spawned.models) {
        let Some(entity) = entity else { continue };
        // [data] NoReflectionTag: left out of the sea's reflection pass (`ocean.rs`).
        if model.no_reflection { commands.entity(*entity).insert(crate::ocean::NoReflection); }
        if model.visibility_masks.is_empty() && model.spin.is_none() && !model.deferred_stage && lod.is_none() { continue; }
        let w = model.world;
        let affine = Mat4::from_cols(Vec3::new(w[0], w[1], w[2]).extend(0.0),
            Vec3::new(w[3], w[4], w[5]).extend(0.0), Vec3::new(w[6], w[7], w[8]).extend(0.0),
            Vec3::new(w[9], w[10], w[11]).extend(1.0));
        commands.entity(*entity).insert(MapPresentation { masks: model.visibility_masks.clone(), spin: model.spin, base: Transform::from_matrix(affine), deferred_stage: model.deferred_stage, lod });
        if model.deferred_stage { commands.entity(*entity).insert(Visibility::Hidden); }
    }
    info!("level LOD: {} mesh variant roots for {} placements; native chains/draw limits {}", spawned.models.len(), shown.len(), if no_lod { "disabled" } else { "enabled" });
    info!("level presentation: {} visibility boxes, {} masked placements, {} spinning placements, {} tagged no-shadow placements, {} tagged no-reflection placements",
        level.0.visibility_boxes.len(), shown.iter().filter(|p| !p.visibility_masks.is_empty()).count(),
        shown.iter().filter(|p| p.spin.is_some()).count(), shown.iter().filter(|p| p.no_shadow).count(),
        shown.iter().filter(|p| p.no_reflection).count());
    info!("level damage geometry: {} deferred stage models hidden; {} intact linked models retained",
        shown.iter().filter(|p| p.deferred_stage).count(), shown.iter().filter(|p| p.linked && !p.deferred_stage).count());
    info!(
        "level: {} placed models, {} instantiated before visibility/LOD, {} with no mesh; {} part entities: {} under {} Instance entities, {} under the root",
        level.0.placed.len(),
        shown.len(),
        spawned.missing,
        spawned.parts,
        spawned.instance_parts,
        spawned.instances,
        spawned.parts - spawned.instance_parts
    );
}
