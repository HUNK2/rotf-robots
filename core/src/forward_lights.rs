//! The game's per-object light set: the two OMNI and two SPOT lights a forward draw gets, picked
//! through a depth-sliced light grid, and the 13-float4 constant block they are written into.
//! `notes\render-144.md` section 1 (this is the writer `notes\level-point-lights.md` LS-30 looked for).
//!
//! [game 0054bdc0] Per frame a grid takes the camera's 4x4 (cam+0x150), near (cam+0x3cc) and far
//! (cam+0x3d0). A point's depth is `dot(p, column 2) + m[14]`.
//! [game 0054be30, 0054bec0] Slices: `lo = floor(sqrt(clamp01((z - near) / (far - near))) * 127)`,
//! `hi = ceil(...)` (capped at 128, which the clamp never reaches), so 128 slices finer near the eye.
//! [game 0054bf50] A light goes into every slice from `lo(z - range)` to `hi(z + range)`, at most 32
//! per slice (a 33rd is dropped from that slice). [game 0054c260, 0054c7f0] Only lights that are not
//! `ambientOnly` and have `affectSpecular` are put in the grid (the with-spec list, at most 512).
//! [game 0054c010] The query for a sphere walks the slices of `z -/+ r` in order, each light once,
//! keeps a light when `(light mask & object mask) != 0` and `|light - centre| < range + r`, and scores
//! it `|light - centre| / range`. The first two kept fill the two slots; every later one REPLACES EACH
//! slot whose score is worse (no break, disasm 0054c200..0054c21e), so a light better than both fills
//! both slots and the object is lit by it twice. That is the game's behaviour and is kept.
//! [game 00589390] The block: see [`Block`].
//! [game 0058a1f0] Which draws get their own block, and the object mask: see [`uses_own_block`],
//! [`object_mask`].

use glam::{Vec3, Vec4};

use crate::point_light::{LevelLight, LightKind};

/// Slices of the grid that can be reached (0..=127).
pub const SLICES: usize = 128;
/// [game 0054bf50] Lights one slice can hold.
pub const SLICE_CAPACITY: usize = 32;
/// [game 0054c799, 0054cd29] The with-spec record list's limit; a light past it is not put in the grid.
pub const LIST_CAPACITY: usize = 512;

/// One light as the grid and the block read it (the runtime Light's fields).
#[derive(Clone, Debug, PartialEq)]
pub struct GridLight {
    /// Light+0xa0..+0xa8.
    pub position: Vec3,
    /// Light+0xb0..+0xb8 (spots).
    pub direction: Vec3,
    /// Light+0x80..+0x8c. [assumed] alpha 1 (the data's colour has three values).
    pub colour: Vec4,
    /// Light+0xf8: the constructor's 1.0 (no data field writes it).
    pub intensity: f32,
    /// Light+0xc4 range, Light+0xcc attenuate.
    pub range: f32,
    pub attenuate: f32,
    /// Light+0xdc cos(hotspot / 2), Light+0xe0 cos(falloff / 2) (spots).
    pub cos_hot: f32,
    pub cos_fall: f32,
    /// Light+0xfc: the constructor's 0xffffffff (every group). [assumed] no data in the levels the port
    /// loads changes it (LightMask / LightGroupAttachment not read).
    pub mask: u32,
}

impl GridLight {
    /// The grid's view of a level light; `None` for lights the grid never holds (capsules, ambient-only
    /// lights, lights without affectSpecular) [game 0054c260, 0054c7f0].
    pub fn from_level(light: &LevelLight) -> Option<GridLight> {
        if light.kind == LightKind::Capsule || light.ambient_only || !light.affect_specular {
            return None;
        }
        Some(GridLight {
            position: light.position,
            direction: light.direction,
            colour: light.colour.extend(1.0),
            intensity: 1.0,
            range: light.range,
            attenuate: light.attenuate,
            cos_hot: (light.hotspot * 0.5).cos(),
            cos_fall: (light.falloff * 0.5).cos(),
            mask: u32::MAX,
        })
    }

    fn lit_colour(&self) -> Vec4 {
        self.colour * self.intensity
    }
}

/// One frame's grid (00d0dd30 for omni lights, 00d12190 for spots).
#[derive(Clone, Debug)]
pub struct LightGrid {
    /// The camera matrix's column 2 as (m[2], m[6], m[10], m[14]): `depth(p) = dot(p, xyz) + w`.
    depth: Vec4,
    near: f32,
    inv_range: f32,
    slices: Vec<Vec<usize>>,
    lights: Vec<GridLight>,
}

impl LightGrid {
    /// [game 0054bdc0] An empty grid for one camera.
    pub fn new(depth: Vec4, near: f32, far: f32) -> LightGrid {
        LightGrid { depth, near, inv_range: 1.0 / (far - near), slices: vec![Vec::new(); SLICES + 1], lights: Vec::new() }
    }

    fn z(&self, p: Vec3) -> f32 {
        self.depth.truncate().dot(p) + self.depth.w
    }

    fn unit(&self, z: f32) -> f32 {
        let x = (z - self.near) * self.inv_range;
        // [game 0054be36..0054be66] x when 0 < x < 1, else 0 below and 1 above.
        if x > 0.0 && x < 1.0 { x } else if x > 0.0 { 1.0 } else { 0.0 }
    }

    /// [game 0054be30]
    pub fn slice_lo(&self, z: f32) -> usize {
        (self.unit(z).sqrt() * 127.0).floor() as usize
    }

    /// [game 0054bec0]
    pub fn slice_hi(&self, z: f32) -> usize {
        let hi = (self.unit(z).sqrt() * 127.0).ceil() as usize;
        if hi > 127 { 128 } else { hi }
    }

    /// [game 0054bf50] Put a light in the slices its depth +/- range covers. Returns its index.
    pub fn insert(&mut self, light: GridLight) -> usize {
        let index = self.lights.len();
        let z = self.z(light.position);
        let (lo, hi) = (self.slice_lo(z - light.range), self.slice_hi(light.range + z));
        for slice in &mut self.slices[lo..=hi] {
            if slice.len() < SLICE_CAPACITY {
                slice.push(index);
            }
        }
        self.lights.push(light);
        index
    }

    /// [game 0054bf50 callers] Fill the grid from a level's lights of one kind, in the given order,
    /// up to the 512 of the with-spec list.
    pub fn fill(&mut self, lights: &[LevelLight], kind: LightKind) {
        for light in lights.iter().filter(|l| l.kind == kind).filter_map(GridLight::from_level).take(LIST_CAPACITY) {
            self.insert(light);
        }
    }

    pub fn light(&self, index: usize) -> &GridLight {
        &self.lights[index]
    }

    /// [game 0054c010] The (at most two) lights of a sphere, as indices; a light may appear twice.
    pub fn query(&self, centre: Vec3, radius: f32, mask: u32) -> Vec<usize> {
        let z = self.z(centre);
        let (lo, hi) = (self.slice_lo(z - radius), self.slice_hi(z + radius));
        let mut seen = vec![false; self.lights.len()];
        let mut slots: Vec<(usize, f32)> = Vec::with_capacity(2);
        for slice in &self.slices[lo..=hi] {
            for &index in slice {
                if std::mem::replace(&mut seen[index], true) {
                    continue;
                }
                let light = &self.lights[index];
                let reach = light.range + radius;
                if light.mask & mask == 0 || (light.position - centre).length_squared() >= reach * reach {
                    continue;
                }
                let score = (light.position - centre).length() / light.range;
                if slots.len() < 2 {
                    slots.push((index, score));
                } else {
                    for slot in &mut slots {
                        if score < slot.1 {
                            *slot = (index, score);
                        }
                    }
                }
            }
        }
        slots.into_iter().map(|(index, _)| index).collect()
    }
}

/// [game 00589390] The per-object block, 13 float4 (0xd0 bytes), the layout of the default 00dec620.
/// Index: 0..1 omni positions (w 1); 2..3 omni colours (colour x intensity; `[2].w` is the omni COUNT);
/// 4 omni parameters (attenuate0, range0, attenuate1, range1); 5..6 spot positions; 7..8 spot directions
/// (w 0); 9..10 spot parameters (cos hot/2, cos fall/2, attenuate, range); 11..12 spot colours (`[11].w`
/// is the spot COUNT). Unused slots are zero, so a count of 0 leaves `[2].w` = 0 and the shader's omni
/// gate (`c14.w > 0`) skips the loop.
pub type Block = [Vec4; 13];

/// [game 00589390] Build an object's block from the two grids for its world bounding sphere.
pub fn object_block(omni: &LightGrid, spot: &LightGrid, centre: Vec3, radius: f32, mask: u32) -> Block {
    let mut b = [Vec4::ZERO; 13];
    let omnis = omni.query(centre, radius, mask);
    for (i, &index) in omnis.iter().enumerate() {
        let l = omni.light(index);
        b[i] = l.position.extend(1.0);
        b[2 + i] = l.lit_colour();
        b[4][2 * i] = l.attenuate;
        b[4][2 * i + 1] = l.range;
    }
    b[2].w = omnis.len() as f32;
    let spots = spot.query(centre, radius, mask);
    for (i, &index) in spots.iter().enumerate() {
        let l = spot.light(index);
        b[5 + i] = l.position.extend(1.0);
        b[7 + i] = l.direction.extend(0.0);
        b[9 + i] = Vec4::new(l.cos_hot, l.cos_fall, l.attenuate, l.range);
        b[11 + i] = l.lit_colour();
    }
    b[11].w = spots.len() as f32;
    b
}

/// [game 0058a1f0] The object's light mask: `(Object+0x60 >> 12 & 0xff) * 2`, or 1 when those bits are 0.
pub fn object_mask(object_flags: u32) -> u32 {
    match object_flags >> 12 & 0xff {
        0 => 1,
        bits => bits * 2,
    }
}

/// [game 0058a1f0] Whether a draw row gets its own block (else the all-zero default 00d17710, no omni or
/// spot light): row flag 0x10 clear, a main mesh, and either row flag 0x08 clear, the primary draw
/// fading (fade < 1), or a secondary (LOD blend) draw without row flag 0x4000.
/// [unknown] what sets row flags 0x08 and 0x10 (not 00587490 or 00587a70).
pub fn uses_own_block(row_flags: u32, primary_fade: f32, has_secondary: bool, has_main: bool) -> bool {
    let secondary = has_secondary && row_flags & 0x4000 == 0;
    row_flags & 0x10 == 0 && (row_flags & 0x08 == 0 || primary_fade < 1.0 || secondary) && has_main
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera looking down +y from the origin: depth = y, near 0.1, far 1000.1.
    fn grid() -> LightGrid {
        LightGrid::new(Vec4::new(0.0, 1.0, 0.0, 0.0), 0.1, 1000.1)
    }

    fn omni(x: f32, y: f32, range: f32) -> GridLight {
        GridLight { position: Vec3::new(x, y, 0.0), direction: Vec3::Z, colour: Vec4::new(1.0, 0.5, 0.25, 1.0), intensity: 1.0, range, attenuate: range * 0.5, cos_hot: 0.9, cos_fall: 0.8, mask: u32::MAX }
    }

    #[test]
    fn slices_follow_the_square_root_of_depth() {
        let g = grid();
        assert_eq!((g.slice_lo(-5.0), g.slice_hi(-5.0)), (0, 0));
        assert_eq!((g.slice_lo(5000.0), g.slice_hi(5000.0)), (127, 127));
        // z = near + 0.25 * range: sqrt = 0.5, 63.5.
        assert_eq!((g.slice_lo(250.1), g.slice_hi(250.1)), (63, 64));
    }

    #[test]
    fn the_first_two_fill_and_a_better_one_takes_every_worse_slot() {
        let mut g = grid();
        let a = g.insert(omni(0.0, 50.0, 10.0)); // centre at y 50: score 0
        let _b = g.insert(omni(4.0, 50.0, 10.0)); // 0.4: replaced by c
        let c = g.insert(omni(2.0, 50.0, 10.0)); // 0.2: better than b only
        assert_eq!(g.query(Vec3::new(0.0, 50.0, 0.0), 1.0, 1), vec![a, c]);
        let mut g = grid();
        let far = g.insert(omni(8.0, 50.0, 10.0)); // 0.8
        let _mid = g.insert(omni(6.0, 50.0, 10.0)); // 0.6
        let best = g.insert(omni(1.0, 50.0, 10.0)); // 0.1: beats both, fills both
        let _ = far;
        assert_eq!(g.query(Vec3::new(0.0, 50.0, 0.0), 1.0, 1), vec![best, best]);
    }

    #[test]
    fn range_plus_radius_and_the_mask_decide() {
        let mut g = grid();
        g.insert(omni(12.0, 50.0, 10.0));
        let centre = Vec3::new(0.0, 50.0, 0.0);
        assert!(g.query(centre, 1.0, 1).is_empty(), "12 is not < 10 + 1");
        assert_eq!(g.query(centre, 2.5, 1).len(), 1);
        let mut g = grid();
        g.insert(GridLight { mask: 0b100, ..omni(0.0, 50.0, 10.0) });
        assert!(g.query(centre, 1.0, 0b010).is_empty());
        assert_eq!(g.query(centre, 1.0, 0b110).len(), 1);
    }

    #[test]
    fn a_slice_holds_32_lights() {
        let mut g = grid();
        for i in 0..40 {
            g.insert(omni(i as f32 * 0.01, 500.0, 0.001));
        }
        let z = 500.0;
        let s = g.slice_lo(z);
        assert_eq!(g.slices[s].len(), SLICE_CAPACITY);
    }

    #[test]
    fn the_block_has_the_counts_in_the_first_colour_w() {
        let mut o = grid();
        let s = grid();
        o.insert(omni(1.0, 50.0, 10.0));
        let b = object_block(&o, &s, Vec3::new(0.0, 50.0, 0.0), 1.0, 1);
        assert_eq!(b[0], Vec4::new(1.0, 50.0, 0.0, 1.0));
        assert_eq!(b[2], Vec4::new(1.0, 0.5, 0.25, 1.0));
        assert_eq!(b[4], Vec4::new(5.0, 10.0, 0.0, 0.0));
        assert_eq!((b[1], b[3]), (Vec4::ZERO, Vec4::ZERO));
        assert_eq!(b[11], Vec4::ZERO, "no spot: count 0");
        let none = object_block(&s, &s, Vec3::ZERO, 1.0, 1);
        assert_eq!(none, [Vec4::ZERO; 13]);
    }

    #[test]
    fn object_mask_and_row_gate() {
        assert_eq!(object_mask(0), 1);
        assert_eq!(object_mask(0x3000), 6);
        assert_eq!(object_mask(0x800 | 0x4), 1);
        assert!(uses_own_block(0, 1.0, false, true));
        assert!(!uses_own_block(0x10, 0.5, true, true));
        assert!(!uses_own_block(0x08, 1.0, false, true));
        assert!(uses_own_block(0x08, 0.5, false, true));
        assert!(uses_own_block(0x08, 1.0, true, true));
        assert!(!uses_own_block(0x4008, 1.0, true, true));
        assert!(!uses_own_block(0, 1.0, false, false));
    }
}
