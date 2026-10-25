# FlyMode: the per-step update and the enter (survey ticket, 2026-10-12)

Scope: FlyMode update `FUN_00864320` (246 lines) and enter `FUN_00864d60` (called from the
wrapper `FUN_00863950`, `[game]`). Base registers: `mode+0x..` is the FlyMode object,
`owner = *(mode+4)`, `attr = *(mode+0x18)` (characterAttributes block, the modeFly data),
`char+..` is the owner. Radians unless said. Constants checked in disassembly are tagged.
Callees are NOT read in depth; their roles are one-line guesses.

Status: partial. The order and the body write are read; the steering in the AI branch, the
boost ramp, the lift assist internals and `FUN_00869df0` are not finished (see "Not read").

## One step, in order

Source is `FUN_00864320` as decompiled; `[game]` tags give the address.

1. `owner` copies: `mode+0x28 = mode+0x1c`, `mode+0x2c = mode+0x20`, `mode+0x30 = mode+0x24`
   (previous step's pitch, roll, yaw). `[game] 00864320 start`
2. `FUN_0086b230(mode)`: the flight-form update (ledger: writes its own fields, not the
   STEP record). Always called. `[game]` (ledger)
3. Early exit: if `attr+0xb8` is set and `FUN_00866a70(mode, dt)` returns true, the step ends.
   `FUN_00866a70` writes body velocity and calls many damage/camera helpers; `[assumed]` it is
   the crash or landing test. `[game] 00864320`
4. Input reads through the owner's input source, `[*(owner+0x20)]` vtable:
   - button 0x1a (`vt+0xc`) gated by `mode+0x4a0` -> `bVar8` (turbo button)
   - button 0x1b (`vt+0xc`) gated by `mode+0x4a0` -> `bVar3` (second ability button)
   - button 0x1f (`vt+0x8`) -> decides steer branch below
   `[game]` (decompile, 00864320 lines 14-30)
5. Ability branch (only one runs):
   - if `bVar8` and `(mode+0x34 vt+0xc)()`: `turbo_change_state(mode+0x34, 1)` (00732e60);
     rumble preset 0xe (`play_preset_rumble`, only for the player); UI integer hash
     0xa1db8e6b = 1 if `character_input_is_player`; `FUN_00532c70` releases a ref. `[game]`
   - else if `bVar3` and `timed_ability_ready(mode+0x38)` (00732c20):
     `mode+0x38` state set to 1 (`+0x10 = 1`, `+0x18 = +0x14`), `ability_effects(mode+0x38, 1)`
     (007325e0 "on": Trigger FX_brake, see ledger), `FUN_00732b00`, and `mode+0x4ac =
     |body velocity|` (sqrt of the squared velocity from `body_linear_velocity` 00556c00). `[game]`
6. `cStack_10c = timed_ability_active(mode+0x34)` (turbo), `uStack_120 = timed_ability_active(mode+0x38)`.
   `[game] 00864320`
7. `body_world_matrix(...)` copies the body's 12-float transform; three components of it are
   kept as the vector `fStack_50/4c/48` (used in steps 9 and 10). Which row this is: `[assumed]`
   the forward row (docs/damage.md says the matrix forward row is the wall dot).
8. Thrust magnitude `fVar10`:
   - `attr+0xc` (turboThrust) if turbo active, else `attr+0x4` (thrust). `[game] 00864320`
   - Values for Bumblebee's modeFly block: thrust 20, turboThrust 40.
     Equal to these numbers `[data]`; the loader that maps names to offsets is not found, so
     the match is `[assumed]`.
9. If `attr+0x84 == 0`: `fVar10 *= dt`; the thrust vector `(fStack_50..48) * fVar10` is added to
   the body's linear velocity (`body+0xa0/a4/a8`), but only if `body+0x38 == 0` and
   `body+0x34 == 0.0` (the body is not held). `[game] 00864320`
10. Gravity-style push: the `(fStack_1xx)` velocity of the holder is composed with the same
    vector. `[game]`. (Not fully traced: uses `FUN_00869df0` below.)
11. Two avatar matrix reads: `vt+0x30(auStack_8c)` and `vt+0x28(auStack_9c)` on the input
    source give two 3-vectors fed to `FUN_0086d5c0` (a sign-and-threshold helper; the decompile
    shows no output, `[unread]`). Feeds `fStack_108`, `fStack_104` = `|x|*x` for the two
    components of the (throttle-like) input. `[assumed]`
12. Steering branch, from `attr+0xb0`/`attr+0x84` and the button 0x1f and `mode+0x55`:
    - `mode+0x55 == 0`, `attr+0x84 == 0`, button 0x1f up: `mode+0x4c = 0`; `FUN_0086aa60(0)`
      (Trigger / FX). Then if `controller_is_ai`: `FUN_008693f0(mode, &vec, dt)`; else
      `FUN_00866ef0(mode, &stack128, &vec, dt)` (player stick, `[assumed]` the AI side of
      the same logic).
    - `mode+0x55 == 0`, button 0x1f held: `mode+0x4c = 1`, `mode+0x50 = 0` (first press),
      `FUN_0086aa60(1)`; then `FUN_008676c0(mode, ..., dt)` (the player steering, read).
    - `mode+0x55 == 0`, `attr+0x84 != 0`: `FUN_00867f20(...)`.
    - `mode+0x55 != 0`: `FUN_00868b00(...)` when `attr+0x84 == 0 && attr+0xb0 == 0`, else
      `FUN_00868fb0(...)`. `[game]` (`868b00` read: contact response, see below)
    - `FUN_0086e020(mode, dt)` runs after the first branch. (Ledger: damage / camera helper; not read.)
13. `euler_matrix()` (00570470) returns a 3x3 (9 floats) built from the angles; copied to a
    stack 3x3 `uStack_d4..b4`. `[game] 00864320` (see "Writing the body")
14. Position: `owner+0x2c` (the character transform) gives translation from `+0x34,+0x38,+0x3c`
    of the owner's `+0x2c` block. `[game]`
15. `holder_set_affine(...)` (006dbd30): sets the body's current and inverse transform, syncs the
    physics body. `[game] 00864320`
16. `FUN_00869df0(dt, ..., fStack_130)`: velocity integration / flight physics (ledger: flight
    group, calls body velocity and `sqrt`). `[unread]`
17. `mode+0x4b0 = uVar12` (from the owner's `+0x2c` block `iVar2+0x3c`). `[game]`
18. Pitch and roll event parameters (only if `mode+0x3c != 0`, i.e. a sound event is playing):
    - `pitch = mode+0x1c / (attr+0x58 * 0.0174533)` if `mode+0x1c > 0`; with
      `attr+0x5c` when `< 0`; `0` at exactly 0. Sent to `event_parameter_by_hash(0x279fbed9 = 'pitch')`.
    - `roll = mode+0x20 / (attr+0x64 * 0.0174533)`. Sent to `event_parameter_by_hash(0x2eb532ce = 'roll')`.
    The float value is written with `fmod_parameter_set_wrapper`. `[game] 00864b40-00864bc6`
    (asm: `movss xmm0,[eax+0x64]; mulss xmm0,[0x00b6a40c]` where `[00b6a40c] = 0.0174533`).
19. Then the sound: `character_object_lookup`, and the stopping of a hanging event.
20. Boost ramp: `mode+0x4a4 += dt` (elapsed; the AdvTransform timer). If `mode+0x4a4 >= 0.25`
    (`[0x00d40b74]`, which docs/melee.md gives as advTransformDelay) and `ability+0x150 bit0` is
    clear: set that bit, if `FUN_00725a40` (a has-player check) sets the event `AdvTransformReady`
    (hash 0x54a474d6) with `Trigger` (0xd5d636c1). `[game] 00864320`
21. `FUN_0086cbf0(mode, dt)`: builds the render/affine chain (Scene/BasePose; ledger: camera/IFL).
    `[game]` (ledger). Not read.
22. `FUN_0086d630(dt)`: if `attr+0xb0 != 0`, a smoothed value at `mode+0x528` moves toward the
    input by `attr+0xb4 * dt`. `[game]` (`FUN_0086d630`, decompile lines 1-8). Partly read.
23. Boost ramp output: `mode+0x4b8 = min(1, mode+0x4b8 + 2*dt)`. `[game]`
    (used as a multiplier on the steering input in `008676c0` when the input is positive).
24. `mode+0x550 += dt` (accumulator; when > 0.5 `FUN_0086ec70` adds a surface-damage sphere). `[game]`
25. Return.

### The inner steering (player), `FUN_008676c0`, read

Called with `(mode, &stack128, &vec, dt)`. Order of operations `[game]` (lines numbered by decompile):

- Build a frame from the owner body's velocity (`body+0xa0..a8`) and the `vec` argument, cross
  products and a unit normalize (`1e-8` threshold); this gives a forward-like axis and a dot
  product with velocity scaled by `-0.95`; the velocity is then changed by `axis * (dot*-0.95)`
  (lines 17-38). `[game]`, the "ground bounce" term; `[assumed]` it is a constraint push.
- Roll target (`mode+0x50`): if zero, set to `+attr+0x64 * rad` when the stick x is > 0, `-` when < 0 (lines 52-62).
- Roll smoothing (`mode+0x20`): lerp toward `mode+0x50` with rate `attr+0x74 * dt` (lines 64-70).
- Turbo gate: `mode+0x4b8` (ramp) multiplies the thrust when positive (lines 87-89).
- Stall / lift assist: `mode+0x78` (a speed-like value, minus `2*(|sin(mode+0x1c)|*..)` terms,
  lines 90-104) is compared with `1.85` (or `2.85` if `mode+0x4bc != 0`). If below:
  `lift = (1 - v/limit) * 50`; `body+0xa8 += lift*3*dt` (line 119). `[game]` numbers; `[assumed]` role.
- Pitch (`mode+0x1c`): target `-(attr+0x58) * rad * stick`; smoothed with rate `attr+0x50 * dt`;
  wrapped to (-pi, pi] by `x - round(x)` (line 129-131, `0.15915494` = 1/2pi, `6.2831855` = 2pi);
  clamped to `+-attr+0x58*rad` (lines 134-140). `[game]`

## Mode object fields (read or written by the update / enter)

| field | type seen | what it seems to be |
|---|---|---|
| +0x4 | ptr | owner (character) |
| +0x18 | ptr | attr block (characterAttributes, the modeFly data) |
| +0x1c | float | pitch, rad (asin of a matrix element; 00570250) |
| +0x20 | float | roll, rad (smoothed toward +-maxRoll) |
| +0x24 | float | yaw, rad (atan2) |
| +0x28,+0x2c,+0x30 | float x3 | previous step's +0x1c,+0x20,+0x24 |
| +0x34 | ptr | turbo timed ability (turbo_change_state, timed_ability_active) |
| +0x38 | ptr | second timed ability (`timed_ability_ready`, state +0x10, +0x14, +0x18) |
| +0x3c | ptr | body holder for the sound / object (`mode+0x3c != 0` gates the events) |
| +0x44 | float | smoothed angle value (lerp in 868b00) |
| +0x48 | float | zeroed in enter; not read |
| +0x4a0 | byte | gate for the ability buttons 0x1a/0x1b |
| +0x4a4 | float | elapsed time in the mode; AdvTransform timer, compared with 0.25 |
| +0x4ac | float | speed at ability start (sqrt of squared velocity) |
| +0x4b0 | u32 | copied from owner block; not read further |
| +0x4b8 | float | boost ramp, 0..1, +2*dt per step |
| +0x4bc | float | flag: 2.85 stall threshold when nonzero |
| +0x4c | byte | "fly button" state (set 0/1 with FUN_0086aa60) |
| +0x4c0, +0x4cc | byte, float | written in 86c970 (not read) |
| +0x50 | float | roll target, rad (set once on first press, from attr+0x64) |
| +0x55 | byte | branch selector (landing / contact) |
| +0x56 | byte | set when the surface dot > 0.95 (868b00) |
| +0x58 | float | ground-normal style vector component (868b00, not read deeper) |
| +0x5c,+0x60 | ptr | render object pointers (enter: uses +0x5c/+0x60 of owner) |
| +0x68,+0x6c,+0x70 | float x3 | a 3-vector dotted with the stick in 868b00 |
| +0x78 | float | speed-like value for the stall test (008676c0) |
| +0x3fc (owner) | u32 | animation state name (set in enter) |
| +0x400, +0x404 | ptr | effect / sound objects (enter stops them via 0x4c8 family) |
| +0x4e8 | byte | set to 1 in enter |
| +0x49c | float | set to 1.0 in enter (use not read) |
| +0x4a4 | float | (see above) |
| +0x518 | ptr | if nonzero, `thunk_FUN_011869b0` in enter (not read) |
| +0x528 | float | smoothed value (0086d630), zeroed in enter |
| +0x544, +0x548, +0x54c | float | zeroed in enter |
| +0x550 | float | time accumulator; > 0.5 adds a surface-damage sphere |
| +0x430 | byte | flag read in 86c970 (not read) |

Attribute block (`attr = mode+0x18`) fields the update reads: `+0x4` thrust, `+0xc` turboThrust
`[assumed]` (values match Bumblebee's modeFly thrust 20 and turboThrust 40), `+0x50` pitch
smoothing rate, `+0x58` maxPitch, `+0x5c` minPitch `[assumed]`, `+0x60` roll-related target
(868b00), `+0x64` maxRoll `[assumed]` (value 25 for Bumblebee's modeFly maxRoll), `+0x74`
roll smoothing rate, `+0x84` byte (0 = thrust applied, and selects the form-state name), `+0xb0`
byte (selects the 868b00 / 868fb0 branch and the state name), `+0xb4` float, `+0xb8` byte
(early exit), `+0x100` float (threshold -0.95 in 868fb0). Names of the attribute fields come
from the loader, which was not found (see flying-forms.md).

## Constants (checked)

| value | where | source |
|---|---|---|
| 0.0174533 (deg to rad) | 00b6a40c | asm 00864b8e-00864b91 |
| 0.25 s (advTransformDelay) | 00d40b74 | docs/melee.md:544 (the asm was not read) |
| 1.0, 2.0 (boost ramp), 1.0 clamp | in 00864320 | decompile |
| 0.15915494 = 1/(2 pi), 6.2831855 = 2 pi | in 008676c0 | decompile |
| -100000 velocity clamp lower | 00cae3ec | docs/character-ground-test.md:24 |
| 100000 clamp upper | in 00864d60 | decompile |
| -0.9 (surface reflect) | in 00868b00 | decompile `* -0.9` |
| -0.95 (steering dot factor, 008676c0; threshold 868fb0) | in 008676c0, 00868fb0 | decompile |
| 0.9999 gimbal limit | in 00570250 | decompile |
| 0.95 (surface dot) | in 00868b00 | decompile |
| 1.85 / 2.85 (stall threshold) | in 008676c0 | decompile |
| 50 (lift) times 3 (time factor) | in 008676c0 | decompile |
| 0.5 (ssd sphere threshold) | in 0086ec70 | decompile |

Floats not checked in asm are `[assumed]`.

## Where the inputs come in

- Stick and buttons: the owner's input source at `owner+0x20`. Its vtable: `+0x8` and `+0xc`
  return a button held (`0x1a`, `0x1b`, `0x1f`). `+0x24` is the 2D stick direction
  (docs/bumblebee-mechanics.md). The update also reads `+0x28` and `+0x30` (3-vectors; the
  notes give slot `+0x28` as the camera stick in weapon mode, `[assumed]` the same here).
  The `vec` argument to the steering is `fStack_110`, a body-matrix-derived vector, not a stick.
- Pitch and roll reach the steering through `mode+0x1c` / `mode+0x20`, which come from
  `FUN_00570250` and the smoothing above; the stick is the `stick` term in step 12 and
  `008676c0`. `[game]` / `[assumed]` for the exact stick axis (the decompile shows `param_2`
  used as a float; `[unread]`).
- Pitch and roll as sound-event parameters: `pitch` = mode+0x1c / (maxPitch rad), `roll` =
  mode+0x20 / (maxRoll rad) (step 18). So the sound gets the fraction of the stick-driven
  angle `[game]`.
- Turbo: `timed_ability_active(mode+0x34)` (00732ce0; state +0x10 == 1) picks turboThrust;
  turbo starts by `turbo_change_state(mode+0x34, 1)` (00732e60) from button 0x1a.
- `AdvTransformReady` (hash 0x54a474d6) and `Trigger` (0xd5d636c1): fired once, when the
  step's timer `mode+0x4a4` reaches 0.25 s, through `FUN_00724940` + `FUN_007254a0`
  (a value-sending call, not read). `[game]` for the hashes; the meaning of the event is
  `[assumed]` (the advanced transform is ready for a later step).
- `Trigger` is also sent by `FUN_0086aa60` (flag 1: start of fly button; flag 0: stop).

## Writing the body

Final write (step 13-15) `[game] 00864320`:

- `euler_matrix()` (00570470) gives a 3x3 (9 floats). ledger: "positive column-major Rz(z)*Rx(x)*Ry(y)"
  (docs/surface-damage.md). It takes its angles from registers and memory that the decompile
  does not show, so the exact angles are `[unread]`; by the ledger they are the mode's
  pitch, roll and yaw (+0x1c, +0x20, +0x24) `[assumed]`.
- Translation: owner `+0x2c` block `+0x34..+0x3c` (the character transform position) `[game]`.
- The 12-float affine is handed to `holder_set_affine(...)` (006dbd30) which sets the body's
  current and inverse transform and syncs the physics body `[game]` (ledger).
- Net formula `[assumed]` from the reads above:
  `R = Rz(yaw) * Rx(pitch) * Ry(roll)` (see ledger order; careful: the order is what the ledger
  says, `Rz*Rx*Ry`), `t` = owner position, body velocity `v += forward * thrust * dt` when
  boosting or thrusting, then `FUN_00869df0` integrates.

## Enter, `FUN_00864d60` (called via `FUN_00863950`) [game]

1. Name hash of the animation state at `owner+0x3fc`: default `0x5f0a36b3`; `0x2bf2690a` if
   `attr+0x84 != 0`; `0x622336bc` if `attr+0xb0 != 0` (both names not in the tables).
2. Calls `FUN_007106f0(C_VEHICLE_STUNT 0x53d0af4b)` and resets a state flag (`+8`, `*+0 = 1`).
3. Zeroes a set of local vectors and then loops over the owner's contacts list
   (`owner+0x38 ... +0x1e4`), applying `body_contact_adjust` (006dd900) with contact normal times
   `owner+0x174` (float). This is a contact pre-pass, `[game]`.
4. Sets the body's velocity clamp: each component clamped to `[-100000, 100000]` (lower bound
   `0xcae3ec = -100000`), the velocity is not set to a value here `[game]`.
5. Body flags: `body+0x1d4 |= 0x200000`; `|= 0x20`; body `+0x44, +0x48, +0x4c = 0`;
   bit 0x40 set or cleared depending on bit 0x20 `[game]`.
6. `mode+0x4a4 = 0` (elapsed), `mode+0x518` thunk if set.
7. `character_object_lookup` for the avatar's `+0x150` flag: clears bit 0 (the AdvTransform
   latch) `[game]`.
8. If the fly-state `+0x14 == 0` form: sound event start `FUN_00556540` / `sound_event_wrapper_start`
   with the object at `ability+0x294`, `FUN_009518f0` (sound event stored at `mode+0x3c`) `[game]`.
9. `FX_exhaust_smoke_skid` (0x92a881cc) and `Trigger` are sent (`FUN_00724940` + `FUN_007254a0`) `[game]`.
10. If the flag `+0x390` on the animation controller is set, `animation_set_start(V2RPartialSet
    0xa1c4e1d0, Trans_R2V_Partial 0x6428f474, 1, 0)`; and if `owner+0x26c` is set,
    `motion_control_set` with (0, 0, 1) `[game]`.
11. Messages: if not player and not `attr+0x84`: `character_message_handler(0x69d4e2f9, 0xcc0d1c75)` `[game]`.
12. Angles: `mode+0x1c..0x24 = FUN_00570250(...)` (current pitch, roll, yaw from the owner's
    matrix) `[game]`; then `mode+0x20 = 0` (roll reset), `mode+0x44 = 0`, `mode+0x48 = 0`,
    `mode+0x528 = 0`, `mode+0x4c1 = 0`, `mode+0x4c2 = 0`, `mode+0x4e8 = 1`,
    `mode+0x544, +0x548, +0x54c = 0`, `mode+0x49c = 1.0`.
13. Initial speed, altitude: not set in enter. Body velocity is clamped, not reset. `[game]`
    (so the initial speed is whatever the previous mode left in the body).

## Not read

- Callees with no read body: `FUN_00866a70` (early exit), `FUN_0086aa60` (flag and Trigger, read
  as outline only), `FUN_008693f0` and `FUN_00866ef0` (AI and player steer for the non-held
  button), `FUN_00867f20`, `FUN_00868fb0`, `FUN_0086e020`, `FUN_0086cbf0` (the render
  chain), `FUN_0086d5c0` (helper, decompile has no outputs), `FUN_00869df0` (the flight
  physics integrator; it changes `body+0xa0..a8`), `FUN_0086c970`, `FUN_0086ec70`.
- `FUN_0086b230` (ledger only, flight-form update).
- `FUN_00725a40` (has-player test, read as outline).
- `FUN_00570470` `euler_matrix` angle inputs.
- `FUN_00732b00`, `FUN_00532c70`, `FUN_007254a0`, `FUN_00724940` (refcount and event senders).
- The loader that gives `attr` field names (modeFly fields such as `+0x4`, `+0x58`, `+0x64`).
- The stick axis and sign convention of `param_2` in `008676c0` (decompile shows it as a float).
- The exact stick-to-yaw path (yaw is never written by the update except through the body matrix).
- Constants 0xcae444 * 0.49 at step 139 of 008676c0 (not identified).
