# Bumblebee mechanics: research notes

Working notes for adding Bumblebee to the mashup as a playable character. Addresses refer to the
analysed `Transformers2.exe`; they belong here and must never be copied into the mashup's code
(its own rules forbid addresses and decompiler names, and `make publish-check` greps for them).

Status: step 1 (how the mashup takes a character) is mapped. Step 2 (the original formulas) is
partly done: the structure of the movement system is known, the jump is read from disassembly
(type, height, launch, air request), on-foot translation is confirmed to be animation root
motion, and the change of form is driven by clip events. The driving physics, and the routine
that applies a mode's control vector, are not read yet.

Everything in section 3 is exercised by the standalone test in `bevy_test\` (`src\sim.rs`).

Where the decompiler drops floating-point work, read the listing instead:
`tools\ghidra_headless.cmd ghidra_proj TF2 -process Transformers2.exe
-noanalysis -readOnly -scriptPath tools\ghidra_scripts -postScript
DumpDisasm.java work\asm <address> ...` writes one `.asm` per function
with constants and call targets annotated (about a minute per run).

## 1. How the mashup is put together

Clone: `mashup`, commit `ab43b8a` (2026-10-01).

- One simulation funnel: `sim::step(world, tick, input, msec, reason) -> Snapshot`
  (`crates/sim/src/carrier.rs`). Server authority, client prediction and replay all call it.
  Humans and bots both arrive as `TickInput.cmds`.
- Per player and per command, `run_players_system` (`crates/sim/src/step.rs`) builds a context and
  a collision backend and calls `movement_iw4::pmove(ps, cmd, context, &backend, ..)`. That call is
  the soldier's entire movement.
- `UserCmd` (`crates/playerstate_iw4/src/usercmd.rs`): `server_time`, `buttons`, `angles[3]`,
  `forwardmove: i8`, `rightmove: i8`, weapon ids. Button bits include ATTACK, SPRINT, USE, RELOAD,
  PRONE, CROUCH, JUMP, ADS, FRAG, SMOKE.
- Collision for movement: `CollisionBackend::trace(GroundTraceInput { start, end, mins, maxs,
  tracemask }) -> Trace { fraction, normal, surface_flags, material, allsolid, startsolid,
  walkable, endpos, .. }`. A swept axis-aligned box.
- `MATCH_TICK_MS = 50`. Movement is stepped per command with its own clamped `msec`.
- Units are MW2's (inches). The Transformers data is in other units (speeds of 24 to 45, gravity
  35), so a scale factor has to be chosen and applied consistently.

### How Skate 3 takes over the player

- `crates/render_anim/src/skate.rs` runs the skate physics in a worker thread, stepped once per
  rendered frame with `dt.min(0.1)`, reading the gamepad directly.
- On activation it calls `authority.set_external_motion(client, true)`. In `run_players_system`
  an externally moved client only has its command time advanced; `pmove` is skipped.
- Every frame it writes the result back with `authority.set_origin(client, position)` and stores
  bones, camera and status in the `frame::SkateMode` resource.
- `crates/net/src/client/runtime.rs` zeroes `forwardmove`, `rightmove` and `buttons` in the
  outgoing command while skating.
- Camera: `render_anim/src/occupancy/view_kick.rs` uses `SkateMode.camera`. Body:
  `occupancy/remote_body.rs` and `fpv_present.rs` pose the local soldier from `SkateMode.bones`.
  Reticle hidden in `hud/src/reticle.rs`. Toggle on J in `console/src/plugin/mod.rs`.
- Limits: it needs `net::AuthorityWorld`, so it only works on the host; the pose is shown only on
  the local machine; nothing is predicted or replayed as movement.

### Two ways to attach Bumblebee

A. Like skating: external motion plus `set_origin`, own simulation, own input and camera.
   Isolated and quick. Host only, not deterministic, other players see a soldier sliding around.

B. Inside the funnel: a second movement routine beside `pmove`, chosen per player, fed by
   `UserCmd` and the same collision backend, with a fixed internal sub-step. Deterministic,
   predicted, replayable, usable by bots. Touches `sim`, `playerstate_iw4` and the network
   delta, because his extra state (form, turbo timers, wheel state) has to live in player state.

For "very accurate" B is the right target; A is a way to see him move sooner.

### Repo conventions that affect this work

- No addresses, offsets or decompiler names in code or comments; names must be the repo's own.
- Constants transcribed at full float precision (the workspace disables the clippy lints that
  would shorten them).
- Tests only in `crates/approved_tests`; comments only where the code cannot show a hazard.
- A new crate or subsystem counts as architectural: upstream asks for an issue first.

## 2. The original movement system

### Where the tuning lives in memory

`FUN_00726710` loads `characterAttributes<Name>` into an object. Mode blocks sit at:

| block | offset | loader |
|---|---|---|
| climb | 0x1a0 | `FUN_0085ad50` |
| dash | 0x1b0 | `FUN_0085c1c0` |
| drive | 0x1b8 | `FUN_00859710` (first byte is `createMotionModeDrive`) |
| fly | 0x2e0 | `FUN_0085aea0` |
| jump | 0x4c4 | `FUN_00858da0` |
| move | 0x544 | `FUN_00858bd0` |
| strafe | 0x554 | `FUN_00858d20` |
| hover | 0x560 | `FUN_0085bf90` |

`python tools\decomp_lookup.py fields <loader> <base>` prints the field-to-offset map for any of
them. Angles are converted to radians when stored. `walkThreshold` is never read.

Jump block, relative offsets: 0x00 jumpHeight, 0x04 jumpHeightDouble, 0x08 jumpSpeed,
0x0c jumpControl, 0x10 jumpMinHeight, 0x14 jumpTurnSpeed; the same six for run at 0x18
(control at 0x28, min height at 0x24), high standing at 0x30, long at 0x48; then
0x60 climbJumpUpHeight, 0x64 climbJumpOffHeight, 0x68 climbJumpOffSpeed, 0x6c groundPunchAccel,
0x70 groundPunchVelLimit, 0x74 jumpWeaponZVelStartDilation, 0x78 jumpWeaponDilation,
0x7c jumpWeaponDilationAcceleration.

### Modes

`FUN_00851a40` builds a list of mode objects per character. Names are compared by CRC-32.

| mode | name hash | class id | table | update | enter | exit |
|---|---|---|---|---|---|---|
| (unnamed first mode) | 3079313a | 1ad6ae68 | 00b626a8 | stub | 00856cc0 | stub |
| ClimbMode | d958988e | d7068c31 | 00b626d0 | 00856d00 | 00856cf0 | 008583f0 |
| DashMode | d77c2045 | 517c0a3b | 00b626f8 | 00858530 | 00858520 | 008588f0 |
| DriveMode (if `canDrive`) | 0f39f816 | e2b22eb4 | 00b62730 | 0085cad0 | 0085c290 | 0085ed50 |
| (drive companion) | 64a59130 | f081d468 | 00b62758 | 00863070 | 00863670 | 00863760 |
| FlyMode (if `canFly`) | 27ffe52b | fb9f3262 | 00b62780 | 00864320 | 00863950 | 00865b80 |
| GrabMode | 0b6b1ac4 | c5882131 | 00b627a8 | 0086ef20 | 0086ef10 | 0086f440 |
| HoverMode | c8a7f878 | 70776cef | 00b627d0 | 0086f960 | 0086f510 | 00871580 |
| InteractMode | 6ad1caf7 | bbc94a36 | 00b627f8 | 00872660 | 00872650 | 00872db0 |
| JumpMode | 382b3502 | 6da1d12d | 00b62820 | 00872fe0 | 008740e0 | 00874850 |
| KnockBackMode | 83de9b88 | 1839220c | 00b6284c | 00874f30 | 00874f20 | stub |
| MoveMode | 30511531 | 252ce043 | 00b62874 | 00875b20 | 00875eb0 | 00875ec0 |
| StrafeMode | 469135ab | 3c956e02 | 00b6289c | 00875ef0 | 00875ee0 | 00876ea0 |
| SyncMode | 3dabea8a | e6392241 | 00b628c4 | 008772c0 | 00877220 | 00877620 |
| ThrownMode | 01dc04c5 | 4a15f3ab | 00b628ec | stub | 0092a050 | stub |
| AttackMode | 4c58422b | cd2f4b0c | 00b62658 | 00855af0 | 008569a0 | 00856a10 |

`python work\mode_ids.py` regenerates this table from the executable.

Every mode has the same ten-slot function table: 0 destructor, 2 update(dt), 3 returns the class
id, 4 "is this class id", 5 on enter, 6 on exit. DriveMode, FlyMode and HoverMode use slots 1, 8
and 9 as well (DriveMode: `FUN_0085c880`, `FUN_00860870`, `FUN_0085f780`).

A mode object is: function table, owner (+4), name hash (+8), a three-float control vector
(+0xc, +0x10, +0x14) and usually a pointer to its tuning block. The control vector says how much
stick input counts along each axis while the mode is active: move and dash are (1, 1, 0), climb
is (1, 1, 1), jump sets it every frame from the jump type.

MoveMode's enter and exit only set and clear a flag on the owner (+0x3e4).

The owner holds: +0x20 an input source (virtual call at +0x24 returns the 2D stick direction),
+0x2c the character transform (3x3 rotation at +0x10, position at +0x34), +0x34 the animation
controller (`[[+0x34]+0x20]+0x18` is the name hash of the animation set now playing), +0x38
the physics body holder (body at `**(+0xa4)`), +0x26c a byte the jump mode tests before it
launches (see JumpMode).

The body: +0x18 is its gravity as a force (mass times the gravity vector; times the inverse
mass at +0x28 it is the gravity acceleration), +0x24 mass, +0x28 inverse mass, +0xa0 linear
velocity. An earlier version of these notes called +0x18 momentum; that was wrong, and it is
why the jump mode looked as if it were "recording vertical velocity": it records, scales and
restores the body's gravity. The third coordinate is up.

### MoveMode update

Takes the stick direction, transforms it by the character matrix, and if its squared length is
over 0.01 rotates the character toward it by at most `turnSpeed` degrees per second times dt
(360 for Bumblebee). It does not move him. Translation is the root motion of the clip that is
playing; see section 3.

### JumpMode

On enter (`FUN_008740e0`, given the previous mode and a second argument): reads the body's
gravity, keeps its vertical part on the mode (+0x20), writes it back scaled (+0x24; the scale
is 1 unless an upgrade is active or a global flag halves it), resets elapsed time and the
height limit, then picks a jump type and stores it in the animation variable `C_JUMP_TYPE`
(hash 32564629):

- second argument is 31420837: type aa185eeb, no launch
- playing `ClimbSet` (7048543e): type dfb7146d, vertical velocity cleared, with a forward push
  of 20 along facing
- playing `GroundPunchSet` (ece50ea5): type 2494da83, no launch
- previous mode is DriveMode or its companion, and a jump was asked for (owner +0x26c):
  horizontal speed squared <= 3.0 gives `HIGH_STANDING_JUMP` (f4312e7f), otherwise `LONG_JUMP`
  (225a6ca8); these are the jumps out of vehicle form
- previous mode is FlyMode, grounded, speed squared >= 25: `LONG_JUMP`, no launch
- `FallSet` (75352b1b) is playing, or owner +0x26c is clear: type aa185eeb, no launch. This is
  how walking off an edge enters the mode. (Corrected 2026-10-03: +0x26c is "on the
  ground", as the state machine's `checkOnGround` shows; see `state-machine.md`.)
- otherwise: stick squared <= 0.01 gives type aa185eeb (standing), else type 0ec51d8e (moving)

The launch, when there is one:

- Height by type (`FUN_00874b30`): moving 0ec51d8e uses jumpRunHeight; both f4312e7f and
  225a6ca8 use jumpLongHeight; 9c689146 uses climbJumpUpHeight; dfb7146d uses
  climbJumpOffHeight; everything else, the standing jump included, uses jumpRunHeight times
  1.2. `jumpHeight` and `jumpHighStandingHeight` are not read here. For Bumblebee: standing 12,
  moving 10, out of vehicle form 14. An active upgrade multiplies the height by one plus its
  value, and a global flag doubles it.
- Launch velocity (`FUN_006e7aa0`): straight against gravity with speed sqrt(2 * |g| * height).
- Horizontal: the body's horizontal velocity is kept and added to the launch. For a moving
  jump entered from MoveMode with stick squared over 0.9801 it is first rescaled to exactly
  MoveMode's `maxSpeed` (StrafeMode's speed when coming from there), so the take-off speed does
  not depend on where in the stride the run clip was.
- The vertical launch speed is also kept on the mode (+0x38).

On exit (`FUN_00874850`, given the next mode): puts the body's gravity back. When the next mode
is MoveMode it reads that mode's fourth tuning value (`walkRunSpeed`) and, if the body is moving
horizontally faster than that, scales horizontal velocity down to it; StrafeMode does the same
through a helper.

Update (`FUN_00872fe0`), each frame:

- If the gravity scale changed, vertical velocity is rescaled so the remaining rise is kept.
- Slow motion while aiming in the air: while `WeaponSet` (af5106dc) plays or `FUN_00721b50`
  holds (taken to be the aiming test; not read), once |vz| is at or below jumpWeaponZVelStartDilation the
  factor at +0x28 moves toward jumpWeaponDilation at jumpWeaponDilationAcceleration per second;
  otherwise it moves back to 1. Gravity is multiplied by the factor and dt by its square root.
- The request for this frame: the stick turned into world space (not normalised), if its
  squared length is over 0.01. A long jump (flag at +0x34) with the stick centred uses the
  direction of the body's horizontal velocity instead. Then, by type, speed from
  `FUN_00874cb0` (moving jumpRunSpeed, f4312e7f jumpHighStandingSpeed, 225a6ca8 jumpLongSpeed,
  dfb7146d climbJumpOffSpeed, else jumpSpeed) and turn speed from `FUN_00874df0`. The character
  transform is turned toward the direction by at most turn speed times dt and moved by
  direction times speed times dt (`FUN_0056e4e0`, `FUN_0056dfe0`, `FUN_00545900`).
- Ground punch: the body's gravity is pushed down by groundPunchAccel each update, limited to
  groundPunchVelLimit, with no dt (so it depended on frame rate in the original).
- control vector = (c, c, 0) with c chosen by type: moving jump uses jumpRunControl, high
  standing uses jumpHighStandingControl, long uses jumpLongControl, climb jump uses 0,
  everything else jumpControl. For Bumblebee: 0.038, 0.1, 0.025, 0, 0.1.
- tracks fall distance for landing.

### How a mode's request reaches the body (read 2026-10-02)

The owner's update (`FUN_00852220`, which also calls the current mode's update) hands a control
vector to the physics holder each frame through `FUN_006e8da0`, which stores it at
`[holder + 0xa4] + 0x2c` (a second vector, for rotation, goes to `+0x38`, always zero here):

- owner `+0x34c` set: (0.1, 0.1, 0);
- owner `+0x3d4` set: the vector stored at owner `+0x3d8`;
- the mode is MoveMode and no jump was asked for (`+0x26c` clear): (0.1, 0.1, 0);
- otherwise the mode's own control vector (`mode + 0xc`).

Corrected 2026-10-26: `+0x26c` is the ground flag (see JumpMode), so the 0.1 applies to MoveMode
OFF the ground (stepping off an edge before the fall starts). On the ground MoveMode uses its own
(1, 1, 0): the body takes the clip's velocity at once, and the start and stop ramps are the
clips' own blends (`MECHANICS.md`, "On foot"; `core/src/sim.rs` `MOVE_AIR_CONTROL`).

The holder's step (`FUN_006db490`, given dt) then does, per axis:

    wanted = (animated position - body position) / dt
    velocity += control * (wanted - velocity)

where the animated position is the object's transform (moved this frame by clip root motion
and by whatever the mode did to it) times the holder's offset. The result is clipped against
the contact planes with static geometry (`FUN_006dd900`, `FUN_006dd800`, `FUN_006dd5b0`) and
written as the body's velocity (`FUN_00557000`). A contact flagged at `+0x1ec` zeroes the
control for that step.

What follows from it:

- On foot the body takes the clip's root-motion velocity at once; off the ground in MoveMode it
  closes 10% of the gap each update (corrected 2026-10-26, above).
- In the air the gap closes by the jump type's control value per update (0.038 running, 0.1
  standing, 0.025 long). With the stick centred the mode moves nothing, so `wanted` is zero and
  the horizontal speed bleeds off at that same rate.
- Vertical control is 0 in both, so height is gravity and the launch alone.
- It is per update, not per second: the original's feel depends on its frame rate (the step is
  the measured frame time, see `camera.md`).

Not confirmed by reading: that the transform is put back on the body after the physics step
(it must be, or the vertical axis could not work), and the holder's alternative mode (flag bit
5, which uses the change in `wanted` and also drives rotation).

## 3. What the character's data settles

These come from `characters\bumblebee.str` and need no code reading.

### On-foot movement is root motion

Clip times count sixtieths of a second. `WalkSet/Run` is 40 ticks and carries the root 16.0
forward: 24 per second, which is exactly `modeMove.maxSpeed`. `WalkSet/Walk` is 64 ticks and
5.99 forward: 5.61 per second. So the robot has two paces, not a speed proportional to the
stick. Which one plays is decided by the animation rules: `animRuleMovement` is "stick over
0.01" and `animRuleMovementRun` is "stick over 0.7". Walk and run blend in over 0.25 s
(`xfadeTime`), and `IdleStandSet/Run2Idle` (20 ticks, 3.48 forward) is the stop.

Other clips move the root too and would drive those moves the same way: the four dodges carry
it 24 (22 for the right one) in 46 ticks, the landing-into-run clips 3.9 and 6.2, the climbs
7.3 to 7.7 upward, the wall shimmies 7.5 sideways. `work\animids.py` lists every clip with its
length, loop flag, blend time and root travel.

### The change of form

Both models carry their own clips. The robot's are in `DriveSet` (`Trans_R2V_A` 26 ticks then
`Trans_R2V_B` 72, with `_Run_` variants of 34 and 76) and `Vehicle2RobotSet` (`Trans_V2R_A` 10
then `Trans_V2R_B` 50, `_Run_` variants of 16 and 44, and `Trans_V2R_Jump` 42). The vehicle's
are in the `AnimRefBundle` attached to its object, under the same names.

`AlternateModelEvent` keys in the robot's clips run the show: each says whether to show the
alternate model (the vehicle) and the true model (the robot), which clip to start on the
vehicle, and what the vehicle plays after it (`V_Idle`). Robot to vehicle: both shown from the
start, the robot hidden 14 ticks into part B. Vehicle to robot: both shown, the vehicle hidden
22 ticks into part B (at the end of the 42 ticks for the jumping exit). The robot's `Main` node
is scaled down to 0.56 on the way in and back up on the way out.

Where the root is in these clips (found 2026-10-03 from the stand-in host's pictures): the
`Root` channel's position is where the clip was made, not an offset to show. Clips that
follow each other carry on from each other's: `Trans_R2V_A` goes 0 to 3.5 forward (most of
it in the last five ticks), `Trans_R2V_B` stays at 3.5, the car's `V_Idle` sits at -4.01 and
`Trans_V2R_Run_A` starts there. Drawn as stored, the car sat 3.5 m ahead during the fold
and 4 m behind for the whole drive. So the root is held still when drawing
(`Model::sample` in `ak-host\src\bumblebee.rs`) and its travel counts only as movement.

What run 1 shows of the timing (`work\traces\rotf_run1_overview.txt`, rows around 433.8,
412.9, 435.8, 619.6):

- Into the car from standing: DriveMode starts on the update the trigger goes down and the
  car pulls away at once (0, 0.08, 0.21, 0.40 ... 36.5 after 1.2 s) while the 98-tick clip
  still plays. The root's 3.5 m is NOT applied to the body. So the mod driving off while he
  folds is what the game does.
- Into the car from a run: two updates of MoveMode with DriveSet first (23.2, 24.4), then
  DriveMode at 24.3.
- Out of the car on the ground: the companion mode (`00b62758`) with Vehicle2RobotSet for
  1.03 s (the 60 ticks of `Trans_V2R_A` + `_B`). Speed falls 1.15 an update (about 35.5 a
  second) to 23.5 and holds there if the stick is held, or on down to zero if not; then
  MoveMode at 24.0 or the first mode at 0. Not in the sim yet: it goes straight to
  MoveMode and sheds speed at `driveExitToRunDeceleration`.
- Out of the car with jump: JumpMode with Vehicle2RobotSet for 0.71 s (`Trans_V2R_Jump`,
  42 ticks), then JumpSet.

### Bindings

`R2` vehicle mode, accelerate and fire; `L1` brake; `L2` weapon mode and drift; the bottom face
button jump and turbo; the left one melee; the right one action and climb; `L3` strafe.
`Character.advTransformDelay` is 0.25.

## Not read yet

- What reads `jumpHeight`, `jumpHighStandingHeight`, the `*MinHeight` and `*HeightDouble` values.
- Driving: see `driving.md` (speed rules, steering, tyre force read; drift, the wheel set's
  second step, the exit `FUN_0085ed50`, slot 8 `FUN_00860870` and the companion mode not).
- Dash, climb, strafe and hover updates.
- The animation rules: see `animation-rules.md` (evaluators and variables read; the controller
  that calls them not). The test still uses a small hand-written stand-in.
- Camera code: see `camera.md` (follow and driving cameras read; weapon camera and collision not).
- Jump type names for aa185eeb, 0ec51d8e, dfb7146d, 9c689146 and 2494da83.

The export in `export\decomp` was regenerated after defining 1,994 functions that are only
reachable through function tables (23,462 functions, 1,346,074 lines).
