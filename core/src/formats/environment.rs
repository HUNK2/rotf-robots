//! A level's `Environment`: its light, fog, bloom, colour filter and exposure
//! (`docs/shaders.md` "The environment"). The class is `Environment` (factory
//! `FUN_005690a0`, loader `FUN_00567660`); a level's own is a root of one of its zone
//! pack's data chunks, with the flag `DEFAULT`, and the zone's first data chunk holds the
//! special ones every level has (`Environment_SpeedBlur`, `Environment_LowHeath`...).

use std::path::Path;

use glam::Vec3;

use super::lxb::{DataFile, Node};
use super::pack::{self, Pack};

/// The numbers are as the loader keeps them (what it scales is said on each).
#[derive(Clone, Debug, PartialEq)]
pub struct Environment {
    pub name: String,
    /// `ambient`, bytes over 255.
    pub ambient: Vec3,
    /// `shadowLight`: the way TO the sun (the light's z axis: its place over its
    /// distance from the middle of the level), its `colour` and `range`. [data]
    /// [assumed: `range` scales a directional light's colour; the light's update,
    /// `FUN_00569cd0`, is not read]
    pub to_sun: Vec3,
    pub sun_colour: Vec3,
    pub sun_range: f32,
    /// `sceneLights` without the sun: the fill (bounce) directionals. The sun is the entry
    /// that is the same stored object as `shadowLight`; the game skips that handle in
    /// `environment_irradiance_build` 00566e90 [game]. Each fill light's direction is the
    /// matrix's row 2, as for the sun [assumed: same convention as `to_sun`; not checked
    /// against the shader]. Its colour and range are as for the sun.
    pub fill_lights: Vec<FillLight>,
    /// `fog` (bytes over 255), `fogHotspot`, `fogRange`, `fogFalloff`, `fogHeight`.
    pub fog_colour: Vec3,
    pub fog_hotspot: f32,
    pub fog_range: f32,
    pub fog_falloff: f32,
    pub fog_height: f32,
    /// `bloomThreshold` x 0.01, `bloomBrightness`, `bloomSaturation` x 0.01, `bloomTint`
    /// bytes over 255, `bloomBlurLevel` (`FUN_00567660`). [game: the scales]
    pub bloom_threshold: f32,
    pub bloom_brightness: f32,
    pub bloom_saturation: f32,
    pub bloom_tint: Vec3,
    pub bloom_blur: f32,
    /// `colorFilterBrightness`, `Contrast`, `Saturation`, `Lightness`, as stored (127.5
    /// is the middle of each).
    pub filter_brightness: f32,
    pub filter_contrast: f32,
    pub filter_saturation: f32,
    pub filter_lightness: f32,
    /// [game 00547190] SMH and tint controls are multiplied by2/255 before upload.
    pub filter_tint: Vec3,
    pub filter_smh: [glam::Vec4; 3],
    /// Full source path, hashed by lower33 by the Environment loader00567660.
    pub filter_lut: Option<String>,
    /// `irisTargetLum`, `irisMin`, `irisMax`, `irisBloomGain`.
    pub iris_target: f32,
    pub iris_min: f32,
    pub iris_max: f32,
    pub iris_bloom_gain: f32,
    /// [game 00568c40..00568e95, the loader's stores, settled 2026-10-16] `irisSpeed` (+0x2d4, the rate),
    /// `irisResponsiveness` (+0x2d8, the pow exponent), `irisLumPower` (+0x2dc, the luminance pass's
    /// `LuminancePower`), `irisGate` (+0x2e0). `iris_enabled`: the IRIS flag (the loader's enable byte +0x2c4).
    pub iris_speed: f32,
    pub iris_responsiveness: f32,
    pub iris_lum_power: f32,
    pub iris_gate: f32,
    pub iris_enabled: bool,
    /// The SSAO flag and `ssaoFinalScale` (SSAOFinalScale of the ambient composite, effects 007650).
    pub ssao_enabled: bool,
    /// The BLOOM flag.
    pub bloom_enabled: bool,
    /// The DOF and DOF_USE_SOFTER flags and the DOF fields as stored: `dofBlurAmount`, `dofMaxBlur`,
    /// `dofNearFocus`, `dofFarFocus`, `dofNearFalloff`, `dofFarFalloff`, `dofKernelTap`.
    /// `shadowRange` and `shadowMapSize` (the sun's two shadow maps, the unpublished notes: 60 / 1320).
    pub shadow_range: f32,
    pub shadow_map_size: f32,
    pub dof_enabled: bool,
    pub dof_softer: bool,
    pub dof: [f32; 7],
    pub ssao_final_scale: f32,
}

/// One fill light of `sceneLights`: a directional Light. The colour is the Light's `colour`
/// (raw), the range its `range` (1.016 in the data); `to_light` is the matrix's row 2,
/// normalised. [data] for the fields; [assumed] for the direction's meaning.
#[derive(Clone, Debug, PartialEq)]
pub struct FillLight {
    pub colour: Vec3,
    pub range: f32,
    pub to_light: Vec3,
}

/// The Light an object carries: `things[0].attachments[*].thing` of type `Light`.
fn light_of(object: Node) -> Option<Node> {
    object.get("things")?.at(0)?.get("attachments")?.items().filter_map(|a| a.get("thing")).find(|t| t.type_name() == Some("Light"))
}

/// `sceneLights` without the sun. An entry is the sun when it is the same stored object as
/// `shadowLight` (its offset in the file). Entries with no matrix or no Light are skipped.
fn fill_lights(root: Node, sun: Node) -> Vec<FillLight> {
    let sun_at = sun.offset();
    root.get("sceneLights").map_or_else(Vec::new, |list| {
        list.items()
            .filter(|object| !(sun_at.is_some() && object.offset() == sun_at))
            .filter_map(|object| {
                let matrix = object.get("matrix").map(Node::floats).filter(|m| m.len() == 12)?;
                let light = light_of(object)?;
                let colour = light.get("colour").map(Node::floats).filter(|c| c.len() == 3)?;
                Some(FillLight {
                    colour: Vec3::new(colour[0], colour[1], colour[2]),
                    range: light.get("range").and_then(Node::float).unwrap_or(1.0),
                    to_light: Vec3::new(matrix[6], matrix[7], matrix[8]).normalize_or_zero(),
                })
            })
            .collect()
    })
}

fn bytes(node: Option<Node>) -> Vec3 {
    let v = node.map(Node::ints).unwrap_or_default();
    if v.len() >= 3 { Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32) / 255.0 } else { Vec3::ZERO }
}

fn read(name: &str, root: Node) -> Option<Environment> {
    let num = |field: &str| root.get(field).and_then(Node::float).unwrap_or(0.0);
    root.get("bloomThreshold")?;
    let sun = root.get("shadowLight")?;
    let matrix = sun.get("matrix").map(Node::floats).filter(|m| m.len() == 12)?;
    let light = light_of(sun)?;
    let colour = light.get("colour").map(Node::floats).filter(|c| c.len() == 3)?;
    Some(Environment {
        name: name.to_owned(),
        ambient: bytes(root.get("ambient")),
        to_sun: Vec3::new(matrix[6], matrix[7], matrix[8]).normalize_or_zero(),
        sun_colour: Vec3::new(colour[0], colour[1], colour[2]),
        sun_range: light.get("range").and_then(Node::float).unwrap_or(1.0),
        fill_lights: fill_lights(root, sun),
        fog_colour: bytes(root.get("fog")),
        fog_hotspot: num("fogHotspot"),
        fog_range: num("fogRange"),
        fog_falloff: num("fogFalloff"),
        fog_height: num("fogHeight"),
        bloom_threshold: num("bloomThreshold") * 0.01,
        bloom_brightness: num("bloomBrightness"),
        bloom_saturation: num("bloomSaturation") * 0.01,
        bloom_tint: bytes(root.get("bloomTint")),
        bloom_blur: num("bloomBlurLevel"),
        filter_brightness: num("colorFilterBrightness"),
        filter_contrast: num("colorFilterContrast"),
        filter_saturation: num("colorFilterSaturation"),
        filter_lightness: num("colorFilterLightness"),
        filter_tint: Vec3::new(num("colorFilterTintRed"), num("colorFilterTintGreen"), num("colorFilterTintBlue")),
        filter_smh: [
            glam::Vec4::new(num("colorFilterShadowsRed"), num("colorFilterShadowsGreen"), num("colorFilterShadowsBlue"), num("colorFilterShadowsSat")),
            glam::Vec4::new(num("colorFilterMidtonesRed"), num("colorFilterMidtonesGreen"), num("colorFilterMidtonesBlue"), num("colorFilterMidtonesSat")),
            glam::Vec4::new(num("colorFilterHighlightsRed"), num("colorFilterHighlightsGreen"), num("colorFilterHighlightsBlue"), num("colorFilterHighlightsSat")),
        ],
        filter_lut: root.get("colorFilterLUTName").and_then(Node::text),
        iris_target: num("irisTargetLum"),
        iris_min: num("irisMin"),
        iris_max: num("irisMax"),
        iris_bloom_gain: num("irisBloomGain"),
        iris_speed: num("irisSpeed"),
        iris_responsiveness: num("irisResponsiveness"),
        iris_lum_power: num("irisLumPower"),
        iris_gate: num("irisGate"),
        // [assumed] the enable byte is the IRIS flag (the loader sets +0x2c4 when it reads the iris).
        iris_enabled: root.get("flags").is_some_and(|f| f.flag_names().contains(&"IRIS")),
        ssao_enabled: root.get("flags").is_some_and(|f| f.flag_names().contains(&"SSAO")),
        ssao_final_scale: num("ssaoFinalScale"),
        bloom_enabled: root.get("flags").is_some_and(|f| f.flag_names().contains(&"BLOOM")),
        shadow_range: num("shadowRange"),
        shadow_map_size: num("shadowMapSize"),
        dof_enabled: root.get("flags").is_some_and(|f| f.flag_names().contains(&"DOF")),
        dof_softer: root.get("flags").is_some_and(|f| f.flag_names().contains(&"DOF_USE_SOFTER")),
        dof: ["dofBlurAmount", "dofMaxBlur", "dofNearFocus", "dofFarFocus", "dofNearFalloff", "dofFarFalloff", "dofKernelTap"].map(num),
    })
}

/// The level's own environment: the first root of the zone's data chunks that has a sun
/// (`shadowLight`). The special environments have none.
pub fn load(game_dir: &Path, level: &str) -> Result<Environment, String> {
    load_pack(&game_dir.join("levels").join(level).join("zone.str")).map_err(|e| format!("{level}: {e}"))
}

/// The first environment with a sun in any pack (the front end's `levels\frontend\mission.str` has no zone).
pub fn load_pack(path: &Path) -> Result<Environment, String> {
    let pack = Pack::open(path)?;
    for chunk in pack.of_type(pack::DATA) {
        // A chunk with no environment in it is not worth parsing: the field's name is in
        // the chunk's own name table as text.
        let data = pack.data(chunk);
        if !data.windows(14).any(|w| w == b"bloomThreshold") {
            continue;
        }
        let file = DataFile::parse(data.to_vec())?;
        for (hash, root) in file.root().fields() {
            let name = file.name(hash).map_or_else(|| format!("#{hash:08x}"), str::to_owned);
            if let Some(environment) = read(&name, root) {
                return Ok(environment);
            }
        }
    }
    Err("no environment with a sun".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the game install.
    #[test]
    fn the_west_town_has_a_low_warm_sun_and_a_blue_ambient() {
        if crate::install::missing() { return; }
        let e = load(crate::install::dir(), "us_west_town").unwrap();
        assert_eq!(e.name, "Environment01");
        assert!((e.ambient - Vec3::new(35.0, 54.0, 71.0) / 255.0).length() < 1e-4);
        assert!((e.to_sun - Vec3::new(-0.24337, 0.815195, 0.525575)).length() < 1e-3);
        assert!((e.sun_colour - Vec3::new(1.2902, 0.953922, 0.706863)).length() < 1e-4);
        assert!((e.bloom_threshold - 0.586).abs() < 1e-4 && e.bloom_brightness == 0.2 && e.bloom_saturation == 1.0);
        assert_eq!((e.fog_hotspot, e.fog_range, e.fog_falloff), (164.0, 700.0, 95.0));
        assert_eq!((e.filter_brightness, e.filter_contrast, e.filter_saturation), (142.0, 150.0, 126.0));
        assert!((e.iris_target - 0.368).abs() < 1e-4);
    }

    /// Needs the game install. Shanghai's default sun is the one in the dump at
    /// work/levellight/env_shanghai_all.txt (matrix row 2 = 0.448203, 0.547746, 0.706462);
    /// its sceneLights are the sun and four fill directionals (LT_side, LT_bounce,
    /// LT_side02, LT_side03). The sun must not be among the fill lights.
    #[test]
    fn shanghai_default_has_four_fill_lights_and_not_the_sun() {
        if crate::install::missing() { return; }
        let e = load(crate::install::dir(), "shanghai_01").unwrap();
        // [data] flags [AMBIENT, BACKGROUND, FOG, BLOOM, DOF, IRIS, SSAO, DOF_USE_SOFTER, DEFAULT, COLOR_FILTER_LUT].
        assert!(e.iris_enabled);
        for (got, want) in [(e.iris_speed, 0.03), (e.iris_responsiveness, 0.593), (e.iris_lum_power, 0.933), (e.iris_gate, 0.0005)] {
            assert!((got - want).abs() < 1e-6, "{got} {want}");
        }
        assert!((e.to_sun - Vec3::new(0.448203, 0.547746, 0.706462).normalize()).length() < 1e-3);
        assert_eq!(e.fill_lights.len(), 4);
        let side = &e.fill_lights[0];
        assert!((side.colour - Vec3::new(0.196108, 0.236682, 0.246826)).length() < 1e-4);
        assert!((side.to_light - Vec3::new(0.686123, -0.486740, 0.540666)).length() < 1e-3);
        assert!((side.range - 1.016).abs() < 1e-4);
        assert!(e.fill_lights.iter().all(|f| (f.colour - e.sun_colour).length() > 1e-4));
    }
}
