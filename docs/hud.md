# The HUD (2026-10-05)

Two separate things draw it in the original, and both are ported as far as this note says:

1. **The reticle**: drawn by the executable itself, textured quads, not Flash.
   `core\src\reticle.rs`, drawn by `game\src\hud.rs` (`draw_reticle`).
2. **The Flash HUD** (`UI\Flash\hud.gfx`, Scaleform): the meters, the radar, the counters
   and the messages. `core\src\formats\gfx.rs` reads and draws the movie's shapes;
   `core\src\hud.rs` plays the part of its scripts for three meters; `game\src\hud.rs`
   (`draw_meters`) shows them.

Nothing here has been compared with the original: there is no recording or picture of its
HUD. Captures: `work\shots\hud_ret_*.png`, `hud_fl_*.png`, `hud_trail_*.png`.

## The reticle [game]

Everything is in a picture of 1280 x 720, y down. One routine draws every quad:
`FUN_0055f670(texture, width, height, &pos, project, turn, &uv)`, with ECX = a pivot
offset and EAX = the colour (r, g, b, a); the decompiler loses the two registers and its
names for the stack arrays shift between calls, so read the arrays by their values: the
one holding 0, 0, 1, x is the uv rect (left, top, right, bottom), the one holding 1, 1, 1, a
the colour. The quad is `width` x `height` about `pos` (`FUN_0055f7f0`); `turn` is about
`pos + pivot`, clockwise; `project` 1 means `pos` is a point of the world, projected and
scaled by 1280 / width and 720 / height (x squeezed by 0.75 about the middle on a 4:3
picture).

Textures: `FUN_007b54c0` loads 39 by path, `c:\tf2\export\textures\crosshair\<name>.dds`.
They are texture chunks of `bnxglobal.str`; the id INSIDE the chunk (not the chunk's own
id) is `h * 33 + c` over the lower-cased path (`FUN_00550ac0`). `work\reticle_tex.py`
lists them; `work\shots\hud_reticle_sheet.png` shows them.

`FUN_007b8420` each frame, in this order:

- `FUN_007b80b0`: four arrows towards whoever hurt him (`hitarrow`, 128 x 256 at 640, 232,
  pivot 0, 128, turned by the bearing, alpha squared as it fades). NOT PORTED: nothing
  hurts him here.
- The two meters' values: the manager's weapons at `+0x34` (left) and `+0x3c` (right),
  each min(heat `+0xf0`, capacity `+0xb0`) / capacity.
- `FUN_007b7560`: each weapon's icon and frame, by the weapon's clock (word 0xf6,
  `FUN_007a73c0`: up while slot `+0x50` says yes, else 0). Frame
  (`hud_reticle_weapon_frame_l`, mirrored for the right one) 52 square, icon 48, at
  640 -/+ 90, 360. Frame alpha 5 t under 0.2 s; icon alpha (t - 0.3) * 5 under 0.5 s; from
  1.5 to 1.7 s both shrink to 0.3 and move in to 74 while the icon fades; past 1.7 s only
  the small frame is left, and only in weapon mode. Icons: a launcher's `missile`; a
  gun's `charge` with `hasChargedShot`, `shotgun` with more than one projectile a firing,
  else `rapid` (his cannon: rapid; the car's gun: shotgun).
- `FUN_007b6f90`: a meter that is full flashes its fill: alpha sqrt(1 - 2 t), t from 0 to
  0.5 s and round again.
- In weapon mode (`+0x31c` == 2):
  - the lock bracket round a launcher's target (`crosshair_missile`, and
    `crosshair_missile_focus` once locked or every other 0.1 s while locking), at the
    target's middle, its size 1.5 x the larger of its height and width in the picture,
    between 30 and 300;
  - `FUN_007b7f40` picks the base by the weapon in hand: `base_gun` (kind 0; `_chg` with a
    charge), `base_missle` (1), `base_msg` (2), `base_flame` (3), `base_sniper`; and
    the `_target` one when the aim assist has a target within the weapon's range;
  - `FUN_007b67c0`: the base, 128 square at 640, 360; four lock lines
    (`hud_reticle_lockline`, 16 wide, 16 + 48 p tall, v up to 0.25 + 0.75 p; top at
    640, 304 + 24 p, the others turned by pi, pi/2, -pi/2 at the matching places), p the
    lock from 0 to 1; the hit mark (`hud_reticle_hit`, 128 square, alpha the square of a
    value that is 1 for 0.225 s after `+0x234`'s `+0x30` and 0 at 0.45 s); the charge bar
    (64 x 32 at 640, 424: not ported, he has no charged shot).
  - A sound `UI_LOCK_ON` as the lock passes 0.25, 0.5, 0.7, 0.85, 0.95, another
    (e770ea46) as it reaches 1. NOT PORTED.
- In the car with its weapon in hand (`FUN_007b44e0`): the base 77 square at the point 50
  ahead of the car's node `Flipper`, the base (or `_target`) 128 square at the point 30
  ahead, and the hit mark there.
- `FUN_007b7170`: the meters, whenever their alpha is over 0 (1 in weapon mode): frames
  32 x 64 at 576, 360 and 704, 360; a fill 32 x 64 h at y = 392 - 32 h with v from 1 - h
  to 1; the `_locked` textures when the aim assist has a target.

Assumed or stand-in in the port:

- The hit mark's clock starts as one of his shots takes health from a target (what
  writes `+0x30` is not read).
- The icons' clock (read 2026-10-05, after the user found the icons wrong: both sides
  showed once at the start and then only empty small frames): slot `+0x50` is
  `FUN_007a7470`, called with (1, 1, 1). It says yes for the manager's weapon IN HAND
  (`FUN_007b5030`: the current index `+0x74`, or in the car's form the car's at `+0x78`)
  when the weapon's definition has `+0x5d` and `+0x9c` at 0. So only the weapon in hand
  has an icon; it comes up afresh on a switch and on leaving the car. Ported in
  `game\src\weapons.rs`. [assumed] his definitions' `+0x5d` and `+0x9c` are 0 (not
  identified).
- Out of weapon mode the meters' alpha is, by the pseudocode alone, the last weapon's
  heat (so they show while a weapon is warm). The disassembly was not checked.
- (Settled 2026-10-20) The car's points: 30 and 50 along the `Flipper` node's +y row from the
  camera target point 0071a2e0 [game 007b8420 at 007b94d2-007b9600], ported. Still no `_target`
  for the car.
- The picture is taken to be 16:9.

## `hud.gfx`

Scaleform GFx: a Flash 8 file, signature `GFX`, not compressed, stage 1280 x 720, 30
frames a second. `hud4x3.gfx` is the same for 4:3. `hud.apk` holds its four external
images (tag 1001: `Radar_RT`, `PIP`, two unnamed 64 x 64). Tags used: shapes (2, 22, 32,
83), sprites (39), text fields (37), morph shapes (46), `DoInitAction` (59: the classes,
ActionScript 2 bytecode), exports (56).

`work\gfx_dump.py <file> tree | at | frames | shape | code | images` prints all of it:
the display list of a symbol, its state at a frame, a sprite's frames with their scripts
disassembled, a shape's fills and edges, a class's code.

`Hud_SP` (id 450) places, among others: `Health_Meter` (70) and `Turbo_Meter` (27) at
639.2, 359.1; `Ability_Meter` (50) at 122.2, 600; `Radar` (106), `Timer` (134),
`Energon_Counter` (430), `Overdrive_Meter` (189), `Missile_Lock` (195),
`Hit_Indicator` (13), objective meters and counters, boss health meters, `PIP`. Each
has labels `in`, `in_complete`, `main`, `out`, `highlight_loop`.

The classes (`__Packages.*`, each a sprite id with a `DoInitAction`):

- `core.BaseMovieClip2` (466): `setEnabled`, `show` / `hide` (play `in` / `out`, or tween
  `_alpha`), `registerMovieClip(name)`; the executable finds a clip by that name
  (`hud_health_meter`, `hud_turbo_indicator`, `hud_ability_indicator`...).
- `hud2.Hud` (519) `UpdateView`: copies the data object's fields onto the clips:
  `health_meter`, `turbo_indicator_meter`, `turbo_indicator_icon`,
  `ability_indicator_meter`, `ability_indicator_icon`, `overdrive_meter`, `timer`,
  `energon_count`... What the executable writes into them is NOT READ (the names are not
  among its strings; hashed or built).
- `core.BaseMeter` (472): `meter = v` (1 above 0.9999, 0 under 0.00001) sets
  `Mask._x = -w + v * w`, w = `Mask._width` less `meterOffset`.
- `hud2.common.HealthMeter` (473): `Mask` is `Meter_Animation.Meter.Mask` (111 x 23),
  `meterOffset` 0, `low_threshold` 0.4: under it `Meter` plays `low_loop` (frames 2 to
  32, a white sheen growing over the bar, then `gotoAndPlay` back), else `gotoAndStop(1)`.
- `core.CoolDownBaseMeter` (467): `Mask._y = -b + v * b`, b = `Mask._height` *
  `orientationModifier` (-1): the fill shows v of its height from the bottom. A value of
  -1 is drawn as 0. Reaching 1 calls `onMeterActivated`, coming to 0 `onMeterReady`.
- `hud2.common.TurboIndicator` (468), `AbilityIndicator` (469): those two calls play
  `activate` and `ready` on `Turbo_Meter_Animation` / `Ability_Meter_Animation` (turbo:
  `ready` at frame 2, `activate` at 8, of 90; ability: 2 and 80). `icon = n` calls the
  `Icon` clip's `SetIcon`, which attaches a movie named by an `IconList` that nothing in
  the file fills: no icon is drawn. The ability also gets a button prompt
  (`createConsoleButton`, from `buttons_plat.gfx`): not ported.

The bars are filled with the colour 187, 90, 99 in the file itself; no script tints them.

## In the port

- `formats::gfx`: the tags above into shapes, sprites (frames of place / remove, labels,
  whether frame 1 stops) and exports; bitmap fill IDs and matrices are retained for paired
  texture lookup. A scanline renderer (four sub-rows a pixel, exact
  cover across; even-odd over the edges that have a fill on one side only) with solid,
  linear and radial fills, colour transforms, masks (clip depths: by the mask's shape,
  whatever its colour) and nested timelines. A timeline whose frame 1 does not stop
  plays by the clock (the health bar's sheen does). `Pose` is what a script would have
  set: an instance's frame, `_x`, `_y`, by its path of names.
  Bitmap fill IDs/matrices are read but general bitmap-fill rendering is not implemented.
  NOT READ: strokes, text, morph shapes, filters.
- `hud::Hud`: the three meters' values, the low loop, the two cooldown animations; each
  meter is drawn into its own small picture (`Meter::stage_rect`) when it changes.
- 2026-10-09: the game HUD draws the installed ability and turbo art over their meters.
  `IconList` order is read from `characterselect2.gfx`: ability indices 0..4 are Damage,
  Defense, Heal, Projectile, Turret. Bumblebee's `specialAbilityIcon` is 0 (`false` in the
  data export), so he uses `Ability_Damage`; turbo icon 12 is `SC_Turbo_Icon_1`. The icon
  sprites' external bitmap IDs resolve to textures in `globalassets.apk` by lowercase
  hexadecimal ID (for example, bitmap 104 is `globalassets_i68`), read at runtime.
- What the executable sends (read 2026-10-05, see "The binding"): health over the most;
  turbo 1 while it burns then the cooldown left over the cooldown; ability the cooldown
  left over the cooldown from the moment of use. [assumed] the turbo meter shows only in
  the car.
- Not ported: everything else of the movie (radar, timer, counters, messages, overdrive,
  boss health, the missile-lock text, the hit indicator), the `in` / `out` animations,
  and the ability's controller button prompt.
- The death message and the top-left readout are this build's own, not the game's.

## The binding: `UI\uiroot.lxb` (the user pointed at `<install>\UI`, 2026-10-05)

`UI\ui.lxb` names the root file and a state for each UI mode (`UI_MODE_IN_GAME`...).
`UI\uiroot.lxb` holds, besides level-script triggers (`UICmdTrigger` with
`SendGCmd_Basic` "run this script command with this argument", e.g.
`_global.hud_objective_meter_a.setEnabled`, and `SetUIData` "set game value `gameId`"),
a table of 20-byte records { game value's name hash, pointer to the movie variable's
path, type (1 a number, 0 an integer), ... } at about 0x2e00 to 0x3400, the paths at
0x6000 on (`tools\tf2_lxb.py dump` does not print this table; read it raw). So the
movie's fields are fed from named game values:

| movie field (`_global.hud.gdo.`) | game value | sent by |
| --- | --- | --- |
| `health_meter` | `CurrentPlayerHealth` (22e00534) | `FUN_00716890`: health (word 0x79) / the most (`FUN_0071fcb0`); also `FUN_0071fbd0` |
| `turbo_indicator_meter` | `HUD_STAT_TURBO_COOLDOWN` (a5176c6d) | `FUN_0071dc20` (the character's update): 1 with no turbo at all or while it is active (`FUN_00732ce0`: state 1); in state 2 the cooldown left `+0x20` over the cooldown `+0x1c`; 0 when it can be used. The timed object is `FUN_007324d0`: state 1 counts `+0x18` down, then state 2 counts `+0x20` down from `+0x1c`. |
| `ability_indicator_meter` | `HUD_STAT_ABILITY_COOLDOWN` (797f36ea) | `FUN_0072a550`: while the cooldown `+0x84` is over 0, it over `+0x88`; also `FUN_00728a60`, `FUN_00881c00` (not read) |
| `turbo_indicator_icon` | `HUD_SET_TURBO_ICON` (017651a9) | `FUN_00716890`: 12 |
| `ability_indicator_icon` | `HUD_SET_ABILITY_ICON` (8f1249d5) | `FUN_00716890`: the character attributes' `+0xec` |

A value is sent with `FUN_00724300` (a number) or `FUN_00912a10` (an integer). The other
fields in the table (timer, energon, objective meters and counters, boss health, kill
messages, `showMissileLock`, `showAbilityIndicatorReady`...) are bound the same way:
`work\find_imm.py <hash>` finds who sends each.

## What is left, in order

1. When each element is enabled (`_global.hud.setEnabled`, `hud_mode`, and the
   `HUD_onEnabled` listeners): whether the turbo meter shows on foot. The ability and
   turbo icons are ported from the game's `IconList`; only the ability's controller button
   prompt remains here.
2. Text (fields and `font.gfx`), then the missile-lock message and the kill messages.
3. The radar (`Radar_Master`, `Radar_RT` is a render target the executable draws).
4. The hit arrows, the lock-on sounds.
