# 008524b0 character ground test: line-by-line reading (reader ticket, 2026-10-12)

Scope: 008524b0 in full (disassembly checked), 00852220 from line 60 to its end (plus its lines 1-59 where they touch the fall timer), the writers of the contact list (+0x1e4) and of +0x330, and the emulator question. Static reading only. Nothing run, nothing written under the install. Tags: [game] read in code (disassembly unless said), [assumed] a guess with what would settle it, [unread] not read.

CORRECTION TO character-ground-rule.md: the flags and heights are NOT on the owner. In 008524b0 the first argument (ebp+8, = ECX in 00852220) is the movement step record, call it STEP. Its +0x38 is the owner (the character object). So "owner +0x26c", "owner +0x330", "owner +0x3e8" in the earlier notes mean STEP+0x26c, STEP+0x330, STEP+0x3e8 [game]. Only the owner-side fields are: owner +0xa4 (avatar), +0x120..+0x134 (frame), +0x1e4 (contact list).

## 0. Records and constants

Records [game, from the disassembly of 008524b0 and 00852220]:
- STEP (arg): +0x10 form object (vtable +0xc = form id; must be non-null, the first call dereferences it); +0x2c object O2 (vector at O2+0x1c, +0x20, +0x24); +0x2d0 byte gate; +0x244 slot index; +0x140[slot], +0x170[slot] floats; +0x26c grounded byte; +0x2ac, +0x2b0, +0x2b4 floats (copied to +0x340..+0x348); +0x318, +0x31c held-handle pointers; +0x324, +0x33c, +0x330, +0x334 floats; +0x34c, +0x34d bytes (outputs); +0x3e4 byte; +0x3e8..+0x3f0 (velocity times dt, written by 00852220); +0x3f4 fall timer; +0x38 owner.
- Owner: +0x120/+0x124/+0x128 = U (up column), +0x12c/+0x130/+0x134 = P (position), +0xa4 = Y (avatar), +0x1e4 = contact list head.
- Y: +0x54 = gap (float); *Y = physics body, body +0xa0/+0xa4/+0xa8 = linear velocity.
- Contact node: +0x1b8 point count n; +0x1c0 body A; +0x1c4 body B; +0x1cc next node for body A; +0x1d0 next node for body B; point k at +0x24+0x58k (x,y,z); normal at +0x30..+0x38 (block 0 only, see step 4).
- Other body flags: word at (other body)+0x9c; bit 0x200.

Constants (read from the executable, pe_read):
| value | address | used at |
|---|---|---|
| 0.01 (0.00999999978) | 0xb343a0 | normal sum squared > 0.01 (852b34); height tolerance (852fcc) |
| 1e-8 | 0xb345d8 | unit normal guard (852b5e) |
| 1.0 | 0xb38834 | normal division numerator; landing vz cap |
| 5.0 | 0xb3476c | push 5 x unit normal |
| 100000 | 0xb3889c | velocity clamp upper |
| -100000 | 0xcae3ec | velocity clamp lower |
| FLT_MAX | 0xb387b4 | initial min z and min R |
| 2.5 | 0xb3473c | height gain forcing the test |
| 0.3 | 0xb2a0e4 | 0.3 R1 entry test |
| 0.9 | 0xb3429c | 0.9 R1 and 0.9 R2 grounded test |
| 2.0 | 0xb38838 | cone test (2 dot^2) |
| 0.05 | 0xb34754 | landing: vertical speed threshold |
| 20.0 | 0xb347d4 | landing: cap test |
| 30.0 | 0xb34570 | landing: gain factor |
| 0.8 | 0xb6a3f0 | landing: negative speed factor |
| -0.0 | 0xb6a3ac | sign flip (80000000) |
| 1.25 | 0xb3450c | STEP+0x330 at construction (008511f0 at 008512b9) |
| drive form id 0xe2b22eb4 (= -0x1d4dd14c) | | |
| fall form id 0xd7068c31 (= -0x28f973cf), 0xfb9f3262 (= -0x460cd9e) | | 00852220 |

## 1. 008524b0, step by step (disassembly addresses)

Entry 8524b0 (stdcall, ret 4). Clears STEP+0x34c and +0x34d (8524c3-8524ca).

S1. Form test (8524d1-8524e8): id = STEP+0x10 vtable +0xc call.
- If id == 0xe2b22eb4 (drive): if STEP+0x10 is 0, STEP+0x26c = 0 and return (8524fc); else STEP+0x26c = any_wheel_on_ground 00851140 and return (8524e8). This path returns WITHOUT the push, the grounded decision, and the call to 853420 [game].
- Otherwise go to S2.

S2. Bottom point (85250c-85257d) [game]:
  Avatar Y = owner+0xa4, gap = Y+0x54.
  bottom_x = P_x - U_x*gap (owner+0x12c - owner+0x120*gap), bottom_y = owner+0x130 - owner+0x124*gap, bottom_z = owner+0x134 - owner+0x128*gap. Stored in locals.
  List head L = owner+0x1e4.

S3. Walk the contacts (8525a0 onward, loop until the node is 0). For each node N:
  - other = N+0x1c0; if other == owner then other = N+0x1c4 and the next pointer is N+0x1cc, else next is N+0x1d0. (So the next pointer is chosen by which side the owner is on; the owner side is +0x1c0 when it matches, +0x1c4 otherwise. [assumed] that when owner is not +0x1c0 it is +0x1c4, as the code implies; the inserter below agrees.)
  - n = N+0x1b8 (point count). Blocks k = 0..n-1 at base 0x24+0x58k.
  - POINTS (852603-852625): for k with stored count c < 10: store relative point (x_k - bottom_x, y_k - bottom_y, z_k - bottom_z) at buffer B[c]; c++. Stops storing after 10 points.
  - NORMAL (852641-8526b0): if normal count nn < 1 and word [other+0x9c] & 0x200 != 0: take the normal at N+0x30..+0x38 (block 0, the same for every block) and store the TRANSFORM: with x,y,z the raw normal and k = x*x + y*y + z*0.0 (xmm7 = 0.0; the z square is NOT in k [game], confirmed in the disassembly 852664-852679):
      stored = ( x - x*k,  y - y*k,  -0.0 - z*k ).
    nn becomes 1. Only ONE normal is ever stored (the guard "nn < 1" at 852641 and 8526b0). So the later "average" is the single stored vector.
  - next node: N = next pointer chosen above.

S4. Sum and push (852a11-852c77):
  - Sum S of the stored normals: with nn <= 1, S = the stored vector (sums at ESP+0x14/18/1c, x, y, z).
  - b8 = Sx^2 + Sy^2 + Sz^2 (852b17-852b30). If b8 > 0.01 (852b34): len = sqrt(b8) (sqrt 00409a70); if len > 1e-8 then unit u = S / len (852b8d); else u = S.
  - Velocity read: character_body_velocity 0070a990 into V (852bc6). It reads *Y body velocity (or zero if Y or *Y is null).
  - V' = V + 5.0 * u, component-wise (852bd3-852c02).
  - Clamp each component to [-100000, 100000] (852c1e-852c59): if c <= -100000 then -100000; else if c >= 100000 then 100000; else c.
  - Write V' to body (*Y)+0xa0, +0xa4, +0xa8 (852c5f-852c6f) and STEP+0x34c = 1 (852c77). This is a direct store, not the setter.
  - If b8 <= 0.01 nothing is pushed and +0x34c stays 0.

S5. Minimum height (852c80-852dcc) [game]:
  - minZ = FLT_MAX, minR = FLT_MAX initially. For each stored point p = (x, y, z) in B (in order): if z < minZ then minZ = z and minR = sqrt(x*x + y*y) (852d20-852d5a). Note minR is the horizontal distance of the point that gave the current minimum.
  - Cone test (852d66-852dbf): V = O2+0x1c.. (vector a = O2+0x1c, b = O2+0x20, c = O2+0x24). dot = a*x + b*y + c*z (the z term IS used, 852d6a-852d90). If dot > 0 and x*x + y*y < 2*dot*dot then STEP+0x34d = 1 (852dbf).
  - The +0x34d flag has no reader in 008524b0 [game]; its reader is [unread].

S6. Gate (852dd2-852dda) [game]: if STEP+0x2d0 == 0, skip everything below to the call of 853420 (S9). STEP+0x26c is NOT written on that path.

S7. Held handle (852de0-852e28) [game]:
  - H = STEP+0x31c. If H != 0: if [H+8] < 0, r = call 00532cc0 (an atomic handle path); else if [H+4] == 0 then r = 0 (no hold); else [H+4] += 1 (atomic-free count) and r = [H+4].
  - holdPresent = (r != 0) and STEP+0x318 != 0 and [STEP+0x318 + 0xa4] != 0. Then esi = H (to release at S9).

S8. Grounded decision (852e2d-85302c) [game]:
  Let h = STEP+0x33c + STEP+0x324 (the current height; fVar15 in the earlier note), minZ, minR as above.
  ENTER (go to test T) if any of:
  - STEP+0x3e4 == 0;
  - STEP+0x26c == 0 (previous grounded flag clear);
  - holdPresent;
  - STEP+0x330 + 2.5 < h (852e62-852e78);
  - 0.3*R1 <= minZ and minZ != FLT_MAX (852e7e-852ea5), where R1 = STEP+0x140[slot] (slot = STEP+0x244).
  If not ENTER: OUTSIDE branch O.
  Test T (852fc1-85300d-85301d):
  - if h <= STEP+0x330 + 0.01: GROUNDED (STEP+0x26c = 1, STEP+0x334 = 0);
  - else if 0.9*R1 > minZ and 0.9*R2 > minR (R2 = STEP+0x170[slot]): GROUNDED (same);
  - else NOT grounded: STEP+0x26c = 0, STEP+0x334 = h - STEP+0x330.
  Outside branch O (852eab-852fbf):
  - STEP+0x26c = 1, STEP+0x334 = 0.
  - Copies STEP+0x2ac, +0x2b0, +0x2b4 into +0x340, +0x344, +0x348 (852ee2-852ef2) before the test.
  - gain = h - STEP+0x330. If gain > 0 and STEP+0x2b4 > 0.05: landing:
    - V = character_body_velocity (post-push velocity, 852f15-852f17).
    - dot = V.y*(+0x344) + 0*(+0x348) + V.x*(+0x340) (852f1c-852f43: the z component of V is multiplied by zero, the +0x348 term is 0*C; note the x87-free SSE shows xmm1=0). Then dot = dot / (+0x348) + 30.0*gain (852f4f-852f65).
    - vn = -0.0 - dot (852f69-852f71). If vn < 0 then vn = 0.8*vn (852f75-852f7a).
    - If V.z > 1.0 and V.z - vn > 20.0 then vn = 1.0 (852f82-852fa5).
    - If vn <= V.z then V.z = vn (852fa8-852fad).
    - physics_body_set_velocity 00557000 with ECX = owner (STEP+0x38) and EAX = &V (852fb3-852fba). This is a SET of the whole vector, not an add.
  - Else no landing.
  Inside test T, no landing and no snapshot beyond the common one.
  Common tail (85302c-85305e): STEP+0x340/+0x344/+0x348 = STEP+0x2ac/+0x2b0/+0x2b4 again.

S9. Release the held handle (85305e-853079): if esi != 0, decrement its count (+0x4 or lock xadd on +0x4 when [+8] < 0); if it reaches 0 call 00532c70.

S10. Call 00853420 (85307e-853080) [game, read in full]: a function whose ESI is STEP. It writes STEP+0x338 (from +0x324), +0x2e4, +0x2e8, +0x2d4, +0x2d8, +0x2f5, +0x270..+0x290 (uses owner+0x2c object and STEP+0x330 and +0x33c), and calls 00854df0 controller_is_ai, 00532c40 and 006e0a30 [unread]. Only when STEP+0x2d1 == 0 or STEP+0x2d0 != 0. It does not write +0x26c, +0x330, +0x334 or the velocity.

Ret 4.

## 2. Writers of the inputs

- Contact list +0x1e4 and the nodes [game]:
  - 006d7280 inserts a node at the head of BOTH bodies' lists: the old head of body A (edi) goes to N+0x1cc, the old head of body B (ebx) to N+0x1d0 (006d72dc-006d72fa). So +0x1cc is the link for the body at +0x1c0 (= A), which matches S3.
  - Its only caller is 006e6a10 physics_contact_generate (x1), when its param_8 flag is set [game, from the ledger note; ledger caller check].
  - 006d7230 walks a body's +0x1e4 list and unlinks nodes whose +0x1c0 (or +0x1c4) is that body (ledger note: collision-contact-generator.md lines 19 and 123) [game for the walk, partly read].
  - The node count +0x1b8 and the points are written by the generator (collision-contact-generator.md; not re-read here) [unread here].
- +0x330 [game]: written ONCE, to 1.25, in 008511f0 (008512b9), the constructor of the step (called once from 00712bb0). The search for the 32-bit displacement 0x330 found no other writer; the one other hit (00853420) is a misaligned decode. The value is therefore constant for the life of the step [game, search-based; [assumed] no computed-offset write exists].
- +0x26c: written by 008524b0 (S1, S8) and at construction by 008511f0 (008512a0, value of bl not read) [game].
- +0x3e4, +0x2d0, +0x318, +0x31c, +0x140/+0x170, +0x244: [unread] writers.

## 3. 00852220 movement step (lines 1-76 read in full, disassembly 852220-852470)

1. Velocity copy (lines 1-17, 852220-8522a5): V0 = *(*(STEP+0x38)+0xa4) body velocity (zero if Y is null). STEP+0x3e8, +0x3ec, +0x3f0 = V0 * dt (dt is the argument, param_2). These are on STEP, not the owner.
2. 008524b0(STEP) (line 18, 8522ad).
3. avatar_transition_update 00854160 (line 19, 8522ba) [unread body; the ledger says it decrements a dt and recomposes the blended avatar affine].
4. If STEP+0x10 != null and STEP+0x38 != 0 (lines 20-21): a block sets two locals (local_c, local_8) from STEP+0x34c and the form id at lines 22-37; that block was read only in the decompile and its exact meaning is [unread]. Then FUN_006e8da0(1) (line 39, unread). Then the form vtable +8 call with dt (line 41).
5. Form id test (lines 43-46): if id != 0xe2b22eb4 and FUN_008521e0(0xe2b22eb4) != 0, call 00851170 (unread).
6. FALL TIMER (lines 47-59, 852410-852450) [game]:
  - If STEP+0x26c == 0 (not grounded): V = character_body_velocity (after 008524b0's push and landing). If V.z < 0 (strict; the test is "0 <= V.z skip", 852421-852428): fall = STEP+0x3f4 + dt; otherwise fall = 0. Then STEP+0x3f4 = fall (85244e). This is the only place the fall timer is counted.
  - If STEP+0x26c != 0 (grounded): fall = 0 only if the form id (vtable +0xc) == 0xd7068c31; otherwise STEP+0x3f4 is left unchanged (852442-852449; the write at 85244e is skipped). [game]
7. Lines 60-76 (852456-852470 and on): if owner != 0 (STEP+0x38), the form id == 0xfb9f3262 clears owner+0x1d4 bit 0x2000, else sets it (lines 61-71, [game]); then the vtable +8 of STEP+0x20 called with 0xb: if it returns 0, STEP+0x3f8 = 0 (lines 72-75).
8. Anything that changes vertical position or velocity in 00852220: nothing writes position. Vertical velocity changes only through 008524b0 (the push, S4, and the landing set, S8) and through callees not read (00854160, FUN_006e8da0, FUN_008521e0, 00851170, the form vtable +8). [game for the direct writes; the callee effects [unread].]

## 4. The contact normal: what is stored (the odd part)

- Only the FIRST contact whose other body has flag 0x200 contributes, and only its block-0 normal (n = N+0x30..+0x38), and it is the transformed vector (x(1-k), y(1-k), -z k), k = x^2+y^2 (z excluded). Then it is normalised and used as the push direction. So the push is not the contact normal. [game, disassembly 852655-8526b0 and 852b8d]
- [assumed] this transform is a quirk of how the game computed a frame (the decompile labelled it "contact frame construction" via sqrt). It is what the game does, so it is ported as read. Settles by: an emulator run of 008524b0 with a fixed contact (the harness below), or a recording of a landing on a sloped contact.
- The 0.0 in the decompile is the real xmm7 = 0 (xorps at 852550, 852c5c, 852cbd, 852d09, 852d60), not a decompiler artefact [game].

## 5. Emulator feasibility (question 4)

Self-contained enough, with stubs. The function reads plain memory and calls:
- vtable call [STEP+0x10]+0xc (form id): a fake object with a vtable slot returning 0xe2b22eb4 (drive) or another id; must be non-null.
- 00851140 any_wheel_on_ground: only on the drive path (STEP+0x10 wheels, +0x90 flags); stub it.
- 0070a990 character_body_velocity: pure read; in_EAX = STEP (it reads STEP+0x38, owner+0xa4, *Y, then +0xa0..+0xa8). Output to the pointer argument. Emulate for real (it is small).
- 00409a70 sqrt: a CRT-style x87 wrapper (fld/fstp around the call); emulate with the FPU or stub with sqrtf (the result is the same single-precision value).
- 00557000 physics_body_set_velocity (ECX = owner, EAX = &vector): stub and capture the vector.
- 00532cc0, 00532c70, 00532c40 (held handles): stub; reached only when STEP+0x31c or +0x2d8 is non-null.
- 00853420: called at the end of every non-drive path; it writes STEP fields only (listed in S10) and calls 00854df0 controller_is_ai, 00532c40 and 006e0a30 [unread]. For a pure test, stub it (008524b0 does not read those outputs) or emulate with the four callees stubbed.
- Memory to lay out: STEP (at least 0x350 bytes), O2 (+0x1c..+0x24), owner (0x140 bytes: +0x120..+0x134, +0x1e4, +0x38, +0xa4), Y (+0x54, pointer to body), body (+0xa0..+0xa8, velocity), form object with vtable, contact nodes (0x1d4 bytes each: +0x1b8, +0x1c0, +0x1c4, +0x1cc, +0x1d0, points at +0x24+0x58k, normal at +0x30), the other bodies (+0x9c word), held handle (optional).
- Not settled: the slot index (+0x244) and the arrays +0x140/+0x170 (see section 6); emulator on 006e7e90 with its stack arguments would settle them.

## 6. Still open

1. Slot mapping (+0x130/+0x140/+0x150 versus the three shape outputs): as the earlier note, still [assumed]. Not re-read here.
2. The contact node creation and the block count +0x1b8 (006e6a10 read in its own note); the walk order of the list (head insertion, so the most recent contact is first; the first 0x200 contact in list order is the one used).
3. +0x2ac/+0x2b0/+0x2b4 writers, +0x2d0, +0x3e4, +0x318/+0x31c writers [unread].
4. Meaning of the cone test (S5) and of +0x34d [unread reader].
5. Whether 008511f0 writes +0x26c with bl = 0: the value of bl not read.
6. 006e0a30 (called by 00853420) [unread].
7. Caller of 00852220 (the owner update) and physics_world_step's gravity vector: not re-read here (see character-ground-rule.md section 3).

## 7. Corrections to earlier notes

- Fields are on the step record, not the owner: +0x26c, +0x330, +0x334, +0x3e8..+0x3f0, +0x3f4 (character-ground-rule.md section 4 says owner).
- The push uses ONE normal (the first 0x200 contact's transformed vector), not a sum of several (character-ground-rule.md section 4 says the normals are summed).
- The fall test is vz < 0 strictly, not <= 0 and nonzero (equivalent in practice).
- The grounded decision is gated by STEP+0x2d0 (skipped when zero), which the earlier note did not record.
- The earlier note's "contact list writer: not found" is now: 006d7280 (head insert on both bodies), called from 006e6a10.
- The +0x330 writer is 008511f0 (constructor, value 1.25); no per-frame write exists.
