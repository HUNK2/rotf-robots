//! The placed `Colorizer`: a weighted random colour picked once at load for the Instance that carries it
//! (Shanghai's parked taxis and traffic cars, an awning). `notes\level-animthing-effects.md` sections 3,
//! "Colorizer render (LS-52)", "Colorizer constant (LS-56)" and "Colorizer target (LS-58)".
//!
//! [data] `colours`: a list of { `colour` (RGBA bytes), `likelyhood` (a weight) }.
//! [game 007db420] The pick: the weights are summed; the generator advances once
//! (`seed = seed * 0x343fd + 0x269ec3`); `x = (time_s & 0x7fff) ^ ((seed >> 16) & 0x7fff)`; the threshold is
//! `x / 32768 * total`; the first entry whose running sum reaches it wins, the last one otherwise.
//! RGB is byte / 255; the alpha is 1 [game 007db420, LS-52].
//! [game 007db420] checked 2026-10-20: the threshold's bits (`(time << 8 ^ seed >> 8) & 0x7fff00`, the mantissa
//! over 2^23) equal the `x / 32768` above.
//! [assumed] the generator's start (the game seeds it from the clock) and `time_s` are the port's own fixed
//! values, so a level looks the same each run; the game's colours differ from run to run anyway.
//! QUEUED: settles: a recording of the placed Colorizer colours across two loads of one level. The seed
//! DAT_00c02ef0 is one generator shared by eight functions (007db420 and 0073c010, 00738e00, 011904f0,
//! 0093ca40, 0091d090, 00919510, 009186f0, by `decomp_lookup.py grep DAT_00c02ef0`), so its value at a
//! pick depends on every earlier draw of the run; the clock term is `_time64` at load (007db420). Neither
//! is in the executable, so only a recording of the placed colours settles them.

use crate::formats::lxb::Node;

#[derive(Clone, Debug, PartialEq)]
pub struct Colorizer {
    /// (RGB as byte / 255, weight), in the data's order.
    pub choices: Vec<([f32; 3], f32)>,
}

impl Colorizer {
    /// A `Colorizer` node's colour list, or None when it has no entries.
    pub fn read(node: Node) -> Option<Self> {
        let choices: Vec<([f32; 3], f32)> = node
            .get("colours")?
            .items()
            .filter_map(|entry| {
                let c = entry.get("colour")?.ints();
                if c.len() < 3 {
                    return None;
                }
                // [game 007db420] an integer weight, or a float cast to one (LS-52).
                let weight = entry.get("likelyhood").and_then(|w| w.int().map(|v| v as f32).or_else(|| w.float())).unwrap_or(0.0);
                Some(([c[0], c[1], c[2]].map(|v| v as f32 / 255.0), weight))
            })
            .collect();
        (!choices.is_empty()).then_some(Self { choices })
    }

    /// The weighted pick, advancing `seed` once. `time_s` is the clock term of the game's draw.
    pub fn pick(&self, seed: &mut u32, time_s: u32) -> [f32; 3] {
        *seed = seed.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        let x = (time_s & 0x7fff) ^ ((*seed >> 16) & 0x7fff);
        let total: f32 = self.choices.iter().map(|(_, w)| w).sum();
        let threshold = x as f32 / 32768.0 * total;
        let mut running = 0.0;
        for (colour, weight) in &self.choices {
            running += weight;
            if running >= threshold {
                return *colour;
            }
        }
        self.choices.last().map_or([1.0; 3], |(c, _)| *c)
    }
}

/// [assumed] The port's start for the pick generator (the game's DAT_00c02ef0, shared by eight functions;
/// its value at a pick is the run's draw count, see the module note).
/// QUEUED: settles: a recording of the Colorizer colours across two loads of one level.
pub const SEED: u32 = 1;
/// [assumed] The port's clock term (the game uses `_time64` in seconds at load, 007db420).
/// QUEUED: settles: a recording of the Colorizer colours across two loads (the time is run time).
pub const TIME_S: u32 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    fn taxi() -> Colorizer {
        // [data] shanghai_01 TaxiCar_White01's list.
        let c = |r: f32, g: f32, b: f32| [r / 255.0, g / 255.0, b / 255.0];
        Colorizer { choices: vec![(c(202., 202., 207.), 20.0), (c(254., 254., 254.), 20.0), (c(0., 0., 0.), 20.0), (c(0., 14., 46.), 15.0), (c(110., 0., 0.), 15.0)] }
    }

    #[test]
    fn the_pick_follows_the_running_weights() {
        let t = taxi();
        // x = (seed >> 16) & 0x7fff after one step from seed 1: 0x343fd + 0x269ec3 = 0x29e2c0 -> x = 0x29 = 41.
        let mut seed = 1;
        let first = t.pick(&mut seed, 0);
        assert_eq!(seed, 0x0029_e2c0);
        // threshold = 41 / 32768 * 90, below the first weight: the first colour.
        assert_eq!(first, t.choices[0].0);
    }

    /// [data] shanghai_01: the placed models under a Colorizer's Instance get its colour. PRINTS the counts.
    #[test]
    fn shanghai_taxis_are_tinted() {
        let zone = std::path::PathBuf::from(crate::install::dir_string()).join(r"levels\shanghai_01\zone.str");
        if !zone.is_file() {
            return;
        }
        let level = crate::level_render::load(&zone).unwrap();
        let tinted: Vec<_> = level.placed.iter().filter(|p| p.tint.is_some()).collect();
        let instances: std::collections::BTreeSet<_> = tinted.iter().filter_map(|p| p.owner).collect();
        let mut colours = std::collections::BTreeMap::new();
        for p in &tinted { *colours.entry(format!("{:?}", p.tint.unwrap().map(|v| (v * 255.0).round() as u8))).or_insert(0) += 1; }
        let taxis = tinted.iter().filter(|p| p.model.to_lowercase().starts_with("taxi_car_white")).count();
        for p in tinted.iter().filter(|p| p.lod == 0).take(8) {
            println!("colorizer: {} at ({:.1}, {:.1}, {:.1}) colour {:?}", p.model, p.world[9], p.world[10], p.world[11], p.tint.unwrap());
        }
        println!("colorizer: {} placed models under {} Instances, {taxis} taxi models; by colour {colours:?}", tinted.len(), instances.len());
        assert!(instances.len() >= 50, "the 51 taxis and the traffic cars");
        assert!(colours.len() >= 4, "several colours are picked");
    }

    #[test]
    fn every_colour_is_reached_over_many_picks_in_proportion() {
        let t = taxi();
        let mut seed = SEED;
        let mut counts = [0usize; 5];
        for _ in 0..9000 {
            let c = t.pick(&mut seed, TIME_S);
            counts[t.choices.iter().position(|(x, _)| *x == c).unwrap()] += 1;
        }
        // Weights 20/20/20/15/15 of 90: about 2000/2000/2000/1500/1500.
        for (n, want) in counts.iter().zip([2000.0, 2000.0, 2000.0, 1500.0, 1500.0]) {
            assert!((*n as f32 - want).abs() < want * 0.1, "{counts:?}");
        }
    }
}
