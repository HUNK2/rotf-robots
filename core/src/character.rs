//! Loads one character pack from the game install into plain data.

use std::path::Path;

use crate::formats::anim::{self, AnimRef, AnimSet};
use crate::formats::lxb::{DataFile, Node};
use crate::formats::model::Library;
use crate::formats::pack::{self, Pack};
use crate::formats::scene::{self, ObjectDef};
use crate::particles::Template;
use crate::weapons::{self, WeaponDef};

pub struct CharacterData {
    pub robot: ObjectDef,
    pub vehicle: Option<ObjectDef>,
    pub animations: AnimSet,
    /// The clips the vehicle model carries for itself, for the change of form.
    pub vehicle_animations: Vec<AnimRef>,
    /// The Character's `nameID` (the CRC-32 of its name: Bumblebee 0x63bbaba9), which the constructor
    /// 00713300 stores at +0x170 (l.192-199, field hash f034c6a4) [game, data].
    pub name_id: u32,
    pub library: Library,
    pub sounds: Sounds,
    pub chatter: Vec<crate::chatter::Line>,
    /// `particleHitTable.meleeHitSparkPreset`: the sound and the effect of a melee hit,
    /// by the surface struck.
    pub melee_impact: ImpactPreset,
    /// `footStepFX`: the same for a step, by the ground's surface.
    pub step_impact: ImpactPreset,
    /// `climbData`'s five presets, in `anim::CLIMB_SIDES`' order: the dust of a hand or a
    /// foot on the wall, and of sliding down it. [data]
    pub climb_impact: Vec<ImpactPreset>,
    /// `vehicleTireSkidFX` and `vehicleTireRoadFX` (character `+0x35c`, `+0x360`): the loop
    /// and the effect of a skidding and of a rolling tyre, by the ground's surface.
    pub tyre_skid: ImpactPreset,
    pub tyre_road: ImpactPreset,
    /// His weapons, primary first; the car's guns come last.
    pub weapons: Vec<WeaponDef>,
    /// Each weapon's own object, in the same order.
    pub weapon_objects: Vec<Option<ObjectDef>>,
    /// Each weapon's projectile model (a grenade's; `weapons::read_grenade_objects`), in the same order.
    pub grenade_objects: Vec<Option<ObjectDef>>,
    /// Each weapon's deployed turret model and its clips, and its missile model
    /// (`weapons::read_turret_objects`, `read_turret_clips`, `read_missile_objects`), in the same order.
    pub turret_objects: Vec<Option<ObjectDef>>,
    pub turret_clips: Vec<Vec<anim::AnimRef>>,
    pub missile_objects: Vec<Option<ObjectDef>>,
    /// Each weapon object's own clips, in the same order.
    pub weapon_animations: Vec<Vec<AnimRef>>,
    /// Its melee weapons in the manager's slot order (his axe; Optimus's and the Audi's two
    /// arm blades), each with its object and clips. [game 007b4680] [data]
    pub melee_weapons: Vec<weapons::MeleeWeapon>,
    /// What each weapon shows, by the weapon's name.
    pub weapon_effects: Vec<WeaponEffects>,
    /// The robot object's `ShockWave` attachments, each with the node it sits on (its attachment
    /// `index`). Only Devastator's object has any among the playable packs: the two his pound's
    /// Trigger events set off (`crate::giant`). [data]
    pub shockwaves: Vec<(usize, crate::shockwave::Def)>,
    /// Sound events the port gives him beyond what his data names (Devastator's,
    /// `giant::SOUNDS`), to be loaded with the rest. Empty from the pack.
    pub extra_sounds: Vec<u32>,
    /// [port design] What Grindor's cinematic wave borrows (`crate::energon_wave::read`), put
    /// in by the game for a character holding his cannon. None from the pack.
    pub energon: Option<crate::energon_wave::Dressing>,
}

/// An object's own `ShockWave` attachments and their nodes. [data]
pub(crate) fn read_shockwaves(object: Node) -> Vec<(usize, crate::shockwave::Def)> {
    object
        .get("attachments")
        .into_iter()
        .flat_map(|a| a.items())
        .filter_map(|a| {
            let thing = a.get("thing").filter(|t| t.type_name() == Some("ShockWave"))?;
            Some((a.get("index").and_then(Node::int).unwrap_or(0) as usize, crate::shockwave::read(thing)))
        })
        .collect()
}

/// Sound events the character names outside its clips, by id (the CRC-32 of the event's
/// name); 0 where the data gives none.
#[derive(Default, Clone, Copy, Debug)]
pub struct Sounds {
    /// `soundTable.vehicleTransmission`: the engine, driven by `rpm` and `load`.
    pub engine: u32,
    /// `soundTable.vehicleDriftStart`: a burst of revs.
    pub drift_start: u32,
    pub suspension: u32,
    pub turbo: u32,
    pub turbo_ready: u32,
    /// `soundTable.vehicleDrive`: a flyer's drive loop (`VEH_F22`, `VEH_HELI_BLACKOUT`), fed
    /// `speed`, and `roll` and `pitch` for the jet. [data] 0 for the car and the robots.
    pub drive: u32,
    /// `soundTable.hoverSound`: a flyer's hover loop (`VEH_F22_HOVER`), fed `speed`. [data]
    pub hover: u32,
    /// `footStepFX`: the sound for the default ground material.
    pub footstep: u32,
}

/// One entry of a `MaterialImpactPreset`'s `materialInfos`. [data]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImpactInfo {
    /// The surface materials this entry is for, by the CRC-32 of their names.
    pub materials: Vec<u32>,
    /// `soundOnImpact`: a sound event's id, 0 for none.
    pub sound: u32,
    /// `effectOnImpact`: the particle template, by the CRC-32 of its name (`r_hit_melee`
    /// for every entry of his melee preset), and the template itself.
    pub effect: Option<u32>,
    pub template: Option<Template>,
    /// `effectOnImpactFlags`: how the effect is turned (bit 0 z along the normal, 1 z up,
    /// 2 y along the normal, 3 y up, 4 and 5 a random y or z, 6 and 7 along the forward
    /// handed in) and bit 8, tied to the object that was hit (`FUN_0077c180`). [game]
    pub effect_flags: u32,
}

/// A `MaterialImpactPreset`: what an impact sounds and looks like on each surface. The game
/// keeps them by name (`thunk_FUN_01184300`) and asks with a surface (`FUN_0077cd60`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImpactPreset {
    pub infos: Vec<ImpactInfo>,
    /// Persistent surface damage, selected independently of materialInfos. [data/game]
    pub ssd: Vec<crate::ssd::Info>,
}

impl ImpactPreset {
    /// The entry for a surface (`FUN_0077bff0`): the first that lists it, else the first
    /// that lists `0 - Default`, else none. [game]
    pub fn pick(&self, surface: u32) -> Option<&ImpactInfo> {
        let listing = |material: u32| self.infos.iter().find(|info| info.materials.contains(&material));
        listing(surface).or_else(|| listing(crate::melee::surface::DEFAULT))
    }
}

fn read_impact(preset: Option<Node>) -> ImpactPreset {
    let infos = preset.and_then(|preset| preset.get("materialInfos")).into_iter().flat_map(|infos| infos.items());
    ImpactPreset {
        ssd: crate::ssd::read(preset),
        infos: infos
            .map(|info| ImpactInfo {
                materials: info.get("material").map(|m| m.ints().into_iter().map(|h| h as u32).collect()).unwrap_or_default(),
                sound: info.get("soundOnImpact").and_then(Node::int).map_or(0, |h| h as u32),
                effect: info.get("effectOnImpact").and_then(Node::label),
                template: info.get("effectOnImpact").and_then(Template::read),
                effect_flags: info.get("effectOnImpactFlags").and_then(Node::int).map_or(0, |v| v as u32),
            })
            .collect(),
    }
}

/// What a weapon shows as it fires and where its rounds land. [data]
/// Muzzle particles follow the weapon's flash/barrel node (007ae010/007aac80).
/// Impact and projectile effect adapters are described in `game\src\effects.rs`.
#[derive(Clone, Debug, Default)]
pub struct WeaponEffects {
    /// The weapon's name, as in its `WeaponDef`.
    pub name: u32,
    /// `muzzleFlashEffect`.
    pub muzzle_flash: Option<Template>,
    /// A gun's `bulletProjectileObj`'s `BulletTrace.impactFX`: by the surface struck.
    pub impact: ImpactPreset,
    /// The projectile object's own `ParticleEmitter`: what it trails.
    pub trail: Option<Template>,
    /// The `ParticleEmitter` of the object the projectile's `DamageLink` lets go of
    /// where it ends (a missile's explosion).
    pub blast: Option<Template>,
    pub blast_ssd: Vec<crate::ssd::Info>,
    /// Anonymous SoundPlayers of the projectile and its released objects. [data]
    pub flight_sounds: Vec<u32>,
    pub blast_sounds: Vec<u32>,
    /// `genericWeaponSoundTable.fireSound` of the weapon the row is read from. Kept for the
    /// deployed turret's gun, which is no weapon of his list (`turret_gun_key`). [data]
    pub fire_sound: u32,
    /// The projectile object's own `ParticleEmitter`s, each on its node (Bumblebee's micro missile:
    /// five `w_bullet_trail_micro_missiles2` on `FX_trail01..05`), with 0057d0d0's defaults. [data]
    pub emitters: Vec<scene::PlacedEmitterDef>,
    /// The projectile object's own `Light`s, in its space: they ride it for as long as it lives
    /// (none has a TimeoutDeleter). [data]
    pub flight_lights: Vec<crate::point_light::LevelLight>,
    /// The Light and TimeoutDeleter of the first `DetachLink` object of the DamageLink's object
    /// that has a Light (`detach1`): the blast's flash. [data]
    pub blast_light: Option<crate::point_light::FlashLight>,
    /// The `ControllerRumble` among the DamageLink object's own attachments: `waitForTrigger` 0, so
    /// it starts as the blast object exists (`controller_rumble_activate` 007dba80). [game] [data]
    pub blast_rumble: Option<crate::rumble::RumbleDef>,
    /// The projectile's `Destructable.destroyEffectSpawn`: the lingering smoke its `FX_Dummy`'s
    /// `TriggerOnSpawn` turns on and off by name. [data]
    pub after_blast: Vec<TimedParticle>,
    /// The ground wave a missile lets go (`Destructable.spawnOnDestruction[0]`'s object, when it has
    /// a `SnakerProjectile`): Grindor's and Jetfire's energon cannons, OptimusFlight's charged cannon.
    pub wave: Option<WaveEffects>,
    /// The pieces the blast object throws (its DetachLinks with a Debris: Bumblebee's micro missile's
    /// five chunks; no other round of the roster has any), placed in the projectile's space. [data]
    pub debris: Vec<crate::debris::DebrisPiece>,
}

/// One `TriggerableParticle` of a projectile's `destroyEffectSpawn` object, with the delays its
/// `TriggerOnSpawn` sends it its `TRIGGER` (d5d636c1) and `UNTRIGGER` (9caa723d) after: each
/// `delayMin + r (delayMax - delayMin)` [game 0080daf0]. [data]
#[derive(Clone, Debug)]
pub struct TimedParticle {
    pub template: Template,
    /// Where it sits from the projectile's end: the Instance's matrix applied to its node's bind.
    pub offset: glam::Vec3,
    /// Its `useRot` (default on, 00746fc0) and `useScale`.
    pub use_rot: bool,
    pub use_scale: bool,
    pub on: (f32, f32),
    pub off: (f32, f32),
}

/// What the ground wave's own object carries beside the `SnakerProjectile`'s rules: its picture is
/// its `ParticleEmitter` (there is no RenderThing), its Light with a TimeoutDeleter that removes
/// the whole object, a ControllerRumble and SoundPlayers. [data: blackout 698222, jetfire 649524,
/// optimusflight 754586]
#[derive(Clone, Debug)]
pub struct WaveEffects {
    pub emitter: Option<scene::PlacedEmitterDef>,
    /// The Light (in the wave object's space) and the object's TimeoutDeleter.
    pub light: Option<crate::point_light::FlashLight>,
    pub rumble: Option<crate::rumble::RumbleDef>,
    /// The TimeoutDeleter's `time`: the wave lives min(this, `snakerInfo.timeTilDies`) [game 00776f50].
    pub life: f32,
    /// `SnakerProjectile.checkGroundHeight` and `initCheckGroundRadius` (the ctor 0078cfd0's
    /// defaults are 1 and 2 [game]; the data gives 3 and 6).
    pub check_ground_height: f32,
    pub init_check_ground_radius: f32,
    /// The radius of the object's `Collision` sphere (1.36743), with its matrix's scale.
    pub radius: f32,
    /// The object's SoundPlayers that need no trigger (for the sound side).
    pub sounds: Vec<u32>,
}

/// The matrix of an Instance node as rows, or the identity.
fn instance_matrix(node: Node) -> [[f32; 3]; 4] {
    let v = node.get("matrix").map(Node::floats).unwrap_or_default();
    if v.len() != 12 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0; 3]];
    }
    std::array::from_fn(|r| std::array::from_fn(|c| v[r * 3 + c]))
}

fn rows_affine(m: [[f32; 3]; 4]) -> glam::Affine3A {
    glam::Affine3A::from_cols_array(&[m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2], m[3][0], m[3][1], m[3][2]])
}

/// The things of an object's attachments, with each attachment's `index`.
fn attached(object: Node<'_>) -> Vec<(i64, Node<'_>)> {
    object.get("attachments").into_iter().flat_map(|a| a.items())
        .filter_map(|a| Some((a.get("index").and_then(Node::int).unwrap_or(0), a.get("thing")?))).collect()
}

/// The first `spawnScene.things[0]` Instance of a Destructable list field, and its Object.
fn spawned<'a>(destructable: Node<'a>, field: &str) -> Option<(Node<'a>, Node<'a>)> {
    let instance = destructable.get(field)?.at(0)?.path(&["spawnScene", "things"])?.at(0)?;
    Some((instance, instance.get("things")?.at(0)?))
}

/// The lingering smoke of `destroyEffectSpawn`: each TriggerableParticle with the delays of the
/// TriggerOnSpawn entries that name it (`luxScriptNameSendFilter` = its `LuxScriptName`). [data]
fn read_after_blast(destructable: Node) -> Vec<TimedParticle> {
    let Some((instance, object)) = spawned(destructable, "destroyEffectSpawn") else { return Vec::new() };
    let place = rows_affine(instance_matrix(instance));
    let things = attached(object);
    let sends: Vec<(bool, u32, (f32, f32))> = things.iter()
        .filter(|(_, t)| t.type_name() == Some("TriggerOnSpawn"))
        .flat_map(|(_, t)| t.path(&["trigOnSpawn", "triggerArray"]).into_iter().flat_map(|a| a.items()))
        .filter_map(|entry| {
            let kind = entry.get("msgType")?;
            // By name where the file's table spells it, else by its hash (Ironhide's pack stores
            // the hash itself: `#d5d636c1`).
            let raw = kind.int().map(|v| v as u32);
            let on = match (kind.enum_name(), kind.enum_hash(), raw) {
                (Some("TRIGGER"), _, _) | (_, Some(0xd5d6_36c1), _) | (_, _, Some(0xd5d6_36c1)) => true,
                (Some("UNTRIGGER"), _, _) | (_, Some(0x9caa_723d), _) | (_, _, Some(0x9caa_723d)) => false,
                _ => return None,
            };
            let num = |name: &str| entry.get(name).and_then(Node::float).unwrap_or(0.0);
            Some((on, entry.get("luxScriptNameSendFilter").and_then(Node::int)? as u32, (num("delayMin"), num("delayMax"))))
        })
        .collect();
    let mut out = Vec::new();
    for (index, thing) in &things {
        if thing.type_name() != Some("TriggerableParticle") { continue }
        let Some(template) = thing.get("id").and_then(Template::read) else { continue };
        let name = thing.get("LuxScriptName").and_then(Node::int).unwrap_or(0) as u32;
        let delay = |on: bool| sends.iter().find(|(o, n, _)| *o == on && *n == name).map(|s| s.2);
        let Some(start) = delay(true) else { continue };
        let flag = |field: &str, default: bool| thing.get(field).map_or(default, |n| n.int().map_or_else(|| n.is_true(), |v| v != 0));
        let bind = crate::level_render::flat32(&crate::level_render::bind_of(object, *index));
        out.push(TimedParticle {
            template,
            offset: place.transform_point3(glam::Vec3::new(bind[9], bind[10], bind[11])),
            use_rot: flag("useRot", true),
            use_scale: flag("useScale", false),
            on: start,
            // No UNTRIGGER sent: it runs until its own elements end. [data] bumblebee and starscream
            // (export/data): every TriggerOnSpawn of the blast carries TRIGGER (`#d5d636c1`) only, and the
            // UNTRIGGER entries (`#9caa723d`) belong to AnimExitTriggerEvent. Other packs not checked.
            off: delay(false).unwrap_or((f32::INFINITY, f32::INFINITY)),
        });
    }
    out
}

/// The ground wave's object (`spawnOnDestruction[0]`), when it carries a `SnakerProjectile`. [data]
fn read_wave(destructable: Node) -> Option<WaveEffects> {
    let (_, object) = spawned(destructable, "spawnOnDestruction")?;
    let things = attached(object);
    let snaker = things.iter().find(|(_, t)| t.type_name() == Some("SnakerProjectile"))?.1;
    let num = |node: Node, name: &str, default: f32| node.get(name).and_then(Node::float).unwrap_or(default);
    let radius = things.iter().filter(|(_, t)| t.type_name() == Some("Collision"))
        .flat_map(|(_, t)| t.get("primitives").into_iter().flat_map(|p| p.items()))
        .find_map(|primitive| {
            let packet = primitive.get("packet").filter(|p| p.type_name() == Some("CollisionSpherePacket"))?;
            let scale = glam::Vec3::from_slice(&instance_matrix(primitive)[0]).length();
            Some(packet.get("radius").and_then(Node::float)? * scale)
        })
        .unwrap_or(0.0);
    let deleter = things.iter().find(|(_, t)| t.type_name() == Some("TimeoutDeleter")).map(|(_, t)| *t);
    Some(WaveEffects {
        emitter: scene::read_placed_emitters(object).into_iter().next(),
        light: crate::point_light::read_light_and_deleter(object),
        rumble: scene::read_rumble(object),
        life: deleter.map_or(f32::INFINITY, |d| num(d, "time", 1.0)),
        // [game 0078cfd0] the ctor's defaults where the data is silent.
        check_ground_height: num(snaker, "checkGroundHeight", 1.0),
        init_check_ground_radius: num(snaker, "initCheckGroundRadius", 2.0),
        radius,
        sounds: scene::read_spawn_sounds(object),
    })
}

/// The key of a turret launcher's deployed turret's gun in `weapon_effects`: the turret's
/// `GunWeapon` (in `deployableTurretObj`) has `weaponName` 0 [data: blackout_turret], so its row
/// is keyed by the launcher's name with a hash of its own folded in. A key of the port's, not of
/// the game. [data] It meets no weapon's name of the 13 robot packs, nor another weapon's key
/// (`alt_projectile_rows_do_not_meet_a_weapon_name`, 2026-10-20). Only the roster is checked.
pub const fn turret_gun_key(launcher: u32) -> u32 {
    launcher ^ 0x7e1c_52a3
}

/// The key of a weapon's alternate projectile's row in `weapon_effects` (its
/// `chargeMaxProjectileObj`, `weapons::fire_choice`): the weapon's name with the field's own
/// hash folded in. A key of the port's, not of the game. [data] It meets no weapon's name of the
/// 13 robot packs, nor another weapon's key (`alt_projectile_rows_do_not_meet_a_weapon_name`,
/// 2026-10-20). Only the roster is checked.
pub const fn alt_projectile_key(weapon: u32) -> u32 {
    weapon ^ 0x3d25_9fad
}

fn read_weapon_effects<'a>(character: Node<'a>, name_of: &dyn Fn(u32) -> Option<&'a str>, presets: &crate::collision_masks::Presets) -> Vec<WeaponEffects> {
    let mut out = Vec::new();
    for file in character.get("weapons").into_iter().flat_map(|weapons| weapons.items()) {
        out.extend(object_weapon_effects(file.get("obj"), name_of, presets));
    }
    out
}

/// The effects rows of the weapons one object carries (`read_weapon_effects` of a weapon file's
/// `obj`, or a placed object's: Devastator's pods, `giant::read_pods`).
pub fn object_weapon_effects<'a>(object: Option<Node<'a>>, name_of: &dyn Fn(u32) -> Option<&'a str>, presets: &crate::collision_masks::Presets) -> Vec<WeaponEffects> {
    let mut out = Vec::new();
    {
        let attached = object.and_then(|o| o.get("attachments")).into_iter().flat_map(|a| a.items());
        for thing in attached.filter_map(|a| a.get("thing")) {
            // A grenade's projectile is laid out as a missile's: its `obj` holds the trail's
            // emitter and a DamageLink to the `damage` object (the blast's emitter, the
            // explosion and its SoundPlayer). [data: every playable pack, roster_effects /
            // weapon_fx_probe 2026-10-18]
            if !matches!(thing.type_name(), Some("GunWeapon" | "MissileLauncher" | "GrenadeLauncher" | "TurretLauncher")) {
                continue;
            }
            fn things(object: Option<Node<'_>>) -> Vec<Node<'_>> {
                object.and_then(|o| o.get("attachments")).into_iter().flat_map(|a| a.items()).filter_map(|a| a.get("thing")).collect()
            }
            let emitter = |things: &[Node]| things.iter().find(|t| t.type_name() == Some("ParticleEmitter")).and_then(|e| e.get("id")).and_then(Template::read);
            let name = thing.get("weaponName").and_then(Node::int).map_or(0, |h| h as u32);
            // `weapon`: the thing whose muzzle flash and fire sound the row takes; `missile`: the
            // object whose explosion and sounds a launcher's rounds take.
            let row = |name: u32, weapon: Node<'a>, object: Option<Node<'a>>, missile: Option<Node<'a>>| {
                let projectile = things(object);
                let damage = projectile.iter().find(|t| t.type_name() == Some("DamageLink")).and_then(|link| link.get("thing"));
                let let_go = things(damage);
                let destructable = projectile.iter().find(|t| t.type_name() == Some("Destructable")).copied();
                WeaponEffects {
                    emitters: object.map(scene::read_placed_emitters).unwrap_or_default(),
                    flight_lights: object.map(scene::read_lights).unwrap_or_default().into_iter().map(|(_, light)| light).collect(),
                    blast_light: let_go.iter().filter(|t| t.type_name() == Some("DetachLink"))
                        .filter_map(|link| link.get("thing")).find_map(crate::point_light::read_light_and_deleter),
                    blast_rumble: damage.and_then(scene::read_rumble),
                    after_blast: destructable.map(read_after_blast).unwrap_or_default(),
                    wave: destructable.and_then(read_wave),
                    debris: object.map(|o| crate::debris::read_pieces(o, name_of, presets)).unwrap_or_default(),
                    name,
                    muzzle_flash: weapon.get("muzzleFlashEffect").and_then(Template::read),
                    fire_sound: weapon.path(&["genericWeaponSoundTable", "fireSound"]).and_then(Node::int).map_or(0, |h| h as u32),
                    impact: read_impact(projectile.iter().find(|t| t.type_name() == Some("BulletTrace")).and_then(|trace| trace.get("impactFX"))),
                    trail: emitter(&projectile),
                    blast: emitter(&let_go),
                    blast_ssd: missile.map(crate::formats::scene::read_explosion_ssd).unwrap_or_default(),
                    flight_sounds: missile.map(scene::read_spawn_sounds).unwrap_or_default(),
                    blast_sounds: missile.map(scene::read_projectile_end_sounds).unwrap_or_default(),
                }
            };
            let thrown = thing.path(&["missileProjectileObj", "obj"]).or_else(|| thing.path(&["grenadeProjectileObj", "obj"]));
            let own = thing.path(&["bulletProjectileObj", "obj"]).or(thrown);
            out.push(row(name, thing, own, thrown));
            // The round the weapon fires instead while Starscream's special runs (or at a full
            // sniper's steady): `chargeMaxProjectileObj`, weapon `+0x3a8`. [game 007ae010, 007aaeb0]
            if let Some(alt) = thing.path(&["chargeMaxProjectileObj", "obj"]) {
                let launcher = thing.type_name() == Some("MissileLauncher");
                out.push(row(alt_projectile_key(name), thing, Some(alt), Some(alt).filter(|_| launcher)));
            }
            // A turret launcher's row above is its own muzzle flash (the throw). The deployed
            // turret carries a gun of its own in `deployableTurretObj`, laid out as any gun:
            // muzzle flash, fire sound, and its bullet's impacts. [data: blackout_turret's
            // GunWeapon, `w_muzzle_flash_blackout_gatling`; Ironhide's turret has a
            // MissileLauncher, which nothing fires (`notes\special-weapons.md`), so is not read]
            let turret = things(thing.path(&["deployableTurretObj", "obj"]));
            if let Some(gun) = turret.iter().find(|t| t.type_name() == Some("GunWeapon")) {
                // Its round's lights, rumble and smoke are not read for it (the turret's own side).
                out.push(WeaponEffects { emitters: Vec::new(), flight_lights: Vec::new(), blast_light: None, blast_rumble: None,
                    after_blast: Vec::new(), wave: None, debris: Vec::new(), ..row(turret_gun_key(name), *gun, gun.path(&["bulletProjectileObj", "obj"]), None) });
            }
        }
    }
    out
}

fn read_sounds(character: Node) -> Sounds {
    let id = |node: Option<Node>| node.and_then(Node::int).map_or(0, |h| h as u32);
    let table = |field: &str| id(character.path(&["soundTable", field]));
    Sounds {
        engine: table("vehicleTransmission"),
        drift_start: table("vehicleDriftStart"),
        suspension: table("vehicleSuspension"),
        turbo: table("vehicleTurbo"),
        turbo_ready: table("vehicleTurboReady"),
        drive: table("vehicleDrive"),
        hover: table("hoverSound"),
        footstep: id(character.path(&["footStepFX", "materialInfos"]).and_then(|infos| infos.at(0)).and_then(|info| info.get("soundOnImpact"))),
    }
}

/// The first node under `node` whose object label is `label`, depth first. Depth 40 is deeper than
/// a CharacterSpawnManager's definition sits (about 12 objects down).
pub fn find_label<'a>(node: Node<'a>, label: u32, depth: u32, seen: &mut std::collections::HashSet<usize>) -> Option<Node<'a>> {
    if depth > 40 || node.offset().is_some_and(|o| !seen.insert(o)) {
        return None;
    }
    if node.label() == Some(label) {
        return Some(node);
    }
    node.fields().into_iter().map(|(_, n)| n).chain(node.items()).find_map(|n| find_label(n, label, depth + 1, seen))
}

pub fn load(game_dir: &Path, pack_name: &str) -> Result<CharacterData, String> {
    let pack = Pack::open(&crate::formats::characterlist::pack_file(game_dir, pack_name))?;
    // The node whose fields are the character's roots: the data object itself for a `characters\`
    // pack; for one kept in a mission pack (`characterlist::ELSEWHERE`), the embedded scene of his
    // CharacterDefinition, in whichever data chunk holds it. [data]
    let elsewhere = crate::formats::characterlist::elsewhere(pack_name);
    let chunks = pack.of_type(pack::DATA).take(if elsewhere.is_some() { usize::MAX } else { 1 });
    let files = chunks.map(|chunk| DataFile::parse(pack.data(chunk).to_vec())).collect::<Result<Vec<_>, _>>()?;
    let (file, top) = match elsewhere {
        None => files.first().map(|f| (f, f.root())).ok_or("pack has no data object")?,
        Some(e) => {
            let label = crate::formats::hash::crc32(e.scene.as_bytes());
            files.iter()
                .find_map(|f| find_label(f.root(), label, 0, &mut std::collections::HashSet::new()).map(|n| (f, n)))
                .ok_or_else(|| format!("{pack_name}: no {} in {}", e.scene, e.file))?
        }
    };
    let name_of = |hash| file.name(hash);

    let mut robot = None;
    let mut vehicle = None;
    let mut animations = AnimSet::default();
    let mut vehicle_animations = Vec::new();
    let mut vehicle_set = Vec::new();
    let mut sounds = Sounds::default();
    let mut chatter = Vec::new();
    let mut name_id = 0;
    let (mut melee_impact, mut step_impact) = (ImpactPreset::default(), ImpactPreset::default());
    let mut climb_impact = Vec::new();
    let (mut tyre_skid, mut tyre_road) = (ImpactPreset::default(), ImpactPreset::default());
    let mut weapon_defs = Vec::new();
    let mut weapon_objects = Vec::new();
    let mut grenade_objects = Vec::new();
    let (mut turret_objects, mut turret_clips, mut missile_objects) = (Vec::new(), Vec::new(), Vec::new());
    let mut weapon_animations = Vec::new();
    let mut melee_weapons = Vec::new();
    let mut weapon_effects = Vec::new();
    let mut shockwaves = Vec::new();
    for (_, root) in top.fields() {
        for thing in root.get("things").into_iter().flat_map(|things| things.items()) {
            match thing.type_name() {
                Some("Object") if robot.is_none() => {
                    robot = scene::read_object(thing, name_of);
                    shockwaves = read_shockwaves(thing);
                }
                Some("Character") => {
                    sounds = read_sounds(thing);
                    chatter = thing.get("battleChatter").map(crate::chatter::read).unwrap_or_default();
                    name_id = thing.get("nameID").and_then(|n| n.int()).map_or(0, |v| v as u32);
                    melee_impact = read_impact(thing.path(&["particleHitTable", "meleeHitSparkPreset"]));
                    step_impact = read_impact(thing.get("footStepFX"));
                    climb_impact = ["climbLeftHandFX", "climbRightHandFX", "climbLeftFootFX", "climbRightFootFX", "climbSlideFX"]
                        .iter()
                        .map(|name| read_impact(thing.path(&["climbData", name])))
                        .collect();
                    tyre_skid = read_impact(thing.get("vehicleTireSkidFX"));
                    tyre_road = read_impact(thing.get("vehicleTireRoadFX"));
                    weapon_defs = weapons::read_defs(thing);
                    // The debris pieces' Collision presets. [data: bnxglobal.str chunk 2]
                    let presets = crate::collision_masks::Presets::load(game_dir).unwrap_or_default();
                    weapon_effects = read_weapon_effects(thing, &name_of, &presets);
                    weapon_objects = weapons::read_objects(thing, name_of);
                    grenade_objects = weapons::read_grenade_objects(thing, name_of);
                    turret_objects = weapons::read_turret_objects(thing, name_of);
                    turret_clips = weapons::read_turret_clips(&file, thing);
                    missile_objects = weapons::read_missile_objects(thing, name_of);
                    weapon_animations = weapons::read_object_clips(&file, thing);
                    melee_weapons = weapons::read_melees(&file, thing, name_of);
                    if let Some(object) = thing.path(&["transformModel", "vehicleProxy", "obj"]) {
                        vehicle = scene::read_object(object, name_of);
                        let attached = object.get("attachments").into_iter().flat_map(|a| a.items());
                        for bundle in attached.filter_map(|a| a.get("thing")).filter(|t| t.type_name() == Some("AnimRefBundle")) {
                            vehicle_animations.extend(anim::read_bundle(&file, bundle));
                            vehicle_set.extend(anim::read_bundle(&file, bundle));
                        }
                    }
                    if let Some(set) = thing.get("animSet") {
                        animations = anim::read_set(&file, set);
                    }
                }
                _ => {}
            }
        }
    }
    // The car model's clips also go in the animation set under their own set name, so the
    // special's clip is found by set and id like the robot's (`Actions::VEHICLE_SET`). [data]
    animations.sets.insert(crate::tuning::Actions::VEHICLE_SET.to_owned(), vehicle_set);
    left_cannon_on_its_barrel(name_id, &mut weapon_defs, &mut weapon_objects);
    // [deviation] Compensated scales drawn in a straight line (`Clip::even_out_compensated_scales`),
    // for Grindor only: the user's report is his, it clears all 30 of his spikes over 2 m
    // (`clip_spikes blackout 2`), and on Starscream (29 spans) it clears some and makes a new one
    // (his thrust flaps in `Trans_V2R_Hover`, 3.9 m). `TF_NO_EVEN` leaves the clips as the data has
    // them, for comparing.
    let robot = robot.ok_or("pack has no robot object")?;
    let mut evened = 0;
    let even = name_id == crate::formats::hash::crc32(b"Blackout") && std::env::var_os("TF_NO_EVEN").is_none();
    for (set, refs) in animations.sets.iter_mut().filter(|_| even) {
        let on = if set == crate::tuning::Actions::VEHICLE_SET { vehicle.as_ref() } else { Some(&robot) };
        if let Some(object) = on {
            evened += even_out_compensated_scales(refs.iter_mut(), object);
        }
    }
    if let Some(car) = vehicle.as_ref().filter(|_| even) {
        evened += even_out_compensated_scales(vehicle_animations.iter_mut(), car);
    }
    if evened > 0 && std::env::var_os("TF_EVEN_LOG").is_some() {
        eprintln!("{pack_name}: {evened} compensated scale spans evened out");
    }
    Ok(CharacterData {
        robot,
        vehicle,
        animations,
        vehicle_animations,
        name_id,
        library: Library::load(&pack),
        sounds,
        chatter,
        melee_impact,
        step_impact,
        climb_impact,
        tyre_skid,
        tyre_road,
        weapons: weapon_defs,
        weapon_objects,
        grenade_objects,
        turret_objects,
        turret_clips,
        missile_objects,
        weapon_animations,
        melee_weapons,
        weapon_effects,
        shockwaves,
        extra_sounds: Vec::new(),
        energon: None,
    })
}

/// Each clip's compensated scales evened out against `object`'s hierarchy; how many spans changed.
pub fn even_out_compensated_scales<'a>(clips: impl Iterator<Item = &'a mut AnimRef>, object: &ObjectDef) -> usize {
    let parent = |name: u32| {
        let node = object.nodes.iter().find(|n| n.name_hash == name)?;
        node.parent.map(|p| object.nodes[p].name_hash)
    };
    clips.filter_map(|r| r.clip.as_mut()).map(|clip| clip.even_out_compensated_scales(&parent)).sum()
}

/// The node Ironhide's left cannons hang on: the front of his left barrel, as `Weapon_R_FX_Attach`
/// is of his right. [data]
const IRONHIDE_LEFT_CANNON: u32 = crate::formats::hash::crc32(b"Weapon_L_FX_Attach");

/// [deviation, asked for by the user 2026-10-25] Ironhide's left cannons fire from the front of the
/// barrel. In his data all three left-hand weapons share one invisible object whose FireDummy and
/// MuzzleFlashDummy sit at (1.33, -0.29, -0.68), a place made for `ObjectAttachment_Hand_L` (the
/// one of them hung there): on that node it lands within 0.2 m of the barrel's mouth. The two hung
/// on `Weapon_L_FX_Attach`, a bone turned a quarter round from the hand, put their flash, smoke and
/// rounds 1.4 m inboard by his left hand, and the original's link (007b4f20 -> 0056c9b0,
/// LinkConstraint 0056c2f0, no offset) does the same as read. Here those two objects' dummies sit
/// at the object's origin, the bone itself, as his right cannons' do in the data.
fn left_cannon_on_its_barrel(name_id: u32, defs: &mut [WeaponDef], objects: &mut [Option<ObjectDef>]) {
    if name_id != crate::formats::hash::crc32(b"Ironhide") {
        return;
    }
    for (def, object) in defs.iter_mut().zip(objects.iter_mut()) {
        let Some(object) = object.as_mut().filter(|_| def.bone == IRONHIDE_LEFT_CANNON) else { continue };
        for node in object.nodes.iter_mut().filter(|n| n.name.contains("Dummy")) {
            node.bind[3] = [0.0; 3];
        }
        def.muzzle = glam::Vec3::ZERO;
        def.barrels = [glam::Vec3::ZERO; 3];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::hash::crc32;
    use crate::melee::surface;

    #[test]
    fn grindors_rear_rotor_housing_no_longer_flies_out_as_he_changes_back() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "blackout").unwrap();
        let car = data.vehicle.as_ref().unwrap();
        let rig = crate::pose::Rig::new(car);
        let clip = data.vehicle_animations.iter().find(|r| r.id == "Trans_V2R_B").and_then(|r| r.clip.as_ref()).unwrap();
        let node = car.nodes.iter().position(|n| n.name == "Rotorhousing_Back_L").unwrap();
        let at = |t: f32| rig.pose(clip, false, t)[node].translation;
        // The data's keys put it 16 m out at tick 31, half way between two places 0.2 m apart.
        for t in [30.5, 31.0, 31.5] {
            let mid = (at(t - 0.5) + at(t + 0.5)) * 0.5;
            assert!((at(t) - mid).length() < 0.5, "tick {t}: {:?} against {mid:?}", at(t));
        }
        assert!((at(31.0) - (at(30.0) + at(32.0)) * 0.5).length() < 0.5);
    }

    #[test]
    fn ironhides_left_cannons_fire_from_their_barrel_and_the_hand_weapon_keeps_its_place() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "ironhide").unwrap();
        let mut on_cannon = 0;
        for (def, object) in data.weapons.iter().zip(&data.weapon_objects) {
            let Some(object) = object else { continue };
            let dummy = object.nodes.iter().find(|n| n.name == "FireDummy").map(|n| n.bind[3]);
            if def.bone == IRONHIDE_LEFT_CANNON {
                on_cannon += 1;
                assert_eq!((def.muzzle, dummy), (glam::Vec3::ZERO, Some([0.0; 3])));
            } else if def.bone == crc32(b"ObjectAttachment_Hand_L") {
                assert!((def.muzzle - glam::Vec3::new(1.33, -0.29, -0.68)).length() < 0.02, "{:?}", def.muzzle);
            }
        }
        assert_eq!(on_cannon, 2);
    }

    #[test]
    fn anonymous_spawn_players_supply_the_punch_and_missile_sounds() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "bumblebee").unwrap();
        let launcher = data.weapons.iter().find(|w| w.kind == weapons::Kind::Launcher).unwrap();
        let audio = data.weapon_effects.iter().find(|w| w.name == launcher.name).unwrap();
        assert_eq!(audio.flight_sounds, [crc32(b"WPN_OPTIMUS_MISSLE_LOOP")]);
        assert_eq!(audio.blast_sounds, [crc32(b"EXP_OPTIMUS_MISSLE")]);
        let sounds: Vec<_> = data.robot.spawn_sound_players.iter().flat_map(|s| &s.2).copied().collect();
        assert_eq!(sounds, [crc32(b"IMP_CONC_LAND"); 3]);
        for object in std::iter::once(&data.robot).chain(&data.vehicle) {
            assert!(object.sound_players.iter().all(|p| p.node < object.nodes.len()),
                "installed SoundPlayer attachments must address their object's nodes");
        }
    }

    /// Devastator has no `characters\` pack: he loads from the Cairo City boss mission's pack, out of
    /// `CharDef_Devastator`'s embedded scene, with his six animation sets and his model. [data 2026-10-22]
    #[test]
    fn devastator_loads_from_the_boss_mission_pack() {
        let dir = crate::install::dir();
        if !dir.join(crate::formats::characterlist::ELSEWHERE[0].file).is_file() {
            return;
        }
        let data = load(dir, "devastator").unwrap();
        assert_eq!(data.name_id, crc32(b"Devastator"));
        assert!(data.vehicle.is_none());
        for (set, id) in [("IdleStandSet", "Idle"), ("WalkSet", "Walk"), ("WalkSet", "IdleToWalk"), ("TurnSet", "TurnLeft"),
            ("TurnSet", "TurnRight"), ("CombatSet", "Attack1"), ("PlayAnimSet", "Idle2AttackSpecial"), ("PlayAnimSet", "AttackSpecial"),
            ("DeathSet", "Death")] {
            assert!(data.animations.find(set, id).and_then(|r| r.clip.as_ref()).is_some(), "{set}/{id}");
        }
        let walk = data.animations.find("WalkSet", "Walk").and_then(|r| r.clip.as_ref()).unwrap();
        println!("devastator: {} nodes, walk {:.2} m/s, {} meshes", data.robot.nodes.len(), walk.forward_speed(), data.library.meshes.len());
        assert!(walk.forward_speed() > 0.0);
    }

    /// `optimusflight` and `megatronflight` carry one damage-less `ShockWave` each on their
    /// robot object, Devastator two of 25 [data]: only he is the giant (`giant::is_giant`).
    /// Keying the giant on "has ShockWaves" made both flyers walk-only (2026-10-25).
    #[test]
    fn only_devastator_is_the_giant() {
        let dir = crate::install::dir();
        if !dir.join("characters").is_dir() || !dir.join(crate::formats::characterlist::ELSEWHERE[0].file).is_file() {
            return;
        }
        for pack in ["bumblebee", "optimus", "ratchet", "ironhide", "megatron", "longhaul", "audir8", "jetfire", "firestorm",
            "starscream", "blackout", "optimusflight", "megatronflight", "devastator"] {
            let data = load(dir, pack).unwrap();
            let waves = match pack {
                "optimusflight" | "megatronflight" => 1,
                "devastator" => 2,
                _ => 0,
            };
            assert_eq!(data.shockwaves.len(), waves, "{pack}");
            assert_eq!(crate::giant::is_giant(&data), pack == "devastator", "{pack}");
        }
    }

    /// Starscream's special makes his weapons fire their `chargeMaxProjectileObj` (007ae010,
    /// 007aaeb0): the Gatling gun's `gatlinggunspecial` lands with its own impacts and no blast,
    /// the launcher's `nullray` leaves its own explosion. [data]
    #[test]
    fn starscreams_special_rounds_are_his_weapons_alternate_projectiles() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "starscream").unwrap();
        let (gatling, launcher) = (-1197494719i32 as u32, -1877059191i32 as u32);
        let def = |name| data.weapons.iter().find(|w| w.name == name).unwrap();
        let row = |name| data.weapon_effects.iter().find(|w| w.name == name).unwrap();
        assert!(def(gatling).alt_projectile && def(gatling).alt_explosion.is_none());
        assert_eq!(weapons::fire_choice(true, 0.0), (weapons::fire_flags::SPECIAL, true));
        assert_eq!(weapons::fire_choice(false, 1.0), (weapons::fire_flags::STEADIED, true));
        assert_eq!(weapons::fire_choice(false, 0.99), (0, false));
        let (own, alt) = (row(gatling), row(alt_projectile_key(gatling)));
        let sounds = |r: &WeaponEffects| r.impact.infos.iter().map(|i| i.sound).collect::<Vec<_>>();
        let radius = |r: &WeaponEffects| r.impact.ssd.first().map(|s| s.radius);
        println!("gatling own {:?} r {:?}; special {:?} r {:?}", sounds(own), radius(own), sounds(alt), radius(alt));
        assert_ne!(sounds(own), sounds(alt));
        assert_eq!(radius(alt), Some(3.5));
        let launched = def(launcher);
        let (own_blast, alt_blast) = (launched.explosion_for(false).unwrap(), launched.explosion_for(true).unwrap());
        println!("launcher own damage {} radius {}; special damage {} radius {}", own_blast.damage, own_blast.radius, alt_blast.damage, alt_blast.radius);
        assert_eq!((own_blast.damage, alt_blast.damage), (20.0, 5.0));
        assert!(!row(alt_projectile_key(launcher)).blast_sounds.is_empty());
    }

    /// The port's alternate-projectile key meets no weapon name of the roster.
    #[test]
    fn alt_projectile_rows_do_not_meet_a_weapon_name() {
        if crate::install::missing() { return; }
        let packs = ["bumblebee", "optimus", "ratchet", "megatron", "longhaul", "firestorm", "blackout", "audir8",
            "starscream", "jetfire", "ironhide", "optimusflight", "megatronflight"];
        let mut names = Vec::new();
        let mut rows = 0;
        for pack in packs {
            let data = load(crate::install::dir(), pack).unwrap();
            names.extend(data.weapons.iter().map(|w| w.name));
            rows += data.weapons.iter().filter(|w| w.alt_projectile).count();
        }
        println!("{rows} weapons of {} have an alternate projectile", names.len());
        for name in &names {
            assert!(!names.contains(&alt_projectile_key(*name)));
            assert!(!names.contains(&turret_gun_key(*name)));
            assert_ne!(alt_projectile_key(*name), turret_gun_key(*name));
            // Across weapons too: `alt(a) == turret(b)` for another weapon's name b would
            // share a row (the two folds differ by a constant, so such a pair is possible).
            assert!(!names.iter().any(|other| alt_projectile_key(*name) == turret_gun_key(*other)));
        }
    }

    #[test]
    fn blackouts_turret_gun_has_a_row_of_its_own() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "blackout").unwrap();
        let launchers: Vec<u32> = data.weapons.iter().filter(|w| w.kind == crate::weapons::Kind::Turret).map(|w| w.name).collect();
        assert!(!launchers.is_empty());
        for launcher in launchers {
            let gun = data.weapon_effects.iter().find(|w| w.name == turret_gun_key(launcher)).expect("a row for the turret's gun");
            // [data] blackout_turret's GunWeapon: fireSound -278946228, `w_muzzle_flash_blackout_gatling`.
            assert_eq!(gun.fire_sound, -278_946_228i32 as u32);
            assert!(gun.muzzle_flash.is_some());
            assert!(!gun.impact.infos.is_empty(), "its bullet's impactFX");
            assert!(data.weapon_effects.iter().any(|w| w.name == launcher && w.muzzle_flash.is_some()), "the launcher's own flash");
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    /// r3-fx: the micro missile's five trail emitters on their nodes, its flight light, ten
    /// ribbons, the blast's flash and rumble. [data: bumblebee.txt 551866-556188]
    #[test]
    fn bumblebees_missile_carries_its_emitters_lights_ribbons_and_blast_rumble() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "bumblebee").unwrap();
        let name = -364_875_044i32 as u32;
        let row = data.weapon_effects.iter().find(|w| w.name == name).unwrap();
        let flash = row.blast_light.as_ref().expect("detach1's Light");
        assert_eq!((flash.light.range, flash.light.attenuate, flash.time), (15.0, 7.0, 0.25));
        assert!(close(flash.fade(), 0.13) && close(flash.light.colour.y, 4.43137) && close(flash.light.colour.z, 2.70588));
        assert_eq!(row.blast_rumble, Some(crate::rumble::RumbleDef { kind: 1, duration: 0.5, strength: 0.75, scale_type: 1, distance_near: 1.0, distance_far: 85.0 }));
        let index = data.weapons.iter().position(|w| w.name == name).unwrap();
        let object = data.missile_objects[index].as_ref().unwrap();
        assert_eq!(object.emitters.len(), 5);
        for (e, node) in object.emitters.iter().zip(7..=11) {
            assert_eq!((e.template.name, e.node), (0xe3e1_a1b3, node));
            assert!(e.use_rot && e.auto_start && !e.use_scale && !e.delete_when_done && !e.instant_kill);
            assert_eq!(e.bind, object.nodes[node].bind, "the bind is the node's, in the object's space");
        }
        assert_eq!(row.emitters.len(), 5);
        let (node, light) = &object.lights[0];
        assert_eq!((*node, light.range, light.attenuate), (1, 6.0, 2.0));
        assert!(close(light.colour.y, 0.839216) && close(light.colour.z, 0.498039));
        assert_eq!(row.flight_lights.len(), 1);
        assert_eq!(object.ribbons.len(), 10);
        assert!(row.wave.is_none() && row.after_blast.is_empty());
    }

    #[test]
    fn ironhides_grenade_has_a_ribbon_a_blast_light_and_ash_rain() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "ironhide").unwrap();
        let name = 2_128_319_374u32;
        let row = data.weapon_effects.iter().find(|w| w.name == name).unwrap();
        let index = data.weapons.iter().position(|w| w.name == name).unwrap();
        let ribbons = &data.grenade_objects[index].as_ref().unwrap().ribbons;
        assert_eq!(ribbons.len(), 1);
        let r = &ribbons[0];
        assert_eq!(r.links, [crc32(b"FX_ribbonA"), crc32(b"FX_ribbonB")]);
        assert_eq!((r.trails, r.subdivisions, r.trigger_speed, r.enabled), (8, 4, 0.25, true));
        let flash = row.blast_light.as_ref().unwrap();
        assert_eq!((flash.light.range, flash.light.attenuate, flash.time), (12.0, 7.0, 1.1));
        assert!(close(flash.fade(), 0.8));
        let rumble = row.blast_rumble.unwrap();
        assert!(close(rumble.strength, 0.85) && rumble.distance_far == 75.0);
        assert_eq!(row.after_blast.len(), 1);
        let ash = &row.after_blast[0];
        assert_eq!(ash.template.name, crc32(b"smk_impact_bldg_ashrain"));
        assert_eq!((ash.on, ash.off), ((0.5, 0.5), (4.0, 5.0)));
    }

    /// The energon cannon's wave object: its emitter is its picture, its deleter its life. [data]
    #[test]
    fn grindors_and_jetfires_waves_are_read_from_their_objects() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "blackout").unwrap();
        let row = data.weapon_effects.iter().find(|w| w.name == 252_926_678).unwrap();
        let wave = row.wave.as_ref().expect("the radial_attack object");
        assert_eq!(wave.emitter.as_ref().map(|e| e.template.name), Some(crc32(b"w_exp_energoncannon")));
        assert!(wave.emitter.as_ref().unwrap().delete_when_done);
        assert_eq!((wave.check_ground_height, wave.init_check_ground_radius, wave.life), (3.0, 6.0, 1.75));
        assert!(close(wave.radius, 1.36743));
        let light = wave.light.as_ref().unwrap();
        assert_eq!((light.light.range, light.light.attenuate), (15.0, 11.0));
        assert!(close(light.light.colour.x, 0.439216) && close(light.light.colour.y, 0.678431) && close(light.fade(), 0.3));
        let rumble = wave.rumble.unwrap();
        assert!(close(rumble.strength, 0.75) && rumble.duration == 0.5 && rumble.distance_far == 85.0);
        assert!(row.blast_light.is_none());
        let names: Vec<_> = row.after_blast.iter().map(|p| (p.template.name, p.on, p.off)).collect();
        println!("{names:x?}");
        assert_eq!(row.after_blast.len(), 3);
        let find = |n: &[u8]| row.after_blast.iter().find(|p| p.template.name == crc32(n)).unwrap();
        assert_eq!((find(b"w_impact_micromissile").on, find(b"w_impact_micromissile").off), ((0.25, 0.25), (2.5, 3.5)));
        assert_eq!((find(b"smk_impact_bldg_debris").on, find(b"smk_impact_bldg_debris").off), ((1.0, 3.0), (4.0, 5.0)));
        assert_eq!((find(b"smk_impact_bldg_ash").on, find(b"smk_impact_bldg_ash").off), ((2.0, 2.5), (4.0, 5.0)));
        let z = find(b"smk_impact_bldg_ash").offset.z;
        assert!((z - 0.505).abs() < 0.01, "FX_Dummy sits 0.505 up: {z}");
        let jetfire = load(crate::install::dir(), "jetfire").unwrap();
        let row = jetfire.weapon_effects.iter().find(|w| w.name == -934_361_366i32 as u32).unwrap();
        assert_eq!(row.wave.as_ref().map(|w| w.life), Some(1.0));
    }

    /// The spec (r3-fx 1.2) had no rumble here; the damage object's sixth attachment is one
    /// (megatron dump 807937: 75, far 85, past where the survey stopped reading). [data]
    #[test]
    fn megatrons_fusion_arm_flashes_and_rumbles() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "megatron").unwrap();
        let row = data.weapon_effects.iter().find(|w| w.name == 270_503_727).unwrap();
        let flash = row.blast_light.as_ref().unwrap();
        assert_eq!((flash.light.range, flash.light.attenuate, flash.time), (12.0, 7.0, 1.0));
        assert!(close(flash.fade(), 0.5));
        let rumble = row.blast_rumble.expect("the damage object's ControllerRumble");
        assert!(close(rumble.strength, 0.75) && rumble.distance_far == 85.0);
    }

    /// The rumble's distance scale at the blast's numbers (007dbdc0). [game]
    #[test]
    fn a_blast_rumble_weakens_to_nothing_at_its_far_distance() {
        let def = crate::rumble::RumbleDef { kind: 1, duration: 0.5, strength: 0.75, scale_type: 1, distance_near: 1.0, distance_far: 85.0 };
        assert!(close(def.strength_at(43.0).unwrap(), 0.375));
        assert_eq!(def.strength_at(86.0), None);
    }

    /// A node riding a missile: the frame's turn and place applied to the node's bind, unit axes.
    #[test]
    fn an_emitter_rides_its_node_on_the_missile() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "bumblebee").unwrap();
        let index = data.weapons.iter().position(|w| w.name == -364_875_044i32 as u32).unwrap();
        let e = &data.missile_objects[index].as_ref().unwrap().emitters[0];
        let frame = glam::Affine3A::from_rotation_translation(weapons::look_along(glam::Vec3::X), glam::Vec3::new(0.0, 0.0, 10.0));
        let bind = rows_affine(e.bind);
        let place = frame * bind;
        let t = glam::Vec3::from(bind.translation);
        assert!((glam::Vec3::from(place.translation) - (weapons::look_along(glam::Vec3::X) * t + glam::Vec3::new(0.0, 0.0, 10.0))).length() < 1e-5);
        let unit = |v: glam::Vec3A| v.normalize();
        let m = place.matrix3;
        assert!((unit(m.x_axis).length() - 1.0).abs() < 1e-6 && (unit(m.y_axis).length() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn an_impact_preset_falls_back_on_the_default_surface() {
        let info = |materials: &[u32], sound| ImpactInfo { materials: materials.to_vec(), sound, effect: None, template: None, effect_flags: 0 };
        let preset = ImpactPreset { infos: vec![info(&[surface::DEFAULT, 7], 1), info(&[8, 9], 2)], ..Default::default() };
        assert_eq!(preset.pick(9).map(|i| i.sound), Some(2));
        assert_eq!(preset.pick(7).map(|i| i.sound), Some(1));
        assert_eq!(preset.pick(1234).map(|i| i.sound), Some(1), "an unlisted surface is the default one");
        let bare = ImpactPreset { infos: vec![info(&[8], 2)], ..Default::default() };
        assert!(bare.pick(1234).is_none());
    }

    /// Needs the game install, as the other data tests do.
    #[test]
    fn his_tyres_sound_by_the_ground_under_them() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "bumblebee").unwrap();
        let event = |name: &str| Some(crc32(name.as_bytes()));
        let skid = |name: &str| data.tyre_skid.pick(crc32(name.as_bytes())).map(|info| info.sound);
        let road = |name: &str| data.tyre_road.pick(crc32(name.as_bytes())).map(|info| info.sound);
        assert_eq!(skid("0 - Default"), event("VEH_SPORTS_SKID_PAVE"));
        assert_eq!(skid("Dirt"), event("VEH_SPORTS_SKID_DIRT"));
        assert_eq!(skid("Sand"), event("VEH_SPORTS_SKID_DIRT"));
        assert_eq!(road("0 - Default"), event("VEH_ROAD_PAVE"));
        assert_eq!(road("Dirt"), event("VEH_ROAD_GRASS"));
        assert_eq!(road("Mud"), event("VEH_ROAD_MUD"));
        assert_eq!(road("Sand"), event("VEH_ROAD_SAND"));
        assert_eq!(road("a surface no entry lists"), event("VEH_ROAD_PAVE"));
        assert_eq!(Some(data.sounds.suspension), event("VEH_SUSPENSION_SM"));
    }

    /// Megatron's tank lays its marks from its treads: `tireName` names `Tread_Back_L`... on
    /// nodes of his vehicle, one for each wheel by the game's map (`vehicle::wheel_of_node`,
    /// 01185740). [data] [game] They were dropped when only the tyres' names counted (the
    /// user's report, 2026-10-25: "no trail when in vehicle mode").
    #[test]
    fn megatrons_marks_are_his_treads_one_a_wheel() {
        if crate::install::missing() { return; }
        let Ok(data) = load(crate::install::dir(), "megatron") else { return };
        let tank = data.vehicle.as_ref().unwrap();
        let mut wheels: Vec<usize> = tank.skid_marks.iter().map(|m| {
            assert!(tank.nodes.iter().any(|n| n.name_hash == m.tyre), "{:#010x} is a node of the tank", m.tyre);
            crate::vehicle::wheel_of_node(m.tyre).expect("a tread the map knows")
        }).collect();
        wheels.sort();
        assert_eq!(wheels, [0, 1, 2, 3]);
    }

    /// Needs the game install.
    #[test]
    fn his_car_has_a_skid_mark_and_smoke_for_each_tyre() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "bumblebee").unwrap();
        let car = data.vehicle.as_ref().unwrap();
        // Each sits on an effect node of its own; `tireName` names the tyre's node, where
        // the points are put (`FUN_0080bc30`).
        let tyre = |m: &crate::skidmark::SkidMarkDef| car.nodes.iter().find(|n| n.name_hash == m.tyre).map(|n| n.name.as_str());
        let tyres: Vec<_> = car.skid_marks.iter().filter_map(tyre).collect();
        assert_eq!(tyres, ["Tire_Rear_L", "Tire_Rear_R", "Tire_Front_R", "Tire_Front_L"]);
        for mark in &car.skid_marks {
            assert_eq!((mark.point_length, mark.point_width, mark.lifetime), (2.0, 0.3, 10.0));
            assert_eq!(mark.colour, [191.0 / 255.0; 4]);
            let material = data.library.materials.get(&mark.material).expect("the material is in his pack");
            assert_eq!(material.name, "bumblebeevehicle.fx_tireskid01");
            assert!(material.texture("diffuse_texture1_texture").is_some_and(|t| data.library.textures.contains_key(&t)));
        }
        // The tyres' effects by the ground: smoke on the road, dirt on dirt.
        let effect = |preset: &ImpactPreset, name: &str| preset.pick(crc32(name.as_bytes())).and_then(|info| info.template.as_ref()).map(|t| t.name);
        assert_eq!(effect(&data.tyre_skid, "0 - Default"), Some(crc32(b"v_smoke_exhaust_skid_bumblebee")));
        assert_eq!(effect(&data.tyre_skid, "Dirt"), Some(crc32(b"v_dirt_skid")));
        assert_eq!(effect(&data.tyre_road, "0 - Default"), Some(crc32(b"v_asphalt_constant")));
        assert!(data.tyre_skid.infos.iter().chain(&data.tyre_road.infos).all(|info| info.effect_flags == 0x100), "tied to the car");
        assert_eq!(car.collision_fx.len(),1);
        let collision=&car.collision_fx[0];
        assert_eq!((collision.mask,collision.types,collision.min_speed,collision.max_speed),
            (1,7,0.05,1e7));
        assert!(collision.align);
        assert_eq!(collision.sounds,[[0;3];2],"Bumblebee has no CollisionFX SFX overrides");
        assert!(collision.particles.iter().flatten().all(Option::is_some),
            "hard/soft impact, roll and scrape templates resolve from the install");
    }

    /// Needs the game install, as the other data tests do.
    #[test]
    fn installed_chatter_feet_and_collision_sounds_resolve_the_original_bindings() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "bumblebee").unwrap();
        assert_eq!(data.chatter.len(), 115);
        let generic_attack: Vec<_> = data.chatter.iter().filter(|l| l.action == crate::chatter::ATTACK
            && l.category == crate::chatter::GENERIC && l.subcategory == crate::chatter::ENEMY).collect();
        assert_eq!(generic_attack.len(), 4);
        assert!(generic_attack.iter().all(|l| l.frequency == 0.333));
        let feet = &data.robot.footsteps;
        assert_eq!(feet.len(), 2);
        assert_eq!(feet.iter().map(|f| data.robot.nodes[f.node].name.as_str()).collect::<Vec<_>>(), ["Midfoot_L", "Midfoot_R"]);
        assert!(feet.iter().all(|f| f.height == 1.5));
        assert_ne!(feet[0].side, feet[1].side);
        let car = data.vehicle.as_ref().unwrap();
        assert_eq!(car.collision_sounds.len(), 1);
        let mut sounds = car.collision_sounds[0].clone();
        assert_eq!((sounds.filter.mask, sounds.filter.types, sounds.filter.min_speed), (1, 5, 0.08));
        let impact = sounds.select(crate::collisionfx::Kind::Impact, false, false).unwrap();
        assert_eq!(impact.id, crc32(b"VEH_CAR_IMP_OBJ"));
        assert!(impact.attached && !impact.once);
        let scrape = sounds.select(crate::collisionfx::Kind::Scrape, true, false).unwrap();
        assert_eq!(scrape.id, crc32(b"VEH_IMP_SKID_WALL"));
        assert!(scrape.attached);
        assert_ne!(sounds.select(crate::collisionfx::Kind::Impact, true, true).unwrap().id, impact.id);
        assert_eq!(data.robot.sound_players.iter().map(|p| (p.script, p.id)).collect::<Vec<_>>(), [
            (crc32(b"allspark1"), crc32(b"UI_MP_ALLSPARK_PICKUP")),
            (crc32(b"allspark1"), crc32(b"UI_MP_ALLSPARK_ENERGY_LP")),
            (crc32(b"jet_dodge"), crc32(b"ANIM_AUTOBOT_DODGE_THRUST")),
            (crc32(b"specialAttack"), crc32(b"WPN_BUMBLEBEE_SPEC_FIRE")),
        ]);
        assert!(data.robot.sound_players.iter().chain(&car.sound_players).all(|p| p.wait_for_trigger && !p.once));
        assert!(data.robot.sound_players.iter().all(|p| p.retrigger));
        assert!(!car.sound_players.iter().find(|p| p.script == crc32(b"FX_turbocharged")).unwrap().retrigger);
    }

    /// Needs the game install, as the other data tests do.
    #[test]
    fn his_punch_sounds_by_what_it_strikes() {
        if crate::install::missing() { return; }
        let data = load(crate::install::dir(), "bumblebee").unwrap();
        let sound = |name: &str| data.melee_impact.pick(crc32(name.as_bytes())).map(|info| info.sound);
        let event = |name: &str| Some(crc32(name.as_bytes()));
        assert_eq!(data.melee_impact.infos.len(), 7);
        assert_eq!(sound("UpperBody"), event("H2H_PUNCH_SM"));
        assert_eq!(sound("Concrete"), event("H2H_PUNCH_SM_CONC"));
        assert_eq!(sound("MetalSoft"), event("H2H_PUNCH_SM_METAL"));
        assert_eq!(sound("Glass"), event("H2H_PUNCH_SM_GLASS"));
        assert_eq!(sound("no such surface"), event("H2H_PUNCH_SM"));
        // Every entry's effect is tied to the object hit, and is the same template.
        assert!(data.melee_impact.infos.iter().all(|info| info.effect_flags == 0x100));
        assert!(data.melee_impact.infos.iter().all(|info| info.effect == Some(crc32(b"r_hit_melee"))));
        // The step preset's first entry is the one the footsteps have used so far.
        assert_eq!(data.step_impact.pick(surface::DEFAULT).map(|info| info.sound), Some(data.sounds.footstep));
    }

    /// Needs the game install. The blasts of his two ground punches name a camera shake the
    /// game has, and carry a pad rumble.
    #[test]
    fn his_ground_punches_shake_the_camera_and_the_pad() {
        if crate::install::missing() { return; }
        let game = crate::install::dir();
        let data = load(game, "bumblebee").unwrap();
        let tuning = crate::tuning::load(game, "Bumblebee").unwrap();
        let spawner = |name: &str| data.robot.spawners.iter().find(|s| s.script_name == crc32(name.as_bytes())).unwrap();
        let (strong, weak) = (spawner("FX_groundpunch"), spawner("FX_groundpunchweak"));
        assert_eq!(strong.explosion.camera_shake, crc32(b"MassiveCameraShake"));
        assert_eq!(weak.explosion.camera_shake, crc32(b"MedHardCameraShake"));
        let massive = tuning.shakes[&strong.explosion.camera_shake];
        assert_eq!((massive.duration, massive.half_duration, massive.full_dist, massive.max_dist), (2.5, 1.25, 20.0, 90.0));
        assert!((massive.ang_horiz - 1.2_f32.to_radians()).abs() < 1e-6 && massive.pos_vert == 0.35);
        let med_hard = tuning.shakes[&weak.explosion.camera_shake];
        assert_eq!((med_hard.duration, med_hard.half_duration, med_hard.full_dist, med_hard.max_dist), (0.65, 0.15, 25.0, 30.0));
        // The shakes his punch CLIPS name are in no file of the game: they start nothing.
        assert!(!tuning.shakes.contains_key(&crc32(b"MediumShake1_16f")));
        assert!(!tuning.shakes.contains_key(&crc32(b"StrongCameraShake")));
        assert_eq!(tuning.shakes.len(), 14);
        let rumble = |s: &crate::formats::scene::SpawnerDef| {
            let r = s.rumble.unwrap();
            (r.kind, r.duration, (r.strength * 100.0).round(), r.scale_type, r.distance_near, r.distance_far)
        };
        assert_eq!(rumble(strong), (1, 0.7, 55.0, 1, 1.0, 25.0));
        assert_eq!(rumble(weak), (1, 0.3, 55.0, 1, 1.0, 25.0));
        // The preset rumbles, by number: a melee hit (strong, fast), the climb (moved,
        // sliding), the car (wall, ground, turbo).
        use crate::rumble::{Preset, preset};
        let p = |n: usize| tuning.rumble_presets[n];
        assert_eq!(tuning.rumble_presets.len(), 15);
        assert_eq!(p(preset::STRONG_HIT_ATTACKER), Preset { kind: 1, duration: 0.45, strength: 1.0 });
        assert_eq!(p(preset::FAST_HIT_ATTACKER), Preset { kind: 1, duration: 0.25, strength: 1.0 });
        assert_eq!(p(preset::CLIMB_MOVED), Preset { kind: 3, duration: 0.0, strength: 0.4 });
        assert_eq!(p(preset::CLIMB_SLIDING), Preset { kind: 3, duration: 0.2, strength: 0.2 });
        assert_eq!(p(preset::VEHICLE_HIT_WALL), Preset { kind: 1, duration: 0.25, strength: 1.0 });
        assert_eq!(p(preset::VEHICLE_HIT_GROUND), Preset { kind: 1, duration: 0.3, strength: 0.75 });
        assert_eq!(p(preset::VEHICLE_TURBO), Preset { kind: 1, duration: 0.35, strength: 0.75 });
    }

    /// Needs the game install. Who starts his other effects, as his pack has it: the
    /// clips' events, the step preset and the weapons.
    #[test]
    fn his_clips_steps_and_weapons_name_their_effects() {
        if crate::install::missing() { return; }
        use crate::formats::anim::EffectEventKind;
        let data = load(crate::install::dir(), "bumblebee").unwrap();
        let clip = |set: &str, id: &str| data.animations.sets[set].iter().find(|r| r.id == id).and_then(|r| r.clip.as_ref()).unwrap();
        // A landing: dust at a foot at tick 8, left where it was made.
        let land: Vec<_> = clip("IdleStandSet", "Jump_Land")
            .effect_events
            .iter()
            .filter_map(|e| match &e.kind {
                EffectEventKind::Spawn { id, template, track, use_rotation, .. } => Some((e.time, *id, template.name, *track, *use_rotation, template.unread())),
                _ => None,
            })
            .collect();
        assert_eq!(land, [(8.0, 0, crc32(b"r_contactdust_landing"), false, false, Vec::new())]);
        // Climbing: steps of hands and feet with the climbing sides, and a preset for each
        // of the five whose effect for a plain wall is the climbing dust.
        let climbing = data.animations.sets.iter().flat_map(|(_, clips)| clips).filter_map(|r| r.clip.as_ref()).flat_map(|c| &c.effect_events);
        let limbs: std::collections::BTreeSet<usize> =
            climbing.filter_map(|e| if let EffectEventKind::ClimbStep { limb } = e.kind { Some(limb) } else { None }).collect();
        assert!(limbs.len() >= 4, "{limbs:?}");
        assert_eq!(data.climb_impact.len(), 5);
        let dust = data.climb_impact[0].pick(surface::DEFAULT).and_then(|info| info.template.as_ref()).unwrap();
        assert_eq!(dust.name, crc32(b"r_contactdust_climb"));
        // A walk: a step for each foot, each on its foot's node.
        let steps: Vec<_> = clip("WalkSet", "Walk").effect_events.iter().filter(|e| matches!(e.kind, EffectEventKind::FootStep { .. })).map(|e| (e.time, e.node)).collect();
        assert_eq!(steps, [(18.0, crc32(b"Foot_L")), (50.0, crc32(b"Foot_R"))]);
        for event in clip("WalkSet", "Walk").effect_events.iter() {
            if let EffectEventKind::FootStep { side } = event.kind {
                assert!(data.robot.footsteps.iter().any(|foot| foot.side == side), "step side must name a component");
            }
        }
        let orientation_flags: Vec<_> = data.step_impact.infos.iter().chain(data.climb_impact.iter().flat_map(|p| &p.infos))
            .filter(|info| info.template.is_some()).map(|info| info.effect_flags).collect();
        assert!(orientation_flags.iter().all(|flags| matches!(flags, 1 | 0x100)), "step/climb orientation flags {orientation_flags:x?}");
        let step = data.step_impact.pick(surface::DEFAULT).and_then(|info| info.template.as_ref()).unwrap();
        assert_eq!((step.name, step.unread()), (crc32(b"r_footstep_small_concrete"), Vec::new()));
        // The cannon: a flash, and an impact effect by the surface; the launcher: a
        // flash, a trail and an explosion; the car's gun: a flash and a trail.
        let names: Vec<_> = data.weapon_effects.iter().map(|w| (w.muzzle_flash.as_ref().map(|t| t.name), w.impact.infos.len(), w.trail.as_ref().map(|t| t.name), w.blast.as_ref().map(|t| t.name))).collect();
        assert_eq!(
            names,
            [
                (Some(crc32(b"w_muzzle_solar_plasma_cannon")), 15, None, None),
                (Some(crc32(b"w_muzzle_flash_micro_missiles")), 0, Some(crc32(b"w_bullet_trail_micro_missiles2")), Some(crc32(b"w_exp_micro_missiles2"))),
                (Some(crc32(b"w_muzzle_flash_vehicle_autobot")), 15, Some(crc32(b"w_bullet_trail_vehicle_autobot")), None),
            ]
        );
        // Every action and domain of the effects his clips start is one the port runs
        // (the values in slots 13 to 15 are kept by nothing here).
        for r in data.animations.sets.values().flatten() {
            for event in r.clip.iter().flat_map(|clip| &clip.effect_events) {
                if let EffectEventKind::Spawn { template, .. } = &event.kind {
                    assert!(template.unread().iter().all(|what| what.starts_with("slot")), "{}: {:?}", r.id, template.unread());
                }
            }
        }
    }

    /// Needs the game install. The effect of a melee hit and of the two ground punches,
    /// as his pack has them, and that everything in them is something the port runs.
    #[test]
    fn his_hit_and_punch_effects_are_read_whole() {
        if crate::install::missing() { return; }
        use crate::particles::{Domain, Kind};
        use glam::Vec3;
        let data = load(crate::install::dir(), "bumblebee").unwrap();
        let hit = data.melee_impact.pick(surface::UPPER_BODY).unwrap();
        assert_eq!(hit.effect, Some(crc32(b"r_hit_melee")));
        let t = hit.template.as_ref().unwrap();
        assert_eq!(t.unread(), Vec::<String>::new());
        // The flash, the electric flicker, the sparks.
        let e: Vec<_> = t.elements.iter().map(|e| (e.max_particles, e.end, e.local_space, e.actions.len())).collect();
        assert_eq!(e, [(1, 0.3, true, 4), (2, 0.6, true, 3), (3, 0.6, false, 5)]);
        let flash = &t.elements[0];
        assert_eq!(flash.renderer.texture, 0x47b1_03e3);
        assert!(flash.renderer.texture_path.ends_with("fx_blueflash.dds") && flash.renderer.blend_mode == 1);
        assert!(data.library.textures.contains_key(&flash.renderer.texture), "the texture is in his pack");
        let Kind::Burst { count, slots } = &flash.actions[0].kind else { panic!("{:?}", flash.actions[0]) };
        assert_eq!((*count, slots[1], slots[9]), (2.0, Some(Domain::Point(Vec3::new(5.0, 0.0, 0.0))), Some(Domain::Point(Vec3::new(0.2, 0.0, 0.0)))));
        assert_eq!(flash.actions[1].kind, Kind::LinearScale { delta: 7.0 });
        assert_eq!(flash.actions[2].kind, Kind::AlphaFade { begin_alpha: 1.0, fade_begin: 0.5 });
        let sparks = &t.elements[2];
        assert!(sparks.renderer.rectangle() && sparks.renderer.velocity_tracked);
        assert_eq!(t.elements[1].renderer.frames, (2, 2));
        // The punches: both on `Hand_R`, upright and tied to the hand; each texture there.
        let effect = |name: &str| data.robot.effects.iter().filter(|e| e.script_name == crc32(name.as_bytes())).collect::<Vec<_>>();
        let (strong, weak) = (effect("FX_groundpunch"), effect("FX_groundpunchweak"));
        assert_eq!(weak.len(), 1, "{:?}", weak.iter().map(|e| e.template.name).collect::<Vec<_>>());
        assert_eq!(weak[0].template.name, crc32(b"r_attack_radial_small"));
        assert_eq!(weak[0].mesh.as_deref(), Some("R_RadialAttack_small.RadialAttack_small"));
        assert_eq!(weak[0].timeout, Some((0.5, 0.2)));
        // The strong one: a spawner whose object is a mesh alone, and the particles as a
        // `TriggerableParticle`.
        assert_eq!(strong.len(), 2);
        assert_eq!((strong[0].mesh.as_deref(), strong[0].timeout), (Some("R_RadialAttack_Optimus.RadialAttack_Optimus_a"), Some((0.7, 0.5))));
        assert!(strong[0].template.elements.is_empty() && !strong[0].attach);
        assert_eq!((strong[1].template.name, strong[1].attach), (crc32(b"r_attack_radial_optimus"), true));
        assert!(!weak[0].attach, "the data says the weak punch's effect stays where it was put");
        // Each spawner's object: one mesh the pack has, and a clip that scales it.
        for (e, ticks) in [(weak[0], 20.0), (strong[0], 70.0)] {
            let proxy = e.proxy.as_ref().unwrap();
            assert!(proxy.renders.iter().all(|r| data.library.meshes.contains_key(&crate::formats::hash::lower33(&r.model))), "{:?}", e.mesh);
            let clip = e.clip.as_ref().unwrap();
            assert_eq!(clip.length, ticks);
            let grown = |tick: f32| clip.channels.iter().find_map(|c| c.scale.sample(tick, clip.length, false)).unwrap();
            println!("{:?}: {} renders, scale {:?} -> {:?} -> {:?}", e.mesh, proxy.renders.len(), grown(0.0), grown(ticks / 2.0), grown(ticks));
        }
        for e in strong.iter().chain(&weak) {
            assert_eq!(data.robot.nodes[e.node].name, "Hand_R");
            assert!(e.upright, "{:08x}", e.template.name);
            assert_eq!(e.template.unread(), Vec::<String>::new());
            for element in &e.template.elements {
                let r = &element.renderer;
                let texture = data.library.textures.get(&r.texture).unwrap_or_else(|| panic!("{}", r.texture_path));
                let name = r.texture_path.rsplit('\\').next().unwrap();
                println!("{name}: {:?} {}x{}, BlendMode {}", texture.format, texture.width, texture.height, r.blend_mode);
            }
        }
        println!("strong: {:?}", strong.iter().map(|e| (e.template.name, e.template.elements.len(), e.mesh.clone())).collect::<Vec<_>>());
    }
}
