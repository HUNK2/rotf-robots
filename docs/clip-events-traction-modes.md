# Clip events: traction in FlyMode, HoverMode and ClimbMode (2026-10-20)

Scope: what the control vector (mode+0xc, +0x10, +0x14) of the three modes that
`motion_control_set` 00854960 writes does, and what each mode's clips write into it.
Follows `docs/clip-events-traction-motion.md` sections 2 and 4. Read-only research: no
source file, no existing note changed. Tags: `[game]` read in code (address given),
`[data]` read in the packs or `global_states.txt`, `[trace]` a recording, `[assumed]`
not checked, `[unsettled]` could not be settled from what was read.

Method. Disassembly by `work\raw_disasm.py` on narrow ranges; decompile by `tools\fn.py`
only. Clip data by reading the pack files with the same loader as `work\anim_events.py`
(`tf2_str`, `tf2_lxb`), walking the whole Character tree for `ChangeTractionEvent`:
93 events in the 13 robot packs, which is the count the earlier note gave. Eight of them
(starscream) sit in `transformModel.vehicleProxy`, not in the character's anim sets, and
the packdump text carries them too. The scan scripts are in the session scratchpad, not
in the project.

## Summary

1. FlyMode (`00866900`): a traction vector with any component nonzero is only stored.
   The all-zero vector ENDS a stunt: heading re-taken from the nose, pitch and roll zeroed
   for the loops, body velocity set to the nose times the normal speed cap (not for
   helicopter or robot flags), and the stunt state cleared. Every event that ends a stunt
   therefore arrives as `[0,0,0]` at the clip's last key.
2. In FlyMode the vector is read as a GATE in three places: the stick steering `00866ef0`
   and `00867f20` run their full body only when all three components are exactly 0.0; the
   stunt trigger `0086a590` needs zero. Separately, the movement step `00852220` copies the
   current mode's vector to the body's holder control (for every mode, FlyMode included).
3. The stunt step `00866a70` writes the vector only while the stunt state is nonzero, so in
   normal flight the vector is zero (this settles the open point of
   `docs/flight-stunts.md` 4.3 and the contradiction in the unpublished notes).
4. Jet stunt clips (starscream and firestorm, FlySet): `Vehicle_RollLeft`, `Vehicle_RollRight`,
   `Vehicle_LoopFull`, `Vehicle_LoopBack` carry `[1,1,1]` at key 0 and `[0,0,0]` at the
   last key. Starscream also has a second copy of the four in its vehicle proxy (attachment
   7) with `[1,0,1]` for the two rolls. Jetfire and the flight-form robots have no stunt
   clips with traction.
5. HoverMode: its update (`0086f960`) never reads the vector. The vector reaches the body
   only through the holder control, so during the hover-entry clip `Trans_V2R_Hover` (`[0,0,1]`
   then `[0,0,0]`) the body's vertical velocity follows the clip's root while the horizontal
   velocity stays the hover's own. Hover_Idle has no traction, so the vector is then zero.
6. ClimbMode: state 6 is `PullUp` (`WallL/R_PullUp`). No climb clip in any of the 13 packs
   carries a traction event, so the `d7068c31` branch of `00854960` is never fed by data.
   The climb's own `(1,1,1)` is written directly by its enter, not by a traction event.
7. Port: `clip_traction` (sim.rs, line 4385) stores nothing for Fly, Hover or Climb. The
   recipes are in sections 2.7 and 3.5. The stunt end in `step_fly` (a 0.2 s stand-in) should be replaced
   by the zero event once it is stored.

## 1. Writers and readers shared by the three modes

- `motion_control_set` 00854960 (`[game]`): the current mode is `STEP+0x10`. At 854a04 it
  compares the class with `0xfb9f3262` (FlyMode) and, when the mode object is set, calls
  `00866900` with the vector pointer in EAX (854a14). The HoverMode and Climb branches
  are above it in the same function (`docs/clip-events-traction-motion.md` section 2).
- The tail of 00854960 (854ab9) and the movement step's own choice (008523c9) both call
  `holder_control_set` 006e8da0 with `push 1` (`[game]`, asm checked). The flag sets bit 0
  of holder `+0x1d4`, which is the bit that gates `body_control_step` 006db490
  (`[game]`). So a nonzero mode vector does reach the body.
- Movement step `00852220` (`[game]`, asm 8523a8-8523c9 and 852314-852369): the control is
  the owner's `+0x34c` value `(0.1,0.1,0)` when that flag is set; else the owner vector at
  `+0x3d8` when `+0x3d4` is set; else for MoveMode `(0.1,0.1,0)` when `+0x26c` is clear; else
  `mode+0xc..0x14` of the current mode (`mov eax,[edi+0x10]; add eax,0xc` at 852363). It is
  reached only through STEP's vtable slot 1 (the unpublished notes section 2), so it
  runs every character step. `[unsettled]` whether `+0x3d4` or `+0x34c` is set in flight:
  not read.
- Writers of the three vectors (`[game]`, asm checked, all write sites found by a scan of
  the modes' code ranges):
  - FlyMode: constructor 00863970 zeroes them (8639d0 `xorps`, writes 8639f0-8639fa).
    `flymode_enter` 00864d60 calls `motion_control_set` with `(0,0,1)` only when the
    character is grounded (`STEP+0x26c` nonzero; 8659f1-865a28). Traction events. The stunt
    step 00866a70 (only while `mode+0x520` is nonzero, section 2.3).
  - HoverMode: the constructor 0086f520 zeroes them (writes at 0086f570-7a). Enter 008713c0
    and exit 00871580 do not write them. Traction events only.
  - ClimbMode: enter 00857dd0 writes `(1,1,1)` directly at 857dd0 and again at 8583c3-8583db
    (the flag `+0x49` is set there too). Its update 00856d00 writes none. Traction events
    only from the climb clips, and there are none (section 4).

## 2. FlyMode

### 2.1 `00866900` (the FlyMode branch of `motion_control_set`), read in asm

The vector is copied to `mode+0xc..0x14` first (866900-866919). Then each component is
compared with 0.0 with the `ucomiss` / `lahf` / `test ah,0x44` / `jp` sequence (86691e-86694a).
The parity logic was worked out by hand: a component that is not exactly 0.0, or is NaN,
takes the exit (866a61). So the body runs only for the all-zero vector.

All-zero branch:

a. `mode+0x24 = FUN_00570610()` (86697b-866980). `[game]` That function takes the three
   floats at `ESI+0xc..0x14`, where `ESI = [owner+0x2c]+0x10`, normalises them and returns
   a heading: `atan2(-x, y)` of the normalised vector (`FUN_0046be80`, taken as atan2 from
   `docs/flight-update.md`), or from the other row when the vector is nearly vertical
   (|z| >= 0.9999). So after a stunt the yaw is re-taken from where the nose points.
   `[assumed]` FUN_0046be80 is atan2: its body not read.
b. If `mode+0x520` is 1 (Down) or 4 (Up): `mode+0x1c = mode+0x20 = 0` (8669a7-8669c1). Pitch
   and roll are cleared for the loops only. The roll stunts (2, 3) keep their decaying roll.
c. If `*(mode+0x18)+0x84` (helicopter) and `+0xb0` (robot) are both zero (8669c4-8669db):
   body world matrix (006de7e0) row 1 (the nose, `out[3..5]`) times `FUN_0086a840`
   (flight_speed_cap_normal, the maxSpeed cap times the speed scale) is set as the body's
   velocity through `physics_body_set_velocity` 00557000 (8669dd-866a4e). Starscream and
   firestorm and jetfire and the other jets have both flags zero (`docs/flight-attributes.md`
   row 62 and the helicopter row; Blackout has 0x84 set).
d. `mode+0x520 = 0` (866a57). The stunt is over, whatever its kind.

Non-zero branch: the store only. Nothing else in the function runs.

So the vector is a signal with two meanings in this function: nonzero means "a clip is
driving the body", zero means "the clip has finished, end the stunt". Nothing reads the
stored value besides the gates in 2.2 and the holder.

### 2.2 Who reads the FlyMode vector

- `00866ef0` (stick steering, the normal branch): `[game]` asm 866f00-866f2a. The three
  compares, then `jnp 0x866f44`: the full steering body (866f44 on) runs only when all
  three components are exactly zero. Otherwise only `FUN_008699d0` (the contact heading
  correction) is called and the function returns. The sense of the parity test was worked
  out by hand, as in the unpublished notes.
- `00867f20`: `[game]` asm 867f00-867f5b, the same pattern (`jnp 0x867f77` on zero; the
  call to `FUN_008699d0` at 867f69).
- `FUN_008676c0`: the notes (the unpublished notes section 5) say the same gate; its
  first lines were checked only for a `je` on a flag, not the three compares. `[assumed]`.
- `0086a590` (stunt trigger): needs all zero (`docs/flight-stunts.md` section 2; `[game]`
  asm 86a5be reads `mode+0xc`; the jump sense was not re-checked).
- The holder control in `00852220` (section 1): the vector is sent every step while
  FlyMode is current.

Consequence: while the vector is nonzero the flyer does not steer with the stick and cannot
start a stunt. While it is zero (the normal state) the stick steers.

### 2.3 Who writes the stunt vectors, and when (settles `flight-stunts.md` 4.3)

`00866a70` (`[game]`, asm 866a7e-866aa7, 866c1a, 866c45): entry gates `mode+0x4c`, `tun+0x84`
and `tun+0xb8`, then `mode+0x520`. At 866c1a the code is `cmp dword [ebp+0x520],0` and
`je 0x866e58`: with no stunt the function skips every write. The writes to `+0xc..0x14`
(866c6e-866ca1 for the impact-pending values, 866d3b-866d45 for `(1,1,1)`, 866dc6-866dcd for
the Down control term `max(0.01, speed/35)`) all lie after that test. So in normal flight
nothing writes the vector, and it stays at its value from the last traction event. This
settles the open point of `docs/flight-stunts.md` 4.3 and the unpublished notes:
the vector is zero in normal flight, and the stick body runs.

What the stunt step writes while a stunt runs (`docs/flight-stunts.md` section 3.3):
Left and Right `(1,1,1)`; Up `(1,1,1)`; Down `(1,1,max(0.01, speed/35))` when the speed is
35 or less, else `(1,1,1)`; impact pending: `(1,1,0)` or `(0,0,1)` from the closing value.
These are written every step while `mode+0x520` is nonzero. So at a stunt's start the clip's
key-0 `[1,1,1]` adds nothing to the FlyMode vector, which the stunt step already holds.

### 2.4 The stunt states and their clips, and the traction in each

`global_states.txt` (`[data]`): `stateFlyStuntLeft` animName `488087824` = `Vehicle_RollLeft`
(`1d17a110` in `names.tsv`); `stateFlyStuntRight` `-361497508` = `Vehicle_RollRight`
(`ea73fc5c`); `stateFlyStuntUp` `-843400724` = `Vehicle_LoopFull` (`cdbab9ec`);
`stateFlyStuntDown` `1074506875` = `Vehicle_LoopBack` (`400bac7b`). The stunt states have no
`motionModeType` `[data]`, so the mode they run in is the one already current; that FlyMode
is current during them is `[assumed]` (the stunt step is FlyMode's own, and it runs only when
FlyMode is the mode).
Kinds (`docs/flight-stunts.md`): 2 Left = RollLeft, 3 Right = RollRight, 4 Up = LoopFull,
1 Down = LoopBack.

Traction events of the flight clips (`[data]`, scan of the packs; `t` = key time in ticks,
length in ticks):

| pack, set | clip (length) | events |
|---|---|---|
| starscream FlySet | Vehicle_RollLeft (60) | `[1,1,1]` t0, `[0,0,0]` t60; root moves (-50.1, 0, 0) |
| starscream FlySet | Vehicle_RollRight (60) | `[1,1,1]` t0, `[0,0,0]` t60; root (+49.9, 0, 0) |
| starscream FlySet | Vehicle_LoopFull (80) | `[1,1,1]` t0, `[0,0,0]` t80; root (0, 3.14, 30.0) |
| starscream FlySet | Vehicle_LoopBack (80) | `[1,1,1]` t0, `[0,0,0]` t80; root (0, -3.14, -30.3) |
| starscream vehicleProxy | RollLeft (60) | `[1,0,1]` t0, `[0,0,0]` t60; end root (-24.9, 1.1, 5.5) |
| starscream vehicleProxy | RollRight (60) | `[1,0,1]` t0, `[0,0,0]` t60; end root (25.1, 1.1, 5.5) |
| starscream vehicleProxy | LoopFull (164) | `[1,1,1]` t0, `[0,0,0]` t164; end root (0.1, 76.7, 5.5) |
| starscream vehicleProxy | LoopBack (80) | `[1,1,1]` t0, `[0,0,0]` t80; end root (0.1, 46.1, 5.2) |
| firestorm FlySet | RollLeft, RollRight (60), LoopFull, LoopBack (80) | as starscream's FlySet |
| jetfire FlySet | no stunt clip (the set has only Vehicle_Idle and the banks) | none |
| blackout | no stunt clip; helicopter | none |
| optimusflight, megatronflight | Fly_* (124 ticks) clips; no stunt clips | none |

Jetfire, Optimus and Megatron flight have no stunt clips in their sets. An explicit clip
that a set lacks starts nothing (`anim_clip_find_in_set` 01193520, `docs/animation-rules.md`),
so their stunt states run without a clip `[assumed]`. Only starscream and firestorm stunts
have a clip with traction.

Starscream's vehicle proxy (`transformModel.vehicleProxy`, attachment 7) carries its own
copies of the four stunt clips. Which set of events reaches the Character is `[unsettled]`:
the handler acts on the registered Character (`character_object_lookup` 0070a3c0), so both
copies could write the same vector. The two copies disagree on the roll's `y` (`[1,1,1]` in
the FlySet clip, `[1,0,1]` in the proxy).

### 2.5 What the stunt clips do to the jet's motion

`[game]` for the rules, `[data]` for the clips, `[assumed]` for the units of the root.

1. Start. The trigger had already set `mode+0x520`. For a roll, the stunt step's first pass
   sets the velocity to the nose times the normal cap, once (block A, 3.1 c in
   `docs/flight-stunts.md`). From the next physics step the holder control is `(1,1,1)`, so
   the body's velocity on each axis moves to the clip's root velocity (`v += 1*(wanted - v)`,
   `wanted = (animated origin - origin) / dt`). The flight's thrust, drag, speed cap and
   gravity no longer set that axis. The stick steering is shut off (vector nonzero).
2. During the clip the jet follows the clip's root: the roll goes sideways about 50 units a
   second (RollLeft, root -50 over one second, units `[assumed]` metres), the loop climbs
   about 22 a second (LoopFull, root z +30 over 80 ticks). The body's own motion is gone on
   every axis for the starscream and firestorm FlySet clips. For the starscream proxy roll
   `[1,0,1]`, the forward axis keeps the body's own speed while x and z follow the clip.
3. Kind 1 (LoopBack, Down): the stunt step itself rewrites the vector's `z` each step
   (`max(0.01, speed/35)` when the speed is 35 or less). That can override the clip's `z=1`
   before the holder reads it. The order of the two writes in a step is `[unsettled]`.
4. End. The clip's last key sends `[0,0,0]`: the zero branch of 2.1 runs. The stunt is over,
   the heading is re-taken from the nose, loops are level, the velocity is the nose times the
   normal cap, and the holder control goes to zero: the jet flies on its own physics and
   steers with the stick again. The stunt has no timer in the game; its length is the clip's.
5. Order of events. The clip event and the stunt step can both write in one step; the
   game's order is `[unsettled]`. Either order leaves a zero vector after the clip's last
   key, because the zero branch sets `mode+0x520 = 0` and the stunt step then writes nothing.

### 2.6 The form-change clips in FlySet, and a risk

The robot-to-jet and jet-to-robot clips (`Trans_Robot2Vehicle`, `Trans_R2V_A`, `Trans_R2V_B`,
`Trans_V2R_A`, `Trans_V2R_B`) carry `[0,0,1]` at their start (`[data]`, all packs). Some end
with `[0,0,0]` (starscream `Trans_R2V_B` at t16; firestorm `Trans_R2V_A` at t38; jetfire
`Trans_R2V_B` at t6; blackout `Trans_R2V_A` at t36; `Trans_Robot2Vehicle` at t60). Others end
still at `[0,0,1]`: starscream `Trans_R2V_A` (t20), `Trans_V2R_A` (t30), `Trans_V2R_B`; jetfire
`Trans_R2V_A` (t30); blackout `Trans_V2R_A` (t28), `Trans_V2R_B`. The ground-punch chain
`Trans_V2R_GroundPunch` (starscream, jetfire) ends at `[1,1,1]` with no later key; in the
ground chain the zero comes from `GroundPunchFall` (`[0,0,0]`, its own set).

Which mode is current during these clips is `[unsettled]`: `stateFlyEnterA/B` and
`stateFlyExitA/B` have no `motionModeType` (`[data]`), so they run in whatever mode the
character already has. If FlyMode is current, a `[0,0,1]` that ends the clip stays in the
vector. Nothing found zeroes it: the enter (`00864d60`) writes only when grounded, and the
exit (`00865b80`, `[game]` decompile and asm) writes the owner's affine (`owner+0x2c`, saved and
restored around `align_z_matrix`) and clears `mode+0x520`, `+0x4c0` and `+0x74`, but not the
vector. The risk is then that the next flight starts with the stick steering shut off
and no stunt trigger, until a zero event arrives. That contradicts ordinary play, so one of
the reads is wrong, or something not read clears the vector. `[unsettled]`, the point to
settle first: a trace of the vector over a robot-to-jet, jet-to-robot, robot-to-jet cycle
(`trace\` or Cheat Engine, which the user runs), or a read of the callees of `00865b80`.

### 2.7 Port recipe for FlyMode (`core\src\sim.rs`)

- Store: a `control: Vec3` on the fly state (zero at construction, as `flymode_init`
  does), kept across mode changes (no reset was found on exit or enter).
- `clip_traction` (line 4385): add a `Mode::Fly` arm. If the vector is nonzero, store it and
  return. If it is zero: pitch and roll to zero for `Stunt::Down` and `Stunt::Up`; heading
  to the nose's yaw (`yaw_of` of the body's forward); if the flight has neither the
  helicopter nor the robot flag, velocity to the forward times `flight_speed_cap_normal`
  (the port's cap function in `flight.rs`, with `cap_scale`); then `fly.stunt = None`.
  The port's own stunt message is sent once when the stunt starts, not again here.
- `step_fly` (line 2844): (a) the stunt-over rule `plain && s.stunt_time > 0.2` (line 2906)
  is a stand-in and goes once the zero event ends the stunt. (b) Steering in `flight::Fly`
  runs its full body only when the stored vector is zero. (c) The stunt trigger in
  `flight.rs` runs only when the vector is zero. (d) After the flight's own velocity update
  and before the position is integrated, for each axis with a nonzero control:
  `v_i += c_i * (wanted_i - v_i)`, where `wanted` is the current action's root velocity
  (`clip_motion`'s `(root_at(tick) - root_at(last_tick)) / dt`, rotated by the body's
  orientation, as the game's `(animated - origin)/dt`). `[unsettled]` the orientation of the
  root's frame for a flyer. Skip the axis when a listed contact is present (`control_velocity`
  in `physics_manifold.rs` already has that gate).
- Clip selection: the stunt states use the clip of the stunt kind (section 2.4). The port's
  stunt clips for jetfire, Optimus and Megatron flight are absent; the port must not
  invent them.

## 3. HoverMode

### 3.1 The update does not read the vector

`00864320`'s counterpart for hover, `0086f960` (`docs/flight-hover.md` section 4), was
read in the decompile (`tools\fn.py 0086f960 --full`, 284 lines). Its reads of the mode
object are `mode+0x18` (the tuning block) and, through it, `T+0x10` (`hoverDeceleration`)
and `T+0x14` (`maxLeanDegrees`). No `mode+0xc..0x14` read appears. `[game]` The callees
(00871d50, 00870aa0) were not checked for it. So the hover's own motion does not depend on
the vector; only the holder control does.

### 3.2 What reads it: the holder control

HoverMode is the current mode in `stateHover` and `stateHoverEnter` (`[data]`,
`global_states.txt` lines 665 and 678). Each step the movement step copies its vector to the
holder control (section 1). `body_control_step` then pins each controlled axis to the clip's
root velocity. With no clip playing (Hover_Idle, no traction), the vector stays where the last
event put it, and that is zero after `Trans_V2R_Hover` ends.

### 3.3 Hover clips with traction (`[data]`)

- `Trans_V2R_Hover` (vehicle to hover, the hover-entry clip): `[0,0,1]` at t0 and `[0,0,0]`
  at the last key. Starscream (HoverSet, 60 ticks, root (-0.13, -0.08, -6.69)): `[0,0,1]`
  t0, `[0,0,0]` t58. Firestorm, jetfire: the same pair. Blackout: `[0,0,1]` t0, `[0,0,1]`
  t12, `[0,0,0]` t58.
- The same clip is a `shared` AnimFile in `Vehicle2RobotSet` for starscream, firestorm,
  jetfire and blackout (the event is the same data). Optimus and Megatron flight have their
  own 20-tick `Trans_V2R_Hover` in `Vehicle2RobotSet`: Megatron's `[0,0,1]` t0, `[0,0,0]` t18.
  Optimus flight's copy ends at `[0,0,1]` (t0 only), so its hover entry leaves the vector
  nonzero `[data]`, and its `Hover_Idle` would then pin the body's z to the last clip value
  `[unsettled]` (the risk of 2.6 applies to hover too).
- `Hover_Idle` (124 ticks, loop): no traction.
- `Trans_Hover2Fly` (hover to fly, `stateHoverExit`): no traction in any pack.
- Other: `Death_Explode_Fall` `[1,1,0]` and `StunStart` `[1,1,0]` are in other modes.

### 3.4 Effect on the motion

While `Trans_V2R_Hover` runs in starscream: the body's vertical velocity follows the clip's
root (the clip's root falls about 6.7 units over its one second), and
horizontal velocity stays the hover's own (the hover update sets it from the stick). After
the clip the vector is zero, the body keeps its own vertical velocity, and the hover's
decay (`hoverDeceleration` per update) applies. The enter zeroes gravity (`docs/flight-hover.md`
5, `[game]`), so this is the only vertical drive during the entry.

### 3.5 Port recipe for HoverMode (`step_hover` at sim.rs line 2934)

- Store: a `control: Vec3` on the hover state, zero at construction.
- `clip_traction` (line 4385): add a `Mode::Hover` arm that stores the vector. Nothing else
  happens (the hover update never reads it).
- `step_hover`: after the velocity update (the stick branch and the decay) and before
  `s.pos += s.vel * dt`, for each axis with a nonzero control: `v_i += c_i * (wanted_i - v_i)`,
  `wanted` = the clip's root velocity (as in 2.7). Apply only while a clip with root motion
  plays. `[unsettled]` what the game uses as the animated origin when no clip plays: with a
  nonzero stored vector and no clip, the port should not pull to the origin, so the stored
  vector must be ignored then.
- The port's `floor` and `push_out` stand-ins stay as they are (they are marked stand-in).

## 4. ClimbMode

- `mode+0x18` is the climb state (`docs/climbing.md`, ClimbMode section, and
  `core\src\climb.rs` `Phase`). State 6 is `PullUp` (`WallL_PullUp` / `WallR_PullUp`).
  The pack gives 60 ticks and root (0, 6.47, 10.93) `[data]`; `docs/climbing.md` says 56
  ticks, 7.45 up and 5.3 forward. The two disagree and were not reconciled `[unsettled]`.
- The `d7068c31` branch of `00854960` writes the vector only when the state is 6. `[game]`
- No ClimbSet clip in any of the 13 packs carries `ChangeTractionEvent` (`[data]`, scan of
  every ClimbSet clip; the whole-tree count for each pack has no climb set). So the branch is
  not fed by the data.
- The climb's own vector comes from its enter: `(1,1,1)` and the first-update flag `+0x49`
  (`00857dd0` at 857dd0 and 8583c3-8583db, `[game]`, asm checked). The movement is the
  wall clips' root; the port's `step_climb` (line 4605) sets the velocity from `climb::update`
  and does not use the vector.
- Port: nothing is needed for traction. If a climb clip ever carries a traction event, the
  `Phase::PullUp` arm of `clip_traction` would store it and `step_climb` would have to apply
  it as in 3.5. `[assumed]` no such clip in the roster.

## 5. Corrections to the earlier notes

- `docs/clip-events-traction-motion.md` section 2 says the mode writers are "Stand, Hover,
  ClimbMode with field value 6, and (unread) Fly": the Fly branch is now read (section 2.1)
  and the Hover and Climb branches are confirmed (section 1).
- The traction total in that note (93) matches the whole-tree count here. Starscream's 31
  include the 8 vehicle-proxy events.
- the unpublished notes "OPEN CONTRADICTION": resolved by the zero test of `0x866c1a`
  (section 2.3). The vector is zero in normal flight. The steering body does run.
- `docs/flight-stunts.md` 4.3 ("treat the vector as zero in normal flight, assumed"): now
  settled by asm (section 2.3).
- The port's `step_fly` stunt-over rule (0.2 s) is a stand-in for the zero event (section 2.7).

## 6. What could not be settled (in order of importance)

1. Whether a `[0,0,1]` or `[1,1,1]` that ends a V2R clip while FlyMode is current stays in the
   vector until the next flight (section 2.6). This is the one that would change the stick
   behaviour in ordinary play, so it comes first. Either the vector is reset by something not
   read, or the gate reading of `00866ef0` is wrong. `[unsettled]`
2. Which mode is current during the R2V and V2R clips, since the enter and exit states of
   FlyEnterA/B and ExitA/B name no mode (section 2.6).
3. Order within a frame: the clip's event, the stunt step (`00866a70`, from the FlyMode
   update), the movement step's choice (`00852220`) and `body_control_step` (section 2.5).
4. Whether `+0x3d4` or `+0x34c` is set in flight (section 1), which would replace the
   vector with another in the holder control.
5. The root motion's frame and units for a flyer: the clip's root displacement in world
   or body axes, and the units (metres assumed). Needed for the `wanted` term in 2.7 and 3.5.
6. Which of the two starscream stunt copies (FlySet `[1,1,1]`, vehicle proxy `[1,0,1]`)
   reaches the Character. The handler acts on the registered Character (section 2.4).
7. The body flag `0x20` (bit 5 of holder `+0x1d4`): `body_control_step` runs its
   translation part only when it is clear, and the rotation part is unported. Whether a stunt
   sets it (the stunt's start calls `0086eea0` with 0 for kinds 1 and 4, and the exit calls it
   with 1) is in `docs/flight-stunts.md` 4.1; the effect on the rotation is `[unsettled]`.
8. `FUN_008676c0`'s gate (section 2.2) was not checked by its three compares.
9. The trigger's jump sense in `0086a590` (section 2.2) was not checked.
10. `FUN_0046be80` taken as atan2 (section 2.1a).
11. Whether the stunt clip's length, not the stunt block, is what ends a stunt in the game
    when the zero event does not arrive (no evidence either way; `[assumed]` it does not
    happen).

## 7. Ledger changes

Ledger lines added or extended (`tools\fn.py note`, each keeps its earlier text and adds this
session's reading): new 00866900 (`flymode_traction_set`) and 00570610
(`flight_nose_heading`); extended 00866ef0, 00867f20, 00866a70, 00864d60, 00852220,
00857dd0, 00863970, 0086f960, 0086f520, 0086a590. No other file was changed.
