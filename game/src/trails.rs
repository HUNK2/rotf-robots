//! Attack trails from RibbonTrail data, with timed and interrupted-clip messages.
//! Character named trail messages also reach the attached axe object [game 00723393-007233a7, settled 2026-10-20 (wave 24
//! L8 read, the lead checked the bytes)]: after its own ring, the character's message dispatch 00723320 calls each
//! weapon-manager slot's vtable +0x30 (obj +0x238, slots at +0x34) and, when that returns false, broadcasts the message
//! (any type, no gate) to the SLOT OBJECT's own tree (00724940 with EAX = [slot+0xc]). The melee slot's +0x30 is
//! 007bb820 (`xor al,al; ret`: always forwarded); the gun, flame and launcher slots' is 0074d100 (`mov al,1`: never).
//! So the axe object's own listeners (its trails) get the character's messages. (Corrected: the earlier text said the
//! forward went to the character's same ring and only for a Trigger.) Read 2026-10-20: the character's
//! message dispatch 00723320 forwards a `Trigger` message to the listener ring of +0x258's tree (key 4006c641, 00724940,
//! registered by 0117ebd0 from the character ctor 00713300 and the foot IK 0073f1c0) when its +0x258 +0x60 bit 0 is
//! clear. The ring is key 4006c641 (00724940 = message_broadcast_children, 2026-10-20: the broadcast goes
//! to the entries of that key's ring only). Its registrants found so far (00724d40 thunk callers): the character
//! ctor 00713300, foot IK 0073f1c0, the spawner ctor 0073a470, 0073f850, the group ctors 007e9590 / 007ea540, the
//! render set 00802f10, the scene triggers 00803a90 / 0080a330, and the callers in 0082c610, 00880280, 011864a0,
//! 01186930, 01187c40, 011889c0, 01189160, 01189890 (not read). No registrant among those read is the axe.
//! The listener on the axe side is the trail itself [game 0078ff30, settled 2026-10-20]: the RibbonTrail ctor 0078ff30
//! runs script_object_base_ctor 0080de60, which registers its ring node through 0117ebd0 (the key-4006c641 ring of the
//! object it is on), so a trail on the axe object hears what 00723320 forwards to that object's tree.

use crate::assets::{ObjectNodes, game_to_bevy, image};
use crate::camera::Rig;
use crate::fx::Glow;
use crate::player::RobotModel;
use crate::weapons::AxeModel;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::collections::HashMap;
use tf2_core::formats::anim::Clip;
use tf2_core::ribbon::{self, Trail};

struct Shown {
    trail: Trail,
    mesh: Option<(Entity, Handle<Mesh>)>,
}

#[derive(Resource, Default)]
pub struct Trails {
    messages: Vec<(u32, u32)>,
    exits: Vec<(u32, u32)>,
    shown: HashMap<(Entity, usize), Shown>,
    materials: HashMap<u32, Handle<Glow>>,
}

impl Trails {
    /// Everything back to the start for another character (`charmenu.rs`); returns the ribbon
    /// meshes' entities to despawn.
    pub fn reset(&mut self) -> Vec<Entity> {
        let gone = self.shown.values().filter_map(|s| s.mesh.as_ref().map(|(e, _)| *e)).collect();
        *self = Self::default();
        gone
    }

    /// [assumed] Exit keys fire when the leading clip changes, including during a fade.
    /// Read 2026-10-20: an XFade's outgoing clip is advanced in exit-handler mode 1 during the fade
    /// (0053f350, the outgoing `+0x28` with mode 1 unless flag `+0x3c`), and its unpassed keys' exits
    /// run when the fade finishes (0053f3f0, slot 8). A BlendAnimation forwards slot 8 to both children
    /// (0053a440, slot +0x20). The port differs in two places: it fires the previous clip's whole exit
    /// list here at the change (`exits`, from `clip_events`), not as the fade advances, and it does not
    /// gate that list on the clip's `allowEventsOnBlendFromThisAnim` (`Clock.exit_events`). The port's
    /// blend callback (animation.rs l.327-331) feeds the outgoing clip's cues to sound, shakes and
    /// effects, not to trails. settles: BlendAnimation slot 1 (0053a2d0 / 0053a310, the advance with
    /// a mode, not read) and 0053f350's exit handling for the flag `+0x3c`; the port fix would route
    /// outgoing exits through the blend callback.
    pub fn clip_changed(&mut self) {
        self.messages.append(&mut self.exits);
    }
    pub fn clip_events(&mut self, clip: &Clip, from: f32, to: f32) {
        self.exits = ribbon::exit_messages(clip)
            .map(|e| (e.name, e.message))
            .collect();
        self.messages.extend(
            clip.trigger_events
                .iter()
                .filter(|e| e.due(from, to))
                .map(|e| (e.name, e.message)),
        );
    }
}

/// The projectiles' ribbons by (projectile number, ribbon), and their materials.
#[derive(Default)]
pub struct ProjectileTrails {
    shown: HashMap<(u32, usize), Shown>,
    made: HashMap<u32, Handle<Glow>>,
}

/// The ribbons of the projectiles in flight (Bumblebee's micro missile: ten, two per sub-missile;
/// Ironhide's grenade: one): each `RibbonTrail` of the projectile's object, its points the linked
/// nodes' binds carried by the projectile's frame (`Arsenal::projectile_frame`), kept by the
/// projectile's number. Drawn through the global trail list (00791290 / 00791440); when the
/// projectile goes its trail is taken out at once (`ribbon_teardown` 00790e60). [game] [data]
/// The alternate projectiles (`chargeMaxProjectileObj`) are not drawn: none of the roster's has a
/// ribbon. (The mesh building is `draw`, shared with `run`.)
pub fn run_projectiles(
    time: Res<Time>,
    data: Res<crate::GameData>,
    rig: Res<Rig>,
    arsenal: Option<Res<crate::weapons::Arsenal>>,
    mut state: Local<ProjectileTrails>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<Glow>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(arsenal) = arsenal else { return };
    let ProjectileTrails { shown, made } = &mut *state;
    let mut live = Vec::new();
    for (number, round, weapon, grenade) in arsenal.projectiles() {
        let Some(frame) = arsenal.projectile_frame(number) else { continue };
        if round != weapon { continue }
        let Some(index) = data.0.weapons.iter().position(|w| w.name == weapon) else { continue };
        let objects = if grenade { &data.0.grenade_objects } else { &data.0.missile_objects };
        let Some(object) = objects.get(index).and_then(Option::as_ref) else { continue };
        for (ribbon, def) in object.ribbons.iter().enumerate() {
            live.push((number, ribbon));
            let entry = shown.entry((number, ribbon)).or_insert_with(|| Shown { trail: Trail::new(def), mesh: None });
            let points = def.links.iter().map(|name| {
                let node = object.nodes.iter().find(|n| n.name_hash == *name)?;
                Some(game_to_bevy(frame.transform_point3(Vec3::from(node.bind[3]))))
            }).collect::<Option<Vec<_>>>();
            let Some(points) = points else { continue };
            entry.trail.update(def, points, time.delta_secs());
            draw(entry, def, false, game_to_bevy(rig.view.eye), made, &data, &mut commands, &mut meshes, &mut materials, &mut images);
        }
    }
    // A projectile that is gone takes its trails with it, at once. [game 00790e60]
    shown.retain(|key, entry| {
        let keep = live.contains(key);
        if !keep && let Some((entity, _)) = &entry.mesh {
            commands.entity(*entity).despawn();
        }
        keep
    });
}

/// Draws one updated ribbon, shared by `run` (attack trails) and `run_projectiles`: the trail's
/// quads for this eye go into its mesh (made, with its entity, the first time; the material is
/// built once per material hash into `made`); an empty trail, or `hidden`, hides the entity.
/// A material that cannot be built leaves the ribbon undrawn, as before.
#[allow(clippy::too_many_arguments)]
fn draw(
    shown: &mut Shown,
    def: &ribbon::Def,
    hidden: bool,
    eye: Vec3,
    made: &mut HashMap<u32, Handle<Glow>>,
    data: &crate::GameData,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<Glow>,
    images: &mut Assets<Image>,
) {
    let (vertices, indices) = shown.trail.mesh(def, eye);
    if vertices.is_empty() || hidden {
        if let Some((entity, _)) = &shown.mesh {
            commands.entity(*entity).insert(Visibility::Hidden);
        }
        return;
    }
    let mesh = ribbon_mesh(&vertices, indices);
    if let Some((entity, handle)) = &shown.mesh {
        let _ = meshes.insert(handle, mesh);
        commands.entity(*entity).insert(Visibility::Inherited);
        return;
    }
    let material = match made.get(&def.material) {
        Some(material) => material.clone(),
        None => {
            let Some(source) = data.0.library.materials.get(&def.material) else { return };
            let texture = Glow::texture(source)
                .flatten()
                .and_then(|id| data.0.library.textures.get(&id))
                .and_then(|t| image(t, false))
                .map(|i| images.add(i));
            let alpha = match source.word("blendmode") {
                Some(v) if v == tf2_core::formats::hash::crc32(b"Additive") => AlphaMode::Add,
                Some(v) if v == tf2_core::formats::hash::crc32(b"Normal") => AlphaMode::Blend,
                _ => AlphaMode::Opaque,
            };
            let Some(material) = Glow::build(source, texture, alpha) else { return };
            let material = materials.add(material);
            made.insert(def.material, material.clone());
            material
        }
    };
    let handle = meshes.add(mesh);
    let entity = commands
        .spawn((
            Mesh3d(handle.clone()),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            NoFrustumCulling,
            NotShadowCaster,
        ))
        .id();
    shown.mesh = Some((entity, handle));
}

/// A ribbon's quads as a triangle-list mesh, vertices in the trail's own order.
fn ribbon_mesh(vertices: &[ribbon::Vertex], indices: Vec<u32>) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices.iter().map(|v| v.position.to_array()).collect::<Vec<_>>());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vertices.iter().map(|v| v.normal.to_array()).collect::<Vec<_>>());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vertices.iter().map(|v| v.uv).collect::<Vec<_>>());
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Run after transform propagation so histories sample this frame's posed nodes.
pub fn run(
    time: Res<Time>,
    data: Res<crate::GameData>,
    rig: Res<Rig>,
    owners: Query<(
        Entity,
        &ObjectNodes,
        &Visibility,
        Option<&RobotModel>,
        Option<&AxeModel>,
    )>,
    placed: Query<&GlobalTransform>,
    mut trails: ResMut<Trails>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<Glow>>,
    mut images: ResMut<Assets<Image>>,
) {
    let trails = &mut *trails;
    let messages = std::mem::take(&mut trails.messages);
    for (owner, nodes, visible, robot, axe) in &owners {
        let object = if robot.is_some() {
            Some(&data.0.robot)
        } else if let Some(axe) = axe {
            // Each melee weapon's own (Optimus's two blades share one `LuxScriptName`, so one
            // message starts both). [data]
            data.0.melee_weapons.get(axe.index).and_then(|w| w.object.as_ref())
        } else {
            None
        };
        let Some(object) = object else { continue };
        for (index, def) in object.ribbons.iter().enumerate() {
            let shown = trails.shown.entry((owner, index)).or_insert_with(|| Shown {
                trail: Trail::new(def),
                mesh: None,
            });
            for &(name, message) in &messages {
                if name == def.name {
                    shown.trail.message(message);
                }
            }
            let points = def
                .links
                .iter()
                .map(|name| {
                    let i = nodes.name_hashes.iter().position(|hash| hash == name)?;
                    Some(placed.get(nodes.entities[i]).ok()?.translation())
                })
                .collect::<Option<Vec<_>>>();
            let Some(points) = points else { continue };
            shown.trail.update(def, points, time.delta_secs());
            let hidden = *visible == Visibility::Hidden;
            draw(shown, def, hidden, game_to_bevy(rig.view.eye), &mut trails.materials, &data, &mut commands, &mut meshes, &mut materials, &mut images);
        }
    }
}
