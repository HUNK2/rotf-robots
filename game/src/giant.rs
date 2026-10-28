//! The walking giant's (Devastator's) presentation in the game: what a 60 m robot needs that the
//! game's man-sized setup does not give him (`tf2_core::giant`).

use bevy::light::{CascadeShadowConfigBuilder, DirectionalLight, DirectionalLightShadowMap};
use bevy::math::Affine3A;
use bevy::prelude::*;

use crate::player::TuningRes;

/// [assumed port design] His sun shadow. The stage's `shadowRange` (60 m on Shanghai, the arena's
/// look the same order) is sized for a 6 m robot seen from 5 m: Devastator stands 60 m high and his
/// camera sits about 165 m back, so neither he nor what is round him cast a shadow in view. While a
/// giant is played the sun's two cascades reach `GIANT_SHADOW_RANGE`, the first ending past him
/// (`GIANT_FIRST_CASCADE`), on a 4096 map; any other character gets the stage's own back.
const GIANT_SHADOW_RANGE: f32 = 900.0;
const GIANT_FIRST_CASCADE: f32 = 320.0;
const GIANT_SHADOW_MAP: usize = 4096;

pub fn shadows(
    tuning: Res<TuningRes>,
    mut commands: Commands,
    suns: Query<Entity, With<DirectionalLight>>,
    mut done: Local<Option<bool>>,
) {
    let giant = tuning.0.giant.is_some();
    if *done == Some(giant) {
        return;
    }
    *done = Some(giant);
    let look = crate::world::look().filter(|e| e.shadow_range > 0.0);
    let (config, size) = if giant {
        (Some(CascadeShadowConfigBuilder {
            num_cascades: 2,
            first_cascade_far_bound: GIANT_FIRST_CASCADE,
            maximum_distance: GIANT_SHADOW_RANGE,
            ..default()
        }.build()), Some(GIANT_SHADOW_MAP))
    } else {
        // As `world::spawn` sets it up.
        (look.map(|e| CascadeShadowConfigBuilder { num_cascades: 2, maximum_distance: e.shadow_range, ..default() }.build()),
            look.filter(|e| e.shadow_map_size > 0.0).map(|e| (e.shadow_map_size as usize).next_power_of_two()))
    };
    for sun in &suns {
        match &config {
            Some(config) => { commands.entity(sun).insert(config.clone()); }
            None => { commands.entity(sun).insert(CascadeShadowConfigBuilder::default().build()); }
        }
    }
    commands.insert_resource(DirectionalLightShadowMap { size: size.unwrap_or(2048) });
}

/// What his hits and his fall add to the clips that play them (`tf2_core::giant::Stagger`)
/// [assumed port design]: a stagger's grunt and hard shake (and the heave, when only his body
/// sways), the shakes of his parts meeting the ground as he falls (`FALL_IMPACTS`), and the sway
/// itself, turning his model about his feet on top of its own place.
#[allow(clippy::too_many_arguments)]
pub fn reactions(
    tuning: Res<TuningRes>,
    player: Single<&crate::player::Player>,
    sound: Option<Res<crate::sound::SoundOut>>,
    mut rig: ResMut<crate::camera::Rig>,
    mut robot: Query<(Entity, &mut Transform), With<crate::player::RobotModel>>,
    mut seen: Local<(u32, u32, f32)>,
    mut rest: Local<Option<(Entity, Transform)>>,
) {
    use tf2_core::giant::{FALL_IMPACTS, STAGGER_SHAKE, STAGGER_SOUND, STAGGER_SWAY_SOUND};
    let t = &tuning.0;
    if t.giant.is_none() {
        return;
    }
    let state = &player.state;
    let chest = state.pos + Vec3::Z * t.robot_height * t.aiming_height_percentage * 0.01;
    let (staggers, kicks, fall_tick) = &mut *seen;
    if state.giant_staggers != *staggers {
        *staggers = state.giant_staggers;
        rig.start_named_shake(STAGGER_SHAKE, t);
        if let Some(sound) = &sound {
            sound.play_event_at(STAGGER_SOUND, chest);
        }
    }
    if state.giant_stagger.kicks != *kicks {
        *kicks = state.giant_stagger.kicks;
        if let Some(sound) = &sound {
            sound.play_event_at(STAGGER_SWAY_SOUND, chest);
        }
    }
    let falling = state.action_clip(t).filter(|(set, id, _)| *set == "DeathSet" && *id == "Death").map(|(_, _, tick)| tick);
    if let Some(tick) = falling {
        for (at, shake) in FALL_IMPACTS {
            if *fall_tick < at && tick >= at {
                rig.start_named_shake(shake, t);
            }
        }
    }
    *fall_tick = falling.unwrap_or(0.0);
    let Ok((entity, mut transform)) = robot.single_mut() else { return };
    if rest.is_none_or(|(e, _)| e != entity) {
        *rest = Some((entity, *transform));
    }
    let Some((_, base)) = *rest else { return };
    let lean = state.giant_stagger.lean;
    // As `player::present` turns him: pitch lifts his front, roll his left.
    transform.rotation = Quat::from_rotation_x(lean.x) * Quat::from_rotation_z(-lean.y) * base.rotation;
}

/// The dressing's kept emitters, by the port's own ids (`Effects::start_giant`; the range is free of
/// the data's eventIds and of the effects' own keys): a gatling pod's overheat smoke and a broken
/// pod's smoke (plus the pod's number), his burning, and the eruption's smoke.
const OVERHEAT_SMOKE: u32 = 0x7e00_0000;
const DAMAGE_SMOKE: u32 = 0x7e00_0100;
const BURNING: u32 = 0x7e00_0200;
const ERUPTION_SMOKE: u32 = 0x7e00_0300;
/// [assumed port design] Seconds the eruption's smoke pours after a firing; seconds between a
/// broken pod's sparks at the first stage (fewer the more he is broken).
const ERUPTION_SMOKE_TIME: f32 = 1.5;
const SPARK_EVERY: [f32; 2] = [1.5, 4.5];

#[derive(Default)]
pub struct Dressed {
    stage: usize,
    eruptions: Option<u32>,
    overheated: bool,
    eruption_smoke: f32,
    spark_wait: f32,
    dead: bool,
    seed: u32,
    /// Seconds since his last roar and since his last firing, his firings and action clip seen.
    since_roar: f32,
    quiet: f32,
    firings: Option<u32>,
    clip: String,
    /// His ambience: the engine and the wind kept, the engine's rpm, and the next creak's wait.
    engine: Option<tf2_core::sound::Handle>,
    wind: Option<tf2_core::sound::Handle>,
    rpm: f32,
    creak_wait: f32,
}

/// [assumed port design] What the audit of 2026-10-23 found missing on him (`docs/devastator.md`),
/// from the boss mission's own effects and sounds (`tf2_core::giant::Dressing`, `SOUNDS`):
/// - his gatling pods smoke (their `smk_deva_gunpod`) while the overheat locks them (its sound is
///   the pods' weapon's, `Arsenal::load_giant`);
/// - an eruption bursts at its vent (`r_devastator_damaged`), pours smoke for `ERUPTION_SMOKE_TIME`,
///   booms (`ERUPTION_SOUND`) and shakes the camera;
/// - his damage (`giant::damage_stage`): at each stage a third of his pods blow (the burst, sparks,
///   a large blast, his roar and a hard shake) and smoke from then on, sparking now and then; under
///   the last stage his back burns (`smk_plumesmokelarge`, started again while he stays there);
///   mended back over a stage, its smoke stops;
/// - as he dies, his roar and the death cinematic's sound bed.
#[allow(clippy::too_many_arguments)]
pub fn dress(
    time: Res<Time>,
    tuning: Res<TuningRes>,
    player: Single<&crate::player::Player>,
    mut arsenal: Option<ResMut<crate::weapons::Arsenal>>,
    sound: Option<Res<crate::sound::SoundOut>>,
    effects: Option<ResMut<crate::effects::Effects>>,
    mut rig: ResMut<crate::camera::Rig>,
    pods: Query<(&PodModel, &crate::assets::ObjectNodes)>,
    robot: Query<&crate::assets::ObjectNodes, With<crate::player::RobotModel>>,
    placed: Query<&GlobalTransform>,
    mut seen: Local<Dressed>,
) {
    use tf2_core::giant::{self as g, DAMAGE_STAGES};
    let t = &tuning.0;
    let (Some(giant), Some(mut effects)) = (t.giant.as_ref(), effects) else { return };
    let dressing = &giant.dressing;
    let state = &player.state;
    let dt = time.delta_secs();
    let seen = &mut *seen;
    let roll = |seed: &mut u32| {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*seed >> 8) as f32 / (1u32 << 24) as f32
    };
    let at = |entity: Entity| placed.get(entity).ok().map(|p| crate::assets::bevy_to_game(p.translation()));
    let node = |nodes: &crate::assets::ObjectNodes, kind: usize, index: usize| {
        let index = g::pod_effect_node(kind, index);
        nodes.entities.get(index.min(nodes.entities.len().saturating_sub(1))).copied()
    };
    let play = |id: u32, point: Vec3| {
        if let Some(sound) = &sound {
            sound.play_event_at(id, point);
        }
    };
    let chest = state.pos + Vec3::Z * t.robot_height * t.aiming_height_percentage * 0.01;

    // The overheat's smoke on every gatling pod.
    let gatling = giant.pods.iter().position(|p| p.kind == g::GATLING);
    let hot = arsenal.as_ref().zip(gatling).is_some_and(|(a, i)| a.car_overheated(i));
    if hot != seen.overheated {
        seen.overheated = hot;
        for (pod, nodes) in pods.iter().filter(|(p, _)| p.kind == g::GATLING) {
            let id = OVERHEAT_SMOKE + pod.index as u32;
            match (hot, &dressing.pod_smoke, node(nodes, pod.kind, g::POD_SMOKE_NODE)) {
                (true, Some(smoke), Some(entity)) => {
                    let place = Affine3A::from_translation(at(entity).unwrap_or(chest));
                    effects.start_giant(smoke, Some(entity), place, id);
                }
                _ => effects.stop_giant(id),
            }
        }
    }

    // His blasts shake the camera by how near they land.
    if let Some(arsenal) = arsenal.as_mut() {
        for (at, bomb) in arsenal.giant_blasts.drain(..) {
            rig.start_named_shake_at(if bomb { g::BOMB_BLAST_SHAKE } else { g::MISSILE_BLAST_SHAKE }, t, at);
        }
    }

    // The eruption's launch.
    if let Some(arsenal) = &arsenal {
        let was = *seen.eruptions.get_or_insert(arsenal.eruptions);
        if arsenal.eruptions != was {
            seen.eruptions = Some(arsenal.eruptions);
            let vent = arsenal.eruption_at;
            if let Some(blast) = &dressing.pod_blast {
                effects.start_giant(blast, None, Affine3A::from_translation(vent), 0);
            }
            if let Some(smoke) = &dressing.pod_smoke
                && !effects.giant_running(ERUPTION_SMOKE) {
                effects.start_giant(smoke, None, Affine3A::from_translation(vent), ERUPTION_SMOKE);
            }
            seen.eruption_smoke = ERUPTION_SMOKE_TIME;
            rig.start_named_shake(g::ERUPTION_SHAKE, t);
            play(g::ERUPTION_SOUND, vent);
        }
    }
    if seen.eruption_smoke > 0.0 {
        seen.eruption_smoke -= dt;
        if seen.eruption_smoke <= 0.0 {
            effects.stop_giant(ERUPTION_SMOKE);
        }
    }

    // His damage.
    let share = if t.base_health > 0.0 { state.health / t.base_health } else { 1.0 };
    let stage = g::damage_stage(share, seen.stage);
    let last = DAMAGE_STAGES.len();
    if stage > seen.stage {
        for reached in seen.stage + 1..=stage {
            for (pod, nodes) in pods.iter().filter(|(p, _)| g::pod_breaks_at(p.index) == reached) {
                let (Some(spark_node), Some(smoke_node)) = (node(nodes, pod.kind, g::POD_SPARK_NODE), node(nodes, pod.kind, g::POD_SMOKE_NODE)) else { continue };
                let point = at(spark_node).unwrap_or(chest);
                let local = |p: Vec3| {
                    let (d, f) = (p - state.pos, state.body_forward());
                    format!("({:.1} right, {:.1} ahead, {:.1} up)", d.x * f.y - d.y * f.x, d.x * f.x + d.y * f.y, d.z)
                };
                if std::env::var_os("TF_DBG").is_some() {
                    let root = nodes.entities.first().copied().and_then(at);
                    eprintln!("giant: pod {} (kind {}, {} nodes) breaks: root {}, sparks {}, smoke {}", pod.index, pod.kind,
                        nodes.entities.len(), root.map_or("-".into(), local), local(point), at(smoke_node).map_or("-".into(), local));
                }
                if let Some(blast) = &dressing.pod_blast {
                    effects.start_giant(blast, None, Affine3A::from_translation(point), 0);
                }
                if let Some(sparks) = &dressing.sparks {
                    effects.start_giant(sparks, Some(spark_node), Affine3A::from_translation(point), 0);
                }
                if let Some(smoke) = &dressing.pod_smoke {
                    let place = Affine3A::from_translation(at(smoke_node).unwrap_or(point));
                    effects.start_giant(smoke, Some(smoke_node), place, DAMAGE_SMOKE + pod.index as u32);
                }
                play(g::POD_BLOW_SOUND, point);
            }
            eprintln!("giant: damage stage {reached} of {last} (health {:.0}%)", share * 100.0);
        }
        // One roar and shake however many stages the hit crossed.
        if !state.dead() {
            play(g::ROAR_SOUND, chest);
            seen.since_roar = 0.0;
        }
        rig.start_named_shake(g::POD_BLOW_SHAKE, t);
        seen.spark_wait = SPARK_EVERY[0];
    } else if stage < seen.stage {
        for (pod, _) in pods.iter().filter(|(p, _)| g::pod_breaks_at(p.index) > stage) {
            effects.stop_giant(DAMAGE_SMOKE + pod.index as u32);
        }
        eprintln!("giant: mended to damage stage {stage} (health {:.0}%)", share * 100.0);
    }
    seen.stage = stage;
    // Under the last stage his back burns, the plume started again as it ends.
    let back = robot.iter().next().and_then(|nodes| {
        let i = nodes.name_hashes.iter().position(|&h| h == g::ERUPTION_VENT)?;
        nodes.entities.get(i).copied()
    });
    if stage >= last && !effects.giant_running(BURNING) {
        if let (Some(plume), Some(entity)) = (&dressing.plume, back) {
            let point = at(entity).unwrap_or(chest);
            effects.start_giant(plume, Some(entity), Affine3A::from_translation(point), BURNING);
            play(FIRE_SOUND, point);
        }
    } else if stage < last && effects.giant_running(BURNING) {
        effects.stop_giant(BURNING);
    }
    // A broken pod sparks now and then.
    if stage > 0 {
        seen.spark_wait -= dt;
        if seen.spark_wait <= 0.0 {
            let broken: Vec<_> = pods.iter().filter(|(p, _)| g::pod_breaks_at(p.index) <= stage).collect();
            let pick = (roll(&mut seen.seed) * broken.len() as f32) as usize;
            if let (Some((pod, nodes)), Some(sparks)) = (broken.get(pick.min(broken.len().saturating_sub(1))), &dressing.sparks)
                && let Some(entity) = node(nodes, pod.kind, g::POD_SPARK_NODE) {
                let point = at(entity).unwrap_or(chest);
                effects.start_giant(sparks, Some(entity), Affine3A::from_translation(point), 0);
                play(g::SPARK_SOUND, point);
            }
            let wait = SPARK_EVERY[0] + roll(&mut seen.seed) * (SPARK_EVERY[1] - SPARK_EVERY[0]);
            seen.spark_wait = wait / stage as f32;
        }
    }

    // His roar as he attacks: at his pound's and vortex's windup, and at a burst's start.
    let dead = state.dead();
    seen.since_roar += dt;
    seen.quiet += dt;
    let clip = state.action_clip(t).map_or("", |(_, id, _)| id);
    let attacking = clip.contains("Attack");
    let mut roar = false;
    if clip != seen.clip {
        roar |= attacking && !dead;
        seen.clip = clip.to_owned();
    }
    if let Some(arsenal) = &arsenal {
        let was = *seen.firings.get_or_insert(arsenal.giant_firings);
        if arsenal.giant_firings != was {
            seen.firings = Some(arsenal.giant_firings);
            roar |= seen.quiet >= g::ROAR_BURST_GAP && roll(&mut seen.seed) < g::ROAR_FIRE_CHANCE;
            seen.quiet = 0.0;
        }
    }
    if roar && seen.since_roar >= g::ROAR_REST {
        play(g::ROAR_SOUND, chest);
        seen.since_roar = 0.0;
    }

    // His ambience: the engine, the wind and the creaks of a standing giant.
    if let Some(sound) = &sound {
        let head = state.pos + Vec3::Z * t.robot_height;
        if dead {
            for handle in [seen.engine.take(), seen.wind.take()].into_iter().flatten() {
                sound.stop(handle);
            }
        } else {
            if seen.engine.is_none() {
                seen.engine = sound.start_kept(g::ENGINE_SOUND, None);
                seen.rpm = g::ENGINE_RPM[0];
            }
            match seen.wind {
                Some(handle) => sound.move_to(handle, head),
                None => {
                    seen.wind = sound.start_kept_at(g::WIND_SOUND, None, head);
                    if let Some(handle) = seen.wind {
                        sound.set_param(handle, "speed", g::WIND_SPEED);
                    }
                }
            }
            // The engine works harder with his walk and his attacks, and eases back over a second or two.
            let walk = (state.vel.truncate().length() / 5.0).clamp(0.0, 1.0);
            let effort = if attacking || seen.quiet < 0.3 { 1.0 } else { walk * 0.7 };
            let wanted = g::ENGINE_RPM[0] + (g::ENGINE_RPM[1] - g::ENGINE_RPM[0]) * effort;
            seen.rpm += (wanted - seen.rpm) * (dt * if wanted > seen.rpm { 2.5 } else { 0.8 }).min(1.0);
            if let Some(handle) = seen.engine {
                sound.set_param(handle, "rpm", seen.rpm);
                sound.set_param(handle, "load", effort);
            }
            seen.creak_wait -= dt;
            if seen.creak_wait <= 0.0 {
                if walk < 0.1 && !attacking {
                    let which = (roll(&mut seen.seed) * g::IDLE_CREAKS.len() as f32) as usize;
                    play(g::IDLE_CREAKS[which.min(g::IDLE_CREAKS.len() - 1)], chest);
                }
                let [low, high] = g::IDLE_CREAK_EVERY;
                seen.creak_wait = low + roll(&mut seen.seed) * (high - low);
            }
        }
    }

    // His death.
    if dead && !seen.dead {
        play(g::ROAR_SOUND, chest);
        play(g::DEATH_SOUND, chest);
    }
    seen.dead = dead;
}

/// [assumed port design] His burning's sound as it catches.
const FIRE_SOUND: u32 = tf2_core::formats::hash::crc32(b"FIRE_LG");

/// TEST AID, not game behaviour: `--hurt-at <s>:<damage>[,<s>:<damage>...]` lands one enemy hit
/// on the player at each time (seconds since start), pushing him back from ahead; `<damage>`
/// over his health kills him. For captures of a stagger and a fall without a scout squad.
pub fn hurt_aid(
    time: Res<Time>,
    tuning: Res<TuningRes>,
    mut player: Single<&mut crate::player::Player>,
    mut hits: Local<Option<Vec<(f32, f32)>>>,
) {
    let hits = hits.get_or_insert_with(|| {
        let args: Vec<String> = std::env::args().collect();
        let list = args.iter().position(|a| a == "--hurt-at").and_then(|i| args.get(i + 1));
        list.map_or(Vec::new(), |list| list.split(',').filter_map(|hit| {
            let (at, damage) = hit.split_once(':')?;
            Some((at.parse().ok()?, damage.parse().ok()?))
        }).collect())
    });
    let now = time.elapsed_secs();
    let t = &tuning.0;
    let due: Vec<f32> = hits.iter().filter(|(at, _)| *at <= now).map(|(_, damage)| *damage).collect();
    hits.retain(|(at, _)| *at > now);
    for amount in due {
        let state = &mut player.state;
        let ahead = Vec3::new(-state.yaw.sin(), state.yaw.cos(), 0.0);
        let enemy = tf2_core::damage::Actor { id: u32::MAX, team: state.actor.team.wrapping_add(1), ..Default::default() };
        let packet = tf2_core::damage::Packet { source: Some(enemy), amount, direction: -ahead, ..Default::default() };
        let accepted = state.receive_damage(packet, t).accepted;
        eprintln!("hurt-at {now:.2}: {amount} damage, accepted {accepted}, health {:.0}", state.health);
    }
}

/// One of his pods' drawn models (`tf2_core::giant::Pods::object`): its kind (`Pods::kind`) and its
/// number among all his pods.
#[derive(Component)]
pub struct PodModel {
    pub kind: usize,
    pub index: usize,
}

/// His pods drawn on the bones they hang from, as the hand weapons hang on theirs (`attach_models`:
/// a child of the node's frame, identity). [data: the boss mission's `BonePositioner`s] [assumed: the
/// pod takes the bone's frame whole, at its own size; the mission's Instance matrices are scale 0.5
/// before the positioner moves them, and whether it keeps that scale is not read]. Made again for a
/// new robot model; a model of anyone else gets none.
#[allow(clippy::too_many_arguments)]
pub fn pods(
    data: Res<crate::GameData>,
    tuning: Res<TuningRes>,
    robot: Option<Single<(Entity, &crate::assets::ObjectNodes), With<crate::player::RobotModel>>>,
    mut made_for: Local<Option<Entity>>,
    mut cache: Local<crate::assets::Cache>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surfaces: ResMut<Assets<crate::surface::Surface>>,
    mut glows: ResMut<Assets<crate::fx::Glow>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
) {
    let Some(robot) = robot else { return };
    let (entity, nodes) = *robot;
    if *made_for == Some(entity) || nodes.entities.is_empty() {
        return;
    }
    *made_for = Some(entity);
    let Some(giant) = tuning.0.giant.as_ref() else { return };
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
    let mut count = 0;
    for pods in &giant.pods {
        let Some(object) = &pods.object else { continue };
        for mount in &pods.mounts {
            let Some(node) = nodes.name_hashes.iter().position(|&h| h == mount.bone) else { continue };
            let (model, _) = builder.spawn(&mut commands, &object.0, "pod");
            commands.entity(model).insert((PodModel { kind: pods.kind, index: count }, Transform::IDENTITY));
            commands.entity(nodes.frame(node)).add_child(model);
            count += 1;
        }
    }
    eprintln!("devastator: {count} pods drawn on his attach points");
}

/// A round of his gatling in flight, drawn as a slug of light (`tracers`).
#[derive(Component)]
pub struct Tracer {
    from: Vec3,
    to: Vec3,
    speed: f32,
    travelled: f32,
}

/// [assumed port design] His gatling's tracers. The boss's round (`laserrifle`) has no emitter of
/// its own [data], so in the game nothing is seen of it in flight; from a camera 50 to 100 m off a
/// man-sized flash and impact are all a player sees of sixteen barrels. Each round here is a glowing
/// slug `TRACER_LENGTH` long and `TRACER_RADIUS` thick flying at the round's own speed (300 m/s
/// [data]) from its barrel to where it lands, gone as it gets there. The damage lands as it fires,
/// as the game's does (`Arsenal::shoot`).
const TRACER_LENGTH: f32 = 14.0;
const TRACER_RADIUS: f32 = 0.35;
const TRACER_COLOUR: Vec3 = Vec3::new(14.0, 7.5, 2.0);

pub fn tracers(
    time: Res<Time>,
    arsenal: Option<ResMut<crate::weapons::Arsenal>>,
    mut flying: Query<(Entity, &mut Tracer, &mut Transform, &mut Visibility)>,
    mut look: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    use crate::assets::game_to_bevy;
    let dt = time.delta_secs();
    for (entity, mut tracer, mut transform, mut visibility) in &mut flying {
        tracer.travelled += tracer.speed * dt;
        let way = tracer.to - tracer.from;
        let length = way.length();
        if tracer.travelled - TRACER_LENGTH >= length {
            commands.entity(entity).despawn();
            continue;
        }
        // The slug's head runs ahead, its tail starts at the barrel and stops where it lands.
        let along = way / length.max(1e-3);
        let head = tracer.from + along * tracer.travelled.min(length);
        let tail = tracer.from + along * (tracer.travelled - TRACER_LENGTH).clamp(0.0, length);
        let span = (head - tail).length();
        transform.translation = game_to_bevy((head + tail) * 0.5);
        transform.rotation = Quat::from_rotation_arc(Vec3::Y, game_to_bevy(along).normalize_or(Vec3::Y));
        transform.scale = Vec3::new(1.0, span.max(0.01), 1.0);
        *visibility = Visibility::Inherited;
    }
    let Some(mut arsenal) = arsenal else { return };
    if arsenal.giant_rounds.is_empty() {
        return;
    }
    let (mesh, material) = look.get_or_insert_with(|| {
        (meshes.add(Cylinder::new(TRACER_RADIUS, 1.0)), materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.75, 0.3),
            emissive: LinearRgba::rgb(TRACER_COLOUR.x, TRACER_COLOUR.y, TRACER_COLOUR.z),
            unlit: true,
            ..default()
        }))
    }).clone();
    for (from, to, speed) in arsenal.giant_rounds.drain(..) {
        commands.spawn((
            Tracer { from, to, speed: speed.max(1.0), travelled: 0.0 },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(game_to_bevy(from)),
            Visibility::Hidden,
            bevy::light::NotShadowCaster,
        ));
    }
}

/// [assumed port design] His fill light. His camera looks the way he faces, so on the arena he is
/// mostly seen from behind; the stage's one sun lights his front, and the rest of him is the stand-in
/// ambient: a 60 m back that reads as a black mass. The game's own rig (try `--arena-rig`) changes
/// little. So each of his own Surface materials gets `GIANT_FILL` added to its ambient (the shader's
/// Ambient_Boost term), the way a film lights a giant from the camera's side. Only his model's.
const GIANT_FILL: Vec3 = Vec3::new(0.2, 0.19, 0.17);

pub fn fill(
    tuning: Res<TuningRes>,
    parts: Query<(Entity, &MeshMaterial3d<crate::surface::Surface>)>,
    parents: Query<&ChildOf>,
    robots: Query<(), With<crate::player::RobotModel>>,
    mut surfaces: ResMut<Assets<crate::surface::Surface>>,
    mut lifted: Local<std::collections::HashSet<AssetId<crate::surface::Surface>>>,
) {
    if tuning.0.giant.is_none() {
        return;
    }
    for (part, material) in &parts {
        if lifted.contains(&material.id()) || !parents.iter_ancestors(part).any(|a| robots.contains(a)) {
            continue;
        }
        if let Some(mut surface) = surfaces.get_mut(material.id()) {
            surface.lift_ambient(GIANT_FILL);
            lifted.insert(material.id());
        }
    }
}
