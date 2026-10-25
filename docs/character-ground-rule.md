# Character on foot: height over the ground and the grounded flag (reader ticket, 2026-10-12)

Scope: the five questions of the ticket. Static reading only (fn.py pseudocode and the notes). Nothing run,
nothing written under the install. Tags: [game] read in code; [assumed] a guess, with what would settle it;
[unread] not read. Supersedes the "NOT SETTLED" parts of character-world-contact.md for questions 2 and 4.

## Summary

1. Slots and the three numbers: the creator stores the body's +0x50/+0x54/+0x58 from three slot pointers
   [game]. Which shape output lands in which slot is taken from climbing.md [assumed]; the call-site stack
   layout did not settle it (see 1 below). Under that mapping the body's +0x54 is the GAP, 30/121 H, and
   the ride height +0x140[slot] is 0.8 gap = 24/121 H [assumed].
2. avatar_bottom_distance 006e9550 returns |U| * body+0x54, where U = root owner+0x120..0x128 and the root
   is found by walking +0x204 to the end [game]. So it is the gap (30/121 H) times the length of U.
3. Vertical motion: constant gravity is added by 006e96c0 (no dt) and is called only from physics_world_step
   006df7c0 [game for the call; that function is unread]. What stops the fall on the ground is the contact
   response plus the ground test 008524b0, which adds 5 x the average contact normal to the body velocity when
   contacts exist [game]. The ground snap 00737120 does NOT feed the ground decision and does NOT write z [game].
4. Grounded flag: owner +0x26c, written only by 008524b0 [game]. Its test is the lowest contact (z relative
   to the pencil bottom, and the horizontal distance of that contact), with hysteresis against the previous
   height +0x330 [game]. "Falling" is +0x26c == 0 with vertical speed <= 0, which counts up the fall timer +0x3f4
   [game, 00852220 lines 47-59]. No time-without-contact test was found.
5. Slope and step: no slope limit and no step-up height found in 008524b0 [game]. The only normal test is the
   sign and the average direction of the contact normals. Step-up: NOT FOUND.

## 1. Which pencil output is which slot

- first_avatar_create 00853550 calls pencil_avatar_create 006e7b60 with slot pointers: +0x140+i*4 (creator
  param_3), +0x130+i*4 (param_4), +0x150+i*4 (param_5), and +0x2c (param_6); it also writes the form height
  h to +0x160+i*4 and radius r to +0x170+i*4 [game].
- 006e7b60 stores body+0x50 = *param_4, body+0x54 = *param_5, body+0x58 = *param_6 [game, read in full].
  So body+0x54 is the slot at +0x150 and body+0x50 the slot at +0x130 [game].
- 006e7e90 (pencil_avatar_shape) writes three values: *param_6 = 0.8 gap = 24/121 H; *param_8 = gap =
  30/121 H; *param_7 = 3/11 H + gap = 63/121 H [game, read in full, the formulas].
- Which of these reaches +0x130, +0x140, +0x150 is NOT settled by the decompile: the call in 006e7b60 passes
  fewer arguments than 006e7e90 declares, and the disassembly of the call (006e7c86) uses stack slots that
  I could not map in the budget [unread]. The climbing note (climbing.md lines 26-33) and core\src\climb.rs
  say +0x140 = 0.8 gap and +0x150 = gap [assumed]. Settles by: following the stack pushes at 006e7b6x-006e7c86
  in raw_disasm, or by the emulator on 006e7e90 with the callee's stack args set.
- The 63/121 H value: no reader found [unread].

Consequences under that mapping [assumed]:
- body+0x54 = gap = 30/121 H (read by 006e9550 and 008524b0 as the bottom offset).
- owner+0x140[slot] = 0.8 gap = 24/121 H (the ride height in 008524b0).
- owner+0x170[slot] = r (the radius, in 008524b0).

## 2. 006e9550 avatar_bottom_distance [game, read in full]

    walk owner +0x204 to the last non-null (the root)
    return sqrt(root.+0x120^2 + root.+0x124^2 + root.+0x128^2) * (root.+0xa4)->body.+0x54

So it is |U| * gap, with U the root's up column. The ground override in 006e2c10 (lines 100-138, read in
collision-response.md) compares the contact height with this bottom distance; on a hit it forces the normal to
(0,0,+-1) and skips the commit [game, as read earlier].

## 3. Gravity and the ground

- 006e96c0 body_gravity_and_damping [game, read in full]: adds body+0x18, +0x1c, +0x20 to body+0x3c, +0x40,
  +0x44 (the constant force, which is gravity by the driving note); multiplies the linear velocity by +0x2c and
  the angular by +0x30; zeroes tiny speeds (squared < 2.5e-5 linear, < 1e-8 angular) and speeds above 100000.
  No dt. Caller: physics_world_step 006df7c0 only [game]. Meaning of the +0x18..0x20 vector as gravity:
  [assumed] from driving.md; settles by reading 006df7c0 lines around the call.
- 00737120 character_ground_snap [game, read in full]: runs once per update when the latch +0x38 is clear and
  body +0xa4 is set. It casts a vertical ray (x, y, z+5) to (x, y, z-20) with mask 0x14 through
  world_query_head 005be5a0, copies the hit point to locals, then calls FUN_00737720. No write to the position
  or to z [game]. The hit point does not reach 008524b0 [game].
- Caller of 00737120: NOT FOUND by fn.py callers and find_call (it is probably called through a vtable or a
  thunk) [unread].
- 00737720 writes no height [game, as read in character-world-contact.md section 5].

## 4. The grounded flag +0x26c and the height +0x334

- Writer: 008524b0 only [game]. Caller: movement_owner_step 00852220 at its start (line 18), before the avatar
  transition update and the state update [game].
- For a drive form (form id -0x1d4dd14c) the flag is any_wheel_on_ground 00851140 [game].
- For a character on foot [game, read in full]:
  - bottom point: P - U * gap, where P is the owner's position (+0x12c, +0x130, +0x134) and U is the owner's
    up column (+0x120..0x128) [game].
  - for each contact in the owner's contact list (+0x1e4, records with count +0x1b8 and body +0x1c0): up to 10
    contact points relative to the bottom are kept; the normals of contacts whose root mask has 0x200 are
    summed (projected to the tangent plane); the sum's length gives an average normal [game].
  - minZ = the lowest contact's z relative to the bottom; minR = the horizontal distance of that contact [game].
  - if the normal sum squared > 0.01: body velocity += 5 * average normal, each component clamped to 100000,
    and the pushed flag +0x34c is set [game].
  - the decision, with R1 = owner+0x140[slot] (0.8 gap), R2 = owner+0x170[slot] (radius), and fVar15 = owner
    +0x33c + +0x324 (the current height reference), [game]:
    - ENTER the test if any of: owner+0x3e4 is clear; the previous flag +0x26c is clear; a held body (+0x318,
      +0x31c) is present; fVar15 > +0x330 + 2.5; or (minZ present and 0.3 * R1 <= minZ).
      - inside: GROUNDED (+0x26c = 1, height = 0) if fVar15 <= +0x330 + 0.01, or (minZ < 0.9 R1 and minR < 0.9 R2).
        Otherwise NOT grounded, +0x334 = fVar15 - +0x330.
    - OTHERWISE: GROUNDED, +0x334 = 0, and a landing velocity correction: if the height gain is positive and
      +0x2b4 > 0.05, the vertical speed is capped (factor 30 and 0.8 for a negative value; capped at 20 and 1.0
      in one branch) and written by physics_body_set_velocity 00557000 [game].
- Falling: movement_owner_step lines 47-59 [game]: while +0x26c is clear and the body's vertical speed is <= 0
  (and not zero), the fall timer +0x3f4 counts up by dt; otherwise it is 0. The state machine's connectionFall
  (falling faster than 5, more than 3 above ground, state-machine.md line 166) uses this [game for the
  connection, the link to +0x334 is [assumed]].
- The earlier note's "grounded flag not found" is now answered: 008524b0 is the writer.

## 5. Constants on this path [game, from 008524b0 and 006e9550]

| value | where | role |
|---|---|---|
| 5.0 | 008524b0 | push: velocity += 5 x average normal |
| 0.01 | 008524b0 | normal sum squared threshold; height tolerance |
| 0.3 and 0.9 | 008524b0 | 0.3 R1 on the contact test; 0.9 R1 and 0.9 R2 on the grounded test |
| 2.5 | 008524b0 | height gain that forces the airborne test |
| 0.05 | 008524b0 | vertical speed threshold of the landing correction |
| 30.0, 0.8, 20.0, 1.0 | 008524b0 | landing velocity cap |
| 100000.0 | 008524b0 and 006e96c0 | velocity clamp |
| 2.5e-5, 1e-8 | 006e96c0 | tiny speed zeroing |

Slope limit: NOT FOUND [game for the absence in 008524b0 and 006e2c10 lines read]. Step-up height: NOT FOUND.

## Open points and next reads

1. Settle the slot mapping (+0x130/+0x140/+0x150 versus the three shape outputs): raw_disasm of 006e7b60 from the
   pushes before the call at 006e7c86, or emulate 006e7e90 with its stack arguments set.
2. The contact list writer and the record layout (owner +0x1e4, records at +0x1b8, +0x1c0, +0x1c4); the flag
   +0x3e4 and the held-body fields +0x318/+0x31c; the writer of +0x330 (the previous ground height).
3. The tail of 00852220 (after line 60) and the caller of 00852220 (the owner update).
4. physics_world_step 006df7c0: where body+0x18..0x20 is set and the gravity vector's sign.
5. Caller of 00737120 (probably a vtable): not found.

## Next function to read

- 00852220 tail (lines 60 to end) and its caller, the owner's update; then 006df7c0 around the gravity call.

## Rules kept

No game process, no file written under the install, no Rust written. Export\decomp not opened. Ledger entries
written for 008524b0 and 00852220 (game); 006e9550, 00737120, 006e7b60, 006e7e90, 00853550 were read and already
in the ledger.

## The slot mapping, settled in disassembly (manager, 2026-10-12)

[game] `006e7e90` takes eight stack arguments; its three outputs are written at 006e7fae
(`*p8` = gap = 5/11 of the cylinder = 30/121 H), 006e7fbd (`*p6` = 0.8 x gap = 24/121 H) and
006e7fcd (`*p7` = half the cylinder + gap = 63/121 H). `006e7b60` pushes its own stack
arguments [ebp+0x10], [ebp+0x14], [ebp+0x18] as p6, p7, p8 (pushes at 006e7c0d, 006e7bfe,
006e7be1) and afterwards stores `*[ebp+0x10]` at body+0x50, `*[ebp+0x14]` at body+0x54 and
`*[ebp+0x18]` at body+0x58 (006e7dbc to 006e7dde). So:

- body+0x50 = 24/121 H (0.8 x gap; owner slot +0x140, as `docs/climbing.md` has it);
- body+0x54 = 63/121 H (owner slot +0x130): the height of the body's CENTRE above the feet,
  since the cylinder's lower rim is 3/11 H under the centre and the gap is under that. This
  is what `006e9550` multiplies by the up vector's length: "the avatar's bottom" is the level
  of his feet, not the cylinder's rim;
- body+0x58 = 30/121 H (the gap; owner slot +0x150).

The cone's tip is H/2 under the centre, so 2.5/121 H above the feet (0.11 m for the robot's
5.4): on flat ground the pencil does not touch. What holds him at that height is therefore
not the pencil resting on its tip. [assumed] it is the ground test's push (008524b0 adds to
the velocity when there are contacts) against gravity, or a rule not yet read.
