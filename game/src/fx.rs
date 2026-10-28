//! The original's shaders for what glows: particles (`fx.wgsl`, from the shaders compiled
//! into the executable) and the unlit mesh techniques `emissive` and `dull_fx_emissive`
//! (`glow.wgsl`, from `shaders.lsa`). Each shader file says what in it is the game's.

use bevy::asset::uuid_handle;
use bevy::prelude::*;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::render::render_resource::{AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::{Shader, ShaderRef};
use tf2_core::particles::{Blend, Fog, Renderer};

use crate::formats::hash::crc32;
use crate::formats::model;

const FX_SHADER: Handle<Shader> = uuid_handle!("0d6a3f1e-52c7-4b8e-8e1a-7c41f3a9d2b6");
const GLOW_SHADER: Handle<Shader> = uuid_handle!("b7e2c4a9-18f3-4d65-9c0b-5a2e6d7f8c13");
const WARP_SHADER: Handle<Shader> = uuid_handle!("5c1d9e72-3a4b-4f60-b8d1-2e7a9c0f4b35");

const ADD: u32 = 1;
const FEATHER: u32 = 2;
const LIT: u32 = 4;
/// `BlendMode` 2: the source times its alpha taken from what is behind (`specialize`).
const SUBTRACT: u32 = 8;
const EDGE: u32 = 2;
const VERTEX_COLOUR: u32 = 4;
const GLOW_RIG: u32 = 8;
const CUTOUT: u32 = 16;
const SOFT: u32 = 32;
const BARRIER: u32 = 128;

/// [game 00781a00, 005a6f20] the game's near plane: the camera base's default near 0.5 (`in_EAX`
/// dword 0x26 = +0x98, 0x3f000000), which World::Draw passes on at 005a6f20 l.99 (`FUN_00a96e80(+0x98, +0x9c)`)
/// into the render camera's near at +0x3cc. The main camera (main.rs) uses the same 0.5. [game] the
/// writer of the camera input's +0x98 for other views is not read: it is the default here.
const NEAR: f32 = 0.5;
/// [stand-in] `LightColor` for the shock rings' edge light (the level's in the original).
const LIGHT: Vec3 = Vec3::new(1.0, 0.97, 0.9);

#[derive(Clone, ShaderType)]
pub struct FxValues {
    flags: UVec4,
    values: Vec4,
    /// The lit shaders' constants: `DIR_TO_LIGHT`, `DIFFUSE_COLOR`, `AMBIENT_COLOR` and
    /// `INV_COLOR_COEFF` (its w the back-light).
    to_light: Vec4,
    diffuse: Vec4,
    ambient: Vec4,
    inverse: Vec4,
    fog: Vec4,
    fog_colour: Vec4,
}


/// What a particle material's pipeline depends on beyond its bindings: `BlendMode` 2's blend
/// operation, which Bevy's alpha modes do not have (`specialize`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FxKey {
    subtract: bool,
    opaque: bool,
}

impl From<&Fx> for FxKey {
    fn from(fx: &Fx) -> Self {
        FxKey { subtract: fx.values.flags.x & SUBTRACT != 0, opaque: fx.opaque }
    }
}

/// A particle renderer's material.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(FxKey)]
pub struct Fx {
    #[uniform(0)]
    values: FxValues,
    #[texture(1)]
    #[sampler(2)]
    picture: Option<Handle<Image>>,
    alpha_mode: AlphaMode,
    /// `BlendMode` 4 (`Blend::Opaque`): the source as it is (`specialize`).
    opaque: bool,
}

impl Material for Fx {
    fn vertex_shader() -> ShaderRef {
        FX_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        FX_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    /// [game 00a9db20] `BlendMode` 2: SRCBLEND SRCALPHA, DESTBLEND ONE, BLENDOP REVSUBTRACT (3):
    /// what is behind less the source times its alpha. The shader hands over the colour already
    /// times its alpha (as for the additive mode), so the source factor here is one.
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        use bevy::render::render_resource::{BlendComponent, BlendFactor, BlendOperation, BlendState};
        // [game 00a9db20] `BlendMode` 4: SRCBLEND ONE (2), DESTBLEND ZERO (1), BLENDOP ADD (1), with
        // no separate alpha blend: the colour replaces what is behind, alpha included.
        let blend = if key.bind_group_data.subtract {
            Some(BlendState {
                color: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::ReverseSubtract },
                alpha: BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
            })
        } else if key.bind_group_data.opaque {
            Some(BlendState {
                color: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::Zero, operation: BlendOperation::Add },
                alpha: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::Zero, operation: BlendOperation::Add },
            })
        } else {
            None
        };
        if let Some(blend) = blend
            && let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(blend);
            }
        }
        Ok(())
    }
}

impl Fx {
    /// [game 00a9db20] the blend by `BlendMode`: 0 and 3 source alpha over the rest, 1 added,
    /// 2 taken away (`specialize`); every playable pack has one `BlendMode` 2 element (a `CUVA`
    /// on `pyro_rollfire1b`). `Opaque` (4) is the source as it is, one / zero (`specialize`):
    /// [game 00a9db20] its case sets BLENDOP 1, SRCBLEND 2 (ONE), DESTBLEND 1 (ZERO). No playable
    /// pack's renderer has it (the 2026-10-18 survey), so it is never drawn in play.
    pub fn new(renderer: &Renderer, picture: Option<Handle<Image>>) -> Self {
        let blend = renderer.blend();
        let add = blend == Blend::Add;
        let subtract = blend == Blend::Subtract;
        let opaque = blend == Blend::Opaque;
        let flags = if add { ADD } else { 0 } | if subtract { SUBTRACT } else { 0 } | if renderer.z_feather > 0.0 { FEATHER } else { 0 } | if renderer.lit() { LIT } else { 0 };
        // `FUN_00973310`: diffuse = `DiffuseCoeff` x the light's colour, ambient =
        // `AmbientCoeff` x the ambient colour, and the "inverse" colour = 1 less both
        // coefficients (what of the particle no light changes), with `BackLitCoeff` as
        // its w. [game: 00973310 uses 005431f0 to copy RGB with an explicit zero w]
        let (diffuse, ambient) = (Vec3::from(renderer.diffuse), Vec3::from(renderer.ambient));
        // The level's light: its sun's colour and its ambient colour. [data: the level
        // the arena borrows its environment from] [game 00973310: the particles take the render node's direction at +0xc0 and its colours at +0xd0 (diffuse) and +0xe0 (ambient)] [assumed: the level_light (the sun and ambient) is what those node words hold; the writer of +0xc0..+0xe0 is not read] settles: the code that fills the render node's +0xc0..+0xe0 (the caller of 00973310's node, EDI+0xc0; read to see which light it takes).
        let (level_diffuse, level_ambient) = crate::world::level_light();
        // [game 00a9b620] Fog is disabled for BlendMode 1/2. The fogHotspot/range/falloff mapping is the game's:
        // [game 00567660] environment_load writes fogHotspot to the Environment node +0xd4, fogRange to
        // +0xd8 and fogFalloff x 0.01 to +0xdc, the same three values as `Fog::new` takes here. [assumed] that
        // particles take these values. OPEN, read 2026-10-20: particles_render_begin 0057e670 feeds
        // `particle_fog_set` 00aa0500 from the shared game state (global_game_state 005a9cc0): colour +0x194,
        // near +0x1a4, far +0x1a8, amount +0x1ac. So the particles' fog is the state's words, not the
        // Environment node's +0xd4..+0xdc that this code passes. env_load writes +1a4..+1ac as
        // radialBlurminBlend / maxBlend / Iterations (notes), which would make them not fog at all.
        // settles: the writer of state +0x1a8 and +0x1ac (a decoded store to 005a9cc0's result, not
        // yet found) and whether they equal fogRange / fogFalloff; if not, this port's particle fog is wrong.
        // Ruled out (lead, 2026-10-20): 00585880 (LS-6's candidate) writes +0x194..+0x1b8 of a VIEW object
        // (its frustum planes, param_1), not the game state 00dadde0; no code takes 00dadf84 / 00dadf88 /
        // 00dadf8c as an immediate. The open copy is the unpublished notes section 5 (a): the
        // Environment block shifted by +0xd0, via the caller of 005a6f20.
        let (fog, fog_colour) = crate::world::look().filter(|_| renderer.fogged()).map_or(
            (Fog::default(), Vec3::ZERO),
            |e| (Fog::new(e.fog_hotspot, e.fog_range, 0.0, e.fog_falloff * 0.01), e.fog_colour),
        );
        let [fade_slope, fade_intercept] = renderer.fade_constants();
        Fx {
            values: FxValues {
                flags: UVec4::new(flags, 0, 0, 0),
                values: Vec4::new(NEAR, fade_slope, fade_intercept, 0.0),
                to_light: crate::world::to_sun().extend(0.0),
                diffuse: (diffuse * level_diffuse).extend(0.0),
                ambient: (ambient * level_ambient).extend(0.0),
                inverse: (Vec3::ONE - diffuse - ambient).extend(renderer.back_lit),
                fog: Vec4::from_array(fog.0),
                fog_colour: fog_colour.extend(0.0),
            },
            picture,
            // Subtract rides the transparent phase as Add does; `specialize` changes its blend.
            alpha_mode: if add || subtract { AlphaMode::Add } else { AlphaMode::Blend },
            opaque,
        }
    }
}

#[derive(Clone, ShaderType)]
pub struct GlowValues {
    colour: Vec4,
    uv_xy: Vec4,
    uv_zt: Vec4,
    edge: Vec4,
    falloff: Vec4,
    light: Vec4,
    flags: UVec4,
    fog_colour: Vec4,
    fog_range: Vec4,
    cutout: Vec4,
    mask_uv_xy: Vec4,
    mask_uv_zt: Vec4,
    /// x scrolling clock, yzw BarrierPlayerPos in Bevy world space.
    dynamic: Vec4,
}

/// What a Glow pipeline depends on beyond its bindings: a DepthBias shifts the colour pass's depth.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlowKey {
    depth_bias: bool,
}

impl From<&Glow> for GlowKey {
    fn from(glow: &Glow) -> Self {
        static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let off = *OFF.get_or_init(|| std::env::args().any(|a| a == "--no-depth-bias"));
        GlowKey { depth_bias: glow.values.cutout.w != 0.0 && !off }
    }
}

/// Unlit emissive, emissive_vc, dull_fx_emissive and depth-softened glow_soft_vc_emissive meshes.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(GlowKey)]
pub struct Glow {
    #[uniform(0)]
    values: GlowValues,
    #[texture(1)]
    #[sampler(2)]
    diffuse: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    mask: Option<Handle<Image>>,
    alpha_mode: AlphaMode,
    /// `Diffuse_Color`, kept so the object's colour can be laid over it again.
    diffuse_color: Vec4,
}

impl Material for Glow {
    fn fragment_shader() -> ShaderRef {
        GLOW_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn prepass_fragment_shader() -> ShaderRef {
        GLOW_SHADER.into()
    }

    fn enable_shadows() -> bool {
        false
    }

    /// As `Surface::specialize`: the depth prepass must drop the faces the main pass drops.
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = Some(Face::Back);
        // As `Surface::specialize`: DepthBias shifts the colour pass's depth only (`GLOW_DEPTH_BIAS`).
        let prepass = descriptor.vertex.shader_defs.iter().any(|d| *d == "PREPASS_PIPELINE".into());
        if key.bind_group_data.depth_bias && !prepass
            && let Some(fragment) = descriptor.fragment.as_mut() { fragment.shader_defs.push("GLOW_DEPTH_BIAS".into()); }
        if !descriptor.vertex.shader_defs.contains(&"VERTEX_OUTPUT_INSTANCE_INDEX".into()) {
            descriptor.vertex.shader_defs.push("VERTEX_OUTPUT_INSTANCE_INDEX".into());
        }
        if let Some(fragment) = descriptor.fragment.as_mut()
            && !fragment.shader_defs.contains(&"VERTEX_OUTPUT_INSTANCE_INDEX".into()) { fragment.shader_defs.push("VERTEX_OUTPUT_INSTANCE_INDEX".into()); }
        Ok(())
    }
}

impl Glow {
    pub fn set_diffuse(&mut self, image: Handle<Image>) { self.diffuse = Some(image); }
    pub fn set_secondary(&mut self, image: Handle<Image>) { self.mask = Some(image); }
    pub fn enable_lod_fade(&mut self) {
        if self.alpha_mode == AlphaMode::Opaque { self.alpha_mode = AlphaMode::Mask(0.0); }
    }
    /// The object's own colour's alpha (`ObjectDiffuseColor`, which the vertex shader
    /// multiplies into the material's): what fades an effect's object out.
    /// A placed Instance's Colorizer colour (as `Surface::tint`). [assumed] ObjectDiffuseColor. OPEN: settles:
    /// the game's write of a Colorizer colour into the object's `ObjectDiffuseColor` (c23) in the instance
    /// draw (`tools\decomp_lookup.py grep ObjectDiffuseColor`, the LS-56 / LS-58 notes).
    pub fn tint(&mut self, rgb: Vec3) {
        self.diffuse_color *= rgb.extend(1.0);
        self.values.colour *= rgb.extend(1.0);
    }
    pub fn set_object_alpha(&mut self, alpha: f32) {
        self.values.colour = self.diffuse_color * Vec4::new(1.0, 1.0, 1.0, alpha);
    }

    /// The colour texture a material of these techniques wants, or `None` for another
    /// technique.
    pub fn texture(source: &model::Material) -> Option<Option<u32>> {
        edge(source.technique).map(|_| source.texture("diffuse_texture1_texture"))
    }

    pub fn build(source: &model::Material, diffuse: Option<Handle<Image>>, alpha_mode: AlphaMode) -> Option<Self> {
        let edge = edge(source.technique)?;
        let pair = |name: &str, or: [f32; 2]| source.vector(name).map_or(or, |v| [v[0], v[1]]);
        let ([xx, xy], [yx, yy]) = (pair("diffuse_texture1_x", [1.0, 0.0]), pair("diffuse_texture1_y", [0.0, 1.0]));
        let ([zx, zy], [tx, ty]) = (pair("diffuse_texture1_z", [0.0; 2]), pair("diffuse_texture1_t", [0.0; 2]));
        let number = |name: &str, or: f32| source.number(name).unwrap_or(or);
        let barrier = source.technique == crc32(b"barrier");
        let diffuse_color = if barrier { Vec4::ONE } else { source.vector("diffuse_color").map_or(Vec4::ONE, Vec4::from) };
        let add = alpha_mode == AlphaMode::Add;
        let rig = crate::world::mission_rig();
        let (fog_colour, fog_range) = rig.map_or((Vec4::ZERO, Vec4::ZERO), |r| (r.fog_color(), r.fog_range()));
        let cutout = if let AlphaMode::Mask(t) = alpha_mode { Some(t) } else { None };
        let soft = source.technique == crc32(b"glow_soft_vc_emissive");
        let masked = source.technique == crc32(b"glow_vc_mask_emissive");
        let vertex_colour = soft || masked || source.technique == crc32(b"emissive_vc") || source.technique == crc32(b"emissive_vc_ssd");
        let ([mxx, mxy], [myx, myy]) = (pair("diffuse_texture2_x", [1.0, 0.0]), pair("diffuse_texture2_y", [0.0, 1.0]));
        let ([mzx, mzy], [mtx, mty]) = (pair("diffuse_texture2_z", [0.0; 2]), pair("diffuse_texture2_t", [0.0; 2]));
        let mut values = GlowValues {
            colour: diffuse_color,
            uv_xy: Vec4::new(xx, xy, yx, yy),
            uv_zt: Vec4::new(zx, zy, tx, ty),
            edge: source.vector("edge_color").map_or(Vec3::ZERO, |v| Vec3::new(v[0], v[1], v[2])).extend(number("edge_power", 1.0)),
            // [game 094420] Opacity and AlphaBoost multiply separately, before output saturation.
            falloff: Vec4::new(
                number("intransfalloff", 1.0),
                number("outtransfalloff", 1.0),
                number("diffuse_texture1_opacity", 1.0),
                crate::assets::alpha_boost(source),
            ),
            light: rig.map_or(LIGHT.extend(0.0), |r| r.sun_colour),
            flags: UVec4::new(
                (if add { ADD } else { 0 }) | (if edge { EDGE } else { 0 })
                    | (if vertex_colour { VERTEX_COLOUR } else { 0 })
                    | (if rig.is_some() && !soft && !masked && !barrier { GLOW_RIG } else { 0 })
                    | (if soft { SOFT } else { 0 })
                    | (if masked { 64 } else { 0 })
                    | (if barrier { BARRIER } else { 0 })
                    | (if cutout.is_some() { CUTOUT } else { 0 }), 0, 0, 0),
            fog_colour,
            fog_range,
            // y is ZFeather_Distance [game 00973060: the feather is on when the node's +0xf0 +0x78 > 0];
            // [data 2026-10-20, robots] `zfeather_distance` (474dba22) is absent from every material of the ten
            // robot packs (`work\mat_dump.py`, no line with the name or hash), so a robot's feather is always the
            // default here. [assumed] absent is 1 metre. OPEN: settles: the writer of that +0xf0 +0x78 float and its
            // default for an absent `zfeather_distance` (the hash 474dba22 has no immediate in the executable,
            // `work\find_imm.py 474dba22`; the 1 m default is not found in the code).
            // w: the material word `depthbias` (signed). [game 0459c0: emissive_vc's VS moves the view
            // position by DepthBias * 0.1] [game 0956a0: the barrier vertex shader does the same: its
            // `nrm r1` of the view position, `mul r1, r1, c21.x` (DepthBias), `mad r0, r1, c22.x, r0`
            // (c22 = 0.1), and the clip position `o0` is computed from that biased r0]. The port's glow.wgsl
            // applies the bias per pixel along the eye ray for depth only, not to the clip position:
            // a difference in glow.wgsl, not in this file.
            cutout: Vec4::new(cutout.unwrap_or(0.0), number("zfeather_distance", 1.0), 0.0,
                source.word("depthbias").unwrap_or(0) as i32 as f32),
            mask_uv_xy: Vec4::new(mxx, mxy, myx, myy),
            mask_uv_zt: Vec4::new(mzx, mzy, mtx, mty),
            dynamic: Vec4::ZERO,
        };
        if barrier {
            // [game 005a2af0 raw005a3100..3164] Names of these material
            // parameters are unresolved; raw hashes and defaults are verified.
            let raw_number = |hash, default| source.params.iter().find(|p| p.name == hash && p.owner == 0)
                .and_then(|p| match p.value { model::ParamValue::Number(v) => Some(v), _ => None }).unwrap_or(default);
            let width = raw_number(0x57fbe4bf,10.0);
            let offset = raw_number(0x7496a107,10.0);
            let m = -0.5 / width.max(1e-7);
            values.falloff.x = m;
            values.falloff.y = -(width+offset)*m;
            values.cutout.z = number("diffuse_texture2_opacity",1.0);
        }
        Some(Glow { values, diffuse, mask: None, alpha_mode, diffuse_color })
    }

    pub fn with_mask(mut self, mask: Option<Handle<Image>>) -> Self {
        self.mask = mask;
        self
    }
}

/// Whether a technique is one of the two, and if so whether it has the edge light and the
/// facing fade.
fn edge(technique: u32) -> Option<bool> {
    const EMISSIVE: u32 = crc32(b"emissive");
    const EMISSIVE_VC: u32 = crc32(b"emissive_vc");
    const EMISSIVE_VC_SSD: u32 = crc32(b"emissive_vc_ssd");
    const GLOW_SOFT_VC: u32 = crc32(b"glow_soft_vc_emissive");
    const DULL_FX: u32 = crc32(b"dull_fx_emissive");
    const GLOW_MASK: u32 = crc32(b"glow_vc_mask_emissive");
    const BARRIER_TECH: u32 = crc32(b"barrier");
    match technique {
        // [game 095210] With SSD mask zero, the remaining factor is1 and the
        // emissive/vertex-colour/fog terms are094420's. Damage sampling is still owed.
        EMISSIVE | EMISSIVE_VC | EMISSIVE_VC_SSD | BARRIER_TECH => Some(false),
        DULL_FX | GLOW_SOFT_VC | GLOW_MASK => Some(true),
        _ => None,
    }
}

/// A particle renderer that bends the picture behind it (`Dstr`, `IsShimmer`):
/// `warp.wgsl`.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct Warp {
    #[texture(0)]
    #[sampler(1)]
    pub map: Option<Handle<Image>>,
}

impl Material for Warp {
    fn fragment_shader() -> ShaderRef {
        WARP_SHADER.into()
    }

    /// Drawn after the solid scene with a copy of it at hand (Bevy's phase for glass).
    fn reads_view_transmission_texture(&self) -> bool {
        true
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    /// It hides nothing: what is drawn after it must not be cut out by its depth.
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

pub fn plugin(app: &mut App) {
    app.add_plugins((MaterialPlugin::<Fx>::default(), MaterialPlugin::<Glow>::default(), MaterialPlugin::<Warp>::default()));
    app.add_systems(Update, update_glow_dynamic.after(crate::player::tick));
    app.world_mut()
        .resource_mut::<Assets<Shader>>()
        .insert(&WARP_SHADER, Shader::from_wgsl(include_str!("warp.wgsl"), "warp.wgsl"))
        .expect("the distortion shader's handle");
    let mut shaders = app.world_mut().resource_mut::<Assets<Shader>>();
    shaders.insert(&FX_SHADER, Shader::from_wgsl(include_str!("fx.wgsl"), "fx.wgsl")).expect("the particle shader's handle");
    shaders.insert(&GLOW_SHADER, Shader::from_wgsl(include_str!("glow.wgsl"), "glow.wgsl")).expect("the glow shader's handle");
}

fn update_glow_dynamic(time: Res<Time>, player: Single<&crate::player::Player>, mut materials: ResMut<Assets<Glow>>, mut timed: Local<(usize, Vec<AssetId<Glow>>)>) {
    // [game 0077a4c0, 005a4f20] BarrierPlayerPos is character_camera_target_point:
    // root position for the ground character. Airborne flight target remains a debt.
    let at = crate::assets::game_to_bevy(player.state.pos);
    // Only the materials that read `dynamic` are written (as `surface::update_material_time`: `iter_mut` would
    // mark every Glow modified): barriers (the player's position, their second layer's scroll) and the ones whose
    // depth pass cuts at scrolling coordinates.
    if timed.0 != materials.len() {
        timed.1 = materials.iter().filter(|(_, m)| m.values.flags.x & BARRIER != 0
            || m.values.uv_zt.zw() != Vec2::ZERO || m.values.mask_uv_zt.zw() != Vec2::ZERO).map(|(id, _)| id).collect();
        timed.0 = materials.len();
        debug!("glow clock: {} of {} materials", timed.1.len(), materials.len());
    }
    let dynamic = Vec4::new(time.elapsed_secs_wrapped(),at.x,at.y,at.z);
    for id in &timed.1 {
        if let Some(mut material) = materials.get_mut(*id) && material.values.dynamic != dynamic { material.values.dynamic = dynamic; }
    }
}
