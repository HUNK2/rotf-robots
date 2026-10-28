//! The level's ambient occlusion, as the game's deferred path (VideoQuality 2, the user's) makes it.
//!
//! [game] 0059d3f0 draws effects.lsa 003ce0 (eight taps against the depth and the G-buffer normal) into the
//! DownsampledSSAOMap, then 005090 (a bilateral blur) twice; the ambient composite 007650 multiplies the ambient
//! by `1 - sat(SSAO.g * SSAOFinalScale)` (`surface.wgsl`). `ssao.wgsl` has the arithmetic.
//! Here: one pass after Bevy's depth prepass and before the main pass writes the shared texture `MAP`, which
//! every Surface material binds (19, 20). Without a level, or with the SSAO flag off, or `--no-ssao`, `MAP`
//! stays zero (no occlusion) and the pass does not run.
//! [assumed] `MAP` is 960x540 (the game's "downsampled" size is not read); the normal is rebuilt from the depth.
use bevy::{
    asset::{uuid_handle, RenderAssetUsages},
    core_pipeline::{prepass::ViewPrepassTextures, schedule::Core3d, Core3dSystems, FullscreenShader},
    prelude::*,
    render::{
        render_asset::RenderAssets,
        render_resource::{binding_types::{sampler, texture_2d, texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer_sized}, *},
        renderer::{RenderContext, RenderDevice, ViewQuery},
        texture::{CachedTexture, GpuImage, TextureCache},
        view::{ExtractedView, Msaa},
        Render, RenderApp, RenderStartup, RenderSystems,
    },
};

/// The occlusion map every Surface material samples (x octant code, y occlusion).
pub const MAP: Handle<Image> = uuid_handle!("8d2f5c1e-4a7b-4e39-9b62-1f0c3d5a7e84");
const SHADER: Handle<Shader> = uuid_handle!("b4e1a9d2-5c3f-4b8e-a716-2d9f0e6c4b31");
/// [assumed] the map's size (half of 1920x1080).
const SIZE: (u32, u32) = (960, 540);
/// [game 0059d3f0] SSAOSampleRange and SSAOSampleRangeFar (uv), set right before the draw.
const SAMPLE_RANGE: f32 = 0.071;
const SAMPLE_RANGE_FAR: f32 = 0.01;
/// [game 0059d3f0] the blur's tap spacing in screen pixels.
const BLUR_STEP: f32 = 1.845;

/// The ambient composite's SSAOFinalScale, 0 when the occlusion is off.
pub fn final_scale() -> f32 {
    if !running() { return 0.0; }
    crate::world::look().map_or(0.0, |e| e.ssao_final_scale)
}

/// The pass runs with a mission's level whose Environment has the SSAO flag, unless `--no-ssao`.
fn running() -> bool {
    let args: Vec<String> = std::env::args().collect();
    args.iter().any(|a| a == "--mission") && !args.iter().any(|a| a == "--no-ssao")
        && crate::world::look().is_some_and(|e| e.ssao_enabled)
}

pub fn plugin(app: &mut App) {
    // The map always exists (zero: no occlusion), so every Surface's binding is valid.
    let mut image = Image::new_fill(Extent3d { width: SIZE.0, height: SIZE.1, depth_or_array_layers: 1 }, TextureDimension::D2,
        &[0, 0], TextureFormat::Rg8Unorm, RenderAssetUsages::RENDER_WORLD);
    image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING;
    app.world_mut().resource_mut::<Assets<Image>>().insert(&MAP, image).expect("the SSAO map's handle");
    app.world_mut().resource_mut::<Assets<Shader>>()
        .insert(&SHADER, Shader::from_wgsl(include_str!("ssao.wgsl"), "ssao.wgsl")).expect("the SSAO shader's handle");
    if !running() { return; }
    info!("ssao: the level's occlusion pass runs (SSAOFinalScale {})", final_scale());
    let Some(render) = app.get_sub_app_mut(RenderApp) else { return };
    render.init_resource::<SpecializedRenderPipelines<Pipeline>>()
        .add_systems(RenderStartup, init)
        .add_systems(Render, prepare.in_set(RenderSystems::Prepare))
        .add_systems(Core3d, draw.after(Core3dSystems::Prepass).before(Core3dSystems::MainPass));
}

#[derive(Resource)]
struct Pipeline { layouts: [BindGroupLayoutDescriptor; 2], sampler: Sampler, fullscreen: FullscreenShader }

impl SpecializedRenderPipeline for Pipeline {
    /// (multisampled depth, blur pass)
    type Key = (bool, bool);
    fn specialize(&self, (multisampled, blur): Self::Key) -> RenderPipelineDescriptor {
        let shader_defs = if multisampled { vec!["MULTISAMPLED".into()] } else { Vec::new() };
        RenderPipelineDescriptor {
            label: Some("level ssao".into()),
            layout: vec![self.layouts[multisampled as usize].clone()],
            vertex: self.fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: SHADER, shader_defs, entry_point: Some(if blur { "blur" } else { "ssao" }.into()),
                targets: vec![Some(ColorTargetState { format: TextureFormat::Rg8Unorm, blend: None, write_mask: ColorWrites::ALL })],
            }),
            ..default()
        }
    }
}

fn init(mut commands: Commands, device: Res<RenderDevice>, fullscreen: Res<FullscreenShader>) {
    let layout = |multisampled: bool| BindGroupLayoutDescriptor::new("level ssao bindings", &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (
        uniform_buffer_sized(false, None),
        if multisampled { texture_depth_2d_multisampled() } else { texture_depth_2d() },
        texture_2d(TextureSampleType::Float { filterable: true }),
        sampler(SamplerBindingType::Filtering),
    )));
    commands.insert_resource(Pipeline {
        layouts: [layout(false), layout(true)],
        sampler: device.create_sampler(&SamplerDescriptor { mag_filter: FilterMode::Nearest, min_filter: FilterMode::Nearest, ..default() }),
        fullscreen: fullscreen.clone(),
    });
}

#[derive(Component)]
struct Prepared { raw: CachedTexture, across: CachedTexture, ssao: CachedRenderPipelineId, blur: CachedRenderPipelineId, multisampled: bool }

fn prepare(mut commands: Commands, device: Res<RenderDevice>, mut textures: ResMut<TextureCache>, pipeline: Res<Pipeline>, cache: Res<PipelineCache>,
    mut specialized: ResMut<SpecializedRenderPipelines<Pipeline>>, views: Query<(Entity, &Msaa), (With<ViewPrepassTextures>, With<Camera3d>, Without<crate::ocean::OceanReflection>)>) {
    for (entity, msaa) in &views {
        let texture = |label| TextureDescriptor { label: Some(label), size: Extent3d { width: SIZE.0, height: SIZE.1, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2, format: TextureFormat::Rg8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING, view_formats: &[] };
        let multisampled = msaa.samples() > 1;
        commands.entity(entity).insert(Prepared {
            raw: textures.get(&device, texture("level ssao raw")),
            across: textures.get(&device, texture("level ssao across")),
            ssao: specialized.specialize(&cache, &pipeline, (multisampled, false)),
            blur: specialized.specialize(&cache, &pipeline, (multisampled, true)),
            multisampled,
        });
    }
}

fn draw(view: ViewQuery<(&ExtractedView, &ViewPrepassTextures, Option<&Prepared>)>, pipeline: Res<Pipeline>, cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>, mut ctx: RenderContext) {
    let (extracted, prepass, prepared) = view.into_inner();
    let (Some(prepared), Some(depth), Some(map)) = (prepared, prepass.depth_view(), images.get(&MAP)) else { return };
    let (Some(ssao), Some(blur)) = (cache.get_render_pipeline(prepared.ssao), cache.get_render_pipeline(prepared.blur)) else { return };
    let layout = cache.get_bind_group_layout(&pipeline.layouts[prepared.multisampled as usize]);
    let size = Vec2::new(prepass.size.width as f32, prepass.size.height as f32);
    // [game 0059d3f0] the game camera's near and far (also the port's: `main.rs`).
    let base = |step: Vec2| {
        let mut floats: Vec<f32> = Vec::with_capacity(40);
        floats.extend(extracted.clip_from_view.inverse().to_cols_array());
        floats.extend(extracted.world_from_view.to_matrix().to_cols_array());
        floats.extend([0.5, 1500.0, SAMPLE_RANGE, SAMPLE_RANGE_FAR, size.x, size.y, step.x, step.y]);
        let bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
        ctx.render_device().create_buffer_with_data(&BufferInitDescriptor { label: Some("level ssao params"), contents: &bytes, usage: BufferUsages::UNIFORM })
    };
    let buffers = [base(Vec2::ZERO), base(Vec2::new(BLUR_STEP / size.x, 0.0)), base(Vec2::new(0.0, BLUR_STEP / size.y))];
    let passes = [
        (&buffers[0], &prepared.across.default_view, &prepared.raw.default_view, ssao),
        (&buffers[1], &prepared.raw.default_view, &prepared.across.default_view, blur),
        (&buffers[2], &prepared.across.default_view, &map.texture_view, blur),
    ];
    for (buffer, source, target, pass_pipeline) in passes {
        let group = ctx.render_device().create_bind_group("level ssao", &layout,
            &BindGroupEntries::sequential((buffer.as_entire_binding(), depth, source, &pipeline.sampler)));
        let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
            label: Some("level ssao"),
            color_attachments: &[Some(RenderPassColorAttachment { view: target, depth_slice: None, resolve_target: None, ops: Operations::default() })],
            depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
        });
        pass.set_pipeline(pass_pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}
