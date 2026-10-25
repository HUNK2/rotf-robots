# Flight forces: the seven helpers of the FlyMode update (ticket, 2026-10-12)

Scope: the seven functions the FlyMode update `00864320` calls in the "flight" block:
008693f0, 00868b00, 00868fb0, 00869df0, 0086aa60 (called twice), 0086c970, 0086cbf0.
Every tag is `[game]` (read in code, address given), `[data]` (from the unpublished notes),
or `[assumed]`. Names are proposals. Ledger entries were written with `fn.py note`.

## What the update does, from the call sites `[game]` 00864320

- Line 4-6: copies mode+0x1c..0x24 to mode+0x28..0x30 (previous angles).
- After `FUN_0086b230` and the `FUN_00866a70` early exit, mode+0x1c, +0x20, +0x24 are set
  from `matrix_to_euler_asin` (00570250) of the character matrix.
- `if mode+0x55 == 0` (free flight):
  - if the ability button 0x1f is NOT held: `0086aa60(0)`; then if controller_is_ai:
    `008693f0(mode, &steer, dt)` and skip to the end; else `00866ef0(mode, &vec, &steer, dt)`.
  - if 0x1f IS held: once, `mode+0x4c = 1`, `mode+0x50 = 0`, `0086aa60(1)`; then
    `flight_steer_player` 008676c0 (not one of the seven).
  - then `0086e020(mode, dt)` (not read).
  - (the branch `char+0x84 != 0` calls `00867f20` instead; not read.)
- `else` (mode+0x55 set, contact): `char+0x84 == 0 && char+0xb0 == 0` gives `00868b00`,
  otherwise `00868fb0`.
- At the end: euler from the holder matrix, `holder_set_affine`, then `00869df0(dt, ...)`,
  and near the end `0086c970()` and `0086cbf0(mode, dt)`.
- So the seven are: a steer/contact layer (008693f0, 00868b00, 00868fb0), one speed layer
  (00869df0), one event sender (0086aa60), one probe (0086c970) and one visual (0086cbf0).

## Correction to the brief's expectations (read this first)

- No thrust is applied in any of the seven. 00869df0 only drags and clamps speed. The
  "thrust into body velocity" in the ledger's note for 00864320 is inside 00864320 itself
  or in 008676c0 (not read). `[game]`
- No pitch lag, no maxPitch, no maxRoll clamp is in the seven. The ledger attributes the
  roll target from maxRoll and the pitch clamp to `008676c0` (flight_steer_player) and
  `00866ef0`. `[game]` (ledger)
- No altitude limit and no ground avoidance in the seven. Obstacle look-ahead is 0086c970.
- Turbo: 00869df0 has a speed-latch branch (mode+0x4ac) driven by two of its arguments; the
  turbo gates themselves are in 00864320 (not read here).
- Yaw (turn) rate: found, in 008693f0 and 00868b00 (see below). The data name is not
  settled (see Fields).

---

## 008693f0  flight_ai_hold_steer (proposed)

Called from 00864320 only when `controller_is_ai` is true and button 0x1f is not held.

Arguments (checked: `008693f0` at entry, `mov ebx,[esp+0xe0]` after `sub esp,0xd8` and one push):
- stack arg 1 = mode (FlyMode object).
- stack arg 2 = `float *yawStick` (pointer to one float; the decompiler's `param_2`).
- stack arg 3 = `dt` (float).
- EAX = `float *n`: a 3-float contact normal. The decompiler lost this (`in_EAX`); the
  disassembly shows `mov edi, eax` and then `[edi]`, `[edi+4]`, `[edi+8]` used as the normal.

Steps:
1. Gate: if mode+0xc, +0x10 and +0x14 are all exactly 0.0 (`ucomiss` against zero at
   0x869409, 0x869420, 0x86942f; any NaN or non-zero skips the whole body), continue.
   Otherwise return at once. `[game]`
2. Lever arm: the body point velocity is the linear velocity plus omega x r with r = 0 (the
   code multiplies the body angular velocity by a zeroed register). So `vp = v`. `[game]`
   (`xorps xmm4,xmm4` at 0x8693f0; `mulss` by xmm4 at 0x869488..0x86948c.)
3. Tangential damping (if body +0x38 byte == 0 and body +0x34 float == 0.0):
   - `p = v x n`, `q = n x p` (the code computes `e x (e x v)` with e = n), normalise q
     (if |q| > 1e-8) to `d`.
   - `k = -0.95 * dot(d, v)`   (constant -0.95 at 0x869636).
   - `v = v + k * d`. Since d points along v_perp, this keeps 5% of the normal-perpendicular
     velocity (v_par + 0.05 v_perp). `[game]` (ops at 0x8694..0x86A0; written to body +0xa0,
     +0xa4, +0xa8 by the direct stores 0x869...; the reflection formula is checked by the
     signs in the disassembly, the decompiler form is in the output above).
4. Orientation toward target (if |target - pos| > 0.01): target = mode+0x4dc..0x4e4, pos =
   char+0x2c position at +0x34..0x3c. Calls 0056e4e0, 0056dfe0 (rotate by `angle * dt`),
   00572110 (copy 3x4), 00570250 (matrix to euler, written back to mode+0x1c..0x24).
   - `angle = pi/4 = 0.785398` if mode+0x4c0 == 0, else `3pi/4 = 2.35619` (constants
     00b3990c, 00b39910). `[game]`
   - Before that: if mode+0x74 != 0 and mode+0x90 > 0.1 and the z-component of the offset is
     < 0, that z-component is set to 0 (no pitch-down toward the target). `[game]`
5. Yaw rate lag (still inside the gate of step 1; this runs every step the gate passes):
   ```
   rate_target = T[0x60] * 0.0174533 * (*yawStick)     // deg/s -> rad/s
   lag = T[0x6c]
   if lag * dt < 1 and lag > 0:
       mode[0x20] = mode[0x20] + (rate_target - mode[0x20]) * lag * dt
   else:
       mode[0x20] = rate_target
   ```
   `[game]` (0x869941 for the 0.0174533 factor; decompile lines near the end). Note that
   the first `fVar6 = T[0x6c]` read is `*(T+0x6c)` with T = mode+0x18. `[assumed]` that
   T+0x6c is turnLag (0.15 Bumblebee) and T+0x60 is turnSpeed or maxTurnSpeed.
   CORRECTED 2026-10-16 (flight session): mode+0x20 is the ROLL, not a yaw rate; T+0x60 is
   maxRoll and T+0x6c rollStrength (flight-attributes.md), and `*yawStick` is the stick
   pair's first slot (lateral L). So this is the player's step 8 (flight-steering.md) word for
   word. The yaw rate mode+0x44 is not touched by 008693f0.

2026-10-16 second read (asm 008697a0-008699c9, flight session), settling step 4:
- Arguments: EAX = the nose N (the same `&[F+0x28]` as 00866ef0's arg 2), arg 2 = the stick pair,
  arg 3 = dt (call at 00864977). Step 3's `n` is the nose. `[game]`
- `mode+0x90` is the drop probe's ground normal z (0086b230 copies +0x34c..0x354 to +0x88..0x90 when
  it sets the latch +0x74), and `mode+0x74` the on-ground latch: "on the ground and the ground is not a
  wall" keeps it from aiming down. `[game]`
- The frame: current = body_world_matrix (006de7e0) at esp+0x88; target = look-at 0056e4e0(dir, current)
  at esp+0x4c; `0056dfe0(out, current, target, angle * dt)`: current turned TOWARD the look-at.
  0056db30 caps acos|qa.qb| (half the turn) at the angle, so pi/4 a second turns the nose up to pi/2 a
  second. `[game]`
- Port: `flight.rs` `Fly::steer_ai`, `look_at`, `turn_toward`, `angles_of` (2026-10-16).
6. Final: mode+0x1c..0x24 = matrix_to_euler (00570250) of the rotated frame.

Writes: body +0xa0..0xa8 (step 3), mode+0x1c..0x24 (steps 4, 6), mode+0x20 (step 5).
Reads: mode+0x4 (owner), +0x18 (tuning), +0x38/+0x3c (char handles), +0x2c (char transform),
+0x4c0, +0x4dc..0x4e4, +0x74, +0x90, body +0x34, +0x38, +0xa0..0xa8.

---

## 00868b00  flight_surface_bounce (proposed)

Called from 00864320 in the contact branch (mode+0x55 set, char+0x84 clear, char+0xb0 clear).

Arguments (checked from the call site and the decompile `unaff_EDI`):
- mode in EDI (not on the stack).
- arg 1 = `float *steer` (3 floats: the same steer vector 00864320 passes to the steer
  routines, `&iStack_128`). Read as a direction below.
- arg 2 = `dt` (float; the decompiler's `param_2`). Checked: `fld` order at entry is not
  re-verified; the decompile treats it as float.

Steps:
1. Velocity reflect (body as in 008693f0; lever arm 0; gate body +0x38 == 0 and +0x34 == 0.0):
   ```
   n = (steer[1] stuff)   // 2D normal built from the input direction
   k = -0.9 * dot(n, v)   // constant -0.9 at 0x868c31
   v += n * k             // v = v + n*k, body +0xa0..0xa8
   ```
   The decompile's normal is n = (-steer[0], steer[1], 0) in the `(-0.0 - x)` terms;
   dot(n, v) = steer[1]*vx - steer[0]*vy, and the z component of n is 0. `[game]`
   (`mulss -0.9` at 0x868c31; `-0.0` sign flips at 0x868b06 and 0x868d6b.)
2. Latch (dot test): `c = mode+0x68*steer[0]... ` the decompile's
   `d = mode[0x70]*steer[2] + mode[0x6c]*steer[1] + mode[0x68]*steer[0]`.
   - if d > 0.95 (constant at 0x868d0c) or mode+0x56 != 0: mode+0x44 = 0, mode+0x56 = 1.
   - else: `ramp = mode+0x58` (target), lerped toward ramp with `ramp_new = ramp + (mode+0x44 -
     ramp)*5*dt` if 5*dt < 1 (`5` at 0x868cf2 and 0x868df6), stored in mode+0x44, then
     mode+0x44 is the yaw rate: `+-T[0x60] * 0.0174533` (sign by mode+0x44 >= 0).
3. Integrate heading: `mode+0x24 = mode+0x24 - dt * mode+0x44`, then wrap to [-pi, pi):
   ```
   x = mode[0x24]
   mode[0x24] = ((x * 0.159155 + 0.5) - floor(x * 0.159155 + 0.5) - 0.5) * 6.2831855
   ```
   constants 0.159155 = 1/(2pi) at 0x868dd1; 0.5 at 0x868dda; 6.28319 at 0x868e1a.
   `floor` is `FUN_00542f80` (not read; the decompile's `extraout_ST0` is its result. `[assumed]` floor).
   > CORRECTION 2026-10-12 (flight-boundary.md 12.9, settled): mode+0x20 is ROLL and mode+0x1c is PITCH; mode+0x24 is heading. The text below that calls mode+0x20 a yaw-rate target and mode+0x1c roll is wrong.
4. mode+0x20 = the yaw-rate target from step 2 with lerp `5*dt` (`5` at 0x868df6): if 5*dt < 1,
   `mode[0x20] = (target - mode[0x20]) * 5*dt + mode[0x20]`; else target. `[game]`
5. Gate on mode+0x64 (the z of a stored vector mode+0x5c..0x64):
   - if mode+0x64 >= -0.95 (constant 0x868fe6 is in 00868fb0; here `-0.95` at 0x868e44):
     - roll mode+0x1c decays: if 5*dt < 1, `mode[0x1c] = mode[0x1c] - mode[0x1c]*5*dt`;
       else 0. `[game]`
   - else (steep or inverted): body velocity += `(mode+0x5c, +0x60, +0x64) * 30 * dt` if
     body +0x34 == 0.0 and body +0x38 == 0 (`30.0` at 0x868e59); and roll mode+0x1c lerps
     toward `-0.5235988` (= -pi/6, 0x868f3b) at rate `2*dt` (`2.0` at 0x868f20) if 2*dt < 1,
     else set -pi/6 directly.
6. If not cleared: mode+0x1c is written with uVar11 (0 or -pi/6) on the paths that fall through.

Writes: body +0xa0..0xa8 (steps 1, 5), mode+0x20, +0x24, +0x44, +0x56, +0x1c.
Reads: steer[0..2], mode+0x18 (tuning +0x60), +0x44, +0x58, +0x5c..0x70, +0x56, body +0x34, +0x38.

Known open point: the sign and the exact pairing of steer[0]/steer[1] in step 1 are taken
from the decompile's `(-0.0 - x)` and the disassembly's sign flips, not from a test.
`[assumed]`. Test it with a steer vector and a velocity before porting.

---

## 00868fb0  flight_wall_stop (proposed)

Called from 00864320 in the contact branch when char+0x84 or char+0xb0 is set (not the
case of 00868b00).

Arguments: mode in EAX (`in_EAX`); arg 1 = `float *out` (the 3-float steer vector, the
same `&iStack_128` as 00868b00); arg 2 = dt.

Steps (decompile and constants checked):
1. Gate: if mode+0x64 >= -0.95 (constant at 0x868fe6), do the wall logic (step 2); else
   the inverted path (step 4).
2. Forward row: `f = body_world_matrix` (006de7e0) row 0 (`pfVar5[0..2]`). The code forms
   `w = (-f.y, 0, f.x)` after multiplications by zero (`*0.0`, zero register). `[game]`
   `w` is the "forward" direction in the plane used for the wall test. `[assumed]` name.
   - `c = dot(mode+0x68..0x70, w)`. If `c >= 0.8` (constant `0.8` at 0x869135): out =
     (0, 1, 0) (`local_94 = 1.0`, 0x869... `0x3f800000`). `[game]`
   - else: `d = mode[0x68..0x70] - w`; if |d| > 1e-8, normalise. Then `holder_inverse_matrix`
     (005e3820) applied to d, and the body velocity is read (00556c00), scaled by 0.95
     (`0.95` at 0x869324) and set (00557000). The velocity -0.9 reflection (`-0.9` at
     0x8692bd) is also on this path. `[game]` The out vector stays (0,0,0).
3. Common tail: `FUN_00867f20(out, dt)` (not read).
4. Inverted path (mode+0x64 < -0.95): once-only tip (`cmp byte [0xdad8ae]`, `FUN_00750660`,
   `FUN_00a8b36e`), then a virtual call on the input source (`vtable +0x28`). `[game]`
   The tip path is UI; treat as non-physics.

Writes: body +0xa0..0xa8 (step 2 else), out vector (step 2), and the once-only UI flag at
0xdb0414 (inverted path). Reads: mode+0x4 (owner), +0x64, +0x68..0x70, +0x6c, body +0x34..0x38.

Unfinished: the else-branch vector algebra of step 2 is not fully traced (locals local_a4..,
local_bc..); this is the part to re-check with `raw_disasm.py 0x869196 0x869330` if the port
needs it.

---

## 00869df0  flight_speed_integrator (proposed)

Called once per step from 00864320: `00869df0(dt, byte_a, byte_b)`, mode in EAX.
Arguments: arg 1 = dt (float, the decompiler's `param_1`); arg 2 = `char` from 00864320's
`iStack_11c` (a gate; the decompiler's types disagree); arg 3 = `char` from `fStack_130`
(a gate; same caveat). Both are tested as booleans only. `[trace]` the caller's types are
unclear; decode the true type from the caller before porting.

Setup:
- v = body linear velocity (+0xa0, +0xa4, +0xa8); if the body pointer is null, v = 0.
- `s = |v|`; `u = v / s` if s > 1e-8.

Branch A: arg 2 == 0 and arg 3 == 0 (normal flight):
1. If mode+0x4c1 != 0 (horizontal quadratic drag):
   ```
   s_h = |(vx, vz)|                       // horizontal (the decompile uses x and z)
   s_new = s_h - (s_h*s_h / (K + K)) * T[0x98] * dt     // K = mode+0x4c4
   (vx, vz) = unit(vx, vz) * s_new
   ```
   The code does: `local_90 = s_h - (s_h²/(2K)) * T[0x98] * dt`, then it normalises and
   multiplies by `local_90`. `[game]` (the divisor reads `*(in_EAX+0x4c4)` twice; `2.0` is
   not a constant, it is the sum of two equal reads.) `K` meaning `[assumed]` (perhaps the
   top speed).
2. If mode+0x4c2 != 0 and mode+0x4c8 != 0: the second velocity component (`local_58`) is
   reduced: `c = c - (c*c / (2*mode+0x4c8)) * dt`. `[game]` (2.0 at 0x868... decompile
   shows `*(in_EAX+0x4c8)*2.0`.) `[assumed]` the component is the vertical one.
3. If the body's flag (mode+0x34 pointer, byte +0x24) == 0: call 0086a840 (not read), return
   if |v| <= the given value, else set v = |v| * u (no change). `[game]` (The decompile here is
   partly garbled; the outcome is "keep the velocity".)
4. Else (flag set): if mode+0x4b4 != 0, clear the flag and return. Else latch: if
   mode+0x4a8 == 0, call 0086a950 and set `mode+0x4ac = |v|`, `mode+0x4a8 = 1`.
   Then:
   ```
   d = T[0xd4] * dt
   mode[0x4ac] = mode[0x4ac] - d
   call 0086a840
   if mode[0x4ac] <= d: mode[0x4ac] = d; mode[0x4a8] = 0; body flag = 0
   v = mode[0x4ac] * u                  // set body velocity
   ```
   `[game]` (`T[0xd4]` read at +0xd4 of the tuning block, `0xd4` in the decompile as
   `*(in_EAX+0x18)+0xd4`.) `[assumed]` 0xd4 is a decay rate, and the latch is the turbo
   speed that decays back to the normal speed.

Branch B: arg 3 != 0 (the other gate):
```
mode[0x4ac] = mode[0x4ac] - T[0xd8] * dt
call 0086a840
f = T[0xd0]
if f*f <= mode[0x4ac] (and not equal): mode[0x4ac] = f*f
v = mode[0x4ac] * u
```
`[game]` (reads +0xd0 and +0xd8 of the tuning block). `[assumed]` floor = T[0xd0]^2 (the
decompile multiplies `fVar6 = d0 * local_ac` with local_ac = d0).

Branch C: arg 2 != 0 (the `else` of the first test in the decompile, the other half of the
gate): calls 0086a950 then sets the velocity from `fVar6 * u` (a speed `fVar6` from
0086a950's result) and decrements a refcount (`0x532c70`). Not traced further.

Writes: body +0xa0..0xa8 (every branch), mode+0x4a8, +0x4ac, body flag +0x24 of mode+0x34.
Reads: mode+0x4 (owner), +0x18 (tuning +0x98, +0xd0, +0xd4, +0xd8), +0x34, +0x4a8, +0x4ac,
+0x4b4, +0x4c1, +0x4c2, +0x4c4, +0x4c8.

---

## 0086aa60  flight_fx_event (proposed)

Called twice from 00864320: `0086aa60(0)` when button 0x1f is not held (line 138), and
`0086aa60(1)` on the edge when it is held (line 150). Mode in EAX; arg 1 = char (0 or 1).

- arg 1 == 0: sends event hash `0x9caa723d` (name not in `export\data\names.tsv`) with
  `0x267ea3dc` as the object key, to the owner's object `char+0x30` through `FUN_00724940`.
- arg 1 != 0: sends `0xd5d636c1` ("Trigger") with the same key.
- Then releases a refcount on the returned handle (`0x532c70`). `[game]`

The ledger (`007325e0 ability_effects`) says the on/off pair is the FX_brake / FX_smoke_exhaust
pair and that only FlyMode calls it with 1. `[game]` (ledger). The 0x9caa723d name is `[data]`
open; resolve with `tools\tf2_names.py lookup 9caa723d` before naming the effect.

Writes: none to the mode. Reads: mode+0x4 (owner) then char+0x30.

---

## 0086c970  flight_probe_ahead (proposed)

No arguments; mode in EAX. Called near the end of 00864320.

1. Gate: if T[0x84] == 0 (tuning flag, `[assumed]` helicopter-or-hover flag):
   - If mode+0x430 != 0: if mode+0x484 >= FLT_MAX (3.40282e38, 0x86c9a0): mode+0x4c0 = 0;
     else mode+0x4c0 = 1, mode+0x4cc = mode+0x484 (hit distance), and the hit point
     mode+0x4d0..0x4d8 = mode+0x40c..0x414. `[game]`
   - If mode+0x431 == 0 or mode+0x430 != 0: 
     - `FUN_0074af20` (a world query; not read) on the owner's object.
     - Probe start `P = char+0x2c position (+0x34, +0x38, +0x3c)`.
     - Probe direction `D = (char+0x1c, char+0x20, char+0x24)` (a column of the rotation).
     - Probe length `L = T[0xa0]`.
     - Segment: `mode+0x3d0..0x3d8 = P`, `mode+0x3dc = 1.0`, `mode+0x3e0 = P.x + L*D.x`,
       `+0x3e4 = P.y + L*D.y`, `+0x3e8 = P.z + L*D.z` (the decompile writes them with the
       matrix row offsets), `mode+0x3ec = 1.0`. Then `probe_submit` (006e0a30).
     `[game]` (`mulss` at 0x86cbd5 region; `1.0` = 0x3f800000 at 0x86cba8.)

Writes: mode+0x4c0, +0x4cc, +0x4d0..0x4d8, +0x3d0..0x3ec, probe queue (006e0a30).
Reads: mode+0x430, +0x431, +0x484, +0x40c..0x414, char+0x2c..0x3c, T+0xa0, T+0x84.

Unfinished: the direction axis (char+0x1c..0x24 is the rotation column the decompile labels
fVar8/fVar6/fVar7); whether it is "forward" or "up" is `[assumed]` forward per the ledger's
body_world_matrix row note. Check with a capture if the probe matters for the port.

---

## 0086cbf0  flight_surface_anim (read and ported 2026-10-20; see the end of this section)

Arguments: arg 1 = mode (stack); arg 2 = dt (float). Returns on dt <= 0.

Visual only (does not change the body). Two branches on the tuning flag T[0x84]
(`[assumed]` helicopter flag; see the data note: helicopter false for Bumblebee, Starscream;
true for Blackout):

Fixed-wing branch (T[0x84] == 0):
1. Velocity estimate: `r_i = (mode+0x1c, +0x20, +0x24) - (mode+0x28, +0x2c, +0x30)`
   divided by dt, i.e. the angular rates of the euler triple. Rates are smoothed:
   `k = dt * 4.4` (`4.4` at 0x86cfbb). If `k < 1`: `s_i = s_i + (r_i - s_i) * k`, else
   `s_i = r_i`. Smoothed values go to mode+0x4ec, +0x4f0, +0x4f4. `[game]`
   > CORRECTION 2026-10-12 (flight-boundary.md 12.9): the heading is mode+0x24, not mode+0x20 (mode+0x20 is roll); the pitch and roll drivers here are mode+0x1c and mode+0x20 respectively.
2. Rotate by the heading `h = mode+0x20` (decompile `local_564`): `(x, y) = (s_x cos h - s_z sin h,
   s_z cos h + s_x sin h)` (`cos` 0040b080, `sin` 0040b0a0). The rotated pair (fVar11, fVar8)
   is used as roll and pitch drivers. `[game]` (the exact pairing of the outputs is from the
   decompile; the sign conventions are `[assumed]`).
3. Node rotations (each found by `FUN_00578440(name hash)`, applied with `rotate_x_matrix`
   0056fb50 or `rotate_z_matrix` 0056fc50, then `FUN_00560fa0` and `FUN_00545850`):
   - Aileron_L (`143cfef2`): x-rotation by `T[0xbc] * s_y`.
   - Aileron_R (`ee33c391`): x-rotation by `-T[0xbc] * s_y`.
   - Flaperon_L (`823e7d36`) and Flaperon_R (`78314055`): same as the ailerons (`T[0xbc]*s_y`).
   - Rutterwing_L (`c3dc1d5b`), Rutterwing_R (`39d32038`), Rutterwing_L01 (`f5080fac`),
     Rutterwing_R01 (`e3b081d6`): x-rotation by `-T[0xc0] * rotated_pitch`.
   - Vert_StabilizerA_L (`68d9500a`), Vert_StabilizerB_L (`6a9fee53`), Vert_StabilizerA_R
     (`92d66d69`), Vert_StabilizerB_R (`9090d330`): z-rotation by `-T[0xc4] * rotated_roll`.
   `[game]` (name hashes from the decompile comments; `0x143cfef2` etc. are CRC-32 names.)

Helicopter branch (T[0x84] != 0): 
- `mode+0x49c = mode+0x49c - dt*2` (`2.0` at 0x86cd19 region) and clamp to `T[0xdc]`
  (minRotorAlpha 0.25 in the data). `[game]`
- Then a loop over blade nodes via `FUN_00765b20` (list walk) with constants `3.14159`,
  `0.00555556` (1/180) and `15.708` (5pi) at 0x86ce1d..0x86ced0. The loop is NOT fully
  read; see "Not read".

Writes: mode+0x4ec..0x4f4 (smoothed rates), mode+0x49c (rotor), node transforms.
Reads: mode+0x1c..0x24, +0x28..0x30, +0x20, +0x4ec..0x4f4, +0x49c, mode+0x18 (T+0xbc, +0xc0,
+0xc4, +0xdc, +0x84), char object via `character_object_lookup`.

### Read in disassembly and ported, 2026-10-20 (`core\src\flightsurface.rs`)

This supersedes the "proposed" text above where they differ. `[game]` throughout unless tagged.

- Fixed wing (0086cf4e-0086d557). Rates `r = wrap(angle - prev) / dt` for (pitch +0x1c, roll
  +0x20, heading +0x24) (wrap 0056f290; 1/dt in x87). Smoothed into +0x4ec/+0x4f0/+0x4f4 by
  `s += (r - s) * dt * 4.4` when `4.4 dt < 1`, else `s = r`. Then with the ROLL angle `R` (mode+0x20,
  saved at 0086cf74 into [esp+0x40]): `a = s_pitch cos R - s_heading sin R`,
  `b = s_heading cos R + s_pitch sin R` (x87 at 0086d089-0086d0b7; the stack slots were tracked
  through the stdcall 00578440 calls, which pop their argument). So the rotation angle is the roll,
  as the 2026-10-12 correction says the drivers are; the earlier "heading" label was wrong.
- Signs, checked block by block: Aileron_L `Rx(+aileronScale * s_roll)`, Aileron_R `Rx(-...)`,
  Flaperon_L `Rx(+...)`, Flaperon_R `Rx(-...)`; Rutterwing_L, _R, _L01, _R01 all `Rx(-elevatorScale * a)`;
  Vert_StabilizerA_L, B_L, A_R, B_R all `Rz(-rudderScale * b)`. The negations are `-0.0 - x` (subss
  with 00b6a3ac). The names of T+0xbc/+0xc0/+0xc4 are the loader's (`docs/flight-attributes.md`):
  aileronScale, elevatorScale, rudderScale.
- The node's local is REPLACED by the bare turn: `basis_to_affine` 00560fa0 zeroes the translation
  before `base_pose_set_local`. `[data]` every such node in the four jets' vehicle objects sits at
  zero offset under its own `Parent_<name>` pivot (`core\examples\surface_nodes.rs`), so nothing jumps.
- Helicopter (0086cd10-0086cf44). `+0x49c -= 2 dt`, floored at minRotorAlpha (T+0xdc). The blade
  loop: 00765b20 copies the list at T+0x1d0 (`bladeList`, `[assumed]` the loader's place for it); for
  each blade name found in the vehicle object's name table (+0x74, count +0x64) it calls 0086ee10
  with {alpha, node index}. 0086ee10 sends that under key 0x3156f8c8 to the object's listeners; the
  RenderInstance ctor 00587100 registers 00588050 for it (via 01177780), which calls 00587c70: if the
  instance's node is the blade or below it, `lod_fade_table_write` 0058b3d0 with xmm0 = alpha. So
  the blades' meshes take the alpha as their per-instance fade (the same fade a level LOD writes).
  Blackout's list: Rotor_A..F, BlackoutV_Tailrotor_A..D, BlackoutV_TailrotorBase_A..D; all 14 are
  on his vehicle model, one render each.
- Then Rotor_GOAL: its current BasePose local (00572110 from +0x6c + 0x30 (idx-1)) composed with
  `Ry(-(mode+0x490) * pi * (1/180) * dt)` (0056fbd0 is a positive Ry) on the node's own side
  (compose_affine, P1 = current, P2 = turn). RearRotor: the same with `Rx(dt * 5 pi)` (15.707964),
  a fixed 2.5 turns a second. No Rotor_GOAL: the function exits before RearRotor.
- mode+0x490: flymode_init 00863970 sets +0x490 = 1100, +0x494 = 1500, +0x498 = 900 (deg/s), and
  nothing else writes +0x494/+0x498; 00867f20's last block (SM) eases +0x490 at rate dt toward
  1500 when its final P >= 0.05, 900 when P <= -0.05, 1200 between (the comparison is on 0 - P,
  00868a86; P is the stack slot N4 also reads, so after the turbo scale, invert and ground latch).
- 00863970 zeroes +0x28..0x30 and +0x4ec..0x4f4; the enter 00864d60 writes only +0x49c = 1. The
  state survives flights (`sim::State::flight_surfaces`).

Port: `flightsurface::Surfaces` (state, `steer_rotor` = SM, `step` = 0086cbf0, `pose`, `apply`,
`blade_nodes`), stepped in `sim::step_fly` after the steering; `Fly::rotor_lift` carries P;
`game\src\animation.rs` lays the turns on the vehicle clip's pose while flying (`[assumed]` after
the clip: the jets' transformation and bank clips and Blackout's transformation clips drive these
nodes, and the order of the clip's write and the mode update is not read); `game\src\flightparts.rs`
draws the blade fade through the port's LOD-fade screen door (`[stand-in]`, the fade table's draw-time
reader is not read).

Checked (2026-10-20): `core\examples\surface_fly.rs <pack:Name> [pitch|roll|lift]` prints the angles,
rates and turns in flight. With Starscream rolling, the ailerons reach +-21 deg. With Blackout, the alpha
goes 1 -> 0.25 and the rotor speed moves 997..1324 with the lift. Captures `work\shots\surf_black_a/b.png`
show the faded blades turned 0.1 s apart. Nothing is compared with the original.

---

## Fields seen

Base: mode = FlyMode object. T = mode+0x18 (its tuning block; for FlyMode `[assumed]` the
modeFly block of the character). C = mode+0x4 (owner character). B = body = `**(C+0x38)+0xa4`
read as `**(holder+0xa4)`; the holder is C+0x38. `[game]`

| base | offset | meaning (proposed) | read by | written by |
|---|---|---|---|---|
| mode | +0x4 | owner character C | 008693f0, 00868b00, 00868fb0, 00869df0, 0086aa60, 0086cbf0 | (set at construction) |
| mode | +0xc, +0x10, +0x14 | control vector (stick weight per axis) | 008693f0 | (set elsewhere) |
| mode | +0x18 | tuning block T | all | |
| mode | +0x1c, +0x20, +0x24 | euler triple (asin decomposition), then overwritten by steer; +0x20 is the yaw rate after the steer | 00868b00, 0086cbf0 | 00864320 (euler), 008693f0, 00868b00 |
| mode | +0x28, +0x2c, +0x30 | previous euler triple | 0086cbf0 | 00864320 (copy of +0x1c..0x24) |
| mode | +0x44 | yaw rate (rad/s), ramped | 00868b00 | 00868b00 |
| mode | +0x4c | 1 while the steer button is held | 00864320 | 00864320 |
| mode | +0x4a8 | speed latch on | 00869df0 | 00869df0 |
| mode | +0x4ac | speed latch value | 00869df0 | 00869df0 |
| mode | +0x4b4 | latch clear flag | 00869df0 | |
| mode | +0x4c0 | obstacle ahead flag | 008693f0 (picks pi/4 vs 3pi/4) | 0086c970 |
| mode | +0x4c1 | horizontal drag enable | 00869df0 | |
| mode | +0x4c2 | component decel enable | 00869df0 | |
| mode | +0x4c4 | K (drag divisor, 2K) | 00869df0 | |
| mode | +0x4c8 | X (component decel divisor) | 00869df0 | |
| mode | +0x4cc, +0x4d0..0x4d8 | obstacle distance and hit point | 008693f0 (not), 0086c970 | 0086c970 |
| mode | +0x4dc..0x4e4 | target point for orientation | 008693f0 | |
| mode | +0x4ec..0x4f4 | smoothed body-frame angular rates | 0086cbf0 | 0086cbf0 |
| mode | +0x49c | rotor value (helicopter) | 0086cbf0 | 0086cbf0 |
| mode | +0x55 | contact (surface) mode flag | 00864320 | |
| mode | +0x56, +0x58 | surface latch and target | 00868b00 | 00868b00 |
| mode | +0x5c, +0x60, +0x64 | push vector (z at +0x64 gates) | 00868b00 | |
| mode | +0x68, +0x6c, +0x70 | stored wall / reference direction | 00868b00, 00868fb0 | |
| mode | +0x74 | flag, with +0x90 | 008693f0 | |
| mode | +0x90 | float threshold 0.1 (`[game]` 0.1) | 008693f0 | |
| mode | +0x3d0..0x3ec | probe segment | | 0086c970 |
| mode | +0x430, +0x431 | probe flags | 0086c970 | |
| mode | +0x484 | obstacle distance (FLT_MAX = none) | 0086c970 | |
| mode | +0x40c..0x414 | probe hit point | 0086c970 | |
| mode | +0x34 (pointer) +0x24 (byte) | body flag used in 00869df0 | 00869df0 | |
| T | +0x60 | turn rate, deg/s (`[assumed]` turnSpeed or maxTurnSpeed) | 008693f0, 00868b00 | |
| T | +0x6c | yaw-rate lag rate (`[assumed]` turnLag) | 008693f0 | |
| T | +0x84 | flag: helicopter or hover (`[assumed]`) | 00864320, 0086c970, 0086cbf0 | |
| T | +0x98 | horizontal drag coefficient (`[assumed]`) | 00869df0 | |
| T | +0xa0 | probe length (`[assumed]`) | 0086c970 | |
| T | +0xb0 | flag: contact branch selection | 00864320 | |
| T | +0xbc | aileron angle scale (`[assumed]` aileronScale) | 0086cbf0 | |
| T | +0xc0 | rutterwing scale (`[assumed]`) | 0086cbf0 | |
| T | +0xc4 | stabilizer scale (`[assumed]` elevator/rudder) | 0086cbf0 | |
| T | +0xd0 | speed floor base (floor = d0^2) (`[assumed]`) | 00869df0 | |
| T | +0xd4 | speed-latch decay rate (`[assumed]`) | 00869df0 | |
| T | +0xd8 | speed-latch decay (branch B) (`[assumed]`) | 00869df0 | |
| T | +0xdc | minimum rotor value (`[data]` minRotorAlpha 0.25) | 0086cbf0 | |
| char | +0x1c..0x24 | rotation column used by probe and smoothing | 0086c970, 0086cbf0 | |
| char | +0x2c | character transform (+0x34..0x3c position) | 008693f0, 0086c970 | |
| char | +0x30 | owner object id (for FX events) | 0086aa60, 0086c970 | |
| char | +0x38 | physics holder H; body = `**(H+0xa4)` | 008693f0, 00868b00, 00868fb0, 00869df0 | |
| char | +0x3c | refcounted handle (`lock xadd` on +4) | 008693f0, 00868b00 | |
| char | +0x84 | byte flag (state gate) | 00864320, 0086c970, 0086cbf0 | |
| char | +0xb0 | byte flag (contact gate) | 00864320 | |
| body | +0x34 (float) | body flag, must be 0.0 to write velocity | all velocity writers | |
| body | +0x38 (byte) | body flag, must be 0 to write velocity | all velocity writers | |
| body | +0xa0, +0xa4, +0xa8 | linear velocity x, y, z | all | all |
| body | +0xac, +0xb0, +0xb4 | angular velocity (multiplied by a zero lever) | 008693f0, 00868b00 | |

Note: the decompiler used different types for the same byte in 00869df0's arguments. `[trace]`

## Constants

| value | where (address) | use |
|---|---|---|
| 1e-08 (00b345d8) | 008693f0, 00869df0, 00868fb0 | normalisation guard |
| 0.01 (00b343a0) | 008693f0 | distance guard for the target |
| 0.1 (00b345ec) | 008693f0 | mode+0x90 threshold |
| 0.8 (00b6a3f0) | 00868fb0 (0x869135) | cosine limit on the wall dot |
| 0.95 (00b6a8e8) | 00868b00 (0x868d0c), 00868fb0 (0x869324) | latch threshold; velocity scale |
| -0.95 (00b6b194) | 008693f0 (0x869636), 00868b00 (0x868e44), 00868fb0 (0x868fe6) | reflection factor (008693f0); inverted gate |
| -0.9 (00b6b19c) | 00868b00 (0x868c31), 00868fb0 (0x8692bd) | normal-velocity damping |
| 5 (00b3476c) | 00868b00 (0x868cf2, 0x868df6) | lag rate for the yaw and roll smoothing |
| 2 (00b38838) | 00868b00 (0x868f20), 0086cbf0 | rate for roll toward -pi/6 |
| 30 (00b34570) | 00868b00 (0x868e59) | push acceleration on the steep gate |
| -0.5235988 (00b6b198) | 00868b00 (0x868f3b) | roll target -pi/6 |
| 0.0174533 (00b6a40c) | 008693f0 (0x869941), 00868b00 (0x868d63, 0x868d87) | deg to rad |
| 0.785398 (00b3990c) | 008693f0 (0x8698c7) | orientation rate cap, pi/4 |
| 2.35619 (00b39910) | 008693f0 (0x8698bd) | orientation rate cap, 3pi/4 |
| 0.159155 (00b6a958) | 00868b00 (0x868dd1) | 1/(2pi) for the wrap |
| 6.28319 (00b387d8) | 00868b00 (0x868e1a) | 2pi for the wrap |
| 0.5 (00b38830) | 00868b00 | wrap offset |
| 3.40282e+38 (00b387b4) | 0086c970 (0x86c9a0) | FLT_MAX = no hit |
| 4.4 (00b6b344) | 0086cbf0 (0x86cfbb) | smoothing rate of the body-frame rates |
| 3.14159 (00b387d4), 0.00555556 (00b6b34c), 15.708 (00b6b348) | 0086cbf0 (0x86ce1d..0x86ced0) | Rotor_GOAL deg-to-rad of mode+0x490; RearRotor 5 pi rad/s |
| 0x267ea3dc, 0x9caa723d, 0xd5d636c1 | 0086aa60 | event key and hashes (Trigger = d5d636c1) |

## Not read

- Helpers called by these seven, not read: 0086a840, 0086a950 (speed latch helpers);
  0070aaf0 and 00557150 (a push / pop pair around the body velocity read and write; not
  confirmed); 0056f290 (a float helper, probably absolute value); 0040b080 cos; 0040b0a0 sin;
  00542f80 (floor, `[assumed]`); 00544340 (virtual no-op); 00578440 (node find by name hash);
  00560fa0 and 00545850 (node matrix set); 0056fb50 rotate_x; 0056fc50 rotate_z;
  00765b20 (list walk); 0074af20 (world query); 006e0a30 probe_submit (ledger);
  0056e4e0, 0056dfe0, 00572110, 00570250 (orientation helpers; 00570250 is
  matrix_to_euler_asin per the 00864320 caller); 00867f20 (the common steer tail);
  0086e020; 00866ef0 and 008676c0 (steer routines not in the seven; the pitch lag, maxPitch and
  maxRoll clamps are there `[game]` (ledger)); 0086b230 (ledger: flight-form update);
  009518f0 and 007255d0 (in 0086cbf0's object lookup); 00750660 and 00a8b36e (UI tip).
- (Read 2026-10-20: the blade loop of 0086cbf0, see its section.)
- The else-branch of 00869df0 (arg 2 != 0) beyond the latch read.
- The vector algebra of 00868fb0's else-branch step (locals local_a4..local_bc).
- The modeFly loader that names the tuning offsets: not found. Every
  T+0x.. name above is `[assumed]` until that loader is found.
- Where thrust is applied (not in the seven).
- What K (mode+0x4c4) and X (mode+0x4c8) are set to.
