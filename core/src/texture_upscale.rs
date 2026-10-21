//! Optional texture enhancement for the rebuild, not an original-game mechanic.
//! All pixels come from the user's installed pack. No game assets are bundled.
//! Colour is resampled with Lanczos; shader data/masks use non-ringing linear filtering.
//! The original authored mip levels follow the added levels, preserving distant detail.

use std::io::{Cursor, Read};
use std::path::Path;

use image::codecs::dds::DdsDecoder;
use image::imageops::{self, FilterType};
use image::{DynamicImage, GrayImage, RgbaImage};

use crate::formats::texture::{PixelFormat, Texture};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapKind {
    Colour,
    /// Normal xy, specular and emissive values are shader data, not photographs.
    Data,
}

/// Only the robots' own surface/weapon maps: a robot's `characters\<robot>\maps` and its
/// `global\weapons\<robot>\maps` (Bumblebee's alone until 2026-10-19, extended to the roster at
/// the user's request). Shared effects, level art, the humans and cube maps stay original.
/// [data] Paths and file names from every `characters\*.str` texture chunk
/// (`robot_material_audit --textures`): colour maps are named `_color` / `_c`, shader data
/// `_normal` / `_n`, `_spec` / `_specular` / `_s`, and the glow and light pictures `glow` /
/// `taillight`. A file named otherwise (Ironhide's `4x4.dds`) is left as it is.
pub fn map_kind(texture: &Texture) -> Option<MapKind> {
    if texture.faces != 1 {
        return None;
    }
    let name = texture.name.replace('\\', "/").to_ascii_lowercase();
    let (parent, file) = name.rsplit_once('/')?;
    let (owner_dir, maps) = parent.rsplit_once('/')?;
    let (kind_dir, _robot) = owner_dir.rsplit_once('/')?;
    let own = maps == "maps" && (kind_dir.ends_with("/characters") || kind_dir.ends_with("/global/weapons"));
    if !own {
        return None;
    }
    let stem = file.strip_suffix(".dds")?;
    // Trailing digits number a robot's sheets (`megatron_color3`): they do not change the kind.
    let base = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    if base.contains("color") || base.ends_with("_c") {
        Some(MapKind::Colour)
    } else if base.contains("normal")
        || base.contains("spec")
        || base.contains("glow")
        || base == "taillight"
        || base.ends_with("_n")
        || base.ends_with("_s")
    {
        Some(MapKind::Data)
    } else {
        None
    }
}

fn level_bytes(format: PixelFormat, width: u32, height: u32) -> usize {
    let blocks = width.div_ceil(4) as usize * height.div_ceil(4) as usize;
    match format {
        PixelFormat::Dxt1 => blocks * 8,
        PixelFormat::Dxt3 | PixelFormat::Dxt5 => blocks * 16,
        PixelFormat::Bgra8 => width as usize * height as usize * 4,
        PixelFormat::Bgr8 => width as usize * height as usize * 3,
        PixelFormat::L8 => width as usize * height as usize,
    }
}

/// Decode one mip, including BC1's transparent index and the tiny padded block levels.
fn decode(format: PixelFormat, width: u32, height: u32, bytes: &[u8]) -> Result<RgbaImage, String> {
    if bytes.len() != level_bytes(format, width, height) {
        return Err("texture mip has the wrong byte count".into());
    }
    let fourcc = match format {
        PixelFormat::Dxt1 => Some(*b"DXT1"),
        PixelFormat::Dxt3 => Some(*b"DXT3"),
        PixelFormat::Dxt5 => Some(*b"DXT5"),
        _ => None,
    };
    if let Some(fourcc) = fourcc {
        let (padded_w, padded_h) = (width.div_ceil(4) * 4, height.div_ceil(4) * 4);
        let mut header = [0u8; 128];
        header[..4].copy_from_slice(b"DDS ");
        for (offset, word) in [
            (4, 124u32),
            (8, 0x81007),
            (12, padded_h),
            (16, padded_w),
            (20, bytes.len() as u32),
            (28, 1),
            (76, 32),
            (80, 4),
            (108, 0x1000),
        ] {
            header[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        header[84..88].copy_from_slice(&fourcc);
        let decoder = DdsDecoder::new(Cursor::new(header).chain(Cursor::new(bytes)))
            .map_err(|e| e.to_string())?;
        let mut image = DynamicImage::from_decoder(decoder)
            .map_err(|e| e.to_string())?
            .into_rgba8();
        // image's DXT1 decoder returns RGB. Recover the one-bit alpha the GPU uses.
        if format == PixelFormat::Dxt1 {
            for (block, bytes) in bytes.chunks_exact(8).enumerate() {
                let c0 = u16::from_le_bytes([bytes[0], bytes[1]]);
                let c1 = u16::from_le_bytes([bytes[2], bytes[3]]);
                if c0 > c1 {
                    continue;
                }
                let indices = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
                let bx = block as u32 % (padded_w / 4) * 4;
                let by = block as u32 / (padded_w / 4) * 4;
                for i in 0..16u32 {
                    if (indices >> (2 * i)) & 3 == 3 {
                        image.get_pixel_mut(bx + i % 4, by + i / 4)[3] = 0;
                    }
                }
            }
        }
        return Ok(imageops::crop_imm(&image, 0, 0, width, height).to_image());
    }
    let rgba: Vec<u8> = match format {
        PixelFormat::Bgra8 => bytes
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect(),
        PixelFormat::Bgr8 => bytes
            .chunks_exact(3)
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect(),
        PixelFormat::L8 => bytes.iter().flat_map(|&p| [p, p, p, 255]).collect(),
        _ => unreachable!(),
    };
    RgbaImage::from_raw(width, height, rgba).ok_or_else(|| "invalid texture dimensions".into())
}

fn resized(source: &RgbaImage, width: u32, height: u32, kind: MapKind) -> RgbaImage {
    // [assumed] These enhancement filters are a fidelity/clarity choice, not recovered
    // game behaviour. Compare close-up captures; see notes/status.md's texture entry.
    let filter = if kind == MapKind::Colour {
        FilterType::Lanczos3
    } else {
        FilterType::Triangle
    };
    let mut result = imageops::resize(source, width, height, filter);
    // Alpha is paint coverage/transparency, not colour: no ringing, no premultiplication.
    // [data/game] The paint shaders mix their paint ramp with RGB using this mask.
    if kind == MapKind::Colour {
        let alpha = GrayImage::from_fn(source.width(), source.height(), |x, y| {
            image::Luma([source.get_pixel(x, y)[3]])
        });
        let alpha = imageops::resize(&alpha, width, height, FilterType::Triangle);
        for (p, a) in result.pixels_mut().zip(alpha.pixels()) {
            p[3] = a[0];
        }
    }
    result
}

/// Add 2x/4x levels in stored-value space, matching the existing shaders' sampling.
/// This is interpolation: it does not recover missing detail or redraw UV islands.
pub fn upscale(source: &Texture, factor: u32, kind: MapKind) -> Result<Texture, String> {
    // The user selected faithful 4x enhancement; the renderer also offers 1x/2x.
    if !matches!(factor, 2 | 4)
        || source.faces != 1
        || source.width == 0
        || source.height == 0
        || source.mips == 0
    {
        return Err(
            "upscaling needs one face, nonzero dimensions/mips and a factor of 2 or 4".into(),
        );
    }
    let width = source
        .width
        .checked_mul(factor)
        .filter(|&w| w <= 8192)
        .ok_or("upscaled width exceeds 8192")?;
    let height = source
        .height
        .checked_mul(factor)
        .filter(|&h| h <= 8192)
        .ok_or("upscaled height exceeds 8192")?;
    let max_mips = source.width.max(source.height).ilog2() + 1;
    if source.mips > max_mips {
        return Err("too many source mip levels".into());
    }
    let sizes: Vec<_> = (0..source.mips)
        .map(|level| {
            let (w, h) = (
                (source.width >> level).max(1),
                (source.height >> level).max(1),
            );
            (w, h, level_bytes(source.format, w, h))
        })
        .collect();
    if sizes.iter().map(|s| s.2).sum::<usize>() != source.pixels.len() {
        return Err("source texture mip chain has the wrong byte count".into());
    }
    let top = decode(
        source.format,
        source.width,
        source.height,
        &source.pixels[..sizes[0].2],
    )?;
    let added = factor.ilog2();
    let capacity: usize = (0..source.mips + added)
        .map(|level| (width >> level).max(1) as usize * (height >> level).max(1) as usize * 4)
        .sum();
    let mut pixels = Vec::with_capacity(capacity);
    let mut append = |image: RgbaImage| {
        for p in image.pixels() {
            pixels.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    };
    for level in 0..added {
        append(resized(
            &top,
            (width >> level).max(1),
            (height >> level).max(1),
            kind,
        ));
    }
    append(top);
    let mut offset = sizes[0].2;
    for &(w, h, size) in &sizes[1..] {
        append(decode(
            source.format,
            w,
            h,
            &source.pixels[offset..offset + size],
        )?);
        offset += size;
    }
    Ok(Texture {
        name: source.name.clone(),
        id: source.id,
        width,
        height,
        faces: 1,
        mips: source.mips + added,
        format: PixelFormat::Bgra8,
        pixels,
    })
}

/// The top level decoded to RGBA.
pub fn top_rgba(texture: &Texture) -> Result<RgbaImage, String> {
    let count = level_bytes(texture.format, texture.width, texture.height);
    let bytes = texture.pixels.get(..count).ok_or("missing top mip")?;
    decode(texture.format, texture.width, texture.height, bytes)
}

/// Save a top level for inspection; no original-game files are changed.
pub fn save_png(texture: &Texture, path: &Path) -> Result<(), String> {
    top_rgba(texture)?.save(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture(format: PixelFormat, pixels: Vec<u8>) -> Texture {
        Texture {
            name: "c:/tf2/assets/characters/bumblebee/maps/bumblebee_color.dds".into(),
            id: 123,
            width: 4,
            height: 4,
            faces: 1,
            mips: 1,
            format,
            pixels,
        }
    }

    #[test]
    fn bc1_keeps_transparent_index_and_tiny_mips() {
        let block = [0, 0, 255, 255, 255, 255, 255, 255];
        let decoded = decode(PixelFormat::Dxt1, 4, 4, &block).unwrap();
        assert!(decoded.pixels().all(|p| p.0 == [0, 0, 0, 0]));
        let tiny = decode(PixelFormat::Dxt1, 1, 1, &block).unwrap();
        assert_eq!(tiny.dimensions(), (1, 1));
        assert_eq!(tiny.get_pixel(0, 0).0, [0, 0, 0, 0]);
        let opaque = decode(PixelFormat::Dxt1, 4, 4, &[0, 248, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(opaque.pixels().all(|p| p.0 == [255, 0, 0, 255]));
    }

    #[test]
    fn bc2_explicit_alpha_and_bc3_interpolated_alpha() {
        let colour = [0, 248, 0, 0, 0, 0, 0, 0];
        let bc2 = [vec![0x88; 8], colour.to_vec()].concat();
        assert!(
            decode(PixelFormat::Dxt3, 4, 4, &bc2)
                .unwrap()
                .pixels()
                .all(|p| p.0 == [255, 0, 0, 136])
        );
        // Every BC3 pixel selects alpha index 2: (6*210+0)/7 = 180.
        let indices = (0..16)
            .fold(0u64, |bits, i| bits | (2u64 << (3 * i)))
            .to_le_bytes();
        let bc3 = [vec![210, 0], indices[..6].to_vec(), colour.to_vec()].concat();
        assert!(
            decode(PixelFormat::Dxt5, 4, 4, &bc3)
                .unwrap()
                .pixels()
                .all(|p| p.0 == [255, 0, 0, 180])
        );
    }

    #[test]
    fn original_mips_are_preserved_and_bgra_is_correct() {
        let mut source = texture(PixelFormat::Bgra8, [30, 20, 10, 136].repeat(16));
        source.mips = 3;
        source.pixels.extend([60, 50, 40, 255].repeat(4));
        source.pixels.extend([90, 80, 70, 17]);
        let result = upscale(&source, 4, MapKind::Colour).unwrap();
        assert_eq!(
            (result.width, result.height, result.mips, result.id),
            (16, 16, 5, 123)
        );
        assert_eq!(result.format, PixelFormat::Bgra8);
        assert_eq!(&result.pixels[(16 * 16 + 8 * 8) * 4..], &source.pixels);
        assert!(
            result.pixels[..16 * 16 * 4]
                .chunks_exact(4)
                .all(|p| p == [30, 20, 10, 136])
        );
        assert_eq!(
            decode(PixelFormat::Bgra8, 1, 1, &[30, 20, 10, 136])
                .unwrap()
                .get_pixel(0, 0)
                .0,
            [10, 20, 30, 136]
        );
    }

    #[test]
    fn data_and_alpha_filters_do_not_ring_or_premultiply() {
        let source = RgbaImage::from_fn(4, 4, |x, _| {
            image::Rgba(if x < 2 {
                [100, 20, 5, 120]
            } else {
                [200, 40, 10, 180]
            })
        });
        let data = resized(&source, 16, 16, MapKind::Data);
        assert!(
            data.pixels()
                .all(|p| (100..=200).contains(&p[0]) && (120..=180).contains(&p[3]))
        );
        let colour = resized(&source, 16, 16, MapKind::Colour);
        assert!(colour.pixels().all(|p| (120..=180).contains(&p[3])));
        let same_rgb = RgbaImage::from_fn(4, 4, |x, _| {
            image::Rgba([77, 88, 99, if x < 2 { 0 } else { 255 }])
        });
        assert!(
            resized(&same_rgb, 16, 16, MapKind::Colour)
                .pixels()
                .all(|p| p.0[..3] == [77, 88, 99])
        );
    }

    /// [data] Names from the roster's packs (`robot_material_audit --textures`).
    #[test]
    fn every_robots_own_maps_are_selected_and_shared_ones_are_not() {
        let kind = |path: &str| {
            let mut t = texture(PixelFormat::Dxt1, vec![0; 8]);
            t.name = path.into();
            map_kind(&t)
        };
        let colour = Some(MapKind::Colour);
        let data = Some(MapKind::Data);
        assert_eq!(kind(r"c:\tf2\assets\characters\megatron\maps\megatron_color3.dds"), colour);
        assert_eq!(kind(r"c:\tf2\assets\characters\starscream\maps\starv01_c.dds"), colour);
        assert_eq!(kind(r"c:\tf2\assets\characters\megatron_gold\maps\megatron_gold_vehicle_color2.dds"), colour);
        assert_eq!(kind(r"c:\tf2\assets\global\weapons\optimus\maps\optimus_meleeweapon_color.dds"), colour);
        assert_eq!(kind(r"c:\tf2\assets\characters\optimus\maps\optimustruck_normal.dds"), data);
        assert_eq!(kind(r"c:\tf2\assets\characters\starscream\maps\starv01_n.dds"), data);
        assert_eq!(kind(r"c:\tf2\assets\characters\dcomscout\maps\scout_r_specular.dds"), data);
        assert_eq!(kind(r"c:\tf2\assets\characters\optimus\maps\optimus_fx_s.dds"), data);
        assert_eq!(kind(r"c:\tf2\assets\characters\starscream\maps\red_glow.dds"), data);
        assert_eq!(kind(r"c:\tf2\assets\characters\bumblebee\maps\taillight.dds"), data);
        assert_eq!(kind(r"c:\tf2\assets\characters\ironhide\maps\4x4.dds"), None);
        assert_eq!(kind(r"c:\tf2\assets\characters\humans\soldiermale\maps\soldier_nest_color.dds"), None);
        assert_eq!(kind(r"c:\tf2\assets\global\maps\outputcube.dds"), None);
        assert_eq!(kind(r"c:\tf2\assets\effects\textures\color\fx_smokecloud07.dds"), None);
        assert_eq!(kind(r"c:\tf2\assets\art_sets\urban\roads\maps\broken_road_c.dds"), None);
    }

    #[test]
    fn only_own_maps_are_selected_and_bad_data_returns_errors() {
        let mut t = texture(PixelFormat::Dxt1, vec![0; 8]);
        assert_eq!(map_kind(&t), Some(MapKind::Colour));
        t.name = t.name.replace("bumblebee_color", "outputcube");
        assert_eq!(map_kind(&t), None);
        t.name = t.name.replace("outputcube", "bumblebee_normal");
        assert_eq!(map_kind(&t), Some(MapKind::Data));
        t.faces = 6;
        assert_eq!(map_kind(&t), None);
        assert!(upscale(&t, 4, MapKind::Data).is_err());
        t.faces = 1;
        t.pixels.pop();
        assert!(upscale(&t, 4, MapKind::Data).is_err());
        assert!(upscale(&t, 3, MapKind::Data).is_err());
    }
}
