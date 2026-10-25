# Flying forms: the StateFly and StateFlyExit control states (survey ticket, 2026-10-12)

Scope: the control states that take a flying character (Starscream as the main case, the
connections of `characterAttributesStarscream`) into and out of flight. Compared with
StateDrive (Bumblebee's car) read in `docs/state-machine.md`. The mode functions (FlyMode
update 00864320, exit 00865b80) are in `docs/flight-update.md` and are not repeated here.

Tags: `[game]` read in code (address given), `[data]` read from `export/data/global_states.txt`
or the connection dump (`work/statemachine_dump.py <install>/bnxglobal.str <attr> all`, output
kept in `work/survey/flight/sc_all.txt`, `sc_conn.txt`, `bb_all.txt`, `bb_conn.txt`),
`[assumed]` our reading, not checked.

Notation: state object `S`, owner (the character) `O`. `O+0x14` is the mode list (current mode
at `O+0x14+0x10`, list head `+0x18`), `O+0x20` the state pointer the enter test uses (see
"StateFly" 5), `O+0x26c` the on-ground byte, `O+0x10` the animation controller. Mode object
`M`: `M+0xe8` a "vehicle mode active" byte, `M+0x28` a ref handle.

## Button ids used here `[assumed]`

Button numbers are the enum of `docs/state-machine.md` "The data": 0x15 VehicleMode, 0x18
WeaponMode, 0x1a-0x20 the seven Flying* in the binding order of `export/data/global_tuning.txt`
(FlyingAfterburn 0x1a, FlyingBrake 0x1b, FlyingMode 0x1c, ...), 0x21 AttackTransformHover, 0x22
Jump, 0x23 Punch, 0x24 Slam. The binding list order was checked (FlyingMode is the third of the
Flying* entries). The connections are consistent with the names: the WeaponMode connection
excludes 0x18, the air punch uses 0x23, the hover uses 0x21. Not checked against the bindings
file itself. `[assumed]`

## The class IDs and vtables

| object | class id | vtable | slots used |
|---|---|---|---|
| StateFly | f1285ef4 (`FUN_0087c1e0`, slot 12) | 00b62b8c | 0 dtor 0087c2b0; 1 load 0087c780; 4 update 0087c300; 5 enter 0087c320; 7 exit 0087bc70; 8 may-leave 008783e0 (returns 1); 9 done 00929ff0 (returns 0); 11 may-enter 0087c550 (returns 0 while DISABLE_VEHICLE runs; the same function as StateDrive's) |
| StateFlyExit | ccfac55a (`FUN_0087c660`, slot 12) | 00b62bd0 | 0 dtor 0087c730; 1 load 0087c780; 4 update 0087c910; 5 enter 0087c7e0; 7 exit 00878140; 8 may-leave 008783e0; 9 done 00929ff0; 11 may-enter 0076c530 (returns 1) |
| StateDrive (for comparison) | 2f472086 (`0087b7b0`) | 00b62b0c | 4 update 0087b8d0; 5 enter 0087ba10; 7 exit 0087bc70; 11 may-enter 0087c550 |

Vtables read with `tools/pe_read.py vtable`. `[game]`

Factories and registrars (`[game]`): the StateFly factory `FUN_0087c1f0` takes a pool slot of
size 9 (`FUN_005343a0`), sets the vtable to 00b62b8c, `+0x44 = 0`, copies the animation set
of the data object (`+8`) and the name hash (`crc32_name`) into `+4`, then calls slot 1 (the
loader) with the data object. The orphan copy `0087c270` (no pseudocode in the export) does the
same and copies `+0x44` from its source. The registrar `FUN_00adf4e0` stores the factory at
`_DAT_00d42574`. StateFlyExit has the same shape: factory `FUN_0087c670`, vtable 00b62bd0,
registrar `FUN_00adf540`.

The loader (slot 1, `FUN_0087c780`, shared by all three): `S+0x44` = the `animName` field
(name hash 0x33e6abbe in the data object) as a scalar, or 0 when the field is absent. `[game]`
So `S+0x44` is the clip that the state's enter starts. `[data]` confirms the data `animName`
values (see "The states in the data").

## StateFly

Enter `FUN_0087c320` (554 bytes; ECX = S, stack argument = O; disassembly `0087c320`-`0087c520`
checked for the constants and the call order). `[game]`

1. `FUN_00878070(O)`: takes a reference on O's handle at `O+0xc` (the same routine StateDrive's
   enter calls first).
2. `animation_set_start(FlySet 0x032e87a6, S+0x44, 0, 1)` on the animation controller `O+0x10`.
   The clip is `S+0x44`: for `stateFly` it is 0, which `animation_set_start` treats as "walk the
   entry rules" (ledger note for `FUN_0084ddb0`). For EnterA/EnterB it is the Trans clip.
3. `FUN_00852150(O+0x14, 0xfb9f3262 = FlyMode, 0)`: makes FlyMode the current motion mode. If it
   is already current, nothing happens (the routine returns 1 at once). Otherwise the old mode
   gets slot 6 (exit), the new one gets slot 5 (enter, which is `FUN_00863950` then
   `flymode_enter` 00864d60, `docs/flight-update.md`). This is the ONLY place this state
   changes the mode, and it runs in all three flight-entry states, not only `stateFly`.
4. Refcount copy of `O+8`, then if the object that comes out is not null,
   `FUN_0071ab70(obj, 3)`: the control mode "3" (the car / vehicle value that StateDrive also
   uses; see `docs/state-machine.md` "robotForm"). Ledger note for `FUN_0071ab70`: it reads only
   the `canFly` attribute as a gate; the rest of it is not read. So whether the 3 does anything
   for a character with `canFly` false is `[game]` read as: the gate is canFly. `[game]`
5. Only when `S+8 == 0x085e3ca2` ("stateFly", the hub; `cmp [S+8]` at `0087c3ed`): the
   visibility block (`0087c3f4`-`0087c4c6`): if the component from `O+0xc` through `FUN_00544db0`
   is not null, its `+0x100` object is not null, that object's slot 1 returns something other
   than 2, its `+0x140 -> +0x390` byte is 0, and the class id of the state at `O+0x20` (slot 12)
   is not StateFly `f1285ef4`, then `FUN_0071cfa0(component, 1, 0, V_Idle 0x8c04d764, 0)`.
   So the vehicle model is switched on with the V_Idle clip when the hub is entered from another
   state. `[game]` The state at `O+0x20` is read as "the state the character came from" `[assumed]`
   (the StateDrive enter tests the same pointer against StateDrive's id).
6. Still in the hub block: if bit 17 (`0x20000`) of the character's `+0x14c` is set
   (`0087c4f9`-`0087c502`: `mov edx,[ecx+0x14c]; shr edx,0x11; test dl,1`) and `O+0x10` (the
   current mode) is not null, then `M+0xe8 = 1`. `[game]`

Update `FUN_0087c300` (21 bytes): `state_transition_update(S, O)` and nothing else. No mode
work, no ground test, no clip work. `[game]` The transitions are the whole state (see "The
connections").

Exit: slot 7 is `FUN_0087bc70` (the same as StateDrive's exit): `FUN_00878140(S)` (overlay exit),
then the current mode `M = **(O+0x10)`: `M+0xe8 = 0` and its handle at `M+0x28` is released
(`FUN_00544480`). The exit does NOT change the mode. `[game]`

Differences from StateDrive's enter `FUN_0087ba10` (593 bytes, `[game]` as read in
`docs/state-machine.md` and in the decompile here):

| point | StateDrive enter 0087ba10 | StateFly enter 0087c320 |
|---|---|---|
| animation set | DriveSet 0xc78b249e | FlySet 0x032e87a6 |
| clip | `S+0x44` | `S+0x44` (same field) |
| ground test | only when `O+0x26c` (on ground) is set; in the air the enter starts no mode | none |
| mode started | if clip is Trans_R2V_A or Trans_R2V_Run_A and `fold_in_to_car` says no: MoveMode 0x252ce043; else DriveMode 0xe2b22eb4 | FlyMode 0xfb9f3262, always |
| robot form 3 | only inside the DriveMode branch | always, when the handle object is not null |
| visibility toggle (V_Idle) | when S is `stateDrive` and the state at `O+0x20` is not StateDrive | when S is `stateFly` (the hub only, not EnterA/B) and the state at `O+0x20` is not StateFly |
| `M+0xe8 = 1` | always, for `stateDrive` | only when char `+0x14c` bit 17 is set |
| update | `FUN_0087b8d0` (317 bytes, not read here; `state-machine.md` says it repeats the mode choice while not in DriveMode) | transition walk only |
| exit | `FUN_0087bc70` | `FUN_0087bc70` (same) |
| may-enter | `FUN_0087c550` | `FUN_0087c550` (same) |

## StateFlyExit

Enter `FUN_0087c7e0` (294 bytes; decompile only, the constants are the decompile's; the
companion value -0xf7e2b98 equals 0xf081d468). `[game]`

1. `FUN_00878070(O)`: refcount on `O+0xc`.
2. `animation_set_start(FlySet, S+0x44, 0, 1)` on `O+0x10`. `S+0x44` is the exit clip.
3. If the current mode's class id (`(**(O+0x14 current)+0xc)()`, slot 3) is NOT the drive
   companion `f081d468`, `FUN_00852150(O+0x14, 0x6da1d12d = JumpMode, 0)`. So leaving flight
   starts JumpMode (the jump is what ends the flight; `docs/flight-update.md` lists "previous
   mode is FlyMode, grounded, speed squared >= 25: LONG_JUMP, no launch").
4. Refcount on `O+8`, the object from it: if not null, `FUN_0071ab70(obj, 1)` (robot form 1,
   "on foot") and `FUN_0071cfa0(obj, 0, 1, 0, 0)` (the visibility toggle with the 0 value; not
   decoded further). `[game]`

Update `FUN_0087c910` (84 bytes): if the current mode is JumpMode (class `6da1d12d`) and
`O+0x26c` (on ground) is set, `FUN_00852150(O+0x14, 0xf081d468 = drive companion, 0)`; then
`state_transition_update(S, O)`. So after the jump starts, when the character is on the ground
the companion mode takes over (the companion decelerates the body and turns it to the exit turn
speed, `docs/state-machine.md` "Companion mode"). `[game]`

Exit: slot 7 is `FUN_00878140` (overlay exit, then slot 6 of each overlay, 84 bytes). Unlike
StateFly's exit, it does NOT clear `M+0xe8`. `[game]`

## The states in the data

Names resolved with `export/data/names.tsv` (hash to text). `[data]` Hash values are the signed
field values in `global_states.txt`.

| state | class | animName (hash) | clip name | overlay | canGoToStates |
|---|---|---|---|---|---|
| stateFly | StateFly | 0 | (none: entry rules) | no | DEATH, HIT_REACT, STUN |
| stateFlyEnterA | StateFly | -1287269621 (0xb345d30b) | Trans_R2V_A | no | DEATH, STUN |
| stateFlyEnterB | StateFly | 709657265 (0x2a4c82b1) | Trans_R2V_B | no | DEATH, STUN |
| stateFlyExit | StateFlyExit | 1502413892 (0x598d0444) | Trans_Vehicle2Robot | no | DEATH, STUN |
| stateFlyExitA | StateFlyExit | 1103944983 (0x41ccdd17) | Trans_V2R_A | no | DEATH, STUN |
| stateFlyExitB | StateFlyExit | -658142035 (0xd8c58cad) | Trans_V2R_B | no | DEATH, STUN |
| stateFlyAttackSlam | StateFly | 2033279735 (0x793162f7) | Trans_Vehicle2RobotSlam | no | (long list) |
| stateFlyTransAirPunch | StateBasic | (anim Trans_V2R_GroundPunch, set FlySet) | Trans_V2R_GroundPunch | no | (long list) |

For comparison (Bumblebee's car): stateDriveEnterA -1287269621 (Trans_R2V_A), stateDriveEnterB
709657265 (Trans_R2V_B): the SAME clips as the jet's EnterA and EnterB. So the clip names do not
tell car from jet; the enter code and the mode do (the set is DriveSet for the car, FlySet for
the jet, `animation_set_start` first argument). `[data]`

Machines that hold these states (Starscream, `characterAttributesStarscream`): `stateMachineFlyAdvance`
(hub stateFly, EnterA/B, ExitA/B, the air punch), `stateMachineFlyStunt` (the four stunt
states), `stateMachineGenericSpecialFlyWithAnim` (a special with the hover and slam states), and
`stateMachineHoverAndAirSlam`. The plain `stateMachineFly` (with `stateFlyExit`, Trans_Vehicle2Robot)
is NOT in Starscream's list. Which character uses it: not found. `[data]`

Priorities: the dump does not print priorities for these connections; the raw order is the
dump's index. Priorities are in `global_states.txt` per state (`priority: N`), not extracted per
connection. `[data]` not done.

## The connections

Connection objects (`connectionFlyHold` etc.) are in the dump `sc_conn.txt`; the Bumblebee
drive ones in `bb_conn.txt`. Every detector below is a single stage with `time: 0`, no
stick limits (amplitude -1 to 100, angle -360 to 360 both sticks, camera-relative) unless noted.
`checkX` flags are all false unless noted. `[data]`

Fly (Starscream):

| connection | kind | detector (button includes / excludes) | other checks | from -> to |
|---|---|---|---|---|
| connectionFlyHold | ControlDetect | include 0x1c (FlyingMode) / exclude 0x18 (WeaponMode) | none | (inside the list below) |
| connectionFlyHoldLight | ControlDetect | include 0x1c / none | none | (inside the list below) |
| connectionListFlyHold {FlyHold, EnvCanSwitchAvatar} | list | as above | no ground test | stateIdle (idx 5), stateMove (0), stateFall (1), stateHoverFall (1), stateJump (0), stateHitReact (2) -> stateFlyEnterA |
| connectionListMeleeTransform {connectionDrive, EnvCanSwitchAvatar, WaitForSecondAnimBranch} | list | connectionDrive: include 0x15 (VehicleMode) / exclude 0x18 | branch 2 | stateAttack1, stateAttack2, stateAttack3 -> stateFlyEnterA |
| connectionListFlyRelease {ReleaseFly, EnvInsideFlightBoundary, EnvCanSwitchAvatar} | list | ReleaseFly: include none / exclude 0x1c | envTest #f65c039d (boundary) and the switch test (not read) | stateFly (idx 0), stateFlyStunt* (idx 3) -> stateFlyExitA |
| connectionListFlyToAirPunch {FlyToAirPunch, EnvInsideFlightBoundary, EnvCanSwitchAvatar} | list | include 0x23 (Punch) / exclude 0x1c; checkCanAdvTransform true | as above | stateFly (idx 1) -> stateFlyTransAirPunch |
| connectionWeaponModeVehicleAttack | (not read) | (not read) | | stateFly (idx 2), EnterA (3), EnterB (3) -> stateWeaponModeVehicleAttack |
| connectionListFlyLightAndAnimDone {FlyHoldLight, WaitForAnimDone} | list | include 0x1c | anim 100/100 | EnterA (1) -> EnterB; EnterB (1) -> stateFly |
| connectionWaitForAnimDone | ControlDetect | none | anim branch 100 / 100 (clip finished) | EnterA (0) -> EnterB; EnterB (0) -> stateFly; ExitA (0) -> ExitB |
| connectionListFlyReleaseAndAnimDone {ReleaseFly, InsideBoundary, EnvCanSwitchAvatar, WaitForAnimDone} | list | ReleaseFly | anim done | EnterA (2) -> stateFlyExitB |
| connectionListFlyReleaseAndSecondAnimBranch {ReleaseFly, InsideBoundary, EnvCanSwitchAvatar, WaitForSecondAnimBranch} | list | ReleaseFly | anim branch 2 / 2 | EnterB (2) -> stateFlyExitA |
| connectionListFlyAndAnimDone {connectionFlyHold, EnvCanSwitchAvatar, WaitForAnimDone} | list | FlyHold (include 0x1c / exclude 0x18) | anim done | ExitA (1) -> EnterB; ExitB (1) -> EnterA |
| connectionListNoInputAndAnimDone {connectionNoInput, WaitForAnimDone} | list | NoInput: no buttons (all stick ranges full) | connectionNoInput animBranch 100 (clip finished); anim done | ExitB (0) -> stateFall |
| connectionMessageFlyStuntUp/Down/Left/Right | ConnectionMessage | message ids (msgUserDefined -720196535, -1968591941, -332276504, 1508155407) | | stateFly (stateMachineFlyStunt idx 20-23) -> stateFlyStuntUp/Down/Left/Right |
| connectionFlyToHover, connectionFlyToSlam, connectionFlytoWeaponHover | ControlDetect | include 0x21 (AttackTransformHover) / 0x21 with checkCanAdvTransform; Slam: include "true" (= button 1, AttackFast `[assumed]`) with checkCanAdvTransform; WeaponHover: include 0x18 / exclude 0x1c | | stateFly in stateMachineGenericSpecialFlyWithAnim -> stateHoverEnter / stateFlyAttackSlam |

Drive (Bumblebee, for the car):

| connection | kind | detector | other checks | from -> to |
|---|---|---|---|---|
| connectionListDrive {connectionDrive, EnvCanSwitchAvatar} | list | include 0x15 (VehicleMode) / exclude 0x18 (WeaponMode) | none (no ground test on the connection) | stateMove, stateJump, stateJumpAdv, stateIdle, stateHitReact, stateFall -> stateDriveEnterA |
| connectionListMeleeTransform {connectionDrive, EnvCanSwitchAvatar, WaitForSecondAnimBranch} | list | as above | branch 2 | stateAttack1, stateAttack2, stateAttack3 -> stateDriveEnterA |
| connectionListDriveAndAnimDone {connectionDrive, EnvCanSwitchAvatar, WaitForAnimDone} | list | include 0x15 / exclude 0x18 | anim done | stateDriveExitA (1) -> stateDriveEnterB; stateDriveExitB (1) -> stateDriveEnterA; stateDriveExitRunA/B likewise |
| connectionListDriveReleaseAndAnimDone {connectionReleaseDrive, EnvCanSwitchAvatar, WaitForAnimDone} | list | ReleaseDrive | anim done | stateDriveEnterA (2) -> stateDriveExitB |
| connectionListDriveReleaseAndSecondAnimBranch | list | ReleaseDrive | branch 2 | stateDriveEnterB (2) -> stateDriveExitA |
| connectionListJumpDrive {connectionJumpDrive, EnvCanSwitchAvatar} | list | JumpDrive (Jump 0x22) | | stateDriveEnterA (4), B (4) -> stateDriveExitJump |
| connectionDriveToAirPunch | ControlDetect | include 0x23 / exclude 0x15; checkOnGround true, checkCanAdvTransform true | | (drive to the air punch) |

## Port recipe

Starscream standing on the ground, presses and holds FlyingMode (0x1c), not WeaponMode (0x18).
`[data]` `[game]` unless tagged.

1. The state machine (before the mode, same update, `docs/state-machine.md`) finds
   `connectionListFlyHold` from stateIdle. The `connectionFlyHold` detector needs 0x1c down and
   0x18 up. No ground test. Then the best-priority winner goes to `stateFlyEnterA`. Entering it
   calls its enter (`0087c320`), which: sets the animation set to FlySet and starts clip
   `Trans_R2V_A` (0xb345d30b); makes FlyMode current on `O+0x14` (FUN_00852150); sets robot form
   3 (`0071ab70`, which reads the canFly gate); the vehicle-model visibility (V_Idle) is NOT set
   here (only in the hub). `[game]`
2. FlyMode now runs every step (its update, `docs/flight-update.md`), so the body flies while
   the transform clip plays. `[game]` `[assumed]` that this is the order (the mode runs after the
   state in the same update, as `docs/state-machine.md` says for the other modes).
3. Clip `Trans_R2V_A` ends (anim branch 100): stateFlyEnterA's connections at anim done are
   `WaitForAnimDone` (priority 0) and `FlyHoldLight` (1) -> `stateFlyEnterB`, and
   `ReleaseFly` + boundary + switch + anim done (priority 2) -> `stateFlyExitB`. So if 0x1c is
   released by the time the clip ends (and the boundary and switch tests pass), the flight is
   abandoned into ExitB; otherwise it goes on to EnterB. The priorities are from
   `global_states.txt` (stateMachineFlyAdvance, stateFlyEnterA). `[data]`
4. `stateFlyEnterB`: clip `Trans_R2V_B` (0x2a4c82b1). Its enter is the same code (FlyMode already
   current: no restart). Anim done -> `stateFly` (the hub). Release at its second branch
   (`connectionWaitForSecondAnimBranch`, 2) goes to `stateFlyExitA`. `[data]`
5. The hub `stateFly`: enter `0087c320` again (mode no-op) plus the hub-only block: the vehicle
   model visibility on (`FUN_0071cfa0(..., 1, 0, V_Idle, 0)`, when the state the character came
   from is not StateFly) and `M+0xe8 = 1` when char `+0x14c` bit 17 is set. The clip is the data
   default (`animName` 0: FlySet's entry rules). `[game]` Which clip that is: not found.
   Where the jet (vehicle) model replaces the robot model: the `AlternateModelEvent` keys in the
   transform clips (docs/bumblebee-mechanics.md "AlternateModelEvent", docs/character-weapon-data.md:
   for Bumblebee's clips). Whether Starscream's Trans_R2V_A/B carry them: not checked. `[assumed]`
   The hub's update walks its transitions: release -> exit; Punch -> air punch; the special
   and slam/hover ones are in the special machine.
6. Leaving flight, 0x1c released in the hub (inside the boundary and the switch test): ReleaseFly
   -> `stateFlyExitA`, clip `Trans_V2R_A` (0x41ccdd17). Its enter: FlySet clip; if the current mode is
   not the companion, JumpMode (`0x6da1d12d`) starts; robot form 1 (on foot); visibility off. Its
   update: if JumpMode is current and the body is on the ground (`O+0x26c`), the drive companion
   (`0xf081d468`) starts. Anim done -> `stateFlyExitB`. If 0x1c is pressed again (FlyHold, anim
   done) -> `stateFlyEnterB`: back into flight.
7. `stateFlyExitB`, clip `Trans_V2R_B` (0xd8c58cad). Anim done: 0x1c held -> `stateFlyEnterA`
   (flight again); nothing held (`connectionNoInput`, clip finished) -> `stateFall`. `[data]`
8. `stateFall` (the ordinary fall, `docs/state-machine.md`: FallSet, then JumpMode). `[game]`
   So the release does NOT go to hover. It goes to `stateFall` (the character falls to the ground
   and lands through the normal ground states). Hover is entered only by `connectionFlyToHover`
   (AttackTransformHover 0x21), not by release. `[data]`
9. Leaving flight through a jump (the `stateDriveExitJump`-like path): not found for Starscream.
   `[data]` The FlyMode exit (`00865b80`) is not read here.

Car, for contrast (Bumblebee): the same shape, with `connectionListDrive` (VehicleMode 0x15 held)
-> `stateDriveEnterA` (Trans_R2V_A), DriveSet, and the drive enter starts DriveMode only when on the
ground (a grounded character that does not line up with its motion gets MoveMode first), with the
robot form 3 set only in that branch.

## Not found

- The FlyMode exit `00865b80` and update `00864320` (other tickets). Not read here.
- The `envTest`s of `connectionEnvInsideFlightBoundary` (#f65c039d) and `connectionEnvCanSwitchAvatar`
  (`ConnectionEnvironment`, the environment test is switched on in `FUN_0084cee0`, `state-machine.md`):
  not read. So what happens when the character is outside the flight boundary: not found (none of the
  hub's connections fires on that alone).
- The clip the hub plays with `animName` 0 (`FlySet` entry rules): not resolved.
- Whether Starscream's `Trans_R2V_A/B` (and `Trans_V2R_A/B`) carry `AlternateModelEvent` keys: not checked.
- `FUN_0071ab70` (robot form) past its canFly gate, `FUN_0071cfa0` (visibility toggle) and
  `FUN_00544db0`: not read in full.
- `FUN_0087b8d0` (DriveState update): not read here.
- The connection priorities for the fly states (in `global_states.txt` per state): not extracted.
- The use of `stateMachineFly` / `stateFlyExit` (Trans_Vehicle2Robot): no character found.
- The census name for `0087c550` ("state_drive_may_enter") is the shared may-enter of both
  StateDrive and StateFly; it is the same function, so the port's may-enter for StateFly is the
  same DISABLE_VEHICLE test. `[game]`
- The `connectionListFlyHold` priority in the states it is listed in (stateIdle idx 5 etc.) is the
  dump index only.

## Ledger

Functions read here are noted in the unpublished notes (tag `game`): 0087c320, 0087c300, 0087c1f0,
0087c780, 0087c7e0, 0087c910, 0087bc70, 0087c660, 0087c1e0, 0087c670, 0087c190, 00929ff0, 008783e0,
0076c530, 00adf4e0, 00adf540, 00863950. Written up: this file.
