//! The game's sound mix above single events: audio presets (per-category volume ducking,
//! pitch offsets and pauses), the music queue (play now, fade out then play, crossfade) and
//! the music volume fade. Data is read at runtime from the user's install
//! (`bnxglobal.str`, the data chunk whose root holds `presets`: the game's
//! `audio\presets.lxb`); nothing from the game is bundled.
//!
//! Sources [game]: 01196290 (presets.lxb `presets` field), 00772820 (preset by id),
//! 007726f0 (entry by category name), 00771230 (play a preset: fall-off by distance),
//! 011832a0 / 01178880 (release), 00595ff0 (slot claim / re-apply), 00595fb0 (clear),
//! 00595f30 (category record init), 00596200 (category update, pitch from disassembly
//! 00596350-0059637b), 007719d0 (music play modes), 00771540 (audio manager update: music
//! fades and listener), 007705c0 / 007706d0 (MusicPlayer load and trigger), 00773cd0 /
//! 00773de0 / 00599070 (SetMusicVolume and its fade). Write-up: `notes\sound.md`
//! "Sound mix: presets and music (2026-10-16)".

use std::path::Path;

use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};

/// Slots per category: the record holds eight 0x38-byte slots (00596200 counts 8). [game]
pub const SLOTS: usize = 8;

/// One `AudioPresetEntry`: what a preset does to one category. Field names from the data.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioPresetEntry {
    pub category: String,
    /// Volume the category is ducked to (1 = untouched).
    pub volume: f32,
    pub volume_duration: f32,
    /// Below 0 means "the same as `volume_duration`" (00595ff0). [game]
    pub volume_out_duration: f32,
    /// Raw pitch offset added to the category's pitch (00596200). [game]
    pub pitch: f32,
    pub pitch_duration: f32,
    pub pitch_out_duration: f32,
    /// Pauses the category while the preset holds it.
    pub pause: bool,
    /// Hold time once fully in; below 0 holds until released (00596200 phase 2). [game]
    pub life_span: f32,
    pub fall_off_min: f32,
    /// Above 0 turns on the distance fall-off (00771230). [game]
    pub fall_off_max: f32,
}

/// One `AudioPreset`: its id (the object's name hash) and its entries.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioPreset {
    pub id: u32,
    pub entries: Vec<AudioPresetEntry>,
}

impl AudioPreset {
    /// The entry for a category, matched by exact name as 007726f0 does (strcmp). [game]
    pub fn entry(&self, category: &str) -> Option<&AudioPresetEntry> {
        self.entries.iter().find(|e| e.category == category)
    }
}

/// Finds a preset by id (00772820: linear search of the array). [game]
pub fn preset(presets: &[AudioPreset], id: u32) -> Option<&AudioPreset> {
    presets.iter().find(|p| p.id == id)
}

fn entry_from(node: Node) -> Option<AudioPresetEntry> {
    let f = |name: &str| node.get(name).and_then(|n| n.float()).unwrap_or(0.0);
    Some(AudioPresetEntry {
        category: node.get("category")?.text()?,
        volume: f("volume"),
        volume_duration: f("volumeDuration"),
        volume_out_duration: f("volumeOutDuration"),
        pitch: f("pitch"),
        pitch_duration: f("pitchDuration"),
        pitch_out_duration: f("pitchOutDuration"),
        // Stored as an integer (0 / 1) in presets.lxb.
        pause: node.get("pause").is_some_and(|n| n.int().map_or(n.is_true(), |v| v != 0)),
        life_span: f("lifeSpan"),
        fall_off_min: f("fallOffMin"),
        fall_off_max: f("fallOffMax"),
    })
}

/// Reads the presets from a decoded data file whose root holds `presets` (an array of
/// presets, each an array of entries with a `category`). Returns None for any other file.
pub fn presets_from(file: &DataFile) -> Option<Vec<AudioPreset>> {
    let list = file.root().get("presets")?;
    let first = list.items().next()?;
    first.items().next()?.get("category")?;
    Some(
        list.items()
            .map(|p| AudioPreset { id: p.label().unwrap_or(0), entries: p.items().filter_map(entry_from).collect() })
            .collect(),
    )
}

/// Loads the presets from `bnxglobal.str` in the install: the data chunk whose root holds
/// the preset array (chunk 18 on the user's install; found by content, not by number).
pub fn load_presets(game_dir: &Path) -> Result<Vec<AudioPreset>, String> {
    let pack = Pack::open(&game_dir.join("bnxglobal.str"))?;
    for chunk in pack.chunks.iter().filter(|c| c.tag == pack::DATA) {
        let Ok(file) = DataFile::parse(pack.data(chunk).to_vec()) else { continue };
        if let Some(presets) = presets_from(&file) {
            return Ok(presets);
        }
    }
    Err("bnxglobal.str has no audio presets chunk".into())
}

/// The volume a preset entry asks for at a distance (00771230). Without fall-off, or
/// without a source position, the entry's own volume. Beyond `fall_off_max` the entry is
/// skipped (None). Between, `1 - (1 - (d - min) / (max - min)) * (1 - volume)`; nothing
/// clamps it below `fall_off_min`, as in the game. [game]
pub fn entry_volume(entry: &AudioPresetEntry, distance: Option<f32>) -> Option<f32> {
    let Some(d) = distance.filter(|_| entry.fall_off_max > 0.0) else { return Some(entry.volume) };
    if entry.fall_off_max < d {
        return None;
    }
    let t = (d - entry.fall_off_min) / (entry.fall_off_max - entry.fall_off_min);
    Some(1.0 - (1.0 - t) * (1.0 - entry.volume))
}

/// A slot's phase (00596200: 0 free, 1 fading in, 2 holding, 3 fading out). [game]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Free,
    In,
    Hold,
    Out,
}

/// One preset's hold on a category (one 0x38-byte slot). [game]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slot {
    pub id: u32,
    pub phase: Phase,
    pub volume: f32,
    pub pitch: f32,
    pub pause: bool,
    pub volume_in_rate: f32,
    pub pitch_in_rate: f32,
    pub volume_out_rate: f32,
    pub pitch_out_rate: f32,
    /// How far in the volume duck is (0..1).
    pub volume_amount: f32,
    /// How far in the pitch offset is (0..1).
    pub pitch_amount: f32,
    pub life_span: f32,
    pub held: f32,
    /// How many plays hold the slot.
    pub count: i32,
}

/// What the category update sets on the FMOD category (vtable +0x1c, +0x24, +0x2c).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CategoryOut {
    pub volume: f32,
    /// Raw pitch: the units argument is 0 (disassembly 005963b2: EBP is 0 after the slot
    /// loop). [game: 004b8d20, EventCategoryI slot 9] units 1 (octaves) are divided by 4,
    /// 2 (semitones) by 48, 3 (tones) by 24, 0 kept: raw, where 1.0 is four octaves; see
    /// [`raw_pitch_multiplier`].
    pub pitch: f32,
    pub paused: bool,
}

/// One FMOD event category's preset record (00595f30). [game]
#[derive(Clone, Debug, PartialEq)]
pub struct CategoryMix {
    pub name: String,
    pub base_volume: f32,
    pub base_pitch: f32,
    /// Plays of presets with `pause` that hold the category (+0x14).
    pub pause_count: i32,
    /// The update runs only while set (+0x18); it stays set while any slot is in use.
    pub dirty: bool,
    pub slots: [Slot; SLOTS],
}

impl CategoryMix {
    pub fn new(name: &str) -> Self {
        Self { name: name.into(), base_volume: 1.0, base_pitch: 0.0, pause_count: 0, dirty: true, slots: [Slot::default(); SLOTS] }
    }

    /// A preset plays on this category (00595ff0). `volume` is the entry's volume after the
    /// fall-off; `distanced` is whether the fall-off applied (fall_off_max > 0).
    pub fn apply(&mut self, id: u32, volume: f32, entry: &AudioPresetEntry, distanced: bool) {
        self.dirty = true;
        if let Some(s) = self.slots.iter_mut().find(|s| s.phase != Phase::Free && s.id == id) {
            s.count += 1;
            s.held = 0.0;
            if distanced {
                // Re-aim the fade from the current level. The target volume is NOT updated
                // here, as read (00595ff0 lines after the found slot). [game]
                let current = s.volume * s.volume_amount + 1.0 - s.volume_amount;
                let louder = if s.phase == Phase::Out { current > volume } else { s.volume > volume };
                if louder {
                    let amount = (1.0 - current) / (1.0 - volume);
                    s.volume_amount = amount;
                    s.volume_in_rate *= 1.0 - amount;
                    s.phase = Phase::In;
                }
            }
            if s.pause {
                self.pause_count += 1;
            }
            return;
        }
        let Some(s) = self.slots.iter_mut().find(|s| s.phase == Phase::Free) else { return };
        let vin = entry.volume_duration.max(0.01);
        let vout = if entry.volume_out_duration < 0.0 { vin } else { entry.volume_out_duration }.max(0.01);
        let pin = entry.pitch_duration.max(0.01);
        let pout = if entry.pitch_out_duration < 0.0 { pin } else { entry.pitch_out_duration }.max(0.01);
        *s = Slot {
            id,
            phase: Phase::In,
            volume,
            pitch: entry.pitch,
            pause: entry.pause,
            volume_in_rate: 1.0 / vin,
            pitch_in_rate: 1.0 / pin,
            volume_out_rate: 1.0 / vout,
            pitch_out_rate: 1.0 / pout,
            volume_amount: 0.0,
            pitch_amount: 0.0,
            life_span: entry.life_span,
            held: 0.0,
            count: 1,
        };
        if entry.pause {
            self.pause_count += 1;
        }
    }

    /// One play of a preset ends (01178880): the count drops; the fade-out follows in the
    /// update once it reaches 0. [game]
    pub fn release(&mut self, id: u32) {
        self.dirty = true;
        if let Some(s) = self.slots.iter_mut().find(|s| s.id == id) {
            if s.count > 0 {
                s.count -= 1;
                if s.pause {
                    self.pause_count -= 1;
                }
            }
        }
    }

    /// Drops every preset at once, with no fade (00595fb0). [game]
    pub fn clear(&mut self) {
        self.dirty = true;
        for s in &mut self.slots {
            if s.phase != Phase::Free {
                if s.pause {
                    self.pause_count -= s.count;
                }
                s.phase = Phase::Free;
                s.count = 0;
            }
        }
    }

    /// The per-frame update (00596200). Returns what to set on the category, or None while
    /// the record is not dirty (the game skips it then). [game]
    pub fn update(&mut self, dt: f32) -> Option<CategoryOut> {
        if !self.dirty {
            return None;
        }
        let mut volume = self.base_volume;
        let mut pitch = self.base_pitch;
        let mut busy = false;
        for s in &mut self.slots {
            if s.phase == Phase::Free {
                continue;
            }
            busy = true;
            match s.phase {
                Phase::In => {
                    if s.count == 0 {
                        s.phase = Phase::Out;
                    } else {
                        s.volume_amount = (s.volume_in_rate * dt + s.volume_amount).min(1.0);
                        s.pitch_amount = (s.pitch_in_rate * dt + s.pitch_amount).min(1.0);
                        if s.volume_amount >= 1.0 && s.pitch_amount >= 1.0 {
                            s.phase = Phase::Hold;
                        }
                    }
                }
                Phase::Hold => {
                    let held = s.held + dt;
                    s.held = held;
                    if (s.life_span >= 0.0 && s.life_span < held) || s.count == 0 {
                        // The count is zeroed without touching the pause count, as read. [game]
                        s.count = 0;
                        s.phase = Phase::Out;
                    }
                }
                Phase::Out => {
                    if s.count < 1 {
                        s.volume_amount = (s.volume_amount - s.volume_out_rate * dt).max(0.0);
                        s.pitch_amount = (s.pitch_amount - s.pitch_out_rate * dt).max(0.0);
                        if s.volume_amount <= 0.0 && s.pitch_amount <= 0.0 {
                            s.phase = Phase::Free;
                        }
                    } else {
                        s.phase = Phase::In;
                    }
                }
                Phase::Free => {}
            }
            // Volume multiplies by lerp(1, target, amount); pitch adds pitch * amount
            // (disassembly 00596350-0059637b). A slot freed this frame still counts. [game]
            volume *= s.volume * s.volume_amount + 1.0 - s.volume_amount;
            pitch += s.pitch * s.pitch_amount;
        }
        self.dirty = busy;
        Some(CategoryOut { volume, pitch, paused: self.pause_count > 0 })
    }
}

/// Raw pitch to a frequency multiplier: 1.0 raw = four octaves. [game: 004b8d20] the
/// category keeps the raw value (+0x24) and hands its channel group 2^(raw x 4)
/// (`FUN_0040c6f0(2.0, raw * 4)` -> 00452da0).
pub fn raw_pitch_multiplier(raw: f32) -> f32 {
    (raw * 4.0).exp2()
}

/// The mix: one record per category, as the sound system keeps them (list 00d4f2f0).
#[derive(Clone, Debug, Default)]
pub struct Mix {
    pub categories: Vec<CategoryMix>,
}

impl Mix {
    pub fn new(names: &[&str]) -> Self {
        Self { categories: names.iter().map(|n| CategoryMix::new(n)).collect() }
    }

    /// Plays a preset (00771230): every category with an entry in it, each entry's volume
    /// after the fall-off from `distance` (listener to source; None = no position given).
    pub fn play(&mut self, preset: &AudioPreset, distance: Option<f32>) {
        for c in &mut self.categories {
            let Some(e) = preset.entry(&c.name) else { continue };
            let Some(v) = entry_volume(e, distance) else { continue };
            let distanced = distance.is_some() && e.fall_off_max > 0.0;
            c.apply(preset.id, v, e, distanced);
        }
    }

    /// Releases a preset on every category it has an entry for (011832a0). [game]
    pub fn stop(&mut self, preset: &AudioPreset) {
        for c in &mut self.categories {
            if preset.entry(&c.name).is_some() {
                c.release(preset.id);
            }
        }
    }

    pub fn category(&self, name: &str) -> Option<&CategoryMix> {
        self.categories.iter().find(|c| c.name == name)
    }
}

/// How a MusicPlayer hands its track over (`playType`, 007719d0). [game]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayType {
    /// 0: stop what plays, start the new track now.
    Now,
    /// 1: fade what plays out over `fadeTime`, then start the new track (Now if nothing plays).
    AfterFade,
    /// 2: crossfade: the new track starts silent and fades in while the old fades out.
    Crossfade,
}

impl PlayType {
    /// Other values do nothing in 007719d0 beyond a handle copy. [game]
    pub fn from_data(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Now),
            1 => Some(Self::AfterFade),
            2 => Some(Self::Crossfade),
            _ => None,
        }
    }
}

/// What the music queue asks of the sound player, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MusicCmd {
    Start(u32),
    Stop(u32),
    Volume(u32, f32),
}

/// The audio manager's music slots (+0x1c current, +0x24 next, +0x2c state, +0x40 elapsed,
/// +0x44 fade time). Tracks are the caller's ids. [game]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MusicQueue {
    pub current: Option<u32>,
    pub next: Option<u32>,
    /// 0 idle, 1 fading out before `next`, 2 crossfading to `next`.
    pub state: u8,
    pub elapsed: f32,
    pub fade_time: f32,
}

impl MusicQueue {
    /// A MusicPlayer is triggered with its event, play type and fade time (007719d0).
    pub fn play(&mut self, track: u32, play_type: PlayType, fade_time: f32) -> Vec<MusicCmd> {
        let mut out = Vec::new();
        let mode = if self.current.is_none() && play_type == PlayType::AfterFade { PlayType::Now } else { play_type };
        match mode {
            PlayType::Now => {
                if let Some(c) = self.current {
                    out.push(MusicCmd::Stop(c));
                }
                out.push(MusicCmd::Start(track));
                // The closing handle copy writes ESI: +0x1c (current) on this path, set at
                // 007719df; modes 1 and 2 load +0x24 (next) first. [game, disassembly]
                self.current = Some(track);
            }
            PlayType::AfterFade => {
                self.fade_time = fade_time;
                self.elapsed = 0.0;
                self.state = 1;
                self.next = Some(track);
            }
            PlayType::Crossfade => {
                self.fade_time = fade_time;
                self.elapsed = 0.0;
                self.state = 2;
                self.next = Some(track);
                out.push(MusicCmd::Volume(track, 0.0));
                out.push(MusicCmd::Start(track));
            }
        }
        out
    }

    /// The per-frame part of 00771540. [game]
    pub fn update(&mut self, dt: f32) -> Vec<MusicCmd> {
        let mut out = Vec::new();
        match self.state {
            1 => {
                if let Some(c) = self.current {
                    out.push(MusicCmd::Volume(c, 1.0 - self.elapsed / self.fade_time));
                }
                self.elapsed += dt;
                if self.elapsed < self.fade_time {
                    return out;
                }
                self.state = 0;
                if let Some(c) = self.current {
                    out.push(MusicCmd::Stop(c));
                }
                if let Some(n) = self.next {
                    out.push(MusicCmd::Start(n));
                }
            }
            2 => {
                let k = self.elapsed / self.fade_time;
                if let Some(c) = self.current {
                    out.push(MusicCmd::Volume(c, 1.0 - k));
                }
                if let Some(n) = self.next {
                    out.push(MusicCmd::Volume(n, k));
                }
                self.elapsed += dt;
                if self.elapsed < self.fade_time {
                    return out;
                }
                self.state = 0;
                if let Some(c) = self.current {
                    out.push(MusicCmd::Stop(c));
                }
                if let Some(n) = self.next {
                    out.push(MusicCmd::Volume(n, 1.0));
                }
            }
            _ => return out,
        }
        // A zero fade time gives 0/0 for one frame in the game (a NaN volume handed to
        // FMOD, then the switch). [assumed: the port drops non-finite volumes] No playable robot
        // reaches this: MusicQueue has no caller outside its tests (2026-10-20 grep; Wave 18 the same).
        // settles: the game's music fade caller (the function that owns the queue, not ported; read
        // its 0/0 path there) if music is ported. Not a robot question.
        out.retain(|c| !matches!(c, MusicCmd::Volume(_, v) if !v.is_finite()));
        self.current = self.next.take();
        out
    }
}

/// SetMusicVolume and its fade on the music system's volume (00773de0, 00599070). [game]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MusicVolume {
    pub volume: f32,
    pub target: f32,
    pub remaining: f32,
}

impl Default for MusicVolume {
    /// The sound system's start values: target 1, no fade (00596430). [game]
    fn default() -> Self {
        Self { volume: 1.0, target: 1.0, remaining: 0.0 }
    }
}

impl MusicVolume {
    /// `volume` over `duration` seconds; at once when the duration is not above 0.
    pub fn set(&mut self, volume: f32, duration: f32) {
        self.target = volume;
        self.remaining = duration;
        if duration <= 0.0 {
            self.volume = volume;
        }
    }

    /// Per frame: moves the dt / remaining part of the way, or lands on the target when the
    /// frame covers what is left. [game]
    pub fn update(&mut self, dt: f32) {
        if self.remaining <= 0.0 {
            return;
        }
        self.volume = if dt >= self.remaining {
            self.target
        } else {
            (self.target - self.volume) * (dt / self.remaining) + self.volume
        };
        self.remaining -= dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(category: &str, volume: f32, din: f32, dout: f32) -> AudioPresetEntry {
        AudioPresetEntry {
            category: category.into(),
            volume,
            volume_duration: din,
            volume_out_duration: dout,
            pitch: 0.0,
            pitch_duration: din,
            pitch_out_duration: dout,
            pause: false,
            life_span: -1.0,
            fall_off_min: 0.0,
            fall_off_max: 0.0,
        }
    }

    #[test]
    fn duck_fades_in_holds_and_fades_out() {
        let e = entry("FX", 0.5, 1.0, 2.0);
        let mut c = CategoryMix::new("FX");
        c.apply(7, 0.5, &e, false);
        let v = c.update(0.5).unwrap().volume;
        assert!((v - 0.75).abs() < 1e-6, "half way in: {v}");
        let v = c.update(0.5).unwrap().volume;
        assert!((v - 0.5).abs() < 1e-6);
        assert_eq!(c.slots[0].phase, Phase::Hold);
        c.update(10.0);
        assert_eq!(c.slots[0].phase, Phase::Hold, "lifeSpan -1 holds");
        c.release(7);
        c.update(0.0); // Hold sees count 0: to Out
        assert_eq!(c.slots[0].phase, Phase::Out);
        let v = c.update(1.0).unwrap().volume;
        assert!((v - 0.75).abs() < 1e-6, "half way out over 2 s: {v}");
        c.update(1.0);
        assert_eq!(c.slots[0].phase, Phase::Free);
        let out = c.update(0.1).unwrap();
        assert_eq!(out.volume, 1.0);
        assert!(!c.dirty);
        assert_eq!(c.update(0.1), None);
    }

    #[test]
    fn two_presets_multiply_and_pitch_adds() {
        let mut a = entry("music", 0.5, 0.01, -1.0);
        a.pitch = 0.1;
        let mut b = entry("music", 0.5, 0.01, -1.0);
        b.pitch = -0.25;
        let mut c = CategoryMix::new("music");
        c.apply(1, 0.5, &a, false);
        c.apply(2, 0.5, &b, false);
        let out = c.update(1.0).unwrap();
        assert!((out.volume - 0.25).abs() < 1e-6);
        assert!((out.pitch + 0.15).abs() < 1e-6);
        assert!((raw_pitch_multiplier(0.25) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn life_span_and_pause_count() {
        let mut e = entry("VO", 0.0, 0.01, 0.01);
        e.pause = true;
        e.life_span = 1.0;
        let mut c = CategoryMix::new("VO");
        c.apply(3, 0.0, &e, false);
        c.apply(3, 0.0, &e, false);
        assert_eq!(c.slots[0].count, 2);
        assert_eq!(c.pause_count, 2);
        assert!(c.update(0.02).unwrap().paused);
        c.update(0.5);
        c.update(0.6); // held 1.1 > 1.0: out, count zeroed, pause count left as read
        assert_eq!(c.slots[0].phase, Phase::Out);
        assert_eq!(c.pause_count, 2);
        c.clear();
        assert_eq!(c.pause_count, 2, "clear subtracts the zeroed count (0)");
        assert_eq!(c.slots[0].phase, Phase::Free);
    }

    #[test]
    fn full_slots_ignore_a_ninth_preset() {
        let e = entry("UI", 0.5, 1.0, 1.0);
        let mut c = CategoryMix::new("UI");
        for id in 0..9 {
            c.apply(id, 0.5, &e, false);
        }
        assert!(c.slots.iter().all(|s| s.id != 8));
    }

    #[test]
    fn fall_off_by_distance() {
        let mut e = entry("FX", 0.2, 1.0, 1.0);
        e.fall_off_min = 10.0;
        e.fall_off_max = 30.0;
        assert_eq!(entry_volume(&e, None), Some(0.2));
        assert_eq!(entry_volume(&e, Some(31.0)), None);
        assert!((entry_volume(&e, Some(10.0)).unwrap() - 0.2).abs() < 1e-6);
        assert!((entry_volume(&e, Some(20.0)).unwrap() - 0.6).abs() < 1e-6);
        assert!((entry_volume(&e, Some(30.0)).unwrap() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn music_now_after_fade_and_crossfade() {
        let mut q = MusicQueue::default();
        assert_eq!(q.play(1, PlayType::AfterFade, 2.0), vec![MusicCmd::Start(1)], "nothing plays: Now");
        assert_eq!(q.current, Some(1));
        assert!(q.play(2, PlayType::AfterFade, 2.0).is_empty());
        assert_eq!(q.update(1.0), vec![MusicCmd::Volume(1, 1.0)]);
        assert_eq!(q.update(1.0), vec![MusicCmd::Volume(1, 0.5), MusicCmd::Stop(1), MusicCmd::Start(2)]);
        assert_eq!((q.current, q.next, q.state), (Some(2), None, 0));
        assert_eq!(q.play(3, PlayType::Crossfade, 1.0), vec![MusicCmd::Volume(3, 0.0), MusicCmd::Start(3)]);
        assert_eq!(q.update(0.5), vec![MusicCmd::Volume(2, 1.0), MusicCmd::Volume(3, 0.0)]);
        assert_eq!(
            q.update(0.5),
            vec![MusicCmd::Volume(2, 0.5), MusicCmd::Volume(3, 0.5), MusicCmd::Stop(2), MusicCmd::Volume(3, 1.0)]
        );
        assert_eq!(q.current, Some(3));
        assert_eq!(q.play(4, PlayType::Now, 0.0), vec![MusicCmd::Stop(3), MusicCmd::Start(4)]);
        q.play(5, PlayType::AfterFade, 0.0);
        assert_eq!(q.update(0.032), vec![MusicCmd::Stop(4), MusicCmd::Start(5)], "zero fade: NaN volume dropped");
    }

    #[test]
    fn music_volume_fade() {
        let mut m = MusicVolume::default();
        m.set(0.0, 1.0);
        m.update(0.5);
        assert!((m.volume - 0.5).abs() < 1e-6);
        m.update(0.25);
        assert!((m.volume - 0.25).abs() < 1e-6);
        m.update(0.5);
        assert_eq!(m.volume, 0.0);
        m.set(0.8, 0.0);
        assert_eq!(m.volume, 0.8);
    }

    #[test]
    fn install_presets() {
        let dir = PathBuf::from(crate::install::dir_string());
        if !dir.join("bnxglobal.str").is_file() {
            return;
        }
        let presets = load_presets(&dir).unwrap();
        let entries: usize = presets.iter().map(|p| p.entries.len()).sum();
        assert_eq!(entries, 77, "tf2_lxb dump of bnxglobal.str#18 counts 77 entries");
        let pause = crate::formats::hash::crc32(b"SystemPausePreset");
        let p = preset(&presets, pause).expect("SystemPausePreset by its name hash");
        assert!(p.entry("FX").is_some_and(|e| e.pause) && p.entry("UI").is_some_and(|e| !e.pause));
        // The first preset (ca824e2e, name not in names.tsv) pauses music; UI stays on.
        let first = &presets[0];
        assert_eq!(first.id, 0xca82_4e2e);
        assert!(first.entry("music").is_some_and(|e| e.pause) && first.entry("UI").is_some_and(|e| !e.pause));
        assert_eq!(preset(&presets, 0x39539434).and_then(|p| p.entry("music")).map(|e| e.volume), Some(0.6));
        assert!(presets.iter().all(|p| p.id != 0), "every preset has a name hash");
        // Optimus's and Powered-Up Optimus's special plays the preset `OptimusSpecial` (0072afd0 case
        // 0 / 1 through 01194410 = the audio manager and 00771230 = audio_preset_play).
        let optimus = preset(&presets, crate::formats::hash::crc32(b"OptimusSpecial"));
        println!("OptimusSpecial preset: {:?}", optimus.map(|p| p.entries.iter().map(|e| (e.category.clone(), e.volume, e.pitch, e.pause)).collect::<Vec<_>>()));
    }
}
