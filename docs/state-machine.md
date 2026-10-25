# The control state machine: research notes

Addresses are for the analysed `Transformers2.exe` and stay in these notes.

Status (2026-10-03): the data model, the detector, the connection test, the choice between
transitions, the attack and dash states, and the climb (`climbing.md`, with the climbable
environment test in `FUN_0084cee0`) are read. The other environment tests and the weapon and
drive states' own code are NOT read.

What decides WHEN Bumblebee jumps, attacks, dodges, climbs, changes form or aims is not
code: it is a graph of states and connections in `bnxglobal.str#16`. Code only supplies what
each state does when entered and each kind of connection's test.

## Looking at it

`python work\statemachine_dump.py <install>\bnxglobal.str characterAttributesBumblebee
[machines | states | connections | all]` follows every reference. Its output for Bumblebee
is kept in `work\bb_machines.txt`, `work\bb_states.txt`, `work\bb_connections.txt` (and
`bb_connections_short.txt`, with default fields dropped). `work\cclean.py <address>...`
prints pseudocode without the reference-counting noise.

## The data

- `characterAttributes<Name>.stateMachineList` names a `StateMachineList`; Bumblebee's is
  `stateMachineListPlayerLand`: Common, DriveAdvance, Weapon, Climb, DriveAttacksAdvance,
  GenericSpecial, JumpGroundPunch, PickUpHuman, CarFlip, Stun, PlayAnim, CombatAdvance,
  CSAInteract.
- A `StateMachine` is `states: [{state, connections: [{connection | connectionList,
  outputState, priority}]}]`. States are shared objects: every machine that lists
  `stateIdle` ADDS its transitions to the one `stateIdle` (`FUN_00850280`), so a state's
  real transition list is the union over the machines.
- A `ConnectionList` passes when all of its connections pass.
- Button numbers in detectors are an enum in code order, which is the binding list of
  `global_tuning.txt` with the `Flying*` entries moved after `WeaponSwitchUp`:
  0 Action, 1 AttackFast, 2 AttackSpecial, 3 AttackVehicle, 0xb Climb, 0xc Dodge, 0xd Drop,
  0xe Jump, 0xf Pickup, 0x10 StrafeMode, 0x11 Throw, 0x12 VehicleAcclerate, 0x13
  VehicleBrake, 0x14 VehicleDrift, 0x15 VehicleMode, 0x16 VehicleTurbo, 0x17 WeaponFire,
  0x18 WeaponMode, 0x19 WeaponSwitchUp, 0x1a to 0x20 the seven `Flying*`, 0x21 to 0x24
  AttackTransformHover, Jump, Punch, Slam. The text dumps print 0 and 1 as `false` and
  `true`. (Checked against what the connections must mean: Jump, Climb, Dodge, VehicleMode,
  WeaponMode, WeaponFire and the three transform attacks all land on the right names.)

## Choosing a transition (`FUN_00877d90`, `FUN_00878230`)

Each update the current state runs its own update, then every transition whose connections
all pass AND whose output state agrees to be entered (state slot 11, e.g. the dash's
cooldown) competes; the HIGHEST `priority` wins, the first listed on a tie. The machine
(`FUN_00850760`) then calls exit on the old state and enter on the new one and runs the new
state's update in the same update. Overlay states (`overlayState: true`, the special
attack) run on top of a state rather than replacing it.

## Overlay states (read 2026-10-04; ported in `control::Runner`)

A state object holds: `+0x2c` its `overlayState` flag, `+0x30` the overlay state running on
top of it, `+0x34` the one asked for, `+0x38` (on an overlay state) the state under it.

- `FUN_00877d90`, which every state's update ends with, starts with `FUN_00878370`: if
  `+0x34` differs from `+0x30`, exit the old overlay, enter the new (setting its `+0x38`),
  `+0x30` = `+0x34`; then the running overlay's own update.
- Then the state's detectors, and `FUN_00878230(state, machine, 0)`: the best transition
  into a state that is NOT an overlay state. A transition with no output counts only when
  the state asking is itself an overlay state. Found: the machine's next state.
- Nothing found, and no overlay running: `FUN_00878230(..., 1)`, the best transition INTO
  an overlay state (its connections and the state's slot 11; the current state's "may
  leave", slot 8, is not asked). Found: `+0x34` = it. It is entered at the start of the
  NEXT update.
- For an overlay state the same routine: a winning transition with no output, or back to
  the state under it, clears the under state's `+0x34` (it is left at the start of the next
  update); into another overlay state it replaces it; into any other state nothing.
  Finding nothing, it looks for an overlay of its own (the special on top of the firing).
- A state's exit (`FUN_00878140` -> `FUN_00878310`) exits its overlay and clears both.
- A completed ordinary state with no transition or overlay to start asks slot9 and,
  when done, requests the owner's default state at +0x318 (`00877d90`, raw
  `00878020..00878056`). Character setup `00850a70` binds this to stateIdle
  (`00716d8b..00716d9b`). Ported for full hit reactions 2026-10-06.

In the port a transition with no output out of an ordinary state used to win and do
nothing; it is now left out of the choice, as in the original.

More of the same routines, read 2026-10-04 (later) for the car's gun and the special
(`docs/weapon-mode.md`), and ported in `control::Runner`:

- An overlay state with no transition that wins, and no overlay of its own to start, asks
  its own slot 9 ("done"); if yes, the state under it's `+0x34` is cleared, so it is left
  at the next update's start. `StateAttackSpecial`'s slot 9 (`FUN_00879950`) is yes for
  an overlay state; `StateAttack`'s (`FUN_00878d20`) is no for one. So the special lives
  one update and a firing state lives until its release connection.
- "An overlay of its own": the overlay state's own `+0x30` / `+0x34`, swapped at the
  start of ITS update. In the port `Runner::nested` / `nested_pending`.
- `+0x3c` on the state under: the transition that asked for the overlay. When the
  overlay is exited, that transition's connections are reset (`FUN_0084c520`: each one's
  slot `+0x18`). In the port `Runner::ask_overlay` and the reset in `swap_overlay`.
- State classes: `StateAttack` `00b62984` (slot 11 may-enter `FUN_0076c530` and slot 8
  may-leave `FUN_008783e0` both always yes; `+0x48` is `attackRanged`, `+0x2c` the
  overlay flag, `+0x50` `robotForm`, `+0x5c` `fireButton`), `StateAttackSpecial`
  `00b629cc` (update `FUN_00879490`, enter `FUN_00879110`, exit `FUN_00879700`, may-enter
  `FUN_008799c0`).
- `robotForm` is the control mode a state's enter hands to `FUN_0071ab70` (character
  `+0x31c`: 1 on foot, 2 weapon, 3 the car), 5 for "leave it"; the drive states set 3 and
  the weapon states 2 in their own code. `FUN_0071ab70` switches the camera (`+0x1d8`
  weapon, `+0x1dc` driving) and tells the weapon manager (`FUN_007b43b0`). In the port
  `StateDef::robot_form` and `State::car_mode`.

## A connection's own test (`FUN_0084c050`, fields loaded by `FUN_0084be60`)

All must hold:

- `checkInAir`: owner `+0x26c` is clear. `checkOnGround`: it is set. So owner `+0x26c` is
  "on the ground", NOT "a jump was asked for" as `bumblebee-mechanics.md` guessed. That
  also settles the control vector: MoveMode on the ground uses its own (1, 1, 0), and the
  (0.1, 0.1, 0) is for MoveMode in the air, which is what run 1 showed.
- `checkHoldingSomething` / `checkNotHoldingSomething`: character flag `+0x148` bit 12.
- vertical velocity strictly between `minZVelocity` and `maxZVelocity`.
- the clip's branch count is at least `animBranchBegin` and at least `animBranchEnd`. The
  count (`+0xdc` on the animation player, one per channel) is set to 1 when a clip starts,
  each `AnimBranchPointEvent` adds one, and a finished clip counts as 100. So
  `connectionWaitForSecondAnimBranch` (2, 2) is "the clip's branch point has passed" and
  `connectionWaitForAnimDone` (100, 100) and `connectionNoInput` (100, 0) are "the clip has
  finished".
- `checkAttackCanBranch`: animation variable `a8d2d204` is 1 (see the attack state).
- `checkCanAdvTransform`: `FUN_00725990`, which tests one of the character's flags
  (`+0x14c`) by index: 0x20, which the drive mode sets after `advTransformDelay` (0.25 s)
  (2026-10-04; `docs/melee.md` "The attacks out of the car").

Kinds: `ConnectionControlDetect` adds "its detector is active"; `ConnectionEnvironment`
(`envTest`, a hash switched on in `FUN_0084cee0`). Read 2026-10-16 (movement session):
`ConnectionDistToGround` (`FUN_0084cd30`): owner `+0x334` (height over the ground, 0 when
grounded) strictly over `distLimit` with `greaterThan`, strictly under it without; absent
fields are 1 and true (`FUN_0084cc80`). `ConnectionTimeInState` (`FUN_0084dc00`): the
machine's time in the state (`+0x28`, dt added first thing in `FUN_00850760`, zeroed on a
change of state) at least `timeLimit` (1 when absent). `ConnectionMessage` (`FUN_0084d9c0`
load: `msg`, or `msgUserDefined` when `msg` is 0; `FUN_0084da50` sets its flag when the
message matches, `FUN_0084da60` clears it on reset).

State slots (vtables dumped): 4 update, 5 enter, 7 exit, 8 may-leave, 9 done, 11 may-enter,
12 class id. `StateBasic` (`00b62a0c`): may-leave `FUN_0087a0a0` (`finishOnAnimSetExit`:
the playing set is no longer its own; else no `finishAnim`: yes; else the clip has
finished), done `FUN_0087a1b0` (`finishOnAnimSetExit` as above; else `playAnimOnce` and the
clip finished), exit `FUN_0087a5d0` (the ledge catch `FUN_0087a200`, which acts only for
`stateClimbPullup`). An ordinary state that is done with no transition winning asks for the
default state (`state_transition_update`). `StateClimb` exit (`FUN_0087a820`): Climb still
down sets owner `+0x3f8`, which the climbable test refuses until the body's step clears it
with Climb up. `StateDash` exit (`FUN_0087abc0`): the weapon manager puts the gun away
(`FUN_007b2eb0(0)`) unless the next state is a `StateWeapon`. `StateIdle` and `StateMove`
enters zero the body's vertical speed.

## The detector (`FUN_0084f480`, stage test `FUN_0084f230`)

A detector is a list of stages plus `timeActivationStaysOn` and `holdWithLastStage`. A
stage: `time`, `timeIncludeButtonCharge`, `includeButtons` (all must be DOWN),
`excludeButtons` (none may be down), `includeHitButton` (must have gone down this update;
-1 for none), and for each stick an amplitude range and an angle range (degrees in data;
`cameraRelative: false` measures the angle against the character instead).

Kept per detector: time in the stage, time the stage's include buttons have been held, the
stage number, active, active time left. Each update, with `dt`:

1. stage time += dt; held time += dt if the stage's include buttons are all down, else 0.
2. if the stage's test passes, and held time >= `timeIncludeButtonCharge`, and stage time
   >= `time`: subtract both and go to the next stage.
3. past the last stage: active, active time = `timeActivationStaysOn`, back to stage 0.
4. while active: with `holdWithLastStage` it stays active only while the stage test still
   passes; without, it runs its time down and goes inactive at zero.

A detector with no stages is always active.

What that makes of Bumblebee's:

| connection | means |
|---|---|
| `connectionAttack` | AttackFast pressed, on the ground; remembered for 0.5 s |
| `connectionAttackNotCharging` | AttackFast is not down |
| `connectionAttackChain` | on the ground and the attack state's "can branch" is set |
| `connectionAttackJump` | in the air: AttackFast up, down, up (a tap); remembered 0.5 s |
| `connectionChargeXButton` | AttackFast held for 0.22 s |
| `connectionDash` | on the ground, WeaponMode down, Dodge pressed, stick 0.25 or more; 0.1 s |
| `connectionClimbStart` | Climb down (stays on while held) |
| `connectionClimb` | Climb let go and pressed again |
| `connectionClimbJump` | Jump pressed; 0.1 s |
| `connectionMoveDown` | stick 0.25 or more and within 60 degrees of straight back, character relative |
| `connectionJump` | Jump pressed, on the ground; 0.1 s |
| `connectionMove` / `ReleaseMove` | stick 0.18 or more / 0.15 or less |
| `connectionMoveOnGround` | on the ground, stick 0.25 or more |
| `connectionFall` | in the air, falling faster than 5, more than 3 above the ground |
| `connectionJumpSwitchDelay` | 0.2 s in the state |
| `connectionDrive` / `DriveRun` | VehicleMode down and WeaponMode not (Run: stick 0.11 or more) |
| `connectionWeaponMode` | WeaponMode down, not carrying |

## Melee (corrects `handoff\result-02.md`, which guessed the chain from the events)

Graph (machine CombatAdvance; priorities in brackets):

- Idle or Move, `connectionAttack` -> `stateAttackChargeInit` (clip `AttackFast_3_Charging`,
  56 ticks at speed 1.8, so 0.52 s).
- ChargeInit: AttackFast not down [1] -> `stateAttack1`; clip finished [0] ->
  `stateAttackChargeLoop`. So a tap leaves the charge on the next update and a hold of half
  a second goes on to the charge.
- ChargeLoop: AttackFast not down -> `stateAttackCharge` (`AttackFast_3_ChargeAttk`, the
  70-damage hit); finished -> Idle, or Fall at its branch point.
- Attack1: chain [2] -> `stateAttackChargeInit2`; finished [0] -> Idle; falling at the
  branch point [1] -> Fall; climb at the branch point [4]; VehicleMode at the branch point
  -> the change of form; WeaponMode at the branch point -> weapon mode.
- ChargeInit2: not down -> `stateAttack2`; finished -> ChargeLoop. Attack2 chains to
  ChargeInit3 (clip `AttackFast_3_ChargingBranch2`), which gives `stateAttack3`
  (`AttackFast_3`, 40 damage) or the charge. Attack3 does not chain.
- So holding the button at ANY of the three steps gives the charged finisher, and the
  third tap is the uncharged finisher.
- In the air (Jump, Fall): a tap -> `stateAttackJump` [3]; held 0.22 s ->
  `stateGroundPunchWeakFall` [4], which becomes `stateGroundPunchWeak` on landing. The full
  `stateGroundPunch` is reached only from the transform attacks.

The attack state (`StateAttack`, table `00b62984`; enter `FUN_008788b0`, update
`FUN_00878740`, exit `FUN_00878be0`): on enter it plays `animName` from CombatSet and sets
the motion mode `motionModeType` (AttackMode, or JumpMode for the air attack). The chain:
`AttackButtonWindowEvent` with `start` set opens a window (state `+0x60`); a press of
AttackFast while it is open is noted (`+0x61`); the event with `start` clear closes it and,
if a press was noted, sets variable `a8d2d204`, which is what `connectionAttackChain`
tests. So the next hit starts at the END of the button window (tick 36 of 60 in hits 1 and
2), not at the branch point.

AttackMode (`FUN_00855af0`), per update, from the playing clip's `animRef`:

- `AttackFaceAndSlideEvent` sets three character flags: `allowSlide`, `allowFace`,
  `allowStickMovement`.
- With `allowSlide` or `allowFace`: the wanted direction is the stick's (kept within
  `stick_choose_target_max_degrees` of the facing), or the facing with no stick; a target
  is looked for along it within `maxRange` and `auto_target_search_angle`
  (`FUN_0071efb0`). With a target further than touching distance: turn to it at
  `auto_target_turn_speed`, and slide toward it at `slideSpeed`, stopping at touching
  distance, while the distance is between `minRange` and `maxRange`.
- With `allowStickMovement`, no target and a stick: turn toward the stick at `turn_speed`
  (900 degrees a second for the fast attacks).
- Movement otherwise is the clip's root motion, as everywhere.
- Read since (2026-10-03), with corrections to the lines above, in `melee.md`: the target
  is looked for once and kept; the slide does not stop at touching distance, it stops
  being applied inside it; the stick's turn also needs no target turn that update, flag
  0x15 clear and, with `stop_on_hit`, no hit yet; the hit itself; the end of the routine
  (off the ground, the upward speed is capped at 2). `FUN_00725990(index)` tests a
  character flag (`+0x14c`): 0x17 allowSlide, 0x18 allowFace, 0x19 allowStickMovement.
  `FUN_00725a40` is "run by a human player here" (controller type 0).

## Dash (dodge)

- Reachable only from `stateWeaponMode` (priority 2), from a hit reaction, and at the end
  of leaving the car with WeaponMode held. Bumblebee has no dodge outside weapon mode,
  which is why run 1 has none.
- `StateDash` enter (`FUN_0087a960`): plays DashSet with no clip named, so the set's
  incoming rules pick the clip from `F_L_STICK_DIR` (stick against aim): within 45 degrees
  DodgeFront, 45 to 135 DodgeLeft, -45 to -135 DodgeRight, else DodgeBack. Sets DashMode.
- It may not be entered while the cooldown runs (`FUN_0087acb0`): DashMode's exit
  (`FUN_008588f0`) sets it to `groundDashCooldown` (0.5 s), counted down every update.
- DashMode update (`FUN_00858530`): rebuilds the facing each update from the heading kept
  at owner `+0x260` (the aim), so he does not turn; control vector (1, 1, 0). The travel is
  the clip's root motion: 24 forward, back or left, 22 right, in 46 ticks.
- Out: at the clip's branch point (tick 40) back to weapon mode [0]; fire [1]; falling [2];
  VehicleMode -> the change of form.

## Checked against `handoff\result-02.md` (2026-10-03)

Right: the clip tables (lengths, loop flags, blend times, speeds, root travel), the events
and their ticks, the damage and attack numbers, `modeDash`, `modeClimb`, the climb jump
values, the code addresses of the dash, climb and attack modes and of the hit event.

Wrong:

- The melee chain: every hit starts through a charge state, the chain fires when the
  button window SHUTS (not at the branch point), and holding at any step gives the charge.
- `AttackFast_3` does NOT share `AttackFast_3_ChargeAttk`'s clip: its `animFile` points at
  the same clip as `AttackFastMP` (70 ticks, 6.81 forward). The Python dump only printed
  "see object", and the handoff guessed the wrong one; the Rust reader follows the pointer.
- The dodge: only reachable in weapon mode, and its button is held weapon mode plus Dodge
  (the same face button as AttackFast). Dodge and climb bindings were right.
- The sideways dodges are not "rotated matrices": the root travels along y turned a quarter
  round; taking that turn out gives 24 left and 22 right.

## What runs in the port (`core\src\control.rs`, `sim.rs`)

The graph, the detectors, the connection checks and the choice of transition are the
original's and run on the real data, and since 2026-10-03 the machine decides everything:
`sim::control` runs it first each update, `enter_state` does what each state class's enter
does (above), and the motion mode it leaves (`Mode::Stand`, `Move`, `Jump`, `Drive`,
`Unfold`, `Action`, `Climb`) runs after it in the same update. The body never changes mode
by itself any more: a landing or running off an edge only changes contact, and the ground
flag the machine sees trails it (`GROUND_FLAG_LAG`). The unbuilt classes (hit reactions,
the stun, carrying, hover) are refused at may-enter; the overlay states built are the two
firing states and the special. `cargo run -p tf2-core --example control_check` plays scripted
button sequences through it (melee, dodge, ground punch, the car both ways) and prints each
state change. The `sim` tests carry a copy of the Common, DriveAdvance and Weapon parts of
the graph (`common_graph`).

## Idle, Move, Jump, Fall, Drive (read and ported 2026-10-03)

State tables: StateBasic `00b62a0c`, StateDrive `00b62b0c`, StateFall `00b62b4c`, StateIdle
`00b62c94`, StateJump `00b62d18`, StateMove `00b62d58` (slot 4 update, 5 enter, 7 exit,
8 may-leave, 11 may-enter, 12 class id).

- Idle enter: IdleStandSet, first mode (1ad6ae68). Move enter: I_IS_STRAFING = 0, WalkSet,
  MoveMode. Jump enter: JumpMode THEN JumpSet. Fall enter: FallSet THEN JumpMode (no launch).
  Jump may-leave: JumpMode time > 0.1. Fall may-leave: same, only if entered from
  stateWeaponModeJump. A mode already running is not re-entered (`FUN_00852150`).
- StateDrive enter: DriveSet clip; on the ground MoveMode if the clip is Trans_R2V_A/Run_A
  and the stick (> 0.01) is more than acos 0.9 off the body's motion (`FUN_0087bcf0`), else
  DriveMode; its update repeats that choice while not in DriveMode.
- StateBasic enter: set and clip, then `motionModeType`. Exits A/B use the companion mode.
- Companion mode (`FUN_00863070`, disassembly): speed = body speed; if > least, minus decel*dt,
  floored at 0 (not at least: run 1 holds 23.7); turn at exit turn speed; velocity = facing
  * speed. Run/idle values chosen at enter by stick > 0.05 (`FUN_00863670`).
- Trans_V2R_B has no branch point: Move/Idle/Fall only when it finishes (60 ticks total).
- Run 1 order: state machine first, then the mode, same update (jump update reads 24.52, not
  24.0). Landing: two updates in Jump after touchdown, so the ground flag lags one update.
- Leap out of the car needs Jump held as the trigger goes up (`connectionJumpDrive`, button
  0x22 = jump); no jump during the unfold.

- Run 1 also: the first car update reads 0.000 (the car mode does not push on the update it
  starts); the companion's first update already decelerates.

StateDrive's may-enter (`FUN_0087c550`, read 2026-10-04): true unless the character's
damage object (`+0x234`) exists and its float `+0x48` is over 0. That float is the
"vehicle disabled" time: its only setter is `FUN_00728930`, called from the damage routine
`FUN_00729120` (at `00729920`) when the hit's flags (`+0x3c`) have bit 12,
`DISABLE_VEHICLE`, and the hit's time (`+0x58`) is over 0; the object's update counts it
down and clears it. Setting and clearing both go through `FUN_0072c120`, which triggers
the effect `FX_stun`. (The next test at `00729925` is the same for bit 22,
`DISABLE_TRANSFORM`, with its own timer; not followed.) Nothing in the port deals a
`DISABLE_VEHICLE` hit, so `sim`'s may-enter stays true; the timer belongs with the
target's side of damage. Not looked at: which weapons carry the flag.

Not read: DriveMode's exit, the companion's exit (resets his orientation).

## The Jump state's ledge catch, Idle's update, StateWeapon (read 2026-10-03)

Jump update (`FUN_0087e1d0`): sets its `+0x44` once the ground flag is clear (cleared by its
exit; its reader was not looked for), then the ledge catch `FUN_0087dde0` (disassembly; the pseudocode
loses the floats), then the base update. The catch runs only when the state before
(`+0x20`) is `stateClimbJumpUp` (fba80d7b):

- body velocity z < 0;
- the ClimbMode object (found by class on the owner, `FUN_008521e0`; it outlives the climb)
  has a facing at the top F (`+0x38`) with |F|^2 > 0.9 and a top target T (`+0x2c`). The
  climb mode's enter zeroes both and only going over the top sets them, so in practice this
  follows "exit jump-up" (a wall with "extra above the top"). This is the reader of the top
  target that `climbing.md` was missing. `jumpForgiveDist` is NOT used here;
- T.z > his z; (P - T) . F < radius (`owner+0x170[form]`, `FUN_0070ab60`); his forward
  (xy, normalised) . F.xy > 0.98;
- a sweep `FUN_005be5a0` (mask 0x5d, radius = his radius) from T + F * 2.25 * radius, raised
  by h, straight down to his own z. h is `FUN_0070abe0`: `owner+0x160[form]` (his height)
  plus, when positive, holder `+0x134` - body `+0x54` * holder `+0x128` - his z (not
  understood);
- the hit must be above his z, less than 1.5 above it, under his z + h, and more than 0.25
  from the sweep's start;
- then `FUN_00854790`: the aim heading (`owner+0x260`) = F, his matrix rebuilt from the yaw
  `atan2(-F.x, F.y)` with the hit as position, the physics holder reset (`FUN_006e8500`),
  and `FUN_00855940` sets the body's velocity to zero.

Idle update (`FUN_0087d790`): while the first mode (1ad6ae68) runs, its control vector is
(1, 1, 1) when his height over the ground (`owner+0x334`, written by the owner's update
`FUN_00852220`: 0 on the ground) is within 0.01 of zero and he is not both on the ground
and pushed (`+0x34c`, set where the owner's update adds an impulse to the body, cleared
each update); else (1, 1, 0). So standing, the body's height follows the animated one.

StateWeapon (table `00b62e98`): load `FUN_0087f420` (`motionModeType` -> `+0x44`,
`canEnterOnlyIfNotSniping` -> `+0x48`; true only for `stateWeaponModeJump`).

- may-enter `FUN_0087fcf0`: with the flag, refused while sniping (`FUN_0117e860`).
- enter `FUN_0087f4a0`: the motion mode FIRST (`stateWeaponMode`: StrafeMode 3c956e02), then
  `WeaponSet`. The character's control mode (`+0x31c`, `FUN_0071ab70`) becomes 2 (weapon;
  the jump state sets 1), which for a player also switches the camera (`+0x1d8`). Unless
  the state before is a StateDash or StateWeapon: flag 0x100000 at character `+0x14c`, and
  the gun comes out: after `Vehicle2RobotSet` at once (weapon slot `+0x24`(1)), else if it
  is not out, `WEAPON_ACTION_PULL_OUT_GUN` played on `WeaponPartialSet`.
- exit `FUN_0087f6f0`: unless the next state is a StateDash or StateWeapon, the flag is
  cleared and the weapon manager (`+0x238`) told (`FUN_007b2eb0`).
- update `FUN_0087f800`: only for characters no player controls (`FUN_0087f900`).

In the port: the catch is `sim::ledge_catch` (the sweep is `World::drop_onto`; the arena's
ignores the ball's width [stand-in]; h is his height alone [assumed]). The arena's walls
never have "extra above the top", so play does not reach it; a test does. Idle's rule
changes nothing the body here does not already do (no vertical speed on the ground, no
pushes). StateWeapon's mode-then-set order was already what the port did; the gun's draw
and put-away wait for the weapon layer.

## Climb

The climb mode, the climbable test and the messages are read: `climbing.md`. The graph
(machine Climb):

- Idle, Move, Jump, Fall, the air attack: Climb held and the environment says climbable ->
  `stateClimb`.
- Climb: message "exit pullup" [4] -> `stateClimbPullup` -> Fall when its clip ends;
  message "exit bottom" [5] -> Fall; Jump pressed [1] -> `stateClimbJumpUp`
  (`Wall_JumpUp`), with the stick back [2] -> `stateClimbJumpOff` (`Wall_JumpOff`); Climb
  let go and pressed again [0] -> Fall.
- Messages: "exit pull-up" 98799ecd, "exit bottom" a884a9c4, "exit jump-up" 0230a6e3 (the
  last also -> `stateClimbJumpUp` [3]).
- `handoff\result-02.md`'s account of the stick directions inside ClimbMode: superseded by
  `climbing.md` (up within 45 degrees, down beyond 135, the sides between, on the pad's own
  stick).

In the port (`core\src\climb.rs`, `sim.rs`) the climb states run through the graph like the
attacks: the climbable environment test is real, `stateClimb` starts the climb mode, the
pull-up state leaves it running, and the messages reach `ConnectionMessage`.
