//! Bumblebee's robot-form weapons: the trigger, the heat, what is fired and what it hits.
//! The rules and numbers are `tf2_core::weapons` (the game's code and data): the trigger
//! comes from the firing state of the control state machine, the aim point is the weapon
//! manager's, and a gun's round is traced from the camera as the original traces it. This
//! file traces them through the arena and scouts, plays their sounds and draws them.
//!
//! Missiles and grenades are drawn with their own models (`draw`), their emitters, lights and
//! ribbons ride them (`effects`, `trails`); the tracer lines and the `--bodies` balls and rings
//! are the port's debug drawing.
//! The gun on his arm follows `tf2_core::gun`: his weapon layer's clips and the weapon
//! object's own. The launcher's lock-on and its missile's guidance are `tf2_core::lockon`
//! and `weapons::Missile`; its explosion is `weapons::MissileBlast`. The car's gun is the
//! same rules with the car's own firing state, its wait after the change and its two
//! barrels (`Car`).

use bevy::math::Affine3A;
use bevy::prelude::*;
use tf2_core::character::CharacterData;
use tf2_core::gun::Clips;
use tf2_core::turret::State;
use tf2_core::weapons;

pub use tf2_core::arsenal::*;

use crate::GameData;
use crate::animation::{AnimLibrary, ObjectBlend};
use crate::assets::{game_to_bevy, model_space_rotation, ObjectNodes};
use crate::player::{Player, RobotModel, VehicleModel};

/// The robot's weapons (`tf2_core::arsenal`), as the game's resource.
#[derive(Resource, Deref, DerefMut)]
pub struct Arsenal(pub tf2_core::arsenal::Arsenal);

/// The clips the gun's rules run on: his weapon layer's set with its entry rules, and the
/// clips the weapon object carries (one object for the cannon and the launcher).
pub fn gun_clips<'a>(library: &'a AnimLibrary, data: &'a CharacterData) -> Clips<'a> {
    Clips {
        arm: library.robot.sets.get(WEAPON_LAYER_SET).map_or(&[], Vec::as_slice),
        entries: library.robot.incoming.get(WEAPON_LAYER_SET).map_or(&[], Vec::as_slice),
        object: data.weapon_animations.first().map_or(&[], Vec::as_slice),
    }
}


/// A weapon's own model (the game's object, from his pack), spawned once for each weapon:
/// which weapon it is, and the node of the robot it hangs from (`startBoneAttachment`,
/// which `FUN_007a6380` looks up on the owner). [data]
#[derive(Component)]
pub struct WeaponModel {
    /// The first weapon of its list, and every weapon of the list (names, and places in the
    /// manager's list: `data.weapons`, the manager's own order). [game 00713180, 007b0f10, read 2026-10-20]
    /// `character_weapons_setup` views the character data's `weapons` field (hash 0x520ebbe1) and the fill
    /// walks that view's elements in order (`view_element` 00573ad0, at most 8); for each element's built
    /// object it numbers EVERY Weapon component it finds (007bb780 then 00578270, a running slot counter
    /// stored at the weapon's +0x30, l.~88-106). So the elements are in the list's order [game]; [assumed]
    /// within one element holding several Weapon components, the scene chain's order is the data's. settles: the
    /// same question as `Held::slot` (the Scene hash's insertion order for Weapon components; the inserter is not found).
    pub name: u32,
    pub names: Vec<u32>,
    pub indices: Vec<usize>,
    pub bone: u32,
    /// The weapon whose object clips pose it (`data.weapon_animations`).
    pub weapon: usize,
}

/// The node of his a weapon object is hung on: where the pull-out's (and every other arm
/// clip's) `WeaponAttachmentEvent` for one of its weapons sits, by name or by its place in the
/// manager's list (`FUN_007b4680` -> `FUN_007b4f20`): `WeaponAttachment_Arm_R` for Bumblebee,
/// `ObjectAttachment_Hand_R` / `_L` for Optimus's right and left pairs. With no such event,
/// its `startBoneAttachment` (where the manager links it first, `007b2b40`). [game] [data]
fn hang_node(model: &WeaponModel, arm: &[tf2_core::formats::anim::AnimRef]) -> u32 {
    use tf2_core::formats::anim::WeaponEventKind;
    use tf2_core::formats::hash::crc32;
    let pull_out = crc32(b"Trans_R2W");
    let mut clips: Vec<_> = arm.iter().filter_map(|r| Some((crc32(r.id.as_bytes()), r.clip.as_ref()?))).collect();
    clips.sort_by_key(|(id, _)| *id != pull_out);
    clips
        .iter()
        .flat_map(|(_, clip)| &clip.weapon_events)
        .find(|event| match event.kind {
            WeaponEventKind::Attach { weapon, index, .. } => {
                if weapon != 0 { model.names.contains(&weapon) } else { index >= 0 && model.indices.contains(&(index as usize)) }
            }
            _ => false,
        })
        .map_or(model.bone, |event| event.node)
}

/// The node his hand weapons hang from while they are out. `startBoneAttachment` is only
/// where a weapon is linked when it is made (the cannon's is `ObjectAttachment_Hand_R`);
/// the clips that bring the gun out, change it and fire it (`Trans_R2W`, `Weapon_Switch`,
/// `Weapon_Recoil`) each carry a `WeaponAttachmentEvent` for the cannon and one for the
/// launcher on this node's channel, and the event's handler links the weapon's object to
/// the node the event sits on (`FUN_007b4f20`) and unhides it. [data] [game]
const WEAPON_NODE: &str = "WeaponAttachment_Arm_R";

/// Hangs each weapon's model on the robot's weapon node with the node's own axes: the
/// original's link (`lux::LinkConstraint`, update `FUN_0056c2f0`) gives the object the
/// node's matrix, and the manager makes it with no offset (`FUN_007b4f20` passes the
/// identity). [game] The node has a channel of its own in the weapon clips, lying along
/// the forearm, and its roll (11 degrees in `Weapon_Idle`) is the tilt the gun's parts are
/// modelled with, taken off again. A weapon the robot has no node for (the car's gun: its
/// `startBoneAttachment` is on the car) is left unattached and hidden.
pub fn attach_models(
    mut commands: Commands,
    robot: Single<&ObjectNodes, With<RobotModel>>,
    vehicle: Option<Single<&ObjectNodes, (With<VehicleModel>, Without<RobotModel>)>>,
    library: Res<AnimLibrary>,
    models: Query<(Entity, &WeaponModel)>,
    mut arsenal: ResMut<Arsenal>,
) {
    let weapon_node = tf2_core::formats::hash::crc32(WEAPON_NODE.as_bytes());
    let node_of = |name: u32| robot.name_hashes.iter().position(|&hash| hash == name);
    let arm = library.robot.sets.get(WEAPON_LAYER_SET).map_or(&[][..], Vec::as_slice);
    // Each weapon of the model's list is taken to hang where its model is (the muzzle's
    // fallback, `Arsenal::barrel`).
    let mut hung = |model: &WeaponModel, node: u32| {
        for &name in &model.names {
            arsenal.hung.insert(name, node);
        }
    };
    for (entity, model) in &models {
        // The node its events name (falling back to his right arm's weapon node, as before,
        // for a weapon whose node he does not have). [game] [data]
        let events = hang_node(model, arm);
        if let Some(node) = node_of(events).filter(|_| node_of(model.bone).is_some()) {
            commands.entity(robot.frame(node)).add_child(entity).insert(Transform::IDENTITY);
            hung(model, events);
            continue;
        }
        if node_of(model.bone).is_none() {
            // A car-form weapon hangs on a node of the car (`WheelWell_Front_R`, the
            // specials' Cannon_Tip, Back_Window_L...), by the same link. [game 007a6380]
            let on_car = vehicle.as_ref().and_then(|car| Some(car.frame(car.name_hashes.iter().position(|&hash| hash == model.bone)?)));
            if let Some(node) = on_car {
                commands.entity(node).add_child(entity).insert(Transform::IDENTITY);
                hung(model, model.bone);
            }
            continue;
        }
        let Some(node) = node_of(weapon_node) else { continue };
        commands.entity(robot.frame(node)).add_child(entity).insert(Transform::IDENTITY);
        hung(model, weapon_node);
    }
}

/// Shows the weapon's object when the gun's rules have it shown (the event of the arm's
/// pull-out clip unhides it; the end of its own put-away clip hides it), and poses it by
/// the clip of its own that is playing. His cannon and launcher are ONE object in the
/// original, so one model is drawn for both: the first weapon's. [game] [data]
pub fn show_models(
    time: Res<Time>,
    arsenal: Res<Arsenal>,
    data: Res<GameData>,
    player: Single<&Player>,
    mut models: Query<(Entity, &WeaponModel, &ObjectNodes, &mut Visibility)>,
    mut transforms: Query<&mut Transform, Without<WeaponModel>>,
    mut effects: ResMut<crate::effects::Effects>,
    mut states: Local<std::collections::HashMap<Entity, ((u32, f32), ObjectBlend)>>,
    tuning: Res<crate::player::TuningRes>,
) {
    // The robot is drawn on foot and in a flight as the robot (OptimusFlight, MegatronFlight):
    // nothing in the weapon code hides a gun by form, so one that is shown (OptimusFlight's,
    // which always appear) stays drawn on his hand in flight. [game 007a7150, 007a7750] [data]
    let robot_drawn = !player.state.is_vehicle() || player.state.flies_as_robot(&tuning.0);
    // The object of the weapon in hand is out, and with it its child's in his other hand (the
    // weapon's show, hide and clip slots pass to the child: `007a7630`, `007a7750`, `007ab0f0`...).
    // [game] The child's object starts the same clips by id (both lists carry `Idle_WeaponA`...). [data]
    let shown = arsenal.shown_names();
    let gun = &arsenal.gun;
    for (entity, model, nodes, mut visibility) in &mut models {
        let (last, blend) = states.entry(entity).or_default();
        let clips_of = data.0.weapon_animations.get(model.weapon).map_or(&[][..], Vec::as_slice);
        // The car's weapon is shown while it is in hand, which is while his control mode
        // is the car's (`FUN_007b43b0` -> `FUN_007b2c60` -> the weapon's slot `+0x24`);
        // it hangs on the car, which is drawn only in the car's form. [game]
        if arsenal.is_car_weapon(model.name) {
            *visibility = if player.state.car_mode { Visibility::Inherited } else { Visibility::Hidden };
            // Hidden out of the car: the same hide as a gun's (weapon_hide 007a7750 stops its emitters).
            if matches!(*visibility, Visibility::Hidden) {
                effects.object_hidden(nodes);
            }
            continue;
        }
        // A special weapon's object follows the special's clips (`show_special_models`).
        if arsenal.special_object(&model.names).is_some() && !shown.iter().any(|&name| name != 0 && model.names.contains(&name)) {
            continue;
        }
        let mine = shown.iter().any(|&name| name != 0 && model.names.contains(&name));
        // The scope hides the weapon in hand's object, and its child's with it (00721ce0). [game]
        *visibility = if mine && gun.shown && robot_drawn && !arsenal.scope_hidden { Visibility::Inherited } else { Visibility::Hidden };
        // The game's hide of the object (weapon_hide 007a7750, the put-away and the scope) stops its
        // emitters: 005790a0(1) -> each emitter's listener -> particle_emitter_stop. [game] Not the form
        // hide: the game does not hide a gun by form (see above), so robot_drawn is left out.
        if !mine || !gun.shown || arsenal.scope_hidden {
            effects.object_hidden(nodes);
        }
        if !mine {
            continue;
        }
        let playing = gun.object.and_then(|object| {
            let r = clips_of.iter().find(|r| tf2_core::formats::hash::crc32(r.id.as_bytes()) == object.id)?;
            Some((r, r.clip.as_ref()?, object.tick))
        });
        let pose = blend.pose(clips_of, gun.object.map_or(0, |o| o.id),
            gun.object.map_or(0.0, |o| o.tick), time.delta_secs(), nodes);
        // The clip's effect events (the overheat clip's smoke), on the gun's own nodes.
        let id = playing.as_ref().map_or(0, |_| gun.object.map_or(0, |object| object.id));
        if last.0 != id {
            effects.thing_clip_changed(nodes);
            if std::env::var_os("TF_DBG").is_some() {
                let name = |id: u32| clips_of.iter().find(|r| tf2_core::formats::hash::crc32(r.id.as_bytes()) == id).map(|r| r.id.clone());
                eprintln!("weapon model {}: object clip {:?} (asked {:?}), shown {}", model.weapon, name(id),
                    gun.object.map(|o| name(o.id).unwrap_or(format!("{:#010x} not in its clips", o.id))), gun.shown);
            }
        }
        if let Some((_, clip, tick)) = playing {
            // Keep the previous phase across a loop: to < from processes its tail,
            // including the overheat smoke's stop key just before the loop boundary.
            let from = if last.0 == id { last.1 } else { 0.0 };
            effects.thing_clip_events(clip, from, tick, nodes);
            *last = (id, tick);
        } else {
            *last = (0, 0.0);
        }
        // Node 0 is the object itself, which hangs on his arm's node: a clip's root is
        // not put on it (the link overwrites the object's own matrix in the original).
        for (node, transform) in nodes.entities.iter().zip(&pose).skip(1) {
            if let Ok(mut placed) = transforms.get_mut(*node) {
                *placed = *transform;
            }
        }
    }
}

/// A melee weapon's own model (the game's object, from his pack): which of
/// `CharacterData::melee_weapons` it is, and the node of his it hangs from now (CRC-32 of the
/// node's name).
#[derive(Component)]
pub struct AxeModel {
    pub index: usize,
    pub hung: u32,
}

/// Shows the axe when its rules have it shown, hangs it on the node its last event sat on
/// (the link is the one the guns have, `FUN_007b4f20`: the node's own axes, no offset)
/// and poses it by the clip of its own that is playing. [game] [data]
pub fn show_axe(
    time: Res<Time>,
    mut commands: Commands,
    arsenal: Res<Arsenal>,
    data: Res<GameData>,
    player: Single<&Player>,
    robot: Single<&ObjectNodes, With<RobotModel>>,
    mut models: Query<(Entity, &mut AxeModel, &ObjectNodes, &mut Visibility), Without<RobotModel>>,
    mut transforms: Query<&mut Transform, Without<AxeModel>>,
    mut blends: Local<std::collections::HashMap<Entity, ObjectBlend>>,
    tuning: Res<crate::player::TuningRes>,
) {
    // As for the guns (`show_models`): drawn while the robot is. [assumed: no melee in flight,
    // so no visible change] settles: whether a flyer's melee axe is ever shown in flight (the melee slots'
    // WeaponAttachment events while flying, 007b4680; the six flyers' melee data)
    let robot_drawn = !player.state.is_vehicle() || player.state.flies_as_robot(&tuning.0);
    // Each model by its own weapon's state (Optimus's two blades apart). [game 007b4680]
    for (entity, mut model, nodes, mut visibility) in &mut models {
        let (Some(weapon), Some(axe)) = (data.0.melee_weapons.get(model.index), arsenal.axes.get(model.index)) else { continue };
        let blend = blends.entry(entity).or_default();
        if axe.node != 0 && model.hung != axe.node
            && let Some(node) = robot.name_hashes.iter().position(|&hash| hash == axe.node) {
            commands.entity(robot.frame(node)).add_child(entity).insert(Transform::IDENTITY);
            model.hung = axe.node;
        }
        let hung = model.hung == axe.node && axe.node != 0;
        *visibility = if axe.shown && hung && robot_drawn { Visibility::Inherited } else { Visibility::Hidden };
        let pose = blend.pose(&weapon.clips, axe.object.map_or(0, |o| o.id), axe.object.map_or(0.0, |o| o.tick), time.delta_secs(), nodes);
        // As for the gun: node 0 is the object itself, which the link places.
        for (node, transform) in nodes.entities.iter().zip(&pose).skip(1) {
            if let Ok(mut placed) = transforms.get_mut(*node) {
                *placed = *transform;
            }
        }
    }
}

/// Shows each special weapon's object while the special's events have it shown, hangs it on the
/// node its last event sat on and poses it by its own clip, as `show_axe` does the melee weapon
/// (the same link, `FUN_007b4f20`). [game] [data] Megatron's fusion arm comes out on his right
/// hand in `SpecialAttack_Intro` and folds away in `SpecialAttack_Exit`. It is drawn in every
/// form [assumed: the robot is what is drawn in Megatron-flight's flight, so it is not hidden there
/// as the held guns are; whether the game hides it in flight is unread] settles: the flight clips' WeaponAttachment
/// events for the special object (007b4680) and whether they hide it (`weapon_hide` 007a7750 for a special, not read).
pub fn show_special_models(
    time: Res<Time>,
    mut commands: Commands,
    arsenal: Res<Arsenal>,
    data: Res<GameData>,
    robot: Single<&ObjectNodes, With<RobotModel>>,
    mut models: Query<(Entity, &WeaponModel, &ObjectNodes, &mut Visibility)>,
    mut transforms: Query<&mut Transform, Without<WeaponModel>>,
    mut states: Local<std::collections::HashMap<Entity, (u32, ObjectBlend)>>,
) {
    let shown = arsenal.shown_names();
    for (entity, model, nodes, mut visibility) in &mut models {
        let Some((object, index)) = arsenal.special_object(&model.names) else { continue };
        // The weapon in hand keeps `show_models` (should a special ever be the weapon in hand).
        if shown.iter().any(|&name| name != 0 && model.names.contains(&name)) {
            continue;
        }
        let (hung, blend) = states.entry(entity).or_default();
        if object.node != 0 && *hung != object.node
            && let Some(node) = robot.name_hashes.iter().position(|&hash| hash == object.node) {
            commands.entity(robot.frame(node)).add_child(entity).insert(Transform::IDENTITY);
            *hung = object.node;
        }
        *visibility = if object.shown && *hung == object.node && object.node != 0 { Visibility::Inherited } else { Visibility::Hidden };
        let clips = data.0.weapon_animations.get(index).map_or(&[][..], Vec::as_slice);
        let pose = blend.pose(clips, object.object.map_or(0, |o| o.id), object.object.map_or(0.0, |o| o.tick), time.delta_secs(), nodes);
        // As for the gun: node 0 is the object itself, which the link places.
        for (node, transform) in nodes.entities.iter().zip(&pose).skip(1) {
            if let Ok(mut placed) = transforms.get_mut(*node) {
                *placed = *transform;
            }
        }
    }
}

/// How many deployed turrets of one launcher can have their model drawn at once (`TurretModel`);
/// past that the cube is drawn. Both turrets' `maxTurretsAtOnce` is below it. [data]
pub const TURRET_POOL: usize = 4;
/// How many models of one launcher's missile are made at the start (`MissileModel`). The game has
/// no such pool: each shot builds a new instance of the projectile's object [game 007aaeb0], so
/// the port makes one more model whenever every one is carrying a missile (`PoolDemand`,
/// `grow_pools`); the new one shows from the next frame. [port]
pub const MISSILE_POOL: usize = 16;

/// One of a turret launcher's pool of deployed-turret models (`deployableTurretObj`), hidden
/// until `draw_turrets` stands a turret of `name` with it. `weapon` is the launcher's index in
/// his weapon list (its clips: `CharacterData::turret_clips`).
#[derive(Component)]
pub struct TurretModel {
    pub name: u32,
    pub weapon: usize,
    pub slot: usize,
}

/// One of a missile launcher's pool of missile models (`missileProjectileObj`), bound to the
/// missile it carries by its number while that flies, so a model never jumps to another missile.
#[derive(Component)]
pub struct MissileModel {
    pub name: u32,
    /// Its number in the pool (for a reader of the world; nothing chooses by it).
    #[allow(dead_code)]
    pub slot: usize,
    pub carrying: Option<u32>,
}

/// The projectiles `draw` found no free model for this frame: (weapon name, grenade). `grow_pools`
/// makes one model for each.
#[derive(Resource, Default)]
pub struct PoolDemand(pub Vec<(u32, bool)>);

/// The materials and images the grown models share. [port]
#[derive(Resource, Default)]
pub struct PoolCache(pub crate::assets::Cache);

const WEAPON_ROTATOR: u32 = tf2_core::formats::hash::crc32(b"WeaponRotator");

/// Draws each deployed turret with its own model and clips: `Compact_Idle` in flight, `Deploy`
/// from the landing (its 90 / 86 ticks are the deploy time), `Idle` once active [data: the
/// DeployableTurret's animOnStart / animOnLand / animIdle]. The base keeps the heading it was
/// thrown along and the rotator's yaw and pitch turn its `WeaponRotator` bone (the TurretRotator's
/// `turretBoneName`) [data], its world turn set by them [game 00813e30, see the code].
/// Once its gun has fired, the gun's last clip (`Fire` at each firing, `Overheat`) plays and is
/// held at its end, as the object's clip player does with nothing to play after it [game, see
/// `Gear::clip`]; `UnDeploy` is not played (nothing in the port takes a turret away but the cap
/// and the owner's death, which remove it at once). A turret with no model left in the pool is
/// drawn as the cube.
pub fn draw_turrets(
    arsenal: Option<ResMut<Arsenal>>,
    data: Res<crate::GameData>,
    mut models: Query<(&TurretModel, &ObjectNodes, &mut Transform, &mut Visibility)>,
    mut nodes: Query<&mut Transform, Without<TurretModel>>,
    mut gizmos: Gizmos,
) {
    let Some(mut arsenal) = arsenal else { return };
    let mut taken: Vec<(u32, usize)> = Vec::new();
    // Each drawn turret's barrel, where its gun's rounds leave from (`Gear::muzzle`), by turret.
    let mut muzzles: Vec<Option<Vec3>> = vec![None; arsenal.turrets.len()];
    for (index, (body, gear)) in arsenal.turrets.iter().zip(&arsenal.gear).enumerate() {
        let mut drawn = false;
        for (model, object, mut transform, mut visibility) in &mut models {
            if model.name != gear.weapon || taken.contains(&(model.name, model.slot)) {
                continue;
            }
            taken.push((model.name, model.slot));
            transform.translation = game_to_bevy(body.position);
            transform.rotation = model_space_rotation() * Quat::from_rotation_z(gear.heading);
            *visibility = Visibility::Inherited;
            drawn = true;
            let (id, tick, looping) = match body.state {
                State::Flying => ("Compact_Idle", arsenal.turret_clock * 60.0, true),
                State::Deploying => ("Deploy", body.deployed_for() * 60.0, false),
                // The gun's last clip, held at its end; `Idle` before the first.
                State::Active | State::Removed => match gear.clip {
                    Some((clip, since)) => (clip, (arsenal.turret_clock - since) * 60.0, false),
                    None => ("Idle", arsenal.turret_clock * 60.0, true),
                },
            };
            let clips = data.0.turret_clips.get(model.weapon).map_or(&[][..], Vec::as_slice);
            let mut pose = match clips.iter().find(|r| r.id == id).and_then(|r| r.clip.as_ref()) {
                Some(clip) => crate::animation::sample(clip, looping, tick, &object.name_hashes, &object.rest),
                None => object.rest.clone(),
            };
            // The rotator SETS the bone's world turn, whatever the clip gives it: Rz(yaw) after
            // Rx(-pitch) from the identity (trackParent false), then the bone's local pose is that
            // times the inverse of its parent's world (`base_pose_set_local` 00545850). [game
            // 00813e30: rotate_z_matrix(+0x70), rotate_x_matrix(-(+0x74)), inverse_affine3 and
            // compose_affine; the order checked against the aim formula of the same function]
            if let Some(i) = object.name_hashes.iter().position(|&h| h == WEAPON_ROTATOR) {
                let mut parent = Quat::from_rotation_z(gear.heading);
                let mut chain = Vec::new();
                let mut at = object.parents.get(i).copied().flatten();
                while let Some(p) = at {
                    chain.push(p);
                    at = object.parents.get(p).copied().flatten();
                }
                for &p in chain.iter().rev() {
                    parent *= pose[p].rotation;
                }
                let world = Quat::from_rotation_z(gear.rotator.yaw) * Quat::from_rotation_x(-gear.rotator.pitch);
                pose[i].rotation = (parent.inverse() * world).normalize();
            }
            // The gun's FireDummy in the posed object (its parents' poses composed), turned by the base's
            // heading and set on its place: the pods' mouth as they stand this frame. [data: the node]
            let barrel = gear.gun.as_ref().map_or(0, |gun| gun.barrel);
            if let Some(i) = object.name_hashes.iter().position(|&h| h == barrel && barrel != 0) {
                let mut place = pose[i].compute_affine();
                let mut at = object.parents.get(i).copied().flatten();
                while let Some(p) = at {
                    place = pose[p].compute_affine() * place;
                    at = object.parents.get(p).copied().flatten();
                }
                muzzles[index] = Some(body.position + Quat::from_rotation_z(gear.heading) * Vec3::from(place.translation));
            }
            for (entity, local) in object.entities.iter().zip(&pose) {
                if let Ok(mut node) = nodes.get_mut(*entity) {
                    *node = *local;
                }
            }
            break;
        }
        if !drawn {
            let at = game_to_bevy(body.position + Vec3::Z * 0.5);
            let color = if body.state == State::Active { Color::srgb(1.0, 0.45, 0.1) } else { Color::srgb(0.95, 0.9, 0.2) };
            gizmos.cube(Transform::from_translation(at).with_scale(Vec3::new(0.8, 0.8, 1.0)), color);
        }
    }
    for (model, _, _, mut visibility) in &mut models {
        if !taken.contains(&(model.name, model.slot)) {
            *visibility = Visibility::Hidden;
        }
    }
    for (gear, muzzle) in arsenal.gear.iter_mut().zip(muzzles) {
        if muzzle.is_some() {
            gear.muzzle = muzzle;
        }
    }
}

/// How many models of one grenade weapon are made at the start (`GrenadeModel`); more are made as
/// needed, as for the missiles (`MISSILE_POOL`). [port]
pub const GRENADE_POOL: usize = 8;

/// One of a grenade weapon's pool of models (`spawn_player`, `grow_pools`): hidden until `draw`
/// flies a grenade of `name` with it, bound to that grenade's number while it flies.
#[derive(Component)]
pub struct GrenadeModel {
    pub name: u32,
    #[allow(dead_code)]
    pub slot: usize,
    pub carrying: Option<u32>,
}

/// Makes one more projectile model for each of this frame's `PoolDemand`, with the shared cache.
/// [port] A weapon whose data has no projectile model gets none (nothing is drawn for it but the
/// `--bodies` ball).
#[allow(clippy::too_many_arguments)]
pub fn grow_pools(
    data: Res<crate::GameData>,
    mut demand: ResMut<PoolDemand>,
    mut cache: ResMut<PoolCache>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surfaces: ResMut<Assets<crate::surface::Surface>>,
    mut glows: ResMut<Assets<crate::fx::Glow>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    mut made: Local<usize>,
) {
    if demand.0.is_empty() {
        return;
    }
    let mut builder = crate::assets::Builder {
        library: &data.0.library,
        meshes: &mut meshes,
        materials: &mut materials,
        surfaces: &mut surfaces,
        glows: &mut glows,
        images: &mut images,
        bindposes: &mut bindposes,
        cache: &mut cache.0,
    };
    for (name, grenade) in std::mem::take(&mut demand.0) {
        let Some(object) = data.0.projectile_object(name, grenade) else { continue };
        *made += 1;
        let slot = 1000 + *made;
        let (model, _) = builder.spawn(&mut commands, object, if grenade { "grenade" } else { "missile" });
        if grenade {
            commands.entity(model).insert((GrenadeModel { name, slot, carrying: None }, Visibility::Hidden));
        } else {
            commands.entity(model).insert((MissileModel { name, slot, carrying: None }, Visibility::Hidden));
        }
        if std::env::var_os("TF_DBG").is_some() {
            eprintln!("pool: one more {} model for {name:#010x} ({} grown so far)", if grenade { "grenade" } else { "missile" }, *made);
        }
    }
}

/// One model of a thrown piece (`WeaponEffects::debris`: the round's row key and the piece's index),
/// bound to the piece it carries by number while that flies, as the missiles' models are.
#[derive(Component)]
pub struct DebrisModel {
    pub round: u32,
    pub piece: usize,
    pub carrying: Option<u32>,
}

/// The pieces `draw_debris` found no free model for this frame: (round, piece). `grow_debris` makes them.
#[derive(Resource, Default)]
pub struct DebrisDemand(pub Vec<(u32, usize)>);

/// Draws each thrown piece with its own Object (its RenderThing: the chunk mesh), at its frame (its spin after its
/// placed turn, its uniform scale) [game 005522f0, 00552930]. [stand-in] the fade's alpha (00553b70 broadcasts it
/// as key 611d70bc for the last 0.5 s; what the RenderThing does with it is unread) is not drawn: the piece is
/// shown whole until it goes. settles: RenderThing's use of the alpha (00553b70's broadcast; the RenderThing
/// handler not read yet), then a capture of a debris piece's last half second.
pub fn draw_debris(
    arsenal: Res<Arsenal>,
    mut demand: ResMut<DebrisDemand>,
    mut models: Query<(Entity, &mut DebrisModel, &mut Transform, &mut Visibility)>,
) {
    let live: Vec<(u32, u32, usize, Affine3A, f32)> = arsenal.debris_pieces().collect();
    for (_, mut model, _, _) in &mut models {
        if model.carrying.is_some_and(|n| !live.iter().any(|(m, ..)| *m == n)) {
            model.carrying = None;
        }
    }
    for (number, round, piece, frame, _alpha) in &live {
        let carrier = models.iter().find(|(_, m, _, _)| m.carrying == Some(*number)).map(|(e, ..)| e)
            .or_else(|| models.iter().find(|(_, m, _, _)| m.round == *round && m.piece == *piece && m.carrying.is_none()).map(|(e, ..)| e));
        let Some((_, mut model, mut transform, mut visibility)) = carrier.and_then(|e| models.get_mut(e).ok()) else {
            demand.0.push((*round, *piece));
            continue;
        };
        model.carrying = Some(*number);
        let (scale, turn, at) = frame.to_scale_rotation_translation();
        transform.translation = game_to_bevy(at);
        transform.rotation = model_space_rotation() * turn;
        transform.scale = scale;
        *visibility = Visibility::Inherited;
    }
    for (_, model, _, mut visibility) in &mut models {
        if model.carrying.is_none() {
            *visibility = Visibility::Hidden;
        }
    }
}

/// Makes one model for each of this frame's `DebrisDemand` from the piece's own Object, with the pools' shared
/// cache; it shows from the next frame. [port]
#[allow(clippy::too_many_arguments)]
pub fn grow_debris(
    data: Res<crate::GameData>,
    mut demand: ResMut<DebrisDemand>,
    mut cache: ResMut<PoolCache>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surfaces: ResMut<Assets<crate::surface::Surface>>,
    mut glows: ResMut<Assets<crate::fx::Glow>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
) {
    if demand.0.is_empty() {
        return;
    }
    let mut builder = crate::assets::Builder {
        library: &data.0.library,
        meshes: &mut meshes,
        materials: &mut materials,
        surfaces: &mut surfaces,
        glows: &mut glows,
        images: &mut images,
        bindposes: &mut bindposes,
        cache: &mut cache.0,
    };
    for (round, piece) in std::mem::take(&mut demand.0) {
        let Some(p) = data.0.weapon_effects.iter().find(|r| r.name == round).and_then(|r| r.debris.get(piece)) else { continue };
        let (model, _) = builder.spawn(&mut commands, &p.object, "debris");
        commands.entity(model).insert((DebrisModel { round, piece, carrying: None }, Visibility::Hidden));
        if std::env::var_os("TF_DBG").is_some() {
            eprintln!("pool: one more debris model for {round:#010x} piece {piece}");
        }
    }
}

/// Draws what is in flight and what has just landed: missiles and grenades with their own
/// models (see below). The tracer lines (`TF_TRACERS`) and, under `--bodies`, the blast and impact
/// spheres are debug drawing only [port]: the blast itself is drawn by its effects row and light
/// (`effects.rs`, `WeaponFxKind::Blast`) [data]. Under `--bodies` a projectile with no model is a ball
/// [stand-in]: its model (the same pack's object) is not read. (Debug drawing only: `--bodies`
/// is a developer flag, no robot's play draws it.) settles: the projectile's model object in the pack (the
/// `projectile` object's model reference, for the debug draw only; no play behaviour depends on it).
pub fn draw(
    arsenal: Res<Arsenal>,
    show_bodies: Res<crate::targets::ShowBodies>,
    mut demand: ResMut<PoolDemand>,
    mut models: Query<(Entity, &mut GrenadeModel, &mut Transform, &mut Visibility), Without<MissileModel>>,
    mut missile_models: Query<(Entity, &mut MissileModel, &mut Transform, &mut Visibility), Without<GrenadeModel>>,
    mut gizmos: Gizmos,
) {
    for tracer in &arsenal.tracers {
        let fade = 1.0 - tracer.age / TRACER_SECONDS;
        gizmos.line(game_to_bevy(tracer.from), game_to_bevy(tracer.to), Color::srgba(1.0, 0.85, 0.3, fade));
    }
    // A missile is drawn with its launcher's own model (`missileProjectileObj`), its +y along its
    // direction, up world z, or world y above z 0.99 (`weapons::missile_look_along`) [game 007847e0 l.250-261, 00785542-007855dc: the update
    // sets the body to `look_at_y_matrix` toward position + direction]. Each model stays with the
    // missile it took (by number) until that ends; a missile with no free model asks for one more
    // (`PoolDemand`). [port]
    let live: Vec<u32> = arsenal.missiles.iter().map(|(_, n, _)| *n).collect();
    for (_, mut model, _, _) in &mut missile_models {
        if model.carrying.is_some_and(|n| !live.contains(&n)) {
            model.carrying = None;
        }
    }
    for (index, number, missile) in &arsenal.missiles {
        let name = weapon_in(&arsenal.held, &arsenal.special_weapons, &arsenal.car, *index).def.name;
        let direction = missile.direction.normalize_or(Vec3::Y);
        let carrier = missile_models.iter().find(|(_, m, _, _)| m.carrying == Some(*number)).map(|(e, ..)| e)
            .or_else(|| missile_models.iter().find(|(_, m, _, _)| m.name == name && m.carrying.is_none()).map(|(e, ..)| e));
        if let Some((_, mut model, mut transform, mut visibility)) = carrier.and_then(|e| missile_models.get_mut(e).ok()) {
            model.carrying = Some(*number);
            transform.translation = game_to_bevy(missile.position);
            transform.rotation = model_space_rotation() * weapons::missile_look_along(direction);
            *visibility = Visibility::Inherited;
        } else {
            demand.0.push((name, false));
            if show_bodies.0 {
                let at = game_to_bevy(missile.position);
                gizmos.sphere(at, 0.35, Color::srgb(1.0, 0.55, 0.15));
                gizmos.line(at, at - game_to_bevy(direction) * 3.0, Color::srgba(1.0, 0.8, 0.5, 0.6));
            }
        }
    }
    for (_, model, _, mut visibility) in &mut missile_models {
        if model.carrying.is_none() {
            *visibility = Visibility::Hidden;
        }
    }
    // A grenade (Long Haul's rounds, Jetfire's special, the Audi's mine) is drawn with its
    // weapon's own projectile model (`grenadeProjectileObj`, read as `weapons::read_grenade_objects`),
    // turned as `Grenade::orientation`: spun about its axis, or (the Audi's mines,
    // `velocityOrient`) its +y along the velocity [game 00786a60, 00786ff0]; bound to it by number
    // as the missiles' are.
    let live: Vec<u32> = arsenal.grenades.iter().map(|(_, n, _)| *n).collect();
    for (_, mut model, _, _) in &mut models {
        if model.carrying.is_some_and(|n| !live.contains(&n)) {
            model.carrying = None;
        }
    }
    for (index, number, grenade) in &arsenal.grenades {
        let name = weapon_in(&arsenal.held, &arsenal.special_weapons, &arsenal.car, *index).def.name;
        let velocity = grenade.velocity.normalize_or(Vec3::Y);
        let carrier = models.iter().find(|(_, m, _, _)| m.carrying == Some(*number)).map(|(e, ..)| e)
            .or_else(|| models.iter().find(|(_, m, _, _)| m.name == name && m.carrying.is_none()).map(|(e, ..)| e));
        if let Some((_, mut model, mut transform, mut visibility)) = carrier.and_then(|e| models.get_mut(e).ok()) {
            model.carrying = Some(*number);
            transform.translation = game_to_bevy(grenade.position);
            transform.rotation = model_space_rotation() * grenade.orientation;
            *visibility = Visibility::Inherited;
        } else {
            demand.0.push((name, true));
            if show_bodies.0 {
                let at = game_to_bevy(grenade.position);
                gizmos.sphere(at, 0.3, Color::srgb(0.45, 0.9, 0.35));
                gizmos.line(at, at - game_to_bevy(velocity) * 2.0, Color::srgba(0.7, 1.0, 0.6, 0.6));
            }
        }
    }
    for (_, model, _, mut visibility) in &mut models {
        if model.carrying.is_none() {
            *visibility = Visibility::Hidden;
        }
    }
    // Blast radii and shot impact markers are debug overlays, shown with B/--bodies.
    // Damage and the installed particle effects are updated independently.
    if !show_bodies.0 {
        return;
    }
    for blast in &arsenal.blasts {
        gizmos.sphere(game_to_bevy(blast.centre), blast.radius, Color::srgb(1.0, 0.7, 0.2));
    }
    for impact in &arsenal.impacts {
        let grown = impact.age / IMPACT_SECONDS;
        let radius = if impact.explosive { 1.0 + 5.0 * grown } else { 0.2 + 0.6 * grown };
        gizmos.sphere(game_to_bevy(impact.at), radius, Color::srgba(1.0, 0.6, 0.2, 1.0 - grown));
    }
}
