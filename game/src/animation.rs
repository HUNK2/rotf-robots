//! Plays the character's own clips on the robot's and the vehicle's node trees.
//!
//! The clips, their loop flags, blend times, speeds, model events and sound cues are the
//! game's, and so is the choice of which one plays. A control state that plays a clip of its
//! own (the attacks, the dodge, the climb, both halves of the change of form, the weapon
//! set) names it; the rest (standing, walking, running, stopping, jumping, falling,
//! landing) is the base layer's clip, picked by the sets' own entry and exit rules
//! (`tf2_core::animrules::Base`, run by the simulation). Either is drawn at the tick the
//! movement keeps, so what is drawn and what the movement acts on are the same moment.

use bevy::prelude::*;

use crate::assets::ObjectNodes;
use crate::camera::Rig;
use crate::formats::anim::{AnimRef, AnimSet, Clip, TICKS_PER_SECOND};
use crate::formats::hash::crc32;
use crate::player::{Player, RobotModel, TuningRes, VehicleModel};
use tf2_core::pose::{blend_turn, offset_position, offset_scale, offset_turn, stored_turn};
use tf2_core::animblend::{Blend, Clock};
use crate::sim::Mode;
use crate::sound::SoundOut;

const ROOT: u32 = crc32(b"Root");
/// Blend time for clips whose own is negative, which the data uses for "no value given"
/// (`FUN_00748f00` takes 0.1 for any blend time under 0). [game]
const DEFAULT_CROSSFADE: f32 = 0.1;
/// The tyre nodes the car's pose turns and drops, in the game's order, each with the wheel
/// (front left, front right, rear left, rear right) whose spin and contact it takes:
/// `drive_pose_wheels` 0085fd90 calls `drive_render_wheel` 008601d0 for these ten names, and
/// 01185740 maps each name to its wheel (`Mid` and `Mid2` to the rear pair, `Mid1` to the
/// front). A name the model lacks is skipped. Optimus's truck has `Tire_Mid_L/R`. [game]
const TYRES: [(&str, usize); 10] = [
    ("Tire_Front_L", 0), ("Tire_Front_R", 1), ("Tire_Rear_L", 2), ("Tire_Rear_R", 3), ("Tire_Mid_L", 2),
    ("Tire_Mid_R", 3), ("Tire_Mid1_L", 0), ("Tire_Mid1_R", 1), ("Tire_Mid2_L", 2), ("Tire_Mid2_R", 3),
];

#[derive(Resource)]
pub struct AnimLibrary {
    pub robot: AnimSet,
    /// The clips the vehicle model carries for itself.
    pub vehicle: Vec<AnimRef>,
}

#[derive(Component)]
pub struct Animator {
    /// A clip with model events (the change of form) says which model shows.
    transforming: bool,
    /// Position in the clip, in ticks, and how many of its model events have been applied.
    time: f32,
    events_done: usize,
    /// The clip has gone round since it started: its events that do not allow a rerun
    /// (`allowReRun` 0) are done with. [game] `FUN_0053ab90`
    went_round: bool,
    /// The still-playing outgoing clips and their nested fades. [game]
    blend: Option<Blend>,
    pose: Vec<Transform>,
    /// The nodes' own scales that go with `pose` (`own_scales`).
    own: Vec<Transform>,
    foot_ik: tf2_core::footik::FootIk,
    show_robot: bool,
    show_vehicle: bool,
    /// Hold one clip at one time instead of following the simulation, for captures.
    pub forced: Option<(String, String, f32)>,
    /// The clip the movement says is playing (a control state's, or the base layer's), by set
    /// and id, and the base layer's count of clips started when it was taken up, which tells
    /// one clip started again from one still playing.
    action: Option<(String, String)>,
    serial: Option<u32>,
}

/// The vehicle model's own clip player; the robot's clips tell it what to play.
#[derive(Component, Default)]
pub struct VehicleAnimator {
    clip: tf2_core::vehicle_clip::Driver,
    /// The clip's pose last frame, before the tyres and the flight surfaces: a node the next clip
    /// has no channel for keeps it (00544e00 writes only the output's channels). [game]
    last: Vec<Transform>,
}

impl Animator {
    pub fn new(rest: &[Transform]) -> Self {
        Self {
            transforming: false,
            time: 0.0,
            events_done: 0,
            went_round: false,
            blend: None,
            pose: rest.to_vec(),
            own: vec![Transform::IDENTITY; rest.len()],
            foot_ik: tf2_core::footik::FootIk::default(),
            show_robot: true,
            show_vehicle: false,
            forced: None,
            action: None,
            serial: None,
        }
    }

    pub fn describe(&self) -> String {
        match (&self.forced, &self.action) {
            (Some((set, id, time)), _) => format!("{set}/{id} held at tick {time:.0}"),
            (None, Some((set, id))) => format!("{set}/{id} at tick {:.0}", self.time),
            (None, None) => "none".into(),
        }
    }
}

fn find_vehicle<'a>(clips: &'a [AnimRef], id: &str) -> Option<(&'a AnimRef, &'a Clip)> {
    clips.iter().find(|r| r.id == id).and_then(|r| r.clip.as_ref().map(|clip| (r, clip)))
}

fn find_clock(set: &AnimSet, clock: Clock) -> Option<(&AnimRef, &Clip)> {
    let refs = set.sets.iter().find(|(name, _)| crc32(name.as_bytes()) == clock.set)?.1;
    let r = refs.iter().find(|r| crc32(r.id.as_bytes()) == clock.clip)?;
    Some((r, r.clip.as_ref()?))
}

/// Weapon-object fades use the same moving player tree as the character. [game:
/// FUN_007c3870 creates a BlendAnimation with the old clip and the new clip's xfadeTime]
#[derive(Default)]
pub(crate) struct ObjectBlend {
    blend: Option<Blend>,
    last: Option<(u32, f32)>,
}

impl ObjectBlend {
    pub(crate) fn pose(&mut self, refs: &[AnimRef], id: u32, tick: f32, dt: f32, nodes: &ObjectNodes) -> Vec<Transform> {
        let Some(r) = refs.iter().find(|r| crc32(r.id.as_bytes()) == id) else {
            *self = Self::default();
            return nodes.rest.clone();
        };
        let Some(clip) = r.clip.as_ref() else { return nodes.rest.clone() };
        let changed = self.last.is_none_or(|(old, at)| old != id || (!r.looping && tick < at));
        if changed {
            let clock = Clock { set: 0, clip: id, tick, length: clip.length, speed: r.speed, looping: r.looping, exit_events: r.exit_events };
            // Object clips take their own time directly; a negative value swaps at once.
            // [game: FUN_007c3870, unlike the character starter's 0.1s fallback]
            let duration = r.crossfade;
            match self.blend.as_mut() {
                Some(blend) => blend.start(clock, duration),
                None => self.blend = Some(Blend::new(clock)),
            }
        }
        self.last = Some((id, tick));
        let blend = self.blend.as_mut().unwrap();
        blend.update(tick, dt, &mut |_, _, _| {});
        blend.evaluate(&|clock| {
            refs.iter().find(|r| crc32(r.id.as_bytes()) == clock.clip).and_then(|r| {
                r.clip.as_ref().map(|clip| sample(clip, r.looping, clock.tick, &nodes.name_hashes, &nodes.rest))
            }).unwrap_or_else(|| nodes.rest.clone())
        }, &|from, mut pose, weight| {
            cross_fade(&mut pose, &from, weight);
            pose
        })
    }
}

/// The pose a clip gives at `time`, for every node; nodes the clip does not move keep `rest`.
pub(crate) fn sample(clip: &Clip, looping: bool, time: f32, name_hashes: &[u32], rest: &[Transform]) -> Vec<Transform> {
    name_hashes
        .iter()
        .zip(rest)
        .map(|(&name, rest)| {
            // A node the clip has a channel for takes no move, no turn and scale 1 where the
            // channel keys none: the game starts each channel's output there (0053b520) and
            // writes the whole of it (00544e00), not the rest's (`Channel::local`). [game]
            // Breakaway's and Jetfire's guns rest folded (0.08 to 0.56) and their clips leave the
            // scale out; Breakaway's `Weapon_Idle` leaves his right forearm's turn out on a rest
            // bent 33 degrees in, and Long Haul's jumps and runs his forearms' on rests bent 74:
            // kept at rest, the rifle pointed in and down and his arms crossed (the user's
            // reports, 2026-10-25).
            let Some((p, q, s)) = clip.channel(name).and_then(|c| c.local(time, clip.length, looping)) else { return *rest };
            let mut out = *rest;
            // The root's place in a clip is where the clip was made, not where the model is:
            // clips that follow each other carry on from each other's (the car's idle sits 4
            // behind, part B of a change of form starts 3.5 ahead). Its travel is movement,
            // which the simulation does, so the root itself stays put. [data]
            // Its turn likewise: the strafes and side dodges are made running along a root
            // turned a quarter round, and that turn says which way the travel goes. [data]
            if name != ROOT {
                out.translation = Vec3::from(p);
                // Stored for the game's row-vector matrices: the same turn the other way round here.
                out.rotation = Quat::from_xyzw(-q[0], -q[1], -q[2], q[3]);
            }
            out.scale = Vec3::from(s);
            out
        })
        .collect()
}

/// Each node's own scale from a clip at `time` (`Channel::own_scale`), as transforms for the
/// nodes' own frames (`ObjectNodes::frames`): one where the clip keys none, and never under
/// 0.001, as the game clamps it before keeping its inverse for the children. [game 00544e00]
/// The fades blend these as they blend the poses (`cross_fade`): BlendOutputs lerps the
/// output's `+0x18` scale with the same weight as its `+0x14` one. [game 0053d600]
pub(crate) fn own_scales(clip: &Clip, looping: bool, time: f32, name_hashes: &[u32]) -> Vec<Transform> {
    name_hashes
        .iter()
        .map(|&name| {
            let scale = clip.channel(name).and_then(|c| c.own_scale.sample(time, clip.length, looping));
            Transform::from_scale(scale.map_or(Vec3::ONE, |s| Vec3::from(s).max(Vec3::splat(0.001))))
        })
        .collect()
}

/// No own scale on any node.
pub(crate) fn unscaled(nodes: &ObjectNodes) -> Vec<Transform> {
    vec![Transform::IDENTITY; nodes.entities.len()]
}

/// Puts each node's own scale on its own frame (`ObjectNodes::frames`).
fn write_frames<F: bevy::ecs::query::QueryFilter>(nodes: &ObjectNodes, own: &[Transform], transforms: &mut Query<&mut Transform, F>) {
    for (frame, own) in nodes.frames.iter().zip(own) {
        if let Ok(mut frame) = transforms.get_mut(*frame) {
            *frame = *own;
        }
    }
}

/// The pose a clip is fading in over, blended toward the clip's by `weight`: turns go in a
/// straight line the short way and are normalised (`BlendAnimation::BlendOutputs`). [game]
pub(crate) fn cross_fade(pose: &mut [Transform], from: &[Transform], weight: f32) {
    for (to, from) in pose.iter_mut().zip(from) {
        to.translation = from.translation.lerp(to.translation, weight);
        to.rotation = blend_turn(from.rotation, to.rotation, weight);
        to.scale = from.scale.lerp(to.scale, weight);
    }
}

/// A clip of offsets laid over a pose by `weight`, on the nodes it has channels for
/// (`AdditiveAnimation::BlendOutputs`, `FUN_0053e180`). [game]
pub(crate) fn lay_offsets(pose: &mut [Transform], over: &Clip, tick: f32, looping: bool, weight: f32, name_hashes: &[u32]) {
    for (node, &name) in pose.iter_mut().zip(name_hashes) {
        let Some(channel) = over.channel(name).filter(|_| name != ROOT) else { continue };
        if let Some(p) = channel.position.sample(tick, over.length, looping) {
            node.translation = offset_position(node.translation, Vec3::from(p), weight);
        }
        if let Some(q) = channel.rotation.sample(tick, over.length, looping) {
            node.rotation = offset_turn(node.rotation, stored_turn(q), weight);
        }
        if let Some(s) = channel.scale.sample(tick, over.length, looping) {
            node.scale = offset_scale(node.scale, Vec3::from(s), weight);
        }
    }
}

/// The own scales an offsets clip keys, laid on the pose's own scales (`own_scales`) by the
/// rule of the ordinary ones: AdditiveAnimation::BlendOutputs writes the output's `+0x18` as
/// base x ((offset - 1) x weight + 1), as it does `+0x14`. [game 0053e180, l.142-149 of
/// `fn.py --full`] Kept above 0.001 as `own_scales` keeps them. [game 00544e00]
pub(crate) fn lay_own_offsets(own: &mut [Transform], over: &Clip, tick: f32, looping: bool, weight: f32, name_hashes: &[u32]) {
    for (node, &name) in own.iter_mut().zip(name_hashes) {
        let Some(channel) = over.channel(name).filter(|_| name != ROOT) else { continue };
        if let Some(s) = channel.own_scale.sample(tick, over.length, looping) {
            node.scale = offset_scale(node.scale, Vec3::from(s), weight).max(Vec3::splat(0.001));
        }
    }
}

/// The aim laid on a clip's pose: each weapon-set clip carries two offset clips, `Down`
/// and `Up` (spine, arms, neck and head turned 65 degrees in all), and the aim's rise says
/// which is on and how much (`aim::Aim::offset`). The clip's player is wrapped with them
/// when it starts (`MetaAdditiveAnimation`, `FUN_00748f00`), so the offsets are part of
/// the clip's own pose and fade in and out with it. [game] [data: the offset clips hold
/// one pose for their 4 ticks, so the time in them does not matter]
fn lay_aim(pose: &mut [Transform], r: &AnimRef, (side, amount): (usize, f32), name_hashes: &[u32]) {
    if let Some(over) = r.aim_offsets.get(side).filter(|_| amount > 0.0) {
        lay_offsets(pose, over, 0.0, false, amount, name_hashes);
    }
}

/// The port's foot planting, which the game does not do for its robots (see `animate`). ON by
/// default again (user, 2026-10-20) [stand-in]; `--no-foot-ik` turns it off to match the game.
fn foot_ik_wanted() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::args().any(|a| a == "--no-foot-ik"))
}

fn visibility(shown: bool) -> Visibility {
    if shown { Visibility::Inherited } else { Visibility::Hidden }
}

pub fn animate(
    time: Res<Time>,
    library: Res<AnimLibrary>,
    tuning: Res<TuningRes>,
    arena: Res<crate::world::ArenaRes>,
    data: Res<crate::GameData>,
    sound: Option<Res<SoundOut>>,
    arsenal: Option<Res<crate::weapons::Arsenal>>,
    mut rig: ResMut<Rig>,
    mut effects: ResMut<crate::effects::Effects>,
    mut trails: ResMut<crate::trails::Trails>,
    player: Single<&Player>,
    mut robot: Single<(&mut Animator, &ObjectNodes, &mut Visibility), (With<RobotModel>, Without<VehicleModel>)>,
    mut vehicle: Single<(&mut VehicleAnimator, &ObjectNodes, &mut Visibility), (Or<(With<VehicleModel>, With<crate::player::NoVehicle>)>, Without<RobotModel>)>,
    mut transforms: Query<&mut Transform, (Without<RobotModel>, Without<VehicleModel>)>,
) {
    let (animator, nodes, robot_visibility) = &mut *robot;
    let (driver, vehicle_nodes, vehicle_visibility) = &mut *vehicle;
    let state = &player.state;
    let dt = time.delta_secs();
    let set = &library.robot;
    let sound = sound.as_deref();

    let (pose, own) = if let Some((set_name, id, at)) = &animator.forced {
        match set.find(set_name, id).and_then(|r| r.clip.as_ref()) {
            Some(clip) => (sample(clip, false, *at, &nodes.name_hashes, &nodes.rest), own_scales(clip, false, *at, &nodes.name_hashes)),
            None => (nodes.rest.clone(), unscaled(nodes)),
        }
    } else if let Some((set_name, id, tick, base)) = state
        .action_clip(&tuning.0)
        .map(|(set, id, tick)| (set, id, tick, None))
        .or_else(|| state.base_clip(&tuning.0).map(|clip| (clip.set, clip.id, clip.tick, Some(clip))))
    {
        // A control state plays its own clip, or the base layer the one its rules picked, at
        // the tick the movement keeps, so what is drawn and what the movement acts on are the
        // same moment. [game]
        match set.find(set_name, id).and_then(|r| r.clip.as_ref().map(|clip| (r, clip))) {
            Some((r, clip)) => {
                let current = Some((set_name.to_owned(), id.to_owned()));
                let serial = base.map(|clip| clip.serial);
                let restarted = !r.looping && tick < animator.time;
                if animator.action != current || animator.serial != serial || restarted {
                    if let Some(sound) = sound {
                        sound.clip_changed(Some(clip));
                    }
                    effects.clip_changed();
                    trails.clip_changed();
                    animator.went_round = false;
                    // A clip fades in over the time the movement has for it: its own blend,
                    // or the one the clip before asked for on its way out where that names it
                    // (a landing into a run). [game]
                    let own = if r.crossfade < 0.0 { DEFAULT_CROSSFADE } else { r.crossfade };
                    let blend_time = match base {
                        Some(clip) => clip.blend,
                        None => state.weapon_fade().filter(|_| set_name == "WeaponSet").unwrap_or(own),
                    };
                    let clock = Clock {
                        set: crc32(set_name.as_bytes()), clip: crc32(id.as_bytes()), tick,
                        length: clip.length, speed: r.speed, looping: r.looping, exit_events: r.exit_events,
                    };
                    match animator.blend.as_mut() {
                        Some(blend) => blend.start(clock, blend_time),
                        None => animator.blend = Some(Blend::new(clock)),
                    }
                    animator.action = current;
                    animator.serial = serial;
                    // A weapon-set clip can start where the one before had got to
                    // (`usePrevAnimFrame`); its cues before that are not played.
                    animator.time = if state.mode == Mode::Strafe || state.weapon_hover() { tick } else { 0.0 };
                    animator.events_done = 0;
                }
                // A loop that has gone round plays the cues after its last position and up to
                // its new one.
                let due = tick >= animator.time || (r.looping && base.is_some());
                if let Some(sound) = sound.filter(|_| due) {
                    sound.clip_cues(clip, animator.time, tick, animator.went_round);
                }
                if due {
                    rig.clip_shakes(clip, animator.time, tick, state.pos, &tuning.0);
                    effects.clip_events(clip, animator.time, tick, animator.went_round);
                    trails.clip_events(clip, animator.time, tick);
                }
                animator.went_round |= tick < animator.time;
                // The clip's own events say which model shows and what the vehicle plays:
                // this is how the change of form swaps the robot and the car. [data]
                animator.transforming = !clip.model_events.is_empty();
                while let Some(event) = clip.model_events.get(animator.events_done).filter(|e| e.time <= tick) {
                    animator.events_done += 1;
                    animator.show_robot = event.show_true;
                    animator.show_vehicle = event.show_alternate;
                    if let Some(id) = &event.alternate_clip {
                        driver.clip.start(id, event.alternate_then.clone());
                    }
                }
                animator.time = tick;
                let aim = rig.aim.offset();
                if let Some(blend) = animator.blend.as_mut() {
                    blend.update(tick, dt, &mut |clock, before, after| {
                        if let Some((_, outgoing)) = find_clock(set, clock) {
                            let length = outgoing.length.max(1.0);
                            let from = if clock.looping { before % length } else { before };
                            let to = if clock.looping { after % length } else { after };
                            if let Some(sound) = sound {
                                sound.clip_cues(outgoing, from, to, before >= length);
                            }
                            rig.clip_shakes(outgoing, from, to, state.pos, &tuning.0);
                            effects.clip_events(outgoing, from, to, before >= length);
                        }
                    });
                    let mix = |from: Vec<Transform>, mut pose: Vec<Transform>, weight: f32| {
                        cross_fade(&mut pose, &from, weight);
                        pose
                    };
                    let pose = blend.evaluate(&|clock| {
                        let Some((r, clip)) = find_clock(set, clock) else { return nodes.rest.clone() };
                        let mut pose = sample(clip, r.looping, clock.tick, &nodes.name_hashes, &nodes.rest);
                        lay_aim(&mut pose, r, aim, &nodes.name_hashes);
                        pose
                    }, &mix);
                    // The own scales go through the same fades [game 0053d600], and the aim's
                    // offsets lay theirs on them as on the pose [game 0053e180].
                    let own = blend.evaluate(&|clock| {
                        let Some((r, clip)) = find_clock(set, clock) else { return unscaled(nodes) };
                        let mut own = own_scales(clip, r.looping, clock.tick, &nodes.name_hashes);
                        if let Some(over) = r.aim_offsets.get(aim.0).filter(|_| aim.1 > 0.0) {
                            lay_own_offsets(&mut own, over, 0.0, false, aim.1, &nodes.name_hashes);
                        }
                        own
                    }, &mix);
                    (pose, own)
                } else {
                    (sample(clip, r.looping, tick, &nodes.name_hashes, &nodes.rest), own_scales(clip, r.looping, tick, &nodes.name_hashes))
                }
            }
            None => (animator.pose.clone(), animator.own.clone()),
        }
    } else {
        // Nothing is playing on the robot: in the car, or before the first update has picked
        // a clip. It keeps the pose it had.
        if animator.action.is_some() { trails.clip_changed(); }
        animator.action = None;
        animator.serial = None;
        animator.transforming = false;
        (animator.pose.clone(), animator.own.clone())
    };
    // The weapon layer: its clip is laid over the nodes it has channels for (the right arm,
    // and for some the neck, head or spine), by the layer's weight. A clip of play type 1
    // (the recoil, the overheat) holds offsets, which are added to the pose under it; the
    // others hold poses, which replace it. [data] [game: FUN_0053e180 / FUN_0053d600]
    // What the next clip fades in over is the base layer's pose, before the layers above
    // it and the spine's twist are put on.
    animator.pose = pose.clone();
    animator.own = own.clone();
    let (mut pose, mut own) = (pose, own);
    let layer = arsenal.as_deref().filter(|_| animator.forced.is_none() && !state.is_vehicle()).and_then(|arsenal| {
        let clips = library.robot.sets.get(crate::weapons::WEAPON_LAYER_SET)?;
        let arm = arsenal.gun.arm?;
        let r = clips.get(arm.clip)?;
        Some((r, r.clip.as_ref()?, arm.tick, arm.weight()))
    });
    if let Some((r, clip, tick, weight)) = layer.filter(|layer| layer.3 > 0.0) {
        if r.additive {
            // The layer's `AdditiveAnimation`: the same routine as the aim's offsets.
            lay_offsets(&mut pose, clip, tick, r.looping, weight, &nodes.name_hashes);
            lay_own_offsets(&mut own, clip, tick, r.looping, weight, &nodes.name_hashes);
        } else {
            // The layer's `BlendAnimation`: the clip's nodes blended over the pose's, their own
            // scales too (Bumblebee's `Weapon_Switch` keys one on `Hand_R`). [game 0053d600]
            for ((node, own), &name) in pose.iter_mut().zip(own.iter_mut()).zip(&nodes.name_hashes) {
                let Some(channel) = clip.channel(name).filter(|_| name != ROOT) else { continue };
                if let Some(s) = channel.own_scale.sample(tick, clip.length, r.looping) {
                    own.scale = own.scale.lerp(Vec3::from(s).max(Vec3::splat(0.001)), weight);
                }
                // What the layer's player puts out for the node, unkeyed parts as `sample` has them.
                let Some((p, q, s)) = channel.local(tick, clip.length, r.looping) else { continue };
                node.translation = node.translation.lerp(Vec3::from(p), weight);
                node.rotation = blend_turn(node.rotation, stored_turn(q), weight);
                node.scale = node.scale.lerp(Vec3::from(s), weight);
            }
        }
    }
    // Layer 2 sits above the weapon arm. Its HitFront keys are additive offsets, so
    // locomotion and recoil continue underneath it. [game: FUN_0084dfa0 / FUN_007482c0]
    if animator.forced.is_none()
        && let Some(r) = state.hit_partial.current(&tuning.0.rule_sets).and_then(|c| {
        library.robot.sets.get("HitReactPartialSet")?.iter().find(|r| crc32(r.id.as_bytes()) == c.id)
    })
            && let Some(clip) = r.clip.as_ref() {
        lay_offsets(&mut pose, clip, state.hit_partial.tick, r.looping, 1.0, &nodes.name_hashes);
        lay_own_offsets(&mut own, clip, state.hit_partial.tick, r.looping, 1.0, &nodes.name_hashes);
    }
    // Standing in weapon mode his spine twists toward the aim (StrafeMode's `+0x24`, handed
    // to the model's `SpineIK`, which links `Spine_02` and `Spine_03`). [game] [data]
    // [game] The links share it equally: `FUN_0084e4f0` stores rotate_z(twist / link count)
    // in the SpineIK, and its update `FUN_0073d500` turns each link's world matrix by it
    // about the vertical through the bone (the translation is put back), then writes the
    // local matrix back (`FUN_00545850`, which marks the descendants dirty, so the second
    // link turns on top of the first).
    // Also while a special runs in StrafeMode outside the port's `Mode::Strafe` (Megatron's:
    // `sim::step_action`), whose twist the mode writes the same way. [game 00875ef0]
    let strafing = state.mode == Mode::Strafe || (state.mode == Mode::Action && state.strafe.strafing);
    let twist = if strafing && animator.forced.is_none() { state.strafe.twist } else { 0.0 };
    if twist != 0.0 && !nodes.spine_links.is_empty() {
        let share = Quat::from_rotation_z(twist / nodes.spine_links.len() as f32);
        for &link in &nodes.spine_links {
            // The turn its parents give it, so the twist is about his own vertical.
            let mut above = Quat::IDENTITY;
            let mut parent = nodes.parents[link];
            while let Some(p) = parent {
                above = pose[p].rotation * above;
                parent = nodes.parents[p];
            }
            pose[link].rotation = above.inverse() * share * above * pose[link].rotation;
        }
    }
    if !animator.transforming {
        // A robot flyer has no other model: it stays the robot in the air. [data]
        let in_car = state.is_vehicle() && !state.flies_as_robot(&tuning.0);
        animator.show_robot = !in_car;
        animator.show_vehicle = in_car;
    }
    // While sniping his object is hidden (`character_set_sniping` 00721ce0: 01176750 on
    // `char+0xc` sets the holder's `+0x60` bit 0, and the render instances leave the lists),
    // and shown again on leaving (00578490). `show_robot` itself is left alone. [game]
    **robot_visibility = visibility(animator.show_robot && !player.sniping);
    **vehicle_visibility = visibility(animator.show_vehicle);
    // Devastator plants his arms too, with a probe his size (`tf2_core::giant::limbs`) [assumed].
    let giant_limbs = tuning.0.giant.as_ref().and_then(|g| g.limbs.as_ref());
    if let Some(foot_rig) = giant_limbs.or(nodes.foot_rig.as_ref()) {
        // ON by default again (user's call, 2026-10-20) [stand-in]; `--no-foot-ik` turns it off.
        // The game itself puts no foot IK on the playable robots. [game: the
        // FootIK solver 0073d8d0 is built only by its data loader 0073f000, reached only through
        // the factory 0073f1b0 that 00ad7a10 registers; data: none of the 13 playable packs holds
        // a FootIK object (`footik::roster_survey`, 2026-10-18), all hold one SpineIK]
        // The port's own planting runs on ground locomotion and the weapon stance only
        // [assumed]. Reach: every playable robot, unless `--no-foot-ik` (`foot_ik_wanted`); no
        // playable pack holds the game's FootIK object, so no game gating applies.
        // settles: 0073d8d0's own state gating (which modes it runs in), read only if a debug
        // comparison with the game's foot IK is ever wanted (docs/foot-ik.md); not read here.
        // His turns, pound and vortex are actions he plays standing on all fours: his limbs stay
        // planted through them [assumed].
        let active = foot_ik_wanted() && animator.forced.is_none() && !animator.transforming && !state.dead()
            && (matches!(state.mode, Mode::Stand | Mode::Move | Mode::Strafe)
                || giant_limbs.is_some() && matches!(state.mode, Mode::Action))
            && state.ground_flag() && state.vel.z <= 0.0;
        let mut local: Vec<_> = pose.iter().map(Transform::compute_affine).collect();
        let root = bevy::math::Affine3A::from_rotation_translation(Quat::from_rotation_z(state.yaw), state.pos);
        animator.foot_ik.apply(foot_rig, &mut local, &nodes.parents, root, active, dt, &arena.0);
        for (pose, at) in pose.iter_mut().zip(local) {
            let (scale, rotation, translation) = at.to_scale_rotation_translation();
            *pose = Transform { scale, rotation, translation };
        }
    }
    let local: Vec<_> = pose.iter().map(Transform::compute_affine).collect();
    let root = bevy::math::Affine3A::from_rotation_translation(Quat::from_rotation_z(state.yaw), state.pos);
    let world: Vec<_> = tf2_core::footik::accumulate(&local, &nodes.parents).into_iter().map(|at| root * at).collect();
    effects.steps(&world, &data.0, state.body_forward(), &arena.0);
    if let Some(sound) = sound {
        sound.component_sources(false, &world);
        sound.footsteps(&world, &data.0.robot.footsteps, &arena.0, dt);
        sound.climb_steps(&world, &data.0.robot.climb_steps, state.body_forward(), &arena.0, dt);
        sound.spawn_cues(&world, &data.0.robot.spawn_sound_players);
    }
    for (entity, transform) in nodes.entities.iter().zip(&pose) {
        if let Ok(mut node) = transforms.get_mut(*entity) {
            *node = *transform;
        }
    }
    write_frames(nodes, &own, &mut transforms);

    // The vehicle: its own clip if one is playing, then the tyres on top.
    let mut vehicle_pose = vehicle_nodes.rest.clone();
    let mut vehicle_own = unscaled(vehicle_nodes);
    if driver.last.len() != vehicle_pose.len() {
        driver.last = vehicle_nodes.rest.clone();
    }
    // The vehicle form's animator takes his animation rate too (0071f4d0's `+0x258` branch:
    // its `+0x5c` and its players' rates). [game]
    let speed = driver.clip.playing.as_deref().and_then(|id| find_vehicle(&library.vehicle, id)).map_or(1.0, |(r, _)| r.speed);
    let step = driver.clip.advance(dt * TICKS_PER_SECOND * speed * state.special_rate, |id| {
        find_vehicle(&library.vehicle, id).map(|(r, clip)| (clip.length, r.looping))
    });
    if let Some((step, (r, clip))) = step.and_then(|s| find_vehicle(&library.vehicle, &s.clip).map(|found| (s, found))) {
        let (before, after) = step.passed;
        if let Some(sound) = sound.filter(|_| !r.looping && after > before) {
            sound.clip_cues(clip, before, after.min(clip.length), false);
        }
        // The model clip's `AnimSendTriggerEvent`s go to the vehicle object by name, as the
        // robot's do: the jets' and Blackout's take-off clip `Trans_R2V_B` starts their exhaust
        // (`FX_smoke_exhaust`) this way. [data] [game: the player's event walk 0053ab90]
        // A clip's events at tick 0 count on the frame after it is started, its first advance.
        let length = clip.length.max(1.0);
        for event in clip.action_events.iter().filter(|_| after > before) {
            let tf2_core::formats::anim::Action::Trigger { name } = event.action else { continue };
            let from_start = before <= 0.0 && event.time <= 0.0;
            let passed = if r.looping && after >= length {
                event.time > before || event.time <= after - length
            } else {
                (event.time > before || from_start) && event.time <= after.min(clip.length)
            };
            if passed {
                effects.vehicle_triggers.push(name);
            }
        }
        // A node the clip has no channel for keeps where the clip before left it. [game 00544e00]
        vehicle_pose = sample(clip, r.looping, step.time, &vehicle_nodes.name_hashes, &vehicle_nodes.rest);
        for ((pose, last), &name) in vehicle_pose.iter_mut().zip(&driver.last).zip(&vehicle_nodes.name_hashes) {
            if clip.channel(name).is_none() {
                *pose = *last;
            }
        }
        driver.last.clone_from(&vehicle_pose);
        // Megatron's tank stretches each link of its tread chains by its own scale, which the
        // next link does not take (`V_Idle`: about 1.31 along each link). [data] [game 00544e00]
        vehicle_own = own_scales(clip, r.looping, step.time, &vehicle_nodes.name_hashes);
    }
    // 008601d0: visual travel clamps, front steering multiplier and suspension offsets.
    // Model nodes remain in game space under the root's model-space conversion. [game]
    let presentation=state.car_presentation();
    let wheels=&presentation.wheels;
    let turn=presentation.rotation;
    let up=turn*Vec3::Z;
    let steer = Quat::from_rotation_z(presentation.steer_angle*-3.0);
    // Each tyre node the model has takes another of the frame's spin off its wheel's angle
    // (`drive_render_wheel` 008601d0: `wheel+0x60 -= spin` per call), so a wheel with two tyres
    // (Optimus's rear and middle) turns twice as fast as one with one; as the angle only ever
    // grows by the frame's spin, that is the wheel's angle times its tyre count. [game]
    // [assumed] the one-frame phase between two tyres of a wheel (the second call's extra spin,
    // 0.2 to 0.55 rad) is not drawn: both show the wheel's angle after the frame's calls.
    // Settles: nothing visible on a round tyre; a capture of Optimus's middle axle in the original.
    let tyres_on = |wheel: usize| TYRES.iter().filter(|&&(n, w)| w == wheel && vehicle_nodes.names.iter().any(|m| m == n)).count();
    let turns = [0, 1, 2, 3].map(|w| tyres_on(w).max(1) as f32);
    for (i, name) in vehicle_nodes.names.iter().enumerate() {
        if let Some(&(_, wheel))=TYRES.iter().find(|(n, _)|*n==name) {
            vehicle_pose[i].rotation *= Quat::from_rotation_x(wheels[wheel].render_spin * turns[wheel]);
        } else if name.starts_with("TireGOAL_Front") {
            vehicle_pose[i].rotation *= steer;
        }
    }
    if state.mode==Mode::Drive {
        for (name,wheel) in TYRES {
            let Some(node)=vehicle_nodes.names.iter().position(|n|n==name) else {continue;};
            let mut frame=vehicle_pose[node].to_matrix();
            let mut parent=vehicle_nodes.parents[node];
            while let Some(p)=parent {frame=vehicle_pose[p].to_matrix()*frame;parent=vehicle_nodes.parents[p];}
            let point=presentation.pos+turn*frame.w_axis.truncate();
            let offset=tf2_core::vehicle::Chassis::render_offset(&wheels[wheel],point.z,up.z,&tuning.0.drive);
            // World body-up becomes root-local +z; rotate through the node's parents.
            let mut parent_turn=Quat::IDENTITY;
            let mut parent=vehicle_nodes.parents[node];
            while let Some(p)=parent {parent_turn=vehicle_pose[p].rotation*parent_turn;parent=vehicle_nodes.parents[p];}
            vehicle_pose[node].translation-=parent_turn.inverse()*Vec3::Z*offset;
        }
    }
    // FlyMode's moving parts (0086cbf0, stepped in `sim::step_fly`): the jets' control surfaces
    // take the bare turn (zero translation: every surface sits on its `Parent_` pivot, [data]
    // `surface_nodes`), the rotors their summed turn after the clip's pose. [game]
    // [assumed] laid after the clip: the jets' transformation and bank clips drive some of these
    // nodes, and whether the clip's write (00544e00) or 0086cbf0's comes later in the game's
    // frame is not read; settles: the order of the anim controller's pose apply and the mode
    // update in the character's frame, or a capture of Starscream banking.
    if state.fly.is_some() {
        let turns = state.flight_surfaces.pose(&tuning.0.forms.fly);
        let mut rotation: Vec<Quat> = vehicle_pose.iter().map(|t| t.rotation).collect();
        let mut translation: Vec<Vec3> = vehicle_pose.iter().map(|t| t.translation).collect();
        tf2_core::flightsurface::apply(&turns, &vehicle_nodes.names, &mut rotation, &mut translation);
        for ((pose, rotation), translation) in vehicle_pose.iter_mut().zip(rotation).zip(translation) {
            (pose.rotation, pose.translation) = (rotation, translation);
        }
    }
    if let Some(sound) = sound {
        let local: Vec<_> = vehicle_pose.iter().map(Transform::compute_affine).collect();
        let root = bevy::math::Affine3A::from_rotation_translation(turn, presentation.pos);
        let world: Vec<_> = tf2_core::footik::accumulate(&local, &vehicle_nodes.parents)
            .into_iter().map(|at| root * at).collect();
        sound.component_sources(true, &world);
    }
    for (entity, transform) in vehicle_nodes.entities.iter().zip(&vehicle_pose) {
        if let Ok(mut node) = transforms.get_mut(*entity) {
            *node = *transform;
        }
    }
    write_frames(vehicle_nodes, &vehicle_own, &mut transforms);
}
