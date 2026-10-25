//! Reverb: FMOD Ex's system reverb as the game sets it up, read from the executable.
//!
//! What the game does [game]: a zone's ReverbTrigger (00770b20 / 00770b60) sets FMOD's ambient
//! reverb to one of the event system's reverb presets by index (00598bd0 ->
//! EventSystemI::getReverbPresetByIndex 004d5e90 -> setReverbAmbientProperties 004d5fe0); a
//! ReverbArea (loader 00770c50, activate 00770da0 -> 00598c30) would make a 3D reverb at a point
//! with a min and max distance (createReverb 004d5f30, set3DAttributes, setProperties). Each
//! FMOD update morphs the 3D reverbs around the listener with the ambient (004305d0, weights
//! 0041f630, sums 0041f810, normalised 0041f9a0) and gives the result to the one physical reverb
//! (0042f740 -> System::setReverbProperties 0041ed70), the SFX Reverb DSP
//! (`lib\sfx\foreverb\aSfxDsp.cpp`: wrapper 00475930 / 00475d40, parameter setters 004765c0 ..
//! 00476f20, process 0047e300). Every channel sends into it at its event's reverb wet level.
//!
//! What the data holds [data]: the presets are `global.fev`'s 20 (`Fev::reverbs`): "Off" and
//! "Generic" .. "UnderWater", but only "Off" to "Bathroom" (0..4) carry their own values; 5..19
//! ("Stone Room", "City", "Mountains", "Plain", "Parking Lot" ...) are all Generic's. No pack
//! of the install holds a ReverbArea, so there is never anything to morph: each zone has one
//! ambient for its whole length, set at spawn or never (`zone_reverb`). Every event of the
//! install has reverb wet and dry levels of 0 dB.
//!
//! Tags as in `sound.rs`.

use std::f32::consts::{PI, SQRT_2, TAU};

use glam::Vec3;

use crate::formats::fev::ReverbProperties;
use crate::mission_script::{self, ObjectId, Script};

/// A level in millibels as a gain. [game: 00476b60 / 00476c70] `10^(mB * 0.01 / 20)`.
pub fn millibels(mb: f32) -> f32 {
    10f32.powf(mb * 0.01 / 20.0)
}

/// A level in decibels as a gain (an event's reverb wet and dry levels, which FMOD takes as
/// millibels x 100: 004c7be0 / 004c7b70 -> the channel's Room and Direct).
pub fn decibels(db: f32) -> f32 {
    millibels(db * 100.0)
}

/// [game: 00476660] The one-pole low-pass `y = (1 - a) x + a y[-1]` whose gain at `hertz` is
/// `gain`: FMOD's damping and RoomHF filter. 0 for a gain of 1 or more; at most 0.99.
pub fn one_pole(gain: f32, hertz: f32, rate: f32) -> f32 {
    if gain >= 1.0 {
        return 0.0;
    }
    let g2 = gain * gain;
    let q = g2 - 1.0;
    let b = 2.0 * (1.0 - (hertz * TAU / rate).cos() * g2);
    let disc = (b * b - q * 4.0 * q).max(0.0);
    let a = (disc.sqrt() - b) / (q + q);
    if a >= 0.0 { a.min(0.99) } else { 0.0 }
}

/// [game: 004767e0] The RoomLF shelf's biquad (b0, b1, b2, -a1, -a2) for `db` at `hertz`.
fn shelf(db: f32, hertz: f32, rate: f32) -> [f32; 5] {
    let k = (hertz * PI / rate).tan();
    // 0.057564627 = ln 10 / 40: the square root of the shelf's gain.
    let v = (db * 0.057_564_627).exp();
    let kv = k / v;
    let norm = 1.0 / (kv * (kv + SQRT_2) + 1.0);
    [
        norm * ((k + SQRT_2) * k + 1.0),
        2.0 * (k * k - 1.0) * norm,
        ((k - SQRT_2) * k + 1.0) * norm,
        (kv * kv - 1.0) * -2.0 * norm,
        -(((kv - SQRT_2) * kv + 1.0) * norm),
    ]
}

/// The SFX Reverb's parameters (FMOD_DSP_SFXREVERB 0..14), as `System::setReverbProperties`
/// (0041ed70) hands a reverb's properties to it: levels in millibels, times in seconds,
/// diffusion and density in percent, references in hertz.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SfxParams {
    pub dry_level: f32,
    pub room: i32,
    pub room_hf: i32,
    pub room_rolloff: f32,
    pub decay_time: f32,
    pub decay_hf_ratio: f32,
    pub reflections: i32,
    pub reflections_delay: f32,
    pub reverb: i32,
    pub reverb_delay: f32,
    pub diffusion: f32,
    pub density: f32,
    pub hf_reference: f32,
    pub room_lf: i32,
    pub lf_reference: f32,
}

impl Default for SfxParams {
    /// [game: 00475930] the wrapper's defaults. DryLevel -100000 mB (`+0x75c`): the system
    /// reverb adds only its wet signal.
    fn default() -> Self {
        Self {
            dry_level: -100000.0,
            room: -10000,
            room_hf: -10000,
            room_rolloff: 0.0,
            decay_time: 1.0,
            decay_hf_ratio: 1.0,
            reflections: -2602,
            reflections_delay: 0.007,
            reverb: 200,
            reverb_delay: 0.011,
            diffusion: 0.0,
            density: 0.0,
            hf_reference: 5000.0,
            room_lf: 0,
            lf_reference: 250.0,
        }
    }
}

impl SfxParams {
    /// [game: 0041ed70] the properties' clamps and the parameters they set (1 Room, 2 RoomHF,
    /// 3 RoomRolloffFactor, 4 DecayTime, 5 DecayHFRatio, 6 Reflections, 7 ReflectionsDelay,
    /// 8 Reverb, 9 ReverbDelay, 10 Diffusion, 11 Density, 12 HFReference, 13 RoomLF,
    /// 14 LFReference); the dry level is not among them.
    pub fn from_properties(p: &ReverbProperties) -> Self {
        Self {
            dry_level: Self::default().dry_level,
            room: p.room.clamp(-10000, 0),
            room_hf: p.room_hf.clamp(-10000, 0),
            room_rolloff: p.room_rolloff_factor.clamp(0.0, 10.0),
            decay_time: p.decay_time.clamp(0.1, 20.0),
            decay_hf_ratio: p.decay_hf_ratio.clamp(0.1, 2.0),
            reflections: p.reflections.clamp(-10000, 1000),
            reflections_delay: p.reflections_delay.clamp(0.0, 0.3),
            reverb: p.reverb.clamp(-10000, 2000),
            reverb_delay: p.reverb_delay.clamp(0.0, 0.1),
            diffusion: p.diffusion.clamp(0.0, 100.0),
            density: p.density.clamp(0.0, 100.0),
            hf_reference: p.hf_reference.clamp(1000.0, 20000.0),
            room_lf: p.room_lf.clamp(-10000, 0),
            lf_reference: p.lf_reference.clamp(20.0, 1000.0),
        }
    }
}

/// A value the DSP moves to across one block. [game: 0047e300] at a block's start the target
/// becomes the value; when it differs from the last block's, each sample n of the N takes
/// `(n new + (N - n) old) / N`.
#[derive(Clone, Copy, Debug)]
struct Glide {
    target: f32,
    now: f32,
    last: f32,
    then: f32,
    moving: bool,
}

impl Glide {
    fn new(value: f32) -> Self {
        Self { target: value, now: value, last: value, then: value, moving: false }
    }

    fn begin(&mut self) {
        self.now = self.target;
        self.moving = self.last != self.now;
        if self.moving {
            self.then = self.last;
            self.last = self.now;
        }
    }

    fn at(&self, new: f32, old: f32, inv: f32) -> f32 {
        if self.moving { (new * self.now + old * self.then) * inv } else { self.now }
    }
}

/// A delay line of a power-of-two size, read and written at positions that step down one a
/// sample (`(p - 1) & mask`), as all of the SFX Reverb's lines are. [game: 0047e300]
#[derive(Clone, Debug)]
struct Ring {
    data: Vec<f32>,
    mask: usize,
}

impl Ring {
    /// [assumed] a size that holds the longest delay the parameters allow at `rate`; the
    /// allocators (0047ddb0, 0047de80, 0047df60, 0047e030) are not read. Only a delay longer
    /// than the game's buffer would differ. settles: those allocators' sizes.
    fn new(seconds: f32, rate: f32) -> Self {
        let size = ((seconds * rate).ceil() as usize + 2).next_power_of_two();
        Self { data: vec![0.0; size], mask: size - 1 }
    }

    fn step(&self, at: &mut usize) -> usize {
        *at = at.wrapping_sub(1) & self.mask;
        *at
    }

    fn tap(&self, write: usize, length: usize) -> usize {
        (write + length).wrapping_sub(1) & self.mask
    }
}

/// One of the eight late lines (the DSP's arrays at +0x2d4 .. +0x41c).
#[derive(Clone, Debug)]
struct Line {
    ring: Ring,
    write: usize,
    length: usize,
    last_length: usize,
    feedback: usize,
    feedback_old: usize,
    out_offset: usize,
    out: usize,
    moved: bool,
    gain: Glide,
    damping: Glide,
    state: f32,
}

/// Blocks the DSP works in. [assumed] FMOD Ex's default DSP buffer, 1024 samples (the game's
/// `setDSPBufferSize`, if any, is not read). It only sets how long a change of parameters glides.
/// settles: the game's System init (FMOD's dspbuffersize).
pub const BLOCK: usize = 1024;

/// The longest time a line's delay may take at any parameters, for the buffers' sizes.
const LONGEST_PREDELAY: f32 = 0.3 + 0.1 + 0.0187 * 1.29 * 1.29 * 1.29 * 1.29 * 1.29 * 1.29 + 0.01;
const LONGEST_LINE: f32 = 0.061 * 1.32 * 1.32 * 1.32 * 1.32 * 1.32 * 1.32 * 1.32 + 0.01;

/// FMOD Ex's SFX Reverb (an I3DL2 reverb) as the executable runs it, on a stereo signal.
/// [game: 0047d860 init, 0047e300 process, setters 004765c0 .. 00476f20]
///
/// The signal: both channels summed (0047f320); two one-pole low-passes for RoomHF; the RoomLF
/// shelf when RoomLF is not 0; a pre-delay line. Early part: one tap at ReflectionsDelay, times
/// the reflections' gain, through two all-passes (0.0059 s and 0.0077 s, gain 0.63) into a line
/// tapped seven times (0.005 s x 1.59^k). Late part: eight lines (0.061 s x Density's factor x
/// 1.32^i) fed from pre-delay taps (ReflectionsDelay + ReverbDelay, then + 0.0187 s x 1.29^k)
/// times the reverb's gain, each with its decay gain and one-pole HF damping, mixed by three
/// stages of rotations by `tan(Diffusion x pi / 4)` (normalised by cos^3). Each output channel
/// is four early taps and four lines read at 0.0015 s x 1.47^i, alternating left and right.
#[derive(Clone, Debug)]
pub struct SfxReverb {
    rate: f32,
    params: SfxParams,
    dry: f32,
    input: Glide,
    lowpass: [f32; 2],
    shelf_on: bool,
    shelf: [f32; 5],
    shelf_state: [f32; 2],
    reflections: Glide,
    reverb: Glide,
    rotation: Glide,
    norm: Glide,
    predelay: Ring,
    predelay_write: usize,
    taps: [usize; 9],
    taps_last: [usize; 9],
    taps_at: [usize; 9],
    taps_old: [usize; 9],
    taps_moved: [bool; 9],
    allpass: [Ring; 2],
    allpass_write: [usize; 2],
    allpass_read: [usize; 2],
    early: Ring,
    early_write: usize,
    early_read: [usize; 7],
    lines: Vec<Line>,
    times: [f32; 8],
}

/// [game: 0047d860] the all-passes' gain (`+0x2cc`).
const ALLPASS_GAIN: f32 = 0.63;

impl SfxReverb {
    pub fn new(rate: f32) -> Self {
        let line = Line {
            ring: Ring::new(LONGEST_LINE, rate),
            write: 0,
            length: 1,
            last_length: 1,
            feedback: 0,
            feedback_old: 0,
            out_offset: 1,
            out: 0,
            moved: false,
            gain: Glide::new(0.0),
            damping: Glide::new(0.0),
            state: 0.0,
        };
        // [game: 0047f7c0] the all-passes' lengths, fixed at the rate: their reads start
        // length - 1 ahead of their writes. [assumed] the writes start at 0 (the allocators
        // are not read; only the difference matters).
        let allpass_lengths = [(0.0059 * rate) as usize, (0.0077 * rate) as usize];
        // [game: 0047f810] the early line's seven taps, 0.005 s x 1.59^k.
        let mut early_read = [0; 7];
        let mut t = 0.005f32;
        for read in &mut early_read {
            *read = ((t * rate) as usize).wrapping_sub(1);
            t *= 1.59;
        }
        let early = Ring::new(t, rate);
        for read in &mut early_read {
            *read &= early.mask;
        }
        let mut reverb = Self {
            rate,
            params: SfxParams::default(),
            dry: 0.0,
            input: Glide::new(0.0),
            lowpass: [0.0; 2],
            shelf_on: false,
            shelf: [0.0; 5],
            shelf_state: [0.0; 2],
            reflections: Glide::new(0.0),
            reverb: Glide::new(0.0),
            rotation: Glide::new(0.0),
            norm: Glide::new(0.0),
            predelay: Ring::new(LONGEST_PREDELAY, rate),
            predelay_write: 0,
            taps: [1; 9],
            taps_last: [1; 9],
            taps_at: [0; 9],
            taps_old: [0; 9],
            taps_moved: [false; 9],
            allpass: [Ring::new(0.0059, rate), Ring::new(0.0077, rate)],
            allpass_write: [0; 2],
            allpass_read: [allpass_lengths[0].wrapping_sub(1), allpass_lengths[1].wrapping_sub(1)],
            early,
            early_write: 0,
            early_read,
            lines: vec![line; 8],
            times: [0.0; 8],
        };
        for (read, ring) in reverb.allpass_read.iter_mut().zip(&reverb.allpass) {
            *read &= ring.mask;
        }
        reverb.derive();
        reverb.settle();
        reverb
    }

    /// As if the DSP had run long enough with these values for every glide to be over.
    fn settle(&mut self) {
        for glide in [&mut self.input, &mut self.reflections, &mut self.reverb, &mut self.rotation, &mut self.norm] {
            *glide = Glide::new(glide.target);
        }
        for line in &mut self.lines {
            line.gain = Glide::new(line.gain.target);
            line.damping = Glide::new(line.damping.target);
            line.last_length = line.length;
        }
        self.taps_last = self.taps;
    }

    pub fn params(&self) -> SfxParams {
        self.params
    }

    /// Sets every parameter. [game] the wrapper's update (00475d40) calls each setter whose
    /// value changed. [assumed] recomputing everything in dependency order gives the state the
    /// setters' chain does (the density setter 00476a40 changes the lines' times, which the decay
    /// gains 004768d0 use: if the game does not recompute those after it, a change of Density
    /// alone keeps the old gains for the new lengths). settles: whether 00476a40 or 00475d40
    /// calls 004768d0 after it.
    pub fn set(&mut self, params: SfxParams) {
        if params != self.params {
            self.params = params;
            self.derive();
        }
    }

    /// [game] the setters' clamps and what each computes.
    fn derive(&mut self) {
        let rate = self.rate;
        let p = &mut self.params;
        // 0118a2c0 / 004765c0 / 00476b60 / 00476c70 / 00476760: levels.
        p.room = p.room.clamp(-10000, 0);
        p.room_hf = p.room_hf.clamp(-10000, 0);
        p.reflections = p.reflections.clamp(-10000, 1000);
        p.reverb = p.reverb.clamp(-10000, 2000);
        p.room_lf = p.room_lf.clamp(-10000, 0);
        // 004768d0 / 00476b00 / 00476c00 / 00476dc0 / 00476e40 / 00476ed0 / 00476f20 / 00476760.
        p.decay_time = p.decay_time.clamp(0.1, 10000.0);
        p.decay_hf_ratio = p.decay_hf_ratio.clamp(0.1, 2.0);
        p.reflections_delay = p.reflections_delay.clamp(0.0, 0.3);
        p.reverb_delay = p.reverb_delay.clamp(0.0, 0.1);
        p.diffusion = p.diffusion.clamp(0.0, 100.0);
        p.density = p.density.clamp(0.0, 100.0);
        p.hf_reference = p.hf_reference.clamp(20.0, 20000.0);
        p.lf_reference = p.lf_reference.clamp(20.0, 1000.0);
        let p = *p;
        self.dry = 10f32.powf(p.dry_level / 2000.0);
        // 00476c00: the reflections' tap; 00476dc0 -> 0047f740: the eight late lines' taps.
        self.taps[0] = ((rate * p.reflections_delay) as usize).max(1);
        let start = p.reverb_delay + p.reflections_delay;
        self.taps[1] = ((start as f64 * rate as f64) as usize).max(1);
        let mut spacing = 0.0187f32;
        for tap in &mut self.taps[2..] {
            let time = start + spacing;
            *tap = (time as f64 * rate as f64) as usize;
            spacing *= 1.29;
        }
        // 00476a40 -> 0047f860: the lines' lengths and output taps from the density.
        let d = p.density * 0.01;
        let factor = (d * d * d * 0.9 + 0.1).clamp(0.1, 1.0);
        let mut time = factor * 0.061;
        let mut out = 0.0015 * rate;
        for (line, t) in self.lines.iter_mut().zip(&mut self.times) {
            *t = time;
            line.length = ((time * rate) + 0.5) as usize;
            line.out_offset = (out + 0.5) as usize;
            time *= 1.32;
            out *= 1.47;
        }
        // 004768d0: each line's decay gain and HF damping.
        for (line, &t) in self.lines.iter_mut().zip(&self.times) {
            let low = (-60.0 / p.decay_time) * t;
            line.gain.target = 10f32.powf(low / 20.0);
            let high = 10f32.powf(((-60.0 / (p.decay_hf_ratio * p.decay_time)) * t - low) / 20.0);
            line.damping.target = 1.0 - one_pole(high, p.hf_reference, rate);
        }
        // 00476b60: the reflections' gain, over the eight early taps.
        self.reflections.target = millibels((p.room + p.reflections) as f32) * 0.125f32.sqrt();
        // 00476c70: the reverb's gain, corrected for the lines' mean square decay gain.
        let gain = millibels((p.room + p.reverb) as f32);
        let mean = self.lines.iter().map(|l| l.gain.target * l.gain.target).sum::<f32>() / 8.0;
        let spread = if mean == 1.0 { 1.0 } else { mean / (1.0 - mean) + 1.0 };
        let gain = if mean != 1.0 && spread <= 0.0 { gain } else { gain / spread.sqrt() };
        self.reverb.target = gain * 0.125f32.sqrt();
        // 004765c0: RoomHF as two equal one-poles.
        self.input.target = 1.0 - one_pole(10f32.powf(p.room_hf as f32 * 0.01 * 0.5 / 20.0), p.hf_reference, rate);
        // 00476e40: the rotations; cos to the power log2(8) = 3 (`+0x6d8`, 00475a80).
        let angle = p.diffusion * 0.01 * PI * 0.25;
        self.rotation.target = angle.tan();
        self.norm.target = angle.cos().powi(3);
        // 00476760: RoomLF.
        let db = p.room_lf as f32 / 100.0;
        self.shelf_on = db != 0.0;
        self.shelf = shelf(db, p.lf_reference, rate);
    }

    /// Adds the reverb of `input` (stereo, interleaved) to `out`. [game: 0047e300]
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) {
        for (input, out) in input.chunks(BLOCK * 2).zip(out.chunks_mut(BLOCK * 2)) {
            self.block(input, out);
        }
    }

    fn begin(&mut self) {
        let write = self.predelay_write;
        for k in 0..9 {
            self.taps_at[k] = self.predelay.tap(write, self.taps[k]);
            self.taps_moved[k] = self.taps[k] != self.taps_last[k];
            if self.taps_moved[k] {
                self.taps_old[k] = self.predelay.tap(write, self.taps_last[k]);
                self.taps_last[k] = self.taps[k];
            }
        }
        for line in &mut self.lines {
            line.feedback = line.ring.tap(line.write, line.length);
            line.out = line.ring.tap(line.write, line.out_offset);
            line.moved = line.length != line.last_length;
            if line.moved {
                line.feedback_old = line.ring.tap(line.write, line.last_length);
                line.last_length = line.length;
            }
            // A change of decay gain crossfades the line as a change of length does.
            line.gain.now = line.gain.target;
            if line.gain.last == line.gain.now {
                line.gain.then = line.gain.now;
            } else {
                if !line.moved {
                    line.moved = true;
                    line.feedback_old = line.ring.tap(line.write, line.last_length);
                }
                line.gain.then = line.gain.last;
                line.gain.last = line.gain.now;
            }
            line.damping.begin();
        }
        self.reverb.begin();
        self.reflections.begin();
        self.input.begin();
        self.rotation.begin();
        self.norm.begin();
    }

    fn block(&mut self, input: &[f32], out: &mut [f32]) {
        let frames = input.len() / 2;
        if frames == 0 {
            return;
        }
        self.begin();
        let inv = 1.0 / frames as f32;
        let mut late = [0f32; 8];
        let mut early = [0f32; 8];
        for n in 0..frames {
            let (new, old) = (n as f32, (frames - n) as f32);
            let c = self.input.at(new, old, inv);
            let rotation = self.rotation.at(new, old, inv);
            let norm = self.norm.at(new, old, inv);
            // [assumed] FMOD's +-1e-15 denormal bias on the input is left out: inaudible.
            let x = input[n * 2] + input[n * 2 + 1];
            self.lowpass[0] = c * x + (1.0 - c) * self.lowpass[0];
            self.lowpass[1] = (1.0 - c) * self.lowpass[1] + self.lowpass[0] * c;
            let mut y = self.lowpass[1];
            if self.shelf_on {
                let [b0, b1, b2, a1, a2] = self.shelf;
                let filtered = b0 * y + self.shelf_state[0];
                self.shelf_state[0] = b1 * y + self.shelf_state[1] + filtered * a1;
                self.shelf_state[1] = a2 * filtered + b2 * y;
                y = filtered;
            }
            let write = self.predelay.step(&mut self.predelay_write);
            self.predelay.data[write] = y;
            // Early: the reflections' tap through the two all-passes into the early line.
            let reflections = self.reflections.at(new, old, inv);
            let at = self.predelay.step(&mut self.taps_at[0]);
            let mut e = reflections * self.predelay.data[at];
            if self.taps_moved[0] {
                let at = self.predelay.step(&mut self.taps_old[0]);
                e = (new * e + old * reflections * self.predelay.data[at]) * inv;
            }
            let w = self.allpass[0].step(&mut self.allpass_write[0]);
            let r = self.allpass[0].step(&mut self.allpass_read[0]);
            let first = self.allpass[0].data[r] - e * ALLPASS_GAIN;
            self.allpass[0].data[w] = first * ALLPASS_GAIN + e;
            let w = self.allpass[1].step(&mut self.allpass_write[1]);
            let r = self.allpass[1].step(&mut self.allpass_read[1]);
            early[0] = self.allpass[1].data[r] - ALLPASS_GAIN * first;
            self.allpass[1].data[w] = first + early[0] * ALLPASS_GAIN;
            let w = self.early.step(&mut self.early_write);
            self.early.data[w] = early[0];
            for k in 0..7 {
                let at = self.early.step(&mut self.early_read[k]);
                early[k + 1] = self.early.data[at];
            }
            // Late: each line's own delayed output and its pre-delay tap, damped.
            let gain = self.reverb.at(new, old, inv);
            for (i, line) in self.lines.iter_mut().enumerate() {
                let at = self.predelay.step(&mut self.taps_at[i + 1]);
                let mut tap = self.predelay.data[at];
                if self.taps_moved[i + 1] {
                    let at = self.predelay.step(&mut self.taps_old[i + 1]);
                    tap = (new * tap + old * self.predelay.data[at]) * inv;
                }
                let at = line.ring.step(&mut line.feedback);
                let fed = if line.moved {
                    let was = line.ring.step(&mut line.feedback_old);
                    (old * line.ring.data[was] * line.gain.then + new * line.ring.data[at] * line.gain.now) * inv
                } else {
                    line.ring.data[at] * line.gain.now
                };
                let d = line.damping.at(new, old, inv);
                line.state = (1.0 - d) * line.state + norm * (fed + gain * tap) * d;
                late[i] = line.state;
                line.ring.step(&mut line.write);
            }
            // The three stages of rotations, written back one line along.
            let v = late;
            let r = rotation;
            let s1 = [
                v[0] + r * v[1], v[1] - r * v[0], v[2] + r * v[3], v[3] - r * v[2],
                v[4] + r * v[5], v[5] - r * v[4], v[6] + r * v[7], v[7] - r * v[6],
            ];
            let s2 = [
                s1[2] * r + s1[0], s1[3] * r + s1[1], s1[2] - s1[0] * r, s1[3] - s1[1] * r,
                s1[6] * r + s1[4], s1[7] * r + s1[5], s1[6] - s1[4] * r, s1[7] - s1[5] * r,
            ];
            let s3 = [
                s2[4] * r + s2[0], s2[5] * r + s2[1], s2[6] * r + s2[2], s2[7] * r + s2[3],
                s2[4] - s2[0] * r, s2[5] - s2[1] * r, s2[6] - s2[2] * r, s2[7] - s2[3] * r,
            ];
            for (i, line) in self.lines.iter_mut().enumerate() {
                let w = line.write;
                line.ring.data[w] = s3[(i + 1) % 8];
            }
            // Output: channel c takes lines and early taps c, c + 2, c + 4, c + 6.
            for channel in 0..2 {
                let mut sum = 0.0;
                for k in 0..4 {
                    let line = &mut self.lines[channel + 2 * k];
                    let at = line.ring.step(&mut line.out);
                    sum += line.ring.data[at] + early[channel + 2 * k];
                }
                out[n * 2 + channel] += input[n * 2 + channel] * self.dry + sum;
            }
        }
    }

    /// Empties every line (the reset 0047e1e0 / 0047e210).
    pub fn clear(&mut self) {
        self.predelay.data.fill(0.0);
        self.early.data.fill(0.0);
        for ring in &mut self.allpass {
            ring.data.fill(0.0);
        }
        for line in &mut self.lines {
            line.ring.data.fill(0.0);
            line.state = 0.0;
        }
        self.lowpass = [0.0; 2];
        self.shelf_state = [0.0; 2];
    }
}

/// A 3D reverb (what a ReverbArea makes): full inside `min_distance` of its position, nothing
/// beyond `max_distance`. [game: 00770da0 -> 00598c30]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reverb3d {
    pub position: Vec3,
    pub min_distance: f32,
    pub max_distance: f32,
    pub properties: ReverbProperties,
    pub active: bool,
}

/// [game: 0041f630] A 3D reverb's weight at the listener: 1 within its min distance, falling
/// linearly to 0 at its max. (The same function gives physical reverbs `10^(-3 f)` instead;
/// the game's are virtual and take the linear one, 004305d0.) [assumed] the reverb's "3D set"
/// flag (`+0x231`) is up once it has a position. settles: FMOD's Reverb::set3DAttributes.
pub fn weight(area: &Reverb3d, listener: Vec3) -> f32 {
    let d = listener.distance(area.position);
    if d <= area.min_distance {
        1.0
    } else if d < area.max_distance {
        let range = area.max_distance - area.min_distance;
        if range > 0.0 { 1.0 - (d - area.min_distance) / range } else { 1.0 }
    } else {
        0.0
    }
}

/// The morph's sums (0041f810's accumulator).
#[derive(Default)]
struct Sums {
    room: f32,
    room_hf: f32,
    room_lf: f32,
    decay: f32,
    decay_hf: f32,
    reflections: f32,
    reflections_delay: f32,
    reverb: f32,
    reverb_delay: f32,
    hf_reference: f32,
    lf_reference: f32,
    rolloff: f32,
    diffusion: f32,
    density: f32,
}

impl Sums {
    /// [game: 0041f810] levels are summed as `exp(mB x 0.002)`, references as their logarithm.
    fn add(&mut self, p: &ReverbProperties, w: f32) {
        let level = |mb: i32| (mb as f32 * 0.002).exp() * w;
        self.room += level(p.room);
        self.room_hf += level(p.room_hf);
        self.room_lf += level(p.room_lf);
        self.rolloff += p.room_rolloff_factor * w;
        self.decay += p.decay_time * w;
        self.decay_hf += w * p.decay_hf_ratio;
        self.reflections += level(p.reflections);
        self.reflections_delay += w * p.reflections_delay;
        self.reverb += level(p.reverb);
        self.reverb_delay += p.reverb_delay * w;
        self.diffusion += p.diffusion * w;
        self.density += w * p.density;
        if p.hf_reference > 0.0 {
            self.hf_reference += p.hf_reference.ln() * w;
        }
        if p.lf_reference > 0.0 {
            self.lf_reference += p.lf_reference.ln() * w;
        }
    }

    /// [game: 0041f9a0] back to properties, everything else 0. A level is
    /// `ln(sum) x 500 x norm`, rounded by +0.5 and truncation (-10000 for an empty sum).
    fn properties(&self, norm: f32) -> ReverbProperties {
        let level = |sum: f32| if sum <= 0.0 { -10000 } else { (sum.ln() * 500.0 * norm + 0.5) as i32 };
        ReverbProperties {
            instance: 0,
            environment: 0,
            env_size: 0.0,
            env_diffusion: 0.0,
            room: level(self.room),
            room_hf: level(self.room_hf),
            room_lf: level(self.room_lf),
            decay_time: self.decay * norm,
            decay_hf_ratio: self.decay_hf * norm,
            decay_lf_ratio: 0.0,
            reflections: level(self.reflections),
            reflections_delay: self.reflections_delay * norm,
            reflections_pan: [0.0; 3],
            reverb: level(self.reverb),
            reverb_delay: self.reverb_delay * norm,
            reverb_pan: [0.0; 3],
            echo_time: 0.0,
            echo_depth: 0.0,
            modulation_time: 0.0,
            modulation_depth: 0.0,
            air_absorption_hf: 0.0,
            hf_reference: (norm * self.hf_reference).exp(),
            lf_reference: (self.lf_reference * norm).exp(),
            room_rolloff_factor: self.rolloff * norm,
            diffusion: self.diffusion * norm,
            density: self.density * norm,
            flags: 0,
        }
    }
}

/// [game: 004305d0] What the physical reverb is given: the active 3D reverbs around the
/// listener (each with a weight over 0.001) and the ambient for the weight they leave. An
/// ambient of Environment -1 (Off) adds only `exp(-20)` to Room (no other property), so a
/// listener half inside an area under an Off ambient hears the area's times halved.
/// [assumed] FMOD's geometry occlusion of each 3D reverb (004173e0) is 0: the port gives FMOD
/// no geometry. settles: whether the game adds any (System::createGeometry callers).
pub fn morph(listener: Vec3, ambient: &ReverbProperties, areas: &[Reverb3d]) -> ReverbProperties {
    let mut sums = Sums::default();
    let mut total = 0.0;
    for area in areas.iter().filter(|a| a.active) {
        let w = weight(area, listener);
        if w > 0.001 {
            sums.add(&area.properties, w);
            total += w;
        }
    }
    if total < 1.0 {
        if ambient.environment == -1 {
            sums.room += (-20f32).exp() * (1.0 - total);
        } else {
            sums.add(ambient, 1.0 - total);
        }
        total = 1.0;
    }
    sums.properties(1.0 / total)
}

/// The system reverb: the ambient and 3D reverbs the game has set, the listener, and the DSP
/// they drive. Every channel's reverb send is summed into one signal for it.
#[derive(Clone, Debug)]
pub struct ReverbSystem {
    /// Set by a ReverbTrigger (`ambient_preset`). None until one fires: [game] FMOD then never
    /// runs the morph (004305d0's flag `+0xa264`) and its reverb stays at the DSP's defaults
    /// (Room -10000: silent), which the port skips.
    pub ambient: Option<ReverbProperties>,
    pub areas: Vec<Reverb3d>,
    /// The listener's position in the world (the camera's, 00771540).
    pub listener: Vec3,
    dsp: SfxReverb,
    /// What the DSP was last given (None: never).
    pub applied: Option<ReverbProperties>,
    wet: Vec<f32>,
    /// The wet signal's sum of squares and frames since `take_wet_rms` (the trace's measure).
    wet_energy: f64,
    wet_frames: usize,
}

impl ReverbSystem {
    pub fn new(rate: f32) -> Self {
        Self { ambient: None, areas: Vec::new(), listener: Vec3::ZERO, dsp: SfxReverb::new(rate), applied: None, wet: Vec::new(), wet_energy: 0.0, wet_frames: 0 }
    }

    /// [game: 00770b20 / 00770b60 -> 00598bd0] a ReverbTrigger's Trigger sets the ambient to
    /// its preset, its Untrigger to preset 0; a trigger of preset 0 does nothing either way.
    pub fn trigger(&mut self, presets: &[ReverbProperties], preset: u32) {
        if preset != 0
            && let Some(p) = presets.get(preset as usize) {
            self.ambient = Some(*p);
        }
    }

    pub fn untrigger(&mut self, presets: &[ReverbProperties], preset: u32) {
        if preset != 0
            && let Some(p) = presets.first() {
            self.ambient = Some(*p);
        }
    }

    /// What the morph gives now (None while nothing is set).
    pub fn current(&self) -> Option<ReverbProperties> {
        if self.ambient.is_none() && self.areas.is_empty() {
            return None;
        }
        // [assumed] with areas but no ambient set, the ambient is FMOD's default, Off.
        let ambient = self.ambient.unwrap_or_default();
        Some(morph(self.listener, &ambient, &self.areas))
    }

    /// Adds the reverb of `send` (stereo, interleaved: every channel's send summed) to `out`.
    /// [assumed] the morph runs once a call with the listener of then (FMOD runs it in each
    /// System::update; the port's output callback is about as often). Returns whether the
    /// properties changed.
    pub fn render(&mut self, send: &[f32], out: &mut [f32]) -> bool {
        let Some(now) = self.current() else { return false };
        let changed = self.applied != Some(now);
        if changed {
            self.dsp.set(SfxParams::from_properties(&now));
            self.applied = Some(now);
        }
        self.wet.clear();
        self.wet.resize(out.len(), 0.0);
        let mut wet = std::mem::take(&mut self.wet);
        self.dsp.process(send, &mut wet);
        for (o, w) in out.iter_mut().zip(&wet) {
            *o += w;
        }
        self.wet_energy += wet.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>();
        self.wet_frames += wet.len() / 2;
        self.wet = wet;
        changed
    }

    /// The wet signal's RMS (both channels) since the last call, and the frames it covers.
    pub fn take_wet_rms(&mut self) -> (f32, usize) {
        let frames = self.wet_frames;
        let rms = if frames > 0 { (self.wet_energy / (frames * 2) as f64).sqrt() as f32 } else { 0.0 };
        self.wet_energy = 0.0;
        self.wet_frames = 0;
        (rms, frames)
    }
}

/// A zone's ReverbTrigger and whether the zone's spawn fires it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneTrigger {
    /// `2c5fe432`: the preset's index. [data; game 00770b20 reads it at +0x44]
    pub preset: u32,
    /// The delay of the TriggerOnSpawn link that fires it, None when no spawn link reaches it
    /// with a target (us_test's link has a null target, mp_skyscraper's trigger no sender).
    pub at_spawn: Option<f32>,
}

/// CRC-32 of the ReverbTrigger's preset field (`2c5fe432`).
const PRESET: u32 = 0x2c5f_e432;

/// Every ReverbTrigger in a zone's script. [game] a link's message other than Untrigger,
/// Disable or Enable goes to the receiver's trigger (the Receiver dispatch); a link with no
/// resolved target is skipped. [assumed] only TriggerOnSpawn links count as firing at the
/// stage's start: other senders (the war rooms' BGPlayer `trigWhenStarted`) belong to the
/// music, which the port leaves off.
pub fn zone_triggers(script: &Script) -> Vec<ZoneTrigger> {
    let mut found = Vec::new();
    for (i, object) in script.objects.iter().enumerate() {
        if object.type_name != "ReverbTrigger" {
            continue;
        }
        let Some(preset) = script
            .node(ObjectId(i))
            .and_then(|n| n.fields().into_iter().find(|(h, _)| *h == PRESET))
            .and_then(|(_, v)| v.int())
        else {
            continue;
        };
        let mut chain = vec![ObjectId(i)];
        let mut at = object.parent;
        while let Some(p) = at {
            chain.push(p);
            at = script.object(p).and_then(|x| x.parent);
        }
        let at_spawn = script
            .objects
            .iter()
            .filter(|s| s.type_name == "TriggerOnSpawn")
            .flat_map(|s| s.links.iter().flat_map(|site| site.array.0.iter()))
            .filter(|l| l.target.is_some_and(|t| chain.contains(&t)))
            .filter(|l| ![mission_script::MSG_UNTRIGGER, mission_script::MSG_DISABLE, mission_script::MSG_ENABLE].contains(&l.msg_type))
            .map(|l| l.delay_min)
            .reduce(f32::max);
        found.push(ZoneTrigger { preset: preset as u32, at_spawn });
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::fev::Fev;

    const RATE: f32 = 48000.0;

    fn presets() -> Vec<ReverbProperties> {
        let data = std::fs::read(crate::install::dir().join("audio/eng/global.fev")).expect("global.fev");
        Fev::parse(&data).expect("parse").reverbs.iter().map(|r| r.properties).collect()
    }

    fn generic() -> ReverbProperties {
        presets()[1]
    }

    fn impulse(reverb: &mut SfxReverb, seconds: f32) -> Vec<f32> {
        let frames = (seconds * RATE) as usize;
        let mut input = vec![0.0; frames * 2];
        input[0] = 1.0;
        let mut out = vec![0.0; frames * 2];
        reverb.process(&input, &mut out);
        out
    }

    fn rms(out: &[f32], from: f32, to: f32) -> f32 {
        let span = &out[(from * RATE) as usize * 2..(to * RATE) as usize * 2];
        (span.iter().map(|v| v * v).sum::<f32>() / span.len() as f32).sqrt()
    }

    #[test]
    fn the_one_pole_has_its_gain_at_its_reference() {
        for (gain, hertz) in [(0.5f32, 5000.0f32), (0.9, 2000.0), (0.1, 8000.0)] {
            let a = one_pole(gain, hertz, RATE);
            let w = hertz * TAU / RATE;
            // |(1 - a) / (1 - a e^-jw)|
            let response = (1.0 - a) / ((1.0 - a * w.cos()).powi(2) + (a * w.sin()).powi(2)).sqrt();
            assert!((response - gain).abs() < 1e-3, "gain {gain} at {hertz}: {response} (a {a})");
        }
        assert_eq!(one_pole(1.0, 5000.0, RATE), 0.0);
    }

    #[test]
    fn the_install_presets_past_bathroom_are_all_generic() {
        if crate::install::missing() { return; }
        let p = presets();
        assert_eq!(p.len(), 20);
        assert_eq!(p[0].environment, -1);
        assert_eq!(p[0].room, -10000);
        for (i, preset) in p.iter().enumerate().skip(5) {
            assert_eq!(*preset, p[1], "preset {i}");
        }
        assert_eq!((p[1].room, p[1].room_hf, p[1].decay_time, p[1].reverb), (-1000, -100, 1.49, 200));
    }

    #[test]
    fn the_first_reflection_comes_after_the_reflections_delay() {
        if crate::install::missing() { return; }
        let mut reverb = SfxReverb::new(RATE);
        reverb.set(SfxParams::from_properties(&generic()));
        reverb.settle();
        let out = impulse(&mut reverb, 0.05);
        let first = out.chunks(2).position(|f| f[0] != 0.0 || f[1] != 0.0).unwrap();
        // ReflectionsDelay 0.007 s: tap 336, read 335 samples after the write.
        assert_eq!(first, (0.007 * RATE) as usize - 1);
    }

    #[test]
    fn generic_decays_and_off_is_silent() {
        if crate::install::missing() { return; }
        let mut reverb = SfxReverb::new(RATE);
        reverb.set(SfxParams::from_properties(&generic()));
        reverb.settle();
        let out = impulse(&mut reverb, 2.0);
        let (early, late) = (rms(&out, 0.1, 0.2), rms(&out, 1.1, 1.2));
        let drop = 20.0 * (early / late).log10();
        // A decay time of 1.49 s is 60 dB in 1.49 s: about 40 dB over this second at low
        // frequencies, more with the HF damping (DecayHFRatio 0.83).
        assert!(early > 1e-4, "early {early}");
        assert!((35.0..60.0).contains(&drop), "drop {drop} dB ({early} -> {late})");
        let mut off = SfxReverb::new(RATE);
        off.set(SfxParams::from_properties(&presets()[0]));
        off.settle();
        let out = impulse(&mut off, 0.5);
        assert!(out.iter().all(|v| v.abs() < 1e-4));
    }

    #[test]
    fn a_change_of_preset_glides_without_a_jump() {
        if crate::install::missing() { return; }
        let mut reverb = SfxReverb::new(RATE);
        reverb.set(SfxParams::from_properties(&generic()));
        reverb.settle();
        let frames = BLOCK * 4;
        let input: Vec<f32> = (0..frames * 2).map(|i| ((i / 2) as f32 * 0.05).sin() * 0.5).collect();
        let mut out = vec![0.0; frames * 2];
        reverb.process(&input[..BLOCK * 4], &mut out[..BLOCK * 4]);
        let mut room = presets()[3];
        room.decay_time = 0.4;
        reverb.set(SfxParams::from_properties(&room));
        reverb.process(&input[BLOCK * 4..], &mut out[BLOCK * 4..]);
        let step = out.chunks(2).zip(out.chunks(2).skip(1)).map(|(a, b)| (a[0] - b[0]).abs()).fold(0f32, f32::max);
        assert!(step < 0.2, "largest step {step}");
    }

    fn area(properties: ReverbProperties) -> Reverb3d {
        Reverb3d { position: Vec3::new(10.0, 0.0, 0.0), min_distance: 2.0, max_distance: 6.0, properties, active: true }
    }

    #[test]
    fn the_weight_is_full_inside_min_and_linear_to_max() {
        if crate::install::missing() { return; }
        let a = area(generic());
        assert_eq!(weight(&a, Vec3::new(11.0, 0.0, 0.0)), 1.0);
        assert!((weight(&a, Vec3::new(14.0, 0.0, 0.0)) - 0.5).abs() < 1e-6);
        assert_eq!(weight(&a, Vec3::new(16.0, 0.0, 0.0)), 0.0);
        assert_eq!(weight(&a, Vec3::new(30.0, 0.0, 0.0)), 0.0);
    }

    #[test]
    fn the_morph_follows_the_listener_between_an_area_and_the_ambient() {
        if crate::install::missing() { return; }
        let p = presets();
        let mut bathroom = p[4];
        bathroom.environment = 3;
        let areas = [area(bathroom)];
        // Outside: the ambient (Generic), to within the rounding of 0041f9a0.
        let outside = morph(Vec3::ZERO, &p[1], &areas);
        assert!((outside.room - p[1].room).abs() <= 1 && (outside.decay_time - 1.49).abs() < 1e-5);
        // Inside: the area.
        let inside = morph(Vec3::new(10.0, 0.0, 0.0), &p[1], &areas);
        assert!((inside.reflections - bathroom.reflections).abs() <= 1, "{}", inside.reflections);
        assert!((inside.reverb - 1030).abs() <= 1);
        // Half way: levels blend as exp(mB x 0.002), times linearly.
        let half = morph(Vec3::new(14.0, 0.0, 0.0), &p[1], &areas);
        let blend = (((p[1].reverb as f32 * 0.002).exp() + (1030f32 * 0.002).exp()) * 0.5).ln() * 500.0;
        assert!((half.reverb as f32 - blend).abs() <= 1.0, "{} {blend}", half.reverb);
        assert!((half.decay_time - 1.49).abs() < 1e-5);
        assert!((half.density - 80.0).abs() < 1e-4);
        // Under an Off ambient, the leftover weight adds only to Room: times are halved.
        let off = morph(Vec3::new(14.0, 0.0, 0.0), &p[0], &areas);
        assert!((off.decay_time - 0.745).abs() < 1e-5);
        assert!(morph(Vec3::ZERO, &p[0], &[]).room <= -9999);
    }

    #[test]
    fn the_system_reverb_is_dry_until_a_trigger_and_wet_after() {
        if crate::install::missing() { return; }
        let p = presets();
        let mut system = ReverbSystem::new(RATE);
        let frames = 4800;
        let send: Vec<f32> = (0..frames * 2).map(|i| if (i / 2) % 480 == 0 { 0.5 } else { 0.0 }).collect();
        let mut out = vec![0.0; frames * 2];
        assert!(!system.render(&send, &mut out));
        assert!(out.iter().all(|&v| v == 0.0));
        system.trigger(&p, 0);
        assert!(system.ambient.is_none(), "preset 0 does nothing");
        system.trigger(&p, 13);
        assert!(system.render(&send, &mut out));
        assert!(system.take_wet_rms().0 > 1e-3);
        // A ReverbArea: the wet follows the listener in and out of it.
        system.areas.push(area(p[4]));
        let level = |system: &mut ReverbSystem, out: &mut Vec<f32>, at: Vec3| {
            system.listener = at;
            for _ in 0..20 {
                out.fill(0.0);
                system.render(&send, out);
            }
            system.take_wet_rms();
            out.fill(0.0);
            system.render(&send, out);
            system.take_wet_rms().0
        };
        let away = level(&mut system, &mut out, Vec3::ZERO);
        assert_eq!(system.applied.unwrap().reverb, p[13].reverb);
        let inside = level(&mut system, &mut out, Vec3::new(10.0, 0.0, 0.0));
        assert!((system.applied.unwrap().reverb - p[4].reverb).abs() <= 1);
        // Bathroom (reverb +1030 mB) against City (Generic, +200 mB).
        assert!(inside > away * 1.3, "inside {inside} away {away}");
    }

    #[test]
    fn the_zones_fire_their_presets_at_spawn_or_never() {
        if crate::install::missing() { return; }
        let install = crate::install::dir();
        let fired = |zone: &str| {
            let script = Script::load(install, std::path::Path::new(&format!("levels/{zone}/zone.str"))).expect(zone);
            zone_triggers(&script)
        };
        assert_eq!(fired("cairo_city"), vec![ZoneTrigger { preset: 13, at_spawn: Some(0.1) }]);
        assert_eq!(fired("shanghai_02"), vec![ZoneTrigger { preset: 17, at_spawn: Some(0.0) }]);
        assert_eq!(fired("us_test"), vec![ZoneTrigger { preset: 13, at_spawn: None }]);
        assert_eq!(fired("shanghai_01"), vec![ZoneTrigger { preset: 0, at_spawn: Some(0.0) }]);
    }
}
