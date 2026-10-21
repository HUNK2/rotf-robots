//! The mission scripts' animation trigger objects: `AnimTrigger`, `AnimTriggerList`,
//! `AnimTriggerByName` (plays an object's own clips from its `AnimBundle` when triggered),
//! `PlayAnimRefBundle` (the same from its `AnimRefBundle`) and `AnimationSetTrigger`
//! (sends trigger arrays when a character enters or leaves an animation set). Engine free: each object is told what happened and answers with what
//! the game would do; the mission runner owns the objects, the trigger arrays and the clips.
//!
//!   [game]     read from the game's code (addresses below; `notes\functions.tsv`)
//!   [assumed]  our reading of something not checked (each one is listed in
//!              `notes\status.md`)
//!
//! The clip side, as read: `AnimBundle` play (`FUN_01174220` by name, `FUN_005440e0` by
//! entry) builds a new `BasicAnimation` on the entry's `anim` file and installs it as the
//! object's playing clip. Each time that clip passes a loop boundary, or reaches its end
//! once when it does not loop, its listeners run (`FUN_0053cc40`: loop count `+0x100` and
//! the event queue push; `FUN_01173d40` binds the listeners). That is `on_boundary` here.
//! An object's animation controller can be frozen (`FUN_005446f0`, ring `00c02b2c`) and
//! unfrozen (`FUN_00544480`, put on the active ring `00c02b20`). `AnimCtrl::UpdateAll`
//! (`FUN_00544b60`) first takes every frozen controller off the active ring
//! (`FUN_01174320`), then advances only the active ones, so a frozen object holds its
//! pose from the next update on. [game]

/// How a clip is played: `loop` and `playbackType` in the data. [game]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Playback {
    /// -1: the clip's own setting.
    ClipDefault,
    /// 0: once.
    Once,
    /// 1: looping.
    Loop,
}

impl Playback {
    /// `loop` as stored by the `AnimTrigger` loader (-1 when absent). [game]
    pub fn from_loop(value: i32) -> Self {
        match value {
            0 => Playback::Once,
            1 => Playback::Loop,
            _ => Playback::ClipDefault,
        }
    }

    /// `playbackType` of an `animDescList` entry, a name hash (`FUN_007c5390`): `408ffc3e`
    /// loops, `2d252edb` plays once, anything else (the list made from the bundle uses
    /// `0ee3a531`) keeps the clip's own. [game]
    pub fn from_playback_type(hash: u32) -> Self {
        match hash {
            0x408f_fc3e => Playback::Loop,
            0x2d25_2edb => Playback::Once,
            _ => Playback::ClipDefault,
        }
    }
}

/// What the object asks of the game, in the game's order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimAction {
    /// Play the bundle entry with this name (`FUN_01174220`; no such entry: nothing).
    Play { anim: u32, playback: Playback },
    /// The animation player's root mode (`+0xc38`: 0 the clip's whole root transform, 1 its
    /// axes only, 2 none; `notes\melee.md`), and the playing clip's `+0x104`.
    RootMode(i32),
    /// Restart the playing clip from its start (`FUN_0053d420`).
    Restart,
    Freeze,
    Unfreeze,
    /// Send the object's `trigDone` array.
    SendDone,
}

/// `AnimTrigger` (vtable `00b600fc`; ctor `FUN_01186660`, loader `FUN_011866e0`). [game]
#[derive(Clone, Debug, Default)]
pub struct AnimTrigger {
    /// `animName` (`+0x80`); 0 plays nothing new.
    pub anim_name: u32,
    /// `loop` (`+0x88`, -1 when absent).
    pub loop_value: i32,
    /// `rootmotion` (`+0x8c`, 0 when absent): the root mode while it plays.
    pub root_motion: i32,
    /// `alwaysRestart` (`+0x91`).
    pub always_restart: bool,
    /// `+0x90`: playing, listening for the clip's boundaries.
    pub playing: bool,
    activated: bool,
}

impl AnimTrigger {
    pub fn new(anim_name: u32, loop_value: Option<i32>, root_motion: Option<i32>, always_restart: bool) -> Self {
        AnimTrigger {
            anim_name,
            loop_value: loop_value.unwrap_or(-1),
            root_motion: root_motion.unwrap_or(0),
            always_restart,
            ..Default::default()
        }
    }

    /// Slot 4 (`FUN_007c5140`), once: the target's animation is frozen until triggered.
    /// (The `trigDone` array is resolved here too.) [game]
    pub fn activate(&mut self) -> Vec<AnimAction> {
        if self.activated {
            return Vec::new();
        }
        self.activated = true;
        vec![AnimAction::Freeze]
    }

    /// Slot 6 (`FUN_007c4c90`), the trigger: nothing while already playing. `has_clip`: the
    /// object has a playing clip after the play (the restart and the clip's root field need
    /// one). [game]
    pub fn trigger(&mut self, has_clip: bool) -> Vec<AnimAction> {
        if self.playing {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.anim_name != 0 {
            out.push(AnimAction::Play { anim: self.anim_name, playback: Playback::from_loop(self.loop_value) });
        }
        out.push(AnimAction::RootMode(self.root_motion));
        if has_clip && self.always_restart {
            out.push(AnimAction::Restart);
        }
        out.push(AnimAction::Unfreeze);
        self.playing = true;
        out
    }

    /// Slot 7 (`FUN_007c4df0`), the untrigger: stops listening and freezes the animation.
    /// [game]
    pub fn untrigger(&mut self) -> Vec<AnimAction> {
        if !self.playing {
            return Vec::new();
        }
        self.playing = false;
        vec![AnimAction::Freeze]
    }

    /// The playing clip passed a loop boundary or reached its end (`FUN_007c4c00`): send
    /// `trigDone`, and with `loop` 0 untrigger itself. Only while playing (it leaves the
    /// listener list when it stops). [game]
    pub fn on_boundary(&mut self) -> Vec<AnimAction> {
        if !self.playing {
            return Vec::new();
        }
        let mut out = vec![AnimAction::SendDone];
        if self.loop_value == 0 {
            out.extend(self.untrigger());
        }
        out
    }
}

/// `AnimTriggerList` (vtable `00b60144`; ctor `FUN_01186880`, loader `FUN_007c5640`) and
/// `AnimTriggerByName` (the same with a `Trigger` message carrying a name, vtable
/// `00b60168`). [game]
#[derive(Clone, Debug, Default)]
pub struct AnimTriggerList {
    /// `animDescList`: `name` and `playbackType`.
    pub entries: Vec<(u32, Playback)>,
    /// `randomSelect` (`+0xa0`).
    pub random_select: bool,
    /// `startFirstAnim` (`+0xa1`).
    pub start_first: bool,
    /// `ignoreUntrigger` (`+0xa2`).
    pub ignore_untrigger: bool,
    /// `+0x98`, -1 before anything plays.
    pub index: i32,
    /// `+0xa3`.
    pub active: bool,
    activated: bool,
}

impl AnimTriggerList {
    pub fn new(entries: Vec<(u32, Playback)>, random_select: bool, start_first: bool, ignore_untrigger: bool) -> Self {
        AnimTriggerList { entries, random_select, start_first, ignore_untrigger, index: -1, ..Default::default() }
    }

    /// Slot 4 (`FUN_007c5820`), once: freezes the animation unless `startFirstAnim`; an
    /// empty list becomes every name of the object's bundle, each with the clip's own
    /// playback; `startFirstAnim` then plays the first. [game]
    pub fn activate(&mut self, bundle_names: &[u32]) -> Vec<AnimAction> {
        if self.activated {
            return Vec::new();
        }
        self.activated = true;
        let mut out = Vec::new();
        if !self.start_first {
            out.push(AnimAction::Freeze);
        }
        if self.entries.is_empty() {
            self.entries = bundle_names.iter().map(|&n| (n, Playback::from_playback_type(0x0ee3_a531))).collect();
        }
        if self.start_first {
            self.index = 0;
            out.extend(self.play());
        }
        out
    }

    /// Slot 6 (`FUN_007c52b0`): a random entry (`random(count)` gives 0 up to count - 1,
    /// the game's `FUN_011904f0`), or the next, back to the first after the last. [game]
    pub fn trigger(&mut self, random: &mut dyn FnMut(i32) -> i32) -> Vec<AnimAction> {
        self.active = true;
        let count = self.entries.len() as i32;
        self.index = if self.random_select {
            random(count)
        } else if self.index >= count - 1 {
            0
        } else {
            self.index + 1
        };
        self.play()
    }

    /// Slot 7 (`FUN_007c5320`). [game]
    pub fn untrigger(&mut self) -> Vec<AnimAction> {
        self.active = false;
        if self.ignore_untrigger { Vec::new() } else { vec![AnimAction::Freeze] }
    }

    /// `AnimTriggerByName`'s `Trigger` message (`007c5b60`): a name that is in the list
    /// plays that entry; 0 or an unknown name does nothing. [game]
    pub fn trigger_by_name(&mut self, name: u32) -> Vec<AnimAction> {
        if name == 0 {
            return Vec::new();
        }
        match self.entries.iter().position(|&(n, _)| n == name) {
            Some(i) => {
                self.index = i as i32;
                self.play()
            }
            None => Vec::new(),
        }
    }

    /// Every boundary of the object's clip sends `trigDone` (`FUN_007c5b20`). [game]
    pub fn on_boundary(&mut self) -> Vec<AnimAction> {
        vec![AnimAction::SendDone]
    }

    /// `FUN_007c5390`: play the entry at `index`; a 0 name plays and unfreezes nothing.
    /// [game]
    fn play(&self) -> Vec<AnimAction> {
        let Some(&(name, playback)) = usize::try_from(self.index).ok().and_then(|i| self.entries.get(i)) else {
            return Vec::new();
        };
        if name == 0 {
            return Vec::new();
        }
        vec![AnimAction::Play { anim: name, playback }, AnimAction::Unfreeze]
    }
}

/// `PlayAnimRefBundle` (vtable `00b600a4`; loader `FUN_007c4340`): plays one clip of the
/// object's own `AnimRefBundle` when triggered. [game]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayAnimRefBundle {
    /// `animID` (0: nothing to play).
    pub anim_id: u32,
    /// `animOnAnimDone`: the clip to go on to.
    pub anim_on_done: u32,
    /// `hideOnAnimDone`.
    pub hide_on_done: bool,
    /// `dontPlayIfPlayingAlready`.
    pub unless_playing: bool,
    /// `xFadeTimeOverride` (-1 when absent: the clip's own).
    pub xfade_override: f32,
    /// `unhide`.
    pub unhide: bool,
}

/// What a `PlayAnimRefBundle` asks of its object when triggered.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RefBundleRequest {
    /// Clear the object's hidden bit (`+0x60` bit 0) and show it first.
    pub unhide: bool,
    /// The `PlayAnim` event (`FUN_007247f0`) to the object's clip bundle, handled as
    /// `object_clip_request` (`FUN_007c37e0`) handles it.
    pub play: Option<PlayAnimRefBundle>,
}

impl PlayAnimRefBundle {
    pub fn new(anim_id: u32, anim_on_done: u32, hide_on_done: bool, unless_playing: bool, xfade_override: Option<f32>, unhide: bool) -> Self {
        PlayAnimRefBundle { anim_id, anim_on_done, hide_on_done, unless_playing, xfade_override: xfade_override.unwrap_or(-1.0), unhide }
    }

    /// Slot 6, the trigger (`FUN_007c44f0`). [game]
    pub fn trigger(&self) -> RefBundleRequest {
        RefBundleRequest { unhide: self.unhide, play: (self.anim_id != 0).then(|| self.clone()) }
    }
}

/// One entry of `animationSetEnterArray` or `animationSetExitArray`. [game]
#[derive(Clone, Debug)]
pub struct SetEntry {
    /// `animationSetID`.
    pub set: u32,
    /// `oneShot` (true when absent).
    pub one_shot: bool,
    /// `delay` in seconds (0 when absent).
    pub delay: f32,
    fired: bool,
    countdown: f32,
}

impl SetEntry {
    pub fn new(set: u32, one_shot: Option<bool>, delay: Option<f32>) -> Self {
        SetEntry { set, one_shot: one_shot.unwrap_or(true), delay: delay.unwrap_or(0.0), fired: false, countdown: 0.0 }
    }
}

/// `AnimationSetTrigger` (vtable `00b617c0`; loader `FUN_00811020`, update
/// `FUN_00811a90`). It watches one character's base animation set (character `+0xfc`,
/// then `+0x20`, then `+0x18`): the player's when `watchPlayer` (the default), else the last
/// live character of `animSetObjectIds`, else the one `characterSpawnerId` spawned; none
/// is set 0. [game]
#[derive(Clone, Debug, Default)]
pub struct AnimationSetTrigger {
    pub enter: Vec<SetEntry>,
    pub exit: Vec<SetEntry>,
    /// `+0x78`: the set seen on the update before (0 at the start).
    prev: u32,
}

/// Which array an `AnimationSetTrigger` sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetSend {
    Enter(usize),
    Exit(usize),
}

impl AnimationSetTrigger {
    pub fn new(enter: Vec<SetEntry>, exit: Vec<SetEntry>) -> Self {
        AnimationSetTrigger { enter, exit, prev: 0 }
    }

    /// One update with the watched set (`current`, 0 for none) and the step. On a change
    /// of set an enter entry arms when the new set is its own, an exit entry when it is
    /// not; armed, its countdown starts at `delay`. A `oneShot` entry stays armed for good,
    /// another disarms when the change goes the other way. The countdown is held at -1
    /// while the set does not fit (enter) or does (exit); the array is sent on the update
    /// the countdown is at 0 or more and under the step, then it counts down. So an exit
    /// entry fires on the first update if the character is already in another set, and
    /// the arrays go out in stored order, enter entries first. [game]
    pub fn update(&mut self, current: u32, dt: f32) -> Vec<SetSend> {
        let prev = self.prev;
        self.prev = current;
        let mut out = Vec::new();
        for (i, e) in self.enter.iter_mut().enumerate() {
            if Self::step(e, current, prev, dt, true) {
                out.push(SetSend::Enter(i));
            }
        }
        for (i, e) in self.exit.iter_mut().enumerate() {
            if Self::step(e, current, prev, dt, false) {
                out.push(SetSend::Exit(i));
            }
        }
        out
    }

    fn step(e: &mut SetEntry, current: u32, prev: u32, dt: f32, enter: bool) -> bool {
        let fits = (current == e.set) == enter;
        if current != prev {
            if !e.fired {
                if fits {
                    e.fired = true;
                    e.countdown = e.delay;
                }
            } else if !e.one_shot && !fits {
                e.fired = false;
            }
        }
        if !fits {
            e.countdown = -1.0;
        }
        let send = e.countdown >= 0.0 && e.countdown < dt;
        e.countdown -= dt;
        send
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 0.032;

    #[test]
    fn anim_trigger_plays_once_and_stops_itself_at_the_end() {
        let mut t = AnimTrigger::new(0x1234, Some(0), Some(2), true);
        assert_eq!(t.activate(), vec![AnimAction::Freeze]);
        assert!(t.activate().is_empty());
        let a = t.trigger(true);
        assert_eq!(a, vec![
            AnimAction::Play { anim: 0x1234, playback: Playback::Once },
            AnimAction::RootMode(2),
            AnimAction::Restart,
            AnimAction::Unfreeze,
        ]);
        // Triggering again while it plays does nothing.
        assert!(t.trigger(true).is_empty());
        // The end: trigDone, then it stops and freezes.
        assert_eq!(t.on_boundary(), vec![AnimAction::SendDone, AnimAction::Freeze]);
        assert!(!t.playing);
        assert!(t.on_boundary().is_empty());
    }

    #[test]
    fn anim_trigger_with_the_clips_own_loop_keeps_sending_done() {
        let mut t = AnimTrigger::new(0, None, None, false);
        let a = t.trigger(false);
        // No name: the clip already there keeps playing; root mode 0 is still written.
        assert_eq!(a, vec![AnimAction::RootMode(0), AnimAction::Unfreeze]);
        assert_eq!(t.on_boundary(), vec![AnimAction::SendDone]);
        assert_eq!(t.on_boundary(), vec![AnimAction::SendDone]);
        assert_eq!(t.untrigger(), vec![AnimAction::Freeze]);
        assert!(t.untrigger().is_empty());
    }

    #[test]
    fn trigger_list_cycles_and_wraps() {
        let mut l = AnimTriggerList::new(
            vec![(1, Playback::from_playback_type(0x408f_fc3e)), (2, Playback::from_playback_type(0x2d25_2edb)), (3, Playback::ClipDefault)],
            false, false, false,
        );
        assert_eq!(l.activate(&[]), vec![AnimAction::Freeze]);
        let mut never = |_: i32| -> i32 { panic!("not random") };
        let names: Vec<u32> = (0..5)
            .map(|_| match l.trigger(&mut never)[0] { AnimAction::Play { anim, .. } => anim, _ => 0 })
            .collect();
        assert_eq!(names, vec![1, 2, 3, 1, 2]);
        assert_eq!(l.trigger(&mut never)[0], AnimAction::Play { anim: 3, playback: Playback::ClipDefault });
        assert_eq!(l.untrigger(), vec![AnimAction::Freeze]);
    }

    #[test]
    fn trigger_list_from_the_bundle_starts_first_and_ignores_untrigger() {
        let mut l = AnimTriggerList::new(Vec::new(), true, true, true);
        let a = l.activate(&[10, 20]);
        assert_eq!(a, vec![AnimAction::Play { anim: 10, playback: Playback::ClipDefault }, AnimAction::Unfreeze]);
        let mut pick = |n: i32| { assert_eq!(n, 2); 1 };
        assert_eq!(l.trigger(&mut pick)[0], AnimAction::Play { anim: 20, playback: Playback::ClipDefault });
        assert!(l.untrigger().is_empty());
        assert!(!l.active);
    }

    #[test]
    fn trigger_by_name_plays_the_named_entry_only() {
        let mut l = AnimTriggerList::new(vec![(5, Playback::Loop), (6, Playback::Once)], false, false, false);
        assert!(l.trigger_by_name(0).is_empty());
        assert!(l.trigger_by_name(7).is_empty());
        assert_eq!(l.trigger_by_name(6)[0], AnimAction::Play { anim: 6, playback: Playback::Once });
        assert_eq!(l.index, 1);
        assert_eq!(l.on_boundary(), vec![AnimAction::SendDone]);
    }

    #[test]
    fn play_anim_ref_bundle_unhides_and_sends_its_clip() {
        let p = PlayAnimRefBundle::new(0x77, 0x88, true, false, None, true);
        let r = p.trigger();
        assert!(r.unhide);
        let play = r.play.unwrap();
        assert_eq!((play.anim_id, play.anim_on_done, play.xfade_override), (0x77, 0x88, -1.0));
        let only_unhide = PlayAnimRefBundle::new(0, 0, false, false, Some(0.2), true).trigger();
        assert!(only_unhide.unhide && only_unhide.play.is_none());
    }

    #[test]
    fn set_trigger_enter_waits_its_delay_and_fires_once() {
        let mut t = AnimationSetTrigger::new(vec![SetEntry::new(0xAA, None, Some(0.1))], Vec::new());
        assert!(t.update(0xBB, DT).is_empty());
        // Into the set: armed with 0.1 s; sent on the update the countdown drops under a step.
        let mut sent_at = None;
        for n in 0..10 {
            if !t.update(0xAA, DT).is_empty() {
                sent_at = Some(n);
                break;
            }
        }
        assert_eq!(sent_at, Some(3), "0.1 - 3 * 0.032 = 0.004 is under a step");
        // One shot: leaving and coming back does not fire again.
        for set in [0xBB, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA] {
            assert!(t.update(set, DT).is_empty());
        }
    }

    #[test]
    fn set_trigger_rearms_when_not_one_shot_and_leaving_cancels_the_delay() {
        let mut t = AnimationSetTrigger::new(vec![SetEntry::new(0xAA, Some(false), None)], Vec::new());
        assert_eq!(t.update(0xAA, DT), vec![SetSend::Enter(0)]);
        assert!(t.update(0xAA, DT).is_empty());
        assert!(t.update(0xBB, DT).is_empty());
        assert_eq!(t.update(0xAA, DT), vec![SetSend::Enter(0)]);
        // A delayed one left before its time never fires.
        let mut d = AnimationSetTrigger::new(vec![SetEntry::new(0xAA, Some(false), Some(1.0))], Vec::new());
        d.update(0xAA, DT);
        d.update(0xBB, DT);
        for _ in 0..60 {
            assert!(d.update(0xBB, DT).is_empty());
        }
    }

    #[test]
    fn set_trigger_exit_fires_at_once_from_another_set_and_on_leaving() {
        // Already in another set on the first update: the exit array goes out (0 to BB is a change).
        let mut t = AnimationSetTrigger::new(Vec::new(), vec![SetEntry::new(0xAA, Some(false), None)]);
        assert_eq!(t.update(0xBB, DT), vec![SetSend::Exit(0)]);
        assert!(t.update(0xAA, DT).is_empty());
        assert_eq!(t.update(0xBB, DT), vec![SetSend::Exit(0)]);
        // Enter entries first, then exit entries.
        let mut both = AnimationSetTrigger::new(vec![SetEntry::new(0xCC, None, None)], vec![SetEntry::new(0xAA, None, None)]);
        assert_eq!(both.update(0xCC, DT), vec![SetSend::Enter(0), SetSend::Exit(0)]);
    }
}
