//! The iris: the game's adapted exposure (`IrisParams.x`), updated once per frame from the scene's
//! measured luminance. the unpublished notes section 5; settled further on 2026-10-16
//! (the unpublished notes "Iris").
//!
//! [game 00562c70 `iris_exposure_update`], in order:
//! 1. The scene is reduced to a 4x4 A8R8G8B8 target by 16-tap passes. The power pass (executable shader
//!    at file 0x71e740, `LuminancePower` = `irisLumPower`) takes 16 point taps of the source's RED channel
//!    and writes their power mean `(mean r^p)^(1/p)` (`00583420`, `00581490`); with p = 1 a plain 16-tap box
//!    (shader 0x7219a8) is used instead.
//! 2. The 4x4 is read back and histogrammed (`00581a00`); `lum = sum(bin * fraction) / 255`, the mean red
//!    byte over 255 (the sum starts at bin 0 of channel x, 00562dce).
//! 3. `desired = iris + speed * iris * (pow(target / (lum + 0.001), responsiveness) - 1)`.
//! 4. `desired` is clamped to [min, max]; the iris takes it only when it moves by at least `gate`.
//! 5. `IrisParams.x = iris` (1 when the iris is disabled).

/// The Environment's iris block (`formats::environment::Environment`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IrisParams {
    pub enabled: bool,
    pub target: f32,
    pub min: f32,
    pub max: f32,
    pub speed: f32,
    pub responsiveness: f32,
    pub gate: f32,
}

/// [assumed] The iris's value when a level starts (`DAT_00ce3ab4`'s first value is not read).
pub const START: f32 = 1.0;

/// One update of the controller: the next iris from the current one and the measured luminance (0..1).
pub fn step(iris: f32, lum: f32, p: &IrisParams) -> f32 {
    if !p.enabled {
        return 1.0;
    }
    let ratio = p.target / (lum + 0.001);
    let desired = (ratio.powf(p.responsiveness) * iris - iris) * p.speed + iris;
    let clamped = desired.max(p.min).min(p.max);
    if (clamped - iris).abs() >= p.gate { clamped } else { iris }
}

/// The luminance of the read-back 4x4 target: the mean of its red bytes over 255 [game 00581a00, 00562c70].
pub fn luminance(red_bytes: &[u8]) -> f32 {
    if red_bytes.is_empty() {
        return 0.0;
    }
    red_bytes.iter().map(|&b| b as f32).sum::<f32>() / red_bytes.len() as f32 / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [data] shanghai_01 Environment_Default.
    fn shanghai() -> IrisParams {
        IrisParams { enabled: true, target: 0.155, min: 0.98, max: 1.581, speed: 0.03, responsiveness: 0.593, gate: 0.0005 }
    }

    #[test]
    fn a_dark_scene_opens_the_iris_up_to_its_maximum() {
        let p = shanghai();
        let mut iris = START;
        // A scene darker than the target: the iris grows each frame, bounded by irisMax.
        for _ in 0..2000 { iris = step(iris, 0.05, &p); }
        assert!((iris - p.max).abs() < 1e-4, "{iris}");
    }

    #[test]
    fn the_first_step_follows_the_formula() {
        let p = shanghai();
        let next = step(1.0, 0.05, &p);
        let want = 1.0 + 0.03 * ((0.155f32 / 0.051).powf(0.593) - 1.0);
        assert!((next - want).abs() < 1e-6, "{next} {want}");
    }

    #[test]
    fn a_bright_scene_closes_it_to_its_minimum_and_small_moves_are_gated() {
        let p = shanghai();
        let mut iris = 1.3;
        for _ in 0..2000 { iris = step(iris, 0.6, &p); }
        assert!((iris - p.min).abs() < 1e-4);
        // At the target the desired change is ~0: below the gate, the iris holds.
        assert_eq!(step(1.2, 0.154, &p), 1.2);
    }

    #[test]
    fn disabled_is_one_and_luminance_is_the_mean_red_byte() {
        assert_eq!(step(1.4, 0.1, &IrisParams { enabled: false, ..shanghai() }), 1.0);
        assert!((luminance(&[0, 255, 51, 102]) - 0.4).abs() < 1e-6);
    }
}
