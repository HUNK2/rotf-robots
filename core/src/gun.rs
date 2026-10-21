//! The gun in his hand (`notes\weapon-mode.md`, "The weapon layer and the gun's clips"):
//! the clips of his weapon layer (`WeaponPartialSet`: the arm pulling the gun out, the
//! recoil, the change of weapon, the overheat, putting it away) and the clips the weapon's
//! own object plays, which the arm clips' events and the weapon's code start.
//!
//! Read from the original: which message starts which arm clip (`FUN_0084dfa0` and the
//! set's own entry rules on `C_WEAPON_ACTION`), the events (`FUN_007b4680`), the object's
//! clip player (`FUN_007c37e0`, `FUN_007c4170`), the weapon's own starts (its slots
//! `+0x10c` to `+0x120`), and the trigger's gate (`FUN_007b37f0`).
//!
//! The layer's blending is the animation controller's (`FUN_0074a180` starts a layer's
//! clip, `FUN_0074a970` updates the layer, `FUN_0074a610` takes it off): see `Arm::blend`.
//! Not ported: an event for a weapon that is not in hand but shares the object of the one
//! that is (the third branch of `FUN_007b4680`), and the object's own cross-fade.

use crate::animrules::{self, var, Value};
use crate::formats::anim::{Action, AnimRef, Clip, IncomingRule, SoundCue, WeaponEventKind};
use crate::formats::hash::crc32;
use crate::weapons::WeaponDef;

/// The arm's set, the one `PlayCharPartialAnim` names (`0x84ff5f30`). [data]
const WEAPON_PARTIAL_SET: u32 = crc32(b"WeaponPartialSet");

/// Whether an object clip's key at `time` passes in a step from `from` (None: the clip has just
/// started) to `now`; a looping clip that came round the end has `now < from`.
fn key_passes(time: f32, from: Option<f32>, now: f32) -> bool {
    match from {
        None => time <= now,
        Some(from) if now >= from => time > from && time <= now,
        Some(from) => time > from || time <= now,
    }
}

/// Values of `C_WEAPON_ACTION`. [game]
pub mod action {
    use crate::formats::hash::crc32;
    pub const PULL_OUT: u32 = crc32(b"WEAPON_ACTION_PULL_OUT_GUN");
    pub const SWITCH: u32 = crc32(b"WEAPON_ACTION_SWITCH");
    pub const OVERHEAT: u32 = crc32(b"WEAPON_ACTION_OVERHEAT");
    /// What message ab304c20 (a round fired) sets; its name is not in the dictionary.
    pub const RECOIL: u32 = 0xb90a_70bd;
}

/// The two arm clips the trigger does not get past (`FUN_007b37f0`). [game]
const PULL_OUT_CLIP: u32 = crc32(b"Trans_R2W");
const SWITCH_CLIP: u32 = crc32(b"Weapon_Switch");
/// The trigger pulled during one of them ends it over this long (message 409f0e77 sets
/// the layer's blend to 0x3e083127), and the weapon fires once that is over. [game]
const CUT_SHORT: f32 = 0.132_999_99;
const TICKS_PER_SECOND: f32 = 60.0;

/// A clip of the weapon layer as it plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arm {
    /// Which clip of `WeaponPartialSet`.
    pub clip: usize,
    pub tick: f32,
    /// Ticks played since it started, NOT wrapped round a loop: the player's elapsed time
    /// (`+0xe0`, counted on by `FUN_0053cc40`, returned by its slot `+0x14` `0053a2b0`),
    /// which is what the layer's fade in reads. [game] So a loop fades in once; the wrapped
    /// `tick` made every pass of a looping arm clip (the overheat) fade out and in again.
    pub elapsed_ticks: f32,
    /// How much of the clip is laid over his pose, 0 to 1 (for a clip of poses, 1 less
    /// the layer's `BlendAnimation` weight; for one of offsets, its `AdditiveAnimation`'s).
    weight: f32,
    /// The trigger ended it (the layer's `+0x94` is set).
    cut: bool,
    /// A looping clip that was left fades out over this long (the layer's `+0x9c`).
    out: Option<f32>,
    /// Seconds counted against either of the two (the layer's `+0xa4`).
    elapsed: f32,
}

impl Arm {
    pub fn weight(&self) -> f32 {
        self.weight
    }

    /// The layer's update (`FUN_0074a970`), with the clip `t` seconds in (its elapsed time,
    /// not wrapped round a loop: `elapsed_ticks`) and `length` seconds long. Gives false when
    /// the layer is taken off. [game]
    ///
    /// - Cut by the trigger: gone after 0.133 s or when a clip that is not a loop ends;
    ///   until then its share drops with the time since the cut.
    /// - A loop that was left: gone after its `xfadeTimeOnExit`, dropping the same way.
    /// - A clip that is not a loop fades out INSIDE its own last `xfadeTimeOnExit`
    ///   seconds, so it is gone the moment it ends.
    /// - Otherwise, while it is less than `xfadeTime` in, its share is how far in it is.
    ///   Nothing sets the share to 1 when that is over: it stays at the last value the
    ///   fade gave (0.94 for 0.3 s at 60 updates a second).
    /// - A fade out never raises the share.
    fn blend(&mut self, r: &AnimRef, t: f32, length: f32, dt: f32) -> bool {
        let share = |part: f32, of: f32| (part / of).clamp(0.0, 1.0);
        if self.cut {
            self.elapsed += dt;
            if self.elapsed >= CUT_SHORT || (!r.looping && length - t <= 0.0) {
                return false;
            }
            self.weight = self.weight.min(1.0 - share(self.elapsed, CUT_SHORT));
        } else if let Some(out) = self.out {
            self.elapsed += dt;
            if self.elapsed >= out {
                return false;
            }
            self.weight = self.weight.min(1.0 - share(self.elapsed, out));
        } else if r.exit_crossfade > 0.0 && !r.looping && length - t < r.exit_crossfade {
            self.weight = self.weight.min(share(length - t, r.exit_crossfade));
        } else if r.crossfade > 0.0 && t < r.crossfade {
            self.weight = share(t, r.crossfade);
        }
        true
    }
}

/// The weapon object's own clip as it plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectClip {
    /// Its id (CRC-32 of the name).
    pub id: u32,
    pub tick: f32,
    finished: bool,
}

/// The weapon in hand as the weapon events see it (`FUN_007b4680`): its name, its slot in
/// the manager's list (the index an event without a name gives), and its child's, whose
/// events also count while the parent is in hand. [game 007b4680]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hand {
    pub name: u32,
    pub slot: i32,
    pub child: Option<(u32, i32)>,
}

/// A slot no event's `weaponIndex` gives (events give -1 for none).
pub const NO_SLOT: i32 = i32::MIN;

impl Default for Hand {
    fn default() -> Self {
        Self::from(0)
    }
}

impl From<u32> for Hand {
    /// A weapon known only by its name.
    fn from(name: u32) -> Self {
        Self { name, slot: NO_SLOT, child: None }
    }
}

impl Hand {
    /// `I_WEAPON_INDEX` for this weapon in hand (`FUN_0070cae0` -> `FUN_007b51b0`): its
    /// slot, or its child's while the child's turn is on; 0 for a weapon with no slot. [game]
    pub fn index(&self, child_turn: bool) -> i32 {
        let slot = match self.child {
            Some((_, child)) if child_turn && child != NO_SLOT => child,
            _ => self.slot,
        };
        if slot == NO_SLOT { 0 } else { slot }
    }

    /// Whether an event naming `named` (0 for none) at `index` is for this weapon or its
    /// child: by name, or, naming none, by slot (`FUN_007b4680` l.28-29). [game]
    fn takes(&self, named: u32, index: i32) -> (bool, bool) {
        let is = |name: u32, slot: i32| (named != 0 && named == name) || (named == 0 && slot != NO_SLOT && index == slot);
        (is(self.name, self.slot), self.child.is_some_and(|(name, slot)| is(name, slot)))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Gun {
    /// The gun is out (character flag 0x100000).
    pub out: bool,
    action: u32,
    pub arm: Option<Arm>,
    /// A `WeaponEnableFireEvent` has shut the trigger off.
    fire_shut: bool,
    /// The weapon's object is shown, the node of his it is linked to, and what it plays.
    pub shown: bool,
    pub node: u32,
    pub object: Option<ObjectClip>,
    /// When the object's clip ends: the clip to go on to, and whether to hide it.
    then: u32,
    hide_when_done: bool,
    /// Authored clip audio, collected during simulation so render frames cannot skip cues.
    pub sound_commands: Vec<SoundCommand>,
    arm_sound: ClipSound,
    object_sound: ClipSound,
    /// The weapon in hand at the last step, fire or switch: what a pull-out's first events
    /// are matched against.
    hand: Hand,
    /// `I_WEAPON_INDEX`: the manager slot of the weapon whose message started the arm clip
    /// (0 with no weapon known). See `Gun::value`. [game 0070cae0, 007b51b0]
    index: i32,
}

/// Each clip player owns its autoStop sounds independently. [game: AnimPlaySoundEvent]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SoundCommand {
    Changed { layer: usize },
    Cue { layer: usize, cue: SoundCue },
}

#[derive(Clone, Debug, Default)]
struct ClipSound {
    start: bool,
    rerun: bool,
}

impl ClipSound {
    fn changed(&mut self, layer: usize, commands: &mut Vec<SoundCommand>) {
        *self = Self { start: true, rerun: false };
        commands.push(SoundCommand::Changed { layer });
    }

    fn advance(&mut self, layer: usize, r: &AnimRef, from: f32, dt: f32, commands: &mut Vec<SoundCommand>) {
        let Some(clip) = &r.clip else { return };
        let mut first = std::mem::take(&mut self.start);
        let mut from = from;
        let mut to = from + dt * TICKS_PER_SECOND * r.speed;
        let length = clip.length.max(1.0);
        loop {
            let end = to.min(if r.looping { length } else { clip.length });
            for event in &clip.sound_events {
                if ((event.time > from && event.time <= end) || (first && event.time == 0.0))
                    && (!self.rerun || event.rerun)
                {
                    commands.push(SoundCommand::Cue { layer, cue: event.cue });
                }
            }
            if !r.looping || to < length { break; }
            to -= length;
            from = 0.0;
            first = true;
            self.rerun = true;
        }
    }
}

/// The clips the gun's rules run on, from his pack.
#[derive(Clone, Copy)]
pub struct Clips<'a> {
    /// `WeaponPartialSet` and its entry rules.
    pub arm: &'a [AnimRef],
    pub entries: &'a [IncomingRule],
    /// The weapon object's own.
    pub object: &'a [AnimRef],
}

fn id_of(r: &AnimRef) -> u32 {
    crc32(r.id.as_bytes())
}

/// One update of a weapon object's clip player (`FUN_007c4170`): a clip that is not a
/// loop and has ended hands over to the clip named for then, and hides the object if it
/// was asked to. Otherwise it holds its end. [game]
fn step_object(object: &mut Option<ObjectClip>, then: &mut u32, hide_when_done: &mut bool, shown: &mut bool, clips: &[AnimRef], dt: f32) {
    let Some(mut playing) = *object else { return };
    if let Some((r, clip)) = clips.iter().find(|r| id_of(r) == playing.id).and_then(|r| r.clip.as_ref().map(|c| (r, c))) {
        playing.tick += dt * TICKS_PER_SECOND * r.speed;
        if r.looping {
            playing.tick %= clip.length.max(1.0);
        } else if playing.tick >= clip.length {
            playing.tick = clip.length;
            playing.finished = true;
        }
    } else {
        playing.finished = true;
    }
    *object = Some(playing);
    if playing.finished {
        if *then != 0 {
            *object = Some(ObjectClip { id: std::mem::take(then), tick: 0.0, finished: false });
        }
        if std::mem::take(hide_when_done) {
            *shown = false;
        }
    }
}

/// The melee weapon in his hand (the axe). It is a weapon of the manager's list like the
/// guns, but one that is never "in hand" (its slot `+0x30` says no), so the events that
/// name it always count (`FUN_007b4680`): the attack clips link it to the node their
/// event sits on (`ObjectAttachment_Hand_L`), show it and start its object's clips
/// (`Trans_R2W` unfolding it, then `Idle`; `Trans_W2R` folding it, hidden when done).
/// Leaving an attack state for anything but another attack state puts it away
/// (`FUN_00878be0` -> `FUN_007b2d80` -> its slot `+0x28`, `FUN_007a67b0`). [game]
#[derive(Clone, Debug, Default)]
pub struct Axe {
    pub shown: bool,
    /// The node of his it is linked to (CRC-32 of its name).
    pub node: u32,
    pub object: Option<ObjectClip>,
    then: u32,
    hide_when_done: bool,
    /// An event has brought it out and nothing has put it away since (weapon `+0x38`).
    in_use: bool,
}

impl Axe {
    /// The weapon events of the clip he plays, from `from` (not counted; `None` when the
    /// clip has just started, and its events at the start count) to `to`, for the weapon
    /// called `name`. An event that names no weapon is for the one at its `weaponIndex`
    /// in the manager's list: the charge attack's put-away event names none and gives
    /// -1, so it does nothing, here as in the original. [game] [data]
    pub fn events(&mut self, clip: &Clip, from: Option<f32>, to: f32, name: u32) {
        for event in clip.weapon_events.iter().filter(|e| e.time <= to && from.is_none_or(|from| e.time > from)) {
            let WeaponEventKind::Attach { weapon, hide, unhide, clip, hide_when_done, then, .. } = event.kind else { continue };
            if weapon != name {
                continue;
            }
            self.node = event.node;
            if hide {
                (self.shown, self.in_use) = (false, false);
            } else if unhide {
                (self.shown, self.in_use) = (true, true);
            }
            if clip != 0 {
                self.object = Some(ObjectClip { id: clip, tick: 0.0, finished: false });
                (self.then, self.hide_when_done) = (0, false);
            }
            if then != 0 {
                self.then = then;
            }
            self.hide_when_done = hide_when_done;
        }
    }

    /// Put away (`FUN_007a67b0`): if it is in use, its object plays `exit_clip` unless
    /// that is the clip it already plays, and is hidden when the clip ends; with no such
    /// clip it is hidden at once. [game]
    pub fn put_away(&mut self, exit_clip: u32) {
        if !std::mem::take(&mut self.in_use) {
            return;
        }
        if exit_clip == 0 {
            self.shown = false;
        } else {
            if self.object.is_none_or(|o| o.id != exit_clip) {
                self.object = Some(ObjectClip { id: exit_clip, tick: 0.0, finished: false });
                self.then = 0;
            }
            self.hide_when_done = true;
        }
    }

    /// Advances its object's clip by `dt` seconds.
    pub fn step(&mut self, clips: &[AnimRef], dt: f32) {
        step_object(&mut self.object, &mut self.then, &mut self.hide_when_done, &mut self.shown, clips, dt);
    }
}

impl Gun {
    /// How much of the arm clip is laid over his pose, 0 to 1.
    pub fn arm_weight(&self) -> f32 {
        self.arm.map_or(0.0, |arm| arm.weight)
    }

    /// What the weapon layer's rules ask about the gun: `C_WEAPON_ACTION`, the action last
    /// sent, and `I_WEAPON_INDEX`, the manager slot of the weapon that sent it. Anything
    /// else is not answered (it compares as 0). [game: `FUN_0070c900` for the action;
    /// `FUN_0070cae0` for the index: 0 with no weapon in hand, else `FUN_007b51b0`]
    fn value(&self, variable: u32) -> Option<Value> {
        match variable {
            var::WEAPON_ACTION => Some(Value::Hash(self.action)),
            var::WEAPON_INDEX => Some(Value::Int(self.index)),
            _ => None,
        }
    }

    /// Walks the set's entry rules with the action as it stands and starts the clip the
    /// first fitting one names (`FUN_0084ddb0` with no clip, on layer 1). [game]
    fn start_by_rules(&mut self, clips: Clips) {
        let fits = |entry: &&IncomingRule| entry.rules.iter().all(|rule| animrules::holds(rule, self.value(rule.variable)));
        if let Some(entry) = clips.entries.iter().find(fits) {
            self.start_arm(clips, entry.to_id);
        }
    }

    /// A `PlayCharPartialAnim` clip event's ask (`sim::State::partial_requests`): the named clip
    /// of the arm's set starts on its layer (`0084ddb0` with a clip named, layer 1), as Ratchet's
    /// ground punch and Long Haul's special start `Weapon_Charging`. [game 007baa00] A clip the
    /// set does not hold starts nothing. [game 0084ddb0]
    pub fn play_partial(&mut self, clips: Clips, id: u32) {
        self.start_arm(clips, id);
    }

    fn start_arm(&mut self, clips: Clips, id: u32) {
        if let Some(clip) = clips.arm.iter().position(|r| id_of(r) == id && r.clip.is_some()) {
            // A layer's clip takes the layer over at once, whatever played there, and
            // starts at no share if it has a blend time and in full if not
            // (`FUN_0074a180`). [game]
            let weight = if clips.arm[clip].crossfade > 0.0 { 0.0 } else { 1.0 };
            self.arm = Some(Arm { clip, tick: 0.0, elapsed_ticks: 0.0, weight, cut: false, out: None, elapsed: 0.0 });
            self.arm_sound.changed(0, &mut self.sound_commands);
            self.arm_events(clips, clip, None, 0.0, Hand::from(0));
        }
    }

    /// Starts a clip on the weapon's object (`FUN_007c37e0`): `unless_playing` leaves a
    /// clip that is already the one playing alone.
    fn play(&mut self, id: u32, then: u32, hide_when_done: bool, unless_playing: bool) {
        if unless_playing && self.object.is_some_and(|o| o.id == id) {
            return;
        }
        if id != 0 {
            self.object = Some(ObjectClip { id, tick: 0.0, finished: false });
            self.object_sound.changed(1, &mut self.sound_commands);
            (self.then, self.hide_when_done) = (0, false);
        }
        if then != 0 {
            self.then = then;
        }
        self.hide_when_done = hide_when_done;
    }

    /// The arm clip's events from `from` (not counted, or counted when `None`: the clip
    /// has just started) to `to`, for the weapon in hand.
    fn arm_events(&mut self, clips: Clips, clip: usize, from: Option<f32>, to: f32, weapon: Hand) {
        let Some(data) = clips.arm.get(clip).and_then(|r| r.clip.as_ref()) else { return };
        for event in data.weapon_events.iter().filter(|e| e.time <= to && from.is_none_or(|from| e.time > from)) {
            match event.kind {
                WeaponEventKind::EnableFire(allow) => self.fire_shut = !allow,
                WeaponEventKind::Attach { weapon: named, index, only_if_current, hide, unhide, clip, hide_when_done, then } => {
                    // "Only if in use": the slot must be the one in hand, or a child of it
                    // (its slot `+0xdc` gives the parent in hand). [game 007b4680 l.30, 112-118]
                    // The child's object shares this state (its show, hide and clips follow the
                    // parent's), so its event does not move the node.
                    let (current, child) = weapon.takes(named, index);
                    if only_if_current && !current && !child {
                        continue;
                    }
                    if current || !child {
                        self.node = event.node;
                    }
                    if hide {
                        self.shown = false;
                    } else if unhide {
                        self.shown = true;
                    }
                    self.play(clip, then, hide_when_done, false);
                }
            }
        }
    }

    /// He enters a weapon state from one that is neither a weapon state nor the dodge
    /// (`FUN_0087f4a0`): if the gun is not out, the arm pulls it out. `from_car`: the set
    /// his base layer played until now is `Vehicle2RobotSet` (he comes straight out of
    /// the change from the car). Then no arm clip plays: the weapon in hand is shown at
    /// once (its slot `+0x24` with 1, `FUN_007a7630`, which also starts its idle clip, as
    /// an unhiding event with no clip does), and the trigger works at once. [game]
    pub fn take_aim(&mut self, clips: Clips, weapon: &WeaponDef, from_car: bool) {
        if self.out {
            return;
        }
        self.out = true;
        if from_car {
            self.shown = true;
            self.play(weapon.idle_clip, 0, false, false);
        } else {
            self.action = action::PULL_OUT;
            self.start_by_rules(clips);
            self.run_start_events(clips, self.hand);
        }
    }

    /// A weapon that always appears is shown from its start, with no clip: the Gun's slot
    /// `+0x10` (`007a7150`) ends by unhiding the object when `alwaysAppear` (weapon `+0x3c4`)
    /// is set, the three lines of `weapon_show` 007a7630. [game] [assumed] that slot runs once
    /// at the component's start (its caller is unread); shown from the start either way. The port calls `appear` at the arsenal's start (`game/src/weapons.rs` l.818) and in the arsenal's reset (l.2678).
    /// Read: `007ac270` (the weapon's init) calls `007a7150` first, unconditionally, then sets
    /// the weapon's flags (`+0x340`). The init is slot 4 (`+0x10`) of the gun and launcher
    /// vtable `00b5fba0` (found by `find_ptr`, 2026-10-20; its ctor is `007ab580`; slot 2 is
    /// `007ac370`, slot 7 `007a75a0`). settles: the caller of vtable slot `+0x10` on a weapon
    /// object (the component start walk over the scene's components, not read; a decompile grep
    /// for `+ 0x10))(` finds about 60 functions, none yet checked as the weapon's).
    pub fn appear(&mut self, weapon: &WeaponDef) {
        if weapon.always_appear {
            self.shown = true;
        }
    }

    /// The charge started (`007adae0` -> slot `+0x154`, `007b03c0`): PlayAnim on the weapon's
    /// object with `chargingAnimation`, `animOnDone` `chargedAnimation`, no hide, not "unless
    /// playing", crossfade -1, so Charging plays once and hands over to Charged (which loops).
    /// Nothing stops them: the release's recoil starts the object's fire clip through the arm's
    /// events, as for a tap. [game 007b03c0-007b0403] The child's object follows its parent's
    /// clips (`step` plays the same ids on both).
    pub fn charge_started(&mut self, weapon: &WeaponDef) {
        if weapon.charging_clip != 0 {
            self.play(weapon.charging_clip, weapon.charged_clip, false, false);
        }
    }

    /// He leaves the weapon states (`FUN_0087f6f0` -> the manager's `FUN_007b2eb0` -> the
    /// weapon's slot `+0x110`, `FUN_007ab1b0`): with an `exitAnimation`, his arm plays
    /// `exitCharacterAnimation` and the object the exit clip, hidden when that is done
    /// unless it always appears. [game]
    pub fn put_away(&mut self, clips: Clips, weapon: &WeaponDef) {
        if !std::mem::take(&mut self.out) {
            return;
        }
        self.action = 0;
        if weapon.exit_clip == 0 {
            return;
        }
        if weapon.exit_arm_clip != 0 {
            self.start_arm(clips, weapon.exit_arm_clip);
        }
        self.play(weapon.exit_clip, 0, !weapon.always_appear, true);
    }

    /// A round left the weapon (`FUN_007ae010` sends ab304c20): in weapon mode the arm
    /// recoils, and the recoil's event starts the object's firing clip. [game]
    pub fn fired(&mut self, clips: Clips, weapon: impl Into<Hand>) {
        self.fired_on_turn(clips, weapon, false);
    }

    /// The same, saying whose turn it was: with `child_turn` the weapon in hand is an
    /// alternating pair's parent whose turn counter (`+0x39c`) is 1, so the weapon that fired
    /// is its child, and `I_WEAPON_INDEX` is the child's slot (`FUN_007b51b0`; the child is
    /// not in the manager's list under any other circumstance, so then the index falls back
    /// to the manager's current one, the parent's). [game 007b51b0, 007ae010 l.51-71, 434]
    pub fn fired_on_turn(&mut self, clips: Clips, weapon: impl Into<Hand>, child_turn: bool) {
        let weapon = weapon.into();
        self.hand = weapon;
        self.index = weapon.index(child_turn);
        if self.out {
            self.action = action::RECOIL;
            self.start_by_rules(clips);
            self.run_start_events(clips, weapon);
        }
    }

    /// The weapon in hand was changed (message f800db01). [game]
    pub fn switched(&mut self, clips: Clips, weapon: impl Into<Hand>) {
        let weapon = weapon.into();
        self.hand = weapon;
        self.index = weapon.index(false);
        if self.out {
            self.action = action::SWITCH;
            self.start_by_rules(clips);
            self.run_start_events(clips, weapon);
        }
    }

    /// The weapon overheated (`FUN_007a9ec0`: its slot `+0x120` starts the object's
    /// `overheatAnimation`; message 68479e46 starts the arm's). [game]
    pub fn overheated(&mut self, clips: Clips, weapon: &WeaponDef) {
        self.play(weapon.overheat_clip, 0, false, false);
        if self.out {
            self.action = action::OVERHEAT;
            self.start_by_rules(clips);
        }
    }

    /// The overheat is over (`FUN_007a9700`: slot `+0x118` starts `exitOverheatAnimation`;
    /// message c29eb8df clears the action, which the arm clip's exit rule waits for). [game]
    pub fn cooled(&mut self, weapon: &WeaponDef) {
        self.play(weapon.exit_overheat_clip, 0, false, false);
        if self.action == action::OVERHEAT {
            self.action = 0;
        }
    }

    /// The events at a clip's very start need the weapon in hand, which `start_arm` does
    /// not know: run them again with it.
    fn run_start_events(&mut self, clips: Clips, weapon: Hand) {
        if let Some(arm) = self.arm.filter(|arm| arm.tick == 0.0) {
            self.arm_events(clips, arm.clip, None, 0.0, weapon);
        }
    }

    /// Whether the trigger is kept from the weapon this update (`FUN_007b37f0`): while
    /// the arm pulls the gun out or changes it, while such a clip that the trigger ended
    /// fades, and while an event has shut the trigger off. A trigger held during one of
    /// the two clips, with fire allowed, ends the clip. [game]
    pub fn holds_trigger(&mut self, clips: Clips, trigger: bool) -> bool {
        let busy = self.arm.filter(|arm| !arm.cut).and_then(|arm| clips.arm.get(arm.clip)).map(id_of);
        let busy = matches!(busy, Some(PULL_OUT_CLIP | SWITCH_CLIP));
        let cutting = self.arm.is_some_and(|arm| arm.cut);
        if trigger && busy && !self.fire_shut {
            if let Some(arm) = &mut self.arm {
                (arm.cut, arm.elapsed) = (true, 0.0);
            }
            return true;
        }
        busy || cutting || self.fire_shut
    }

    /// Advances both players by `dt` seconds; `weapon` is the name of the one in hand.
    pub fn step(&mut self, clips: Clips, weapon: impl Into<Hand>, dt: f32) {
        let weapon = weapon.into();
        self.hand = weapon;
        if let Some(mut arm) = self.arm.take() {
            let keep = match clips.arm.get(arm.clip).and_then(|r| r.clip.as_ref().map(|clip| (r, clip))) {
                None => false,
                Some((r, clip)) => {
                    // The clip plays on while it fades, whatever ends it.
                    let before = arm.tick;
                    self.arm_sound.advance(0, r, before, dt, &mut self.sound_commands);
                    arm.tick += dt * TICKS_PER_SECOND * r.speed;
                    let finished = !r.looping && arm.tick >= clip.length;
                    arm.tick = if r.looping { arm.tick % clip.length.max(1.0) } else { arm.tick.min(clip.length) };
                    // A clip that is not a loop clamps its elapsed time at the end (its overrun
                    // goes to `+0xe4`); a loop's runs on. [game 0053cc40]
                    arm.elapsed_ticks = if r.looping { arm.elapsed_ticks + dt * TICKS_PER_SECOND * r.speed } else { arm.tick };
                    if arm.tick >= before {
                        self.arm_events(clips, arm.clip, Some(before), arm.tick, weapon);
                    }
                    // Its exit rules: once it has finished, or at any time, while the
                    // rules on the action hold. All of his lead to the empty set, which
                    // is `FUN_0074a180` with no clip: a LOOP with an `xfadeTimeOnExit`
                    // fades out over it, anything else is off at once. [game]
                    let leaves = arm.out.is_none()
                        && r.exits.iter().any(|exit| {
                            (finished || !exit.finish) && exit.rules.iter().all(|rule| animrules::holds(rule, self.value(rule.variable)))
                        });
                    let fades = r.looping && r.exit_crossfade > 0.0;
                    if leaves && fades {
                        (arm.out, arm.elapsed) = (Some(r.exit_crossfade), 0.0);
                    }
                    (!leaves || fades) && arm.blend(r, arm.elapsed_ticks / TICKS_PER_SECOND, clip.length / TICKS_PER_SECOND, dt)
                }
            };
            self.arm = keep.then_some(arm);
            if !keep {
                self.sound_commands.push(SoundCommand::Changed { layer: 0 });
            }
        }
        if let Some(object) = self.object {
            if let Some(r) = clips.object.iter().find(|r| id_of(r) == object.id) {
                self.object_sound.advance(1, r, object.tick, if object.finished { 0.0 } else { dt }, &mut self.sound_commands);
            }
        }
        let object_before = self.object;
        let before = self.object.map(|o| o.id);
        step_object(&mut self.object, &mut self.then, &mut self.hide_when_done, &mut self.shown, clips.object, dt);
        if before != self.object.map(|o| o.id) {
            self.object_sound.changed(1, &mut self.sound_commands);
        }
        // The weapon object's own clip asks for an arm clip on the way (`PlayCharPartialAnim`:
        // the charged launchers' `WeaponB_Charging`, Ratchet's at keys 88 and 178, Long Haul's at
        // 108 and 214, starts `Weapon_Charging`; that object clip is the weapon's
        // `chargingAnimation`, started by `charge_started`).
        // [game 007baa00] [data] A clip that has just started (a new id) passes its events from
        // its first tick. The clips that carry it loop (Long Haul's and Ratchet's, animID
        // -85286767, ANIM_FLAG_LOOP, 240 and 180 ticks) and the keys have no allowReRun, so the game
        // runs them on every pass (0053ab90) [data, game; wave 28 P1]: a step that comes round the
        // end passes the keys after the old tick and those up to the new one. [assumed: the game's
        // window across the end is the same; its caller 00544a50 is not read]
        if let Some(now) = self.object {
            let from = object_before.filter(|old| old.id == now.id).map(|old| old.tick);
            let asks: Vec<u32> = clips.object.iter().find(|r| id_of(r) == now.id).and_then(|r| r.clip.as_ref()).map_or_else(Vec::new, |clip| {
                clip.action_events
                    .iter()
                    .filter(|e| key_passes(e.time, from, now.tick))
                    .filter_map(|e| match e.action {
                        Action::PartialAnim { set, id } if set == WEAPON_PARTIAL_SET => Some(id),
                        _ => None,
                    })
                    .collect()
            });
            for id in asks {
                self.start_arm(clips, id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::anim::{Clip, ExitRule, Rule, WeaponEvent};
    use crate::weapons::Kind;
    use glam::Vec3;

    const CANNON: u32 = 1;
    const NODE: u32 = 77;
    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn sound_cues_include_start_end_and_loop_tail_without_repeating_first_pass_only_keys() {
        use crate::formats::anim::SoundEvent;
        let cue = |time, id, rerun| SoundEvent { time, cue: SoundCue::Play { id, auto_stop: true }, rerun };
        let mut r = AnimRef { speed: 1.0, looping: true,
            clip: Some(Clip { length: 10.0, sound_events: vec![cue(0.0, 1, true), cue(2.0, 2, false), cue(9.0, 3, true)], ..Default::default() }),
            ..Default::default() };
        let mut tracker = ClipSound::default();
        let mut commands = Vec::new();
        tracker.changed(1, &mut commands);
        tracker.advance(1, &r, 0.0, 8.0 / 60.0, &mut commands);
        tracker.advance(1, &r, 8.0, 5.0 / 60.0, &mut commands);
        let ids: Vec<_> = commands.iter().filter_map(|c| match c {
            SoundCommand::Cue { cue: SoundCue::Play { id, .. }, .. } => Some(*id), _ => None,
        }).collect();
        assert_eq!(ids, [1, 2, 3, 1]);
        commands.clear();
        r.looping = false;
        tracker.changed(1, &mut commands);
        tracker.advance(1, &r, 0.0, 20.0 / 60.0, &mut commands);
        assert_eq!(commands.len(), 4, "the whole final interval survives an overshot clip end");
        commands.clear();
        tracker.advance(1, &r, 10.0, 1.0 / 60.0, &mut commands);
        assert!(commands.is_empty(), "a held end pose must stay silent");
    }

    fn id(name: &str) -> u32 {
        crc32(name.as_bytes())
    }

    fn on_action(hash: u32, equal: bool) -> Vec<Rule> {
        vec![Rule { variable: var::WEAPON_ACTION, logic: if equal { 0 } else { 1 }, hash, ..Default::default() }]
    }

    fn attach(time: f32, clip: &str, unhide: bool) -> WeaponEvent {
        let kind = WeaponEventKind::Attach {
            weapon: CANNON,
            index: -1,
            only_if_current: true,
            hide: false,
            unhide,
            clip: id(clip),
            hide_when_done: false,
            then: 0,
        };
        WeaponEvent { time, node: NODE, kind }
    }

    fn fire(time: f32, allow: bool) -> WeaponEvent {
        WeaponEvent { time, node: NODE, kind: WeaponEventKind::EnableFire(allow) }
    }

    /// His weapon layer's five clips with the lengths, fades, rules and events of his pack.
    fn arm_clips() -> (Vec<AnimRef>, Vec<IncomingRule>) {
        let clip = |name: &str, length: f32, fade: f32, looping: bool, exits: Vec<ExitRule>, events: Vec<WeaponEvent>| AnimRef {
            id: name.into(),
            looping,
            crossfade: fade,
            exit_crossfade: fade,
            speed: 1.0,
            exits,
            clip: Some(Clip { length, weapon_events: events, ..Default::default() }),
            ..Default::default()
        };
        let done = || vec![ExitRule { finish: true, ..Default::default() }];
        let not_overheat = vec![ExitRule { finish: false, rules: on_action(action::OVERHEAT, false), ..Default::default() }];
        let clips = vec![
            clip("Weapon_Recoil", 8.0, -1.0, false, done(), vec![attach(0.0, "FireA", false)]),
            clip("Weapon_Switch", 98.0, 0.3, false, done(), vec![fire(0.0, false), attach(2.0, "SwitchToWeaponA", true), fire(8.0, true)]),
            clip("Weapon_Overheat", 160.0, 0.3, true, not_overheat, vec![]),
            clip("Trans_R2W", 84.0, 0.3, false, done(), vec![fire(0.0, false), attach(4.0, "Trans_R2W_WeaponA", true), fire(8.0, true)]),
            clip("Trans_W2R", 48.0, 0.2, false, done(), vec![]),
        ];
        let entry = |to: &str, action: u32| IncomingRule { to_id: id(to), rules: on_action(action, true), ..Default::default() };
        let entries = vec![
            entry("Trans_R2W", action::PULL_OUT),
            entry("Weapon_Switch", action::SWITCH),
            entry("Weapon_Overheat", action::OVERHEAT),
            entry("Weapon_Recoil", action::RECOIL),
        ];
        (clips, entries)
    }

    fn object_clips() -> Vec<AnimRef> {
        let clip = |name: &str, length: f32, looping: bool| AnimRef {
            id: name.into(),
            looping,
            speed: 1.0,
            clip: Some(Clip { length, ..Default::default() }),
            ..Default::default()
        };
        vec![
            clip("Trans_R2W_WeaponA", 30.0, false),
            clip("Trans_W2R_WeaponA", 20.0, false),
            clip("FireA", 6.0, false),
            clip("Idle_WeaponA", 40.0, true),
            clip("Overheat_Weapon_A", 60.0, true),
            clip("SwitchToWeaponA", 30.0, false),
        ]
    }

    fn cannon() -> WeaponDef {
        WeaponDef {
            kind: Kind::Gun,
            name: CANNON,
            attribute: 0,
            bone: 0,
            fire_sound: 0,
            overheat_sound: 0,
            refused_sound: 0,
            muzzle: Vec3::ZERO,
            barrels: [Vec3::ZERO; 3],
            barrel_count: 1,
            idle_clip: id("Idle_WeaponA"),
            exit_clip: id("Trans_W2R_WeaponA"),
            exit_overheat_clip: id("Idle_WeaponA"),
            overheat_clip: id("Overheat_Weapon_A"),
            exit_arm_clip: id("Trans_W2R"),
            always_appear: false,
            explosion: None,
            flame: [crate::weapons::FlameSphere::default(); 3],
            flame_count: 0,
            ..WeaponDef::test(Kind::Gun)
        }
    }

    fn run(gun: &mut Gun, clips: Clips, ticks: usize) {
        for _ in 0..ticks {
            gun.step(clips, CANNON, DT);
        }
    }

    #[test]
    fn taking_aim_pulls_the_gun_out_and_the_event_at_tick_4_shows_it() {
        let (arm, entries) = arm_clips();
        let object = object_clips();
        let clips = Clips { arm: &arm, entries: &entries, object: &object };
        let mut gun = Gun::default();
        gun.take_aim(clips, &cannon(), false);
        assert_eq!(gun.arm.map(|a| arm[a.clip].id.as_str()), Some("Trans_R2W"));
        assert!(!gun.shown);
        // The trigger is shut off from the clip's start.
        assert!(gun.holds_trigger(clips, false));
        run(&mut gun, clips, 3);
        assert!(!gun.shown);
        run(&mut gun, clips, 2);
        assert!(gun.shown, "shown by the event at tick 4");
        assert_eq!(gun.node, NODE);
        assert_eq!(gun.object.map(|o| o.id), Some(id("Trans_R2W_WeaponA")));
        // 5 ticks in of the 18 it fades in over.
        assert!((gun.arm_weight() - 5.0 / 18.0).abs() < 1e-3, "{}", gun.arm_weight());
        // The fade stops one update short of full and the share stays there.
        run(&mut gun, clips, 45);
        assert!((gun.arm_weight() - 17.0 / 18.0).abs() < 1e-3, "{}", gun.arm_weight());
        // It fades out inside its own last 0.3 s (ticks 66 to 84) and is off as it ends.
        run(&mut gun, clips, 25);
        assert!((gun.arm_weight() - 9.0 / 18.0).abs() < 1e-3, "{}", gun.arm_weight());
        run(&mut gun, clips, 8);
        assert!(gun.arm.is_some());
        run(&mut gun, clips, 2);
        assert!(gun.arm.is_none());
        // The gun stays out, holding the end of its own clip.
        assert!(gun.shown && gun.out);
        assert_eq!(gun.object.map(|o| o.tick), Some(30.0));
        assert!(!gun.holds_trigger(clips, true));
    }

    /// Most packs' events name no weapon and give its slot (Optimus: 0 and 1 in the right
    /// hand, 2 and 3 in the left): such an event counts for the slot in hand and for its
    /// child, and for no other. [game 007b4680] [data]
    #[test]
    fn an_event_by_slot_counts_for_the_slot_in_hand_and_its_child() {
        let by_slot = |index: i32, clip: &str| WeaponEvent { time: 4.0, node: NODE, kind: WeaponEventKind::Attach {
            weapon: 0, index, only_if_current: true, hide: false, unhide: true, clip: id(clip), hide_when_done: false, then: 0,
        }};
        let clip = |events: Vec<WeaponEvent>| AnimRef {
            id: "Trans_R2W".into(), speed: 1.0, crossfade: 0.3, exit_crossfade: 0.3,
            exits: vec![ExitRule { finish: true, ..Default::default() }],
            clip: Some(Clip { length: 84.0, weapon_events: events, ..Default::default() }), ..Default::default()
        };
        let entries = vec![IncomingRule { to_id: id("Trans_R2W"), rules: on_action(action::PULL_OUT, true), ..Default::default() }];
        let object = object_clips();
        let pull_out = |hand: Hand, events: Vec<WeaponEvent>| {
            let arm = vec![clip(events)];
            let clips = Clips { arm: &arm, entries: &entries, object: &object };
            let mut gun = Gun::default();
            gun.step(clips, hand, 0.0);
            gun.take_aim(clips, &cannon(), false);
            for _ in 0..6 {
                gun.step(clips, hand, DT);
            }
            (gun.shown, gun.object.map(|o| o.id))
        };
        let rifle = Hand { name: CANNON, slot: 0, child: Some((CANNON + 2, 2)) };
        let shell = Hand { name: CANNON + 1, slot: 1, child: Some((CANNON + 3, 3)) };
        // Slot 1's event shows the gun only while slot 1 is in hand.
        assert_eq!(pull_out(shell, vec![by_slot(1, "Trans_R2W_WeaponA")]), (true, Some(id("Trans_R2W_WeaponA"))));
        assert_eq!(pull_out(rifle, vec![by_slot(1, "Trans_R2W_WeaponA")]), (false, None));
        // The left hand's slot 2 is the rifle's child: its event counts with the rifle in hand.
        assert_eq!(pull_out(rifle, vec![by_slot(2, "Trans_R2W_WeaponA")]).0, true);
        assert_eq!(pull_out(shell, vec![by_slot(2, "Trans_R2W_WeaponA")]).0, false);
        // A weapon known only by name takes no event by slot.
        assert_eq!(pull_out(Hand::from(CANNON), vec![by_slot(0, "Trans_R2W_WeaponA")]).0, false);
    }

    #[test]
    fn the_trigger_ends_the_pull_out_once_fire_is_allowed_and_fires_after_the_fade() {
        let (arm, entries) = arm_clips();
        let object = object_clips();
        let clips = Clips { arm: &arm, entries: &entries, object: &object };
        let mut gun = Gun::default();
        gun.take_aim(clips, &cannon(), false);
        // Before tick 8 the trigger does nothing to the clip.
        run(&mut gun, clips, 5);
        assert!(gun.holds_trigger(clips, true));
        assert!(gun.arm.is_some_and(|a| !a.cut));
        run(&mut gun, clips, 5);
        // After it, the trigger ends the clip over 0.133 s and is still held back.
        assert!(gun.holds_trigger(clips, true));
        assert!(gun.arm.is_some_and(|a| a.cut));
        run(&mut gun, clips, 4);
        assert!(gun.holds_trigger(clips, true));
        // The clip plays on while it goes, and its share only drops: half the cut's time
        // gone, from the 10 ticks of 18 it had faded in.
        assert!(gun.arm.is_some_and(|a| (a.tick - 14.0).abs() < 1e-3));
        assert!((gun.arm_weight() - 0.4987).abs() < 2e-3, "{}", gun.arm_weight());
        run(&mut gun, clips, 5);
        assert!(gun.arm.is_none());
        assert!(!gun.holds_trigger(clips, true));
        assert!(gun.shown);
    }

    #[test]
    fn a_round_recoils_the_arm_and_fires_the_guns_own_clip() {
        let (arm, entries) = arm_clips();
        let object = object_clips();
        let clips = Clips { arm: &arm, entries: &entries, object: &object };
        let mut gun = Gun { out: true, shown: true, ..Default::default() };
        gun.fired(clips, CANNON);
        assert_eq!(gun.arm.map(|a| arm[a.clip].id.as_str()), Some("Weapon_Recoil"));
        assert_eq!(gun.arm_weight(), 1.0);
        assert_eq!(gun.object.map(|o| o.id), Some(id("FireA")));
        // Another weapon in hand: the cannon's event is not for it.
        let mut other = Gun { out: true, ..Default::default() };
        other.fired(clips, CANNON + 1);
        assert_eq!(other.object, None);
        // The recoil is 8 ticks and has no fade.
        run(&mut gun, clips, 9);
        assert!(gun.arm.is_none());
    }

    #[test]
    fn overheating_holds_the_arm_clip_until_the_weapon_has_cooled() {
        let (arm, entries) = arm_clips();
        let object = object_clips();
        let clips = Clips { arm: &arm, entries: &entries, object: &object };
        let mut gun = Gun { out: true, shown: true, ..Default::default() };
        gun.overheated(clips, &cannon());
        assert_eq!(gun.object.map(|o| o.id), Some(id("Overheat_Weapon_A")));
        run(&mut gun, clips, 300);
        assert_eq!(gun.arm.map(|a| arm[a.clip].id.as_str()), Some("Weapon_Overheat"));
        gun.cooled(&cannon());
        assert_eq!(gun.object.map(|o| o.id), Some(id("Idle_WeaponA")));
        // A loop that is left fades out over its `xfadeTimeOnExit` (0.3 s), still playing.
        let before = gun.arm.map(|a| a.tick);
        run(&mut gun, clips, 1);
        assert!(gun.arm.is_some_and(|a| a.out.is_some()));
        run(&mut gun, clips, 9);
        assert!(gun.arm.map(|a| a.tick) != before);
        assert!((gun.arm_weight() - 8.0 / 18.0).abs() < 1e-3, "{}", gun.arm_weight());
        run(&mut gun, clips, 10);
        assert!(gun.arm.is_none());
    }

    /// A looping arm clip fades in once: the layer's fade reads the player's elapsed time,
    /// which does not wrap (`0053a2b0` returns `+0xe0`; `0053cc40` counts it on). Ironhide's,
    /// Megatron's and Long Haul's `Weapon_Overheat` loop every 40 ticks; with the wrapped
    /// tick the arm faded out and in again on each pass, all through the overheat.
    #[test]
    fn a_short_overheat_loop_does_not_fade_again_on_each_pass() {
        let (mut arm, entries) = arm_clips();
        let overheat = arm.iter_mut().find(|r| r.id == "Weapon_Overheat").unwrap();
        overheat.clip.as_mut().unwrap().length = 40.0;
        let object = object_clips();
        let clips = Clips { arm: &arm, entries: &entries, object: &object };
        let mut gun = Gun { out: true, shown: true, ..Default::default() };
        gun.overheated(clips, &cannon());
        run(&mut gun, clips, 20);
        let plateau = gun.arm_weight();
        assert!(plateau > 0.9, "{plateau}");
        // Five seconds of overheat: more than seven passes of the loop.
        for step in 0..300 {
            run(&mut gun, clips, 1);
            assert_eq!(gun.arm.map(|a| arm[a.clip].id.as_str()), Some("Weapon_Overheat"));
            assert_eq!(gun.arm_weight(), plateau, "weight changed at step {step}");
        }
        assert!(gun.arm.is_some_and(|a| a.tick < 40.0 && a.elapsed_ticks > 300.0));
    }

    /// The axe's events as his first attack and his charge attack carry them.
    #[test]
    fn the_attack_clips_bring_the_axe_out_and_leaving_the_attack_puts_it_away() {
        const AXE: u32 = 9;
        let event = |time: f32, weapon: u32, clip: &str, then: &str, hide_when_done: bool| WeaponEvent {
            time,
            node: NODE,
            kind: WeaponEventKind::Attach {
                weapon,
                index: -1,
                only_if_current: true,
                hide: false,
                unhide: weapon != 0,
                clip: id(clip),
                hide_when_done,
                then: if then.is_empty() { 0 } else { id(then) },
            },
        };
        let own = |name: &str, length: f32, looping: bool| AnimRef {
            id: name.into(),
            looping,
            speed: 1.0,
            clip: Some(Clip { length, ..Default::default() }),
            ..Default::default()
        };
        let clips = vec![own("Trans_R2W", 10.0, false), own("Idle", 30.0, true), own("Trans_W2R", 10.0, false)];
        let first = Clip {
            length: 60.0,
            weapon_events: vec![event(14.0, AXE, "Trans_R2W", "Idle", false), event(36.0, AXE, "Trans_W2R", "", true)],
            ..Default::default()
        };
        let mut axe = Axe::default();
        let play = |axe: &mut Axe, clip: &Clip, from: f32, to: f32| {
            let mut tick = from;
            while tick < to {
                axe.events(clip, Some(tick), tick + 1.0, AXE);
                axe.step(&clips, DT);
                tick += 1.0;
            }
        };
        play(&mut axe, &first, 0.0, 13.0);
        assert!(!axe.shown);
        play(&mut axe, &first, 13.0, 15.0);
        assert!(axe.shown && axe.node == NODE);
        assert_eq!(axe.object.map(|o| o.id), Some(id("Trans_R2W")));
        // Unfolded, it goes on to its idle clip.
        play(&mut axe, &first, 15.0, 30.0);
        assert_eq!(axe.object.map(|o| o.id), Some(id("Idle")));
        // The event at tick 36 folds it, and it is hidden when that clip of 10 ticks ends.
        play(&mut axe, &first, 30.0, 40.0);
        assert!(axe.shown);
        assert_eq!(axe.object.map(|o| o.id), Some(id("Trans_W2R")));
        play(&mut axe, &first, 40.0, 50.0);
        assert!(!axe.shown);
        // Leaving the attack now changes nothing: the exit clip is the one it played.
        axe.put_away(id("Trans_W2R"));
        assert!(!axe.shown && axe.object.is_some_and(|o| o.tick == 10.0));

        // The charge: brought out by the charging clip; the charge attack's own event
        // names no weapon and does nothing, so it is the state's exit that folds it.
        let charging = Clip { length: 56.0, weapon_events: vec![event(46.0, AXE, "Trans_R2W", "", false)], ..Default::default() };
        let attack = Clip { length: 92.0, weapon_events: vec![event(44.0, 0, "Trans_W2R", "", true)], ..Default::default() };
        play(&mut axe, &charging, 40.0, 56.0);
        assert!(axe.shown);
        play(&mut axe, &attack, 0.0, 92.0);
        assert!(axe.shown, "the event without a weapon's name is no one's");
        assert_eq!(axe.object.map(|o| o.id), Some(id("Trans_R2W")));
        axe.put_away(id("Trans_W2R"));
        assert!(axe.shown);
        for _ in 0..11 {
            axe.step(&clips, DT);
        }
        assert!(!axe.shown);
    }

    #[test]
    fn straight_out_of_the_car_the_gun_is_shown_at_once_and_no_arm_clip_plays() {
        let (arm, entries) = arm_clips();
        let object = object_clips();
        let clips = Clips { arm: &arm, entries: &entries, object: &object };
        let mut gun = Gun::default();
        gun.take_aim(clips, &cannon(), true);
        assert!(gun.out && gun.shown);
        assert!(gun.arm.is_none());
        assert_eq!(gun.object.map(|o| o.id), Some(id("Idle_WeaponA")));
        assert!(!gun.holds_trigger(clips, true), "the trigger works at once");
        // Put away and taken out again on foot, the arm pulls it out as usual.
        gun.put_away(clips, &cannon());
        gun.take_aim(clips, &cannon(), false);
        assert_eq!(gun.arm.map(|a| arm[a.clip].id.as_str()), Some("Trans_R2W"));
    }

    /// OptimusFlight's guns (`alwaysAppear`): shown from the start with no clip, and still shown
    /// once the put-away's own clip has ended. [game 007a7150, 007ab1b0] [data]
    #[test]
    fn a_gun_that_always_appears_is_shown_from_its_start_and_after_the_put_away() {
        let (arm, entries) = arm_clips();
        let object = object_clips();
        let clips = Clips { arm: &arm, entries: &entries, object: &object };
        let weapon = WeaponDef { always_appear: true, ..cannon() };
        let mut gun = Gun::default();
        gun.appear(&weapon);
        assert!(gun.shown && !gun.out);
        assert_eq!(gun.object, None, "no clip is started");
        gun.take_aim(clips, &weapon, false);
        run(&mut gun, clips, 90);
        assert!(gun.shown && gun.out);
        gun.put_away(clips, &weapon);
        assert_eq!(gun.object.map(|o| o.id), Some(id("Trans_W2R_WeaponA")));
        run(&mut gun, clips, 40);
        assert_eq!(gun.object.map(|o| o.tick), Some(20.0), "holding the exit clip's last frame");
        assert!(gun.shown, "not hidden at the end of its exit clip");
    }

    /// The charge's start plays the object's Charging clip once, then Charged, which loops
    /// (`007b03c0`). [game]
    #[test]
    fn a_looping_object_clips_keys_pass_across_the_end() {
        // Ratchet's 180-tick clip, its key at 178: a step from 177 round to 1 passes it.
        assert!(key_passes(178.0, Some(177.0), 1.0));
        assert!(key_passes(0.5, Some(177.0), 1.0));
        assert!(!key_passes(88.0, Some(177.0), 1.0));
        assert!(key_passes(88.0, Some(87.0), 89.0));
        assert!(!key_passes(88.0, Some(88.0), 90.0));
        assert!(key_passes(0.0, None, 0.0));
    }

    #[test]
    fn the_charge_plays_charging_then_charged_on_the_object() {
        let (arm, entries) = arm_clips();
        let clip = |name: &str, length: f32, looping: bool| AnimRef {
            id: name.into(),
            looping,
            speed: 1.0,
            clip: Some(Clip { length, ..Default::default() }),
            ..Default::default()
        };
        let object = vec![clip("WeaponB_Charging", 10.0, false), clip("WeaponB_Charged", 6.0, true)];
        let clips = Clips { arm: &arm, entries: &entries, object: &object };
        let weapon = WeaponDef { charging_clip: id("WeaponB_Charging"), charged_clip: id("WeaponB_Charged"), ..cannon() };
        let mut gun = Gun::default();
        gun.charge_started(&weapon);
        assert_eq!(gun.object.map(|o| o.id), Some(id("WeaponB_Charging")));
        run(&mut gun, clips, 40);
        assert_eq!(gun.object.map(|o| o.id), Some(id("WeaponB_Charged")));
        run(&mut gun, clips, 40);
        assert!(gun.object.is_some_and(|o| o.id == id("WeaponB_Charged") && !o.finished), "Charged loops");
        // A weapon with no charging clip starts nothing.
        let mut plain = Gun::default();
        plain.charge_started(&cannon());
        assert_eq!(plain.object, None);
    }

    #[test]
    fn putting_it_away_plays_both_clips_and_hides_the_gun_when_its_own_is_done() {
        let (arm, entries) = arm_clips();
        let object = object_clips();
        let clips = Clips { arm: &arm, entries: &entries, object: &object };
        let mut gun = Gun { out: true, shown: true, ..Default::default() };
        gun.put_away(clips, &cannon());
        assert!(!gun.out);
        assert_eq!(gun.arm.map(|a| arm[a.clip].id.as_str()), Some("Trans_W2R"));
        assert_eq!(gun.object.map(|o| o.id), Some(id("Trans_W2R_WeaponA")));
        run(&mut gun, clips, 19);
        assert!(gun.shown);
        run(&mut gun, clips, 2);
        assert!(!gun.shown, "hidden when its clip of 20 ticks has ended");
        // A weapon that does not always appear is not shown by `appear`.
        let mut idle = Gun::default();
        idle.appear(&cannon());
        assert!(!idle.shown);
        // Taking aim again pulls it out again.
        gun.take_aim(clips, &cannon(), false);
        assert_eq!(gun.arm.map(|a| arm[a.clip].id.as_str()), Some("Trans_R2W"));
    }

    /// The installed arm clips of a robot with a paired weapon: `I_WEAPON_INDEX` picks the
    /// recoil clip (`WeaponPartialSet`'s entries on it were refused before the rules were
    /// evaluated through `animrules::holds`).
    fn installed_arm(pack: &str) -> Option<crate::character::CharacterData> {
        let dir = std::path::Path::new("C:/Games2");
        dir.exists().then(|| crate::character::load(dir, pack).unwrap())
    }

    fn recoil_of(arm: &[AnimRef], entries: &[IncomingRule], hand: Hand, child_turn: bool) -> String {
        let clips = Clips { arm, entries, object: &[] };
        let mut gun = Gun { out: true, ..Default::default() };
        gun.fired_on_turn(clips, hand, child_turn);
        gun.arm.map(|a| arm[a.clip].id.clone()).unwrap_or_default()
    }

    #[test]
    fn optimus_recoils_with_the_arm_of_the_weapon_that_fired() {
        let Some(data) = installed_arm("optimus") else { return };
        let (arm, entries) = (&data.animations.sets["WeaponPartialSet"], &data.animations.incoming["WeaponPartialSet"]);
        // Slot 0 is the rifle, slot 2 its alternating left-hand child (`weapons.rs` test).
        let rifle = Hand { name: 1, slot: 0, child: Some((3, 2)) };
        assert_eq!(recoil_of(arm, entries, rifle, false), "Weapon_RecoilR", "the parent's turn: index 0");
        assert_eq!(recoil_of(arm, entries, rifle, true), "Weapon_RecoilL", "the child's turn: index 2");
        // Another slot matches none of the two entries and falls to the plain recoil.
        let other = Hand { name: 5, slot: 1, child: None };
        assert_eq!(recoil_of(arm, entries, other, false), "Weapon_Recoil");
        // A weapon of no known slot answers 0 (`FUN_0070cae0`: 0 unless a weapon is in hand).
        assert_eq!(recoil_of(arm, entries, Hand::from(1), false), "Weapon_RecoilR");
    }

    #[test]
    fn ironhide_recoils_by_all_four_slots() {
        let Some(data) = installed_arm("ironhide") else { return };
        let (arm, entries) = (&data.animations.sets["WeaponPartialSet"], &data.animations.incoming["WeaponPartialSet"]);
        let at = |slot| recoil_of(arm, entries, Hand { name: 1, slot, child: None }, false);
        assert_eq!((at(0).as_str(), at(1).as_str(), at(2).as_str(), at(3).as_str()), ("Weapon_RecoilR", "Weapon_RecoilR", "Weapon_RecoilL", "Weapon_RecoilL"));
        // The index follows the child on its turn.
        let pair = Hand { name: 1, slot: 1, child: Some((2, 3)) };
        assert_eq!(recoil_of(arm, entries, pair, true), "Weapon_RecoilL");
    }

    #[test]
    fn the_exit_rules_of_the_arm_clips_still_wait_on_the_action() {
        let Some(data) = installed_arm("optimus") else { return };
        let (arm, entries) = (&data.animations.sets["WeaponPartialSet"], &data.animations.incoming["WeaponPartialSet"]);
        let clips = Clips { arm, entries, object: &[] };
        let mut gun = Gun { out: true, ..Default::default() };
        gun.overheated(clips, &cannon());
        assert_eq!(gun.arm.map(|a| arm[a.clip].id.as_str()), Some("Weapon_Overheat"));
        for _ in 0..30 {
            gun.step(clips, 1u32, DT);
        }
        assert!(gun.arm.is_some(), "Weapon_Overheat holds while the action is OVERHEAT");
        gun.cooled(&cannon());
        for _ in 0..30 {
            gun.step(clips, 1u32, DT);
        }
        assert!(gun.arm.is_none(), "and goes when the action is cleared");
    }

    /// Every robot's own `Weapon_Overheat` (a loop of 20 to 160 ticks, faded in over 0.2 or
    /// 0.3 s [data]) holds one weight through a whole overheat period once it has faded in.
    #[test]
    fn every_robots_overheat_arm_holds_its_weight_through_the_overheat() {
        for pack in ["bumblebee", "optimus", "ironhide", "ratchet", "megatron", "longhaul", "audir8", "jetfire", "firestorm", "starscream", "blackout", "optimusflight", "megatronflight"] {
            let Some(data) = installed_arm(pack) else { return };
            let (arm, entries) = (&data.animations.sets["WeaponPartialSet"], &data.animations.incoming["WeaponPartialSet"]);
            let clips = Clips { arm, entries, object: &[] };
            let mut gun = Gun { out: true, ..Default::default() };
            gun.overheated(clips, &cannon());
            let r = &arm[gun.arm.expect("the overheat starts an arm clip").clip];
            assert!(r.looping, "{pack}");
            let fade_in = (r.crossfade / DT).ceil() as usize + 1;
            for _ in 0..fade_in {
                gun.step(clips, 1u32, DT);
            }
            let plateau = gun.arm_weight();
            // The 5 s overheat of the roster's weapons [data], at the test's step.
            for step in 0..(5.0 / DT) as usize {
                gun.step(clips, 1u32, DT);
                assert_eq!(gun.arm_weight(), plateau, "{pack}: {} weight changed at step {step}", r.id);
            }
        }
    }
}
