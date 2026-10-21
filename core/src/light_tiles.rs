//! Conservative screen tiles for the port's forward light evaluation. This is rendering
//! scheduling, not original game code: it preserves every light that can reach a pixel.
use glam::{Mat4, UVec2, Vec2, Vec3};

pub const TILE_SIZE: u32 = 32;
pub const HEADER: usize = 8;

/// Header: grid x/y, tile size, viewport origin x/y, width/height, reserved.
/// Then (offset,count) for each tile and compact one-based light indices. No per-tile cap.
pub fn build(bounds: &[(Vec3, f32)], clip_from_world: Mat4, size: UVec2, origin: UVec2) -> Vec<u32> {
    if size.x == 0 || size.y == 0 { return vec![0; HEADER]; }
    let grid = UVec2::new(size.x.div_ceil(TILE_SIZE), size.y.div_ceil(TILE_SIZE));
    let mut lists = vec![Vec::new(); (grid.x * grid.y) as usize];
    for (index, &(centre, radius)) in bounds.iter().enumerate() {
        if radius <= 0.0 || !radius.is_finite() || !centre.is_finite() { continue; }
        let corners: Vec<_> = (0..8).map(|corner| {
            let sign = Vec3::new(if corner & 1 == 0 { -1.0 } else { 1.0 }, if corner & 2 == 0 { -1.0 } else { 1.0 }, if corner & 4 == 0 { -1.0 } else { 1.0 });
            clip_from_world * (centre + sign * radius).extend(1.0)
        }).collect();
        if corners.iter().all(|p| p.w <= 0.0) { continue; }
        let (mut lo, mut hi) = (Vec2::ZERO, size.as_vec2());
        if corners.iter().all(|p| p.w > 0.0) {
            lo = Vec2::splat(f32::INFINITY);
            hi = Vec2::splat(f32::NEG_INFINITY);
            for p in corners {
                let ndc = p.truncate().truncate() / p.w;
                let pixel = Vec2::new(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5) * size.as_vec2();
                lo = lo.min(pixel);
                hi = hi.max(pixel);
            }
            // Conservative raster rounding: extra candidates have zero physical influence.
            lo -= Vec2::ONE;
            hi += Vec2::ONE;
            if hi.x < 0.0 || hi.y < 0.0 || lo.x > size.x as f32 || lo.y > size.y as f32 { continue; }
            lo = lo.max(Vec2::ZERO);
            hi = hi.min(size.as_vec2());
        }
        let first = (lo / TILE_SIZE as f32).as_uvec2().min(grid - UVec2::ONE);
        let last = (hi / TILE_SIZE as f32).as_uvec2().min(grid - UVec2::ONE);
        for y in first.y..=last.y { for x in first.x..=last.x {
            lists[(y * grid.x + x) as usize].push(index as u32 + 1);
        }}
    }
    let mut words = vec![grid.x, grid.y, TILE_SIZE, origin.x, origin.y, size.x, size.y, 0];
    words.resize(HEADER + 2 * lists.len(), 0);
    for (i, list) in lists.into_iter().enumerate() {
        words[HEADER + 2*i] = words.len() as u32;
        words[HEADER + 2*i + 1] = list.len() as u32;
        words.extend(list);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn includes_every_projected_sphere_point_and_never_truncates_dense_tiles() {
        let size = UVec2::new(640, 360);
        let clip = Mat4::perspective_infinite_reverse_rh(1.0, 640.0/360.0, 0.5);
        let bounds = [(Vec3::new(2.0, 1.0, -10.0), 3.0), (Vec3::new(-4.0, 0.0, -20.0), 2.0)];
        let words = build(&bounds, clip, size, UVec2::ZERO);
        for (i, &(centre, radius)) in bounds.iter().enumerate() {
            for a in 0..36 { for b in 0..19 {
                let (theta, phi) = (a as f32 * std::f32::consts::TAU / 36.0, b as f32 * std::f32::consts::PI / 18.0);
                let p = clip * (centre + radius * Vec3::new(phi.sin()*theta.cos(), phi.sin()*theta.sin(), phi.cos())).extend(1.0);
                let ndc = p.truncate() / p.w;
                let pixel = Vec2::new(ndc.x*0.5+0.5, 0.5-ndc.y*0.5)*size.as_vec2();
                if pixel.x < 0.0 || pixel.y < 0.0 || pixel.x >= size.x as f32 || pixel.y >= size.y as f32 { continue; }
                let tile = pixel.as_uvec2() / TILE_SIZE;
                let slot = HEADER + 2 * (tile.y * words[0] + tile.x) as usize;
                let (at, count) = (words[slot] as usize, words[slot+1] as usize);
                assert!(words[at..at+count].contains(&(i as u32 + 1)));
            }}
        }
        let dense = build(&vec![(Vec3::ZERO, 10.0); 500], clip, UVec2::new(32,32), UVec2::ZERO);
        assert_eq!(dense[HEADER+1], 500);
        assert_eq!(dense.len(), HEADER+2+500);
    }
    #[test]
    fn behind_camera_and_off_screen_bounds_have_no_entries() {
        let clip = Mat4::perspective_infinite_reverse_rh(1.0, 1.0, 0.5);
        let words = build(&[(Vec3::new(0.0,0.0,10.0),1.0), (Vec3::new(100.0,0.0,-10.0),1.0)], clip, UVec2::new(32,32), UVec2::ZERO);
        assert_eq!(words[HEADER+1], 0);
    }
}
