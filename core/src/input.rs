//! The PC input path, from the devices to the pad state the game reads (`notes\input.md`).
//!
//! [game] Once per rendered frame (`main_update_scheduler` 007799f0 calls `platform_input_poll_update`
//! 01174500 before the scheduler step) the Beenox layer polls the keyboard, the mouse and the pads, then
//! every one of the 24 actions takes the MAXIMUM of its bindings' values (004f0690; devices are not
//! added), held actions that are latched read 0 until let go (004f0640), the actions become an
//! XInput-shaped pad (0050b6b0) and the engine's pad filter normalises it with axial dead zones
//! (`pad_dead_zone` 005aa890, defaults 005aa700). The game's commands then test digital bits of that
//! filtered pad (`pad_button_down` 0084f900, the `Controller` table's names to bits at 00b8d9d8).
//!
//! There is no smoothing anywhere on this path. Mouse counts are what the device reports since the last
//! poll, so a frame's counts over the sensitivity divisor make a stick value for that frame.

use crate::formats::bxdata::{Archive, Element};

/// The 24 actions, in the order of `Beenox\actions.bin` and of the action object's values. [data]
pub const ACTIONS: [&str; 24] = [
    "DpadUp", "DpadDown", "DpadLeft", "DpadRight", "ButtonBack", "ButtonStart", "ButtonLThumb", "ButtonRThumb",
    "ButtonLShoulder", "ButtonRShoulder", "ButtonA", "ButtonB", "ButtonX", "ButtonY", "LTrigger", "RTrigger",
    "LThumbUp", "LThumbDown", "LThumbLeft", "LThumbRight", "RThumbUp", "RThumbDown", "RThumbLeft", "RThumbRight",
];

/// Action indices.
pub mod action {
    pub const DPAD_UP: usize = 0;
    pub const DPAD_DOWN: usize = 1;
    pub const DPAD_LEFT: usize = 2;
    pub const DPAD_RIGHT: usize = 3;
    pub const BACK: usize = 4;
    pub const START: usize = 5;
    pub const L_THUMB: usize = 6;
    pub const R_THUMB: usize = 7;
    pub const L_SHOULDER: usize = 8;
    pub const R_SHOULDER: usize = 9;
    pub const A: usize = 10;
    pub const B: usize = 11;
    pub const X: usize = 12;
    pub const Y: usize = 13;
    pub const L_TRIGGER: usize = 14;
    pub const R_TRIGGER: usize = 15;
    pub const L_UP: usize = 16;
    pub const L_DOWN: usize = 17;
    pub const L_LEFT: usize = 18;
    pub const L_RIGHT: usize = 19;
    pub const R_UP: usize = 20;
    pub const R_DOWN: usize = 21;
    pub const R_LEFT: usize = 22;
    pub const R_RIGHT: usize = 23;
}

/// XInput's button bits, as the bridge writes them and the pad getter tests them. [game] (0050b6b0, 004ec6c0)
pub mod bit {
    pub const DPAD_UP: u16 = 0x0001;
    pub const DPAD_DOWN: u16 = 0x0002;
    pub const DPAD_LEFT: u16 = 0x0004;
    pub const DPAD_RIGHT: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const L_THUMB: u16 = 0x0040;
    pub const R_THUMB: u16 = 0x0080;
    pub const L_SHOULDER: u16 = 0x0100;
    pub const R_SHOULDER: u16 = 0x0200;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
    pub const X: u16 = 0x4000;
    pub const Y: u16 = 0x8000;
}

/// The filter's own digital trigger bits (005aa890, when its flags have 0x20), and the `Controller` table's
/// names for them: `L2` and `R2` (00b8d9d8). [game]
pub const L2_BIT: u32 = 0x400;
pub const R2_BIT: u32 = 0x800;

/// The digital button bit (if any) of each action, the bridge's table. [game] 0050b6b0
const ACTION_BITS: [u16; 14] = [
    bit::DPAD_UP, bit::DPAD_DOWN, bit::DPAD_LEFT, bit::DPAD_RIGHT, bit::BACK, bit::START, bit::L_THUMB, bit::R_THUMB,
    bit::L_SHOULDER, bit::R_SHOULDER, bit::A, bit::B, bit::X, bit::Y,
];

/// The XInput pad state (`XINPUT_GAMEPAD`): what the pad device reads and what the bridge writes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pad {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
}

/// The mouse as DirectInput reports it at a poll: relative counts since the last poll (x right, y DOWN, the
/// wheel up), and the eight buttons. [game] 004f09b0
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mouse {
    pub dx: i32,
    pub dy: i32,
    pub wheel: i32,
    pub buttons: [bool; 8],
}

/// The divisor the executable starts with (`00bf361c`). [game]
pub const MOUSE_DIVISOR_DEFAULT: i32 = 10;

/// The options' mouse sensitivity (0..1) as a divisor: float32 `(1.05 - s) * 20`, truncated. [game] 008e8bb0;
/// the option menu clamps the setting to 0..1 (008e21a0). At 1 this is 0, which the mouse getter turns into 1.
pub fn mouse_divisor(setting: f32) -> i32 {
    let s = setting.clamp(0.0, 1.0);
    ((1.05f32 - s) * 20.0f32) as i32
}

/// One direction's counts as a value: none below zero, 1 above the divisor, else counts / divisor. [game] 004f0e30
pub fn mouse_count_value(count: i32, divisor: i32) -> f32 {
    if count < 0 {
        0.0
    } else if count > divisor {
        1.0
    } else {
        count as f32 / divisor as f32
    }
}

/// The mouse's controls by index: 0 Up, 1 Down, 2 Left, 3 Right, 4 WheelUp, 5 WheelDown, 6..13 Button1..8.
/// [game] 004f0e60 (BXMouse slot 11), which first makes a zero divisor 1.
pub fn mouse_value(mouse: &Mouse, control: u8, divisor: i32) -> f32 {
    let divisor = if divisor == 0 { 1 } else { divisor };
    match control {
        0 => mouse_count_value(-mouse.dy, divisor),
        1 => mouse_count_value(mouse.dy, divisor),
        2 => mouse_count_value(-mouse.dx, divisor),
        3 => mouse_count_value(mouse.dx, divisor),
        4 => mouse_count_value(mouse.wheel, divisor),
        5 => mouse_count_value(-mouse.wheel, divisor),
        6..=13 => mouse.buttons[(control - 6) as usize] as u8 as f32,
        _ => 0.0,
    }
}

/// The mouse's control names (004f0ad0).
const MOUSE_CONTROLS: [&str; 14] = [
    "Mouse_Up", "Mouse_Down", "Mouse_Left", "Mouse_Right", "Mouse_WheelUp", "Mouse_WheelDown", "Mouse_Button1",
    "Mouse_Button2", "Mouse_Button3", "Mouse_Button4", "Mouse_Button5", "Mouse_Button6", "Mouse_Button7",
    "Mouse_Button8",
];

/// The XInput pad's controls by index, named as the actions: buttons 1 or 0, triggers byte / 255, each stick
/// direction its half of the axis, the positive half over 32767 and the negative over 32768. No dead zone here.
/// [game] 004ec6c0 and its case bodies 004ec6cb..004ecafb (constants 255.0, 32767.0, -1/32768).
pub fn pad_value(pad: &Pad, control: u8) -> f32 {
    let positive = |v: i16| if v > 0 { (v as f64 / 32767.0) as f32 } else { 0.0 };
    let negative = |v: i16| if v < 0 { (v as f64 * -3.0517578125e-05) as f32 } else { 0.0 };
    match control as usize {
        c @ 0..=13 => (pad.buttons & ACTION_BITS[c] != 0) as u8 as f32,
        action::L_TRIGGER => (pad.left_trigger as f64 / 255.0) as f32,
        action::R_TRIGGER => (pad.right_trigger as f64 / 255.0) as f32,
        action::L_UP => positive(pad.ly),
        action::L_DOWN => negative(pad.ly),
        action::L_LEFT => negative(pad.lx),
        action::L_RIGHT => positive(pad.lx),
        action::R_UP => positive(pad.ry),
        action::R_DOWN => negative(pad.ry),
        action::R_LEFT => negative(pad.rx),
        action::R_RIGHT => positive(pad.rx),
        _ => 0.0,
    }
}

/// The keyboard's control names, `Key_` and DirectInput's names, in the executable's table (00b4cc50..)
/// order, with its alternative spellings mapped to one name.
pub fn key_name(name: &str) -> Option<&'static str> {
    let bare = name.strip_prefix("Key_")?;
    let canon = match bare.to_ascii_uppercase().as_str() {
        "BACKSPACE" => "BACK",
        "LALT" => "LMENU",
        "RALT" => "RMENU",
        "CAPSLOCK" => "CAPITAL",
        "NUMPADSTAR" => "MULTIPLY",
        "NUMPADMINUS" => "SUBTRACT",
        "NUMPADPLUS" => "ADD",
        "NUMPADPERIOD" => "DECIMAL",
        "NUMPADSLASH" => "DIVIDE",
        "UPARROW" => "UP",
        "DOWNARROW" => "DOWN",
        "LEFTARROW" => "LEFT",
        "RIGHTARROW" => "RIGHT",
        "PGUP" => "PRIOR",
        "PGDN" => "NEXT",
        "LSHIFT" => "LSHIFT",
        "RSHIFT" => "RSHIFT",
        "LCONTROL" => "LCONTROL",
        "RCONTROL" => "RCONTROL",
        "KPENTER" => "NUMPADENTER",
        "SUB" => "SUBTRACT",
        "PAGEUP" => "PRIOR",
        "PAGEDOWN" => "NEXT",
        "PRINTSCREEN" => "SYSRQ",
        "ESCAPE" => "ESCAPE",
        "RETURN" => "RETURN",
        other => return KEYS.iter().copied().find(|k| *k == other),
    };
    Some(canon)
}

/// DirectInput key names the executable knows. [game] strings 00b4cc50..00b4d940
pub const KEYS: &[&str] = &[
    "ESCAPE", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "MINUS", "EQUALS", "BACK", "TAB", "Q", "W", "E", "R",
    "T", "Y", "U", "I", "O", "P", "LBRACKET", "RBRACKET", "RETURN", "LCONTROL", "A", "S", "D", "F", "G", "H", "J",
    "K", "L", "SEMICOLON", "APOSTROPHE", "GRAVE", "LSHIFT", "BACKSLASH", "Z", "X", "C", "V", "B", "N", "M", "COMMA",
    "PERIOD", "SLASH", "RSHIFT", "MULTIPLY", "LMENU", "SPACE", "CAPITAL", "F1", "F2", "F3", "F4", "F5", "F6", "F7",
    "F8", "F9", "F10", "NUMLOCK", "SCROLL", "NUMPAD7", "NUMPAD8", "NUMPAD9", "SUBTRACT", "NUMPAD4", "NUMPAD5",
    "NUMPAD6", "ADD", "NUMPAD1", "NUMPAD2", "NUMPAD3", "NUMPAD0", "DECIMAL", "OEM_102", "F11", "F12", "F13", "F14",
    "F15", "KANA", "ABNT_C1", "CONVERT", "NOCONVERT", "YEN", "ABNT_C2", "NUMPADEQUALS", "PREVTRACK", "AT", "COLON",
    "UNDERLINE", "KANJI", "STOP", "AX", "UNLABELED", "NEXTTRACK", "NUMPADENTER", "RCONTROL", "MUTE", "CALCULATOR",
    "PLAYPAUSE", "MEDIASTOP", "VOLUMEDOWN", "VOLUMEUP", "WEBHOME", "NUMPADCOMMA", "DIVIDE", "SYSRQ", "RMENU",
    "PAUSE", "HOME", "UP", "PRIOR", "LEFT", "RIGHT", "END", "DOWN", "NEXT", "INSERT", "DELETE", "LWIN", "RWIN",
    "APPS", "POWER", "SLEEP", "WAKE", "WEBSEARCH", "WEBFAVORITES", "WEBREFRESH", "WEBSTOP", "WEBFORWARD", "WEBBACK",
    "MYCOMPUTER", "MAIL", "MEDIASELECT", "CIRCUMFLEX",
];

/// One physical control a binding names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// A key, by its DirectInput name (`key_name`): 1 held, else 0 (004ed650: the scancode's bit 0x80).
    Key(&'static str),
    Mouse(u8),
    Pad(u8),
}

/// The physical devices' state at one poll.
pub struct Devices<'a> {
    /// Is the key (DirectInput name) down?
    pub key_down: &'a dyn Fn(&str) -> bool,
    pub mouse: Mouse,
    pub mouse_divisor: i32,
    /// The pads in use. [assumed] every connected pad is read as the XInput device and they combine by maximum
    /// (OPEN, settles: the XInput user index the game opens per player slot, read at its device
    /// setup, and a two-pad run)
    /// like separate devices; the original keeps one XInput device per player slot.
    pub pads: &'a [Pad],
}

impl Devices<'_> {
    pub fn value(&self, control: Control) -> f32 {
        match control {
            Control::Key(k) => (self.key_down)(k) as u8 as f32,
            Control::Mouse(c) => mouse_value(&self.mouse, c, self.mouse_divisor),
            Control::Pad(c) => self.pads.iter().map(|p| pad_value(p, c)).fold(0.0, f32::max),
        }
    }
}

/// An action map: each action's bindings across the devices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionMap {
    pub bindings: [Vec<Control>; 24],
    /// Actions suppressed while held when the map is activated (the map's flags at +0x3c, 004f0530).
    /// [game] none: the map record's constructor (0116adc0) fills the 24 flags from its byte +0x48, which is
    /// zero in the static records (00d0b1d0, seven of 0x4c), and no input function writes them after.
    pub latch_on_activate: [bool; 24],
}

impl ActionMap {
    /// The shipped default map of a context (`Robot`, `Weapon`, `Vehicle`, `Flight`): the keyboard's and the
    /// mouse's maps for it and the XInput device's (which ships only for `Robot`). [data] `default_bindings.bin`
    /// Unknown control names are skipped, as the device map reader does (004eed30).
    pub fn defaults(bindings: &Element, context: &str) -> Result<ActionMap, String> {
        let mut map = ActionMap::default();
        for (device, ctx) in [("Keyboard", context), ("Mouse", context), ("XInput Device", "Robot")] {
            let m = bindings
                .children
                .iter()
                .find(|m| m.get("device") == Some(device) && m.get("context") == Some(ctx))
                .ok_or_else(|| format!("no {device} map for {ctx}"))?;
            for b in &m.children {
                let (Some(name), Some(control)) = (b.get("action"), b.get("primary")) else { continue };
                let Some(a) = ACTIONS.iter().position(|x| *x == name) else { continue };
                if let Some(c) = parse_control(device, control) {
                    map.bindings[a].push(c);
                }
            }
        }
        Ok(map)
    }

    /// The defaults read from the install's archive.
    pub fn install_defaults(archive: &Archive, context: &str) -> Result<ActionMap, String> {
        ActionMap::defaults(&archive.tree(r"beenox\default_bindings.bin")?, context)
    }
}

fn parse_control(device: &str, name: &str) -> Option<Control> {
    if name.is_empty() {
        return None;
    }
    match device {
        "Keyboard" => key_name(name).map(Control::Key),
        "Mouse" => MOUSE_CONTROLS.iter().position(|m| m.eq_ignore_ascii_case(name)).map(|i| Control::Mouse(i as u8)),
        _ => ACTIONS.iter().position(|a| a.eq_ignore_ascii_case(name)).map(|i| Control::Pad(i as u8)),
    }
}

/// The 24 action values and their latches. [game] 004f0470 (construct), 004f0690, 004f0640, 004f0530.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Actions {
    pub values: [f32; 24],
    pub latched: [bool; 24],
}

impl Actions {
    /// Each action's value is the largest of its bindings' values. [game] 004f0690
    fn query(&mut self, map: &ActionMap, devices: &Devices) {
        for (v, b) in self.values.iter_mut().zip(&map.bindings) {
            *v = b.iter().map(|&c| devices.value(c)).fold(0.0, f32::max);
        }
    }

    /// One poll: query, then a latched action reads 0 while over 0.5, and its latch clears at 0.5 or below.
    /// [game] 004f0640
    pub fn update(&mut self, map: &ActionMap, devices: &Devices) {
        self.query(map, devices);
        for (v, l) in self.values.iter_mut().zip(self.latched.iter_mut()) {
            if *v <= 0.5 {
                *l = false;
            } else if *l {
                *v = 0.0;
            }
        }
    }

    /// A map becomes the active one: query it, and every action that `latch_all` or the map's own flag names is
    /// latched if held over 0.5 (read as 0 until released) or unlatched. [game] 004f0530; the form change
    /// (0071ab70 via 0050b660) asks `latch_all` only when the context was 4, the menus'.
    pub fn activate(&mut self, map: &ActionMap, devices: &Devices, latch_all: bool) {
        self.query(map, devices);
        for i in 0..24 {
            if latch_all || map.latch_on_activate[i] {
                if self.values[i] <= 0.5 {
                    self.latched[i] = false;
                } else {
                    self.latched[i] = true;
                    self.values[i] = 0.0;
                }
            }
        }
    }
}

/// The actions as an XInput pad: buttons over 0.5, triggers `trunc(v * 255)`, each stick axis from its two
/// directions after a direction under 0.15 is made 0: positive * 32767 - negative * 32768, truncated. [game]
/// 0050b6b0, disassembly 0050b83d..0050b9e7 (truncating control word; `runtime_float_to_integer` 00a8b5e0 is
/// MSVC's truncating float-to-integer).
pub fn bridge(values: &[f32; 24]) -> Pad {
    let mut buttons = 0u16;
    for (i, b) in ACTION_BITS.iter().enumerate() {
        if values[i] > 0.5 {
            buttons |= b;
        }
    }
    let byte = |v: f32| ((v as f64) * 255.0).trunc() as i64 as u8;
    let cut = |v: f32| if v < 0.15 { 0.0 } else { v as f64 };
    let axis = |pos: usize, neg: usize| (cut(values[pos]) * 32767.0 - cut(values[neg]) * 32768.0).trunc() as i64 as i16;
    Pad {
        buttons,
        left_trigger: byte(values[action::L_TRIGGER]),
        right_trigger: byte(values[action::R_TRIGGER]),
        lx: axis(action::L_RIGHT, action::L_LEFT),
        ly: axis(action::L_UP, action::L_DOWN),
        rx: axis(action::R_RIGHT, action::R_LEFT),
        ry: axis(action::R_UP, action::R_DOWN),
    }
}

/// One axis of the engine's pad filter: raw range, dead zone, scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub min: f32,
    pub max: f32,
    pub dead_zone: f32,
    pub scale: f32,
}

impl Axis {
    /// The raw value made -1..1 over the range, zero inside the dead zone, the rest stretched to reach 1 again,
    /// then scaled; outside -1..1 it is the scale itself. [game] 005aa890
    pub fn apply(&self, raw: f32) -> f32 {
        let v = (((raw as f64 - self.min as f64) * 2.0) / (self.max as f64 - self.min as f64) - 1.0) as f32;
        let dz = self.dead_zone;
        if v.abs() < dz {
            return 0.0;
        }
        let r = if v <= 0.0 { (dz + v) / (1.0 - dz) } else { (v - dz) / (1.0 - dz) };
        if r < -1.0 {
            -self.scale
        } else if r > 1.0 {
            self.scale
        } else {
            self.scale * r
        }
    }
}

/// The engine's pad filter (`lux::Controller`, 005aa890) with its defaults (005aa700): sticks over
/// -32768..32767 with Y the other way round (32768..-32767, so positive Y is DOWN after it), axial dead zone 0.25;
/// triggers over 0..255 (so -1 at rest, 1 full) with dead zone 0.1; flags 0x17f (0x20: the trigger bits).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadFilter {
    pub lx: Axis,
    pub ly: Axis,
    pub rx: Axis,
    pub ry: Axis,
    pub lt: Axis,
    pub rt: Axis,
    pub flags: u32,
}

impl Default for PadFilter {
    fn default() -> Self {
        let x = Axis { min: -32768.0, max: 32767.0, dead_zone: 0.25, scale: 1.0 };
        let y = Axis { min: 32768.0, max: -32767.0, dead_zone: 0.25, scale: 1.0 };
        let t = Axis { min: 0.0, max: 255.0, dead_zone: 0.1, scale: 1.0 };
        PadFilter { lx: x, ly: y, rx: x, ry: y, lt: t, rt: t, flags: 0x17f }
    }
}

/// The filtered pad the game reads (controller +0x90..+0xb0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Filtered {
    /// XInput's bits, and `L2_BIT` / `R2_BIT`.
    pub buttons: u32,
    /// Sticks -1..1, x right, y DOWN (the engine's).
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    /// Triggers -1 (at rest) .. 1.
    pub lt: f32,
    pub rt: f32,
}

impl PadFilter {
    /// [game] 005aa890's normal path. L2 is down when its value is over 0 (raw 141 and up), R2 when over -0.999
    /// (raw 1 and up). The branch at +0xbc is `if (byte +0xbc) pad_rumble_update 005aae80`: a call
    /// that only drives the motors, so the values are the same whichever way it goes. [game 005aa890, 005aae80]
    /// (The default of byte +0xbc is not read; the port has no motor state here.)
    pub fn apply(&self, pad: &Pad) -> Filtered {
        let mut f = Filtered {
            buttons: pad.buttons as u32,
            lx: self.lx.apply(pad.lx as f32),
            ly: self.ly.apply(pad.ly as f32),
            rx: self.rx.apply(pad.rx as f32),
            ry: self.ry.apply(pad.ry as f32),
            lt: self.lt.apply(pad.left_trigger as f32),
            rt: self.rt.apply(pad.right_trigger as f32),
        };
        if self.flags & 0x20 != 0 {
            if f.lt > 0.0 {
                f.buttons |= L2_BIT;
            }
            if f.rt > -0.999 {
                f.buttons |= R2_BIT;
            }
        }
        f
    }
}

impl Filtered {
    pub fn down(&self, bits: u32) -> bool {
        self.buttons & bits != 0
    }

    /// A trigger as the car reads it: `(t + 1) / 2` (`notes\driving.md`, step 1 of the update).
    pub fn trigger01(t: f32) -> f32 {
        (t + 1.0) * 0.5
    }
}

/// The port's own keyboard and mouse layout, asked for by the user on 2026-10-21 in place of the
/// shipped one (`--classic-controls` keeps the shipped one). [port] Not the original's: a layout in
/// the manner of current third-person action games, over the same 24 actions, so everything after
/// the actions (bridge, filter, the commands' bits) is unchanged, and the pad keeps its shipped map.
///
/// | Key | Action | Use |
/// | --- | --- | --- |
/// | W A S D | left stick | move, steer |
/// | arrows | right stick | camera (the mouse turns it directly, not through the stick) |
/// | Space | A | jump; turbo |
/// | Left Shift (hold) | RTrigger | vehicle form and throttle |
/// | right mouse | LTrigger | weapon mode; drift |
/// | left mouse | X, or RTrigger while the right is held | melee and the car's gun; fire |
/// | Left Ctrl | X | dodge and quick attack |
/// | E | B | action, climb, pick up, throw |
/// | Q | Y | special |
/// | Shift + S | LShoulder | brake |
/// | wheel, R | RShoulder | next weapon |
/// | middle mouse | RThumb | camera reset |
/// | Z | LThumb | strafe mode |
/// | J / L | d-pad left / right | as shipped |
/// | Tab / Esc | Back / Start | as shipped |
///
/// The left mouse button and the brake depend on other keys, so they are not in the map but in
/// `modern_context`.
pub fn modern_map(shipped: &ActionMap) -> ActionMap {
    use action::*;
    let mut map = ActionMap::default();
    // The pad's bindings as shipped.
    for (to, from) in map.bindings.iter_mut().zip(&shipped.bindings) {
        to.extend(from.iter().copied().filter(|c| matches!(c, Control::Pad(_))));
    }
    let key = |n: &str| Control::Key(KEYS.iter().copied().find(|k| *k == n).expect("a DirectInput key name"));
    let mouse = |n: &str| Control::Mouse(MOUSE_CONTROLS.iter().position(|m| *m == n).expect("a mouse control") as u8);
    for (a, c) in [
        (L_UP, key("W")), (L_DOWN, key("S")), (L_LEFT, key("A")), (L_RIGHT, key("D")),
        (R_UP, key("UP")), (R_DOWN, key("DOWN")), (R_LEFT, key("LEFT")), (R_RIGHT, key("RIGHT")),
        (A, key("SPACE")), (R_TRIGGER, key("LSHIFT")), (L_TRIGGER, mouse("Mouse_Button2")),
        (X, key("LCONTROL")), (B, key("E")), (Y, key("Q")),
        (R_SHOULDER, mouse("Mouse_WheelUp")), (R_SHOULDER, mouse("Mouse_WheelDown")), (R_SHOULDER, key("R")),
        (R_THUMB, mouse("Mouse_Button3")), (L_THUMB, key("Z")),
        (DPAD_LEFT, key("J")), (DPAD_RIGHT, key("L")), (BACK, key("TAB")), (START, key("ESCAPE")),
    ] {
        map.bindings[a].push(c);
    }
    map
}

/// The modern layout's two bindings that depend on other keys (`modern_map`). The left mouse
/// button is the fire trigger while weapon mode is held and the melee / car's gun button
/// otherwise; S is also the brake while the vehicle trigger is held. [port]
pub fn modern_context(values: &mut [f32; 24], devices: &Devices) {
    use action::*;
    if devices.mouse.buttons[0] {
        let a = if values[L_TRIGGER] > 0.5 { R_TRIGGER } else { X };
        values[a] = 1.0;
    }
    if (devices.key_down)("LSHIFT") && (devices.key_down)("S") {
        values[L_SHOULDER] = 1.0;
    }
}

/// The whole path for one poll: the actions are updated from the devices, bridged, and filtered.
#[derive(Clone, Debug, Default)]
pub struct InputPath {
    pub map: ActionMap,
    pub actions: Actions,
    pub filter: PadFilter,
    /// The bridged pad of the last poll (before the filter).
    pub pad: Pad,
    /// The port's modern layout's context rules apply (`modern_context`).
    pub modern: bool,
}

impl InputPath {
    pub fn new(map: ActionMap) -> Self {
        InputPath { map, ..Default::default() }
    }

    /// The modern layout over the shipped map's pad bindings (`modern_map`).
    pub fn modern(shipped: &ActionMap) -> Self {
        InputPath { map: modern_map(shipped), modern: true, ..Default::default() }
    }

    pub fn poll(&mut self, devices: &Devices) -> Filtered {
        self.actions.update(&self.map, devices);
        if self.modern {
            modern_context(&mut self.actions.values, devices);
        }
        self.pad = bridge(&self.actions.values);
        self.filter.apply(&self.pad)
    }
}

/// The modern layout's mouse look: the view turns by an angle per count, as current games do, not
/// at a stick's rate. [port] `degrees` per count; x right and y up, radians.
pub fn mouse_turn(mouse: &Mouse, degrees: f32) -> glam::Vec2 {
    glam::Vec2::new(mouse.dx as f32, -mouse.dy as f32) * degrees.to_radians()
}

/// The modern layout's default degrees per mouse count. [port] Roughly a shooter's usual feel:
/// about 50 degrees per inch at 800 counts per inch.
pub const MOUSE_DEGREES_DEFAULT: f32 = 0.065;

#[cfg(test)]
mod tests {
    use super::*;

    fn devices<'a>(keys: &'a dyn Fn(&str) -> bool, mouse: Mouse, pads: &'a [Pad]) -> Devices<'a> {
        Devices { key_down: keys, mouse, mouse_divisor: MOUSE_DIVISOR_DEFAULT, pads }
    }

    #[test]
    fn mouse_divisor_follows_the_setting() {
        // float32 rounding: 0.5 gives the executable's initial 10 (1.05f32 - 0.5 = 0.54999995), 0.55 gives 9.
        assert_eq!(mouse_divisor(0.5), 10);
        assert_eq!(mouse_divisor(0.55), 9);
        assert_eq!(mouse_divisor(0.0), 21);
        // float32 (1.05 - 1) * 20 is just under 1: the setter writes 0 and the getter reads it as 1.
        assert_eq!(mouse_divisor(1.0), 0);
        let m = Mouse { dx: 3, ..Default::default() };
        assert_eq!(mouse_value(&m, 3, 0), 1.0);
    }

    #[test]
    fn mouse_counts_split_by_direction_and_clamp() {
        let m = Mouse { dx: 3, dy: -25, wheel: 1, buttons: [true, false, false, false, false, false, false, false] };
        assert_eq!(mouse_value(&m, 3, 10), 0.3);
        assert_eq!(mouse_value(&m, 2, 10), 0.0);
        assert_eq!(mouse_value(&m, 0, 10), 1.0); // up: 25 counts over a divisor of 10
        assert_eq!(mouse_value(&m, 1, 10), 0.0);
        assert_eq!(mouse_value(&m, 4, 10), 0.1);
        assert_eq!(mouse_value(&m, 6, 10), 1.0);
        assert_eq!(mouse_value(&m, 7, 10), 0.0);
        assert_eq!(mouse_count_value(10, 10), 1.0);
        assert_eq!(mouse_count_value(11, 10), 1.0);
    }

    #[test]
    fn pad_values_split_each_axis() {
        let p = Pad { buttons: bit::A | bit::BACK, left_trigger: 255, right_trigger: 51, lx: -32768, ly: 32767, rx: 16384, ry: 0 };
        assert_eq!(pad_value(&p, action::A as u8), 1.0);
        assert_eq!(pad_value(&p, action::BACK as u8), 1.0);
        assert_eq!(pad_value(&p, action::START as u8), 0.0);
        assert_eq!(pad_value(&p, action::L_TRIGGER as u8), 1.0);
        assert_eq!(pad_value(&p, action::R_TRIGGER as u8), 0.2);
        assert_eq!(pad_value(&p, action::L_LEFT as u8), 1.0);
        assert_eq!(pad_value(&p, action::L_RIGHT as u8), 0.0);
        assert_eq!(pad_value(&p, action::L_UP as u8), 1.0);
        assert!((pad_value(&p, action::R_RIGHT as u8) - 0.50002).abs() < 1e-5);
    }

    #[test]
    fn bindings_combine_by_maximum_not_sum() {
        let mut map = ActionMap::default();
        map.bindings[action::R_RIGHT] = vec![Control::Key("RIGHT"), Control::Mouse(3)];
        let keys = |_: &str| false;
        let d = devices(&keys, Mouse { dx: 4, ..Default::default() }, &[]);
        let mut a = Actions::default();
        a.update(&map, &d);
        assert_eq!(a.values[action::R_RIGHT], 0.4);
        let keys = |k: &str| k == "RIGHT";
        let d = devices(&keys, Mouse { dx: 4, ..Default::default() }, &[]);
        a.update(&map, &d);
        assert_eq!(a.values[action::R_RIGHT], 1.0);
    }

    #[test]
    fn latched_actions_read_zero_until_released() {
        let mut map = ActionMap::default();
        map.bindings[action::R_TRIGGER] = vec![Control::Key("LSHIFT")];
        let held = |k: &str| k == "LSHIFT";
        let free = |_: &str| false;
        let mut a = Actions::default();
        a.activate(&map, &devices(&held, Mouse::default(), &[]), true);
        assert_eq!(a.values[action::R_TRIGGER], 0.0);
        a.update(&map, &devices(&held, Mouse::default(), &[]));
        assert_eq!(a.values[action::R_TRIGGER], 0.0);
        a.update(&map, &devices(&free, Mouse::default(), &[]));
        a.update(&map, &devices(&held, Mouse::default(), &[]));
        assert_eq!(a.values[action::R_TRIGGER], 1.0);
        // Without the latch request a held action carries on through the change.
        let mut b = Actions::default();
        b.activate(&map, &devices(&held, Mouse::default(), &[]), false);
        assert_eq!(b.values[action::R_TRIGGER], 1.0);
    }

    #[test]
    fn modern_layout_left_mouse_and_brake_follow_context() {
        let mut shipped = ActionMap::default();
        shipped.bindings[action::A] = vec![Control::Pad(action::A as u8), Control::Key("SPACE")];
        let mut path = InputPath::modern(&shipped);
        // The pad's binding is kept, the shipped keyboard one is not doubled.
        assert_eq!(path.map.bindings[action::A], vec![Control::Pad(action::A as u8), Control::Key("SPACE")]);
        let none = |_: &str| false;
        let lmb = Mouse { buttons: [true, false, false, false, false, false, false, false], ..Default::default() };
        let lmb_rmb = Mouse { buttons: [true, true, false, false, false, false, false, false], ..Default::default() };
        // Left mouse alone: melee.
        let f = path.poll(&devices(&none, lmb, &[]));
        assert!(f.down(bit::X as u32) && !f.down(R2_BIT));
        // With the right held: weapon mode and fire, no melee.
        let f = path.poll(&devices(&none, lmb_rmb, &[]));
        assert!(f.down(L2_BIT) && f.down(R2_BIT) && !f.down(bit::X as u32));
        // S brakes only with the vehicle trigger held.
        let s = |k: &str| k == "S";
        assert!(!path.poll(&devices(&s, Mouse::default(), &[])).down(bit::L_SHOULDER as u32));
        let shift_s = |k: &str| k == "S" || k == "LSHIFT";
        let f = path.poll(&devices(&shift_s, Mouse::default(), &[]));
        assert!(f.down(bit::L_SHOULDER as u32) && f.down(R2_BIT));
        // The mouse moves no stick in this layout.
        let f = path.poll(&devices(&none, Mouse { dx: 50, dy: -50, ..Default::default() }, &[]));
        assert_eq!((f.rx, f.ry), (0.0, 0.0));
        let turn = mouse_turn(&Mouse { dx: 10, dy: -20, ..Default::default() }, 0.5);
        assert!((turn.x - 5f32.to_radians()).abs() < 1e-6 && (turn.y - 10f32.to_radians()).abs() < 1e-6);
    }

    #[test]
    fn bridge_matches_the_disassembly() {
        let mut v = [0.0f32; 24];
        v[action::A] = 1.0;
        v[action::B] = 0.5; // not over 0.5
        v[action::L_UP] = 1.0;
        v[action::L_LEFT] = 1.0;
        v[action::R_RIGHT] = 0.3;
        v[action::R_UP] = 0.14; // under 0.15: dropped
        v[action::R_TRIGGER] = 1.0;
        v[action::L_TRIGGER] = 0.999;
        let p = bridge(&v);
        assert_eq!(p.buttons, bit::A);
        assert_eq!(p.ly, 32767);
        assert_eq!(p.lx, -32768);
        assert_eq!(p.rx, 9830); // trunc(0.3f32 * 32767)
        assert_eq!(p.ry, 0);
        assert_eq!(p.right_trigger, 255);
        assert_eq!(p.left_trigger, 254);
    }

    #[test]
    fn filter_dead_zones_and_trigger_bits() {
        let f = PadFilter::default();
        // Full up on the keyboard: raw +32767 is engine Y about -1 (positive is down).
        let up = f.apply(&Pad { ly: 32767, ..Default::default() });
        assert!((up.ly + 1.0).abs() < 1e-4);
        // Three mouse counts at the default divisor: 0.3, bridged to 9830, then the 0.25 dead zone.
        let mouse = f.apply(&Pad { rx: 9830, ..Default::default() });
        assert!((mouse.rx - 0.0667).abs() < 1e-3, "{}", mouse.rx);
        // Two counts (0.2) are inside the dead zone.
        assert_eq!(f.apply(&Pad { rx: 6553, ..Default::default() }).rx, 0.0);
        // Triggers at rest are -1 and not down; R2 is down from raw 1, L2 from raw 141.
        let rest = f.apply(&Pad::default());
        assert_eq!(rest.rt, -1.0);
        assert!(!rest.down(R2_BIT) && !rest.down(L2_BIT));
        assert!(f.apply(&Pad { right_trigger: 1, ..Default::default() }).down(R2_BIT));
        assert!(!f.apply(&Pad { left_trigger: 140, ..Default::default() }).down(L2_BIT));
        assert!(f.apply(&Pad { left_trigger: 141, ..Default::default() }).down(L2_BIT));
        let full = f.apply(&Pad { right_trigger: 255, ..Default::default() });
        assert_eq!(Filtered::trigger01(full.rt), 1.0);
    }

    #[test]
    fn whole_path_mouse_and_keys() {
        let mut map = ActionMap::default();
        map.bindings[action::R_RIGHT] = vec![Control::Mouse(3)];
        map.bindings[action::R_TRIGGER] = vec![Control::Key("LSHIFT"), Control::Mouse(6)];
        let mut path = InputPath::new(map);
        let keys = |_: &str| false;
        let mouse = Mouse { dx: 10, buttons: [true, false, false, false, false, false, false, false], ..Default::default() };
        let out = path.poll(&devices(&keys, mouse, &[]));
        assert!((out.rx - 1.0).abs() < 1e-4);
        assert!(out.down(R2_BIT));
        assert_eq!(out.rt, 1.0);
    }

    #[test]
    fn key_names_resolve_with_aliases() {
        assert_eq!(key_name("Key_LSHIFT"), Some("LSHIFT"));
        assert_eq!(key_name("Key_W"), Some("W"));
        assert_eq!(key_name("Key_UPARROW"), Some("UP"));
        assert_eq!(key_name("Key_LAlt"), Some("LMENU"));
        assert_eq!(key_name("Key_Bogus"), None);
        assert_eq!(key_name("W"), None);
    }

    #[test]
    fn shipped_defaults_load_from_the_install() {
        // Needs the install. [data] the four keyboard/mouse contexts are identical; the pad map is Robot's.
        let Ok(a) = Archive::install(std::path::Path::new(r"C:\Games2")) else { return };
        let robot = ActionMap::install_defaults(&a, "Robot").unwrap();
        for ctx in ["Weapon", "Vehicle", "Flight"] {
            assert_eq!(ActionMap::install_defaults(&a, ctx).unwrap(), robot);
        }
        assert_eq!(robot.bindings[action::L_UP], vec![Control::Key("W"), Control::Pad(action::L_UP as u8)]);
        assert_eq!(
            robot.bindings[action::R_TRIGGER],
            vec![Control::Key("LSHIFT"), Control::Mouse(6), Control::Pad(action::R_TRIGGER as u8)]
        );
        assert_eq!(robot.bindings[action::L_TRIGGER], vec![Control::Mouse(7), Control::Pad(action::L_TRIGGER as u8)]);
        assert_eq!(robot.bindings[action::X], vec![Control::Mouse(8), Control::Pad(action::X as u8)]);
        assert_eq!(robot.bindings[action::Y], vec![Control::Key("F"), Control::Pad(action::Y as u8)]);
        assert_eq!(robot.bindings[action::B], vec![Control::Key("LCONTROL"), Control::Pad(action::B as u8)]);
        assert_eq!(robot.bindings[action::R_UP], vec![Control::Key("UP"), Control::Mouse(0), Control::Pad(action::R_UP as u8)]);
    }
}
