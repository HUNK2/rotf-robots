//! Installed Decepticon scouts: shared character simulation, receiving damage and death.
//! [stand-in] Encounter placement and tactics in the test arena; see docs/damage.md.
//! OPEN: settles: the level's own placements and tactics from its mission data (spawners, dormant with the
//! mission path), which the arena stands in for.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;

use crate::assets::{self, ObjectNodes, game_to_bevy};
use crate::player::{Player, TuningRes};
use crate::sim::World as _;
use crate::tuning::Tuning;
use crate::world::ArenaRes;
use tf2_core::ai_spawn::{self, Action, SpawnWorld, Spawner, SpawnerData};
use tf2_core::combat::Scout;
use tf2_core::formats::scene::Shape;
use tf2_core::melee::Target;
use tf2_core::navpath::{self, Agent};
use tf2_core::repair::{self, Ability, Member, Receiver, Rates};

/// [stand-in] Arena cylinder has one surface; original contact bodies have individual ones.
/// OPEN: settles: the surface field of each contact primitive in the scout's pack Collision nodes
/// (`tf2_lxb.py dump` of its pack, read per body), then one surface per body here.
const SURFACE: u32 = tf2_core::melee::surface::UPPER_BODY;
/// The `Head` node's name hash (CRC-32), as 00709870 looks it up (00578440). [game]
const HEAD: u32 = 0x07c1_59a2;

/// The level's nav mesh, built once at `--mission` load (main.rs `build_nav`). None: no mesh, so the
/// enemies walk straight lines and their sight is the stand-in test.
#[derive(Resource, Default)]
pub struct NavRes(pub Option<std::sync::Arc<tf2_core::navmesh::NavMesh>>);

/// A scout placed at `spawn`, given the level's nav mesh when there is one.
pub(crate) fn new_enemy(id: u32, spawn: Vec3, scout: &tf2_core::combat::Scout, nav: &NavRes) -> tf2_core::combat::Enemy {
    let mut combat = tf2_core::combat::Enemy::new(id, spawn, scout);
    if let Some(mesh) = &nav.0 {
        combat.set_nav(std::sync::Arc::clone(mesh), scout);
    }
    combat
}

#[derive(Resource)]
pub struct Encounter {
    /// The arena's scout. Shared (`Arc`) because a spawned enemy of another pack keeps its own.
    pub scout: Arc<Scout>,
    pub passive: bool,
    pub report: bool,
    /// The scouts run the game's own brain (`Scout::ai`), the default; `--stand-in-brain` gives
    /// them the stand-in instead (`--ai-brain` is still accepted and changes nothing).
    pub ai_brain: bool,
    /// The CombatRepair heal areas of the player's character pack, and the receivers' rates.
    pub heal: HealConfig,
}

/// A heal zone of a character's pack (`HealAreaEffect`): the Lux script name that puts it
/// down, its radius in metres, and its zone slot (0 Long Haul, 1 Ratchet). [data]
#[derive(Clone, Copy, Debug)]
pub struct HealArea {
    pub script: u32,
    pub radius: f32,
    pub slot: usize,
}

#[derive(Default)]
pub struct HealConfig {
    pub areas: Vec<HealArea>,
    /// The player's own heal fields, and the scout's (`Rates::read` of each characterAttributes). [data]
    pub user: Rates,
    pub enemy: Rates,
}

/// The CombatRepair zone of the player and the receivers' state. [game 0072bc40, 007eb6f0,
/// 0072d540] See `tf2_core::repair` and docs/special-combat-repair.md.
#[derive(Default)]
pub struct RepairState {
    seen_starts: u32,
    ability: Option<Ability>,
    receivers: std::collections::HashMap<u32, Receiver>,
}

fn member(id: u32, team: u32, position: Vec3, present: bool) -> Member {
    // [stand-in] the flag (bit 10 of c+0x14c) is set for every member. OPEN: settles: the writers of
    // bit 10 of +0x14c (candidates from a grep of the decompile: 00736060, 007310b0, 0071fdd0,
    // 0071dc20; not read), and whether a scout carries it on its own.
    Member { id, team, position, present, flag: true }
}

/// One step of the CombatRepair special for the player: a press when the special starts (only
/// for a special whose script names a heal area), the zone's scan over the player and the
/// scouts, and the heal of each receiver inside. Other characters never get a heal area.
fn heal_step(
    state: &mut RepairState,
    player: &mut Player,
    t: &Tuning,
    encounter: &Encounter,
    enemies: &mut Query<(&mut EnemyTarget, &mut Transform, &mut Visibility)>,
    dt: f32,
) {
    let car = player.state.car_mode;
    if player.state.special_starts != state.seen_starts {
        state.seen_starts = player.state.special_starts;
        let script = repair::sent_script(&t.special, car);
        if let Some(area) = encounter.heal.areas.iter().find(|a| script != 0 && a.script == script) {
            if state.ability.as_ref().is_none_or(|a| a.cooldown <= 0.0) {
                let mut ability = Ability::new(area.radius, area.slot);
                ability.press(&t.special);
                eprintln!("repair: heal zone on, radius {} slot {}", area.radius, area.slot);
                state.ability = Some(ability);
            } else {
                eprintln!("repair: press refused, cooldown {:.2}s", state.ability.as_ref().map_or(0.0, |a| a.cooldown));
            }
        }
    }
    let mut changes = Vec::new();
    let was_on = state.ability.as_ref().is_some_and(|a| a.zone.active);
    if let Some(ability) = state.ability.as_mut() {
        ability.tick(dt, &mut changes);
        if ability.zone.active {
            // [game 007eb6f0] each member is measured from its point plus half its height (0071a460
            // bottom, 0070abe0 height, l.120-164 of the disasm). The game's zone centre is not the
            // root: it is the effect's own node, `object_node_world_place` 0056bee0 with the node
            // index word at +0x14 of the effect (on node index 0 of his object [data]). The port takes
            // his root point instead [assumed]. settles: node 0's offset from the root in his object
            // (the HealAreaEffect's object node list; not in export\data\*.txt, not located).
            let height = if car { t.vehicle_height } else { t.robot_height };
            let owner = member(player.state.actor.id, player.state.actor.team, player.state.pos + Vec3::Z * 0.5 * height, player.state.health > 0.0);
            let mut members = vec![owner];
            let scout = &encounter.scout.tuning;
            members.extend(enemies.iter().map(|(e, _, _)| {
                let h = if e.combat.state.car_mode { scout.vehicle_height } else { scout.robot_height };
                member(e.id, e.combat.state.actor.team, e.combat.state.pos + Vec3::Z * 0.5 * h, !e.combat.state.dead())
            }));
            let centre = player.state.pos;
            ability.zone.scan(owner, centre, &members, &mut changes);
        }
    }
    if was_on && !state.ability.as_ref().is_some_and(|a| a.zone.active) {
        eprintln!("repair: heal zone off");
    }
    for change in changes {
        state.receivers.entry(change.id).or_default().on_change(change);
    }
    let user = player.state.actor.id;
    if player.state.health > 0.0
        && let Some(rx) = state.receivers.get_mut(&user) {
        let amount = rx.heal(&encounter.heal.user, dt);
        if amount > 0.0 {
            player.state.health = repair::apply(player.state.health, t.base_health, amount);
        }
    }
    let scout_max = encounter.scout.tuning.base_health;
    for (mut enemy, _, _) in enemies.iter_mut() {
        if enemy.combat.state.dead() {
            continue;
        }
        let Some(rx) = state.receivers.get_mut(&enemy.id) else { continue };
        let amount = rx.heal(&encounter.heal.enemy, dt);
        if amount > 0.0 {
            enemy.combat.state.health = repair::apply(enemy.combat.state.health, scout_max, amount);
        }
    }
}
#[derive(Component)]
pub(crate) struct EnemyModel {
    owner: Entity,
    blend: crate::animation::ObjectBlend,
}
#[derive(Component)]
pub(crate) struct EnemyGun {
    owner: Entity,
    blend: crate::animation::ObjectBlend,
}
pub use tf2_core::arsenal::TargetParts;

/// The place of the node named `hash` in the object's space under `pose`, if the object has it.
fn posed_node(nodes: &ObjectNodes, pose: &[Transform], hash: u32) -> Option<Vec3> {
    let index = nodes.name_hashes.iter().position(|&h| h == hash)?;
    let mut model: Vec<bevy::math::Affine3A> = Vec::with_capacity(pose.len());
    for (i, local) in pose.iter().enumerate().take(index + 1) {
        let local = local.compute_affine();
        let at = match nodes.parents.get(i).copied().flatten() {
            Some(parent) if parent < model.len() => model[parent] * local,
            _ => local,
        };
        model.push(at);
    }
    model.get(index).map(|m| Vec3::from(m.translation))
}

/// The bodies of an object posed by `pose` (each node's transform to its parent, game space, as the
/// model's node entities hold it), in the object's space.
fn posed_parts(object: &tf2_core::formats::scene::ObjectDef, nodes: &ObjectNodes, pose: &[Transform]) -> Vec<tf2_core::weapons::PartBody> {
    let mut model: Vec<bevy::math::Affine3A> = Vec::with_capacity(pose.len());
    for (i, local) in pose.iter().enumerate() {
        let local = local.compute_affine();
        let at = match nodes.parents.get(i).copied().flatten() {
            Some(parent) if parent < model.len() => model[parent] * local,
            _ => local,
        };
        model.push(at);
    }
    tf2_core::weapons::part_bodies(object, &model)
}

#[derive(Component)]
pub struct EnemyTarget {
    pub id: u32,
    pub combat: tf2_core::combat::Enemy,
    pub health: f32,
    /// Debug HUD: corpse age; original NPC completion removes it until encounter reset.
    pub down_for: f32,
    pub(crate) reset_serial: u32,
    /// The character's own scout data (its pack and attributes). The arena's enemies share `Encounter::scout`.
    pub scout: Arc<Scout>,
    /// The spawner (index into `SpawnRes::spawners`) that made this enemy. None for the arena's enemies.
    pub spawner: Option<usize>,
    /// [assumed] A spawned enemy placed with no floor within `SPAWN_FLOOR_DROP` is held at its spawn z
    /// until a floor is found under it (`update`). False for every other enemy.
    pub held: bool,
}

/// Marks an enemy for removal, with the reason it is printed with. Only `remove_marked` despawns
/// enemies: every system that wants one gone inserts this, so each enemy is despawned once.
#[derive(Component)]
pub struct Removed(pub String);

/// A spawned enemy is removed this many seconds after its death (`--spawners`) [assumed] 3 s; the
/// game's death animation length is not read here.
pub(crate) const SPAWNED_CULL_SECONDS: f32 = 3.0;

/// [stand-in] Three scouts to the left of the arena start.
pub fn layout(t: &Tuning) -> Vec<Target> {
    [(-25.0, 25.0), (-34.0, 31.0), (-25.0, 42.0)]
        .into_iter()
        .zip(1..)
        .map(|((x, y), id)| Target {
            id,
            pos: Vec3::new(x, y, 0.0),
            radius: t.robot_radius,
            height: t.robot_height,
            character: true,
            large: false,
            surface: SURFACE,
            dashing: false,
            health: f32::MAX,
            head: None,
        })
        .collect()
}

pub fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surfaces: ResMut<Assets<crate::surface::Surface>>,
    mut glows: ResMut<Assets<crate::fx::Glow>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    encounter: Res<Encounter>,
    arena: Res<ArenaRes>,
    nav: Res<NavRes>,
) {
    let scout = &encounter.scout;
    let mut cache = assets::Cache::default();
    let mut builder = assets::Builder {
        library: &scout.character.library,
        meshes: &mut meshes,
        materials: &mut materials,
        surfaces: &mut surfaces,
        glows: &mut glows,
        images: &mut images,
        bindposes: &mut bindposes,
        cache: &mut cache,
    };
    for target in arena.0.targets() {
        let owner = commands
            .spawn((
                Transform::from_translation(game_to_bevy(target.pos)),
                Visibility::Inherited,
                EnemyTarget {
                    id: target.id,
                    combat: new_enemy(target.id, target.pos, scout, &nav),
                    health: scout.tuning.base_health,
                    down_for: 0.0,
                    reset_serial: 0,
                    scout: Arc::clone(&encounter.scout),
                    spawner: None,
                    held: false,
                },
            ))
            .id();
        spawn_models(&mut commands, &mut builder, owner, scout);
    }
}

/// The model and the gun of an enemy, as children of its owner (`spawn` and the spawners share it).
pub(crate) fn spawn_models(commands: &mut Commands, builder: &mut assets::Builder, owner: Entity, scout: &Scout) {
    let (model, _) = builder.spawn(commands, &scout.character.robot, "scout");
    commands.entity(model).insert(EnemyModel {
        owner,
        blend: Default::default(),
    });
    commands.entity(owner).add_child(model);
    if let Some(object) = scout.character.weapon_objects.get(scout.gun_index).and_then(Option::as_ref) {
        let (gun, _) = builder.spawn(commands, object, "scout gun");
        commands.entity(gun).insert((
            EnemyGun {
                owner,
                blend: Default::default(),
            },
            Transform::IDENTITY,
            Visibility::Hidden,
        ));
    }
}

/// Whether his collision bodies that can hit are drawn (`--bodies`, or B in play).
#[derive(Resource, Default)]
pub struct ShowBodies(pub bool);

/// Draws the bodies an attack hits with while they are switched on, as the game's data
/// shapes them and the clip places them, and where each hit of this frame touched.
pub fn draw_bodies(
    keys: Res<ButtonInput<KeyCode>>,
    tuning: Res<TuningRes>,
    player: Single<&Player>,
    mut show: ResMut<ShowBodies>,
    mut gizmos: Gizmos,
) {
    if keys.just_pressed(KeyCode::KeyB) {
        show.0 = !show.0;
    }
    if !show.0 {
        return;
    }
    let colour = Color::srgb(1.0, 0.25, 0.9);
    for (_, _, body) in player.state.attack_bodies(&tuning.0) {
        let at = |local: Vec3| game_to_bevy(body.at.transform_point3(local));
        match body.shape {
            Shape::Sphere { radius } => {
                gizmos.sphere(at(Vec3::ZERO), radius, colour);
            }
            Shape::Cuboid { half } => {
                let corner = |i: usize| {
                    at(Vec3::from(half)
                        * Vec3::from(std::array::from_fn(|axis| {
                            if i >> axis & 1 == 1 { 1.0 } else { -1.0 }
                        })))
                };
                for i in 0..8usize {
                    for axis in 0..3 {
                        if i >> axis & 1 == 0 {
                            gizmos.line(corner(i), corner(i | 1 << axis), colour);
                        }
                    }
                }
            }
            Shape::Cylinder {
                radius,
                half_height,
            } => {
                const SIDES: usize = 24;
                let rim = |i: usize, z: f32| {
                    let (sin, cos) = (i as f32 / SIDES as f32 * std::f32::consts::TAU).sin_cos();
                    at(Vec3::new(cos * radius, sin * radius, z))
                };
                for i in 0..SIDES {
                    gizmos.line(rim(i, half_height), rim(i + 1, half_height), colour);
                    gizmos.line(rim(i, -half_height), rim(i + 1, -half_height), colour);
                    if i % 6 == 0 {
                        gizmos.line(rim(i, -half_height), rim(i, half_height), colour);
                    }
                }
            }
        }
    }
    // The ground punch's blast: its sphere as it grows.
    for blast in player.state.blasts.iter().filter(|blast| blast.live) {
        gizmos.sphere(
            game_to_bevy(blast.centre),
            blast.radius,
            Color::srgb(1.0, 0.6, 0.1),
        );
    }
    for hit in &player.state.hits {
        gizmos.sphere(game_to_bevy(hit.point), 0.3, Color::WHITE);
    }
}

/// Delivers each enemy's group broadcasts (its outbox, ARG_GROUP) to the other enemies with the same
/// `group` [assumed: one step late, after the frame's steps]. Two passes: the outboxes are taken
/// first, then delivered, since the query cannot give out all the enemies at once.
fn route_groups(enemies: &mut Query<(&mut EnemyTarget, &mut Transform, &mut Visibility)>) {
    let mut sent = Vec::new();
    for (mut enemy, _, _) in enemies.iter_mut() {
        let (from, group) = (enemy.id, enemy.combat.group);
        sent.extend(enemy.combat.group_outbox.drain(..).map(|name| (from, group, name)));
    }
    for (from, group, name) in sent {
        for (mut enemy, _, _) in enemies.iter_mut() {
            if enemy.id != from && enemy.combat.group == group {
                enemy.combat.receive_group_message(name);
            }
        }
    }
}

/// An enemy's kept gun sounds, by the rules of the player's (`weapons::KeptSounds`): the weapon
/// code is the same for every character. [game 007a9b40] a firing starts the fire and ring-off
/// sounds while none is kept, and keeps them when the weapon is `automaticFiring` and the event
/// is infinite; [game 007a9d40] the trigger-up branch of the weapon's update
/// (`Weapon::sounds_cut`) keys them off; [game 007a9ec0] so does the round that overheats, which
/// starts the overheat sound, kept until the lock runs out (007a9fe0: a key-off with
/// `fireOverHeatKeyOff`, else a fade). The arena played the fire event once a round and never let
/// go of it, so the scouts' gun (`WPN_SWIND_GUN_FIRE`, a sustain loop) went on for ever after
/// their bursts, three copies at a time.
#[derive(Default)]
pub struct EnemyGunSounds {
    fire: Option<tf2_core::sound::Handle>,
    ring: Option<tf2_core::sound::Handle>,
    overheat: Option<tf2_core::sound::Handle>,
}

impl EnemyGunSounds {
    pub(crate) fn fire(&mut self, sound: &crate::sound::SoundOut, def: &tf2_core::weapons::WeaponDef, tuning: &tf2_core::weapons::WeaponTuning, heat: f32, muzzle: Vec3) {
        for (id, slot, heat) in [(def.fire_sound, &mut self.fire, Some(heat)), (def.ring_off_sound, &mut self.ring, None)] {
            if id == 0 {
                continue;
            }
            match *slot {
                Some(handle) => sound.move_to(handle, muzzle),
                None => {
                    let handle = sound.start_kept_at(id, heat, muzzle);
                    if tuning.automatic_firing && sound.is_infinite(id) {
                        *slot = handle;
                    }
                }
            }
        }
    }

    /// After the enemy's weapon update: the cut or the overheat, and the overheat's end.
    pub(crate) fn after_step(&mut self, sound: &crate::sound::SoundOut, def: &tf2_core::weapons::WeaponDef, gun: &tf2_core::weapons::Weapon, muzzle: Vec3) {
        let overheated = gun.overheated_left > 0.0;
        if overheated && self.overheat.is_none() {
            self.cut(sound);
            if def.overheat_sound != 0 {
                self.overheat = sound.start_kept_at(def.overheat_sound, None, muzzle);
            }
        } else if !overheated && let Some(handle) = self.overheat.take() {
            if def.overheat_key_off { sound.key_off(handle) } else { sound.stop(handle) }
        }
        if gun.sounds_cut {
            self.cut(sound);
        }
        for handle in [self.fire, self.ring, self.overheat].into_iter().flatten() {
            sound.move_to(handle, muzzle);
        }
    }

    fn cut(&mut self, sound: &crate::sound::SoundOut) {
        for handle in [self.fire.take(), self.ring.take()].into_iter().flatten() {
            sound.key_off(handle);
        }
    }

    /// The enemy is gone: its loops end (keyed off, the overheat faded).
    pub(crate) fn let_go(&mut self, sound: &crate::sound::SoundOut) {
        self.cut(sound);
        if let Some(handle) = self.overheat.take() {
            sound.stop(handle);
        }
    }
}

/// Routes all hit packets through the same receiver and runs the scouts' combat.
pub fn update(
    time: Res<Time>,
    tuning: Res<TuningRes>,
    mut player: Single<&mut Player>,
    arsenal: Option<Res<crate::weapons::Arsenal>>,
    encounter: Res<Encounter>,
    sound: Option<Res<crate::sound::SoundOut>>,
    mut arena: ResMut<ArenaRes>,
    mut enemies: Query<(&mut EnemyTarget, &mut Transform, &mut Visibility)>,
    mut gizmos: Gizmos,
    mut repair: Local<RepairState>,
    mut brain_report_clock: Local<f32>,
    mut tactics_seen: Local<HashMap<u32, Option<[f32; 3]>>>,
    nav: Res<NavRes>,
    mut gun_sounds: Local<HashMap<u32, EnemyGunSounds>>,
) {
    let dt = time.delta_secs().min(0.05);
    let mut stepped = HashSet::new();
    // `--ai-brain` with `--combat-report`: the brain's state once per second per scout.
    *brain_report_clock += dt;
    let print_brain = encounter.ai_brain && encounter.report && *brain_report_clock >= 1.0;
    let mut outgoing = Vec::new();
    for (mut enemy, mut transform, mut visibility) in &mut enemies {
        // Each enemy steps with its own scout data (the arena's, or the pack of its spawner).
        let own = Arc::clone(&enemy.scout);
        let scout: &Scout = &own;
        // A spawned enemy is never reset: `spawn_run` removes it instead.
        if enemy.spawner.is_none() && enemy.reset_serial != player.reset_serial {
            enemy.combat = new_enemy(enemy.id, enemy.combat.spawn, scout, &nav);
            enemy.reset_serial = player.reset_serial;
            *visibility = Visibility::Inherited;
            if encounter.report {
                eprintln!(
                    "combat: reset scout {}, player health {:.1}",
                    enemy.id, player.state.health
                );
            }
            if !arena.0.targets.iter().any(|t| t.id == enemy.id) {
                arena.0.targets.push(enemy.combat.target(&scout.tuning));
            }
        }
        if enemy.combat.state.restart_requested {
            continue;
        }
        let id = enemy.id;
        for hit in player.state.hits.iter().filter(|hit| hit.target == id) {
            let packet = tf2_core::damage::Packet::melee(hit, player.state.actor);
            let result = enemy.combat.receive(packet, scout);
            if encounter.report && result.accepted {
                eprintln!(
                    "combat: melee hit scout {}, damage {:.1}, health {:.1}, stun {:.2}",
                    id, result.amount, result.health, enemy.combat.state.damage_timers.stunned
                );
            }
        }
        for shot in arsenal
            .iter()
            .flat_map(|a| &a.struck)
            .filter(|s| s.target == id)
        {
            let result = enemy.combat.receive(
                tf2_core::damage::Packet {
                    source: Some(player.state.actor),
                    ..shot.packet
                },
                scout,
            );
            if encounter.report && result.accepted {
                eprintln!(
                    "combat: weapon hit scout {}, damage {:.1}, health {:.1}, flags {:x}",
                    id, result.amount, result.health, shot.packet.flags
                );
            }
        }
        // A spawned enemy always runs the game's brain (its data is the game's own).
        enemy.combat.use_data_brain = encounter.ai_brain || enemy.spawner.is_some();
        let shots = enemy.combat.step(scout, &player.state, &tuning.0, &arena.0, encounter.passive, dt);
        let gun = &scout.character.weapons[scout.gun_index];
        let kept = gun_sounds.entry(id).or_default();
        for shot in shots {
            // [stand-in] Arena tracer drawing; original scout particle rendering is pending.
            gizmos.line(
                game_to_bevy(shot.from),
                game_to_bevy(shot.to),
                Color::srgb(1.0, 0.6, 0.12),
            );
            if let Some(s) = sound.as_deref() {
                kept.fire(s, gun, &scout.gun, enemy.combat.gun.heat_at_fire, shot.from);
            }
            if let Some(id) = shot.target {
                outgoing.push((id, shot.packet));
            }
        }
        if let Some(s) = sound.as_deref() {
            kept.after_step(s, gun, &enemy.combat.gun, enemy.combat.muzzle(scout));
        }
        stepped.insert(id);
        // Devastator's vortex drags it toward his head (`tf2_core::sim::vortex_pull`) [assumed port design]: moved by
        // its pull this step, and not let fall while it is held.
        if let Some(&(_, pull)) = player.state.pulls.iter().find(|(target, _)| *target == id)
            && !enemy.combat.state.dead() {
            enemy.combat.state.pos += pull * dt;
            enemy.combat.state.vel.z = enemy.combat.state.vel.z.max(0.0);
        }
        // [assumed] A spawned enemy placed with no floor under it stays at its spawn z (no fall) until
        // the collision has a floor under its position; then it is released to the sim.
        if enemy.held {
            let p = enemy.combat.state.pos;
            // Released only by a floor within SPAWN_FLOOR_DROP below its spawn z, the same test as the spawn (mission.rs
            // `spawn`). A floor far below (ground_z finds any surface down the column) would only let it fall to it.
            let floor = ground_z(&arena.0, p.x, p.y, p.z).filter(|f| *f >= enemy.combat.spawn.z - SPAWN_FLOOR_DROP);
            if let Some(f) = floor {
                enemy.held = false;
                eprintln!("enemy {id}: a floor is under it now (z {f:.2}, placement z {:.2}), released from its held spawn z", enemy.combat.spawn.z);
            } else {
                enemy.combat.state.pos.z = enemy.combat.spawn.z;
                enemy.combat.state.vel.z = 0.0;
            }
        }
        for hit in &enemy.combat.state.hits {
            outgoing.push((
                hit.target,
                tf2_core::damage::Packet::melee(hit, enemy.combat.state.actor),
            ));
        }
        if print_brain {
            let p = enemy.combat.state.pos;
            // [diagnostic] The first surface straight down from 5 m above it, down to 200 m under it.
            let below = tf2_core::sim::World::wheel_ray(&arena.0, Vec3::new(p.x, p.y, p.z + 5.0), Vec3::new(p.x, p.y, p.z - 200.0)).map(|h| h.point.z);
            eprintln!("brain scout {}: {}; {}; floor under it {:?}; ray down {:?}", enemy.id, placement(&enemy.combat, player.state.pos), enemy.combat.brain_report(), ground_z(&arena.0, p.x, p.y, p.z), below);
        }
        // The tactics point, printed when it changes (`--ai-brain --combat-report`).
        if encounter.ai_brain && encounter.report {
            let now = enemy.combat.tactics_point().map(|t| t.to_array());
            if tactics_seen.get(&enemy.id) != Some(&now) {
                tactics_seen.insert(enemy.id, now);
                match enemy.combat.tactics_point() {
                    Some(t) => eprintln!(
                        "tactics: scout {} point ({:.2}, {:.2}, {:.2}) Dt {:.1} Dc {:.1}",
                        enemy.id,
                        t.x,
                        t.y,
                        t.z,
                        flat_distance(t, player.state.pos),
                        flat_distance(t, enemy.combat.state.pos)
                    ),
                    None => eprintln!("tactics: scout {} point none", enemy.id),
                }
            }
        }
        enemy.health = enemy.combat.state.health;
        enemy.down_for = enemy.combat.state.death.as_ref().map_or(0.0, |d| d.age);
        transform.translation = game_to_bevy(enemy.combat.state.pos);
        transform.rotation = Quat::from_rotation_y(enemy.combat.state.yaw);
        if enemy.combat.state.dead() {
            arena.0.targets.retain(|t| t.id != enemy.id);
        } else if let Some(t) = arena.0.targets.iter_mut().find(|t| t.id == enemy.id) {
            // The Head place is `animate`'s, from the posed model: kept across this refresh.
            let head = t.head;
            *t = enemy.combat.target(&scout.tuning);
            t.head = head;
        }
        // [game] its physics body, which the player's and the other enemies' bodies meet on a level
        // (`physics_avatar::BodySnapshot`)
        arena.0.bodies.retain(|b| b.id != enemy.id);
        if !enemy.combat.state.dead() {
            arena.0.bodies.extend(enemy.combat.state.body_snapshot(&scout.tuning, true));
        }
        if enemy.combat.state.restart_requested {
            *visibility = Visibility::Hidden;
            if encounter.report {
                eprintln!(
                    "combat: scout {} removed after {:.2}s",
                    enemy.id, enemy.down_for
                );
            }
        }
    }
    // An enemy that did not step (gone, in a cut, restarting) lets its kept gun sounds go.
    gun_sounds.retain(|id, kept| {
        let here = stepped.contains(id);
        if !here {
            sound.as_deref().inspect(|s| kept.let_go(s));
        }
        here
    });
    // Group broadcasts of this frame reach the other members now, after every enemy has stepped.
    route_groups(&mut enemies);
    if print_brain {
        *brain_report_clock = 0.0;
    }
    heal_step(&mut repair, &mut player, &tuning.0, &encounter, &mut enemies, dt);
    for (id, packet) in outgoing {
        if id == player.state.actor.id {
            let result = player.state.receive_damage(packet, &tuning.0);
            if result.accepted && encounter.report {
                eprintln!(
                    "combat: {:.2} s (app clock): scout {} hit player for {:.1}, health {:.1}",
                    time.elapsed_secs(),
                    packet.source.map_or(0, |s| s.id),
                    result.amount,
                    result.health
                );
            }
        } else if let Some((mut enemy, _, _)) = enemies.iter_mut().find(|(e, _, _)| e.id == id) {
            let own = Arc::clone(&enemy.scout);
            enemy.combat.receive(packet, &own);
        }
    }
}

/// Where an enemy's clip was at the last frame, for its sound cues (`other_clip_cues`): the
/// clip (its set and id hashes, the base layer's serial), its tick, and whether it has gone round.
#[derive(Clone, Copy, PartialEq)]
pub struct ClipClock {
    clip: (u32, u32, Option<u32>),
    tick: f32,
    went_round: bool,
}

impl ClipClock {
    /// The cue window from the last frame to `tick` (as the player's `animation::animate`): a
    /// new clip, a new base serial or a non-looping clip started over plays from 0 and ends the
    /// last clip's autoStop sounds; a loop that went round plays past its end and on from 0.
    /// Returns (from, rerun, changed), or None when nothing is due.
    fn advance(last: &mut Option<ClipClock>, clip: (u32, u32, Option<u32>), tick: f32, looping: bool) -> Option<(f32, bool, bool)> {
        let changed = match *last {
            Some(l) => l.clip != clip || (!looping && tick < l.tick),
            None => true,
        };
        if changed {
            *last = Some(ClipClock { clip, tick: 0.0, went_round: false });
        }
        let l = last.as_mut().unwrap();
        let (from, rerun) = (l.tick, l.went_round);
        l.went_round |= tick < from;
        l.tick = tick;
        (tick >= from || looping).then_some((from, rerun, changed))
    }
}

/// Uses the scout's own clips on its model and weapon, including partial reactions, and plays
/// their sound cues and footsteps where it stands (`SoundOut::other_clip_cues`).
#[allow(clippy::too_many_arguments)]
pub fn animate(
    time: Res<Time>,
    enemies: Query<&EnemyTarget>,
    mut models: Query<(&mut EnemyModel, &ObjectNodes)>,
    mut guns: Query<(Entity, &mut EnemyGun, &ObjectNodes, &mut Visibility)>,
    mut nodes: Query<&mut Transform>,
    mut commands: Commands,
    mut arsenal: Option<ResMut<crate::weapons::Arsenal>>,
    mut arena: ResMut<ArenaRes>,
    sound: Option<Res<crate::sound::SoundOut>>,
    mut clocks: Local<HashMap<u32, Option<ClipClock>>>,
) {
    let mut parts: Vec<TargetParts> = Vec::new();
    if let Some(sound) = sound.as_deref() {
        sound.others_tick(time.delta_secs());
    }
    let mut heard = HashSet::new();
    for (mut model, object) in &mut models {
        let Ok(enemy) = enemies.get(model.owner) else {
            continue;
        };
        let own = Arc::clone(&enemy.scout);
        let scout: &Scout = &own;
        let s = &enemy.combat.state;
        let clip = s
            .action_clip(&scout.tuning)
            .map(|(set, id, tick)| (set, tf2_core::formats::hash::crc32(id.as_bytes()), tick, None))
            .or_else(|| {
                s.base_clip(&scout.tuning).map(|c| {
                    (
                        c.set,
                        tf2_core::formats::hash::crc32(c.id.as_bytes()),
                        c.tick,
                        Some(c.serial),
                    )
                })
            });
        if let Some(sound) = sound.as_deref() {
            heard.insert(enemy.id);
            let (point, speed) = (s.pos, s.vel.length());
            sound.follow(enemy.id, point, speed);
            let last = clocks.entry(enemy.id).or_default();
            if let Some((set, id, tick, serial)) = clip
                && let Some(r) = scout.character.animations.sets.get(set)
                    .and_then(|refs| refs.iter().find(|r| tf2_core::formats::hash::crc32(r.id.as_bytes()) == id))
                && let Some(c) = r.clip.as_ref()
                && let Some((from, rerun, changed)) = ClipClock::advance(last, (tf2_core::formats::hash::crc32(set.as_bytes()), id, serial), tick, r.looping)
            {
                sound.other_clip_cues(enemy.id, c, from, tick, rerun, changed, point, speed);
            }
        }
        let mut pose = clip.map_or_else(
            || object.rest.clone(),
            |(set, id, tick, _)| {
                model.blend.pose(
                    scout
                        .character
                        .animations
                        .sets
                        .get(set)
                        .map_or(&[], Vec::as_slice),
                    id,
                    tick,
                    time.delta_secs(),
                    object,
                )
            },
        );
        if let Some(arm) = enemy.combat.weapon_pose.arm
            && let Some(r) = scout.gun_clips().arm.get(arm.clip)
            && let Some(clip) = r.clip.as_ref()
        {
            if r.additive {
                crate::animation::lay_offsets(
                    &mut pose,
                    clip,
                    arm.tick,
                    r.looping,
                    arm.weight(),
                    &object.name_hashes,
                );
            } else {
                let over =
                    crate::animation::sample(clip, r.looping, arm.tick, &object.name_hashes, &pose);
                crate::animation::cross_fade(&mut pose, &over, 1.0 - arm.weight());
            }
        }
        if let Some(r) = s
            .hit_partial
            .current(&scout.tuning.rule_sets)
            .and_then(|c| {
                scout
                    .character
                    .animations
                    .sets
                    .get("HitReactPartialSet")?
                    .iter()
                    .find(|r| tf2_core::formats::hash::crc32(r.id.as_bytes()) == c.id)
            })
            && let Some(clip) = r.clip.as_ref()
        {
            crate::animation::lay_offsets(
                &mut pose,
                clip,
                s.hit_partial.tick,
                r.looping,
                1.0,
                &object.name_hashes,
            );
        }
        // Its bodies as this pose places them, for the rounds of the next tick. [game 007832b0]
        parts.push(TargetParts { id: enemy.id, yaw: s.yaw, bodies: posed_parts(&scout.character.robot, object, &pose) });
        // Its footstep cues probe from its posed feet, as the player's. [game 0073f420]
        if let Some(sound) = sound.as_deref() {
            let local: Vec<_> = pose.iter().map(Transform::compute_affine).collect();
            let root = bevy::math::Affine3A::from_rotation_translation(Quat::from_rotation_z(s.yaw), s.pos);
            let world: Vec<_> = tf2_core::footik::accumulate(&local, &object.parents).into_iter().map(|at| root * at).collect();
            sound.other_steps(enemy.id, &world, &scout.character.robot.footsteps, &scout.character.step_impact, &arena.0, s.vel.length());
        }
        // Its Head node's place, where the aim assist and the lock-on aim (00709870 l.62-74: the Head
        // node, else its bottom + 0.75 of its height; a scout never drives, the +0x31c == 3 case). [game]
        if let Some(target) = arena.0.targets.iter_mut().find(|t| t.id == enemy.id) {
            target.head = posed_node(object, &pose, HEAD).map(|head| {
                bevy::math::Affine3A::from_rotation_translation(Quat::from_rotation_z(s.yaw), s.pos).transform_point3(head)
            });
        }
        // [stand-in] its clips' own scales (`animation::own_scales`) are not put on its nodes'
        // frames, so a clip's per-node sizing misses its skin (its chains are right: the own
        // scale is not in `pose`). settles: evaluating `own_scales` through its fades and layers
        // as `animation::animate` does for the player.
        for (&entity, transform) in object.entities.iter().zip(pose) {
            if let Ok(mut at) = nodes.get_mut(entity) {
                *at = transform;
            }
        }
        for (gun_entity, _, _, mut visibility) in guns
            .iter_mut()
            .filter(|(_, g, _, _)| g.owner == model.owner)
        {
            let def = &scout.character.weapons[scout.gun_index];
            let bone = if enemy.combat.weapon_pose.node != 0 {
                enemy.combat.weapon_pose.node
            } else {
                def.bone
            };
            if let Some(i) = object.name_hashes.iter().position(|&h| h == bone) {
                commands.entity(object.frame(i)).add_child(gun_entity);
            }
            *visibility = if enemy.combat.weapon_pose.shown && !s.dead() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    for (_, mut gun, object, _) in &mut guns {
        let Ok(enemy) = enemies.get(gun.owner) else {
            continue;
        };
        let own = Arc::clone(&enemy.scout);
        let scout: &Scout = &own;
        let pose = enemy.combat.weapon_pose.object.map_or_else(
            || object.rest.clone(),
            |c| {
                gun.blend.pose(
                    scout.gun_clips().object,
                    c.id,
                    c.tick,
                    time.delta_secs(),
                    object,
                )
            },
        );
        for (&entity, transform) in object.entities.iter().zip(pose) {
            if let Ok(mut at) = nodes.get_mut(entity) {
                *at = transform;
            }
        }
    }
    if let Some(arsenal) = arsenal.as_mut() {
        arsenal.target_parts = parts;
    }
    // An enemy that is gone stops its loops; its one-shots play out where it was.
    if let Some(sound) = sound.as_deref() {
        for owner in sound.others() {
            if !heard.contains(&owner) {
                sound.other_gone(owner);
            }
        }
    }
    clocks.retain(|id, _| heard.contains(id));
}

// ---------------------------------------------------------------------------------------------
// Spawners (`--mission <name> --spawners [name]`). The mission's CharacterSpawnManagers run here:
// `tf2_core::ai_spawn` decides when and where, this module makes the enemies. [stand-in] the
// spawners are enabled by a stand-in message (1 s after the load), not by mission scripting.
// ---------------------------------------------------------------------------------------------

/// Seconds after the load when the stand-in Enable goes to the spawners. [stand-in] [assumed] 1 s
/// (the task's value); the mission script that would send it is phase 4, not built.
/// OPEN (dormant mission path): settles: the level's own script message that enables the spawners (its
/// trigger timing in the mission data), once the mission path is wanted again.
const STAND_IN_ENABLE_SECONDS: f32 = 1.0;

/// The mission's spawners and the state the game keeps for them. Built in `main` from the mission's
/// `mission.str` chunk 0 (`ai_spawn::load`).
#[derive(Resource)]
pub struct SpawnRes {
    pub spawners: Vec<Spawner>,
    game_dir: std::path::PathBuf,
    /// The scout data of spawned characters, loaded on first use: (pack, attributes) -> scout.
    scouts: HashMap<(&'static str, &'static str), Arc<Scout>>,
    /// (spawner index, characterDefinition hash) already reported as skipped: one line per spawner, not per definition.
    skipped: HashSet<(usize, u32)>,
    /// (spawner index, the spawner's own id) -> the enemy's actor id (the id damage uses).
    actors: HashMap<(usize, u32), u32>,
    next_actor: u32,
    rng: u32,
    /// `--spawners <name>`: only spawners whose name contains it. None: all of them.
    filter: Option<String>,
    /// Seconds until the stand-in Enable. None once it has gone.
    enable_in: Option<f32>,
    /// Spawned characters whose death has been reported to the spawner (one line each).
    dead_logged: HashSet<u32>,
}

/// A spawned enemy stands on the floor found under its spawn point when that floor lies within this
/// many metres below the spawn z [assumed] (the brief's rule; the game's rule is unread). Otherwise it
/// stands at the spawn point's own z.
pub(crate) const SPAWN_FLOOR_DROP: f32 = 5.0;

impl SpawnRes {
    /// Logs one line per spawner (name, records, maxAtOnce, lifetime total, startEnabled) and keeps them
    /// waiting (`start`: enabled = startEnabled). [data] the values come from the mission's data.
    pub fn new(game_dir: &std::path::Path, data: Vec<SpawnerData>, filter: Option<String>) -> Self {
        // An unnamed spawner is labelled by its index in the mission (`#07 (unnamed)`), so its lines can be told apart.
        let data: Vec<SpawnerData> = data
            .into_iter()
            .enumerate()
            .map(|(i, mut d)| {
                if d.name.is_empty() {
                    d.name = format!("#{i:02} (unnamed)");
                }
                d
            })
            .collect();
        for d in &data {
            eprintln!(
                "spawner {}: records {}, maxAtOnce {}, lifetime {}, startEnabled {}",
                spawner_name(d),
                d.records.len(),
                d.max_enemies_at_once,
                d.max_enemies_lifetime,
                d.start_enabled
            );
        }
        let spawners = data
            .into_iter()
            .map(|d| {
                let mut s = Spawner::new(d);
                s.start();
                s
            })
            .collect();
        Self {
            spawners,
            game_dir: game_dir.to_path_buf(),
            scouts: HashMap::new(),
            skipped: HashSet::new(),
            actors: HashMap::new(),
            next_actor: 0x1000,
            rng: 0x5eed,
            filter,
            enable_in: Some(STAND_IN_ENABLE_SECONDS),
            dead_logged: HashSet::new(),
        }
    }
}

/// The spawner's label in the log. `SpawnRes::new` has already named an unnamed one by its index.
fn spawner_name(d: &SpawnerData) -> String {
    if d.name.is_empty() { "(unnamed)".to_owned() } else { d.name.clone() }
}

/// Whether `--spawners <name>` takes this spawner (all of them when no name was given).
fn wanted(filter: &Option<String>, d: &SpawnerData) -> bool {
    filter.as_ref().is_none_or(|f| d.name.contains(f.as_str()))
}

/// The `--spawners` summary at Enable: what each enabled spawner would do at its first update
/// (`ai_spawn::enable_outcome`). Skips are grouped by definition, with the reason once.
fn enable_summary<'a>(enabled: impl Iterator<Item = &'a Spawner>) -> String {
    use tf2_core::ai_spawn::EnableOutcome;
    let (mut n, mut will, mut waiting, mut no_point, mut done) = (0, 0, 0, 0, 0);
    // (definition name, reason, count), in the order first met.
    let mut skipped: Vec<(String, &'static str, usize)> = Vec::new();
    for spawner in enabled {
        n += 1;
        match ai_spawn::enable_outcome(spawner) {
            EnableOutcome::WillSpawn => will += 1,
            EnableOutcome::Waiting => waiting += 1,
            EnableOutcome::NoPoint => no_point += 1,
            EnableOutcome::Done => done += 1,
            EnableOutcome::Skipped(reason) => {
                let def = spawner.data.records.first().and_then(|r| r.definition.as_ref()).map_or("(no definition)".to_owned(), |d| d.name.clone());
                match skipped.iter_mut().find(|(d, r, _)| *d == def && *r == reason) {
                    Some(entry) => entry.2 += 1,
                    None => skipped.push((def, reason, 1)),
                }
            }
        }
    }
    let skipped_total: usize = skipped.iter().map(|(_, _, c)| c).sum();
    let reasons = if skipped.is_empty() {
        "none".to_owned()
    } else {
        skipped.iter().map(|(d, r, c)| format!("{d} x{c}: {r}")).collect::<Vec<_>>().join("; ")
    };
    let done_part = if done > 0 { format!(", {done} done (lifetime 0)") } else { String::new() };
    format!(
        "spawners: {n} enabled, {will} will spawn now, {waiting} waiting (timer), {no_point} with no spawn point{done_part}, {skipped_total} skipped ({reasons})"
    )
}

/// Flat (x, y) distance, metres.
fn flat_distance(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.y - b.y).length()
}

/// The ground under `(x, y)` from the mission's collision (or the arena's flat floor): the same query
/// the enemies stand on (`Arena::floor`). None when nothing is under the point.
pub(crate) fn ground_z(arena: &tf2_core::sim::Arena, x: f32, y: f32, z: f32) -> Option<f32> {
    let floor = arena.floor(x, y, z + 2.0);
    (floor > tf2_core::level_world::NO_FLOOR).then_some(floor)
}

/// What a spawner asks of the world (`SpawnWorld`), for its update this frame.
struct SpawnView<'a> {
    spawner: usize,
    actors: &'a HashMap<(usize, u32), u32>,
    live: &'a HashSet<u32>,
    arena: &'a tf2_core::sim::Arena,
    nav: Option<&'a tf2_core::navmesh::NavMesh>,
    agent: Agent,
    player: Vec3,
}

impl SpawnWorld for SpawnView<'_> {
    fn is_alive(&self, id: u32) -> bool {
        self.actors.get(&(self.spawner, id)).is_some_and(|actor| self.live.contains(actor))
    }
    fn ground_at(&self, p: [f32; 3]) -> Option<[f32; 3]> {
        ground_z(self.arena, p[0], p[1], p[2]).map(|z| [p[0], p[1], z])
    }
    fn reference_point(&self) -> [f32; 3] {
        self.player.to_array()
    }
    /// [stand-in] Without a nav mesh the corridor is taken as straight (`true`).
    fn straight_corridor(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        self.nav.is_none_or(|mesh| navpath::straight(mesh, Vec3::from_array(from), Vec3::from_array(to), &self.agent))
    }
}

/// One frame of the spawners. The stand-in Enable, then each spawner's update with its actions
/// carried out (spawn an enemy, log a send, delete its enemies). Then the spawned enemies that are
/// dead for `SPAWNED_CULL_SECONDS`, or whose player restarted, are removed (never reset). [stand-in]
/// the update runs once a frame with the frame's dt, as the arena's enemies do, not on a fixed step.
#[allow(clippy::too_many_arguments)]
pub fn spawn_run(
    time: Res<Time>,
    mut res: ResMut<SpawnRes>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surfaces: ResMut<Assets<crate::surface::Surface>>,
    mut glows: ResMut<Assets<crate::fx::Glow>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut arena: ResMut<ArenaRes>,
    nav: Res<NavRes>,
    tuning: Res<TuningRes>,
    player: Single<&Player>,
    enemies: Query<(Entity, &mut EnemyTarget)>,
) {
    let dt = time.delta_secs().min(0.05);
    let SpawnRes { spawners, game_dir, scouts, skipped, actors, next_actor, rng, filter, enable_in, dead_logged } = &mut *res;

    // The stand-in Enable: once, after `STAND_IN_ENABLE_SECONDS`, to every wanted spawner.
    let due = match enable_in {
        Some(wait) => {
            *wait -= dt;
            *wait <= 0.0
        }
        None => false,
    };
    if due {
        *enable_in = None;
        let mut sent = 0;
        for spawner in spawners.iter_mut().filter(|s| wanted(filter, &s.data)) {
            // Enable enables; the spawner returns no actions for it.
            spawner.message(ai_spawn::MSG_ENABLE);
            sent += 1;
        }
        let which = filter.as_deref().map_or_else(|| "all".to_owned(), |f| format!("names containing {f:?}"));
        // [stand-in] this log line reports the stand-in Enable (see STAND_IN_ENABLE_SECONDS). Enemy path only: `spawn_run` is added only with `--spawners` (main.rs l.636-637). OPEN (dormant mission
        // path): settles: the mission script's Enable message to the spawners, not built (phase 4).
        eprintln!("spawners: stand-in Enable sent to {sent} of {} spawner(s) ({which})", spawners.len());
        eprintln!("{}", enable_summary(spawners.iter().filter(|s| wanted(filter, &s.data))));
    }

    // The living enemies (dead ones are not alive for a spawner).
    let live: HashSet<u32> = enemies.iter().filter(|(_, e)| !e.combat.state.dead()).map(|(_, e)| e.id).collect();
    // One line per death of a spawned character, reported to its spawner (the spawner counts it dead from here). [assumed] the callback's timing.
    // OPEN (dormant mission path): settles: the game's spawner death callback (the CharacterSpawnManager's death
    // handler, not read) and the frame it runs in.
    for (_, e) in enemies.iter() {
        if let Some(index) = e.spawner.filter(|_| e.combat.state.dead())
            && dead_logged.insert(e.id) {
            eprintln!("spawner {}: character {} died (reported to the spawner)", spawner_name(&spawners[index].data), e.id);
        }
    }
    let agent = Agent::new(tuning.0.robot_radius, tuning.0.robot_height, 0);
    let player_at = player.state.pos;

    for index in 0..spawners.len() {
        let actions = {
            let view = SpawnView {
                spawner: index,
                actors: &*actors,
                live: &live,
                arena: &arena.0,
                nav: nav.0.as_deref(),
                agent,
                player: player_at,
            };
            spawners[index].update(dt, &view, rng)
        };
        let name = spawner_name(&spawners[index].data);
        for action in actions {
            match action {
                Action::Send { msg: hash, .. } => {
                    eprintln!("spawner {name} sent {hash:#010x}: mission scripting will consume it (phase 4, not built)");
                }
                Action::DeleteAll => {
                    for (entity, _) in enemies.iter().filter(|(_, e)| e.spawner == Some(index)) {
                        commands.entity(entity).try_insert(Removed("deleted by its spawner".to_owned()));
                    }
                    actors.retain(|(i, _), _| *i != index);
                    eprintln!("spawner {name} deleted its characters");
                }
                Action::Spawn { record, point, id } => {
                    let rec = &spawners[index].data.records[record];
                    let (def_hash, def_name) = rec.definition.as_ref().map_or((0, "(no definition)".to_owned()), |d| (d.hash, d.name.clone()));
                    let Some((pack, attributes)) = ai_spawn::character_inputs(def_hash) else {
                        // [assumed] the spawn is counted as dead at the next update (is_alive is false for it).
                        // OPEN (dormant mission path): settles: the game's handling of a spawn whose definition has no
                        // character (the spawner's create path, not read).
                        if skipped.insert((index, def_hash)) {
                            eprintln!("spawner {name}: wants {def_name}, skipped: {}", ai_spawn::skip_reason(def_hash));
                        }
                        continue;
                    };
                    let scout = match scouts.get(&(pack, attributes)) {
                        Some(s) => Arc::clone(s),
                        None => match Scout::load_as(game_dir.as_path(), pack, attributes) {
                            Ok(s) => {
                                let s = Arc::new(s);
                                scouts.insert((pack, attributes), Arc::clone(&s));
                                // The team test is the game's single-player same-team test 0117e760 (no MP flag): the two
                                // objects' team words (+0x244) are equal, nonzero not required [game 0117e760]. The
                                // scout's damage.team is taken as that word [assumed]: its loader store is not read.
                                let (team, mine) = (s.tuning.damage.team, player.state.actor.team);
                                let verdict = if team == mine { "the player's team (game 0117e760): not a foe of him" } else { "another team: a foe of him" };
                                eprintln!("spawner: loaded {pack}:{attributes} for {def_name}, team {team} (player {mine}): {verdict}");
                                s
                            }
                            Err(e) => {
                                if skipped.insert((index, def_hash)) {
                                    eprintln!("spawner {name}: wants {def_name}, skipped: cannot load {pack}:{attributes} ({e})");
                                }
                                continue;
                            }
                        },
                    };
                    let Some(p) = rec.points.get(point) else { continue };
                    // [assumed] The ground under the spawn point, probed from its z + 1.5 m (combat::probe_ground):
                    // a floor within SPAWN_FLOOR_DROP below it is the ground; otherwise the spawn's own z.
                    let [sx, sy, sz] = p.place;
                    let floor = tf2_core::combat::probe_ground(|x, y, z| arena.0.floor(x, y, z), sx, sy, sz);
                    let (z, held) = match floor {
                        Some(f) if f >= sz - SPAWN_FLOOR_DROP => (f, false),
                        _ => (sz, true),
                    };
                    let at = Vec3::new(sx, sy, z);
                    let actor = *next_actor;
                    *next_actor += 1;
                    actors.insert((index, id), actor);
                    if held {
                        eprintln!(
                            "spawn: [assumed] enemy {actor} ({def_name}): no floor within {SPAWN_FLOOR_DROP} m under its spawn point ({sx:.2}, {sy:.2}, z {sz:.2}); held at its spawn z until a floor is under it"
                        );
                    }

                    let mut combat = new_enemy(actor, at, &scout, &nav);
                    combat.group = rec.group_id as u32;
                    // [game] the home point only when the record says so, with the mesh. [assumed] the point is the home.
                    // OPEN (dormant mission path): settles: the game's home setter for a spawned character (the record flag
                    // `use_spawn_point_as_home` in the spawner data, and the function that copies the point to the AI home).
                    combat.home = (rec.use_spawn_point_as_home && nav.0.is_some()).then_some(at.to_array());
                    // [assumed] facing: the same turn rule the enemy's own brain uses (combat.rs brain_input).
                    combat.state.yaw = (-p.forward[0]).atan2(p.forward[1]);
                    for &message in &rec.messages_on_spawn {
                        combat.deliver_message(message);
                    }
                    eprintln!(
                        "spawner {name}: spawned {def_name} id {actor} at ({:.2}, {:.2}, {:.2}) group {} home {}",
                        at.x,
                        at.y,
                        at.z,
                        combat.group,
                        if combat.home.is_some() { "yes" } else { "no" }
                    );
                    arena.0.targets.push(combat.target(&scout.tuning));
                    let owner = commands
                        .spawn((
                            Transform::from_translation(game_to_bevy(at)),
                            Visibility::Inherited,
                            EnemyTarget {
                                id: actor,
                                combat,
                                health: scout.tuning.base_health,
                                down_for: 0.0,
                                reset_serial: player.reset_serial,
                                scout: Arc::clone(&scout),
                                spawner: Some(index),
                                held,
                            },
                        ))
                        .id();
                    let mut cache = assets::Cache::default();
                    let mut builder = assets::Builder {
                        library: &scout.character.library,
                        meshes: &mut meshes,
                        materials: &mut materials,
                        surfaces: &mut surfaces,
                        glows: &mut glows,
                        images: &mut images,
                        bindposes: &mut bindposes,
                        cache: &mut cache,
                    };
                    spawn_models(&mut commands, &mut builder, owner, &scout);
                }
            }
        }
    }

    // Spawned enemies go, never reset: dead for the cull time, or restarted, or the player restarted.
    // Marked here; `remove_marked` despawns them once.
    for (entity, e) in enemies.iter().filter(|(_, e)| e.spawner.is_some()) {
        let why = if e.combat.state.restart_requested {
            "restart requested"
        } else if e.combat.state.dead() && e.down_for >= SPAWNED_CULL_SECONDS {
            "dead for the cull time"
        } else if e.reset_serial != player.reset_serial {
            "the player restarted"
        } else {
            continue;
        };
        commands.entity(entity).try_insert(Removed(why.to_owned()));
    }
}

/// The one place an enemy is despawned. It runs after the spawners, `cull_unplaced` and `animate`, so
/// their commands on an enemy's parts (a gun added to a node, say) apply before the enemy goes; each
/// marked enemy is despawned once, with its parts, and reported. Its arena target and actor id go too.
pub fn remove_marked(
    mut commands: Commands,
    mut arena: ResMut<ArenaRes>,
    mut spawners: Option<ResMut<SpawnRes>>,
    marked: Query<(Entity, &EnemyTarget, &Removed)>,
) {
    for (entity, e, removed) in &marked {
        arena.0.targets.retain(|t| t.id != e.id);
        arena.0.bodies.retain(|b| b.id != e.id);
        match (e.spawner, spawners.as_deref_mut()) {
            (Some(index), Some(res)) => {
                res.actors.retain(|_, actor| *actor != e.id);
                eprintln!(
                    "spawner {}: character {} removed ({}), reported to the spawner",
                    spawner_name(&res.spawners[index].data),
                    e.id,
                    removed.0
                );
            }
            _ => eprintln!("enemy {}: {}", e.id, removed.0),
        }
        commands.entity(entity).try_despawn();
    }
}

/// [assumed] With a mission loaded, an enemy goes when there is no floor under its placement point
/// (probed once, on the first frame it is seen) or when it falls more than `FALL_LIMIT` metres below its
/// placement z. One line each: "enemy N: no floor, removed". The rule is ours (brief item 1b), not the
/// game's; what the game does with a character that has no floor is unread.
const FALL_LIMIT: f32 = 50.0;

/// Spawned enemies are not removed for having no floor at their spawn point: they are held (`update`).
/// Only the fall guard applies to them.
pub fn cull_unplaced(mut commands: Commands, arena: Res<ArenaRes>, mut checked: Local<HashSet<u32>>, enemies: Query<(Entity, &EnemyTarget), Without<Removed>>) {
    for (entity, e) in &enemies {
        let spawn = e.combat.spawn;
        let fell = spawn.z - e.combat.state.pos.z;
        let no_floor = e.spawner.is_none() && checked.insert(e.id) && ground_z(&arena.0, spawn.x, spawn.y, spawn.z).is_none();
        let why = if no_floor {
            "no floor, removed".to_owned()
        } else if fell > FALL_LIMIT {
            format!("no floor, removed (fell {fell:.0} m below its placement z)")
        } else {
            continue;
        };
        commands.entity(entity).try_insert(Removed(why));
    }
}

/// The placement facts of an enemy for the `--combat-report` line: where it is, where the player is,
/// the flat distance, the tactics record, the path source, its shots, its group and whether it has a home.
fn placement(combat: &tf2_core::combat::Enemy, player: Vec3) -> String {
    let p = combat.state.pos;
    let tactics = match combat.tactics_point() {
        Some(t) => format!("valid, point ({:.2}, {:.2}, {:.2})", t.x, t.y, t.z),
        None => "not valid".to_owned(),
    };
    format!(
        "pos ({:.2}, {:.2}, {:.2}), player ({:.2}, {:.2}, {:.2}), distance {:.2} m (flat), tactics {tactics}, path {}, shots {}, group {}, home {}",
        p.x,
        p.y,
        p.z,
        player.x,
        player.y,
        player.z,
        flat_distance(p, player),
        combat.path_kind(),
        combat.shots,
        combat.group,
        if combat.home.is_some() { "yes" } else { "no" }
    )
}

#[cfg(test)]
mod tests {
    use super::ClipClock;

    /// An enemy's cue windows follow the player's rule: a new clip plays from 0 and ends the
    /// last one's autoStop sounds, a loop that goes round plays on from 0 with its rerun flag,
    /// and a one-shot started over counts as a new clip.
    #[test]
    fn enemy_cue_windows_follow_the_players_rule() {
        let mut last = None;
        let walk = (1, 2, Some(0));
        assert_eq!(ClipClock::advance(&mut last, walk, 3.0, true), Some((0.0, false, true)), "a new clip from 0");
        assert_eq!(ClipClock::advance(&mut last, walk, 7.0, true), Some((3.0, false, false)), "then on from where it was");
        assert_eq!(ClipClock::advance(&mut last, walk, 2.0, true), Some((7.0, false, false)), "a loop goes round: 7 to 2");
        assert_eq!(ClipClock::advance(&mut last, walk, 5.0, true), Some((2.0, true, false)), "and its cues are reruns after");
        assert_eq!(ClipClock::advance(&mut last, (1, 2, Some(1)), 1.0, true), Some((0.0, false, true)), "a new base serial is a new clip");
        let shot = (1, 9, None);
        assert_eq!(ClipClock::advance(&mut last, shot, 4.0, false), Some((0.0, false, true)));
        assert_eq!(ClipClock::advance(&mut last, shot, 1.0, false), Some((0.0, false, true)), "a one-shot started over");
    }
}
