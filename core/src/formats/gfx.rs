//! Scaleform `.gfx` movies (the game's HUD, `UI\Flash\hud.gfx`): a Flash 8 file with the
//! signature `GFX`, read far enough to draw its vector shapes as the timelines place them.
//!
//! Read: shapes (solid, linear and radial fills), sprites with their frames (placements,
//! removals, labels, whether frame 1 stops), masks (clip depths), colour transforms, the
//! exported names. Bitmap fills are drawn from the images a caller attaches (`Movie::bitmaps`,
//! from the paired `.apk`). Frame scripts are read only for their timeline controls (stop,
//! play, goto) and the strings they name (`Sprite::scripts`, played by `crate::flash_timeline`).
//! Not read: strokes, morph shapes, filters, and the scripts' logic: whoever draws a movie
//! plays the part of its classes through `Pose` (`work\gfx_dump.py` lists and disassembles
//! what is in a file).

use std::collections::HashMap;

use super::Bytes;

/// x' = a x + c y + tx, y' = b x + d y + ty, in pixels (a twip is 1 / 20).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

    pub fn scale_then_move(scale: f32, tx: f32, ty: f32) -> Self {
        Affine { a: scale, b: 0.0, c: 0.0, d: scale, tx, ty }
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.tx, self.b * x + self.d * y + self.ty)
    }

    /// `self` after `inner`.
    pub fn then(&self, inner: &Affine) -> Affine {
        let (tx, ty) = self.apply(inner.tx, inner.ty);
        Affine {
            a: self.a * inner.a + self.c * inner.b,
            b: self.b * inner.a + self.d * inner.b,
            c: self.a * inner.c + self.c * inner.d,
            d: self.b * inner.c + self.d * inner.d,
            tx,
            ty,
        }
    }

    pub fn inverse(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-12 {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Affine { a, b, c, d, tx: -(a * self.tx + c * self.ty), ty: -(b * self.tx + d * self.ty) })
    }
}

/// The axis-aligned box round a box under a transform.
pub fn transform_box(m: &Affine, b: [f32; 4]) -> [f32; 4] {
    let corners = [m.apply(b[0], b[1]), m.apply(b[2], b[1]), m.apply(b[0], b[3]), m.apply(b[2], b[3])];
    let xs = corners.map(|c| c.0);
    let ys = corners.map(|c| c.1);
    [xs.iter().copied().fold(f32::MAX, f32::min), ys.iter().copied().fold(f32::MAX, f32::min), xs.iter().copied().fold(f32::MIN, f32::max), ys.iter().copied().fold(f32::MIN, f32::max)]
}

/// A colour transform: each of red, green, blue, alpha (0 to 1) times `mul` plus `add`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Tint {
    pub mul: [f32; 4],
    pub add: [f32; 4],
}

impl Tint {
    pub const NONE: Tint = Tint { mul: [1.0; 4], add: [0.0; 4] };

    pub fn apply(&self, colour: [f32; 4]) -> [f32; 4] {
        std::array::from_fn(|i| (colour[i] * self.mul[i] + self.add[i]).clamp(0.0, 1.0))
    }

    /// `self` after `inner`.
    pub fn then(&self, inner: &Tint) -> Tint {
        Tint { mul: std::array::from_fn(|i| self.mul[i] * inner.mul[i]), add: std::array::from_fn(|i| inner.add[i] * self.mul[i] + self.add[i]) }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Fill {
    Solid([f32; 4]),
    /// The gradient's square (819.2 pixels either way) into the shape's space, and the
    /// stops: where (0 to 1) and the colour.
    Linear(Affine, Vec<(f32, [f32; 4])>),
    Radial(Affine, Vec<(f32, [f32; 4])>),
    /// An external bitmap fill, referenced by its image character id in the paired APK.
    /// The general movie renderer does not sample these yet; icon consumers can resolve
    /// the reference and use the original texture directly.
    Bitmap { id: u16, matrix: Affine, repeat: bool, smooth: bool },
    /// A non-bitmap fill kind this reader does not render.
    Other,
}

/// One edge: a line, or a curve by its control point. `fill0` is the fill on its left,
/// `fill1` the one on its right, as places in the shape's `fills` + 1; 0 is none.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Edge {
    pub from: (f32, f32),
    pub control: Option<(f32, f32)>,
    pub to: (f32, f32),
    pub fill0: u32,
    pub fill1: u32,
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Shape {
    /// Left, top, right, bottom.
    pub bounds: [f32; 4],
    pub fills: Vec<Fill>,
    pub edges: Vec<Edge>,
}

/// A `PlaceObject`: what goes at a depth, or what changes about what is there.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Place {
    pub depth: u16,
    /// It changes the object at the depth; else it puts a new one there.
    pub moves: bool,
    pub id: Option<u16>,
    pub matrix: Option<Affine>,
    pub tint: Option<Tint>,
    pub name: Option<String>,
    /// It is a mask for the depths above it up to this one.
    pub clip: Option<u16>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Step {
    Place(Place),
    Remove(u16),
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Sprite {
    pub frames: Vec<Vec<Step>>,
    /// A label and its frame, counted from 1.
    pub labels: Vec<(String, usize)>,
    /// Frame 1's script has a `stop`: the timeline waits for a script to move it.
    pub stops: bool,
    /// The frame scripts (`DoAction`), by the frame they run on (from 1).
    pub scripts: Vec<(usize, Script)>,
}

/// What a frame script does to its own timeline.
#[derive(Clone, PartialEq, Debug)]
pub enum Control {
    Stop,
    Play,
    /// To a frame, counted from 1.
    GotoFrame(usize),
    GotoLabel(String),
}

/// A frame script, as far as a player needs it: its timeline controls in order, and every string it names.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Script {
    pub controls: Vec<Control>,
    pub strings: Vec<String>,
    /// The action bytes (`formats::avm1` reads the functions it defines).
    pub raw: Vec<u8>,
}

/// A movie's external image (tag 1001, its pixels in the paired `.apk`), as a script-less player draws it in a
/// bitmap fill: red, green, blue, alpha bytes, not multiplied by the alpha, rows from the top.
#[derive(Clone, Debug)]
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<[u8; 4]>,
}

impl Bitmap {
    /// The colour at texel position (`u`, `v`) (texel centres at .5), not multiplied by its alpha. `smooth`
    /// interpolates the four nearest texels; `repeat` tiles the image, else its edge texels stretch.
    pub fn sample(&self, u: f32, v: f32, smooth: bool, repeat: bool) -> [f32; 4] {
        if self.width == 0 || self.height == 0 {
            return [0.0; 4];
        }
        let texel = |x: i64, y: i64| -> [f32; 4] {
            let (w, h) = (self.width as i64, self.height as i64);
            let (x, y) = if repeat { (x.rem_euclid(w), y.rem_euclid(h)) } else { (x.clamp(0, w - 1), y.clamp(0, h - 1)) };
            let p = self.rgba[(y * w + x) as usize];
            let a = f32::from(p[3]) / 255.0;
            [f32::from(p[0]) / 255.0 * a, f32::from(p[1]) / 255.0 * a, f32::from(p[2]) / 255.0 * a, a]
        };
        let premultiplied = if smooth {
            let (x, y) = (u - 0.5, v - 0.5);
            let (x0, y0) = (x.floor(), y.floor());
            let (fx, fy) = (x - x0, y - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let (a, b, c, d) = (texel(x0, y0), texel(x0 + 1, y0), texel(x0, y0 + 1), texel(x0 + 1, y0 + 1));
            std::array::from_fn(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
        } else {
            texel(u.floor() as i64, v.floor() as i64)
        };
        let a = premultiplied[3];
        if a <= 0.0 {
            return [0.0; 4];
        }
        [premultiplied[0] / a, premultiplied[1] / a, premultiplied[2] / a, a]
    }
}

impl Sprite {
    /// A label's frame. [assumed] matched without case after an exact match fails, as the Flash player matches AS2's
    /// `gotoAndPlay` labels (OptGraph plays "On" on items labelled "on"). OPEN: settles: the Scaleform label lookup
    /// (not located in the executable), or a check that the game reaches OptGraph's "On" frame by a case-only mismatch.
    pub fn label(&self, name: &str) -> Option<usize> {
        self.labels
            .iter()
            .find(|(label, _)| label == name)
            .or_else(|| self.labels.iter().find(|(label, _)| label.eq_ignore_ascii_case(name)))
            .map(|(_, frame)| *frame)
    }

    /// What is shown once frames 1 to `frame` have run, by depth.
    pub fn shown(&self, frame: usize) -> Vec<Place> {
        let mut shown: Vec<Place> = Vec::new();
        for steps in self.frames.iter().take(frame.max(1)) {
            for step in steps {
                match step {
                    Step::Remove(depth) => shown.retain(|p| p.depth != *depth),
                    Step::Place(place) => {
                        let old = shown.iter().position(|p| p.depth == place.depth);
                        match (old, place.moves) {
                            (Some(at), true) => {
                                let item = &mut shown[at];
                                item.id = place.id.or(item.id);
                                item.matrix = place.matrix.or(item.matrix);
                                item.tint = place.tint.or(item.tint);
                                item.name = place.name.clone().or(item.name.take());
                                item.clip = place.clip.or(item.clip);
                            }
                            (Some(at), false) => shown[at] = place.clone(),
                            (None, _) => shown.push(place.clone()),
                        }
                    }
                }
            }
        }
        shown.sort_by_key(|p| p.depth);
        shown
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Character {
    Shape(Shape),
    Sprite(Sprite),
    Font(Font),
    Text(EditText),
    Button(Button),
    Other,
}

/// A `DefineButton2` (or `DefineButton`) as far as the pointer needs it: the records of its hit-test state
/// (what the pointer is tested against, drawn never). Its other states and its own condition actions are
/// not read (the front end's one button has none: its handlers are set by scripts).
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Button {
    pub hit: Vec<Place>,
}

/// A `DefineFont3` (or `DefineFont2`): glyph outlines in an em square of 1024 (pixels as
/// this reader counts them: the tag's units are 1/20 of that), with the character each
/// stands for and, when the font has a layout, its advance. The game's two HUD fonts live
/// in `UI\Flash\font.gfx` and the HUD imports them by name.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Font {
    pub name: String,
    pub glyphs: Vec<Shape>,
    /// The character code of each glyph.
    pub codes: Vec<u16>,
    /// Each glyph's advance, in the em square; empty when the font has no layout.
    pub advances: Vec<f32>,
    pub ascent: f32,
    pub descent: f32,
    pub leading: f32,
}

impl Font {
    /// The em square of a `DefineFont3` as this reader counts it (20480 units / 20).
    pub const EM: f32 = 1024.0;

    pub fn glyph(&self, c: char) -> Option<usize> {
        let code = u16::try_from(u32::from(c)).ok()?;
        self.codes.iter().position(|&k| k == code)
    }
}

/// A `DefineEditText`: a text field. `text` is the initial text with its HTML taken out
/// (paragraphs become new lines); the movie's scripts or the game set it later (`Tweak::text`).
#[derive(Clone, Default, PartialEq, Debug)]
pub struct EditText {
    /// Left, top, right, bottom, in pixels.
    pub bounds: [f32; 4],
    pub font: Option<u16>,
    /// The font size: the em square's height in pixels.
    pub height: f32,
    pub colour: [f32; 4],
    /// 0 left, 1 right, 2 centre, 3 justify (drawn as left).
    pub align: u8,
    pub left_margin: f32,
    pub right_margin: f32,
    /// Pixels between lines (the layout's `Leading`, signed).
    pub leading: f32,
    pub multiline: bool,
    pub word_wrap: bool,
    /// The field's variable name, if it has one.
    pub variable: String,
    pub text: String,
}

/// A character the movie takes from another (`ImportAssets` / `ImportAssets2`): the file it
/// names, the id it is known by here and the name it is exported under there.
#[derive(Clone, PartialEq, Debug)]
pub struct Import {
    pub url: String,
    pub id: u16,
    pub name: String,
}

pub struct Movie {
    /// The stage in pixels and the frames a second.
    pub stage: [f32; 2],
    pub rate: f32,
    pub characters: HashMap<u16, Character>,
    pub exports: HashMap<String, u16>,
    pub imports: Vec<Import>,
    /// The external images bitmap fills sample, by character id: empty until the caller attaches them from the
    /// movie's `.apk` (a fill whose image is missing is not drawn).
    pub bitmaps: HashMap<u16, Bitmap>,
    /// Pictures set inline in text (`add_text_image`): character `TEXT_IMAGE_FIRST + n` in a field's text is
    /// `text_images[n]`.
    pub text_images: Vec<TextImage>,
}

/// The id the root timeline (its frames, labels and scripts) is kept under among the characters, as a sprite.
pub const ROOT: u16 = u16::MAX;

/// A picture inline in a text field's line, as an html `<img>` shows one: drawn `width` x `height` pixels at the
/// pen, its top `top` pixels from the baseline (negative is above), and the pen moves on by `width`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextImage {
    /// Its pixels, in `Movie::bitmaps`.
    pub bitmap: u16,
    pub width: f32,
    pub height: f32,
    pub top: f32,
}

/// The first character that stands for an inline picture (Unicode's private use area).
pub const TEXT_IMAGE_FIRST: u32 = 0xE000;
/// The first bitmap id inline pictures take (movies here define ids far below it).
const TEXT_IMAGE_BITMAPS: u16 = 0xF000;

struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    bit: u32,
    short: bool,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8], at: usize) -> Self {
        Self { data, at, bit: 0, short: false }
    }

    fn unsigned(&mut self, count: u32) -> u32 {
        let mut value = 0;
        for _ in 0..count {
            let Some(byte) = self.data.get(self.at) else {
                self.short = true;
                return 0;
            };
            value = (value << 1) | u32::from((byte >> (7 - self.bit)) & 1);
            self.bit += 1;
            if self.bit == 8 {
                (self.bit, self.at) = (0, self.at + 1);
            }
        }
        value
    }

    fn signed(&mut self, count: u32) -> i32 {
        let value = self.unsigned(count);
        if count > 0 && count < 32 && value >> (count - 1) != 0 { value as i32 - (1 << count) } else { value as i32 }
    }

    fn align(&mut self) -> usize {
        if self.bit != 0 {
            (self.bit, self.at) = (0, self.at + 1);
        }
        self.at
    }

    fn byte(&mut self) -> u8 {
        self.align();
        let value = self.data.get(self.at).copied();
        self.short |= value.is_none();
        self.at += 1;
        value.unwrap_or(0)
    }

    fn word(&mut self) -> u16 {
        u16::from(self.byte()) | (u16::from(self.byte()) << 8)
    }

    fn rect(&mut self) -> [f32; 4] {
        self.align();
        let count = self.unsigned(5);
        let v: [f32; 4] = std::array::from_fn(|_| self.signed(count) as f32 / 20.0);
        self.align();
        [v[0], v[2], v[1], v[3]]
    }

    fn matrix(&mut self) -> Affine {
        self.align();
        let mut m = Affine::IDENTITY;
        if self.unsigned(1) != 0 {
            let count = self.unsigned(5);
            m.a = self.signed(count) as f32 / 65536.0;
            m.d = self.signed(count) as f32 / 65536.0;
        }
        if self.unsigned(1) != 0 {
            let count = self.unsigned(5);
            m.b = self.signed(count) as f32 / 65536.0;
            m.c = self.signed(count) as f32 / 65536.0;
        }
        let count = self.unsigned(5);
        m.tx = self.signed(count) as f32 / 20.0;
        m.ty = self.signed(count) as f32 / 20.0;
        self.align();
        m
    }

    fn tint(&mut self) -> Tint {
        self.align();
        let (has_add, has_mul) = (self.unsigned(1) != 0, self.unsigned(1) != 0);
        let count = self.unsigned(4);
        let mut tint = Tint::NONE;
        if has_mul {
            tint.mul = std::array::from_fn(|_| self.signed(count) as f32 / 256.0);
        }
        if has_add {
            tint.add = std::array::from_fn(|_| self.signed(count) as f32 / 255.0);
        }
        self.align();
        tint
    }

    fn colour(&mut self, alpha: bool) -> [f32; 4] {
        let rgb: [f32; 3] = std::array::from_fn(|_| f32::from(self.byte()) / 255.0);
        [rgb[0], rgb[1], rgb[2], if alpha { f32::from(self.byte()) / 255.0 } else { 1.0 }]
    }

    fn string(&mut self) -> String {
        let mut text = Vec::new();
        loop {
            match self.byte() {
                0 => break,
                _ if self.short => break,
                c => text.push(c),
            }
        }
        String::from_utf8_lossy(&text).into_owned()
    }
}

/// The tags from `at` to `end`: the code, where its body starts and the body's length.
fn tags(data: &[u8], mut at: usize, end: usize) -> Vec<(u16, usize, usize)> {
    let mut out = Vec::new();
    while at + 2 <= end {
        let Ok(head) = data.u16_at(at) else { break };
        at += 2;
        let (code, mut size) = (head >> 6, usize::from(head & 63));
        if size == 63 {
            let Ok(long) = data.u32_at(at) else { break };
            (size, at) = (long as usize, at + 4);
        }
        if at + size > end {
            break;
        }
        out.push((code, at, size));
        at += size;
        if code == 0 {
            break;
        }
    }
    out
}

fn fill_style(bits: &mut Bits, code: u16) -> Fill {
    let alpha = code >= 32;
    match bits.byte() {
        0 => Fill::Solid(bits.colour(alpha)),
        kind @ (0x10 | 0x12 | 0x13) => {
            let matrix = bits.matrix();
            let count = bits.byte() & 15;
            let stops = (0..count).map(|_| (f32::from(bits.byte()) / 255.0, bits.colour(alpha))).collect();
            if kind == 0x13 {
                bits.word();
            }
            if kind == 0x10 { Fill::Linear(matrix, stops) } else { Fill::Radial(matrix, stops) }
        }
        kind @ (0x40..=0x43) => {
            let id = bits.word();
            let matrix = bits.matrix();
            Fill::Bitmap { id, matrix, repeat: kind == 0x40 || kind == 0x42, smooth: kind == 0x40 || kind == 0x41 }
        }
        _ => {
            bits.word();
            bits.matrix();
            Fill::Other
        }
    }
}

fn styles(bits: &mut Bits, code: u16, fills: &mut Vec<Fill>) {
    let mut count = usize::from(bits.byte());
    if count == 255 {
        count = usize::from(bits.word());
    }
    for _ in 0..count {
        let fill = fill_style(bits, code);
        fills.push(fill);
    }
    let mut count = usize::from(bits.byte());
    if count == 255 {
        count = usize::from(bits.word());
    }
    for _ in 0..count {
        bits.word();
        if code == 83 {
            // A `LINESTYLE2`: the join, whether it has a fill of its own, the caps.
            let flags = bits.byte();
            bits.byte();
            if (flags >> 4) & 3 == 2 {
                bits.word();
            }
            if flags & 8 != 0 {
                fill_style(bits, code);
            } else {
                bits.colour(true);
            }
        } else {
            bits.colour(code >= 32);
        }
    }
}

fn shape(data: &[u8], at: usize, code: u16) -> Result<Shape, String> {
    let mut bits = Bits::new(data, at + 2);
    let mut shape = Shape { bounds: bits.rect(), ..Default::default() };
    if code == 83 {
        bits.rect();
        bits.byte();
    }
    styles(&mut bits, code, &mut shape.fills);
    bits.align();
    records(&mut bits, code, &mut shape)?;
    Ok(shape)
}

/// A glyph of a font: a shape with no styles of its own, filled with fill 1.
fn glyph(data: &[u8], at: usize) -> Result<Shape, String> {
    let mut bits = Bits::new(data, at);
    let mut shape = Shape { fills: vec![Fill::Solid([1.0; 4])], ..Default::default() };
    records(&mut bits, 22, &mut shape)?;
    Ok(shape)
}

/// A shape's records: the fill and line bit counts, then the edges, up to the end record.
fn records(bits: &mut Bits, code: u16, shape: &mut Shape) -> Result<(), String> {
    let (mut fill_bits, mut line_bits) = (bits.unsigned(4), bits.unsigned(4));
    // A record may bring new styles: its fills are counted from where the old ones end.
    let mut base = 0;
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let (mut fill0, mut fill1) = (0, 0);
    loop {
        if bits.short {
            return Err("a shape runs past its tag".into());
        }
        if bits.unsigned(1) == 0 {
            let flags = bits.unsigned(5);
            if flags == 0 {
                break;
            }
            if flags & 1 != 0 {
                let count = bits.unsigned(5);
                x = bits.signed(count) as f32 / 20.0;
                y = bits.signed(count) as f32 / 20.0;
            }
            if flags & 2 != 0 {
                let style = bits.unsigned(fill_bits);
                fill0 = if style == 0 { 0 } else { base + style };
            }
            if flags & 4 != 0 {
                let style = bits.unsigned(fill_bits);
                fill1 = if style == 0 { 0 } else { base + style };
            }
            if flags & 8 != 0 {
                bits.unsigned(line_bits);
            }
            if flags & 16 != 0 {
                base = shape.fills.len() as u32;
                bits.align();
                styles(bits, code, &mut shape.fills);
                bits.align();
                (fill_bits, line_bits) = (bits.unsigned(4), bits.unsigned(4));
                (fill0, fill1) = (0, 0);
            }
        } else if bits.unsigned(1) != 0 {
            let count = bits.unsigned(4) + 2;
            let from = (x, y);
            if bits.unsigned(1) != 0 {
                x += bits.signed(count) as f32 / 20.0;
                y += bits.signed(count) as f32 / 20.0;
            } else if bits.unsigned(1) != 0 {
                y += bits.signed(count) as f32 / 20.0;
            } else {
                x += bits.signed(count) as f32 / 20.0;
            }
            shape.edges.push(Edge { from, control: None, to: (x, y), fill0, fill1 });
        } else {
            let count = bits.unsigned(4) + 2;
            let from = (x, y);
            let control = (x + bits.signed(count) as f32 / 20.0, y + bits.signed(count) as f32 / 20.0);
            x = control.0 + bits.signed(count) as f32 / 20.0;
            y = control.1 + bits.signed(count) as f32 / 20.0;
            shape.edges.push(Edge { from, control: Some(control), to: (x, y), fill0, fill1 });
        }
    }
    Ok(())
}

/// A `DefineFont2` (48) or `DefineFont3` (75). A `DefineFont2`'s units are a twentieth of
/// a `DefineFont3`'s: they are scaled to the same em square.
fn font(data: &[u8], at: usize, size: usize, code: u16) -> Result<Font, String> {
    let end = at + size;
    let mut bits = Bits::new(data, at + 2);
    let flags = bits.byte();
    bits.byte();
    let name_len = usize::from(bits.byte());
    let name_at = bits.at;
    let name = String::from_utf8_lossy(data.get(name_at..name_at + name_len).ok_or("a font name runs past its tag")?);
    let name = name.trim_end_matches('\0').to_owned();
    bits.at = name_at + name_len;
    let count = usize::from(bits.word());
    let (wide_offsets, wide_codes, layout) = (flags & 8 != 0, flags & 4 != 0, flags & 0x80 != 0);
    let table = bits.at;
    let offset = |i: usize| -> Result<usize, String> {
        Ok(if wide_offsets { data.u32_at(table + 4 * i)? as usize } else { usize::from(data.u16_at(table + 2 * i)?) })
    };
    let scale = if code == 48 { 20.0 } else { 1.0 };
    let mut font = Font { name, ..Default::default() };
    // [data] Scaleform strips the outlines from the fonts of `font.gfx`: their offset table
    // does not start past itself. Such a font is a name only; its glyphs come from the
    // same-named font of `font_glyphs.gfx` (`Movie::glyphs_from`).
    if count > 0 && offset(0)? < (count + 1) * if wide_offsets { 4 } else { 2 } {
        return Ok(font);
    }
    for i in 0..count {
        let mut shape = glyph(data, table + offset(i)?)?;
        if scale != 1.0 {
            for e in &mut shape.edges {
                e.from = (e.from.0 * scale, e.from.1 * scale);
                e.to = (e.to.0 * scale, e.to.1 * scale);
                e.control = e.control.map(|c| (c.0 * scale, c.1 * scale));
            }
        }
        font.glyphs.push(shape);
    }
    let codes = table + offset(count)?;
    let mut bits = Bits::new(data, codes);
    for _ in 0..count {
        font.codes.push(if wide_codes { bits.word() } else { u16::from(bits.byte()) });
    }
    if layout && bits.at < end {
        let units = scale / 20.0;
        font.ascent = f32::from(bits.word()) * units;
        font.descent = f32::from(bits.word()) * units;
        font.leading = f32::from(bits.word() as i16) * units;
        for _ in 0..count {
            font.advances.push(f32::from(bits.word() as i16) * units);
        }
    }
    if bits.short {
        return Err(format!("the font {} runs past its tag", font.name));
    }
    Ok(font)
}

/// The text of an HTML text field without its tags: `</p>` and `<br>` make new lines, and
/// the five entities are decoded.
pub fn plain_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            rest = "";
            break;
        };
        let tag = rest[open + 1..open + close].trim().to_ascii_lowercase();
        if tag == "/p" || tag.starts_with("br") {
            out.push('\n');
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    let out = out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&");
    out.trim_end_matches('\n').to_owned()
}

/// One line broken to fit `width` with `advance` per character: at the last space that fits
/// (the space is dropped), else, for a word longer than the line, at the last character that
/// fits. [assumed] Flash's own rule (trailing spaces, hyphens) is not read; this is the plain one. OPEN: settles:
/// the Scaleform text-wrap routine (not read); only the fields with multiline and wordWrap use it.
pub fn wrap(line: &str, width: f32, advance: &dyn Fn(char) -> f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut used = 0.0;
    for word in line.split(' ') {
        let w: f32 = word.chars().map(advance).sum();
        let gap = if current.is_empty() { 0.0 } else { advance(' ') };
        if !current.is_empty() && used + gap + w > width {
            out.push(std::mem::take(&mut current));
            used = 0.0;
        } else if !current.is_empty() {
            current.push(' ');
            used += gap;
        }
        for c in word.chars() {
            let a = advance(c);
            // Only a word longer than the line starts a line of its own (above), so this breaks inside that word.
            if w > width && !current.is_empty() && used + a > width {
                out.push(std::mem::take(&mut current));
                used = 0.0;
            }
            current.push(c);
            used += a;
        }
    }
    out.push(current);
    out
}

fn edit_text(data: &[u8], at: usize) -> EditText {
    let mut bits = Bits::new(data, at + 2);
    let mut text = EditText { bounds: bits.rect(), colour: [0.0, 0.0, 0.0, 1.0], ..Default::default() };
    let (first, second) = (bits.byte(), bits.byte());
    text.word_wrap = first & 0x40 != 0;
    text.multiline = first & 0x20 != 0;
    let html = second & 0x02 != 0;
    if first & 0x01 != 0 {
        text.font = Some(bits.word());
        text.height = f32::from(bits.word()) / 20.0;
    }
    if second & 0x80 != 0 {
        bits.string();
        text.height = f32::from(bits.word()) / 20.0;
    }
    if first & 0x04 != 0 {
        text.colour = bits.colour(true);
    }
    if first & 0x02 != 0 {
        bits.word();
    }
    if second & 0x20 != 0 {
        text.align = bits.byte();
        text.left_margin = f32::from(bits.word()) / 20.0;
        text.right_margin = f32::from(bits.word()) / 20.0;
        bits.word();
        text.leading = f32::from(bits.word() as i16) / 20.0;
    }
    text.variable = bits.string();
    if first & 0x80 != 0 {
        let initial = bits.string();
        text.text = if html { plain_text(&initial) } else { initial };
    }
    text
}

fn imports(data: &[u8], at: usize, code: u16) -> Vec<Import> {
    let mut bits = Bits::new(data, at);
    let url = bits.string();
    if code == 71 {
        bits.byte();
        bits.byte();
    }
    let count = bits.word();
    (0..count)
        .map(|_| {
            let id = bits.word();
            Import { url: url.clone(), id, name: bits.string() }
        })
        .collect()
}

fn place(data: &[u8], at: usize, code: u16) -> Place {
    let mut bits = Bits::new(data, at);
    let flags = bits.byte();
    let more = if code == 70 { bits.byte() } else { 0 };
    let mut place = Place { depth: bits.word(), moves: flags & 1 != 0, ..Default::default() };
    if more & 8 != 0 {
        bits.string();
    }
    if flags & 2 != 0 {
        place.id = Some(bits.word());
    }
    if flags & 4 != 0 {
        place.matrix = Some(bits.matrix());
    }
    if flags & 8 != 0 {
        place.tint = Some(bits.tint());
    }
    if flags & 16 != 0 {
        bits.word();
    }
    if flags & 32 != 0 {
        place.name = Some(bits.string());
    }
    if flags & 64 != 0 {
        place.clip = Some(bits.word());
    }
    place
}

/// Whether a frame script's own actions (not its functions') have a `stop`.
fn has_stop(data: &[u8]) -> bool {
    script(data).controls.contains(&Control::Stop)
}

/// What a frame script does to its own timeline, in order, and the strings it names (its constant pool and pushed
/// strings: the class methods it calls and the sounds it plays, for whoever plays the part of the classes). A
/// function's code is stepped over.
fn script(data: &[u8]) -> Script {
    let mut out = Script { raw: data.to_vec(), ..Script::default() };
    let string_at = |at: usize| {
        let end = data[at..].iter().position(|&b| b == 0).map_or(data.len(), |n| at + n);
        (String::from_utf8_lossy(&data[at..end]).into_owned(), end + 1)
    };
    let mut at = 0;
    while let Some(&op) = data.get(at) {
        at += 1;
        if op == 0 {
            break;
        }
        match op {
            0x06 => out.controls.push(Control::Play),
            0x07 => out.controls.push(Control::Stop),
            _ => {}
        }
        if op >= 0x80 {
            let Ok(size) = data.u16_at(at) else { break };
            let body = at + 2;
            at = body + usize::from(size);
            let end = at.min(data.len());
            match op {
                0x81 => out.controls.push(Control::GotoFrame(usize::from(data.u16_at(body).unwrap_or(0)) + 1)),
                0x8c => out.controls.push(Control::GotoLabel(string_at(body).0)),
                0x88 => {
                    let mut q = body + 2;
                    for _ in 0..data.u16_at(body).unwrap_or(0) {
                        if q >= end {
                            break;
                        }
                        let (s, next) = string_at(q);
                        out.strings.push(s);
                        q = next;
                    }
                }
                0x96 => {
                    // Push: typed values; only the strings are kept.
                    let mut q = body;
                    while q < end {
                        let kind = data[q];
                        q += 1;
                        q += match kind {
                            0 => {
                                let (s, next) = string_at(q);
                                out.strings.push(s);
                                next - q
                            }
                            1 | 7 => 4,
                            4 | 5 | 8 => 1,
                            6 => 8,
                            9 => 2,
                            _ => 0,
                        };
                    }
                }
                _ => {}
            }
            // A function's code follows its header: step over it.
            if (op == 0x9b || op == 0x8e)
                && let Ok(code) = data.u16_at(at.saturating_sub(2)) {
                at += usize::from(code);
            }
        }
    }
    out
}

fn sprite(data: &[u8], at: usize, size: usize) -> Sprite {
    let mut sprite = Sprite::default();
    let mut frame = Vec::new();
    for (code, body, length) in tags(data, at + 4, at + size) {
        match code {
            26 | 70 => frame.push(Step::Place(place(data, body, code))),
            28 => frame.push(Step::Remove(data.u16_at(body).unwrap_or(0))),
            43 => sprite.labels.push((Bits::new(data, body).string(), sprite.frames.len() + 1)),
            12 => {
                if sprite.frames.is_empty() {
                    sprite.stops |= has_stop(&data[body..body + length]);
                }
                sprite.scripts.push((sprite.frames.len() + 1, script(&data[body..body + length])));
            }
            1 => sprite.frames.push(std::mem::take(&mut frame)),
            _ => {}
        }
    }
    sprite
}

/// A button's records (`DefineButton2` 34: flags, actions offset, then records with a colour transform;
/// `DefineButton` 7: records without one), keeping those of the hit-test state (flag 0x8). A record with a
/// filter list (flag 0x10) ends the reading. [data] No record of the install's `UI\**\*.gfx` movies has a
/// filter list: a scan of every DefineButton2 record (2026-10-20, batch W3; the HUD movies `hud.gfx` and
/// `hud4x3.gfx` define no button at all) found none with flag 0x10, so the break is never taken there. A
/// filter list would end the reading early and lose the records after it: the parser does not read one.
fn button(data: &[u8], at: usize, code: u16) -> Button {
    let mut bits = Bits::new(data, at + if code == 34 { 5 } else { 2 });
    let mut out = Button::default();
    loop {
        let flags = bits.byte();
        if flags == 0 || bits.short {
            break;
        }
        let (id, depth) = (bits.word(), bits.word());
        let matrix = bits.matrix();
        let tint = if code == 34 { Some(bits.tint()) } else { None };
        if code == 34 && flags & 0x10 != 0 {
            break;
        }
        if code == 34 && flags & 0x20 != 0 {
            bits.byte();
        }
        if flags & 0x8 != 0 {
            out.hit.push(Place { depth, id: Some(id), matrix: Some(matrix), tint, ..Place::default() });
        }
    }
    out
}

impl Movie {
    pub fn parse(file: &[u8]) -> Result<Movie, String> {
        if file.get(..3) != Some(b"GFX") && file.get(..3) != Some(b"FWS") {
            return Err("not an uncompressed .gfx or .swf movie".into());
        }
        let data = file.get(8..).ok_or("a movie with no body")?;
        let mut bits = Bits::new(data, 0);
        let stage = bits.rect();
        let rate = f32::from(data.u16_at(bits.at)?) / 256.0;
        let mut movie = Movie {
            stage: [stage[2] - stage[0], stage[3] - stage[1]],
            rate,
            characters: HashMap::new(),
            exports: HashMap::new(),
            imports: Vec::new(),
            bitmaps: HashMap::new(),
            text_images: Vec::new(),
        };
        let mut root_frame = Vec::new();
        let mut root = Sprite::default();
        for (code, at, size) in tags(data, bits.at + 4, data.len()) {
            match code {
                // The root timeline's own tags, kept as a sprite (`Movie::root`).
                26 | 70 => root_frame.push(Step::Place(place(data, at, code))),
                28 => root_frame.push(Step::Remove(data.u16_at(at).unwrap_or(0))),
                43 => root.labels.push((Bits::new(data, at).string(), root.frames.len() + 1)),
                12 => {
                    if root.frames.is_empty() {
                        root.stops |= has_stop(&data[at..at + size]);
                    }
                    root.scripts.push((root.frames.len() + 1, script(&data[at..at + size])));
                }
                1 => root.frames.push(std::mem::take(&mut root_frame)),
                2 | 22 | 32 | 83 => {
                    movie.characters.insert(data.u16_at(at)?, Character::Shape(shape(data, at, code)?));
                }
                39 => {
                    movie.characters.insert(data.u16_at(at)?, Character::Sprite(sprite(data, at, size)));
                }
                48 | 75 => {
                    movie.characters.insert(data.u16_at(at)?, Character::Font(font(data, at, size, code)?));
                }
                37 => {
                    movie.characters.insert(data.u16_at(at)?, Character::Text(edit_text(data, at)));
                }
                57 | 71 => movie.imports.extend(imports(data, at, code)),
                7 | 34 => {
                    movie.characters.insert(data.u16_at(at)?, Character::Button(button(data, at, code)));
                }
                46 | 11 | 33 | 1001 => {
                    movie.characters.insert(data.u16_at(at)?, Character::Other);
                }
                56 => {
                    let mut bits = Bits::new(data, at);
                    for _ in 0..bits.word() {
                        let id = bits.word();
                        movie.exports.insert(bits.string(), id);
                    }
                }
                _ => {}
            }
        }
        movie.characters.insert(ROOT, Character::Sprite(root));
        Ok(movie)
    }

    pub fn sprite(&self, id: u16) -> Option<&Sprite> {
        match self.characters.get(&id)? {
            Character::Sprite(sprite) => Some(sprite),
            _ => None,
        }
    }

    /// Takes the characters this movie imports from `library` (whose file is `file`, e.g.
    /// `font.gfx`: the movie names it `font.swf`) by their exported names. Returns how many
    /// it took. The HUD's fonts come this way.
    pub fn import_from(&mut self, file: &str, library: &Movie) -> usize {
        let stem = |name: &str| name.rsplit(['/', '\\']).next().unwrap_or(name).split('.').next().unwrap_or(name).to_ascii_lowercase();
        let wanted = stem(file);
        let mut taken = 0;
        for import in self.imports.iter().filter(|i| stem(&i.url) == wanted) {
            if let Some(found) = library.exports.get(&import.name).and_then(|id| library.characters.get(id)) {
                self.characters.insert(import.id, found.clone());
                taken += 1;
            }
        }
        taken
    }

    /// Gives each font of this movie that has no outlines those of the font of the same
    /// name in `library` (`font_glyphs.gfx`), as Scaleform finds a font's glyphs by its
    /// name. Returns how many it filled.
    pub fn glyphs_from(&mut self, library: &Movie) -> usize {
        let mut filled = 0;
        for character in self.characters.values_mut() {
            let Character::Font(font) = character else { continue };
            if !font.glyphs.is_empty() {
                continue;
            }
            let source = library.characters.values().find_map(|c| match c {
                Character::Font(f) if f.name == font.name && !f.glyphs.is_empty() => Some(f),
                _ => None,
            });
            if let Some(source) = source {
                *font = source.clone();
                filled += 1;
            }
        }
        filled
    }
}

/// What the movie's scripts would have done to an instance, by its path of instance
/// names from the sprite drawn (`Meter_Animation/Meter/Mask`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Tweak {
    /// The frame it is on, counted from 1.
    pub frame: Option<usize>,
    /// Its `_x` and `_y`, in place of its placement's.
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub hidden: bool,
    /// A text field's text (its `text` / `htmlText` as a script set it), in place of the
    /// field's initial text.
    pub text: Option<String>,
    /// Its `_alpha` as a script set it (0..1): the alpha multiplier of its colour transform, in place of
    /// its placement's.
    pub alpha: Option<f32>,
    /// A bitmap a script attached (`attachBitmap`): drawn over its own content from its origin, `width` x
    /// `height` pixels of its space.
    pub attach: Option<Attached>,
    /// A text field's `textColor` as a script set it (red, green, blue 0..1), in place of the field's own.
    pub text_colour: Option<[f32; 3]>,
}

/// A bitmap attached to an instance (`Tweak::attach`): one of `Movie::bitmaps` (`Movie::add_bitmap`) and its size.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Attached {
    pub bitmap: u16,
    pub width: f32,
    pub height: f32,
}

pub type Pose = HashMap<String, Tweak>;

/// A picture being drawn: red, green, blue and alpha of each pixel, the colours already
/// times the alpha.
#[derive(Clone)]
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[f32; 4]>,
    /// The rows anything was drawn on: first, and one past the last (empty when first >= end).
    touched: (usize, usize),
}

/// Sub-scanlines to a pixel row.
const ROWS: usize = 4;

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, pixels: vec![[0.0; 4]; width * height], touched: (usize::MAX, 0) }
    }

    fn over(&mut self, at: usize, colour: [f32; 4], coverage: f32) {
        let alpha = colour[3] * coverage;
        if alpha <= 0.0 {
            return;
        }
        let row = at / self.width.max(1);
        self.touched = (self.touched.0.min(row), self.touched.1.max(row + 1));
        let pixel = &mut self.pixels[at];
        for i in 0..3 {
            pixel[i] = colour[i] * alpha + pixel[i] * (1.0 - alpha);
        }
        pixel[3] = alpha + pixel[3] * (1.0 - alpha);
    }

    /// Red, green, blue, alpha bytes with the colours not multiplied by the alpha.
    pub fn rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len() * 4);
        for pixel in &self.pixels {
            let alpha = pixel[3];
            for value in &pixel[..3] {
                out.push(if alpha > 0.0 { (value / alpha * 255.0).round().clamp(0.0, 255.0) as u8 } else { 0 });
            }
            out.push((alpha * 255.0).round().clamp(0.0, 255.0) as u8);
        }
        out
    }

    /// One fill of a shape: the edges with it on one side only, filled between crossings.
    fn fill(&mut self, segments: &[[f32; 4]], mut paint: impl FnMut(f32, f32) -> [f32; 4]) {
        if segments.is_empty() || self.width == 0 {
            return;
        }
        let top = segments.iter().map(|s| s[1].min(s[3])).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
        let bottom = (segments.iter().map(|s| s[1].max(s[3])).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(self.height);
        let mut coverage = vec![0.0f32; self.width];
        let mut crossings = Vec::new();
        for row in top..bottom {
            coverage.fill(0.0);
            let mut any = false;
            for sub in 0..ROWS {
                let y = row as f32 + (sub as f32 + 0.5) / ROWS as f32;
                crossings.clear();
                for s in segments {
                    let (y0, y1) = (s[1].min(s[3]), s[1].max(s[3]));
                    if y >= y0 && y < y1 {
                        crossings.push(s[0] + (s[2] - s[0]) * (y - s[1]) / (s[3] - s[1]));
                    }
                }
                crossings.sort_by(f32::total_cmp);
                for pair in crossings.as_chunks::<2>().0 {
                    let (x0, x1) = (pair[0].clamp(0.0, self.width as f32), pair[1].clamp(0.0, self.width as f32));
                    if x1 <= x0 {
                        continue;
                    }
                    any = true;
                    for x in x0.floor() as usize..(x1.ceil() as usize).min(self.width) {
                        coverage[x] += (x1.min(x as f32 + 1.0) - x0.max(x as f32)).max(0.0) / ROWS as f32;
                    }
                }
            }
            if !any {
                continue;
            }
            for (x, &covered) in coverage.iter().enumerate() {
                if covered > 0.0 {
                    self.over(row * self.width + x, paint(x as f32 + 0.5, row as f32 + 0.5), covered.min(1.0));
                }
            }
        }
    }
}

fn gradient(stops: &[(f32, [f32; 4])], t: f32) -> [f32; 4] {
    let Some(first) = stops.first() else { return [0.0; 4] };
    if t <= first.0 {
        return first.1;
    }
    for pair in stops.windows(2) {
        if t <= pair[1].0 {
            let k = (t - pair[0].0) / (pair[1].0 - pair[0].0).max(1e-6);
            return std::array::from_fn(|i| pair[0].1[i] + (pair[1].1[i] - pair[0].1[i]) * k);
        }
    }
    stops[stops.len() - 1].1
}

/// Half the side of the square a gradient is defined on: 16384 twips.
const GRADIENT: f32 = 819.2;

impl Movie {
    fn draw_shape(&self, shape: &Shape, to_canvas: &Affine, tint: &Tint, canvas: &mut Canvas) {
        for (index, fill) in shape.fills.iter().enumerate() {
            let style = index as u32 + 1;
            let mut segments = Vec::new();
            for edge in shape.edges.iter().filter(|e| (e.fill0 == style) != (e.fill1 == style)) {
                let mut from = to_canvas.apply(edge.from.0, edge.from.1);
                let parts = if edge.control.is_some() { 8 } else { 1 };
                for part in 1..=parts {
                    let t = part as f32 / parts as f32;
                    let (x, y) = match edge.control {
                        Some(c) => {
                            let u = 1.0 - t;
                            (u * u * edge.from.0 + 2.0 * u * t * c.0 + t * t * edge.to.0, u * u * edge.from.1 + 2.0 * u * t * c.1 + t * t * edge.to.1)
                        }
                        None => edge.to,
                    };
                    let to = to_canvas.apply(x, y);
                    if from.1 != to.1 {
                        segments.push([from.0, from.1, to.0, to.1]);
                    }
                    from = to;
                }
            }
            match fill {
                Fill::Solid(colour) => {
                    let colour = tint.apply(*colour);
                    canvas.fill(&segments, |_, _| colour);
                }
                Fill::Linear(matrix, stops) | Fill::Radial(matrix, stops) => {
                    let Some(back) = to_canvas.then(matrix).inverse() else { continue };
                    let radial = matches!(fill, Fill::Radial(..));
                    canvas.fill(&segments, |x, y| {
                        let (gx, gy) = back.apply(x, y);
                        let t = if radial { (gx * gx + gy * gy).sqrt() / GRADIENT } else { (gx + GRADIENT) / (2.0 * GRADIENT) };
                        tint.apply(gradient(stops, t.clamp(0.0, 1.0)))
                    });
                }
                Fill::Bitmap { id, matrix, repeat, smooth } => {
                    let Some(bitmap) = self.bitmaps.get(id) else { continue };
                    let Some(back) = to_canvas.then(matrix).inverse() else { continue };
                    // [data] the fill's matrix maps the image's texels to twips (scale 20 is one texel a pixel); the
                    // reader's matrices take pixels, so a point the inverse gives is a twentieth of a texel.
                    canvas.fill(&segments, |x, y| {
                        let (u, v) = back.apply(x, y);
                        tint.apply(bitmap.sample(u * 20.0, v * 20.0, *smooth, *repeat))
                    });
                }
                Fill::Other => {}
            }
        }
    }

    /// A text field's lines in its font: each glyph's outline scaled from the em square to
    /// the field's size, set along a baseline one ascent below the field's 2-pixel gutter,
    /// lines a font height (ascent + descent) and the field's leading apart, aligned in the field.
    /// A field with `multiline` and `wordWrap` breaks its lines at spaces to fit (`wrap`).
    /// [stand-in] no kerning or letter spacing; a character the font lacks is
    /// skipped; the HTML's own colours and sizes are not read (the field's are used). OPEN: settles: the Scaleform
    /// glyph advance and kerning rule (not read), and the HTML subset the game's text fields use.
    /// The lines a text field shows `text` in: its own lines, each wrapped to the field when it has multiline and
    /// wordWrap (as `draw_text` lays them out). Empty when the field's font is missing.
    pub fn field_lines(&self, field: &EditText, text: &str) -> Vec<String> {
        let Some(Character::Font(font)) = field.font.and_then(|id| self.characters.get(&id)) else { return Vec::new() };
        let scale = field.height / Font::EM;
        const GUTTER: f32 = 2.0;
        let width = (field.bounds[2] - GUTTER - field.right_margin) - (field.bounds[0] + GUTTER + field.left_margin);
        let advance = |c: char| self.advance(font, scale, c);
        if field.multiline && field.word_wrap {
            text.lines().flat_map(|line| wrap(line, width, &advance)).collect()
        } else {
            text.lines().map(str::to_owned).collect()
        }
    }

    /// A line's width in a field's font (`getLineMetrics(i).width`, `textWidth` of one line): its advances, inline
    /// pictures by their widths. 0 when the font is missing.
    pub fn line_width(&self, field: &EditText, line: &str) -> f32 {
        let Some(Character::Font(font)) = field.font.and_then(|id| self.characters.get(&id)) else { return 0.0 };
        let scale = field.height / Font::EM;
        line.chars().map(|c| self.advance(font, scale, c)).sum()
    }

    /// A line's height in a field's font (`getLineMetrics(i).height`): ascent and descent at the field's size plus
    /// the field's leading, as `draw_text` spaces lines. 0 when the font is missing.
    pub fn line_height(&self, field: &EditText) -> f32 {
        let Some(Character::Font(font)) = field.font.and_then(|id| self.characters.get(&id)) else { return 0.0 };
        (font.ascent + font.descent) * (field.height / Font::EM) + field.leading
    }

    /// A character's bounds in its own space (left, top, right, bottom), as `_width` / `_height` measure them: a
    /// shape's, a text field's, a button's hit area, a sprite's children on frame 1 through their matrices. None for
    /// an empty sprite or another kind.
    pub fn bounds(&self, id: u16) -> Option<[f32; 4]> {
        let union = |places: &[Place]| {
            places.iter().filter_map(|p| Some(transform_box(&p.matrix.unwrap_or(Affine::IDENTITY), self.bounds(p.id?)?))).reduce(|a, b| {
                [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
            })
        };
        match self.characters.get(&id)? {
            Character::Shape(s) => Some(s.bounds),
            Character::Text(t) => Some(t.bounds),
            Character::Button(b) => union(&b.hit),
            Character::Sprite(s) => union(&s.shown(1)),
            _ => None,
        }
    }

    /// The instance at a path of instance names under `root` (on its sprite's frame in `pose`, else frame 1, with
    /// the pose's `_x` / `_y`): its transform to `root`'s space and its character.
    pub fn instance(&self, root: u16, path: &str, pose: &Pose) -> Option<(Affine, u16)> {
        let (mut id, mut to_root, mut at) = (root, Affine::IDENTITY, String::new());
        for name in path.split('/') {
            let Some(Character::Sprite(sprite)) = self.characters.get(&id) else { return None };
            let frame = pose.get(&at).and_then(|t| t.frame).unwrap_or(1);
            let place = sprite.shown(frame).into_iter().find(|p| p.name.as_deref() == Some(name))?;
            at = if at.is_empty() { name.to_owned() } else { format!("{at}/{name}") };
            let mut m = place.matrix.unwrap_or(Affine::IDENTITY);
            if let Some(t) = pose.get(&at) {
                m.tx = t.x.unwrap_or(m.tx);
                m.ty = t.y.unwrap_or(m.ty);
            }
            to_root = to_root.then(&m);
            id = place.id?;
        }
        Some((to_root, id))
    }

    /// The text field at a path of instance names under `root` (each name looked for in any frame of its sprite).
    pub fn text_field(&self, root: u16, path: &str) -> Option<&EditText> {
        let mut id = root;
        for name in path.split('/') {
            let Some(Character::Sprite(sprite)) = self.characters.get(&id) else { return None };
            id = (1..=sprite.frames.len().max(1)).find_map(|f| sprite.shown(f).into_iter().find(|p| p.name.as_deref() == Some(name)).and_then(|p| p.id))?;
        }
        match self.characters.get(&id) {
            Some(Character::Text(field)) => Some(field),
            _ => None,
        }
    }

    fn draw_text(&self, field: &EditText, text: &str, to_canvas: &Affine, tint: &Tint, canvas: &mut Canvas) {
        let Some(Character::Font(font)) = field.font.and_then(|id| self.characters.get(&id)) else { return };
        let scale = field.height / Font::EM;
        let colour = Tint { mul: field.colour, add: [0.0; 4] };
        let tint_outer = tint;
        let tint = tint.then(&colour);
        const GUTTER: f32 = 2.0;
        let (left, right) = (field.bounds[0] + GUTTER + field.left_margin, field.bounds[2] - GUTTER - field.right_margin);
        // [data] the SWF format defines DefineEditText's `Leading` as the distance from the bottom of one line's descent to
        // the top of the next line's ascent, so a line is the font's ascent and descent at the field's size plus the
        // field's own leading, not the font's (the loading screen's top box: 23.1 px a line, where the font's leading gave
        // 20.25; the user's reference frame shows its lines farther apart than ours had them). Not measured on a frame of
        // the original.
        let line_height = (font.ascent + font.descent) * scale + field.leading;
        let mut baseline = field.bounds[1] + GUTTER + font.ascent * scale;
        let advance = |c: char| self.advance(font, scale, c);
        let lines: Vec<String> = if field.multiline && field.word_wrap {
            text.lines().flat_map(|line| wrap(line, right - left, &advance)).collect()
        } else {
            text.lines().map(str::to_owned).collect()
        };
        for line in &lines {
            // [assumed] a line with an inline picture that rises above the font's ascent is moved down by the
            // difference, as a player sizes a line to its tallest item (Scaleform's own rule is not read). OPEN:
            // settles: the Scaleform line-height routine (not read); a HUD text field with an inline picture.
            let rise = line.chars().filter_map(|c| self.text_image(c)).map(|i| -i.top).fold(0.0, f32::max);
            baseline += (rise - font.ascent * scale).max(0.0);
            let width: f32 = line.chars().map(advance).sum();
            let mut x = match field.align {
                1 => right - width,
                2 => (left + right - width) / 2.0,
                _ => left,
            };
            for c in line.chars() {
                if let Some(image) = self.text_image(c) {
                    // The picture keeps its own colours: only the movie's tint applies, not the field's colour.
                    let Some(rect) = self.image_rect(image) else { x += image.width; continue };
                    let place = Affine { tx: x, ty: baseline + image.top, ..Affine::IDENTITY };
                    self.draw_shape(&rect, &to_canvas.then(&place), tint_outer, canvas);
                } else if let Some(g) = font.glyph(c) {
                    let place = Affine { a: scale, b: 0.0, c: 0.0, d: scale, tx: x, ty: baseline };
                    self.draw_shape(&font.glyphs[g], &to_canvas.then(&place), &tint, canvas);
                }
                x += advance(c);
            }
            baseline += line_height;
        }
    }

    /// A character's advance in a field's font at `scale`, or an inline picture's width.
    fn advance(&self, font: &Font, scale: f32, c: char) -> f32 {
        match self.text_image(c) {
            Some(image) => image.width,
            None => font.glyph(c).map_or(0.0, |g| font.advances.get(g).copied().unwrap_or(Font::EM) * scale),
        }
    }

    /// An inline picture as a shape: a `width` x `height` rectangle from (0, 0) filled with its bitmap stretched to
    /// it (smoothed, edges clamped).
    fn image_rect(&self, image: &TextImage) -> Option<Shape> {
        self.bitmap_rect(image.bitmap, image.width, image.height)
    }

    /// A `w` x `h` rectangle from (0, 0) filled with bitmap `id` stretched to it (smoothed, edges clamped).
    fn bitmap_rect(&self, id: u16, w: f32, h: f32) -> Option<Shape> {
        let bitmap = self.bitmaps.get(&id)?;
        // The fill's matrix takes texels to twips (20 a pixel): one texel is w / width pixels across.
        let matrix = Affine { a: 20.0 * w / bitmap.width.max(1) as f32, b: 0.0, c: 0.0, d: 20.0 * h / bitmap.height.max(1) as f32, tx: 0.0, ty: 0.0 };
        let corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
        let edges = (0..4).map(|i| Edge { from: corners[i], control: None, to: corners[(i + 1) % 4], fill0: 0, fill1: 1 }).collect();
        Some(Shape { bounds: [0.0, 0.0, w, h], fills: vec![Fill::Bitmap { id, matrix, repeat: false, smooth: true }], edges })
    }

    /// The inline picture a character stands for.
    pub fn text_image(&self, c: char) -> Option<&TextImage> {
        (c as u32).checked_sub(TEXT_IMAGE_FIRST).and_then(|n| self.text_images.get(n as usize))
    }

    /// Adds a bitmap for `Tweak::attach` and returns its id (shared with the inline pictures' range).
    pub fn add_bitmap(&mut self, bitmap: Bitmap) -> u16 {
        // It takes an inline picture's slot (an unused character), so the two kinds' ids never meet.
        let id = TEXT_IMAGE_BITMAPS + self.text_images.len() as u16;
        self.text_images.push(TextImage { bitmap: id, width: 0.0, height: 0.0, top: 0.0 });
        self.bitmaps.insert(id, bitmap);
        id
    }

    /// Adds a picture to set inline in text, `width` x `height` pixels with its top `top` from the baseline, and
    /// returns the character that stands for it.
    pub fn add_text_image(&mut self, bitmap: Bitmap, width: f32, height: f32, top: f32) -> char {
        let n = self.text_images.len();
        let id = TEXT_IMAGE_BITMAPS + n as u16;
        self.bitmaps.insert(id, bitmap);
        self.text_images.push(TextImage { bitmap: id, width, height, top });
        char::from_u32(TEXT_IMAGE_FIRST + n as u32).unwrap_or('\u{E000}')
    }

    /// `draw` onto a new `width` x `height` canvas, its rows split into bands drawn on `threads` threads (each band
    /// draws the whole movie with its rows moved up; the scanline fill skips what falls outside it).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_banded(&self, id: u16, to_canvas: &Affine, tint: &Tint, pose: &Pose, width: usize, height: usize, threads: usize) -> Canvas {
        let threads = threads.clamp(1, height.max(1));
        let band = height.div_ceil(threads);
        let bands: Vec<Canvas> = std::thread::scope(|scope| {
            let jobs: Vec<_> = (0..threads)
                .map(|i| {
                    let top = (i * band).min(height);
                    let rows = band.min(height - top);
                    scope.spawn(move || {
                        let mut canvas = Canvas::new(width, rows);
                        if rows > 0 {
                            let moved = Affine { tx: 0.0, ty: -(top as f32), ..Affine::IDENTITY }.then(to_canvas);
                            self.draw(id, &moved, tint, pose, "", 0.0, &mut canvas);
                        }
                        canvas
                    })
                })
                .collect();
            jobs.into_iter().map(|j| j.join().unwrap_or_else(|_| Canvas::new(width, 0))).collect()
        });
        let mut out = Canvas::new(width, height);
        let mut at = 0;
        for b in bands {
            let n = b.pixels.len().min(out.pixels.len() - at);
            out.pixels[at..at + n].copy_from_slice(&b.pixels[..n]);
            at += b.width * b.height;
        }
        out.touched = (0, height);
        out
    }

    /// Draws a character as `to_canvas` places it. `clock` is the seconds a timeline that
    /// plays by itself has been playing (they loop); `path` is the instance's own path,
    /// which `pose` is looked up by.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, id: u16, to_canvas: &Affine, tint: &Tint, pose: &Pose, path: &str, clock: f32, canvas: &mut Canvas) {
        match self.characters.get(&id) {
            Some(Character::Shape(shape)) => self.draw_shape(shape, to_canvas, tint, canvas),
            Some(Character::Text(field)) => {
                let text = pose.get(path).and_then(|t| t.text.as_deref()).unwrap_or(&field.text);
                match pose.get(path).and_then(|t| t.text_colour) {
                    Some(c) => {
                        let field = EditText { colour: [c[0], c[1], c[2], field.colour[3]], ..field.clone() };
                        self.draw_text(&field, text, to_canvas, tint, canvas);
                    }
                    None => self.draw_text(field, text, to_canvas, tint, canvas),
                }
            }
            Some(Character::Sprite(sprite)) => {
                let frame = match pose.get(path).and_then(|tweak| tweak.frame) {
                    Some(frame) => frame,
                    None if sprite.stops || sprite.frames.len() < 2 => 1,
                    None => (clock * self.rate) as usize % sprite.frames.len() + 1,
                };
                let shown = sprite.shown(frame);
                let mut at = 0;
                while at < shown.len() {
                    let item = &shown[at];
                    at += 1;
                    let Some(clip) = item.clip else {
                        self.draw_placed(item, to_canvas, tint, pose, path, clock, canvas);
                        continue;
                    };
                    // A mask: what lies above it up to its clip depth shows only where
                    // the mask has a shape, whatever the shape's colour or opacity.
                    let mut mask = Canvas::new(canvas.width, canvas.height);
                    let solid = Tint { mul: [0.0; 4], add: [1.0; 4] };
                    self.draw_placed(item, to_canvas, &solid, pose, path, clock, &mut mask);
                    let mut layer = Canvas::new(canvas.width, canvas.height);
                    while at < shown.len() && shown[at].depth <= clip {
                        // Nothing shows through an empty mask.
                        if mask.touched.0 < mask.touched.1 {
                            self.draw_placed(&shown[at], to_canvas, tint, pose, path, clock, &mut layer);
                        }
                        at += 1;
                    }
                    // Only the rows both the mask and the layer drew on.
                    let (first, end) = (mask.touched.0.max(layer.touched.0), mask.touched.1.min(layer.touched.1));
                    if first >= end {
                        continue;
                    }
                    canvas.touched = (canvas.touched.0.min(first), canvas.touched.1.max(end));
                    let span = first * canvas.width..end * canvas.width;
                    for ((pixel, layer), mask) in canvas.pixels[span.clone()].iter_mut().zip(&layer.pixels[span.clone()]).zip(&mask.pixels[span]) {
                        let alpha = layer[3] * mask[3];
                        if alpha > 0.0 {
                            for i in 0..3 {
                                pixel[i] = layer[i] * mask[3] + pixel[i] * (1.0 - alpha);
                            }
                            pixel[3] = alpha + pixel[3] * (1.0 - alpha);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_placed(&self, item: &Place, to_canvas: &Affine, tint: &Tint, pose: &Pose, path: &str, clock: f32, canvas: &mut Canvas) {
        let Some(id) = item.id else { return };
        let name = item.name.clone().unwrap_or_else(|| format!("#{}", item.depth));
        let child = if path.is_empty() { name } else { format!("{path}/{name}") };
        let tweak = pose.get(&child).cloned().unwrap_or_default();
        if tweak.hidden {
            return;
        }
        let mut matrix = item.matrix.unwrap_or(Affine::IDENTITY);
        matrix.tx = tweak.x.unwrap_or(matrix.tx);
        matrix.ty = tweak.y.unwrap_or(matrix.ty);
        let mut own = item.tint.unwrap_or(Tint::NONE);
        if let Some(a) = tweak.alpha {
            own.mul[3] = a;
        }
        let tint = tint.then(&own);
        // Fully transparent: nothing of it shows (a mask's solid tint adds its alpha, so masks still draw).
        if tint.mul[3] <= 0.0 && tint.add[3] <= 0.0 {
            return;
        }
        let to_child = to_canvas.then(&matrix);
        self.draw(id, &to_child, &tint, pose, &child, clock, canvas);
        if let Some(a) = tweak.attach
            && let Some(rect) = self.bitmap_rect(a.bitmap, a.width, a.height) {
            self.draw_shape(&rect, &to_child, &tint, canvas);
        }
    }

    /// The paths of the buttons whose hit area holds the point (`x`, `y`, in the space `to_canvas` maps to),
    /// in drawing order (the topmost last), walking the display list as `draw` does. An instance the pose
    /// hides (`_visible` false) and what it holds are not hit; transparency does not matter (Flash tests an
    /// `_alpha` 0 button). [assumed] masks do not limit the hit areas. OPEN: settles: the Scaleform hit test for a
    /// button under a mask (not read; the executable's hit routine is the answer).
    #[allow(clippy::too_many_arguments)]
    pub fn hit(&self, id: u16, to_canvas: &Affine, pose: &Pose, path: &str, clock: f32, x: f32, y: f32, out: &mut Vec<String>) {
        match self.characters.get(&id) {
            Some(Character::Button(button)) => {
                if button.hit.iter().any(|r| self.contains(r, to_canvas, x, y)) {
                    out.push(path.to_owned());
                }
            }
            Some(Character::Sprite(sprite)) => {
                let frame = match pose.get(path).and_then(|tweak| tweak.frame) {
                    Some(frame) => frame,
                    None if sprite.stops || sprite.frames.len() < 2 => 1,
                    None => (clock * self.rate) as usize % sprite.frames.len() + 1,
                };
                for item in sprite.shown(frame) {
                    let Some(child_id) = item.id else { continue };
                    let name = item.name.clone().unwrap_or_else(|| format!("#{}", item.depth));
                    let child = if path.is_empty() { name } else { format!("{path}/{name}") };
                    let tweak = pose.get(&child).cloned().unwrap_or_default();
                    if tweak.hidden {
                        continue;
                    }
                    let mut matrix = item.matrix.unwrap_or(Affine::IDENTITY);
                    matrix.tx = tweak.x.unwrap_or(matrix.tx);
                    matrix.ty = tweak.y.unwrap_or(matrix.ty);
                    self.hit(child_id, &to_canvas.then(&matrix), pose, &child, clock, x, y, out);
                }
            }
            _ => {}
        }
    }

    /// Whether a placed character's filled area holds the point: a shape by the even-odd crossings of each
    /// fill's edges (as `draw_shape` fills it), a sprite by any of its frame-1 children.
    fn contains(&self, item: &Place, to_canvas: &Affine, x: f32, y: f32) -> bool {
        let Some(id) = item.id else { return false };
        let m = to_canvas.then(&item.matrix.unwrap_or(Affine::IDENTITY));
        match self.characters.get(&id) {
            Some(Character::Shape(shape)) => {
                let Some(back) = m.inverse() else { return false };
                let (px, py) = back.apply(x, y);
                (1..=shape.fills.len() as u32).any(|style| {
                    let mut inside = false;
                    for edge in shape.edges.iter().filter(|e| (e.fill0 == style) != (e.fill1 == style)) {
                        let parts = if edge.control.is_some() { 8 } else { 1 };
                        let mut from = edge.from;
                        for part in 1..=parts {
                            let t = part as f32 / parts as f32;
                            let to = match edge.control {
                                Some(c) => {
                                    let u = 1.0 - t;
                                    (u * u * edge.from.0 + 2.0 * u * t * c.0 + t * t * edge.to.0, u * u * edge.from.1 + 2.0 * u * t * c.1 + t * t * edge.to.1)
                                }
                                None => edge.to,
                            };
                            let (y0, y1) = (from.1.min(to.1), from.1.max(to.1));
                            if py >= y0 && py < y1 && from.0 + (to.0 - from.0) * (py - from.1) / (to.1 - from.1) < px {
                                inside = !inside;
                            }
                            from = to;
                        }
                    }
                    inside
                })
            }
            Some(Character::Sprite(sprite)) => sprite.shown(1).iter().any(|c| self.contains(c, &m, x, y)),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_wrap_at_spaces_and_long_words_break() {
        let one = |_: char| 1.0;
        assert_eq!(wrap("AB CD EF", 5.0, &one), ["AB CD", "EF"]);
        assert_eq!(wrap("ABCDEFG H", 3.0, &one), ["ABC", "DEF", "G H"]);
        assert_eq!(wrap("", 3.0, &one), [""]);
    }

    fn square(left: f32, top: f32, right: f32, bottom: f32, fill: Fill) -> Shape {
        let corners = [(left, top), (right, top), (right, bottom), (left, bottom)];
        let edges = (0..4).map(|i| Edge { from: corners[i], control: None, to: corners[(i + 1) % 4], fill0: 0, fill1: 1 }).collect();
        Shape { bounds: [left, top, right, bottom], fills: vec![fill], edges }
    }

    fn movie(characters: Vec<(u16, Character)>) -> Movie {
        Movie { stage: [16.0, 16.0], rate: 30.0, characters: characters.into_iter().collect(), exports: HashMap::new(), imports: Vec::new(), bitmaps: HashMap::new(), text_images: Vec::new() }
    }

    #[test]
    fn a_square_covers_its_pixels_and_half_covers_its_edges() {
        let movie = movie(vec![(1, Character::Shape(square(2.0, 2.0, 6.5, 6.0, Fill::Solid([1.0, 0.0, 0.0, 1.0]))))]);
        let mut canvas = Canvas::new(8, 8);
        movie.draw(1, &Affine::IDENTITY, &Tint::NONE, &Pose::new(), "", 0.0, &mut canvas);
        assert_eq!(canvas.pixels[3 * 8 + 3], [1.0, 0.0, 0.0, 1.0]);
        assert!((canvas.pixels[3 * 8 + 6][3] - 0.5).abs() < 1e-5);
        assert_eq!(canvas.pixels[3 * 8 + 7][3], 0.0);
        assert_eq!(canvas.pixels[8 + 3][3], 0.0);
        assert_eq!(&canvas.rgba8()[(3 * 8 + 6) * 4..][..4], &[255, 0, 0, 128]);
    }

    #[test]
    fn a_mask_and_a_pose_cut_what_is_above_them() {
        // Like the game's meters: a bar under a mask that a script slides to the left.
        let bar = Character::Shape(square(0.0, 0.0, 8.0, 4.0, Fill::Solid([0.0, 1.0, 0.0, 1.0])));
        let cover = Character::Shape(square(0.0, 0.0, 8.0, 4.0, Fill::Solid([1.0, 0.0, 0.0, 1.0])));
        let mask = Character::Sprite(Sprite { frames: vec![vec![Step::Place(Place { depth: 1, id: Some(2), ..Default::default() })]], ..Default::default() });
        let meter = Character::Sprite(Sprite {
            frames: vec![vec![
                Step::Place(Place { depth: 1, id: Some(3), name: Some("Mask".into()), clip: Some(4), ..Default::default() }),
                Step::Place(Place { depth: 3, id: Some(1), ..Default::default() }),
            ]],
            ..Default::default()
        });
        let movie = movie(vec![(1, bar), (2, cover), (3, mask), (4, meter)]);
        let mut pose = Pose::new();
        pose.insert("Mask".into(), Tweak { x: Some(-4.0), ..Default::default() });
        let mut canvas = Canvas::new(8, 4);
        movie.draw(4, &Affine::IDENTITY, &Tint::NONE, &pose, "", 0.0, &mut canvas);
        assert_eq!(canvas.pixels[8 + 2], [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(canvas.pixels[8 + 5], [0.0; 4]);
    }

    #[test]
    fn a_gradient_runs_across_its_square_and_a_tint_changes_it() {
        // The square scaled to 8 pixels wide, its middle at x = 4.
        let matrix = Affine { a: 8.0 / (2.0 * GRADIENT), d: 1.0, tx: 4.0, ..Affine::IDENTITY };
        let stops = vec![(0.0, [0.0, 0.0, 0.0, 1.0]), (1.0, [1.0, 1.0, 1.0, 1.0])];
        let movie = movie(vec![(1, Character::Shape(square(0.0, 0.0, 8.0, 2.0, Fill::Linear(matrix, stops))))]);
        let mut canvas = Canvas::new(8, 2);
        movie.draw(1, &Affine::IDENTITY, &Tint::NONE, &Pose::new(), "", 0.0, &mut canvas);
        assert!((canvas.pixels[2][0] - 2.5 / 8.0).abs() < 1e-4);
        let mut tinted = Canvas::new(8, 2);
        let tint = Tint { mul: [0.0, 0.0, 0.0, 0.5], add: [1.0, 0.0, 0.0, 0.0] };
        movie.draw(1, &Affine::IDENTITY, &tint, &Pose::new(), "", 0.0, &mut tinted);
        assert_eq!(tinted.pixels[2], [0.5, 0.0, 0.0, 0.5]);
    }

    #[test]
    fn a_timeline_keeps_what_a_move_does_not_change() {
        let sprite = Sprite {
            frames: vec![
                vec![Step::Place(Place { depth: 4, id: Some(9), name: Some("a".into()), ..Default::default() })],
                vec![Step::Place(Place { depth: 4, moves: true, matrix: Some(Affine { tx: 5.0, ..Affine::IDENTITY }), ..Default::default() })],
                vec![Step::Remove(4)],
            ],
            ..Default::default()
        };
        let second = sprite.shown(2);
        assert_eq!((second[0].id, second[0].name.as_deref(), second[0].matrix.unwrap().tx), (Some(9), Some("a"), 5.0));
        assert!(sprite.shown(3).is_empty());
    }

    #[test]
    fn html_text_loses_its_tags_and_keeps_its_lines() {
        let html = r#"<p align="center"><font face="OCRSTDFont" size="34">00</font></p><p>a &amp; b</p>"#;
        assert_eq!(plain_text(html), "00\na & b");
        assert_eq!(plain_text(r#"<p align="left"></p>"#), "");
    }

    #[test]
    fn a_field_sets_its_glyphs_along_the_baseline_and_aligns_them() {
        // A one-glyph font: a box 512 wide and 1024 tall standing on the baseline, advance 512.
        let mut glyph = square(0.0, -1024.0, 512.0, 0.0, Fill::Solid([1.0; 4]));
        glyph.bounds = [0.0; 4];
        let font = Font { name: "box".into(), glyphs: vec![glyph], codes: vec![u16::from(b'A')], advances: vec![512.0], ascent: 1024.0, descent: 0.0, leading: 0.0 };
        // 10 pixels high (so a glyph is 5 x 10), a field 40 wide, centred: two glyphs fill x 12 to 22... of the
        // 36 inside the gutters (2 to 38): the line is 10 wide, so it starts at 15.
        let field = EditText { bounds: [0.0, 0.0, 40.0, 14.0], font: Some(1), height: 10.0, colour: [0.0, 1.0, 0.0, 1.0], align: 2, text: "AA".into(), ..Default::default() };
        let movie = movie(vec![(1, Character::Font(font)), (2, Character::Text(field))]);
        let mut canvas = Canvas::new(40, 14);
        movie.draw(2, &Affine::IDENTITY, &Tint::NONE, &Pose::new(), "", 0.0, &mut canvas);
        // Baseline at 2 + 10 = 12: the glyphs cover rows 2 to 12, columns 15 to 25.
        assert_eq!(canvas.pixels[5 * 40 + 16], [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(canvas.pixels[5 * 40 + 24], [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(canvas.pixels[5 * 40 + 13][3], 0.0);
        assert_eq!(canvas.pixels[5 * 40 + 26][3], 0.0);
        assert_eq!(canvas.pixels[13 * 40 + 16][3], 0.0);
        // A pose's text replaces the field's: one glyph, centred, covers 17.5 to 22.5.
        let mut pose = Pose::new();
        pose.insert(String::new(), Tweak { text: Some("A".into()), ..Default::default() });
        let mut one = Canvas::new(40, 14);
        movie.draw(2, &Affine::IDENTITY, &Tint::NONE, &pose, "", 0.0, &mut one);
        assert_eq!(one.pixels[5 * 40 + 16][3], 0.0);
        assert_eq!(one.pixels[5 * 40 + 20], [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn the_hud_takes_its_fonts_from_the_font_movie_and_writes_the_timer() {
        // Needs the install.
        let dir = &crate::install::dir().join("UI/Flash");
        let (Ok(hud), Ok(fonts), Ok(glyphs)) = (std::fs::read(dir.join("hud.gfx")), std::fs::read(dir.join("font.gfx")), std::fs::read(dir.join("font_glyphs.gfx"))) else { return };
        let mut movie = Movie::parse(&hud).unwrap();
        let mut fonts = Movie::parse(&fonts).unwrap();
        let glyphs = Movie::parse(&glyphs).unwrap();
        // [data] font.gfx exports "OCR A ATVI Custom" as OCRSTDFont and "SF TransRobotics" as T2Font, both
        // without outlines; font_glyphs.gfx has the outlines under the same names (825 glyphs each).
        let Character::Font(stub) = &fonts.characters[&fonts.exports["OCRSTDFont"]] else { panic!() };
        assert_eq!((stub.name.as_str(), stub.glyphs.len()), ("OCR A ATVI Custom", 0));
        assert_eq!(fonts.glyphs_from(&glyphs), 2);
        let Character::Font(ocr) = &fonts.characters[&fonts.exports["OCRSTDFont"]] else { panic!() };
        assert_eq!((ocr.glyphs.len(), ocr.advances.len()), (825, 825));
        assert!(ocr.glyph('7').is_some() && ocr.glyph(':').is_some() && ocr.ascent > 0.0);
        assert_eq!(movie.import_from("font.gfx", &fonts), 2);
        assert!(matches!(movie.characters.get(&129), Some(Character::Font(f)) if f.name == "OCR A ATVI Custom" && !f.glyphs.is_empty()));
        // The timer's text (Timer_Text's label_txt) drawn with a time: it leaves ink.
        let timer = movie.exports["Timer Class"];
        let mut pose = Pose::new();
        pose.insert("Timer_Text/label_txt".into(), Tweak { text: Some("01:23.45".into()), ..Default::default() });
        let mut canvas = Canvas::new(1280, 720);
        movie.draw(timer, &Affine::scale_then_move(1.0, 640.0, 360.0), &Tint::NONE, &pose, "", 0.0, &mut canvas);
        let ink = canvas.pixels.iter().filter(|p| p[3] > 0.5).count();
        let mut blank = Canvas::new(1280, 720);
        movie.draw(timer, &Affine::scale_then_move(1.0, 640.0, 360.0), &Tint::NONE, &Pose::new(), "", 0.0, &mut blank);
        let without = blank.pixels.iter().filter(|p| p[3] > 0.5).count();
        assert!(ink > without + 100, "{ink} vs {without}");
    }

    #[test]
    fn the_games_hud_reads_as_the_dump_has_it() {
        // Needs the install.
        let Ok(file) = std::fs::read(crate::install::dir().join("UI/Flash/hud.gfx")) else { return };
        let movie = Movie::parse(&file).unwrap();
        assert_eq!((movie.stage, movie.rate), ([1280.0, 720.0], 30.0));
        let health = movie.sprite(movie.exports["Health_Meter Class"]).unwrap();
        assert_eq!((health.frames.len(), health.label("main")), (166, Some(59)));
        // The bar's mask is 111 x 23, and the bar under it plays `low_loop` from frame 2.
        let Character::Shape(mask) = &movie.characters[&55] else { panic!() };
        assert_eq!((mask.bounds, mask.edges.len()), ([0.0, 0.0, 111.0, 23.0], 4));
        let meter = movie.sprite(59).unwrap();
        assert!(meter.stops && meter.label("low_loop") == Some(2));
        assert!(!movie.sprite(66).unwrap().stops);
        // Drawn whole, the bar's middle is its colour; with the mask slid away, nothing.
        let mut canvas = Canvas::new(111, 23);
        movie.draw(59, &Affine::IDENTITY, &Tint::NONE, &Pose::new(), "", 0.0, &mut canvas);
        let middle = canvas.pixels[11 * 111 + 55];
        assert!((middle[0] - 187.0 / 255.0).abs() < 1e-3 && middle[3] == 1.0, "{middle:?}");
        let mut pose = Pose::new();
        pose.insert("Mask".into(), Tweak { x: Some(-111.0), ..Default::default() });
        let mut empty = Canvas::new(111, 23);
        movie.draw(59, &Affine::IDENTITY, &Tint::NONE, &pose, "", 0.0, &mut empty);
        assert_eq!(empty.pixels[11 * 111 + 55][3], 0.0);
    }
}
