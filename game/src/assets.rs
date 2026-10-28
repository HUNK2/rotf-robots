//! Turns loaded game data into Bevy assets and entities.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};

use crate::formats::hash::{crc32, lower33};
use crate::formats::model::{self, Library};
use tf2_core::collision_world::Owner;
use crate::formats::scene::ObjectDef;
use crate::formats::texture::{PixelFormat, Texture};
use crate::fx::Glow;
use crate::sky::Sky;
use crate::ocean::Ocean;
use crate::surface::{self, Distortion, Surface};

const BLEND_NORMAL: u32 = crc32(b"Normal");
const BLEND_ADDITIVE: u32 = crc32(b"Additive");
/// [game 011790f0, 005a2af0] The unnamed cut-out (LS-22): blend off, a threshold of
/// max(alpharef,1)/255 on the alpha, output alpha 1 (`AlphaMode::Mask`).
const BLEND_CUTOUT: u32 = 0xbc07_610c;
/// [game 011790f0, 005a3033-005a30c7] The unnamed 0x3467929c (LS-22): Normal blend, with the
/// alpha times AlphaBoost = 255 / max(X,1), X the word 0x0d7779d1 (default 254).
const BLEND_BOOST: u32 = 0x3467_929c;
/// [game 011790f0] The word of AlphaBoost's source, unnamed (LS-21 section 2).
const WORD_BOOST_SOURCE: u32 = 0x0d77_79d1;

/// The drawn parts of one level Instance (`spawn_placed`): the entity holding them, by the Instance's key. Hidden with
/// `Visibility::Hidden` when the Instance is destroyed (`mission.rs`).
#[derive(Component, Clone, Copy, Debug)]
pub struct LevelInstance {
    #[allow(dead_code)] // read by the mission code, which this copy leaves out
    pub owner: Owner,
}

/// What `spawn_placed` made: the level root, the names with no mesh, the part entities, and the Instance entities.
pub struct PlacedSpawn {
    #[allow(dead_code)] // as LevelInstance::owner
    pub root: Entity,
    pub missing: usize,
    pub parts: usize,
    pub instance_parts: usize,
    pub instances: usize,
    /// One root per requested placement, preserving order; missing meshes have no root.
    pub models: Vec<Option<Entity>>,
}

/// The entities of one spawned object, by node index.
#[derive(Component)]
pub struct ObjectNodes {
    /// Each node's place, which its child nodes hang from.
    pub entities: Vec<Entity>,
    /// Each node's own frame: a child of the node carrying the clip's own scale for the node
    /// (`Channel::own_scale`), which the node's skin, its rigid parts and what is hung on it take
    /// and its child nodes do not. [game: the pose apply 00544e00 sizes a node by both scales and
    /// composes each child with diag(1/parent's own scale) first; a weapon hangs on the node's
    /// matrix, 007b4f20] Identity where no clip gives one. Empty for objects not built by
    /// `Builder::spawn`.
    pub frames: Vec<Entity>,
    pub names: Vec<String>,
    /// CRC-32 of each node's name, which is how animation channels address nodes.
    pub name_hashes: Vec<u32>,
    /// Each node's rest transform relative to its parent.
    pub rest: Vec<Transform>,
    pub parents: Vec<Option<usize>>,
    /// The nodes the object's `SpineIK` links.
    pub spine_links: Vec<usize>,
    pub foot_rig: Option<tf2_core::footik::FootRig>,
}

impl ObjectNodes {
    /// What hangs on node `node` (a weapon, a part) hangs here: its own frame, or the node
    /// itself for an object without frames.
    pub fn frame(&self, node: usize) -> Entity {
        self.frames.get(node).copied().unwrap_or(self.entities[node])
    }
}

/// The game's space is right-handed with z up and y forward; Bevy's has y up and -z forward.
pub fn game_to_bevy(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, -v.y)
}

pub fn bevy_to_game(v: Vec3) -> Vec3 {
    Vec3::new(v.x, -v.z, v.y)
}

/// Rotation that takes game-space model data into Bevy's space.
pub fn model_space_rotation() -> Quat {
    Quat::from_rotation_x(-FRAC_PI_2)
}

/// The file stores matrices for row vectors: three axis rows and a position row.
fn matrix(rows: &[[f32; 3]; 4]) -> Mat4 {
    Mat4::from_cols(
        Vec3::from(rows[0]).extend(0.0),
        Vec3::from(rows[1]).extend(0.0),
        Vec3::from(rows[2]).extend(0.0),
        Vec3::from(rows[3]).extend(1.0),
    )
}

/// A material's blend mode, on all three paths (Surface, Glow, StandardMaterial). Named: Opaque
/// 0x0f2773b0, Normal 0x26296dd2, Additive 0x1f4da15f [data] (the unpublished notes
/// section 1), Background 0xf335b780 (opaque, no threshold [game 011790f0]; named here only in
/// the comment). Unnamed, folded onto the mode they set the same state as [game] (blend switch
/// 011790f0, section 2.1): 0xd6300862 (236 pairs, mostly `bgnd_vc`) as `Normal`, 0xcdacd688
/// (18 pairs) as `Additive`.
/// LS-22: 0xbc07610c (968 pairs; 1,508 Shanghai parts: trees,
/// leaves, decals, fences) is `Mask` (`alpha_mode_of`); 0x3467929c (845 pairs) is `Normal` with
/// AlphaBoost (`alpha_boost`). In the game's blend switch, pass kinds 2 and 3 would take a cut branch
/// for it (blend off, flag 1, alpha clamped to 0xfe), other kinds Normal [game 011790f0,
/// 011791a4-011791dc; wave 27 O10]. The kind is the switch's second stack argument (EBP, loaded at
/// 01179100), and its one caller, 005a2af0 at 005a3388 (through the jmp 005a2880), passes its local
/// [esp+0x44], zeroed at 005a329a just before: kind 0, so Normal, as the port draws it. [game, read
/// 2026-10-20] One robot material has this blend (Ironhide's, `work\mat_dump.py`).
/// 0x411feefe (5 pairs, "Subtractive" [data], the game blends SRCALPHA/ONE with op 3 [game 011790f2]) stays
/// Opaque here [stand-in] (level-only: no robot material has 0x411feefe, see below): no subtract blend is ported yet. The op number is settled [game 005a442a]: the
/// render-state write pushes the raw [+0xc] word as the value of SetRenderState(171 = D3DRS_BLENDOP), with no
/// table between, so 3 is D3DBLENDOP_REVSUBTRACT (the D3D9 enum: 1 ADD, 2 SUBTRACT, 3 REVSUBTRACT). Still
/// stand-in: a Bevy ReverseSubtract material for its 5 pairs is not ported (the unpublished notes item 7).
/// [data 2026-10-20, robots] No `characters\*.str` material has 0x411feefe (scratch count), so the robots never
/// reach this stand-in; the levels keep it.
fn blend_of(source: &model::Material) -> Option<u32> {
    source.word("blendmode").map(|blend| match blend {
        0xd630_0862 => BLEND_NORMAL,
        0xcdac_d688 => BLEND_ADDITIVE,
        other => other,
    })
}

/// The alpha mode of a material's blend, on the Surface and StandardMaterial paths (the Glow
/// path only takes the blends that are not a cut-out, see `Builder::material`).
fn alpha_mode_of(source: &model::Material) -> AlphaMode {
    match blend_of(source) {
        Some(BLEND_ADDITIVE) => AlphaMode::Add,
        Some(BLEND_NORMAL) | Some(BLEND_BOOST) => AlphaMode::Blend,
        Some(BLEND_CUTOUT) => AlphaMode::Mask(cutout_threshold(source)),
        _ => AlphaMode::Opaque,
    }
}

/// The cut-out threshold of blendmode 0xbc07610c [game 005a30ef]: max(alpharef,1) / 255, where
/// alpharef is the raw word of the material [data: word, 91% of rows are 1]. The compare the port uses is the texkill
/// of GBufferAlphaTest [data, shader listings, the unpublished notes LS-17 Q2: kill below AlphaThreshold]. [assumed]
/// that this variant is the one drawn for 0xbc07610c. [data 2026-10-20, robots] Three robot materials carry 0xbc07610c
/// (`ironhidevehicle_paintover.4x4_lod`, `optimusflight` `robot3_s` and `robot3_slod`), so this matters for them.
/// OPEN, read 2026-10-20: 005a2af0 builds ALL 24 variants of a material (005a35e4-005a36ef: index 0..0x17 into the
/// hash table 00b8de48 + 8 i, or 00b8df08 + 8 i when its flag `[esp+0x51]` is set; 005a3681 / 005a3678), so the
/// AlphaTest variant is picked at DRAW time by its index. settles: the draw's variant index for a 0xbc07610c part
/// (the caller that indexes the material's +0x28 / +0x2c variant arrays) and the upload of 0xa92bd3c2 into c2.
pub fn cutout_threshold(source: &model::Material) -> f32 {
    source.word("alpharef").unwrap_or(1).max(1) as f32 / 255.0
}

/// AlphaBoost of blendmode 0x3467929c [game 005a3033-005a3087]: 255 / max(X,1), where X is the
/// unnamed word 0x0d7779d1 (default 254). 1 for every other material. The output is clamped to
/// 1 where it is used [assumed: the consumer of AlphaBoost is not read].
pub fn alpha_boost(source: &model::Material) -> f32 {
    if blend_of(source) != Some(BLEND_BOOST) {
        return 1.0;
    }
    let x = source
        .params
        .iter()
        .find(|p| p.name == WORD_BOOST_SOURCE && p.owner == 0)
        .and_then(|p| match p.value {
            model::ParamValue::Word(v) => Some(v),
            _ => None,
        })
        .unwrap_or(254);
    255.0 / x.max(1) as f32
}

#[derive(Default)]
pub struct Cache {
    images: HashMap<(u32, bool), Option<Handle<Image>>>,
    materials: HashMap<u32, Made>,
    fallback: Option<Handle<StandardMaterial>>,
}

#[derive(Component)]
pub struct IflClock(pub tf2_core::ifl::Clock);

#[derive(Component)]
pub(crate) struct IflPart {
    clock: Entity,
    frames: [Option<Vec<Handle<Image>>>;2],
    material: Made,
    frame: [usize;2],
}

/// [game 005610f0, 00587fb0] Each Object's controller updates its render instances.
/// Animated material copies keep billboard frames and flashing rates independent.
pub fn animate_ifls(
    time: Res<Time>,
    mut clocks: Query<(Entity, &mut IflClock)>,
    mut parts: Query<&mut IflPart>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut surfaces: ResMut<Assets<Surface>>,
    mut glows: ResMut<Assets<Glow>>,
) {
    let frames: HashMap<_, _> = clocks.iter_mut().map(|(entity, mut clock)| {
        clock.0.step(time.delta_secs());
        (entity, clock.0.frame)
    }).collect();
    for mut part in &mut parts {
        let Some(&frame) = frames.get(&part.clock) else { continue };
        // [assumed] The render frame wraps through the ordered TEX list; its draw-side
        // modulo instruction remains to be checked. The clock side is [game 005610f0]: frame =
        // start + trunc(elapsed * fps), unbounded, so the wrap is the port's. OPEN: settles: the draw-side reader of
        // the IFL frame stored at renderInstance+0x50 by 00587fb0 (its modulo, not found yet). [game 2026-10-20]
        // 00587fb0 (render_instance_ifl_frame) is an ORPHAN in the export (disassembly only, no pseudocode): it
        // copies the callback's stack argument into renderInstance+0x50. The draw-side reader of +0x50 is not found
        // (grep of the render callers next: a read of [reg+0x50] after a call through the render instance's vtable).
        for layer in 0..2 {
            let Some(list) = &part.frames[layer] else { continue; };
            let next = frame.rem_euclid(list.len() as i32) as usize;
            if next == part.frame[layer] { continue; }
            let image = list[next].clone();
            match &part.material {
                Made::Standard(id) if layer == 0 => if let Some(mut material) = standard.get_mut(id) { material.base_color_texture = Some(image); },
                Made::Surface(id) => if let Some(mut material) = surfaces.get_mut(id) { if layer == 0 { material.set_diffuse(image); } else { material.set_secondary(image); } },
                Made::Glow(id) => if let Some(mut material) = glows.get_mut(id) { if layer == 0 { material.set_diffuse(image); } else { material.set_secondary(image); } },
                _ => {}
            }
            part.frame[layer] = next;
        }
    }
}

/// A part's material: one of the original's surface shaders where that is ported
/// (`surface.rs`), Bevy's own otherwise.
#[derive(Clone)]
#[allow(clippy::large_enum_variant)] // short-lived: one per part while a model is built
enum Made {
    Standard(Handle<StandardMaterial>),
    Surface(Handle<Surface>),
    /// The original's `emissive` or `dull_fx_emissive` (`fx.rs`).
    Glow(Handle<Glow>),
    /// The original's `sky` or `sky_vc` (`sky.rs`, LS-25). Held by value: the asset is added to
    /// `Assets<Sky>` when the spawn command runs (`insert_sky`), since `Builder` has no such store.
    Sky(Sky),
    /// The original's `ocean_water2` (`ocean.rs`). Held by value as `Sky` is; the asset is added once,
    /// by the first placement's command, and shared by the clones (`insert_ocean`).
    Ocean(Ocean, std::sync::Arc<std::sync::OnceLock<Handle<Ocean>>>),
    /// The original's `distortion_vc_emissive` (LS-47, `surface.rs` Distortion). Held by value as `Ocean` is.
    Distortion(Distortion, std::sync::Arc<std::sync::OnceLock<Handle<Distortion>>>),
}

/// Gives a child entity its sky material (LS-25). The `Assets<Sky>` store is reached through a
/// queued command, so no `Builder` field is needed.
fn insert_sky(commands: &mut Commands, child: Entity, sky: Sky) {
    commands.queue(move |world: &mut World| {
        let handle = world.resource_mut::<Assets<Sky>>().add(sky);
        world.entity_mut(child).insert(MeshMaterial3d(handle));
    });
}

/// Gives a child entity the sea's material, as `insert_sky`, but one asset per material (deep_six_01
/// has 2,040 tiles of one material).
fn insert_ocean(commands: &mut Commands, child: Entity, ocean: Ocean, shared: std::sync::Arc<std::sync::OnceLock<Handle<Ocean>>>) {
    commands.queue(move |world: &mut World| {
        let handle = shared.get_or_init(|| world.resource_mut::<Assets<Ocean>>().add(ocean)).clone();
        world.entity_mut(child).insert((MeshMaterial3d(handle), crate::ocean::OceanPart));
    });
}

fn new_ocean(ocean: Ocean) -> Made {
    Made::Ocean(ocean, Default::default())
}

/// Gives a child entity its distortion sphere's material, as `insert_ocean`: one asset per material.
fn insert_distortion(commands: &mut Commands, child: Entity, distortion: Distortion, shared: std::sync::Arc<std::sync::OnceLock<Handle<Distortion>>>) {
    commands.queue(move |world: &mut World| {
        let handle = shared.get_or_init(|| world.resource_mut::<Assets<Distortion>>().add(distortion)).clone();
        world.entity_mut(child).insert(MeshMaterial3d(handle));
    });
}

fn new_distortion(distortion: Distortion) -> Made {
    Made::Distortion(distortion, Default::default())
}

pub struct Builder<'a> {
    pub library: &'a Library,
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<StandardMaterial>,
    pub surfaces: &'a mut Assets<Surface>,
    pub glows: &'a mut Assets<Glow>,
    pub images: &'a mut Assets<Image>,
    pub bindposes: &'a mut Assets<SkinnedMeshInverseBindposes>,
    pub cache: &'a mut Cache,
}

/// A character part controlled by the pack's RenderSetEnabler.
#[derive(Component)]
pub struct RenderSetPart {
    pub root: Entity,
    pub model: String,
}

#[derive(Component)]
pub struct RenderSets(pub Vec<crate::formats::scene::RenderSetGroup>);

/// Optimus sends SpecialAttackMesh while his ultimate runs and NormalMesh when it ends
/// [game 0072afd0/0072b980]. Read the same active special timer used by the sim's shield.
pub fn update_render_sets(
    player: Single<&crate::player::Player>,
    roots: Query<&RenderSets, Or<(With<crate::player::RobotModel>, With<crate::player::VehicleModel>)>>,
    mut parts: Query<(&RenderSetPart, &mut Visibility)>,
) {
    use tf2_core::special_types::{SpecialType, NORMAL_MESH, SPECIAL_ATTACK_MESH};
    let special = player.state.special.as_ref().is_some_and(|s|
        s.is_on() && matches!(s.kind, SpecialType::Optimus | SpecialType::OptimusFlight));
    let selected = if special { SPECIAL_ATTACK_MESH } else { NORMAL_MESH };
    for (part, mut visibility) in &mut parts {
        let Ok(groups) = roots.get(part.root) else { continue };
        let visible = groups.0.iter().all(|group| {
            let is_special_group = group.sets.iter().any(|s| s.name == SPECIAL_ATTACK_MESH)
                && group.sets.iter().any(|s| s.name == NORMAL_MESH);
            let name = if is_special_group { selected } else { group.sets.first().map_or(0, |s| s.name) };
            group.model_visible(&part.model, name)
        });
        let wanted = if visible { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != wanted { *visibility = wanted; }
    }
}

pub fn image(texture: &Texture, srgb: bool) -> Option<Image> {
    // One face, or the six of a cube map (a reflection): each face's levels follow it in
    // the file, which is the order Bevy wants. Block formats need whole blocks.
    if texture.faces != 1 && texture.faces != 6 {
        return None;
    }
    let compressed = matches!(texture.format, PixelFormat::Dxt1 | PixelFormat::Dxt3 | PixelFormat::Dxt5);
    if compressed && (!texture.width.is_multiple_of(4) || !texture.height.is_multiple_of(4)) {
        return None;
    }
    let (format, data) = match (texture.format, srgb) {
        (PixelFormat::Dxt1, true) => (TextureFormat::Bc1RgbaUnormSrgb, texture.pixels.clone()),
        (PixelFormat::Dxt1, false) => (TextureFormat::Bc1RgbaUnorm, texture.pixels.clone()),
        (PixelFormat::Dxt3, true) => (TextureFormat::Bc2RgbaUnormSrgb, texture.pixels.clone()),
        (PixelFormat::Dxt3, false) => (TextureFormat::Bc2RgbaUnorm, texture.pixels.clone()),
        (PixelFormat::Dxt5, true) => (TextureFormat::Bc3RgbaUnormSrgb, texture.pixels.clone()),
        (PixelFormat::Dxt5, false) => (TextureFormat::Bc3RgbaUnorm, texture.pixels.clone()),
        (PixelFormat::Bgra8, true) => (TextureFormat::Bgra8UnormSrgb, texture.pixels.clone()),
        (PixelFormat::Bgra8, false) => (TextureFormat::Bgra8Unorm, texture.pixels.clone()),
        (PixelFormat::Bgr8, _) => {
            let data = texture.pixels.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
            (if srgb { TextureFormat::Bgra8UnormSrgb } else { TextureFormat::Bgra8Unorm }, data)
        }
        (PixelFormat::L8, _) => {
            let data = texture.pixels.iter().flat_map(|&l| [l, l, l, 255]).collect();
            (if srgb { TextureFormat::Bgra8UnormSrgb } else { TextureFormat::Bgra8Unorm }, data)
        }
    };
    let size = Extent3d { width: texture.width, height: texture.height, depth_or_array_layers: texture.faces };
    // Image::new validates a single uncompressed level. Our payload contains the
    // complete face-major mip chain, including the optional upscaled BGRA levels.
    let mut image = Image::new_uninit(size, TextureDimension::D2, format, RenderAssetUsages::RENDER_WORLD);
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = texture.mips;
    if texture.faces == 6 {
        image.texture_view_descriptor =
            Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    }
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    Some(image)
}

/// A part's Bevy mesh. `blend2` (LS-24): the part's material is `blend2_plus_mask_ssd`, which
/// reads three uv sets (`UV_0`, `UV_1` and `surface::BLEND2_UV2`); each is always there, and a
/// set the file does not give is copied from set 0 [stand-in]. Other parts get no extra sets,
/// so their meshes and pipelines are the same as before. OPEN: settles: the game's vertex-stream fill for a missing
/// TEXCOORD1 or 2 on a blend2 part (the declaration builder that feeds the blend2 technique; not read).
/// [data 2026-10-20, robots] No robot part is blend2: every `characters\*.str` part's technique was counted, and
/// none is `blend2_*`, so this copy never runs for the robots (levels keep the stand-in).
/// `layer2` (LS-41): the part's material is DIFFUSE2 (`basic_cheap`, `basic` and their `_ssd` forms),
/// which reads uv set 1 for its second layer; the mesh then carries it, a copy of set 0 when the file
/// does not give one [stand-in]. Parts of other techniques get no set 1, as before. OPEN: settles: the same fill
/// as for blend2 (the vertex declaration of DIFFUSE2 parts, the unpublished notes LS-41, not read).
/// [data 2026-10-20, robots] No robot part is basic_cheap, basic or their `_ssd` forms (the same count), so this
/// copy never runs for the robots either.
fn mesh(part: &model::Part, blend2: bool, layer2: bool, colour: bool, flatcard: bool) -> Mesh {
    let count = part.positions.len();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, part.positions.clone());
    // COLOR0 can be layer weights, not a tint. Only pass it to a known colour consumer;
    // fallback StandardMaterial would multiply those weights into the surface colour.
    if colour && part.colours.len() == count && count > 0 {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, part.colours.clone());
    }
      if flatcard {
          // [assumed] Missing anchors use the Object origin; missing encoded normals
          // use up. Installed US_Test flatcards are checked for authored anchor streams. OPEN: settles: the
          // game's flatcard vertex-stream reader (the uv1 and uv2 streams' fallback; not read).
          let anchors = if part.uv1_xyz.len() == count { part.uv1_xyz.clone() } else { vec![[0.0, 1.0, 0.0]; count] };
          let normals = if part.uv2_xyz.len() == count { part.uv2_xyz.clone() } else { vec![[0.0, 1.0, 1.0]; count] };
          mesh.insert_attribute(surface::FLATCARD_ANCHOR, anchors);
          mesh.insert_attribute(surface::FLATCARD_NORMAL, normals);
      }
    if blend2 {
        let set = |uvs: &[[f32; 2]], or: &[[f32; 2]]| if uvs.len() == count { uvs.to_vec() } else { or.to_vec() };
        let zero = vec![[0.0; 2]; count];
        let uv0 = set(&part.uvs, &zero);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, set(&part.uvs1, &uv0));
        mesh.insert_attribute(surface::BLEND2_UV2, set(&part.uvs2, &uv0));
        // [assumed] Missing TEXCOORD3 uses (0,0). Six installed US_Test planter
        // parts omit it; native missing-input handling still needs verification. OPEN: settles: the
        // planter parts' vertex declaration and the input the blend3 shader reads for an undeclared TEXCOORD3.
        mesh.insert_attribute(surface::BLEND3_UV3, set(&part.uvs3, &zero));
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv0);
    } else {
        if part.uvs.len() == count {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, part.uvs.clone());
        }
        if layer2 {
            let zero = vec![[0.0; 2]; count];
            let uv0 = if part.uvs.len() == count { part.uvs.clone() } else { zero };
            let uv1 = if part.uvs1.len() == count { part.uvs1.clone() } else { uv0 };
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv1);
        }
    }
    if part.normals.len() == count {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, part.normals.clone());
        if part.tangents.len() == count && part.binormals.len() == count {
            // Bevy rebuilds the bitangent as cross(normal, tangent) times the sign in w.
            let tangents: Vec<[f32; 4]> = (0..count)
                .map(|i| {
                    let (n, t, b) =
                        (Vec3::from(part.normals[i]), Vec3::from(part.tangents[i]), Vec3::from(part.binormals[i]));
                    let sign = if n.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 };
                    [t.x, t.y, t.z, sign]
                })
                .collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
            // The pack's own binormal stream (blend2 uses it, `surface::BINORMAL`); the tangent above is only
            // the rebuild's sign source and stays for every other technique.
            mesh.insert_attribute(surface::BINORMAL, part.binormals.clone());
        }
    }
    if !part.palette.is_empty() && part.blend_indices.len() == count && part.blend_weights.len() == count {
        let indices: Vec<[u16; 4]> = part.blend_indices.iter().map(|v| v.map(|i| i as u16)).collect();
        let weights: Vec<[f32; 4]> = part
            .blend_weights
            .iter()
            .map(|w| {
                let sum: f32 = w.iter().sum();
                if sum > 0.0 { w.map(|v| v / sum) } else { [1.0, 0.0, 0.0, 0.0] }
            })
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(indices));
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights);
    }
    mesh.insert_indices(Indices::U16(part.indices.clone()));
    if part.normals.len() != count {
        mesh.compute_normals();
    }
    mesh
}

impl Builder<'_> {
    fn image(&mut self, id: u32, srgb: bool) -> Option<Handle<Image>> {
        if let Some(found) = self.cache.images.get(&(id, srgb)) {
            return found.clone();
        }
        let resolved = self.library.texture_frames.get(&id).and_then(|s| s.frames.first()).copied().unwrap_or(id);
        let handle = self.library.textures.get(&resolved).and_then(|t| image(t, srgb)).map(|i| self.images.add(i));
        self.cache.images.insert((id, srgb), handle.clone());
        handle
    }

    /// Whether a part's material is `blend2_plus_mask_ssd` (LS-24): its mesh then carries the
    /// three uv sets (`mesh`).
    fn is_blend2_part(&self, part: &model::Part) -> bool {
        part.material.and_then(|id| self.library.materials.get(&id)).is_some_and(|m| surface::is_blend2(m.technique))
    }

    fn is_flatcard_part(&self, part: &model::Part) -> bool {
        part.material.and_then(|id| self.library.materials.get(&id)).is_some_and(|m| surface::is_flatcard(m.technique))
    }

    /// Whether a part's material is DIFFUSE2 (LS-41): its mesh then carries uv set 1 (`mesh`).
    fn is_layer2_part(&self, part: &model::Part) -> bool {
        part.material.and_then(|id| self.library.materials.get(&id)).is_some_and(|m| surface::has_diffuse2(m.technique)
            || m.technique == crc32(b"glow_vc_mask_emissive") || m.technique == crc32(b"distortion_vc_emissive"))
    }

    fn is_colour_part(&self, part: &model::Part) -> bool {
        part.material.and_then(|id| self.library.materials.get(&id)).is_some_and(|m|
            surface::uses_vertex_colour(m.technique) ||
            ["sky_vc", "bgnd_vc", "emissive_vc", "emissive_vc_ssd", "glow_soft_vc_emissive", "glow_vc_mask_emissive", "distortion_vc_emissive"].iter().any(|name| m.technique == crc32(name.as_bytes())))
    }

    fn material(&mut self, id: Option<u32>) -> Made {
        let Some((id, source)) = id.and_then(|id| Some((id, self.library.materials.get(&id)?))) else {
            let fallback =
                self.cache.fallback.get_or_insert_with(|| self.materials.add(Color::srgb(0.6, 0.6, 0.6))).clone();
            return Made::Standard(fallback);
        };
        if let Some(found) = self.cache.materials.get(&id) {
            return found.clone();
        }
        let colour = source.vector("diffuse_color").unwrap_or([1.0; 4]);
        let opacity = source.number("diffuse_texture1_opacity").unwrap_or(1.0);
        // [stand-in] A blended material whose picture is not in this pack (a level's cards
        // name pictures kept elsewhere) would be a plain pale sheet: it is not drawn.
        let pictureless = source.texture("diffuse_texture1_texture").is_some_and(|t| !self.library.textures.contains_key(&t) && !self.library.texture_frames.contains_key(&t));
        // [assumed] The `barrier` technique (the half-clear cards at a level's edge and round
        // closed-off places) is not drawn either: the original shows a barrier by rules not
        // read, and never as the plain sheet this would be.
        // 0x787f26b8 (`distortion_vc_emissive`, LS-47) is drawn by its own material (`surface.rs` Distortion,
        // `distortion.wgsl`): the listings' arithmetic, with the scene copy as the picture (a stand-in for
        // RealtimeSphereMap). It has no other path here. [data 2026-10-20] 66 robot materials; the level
        // copies are the truck's DamageLink stage (LS-47 section 10), and they are drawn the same way.
        if source.technique == crc32(b"distortion_vc_emissive") {
            let normal_map = source.texture("normal_map_texture").and_then(|t| self.image(t, false));
            let mask = source.texture("diffuse_texture2_texture").and_then(|t| self.image(t, false));
            let made = new_distortion(Distortion::build(source, normal_map, mask, alpha_mode_of(source)));
            self.cache.materials.insert(id, made.clone());
            return made;
        }
        // 0x15c51d1a is NOT left undrawn [game lsa_list `shaders.lsa`, 2026-10-20]: it is
        // `glow_vc_mask_emissive`, Base VS 049430 / PS 049a20, which the Glow path follows
        // (`glow.wgsl` MASK: texel.a = mask, vertex colour, edge and facing fade, AlphaBoost). It is
        // taken by `Glow::texture` (`fx.rs` edge()). 25 robot material entries (characters\*.str).
        if pictureless && matches!(blend_of(source), Some(BLEND_ADDITIVE) | Some(BLEND_NORMAL)) {
            let made = Made::Standard(self.materials.add(StandardMaterial {
                base_color: Color::NONE,
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            }));
            self.cache.materials.insert(id, made.clone());
            return made;
        }
        // The level's sky domes and cylinders (LS-25, `sky.rs`): the blend is the material's own, as
        // for the other paths (`alpha_mode_of`). Drawn only where a level's `sky.` models are shown
        // (`level.rs`, `--level-all`).
        if let Some(wanted) = Sky::texture(source) {
            let diffuse = wanted.and_then(|id| self.image(id, false));
            let alpha_mode = alpha_mode_of(source);
            // The bloom texture is the alpha (LS26): read as data, not sRGB, as the Surface path does.
            let bloom = source.texture("bloom_texture").and_then(|t| self.image(t, false));
            let made = Made::Sky(Sky::new(source, diffuse, bloom, alpha_mode));
            self.cache.materials.insert(id, made.clone());
            return made;
        }
        // The sea, `ocean_water2` (`ocean.rs`): its normal maps and its water-colour table are read as
        // data, not sRGB, as the Surface path's textures.
        if let Some(wanted) = crate::ocean::textures(source) {
            let images = wanted.map(|id| id.and_then(|id| self.image(id, false)));
            let made = new_ocean(Ocean::new(source, images, alpha_mode_of(source)));
            self.cache.materials.insert(id, made.clone());
            return made;
        }
        // The original's own shader for the techniques that are ported; its arithmetic is
        // on the stored values, so no texture is decoded from sRGB.
        if let Some(wanted) = surface::textures(source) {
            let mut wanted = wanted;
            // The shader's reflection is a cube map. A level's materials also name flat
            // pictures there (seen in `levels\shanghai_01`): [stand-in] those are left out.
            if wanted[3].is_some_and(|id| self.library.textures.get(&id).is_some_and(|t| t.faces != 6)) {
                wanted[3] = None;
            }
            let images = wanted.map(|id| id.and_then(|id| self.image(id, false)));
            // blend2 (LS-24): its second diffuse layer, the mask and the second normal map too.
            let built = match surface::layer_textures(source) {
                Some(layers) => {
                    let layers = layers.map(|id| id.and_then(|id| self.image(id, false)));
                    surface::build_layered(source, images, layers, alpha_mode_of(source))
                }
                None => surface::build(source, images, alpha_mode_of(source)),
            };
            if let Some(built) = built {
                let made = Made::Surface(self.surfaces.add(built));
                self.cache.materials.insert(id, made.clone());
                return made;
            }
        }
        // Unlit and emissive vertex-colour techniques, including their cut-out main/depth passes.
        let alpha_mode = alpha_mode_of(source);
        if let Some(wanted) = Glow::texture(source) {
            let diffuse = wanted.and_then(|id| self.image(id, false));
            if let Some(built) = Glow::build(source, diffuse, alpha_mode) {
                let mask = source.texture("diffuse_texture2_texture").and_then(|id| self.image(id, false));
                let made = Made::Glow(self.glows.add(built.with_mask(mask)));
                self.cache.materials.insert(id, made.clone());
                return made;
            }
        }
        let base_color_texture = source.texture("diffuse_texture1_texture").and_then(|t| self.image(t, true));
        let normal_map_texture = source.texture("normal_map_texture").and_then(|t| self.image(t, false));
        let blend = blend_of(source);
        // The game's own shaders are not reproduced; these are stand-in surface settings.
        let mut material = StandardMaterial {
            base_color: Color::srgb(colour[0], colour[1], colour[2]),
            base_color_texture,
            normal_map_texture,
            perceptual_roughness: 0.5,
            metallic: 0.35,
            ..default()
        };
        if blend == Some(BLEND_ADDITIVE) {
            material.alpha_mode = AlphaMode::Add;
            material.unlit = true;
            material.base_color = Color::srgba(colour[0], colour[1], colour[2], opacity.clamp(0.0, 1.0));
        } else if blend == Some(BLEND_NORMAL) || blend == Some(BLEND_BOOST) {
            // The boost is 1 for Normal [game 005a3087: the alphaboost record is 1.0 except for 0x3467929c]; for 0x3467929c
            // it scales the alpha (clamped) [assumed: the consumer of the alphaboost record is not read; settles: the
            // shader constant upload of 0x201fea10, the unpublished notes "Not settled" 1].
            material.alpha_mode = AlphaMode::Blend;
            material.base_color = Color::srgba(colour[0], colour[1], colour[2], (opacity * alpha_boost(source)).clamp(0.0, 1.0));
        } else if blend == Some(BLEND_CUTOUT) {
            // Bevy's Mask: a pixel whose alpha (texture alpha times the base alpha) is below the
            // threshold is dropped, the rest are opaque [game 005a30ef threshold; assumed compare].
            material.alpha_mode = AlphaMode::Mask(cutout_threshold(source));
            material.base_color = Color::srgba(colour[0], colour[1], colour[2], opacity.clamp(0.0, 1.0));
        }
        let made = Made::Standard(self.materials.add(material));
        self.cache.materials.insert(id, made.clone());
        made
    }

    /// Spawns models that only stand somewhere (a level's drawn geometry): each is a mesh id
    /// and the 4x3 row-vector matrix that places it, in game space, and the Instance that places it
    /// (None: under the root). One Bevy mesh per part is made once and shared by every placement;
    /// the flag says whether it casts a shadow. Skinned parts are drawn unposed. The parts of one
    /// Instance hang under one `LevelInstance` entity (hidden as one). Returns the root (which holds
    /// the conversion from game space) and the counts.
    /// `tint`: the Instance's Colorizer colour (`PlacedModel::tint`), multiplied into each part's material.
    pub fn spawn_placed(&mut self, commands: &mut Commands, label: &str, placed: impl Iterator<Item = (u32, [f32; 12], bool, Option<Owner>, Option<tf2_core::ifl::Clock>, bool, Option<[f32; 3]>)>) -> PlacedSpawn {
        // Tinted copies of a shared material, one per (material, colour).
        let mut tinted: HashMap<(AssetId<Mesh>, [u32; 3]), Made> = HashMap::new();
        let root = commands
            .spawn((Transform::from_rotation(model_space_rotation()), Visibility::default(), Name::new(label.to_owned())))
            .id();
        let mut made: HashMap<(u32, bool), Vec<(Handle<Mesh>, Made, [Option<Vec<Handle<Image>>>;2])>> = HashMap::new();
        let mut owner_roots: HashMap<Owner, Entity> = HashMap::new();
        let mut missing = 0;
        let (mut parts, mut instance_parts) = (0, 0);
        let mut models = Vec::new();
        for (id, world, shadow, owner, clock, lod_fade, tint) in placed {
            let key = (id, lod_fade);
            if let std::collections::hash_map::Entry::Vacant(e) = made.entry(key) {
                let Some(source) = self.library.meshes.get(&id) else {
                    missing += 1;
                    models.push(None);
                    continue;
                };
                let parts = source
                    .parts
                    .iter()
                    .map(|part| {
                        // No skin is given to a placed model, so its mesh must not ask for one.
                        let mut made = mesh(part, self.is_blend2_part(part), self.is_layer2_part(part), self.is_colour_part(part), self.is_flatcard_part(part));
                        made.remove_attribute(Mesh::ATTRIBUTE_JOINT_INDEX);
                        made.remove_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT);
                        let mut material = self.material(part.material);
                        if lod_fade {
                            match &material {
                                Made::Surface(id) => {
                                    let mut m = self.surfaces.get(id).unwrap().clone();
                                    m.enable_lod_fade();
                                    material = Made::Surface(self.surfaces.add(m));
                                }
                                Made::Glow(id) => {
                                    let mut m = self.glows.get(id).unwrap().clone();
                                    m.enable_lod_fade();
                                    material = Made::Glow(self.glows.add(m));
                                }
                                // [stand-in] StandardMaterial fallback parts switch/cull but
                                // cannot consume our MeshTag fade. Remaining techniques are owed. OPEN: settles:
                                // the list of techniques that reach this fallback (a count per level), then their
                                // shaders' LOD fade.
                                _ => {}
                            }
                        }
                        let frames = ["diffuse_texture1_texture","diffuse_texture2_texture"].map(|name|
                            part.material.and_then(|id| self.library.materials.get(&id)).and_then(|m|m.texture(name))
                                .and_then(|id|self.library.texture_frames.get(&id)).map(|s|s.frames.clone()));
                        let srgb = matches!(material, Made::Standard(_));
                        let frames = frames.map(|list|list.and_then(|ids| ids.into_iter().map(|id| self.image(id, srgb)).collect::<Option<Vec<_>>>()));
                        (self.meshes.add(made), material, frames)
                    })
                    .collect();
                e.insert(parts);
            }
            let rows = [[world[0], world[1], world[2]], [world[3], world[4], world[5]], [world[6], world[7], world[8]], [world[9], world[10], world[11]]];
            let transform = Transform::from_matrix(matrix(&rows));
            // [stand-in] One entity per Instance, under the level root, with the identity transform: the parts keep
            // their absolute transforms, so nothing moves. Hiding it hides the Instance's parts.
            let parent = match owner {
                Some(key) => *owner_roots.entry(key).or_insert_with(|| {
                    let entity = commands
                        .spawn((Transform::default(), Visibility::default(), LevelInstance { owner: key }, Name::new(format!("instance {key:?}"))))
                        .id();
                    commands.entity(root).add_child(entity);
                    entity
                }),
                None => root,
            };
            let placement = commands.spawn((transform, Visibility::Inherited)).id();
            if let Some(clock) = clock { commands.entity(placement).insert(IflClock(clock)); }
            commands.entity(parent).add_child(placement);
            models.push(Some(placement));
            for (mesh_handle, shared_material, frames) in &made[&key] {
                let material = if frames.iter().any(Option::is_some) && clock.is_some() {
                    match shared_material {
                        Made::Standard(id) => Made::Standard(self.materials.add(self.materials.get(id).unwrap().clone())),
                        Made::Surface(id) => Made::Surface(self.surfaces.add(self.surfaces.get(id).unwrap().clone())),
                        Made::Glow(id) => Made::Glow(self.glows.add(self.glows.get(id).unwrap().clone())),
                        Made::Sky(sky) => Made::Sky(sky.clone()),
                        Made::Ocean(ocean, _) => new_ocean(ocean.clone()),
                        Made::Distortion(distortion, _) => new_distortion(distortion.clone()),
                    }
                } else { shared_material.clone() };
                // [assumed] A Colorizer's colour multiplies every part of its Instance (ObjectDiffuseColor, LS-56/58).
                // A part with its own IFL copy (cloned above) is tinted in place; the shared tinted copies are
                // only for parts that share their material. OPEN: settles: the writer of the material record's +0x14
                // float4 that the draw binds as ObjectDiffuseColor (the unpublished notes LS-58: not found).
                let own_copy = frames.iter().any(Option::is_some) && clock.is_some();
                let material = match (tint, own_copy) {
                    // By value, like the ocean: the tinted copy is a new material.
                    (Some(rgb), true) if matches!(material, Made::Distortion(..)) => {
                        let mut m = match &material { Made::Distortion(d, _) => d.clone(), _ => unreachable!() };
                        m.tint(Vec3::from_array(rgb));
                        new_distortion(m)
                    }
                    (Some(rgb), true) => {
                        let rgb = Vec3::from_array(rgb);
                        match &material {
                            Made::Surface(id) => { if let Some(mut m) = self.surfaces.get_mut(id) { m.tint(rgb); } }
                            Made::Glow(id) => { if let Some(mut m) = self.glows.get_mut(id) { m.tint(rgb); } }
                            _ => {}
                        }
                        material
                    }
                    _ => material,
                };
                let material = match tint {
                    Some(rgb) if !own_copy => tinted.entry((mesh_handle.id(), rgb.map(f32::to_bits))).or_insert_with(|| {
                        let rgb = Vec3::from_array(rgb);
                        match &material {
                            Made::Surface(id) => { let mut m = self.surfaces.get(id).unwrap().clone(); m.tint(rgb); Made::Surface(self.surfaces.add(m)) }
                            Made::Glow(id) => { let mut m = self.glows.get(id).unwrap().clone(); m.tint(rgb); Made::Glow(self.glows.add(m)) }
                            Made::Standard(id) => {
                                let mut m = self.materials.get(id).unwrap().clone();
                                let c = m.base_color.to_srgba();
                                m.base_color = Color::srgba(c.red * rgb.x, c.green * rgb.y, c.blue * rgb.z, c.alpha);
                                Made::Standard(self.materials.add(m))
                            }
                            Made::Sky(sky) => Made::Sky(sky.clone()),
                            Made::Ocean(ocean, _) => { let mut m = ocean.clone(); m.tint(rgb); new_ocean(m) }
                            Made::Distortion(d, _) => { let mut m = d.clone(); m.tint(rgb); new_distortion(m) }
                        }
                    }).clone(),
                    _ => material,
                };
                let child = commands.spawn((Mesh3d(mesh_handle.clone()), Transform::default())).id();
                if lod_fade { commands.entity(child).insert(bevy::mesh::MeshTag(0x8000_00ff)); }
                match material.clone() {
                    Made::Standard(handle) => { commands.entity(child).insert(MeshMaterial3d(handle)); }
                    Made::Surface(handle) => { commands.entity(child).insert(MeshMaterial3d(handle)); }
                    Made::Glow(handle) => { commands.entity(child).insert(MeshMaterial3d(handle)); }
                    Made::Sky(sky) => insert_sky(commands, child, sky),
                    Made::Ocean(ocean, shared) => insert_ocean(commands, child, ocean, shared),
                    Made::Distortion(distortion, shared) => insert_distortion(commands, child, distortion, shared),
                };
                if !shadow {
                    commands.entity(child).insert(bevy::light::NotShadowCaster);
                }
                // [assumed, LS-25] a sky casts and receives no shadow: the sky models are placed
                // as `sky.` (`level.rs` `backdrop`, shadow flag off) and the original's sky shader
                // has no shadow term (`sky.wgsl`).
                if matches!(material, Made::Sky(_)) {
                    commands.entity(child).insert(bevy::light::NotShadowReceiver);
                }
                if clock.is_some() && frames.iter().any(Option::is_some) {
                    commands.entity(child).insert(IflPart { clock: placement, frames: frames.clone(), material: material.clone(), frame: [usize::MAX;2] });
                }
                commands.entity(placement).add_child(child);
                parts += 1;
                instance_parts += owner.is_some() as usize;
            }
        }
        PlacedSpawn { root, missing, parts, instance_parts, instances: owner_roots.len(), models }
    }

    /// Spawns an object's node tree with its rendered parts, and returns the root and each
    /// node's rest transform. The root holds the conversion from game space, so children keep
    /// the game's coordinates.
    pub fn spawn(&mut self, commands: &mut Commands, object: &ObjectDef, label: &str) -> (Entity, Vec<Transform>) {
        let world: Vec<Mat4> = object.nodes.iter().map(|n| matrix(&n.bind)).collect();
        let rest: Vec<Transform> = object
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| match node.parent {
                Some(parent) => Transform::from_matrix(world[parent].inverse() * world[i]),
                None => Transform::from_matrix(world[i]),
            })
            .collect();
        let entities: Vec<Entity> = object
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| commands.spawn((rest[i], Visibility::default(), Name::new(node.name.clone()))).id())
            .collect();
        let root = commands
            .spawn((Transform::from_rotation(model_space_rotation()), Visibility::default(), Name::new(label.to_owned())))
            .id();
        if !object.render_sets.is_empty() {
            commands.entity(root).insert(RenderSets(object.render_sets.clone()));
        }
        for (i, node) in object.nodes.iter().enumerate() {
            commands.entity(node.parent.map_or(root, |p| entities[p])).add_child(entities[i]);
        }
        // Each node's own frame (`ObjectNodes::frames`), which the skins and parts below take.
        let frames: Vec<Entity> = entities
            .iter()
            .map(|&node| {
                let frame = commands.spawn((Transform::IDENTITY, Visibility::default())).id();
                commands.entity(node).add_child(frame);
                frame
            })
            .collect();

        for render in &object.renders {
            let Some(source) = self.library.meshes.get(&lower33(&render.model)) else {
                warn!("{label}: no mesh named {}", render.model);
                continue;
            };
            let skeleton = source.skeleton.and_then(|id| self.library.skeletons.get(&id));
            for part in &source.parts {
                let mesh_handle = self.meshes.add(mesh(part, self.is_blend2_part(part), self.is_layer2_part(part), self.is_colour_part(part), self.is_flatcard_part(part)));
                let material = self.material(part.material);
                let parts = (Mesh3d(mesh_handle), Name::new(source.name.clone()));
                let child = match skeleton.filter(|_| !part.palette.is_empty()) {
                    None => {
                        let child = commands.spawn((parts, Transform::IDENTITY)).id();
                        commands.entity(frames[render.node]).add_child(child);
                        child
                    }
                    Some(bones) => {
                        // One joint per palette entry, so the bone numbers in the vertices index it directly.
                        let mut joints = Vec::new();
                        let mut inverse = Vec::new();
                        for &bone in &part.palette {
                            let Some(bone) = bones.get(bone as usize) else { continue };
                            let node = object.nodes.iter().position(|n| n.bone_hash == bone.name);
                            joints.push(node.map_or(root, |n| frames[n]));
                            inverse.push(matrix(&bone.inverse_bind));
                        }
                        let skin = SkinnedMesh { inverse_bindposes: self.bindposes.add(inverse), joints };
                        let child = commands.spawn((parts, Transform::IDENTITY, skin)).id();
                        commands.entity(root).add_child(child);
                        child
                    }
                };
                match material {
                    Made::Standard(handle) => { commands.entity(child).insert(MeshMaterial3d(handle)); }
                    Made::Surface(handle) => { commands.entity(child).insert(MeshMaterial3d(handle)); }
                    Made::Glow(handle) => { commands.entity(child).insert(MeshMaterial3d(handle)); }
                    Made::Sky(sky) => insert_sky(commands, child, sky),
                    Made::Ocean(ocean, shared) => insert_ocean(commands, child, ocean, shared),
                    Made::Distortion(distortion, shared) => insert_distortion(commands, child, distortion, shared),
                };
                if !object.render_sets.is_empty() {
                    let visible = object.render_sets.iter().all(|g| g.model_visible(&render.model, g.sets.first().map_or(0, |s| s.name)));
                    commands.entity(child).insert((
                        RenderSetPart { root, model: render.model.clone() },
                        if visible { Visibility::Inherited } else { Visibility::Hidden },
                    ));
                }
            }
        }
        let names = object.nodes.iter().map(|n| n.name.clone()).collect();
        let name_hashes = object.nodes.iter().map(|n| n.name_hash).collect();
        let parents = object.nodes.iter().map(|n| n.parent).collect();
        let spine_links = object.spine_links.clone();
        let foot_rig = tf2_core::footik::FootRig::read(object, self.library);
        commands.entity(root).insert(ObjectNodes { entities, frames, names, name_hashes, rest: rest.clone(), parents, spine_links, foot_rig });
        (root, rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_upscale_bgra_mip_chain_is_uploaded_in_full() {
        let source = Texture {
            name: "test".into(), id: 1, width: 4, height: 4, faces: 1, mips: 3,
            format: PixelFormat::Bgra8, pixels: [10, 20, 30, 136].repeat(16 + 4 + 1),
        };
        for srgb in [false, true] {
            let result = image(&source, srgb).unwrap();
            assert_eq!(result.texture_descriptor.mip_level_count, 3);
            assert_eq!(result.data.as_deref().unwrap(), source.pixels);
            assert_eq!(result.texture_descriptor.format, if srgb { TextureFormat::Bgra8UnormSrgb } else { TextureFormat::Bgra8Unorm });
        }
    }
}
