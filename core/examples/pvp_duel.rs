//! Robot against robot, without a window: every pair of the roster fights a gun duel and a melee
//! duel to the death, each on its own attributes, health, regeneration and weapons, with the
//! weapons' MULTIPLAYER attributes where the data has them (`WeaponDef::attribute_for(true, true)`).
//! Prints a win/loss table and the seconds to kill.
//!
//! Each round goes the port's gun path: each fighter has its own weapon camera
//! (`camera::Camera`), the round is aimed at what the middle of that picture is on
//! (`weapons::aim_point`), scattered in the picture by `playerNoiseRadius` (`scattered_point`),
//! traced from the muzzle (`bullet_line`, the muzzle on the weapon's node of the posed robot)
//! against the other's posed hit bodies (`part_bodies` / `part_struck`: head, upper and lower
//! body, each with its positional multiplier) and the floor. Missiles fly (`Missile`) and grenades
//! arc and bounce (`Grenade`) against the other's cylinder and the floor, as the game's projectile
//! loop has them, and go off in their explosion (`MissileBlast`, its fall-off). A snipe weapon
//! (`snipeCameraEnabled`) is fired from the sniper camera with no scatter, once its steady is full
//! (`snipe::steady`, `damage_multiplier`); `--no-snipe` turns that off.
//!
//! What is fitted here, not the game's [assumed]: the bots. A gun bot keeps the middle of its
//! picture on the other's middle (half his height) with no error of its own, and stands, or with
//! `--strafe` side-steps, turning every 1.5 s; a semi-automatic weapon is let go one update after
//! each firing; a sniper waits for a full steady. A melee bot walks up and taps the fast attack.
//! No special, no change of form, no flight, no lock-on (missiles fly unguided at the point), no
//! cover, the flat test arena, neither counts as `large`. A flame does its damage per second inside
//! 10 m. The other's hit bodies are posed by the clip he plays at the start of the update.
//! A duel that runs 90 s is a draw.
//!
//! `cargo run --release -p tf2-core --example pvp_duel -- [gun|melee|both] [--at <metres>] [--strafe] [--no-snipe] [--head] [pack:Name...]`

use std::path::Path;

use glam::{Affine3A, Quat, Vec2, Vec3};
use tf2_core::camera::{self, Camera, Sight, Sniper};
use tf2_core::character::CharacterData;
use tf2_core::control::button;
use tf2_core::damage::{Difficulty, Packet};
use tf2_core::melee::Target;
use tf2_core::pose::Rig;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::snipe;
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet, Tuning};
use tf2_core::weapons::{self, Fired, Grenade, Kind, Missile, MissileBlast, Part, PartBody, Weapon, WeaponDef, WeaponTuning};

const ROSTER: [&str; 13] = [
    "bumblebee:Bumblebee", "optimus:Optimus", "ironhide:Ironhide", "ratchet:Ratchet", "megatron:Megatron",
    "longhaul:LongHaul", "audir8:Audi", "jetfire:Jetfire", "firestorm:Firestorm", "starscream:Starscream",
    "blackout:Blackout", "optimusflight:OptimusFlight", "megatronflight:MegatronFlight",
];
const DT: f32 = sim::GAME_UPDATE;
const LIMIT: f32 = 90.0;
/// [assumed] The flame's reach for this test; the port's flame cone is not used here.
const FLAME_REACH: f32 = 10.0;
/// [assumed] bot: seconds between the strafing bot's changes of side.
const STRAFE_TURN: f32 = 1.5;
/// [assumed] bot: the range from which a robot with a snipe weapon snipes.
const SNIPE_FROM: f32 = 40.0;
/// Updates at the start of a gun duel spent in weapon mode without firing (0.5 s), so the
/// first round does not leave while the camera is still turning onto him. [assumed] bot
const SETTLE: u32 = 16;

struct Gun {
    kind: Kind,
    def: WeaponDef,
    w: WeaponTuning,
    /// The node the weapon hangs on, in the robot object's node list.
    node: Option<usize>,
}

struct Robot {
    name: String,
    t: Tuning,
    data: CharacterData,
    rig: Rig,
    gun: Option<Gun>,
    /// Some weapon of his names a multiplayer attribute (else the campaign's numbers are used).
    mp: bool,
}

impl Robot {
    fn snipes(&self) -> bool {
        self.gun.as_ref().is_some_and(|g| g.kind == Kind::Gun && g.w.snipe.camera_enabled) && self.t.sniper_camera.is_some()
    }
}

fn load(dir: &Path, arg: &str) -> Robot {
    let (pack, name) = arg.split_once(':').expect("pack:Name");
    let mut t = tuning::load(dir, name).expect("tuning");
    let data = tf2_core::character::load(dir, pack).expect("character");
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing)
        .with_vehicle(data.vehicle.as_ref())
        .with_melee(&data.melee_weapons);
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    let tunings = weapons::load_for(dir, &data.weapons, true, true).unwrap_or_default();
    // The robot form's first weapon in hand: not the special's, not the car's, not a child. [assumed]
    let gun = data.weapons.iter().zip(tunings).enumerate().find(|(_, (def, w))| {
        def.kind != Kind::Turret && !w.is_special && !w.vehicle_mode && w.parent == 0
    }).map(|(index, (def, w))| {
        let hung = hang_node(&data, def.name, index).unwrap_or(def.bone);
        Gun { kind: def.kind, node: data.robot.nodes.iter().position(|n| n.name_hash == hung), def: def.clone(), w }
    });
    let mp = data.weapons.iter().any(|d| d.mp_attribute != 0);
    let rig = Rig::new(&data.robot);
    Robot { name: name.to_owned(), t, data, rig, gun, mp }
}

/// The node a hand weapon hangs from while it is out: the node of the first `WeaponAttachmentEvent`
/// that names it (by name, or by its place in the list), `Trans_R2W`'s first, as `game\src\weapons.rs`
/// `hang_node` finds it (`startBoneAttachment` is only where it is made). [game 007b4f20] [data]
/// [assumed] every clip of the pack is looked in, not only the gun's arm clips.
fn hang_node(data: &CharacterData, weapon: u32, index: usize) -> Option<u32> {
    use tf2_core::formats::anim::WeaponEventKind;
    let mut clips: Vec<_> = data.animations.sets.values().flatten().filter_map(|r| Some((r.id == "Trans_R2W", r.clip.as_ref()?))).collect();
    clips.sort_by_key(|(first, _)| !first);
    clips.iter().flat_map(|(_, clip)| &clip.weapon_events).find_map(|e| match e.kind {
        WeaponEventKind::Attach { weapon: w, index: i, .. } if (w != 0 && w == weapon) || (w == 0 && i == index as i32) => Some(e.node),
        _ => None,
    })
}

/// A small fixed random source per fighter, so a run is the same each time.
struct Rand(u32);
impl Rand {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

#[derive(Default)]
struct Tally {
    /// Projectiles that left the weapon, and those that struck him (directly or by a blast).
    shots: u32,
    struck: u32,
    heads: u32,
    dealt: f32,
}

struct Fighter<'a> {
    r: &'a Robot,
    s: State,
    weapon: Weapon,
    camera: Camera,
    view: Option<camera::View>,
    /// His nodes in the world and his hit bodies, from the clip he plays.
    nodes: Vec<Affine3A>,
    parts: Vec<PartBody>,
    missiles: Vec<Missile>,
    grenades: Vec<Grenade>,
    blasts: Vec<MissileBlast>,
    rand: Rand,
    sniping: bool,
    tally: Tally,
}

impl<'a> Fighter<'a> {
    fn new(r: &'a Robot, id: u32, pos: Vec3, yaw: f32) -> Self {
        let mut s = State::new(&r.t);
        s.pos = pos;
        s.yaw = yaw;
        s.actor.id = id;
        s.actor.team = id;
        // Player against player: no difficulty scale on what either receives.
        s.difficulty = Difficulty::default();
        let mut f = Fighter {
            r, s, weapon: Weapon::default(), camera: Camera::default(), view: None, nodes: Vec::new(), parts: Vec::new(),
            missiles: Vec::new(), grenades: Vec::new(), blasts: Vec::new(), rand: Rand(0x9e37_79b9 ^ id * 0x85eb_ca6b),
            sniping: false, tally: Tally::default(),
        };
        f.pose();
        f
    }
    fn target(&self) -> Target {
        Target {
            id: self.s.actor.id, pos: self.s.pos, radius: self.r.t.robot_radius, height: self.r.t.robot_height,
            character: true, large: false, surface: tf2_core::melee::surface::UPPER_BODY, dashing: false,
            health: self.s.health, head: None,
        }
    }
    /// Poses him by the clip he plays (as `game\src\animation.rs` picks it), in the world.
    fn pose(&mut self) {
        let (s, t, data) = (&self.s, &self.r.t, &self.r.data);
        let playing = s.action_clip(t).or_else(|| s.base_clip(t).map(|c| (c.set, c.id, c.tick)));
        let local = playing
            .and_then(|(set, id, tick)| data.animations.find(set, id).and_then(|r| r.clip.as_ref().map(|c| self.r.rig.pose(c, r.looping, tick))))
            .unwrap_or_else(|| self.r.rig.rest());
        let place = Affine3A::from_rotation_translation(Quat::from_rotation_z(s.yaw), s.pos);
        self.nodes = local.iter().map(|m| place * *m).collect();
        self.parts = weapons::part_bodies(&data.robot, &local)
            .into_iter()
            .map(|p| PartBody { part: p.part, body: p.body.moved(&place) })
            .collect();
    }
    /// Where the bot puts the middle of its picture on him: 0.75 of his height, the point the
    /// game's aim assist and lock-on take when there is no Head node (00709870 l.63-74) [game]; with
    /// `head`, his posed `Head` node (07c159a2), where they take it when there is one. [assumed] bot:
    /// a player keeps the reticle on the chest, with no error of his own. (Half his height fell
    /// between Optimus's torso and leg boxes.)
    fn middle(&self, head: bool) -> Vec3 {
        let node = head.then(|| self.r.data.robot.nodes.iter().position(|n| n.name_hash == 0x07c1_59a2)).flatten();
        node.and_then(|n| self.nodes.get(n)).map_or(self.s.pos + Vec3::Z * (self.r.t.robot_height * 0.75), |m| m.translation.into())
    }
}

/// How far along the line (0 to 1) it enters an upright cylinder; None when it misses. The game's
/// projectile loop tests a target so (`game\src\weapons.rs` `enters`).
fn enters(from: Vec3, to: Vec3, target: &Target) -> Option<f32> {
    let (o, d) = ((from - target.pos).truncate(), (to - from).truncate());
    let (a, b, c) = (d.length_squared(), o.dot(d), o.length_squared() - target.radius * target.radius);
    let k = if c <= 0.0 {
        0.0
    } else {
        if a <= 1e-12 || b * b - a * c < 0.0 {
            return None;
        }
        (-b - (b * b - a * c).sqrt()) / a
    };
    let z = from.z + (to.z - from.z) * k - target.pos.z;
    ((0.0..=1.0).contains(&k) && (0.0..=target.height).contains(&z)).then_some(k)
}

#[derive(Clone, Copy, PartialEq)]
enum Duel {
    Gun,
    Melee,
}

#[derive(Clone, Copy)]
struct Options {
    at: f32,
    strafe: bool,
    snipe: bool,
    head: bool,
}

struct Report {
    winner: Option<usize>,
    time: f32,
    health: [f32; 2],
    tally: [Tally; 2],
}

/// One gun update of fighter `f` against `o`: the weapon, its rounds, its projectiles and blasts.
/// Returns the packets for `o`.
fn shoot(f: &mut Fighter, o: &Fighter, arena: &Arena) -> Vec<Packet> {
    let mut packets = Vec::new();
    let Some(g) = &f.r.gun else { return packets };
    let w = &g.w;
    let target = o.target();
    let view = f.view.expect("the camera ran");
    let sniping = f.sniping;
    f.weapon.set_sniping(snipe::clock_runs(sniping, true, w.parent != 0, w.is_special));
    let fired = f.weapon.step_status(w, f.s.firing, false, &f.r.t.damage, DT);
    let rounds = match fired { Fired::Rounds(n) | Fired::RoundsAndOverheated(n) => n, _ => 0 };
    // The muzzle: the weapon's barrel on the node it hangs on (identity hang). [game 007a7a40]
    // While sniping, the camera lowered by 1.5. [game]
    let chest = f.s.pos + Vec3::Z * (f.r.t.robot_height * 0.75);
    let muzzle = if sniping {
        view.eye - Vec3::Z * snipe::BARREL_DROP
    } else {
        g.node.and_then(|n| f.nodes.get(n)).map_or(chest, |m| m.transform_point3(g.def.muzzle))
    };
    let direction = |to: Vec3| (to - muzzle).normalize_or(view.forward);
    if g.kind == Kind::Flame {
        if f.s.firing && (target.pos - f.s.pos).truncate().length() <= FLAME_REACH {
            let mut p = Packet::bullet(w, w.flame_damage_per_second * DT, direction(o.middle(false)));
            p.source = Some(f.s.actor);
            packets.push(p);
        }
        return packets;
    }
    // What the middle of the picture is on: the floor (the arena), or his bodies. [game 007b2310]
    let far = view.eye + view.forward * weapons::AIM_REACH;
    let aim = weapons::aim_point(view.eye, view.forward, f.s.pos, arena);
    let from = view.eye + view.forward * (f.s.pos - view.eye).dot(view.forward).clamp(0.0, weapons::AIM_REACH);
    let aim_at = match weapons::part_struck(from, far, &o.parts) {
        Some(hit) if hit.first * (far - from).length() < aim.distance => from + (far - from) * hit.first,
        _ => aim.point,
    };
    let right = view.forward.cross(Vec3::Z).normalize_or(Vec3::X);
    let up = right.cross(view.forward);
    let noise = if sniping { 0.0 } else { w.player_noise_radius };
    let multiplier = if sniping { snipe::damage_multiplier(f.weapon.snipe_steady_shot, &w.snipe, w.damage) } else { 1.0 };
    for _ in 0..rounds * w.projectiles_per_fire.max(1) {
        if std::env::var("PVP_DBG").is_ok_and(|v| v == "2") {
            eprintln!("{} fires {} at {:.2}s", f.r.name, g.kind as u32, f.weapon.heat);
        }
        let (a, b) = (f.rand.next(), f.rand.next());
        let at = weapons::scattered_point(aim_at, view.eye, right, up, view.fov, noise, a, b);
        f.tally.shots += 1;
        if std::env::var("PVP_DBG").is_ok() && f.tally.shots <= 3 {
            eprintln!(
                "{}: pos {:.2?} eye {:.2?} fwd {:.2?} fov {:.1} muzzle {:.2?} aim_at {:.2?} at {:.2?} his parts {} middle {:.2?}",
                f.r.name, f.s.pos, view.eye, view.forward, view.fov, muzzle, aim_at, at, o.parts.len(), o.middle(false)
            );
            eprintln!("    hang node {:?} at {:.2?}, barrel {:.2?}", g.node, g.node.and_then(|n| f.nodes.get(n)).map(|m| m.translation), g.def.muzzle);
            for p in &o.parts {
                eprintln!("    {:?} {:?} at {:.2?}", p.part, p.body.shape, p.body.at.translation);
            }
        }
        match g.kind {
            Kind::Gun => {
                let (start, end) = weapons::bullet_line(muzzle, at, w);
                let floor = arena.sight(start, end).map(|h| (h.point - start).length());
                let struck = match weapons::part_struck(start, end, &o.parts) {
                    Some(hit) => Some((hit.at, hit.part)),
                    // [stand-in] a robot with no Character bodies is struck on his cylinder as an upper body.
                    None if o.parts.is_empty() => enters(start, end, &target).map(|k| (k, Part::UpperBody)),
                    None => None,
                };
                let Some((k, part)) = struck else { continue };
                let travelled = k * (end - start).length();
                if floor.is_some_and(|d| d < travelled) {
                    continue;
                }
                let point = start + (end - start) * k;
                let mut p = Packet::bullet(w, weapons::bullet_damage(w, multiplier, part, travelled, true), direction(at));
                p.headshot = part == Part::Head;
                p.source = Some(f.s.actor);
                packets.push(p);
                f.tally.struck += 1;
                f.tally.heads += u32::from(part == Part::Head);
                // A round that carries an explosion starts it where it lands, with the explosion's own damage. [game]
                if let Some(def) = g.def.explosion_for(false) {
                    let own = WeaponTuning { explosive_damage: -1.0, explosive_min_damage: -1.0, ..Default::default() };
                    let mut blast = MissileBlast::new(def, &own, point);
                    packets.extend(blast_packets(blast.first(std::slice::from_ref(&target)), f));
                    f.blasts.push(blast);
                }
            }
            Kind::Launcher => {
                let rand = &mut f.rand;
                f.missiles.push(Missile::launch(muzzle, view.forward, at, None, w, &mut || rand.next()));
            }
            Kind::Grenade => {
                let rand = &mut f.rand;
                f.grenades.push(Grenade::launch(muzzle, view.forward, at, f.s.vel, w, &mut || rand.next()));
            }
            Kind::Flame | Kind::Turret => {}
        }
    }
    // The projectiles fly, and what ends goes off in its explosion. [game: the projectile loop]
    let mut ended: Vec<(Vec3, bool)> = Vec::new();
    let rand = &mut f.rand;
    f.missiles.retain_mut(|m| {
        let Some((from, to)) = m.step(w, None, &mut || rand.next(), DT) else {
            ended.push((m.position, false));
            return false;
        };
        let floor = arena.sight(from, to).map(|h| (h.point - from).length() / (to - from).length().max(1e-6));
        let body = enters(from, to, &target);
        match (floor, body) {
            (_, Some(k)) if floor.is_none_or(|j| k <= j) => ended.push((from + (to - from) * k, true)),
            (Some(j), _) => ended.push((from + (to - from) * j, false)),
            _ => return true,
        }
        false
    });
    f.grenades.retain_mut(|gr| {
        let touch = |from: Vec3, to: Vec3| {
            let mut found = arena.sight(from, to).map(|h| (h.point, h.normal, false));
            if let Some(k) = enters(from, to, &target) {
                let point = from + (to - from) * k;
                if found.is_none_or(|(n, _, _)| (point - from).length_squared() < (n - from).length_squared()) {
                    found = Some((point, (from - to).normalize_or(Vec3::Z), true));
                }
            }
            found
        };
        let nearest = |at: Vec3| Some(at.distance(target.pos + Vec3::Z * (target.height * 0.5)));
        match gr.step(w, &touch, &nearest, DT) {
            Ok(_) => true,
            Err(at) => {
                ended.push((at, false));
                false
            }
        }
    });
    for (at, direct) in ended {
        match g.def.explosion_for(false) {
            Some(def) => {
                let mut blast = MissileBlast::new(def, w, at);
                packets.extend(blast_packets(blast.first(std::slice::from_ref(&target)), f));
                f.blasts.push(blast);
            }
            // [assumed] a projectile with no explosion does its damage where it strikes him.
            None if direct => {
                let mut p = Packet::bullet(w, w.damage, direction(at));
                p.source = Some(f.s.actor);
                packets.push(p);
                f.tally.struck += 1;
            }
            None => {}
        }
    }
    let mut blasts = std::mem::take(&mut f.blasts);
    blasts.retain_mut(|blast| {
        let (hits, alive) = blast.step(std::slice::from_ref(&target), DT);
        packets.extend(blast_packets(hits, f));
        alive
    });
    f.blasts = blasts;
    packets
}

fn blast_packets(hits: Vec<weapons::BlastHit>, f: &mut Fighter) -> Vec<Packet> {
    hits.into_iter()
        .map(|hit| {
            f.tally.struck += 1;
            Packet {
                source: Some(f.s.actor), amount: hit.damage, flags: hit.flags, direction: hit.direction.extend(0.0),
                knockback_speed: hit.knock_back_speed, knockback_angle: hit.knock_back_angle, ..Default::default()
            }
        })
        .collect()
}

/// One duel; the winner's index (None: draw), the seconds it took, and each side's tally.
fn duel(a: &Robot, b: &Robot, kind: Duel, o: Options) -> Report {
    let start = if kind == Duel::Melee { 6.0 } else { o.at };
    let mut f = [
        Fighter::new(a, 1, Vec3::ZERO, 0.0),
        Fighter::new(b, 2, Vec3::new(0.0, start, 0.0), std::f32::consts::PI),
    ];
    // A gun duel starts with both in weapon mode for `SETTLE` updates without firing, so the
    // cameras have turned onto each other; the clock counts from the end of it.
    let settle = if kind == Duel::Gun { SETTLE } else { 0 };
    let mut time = -(settle as f32) * DT;
    let mut step = 0u32;
    while time < LIMIT {
        let mut pending: [Vec<Packet>; 2] = [Vec::new(), Vec::new()];
        for i in 0..2 {
            let j = 1 - i;
            if f[i].s.dead() || f[i].s.health <= 0.0 {
                continue;
            }
            let to = (f[j].s.pos - f[i].s.pos).truncate();
            let distance = to.length();
            let dir = to.normalize_or(Vec2::Y);
            let arena = Arena { targets: vec![f[j].target()], ..Default::default() };
            let input = match kind {
                Duel::Gun => {
                    let side = if (time / STRAFE_TURN) as u32 % 2 == 0 { 1.0 } else { -1.0 };
                    // A sniper holds fire until the steady is full. [assumed] bot
                    let steady = !f[i].sniping || f[i].r.gun.as_ref().is_some_and(|g| f[i].weapon.snipe_steady(&g.w) >= 1.0);
                    // A weapon that is not automatic fires once a pull, a `holdToLockOn` one (Long Haul's
                    // grenades) on the let-go and a charged one on a tap's let-go: the bot taps, down
                    // one update and up the next. An automatic one is held. [assumed] bot
                    let taps = f[i].r.gun.as_ref().is_some_and(|g| !g.w.automatic_firing || g.w.hold_to_lock_on || g.w.charge.has);
                    let fire = step >= SETTLE && steady && (!taps || step % 2 == 0);
                    Input {
                        aim: Some(dir),
                        stick: if o.strafe { Vec2::new(dir.y, -dir.x) * side } else { Vec2::ZERO },
                        buttons_down: 1 << button::WEAPON_MODE | if fire { 1 << button::WEAPON_FIRE } else { 0 },
                        ..Default::default()
                    }
                }
                Duel::Melee => {
                    let reach = f[i].r.t.robot_radius + f[j].r.t.robot_radius + 1.0;
                    // Tap the fast attack every 8 updates (0.26 s) once in reach. [assumed] bot
                    let tap = distance <= reach && (step + i as u32 * 4) % 8 == 0;
                    let bit = 1u64 << button::ATTACK_FAST;
                    Input {
                        stick: if distance > reach { dir } else { Vec2::ZERO },
                        buttons_down: if tap { bit } else { 0 },
                        buttons_hit: if tap { bit } else { 0 },
                        ..Default::default()
                    }
                }
            };
            sim::step(&mut f[i].s, &input, &f[i].r.t, &arena, DT);
            if std::env::var("PVP_DBG").is_ok_and(|v| v == "2") && step % 8 == 0 {
                eprintln!("{:5.2} {} state {:?} firing {} hp {:.0}", time, f[i].r.name, f[i].s.control_state(&f[i].r.t), f[i].s.firing, f[i].s.health);
            }
            f[i].pose();
            let packets = match kind {
                Duel::Melee => f[i].s.hits.iter().filter(|h| h.target == f[j].s.actor.id).map(|hit| Packet::melee(hit, f[i].s.actor)).collect(),
                Duel::Gun => {
                    // The camera, aimed so its middle is on him: the way from where it looks from now. [assumed] bot
                    let middle = f[j].middle(o.head);
                    let eye = f[i].view.map_or(f[i].s.pos + Vec3::Z * f[i].r.t.robot_height, |v| v.eye);
                    // A sniper snipes from `SNIPE_FROM` out. [assumed] bot
                    let snipes = o.snipe && f[i].r.snipes() && distance >= SNIPE_FROM;
                    if f[i].view.is_none() {
                        // The camera starts behind him, as a player's does when the match begins. [assumed]
                        let (s, t) = (&f[i].s, &f[i].r.t);
                        let start = camera::Target {
                            position: s.pos, height: t.robot_height, facing: s.facing(), radius: t.robot_radius, ..Default::default()
                        };
                        f[i].camera = Camera::behind(&start, t);
                    }
                    if !snipes {
                        f[i].camera.sniper = None;
                    }
                    if snipes && f[i].camera.sniper.is_none() {
                        let (settings, zoom) = f[i].r.t.sniper_camera.clone().expect("sniper camera");
                        f[i].camera.sniper = Some(Sniper::new(settings, zoom));
                    }
                    f[i].sniping = snipes;
                    let (s, t) = (&f[i].s, &f[i].r.t);
                    let target = camera::Target {
                        position: s.pos, height: t.robot_height + t.weapon_look_height, facing: s.facing(), velocity: s.vel,
                        motion: s.vel, aim: Some((middle - eye).normalize_or(Vec3::Y)), radius: t.robot_radius,
                        ..Default::default()
                    };
                    let view = f[i].camera.step(&target, Vec2::ZERO, &f[i].r.t, &arena, DT);
                    f[i].view = Some(view);
                    let (left, right) = f.split_at_mut(1);
                    let (me, other) = if i == 0 { (&mut left[0], &right[0]) } else { (&mut right[0], &left[0]) };
                    shoot(me, other, &arena)
                }
            };
            pending[j] = packets;
        }
        // What each did lands after both have updated, so neither is staggered first by being
        // second in the order. [assumed] (the game updates characters one after the other)
        for j in 0..2 {
            for p in std::mem::take(&mut pending[j]) {
                let before = f[j].s.health;
                f[j].s.receive_damage(p, &f[j].r.t);
                f[1 - j].tally.dealt += (before - f[j].s.health).max(0.0);
            }
            f[j].pose();
        }
        time += DT;
        step += 1;
        let down = [f[0].s.health <= 0.0, f[1].s.health <= 0.0];
        if down[0] || down[1] {
            let winner = match down {
                [true, false] => Some(1),
                [false, true] => Some(0),
                _ => None,
            };
            return report(winner, time, f);
        }
    }
    report(None, time, f)
}

fn report(winner: Option<usize>, time: f32, f: [Fighter; 2]) -> Report {
    let health = [f[0].s.health, f[1].s.health];
    let [a, b] = f;
    Report { winner, time, health, tally: [a.tally, b.tally] }
}

fn main() {
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut o = Options { at: 20.0, strafe: false, snipe: true, head: false };
    if let Some(i) = args.iter().position(|a| a == "--at") {
        o.at = args[i + 1].parse().expect("metres");
        args.drain(i..i + 2);
    }
    for (flag, set) in [("--strafe", &mut o.strafe as &mut bool), ("--head", &mut o.head)] {
        if let Some(i) = args.iter().position(|a| a == flag) {
            *set = true;
            args.remove(i);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--no-snipe") {
        o.snipe = false;
        args.remove(i);
    }
    let mode = match args.first().map(String::as_str) {
        Some("gun" | "melee" | "both") => args.remove(0),
        _ => "both".to_owned(),
    };
    if args.is_empty() {
        args = ROSTER.map(str::to_owned).to_vec();
    }
    let robots: Vec<Robot> = args.iter().map(|a| load(Path::new(&dir), a)).collect();
    println!("== the fighters");
    for r in &robots {
        let gun = r.gun.as_ref().map_or("no gun".to_owned(), |g| {
            let w = &g.w;
            let blast = g.def.explosion_for(false).map_or(String::new(), |e| format!(", blast {} r {}", e.damage, e.radius));
            format!(
                "{:?}{}{}{}: {} dmg{blast} x{} every {:.2}s, range {}, scatter {}, heat {}/{} per round, overheat {}s",
                g.kind,
                if r.mp { " (MP)" } else { " (campaign)" },
                if w.automatic_firing { "" } else { " semi-auto" },
                if r.snipes() { format!(" snipe to {}", w.snipe.max_damage) } else { String::new() },
                w.damage, w.projectiles_per_fire, w.rate_of_fire, w.bullet_range, w.player_noise_radius,
                w.heat_per_round, w.heat_capacity, w.overheat_period
            )
        });
        let parts = weapons::part_bodies(&r.data.robot, &r.rig.rest());
        let count = |p: Part| parts.iter().filter(|b| b.part == p).count();
        println!(
            "  {:15} health {:6.0}, regen delay {:.1}s, radius {:.2}, height {:.2}, hit bodies {}H {}U {}L | {gun}",
            r.name, r.t.base_health, r.t.health_regen_delay, r.t.robot_radius, r.t.robot_height,
            count(Part::Head), count(Part::UpperBody), count(Part::LowerBody)
        );
    }
    let kinds: Vec<(Duel, &str)> = match mode.as_str() {
        "gun" => vec![(Duel::Gun, "gun")],
        "melee" => vec![(Duel::Melee, "melee")],
        _ => vec![(Duel::Gun, "gun"), (Duel::Melee, "melee")],
    };
    for (kind, label) in kinds {
        let range = if kind == Duel::Gun {
            format!(" at {} m{}{}{}", o.at, if o.strafe { ", strafing" } else { ", standing" }, if o.snipe { "" } else { ", no sniping" }, if o.head { ", aimed at the head" } else { "" })
        } else {
            String::new()
        };
        println!("\n== {label} duels{range}: row against column; W win (seconds), L loss, D draw");
        print!("{:15}", "");
        for r in &robots {
            print!(" {:>9.9}", r.name);
        }
        println!("   wins  struck");
        let mut wins = vec![0usize; robots.len()];
        for (i, a) in robots.iter().enumerate() {
            print!("{:15}", a.name);
            let (mut shots, mut struck) = (0u32, 0u32);
            for (j, b) in robots.iter().enumerate() {
                if i == j {
                    print!(" {:>9}", "-");
                    continue;
                }
                // Each pair fights twice with the sides swapped; the cell is from row's side.
                let (r1, r2) = (duel(a, b, kind, o), duel(b, a, kind, o));
                for (r, side) in [(&r1, 0), (&r2, 1)] {
                    shots += r.tally[side].shots;
                    struck += r.tally[side].struck;
                }
                if robots.len() == 2 && i == 0 {
                    for (r, [x, y]) in [(&r1, [a, b]), (&r2, [b, a])] {
                        let [p, q] = &r.tally;
                        eprintln!(
                            "  {} vs {}: {:.1}s; health left {:.0} / {:.0}; dealt {:.0} / {:.0}; struck {}/{} / {}/{} (heads {} / {})",
                            x.name, y.name, r.time, r.health[0], r.health[1], p.dealt, q.dealt,
                            p.struck, p.shots, q.struck, q.shots, p.heads, q.heads
                        );
                    }
                }
                let row_wins = (r1.winner == Some(0)) as i32 + (r2.winner == Some(1)) as i32;
                let row_losses = (r1.winner == Some(1)) as i32 + (r2.winner == Some(0)) as i32;
                let t = (r1.time + r2.time) * 0.5;
                let cell = match (row_wins, row_losses) {
                    (2, _) => format!("W {t:.1}"),
                    (_, 2) => format!("L {t:.1}"),
                    (0, 0) => "D".to_owned(),
                    _ => "split".to_owned(),
                };
                if row_wins == 2 {
                    wins[i] += 1;
                }
                print!(" {cell:>9}");
            }
            let share = if kind == Duel::Gun && shots > 0 { format!("{:5.0}%", 100.0 * struck as f32 / shots as f32) } else { String::new() };
            println!("   {:4}  {share}", wins[i]);
        }
    }
}
