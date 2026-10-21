//! `AnimatedCamera`: a level's scripted camera. A scene object (the rig) carries an animation; while the
//! component is triggered the game camera takes the rig's transform every update, and messages from the rig's
//! animation set the field of view and the depth of field. [game] `notes\camera.md` "AnimatedCamera".
//!
//! The component: registrar 00adb430 (name `AnimatedCamera`, factory 007d0920), constructor 007cfe50, loader
//! 01186d50, vtable 00b60368 (slot 1 teardown 007d0200, 2 update 007d0290, 4 activation 007d0180, 6 Trigger
//! 007d03a0, 7 Untrigger 007d05d0). Its camera behaviour: constructor 007cfd30, vtable 00b60328, update
//! (slot 3) 00913440. Placed in 3 levels (74 objects, `notes\phase1-audit.md`).
//!
//! The CinemaTrigger's own cut (`crate::cinematic`) is a different path: it also builds one of these
//! behaviours (`notes\mission-vo-cinema-energon.md`), keyed 0xa8579652.
use glam::Vec3;

/// The behaviour's field of view until a message sets it, degrees. [game] 007cfd30 stores 0x42700000.
pub const DEFAULT_FOV: f32 = 60.0;

/// What the FOV message's value is multiplied by: 0x422be328 = 42.97183, which is 0.75 * 180 / PI to float
/// precision. [game] 007d0d30 (`mulss [00b6a8e4]`). [assumed] the message carries radians and the 0.75 turns a
/// 4:3 width into the height the camera's `+0x94` holds; the code only shows the product.
pub const FOV_MESSAGE_SCALE: f32 = f32::from_bits(0x422b_e328);

/// Message names the component listens for (its seven rings, 007cfe50, keys from 011867f0..011969b0). [game]
/// None of the seven is in `export\data\names.tsv` or among the executable's strings: the meanings below come
/// from the handlers, not the names.
pub mod msg {
    /// 007d0d10 -> 007d0660: the rig's animation is done. [game] the handler; [assumed] the meaning.
    pub const RIG_DONE: u32 = 0x9388_ead0;
    /// 007d0d20 -> 007d07b0: switch the camera on (true) or off (false). [game]
    pub const ENABLE: u32 = 0x77ed_0811;
    /// 007d0d30: field of view, times [`super::FOV_MESSAGE_SCALE`]. [game]
    pub const FOV: u32 = 0xe783_10ed;
    /// 007d0d60: global 00ce4ae4. [game]
    pub const DOF_NEAR_FOCUS: u32 = 0x65c1_7a30;
    /// 007d0d90: global 00ce4ae0. [game]
    pub const DOF_FAR_FOCUS: u32 = 0x9008_534e;
    /// 007d0dc0: global 00ce4adc. [game]
    pub const DOF_NEAR_FALLOFF: u32 = 0xbf0d_4646;
    /// 007d0df0: global 00ce4ad8. [game]
    pub const DOF_FAR_FALLOFF: u32 = 0x3c8c_f525;
}

/// The depth-of-field override the scripted camera writes. Four globals and a flag; the per-frame light
/// update (00569cd0 at 0056a2cd) copies them over the environment's values while the flag is set.
///
/// [game] the globals, their defaults (written by 007d0200 / 007d05d0 / 007d07b0) and the copy into
/// `lighting_state_snapshot`'s words 4..7 (state words 0x72..0x75).
/// [assumed] the names: the snapshot's word 0 is the environment's DOF flag (bit 4 of its flags, matching the
/// decoder's flag order) and the environment's seven `dof*` fields are taken to sit at words 0x70..0x76 in the
/// decoder's order, which puts these four on nearFocus, farFocus, nearFalloff, farFalloff. The defaults fit
/// (everything in focus from 0 to 100,000). Settled by the order the environment loader writes the block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DofOverride {
    pub near_focus: f32,
    pub far_focus: f32,
    pub near_falloff: f32,
    pub far_falloff: f32,
    /// 00ced676: when set the light update uses the four values above.
    pub active: bool,
}

impl Default for DofOverride {
    /// [game] 0, 0x47c35000 (100,000), 0x3a83126f (0.001) twice, flag clear.
    fn default() -> Self {
        Self { near_focus: 0.0, far_focus: 100_000.0, near_falloff: f32::from_bits(0x3a83_126f), far_falloff: f32::from_bits(0x3a83_126f), active: false }
    }
}

/// The camera's frame from the rig's world axes (right, forward, up as rows). [game] 00913440.
///
/// Forward is normalised (left alone when shorter than 1e-8). Then, by which axis forward is closer to:
/// - `|f.up| <= |f.right|`: up is scaled by `1 - f.up` and normalised, and right = forward x up. The original
///   multiplies up by its OWN components here (`v - v * d`, disassembly 009136f1..0091375e), not by forward's, so
///   up is not made square to forward. Kept as found.
/// - otherwise: right = normalise(right - forward * (f.right)) and up = right x forward.
pub fn rig_frame(right: Vec3, forward: Vec3, up: Vec3) -> (Vec3, Vec3, Vec3) {
    let f = normalise(forward);
    let d_right = right.dot(f);
    let d_up = f.dot(up);
    if d_up.abs() <= d_right.abs() {
        let u = normalise(up - up * d_up);
        (f.cross(u), f, u)
    } else {
        let r = normalise(right - f * d_right);
        (r, f, r.cross(f))
    }
}

/// 00913440's normalisation: divide when the length is above 1e-8, else keep the vector.
fn normalise(v: Vec3) -> Vec3 {
    let len = v.length();
    if len > 1e-8 { v * (1.0 / len) } else { v }
}

/// What the component asks of the world after a call. [game] each comes from the handler named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Put the rig back at the transform it was built with and restart its animation (Trigger, 007d03a0:
    /// `character_set_affine` from the copy at +0xc4, then `basic_animation_restart` 0053d420).
    ResetRig,
    /// Push this camera's behaviour onto the game camera (`camera_behaviour_switch` 0091db00).
    CameraOn,
    /// Take it off again (0091db80: the parent behaviour restarts from the current view).
    CameraOff,
    /// Fire the `trigDone` links.
    TrigDone,
    /// Ask for per-step updates (005785d0), so the skip button is watched.
    WatchSkip,
}

/// The component's state. [game] fields of the 0x118-byte object.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimatedCamera {
    /// `buttonThrough` (+0xc2): the player may skip with the button the update watches.
    pub button_through: bool,
    /// +0xc1: set by Trigger, never cleared by the component itself. [game]
    pub triggered: bool,
    /// +0xbc: the behaviour is live (the camera is the rig's).
    pub active: bool,
    /// The behaviour's field of view (+0x14), degrees.
    pub fov: f32,
    /// The depth-of-field override (globals; one per game, kept here for the port).
    pub dof: DofOverride,
}

impl AnimatedCamera {
    pub fn new(button_through: bool) -> Self {
        Self { button_through, triggered: false, active: false, fov: DEFAULT_FOV, dof: DofOverride::default() }
    }

    /// Slot 6 (007d03a0). Does nothing while the camera is already on. A fresh behaviour starts at 60 degrees.
    /// [game] the update request needs `buttonThrough`, the loaded flag (always set by the loader) and bit 1 of
    /// the component flags +0x16 clear; [assumed] that bit is clear (no one sets it in the levels read).
    pub fn trigger(&mut self) -> Vec<Action> {
        if self.active {
            return Vec::new();
        }
        self.active = true;
        self.fov = DEFAULT_FOV;
        self.triggered = true;
        let mut out = vec![Action::ResetRig, Action::CameraOn];
        if self.button_through {
            out.push(Action::WatchSkip);
        }
        out
    }

    /// Slot 7 (007d05d0), also the body of teardown (007d0200) and of ENABLE(false): the camera off and the
    /// depth of field back to its defaults. No `trigDone`.
    pub fn untrigger(&mut self) -> Vec<Action> {
        if !self.active {
            return Vec::new();
        }
        self.active = false;
        self.dof = DofOverride::default();
        vec![Action::CameraOff]
    }

    /// Slot 2 (007d0290): when the player's pad record (`007212f0`, +0x94) has bit 0x1000, the cut is skipped:
    /// Untrigger and `trigDone`. [game] the bit; [unknown] which button it is (`notes\input.md` has no map
    /// of the record's +0x94). Only runs once [`Action::WatchSkip`] was asked for.
    pub fn step(&mut self, skip_bit: bool) -> Vec<Action> {
        if !skip_bit {
            return Vec::new();
        }
        let mut out = self.untrigger();
        out.push(Action::TrigDone);
        out
    }

    /// A message from the rig. Values for the float messages; `on` for ENABLE. Unknown names do nothing.
    pub fn message(&mut self, name: u32, value: f32, on: bool) -> Vec<Action> {
        match name {
            // 007d0660: only after a Trigger; `trigDone` follows the Untrigger (the loaded flag is always set).
            msg::RIG_DONE if self.triggered => {
                let mut out = self.untrigger();
                out.push(Action::TrigDone);
                out
            }
            // 007d07b0: on without resetting the rig or touching `triggered`; off as Untrigger.
            msg::ENABLE if on => {
                if self.active {
                    return Vec::new();
                }
                self.active = true;
                self.fov = DEFAULT_FOV;
                vec![Action::CameraOn]
            }
            msg::ENABLE => self.untrigger(),
            // 007d0d30..007d0df0: only while the behaviour lives.
            msg::FOV if self.active => {
                self.fov = value * FOV_MESSAGE_SCALE;
                Vec::new()
            }
            msg::DOF_NEAR_FOCUS | msg::DOF_FAR_FOCUS | msg::DOF_NEAR_FALLOFF | msg::DOF_FAR_FALLOFF if self.active => {
                let slot = match name {
                    msg::DOF_NEAR_FOCUS => &mut self.dof.near_focus,
                    msg::DOF_FAR_FOCUS => &mut self.dof.far_focus,
                    msg::DOF_NEAR_FALLOFF => &mut self.dof.near_falloff,
                    _ => &mut self.dof.far_falloff,
                };
                *slot = value;
                self.dof.active = true;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fov_scale_is_three_quarters_of_degrees_per_radian() {
        assert!((FOV_MESSAGE_SCALE - 0.75 * 180.0 / std::f32::consts::PI).abs() < 1e-4);
    }

    #[test]
    fn level_rig_keeps_its_axes() {
        let (r, f, u) = rig_frame(Vec3::X, Vec3::Y * 3.0, Vec3::Z);
        assert!((r - Vec3::X).length() < 1e-6 && (f - Vec3::Y).length() < 1e-6 && (u - Vec3::Z).length() < 1e-6);
    }

    #[test]
    fn up_branch_keeps_the_originals_unsquared_up() {
        // Forward tilted down a little: |f.up| (0.2) <= |f.right| is false for right = X (0), so use a rig whose
        // right leans into forward to take the up branch.
        let f = Vec3::new(0.0, 1.0, -0.2).normalize();
        let right = Vec3::new(1.0, 0.5, 0.0);
        let up = Vec3::Z;
        let (r, ff, u) = rig_frame(right, f, up);
        assert!(f.dot(up).abs() <= right.dot(f).abs());
        // Up is only normalised: still straight up, not square to forward. [game] the quirk.
        assert!((u - Vec3::Z).length() < 1e-6);
        assert!((r - ff.cross(u)).length() < 1e-6);
    }

    #[test]
    fn right_branch_squares_right_to_forward() {
        let f = Vec3::new(0.0, 0.3, -1.0).normalize();
        let (r, ff, u) = rig_frame(Vec3::new(1.0, 0.0, 0.1), f, Vec3::Z);
        assert!(r.dot(ff).abs() < 1e-6 && (r.length() - 1.0).abs() < 1e-6);
        assert!((u - r.cross(ff)).length() < 1e-6);
    }

    #[test]
    fn trigger_messages_and_done() {
        let mut c = AnimatedCamera::new(true);
        assert!(c.message(msg::FOV, 1.0, false).is_empty() && c.fov == DEFAULT_FOV, "FOV ignored while off");
        assert_eq!(c.trigger(), vec![Action::ResetRig, Action::CameraOn, Action::WatchSkip]);
        assert!(c.trigger().is_empty(), "a second Trigger does nothing");
        c.message(msg::FOV, 1.0, false);
        assert!((c.fov - FOV_MESSAGE_SCALE).abs() < 1e-6);
        c.message(msg::DOF_FAR_FOCUS, 40.0, false);
        assert!(c.dof.active && c.dof.far_focus == 40.0);
        assert_eq!(c.message(msg::RIG_DONE, 0.0, false), vec![Action::CameraOff, Action::TrigDone]);
        assert_eq!(c.dof, DofOverride::default());
    }

    #[test]
    fn enable_and_skip() {
        let mut c = AnimatedCamera::new(false);
        assert!(c.message(msg::RIG_DONE, 0.0, false).is_empty(), "done before any Trigger does nothing");
        assert_eq!(c.message(msg::ENABLE, 0.0, true), vec![Action::CameraOn]);
        assert!(!c.triggered);
        assert_eq!(c.step(true), vec![Action::CameraOff, Action::TrigDone]);
        // 007d0290 calls Untrigger and then sends trigDone either way.
        assert_eq!(c.step(true), vec![Action::TrigDone]);
        assert!(c.step(false).is_empty());
    }
}
