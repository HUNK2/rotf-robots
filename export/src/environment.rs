//! A stage's look: the level's `Environment` (its sun, fill lights, ambient, fog, bloom, colour
//! filter and exposure) as JSON, `docs/FORMAT.md` "The environment". The reference's test arena
//! borrows `us_west_town`'s (the Bevy game's `world.rs` `LEVEL`).

use std::path::Path;

use glam::Vec3;
use serde_json::{json, Value};
use tf2_core::formats::environment::{self, Environment};

/// The level whose environment the reference's test arena borrows.
pub const ARENA_LEVEL: &str = "us_west_town";

fn v3(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

/// The level's own environment, as the game's loader keeps it (`docs/shaders.md`).
pub fn export(install: &Path, level: &str) -> Result<Value, String> {
    Ok(describe(&environment::load(install, level)?, level))
}

pub fn describe(e: &Environment, level: &str) -> Value {
    json!({
        "level": level,
        "name": e.name,
        "sun": {"to_sun": v3(e.to_sun), "colour": v3(e.sun_colour), "range": e.sun_range},
        "fill_lights": e.fill_lights.iter().map(|f| json!({"to_light": v3(f.to_light), "colour": v3(f.colour), "range": f.range})).collect::<Vec<_>>(),
        "ambient": v3(e.ambient),
        "fog": {"colour": v3(e.fog_colour), "hotspot": e.fog_hotspot, "range": e.fog_range, "falloff": e.fog_falloff, "height": e.fog_height},
        "bloom": {"enabled": e.bloom_enabled, "threshold": e.bloom_threshold, "brightness": e.bloom_brightness,
            "saturation": e.bloom_saturation, "tint": v3(e.bloom_tint), "blur": e.bloom_blur},
        "colour_filter": {"brightness": e.filter_brightness, "contrast": e.filter_contrast, "saturation": e.filter_saturation,
            "lightness": e.filter_lightness, "tint": v3(e.filter_tint),
            "shadows": e.filter_smh[0].to_array(), "midtones": e.filter_smh[1].to_array(), "highlights": e.filter_smh[2].to_array(),
            "lut": e.filter_lut},
        "iris": {"enabled": e.iris_enabled, "target": e.iris_target, "min": e.iris_min, "max": e.iris_max,
            "bloom_gain": e.iris_bloom_gain, "speed": e.iris_speed, "responsiveness": e.iris_responsiveness,
            "lum_power": e.iris_lum_power, "gate": e.iris_gate},
        "shadow": {"range": e.shadow_range, "map_size": e.shadow_map_size},
        "ssao": {"enabled": e.ssao_enabled, "final_scale": e.ssao_final_scale},
        "dof": {"enabled": e.dof_enabled, "softer": e.dof_softer, "values": e.dof},
    })
}
