//! The HUD's pictures for an engine that cannot play the game's Flash movie: the reticle's
//! textures (`tf2_core::reticle`), the meters' icons, and each meter of `UI\Flash\hud.gfx`
//! (`tf2_core::hud`) cut into the few pictures its scripts can make, with where its bar is, so
//! that a port lays them back together. `hud.json` says how (FORMAT.md, "The HUD").
//!
//! A meter's value only slides its bar's `Mask`, so the meter at any value is the meter full on
//! the bar's side of the mask's edge and the meter empty on the other: two pictures per frame
//! of whatever else the scripts play (the health bar's `low_loop`, the cooldowns' `ready` and
//! `activate`), and one cut at the edge. The health meter's sheen plays by itself on the
//! movie's clock: it is drawn alone, over the rest, a picture per tick of its loop. The export
//! lays every cut back together and compares it with the movie drawn whole (`check`).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::{Value, json};
use tf2_core::formats::gfx::{Affine, Canvas, Character, Fill, Movie, Pose, Tint, Tweak};
use tf2_core::formats::{pack, texture};
use tf2_core::hud::Meter;
use tf2_core::reticle;

/// Pixels of a picture to a pixel of the 1280 x 720 stage.
const SCALE: f32 = 2.0;
/// The movie's frames a second.
const RATE: f32 = 30.0;

/// What the export needs of each meter: the class it is exported as, where `Hud_SP` places it,
/// its animation clip and the mask's size along its slide (as `tf2_core::hud::Meter` has them),
/// whether the bar slides along x, the frames its scripts play, and the sheen's instance.
struct Spec {
    meter: Meter,
    name: &'static str,
    export: &'static str,
    origin: (f32, f32),
    animation: &'static str,
    across: bool,
    sheen: Option<&'static str>,
}

const SPECS: [Spec; 3] = [
    Spec { meter: Meter::Health, name: "health", export: "Health_Meter Class", origin: (639.2, 359.1), animation: "Meter_Animation", across: true, sheen: Some("Meter_Animation/#8") },
    Spec { meter: Meter::Turbo, name: "turbo", export: "Turbo_Meter Class", origin: (639.2, 359.1), animation: "Turbo_Meter_Animation", across: false, sheen: None },
    Spec { meter: Meter::Ability, name: "ability", export: "Ability_Meter Class", origin: (122.2, 600.0), animation: "Ability_Meter_Animation", across: false, sheen: None },
];

/// The pictures written so far, by their pixels: the same picture is written once.
struct Pictures<'a> {
    out: &'a Path,
    seen: HashMap<Vec<u8>, String>,
}

impl Pictures<'_> {
    fn put(&mut self, stem: &str, width: usize, height: usize, rgba: Vec<u8>) -> Result<String, String> {
        if let Some(file) = self.seen.get(&rgba) {
            return Ok(file.clone());
        }
        let file = format!("{stem}_{}.png", self.seen.len());
        image::save_buffer(self.out.join(&file), &rgba, width as u32, height as u32, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
        self.seen.insert(rgba, file.clone());
        Ok(file)
    }
}

/// The name a placement is posed by (`Movie::draw`).
fn place_name(place: &tf2_core::formats::gfx::Place) -> String {
    place.name.clone().unwrap_or_else(|| format!("#{}", place.depth))
}

/// The pose the scripts set: the class on `main`, the animation on `clip_frame`, its `Meter` on
/// `bar_frame`, the mask where `value` puts it (`tf2_core::hud::Hud::pose`).
fn pose(movie: &Movie, spec: &Spec, clip_frame: usize, bar_frame: usize, value: f32, size: f32) -> Pose {
    let mut pose = Pose::new();
    let main = movie.sprite(movie.exports[spec.export]).and_then(|s| s.label("main")).unwrap_or(1);
    pose.insert(String::new(), Tweak { frame: Some(main), ..Default::default() });
    pose.insert(spec.animation.to_owned(), Tweak { frame: Some(clip_frame), ..Default::default() });
    pose.insert(format!("{}/Meter", spec.animation), Tweak { frame: Some(bar_frame), ..Default::default() });
    let mask = if spec.across { Tweak { x: Some(-size + value * size), ..Default::default() } } else { Tweak { y: Some(size - value * size), ..Default::default() } };
    pose.insert(format!("{}/Meter/Mask", spec.animation), mask);
    pose
}

/// Which part of the movie a picture keeps, against the instance at a path.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    /// The instance alone.
    Only,
    /// What is drawn before it.
    Below,
    /// What is drawn after it.
    Above,
}

/// Hides what `part` leaves out, level by level down `path` (masks stay: they draw nothing,
/// and what they cover would go with them).
fn split(movie: &Movie, root: u16, pose: &mut Pose, path: &str, part: Part) {
    let mut id = root;
    let mut at = String::new();
    let parts: Vec<&str> = path.split('/').collect();
    for (level, name_on_path) in parts.iter().enumerate() {
        let frame = pose.get(&at).and_then(|t| t.frame).unwrap_or(1);
        let Some(sprite) = movie.sprite(id) else { return };
        let shown = sprite.shown(frame);
        let Some(target) = shown.iter().find(|p| place_name(p) == *name_on_path) else { return };
        let (depth, next) = (target.depth, target.id);
        for place in &shown {
            let name = place_name(place);
            let child = if at.is_empty() { name.clone() } else { format!("{at}/{name}") };
            let on_path = name == *name_on_path;
            let hide = place.clip.is_none()
                && match part {
                    Part::Only => !on_path,
                    Part::Below => place.depth > depth || (on_path && level + 1 == parts.len()),
                    Part::Above => place.depth < depth || (on_path && level + 1 == parts.len()),
                };
            if hide {
                pose.entry(child).or_default().hidden = true;
            }
        }
        let Some(n) = next else { return };
        id = n;
        at = if at.is_empty() { (*name_on_path).to_owned() } else { format!("{at}/{name_on_path}") };
    }
}

/// `top` over `under`, colours times alpha.
fn over(top: [f32; 4], under: [f32; 4]) -> [f32; 4] {
    [0, 1, 2, 3].map(|i| top[i] + under[i] * (1.0 - top[3]))
}

/// Where the bar (the mask's full size, from the `Meter`'s origin) lies on the stage at a frame
/// of the animation: left, top, right, bottom.
fn bar(movie: &Movie, spec: &Spec, clip_frame: usize, mask: (f32, f32)) -> Option<[f32; 4]> {
    let root = movie.sprite(movie.exports[spec.export])?;
    let main = root.label("main").unwrap_or(1);
    let animation = root.shown(main).into_iter().find(|p| p.name.as_deref() == Some(spec.animation))?;
    let clip = movie.sprite(animation.id?)?;
    let meter = clip.shown(clip_frame).into_iter().find(|p| p.name.as_deref() == Some("Meter"))?;
    let m = Affine { tx: spec.origin.0, ty: spec.origin.1, ..Affine::IDENTITY }
        .then(&animation.matrix.unwrap_or(Affine::IDENTITY))
        .then(&meter.matrix.unwrap_or(Affine::IDENTITY));
    let (x0, y0) = m.apply(0.0, 0.0);
    let (x1, y1) = m.apply(mask.0, mask.1);
    Some([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)])
}

/// The mask's size (its shape's), from the `Meter`'s `Mask` sprite.
fn mask_size(movie: &Movie, spec: &Spec) -> Option<(f32, f32)> {
    let root = movie.sprite(movie.exports[spec.export])?;
    let main = root.label("main").unwrap_or(1);
    let animation = movie.sprite(root.shown(main).into_iter().find(|p| p.name.as_deref() == Some(spec.animation))?.id?)?;
    let meter = movie.sprite(animation.shown(1).into_iter().find(|p| p.name.as_deref() == Some("Meter"))?.id?)?;
    let mask = movie.sprite(meter.shown(1).into_iter().find(|p| p.name.as_deref() == Some("Mask"))?.id?)?;
    let shape = mask.shown(1).into_iter().find_map(|p| match movie.characters.get(&p.id?) {
        Some(Character::Shape(s)) => Some(s.bounds),
        _ => None,
    })?;
    Some((shape[2] - shape[0], shape[3] - shape[1]))
}

fn draw(movie: &Movie, spec: &Spec, pose: &Pose, clock: f32) -> Canvas {
    let rect = spec.meter.stage_rect();
    let mut canvas = Canvas::new(((rect[2] - rect[0]) * SCALE).ceil() as usize, ((rect[3] - rect[1]) * SCALE).ceil() as usize);
    let to = Affine::scale_then_move(SCALE, (spec.origin.0 - rect[0]) * SCALE, (spec.origin.1 - rect[1]) * SCALE);
    movie.draw(movie.exports[spec.export], &to, &Tint::NONE, pose, "", clock, &mut canvas);
    canvas
}

/// The frames the scripts play: the health bar's `Meter` on 1 and `low_loop` (2 to 32); a
/// cooldown's animation on 1 and from `ready` or `activate` to its end (`tf2_core::hud`).
fn frames(movie: &Movie, spec: &Spec) -> Vec<usize> {
    let count = |path: &str| -> Option<usize> {
        let root = movie.sprite(movie.exports[spec.export])?;
        let main = root.label("main").unwrap_or(1);
        let animation = root.shown(main).into_iter().find(|p| p.name.as_deref() == Some(spec.animation))?;
        let clip = movie.sprite(animation.id?)?;
        if path.is_empty() {
            return Some(clip.frames.len());
        }
        let meter = clip.shown(1).into_iter().find(|p| p.name.as_deref() == Some(path))?;
        Some(movie.sprite(meter.id?)?.frames.len())
    };
    if spec.across { (1..=32.min(count("Meter").unwrap_or(1))).collect() } else { (1..=count("").unwrap_or(1)).collect() }
}

/// A cut meter at `value` laid back together, as a port draws it: each pixel from the full
/// picture left of the edge, else from the empty one, and the sheen over. Colours times alpha.
fn laid_cut(full: &Canvas, empty: &Canvas, sheen: Option<&Canvas>, edge: f32) -> Vec<[f32; 4]> {
    let mut out = Vec::with_capacity(full.pixels.len());
    for y in 0..full.height {
        for x in 0..full.width {
            let k = y * full.width + x;
            let p = if (x as f32 + 0.5) < edge { full.pixels[k] } else { empty.pixels[k] };
            out.push(match sheen {
                Some(s) => over(s.pixels[k], p),
                None => p,
            });
        }
    }
    out
}

/// A sliding meter laid back together: what is under the bar, the bar at its step, and what is
/// over it.
fn laid_slide(below: &Canvas, fill: &Canvas, above: &Canvas) -> Vec<[f32; 4]> {
    below.pixels.iter().zip(&fill.pixels).zip(&above.pixels).map(|((b, f), a)| over(*a, over(*f, *b))).collect()
}

/// The largest difference (0 to 255) and the share of pixels off by more than 2.
fn compare(a: &[[f32; 4]], b: &[[f32; 4]]) -> (f32, f32) {
    let (mut worst, mut off) = (0.0f32, 0usize);
    for (p, q) in a.iter().zip(b) {
        let d = (0..4).map(|i| (p[i] - q[i]).abs() * 255.0).fold(0.0, f32::max);
        worst = worst.max(d);
        if d > 2.0 {
            off += 1;
        }
    }
    (worst, off as f32 / a.len().max(1) as f32)
}

fn first_bitmap(movie: &Movie, id: u16, visited: &mut HashSet<u16>) -> Option<u16> {
    if !visited.insert(id) {
        return None;
    }
    match movie.characters.get(&id)? {
        Character::Shape(shape) => shape.fills.iter().find_map(|fill| match fill {
            Fill::Bitmap { id, .. } if *id != u16::MAX => Some(*id),
            _ => None,
        }),
        Character::Sprite(sprite) => sprite.shown(1).into_iter().filter_map(|p| p.id).find_map(|child| first_bitmap(movie, child, visited)),
        _ => None,
    }
}

/// Writes the pictures and `hud.json` into `out`; gives the JSON.
pub fn export(install: &Path, out: &Path) -> Result<Value, String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let flash = install.join("UI").join("Flash");
    let mut pictures = Pictures { out, seen: HashMap::new() };

    // The reticle's textures, found in bnxglobal.str by the hash of their path.
    let global = pack::Pack::open(&install.join("bnxglobal.str"))?;
    let mut found = HashMap::new();
    for chunk in global.of_type(pack::TEXTURE) {
        if let Ok(Some(t)) = texture::parse(global.data(chunk)) {
            found.insert(t.id, t);
        }
    }
    let mut reticle_out = serde_json::Map::new();
    for which in reticle::Texture::ALL {
        let Some(t) = found.get(&which.id()) else { continue };
        let Some(rgba) = crate::texture::rgba(t) else { continue };
        let file = format!("{}.png", which.file());
        image::save_buffer(out.join(&file), &rgba, t.width, t.height, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
        reticle_out.insert(which.file().to_owned(), json!({ "file": file, "size": [t.width, t.height] }));
    }

    // The meters' icons: the special's (by `special.icon`) and the turbo's, from
    // globalassets.gfx's exports and the bitmaps of globalassets.apk.
    let mut icons = serde_json::Map::new();
    if let (Ok(gfx), Ok(apk)) = (std::fs::read(flash.join("globalassets.gfx")), std::fs::read(flash.join("globalassets.apk"))) {
        let movie = Movie::parse(&gfx)?;
        let library: HashMap<String, texture::Texture> = texture::parse_library(&apk)?.into_iter().map(|t| (t.name.to_ascii_lowercase(), t)).collect();
        let names = (0..5).filter_map(tf2_core::hud::ability_icon_export).chain(tf2_core::hud::turbo_icon_export(12));
        for name in names {
            let Some(bitmap) = movie.exports.get(name).and_then(|id| first_bitmap(&movie, *id, &mut HashSet::new())) else { continue };
            // The APK names its bitmaps by a lowercase hexadecimal id.
            let Some(t) = library.get(&format!("globalassets_i{bitmap:x}")) else { continue };
            let Some(rgba) = crate::texture::rgba(t) else { continue };
            let file = format!("icon_{}.png", name.to_ascii_lowercase());
            image::save_buffer(out.join(&file), &rgba, t.width, t.height, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
            icons.insert(name.to_owned(), json!({ "file": file, "size": [t.width, t.height] }));
        }
    }

    // The meters.
    let movie = Movie::parse(&std::fs::read(flash.join("hud.gfx")).map_err(|e| format!("hud.gfx: {e}"))?)?;
    let mut meters = serde_json::Map::new();
    for spec in &SPECS {
        let Some(mask) = mask_size(&movie, spec) else { continue };
        let size = if spec.across { mask.0 } else { mask.1 };
        let rect = spec.meter.stage_rect();
        let mut by_frame = Vec::new();
        let mut canvases = HashMap::new();
        let root = movie.exports[spec.export];
        let meter_path = format!("{}/Meter", spec.animation);
        for frame in frames(&movie, spec) {
            let (clip_frame, bar_frame) = if spec.across { (1, frame) } else { (frame, 1) };
            let mut row = serde_json::Map::new();
            // A bar sliding along x (the health bar, a rectangle over a bar that is not one
            // colour) is cut: the meter full and empty. One sliding along y (the cooldowns: their
            // masks have slanted tops, and the ability's sits in a hexagon's mask) is drawn under
            // and over its bar, a picture per frame, with the bar between (`fills`).
            let layers: Vec<(&str, Option<Part>, f32)> = if spec.across {
                vec![("full", None, 1.0), ("empty", None, 0.0)]
            } else {
                vec![("below", Some(Part::Below), 1.0), ("above", Some(Part::Above), 1.0)]
            };
            for (key, part, value) in layers {
                let mut p = pose(&movie, spec, clip_frame, bar_frame, value, size);
                if let Some(sheen) = spec.sheen {
                    p.insert(sheen.to_owned(), Tweak { hidden: true, ..Default::default() });
                }
                if let Some(part) = part {
                    split(&movie, root, &mut p, &meter_path, part);
                }
                let canvas = draw(&movie, spec, &p, 0.0);
                let file = pictures.put(&format!("hud_{}", spec.name), canvas.width, canvas.height, canvas.rgba8())?;
                canvases.insert((frame, key), canvas);
                row.insert(key.to_owned(), json!(file));
            }
            let b = bar(&movie, spec, clip_frame, mask).unwrap_or(rect);
            row.insert("bar".to_owned(), json!(b));
            by_frame.push(Value::Object(row));
        }
        // A sliding bar alone, a picture per step of a stage pixel along its slide: step k of n
        // is the value k / n. [port design]
        let mut fills = Vec::new();
        let mut fill_canvases = Vec::new();
        if !spec.across {
            let steps = size.round().max(1.0) as usize;
            for k in 0..=steps {
                let mut p = pose(&movie, spec, 1, 1, k as f32 / steps as f32, size);
                split(&movie, root, &mut p, &meter_path, Part::Only);
                let canvas = draw(&movie, spec, &p, 0.0);
                fills.push(json!(pictures.put(&format!("hud_{}_fill", spec.name), canvas.width, canvas.height, canvas.rgba8())?));
                fill_canvases.push(canvas);
            }
        }
        // The sheen alone, a picture per tick of its loop.
        let mut sheen_out = Value::Null;
        let mut sheens = Vec::new();
        if let Some(path) = spec.sheen {
            let mut p = pose(&movie, spec, 1, 1, 1.0, size);
            split(&movie, root, &mut p, path, Part::Only);
            // Its loop: the fewest ticks after which the pictures of 20 s come round again.
            const SPAN: usize = 600;
            let keys: Vec<Vec<u8>> = (0..SPAN).map(|t| draw(&movie, spec, &p, (t as f32 + 0.5) / RATE).rgba8()).collect();
            let length = (1..SPAN / 2).find(|n| (0..SPAN - n).all(|t| keys[t] == keys[t + n])).unwrap_or(SPAN);
            let mut ticks = Vec::new();
            for tick in 0..length {
                let canvas = draw(&movie, spec, &p, (tick as f32 + 0.5) / RATE);
                ticks.push(json!(pictures.put(&format!("hud_{}_sheen", spec.name), canvas.width, canvas.height, canvas.rgba8())?));
                sheens.push(canvas);
            }
            sheen_out = json!({ "rate": RATE, "ticks": ticks });
        }
        // Laid back together at a few values, frames and ticks, against the movie drawn whole.
        let mut worst = (0.0f32, 0.0f32);
        let list = frames(&movie, spec);
        for (n, value) in [0.0, 0.13, 0.5, 0.77, 1.0].into_iter().enumerate() {
            // A sliding bar is drawn at its steps: the check takes the nearest.
            let value = if spec.across { value } else { (value * (fill_canvases.len() - 1) as f32).round() / (fill_canvases.len() - 1) as f32 };
            let frame = list[(n * 7) % list.len()];
            let (clip_frame, bar_frame) = if spec.across { (1, frame) } else { (frame, 1) };
            let tick = n * 37 % sheens.len().max(1);
            let whole = draw(&movie, spec, &pose(&movie, spec, clip_frame, bar_frame, value, size), (tick as f32 + 0.5) / RATE);
            let b = bar(&movie, spec, clip_frame, mask).unwrap_or(rect);
            let got = if spec.across {
                let edge = (b[0] + value * (b[2] - b[0]) - rect[0]) * SCALE;
                laid_cut(&canvases[&(frame, "full")], &canvases[&(frame, "empty")], sheens.get(tick), edge)
            } else {
                let step = (value * (fill_canvases.len() - 1) as f32).round() as usize;
                laid_slide(&canvases[&(frame, "below")], &fill_canvases[step], &canvases[&(frame, "above")])
            };
            let (a, s) = compare(&got, &whole.pixels);
            // ROTF_HUD_DEBUG keeps both for a look.
            if std::env::var_os("ROTF_HUD_DEBUG").is_some() {
                let straight = |p: &[[f32; 4]]| -> Vec<u8> {
                    p.iter().flat_map(|q| {
                        let a = q[3];
                        [0, 1, 2].map(|i| if a > 0.0 { (q[i] / a * 255.0).round() as u8 } else { 0 }).into_iter().chain([(a * 255.0).round() as u8])
                    }).collect()
                };
                for (tag, p) in [("whole", &whole.pixels), ("laid", &got)] {
                    image::save_buffer(out.join(format!("debug_{}_{n}_{tag}.png", spec.name)), &straight(p), whole.width as u32, whole.height as u32, image::ExtendedColorType::Rgba8).ok();
                }
                println!("{} value {value} frame {frame} tick {tick}: bar {b:?}: worst {a} share {s}", spec.name);
            }
            worst = (worst.0.max(a), worst.1.max(s));
        }
        meters.insert(spec.name.to_owned(), json!({
            "rect": rect,
            "across": spec.across,
            "frames": by_frame,
            "fills": fills,
            "sheen": sheen_out,
            "check": { "worst": worst.0, "share_off": worst.1 },
        }));
    }
    let doc = json!({
        "scale": SCALE,
        "picture": [1280, 720],
        "reticle": reticle_out,
        "icons": icons,
        "meters": meters,
    });
    std::fs::write(out.join("hud.json"), serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(doc)
}
