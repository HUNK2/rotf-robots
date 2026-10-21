//! A character's guns and launchers: which he has, their numbers, and the rules for firing,
//! heat and what is fired. No engine types; space is the game's (x right, y forward, z up).
//!
//! The numbers are the game's own, read from the install: the character's pack names each
//! weapon and its sounds, and `bnxglobal.str` holds the attribute block each weapon points
//! at (`handoff\result-01.md` lists them). HOW the numbers are used is taken from their
//! names, except the heat meter, which is read from the weapon's code. Each rule says so:
//!
//!   [game]     read from the game's code
//!
//!   [data]     a value from the game's data, used as its name says
//!   [assumed]  our reading of what a value is for
//!   [stand-in] invented so the weapon can be used; to be replaced

use std::path::Path;

use glam::Vec3;

use crate::formats::lxb::{DataFile, Node};
use crate::particles::Template;
use crate::formats::pack::{self, Pack};
use crate::formats::scene::ExplosionDef;
use crate::melee;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Fires bullets (for Bumblebee, the solar plasma cannon).
    Gun,
    /// Fires missiles.
    Launcher,
    /// Fires grenades (`GrenadeLauncher`: Long Haul's). [game] the same class as the
    /// missile launcher (maker 01186060), with its own projectile.
    Grenade,
    /// A flame (`FlameThrower`, class 00b5dae8: Long Haul's). `Flame`.
    Flame,
    /// A deployable turret's launcher (`TurretLauncher`: Blackout's and Ironhide's special).
    /// It throws the turret (`turret::LauncherTuning::launch`) instead of firing rounds. [game 00784170
    /// kind 4, 0078c2e0]
    Turret,
}

/// One of a flamethrower's damage volumes: a `FlameThrowerDummy` of its object and the
/// `Collision` sphere on the same node. [data] (`notes\weapon-flamethrower.md` B, C)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlameSphere {
    /// The node's rest place in the weapon object's own space.
    pub at: Vec3,
    pub radius: f32,
    /// `delayTilTurningOnNextDummy`: how long after this one the next comes on.
    pub delay_to_next: f32,
    /// The node's number in the weapon object (its `FlameThrowerDummy` attachment's `index`):
    /// where its looping effect is put. [data]
    pub node: usize,
}

/// The effects a charged shot and a flamethrower show, by the weapon's own data (the templates
/// are inline in the weapon's `thing`: `chargeId`, `chargeMaxReadyId`, `streamEffect`, and each
/// `FlameThrowerDummy`'s `loopingParticles`). [data]
#[derive(Clone, Debug, Default)]
pub struct WeaponFxSpec {
    /// `chargeId`: looping on the object's `ChargeDummy` while the charge runs.
    pub charge: Option<Template>,
    /// `chargeMaxReadyId`: once, on `ChargeDummy`, when the charge is full.
    pub charge_ready: Option<Template>,
    /// `streamEffect`: the flame's stream, on the object's `FireDummy`.
    pub stream: Option<Template>,
    /// `impactEffect` (weapon `+0x3f4`): a `TriggerableParticle` put at the flame's ray hit and
    /// triggered every update that hits (`0074c200`, `FUN_00746dd0`). Its elements never end, so
    /// it is one emitter moved to the hit, not one started per update. [game; data]
    pub impact: Option<Template>,
    /// Each flame dummy's looping effect, by the dummy's node number (`FlameSphere::node`).
    pub balls: Vec<(usize, Template)>,
    /// `minChargeEmissionRate`, `maxChargeEmissionRate`: the charge emitter's rate scale runs
    /// from the first to the second as the charge goes from 0 to 1. [data]
    pub charge_emission: [f32; 2],
}

/// A weapon as the character's pack describes it. Names are ids: the CRC-32 of the name.
#[derive(Clone, Debug)]
pub struct WeaponDef {
    pub kind: Kind,
    pub name: u32,
    /// The root in `bnxglobal.str` that holds its numbers.
    pub attribute: u32,
    /// The roots a non-player owner and a multiplayer game use instead (`<kind>AIAttribute`,
    /// `<kind>MPAttribute`; 0 for none): see `attribute_for`. [data]
    pub ai_attribute: u32,
    pub mp_attribute: u32,
    /// The node of the robot it is fixed to.
    pub bone: u32,
    pub fire_sound: u32,
    pub overheat_sound: u32,
    /// Played when the trigger is pulled while it is overheated.
    pub refused_sound: u32,
    /// `fireRingOffSound` (weapon word 0xd2, `+0x348`): started beside the fire sound on every
    /// firing, kept like it (`+0x384`) only for an automatic weapon whose event is infinite. 0 for
    /// none. [game 01185ae0 loader, 007a9b40 block 2] [data]
    pub ring_off_sound: u32,
    /// `weaponIdleSound` (word 0xdb, `+0x36c`): a loop while the weapon is in hand and shown
    /// (Starscream's Gatling gun, the Audi's SlagRepeater). 0 for none. [game 007aa070] [data]
    pub idle_sound: u32,
    /// `fireOverHeatKeyOff` (byte 0xd7, `+0x35c`): the overheat sound's stop at the lock's end is
    /// a key-off (`sound_event_stop`) when set, else `Event::stop` with its fade. [game 007a9fe0]
    /// [data: an enum-typed byte, 0 or 1 in every pack (`work\flags_raw.py`); the text export
    /// prints 1 as `ANIM_TYPE_QUAT`]
    pub overheat_key_off: bool,
    /// Where rounds leave it, in the weapon object's own space: the rest place of its
    /// node `FireDummy`, `FireDummy01`... numbered `minFireDummyIndex` (the weapon's word
    /// 0xcc; `FUN_007a7a40` takes each barrel's place from these nodes). Zero when the
    /// object has no such node. [data] Only the first barrel: his weapons that fire from
    /// the robot have one.
    pub muzzle: Vec3,
    /// Every barrel the object has of those numbered `minFireDummyIndex` to
    /// `maxFireDummyIndex`, in order, and how many (`+0x324`): a firing's projectiles leave
    /// one from each in turn (`FUN_007ae010`; the car's gun has two, `FireDummy` and
    /// `FireDummy01`, though its data asks for three). [data]
    pub barrels: [Vec3; 3],
    pub barrel_count: usize,
    /// The clips of the weapon's own object that its code starts, by id, 0 for none
    /// (`idleAnimation`, `exitAnimation`, `exitOverheatAnimation`, `overheatAnimation`;
    /// weapon `+0x3ac`, `+0x3b0`, `+0x3b8`, `+0x3c0`), and the clip of his weapon layer
    /// that goes with putting it away (`exitCharacterAnimation`, `+0x3b4`). [data]
    pub idle_clip: u32,
    pub exit_clip: u32,
    pub exit_overheat_clip: u32,
    pub overheat_clip: u32,
    pub exit_arm_clip: u32,
    /// `chargingAnimation`, `chargedAnimation` (weapon `+0x44c`, `+0x450`): the object's clips
    /// the charge's start sends (`007b03c0`: Charging once, then Charged), 0 for none. [game; data]
    pub charging_clip: u32,
    pub charged_clip: u32,
    /// The `checkGroundHeight` of the `SnakerProjectile` the full charge's round
    /// (`chargeMaxProjectileObj`) lets go where it is destroyed (in its Destructable's
    /// `spawnOnDestruction`), None when it has none: OptimusFlight's cannon only, 3.0. [data]
    pub alt_snaker_check: Option<f32>,
    /// `alwaysAppear`: it is not hidden once it is put away.
    pub always_appear: bool,
    /// What a launcher's missile lets go of where it ends (`missileProjectileObj`). [data]
    pub explosion: Option<ExplosionDef>,
    /// The weapon has a `chargeMaxProjectileObj` (weapon `+0x3a8`, `007ab9e0`): the projectile
    /// a round becomes when `fire_choice` says so, and what that one lets go of where it ends.
    /// Starscream's Gatling gun: `gatlinggunspecial` (a BulletTrace with `LargeLaserPreset`
    /// impacts); his missile launcher: `nullray`. [game 007aaeb0] [data]
    pub alt_projectile: bool,
    pub alt_explosion: Option<ExplosionDef>,
    /// A flamethrower's volumes, in the order of their `index`, and how many.
    pub flame: [FlameSphere; 3],
    pub flame_count: usize,
    /// The charged shot's sounds (`gunWeaponSoundTable`): the charge's start, its loop, the
    /// full charge's, and the release's firing; 0 for none. [data]
    pub charge_start_sound: u32,
    pub charge_loop_sound: u32,
    pub charge_full_sound: u32,
    pub charge_fire_sound: u32,
    /// The effects of a charged shot or a flamethrower (`WeaponFxSpec`), None for the others.
    pub fx: Option<Box<WeaponFxSpec>>,
    /// Which entry of the character's `weapons` it is in (`optimus_weapons` 0,
    /// `optimus_lefthandweapons` 1...). The weapons of one entry share its one object: the
    /// manager makes an instance per entry (`007b0f10`) and takes every Weapon in it. [game]
    /// [data] So Bumblebee's cannon and launcher are one model, Optimus's right rifle and
    /// right cannon another, his left pair a third.
    pub list: usize,
    /// `minFireDummyIndex` (weapon `+0x330`): barrel k of a firing is the node named
    /// `FIRE_DUMMIES[first_barrel + k]` of the weapon's own object. [game 007a7a40] [data]
    pub first_barrel: usize,
}

impl WeaponDef {
    /// The name hash of the node its first barrel is (`FIRE_DUMMIES[first_barrel]`), 0 past the table.
    pub fn barrel_node(&self) -> u32 {
        FIRE_DUMMIES.get(self.first_barrel).copied().unwrap_or(0)
    }
}

impl WeaponDef {
    /// A weapon with nothing in it, for tests.
    pub fn test(kind: Kind) -> Self {
        Self {
            kind, name: 0, attribute: 0, ai_attribute: 0, mp_attribute: 0, bone: 0, fire_sound: 0, overheat_sound: 0, refused_sound: 0,
            ring_off_sound: 0, idle_sound: 0, overheat_key_off: false, muzzle: Vec3::ZERO,
            barrels: [Vec3::ZERO; 3], barrel_count: 1, idle_clip: 0, exit_clip: 0, exit_overheat_clip: 0, overheat_clip: 0,
            exit_arm_clip: 0, charging_clip: 0, charged_clip: 0, alt_snaker_check: None, always_appear: false, explosion: None, alt_projectile: false, alt_explosion: None, flame: [FlameSphere::default(); 3], flame_count: 0,
            charge_start_sound: 0, charge_loop_sound: 0, charge_full_sound: 0, charge_fire_sound: 0, fx: None, list: 0, first_barrel: 0,
        }
    }

    /// Which attribute root the weapon reads its numbers from (`FUN_007abf40`, the weapon's
    /// attribute binding, kept at weapon `+0x28`). [game 007abf40]
    ///
    /// An owner no player controls takes `<kind>AIAttribute` when it is set (not 0); else a
    /// multiplayer game takes `<kind>MPAttribute`, and when that is 0 or absent the plain
    /// `<kind>Attribute`. The flamethrower (kind 3) has no AI or MP field in that code.
    /// Where the chosen root is not in the data, `FUN_007bc9d0` falls back to the kind's
    /// default (`default_attribute`). `multiplayer`: the global at `00dc9678` has its `+4`
    /// set (`FUN_0088eb10`), which is the global `00dc967c` that the game reads as "single
    /// player when zero" elsewhere [game 00744ce0 (mp_friendly_fire_flag), 0117e760 (ai_same_team),
    /// 0079b5e0 (the optimus MP multiplier)]. So the flag is the multiplayer game, and the MP
    /// fields are the ones it picks [game 007abf40 l.40-80: `gunMPAttribute` is the
    /// `gunAttribute` hash plus 0x6a92978d when the flag is nonzero]. Its only absolute store is
    /// the init to 0 (`mov [00dc967c], ebx` at 00890eb5, in 00890e30) [game, find_ptr]; this port
    /// is single-player, so its callers pass `false`. Other writes, if any, go through the
    /// object's base (`00dc9678`, 122 code sites), not read: they could set it in a
    /// multiplayer session only. settles: those base-relative writes (the mpdata loader 0088ed00
    /// and the sites in 00756190, 00884490, 0088dfa0) and the user's game being single player.
    pub fn attribute_for(&self, by_player: bool, multiplayer: bool) -> u32 {
        let flame = self.kind == Kind::Flame;
        if !by_player && !flame && self.ai_attribute != 0 {
            return self.ai_attribute;
        }
        if multiplayer && !flame && self.mp_attribute != 0 {
            return self.mp_attribute;
        }
        self.attribute
    }

    /// What a round's projectile lets go of where it ends: the alternate projectile's when the
    /// round is one (`fire_choice`) and the weapon has one, else its own. [game 007aaeb0]
    pub fn explosion_for(&self, alt: bool) -> Option<ExplosionDef> {
        if alt && self.alt_projectile { self.alt_explosion } else { self.explosion }
    }
}

/// The fire flags `weapon_fire` gives a round's projectile (missile `+0x30`, grenade `+0x40`,
/// turret `+0x40`, through `weapon_spawn_dispatch` 00784170). [game 007ae010]
pub mod fire_flags {
    /// The owner is a type-6 special (Starscream's) and its timer runs. [game 007ae3d6]
    pub const SPECIAL: u32 = 8;
    /// The sniper's steadied shot is full (vtable `+0x128`, 007ac920, at 1 or more). [game 007ae414]
    pub const STEADIED: u32 = 0x2000;
}

/// How `weapon_fire` sends a round (007ae39b-007ae424): while the owner's type-6 special runs
/// (character type `+0x140 +0xc0` == 6, special timer `+0x234 +0x8c` > 0) the round carries
/// `fire_flags::SPECIAL`; otherwise a full sniper's steady (`sniper_steady` >= 1) gives
/// `fire_flags::STEADIED`. Either way the projectile maker (vtable `+0x80`, 007aaeb0) is asked
/// for template 1, the weapon's `chargeMaxProjectileObj`, which falls back to its own
/// projectile when it has none. Returns (flags, alternate projectile). [game]
/// [unread] a third way to STEADIED (the fire's 5th argument set and its 6th's `+0x38` > 0) is
/// not modelled: its caller's meaning is not read.
pub fn fire_choice(type6_special: bool, sniper_steady: f32) -> (u32, bool) {
    if type6_special {
        (fire_flags::SPECIAL, true)
    } else if sniper_steady >= 1.0 {
        (fire_flags::STEADIED, true)
    } else {
        (0, false)
    }
}

/// The attribute root a kind falls back to when the weapon's own is missing (`FUN_007bc9d0`):
/// `DefaultGun`, `DefaultMissileLauncher`, `DefaultGrenadeLauncher`, `DefaultFlameThrower`,
/// `DefaultTurretLauncher` (all in `bnxglobal.str`). [game 007bc9d0] [data]
pub fn default_attribute(kind: Kind) -> u32 {
    use crate::formats::hash::crc32;
    match kind {
        Kind::Gun => crc32(b"DefaultGun"),
        Kind::Launcher => crc32(b"DefaultMissileLauncher"),
        Kind::Grenade => crc32(b"DefaultGrenadeLauncher"),
        Kind::Flame => crc32(b"DefaultFlameThrower"),
        Kind::Turret => crc32(b"DefaultTurretLauncher"),
    }
}

/// The charged shot's and the flame's effects as the weapon's `thing` gives them (`WeaponFxSpec`).
/// `object` is the weapon's own object, whose attachments hold the flame's dummies. [data]
fn read_fx(thing: Node, object: Option<Node>) -> Option<Box<WeaponFxSpec>> {
    let num = |n: Node, name: &str| n.get(name).and_then(Node::float).unwrap_or(0.0);
    let mut fx = WeaponFxSpec {
        charge: thing.get("chargeId").and_then(Template::read),
        charge_ready: thing.get("chargeMaxReadyId").and_then(Template::read),
        stream: thing.get("streamEffect").and_then(Template::read),
        impact: thing.get("impactEffect").and_then(Template::read),
        balls: Vec::new(),
        charge_emission: [num(thing, "minChargeEmissionRate"), num(thing, "maxChargeEmissionRate")],
    };
    for attached in object.and_then(|o| o.get("attachments")).into_iter().flat_map(|a| a.items()) {
        let Some(dummy) = attached.get("thing").filter(|t| t.type_name() == Some("FlameThrowerDummy")) else { continue };
        let node = attached.get("index").and_then(Node::int).unwrap_or(0) as usize;
        if let Some(ball) = dummy.get("loopingParticles").into_iter().flat_map(|p| p.items()).find_map(Template::read) {
            fx.balls.push((node, ball));
        }
    }
    let any = fx.charge.is_some() || fx.charge_ready.is_some() || fx.stream.is_some() || fx.impact.is_some() || !fx.balls.is_empty();
    any.then(|| Box::new(fx))
}

/// The names of a weapon's barrel nodes by number (the table at `00c0675c`, which
/// `007a7a40` walks from `minFireDummyIndex` to `maxFireDummyIndex`): `FireDummy`,
/// `FireDummy01` ... `FireDummy09`. [game 007a7a40] The table goes on with five more
/// hashes not in `names.tsv` (presumably `FireDummy10`..`14`), left out.
pub const FIRE_DUMMIES: [u32; 10] = [
    0xeffc_a07e, 0xa82b_3c5a, 0x3122_6de0, 0x4625_5d76, 0xd841_c8d5, 0xaf46_f843, 0x364f_a9f9, 0x4148_996f, 0xd1f7_84fe,
    0xa6f0_b468,
];

/// Where a weapon object hangs: its `startBoneAttachment` looked up among the robot's nodes,
/// and when the robot has no such node, among the vehicle object's (`char + 0x258`, made
/// from `vehicleProxy`), whose node it is then linked to. [game 007a6380, 00712ef0] So every
/// car-form weapon (Cannon_Tip, Back_Window_L, Turbine_L, Windshield_R, Interior...) hangs on
/// the car. A name both have is the robot's. The game's last fallback (the robot's
/// `ObjectAttachment_Hand_R` when there is no vehicle object) is not taken: no pack of the
/// roster needs it. [data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mount {
    Robot(usize),
    Car(usize),
}

pub fn mount(bone: u32, robot: &[u32], car: Option<&[u32]>) -> Option<Mount> {
    robot.iter().position(|&h| h == bone).map(Mount::Robot).or_else(|| car?.iter().position(|&h| h == bone).map(Mount::Car))
}

/// The `checkGroundHeight` of the first `SnakerProjectile` under a projectile object: in its
/// attachments' things, a Destructable's `spawnOnDestruction` scenes, and their `things`
/// (OptimusFlight's `shellcannonbulletsnaker`: Destructable -> `radial_attack` Instance ->
/// Object -> SnakerProjectile, 3.0). [data optimusflight 754586-754760]
fn find_snaker_check(node: Node, depth: u32) -> Option<f32> {
    if depth > 10 {
        return None;
    }
    if node.type_name() == Some("SnakerProjectile") {
        return Some(node.get("checkGroundHeight").and_then(Node::float).unwrap_or(1.0));
    }
    let attached = node.get("attachments").into_iter().flat_map(|a| a.items()).filter_map(|a| a.get("thing"));
    let spawned = node.get("spawnOnDestruction").into_iter().flat_map(|s| s.items()).filter_map(|s| s.get("spawnScene"));
    let things = node.get("things").into_iter().flat_map(|t| t.items());
    attached.chain(spawned).chain(things).find_map(|n| find_snaker_check(n, depth + 1))
}

/// The rest place of the weapon object's barrel node numbered `index`.
fn fire_dummy(object: Option<Node>, index: usize) -> Option<Vec3> {
    let object = object?;
    let wanted = *FIRE_DUMMIES.get(index)?;
    let names = object.path(&["hierarchy", "names"])?.ints();
    let node = names.iter().position(|&name| name as u32 == wanted)?;
    if node == 0 {
        return Some(Vec3::ZERO);
    }
    let rest = object.get("sub_matrices")?.at(node - 1)?.floats();
    (rest.len() == 12).then(|| Vec3::new(rest[9], rest[10], rest[11]))
}

/// A flamethrower object's dummies and their spheres: a `FlameThrowerDummy` attachment
/// sits on the node its `index` beside it names, and the `Collision` whose `parent` is
/// that node holds its sphere. [data]
fn read_flame(object: Option<Node>) -> ([FlameSphere; 3], usize) {
    let mut out = ([FlameSphere::default(); 3], 0);
    let Some(object) = object else { return out };
    let attached: Vec<Node> = object.get("attachments").into_iter().flat_map(|a| a.items()).collect();
    let int = |node: Option<Node>| node.and_then(Node::int).unwrap_or(0) as i64;
    let mut dummies: Vec<(i64, i64, f32)> = attached
        .iter()
        .filter_map(|a| {
            let thing = a.get("thing").filter(|t| t.type_name() == Some("FlameThrowerDummy"))?;
            Some((int(thing.get("index")), int(a.get("index")), thing.get("delayTilTurningOnNextDummy").and_then(Node::float).unwrap_or(0.0)))
        })
        .collect();
    dummies.sort_by_key(|d| d.0);
    for (_, node, delay_to_next) in dummies.into_iter().take(3) {
        let radius = attached
            .iter()
            .filter_map(|a| a.get("thing"))
            .filter(|t| t.type_name() == Some("Collision") && int(t.get("parent")) == node)
            .flat_map(|t| t.get("primitives").into_iter().flat_map(|p| p.items()))
            .find_map(|p| p.path(&["packet", "radius"]).and_then(Node::float));
        let Some(radius) = radius else { continue };
        let at = match node {
            0 => Some(Vec3::ZERO),
            n => object.get("sub_matrices").and_then(|m| m.at(n as usize - 1)).map(|m| m.floats()).filter(|m| m.len() == 12).map(|m| Vec3::new(m[9], m[10], m[11])),
        };
        out.0[out.1] = FlameSphere { at: at.unwrap_or(Vec3::ZERO), radius, delay_to_next, node: node.max(0) as usize };
        out.1 += 1;
    }
    out
}

/// The weapon classes `read_defs` reads, in its order. Every reader that lists the weapons
/// (`read_objects`, `read_object_clips`) takes the same ones, so each list lines up with
/// `read_defs` index for index. [game]
pub const WEAPON_CLASSES: [&str; 5] = ["GunWeapon", "MissileLauncher", "GrenadeLauncher", "FlameThrower", "TurretLauncher"];

/// Whether an attachment's `thing` is one of `WEAPON_CLASSES`.
pub fn is_weapon(thing: Node) -> bool {
    thing.type_name().is_some_and(|name| WEAPON_CLASSES.contains(&name))
}

/// A weapon class's kind and the attribute block its numbers come from.
fn weapon_class(name: &str) -> Option<(Kind, &'static str)> {
    match name {
        "GunWeapon" => Some((Kind::Gun, "gunAttribute")),
        "MissileLauncher" => Some((Kind::Launcher, "missileLauncherAttribute")),
        "GrenadeLauncher" => Some((Kind::Grenade, "grenadeLauncherAttribute")),
        "FlameThrower" => Some((Kind::Flame, "flameThrowerAttribute")),
        "TurretLauncher" => Some((Kind::Turret, "turretLauncherAttribute")),
        _ => None,
    }
}

/// The weapons a `Character` object lists, in its own order (primary first).
pub fn read_defs(character: Node) -> Vec<WeaponDef> {
    let id = |node: Option<Node>| node.and_then(Node::int).map_or(0, |h| h as u32);
    let mut defs = Vec::new();
    for (list, file) in character.get("weapons").into_iter().flat_map(|weapons| weapons.items()).enumerate() {
        let attached = file.path(&["obj", "attachments"]).into_iter().flat_map(|a| a.items());
        for thing in attached.filter_map(|a| a.get("thing")) {
            let Some((kind, attribute)) = thing.type_name().and_then(weapon_class) else { continue };
            let sound = |field: &str| id(thing.path(&["genericWeaponSoundTable", field]));
            let gun_sound = |field: &str| id(thing.path(&["gunWeaponSoundTable", field]));
            let (first, last) = (id(thing.get("minFireDummyIndex")) as usize, id(thing.get("maxFireDummyIndex")) as usize);
            let mut barrels = [Vec3::ZERO; 3];
            let mut barrel_count = 0;
            for index in first..=last.min(FIRE_DUMMIES.len() - 1) {
                if barrel_count == barrels.len() {
                    break;
                }
                if let Some(at) = fire_dummy(file.get("obj"), index) {
                    barrels[barrel_count] = at;
                    barrel_count += 1;
                }
            }
            let (flame, flame_count) = if kind == Kind::Flame { read_flame(file.get("obj")) } else { Default::default() };
            defs.push(WeaponDef {
                list,
                first_barrel: first,
                flame,
                flame_count,
                barrels,
                barrel_count,
                kind,
                name: id(thing.get("weaponName")),
                attribute: id(thing.get(attribute)),
                ai_attribute: id(thing.get(&attribute.replace("Attribute", "AIAttribute"))),
                mp_attribute: id(thing.get(&attribute.replace("Attribute", "MPAttribute"))),
                bone: id(thing.get("startBoneAttachment")),
                fire_sound: sound("fireSound"),
                overheat_sound: sound("overHeatSound"),
                refused_sound: sound("fireOverHeatedSound"),
                ring_off_sound: sound("fireRingOffSound"),
                idle_sound: sound("weaponIdleSound"),
                overheat_key_off: thing.path(&["genericWeaponSoundTable", "fireOverHeatKeyOff"])
                    .is_some_and(|f| f.int() == Some(1) || f.is_true()),
                charge_start_sound: gun_sound("chargeStartSound"),
                charge_loop_sound: gun_sound("chargeLoopSound"),
                charge_full_sound: gun_sound("chargeMaxReadySound"),
                charge_fire_sound: gun_sound("chargeFireSound"),
                // Every kind: the charged guns (Optimus's, OptimusFlight's, the Megatrons') carry
                // `chargeId` / `chargeMaxReadyId` too; a weapon with none gets None. [data]
                fx: read_fx(thing, file.get("obj")),
                charging_clip: id(thing.get("chargingAnimation")),
                charged_clip: id(thing.get("chargedAnimation")),
                alt_snaker_check: thing.path(&["chargeMaxProjectileObj", "obj"]).and_then(|o| find_snaker_check(o, 0)),
                muzzle: fire_dummy(file.get("obj"), id(thing.get("minFireDummyIndex")) as usize).unwrap_or(Vec3::ZERO),
                idle_clip: id(thing.get("idleAnimation")),
                exit_clip: id(thing.get("exitAnimation")),
                exit_overheat_clip: id(thing.get("exitOverheatAnimation")),
                overheat_clip: id(thing.get("overheatAnimation")),
                exit_arm_clip: id(thing.get("exitCharacterAnimation")),
                always_appear: thing.get("alwaysAppear").is_some_and(|f| f.int() == Some(1) || f.is_true()),
                explosion: thing
                    .path(&["missileProjectileObj", "obj"])
                    .or_else(|| thing.path(&["grenadeProjectileObj", "obj"]))
                    // [data] A gun's bullet can carry an explosion too: Megatron's special beam
                    // (`bulletProjectileObj`, a DamageLink on an Explosion of damage 200). [assumed]
                    // The blast starts where a round lands (`game/src/weapons.rs` `shoot`).
                    // 2026-10-20: `bullet_visual_update` (00782310) has no explosion spawn of its
                    // own. In its synchronous-hit branch it calls `bullet_damage` 00782b30 (when the object's
                    // damage flag allows) and `ambient_reaction_spawn_short` 00951860; in the other
                    // branch it sends the object's trigger callbacks (`object_trigger_callbacks`
                    // 007479f0, message with a zero vector) and then stores the hit point into the
                    // object (`+0x34..+0x3c`). The DamageLink's start is the message of 007479f0,
                    // not read. [game 007479f0, read 2026-10-20] the callback is a dispatch: it looks up
                    // the registry key 2fe3042d (005bbc00) and calls each entry of the object's list at
                    // +0x10 (entry +0x8 the receiver, entry +0xc the function, with the trigger). So the
                    // explosion's start is whichever listener was added to that list (not read: the
                    // adder's caller is the loader of the bullet's DamageLink). settles: that adder
                    // (the writer of the bullet object's +0x10 list, the DamageLink load), and where the
                    // explosion is put (the trigger's receiver's position).
                    .or_else(|| if kind == Kind::Gun { thing.path(&["bulletProjectileObj", "obj"]) } else { None })
                    .and_then(crate::formats::scene::read_projectile_explosion),
                alt_projectile: thing.path(&["chargeMaxProjectileObj", "obj"]).is_some(),
                alt_explosion: thing.path(&["chargeMaxProjectileObj", "obj"]).and_then(crate::formats::scene::read_projectile_explosion),
            });
        }
    }
    defs
}

/// Each weapon's own object (its node tree and rendered parts), in the order of
/// `read_defs`; `None` where the weapon's file has none that reads.
pub fn read_objects<'a>(character: Node<'a>, name_of: impl Fn(u32) -> Option<&'a str> + Copy) -> Vec<Option<crate::formats::scene::ObjectDef>> {
    let mut objects = Vec::new();
    for file in character.get("weapons").into_iter().flat_map(|weapons| weapons.items()) {
        let attached = file.path(&["obj", "attachments"]).into_iter().flat_map(|a| a.items());
        for thing in attached.filter_map(|a| a.get("thing")) {
            if is_weapon(thing) {
                objects.push(file.get("obj").and_then(|object| crate::formats::scene::read_object(object, name_of)));
            }
        }
    }
    objects
}

/// Each weapon's projectile model (`grenadeProjectileObj`'s own object: a grenade's, drawn in
/// flight), in the order of `read_defs`; `None` where the weapon has none that reads. [data]
/// The grenade's model is its `grenadeProjectileObj`'s own `obj` root: that root's attachments hold
/// the RenderThing that draws it beside the GrenadeProjectile and the Collision of the same object
/// [data: the Audi's `gravitonmine`, RenderThing model `GravitonMine.GravitonMine`,
/// work\survey\weapons\audir8.txt l.581883-581924; Jetfire's `jetfiremissiledump`, RenderThing model
/// `JetFireMissileDump.JetFireMissileDump`, jetfire.txt l.651810-651850, its GrenadeProjectile at l.652449].
pub fn read_grenade_objects<'a>(character: Node<'a>, name_of: impl Fn(u32) -> Option<&'a str> + Copy) -> Vec<Option<crate::formats::scene::ObjectDef>> {
    let mut objects = Vec::new();
    for file in character.get("weapons").into_iter().flat_map(|weapons| weapons.items()) {
        let attached = file.path(&["obj", "attachments"]).into_iter().flat_map(|a| a.items());
        for thing in attached.filter_map(|a| a.get("thing")) {
            if is_weapon(thing) {
                objects.push(thing.path(&["grenadeProjectileObj", "obj"]).and_then(|object| crate::formats::scene::read_object(object, name_of)));
            }
        }
    }
    objects
}

/// Each weapon's deployed turret model (a `TurretLauncher`'s `deployableTurretObj`: `blackout_turret`,
/// `ironhide_turret`), in the order of `read_defs`; `None` for every other weapon. [data]
pub fn read_turret_objects<'a>(character: Node<'a>, name_of: impl Fn(u32) -> Option<&'a str> + Copy) -> Vec<Option<crate::formats::scene::ObjectDef>> {
    each_weapon(character, |thing| thing.path(&["deployableTurretObj", "obj"]).and_then(|object| crate::formats::scene::read_object(object, name_of)))
}

/// Each weapon's missile model (a launcher's `missileProjectileObj`: Bumblebee's
/// `micromissilescluster`), in the order of `read_defs`. [data]
pub fn read_missile_objects<'a>(character: Node<'a>, name_of: impl Fn(u32) -> Option<&'a str> + Copy) -> Vec<Option<crate::formats::scene::ObjectDef>> {
    each_weapon(character, |thing| thing.path(&["missileProjectileObj", "obj"]).and_then(|object| crate::formats::scene::read_object(object, name_of)))
}

/// Each deployed turret model's own clips (the `AnimRefBundle` in `deployableTurretObj`:
/// `Compact_Idle`, `Deploy`, `Idle`, `Fire`, `Overheat`, `UnDeploy`), in the order of `read_defs`. [data]
pub fn read_turret_clips(data: &crate::formats::lxb::DataFile, character: Node) -> Vec<Vec<crate::formats::anim::AnimRef>> {
    each_weapon(character, |thing| {
        let attached = thing.path(&["deployableTurretObj", "obj", "attachments"]).into_iter().flat_map(|a| a.items());
        attached
            .filter_map(|a| a.get("thing"))
            .filter(|t| t.type_name() == Some("AnimRefBundle"))
            .flat_map(|bundle| crate::formats::anim::read_bundle(data, bundle))
            .collect()
    })
}

/// One value per weapon of the character's list, in the order of `read_defs`.
fn each_weapon<'a, T>(character: Node<'a>, mut read: impl FnMut(Node<'a>) -> T) -> Vec<T> {
    let mut out = Vec::new();
    for file in character.get("weapons").into_iter().flat_map(|weapons| weapons.items()) {
        let attached = file.path(&["obj", "attachments"]).into_iter().flat_map(|a| a.items());
        for thing in attached.filter_map(|a| a.get("thing")) {
            if is_weapon(thing) {
                out.push(read(thing));
            }
        }
    }
    out
}

/// Each weapon object's own clips (its `AnimRefBundle`: `Idle_WeaponA`, `Idle_WeaponB`,
/// `Overheat_Weapon_A`, `Overheat_Weapon_B` for his), in the order of `read_defs`. [data]
pub fn read_object_clips(data: &crate::formats::lxb::DataFile, character: Node) -> Vec<Vec<crate::formats::anim::AnimRef>> {
    let mut clips = Vec::new();
    for file in character.get("weapons").into_iter().flat_map(|weapons| weapons.items()) {
        let attached = || file.path(&["obj", "attachments"]).into_iter().flat_map(|a| a.items()).filter_map(|a| a.get("thing"));
        for thing in attached() {
            if is_weapon(thing) {
                let bundles = attached().filter(|t| t.type_name() == Some("AnimRefBundle"));
                clips.push(bundles.flat_map(|bundle| crate::formats::anim::read_bundle(data, bundle)).collect());
            }
        }
    }
    clips
}

/// A melee weapon (Bumblebee's axe, Optimus's two arm blades, Megatron's claw...): a weapon
/// of the manager's list like the guns, one slot of its 8 (`+0x34`, stride 8), told apart
/// from a gun by its slot `+0x30` saying no (`007bb820`) [game 007b4680, 007b2d80, 01185aa0],
/// with an object of its own (his: 11 nodes, six handle links and a blade in five parts),
/// its clips (`Trans_R2W` unfolding it, `Idle`, `Trans_W2R`) and no numbers. Six packs'
/// objects carry an `Attack` box: the blade the attack clips switch on (`melee::MeleeBlade`).
/// [data]
pub struct MeleeWeapon {
    /// `weaponName` (`MeleeWeapon`, Optimus's and the Audi's second `MeleeWeapon_L`), which
    /// the attack clips' events name.
    pub name: u32,
    /// `startBoneAttachment` (CRC-32 of a node name): where the slot is linked at load,
    /// before any event (`007a6380`; `melee::MeleeBlade::link`). [game] [data]
    pub start_node: u32,
    /// `exitAnimation`: what its object plays when it is put away (`Trans_W2R`).
    pub exit_clip: u32,
    pub object: Option<crate::formats::scene::ObjectDef>,
    pub clips: Vec<crate::formats::anim::AnimRef>,
}

/// Every melee weapon of the character's `weapons`, in list order (the manager's slot order),
/// each with its own list's clips. [game 007b4680: every slot] [data]
pub fn read_melees<'a>(
    data: &crate::formats::lxb::DataFile,
    character: Node<'a>,
    name_of: impl Fn(u32) -> Option<&'a str> + Copy,
) -> Vec<MeleeWeapon> {
    let id = |node: Option<Node>| node.and_then(Node::int).map_or(0, |h| h as u32);
    let mut out = Vec::new();
    for file in character.get("weapons").into_iter().flat_map(|weapons| weapons.items()) {
        let attached = || file.path(&["obj", "attachments"]).into_iter().flat_map(|a| a.items()).filter_map(|a| a.get("thing"));
        for thing in attached().filter(|thing| thing.type_name() == Some("MeleeWeapon")) {
            let bundles = attached().filter(|t| t.type_name() == Some("AnimRefBundle"));
            out.push(MeleeWeapon {
                name: id(thing.get("weaponName")),
                start_node: id(thing.get("startBoneAttachment")),
                exit_clip: id(thing.get("exitAnimation")),
                object: file.get("obj").and_then(|object| crate::formats::scene::read_object(object, name_of)),
                clips: bundles.flat_map(|bundle| crate::formats::anim::read_bundle(data, bundle)).collect(),
            });
        }
    }
    out
}

/// A weapon's numbers, named as the data names them.
#[derive(Clone, Default, Debug)]
pub struct WeaponTuning {
    /// Seconds between shots.
    pub rate_of_fire: f32,
    pub automatic_firing: bool,
    pub projectiles_per_fire: u32,
    /// `burstPerFire` (attributes `+0x1c`): firings one press lets go, the first by the
    /// trigger and the rest by themselves `time_between_bursts` apart (`Weapon::update`).
    /// Ironhide's laser cannon 2, Breakaway's BurstMiniGun 5, Starscream's homing missile 3,
    /// Grindor's car gun 4; 1 for the rest. [data] [game 007ae010, 007ac370]
    pub burst_per_fire: u32,
    /// `timeBetweenEachBurst` (attributes `+0x20`), seconds. [data]
    pub time_between_bursts: f32,
    pub heat_capacity: f32,
    pub heat_per_round: f32,
    pub heat_decrement_per_second: f32,
    pub heat_cooldown_til_decrement: f32,
    pub overheat_period: f32,
    /// How far a player's shots scatter about the aim point, as a share of half the
    /// picture's height at that distance (`playerNoiseRadius`; see `scattered_point`).
    pub player_noise_radius: f32,
    pub general_noise_radius: f32,
    /// The pad's rumble as a round leaves: seconds and strength (`rumbleFireDuration`,
    /// `rumbleFireStrength`; the weapon's fire calls `FUN_00717be0(1, ...)` with them).
    pub rumble_fire_duration: f32,
    pub rumble_fire_strength: f32,
    /// `mustHoldFireTriggerToLockOn` (launchers): the round leaves when the trigger is let
    /// go, not while it is held.
    pub hold_to_lock_on: bool,
    // Guns.
    pub bullet_speed: f32,
    pub bullet_range: f32,
    pub damage: f32,
    pub knock_back_speed: f32,
    pub damage_flags: u32,
    pub knock_back_angle: f32,
    pub stun_time: f32,
    pub slow_time: f32,
    pub dot_time: f32,
    pub heat_damage: f32,
    /// `bulletDamageFallOffRange`: from this far the damage falls to `minDamage` at the
    /// range; negative (his cannon's -1) for none.
    pub bullet_fall_off_range: f32,
    pub min_damage: f32,
    /// `tracerFirePerRound`: over 0 (all his guns), the round is traced over its whole
    /// range and lands in the update it is fired; else it flies at `bulletSpeed`.
    pub tracer_fire_per_round: i32,
    /// `positionalDamageMultiplier`: head, upper body, lower body, vehicle body.
    pub positional: [f32; 4],
    // Launchers.
    pub missile_start_speed: f32,
    pub missile_top_speed: f32,
    pub missile_acceleration: f32,
    pub missile_lifetime: f32,
    pub explosive_damage: f32,
    /// `overwriteExplosiveMinDamage`: -1 (his) leaves the explosion's own.
    pub explosive_min_damage: f32,
    pub lock_on_time: f32,
    /// `guided`: it steers at the target it was locked on.
    pub guided: bool,
    /// `lockOnBracketStayOnTime`: how long a lock outlasts its target leaving the box.
    pub lock_bracket_stay: f32,
    /// `missileLockOnBoxWidth` and `Height`, in pixels of a 1280 x 720 picture.
    pub lock_box: [f32; 2],
    /// `aimAssistMaxRange` (every kind of weapon has one: his cannon 300, his missiles
    /// 350, the car's gun 500): how far from the camera a target may be for the aim assist
    /// and the lock; 0 for the shared default.
    pub aim_assist_max_range: f32,
    /// `minGuidanceDelay`, `maxGuidanceDelay`: seconds before it steers at all.
    pub guidance_delay: [f32; 2],
    /// `minGuidanceOffDelay`, `maxGuidanceOffDelay`: seconds of steering before it gives
    /// up; negative (his) for never.
    pub guidance_off_delay: [f32; 2],
    /// The cosine of `guidedViewAngle` (the loader stores the cosine).
    pub guided_view_cos: f32,
    /// `outOfViewTurnSpeed`, in radians a second.
    pub out_of_view_turn: f32,
    /// `minAccelFactorDelay`, `maxAccelFactorDelay`, and `accelFactor`.
    pub accel_factor_delay: [f32; 2],
    pub accel_factor: f32,
    /// `minWobbleTimer`, `maxWobbleTimer`; `maxWobbleAngle` in radians; `minWobbleDuration`,
    /// `maxWobbleDuration`.
    pub wobble_timer: [f32; 2],
    pub wobble_angle: f32,
    pub wobble_duration: [f32; 2],
    /// The cosine of `pastTargetConeAngleActivated`.
    pub past_target_cos: f32,
    /// `isVehicleModeWeapon`: fitted to the car, not the robot.
    pub vehicle_mode: bool,
    /// `useFacingAsTarget` (attributes `+0x60`, the weapon's slot `+0xa8`): each round is
    /// sent at a point straight ahead of its own barrel, not at the manager's aim point
    /// (`FUN_007ae010`, `FUN_007aab70`). The car's gun.
    pub use_facing_as_target: bool,
    /// `weaponChildren` and `weaponParent` (names of weapons, CRC-32): a left-hand weapon is
    /// the CHILD of the right-hand one. The manager links them at its fill (`007b0f10`: a weapon
    /// whose parent slot `+0xdc` is set goes to the weapon whose children `+0xe0`/`+0xe4` name
    /// it, `+0xe8` stores it at `+0x394`, one child at most, `007ab060`). A child is never the
    /// weapon in hand: it fires from its parent's firing (`007ae010`, see `alternating_fire`),
    /// does not charge on its own (`007ad1c0` returns for a weapon with a parent), and its
    /// object takes its parent's clips (`007ab0f0`...). [game] [data]
    pub children: Vec<u32>,
    pub parent: u32,
    /// `alternatingFire` (attributes `+0xa0`): with it, the parent and its child take turns,
    /// one firing each (the parent's turn counter `+0x39c`: 0 the parent, 1 the child); without
    /// it, the child fires with every firing of the parent. [game 007ae144..007ae2dd, 007af0ee]
    pub alternating_fire: bool,
    /// `isSpecial` (attributes `+0x5d`, the weapon's slot `+0x9c`): the special's weapon, which
    /// the switches never take into his hand (`007b3ea0`, `007b4080`); the special fires it by
    /// name (`007b3c40`). Megatron's beam gun, Jetfire's missile dump, the Audi's mines,
    /// Starscream's special missile, the turrets. [game] [data]
    pub is_special: bool,
    /// Values the data did not have.
    pub missing: Vec<String>,
    /// A grenade launcher's own numbers.
    pub grenade: GrenadeTuning,
    /// `snakerInfo` (read by the gun, launcher and grenade loaders through `FUN_007bba30`):
    /// what a `SnakerProjectile` the weapon fires does. [game 007bba30]
    pub snaker: SnakerTuning,
    /// The charged shot's numbers (the base loader `FUN_007bbc30`).
    pub charge: ChargeTuning,
    /// `snipeCameraEnabled` and the gun's snipe charge (`crate::snipe`). [game 007bbc30, 007bd0d0]
    pub snipe: crate::snipe::SnipeTuning,
    /// A flamethrower's `damagePerSecond` and `damageTicsPerSecond` (loader `FUN_007bf710`).
    pub flame_damage_per_second: f32,
    pub flame_ticks_per_second: f32,
    /// `listenToWeaponAnimEvent` (attribute `+0x5c`, weapon slot `+0x34` `0074f300`): true, the
    /// weapon manager leaves showing, hiding and setting it "out" to the arm clip's
    /// `WeaponAttachmentEvent` (`007b4680`); false, the manager does them at once (`007b2b40`,
    /// `007b2c60`). [game] Read only; the port's held weapons already take the event path.
    pub listen_to_anim_event: bool,
}

/// The charged shot, by the data's names (`notes\weapon-charged-shot.md`). [data]
#[derive(Clone, Copy, Default, Debug)]
pub struct ChargeTuning {
    /// `hasChargedShot`.
    pub has: bool,
    /// `timeTilChargeStarts`, `chargeDurationTilMaxEffect`, `chargeHoldDuration` (not
    /// positive: held as long as wanted).
    pub start: f32,
    pub duration: f32,
    pub hold: f32,
    /// `minChargeDamageMultiplier`, `maxChargeDamageMultiplier`, `minHeatMeterIncrement`,
    /// `maxHeatMeterIncrement`: only for a weapon whose count below is under 2.
    pub damage: [f32; 2],
    pub heat: [f32; 2],
    /// `chargeNumProjectilesMaxMultiplier`: how many firings a full charge lets go.
    pub count: u32,
    /// `heatMeterOverHeatRateWhileCharging` (a second) and
    /// `chargeHeatIncrementPerMultiplierIncrement` (once for each firing gained).
    pub heat_rate: f32,
    pub heat_per_step: f32,
}

/// What a `GrenadeLauncher`'s attributes add (loader `FUN_007bea00`), by the data's names.
/// [data] Not read here: the cluster, the sticky kind, the spin, `oneOutAtATime`.
#[derive(Clone, Copy, Default, Debug)]
pub struct GrenadeTuning {
    /// Radians (`launchArcAngle`, degrees in the data).
    pub launch_arc_angle: f32,
    /// Seconds before it goes off by itself (`grenadeMinLifetime`, `grenadeMaxLifetime`).
    pub lifetime: [f32; 2],
    pub fire_towards_target: bool,
    pub z_gravity: f32,
    pub is_proximity: bool,
    pub proximity_radius: f32,
    /// `proximityExplosionMinDelay`, `MaxDelay` (`+0x180`, `+0x184`, loader 007bea00): when the
    /// proximity test finds a target, the grenade is marked going off (`+0x5a`) and hits at once
    /// when the minimum is 0 or less, else after a time drawn in this range (`random_range`
    /// 0078ff10, the hit message 54732c8d posted). [game 007873f0 at 007875b9-0078761c]
    pub proximity_delay: [f32; 2],
    pub bounciness: f32,
    pub dampening_on_collision: f32,
    pub launch_speed: f32,
    pub tack_on_owner_speed: bool,
    /// `isSticky`: it stops where it first touches the world and stays; its velocity and
    /// gravity are zeroed (`FUN_00787ea0`). [data] [game]
    pub is_sticky: bool,
    /// `proximityActivateOnCollision` (`+0x179`, loader 007bea00): a proximity grenade is armed
    /// by its first contact. `FUN_00786a60` arms it at launch only when this is false;
    /// `FUN_007873f0` counts targets only once armed (`+0x59`). [data] [game]
    pub arms_on_collision: bool,
    /// `stickyOffsetFromCollision` (`+0x194`, loader 007bea00): how far off the surface a sticky
    /// grenade rests, along the contact normal (`FUN_00787ea0`). [data] [game]
    pub sticky_offset: f32,
    /// `stickyMinLifetime`, `stickyMaxLifetime` (`+0x18c`, `+0x190`, loader 007bea00): a sticky
    /// grenade with a minimum above 0 has no launch fuse (`FUN_00786a60` l.30-40); when it sticks
    /// it goes off a drawn time in this range later (`sticky_attach` 00788da5-00788e19 posts the
    /// hit message 54732c8d). With a minimum of 0 or less it keeps the launch fuse, and one that is
    /// not a proximity grenade goes off as it sticks (00788e20-00788e2b). [game] Jetfire's carpet
    /// bomb: 0.1 to 0.1; the Audi's mines: -1. Absent: -1 (`attribute_default`).
    pub sticky_lifetime: [f32; 2],
    /// `oneOutAtATime` (`+0x16c`, loader 007bea00; the Audi's mines): when one of this weapon's
    /// grenades is armed, its other live ones are expired, faded out without going off
    /// (`sticky_expire_fadeout` 00789b10). [game 00786a60 l.6-28]
    pub one_out_at_a_time: bool,
    /// `velocityOrient` (`+0x1a9`): the model turns its +y to the velocity (look-at 0056ede0, at
    /// a speed above sqrt(0.001)); else it spins. [game 00786ff0] Only the Audi's mines have it.
    pub velocity_orient: bool,
    /// `spinAxis` (`+0x1ac`), `randomSpinAxis` (`+0x1b8`), `minSpinSpeed` / `maxSpinSpeed`
    /// (`+0x1bc`, `+0x1c0`, radians a second: the loader converts the degrees). [game 00786a60
    /// l.45-79, loader 007bea00] [data]
    pub spin_axis: Vec3,
    pub random_spin_axis: bool,
    pub spin_speed: [f32; 2],
}

impl GrenadeTuning {
    /// The sticky timer replaces the launch fuse (`FUN_00786a60` l.30-40). [game]
    pub fn sticky_timer(&self) -> bool {
        self.is_sticky && self.sticky_lifetime[0] > 0.0
    }
}

/// Whether a thrown grenade's proximity test counts a target. [game 0117e6d0] Single player:
/// only the two factions' teams are hostile to each other (`0x951af706` the Autobots against
/// `0xa54dc247` the Decepticons [data: the attributes' `team`], the character's word `+0x244`);
/// any other team is hostile to nobody. Multiplayer (not built):
/// any other team (`ai_same_team`).
pub fn thrower_hostile(thrower_team: u32, target_team: u32) -> bool {
    const A: u32 = 0x951a_f706;
    const B: u32 = 0xa54d_c247;
    (thrower_team == A && target_team == B) || (thrower_team == B && target_team == A)
}

/// What an attribute holds before its loader reads the data: the shared weapon attribute's
/// constructor (`FUN_007bca70`) and the gun's (`FUN_007bd4e0`, fields from `0x130`); a
/// field the data leaves out keeps it. Names as the loaders `FUN_007bbc30` and
/// `FUN_007bd0d0` read them. [game 007bca70, 007bd4e0]
fn attribute_default(name: &str, kind: Kind) -> Option<f32> {
    let shared = match name {
        "numProjectilesPerFire" | "burstPerFire" | "chargeNumProjectilesMaxMultiplier" => 1.0,
        "rateOfFire" | "heatMeterCooldownTilDecrement" | "minChargeDamageMultiplier" | "rumbleFireStrength" => 1.0,
        "automaticFiring" => 1.0,
        "timeBetweenEachBurst" => 0.01,
        "heatMeterCapacity" => 100.0,
        "heatMeterIncrementPerRound" | "heatMeterOverHeatPeriod" | "minHeatMeterIncrement" => 5.0,
        "heatMeterDecrementPerSecond" | "heatMeterOverHeatRateWhileCharging" | "maxChargeDamageMultiplier" => 2.0,
        "heatMeterOverHeatCooldownRate" | "chargeHoldDuration" => -1.0,
        "timeTilChargeStarts" => 0.5,
        "chargeDurationTilMaxEffect" => 3.0,
        "maxHeatMeterIncrement" | "chargeHeatIncrementPerMultiplierIncrement" => 10.0,
        "rumbleFireDuration" => 0.25,
        "playerNoiseRadius" | "generalNoiseRadius" | "aimAssistMaxRange" => 0.0,
        _ => f32::NAN,
    };
    if !shared.is_nan() {
        return Some(shared);
    }
    if kind == Kind::Launcher {
        // [game 007be470] the missile launcher's own. The loader (`FUN_007bd900`) converts
        // `guidedViewAngle` and `pastTargetConeAngleActivated` to the cosine of their degrees
        // and `outOfViewTurnSpeed` and `maxWobbleAngle` to radians WHETHER OR NOT the data
        // has them, so a default goes through the same conversion as a read value: the turn
        // speed's 2.356 becomes 2.356 degrees a second, and the cone's default, set by the
        // constructor as cos(30 deg) = 0.866, becomes cos(0.866 deg). Each default here is
        // the constructor's raw value, which `tuning_from` then converts like any other.
        return Some(match name {
            "overwriteDirectDamage" | "overwriteExplosiveDamage" | "overwriteExplosiveMinDamage" => -1.0,
            "minGuidanceOffDelay" | "maxGuidanceOffDelay" => -1.0,
            "missileTopSpeed" => 100.0,
            "missileStartSpeed" => 10.0,
            "missileAcceleration" => 50.0,
            "missileLifetime" | "guidedViewAngle" | "distTilTwirlStartsShortening" => 15.0,
            "missileCrosshairLockOnTime" => 0.0,
            "minGuidanceDelay" | "maxGuidanceDelay" | "minAccelFactorDelay" | "maxAccelFactorDelay" => 0.2,
            "minWobbleTimer" | "maxWobbleDuration" => 0.2,
            "accelFactor" => 2.0,
            "maxWobbleTimer" => 0.8,
            "minWobbleDuration" => 0.02,
            "pastTargetConeAngleActivated" => 0.866_025_4,
            "outOfViewTurnSpeed" => 2.356_194_5,
            "maxWobbleAngle" => 180.0,
            "lockOnBracketStayOnTime" | "minTwirlRadius" | "twirlsPerSecond" => 1.0,
            "missileLockOnBoxWidth" | "missileLockOnBoxHeight" => 128.0,
            "maxTwirlRadius" => 1.5,
            _ => return None,
        });
    }
    if kind == Kind::Grenade {
        // [game 007bf380] the grenade launcher's own (constructor 007be930). As with the
        // missile launcher, the loader (`FUN_007bea00`) converts `launchArcAngle` and the
        // spin speeds to radians whether or not the data has them.
        return Some(match name {
            "overwriteDirectDamage" | "overwriteExplosiveDamage" | "overwriteExplosiveMinDamage" => -1.0,
            "stickyMinLifetime" | "stickyMaxLifetime" => -1.0,
            "damageFlags" => 2560.0,
            "launchArcAngle" | "proximityExplosionMinDelay" | "proximityExplosionMaxDelay" | "stickyOffsetFromCollision" => 0.0,
            "grenadeMinLifetime" | "grenadeMaxLifetime" => 4.0,
            "Zgravity" => -9.81,
            "maxClusterAngle" | "launchSpeed" => 30.0,
            "proximityRadius" => 3.0,
            "bounciness" => 0.6,
            "friction" => 1.0,
            "dampeningOnCollision" => 0.995,
            "minSpinSpeed" => std::f32::consts::PI,
            "maxSpinSpeed" => std::f32::consts::TAU,
            // The flags: 1 for set.
            "fireTowardsTarget" | "proximityActivateOnCollision" | "randomSpinAxis" => 1.0,
            "oneOutAtATime" | "isProximity" | "isSticky" | "stickOrient" | "tackOnOwnerSpeed" | "velocityOrient" => 0.0,
            _ => return None,
        });
    }
    if kind == Kind::Flame {
        // [game 007bf830] the flamethrower's own (constructor 007bf670).
        return Some(match name {
            "damagePerSecond" => 100.0,
            "damageTicsPerSecond" => 20.0,
            "damageFlags" => 16.0,
            _ => return None,
        });
    }
    if kind != Kind::Gun {
        return None;
    }
    Some(match name {
        "bulletSpeed" | "bulletDamageFallOffRange" => 300.0,
        "bulletRange" => 1000.0,
        "damage" => 5.0,
        "damageFlags" => 512.0,
        "minDamage" => 1.0,
        "tracerFirePerRound" => -1.0,
        "maxSnipeDamage" => 100.0,
        "timeTilSnipeChargeStarts" => 0.5,
        "snipeChargeDurationTilMaxEffect" => 2.0,
        _ => return None,
    })
}

fn tuning_from(node: Node, kind: Kind) -> WeaponTuning {
    let mut missing = Vec::new();
    let mut num = |name: &str| match node.get(name).and_then(Node::float) {
        Some(v) => v,
        None => attribute_default(name, kind).unwrap_or_else(|| {
            missing.push(name.to_owned());
            0.0
        }),
    };
    let mut t = WeaponTuning {
        rate_of_fire: num("rateOfFire"),
        projectiles_per_fire: num("numProjectilesPerFire").max(1.0) as u32,
        // [game 007ae230] the count is compared signed (`jl`): 0 or less ends a burst at
        // its first firing, as 1 does.
        burst_per_fire: num("burstPerFire").max(1.0) as u32,
        time_between_bursts: num("timeBetweenEachBurst"),
        heat_capacity: num("heatMeterCapacity"),
        heat_per_round: num("heatMeterIncrementPerRound"),
        heat_decrement_per_second: num("heatMeterDecrementPerSecond"),
        heat_cooldown_til_decrement: num("heatMeterCooldownTilDecrement"),
        overheat_period: num("heatMeterOverHeatPeriod"),
        rumble_fire_duration: num("rumbleFireDuration"),
        rumble_fire_strength: num("rumbleFireStrength"),
        aim_assist_max_range: num("aimAssistMaxRange"),
        ..Default::default()
    };
    match kind {
        Kind::Gun => {
            t.player_noise_radius = num("playerNoiseRadius");
            t.general_noise_radius = num("generalNoiseRadius");
            t.bullet_speed = num("bulletSpeed");
            t.bullet_range = num("bulletRange");
            t.damage = num("damage");
            t.bullet_fall_off_range = num("bulletDamageFallOffRange");
            t.min_damage = num("minDamage");
            t.tracer_fire_per_round = num("tracerFirePerRound") as i32;
            // [game 007bb8e0] a part the data leaves out reads as 0 (the block's 1.0 defaults
            // from the gun loader 007bd0d0 are overwritten); every gun in the data has all four.
            let part = |name: &str| node.path(&["positionalDamageMultiplier", name]).and_then(Node::float).unwrap_or(0.0);
            t.positional = [part("head"), part("upperBody"), part("lowerBody"), part("vehicleBody")];
        }
        Kind::Launcher => {
            t.missile_start_speed = num("missileStartSpeed");
            t.missile_top_speed = num("missileTopSpeed");
            t.missile_acceleration = num("missileAcceleration");
            t.missile_lifetime = num("missileLifetime");
            t.explosive_damage = num("overwriteExplosiveDamage");
            t.explosive_min_damage = num("overwriteExplosiveMinDamage");
            t.lock_on_time = num("missileCrosshairLockOnTime");
            t.guided = node.get("guided").map_or(true, Node::is_true);
            t.lock_bracket_stay = num("lockOnBracketStayOnTime");
            t.lock_box = [num("missileLockOnBoxWidth"), num("missileLockOnBoxHeight")];
            t.guidance_delay = [num("minGuidanceDelay"), num("maxGuidanceDelay")];
            t.guidance_off_delay = [num("minGuidanceOffDelay"), num("maxGuidanceOffDelay")];
            t.guided_view_cos = num("guidedViewAngle").to_radians().cos();
            t.out_of_view_turn = num("outOfViewTurnSpeed").to_radians();
            t.accel_factor_delay = [num("minAccelFactorDelay"), num("maxAccelFactorDelay")];
            t.accel_factor = num("accelFactor");
            t.wobble_timer = [num("minWobbleTimer"), num("maxWobbleTimer")];
            t.wobble_angle = num("maxWobbleAngle").to_radians();
            t.wobble_duration = [num("minWobbleDuration"), num("maxWobbleDuration")];
            t.past_target_cos = num("pastTargetConeAngleActivated").to_radians().cos();
            t.hold_to_lock_on = node.get("mustHoldFireTriggerToLockOn").is_some_and(Node::is_true);
        }
        Kind::Flame => {
            t.flame_damage_per_second = num("damagePerSecond");
            t.flame_ticks_per_second = num("damageTicsPerSecond");
        }
        // The launcher's numbers are read by `turret::read_launcher`, not here. [game]
        Kind::Turret => {}
        Kind::Grenade => {
            t.player_noise_radius = num("playerNoiseRadius");
            t.explosive_damage = num("overwriteExplosiveDamage");
            t.explosive_min_damage = num("overwriteExplosiveMinDamage");
            let flag = |name: &str| node.get(name).map_or(attribute_default(name, kind) == Some(1.0), Node::is_true);
            t.grenade = GrenadeTuning {
                launch_arc_angle: num("launchArcAngle").to_radians(),
                lifetime: [num("grenadeMinLifetime"), num("grenadeMaxLifetime")],
                fire_towards_target: flag("fireTowardsTarget"),
                z_gravity: num("Zgravity"),
                is_proximity: flag("isProximity"),
                proximity_radius: num("proximityRadius"),
                proximity_delay: [num("proximityExplosionMinDelay"), num("proximityExplosionMaxDelay")],
                bounciness: num("bounciness"),
                dampening_on_collision: num("dampeningOnCollision"),
                launch_speed: num("launchSpeed"),
                tack_on_owner_speed: flag("tackOnOwnerSpeed"),
                is_sticky: flag("isSticky"),
                arms_on_collision: flag("proximityActivateOnCollision"),
                sticky_offset: num("stickyOffsetFromCollision"),
                sticky_lifetime: [num("stickyMinLifetime"), num("stickyMaxLifetime")],
                one_out_at_a_time: flag("oneOutAtATime"),
                velocity_orient: flag("velocityOrient"),
                spin_axis: node.get("spinAxis").map(Node::floats).filter(|v| v.len() >= 3).map_or(Vec3::Z, |v| Vec3::new(v[0], v[1], v[2])),
                random_spin_axis: flag("randomSpinAxis"),
                spin_speed: [num("minSpinSpeed").to_radians(), num("maxSpinSpeed").to_radians()],
            };
        }
    }
    if matches!(kind, Kind::Gun | Kind::Launcher | Kind::Grenade) {
        // [game 007bba30] each field 0 when absent (the loaders' 50 / 10 / 10 are overwritten).
        let info = |name| node.path(&["snakerInfo", name]).and_then(Node::float).unwrap_or(0.0);
        t.snaker = SnakerTuning {
            speed: info("speed"),
            damage: info("damage"),
            time_til_dies: info("timeTilDies"),
            damage_flags: node.path(&["snakerInfo", "damageFlags"]).and_then(Node::int).unwrap_or(0) as u32,
        };
    }
    // [game 007bca70] automatic unless the data says not.
    t.automatic_firing = node.get("automaticFiring").map_or(true, Node::is_true);
    let plain = |name| node.get(name).and_then(Node::float).or_else(|| attribute_default(name, kind)).unwrap_or(0.0);
    t.charge = ChargeTuning {
        has: node.get("hasChargedShot").is_some_and(Node::is_true),
        start: plain("timeTilChargeStarts"),
        duration: plain("chargeDurationTilMaxEffect"),
        hold: plain("chargeHoldDuration"),
        damage: [plain("minChargeDamageMultiplier"), plain("maxChargeDamageMultiplier")],
        heat: [plain("minHeatMeterIncrement"), plain("maxHeatMeterIncrement")],
        count: plain("chargeNumProjectilesMaxMultiplier").max(0.0) as u32,
        heat_rate: plain("heatMeterOverHeatRateWhileCharging"),
        heat_per_step: plain("chargeHeatIncrementPerMultiplierIncrement"),
    };
    // [game 007bbc30, 007bd0d0, 007bd4e0] the camera flag for every kind; the charge numbers
    // are the gun's (100, 0.5, 2 when absent), and stay at the gun's defaults for the others.
    t.snipe = crate::snipe::SnipeTuning {
        camera_enabled: node.get("snipeCameraEnabled").is_some_and(Node::is_true),
        max_damage: plain("maxSnipeDamage"),
        charge_start: plain("timeTilSnipeChargeStarts"),
        charge_duration: plain("snipeChargeDurationTilMaxEffect"),
    };
    t.vehicle_mode = node.get("isVehicleModeWeapon").is_some_and(Node::is_true);
    t.use_facing_as_target = node.get("useFacingAsTarget").is_some_and(Node::is_true);
    // Each entry prints as `[hash]` in the dump: an item, or an array of one. [data]
    t.children = node
        .get("weaponChildren")
        .into_iter()
        .flat_map(|list| list.items())
        .flat_map(|item| item.int().map_or_else(|| item.ints(), |h| vec![h]))
        .map(|h| h as u32)
        .filter(|&h| h != 0)
        .collect();
    t.parent = node.get("weaponParent").and_then(Node::int).map_or(0, |h| h as u32);
    t.alternating_fire = node.get("alternatingFire").is_some_and(Node::is_true);
    t.is_special = node.get("isSpecial").is_some_and(Node::is_true);
    // [data] every attribute block names it. [game 007bca70, 007bbc30] false when the data
    // leaves it out: the shared attribute ctor clears the byte at `+0x5c` (every kind's ctor
    // calls it first), and the loader 007bbc30 keeps the byte when the field is absent. The
    // game reads it from `+0x5c` in `weapon_listen_to_anim_event` 0074f300.
    t.listen_to_anim_event = node.get("listenToWeaponAnimEvent").is_some_and(Node::is_true);
    t.knock_back_speed = node.path(&["specialCaseDamageStuff", "knockBackSpeed"]).and_then(Node::float).unwrap_or(0.0);
    // [game 007bd4e0, 007bf380] a gun's flags start at 0x200, a grenade launcher's at 0xa00.
    t.damage_flags = node.get("damageFlags").and_then(Node::int).map_or(attribute_default("damageFlags", kind).unwrap_or(0.0) as u32, |f| f as u32);
    let special = |name| node.path(&["specialCaseDamageStuff", name]).and_then(Node::float).unwrap_or(0.0);
    t.knock_back_angle = special("knockBackAngle").to_radians();
    t.stun_time = special("stunTime");
    t.slow_time = special("slowTime");
    t.dot_time = special("dotTime");
    t.heat_damage = special("heatIncrement");
    t.missing = missing;
    t
}

/// The numbers of each weapon in `defs`, from the game's shared data, for a player's
/// weapons in a single-player game.
pub fn load(game_dir: &Path, defs: &[WeaponDef]) -> Result<Vec<WeaponTuning>, String> {
    load_for(game_dir, defs, true, false)
}

/// The numbers of each weapon in `defs` for an owner a player controls or not, in a single-
/// or multiplayer game (`WeaponDef::attribute_for`).
pub fn load_for(game_dir: &Path, defs: &[WeaponDef], by_player: bool, multiplayer: bool) -> Result<Vec<WeaponTuning>, String> {
    let pack = Pack::open(&game_dir.join("bnxglobal.str"))?;
    let files: Vec<DataFile> =
        pack.of_type(pack::DATA).filter_map(|chunk| DataFile::parse(pack.data(chunk).to_vec()).ok()).collect();
    defs.iter()
        .map(|def| {
            // [game 007bc9d0] a root the data lacks falls back to the kind's default.
            let root = def.attribute_for(by_player, multiplayer);
            let find = |root: u32| files.iter().find_map(|file| file.root().field(root));
            let node = find(root)
                .or_else(|| find(default_attribute(def.kind)))
                .ok_or_else(|| format!("bnxglobal.str has no weapon attributes {:#010x}", root))?;
            Ok(tuning_from(node, def.kind))
        })
        .collect()
}

/// What pulling the trigger did on one step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fired {
    Nothing,
    /// This many rounds left the weapon (each of `projectiles_per_fire` projectiles).
    Rounds(u32),
    /// The last of them filled the heat meter: it is locked until it has cooled.
    RoundsAndOverheated(u32),
    /// The trigger was pulled while it is overheated.
    Refused,
}

impl Fired {
    /// This and then `now`, added up: the rounds of both, overheated if either did.
    pub fn then(self, now: Fired) -> Fired {
        match (self, now) {
            (Fired::Rounds(a), Fired::Rounds(b)) => Fired::Rounds(a + b),
            (Fired::Rounds(a), Fired::RoundsAndOverheated(b)) => Fired::RoundsAndOverheated(a + b),
            (Fired::RoundsAndOverheated(a), Fired::Rounds(b) | Fired::RoundsAndOverheated(b)) => Fired::RoundsAndOverheated(a + b),
            (was, Fired::Nothing) => was,
            (Fired::Nothing | Fired::Refused, now) => now,
            (was, Fired::Refused) => was,
        }
    }
}

/// What a charged shot did in the updates of one `Weapon::step` (cleared at its top): the
/// charge began (`007adae0`, slot `+0x130`), it stepped (`007addc0`, `+0x134`: the level and
/// the level an update before), it reached full (`007b0520`, `+0x14c`), it was let go with this
/// level (`007af210`, `+0x138`), or it was cancelled with no shot (`007add20`, `+0x140`). [game]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChargeEvents {
    pub started: bool,
    pub step: Option<(f32, f32)>,
    pub full: bool,
    pub released: Option<f32>,
    pub cancelled: bool,
}

/// The pad's rumble of one charge step (`007addc0` step 2, a human owner only): for a weapon
/// whose count is under 2, or at full, effect 3 for the update at 1.0 when full, else effect 2
/// at 0.2 (`0x3e4ccccd`). (kind, seconds, strength). [game]
pub fn charge_rumble(c: &ChargeTuning, level: f32, dt: f32) -> Option<(u32, f32, f32)> {
    if dt <= 0.0 || !(c.count < 2 || level >= 1.0) {
        return None;
    }
    Some(if level >= 1.0 { (3, dt, 1.0) } else { (2, dt, 0.2) })
}

/// One weapon's state: its heat and when it may fire again.
#[derive(Clone, Copy, Debug)]
pub struct Weapon {
    pub heat: f32,
    /// Seconds left of the lock after overheating.
    pub overheated_left: f32,
    /// Seconds before the heat starts to fall (the weapon's word 0xf3).
    heat_wait: f32,
    /// Seconds since the last round (0xfb); a new weapon starts at 100000.
    since_round: f32,
    /// Seconds the trigger has been held on (0x106), and whether this pull has fired
    /// (0x107).
    pub held_for: f32,
    fired_this_pull: bool,
    /// Seconds before the refusal may sound again (0xd8).
    refused_wait: f32,
    was_down: bool,
    /// A charged shot is being charged (the weapon's byte `+0x40c`), and what the last
    /// charged firing multiplies a round's damage by.
    pub charging: bool,
    pub charge_damage: f32,
    /// The last firing was a full charge's (level 1 or more: fire flag 0x2000, `007af210`
    /// 007af23a-007af252), so its round is the `chargeMaxProjectileObj` when the count is
    /// under 2; a tap clears it. [game]
    pub charge_full: bool,
    /// What the charge did in this step (`ChargeEvents`).
    pub charge_events: ChargeEvents,
    /// The owner is in `StateAttackSpecial` with special type 4 (Megatron's): `007ac370` runs
    /// the cancel `007add20` in place of the charge update while the weapon charges. [game]
    charge_cancel: bool,
    /// The burst: firings of this press so far (the weapon's word `0xfd`, 0 when none is
    /// going) and its clock (`0xfe`, seconds). [game 007ae010, 007ac370]
    pub bursts_done: u32,
    burst_clock: f32,
    /// What lets a burst go on (`set_burst_context`): the owner's weapons are out (the
    /// weapon's byte `+0x38`), and he is in the drive or fly mode. [game 007ac370]
    weapons_out: bool,
    vehicle_motion: bool,
    /// The owner is dead (his `+0x148` bit 3, set by `character_kill` 007204a0 l.487 and
    /// `state_death_set_dead` 0087b070, cleared by the respawn `00881c00` / `008820e0`): a
    /// burst's round is not fired while it is set (`007ac370` l.41). [game 007ac370] Set by
    /// the caller through `set_owner_dead`; false until it is.
    owner_dead: bool,
    /// The snipe clock (the weapon's `+0x454`, `crate::snipe::clock_after`), whether its owner
    /// is sniping (`set_sniping`), and the steady the last firing was made with (taken before the
    /// shot sets the clock back, as `007ae010` asks slot `+0x160` first). [game]
    pub snipe_clock: f32,
    sniping: bool,
    pub snipe_steady_shot: f32,
    /// The kept fire sounds are let go in this step: an update took the branch that zeroes the
    /// time held (`007ac370` 007ac679-007ac712: trigger up, or this pull has not fired and the
    /// weapon is neither charged nor the lock launcher), which calls slot `+0x5c` =
    /// `gun_fire_sound_stop` 007a9d40, a key-off of the kept fire and ring-off sounds. [game]
    /// Cleared at the top of `step`; a firing in a later update of the same step clears it again,
    /// so it means "the last cut came after the step's last firing" (the caller starts its
    /// sounds before it keys off). [game 007ac370] The cut (slot `+0x5c`, with `+0x106` zeroed)
    /// comes at the end of the weapon update, after its burst firings (l.38-84) and after the
    /// trigger's firing in the caller, so a firing never follows a cut within one update; the
    /// case the old note worried about does not arise.
    pub sounds_cut: bool,
    /// The heat before the last firing's round was added: what the fire sound's `heat`
    /// parameter is set to (`007ae010` starts the sound at l.422 before slot `+0x100` adds the
    /// heat at l.502). [game]
    pub heat_at_fire: f32,
    /// Time not yet used up by whole updates of the game's.
    clock: f32,
}

impl Default for Weapon {
    fn default() -> Self {
        Self {
            heat: 0.0,
            overheated_left: 0.0,
            heat_wait: 0.0,
            since_round: 100_000.0,
            held_for: 0.0,
            fired_this_pull: false,
            refused_wait: 0.0,
            was_down: false,
            charging: false,
            charge_damage: 1.0,
            charge_full: false,
            charge_events: ChargeEvents::default(),
            charge_cancel: false,
            bursts_done: 0,
            burst_clock: 0.0,
            weapons_out: true,
            vehicle_motion: false,
            owner_dead: false,
            snipe_clock: 0.0,
            sniping: false,
            snipe_steady_shot: 0.0,
            sounds_cut: false,
            heat_at_fire: 0.0,
            clock: 0.0,
        }
    }
}

impl Weapon {
    /// [game] 007a8f20 divides the shot interval by slowFireRate. The heat increment
    /// uses slowHeatMeterIncrementRate; the weapon's update clock remains 0.032s.
    pub fn step_status(&mut self, t: &WeaponTuning, trigger: bool, slow: bool, effects: &crate::damage::Tuning, dt: f32) -> Fired {
        if !slow {return self.step(t,trigger,dt);}
        let mut scaled=t.clone();
        scaled.rate_of_fire /= effects.slow_fire_rate.max(1e-6);
        scaled.heat_per_round *= effects.slow_heat_rate;
        self.step(&scaled,trigger,dt)
    }
    /// [game] Damage flags4/5 add heat through the same capacity/cooldown path as firing.
    pub fn add_heat(&mut self, t: &WeaponTuning, heat: f32) {
        if heat<=0.0 {return;}
        self.heat += heat;
        if self.heat >= t.heat_capacity { self.overheated_left = t.overheat_period; self.heat_wait=0.0; }
        else { self.heat_wait = t.heat_cooldown_til_decrement; }
    }
    /// The heat meter, 0 to 1; full while locked.
    pub fn heat_fraction(&self, t: &WeaponTuning) -> f32 {
        if self.overheated_left > 0.0 { 1.0 } else { (self.heat / t.heat_capacity.max(1e-3)).clamp(0.0, 1.0) }
    }

    /// Whether the lock-on may run for a weapon whose trigger is held: it is ready (more
    /// than the rate of fire since its last round, `FUN_007ad080`) and its heat is under
    /// the capacity (slot `+0xf4` under 1). [game]
    pub fn may_lock(&self, t: &WeaponTuning) -> bool {
        self.since_round > t.rate_of_fire && self.heat < t.heat_capacity
    }

    /// A step of any length with the trigger held or not: the game's rules run in whole
    /// updates of its own (0.032 s), which is what spaces the rounds. What the updates in
    /// the step did, added up.
    pub fn step(&mut self, t: &WeaponTuning, trigger: bool, dt: f32) -> Fired {
        self.charge_events = ChargeEvents::default();
        self.sounds_cut = false;
        self.clock += dt;
        let mut all = Fired::Nothing;
        while self.clock >= crate::sim::GAME_UPDATE {
            self.clock -= crate::sim::GAME_UPDATE;
            all = all.then(self.update(t, trigger, crate::sim::GAME_UPDATE));
        }
        all
    }

    /// What lets a burst go on, given before each step: whether the owner's weapons are
    /// out (the weapon's byte `+0x38`: set for every weapon of his as the weapon or dodge
    /// state starts, `007b2b40` from `0087f4a0` / `0087a960`, cleared as he leaves them for
    /// another state, `007b2eb0` from `0087f6f0` / `0087abc0`, through the weapon's slot
    /// `+0x1c` = `007a75a0`), and whether he is in the drive or fly mode
    /// (`character_in_vehicle_motion` 0070ab90). [game]
    pub fn set_burst_context(&mut self, weapons_out: bool, vehicle_motion: bool) {
        self.weapons_out = weapons_out;
        self.vehicle_motion = vehicle_motion;
    }

    /// The owner's dead flag (`+0x148` bit 3), given before each step. [game 007ac370 l.41]
    pub fn set_owner_dead(&mut self, dead: bool) {
        self.owner_dead = dead;
    }

    /// Whether the owner is sniping (`camera_is_sniping` 0117e860) WITH THIS WEAPON IN HAND,
    /// given before each step: it runs the snipe clock and makes the steady. The caller folds in
    /// the clock's "may fire" (slot `+0x50` with 0, 0, 1 = `weapon_is_in_hand` 007a7470: not
    /// `isSpecial`, no `weaponParent`, control mode not 1, and it is the robot's weapon in hand).
    /// The steady (007ac920) keys on sniping alone; only the weapon in hand fires while
    /// sniping, so the two agree. [game 007ac370, 007ae010, 007a7470]
    pub fn set_sniping(&mut self, sniping: bool) {
        self.sniping = sniping;
    }

    /// Whether the owner is in `StateAttackSpecial` with special type 4 (Megatron's), given
    /// before each step: while it is, a charging weapon is cancelled (`007add20`) in place of
    /// its charge update. [game 007ac370]
    pub fn set_charge_cancel(&mut self, cancel: bool) {
        self.charge_cancel = cancel;
    }

    /// The steady as the weapon's slot `+0x128` gives it now (`crate::snipe::steady`). [game 007ac920]
    pub fn snipe_steady(&self, t: &WeaponTuning) -> f32 {
        crate::snipe::steady(self.snipe_clock, &t.snipe, self.sniping)
    }

    /// A firing's count in its burst (`007ae010` at 007aefc4 for the weapon's own firing, and
    /// at 007ae21d for its alternating child's turn): the count goes up, back to 0 with the
    /// clock once it reaches `burstPerFire`; then the gap is taken off the clock, which stays
    /// at 0 or more. [game] The gate [game 007ae010 l.20-56, 007ae059]: the count runs when the
    /// flag word (the weapon's `+0x340`, or'ed with `0x40` by the owner conditions) is nonzero
    /// and the owner's controller is not kind 2 (`scripted_health_controller` 00711260). The
    /// weapon's init 007ac270 always sets a kind bit in `+0x340` (0x10, 0x80, 0x100 or 0x200 by
    /// its projectile kind at `+0x3a0`), so for a robot's weapon the word is never 0 and every
    /// firing counts, as here. A scripted owner (kind 2) is not a robot the player drives.
    fn count_burst(&mut self, t: &WeaponTuning) {
        self.bursts_done += 1;
        if self.bursts_done >= t.burst_per_fire {
            self.bursts_done = 0;
            self.burst_clock = 0.0;
        }
        self.burst_clock = (self.burst_clock - t.time_between_bursts).max(0.0);
    }

    /// One firing: the time since the last round goes to 0, the pull has fired, a round's
    /// heat (`007ae010` 007af0b1: `param_1[0xfb] = 0`, slot `+0x100` = `weapon_add_heat`
    /// 007a90c0) and its count in the burst. [game]
    fn firing(&mut self, t: &WeaponTuning) -> Fired {
        // The round's steady is read first (slot `+0x128` at 007ae3f0, `+0x160` at 007ae774), and
        // a shot made while sniping sets the clock back (007af157-007af17d). [game]
        self.snipe_steady_shot = self.snipe_steady(t);
        if self.sniping {
            self.snipe_clock = crate::snipe::clock_after_shot(&t.snipe);
        }
        self.since_round = 0.0;
        self.fired_this_pull = true;
        // An ordinary round is never charged: 007af210 computes the charged multiplier. [game] An
        // ordinary round's multiplier is the gun vtable's slot `+0x160` (00b5fba0 + 0x160 = 007ac760,
        // read 2026-10-20): the snipe ramp while sniping, else the type-6 special's damage multiplier
        // while it runs (0079b770: the damage tuning's +0xa8 in single player), else 1.0. `game\src\
        // weapons.rs` sends it so. [game 007ac760, 0079b770] History: `007ae010` l.360 calls `bullet_trace_setup` (00784080) with the
        // round's aim, the spawn, and the `+0x160` value as its fifth argument. 00784080 hands
        // that on to `gun_round_trace` 007832b0 as `param_5`, stored at the round's `+0x5c` (the
        // steady). CORRECTED 2026-10-20: `bullet_damage` 00782b30 reads the multiplier from the
        // round record's own `+0x5c` (l.55: `(rec+0x58 +0x13c) * rec+0x5c`); its third argument
        // is the output packet, not the multiplier. [game 007ae010, 00784080, 007832b0, 00782b30]
        // The `+0x160` is a virtual call (007ae774: `(*param_1)+0x160`, slot 88) on the gun itself.
        (self.charge_damage, self.charge_full) = (1.0, false);
        self.count_burst(t);
        self.heat_at_fire = self.heat;
        self.sounds_cut = false;
        self.heat += t.heat_per_round;
        if self.heat < t.heat_capacity {
            self.heat_wait = t.heat_cooldown_til_decrement;
            Fired::Rounds(1)
        } else {
            self.overheated_left = t.overheat_period;
            self.heat_wait = 0.0;
            Fired::RoundsAndOverheated(1)
        }
    }

    /// One update of the game's. [game]
    ///
    /// The trigger, as the weapon state's update gives it to the weapon manager each update
    /// (`FUN_00878740`: held, `FUN_007b37f0`; let go that update, `FUN_007b3a20`), which
    /// calls the weapon (`FUN_007aca50` held, `FUN_007acb90` let go):
    ///
    /// - Held: a round leaves if the weapon can fire (`FUN_007a8da0`: its heat is under
    ///   `heatMeterCapacity`, which it is not all through the overheat period) and is ready
    ///   (`FUN_007ad080`: MORE than `rateOfFire` since the last round; and, unless
    ///   `automaticFiring`, the trigger has not been held on since it fired). So at 0.032 s
    ///   an update his cannon's 0.15 comes to a round every fifth update, 0.16 s.
    /// - A launcher with `mustHoldFireTriggerToLockOn` (his missiles) does nothing while
    ///   held: its round leaves on the update the trigger is let go, if it had been held.
    /// - With the heat at the capacity the refusal sounds instead, and again every
    ///   `rateOfFire` while that goes on (`FUN_007a9dd0`, its wait in the weapon's 0xd8).
    ///
    /// Then the weapon's own update (`FUN_007ac370`): the time since the last round and the
    /// time held go up (the latter only once this pull has fired, or for the launcher above;
    /// it is zeroed with the trigger up), and the heat (`FUN_007a9700`; a round's is
    /// `FUN_007a90c0`, once a firing whatever the number of projectiles): a round adds
    /// `heatMeterIncrementPerRound`; under the capacity that restarts a wait of
    /// `heatMeterCooldownTilDecrement`, after which the heat falls by
    /// `heatMeterDecrementPerSecond`; the round that reaches the capacity starts
    /// `heatMeterOverHeatPeriod` instead, and when that has run the heat is put to zero,
    /// because `heatMeterOverHeatCooldownRate` is not positive for any of his weapons
    /// (where it is, the heat stays and falls at that rate).
    ///
    /// Bursts (`burstPerFire` over 1, none of his) are below; the charged shot is `charge`
    /// ([game 007af210] a charged firing does not count in a burst: the charged shot never reads
    /// or writes the burst fields (`+0xfd`, `+0xfe`) and does not call `007ae010`). Not in: alternating barrels, the upgrades that scale the rate, a round's heat and the
    /// period (levels all 0), and the weapon's slot `+0xc0` (its byte `+0x40c`, set
    /// while a charged shot is being charged: `FUN_007b01a0`), during which the heat does
    /// not fall: none of his weapons charges. [game 0071dc20 l.81, l.137] The character update
    /// runs the state object (`char+0x314` slot 1, l.81) before the weapon manager 007b1850
    /// (l.137), so the state's update comes first. [assumed: the weapon's own update is called
    /// inside 007b1850 after the part read (its first 60 lines); the launcher's release could not
    /// fire otherwise]
    /// settles: the weapon update's call site in 007b1850 past l.60 (`fn.py 007b1850 --lines 60-200`).
    fn update(&mut self, t: &WeaponTuning, trigger: bool, dt: f32) -> Fired {
        let let_go = !trigger && self.was_down;
        self.was_down = trigger;
        let on_release = t.hold_to_lock_on;
        let full = t.heat_capacity <= self.heat;
        let fresh = t.automatic_firing || self.held_for == 0.0;
        // A charged weapon fires nothing while held (`007aca50` fires slot `+0xcc` only when
        // the attributes' `+0x62`, `hasChargedShot`, is 0); let go (`007acb90`) it refuses when
        // it had been held and its heat is full, else a TAP fires the ordinary round: the time
        // held as the charge counts it is not past `timeTilChargeStarts` (past it, the charge
        // update below fires instead). Its ready test asks no fresh press (`007ae010`). [game]
        let charged = t.charge.has && !on_release;
        let asked = if on_release || charged { let_go && self.held_for > 0.0 } else { trigger };
        let tap = !charged || self.charge_clock(t) <= t.charge.start;
        let mut fired = Fired::Nothing;
        if asked && full {
            if (on_release || fresh || charged) && self.refused_wait <= 0.0 {
                self.refused_wait = t.rate_of_fire;
                fired = Fired::Refused;
            }
        } else if asked && tap && self.since_round > t.rate_of_fire && (on_release || fresh || charged) {
            fired = self.firing(t);
        }

        // The snipe clock, first in the weapon's own update (`007ac370` l.30-40): it runs while
        // sniping with this weapon in hand (slot `+0x50` with 0, 0, 1 = `weapon_is_in_hand`
        // 007a7470), which the caller folds into `set_sniping`. The heat plays no part. [game]
        self.snipe_clock = crate::snipe::clock_after(self.snipe_clock, self.sniping, dt);

        // The burst (`007ac370` l.38-84, before the charge and the clocks): while the count
        // is above 0 the update goes on its clock, and once that has reached
        // `timeBetweenEachBurst` the weapon fires again by itself (slot `+0xcc` = `007ae010`
        // with its "forced" argument 1, so neither the rate of fire nor the heat is tested,
        // and the trigger is not needed). It goes on if the weapon `isSpecial`, or for an
        // `isVehicleModeWeapon` while he is in the drive or fly mode, or for another while his
        // weapons are out; else the count goes to 0 and the rest of the burst is dropped.
        // [game] So Ironhide's laser fires its second burst of 5 four updates (0.128 s) after
        // the press, on the left cannon's turn. The owner's `+0x148` bit 3 is the dead flag: the
        // burst's round is not fired while it is set (`007ac370` l.41, the whole fire block;
        // the clock still counts and the count is kept). [game 007ac370, 007204a0, 0087b070,
        // 00881c00, 008820e0] The caller sets it with `set_owner_dead`.
        if self.bursts_done > 0 {
            self.burst_clock += dt;
            if t.time_between_bursts <= self.burst_clock && !self.owner_dead {
                let goes_on = t.is_special || if t.vehicle_mode { self.vehicle_motion } else { self.weapons_out };
                if goes_on {
                    fired = fired.then(self.firing(t));
                } else {
                    self.bursts_done = 0;
                }
            }
        }

        if t.charge.has {
            if self.charge_cancel && self.charging {
                self.cancel_charge();
            } else {
                fired = self.charge(t, trigger, fired, dt);
            }
        }
        if self.refused_wait > 0.0 {
            self.refused_wait -= dt;
        }
        self.since_round += dt;
        // A charged weapon's time held goes on while the trigger is down though nothing has
        // fired (`007ac370` l.117: the reset wants "not fired this pull AND not charged AND not
        // the lock launcher"). [game]
        if !trigger || !(self.fired_this_pull || on_release || charged) {
            self.held_for = 0.0;
            self.fired_this_pull = false;
            // The same branch lets the kept fire sounds go (slot `+0x5c`). [game 007ac370]
            self.sounds_cut = true;
        } else {
            self.held_for += dt;
        }
        if self.overheated_left > 0.0 {
            self.overheated_left -= dt;
            if self.overheated_left <= 0.0 {
                self.overheated_left = 0.0;
                self.heat = 0.0;
            }
        } else if self.heat_wait > 0.0 {
            self.heat_wait -= dt;
        } else if self.heat > 0.0 && !self.charging {
            self.heat = (self.heat - t.heat_decrement_per_second * dt).max(0.0);
        }
        fired
    }

    /// How far the charge is, as the reticle asks (`FUN_007b0b00`): -1 for a weapon with
    /// no charged shot, else 0 to 1. [game]
    pub fn charge_level(&self, t: &WeaponTuning) -> f32 {
        if !t.charge.has {
            return -1.0;
        }
        let held = self.charge_clock(t) - t.charge.start;
        if held <= t.charge.duration { (held / t.charge.duration.max(1e-6)).max(0.0) } else { 1.0 }
    }

    /// The time held as the charge counts it: while the weapon is still inside its
    /// rate of fire, less the time since its last round (`FUN_007ad1c0` 007ad24c). [game]
    fn charge_clock(&self, t: &WeaponTuning) -> f32 {
        if t.rate_of_fire >= self.since_round { self.held_for - self.since_round } else { self.held_for }
    }

    /// The charged shot's update (`FUN_007ad1c0`, called by the weapon's own update before
    /// its clocks go on), `notes\weapon-charged-shot.md`. [game] Past
    /// `timeTilChargeStarts` of holding, the charge runs while the trigger is down (and
    /// `chargeHoldDuration`, where positive, has not run out past full); the update the
    /// trigger is found up, it fires: `floor((count - 1) * level) + 1` firings at once
    /// for a weapon whose count is 2 or more (damage unchanged, a round's heat), else one
    /// whose damage and heat run from the least to the most with the level. While it
    /// charges the heat rises: by `heat_per_step` each time a firing is gained, by
    /// `heat_rate` a second (all the time for a count under 2, at full otherwise). A
    /// weapon that cannot fire loses its charge.
    /// The update order of `007ad1c0` for the player (its third argument is "the owner's
    /// controller is scripted", not the trigger; the trigger is slot `+0x94`): nothing before
    /// `timeTilChargeStarts`; the full notice (`+0x14c`) where the level crosses 1, even on the
    /// update of the release; then, with the trigger down and the hold not run out, the start
    /// (`+0x130`, nothing else that update) or a step (`+0x134`: heat, emitter scale, rumble);
    /// and **unless the trigger is down and the heat under the capacity it fires**: a
    /// release, a hold run out, or a step's heat reaching the capacity while still held.
    /// [game 007ad1c0, 007ad26a-007ad27a] A weapon with a parent returns at once (the
    /// arsenal runs the child through `charge_step_heat` / `charge_release`, driven by its
    /// parent). [game 007add20] The cancel (`weapon_charge_cancel`) clears the charging flag
    /// (`+0x103`) and the time held (`+0x106`, the port's `held_for`), stops the emitter and
    /// calls the cancel event (`+0x150`). That is NOT what a full heat does: with the weapon
    /// unable to fire (slot `+0x8c`, `007a8da0`: heat at the capacity) the update only zeroes the
    /// time held (`+0x106`) and returns, with no cancel event and the charging flag (`+0x103`)
    /// left as it was [game 007ad1c0 l.13-27], so the next charge goes on with steps, not a new
    /// start. The port's `overheated_left` test adds nothing to the heat test: every overheat starts
    /// with the heat at or over the capacity (`step`, `charge_fire`, `add_heat`) and the heat neither
    /// decays nor drops while it runs (`update`), so it is 007a8da0's heat test [game]. 007a8da0's other
    /// way to fire when full, a weapon with a parent (vslot `+0xdc`, the slot 007a7470 tests), never
    /// reaches here: a weapon with a parent returned at the top (007ad26a-007ad27a). [game]
    fn charge(&mut self, t: &WeaponTuning, trigger: bool, fired: Fired, dt: f32) -> Fired {
        let c = &t.charge;
        if self.heat >= t.heat_capacity || self.overheated_left > 0.0 {
            self.held_for = 0.0;
            return fired;
        }
        let held = self.charge_clock(t);
        if held <= c.start {
            return fired;
        }
        let rate = 1.0 / c.duration.max(1e-6);
        let level = ((held - c.start) * rate).min(1.0);
        let before = ((held - c.start - dt) * rate).min(1.0);
        if before < 1.0 && level >= 1.0 {
            self.charge_events.full = true;
        }
        let held_too_long = level >= 1.0 && c.hold > 0.0 && c.hold < held - (c.duration + c.start);
        let trigger = trigger && !held_too_long;
        if trigger {
            if !self.charging {
                self.charging = true;
                self.charge_events.started = true;
            } else {
                self.charge_events.step = Some((level, before));
                self.charge_step_heat(t, level, before, dt);
            }
            if self.heat < t.heat_capacity {
                return fired;
            }
        }
        fired.then(self.charge_release(t, level))
    }

    /// A charge step's heat (`007addc0` step 4, through the round's add-heat `007a90c0`, so it
    /// restarts the cooldown and can overheat): a count under 2 adds
    /// `heatMeterOverHeatRateWhileCharging` a second; otherwise
    /// `chargeHeatIncrementPerMultiplierIncrement` once for each firing gained, else the rate
    /// only at full. [game] Public so that a child is stepped with its parent's level.
    pub fn charge_step_heat(&mut self, t: &WeaponTuning, level: f32, before: f32, dt: f32) {
        let c = &t.charge;
        if dt <= 0.0 {
            return;
        }
        if c.count < 2 {
            self.add_heat(t, c.heat_rate * dt);
        } else {
            let steps = |x: f32| ((c.count - 1) as f32 * x) as i32;
            if steps(level) > steps(before) {
                self.add_heat(t, c.heat_per_step);
            } else if level >= 1.0 {
                self.add_heat(t, c.heat_rate * dt);
            }
        }
    }

    /// The charged firing (`007af210`, slot `+0x138`) at `level`: `int((count-1) * level) + 1`
    /// firings for a count of 2 or more (damage 1, a round's heat), else one whose damage
    /// multiplier and heat run from the least to the most with the level; fire flag 0x2000
    /// (`charge_full`) at level 1 or more. The time since the last round and the time held go
    /// to 0. [game 007af210 l.123-137, 007af23a-007af252, l.378] Public so that a child fires
    /// with its parent's level.
    pub fn charge_release(&mut self, t: &WeaponTuning, level: f32) -> Fired {
        let c = &t.charge;
        self.charging = false;
        let (rounds, heat) = if c.count < 2 {
            self.charge_damage = c.damage[0] + (c.damage[1] - c.damage[0]) * level;
            (1, c.heat[0] + (c.heat[1] - c.heat[0]) * level)
        } else {
            self.charge_damage = 1.0;
            (((c.count - 1) as f32 * level) as u32 + 1, t.heat_per_round)
        };
        self.charge_full = level >= 1.0;
        self.charge_events.released = Some(level);
        self.heat += heat;
        (self.held_for, self.since_round) = (0.0, 0.0);
        self.fired_this_pull = false;
        if self.heat < t.heat_capacity {
            self.heat_wait = t.heat_cooldown_til_decrement;
            Fired::Rounds(rounds)
        } else {
            self.overheated_left = t.overheat_period;
            self.heat_wait = 0.0;
            Fired::RoundsAndOverheated(rounds)
        }
    }

    /// The cancel (`007add20`, slot `+0x140`): the charge ends with no shot, its emitter and
    /// loop stop, the count goes to 0 (the port's time held). [game]
    pub fn cancel_charge(&mut self) {
        if self.charging {
            self.charge_events.cancelled = true;
        }
        (self.charging, self.held_for) = (false, 0.0);
    }
}

/// A flamethrower's flame while the trigger is held (`FUN_0074c200`,
/// `notes\weapon-flamethrower.md`). [game] Its volumes come on one after another, each
/// `delayTilTurningOnNextDummy` after the one before; a tick falls 1 s after the start
/// and then every `1 / damageTicsPerSecond`.
/// [game 0074d610] THE DAMAGE is the weapon's contact handler (a listener at weapon `+0x45c`,
/// callback thunk `0074f000`): `damagePerSecond / damageTicsPerSecond` per hit
/// (0074dbe1-0074dbe9), at once on contact, then at most one hit per victim per
/// `1 / damageTicsPerSecond` (the hit list `+0x42c`: `FlameHits`); the packet is
/// `flame_packet`. The tick here only renews the packet serial and the HUD hit message.
#[derive(Clone, Copy, Debug, Default)]
pub struct Flame {
    /// Seconds it has burned, 0 while off.
    pub on_for: f32,
    /// Seconds to the next tick (the weapon's `+0x43c`).
    tick: f32,
    clock: f32,
    /// The game's updates the last `step` took.
    updates: u32,
}

impl Flame {
    pub fn burning(&self) -> bool {
        self.on_for > 0.0
    }

    /// How many of the weapon's spheres are lit.
    pub fn lit(&self, def: &WeaponDef) -> usize {
        if !self.burning() {
            return 0;
        }
        let mut at = 0.0;
        let mut lit = 0;
        for sphere in &def.flame[..def.flame_count] {
            if self.on_for - crate::sim::GAME_UPDATE < at - 1e-4 {
                break;
            }
            lit += 1;
            at += sphere.delay_to_next;
        }
        lit
    }

    /// Runs it by `dt` in whole updates of the game's; the number of damage ticks that fell.
    pub fn step(&mut self, t: &WeaponTuning, firing: bool, dt: f32) -> u32 {
        if !firing {
            *self = Self::default();
            return 0;
        }
        let mut ticks = 0;
        self.updates = 0;
        self.clock += dt;
        while self.clock >= crate::sim::GAME_UPDATE {
            self.clock -= crate::sim::GAME_UPDATE;
            self.updates += 1;
            if self.on_for == 0.0 {
                self.tick = 1.0;
            }
            self.on_for += crate::sim::GAME_UPDATE;
            self.tick -= crate::sim::GAME_UPDATE;
            if self.tick < 0.0 {
                self.tick = 1.0 / t.flame_ticks_per_second.max(1e-3);
                ticks += 1;
            }
        }
        ticks
    }

    /// What one hit takes from a target: `damagePerSecond / damageTicsPerSecond`.
    /// [game 0074dbe1 movss attr+0x130, 0074dbe9 divss attr+0x134]
    pub fn tick_damage(t: &WeaponTuning) -> f32 {
        t.flame_damage_per_second / t.flame_ticks_per_second.max(1e-3)
    }

    /// How many of the game's updates the last `step` took (0 when off): the flame's hit list
    /// ages and its contacts are tested once per update.
    pub fn updates_taken(&self) -> u32 {
        self.updates
    }

    /// How far the flame's impact ray reaches: to the world place of the current fire dummy, the
    /// last lit one (the weapon's list at `+0x420`, index `+0x404` held to the list's end; its
    /// object `+0xc` and node `+0x14` into `FUN_0056bee0`, which gives the node's world place),
    /// from the weapon's slot-36 origin. [game 0074c200 at 0074ca46-0074caba: that place is the
    /// SECOND POINT of `world_ray_sphere_query` (start, end, mask 3, results, radius 0)] The port
    /// measures it from the muzzle to the lit sphere's place (`FlameSphere::at`, the dummy's);
    /// [game 0074ca46-0074caba] Read: the ray's end is the world place of the dummy at the held
    /// index (`+0x404`, walked along the list `+0x424`), the lit sphere's place; the start is the
    /// slot-36 origin (as read earlier, the first argument pair not re-read here). So the reach is the length of that ray: the muzzle to the lit
    /// sphere. 0 when none is lit.
    ///
    /// What the ray may put the impact effect on [game 0074cad6-0074cb2c]: the hits in order of
    /// distance, skipping any object with a `Character` (0070a3c0) and any Destructable flagged
    /// invulnerable (`+0x134 & 0x1000`); the first other one takes it. So the effect lands on the
    /// world behind or under the target, never on a character.
    pub fn reach(&self, def: &WeaponDef) -> f32 {
        match self.lit(def) {
            0 => 0.0,
            lit => def.flame[lit - 1].at.distance(def.muzzle),
        }
    }
}

/// A flamethrower's victims: the weapon's hit list (`+0x42c`, entries {victim, timer}). Each
/// flamethrower object has its own, so each of Long Haul's two flames hurts on its own.
/// [game 0074c200 lines 153-208, 0074d610, 0074d110]
#[derive(Clone, Debug, Default)]
pub struct FlameHits(Vec<(u32, f32)>);

impl FlameHits {
    /// Each update while it burns, before the contacts: an entry whose timer is under 0 is
    /// dropped, any other loses `dt`. [game 0074c200, inside the firing block] (The game also
    /// drops a dead victim's entry; a dead target is not touched here, so it is not needed.)
    /// [assumed] the list ages before the contacts of the same update. [game 0074c200] Read: the
    /// update walks the hit list (`+0x10c`) and takes `dt` off each entry's timer (`+0x10`),
    /// dropping the entries whose owner is gone; the contact handler 0074d610 skips an entry
    /// whose timer is above 0. 2026-10-20: `0074c200` is slot 0 (`vt+0x0`) of the flamethrower
    /// vtable `00b5daf0` (`pe_read vtable`), and the contact `0074d610` is in no vtable and has no
    /// literal reference (`find_ptr`, `callers` empty): its caller is not found. settles: the
    /// walk that calls `vt+0x0` on the flamethrower and the contact's caller, and whether the
    /// physics contacts of an update run before or after `0074c200` (the world step; not read).
    pub fn age(&mut self, dt: f32) {
        self.0.retain_mut(|(_, timer)| {
            if *timer < 0.0 {
                false
            } else {
                *timer -= dt;
                true
            }
        });
    }

    /// A contact with `victim`: whether it may be hurt now (no entry, or its timer is up).
    /// [game 0074d610: an entry with timer > 0 stops the handler]
    pub fn may_hit(&self, victim: u32) -> bool {
        self.0.iter().find(|(v, _)| *v == victim).is_none_or(|&(_, timer)| timer <= 0.0)
    }

    /// The hit was taken (packet `+0x71` set by the receiver): a new entry at
    /// `1 / damageTicsPerSecond`, or the old one's timer `+= 1 / damageTicsPerSecond`, the
    /// remainder kept (so it averages exactly that many hits a second). [game 0074d610, 0074efa0]
    pub fn taken(&mut self, victim: u32, t: &WeaponTuning) {
        let step = 1.0 / t.flame_ticks_per_second.max(1e-3);
        match self.0.iter_mut().find(|(v, _)| *v == victim) {
            Some((_, timer)) => *timer += step,
            None => self.0.push((victim, step)),
        }
    }

    /// The stop (`flame_set_firing(0)` 0074d110) frees the whole list (`007e1b60`), so a new
    /// press hits again at once. [game]
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// The flame's damage packet, as its contact handler builds it [game 0074d610, 0074db45-0074df90]:
/// amount `damagePerSecond / damageTicsPerSecond`; flags `damageFlags & 0xf27fffff` (a negative
/// `damageFlags`, all bits, is read as 0xffffffff by the loader's cast); the special block
/// (stun, slow, dot, heat, knock-back) as the weapon's, the heat divided by
/// `damageTicsPerSecond` when OVERHEAT_CURRENT or OVERHEAT_ALL is set (0074ddcb); the direction
/// flat, from the weapon's object to the victim's (z 0). `source` is filled in where the hit is
/// resolved, as for every weapon.
pub fn flame_packet(t: &WeaponTuning, from: Vec3, victim: Vec3) -> crate::damage::Packet {
    use crate::damage::flags;
    let direction = (victim - from).truncate().normalize_or_zero().extend(0.0);
    let mut p = crate::damage::Packet::bullet(t, Flame::tick_damage(t), direction);
    p.flags = t.damage_flags & 0xf27f_ffff;
    if p.flags & (flags::OVERHEAT_CURRENT | flags::OVERHEAT_ALL) != 0 {
        p.heat /= t.flame_ticks_per_second.max(1e-3);
    }
    p
}

/// A number drawn evenly between two (`FUN_00551d60`), from a source of 0 to 1.
fn between(range: [f32; 2], random: &mut dyn FnMut() -> f32) -> f32 {
    range[0] + (range[1] - range[0]) * random()
}

/// A missile in flight: the launch (`FUN_00785de0`) and the update (`FUN_007847e0`). [game]
///
/// It leaves along the muzzle at `missileStartSpeed` and lasts `missileLifetime` (a timer
/// set at the launch). Each update of the game's:
///
/// - Once the guidance delay has run, it steers at a point: the middle of its target
///   (the target's place + its up axis * half its height) while it has one, the weapon is
///   `guided` and its own guidance is on; else the point it was launched at (the aim
///   point; for a weapon that is not guided, 10000 along the line to it). With c the
///   cosine between its way and the point: inside `guidedViewAngle` it points straight at
///   the point, at once; outside it turns toward it by `outOfViewTurnSpeed`.
/// - Once the point has been within `pastTargetConeAngleActivated` of its way, a point
///   that is no longer ahead (c <= 0) ends the guidance: it flies straight on.
/// - The wobble: after a wait drawn from `minWobbleTimer`..`maxWobbleTimer`, for a time
///   drawn from `minWobbleDuration`..`maxWobbleDuration`, it turns each update about an
///   axis drawn at random (each part -1 to 1, not made unit) by that axis's length *
///   a rate drawn from 0..`maxWobbleAngle`, either way * the update's time; a turn under
///   0.01 radians is not made. It does not steer while it wobbles.
/// - Speed: + `missileAcceleration` a second while the delay drawn from
///   `minAccelFactorDelay`..`maxAccelFactorDelay` runs, then `accelFactor` times that, up
///   to `missileTopSpeed`.
///
/// The game's update (missile_update 007847e0) only sets the body's velocity (l.242-249: speed
/// with the cap at +0x13c, physics_body_set_velocity) and its orientation (l.250-261); it moves
/// nothing and tests no lifetime. The physics step moves the body, and the lifetime's end is a
/// delayed message (post_delayed_message 005a9800; missile_launch 00785de0 does not send it)
/// [game, wave 28 P3]. [assumed: moving by the new velocity right after the update, with the
/// lifetime tested before it, gives the same path] settles: the sender of that delayed message and
/// where the physics step falls against the missile update.
/// Not in: the guidance ending when the target's damage object's
/// `+0x94` is over 0 (not read), `minGuidanceOffDelay` drawn anew (his is -1: never),
/// `twirl` (his is false), `stealthGuided`, the damage upgrade (`FUN_0079b770`).
#[derive(Clone, Copy, Debug)]
pub struct Missile {
    pub position: Vec3,
    pub direction: Vec3,
    pub speed: f32,
    pub age: f32,
    /// What it was locked on at the launch.
    pub target: Option<u32>,
    /// The point it steers at while it has no target to steer at (`+0x3c`).
    point: Vec3,
    /// Its guidance is on (`+0x80`), and the point has been inside the cone (`+0x81`).
    pub guided: bool,
    armed: bool,
    /// Seconds before it steers (`+0x74`), before it gives up steering (`+0x78`, negative
    /// for never) and before it gathers speed faster (`+0x7c`).
    guidance_wait: f32,
    off_wait: f32,
    accel_wait: f32,
    /// The wobble: seconds to the next (`+0x5c`), seconds left of this one (`+0x60`), and
    /// its axis times its rate (`+0x64`, `+0x70`).
    wobble_wait: f32,
    wobble_left: f32,
    wobble: Vec3,
    /// Time not yet used up by whole updates of the game's.
    clock: f32,
}

impl Missile {
    /// A missile leaving `position` along `forward`, sent at `point` and locked on
    /// `target`. `random` gives numbers from 0 to 1.
    pub fn launch(
        position: Vec3,
        forward: Vec3,
        point: Vec3,
        target: Option<u32>,
        t: &WeaponTuning,
        random: &mut dyn FnMut() -> f32,
    ) -> Self {
        let direction = forward.normalize_or(Vec3::Y);
        let point = if t.guided { point } else { position + (point - position).normalize_or(direction) * 10_000.0 };
        let guidance_wait = between(t.guidance_delay, random);
        let accel_wait = between(t.accel_factor_delay, random);
        Self {
            position,
            direction,
            speed: t.missile_start_speed,
            age: 0.0,
            target,
            point,
            guided: true,
            armed: false,
            guidance_wait,
            off_wait: -1.0,
            accel_wait,
            wobble_wait: 0.0,
            wobble_left: 0.0,
            wobble: Vec3::ZERO,
            clock: 0.0,
        }
    }

    /// Ends the guidance (`FUN_00785690`): it keeps the way it has (`straight`), or the
    /// way to its target's middle as it is now.
    fn guidance_off(&mut self, straight: bool, target_at: Option<Vec3>) {
        self.guided = false;
        let way = match target_at {
            Some(at) if !straight => (at - self.position).normalize_or(self.direction),
            _ => self.direction,
        };
        self.point = self.position + way * 10_000.0;
        self.target = None;
    }

    /// Moves it on by `dt` in whole updates of the game's and returns the stretch it
    /// covered, or `None` once its time is up. `target_at` is the middle of its target, if
    /// that is still there.
    pub fn step(
        &mut self,
        t: &WeaponTuning,
        target_at: Option<Vec3>,
        random: &mut dyn FnMut() -> f32,
        dt: f32,
    ) -> Option<(Vec3, Vec3)> {
        let from = self.position;
        self.clock += dt;
        while self.clock >= crate::sim::GAME_UPDATE {
            self.clock -= crate::sim::GAME_UPDATE;
            self.age += crate::sim::GAME_UPDATE;
            if self.age > t.missile_lifetime {
                return None;
            }
            self.update(t, target_at, random, crate::sim::GAME_UPDATE);
        }
        Some((from, self.position))
    }

    fn update(&mut self, t: &WeaponTuning, target_at: Option<Vec3>, random: &mut dyn FnMut() -> f32, dt: f32) {
        let target_at = target_at.filter(|_| self.target.is_some());
        self.wobble_left -= dt;
        if self.wobble_left <= 0.0 {
            self.wobble_left = 0.0;
            self.wobble_wait = (self.wobble_wait - dt).max(0.0);
        }
        self.guidance_wait -= dt;
        if self.guidance_wait <= 0.0 {
            self.guidance_wait = 0.0;
            if self.guided && self.off_wait < 0.0 && t.guidance_off_delay[0] >= 0.0 {
                self.off_wait = between(t.guidance_off_delay, random);
            }
        }
        if self.off_wait >= 0.0 && self.guided {
            self.off_wait -= dt;
            if self.off_wait <= 0.0 {
                self.off_wait = 0.0;
                self.guidance_off(false, target_at);
            }
        }
        self.accel_wait = (self.accel_wait - dt).max(0.0);

        let mut turned = Vec3::ZERO;
        if self.guidance_wait <= 0.0 {
            let point = match target_at {
                Some(at) if t.guided && self.guided => at,
                _ => self.point,
            };
            if let Some(to) = (point - self.position).try_normalize() {
                let c = to.dot(self.direction);
                if t.guided && !self.armed && c >= t.past_target_cos {
                    self.armed = true;
                }
                if self.armed && self.guided && c <= 0.0 {
                    self.guidance_off(true, target_at);
                }
                if self.wobble_left <= 0.0 {
                    if t.guided_view_cos < c {
                        self.direction = to;
                    } else if c > -0.999 && c < 0.999 {
                        let angle = c.acos().min(t.out_of_view_turn * dt);
                        let axis = self.direction.cross(to).normalize_or(Vec3::Z);
                        self.direction = (glam::Quat::from_axis_angle(axis, angle) * self.direction).normalize();
                    }
                }
            }
            if self.wobble_left <= 0.0 && self.wobble_wait <= 0.0 && t.wobble_timer[0] > 0.0 {
                // A new wobble (`FUN_00786440`).
                self.wobble_wait = between(t.wobble_timer, random);
                self.wobble_left = between(t.wobble_duration, random);
                let axis = Vec3::new(random() * 2.0 - 1.0, random() * 2.0 - 1.0, random() * 2.0 - 1.0);
                let side = if random() < 0.5 { -1.0 } else { 1.0 };
                self.wobble = axis * side * between([0.0, t.wobble_angle], random);
            }
            if self.wobble_left > 0.0 {
                turned = self.wobble * dt;
            }
        }
        if turned.length_squared() > 0.0001 {
            let angle = turned.length();
            self.direction = (glam::Quat::from_axis_angle(turned / angle, angle) * self.direction).normalize();
        }

        let gain = if self.accel_wait > 0.0 { t.missile_acceleration } else { t.accel_factor * t.missile_acceleration };
        self.speed = (self.speed + gain * dt).min(t.missile_top_speed);
        self.position += self.direction * self.speed * dt;
    }
}

/// A grenade in flight (`GrenadeProjectile`: launch `FUN_00789b80`, arm `FUN_00786a60`,
/// update `FUN_00786ff0`, contact `FUN_007876b0`, end `FUN_007891e0`; `notes\weapon-kinds.md`
/// "GrenadeProjectile"). [game] It is thrown once, along the way to the aim point tilted
/// by `launchArcAngle`, at `launchSpeed`; after that it is a body under `Zgravity` that
/// goes off when its drawn lifetime is up, when it touches something, or (a proximity one)
/// when a target is within `proximityRadius`.
/// [game 00789b80 l.48-84, 00789d9c-00789ef5] The arc turns the way about way x (0, 0, 1) (x when
/// |way.z| > 0.999) by `launchArcAngle` as stored (`attr+0x158` in EAX), through 005708f0 ->
/// 00570770, whose matrix applied to a row vector is a right-handed turn (its rows emulated in
/// `notes\special-jetfire-mines.md` "fireDummy angle signs"): a positive angle tilts it UP.
/// [game 00789b80 l.129-143, 007254e0, 006e96c0, 006de230, 006e8e20] Gravity is `Zgravity`
/// whole: the launch stores the force `Zgravity * body +0x24` (the mass, `body_set_mass_properties`)
/// and the integrator adds it as v += (invmass * dt) * force with invmass = `+0x28` = 1/mass, so
/// the acceleration is `Zgravity` whatever the mass. [data] the robots' grenades each have their own
/// Collision with a DynamicsData mass right after the GrenadeProjectile: Jetfire's `jetfiremissiledump`
/// `mass: 5` (work\survey\weapons\jetfire.txt l.652449-652460) and the Audi's `gravitonmine` `mass: 100`
/// (audir8.txt l.581984-581995), so the inverse mass is nonzero and the force is not lost (the enemy
/// scout's `grenadelauncher` has 100 too, dcomscout.txt l.379801-379805).
/// [game 00787650, 00786ff0, 007873f0] The launch's 0.05 s timer message ddf4c407 is handled by
/// GrenadeProjectile's slot 3 (vtable 00b5ebe0), which clears bit 1 of its object's +0x60: the
/// "disabled" bit the broad phase (006e5e00, `notes\physics.md` 1.13) and the world queries
/// (005be890, 005be5a0) skip, so for its first 0.05 s the grenade has no physics contacts (world or
/// body). The proximity test does NOT wait: 00786ff0 (the update) calls 007873f0 every update, and
/// 007873f0 gates only on +0x58, the owner, +0x59 (armed) and +0x5a. A grenade that touches the world and does
/// not go off bounces by the usual rule v - (1 + bounciness)(v.n)n (the game's solver does it and
/// is not read). settles: the grenade body's mass in its collision data (`body +0x28` nonzero) is
/// read; the bounce in the solver is not read here.
/// [game 007876b0 at 00787c23-00787cb4] A proximity grenade goes off on a touch of a BODY only:
/// a character when it is not armed by collision, else an object whose `TargetLockable`
/// (0070a410 on the touched object) the thrower may target (`lockable_ignored_by_owner`
/// 007bb2c0 false). The world has no lockable, so a touch of it never sets one off: it arms
/// (when armed by collision), sticks or bounces like any other.
/// [game 007873f0] The proximity test measures to the middle of a target (its bottom plus half
/// its height, 0071a460 / 0070abe0 x 0.5), marks the grenade going off (`+0x5a`, tested once),
/// and hits at once or after `proximity_delay`.
/// A sticky one (`isSticky`, the Audi's mine) stops at its first touch of the world, rests
/// `stickyOffsetFromCollision` off it and stays; it is armed by that touch when the data says
/// so. Its fuse is the launch fuse, unless `GrenadeTuning::sticky_timer`: then it has none in
/// the air and goes off `sticky_lifetime` after it sticks (Jetfire's carpet bomb: 0.1 s); a
/// sticky one with no timer that is not a proximity grenade goes off as it sticks.
/// [game 00786a60, 00787ea0, 00788da5-00788e2b, 007873f0] The contact test of `FUN_00787ea0`
/// lines 146-151 (its 0.18 bounds) passes for the world's frames, so every world touch sticks.
/// [assumed] every body the caller reports is a character the thrower may target (the arena's
/// targets are the scouts of the other side), so a body touch sets a proximity one off whether
/// or not it is armed by collision.
/// Not in: the cluster, the spin of its model.
#[derive(Clone, Copy, Debug)]
pub struct Grenade {
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    life: f32,
    /// [game] record +0x64: no second slowing by a touch for 0.25 s.
    cooldown: f32,
    clock: f32,
    /// Stuck where it touched (`FUN_00787ea0`): it does not move from there.
    stuck: bool,
    /// Under a sticky timer, the seconds from the stick to the blast. Drawn at the launch, not at
    /// the stick as the game does: the port's random numbers are not the game's sequence anyway.
    sticky_after: f32,
    /// [game] record +0x59: a proximity grenade counts targets only when armed.
    armed: bool,
    /// [game] record +0x5a: the proximity test has found a target; it is not run again.
    going_off: bool,
    /// The proximity delay, drawn at the launch as `sticky_after` is.
    proximity_after: f32,
    /// The model's turn (glam's convention). [game 00784170 kind 2 -> 00789b80] The launch takes
    /// only the spawn position (`param_2`) and the velocity from the aim; it sets no frame (no
    /// barrel matrix is passed to it). The grenade's own update then turns it with its velocity
    /// (`look_along`, when `velocity_orient` is set), so this start value is replaced on the
    /// first update that moves it. Read: the update order (the turn runs before the body moves).
    pub orientation: glam::Quat,
    /// The spin (record `+0x90` axis, world space, and `+0x9c` rate, radians a second, with a
    /// random sign). [game 00786a60 l.45-79, 00786ff0 at 007871af-00787286: the rotation is
    /// composed after the body's basis (`compose_affine` 0052fbf0 with the rotation as the
    /// second factor), so the axis is a world axis]
    spin_axis: Vec3,
    spin_rate: f32,
}

impl Grenade {
    /// Armed: a proximity grenade counts targets (record `+0x59`).
    pub fn armed(&self) -> bool {
        self.armed
    }

    /// Thrown from `position`: at `point` when the data says `fireTowardsTarget`, else
    /// along `forward`. `owner` is the thrower's velocity.
    pub fn launch(position: Vec3, forward: Vec3, point: Vec3, owner: Vec3, t: &WeaponTuning, random: &mut dyn FnMut() -> f32) -> Self {
        let g = &t.grenade;
        let forward = forward.normalize_or(Vec3::Y);
        let mut way = if g.fire_towards_target { (point - position).normalize_or(forward) } else { forward };
        if g.launch_arc_angle != 0.0 {
            let up = if way.z.abs() > 0.999 { Vec3::X } else { Vec3::Z };
            let axis = way.cross(up).normalize_or(Vec3::X);
            way = glam::Quat::from_axis_angle(axis, g.launch_arc_angle) * way;
        }
        let speed = if g.tack_on_owner_speed { g.launch_speed + owner.dot(way) } else { g.launch_speed };
        let life = if g.sticky_timer() { f32::INFINITY } else { between(g.lifetime, random) };
        // The spin, after the fuse: a random axis of three draws in -1..1 (straight up when all
        // are 0), the data's otherwise; the rate drawn, its sign at random. [game 00786a60]
        let spin_axis = if g.random_spin_axis {
            let v = Vec3::new(random() * 2.0 - 1.0, random() * 2.0 - 1.0, random() * 2.0 - 1.0);
            if v.length() > 1e-8 { v.normalize() } else { Vec3::Z }
        } else {
            g.spin_axis.normalize_or(Vec3::Z)
        };
        let mut spin_rate = between(g.spin_speed, random);
        if random() < 0.5 {
            spin_rate = -spin_rate;
        }
        Self {
            position,
            velocity: way * speed,
            age: 0.0,
            // No launch fuse under a sticky timer (`FUN_00786a60` l.30-40). [game]
            life,
            cooldown: 0.0,
            clock: 0.0,
            stuck: false,
            sticky_after: if g.sticky_timer() { between(g.sticky_lifetime, random) } else { 0.0 },
            armed: !g.arms_on_collision,
            going_off: false,
            proximity_after: if g.proximity_delay[0] > 0.0 { between(g.proximity_delay, random) } else { 0.0 },
            orientation: look_along(way),
            spin_axis,
            spin_rate,
        }
    }

    /// Going off: the proximity test found a target and its delay runs (record `+0x5a`).
    pub fn going_off(&self) -> bool {
        self.going_off
    }

    /// Moves it on by `dt` in whole updates of the game's. `Ok` is the stretch it covered;
    /// `Err` is where it went off. `touch` answers what a stretch runs into (the point, the
    /// surface's normal, and whether it is a target's body rather than the world), `nearest`
    /// how far the nearest target's middle (its bottom plus half its height) is from a point.
    pub fn step(
        &mut self,
        t: &WeaponTuning,
        touch: &dyn Fn(Vec3, Vec3) -> Option<(Vec3, Vec3, bool)>,
        nearest: &dyn Fn(Vec3) -> Option<f32>,
        dt: f32,
    ) -> Result<(Vec3, Vec3), Vec3> {
        let g = &t.grenade;
        let from = self.position;
        self.clock += dt;
        while self.clock >= crate::sim::GAME_UPDATE {
            self.clock -= crate::sim::GAME_UPDATE;
            let dt = crate::sim::GAME_UPDATE;
            self.age += dt;
            self.cooldown = (self.cooldown - dt).max(0.0);
            if self.age > self.life {
                return Err(self.position);
            }
            // Squared distance <= radius squared; then at once, or the hit message after the
            // drawn delay (the fuse already posted may still come first). No 0.05 s wait here:
            // the update runs the test every time. [game 00786ff0, 007873f0]
            if g.is_proximity && self.armed && !self.going_off && nearest(self.position).is_some_and(|d| d <= g.proximity_radius) {
                self.going_off = true;
                if g.proximity_delay[0] <= 0.0 {
                    return Err(self.position);
                }
                self.life = self.life.min(self.age + self.proximity_after);
            }
            // A stuck one stays where it touched: its velocity and gravity are zeroed (`FUN_00787ea0`).
            if self.stuck {
                continue;
            }
            // The model's turn, before the body moves (00786ff0 runs ahead of the physics step).
            if g.velocity_orient {
                if self.velocity.length_squared() > 0.001 {
                    self.orientation = look_along(self.velocity);
                }
            } else if self.spin_rate != 0.0 {
                self.orientation = (glam::Quat::from_axis_angle(self.spin_axis, self.spin_rate * dt) * self.orientation).normalize();
            }
            self.velocity.z += g.z_gravity * dt;
            let to = self.position + self.velocity * dt;
            // The launch's 0.05 s timer clears the disabled bit: until then the broad phase finds no
            // contact, world or body. [game 00787650, 006e5e00]
            let touched = if self.age < 0.05 { None } else { touch(self.position, to) };
            match touched {
                // A proximity grenade on a target's body: the hit. [game 007876b0 00787c30-00787cb4]
                Some((point, _, true)) if g.is_proximity => return Err(point),
                Some((point, normal, _)) if g.is_sticky => {
                    self.position = point + normal * g.sticky_offset;
                    self.velocity = Vec3::ZERO;
                    self.stuck = true;
                    self.armed |= g.arms_on_collision;
                    // `sticky_attach` after the stick: the timer's hit message, else (no timer, not a
                    // proximity grenade) the hit now. [game 00788da5-00788e2b]
                    if g.sticky_timer() {
                        self.life = self.age + self.sticky_after;
                    } else if !g.is_proximity {
                        return Err(self.position);
                    }
                }
                // The world (no lockable) does not set a proximity one off: it arms it when the
                // data says so (00787cd4-00787ce9) and it bounces. [game 007876b0]
                Some((point, normal, _)) => {
                    self.armed |= g.arms_on_collision;
                    self.position = point + normal * 0.01;
                    self.velocity -= normal * ((1.0 + g.bounciness) * self.velocity.dot(normal));
                    if self.cooldown <= 0.0 {
                        self.cooldown = 0.25;
                        self.velocity *= g.dampening_on_collision;
                    }
                }
                None => self.position = to,
            }
        }
        Ok((from, self.position))
    }
}

/// The turn that points the model's +y along `way`: `look_at_y_matrix` (0056ede0) as the grenade
/// update calls it (00786ff0 at 007870cc-0078715c: up (0, 0, 1) in EDI, fallback (1, 0, 0)). [game]
/// With f the way: x = f x up, z = x x f; when |f . up| > 0.98, z = fallback x f, x = f x z.
/// The missile's update (007847e0 at 00785542-007855dc) uses a different up: [game 00785542-007855dc]
/// (0, 0, 1), and (0, 1, 0) when the way's z is above 0.99 (a plain test). That is
/// `missile_look_along`; this grenade rule is NOT the missile's turn. The game's missile still
/// calls `look_along` (game/src/weapons.rs, the missile's model frame), so that call must move to
/// `missile_look_along` (lead).
pub fn look_along(way: Vec3) -> glam::Quat {
    let f = way.normalize_or(Vec3::Y);
    let (x, z) = if f.dot(Vec3::Z).abs() <= 0.98 {
        let x = f.cross(Vec3::Z).normalize();
        (x, x.cross(f))
    } else {
        let z = Vec3::X.cross(f).normalize();
        (f.cross(z), z)
    };
    glam::Quat::from_mat3(&glam::Mat3::from_cols(x, f, z)).normalize()
}

/// The missile's turn [game 007847e0 at 00785542-007855dc, then look_at_y_matrix 0056ede0]: the
/// same frame as `look_along`, but the up is (0,0,1) and becomes (0,1,0) when the way's z is above
/// 0.99 (a plain test: no absolute value; the compare is `comiss` against 0.99 at 00785542, the
/// up is stored at esp+0x64..0x6c as (0,0,1) before it, and the above-0.99 branch overwrites it
/// with (0,1,0)). The fallback for the near-parallel case (the look-at's 0.98 test on |f . up|,
/// 0056efba) is (1,0,0): the caller pushes a pointer to its esp+0xd8 (007855a3-007855aa), whose
/// x is 1.0 (xmm2 = 00b38834, stored at 007855c1 as [esp+0xdc] after the push) and whose y and z
/// are 0 (xmm0, 0 there as in the up (0,0,1), at 00785577 and 00785580 before the push) [game,
/// lead 2026-10-20; the wave 28 decodes of (target.z, 1, 0) missed the push's 4-byte shift]. The
/// look-at normalises its own direction (0056ee57, 0056ef36), so normalising here changes nothing
/// there; the 0.99 compare takes the way's z as the caller has it [assumed: the guidance way is
/// unit length at the compare]. settles: the scale of the body vector +0x50 (00785380-00785460).
pub fn missile_look_along(way: Vec3) -> glam::Quat {
    let f = way.normalize_or(Vec3::Y);
    let up = if f.z > 0.99 { Vec3::Y } else { Vec3::Z };
    let (x, z) = if f.dot(up).abs() <= 0.98 {
        let x = f.cross(up).normalize();
        (x, x.cross(f))
    } else {
        let z = Vec3::X.cross(f).normalize();
        (f.cross(z), z)
    };
    glam::Quat::from_mat3(&glam::Mat3::from_cols(x, f, z)).normalize()
}

#[cfg(test)]
mod grenade_tests {
    use super::*;

    fn tuning(proximity: bool) -> WeaponTuning {
        WeaponTuning {
            grenade: GrenadeTuning {
                launch_arc_angle: 3.0_f32.to_radians(), lifetime: [0.8, 1.1], fire_towards_target: true, z_gravity: -32.0,
                is_proximity: proximity, proximity_radius: 10.0, proximity_delay: [0.0, 0.0], bounciness: 0.55, dampening_on_collision: 0.995,
                launch_speed: 270.0, tack_on_owner_speed: false,
                is_sticky: false, arms_on_collision: false, sticky_offset: 0.0, sticky_lifetime: [-1.0, -1.0], one_out_at_a_time: false,
                velocity_orient: false, spin_axis: Vec3::Z, random_spin_axis: false, spin_speed: [0.0, 0.0],
            },
            ..Default::default()
        }
    }

    #[test]
    fn a_mine_counts_only_the_other_faction() {
        let (autobot, decepticon) = (0x951a_f706, 0xa54d_c247);
        assert!(thrower_hostile(decepticon, autobot) && thrower_hostile(autobot, decepticon));
        assert!(!thrower_hostile(decepticon, decepticon), "a teammate does not set it off");
        assert!(!thrower_hostile(decepticon, 7) && !thrower_hostile(7, autobot), "a third team is nobody's enemy");
    }

    /// The Audi's mine (`AudiR8_SpecialMine`): sticky, armed by its first contact, a proximity
    /// radius of 15, a fuse of 60 s, thrown at 65 m/s, resting 0.5 off what it touches.
    fn mine() -> WeaponTuning {
        WeaponTuning {
            grenade: GrenadeTuning {
                launch_arc_angle: 0.0, lifetime: [60.0, 60.0], fire_towards_target: false, z_gravity: -32.0,
                is_proximity: true, proximity_radius: 15.0, proximity_delay: [0.0, 0.0], bounciness: 0.6, dampening_on_collision: 0.995,
                launch_speed: 65.0, tack_on_owner_speed: false, is_sticky: true, arms_on_collision: true, sticky_offset: 0.5,
                sticky_lifetime: [-1.0, -1.0], one_out_at_a_time: true,
                velocity_orient: true, spin_axis: Vec3::Z, random_spin_axis: true, spin_speed: [0.0, 0.0],
            },
            ..Default::default()
        }
    }

    /// Jetfire's carpet bomb (`JetFire_CarpetBombAttribute`), as the install has it.
    fn carpet_bomb() -> WeaponTuning {
        let def = WeaponDef { attribute: crate::formats::hash::crc32(b"JetFire_CarpetBombAttribute"), ..WeaponDef::test(Kind::Grenade) };
        load(crate::install::dir(), &[def]).unwrap().remove(0)
    }

    /// Optimus's left-hand laser rifle is the child of the right-hand one and they take turns;
    /// his shell cannons fire together; Long Haul's left flamethrower is the child of his right.
    /// [data] (`Optimus_LaserRifleAttribute` and `...2`, `Optimus_ShellCannonAttribute`,
    /// `LongHaul_FlameThrowerLeftAttribute` in `bnxglobal.str#16`)
    #[test]
    fn a_left_hand_weapon_is_the_child_of_the_right() {
        if crate::install::missing() { return; }
        let tuning = |attribute: &str, kind: Kind| {
            let def = WeaponDef { attribute: crate::formats::hash::crc32(attribute.as_bytes()), ..WeaponDef::test(kind) };
            load(crate::install::dir(), &[def]).unwrap().remove(0)
        };
        let name = |s: &str| crate::formats::hash::crc32(s.as_bytes());
        let rifle = tuning("Optimus_LaserRifleAttribute", Kind::Gun);
        assert_eq!((rifle.children.as_slice(), rifle.parent, rifle.alternating_fire), (&[name("PrimeLaserRifleLeftHand")][..], 0, true));
        let left = tuning("Optimus_LaserRifleAttribute2", Kind::Gun);
        assert_eq!((left.children.len(), left.parent, left.alternating_fire), (0, name("PrimeLaserRifle"), true));
        let cannon = tuning("Optimus_ShellCannonAttribute", Kind::Gun);
        assert_eq!((cannon.children.as_slice(), cannon.alternating_fire), (&[name("ShellCannonL")][..], false));
        let flame = tuning("LongHaul_FlameThrowerLeftAttribute", Kind::Flame);
        assert_eq!(flame.parent, name("LongHaulFlameThrower_Right"));
    }

    /// The Audi's car mine (`AudiR8_VehicleSpecialMine`) is the one grenade with `tackOnOwnerSpeed`:
    /// its speed is `launchSpeed` plus the thrower's velocity along the throw (00789b80 l.95-120).
    /// [data] [game]
    #[test]
    fn the_car_mine_takes_on_the_cars_speed() {
        if crate::install::missing() { return; }
        let def = WeaponDef { attribute: crate::formats::hash::crc32(b"AudiR8_VehicleSpecialMine"), ..WeaponDef::test(Kind::Grenade) };
        let t = load(crate::install::dir(), &[def]).unwrap().remove(0);
        assert!(t.grenade.tack_on_owner_speed, "{:?}", t.grenade);
        let mut random = || 0.5;
        let still = Grenade::launch(Vec3::ZERO, Vec3::Y, Vec3::ZERO, Vec3::ZERO, &t, &mut random);
        let driving = Grenade::launch(Vec3::ZERO, Vec3::Y, Vec3::ZERO, Vec3::new(3.0, 20.0, 0.0), &t, &mut random);
        let way = still.velocity.normalize();
        assert!((driving.velocity.length() - (still.velocity.length() + Vec3::new(3.0, 20.0, 0.0).dot(way))).abs() < 1e-3);
        assert!(driving.velocity.normalize().distance(way) < 1e-5, "the throw keeps its way");
    }

    /// The carpet bomb has no fuse in the air (stickyMin 0.1 > 0: `FUN_00786a60` posts none) and
    /// goes off 0.1 s after it sticks (`sticky_attach` 00788da5). [data] [game]
    #[test]
    fn a_carpet_bomb_goes_off_a_tenth_of_a_second_after_it_lands() {
        if crate::install::missing() { return; }
        let t = carpet_bomb();
        let g = &t.grenade;
        assert!(g.is_sticky && g.is_proximity && !g.arms_on_collision, "{g:?}");
        assert_eq!(g.sticky_lifetime, [0.1, 0.1]);
        assert_eq!(g.lifetime, [3.0, 3.0]);
        assert!(g.sticky_timer());
        let mut random = || 0.5;
        let none = |_: Vec3| None;
        // Dropped from 200 m at launchSpeed 0 under -32: it falls for about 3.5 s, past its 3 s
        // launch lifetime, and does not go off in the air.
        let mut bomb = Grenade::launch(Vec3::new(0.0, 0.0, 200.0), Vec3::Y, Vec3::ZERO, Vec3::ZERO, &t, &mut random);
        let mut landed_at = None;
        let mut went_off = None;
        for tick in 1..400 {
            match bomb.step(&t, &ground, &none, 0.032) {
                Ok(_) if landed_at.is_none() && bomb.stuck => landed_at = Some(tick),
                Ok(_) => {}
                Err(at) => {
                    went_off = Some((tick, at));
                    break;
                }
            }
        }
        let landed = landed_at.expect("it lands");
        let (tick, at) = went_off.expect("it goes off");
        println!("carpet bomb: landed at {:.3} s, went off at {:.3} s ({} updates later) at {at:?}", landed as f32 * 0.032, tick as f32 * 0.032, tick - landed);
        assert!(landed as f32 * 0.032 > 3.0, "no launch fuse in the air");
        assert_eq!(tick - landed, 4, "the first update past 0.1 s after the stick");
        assert!((at.z - g.sticky_offset).abs() < 1e-3);
    }

    /// A sticky grenade with no sticky timer that is not a proximity one goes off as it sticks
    /// (00788e20-00788e2b). [game]
    #[test]
    fn a_sticky_grenade_without_a_timer_or_proximity_goes_off_as_it_sticks() {
        let t = WeaponTuning { grenade: GrenadeTuning { is_proximity: false, ..mine().grenade }, ..Default::default() };
        let mut random = || 0.5;
        let mut m = Grenade::launch(Vec3::new(0.0, 0.0, 2.0), Vec3::X, Vec3::ZERO, Vec3::ZERO, &t, &mut random);
        let at = run(&mut m, &t, &|_| None, 1.0).expect("it goes off on the stick");
        assert!((at.z - 0.5).abs() < 1e-3, "{at:?}");
    }

    /// The ground is z = 0; a touch is where a stretch crosses it, with the ground's normal.
    fn ground(from: Vec3, to: Vec3) -> Option<(Vec3, Vec3, bool)> {
        (from.z >= 0.0 && to.z < 0.0).then(|| (from + (to - from) * (from.z / (from.z - to.z)), Vec3::Z, false))
    }

    /// Steps a grenade on for `seconds`; where it went off, if it did.
    fn run(g: &mut Grenade, t: &WeaponTuning, nearest: &dyn Fn(Vec3) -> Option<f32>, seconds: f32) -> Option<Vec3> {
        for _ in 0..(seconds / 0.032) as usize {
            if let Err(at) = g.step(t, &ground, nearest, 0.032) {
                return Some(at);
            }
        }
        None
    }

    #[test]
    fn a_sticky_mine_stops_where_it_lands_and_stays() {
        let t = mine();
        let mut random = || 0.5;
        let mut m = Grenade::launch(Vec3::new(0.0, 0.0, 2.0), Vec3::X, Vec3::ZERO, Vec3::ZERO, &t, &mut random);
        let none = |_: Vec3| None;
        assert!(run(&mut m, &t, &none, 1.0).is_none());
        // It flew on along the throw, then rests 0.5 off the ground where it touched.
        assert!(m.position.x > 5.0, "{:?}", m.position);
        assert!((m.position.z - 0.5).abs() < 1e-3, "{:?}", m.position);
        assert_eq!(m.velocity, Vec3::ZERO);
        let rest = m.position;
        assert!(run(&mut m, &t, &none, 10.0).is_none());
        assert_eq!(m.position, rest);
    }

    #[test]
    fn a_sticky_mine_goes_off_for_an_enemy_within_its_radius_only_once_armed() {
        let t = mine();
        let mut random = || 0.5;
        // In the air and not yet armed: an enemy 10 m away does not set it off.
        let mut airborne = Grenade::launch(Vec3::new(0.0, 0.0, 50.0), Vec3::Z, Vec3::ZERO, Vec3::ZERO, &t, &mut random);
        let near = |_: Vec3| Some(10.0);
        assert!(run(&mut airborne, &t, &near, 0.5).is_none());
        // Stuck (so armed by the touch): nobody near, it stays; then the same enemy sets it off.
        let mut m = Grenade::launch(Vec3::new(0.0, 0.0, 2.0), Vec3::X, Vec3::ZERO, Vec3::ZERO, &t, &mut random);
        let far = |_: Vec3| Some(40.0);
        assert!(run(&mut m, &t, &far, 1.0).is_none());
        let at = run(&mut m, &t, &near, 0.5).expect("an armed mine goes off near an enemy");
        assert!((at.z - 0.5).abs() < 1e-3, "{at:?}");
    }

    #[test]
    fn a_proximity_grenade_tests_its_targets_from_the_first_update() {
        // The proximity test (007873f0) has no 0.05 s wait; only the physics contacts do.
        let t = WeaponTuning {
            grenade: GrenadeTuning { is_sticky: false, arms_on_collision: false, proximity_delay: [0.0, 0.0], ..mine().grenade },
            ..mine()
        };
        let mut random = || 0.5;
        let mut m = Grenade::launch(Vec3::new(0.0, 0.0, 50.0), Vec3::Z, Vec3::ZERO, Vec3::ZERO, &t, &mut random);
        let near = |_: Vec3| Some(0.0);
        let at = run(&mut m, &t, &near, 0.032).expect("it goes off on the first update");
        assert!(m.age < 0.05 && at.z > 49.0, "{at:?} {}", m.age);
    }

    #[test]
    fn a_stuck_mine_still_goes_off_on_its_fuse() {
        let t = WeaponTuning { grenade: GrenadeTuning { lifetime: [4.0, 4.0], ..mine().grenade }, ..Default::default() };
        let mut random = || 0.5;
        let mut m = Grenade::launch(Vec3::new(0.0, 0.0, 2.0), Vec3::X, Vec3::ZERO, Vec3::ZERO, &t, &mut random);
        let none = |_: Vec3| None;
        assert!(run(&mut m, &t, &none, 3.0).is_none());
        // The fuse runs from the throw, not from the stick: 4 s is up at 4 s.
        let at = run(&mut m, &t, &none, 2.0).expect("the fuse is up");
        assert!((at.z - 0.5).abs() < 1e-3, "{at:?}");
    }

    #[test]
    fn a_flame_lights_its_spheres_in_turn_and_ticks_five_times_a_second() {
        let t = WeaponTuning { flame_damage_per_second: 110.0, flame_ticks_per_second: 5.0, ..Default::default() };
        let sphere = |radius| FlameSphere { at: Vec3::ZERO, radius, delay_to_next: 0.35, node: 0 };
        let def = WeaponDef { flame: [sphere(2.5), sphere(3.0), sphere(3.5)], flame_count: 3, ..WeaponDef::test(Kind::Flame) };
        let mut flame = Flame::default();
        assert_eq!(flame.lit(&def), 0);
        let (mut ticks, mut lit) = (0, Vec::new());
        for _ in 0..63 {
            ticks += flame.step(&t, true, 0.032);
            lit.push(flame.lit(&def));
        }
        // 2.016 s: the first tick a second in, then one every fifth of a second.
        assert_eq!((lit[0], lit[9], lit[13], lit[20], lit[25]), (1, 1, 2, 2, 3));
        assert!((5..=6).contains(&ticks), "{ticks}");
        assert_eq!(Flame::tick_damage(&t), 22.0);
        flame.step(&t, false, 0.032);
        assert!(!flame.burning());
    }

    #[test]
    fn the_flame_impact_ray_reaches_the_last_lit_dummy() {
        // The reach grows as the spheres light: to the last lit one's place. [game 0074c200]
        let t = WeaponTuning { flame_damage_per_second: 110.0, flame_ticks_per_second: 5.0, ..Default::default() };
        let sphere = |d: f32, radius| FlameSphere { at: Vec3::new(d, 0.0, 0.0), radius, delay_to_next: 0.35, node: 0 };
        let def = WeaponDef { flame: [sphere(1.0, 2.5), sphere(4.0, 3.0), sphere(8.0, 3.5)], flame_count: 3, muzzle: Vec3::ZERO, ..WeaponDef::test(Kind::Flame) };
        let mut flame = Flame::default();
        flame.step(&t, true, 0.032);
        assert_eq!(flame.reach(&def), 1.0);
        for _ in 0..30 { flame.step(&t, true, 0.032); }
        assert_eq!(flame.reach(&def), 8.0);
        assert_eq!(Flame::default().reach(&def), 0.0);
    }

    /// The hit list of the flame's contact handler, one update at a time (age, then the
    /// contact), against `work\weapon-audit\scratch\r3\flame_hits_sim.py`. [game 0074d610, 0074c200]
    #[test]
    fn flame_hits_follow_the_contact_handler() {
        let t = WeaponTuning { flame_damage_per_second: 110.0, flame_ticks_per_second: 5.0, ..Default::default() };
        let run = |updates: usize, touching: &dyn Fn(usize) -> bool| {
            let mut list = FlameHits::default();
            let mut hits = Vec::new();
            for k in 0..updates {
                list.age(0.032);
                if touching(k) && list.may_hit(7) {
                    hits.push(k);
                    list.taken(7, &t);
                }
            }
            hits
        };
        assert_eq!(run(32, &|_| true), vec![0, 7, 13, 19, 25]);
        let long = run(157, &|_| true);
        assert_eq!(long.len(), 25);
        assert_eq!(long.len() as f32 * Flame::tick_damage(&t), 550.0);
        assert_eq!(run(32, &|k| !(3..10).contains(&k)), vec![0, 10, 17, 23, 29]);
        // Each victim has its own entry.
        let mut list = FlameHits::default();
        list.taken(1, &t);
        assert!(!list.may_hit(1) && list.may_hit(2));
    }

    /// The release frees the list: a new press hits again at once. [game 0074d110]
    #[test]
    fn the_flame_hits_again_after_a_release() {
        let t = WeaponTuning { flame_damage_per_second: 110.0, flame_ticks_per_second: 5.0, ..Default::default() };
        let mut list = FlameHits::default();
        list.age(0.032);
        assert!(list.may_hit(3));
        list.taken(3, &t);
        list.age(0.032);
        assert!(!list.may_hit(3));
        list.clear();
        list.age(0.032);
        assert!(list.may_hit(3));
    }

    /// Long Haul's flame packet from the install: 22 a hit, flags 0x810, heat 30 / 5, the
    /// knock-back as the data's, the direction flat from the weapon to the victim. [game 0074d610]
    #[test]
    fn the_flame_packet_divides_heat_with_the_overheat_bits() {
        if crate::install::missing() { return; }
        let def = WeaponDef { attribute: crate::formats::hash::crc32(b"LongHaul_FlameThrowerAttribute"), ..WeaponDef::test(Kind::Flame) };
        let mut t = load(crate::install::dir(), &[def]).unwrap().remove(0);
        let p = flame_packet(&t, Vec3::new(0.0, 0.0, 1.0), Vec3::new(3.0, 4.0, 9.0));
        assert_eq!(p.amount, 22.0);
        assert_eq!(p.flags, 0x810);
        assert!((p.heat - 6.0).abs() < 1e-5, "{}", p.heat);
        assert_eq!(p.knockback_speed, 15.0);
        assert!((p.knockback_angle - 35.0_f32.to_radians()).abs() < 1e-5);
        assert_eq!(p.dot_time, 0.0);
        assert!((p.direction - Vec3::new(0.6, 0.8, 0.0)).length() < 1e-6, "{:?}", p.direction);
        assert!(t.listen_to_anim_event, "every held robot weapon listens to its arm clip");
        t.damage_flags = 0x800;
        assert_eq!(flame_packet(&t, Vec3::ZERO, Vec3::X).heat, 30.0);
        // An all-bits value keeps only the bits the handler lets through.
        t.damage_flags = u32::MAX;
        assert_eq!(flame_packet(&t, Vec3::ZERO, Vec3::X).flags, 0xf27f_ffff);
    }

    /// Long Haul's launcher: nothing leaves on the press (`007aca50` fires only without
    /// `hasChargedShot`); a tap throws one as it is let go (`007acb90`); held on, the charge
    /// gains a grenade at half and at full, each for 14 of heat, and letting go throws them at
    /// once (`007ad1c0`, `007addc0`, `007af210`). [game]
    #[test]
    fn a_held_trigger_charges_up_to_three_grenades() {
        let t = WeaponTuning {
            rate_of_fire: 0.4, heat_capacity: 100.0, heat_per_round: 8.0, heat_cooldown_til_decrement: 1.0, heat_decrement_per_second: 20.0,
            charge: ChargeTuning { has: true, start: 0.2, duration: 4.0, hold: -1.0, damage: [1.0, 2.0], heat: [5.0, 10.0], count: 3, heat_rate: 9.0, heat_per_step: 14.0 },
            ..Default::default()
        };
        let hold = |seconds: f32| {
            let mut w = Weapon::default();
            let mut thrown = Vec::new();
            for step in 0..(seconds / 0.032) as usize + 1 {
                if let Fired::Rounds(n) = w.update(&t, step < (seconds / 0.032) as usize, 0.032) { thrown.push((step, n)); }
            }
            (thrown, w)
        };
        // A tap: one grenade on the update it is let go, none on the press.
        assert_eq!(hold(0.1).0, vec![(3, 1)]);
        // Under half (0.32): one. Past half (0.69): two. Full: three, and the heat of two steps,
        // about 0.8 s at full at 9 a second, and the release's round of 8.
        assert_eq!(hold(1.5).0, vec![(46, 1)]);
        assert_eq!(hold(3.0).0, vec![(93, 2)]);
        let (thrown, w) = hold(5.0);
        assert_eq!(thrown, vec![(156, 3)]);
        assert!(w.heat > 8.0 + 28.0 + 6.5 && w.heat < 8.0 + 28.0 + 8.0 && !w.charging, "{}", w.heat);
        assert_eq!(w.charge_level(&t), 0.0);
    }

    /// Optimus's ShellCannon (count 1): nothing on the press, the charge starts on the update
    /// the time held first passes 0.1, steps at 10 heat a second, is full once, and the release
    /// is one round of 2.5 times the damage and 25 of heat, the full charge's. [game 007ad1c0,
    /// 007addc0, 007af210]
    fn shell_cannon() -> WeaponTuning {
        WeaponTuning {
            rate_of_fire: 0.5, heat_capacity: 100.0, heat_per_round: 12.0, heat_decrement_per_second: 25.0, heat_cooldown_til_decrement: 0.8,
            overheat_period: 6.0,
            charge: ChargeTuning { has: true, start: 0.1, duration: 2.25, hold: -1.0, damage: [1.0, 2.5], heat: [12.0, 25.0], count: 1, heat_rate: 10.0, heat_per_step: 10.0 },
            ..Default::default()
        }
    }

    /// Runs `updates` updates with the trigger as `held` says, from `w`: what fired, with the
    /// update, and each update's charge events.
    fn charge_run(w: &mut Weapon, t: &WeaponTuning, updates: usize, held: impl Fn(usize) -> bool) -> (Vec<(usize, Fired)>, Vec<ChargeEvents>) {
        let (mut fired, mut events) = (Vec::new(), Vec::new());
        for k in 0..updates {
            w.charge_events = ChargeEvents::default();
            let f = w.update(t, held(k), 0.032);
            if !matches!(f, Fired::Nothing) {
                fired.push((k, f));
            }
            events.push(w.charge_events);
        }
        (fired, events)
    }

    #[test]
    fn a_full_charge_of_the_shell_cannon_is_one_round_on_the_release() {
        let t = shell_cannon();
        let mut w = Weapon::default();
        let (fired, events) = charge_run(&mut w, &t, 79, |k| k < 78);
        assert_eq!(fired, vec![(78, Fired::Rounds(1))]);
        assert!((w.charge_damage - 2.5).abs() < 1e-5 && w.charge_full, "{}", w.charge_damage);
        // 73 steps of 0.32 and the release's 25.
        assert!((w.heat - 48.4).abs() < 0.5, "{}", w.heat);
        let started: Vec<usize> = (0..events.len()).filter(|&k| events[k].started).collect();
        assert_eq!(started, vec![4]);
        assert!(events[4].step.is_none(), "no step on the start's update");
        let full: Vec<usize> = (0..events.len()).filter(|&k| events[k].full).collect();
        assert_eq!(full, vec![74]);
        assert_eq!(events.iter().filter(|e| e.step.is_some()).count(), 73);
        assert_eq!(events[78].released, Some(1.0));
        assert!(!w.charging);
        // A tap after it: an ordinary round, not charged.
        charge_run(&mut w, &t, 20, |_| false);
        let (fired, _) = charge_run(&mut w, &t, 3, |k| k < 2);
        assert_eq!(fired, vec![(2, Fired::Rounds(1))]);
        assert!(w.charge_damage == 1.0 && !w.charge_full);
    }

    /// The step's heat reaching the capacity while the trigger is still down fires the charge
    /// by itself (`007ad1c0`: it returns only with the trigger down AND the heat under the
    /// capacity), overheating it; nothing more leaves while it is held or let go. [game]
    #[test]
    fn a_charge_that_fills_the_heat_fires_by_itself_while_held() {
        let t = shell_cannon();
        let mut w = Weapon { heat: 90.0, ..Weapon::default() };
        let (fired, events) = charge_run(&mut w, &t, 120, |k| k < 110);
        let rounds: Vec<&(usize, Fired)> = fired.iter().filter(|(_, f)| matches!(f, Fired::Rounds(_) | Fired::RoundsAndOverheated(_))).collect();
        assert_eq!(rounds.len(), 1, "{fired:?}");
        let (k, f) = *rounds[0];
        assert!(k < 110 && f == Fired::RoundsAndOverheated(1), "{k} {f:?}");
        let level = events[k].released.unwrap();
        assert!(level > 0.0 && level < 1.0, "{level}");
        // Its multiplier is the level's (it may be overwritten only by a later firing: none).
        assert!(w.charge_damage > 1.0 && w.charge_damage < 2.5, "{}", w.charge_damage);
        // The lock: 6 s from that update (counted down from its own update on).
        let mut again = Weapon { heat: 90.0, ..Weapon::default() };
        charge_run(&mut again, &t, k + 1, |_| true);
        assert!((again.overheated_left - (6.0 - 0.032)).abs() < 1e-4, "{}", again.overheated_left);
    }

    /// The pad's rumble while charging (`007addc0` step 2): a count under 2 rumbles every step,
    /// effect 2 at 0.2, effect 3 at 1.0 at full; a count of 3 only at full. [game]
    #[test]
    fn a_charge_step_rumbles_by_its_count_and_level() {
        let one = ChargeTuning { count: 1, ..Default::default() };
        let three = ChargeTuning { count: 3, ..Default::default() };
        assert_eq!(charge_rumble(&one, 0.5, 0.032), Some((2, 0.032, 0.2)));
        assert_eq!(charge_rumble(&one, 1.0, 0.032), Some((3, 0.032, 1.0)));
        assert_eq!(charge_rumble(&three, 0.5, 0.032), None);
        assert_eq!(charge_rumble(&three, 1.0, 0.032), Some((3, 0.032, 1.0)));
        assert_eq!(charge_rumble(&one, 0.5, 0.0), None);
    }

    /// Megatron's special cancels a charge (`007add20`): no shot, the charge and the time held
    /// go. [game]
    #[test]
    fn the_cancel_ends_a_charge_with_no_shot() {
        let t = shell_cannon();
        let mut w = Weapon::default();
        charge_run(&mut w, &t, 20, |_| true);
        assert!(w.charging);
        w.set_charge_cancel(true);
        let (fired, events) = charge_run(&mut w, &t, 1, |_| true);
        assert!(fired.is_empty() && events[0].cancelled && !w.charging);
        w.set_charge_cancel(false);
        // The time held starts again from the cancel: let go at once, it is a tap.
        let (fired, _) = charge_run(&mut w, &t, 2, |k| k == 0);
        assert_eq!(fired, vec![(1, Fired::Rounds(1))]);
    }

    #[test]
    fn it_is_thrown_at_the_point_tilted_up_and_goes_off_when_its_time_is_up() {
        let t = tuning(true);
        let mut g = Grenade::launch(Vec3::ZERO, Vec3::Y, Vec3::new(0.0, 100.0, 0.0), Vec3::ZERO, &t, &mut || 0.5);
        assert!((g.velocity.length() - 270.0).abs() < 1e-2 && (g.velocity.z - 270.0 * 3.0_f32.to_radians().sin()).abs() < 1e-2, "{:?}", g.velocity);
        let mut flown = 0.0;
        let at = loop {
            match g.step(&t, &|_, _| None, &|_| None, 0.032) {
                Ok((a, b)) => flown += a.distance(b),
                Err(at) => break at,
            }
        };
        assert!((g.age - 0.95).abs() < 0.04 && flown > 230.0 && at.y > 230.0, "{} {flown} {at:?}", g.age);
    }

    #[test]
    fn a_proximity_one_goes_off_near_a_target_or_on_a_body_and_bounces_off_the_world() {
        let t = tuning(true);
        let target = Vec3::new(0.0, 60.0, 0.0);
        let mut g = Grenade::launch(Vec3::ZERO, Vec3::Y, target, Vec3::ZERO, &t, &mut || 0.5);
        let at = loop {
            if let Err(at) = g.step(&t, &|_, _| None, &|p| Some(p.distance(target)), 0.032) {
                break at;
            }
        };
        assert!(at.distance(target) <= 10.0 && g.age < 0.3, "{at:?}");
        // A floor under one thrown level and low: the world sets no proximity grenade off
        // (007876b0: it has no TargetLockable), so both kinds bounce on it, each time lower,
        // until the fuse. [game]
        let floor = |a: Vec3, b: Vec3| (a.z > -1.0 && b.z <= -1.0).then(|| (a.lerp(b, (a.z + 1.0) / (a.z - b.z)), Vec3::Z, false));
        let slow = |proximity| WeaponTuning { grenade: GrenadeTuning { launch_speed: 10.0, lifetime: [4.0, 4.0], ..tuning(proximity).grenade }, ..Default::default() };
        for proximity in [true, false] {
            let mut g = Grenade::launch(Vec3::ZERO, Vec3::Y, Vec3::Y, Vec3::ZERO, &slow(proximity), &mut || 0.5);
            let (mut bounces, mut was_falling) = (0, true);
            while let Ok(_) = g.step(&slow(proximity), &floor, &|_| None, 0.032) {
                assert!(g.position.z > -1.0);
                bounces += u32::from(was_falling && g.velocity.z > 0.0);
                was_falling = g.velocity.z <= 0.0;
            }
            assert!(bounces >= 2 && (g.age - 4.0).abs() < 0.05, "{proximity}: {bounces} {}", g.age);
        }
        // The same floor as a target's body: the proximity one goes off on it, the other bounces.
        let body = |a: Vec3, b: Vec3| floor(a, b).map(|(p, n, _)| (p, n, true));
        let mut g = Grenade::launch(Vec3::ZERO, Vec3::Y, Vec3::Y, Vec3::ZERO, &slow(true), &mut || 0.5);
        let at = loop {
            if let Err(at) = g.step(&slow(true), &body, &|_| None, 0.032) {
                break at;
            }
        };
        assert!((at.z + 1.0).abs() < 1e-3 && g.age < 1.0, "{at:?}");
        let mut g = Grenade::launch(Vec3::ZERO, Vec3::Y, Vec3::Y, Vec3::ZERO, &slow(false), &mut || 0.5);
        assert!(run_with(&mut g, &slow(false), &body, 1.0).is_none());
    }

    fn run_with(g: &mut Grenade, t: &WeaponTuning, touch: &dyn Fn(Vec3, Vec3) -> Option<(Vec3, Vec3, bool)>, seconds: f32) -> Option<Vec3> {
        for _ in 0..(seconds / 0.032) as usize {
            if let Err(at) = g.step(t, touch, &|_| None, 0.032) {
                return Some(at);
            }
        }
        None
    }

    /// `look_at_y_matrix`'s frame: +y on the way, x = f x up, z = x x f; near vertical, the
    /// fallback x. [game 0056ede0]
    #[test]
    fn the_look_at_points_y_along_the_way_and_keeps_z_up() {
        let q = look_along(Vec3::new(1.0, 1.0, 0.0));
        assert!((q * Vec3::Y).distance(Vec3::new(1.0, 1.0, 0.0).normalize()) < 1e-5);
        assert!((q * Vec3::Z).distance(Vec3::Z) < 1e-5);
        assert!((q * Vec3::X).distance(Vec3::new(1.0, -1.0, 0.0).normalize()) < 1e-5, "x = f x up");
        // Straight down: z = X x f = X x -Z = Y, x = f x z = -Z x Y = X.
        let down = look_along(-Vec3::Z);
        assert!((down * Vec3::Y).distance(-Vec3::Z) < 1e-5 && (down * Vec3::Z).distance(Vec3::Y) < 1e-5 && (down * Vec3::X).distance(Vec3::X) < 1e-5);
    }

    /// The missile's up: (0,0,1), or (0,1,0) when the way's z is above 0.99 [game 00785542].
    #[test]
    fn the_missile_turn_uses_y_up_only_above_z_point_99() {
        // A way with z 0.95 is the ordinary look.
        let w = Vec3::new(0.3, 0.0, 0.95).normalize();
        assert!((missile_look_along(w) * Vec3::Y).distance(w) < 1e-5);
        assert!(missile_look_along(w).angle_between(look_along(w)) < 1e-5);
        // Straight up: up (0,1,0), x = Z x Y = -X, z = x x f = Y.
        let up = missile_look_along(Vec3::Z);
        assert!((up * Vec3::Y).distance(Vec3::Z) < 1e-5);
        assert!((up * Vec3::X).distance(-Vec3::X) < 1e-5, "x {:?}", up * Vec3::X);
        assert!((up * Vec3::Z).distance(Vec3::Y) < 1e-5, "z {:?}", up * Vec3::Z);
        // The grenade's rule differs there: its z is -Y.
        assert!((look_along(Vec3::Z) * Vec3::Z).distance(-Vec3::Y) < 1e-5);
    }

    /// A grenade that does not turn to its velocity spins about its axis (a world axis) at its
    /// drawn rate; the Audi's mines turn to their velocity. [game 00786a60, 00786ff0] [data]
    #[test]
    fn a_grenade_spins_about_its_axis_or_turns_to_its_velocity() {
        let spinning = WeaponTuning {
            grenade: GrenadeTuning { spin_axis: Vec3::Z, random_spin_axis: false, spin_speed: [std::f32::consts::PI, std::f32::consts::PI], launch_speed: 10.0, fire_towards_target: false, launch_arc_angle: 0.0, ..tuning(false).grenade },
            ..Default::default()
        };
        // Draws: the fuse, the rate, its sign (0.9: kept positive).
        let mut g = Grenade::launch(Vec3::new(0.0, 0.0, 100.0), Vec3::Y, Vec3::ZERO, Vec3::ZERO, &spinning, &mut || 0.9);
        let start = g.orientation * Vec3::Y;
        for _ in 0..10 {
            g.step(&spinning, &|_, _| None, &|_| None, 0.032).unwrap();
        }
        // 0.32 s at 180 degrees a second about world z: 57.6 degrees.
        let turned = start.angle_between(g.orientation * Vec3::Y).to_degrees();
        assert!((turned - 57.6).abs() < 0.01 && (g.orientation * Vec3::Z).distance(Vec3::Z) < 1e-4, "{turned}");
        let oriented = WeaponTuning { grenade: GrenadeTuning { velocity_orient: true, ..spinning.grenade }, ..Default::default() };
        let mut m = Grenade::launch(Vec3::new(0.0, 0.0, 100.0), Vec3::Y, Vec3::ZERO, Vec3::ZERO, &oriented, &mut || 0.9);
        for _ in 0..20 {
            m.step(&oriented, &|_, _| None, &|_| None, 0.032).unwrap();
        }
        let v = m.velocity.normalize();
        // It turns at the start of an update, before gravity changes the velocity: one update behind.
        assert!((m.orientation * Vec3::Y).angle_between(v).to_degrees() < 10.0, "{:?} {v:?}", m.orientation * Vec3::Y);
    }

    /// The install's spin data: Long Haul's grenade spins (180 to 360 degrees a second, a random
    /// axis); the Audi's mine turns to its velocity. [data]
    #[test]
    fn the_spin_data_is_read() {
        if crate::install::missing() { return; }
        let def = WeaponDef { attribute: crate::formats::hash::crc32(b"AudiR8_SpecialMine"), ..WeaponDef::test(Kind::Grenade) };
        let t = load(crate::install::dir(), &[def]).unwrap().remove(0);
        assert!(t.grenade.velocity_orient);
        let def = WeaponDef { attribute: crate::formats::hash::crc32(b"Artillery_Attribute"), ..WeaponDef::test(Kind::Grenade) };
        let t = load(crate::install::dir(), &[def]).unwrap().remove(0);
        assert!(!t.grenade.velocity_orient && t.grenade.random_spin_axis, "{:?}", t.grenade);
        assert_eq!(t.grenade.spin_axis, Vec3::Z);
        assert!((t.grenade.spin_speed[0] - std::f32::consts::PI).abs() < 1e-4 && (t.grenade.spin_speed[1] - std::f32::consts::TAU).abs() < 1e-4, "{:?}", t.grenade.spin_speed);
        assert_eq!(t.grenade.proximity_delay, [0.02, 0.02]);
    }

    /// `proximityExplosionMinDelay` / `MaxDelay`: found at once, gone off one update past the
    /// delay; the test is not run again meanwhile, and a 0 delay goes off on the update it finds
    /// the target. [game 007873f0] Long Haul's grenade (0.02 to 0.04 in his data) goes off on the
    /// next update; a 0.25 delay (`AOmniScout1_LoogeyGunAttribute`) eight updates on.
    #[test]
    fn a_proximity_one_goes_off_after_its_drawn_delay() {
        let at_rest = |delay: [f32; 2]| WeaponTuning {
            grenade: GrenadeTuning { launch_speed: 0.0, z_gravity: 0.0, lifetime: [10.0, 10.0], proximity_delay: delay, ..tuning(true).grenade },
            ..Default::default()
        };
        let updates_to_go_off = |delay: [f32; 2]| {
            let t = at_rest(delay);
            let mut g = Grenade::launch(Vec3::ZERO, Vec3::Y, Vec3::ZERO, Vec3::ZERO, &t, &mut || 0.5);
            // Nobody near for 0.1 s, then a target 5 m off.
            let mut found = None;
            for update in 1..100 {
                let near = |_: Vec3| Some(if update > 3 { 5.0 } else { 50.0 });
                let stepped = g.step(&t, &|_, _| None, &near, 0.032);
                if g.going_off() && found.is_none() {
                    found = Some(update);
                }
                if stepped.is_err() {
                    return (found.unwrap_or(update), update);
                }
            }
            panic!("it did not go off");
        };
        assert_eq!(updates_to_go_off([0.0, 0.0]), (4, 4));
        let (found, off) = updates_to_go_off([0.02, 0.04]);
        assert_eq!((found, off - found), (4, 1));
        let (found, off) = updates_to_go_off([0.25, 0.25]);
        assert_eq!((found, off - found), (4, 8));
    }
}

/// What a missile lets go of where it ends: its `Explosion` (`FUN_007e5f00`, `FUN_007e60f0`,
/// the same as the ground punch's blast: a sphere that grows from `start_scale` to its full
/// size over `explosion_time`, and hits each thing it touches once, by the distance from its
/// middle). [game] The owner's routine (`FUN_007e6af0`, the branch for a missile) replaces
/// the explosion's damage with the launch's `overwriteExplosiveDamage` when that is 0 or
/// more, and its `minDamage` with `overwriteExplosiveMinDamage` likewise (his is -1: the
/// explosion's own 1 stays). The missile ends on the first thing it touches, or at the end
/// of its lifetime (`FUN_00785c00` sends it a kill of 100000 damage of the destroy kind the
/// data's `DamageData` names). [data] [assumed: the touch ends it at once; that it ends on
/// any contact and not only on some surfaces; that it is made in the update after the one
/// that ended the missile] Not in: the camera shake (his has none), the light, the sound
/// and the particles the object carries, the scorch on a surface (`SSDImpactPreset`).
/// A weapon's `snakerInfo`. [game 007bba30]
/// settles: the touch test of the snaker's contact (which surfaces end it, and in which update), read from the snaker's contact handler (not located in this pass).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SnakerTuning {
    /// Metres a second along the ground.
    pub speed: f32,
    /// Dealt to a character it touches; the push is its way times damage x 100.
    pub damage: f32,
    /// Its life from activation (message 54732c8d at the end).
    pub time_til_dies: f32,
    pub damage_flags: u32,
}

/// A `SnakerProjectile`: Blackout's ground wave (`Blackout_VehicleSnakerAttribute`: speed 60,
/// damage 100, life 1.75 s), also listed by Jetfire's and OptimusFlight's missiles and the
/// DConst flame wave. It drops until it rests on the ground, then slides along it at
/// `speed`, kept on the surface by a ray along the surface's normal, and ends at a wall, a
/// drop, or its time. Code: ctor `FUN_0078cfd0`, activation `FUN_0078d1e0`, first ground
/// check `FUN_0078d560`, update `FUN_0078d4a0`, start of the slide `FUN_0078ef60`, the slide
/// `FUN_0078da10`, the wall test `FUN_0078e300`, contact `FUN_0078e950`, end `FUN_0078e730`.
/// [game]
///
/// `world` answers rays with the first hit of the WORLD: the game skips destructables and
/// characters in the hit list (`FUN_0070a460`, `FUN_0070a3c0`), the caller's `Sight` does
/// the same. [game 0078d560, 0078da10] `Sight::sight` stands for the game's ray query with mask 1
/// (`world_ray_sphere_query` 005be890 (.., 1, .., 0)): the snaker's ground check (0078d560) and
/// its slide (0078da10) both pass mask 1 and a last argument of 0, as `aim_point` does.
/// Not in: the owner-based ground-check height (`FUN_0078d560` uses the owner character's
/// bottom + 0.25 when that is higher than `checkGroundHeight`), the hidden / physics-off
/// state before activation, the trigger callbacks of its first ground contact, the frame's
/// matrix (only its forward is kept), and the damage message itself (`contact` returns it).
#[derive(Clone, Debug)]
pub struct Snaker {
    pub position: Vec3,
    /// The way it was sent (`+0x48..+0x50`), made level with the surface once it starts.
    pub direction: Vec3,
    /// The surface's normal (`+0x54..+0x5c`).
    pub normal: Vec3,
    /// Its forward along the surface (the object's second row, rebuilt each slide step).
    pub forward: Vec3,
    /// `+0x29` sliding, `+0x2a` on the ground, `+0x2b` resting on level ground.
    pub sliding: bool,
    pub grounded: bool,
    pub settled: bool,
    /// Updates spent neither grounded nor sliding (`+0x60`): past 2 it is removed.
    pub idle_updates: u32,
    pub age: f32,
    pub alive: bool,
    /// `checkGroundHeight` (`+0x30`, ctor default 1): how far above and below the surface
    /// the ray runs.
    pub check_height: f32,
    /// The wave object's `TimeoutDeleter` time (`character::WaveEffects::life`); infinite when
    /// none is known.
    pub life: f32,
}

impl Snaker {
    /// The same wave with its object's deleter time: it ends at min(`timeTilDies`, `life`).
    pub fn with_life(mut self, life: f32) -> Self {
        self.life = life;
        self
    }

    /// Activation (`FUN_0078d1e0`) with its first ground check (`FUN_0078d560`): a ray
    /// from `check_height` above to `check_height` below; a world hit whose normal is more
    /// than 0.25 up puts it on the ground there, normal straight up.
    pub fn activate(position: Vec3, direction: Vec3, check_height: f32, world: &dyn crate::camera::Sight) -> Self {
        let mut s = Self {
            position,
            direction,
            normal: Vec3::Z,
            forward: direction.normalize_or(Vec3::Y),
            sliding: false,
            grounded: false,
            settled: false,
            idle_updates: 0,
            age: 0.0,
            alive: true,
            check_height,
            life: f32::INFINITY,
        };
        let up = Vec3::Z * check_height;
        // [game 0078d560] the ground check's query takes mask 1 (PUSH), `notes\collision-query-masks.md`.
        if let Some(hit) = world.sight_mask(position + up, position - up, crate::collision_masks::PUSH) {
            if hit.normal.normalize_or_zero().z > 0.25 {
                s.grounded = true;
                s.settled = true;
                s.normal = Vec3::Z;
                s.position = hit.point;
            }
        }
        s
    }

    /// A contact with the world (`FUN_0078e950`, the branch for a world touch): until it
    /// rests level, it counts as on the ground, the most upward normal so far is kept, and a
    /// normal 0.99 up settles it. [game 0078e950]
    pub fn touch_world(&mut self, normal: Vec3) {
        if !self.alive || (self.settled && self.grounded) {
            return;
        }
        self.grounded = true;
        let n = normal.normalize_or_zero();
        if n.z > self.normal.z {
            self.normal = n;
        }
        if self.normal.z >= 0.99 {
            self.settled = true;
        }
    }

    /// A contact with a character that is not its owner (`FUN_0078e950`): the damage it
    /// sends and the push, along its way at damage x 100. [game 0078e950]
    pub fn touch_character(&self, t: &SnakerTuning) -> Option<(f32, Vec3)> {
        (self.alive && self.direction.length_squared() > 0.001).then(|| (t.damage, self.direction.normalize_or_zero() * t.damage * 100.0))
    }

    /// One update (`FUN_0078d4a0`). [game]
    pub fn step(&mut self, t: &SnakerTuning, world: &dyn crate::camera::Sight, dt: f32) {
        if !self.alive {
            return;
        }
        self.age += dt;
        // [game 0078d1e0] the life timer (message 54732c8d after `timeTilDies`). [game 0078e710]
        // That message's handler on the snaker ends it: `54732c8d` calls `snaker_end` 0078e730
        // (the kill damage message, flags 0x10000000). Whether the snaker's end is the same as
        // the timer's kill is taken from that call. settles: 0078e730's damage packet is not
        // traced beyond its flags.
        // The object's own TimeoutDeleter removes it, wave and all, at `life` (`with_life`):
        // whichever comes first. [game 00776f50] [data: Jetfire's 1.0 s against his 5.0 s]
        if self.age >= t.time_til_dies.min(self.life) {
            self.alive = false;
            return;
        }
        if self.sliding {
            self.slide(t, world, dt);
        } else if self.grounded {
            self.start_slide(t, world);
        } else if dt != 0.0 {
            self.idle_updates += 1;
            if self.idle_updates > 2 {
                self.alive = false;
            }
        }
    }

    /// `FUN_0078ef60`: the way made level with the surface (a cross with a fallback axis
    /// when it lies along the normal), then a slide step of no time. [game]
    fn start_slide(&mut self, t: &SnakerTuning, world: &dyn crate::camera::Sight) {
        self.sliding = true;
        let n = self.normal;
        let mut d = self.direction.normalize_or_zero();
        if d.dot(n).abs() > 0.9999 {
            d = if n.z.abs() >= 0.9999 { Vec3::Y } else { Vec3::Z };
        }
        let side = n.cross(d).normalize_or_zero();
        self.direction = side.cross(n).normalize_or_zero();
        self.slide(t, world, 0.0);
    }

    /// `FUN_0078da10`: ahead by speed x dt, a ray along the normal from `check_height` above
    /// to below; no world hit, or one whose normal is under 0.25 along the surface's, ends
    /// it; else it stands on the hit with its forward level with the surface, and the wall
    /// test follows. [game]
    fn slide(&mut self, t: &SnakerTuning, world: &dyn crate::camera::Sight, dt: f32) {
        let ahead = self.position + self.direction * t.speed * dt;
        let h = self.normal * self.check_height;
        // [game 0078da10] the slide's ray takes mask 1 (PUSH), `notes\collision-query-masks.md`.
        let Some(hit) = world.sight_mask(ahead + h, ahead - h, crate::collision_masks::PUSH) else {
            self.alive = false;
            return;
        };
        let n = hit.normal.normalize_or_zero();
        // The object's up row is the kept normal (the frame is built on it).
        if n != self.normal && self.normal.dot(n) < 0.25 {
            self.alive = false;
            return;
        }
        // [game 0078da10] the frame is rebuilt from the KEPT normal and the way (the hit's
        // normal is not taken here; only a contact changes the normal).
        let side = self.normal.cross(self.direction).normalize_or_zero();
        self.forward = side.cross(self.normal).normalize_or(self.forward);
        self.position = hit.point;
        if self.hits_wall(t, world, dt) {
            self.alive = false;
        }
    }

    /// `FUN_0078e300`: from `check_height` up the normal, a ray along its forward from
    /// speed x dt ahead to as far behind; a world hit whose normal is under 0.5 along the
    /// surface's is a wall. [game]
    fn hits_wall(&self, t: &SnakerTuning, world: &dyn crate::camera::Sight, dt: f32) -> bool {
        let base = self.position + self.normal * self.check_height;
        let reach = self.forward * t.speed * dt;
        // [game 0078e300] the wall ray takes mask 1 (PUSH), `notes\collision-query-masks.md`.
        world.sight_mask(base + reach, base - reach, crate::collision_masks::PUSH).is_some_and(|hit| self.normal.dot(hit.normal.normalize_or_zero()) < 0.5)
    }
}

#[derive(Clone, Debug)]
pub struct MissileBlast {
    pub centre: Vec3,
    /// The sphere's size now.
    pub radius: f32,
    pub age: f32,
    def: ExplosionDef,
    damage: f32,
    min_damage: f32,
    hit: Vec<u32>,
    /// Time not yet used up by whole updates of the game's.
    clock: f32,
}

/// A target an explosion reached: what it takes, and the way the blow went.
#[derive(Clone, Copy, Debug)]
pub struct BlastHit {
    pub target: u32,
    pub damage: f32,
    pub point: Vec3,
    /// Flat, from the explosion's middle to where the target stands.
    pub direction: glam::Vec2,
    pub knock_back_speed: f32,
    pub knock_back_angle: f32,
    pub flags: u32,
}

impl MissileBlast {
    /// A missile's explosion at `centre`, with the launcher's numbers.
    pub fn new(def: ExplosionDef, t: &WeaponTuning, centre: Vec3) -> Self {
        let damage = if t.explosive_damage >= 0.0 { t.explosive_damage } else { def.damage };
        let min_damage = if t.explosive_min_damage >= 0.0 { t.explosive_min_damage } else { def.min_damage };
        Self { centre, radius: def.radius * def.start_scale, age: 0.0, def, damage, min_damage, hit: Vec::new(), clock: 0.0 }
    }

    /// Runs it on by `dt` in whole updates of the game's, and says what its sphere touched
    /// and whether it is still there.
    pub fn step(&mut self, targets: &[melee::Target], dt: f32) -> (Vec<BlastHit>, bool) {
        let mut hits = Vec::new();
        self.clock += dt;
        while self.clock >= crate::sim::GAME_UPDATE {
            self.clock -= crate::sim::GAME_UPDATE;
            let step = crate::sim::GAME_UPDATE;
            let before = self.age;
            self.age += step;
            if self.age > self.def.time && before > self.def.time {
                return (hits, false);
            }
            self.radius = self.def.radius * self.def.scale(self.age, step);
            self.touch(targets, &mut hits);
        }
        (hits, true)
    }

    /// Its first update: the sphere at its starting size.
    pub fn first(&mut self, targets: &[melee::Target]) -> Vec<BlastHit> {
        let mut hits = Vec::new();
        self.touch(targets, &mut hits);
        hits
    }

    fn touch(&mut self, targets: &[melee::Target], hits: &mut Vec<BlastHit>) {
        let sphere = melee::Body { shape: crate::formats::scene::Shape::Sphere { radius: self.radius }, at: glam::Affine3A::from_translation(self.centre) };
        for target in targets {
            if self.hit.contains(&target.id) {
                continue;
            }
            let Some(point) = melee::touch(&sphere, &melee::Body::of_target(target)) else { continue };
            self.hit.push(target.id);
            hits.push(BlastHit {
                target: target.id,
                damage: self.def.damage_at(self.damage, self.min_damage, point.distance(self.centre), self.radius),
                point,
                direction: glam::Vec2::new(target.pos.x - self.centre.x, target.pos.y - self.centre.y).normalize_or_zero(),
                knock_back_speed: self.def.knock_back_speed,
                knock_back_angle: self.def.knock_back_angle.to_radians(),
                flags: self.def.damage_flags,
            });
        }
    }
}

/// The point a player's round is sent at: the aim point moved in the camera's picture
/// plane (`FUN_007aa210`). The most it moves is the distance from the camera to the aim
/// point times the tangent of half the camera's field of view times `playerNoiseRadius`,
/// so the scatter is the same size on screen at any range (his cannon's 0.01666 is a
/// sixtieth of half the picture's height). The way is an angle drawn evenly from the whole
/// turn along the camera's right and up, and how far a number drawn evenly from -1 to 1
/// times that most. [game 007aa210, read 2026-10-20] `a` and `b` are two numbers from 0 to 1, the
/// caller's random source; `fov` is in degrees, read by 00557aa0 (the scene camera's fov, through
/// tan(fov * 0.5) at 007aa210 l.26-27). The field of view is the camera's vertical one [game 00585880
/// l.42-57, settled 2026-10-20]: the view build (called by 00579c10 with the camera's fov, default 60)
/// takes t = tan(fov/2) (sin/cos, 00548390) and multiplies it by the view's +0x178 factor for the planes
/// leaning in x (+0x180, +0x188) only; the y planes take t alone.
/// Not in: a non-player's scatter (`generalNoiseRadius`, a disc of that radius across the
/// line from the muzzle), and no scatter while sniping.
pub fn scattered_point(target: Vec3, eye: Vec3, right: Vec3, up: Vec3, fov: f32, radius: f32, a: f32, b: f32) -> Vec3 {
    if radius <= 0.0 {
        return target;
    }
    let most = (target - eye).length() * (fov.to_radians() * 0.5).tan() * radius;
    let (sin, cos) = ((a * 2.0 - 1.0) * std::f32::consts::PI).sin_cos();
    target + (up * sin + right * cos) * ((b * 2.0 - 1.0) * most)
}

/// How far along the camera's line the aim point is looked for (`DAT_00cad46c`). [game]
pub const AIM_REACH: f32 = 500.0;

/// What the middle of the picture is on: the weapon manager's aim point (`+0x94`), the
/// surface's normal there (`+0xa0`, zero when nothing was hit) and how far it is from where
/// the line began (`+0x90`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    pub point: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

/// The aim point of a player in weapon mode, as the weapon manager works it out every
/// update (`FUN_007b2310`). The line runs along the camera's forward from the point of it
/// nearest to where he stands (`FUN_006fa7b0`: the camera itself if he is behind it, so
/// nothing between the camera and him is aimed at) to `AIM_REACH` from the camera. The aim
/// point is the first thing the line meets, or its far end. [game]
///
/// `body` is the character's position (`FUN_0071a2e0`: his object's, at his feet).
/// [stand-in: `world` is the arena; the original's trace is its collision query with mask 3,
/// leaves out his own bodies, and passes things flagged 0x1000 that are not characters]
/// settles: the game's trace callee under 007b2310 (its own-body exclusion and the 0x1000 flag), not read.
/// Not in: the aim out of weapon mode (the point keeps its distance along the line from
/// the muzzle), and `FUN_007b34a0`'s other answer while a game mode of type 768ce890,
/// d63697a6 or fc36eb23 runs (the muzzle plus the aim vector times 1000).
pub fn aim_point(eye: Vec3, forward: Vec3, body: Vec3, world: &dyn crate::camera::Sight) -> Aim {
    let far = eye + forward * AIM_REACH;
    let along = (body - eye).dot(forward).clamp(0.0, AIM_REACH);
    let from = eye + forward * along;
    // [game 007b2310] weapon_aim_point's line takes mask 3 (PUSH | PUNCH), `notes\collision-query-masks.md`.
    match world.sight_mask(from, far, crate::collision_masks::PUSH | crate::collision_masks::PUNCH) {
        Some(hit) => Aim { point: hit.point, normal: hit.normal, distance: (from - hit.point).length() },
        None => Aim { point: far, normal: Vec3::ZERO, distance: (from - far).length() },
    }
}

/// The line a gun round is traced along (`FUN_007832b0`): from the round's own place, the
/// MUZZLE, towards the scattered aim point, `bulletRange` long. With `tracerFirePerRound`
/// over 0 the whole of it is traced in the update the round is fired and the hit lands at
/// once; otherwise only `bulletSpeed` times the update's length, the round flying on from
/// there (not done: none of his guns is so). [game]
///
/// Corrected 2026-10-04: this was traced from the camera, on a misreading of the test the
/// routine makes before taking the camera's place for the start. That test
/// (`FUN_0117e860`) is "the character's sniper camera (`+0x1e0`) is the one in use", not
/// "a player's": only a SNIPING shot starts at the camera, and Bumblebee has no sniper
/// camera (`sniperCamera: 0`).
pub fn bullet_line(muzzle: Vec3, at: Vec3, t: &WeaponTuning) -> (Vec3, Vec3) {
    let way = (at - muzzle).normalize_or_zero();
    (muzzle, muzzle + way * t.bullet_range)
}

/// The visible projectile continues to travel even when tracerFirePerRound already
/// applied damage instantly (00782310). The async endpoint query is supplied by the host.
#[derive(Clone,Copy,Debug)]
pub struct BulletVisual {pub position:Vec3,pub end:Vec3,pub velocity:Vec3}
impl BulletVisual {
    pub fn new(from:Vec3,end:Vec3,speed:f32)->Self {
        Self {position:from,end,velocity:(end-from).normalize_or_zero()*speed}
    }
    /// Original endpoint crossing: delete once advanced position is beyond the hit
    /// along its velocity, rather than assigning a fitted trail lifetime. [game]
    pub fn step(&mut self,dt:f32)->bool {
        let next=self.position+self.velocity*dt;
        if self.velocity==Vec3::ZERO || (next-self.end).dot(self.velocity)>0.0 {return false;}
        self.position=next;true
    }
}

/// How far ahead of its barrel a weapon with `useFacingAsTarget` sends a round
/// (`DAT_00b3487c`). [game]
pub const FACING_REACH: f32 = 100.0;

/// What a round of a weapon with `useFacingAsTarget` is sent at: the point `FACING_REACH`
/// straight ahead of the barrel it leaves (`FUN_007aab70`: the barrel's place plus its
/// forward row times 100). The barrel's axes are the character's own outside weapon mode
/// (`FUN_007a7a40` without `useFireDummyMatrix`), so the car's gun shoots where the car
/// points, each barrel along its own line; the scatter is then laid on that point as on
/// any other (`FUN_007aa210`). [game]
pub fn facing_target(barrel: Vec3, forward: Vec3) -> Vec3 {
    barrel + forward * FACING_REACH
}

/// The weapon manager's wait before a car weapon may fire (its `+0xd0`). Changing to the
/// car's control mode (3) takes the car's weapon in hand and starts the wait at the
/// character's `vehicleWeaponTimeTilCanFire` (`FUN_007b43b0`, called by the control mode's
/// setter `FUN_0071ab70`); the manager's update counts it down (`FUN_007b1850`); and a
/// weapon with `isVehicleModeWeapon` gets the trigger only once it has run out and he is
/// in the car's form (`FUN_007b44e0`, asked by the manager's "held" and "released").
/// Leaving the car's mode puts the weapon he held before back in hand and zeroes the wait.
/// [game]
#[derive(Clone, Copy, Debug, Default)]
pub struct VehicleWait {
    left: f32,
    in_car: bool,
}

impl VehicleWait {
    /// Each frame: whether his control mode is the car's, and the wait a change into it
    /// starts. True on the frame the mode changes either way.
    pub fn step(&mut self, in_car: bool, wait: f32, dt: f32) -> bool {
        let changed = in_car != self.in_car;
        if changed {
            self.in_car = in_car;
            self.left = if in_car { wait } else { 0.0 };
        } else if self.left > 0.0 {
            self.left = (self.left - dt).max(0.0);
        }
        changed
    }

    /// Whether a car weapon gets the trigger.
    pub fn ready(&self) -> bool {
        self.in_car && self.left <= 0.0
    }

    pub fn left(&self) -> f32 {
        self.left
    }
}

/// The part of a character a round struck, by the collision body's surface (the hit's
/// `+0x254`: `Head`, `UpperBody`, `LowerBody`, `VehicleBody`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Head,
    UpperBody,
    LowerBody,
    VehicleBody,
    /// Anything else: no multiplier.
    Other,
}

/// A gun round's damage where it lands (`FUN_00782b30`): `damage` times the charge
/// (`charge`, 1 for a weapon without a charged shot), times `positionalDamageMultiplier`
/// for the part struck, and past `bulletDamageFallOffRange` (when that is not negative)
/// falling in a straight line to `minDamage` at `bulletRange`. `travelled` is from where
/// the round was made (the muzzle) to the hit. A head counts as a head only when the one
/// who fired is a player or of the second controller kind (`FUN_00711210`), or when no
/// character fired it (pass `by_player` true then); for anyone else it is an upper body.
/// [game] The blow pushes along the round's way with
/// `damage * 10`, and carries the data's `damageFlags` and `specialCaseDamageStuff`; a
/// round on a Head sets the packet's head-shot bit (`+0x30 | 1`, `Packet::headshot`)
/// whoever fired it. [game 00782b30]
pub fn bullet_damage(t: &WeaponTuning, charge: f32, part: Part, travelled: f32, by_player: bool) -> f32 {
    let mut damage = t.damage * charge;
    damage *= match part {
        Part::Head if by_player => t.positional[0],
        Part::Head | Part::UpperBody => t.positional[1],
        Part::LowerBody => t.positional[2],
        Part::VehicleBody => t.positional[3],
        Part::Other => 1.0,
    };
    let fall_off = t.bullet_fall_off_range;
    if fall_off >= 0.0 && travelled >= fall_off {
        damage = if travelled < t.bullet_range {
            t.min_damage + (1.0 - (travelled - fall_off) / (t.bullet_range - fall_off)) * (damage - t.min_damage)
        } else {
            t.min_damage
        };
    }
    damage
}

impl Part {
    /// The part a collision body's material names (`00782b30` compares the hit's `+0x64`, the
    /// body's `+0x254`, with these four hashes). [game]
    pub fn of_material(material: u32) -> Part {
        match material {
            0x07c1_59a2 => Part::Head,
            0x1f01_07fa => Part::UpperBody,
            0x7f5e_eed9 => Part::LowerBody,
            0x2a39_eb67 => Part::VehicleBody,
            _ => Part::Other,
        }
    }
}

/// The `Character` mask preset: what a character's limb boxes carry. It sends `PUNCH` (0x2),
/// the mask a gun round's query asks for (`world_ray_sphere_query(.., 2, ..)` in `007832b0`).
/// The hand spheres (`Attack`) and the explosion-only sphere do not send it. [data
/// global_collision #31, #39] [assumed] a primitive's own `sendMask` field does not override
/// its preset (the scout's limb boxes say 37, without 0x2); settles: the loader `005b9b33`'s
/// mask resolution.
const CHARACTER_PRESET: u32 = 0x118b_3297;

/// One collision body of a character a round can strike, with the part it counts as.
#[derive(Clone, Copy, Debug)]
pub struct PartBody {
    pub part: Part,
    pub body: melee::Body,
}

/// A character's bodies that rounds strike, posed: each `Collision` primitive of its object
/// with the `Character` preset, on its node of `pose` (the nodes in the object's space, as
/// `pose::Rig::pose` gives them), in the object's space. [game 007832b0] [data] The scout
/// has a Head box on node 22, eight UpperBody and six LowerBody boxes.
pub fn part_bodies(object: &crate::formats::scene::ObjectDef, pose: &[glam::Affine3A]) -> Vec<PartBody> {
    object
        .collisions
        .iter()
        .filter(|c| c.mask_preset == CHARACTER_PRESET && c.node < pose.len())
        .map(|c| PartBody { part: Part::of_material(c.material), body: melee::Body::new(c.shape, pose[c.node] * crate::pose::affine(&c.matrix)) })
        .collect()
}

/// How far along the line from `from` to `to` (0 to 1) it enters a posed body. None when it
/// misses. A line starting inside a body strikes it at 0 [game 005c1ed0, 005c3670, 005c6a50, run
/// in the emulator: see the test below]: the ray test's hit fraction starts at 0 and the hit is
/// taken with the start as its point. [game 005be890] Read: the query dispatches each shape by its
/// type through the ray table `PTR_FUN_00b8dd0c` (radius 0): 0 soup `005bcb30`, 1 sphere
/// `005c1ed0`, 2 cube `005c3670`, 3 capsule `005c5020`, 4 cylinder `005c6a50`, 5 cone `005c8480`,
/// 6 pencil `005c9eb0`, 7-10 hulls 32/16/8/4 (`005cb8e0`..`005d0320`). The sphere-sweep table
/// `PTR_FUN_00b8dd38` (radius nonzero) is not the port's need. Emulated (2026-10-20, the
/// `ray_shapes` setup, shape fill 1.0, ray from the origin up +z): sphere, cube, cylinder and
/// pencil strike at lambda 0; the cone and the capsule did not (the cone's shape data is a
/// degenerate fill here: [assumed, cone not ported] its inside miss is real, to be rerun with
/// real packet values; settles: rerun 005c8480 (cone) in the emulator with the packet's shape
/// data, once the cone is ported).
/// Not ported: the cone, the capsule and the hulls (the port's shapes are sphere, cuboid and
/// cylinder). All three ported tests open with the same zero-length test, `|d|^2 < 1e-4`, which
/// sets lambda -1 and returns the miss, inside or not [game 005c1ed0 (sphere, emulated),
/// 005c3670 (cube: fn.py lines 25-28, the test on |d|^2 with 0.0001), 005c6a50 (cylinder: fn.py
/// lines 25-27, the same test)]. The cuboid and cylinder keep 1e-12 for a parallel axis /
/// direction (a port choice in the slab and the cap tests: [assumed]. 2026-10-20: 005c3670 l.37-40
/// normalise `d` by its length (at least 0.01, so the division is safe); the GJK past l.60 is not
/// read. settles: 005c3670 past
/// line 40, its GJK ray cast's handling of a parallel direction, and 005c6a50's cap tests).
pub fn ray_enters(body: &melee::Body, from: Vec3, to: Vec3) -> Option<f32> {
    use crate::formats::scene::Shape;
    let inverse = body.at.inverse();
    let (o, d) = (inverse.transform_point3(from), inverse.transform_vector3(to - from));
    // A line shorter than 0.01 m misses, inside or not [game 005c1ed0, 005c3670, 005c6a50].
    if d.length_squared() < 1e-4 {
        return None;
    }
    let k = match body.shape {
        Shape::Sphere { radius } => {
            let a = d.length_squared();
            let b = o.dot(d);
            let c = o.length_squared() - radius * radius;
            // Inside: the start is the hit [game 005c1ed0, emulated].
            if c <= 0.0 {
                return Some(0.0);
            }
            let root = b * b - a * c;
            if root < 0.0 {
                return None;
            }
            (-b - root.sqrt()) / a
        }
        Shape::Cuboid { half } => {
            let (mut enter, mut exit) = (f32::NEG_INFINITY, f32::INFINITY);
            for axis in 0..3 {
                let (o, d, h) = (o[axis], d[axis], half[axis]);
                if d.abs() <= 1e-12 {
                    if o.abs() > h {
                        return None;
                    }
                    continue;
                }
                let (k1, k2) = ((-h - o) / d, (h - o) / d);
                enter = enter.max(k1.min(k2));
                exit = exit.min(k1.max(k2));
            }
            if enter > exit || exit < 0.0 {
                return None;
            }
            // Inside the box the enter is behind the start: the hit is the start [game 005c3670,
            // emulated].
            enter.max(0.0)
        }
        Shape::Cylinder { radius, half_height } => {
            // Inside the cylinder the start is the hit [game 005c6a50, emulated].
            if o.truncate().length_squared() <= radius * radius && o.z.abs() <= half_height {
                return Some(0.0);
            }
            let mut best: Option<f32> = None;
            let (a, b) = (d.truncate().length_squared(), o.truncate().dot(d.truncate()));
            let c = o.truncate().length_squared() - radius * radius;
            if a > 1e-12 && b * b - a * c >= 0.0 {
                let k = (-b - (b * b - a * c).sqrt()) / a;
                if (o.z + d.z * k).abs() <= half_height {
                    best = Some(k);
                }
            }
            if d.z.abs() > 1e-12 {
                for cap in [-half_height, half_height] {
                    let k = (cap - o.z) / d.z;
                    if (o.truncate() + d.truncate() * k).length() <= radius && best.is_none_or(|b| k < b) {
                        best = Some(k);
                    }
                }
            }
            best?
        }
    };
    (0.0..=1.0).contains(&k).then_some(k)
}

/// Where a round's line strikes one character's bodies (`bodies`, all posed in the same space
/// as the line).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartHit {
    /// The share of the line where it first enters any of them: what orders the character
    /// against the world and the others.
    pub first: f32,
    /// The share where the hit is taken, and its part.
    pub at: f32,
    pub part: Part,
}

/// The hit a round's line takes on one character (`007832b0`'s walk over the query's hits,
/// sorted by distance): the nearest of the character's bodies, unless a Head body lies
/// anywhere along the line through that same character, which then takes it (the walk goes
/// on over the first character's hits while the kept one is not a Head, and takes a later
/// one when it is a Head). [game 007832b0 l.137-172]
pub fn part_struck(from: Vec3, to: Vec3, bodies: &[PartBody]) -> Option<PartHit> {
    let mut hits: Vec<(f32, Part)> = bodies.iter().filter_map(|b| ray_enters(&b.body, from, to).map(|k| (k, b.part))).collect();
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    let &(first, nearest) = hits.first()?;
    let (at, part) = hits.iter().copied().find(|&(_, part)| part == Part::Head).unwrap_or((first, nearest));
    Some(PartHit { first, at, part })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROSTER: [&str; 13] = ["bumblebee", "optimus", "ratchet", "megatron", "longhaul", "firestorm", "blackout", "audir8",
        "starscream", "jetfire", "ironhide", "optimusflight", "megatronflight"];

    fn pack(name: &str) -> crate::character::CharacterData {
        crate::character::load(crate::install::dir(), name).unwrap()
    }

    fn hashes(object: &crate::formats::scene::ObjectDef) -> Vec<u32> {
        object.nodes.iter().map(|n| n.name_hash).collect()
    }

    fn crc(name: &str) -> u32 {
        crate::formats::hash::crc32(name.as_bytes())
    }

    /// The barrel names are the table at `0xc0675c`. [game 007a7a40]
    #[test]
    fn the_barrel_names_are_the_games_table() {
        let names = ["FireDummy", "FireDummy01", "FireDummy02", "FireDummy03", "FireDummy04", "FireDummy05", "FireDummy06",
            "FireDummy07", "FireDummy08", "FireDummy09"];
        for (i, name) in names.iter().enumerate() {
            assert_eq!(crc(name), FIRE_DUMMIES[i], "{name}");
        }
        assert_eq!(WeaponDef { first_barrel: 1, ..WeaponDef::test(Kind::Gun) }.barrel_node(), 0xa82b_3c5a);
        assert_eq!(WeaponDef { first_barrel: 40, ..WeaponDef::test(Kind::Gun) }.barrel_node(), 0);
    }

    /// The robot is looked in first, then the car. [game 007a6380]
    #[test]
    fn a_weapon_hangs_on_the_robots_node_else_on_the_cars() {
        assert_eq!(mount(7, &[7], Some(&[7])), Some(Mount::Robot(0)));
        assert_eq!(mount(7, &[3, 4], Some(&[1, 7])), Some(Mount::Car(1)));
        assert_eq!(mount(7, &[3, 4], None), None);
        assert_eq!(mount(7, &[3, 4], Some(&[1])), None);
    }

    /// Every car-form special's node is a node of the car, not of the robot; the foot-form ones
    /// are the robot's. [data]
    #[test]
    fn car_form_specials_hang_on_the_car_and_foot_ones_on_the_robot() {
        if crate::install::missing() { return; }
        let cases: [(&str, &str, bool); 9] = [
            ("megatron", "MegatronSpecialVehicleWeapon", true),
            ("megatron", "Megatron_VehicleGun", true),
            ("megatron", "MegatronSpecialGun", false),
            ("audir8", "AudiR8VehicleSpecialMine", true),
            ("audir8", "AudiR8SpecialMine", false),
            ("blackout", "Blackout_VehicleSpecial", true),
            ("ironhide", "Ironhide_VehicleSpecial", true),
            ("jetfire", "JetFireCarpetBomb", true),
            ("jetfire", "JetFireMissileDump", false),
        ];
        for (name, weapon, on_car) in cases {
            let data = pack(name);
            let def = data.weapons.iter().find(|d| d.name == crc(weapon)).unwrap_or_else(|| panic!("{name}: no {weapon}"));
            let car = data.vehicle.as_ref().map(hashes);
            let found = mount(def.bone, &hashes(&data.robot), car.as_deref());
            assert!(matches!(found, Some(Mount::Car(_))) == on_car && found.is_some(), "{name} {weapon}: {found:?}");
        }
        let megatron = pack("megatron");
        let car = megatron.vehicle.as_ref().map(hashes);
        assert!(matches!(mount(crc("Cannon_Tip"), &hashes(&megatron.robot), car.as_deref()), Some(Mount::Car(_))));
        assert!(matches!(mount(crc("WeaponAttachment_Hand_R"), &hashes(&megatron.robot), car.as_deref()), Some(Mount::Robot(_))));
        let jetfire = pack("jetfire");
        let car = jetfire.vehicle.as_ref().map(hashes);
        assert!(matches!(mount(crc("Spine_01"), &hashes(&jetfire.robot), car.as_deref()), Some(Mount::Robot(_))));
    }

    /// Megatron's car beam leaves from Cannon_Tip's FireDummy on the car (rest pose), far from
    /// any robot hand point. [game 007a6380, 007a7a40] [data]
    #[test]
    fn megatrons_car_beam_leaves_from_the_cannon_tip() {
        if crate::install::missing() { return; }
        let data = pack("megatron");
        let def = data.weapons.iter().find(|d| d.name == crc("MegatronSpecialVehicleWeapon")).unwrap();
        let vehicle = data.vehicle.as_ref().unwrap();
        let Some(Mount::Car(node)) = mount(def.bone, &hashes(&data.robot), Some(&hashes(vehicle))) else { panic!() };
        let tip = crate::pose::Rig::new(vehicle).rest()[node];
        let muzzle = tip.transform_point3(def.muzzle);
        println!("Megatron's car beam at rest, car space: {muzzle:?} (Cannon_Tip at {:?})", tip.translation);
        assert!((muzzle - Vec3::from(tip.translation)).length() > 1.0, "1.06 m out along the tip");
        let robot = hashes(&data.robot);
        let hand = robot.iter().position(|&h| h == crc("WeaponAttachment_Hand_R")).unwrap();
        let hand = crate::pose::Rig::new(&data.robot).rest()[hand].transform_point3(def.muzzle);
        assert!((muzzle - hand).length() > 0.5, "not the robot hand's point {hand:?}");
    }

    /// His cannon's round leaves from its FireDummy on `WeaponAttachment_Arm_R` (where the arm
    /// clips' events link the object), not on `ObjectAttachment_Hand_R` (`startBoneAttachment`).
    /// [game 007b4f20] [data: `WeaponSet/Weapon_Idle` tick 0]
    #[test]
    fn bumblebees_cannon_fires_from_the_forearm_node() {
        if crate::install::missing() { return; }
        let data = pack("bumblebee");
        let cannon = data.weapons.iter().find(|d| d.name == crc("SolarPlasmaCannon")).unwrap();
        assert_eq!(cannon.bone, crc("ObjectAttachment_Hand_R"));
        let r = data.animations.find("WeaponSet", "Weapon_Idle").unwrap();
        let pose = crate::pose::Rig::new(&data.robot).pose(r.clip.as_ref().unwrap(), r.looping, 0.0);
        let at = |node: &str| {
            let i = data.robot.nodes.iter().position(|n| n.name_hash == crc(node)).unwrap();
            pose[i].transform_point3(cannon.muzzle)
        };
        let (arm, hand) = (at("WeaponAttachment_Arm_R"), at("ObjectAttachment_Hand_R"));
        println!("cannon muzzle on Arm_R {arm:?}, on Hand_R {hand:?}");
        assert!((arm - Vec3::new(0.60, 3.62, 4.33)).length() < 0.05, "{arm:?}");
        // The hand is folded away to 0.24 of its size in the weapon stance (`Hand_R`'s scale,
        // stream type 0x41), which shrinks the offset on its attachment. [data] (Before the own
        // scale of stream 0x42 had a track of its own, its keys of one were mixed into this one.)
        assert!((hand - Vec3::new(0.59, 2.80, 4.24)).length() < 0.05, "{hand:?}");
        // The pull-out's event for the cannon (by name, or by its slot 0) sits on Arm_R: the
        // node the port's `hang_node` hangs the model on.
        let arm_set = data.animations.sets.get("WeaponPartialSet").unwrap();
        let pull_out = arm_set.iter().find(|r| r.id == "Trans_R2W").and_then(|r| r.clip.as_ref()).unwrap();
        let event = pull_out.weapon_events.iter().find(|e| match e.kind {
            crate::formats::anim::WeaponEventKind::Attach { weapon, index, .. } => weapon == cannon.name || (weapon == 0 && index == 0),
            _ => false,
        });
        assert_eq!(event.map(|e| e.node), Some(crc("WeaponAttachment_Arm_R")));
    }

    /// Only OptimusFlight's two guns always appear. [data]
    #[test]
    fn only_optimus_flights_guns_always_appear() {
        if crate::install::missing() { return; }
        for name in ROSTER {
            for def in pack(name).weapons {
                assert_eq!(def.always_appear, name == "optimusflight", "{name} {:#010x}", def.name);
            }
        }
        let flight = pack("optimusflight");
        for weapon in ["PrimeFlightLaserRifle", "OptimusFlightShellCannon"] {
            assert!(flight.weapons.iter().any(|d| d.name == crc(weapon) && d.always_appear), "{weapon}");
        }
    }

    /// The charged guns' object clips and effects, and OptimusFlight's snaker round. [data]
    #[test]
    fn the_charged_guns_read_their_clips_effects_and_snaker() {
        if crate::install::missing() { return; }
        let optimus = pack("optimus");
        let cannon = optimus.weapons.iter().find(|d| d.name == crc("ShellCannon")).expect("ShellCannon");
        assert_eq!(cannon.charging_clip, crc("WeaponB_Charging"));
        assert_eq!(cannon.charging_clip, -85286767i32 as u32);
        assert_eq!(cannon.charged_clip, -1500044211i32 as u32);
        let fx = cannon.fx.as_deref().expect("a gun's charge effects");
        assert!(fx.charge.is_some() && fx.charge_ready.is_some());
        assert_eq!(cannon.alt_snaker_check, None);
        let left = optimus.weapons.iter().find(|d| d.name == crc("ShellCannonL")).expect("ShellCannonL");
        assert!(left.fx.as_deref().is_some_and(|fx| fx.charge.is_some()) && left.charging_clip == cannon.charging_clip);
        let flight = pack("optimusflight");
        let gun = flight.weapons.iter().find(|d| d.name == crc("OptimusFlightShellCannon")).expect("flight cannon");
        assert_eq!(gun.alt_snaker_check, Some(3.0));
        for name in ["megatron", "megatronflight"] {
            let data = pack(name);
            let tunings = load(crate::install::dir(), &data.weapons).unwrap();
            let charged: Vec<&WeaponDef> = data.weapons.iter().zip(&tunings).filter(|(_, t)| t.charge.has).map(|(d, _)| d).collect();
            assert!(!charged.is_empty() && charged.iter().all(|d| d.fx.as_deref().is_some_and(|fx| fx.charge.is_some())), "{name}");
        }
        // MegatronFlight's other gun names the clips but has no charged shot, so they never play.
        let data = pack("megatronflight");
        let tunings = load(crate::install::dir(), &data.weapons).unwrap();
        let named = data.weapons.iter().zip(&tunings).find(|(d, _)| d.name == 0x5274_4033);
        if let Some((def, t)) = named {
            assert!(def.charging_clip != 0 && !t.charge.has);
        }
    }

    #[test]
    fn the_attribute_root_follows_the_owner_and_the_game_then_falls_back_to_the_kinds_default() {
        // Bumblebee's cannon as his pack lists it (bumblebee.txt 546565..546567).
        let def = WeaponDef { attribute: 1887510828, mp_attribute: -1942435432i32 as u32, ai_attribute: -271024620i32 as u32, ..WeaponDef::test(Kind::Gun) };
        assert_eq!(def.attribute_for(true, false), def.attribute);
        assert_eq!(def.attribute_for(false, false), def.ai_attribute);
        assert_eq!(def.attribute_for(true, true), def.mp_attribute);
        assert_eq!(def.attribute_for(false, true), def.ai_attribute);
        // His car's gun: gunAIAttribute 0, so the AI takes the plain one (or the MP one).
        let car = WeaponDef { ai_attribute: 0, ..def.clone() };
        assert_eq!(car.attribute_for(false, false), def.attribute);
        assert_eq!(car.attribute_for(false, true), def.mp_attribute);
        // The flamethrower has no AI or MP choice in 007abf40.
        let flame = WeaponDef { kind: Kind::Flame, ..def.clone() };
        assert_eq!(flame.attribute_for(false, true), def.attribute);
        assert_eq!(default_attribute(Kind::Gun), 0x378d8e76);
        assert_eq!(default_attribute(Kind::Launcher), 0x5657963d);
        assert_eq!(default_attribute(Kind::Grenade), 0x3793d80e);
        assert_eq!(default_attribute(Kind::Flame), 0x7dd845f0);
        assert_eq!(default_attribute(Kind::Turret), 0x55082aad);
    }

    #[test]
    fn a_field_the_data_leaves_out_keeps_the_constructors_value() {
        assert_eq!(attribute_default("rateOfFire", Kind::Launcher), Some(1.0));
        assert_eq!(attribute_default("heatMeterCapacity", Kind::Grenade), Some(100.0));
        assert_eq!(attribute_default("heatMeterOverHeatCooldownRate", Kind::Gun), Some(-1.0));
        assert_eq!(attribute_default("chargeHoldDuration", Kind::Gun), Some(-1.0));
        assert_eq!(attribute_default("bulletRange", Kind::Gun), Some(1000.0));
        assert_eq!(attribute_default("tracerFirePerRound", Kind::Gun), Some(-1.0));
        // The gun's own fields are only a gun's.
        assert_eq!(attribute_default("bulletRange", Kind::Launcher), None);
        assert_eq!(attribute_default("missileTopSpeed", Kind::Launcher), Some(100.0));
        assert_eq!(attribute_default("guided", Kind::Gun), None);
        assert_eq!(attribute_default("missileLockOnBoxWidth", Kind::Launcher), Some(128.0));
        // The cone's default is converted twice, the constructor's cosine taken as degrees.
        let cone = attribute_default("pastTargetConeAngleActivated", Kind::Launcher).unwrap().to_radians().cos();
        assert!((cone - 0.999_886).abs() < 1e-5, "{cone}");
    }

    /// `read_defs`, `read_objects` and `read_object_clips` all take their weapons from
    /// `WEAPON_CLASSES`: every listed class has a kind (so `read_defs` reads it), the five
    /// kinds come in the order `read_defs` gives them, and a non-weapon class is left out.
    /// (Before the shared list, `read_objects` and `read_object_clips` skipped the grenade
    /// launcher and the flamethrower, so Long Haul's objects were one short of his weapons.)
    #[test]
    fn weapon_lists_share_one_set_of_classes() {
        for name in WEAPON_CLASSES {
            assert!(weapon_class(name).is_some(), "{name} is listed but read_defs does not read it");
        }
        let kinds: Vec<Kind> = WEAPON_CLASSES.iter().filter_map(|name| weapon_class(name)).map(|(kind, _)| kind).collect();
        assert_eq!(kinds, vec![Kind::Gun, Kind::Launcher, Kind::Grenade, Kind::Flame, Kind::Turret]);
        assert!(weapon_class("AnimRefBundle").is_none());
    }

    #[test]
    fn visible_tracer_moves_at_data_speed_and_ends_only_after_crossing() {
        let mut v=BulletVisual::new(Vec3::ZERO,Vec3::Y*64.0,1000.0);
        assert!(v.step(0.032));assert_eq!(v.position,Vec3::Y*32.0);
        assert!(v.step(0.032));assert_eq!(v.position,Vec3::Y*64.0);
        assert!(!v.step(0.032));
    }

    /// Bumblebee's two robot weapons, as the game's data has them.
    fn cannon() -> WeaponTuning {
        WeaponTuning {
            rate_of_fire: 0.15,
            automatic_firing: true,
            projectiles_per_fire: 1,
            heat_capacity: 100.0,
            heat_per_round: 4.2,
            heat_decrement_per_second: 38.0,
            heat_cooldown_til_decrement: 0.4,
            overheat_period: 5.0,
            player_noise_radius: 0.01666,
            bullet_speed: 1000.0,
            bullet_range: 300.0,
            damage: 15.0,
            bullet_fall_off_range: -1.0,
            min_damage: 1.0,
            tracer_fire_per_round: 1,
            positional: [2.0, 1.0, 1.0, 1.0],
            ..Default::default()
        }
    }

    #[test]
    fn a_sniping_shot_takes_its_steady_and_sets_the_clock_back() {
        // The Audi's PlasmaLongRifle: one round a press, 0.9 s to start, 0.9 s to full. [data]
        let t = WeaponTuning {
            automatic_firing: false,
            snipe: crate::snipe::SnipeTuning { camera_enabled: true, max_damage: 200.0, charge_start: 0.9, charge_duration: 0.9 },
            damage: 100.0,
            ..cannon()
        };
        let mut w = Weapon::default();
        w.set_sniping(true);
        // 1 s sniping without firing: past the 0.9 s to full from 0.
        assert_eq!(w.step(&t, false, 1.0), Fired::Nothing);
        assert_eq!(w.snipe_steady(&t), 1.0);
        assert!(matches!(w.step(&t, true, crate::sim::GAME_UPDATE), Fired::Rounds(1)));
        assert_eq!(w.snipe_steady_shot, 1.0);
        // The shot set the clock to -0.9, and the update after it ran one tick.
        assert!((w.snipe_clock - (-0.9 + crate::sim::GAME_UPDATE)).abs() < 1e-5);
        assert_eq!(w.snipe_steady(&t), 0.0);
        // Out of sniping the steady is 0 and a shot leaves the clock alone.
        w.set_sniping(false);
        assert_eq!(w.step(&t, false, 2.0), Fired::Nothing);
        assert_eq!(w.snipe_clock, 0.0);
        assert!(matches!(w.step(&t, true, crate::sim::GAME_UPDATE), Fired::Rounds(1)));
        assert_eq!((w.snipe_steady_shot, w.snipe_clock), (0.0, 0.0));
    }

    #[test]
    fn the_snipe_clock_ignores_the_heat_and_runs_only_in_hand() {
        // [game 007a7470] "may fire" is "in hand": a rifle at its heat capacity still runs it.
        let t = WeaponTuning {
            automatic_firing: false,
            snipe: crate::snipe::SnipeTuning { camera_enabled: true, max_damage: 200.0, charge_start: 0.9, charge_duration: 0.9 },
            damage: 100.0,
            ..cannon()
        };
        let mut w = Weapon::default();
        w.heat = t.heat_capacity;
        w.set_sniping(true);
        w.step(&t, false, crate::sim::GAME_UPDATE);
        assert!(w.snipe_clock > 0.0);
        // Sniping, but this weapon is not in hand: the caller gives false; the clock stays at 0.
        let mut other = Weapon::default();
        other.set_sniping(crate::snipe::clock_runs(true, false, false, false));
        for _ in 0..20 {
            other.step(&t, false, crate::sim::GAME_UPDATE);
        }
        assert!(other.snipe_clock <= 0.0);
    }

    /// A wall across y = 40, for the aim's line.
    struct Wall;

    impl crate::camera::Sight for Wall {
        fn sight(&self, from: Vec3, to: Vec3) -> Option<crate::camera::Hit> {
            (from.y < 40.0 && to.y >= 40.0)
                .then(|| crate::camera::Hit { point: from + (to - from) * ((40.0 - from.y) / (to.y - from.y)), normal: Vec3::NEG_Y, ..Default::default() })
        }
        fn sweep(&self,from:Vec3,to:Vec3,radius:f32)->Option<crate::camera::Hit> {
            let plane=40.0-radius;
            (from.y<plane && to.y>=plane).then(||crate::camera::Hit {
                point:from+(to-from)*((plane-from.y)/(to.y-from.y)),normal:Vec3::NEG_Y,..Default::default()
            })
        }
    }

    /// Flat ground at z 0 that ends at y 30 (a drop), with a wall at y 20 when `wall`.
    struct Ground {
        wall: bool,
    }

    impl crate::camera::Sight for Ground {
        fn sight(&self, from: Vec3, to: Vec3) -> Option<crate::camera::Hit> {
            if self.wall && (from.y - 20.0) * (to.y - 20.0) <= 0.0 && from.y != to.y {
                let f = (20.0 - from.y) / (to.y - from.y);
                return Some(crate::camera::Hit { point: from + (to - from) * f, normal: Vec3::NEG_Y, ..Default::default() });
            }
            if from.z > 0.0 && to.z <= 0.0 {
                let p = from + (to - from) * (from.z / (from.z - to.z));
                return (p.y < 30.0).then(|| crate::camera::Hit { point: p, normal: Vec3::Z, ..Default::default() });
            }
            None
        }
        fn sweep(&self, from: Vec3, to: Vec3, _radius: f32) -> Option<crate::camera::Hit> {
            self.sight(from, to)
        }
    }

    #[test]
    fn a_snaker_rests_slides_along_the_ground_and_ends_at_a_wall_a_drop_or_its_time() {
        let t = SnakerTuning { speed: 60.0, damage: 100.0, time_til_dies: 1.75, damage_flags: 0 };
        // Spawned just above the ground: the first check puts it on it, normal up.
        let mut s = Snaker::activate(Vec3::new(0.0, 0.0, 0.5), Vec3::new(0.0, 1.0, -0.5), 1.0, &Ground { wall: true });
        assert!(s.grounded && s.settled && s.position.z == 0.0);
        s.step(&t, &Ground { wall: true }, DT);
        // The first update starts the slide: its way is made level with the ground.
        assert!(s.sliding && (s.direction - Vec3::Y).length() < 1e-6, "{:?}", s.direction);
        let mut steps = 0;
        while s.alive && steps < 100 {
            s.step(&t, &Ground { wall: true }, DT);
            steps += 1;
        }
        // 60 m/s from y 0: the wall at y 20 ends it on the update whose reach meets it.
        assert!(!s.alive && s.position.y > 18.0 && s.position.y < 20.0, "{} {}", s.position, steps);
        // No wall: it runs off the edge at y 30.
        let mut s = Snaker::activate(Vec3::new(0.0, 0.0, 0.5), Vec3::Y, 1.0, &Ground { wall: false });
        while s.alive {
            s.step(&t, &Ground { wall: false }, DT);
        }
        assert!(s.position.y < 30.0 && s.position.y > 27.0 && s.age < 1.75, "{}", s.position);
        // In the air it is removed after three updates.
        let mut s = Snaker::activate(Vec3::new(0.0, 0.0, 5.0), Vec3::Y, 1.0, &Ground { wall: false });
        for _ in 0..3 {
            s.step(&t, &Ground { wall: false }, DT);
        }
        assert!(!s.alive && s.idle_updates == 3);
        // A touch of a slope keeps the most upward normal; 0.99 up settles it.
        let mut s = Snaker::activate(Vec3::new(0.0, 0.0, 5.0), Vec3::Y, 1.0, &Ground { wall: false });
        s.normal = Vec3::ZERO;
        s.touch_world(Vec3::new(0.0, -0.6, 0.8));
        assert!(s.grounded && !s.settled && (s.normal.z - 0.8).abs() < 1e-6);
        let (damage, push) = s.touch_character(&t).unwrap();
        assert!(damage == 100.0 && (push - Vec3::Y * 10_000.0).length() < 1e-3);
    }

    /// Jetfire's wave: `snakerInfo.timeTilDies` 5.0, but its object's TimeoutDeleter (1.0 s) takes
    /// the object away first. [data: jetfire 649524] [game 00776f50]
    #[test]
    fn a_wave_ends_with_its_objects_deleter_when_that_comes_first() {
        let t = SnakerTuning { speed: 10.0, damage: 100.0, time_til_dies: 5.0, damage_flags: 0 };
        let mut s = Snaker::activate(Vec3::new(0.0, 0.0, 0.5), Vec3::Y, 3.0, &Ground { wall: false }).with_life(1.0);
        let mut age = 0.0;
        while age < 0.95 {
            s.step(&t, &Ground { wall: false }, DT);
            age += DT;
        }
        assert!(s.alive, "still sliding at {age}");
        while age < 1.1 {
            s.step(&t, &Ground { wall: false }, DT);
            age += DT;
        }
        assert!(!s.alive && s.age < 1.1, "{}", s.age);
    }

    #[test]
    fn the_aim_point_is_what_the_cameras_line_meets_beyond_him() {
        // The camera 10 behind and 4 above him, looking level along +y.
        let (eye, forward, body) = (Vec3::new(0.0, -10.0, 4.0), Vec3::Y, Vec3::ZERO);
        // Nothing in the way: 500 from the camera, measured from beside him.
        let open = aim_point(eye, forward, body, &crate::camera::Open);
        assert_eq!(open.point, Vec3::new(0.0, 490.0, 4.0));
        assert_eq!((open.normal, open.distance), (Vec3::ZERO, 490.0));
        // A wall 40 ahead of him.
        let wall = aim_point(eye, forward, body, &Wall);
        assert_eq!(wall.point, Vec3::new(0.0, 40.0, 4.0));
        assert_eq!((wall.normal, wall.distance), (Vec3::NEG_Y, 40.0));
        // The line starts beside him, not at the camera: a wall between the two is not
        // aimed at.
        let behind = aim_point(Vec3::new(0.0, 30.0, 4.0), forward, Vec3::new(0.0, 45.0, 0.0), &Wall);
        assert_eq!(behind.point, Vec3::new(0.0, 530.0, 4.0));
        // With him behind the camera it starts at the camera.
        let ahead = aim_point(Vec3::new(0.0, 30.0, 4.0), forward, Vec3::new(0.0, 20.0, 0.0), &Wall);
        assert_eq!(ahead.point, Vec3::new(0.0, 40.0, 4.0));
    }

    #[test]
    fn a_round_is_traced_from_the_muzzle_and_hurts_by_the_part_and_the_range() {
        let mut t = cannon();
        // From the muzzle towards the point it was sent at, over the range.
        let (from, to) = bullet_line(Vec3::new(0.0, -10.0, 4.0), Vec3::new(0.0, 40.0, 4.0), &t);
        assert_eq!((from, to), (Vec3::new(0.0, -10.0, 4.0), Vec3::new(0.0, 290.0, 4.0)));
        // His cannon: 15, twice that on a head, with no fall-off.
        assert_eq!(bullet_damage(&t, 1.0, Part::UpperBody, 250.0, true), 15.0);
        assert_eq!(bullet_damage(&t, 1.0, Part::Head, 250.0, true), 30.0);
        // A head is an upper body to a round that is not a player's.
        assert_eq!(bullet_damage(&t, 1.0, Part::Head, 250.0, false), 15.0);
        // With a fall-off from 100: half way to the range, half way down to `minDamage`.
        t.bullet_fall_off_range = 100.0;
        assert_eq!(bullet_damage(&t, 1.0, Part::Other, 99.0, true), 15.0);
        assert_eq!(bullet_damage(&t, 1.0, Part::Other, 200.0, true), 8.0);
        assert_eq!(bullet_damage(&t, 1.0, Part::Other, 300.0, true), 1.0);
    }

    #[test]
    fn a_head_along_the_line_takes_the_round_from_a_nearer_body() {
        let boxed = |part, x: f32| PartBody { part, body: melee::Body::new(crate::formats::scene::Shape::Cuboid { half: [0.5; 3] }, glam::Affine3A::from_translation(Vec3::new(x, 0.0, 0.0))) };
        let (from, to) = (Vec3::new(-10.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0));
        // The chest first, the head behind it: the head takes it, where the line enters it.
        let hit = part_struck(from, to, &[boxed(Part::UpperBody, 0.0), boxed(Part::Head, 3.0)]).unwrap();
        assert_eq!((hit.first, hit.at, hit.part), (0.475, 0.625, Part::Head));
        // Without a head the nearest body.
        let hit = part_struck(from, to, &[boxed(Part::LowerBody, 3.0), boxed(Part::UpperBody, 0.0)]).unwrap();
        assert_eq!((hit.at, hit.part), (0.475, Part::UpperBody));
        assert!(part_struck(from, Vec3::new(-10.0, 5.0, 0.0), &[boxed(Part::Head, 0.0)]).is_none());
        // Spheres and cylinders are entered too.
        let ball = melee::Body::new(crate::formats::scene::Shape::Sphere { radius: 1.0 }, glam::Affine3A::IDENTITY);
        assert_eq!(ray_enters(&ball, from, to), Some(0.45));
        // A line starting inside strikes at 0 (the start), as the game's ray test does [emulated
        // 2026-10-20: sphere, cube, cylinder, pencil]. The ray from the origin out to +x leaves it.
        assert_eq!(ray_enters(&ball, Vec3::ZERO, to), Some(0.0));
        // A zero-length line misses, even at the centre, and so does one shorter than 0.01 m
        // that crosses the surface [game 005c1ed0: |d|^2 < 1e-4 returns the miss before any test].
        assert_eq!(ray_enters(&ball, Vec3::ZERO, Vec3::ZERO), None);
        assert_eq!(ray_enters(&ball, Vec3::new(-1.003, 0.0, 0.0), Vec3::new(-0.996, 0.0, 0.0)), None);
        // The cube and the cylinder run the same zero-length test first [game 005c3670 l.25-28,
        // 005c6a50 l.25-28]: a point inside them is a miss, not a hit at 0.
        let cube_at_origin = melee::Body::new(crate::formats::scene::Shape::Cuboid { half: [1.0; 3] }, glam::Affine3A::IDENTITY);
        assert_eq!(ray_enters(&cube_at_origin, Vec3::ZERO, Vec3::ZERO), None);
        assert_eq!(ray_enters(&cube_at_origin, Vec3::new(0.999, 0.0, 0.0), Vec3::new(1.003, 0.0, 0.0)), None);
        let can_at_origin = melee::Body::new(crate::formats::scene::Shape::Cylinder { radius: 1.0, half_height: 2.0 }, glam::Affine3A::IDENTITY);
        assert_eq!(ray_enters(&can_at_origin, Vec3::ZERO, Vec3::ZERO), None);
        assert_eq!(ray_enters(&can_at_origin, Vec3::new(0.999, 0.0, 0.0), Vec3::new(1.003, 0.0, 0.0)), None);
        let can =melee::Body::new(crate::formats::scene::Shape::Cylinder { radius: 1.0, half_height: 2.0 }, glam::Affine3A::IDENTITY);
        assert_eq!(ray_enters(&can, from, to), Some(0.45));
        assert_eq!(ray_enters(&can, Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -10.0)), Some(0.4));
        assert_eq!(ray_enters(&can, Vec3::ZERO, Vec3::new(0.0, 0.0, 10.0)), Some(0.0));
        let boxed_at_origin = melee::Body::new(crate::formats::scene::Shape::Cuboid { half: [1.0; 3] }, glam::Affine3A::IDENTITY);
        assert_eq!(ray_enters(&boxed_at_origin, Vec3::ZERO, Vec3::new(5.0, 0.0, 0.0)), Some(0.0));
        // A box behind the line is not struck.
        assert_eq!(ray_enters(&boxed_at_origin, Vec3::new(3.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 0.0)), None);
    }

    /// The scout's own bodies at rest (`dcomscout.str`): a round through its Head box is a Head
    /// with the head multiplier for the player, an upper body for an AI's round; through the chest
    /// none. [data] [game 00782b30, 007832b0]
    #[test]
    fn a_round_through_the_scouts_head_box_is_a_head_shot() {
        if crate::install::missing() { return; }
        let scout = crate::character::load(crate::install::dir(), "dcomscout").expect("installed scout");
        let object = &scout.robot;
        let pose = crate::pose::Rig::new(object).rest();
        let bodies = part_bodies(object, &pose);
        assert_eq!(bodies.iter().filter(|b| b.part == Part::Head).count(), 1);
        let head = bodies.iter().find(|b| b.part == Part::Head).unwrap();
        let head_node = object.collisions.iter().find(|c| Part::of_material(c.material) == Part::Head).unwrap().node;
        assert_eq!(head_node, 22);
        let chest = object.collisions.iter().position(|c| c.node == 7 && c.mask_preset == CHARACTER_PRESET).unwrap();
        let chest = melee::Body::new(object.collisions[chest].shape, pose[7] * crate::pose::affine(&object.collisions[chest].matrix));
        let (head_at, chest_at) = (Vec3::from(head.body.at.translation), Vec3::from(chest.at.translation));
        assert!(head_at.z > chest_at.z + 0.5, "head {head_at} above chest {chest_at}");
        let t = cannon();
        let across = |at: Vec3| part_struck(at - Vec3::X * 20.0, at + Vec3::X * 20.0, &bodies);
        let hit = across(head_at).expect("the head line strikes");
        assert_eq!(hit.part, Part::Head);
        assert_eq!(bullet_damage(&t, 1.0, hit.part, 20.0, true), t.damage * t.positional[0]);
        assert_eq!(bullet_damage(&t, 1.0, hit.part, 20.0, false), t.damage * t.positional[1]);
        let hit = across(chest_at).expect("the chest line strikes");
        assert_ne!(hit.part, Part::Head);
        assert_eq!(bullet_damage(&t, 1.0, hit.part, 20.0, true), t.damage);
    }

    #[test]
    fn the_cars_gun_waits_after_the_change_and_shoots_where_each_barrel_points() {
        // The wait starts as the car's mode starts, and runs out in 0.75 s.
        let mut wait = VehicleWait::default();
        assert!(!wait.ready(), "no car weapon on foot");
        assert!(wait.step(true, 0.75, DT));
        assert!(!wait.ready());
        let mut updates = 0;
        while !wait.ready() {
            assert!(!wait.step(true, 0.75, DT));
            updates += 1;
        }
        assert_eq!(updates, 24, "0.75 s is 24 updates of 0.032");
        // Out of the car it is not ready, and the next change starts the wait again.
        assert!(wait.step(false, 0.75, DT));
        assert!(!wait.ready());
        wait.step(true, 0.75, DT);
        assert_eq!(wait.left(), 0.75);
        // Each barrel's round goes at the point 100 ahead of that barrel: two parallel
        // lines the barrels' distance apart, not two lines meeting at one aim point.
        let (left, right) = (Vec3::new(-1.78, 1.14, 0.19), Vec3::new(-0.14, 1.14, 0.19));
        let (a, b) = (facing_target(left, Vec3::Y), facing_target(right, Vec3::Y));
        assert_eq!(a, Vec3::new(-1.78, 101.14, 0.19));
        assert!(((b - a) - (right - left)).length() < 1e-5);
    }

    fn launcher() -> WeaponTuning {
        WeaponTuning {
            rate_of_fire: 0.6,
            automatic_firing: false,
            projectiles_per_fire: 1,
            heat_capacity: 100.0,
            heat_per_round: 23.0,
            heat_decrement_per_second: 25.0,
            heat_cooldown_til_decrement: 0.6,
            overheat_period: 6.25,
            missile_start_speed: 70.0,
            missile_top_speed: 140.0,
            missile_acceleration: 70.0,
            missile_lifetime: 4.0,
            explosive_damage: 75.0,
            hold_to_lock_on: true,
            lock_on_time: 0.4,
            guided: true,
            lock_bracket_stay: 1.0,
            lock_box: [128.0, 128.0],
            aim_assist_max_range: 350.0,
            guidance_off_delay: [-1.0, -1.0],
            guided_view_cos: 15f32.to_radians().cos(),
            out_of_view_turn: 135f32.to_radians(),
            accel_factor_delay: [0.2, 0.2],
            accel_factor: 2.0,
            wobble_timer: [0.2, 0.8],
            wobble_angle: 20f32.to_radians(),
            wobble_duration: [0.02, 0.2],
            past_target_cos: 30f32.to_radians().cos(),
            ..Default::default()
        }
    }

    fn micro_missile_explosion() -> ExplosionDef {
        ExplosionDef {
            radius: 5.0,
            start_scale: 0.5,
            time: 0.1,
            delay: 0.0,
            damage: 8.0,
            min_damage: 1.0,
            fall_off: 0.75,
            pound_override: false,
            damage_player: true,
            can_damage_self: false,
            damage_flags: 0,
            knock_back_speed: 30.0,
            knock_back_angle: 15.0,
            camera_shake: 0,
            stun_time: 0.0,
            slow_time: 0.0,
            dot_time: 0.0,
            heat: 0.0,
        }
    }

    fn dummy(id: u32, at: Vec3) -> melee::Target {
        melee::Target { id, pos: at, radius: 1.0, height: 5.0, character: true, large: false, surface: 0, dashing: false, health: f32::MAX, head: None }
    }

    #[test]
    fn a_missiles_explosion_does_the_launchers_damage_by_distance_from_its_middle() {
        let t = WeaponTuning { explosive_min_damage: -1.0, ..launcher() };
        // Beside the middle, 4 away (a body 1 wide, so touched from 3 on) and 5.5 away.
        let targets = [dummy(1, Vec3::new(0.0, 0.5, 0.0)), dummy(2, Vec3::new(4.0, 0.0, 0.0)), dummy(3, Vec3::new(5.5, 0.0, 0.0))];
        let mut blast = MissileBlast::new(micro_missile_explosion(), &t, Vec3::new(0.0, 0.0, 2.0));
        // `overwriteExplosiveDamage` 75 replaces the 8; `minDamage` stays at 1.
        assert_eq!((blast.damage, blast.min_damage), (75.0, 1.0));
        let mut hits = blast.first(&targets);
        for _ in 0..8 {
            hits.extend(blast.step(&targets, DT).0);
        }
        // The sphere starts at 2.5 and is 5 by the end of 0.1 s.
        assert!((blast.radius - 5.0).abs() < 1e-4, "{}", blast.radius);
        let by = |id: u32| hits.iter().filter(|h| h.target == id).collect::<Vec<_>>();
        assert_eq!(by(1).len(), 1);
        assert_eq!(by(1)[0].damage, 75.0);
        // Reached once the sphere is big enough, near its edge, so far down the fall-off.
        assert_eq!(by(2).len(), 1);
        assert!(by(2)[0].damage < 75.0 && by(2)[0].damage >= 1.0, "{}", by(2)[0].damage);
        // Out of reach: 4.5 from the middle to its nearest side, past 5 only from the middle.
        assert!(by(3).len() <= 1);
        assert_eq!(by(1)[0].knock_back_speed, 30.0);
    }

    #[test]
    fn an_explosion_is_gone_once_its_time_is_over() {
        let t = WeaponTuning { explosive_min_damage: -1.0, ..launcher() };
        let mut blast = MissileBlast::new(micro_missile_explosion(), &t, Vec3::ZERO);
        let mut alive = true;
        let mut updates = 0;
        while alive && updates < 100 {
            alive = blast.step(&[], DT).1;
            updates += 1;
        }
        // 0.1 s is 4 updates of 0.032; it stays one update past the time, then goes.
        assert!((4..=6).contains(&updates), "{updates}");
    }

    /// A source of numbers from 0 to 1 for the tests.
    fn dice() -> impl FnMut() -> f32 {
        let mut seed = 0x1234_5678u32;
        move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 8) as f32 / (1u32 << 24) as f32
        }
    }

    /// The game's update.
    const DT: f32 = crate::sim::GAME_UPDATE;

    fn rounds(fired: Fired) -> u32 {
        match fired {
            Fired::Rounds(n) | Fired::RoundsAndOverheated(n) => n,
            _ => 0,
        }
    }

    #[test]
    fn the_cannon_fires_at_its_rate_and_overheats_on_the_24th_round() {
        let t = cannon();
        let mut w = Weapon::default();
        let (mut total, mut time, mut locked_at) = (0, 0.0, None);
        while locked_at.is_none() {
            let fired = w.step(&t, true, DT);
            total += rounds(fired);
            time += DT;
            if matches!(fired, Fired::RoundsAndOverheated(_)) {
                locked_at = Some(time);
            }
        }
        // 100 / 4.2 is 23.8, so the 24th round fills the meter. A round needs more than
        // 0.15 s since the last, looked at once an update of 0.032 s: every fifth, 0.16 s.
        assert_eq!(total, 24);
        assert!((locked_at.unwrap() - (23.0 * 0.16 + DT)).abs() < 1e-3, "{locked_at:?}");
        // Locked for five seconds. The trigger held on is refused at once and again every
        // 0.15 s (five updates); then the meter is empty and it fires again.
        assert_eq!(w.step(&t, false, DT), Fired::Nothing);
        let refusals: Vec<bool> = (0..11).map(|_| w.step(&t, true, DT) == Fired::Refused).collect();
        assert_eq!(refusals, [true, false, false, false, false, true, false, false, false, false, true]);
        let mut waited = 12.0 * DT;
        while w.overheated_left > 0.0 {
            w.step(&t, false, DT);
            waited += DT;
        }
        assert!((waited - 5.0).abs() < 0.05 && w.heat == 0.0, "{waited}");
        assert_eq!(rounds(w.step(&t, true, DT)), 1);
    }

    #[test]
    fn the_rounds_are_spaced_by_the_games_update_whatever_the_frame_rate() {
        let t = cannon();
        for frame in [1.0 / 144.0, 1.0 / 60.0, 1.0 / 30.0, 0.1] {
            let mut w = Weapon::default();
            let (mut total, mut time) = (0, 0.0);
            while time < 2.0 {
                total += rounds(w.step(&t, true, frame));
                time += frame;
            }
            // Updates 0, 5, 10 ... of the 62 in two seconds.
            assert!((12..=13).contains(&total), "{total} rounds at {frame} a frame");
        }
    }

    #[test]
    fn heat_falls_after_a_pause() {
        let t = cannon();
        let mut w = Weapon::default();
        for _ in 0..31 {
            w.step(&t, true, DT);
        }
        let hot = w.heat;
        assert!((hot - 7.0 * 4.2).abs() < 0.01, "{hot}");
        // Nothing until 0.4 s after the last round (which left on the last of those
        // updates), then 38 a second.
        for _ in 0..12 {
            w.step(&t, false, DT);
        }
        assert_eq!(w.heat, hot);
        for _ in 0..16 {
            w.step(&t, false, DT);
        }
        assert!((hot - w.heat - 38.0 * 16.0 * DT).abs() < 0.01, "{}", w.heat);
    }

    #[test]
    fn the_launcher_fires_when_the_trigger_is_let_go() {
        let t = launcher();
        let mut w = Weapon::default();
        // Held for a second: nothing leaves, the time held runs.
        let mut total = 0;
        for _ in 0..31 {
            total += rounds(w.step(&t, true, DT));
        }
        assert_eq!(total, 0);
        assert!((w.held_for - 31.0 * DT).abs() < 1e-4);
        // Let go: one missile, on that update.
        assert_eq!(rounds(w.step(&t, false, DT)), 1);
        assert_eq!(w.held_for, 0.0);
        assert_eq!(rounds(w.step(&t, false, DT)), 0);
        // A tap inside the 0.6 s since that one does nothing; after it, another.
        w.step(&t, true, DT);
        assert_eq!(rounds(w.step(&t, false, DT)), 0);
        for _ in 0..20 {
            w.step(&t, false, DT);
        }
        w.step(&t, true, DT);
        assert_eq!(rounds(w.step(&t, false, DT)), 1);
    }

    #[test]
    fn a_weapon_that_is_not_automatic_fires_once_a_pull() {
        let t = WeaponTuning { hold_to_lock_on: false, ..launcher() };
        let mut w = Weapon::default();
        let mut total = 0;
        for _ in 0..60 {
            total += rounds(w.step(&t, true, DT));
        }
        assert_eq!(total, 1);
        w.step(&t, false, DT);
        assert_eq!(rounds(w.step(&t, true, DT)), 1);
    }

    /// The four burst weapons of the roster, as the install has them. [data]
    #[test]
    fn the_burst_weapons_read_their_counts_and_gaps() {
        if crate::install::missing() { return; }
        let tuning = |attribute: &str, kind: Kind| {
            let def = WeaponDef { attribute: crate::formats::hash::crc32(attribute.as_bytes()), ..WeaponDef::test(kind) };
            load(crate::install::dir(), &[def]).unwrap().remove(0)
        };
        let ms = |s: f32| (s * 1000.0).round() as i32;
        let laser = tuning("Ironhide_LaserCannonAttribute", Kind::Gun);
        assert_eq!((laser.burst_per_fire, ms(laser.time_between_bursts), laser.projectiles_per_fire), (2, 120, 5));
        assert_eq!(tuning("Ironhide_LaserCannonAttribute2", Kind::Gun).burst_per_fire, 1);
        let minigun = tuning("Firestorm_BurstMiniGunAttribute", Kind::Gun);
        assert_eq!((minigun.burst_per_fire, ms(minigun.time_between_bursts)), (5, 50));
        let missile = tuning("Starscream_MissileAttribute", Kind::Launcher);
        assert_eq!((missile.burst_per_fire, ms(missile.time_between_bursts)), (3, 160));
        let car = tuning("Blackout_VehicleMissileAttribute", Kind::Launcher);
        assert_eq!((car.burst_per_fire, ms(car.time_between_bursts), car.vehicle_mode), (4, 200, true));
        // A weapon that leaves them out keeps the constructor's 1 and 0.01. [game 007bca70]
        assert_eq!((cannon().burst_per_fire.max(1), tuning("Optimus_LaserRifleAttribute", Kind::Gun).burst_per_fire), (1, 1));
    }

    /// Ironhide's laser: a press is two firings, the second by itself on the fourth update
    /// (0.032 x 4 = 0.128 s is the first clock at or past 0.12), with the trigger let go;
    /// then nothing until the next press. [game 007ae010, 007ac370]
    #[test]
    fn a_press_lets_go_its_whole_burst() {
        let t = WeaponTuning { automatic_firing: false, rate_of_fire: 0.6, burst_per_fire: 2, time_between_bursts: 0.12, ..cannon() };
        let mut w = Weapon::default();
        let mut by_update = Vec::new();
        for i in 0..30 {
            by_update.push(rounds(w.step(&t, i == 0, DT)));
        }
        assert_eq!(&by_update[..5], &[1, 0, 0, 1, 0]);
        assert_eq!(by_update.iter().sum::<u32>(), 2);
        assert_eq!(w.bursts_done, 0);
    }

    /// Each firing of a burst is a round's heat, and the burst's rounds fire on with the heat
    /// full: only the trigger's firing tests it (`007a8da0`). [game 007ae010 l.448] The burst's
    /// round is added through the weapon's vtable `+0x100` (`weapon_add_heat` 007a90c0) in the
    /// same firing block as the trigger's (l.131), so it is the same heat path; the overheat
    /// start is inside 007a90c0 and was not read line by line. settles: 007a90c0's overheat
    /// branch (the capacity test that starts `overheat_period`).
    #[test]
    fn a_burst_heats_the_weapon_and_ignores_the_heat() {
        let t = WeaponTuning { heat_capacity: 10.0, heat_per_round: 6.0, burst_per_fire: 3, time_between_bursts: 0.05, ..cannon() };
        let mut w = Weapon::default();
        let mut total = 0;
        let mut overheated = false;
        for i in 0..10 {
            let fired = w.step(&t, i == 0, DT);
            overheated |= matches!(fired, Fired::RoundsAndOverheated(_));
            total += rounds(fired);
        }
        assert_eq!((total, overheated), (3, true));
        assert!(w.overheated_left > 0.0);
    }

    /// A five-round burst 0.05 apart (Breakaway's minigun) comes on whole updates, the gap's
    /// remainder carried: the clock gains 0.032 an update (from the press's own update) and
    /// loses 0.05 a firing, so firings on updates 0, 1, 3, 4, 6. [game]
    #[test]
    fn a_burst_comes_on_whole_updates() {
        let t = WeaponTuning { rate_of_fire: 0.4, burst_per_fire: 5, time_between_bursts: 0.05, ..cannon() };
        let mut w = Weapon::default();
        let fired: Vec<usize> = (0..20).filter(|&i| rounds(w.step(&t, i == 0, DT)) > 0).collect();
        assert_eq!(fired, vec![0, 1, 3, 4, 6]);
    }

    /// Put away (his weapons no longer out) the rest of a robot weapon's burst is dropped; a
    /// car's weapon goes on only in the drive or fly mode; a special always. [game 007ac370]
    #[test]
    fn a_burst_stops_when_it_may_not_go_on() {
        let t = WeaponTuning { rate_of_fire: 0.4, burst_per_fire: 5, time_between_bursts: 0.05, ..cannon() };
        let run = |t: &WeaponTuning, out: bool, motion: bool| {
            let mut w = Weapon::default();
            let first = rounds(w.step(t, true, DT));
            w.set_burst_context(out, motion);
            first + (0..20).map(|_| rounds(w.step(t, false, DT))).sum::<u32>()
        };
        assert_eq!(run(&t, true, false), 5);
        assert_eq!(run(&t, false, false), 1);
        let car = WeaponTuning { vehicle_mode: true, ..t.clone() };
        assert_eq!(run(&car, true, false), 1);
        assert_eq!(run(&car, false, true), 5);
        let special = WeaponTuning { is_special: true, ..t.clone() };
        assert_eq!(run(&special, false, false), 5);
    }

    /// A dead owner (`+0x148` bit 3) fires no burst round: the count is kept and the clock still
    /// runs, so the rounds come back when he does not die (`007ac370` l.41). [game 007ac370]
    #[test]
    fn a_dead_owner_fires_no_burst_round() {
        let t = WeaponTuning { rate_of_fire: 0.4, burst_per_fire: 5, time_between_bursts: 0.05, ..cannon() };
        let mut w = Weapon::default();
        let first = rounds(w.step(&t, true, DT));
        w.set_owner_dead(true);
        let dead: u32 = (0..20).map(|_| rounds(w.step(&t, false, DT))).sum();
        assert_eq!((first, dead), (1, 0));
        assert_eq!(w.bursts_done, 1);
        w.set_owner_dead(false);
        assert_eq!(rounds(w.step(&t, false, DT)), 1);
    }

    /// A held trigger on an automatic burst weapon: the burst rounds do not wait for the rate,
    /// and a trigger firing during a burst counts in it. [game]
    #[test]
    fn a_held_trigger_and_its_burst_share_the_count() {
        let t = WeaponTuning { rate_of_fire: 0.1, burst_per_fire: 3, time_between_bursts: 0.2, ..cannon() };
        let mut w = Weapon::default();
        let mut total = 0;
        for _ in 0..8 {
            total += rounds(w.step(&t, true, DT));
        }
        // Trigger firings on updates 0 and 4 (more than 0.1 since the last: 0.128); the
        // burst's clock reaches 0.2 on no update before the count runs out.
        assert_eq!(total, 2);
        assert_eq!(w.bursts_done, 2);
    }

    #[test]
    fn a_missile_gathers_speed_and_runs_out() {
        // No wobble, so that it flies straight.
        let t = WeaponTuning { wobble_timer: [0.0, 0.0], ..launcher() };
        let mut random = dice();
        let mut m = Missile::launch(Vec3::ZERO, Vec3::Y, Vec3::Y * 1000.0, None, &t, &mut random);
        let mut steps = 0;
        while m.step(&t, None, &mut random, DT).is_some() {
            steps += 1;
        }
        // 70 rising 70 a second for 0.2 s (to 84), then 140 a second to 140 (0.4 s more),
        // then 140 to the end of its 4 s: about 15 + 45 + 476.
        assert!((steps as f32 * DT - 4.0).abs() < 0.05);
        assert!((m.position.y - 536.0).abs() < 6.0 && m.speed == 140.0, "{} at {}", m.position.y, m.speed);
        assert!(m.position.x.abs() < 1e-3 && m.position.z.abs() < 1e-3);
    }

    #[test]
    fn a_missile_turns_to_its_target_and_flies_on_once_past_it() {
        let t = WeaponTuning { wobble_timer: [0.0, 0.0], ..launcher() };
        let mut random = dice();
        // The target is 60 degrees off its way: outside the 15 degree cone, so it turns at
        // 135 degrees a second, 4.32 an update.
        let target = Vec3::new(60f32.to_radians().sin(), 60f32.to_radians().cos(), 0.0) * 400.0;
        let mut m = Missile::launch(Vec3::ZERO, Vec3::Y, Vec3::Y * 500.0, Some(1), &t, &mut random);
        m.step(&t, Some(target), &mut random, DT);
        let turned = m.direction.angle_between(Vec3::Y).to_degrees();
        assert!((turned - 4.32).abs() < 0.05, "{turned}");
        // Inside the cone it points straight at the target, and so reaches it.
        let mut nearest = f32::MAX;
        for _ in 0..125 {
            if m.step(&t, Some(target), &mut random, DT).is_none() {
                break;
            }
            nearest = nearest.min((m.position - target).length());
        }
        assert!(nearest < 5.0, "{nearest}");
        // Past the target its guidance is off: it keeps its way.
        assert!(!m.guided && m.target.is_none());
        let way = m.direction;
        m.step(&t, Some(target), &mut random, DT);
        assert!(m.direction.angle_between(way) < 1e-3);
    }

    #[test]
    fn a_missile_wobbles_off_its_line_but_comes_back_to_the_target() {
        let t = launcher();
        let mut random = dice();
        let target = Vec3::Y * 300.0;
        let mut m = Missile::launch(Vec3::ZERO, Vec3::Y, target, Some(1), &t, &mut random);
        let (mut off, mut nearest) = (0.0f32, f32::MAX);
        for _ in 0..125 {
            if m.step(&t, Some(target), &mut random, DT).is_none() || !m.guided {
                break;
            }
            off = off.max(m.direction.angle_between((target - m.position).normalize()));
            nearest = nearest.min((m.position - target).length());
        }
        assert!(off > 0.005, "it never wobbled: {off}");
        assert!(nearest < 6.0, "{nearest}");
    }

    #[test]
    fn scatter_is_a_disc_in_the_picture_that_grows_with_the_distance() {
        let (eye, right, up) = (Vec3::ZERO, Vec3::X, Vec3::Z);
        for distance in [10.0f32, 100.0] {
            // A 45 degree view: half the picture is 0.414 of the distance high.
            let most = distance * 22.5f32.to_radians().tan() * 0.01666;
            let target = Vec3::Y * distance;
            let mut widest = 0.0f32;
            for i in 0..200 {
                let p = scattered_point(target, eye, right, up, 45.0, 0.01666, i as f32 / 200.0, (i * 37 % 200) as f32 / 199.0);
                assert!((p.y - distance).abs() < 1e-3, "it stays in the picture plane");
                widest = widest.max((p - target).length());
            }
            assert!(widest <= most + 1e-4 && widest > most * 0.95, "{widest} of {most}");
        }
        assert_eq!(scattered_point(Vec3::Y, eye, right, up, 45.0, 0.0, 0.3, 0.9), Vec3::Y);
    }
}

/// The weapons' sound fields and which of their fire sounds the weapon keeps (r3 sound topic).
#[cfg(test)]
mod sound_tests {
    use super::*;
    use crate::sound::{event_id, Library};

    const ROSTER: [&str; 13] = ["bumblebee", "optimus", "ratchet", "megatron", "longhaul", "firestorm", "blackout", "audir8",
        "starscream", "jetfire", "ironhide", "optimusflight", "megatronflight"];

    fn pack(name: &str) -> crate::character::CharacterData {
        crate::character::load(crate::install::dir(), name).unwrap()
    }

    fn library(name: &str) -> Library {
        let dir = crate::install::dir();
        Library::load(&[dir.join(format!(r"characters\eng\{name}.fev")), dir.join(r"audio\eng\global.fev")]).0
    }

    /// Which weapons keep their fire sound (automatic AND an infinite event, 007a9b40), which have
    /// a ring-off or an idle loop, and that no special weapon's fire sound is infinite (the
    /// arsenal never keys one off). [data]
    #[test]
    fn the_kept_fire_sounds_are_the_gatling_the_audi_car_gun_and_the_flames() {
        if crate::install::missing() { return; }
        let (mut kept, mut rings, mut idles) = (Vec::new(), Vec::new(), Vec::new());
        for name in ROSTER {
            let data = pack(name);
            let tunings = load(crate::install::dir(), &data.weapons).unwrap();
            let lib = library(name);
            let named = |id: u32| lib.name(id).unwrap_or("?").to_owned();
            for (d, t) in data.weapons.iter().zip(&tunings) {
                if d.fire_sound != 0 && t.automatic_firing && lib.infinite(d.fire_sound) {
                    kept.push((name, named(d.fire_sound)));
                }
                if t.is_special {
                    assert!(d.fire_sound == 0 || !lib.infinite(d.fire_sound), "{name} {:#010x}", d.name);
                }
                if d.ring_off_sound != 0 {
                    assert!(!lib.infinite(d.ring_off_sound), "a ring-off is a one-shot");
                    rings.push((name, d.name));
                }
                if d.idle_sound != 0 {
                    assert!(lib.infinite(d.idle_sound), "an idle sound is a loop");
                    idles.push((name, named(d.idle_sound)));
                }
            }
        }
        let own = |v: &[(&'static str, &str)]| v.iter().map(|&(a, b)| (a, b.to_string())).collect::<Vec<_>>();
        assert_eq!(kept, own(&[("longhaul", "WPN_FIRE_FLAME"), ("longhaul", "WPN_FIRE_FLAME"),
            ("blackout", "WPN_BLAC_GAT_FIRE_LP"), ("audir8", "WPN_VEH_SHARED_GUN_FIRE_LOOP")]));
        assert_eq!(idles, own(&[("audir8", "ANIM_AUDR8_WEPN_IDL_A_LP"), ("starscream", "ANIM_STAR_WEPN_A_LP")]));
        // Optimus's two shell cannons, Starscream's Gatling gun, Jetfire's bombs (an event that
        // is in no file).
        assert_eq!(rings.iter().map(|r| r.0).collect::<Vec<_>>(), ["optimus", "optimus", "starscream", "jetfire", "jetfire"]);
    }

    /// `fireOverHeatKeyOff` (an enum-typed byte, 0 or 1) read as `work\flags_raw.py` counts the
    /// ones per pack: Starscream 2, Ironhide 1, Blackout 2, Optimus 2, Ratchet 3, Audi 2, Long Haul 1.
    #[test]
    fn the_overheat_key_off_byte_is_read_as_the_raw_data_counts_it() {
        if crate::install::missing() { return; }
        for (name, ones) in [("starscream", 2), ("ironhide", 1), ("blackout", 2), ("optimus", 2), ("ratchet", 3), ("audir8", 2), ("longhaul", 1)] {
            let count = pack(name).weapons.iter().filter(|d| d.overheat_key_off).count();
            assert_eq!(count, ones, "{name}");
        }
        let starscream = pack("starscream");
        let gatling = starscream.weapons.iter().find(|d| d.name == 0xb89f_ae41).unwrap();
        assert!(gatling.overheat_key_off && gatling.ring_off_sound == event_id("WPN_STARS_FIRE_RINGOFF")
            && gatling.idle_sound == event_id("ANIM_STAR_WEPN_A_LP"));
    }

    /// The ground waves' own sounds, which the arsenal starts with the wave: the snaker object's
    /// SoundPlayers `ANIM_BUMB_CHARGE_LP` (a loop of global.fev) and `WPN_BLAC_CAN_IMP` (a
    /// one-shot) in Blackout's, Jetfire's and OptimusFlight's packs. [data r3-sound 1.7]
    #[test]
    fn the_ground_waves_carry_the_charge_loop_and_the_cannon_impact() {
        if crate::install::missing() { return; }
        let wanted = [event_id("ANIM_BUMB_CHARGE_LP"), event_id("WPN_BLAC_CAN_IMP")];
        for name in ["blackout", "jetfire", "optimusflight"] {
            let data = pack(name);
            let waves: Vec<&Vec<u32>> = data.weapon_effects.iter().filter_map(|r| r.wave.as_ref()).map(|w| &w.sounds).collect();
            assert!(!waves.is_empty(), "{name}");
            for sounds in waves {
                assert_eq!(sounds.as_slice(), wanted.as_slice(), "{name}");
            }
            let lib = library(name);
            assert!(lib.infinite(wanted[0]) && !lib.infinite(wanted[1]), "{name}");
        }
    }

    fn gatling() -> WeaponTuning {
        let data = pack("blackout");
        let tunings = load(crate::install::dir(), &data.weapons).unwrap();
        let i = data.weapons.iter().position(|d| d.name == 0x0e10_e69c).expect("BlackoutGatling");
        tunings[i].clone()
    }

    /// The gatling held: its sounds are not cut on any step after the first firing, and the step
    /// the trigger goes up cuts them. `heat_at_fire` is the heat before each round. [game 007ac370,
    /// 007ae010]
    #[test]
    fn a_held_gatling_keeps_its_sounds_until_the_trigger_goes_up() {
        if crate::install::missing() { return; }
        let t = gatling();
        assert!(t.automatic_firing && t.heat_per_round > 0.0);
        let mut w = Weapon::default();
        let dt = crate::sim::GAME_UPDATE;
        let mut rounds = 0;
        let mut fired_yet = false;
        let mut steps = 0;
        while steps < (1.0 / dt) as usize && w.overheated_left <= 0.0 {
            let before = w.heat;
            let fired = w.step(&t, true, dt);
            if let Fired::Rounds(n) | Fired::RoundsAndOverheated(n) = fired {
                rounds += n;
                fired_yet = true;
                // No cooling while it fires: round n starts from (n - 1) rounds' heat.
                assert!((w.heat_at_fire - before).abs() < 1e-4, "{} {before}", w.heat_at_fire);
                assert!((w.heat_at_fire - (rounds - 1) as f32 * t.heat_per_round).abs() < 1e-3);
            }
            if fired_yet {
                assert!(!w.sounds_cut, "cut at step {steps} while held");
            }
            steps += 1;
        }
        assert!(rounds > 1);
        w.step(&t, false, dt);
        assert!(w.sounds_cut, "the trigger up cuts the sounds");
        w.step(&t, false, dt);
        assert!(w.sounds_cut);
    }

    /// The arsenal's rule (`game\src\weapons.rs` `KeptSounds`) over the mixer: Blackout holds the
    /// gatling 2 s, a start per firing only while none is kept, kept when automatic and infinite,
    /// keyed off when the weapon cuts its sounds. ONE `WPN_BLAC_GAT_FIRE_LP` lives while held
    /// (not one a round), and after the release it plays its spin-down and ends. [game 007a9b40,
    /// 007a9d40]
    #[test]
    fn a_held_gatling_plays_one_loop_and_its_spin_down_after_the_release() {
        if crate::install::missing() { return; }
        let t = gatling();
        let lib = library("blackout");
        let id = event_id("WPN_BLAC_GAT_FIRE_LP");
        lib.prepare(id);
        let mut mixer = crate::sound::Mixer::new(48000);
        let mut w = Weapon::default();
        let dt = crate::sim::GAME_UPDATE;
        let mut kept: Option<crate::sound::Handle> = None;
        let mut buffer = vec![0.0; (dt * 48000.0) as usize * 2];
        let (mut rounds, mut most) = (0, 0);
        let mut step = |w: &mut Weapon, kept: &mut Option<crate::sound::Handle>, mixer: &mut crate::sound::Mixer, trigger: bool| {
            let mut fired = 0;
            if let Fired::Rounds(n) | Fired::RoundsAndOverheated(n) = w.step(&t, trigger, dt) {
                for _ in 0..n {
                    fired += 1;
                    if kept.is_none() {
                        let handle = mixer.play(&lib, id, 1.0);
                        if t.automatic_firing && lib.infinite(id) {
                            *kept = handle;
                        }
                    }
                }
            }
            if w.sounds_cut {
                if let Some(handle) = kept.take() {
                    mixer.key_off(handle);
                }
            }
            mixer.render(&lib, &mut buffer);
            fired
        };
        for _ in 0..(2.0 / dt) as usize {
            rounds += step(&mut w, &mut kept, &mut mixer, true);
            most = most.max(mixer.live(id));
        }
        assert!(rounds > 10, "{rounds}");
        assert_eq!(most, 1, "one held loop, not one a round");
        let handle = kept.expect("kept while held");
        step(&mut w, &mut kept, &mut mixer, false);
        assert!(kept.is_none() && mixer.is_alive(handle), "keyed off: its tail plays");
        for _ in 0..(3.0 / dt) as usize {
            step(&mut w, &mut kept, &mut mixer, false);
        }
        assert!(!mixer.is_alive(handle), "it ends after its spin-down");
    }

    /// Held to the overheat: the round that fills it says so, which the arsenal turns into the
    /// fire loop's key-off before the overheat sound (007a90c0 -> 007a9ec0). [game]
    #[test]
    fn the_gatling_held_to_its_capacity_overheats_on_a_round() {
        if crate::install::missing() { return; }
        let t = gatling();
        let mut w = Weapon::default();
        let dt = crate::sim::GAME_UPDATE;
        let mut overheated = false;
        for _ in 0..2000 {
            if let Fired::RoundsAndOverheated(_) = w.step(&t, true, dt) {
                overheated = true;
                break;
            }
        }
        assert!(overheated && w.overheated_left > 0.0);
        assert!(w.heat_at_fire < t.heat_capacity);
    }

    /// One game update of the mixer; the loudest sample of it.
    fn render_peak(mixer: &mut crate::sound::Mixer, lib: &Library, buffer: &mut [f32]) -> f32 {
        mixer.render(lib, buffer);
        buffer.iter().fold(0.0f32, |m, &s| m.max(s.abs()))
    }

    /// Every charged weapon's `chargeLoopSound` over the real mixer: the roster's charge loops
    /// are Optimus's (both forms) `WPN_OPTI_GUN_CHARGE`, Megatron's (both forms)
    /// `WPN_MEGA_BEAM_CHARGE`, Ratchet's `ANIM_RATC_GRE_CHARG_LP`, Long Haul's
    /// `ANIM_LONG_GRE_CHRG_LP` (no child weapon has one); each is infinite, holds at its primary
    /// parameter's sustain point (heard after 2 s), and after the key-off the arsenal gives it at
    /// the release and the cancel (007b0aa0 -> 00595c90) runs on into silence within 1 s (the
    /// beam's after its charge-down). [data, game 007b08a0, 007b0aa0]
    #[test]
    fn every_charge_loop_holds_while_charging_and_falls_silent_after_its_key_off() {
        if crate::install::missing() { return; }
        let mut loops = Vec::new();
        let dt = crate::sim::GAME_UPDATE;
        for name in ROSTER {
            let data = pack(name);
            let tunings = load(crate::install::dir(), &data.weapons).unwrap();
            let lib = library(name);
            for (d, t) in data.weapons.iter().zip(&tunings) {
                let id = d.charge_loop_sound;
                if !t.charge.has || id == 0 {
                    // A child of a charged weapon is driven by its parent and has no loop of its own.
                    assert!(!(t.charge.has && t.parent != 0 && id != 0), "{name} {:#010x}", d.name);
                    continue;
                }
                assert!(lib.infinite(id), "{name}: a charge loop runs until stopped");
                lib.prepare(id);
                let mut mixer = crate::sound::Mixer::new(48000);
                let mut buffer = vec![0.0; (dt * 48000.0) as usize * 2];
                let handle = mixer.play(&lib, id, 1.0).unwrap();
                let mut heard = 0.0f32;
                for k in 0..(2.0 / dt) as usize {
                    let peak = render_peak(&mut mixer, &lib, &mut buffer);
                    if k as f32 * dt > 1.5 { heard = heard.max(peak); }
                }
                assert!(mixer.is_alive(handle) && heard > 0.01, "{name}: held and heard ({heard})");
                mixer.key_off(handle);
                let (mut silent_at, mut ended_at) = (None, None);
                for k in 0..(10.0 / dt) as usize {
                    let peak = render_peak(&mut mixer, &lib, &mut buffer);
                    if peak > 1e-4 { silent_at = None } else if silent_at.is_none() { silent_at = Some(k as f32 * dt) }
                    if ended_at.is_none() && !mixer.is_alive(handle) { ended_at = Some(k as f32 * dt) }
                }
                let silent_at = silent_at.unwrap_or(f32::INFINITY);
                eprintln!("{name} {}: silent {silent_at:.2} s, ended {ended_at:?} after the key-off", lib.name(id).unwrap());
                // Megatron's beam plays its one-shot charge-down (layer 2, at the end of
                // `param01`) for about 3.1 s after the key-off; the others only end their loop.
                let tail = if lib.name(id) == Some("WPN_MEGA_BEAM_CHARGE") { 3.5 } else { 1.0 };
                assert!(silent_at < tail, "{name}: silent {silent_at} s after the key-off");
                // And it leaves the mix (no silent copies left for `max_playbacks` to evict).
                assert!(ended_at.is_some(), "{name}: still in the mix 10 s after the key-off");
                loops.push((name, lib.name(id).unwrap().to_owned()));
            }
        }
        let own = |v: &[(&'static str, &str)]| v.iter().map(|&(a, b)| (a, b.to_string())).collect::<Vec<_>>();
        assert_eq!(loops, own(&[("optimus", "WPN_OPTI_GUN_CHARGE"), ("ratchet", "ANIM_RATC_GRE_CHARG_LP"),
            ("megatron", "WPN_MEGA_BEAM_CHARGE"), ("longhaul", "ANIM_LONG_GRE_CHRG_LP"),
            ("optimusflight", "WPN_OPTI_GUN_CHARGE"), ("megatronflight", "WPN_MEGA_BEAM_CHARGE")]));
    }

    fn shell_cannon() -> WeaponTuning {
        let data = pack("optimus");
        let tunings = load(crate::install::dir(), &data.weapons).unwrap();
        let i = data.weapons.iter().position(|d| d.name == 0x690c_62d1).expect("ShellCannon");
        tunings[i].clone()
    }

    /// The arsenal's charge-loop rule (`game\src\weapons.rs` `KeptSounds::charge_start` /
    /// `charge_stop`) over the mixer and Optimus's ShellCannon: the loop starts on the charge's
    /// start, only while none is kept, and is keyed off on the release (a partial charge, then a
    /// full one held past its full notice, which does not stop it) and on the cancel. One loop
    /// lives while charging, it falls silent after each key-off, and no copies pile up.
    /// [game 007b08a0, 007af210, 007add20, 007b0aa0, 007b0520]
    #[test]
    fn the_shell_cannons_charge_loop_is_keyed_off_on_the_release_and_the_cancel() {
        if crate::install::missing() { return; }
        let t = shell_cannon();
        assert!(t.charge.has);
        let lib = library("optimus");
        let id = event_id("WPN_OPTI_GUN_CHARGE");
        lib.prepare(id);
        let mixer = crate::sound::Mixer::new(48000);
        let dt = crate::sim::GAME_UPDATE;
        let buffer = vec![0.0; (dt * 48000.0) as usize * 2];
        struct Rig {
            w: Weapon,
            mixer: crate::sound::Mixer,
            buffer: Vec<f32>,
            kept: Option<crate::sound::Handle>,
            starts: usize,
            stops: usize,
            fulls: usize,
            most: usize,
        }
        impl Rig {
            /// One game update: the weapon, then what the arsenal does with its charge events.
            fn step(&mut self, t: &WeaponTuning, lib: &Library, id: u32, trigger: bool, cancel: bool) -> f32 {
                let dt = crate::sim::GAME_UPDATE;
                self.w.set_charge_cancel(cancel);
                self.w.step(t, trigger, dt);
                let e = self.w.charge_events;
                if e.started {
                    self.starts += 1;
                    if self.kept.is_none() {
                        self.kept = self.mixer.play(lib, id, 1.0);
                    }
                }
                self.fulls += e.full as usize;
                if e.released.is_some() || e.cancelled {
                    self.stops += 1;
                    if let Some(h) = self.kept.take() {
                        self.mixer.key_off(h);
                    }
                }
                self.most = self.most.max(self.mixer.live(id));
                render_peak(&mut self.mixer, lib, &mut self.buffer)
            }
            /// Trigger up for 1.5 s: when the output went silent for good, in seconds.
            fn quiet_after(&mut self, t: &WeaponTuning, lib: &Library, id: u32) -> f32 {
                let dt = crate::sim::GAME_UPDATE;
                let mut silent_at = None;
                for k in 0..(1.5 / dt) as usize {
                    if self.step(t, lib, id, false, false) > 1e-4 { silent_at = None } else if silent_at.is_none() { silent_at = Some(k as f32 * dt) }
                }
                silent_at.unwrap_or(f32::INFINITY)
            }
        }
        let mut r = Rig { w: Weapon::default(), mixer, buffer, kept: None, starts: 0, stops: 0, fulls: 0, most: 0 };
        // A partial charge (1.5 s held), released.
        let mut heard = 0.0f32;
        for _ in 0..(1.5 / dt) as usize {
            heard = heard.max(r.step(&t, &lib, id, true, false));
        }
        assert!(r.kept.is_some() && r.w.charging && heard > 0.01, "charging and heard: {heard}");
        r.step(&t, &lib, id, false, false);
        assert!(r.kept.is_none() && r.stops == 1, "the release keys it off");
        let q = r.quiet_after(&t, &lib, id);
        assert!(q < 1.0, "{q}");
        // A full charge held about 1 s past full: the full notice keeps the loop going.
        for _ in 0..(4.0 / dt) as usize {
            r.step(&t, &lib, id, true, false);
        }
        assert!(r.fulls == 1 && r.kept.is_some(), "full ({}) and still kept", r.fulls);
        r.step(&t, &lib, id, false, false);
        assert!(r.kept.is_none() && r.stops == 2);
        let q = r.quiet_after(&t, &lib, id);
        assert!(q < 1.0, "{q}");
        // A charge cancelled (Megatron's special's path, 007add20): keyed off, no shot.
        for _ in 0..(1.0 / dt) as usize {
            r.step(&t, &lib, id, true, false);
        }
        assert!(r.kept.is_some());
        r.step(&t, &lib, id, true, true);
        assert!(r.kept.is_none() && r.stops == 3 && !r.w.charging, "the cancel keys it off");
        let q = r.quiet_after(&t, &lib, id);
        assert!(q < 1.0, "{q}");
        assert_eq!(r.starts, 3);
        assert!(r.most <= 2, "copies piled up: {}", r.most);
    }
}
