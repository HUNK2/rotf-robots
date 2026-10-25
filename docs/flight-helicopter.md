# Helicopter steering: 00867f20 (port recipe)

Ticket: work/survey/flight_brief.txt. Tags: `[game]` read in disassembly (address given),
`[data]` from docs/flight-attributes.md, `[assumed]` not confirmed, `[trace]` not read in asm,
`[stand-in]` none here. The ledger row calls this `flight_control_step_grounded` (its name
in the unpublished notes section 6); this note calls it `flight_control_step_helicopter`.
Same address. The disassembly was read with `work/raw_disasm.py` in slices 00867f20-00868b00
and 008645e0-008648fa; a few caller lines decode out of step and are flagged where they matter.

Read against: the unpublished notes (the jet 00866ef0, the model for this note),
the unpublished notes section 6 (structure sketch; several of its claims are corrected below).

## 0. Where it runs

- Caller: FlyMode update 00864320. The helicopter test is `[game]` at 008648d7-008648e1:
  `mov edx,[ebx+0x18]` (tun); `cmp byte [edx+0x84],0`; `je 0x864904` (not helicopter, jet path).
  This block runs only when `mode+0x55 == 0` (008648890-0086488c: `cmp byte [ebx+0x55],0; je
  0x8648d7`). When `mode+0x55 != 0` the code calls 00868fb0 (helicopter) or 00868b00 (jet) instead
  (008648bf, 008648b5). The call itself is 008648fa: `mov edi,ebx` (008648f8), `call 0x867f20`.
- Second caller: 008693d9 inside FUN_00868fb0 (not read).
- The caller's own thrust block is SKIPPED for the helicopter `[game]`: at 0086461d
  `cmp byte [eax+0x84],0` with eax = tun, then 00864624 `jne 0x8646e2`. The block 0086462a-
  008646c0 (adds the caller's nose thrust to body velocity) is jumped over. The stick build
  008646e2-00864800 still runs for both flags. (Answer to question 7 in part; section 6.)
- The function is stdcall, `ret 0xc` (00868af7-00868afa).

## 1. Arguments and registers (question 1)

Entry is `[game]` 00867f20-00867f36. Stack after the prologue (`sub esp,0x6c`, push ebx, ebp,
esi) puts the arguments at `[E+4]`, `[E+8]`, `[E+0xc]` (E = entry esp).

| where | value | what it is | evidence |
|---|---|---|---|
| ebx = `[E+4]` | N | nose, pointer to 3 floats: caller slot `S+0x28` | 00867f2c, 008648ef-008648f3 |
| ebp = `[E+8]` | Vc | pointer to 3 floats: caller slot `S+0x40` (see below) | 00867f31, 008648e6-008648ea |
| `[E+0xc]` | dt | step length (`[esp+0x84]` inside), copied from the caller's `[ebp+8]` | 008648e3, 00867f5d-00867f69 |
| eax = esi | stick | pointer to 2 floats at caller slot `S+0x70`: stick[0] = L, stick[1] = P | 00867f39, 008648f4 |
| edi | mode | the FlyMode object | 008648f8 |

`S` is the caller's esp at 008648e6 (before the three pushes). The frame equality with the
thrust block (`[esp+0x28]` written at 008645fd-00864609) is `[assumed]`: it needs the stick
block's `sub/add esp` to balance, which the reads suggest but did not check line by line.

- N (arg 1) is the caller's nose copied at 008645df-00864609 from `[esp+0xf0..0xf8]` (source
  of that copy not traced, `[trace]`). In this path it is not scaled (the scale `mulss` at
  0086462d is in the skipped block).
- Vc (arg 2) is three floats `[assumed]`: `Vc0 = L_sq`, `Vc1 = P_sq`, `Vc2 = 0.0`, where
  `L_sq = |L_raw| * L_raw` and `P_sq = |P_raw| * P_raw`, both from the jet's `+0x30` stick
  calls, and the zero from `movss [esp+0x4c],0` at 008647a6. The stores (0086476a
  `fstp [esp+0x44]`, 0086478a `fstp [esp+0x48]`, 008647a6) are read with the caller's esp at
  S-4, which puts them at S+0x40, S+0x44, S+0x48. The raw disassembly of 00864760-0086476d is
  mis-aligned (`.byte` and `ljmp` lines), so this mapping is `[assumed]`.
- stick (in eax) `[game]`: stick[0] = L_h = `D(out2[0], dz)` from the owner's input object at
  008647e0-008647ea, stored `[esp+0x70]`; stick[1] = P_h = `D(out1[1], dz)` at 00864802-00864815,
  stored `[esp+0x74]`. Here `D(a, dz)` is the deadzone map of the unpublished notes ("Stick
  pair") and there is NO squaring (no fabs/fmul between the call and the store). Both come from
  the owner's input object `[[ebx+4]+0x20]` through its vtable `+0x28` (two calls, with outputs
  at caller `+0xac` and `+0xc4`). The input identity (which device axis) is `[unread]`.

Derived in the function: `owner = mode+4`, `tun = mode+0x18`, `char_38 = [owner+0x38]`,
`body = [char_38+0xa4]` (dereferenced once), velocity `V = body+0xa0, +0xa4, +0xa8`,
body gates `body+0x34` (float) and `body+0x38` (byte). `[game]` (008680a0-00868107, 00868703,
00868631). The owner's input object is `[owner+0x20]`; its vtable `+8` with argument 0x14 is the
button query (`[game]` 00868291-008682a0; the brief names 0x14 VehicleDrift).

## 2. Constants (all `[game]` in the asm)

| name | value | address |
|---|---|---|
| DEG | 0.017453292 | 00b6a40c |
| 2*pi | 6.2831855 | 00b387d8 |
| 1/(2*pi) | 0.15915494 | 00b6a958 |
| pi/2 | 1.5707964 | 00b387d0 |
| 0.85 (boost scale) | 0.85 | 00b6abc0 |
| 0.95 (damping) | 0.95 | 00b6a8e8 |
| 0.01 (stick and speed thresholds) | 0.01 | 00b343a0 |
| 1e-8 (normal test) | 1e-8 | 00b345d8 |
| 0.05 (yaw threshold, smoothing dead zone) | 0.05 | 00b34754 |
| 0.5 | 0.5 | 00b38830 |
| 1.0 | 1.0 | 00b38834 |
| 2.0 (pitch rule B rate) | 2.0 | 00b38838 |
| 0.0 / -0.0 | 0 / -0 | 00b387f4 / 00b6a3ac |

## 3. The step, in order (questions 2, 3, 4, 5, 6)

Notation: `P` is the linear pitch stick (stick[1]); `L` the linear lateral stick (stick[0]);
`Q0 = Vc0` (squared lateral), `Q1 = Vc1` (squared pitch). `k_b` is the boost flag.
Scaling is written where it is applied.

```
G. Gate (00867f20-00867f5b): if mode+0xc, mode+0x10 and mode+0x14 are all exactly 0.0,
   continue. Else call flight_contact_heading(mode, dt) [game, ledger] and return.

B. Boost flag (00867f77-00867fb0)  [game]
   boost = timed_ability_active(mode+0x34) AND mode+0x55 == 0
     (timed_ability_active = 00732ce0, ledger; mode+0x34 is the handle it is given, not
      identified here).
   if boost:
       Q1 = 1.0 if Q1 >= 0 else Q1      [game 00867f8a-00867fa8: "1.0" stored into the Q1 slot]
       Q0, L, P *= 0.85                 [game 00867fcd-00868013]
   Note: with boost and Q1 >= 0, the pitch and thrust terms below see Q1 = 1.0, not the stick.
   `[assumed]` this is a designed "full push while boosting"; the asm only shows the value.

I. Invert (00868017-00868062)  [game]
   if not controller_is_ai() and option DAT_00dad8ae: P = -P.
   (options_static_init 00750660 is run once, bit 0 of DAT_00db0414.)

V0. Velocity copy (00868068-00868107)  [game]
   Vb = (V0, V1, V2) = body velocity, copied to the local slots used below.
   If the body pointer is null: Vb = 0.

LT. Pitch latch mode+0x74 (0086812a-0086816b)  [game]
   if mode+0x74 != 0:
       if P >= 0: P = 0; V2 = 0; write Vb to body (set velocity 00557000)
       else:      mode+0x74 = 0
   mode+0x74 is set elsewhere; not found in this function.

D1. Horizontal damping, no stick (0086816f-008681f2)  [game]
   if not boost and sqrt(Q0^2 + Q1^2 + Vc2^2) < 0.01:      (Vc2 = 0)
       V0 *= 0.95; V1 *= 0.95; write Vb to body.           (V2 NOT scaled)
   `[game]` the 0.01 test is `jbe` on fcompi of (0.01, sqrt): proceed when sqrt < 0.01.

D2. Vertical damping, no pitch (0086820d-0086826a)  [game]
   if |P| < 0.01:
       V2 *= 0.95; write Vb to body.                         (runs with or without boost)

H. Hold (drift) branch (00868291-008682c2, then 008682c8-00868345)  [game]
   if hold(0x14) and not boost and not robot (tun+0xb0 == 0):
       Vb *= 0.95 on all three components; write Vb to body.   [game 008682c8-00868314]
       L = 0.5 * Q0             (stick[0] overwritten: 00868320-00868330)
       Q0 = 0                   (00868339)
       P  = 0                   (0086833f)
       goto R (roll). The normal path steps N1 to N3 are skipped.
   else:
       goto N1.

N1. Lateral thrust (0086834a-008683f1, body add 008683e3-00868427)  [game]
   right = (N1, -N0, 0)                       (the horizontal right of the nose; 0086834a-00868377)
   if body.f34 == 0.0 and body.b38 == 0:      (gate, body add; 00868703/00868631 pattern)
       Vb += lateralThrust * Q0 * dt * right  (lateralThrust = tun+0x08)
   `[game]` the "2 * row" term of the sketch is not here; see section 7.

N2. Forward direction (00868466-00868495)  [game]
   W = (N0, N1, 0)                            (N with its z component zeroed; 0086847c-00868495)
   Thrust magnitude (0086846e-008684af):
       T = turboThrust (tun+0x0c) if boost else thrust (tun+0x04)

N3. Forward push and turbo turn (00868536-00868570, 00868572-0086861f, 0086861f-00868670)  [game]
   if not boost:
       Vb += T * Q1 * dt * W                  (plain push; 008685d8-00868670 path)
   if boost:
       D = dot of the normalised body velocity with W  [trace] (00868536-00868570; the
           lanes are not checked; the decompile shows the same test)
       if D > 0:  Vb += tun+0x88 * T * Q1 * dt * W  (helicopterTurboTurnFactor, 00868572-
                    00868586; body_add_velocity 00556b40 at 008685bf)
       else:      Vb += T * Q1 * dt * W            (same as the plain push)
   Writes go to body+0xa0.. only when the body gate holds (00868625-00868631).

N4. Lift (0086869b-008686b8 and 00868703-0086874d)  [game]
   Vb2 += -lift * P * dt        (lift = tun+0x10; xmm0 = -0.0 - lift*P*dt at 008686a3-008686c8)
   Same body gate as N1.
   Note: the vertical term uses the linear P (stick[1]), not Q1.

R. Roll, mode+0x20 (0086876f-008687d9)  [game]
   tr = maxRoll * DEG                         (tun+0x60, 00868779-00868782)
   if mode+0x4c1: tr *= 0.5                   (00868786-00868788)
   T = tr * Q0                                (0086879e)
   k = rollStrength * dt                      (tun+0x6c, 00868790-00868795)
   if 0 < rollStrength and k < 1:  mode+0x20 = mode+0x20 + (T - mode+0x20) * k   (00868797-008687c9)
   else:                           mode+0x20 = T
   No clamp. Note the roll target uses the SQUARED lateral Q0 (and Q0 = 0 on the hold path).

Y. Yaw rate, mode+0x44 (008687e3-00868867)  [game]
   rate = turnLag (tun+0x34) if |L| >= 0.05 else turnRestoreLag (tun+0x38)
                                              (fabs 00409a50, compare 00868754/008687e3-008687f9)
   T = turnSpeed * L                          (tun+0x28, LINEAR L, 0086880c)
   k = rate * dt
   if 0 < rate and k < 1:  X = mode+0x44 + (T - mode+0x44) * k
   else:                   X = T
   X = clamp(X, -maxTurnSpeed, +maxTurnSpeed)     (tun+0x2c; 00868847-00868864)  -- NEW vs jet
   mode+0x44 = X
   if flight_contact_heading(mode, dt) != 0: mode+0x44 = 0     (00868878-00868884)
   helicopterTurboTurnFactor (tun+0x88) is NOT used here; it is used only in N3.

H2. Heading, mode+0x24 (00868889-008688d9)  [game]
   mode+0x24 = mode+0x24 - mode+0x44 * dt
   mode+0x24 = 2*pi * ( frac(mode+0x24 * (1/2pi) + 0.5) - 0.5 )
     frac(x) = x - floor(x)  (FUN_00542f80 is floor, decompile; 008688bc call, 008688c1 fsubr)

PT. Pitch: rule B or rule A (008688e6-00868900)  [game]
   if hold(0x14) and not boost and not robot: rule B
   else:                                      rule A
   (same test as H; the robot flag is tun+0xb0.)

RB. Rule B, hold (0086890e-008689ef)  [game]
   T_B = Q1 * (pi/2)                          (00868927-0086892d; NOT degrees)
   k = 2.0 * dt                               (00868935-0086893d)
   if k < 1:  mode+0x48 = mode+0x48 + (T_B - mode+0x48) * k      (00868941-00868956)
   else:      mode+0x48 = T_B
   mode+0x1c = mode+0x1c - mode+0x48 * dt     (00868963-00868970)
   lo = minPitch * DEG (tun+0x5c), hi = maxPitch * DEG (tun+0x58)   (00868978-00868986)
   mode+0x1c = clamp(mode+0x1c, lo, hi)                             (00868975-0086899f)
   if mode+0x1c == lo or == hi: mode+0x48 = 0                       (008689a4-008689ef)
   No halving by mode+0x4c1 in rule B.

RA. Rule A, not held (008689f4-00868a86)  [game]
   mode+0x48 = 0                                                    (00868a0b)
   M = maxPitch (tun+0x58) if Q1 <= 0 else -minPitch (tun+0x5c)     (008689f7-00868a1c)
       M is a positive number on either side; the sign of Q1 picks the limit.
   if mode+0x4c1: M *= 0.5                                          (00868a21-00868a2a)
   T_A = -(M * DEG) * Q1                                            (00868a32-00868a61)
   k = pitchStrength * dt                 (tun+0x68, 00868a3a-00868a50)
   if 0 < pitchStrength and k < 1:  mode+0x1c = mode+0x1c + (T_A - mode+0x1c) * k  (00868a65-00868a81)
   else:                            mode+0x1c = T_A
   No clamp in rule A. (Corrects flight-helpers.md section 6, which put the minPitch form on
   the negative stick: the asm picks -minPitch for Q1 > 0.)

SM. Smoothing mode+0x490 (00868a86-00868aee)  [game]
   S_t = mode+0x494      if P >= 0.05
       = mode+0x498      if P <= -0.05
       = (mode+0x494 + mode+0x498) * 0.5 otherwise           (00868a86-00868ac9)
   if dt < 1:  mode+0x490 = mode+0x490 + (S_t - mode+0x490) * dt    (00868ad3-00868ae8)
   else:       mode+0x490 = S_t
   mode+0x494 and mode+0x498 are read here and not written in this function; the writer is
   not found in notes. What mode+0x490 drives is not read.
   Return (00868aec-00868afa).
```

## 4. Answers, one place each

- Q2 (velocity): NO across-the-nose 95% removal, that is the jet's. The helicopter applies
  (a) V0, V1 *= 0.95 when `|Vc| < 0.01` and not boost (D1); (b) V2 *= 0.95 when `|P| < 0.01`
  (D2); (c) all three *= 0.95 on the hold branch (H). Thrust: `V += lateralThrust*Q0*dt*(N1,-N0,0)`
  (N1, lateral, squared stick); `V += T*Q1*dt*(N0,N1,0)` (N3, forward, squared pitch, `T` = thrust
  or turboThrust); `V2 += -lift*P*dt` (N4, vertical, linear pitch). The "2 * row" term of the
  sketch is not present; the "0.85" is the boost scale of Q0, L and P (section 3 step B), not a
  pitch scale.
- Q3 (pitch): Rule A (not hold, or boost, or robot): `mode+0x1c` lerps toward
  `T_A = -M*DEG*Q1`, M = maxPitch if Q1 <= 0 else -minPitch, halved by mode+0x4c1, rate
  `pitchStrength*dt` (tun+0x68) when 0 < rate and rate*dt < 1, no clamp; `mode+0x48 = 0`.
  Rule B (hold, not boost, not robot): `mode+0x48` lerps toward `Q1*pi/2` with rate `2*dt`;
  `mode+0x1c -= mode+0x48*dt`; clamp to [minPitch, maxPitch]*DEG; `mode+0x48 = 0` at a limit.
  mode+0x48 is the pitch-rate state of rule B (and rule A zeroes it); not a stick value.
- Q4 (roll): target `maxRoll*DEG*Q0`, halved if `mode+0x4c1`; rate `rollStrength` (tun+0x6c),
  lerp when 0 < rate and rate*dt < 1. `mode+0x4c1` is set by flight_approach_drag 0086dc00
  (ledger; the unpublished notes:88): a horizontal-drag flag set from a speed test against
  tun+0x90. It is NOT a steering choice here, and this function only reads it.
- Q5 (yaw and heading): target `turnSpeed*L` (linear L); rate turnLag if |L| >= 0.05 else
  turnRestoreLag; lerp when 0 < rate and rate*dt < 1; clamp to +-maxTurnSpeed (YES, this
  function clamps; the jet does not); then `flight_contact_heading` zeroes it on contact;
  `mode+0x24 -= mode+0x44*dt` and wrap. `helicopterTurboTurnFactor` (tun+0x88, NOT +0x80 as
  the brief has it) appears: in N3 only, as a multiplier of the boost forward push.
- Q6 (smoothing): mode+0x490 lerps (rate dt, dt < 1) toward mode+0x494 / mode+0x498 / their
  mean, selected by the sign of P with dead zone 0.05 (section 3 SM). Meaning of the quantity:
  not read.
- Q7 (forward thrust): YES, but not from 00864320. 00864320 skips its thrust block for the
  helicopter (0086461d-00864624). Inside 00867f20 the helicopter gets `V += T*Q1*dt*(N0,N1,0)`
  (N3), where T = thrust (tun+0x04) or turboThrust (boost, tun+0x0c), Q1 = squared pitch (or 1.0
  under boost with Q1 >= 0), and the direction is the horizontal nose, z zeroed. Under boost and
  forward motion (D > 0, [trace]) the push is multiplied by tun+0x88.
- Q8 (callees): see section 5.

## 5. Callees (question 8)

- 0x8699d0 flight_contact_heading: ledger (contact steer; returns nonzero, zero yaw).
- 0x732ce0 timed_ability_active: ledger.
- 0x854df0 controller_is_ai: ledger.
- 0x750660 options_static_init: ledger (once, bit 0 of DAT_00db0414).
- 0xa8b36e: called once in the options init; NOT in the ledger, not read.
- 0x409a50 float_abs: ledger (fabs).
- 0x409a70 sqrt: ledger (unread; used as sqrt).
- 0x542f80: decompile is `floor((double)x)` (read now). Used as frac = x - floor(x).
- 0x70aaf0: body handle acquire (decompile: copies a two-word shared handle from
  `param+0x38/0x3c` into the out pointer and bumps a count); ledger lines in the unpublished notes.
- 0x557000 physics_body_set_velocity: ledger.
- 0x557150: releases the handle from 0x70aaf0 (decompile reads only `param+4`); ledger.
- 0x556b40 body_add_velocity: ledger (adds to body velocity, gated by body+0x34 and +0x38).
- 0x532c70: refcount release on the handle (decompile: calls a vtable slot); not read further.
- Vtable calls on the owner input object `[[mode+4]+0x20]`: `+8` with 0x14 (button hold,
  three uses: 00868291, 008688e6, and the same 0x14 test in the caller path); `+0x28` (two calls,
  for L_h and P_h, caller side); `+0x30` (the jet's squared sticks, caller side). Identity of the
  devices behind these is `[unread]`.

## 6. Caller side, short (what this function depends on)

- 008645df-00864609: copies `[esp+0xf0..0xf8]` to `[esp+0x28..0x30]` (N). Source not traced.
- 0086461d-00864624: helicopter skip of the caller's thrust block (to 008646e2).
- 008646e2-00864800: shared stick build (jet and helicopter). For the helicopter the values used
  are the linear D() results at `[esp+0x70]`, `[esp+0x74]` (L_h, P_h), and the squared ones
  written at S+0x40 and S+0x44 (Vc, `[assumed]`).
- 008648d7-008648fa: helicopter branch and call.

## 7. Corrections to earlier notes

- the unpublished notes section 6: rule A's limit is -minPitch for a positive stick and
  maxPitch otherwise (asm 008689f7-00868a1c), not the reverse. Rule B is `Q1*pi/2` (squared
  pitch), not the linear stick.
- The "2*row" term (sketch line 251) is the rule-B rate constant `2.0`, and the "0.85" is the
  boost scale. There is no 95% across-nose damping here.
- The sketch's yaw clamp by maxTurnSpeed is right; the jet has no such clamp.
- The brief's `helicopterTurboTurnFactor` at tun+0x80 is at tun+0x88 (flight-attributes.md,
  asm 00868572).
- The sketch's lateral thrust term uses the SQUARED stick (Q0), not the linear one.

## 8. Not read / not sure

- The source of the caller's nose vector (`[esp+0xf0..0xf8]` in 00864320) and the frame
  mapping S+0x28, S+0x40, S+0x70 (argued from the push sequence, `[assumed]`).
- Vc[0] and Vc[1] as squared sticks, and Vc[2] = 0, from raw stores in a region whose raw
  disassembly is mis-aligned (00864760-0086476d) `[assumed]`.
- The dot test D in the boost branch (00868536-00868570): which lanes, and the sign convention
  of the normalised velocity. `[trace]`. The decompile reads the same test as "velocity
  pointing forward".
- Which device is `L` and which is `P` behind owner-input vtable `+0x28` and `+0x30`
  (`[unread]`).
- mode+0x34 (the handle given to timed_ability_active): not identified. mode+0x55: not identified
  (it gates the boost flag and the caller's branch).
- mode+0x74 (latch) setter: not found in this function and not in the notes read.
- mode+0x494 and mode+0x498 setter: not found in the notes. mode+0x490 consumer: not read.
- The boost design (Q1 forced to 1.0 when Q1 >= 0 under boost) is read, not explained `[assumed]`.
- Fields not used by this function: tun+0x14 (liftQuick), +0x1c/+0x20 (turbo speeds),
  +0x30 (quickTurnSpeed), +0x40..0x4c, +0x64 (maxRollQuick), +0x70, +0x74, +0x78, +0x7c.
- 00868b00, 00868fb0, 008693d9, 00864904 (the jet branch) and the rest of the caller 00864320:
  not read here.
- Frame of flight_contact_heading internals: ledger only.

## 9. Fields seen (read R / write W)

mode (FlyMode, `mode+..`):

| field | R/W | where |
|---|---|---|
| +0x04 owner | R | 00868068, 008648ef region |
| +0x0c, +0x10, +0x14 | R (gate) | 00867f36-00867f5b |
| +0x18 tun | R | everywhere |
| +0x1c pitch | R/W | RB 00868963-00868970, RA 00868a65-00868a81 |
| +0x20 roll | R/W | R 00868790-008687d9 |
| +0x24 heading | R/W | H2 00868889-008688d9 |
| +0x34 ability handle | R | 00867f7b |
| +0x44 yaw rate | R/W | Y 0086880c-00868884, H2 0086889e |
| +0x48 pitch rate (rule B) | R/W | RB 0086893a-0086895e, RA 00868a0b, RB 0086899a-008689e2 |
| +0x490 smoothing | R/W | SM 00868ad6-00868aee |
| +0x494, +0x498 smoothing targets | R | SM 00868a86-00868abc |
| +0x4c1 horizontal-drag flag (halves) | R | R 00868786, RA 00868a28 |
| +0x55 | R | 00867f84, 008648890 |
| +0x74 latch | R/W | LT 0086812a-0086816b |

tun (attributes block, `tun+..`, decimal offset in brackets):

| field | R/W | where |
|---|---|---|
| +0x00 lStickDeadzone | R (caller) | 008646eb, 00864721 |
| +0x04 thrust | R | N2 0086846e-008684aa |
| +0x08 lateralThrust | R | N1 00868382 |
| +0x0c turboThrust | R | N2 0086847-008684a3 |
| +0x10 lift | R | N4 0086869e-008686a9 |
| +0x28 turnSpeed | R | Y 0086880c |
| +0x2c maxTurnSpeed | R | Y 00868847-0086884c |
| +0x34 turnLag | R | Y 008687f2-008687fe |
| +0x38 turnRestoreLag | R | Y 008687f9-008687fe |
| +0x58 maxPitch | R | RA 00868a1c, RB 00868986 |
| +0x5c minPitch | R | RA 00868a15, RB 00868978 |
| +0x60 maxRoll | R | R 00868779 |
| +0x68 pitchStrength | R | RA 00868a3a |
| +0x6c rollStrength | R | R 00868790 |
| +0x84 helicopter (bool) | R (caller) | 0086461d, 008648da |
| +0x88 helicopterTurboTurnFactor | R | N3 00868572 |
| +0xb0 robot (bool) | R | 008682bb, 008688f0 |

body (`body+..`): +0x34 (float gate, R), +0x38 (byte gate, R), +0xa0, +0xa4, +0xa8 (velocity, R/W
via set/add).

char / owner (`char+..`): +0x20 input object (R), +0x38 (R) then +0xa4 body handle (R).

stick and inputs: owner input vtable `+8(0x14)` (R, hold), `+0x28` (R, the helicopter stick, two
calls), `+0x30` (caller, jet squared stick).
