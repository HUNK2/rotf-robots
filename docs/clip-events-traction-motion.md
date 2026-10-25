# Clip events ChangeTractionEvent and ChangeMotionModeEvent: what they do

Scope: the two animation key events that every robot's clips carry and the port ignores.
Read-only research (2026-10-20). Nothing under the install was touched, no .rs file and no
existing note was edited. Tags: [game <address>] read in code or disassembly, [data] read
from the decoded pack dumps, [assumed] a guess with what would settle it, [unsettled] open.

## Summary

- ChangeTractionEvent carries one field, `traction` (3 floats). When its key is passed, the
  game's run handler 00712960 writes that vector into the CURRENT MOTION MODE's control
  vector (mode+0xc..+0x14) for the modes that accept it, and in BasicMotionMode also zeroes
  the body's horizontal velocity. The control vector is the per-axis share with which the
  clip's root motion drives the body: 1 means the animation moves that axis, 0 leaves it to
  the physics. There is no exit handler, so the value stays until the next write.
- ChangeMotionModeEvent carries one field, `newMode` (a class hash). Its run handler 00712a20
  calls the mode switch 00852150, which makes the mode of that class the current motion mode
  (exit of the old, enter of the new), if the character's mode list has one. No exit handler.
- Neither event is in the port (no hook in core\src\formats\anim.rs or core\src\sim.rs).

## 1. Loaders, registration and the fire path

- Registrars (census labels "registrar", both right in kind):
  - 00ad74f0 registers ChangeTractionEvent: name hash (crc32_name), run handler 00712960 at
    registry node +0x10, exit handler 0 at +0x14. [game 00ad74f0]
  - 00ad7550 registers ChangeMotionModeEvent: hash, run handler 00712a20 at +0x10, exit 0 at
    +0x14. [game 00ad7550]
  - The registry is the AnimatorEvent type registry 0053aa20 (node +0xc hash, +0x10 run, +0x14
    exit). [game 0053aa20, ledger]
- The census labels for 00712960 and 00712a20 ("factory of ...") are WRONG: they are the run
  handlers the registry calls, not factories. [game 00712960, 00712a20]
- Fire path. The clip's event channel is walked by animator_run_events 0053ab90: an event is
  due when its u16 key time (at key+2) plus the clip offset (+0xc) is at or below the clip
  time; the event is then pushed as {handler, key, node} onto the frame queue by
  event_queue_push 0053f4c0. [game 0053ab90, 0053f4c0] The queue is run by anim_ctrl_send_events
  00544a00 after the controller's sweep (listener_list_call 005443c0 per queued entry, then the
  count is reset). [game 00544a00] The exact dispatch from the queue to each handler is not
  traced: [assumed] the handler runs in the same update, after the animation step.
- Exit path: animator_finish 0053adb0 queues an exit handler only when the node has one. Both
  events have exit 0, so leaving the clip does nothing for either. [game 0053adb0 (ledger),
  registrars]
- No loader/vtable for the event objects was located. The dumps write them as
  `#object: ChangeTractionEvent` inside AnimKeysEvent arrays. [unsettled] how the game builds
  the key block from the file: the run handlers read it by hash, see section 2.
- The "fire virtual" the task asked for is therefore the registry's run handler (node+0x10),
  not a vtable slot of an event object.

## 2. ChangeTractionEvent: the run handler 00712960

Read from disassembly 00712960-00712a1d (raw_disasm). [game 00712960]

1. Character: calls character_object_lookup 0070a3c0 and uses its result only if non-null.
   0070a3c0 returns the value stored under the name hash 'Character' (118b3297) in the scene
   table 005781b0. [game 0070a3c0, 005781b0] So the handler acts on THE registered Character,
   not necessarily the character whose clip carries the event. [unsettled] which object that
   is when more than one robot is in the scene; in the one-character arena it is the avatar.
2. Field: the key block (second argument, ECX-relative block pointer) is searched for the
   field hash 0xc0c1ac30 ('traction') by data_field_by_hash 00573940. [game 00573940 (ledger)]
   The three floats at the field's pointer are copied to a local vector.
3. Write: EAX = [char+0x104] (the movement owner record STEP, see the unpublished notes)
   and motion_control_set 00854960 is called with the vector pointer. [game 00712960]

### motion_control_set 00854960 (the writer)

Read from disassembly 00854960-00854a64. [game 00854960]

- Takes the current mode M = STEP+0x10 and reads its type with vtable +0xc:
  - 1ad6ae68 (BasicMotionMode, the first mode; census "Stand"): writes the vector to M+0xc,
    +0x10, +0x14. Then, if the body (STEP+0x38) exists and STEP+0x3d4 is clear, it calls
    body_linear_velocity 00556c00 and physics_body_set_velocity 00557000 with the velocity
    (0, 0, vz): horizontal velocity zeroed, vertical kept. [game 00854960 at 8549af-8549f8]
    This is hand-derived from the stack offsets (the velocity is written to esp-0xc..-0x4
    before the call, and the z component is read from esp-0x4). [unsettled] Not emulated.
  - fb9f3262 (FlyMode): calls 00866900 with the vector. [game 00854a04-00854a14] 00866900 is
    NOT READ.
  - d7068c31 (ClimbMode, the census "climb"): writes M+0xc..+0x14 only when M+0x18 == 6.
    [game 00854a25-00854a39]
  - 70776cef (HoverMode): writes M+0xc..+0x14. [game 00854a45-00854a53]
  - Any other type (MoveMode 252ce043, StrafeMode 3c956e02, JumpMode 6da1d12d, Drive, Action,
    Death, ...): writes nothing. [game, the branches above fall through]
- Tail (for every type): if the body exists and STEP+0x3d4 is clear, it re-sends the current
  mode's control M+0xc to the holder through holder_control_set 006e8da0 (see section 4).
  [game 00854a64-00854ab9]

So a traction event changes the control of Stand, Hover, a Climb with field value 6, and
(unread) Fly. It does nothing in Move, Strafe, Jump or the attack and dash modes.

### Data: the traction values and where they occur

Decoded dumps, work\packdump\*.txt, 13 robots, 93 ChangeTractionEvent uses. [data] Values
and the clip that carries them (owner resolved by names.tsv; see caveat in section 6):

- [1,1,1]: Trans_Vehicle2Robot2GroundPunch (6), Trans_V2R_GroundPunch (2), Vehicle_LoopFull
  (2), Vehicle_LoopBack (2), Vehicle_RollLeft (2), Vehicle_RollRight (2), some clips with
  unnamed owners (2). All at key time 0 in the rows checked.
- [1,1,0]: Death_Explode_Fall (13, at key time 0), StunStart (1).
- [0,0,0]: GroundPunchFall (9, key time 0), Trans_R2V_A/B, Trans_Robot2Vehicle, Trans_RobotRun2Vehicle,
  Trans_V2R_Hover, Vehicle_LoopFull, Vehicle_LoopBack, Vehicle_RollLeft, Vehicle_RollRight
  (all at a later key time, usually the clip's end).
- [0,0,1]: Trans_V2R_Hover (6 at key time 0, 1 later), Trans_R2V_A (4 at 0, 2 later), Trans_V2R_A (2 at 0, 2 later), Trans_V2R_B (2), Trans_R2V_B (2 at 0), Trans_Robot2Vehicle (1 at 0, 1 later). Most are in starscream (9), blackout (6), jetfire (4), firestorm (2).
- [1,0,1]: 2 rows in starscream, owner not resolved.

The task's example ([1,1,1] at a stunt's start, [0,0,0] at its end) matches the Vehicle_Loop*
and Vehicle_Roll* clips: [1,1,1] at key time 0, [0,0,0] at a later key time. [data]

Counts by robot (traction): starscream 31, blackout 10, firestorm 14, jetfire 9, megatronflight 7,
audir8 3, bumblebee 3, megatron 3, optimus 3, ratchet 3, optimusflight 5, ironhide 1, longhaul 1.
Total 93. [data]

## 3. ChangeMotionModeEvent: the run handler 00712a20 and the mode switch 00852150

Read from disassembly 00712a20-00712ac8. [game 00712a20]

1. Same character lookup as section 2. [game 00712a20, 0070a3c0]
2. Field: newMode (hash 0x43927d79) read by 00573940 from the key block; the value is the
   class hash of the mode to make current. [game 00712a20] The only field the dump carries is
   `newMode`. [data]
3. Calls FUN_00852150(manager, newMode, 0), where manager = *([char+0x314] + 0x14). [game 00712a20]
   [assumed] that manager is the same record as STEP (char+0x104), which holds the current
   mode at +0x10 (the traction path reads the current mode at STEP+0x10). [unsettled]

### The switch 00852150

Read from decompile and the callee vtable reads. [game 00852150, 00856c90, 00544340]

- Walks the mode list (head at +0x18, sentinel at +0x14, current at +0x10).
- For each mode, vtable +0x10 is called with the hash; the base implementation 00856c90
  returns `type() == hash`, with type = vtable +0xc. [game 00856c90] So the first listed mode
  whose class is the hash is taken (and the optional mode+8 filter; here the argument is 0,
  so any).
- If that mode is already current: vtable +0x1c is called, which is 00544340 (a no-op) for
  FlyMode and HoverMode; the switch returns 1. [game 00544340, census: the no-op]
- Otherwise: the old current gets its exit (+0x18, called with the new mode), the new one
  becomes current, then gets its enter (+0x14, called with the old current and the hash).
  Returns 1. If no listed mode matches, it returns 0 and nothing changes. [game 00852150]
- FlyMode's type getter 00863940 returns fb9f3262. [game 00863940]

Which modes a character's list holds was NOT read. [unsettled] For this reason a mode event
to a class the character does not own is silently ignored.

### Data: the mode values and where they occur

68 ChangeMotionModeEvent uses in the 13 dumps. [data] Values:

- newMode 3c956e02 (StrafeMode): 58 uses (9 robots). Owner clips: Weapon_FallBigLand,
  Weapon_FallBigLand2RunFwd/LeftRight, Weapon_FallLand, Weapon_FallLand2RunFwd/LeftRight. All at
  key time 0. [data]
- newMode 6da1d12d (JumpMode): 6 uses (bumblebee, firestorm, ironhide, longhaul, optimus,
  ratchet). Owner: Weapon_Fall. Key time 0. [data]
- newMode 1ad6ae68 (BasicMotionMode / Stand): 4 uses (blackout, firestorm, jetfire, starscream).
  All four owned by Hit_WallFront_2_Fall. Key time 0. [data]

Counts per robot: audir8 4, blackout 5, bumblebee 9, firestorm 8, ironhide 9, jetfire 1,
longhaul 9, megatron 4, optimus 9, ratchet 9, starscream 1. Total 68. [data]

So the game uses it to put the weapon set's landing and run clips into StrafeMode (the
weapon mode on foot) at key time 0, and in Weapon_Fall to make JumpMode current.

## 4. What the vector is: readers of mode+0xc

The per-frame control is chosen in movement_owner_step 00852220 (read from disassembly
8522d0-852360). [game 00852220 (disasm)]

- owner+0x34c set (the owner's impulse flag, docs/state-machine.md): control (0.1, 0.1, 0).
- else owner+0x3d4 set: the vector at owner+0x3d8.
- else, MoveMode (252ce043) with owner+0x26c clear (in the air): control (0.1, 0.1, 0).
- else: the current mode's mode+0xc..+0x14 (the value a traction event writes).

The choice goes to holder_control_set 006e8da0 with the body at owner+0x38: it stores the
vector at holder+0xa4 +0x2c..+0x34 (the control) and the rotation vector at +0x38..+0x40.
[game 006e8da0]

The holder's step body_control_step 006db490 then does, per axis:
    wanted = (animated origin - origin) / dt;  v += control * (wanted - v)
(control 0 when any listed contact has +0x1ec). [game 006db490; already ported as
physics_manifold::control_velocity, see docs/physics.md 1.6]

So: control 1 on an axis = the body follows the clip's root motion on that axis; control 0 =
the body keeps its own velocity there (gravity and contacts act). Per axis, because the vector
is per axis.

Other writers of mode+0xc, for context: basic_motion_enter 00856cc0 sets (1,1,1) [game];
StateIdle's update 0087d790 writes the first mode's control directly: (1,1,1) when on the ground
and not pushed, else (1,1,0) [game 0087d790]; StateBasic enter 00879ef0 calls 00852150 for its
motion mode and motion_control_set for (1,1,0) when its allowFall flag is set [game 00879ef0].

Reading: a traction event overrides the state's own (1,1,1) or (1,1,0) until the next state enter
or idle update writes it again. [assumed: the idle update runs every frame and so restores
its own value at once while the idle state is current. Settles by a trace of
ChangeTractionEvent in the running game.]

## 5. The port: what is there and the recipe

What is in the port now (grep of core\src and game\src):
- control vectors: none stored per mode. The port uses fixed per-state values, and the
  horizontal blend is a stand-in (docs/physics.md, "the mode's blended x / y velocity is the
  control target ... stand-in"); idle's (1,1,1)/(1,1,0) is written as a comment at
  core\src\sim.rs around line 3204 and not as a field.
- mode switches: sim.rs set_mode at line 2896 is the port's mode setter with entry and exit
  effects. The clip events are parsed in core\src\formats\anim.rs (read_clip at line 648;
  read_keys at 604; special fire events are the model: read_special_fire_events at 541, dispatch
  at 677, sort at 688, field in Clip at 73).

Recipe:
1. anim.rs: add `traction_events: Vec<(time, [f32; 3])>` and `motion_mode_events:
   Vec<(time, u32)>` to Clip, read them in read_keys like read_special_fire_events, from class
   names "ChangeTractionEvent" (field `traction`, 3 floats) and "ChangeMotionModeEvent" (field
   `newMode`, a hash), with time in ticks (1/60 s, see section 6). Sort by time.
2. sim.rs: when the clip's events fall due (same rule as animator_run_events: time at or below
   the clip time, respecting the loop rule), apply them in event order:
   - traction: if the current mode is Stand (1ad6ae68), store the vector as the mode's control
     (new field, default (1,1,1)); in Stand with a body, set the horizontal velocity to zero and
     keep vz (section 2; [game], hand-derived). Hover: store the control. Climb: store only if
     its field is 6. Fly: do nothing until 00866900 is read. Other modes: nothing.
   - mode: map 1ad6ae68 -> Mode::Stand, 3c956e02 -> Mode::Strafe, 6da1d12d -> Mode::Jump(_)
     (the JumpType for it is [unsettled]); call set_mode only when the robot's mode list is
     known to hold that mode. No exit handler: leave the mode until the next state enter.
3. The holder blend in sim.rs: replace the fixed per-state control with the stored per-axis
   control, using the same per-axis step as body_control_step (v += control * (wanted - v)),
   which physics_manifold::control_velocity already implements bit-exact.
4. Tests: a core test with a scripted clip (one traction event at 0 with [1,1,1], one at 60
   with [0,0,0]) to check that the per-axis control is stored, that the horizontal velocity is
   zeroed on Stand, and that the mode event switches Stand to Strafe and back. Capture the
   Vehicle_LoopFull / Death_Explode_Fall clips with `--shot` once wired.

## 6. Caveats and what is not settled

- [unsettled] The run handlers act on the registered 'Character' (0070a3c0 via 005781b0), not
  necessarily the clip's owner. Settles by reading 005781b0's table fill (who registers
  'Character' and in what order), or by a multi-robot trace.
- [unsettled] Mode-list membership per character: the mode switch returns 0 and does nothing
  for a class the character lacks. Settles by reading the character constructor for the mode
  list (the constructor of the movement owner, the unpublished notes section 1, and
  the state data's motionModeType values).
- [unsettled] 00866900 (the FlyMode branch of motion_control_set) is not read. Flyers' traction
  behaviour is therefore unknown. It is in the census as unread.
- [assumed] The BasicMotionMode velocity write (0, 0, vz) rests on a hand reading of the stack
  offsets in 00854960 (8549c7-8549f8); the decompile drops it. Settles by a second reading or an
  emulator run of that branch with the externals stubbed (work\emu, one pure function at a time;
  the external calls here are not pure, so this is hard). Until then it is [game, hand-derived].
- [assumed] The manager in the mode switch (*([char+0x314]+0x14)) is the STEP record. Settles
  by reading the character constructor where char+0x314 is set.
- [unsettled] The exact time the queued handler runs relative to the body step of the same frame
  (section 1). Settles by a trace or by reading listener_list_call 005443c0 and the queue consumer.
- [assumed] Time units: the key time is a u16 in ticks (0053ab90 reads it at key+2); 1/60 s per
  tick is from CLAUDE.md and the anim.rs convention, not checked against a recording.
- [assumed] Clip owners in section 2 and 3 come from the dump's layout: the first `animID:` line
  at 14-space indentation after the event belongs to the clip that holds the stream. Checked on
  a handful of rows (the owner names read as plausible clip names), not on all 221 rows.
  Scanner output: work\packdump-scan is not kept; the script is in the session scratchpad.
- [unsettled] The FlyMode and Hover tests of section 4 use the control vector only through
  006e8da0; no flight recording exists to compare.

## 7. Ledger changes made while reading

Functions read and recorded with `fn.py note` (rows in the unpublished notes): 00712960,
00712a20, 00ad74f0, 00ad7550, 00854960, 00852150, 00856c90, 00863940, 006e8da0, 00852220,
0087d790, 00879ef0, 005781b0, 0070a3c0, 0053ab90.

Three existing rows were overwritten by mistake and restored: 0070a3c0 and 00854960 with their
original text plus the new findings; 00852220 with the head of its original text. The row for
00852220 lost the tail of its original text (after "tail sets/clears owner+0x1d4 b"), which
could not be recovered; the row says so.
