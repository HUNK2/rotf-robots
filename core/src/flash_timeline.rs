//! Playing a Flash movie's timelines as the player does, without its classes: every sprite instance has
//! its own playhead, which starts on frame 1 when the instance is placed, moves a frame a tick (the
//! movie's rate) and loops at its end, and is moved by its frame scripts' `stop`, `play` and `goto`
//! (`gfx::Sprite::scripts`). The classes' part (methods a frame script calls, `_visible`, a
//! `gotoAndPlay` from code) is played by the caller: it reads which frames were entered
//! (`take_entered`, with each frame script's strings) and moves playheads (`goto_and_play`, `stop`,
//! `set_visible`). The result is a `gfx::Pose` with every instance's frame.
//!
//! [game] Flash 8 semantics as far as the briefing needs them: an instance keeps its playhead while it
//! stays at its depth with the same character (a timeline looping or jumping back does not restart it);
//! `ActionGotoFrame` stops and a following `ActionPlay` plays; a one-frame timeline does not loop.
//! [assumed] within a tick, parents' playheads move before their children's and a new instance's
//! frame 1 script runs in the tick it appears (the Flash player's exact action order is not modelled).
//! OPEN: settles: the Scaleform advance routine's action order (not located in the executable); the HUD movies'
//! frame scripts that depend on it (`gfx_dump.py code`).

use std::collections::{HashMap, HashSet};

use crate::formats::gfx::{Control, Movie, Pose, Tweak};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Head {
    pub id: u16,
    /// Counted from 1.
    pub frame: usize,
    pub playing: bool,
    pub visible: bool,
}

/// A frame an instance entered: its path of instance names (`""` the root, `PopUp/PopUp_3`), its sprite and
/// the frame (from 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Entered {
    pub path: String,
    pub id: u16,
    pub frame: usize,
}

#[derive(Clone, Debug)]
pub struct Timeline {
    root: u16,
    heads: HashMap<String, Head>,
    /// Frames entered whose scripts have not run yet.
    queue: Vec<Entered>,
    /// Frames entered since `take_entered`.
    entered: Vec<Entered>,
}

impl Timeline {
    /// The root sprite `root` on its frame 1, with its instances placed.
    pub fn new(movie: &Movie, root: u16) -> Timeline {
        let mut t = Timeline { root, heads: HashMap::new(), queue: Vec::new(), entered: Vec::new() };
        t.heads.insert(String::new(), Head { id: root, frame: 1, playing: true, visible: true });
        t.queue.push(Entered { path: String::new(), id: root, frame: 1 });
        t.settle(movie);
        t
    }

    pub fn head(&self, path: &str) -> Option<&Head> {
        self.heads.get(path)
    }

    /// The frames entered since the last call, in the order their scripts ran.
    pub fn take_entered(&mut self) -> Vec<Entered> {
        std::mem::take(&mut self.entered)
    }

    /// One tick: every playing timeline moves a frame (looping at its end), then the entered frames' scripts run
    /// and new instances are placed.
    pub fn tick(&mut self, movie: &Movie) {
        let mut paths: Vec<String> = self.heads.keys().cloned().collect();
        // A parent's path sorts before its children's.
        paths.sort();
        for path in paths {
            let Some(head) = self.heads.get_mut(&path) else { continue };
            let Some(sprite) = movie.sprite(head.id) else { continue };
            let n = sprite.frames.len();
            if !head.playing || n < 2 {
                continue;
            }
            head.frame = if head.frame >= n { 1 } else { head.frame + 1 };
            self.queue.push(Entered { path, id: head.id, frame: head.frame });
        }
        self.settle(movie);
    }

    /// A class's `gotoAndPlay(label)` on the instance at `path`. False when there is no such instance or label.
    pub fn goto_and_play(&mut self, movie: &Movie, path: &str, label: &str) -> bool {
        let Some(head) = self.heads.get(path).copied() else { return false };
        let Some(frame) = movie.sprite(head.id).and_then(|s| s.label(label)) else { return false };
        self.goto(path, frame, true);
        self.settle(movie);
        true
    }

    /// A class's `gotoAndStop(frame)` (frame counted from 1; ActionScript's `gotoFrame 0` is frame 1) on the
    /// instance at `path`. False when there is no such instance.
    pub fn goto_and_stop(&mut self, movie: &Movie, path: &str, frame: usize) -> bool {
        if !self.heads.contains_key(path) {
            return false;
        }
        self.goto(path, frame, false);
        self.settle(movie);
        true
    }

    /// A class's `stop()` on the instance at `path`.
    pub fn stop(&mut self, path: &str) {
        if let Some(head) = self.heads.get_mut(path) {
            head.playing = false;
        }
    }

    /// A class's `_visible` on the instance at `path` (a hidden instance still plays).
    pub fn set_visible(&mut self, path: &str, visible: bool) {
        if let Some(head) = self.heads.get_mut(path) {
            head.visible = visible;
        }
    }

    /// Every instance's frame and visibility.
    pub fn pose(&self) -> Pose {
        self.heads.iter().map(|(path, head)| (path.clone(), Tweak { frame: Some(head.frame), hidden: !head.visible, ..Default::default() })).collect()
    }

    fn goto(&mut self, path: &str, frame: usize, play: bool) {
        let Some(head) = self.heads.get_mut(path) else { return };
        head.frame = frame.max(1);
        head.playing = play;
        self.queue.push(Entered { path: path.to_owned(), id: head.id, frame: head.frame });
    }

    /// Runs the queued frames' scripts and places new instances until nothing more happens.
    fn settle(&mut self, movie: &Movie) {
        for _ in 0..64 {
            self.place(movie);
            if self.queue.is_empty() {
                return;
            }
            for e in std::mem::take(&mut self.queue) {
                // A later jump in the same tick replaced this frame: its script does not run.
                if self.heads.get(&e.path).is_none_or(|h| h.id != e.id || h.frame != e.frame) {
                    continue;
                }
                let Some(sprite) = movie.sprite(e.id) else { continue };
                for (_, script) in sprite.scripts.iter().filter(|(f, _)| *f == e.frame) {
                    for control in &script.controls {
                        let Some(head) = self.heads.get_mut(&e.path) else { break };
                        match control {
                            Control::Stop => head.playing = false,
                            Control::Play => head.playing = true,
                            Control::GotoFrame(n) => self.goto(&e.path, *n, false),
                            Control::GotoLabel(label) => {
                                if let Some(n) = sprite.label(label) {
                                    self.goto(&e.path, n, false);
                                }
                            }
                        }
                    }
                }
                self.entered.push(e);
            }
        }
    }

    /// Gives every sprite instance shown a playhead (new ones on frame 1) and drops those no longer shown.
    fn place(&mut self, movie: &Movie) {
        let mut seen = HashSet::new();
        self.walk(movie, String::new(), self.root, &mut seen);
        self.heads.retain(|path, _| seen.contains(path));
    }

    fn walk(&mut self, movie: &Movie, path: String, id: u16, seen: &mut HashSet<String>) {
        let Some(sprite) = movie.sprite(id) else { return };
        let frame = self.heads.get(&path).map_or(1, |h| h.frame);
        seen.insert(path.clone());
        for item in sprite.shown(frame) {
            let Some(child_id) = item.id else { continue };
            if movie.sprite(child_id).is_none() {
                continue;
            }
            let name = item.name.clone().unwrap_or_else(|| format!("#{}", item.depth));
            let child = if path.is_empty() { name } else { format!("{path}/{name}") };
            if self.heads.get(&child).is_none_or(|h| h.id != child_id) {
                self.heads.insert(child.clone(), Head { id: child_id, frame: 1, playing: true, visible: true });
                self.queue.push(Entered { path: child.clone(), id: child_id, frame: 1 });
            }
            self.walk(movie, child, child_id, seen);
        }
    }
}

/// The strings a frame's scripts name (the class methods they call, the sounds they play).
pub fn script_strings<'a>(movie: &'a Movie, e: &Entered) -> impl Iterator<Item = &'a str> + 'a {
    let frame = e.frame;
    movie.sprite(e.id).into_iter().flat_map(move |s| s.scripts.iter().filter(move |(f, _)| *f == frame)).flat_map(|(_, s)| s.strings.iter().map(String::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::gfx::{Character, Place, Script, Sprite, Step};

    fn place(depth: u16, id: u16, name: &str) -> Step {
        Step::Place(Place { depth, id: Some(id), name: Some(name.into()), ..Default::default() })
    }

    #[test]
    fn playheads_start_when_placed_stop_and_loop_by_their_scripts() {
        // 2: three frames, stops on its last. 3: two frames, loops back with gotoFrame 0; play. 1: the root, places
        // 2 on frame 1 and 3 on frame 2.
        let stopper = Sprite {
            frames: vec![vec![], vec![], vec![]],
            scripts: vec![(3, Script { controls: vec![Control::Stop], strings: vec!["showNextPopup".into()], raw: Vec::new() })],
            ..Default::default()
        };
        let looper = Sprite {
            frames: vec![vec![], vec![], vec![]],
            scripts: vec![(3, Script { controls: vec![Control::GotoFrame(1), Control::Play], strings: vec![], raw: Vec::new() })],
            labels: vec![("Main".into(), 2)],
            ..Default::default()
        };
        let root = Sprite { frames: vec![vec![place(1, 2, "A")], vec![place(2, 3, "B")], vec![], vec![], vec![]], ..Default::default() };
        let characters = [(1, Character::Sprite(root)), (2, Character::Sprite(stopper)), (3, Character::Sprite(looper))].into_iter().collect();
        let movie = Movie { stage: [16.0, 16.0], rate: 30.0, characters, exports: HashMap::new(), imports: Vec::new(), bitmaps: HashMap::new(), text_images: Vec::new() };
        let mut t = Timeline::new(&movie, 1);
        assert_eq!(t.head("A").map(|h| h.frame), Some(1));
        assert!(t.head("B").is_none());
        t.tick(&movie);
        assert_eq!((t.head("A").unwrap().frame, t.head("B").unwrap().frame), (2, 1));
        t.tick(&movie);
        // A's frame 3 stops it and names its call.
        let entered = t.take_entered();
        let a3 = entered.iter().find(|e| e.path == "A" && e.frame == 3).unwrap();
        assert_eq!(script_strings(&movie, a3).collect::<Vec<_>>(), ["showNextPopup"]);
        t.tick(&movie);
        t.tick(&movie);
        assert_eq!(t.head("A").unwrap().frame, 3, "stopped");
        assert!(!t.head("A").unwrap().playing);
        // B: 1 (tick 1), 2, 3 -> goto 1 (tick 3), 2 (tick 4).
        assert_eq!(t.head("B").unwrap().frame, 2);
        // The root loops (no script): at its frame 1 again A is kept, B (placed on frame 2) is dropped.
        t.tick(&movie);
        assert_eq!(t.head("").unwrap().frame, 1);
        assert!(t.head("B").is_none() && t.head("A").unwrap().frame == 3);
        // A class's gotoAndPlay.
        t.set_visible("A", false);
        assert!(t.goto_and_play(&movie, "", "Missing") == false);
        t.tick(&movie);
        assert!(t.goto_and_play(&movie, "B", "Main"));
        assert_eq!(t.head("B").unwrap().frame, 2);
        assert!(t.pose()["A"].hidden);
    }
}
