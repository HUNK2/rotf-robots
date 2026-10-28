//! The original's surface shaders for his materials: the car paint, the two metals, the
//! rubber and the glass. `surface.wgsl` has the arithmetic and what in it is the game's;
//! this builds a material from a pack's `MAT` entry.
//!
//! With `--mission` the light (sun colour and direction, ambient, the nine irradiance terms)
//! is the level's rig (`world::mission_rig`, the zone's own Environment). In the arena it
//! is the stand-in constants below. What the paint reflects is still a stand-in.

use bevy::asset::uuid_handle;
use bevy::prelude::*;
use bevy::mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::render::render_resource::{AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, VertexFormat};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::{Shader, ShaderRef};
use tf2_core::point_light::{self, LevelLight, LightKind};

use crate::assets::game_to_bevy;
use crate::formats::hash::crc32;
use crate::formats::model;

const SHADER: Handle<Shader> = uuid_handle!("6b1f0c52-3d0e-4c1b-9a53-2f6d1e7a90b4");
const DISTORTION_SHADER: Handle<Shader> = uuid_handle!("9d4e2b71-6c3a-4f8e-a5b2-0e7c1d93f6a4");

/// [game LS-37] The level's point lights, one storage buffer that every Surface material holds a clone of
/// (`Surface::lights`, binding 15). All lights are retained; binding16 schedules screen tiles.
/// Record 0 is a placeholder: the lights are records 1 to `arrayLength - 1`.
const LIGHTS: Handle<ShaderBuffer> = uuid_handle!("3f0c9a6e-7b21-4d5e-8a14-c2e9d0b7f153");
const LIGHT_TILES: Handle<ShaderBuffer> = uuid_handle!("607ff32b-34d4-4bfd-83d9-e997924b48f6");

/// The records kept at the end of `LIGHTS` for the lights that come and go (the weapons' muzzle
/// lights, `point_light::FlashLight`). A fixed count keeps the buffer's size, so Bevy writes the new
/// records into the same GPU buffer and the materials' bind groups stay valid. An unused record has
/// no reach and adds nothing (`point_fade`). The port's number: the game caps each light list at
/// 0x200 records (the unpublished notes LS35.2). 32 since r3-fx (was 8): the projectiles'
/// flight lights and the blasts' flashes ride here too (Ironhide's 2 x 5 laser burst alone makes ten
/// bullet lights and ten muzzle lights); `effects::run` keeps the 32 nearest the eye. [port]
pub const FLASH_SLOTS: usize = 32;

/// The lights that come and go this frame, in game space (the muzzle lights, written by
/// `effects::run`). The first `FLASH_SLOTS` are drawn.
#[derive(Resource, Default)]
pub struct DynamicLights(pub Vec<LevelLight>);

const PAINT: u32 = 1;
const EDGE: u32 = 2;
const REFLECT_CUBE: u32 = 4;
const REFLECT_SPHERE: u32 = 8;
const FALLOFF: u32 = 16;
const NORMAL_MAP: u32 = 32;
const VERTEX_COLOR: u32 = 64;
/// The specular term (`Specular_Color`, `Specular_Power`, the specular map). Set per technique
/// from its own shader: the cheap family has it, `dull` and `dull_vc` do not [game].
const SPECULAR: u32 = 128;
/// [data] `blendmode` Additive (0x1f4da15f, the unpublished notes section 1): the shader writes
/// the colour premultiplied by its alpha with alpha 0, because Bevy's `AlphaMode::Add` blend is
/// premultiplied (`bevy_pbr` 0.19.1 `render\mesh.rs`, BLEND_PREMULTIPLIED_ALPHA, ticket LS-18).
const ADD: u32 = 256;
/// [game 0382c0_vs, 00566e90] The level's light rig is in the uniform (`rig_*`, `irradiance`)
/// and replaces the stand-in sun colour, sun direction and ambient. Set only with `--mission`
/// (`world::mission_rig`); the arena keeps its stand-in values.
const RIG: u32 = 512;
/// [game 011790f0, 005a2af0] blendmode 0xbc07610c (the unnamed cut-out): the alpha is cut below
/// the threshold in `cutout.x` (`assets::cutout_threshold`), in the colour pass and the depth and
/// shadow prepass (`prepass_fragment_shader`). [assumed] the compare, see `surface.wgsl`. settles: the
/// variant pick for 0xbc07610c, as `assets.rs` `cutout_threshold` (the one home of that question; the unpublished notes Q2/Q3).
/// [data 2026-10-20, robots] Three robot materials carry 0xbc07610c (`ironhidevehicle_paintover.4x4_lod`, on
/// `dull`; `optimusflight` `robot3_s` on `glass_plus` and `robot3_slod` on `glass_cheap`), so the variant
/// question is a robot question too. [game 2026-10-20] the variant hashes (3661b05b GBufferAlphaTest, ee1f5b3c ZOnlyAlphaTest, ...) sit only in
/// the table at 00b8de50 (filled in `pending_manager_init` 00ae9980); no code refers to that table's
/// address (find_ptr), and 011790f0 (blend_mode_switch) sets the record's flag for 0xbc07610c (bc07610c
/// at 01179206). The table's reader IS 005a2af0, by index (`[ecx*8 + 0xb8de48]` at 005a3681, read
/// 2026-10-20): it builds all 24 variants of a material, so the AlphaTest one is picked at draw time
/// by its index (the draw's index for a flagged part is the open part, see `assets::cutout_threshold`).
const CUTOUT: u32 = 1024;
/// [game 08db60, 0b7ca0] `blend2_plus_mask_ssd`: two diffuse layers blended by a mask, two normal
/// maps blended in tangent space by the same weights, the mask's own uv set (LS-24,
/// the unpublished notes A.6). Its mesh parts carry three uv sets (`assets.rs`).
const BLEND2: u32 = 2048;
/// [game 0738c0:84-92] DIFFUSE2 (LS-41, `basic_cheap`; [game 06b4c0:94-99] `basic`): a second diffuse layer on
/// texture coordinate set 1 (TEXCOORD1), lerped over the first by its alpha times Diffuse_Texture2_Opacity
/// (the unpublished notes 2.2, the unpublished notes 3.4 and 3.5). Its mesh
/// parts carry uv set 1 (`assets.rs` `mesh`). Its shader is the plain Surface one: no own vertex stage.
const DIFFUSE2: u32 = 4096;
/// [game 0601e0:106, 0601e0:244] AMBIENT_BOOST (LS-41, `cheap_plus_boost`): Ambient_Boost is added to the
/// ambient term before the albedo multiply (the unpublished notes 6.1).
const AMBIENT_BOOST: u32 = 8192;
/// [game 031e50, 04dfc0] The dull_cheap/shiny families have no diffuse sampler.
const UNTEXTURED: u32 = 16384;
/// [game 050250] cheap_cheap has a uniform specular colour, without a specular sampler.
const UNIFORM_SPECULAR: u32 = 32768;
/// [game 084850, 085260, 087a50, 0881b0] COLOR0.rg is normalised layer weight.
const VERTEX_WEIGHTS: u32 = 65536;
const FLAT_BLEND_NORMAL: u32 = 131072;
/// [game 090410/090df0] Three diffuse layers, independent RGB mask on TEXCOORD3,
/// and one normal map on TEXCOORD0. Shares the layered rigid vertex pipeline.
const BLEND3: u32 = 262144;
const FLATCARD: u32 = 524288;
const UNLIT: u32 = 1048576;
/// [game 089a70:102-113] blend2 with one normal map, no Normal_MapB and no blend: N_ts = nA.
const SINGLE_NORMAL: u32 = 2097152;
/// [game 063a70:91-92] alpha = layer 2's red times Diffuse_Color.a (no opacity term); colour = layer 1 only.
const ALPHA_LAYER2: u32 = 4194304;
/// [game 0675a0_ps:189-200] cheap_plus_fx: the alpha fades with the view-space normal between `intransfalloff` and
/// `outtransfalloff` (`cutout.y`, `.z`).
const TRANS_FALLOFF: u32 = 8388608;

/// The vertex attribute of texture coordinate set 2 (the blend2 mask's uv, TEXCOORD2). Bevy's
/// own sets 0 and 1 are `ATTRIBUTE_UV_0` and `ATTRIBUTE_UV_1`. The id is a fixed number, chosen
/// here so that no other attribute uses it. Settled 2026-10-20 from bevy_mesh 0.19.1 `vertex.rs` l.20-22: the id
/// is any unique u64 ("use a random / very large u64 to avoid conflicts"); the built-in attributes take 0..7
/// (`mesh.rs` l.269-332), so these large fixed ids cannot collide with them.
pub const BLEND2_UV2: MeshVertexAttribute = MeshVertexAttribute::new("Blend2Uv2", 0x6b32_9d1f_c4a7_0e15, VertexFormat::Float32x2);
pub const BLEND3_UV3: MeshVertexAttribute = MeshVertexAttribute::new("Blend3Uv3", 0x6b32_9d1f_c4a7_0e16, VertexFormat::Float32x2);
pub const FLATCARD_ANCHOR: MeshVertexAttribute = MeshVertexAttribute::new("FlatcardAnchor", 0x6b32_9d1f_c4a7_0e17, VertexFormat::Float32x3);
pub const FLATCARD_NORMAL: MeshVertexAttribute = MeshVertexAttribute::new("FlatcardNormal", 0x6b32_9d1f_c4a7_0e18, VertexFormat::Float32x3);
/// The pack's own binormal stream (`dcl_binormal`, read by the game's 08d150_vs). blend2 only; a part
/// without one keeps the rebuild `cross(N, T) * w` (`VERTEX_BINORMAL` is off then). Location 12.
pub const BINORMAL: MeshVertexAttribute = MeshVertexAttribute::new("Binormal", 0x6b32_9d1f_c4a7_0e19, VertexFormat::Float32x3);
const BINORMAL_LOCATION: u32 = 12;
/// The shader location of `BLEND2_UV2` (the vertex input `uv2` in `surface.wgsl`'s `blend2_vertex`).
const BLEND2_UV2_LOCATION: u32 = 8;

/// [stand-in] the sun's colour, in the original's own scale (a lit texel is its value
/// times this plus the ambient, cut off at 1). The arena has no level, so the game has no
/// sun colour here; with `--mission` the level's LightColor replaces it [game]. settles: nothing
/// in the executable for the arena (a stage choice).
const LIGHT: Vec3 = Vec3::new(1.0, 0.97, 0.9);
/// [stand-in] the ambient on a surface facing up, and on one facing down. The arena has no game
/// ambient (a stage); with `--mission` the level's rig does (`RIG`). settles: nothing in the executable
/// for the arena.
const AMBIENT_SKY: Vec3 = Vec3::new(0.42, 0.46, 0.52);
const AMBIENT_GROUND: Vec3 = Vec3::new(0.26, 0.25, 0.23);
/// The id of his reflection cube map (`outputcube.dds`) in his pack.
const HIS_CUBE: u32 = 0x224f_6431;
/// [stand-in] the sky the paint reflects where that cube is not found: the arena's clear
/// colour. settles: the RealtimeSphereMap pass of a level (not read; see `surface.wgsl`).
const SKY_SEEN: Vec3 = Vec3::new(0.55, 0.68, 0.82);

#[derive(Clone, ShaderType)]
pub struct SurfaceValues {
    diffuse_color: Vec4,
    specular: Vec4,
    reflection: Vec4,
    edge: Vec4,
    paint0: Vec4,
    paint1: Vec4,
    paint2: Vec4,
    light: Vec4,
    sky: Vec4,
    ground: Vec4,
    sky_seen: Vec4,
    uv_x: Vec4,
    uv_y: Vec4,
    uv_z: Vec4,
    uv_t: Vec4,
    // The normal map's and the specular map's own transforms (each texture has its own in
    // the original's vertex shader).
    normal_x: Vec4,
    normal_y: Vec4,
    normal_z: Vec4,
    normal_t: Vec4,
    specular_x: Vec4,
    specular_y: Vec4,
    specular_z: Vec4,
    specular_t: Vec4,
    // The level's light rig (flag RIG): rgb LightColor; xyz LightDir in Bevy's space (the way
    // to the sun); rgb AmbientColor; and AmbientIrradiance's nine float4 in the game's order
    // (x^2, y^2, z^2, xy, yz, zx, x, y, z), evaluated on the game-space normal [game 0382c0_vs].
    rig_sun: Vec4,
    rig_dir: Vec4,
    rig_ambient: Vec4,
    irradiance: [Vec4; 9],
    // x: the cut-out threshold (flag CUTOUT); the rest unused
    cutout: Vec4,
    flags: UVec4,
    // blend2 only (flag BLEND2): the second diffuse layer's transform (its own uv set, TEXCOORD1),
    // the second normal map's transform (TEXCOORD1), and `blend`: x normal A is DXT5 (A,G),
    // y normal B is DXT5, z normal A's height scale, w normal B's height scale.
    diffuse2_x: Vec4,
    diffuse2_y: Vec4,
    diffuse2_z: Vec4,
    diffuse2_t: Vec4,
    normal_b_x: Vec4,
    normal_b_y: Vec4,
    normal_b_z: Vec4,
    normal_b_t: Vec4,
    blend: Vec4,
    // The level's fog (flag RIG only) [game 05cb30_ps:257-265, 038900_ps:203-212, 0ab3b0_ps:321-329]:
    // fog_color rgb FogColor, w its falloff (FogColor.w); fog_range FogRange (x near, y slope,
    // z unused, w height). Filled from `LightRig::fog_color` / `fog_range` in `build_with`.
    fog_color: Vec4,
    fog_range: Vec4,
    // DIFFUSE2 (LS-41): x Diffuse_Texture2_Opacity. AMBIENT_BOOST (LS-41): rgb Ambient_Boost.
    layer2: Vec4,
    ambient_boost: Vec4,
    diffuse3_x: Vec4,
    diffuse3_y: Vec4,
    diffuse3_z: Vec4,
    diffuse3_t: Vec4,
    motion: Vec4,
    // [game] the material word `depthbias` (signed) in x; applied as a depth shift in the colour pass
    // (`SURFACE_DEPTH_BIAS`, `surface.wgsl`).
    depth_bias: Vec4,
}

/// What a material's pipeline depends on beyond its bindings: blend2 changes the vertex stage,
/// its inputs and the shader's defines (`specialize`); DIFFUSE2 (LS-41) needs uv set 1 as an input.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceKey {
    blend2: bool,
    /// LS-41: a second diffuse layer on uv set 1 (flag DIFFUSE2); `specialize` adds that input.
    layer2: bool,
    /// LS-34: the material's cullmode is None, so both faces are drawn (`specialize`).
    double_sided: bool,
    flatcard: bool,
    /// The material's `depthbias` word is not 0: the colour pass writes a shifted depth.
    depth_bias: bool,
}

/// `--no-depth-bias`: a comparison switch that leaves DepthBias out (captures before / after).
fn no_depth_bias() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::args().any(|a| a == "--no-depth-bias"))
}

impl From<&Surface> for SurfaceKey {
    fn from(surface: &Surface) -> Self {
        SurfaceKey {
            blend2: surface.values.flags.x & BLEND2 != 0,
            layer2: surface.values.flags.x & DIFFUSE2 != 0,
            double_sided: surface.double_sided,
            flatcard: surface.values.flags.x & FLATCARD != 0,
            depth_bias: surface.values.depth_bias.x != 0.0 && !no_depth_bias(),
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(SurfaceKey)]
pub struct Surface {
    #[uniform(0)]
    values: SurfaceValues,
    #[texture(1)]
    #[sampler(2)]
    diffuse: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    normal: Option<Handle<Image>>,
    #[texture(5)]
    #[sampler(6)]
    specular: Option<Handle<Image>>,
    #[texture(7, dimension = "cube")]
    #[sampler(8)]
    reflection: Option<Handle<Image>>,
    // blend2 and DIFFUSE2 (LS-41): the second diffuse layer; blend2 only: the mask (`diffuse_mix_texture`) and
    // the second normal map.
    #[texture(9)]
    #[sampler(10)]
    diffuse2: Option<Handle<Image>>,
    #[texture(11)]
    #[sampler(12)]
    mask: Option<Handle<Image>>,
    #[texture(13)]
    #[sampler(14)]
    normal_b: Option<Handle<Image>>,
    /// LS-37 (flag RIG): the level's point lights, the shared buffer `LIGHTS` (binding 15).
    #[storage(15, read_only)]
    lights: Handle<ShaderBuffer>,
    #[storage(16, read_only)]
    light_tiles: Handle<ShaderBuffer>,
    #[texture(17)]
    #[sampler(18)]
    diffuse3: Option<Handle<Image>>,
    /// The level's ambient occlusion (`ssao.rs`, the game's DownsampledSSAOMap), shared by every Surface.
    #[texture(19)]
    #[sampler(20)]
    ssao: Option<Handle<Image>>,
    alpha_mode: AlphaMode,
    /// LS-34 [data]: cullmode word 0xdfa2aff1 (CRC-32 of "None", the unpublished notes
    /// section 2) draws both faces; anything else keeps the back-face cull.
    double_sided: bool,
}

impl Material for Surface {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    /// The depth and shadow prepass of a cut-out part (`AlphaMode::Mask`) uses this shader's
    /// `PREPASS_PIPELINE` entry to cut the same pixels as the colour pass (`surface.wgsl`).
    /// Other parts never reach it: their prepass has no fragment stage.
    fn prepass_fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    /// Bevy's depth prepass draws both faces of a custom material's mesh while the main
    /// pass draws the front ones: the back of a panel then left its depth with no colour,
    /// a hole in the sky's colour over whatever was behind (the "bits of blue" the user
    /// saw). Both passes drop the back faces now, unless the material's cullmode is None (LS-34,
    /// `double_sided`). [assumed: the original does not draw his panels from behind either; the
    /// back-face cull is kept for every cullmode but None]
    /// blend2 (flag BLEND2, LS-24) also changes the vertex stage: its own entry `blend2_vertex`
    /// (`surface.wgsl`), which takes the mask's uv set (`BLEND2_UV2`) as one more vertex input.
    /// Flatcards likewise use their anchor/motion vertex stage in colour, depth and shadows.
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // LS-34 [data]: a material whose cullmode is None (`double_sided`) draws both faces; the rest
        // keep the back-face cull. Captures: `ls34_ustest_cullnone.png` shows the powerline wires
        // that the back-face cull hid in `ls34_ustest_base.png`.
        descriptor.primitive.cull_mode = if key.bind_group_data.double_sided { None } else { Some(Face::Back) };
        let prepass = descriptor.vertex.shader_defs.iter().any(|d| *d == "PREPASS_PIPELINE".into());
        // [game] DepthBias (decal note 7.5): the depth, GBuffer and Base vertex shaders move the view
        // position by normalize(view) * DepthBias * 0.1; ShadowGen does not. Here only the colour pass
        // shifts, by writing the shifted depth in the fragment stage (`SURFACE_DEPTH_BIAS`), so shadow
        // views are untouched. [stand-in] the depth prepass keeps the unshifted depth: the colour pass's
        // GreaterEqual test still lets the decal win over its surface; early depth is lost for these parts.
        // [data 2026-10-20, robots] All 1,131 `depthbias` words in the 27 `characters\*.str` packs are 0, so no
        // robot part takes this shift or this stand-in: it is a level-only debt.
        // settles: the game's depth vertex shader moves the position too (this note, decal 7.5); the port's
        // fix is to write the shifted depth in the prepass fragment (`frag_depth`, as `glow.wgsl` does), and
        // that is not done: no capture has shown the early-depth loss, and the prepass also serves the shadow passes.
        if key.bind_group_data.depth_bias && !prepass
            && let Some(fragment) = descriptor.fragment.as_mut() { fragment.shader_defs.push("SURFACE_DEPTH_BIAS".into()); }
        if !descriptor.vertex.shader_defs.contains(&"VERTEX_OUTPUT_INSTANCE_INDEX".into()) {
            descriptor.vertex.shader_defs.push("VERTEX_OUTPUT_INSTANCE_INDEX".into());
        }
        if let Some(fragment) = descriptor.fragment.as_mut()
            && !fragment.shader_defs.contains(&"VERTEX_OUTPUT_INSTANCE_INDEX".into()) { fragment.shader_defs.push("VERTEX_OUTPUT_INSTANCE_INDEX".into()); }
        if key.bind_group_data.blend2 || key.bind_group_data.layer2 || key.bind_group_data.flatcard {
            // Custom vertex stages share one input layout in every pass. DIFFUSE2
            // keeps Bevy's vertex stage and its pass-specific UV locations.
            let wanted: Vec<(MeshVertexAttribute, u32)> = if key.bind_group_data.flatcard {
                vec![(Mesh::ATTRIBUTE_POSITION, 0), (Mesh::ATTRIBUTE_UV_0, 2),
                    (Mesh::ATTRIBUTE_COLOR, 5), (FLATCARD_ANCHOR, 10), (FLATCARD_NORMAL, 11)]
            } else if key.bind_group_data.blend2 {
                vec![
                    (Mesh::ATTRIBUTE_POSITION, 0),
                    (Mesh::ATTRIBUTE_NORMAL, 1),
                    (Mesh::ATTRIBUTE_UV_0, 2),
                    (Mesh::ATTRIBUTE_UV_1, 3),
                    (Mesh::ATTRIBUTE_TANGENT, 4),
                    (Mesh::ATTRIBUTE_COLOR, 5),
                    (BLEND2_UV2, BLEND2_UV2_LOCATION),
                    (BLEND3_UV3, 9),
                    (BINORMAL, BINORMAL_LOCATION),
                ]
            } else {
                vec![(Mesh::ATTRIBUTE_UV_1, if prepass { 2 } else { 3 })]
            };
            if key.bind_group_data.blend2 || key.bind_group_data.flatcard {
                // A custom vertex entry uses its own locations in every pass. Bevy's
                // prepass UV/normal/colour locations differ from its forward layout.
                let attributes: Vec<_> = wanted.into_iter().filter(|(a, _)| layout.0.contains(*a))
                    .map(|(a, location)| a.at_shader_location(location)).collect();
                descriptor.vertex.buffers = vec![layout.0.get_layout(&attributes)?];
                descriptor.vertex.shader_defs.push("CUSTOM_SURFACE_VERTEX".into());
                if let Some(fragment) = descriptor.fragment.as_mut() { fragment.shader_defs.push("CUSTOM_SURFACE_VERTEX".into()); }
            } else {
                for (attribute, location) in wanted {
                    if !layout.0.contains(attribute) {
                        continue;
                    }
                    if let Some(buffer) = descriptor.vertex.buffers.first_mut()
                        && !buffer.attributes.iter().any(|a| a.shader_location == location) {
                        let extra = layout.0.get_layout(&[attribute.at_shader_location(location)])?;
                        buffer.attributes.extend(extra.attributes);
                    }
                }
            }
        }
        if key.bind_group_data.blend2 {
            descriptor.vertex.shader = SHADER;
            descriptor.vertex.entry_point = Some("blend2_vertex".into());
            descriptor.vertex.shader_defs.push("BLEND2".into());
            if layout.0.contains(BINORMAL) {
                // The part has the game's binormal stream: the shader uses it instead of the rebuild.
                descriptor.vertex.shader_defs.push("VERTEX_BINORMAL".into());
                if let Some(fragment) = descriptor.fragment.as_mut() { fragment.shader_defs.push("VERTEX_BINORMAL".into()); }
            }
            if let Some(fragment) = descriptor.fragment.as_mut() {
                fragment.shader_defs.push("BLEND2".into());
            }
        }
        if key.bind_group_data.flatcard {
            descriptor.vertex.shader = SHADER;
            descriptor.vertex.entry_point = Some("flatcard_vertex".into());
            descriptor.vertex.shader_defs.push("FLATCARD".into());
            if let Some(fragment) = descriptor.fragment.as_mut() { fragment.shader_defs.push("FLATCARD".into()); }
        }
        Ok(())
    }
}

/// The original's `distortion_vc_emissive` (LS-47, `distortion.wgsl`): the robots' invulnerability eggs
/// and the other sphere effects. Its uniform block is the listing's constants, packed in vec4s.
#[derive(Clone, ShaderType)]
pub struct DistortionValues {
    normal_xy: Vec4,
    normal_zt: Vec4,
    mask_xy: Vec4,
    mask_zt: Vec4,
    colour: Vec4,
    params: Vec4,
    falloff: Vec4,
    fog_colour: Vec4,
    fog_range: Vec4,
}

/// A distortion sphere. It bends the picture behind it, so it reads the view's transmission copy
/// (as `Warp`). Drawn on both faces, without depth writes.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct Distortion {
    #[uniform(0)]
    values: DistortionValues,
    #[texture(1)]
    #[sampler(2)]
    normal_map: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    mask: Option<Handle<Image>>,
    alpha_mode: AlphaMode,
}

impl Distortion {
    /// [stand-in] Held by value as the ocean is: its mask is not animated by IFL frames and it takes no
    /// LOD fade (`assets.rs` has no arm for it). [data 2026-10-20, robots] The egg masks are single pictures:
    /// the 66 materials name four `.dds` files (gradient_ramp_z, radial_glow, fx_smokecloud07, radial_noise),
    /// none an ifl list. settles (LOD fade only): the game's LOD fade for a distortion part, not read.
    /// The material of a `distortion_vc_emissive` part, from its own parameters (the values are the
    /// same for every robot material that uses it, [data 2026-10-20]: DepthBias 0, Normal blend or
    /// Additive, `Distortion_Distance` 1 or 5 or -1, `Normal_Map_HeightScale` 0.4, 1 or 5).
    pub fn build(source: &model::Material, normal_map: Option<Handle<Image>>, mask: Option<Handle<Image>>, alpha_mode: AlphaMode) -> Self {
        let pair = |name: &str, or: [f32; 2]| source.vector(name).map_or(or, |v| [v[0], v[1]]);
        let ([xx, xy], [yx, yy]) = (pair("normal_map_x", [1.0, 0.0]), pair("normal_map_y", [0.0, 1.0]));
        let ([zx, zy], [tx, ty]) = (pair("normal_map_z", [0.0; 2]), pair("normal_map_t", [0.0; 2]));
        let ([mxx, mxy], [myx, myy]) = (pair("diffuse_texture2_x", [1.0, 0.0]), pair("diffuse_texture2_y", [0.0, 1.0]));
        let ([mzx, mzy], [mtx, mty]) = (pair("diffuse_texture2_z", [0.0; 2]), pair("diffuse_texture2_t", [0.0; 2]));
        let number = |name: &str, or: f32| source.number(name).unwrap_or(or);
        // [data 2026-10-20, robots] All 66 distortion materials carry intransfalloff, outtransfalloff,
        // distortion_distance and normal_map_heightscale, so the absent defaults are never read for a robot.
        let dxt5 = if source.word("normal_map_dxt5_nm").unwrap_or(0) != 0 { 1.0 } else { 0.0 };
        let diffuse_color = source.vector("diffuse_color").map_or(Vec4::ONE, Vec4::from);
        let rig = crate::world::mission_rig();
        let (fog_colour, fog_range) = rig.map_or((Vec4::ZERO, Vec4::ZERO), |r| (r.fog_color(), r.fog_range()));
        Distortion {
            values: DistortionValues {
                normal_xy: Vec4::new(xx, xy, yx, yy),
                normal_zt: Vec4::new(zx, zy, tx, ty),
                mask_xy: Vec4::new(mxx, mxy, myx, myy),
                mask_zt: Vec4::new(mzx, mzy, mtx, mty),
                colour: diffuse_color,
                params: Vec4::new(number("distortion_distance", 1.0), number("normal_map_heightscale", 1.0), dxt5, crate::assets::alpha_boost(source)),
                falloff: Vec4::new(number("intransfalloff", 1.0), number("outtransfalloff", 1.0), 0.0, 0.0),
                fog_colour,
                fog_range,
            },
            normal_map,
            mask,
            alpha_mode,
        }
    }

    /// Multiplies the object's colour (`ObjectDiffuseColor`) in, as the Glow path does.
    pub fn tint(&mut self, rgb: Vec3) {
        self.values.colour *= rgb.extend(1.0);
    }
}

impl Material for Distortion {
    fn fragment_shader() -> ShaderRef {
        DISTORTION_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    /// Drawn after the solid scene with a copy of it at hand (`Warp`).
    fn reads_view_transmission_texture(&self) -> bool {
        true
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    /// The pack's cullmode is `CW` on all 44 of these additive parts [data], the word 92.7% of every
    /// material carries, which the `Surface` pipeline above draws with the back-face cull (LS-34). The
    /// same here (2026-10-20; before: both faces). The word's D3DCULL mapping is the shared one of
    /// the unpublished notes section 2. Not seen in a capture: none spawns these effects.
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        descriptor.primitive.cull_mode = Some(Face::Back);
        Ok(())
    }
}

pub fn plugin(app: &mut App) {
    app.add_plugins((MaterialPlugin::<Surface>::default(), MaterialPlugin::<Distortion>::default()));
    app.add_systems(Update, update_material_time);
    let shader = Shader::from_wgsl(include_str!("surface.wgsl"), "surface.wgsl");
    app.world_mut().resource_mut::<Assets<Shader>>().insert(&SHADER, shader).expect("the surface shader's handle");
    let distortion = Shader::from_wgsl(include_str!("distortion.wgsl"), "distortion.wgsl");
    app.world_mut().resource_mut::<Assets<Shader>>().insert(&DISTORTION_SHADER, distortion).expect("the distortion shader's handle");
    // [game LS-37] One buffer for all the Surface materials: a placeholder record, then the level's lights in
    // Bevy's space. The arena (no `--mission`) has no lights, so the buffer holds the placeholder only.
    let mut records = vec![GpuLight::default()];
    records.extend(crate::world::mission_lights().iter().map(gpu_light));
    // Then the empty records of the lights that come and go (`FLASH_SLOTS`).
    records.extend(std::iter::repeat_n(GpuLight::default(), FLASH_SLOTS));
    app.init_resource::<DynamicLights>();
    app.add_systems(Update, upload_dynamic_lights.after(crate::effects::run));
    app.world_mut()
        .resource_mut::<Assets<ShaderBuffer>>()
        .insert(&LIGHTS, ShaderBuffer::from(records))
        .expect("the point lights' buffer handle");
    app.world_mut().resource_mut::<Assets<ShaderBuffer>>()
        .insert(&LIGHT_TILES, ShaderBuffer::from(vec![0u32; tf2_core::light_tiles::HEADER]))
        .expect("the point lights' tile buffer handle");
    if !std::env::args().any(|a| a == "--no-light-tiles") {
        app.add_systems(Update, update_light_tiles.after(crate::camera::apply).after(crate::effects::run));
    }
}

/// The records of the lights that come and go, `FLASH_SLOTS` of them (an empty one has no reach).
fn flash_records(dynamic: &DynamicLights) -> Vec<GpuLight> {
    let mut slots = vec![GpuLight::default(); FLASH_SLOTS];
    for (slot, light) in slots.iter_mut().zip(&dynamic.0) {
        *slot = gpu_light(light);
    }
    slots
}

/// Writes the lights that come and go into the last `FLASH_SLOTS` records of `LIGHTS`, when they change.
fn upload_dynamic_lights(dynamic: Res<DynamicLights>, mut buffers: ResMut<Assets<ShaderBuffer>>, mut last: Local<Vec<GpuLight>>, mut level: Local<Option<Vec<GpuLight>>>) {
    let slots = flash_records(&dynamic);
    if *last == slots || (last.is_empty() && dynamic.0.is_empty()) {
        return;
    }
    let level = level.get_or_insert_with(|| crate::world::mission_lights().iter().map(gpu_light).collect());
    let mut records = Vec::with_capacity(1 + level.len() + FLASH_SLOTS);
    records.push(GpuLight::default());
    records.extend(level.iter().copied());
    records.extend(slots.iter().copied());
    if let Some(mut buffer) = buffers.get_mut(&LIGHTS) {
        buffer.set_data(records);
    }
    *last = slots;
}

// Prepass/shadow view layouts have no globals binding. Keep the same clock in the
// material so wind moves identically in the colour, depth and shadow vertex stages.
// Only the materials that read the clock are written: `Assets::iter_mut` marks EVERY material modified, and
// re-preparing all of a level's materials each frame cost two thirds of the frame (2026-10-16, `--fps`).
fn update_material_time(time: Res<Time>, mut materials: ResMut<Assets<Surface>>, mut timed: Local<(usize, Vec<AssetId<Surface>>)>) {
    if timed.0 != materials.len() {
        timed.1 = materials.iter().filter(|(_, m)| m.reads_clock()).map(|(id, _)| id).collect();
        debug!("surface clock: {} of {} materials", timed.1.len(), materials.len());
        timed.0 = materials.len();
    }
    let now = time.elapsed_secs_wrapped();
    for id in &timed.1 {
        if let Some(mut material) = materials.get_mut(*id) { material.values.motion.w = now; }
    }
}

impl Surface {
    /// Whether a shader stage reads `motion.w` (the material's clock): a flatcard's wind, or a cut-out's depth pass
    /// cutting at scrolling texture coordinates. The colour pass uses Bevy's own clock.
    fn reads_clock(&self) -> bool {
        let v = &self.values;
        let flags = v.flags.x;
        let wind = flags & FLATCARD != 0 && v.motion.x != 0.0 && v.motion.y != 0.0;
        let scrolls = [v.uv_t, v.diffuse2_t, v.diffuse3_t].iter().any(|t| *t != Vec4::ZERO);
        wind || (flags & CUTOUT != 0 && scrolls)
    }
    /// The placed Instance's Colorizer colour times Diffuse_Color (`tf2_core::colorizer`). [game 0382c0_vs:72,
    /// 08d150_vs:108, 044aa0_vs:73] the vertex shaders multiply Diffuse_Color by ObjectDiffuseColor (c23, LS-56):
    /// this is that product, so every technique that reads `diffuse_color` (blend2 included) carries it. [assumed]
    /// the Colorizer colour is what the game uploads as c23 for a placed Instance. [game 2026-10-20] the upload
    /// is read: `00580660` (plain_shader_bind_lights) calls `00550c50` with the crc of "objectdiffusecolor"
    /// (d5fc1313, at 005808dd) and the 16 bytes at esi, which is `[node+0x14] + 0x14` (005806bc), the float4 of
    /// the material record (the unpublished notes H1). [assumed] that float4 is the Colorizer's
    /// colour: its writer (material+0x14) is NOT found; the Colorizer route to it is open (H1, H2).
    /// settles: the writer of material+0x14 in the Instance's draw record (not located).
    pub fn tint(&mut self, rgb: Vec3) { self.values.diffuse_color *= rgb.extend(1.0); }
    pub fn set_diffuse(&mut self, image: Handle<Image>) { self.diffuse = Some(image); }
    pub fn set_secondary(&mut self, image: Handle<Image>) { self.diffuse2 = Some(image); }
    /// Adds `rgb` to the material's ambient through its Ambient_Boost term (flag AMBIENT_BOOST).
    /// The port's fill for the walking giant (`giant.rs`) [assumed port design].
    pub fn lift_ambient(&mut self, rgb: Vec3) {
        self.values.flags.x |= AMBIENT_BOOST;
        self.values.ambient_boost += rgb.extend(0.0);
    }
    pub fn enable_lod_fade(&mut self) {
        // A fragment stage must run in depth/shadow passes even on solid map pieces.
        if self.alpha_mode == AlphaMode::Opaque { self.alpha_mode = AlphaMode::Mask(0.0); }
    }
}

/// The port's conservative light scheduling: all lights stay available and each 32px tile
/// gets every light volume that can project into it. No global or per-tile light cap.
fn update_light_tiles(
    camera: Query<(&Camera, &Transform, &Projection), (With<Camera3d>, Without<crate::ocean::OceanReflection>, Without<crate::duel::SecondView>)>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut previous: Local<Option<Vec<u32>>>,
    dynamic: Res<DynamicLights>,
) {
    let lights = crate::world::mission_lights();
    // The arena has no level lights and no tiles: the shader then walks every record (the few
    // `FLASH_SLOTS`).
    if lights.is_empty() { return; }
    let Some((camera, transform, projection)) = camera.iter().next() else { return };
    let Some(rect) = camera.physical_viewport_rect() else { return };
    let reach_of = |light: &LevelLight| match light.kind { LightKind::Capsule => light.range.max(light.range2) + light.offset_len, _ => light.range };
    let mut bounds: Vec<_> = lights.iter().map(|light| (game_to_bevy(light.position), reach_of(light))).collect();
    // The records after the level's: the lights that come and go, an empty slot with no reach (no tile).
    bounds.extend((0..FLASH_SLOTS).map(|i| dynamic.0.get(i).map_or((Vec3::ZERO, 0.0), |light| (game_to_bevy(light.position), reach_of(light)))));
    let clip_from_world = projection.get_clip_from_view() * transform.to_matrix().inverse();
    let words = tf2_core::light_tiles::build(&bounds, clip_from_world, rect.size(), rect.min);
    if previous.as_ref() == Some(&words) { return; }
    if let Some(mut buffer) = buffers.get_mut(&LIGHT_TILES) {
        if previous.is_none() {
            eprintln!("level lights: all {} placed lights available through {}x{} uncapped screen tiles", lights.len(), words[0], words[1]);
        }
        buffer.set_data(words.clone());
        *previous = Some(words);
    }
}

/// [game LS-37, point_light.rs] One point light as the shader reads it, in Bevy's space (`game_to_bevy` is a
/// rotation: distances and dot products are the game's). `place.w` is the kind (0 omni, 1 spot, 2 capsule).
/// The colours are BEFORE the far fade: the shader applies `point_light::far_fade` from `origin` and `origin.w`
/// (the reach) to the eye, as `shader_light` does per camera.
#[derive(Clone, Copy, Default, PartialEq, ShaderType)]
pub struct GpuLight {
    /// xyz: the record's position (omni, spot: the Light; capsule: Pc = position - |offset| axis). w: kind.
    place: Vec4,
    /// xyz: the spot axis (c10, as uploaded) or the capsule's segment A (c10). w: capsule 1 / |A|^2 (c10.w).
    axis: Vec4,
    /// rgb: the colour (omni c10, spot c11, capsule c11 at u = 0).
    colour: Vec4,
    /// rgb: the capsule's colour at u = 1 (c12). Zero for the others.
    colour2: Vec4,
    /// Omni c11 (inner^2, outer^2, 1/(outer^2 - inner^2)), spot c12 (cos inner, cos outer, inner^2, outer^2),
    /// capsule c13 (inner0, outer0, inner1, outer1), as `point_light`'s records.
    params: Vec4,
    /// xyz: the Light's own position (the far fade's distance is from here to the eye). w: the far fade's reach
    /// (`range`; the capsule's `max(range, range2) + |offset|`).
    origin: Vec4,
}

/// [game LS-37] A light's record in Bevy's space, from the core's data (`point_light::shader_light` does the
/// far fade per eye, in the shader here).
fn gpu_light(light: &LevelLight) -> GpuLight {
    let bevy = |v: Vec3| game_to_bevy(v).extend(0.0);
    let origin = game_to_bevy(light.position).extend(0.0);
    match light.kind {
        LightKind::Omni => GpuLight {
            place: origin.with_w(0.0),
            colour: light.colour.extend(0.0),
            params: point_light::omni_params(light.attenuate, light.range),
            origin: origin.with_w(light.range),
            ..default()
        },
        LightKind::Spot => GpuLight {
            place: origin.with_w(1.0),
            axis: bevy(light.direction),
            colour: light.colour.extend(0.0),
            params: point_light::spot_params(light.hotspot, light.falloff, light.attenuate, light.range),
            origin: origin.with_w(light.range),
            ..default()
        },
        LightKind::Capsule => {
            let (pc, a, w10) = point_light::capsule_segment(light.position, light.capsule_axis, light.offset_len);
            GpuLight {
                place: game_to_bevy(pc).extend(2.0),
                axis: bevy(a).with_w(w10),
                colour: light.colour.extend(0.0),
                colour2: light.colour2.extend(0.0),
                params: point_light::capsule_params(light.attenuate, light.range, light.attenuate2, light.range2),
                origin: origin.with_w(light.range.max(light.range2) + light.offset_len),
            }
        }
    }
}


/// What a technique of the original's does, or `None` for one that is not ported.
fn flags(technique: u32) -> Option<u32> {
    const PAINT_X: u32 = crc32(b"aniso_paint_x");
    const PAINT_CUBE: u32 = crc32(b"aniso_paint");
    const GLASS_PLUS: u32 = crc32(b"glass_plus");
    const GLASS_PLUS_X: u32 = crc32(b"glass_plus_x");
    const CHEAP_PLUS: u32 = crc32(b"cheap_plus");
    const CHEAP_PLUS_FX: u32 = crc32(b"cheap_plus_fx");
    const GLASS_CHEAP: u32 = crc32(b"glass_cheap");
    const DULL_VC: u32 = crc32(b"dull_vc");
    const DULL: u32 = crc32(b"dull");
    const CHEAP: u32 = crc32(b"cheap");
    const BASIC: u32 = crc32(b"basic");
    const CHEAP_SSD: u32 = crc32(b"cheap_ssd");
    const BASIC_SSD: u32 = crc32(b"basic_ssd");
    const CHEAP_PLUS_SSD: u32 = crc32(b"cheap_plus_ssd");
    const BLEND2_PLUS_MASK_SSD: u32 = crc32(b"blend2_plus_mask_ssd");
    const BLEND2_PLUS_MASK: u32 = crc32(b"blend2_plus_mask");
    const BLEND3_MASK: u32 = crc32(b"blend3_mask");
    const BLEND3_MASK_SSD: u32 = crc32(b"blend3_mask_ssd");
    const BLEND2_PLUS_VERTEX_SSD: u32 = crc32(b"blend2_plus_vertex_ssd");
    const BLEND2_PLUS_VERTEX: u32 = crc32(b"blend2_plus_vertex");
    const BLEND2_VERTEX_NONORMAL: u32 = crc32(b"blend2_vertex_nonormal");
    const BLEND2_VERTEX_NONORMAL_SSD: u32 = crc32(b"blend2_vertex_nonormal_ssd");
    const BASIC_CHEAP: u32 = crc32(b"basic_cheap");
    const BASIC_CHEAP_SSD: u32 = crc32(b"basic_cheap_ssd");
    const DULL_SSD: u32 = crc32(b"dull_ssd");
    const GLASS_PLUS_SSD: u32 = crc32(b"glass_plus_ssd");
    const GLASS_CHEAP_SSD: u32 = crc32(b"glass_cheap_ssd");
    const CHEAP_PLUS_BOOST: u32 = crc32(b"cheap_plus_boost");
    const CHEAP_VC: u32 = crc32(b"cheap_vc");
    const CHEAP_VC_SSD: u32 = crc32(b"cheap_vc_ssd");
    const CHEAP_PLUS_VC: u32 = crc32(b"cheap_plus_vc");
    const CHEAP_PLUS_VC_SSD: u32 = crc32(b"cheap_plus_vc_ssd");
    const BASIC_CHEAP_VC: u32 = crc32(b"basic_cheap_vc");
    const BASIC_CHEAP_VC_SSD: u32 = crc32(b"basic_cheap_vc_ssd");
    const GLASS_PLUS_VC: u32 = crc32(b"glass_plus_vc");
    const CHEAP_CHEAP: u32 = crc32(b"cheap_cheap");
    const CHEAP_CHEAP_SSD: u32 = crc32(b"cheap_cheap_ssd");
    const DULL_CHEAP: u32 = crc32(b"dull_cheap");
    const DULL_CHEAP_SSD: u32 = crc32(b"dull_cheap_ssd");
    const DULL_CHEAP_VC: u32 = crc32(b"dull_cheap_vc");
    const SHINY: u32 = crc32(b"shiny");
    const SHINY_CHEAP: u32 = crc32(b"shiny_cheap");
    const FLATCARD_CHEAP: u32 = crc32(b"flatcard_cheap");
    const FLATCARD_BASIC: u32 = crc32(b"flatcard");
    const FLATCARD_PLUS: u32 = crc32(b"flatcard_plus");
    const FLATCARD_EMISSIVE: u32 = crc32(b"flatcard_emissive");
    // [game 053a70, 096f50, 055320] the decal family (LS-54, the unpublished notes 7.2).
    // Base PS 053a70 (decal, decal_vc) and 096f50 (decal_reflective) are the lit forms: their
    // light-buffer reads are the port's light sum [stand-in, decal note 7.3].
    const DECAL: u32 = crc32(b"decal");
    const DECAL_VC: u32 = crc32(b"decal_vc");
    const DECAL_REFLECTIVE: u32 = crc32(b"decal_reflective");
    const DECAL_SSD: u32 = crc32(b"decal_ssd");
    const DECAL_VC_SSD: u32 = crc32(b"decal_vc_ssd");
    const BLEND2_MASK_SSD: u32 = crc32(b"blend2_mask_ssd");
    const CHEAP_PLUS_MASK_SSD: u32 = crc32(b"cheap_plus_mask_ssd");
    const BLEND3_VERTEX_SSD: u32 = crc32(b"blend3_vertex_ssd");
    // Phase 1 audit (the unpublished notes 3): the techniques
    // that were drawn by the StandardMaterial fallback outside Shanghai and us_test.
    const CHEAP_BOOST: u32 = crc32(b"cheap_boost");
    const BLEND2_VERTEX: u32 = crc32(b"blend2_vertex");
    const BLEND2_VERTEX_SSD: u32 = crc32(b"blend2_vertex_ssd");
    const BLEND3_VERTEX: u32 = crc32(b"blend3_vertex");
    const CHEAP_MASK: u32 = crc32(b"cheap_mask");
    const DULL_MASK: u32 = crc32(b"dull_mask");
    match technique {
        // [game 08db60, 0b7ca0] blend2_plus_mask_ssd: the specular term is Specular_Color times
        // the blended diffuse alpha (no specular map). Its normal maps and layers are BLEND2's.
        BLEND2_PLUS_MASK_SSD | BLEND2_PLUS_MASK => Some(BLEND2 | SPECULAR),
        BLEND3_MASK | BLEND3_MASK_SSD => Some(BLEND2 | BLEND3 | NORMAL_MAP | SPECULAR),
        BLEND2_PLUS_VERTEX_SSD | BLEND2_PLUS_VERTEX => Some(BLEND2 | SPECULAR | VERTEX_WEIGHTS),
        BLEND2_VERTEX_NONORMAL | BLEND2_VERTEX_NONORMAL_SSD => Some(BLEND2 | SPECULAR | VERTEX_WEIGHTS | FLAT_BLEND_NORMAL),
        // The five the port had; each has the specular term (its shader has it: `specular`
        // in the original's pixel shader, every one of them but dull_vc).
        PAINT_X => Some(PAINT | EDGE | REFLECT_SPHERE | FALLOFF | NORMAL_MAP | SPECULAR),
        PAINT_CUBE => Some(PAINT | REFLECT_CUBE | FALLOFF | NORMAL_MAP | SPECULAR),
        GLASS_PLUS => Some(REFLECT_CUBE | NORMAL_MAP | SPECULAR),
        // [game 09b330] glass_plus_x (Base and Skin; VS 05c290 / 05edf0, glass_plus's): glass_plus's
        // 09a0d0 with the paint's edge light after the lights and its reflection of RealtimeSphereMap
        // in view space times the falloff (0ab3b0:287-314), in place of the cube map. The main metal and
        // paint of every robot but Bumblebee and Starscream (`robot_material_audit`); it was drawn by the
        // StandardMaterial fallback until 2026-10-19. The sphere map takes the paint's stand-in (`build`).
        GLASS_PLUS_X => Some(EDGE | REFLECT_SPHERE | FALLOFF | NORMAL_MAP | SPECULAR),
        CHEAP_PLUS => Some(NORMAL_MAP | SPECULAR),
        // [game] cheap_plus_fx (the front end's globe and cloud layers; technique 206fbe0a): its Base pixel shader
        // 0675a0 is cheap_plus's (05cb30) with the edge light added after the lights (as the paint's: Edge_Color,
        // Edge_Power) and the alpha falloff by the view-space normal. Its Deferred shader (0688d0) is not read.
        CHEAP_PLUS_FX => Some(NORMAL_MAP | SPECULAR | EDGE | TRANS_FALLOFF),
        GLASS_CHEAP => Some(REFLECT_CUBE | SPECULAR),
        DULL_VC => Some(VERTEX_COLOR),
        // [game] dull: 038900 with the 0382c0 vertex shader. Diffuse only, no vertex colour.
        DULL => Some(0),
        // [game] cheap: 052560, cheap_plus without the normal map (the geometric normal).
        CHEAP => Some(SPECULAR),
        // [game 06b4c0] basic: cheap_plus with a second diffuse layer on uv set 1 (LS-41; the second
        // layer was a stand-in until then). Its normal map and specular are cheap_plus's.
        BASIC => Some(NORMAL_MAP | SPECULAR | DIFFUSE2),
        // [game] the `_ssd` forms: their Base shaders are the same ids as the non-ssd forms
        // (ls-4 section 1, [data]), so the same flags. [game lsa_list 2026-10-20] The Base pixel shaders
        // of these families (dull 038900, cheap 052560, cheap_plus 05cb30, cheap_cheap 050250, basic_cheap
        // 0738c0, basic 06b4c0 / 070fc0, glass 098a20 / 09a0d0) have no damage sampler (`Cracks_Map_Texture`,
        // `Damage_Map_Texture`, `SSDMap` appear only in the listings of 03d120, 0729c0, 055320 and others),
        // so the Base path does not touch the damage maps. decal_ssd's Base pixel shader (055320) does
        // read them: see the DECAL_SSD entry.
        CHEAP_SSD => Some(SPECULAR),
        BASIC_SSD => Some(NORMAL_MAP | SPECULAR | DIFFUSE2),
        CHEAP_PLUS_SSD => Some(NORMAL_MAP | SPECULAR),
        // [game 0738c0] basic_cheap: cheap (no normal map) with the second diffuse layer (LS-41). Base VS 0730a0,
        // PS 0738c0; `_ssd` Base ids the same (the unpublished notes 1, table rows 1 and 3, [data]).
        BASIC_CHEAP | BASIC_CHEAP_SSD => Some(SPECULAR | DIFFUSE2),
        // [game, Base ids confirmed by lsa_list: 0382c0 / 038900 = dull] dull_ssd: the same flags as dull.
        DULL_SSD => Some(0),
        // [game, Base ids confirmed by lsa_list: 05c290 / 09a0d0 = glass_plus] glass_plus_ssd: as glass_plus.
        GLASS_PLUS_SSD => Some(REFLECT_CUBE | NORMAL_MAP | SPECULAR),
        // [game, Base ids confirmed by lsa_list: 051e20 / 098a20 = glass_cheap] glass_cheap_ssd: as glass_cheap.
        GLASS_CHEAP_SSD => Some(REFLECT_CUBE | SPECULAR),
        // [game 0601e0] cheap_plus_boost: cheap_plus with Ambient_Boost added to the ambient (LS-41). Base VS 05c290
        // (cheap_plus's), PS 0601e0 (the unpublished notes 6). Its shadow, read 2026-10-20 (wave 27
        // O5): 0601e0_ps samples ShadowMap1 on s1 only (no ShadowMap0): four PCF taps through mShadowTrapezoid1
        // (c8-c11, offsets c42 / c38 scaled by c30.w), each compared with the receiver depth, averaged (0.25),
        // and faded by ShadowBlendFilterParams c30.z (S = shadow * fade + 1 - fade). [game] [stand-in: Bevy's
        // sun shadow is used, as for every technique] settles: a port of the trapezoid map and its filter,
        // and the engine-set values of c30.
        CHEAP_PLUS_BOOST => Some(NORMAL_MAP | SPECULAR | AMBIENT_BOOST),
        // [game/data] Identical Base pixel shaders; each vc vertex shader multiplies the
        // material/object tint by COLOR0. See the unpublished notes.
        CHEAP_VC | CHEAP_VC_SSD => Some(SPECULAR | VERTEX_COLOR),
        CHEAP_PLUS_VC | CHEAP_PLUS_VC_SSD => Some(NORMAL_MAP | SPECULAR | VERTEX_COLOR),
        BASIC_CHEAP_VC | BASIC_CHEAP_VC_SSD => Some(SPECULAR | DIFFUSE2 | VERTEX_COLOR),
        GLASS_PLUS_VC => Some(REFLECT_CUBE | NORMAL_MAP | SPECULAR | VERTEX_COLOR),
        CHEAP_CHEAP | CHEAP_CHEAP_SSD => Some(SPECULAR | UNIFORM_SPECULAR),
        DULL_CHEAP | DULL_CHEAP_SSD => Some(UNTEXTURED),
        DULL_CHEAP_VC => Some(UNTEXTURED | VERTEX_COLOR),
        SHINY => Some(UNTEXTURED | SPECULAR),
        SHINY_CHEAP => Some(UNTEXTURED | SPECULAR | UNIFORM_SPECULAR),
        // [game 0ad330/0ad870, 0b2300/0b2a20, 0b1340/094420] The flatcard
        // vertex stages face the view around TEXCOORD1 anchors and apply authored wind.
        FLATCARD_CHEAP | FLATCARD_BASIC => Some(FLATCARD | VERTEX_COLOR | AMBIENT_BOOST),
        FLATCARD_PLUS => Some(FLATCARD | VERTEX_COLOR | AMBIENT_BOOST | NORMAL_MAP),
        FLATCARD_EMISSIVE => Some(FLATCARD | VERTEX_COLOR | UNLIT),
        DECAL => Some(SPECULAR),
        DECAL_VC => Some(SPECULAR | VERTEX_COLOR),
        DECAL_REFLECTIVE => Some(REFLECT_CUBE | SPECULAR),
        // [stand-in] decal_ssd / decal_vc_ssd: the damage mix (cracks, damage and the screen SSD map,
        // TEXCOORD6) is NOT ported (destruction is out of scope, LS-54). Damage m is 1 here, so
        // the colour is the undamaged decal. [data 2026-10-20, robots] No robot part is decal_ssd or
        // decal_vc_ssd (the 3 robot decal parts are optimusflight's, none `_ssd`): a level-only debt. [game lsa_list 2026-10-20] decal_ssd's Base pass is vs 054c00 and
        // ps 055320 (its entry has no Deferred variant). 055320 is deferred-lit: it samples SSDMap (s0) at the
        // screen position, DiffuseLightingBuffer (s1) and SpecularLightingBuffer (s2), the cracks (s5) and the
        // damage map (s6, `texld_pp r3, v2.zwzw, s6`), and mixes diffuse * vertex colour with the damage map by
        // the crack term. The port lights the decal as a Surface instead (no lighting buffers, no damage mix):
        // that is the stand-in. settles: porting the damage mix and the screen SSD buffer (`0059f970`) and the
        // lighting buffers the Base pass needs (the deferred pass's output, not read here).
        DECAL_SSD => Some(SPECULAR),
        DECAL_VC_SSD => Some(SPECULAR | VERTEX_COLOR),
        // [game 089180 / 089a70] blend2_mask_ssd: blend2_plus_mask_ssd with ONE normal map, no blend of
        // two (the unpublished notes 1). [game lsa_list 2026-10-20] its Base pixel shader 089a70
        // has no damage sampler, so there is no _ssd damage term in the Base path to stand in for.
        BLEND2_MASK_SSD => Some(BLEND2 | SPECULAR | SINGLE_NORMAL),
        // [game 0630f0 / 063a70] cheap_plus_mask_ssd: cheap_plus colour from layer 1, alpha from layer 2's
        // red (mask-blend note 2). DIFFUSE2 binds layer 2 and uv set 1; ALPHA_LAYER2 replaces its mix.
        CHEAP_PLUS_MASK_SSD => Some(NORMAL_MAP | SPECULAR | DIFFUSE2 | ALPHA_LAYER2),
        // [game 07a6e0 / 07b0d0] blend3_vertex_ssd: three layers weighted by COLOR0.rgb over R+G+B, one
        // normal map, specular from the blended alpha (mask-blend note 3).
        // [data] blend3_vertex has the same Base shaders as blend3_vertex_ssd (VS 07a6e0, PS 07b0d0; lsa_list).
        BLEND3_VERTEX_SSD | BLEND3_VERTEX => Some(BLEND2 | BLEND3 | VERTEX_WEIGHTS | SPECULAR),
        // [game 055970 / VS 051e20] cheap_boost: PS 055970 is cheap_plus_boost's 0601e0 without the normal map
        // (register renumbering aside, the listings match), on cheap's vertex shader. Its shadow is 0601e0's
        // exactly: ShadowMap1 on s1, four PCF taps, averaged, faded by c30.z (055970_ps lines 103-121; wave 27
        // O5) [game]. [stand-in: Bevy's sun shadow, as `cheap_plus_boost` above] settles: the same port.
        CHEAP_BOOST => Some(SPECULAR | AMBIENT_BOOST),
        // [game 081790 / VS 080ea0, both techniques] blend2_vertex(_ssd): blend2_plus_vertex's 085260 with ONE
        // normal map (Normal_Map on TEXCOORD6.xy) in place of the weighted pair; the rest matches once registers
        // are renumbered. [game lsa_list 2026-10-20] Its Base pixel shader 081790 has no damage sampler: the
        // _ssd form has no damage term in the Base path.
        BLEND2_VERTEX | BLEND2_VERTEX_SSD => Some(BLEND2 | SPECULAR | VERTEX_WEIGHTS | SINGLE_NORMAL),
        // [game 059700 / VS 058ee0] cheap_mask: cheap's 052560 with alpha := layer 2's red (`texld_pp r1, v2.zw, s1;
        // mov_pp r0.w, r1.x`, layer 2 on TEXCOORD1 with its own transform) and no Diffuse_Texture1_Opacity term:
        // cheap_plus_mask_ssd's rule without the normal map.
        CHEAP_MASK => Some(SPECULAR | DIFFUSE2 | ALPHA_LAYER2),
        // [game 03dd70 / VS 03d640] dull_mask: dull's 038900 with the same alpha from layer 2's red.
        DULL_MASK => Some(DIFFUSE2 | ALPHA_LAYER2),
        _ => None,
    }
}

/// COLOR0 is a tint only for these families. Other families use it as layer weights.
pub fn uses_vertex_colour(technique: u32) -> bool {
    flags(technique).is_some_and(|f| f & (VERTEX_COLOR | VERTEX_WEIGHTS) != 0)
}

pub fn is_flatcard(technique: u32) -> bool { flags(technique).is_some_and(|f| f & FLATCARD != 0) }

/// The ids of the textures a material of a ported technique wants: colour, normal,
/// specular and reflection.
pub fn textures(source: &model::Material) -> Option<[Option<u32>; 4]> {
    flags(source.technique)?;
    Some([
        source.texture("diffuse_texture1_texture"),
        source.texture("normal_map_texture"),
        source.texture("specular_map_texture"),
        // [stand-in] `aniso_paint_x` and `glass_plus_x` reflect `RealtimeSphereMap`, the level drawn as the
        // game runs. There is no level here: his own cube map (`outputcube.dds`, which his
        // other materials reflect) stands for it. In another pack the id finds nothing
        // and the flat sky and ground below are used.
        source.texture("reflectionmap_texture").or((flags(source.technique)? & REFLECT_SPHERE != 0).then_some(HIS_CUBE)),
    ])
}

/// One texture's UV transform, `X*u + Y*v + Z + T*time`, from its material parameters
/// `<prefix>_x`..`_t`: [game] every Base vertex shader applies it per texture (`0382c0:66-70`,
/// `05c290:87-91` diffuse, `05c290:117-124` normal and specular). An absent transform is the
/// identity, and so is one whose X and Y are both zero [assumed] (level-only: no robot pair is zero, see below): a zero-scaled transform
/// would send every UV to Z. [data 2026-10-20] The count over every pack under the install (82,297 present
/// `*_x`/`*_y` pairs): 235 are zero in both X and Y, all `normal_map` and all in `mission.str` (121) and
/// `zone.str` (114); none in `characters\*.str`, so the robots never reach this case. Rerun 2026-10-20 over
/// `characters\*.str` (scratch example, 2,022 prefixed `_x`/`_y` pairs): 0 both zero, and 0 normal maps with
/// either missing, so the robot side is settled [data]. The game's
/// reading of a zero normal-map transform is still not read (the upload, see below), so the level
/// cases stay [assumed]. settles: the shader constant upload for normal_map's X/Y (not read), or a
/// level part with a zero normal map on a recording. [game 2026-10-20] the
/// material builder `005a2af0` does not name these parameters (its body has the hashes of alphaboost,
/// alphathreshold, cullmode and depthbias, not of any `*_x`, texture or motion name), so the absent default
/// is set in the shader-constant upload, which is not read (level-lighting-setters.md: the upload is by CTAB).
fn uv_transform(source: &model::Material, prefix: &str) -> [Vec4; 4] {
    let get = |suffix: &str, or: Vec4| source.vector(&format!("{prefix}_{suffix}")).map_or(or, Vec4::from);
    let (x, y) = (get("x", Vec4::X), get("y", Vec4::Y));
    if x.xy() == Vec2::ZERO && y.xy() == Vec2::ZERO {
        return [Vec4::X, Vec4::Y, Vec4::ZERO, Vec4::ZERO];
    }
    [x, y, get("z", Vec4::ZERO), get("t", Vec4::ZERO)]
}

/// The second layers of a material with layers (LS-24 blend2, LS-41 DIFFUSE2), as texture ids:
/// `diffuse_texture2_texture`, and for blend2 also `diffuse_mix_texture` (the mask, TEXCOORD2) and
/// `normal_mapb_texture`. `None` for any other technique.
pub fn layer_textures(source: &model::Material) -> Option<[Option<u32>; 4]> {
    if is_blend2(source.technique) {
        return Some([source.texture("diffuse_texture2_texture"), source.texture("diffuse_mix_texture"), source.texture("normal_mapb_texture"), source.texture("diffuse_texture3_texture")]);
    }
    has_diffuse2(source.technique).then(|| [source.texture("diffuse_texture2_texture"), None, None, None])
}

/// Whether a technique is `blend2_plus_mask_ssd` (LS-24): its material needs `layer_textures`
/// and the mesh's second and third uv sets.
pub fn is_blend2(technique: u32) -> bool {
    flags(technique).is_some_and(|f| f & BLEND2 != 0)
}

/// Whether a technique has a second diffuse layer on uv set 1 (LS-41, flag DIFFUSE2): `basic_cheap`,
/// `basic` and their `_ssd` forms. Its mesh parts carry that uv set (`assets.rs`).
pub fn has_diffuse2(technique: u32) -> bool {
    matches!(flags(technique), Some(f) if f & DIFFUSE2 != 0)
}

/// The material for a pack's `MAT` entry, its textures already made (in the order of
/// `textures`), or `None` if its technique is not one of the ported ones. A technique with
/// layers (`layer_textures`) needs `build_layered`: without its layers it is not built here.
pub fn build(source: &model::Material, images: [Option<Handle<Image>>; 4], alpha_mode: AlphaMode) -> Option<Surface> {
    if is_blend2(source.technique) || has_diffuse2(source.technique) {
        return None;
    }
    build_with(source, images, [None, None, None, None], alpha_mode)
}

/// The material of a technique with layers (blend2 and DIFFUSE2): `images` as for `build`
/// (`textures`), `layers` as `layer_textures`. [LS-24] A layer the pack does not give is not replaced
/// by a fallback picture: a blend2 layer is sampled as Bevy's fallback [stand-in]; a DIFFUSE2 layer
/// with no texture is not drawn at all [assumed, LS-41: the flag is cleared, as a missing normal map is].
/// [data 2026-10-20, robots] No robot part takes this path: no `characters\*.str` part has a blend2 or DIFFUSE2
/// technique (scratch count). The robot parts with `diffuse_texture2_texture` are the 66 distortion_vc_emissive
/// (undrawn) and the 25 glow_vc_mask_emissive (Glow path); all 91 of those layer entries carry a TEX, so the
/// empty-layer path is not taken by a robot either. Levels keep the stand-in and [assumed].
/// settles: what the game samples when a layer's texture is absent. [game 2026-10-20] `005a2af0` has no
/// texture-name hash for the layers (diffuse_texture2_texture fc5f1695 and the others absent), so the empty
/// slot is handled by the texture-bind path (not read; `005a2020` is the sampler setter only).
pub fn build_layered(
    source: &model::Material,
    images: [Option<Handle<Image>>; 4],
    layers: [Option<Handle<Image>>; 4],
    alpha_mode: AlphaMode,
) -> Option<Surface> {
    if !is_blend2(source.technique) && !has_diffuse2(source.technique) {
        return None;
    }
    build_with(source, images, layers, alpha_mode)
}

fn build_with(
    source: &model::Material,
    images: [Option<Handle<Image>>; 4],
    layers: [Option<Handle<Image>>; 4],
    alpha_mode: AlphaMode,
) -> Option<Surface> {
    let mut flags = flags(source.technique)?;
    if alpha_mode == AlphaMode::Add {
        flags |= ADD;
    }
    let cutout = match alpha_mode {
        AlphaMode::Mask(threshold) => {
            flags |= CUTOUT;
            threshold
        }
        _ => 0.0,
    };
    // The level's rig, when a mission is loaded (`world::mission_rig`, from the zone's own
    // Environment object). The arena has none and keeps the stand-in values below.
    let rig = crate::world::mission_rig();
    if rig.is_some() {
        flags |= RIG;
    }
    // [game] the fog of the level's rig (LS-29): FogColor and FogRange, zero in the arena (the
    // shader only reads them with flag RIG).
    let (fog_color, fog_range) = match rig {
        Some(rig) => (rig.fog_color(), rig.fog_range()),
        None => (Vec4::ZERO, Vec4::ZERO),
    };
    let (rig_sun, rig_dir, rig_ambient, irradiance) = match rig {
        Some(rig) => (
            rig.sun_colour,
            // [game] the rig's LightDir is the game's space (x right, y forward, z up); the
            // shader works in Bevy's, so it is turned here (`assets::game_to_bevy`).
            crate::assets::game_to_bevy(rig.sun_dir).extend(0.0),
            // w: SSAOFinalScale [game 007650 c2; data `ssaoFinalScale`], 0 when the SSAO flag is off or `--no-ssao`.
            rig.ambient_colour.truncate().extend(crate::ssao::final_scale()),
            rig.irradiance.k,
        ),
        None => (Vec4::ZERO, Vec4::ZERO, Vec4::ZERO, [Vec4::ZERO; 9]),
    };
    let [diffuse, normal, specular, reflection] = images;
    let [diffuse2, mask, normal_b, diffuse3] = layers;
    if normal.is_none() {
        flags &= !NORMAL_MAP;
    }
    // [assumed, LS-41] a DIFFUSE2 technique whose pack gives no second layer draws its first one alone
    // (level-only: every robot DIFFUSE2 entry carries its layer's TEX, see `build_layered` [data 2026-10-20]).
    if diffuse2.is_none() {
        flags &= !DIFFUSE2;
    }
    if reflection.is_none() {
        flags &= !REFLECT_CUBE;
    } else if flags & REFLECT_SPHERE != 0 {
        // The stand-in for the sphere map (see `textures`).
        flags = flags & !REFLECT_SPHERE | REFLECT_CUBE;
    }
    let colour = |name: &str, or: f32| source.vector(name).map_or(Vec3::splat(or), |v| Vec3::new(v[0], v[1], v[2]));
    let number = |name: &str, or: f32| source.number(name).unwrap_or(or);
    let diffuse_uv = uv_transform(source, "diffuse_texture1");
    let normal_uv = uv_transform(source, "normal_map");
    let specular_uv = uv_transform(source, "specular_map");
    let diffuse2_uv = uv_transform(source, "diffuse_texture2");
    let diffuse3_uv = uv_transform(source, "diffuse_texture3");
    let normal_b_uv = uv_transform(source, "normal_mapb");
    // blend2 [game 08db60:106-128]: a normal map that is not there is flat (its height scale 0
    // gives the normal (0, 0, 1)) [assumed: the original's behaviour for a missing map is not read].
    // [game 2026-10-20, 08db60_ps.asm:101-124] The shader does r3.xy = sampled.xy * c32.x (and c34.x for the second
    // map), so a zero height scale gives a flat normal whatever the texel is. What is still open is the value of
    // c32 / c34 when the material has no normal map (the constant's initial value: the name binders write nothing
    // for an absent name, see the DXT5 note). OPEN, same question as RD9f96e5 below.
    // DXT5 flags as the shader reads them, (A,G) when set; an absent flag is 0 [assumed]. settles: the
    // default for an absent `*_DXT5_NM` word. [game 2026-10-20] `005a2af0` does not name it (hash
    // 246a1514 / 588a3537 absent from its body). The name binders of the shader constants (`00550c50`,
    // `00548990`, and `hash_table_find` 0053a140) return 0 and write NOTHING when the name is not in the
    // shader's table ("found" is `index < count`): so an absent name leaves the register at its last value,
    // not at zero. The port's 0 is therefore a guess. [data 2026-10-20] Robots: 10 materials have a
    // normal_map_texture and no DXT5 word, all Optimus (`optimus` and `optimusgold`: `optimusv_trans_v2r`
    // truckbase, mirrormetal, bluepaint, redpaint on aniso_paint; tires on cheap_plus). So the default
    // decides those 10 robot normal maps: OPEN. settles: the material's own parameter upload (the caller
    // of those binders with the material's names, not located), or a capture of an Optimus truck part.
    let dxt5 = |name: &str| if source.word(name).unwrap_or(0) != 0 { 1.0 } else { 0.0 };
    let normal_height = |present: bool, name: &str| if present { number(name, 1.0) } else { 0.0 };
    let values = SurfaceValues {
        diffuse_color: source.vector("diffuse_color").map_or(Vec4::ONE, Vec4::from),
        specular: colour("specular_color", 0.0).extend(number("specular_power", 1.0)),
        reflection: colour("reflection_color", 0.0).extend(number("reflection_falloff", 1.0)),
        edge: colour("edge_color", 0.0).extend(number("edge_power", 1.0)),
        paint0: colour("paint_color0", 0.0).extend(number("paint_pow1", 1.0)),
        paint1: colour("paint_color1", 0.0).extend(number("paint_pow2", 1.0)),
        paint2: colour("paint_color2", 0.0).extend(number("normal_map_heightscale", 1.0)),
        light: LIGHT.extend(number("diffuse_texture1_opacity", 1.0)),
        sky: AMBIENT_SKY.extend(number("reflection_falloffpower", 1.0)),
        // w: AlphaBoost, [game 005a3087] 255 / max(X,1) for blendmode 0x3467929c, else 1.
        depth_bias: Vec4::new(source.word("depthbias").unwrap_or(0) as i32 as f32, 0.0, 0.0, 0.0),
        ground: AMBIENT_GROUND.extend(crate::assets::alpha_boost(source)),
        sky_seen: SKY_SEEN.extend(0.0),
        uv_x: diffuse_uv[0],
        uv_y: diffuse_uv[1],
        uv_z: diffuse_uv[2],
        uv_t: diffuse_uv[3],
        normal_x: normal_uv[0],
        normal_y: normal_uv[1],
        normal_z: normal_uv[2],
        normal_t: normal_uv[3],
        specular_x: specular_uv[0],
        specular_y: specular_uv[1],
        specular_z: specular_uv[2],
        specular_t: specular_uv[3],
        rig_sun,
        rig_dir,
        rig_ambient,
        irradiance,
        cutout: Vec4::new(cutout, number("intransfalloff", 1.0), number("outtransfalloff", 1.0), 0.0),
        flags: UVec4::new(flags, 0, 0, 0),
        diffuse2_x: diffuse2_uv[0],
        diffuse2_y: diffuse2_uv[1],
        diffuse2_z: diffuse2_uv[2],
        diffuse2_t: diffuse2_uv[3],
        normal_b_x: normal_b_uv[0],
        normal_b_y: normal_b_uv[1],
        normal_b_z: normal_b_uv[2],
        normal_b_t: normal_b_uv[3],
        blend: Vec4::new(
            dxt5("normal_map_dxt5_nm"),
            dxt5("normal_mapb_dxt5_nm"),
            normal_height(normal.is_some(), "normal_map_heightscale"),
            normal_height(normal_b.is_some(), "normal_mapb_heightscale"),
        ),
        fog_color,
        fog_range,
        // Diffuse_Texture2_Opacity: [data 2026-10-20, robots] absent in all 24 robot materials that carry a
        // diffuse_texture2_texture (the energon, egg, overdrive and radial-attack effects in each robot pack), so
        // a robot's layer 2 never takes the material's value and the 1 here is the default alone
        // Read 2026-10-20: the basic_cheap PS list (0738c0, the unpublished notes section 2) gives no `def` for c32 (its only def is c38), and the name's hash 3dc47681 is no immediate in the executable (work\find_imm.py), so the binder is a table walk not read.
        // [assumed: 1 is what the game's constant holds for an absent name. OPEN: settles: the binder that fills a material's constants (the table walk that reads diffuse_texture2_opacity, not found by its hash) and the constant block's initial value]. Ambient_Boost absent: 0 [assumed, LS-41].
        layer2: Vec4::new(number("diffuse_texture2_opacity", 1.0), 0.0, 0.0, 0.0),
        ambient_boost: colour("ambient_boost", 0.0).extend(0.0),
        diffuse3_x: diffuse3_uv[0],
        diffuse3_y: diffuse3_uv[1],
        diffuse3_z: diffuse3_uv[2],
        diffuse3_t: diffuse3_uv[3],
        // Absent motion parameters are zero. [data 2026-10-20, robots] Only the 25 flatcard_emissive materials
        // carry motion words (all three, `energon.glow_flatcard` in each robot pack); every other robot material
        // lacks them. The wind reads them only on a flatcard (see `wind` above), and every robot flatcard has all
        // three, so the default never reaches a robot. Levels are
        // not counted: [assumed] there. [game 2026-10-20] `005a2af0` does not name them (hashes e7f8ed0c,
        // 6e6ea977, f446c6b5 absent from its body); the name binders write nothing for an absent name (see the
        // DXT5 note), so for levels an absent motion would keep the constant's last value.
        motion: Vec4::new(number("motion_speed", 0.0), number("motion_strength", 0.0), number("motion_distort", 0.0), 0.0),
    };
    // LS-34 [data]: cullmode None, CRC-32 0xdfa2aff1 (the unpublished notes section 2).
    let double_sided = source.word("cullmode") == Some(crc32(b"None"));
    Some(Surface { values, diffuse, normal, specular, reflection, diffuse2, mask, normal_b, diffuse3, lights: LIGHTS, light_tiles: LIGHT_TILES, ssao: Some(crate::ssao::MAP), alpha_mode, double_sided })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The phase 1 audit's techniques (the unpublished notes 3): each maps onto the shader whose listing it
    /// shares, as read in `work\shaders\asm` (see `flags`).
    #[test]
    fn phase1_audit_techniques_map_to_their_shared_shaders() {
        let f = |name: &[u8]| flags(crc32(name));
        assert_eq!(f(b"cheap_boost"), Some(SPECULAR | AMBIENT_BOOST));
        assert_eq!(f(b"cheap_boost").map(|x| x | NORMAL_MAP), f(b"cheap_plus_boost"));
        assert_eq!(f(b"blend2_vertex"), Some(BLEND2 | SPECULAR | VERTEX_WEIGHTS | SINGLE_NORMAL));
        assert_eq!(f(b"blend2_vertex_ssd"), f(b"blend2_vertex"));
        assert_eq!(f(b"blend3_vertex"), f(b"blend3_vertex_ssd"));
        assert_eq!(f(b"cheap_mask"), Some(SPECULAR | DIFFUSE2 | ALPHA_LAYER2));
        assert_eq!(f(b"dull_mask"), Some(DIFFUSE2 | ALPHA_LAYER2));
        // ocean_water2 is not a Surface: it has its own material (`ocean.rs`).
        assert!(f(b"ocean_water2").is_none());
        assert!(crate::ocean::is_ocean(crc32(b"ocean_water2")));
    }

    /// [game 09b330 against 09a0d0 and 0ab3b0] glass_plus_x is glass_plus with the paint's edge light and
    /// sphere-map reflection with falloff in place of the cube map; no paint colour ramp.
    #[test]
    fn glass_plus_x_is_glass_plus_with_the_paints_edge_and_sphere_reflection() {
        let f = |name: &[u8]| flags(crc32(name)).unwrap();
        let x = f(b"glass_plus_x");
        assert_eq!(x, f(b"glass_plus") & !REFLECT_CUBE | EDGE | REFLECT_SPHERE | FALLOFF);
        assert_eq!(x, f(b"aniso_paint_x") & !PAINT);
    }
}
