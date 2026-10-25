//! A texture of the pack as files: the game's own data as a `.dds` (every level, every face, as
//! the pack holds it), and the top level of a flat texture as a `.png` for tools that want one.

use tf2_core::formats::texture::{PixelFormat, Texture};

/// The texture as a DDS file: the standard 128-byte header over the pack's pixels, which are
/// already laid out as DDS wants them (each face's levels after it).
pub fn dds(texture: &Texture) -> Vec<u8> {
    const CAPS: u32 = 0x1;
    const HEIGHT: u32 = 0x2;
    const WIDTH: u32 = 0x4;
    const PITCH: u32 = 0x8;
    const PIXEL_FORMAT: u32 = 0x1000;
    const MIPMAP_COUNT: u32 = 0x20000;
    const LINEAR_SIZE: u32 = 0x80000;
    const FOURCC: u32 = 0x4;
    const RGB: u32 = 0x40;
    const ALPHA_PIXELS: u32 = 0x1;
    const LUMINANCE: u32 = 0x20000;
    let (w, h) = (texture.width, texture.height);
    let blocks = |bytes: u32| w.div_ceil(4).max(1) * h.div_ceil(4).max(1) * bytes;
    // (pixel format flags, fourcc, bits, red, green, blue, alpha masks), and the top level's size.
    let (format, size_flag, size) = match texture.format {
        PixelFormat::Dxt1 => ((FOURCC, *b"DXT1", 0, 0, 0, 0, 0), LINEAR_SIZE, blocks(8)),
        PixelFormat::Dxt3 => ((FOURCC, *b"DXT3", 0, 0, 0, 0, 0), LINEAR_SIZE, blocks(16)),
        PixelFormat::Dxt5 => ((FOURCC, *b"DXT5", 0, 0, 0, 0, 0), LINEAR_SIZE, blocks(16)),
        PixelFormat::Bgr8 => ((RGB, [0; 4], 24, 0xff_0000, 0xff00, 0xff, 0), PITCH, w * 3),
        PixelFormat::Bgra8 => ((RGB | ALPHA_PIXELS, [0; 4], 32, 0xff_0000, 0xff00, 0xff, 0xff00_0000), PITCH, w * 4),
        PixelFormat::L8 => ((LUMINANCE, [0; 4], 8, 0xff, 0, 0, 0), PITCH, w),
    };
    let cube = texture.faces == 6;
    let caps = 0x1000 | if texture.mips > 1 { 0x40_0008 } else { 0 } | if cube { 0x8 } else { 0 };
    let caps2 = if cube { 0x200 | 0xfc00 } else { 0 };
    let mut header = [0u32; 31];
    header[0] = 124;
    header[1] = CAPS | HEIGHT | WIDTH | PIXEL_FORMAT | size_flag | if texture.mips > 1 { MIPMAP_COUNT } else { 0 };
    header[2] = h;
    header[3] = w;
    header[4] = size;
    header[6] = texture.mips.max(1);
    header[18] = 32;
    header[19] = format.0;
    header[20] = u32::from_le_bytes(format.1);
    header[21] = format.2;
    header[22] = format.3;
    header[23] = format.4;
    header[24] = format.5;
    header[25] = format.6;
    header[26] = caps;
    header[27] = caps2;
    let mut out = Vec::with_capacity(128 + texture.pixels.len());
    out.extend_from_slice(b"DDS ");
    for word in header {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&texture.pixels);
    out
}

/// The top level of a flat texture as straight RGBA bytes, row by row from the top. None for a
/// cube map, or when the pixels are short of the top level.
pub fn rgba(texture: &Texture) -> Option<Vec<u8>> {
    if texture.faces != 1 {
        return None;
    }
    let (w, h) = (texture.width as usize, texture.height as usize);
    let px = &texture.pixels;
    let mut out = vec![0u8; w * h * 4];
    match texture.format {
        PixelFormat::Dxt1 | PixelFormat::Dxt3 | PixelFormat::Dxt5 => {
            let stride = if texture.format == PixelFormat::Dxt1 { 8 } else { 16 };
            let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
            if px.len() < bw * bh * stride {
                return None;
            }
            for by in 0..bh {
                for bx in 0..bw {
                    let block = &px[(by * bw + bx) * stride..][..stride];
                    let texels = decode_block(texture.format, block);
                    for (k, texel) in texels.iter().enumerate() {
                        let (x, y) = (bx * 4 + k % 4, by * 4 + k / 4);
                        if x < w && y < h {
                            out[(y * w + x) * 4..][..4].copy_from_slice(texel);
                        }
                    }
                }
            }
        }
        PixelFormat::Bgra8 | PixelFormat::Bgr8 | PixelFormat::L8 => {
            let size = match texture.format { PixelFormat::Bgra8 => 4, PixelFormat::Bgr8 => 3, _ => 1 };
            if px.len() < w * h * size {
                return None;
            }
            for (k, texel) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let s = &px[k * size..][..size];
                *texel = match size {
                    4 => [s[2], s[1], s[0], s[3]],
                    3 => [s[2], s[1], s[0], 255],
                    _ => [s[0], s[0], s[0], 255],
                };
            }
        }
    }
    Some(out)
}

/// One 4x4 block of DXT1, 3 or 5 as sixteen RGBA texels, row by row.
fn decode_block(format: PixelFormat, block: &[u8]) -> [[u8; 4]; 16] {
    let colour = if format == PixelFormat::Dxt1 { block } else { &block[8..] };
    let c0 = u16::from_le_bytes([colour[0], colour[1]]);
    let c1 = u16::from_le_bytes([colour[2], colour[3]]);
    let expand = |c: u16| {
        let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
        [(r << 3 | r >> 2) as u32, (g << 2 | g >> 4) as u32, (b << 3 | b >> 2) as u32]
    };
    let (a, b) = (expand(c0), expand(c1));
    let mix = |wa: u32, wb: u32, d: u32| [0, 1, 2].map(|i| ((a[i] * wa + b[i] * wb) / d) as u8);
    let opaque = |c: [u32; 3]| [c[0] as u8, c[1] as u8, c[2] as u8, 255];
    // DXT1 with c0 <= c1 has three colours and transparent black; DXT3 and 5 always have four.
    let four = format != PixelFormat::Dxt1 || c0 > c1;
    let palette: [[u8; 4]; 4] = if four {
        let (m, n) = (mix(2, 1, 3), mix(1, 2, 3));
        [opaque(a), opaque(b), [m[0], m[1], m[2], 255], [n[0], n[1], n[2], 255]]
    } else {
        let m = mix(1, 1, 2);
        [opaque(a), opaque(b), [m[0], m[1], m[2], 255], [0, 0, 0, 0]]
    };
    let bits = u32::from_le_bytes([colour[4], colour[5], colour[6], colour[7]]);
    let mut texels = [[0u8; 4]; 16];
    for (k, texel) in texels.iter_mut().enumerate() {
        *texel = palette[((bits >> (2 * k)) & 3) as usize];
    }
    match format {
        PixelFormat::Dxt3 => {
            for (k, texel) in texels.iter_mut().enumerate() {
                let nibble = (block[k / 2] >> (4 * (k % 2))) & 15;
                texel[3] = nibble << 4 | nibble;
            }
        }
        PixelFormat::Dxt5 => {
            let (a0, a1) = (block[0] as u32, block[1] as u32);
            let alphas: [u32; 8] = if a0 > a1 {
                [a0, a1, (6 * a0 + a1) / 7, (5 * a0 + 2 * a1) / 7, (4 * a0 + 3 * a1) / 7, (3 * a0 + 4 * a1) / 7, (2 * a0 + 5 * a1) / 7, (a0 + 6 * a1) / 7]
            } else {
                [a0, a1, (4 * a0 + a1) / 5, (3 * a0 + 2 * a1) / 5, (2 * a0 + 3 * a1) / 5, (a0 + 4 * a1) / 5, 0, 255]
            };
            let bits = block[2..8].iter().rev().fold(0u64, |acc, &b| acc << 8 | b as u64);
            for (k, texel) in texels.iter_mut().enumerate() {
                texel[3] = alphas[((bits >> (3 * k)) & 7) as usize] as u8;
            }
        }
        _ => {}
    }
    texels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture(format: PixelFormat, width: u32, height: u32, pixels: Vec<u8>) -> Texture {
        Texture { name: "t".into(), id: 1, width, height, faces: 1, mips: 1, format, pixels }
    }

    #[test]
    fn dxt1_block_decodes_its_two_end_colours_and_the_thirds() {
        // c0 pure red, c1 pure blue; texel indices 0, 1, 2, 3 across the first row.
        let block = vec![0x00, 0xf8, 0x1f, 0x00, 0b1110_0100, 0, 0, 0];
        let rgba = rgba(&texture(PixelFormat::Dxt1, 4, 4, block)).unwrap();
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[0, 0, 255, 255]);
        assert_eq!(&rgba[8..12], &[170, 0, 85, 255]);
        assert_eq!(&rgba[12..16], &[85, 0, 170, 255]);
    }

    #[test]
    fn bgr8_is_swizzled_to_rgba() {
        let rgba = rgba(&texture(PixelFormat::Bgr8, 1, 1, vec![1, 2, 3])).unwrap();
        assert_eq!(rgba, vec![3, 2, 1, 255]);
    }

    #[test]
    fn dds_header_names_the_block_format_and_keeps_the_pixels() {
        let file = dds(&texture(PixelFormat::Dxt5, 4, 4, vec![7; 16]));
        assert_eq!(&file[0..4], b"DDS ");
        assert_eq!(&file[84..88], b"DXT5");
        assert_eq!(file.len(), 128 + 16);
    }
}
