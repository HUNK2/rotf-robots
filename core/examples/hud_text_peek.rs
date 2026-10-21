//! Draws one of the HUD movie's clips with text set, to a PNG, to look at the fonts.
//!
//! cargo run -p tf2-core --example hud_text_peek -- <out.png> [export name] [instance path=text]...
//! Default: the timer ("Timer Class") reading 01:23.45. The clip is drawn as the HUD places it
//! (its origin at the stage centre), on a dark grey ground, at 1280 x 720.

use tf2_core::formats::gfx::{Affine, Canvas, Movie, Pose, Tint, Tweak};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.first().cloned().unwrap_or_else(|| r"C:\Bumblebee\work\shots\hud_text_peek.png".into());
    let export = args.get(1).cloned().unwrap_or_else(|| "Timer Class".into());
    let dir = std::path::Path::new(r"C:\Games2\UI\Flash");
    let read = |name: &str| Movie::parse(&std::fs::read(dir.join(name)).expect(name)).expect(name);
    let movie_file = std::env::var("PEEK_MOVIE").unwrap_or_else(|_| "hud.gfx".into());
    let mut hud = read(&movie_file);
    let mut fonts = read("font.gfx");
    let glyphs = read("font_glyphs.gfx");
    println!("fonts given outlines: {}", fonts.glyphs_from(&glyphs));
    println!("characters the HUD took from font.gfx: {}", hud.import_from("font.gfx", &fonts));
    let mut pose = Pose::new();
    let sets: Vec<&String> = args.iter().skip(2).collect();
    if sets.is_empty() && export == "Timer Class" {
        pose.insert("Timer_Text/label_txt".into(), Tweak { text: Some("01:23.45".into()), ..Default::default() });
    }
    for set in sets {
        // `path=text` sets a field's text; `path@frame` puts a clip on a frame (`last`: its last one).
        if let Some((path, frame)) = set.split_once('@') {
            let frame = if frame == "last" {
                let id = if path.is_empty() { hud.exports[&export] } else { 0 };
                hud.sprite(id).map_or(1, |s| s.frames.len())
            } else {
                frame.parse().expect("a frame number")
            };
            pose.entry(path.into()).or_default().frame = Some(frame);
            continue;
        }
        let (path, text) = set.split_once('=').expect("path=text");
        pose.entry(path.into()).or_default().text = Some(text.replace("\\n", "\n"));
    }
    let id = hud.exports[&export];
    let mut canvas = Canvas::new(1280, 720);
    for p in &mut canvas.pixels {
        *p = [0.12, 0.12, 0.12, 1.0];
    }
    // The HUD's clips are placed about the stage's centre; a screen movie's root clip at the corner (PEEK_ORIGIN=0).
    let centre = std::env::var("PEEK_ORIGIN").map_or(true, |v| v != "0");
    let (ox, oy) = if centre { (640.0, 360.0) } else { (0.0, 0.0) };
    hud.draw(id, &Affine::scale_then_move(1.0, ox, oy), &Tint::NONE, &pose, "", 0.0, &mut canvas);
    image::save_buffer(&out, &canvas.rgba8(), 1280, 720, image::ColorType::Rgba8).expect("png");
    println!("wrote {out}");
}
