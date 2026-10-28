//! The original's `ocean_water2` technique, the sea of deep_six_01 (`OceanTile.oceantile_*`, 3.67 M
//! triangles): vertex shader 0a0c30, pixel shader 0a1d90, decoded in the unpublished notes.
//! `ocean.wgsl` has the arithmetic, line for line, and what in it is the game's.
//!
//! The values are the material's own [data], by the names of the shaders' constant tables (lower case,
//! CRC-32; the unpublished notes section 4). The light rig and fog are the level's
//! (`world::mission_rig`), as `surface.rs` fills them.
//!
//! The reflection: the game draws a mirrored view of the level into a 511x511 target (`ReflectionMapDraw`
//! 0057a1b0) with a clip plane at the sea (005797b0). The port: a second camera into a 512x512 image (`setup`,
//! `follow` below), leaving out the sea, the sky and the NoReflectionTag placements. `--no-ocean-reflection`
//! turns it off (the reflection is then black and "nothing drawn", the first cut).

use bevy::asset::uuid_handle;
use bevy::camera::CameraProjection;
use bevy::camera::primitives::MeshAabb;
use bevy::prelude::*;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::render::render_resource::{AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::{Shader, ShaderRef};

use crate::formats::hash::crc32;
use crate::formats::model;

const OCEAN_SHADER: Handle<Shader> = uuid_handle!("4d1f7c2a-8e36-4b90-9a5d-0c6e2b8f13a7");

/// `ocean_water2` [data: the unpublished notes 3].
const OCEAN_WATER2: u32 = crc32(b"ocean_water2");

/// Flags of `ocean.wgsl`.
const RIG: u32 = 1;
/// A reflection image is bound (`reflect`).
const REFLECTION: u32 = 2;
/// The port's reflection camera is a rotation, not a mirror: its picture is sampled at 1 - u.
const MIRRORED_X: u32 = 4;

/// The render layer of what the reflection pass leaves out: the sea itself (its material samples the
/// reflection, so it must not be drawn into it), the sky domes and the NoReflectionTag placements. The
/// main camera sees layers 0 and this one; the reflection camera layer 0 only.
pub const NOT_REFLECTED: usize = 1;

/// [game 0057a1b0] `FUN_00a98fd0(511.0, 511.0)`: the reflection target is 511x511 [assumed: the two numbers
/// are its size]. The port uses 512.
const REFLECTION_SIZE: u32 = 512;

#[derive(Clone, ShaderType)]
pub struct OceanValues {
    wave_period: Vec4,
    wave_length: Vec4,
    wave_amp: Vec4,
    wave_phase: Vec4,
    wave_dir: Vec4,
    wave_enable: Vec4,
    /// x Stormy, y Time_Scale, z Vertex_Fade_Dist_Softness, w Vertex_Fade_Dist_Thresh
    wave_misc: Vec4,
    /// x Wrap_Dist
    wrap: Vec4,
    texcoord_xy: Vec4,
    texcoord_zt: Vec4,
    normal_xy: [Vec4; 4],
    normal_zt: [Vec4; 4],
    dxt5: Vec4,
    height_scale: Vec4,
    /// Diffuse_Color * ObjectDiffuseColor
    diffuse: Vec4,
    /// rgb Specular_Color, w Specular_Power
    specular: Vec4,
    /// rgb Reflection_Color, w AlphaBoost
    reflection: Vec4,
    /// x Ocean_Fresnel_Max, y _Min, z _Pow, w Snell_Amount
    fresnel: Vec4,
    /// Ocean_Use_Diffuse, _Reflection, _Specular, _Water
    use_: Vec4,
    /// Ocean_Distortion_Border, _Max, _Aspect_Ratio, _Scale
    distortion: Vec4,
    /// x Ocean_Distortion_Flip_Horiz, y _Flip_Vert
    flip: Vec4,
    /// Ocean_Show_Fresnel_Mask, _Specular, _Reflection, _Diffuse
    show: Vec4,
    /// x Ocean_Show_Water
    show2: Vec4,
    /// mReflectWorldView's two rows, set each frame by `follow` [assumed rows]
    reflect_view: [Vec4; 2],
    rig_sun: Vec4,
    rig_dir: Vec4,
    rig_ambient: Vec4,
    irradiance: [Vec4; 9],
    fog_color: Vec4,
    fog_range: Vec4,
    flags: UVec4,
}

/// The sea's material.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(OceanKey)]
pub struct Ocean {
    #[uniform(0)]
    values: OceanValues,
    #[texture(1)]
    #[sampler(2)]
    normal0: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    normal1: Option<Handle<Image>>,
    #[texture(5)]
    #[sampler(6)]
    normal2: Option<Handle<Image>>,
    #[texture(7)]
    #[sampler(8)]
    normal3: Option<Handle<Image>>,
    #[texture(9)]
    #[sampler(10)]
    fresnel: Option<Handle<Image>>,
    /// ReflectionMap (s3): the reflection camera's picture, set by `follow` (none until then, flag REFLECTION off).
    #[texture(11)]
    #[sampler(12)]
    reflection: Option<Handle<Image>>,
    alpha_mode: AlphaMode,
    double_sided: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct OceanKey { double_sided: bool }

impl From<&Ocean> for OceanKey {
    fn from(ocean: &Ocean) -> Self { Self { double_sided: ocean.double_sided } }
}

impl Material for Ocean {
    fn vertex_shader() -> ShaderRef {
        OCEAN_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        OCEAN_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    /// [assumed] The sea is left out of Bevy's depth prepass (its waves are moved in the vertex stage, which
    /// the prepass would have to repeat): it writes its depth in the main pass. The port's SSAO, which reads
    /// the prepass, does not see it; the ocean's pixel shader reads no SSAO [game 0a1d90].
    fn enable_prepass() -> bool {
        false
    }

    /// [assumed] The sea casts no shadow: it is the lowest thing in the level, and the game's ZOnly pass
    /// (0a3280/0a3ea0) is not known to feed the shadow maps.
    fn enable_shadows() -> bool {
        false
    }

    /// Back faces are dropped as the other ported techniques do [assumed: the cull state of the original's
    /// pass is not read]; cullmode None keeps both.
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = if key.bind_group_data.double_sided { None } else { Some(Face::Back) };
        Ok(())
    }
}

/// The textures an ocean material wants: the four normal maps and Fresnel_Texture, or `None` for another
/// technique.
pub fn textures(source: &model::Material) -> Option<[Option<u32>; 5]> {
    is_ocean(source.technique).then(|| {
        [
            source.texture("normal_map_texture0"),
            source.texture("normal_map_texture1"),
            source.texture("normal_map_texture2"),
            source.texture("normal_map_texture3"),
            source.texture("fresnel_texture"),
        ]
    })
}

/// Whether a technique is the sea's.
pub fn is_ocean(technique: u32) -> bool {
    technique == OCEAN_WATER2
}

impl Ocean {
    /// The sea's material from its parameters [data]. `images`: the four normal maps and Fresnel_Texture.
    pub fn new(source: &model::Material, images: [Option<Handle<Image>>; 5], alpha_mode: AlphaMode) -> Self {
        // A parameter by its name: a number, or a word (the switches are stored as words: Wave_Enable,
        // Stormy, the DXT5 and Show switches) taken as its integer value [assumed: 1 means 1.0].
        let number = |name: &str, default: f32| {
            source.number(name).or_else(|| source.word(name).map(|w| w as f32)).unwrap_or(default)
        };
        let vector = |name: &str, default: [f32; 4]| Vec4::from_array(source.vector(name).unwrap_or(default));
        let four = |stem: &str, default: f32| Vec4::from_array([0, 1, 2, 3].map(|i| number(&format!("{stem}{i}"), default)));
        // [assumed] defaults for a parameter the material lacks (deep_six_01's has all of them): identity
        // transforms, zero for the rest, 1 for the scales and the wrap.
        let xy = |name: &str, x: [f32; 2], y: [f32; 2]| {
            let a = vector(&format!("{name}_x"), [x[0], x[1], 0.0, 0.0]);
            let b = vector(&format!("{name}_y"), [y[0], y[1], 0.0, 0.0]);
            Vec4::new(a.x, a.y, b.x, b.y)
        };
        let zt = |name: &str| {
            let z = vector(&format!("{name}_z"), [0.0; 4]);
            let t = vector(&format!("{name}_t"), [0.0; 4]);
            Vec4::new(z.x, z.y, t.x, t.y)
        };
        let rgb_w = |name: &str, w: f32| vector(name, [1.0; 4]).truncate().extend(w);
        let rig = crate::world::mission_rig();
        let mut flags = 0;
        if rig.is_some() {
            flags |= RIG;
        }
        let (rig_sun, rig_dir, rig_ambient, irradiance, fog_color, fog_range) = match rig {
            // [game] the rig's LightDir is in the game's space, which ocean.wgsl works in: not turned.
            Some(rig) => (rig.sun_colour, rig.sun_dir.extend(0.0), rig.ambient_colour, rig.irradiance.k, rig.fog_color(), rig.fog_range()),
            None => (Vec4::ZERO, Vec4::ZERO, Vec4::ZERO, [Vec4::ZERO; 9], Vec4::ZERO, Vec4::ZERO),
        };
        let values = OceanValues {
            wave_period: four("wave_period", 1.0),
            wave_length: four("wave_wavelength", 1.0),
            wave_amp: four("wave_amp", 0.0),
            wave_phase: four("wave_phase", 0.0),
            wave_dir: four("wave_dir", 0.0),
            wave_enable: four("wave_enable", 0.0),
            wave_misc: Vec4::new(
                number("stormy", 0.0),
                number("time_scale", 1.0),
                number("vertex_fade_dist_softness", 0.0),
                number("vertex_fade_dist_thresh", 0.0),
            ),
            wrap: Vec4::new(number("wrap_dist", 1.0), 0.0, 0.0, 0.0),
            texcoord_xy: xy("texcoord", [1.0, 0.0], [0.0, 1.0]),
            texcoord_zt: zt("texcoord"),
            normal_xy: [0, 1, 2, 3].map(|i| xy(&format!("normal_map{i}"), [1.0, 0.0], [0.0, 1.0])),
            normal_zt: [0, 1, 2, 3].map(|i| zt(&format!("normal_map{i}"))),
            dxt5: four("normal_map_dxt5_nm", 0.0),
            height_scale: four("normal_map_heightscale", 1.0),
            // [assumed] ObjectDiffuseColor is 1 unless a Colorizer tints the placement (`tint`).
            diffuse: vector("diffuse_color", [1.0; 4]),
            specular: rgb_w("specular_color", number("specular_power", 1.0)),
            // [game 005a3087, LS-21] AlphaBoost is 1 for every blend but 0x3467929c (`assets::alpha_boost`).
            reflection: rgb_w("reflection_color", crate::assets::alpha_boost(source)),
            fresnel: Vec4::new(
                number("ocean_fresnel_max", 1.0),
                number("ocean_fresnel_min", 0.0),
                number("ocean_fresnel_pow", 1.0),
                number("snell_amount", 0.0),
            ),
            use_: Vec4::new(
                number("ocean_use_diffuse", 0.0),
                number("ocean_use_reflection", 0.0),
                number("ocean_use_specular", 0.0),
                number("ocean_use_water", 0.0),
            ),
            distortion: Vec4::new(
                number("ocean_distortion_border", 1.0),
                number("ocean_distortion_max", 1.0),
                number("ocean_distortion_aspect_ratio", 1.0),
                number("ocean_distortion_scale", 0.0),
            ),
            flip: Vec4::new(number("ocean_distortion_flip_horiz", 0.0), number("ocean_distortion_flip_vert", 0.0), 0.0, 0.0),
            show: Vec4::new(
                number("ocean_show_fresnel_mask", 0.0),
                number("ocean_show_specular", 0.0),
                number("ocean_show_reflection", 0.0),
                number("ocean_show_diffuse", 0.0),
            ),
            show2: Vec4::new(number("ocean_show_water", 0.0), 0.0, 0.0, 0.0),
            reflect_view: [Vec4::ZERO; 2],
            rig_sun,
            rig_dir,
            // w unused here (surface.rs puts SSAOFinalScale there; the ocean reads no SSAO).
            rig_ambient: rig_ambient.truncate().extend(0.0),
            irradiance,
            fog_color,
            fog_range,
            flags: UVec4::new(flags, 0, 0, 0),
        };
        let [normal0, normal1, normal2, normal3, fresnel] = images;
        Ocean {
            values,
            normal0,
            normal1,
            normal2,
            normal3,
            fresnel,
            reflection: None,
            alpha_mode,
            // [data] cullmode None, as on Surface (LS-34).
            double_sided: source.word("cullmode") == Some(crc32(b"None")),
        }
    }

    /// A Colorizer's colour (ObjectDiffuseColor) multiplies the vertex shader's colour o2 [game 0a0c30].
    pub fn tint(&mut self, rgb: Vec3) {
        let d = self.values.diffuse;
        self.values.diffuse = (d.truncate() * rgb).extend(d.w);
    }
}

// ---- The reflection pass [game 0057a540 OceanMapDraw, 0057a1b0 ReflectionMapDraw, 005797b0 its clip plane] ----
//
// [game] The game draws the level again into a 511x511 target from a mirrored view, with a D3D user clip
// plane at the sea (005797b0: SetClipPlane from the nearest OceanSurface's z axis DAT_00c02fb0 and position
// DAT_00c02fc0, offset by the material word 0xd5b86a32 = 0 for deep_six_01 [data, 005a2f03]; 00579bf0 turns it
// off after). The port: a second camera, the main one mirrored about the sea plane, whose projection's near
// plane is the sea plane (`ReflectionProjection`, an oblique near plane: the same cut as the clip plane).

/// The sea's meshes (to find the sea's height and to keep them out of the reflection).
#[derive(Component)]
pub struct OceanPart;

/// A placement with NoReflectionTag (`level_render::PlacedModel::no_reflection`): its meshes go to
/// `NOT_REFLECTED`.
#[derive(Component)]
pub struct NoReflection;

/// The reflection camera. The game's systems that look for "the" camera leave it out.
#[derive(Component)]
pub struct OceanReflection;

#[derive(Resource)]
struct Reflection {
    image: Handle<Image>,
    camera: Entity,
    /// The sea plane's height (Bevy y = game z).
    height: f32,
}

/// The reflection camera's projection: the main camera's perspective (its fov and its aspect ratio, so that the
/// picture lines up with the screen uv the sea samples it at, as the game's ReflectionMap is sampled at vPos),
/// with the near plane replaced by the sea plane (Lengyel's oblique near plane on Bevy's reversed infinite
/// perspective: clip z = w - a (C . v), so z <= w keeps C . v >= 0, above the sea, and z >= 0 keeps a (C . v) <= w,
/// true for every point in the frustum when a = cos of the half diagonal angle).
#[derive(Clone, Debug)]
struct ReflectionProjection {
    fov: f32,
    aspect: f32,
    near: f32,
    far: f32,
    /// The sea plane in the camera's view space (unit normal, pointing up).
    plane: Vec4,
}

impl ReflectionProjection {
    fn perspective(&self) -> Mat4 {
        Mat4::perspective_infinite_reverse_rh(self.fov, self.aspect, self.near)
    }
}

impl CameraProjection for ReflectionProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        let rows = self.perspective().transpose();
        let half = (self.fov * 0.5).tan();
        let a = 0.99 / (1.0 + half * half * (1.0 + self.aspect * self.aspect)).sqrt();
        // The z row becomes the w row (0, 0, -1, 0) minus a C.
        let z = Vec4::new(0.0, 0.0, -1.0, 0.0) - a * self.plane;
        Mat4::from_cols(rows.x_axis, rows.y_axis, z, rows.w_axis).transpose()
    }

    fn get_clip_from_view_for_sub(&self, _: &bevy::camera::SubCameraView) -> Mat4 {
        self.get_clip_from_view()
    }

    /// The target is square; the main view's aspect ratio is kept (set by `follow`).
    fn update(&mut self, _: f32, _: f32) {}

    fn far(&self) -> f32 {
        self.far
    }

    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [bevy::math::Vec3A; 8] {
        PerspectiveProjection { fov: self.fov, aspect_ratio: self.aspect, near: self.near, far: self.far, ..default() }
            .get_frustum_corners(z_near, z_far)
    }
}

/// Puts the sea, the sky and the NoReflectionTag placements on `NOT_REFLECTED`, and makes the reflection camera
/// once a sea exists.
fn setup(
    mut commands: Commands,
    reflection: Option<Res<Reflection>>,
    parts: Query<(Entity, &GlobalTransform, &Mesh3d), Added<OceanPart>>,
    skies: Query<Entity, Added<MeshMaterial3d<crate::sky::Sky>>>,
    tagged: Query<&Children, Added<NoReflection>>,
    meshes: Res<Assets<Mesh>>,
    main: Query<Entity, (With<Camera3d>, Without<OceanReflection>, Without<crate::duel::SecondView>)>,
    mut images: ResMut<Assets<Image>>,
) {
    let layer = bevy::camera::visibility::RenderLayers::layer(NOT_REFLECTED);
    // The main view draws `NOT_REFLECTED` whether or not the level has a sea: before 2026-10-18 it was
    // given the layer only with a sea, so on Cairo and US_Test (no sea) the sky domes moved there vanished.
    if !skies.is_empty() || !tagged.is_empty() || !parts.is_empty() {
        for camera in &main {
            commands.entity(camera).insert(bevy::camera::visibility::RenderLayers::from_layers(&[0, NOT_REFLECTED]));
        }
    }
    for sky in &skies {
        // [assumed] the sky domes are not drawn into the reflection: the specular only shows where the
        // reflection pass drew nothing [game 0a1d90], and a sky drawn everywhere would hide it.
        commands.entity(sky).insert(layer.clone());
    }
    for children in &tagged {
        for child in children.iter() {
            commands.entity(child).insert(layer.clone());
        }
    }
    let mut height = None;
    for (entity, at, mesh) in &parts {
        commands.entity(entity).insert(layer.clone());
        // [assumed] the sea plane is the tiles' own height (the game's is the nearest OceanSurface object's
        // position, 00ad5e20, not read): the middle of the first tile's bounds.
        if height.is_none()
            && let Some(aabb) = meshes.get(&mesh.0).and_then(|m| m.compute_aabb()) {
            height = Some(at.transform_point(Vec3::from(aabb.center)).y);
        }
    }
    let (Some(height), None) = (height, reflection) else { return };
    let mut image = Image::new_target_texture(REFLECTION_SIZE, REFLECTION_SIZE, bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb, None);
    image.sampler = bevy::image::ImageSampler::linear();
    let image = images.add(image);
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                // Before the main view, which samples it.
                order: -1,
                // [game 0057a1b0] FUN_00a97520(0, 0, 1.0): cleared to colour 0. [assumed] alpha 0 is "nothing drawn"
                // (the game's ReflectionDepthMap == 0).
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            bevy::camera::RenderTarget::Image(image.clone().into()),
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            Msaa::Off,
            bevy::camera::visibility::RenderLayers::layer(0),
            Projection::custom(ReflectionProjection { fov: 1.0, aspect: 1.0, near: 0.5, far: 1500.0, plane: Vec4::Y }),
            Transform::default(),
            OceanReflection,
            Name::new("ocean reflection camera"),
        ))
        .id();
    info!("ocean: reflection camera made, the sea plane at height {height:.3}");
    commands.insert_resource(Reflection { image, camera, height });
}

/// Each frame: the reflection camera is the main camera mirrored about the sea plane, and the sea's material
/// gets the picture and mReflectWorldView.
fn follow(
    reflection: Option<Res<Reflection>>,
    main: Query<(&Transform, &Projection), (With<Camera3d>, Without<OceanReflection>, Without<crate::duel::SecondView>)>,
    mut cameras: Query<(&mut Transform, &mut Projection, &mut Camera), With<OceanReflection>>,
    mut oceans: ResMut<Assets<Ocean>>,
) {
    let Some(reflection) = reflection else { return };
    let Ok((view, projection)) = main.single() else { return };
    let Ok((mut transform, mut own, mut camera)) = cameras.get_mut(reflection.camera) else { return };
    let Projection::Perspective(perspective) = projection else { return };
    let h = reflection.height;
    // Under the sea the pass would see nothing above it: it is not drawn (the sea is then black and specular).
    camera.is_active = view.translation.y > h;
    let mirror = |v: Vec3| Vec3::new(v.x, -v.y, v.z);
    let position = Vec3::new(view.translation.x, 2.0 * h - view.translation.y, view.translation.z);
    let forward = mirror(view.forward().into());
    let up = mirror(view.up().into());
    // A rotation cannot mirror: the camera keeps the mirrored forward and up, so its right is the mirror image's
    // left (sampled at 1 - u, flag MIRRORED_X).
    *transform = Transform::from_translation(position).looking_to(forward, up);
    let world_from_view = transform.to_matrix();
    // The sea plane y = h, (0, 1, 0, -h) in the world, in the camera's view space.
    let plane = world_from_view.transpose() * Vec4::new(0.0, 1.0, 0.0, -h);
    if let Projection::Custom(custom) = &mut *own
        && let Some(own) = custom.get_mut::<ReflectionProjection>()
    {
        *own = ReflectionProjection { fov: perspective.fov, aspect: perspective.aspect_ratio, near: perspective.near, far: perspective.far, plane };
    }
    // mReflectWorldView's two rows [assumed: where the game sets it is not found; taken as the true mirror view's
    // right and up, in the game's space]: the right of the mirror image is the mirrored main right.
    let game = |v: Vec3| Vec3::new(v.x, -v.z, v.y).extend(0.0);
    let rows = [game(mirror(view.right().into())), game(mirror(view.up().into()))];
    let flags = if camera.is_active { REFLECTION | MIRRORED_X } else { 0 };
    for (_, ocean) in oceans.iter_mut() {
        ocean.values.reflect_view = rows;
        ocean.values.flags.x = (ocean.values.flags.x & RIG) | flags;
        if ocean.reflection.as_ref() != Some(&reflection.image) {
            ocean.reflection = Some(reflection.image.clone());
        }
    }
}

pub fn plugin(app: &mut App) {
    app.add_plugins(MaterialPlugin::<Ocean>::default());
    let shader = Shader::from_wgsl(include_str!("ocean.wgsl"), "ocean.wgsl");
    app.world_mut().resource_mut::<Assets<Shader>>().insert(&OCEAN_SHADER, shader).expect("the ocean shader's handle");
    if !std::env::args().any(|a| a == "--no-ocean-reflection") {
        // `setup` after the transforms are propagated (a new tile's GlobalTransform is valid then); `follow`
        // before, so the camera it moves is propagated the same frame.
        app.add_systems(PostUpdate, (
            follow.before(bevy::transform::TransformSystems::Propagate),
            setup.after(bevy::transform::TransformSystems::Propagate),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parameter names are the shaders' constant names in lower case [data: their CRC-32 are the
    /// hashes of deep_six_01's material, `work\phase1-audit\ocean_params.txt`].
    #[test]
    fn parameter_names_hash_to_deep_six_material() {
        assert_eq!(crc32(b"wave_period3"), 0x575f_31fe);
        assert_eq!(crc32(b"normal_map1_t"), 0x0ed4_2756);
        assert_eq!(crc32(b"ocean_distortion_scale"), 0x27ad_29cf);
        assert_eq!(crc32(b"snell_amount"), 0xca80_689a);
        assert_eq!(crc32(b"fresnel_texture"), 0xe029_ec8b);
        assert_eq!(crc32(b"normal_map_texture0"), 0x94ac_83a8);
        assert!(is_ocean(crc32(b"ocean_water2")));
    }

    /// The reflection camera's oblique near plane is the sea plane [game 005797b0's clip plane]: a point above
    /// the sea is inside the clip volume (0 <= z <= w), a point below it is cut (z > w), and depth still falls
    /// with distance along a ray (reversed z).
    #[test]
    fn reflection_projection_cuts_at_the_sea() {
        let h = 0.0;
        // The mirror of an eye 10 m above the sea, looking down a little: 10 m under it, looking up a little.
        let transform = Transform::from_xyz(0.0, -10.0, 0.0).looking_to(Vec3::new(0.0, 0.3, -1.0), Vec3::Y);
        let world_from_view = transform.to_matrix();
        let plane = world_from_view.transpose() * Vec4::new(0.0, 1.0, 0.0, -h);
        let projection = ReflectionProjection { fov: 1.0, aspect: 16.0 / 9.0, near: 0.5, far: 1500.0, plane };
        let clip_from_world = projection.get_clip_from_view() * world_from_view.inverse();
        let clip = |p: Vec3| clip_from_world * p.extend(1.0);
        let inside = |c: Vec4| c.w > 0.0 && c.z >= 0.0 && c.z <= c.w && c.x.abs() <= c.w && c.y.abs() <= c.w;
        // Above the sea, in view, near and far.
        for p in [Vec3::new(0.0, 1.0, -40.0), Vec3::new(5.0, 20.0, -100.0), Vec3::new(-50.0, 200.0, -1000.0)] {
            assert!(inside(clip(p)), "{p} should be drawn: {}", clip(p));
        }
        // Below the sea, in view: cut by the plane.
        for p in [Vec3::new(0.0, -1.0, -40.0), Vec3::new(0.0, -8.0, -60.0)] {
            let c = clip(p);
            assert!(c.z > c.w, "{p} should be cut: {c}");
        }
        // Depth (z / w) falls with distance along one ray above the sea.
        let ray = |t: f32| { let c = clip(Vec3::new(0.0, -10.0, 0.0) + Vec3::new(0.0, 0.5, -1.0) * t); c.z / c.w };
        assert!(ray(40.0) > ray(80.0) && ray(80.0) > ray(400.0));
    }
}
