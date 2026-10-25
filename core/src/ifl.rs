//! Original IFLCtrl clock, independent of rendering. See the unpublished notes.
use crate::formats::lxb::Node;

#[derive(Clone, Copy, Debug)]
pub struct Definition {
    pub start_frame: i32,
    pub frames_per_second: f32,
    pub frames_to_play: i32,
    pub random_start: bool,
}

impl Definition {
    pub fn read(node: Node<'_>) -> Self {
        Self {
            start_frame: node.get("startFrame").and_then(Node::int).unwrap_or(0) as i32,
            frames_per_second: node.get("framesPerSecond").and_then(Node::float).unwrap_or(0.0),
            frames_to_play: node.get("numFramesToPlay").and_then(Node::int).unwrap_or(0) as i32,
            random_start: node.get("randomStartFrame").and_then(Node::int).unwrap_or(0) != 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Clock {
    pub frame: i32,
    start: i32,
    elapsed: f32,
    fps: f32,
    remaining: f32,
}

impl Clock {
    /// [game 005613e0] A random start REPLACES startFrame. The host supplies the rand() draw.
    pub fn new(def: Definition, random_draw: i32) -> Self {
        let start = if def.random_start { random_draw } else { def.start_frame };
        Self { frame: start, start, elapsed: 0.0, fps: def.frames_per_second, remaining: def.frames_to_play as f32 }
    }

    /// [game 005610f0] Advance elapsed time, truncate elapsed*fps (x87 control word
    /// is explicitly set to truncation at00561135), then decrement a finite budget.
    /// A negative budget loops; zero stops. Zero/nonpositive fps makes a fixed frame.
    pub fn step(&mut self, dt: f32) {
        if self.fps <= 0.0 || self.remaining == 0.0 { return; }
        self.elapsed += dt;
        self.frame = self.start.wrapping_add((self.elapsed * self.fps) as i32);
        if self.remaining > 0.0 { self.remaining = (self.remaining - dt * self.fps).max(0.0); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn definition(fps: f32, count: i32) -> Definition {
        Definition { start_frame: 3, frames_per_second: fps, frames_to_play: count, random_start: false }
    }
    #[test]
    fn clock_truncates_instead_of_rounding_and_random_replaces_start() {
        let mut clock = Clock::new(definition(2.0, -1), 17);
        clock.step(0.49);
        assert_eq!(clock.frame, 3);
        clock.step(0.02);
        assert_eq!(clock.frame, 4);
        let random = Clock::new(Definition { random_start: true, ..definition(2.0, -1) }, 17);
        assert_eq!(random.frame, 17);
    }
    #[test]
    fn finite_budget_stops_and_zero_fps_keeps_the_authored_frame() {
        let mut clock = Clock::new(definition(10.0, 2), 0);
        clock.step(0.1);
        assert_eq!(clock.frame, 4);
        clock.step(0.1);
        assert_eq!(clock.frame, 5);
        clock.step(1.0);
        assert_eq!(clock.frame, 5);
        let mut fixed = Clock::new(definition(0.0, -1), 0);
        fixed.step(10.0);
        assert_eq!(fixed.frame, 3);
    }
}
