# Flight: camera target for a flyer, and the way out of flight (survey ticket, 2026-10-12)

Tags: `[game]` read in code (address given), `[data]` read from export/data, `[assumed]` our
reading, not checked against the executable or a recording. Offsets: `C+0x..` is the camera owner
or character, `T` the character transform (`owner+0x2c`), `B` the physics holder (`owner+0x38`),
`M` a mode object, `K` a mode's tuning block (`M+0x18`).

## Part A: the camera's target point for a flyer

### A1. What 0071a460 is (its callers) [game]

- `0071a460` `flying_vehicle_camera_target_bottom`, `__fastcall`: ECX = camera owner `C`, ESI =
  output (3 floats; EAX returns ESI).
- Its only caller in the census is `0071a2e0` `character_camera_target_point`. Per
  `docs/camera.md` (line 108), `0071a2e0` takes this path only when the vehicle model attributes
  `canFly` is set (field `+0xa5` at `00726710`). Bumblebee has `canFly=false`, so for him this
  function is never reached. `[data]` `[game]`
- The ledger already had it as read ("Complete audit"); this ticket re-read the disassembly.

### A2. Pseudo-code (from `work/raw_disasm.py 0x71a460 0x71a4c0`) [game]

```
target(C, out):
 1. K = C[0x104]                       ; the flying vehicle's character object
    if K == 0: return character_camera_target_point(C, out)     ; 0071a2e0, root position
 2. B = K[0x38]                        ; physics holder
    T = K[0x2c]                        ; character transform
 3. zBottom = B[0x134] - B[0xa4][0x54] * B[0x128]
        ; 0x71a46d..0x71a488: edx = [B+0xa4]; xmm0 = [B+0x134];
        ; xmm1 = [edx+0x54] * [B+0x128]; xmm0 -= xmm1
 4. rootZ = T[0x3c]
    if rootZ <= zBottom:               ; 0x71a494 comiss, 0x71a497 jbe
        out = (T[0x34], T[0x38], rootZ)
    else:
        out = (T[0x34], T[0x38], zBottom)
    return out
```

Result: `out.z = min(rootZ, zBottom)`, `out.xy` = the root's xy. The disassembly of step 4 is
checked (`0x71a499..0x71a4aa` writes `zBottom` into `out[2]` on the `>` path; the `<=` path at
`0x71a4b2` writes the root). `[game]`

- Orientation: none. The function reads no rotation: no matrix row at `T+0x10..0x2c` is used.
  The target is not a point ahead of the body. Roll and pitch do not move it. `[game]`
- Meaning of the bottom term: the lower of the root height and the body's bottom estimate.
  `[assumed]` that `B+0x134` is the holder's height and `[[B+0xa4]+0x54]*[B+0x128]` a
  half-extent along the body's up direction (the ledger calls the first two `holderZ` and
  `upZ`). The names are not in the export.
- Not checked: `[B+0xa4]+0x54` is read straight from `[B+0xa4]` (not `**`), so `[B+0xa4]` here is
  an object of its own, not the body `**(+0xa4)` the mode notes use. `[game]`

### A3. What the flyer's camera does with that target `[assumed]`

- The camera's look-at point is this target. The rest of the follow camera (driving behaviour,
  `docs/camera.md` steps 1-8, and the look-at height rule "target z at least the body's height")
  is not read for flight. `[assumed]` it is the same class: the attribute name is `drivingCamera`
  (A4), and the driving behaviour reads `vehicle's pitch` and `roll` (step 7 and "lean"). Whether
  a flyer's pitch or roll feeds the tilt term is NOT read: `[assumed]` the body's pitch, unread.
- Flight-specific gates in the camera (look-aside, `00916610`/`009162d0`) are unread.

### A4. Which camera data block a flyer uses [data]

- Starscream's attributes (`export/data/global_states.txt`, `characterAttributesStarscream`, from
  line ~68437): `drivingCamera: 1414596080`, `cameraDriveTargetHeightOffset: 0`,
  `cameraWeaponTargetHeightOffset: 0.7`.
- CRC-32 of the names (checked with `zlib.crc32`): `starscreamDriveNearCamera` = 1414596080.
  So Starscream's flight-form camera block is **`starscreamDriveNearCamera`**
  (`export/data/global_cameras.txt:4613`), not `starscreamDriveCamera` (line 4653).
- The brief's block `starscreamDriveCamera` (line 4653; hash 1570740567) has `tiltAmount 80`
  and `leanAmount 25`, as the brief said. Its `next` points to `starscreamDrivefarCamera`
  (-625835385). The near block is the one the attributes name.
- Bumblebee's `drivingCamera` is `bbeeDriveVeryNearCamera` (1687550972), not `bbeeDriveCamera`
  (`bbeeDriveCamera` = hash -1445234762). `[data]`. (Not used for flight: Bumblebee cannot fly.)

Values of `starscreamDriveNearCamera` (the block the attributes name), copied from the export:

| field | value | field | value |
|---|---|---|---|
| defaultFOV | 45 | maxDist | 14 |
| minDist | 12 | faceExtraDist | 0 |
| faceExtraDistGain | 0.5 | climbExtraDist | 0 |
| minElevation | -65 | defaultElev | 12 |
| maxElevation | 65 | userRotSpdZ | 160 |
| userRotSpdX | 100 | scaleRotSpdMin | 80 |
| scaleRotSpdMax | 120 | spongeHeight | 0.1 |
| minHeightGain | 0.6 | maxHeightGain | 0.9 |
| interpElevGain | 0.05 | interpAngGain | 0.075 |
| interpTargetHeightTime | 0.5 | minSpdAlignMove | 45 |
| maxSpdAlignMove | 65 | alignGain | 0.5 |
| alignDamp | 0.9 | maxSpdFOVScale | 145 |
| maxSpdDistAdd | 0 | alignFaceMaxSpeed | 100 |
| alignFaceMinSpeed | 0 | alignAutoMinSpeed | 0 |
| sideOffset | 0 | userRotSclMaxSpeed | 100 |
| maxFaceAngOffs | 0 | userRotGain | 2.6 |
| userRotDamp | 0.15 | heightOffset | 0 |
| tiltAmount | 80 | tiltGain | 0.1 |
| leanAmount | 25 | leanGain | 0.25 |

The `starscreamDriveCamera` block (line 4653) is identical except `maxDist 18` and `minDist 16`.
The `starscreamDrivefarCamera` block (line 4693) is identical except `maxDist 24`, `minDist 22`.

Bumblebee's own driving camera (`bbeeDriveCamera`, line 1501) has `maxDist 8`, `minDist 6`,
`minElevation -8`, `defaultElev 10`, `maxElevation 35`. `[data]`

## Part B: leaving flight

### B1. The exit states [data]

`export/data/global_states.txt`:

- `stateFlyExit` (line 540): clip `animName 1502413892` = hash `0x598d0444`, no name found.
  `overlayState false`, `canGoToStates [STATE_CAN_GO_TO_DEATH, STATE_CAN_GO_TO_STUN]`.
- `stateFlyExitA` (line 545): clip `1103944983` = `0x41ccdd17` (Trans_V2R_A per the notes).
- `stateFlyExitB` (line 550): clip `-658142035` = `0xd8c58cad` (Trans_V2R_B per the notes).
- `stateFlyEnterB` (line 535): clip `709657265` = `0x2a4c82b1`.

Which transition reaches which state (the connection lists in the dump,
`work/survey/flight/sc_conn.txt`, lines 1004-1020):

| list | members | used by |
|---|---|---|
| connectionListFlyRelease | connectionReleaseFly, connectionEnvInsideFlightBoundary, connectionEnvCanSwitchAvatar | stateMachineFly (line 7747) -> `stateFlyExit` (line 7764) |
| connectionListFlyReleaseAndAnimDone | the same 3, plus connectionWaitForAnimDone | stateMachineFlyAdvance (7882) -> `stateFlyExitB` (line 7940) |
| connectionListFlyReleaseAndSecondAnimBranch | the same 3, plus connectionWaitForSecondAnimBranch | (not traced) |
| (state at 0x6d64, connection list 0x9114) | not named | stateMachineFlyAdvance (line 7891) -> `stateFlyExitA` |

Discrepancy: `docs/flight-states.md` (step 6 of the StateFly section) says ReleaseFly goes to
`stateFlyExitA`. The dump shows the plain `stateFlyExit` (0x598d0444) on that list, in the
machine at line 7747. `stateFlyExitA` appears only in the Advance machine. `[data]` Which
machine a character uses is not settled (`docs/flight-states.md` says so). `[assumed]`
The notes' route is the Advance one.

Exit states' outgoing transitions (`docs/flight-states.md` step 6-8, read from the data, not
re-verified here): `stateFlyExitA` (anim done, or Fly hold again) -> `stateFlyEnterB`;
`stateFlyExitB` (0x1c held) -> `stateFlyEnterA`; (nothing held, `connectionNoInput`, clip
finished) -> `stateFall`. The plain `stateFlyExit`'s own outgoing transitions were not found.

### B2. What each exit state does [game]

Enter `0087c7e0` `state_fly_exit_enter` (slot 5 of `00b62bd0`):
1. `animation_set_start(FlySet, S+0x44, 0, 1)` on `O+0x10`: the exit clip.
2. If the current mode's class (slot 3 of the current mode) is not the companion `f081d468`,
   `FUN_00852150(O+0x14, 0x6da1d12d = JumpMode, 0)`: JumpMode becomes the mode.
3. `O+8`: `FUN_0071ab70(obj, 1)` (robot form, on foot) and `FUN_0071cfa0(obj, 0, 1, 0, 0)` (vehicle
   visibility off). `[game]`

Update `0087c910` `state_fly_exit_update` (slot 4):
1. If the current mode is JumpMode and `O+0x26c` (on ground) is set:
   `FUN_00852150(O+0x14, 0xf081d468 = drive companion, 0)`.
2. `state_transition_update(S, O)`.

Exit `00878140` (slot 7): overlay exit; does NOT clear the mode's `+0xe8` (unlike StateFly's exit
`0087bc70`). `[game]`

### B3. The FlyMode exit `00865b80` (flight_exit_handover) [game]

Read in the unpublished notes section 9; not re-read in full here. What the body keeps:

1. Save pre-exit linear velocity `v0 = body+0xa0..a8` and angular velocity `w0 = body+0xac..b4`.
2. Clamp each component of the linear velocity to `[-100000, 100000]` (`00865e1f`
   `DAT_00cae3ec = -100000`). Restore `w0`.
3. Body flags at `body+0x1d4`: clear 0x20, toggle 0x40, clear 0x200000, set 0x2000; zero
   `body+0x44, +0x48, +0x4c`.
4. Upright transform: `align_z_matrix` (`0056e7b0`, unread) on the owner's 3x4 at `owner+0x2c`.
5. `C_VEHICLE_STUNT` event; partial V2R animation if `char+0x140+0x390` is set.
6. The exit has no fall-or-long-jump choice. That comes from the next mode (B4).

Gravity is not touched here. The body keeps its velocity; the velocity is not zeroed.

### B4. The mode that takes over: JumpMode, then the companion [game]

Successor after `StateFlyExit` enter: JumpMode (`6da1d12d`). Its table (`docs/bumblebee-mechanics.md`
mode table): enter `008740e0`, update `00872fe0`, exit `00874850`.

JumpMode enter, with previous mode FlyMode (`docs/bumblebee-mechanics.md` JumpMode section):
- on ground (`O+0x26c`) and horizontal speed squared >= 25: `LONG_JUMP` (225a6ca8), no launch;
- FallSet playing, or not on ground: `aa185eeb` (fall), no launch. The exit enter runs while
  FlySet plays and the body is usually in the air, so the fall type is the likely one. `[assumed]`
- Launch and height rules apply only to the launching types.

JumpMode update `00872fe0`: gravity rescaled; slow motion while aiming; the stick turned to world
space and the body moved only for launching types; tracks fall distance for landing.
`[game]` (notes). Height is not set here for no-launch types.

When `O+0x26c` (on ground) is set, the state update (B2) starts the drive companion, mode
`f081d468` (table `00b62758`: enter `00863670`, update `00863070`, exit `00863760`). JumpMode's
exit `00874850` puts gravity back on the body first. `[game]` (notes).

### B5. The companion mode update `00863070` (drive_companion_update) [game]

Read from `work/raw_disasm.py 0x863070 0x863420` (the decompile dropped the speed register; the
speed is the frame slot `[esp+0xc]` at `0x863181`, multiplied in at `0x8634d2`).

Inputs: `M` = mode (ECX), `O = M[4]` owner, `B = O[0x38]`, body `= B[0xa4]` (its velocity at
`+0xa0..a8`), `T = O[0x2c]` transform, input source `I = O[0x20]` (vtable `+0x24` returns the stick
`(sx, sy)`), tuning `K = M[0x18]`, flag `F = byte M[0x1c]`, `dt` = `[ebp+8]`.

```
companion_update(M, dt):
 1. s0 = sqrt(vx^2 + vy^2) of body velocity (0 if no body)                   ; 0x863159
 2. if F != 0: decel = K[0x80], least = K[0x84], turn = K[0x88]               ; 0x863162-0x863190
    else:      decel = K[0x8c], least = K[0x90], turn = K[0x94]               ; 0x86319a, 0x8632a0
 3. s = s0
    if s0 > least:                                                           ; 0x863187 jbe
        s = max(0, s0 - decel * dt)                                          ; 0x8631a2..0x8631bc
 4. stick (sx, sy) from I; world w = (sx*T[0x10] + sy*T[0x1c],
                                      sx*T[0x14] + sy*T[0x20],
                                      sx*T[0x18] + sy*T[0x24])               ; 0x8631c2..0x863271
 5. if |w|^2 > 0.01:            ; constant 00b343a0 = 0.01                    ; 0x863271 comiss
        angle = turn * dt                                                    ; 0x8632a8, 0x8632c1
        rotate T about vertical by angle (FUN_0056dfe0; root copy FUN_0056e4e0,
            matrix copy FUN_006de7e0; axis and sign inside these, not read)   ; 0x8633a6
        character_set_affine(T)  (FUN_00545900)                               ; 0x8633de
 6. f = (T[0x1c], T[0x20]); len = sqrt(f.x^2 + f.y^2)                         ; re-read after step 5
    if len^2 > 1e-8: f = f / len                                              ; 00b345d8 = 1e-8
 7. v.xy = f * s;  v.z = body vz (unchanged)                                  ; 0x8634d2..
 8. clamp v.x, v.y, v.z each to [-100000, 100000]; write body+0xa0, +0xa4, +0xa8
```

Companion enter `00863670` (drive_companion_enter): `F = 1` if |w| > 0.05 (the 3D world stick,
through the same rows), else `F = 0`. `[game]`

What this does, in order, each step (dt in seconds):
- Horizontal speed: above `least` it falls by `decel * dt`, floored at 0. At or below `least`
  it is kept (no braking to zero). `[game]`
- Facing: with the stick past 0.01 (world), the facing turns by `turn * dt` (no sensitivity
  scale). Direction and sign of the turn are inside `FUN_0056dfe0`, not read. `[assumed]` toward
  the stick.
- Velocity: horizontal velocity is the facing times the speed. The body is moved only by this
  velocity change; no position is written here.
- Vertical: velocity z is not changed; gravity and contacts act in the body step. There is no
  height target and no landing test in this mode. Landing is detected by the state machine's
  on-ground flag `O+0x26c` (lags one update, `docs/state-machine.md`).
- Tuning block `K` is the mode's own `+0x18` block. Its name is not found in the export; the
  pairs (`+0x80/84/88`, `+0x8c/90/94`) are the decompile's field words. `[data]` unresolved.

Not read: companion exit `00863760`; `FUN_0056dfe0` (turn), `FUN_0056e4e0`, `FUN_006de7e0`;
the mode switch `FUN_00852150`; `state_transition_update`.

### B6. How a flyer lands as a robot `[game]`/`[assumed]`

No flight mode lands it. The chain is: exit enter (robot form, FlySet exit clip) -> JumpMode
(gravity back; fall type when airborne) -> on ground (`O+0x26c`): companion (horizontal coast,
gravity still in the body) -> exit clip done with nothing held: `stateFall` (`FallSet`, then
JumpMode, `docs/state-machine.md`) -> the ordinary ground states. The ground states were not read.

## Unread list
- `0071a2e0` details beyond its gate (the notes give them).
- `00865b80` steps 4 and the animation (`align_z_matrix` `0056e7b0`, the V2R partial set).
- `00863760` companion exit; `0056dfe0`, `0056e4e0` (turn helpers); `00852150` (mode switch).
- The drive camera's flight case: whether a flyer's pitch/roll feeds tilt; the look-aside variant.
- `stateFlyExit`'s own outgoing transitions; the connection list at `0x6d64` / `0x9114`.
- The ground states after `stateFall` (landing into run/idle).
- Which state machine a given flyer uses (the attribute stateMachineList mapping, not resolved).

## Flyer camera target: its inputs (second read, 2026-10-12)

Reader ticket; static reading only; nothing run; nothing written under the install. Tags as above.

1. What K, B and T are [game]. The gate and 0071a460 use different fields of the character C (the camera owner, ECX):
   - K = C+0x104 [game, 0071a460 at 0x71a460-0x71a468]. This is the STEP movement record (0x410 bytes, built by 00712bb0, stored at character+0x104; see character-step-class.md section 1 and character-height-fields.md section 7). It is NOT a "flying vehicle's character object" as the ledger first said.
   - B = K+0x38 [game, 0x71a46a]. The physics holder (owner+0x38 in the STEP notes). Its layout, from the pair-record notes: affine at B+0x108..0x12c, origin at B+0x12c/+0x130/+0x134.
   - B+0xa4 is the WRAPPER (camera.md "Holder a4 points to a WRAPPER, whose first word points to the physics body"). [game] Wrapper+0x54 is a half height for vehicle forms: "00853a10/006e81e0 set vehicle wrapper54=avatarHeight*.5" (camera.md, read earlier; the store itself was not re-read here). For robot forms the same slot is the ground-test gap (character-ground-rule.md), so the meaning depends on the form.
   - T = K+0x2c [game, 0x71a488], the transform (STEP's O2, character-step-class.md). Root = T+0x34, +0x38, +0x3c.
2. The three fields [game by offset, meaning assumed]:
   - B+0x134 = holder origin z, the holder's CENTRE z in world space [assumed, from the affine layout and camera.md "holder centreZ"].
   - [B+0xa4]+0x54 = wrapper half height along the body's up axis (for a vehicle form, avatarHeight*0.5) [assumed from camera.md].
   - B+0x128 = affine element at offset 0x20, the z of the body's up axis (m22 under either row or column convention) = upZ [game offset; the name "upZ" is from camera.md line 33].
   - zBottom = centreZ - halfHeight*upZ. The formula is checked in disassembly (0x71a473..0x71a48b: subss, mulss, comiss at 0x71a494, jbe at 0x71a497).
3. When 0071a460 is used [game]. Only from 0071a2e0 (character_camera_target_point), which is called by character_camera_message 0071a5c0 at 0071a61d. The camera asks for its target through message ca291e26 (camera.md "What the character hands the camera each update"). 0071a2e0 uses the flyer path only when all three hold (0x71a2ed..0x71a361):
   - (a) bit 10 of C+0x14c is set;
   - (b) character_in_vehicle_motion 0070ab90 returns true (ledger: "DriveMode or vehicle companion mode type"; its branch for FlyMode was not re-read [unread detail]);
   - (c) the vehicle object at C+0x108 is non-null and its byte at (*(C+0x108)+4)+0x140+0xa5 (canFly) is non-zero. [data] Starscream's attributes say canFly: true (flight.rs:1126 test); Bumblebee's say false (global_states.txt).
   Mode: the flyer's ordinary state (FlyMode) is a vehicle form in sim.rs (`is_vehicle` is Drive or Fly, sim.rs:1100). So the path is the flying form's, not a separate mode switch. [assumed] that (b) is true in FlyMode; not confirmed.
   - The result of 0071a460 then passes through FUN_00562970 (0x71a36a, its argument is a local copy; not read). The ESI output is the caller's out pointer.
4. The fallback 0071a2e0 in three lines [game]:
   - if the gate in 3 is false: out = (T.x, T.y, T.z) with T = *(C+0xc), i.e. the root at +0x34, +0x38, +0x3c (0x71a39f..0x71a3a2 and 0x71a4b2..0x71a4bf);
   - if C+0x104 is zero inside 0071a460 it calls 0071a2e0 again with the same ECX (0x71a4c6-0x71a4c7). That is a loop if the gate passes and K is null. [assumed] never reached: the STEP record is always built (00712bb0 from the constructor), so K is never null for a character. Not proven.
   - the flying path does not read C+0xc; it reads K+0x2c (STEP's O2). [assumed] that O2 and C+0xc hold the same root; not checked.
5. Values for a flying jet, from the data [data]:
   - Starscream's vehicle avatar (characterAttributesStarscream, global_states.txt:68437-68530): avatarDescArray[1] has height 0.7, radius 3.5, length 4.5, width 4, offsetZ 0.5. Robot avatar height 8.8.
   - So up_half = 0.35 and offsetZ = 0.5.
   - With the holder centre at root + upZ*offsetZ [assumed, the same rule game/src/camera.rs uses at line 120 and 165]: zBottom - root.z = upZ*(0.5 - 0.35) = 0.15*upZ. For an upright jet (upZ near 1) zBottom is ABOVE the root, so min(root z, zBottom) = root z. The bottom only lowers the target when the jet is inverted (upZ < 0). [assumed]; this is a consequence, not a recording.
6. Quantities in our sim for each input:
   - root: `Fly.pos` (flight.rs:290; sim.rs:2335 sets `s.pos = fly.pos`). Root = Fly.pos [assumed].
   - upZ: no field. Closest: `Fly.pitch` and `Fly.roll` (flight.rs ~369 `rotation()` = Rz(heading) Rx(pitch) Ry(roll)), so upZ = cos(pitch)*cos(roll). [game-port composition; the angle assignment is [assumed] per flight.rs's own comment].
   - half height: `Tuning.vehicle_height` (tuning.rs:797, avatar[vehicle] height; 0.7 for Starscream). Use vehicle_height*0.5.
   - holder centre offset: `Tuning.drive.body_offset_z` (tuning.rs:799, from the vehicle avatar's offsetZ; 0.5 for Starscream). Use centre z = root.z + upZ*body_offset_z.
   - none exists for: the holder's world origin as a separate value (the sim has only the root).
7. Call site in our code [assumed, for the wiring]:
   - game/src/camera.rs `target()` (line 107). It sets `position: pose.pos` (115) and `height` (116-125) from the vehicle formula (0070abe0), and `flying: false` (166). A flyer already takes the vehicle branch, because `state.is_vehicle()` includes Fly. So the change belongs there: when `state.is_flying()`, set `flying: true` and make the look-at point `flycam::target(pose.pos, holder_bottom(...))`. The look-at is `position + Z*(height + height_offset)` (core/src/camera.rs:647 and 193), so `height = target.z - pose.pos.z - height_offset`.
   - core/src/camera.rs already has `flying` branches (1642, 1680, 1685) that read `target.flying`, and `settings_for` (line 633) picks the drive camera for any `target.vehicle`, which matches `drivingCamera` in data (section A4).
   - The two rules differ: 0070abe0 (the game camera's current height) lifts the look-at ABOVE the root by up.z*(offsetZ - halfH), clamped at 0; 0071a460 only lowers it. Both are [game]. The port needs both, one for each case.
8. Still unknown:
   - whether B+0x134 is the world centre or relative to the root (the offset rule in 5 is [assumed]);
   - the store of wrapper+0x54 in 00853a10 / 006e81e0 (taken from camera.md, not re-read);
   - FUN_00562970 (called after both paths, unread);
   - the FlyMode branch of 0070ab90 (unread) and the meaning of C+0x14c bit 10 (unnamed);
   - whether the camera's target message reads the flyer's point or only the root (0071a5c0 was not read past its call at 0071a61d).

## 2026-10-20: 0071a460 read again (disassembly 0x71a460-0x71a4b2)

- Confirmed: zBottom = `[B+0x134] - [[B+0xa4]+0x54] * [B+0x128]` with B = K+0x38, K = C+0x104;
  out.z = min(root z, zBottom) where root z = `[T+0x3c]`, T = K+0x2c. `[game]`.
- The half-extent `[shape+0x54]` and `[B+0x128]` (upZ) are not named `[assumed]`. The boundary's P
  (`flight.rs` `boundary_contact`, the stand-in note there) is NOT ported: for an upright body it
  would lower the boundary's test by that half-extent, and the mapping of holder z to `pos` is not
  settled.
