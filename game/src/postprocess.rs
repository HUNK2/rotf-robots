//! Native static Environment colour controls and32^3 LUT. Bloom/exposure and active
//! environment blending remain separate debts; see the unpublished notes.
use bevy::{
    asset::uuid_handle,
    core_pipeline::{Core3dSystems, FullscreenShader, schedule::Core3d, tonemapping},
    prelude::*,
    render::{Render, RenderApp, RenderStartup, RenderSystems,
        extract_component::{ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin, UniformComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::{binding_types::{sampler, texture_2d, texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer}, *},
        renderer::{RenderContext, RenderDevice, ViewQuery},
        texture::{CachedTexture, GpuImage, TextureCache}, view::ViewTarget,
    },
};
const SHADER: Handle<Shader> = uuid_handle!("041a79e6-67bc-4140-9a68-ce07b5efeb06");

#[derive(Component, Clone, Copy, ExtractComponent, ShaderType)]
struct Settings {
    smh: [Vec4;3], tint: Vec4, params: Vec4, fade: Vec4, flags: UVec4,
    /// x the iris (IrisParams.x), y irisLumPower.
    iris: Vec4,
    /// BloomLevel (threshold, 1/threshold, saturation, w: 1 when the native bloom runs) [game 00546350].
    bloom_level: Vec4,
    /// Brightness: (bloomTint + 0.1) / max(bloomTint + 0.1) * bloomBrightness [game 00546350].
    bloom_brightness: Vec4,
    /// Gaussian13 taps (texel offset, weight) [game 005839a0], sigma 2, weight scale 1.1.
    taps: [Vec4; 13],
    /// DOFParams (farFocus, nearFocus - nearFalloff, 1/max(farFalloff,1e-4), 1/max(nearFalloff,1e-4)) [game 00551670].
    dof: Vec4,
    /// MaxBlur, dofKernelTap, the camera's near plane, 1 when the soft DOF runs.
    dof2: Vec4,
}

/// [game 00546350] The bloom threshold: max(bloomThreshold, 0.01), or with the iris on
/// max(irisBloomGain * iris / (iris + 1), 0). Threshold and saturation are the frame's blended ones
/// (`EnvironmentBlend`), the base environment's when nothing is active.
fn bloom_level(env: &tf2_core::formats::environment::Environment, look: &Look, iris: Option<f32>, on: bool) -> Vec4 {
    let threshold = match iris {
        Some(iris) => (env.iris_bloom_gain * iris / (iris + 1.0)).max(0.0),
        None => look.bloom_threshold.max(0.01),
    }.max(1e-4);
    Vec4::new(threshold, 1.0 / threshold, look.bloom_saturation, if on { 1.0 } else { 0.0 })
}

/// [game 00546350] The bloom's brightness vector: (bloomTint + 0.1) / max(bloomTint + 0.1) * bloomBrightness.
fn bloom_brightness(look: &Look) -> Vec4 {
    let tint = look.bloom_tint + Vec3::splat(0.1);
    (tint / tint.max_element() * look.bloom_brightness).extend(0.0)
}

/// [game 00547190] The colour filter controls as uploaded: every control x 2/255 (127.5 is neutral).
fn colour_controls(s: &mut Settings, f: &tf2_core::environment_blend::Filter) {
    let scale = 2.0 / 255.0;
    s.smh = f.smh.map(|c| c * scale);
    s.tint = (f.tint * scale).extend(f.saturation * scale);
    s.params = Vec4::new(f.brightness * scale, 0.0, f.contrast * scale, f.lightness * scale);
}

use tf2_core::environment_blend::{EnvBlend, Look};

/// [game 00565c00, 007e5020, 00565da0] The active-environment list and what it makes of the base this frame.
/// [stand-in] only the trigger's list (00d16fd0) is kept; the region list 00d16fc4 (environments with
/// trigRects, 00565a90) and the state environments (FlashBang, LowHeath, Overdrive) do not feed it yet.
#[derive(Resource)]
pub struct EnvironmentBlend {
    list: EnvBlend,
    base: Look,
    look: Look,
    clock: f32,
    /// `--environment-blend <name> <in s> [<out s>]`: a test aid that blends one zone environment in and out.
    schedule: Vec<(f32, u32, bool)>,
    /// Whether the base's LUT texture resolved (`Settings.flags.x` at setup).
    has_source: bool,
}

fn environment_blend_schedule(args: &[String]) -> Vec<(f32, u32, bool)> {
    let Some(at) = args.iter().position(|a| a == "--environment-blend") else { return Vec::new() };
    let name = args.get(at + 1).map(|n| tf2_core::formats::hash::crc32(n.as_bytes())).unwrap_or(0);
    let time = |i: usize| args.get(at + i).and_then(|t| t.parse::<f32>().ok());
    [time(2).map(|t| (t, name, true)), time(3).map(|t| (t, name, false))].into_iter().flatten().collect()
}

/// One game step (0.032 s) at a time, as the iris: fold the list into the base with the weights from before
/// the step [game 00565da0], then step it; write the uniforms when the result changes.
fn update_environment_blend(time: Res<Time>, mut state: ResMut<EnvironmentBlend>, iris: Option<Res<Iris>>, mut settings: Query<&mut Settings>) {
    let specs = crate::world::env_specs();
    let now = time.elapsed_secs();
    let due: Vec<_> = state.schedule.iter().filter(|(at, ..)| *at <= now).map(|(_, n, on)| (*n, *on)).collect();
    state.schedule.retain(|(at, ..)| *at > now);
    for (name, on) in due {
        // [game 007e4ea0] an unresolved name is skipped.
        let Some(spec) = specs.iter().find(|s| s.name == name) else { warn!("environment blend: no environment {name:08x} in this zone"); continue };
        if on { state.list.blend_in(spec) } else { state.list.blend_out(spec) }
        info!("environment blend: {} {} (blend {} s in, {} s out, priority {}; bloom {} threshold {} brightness {} saturation {} tint {})", spec.label, if on { "in" } else { "out" },
            spec.blend_in, spec.blend_out, spec.priority, spec.look.bloom, spec.look.bloom_threshold, spec.look.bloom_brightness, spec.look.bloom_saturation, spec.look.bloom_tint);
        if spec.look.colour_layer && spec.look.filter.lut.as_deref().is_some_and(|l| !l.is_empty()) && spec.look.filter.lut != state.base.filter.lut {
            warn!("environment blend: {} has its own LUT {:?} [stand-in: the base LUT texture stays; only its controls change]", spec.label, spec.look.filter.lut);
        }
    }
    state.clock += time.delta_secs();
    let mut stepped = false;
    while state.clock >= 0.032 {
        state.clock -= 0.032;
        state.look = tf2_core::environment_blend::blend_look(&state.base, &state.list, specs);
        state.list.step(0.032);
        stepped = true;
    }
    if !stepped { return; }
    let Some(env) = crate::world::look() else { return };
    for mut s in &mut settings {
        let level = bloom_level(env, &state.look, iris.as_ref().filter(|i| i.params.enabled).map(|i| i.iris), s.bloom_level.w > 0.0);
        let brightness = bloom_brightness(&state.look);
        if s.bloom_level != level { s.bloom_level = level; }
        if s.bloom_brightness != brightness { s.bloom_brightness = brightness; }
        let before = (s.smh, s.tint, s.params, s.flags.x);
        colour_controls(&mut s, &state.look.filter);
        // [game 00566690] a layer without a LUT (env+0x238 zero) composes from the identity cube cf56c4; one
        // with the base's LUT from that texture. [stand-in] a layer with another LUT keeps the base texture.
        let lut = state.look.filter.lut.as_deref().filter(|l| !l.is_empty());
        s.flags.x = (lut.is_some() && state.has_source) as u32;
        if (s.smh, s.tint, s.params, s.flags.x) != before { info!("environment blend: colour controls {:?}", state.look.filter); }
    }
}
#[derive(Resource, Clone, ExtractResource)]
struct SourceLut(Handle<Image>);
/// The 4x4 luminance target the iris reads back (`tf2_core::iris`).
#[derive(Resource, Clone, ExtractResource)]
struct IrisTarget(Handle<Image>);

/// The iris controller's state in the main world.
#[derive(Resource)]
pub struct Iris {
    params: tf2_core::iris::IrisParams,
    pub iris: f32,
    pub lum: Option<f32>,
    clock: f32,
}

/// [game 00562c70] One controller update per frame, with the last read-back luminance. [assumed] the game's
/// frame is its 0.032 s step; the port steps the controller that often whatever its own frame rate.
fn update_iris(time: Res<Time>, mut iris: ResMut<Iris>, blend: Option<Res<EnvironmentBlend>>, mut settings: Query<&mut Settings>) {
    let Some(lum) = iris.lum else { return };
    iris.clock += time.delta_secs();
    let before = (time.elapsed_secs() - time.delta_secs()) as u32;
    while iris.clock >= 0.032 {
        iris.clock -= 0.032;
        iris.iris = tf2_core::iris::step(iris.iris, lum, &iris.params);
    }
    if time.elapsed_secs() as u32 != before { info!("iris {:.3} (luminance {:.3})", iris.iris, lum); }
    let env = crate::world::look();
    for mut s in &mut settings {
        if s.iris.x != iris.iris {
            s.iris.x = iris.iris;
            if let Some(env) = env {
                let on = s.bloom_level.w;
                let base = blend.as_ref().map_or_else(|| Look::of_environment(env), |b| b.look.clone());
                s.bloom_level = bloom_level(env, &base, Some(iris.iris), on > 0.0);
            }
        }
    }
}

pub fn plugin(app: &mut App) {
    let args: Vec<_> = std::env::args().collect();
    if crate::world::look().is_none() || !args.iter().any(|a| a == "--mission" || a == "--level")
        || args.iter().any(|a| a == "--legacy-level-post") { return; }
    app.world_mut().resource_mut::<Assets<Shader>>()
        .insert(&SHADER, Shader::from_wgsl(include_str!("postprocess.wgsl"), "postprocess.wgsl")).expect("LUT shader");
    app.add_plugins((ExtractComponentPlugin::<Settings>::default(), UniformComponentPlugin::<Settings>::default(), ExtractResourcePlugin::<SourceLut>::default(),
            ExtractResourcePlugin::<IrisTarget>::default()))
        .add_systems(Startup, setup.after(crate::spawn_player))
        // `setup` is skipped when its `Single` camera does not validate, and then there is no Iris.
        .add_systems(Update, (update_iris.run_if(resource_exists::<Iris>), update_environment_blend.run_if(resource_exists::<EnvironmentBlend>)));
    let Some(render) = app.get_sub_app_mut(RenderApp) else { return; };
    render.init_resource::<SpecializedRenderPipelines<Pipeline>>()
        .add_systems(RenderStartup, init)
        .add_systems(Render, prepare.in_set(RenderSystems::Prepare))
        // [assumed] Current port ordering: after its bloom, before tonemapping/HUD.
        .add_systems(Core3d, draw.in_set(Core3dSystems::PostProcess)
            .after(bevy::post_process::bloom::bloom).before(tonemapping::tonemapping));
}

fn setup(mut commands: Commands, camera: Single<Entity, (With<Camera3d>, Without<crate::ocean::OceanReflection>, Without<crate::duel::SecondView>)>, level: Option<Res<crate::level::LevelModels>>, mut images: ResMut<Assets<Image>>) {
    let Some(env) = crate::world::look() else { return; };
    let source = env.filter_lut.as_ref().filter(|name| !name.is_empty()).and_then(|name| {
        let id = tf2_core::formats::hash::lower33(name);
        let texture = level.as_ref().and_then(|level| level.0.library.textures.get(&id));
        match texture {
            Some(t) if t.width == 1024 && t.height == 32 && t.faces == 1 => crate::assets::image(t,false),
            Some(t) => { warn!("colour LUT {name}: unsupported size {}x{} faces{}",t.width,t.height,t.faces); None }
            None => { warn!("colour LUT {name} ({id:08x}) missing; using identity source"); None }
        }
    });
    let has_source = source.is_some();
    // An unused but valid binding when the native identity cube is the source.
    let handle = images.add(source.unwrap_or_else(Image::default));
    commands.insert_resource(SourceLut(handle));
    let scale = 2.0/255.0;
    // `--no-iris`: a comparison switch (the iris held at 1, as before 2026-10-16).
    let params = tf2_core::iris::IrisParams { enabled: env.iris_enabled && !std::env::args().any(|a| a == "--no-iris"),
        target: env.iris_target, min: env.iris_min, max: env.iris_max, speed: env.iris_speed,
        responsiveness: env.iris_responsiveness, gate: env.iris_gate };
    let start = if params.enabled { tf2_core::iris::START } else { 1.0 };
    commands.insert_resource(Iris { params, iris: start, lum: None, clock: 0.0 });
    // The 4x4 luminance target and its readback every frame [game 00581a00].
    let mut target = Image::new_uninit(Extent3d { width: 4, height: 4, depth_or_array_layers: 1 }, TextureDimension::D2,
        TextureFormat::Rgba8Unorm, bevy::asset::RenderAssetUsages::RENDER_WORLD);
    target.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC | TextureUsages::TEXTURE_BINDING;
    let target = images.add(target);
    commands.insert_resource(IrisTarget(target.clone()));
    if params.enabled {
        commands.spawn(bevy::render::gpu_readback::Readback::texture(target)).observe(
            |event: On<bevy::render::gpu_readback::ReadbackComplete>, mut iris: ResMut<Iris>| {
                // Rows are padded to the copy alignment: the red byte of texel (x, y) is at y * stride + 4x.
                let stride = event.data.len() / 4;
                let reds: Vec<u8> = (0..4).flat_map(|y| (0..4).map(move |x| (y, x))).filter_map(|(y, x)| event.data.get(y * stride + 4 * x).copied()).collect();
                iris.lum = Some(tf2_core::iris::luminance(&reds));
            });
    }
    // The native bloom [game 00546350] replaces Bevy's stand-in; `--bevy-bloom` keeps the stand-in for comparison.
    let native_bloom = env.bloom_enabled && !std::env::args().any(|a| a == "--bevy-bloom");
    if native_bloom { commands.entity(*camera).remove::<bevy::post_process::bloom::Bloom>(); }
    let base = Look::of_environment(env);
    let brightness = bloom_brightness(&base);
    let taps = tf2_core::speedblur::gaussian_taps(2.0).map(|t| Vec4::new(t[0], t[1], t[2] * 1.1, 0.0));
    // [game 00551670] the soft branch (one five-tap pass). [assumed] DOF_USE_SOFTER selects it; the hard branch
    // (quarter copy and Gaussian13 by dofBlurAmount) is not ported for the level look (the turbo uses it).
    let [_blur_amount, max_blur, near_focus, far_focus, near_falloff, far_falloff, kernel] = env.dof;
    let soft_dof = env.dof_enabled && env.dof_softer && !std::env::args().any(|a| a == "--no-dof");
    commands.entity(*camera).insert((Settings {
        smh: env.filter_smh.map(|c| c*scale),
        tint: (env.filter_tint*scale).extend(env.filter_saturation*scale),
        params: Vec4::new(env.filter_brightness*scale,0.0,env.filter_contrast*scale,env.filter_lightness*scale),
        // [assumed] No global fade/debug brightness override on initial free-roam load.
        fade: Vec4::ZERO, flags: UVec4::new(has_source as u32,0,0,0),
        iris: Vec4::new(start, env.iris_lum_power, 0.0, 0.0),
        bloom_level: bloom_level(env, &base, params.enabled.then_some(start), native_bloom),
        bloom_brightness: brightness,
        taps,
        dof: Vec4::new(far_focus, near_focus - near_falloff, 1.0 / far_falloff.max(1e-4), 1.0 / near_falloff.max(1e-4)),
        dof2: Vec4::new(max_blur, kernel, 0.5, if soft_dof { 1.0 } else { 0.0 }),
    }, bevy::core_pipeline::tonemapping::Tonemapping::None, bevy::render::view::ColorGrading::default()));
    let args: Vec<String> = std::env::args().collect();
    commands.insert_resource(EnvironmentBlend { list: EnvBlend::default(), look: base.clone(), base, clock: 0.0, schedule: environment_blend_schedule(&args), has_source });
    info!("native depth of field: soft {soft_dof} (flags DOF {}, DOF_USE_SOFTER {}), focus {near_focus}..{far_focus}, falloff {near_falloff}/{far_falloff}, max blur {max_blur}",
        env.dof_enabled, env.dof_softer);
    info!("native bloom: {native_bloom}, threshold {}, saturation {}, brightness {brightness}", env.bloom_threshold, env.bloom_saturation);
    info!("iris: enabled {}, target {}, min {}, max {}, speed {}, responsiveness {}, lum power {}, gate {}", params.enabled, params.target,
        params.min, params.max, params.speed, params.responsiveness, env.iris_lum_power, params.gate);
    info!("native colour controls: LUT {:?}, source resolved {has_source}, brightness {}, contrast {}, saturation {}, lightness {}", env.filter_lut,env.filter_brightness,env.filter_contrast,env.filter_saturation,env.filter_lightness);
}

#[derive(Resource)]
struct Pipeline { layouts: [BindGroupLayoutDescriptor;2], sampler: Sampler, fullscreen: FullscreenShader }
impl SpecializedRenderPipeline for Pipeline {
    /// The target format, the entry (0 compose_lut, 1 apply_lut, 2 luminance, 3 bloom_extract, 4 bloom_copy,
    /// 5 bloom_blur, 6 dof) and whether the depth is multisampled.
    type Key = (TextureFormat,u8,bool);
    fn specialize(&self,(format,entry,multisampled):Self::Key)->RenderPipelineDescriptor {
        let shader_defs = if multisampled { vec!["MULTISAMPLED".into()] } else { Vec::new() };
        RenderPipelineDescriptor { label:Some("native colour LUT".into()),layout:vec![self.layouts[multisampled as usize].clone()],vertex:self.fullscreen.to_vertex_state(),
            fragment:Some(FragmentState { shader:SHADER,shader_defs,entry_point:Some(["compose_lut","apply_lut","luminance","bloom_extract","bloom_copy","bloom_blur","dof"][entry as usize].into()),
                targets:vec![Some(ColorTargetState {format,blend:None,write_mask:ColorWrites::ALL})] }),..default() }
    }
}
fn init(mut commands:Commands,device:Res<RenderDevice>,fullscreen:Res<FullscreenShader>) {
    let layout = |multisampled: bool| BindGroupLayoutDescriptor::new("native post bindings",&BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT,(
            texture_2d(TextureSampleType::Float {filterable:true}),sampler(SamplerBindingType::Filtering),
            texture_2d(TextureSampleType::Float {filterable:true}),uniform_buffer::<Settings>(true),
            texture_2d(TextureSampleType::Float {filterable:true}),
            if multisampled { texture_depth_2d_multisampled() } else { texture_depth_2d() })));
    commands.insert_resource(Pipeline {
        layouts:[layout(false),layout(true)],
        sampler:device.create_sampler(&SamplerDescriptor {mag_filter:FilterMode::Linear,min_filter:FilterMode::Linear,..default()}),fullscreen:fullscreen.clone(),
    });
}
#[derive(Component)]
struct Prepared { lut:CachedTexture, quarter:CachedTexture, eighth:[CachedTexture;2], dof:CachedTexture, pipelines:[CachedRenderPipelineId;7], multisampled:bool }
fn prepare(mut commands:Commands,device:Res<RenderDevice>,mut textures:ResMut<TextureCache>,pipeline:Res<Pipeline>,cache:Res<PipelineCache>,mut specialized:ResMut<SpecializedRenderPipelines<Pipeline>>,
    views:Query<(Entity,&ViewTarget,&bevy::render::view::Msaa),With<Settings>>) {
    for (entity,target,msaa) in &views {
        let texture = |label, width, height, format| TextureDescriptor {label:Some(label),size:Extent3d {width,height,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:TextureDimension::D2,
            format,usage:TextureUsages::RENDER_ATTACHMENT|TextureUsages::TEXTURE_BINDING,view_formats:&[]};
        let size = target.main_texture().size();
        let main = target.main_texture_format();
        let rgba8 = TextureFormat::Rgba8Unorm;
        // [game 01177140, 011771c0] the bloom's targets: align16(screen / 4) and align16(screen / 8), 8-bit.
        let [qw, qh] = tf2_core::speedblur::intermediate_size(size.width, size.height);
        let [ew, eh] = [size.width, size.height].map(|n| (n / 8).max(1).div_ceil(16) * 16);
        let lut = textures.get(&device,texture("native composed colour LUT",1024,32,rgba8));
        let quarter = textures.get(&device,texture("native bloom quarter",qw,qh,rgba8));
        let eighth = [textures.get(&device,texture("native bloom eighth a",ew,eh,rgba8)), textures.get(&device,texture("native bloom eighth b",ew,eh,rgba8))];
        let dof = textures.get(&device,texture("native depth of field",size.width,size.height,main));
        let multisampled = msaa.samples() > 1;
        let pipelines = [(rgba8,0),(main,1),(rgba8,2),(rgba8,3),(rgba8,4),(rgba8,5),(main,6)]
            .map(|(format,entry)| specialized.specialize(&cache,&pipeline,(format,entry,multisampled)));
        commands.entity(entity).insert(Prepared {lut,quarter,eighth,dof,pipelines,multisampled});
    }
}
fn draw(view:ViewQuery<(&ViewTarget,&DynamicUniformIndex<Settings>,&Settings,Option<&bevy::core_pipeline::prepass::ViewPrepassTextures>,Option<&Prepared>)>,pipeline:Res<Pipeline>,cache:Res<PipelineCache>,uniforms:Res<ComponentUniforms<Settings>>,source:Option<Res<SourceLut>>,
    iris_target:Option<Res<IrisTarget>>,images:Res<RenderAssets<GpuImage>>,mut ctx:RenderContext) {
    let (target,index,settings,prepass,prepared)=view.into_inner();
    let Some(prepared)=prepared else {return;};
    let Some(depth)=prepass.and_then(|p|p.depth_view()) else {return;};
    let Some(image)=source.and_then(|s|images.get(&s.0)) else {return;};
    let Some(binding)=uniforms.uniforms().binding() else {return;};
    let Some(pipelines)=prepared.pipelines.iter().map(|id|cache.get_render_pipeline(*id)).collect::<Option<Vec<_>>>() else {return;};
    let [compose,apply,luminance,extract,copy,blur,dof]=[0,1,2,3,4,5,6].map(|i|pipelines[i]);
    let layout=cache.get_bind_group_layout(&pipeline.layouts[prepared.multisampled as usize]);
    let post=target.post_process_write();
    let (lut,quarter,[a,b],dof_target)=(&prepared.lut.default_view,&prepared.quarter.default_view,[&prepared.eighth[0].default_view,&prepared.eighth[1].default_view],&prepared.dof.default_view);
    let iris=iris_target.and_then(|t|images.get(&t.0));
    let soft_dof=settings.dof2.w>0.0;
    // (source, lut binding, bloom binding, target, pipeline). No pass reads the texture it writes.
    let mut passes=vec![
        (&image.texture_view,&image.texture_view,a,lut,compose),
        // [game 00546350] the bloom: extract to the quarter, copy to the eighth, three Gaussian13 passes.
        (post.source,lut,a,quarter,extract),
        (quarter,lut,b,a,copy),
        (a,lut,quarter,b,blur),
        (b,lut,quarter,a,blur),
        (a,lut,quarter,b,blur),
    ];
    // [game 00551670] the depth of field on the scene, before the final colour pass reads it.
    if soft_dof { passes.push((post.source,lut,b,dof_target,dof)); }
    passes.push((if soft_dof { dof_target } else { post.source },lut,b,post.destination,apply));
    // The iris's 4x4 luminance target, from the same pre-LUT scene [game 00562c70].
    if let Some(iris)=iris { passes.push((post.source,lut,b,&iris.texture_view,luminance)); }
    for (source,lut,bloom,destination,pass_pipeline) in passes {
        let group=ctx.render_device().create_bind_group("native post",&layout,&BindGroupEntries::sequential((source,&pipeline.sampler,lut,binding.clone(),bloom,depth)));
        let mut pass=ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {label:Some("native post"),color_attachments:&[Some(RenderPassColorAttachment {view:destination,depth_slice:None,resolve_target:None,ops:Operations::default()})],depth_stencil_attachment:None,timestamp_writes:None,occlusion_query_set:None,multiview_mask:None});
        pass.set_pipeline(pass_pipeline);pass.set_bind_group(0,&group,&[index.index()]);pass.draw(0..3,0..1);
    }
}
