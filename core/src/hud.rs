//! The game's HUD (`UI\Flash\hud.gfx`): the part of its ActionScript classes that the
//! three meters of a single-player character need, played over `formats::gfx`. Read from
//! the movie's own scripts (`work\gfx_dump.py <file> code <id>`). [game] where the scripts
//! say it; what the executable sends them is in `notes\hud.md` "The binding".
//!
//! - `core.BaseMeter` (health): the value, 1 above 0.9999 and 0 under 0.00001, slides
//!   `Mask` to `_x = -w + value * w`, w the mask's width. `hud2.common.HealthMeter`: under
//!   `low_threshold` 0.4 the bar plays `low_loop`; else it stands on frame 1.
//! - `core.CoolDownBaseMeter` (turbo, ability): `Mask._y = h - value * h`, h the mask's
//!   height (`orientationModifier` -1). Reaching 1 plays `activate`, coming back to 0
//!   plays `ready`, on the meter's animation clip.
//!
//! Not ported here: the button prompt on the ability, text, the radar, the counters, the
//! messages, and each element's `in` and `out` animations (they stand on `main`).

use crate::formats::gfx::{Affine, Canvas, Movie, Pose, Tint, Tweak};

/// Names in `characterselect2.gfx`'s shared `IconList`, indexed by
/// `specialAbilityIcon` in a character's attributes. [game]
pub fn ability_icon_export(index: u32) -> Option<&'static str> {
    ["Ability_Damage", "Ability_Defense", "Ability_Heal", "Ability_Projectile", "Ability_Turret"]
        .get(index as usize)
        .copied()
}

/// `FUN_00716890` sends icon 12 for Bumblebee's turbo. [game]
pub fn turbo_icon_export(index: u32) -> Option<&'static str> {
    (index == 12).then_some("SC_Turbo_Icon_1")
}

/// The three meters, by the names the movie exports them under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Meter {
    Health,
    Turbo,
    Ability,
}

impl Meter {
    pub const ALL: [Meter; 3] = [Meter::Health, Meter::Turbo, Meter::Ability];

    fn export(self) -> &'static str {
        match self {
            Meter::Health => "Health_Meter Class",
            Meter::Turbo => "Turbo_Meter Class",
            Meter::Ability => "Ability_Meter Class",
        }
    }

    /// Where `Hud_SP` puts the clip on the 1280 x 720 stage. [data]
    fn origin(self) -> (f32, f32) {
        match self {
            Meter::Health | Meter::Turbo => (639.2, 359.1),
            Meter::Ability => (122.2, 600.0),
        }
    }

    /// The part of the stage it is drawn in: left, top, right, bottom, a little more than
    /// its shapes' bounds.
    pub fn stage_rect(self) -> [f32; 4] {
        match self {
            Meter::Health => [150.0, 580.0, 274.0, 620.0],
            Meter::Turbo => [120.0, 433.0, 165.0, 478.0],
            Meter::Ability => [90.0, 564.0, 154.0, 636.0],
        }
    }

    /// The clip whose `Meter` holds the mask, and the mask's size along the way it slides.
    fn animation(self) -> (&'static str, f32) {
        match self {
            Meter::Health => ("Meter_Animation", 111.0),
            Meter::Turbo => ("Turbo_Meter_Animation", 31.0),
            Meter::Ability => ("Ability_Meter_Animation", 57.2),
        }
    }
}

/// The movie's frames a second.
const RATE: f32 = 30.0;
/// `low_loop` in the health bar's `Meter`: frames 2 to 32, then back.
const LOW_LOOP: (f32, f32) = (2.0, 32.0);

/// `BaseMeter`'s and `CoolDownBaseMeter`'s rounding of a value.
pub fn rounded(value: f32) -> f32 {
    if value > 0.9999 {
        1.0
    } else if value < 0.00001 {
        0.0
    } else {
        value
    }
}

/// A cooldown meter's state: its value and the frame its animation clip is playing.
#[derive(Clone, Copy, Debug, Default)]
struct Cooldown {
    value: f32,
    playing: Option<f32>,
}

impl Cooldown {
    fn step(&mut self, value: f32, labels: (f32, f32), frames: f32, dt: f32) {
        let value = rounded(value);
        if let Some(frame) = &mut self.playing {
            *frame += dt * RATE;
            if *frame >= frames + 1.0 {
                self.playing = None;
            }
        }
        // [simplified: the script tells the two apart by its `_lastmeter`]
        if value >= 1.0 && self.value < 1.0 {
            self.playing = Some(labels.1);
        } else if value == 0.0 && self.value > 0.0 {
            self.playing = Some(labels.0);
        }
        self.value = value;
    }
}

/// What the three meters show.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hud {
    health: f32,
    low_frame: Option<f32>,
    turbo: Cooldown,
    ability: Cooldown,
    clock: f32,
}

impl Hud {
    /// One frame. `health` is his health's share; `turbo` and `ability` are 1 as the
    /// thing is used, falling to 0 as it becomes ready again.
    pub fn step(&mut self, movie: &Movie, health: f32, turbo: f32, ability: f32, dt: f32) {
        self.clock += dt;
        self.health = rounded(health);
        // `onMeterChange`: low under the threshold; `onMeterFull` and the rest: normal.
        self.low_frame = if self.health < 0.4 {
            let frame = self.low_frame.map_or(LOW_LOOP.0, |frame| frame + dt * RATE);
            Some(if frame >= LOW_LOOP.1 + 1.0 { LOW_LOOP.0 + (frame - LOW_LOOP.1 - 1.0) } else { frame })
        } else {
            None
        };
        for (meter, state, value) in [(Meter::Turbo, &mut self.turbo, turbo), (Meter::Ability, &mut self.ability, ability)] {
            let clip = Self::clip(movie, meter);
            let labels = clip.map_or((2.0, 2.0), |c| (c.label("ready").unwrap_or(2) as f32, c.label("activate").unwrap_or(2) as f32));
            state.step(value, labels, clip.map_or(1.0, |c| c.frames.len() as f32), dt);
        }
    }

    fn clip(movie: &Movie, meter: Meter) -> Option<&crate::formats::gfx::Sprite> {
        let root = movie.sprite(*movie.exports.get(meter.export())?)?;
        let main = root.label("main")?;
        let id = root.shown(main).into_iter().find(|p| p.name.as_deref() == Some(meter.animation().0))?.id?;
        movie.sprite(id)
    }

    /// What the scripts have set on a meter's clips.
    pub fn pose(&self, movie: &Movie, meter: Meter) -> Pose {
        let mut pose = Pose::new();
        let root = movie.exports.get(meter.export()).and_then(|id| movie.sprite(*id));
        let main = root.and_then(|sprite| sprite.label("main")).unwrap_or(1);
        pose.insert(String::new(), Tweak { frame: Some(main), ..Default::default() });
        let (animation, size) = meter.animation();
        let (clip_frame, bar_frame, mask) = match meter {
            Meter::Health => {
                let x = -size + self.health * size;
                (1, self.low_frame.map_or(1, |f| f as usize), Tweak { x: Some(x), ..Default::default() })
            }
            Meter::Turbo | Meter::Ability => {
                let state = if meter == Meter::Turbo { &self.turbo } else { &self.ability };
                let y = size - state.value * size;
                (state.playing.map_or(1, |f| f as usize), 1, Tweak { y: Some(y), ..Default::default() })
            }
        };
        pose.insert(animation.to_owned(), Tweak { frame: Some(clip_frame), ..Default::default() });
        pose.insert(format!("{animation}/Meter"), Tweak { frame: Some(bar_frame), ..Default::default() });
        pose.insert(format!("{animation}/Meter/Mask"), mask);
        pose
    }

    /// What decides a meter's picture: it need not be drawn again while this is the same.
    pub fn key(&self, meter: Meter) -> [i32; 3] {
        let tick = (self.clock * RATE) as i32;
        match meter {
            // Its sheen plays by itself.
            Meter::Health => [(self.health * 1024.0) as i32, self.low_frame.map_or(0, |f| f as i32), tick],
            Meter::Turbo => [(self.turbo.value * 1024.0) as i32, self.turbo.playing.map_or(0, |f| f as i32), 0],
            Meter::Ability => [(self.ability.value * 1024.0) as i32, self.ability.playing.map_or(0, |f| f as i32), 0],
        }
    }

    /// A meter's picture at `scale` pixels to a stage pixel, covering `Meter::stage_rect`.
    pub fn draw(&self, movie: &Movie, meter: Meter, scale: f32) -> Option<Canvas> {
        let id = *movie.exports.get(meter.export())?;
        let rect = meter.stage_rect();
        let mut canvas = Canvas::new(((rect[2] - rect[0]) * scale).ceil() as usize, ((rect[3] - rect[1]) * scale).ceil() as usize);
        let origin = meter.origin();
        let to_canvas = Affine::scale_then_move(scale, (origin.0 - rect[0]) * scale, (origin.1 - rect[1]) * scale);
        movie.draw(id, &to_canvas, &Tint::NONE, &self.pose(movie, meter), "", self.clock, &mut canvas);
        Some(canvas)
    }
}

/// The export of the objective meter's class (`hud2.common.ObjectiveMeter`, sprite 285). [data]
const OBJECTIVE_EXPORT: &str = "Obj_Meter Class";
/// Where `Hud_SP` places `objective_meter_a` on the 1280 x 720 stage. [data] (gfx_dump: `objective_meter_a at -469.6 180.6`)
pub const OBJECTIVE_ORIGIN: (f32, f32) = (-469.6, 180.6);
/// The part of the stage the objective meter is drawn in: its shapes' bounds, a little more. [data]
pub const OBJECTIVE_RECT: [f32; 4] = [95.0, 230.0, 280.0, 300.0];
/// The width the bar's Mask moves over: the Mask's shape is 130.5 wide, less `meterOffset` 10 (its height). [data] [assumed]
/// [game hud.gfx sprite 472 at 0x162] The BaseMeter `meter` setter reads `barFactor` for the Mask's width and x: with it
/// undefined the width is left alone and the x move is computed from undefined (NaN in Flash), so the game's bar does not
/// visibly move by this rule. The objective meter sets no `barFactor` (sprite 505 has none; `HealthMeter`, sprite 473, does),
/// so the 120.5 below is NOT read from the game: it is kept as the health bar's rule. What Flash does with a NaN `_x` is not
/// read. settles: a recording of the convoy bar at two values (does the Mask move, and by how much).
const OBJECTIVE_BAR: f32 = 120.5;
/// `low_loop` in the objective meter's `Meter_Animation`: frames 2 to 10 (frame 10 goes back to the label). [data]
const OBJECTIVE_LOW: (f32, f32) = (2.0, 10.0);

/// The objective meter `objective_meter_a`, the convoy's health in the escort. [game] `hud2.common.ObjectiveMeter` is a
/// `core.BaseMeter`: its `meterOffset` is the Mask's height (sprite 505 at 0x280), and its `meter` setter moves the Mask
/// by `barFactor` (sprite 472 at 0x191). Shown while the value is above 0 and the script shows it (`HUD_onEnabled`,
/// sprite 505 at 0x34d7c). Under 0.4 `Meter_Animation` plays `low_loop` (`updateAnimationState`).
/// NOT drawn: the icon (`objective_meter_a_icon`; the frame the game sends is [unknown], and the `IconList` of the
/// icons is not read), the `in` and `out` animations, and the text (`label_txt`, the movie's text fields are not read).
#[derive(Clone, Copy, Debug, Default)]
pub struct Objective {
    value: f32,
    shown: bool,
    low_frame: Option<f32>,
}

impl Objective {
    /// One frame. `bar` is the convoy's health view's HUD bar: its fraction while the script shows it, None while it is
    /// hidden or the mission has ended. [stand-in] the show and hide follow the runner's HealthBar events, not the
    /// game's `setEnabled` path (`uiroot.lxb`, `_global.hud_objective_meter_a.setEnabled`). OPEN (mission path, not a
    /// robot's): settles: the `setEnabled` callers for `hud_objective_meter_a` in `uiroot.lxb` (the binding that
    /// calls sprite 505's HUD_onEnabled), read with `tools\tf2_lxb.py`.
    pub fn step(&mut self, bar: Option<f32>, dt: f32) {
        self.value = rounded(bar.unwrap_or(0.0));
        // [game hud.gfx sprite 505, HUD_onEnabled at 0x34d84] a meter at 0 is hidden: the handler shows the meter only when
        // getEnabled() is true and `meter` is not 0.
        self.shown = bar.is_some() && self.value > 0.0;
        self.low_frame = if self.shown && self.value < 0.4 {
            let frame = self.low_frame.map_or(OBJECTIVE_LOW.0, |frame| frame + dt * RATE);
            Some(if frame >= OBJECTIVE_LOW.1 + 1.0 { OBJECTIVE_LOW.0 + (frame - OBJECTIVE_LOW.1 - 1.0) } else { frame })
        } else {
            None
        };
    }

    /// Whether the meter is drawn this frame.
    pub fn is_shown(&self) -> bool {
        self.shown
    }

    /// The Mask's x in the meter: `-barFactor + value * barFactor` (the setter's sum). [game] sprite 472 at 0x191.
    fn mask_x(&self) -> f32 {
        -OBJECTIVE_BAR + self.value * OBJECTIVE_BAR
    }

    /// What the scripts have set on the meter's clips.
    pub fn pose(&self, movie: &Movie) -> Pose {
        let mut pose = Pose::new();
        let root = movie.exports.get(OBJECTIVE_EXPORT).and_then(|id| movie.sprite(*id));
        let main = root.and_then(|sprite| sprite.label("main")).unwrap_or(1);
        pose.insert(String::new(), Tweak { frame: Some(main), ..Default::default() });
        pose.insert("Meter_Animation".to_owned(), Tweak { frame: Some(self.low_frame.map_or(1, |f| f as usize)), ..Default::default() });
        pose.insert("Meter_Animation/Meter".to_owned(), Tweak { frame: Some(1), ..Default::default() });
        pose.insert("Meter_Animation/Meter/Mask".to_owned(), Tweak { x: Some(self.mask_x()), ..Default::default() });
        pose
    }

    /// What decides the picture: the value and the low sheen's frame.
    pub fn key(&self) -> [i32; 2] {
        [(self.value * 1024.0) as i32, self.low_frame.map_or(0, |f| (f * 16.0) as i32)]
    }

    /// The meter's picture at `scale` pixels to a stage pixel, covering `OBJECTIVE_RECT`.
    pub fn draw(&self, movie: &Movie, scale: f32) -> Option<Canvas> {
        let id = *movie.exports.get(OBJECTIVE_EXPORT)?;
        let rect = OBJECTIVE_RECT;
        let mut canvas = Canvas::new(((rect[2] - rect[0]) * scale).ceil() as usize, ((rect[3] - rect[1]) * scale).ceil() as usize);
        let to_canvas = Affine::scale_then_move(scale, (OBJECTIVE_ORIGIN.0 - rect[0]) * scale, (OBJECTIVE_ORIGIN.1 - rect[1]) * scale);
        movie.draw(id, &to_canvas, &Tint::NONE, &self.pose(movie), "", 0.0, &mut canvas);
        Some(canvas)
    }
}

/// The time as the HUD writes it: `MM:SS.hh`; "60:00.00" from an hour on, "--:--.--" below 0. [game] the movie's own
/// `_global.ConvertToTF2TimerFormat` (hud.gfx, the root timeline's script at 0x1c264): minutes = floor(t / 60),
/// seconds = floor(t) - 60 minutes, hundredths = floor((t - floor t) * 100), each below 10 with a leading "0".
/// Its argument is in SECONDS (the hour test divides by 3600), which backs the timer's seconds reading.
pub fn timer_text(seconds: f32) -> String {
    if seconds / 3600.0 >= 1.0 {
        return "60:00.00".into();
    }
    if seconds < 0.0 {
        return "--:--.--".into();
    }
    let minutes = (seconds / 60.0).floor();
    let whole = seconds.floor() - minutes * 60.0;
    let hundredths = ((seconds - seconds.floor()) * 100.0).floor();
    format!("{:02}:{:02}.{:02}", minutes as u32, whole as u32, hundredths as u32)
}

/// The export of the mission timer's clip (`hud2.common.Timer`, sprite 134). [data]
const TIMER_EXPORT: &str = "Timer Class";
/// Where `Hud_SP` places `Timer` on the stage. [data] (gfx_dump: `Timer at 639.2 359.1`)
pub const TIMER_ORIGIN: (f32, f32) = (639.2, 359.1);
/// The part of the stage the timer is drawn in: its shapes' bounds (x 92 to 301, y 50 to 142), a little more. [data]
pub const TIMER_RECT: [f32; 4] = [85.0, 40.0, 310.0, 150.0];

/// The mission timer `Timer`: the clock and its frame, its text the timer's seconds as `timer_text` writes them.
/// [game] `hud2.common.Timer` (sprite 475): its `time` setter writes `ConvertToTF2TimerFormat(time)` into
/// `Timer_Text.label_txt`. [stand-in] shown on its `main` frame while the script's timer has started and the mission has
/// not ended (the game's enable path, `HUD_onEnabled` and the `gdo.timer` binding, is not read; the `in` and `out`
/// animations are not played); the text's shadow (`setShadow`) is not drawn. OPEN (mission path): sprite 475's
/// `HUD_onEnabled` (hud.gfx at 0x2a515) only re-enables the clip and traces "hud enabled"; what shows it is the
/// `gdo.timer` binding, not read. settles: the `gdo.timer` binding in `uiroot.lxb`, and the `main` and `in` frames.
#[derive(Clone, Debug, Default)]
pub struct MissionTimer {
    text: Option<String>,
}

impl MissionTimer {
    /// One frame: the timer's seconds while it is shown, None while it is not.
    pub fn step(&mut self, seconds: Option<f32>) {
        self.text = seconds.map(timer_text);
    }

    pub fn is_shown(&self) -> bool {
        self.text.is_some()
    }

    /// What decides the picture: the text.
    pub fn key(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn pose(&self, movie: &Movie) -> Pose {
        let mut pose = Pose::new();
        let main = movie.exports.get(TIMER_EXPORT).and_then(|id| movie.sprite(*id)).and_then(|s| s.label("main")).unwrap_or(1);
        pose.insert(String::new(), Tweak { frame: Some(main), ..Default::default() });
        pose.insert("Timer_Text/label_txt".to_owned(), Tweak { text: self.text.clone(), ..Default::default() });
        pose
    }

    /// The timer's picture at `scale` pixels to a stage pixel, covering `TIMER_RECT`. The movie must have its fonts
    /// (`Movie::import_from` font.gfx, after `glyphs_from` font_glyphs.gfx).
    pub fn draw(&self, movie: &Movie, scale: f32) -> Option<Canvas> {
        let id = *movie.exports.get(TIMER_EXPORT)?;
        let rect = TIMER_RECT;
        let mut canvas = Canvas::new(((rect[2] - rect[0]) * scale).ceil() as usize, ((rect[3] - rect[1]) * scale).ceil() as usize);
        let to_canvas = Affine::scale_then_move(scale, (TIMER_ORIGIN.0 - rect[0]) * scale, (TIMER_ORIGIN.1 - rect[1]) * scale);
        movie.draw(id, &to_canvas, &Tint::NONE, &self.pose(movie), "", 0.0, &mut canvas);
        Some(canvas)
    }
}

/// Loads the HUD's two fonts into `movie` (hud.gfx) from the folder they sit in: `font.gfx` names them and the HUD
/// imports them from it; their outlines are in `font_glyphs.gfx` under the same names. Returns the fonts taken.
pub fn load_fonts(movie: &mut Movie, flash_dir: &std::path::Path) -> Result<usize, String> {
    let read = |name: &str| -> Result<Movie, String> {
        let file = std::fs::read(flash_dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
        Movie::parse(&file).map_err(|e| format!("{name}: {e}"))
    };
    let mut fonts = read("font.gfx")?;
    fonts.glyphs_from(&read("font_glyphs.gfx")?);
    Ok(movie.import_from("font.gfx", &fonts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn movie() -> Option<Movie> {
        // Needs the install.
        Movie::parse(&std::fs::read(crate::install::dir().join("UI/Flash/hud.gfx")).ok()?).ok()
    }

    /// The colour the bars are filled with.
    const BAR: [f32; 3] = [187.0 / 255.0, 90.0 / 255.0, 99.0 / 255.0];

    fn is_bar(pixel: [f32; 4]) -> bool {
        pixel[3] > 0.9 && (0..3).all(|i| (pixel[i] / pixel[3] - BAR[i]).abs() < 0.2)
    }

    #[test]
    fn the_timer_writes_minutes_seconds_and_hundredths_as_the_movie_does() {
        assert_eq!(timer_text(0.0), "00:00.00");
        assert_eq!(timer_text(83.456), "01:23.45");
        assert_eq!(timer_text(599.999), "09:59.99");
        assert_eq!(timer_text(3599.5), "59:59.50");
        assert_eq!(timer_text(3600.0), "60:00.00");
        assert_eq!(timer_text(-0.1), "--:--.--");
    }

    #[test]
    fn the_timer_draws_its_time_in_the_huds_font() {
        let Some(mut movie) = movie() else { return };
        assert_eq!(load_fonts(&mut movie, crate::install::dir().join("UI/Flash").as_path()), Ok(2));
        let mut timer = MissionTimer::default();
        timer.step(None);
        assert!(!timer.is_shown());
        let ink = |timer: &MissionTimer| timer.draw(&movie, 1.0).unwrap().pixels.iter().filter(|p| p[3] > 0.5).count();
        timer.step(Some(83.456));
        assert_eq!(timer.key(), Some("01:23.45"));
        let with_time = ink(&timer);
        timer.step(Some(0.0));
        let at_zero = ink(&timer);
        // "01:23.45" and "00:00.00" have the same glyph count but different ink; both draw text and the frame.
        assert!(with_time > 500 && at_zero > 500 && with_time != at_zero, "{with_time} {at_zero}");
    }

    #[test]
    fn values_round_as_the_scripts_round_them() {
        assert_eq!((rounded(0.99995), rounded(0.000001), rounded(0.5)), (1.0, 0.0, 0.5));
    }

    #[test]
    fn the_health_bar_is_as_long_as_the_health() {
        let Some(movie) = movie() else { return };
        let mut hud = Hud::default();
        hud.step(&movie, 0.5, 0.0, 0.0, 0.0);
        let canvas = hud.draw(&movie, Meter::Health, 1.0).unwrap();
        // The bar runs from x = 156.7 to 267.7 on the stage, y = 588.5 to 611.5: at half
        // health its left half is filled and its right half is not.
        let at = |x: f32, y: f32| canvas.pixels[(y - 580.0) as usize * canvas.width + (x - 150.0) as usize];
        assert!(is_bar(at(180.0, 600.0)), "{:?}", at(180.0, 600.0));
        assert!(!is_bar(at(240.0, 600.0)), "{:?}", at(240.0, 600.0));
        assert_eq!(hud.pose(&movie, Meter::Health)["Meter_Animation/Meter/Mask"].x, Some(-55.5));
        // Under 0.4 the bar plays its loop, frames 2 to 32, and goes back to 1 above it.
        hud.step(&movie, 0.3, 0.0, 0.0, 0.0);
        assert_eq!(hud.pose(&movie, Meter::Health)["Meter_Animation/Meter"].frame, Some(2));
        hud.step(&movie, 0.3, 0.0, 0.0, 1.0);
        assert_eq!(hud.pose(&movie, Meter::Health)["Meter_Animation/Meter"].frame, Some(32));
        hud.step(&movie, 0.3, 0.0, 0.0, 0.1);
        assert_eq!(hud.pose(&movie, Meter::Health)["Meter_Animation/Meter"].frame, Some(4));
        hud.step(&movie, 0.9, 0.0, 0.0, 0.1);
        assert_eq!(hud.pose(&movie, Meter::Health)["Meter_Animation/Meter"].frame, Some(1));
    }

    #[test]
    fn a_cooldown_meter_drains_and_plays_its_two_animations() {
        let Some(movie) = movie() else { return };
        let mut hud = Hud::default();
        hud.step(&movie, 1.0, 1.0, 0.0, 0.0);
        let pose = hud.pose(&movie, Meter::Turbo);
        // Used: the mask is over the whole fill, and `activate` (frame 8) starts.
        assert_eq!((pose["Turbo_Meter_Animation/Meter/Mask"].y, pose["Turbo_Meter_Animation"].frame), (Some(0.0), Some(8)));
        hud.step(&movie, 1.0, 0.5, 0.0, 0.1);
        let pose = hud.pose(&movie, Meter::Turbo);
        assert_eq!((pose["Turbo_Meter_Animation/Meter/Mask"].y, pose["Turbo_Meter_Animation"].frame), (Some(15.5), Some(11)));
        // Ready again: `ready` plays from frame 2, and in time the clip is back on 1.
        hud.step(&movie, 1.0, 0.0, 0.0, 0.1);
        assert_eq!(hud.pose(&movie, Meter::Turbo)["Turbo_Meter_Animation"].frame, Some(2));
        hud.step(&movie, 1.0, 0.0, 0.0, 4.0);
        assert_eq!(hud.pose(&movie, Meter::Turbo)["Turbo_Meter_Animation"].frame, Some(1));
        // Half way through, the lower half of its 31 x 31 fill shows (stage 127 to 158,
        // 440 to 471).
        hud.step(&movie, 1.0, 1.0, 0.0, 0.0);
        hud.step(&movie, 1.0, 0.5, 0.0, 10.0);
        let canvas = hud.draw(&movie, Meter::Turbo, 1.0).unwrap();
        let at = |x: f32, y: f32| canvas.pixels[(y - 433.0) as usize * canvas.width + (x - 120.0) as usize];
        assert!(is_bar(at(142.0, 465.0)), "{:?}", at(142.0, 465.0));
        assert!(!is_bar(at(142.0, 446.0)), "{:?}", at(142.0, 446.0));
        assert_ne!(hud.key(Meter::Turbo), Hud::default().key(Meter::Turbo));
    }

    #[test]
    fn the_objective_bar_fills_by_the_fraction_the_health_view_sends() {
        // 1150 of 2500 (150 damage at 9 a second over 10 s, `--script-hurt-convoy 150,3`): 0.46 of the bar.
        let mut objective = Objective::default();
        objective.step(Some(1150.0 / 2500.0), 0.0);
        assert!(objective.is_shown());
        assert!((objective.mask_x() - (-120.5 + 0.46 * 120.5)).abs() < 1e-3, "{}", objective.mask_x());
        objective.step(Some(1.0), 0.0);
        assert_eq!(objective.mask_x(), 0.0);
        // Hidden while the script hides it, and at zero.
        objective.step(None, 0.0);
        assert!(!objective.is_shown());
        objective.step(Some(0.0), 0.0);
        assert!(!objective.is_shown());
    }

    #[test]
    fn the_objective_bar_is_drawn_as_long_as_its_fraction() {
        let Some(movie) = movie() else { return };
        // The bar's row on the stage (y 252) and the stage x of its red fill: from about 148 to 265 when full.
        let red_at = |value: f32, x: f32| {
            let mut o = Objective::default();
            o.step(Some(value), 0.0);
            let canvas = o.draw(&movie, 1.0).unwrap();
            let p = canvas.pixels[(252.0 - OBJECTIVE_RECT[1]) as usize * canvas.width + (x - OBJECTIVE_RECT[0]) as usize];
            is_bar(p)
        };
        assert!(red_at(1.0, 250.0), "full bar");
        // 0.46 (1150 of 2500): the fill ends near 200, the track after it is not red.
        assert!(red_at(0.46, 170.0) && !red_at(0.46, 230.0));
    }

    #[test]
    fn the_objective_meter_loops_its_sheen_under_0_4_and_stops_above() {
        let mut objective = Objective::default();
        objective.step(Some(0.3), 0.0);
        assert_eq!(objective.low_frame, Some(2.0));
        objective.step(Some(0.3), 0.5);
        // 2 + 15 frames = 17: past frame 10, it goes back to frame 2 and plays on: frame 8.
        assert_eq!(objective.low_frame.map(|f| f.round()), Some(8.0));
        objective.step(Some(0.9), 0.1);
        assert_eq!(objective.low_frame, None);
    }

    #[test]
    fn the_ability_fills_its_hexagon() {
        let Some(movie) = movie() else { return };
        let mut hud = Hud::default();
        hud.step(&movie, 1.0, 0.0, 1.0, 0.0);
        hud.step(&movie, 1.0, 0.0, 1.0, 10.0);
        let canvas = hud.draw(&movie, Meter::Ability, 1.0).unwrap();
        // Its middle is at 122, 600 on the stage; the fill is 60% opaque over a dark hexagon.
        let middle = canvas.pixels[(600 - 564) * canvas.width + (122 - 90)];
        assert!(middle[0] / middle[3] > 0.4 && middle[3] > 0.6, "{middle:?}");
        let empty = Hud::default().draw(&movie, Meter::Ability, 1.0).unwrap();
        let middle = empty.pixels[(600 - 564) * empty.width + (122 - 90)];
        assert!(middle[0] / middle[3] < 0.1, "{middle:?}");
    }
}
