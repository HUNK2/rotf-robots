//! Sniping: the Audi's PlasmaLongRifle and Breakaway's LongBeamRifle (`snipeCameraEnabled`),
//! with the character's `sniperCamera` (`audir8SniperCamera`, `firestormSniperCamera`).
//! `docs/weapon-mode.md` "Sniping" has the reading.
//!
//! - In and out (`character_update` 0071dc20, `FUN_00721ce0`): a human player in weapon mode
//!   (control mode 2) whose weapon in hand has `snipeCameraEnabled`, with a sniper camera,
//!   presses `CameraReset` (button 7, R3): sniping toggles. It ends by itself when the weapon
//!   in hand has no snipe camera, he leaves `StateWeapon`, or the game's blockers are up.
//!   Going in shows `SniperScope_Gfx` (Activate) and switches to the sniper camera; going out
//!   hides it and takes the camera behaviour off. [game]
//! - The camera (`zoom_camera_fov_update` 00929d50 over the weapon camera 00929470): the
//!   weapon camera's update with the sniper settings, and its field of view zoomed between
//!   `defaultFOV` and `zoomFOV` by `CameraDebugFollowCloser` / `Farther` (buttons 4 and 5,
//!   D-pad up and down), in `1/tan` steps that take `zoomFullTime` for the whole way. [game]
//! - The aim (`aim_stick_update` 0071b990): no aim assist, no fine-aim curve, the
//!   `aimSniping*` rates, scaled down toward `aimSnipingSensitivityMultiplierOnFullZoom` as
//!   the view zooms. [game]
//! - The shot (`weapon_tick_outline` 007ac370, `weapon_fire` 007ae010, 007ac920, 007ac760): a
//!   clock that runs while sniping and set to `-timeTilSnipeChargeStarts` by each shot; the
//!   steady is `(clock / snipeChargeDurationTilMaxEffect)^2` up to 1; the round's damage
//!   multiplier goes from 1 to `maxSnipeDamage / damage` with the steady; a full steady fires
//!   the STEADIED round (`weapons::fire_choice`). The round is traced from the camera and its
//!   barrels are the camera's matrix lowered by 1.5 (`007a7a40`, `007832b0`); no scatter. [game]

/// The snipe fields of a weapon's attributes (the gun loader `007bd0d0`; defaults `007bd4e0`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SnipeTuning {
    /// `snipeCameraEnabled` (attributes `+0x5f`, the weapon's slot `+0xa4` / `+0xbc`): every
    /// kind of weapon's (the shared loader `007bbc30`). [game] [data]
    pub camera_enabled: bool,
    /// `maxSnipeDamage` (`+0x168`), `timeTilSnipeChargeStarts` (`+0x16c`),
    /// `snipeChargeDurationTilMaxEffect` (`+0x170`): guns only; 100, 0.5 and 2 when absent.
    pub max_damage: f32,
    pub charge_start: f32,
    pub charge_duration: f32,
}

/// The weapon's snipe clock (`+0x454`) after one update of its own (`007ac370` l.30-40):
/// while sniping with the weapon in hand (its slot `+0x50` with 0, 0, 1 = `weapon_is_in_hand`
/// 007a7470: not `isSpecial`, no `weaponParent`, control mode not 1, the robot's weapon in
/// hand) it runs on; else it runs on only up to 0, so the wait after a shot still counts down.
/// The heat has no part in it. [game 007ac370 l.27-37, 007a7470]
pub fn clock_after(clock: f32, sniping_in_hand: bool, dt: f32) -> f32 {
    let t = clock + dt;
    if sniping_in_hand { t } else { t.min(0.0) }
}

/// The condition `clock_after` takes, as `weapon_is_in_hand` 007a7470 gives it with the
/// arguments 0, 0, 1: sniping, the weapon is the robot's in hand, it is not `isSpecial` and has
/// no `weaponParent`. The control mode is 2 while sniping, so its "not 1" holds. [game]
pub fn clock_runs(sniping: bool, in_hand: bool, has_parent: bool, is_special: bool) -> bool {
    sniping && in_hand && !has_parent && !is_special
}

/// The clock a shot fired while sniping leaves (`007ae010` l.505-512): minus
/// `timeTilSnipeChargeStarts`. [game]
pub fn clock_after_shot(t: &SnipeTuning) -> f32 {
    -t.charge_start
}

/// The steady (`weapon_sniper_steady` 007ac920, the weapon's slot `+0x128`): 0 unless
/// sniping; else the clock over `snipeChargeDurationTilMaxEffect`, 0 to 1, squared. [game]
pub fn steady(clock: f32, t: &SnipeTuning, sniping: bool) -> f32 {
    if !sniping {
        return 0.0;
    }
    let s = if clock > 0.0 { (clock / t.charge_duration).min(1.0) } else { 0.0 };
    s * s
}

/// The round's damage multiplier while sniping (`007ac760`, the weapon's slot `+0x160`, handed
/// to `bullet_trace_setup` 00784080): `(steady * (maxSnipeDamage - damage) + damage) / damage`,
/// 1 for a weapon with no damage. [game] Out of sniping the same slot gives the type-6
/// special's upgrade scale or 1, which this does not cover.
pub fn damage_multiplier(steady: f32, t: &SnipeTuning, damage: f32) -> f32 {
    if damage > 0.0 { (steady * (t.max_damage - damage) + damage) / damage } else { 1.0 }
}

/// What the zoom input is while sniping (`character_camera_message` 0071a5c0): +1 with
/// `CameraDebugFollowCloser` (button 4) held, else -1 with `CameraDebugFollowFarther`
/// (button 5) held, else 0. It replaces the second stick's y the message hands the camera
/// (its `+0x28`, the behaviour's `+0x78`). [game]
pub fn zoom_input(closer: bool, farther: bool) -> f32 {
    if closer {
        1.0
    } else if farther {
        -1.0
    } else {
        0.0
    }
}

/// The sniper camera's zoom settings (`00929b20`): `defaultFOV` (the base loader's), `zoomFOV`
/// (5 when absent) and `zoomFullTime` (1.5 when absent). Degrees and seconds. [game] [data]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomTuning {
    pub default_fov: f32,
    pub zoom_fov: f32,
    pub full_time: f32,
}

/// One step of the zoom (`zoom_camera_fov_update` 00929d50, after the weapon camera's update;
/// nothing for a step of 1e-6 s or less): `1/tan` of the field of view moves by its whole range
/// over `zoomFullTime` times the input, kept between the two ends. The angle is the camera's
/// whole field of view in degrees, as the routine takes it. [game] Not in: the sound roll-off
/// scale it sets to fov / 60.
pub fn zoom_step(fov: f32, t: &ZoomTuning, input: f32, dt: f32) -> f32 {
    if dt <= 1e-6 {
        return fov;
    }
    let inv = |deg: f32| 1.0 / deg.to_radians().tan();
    let (wide, narrow) = (inv(t.default_fov), inv(t.zoom_fov));
    let now = (narrow - wide) / t.full_time * input * dt + inv(fov);
    let clamped = now.max(wide).min(narrow);
    (1.0 / clamped).atan().to_degrees()
}

/// The aim's rates while sniping (`AimAssist`'s `aimSniping*`; the loader `0079bea0`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SnipeAim {
    /// Degrees a second at the lowest and highest aim sensitivity, the push's acceleration,
    /// the pitch limits, and the share of the rate left at full zoom. [data] 18, 36, 350, -80,
    /// 80, 0.19.
    pub lowest_sensitivity: f32,
    pub highest_sensitivity: f32,
    pub acceleration: f32,
    pub min_pitch: f32,
    pub max_pitch: f32,
    pub full_zoom_share: f32,
}

/// How much of the sniping rate the zoom leaves (`0071b990`, the sniping branch):
/// `(sin fov - sin 5) / (sin 15 - sin 5)` (degrees; the constants are 0.0872665 and 0.2617994
/// radians), taken from the share to 1 and kept in that range. `fov` is the view's, degrees.
/// [game] the camera's field of view it reads (`scene_camera_fov` 00557aa0, a return the
/// decompiler drops) is the zoomed one: 00557aa0 (raw disassembly) returns the active camera's
/// `+0x94` when its flag at `+0x464` bit 0 is set and `+0x468` is above 0. The sniper camera's
/// zoom step, `zoom_camera_fov_update` 00929d50, writes that `+0x94` (`*(cam + 0x94) = fov` after
/// stepping `1/tan`), so the value read is the zoomed fov. The zoom input is the sniping sign
/// (`00929d50`'s behaviour `+0x78`). [game 00929d50]
pub fn zoom_rate_share(fov: f32, full_zoom_share: f32) -> f32 {
    let low = 5f32.to_radians().sin();
    let high = 15f32.to_radians().sin();
    let along = (fov.to_radians().sin() - low) / (high - low);
    ((1.0 - full_zoom_share) * along + full_zoom_share).clamp(full_zoom_share, 1.0)
}

/// Whether sniping must end this update (`0071dc20`, before the toggle): it is on and the weapon
/// in hand has no snipe camera (or there is none), or he is not in `StateWeapon`. [game] The
/// game's blockers (`option_flag_bit0` 00913100, `ai_input_blocked_gate` 0091cef0) are not
/// modelled.
pub fn must_leave(sniping: bool, weapon_has_snipe_camera: bool, in_state_weapon: bool) -> bool {
    sniping && (!weapon_has_snipe_camera || !in_state_weapon)
}

/// Whether `CameraReset` toggles sniping this update (`0071dc20`): a human player, the weapon in
/// hand with `snipeCameraEnabled`, the character with a sniper camera, control mode 2 (weapon
/// mode), and the button pressed (the detector's slot `+0xc` with 7). [game]
pub fn toggles(human: bool, weapon_has_snipe_camera: bool, has_sniper_camera: bool, weapon_mode: bool, reset_pressed: bool) -> bool {
    human && weapon_has_snipe_camera && has_sniper_camera && weapon_mode && reset_pressed
}

/// How far below the camera a sniping round's barrel is (`007a7a40`: the barrel's matrix is the
/// camera's with its translation's z less 1.5). [game]
pub const BARREL_DROP: f32 = 1.5;

/// The character's animation rate while sniping (`character_set_sniping` 00721ce0 calls his
/// vtable `+0x1c` = `character_speed_scale_set` 0071f4d0 with `[00b34768]` = 0.2 going in, and
/// with 1.0 going out). It scales every animation channel of his (`+0x3c`, `+0x54`, `+0x5c`)
/// and the vehicle-form object's. [game] animation only, not his movement (settled by the grep
/// below, E1 second pass). [game 0071f4d0] Read: the writer `character_speed_scale_set`
/// sets char `+0xd8` and scales the animation channels (`+0x3c`, `+0x54`, `+0x5c`) and the child
/// at `+600`. [game] Export-wide grep of `(param_1|unaff_ESI|in_EAX) + 0xd8` field reads (78
/// lines, 2026-10-20): the character's `+0xd8` is read by no movement function. The rest belong
/// to other classes (AnimController 00748dc0 has its own `+0xd8`, 00778160 a byte at +0xd8 of an
/// effects object, 00720000 copies). Hits of `+0xd8)` as a VTABLE slot (0074f2d0) are not field
/// reads. The pass is by grep, not by reading each reader's class: a reader in a function whose
/// class is not named could be missed.
pub const ANIM_RATE: f32 = 0.2;

/// The rate after leaving: 1.0 without condition (it would end a DComScout 1.5 early). [game]
pub const ANIM_RATE_OUT: f32 = 1.0;

/// Whether the rifle stays hidden after this update (`character_set_sniping`'s third argument):
/// going in hides the weapon in hand (`manager_weapon_in_hand(+0x238, 0)` -> 01176750); a
/// toggle leave (third argument 1) re-shows it through `weapon_show` 007a7630; a forced leave
/// (0, 0) does not, and the weapon switch that caused it decides. [game 00721ce0, 0071dc20
/// l.52, l.121 (forced leaves: reshow 0), l.135 (the toggle: reshow 1)] [assumed] Out of
/// sniping it is never hidden: a forced leave leaves the rifle hidden as it was, unless the
/// weapon switch that caused the leave shows it (not read). settles: the weapon switch's show
/// or hide calls (the callers of `0071dc20`'s leave path, the weapon change in the state
/// machine). 2026-10-20 read: the weapon change `manager_take_in_hand` 007b2c60 hides the old
/// weapon (slot `+0x28`) and shows the new one (slot `+0x24`) only when that weapon's
/// `listenToWeaponAnimEvent` is false, and the sniper rifles listen (AudiR8, Firestorm,
/// DComSniper, DStnSniper): so for them a weapon switch does neither, and the rifle's show or
/// hide is left to its animation events (their handler is not read). That may make a forced
/// leave by a switch keep the rifle hidden, not show it as this rule does. Not changed: the
/// event that shows it is unread. settles: the handler of the rifle's animation events that shows or
/// hides the object (the one behind `listenToWeaponAnimEvent`, not identified), read against the leave.
pub fn rifle_hidden(sniping: bool) -> bool {
    sniping
}

/// `SniperScope_Gfx` (GfxUIEntity c815cb1d in `bnxglobal.str#23`): its movie, by the picture's
/// shape, under the install's `UI\Flash`. The widescreen flag `00d4cbf2` is `4/3 < w/h` from the
/// window [game 00a95c40 l.90, the window's own aspect, not an option]. The loader
/// `gfx_movie_load` 008f6fe0 makes the movie for the display the PRIMARY view (`+0xc`): the 4:3
/// name when the flag is 0 (l.64-68), the widescreen name when it is 1 (l.69-73). The second view
/// (`+0x21c`) holds the other one. [game 008f6fe0] `movie_visibility_aspect` 008f75f0 takes its
/// "primary shown" branch only when the mode (`+0x218`) is 2 with the flag 0 or 1 with the flag 1,
/// and the loader only sets modes 1 and 2 (1 with the flag 0, 2 with the flag 1): so the else
/// branch always runs, the primary's byte `+0x4e` is 0 (visible, the loader's "visible 0") and
/// the second's is 1. [game 008f75f0 l.4-30] So the primary, the movie for the display, is shown.
/// The rule: 4:3 for a window with `w/h <= 4/3` (the flag's own test), widescreen otherwise. This
/// was 1.5 before (2026-10-20 read): a window between 4:3 and 1.5 got the 4:3 movie in error.
/// The names: `sniperosd.gfx` and `sniperosd4x3.gfx` are the install's files (`UI\Flash`), and the
/// data pairs them so [data, read by hand 2026-10-20]: in `SniperScope_Gfx`'s chunk
/// (`work\survey\global_uinx\85926d4a_ac51a1ee.bin`, hash c815cb1d at 0x255c) the `FlashMovie`
/// type puts `filename_43` at +0 and `filename_ws` at +4 (descriptor triples at 0xf04 / 0xf10), and the
/// movie record at 0x3a70 holds self-relative offsets to `uilash\sniperosd4x3.gfx` (0x714c) at +0
/// and `uilash\sniperosd.gfx` (0x7166) at +4 (its base part's `filename`, 0x3a6c, is the 4x3 one).
/// The loader picks between them by the flag (`008f6fe0`).
pub fn scope_movie_file(aspect: f32) -> &'static str {
    // [game 00a95c40 l.90] widescreen when 4/3 < w/h, with 4/3 as a float constant.
    if aspect > 4.0 / 3.0 { "sniperosd.gfx" } else { "sniperosd4x3.gfx" }
}

/// The root frame the scope movie is on `clock` seconds after `Activate`: it plays from frame 1
/// at the movie's rate and stops at frame 5 (its `stop`). [data: the root's scripts] [assumed]
/// it starts again from frame 1 on each Activate (the widget's activate handler is not read).
/// Read (`work\gfx_dump.py sniperosd.gfx tree 0 2`): the root sprite (id 0, 10 frames) has frame
/// scripts at 1, 2, 4, 5 and 10; the movie's own `SniperOSD` child is at depth 1. The Activate
/// call comes from the game (`SniperScope_Gfx` in `character_set_sniping` 00721ce0:
/// `ui_key_select` 00725640 then `ui_widget_change_state` 008d6220 with Activate, read there).
/// [game] The movie has no Activate handler: no frame action in sprites 0..40 names Activate or
/// gotoAndPlay; the root's only gotoAndStop calls (5 and 10) are in its PauseMenu-style
/// functions (gcmd*), not on Activate. So the frame reset, if any, is in the widget state
/// change (`008d6220`: it finds the widget and calls its vtable slot 1 with the state hash, read).
/// settles: the SniperScope_Gfx widget's slot 1 handler for the Activate hash (does it play or
/// reset the movie): not read.
pub fn scope_root_frame(clock: f32, rate: f32) -> usize {
    ((clock.max(0.0) * rate) as usize + 1).min(5)
}

/// One picture of the scope movie, `clock` seconds after `Activate`, the stage scaled by
/// `scale`: the root's display list at its frame, the nested timelines (the rings, the scan
/// lines) running by the same clock and looping. `gcmdIn(0)` calls `InitMenu` on a sprite
/// with no class, so it does nothing; nothing else drives the movie. [data]
pub fn scope_draw(movie: &crate::formats::gfx::Movie, clock: f32, scale: f32) -> crate::formats::gfx::Canvas {
    use crate::formats::gfx::{Affine, Canvas, Pose, Tint, Tweak, ROOT};
    let mut canvas = Canvas::new((movie.stage[0] * scale).ceil() as usize, (movie.stage[1] * scale).ceil() as usize);
    let mut pose = Pose::new();
    pose.insert(String::new(), Tweak { frame: Some(scope_root_frame(clock, movie.rate)), ..Default::default() });
    movie.draw(ROOT, &Affine::scale_then_move(scale, 0.0, 0.0), &Tint::NONE, &pose, "", clock, &mut canvas);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = crate::sim::GAME_UPDATE;

    /// The widescreen flag is 4/3 < w/h [game 00a95c40 l.90]: a 1.4 window (between 4:3 and 1.5)
    /// gets the widescreen movie, a 4:3 window the 4:3 one.
    #[test]
    fn the_scope_movie_is_the_4x3_one_only_at_four_by_three_or_narrower() {
        assert_eq!(scope_movie_file(4.0 / 3.0), "sniperosd4x3.gfx");
        assert_eq!(scope_movie_file(1.3), "sniperosd4x3.gfx");
        assert_eq!(scope_movie_file(1.4), "sniperosd.gfx");
        assert_eq!(scope_movie_file(16.0 / 10.0), "sniperosd.gfx");
        assert_eq!(scope_movie_file(16.0 / 9.0), "sniperosd.gfx");
    }

    /// Needs the install. [data] `SniperScope_Gfx`'s movie: 1280 x 720 at 30 a second, 10 root
    /// frames, and it draws its teal frame round a clear middle.
    #[test]
    fn the_scope_movie_parses_and_draws_a_frame_round_a_clear_middle() {
        let dir = &crate::install::dir().join("UI/Flash");
        let Ok(file) = std::fs::read(dir.join(scope_movie_file(16.0 / 9.0))) else { return };
        let movie = crate::formats::gfx::Movie::parse(&file).unwrap();
        assert_eq!((movie.stage, movie.rate), ([1280.0, 720.0], 30.0));
        assert_eq!(movie.sprite(crate::formats::gfx::ROOT).unwrap().frames.len(), 10);
        assert_eq!(scope_root_frame(0.0, 30.0), 1);
        assert_eq!(scope_root_frame(10.0, 30.0), 5);
        let started = std::time::Instant::now();
        let canvas = scope_draw(&movie, 0.5, 0.5);
        eprintln!("scope at half size: {:?}", started.elapsed());
        assert_eq!((canvas.width, canvas.height), (640, 360));
        // `SCOPE_PNG=<path>` saves the picture to look at.
        if let Ok(path) = std::env::var("SCOPE_PNG") {
            image::RgbaImage::from_raw(640, 360, canvas.rgba8()).unwrap().save(path).unwrap();
        }
        let at = |x: usize, y: usize| canvas.pixels[y * canvas.width + x];
        // A corner is covered by the frame; the middle is (nearly) clear.
        assert!(at(4, 4)[3] > 0.5, "corner {:?}", at(4, 4));
        assert!(at(320, 180)[3] < 0.5, "middle {:?}", at(320, 180));
        let gfx4x3 = std::fs::read(dir.join(scope_movie_file(4.0 / 3.0))).unwrap();
        assert!(crate::formats::gfx::Movie::parse(&gfx4x3).is_ok());
    }

    /// Needs the install. [data] The scope's sounds are in the snipers' own event files.
    #[test]
    fn the_scope_sounds_are_in_the_snipers_event_files() {
        let dir = &crate::install::dir().join("characters/eng");
        for (file, names) in [
            ("audir8.fev", ["WPN_SNIPER_ZOOM_D", "BG_SNIPER_D", "ANIM_DECEPTICON_SNIPER_RETURN"]),
            ("firestorm.fev", ["WPN_SNIPER_ZOOM_A", "BG_SNIPER_A", "ANIM_AUTOBOT_SNIPER_RETURN"]),
        ] {
            let Ok(bytes) = std::fs::read(dir.join(file)) else { return };
            for name in names {
                assert!(bytes.windows(name.len()).any(|w| w == name.as_bytes()), "{name} not in {file}");
            }
        }
    }

    /// The Audi's PlasmaLongRifle. [data]
    fn audi() -> SnipeTuning {
        SnipeTuning { camera_enabled: true, max_damage: 200.0, charge_start: 0.9, charge_duration: 0.9 }
    }

    #[test]
    fn the_clock_runs_only_while_sniping_and_a_shot_sets_it_back() {
        let t = audi();
        let mut clock = 0.0;
        // Not sniping: held at 0.
        for _ in 0..10 {
            clock = clock_after(clock, false, DT);
        }
        assert_eq!(clock, 0.0);
        // A shot leaves the wait, which counts down even out of sniping, and stops at 0.
        clock = clock_after_shot(&t);
        assert_eq!(clock, -0.9);
        for _ in 0..40 {
            clock = clock_after(clock, false, DT);
        }
        assert_eq!(clock, 0.0);
        // Sniping: it runs on past 0.
        clock = clock_after_shot(&t);
        for _ in 0..50 {
            clock = clock_after(clock, true, DT);
        }
        assert!((clock - (-0.9 + 50.0 * DT)).abs() < 1e-4);
    }

    #[test]
    fn the_clock_runs_only_for_the_weapon_in_hand() {
        assert!((clock_after(0.3, true, 0.032) - 0.332).abs() < 1e-6);
        assert!(clock_runs(true, true, false, false));
        assert!(!clock_runs(false, true, false, false));
        assert!(!clock_runs(true, false, false, false));
        assert!(!clock_runs(true, true, true, false));
        assert!(!clock_runs(true, true, false, true));
    }

    #[test]
    fn the_steady_is_the_squared_share_of_the_charge_time() {
        let t = audi();
        assert_eq!(steady(0.45, &t, false), 0.0);
        assert_eq!(steady(-0.2, &t, true), 0.0);
        assert!((steady(0.45, &t, true) - 0.25).abs() < 1e-6);
        assert_eq!(steady(5.0, &t, true), 1.0);
    }

    #[test]
    fn the_damage_goes_from_the_weapons_to_max_snipe_damage() {
        let t = audi();
        assert_eq!(damage_multiplier(0.0, &t, 100.0), 1.0);
        assert!((damage_multiplier(0.25, &t, 100.0) - 1.25).abs() < 1e-6);
        assert_eq!(damage_multiplier(1.0, &t, 100.0), 2.0);
        // Breakaway's rifle: 85 to 255, three times. [data]
        let b = SnipeTuning { max_damage: 255.0, charge_start: 0.5, charge_duration: 2.7, camera_enabled: true };
        assert!((damage_multiplier(1.0, &b, 85.0) - 3.0).abs() < 1e-6);
        assert_eq!(damage_multiplier(1.0, &b, 0.0), 1.0);
    }

    #[test]
    fn the_zoom_takes_the_full_time_from_one_end_to_the_other() {
        // `audir8SniperCamera`: 15 to 5 degrees over 0.25 s. [data]
        let z = ZoomTuning { default_fov: 15.0, zoom_fov: 5.0, full_time: 0.25 };
        let mut fov = z.default_fov;
        let mut steps = 0;
        while fov > z.zoom_fov + 1e-3 {
            fov = zoom_step(fov, &z, 1.0, DT);
            steps += 1;
            assert!(steps < 100);
        }
        // 0.25 s is 7.8 updates of 0.032: the eighth reaches the end and is held there.
        assert_eq!(steps, 8);
        assert!((zoom_step(fov, &z, 1.0, DT) - 5.0).abs() < 1e-3);
        // No input, no change; out again to 15 and no further.
        assert_eq!(zoom_step(10.0, &z, 0.0, DT), 10.0);
        for _ in 0..20 {
            fov = zoom_step(fov, &z, -1.0, DT);
        }
        assert!((fov - 15.0).abs() < 1e-3);
        // A step of no time does nothing.
        assert_eq!(zoom_step(10.0, &z, 1.0, 0.0), 10.0);
    }

    #[test]
    fn the_aim_rate_share_falls_to_the_full_zoom_share() {
        assert!((zoom_rate_share(15.0, 0.19) - 1.0).abs() < 1e-5);
        assert!((zoom_rate_share(5.0, 0.19) - 0.19).abs() < 1e-5);
        assert_eq!(zoom_rate_share(60.0, 0.19), 1.0);
        let mid = zoom_rate_share(10.0, 0.19);
        assert!(mid > 0.19 && mid < 1.0);
    }

    #[test]
    fn sniping_toggles_only_in_weapon_mode_with_a_snipe_weapon_and_camera() {
        assert!(toggles(true, true, true, true, true));
        assert!(!toggles(false, true, true, true, true));
        assert!(!toggles(true, false, true, true, true));
        assert!(!toggles(true, true, false, true, true));
        assert!(!toggles(true, true, true, false, true));
        assert!(!toggles(true, true, true, true, false));
        assert!(must_leave(true, false, true));
        assert!(must_leave(true, true, false));
        assert!(!must_leave(true, true, true));
        assert!(!must_leave(false, false, false));
    }
}
