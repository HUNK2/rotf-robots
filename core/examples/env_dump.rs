//! Prints a level's default Environment (sun, ambient, fills, fog, bloom, filter, iris).
//! `cargo run -p tf2-core --example env_dump -- <level folder> [<level folder>...]`, e.g. `cairo_city shanghai_01`.
use std::path::Path;

fn main() {
    let game = Path::new("C:/Games2");
    for level in std::env::args().skip(1) {
        match tf2_core::formats::environment::load(game, &level) {
            Ok(e) => {
                println!("== {level}: {}", e.name);
                println!("ambient {:?}  to_sun {:?}  sun {:?} x {}", e.ambient, e.to_sun, e.sun_colour, e.sun_range);
                for f in &e.fill_lights { println!("  fill {:?} x {} to {:?}", f.colour, f.range, f.to_light); }
                println!("fog {:?} hotspot {} range {} falloff {} height {}", e.fog_colour, e.fog_hotspot, e.fog_range, e.fog_falloff, e.fog_height);
                println!("bloom thr {} bright {} sat {} tint {:?}", e.bloom_threshold, e.bloom_brightness, e.bloom_saturation, e.bloom_tint);
                println!("filter b/c/s/l {} {} {} {} tint {:?} smh {:?} lut {:?}", e.filter_brightness, e.filter_contrast, e.filter_saturation, e.filter_lightness, e.filter_tint, e.filter_smh, e.filter_lut);
                println!("iris on {} target {} min {} max {} gain {}", e.iris_enabled, e.iris_target, e.iris_min, e.iris_max, e.iris_bloom_gain);
            }
            Err(err) => println!("== {level}: {err}"),
        }
    }
}
