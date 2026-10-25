# Hit consumption and contact response: 006e2c10 lines 31-599 (reader ticket, 2026-10-11)

Scope: the body of 006e2c10 between the drain/sort header (lines 1-30, read by the broadphase
ticket) and the loop tail (600-644, also read there). Frame size `alloca 0x10440`. Tags:
`[game]` read in the decompiler text and, where marked, in disassembly; `[assumed]` inferred,
with what would settle it. Callees not read are listed in "Not read".

## 1. Step table

| lines | what it does | calls (address, ledger name, status) |
|---|---|---|
| 29-30 | normal n = rec+0xcc (x), copy into the local; node = list +0x28050[rec+0x104] | none |
| 31-33 | copy rec+0xd0, +0xd4 (n y, z) | none |
| 34-35 | **time gate**: accept only if rec+0xf8 <= holderA+0x244 and <= holderB+0x244 (disasm 006e2d64, 006e2d90) | none |
| 36-43 | node+0x32 == 0: null the list slot (`*piVar2 = 0`), i.e. drop the pair. Else set byte at pool object +0x3c + rec+0xf0*0x40 = 1 (per-shape hit flag) | none |
| 44-49 | masks: m = (B+0x9c & A+0x9e) \| (B+0x9e & A+0x9c); uVar19 = m & 0x4d. Zero means no response | none |
| 50-59 | orient n: if n · (A.origin - p) < 0, negate n (rec+0xcc..0xd4). A.origin = holder+0x12c..0x134 (translation), p = rec+0xc0..0xc8 | none |
| 60-83 | root bodies: +0x1fc (fallback self), +0x204 (fallback root). Drop (uVar19=0) if either root has bit 0x400 of +0x1d4 (lines 155-157), or if the two roots are the same or share an owner (+0x208 vs +0xc) | none |
| 84-99 | bVar9 = both holders have bit 0x1000 of +0x1d4. Tolerance fVar26 = 0.01, or 1.0 if a root has bit 0x10 | float_abs 00409a50 (game) |
| 100-138 | avatar ground override (only when exactly one holder has 0x1000): if the B (or A) mask has 0x10 and a root has 0x2000 (or mask 0x800), and the other side's +0xa4 is null, compare the contact height with the avatar bottom. On a hit: normal forced to (0,0,+1) or (0,0,-1), bVar10 = 0, and the function jumps to 159 (no commit) | 006e95c0 [unread], 006e9550 avatar_bottom_distance (game), 006e94e0 [unread] |
| 139-152 | set bit 0x4000 on holder A +0x1d4; if A+0xa4 body has +0x38 == 0 and +0x34 == 0.0: **commit A pose** holder_commit_affine(A, rec+0x30). Same for B (only if B+0xa4 non-null) with rec+0x90, bit 0x4000 set | 006dc3b0 holder_commit_affine (game) |
| 159-170 | (label 159). If uVar19 set: if (vA - vB) · n >= 0 then uVar19 = 0 (separating). Then write rec+0xf8 into holderA+0x244 and holderB+0x244 **regardless of the separating test** (disasm 006e3243, 006e3252) | none |
| 171-187 | copy to locals: point (rec+0xc0..0xc8), n (rec+0xcc..0xd4), block rec+0xd8..0xec (six floats) and two zero dwords | none |
| 188-200 | uVar18 = A+0x31 != 0 && A+0xc == 0 && hit (flag); uVar15 = A+0xc (previous contact) or 0 if no hit | none |
| 201-203 | **contact generation**: contact = physics_contact_generate(world, pair, prevContact, &block, A, B, rec+0xf0, flag) | 006e6a10 physics_contact_generate [unread past line 75] |
| 204-211 | if hit and contact != 0: pair+0xc = contact; contact+0x1e0 = pair; then 006e9460, 006e9360(A), 006e9460 | 006e9460 [unread], 006e9360 [unread] |
| 212-301 | **contact+0x1e8 == 0** (simple path). If A dynamic (A+0xa4 deref +0x38 == 0): normal/tangent frame (006deda0 with contact+0x1ea), pencil test 006e92f0, position slide on A (277-289), velocity from 00556c00, kill approaching normal velocity (291-298), write 00557000 | 006deda0 [unread], 006e92f0 [unread], 006e9250 [unread], 006e8f00 [unread], 00556c00 (game), 00557000 (game), 00409a70 sqrt |
| 303-398 | (else of "A dynamic") if B dynamic: the mirror of 212-301 on B, plus 006e9340 | 006e9340 [unread] |
| 400-599 | **contact+0x1e8 != 0** (full path, lever arms and inertia): invMass sum = A+0x28 + B+0x28 (lines 419, 502-532 use it, `[assumed]` inverse mass); inverse-inertia terms; normal and tangent effective masses 1/k (502-507, 527-533); **normal impulse** lambda = -(relative normal velocity)/k (553-557), clamped to >= 0 (567-569); friction accumulate into the contact's per-slot array at (contact+0x1b8)*0x58 + 0x50 (256-260, 565-566, 0.333 cap at 1.0); impulses applied (573-580) via 006e8f00; lambda scaled by contact+0x1bc (593); tangential terms (594-599) | 006e8f00 [unread], 006e9250 [unread], 006e92f0 [unread], 00409a70 sqrt, 00409a50 fabs |
| 600+ | (read by the broadphase ticket) fraction clamps, further impulse terms | 006e8f00 |

## 2. The output record (0x108 bytes)

Offsets confirmed by disassembly for the reads marked (d); the rest from the decompiler.

| offset | type | reading |
|---|---|---|
| +0x00..+0x2f | not read here | `[assumed]` header, written by the cell |
| +0x30..+0x8f | 0x60 bytes, affine blob for A | pose at impact, passed to holder_commit_affine (line 143) `[assumed]` it is the pose at time +0xf8 |
| +0x90..+0xef | 0x60 bytes, affine blob for B | same for B (line 149) |
| +0xc0, +0xc4, +0xc8 | float x,y,z | contact point p (lines 50-59, 171-175; disasm 006e2e3e, 006e2e46) (d) |
| +0xcc, +0xd0, +0xd4 | float x,y,z | contact normal n. Oriented toward A (line 50-59). Written/negated in place (006e2e94-006e2ea4) (d) |
| +0xd8..+0xec | 6 floats (two vec3) | block passed to physics_contact_generate (lines 177-182) (d 006e32c7, 006e32e9) `[assumed]` the two contact-side points |
| +0xf0 | int | feature or shape index. Used in the per-shape flag at pool object +0x3c+index*0x40 (line 41) and as arg 7 of physics_contact_generate (d 006e2dbe, 006e338f) |
| +0xf8 | float | **time of impact** (hit fraction). Sort key (lines 1-30, disasm 006e2c70). Gate and +0x244 mark (d 006e323d) |
| +0x104 | int | node index into list +0x28050 (d 006e2d25) |

## 3. The sort

`FUN_00584e20(&desc, keys, n)` (line 24). Keys = raw dwords of rec+0xf8 (float bit patterns,
copied at lines 13-15 of the header); the result is an index array `iStack_10238`, used as
`aiStack_8000[index[k]]` (line 28). The descriptor's first dword is 0x80000000.
Direction: not read. `[assumed]` ascending. The gating (section 4) is only a "first impact
wins" rule if the earliest hit comes first; under a descending sort the first applied hit
would be the latest.
Caveat `[assumed]`: raw dwords order like floats only for non-negative keys. Fractions are
non-negative, so this should hold.

## 4. Commit and the time gate (what "commit poses" does)

It does not advance any body by a fraction in this function. There is no dt or
(1 - t) term here. Per hit, in order:
1. Gate: rec+0xf8 <= holderA+0x244 and <= holderB+0x244. Each holder's +0x244 starts at
   FLT_MAX (set in 006e1bc0) and takes the time of the first applied hit (lines 168-169).
   So each body takes its earliest hit in this pass.
2. Commit: holder_commit_affine(A, rec+0x30) and (B, rec+0x90), when the body guard holds
   (section 7). The pose is copied from the record, not computed here.
`[assumed]` the record's affine blobs are the body poses at time rec+0xf8, written by the
cell (005ef6f0 family). Settled by reading 005ef6f0's writes to obj+0x11000+...+0x30.
Formula the code implements: pose_body = record.affine (no interpolation).

## 5. What "run contacts" calls

`physics_contact_generate` (006e6a10) at line 201, given: world (param_1), pair node, prior
contact (pair+0xc, or 0 if no hit), &block (point and normal and the 0xd8..0xec block),
holder A, holder B, rec+0xf0 (feature index), flag (A+0x31 and no prior contact). It returns
a contact object, stored at pair+0xc (line 205) with contact+0x1e0 = pair. The contact is
the persistent record for the next pass and the object whose +0x1e8, +0x1ea, +0x1b8,
+0x1bc are read in the response. The solver body (after line 75 of 006e6a10) is not read.

## 6. The loop

```
loop:                                       (header lines 6-23)
  n = 0; keys = []
  while (h = 005e3da0(world)) != 0:         drain, decrements hits  (lines 11-18)
      keys[n] = h.+0xf8; hits[n] = h; n++
  if n < 1: break                           (line 19)
  order = sort(keys)                        00584e20 (line 24)
  for k in 0..n-1:                          (lines 26-636)
      rec = hits[order[k]]
      apply section 1 steps to rec          (gate, masks, normal, commit, contact, response)
  006e1bc0(world)                           pair setup (line 638)
  005e3d50(world)                           bin test (line 639)
  01177330(world)                           (line 640, not read)
  goto loop
```

- Iteration cap: none in 006e2c10. The loop ends only when the drain finds no hit. The
  per-pass +0x244 reset (006e1bc0) lets a body be hit again on the next pass.
- Time left in the step: no step-time variable in this function. Nothing here subtracts the
  impact time from the step or reads dt. `[assumed]` the remaining motion is covered by the
  next 006e1bc0 / 005e3d50 pass, which re-tests the whole step. Not settled.
- Overflow risk `[assumed]`: aiStack_8000 and local_10000 are 0x8000 bytes apart, so the
  key and hit arrays hold about 2048 entries. The drain loop (lines 11-18) has no bound
  check. Settled by reading 005e3da0 and the frame layout, or by a count check.

## 7. Body-kind conditions (flags tested)

| test | where | meaning `[assumed]` |
|---|---|---|
| node+0x32 == 0 (byte) | 36-43 | 0: both bodies are in the bin; nonzero: partner is the constant world record (see 006e1bc0) |
| holder +0x1d4 bit 0x400 (root) | 78-79, 155 | either root set: contact dropped (not a response body) |
| holder +0x1d4 bit 0x1000 | 85-92, 100, 246, 344 | in the response, selects the simple-path side and the ground override |
| holder +0x1d4 bit 0x10 (root) | 93-99 | tolerance 0.01 or 1.0 |
| holder +0x1d4 bit 0x2000 (root) | 119-121 (disasm tests 006e2fde, 006e3097) | avatar ground override gate |
| holder +0x1d4 bit 0x4000 | 140, 151 | set after commit: body was committed this pass |
| mask +0x9c bit 0x10, mask +0x9e bit 0x800 | 101-103, 119-121 | avatar-related masks for the ground override |
| body (+0xa4 -> body) byte +0x38 == 0 and float +0x34 == 0.0 | 142-148, 213, 303-304 | the body guard for commit and for the dynamic response (`[assumed]` non-static, non-sleeping) |
| body +0xa4 null | 101-104, 119-122, 146, 410 | holder has no physics body: no commit, no response on that side |
| owner +0xc / +0x208 | 80-81 | same owner: no response |
| contact +0x1e8 byte | 212, 429 | selects the simple path (0) or the full inertia path (nonzero) |
| contact +0x1ea byte | 214, 306, 430 | argument to 006deda0, and selects the tangent source |
| A +0x31 byte and A+0xc == 0 | 188-190 | flag for the new contact |

## Assumptions

- **C1** [assumed] The sort at 00584e20 is ascending. Settled by reading 00584e20 (ascending
  compare) or by the gating's intent.
- **C2** [assumed] rec+0x30 and rec+0x90 are affine poses at time rec+0xf8. Settled by
  005ef6f0's writes to those offsets.
- **C3** [assumed] +0xa4 body, +0x38 byte and +0x34 float are the body "static / sleeping"
  guard. Settled by 006dc3b0 and the body struct layout.
- **C4** [assumed] body +0x28 (both sides, summed) is inverse mass. Settled by its writer.
- **C5** [assumed] +0x214 is the friction coefficient, used as max(A,B). Settled by its writer
  and 006e6a10.
- **C6** [assumed] The response is inelastic (normal velocity killed, no restitution term).
  Confirmed only for the simple path lines 291-298; the full path scales lambda by +0x1bc
  (line 593), which may be a relaxation or restitution factor. Settled by reading +0x1bc's
  writer.
- **C7** [assumed] The separating test (line 163) comes after the +0x244 write (lines 168-169),
  so a separating pair still sets the time gate. This is what the code does; whether that is
  intended is not known.
- **C8** [assumed] The aiStack/local arrays hold 2048 entries (section 6).

## Not read

- 00584e20 (the sort): direction and descriptor.
- 006e6a10 beyond line 75 (contact generation and what it writes to the block).
- 006e8f00, 006e9250, 006e92f0, 006e9360, 006e9340, 006e9460, 006deda0, 006e95c0, 006e94e0
  (all tagged `unread` in the ledger, outlines only).
- 01177330 (called at 640).
- 005ef6f0 writers of the record (+0x30 blob, +0xc0-0xf8, +0xd8-0xec).
- 005e3da0 body (drain; capacity check).
- Writers of holder +0x1bc, +0x214, +0x28, +0x1e8, +0x1ea, +0x1b8, the per-slot array at +0x50.

## Next tickets

1. Read 00584e20 (sort direction, descriptor 0x80000000) and 005e3da0 (drain, bounds).
2. Read 006e6a10 past line 75: what the contact object and block hold; read 006deda0 and
   006e9250 (returns vec3 pointers) and 006e92f0 (pencil predicate).
3. Find the writers of rec+0x30, +0x90 (005ef6f0 family, the cell writes) to settle C2.
4. Find the writers of body +0x28, +0x214, +0x34, +0x38 and holder +0x1bc to settle C3-C6.
5. Read 01177330 (after each pass) and 006e9460 / 006e9360 (per-holder prep).
6. Confirm the bit names of holder +0x1d4 (0x10, 0x400, 0x1000, 0x2000, 0x4000) against the
   body struct in formats.md.
