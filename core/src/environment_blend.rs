//! The game's list of ACTIVE environments and how each one's weight moves, ported for the
//! EnvironmentTrigger (and the state environments: FlashBang, SpeedBlur, Overdrive, LowHeath).
//! `notes\render-144.md` section 2.
//!
//! [game 00565c00] `environment_blend_in`: if the environment is already in the list (matched by its
//! name hash, the Environment's first dword), only its rate is reset to `1 / max(blendDuration, 1e-5)`
//! and its weight is kept. Otherwise a node {env, weight 0, rate 1/max(blendDuration, 1e-5), scale 1}
//! is inserted BEFORE the first node whose environment's priority (int, env+0x388) is >= its own, so
//! the list runs in ascending priority and a newcomer goes in front of equals.
//! [game 007e5020] the EnvironmentTrigger's Untrigger: the node of each of its environments gets
//! `rate = -1 / max(blendOutDuration, 1e-5)` (env+0x384). Nothing else changes.
//! [game 00565da0] each frame: `weight = min(1, weight + rate * dt)`; a node whose weight is <= 0
//! leaves the list. The blend factor of a node is `scale * weight` (scale is 1 from 00565c00).
//!
//!
//! What the blend does to the values, for bloom and the colour filter ([`blend_look`]; `notes\render-144.md`
//! section 3):
//! [game 00565da0] the working block starts as the base environment (00d16fc0, the level's DEFAULT one);
//! then, list by list (00d16fc4 then 00d16fd0) and node by node, `light_group_copy` 005645d0 folds the node's
//! environment in with `t = scale * weight` taken BEFORE the node's weight is stepped this frame.
//! [game 005645d0 lines 69-118] group 8 (the env's BLOOM flag, payload dwords 0x5c..0x6f = env+0x180..+0x1cc:
//! bloomTint, bloomThreshold, bloomBrightness, bloomSaturation, bloomBlurLevel, the radial blur): every value
//! `(1 - t) * working + t * env`, except radialBlurIterations (0x67) and the light direction's w (0x6f), which
//! take the env's value. An environment without the flag leaves the group as it is.
//! [game 00566840, 00566690, 00566560] the colour filter is NOT lerped: the base and every node with weight
//! != 0 whose env has COLOR_FILTER or LUT (flags 0x20 | 0x4000) is drawn as a layer, in list order, with
//! D3D blend BLENDFACTOR / INVBLENDFACTOR and BLENDFACTOR = `(round(w*255) << 24) | 0xffffff` (disasm
//! 00566580..005665f8). The colour factor is 0xff, 1, so a layer REPLACES the LUT's colour and only its alpha
//! sees the weight: the last layer wins outright. A node at weight 1 also drops the layers below it.

use std::path::Path;

use glam::{Vec3, Vec4};

use crate::formats::environment::Environment;
use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};

/// The colour filter controls of one environment (env+0x1fc..+0x244 and the LUT name). [game 00567660]
#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub lightness: f32,
    pub tint: Vec3,
    pub smh: [Vec4; 3],
    pub lut: Option<String>,
}

/// The values of an environment that the port blends. Scales as `environment::read` (the loader's).
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// The BLOOM flag (0x8): group 8 is folded in. [data] flag names as decoded.
    pub bloom: bool,
    /// COLOR_FILTER or LUT (0x20 | 0x4000): the environment is a colour layer.
    pub colour_layer: bool,
    pub bloom_tint: Vec3,
    pub bloom_threshold: f32,
    pub bloom_brightness: f32,
    pub bloom_saturation: f32,
    pub bloom_blur: f32,
    pub filter: Filter,
}

impl Look {
    /// The base environment's values (the port's `world::look`). [assumed] the base's flags do not gate it:
    /// 00565fd0 copies the whole base and 00566840 always queues the base layer (bit 0 set first).
    pub fn of_environment(e: &Environment) -> Look {
        Look {
            bloom: e.bloom_enabled,
            colour_layer: true,
            bloom_tint: e.bloom_tint,
            bloom_threshold: e.bloom_threshold,
            bloom_brightness: e.bloom_brightness,
            bloom_saturation: e.bloom_saturation,
            bloom_blur: e.bloom_blur,
            filter: Filter {
                brightness: e.filter_brightness,
                contrast: e.filter_contrast,
                saturation: e.filter_saturation,
                lightness: e.filter_lightness,
                tint: e.filter_tint,
                smh: e.filter_smh,
                lut: e.filter_lut.clone(),
            },
        }
    }
}

/// [game 00567660] An environment root's blended values, with the scales of `environment::read`
/// (threshold and saturation x 0.01, the tint's bytes / 255). [assumed] an absent field reads 0, as there.
pub fn look_of(root: Node) -> Look {
    let num = |field: &str| root.get(field).and_then(Node::float).unwrap_or(0.0);
    let flags: Vec<&str> = root.get("flags").map(|f| f.flag_names()).unwrap_or_default();
    let tint = root.get("bloomTint").map(Node::ints).unwrap_or_default();
    Look {
        bloom: flags.contains(&"BLOOM"),
        colour_layer: flags.contains(&"COLOR_FILTER") || flags.contains(&"LUT"),
        bloom_tint: if tint.len() >= 3 { Vec3::new(tint[0] as f32, tint[1] as f32, tint[2] as f32) / 255.0 } else { Vec3::ZERO },
        bloom_threshold: num("bloomThreshold") * 0.01,
        bloom_brightness: num("bloomBrightness"),
        bloom_saturation: num("bloomSaturation") * 0.01,
        bloom_blur: num("bloomBlurLevel"),
        filter: Filter {
            brightness: num("colorFilterBrightness"),
            contrast: num("colorFilterContrast"),
            saturation: num("colorFilterSaturation"),
            lightness: num("colorFilterLightness"),
            tint: Vec3::new(num("colorFilterTintRed"), num("colorFilterTintGreen"), num("colorFilterTintBlue")),
            smh: [
                Vec4::new(num("colorFilterShadowsRed"), num("colorFilterShadowsGreen"), num("colorFilterShadowsBlue"), num("colorFilterShadowsSat")),
                Vec4::new(num("colorFilterMidtonesRed"), num("colorFilterMidtonesGreen"), num("colorFilterMidtonesBlue"), num("colorFilterMidtonesSat")),
                Vec4::new(num("colorFilterHighlightsRed"), num("colorFilterHighlightsGreen"), num("colorFilterHighlightsBlue"), num("colorFilterHighlightsSat")),
            ],
            lut: root.get("colorFilterLUTName").and_then(Node::text),
        },
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    (1.0 - t) * a + t * b
}

/// [game 00565da0, 005645d0 group 8, 00566840..00566560] The frame's bloom and colour filter: the base, then
/// every active node (with its factor before this frame's step) in list order. Nodes whose name has no spec
/// are skipped (the game's unresolved names never reach the list).
pub fn blend_look(base: &Look, list: &EnvBlend, specs: &[EnvSpec]) -> Look {
    let mut out = base.clone();
    let mut filter = base.filter.clone();
    for node in &list.active {
        let Some(spec) = specs.iter().find(|s| s.name == node.name) else { continue };
        let (env, t) = (&spec.look, node.factor());
        if env.bloom {
            out.bloom_tint = out.bloom_tint.lerp(env.bloom_tint, t);
            out.bloom_threshold = lerp(out.bloom_threshold, env.bloom_threshold, t);
            out.bloom_brightness = lerp(out.bloom_brightness, env.bloom_brightness, t);
            out.bloom_saturation = lerp(out.bloom_saturation, env.bloom_saturation, t);
            out.bloom_blur = lerp(out.bloom_blur, env.bloom_blur, t);
        }
        if env.colour_layer && t != 0.0 {
            filter = env.filter.clone();
        }
    }
    out.filter = filter;
    out
}

/// The four fields of an Environment the blend list reads. [game 00567660 for the offsets]
#[derive(Clone, Debug, PartialEq)]
pub struct EnvSpec {
    /// The root's name hash: CRC-32 of e.g. `Environment_BarrierCollide` (what an EnvironmentTrigger's
    /// `environments` list holds and what `environment_by_name` 0056a790 matches).
    pub name: u32,
    pub label: String,
    /// `priority` (env+0x388, an int). [data] Environment_Default 128 in shanghai_01.
    pub priority: i32,
    /// `blendDuration` (env+0x380), seconds.
    pub blend_in: f32,
    /// `blendOutDuration` (env+0x384), seconds.
    pub blend_out: f32,
    /// The values it blends ([`look_of`]).
    pub look: Look,
}

/// One node of the active list.
#[derive(Clone, Debug, PartialEq)]
pub struct Active {
    pub name: u32,
    pub priority: i32,
    pub weight: f32,
    pub rate: f32,
    pub scale: f32,
}

impl Active {
    /// The node's blend factor, `scale * weight` [game 00565da0].
    pub fn factor(&self) -> f32 {
        self.scale * self.weight
    }
}

/// [game] The rate of a blend of `duration` seconds: the reciprocal of `max(duration, 1e-5)`.
pub fn rate(duration: f32) -> f32 {
    1.0 / duration.max(1e-5)
}

/// The list at 00d16fd0, in ascending priority.
#[derive(Clone, Debug, Default)]
pub struct EnvBlend {
    pub active: Vec<Active>,
}

impl EnvBlend {
    /// [game 00565c00] Blend an environment in (insert, or restart the rate of the one already there).
    pub fn blend_in(&mut self, env: &EnvSpec) {
        if let Some(node) = self.active.iter_mut().find(|n| n.name == env.name) {
            node.rate = rate(env.blend_in);
            return;
        }
        let at = self.active.iter().position(|n| env.priority <= n.priority).unwrap_or(self.active.len());
        self.active.insert(at, Active { name: env.name, priority: env.priority, weight: 0.0, rate: rate(env.blend_in), scale: 1.0 });
    }

    /// [game 007e5020] Blend an environment out: its node's rate becomes `-1 / max(blendOutDuration, 1e-5)`.
    /// An environment not in the list is left alone.
    pub fn blend_out(&mut self, env: &EnvSpec) {
        if let Some(node) = self.active.iter_mut().find(|n| n.name == env.name) {
            node.rate = -rate(env.blend_out);
        }
    }

    /// [game 00565da0] One frame: every weight moves by `rate * dt` (at most 1) and the nodes at or
    /// below 0 leave the list.
    pub fn step(&mut self, dt: f32) {
        for node in &mut self.active {
            node.weight = (node.weight + node.rate * dt).min(1.0);
        }
        self.active.retain(|n| n.weight > 0.0);
    }

    /// The blend factor of an environment, 0 when it is not active.
    pub fn factor(&self, name: u32) -> f32 {
        self.active.iter().find(|n| n.name == name).map_or(0.0, Active::factor)
    }
}

/// The Environment roots of a zone pack, in pack order and then file order (as `environment::load`
/// scans them). A root counts when it has a `bloomThreshold` or a `blendDuration` field.
pub fn zone_specs(zone: &Path) -> Result<Vec<EnvSpec>, String> {
    let pack = Pack::open(zone)?;
    let mut specs = Vec::new();
    for chunk in pack.of_type(pack::DATA) {
        let data = pack.data(chunk);
        if !data.windows(13).any(|w| w == b"blendDuration") {
            continue;
        }
        let file = DataFile::parse(data.to_vec())?;
        for (hash, root) in file.root().fields() {
            if root.get("blendDuration").is_none() && root.get("bloomThreshold").is_none() {
                continue;
            }
            let label = file.name(hash).map_or_else(|| format!("#{hash:08x}"), str::to_owned);
            specs.push(spec_of(hash, label, root));
        }
    }
    Ok(specs)
}

/// [game 00567660] The three fields, with the loader's defaults when one is absent: [assumed] 0 for each
/// (the loader's own defaults were not read for these three).
fn spec_of(name: u32, label: String, root: Node) -> EnvSpec {
    let num = |field: &str| root.get(field).and_then(|n| n.floats().first().copied());
    EnvSpec {
        name,
        label,
        // [game 00567660] the loader stores an int; the data may hold it as an int or a float.
        priority: root
            .get("priority")
            .and_then(|n| n.ints().first().copied().or_else(|| n.floats().first().map(|f| *f as i64)))
            .unwrap_or(0) as i32,
        blend_in: num("blendDuration").unwrap_or(0.0),
        blend_out: num("blendOutDuration").unwrap_or(0.0),
        look: look_of(root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(bloom: bool, colour_layer: bool, value: f32) -> Look {
        Look {
            bloom,
            colour_layer,
            bloom_tint: Vec3::splat(value),
            bloom_threshold: value,
            bloom_brightness: value,
            bloom_saturation: value,
            bloom_blur: value,
            filter: Filter { brightness: value, contrast: value, saturation: value, lightness: value, tint: Vec3::splat(value), smh: [Vec4::splat(value); 3], lut: None },
        }
    }

    fn env(name: u32, priority: i32, blend_in: f32, blend_out: f32) -> EnvSpec {
        EnvSpec { name, label: String::new(), priority, blend_in, blend_out, look: plain(true, true, 1.0) }
    }

    #[test]
    fn bloom_lerps_by_the_factor_and_needs_the_flag() {
        let base = plain(true, true, 0.0);
        let specs = vec![
            EnvSpec { look: plain(true, false, 1.0), ..env(1, 0, 1.0, 1.0) },
            EnvSpec { look: plain(false, false, 9.0), ..env(2, 5, 1.0, 1.0) },
        ];
        let mut list = EnvBlend::default();
        list.blend_in(&specs[0]);
        list.blend_in(&specs[1]);
        list.step(0.25);
        let out = blend_look(&base, &list, &specs);
        assert_eq!((out.bloom_threshold, out.bloom_saturation, out.bloom_tint), (0.25, 0.25, Vec3::splat(0.25)));
        assert_eq!(out.filter, base.filter, "no colour layer");
        // Two bloom environments fold in one after the other.
        let specs = vec![EnvSpec { look: plain(true, false, 1.0), ..env(1, 0, 1.0, 1.0) }, EnvSpec { look: plain(true, false, 2.0), ..env(2, 5, 1.0, 1.0) }];
        let mut list = EnvBlend::default();
        list.blend_in(&specs[0]);
        list.blend_in(&specs[1]);
        list.step(0.5);
        // ((0 * 0.5 + 1 * 0.5) * 0.5 + 2 * 0.5) = 1.25
        assert_eq!(blend_look(&base, &list, &specs).bloom_brightness, 1.25);
    }

    #[test]
    fn the_last_colour_layer_with_weight_replaces_the_filter_outright() {
        let base = plain(true, true, 0.0);
        let specs = vec![EnvSpec { look: plain(false, true, 3.0), ..env(1, 0, 10.0, 10.0) }];
        let mut list = EnvBlend::default();
        list.blend_in(&specs[0]);
        assert_eq!(blend_look(&base, &list, &specs).filter, base.filter, "weight 0 is not a layer");
        list.step(0.032);
        let out = blend_look(&base, &list, &specs);
        assert!(list.factor(1) < 0.01);
        assert_eq!(out.filter, specs[0].look.filter, "the layer's colour factor is 1");
    }

    #[test]
    fn insert_runs_in_ascending_priority_and_a_newcomer_goes_before_equals() {
        let mut list = EnvBlend::default();
        list.blend_in(&env(1, 130, 1.0, 1.0));
        list.blend_in(&env(2, 128, 1.0, 1.0));
        list.blend_in(&env(3, 130, 1.0, 1.0));
        list.blend_in(&env(4, 200, 1.0, 1.0));
        let order: Vec<u32> = list.active.iter().map(|n| n.name).collect();
        assert_eq!(order, vec![2, 3, 1, 4]);
        assert!(list.active.iter().all(|n| n.weight == 0.0 && n.scale == 1.0));
    }

    #[test]
    fn blend_in_twice_keeps_the_weight_and_resets_the_rate() {
        let mut list = EnvBlend::default();
        let e = env(7, 0, 0.5, 0.25);
        list.blend_in(&e);
        list.step(0.25);
        assert_eq!(list.factor(7), 0.5);
        list.blend_out(&e);
        assert_eq!(list.active[0].rate, -4.0);
        list.blend_in(&e);
        assert_eq!((list.active.len(), list.active[0].weight, list.active[0].rate), (1, 0.5, 2.0));
    }

    #[test]
    fn weights_clamp_at_one_and_leave_at_zero() {
        let mut list = EnvBlend::default();
        // BarrierCollide's blendDuration 0.05 [data]: full after two 0.032 s frames.
        let e = env(9, 0, 0.05, 0.05);
        list.blend_in(&e);
        list.step(0.032);
        assert!((list.factor(9) - 0.64).abs() < 1e-6);
        list.step(0.032);
        assert_eq!(list.factor(9), 1.0);
        list.blend_out(&e);
        list.step(0.032);
        assert!((list.factor(9) - 0.36).abs() < 1e-5);
        list.step(0.032);
        assert!(list.active.is_empty());
    }

    #[test]
    fn a_zero_duration_is_instant_through_the_1e5_floor() {
        let mut list = EnvBlend::default();
        list.blend_in(&env(5, 0, 0.0, -1.0));
        assert_eq!(list.active[0].rate, 1e5);
        list.step(0.032);
        assert_eq!(list.factor(5), 1.0);
        list.blend_out(&env(5, 0, 0.0, -1.0));
        assert_eq!(list.active[0].rate, -1e5);
        list.step(0.032);
        assert!(list.active.is_empty());
    }

    /// Needs the game install: shanghai_01's zone holds Environment_BarrierCollide, the name its
    /// EnvironmentTrigger lists (0x13b09e14 = crc32 of the name).
    #[test]
    fn shanghai_has_the_barrier_collide_environment() {
        let specs = zone_specs(Path::new(r"C:\Games2\levels\shanghai_01\zone.str")).unwrap();
        let barrier = specs.iter().find(|s| s.label == "Environment_BarrierCollide").expect("BarrierCollide");
        assert_eq!(barrier.name, 0x13b0_9e14);
        assert_eq!(barrier.name, crate::formats::hash::crc32(b"Environment_BarrierCollide"));
        assert!((barrier.blend_in - 0.05).abs() < 1e-6, "{barrier:?}");
        // [data] flags BLOOM and COLOR_FILTER only, bloom threshold 85.9 x 0.01.
        assert!(barrier.look.bloom && barrier.look.colour_layer, "{:?}", barrier.look);
        assert!((barrier.look.bloom_threshold - 0.859).abs() < 1e-4, "{:?}", barrier.look);
        let default = specs.iter().find(|s| s.label == "Environment_Default").expect("Default");
        assert_eq!(default.priority, 128);
    }
}
