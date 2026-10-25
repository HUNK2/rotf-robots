# PC mouse and keyboard input

## Static audit (2026-10-09; research only)

Read through `tools/fn.py`, with narrow executable disassembly for arithmetic
Ghidra omitted. No original-game execution, input injection, gameplay-code change,
or new runtime assumption. Earlier control changes in this chat were already
reverted. The ledger distinguishes complete reads from partial reads / outlines;
the latter do not establish the rest of a function.

## Device acquisition and polling [game]

- `004ecd70` (initialization prefix only): creates the DirectInput keyboard,
  sets its format and cooperative flags `0x16`, acquires it, and reads the Windows
  layout. Remaining key-table construction loops were not fully read.
  `004ed490` uses `GetKeyboardLayout`, `MapVirtualKeyExW`, and `ToUnicodeEx` for
  layout-specific labels.
- `004ed380`: polls / reacquires / retries; reads 256 keyboard bytes into
  `00ce36b0`, clearing state on failure. `004ed650` returns exactly 0 or 1 from
  scancode bit `0x80`. Logical indices look up scancodes in `00d0afe0`, stride `0x24`.
- `004f08c0`: creates / acquires the DirectInput mouse, format and cooperative
  flags `6`. `004f09b0` polls / reacquires / retries; reads 20 bytes at `00ce369c`:
  signed dx, dy, wheel, then eight button bytes at `00ce36a8`. Failure clears
  state. Separate `GetCursorPos` coordinates are for the UI, not gameplay deltas.
- No temporal smoothing found in device polling, directional count conversion,
  or action aggregation. This does not exclude effects from camera dynamics or
  scheduling on the final view.

## Directional mouse values and sensitivity [game]

`004f0e60` is BXMouse's value getter (slot 11), checked against its jump table in
`004f0e60..004f0f40`:

| Control index | Value |
| --- | --- |
| 0 / 1 | Up: convert(-dy) / Down: convert(dy) |
| 2 / 3 | Left: convert(-dx) / Right: convert(dx) |
| 4 / 5 | WheelUp: convert(wheel) / WheelDown: convert(-wheel) |
| 6..13 | Button1..8: high bit gives 1, otherwise 0 |

`004f0e30` gives `clamp(count / divisor, 0, 1)`; the initial divisor is 10.
Previously audited `008e8bb0` writes `trunc(float32((1.05 - SETTING_AIM_MOUSE) * 20))`;
options clamp the setting to 0..1 (`008e21a0`), saved options use `00753790`.

**Endpoint correction:** `004f0e60` checks `00bf361c` before dispatch and writes 1
if the divisor is zero. This guards the zero that float truncation can produce at
setting 1. The converter alone lacks the guard. No option implementation changed.

`004f0ad0` names Up/Down/Left/Right, WheelUp/WheelDown and numbered Mouse_ButtonN;
`004f0be0` generates display labels rather than polling input.

## Aggregation and engine stick bridge [game]

`004f0470` constructs a 24-action object. `004f0690` clears each value, queries
bindings through physical device vtable `+0x2c`, and keeps the **maximum**;
simultaneous device values are not added. Each binding is device pointer + index
(8 bytes); each action binding list has stride `0x14`. `004f0640` suppresses held
values above 0.5 whose latch is set, and clears a latch at 0.5 or below.
`004f0530` uses these latches on map changes.

`00547ea0` (thunk/export alias of `01174500`) polls keyboard, mouse and gamepads,
updates actions, then normally calls `005aa890`. UI/replay branches also exist;
this does not establish render-versus-simulation polling cadence.

`0050b6b0` bridges action values at object `+0x58` to an XInput-shaped state:
buttons threshold at `>0.5`, triggers scale to bytes by 255. Stick directional
values below 0.15 first become zero. Disassembly `0050b853..0050b9f0` restores the
omitted math: left/right raw X = Right * 32767 - Left * 32768; raw Y =
Up * 32767 - Down * 32768. Scales are doubles at `00b6a628` / `00b6a850`;
integer conversion is `00a8b5e0` (no new rounding approximation introduced).

The bridge returns 0 normally, so `005aa890` continues its normalization and
dead-zone processing. `005aa700` initializes axial dead zones to 0.25, X range
-32768..32767, Y range 32768..-32767, unit scale. `005aa890` zeros values inside
the dead zone and rescales the remainder by `1 / (1 - dead_zone)`. Engine Y is
therefore positive down after normalization; intermediate XInput-shaped Y is
positive up. This settles device/action/intermediate signs, not all separate
host-camera or aim-assist picture-vector questions.

There is no mouse-only bypass in the bridge or normal path read. Other
configuration / exceptional branches of `005aa890`, including `005aae80`, were
not re-audited: 0.25 is the initialized default, not a claim about every possible
user/device configuration.

**Observed rebuild difference:** `game/src/player.rs` applies `dead_zone()` to
gamepads, then adds mouse counts/divisor directly. It adds simultaneous device
values instead of the original per-action maximum. At divisor 10, 3 mouse counts
give action 0.3; the original default dead zone gives idealized magnitude about
0.067 before integer quantization. This changes low-speed response; it does not
establish the cause of the user's jitter. Sampling and update timing remain open.

## Shipped bindings [data]

The exact defaults are in the user's `<install>/Beenox/bxdata.dat`.
`004f2de0` establishes its directory: LE u32 entry count, then u32 UTF-16 character
count, UTF-16LE path, u32 size, u32 absolute offset per entry. This install has
444 entries. `beenox\actions.bin` is at 33112, size 1960; `default_bindings.bin`
at 35072, size 98552. The former names 24 actions; the latter has 27 input maps.

`004f5010` / `004f4f70` establish BXML's BOM-prefixed null-terminated UTF-16LE
strings: token 1 opens an element with attributes, 2 supplies text, 3 closes an
element, 4 is ignored, the default branch ends parsing. `tools/tf2_bxdata.py`
validates both resources as complete trees without extracting assets. All
directory payload bounds are checked. Four Keyboard and four Mouse maps cover
Robot, Weapon, Vehicle, Flight; the four contexts have identical shipped bindings
per device, each with all 24 records (including empty bindings).

The Controller table in `global_tuning.txt` and hashes in `names.tsv` supply
the uses below. Shared triggers retain context-dependent uses; default bindings
alone do not settle every downstream state transition or input gate. Saved
custombindings may override defaults; saved user settings were not loaded here.

| Physical default | Virtual action | Installed Controller use |
| --- | --- | --- |
| W / S / A / D | LThumbUp / Down / Left / Right | Movement stick |
| Mouse or arrows | RThumbUp / Down / Left / Right | Camera stick |
| Space | ButtonA / DPAD_R_DOWN | Jump; vehicle turbo |
| Left Ctrl | ButtonB / DPAD_R_RIGHT | Action, climb, drop, pickup, throw |
| Middle mouse (Button3) | ButtonX / DPAD_R_LEFT | Fast attack, dodge, vehicle attack |
| F | ButtonY / DPAD_R_UP | Special attack |
| Left Shift or left mouse (Button1) | RTrigger / R2 | Vehicle mode/acceleration; fire |
| Right mouse (Button2) | LTrigger / L2 | Weapon mode; drift |
| R | ButtonLShoulder / L1 | Vehicle brake; ActivateOverdrive |
| T | ButtonRShoulder / R1 | WeaponSwitchUp |
| Q | ButtonLThumb / L3 | StrafeMode |
| E | ButtonRThumb / R3 | CameraReset |
| I or wheel up | DpadUp / DPAD_L_UP | CameraSwitchBack; debug closer |
| K or wheel down | DpadDown / DPAD_L_DOWN | CameraSwitch; debug farther |
| J / L | DpadLeft / DpadRight | Virtual d-pad left/right; further use not asserted |
| Tab / Escape | ButtonBack / ButtonStart | Back/Start virtual buttons |

Keyboard ButtonX and LTrigger are empty; other mouse controls are empty apart
from those listed. Second sources above are separate devices combined by max.
The rebuild currently differs: F is melee, X special, E climb, Ctrl brake,
Q weapon switch, Home recenter. This audit changed no binding.

```
python tools/tf2_bxdata.py actions
python tools/tf2_bxdata.py input --device Keyboard --context Robot
python tools/tf2_bxdata.py input --device Mouse --context Robot
```

## Map/persistence read boundaries

- `004eed30` reads physical-device maps, resolves named controls via the device
  name-to-index virtual, skips unknowns and accepts numeric fallback. Some schema
  details have lost register arguments in Ghidra. `004ed680` resolves keyboard
  name hash -> scancode -> control-table index.
- `007035b0` (selected ranges) builds/imports defaults and custom maps; its
  remaining 1000-line body was not fully read. `004ef100` opens an archive resource
  and invokes BXML. `00702860` creates Keyboard_menu/Edit/Cinematic maps; complete
  physical bindings of those maps remain outside this defaults audit.
- `00702710` resolves context indices 0..4, `00702750` resolves names. `00705f80`,
  `00705c00`, `00704c00` have unread branches, recorded in the ledger.
- `00707240` consumes device events, treats mouse as keyboard in a selection
  branch, and rebuilds maps on connected-device changes.
- `00706450` (selected ranges) writes custombindings/inputmaps through `004ee020`
  (serializer outline only). This is input persistence, not an animation
  controller as an old candidate list guessed. Partial lookup/label/allocation
  helpers are also in the ledger; their unread parts support no further claims.

## Port (2026-10-16): `core/src/input.rs`, wired in `game/src/player.rs` `read_input`

Further reads this session (disassembly checked; ledger updated):

- **XInput pad getter** `004ec6c0` (cases `004ec6cb..004ecafb`; the census filed the
  bodies as platform code). State `XINPUT_GAMEPAD` at device +0x10. Controls 0..13
  test wButtons bits (same layout as the bridge); 14/15 trigger byte / 255.0;
  16..23 each stick direction its half: positive / 32767, negative * -1/32768. No
  dead zone in the device: the pad goes through the same action / bridge / filter
  path as keyboard and mouse.
- **Filter triggers** (`005aa700` / `005aa890`): range 0..255 normalised to -1..1
  (rest = -1), dead zone 0.1, scale 1. With flags bit 0x20 (default 0x17f) the filter
  ORs bit **0x400 when LT > 0** (raw 141 and up) and **0x800 when RT > -0.999**
  (raw 1 and up). The filter writes raw XInput buttons to +0x90.
- **Commands read digital bits.** `controller_tuning_load` 01185160 maps every
  `Controller` command through `011850b0` to a mask from table `00b8d9d8` (32 pairs
  CRC-32 name, mask): DPAD_L_UP/DOWN/LEFT/RIGHT 1/2/4/8, START 0x10, SELECT 0x20,
  L3 0x40, R3 0x80, L1 0x100, R1 0x200, **L2 0x400, R2 0x800**, DPAD_R_DOWN 0x1000,
  _RIGHT 0x2000, _LEFT 0x4000, _UP 0x8000, then stick directions 0x10000.. (unread).
  `pad_button_down` 0084f900 tests those bits. So weapon mode / drift (L2) needs
  more than half the trigger; vehicle mode / fire (R2) any touch. The car reads the
  analog trigger as `(t + 1) / 2` (`docs/driving.md`).
- **Bridge rounding**: triggers `trunc(v * 255)` (control word 0xC00); stick
  directions under 0.15 become 0 (0.15 itself kept); axes truncated (`00a8b5e0`
  is MSVC's truncating float-to-integer).
- **Mouse**: `004f0e30` rechecked: integer counts, negative 0, above divisor 1, else
  `float(count) / divisor`. No other code reads the mouse globals (`00ce369c..`):
  the action path is the only mouse path. Sensitivity setter float32: setting 0.5
  gives the executable's initial divisor 10, 0.55 gives 9, 1 gives 0 (guarded to 1).
- **Poll cadence**: `main_update_scheduler` 007799f0 calls `01174500` (poll, actions,
  filter) ONCE per rendered frame, before the scheduler step `005a8740(dt)`; so mouse
  counts are per rendered frame, as the rebuild's `AccumulatedMouseMotion`.
- **Latch on form change**: `0050b660` (thunk of 0116ef30; census says net) sets the
  active map (EAX index, arg latch-all) and calls `004f0530`. The form change
  `0071ab70` always activates map 0, with latch-all only when the context was 4
  (the menus'); otherwise only actions flagged in the map's own array (+0x3c) latch.
  Where those flags are set (`007035b0` / `00704c00` map construction) is unread:
  the port flags none [assumed].

What the port does: the install's `default_bindings.bin` maps (keyboard and mouse
for the context, XInput's Robot map), per-action maximum, latch, bridge, filter; the
game's `Controls` take the filtered pad: sticks (engine Y negated to the rebuild's
up), `vehicle = (rt + 1) / 2`, aim = L2 bit, fire = R2 bit, brake = L1, switch = R1,
jump = A, melee = X, climb = B, special = Y, camera reset = R3. Bevy's own pad dead
zones are switched off so the pad reaches the game's filter raw. `Controls.pad` holds
the filtered pad for other systems.

Keyboard result (the original's defaults, changed from the rebuild's): F is now the
special (was melee), middle mouse the melee (the keyboard has none), Left Ctrl climb /
action (was brake), R brake (was reset; reset moved to F5), T next weapon (was Q),
E camera reset (was climb), Q strafe mode (no rebuild consumer yet), Left Alt no
longer weapon mode (right mouse only), Home no longer recentres. The mouse now goes
through the 0.15 cut and the 0.25 dead zone: at divisor 10, 1-2 counts in a frame do
nothing and 3 counts give 0.067.

## Modern layout (2026-10-21, the user's request) [port]

The default for keyboard and mouse since 2026-10-21; `--classic-controls` keeps everything above.
`modern_map` builds a map over the same 24 actions (the shipped pad bindings kept), so the bridge,
filter and commands are untouched; `modern_context` adds two context bindings after the query (left
mouse is R2 while L2 is over 0.5, else X; Shift + S also L1). The table is in `modern_map`'s doc and
`game\README.md`. The mouse moves no stick: `mouse_turn` (counts times degrees per count) turns the
follow / driving camera through `Camera::nudge` and the weapon aim through `Aim::turn`. Open items:
the unpublished notes "Input: a modern keyboard and mouse layout".

## Remaining questions

- Per-map latch flags (`007035b0` / `00704c00`), and whether any action latches on a
  robot / vehicle change in play.
- Nondefault axis configuration, the +0xbc branch `005aae80`, full custombindings
  save/restore, menu and text-entry physical bindings, the stick-direction bits
  0x10000.. in `00b8d9d8`.
- Whether the character code limits the stick to length 1 (the rebuild clamps).
- Systems that still read keys directly (`camera.rs` C / Esc / D-pad, `pause.rs`,
  `hud.rs` F1, `targets.rs` B) instead of `Controls.pad`.
- Cause of rebuild mouse-camera jitter. No smoothing value or fix was guessed; the
  port now has the original's dead zone on the mouse, so slow mouse motion is dead.
