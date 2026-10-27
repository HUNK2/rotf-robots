//! One robot's whole update without a window, in the order the Bevy game runs it
//! (`game/src/player.rs` `tick`, `game/src/targets.rs`): the pad as the player holds it, the aim
//! in weapon mode, the movement (`sim::step`), the camera, his weapons (`arsenal`), and the
//! scouts that fight him (`combat::Enemy`), hits going both ways. The reference runs
//! (rotf-export `--run`) are this, update by update.
//!
//! What it leaves out of the Bevy game, and how it differs:
//! - Muzzles are placed on his skeleton posed by the clip the movement plays (or the base layer)
//!   with the weapon layer laid over it (`pose::Rig::pose_layer`), from the node the weapon hangs
//!   on. The Bevy game places them on the drawn model, which also blends clips, lays the aim's
//!   offsets and the hit reactions on, twists the spine and plants the feet, and reads a weapon
//!   model's own `FireDummy` where its model is drawn. So a round can leave a few centimetres
//!   from where it leaves in the game; where it goes is the same (the camera's line).
//!   [port design]
//! - The car's weapons hang on the vehicle object in its rest pose. [port design]
//! - No sniping, no mouse turn, no recentring, no slam finisher camera, no camera shakes on the
//!   view. No sound unless one is given.
//! - The scouts are the arena's scouts (`combat::Scout`, its own brain), on the flat ground.

use glam::{Affine3A, Quat, Vec2, Vec3};

use crate::aim::{self, Aim};
use crate::arsenal::{Arsenal, Barrel, Car, TurretOwner, WeaponSound};
use crate::camera::{self, Camera, Sight, View};
use crate::character::CharacterData;
use crate::combat::{Enemy, Scout};
use crate::control::button;
use crate::damage::Packet;
use crate::gun::Clips;
use crate::pose::Rig;
use crate::sim::{self, Arena, Input, State};
use crate::tuning::Tuning;

/// The weapon layer's clips. [data]
pub const WEAPON_LAYER_SET: &str = "WeaponPartialSet";

/// The pad for one update, as the player holds it. The stick is the left stick, taken
/// relative to the camera on foot (up is away from the camera) and as the steering in the car.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pad {
    pub stick: Vec2,
    /// The right stick: turns the camera, and the aim in weapon mode.
    pub look: Vec2,
    /// The right trigger: the change of form and the car's throttle, or the fire in weapon mode.
    pub vehicle: f32,
    /// The left trigger held: weapon mode on foot, the slide in the car.
    pub aim: bool,
    /// The brake button.
    pub brake: f32,
    /// Jump: pressed this update, held.
    pub jump: bool,
    pub jump_held: bool,
    /// The left face button (fast attack, dodge, the car's gun): pressed, held.
    pub melee: bool,
    pub melee_held: bool,
    /// The right face button (action, climb): pressed, held.
    pub climb: bool,
    pub climb_held: bool,
    /// The top face button (the special): pressed, held.
    pub special: bool,
    pub special_held: bool,
    /// The weapon switch (D-pad), held.
    pub switch: bool,
    /// Other buttons by their number (`control::button`), held and pressed.
    pub buttons_down: u64,
    pub buttons_hit: u64,
}

/// What one update did beyond his own state.
#[derive(Clone, Debug)]
pub struct Update {
    /// The view the camera gives, before shakes.
    pub view: View,
    /// He was in weapon mode, and the aim's way.
    pub aim: Option<Vec3>,
    /// A round left this update: its rumble (duration, strength).
    pub fired: Option<(f32, f32)>,
    /// What his weapons hit this update: (target, packet).
    pub struck: Vec<(u32, Packet)>,
    /// What his melee hit: (target, damage).
    pub melee: Vec<(u32, f32)>,
    /// What hit him this update: (the scout that sent it, the packet, the damage taken).
    pub taken: Vec<(u32, Packet, f32)>,
    /// The muzzle his rounds leave from this update: his gun's, or in the car the car gun's first barrel.
    pub muzzle: Vec3,
}

/// One robot with his camera, aim and weapons, and the scouts that fight him.
pub struct Robot<'a> {
    pub t: &'a Tuning,
    pub data: &'a CharacterData,
    pub state: State,
    pub camera: Camera,
    pub view: View,
    pub aim: Aim,
    pub aiming: bool,
    pub arsenal: Option<Arsenal>,
    pub scout: Option<Scout>,
    pub scouts: Vec<Enemy>,
    /// The scouts stand and do not shoot.
    pub passive: bool,
    robot: Rig,
    car: Option<Rig>,
}

impl<'a> Robot<'a> {
    /// He stands at `pos` facing `yaw` (radians, 0 is +y), the camera behind him.
    pub fn new(t: &'a Tuning, data: &'a CharacterData, arsenal: Option<Arsenal>, pos: Vec3, yaw: f32) -> Self {
        let mut state = State::new(t);
        (state.pos, state.yaw) = (pos, yaw);
        let target = camera::follow_target(&state, t, None);
        let mut camera = Camera::default();
        camera.reset(&target, t);
        let view = camera.latest();
        let mut aim = Aim::default();
        aim.set(state.body_forward());
        Self {
            t,
            data,
            state,
            camera,
            view,
            aim,
            aiming: false,
            arsenal,
            scout: None,
            scouts: Vec::new(),
            passive: false,
            robot: Rig::new(&data.robot),
            car: data.vehicle.as_ref().map(Rig::new),
        }
    }

    /// Puts a scout at `pos` with the target number `id` (not one an arena target has), the scout's
    /// data loaded once (`Scout::load`) and kept for all of them.
    pub fn add_scout(&mut self, scout: impl FnOnce() -> Result<Scout, String>, pos: Vec3, id: u32) -> Result<u32, String> {
        if self.scout.is_none() {
            self.scout = Some(scout()?);
        }
        let Some(scout) = &self.scout else { unreachable!() };
        self.scouts.push(Enemy::new(id, pos, scout));
        Ok(id)
    }

    /// Where a node of his (or of his car) is in the world, with `at` in the node's own space.
    fn place(&self, robot: &[Affine3A], bone: u32, at: Vec3) -> Option<Vec3> {
        let car_hashes: Option<Vec<u32>> = self.data.vehicle.as_ref().map(|v| v.nodes.iter().map(|n| n.name_hash).collect());
        let robot_hashes: Vec<u32> = self.data.robot.nodes.iter().map(|n| n.name_hash).collect();
        match crate::weapons::mount(bone, &robot_hashes, car_hashes.as_deref())? {
            crate::weapons::Mount::Robot(node) => {
                let root = Affine3A::from_rotation_translation(Quat::from_rotation_z(self.state.yaw), self.state.pos);
                Some((root * robot[node]).transform_point3(at))
            }
            crate::weapons::Mount::Car(node) => {
                let pose = self.state.car_presentation();
                let root = Affine3A::from_rotation_translation(pose.rotation, pose.pos);
                Some((root * self.car.as_ref()?.rest()[node]).transform_point3(at))
            }
        }
    }

    /// His skeleton posed as the movement and the weapon layer have it now, in his own space.
    fn posed(&self) -> Vec<Affine3A> {
        let t = self.t;
        let base = match self.state.action_clip(t) {
            Some((set, id, tick)) => self.data.animations.find(set, id).and_then(|r| Some((r.clip.as_ref()?, r.looping, tick))),
            None => self.state.base_clip(t).and_then(|c| {
                let r = self.data.animations.find(c.set, c.id)?;
                Some((r.clip.as_ref()?, r.looping, c.tick))
            }),
        };
        let layer = self.arsenal.as_ref().filter(|_| !self.state.is_vehicle()).and_then(|arsenal| {
            let arm = arsenal.gun.arm?;
            let r = self.data.animations.sets.get(WEAPON_LAYER_SET)?.get(arm.clip)?;
            Some((r.clip.as_ref()?, r.looping, arm.tick, r.additive, arm.weight()))
        });
        match (base, layer) {
            (_, Some((over, looping, tick, additive, weight))) if weight > 0.0 => self.robot.pose_layer(base, over, looping, tick, additive, weight),
            (Some((clip, looping, tick)), _) => self.robot.pose(clip, looping, tick),
            (None, _) => self.robot.rest(),
        }
    }

    /// One update of `dt` seconds (the game's is `sim::GAME_UPDATE`).
    pub fn step(&mut self, pad: &Pad, arena: &mut Arena, sound: Option<&dyn WeaponSound>, dt: f32) -> Update {
        let t = self.t;
        let mut out = Update { view: self.view, aim: None, fired: None, struck: Vec::new(), melee: Vec::new(), taken: Vec::new(), muzzle: Vec3::ZERO };
        // The scouts are targets for his melee and his rounds. [game]
        if let Some(scout) = &self.scout {
            arena.targets.retain(|target| !self.scouts.iter().any(|e| e.state.actor.id == target.id));
            arena.targets.extend(self.scouts.iter().filter(|e| !e.state.dead()).map(|e| e.target(&scout.tuning)));
        }

        // On foot the stick is relative to the camera; in the car it steers.
        let forward = Vec2::new(self.view.forward.x, self.view.forward.y).normalize_or(Vec2::Y);
        let right = Vec2::new(forward.y, -forward.x);
        let both = |down: bool| (down as u64) << button::ATTACK_FAST | (down as u64) << button::DODGE | (down as u64) << button::ATTACK_VEHICLE;
        let climbing = |down: bool| (down as u64) << button::CLIMB;
        let special_bits = |down: bool| (down as u64) << button::ATTACK_SPECIAL;
        let vehicle = self.state.is_vehicle();
        let aiming = pad.aim && !vehicle && self.arsenal.is_some();
        let giant_armed = t.giant.as_ref().is_some_and(|g| !g.pods.is_empty()) && self.arsenal.is_some();
        let was_aiming = std::mem::replace(&mut self.aiming, aiming);
        let fire = (aiming && pad.vehicle > 0.1) as u64;
        if let Some(arsenal) = self.arsenal.as_mut() {
            arsenal.anim_rate = self.state.special_rate;
        }

        // In weapon mode the right stick turns his aim, with the aim assist. [game 0071b990]
        let looking = self.view.forward;
        if aiming {
            let look = self.camera.options.stick(pad.look);
            let right = looking.cross(Vec3::Z).normalize_or(Vec3::X);
            let eye = aim::Camera { eye: self.view.eye, right, forward: looking, up: right.cross(looking) };
            let clear = |from: Vec3, to: Vec3| arena.sight(from, to).is_none();
            if !was_aiming {
                self.aim.enter(looking, look);
            }
            if self.aim.snap_wanted && self.aim.snap.is_none() {
                let range = self.arsenal.as_ref().map_or(t.aim.default_max_range, |a| a.assist_range(&t.aim));
                self.aim.snap = aim::snap_target(&eye, range, &arena.targets, &t.aim, &clear);
            }
            match self.aim.snap.and_then(|id| arena.targets.iter().find(|target| target.id == id)) {
                Some(target) => self.aim.step_snap(aim::assist_point(target), eye.eye, self.state.pos, dt),
                None => {
                    self.aim.snap = None;
                    let scouts = &self.scouts;
                    let held = self.arsenal.as_ref().and_then(|a| {
                        a.assist_target(&arena.targets, &self.view, |id| scouts.iter().find(|e| e.state.actor.id == id).map_or(Vec3::ZERO, |e| e.state.vel))
                    });
                    let assist = aim::assist(look, &eye, self.state.vel, held.as_ref(), &t.aim, dt);
                    let moving = pad.stick.length_squared() > 0.001;
                    self.aim.step_assisted(look, moving, self.state.pos, assist, &t.aim, dt);
                }
            }
        } else {
            self.aim.leave();
            self.aim.set(self.state.body_forward());
        }
        let aim = aiming.then(|| self.aim.direction());
        out.aim = aim;
        let input = Input {
            stick: right * pad.stick.x + forward * pad.stick.y,
            steer: pad.stick.x,
            pad_stick: pad.stick,
            look: pad.look,
            jump: pad.jump,
            jump_held: pad.jump_held,
            vehicle: if aiming || giant_armed { 0.0 } else { pad.vehicle },
            brake: pad.brake,
            turbo: pad.jump && vehicle,
            drift: pad.aim,
            aim: aim.filter(|_| !giant_armed).map(|aim| Vec2::new(aim.x, aim.y))
                .or_else(|| (pad.aim && self.state.mode == sim::Mode::Fly).then(|| self.state.facing())),
            buttons_down: both(pad.melee_held || pad.melee)
                | climbing(pad.climb_held || pad.climb)
                | special_bits(pad.special_held || pad.special)
                | fire << button::WEAPON_FIRE
                | pad.buttons_down,
            buttons_hit: both(pad.melee) | climbing(pad.climb) | special_bits(pad.special) | pad.buttons_hit,
        };
        let was_unfolding = self.state.action_clip(t).is_some_and(|(set, _, _)| set == "Vehicle2RobotSet");
        sim::step(&mut self.state, &input, t, arena, dt);

        // The camera. [game]
        let target = camera::follow_target(&self.state, t, aim);
        let fadeable = Vec::new();
        let world = camera::Scene { solids: &*arena, fadeable: &fadeable };
        self.view = self.camera.step(&target, pad.look, t, &world, dt);
        out.view = self.view;

        // His weapons. [game] (`game/src/player.rs`, with the muzzles as this module's notes say)
        let posed = self.posed();
        if let Some(mut arsenal) = self.arsenal.take() {
            let state = &self.state;
            let chest = state.pos + Vec3::Z * (t.robot_height * 0.7) + state.facing().extend(0.0) * 1.5;
            let muzzle_of = |barrel: Barrel| self.place(&posed, barrel.hung, barrel.at);
            let muzzle = muzzle_of(arsenal.barrel()).unwrap_or(chest);
            let partner_muzzle = arsenal.partner_barrel().and_then(muzzle_of).unwrap_or(muzzle);
            let special_muzzle = arsenal.special_barrel().and_then(muzzle_of).unwrap_or(muzzle);
            out.muzzle = muzzle;
            arsenal.tick_axe(&state.melee);
            let armed = match state.state_class(t) {
                Some("StateWeapon") => true,
                Some("StateDash") => arsenal.gun.out,
                _ => false,
            };
            let nose = state.body_forward();
            let car_barrels: Vec<Vec3> = arsenal.car_weapon().map_or(Vec::new(), |(bone, barrels)| {
                barrels.iter().map(|&barrel| self.place(&posed, bone, barrel).unwrap_or(state.pos + nose * 3.0 + Vec3::Z)).collect()
            });
            let flipper = self.place(&posed, 0xc8ec_021e, Vec3::Y).zip(self.place(&posed, 0xc8ec_021e, Vec3::ZERO)).map(|(a, b)| a - b)
                .filter(|_| self.data.vehicle.as_ref().is_some_and(|v| v.nodes.iter().any(|n| n.name_hash == 0xc8ec_021e)));
            // Devastator's pods ride the car weapons' path [assumed port design]: the selected kind's
            // barrels on every pod, pod by pod; the gatling only from the pods on the side he shoots
            // to, the bug bombs from spots round the vent on his back (`giant`).
            let pods = t.giant.as_ref().map_or(&[][..], |g| g.pods.as_slice());
            let pod_barrels: Vec<Vec3> = pods.get(arsenal.car_current()).filter(|_| giant_armed).map_or(Vec::new(), |p| {
                let placed: Vec<Vec<Vec3>> =
                    p.mounts.iter().map(|m| m.barrels.iter().filter_map(|&barrel| self.place(&posed, m.bone, barrel)).collect()).collect();
                let open = if p.kind == crate::giant::GATLING {
                    let centres: Vec<Vec3> = placed.iter().map(|b| b.iter().copied().sum::<Vec3>() / b.len().max(1) as f32).collect();
                    crate::giant::pods_facing(&centres, self.view.forward)
                } else if p.kind == crate::giant::BUG_BOMB {
                    return self.place(&posed, crate::giant::ERUPTION_VENT, Vec3::ZERO)
                        .map(|bone| {
                            let vent = bone - nose * crate::giant::ERUPTION_VENT_BACK;
                            crate::giant::vent_spots(vent, nose, crate::giant::ERUPTION_BOMBS as usize)
                        })
                        .unwrap_or_default();
                } else {
                    vec![true; placed.len()]
                };
                let most = placed.iter().map(Vec::len).max().unwrap_or(0);
                (0..most).flat_map(|k| placed.iter().zip(&open).filter(|(_, o)| **o).filter_map(move |(b, _)| b.get(k).copied())).collect()
            });
            if let Some(mount) = pods.iter().filter(|p| p.kind == crate::giant::BUG_BOMB)
                .flat_map(|p| p.mounts.iter()).find(|m| m.bone == crate::giant::ERUPTION_VENT && !m.barrels.is_empty())
            {
                let ends: Vec<Vec3> = mount.barrels.iter().filter_map(|&b| self.place(&posed, mount.bone, b)).collect();
                if let (Some(bone), false) = (self.place(&posed, mount.bone, Vec3::ZERO), ends.is_empty()) {
                    let middle = ends.iter().copied().sum::<Vec3>() / ends.len() as f32;
                    arsenal.vent_axis = (middle - bone).normalize_or(Vec3::Z);
                }
            }
            if state.car_mode && let Some(&barrel) = car_barrels.first() {
                out.muzzle = barrel;
            }
            if let Some(&barrel) = pod_barrels.first() {
                out.muzzle = barrel;
            }
            let car = if giant_armed {
                Car { mode: true, formed: true, wait: 0.0, barrels: &pod_barrels, forward: self.view.forward, flipper: None, target_point: self.view.eye }
            } else {
                Car {
                    mode: state.car_mode,
                    formed: state.is_vehicle(),
                    wait: t.vehicle_weapon_wait,
                    barrels: &car_barrels,
                    forward: nose,
                    flipper,
                    target_point: state.pos,
                }
            };
            if giant_armed {
                arsenal.switch_car(pad.switch, sound);
            }
            arsenal.special_started(state.special_starts, state.car_mode);
            arsenal.type6_special = state.slowing_special(t);
            arsenal.charge_cancel = t.special.kind == 4 && state.state_class(t) == Some("StateAttackSpecial");
            arsenal.owner_velocity = state.vel;
            arsenal.owner_dead = state.dead();
            let weapon_origin = self.place(&posed, arsenal.barrel().hung, Vec3::ZERO).unwrap_or(muzzle);
            let clips = Clips {
                arm: self.data.animations.sets.get(WEAPON_LAYER_SET).map_or(&[], Vec::as_slice),
                entries: self.data.animations.incoming.get(WEAPON_LAYER_SET).map_or(&[], Vec::as_slice),
                object: self.data.weapon_animations.first().map_or(&[], Vec::as_slice),
            };
            out.fired = arsenal.tick(
                armed,
                was_unfolding,
                clips,
                if giant_armed { pad.vehicle > 0.1 } else { state.firing },
                pad.switch,
                muzzle,
                weapon_origin,
                partner_muzzle,
                state.pos,
                self.view.eye,
                self.view.forward,
                self.view.fov,
                &*arena,
                &arena.targets,
                &car,
                &t.aim,
                &t.damage,
                state.damage_timers.slowed > 0.0,
                sound,
                dt,
            );
            if state.hits.iter().any(|hit| hit.damage > 0.0) {
                arsenal.mark_hit();
            }
            let facing = if state.car_mode { nose } else { state.facing().extend(0.0) };
            let special_clip = state.action_clip(t).and_then(|(set, id, tick)| {
                let clip = self.data.animations.find(set, id)?.clip.as_ref()?;
                Some((clip, set, id, tick))
            });
            arsenal.tick_special_events(special_clip, state.car_mode, &self.data.weapon_animations, dt);
            arsenal.tick_special(special_muzzle, facing, &*arena, &arena.targets, sound, dt);
            let scouts: Vec<(u32, u32, Vec3, bool, f32)> = match &self.scout {
                Some(scout) => self.scouts.iter().map(|e| (e.state.actor.id, e.state.actor.team,
                    e.state.pos + Vec3::Z * (0.75 * scout.tuning.robot_height), !e.state.dead(), scout.tuning.robot_radius)).collect(),
                None => Vec::new(),
            };
            let owner = TurretOwner { alive: state.health > 0.0, team: state.actor.team };
            arsenal.tick_turrets(dt, owner, &*arena, &arena.targets, &scouts, sound);
            if let Some((duration, strength)) = out.fired {
                self.state.rumbles.push(crate::rumble::Rumble { kind: 1, duration, strength });
            }
            if let Some((kind, duration, strength)) = arsenal.charge_rumble {
                self.state.rumbles.push(crate::rumble::Rumble { kind, duration, strength });
            }
            out.struck = arsenal.struck.iter().map(|s| (s.target, s.packet)).collect();
            self.arsenal = Some(arsenal);
        }

        // The scouts: his hits land on them, then they step and shoot back. [game] (`game/src/targets.rs`)
        if let Some(scout) = &self.scout {
            let mut incoming: Vec<(u32, Packet)> = Vec::new();
            for enemy in &mut self.scouts {
                let id = enemy.state.actor.id;
                for hit in self.state.hits.iter().filter(|hit| hit.target == id) {
                    enemy.receive(Packet::melee(hit, self.state.actor), scout);
                    out.melee.push((id, hit.damage));
                }
                for (_, packet) in out.struck.iter().filter(|(target, _)| *target == id) {
                    enemy.receive(Packet { source: Some(self.state.actor), ..*packet }, scout);
                }
                let me = self.state.actor.id;
                for shot in enemy.step(scout, &self.state, t, arena, self.passive, dt) {
                    if shot.target == Some(me) {
                        incoming.push((id, shot.packet));
                    }
                }
                for hit in enemy.state.hits.iter().filter(|hit| hit.target == me) {
                    incoming.push((id, Packet::melee(hit, enemy.state.actor)));
                }
            }
            for (from, packet) in incoming {
                let before = self.state.health;
                let result = self.state.receive_damage(packet, t);
                if result.accepted {
                    out.taken.push((from, packet, before - self.state.health));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In weapon mode with the trigger held, Bumblebee's gun brings a scout 25 m ahead down, and
    /// the scout's own fire takes some of his health on the way.
    #[test]
    fn bumblebee_shoots_a_scout_down_and_is_shot_back() {
        if crate::install::missing() { return; }
        let dir = crate::install::dir();
        let data = crate::character::load(dir, "bumblebee").unwrap();
        let mut t = crate::tuning::load(dir, "Bumblebee").unwrap();
        t.locomotion = crate::tuning::Locomotion::from_animations(&data.animations, &mut t.missing);
        t.actions = crate::tuning::Actions::from_animations(&data.animations, &data.robot, &mut t.missing)
            .with_vehicle(data.vehicle.as_ref())
            .with_melee(&data.melee_weapons);
        t.weapon_set = crate::tuning::RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
        t.rule_sets = crate::tuning::RuleSet::all(&data.animations);
        let arsenal = crate::arsenal::arm(dir, "Bumblebee", &data, None);
        let mut robot = Robot::new(&t, &data, arsenal, Vec3::ZERO, 0.0);
        robot.add_scout(|| Scout::load(dir), Vec3::new(0.0, 25.0, 0.0), 1).unwrap();
        let mut arena = Arena::default();
        let (mut fired, mut struck) = (0, 0);
        for i in 0..250 {
            let time = i as f32 * sim::GAME_UPDATE;
            let pad = Pad { aim: time > 0.5, vehicle: if time > 1.5 { 1.0 } else { 0.0 }, ..Default::default() };
            let out = robot.step(&pad, &mut arena, None, sim::GAME_UPDATE);
            fired += out.fired.is_some() as u32;
            struck += out.struck.len();
        }
        println!("{fired} rounds, {struck} hits, health {}", robot.state.health);
        assert!(fired > 10 && struck > 5, "{fired} rounds, {struck} hits");
        assert!(robot.scouts[0].state.dead());
        assert!(robot.state.health < State::new(&t).health);
    }
}
