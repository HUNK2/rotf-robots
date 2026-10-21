//! Deferred point lights: the OMNI, SPOT and CAPSULE Light objects of a zone (and a mission),
//! the per-light shader constants the game uploads, and the shading the light passes do
//! (engine-free, glam only). The game uses the gated placed loader and camera-based selection.
//!
//! Read from the game's code and listings: notes/level-deferred-lighting.md (LS-33: the
//! shaders and the composite) and its LS-35 section (LS35.1 the loader 00563870, LS35.2 the
//! record writers 0054c260 omni, 0054c7f0 spot, 0054cd70 capsule, LS35.5 for the porter).
//! Listings: work/shaders/effects/008250_ps.asm (omni), 008e70_ps.asm (spot), 009cb0_ps.asm
//! (capsule, traced line by line for the representative-point specular).
//!
//! Units: game space, x right, y forward, z up, metres. `LevelLight` holds the DATA values of
//! one Light. `ShaderLight` is the record the game uploads for that light at one camera (the
//! colour is faded by the distance to the eye, and far lights have no record at all).
//!
//! Output convention: the shading functions return the FULL diffuse and FULL specular of one
//! light at one point. The light pass writes 0.5 x diffuse and 0.25 x specular, and the
//! composite multiplies the diffuse buffer by 2 and the specular by 4 (LS-33 B.3), so the
//! pair cancels: the functions return the value the composite would see, not the pass value.
//! The normal is used as given (the deferred path does not renormalise N'; LS-33 A).
//!
//! Tags: `[game <address>]` read in code, `[data]` from the pack, `[assumed]` a guess (the
//! note section says what settles it), `[stand-in]` a placeholder marked in the code.

use std::collections::HashSet;
use std::path::PathBuf;

use glam::{Vec3, Vec4};

use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};
use crate::level_render::{bind_of, compose, flat, flat32, matrix_of, Mat, IDENTITY, KEY_SCALE};

/// The walk's depth limit, as `level_render::MAX_DEPTH` (80).
const WALK_DEPTH: u32 = 80;

/// [assumed] A bound on the calls to `Walk::visit` in one data chunk. Shanghai's zone and mission
/// packs need far fewer; the bound stops a walk that is not finite (see the LS-38 note) and
/// reports where it was instead of running out of memory.
const VISIT_LIMIT: u64 = 5_000_000;

/// The three point light types the deferred passes draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightKind {
    Omni,
    Spot,
    Capsule,
}

/// One Light object's DATA values, with its world placement. Angles are the data's full
/// cone angles in radians (the writer halves them, see `spot_params`).
#[derive(Clone, Debug, PartialEq)]
pub struct LevelLight {
    pub kind: LightKind,
    /// The Light's position (`Light+0xa0`): the world matrix's row 3. [game 00563870] see `placement`.
    pub position: Vec3,
    /// The Light's direction (`Light+0xb0`): the world matrix's row 2, not normalised.
    /// Used by the spot as its cone axis. [game 00563870] the row (the loader stores it raw, no sign change).
    /// Second pass (2026-10-20): the spot shader 008e70 (work\shaders\..., `SpotLightDirection` c10) takes
    /// the cone term as ramp(L . c10, cos(falloff/2), cos(hot/2)) with L = normalize(SpotLightPosition - P),
    /// and the diffuse as N . c10; `spot_diffuse_specular` is the same. [game listing 008e70] So the shader
    /// needs c10 to point from the surface toward the light. [game 0054c7f0] The writer (the spot record,
    /// rec+0x50) copies Light+0xb0 raw (the loader's row 2, w 0), with no sign change: the game's own c10
    /// is this row, so the port's raw row matches. The game's data convention for the row's meaning is
    /// the one `to_sun` uses (row 2 points toward the source), not checked further. 008250 is another
    /// variant with a projected texture (s0) and no c10 cone.
    pub direction: Vec3,
    /// `colour` (`Light+0x80`), rgb, may exceed 1.
    pub colour: Vec3,
    /// `colour2` (`Light+0x90`), the capsule's colour at its far end. Zero in the Shanghai data.
    pub colour2: Vec3,
    /// `range` (`+0xc4`), `range2` (`+0xc8`), `attenuate` (`+0xcc`), `attenuate2` (`+0xd0`).
    pub range: f32,
    pub range2: f32,
    pub attenuate: f32,
    pub attenuate2: f32,
    /// `hotspot` (`+0xd4`) and `falloff` (`+0xd8`): full cone angles, radians.
    pub hotspot: f32,
    pub falloff: f32,
    /// `|offset|` (`+0xe8`, stored as an absolute value): the capsule's half length.
    pub offset_len: f32,
    /// `affectDiffuse`, `affectSpecular`, `ambientOnly` (`+0xec`, `+0xf0`, `+0xf4`).
    pub affect_diffuse: bool,
    pub affect_specular: bool,
    pub ambient_only: bool,
    /// The capsule's axis: normalized Light+0xb0, as 0054d269..0054d294 passes it through
    /// 00579740. The parent matrix convention follows `placement`'s existing assumption.
    pub capsule_axis: Vec3,
}

/// One light's shader record: the constants the game's writer uploads (the names are those
/// of the listings). The colour already carries the far fade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShaderLight {
    pub kind: LightKind,
    /// `c9`: the Light position (omni, spot) or the capsule's segment start `Pc`.
    pub position: Vec3,
    /// Spot: `c10.xyz`, the cone axis as uploaded (the raw `Light+0xb0`). Capsule: `c10.xyz`
    /// = `A`, the whole segment `2 |offset| axis`. Omni: unused, zero.
    pub direction: Vec3,
    /// Capsule: `c10.w` = `1 / |A|^2`. Others: zero.
    pub w10: f32,
    /// Omni `c10`, spot `c11`, capsule `c11` (the colour at u = 0), all with the fade.
    pub colour: Vec3,
    /// Capsule `c12` (the colour at u = 1), with the same fade. Zero for the others.
    pub colour2: Vec3,
    /// Omni `c11` = (inner^2, outer^2, 1/(outer^2 - inner^2), 0); spot `c12` = (cos inner half
    /// angle, cos outer half angle, inner^2, outer^2); capsule `c13` = (inner0, outer0,
    /// inner1, outer1), LINEAR radii.
    pub params: Vec4,
}

const FADE_SCALE: f32 = 0.02;
const FAR_LIMIT: f32 = 50.0;

/// `1 - (0.02 D / r)^2`, the far fade on the colour, or None when `D / r >= 50` (no record).
/// `D` is the distance from the eye to the light, `r` the light's reach. [game 0054c31f: the
/// 0.02 at [00cad454]; 0054c760: the 1.0 divisor] [game LS-35 LS35.2: the gate `D / range < 50`]
/// The fade is zero at D = 50 r, so the cut is continuous.
pub fn far_fade(distance: f32, reach: f32) -> Option<f32> {
    if !(reach > 0.0) {
        return None;
    }
    let ratio = distance / reach;
    if ratio >= FAR_LIMIT {
        return None;
    }
    let k = FADE_SCALE * ratio;
    Some(1.0 - k * k)
}

/// The omni `c11` record. [game 0054c260: `local_70 = att*att`, `fStack_6c = range*range`,
/// `uStack_68 = divps([00b38840] = 1.0, range^2 - att^2)`; LS35.2 OMNI]. The shader uses x and y.
pub fn omni_params(attenuate: f32, range: f32) -> Vec4 {
    let inner2 = attenuate * attenuate;
    let outer2 = range * range;
    Vec4::new(inner2, outer2, 1.0 / (outer2 - inner2), 0.0)
}

/// The spot `c12` record: the cones are the HALF angles, cos(hotspot * 0.5) and cos(falloff * 0.5)
/// [game 0054c7f0 through the cos calls at 00563c57..00563c84, the 0.5 at [00b38830]; LS35.1],
/// then the squared radii (inner, outer). [game 0054c7f0 rec+0x70, LS35.2 SPOT]
pub fn spot_params(hotspot: f32, falloff: f32, attenuate: f32, range: f32) -> Vec4 {
    Vec4::new((hotspot * 0.5).cos(), (falloff * 0.5).cos(), attenuate * attenuate, range * range)
}

/// The capsule `c13` record: the four radii, linear [game 0054cd70 rec+0x80, LS35.2 CAPSULE;
/// the shader squares them, LS-33 A.4]. Order (inner end 0, outer end 0, inner end 1, outer end 1).
pub fn capsule_params(attenuate: f32, range: f32, attenuate2: f32, range2: f32) -> Vec4 {
    Vec4::new(attenuate, range, attenuate2, range2)
}

/// The capsule segment for a Light at `position` with unit `axis` and half length `half`:
/// `Pc = position - half * axis`, `A = 2 half axis`, `c10.w = 1 / |A|^2`.
/// [game 0054cd70 rec+0x40, rec+0x50: `rcpps` plus one Newton step, `fStack_84 = (1 - |A|^2 r) r + r`;
/// LS35.2 CAPSULE]. A zero-length segment gives an infinite `w10`, as the game's does.
pub fn capsule_segment(position: Vec3, axis: Vec3, half: f32) -> (Vec3, Vec3, f32) {
    let a = axis * (2.0 * half);
    (position - axis * half, a, 1.0 / a.dot(a))
}

/// The omni record for one light at `eye` (the camera). None when far. [game 0054c260 LS35.2]
pub fn omni_record(light: &LevelLight, eye: Vec3) -> Option<ShaderLight> {
    let fade = far_fade((light.position - eye).length(), light.range)?;
    Some(ShaderLight {
        kind: LightKind::Omni,
        position: light.position,
        direction: Vec3::ZERO,
        w10: 0.0,
        colour: light.colour * fade,
        colour2: Vec3::ZERO,
        params: omni_params(light.attenuate, light.range),
    })
}

/// The spot record for one light at `eye`. None when far. [game 0054c7f0 LS35.2 SPOT; the
/// far gate is the omni's: [assumed] the spot writer's gate is not read separately].
pub fn spot_record(light: &LevelLight, eye: Vec3) -> Option<ShaderLight> {
    let fade = far_fade((light.position - eye).length(), light.range)?;
    Some(ShaderLight {
        kind: LightKind::Spot,
        position: light.position,
        direction: light.direction,
        w10: 0.0,
        colour: light.colour * fade,
        colour2: Vec3::ZERO,
        params: spot_params(light.hotspot, light.falloff, light.attenuate, light.range),
    })
}

/// The capsule record for one light at `eye`. The reach is `max(range, range2) + |offset|`,
/// and the colour has no `Light+0xf8` factor [game 0054cd70; LS35.2 CAPSULE]. None when far.
/// [game 0054cd70] `D` is the distance from the eye to the Light's position (the writer's `+0xa0`, the
/// capsule's centre); its record point `Pc` is `+0xa0 - |offset| * axis` (its `local_a0`), not the distance's point.
pub fn capsule_record(light: &LevelLight, eye: Vec3) -> Option<ShaderLight> {
    let reach = reach_of(light);
    let fade = far_fade((light.position - eye).length(), reach)?;
    let (pc, a, w10) = capsule_segment(light.position, light.capsule_axis, light.offset_len);
    Some(ShaderLight {
        kind: LightKind::Capsule,
        position: pc,
        direction: a,
        w10,
        colour: light.colour * fade,
        colour2: light.colour2 * fade,
        params: capsule_params(light.attenuate, light.range, light.attenuate2, light.range2),
    })
}

/// The record for any light, or None when the light is dropped at this eye.
pub fn shader_light(light: &LevelLight, eye: Vec3) -> Option<ShaderLight> {
    match light.kind {
        LightKind::Omni => omni_record(light, eye),
        LightKind::Spot => spot_record(light, eye),
        LightKind::Capsule => capsule_record(light, eye),
    }
}

fn sat(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// `t*t*(3 - 2t)`, the shaders' falloff. [game 008250, 008e70, 009cb0: `mad (3, -2)`, `mul t t`]
fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// `sat((value - outer) / (inner - outer))`: 1 at `inner`, 0 at `outer`. [game 008250 lines 38-39
/// and 55, 008e70 lines 58-59 and 61 (the cone and radial ramps), 009cb0 lines 59 and 65]. The shader
/// takes the reciprocal of the span (`rcp`) and multiplies: with inner equal to outer that is +inf
/// [game], so the ramp is 1 above `outer` and 0 at or below it (at `outer` the product is NaN, which
/// the `mul_sat` modifier turns into 0 [game 008250 lines 39 and 55: `rcp r0.x` then `mul_sat`]).
/// The span == 0 branch below gives the same answers without the NaN.
fn ramp(value: f32, outer: f32, inner: f32) -> f32 {
    let span = inner - outer;
    if span == 0.0 {
        return if value > outer { 1.0 } else { 0.0 };
    }
    sat((value - outer) / span)
}

/// `pow(sat(N.H), SP) * (SP/30 + 1)`. [game 008250 lines 61-62, 008e70 lines 79-81, 009cb0 lines 93-95]
fn specular_lobe(normal: Vec3, half: Vec3, power: f32) -> f32 {
    sat(normal.dot(half)).powf(power) * (power / 30.0 + 1.0)
}

/// Omni, the full diffuse and specular at `point` [game 008250 (Base)]:
/// `L = c9 - P`, `d2 = |L|^2`, `t = ramp(d2, outer2, inner2)`, `atten = smooth(t)`;
/// diffuse `atten * sat(N.L^) * C`; specular `atten * C * (SP/30+1) * sat(N.H)^SP`, `H = normalize(V^ + L^)`.
/// `view_dir` is V^ (unit, from the point to the eye). `specular_power` is SP = 255 G.a + 1.
pub fn omni_diffuse_specular(light: &ShaderLight, point: Vec3, normal: Vec3, view_dir: Vec3, specular_power: f32) -> (Vec3, Vec3) {
    let to_light = light.position - point;
    let d2 = to_light.dot(to_light);
    let l = to_light.normalize_or_zero();
    let p = light.params;
    let atten = smooth(ramp(d2, p.y, p.x));
    let diffuse = atten * sat(normal.dot(l)) * light.colour;
    let h = (view_dir + l).normalize_or_zero();
    let specular = atten * light.colour * specular_lobe(normal, h, specular_power);
    (diffuse, specular)
}

/// Spot, the full diffuse and specular at `point` [game 008e70 (Base)]: the radial ramp in
/// squared distance (z inner, w outer), the cone `cosA = L^ . D` ramped between cos(inner) (x)
/// and cos(outer) (y), `atten = smooth(t_r) smooth(t_c)`. Diffuse uses the AXIS, not L^:
/// `atten * sat(N.D) * C`. Specular: `H = normalize(V^ + D)`, `atten * C * (SP/30+1) * sat(N.H)^SP`.
/// `light.direction` is the shader's `c10`, the axis as uploaded [assumed sign, see LevelLight].
pub fn spot_diffuse_specular(light: &ShaderLight, point: Vec3, normal: Vec3, view_dir: Vec3, specular_power: f32) -> (Vec3, Vec3) {
    let to_light = light.position - point;
    let d2 = to_light.dot(to_light);
    let l = to_light.normalize_or_zero();
    let p = light.params;
    let axis = light.direction;
    let radial = smooth(ramp(d2, p.w, p.z));
    let cone = smooth(ramp(l.dot(axis), p.y, p.x));
    let atten = radial * cone;
    let diffuse = atten * sat(normal.dot(axis)) * light.colour;
    let h = (view_dir + axis).normalize_or_zero();
    let specular = atten * light.colour * specular_lobe(normal, h, specular_power);
    (diffuse, specular)
}

/// Capsule, the full diffuse and specular at `point` [game 009cb0 (Base), line numbers in brackets]:
/// `u = sat((P - Pc) . A * c10.w)` [54-55]; radii `Ro = lerp(outer0, outer1, u)`,
/// `Ri = lerp(inner0, inner1, u)` [56], squared; `Q = Pc + u A`, `dist2 = |Q - P|^2` [60-61];
/// `t = ramp(dist2, Ro^2, Ri^2)`, `atten = smooth(t)` [62-68]; colour `C(u) = lerp(c11, c12, u)` [73-75].
/// Diffuse `atten * sat(N.Lq) * C(u)`, `Lq = normalize(Q - P)` [71-77].
/// Specular at a representative point [78-93]: `M = V - 2 (V.N) N`, `w = sat(((P - Pc).((A.M) M - A)) / ((A.M)^2 - |A|^2))`,
/// `Q2 = Pc + w A`, `H = normalize(normalize(Q2 - P) + V)`; `atten * C(u) * (SP/30+1) * sat(N.H)^SP`.
/// The zero-denominator case of `w` [game 009cb0 lines 86-88: `rcp` of (A.M)^2 - |A|^2, then `mul_sat`]: the
/// reciprocal of exactly zero is +inf, so w = 1 when the numerator is above 0 and 0 otherwise.
pub fn capsule_diffuse_specular(light: &ShaderLight, point: Vec3, normal: Vec3, view_dir: Vec3, specular_power: f32) -> (Vec3, Vec3) {
    let a = light.direction;
    let rel = point - light.position;
    let p = light.params;
    let u = sat(rel.dot(a) * light.w10);
    let ro = p.y + (p.w - p.y) * u;
    let ri = p.x + (p.z - p.x) * u;
    let q = light.position + a * u;
    let qp = q - point;
    let dist2 = qp.dot(qp);
    let atten = smooth(ramp(dist2, ro * ro, ri * ri));
    let colour = light.colour + (light.colour2 - light.colour) * u;
    let lq = qp.normalize_or_zero();
    let diffuse = atten * sat(normal.dot(lq)) * colour;

    let m = view_dir - 2.0 * view_dir.dot(normal) * normal;
    let am = a.dot(m);
    let denom = am * am - a.dot(a);
    let num = rel.dot(m * am - a);
    let w = if denom == 0.0 { if num > 0.0 { 1.0 } else { 0.0 } } else { sat(num / denom) };
    let q2 = light.position + a * w;
    let h = ((q2 - point).normalize_or_zero() + view_dir).normalize_or_zero();
    let specular = atten * colour * specular_lobe(normal, h, specular_power);
    (diffuse, specular)
}

/// Dispatch: the full diffuse and specular of one light at `point`, Base variants only.
/// The ambient-only and no-specular variants (LS-33 A.2, C.1) are not ported here.
pub fn light_diffuse_specular(light: &ShaderLight, point: Vec3, normal: Vec3, view_dir: Vec3, specular_power: f32) -> (Vec3, Vec3) {
    match light.kind {
        LightKind::Omni => omni_diffuse_specular(light, point, normal, view_dir, specular_power),
        LightKind::Spot => spot_diffuse_specular(light, point, normal, view_dir, specular_power),
        LightKind::Capsule => capsule_diffuse_specular(light, point, normal, view_dir, specular_power),
    }
}

/// Where a Light sits, from its parent's world matrix (rows of three, row vectors).
/// position = row 3 (floats 9..12), direction = row 2 (floats 6..9). [game 00563870] the loader reads
/// the direction from `+0x18` (float 6) and the position from `+0x24` (float 9) of a 12-float affine,
/// the same layout. [game 00563870 l.183-200, read 2026-10-20] that affine is the object's own matrix
/// (`param_1 + 0x14`, the Instance matrix base of 00575d30 / 00574c70) when the object has no node
/// (`+0xc == 0`), else the node's composed matrix that `parent_matrix_get` 00550930 returns; the
/// offsets are then taken from that affine's base, so float 6 and float 9 as here.
pub fn placement(world: &[f32; 12]) -> (Vec3, Vec3) {
    (Vec3::new(world[9], world[10], world[11]), Vec3::new(world[6], world[7], world[8]))
}

fn install_path(relative: &str) -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| r"C:\Games2".into()));
    let path = dir.join(relative);
    path.is_file().then_some(path)
}

/// The Light type from `lighttype` (`9884ece8`): the data's raw enum value. [data] the table
/// BACKGROUND 0, AMBIENT 1, FOG 2, DIRECTIONAL 3, OMNI 4, SPOT 5, CAPSULE 6 (level-lighting.md
/// section 3, chunk 1190). [assumed] the same in every chunk. Others (DIRECTIONAL) are not read here.
/// The decoded robot packs (`export\data\bumblebee.txt`, `starscream.txt`, 2026-10-20) agree on 4 = OMNI and
/// 5 = SPOT (`LIGHT_TYPE_OMNI` among them); none holds a CAPSULE (6) value, so 6 is unchecked here.
/// OPEN: settles: the `lighttype` values of the level light chunks (`tools\tf2_lxb.py dump` of each light chunk).
fn kind_of(light: Node) -> Option<LightKind> {
    let field = light.get("lighttype")?;
    match field.int() {
        Some(4) => Some(LightKind::Omni),
        Some(5) => Some(LightKind::Spot),
        Some(6) => Some(LightKind::Capsule),
        Some(_) => None,
        None => match field.enum_name()? {
            "LIGHT_TYPE_OMNI" => Some(LightKind::Omni),
            "LIGHT_TYPE_SPOT" => Some(LightKind::Spot),
            "LIGHT_TYPE_CAPSULE" => Some(LightKind::Capsule),
            _ => None,
        },
    }
}

/// One Light placed by `world` (an object's node bind): what `scene::read_lights` takes.
pub(crate) fn read_light_at(light: Node, world: &Mat) -> Option<LevelLight> {
    read_light(light, world)
}

fn read_light(light: Node, world: &Mat) -> Option<LevelLight> {
    let kind = kind_of(light)?;
    let num = |name: &str| light.get(name).and_then(|n| n.float().or_else(|| n.int().map(|v| v as f32))).unwrap_or(0.0);
    let rgb = |name: &str| light.get(name).map(Node::floats).filter(|c| c.len() >= 3).map_or(Vec3::ZERO, |c| Vec3::new(c[0], c[1], c[2]));
    let flag = |name: &str| light.get(name).is_some_and(Node::is_true);
    let (position, direction) = placement(&flat32(world));
    // [game 0054d269..0054d294] The record writer normalizes the same stored direction used
    // by spot lights before multiplying by |offset|. 00579740 clears a zero-length vector.
    let capsule_axis = if kind == LightKind::Capsule { direction.normalize_or_zero() } else { Vec3::ZERO };
    Some(LevelLight {
        kind,
        position,
        direction,
        colour: rgb("colour"),
        colour2: rgb("colour2"),
        range: num("range"),
        range2: num("range2"),
        attenuate: num("attenuate"),
        attenuate2: num("attenuate2"),
        hotspot: num("hotspot"),
        falloff: num("falloff"),
        offset_len: num("offset").abs(),
        affect_diffuse: flag("affectDiffuse"),
        affect_specular: flag("affectSpecular"),
        ambient_only: flag("ambientOnly"),
        capsule_axis,
    })
}

/// The walk: as `level_render::visit` (an Instance's matrix places what is under it; an
/// attachment rides the bind of its index) and it keeps Light objects.
///
/// Two keyings (LS-38), chosen by `per_placement`:
/// - object (the default, the survey's count): the walk goes down through every field, with
///   no container gate, and a typed node is visited once per placed state WITHOUT its matrix
///   in the key. That is finite by construction. A Light is read once, on its first route.
///   Zone: 62 omni, 14 spot, 28 capsule (exact). Mission: 17 omni, 1 spot.
/// - placement: the walk goes down through the containers only (`level_render::carries_placement`)
///   and a typed node is keyed with its matrix; one Light per stored object per placement. Without
///   the gate the matrix-keyed walk does not end on the mission pack (LS-38: 3.9 million visited
///   states in the first 5 million visits, the matrices of a DAG multiplying with depth).
/// LS-42: a Light's key in a walk: (pack ordinal in the list, data chunk index, the Light's offset in the chunk).
type LightKey = (usize, usize, usize);

struct Walk<'a> {
    out: &'a mut Vec<(LightKey, LevelLight)>,
    /// The pack ordinal and the chunk index of the chunk walked (the first two parts of `LightKey`).
    pack: usize,
    chunk: usize,
    /// (Light offset, quantised world matrix or None): with `per_placement`, one Light per stored
    /// object per placement (two placements of one prefab are two lights in the world, as
    /// `level_render::Sink::seen` does for things); without it, one per stored object.
    seen: HashSet<(usize, Option<[i64; 12]>)>,
    per_placement: bool,
    /// (node offset, its type, quantised world matrix or zero, placed): one visit per state.
    /// Typed nodes only, as `level_render::visit`.
    visited: HashSet<(usize, usize, [i64; 12], bool)>,
    /// Calls to `visit` in this chunk, and the first place the budget ran out (see `VISIT_LIMIT`).
    visits: u64,
    aborted: Option<String>,
}

impl Walk<'_> {
    fn visit(&mut self, node: Node, acc: Mat, placed: bool, depth: u32) {
        if depth > WALK_DEPTH || self.aborted.is_some() {
            return;
        }
        self.visits += 1;
        if self.visits > VISIT_LIMIT {
            self.aborted = Some(format!(
                "walk over the visit budget ({VISIT_LIMIT}) at depth {depth}, type {}, visited states {}",
                node.type_name().unwrap_or("untyped"),
                self.visited.len()
            ));
            return;
        }
        // A Light that is itself a child (a field or a list item, not an attachment). It is a
        // leaf: nothing under it is walked.
        if node.type_name() == Some("Light") {
            if placed {
                self.light(node, &acc);
            }
            return;
        }
        // The gate of `level_render::visit`: a non-container node (a component, a particle
        // effect, a trigger) is not entered, so its pointers place nothing.
        if self.per_placement && !crate::level_render::carries_placement(node) {
            return;
        }
        // (This walk is for the level's lights, zone and mission packs; a robot's lights come from `read_light_at`.)
        // [assumed] (level-only: a robot's lights come from `read_light_at`, not this walk) the nearest ancestor with a matrix places the Light (note section 3), not only an Instance.
        // OPEN: settles: the game's placement walk for a Light under a matrix-bearing non-Instance node (`level_render::visit`).
        // As `level_render::visit`: an Instance (a matrix on a node with `things`) places what is under it.
        let instance = node.get("matrix").and_then(matrix_of).filter(|_| node.get("things").is_some());
        let placed = placed || instance.is_some();
        let acc = instance.map_or(acc, |m| compose(&m, &acc));
        // As `level_render::visit`: a typed node reached again at the same place and in the same
        // state has nothing new under it.
        if let (Some(name), Some(at)) = (node.type_name(), node.offset()) {
            // Object mode: the matrix is left out of the key, so each stored node is visited once
            // per placed state, and the walk is finite without the gate (the first route wins).
            let key = if self.per_placement { flat(&acc).map(|v| (v * KEY_SCALE).round() as i64) } else { [0; 12] };
            if !self.visited.insert((at, name.as_ptr() as usize, key, placed)) {
                return;
            }
        }
        if let Some(attachments) = node.get("attachments") {
            for attachment in attachments.items() {
                let Some(thing) = attachment.get("thing") else { continue };
                let index = attachment.get("index").and_then(Node::int).unwrap_or(0);
                let riding = compose(&bind_of(node, index), &acc);
                if thing.type_name() == Some("Light") {
                    if placed {
                        self.light(thing, &riding);
                    }
                    continue;
                }
                self.visit(thing, riding, placed, depth + 1);
            }
        }
        for (hash, child) in node.fields() {
            if matches!(node.file().name(hash), Some("attachments" | "sub_matrices" | "matrix" | "hierarchy")) {
                continue;
            }
            self.visit(child, acc, placed, depth + 1);
        }
        if node.len() > 0 {
            for child in node.items() {
                self.visit(child, acc, placed, depth + 1);
            }
        }
    }

    fn light(&mut self, thing: Node, world: &Mat) {
        let Some(at) = thing.offset() else { return };
        let key = self.per_placement.then(|| flat(world).map(|v| (v * KEY_SCALE).round() as i64));
        if !self.seen.insert((at, key)) {
            return;
        }
        if let Some(light) = read_light(thing, world) {
            self.out.push(((self.pack, self.chunk, at), light));
        }
    }
}

/// Every OMNI, SPOT and CAPSULE Light object of the given packs (the zone pack first, then a
/// mission pack if there is one), each with its parent's world matrix. DIRECTIONAL and other
/// types are skipped. Each Light object is read once per data chunk.
/// [assumed] the placement and the walk (see `placement` and `Walk`); the zone count is checked
/// against the survey's (Shanghai zone: 62 omni, 14 spot, 28 capsule) in the tests.
pub fn load_level_lights(packs: &[PathBuf]) -> Result<Vec<LevelLight>, String> {
    load_level_lights_keyed(packs, PER_PLACEMENT)
}

/// [assumed] Object keying (false) is the default: one light per stored Light object, on its
/// first route. It matches the survey's zone count. A Light placed by several Instances is ONE
/// light here. [assumed] that is wrong for the world, where each placed copy would light its own
/// spot: the per-placement keying (true) gives 991 omni on the zone, far off the survey, so it was
/// not adopted. Settle: the game's own placement of a shared Light (`00563870` or its callers, not
/// read), or a render of a prefab used at several places.
const PER_PLACEMENT: bool = false;

/// `load_level_lights` with the keying of `Walk::seen` given (the tests print both).
pub fn load_level_lights_keyed(packs: &[PathBuf], per_placement: bool) -> Result<Vec<LevelLight>, String> {
    Ok(walk_lights(packs, per_placement)?.into_iter().map(|(_, light)| light).collect())
}

/// The walk over every data chunk of the packs, with each Light's key (`LightKey`).
fn walk_lights(packs: &[PathBuf], per_placement: bool) -> Result<Vec<(LightKey, LevelLight)>, String> {
    let mut out = Vec::new();
    for (ordinal, path) in packs.iter().enumerate() {
        let pack = Pack::open(path)?;
        for chunk in pack.of_type(pack::DATA) {
            let file = DataFile::parse(pack.data(chunk).to_vec()).map_err(|e| format!("{}#{}: {e}", path.display(), chunk.index))?;
            let mut walk = Walk { out: &mut out, pack: ordinal, chunk: chunk.index, seen: HashSet::new(), per_placement, visited: HashSet::new(), visits: 0, aborted: None };
            walk.visit(file.root(), IDENTITY, false, 0);
            if let Some(why) = walk.aborted {
                return Err(format!("{}#{}: {why}", path.display(), chunk.index));
            }
        }
    }
    Ok(out)
}

/// LS-42: one LevelLight per distinct (Light object, world matrix) pair of the packs, the placement
/// rule of `level_render` (the gated walk: only containers are entered, and a Light is keyed with its
/// world matrix, so each placed copy of a prefab's lamp is one light at that lamp's place).
/// [assumed] Only the gated placements are static world lights. The ungated-only routes hang under
/// traffic vehicle and character templates (LS-42), so adding them here lights the origin with
/// unspawned headlights. Follows the manager decision in notes/status.md; the dynamic spawners
/// would settle their runtime placement. OPEN: settles: the dynamic spawners' runtime placement (a
/// recording of the level's lights while a spawner runs), and the manager decision in notes/status.md.
pub fn load_level_lights_placed(packs: &[PathBuf]) -> Result<Vec<LevelLight>, String> {
    Ok(placed_lights(packs)?.into_iter().map(|(_, light)| light).collect())
}

/// `load_level_lights_placed` with each light's key (`LightKey`), for the tests.
fn placed_lights(packs: &[PathBuf]) -> Result<Vec<(LightKey, LevelLight)>, String> {
    walk_lights(packs, true)
}

/// LS-42: the reach that sets a light's far cut: `range`, or for a capsule `max(range, range2) + |offset|`,
/// as the record builders use it.
fn reach_of(light: &LevelLight) -> f32 {
    match light.kind {
        LightKind::Capsule => light.range.max(light.range2) + light.offset_len,
        LightKind::Omni | LightKind::Spot => light.range,
    }
}

/// LS-42: the indices of the lights that can matter at `eye`: the ones `far_fade` keeps (distance
/// below 50 reaches, the cut of `shader_light`), nearest first, at most `max`. Pure. A light with no
/// reach, or a non-finite distance, is dropped. [game LS-35 LS35.2: the gate `D / range < 50`]
pub fn nearest_lights(lights: &[LevelLight], eye: Vec3, max: usize) -> Vec<usize> {
    let mut near: Vec<(f32, usize)> = lights
        .iter()
        .enumerate()
        .filter_map(|(i, light)| {
            let d = (light.position - eye).length();
            (d.is_finite() && far_fade(d, reach_of(light)).is_some()).then_some((d, i))
        })
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    near.truncate(max);
    near.into_iter().map(|(_, i)| i).collect()
}

/// A weapon's muzzle light: the Light of its `muzzleFlashObject` (weapon `+0x33c`), which the
/// weapon's muzzle effect routine spawns at the flash node's matrix with every shot that makes a
/// flash. [game 007aac80, reached from 007ae010 through vtable `+0x7c`; notes/particles.md "Muzzle
/// attachment and exhaust timing"] Every playable pack's flash object is one OMNI Light and one
/// TimeoutDeleter (`muzzleflash_orange`: colour 0.8 / 0.75 / 0.61, range 10, attenuate 6, time 0.05,
/// fadeStart -1). [data]
#[derive(Clone, Debug, PartialEq)]
pub struct FlashLight {
    /// The Light, placed in the flash object's own frame (its scene Instance and the attachment's bind).
    pub light: LevelLight,
    /// The TimeoutDeleter's `time`: seconds until the object is removed.
    pub time: f32,
    /// The TimeoutDeleter's `fadeStart` (-1 when absent, its loader's default). [game 00776e40]
    pub fade_start: f32,
}

/// A flash light in the world, counting down to its removal.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveLight {
    pub light: LevelLight,
    /// The deleter's `+0x28`: what is left of `time`.
    pub remaining: f32,
    /// The Light's own colour, before the fade.
    pub base: Vec3,
    /// The deleter's `+0x2c`: `time - fadeStart`, or -1 with no fade (`FlashLight::fade`).
    pub fade: f32,
}

impl FlashLight {
    /// The span of the deleter's fade (`+0x2c`): `time - fadeStart` when `fadeStart` >= 0, else -1.
    /// [game 00776e40]
    pub fn fade(&self) -> f32 {
        if self.fade_start >= 0.0 { self.time - self.fade_start } else { -1.0 }
    }

    /// The light spawned at `at` (the flash node's matrix, game space). The spawned object is not
    /// tied to the node: it stays where it was made. [game 007aac80: the object is built at the node's
    /// matrix; no link to the node was seen there]
    pub fn spawn(&self, at: glam::Affine3A) -> LiveLight {
        let mut light = self.light.clone();
        light.position = at.transform_point3(light.position);
        light.direction = at.transform_vector3(light.direction);
        light.capsule_axis = at.transform_vector3(light.capsule_axis).normalize_or_zero();
        LiveLight { base: light.colour, light, remaining: self.time, fade: self.fade() }
    }
}

impl LiveLight {
    /// One update of its TimeoutDeleter: the count goes down by `dt`, and at zero or below the
    /// object is marked for removal. Returns whether it is still there. [game 00776f50]
    /// With a fade (`fadeStart` >= 0, `+0x2c` = `time - fadeStart`) the deleter's alpha (`+0x30`) is
    /// `remaining / +0x2c` once what is left is under `+0x2c`, clamped to 0..1, and broadcast
    /// (008fc210) [game 00776f50]; with `fadeStart` < 0 the loader leaves `+0x2c` at -1 and the branch
    /// never runs (the muzzle lights). [game 005635e0, 0056b610, 0054c7f0] The alpha reaches the Light
    /// through its listener on key 611d70bc: the Light's constructor installs 0056b610, which stores the
    /// broadcast value in Light+0xf8, and the spot record writer multiplies the colour by Light+0xf8
    /// (second pass 2026-10-20). So "the alpha goes into the colour" is the game's rule. OPEN: the removal:
    /// 00776f50 is the deleter (its owner's mark_for_removal, 005a91e0, owner bit 31); 005572c0 is a
    /// plain handle release, NOT the despawn; the consumer of mark_for_removal's list is not read.
    /// settles: that consumer (the removal happens there).
    /// [game 0054cd70] A capsule's colour has no Light+0xf8 factor (the capsule record writer reads
    /// Light+0x80 / +0x90 only), so a capsule keeps its colour through the fade.
    pub fn step(&mut self, dt: f32) -> bool {
        self.remaining -= dt;
        if self.fade > 0.0 && self.remaining < self.fade && self.light.kind != LightKind::Capsule {
            self.light.colour = self.base * (self.remaining / self.fade).clamp(0.0, 1.0);
        }
        self.remaining > 0.0
    }
}

/// The FlashLight of one object whose own attachments hold a Light and its TimeoutDeleter (a
/// projectile's blast object, `detach1`; the ground wave's object): the first Light, placed by its
/// node's bind, and the deleter of THE SAME object. `read_flash_light`'s deep search could take
/// another object's deleter. None without a Light; no deleter is a light that stays. [data]
pub fn read_light_and_deleter(object: Node) -> Option<FlashLight> {
    let things = || object.get("attachments").into_iter().flat_map(Node::items);
    let light = things().find_map(|a| {
        let thing = a.get("thing").filter(|t| t.type_name() == Some("Light"))?;
        read_light(thing, &bind_of(object, a.get("index").and_then(Node::int).unwrap_or(0)))
    })?;
    let deleter = things().filter_map(|a| a.get("thing")).find(|t| t.type_name() == Some("TimeoutDeleter"));
    let num = |d: Node, name: &str, default: f32| d.get(name).and_then(|n| n.float().or_else(|| n.int().map(|v| v as f32))).unwrap_or(default);
    let (time, fade_start) = deleter.map_or((f32::INFINITY, -1.0), |d| (num(d, "time", 1.0), num(d, "fadeStart", -1.0)));
    Some(FlashLight { light, time, fade_start })
}

/// The FlashLight of one `muzzleFlashObject` node: its first OMNI / SPOT / CAPSULE Light, placed by
/// the walk of the level lights (an Instance places what is under it, an attachment rides its bind),
/// and its TimeoutDeleter. None when it has no Light. A missing deleter is a light that stays
/// (`time` infinite). [data]
pub fn read_flash_light(object: Node) -> Option<FlashLight> {
    let mut found = Vec::new();
    let mut walk = Walk { out: &mut found, pack: 0, chunk: 0, seen: HashSet::new(), per_placement: false, visited: HashSet::new(), visits: 0, aborted: None };
    walk.visit(object, IDENTITY, false, 0);
    let light = found.into_iter().next()?.1;
    fn deleter(node: Node, depth: u32) -> Option<Node> {
        if node.type_name() == Some("TimeoutDeleter") {
            return Some(node);
        }
        if depth > 8 {
            return None;
        }
        let fields = node.fields().into_iter().map(|(_, c)| c);
        let items: Vec<Node> = if node.len() > 0 { node.items().collect() } else { Vec::new() };
        fields.chain(items).find_map(|child| deleter(child, depth + 1))
    }
    let num = |d: Node, name: &str, default: f32| d.get(name).and_then(|n| n.float().or_else(|| n.int().map(|v| v as f32))).unwrap_or(default);
    let (time, fade_start) = deleter(object, 0).map_or((f32::INFINITY, -1.0), |d| (num(d, "time", 1.0), num(d, "fadeStart", -1.0)));
    Some(FlashLight { light, time, fade_start })
}

/// Each weapon's FlashLight, by its `weaponName` (and its alternate projectile's row key,
/// `character::alt_projectile_key`, which fires through the same weapon), for the weapons of a
/// `Character` node that have a `muzzleFlashObject`. The weapon classes are those
/// `character::read_weapon_effects` reads. [data]
pub fn read_weapon_flash_lights(character: Node) -> Vec<(u32, FlashLight)> {
    let mut out = Vec::new();
    for file in character.get("weapons").into_iter().flat_map(|weapons| weapons.items()) {
        let attached = file.path(&["obj", "attachments"]).into_iter().flat_map(|a| a.items());
        for thing in attached.filter_map(|a| a.get("thing")) {
            if !matches!(thing.type_name(), Some("GunWeapon" | "MissileLauncher" | "GrenadeLauncher")) {
                continue;
            }
            let Some(flash) = thing.get("muzzleFlashObject").and_then(read_flash_light) else { continue };
            let name = thing.get("weaponName").and_then(Node::int).map_or(0, |h| h as u32);
            out.push((crate::character::alt_projectile_key(name), flash.clone()));
            out.push((name, flash));
        }
    }
    out
}

/// `read_weapon_flash_lights` of a character pack (`characters\<pack>.str`, its `Character` thing).
pub fn load_flash_lights(game_dir: &std::path::Path, pack_name: &str) -> Result<Vec<(u32, FlashLight)>, String> {
    let path = game_dir.join("characters").join(format!("{pack_name}.str"));
    let pack = Pack::open(&path)?;
    let chunk = pack.of_type(pack::DATA).next().ok_or("pack has no data object")?;
    let file = DataFile::parse(pack.data(chunk).to_vec())?;
    for (_, root) in file.root().fields() {
        for thing in root.get("things").into_iter().flat_map(|things| things.items()) {
            if thing.type_name() == Some("Character") {
                return Ok(read_weapon_flash_lights(thing));
            }
        }
    }
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec4, b: Vec4, tol: f32) -> bool {
        (a - b).abs().max_element() < tol
    }

    /// The shaders take rcp(inner - outer) and multiply: a zero span is +inf, so the ramp is a
    /// step at `outer` [game 008250 lines 38-39, 55; 009cb0 lines 59, 65].
    #[test]
    fn a_zero_span_ramp_is_a_step_above_its_outer_edge() {
        assert_eq!(ramp(2.0, 1.0, 1.0), 1.0);
        assert_eq!(ramp(1.0, 1.0, 1.0), 0.0);
        assert_eq!(ramp(0.5, 1.0, 1.0), 0.0);
        assert_eq!(ramp(0.5, 0.0, 1.0), 0.5);
    }

    /// Part D of notes/level-deferred-lighting.md: the three Shanghai lights as data rows.
    fn freeway() -> LevelLight {
        LevelLight {
            kind: LightKind::Omni,
            position: Vec3::new(24.3893, -16.5026, 9.60939),
            direction: Vec3::ZERO,
            colour: Vec3::new(1.2, 0.795294, 0.357647),
            colour2: Vec3::ZERO,
            range: 10.0,
            range2: 10.0,
            attenuate: 5.24,
            attenuate2: 5.24,
            hotspot: -0.0174533,
            falloff: -0.0174533,
            offset_len: 0.0,
            affect_diffuse: true,
            affect_specular: true,
            ambient_only: false,
            capsule_axis: Vec3::ZERO,
        }
    }

    fn crane() -> LevelLight {
        LevelLight {
            kind: LightKind::Spot,
            position: Vec3::new(304.141, -159.066, -20.0),
            direction: Vec3::new(0.0, 0.0, -1.0),
            colour: Vec3::splat(0.6),
            colour2: Vec3::ZERO,
            range: 80.0,
            range2: 80.0,
            attenuate: 0.0,
            attenuate2: 0.0,
            hotspot: 0.436332,
            falloff: 0.698132,
            offset_len: 0.0,
            affect_diffuse: true,
            affect_specular: true,
            ambient_only: false,
            capsule_axis: Vec3::ZERO,
        }
    }

    fn quickstop() -> LevelLight {
        LevelLight {
            kind: LightKind::Capsule,
            position: Vec3::new(308.68, 93.8637, 4.8892),
            direction: Vec3::ZERO,
            colour: Vec3::new(0.3, 0.222353, 0.0658824),
            colour2: Vec3::ZERO,
            range: 3.0,
            range2: 3.0,
            attenuate: 0.0,
            attenuate2: 0.0,
            hotspot: 0.0,
            falloff: 0.0,
            offset_len: 1.46444,
            affect_diffuse: false,
            affect_specular: false,
            ambient_only: true,
            capsule_axis: Vec3::Z,
        }
    }

    /// (a) The constant records for Part D's lights, with the note's numbers.
    #[test]
    fn point_light_constant_records_match_part_d() {
        // OMNI: c11 = (5.24^2, 10^2, 1/(100 - 27.4576), 0) = (27.4576, 100, 0.0137827, 0).
        let p = omni_params(5.24, 10.0);
        assert!(close(p, Vec4::new(27.4576, 100.0, 1.0 / 72.5424, 0.0), 1e-4), "{p:?}");
        // The fade at D = 5 with range 10: 1 - (0.02 * 0.5)^2 = 0.9999.
        let eye_at_5 = freeway().position + Vec3::X * 5.0;
        let omni = omni_record(&freeway(), eye_at_5).unwrap();
        assert!((omni.colour.x - 1.2 * 0.9999).abs() < 1e-5);
        assert_eq!(omni.position, freeway().position);
        // SPOT: c12 = (cos(0.436332 * 0.5), cos(0.698132 * 0.5), 0, 80^2). The note's D.2 used the
        // full angles (0.906308, 0.766044); the writer halves them (LS35.2), so these differ.
        let s = spot_params(0.436332, 0.698132, 0.0, 80.0);
        assert!(close(s, Vec4::new(0.976296, 0.939693, 0.0, 6400.0), 1e-5), "{s:?}");
        // CAPSULE: c13 = (0, 3, 0, 3), linear.
        assert_eq!(capsule_params(0.0, 3.0, 0.0, 3.0), Vec4::new(0.0, 3.0, 0.0, 3.0));
        // The capsule segment: A = 2 * 1.46444 * z, c10.w = 1 / A^2.
        let (pc, a, w10) = capsule_segment(quickstop().position, Vec3::Z, 1.46444);
        assert!((a - Vec3::new(0.0, 0.0, 2.92888)).length() < 1e-4);
        assert!((w10 - 1.0 / (2.92888f32 * 2.92888)).abs() < 1e-5);
        assert!((pc - (quickstop().position - Vec3::Z * 1.46444)).length() < 1e-4);
    }

    /// The far fade and its cut: 1 at D = 0, 0 at D = 50 range, and no record beyond.
    #[test]
    fn point_light_far_fade_and_cut() {
        assert_eq!(far_fade(0.0, 10.0), Some(1.0));
        // D = 49 range: 1 - (0.02 * 49)^2 = 1 - 0.9604 = 0.0396, close to zero at the cut.
        assert!((far_fade(490.0, 10.0).unwrap() - 0.0396).abs() < 1e-5);
        assert_eq!(far_fade(500.0, 10.0), None);
        assert_eq!(far_fade(600.0, 10.0), None);
        assert_eq!(far_fade(1.0, 0.0), None);
    }

    /// (b) OMNI at half range (d = 5), facing the light: full diffuse = C (the note's 0.5 C,
    /// doubled by the composite). The inner distance is 5.24, so the attenuation is 1.
    #[test]
    fn point_light_omni_half_range_matches_part_d() {
        let light = omni_record(&freeway(), freeway().position).unwrap();
        let point = freeway().position + Vec3::X * 5.0;
        let normal = -Vec3::X; // faces the light
        let (d, s) = omni_diffuse_specular(&light, point, normal, -Vec3::X, 1.0);
        assert!((d - Vec3::new(1.2, 0.795294, 0.357647)).length() < 1e-4, "diffuse {d:?}");
        assert!((d / 2.0 - Vec3::new(0.600, 0.398, 0.179)).length() < 1e-3);
        // Specular with SP = 1: (SP/30+1) * sat(N.H)^SP, H = normalize(V + L) = -X, so N.H = 1.
        assert!((s - Vec3::new(1.2, 0.795294, 0.357647) * (1.0 / 30.0 + 1.0)).length() < 1e-4, "spec {s:?}");
    }

    /// (b) SPOT at half range on axis: full diffuse = 0.84375 * C = 0.50625, so the pass
    /// value (half of it) is 0.253. The axis points from the surface to the light.
    #[test]
    fn point_light_spot_half_range_on_axis_matches_part_d() {
        let light = spot_record(&crane(), crane().position).unwrap();
        let aim = Vec3::new(-1.0, 0.0, 0.0); // the spot points along aim; the axis D = -aim
        let d_axis = -aim;
        let point = crane().position + aim * 40.0;
        let mut light = light;
        light.direction = d_axis;
        let (d, _) = spot_diffuse_specular(&light, point, d_axis, -d_axis, 1.0);
        assert!((d - Vec3::splat(0.84375 * 0.6)).length() < 1e-4, "{d:?}");
        assert!((d / 2.0 - Vec3::splat(0.253)).length() < 1e-3);
    }

    /// (c) Zero beyond range, full inside the inner distance, zero outside the outer cone.
    #[test]
    fn point_light_zero_beyond_range_full_inside_and_cone() {
        let light = omni_record(&freeway(), freeway().position).unwrap();
        let normal = -Vec3::X;
        let (d, s) = omni_diffuse_specular(&light, freeway().position + Vec3::X * 11.0, normal, -Vec3::X, 1.0);
        assert_eq!((d, s), (Vec3::ZERO, Vec3::ZERO));
        // Inside the inner distance (2 < 5.24): attenuation 1.
        let (d_in, _) = omni_diffuse_specular(&light, freeway().position + Vec3::X * 2.0, normal, -Vec3::X, 1.0);
        assert!((d_in - light.colour).length() < 1e-5);

        // SPOT: the axis D = +X (the surface-to-light axis); a point 60 degrees off the aim
        // (cos 0.5) is beyond the outer cone (cos 0.9397 for the 40 degree falloff, half angle 20).
        let mut spot = spot_record(&crane(), crane().position).unwrap();
        spot.direction = Vec3::X;
        let off = Vec3::new(-0.5, 0.866025, 0.0) * 10.0;
        let to_light = (-off).normalize();
        let (d_cone, s_cone) = spot_diffuse_specular(&spot, crane().position + off, to_light, -to_light, 1.0);
        assert_eq!((d_cone, s_cone), (Vec3::ZERO, Vec3::ZERO));
        // The same point on the 20 degree side is inside the cone (cos 0.5 for 60 is not, cos 0.95 is).
        let near = Vec3::new(-0.95, 0.312250, 0.0) * 10.0;
        let (d_in, _) = spot_diffuse_specular(&spot, crane().position + near, (-near).normalize(), -(-near).normalize(), 1.0);
        assert!(d_in.x > 0.0);
    }

    /// (b) CAPSULE at half distance: segment on z from -1 to 1, the point 1.5 out from its middle
    /// (u = 0.5), radii 0 and 3 (linear): t = (2.25 - 9) / (0 - 9) = 0.75, atten = 0.84375.
    #[test]
    fn point_light_capsule_half_range_matches_the_shape() {
        let base = LevelLight {
            kind: LightKind::Capsule,
            position: Vec3::ZERO,
            direction: Vec3::ZERO,
            colour: Vec3::new(0.3, 0.222353, 0.0658824),
            colour2: Vec3::new(0.3, 0.222353, 0.0658824),
            range: 3.0,
            range2: 3.0,
            attenuate: 0.0,
            attenuate2: 0.0,
            hotspot: 0.0,
            falloff: 0.0,
            offset_len: 1.0,
            affect_diffuse: true,
            affect_specular: true,
            ambient_only: false,
            capsule_axis: Vec3::Z,
        };
        let light = capsule_record(&base, Vec3::ZERO).unwrap();
        let point = Vec3::new(-1.5, 0.0, 0.0);
        let normal = Vec3::X; // faces the segment's middle
        let (d, _) = capsule_diffuse_specular(&light, point, normal, Vec3::X, 1.0);
        assert!((d - base.colour * 0.84375).length() < 1e-4, "{d:?}");
        // Beyond the range: zero.
        let far = Vec3::new(-4.0, 0.0, 0.0);
        let (d_far, s_far) = capsule_diffuse_specular(&light, far, normal, Vec3::X, 1.0);
        assert_eq!((d_far, s_far), (Vec3::ZERO, Vec3::ZERO));
    }

    /// The constant writers and shading of a dropped light: a light beyond 50 range gives no record.
    #[test]
    fn point_light_dropped_light_has_no_record() {
        assert!(shader_light(&freeway(), freeway().position + Vec3::X * 600.0).is_none());
        assert!(shader_light(&freeway(), freeway().position).is_some());
    }

    /// LS-38: the Shanghai CSA mission pack (`levels\shanghai_01\missions\...`) loads within a
    /// time bound and returns its lights. The survey says 9 omni and 1 spot (10 Light objects).
    /// Runs in a thread so a walk that does not finish fails the test instead of hanging it.
    #[test]
    fn point_light_mission_pack_loads_in_time() {
        let Some(mission) = install_path(r"levels\shanghai_01\missions\shanghai_01_construction_a_csa\mission.str") else {
            println!("mission pack not installed; skipped");
            return;
        };
        for per_placement in [false, true] {
            let path = mission.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let started = std::time::Instant::now();
            std::thread::spawn(move || {
                let _ = tx.send(load_level_lights_keyed(&[path], per_placement));
            });
            let lights = rx
                .recv_timeout(std::time::Duration::from_secs(60))
                .unwrap_or_else(|e| panic!("mission walk did not finish in 60 s: {e}"))
                .unwrap();
            let elapsed = started.elapsed();
            let count = |k| lights.iter().filter(|l| l.kind == k).count();
            println!(
                "shanghai csa mission per_placement={per_placement}: {} lights ({} omni, {} spot, {} capsule) in {elapsed:?}",
                lights.len(),
                count(LightKind::Omni),
                count(LightKind::Spot),
                count(LightKind::Capsule)
            );
            for l in &lights {
                println!("mission light {:?} position {:?} range {} colour {:?}", l.kind, l.position.to_array(), l.range, l.colour.to_array());
            }
            assert!(elapsed.as_secs() < 30, "{elapsed:?}");
            assert!(!lights.is_empty());
            assert_eq!(count(LightKind::Capsule), 0);
            // [unknown] object keying finds 17 omni and 1 spot here; the survey says 9 and 1. Cause
            // not settled (LS-38 note). Not forced.
        }
    }

    /// (d) Shanghai_01's zone: the survey's counts (62 omni, 14 spot, 28 capsule), and five
    /// lights printed for notes/level-deferred-lighting.md (LS-36). Needs the game install.
    #[test]
    fn point_light_shanghai_zone_counts_and_five_lights() {
        let Some(zone) = install_path(r"levels\shanghai_01\zone.str") else { return };
        for per_placement in [false, true] {
            let lights = load_level_lights_keyed(std::slice::from_ref(&zone), per_placement).unwrap();
            let count = |k| lights.iter().filter(|l| l.kind == k).count();
            println!(
                "shanghai_01 zone per_placement={per_placement}: {} lights: {} omni, {} spot, {} capsule (survey: 62 omni, 14 spot, 28 capsule)",
                lights.len(),
                count(LightKind::Omni),
                count(LightKind::Spot),
                count(LightKind::Capsule)
            );
        }
        let lights = load_level_lights(&[zone]).unwrap();
        let count = |k| lights.iter().filter(|l| l.kind == k).count();
        let (omni, spot, capsule) = (count(LightKind::Omni), count(LightKind::Spot), count(LightKind::Capsule));
        for l in lights.iter().take(5) {
            println!("light {:?} position {:?} range {} colour {:?}", l.kind, l.position.to_array(), l.range, l.colour.to_array());
        }
        // LS-38: the object keying matches the survey exactly (62 / 14 / 28). The gated walk, which
        // the two existing walks use, gives 57 / 6 / 28 instead (see the note).
        assert_eq!((omni, spot, capsule), (62, 14, 28));
    }

    /// LS-38: the gated walk (`per_placement`) is bounded too: it finishes the zone in seconds.
    #[test]
    fn point_light_zone_walk_is_bounded_per_placement() {
        let Some(zone) = install_path(r"levels\shanghai_01\zone.str") else { return };
        let started = std::time::Instant::now();
        let lights = load_level_lights_keyed(&[zone], true).unwrap();
        assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
        assert!(lights.len() >= 104, "{}", lights.len());
    }

    /// LS-42: the Shanghai CSA mission pack (as `levels\shanghai_01\missions\...`, LS-38).
    const CSA_REL: &str = r"levels\shanghai_01\missions\shanghai_01_construction_a_csa\mission.str";

    /// LS-42: the report goes to `work\ls42\<name>` and to stdout (the ticket's test run has no `--nocapture`).
    fn ls42_report(name: &str, text: &str) {
        println!("{text}");
        let dir = PathBuf::from(r"C:\Bumblebee\work\ls42");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(name), text);
    }

    fn kind_counts(lights: &[LevelLight]) -> (usize, usize, usize) {
        let count = |k| lights.iter().filter(|l| l.kind == k).count();
        (count(LightKind::Omni), count(LightKind::Spot), count(LightKind::Capsule))
    }

    /// The placements that have another placement of the same Light object within 0.5 m.
    fn near_duplicates(placed: &[(LightKey, LevelLight)]) -> usize {
        let mut groups: std::collections::HashMap<LightKey, Vec<Vec3>> = Default::default();
        for (key, light) in placed {
            groups.entry(*key).or_default().push(light.position);
        }
        groups
            .values()
            .map(|ps| (0..ps.len()).filter(|&i| (0..ps.len()).any(|j| j != i && (ps[i] - ps[j]).length() < 0.5)).count())
            .sum()
    }

    /// LS-42 (3): the kind counts of the object walk and of the placed loader, for the packs the notes name.
    #[test]
    fn point_light_ls42_pack_counts() {
        let mut report = String::from("LS-42 counts per kind (omni, spot, capsule)\n");
        for (name, rel) in [
            ("shanghai_01 zone", r"levels\shanghai_01\zone.str"),
            ("shanghai_01 CSA mission", CSA_REL),
            ("us_test zone", r"levels\us_test\zone.str"),
        ] {
            let Some(path) = install_path(rel) else { continue };
            let packs = [path];
            let object = walk_lights(&packs, false).unwrap();
            let placed = placed_lights(&packs).unwrap();
            let object_lights: Vec<LevelLight> = object.iter().map(|(_, l)| l.clone()).collect();
            let placed_lights_only: Vec<LevelLight> = placed.iter().map(|(_, l)| l.clone()).collect();
            let distinct: HashSet<LightKey> = placed.iter().map(|(k, _)| *k).collect();
            let gated: Vec<LevelLight> = walk_lights(&packs, true).unwrap().into_iter().map(|(_, l)| l).collect();
            report += &format!(
                "{name}: one per object {:?}; gated placements only {:?}; placed {:?} = {} lights of {} Light objects; {} placements within 0.5 m of another of their object\n",
                kind_counts(&object_lights),
                kind_counts(&gated),
                kind_counts(&placed_lights_only),
                placed.len(),
                distinct.len(),
                near_duplicates(&placed)
            );
        }
        ls42_report("pack_counts.txt", &report);
    }

    /// LS-42: the nearest-lights selection is pure: nearest first, the far cut, the cap, and no reach.
    #[test]
    fn point_light_nearest_lights_pure() {
        let omni = |x: f32, range: f32| LevelLight { position: Vec3::new(x, 0.0, 0.0), range, ..freeway() };
        let capsule = |x: f32| LevelLight {
            kind: LightKind::Capsule,
            position: Vec3::new(x, 0.0, 0.0),
            range: 3.0,
            range2: 3.0,
            offset_len: 1.46444,
            ..freeway()
        };
        let lights = vec![
            omni(0.0, 10.0),   // 0: at the eye, kept
            omni(30.0, 10.0),  // 1: 3 reaches, kept
            omni(600.0, 10.0), // 2: 60 reaches, dropped
            omni(0.0, 0.0),    // 3: no reach, dropped
            capsule(200.0),    // 4: reach 3 + 1.46 = 4.46, 44.8 reaches, kept
            capsule(240.0),    // 5: 53.8 reaches, dropped
        ];
        assert_eq!(nearest_lights(&lights, Vec3::ZERO, 10), vec![0, 1, 4]);
        assert_eq!(nearest_lights(&lights, Vec3::ZERO, 2), vec![0, 1]);
        assert_eq!(nearest_lights(&lights, Vec3::new(-1.0, 0.0, 0.0), 10), vec![0, 1, 4]);
        assert!(nearest_lights(&lights, Vec3::new(f32::NAN, 0.0, 0.0), 10).is_empty());
        assert!(nearest_lights(&[], Vec3::ZERO, 10).is_empty());
        assert!(nearest_lights(&lights, Vec3::ZERO, 0).is_empty());
    }

    /// LS-42 (2) and (3): Shanghai's placed lights against the placed lamp-like models, and the lights
    /// within 150 m of the spawn. Writes `work\ls42\lamp_geometry.txt`. The outcome is reported, not asserted.
    #[test]
    fn point_light_ls42_lights_at_lamp_models() {
        let Some(zone) = install_path(r"levels\shanghai_01\zone.str") else { return };
        let Some(mission) = install_path(CSA_REL) else { return };
        let level = crate::level_render::load(&zone).unwrap();
        let is_lamp = |name: &str| {
            let n = name.to_ascii_lowercase();
            ["light", "lamp", "post"].iter().any(|k| n.contains(k))
        };
        let lamps: Vec<&crate::level_render::PlacedModel> = level.placed.iter().filter(|m| is_lamp(&m.model)).collect();
        let mut stems: std::collections::BTreeMap<String, usize> = Default::default();
        for m in &lamps {
            *stems.entry(m.model.split('.').next().unwrap_or("").to_string()).or_default() += 1;
        }
        let mut top: Vec<(String, usize)> = stems.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut report = format!("lamp-like placed models (name has light, lamp or post): {} of {} placed\ntop name stems:\n", lamps.len(), level.placed.len());
        for (stem, n) in top.iter().take(20) {
            report += &format!("  {n:6}  {stem}\n");
        }

        let lights = load_level_lights_placed(std::slice::from_ref(&zone)).unwrap();
        assert!(!lights.is_empty() && !lamps.is_empty());
        // Per light: (horizontal distance, height of the light above the lamp's origin, lamp index) of the nearest lamp model.
        let per_light: Vec<Option<(f32, f32, usize)>> = lights
            .iter()
            .map(|l| {
                lamps
                    .iter()
                    .enumerate()
                    .map(|(j, m)| {
                        let dx = m.world[9] - l.position.x;
                        let dy = m.world[10] - l.position.y;
                        ((dx * dx + dy * dy).sqrt(), l.position.z - m.world[11], j)
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
            })
            .collect();
        let within = |r: f32| per_light.iter().filter(|p| p.is_some_and(|(h, _, _)| h <= r)).count();
        report += &format!(
            "lights (zone, placed): {}. Nearest lamp model: within 1 m horizontally {}, within 5 m {}, further or none {}\n",
            lights.len(),
            within(1.0),
            within(5.0),
            lights.len() - within(5.0)
        );
        let mut heights: Vec<f32> = per_light.iter().filter_map(|p| p.as_ref()).filter(|p| p.0 <= 1.0).map(|p| p.1).collect();
        heights.sort_by(|a, b| a.total_cmp(b));
        if !heights.is_empty() {
            report += &format!(
                "height of the light above its nearest lamp's origin, lights within 1 m: n {}, p10 {:.2}, median {:.2}, p90 {:.2} m\n",
                heights.len(),
                heights[heights.len() / 10],
                heights[heights.len() / 2],
                heights[heights.len() * 9 / 10]
            );
        }
        let lamp_with_light = (0..lamps.len())
            .filter(|&j| {
                lights.iter().any(|l| {
                    let m = lamps[j];
                    let dx = m.world[9] - l.position.x;
                    let dy = m.world[10] - l.position.y;
                    (dx * dx + dy * dy).sqrt() <= 1.0
                })
            })
            .count();
        report += &format!("lamp models with a placed light within 1 m horizontally: {} of {}\n", lamp_with_light, lamps.len());
        for k in 0..5 {
            let i = k * lights.len() / 5;
            let l = &lights[i];
            let at = format!("light {:?} at ({:.2}, {:.2}, {:.2})", l.kind, l.position.x, l.position.y, l.position.z);
            report += &match per_light[i] {
                Some((h, up, j)) => {
                    let m = lamps[j];
                    format!(
                        "example {k}: {at}; nearest lamp {} at ({:.2}, {:.2}, {:.2}), {h:.2} m away horizontally, light {up:.2} m above its origin\n",
                        m.model, m.world[9], m.world[10], m.world[11]
                    )
                }
                None => format!("example {k}: {at}; no lamp model\n"),
            };
        }

        // The spawn: the first PlayerSpawnPoint of the zone and the mission (origin when none).
        let packs = [zone.clone(), mission.clone()];
        let spawns = crate::level_things::spawn_points(&packs).unwrap_or_default();
        let spawn = spawns.first().map(|s| s.pos);
        report += &format!("spawn points: {:?}\n", spawns.iter().map(|s| (s.name.clone(), s.pos.to_array())).collect::<Vec<_>>());
        let at = spawn.unwrap_or(Vec3::ZERO);
        if spawn.is_none() {
            report += "no spawn point found: the origin is used [assumed]\n";
        }
        let near = |ls: &[LevelLight]| kind_counts(&ls.iter().filter(|l| (l.position - at).length() < 150.0).cloned().collect::<Vec<_>>());
        let level_lights = load_level_lights_placed(&packs).unwrap();
        report += &format!(
            "within 150 m of the spawn (omni, spot, capsule): zone alone {:?}; zone and CSA mission {:?}\n",
            near(&lights),
            near(&level_lights)
        );
        ls42_report("lamp_geometry.txt", &report);
    }

    /// LS-42 (1): the Lights the gate misses, and the first ungated route to each (where they hang).
    /// Writes `work\ls42\gate_missed.txt`. Asserts the LS-38 count (5 omni, 8 spot, 0 capsule).
    #[test]
    fn point_light_gate_missed_lights_where_they_hang() {
        let Some(zone) = install_path(r"levels\shanghai_01\zone.str") else { return };
        let mission = install_path(CSA_REL);
        let mut report = String::new();
        let mut zone_kinds = (0, 0, 0);
        for (name, path) in [Some(zone), mission].into_iter().flatten().enumerate() {
            let placed = walk_lights(std::slice::from_ref(&path), true).unwrap();
            let object = walk_lights(std::slice::from_ref(&path), false).unwrap();
            let have: HashSet<LightKey> = placed.iter().map(|(k, _)| *k).collect();
            let missed: Vec<(LightKey, LevelLight)> = object.into_iter().filter(|(k, _)| !have.contains(k)).collect();
            let kinds = kind_counts(&missed.iter().map(|(_, l)| l.clone()).collect::<Vec<_>>());
            if name == 0 {
                zone_kinds = kinds;
            }
            report += &format!("pack {} (0 zone, 1 mission): {} gate-missed lights, (omni, spot, capsule) {kinds:?}\n", name, missed.len());
            let pack = Pack::open(&path).unwrap();
            for chunk in pack.of_type(pack::DATA) {
                let targets: HashSet<usize> = missed.iter().filter(|((_, c, _), _)| *c == chunk.index).map(|((_, _, at), _)| *at).collect();
                if targets.is_empty() {
                    continue;
                }
                let file = DataFile::parse(pack.data(chunk).to_vec()).unwrap();
                for (at, route) in light_routes(&file, &targets) {
                    let (_, light) = missed.iter().find(|((_, c, a), _)| *c == chunk.index && *a == at).unwrap();
                    report += &format!(
                        "chunk {} @ {at:#x}: {:?} at ({:.2}, {:.2}, {:.2}) range {} colour {:?}\n",
                        chunk.index,
                        light.kind,
                        light.position.x,
                        light.position.y,
                        light.position.z,
                        light.range,
                        light.colour.to_array()
                    );
                    for step in &route {
                        report += &format!("    {step}\n");
                    }
                }
            }
        }
        ls42_report("gate_missed.txt", &report);
        assert_eq!(zone_kinds, (5, 8, 0), "{report}");
    }

    /// LS-42 diagnostic: the first ungated route from a chunk's root to each Light at the given offsets,
    /// as labels from the root down (the object walk's order). Visits each typed node once, without matrices.
    fn light_routes(file: &DataFile, targets: &HashSet<usize>) -> Vec<(usize, Vec<String>)> {
        let mut found = Vec::new();
        let mut path = Vec::new();
        let mut seen = HashSet::new();
        route_visit(file.root(), "root", &mut path, targets, &mut seen, &mut found, 0);
        found
    }

    fn route_label(node: Node, field: &str) -> String {
        let name = node.get("name").and_then(Node::text).map(|n| format!(" name '{n}'")).unwrap_or_default();
        format!("{field}: {}{name}", node.type_name().unwrap_or("untyped"))
    }

    #[allow(clippy::too_many_arguments)]
    fn route_visit(
        node: Node,
        field: &str,
        path: &mut Vec<String>,
        targets: &HashSet<usize>,
        seen: &mut HashSet<(usize, usize)>,
        found: &mut Vec<(usize, Vec<String>)>,
        depth: u32,
    ) {
        if depth > WALK_DEPTH || found.len() == targets.len() {
            return;
        }
        if node.type_name() == Some("Light") {
            if let Some(at) = node.offset() {
                if targets.contains(&at) && !found.iter().any(|(a, _)| *a == at) {
                    let mut route = path.clone();
                    route.push(route_label(node, field));
                    found.push((at, route));
                }
            }
            return;
        }
        if let (Some(type_name), Some(at)) = (node.type_name(), node.offset()) {
            if !seen.insert((at, type_name.as_ptr() as usize)) {
                return;
            }
        }
        path.push(route_label(node, field));
        if let Some(attachments) = node.get("attachments") {
            for attachment in attachments.items() {
                if let Some(thing) = attachment.get("thing") {
                    route_visit(thing, "attachment thing", path, targets, seen, found, depth + 1);
                }
            }
        }
        for (hash, child) in node.fields() {
            let name = node.file().name(hash).unwrap_or("?");
            if matches!(name, "attachments" | "sub_matrices" | "matrix" | "hierarchy") {
                continue;
            }
            route_visit(child, name, path, targets, seen, found, depth + 1);
        }
        if node.len() > 0 {
            for child in node.items() {
                route_visit(child, "item", path, targets, seen, found, depth + 1);
            }
        }
        path.pop();
    }
}

#[cfg(test)]
mod flash_tests {
    use super::*;
    use glam::{Affine3A, Quat};

    const ROSTER: [&str; 13] = [
        "bumblebee", "optimus", "ratchet", "ironhide", "megatron", "longhaul", "audir8", "jetfire", "firestorm",
        "starscream", "blackout", "optimusflight", "megatronflight",
    ];

    /// [data] Bumblebee's gun's `muzzleflash_orange`: one OMNI Light at the object's origin and a
    /// 0.05 s TimeoutDeleter with no fade (export/data/bumblebee.txt).
    #[test]
    fn bumblebees_muzzle_light_is_the_orange_omni() {
        let Ok(lights) = load_flash_lights(std::path::Path::new("C:/Games2"), "bumblebee") else { return };
        let gun = &lights.first().expect("his gun has a muzzle flash object").1;
        assert_eq!(gun.light.kind, LightKind::Omni);
        assert!((gun.light.colour - Vec3::new(0.8, 0.749804, 0.611765)).abs().max_element() < 1e-5);
        assert_eq!((gun.light.range, gun.light.attenuate), (10.0, 6.0));
        assert_eq!(gun.light.position, Vec3::ZERO);
        assert_eq!((gun.time, gun.fade_start), (0.05, -1.0));
        // The gun and the car's gun share the object; the third weapon has none. Two rows each.
        assert_eq!(lights.len(), 4, "{:?}", lights.iter().map(|(n, _)| format!("{n:08x}")).collect::<Vec<_>>());
    }

    /// [game 007aac80, 00776f50] Spawned at the node's matrix, it stays there and is removed by the
    /// update that takes its count to zero: at the game's 0.032 s step it is seen for one update.
    #[test]
    fn a_spawned_flash_stays_put_and_lives_out_its_deleter() {
        let flash = FlashLight {
            light: LevelLight {
                kind: LightKind::Omni, position: Vec3::new(0.0, 0.5, 0.0), direction: Vec3::Z, colour: Vec3::ONE, colour2: Vec3::ZERO,
                range: 10.0, range2: 0.0, attenuate: 6.0, attenuate2: 0.0, hotspot: 0.0, falloff: 0.0, offset_len: 0.0,
                affect_diffuse: true, affect_specular: true, ambient_only: false, capsule_axis: Vec3::ZERO,
            },
            time: 0.05,
            fade_start: -1.0,
        };
        let at = Affine3A::from_rotation_translation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), Vec3::new(3.0, 4.0, 1.0));
        let mut live = flash.spawn(at);
        assert!((live.light.position - Vec3::new(2.5, 4.0, 1.0)).length() < 1e-5);
        assert!(live.step(0.032), "0.018 s left after the first update");
        assert!(!live.step(0.032), "removed at the second");
        // At 60 Hz: three updates see it at most.
        let mut live = flash.spawn(at);
        let seen = (0..10).take_while(|_| live.step(1.0 / 60.0)).count();
        assert!((2..=3).contains(&seen), "{seen}");
        // The colour stays whole with no fade until the removal.
        let mut live = flash.spawn(at);
        live.step(0.02);
        assert_eq!(live.light.colour, Vec3::ONE);
    }

    /// [game 00776f50, 0056b610, 0054c260] A blast's flash (time 0.25, fadeStart 0.12): the fade spans the
    /// last 0.13 s, so with 0.065 s left the colour is half. An omni takes the broadcast alpha through
    /// Light+0xf8 (the listener on key 611d70bc, 0056b610), so its colour is scaled by it.
    #[test]
    fn a_blast_flash_fades_over_its_deleters_last_stretch() {
        let light = LevelLight {
            kind: LightKind::Omni, position: Vec3::ZERO, direction: Vec3::Z, colour: Vec3::new(5.0, 4.0, 2.0), colour2: Vec3::ZERO,
            range: 15.0, range2: 0.0, attenuate: 7.0, attenuate2: 0.0, hotspot: 0.0, falloff: 0.0, offset_len: 0.0,
            affect_diffuse: true, affect_specular: true, ambient_only: false, capsule_axis: Vec3::ZERO,
        };
        let flash = FlashLight { light, time: 0.25, fade_start: 0.12 };
        let mut live = flash.spawn(Affine3A::IDENTITY);
        assert!(live.step(0.1));
        assert_eq!(live.light.colour, Vec3::new(5.0, 4.0, 2.0), "before the fade");
        assert!(live.step(0.085));
        assert!((live.light.colour - Vec3::new(2.5, 2.0, 1.0)).length() < 1e-4, "{:?}", live.light.colour);
    }

    /// [game 0054cd70] A capsule's colour has no Light+0xf8 factor: the same deleter fade leaves it whole.
    #[test]
    fn a_capsule_flash_keeps_its_colour_through_the_fade() {
        let light = LevelLight {
            kind: LightKind::Capsule, position: Vec3::ZERO, direction: Vec3::Z, colour: Vec3::new(5.0, 4.0, 2.0), colour2: Vec3::ZERO,
            range: 15.0, range2: 0.0, attenuate: 7.0, attenuate2: 0.0, hotspot: 0.0, falloff: 0.0, offset_len: 0.0,
            affect_diffuse: true, affect_specular: true, ambient_only: false, capsule_axis: Vec3::Z,
        };
        let flash = FlashLight { light, time: 0.25, fade_start: 0.12 };
        let mut live = flash.spawn(Affine3A::IDENTITY);
        assert!(live.step(0.085));
        assert_eq!(live.light.colour, Vec3::new(5.0, 4.0, 2.0), "a capsule does not fade");
    }

    /// Every playable pack's muzzle flash objects hold a readable light (printed for the notes).
    #[test]
    fn the_rosters_muzzle_lights_read() {
        let dir = std::path::Path::new("C:/Games2");
        if !dir.join("characters").is_dir() { return; }
        for pack in ROSTER {
            let lights = load_flash_lights(dir, pack).unwrap();
            for (name, flash) in lights.iter().skip(1).step_by(2) {
                println!("{pack} {name:08x}: {:?} colour {:?} range {} attenuate {} at {:?} time {} fade {}",
                    flash.light.kind, flash.light.colour, flash.light.range, flash.light.attenuate, flash.light.position, flash.time, flash.fade_start);
                assert!(flash.light.range > flash.light.attenuate && flash.time > 0.0);
            }
        }
    }
}
