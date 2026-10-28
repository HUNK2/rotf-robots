//! The sound debug overlay (F8): every sound in the mix, who started it, where it is from the
//! listener, how loud it is now, and whether it is looping, held at a sustain point or stopping.
//! Sounds that ended in the last few seconds stay listed, dimmed. F9 freezes the list and prints
//! it to the console. `--sound-debug` opens it at the start (and in a `--shot` capture).
//! Not the game's: a debug aid (`SoundOut::debug_view`, `Mixer::view`).

use std::collections::HashMap;

use bevy::prelude::*;
use tf2_core::sound::Handle;

use crate::sound::{DebugSound, SoundOut};

/// Seconds an ended sound stays listed.
const ENDED_FOR: f32 = 3.0;
/// Seconds between redraws.
const REFRESH: f32 = 0.1;
/// Rows drawn; the rest are counted.
const MOST_ROWS: usize = 40;
/// A sound alive this long is marked: what a sound that never ends looks like.
const LONG: f32 = 20.0;
/// The meter's floor, in dB below full scale.
const FLOOR_DB: f32 = 60.0;

#[derive(Resource)]
pub struct SoundDebug {
    shown: bool,
    frozen: bool,
    clock: f32,
    /// When the list was last drawn (the app's seconds).
    drawn_at: f32,
    /// Each sound's meter, held and let down slowly so a short peak can be read.
    meters: HashMap<Handle, f32>,
    /// Last redraw's sounds (name, owner, age), to see which ended.
    seen: HashMap<Handle, (String, String, f32)>,
    /// Ended sounds: name, owner, how long it played, seconds left on the list.
    ended: Vec<(String, String, f32, f32)>,
}

impl SoundDebug {
    pub fn new(shown: bool) -> Self {
        Self { shown, frozen: false, clock: 0.0, drawn_at: 0.0, meters: HashMap::new(), seen: HashMap::new(), ended: Vec::new() }
    }
}

#[derive(Component)]
pub struct SoundDebugPanel;

pub fn spawn(mut commands: Commands, debug: Res<SoundDebug>) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(10.0),
            top: Val::Px(10.0),
            width: Val::Px(700.0),
            max_width: Val::Percent(60.0),
            max_height: Val::Percent(96.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(6.0)),
            row_gap: Val::Px(1.0),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        GlobalZIndex(40),
        if debug.shown { Visibility::Inherited } else { Visibility::Hidden },
        SoundDebugPanel,
    ));
}

/// Which way a source is from the listener (x right, y front, z up), as a word.
fn direction(offset: Vec3) -> &'static str {
    let flat = Vec2::new(offset.x, offset.y);
    if flat.length() < 0.25 {
        return if offset.z > 0.25 { "above" } else if offset.z < -0.25 { "below" } else { "here" };
    }
    // Clockwise from the front, in eighths.
    let angle = offset.x.atan2(offset.y).to_degrees().rem_euclid(360.0);
    ["front", "front-right", "right", "back-right", "back", "back-left", "left", "front-left"][((angle + 22.5) / 45.0) as usize % 8]
}

fn decibels(level: f32) -> f32 {
    if level > 0.0 { 20.0 * level.log10() } else { -f32::INFINITY }
}

/// One sound's line: name, owner, place, level, age and state.
fn describe(sound: &DebugSound, meter: f32) -> (String, Color) {
    let v = &sound.view;
    let place = if !v.placed {
        "2D".to_owned()
    } else if !v.own_place {
        format!("3D on the body {:.1} m {}", v.offset.length(), direction(v.offset))
    } else {
        format!("3D {:.1} m {}", v.offset.length(), direction(v.offset))
    };
    let db = decibels(meter);
    let level = if db < -FLOOR_DB { "silent".to_owned() } else { format!("{db:.0} dB") };
    let mut state = Vec::new();
    if sound.infinite { state.push("loop".to_owned()); }
    if v.held { state.push("held".to_owned()); }
    if v.key_offs > 0 { state.push(format!("key-off x{}", v.key_offs)); }
    if v.stopping { state.push(format!("stopping {:.0}%", v.fade * 100.0)); }
    if v.placed && v.rolloff < 1.0 { state.push(format!("roll-off {:.2}", v.rolloff)); }
    if v.elapsed >= LONG { state.push("LONG".to_owned()); }
    let line = format!("{}  [{}]  {place}  {level}  {:.1} s  {}/{} on  {}",
        v.name, sound.owner, v.elapsed, v.sounding, v.sounds, state.join(" "));
    let colour = if v.stopping {
        Color::srgb(0.6, 0.6, 0.6)
    } else if v.elapsed >= LONG {
        Color::srgb(1.0, 0.6, 0.2)
    } else if sound.infinite {
        Color::srgb(0.6, 0.85, 1.0)
    } else {
        Color::srgb(0.95, 0.95, 0.9)
    };
    (line, colour)
}

pub fn run(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    sound: Option<Res<SoundOut>>,
    mut debug: ResMut<SoundDebug>,
    panel: Single<(Entity, &mut Visibility), With<SoundDebugPanel>>,
) {
    let (panel, mut visibility) = panel.into_inner();
    if keys.just_pressed(KeyCode::F8) {
        debug.shown = !debug.shown;
        *visibility = if debug.shown { Visibility::Inherited } else { Visibility::Hidden };
        debug.frozen = false;
    }
    if !debug.shown {
        return;
    }
    let print = debug.shown && keys.just_pressed(KeyCode::F9);
    if print {
        debug.frozen = !debug.frozen;
    }
    let dt = time.delta_secs();
    for meter in debug.meters.values_mut() {
        // Let down 20 dB a second.
        *meter *= 0.1f32.powf(dt);
    }
    debug.clock -= dt;
    if (debug.frozen && !print) || (debug.clock > 0.0 && !print) {
        return;
    }
    debug.clock = REFRESH;
    let Some(sound) = sound else {
        commands.entity(panel).despawn_related::<Children>().with_child((
            Text::new("Sound debug: no sound (--no-sound, or the output did not start)"),
            TextFont::from_font_size(13.0),
        ));
        return;
    };
    let mut view = sound.debug_view();
    let since = time.elapsed_secs() - debug.drawn_at;
    debug.drawn_at = time.elapsed_secs();
    // Meters: the louder of the held value and the last render's peak.
    let mut meters = HashMap::new();
    for s in &view.sounds {
        let held = debug.meters.get(&s.view.handle).copied().unwrap_or(0.0);
        meters.insert(s.view.handle, held.max(s.view.peak));
    }
    // Sounds gone since the last redraw.
    let now: HashMap<Handle, (String, String, f32)> =
        view.sounds.iter().map(|s| (s.view.handle, (s.view.name.clone(), s.owner.clone(), s.view.elapsed))).collect();
    let gone: Vec<_> = debug.seen.iter().filter(|(h, _)| !now.contains_key(h)).map(|(_, v)| v.clone()).collect();
    for (name, owner, age) in gone {
        debug.ended.insert(0, (name, owner, age, ENDED_FOR));
    }
    for e in &mut debug.ended {
        e.3 -= since;
    }
    debug.ended.retain(|e| e.3 > 0.0);
    debug.seen = now;
    debug.meters = meters;

    // The player's own first, then by owner, then oldest first.
    view.sounds.sort_by(|a, b| (a.owner != "player", &a.owner, a.view.handle.number()).cmp(&(b.owner != "player", &b.owner, b.view.handle.number())));
    let placed = view.sounds.iter().filter(|s| s.view.placed).count();
    let looping = view.sounds.iter().filter(|s| s.infinite && !s.view.stopping).count();
    let stopping = view.sounds.iter().filter(|s| s.view.stopping).count();
    let header = format!(
        "SOUNDS {}  (3D {placed}, 2D {}, looping {looping}, stopping {stopping})  output {:.1} s  master {:.2}\nreverb: {}   F8 hide   F9 {}",
        view.sounds.len(), view.sounds.len() - placed, view.output_time, view.master, view.reverb,
        if debug.frozen { "resume (frozen)" } else { "freeze and print" });
    if print && debug.frozen {
        eprintln!("sound debug at output {:.2} s: {}", view.output_time, header.replace('\n', " | "));
    }
    let mut lines: Vec<(String, Color, Option<f32>)> = Vec::new();
    for s in &view.sounds {
        let meter = debug.meters.get(&s.view.handle).copied().unwrap_or(0.0);
        let (line, colour) = describe(s, meter);
        if print && debug.frozen {
            eprintln!("  {line}");
        }
        lines.push((line, colour, Some(meter)));
    }
    // Ended sounds, the same event and owner counted together, newest first.
    let mut grouped: Vec<(&str, &str, f32, usize)> = Vec::new();
    for (name, owner, age, _) in &debug.ended {
        match grouped.iter_mut().find(|g| g.0 == name && g.1 == owner) {
            Some(g) => g.3 += 1,
            None => grouped.push((name, owner, *age, 1)),
        }
    }
    for (name, owner, age, count) in grouped {
        let times = if count > 1 { format!(" x{count}") } else { String::new() };
        lines.push((format!("ended{times}: {name}  [{owner}]  after {age:.1} s"), Color::srgb(0.5, 0.5, 0.5), None));
    }

    let mut entity = commands.entity(panel);
    entity.despawn_related::<Children>();
    entity.with_children(|p| {
        p.spawn((Text::new(header), TextFont::from_font_size(13.0), TextColor(Color::srgb(1.0, 0.9, 0.4))));
        let more = lines.len().saturating_sub(MOST_ROWS);
        for (line, colour, meter) in lines.into_iter().take(MOST_ROWS) {
            p.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() })
                .with_children(|row| {
                    // The meter: -60 dB to full scale.
                    let fill = meter.map_or(0.0, |m| ((decibels(m) + FLOOR_DB) / FLOOR_DB).clamp(0.0, 1.0));
                    let bar = if fill > 0.9 { Color::srgb(1.0, 0.3, 0.2) } else if fill > 0.7 { Color::srgb(1.0, 0.85, 0.2) } else { Color::srgb(0.3, 0.9, 0.4) };
                    let back = if meter.is_some() { Color::srgba(1.0, 1.0, 1.0, 0.12) } else { Color::NONE };
                    row.spawn((Node { width: Val::Px(50.0), height: Val::Px(8.0), flex_shrink: 0.0, ..default() }, BackgroundColor(back)))
                        .with_child((Node { width: Val::Percent(fill * 100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(bar)));
                    row.spawn((Text::new(line), TextFont::from_font_size(12.0), TextColor(colour)));
                });
        }
        if more > 0 {
            p.spawn((Text::new(format!("... and {more} more")), TextFont::from_font_size(12.0)));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_are_named_from_the_listeners_look() {
        assert_eq!(direction(Vec3::new(0.0, 5.0, 0.0)), "front");
        assert_eq!(direction(Vec3::new(5.0, 0.0, 0.0)), "right");
        assert_eq!(direction(Vec3::new(-5.0, 0.0, 0.0)), "left");
        assert_eq!(direction(Vec3::new(0.0, -5.0, 0.0)), "back");
        assert_eq!(direction(Vec3::new(3.0, 3.0, 0.0)), "front-right");
        assert_eq!(direction(Vec3::new(0.0, 0.0, 2.0)), "above");
    }
}
