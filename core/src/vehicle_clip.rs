//! The vehicle model's own clip player during a change of form: which of its clips plays, at what
//! tick, and when it hands over to the clip after it. The robot's `AlternateModelEvent`s start it
//! (`character_visibility_toggle` 0071cfa0 sends a PlayAnim with the clip, its follow-on and a
//! crossfade of -1, i.e. the clip's own, which is 0 for every vehicle clip read [data]).
//!
//! Two rules of the game's object clip player [game]:
//! - A clip started by an event does not move in the update it is started in: `AnimCtrl::UpdateAll`
//!   (00544b60) advances every controller first and sends the queued events after, so the PlayAnim
//!   lands after this update's advance. [assumed: from the read order, not traced; the robot's own
//!   clip likewise starts at tick 0 in the port] So the vehicle keeps level with the robot.
//! - When a clip that does not loop has ended, `object_clip_update` (007c4170) starts the queued one,
//!   and its pose is written only by the next advance (00544a50 -> 00544e00): the update it ends in
//!   shows the end of the old clip, never the start of either.

/// Where the vehicle clip is, and what follows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Driver {
    pub playing: Option<String>,
    pub then: Option<String>,
    pub time: f32,
    /// Started this frame: it is not advanced until the next.
    fresh: bool,
}

/// What to draw this frame: the clip by id at a tick.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub clip: String,
    pub time: f32,
    /// The ticks the clip passed this frame (for its cues): from, to.
    pub passed: (f32, f32),
}

impl Driver {
    /// A robot event starts `clip` from its first tick, with `then` after it.
    pub fn start(&mut self, clip: &str, then: Option<String>) {
        self.playing = Some(clip.to_owned());
        self.then = then;
        self.time = 0.0;
        self.fresh = true;
    }

    /// Moves the clip on by `ticks` and says what to draw. `info` gives a clip's length and
    /// whether it loops, by id (`None`: no such clip, nothing is drawn from it).
    pub fn advance(&mut self, ticks: f32, info: impl Fn(&str) -> Option<(f32, bool)>) -> Option<Sample> {
        let id = self.playing.clone()?;
        let (length, looping) = info(&id)?;
        if std::mem::take(&mut self.fresh) {
            return Some(Sample { clip: id, time: self.time, passed: (self.time, self.time) });
        }
        let before = self.time;
        self.time += ticks;
        let passed = (before, self.time);
        if looping {
            self.time %= length.max(1.0);
        } else if self.time >= length {
            self.time = length;
            if let Some(next) = self.then.take() {
                // The end of this clip is drawn now; the next one starts from its first tick and
                // moves from the next frame on.
                self.playing = Some(next);
                self.time = 0.0;
                self.fresh = true;
                return Some(Sample { clip: id, time: length, passed });
            }
        }
        Some(Sample { clip: id, time: self.time, passed })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str) -> Option<(f32, bool)> {
        match id {
            "A" => Some((38.0, false)),
            "B" => Some((32.0, false)),
            "Idle" => Some((120.0, true)),
            _ => None,
        }
    }

    #[test]
    fn a_started_clip_keeps_level_with_the_robot_that_started_it() {
        let mut d = Driver::default();
        d.start("A", Some("Idle".into()));
        // The robot's clip is at tick 0 on its first frame; so is the vehicle's.
        assert_eq!(d.advance(1.0, info).unwrap().time, 0.0);
        assert_eq!(d.advance(1.0, info).unwrap().time, 1.0);
        // So when the robot's next clip starts B at its tick 38, A has not yet run out into Idle.
        for _ in 0..36 {
            d.advance(1.0, info);
        }
        assert_eq!(d.playing.as_deref(), Some("A"));
        assert_eq!(d.time, 37.0);
    }

    #[test]
    fn the_hand_over_frame_shows_the_end_of_the_old_clip_then_the_new_one_from_its_start() {
        let mut d = Driver::default();
        d.start("B", Some("Idle".into()));
        d.advance(1.0, info);
        for _ in 0..31 {
            d.advance(1.0, info);
        }
        assert_eq!(d.time, 31.0);
        let end = d.advance(1.5, info).unwrap();
        assert_eq!((end.clip.as_str(), end.time), ("B", 32.0), "not B's first pose, not Idle's");
        let first = d.advance(1.5, info).unwrap();
        assert_eq!((first.clip.as_str(), first.time), ("Idle", 0.0));
        assert_eq!(d.advance(1.5, info).unwrap().time, 1.5);
    }

    #[test]
    fn a_clip_with_nothing_after_it_holds_its_end_and_a_loop_goes_round() {
        let mut d = Driver::default();
        d.start("A", None);
        d.advance(1.0, info);
        for _ in 0..50 {
            d.advance(1.0, info);
        }
        assert_eq!((d.playing.as_deref(), d.time), (Some("A"), 38.0));
        d.start("Idle", None);
        d.advance(1.0, info);
        for _ in 0..125 {
            d.advance(1.0, info);
        }
        assert_eq!(d.time, 5.0);
    }
}
