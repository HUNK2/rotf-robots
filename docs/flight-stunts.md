# Flight stunts (barrel rolls and loops): port recipe, 2026-10-12

Ticket: work/survey/flight_brief.txt. Tags: `[game]` read in decompile or disassembly (address
given), `[data]` read in export/data, `[assumed]` not checked, `[unread]` not read.
Offsets: `mode+0x..` is the FlyMode object, `tun+0x..` (or `attr+0x..`) the character attribute
block, `body+0x..` the physics body, `char+0x..` the character.

## 0. Summary

- A stunt is started by `flight_stunt_trigger` (0086a590) from the stick, and it sets `mode+0x520`
  to 1 Down, 2 Left, 3 Right, 4 Up. `[game]`
- Kinds 2 and 3 (Left and Right, the barrel rolls) change code state: the roll `mode+0x20` decays
  each step, the attitude is rebuilt from the euler angles, and the velocity is set once to
  the nose direction at the normal speed cap. `[game]`
- Kinds 1 and 4 (Down and Up, the loops) change no physics state in code except the vertical
  control term for Down. The loop is therefore either clip-driven or carried by momentum; the
  code that would decide this is unread. `[game]` + open.
- Nothing in the read code ends a stunt by time. `mode+0x520 = 0` happens only in the FlyMode
  exit 00865b80. The exit trigger is not identified. Open.
- The kind does not go into `C_VEHICLE_STUNT`. It goes to the state machine as a message
  (FlyStunt*) that enters one of four StateFly states, each playing its own clip. `[game]` + `[data]`

## 1. What sets mode+0x4a1 (the stunt-enable byte)

1.1 Default: the FlyMode constructor 00863970 writes `mode+0x4a1 = 1` and `mode+0x4a2 = 1`. `[game]`
    (decompile of 00863970, lines near `*(param_2+0x4a1)=1`.)

1.2 Per step, the control-state mapper 008013e0 loops over the view elements. For the element whose
    controlstate hash is `0xd73818a4` (that is -0x28c7e75c signed, `[game]` decompile case
    `iVar3 == -0x28c7e75c`), it reads the element's `enabled` field (hash 0x50f9bb84) into `bVar5`
    and writes `*(obj+0x4a1) = bVar5`, where `obj = FUN_008521e0(0xfb9f3262)`. `[game]`
    - So mode+0x4a1 mirrors whether that control state is enabled. `[assumed]` that the object is
      the FlyMode (the field is in its layout; 00863970 writes it).
    - Which pad action this control state is: NOT SETTLED. `[unread]` The name hash `0xd73818a4`
      is not in export/data/names.tsv, and it is not the CRC-32 of "FlyingStuntMode"
      (zlib CRC-32 gives 0xd57c3257). The tuning value listed for FlyingStuntMode in
      export/data/global_tuning.txt (line 117) is -708689034 = 0xd5c24376, also not equal to
      0xd73818a4, so the list values are not names. The ticket brief's "button 0x1f" is `[assumed]`.
    - Also unresolved: the hash `0xfb9f3262` (not in names.tsv).

## 2. Trigger: 0086a590 flight_stunt_trigger(mode) [game]

Called from 00866a70 at the end, when `mode+0x520 == 0` and the function did not return earlier
(decompile of 00866a70; the brief said 00866ec1, the call site is at the end of the function).

2.1 Gates, in order; any failure returns 0 (no stunt, step continues):
    G1 `mode+0x520 == 0`.
    G2 byte `mode+0x55 == 0` (field unnamed).
    G3 byte `mode+0x4a1 != 0` (section 1).
    G4 control vector all zero: `mode+0xc == 0.0`, `mode+0x10 == 0.0`, `mode+0x14 == 0.0`
       (ucomiss checks, asm 0086a5be-0086a5ea).
    G5 `char = character_object_lookup()` non-null, and the current state object (FUN_007235d0)
       has name hash `0x085e3ca2` = stateFly (`[game]` decompile; `[assumed]` the state-object
       pointer is the first field, as in the other state checks). If the state is not stateFly: return 0.

2.2 Stick vector A: `A = FUN_00855910()` gives (A.x, A.y, 0). It is a virtual call on
    `[in_EAX+0x20]` vtable+0x30 (`[game]`; 00855910 is `[unread]` beyond that). `[assumed]`:
    the stick (lateral, pitch) as used by 00866ef0 (flight-helpers.md section 4).

2.3 Branch (`[game]`, decompile lines 19-55; `|A.x| = fVar6`, `|A.y| = fVar7`):

    IF |A.y| < |A.x|  (x dominant):
      X1  if A.x <= -tun+0xa8 and A.x != -tun+0xa8:
            mode+0x520 = 2 (FlyStuntLeft); message FlyStuntLeft (0xec31dce8); mode+0x524 = 1;
            return 1.
      X2  else if tun+0xa8 < A.x:
            mode+0x520 = 3 (FlyStuntRight); message FlyStuntRight (0x59e4a00f); mode+0x524 = 1;
            return 1.
      X3  else: nothing set, return 1.   (The step is consumed; no stunt.)

    ELSE  (y dominant):
      Y1  if A.y >= -tun+0xac (i.e. -0.9):
            if A.y <= tun+0xac: nothing set, return 1.              (dead zone; consumed)
            if mode+0x78 <= 10.0: return 0.                          (too slow for Down)
            mode+0x520 = 1 (FlyStuntDown); message FlyStuntDown (0x8aa9abbb);
            FUN_0070aaf0 (body) then FUN_0086eea0(body, 0); return 1.
      Y2  else (A.y < -tun+0xac):
            mode+0x520 = 4 (FlyStuntUp); message FlyStuntUp (0xd512ac49);
            FUN_0086eea0(body, 0); return 1.

    Notes: the message call is `character_message_handler(hash, 0xcc0d1c75)` in every case.
    `mode+0x78` is the speed used by the Down test and by section 3.3 `[assumed]` (unnamed).
    The Y1 speed test is the decompile constant 10.0; `[assumed]` the asm was not checked for it.

2.4 Consequence to check: 00866a70 returns the trigger's value (`cVar1 != 0`), and
    flight-update.md line 20 says a true return from 00866a70 ENDS the flight update step
    (`attr+0xb8` set). So X3 and Y1-dead-zone return 1 and would end the step every frame the
    stick is in the dead zone while `mode+0x520 == 0` and the control vector is zero. Normal flight
    gives the control vector zero (section 4.3). This would make normal flight skip the step.
    `[assumed]`, NOT settled: the flight-update read is one line and the caller's use of the
    value was not read. Check with a trace of normal flight before porting this rule.

## 3. Per-step behaviour while mode+0x520 != 0, in 00866a70 [game]

The function runs these blocks in this order every step (decompile of 00866a70):

3.0 Entry gates (before anything): if `mode+0x4c != 0`, or `tun+0x84` (helicopter) != 0, or
    `tun+0xb8` (canDoStunts) == 0: return 0. Asm 00866a7e-00866aa1. `[game]`

3.1 Block A, only for kind 2 or 3 (`mode+0x520 == 2 or 3`; asm 00866aa7-00866ab5):
    a. Roll decay: `dt = param_2` (the step, 0.032 s). If `10*dt >= 1.0` then `mode+0x20 = 0`;
       else `mode+0x20 = mode+0x20 - mode+0x20*dt*10`, i.e. `roll *= (1 - 10 dt)`.
       Constants: 10.0 at 00b34278, 1.0 at 00b38834 (asm 00866ae6-00866afa). `[game]`
    b. Euler rebuild: `euler_matrix(&mode+0x1c)` reads three floats: x = mode+0x1c (pitch),
       y = mode+0x20 (roll, the decayed value), z = mode+0x24 (yaw) (asm 00866b23-00866b2f,
       ESI = mode+0x1c). The matrix is Rz(z)*Rx(x)*Ry(y) (00570470, `[game]`).
       The result is written to the body with holder_set_affine and character_set_affine
       (calls at 00866b73-00866b9e). `[game]`. So the roll is applied about the body's Y axis
       (the forward axis in this game's space, `[assumed]` from the axis convention in the brief).
    c. Velocity set, once: only when `mode+0x524 != 0` (set by the trigger for kinds 2 and 3).
       Asm 00866bb2-00866c13:
         `cap = FUN_0086a840(mode)`  (flight_speed_cap_normal = maxSpeed * s, see section 6)
         `v = cap * F`, where F = the body's world matrix elements 3, 4, 5 (the forward row;
         asm 00866b40-00866b6d copies the matrix from esp+0x5c, so the source is elements 3..5 at
         esp+0x68..0x70; `[assumed]` the forward row, from flight-helpers.md section "body_world_matrix").
         `physics_body_set_velocity(v)`; then `mode+0x524 = 0`.
       So the nose direction at the normal cap, once, at the start of a barrel roll.

3.2 Block B, for every kind 1..4 (`mode+0x520 != 0`; decompile, the second `if`):
    a. `body+0x1d4 |= 0x40` (flag; FUN_0070aaf0 gives the body); `mode+0x54 = 1`.
    b. If `mode+0x514` is set (impact pending): the control vector is
         (1, 1, 0) if mode+0x9c > 0.1; (0, 0, 1) if mode+0x9c < 0.1 (nothing if equal);
       then a call pair (FUN_00724850, FUN_00711ec0: `[unread]`, effects or hit feedback);
       clear `mode+0x514`; RETURN TRUE. (Impact response, section 5.)
    c. Else the control vector is (1, 1, 1): `mode+0xc = mode+0x10 = mode+0x14 = 1.0`.
    d. Kind 1 only (`mode+0x520 == 1`), and `mode+0x78 <= 35.0` and `mode+0x78 != 35.0`:
         `mode+0x14 = mode+0x78 * (1/35)` (constant 0.028571429, asm-checked by the notes),
         and if that is < 0.01 then `mode+0x14 = 0.01` (constant 0x3c23d70a).
       So the Down stunt has control (1, 1, max(0.01, speed/35)) for speed up to 35.
       `[game]` decompile; the 35.0 compare and the velocity read-and-set (FUN_00556c00 then
       FUN_00557000, args unresolved) are `[unread]`. The read-then-set looks like a no-op
       `[assumed]`.
    e. FX and character calls (FUN_007255d0, character_object_lookup, FUN_00555e90, FUN_00562970):
       `[unread]`, not stunt-specific.
    f. RETURN TRUE. The caller then ends the flight step (flight-update.md line 20).

3.3 What each kind does per step, from 3.1 and 3.2:

| kind | mode+0x520 | name (message) | per-step code | control vector (mode+0xc,+0x10,+0x14) | one-shot | physics change in code |
|---|---|---|---|---|---|---|
| 1 | 1 | Down (FlyStuntDown) | 3.2 only | (1, 1, max(0.01, speed/35)) if speed <= 35 else (1,1,1) | Y1: FUN_0086eea0(body,0) at start | none beyond flags and the control term |
| 2 | 2 | Left (FlyStuntLeft) | 3.1 a, b; 3.2 | (1,1,1) | 3.1 c velocity = cap_n * forward, once | roll decays by (1 - 10 dt); attitude from euler |
| 3 | 3 | Right (FlyStuntRight) | 3.1 a, b; 3.2 | (1,1,1) | 3.1 c velocity = cap_n * forward, once | roll decays by (1 - 10 dt); attitude from euler |
| 4 | 4 | Up (FlyStuntUp) | 3.2 only | (1,1,1) | Y2: FUN_0086eea0(body,0) at start | none beyond flags |

3.4 Timing: the code has no timer for any kind. No field counts a stunt's length. The length
    comes from the clip (section 7), which is `[data]` only, and its length was not read. Open.

## 4. Body flags and the control vector (readers)

4.1 `FUN_0086eea0(body, flag)` (decompile 0086eea0, `[game]`): flag 1 sets body+0x1d4 bit 0x20,
    flag 0 clears it; then zeroes body+0xa4 fields +0x44, +0x48, +0x4c; then bit 0x40 is set when
    0x20 is clear and cleared when 0x20 is set. So flag 0 (Y1, Y2 starts) = bit 0x20 off, bit 0x40 on.
    The exit path calls it with 1 (00866a70 at the end of the mode+0x54 block).
    Meaning of bits 0x20 and 0x40: `[unread]`.

4.2 Control vector readers (`[game]`): 00866ef0 (stick steering) runs its full body only when the
    vector is all zero; FUN_008676c0, FUN_00867f20 the same; the trigger (section 2) needs zero.
    So stick steering is off during any stunt (vector is (1,1,1)), and the trigger is armed only
    when the vector is zero.

4.3 Open point from flight-common-step.md (its "OPEN CONTRADICTION"): that note says (1,1,1) is
    written in normal flight. The decompile of 00866a70 puts the (1,1,1) write INSIDE the
    `mode+0x520 != 0` block (3.2 c), so in normal flight the vector is not written by this function.
    `[game]` by decompile layout; the asm for 00866c50 `je 0x866d33` was not re-checked against
    this block boundary. Treat the vector as zero in normal flight, `[assumed]`.

## 5. Impact use of the stunt block (e)

- The `mode+0x514` (impact pending) branch is inside the stunt block (3.2 b). When pending, the
  control vector is set from `mode+0x9c` (closing value; writer NOT FOUND), effects run, and the
  step returns true with the stunt state unchanged. `[game]`
- The writer of `mode+0x514` was not found (grep of decomp for 0x514 gives many unrelated hits).
  `[unread]`.
- The effect calls FUN_00724850 and FUN_00711ec0 are `[unread]`.
- No damage is applied in the stunt code itself (no health or damage call in 00866a70 or 0086a590).

## 6. Attribute values (copied from docs/flight-attributes.md, checked vs export/data)

Columns in flight-attributes.md are Starscream | Blackout | Bumblebee. All three blocks give the
same stunt values `[data]`:

| attr | field | Starscream | Blackout | Bumblebee | used by |
|---|---|---|---|---|---|
| 0xa4 | stuntMoveButtonPressTime | 0.2 | 0.2 | 0.2 | NOT read in 0086a590 or 00866a70; the button-hold timer is `[unread]` (likely the input layer) |
| 0xa8 | stuntMoveStickThresholdX | 0.5 | 0.5 | 0.5 | trigger X1, X2 (|A.x| threshold, sign-flipped for Left) |
| 0xac | stuntMoveStickThresholdY | 0.9 | 0.9 | 0.9 | trigger Y1, Y2 (|A.y| threshold) |
| 0xb8 | canDoStunts | true | true | true | gate 3.0 |
| 0x84 | helicopter | false | true | false | gate 3.0 (Blackout is the helicopter) |

Other numbers used by the stunt code (code constants, not attributes):
- trigger speed gate 10.0 (mode+0x78) for Down (Y1); `[game]` decompile.
- Down control term: 1/35 and floor 0.01; speed limit 35.0 (3.2 d). `[game]`.
- roll decay factor 10.0 (00b34278), `1.0` (00b38834) (3.1 a). `[game]`
- impact threshold 0.1 on mode+0x9c (00b345ec, from flight-common-step.md). `[game]`

Speed caps used for the one-shot velocity (flight_speed_cap_normal, flight-common-step.md step 2):
cap_n = tun.maxSpeed * s; `[data]` maxSpeed Bumblebee 100, Blackout 40, Starscream 45. The
scale s is in flight-common-step.md step 1 (`[game]`).

## 7. The data: FlyStunt states and messages (export/data/global_states.txt) [data]

Machine `stateMachineFlyStunt` (global_states.txt line 8089). Four connections, all from the
character's messages, all with `checkInAir/OnGround/...` false, `minZ..maxZ` = -100000..100000,
`animBranchBegin/End` false, `msg` false. Each goes to a StateFly:

| priority | message (connectionMessage*) | msgUserDefined hash | output state | state animName (clip) | clip name `[data]` | hash resolves to |
|---|---|---|---|---|---|---|
| 20 | FlyStuntDown | -1968591941 = 0x8aa9abbb | stateFlyStuntDown | 1074506875 = 0x400bac7b | Vehicle_LoopBack | names.tsv |
| 21 | FlyStuntLeft | -332276504 = 0xec31dce8 | stateFlyStuntLeft | 488087824 = 0x1d17a110 | Vehicle_RollLeft | names.tsv |
| 22 | FlyStuntRight | 1508155407 = 0x59e4a00f | stateFlyStuntRight | -361497508 = 0xea73fc5c | Vehicle_RollRight | names.tsv |
| 23 | FlyStuntUp | -720196535 = 0xd512ac49 | stateFlyStuntUp | -843400724 = 0xcdbab9ec | Vehicle_LoopFull | names.tsv |

- Each state: `overlayState: false`, `canGoToStates: [STATE_CAN_GO_TO_DEATH, STATE_CAN_GO_TO_STUN]`
  (global_states.txt lines 555-575). So a stunt can be left to death or stun only, by the data.
- No connection leaves a stunt state back to flight in `stateMachineFlyStunt`. `[data]`. So the
  return from a stunt to flight is not in these states. It is `[unread]` (the anim end or the
  FlyMode exit path).
- The kind-to-message mapping equals the trigger's message hashes (section 2.3). `[game]` + `[data]`.
- The set name for these states is not in this block; notes say the flight states use FlySet
  (0x032e87a6), `[assumed]` the same here. The brief says the FlySet roll, loop and bank clips
  are used only with C_VEHICLE_STUNT. Names in names.tsv include animRuleVehicleStuntRollLeft
  (5bb811c2), animRuleVehicleStuntRollRight, animRuleListVehicleStuntRollRight (4db753c3),
  animRuleListVehicleStuntBankLeft (6df26899), animRuleListVehicleStuntLoopFull (6e334708),
  animRuleVehicleStuntLoopBack (06a41ca9) `[data]`. Their clip lengths: NOT READ.

## 8. C_VEHICLE_STUNT (0x53d0af4b, names.tsv line 29361)

8.1 Writers found `[game]`:
- FlyMode enter, 00864d60 (decompile lines 15-21): `FUN_007106f0(&C_VEHICLE_STUNT)` looks up
  the variable record; then `record+8 = 0`, `record[0] = 1` (byte).
- FlyMode exit, 00865b80 (decompile lines 195-201): the same pattern.
- The default-hash table 0070c900 has the hash (decompile line 10: `param_1 != 0x53d0af4b`); the
  branch that handles it is `[unread]`.
- 01193570 (FUN_0055bbb0 call, decompile line 106): `[unread]`.

8.2 Value: the kind is NOT written to C_VEHICLE_STUNT anywhere read. Enter and exit both write the
    same pattern (record[0] = 1). Whether that is a value or a dirty flag is `[unread]`; the
    notes did not settle it. So C_VEHICLE_STUNT per kind is UNKNOWN; per the code it is one
    value for the whole mode. The kind reaches the animation through the state machine (section 7).

8.3 Exit writes the mode back to 0: `mode+0x520 = 0` (00865b80, the last steps, `[game]`).

## 9. Exit, and what ends a stunt

- 00865b80 (flight exit handover; the unpublished notes section 9) sets `mode+0x520 = 0`,
  `mode+0x74 = 0`, `mode+0x4c0 = 0` (flight-common-step.md step 14), and sends C_VEHICLE_STUNT.
  `[game]`.
- Who calls the exit is `[unread]`. The data gives no exit connection from the stunt states
  (section 7). The arc of a stunt, from start to exit, is not established by read code.

## 10. Port recipe (what a Rust port needs, in order)

Constants: STEP = 0.032 s. Kinds: 0 none, 1 Down, 2 Left, 3 Right, 4 Up.

Start (trigger), each step, when mode.kind == 0 and not (mode.byte55) and mode.enable (4a1) and
control vector == (0,0,0) and state == stateFly:
  1. Read A = (x, y) stick.
  2. If |y| < |x|:
       if x <= -0.5 (and x != -0.5): kind = Left; msg FlyStuntLeft; rollPending = true; return consumed.
       elif x > 0.5: kind = Right; msg FlyStuntRight; rollPending = true; return consumed.
       else: return consumed (no stunt).
     else:
       if y >= -0.9:
          if y <= 0.9: return consumed.
          if speed <= 10: return NOT consumed.
          kind = Down; msg FlyStuntDown; bodyFlagsSet(bit20 off, bit40 on); return consumed.
       else: kind = Up; msg FlyStuntUp; bodyFlagsSet(bit20 off, bit40 on); return consumed.
  "consumed" means 00866a70 returns true. See section 2.4: this may be wrong; verify first.

Each step while kind != 0 (00866a70):
  3. If the entry gates fail (mode.4c set, helicopter, not canDoStunts): return not consumed.
  4. If kind is Left or Right:
       roll = (10*dt >= 1) ? 0 : roll * (1 - 10*dt)   (mode+0x20)
       attitude = Rz(yaw) * Rx(pitch) * Ry(roll)  (mode+0x24, mode+0x1c, mode+0x20); apply to body
       if rollPending: v = F * speedCapNormal (F = body forward row); set body velocity to v;
                       rollPending = false.
  5. Body flag 0x40 set.
  6. If impact pending (mode+0x514): control = (1,1,0) if closing > 0.1, (0,0,1) if < 0.1;
     effects (unread); clear pending; return consumed.
  7. Else control = (1,1,1); if kind == Down and speed <= 35 and speed != 35:
       control.z = max(0.01, speed / 35).
  8. Effects (unread). Return consumed.

The stunt ends only at the FlyMode exit (00865b80, kind = 0). The trigger for that is unread.

## 11. Assumptions (logged here per the rule; code tags match)

- [assumed] mode+0x4a1 is the enable of the control state 0xd73818a4, and that control is the
  stunt-mode button (brief: 0x1f). Settle by: reading the view/control-state table, or a trace
  with the button held.
- [assumed] the stick vector from 00855910 is the lateral and pitch stick (as in 00866ef0).
- [assumed] mode+0x78 is the forward speed (the notes use it as speed for the stall term).
- [assumed] the forward row of the world matrix is elements 3..5 (flight-helpers.md).
- [assumed] the roll axis is body Y (the forward axis).
- [assumed] the trigger's return 1 with no stunt ends the flight step (section 2.4).
- [assumed] the body velocity read-and-set in kind 1 is a no-op.
- [assumed] the control vector is zero in normal flight (section 4.3).
- [assumed] C_VEHICLE_STUNT's enter and exit writes are a dirty flag, not a value (section 8.2).

## 12. Not found / unread (for the next ticket)

- The writer of the impact pending flag mode+0x514 and of the closing value mode+0x9c. Not found.
- The effect calls FUN_00724850, FUN_00711ec0 (impact).
- What ends a stunt: the caller of FlyMode exit 00865b80 and the anim-end path of the stunt states.
- Clip lengths of Vehicle_LoopBack, Vehicle_RollLeft, Vehicle_RollRight, Vehicle_LoopFull (none read).
- Physics for kinds 1 and 4 (loops): no code in 00866a70 moves the body; the body velocity during a
  loop is what 00864320 leaves it (`[unread]`; the early exit at flight-update line 20 suggests thrust
  and steering are skipped while a stunt runs).
- FUN_007235d0 (current-state reader), FUN_00855910's source object (the stick), FUN_0070aaf0.
- FUN_0086eea0's meaning for the bits 0x20 and 0x40 of body+0x1d4.
- FUN_007106f0 (variable lookup) and the value semantics of the C_VEHICLE_STUNT record.
- FUN_008521e0(0xfb9f3262) object type and the hash 0xfb9f3262.
- The control-state hash 0xd73818a4 and its pad action.
- The use of stuntMoveButtonPressTime (attr+0xa4, 0.2 s) in the input layer.
- Whether any stunt damage applies (none found in the stunt code).

Ledger entries written this ticket (fn.py note): 00866a70, 0086a590, 00855910, 0086eea0, 008013e0,
00863970.
