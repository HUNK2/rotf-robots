//! The original's sky and backdrop shaders: `sky` (0b7390/0b7530), `sky_vc` (0b76d0/0b7890)
//! and `bgnd_vc` (0b7a60/0b7ca0). `sky.wgsl` has the
//! arithmetic and what in it is the game's. Level notes: the unpublished notes, LS-25 section.
//!
//! The sky models are the level's domes and cylinders (35 km across). They are drawn where they
//! are placed, not moved with the camera, and at the far depth so that everything else covers them.
//! [stand-in] IrisParams (exposure) is 1; SpinnyThing is not turned. The alpha is the bloom texture's
//! green times AlphaBoost, as the game's pixel shader writes it (LS26).

use bevy::asset::uuid_handle;
use bevy::prelude::*;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::render::render_resource::{AsBindGroup, CompareFunction, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::{Shader, ShaderRef};

use crate::formats::hash::crc32;
use crate::formats::model;

const SKY_SHADER: Handle<Shader> = uuid_handle!("9c4e2a71-5b3d-4f08-a6e2-d1b7c03f5e94");

/// `sky` (062674ef) and `sky_vc` (0e9ff099) [data: the unpublished notes rows 48 and 69].
const SKY: u32 = crc32(b"sky");
const SKY_VC: u32 = crc32(b"sky_vc");
/// [game 0b7a60, 0b7ca0] Backdrop cards share the far-depth vertex projection; their colour
/// is the texture times vertex colour, with the level's distance/height fog.
const BGND_VC: u32 = crc32(b"bgnd_vc");
const BACKDROP: u32 = 2;
const ADD: u32 = 4;

/// The `sky_vc` flag: the colour is multiplied by the vertex colour (`sky.wgsl`).
const VERTEX_COLOUR: u32 = 1;

/// [stand-in] `IrisParams.x`, the exposure the pixel shader multiplies the texture by
/// [game 0b7530 `mul_pp oC0.xyz, r0, c0.x`]. The iris is not ported, so 1.
const EXPOSURE: f32 = 1.0;

/// [game 005a3087, LS-21 the unpublished notes line 471] AlphaBoost is 1.0 for every blend
/// but 0x3467929c, so for 0xd6300862 (the sky's blend) it is 1. The sky shader's c1 is taken to be
/// that record [assumed: the sky material goes through the same pass builder].
const ALPHA_BOOST: f32 = 1.0;

#[derive(Clone, ShaderType)]
pub struct SkyValues {
    /// x: the exposure (`EXPOSURE`), [stand-in]
    exposure: Vec4,
    /// x: `VERTEX_COLOUR` for `sky_vc`
    flags: UVec4,
    /// x: AlphaBoost (`ALPHA_BOOST`), multiplies the bloom texture's green (LS26)
    boost: Vec4,
    fog_colour: Vec4,
    fog_range: Vec4,
}

/// A sky model's material: its diffuse texture, lit by nothing, at the far depth.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(SkyKey)]
pub struct Sky {
    #[uniform(0)]
    values: SkyValues,
    #[texture(1)]
    #[sampler(2)]
    diffuse: Option<Handle<Image>>,
    /// `bloom_texture` (LS26): the game's pixel shader takes the sky's alpha from its green channel
    /// [game 0b7530:159-160]. Without it the alpha is 1 and the horizon-light cylinder is opaque.
    #[texture(3)]
    #[sampler(4)]
    bloom: Option<Handle<Image>>,
    alpha_mode: AlphaMode,
    double_sided: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SkyKey { double_sided: bool }

impl From<&Sky> for SkyKey {
    fn from(sky: &Sky) -> Self { Self { double_sided: sky.double_sided } }
}

impl Material for Sky {
    fn vertex_shader() -> ShaderRef {
        SKY_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SKY_SHADER.into()
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

    /// No depth write, and the depth test is the far one: Bevy's depth is reversed (the far plane
    /// is 0 and the clear value), so the sky at clip z 0 passes against an empty pixel and fails
    /// against anything nearer. [game 0b7390:138-145, clip z = w * K; K = far, assumed]
    /// Back faces are dropped as the other ported techniques do [assumed: the cull state of the
    /// original's pass is not read].
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = if key.bind_group_data.double_sided { None } else { Some(Face::Back) };
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
            depth.depth_compare = Some(CompareFunction::GreaterEqual);
        }
        Ok(())
    }
}

impl Sky {
    /// The diffuse texture a sky material wants, or `None` for another technique.
    pub fn texture(source: &model::Material) -> Option<Option<u32>> {
        is_sky(source.technique).then(|| source.texture("diffuse_texture1_texture"))
    }

    /// A sky material. `alpha_mode` is the blend the material names (`assets::alpha_mode_of`):
    /// the dome is opaque, the cylinders are the normal blend (`SRCALPHA` / `INVSRCALPHA`, LS-21).
    pub fn new(
        source: &model::Material,
        diffuse: Option<Handle<Image>>,
        bloom: Option<Handle<Image>>,
        alpha_mode: AlphaMode,
    ) -> Self {
        // [game 0b7890:170, 0b7ca0:45] the authored colour on sky_vc and bgnd_vc.
        let mut flags = if matches!(source.technique, SKY_VC | BGND_VC) { VERTEX_COLOUR } else { 0 };
        if source.technique == BGND_VC { flags |= BACKDROP; }
        if alpha_mode == AlphaMode::Add { flags |= ADD; }
        let (fog_colour, fog_range) = crate::world::mission_rig()
            .map_or((Vec4::ZERO, Vec4::ZERO), |rig| (rig.fog_color(), rig.fog_range()));
        Sky {
            values: SkyValues {
                exposure: Vec4::new(EXPOSURE, 0.0, 0.0, 0.0),
                flags: UVec4::new(flags, 0, 0, 0),
                boost: Vec4::new(ALPHA_BOOST, 0.0, 0.0, 0.0),
                fog_colour,
                fog_range,
            },
            diffuse,
            bloom,
            alpha_mode,
            // [data] The material's cullmode None has the same meaning as on Surface (LS-34).
            double_sided: source.word("cullmode") == Some(crc32(b"None")),
        }
    }
}

/// Whether a technique uses the sky/backdrop material.
pub fn is_sky(technique: u32) -> bool {
    matches!(technique, SKY | SKY_VC | BGND_VC)
}

pub fn plugin(app: &mut App) {
    app.add_plugins(MaterialPlugin::<Sky>::default());
    let shader = Shader::from_wgsl(include_str!("sky.wgsl"), "sky.wgsl");
    app.world_mut().resource_mut::<Assets<Shader>>().insert(&SKY_SHADER, shader).expect("the sky shader's handle");
}
