# Hover mode (modeHover): class, update, timer, states, per-character values, 2026-10-12

Scope: the HoverMode class (class id 70776cef, the "hover-in-air" motion mode), its
attribute block, its timer, its enter and exit, and which states and button start it.
Tags: `[game]` read in code (address given), `[data]` read in global_states.txt or the
attribute dump, `[assumed]` inferred, `[unread]` not read.

## 1. Class map

| role | address | notes |
|---|---|---|
| attribute block | attr+0x560 `[game]` | loader `FUN_0085bf90`, field offsets in docs/flight-attributes.md |
| builder (mode list) | `FUN_00851a40` `[game]` | `00852073 lea ecx,[ebp+0x560]`, push, `00852088 call 0x86f520`: the tuning pointer passed is attr+0x560, class id `c8a7f878` pushed at `0085207c` |
| constructor | `FUN_0086f520` `[game]` | vtable `PTR_FUN_00b627d0`; mode+0x04 owner; mode+0x18 = tuning pointer (attr+0x560); mode+0x3c = timer object (below) |
| enter (slot 5) | `FUN_0086f510` -> `FUN_008713c0` `[game]` | the slot-5 wrapper just calls the real enter |
| update (slot 2) | `FUN_0086f960` `[game]` | the per-frame step, dt in the 2nd argument; 259 lines |
| update, alt. branch | `FUN_00870aa0` `[game]` | runs instead of the main update when mode+0x55 is set (homing move, see 4) |
| exit (slot 6) | `FUN_00871580` `[game]` | |
| per-frame surface effect | `FUN_00871d50` `[game]` | prefix of the update; the hover's surface/water effect; only partly read |
| timer tick | `FUN_00735a00` `[game]` | method of the timer object at mode+0x3c |
| timer FX start | `FUN_00735bb0` `[game]` | starts effect `FX_hover` (name hash 5bc84938) |
| owner's mode lookup | `FUN_008521e0` `[game]` | called with class id 70776cef to get the owner's HoverMode |
| helper, body flags | `FUN_0086eea0` `[game]` | sets/clears body flag bits; already in the unpublished notes |

The class id 70776cef, the table 00b627d0 and the update/enter/exit addresses match the
table in docs/bumblebee-mechanics.md (HoverMode row) `[game]`. The HoverMode object is
created in `FUN_00851a40` with no flag test around the constructor call (only the
allocation's null check at `0085206f`) `[game]`; so every character's mode list has
one, and reaching it depends on the state machine (section 5).

## 2. Attribute block (attr+0x560, tuning pointer at mode+0x18)

From docs/flight-attributes.md (loader `FUN_0085bf90`):
+0x00 maxHoverTime, +0x04 moveAcceleration, +0x08 maxMoveSpeed, +0x0c hoverDoneParticlesPct,
+0x10 hoverDeceleration, +0x14 maxLeanDegrees (already radians), +0x18 leanStrength.
Offsets are off the tuning pointer (mode+0x18), the same as the base of the struct.

The ctor copies two of them into the timer object (section 3): `*(tuning+0x00)` into
timer+0x14 and `*(tuning+0x0c)` into timer+0x20 `[game]` (`0086f520`, the stores
`puVar5[5] = *param_2[6]` and `puVar5[8] = ((undefined4*)param_2[6])[3]`).

## 3. Timer object (mode+0x3c)

Allocated in the ctor (`FUN_005343a0`, size 5, vtable `PTR_FUN_00b5d650`) `[game]`.
Fields, off the timer register (ebx in `FUN_00735a00`):
- +0x08 sub-object (its refcount is touched on exit)
- +0x10 state: 1 = parked, 0 = running. Set to 1 at construction; the enter sets it to 0 `[game]`
- +0x14 limit = maxHoverTime (copied at construction) `[game]`
- +0x18 elapsed seconds, +0x1c seconds since the last reset `[game]`
- +0x20 hoverDoneParticlesPct (copied at construction). The reader was NOT found in the
  code read so far `[unread]`

Tick (`FUN_00735a00`, dt = its argument), as the disassembly reads it `[game]` (00735a06-00735b78):
```
if (timer_ptr == 0 or timer.state == 1): skip the timing (only the lower part runs)
else:
  elapsed += dt
  since_reset += dt
  if since_reset < 0.5: FUN_00735bb0(1,0)   # start FX_hover
  if limit >= 0:                            # limit = -1 means no timeout (data uses -1)
     mode = owner's HoverMode (FUN_008521e0, class 70776cef)
     if mode != 0:
        if mode+0x7c != 0 (over Water): a global counter at 00dc9678 (FUN_0088eb10) is read; unread detail
        if elapsed > limit and mode+0x55 == 0:
           send message b587f609 "HoverDone" to the owner's state machine  (FUN_007236c0, then vtable +4 of the timer)
```
[data] b587f609 is the msg of connectionMessageHoverDone, so the name is HoverDone.
The `limit < 0` test is `comiss` of 0 against +0x14 `[game]` (`00735ade`); -1 never times out.

Water: the per-frame prefix (`FUN_00871d50`, lines 8-11 of its decompile) sets mode+0x7c = 1
when the surface under the hover is `Water` (hash 3af23bde) and 0 otherwise `[game]`.

## 4. Update, as numbered pseudo-code (`FUN_0086f960`)

Symbols: `T` = tuning (attr+0x560, via mode+0x18); `B` = owner's body (owner+0x38 -> `**(+0xa4)`
... i.e. the body the code reads at `iVar2+0xa4`), with velocity at body+0xa0/+0xa4/+0xa8 `[game]`;
`S = (sx, sy)` = the 2-D stick from the input source (owner's +0x20, virtual +0x24) `[game]`;
`dt` = the update argument `[game]`. World axes: x, y horizontal, z up `[data]`.

1. `FUN_00871d50(mode)` runs first (surface effect; see section 3).
2. If mode+0x55 != 0: call `FUN_00870aa0(mode, dt)` (homing move, below) and return.
3. Read the body velocity `v = B.vel` (zero if there is no body) `[game]` (`0086f960` lines 26-40).
4. Clear body flag 0x40 in owner's +0x38 -> +0x1d4 (`&= ~0x40`) `[game]` (line 48).
5. If the vehicle special is active (`vehicle_special_active`, `00721b50`) and owner's
   flag `local_31c != 0`: set `B.vel = (0,0,0)` and skip to step 14 `[game]` (lines 49-53).
6. Stick `S` = input +0x24 call. Build the frame from up = (0,0,1) with `align_z_matrix`
   (`0056e7b0`) and take the world stick direction `d = M * (sx, sy, 0)` `[game]` (lines 54-64).
   The exact frame columns are not checked `[assumed]`: "a horizontal frame whose x,y are the
   stick's x,y in the heading".
7. If mode+0x44 != 0 (a contact flag): remove the part of `d` that points into the stored normal
   (mode+0x48..0x50) when it is negative, then clear mode+0x44 `[game]` (lines 65-74).
8. Let `L = |d|`.
   - **If L <= 0.01 (no stick)** `[game]` (lines 77-96):
     - if the owner is not AI: `B.vel = hoverDeceleration * v` for all three components, z included.
       (`decel = T+0x10`; `v' = decel * v`). Then `FUN_00557150` (lux::Debris, unread).
     - if AI: read the velocity, reset it (`physics_body_set_velocity`) and do the same.
   - **Else** `[game]` (lines 97-129):
     - `B.vel += d * (T+0x04 moveAcceleration) * dt`  (`body_add_velocity`, line 99-103).
     - speed `s = |B.vel|`; if `s > T+0x08 (maxMoveSpeed)`: scale `B.vel` to length `maxMoveSpeed`
       (lines 111-127).
9. **AI only** (lines 131-174, `controller_is_ai`): if the input action 0x28 is held, `sign = +1`;
   else if action 0x27 is held, `sign = -1`; else the vertical velocity is only read and reset
   (lines 133-143). The added vertical part is `T+0x04 * sign * dt`; then the same speed clamp
   to `T+0x08` (lines 150-173). Action codes are not named `[unread]`.
   **The player (not AI) gets no vertical input in this update** `[game]`: the block is under
   `controller_is_ai != 0` (line 131). Where a player rises or sinks is `[unread]`.
10. Owner's flag `local_31c` set: the heading. For a non-AI player the aim point is the camera
    target minus the character position, normalised in x,y (`0071a2e0`) and the heading is
    `atan2(-x, y)` (`FUN_0046be80`) `[game]` (lines 176-212). In StateWeapon it uses owner
    +0x260/+0x264; otherwise the global state vector (`0059b370`).
11. Constant-angle spin: a quaternion/matrix built from `DAT_00c02990..` (lines 213-224, unread).
12. Lean (AI only, lines 225-249) `[game]`:
    `target = T+0x14 (maxLeanDegrees, rad) * sx`; if `0 < T+0x18 (leanStrength) * dt < 1`:
    `lean += (target - lean) * leanStrength * dt`, else `lean = target`; store to mode+0x40;
    the roll matrix `FUN_0056fbd0(lean)` is built and copied into the root affine.
    For a player the lean is never set in this update (mode+0x40 is 0 from enter) `[game]`.
13. Turn: `rotate_z_matrix(atan2(-x,y))`, `compose_affine`, then `FUN_0056dfe0(..., dt * 2*pi)`
    on the owner's affine (+0x2c) `[game]` (lines 250-253). Meaning of the last rotation `[unread]`.
14. `character_set_affine` (`00545900`) and `holder_set_affine` (`006dbd30`) push the affine
    to the holder and body `[game]` (lines 254-257).

Gravity: this update adds no gravity term and never reads it `[game]`. Vertical velocity
only decays (step 8 scales z by hoverDeceleration when there is no stick). Whether the body
integrator applies gravity during hover depends on body flag 0x200000, which enter sets and
exit clears `[game]`; the integrator's reading of that bit is `[unread]`. So the port must
decide: `[assumed]` no gravity while hovering, vertical velocity decays per step, and the
player has no vertical control in this update.

Per-step decay: `hoverDeceleration` is applied once per update call, not scaled by dt
`[game]` (line 80 has no dt). The update runs once per game step `[assumed]`.

### 4b. Homing variant (`FUN_00870aa0`, when mode+0x55 != 0)
`[game]` The mode+0x55 flag is set elsewhere (writer `[unread]`). The variant:
- `d = target(mode+0x70,0x74,0x78) - owner position (affine +0x34..+0x3c)`; `k = clamp(|d|*0.1, 0, 1)`
  (lines 4-19).
- body velocity += unit(target - pos) * `T+0x04` (moveAcceleration) * `k` * dt, written only
  when the body's flag byte at +0x38 is 0 and its float at +0x34 is 0 (lines 35-38) `[game]`.
- the body speed is clamped to `T+0x08` (maxMoveSpeed) by `physics_body_set_velocity` (lines 60-63);
  body flag 0x40 cleared (line 67) `[game]`.
- the heading is turned toward the target (atan2 via `FUN_0046be80`, lines 76-106), and the
  affine at owner+0x2c gets flag `|=0x200` at +0x60 (line 136) `[game]`.

## 5. How it starts and ends

States (`export/data/global_states.txt`) `[data]`:
- `stateHover` (line 665): anim set HoverSet, anim `Hover_Idle`, motionModeType `#70776cef`
  (= HoverMode), robotForm true.
- `stateHoverEnter` (678): anim set Vehicle2RobotSet, anim `Trans_V2R_Hover`, motionModeType HoverMode.
- `stateHoverExit` (691): anim set FlySet, anim `Trans_Hover2Fly`, motionModeType `#fb9f3262` (FlyMode).
- `stateHoverFall` (704): falls back to the ground.
- `stateMachineHover` (9489): the hover machine. Its exits:
  - `connectionMessageHoverDone` (msg `b587f609`, priority 1) to a state (dump address 0x6d54).
  - `connectionMessageHoverForced` (msg `c51a4425`, = `ConnectionMessageHoverForced`, priority 3).
    Sent/checked by `FUN_00717a50` at `00717b15` (character death / restart checks, docs/animation-rules.md) `[game]`.
  - `connectionListFlyToWeaponHover`, `connectionListFlyToHover`, `connectionListHoverToFall`,
    `connectionListWeaponHoverFall`, and `connectionWeaponModeAttack` (exclude/include buttons).
- Entry from the flying machine: `connectionFlyToHover` (2179) `[data]`: ControlDetect,
  `checkCanAdvTransform: true`, stage includeButtons `[#00000021]`, excludeButtons `[#0000001c]`.
  Button code 0x21 is named `AttackTransformHover` and 0x1c `FlyingMode` in docs/flight-states.md.
  The pad face button is `[unread]`: the bindings table was not checked against 0x21.
- `connectionFlyToHoverHumanoid` (2214) is the humanoid variant (`stateMachineHoverHumanoid`).
- `connectionFlytoWeaponHover` (2319), `connectionHoverToFall` (2369), `connectionWeaponHoverFall` (3555).
- Enter state is `stateHoverEnter`; its anim is the V2R transition; the timer starts on enter (section 3).
- Exit: `stateHoverExit` (to FlySet) when the state machine goes back to flight; `FUN_00871580`
  is the mode's exit `[game]`. It re-uprights the character unless the next mode is DashMode
  (`517c0a3b`) or FlyMode (`fb9f3262`) `[game]`.

Weapon hover (port, 2026-10-20): `stateWeaponModeHover` (StateWeapon, HoverMode [data 1094]) runs
`Mode::Hover` with `State::hover_weapon` (`State::weapon_hover()`), the weapon set's rules and layers on
top, heading on the aim; entered from stateFly on `connectionFlytoWeaponHover` (include 0x18, exclude
0x1c), through `stateHoverEnter`. The game shell passes the left trigger as 0x18 in flight. Test
`the_weapon_hover_hangs_in_the_air_with_the_gun`; the unpublished notes "The flyers' weapon hover".

Button: `[data]` the entry is the detector with include 0x21, exclude 0x1c. Notes say a flight
release goes to `stateFall`, not hover (`docs/flight-states.md` line 243) `[data]`.

Enter (`FUN_008713c0`) `[game]`: I_IS_HOVERING (hash b3bcabff) = 1 in the animation variables;
body flag 0x200000 set; if the timer is parked (state 1): reset elapsed, since_reset and state
to 0 and call `FUN_00735bb0(1,0)` (FX_hover); owner's camera target saved at mode+0x24..0x2c;
mode+0x40 (lean) = 0.

Exit (`FUN_00871580`) `[game]`: I_IS_HOVERING = 0; if the timer exists, release it (vtable +4)
and call `FUN_00871a30`; then if the next mode is not DashMode, the owner is re-uprighted
(`align_z_matrix`, `character_set_affine`, `holder_set_affine`); mode+0x44 = 0; body flags
0x40 set and 0x200000 cleared; `FUN_008718c0` clears the effect handles at owner+0x160..0x168.

## 6. Attribute values per character `[data]`

`maxHoverTime` by block (`export/data/global_states.txt`, the `characterAttributes*` blocks
and `AI_` / `MP_` copies):

- **20**: characterAttributesStarscream (68725), characterAttributesBlackout (51416),
  characterAttributesFirestorm (59320), characterAttributesMilitaryHelicopter (62363),
  characterAttributesMilitaryPlane (63710), characterAttributesNESTHelicopter (64913),
  characterAttributesPoliceHelicopter (67210); MP Jetfire (73344).
- **-1 (no timeout)**: AI_ Blackout, AI_BlackoutEscort, AI_Breakaway, AI_Jetfire,
  AI_MegatronFlight, AI_Starscream, AI_TheFallen; characterAttributes AAerScout1/2, AAerWarrior,
  DSkrSniper, DSkrWarrior1/2, Jetfire (60855), MegatronFlight (61985), OptimusFlight (66446),
  TheFallen (69484).
- **10**: characterAttributesBumblebee (51796), Audi, Human, HumanVIP, Ironhide, LongHaul,
  Megatron, Optimus, Ratchet, Devastator, Dummy, the DCns/DCom/DStn/APrt/AOmn sets, and the
  ground vehicles (SemiTruck, PoliceCar, Wheelbot ...). Ground blocks have these values too but
  are not reached from a flight state `[assumed]`.
- **MP**: 8 (AAerScoutMP, Blackout, DSkrWarriorMP, Firestorm), 10 (APrtScoutMP, Audi, Bumblebee,
  Ironhide), 20 (MP Jetfire).

Full blocks copied from the data `[data]`:

| character (block) | maxHoverTime | maxMoveSpeed | moveAcceleration | hoverDoneParticlesPct | hoverDeceleration | maxLeanDegrees | leanStrength |
|---|---|---|---|---|---|---|---|
| Starscream (characterAttributesStarscream, 68725) | 20 | 10 | 100 | 0.9 | 0.95 | 7 | 5 |
| Blackout (characterAttributesBlackout, 51416) | 20 | 10 | 80 | 0.9 | 0.95 | 7 | 5 |
| Bumblebee (characterAttributesBumblebee, 51796) | 10 | 10 | 50 | 0.9 | 0.95 | 7 | 5 |
| Human (characterAttributesHuman, 59690) | 10 | 10 | 50 | 0.9 | 0.95 | 7 | 5 |

## 7. Unread

- Timer +0x20 (hoverDoneParticlesPct): no reader found yet `[unread]`.
- `FUN_00735bb0` body: READ 2026-10-20 (`docs/particles.md` "The senders"): (1, x) in HoverMode
  Triggers `FX_hover`; otherwise stops `FX_hover` and Triggers (x = 0) or stops (x = 1) `FX_hover_sput`.
  The timer's slot 1 (`007359e0`, park) calls it with (0, 1). Ported in `fxstate::kept`.
- Water counter at 00dc9678 (`FUN_0088eb10`): `[unread]`.
- Writer of mode+0x55 (selects the homing variant) and of mode+0x70..0x78 (its target): `[unread]`.
- Body flag 0x200000: its meaning and the integrator's gravity reading: `[unread]`.
- Player vertical control: not in the update (section 4 step 9); its source is `[unread]`.
- Action codes 0x27, 0x28 (AI vertical) and the pad button for 0x21 (hover entry): `[unread]`.
- Stick frame columns in `align_z_matrix` (`0056e7b0`): not checked `[assumed]`.
- State dump objects (0x6d54, 0x6560, 0x6d64, 0x6ff4): outputs not resolved.
- `FUN_00557150`, `FUN_0056dfe0`, `FUN_0056fbd0`: called, not read.
- Per-frame surface effect `FUN_00871d50`: read only to line 90.
- Ground characters with maxHoverTime 10: whether any ground state reaches HoverMode `[unread]`.
