//! Level lighting, engine-free (glam only): the nine-term irradiance that a level's fill
//! lights make (`Irradiance`), and the light rig that an `Environment` fills (sun, ambient,
//! irradiance, fog) with the game's reset and blend (`LightRig`).
//! Read from the game's code: notes/level-lighting-setters.md LS-11, LS-13, LS-14 and LS-15
//! (notes/checks-2026-10-13-irradiance.md, the emulator check of `add_light`).
//! Not wired into the game yet.
//!
//! Units: the game's own. Directions are unit vectors in x right, y forward, z up.
//! Colours are the Light's colour with its fourth lane as the Light stores it (see
//! `LightRig::from_environment` for the one mapping that is not read).

use glam::{Vec3, Vec4};

use crate::formats::environment::Environment;

/// The square coefficient's constant: `0.07`. [game 00557300, constant `00b34e10` = 0.07]
const SQUARE_OFFSET: f32 = 0.07;
/// The square and linear coefficient: `0.49`. [game 00557300, constant `00b34e00` = 0.49]
const HALF: f32 = 0.49;
/// The cross coefficient: `0.98` (2 x 0.49). [game 00557300, the `2.0` at `00b38850`]
const CROSS: f32 = 0.98;

/// Nine float4 of directional light, in the game's order: x^2, y^2, z^2, xy, yz, zx, x, y, z.
/// The four lanes are the colour's four lanes (r, g, b, w); nothing is normalised.
/// [game 00557300] writes these at env+0x2f0..0x37f (`00566e90`, the Environment's
/// AmbientIrradiance). The vertex shader's constant term (c22) is [unknown] and not ported:
/// the caller adds AmbientColor or the constant term where the shader does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Irradiance {
    pub k: [Vec4; 9],
}

impl Irradiance {
    pub const ZERO: Irradiance = Irradiance { k: [Vec4::ZERO; 9] };

    /// Adds one directional light, exactly as 00557300 does. With d = direction and c = colour:
    /// `k0 += c (0.49 dx^2 + 0.07)`, `k1` and `k2` the same for dy and dz, `k3 += c 0.98 dx dy`,
    /// `k4 += c 0.98 dy dz`, `k5 += c 0.98 dz dx`, `k6 += c 0.49 dx`, `k7`, `k8` for dy and dz.
    /// [game 00557300] (the coefficients and the order are from the disassembly; the emulator
    /// reference is in the tests). The direction is used as given: no normalise, no sign flip.
    pub fn add_light(&mut self, colour: Vec4, direction: Vec3) {
        let d = direction;
        let square = |v: f32| HALF * v * v + SQUARE_OFFSET;
        self.k[0] += colour * square(d.x);
        self.k[1] += colour * square(d.y);
        self.k[2] += colour * square(d.z);
        self.k[3] += colour * (CROSS * d.x * d.y);
        self.k[4] += colour * (CROSS * d.y * d.z);
        self.k[5] += colour * (CROSS * d.z * d.x);
        self.k[6] += colour * (HALF * d.x);
        self.k[7] += colour * (HALF * d.y);
        self.k[8] += colour * (HALF * d.z);
    }

    /// The sum over the fill lights, as 00566e90 builds it: start from zero and add every light
    /// except the sun. `sun` is the index of the sun in `lights`, or `None` when there is none
    /// to skip. Each light is (colour, direction). [game 00566e90: the skip is the handle test
    /// `cmp edi, [env+0x3a0]`, read in LS-15 section 1d]
    pub fn from_fill_lights(lights: &[(Vec4, Vec3)], sun: Option<usize>) -> Self {
        let mut out = Self::ZERO;
        for (index, &(colour, direction)) in lights.iter().enumerate() {
            if Some(index) != sun {
                out.add_light(colour, direction);
            }
        }
        out
    }

    /// The vertex shader's sum for a normal `n`:
    /// `c0 x^2 + c1 y^2 + c2 z^2 + c3 xy + c4 yz + c5 zx + c6 x + c7 y + c8 z`.
    /// For a unit `n` and one light this is `L (0.49 (n.d)^2 + 0.49 (n.d) + 0.07)`.
    /// [assumed] The shader normalises `n` (LS-15 section 3, item 3); for a non-unit `n` the
    /// square terms give `0.07 |n|^2` where the closed form has `0.07`. The constant term
    /// (c22) is not included; it is [unknown].
    pub fn evaluate(&self, n: Vec3) -> Vec4 {
        let k = &self.k;
        k[0] * (n.x * n.x)
            + k[1] * (n.y * n.y)
            + k[2] * (n.z * n.z)
            + k[3] * (n.x * n.y)
            + k[4] * (n.y * n.z)
            + k[5] * (n.z * n.x)
            + k[6] * n.x
            + k[7] * n.y
            + k[8] * n.z
    }

    /// `(1 - t) a + t b`, each of the nine float4 in turn.
    pub fn lerp(a: Irradiance, b: Irradiance, t: f32) -> Irradiance {
        let mut out = Self::ZERO;
        for i in 0..9 {
            out.k[i] = a.k[i] * (1.0 - t) + b.k[i] * t;
        }
        out
    }
}

/// The lighting the game keeps for one frame (the working block of `light_update_frame`):
/// the sun, the ambient, the irradiance and the fog. [game 005640e0] for the defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightRig {
    /// LightColor: the sun's colour (four lanes).
    pub sun_colour: Vec4,
    /// LightDir: the way to the sun (unit length after a blend).
    pub sun_dir: Vec3,
    /// AmbientColor.
    pub ambient_colour: Vec4,
    /// AmbientIrradiance: the nine float4 of the fill lights.
    pub irradiance: Irradiance,
    /// The fog's colour and its fields. Only `fog_height` has a default written by the
    /// reset [game 005640e0, store at state +0x1b0 = 1000]; the other fog fields are zero
    /// [assumed: the reset writes nothing to them].
    pub fog_colour: Vec4,
    /// Fog near: the Environment's `fogHotspot` [assumed mapping].
    pub fog_near: f32,
    /// Fog far: the Environment's `fogRange`, as an absolute distance [assumed].
    pub fog_far: f32,
    /// Fog falloff: the Environment's `fogFalloff` / 100 [game 00567660, the x0.01 scale].
    pub fog_falloff: f32,
    /// Fog height: the Environment's `fogHeight` [game 00567660 stores it as the fourth lane].
    pub fog_height: f32,
}

impl LightRig {
    /// The defaults the game's reset writes [game 005640e0, table in LS-11 section 1]:
    /// LightColor (1,1,1,1); LightDir (1,1,1)/sqrt 3; AmbientColor 0.5 in all four lanes;
    /// AmbientIrradiance zero; fog height 1000.
    pub fn reset() -> Self {
        LightRig {
            sun_colour: Vec4::ONE,
            sun_dir: Vec3::ONE.normalize(),
            ambient_colour: Vec4::splat(0.5),
            irradiance: Irradiance::ZERO,
            fog_colour: Vec4::ZERO,
            fog_near: 0.0,
            fog_far: 0.0,
            fog_falloff: 0.0,
            fog_height: 1000.0,
        }
    }

    /// `(1 - t) self + t other` for every field, then the sun direction is renormalised when
    /// its length is above 1e-8. [game 005645d0 light_group_copy: the lerp of colour, direction
    /// and the nine float4, and `fVar7 > 1e-08` for the renormalise] The game does this under
    /// flag tests (0x20000, 0x80000, 0x40000) that this port does not carry: all fields are
    /// blended here [assumed: the flags are set on the path that reaches this].
    pub fn blend(&mut self, other: &LightRig, t: f32) {
        let s = 1.0 - t;
        self.sun_colour = self.sun_colour * s + other.sun_colour * t;
        self.sun_dir = self.sun_dir * s + other.sun_dir * t;
        let length = self.sun_dir.length();
        if length > 1e-8 {
            self.sun_dir /= length;
        }
        self.ambient_colour = self.ambient_colour * s + other.ambient_colour * t;
        self.irradiance = Irradiance::lerp(self.irradiance, other.irradiance, t);
        self.fog_colour = self.fog_colour * s + other.fog_colour * t;
        self.fog_near = self.fog_near * s + other.fog_near * t;
        self.fog_far = self.fog_far * s + other.fog_far * t;
        self.fog_falloff = self.fog_falloff * s + other.fog_falloff * t;
        self.fog_height = self.fog_height * s + other.fog_height * t;
    }

    /// FogRange as the lit pixel shaders read it [game 05cb30_ps:257-260, 038900_ps:203-206,
    /// 0ab3b0_ps:321-324]: x = near, y = slope, z unused (0), w = height. [game LS-7] near is
    /// `fog_near`, the slope is 1 / (far - near) and the height is `fog_height`.
    /// [stand-in] the slope is 0 when far is not beyond near (no ramp): the game's value there
    /// is unread (no guard was found in the setter's notes), so this is a placeholder.
    pub fn fog_range(&self) -> Vec4 {
        let span = self.fog_far - self.fog_near;
        let slope = if span > 0.0 { 1.0 / span } else { 0.0 };
        Vec4::new(self.fog_near, slope, 0.0, self.fog_height)
    }

    /// FogColor as the lit pixel shaders read it [game 05cb30_ps:264, 038900_ps:210]: rgb the
    /// fog colour, w the falloff (the maximum opacity, `fog_falloff` = Environment / 100).
    pub fn fog_color(&self) -> Vec4 {
        self.fog_colour.truncate().extend(self.fog_falloff)
    }

    /// The fog's blend amount `t` for a pixel at `distance` from the eye and at `height` (the
    /// game's z, up), as the lit techniques compute it [game 05cb30_ps:257-263, 038900_ps:
    /// 203-208, 0ab3b0_ps:321-327]:
    /// `t = saturate((d - near) * slope) * falloff * saturate(1 - (z / height)^2)`.
    /// The colour is then `lerp(lit, fog colour, t)`, before exposure. The distance is the
    /// eye-to-pixel length (`|c3 - v3|`) and z the pixel's world height [game, the lit vertex
    /// shaders write the world position to texcoord3, 05c290_vs:75, 0382c0_vs:52-54].
    pub fn fog_factor(&self, distance: f32, height: f32) -> f32 {
        let range = self.fog_range();
        let by_distance = ((distance - range.x) * range.y).clamp(0.0, 1.0) * self.fog_falloff;
        let r = height / range.w;
        by_distance * (1.0 - r * r).clamp(0.0, 1.0)
    }

    /// Fills the rig from a decoded Environment (its default one, or any other).
    /// [assumed] The mapping of the data's fields to the runtime's lanes is not read (the
    /// Light loader that fills light+0x80..0xb8 from colour, range and matrix is unread):
    /// - the sun colour is `sun_colour` with the fourth lane `sun_range` (the 1.016 of the
    ///   data, which level-lighting section 2 suggests is light+0x8c);
    /// - the sun direction is `to_sun` (matrix row 2) as the data gives it;
    /// - the ambient is `ambient` with fourth lane 1;
    /// - each fill light is (colour, range) with direction `to_light`, the sun excluded
    ///   (`Environment::fill_lights` already leaves it out);
    /// - the fog colour has fourth lane 1; fog near / far / height are the raw values, and
    ///   falloff is divided by 100 [game 00567660].
    pub fn from_environment(env: &Environment) -> Self {
        let fills: Vec<(Vec4, Vec3)> = env.fill_lights.iter().map(|f| (f.colour.extend(f.range), f.to_light)).collect();
        LightRig {
            sun_colour: env.sun_colour.extend(env.sun_range),
            sun_dir: env.to_sun,
            ambient_colour: env.ambient.extend(1.0),
            irradiance: Irradiance::from_fill_lights(&fills, None),
            fog_colour: env.fog_colour.extend(1.0),
            fog_near: env.fog_hotspot,
            fog_far: env.fog_range,
            fog_falloff: env.fog_falloff / 100.0,
            fog_height: env.fog_height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Compares every lane of nine float4 with the expected values, to 1e-6.
    fn check(ir: &Irradiance, expect: [[f32; 4]; 9]) {
        for (i, want) in expect.iter().enumerate() {
            let got = ir.k[i].to_array();
            for lane in 0..4 {
                assert!((got[lane] - want[lane]).abs() < 1e-6, "term {i} lane {lane}: got {} want {}", got[lane], want[lane]);
            }
        }
    }

    /// The five emulator reference cases of notes/checks-2026-10-13-irradiance.md section 2.
    #[test]
    fn add_light_matches_the_emulator_case_1() {
        let mut ir = Irradiance::ZERO;
        ir.add_light(Vec4::ONE, Vec3::Z);
        let z = [0.07; 4];
        check(&ir, [z, z, [0.56; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.49; 4]]);
    }

    #[test]
    fn add_light_matches_the_emulator_case_2() {
        let mut ir = Irradiance::ZERO;
        ir.add_light(Vec4::new(1.0, 0.5, 0.25, 1.0), Vec3::X);
        let z = [0.0; 4];
        check(
            &ir,
            [
                [0.56, 0.28, 0.14, 0.56],
                [0.07, 0.035, 0.0175, 0.07],
                [0.07, 0.035, 0.0175, 0.07],
                z,
                z,
                z,
                [0.49, 0.245, 0.1225, 0.49],
                z,
                z,
            ],
        );
    }

    #[test]
    fn add_light_matches_the_emulator_case_3() {
        let mut ir = Irradiance::ZERO;
        ir.add_light(Vec4::ONE, Vec3::new(1.0, 2.0, 3.0).normalize());
        check(
            &ir,
            [
                [0.105; 4],
                [0.21; 4],
                [0.385; 4],
                [0.14; 4],
                [0.42; 4],
                [0.21; 4],
                [0.130958; 4],
                [0.261916; 4],
                [0.392874; 4],
            ],
        );
    }

    #[test]
    fn add_light_matches_the_emulator_case_4() {
        let mut ir = Irradiance::ZERO;
        ir.add_light(Vec4::ONE, -Vec3::new(1.0, 2.0, 2.0) / 3.0);
        check(
            &ir,
            [
                [0.1244444; 4],
                [0.2877778; 4],
                [0.2877778; 4],
                [0.2177778; 4],
                [0.4355556; 4],
                [0.2177778; 4],
                [-0.1633333; 4],
                [-0.3266667; 4],
                [-0.3266667; 4],
            ],
        );
    }

    #[test]
    fn add_light_matches_the_emulator_case_5() {
        let mut ir = Irradiance::ZERO;
        ir.add_light(Vec4::new(2.0, 3.0, 0.5, 1.5), Vec3::new(0.6, 0.0, 0.8));
        check(
            &ir,
            [
                [0.4928001, 0.7392001, 0.1232, 0.3696001],
                [0.14, 0.21, 0.035, 0.105],
                [0.7672001, 1.1508, 0.1918, 0.5754001],
                [0.0; 4],
                [0.0; 4],
                [0.9408001, 1.4112, 0.2352, 0.7056],
                [0.5880001, 0.8820001, 0.147, 0.441],
                [0.0; 4],
                [0.784, 1.176, 0.196, 0.5880001],
            ],
        );
    }

    /// For a unit n and one light, the shader's sum is L (0.49 (n.d)^2 + 0.49 (n.d) + 0.07).
    #[test]
    fn evaluate_is_the_quadratic_in_n_dot_d() {
        let colour = Vec4::new(1.0, 0.5, 0.25, 1.0);
        let d = Vec3::new(0.6, 0.0, 0.8);
        let n = Vec3::new(1.0, 2.0, 2.0) / 3.0;
        let mut ir = Irradiance::ZERO;
        ir.add_light(colour, d);
        let nd = n.dot(d);
        let want = colour * (0.49 * nd * nd + 0.49 * nd + 0.07);
        assert!((ir.evaluate(n) - want).abs().max_element() < 1e-6);
    }

    #[test]
    fn from_fill_lights_skips_the_sun() {
        let sun = (Vec4::new(9.0, 9.0, 9.0, 1.0), Vec3::Y);
        let fill = (Vec4::new(0.2, 0.3, 0.4, 1.0), Vec3::new(0.0, 0.6, 0.8));
        let mut only_fill = Irradiance::ZERO;
        only_fill.add_light(fill.0, fill.1);
        assert_eq!(Irradiance::from_fill_lights(&[sun, fill], Some(0)), only_fill);
        assert_eq!(Irradiance::from_fill_lights(&[], None), Irradiance::ZERO);
    }

    #[test]
    fn reset_writes_the_game_defaults() {
        let r = LightRig::reset();
        assert_eq!(r.sun_colour, Vec4::ONE);
        assert!((r.sun_dir - Vec3::splat(1.0 / 3f32.sqrt())).length() < 1e-6);
        assert!((r.sun_dir.length() - 1.0).abs() < 1e-6);
        assert_eq!(r.ambient_colour, Vec4::splat(0.5));
        assert_eq!(r.irradiance, Irradiance::ZERO);
        assert_eq!(r.fog_height, 1000.0);
    }

    #[test]
    fn blend_lerps_and_renormalises_the_sun_direction() {
        let a = LightRig::reset();
        let mut b = LightRig::reset();
        b.sun_dir = Vec3::X;
        b.sun_colour = Vec4::ZERO;
        let mut mid = a;
        mid.blend(&b, 0.5);
        assert!((mid.sun_colour - Vec4::splat(0.5)).abs().max_element() < 1e-6);
        assert!((mid.sun_dir.length() - 1.0).abs() < 1e-6);
        let s = Vec3::ONE.normalize() * 0.5 + Vec3::X * 0.5;
        assert!((mid.sun_dir - s.normalize()).length() < 1e-6);

        let mut at_zero = a;
        at_zero.blend(&b, 0.0);
        assert!((at_zero.sun_dir - a.sun_dir).length() < 1e-6);
        assert_eq!(at_zero.sun_colour, a.sun_colour);
        assert_eq!(at_zero.irradiance, a.irradiance);
        let mut at_one = a;
        at_one.blend(&b, 1.0);
        assert!((at_one.sun_dir - Vec3::X).length() < 1e-6);
    }

    #[test]
    fn lerp_of_irradiance_is_componentwise() {
        let mut a = Irradiance::ZERO;
        a.add_light(Vec4::ONE, Vec3::Z);
        let mid = Irradiance::lerp(a, Irradiance::ZERO, 0.25);
        assert!((mid.k[8] - Vec4::splat(0.75 * 0.49)).abs().max_element() < 1e-6);
    }

    /// A rig with the fog fields set, the rest at the reset's values.
    fn fog_rig(near: f32, far: f32, falloff: f32, height: f32) -> LightRig {
        let mut r = LightRig::reset();
        r.fog_colour = Vec4::new(0.2, 0.3, 0.4, 1.0);
        r.fog_near = near;
        r.fog_far = far;
        r.fog_falloff = falloff;
        r.fog_height = height;
        r
    }

    #[test]
    fn fog_is_zero_before_near() {
        let r = fog_rig(100.0, 400.0, 0.9, 1000.0);
        assert_eq!(r.fog_factor(0.0, 0.0), 0.0);
        assert_eq!(r.fog_factor(99.0, 0.0), 0.0);
        assert_eq!(r.fog_factor(100.0, 0.0), 0.0);
    }

    #[test]
    fn fog_is_full_strength_at_and_beyond_far_at_height_zero() {
        let r = fog_rig(100.0, 400.0, 0.9, 1000.0);
        assert!((r.fog_factor(400.0, 0.0) - 0.9).abs() < 1e-6);
        assert!((r.fog_factor(5000.0, 0.0) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn fog_ramps_linearly_between_near_and_far() {
        let r = fog_rig(100.0, 400.0, 0.9, 1000.0);
        // Halfway between near and far, with the slope 1 / (far - near).
        assert!((r.fog_range().y - 1.0 / 300.0).abs() < 1e-9);
        assert!((r.fog_factor(250.0, 0.0) - 0.45).abs() < 1e-6);
    }

    #[test]
    fn fog_height_term_is_zero_at_z_equal_height() {
        let r = fog_rig(100.0, 400.0, 0.9, 130.0);
        assert!(r.fog_factor(5000.0, 130.0).abs() < 1e-6);
        assert!(r.fog_factor(5000.0, -130.0).abs() < 1e-6);
        // Half the height gives 1 - 0.25 = 0.75 of the distance term.
        assert!((r.fog_factor(5000.0, 65.0) - 0.9 * 0.75).abs() < 1e-6);
        // Above the layer the height term is clamped at zero, not negative.
        assert!(r.fog_factor(5000.0, 400.0).abs() < 1e-6);
    }

    #[test]
    fn fog_range_and_colour_fields_match_the_shader_layout() {
        let r = fog_rig(164.0, 700.0, 0.95, 310.0);
        assert_eq!(r.fog_range().x, 164.0);
        assert!((r.fog_range().y - 1.0 / 536.0).abs() < 1e-9);
        assert_eq!(r.fog_range().z, 0.0);
        assert_eq!(r.fog_range().w, 310.0);
        assert_eq!(r.fog_color(), Vec4::new(0.2, 0.3, 0.4, 0.95));
    }

    #[test]
    fn fog_slope_is_zero_when_far_is_not_beyond_near() {
        // [stand-in] the placeholder for a zero or negative span.
        let r = fog_rig(300.0, 300.0, 0.9, 1000.0);
        assert_eq!(r.fog_range().y, 0.0);
        assert_eq!(r.fog_factor(10_000.0, 0.0), 0.0);
    }

    /// Needs the game install. Prints the Shanghai_01 default Environment's fog fields and the
    /// fog factor at a few points, for notes/level-lighting-setters.md ("LS-29"). Run with
    /// --nocapture.
    #[test]
    fn print_shanghai_fog() {
        let env = crate::formats::environment::load(Path::new(r"C:\Games2"), "shanghai_01").unwrap();
        let rig = LightRig::from_environment(&env);
        println!("environment {} fogHotspot {} fogRange {} fogFalloff {} fogHeight {}", env.name, env.fog_hotspot, env.fog_range, env.fog_falloff, env.fog_height);
        println!("fog colour bytes/255 {:?} (w = falloff/100 {})", env.fog_colour.to_array(), rig.fog_falloff);
        println!("FogRange {:?} FogColor {:?}", rig.fog_range().to_array(), rig.fog_color().to_array());
        for (d, z) in [(100.0, 0.0), (300.0, 0.0), (700.0, 0.0), (700.0, 130.0), (700.0, 310.0)] {
            println!("fog_factor(d {d}, z {z}) = {}", rig.fog_factor(d, z));
        }
    }

    /// Needs the game install. Prints the Shanghai_01 default Environment's fill lights and
    /// evaluate() at six normals, for notes/level-lighting-setters.md ("LS-16"). Run with
    /// --nocapture.
    #[test]
    fn print_shanghai_default_fill_irradiance() {
        let env = crate::formats::environment::load(Path::new(r"C:\Games2"), "shanghai_01").unwrap();
        let rig = LightRig::from_environment(&env);
        println!("environment {} fill lights {}", env.name, env.fill_lights.len());
        for (i, f) in env.fill_lights.iter().enumerate() {
            println!("fill {i}: colour {:?} range {} to_light {:?}", f.colour.to_array(), f.range, f.to_light.to_array());
        }
        println!("sun colour {:?} dir {:?}", rig.sun_colour.to_array(), rig.sun_dir.to_array());
        let normals = [("+z", Vec3::Z), ("-z", -Vec3::Z), ("+x", Vec3::X), ("-x", -Vec3::X), ("+y", Vec3::Y), ("-y", -Vec3::Y)];
        for (label, n) in normals {
            println!("evaluate {label}: {:?}", rig.irradiance.evaluate(n).to_array());
        }
    }
}
