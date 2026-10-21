//! The scene side of a data object: an `Object` is a tree of named nodes with things attached
//! to them. Only what is needed to put the rendered parts in place is read here.

use super::hash::{crc32, lower33};
use super::anim::Clip;
use super::lxb::Node;
use crate::particles::Template;
use crate::skidmark::SkidMarkDef;
use crate::rumble::RumbleDef;

pub struct NodeDef {
    pub name: String,
    /// CRC-32 of the name: how animation channels address the node.
    pub name_hash: u32,
    /// Lower-case multiply-by-33 hash of the name: how skeleton bones address the node.
    pub bone_hash: u32,
    pub parent: Option<usize>,
    /// Rest transform in the object's own space: three axis rows and a position row.
    pub bind: [[f32; 3]; 4],
}

pub struct RenderDef {
    pub node: usize,
    /// Mesh name, such as `Bumblebee.Thigh_L`.
    pub model: String,
    pub lod_distances: Vec<f32>,
}

/// [data] RenderSetEnabler's named mesh groups. Exclusive groups start at set 0
/// [game 00803150] and a Trigger selects one group while hiding the others [008031e0].
#[derive(Clone, Debug)]
pub struct RenderSetGroup {
    pub exclusive: bool,
    pub sets: Vec<RenderSetDef>,
}

#[derive(Clone, Debug)]
pub struct RenderSetDef {
    pub name: u32,
    pub nodes: Vec<u32>,
}

impl RenderSetGroup {
    pub fn contains_model(&self, model: &str, set: usize) -> bool {
        // The render lookup alias is the CRC-32 of the model's local name, after its object
        // prefix. [data] every RenderSetEnabler link of all 18 playable packs (13 robots, 5
        // reskins), robot and vehicle objects, resolves to exactly one drawn model this way
        // (`core\tests\roster_mesh_sets.rs`). How the game builds the alias (the registry behind
        // 00803440, key 0x4cb8f1a6) is not read: an object outside the roster may differ.
        let alias = crc32(model.rsplit('.').next().unwrap_or(model).as_bytes());
        self.sets.get(set).is_some_and(|s| s.nodes.contains(&alias))
    }

    pub fn model_visible(&self, model: &str, selected: u32) -> bool {
        if !self.exclusive { return true; }
        let member = (0..self.sets.len()).any(|i| self.contains_model(model, i));
        !member || self.sets.iter().enumerate().any(|(i,s)| s.name == selected && self.contains_model(model, i))
    }
}

/// A collision primitive's shape, in its own space. [data]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Sphere { radius: f32 },
    /// `extent`: half the size along each axis.
    Cuboid { half: [f32; 3] },
    /// `height12` is half the height: Bumblebee's stands 2.89 up with 3, which is him from
    /// foot to head. [game 005b0180] The support function puts the caps at z = +-h, with h the
    /// packet's stored value read as it is (the wrapper 005baea0 copies no field), so the axis is
    /// the primitive's z and the stored value the half height.
    Cylinder { radius: f32, half_height: f32 },
}

/// One primitive of a `Collision` attachment: a body riding a node.
pub struct CollisionDef {
    /// The node it rides (`parent`); the body's number for the messages that name one body.
    pub node: usize,
    pub shape: Shape,
    /// Where it sits in its node's space: three axis rows and a position row. The loader
    /// (`005b9b33`) squares the axes up and keeps their lengths, so a scaled sphere is a
    /// bigger sphere: the hands' are 0.93 scaled by 1.97.
    pub matrix: [[f32; 3]; 4],
    /// CRC-32 of the mask preset's name (`Character`, `Attack`...), which says what it
    /// sends and takes (`bnxglobal.str#2`).
    pub mask_preset: u32,
    /// CRC-32 of its `material`: on a character the part a round struck (`Head`,
    /// `UpperBody`...), the body's `+0x254` that `00782b30` weighs by. [game] [data]
    pub material: u32,
}

/// An `Explosion` with the sphere of the object that carries it (loader `FUN_007e5640`).
/// [data]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExplosionDef {
    /// The sphere of the object's own `Collision`, with its matrix's scale in it. Its mask
    /// preset is `Explosion`: it takes `PUNCH`, so it hits what a fist hits.
    pub radius: f32,
    /// The share of that size it starts at, and the seconds it takes to reach all of it.
    pub start_scale: f32,
    pub time: f32,
    /// Seconds before it starts.
    pub delay: f32,
    pub damage: f32,
    /// The damage at the sphere's edge, and the share of the radius within which the damage
    /// is whole; a share outside 0..=1 means the damage is whole everywhere.
    pub min_damage: f32,
    pub fall_off: f32,
    /// `useGroundPoundDamageOverride`: the damage is the owner's `groundPoundDamage` or
    /// `groundPoundDamageWeak` instead.
    pub pound_override: bool,
    pub damage_player: bool,
    pub can_damage_self: bool,
    /// `damage_flags`: the type flags its hits carry (`melee::damage`).
    pub damage_flags: u32,
    /// `specialCaseDamageStuff`: the throw, speed and degrees above the ground.
    pub knock_back_speed: f32,
    pub knock_back_angle: f32,
    /// `cameraShakeOnExplode`: the CRC-32 of the name of the camera shake it starts where
    /// it is (`FUN_007e5d30`); 0 is none.
    pub camera_shake: u32,
    /// `specialCaseDamageStuff.stunTime`: seconds what it hits is stunned for (his special's
    /// shockwave: 2.5, with the flag `DAMAGE_STUN` and no damage).
    pub stun_time: f32,
    /// `specialCaseDamageStuff.slowTime` and `dotTime` (seconds) and `heatIncrement`: the
    /// loader `FUN_00744f10` reads them in that order after the stun (`+0x50`, `+0x54`,
    /// `+0x58` of the object's special case block). [game 00744f10] [data] Starscream's micro
    /// missile blast has `slowTime` 5 (with `DAMAGE_SLOW` in its flags).
    pub slow_time: f32,
    pub dot_time: f32,
    pub heat: f32,
}

/// An `ObjectSpawnerOnBone`: a trigger message with its `LuxScriptName` makes it put its
/// object at its node (`FUN_007fc270`). Only spawners of explosions are kept.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnerDef {
    pub node: usize,
    pub script_name: u32,
    /// `attachToBone`: the spawned object is tied to the node.
    pub attach: bool,
    pub explosion: ExplosionDef,
    /// The spawned object's `ControllerRumble`, which its `TriggerOnSpawn` starts.
    pub rumble: Option<RumbleDef>,
    pub ssd: Vec<crate::ssd::Info>,
}

/// A particle effect on a node that a trigger message with its `LuxScriptName` starts: an
/// `EffectSpawner` (loader `FUN_007fb2b0`, trigger `FUN_007fb470`), which puts a whole
/// little object there (`effectProxy`: particles, often a mesh and its clip, and a
/// `TimeoutDeleter`), or a `TriggerableParticle`, which is the particles alone.
pub struct EffectDef {
    pub node: usize,
    pub script_name: u32,
    pub template: Template,
    /// An `EffectSpawner`'s switches, each on when the data does not give it: the effect
    /// stands upright (`removeTilt`), without the node's facing or scale (`removeFacing`,
    /// `removeScale`), and follows the node's place while it lives (`attachToBone`).
    /// [game] His two punches give them all: upright, and NOT following. [data]
    /// `useRootFacing` is not ported: with the facing removed it changes nothing for
    /// effects that look the same from every side. A `TriggerableParticle` has `useRot`
    /// and `useScale` (both off on his: upright); it is a thing on the node. Its constructor
    /// (`00746e40`) spawns the emitter attached to its owner (`0057e820`, `particle_emitter_spawn_attached`)
    /// [game 00746e40]. Whether the attached emitter's turn follows the node per frame: the
    /// spawn (0057e820 / 0057b940) is read, and the emitter's message 5 (0057dca0) calls 0057d760,
    /// which reads the owner node's current matrix and normalises its axes for flag 8 [game 0057dca0,
    /// 0057d760, 0057f480]. The sender of message 5 is trigger code 5, which `TriggerableParticle_hit`
    /// (00746dd0, l.8-13) sends to its object after moving that object to the hit (00545900). Its callers
    /// read: the flame's per-frame ray hit (0074c200, l.241-270) and the collision contact update (007d8340,
    /// whose own caller is not read) [game]. For an emitter those hits drive, it follows its owner's LAST
    /// HIT place and refreshes only on frames with a hit. [game 007aac80: the muzzle's spawner is made with flags 0x3a, which include 0x20, the owner-follow registration (ledger note on 0057dd30), so it follows its node each frame without message 5] [assumed: the same holds for the punches' spawners and every other attached effect (effects.rs Anchor::Node); the punches' spawner path is not read] settles: the punches' spawner (its 0057e820 call and flags) for a robot's node.
    pub upright: bool,
    pub attach: bool,
    /// The proxy's mesh and its `TimeoutDeleter` (seconds until it is deleted, and when
    /// its fade starts).
    pub mesh: Option<String>,
    pub timeout: Option<(f32, f32)>,
    /// The proxy object itself, and the clip of its `AnimBundle` (the punches': a mesh
    /// that grows).
    pub proxy: Option<Box<ObjectDef>>,
    pub clip: Option<Clip>,
    /// A `TriggerableParticle`: ONE emitter of its own, which a Trigger starts only while it
    /// has no live system (`triggerable_particle_trigger` 00746f80 -> `particle_emitter_trigger`
    /// 0057dd30 spawns only when the handle at `+0x28` is empty). [game] An `EffectSpawner`
    /// spawns a new object each time.
    pub triggerable: bool,
}

/// A plain `ParticleEmitter` attachment of an object (not an `EffectSpawner`): it starts with the
/// object, by its own flags. Loader `placed_particle_emitter_factory` 0057d0d0 [game]: `autoStart`
/// defaults on (flag 2: triggered at once), `useRot` on (flag 8: the node's axes normalised, 0057d760),
/// `useScale` off, `deleteWhenDone` (flag 0x20), `instantKill` (flag 0x80: the owner's stop removes
/// the particles too; clear, the stop 0057e000 only stops emission), and a `spawnChance` gate on
/// fract(object y + z). Kept apart from `ObjectDef::effects`: it answers to no message. [data]
#[derive(Clone, Debug)]
pub struct PlacedEmitterDef {
    /// The node it rides (the attachment's `index`), and that node's bind in the object's space.
    pub node: usize,
    pub bind: [[f32; 3]; 4],
    pub template: Template,
    pub auto_start: bool,
    pub use_rot: bool,
    pub use_scale: bool,
    pub delete_when_done: bool,
    pub instant_kill: bool,
    pub spawn_chance: f32,
}

/// The `ParticleEmitter`s among an object's own attachments (not those of the objects its links
/// hold), with the defaults of 0057d0d0. [game] [data]
pub fn read_placed_emitters(object: Node) -> Vec<PlacedEmitterDef> {
    let mut out = Vec::new();
    for attachment in object.get("attachments").into_iter().flat_map(Node::items) {
        let Some(thing) = attachment.get("thing").filter(|t| t.type_name() == Some("ParticleEmitter")) else { continue };
        let Some(template) = thing.get("id").and_then(Template::read) else { continue };
        let flag = |name: &str, default: bool| thing.get(name).map_or(default, |n| n.int().map_or_else(|| n.is_true(), |v| v != 0));
        let index = attachment.get("index").and_then(Node::int).unwrap_or(0);
        out.push(PlacedEmitterDef {
            node: index.max(0) as usize,
            bind: {
                let f = crate::level_render::flat32(&crate::level_render::bind_of(object, index));
                std::array::from_fn(|r| std::array::from_fn(|c| f[r * 3 + c]))
            },
            template,
            auto_start: flag("autoStart", true),
            use_rot: flag("useRot", true),
            use_scale: flag("useScale", false),
            delete_when_done: flag("deleteWhenDone", false),
            instant_kill: flag("instantKill", false),
            spawn_chance: thing.get("spawnChance").and_then(Node::float).unwrap_or(1.0),
        });
    }
    out
}

/// The `Light`s among an object's own attachments, each with its node and placed by that node's
/// bind (object space), as the level walk places a Light (`point_light`). [data]
pub fn read_lights(object: Node) -> Vec<(usize, crate::point_light::LevelLight)> {
    let mut out = Vec::new();
    for attachment in object.get("attachments").into_iter().flat_map(Node::items) {
        let Some(thing) = attachment.get("thing").filter(|t| t.type_name() == Some("Light")) else { continue };
        let index = attachment.get("index").and_then(Node::int).unwrap_or(0);
        if let Some(light) = crate::point_light::read_light_at(thing, &crate::level_render::bind_of(object, index)) {
            out.push((index.max(0) as usize, light));
        }
    }
    out
}

pub struct ObjectDef {
    /// Its plain `ParticleEmitter` attachments (`read_placed_emitters`) and its `Light`s
    /// (`read_lights`, object space).
    pub emitters: Vec<PlacedEmitterDef>,
    pub lights: Vec<(usize, crate::point_light::LevelLight)>,
    /// ObjectSpawnerOnBone sounds, including spawners whose object has no Explosion.
    /// Node index, trigger script, anonymous event IDs. [data/game: 0076dcf0]
    pub spawn_sound_players: Vec<(usize, u32, Vec<u32>)>,
    pub nodes: Vec<NodeDef>,
    pub renders: Vec<RenderDef>,
    pub render_sets: Vec<RenderSetGroup>,
    pub collisions: Vec<CollisionDef>,
    pub spawners: Vec<SpawnerDef>,
    pub effects: Vec<EffectDef>,
    /// Its `TireSkidMark`s (loader `FUN_0080aa10`): one on each tyre's node of a car. [data]
    pub skid_marks: Vec<SkidMarkDef>,
    /// Its `ClimbStep`s: the node each is on and its `side` (the name a `ClimbStepEvent`
    /// calls it by; `anim::CLIMB_SIDES`). [data]
    pub climb_steps: Vec<(usize, u32)>,
    /// Its `SoundPlayer`s that wait for a trigger: the `LuxScriptName` each answers to and
    /// the sound event it plays (`id`). [data]
    pub sound_players: Vec<SoundPlayerDef>,
    /// The nodes its `SpineIK` links (Bumblebee: `Spine_02`, `Spine_03`), which share the
    /// twist toward the aim in weapon mode.
    pub spine_links: Vec<usize>,
    pub ribbons: Vec<crate::ribbon::Def>,
    pub collision_fx: Vec<crate::collisionfx::Def>,
    pub collision_sounds: Vec<crate::collisionfx::SoundDef>,
    pub footsteps: Vec<FootstepDef>,
}

#[derive(Clone, Copy, Debug)]
pub struct FootstepDef { pub node: usize, pub side: u32, pub height: f32 }

#[derive(Clone, Debug, PartialEq)]
pub struct SoundPlayerDef {
    /// [data/game: 0076e100] The attachment's object/skeleton transform index.
    pub node: usize,
    pub script: u32,
    pub id: u32,
    pub wait_for_trigger: bool,
    pub once: bool,
    pub retrigger: bool,
    /// `keyOffOnUnTrigger`: an Untrigger keys the event off (into its tail) rather than
    /// stopping it with its fade. [game 0076e450]
    pub key_off_on_untrigger: bool,
}

/// [game: 0076d490] idtext takes precedence over id; trigger/once/retrigger are
/// component flags. Accept anonymous players as well as named trigger recipients.
fn sound_player(thing: Node) -> Option<SoundPlayerDef> {
    if thing.type_name() != Some("SoundPlayer") { return None; }
    let id = |name: &str| thing.get(name).and_then(Node::int).unwrap_or(0) as u32;
    let event = if id("idtext") != 0 { id("idtext") } else { id("id") };
    (event != 0).then(|| SoundPlayerDef { node: 0, script: id("LuxScriptName"), id: event,
        wait_for_trigger: id("waitForTrigger") != 0, once: id("playOnce") != 0,
        retrigger: id("allowRetrigger") != 0, key_off_on_untrigger: id("keyOffOnUnTrigger") != 0 })
}

/// [game: 0076dcf0] Players whose waitForTrigger is false start in object init.
pub fn read_spawn_sounds(object: Node) -> Vec<u32> {
    object.get("attachments").into_iter().flat_map(Node::items)
        .filter_map(|a| a.get("thing")).filter_map(sound_player)
        .filter(|p| !p.wait_for_trigger).map(|p| p.id).collect()
}

/// [data] A projectile's DamageLink owns objects released at destruction. Read
/// every detached sibling, including the anonymous SoundPlayer beside its Explosion.
pub fn read_projectile_end_sounds(object: Node) -> Vec<u32> {
    fn released(object: Node) -> Vec<u32> {
        let mut sounds = read_spawn_sounds(object);
        for link in object.get("attachments").into_iter().flat_map(Node::items)
            .filter_map(|a| a.get("thing"))
            .filter(|t| matches!(t.type_name(), Some("DamageLink" | "DetachLink")))
        {
            if let Some(child) = link.get("thing") { sounds.extend(released(child)); }
        }
        sounds
    }
    object.get("attachments").into_iter().flat_map(Node::items)
        .filter_map(|a| a.get("thing")).filter(|t| t.type_name() == Some("DamageLink"))
        .filter_map(|t| t.get("thing")).flat_map(released).collect()
}

const IDENTITY: [[f32; 3]; 4] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0]];

fn matrix(node: Node) -> Option<[[f32; 3]; 4]> {
    let v = node.floats();
    (v.len() == 12).then(|| std::array::from_fn(|row| std::array::from_fn(|col| v[row * 3 + col])))
}

/// Reads an `Object`. `name_of` turns a name hash back into text.
pub fn read_object<'a>(object: Node<'a>, name_of: impl Fn(u32) -> Option<&'a str>) -> Option<ObjectDef> {
    read(object, &name_of)
}

/// The same, callable from itself: an effect spawner's object is read the same way.
fn read<'a>(object: Node<'a>, name_of: &dyn Fn(u32) -> Option<&'a str>) -> Option<ObjectDef> {
    let sub_matrices = object.get("sub_matrices")?;
    let attachments = object.get("attachments")?;

    // The first node is the object itself; `sub_matrices` starts at the second.
    let mut nodes = Vec::new();
    match object.get("hierarchy").filter(|h| h.get("parents").is_some()) {
        Some(hierarchy) => {
            let parents = hierarchy.get("parents")?.ints();
            let names = hierarchy.get("names")?.ints();
            // Stored only by objects that have a skinned mesh; it is the same hash either way.
            let bone_names = hierarchy.get("tl_names").map(Node::ints).unwrap_or_default();
            for (i, &parent) in parents.iter().enumerate() {
                let name_hash = *names.get(i)? as u32;
                let bind = match i {
                    0 => IDENTITY,
                    _ => sub_matrices.at(i - 1).and_then(matrix).unwrap_or(IDENTITY),
                };
                let name = name_of(name_hash).map_or_else(|| format!("#{name_hash:08x}"), str::to_owned);
                nodes.push(NodeDef {
                    bone_hash: bone_names.get(i).map_or_else(|| lower33(&name), |&h| h as u32),
                    name,
                    name_hash,
                    parent: usize::try_from(parent).ok(),
                    bind,
                });
            }
        }
        None => nodes.push(NodeDef { name: "Root".into(), name_hash: 0, bone_hash: 0, parent: None, bind: IDENTITY }),
    }

    let mut renders = Vec::new();
    let mut collisions = Vec::new();
    let mut spawners = Vec::new();
    let mut effects = Vec::new();
    let mut skid_marks = Vec::new();
    let mut climb_steps = Vec::new();
    let mut spine_links = Vec::new();
    let mut sound_players = Vec::new();
    let mut ribbons = Vec::new();
    let mut collision_fx=Vec::new();
    let mut collision_sounds=Vec::new();
    let mut footsteps=Vec::new();
    let mut spawn_sound_players = Vec::new();
    let mut render_sets = Vec::new();
    for attachment in attachments.items() {
        let Some(thing) = attachment.get("thing") else { continue };
        if thing.type_name() == Some("RenderSetEnabler") {
            render_sets.push(RenderSetGroup {
                // [game 00802cf0] Missing exclusive defaults true.
                exclusive: thing.get("exclusive").and_then(Node::int).is_none_or(|v| v != 0),
                sets: thing.get("sets").into_iter().flat_map(Node::items).map(|set| RenderSetDef {
                    name: set.get("name").and_then(Node::int).unwrap_or(0) as u32,
                    nodes: set.get("nodes").into_iter().flat_map(Node::items).filter_map(Node::label).collect(),
                }).collect(),
            });
            continue;
        }
        if thing.type_name() == Some("FootStep") {
            if let (Some(node), Some(side), Some(height)) = (
                attachment.get("index").and_then(Node::int),
                thing.get("side").and_then(|n| n.enum_hash().or_else(|| n.int().map(|v| v as u32))),
                thing.get("heightFromGround").and_then(Node::float),
            ) { footsteps.push(FootstepDef { node: node as usize, side, height }); }
            continue;
        }
        if thing.type_name() == Some("SoundPhysicsReactor") {
            collision_sounds.push(crate::collisionfx::SoundDef::read(thing));
            continue;
        }
        if thing.type_name()==Some("CollisionFX") {
            collision_fx.push(crate::collisionfx::Def::read(thing));continue;
        }
        if thing.type_name() == Some("RibbonTrail") {
            ribbons.extend(crate::ribbon::read(thing));
            continue;
        }
        if thing.type_name() == Some("SoundPlayer") {
            if let Some(mut player) = sound_player(thing) {
                player.node = attachment.get("index").and_then(Node::int).unwrap_or(0) as usize;
                sound_players.push(player);
            }
            continue;
        }
        if thing.type_name() == Some("TireSkidMark") {
            let node = attachment.get("index").and_then(Node::int).unwrap_or(0) as usize;
            let default = SkidMarkDef::default();
            let num = |name: &str, or: f32| thing.get(name).and_then(Node::float).unwrap_or(or);
            let colour = thing.get("skidColor").map(Node::ints).filter(|c| c.len() == 4);
            skid_marks.push(SkidMarkDef {
                node: node.min(nodes.len() - 1),
                tyre: thing.get("tireName").and_then(Node::int).unwrap_or(0) as u32,
                point_length: num("skidPointLength", default.point_length),
                point_width: num("skidPointWidth", default.point_width),
                lifetime: num("skidPointLifetime", default.lifetime),
                colour: colour.map_or(default.colour, |c| std::array::from_fn(|i| c[i] as f32 / 255.0)),
                material: thing.get("material").and_then(Node::asset_text).map_or(0, |(id, _)| id),
            });
            continue;
        }
        if thing.type_name() == Some("ClimbStep") {
            let node = attachment.get("index").and_then(Node::int).unwrap_or(0) as usize;
            if let Some(side) = thing.get("side").and_then(|side| side.enum_hash().or_else(|| side.int().map(|v| v as u32))) {
                climb_steps.push((node.min(nodes.len() - 1), side));
            }
            continue;
        }
        if thing.type_name() == Some("SpineIK") {
            // Links by name: each is the hash of a node's name.
            let linked = thing.get("links").into_iter().flat_map(Node::items).filter_map(Node::label);
            spine_links.extend(linked.filter_map(|hash| nodes.iter().position(|n| n.name_hash == hash)));
            continue;
        }
        if thing.type_name() == Some("VehicleHeadlight") {
            // `Trigger` spawns its `spawnObject` (a particle emitter: the lamp's glow) and
            // ties it to the node `linkName` names (`FUN_00815790`). [game] Kept as an
            // effect on that node, with the node's turn. [game 00815790, 00711f40 -> 0056c9b0: the tie is a
            // `lux::LinkConstraint` on the node with an identity offset, so the object takes the node's own axes
            // (`notes\weapon-mode.md` "How it sits on the node": offset * node * owner, squared up)]
            let index = attachment.get("index").and_then(Node::int).unwrap_or(0) as usize;
            let link = thing.get("linkName").and_then(Node::int).map(|hash| hash as u32);
            let node = link.and_then(|hash| nodes.iter().position(|n| n.name_hash == hash)).unwrap_or(index);
            let emitter = thing
                .path(&["spawnObject", "obj", "attachments"])
                .into_iter()
                .flat_map(Node::items)
                .filter_map(|a| a.get("thing"))
                .find(|t| t.type_name() == Some("ParticleEmitter"));
            if let Some(template) = emitter.and_then(|e| e.get("id")).and_then(Template::read) {
                effects.push(EffectDef {
                    node: node.min(nodes.len() - 1),
                    script_name: thing.get("LuxScriptName").and_then(Node::int).unwrap_or(0) as u32,
                    template,
                    upright: false,
                    attach: true,
                    mesh: None,
                    timeout: None,
                    proxy: None,
                    clip: None,
                    triggerable: false,
                });
            }
            continue;
        }
        if matches!(thing.type_name(), Some("EffectSpawner" | "TriggerableParticle")) {
            let node = attachment.get("index").and_then(Node::int).unwrap_or(0) as usize;
            let on = |name: &str| thing.get(name).and_then(Node::int).is_none_or(|v| v != 0);
            let spawner = thing.type_name() == Some("EffectSpawner");
            let proxy = || thing.path(&["effectProxy", "obj", "attachments"]).into_iter().flat_map(Node::items).filter_map(|a| a.get("thing"));
            let of_type = |name: &str| proxy().find(|t| t.type_name() == Some(name));
            let template = if spawner { of_type("ParticleEmitter").and_then(|e| e.get("id")) } else { thing.get("id") };
            // A spawner whose object has no particles (the strong punch's: a mesh alone)
            // is kept with an empty template, for its mesh.
            let template = template.and_then(Template::read).or_else(|| spawner.then(Template::default));
            if let Some(template) = template {
                effects.push(EffectDef {
                    node: node.min(nodes.len() - 1),
                    script_name: thing.get("LuxScriptName").and_then(Node::int).unwrap_or(0) as u32,
                    template,
                    // [game: 00746fc0] useRot defaults on and uses its numeric value.
                    upright: if spawner { on("removeTilt") } else { !on("useRot") },
                    attach: !spawner || on("attachToBone"),
                    mesh: of_type("RenderThing")
                        .filter(|_| spawner)
                        .and_then(|r| r.get("meshes"))
                        .and_then(|m| m.at(0))
                        .and_then(|m| m.get("model"))
                        .and_then(Node::text),
                    timeout: of_type("TimeoutDeleter").filter(|_| spawner).map(|t| {
                        let num = |name: &str| t.get(name).and_then(Node::float).unwrap_or(0.0);
                        (num("time"), num("fadeStart"))
                    }),
                    proxy: thing.path(&["effectProxy", "obj"]).filter(|_| spawner).and_then(|o| read(o, name_of)).map(Box::new),
                    clip: of_type("AnimBundle")
                        .filter(|_| spawner)
                        .and_then(|b| b.get("records"))
                        .and_then(|r| r.at(0))
                        .and_then(|r| r.get("anim"))
                        .and_then(|a| super::anim::read_clip(a.file(), a)),
                    triggerable: !spawner,
                });
            }
            continue;
        }
        if thing.type_name() == Some("ObjectSpawnerOnBone") {
            let node = attachment.get("index").and_then(Node::int).unwrap_or(0) as usize;
            let spawned = thing.path(&["spawnObject", "obj"]);
            let sounds = spawned.map(read_spawn_sounds).unwrap_or_default();
            if !sounds.is_empty() {
                spawn_sound_players.push((node.min(nodes.len() - 1), thing.get("LuxScriptName").and_then(Node::int).unwrap_or(0) as u32, sounds));
            }
            if let Some(explosion) = spawned.and_then(read_explosion) {
                spawners.push(SpawnerDef {
                    ssd: spawned.map(read_explosion_ssd).unwrap_or_default(),
                    rumble: spawned.and_then(read_rumble),
                    node: node.min(nodes.len() - 1),
                    script_name: thing.get("LuxScriptName").and_then(Node::int).unwrap_or(0) as u32,
                    // The loader's default when the field is absent is on (`FUN_007fc140`).
                    attach: thing.get("attachToBone").and_then(Node::int).is_none_or(|v| v != 0),
                    explosion,
                });
            }
            continue;
        }
        if thing.type_name() == Some("Collision") {
            let node = thing.get("parent").and_then(Node::int).unwrap_or(0) as usize;
            for primitive in thing.get("primitives").into_iter().flat_map(Node::items) {
                let Some(packet) = primitive.get("packet") else { continue };
                let number = |name: &str| packet.get(name).and_then(Node::float).unwrap_or(0.0);
                let shape = match packet.type_name() {
                    Some("CollisionSpherePacket") => Shape::Sphere { radius: number("radius") },
                    Some("CollisionCuboidPacket") => {
                        let extent = packet.get("extent").map(Node::floats).unwrap_or_default();
                        Shape::Cuboid { half: std::array::from_fn(|i| extent.get(i).copied().unwrap_or(0.0)) }
                    }
                    Some("CollisionCylinderPacket") => Shape::Cylinder { radius: number("radius"), half_height: number("height12") },
                    // Meshes, hulls, cones and capsules: none on a character.
                    _ => continue,
                };
                collisions.push(CollisionDef {
                    node: node.min(nodes.len() - 1),
                    shape,
                    matrix: primitive.get("matrix").and_then(matrix).unwrap_or(IDENTITY),
                    mask_preset: primitive.get("maskPreset").and_then(Node::int).unwrap_or(0) as u32,
                    material: primitive.get("material").and_then(Node::int).unwrap_or(0) as u32,
                });
            }
            continue;
        }
        if thing.type_name() != Some("RenderThing") {
            continue;
        }
        let node = attachment.get("index").and_then(Node::int).unwrap_or(0) as usize;
        for mesh in thing.get("meshes").into_iter().flat_map(Node::items) {
            let Some(model) = mesh.get("model").and_then(Node::text) else { continue };
            let lod_distances = mesh.get("lod_distances").map(Node::floats).unwrap_or_default();
            renders.push(RenderDef { node: node.min(nodes.len() - 1), model, lod_distances });
        }
    }
    Some(ObjectDef { emitters: read_placed_emitters(object), lights: read_lights(object), nodes, renders, render_sets, collisions, spawners, effects, skid_marks, climb_steps, sound_players, spine_links, ribbons, collision_fx, collision_sounds, footsteps, spawn_sound_players })
}

/// The `Explosion` among an object's attachments and the sphere of its `Collision`.
fn read_explosion(object: Node) -> Option<ExplosionDef> {
    let things = || object.get("attachments").into_iter().flat_map(Node::items).filter_map(|a| a.get("thing"));
    let explosion = things().find(|t| t.type_name() == Some("Explosion"))?;
    let radius = things()
        .filter(|t| t.type_name() == Some("Collision"))
        .flat_map(|t| t.get("primitives").into_iter().flat_map(Node::items))
        .find_map(|primitive| {
            let packet = primitive.get("packet").filter(|p| p.type_name() == Some("CollisionSpherePacket"))?;
            let rows = primitive.get("matrix").and_then(matrix).unwrap_or(IDENTITY);
            let scale = rows[0].iter().map(|v| v * v).sum::<f32>().sqrt();
            Some(packet.get("radius").and_then(Node::float)? * scale)
        })?;
    let number = |name: &str| explosion.get(name).and_then(Node::float).unwrap_or(0.0);
    let flag = |name: &str| explosion.get(name).and_then(Node::int).is_some_and(|v| v != 0);
    let special = |name: &str| explosion.path(&["specialCaseDamageStuff", name]).and_then(Node::float).unwrap_or(0.0);
    Some(ExplosionDef {
        radius,
        start_scale: number("start_scale"),
        time: number("explosion_time"),
        delay: number("delay"),
        damage: number("damage"),
        min_damage: number("minDamage"),
        fall_off: number("min_radius_fall_off_damage"),
        pound_override: flag("useGroundPoundDamageOverride"),
        damage_player: flag("damage_player"),
        can_damage_self: flag("can_damage_self"),
        damage_flags: explosion.get("damage_flags").and_then(Node::int).unwrap_or(0) as u32,
        knock_back_speed: special("knockBackSpeed"),
        knock_back_angle: special("knockBackAngle"),
        camera_shake: explosion.get("cameraShakeOnExplode").and_then(Node::int).unwrap_or(0) as u32,
        stun_time: special("stunTime"),
        slow_time: special("slowTime"),
        dot_time: special("dotTime"),
        heat: special("heatIncrement"),
    })
}

/// The explosion a projectile's object lets go of when it is destroyed: the `Explosion`
/// and its sphere sit on an object a `DetachLink` holds, under the projectile's `DamageLink`
/// (his micro missile: damage 8, sphere 5, start scale 0.5 over 0.1 s, fall-off from 0.75,
/// `minDamage` 1, throw 30 at 15 degrees, flags `DAMAGE_BY_NPC | DAMAGE_BY_EXPLOSION`).
/// [data] That a destroyed projectile releases its detach objects is how the data is built
/// (`DamageData.destroy_type` and the lifetime's own kill, `FUN_00785c00`); the release
/// itself is not read.
pub fn read_projectile_explosion(object: Node) -> Option<ExplosionDef> {
    if let Some(found) = read_explosion(object) {
        return Some(found);
    }
    object
        .get("attachments")
        .into_iter()
        .flat_map(Node::items)
        .filter_map(|a| a.get("thing"))
        .filter(|t| matches!(t.type_name(), Some("DamageLink" | "DetachLink")))
        .find_map(|link| link.get("thing").and_then(read_projectile_explosion))
}

/// The surface preset of a plain explosion or of a projectile's detached explosion.
pub fn read_explosion_ssd(object: Node) -> Vec<crate::ssd::Info> {
    let things: Vec<_> = object.get("attachments").into_iter().flat_map(Node::items).filter_map(|a| a.get("thing")).collect();
    if let Some(explosion) = things.iter().find(|t| t.type_name() == Some("Explosion")) {
        return crate::ssd::read(explosion.get("SSDImpactPreset"));
    }
    things.iter().filter(|t| matches!(t.type_name(),Some("DamageLink" | "DetachLink")))
        .filter_map(|link| link.get("thing")).map(read_explosion_ssd).find(|infos| !infos.is_empty()).unwrap_or_default()
}

/// The `ControllerRumble` among an object's attachments (loader `FUN_007dbaa0`). In his
/// two blasts a `TriggerOnSpawn` sends it its trigger the moment the object is there, and
/// `waitForTrigger` and `playOnce` change nothing about that one start. [data]
pub(crate) fn read_rumble(object: Node) -> Option<RumbleDef> {
    let rumble = object
        .get("attachments")
        .into_iter()
        .flat_map(Node::items)
        .filter_map(|a| a.get("thing"))
        .find(|t| t.type_name() == Some("ControllerRumble"))?;
    let number = |name: &str| rumble.get(name).and_then(Node::float).unwrap_or(0.0);
    let int = |name: &str| rumble.get(name).and_then(Node::int).unwrap_or(0) as u32;
    Some(RumbleDef {
        kind: int("rumbleType"),
        duration: number("duration"),
        // A whole number of hundredths in the data.
        strength: number("strength").trunc() * 0.01,
        scale_type: int("scaleType"),
        distance_near: number("distanceNear"),
        distance_far: number("distanceFar"),
    })
}
