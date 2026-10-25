# Character height fields: STEP+0x324, +0x33c, +0x140/+0x170, +0x2d0, +0x3e4 (reader ticket, 2026-10-12)

Scope: who writes the height fields of the movement step record STEP (see character-ground-test.md for 008524b0 and 00852220). Static reading only; nothing run; nothing written under the install. Tags: [game] read in code (disassembly where said), [assumed] a guess with what would settle it, [unread] not read. Where the decompiler's argument list was wrong, the call was checked in disassembly and this is said.

## 1. STEP+0x324 and STEP+0x33c (the current height h = +0x33c + +0x324)

Writers found, STEP record only [game]:
- STEP+0x33c: written 0 in movement_owner_reset 00851600 (line 1623 of the decompile) [game]. It is not written by the constructor 008511f0 (checked lines 1-50) and no other writer was found in 0084f000-00856000. Readers: 008524b0 (852e38), 853420 (853420 at 8534d5 and its line 2930, where it writes STEP+0x290).
- STEP+0x324: NO writer found. Its displacement occurs in the STEP range only as a read: 008524b0 (852e3c, addss) and 853420 (2891-2893, see below). The constructor does not write it either. So its value comes from the memory the record was allocated in, or from a writer I could not find [assumed: a write through a computed pointer; settled by a breakpoint-free search for the STEP base pointer, or by reading the callers of 00852220].
- Writers of +0x324 that exist in the export, all NOT the STEP record [game, by what else they write]:
  - 0071b990 aim_stick_update (weapon-mode aim assist share; docs/weapon-mode.md).
  - 0086b230 flight-form update (census: flight): it writes +0x31c = 1.0 as a float, while STEP+0x31c is a held pointer (008524b0 reads it as a pointer), so it is a different record.
  - 00566e90 EnvironmentEvent (writes +0x324, +0x330..+0x33c as a block).
  - 00440000 and 00443770: platform (FMOD) code, value 0xac44.
  - 00a90000: DAT_00cfff30 global.
- The sentinel: 853420 tests +0x324 against 3.4028235e+38 (0x7f7fffff, FLT_MAX) [game, 00850000.c:2892]. If it is not FLT_MAX, it copies +0x324 to STEP+0x338 (2893) [game]. 00851600 sets +0x338 = FLT_MAX [game]. So FLT_MAX means "no height measured" [assumed, settled by a recording or by the writer of +0x324].
- The ground test's h = +0x33c + +0x324 (008524b0, 852e38-852e44 and 2634 in the decompile) [game]. Its only use is the comparisons in character-ground-test.md S8.

The ground query that DOES exist does not feed +0x324:
- 00737120 character_ground_snap [game]: a ray from (x, y, z+5) to (x, y, z-20), radius 0, mask 0x14 (GROUND | AVATAR_PUSH), at the character's position (character-world-probes.md). Its hit is copied to locals and passed to FUN_00737720.
- 00737720 [game]: "Writes no height" (ledger). It spawns an effect or random jitter at the hit point and pushes the hit into a global ring (00574920). So 00737120 is an effect probe, NOT the height source of STEP+0x324.
- So: no function found yet writes the height above ground into STEP+0x324. Next to read: the callers of 00852220 (the owner update that runs 008524b0) and whichever code runs on STEP before 00852220 each step; the search for the STEP base pointer in the callers.

Answer to "is one of them the height above the ground?": [assumed] yes, +0x324 is the height above the ground under the character (it is the first addend of h, it is compared with 1.25 + 2.5, and it is copied to +0x338 which the camera and state code read). Nothing in the code read here proves it. [game] The ground probe that exists (00737120) is not it.

## 2. STEP+0x140[slot] and STEP+0x170[slot] (R1 and R2)

Slot: STEP+0x244 is an integer, the index of the selected form [game]. It is written by movement_owner_update 00853cb0 as `*(int*)(param_1+0x244) = in_EAX` (the index of the form switched to, after the switch) [game, 00850000.c:3347]. The constructor sets it 0 (+0x244 is unaff_EDI[0x91]) [game]. Note: other classes write +0x244 as a float, so the int is for STEP only [game].

Filled by the avatar creation. The avatar index is iVar2 = STEP+0x240, the count of avatars, which first_avatar_create increments [game].
- first_avatar_create 00853550 (the first avatar, index 0) calls pencil_avatar_create 006e7b60 with the array addresses, then writes +0x160[i] = param_1 and +0x170[i] = param_2 (the 2nd argument) [game, disassembly 853550-853610].
- The caller is avatar_descriptor_dispatch 00851870 [game]: first_avatar_create(fVar4, fVar7, fVar5), where fVar7 = descriptor[+8] times max(scale a, scale b) and that is the form HEIGHT H (pencil_avatar_shape takes it as h).
- The pencil call (006e7b60): disassembly of 853550 gives the push order. Decompile labels were one argument off (it lists local_14 as the first argument). Resolved order after `this`: buffer (out), body STEP+0x60+i*0x34, STEP+0x140+i*4, STEP+0x130+i*4, STEP+0x150+i*4, O2 (STEP+0x2c), the three floats, the local, flags [game, disassembly].
- pencil_avatar_create then stores body+0x50 = *(STEP+0x140 slot), body+0x54 = *(STEP+0x130 slot), body+0x58 = *(STEP+0x150 slot) [game]. movement_owner_update 00853cb0 writes the same three back the same way (body+0x50 from +0x140, +0x54 from +0x130, +0x58 from +0x150) [game]. The two agree.
- pencil_avatar_shape 006e7e90 (h = H, r = radius) [game]: gap = (H - 5/11 H) * 5/11 = 30/121 H; the 0.8 gap output = 24/121 H; the centre output = gap + 3/11 H = 63/121 H.

So:
- R1 = STEP+0x140[slot] = 0.8 times the gap = 24/121 of the form height H [game]. For a 5.4 m robot: R1 = 1.071 m; 0.3 R1 = 0.321 m; 0.9 R1 = 0.964 m.
- R2 = STEP+0x170[slot] = the form height H itself (the 2nd argument of first_avatar_create) [game]. For a 5.4 m robot: R2 = 5.4 m; 0.9 R2 = 4.86 m. This matches the ground test's use: R2 is compared with minR, the horizontal distance of the lowest contact point [game].
- Caveat [assumed]: the scale factors in avatar_descriptor_dispatch (the 'scale' that multiplies the descriptor's numbers) are not read. The 5.4 m figure assumes scale 1. Settles by reading the descriptor's values and the scales in 00851870's callers.
- Other creators (FUN_00853860, FUN_008536b0, vehicle_avatar_create 00853a10, FUN_00853bd0) also write +0x140/+0x170 (00850000.c lines 3029-3261) with different arguments; not read. For the vehicle and the other forms, R1 and R2 may differ [unread].

## 3. STEP+0x2d0 (gate), STEP+0x3e4, STEP+0x2ac..+0x2b4

- +0x2d0 (byte): read by 008524b0 (S6: 0 skips the grounded decision, the velocity push and the landing; 852dd2-852dda) and by 853420 (2891: 853420 writes +0x338 etc. only if +0x2d1 == 0 or +0x2d0 != 0) [game]. No writer in the STEP range. Leads, not identified: 009f0000:2039 (`*(int*)(param_1+0x2d0) = param_2`), 00480000:9071 (byte from param_4). [unread].
- +0x3e4 (byte): read by 008524b0 (ENTER test: 0 means enter the test, 852ee2 area, line 2635 of the decompile) [game]. No writer for STEP+0x3e4 in the STEP range. The MoveMode writes at 00850000.c:5130, 5155, 6405, 6492 write `*(param_1+4)+0x3e4`, which is the owner-side flag of a different object (bumblebee-mechanics.md). Note: the ground test reads STEP+0x3e4 directly, while the MoveMode writes through +4. [assumed] these are two different fields; settles by reading the function at 00850000.c:5130 to see what +4 is. Other +0x3e4 writers (00470000, 00510000, 004f0000, 007a0000, 00860000) are not STEP-checked [unread].
- +0x2ac, +0x2b0, +0x2b4 (floats): copied to +0x340..+0x348 by 008524b0 and read by the landing (S8) [game]. No writer in game code found: the displacement matches only in FMOD (platform) ranges (00420000, 00430000, 00470000, 0047e300, 00490000) and in 008524b0 and 853420 reads. [assumed] they are set by something outside game code or through a pointer; settles by a search for the STEP base pointer.
- +0x318 and +0x31c (held handle pointers): 00850000.c:813 writes `local_8 + 0x318` (a local pointer, not STEP; its function is not identified, [unread]). +0x31c is read in 008524b0 and 853420 (2616). No other writer found.

## 4. Does the height move the character's z, or the vertical velocity?

- 008524b0 writes no position [game]. 00852220 writes no position either [game] (character-ground-test.md section 3).
- Vertical velocity changes in 008524b0 only in two places [game]:
  1. The push: body velocity += 5.0 times the unit normal (the single stored contact normal), each component clamped to +-100000 (S4). This is applied whenever the ground test has contacts with the 0x200 flag and |S| > 0.1 (b8 > 0.01).
  2. The landing: when gain = h - +0x330 > 0 and +0x2b4 > 0.05, the vertical velocity is set to vn (the landing speed, S8) through physics_body_set_velocity 00557000 [game].
- So: there is NO snap to the ground and NO spring on z from h. The height is used only to decide grounded (+0x26c), to set the height excess +0x334 (= h - 1.25 when not grounded, 0 when grounded), and to trigger the landing velocity [game].
- +0x334 readers: none found in 0084f000-00856000 (the only accesses there are its writes, 00850000.c:2649-2653) [game]. Its readers elsewhere were not searched [unread]. Its construction value is 1000.0 (0x447a0000 at constructor line 43) [game].
- +0x330 = 1.25 at construction (0x3fa00000, constructor line 26) and never written again [game].

## 5. Summary for a port

- h = +0x33c + +0x324 is the height above the ground under the character. Its writer is NOT found. [assumed] it is measured by a downward ray from the character; no such writer is found in the game code read here.
- Grounded: h <= 1.25 + 0.01, or (0.9 R1 > minZ and 0.9 R2 > minR) with R1 = 24/121 H and R2 = H of the current form.
- Not grounded: +0x334 = h - 1.25 (an excess the game does not snap away).
- Nothing moves z from h. Only the velocity push (5 times the unit normal) and the landing velocity set do.

## 6. Ledger and rule notes

- Ledger (tag game): 008511f0 (constructor), 0086b230 (flight: not STEP), 00851870 (avatar_descriptor_dispatch). Existing: 00851600, 00853cb0, 00853550, 006e7b60, 006e7e90, 00737120, 00737720.
- Rule broken: none known. Nothing was run; nothing was written under the install. The decompile of 00850000.c was searched with grep only, not read; 006e7b60 and 00853550 were read through fn.py and disassembly.

## 7. SOLVED 2026-10-12: STEP+0x324 is the result of an async ray probe embedded at STEP+0x270

Sections 1 and 3 above looked for a writer that addresses STEP. There is none: the fields belong to a probe object EMBEDDED in the record, and the physics world writes them through the probe's own pointer.

Who holds STEP [game]: 00712bb0 (called once, from character_constructor 00713300) allocates 0x410 bytes, constructs STEP and stores it at character+0x104. 00852220 is not called by address: it is slot 1 of STEP's vtable (00b62650; the pointer sits at 00b62654), so it has no direct callers.

The probe [game]:
- The constructor 008511f0 does `lea eax,[edi+0x270]` then calls ik_probe_initialize 0073a370 (disassembly 85129a-8512a6). So PROBE = STEP+0x270, the same probe class FootIK embeds at its +0x50. Its initialiser sets probe+0xb4 = FLT_MAX, which is STEP+0x324: the record is NOT left uninitialised (section 1 was wrong to say so).
- Map (probe offset = STEP offset - 0x270): +0x00..+0x0c start (STEP+0x270), +0x10..+0x1c end (+0x280), +0x20 radius (+0x290), +0x30..+0x38 hit point (+0x2a0), +0x3c..+0x44 hit normal (+0x2ac..+0x2b4), +0x60 done byte (+0x2d0), +0x61 pending byte (+0x2d1), +0x64/+0x68 ignore handle (+0x2d4/+0x2d8), +0x74 and +0x78 masks (+0x2e4, +0x2e8), +0x85 (+0x2f5), +0xa0/+0xa4 and +0xa8/+0xac held handles of what was hit (+0x310/+0x314, +0x318/+0x31c), +0xb0 (+0x320), +0xb4 hit distance (+0x324).
- Submit, every step: 00853420 (end of the ground test), when not pending or already done, fills the probe and calls probe_submit 006e0a30 with EAX = STEP+0x270 (disassembly 853530-853537). The ray: start = O2 position (owner+0x2c object, +0x34..+0x3c) with z + STEP+0x330 (1.25); end = the same point 10 m lower; radius = STEP+0x33c (0 after 00851600); mask +0x78 = 0x41 for the player, 0x09 for an AI character; mask +0x74 = 0; ignore handle = STEP+0x2c/+0x30.
- 006e0a30 [game]: links the probe at the tail of the world's probe queue (world = DAT_00cf59f0, head +0x28034, tail +0x28038, links at probe+0x88/+0x8c), sets done = 0, pending = 1, trims the segment by the radius (006e1960), drops both held handles, sets +0xb4 = FLT_MAX, and registers the swept box in the broad phase (006e0b40) or queues it.
- World step 006e5e00 lines 260-324 [game]: walks the queue; each broad-phase candidate body that passes the masks (probe+0x74 against body+0x9e, +0x78 against body+0x9c, not +0x7c) becomes a job; the probe is unlinked (006e0e50: pending = 0), +0xb4 = FLT_MAX again, and done = 1 at once when there was no candidate. Jobs run through 006df050 (the shape's ray test, queued).
- THE WRITER, probe_results_collect 006e2a10 [game], called from 006e5e00 after the hit loop: pops every result record (005be160; 0x3c bytes each) and, when record[10] < probe+0xb4, writes probe+0xb4 = record[10] (STEP+0x324), the hit point, the normal (+0x3c..+0x44 = STEP+0x2ac..+0x2b4), +0xb0, and the two held handles (STEP+0x318/+0x31c). So the nearest hit wins. Then every probe that had jobs gets done = 1 (STEP+0x2d0).

What this settles from sections 1 and 3 [game]: the writers of +0x324, +0x2ac..+0x2b4 (the ground normal: the landing's `+0x2b4 > 0.05` is normal z), +0x2d0 (probe finished), +0x318/+0x31c (what was hit). FLT_MAX = no hit within 10 m.

So for a port: h = radius + distance from (position + 1.25 up) straight down to the nearest hit, at most 10 m, masks as above; the value read in a step is the result of the ray submitted at the end of the PREVIOUS step (one step late), and while a probe is still pending the old value stays.

[assumed] record[10] is a distance in metres along the ray, not a 0..1 fraction: with a fraction h could never exceed 1.26 and the character would always be grounded; and 1.25 is exactly the ray's start offset, so standing gives h = 1.25. Not read in code: the records come from the scratch array DAT_00d30940 filled by 005a69a0. Settles by reading 005a69a0 or by a recording of STEP+0x324.
[assumed] mask bits 0x41 / 0x09 are named by the mask table in character-world-probes.md; the exclude mask probe+0x7c (STEP+0x2ec) has no writer read here.

Ledger: 006e0a30 probe_submit, 006e2a10 probe_results_collect, 006e0e50 probe_unlink, 006e1960 probe_trim_segment, 00712bb0 step_record_create.

## Still open (next reads, in order)

1. DONE (section 7). Left over: the unit of record[10] (read 005a69a0).
2. The descriptor scales in 00851870 (what makes H and whether scale is 1 for the robot).
3. FUN_00850000.c:813 (writer of +0x318): name the function.
4. The MoveMode writer at 00850000.c:5130 (what its +4 object is): decides whether STEP+0x3e4 is ever written.
5. The readers of +0x334 across the decompile (not searched).
