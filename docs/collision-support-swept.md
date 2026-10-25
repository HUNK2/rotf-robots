# Collision support helpers for the swept-sphere tests

Read from disassembly (`work\raw_disasm.py`), transliterated in `work\kernels\support_swept.py`, and checked
against the original bytes in the offline emulator by `work\emu\check_support_swept.py`. Model notes:
the unpublished notes (sphere 005e3570) and `docs/collision-support-2.md` (cylinder, cone, pencil,
capsule). Check run: 1200 cases per function, random directions (tiny, axis, one zero, log-spread), positive
shape values, and matrix pairs (X, Y) as rotation pairs (Y = inverse of X's linear part), scaled rotations, and
unrelated random matrices. Every case compares 4 output words, bit for bit.

## The common pattern

Each swept-sphere wrapper computes two points and adds them:

- `buf1 = FUN_005e2dd0(v, P = *rec[0])`: the MOVING sphere's support in the world frame: `r * v / |v|`,
  `w = 1.0`. Zero when `|v|^2 <= 1e-6`.
- `shape = (shape point in the shape's frame) * X + t`, where the direction is first mapped with
  `w = v * Y` (row vector; Y is the inverse of X's linear part). The shape point is the shape's support
  along w (cylinder, cone, pencil, capsule helpers), or the sphere point `r * w/|w|` (005e3570), or the
  cube's sign-selected corner (inline in 005e2250).
- `out_sum = shape + buf1` (componentwise), `out_shape = shape`. The fourth lane is `1.0 + 1.0 = 2.0` in
  `out_sum` (see Unsure).

Where each function departs from it:

| function | role | stack args | v | shape point | departs how |
|---|---|---|---|---|---|
| 005e21a0 | sphere sweep | 6 | stack `[E+8]` | 005e3570 (does the w and X transform itself) | none: the sphere helper includes the w and X steps |
| 005e2250 | cube sweep | 5 | EAX | inline corner from sign of w | no helper; the corner is `(+/-h0, +/-h1, +/-h2)` |
| 005e2410 | capsule sweep | 6 | stack `[E+8]` | 005e32a0 -> capsule_support 005e3400 | none |
| 005e24c0 | cylinder sweep | 5 | EAX | 005e3140 -> cylinder_support 005b0180 | none |
| 005e2570 | cone sweep | 5 | EAX | 005e2fe0 -> cone_support 005b0060 | none |
| 005e2620 | pencil sweep | 5 | EAX | 005e2e80 -> pencil_support 005aff10 | none |

The four transform helpers 005e32a0, 005e3140, 005e2fe0, 005e2e80 are the same code with a different
callee: direction `w = v*A` (A in EAX), shape support of w, then `out = p*M + t` (M in the stack slot).
Only the stack offsets differ (005e3140, 005e2fe0 and 005e2e80 push ESI as well, so their locals sit 4
bytes further on, and they hand the callee ESI/EDI as the support's own in/out registers).

The 5-argument wrappers (cube, cylinder, cone, pencil) read the direction through EAX; the 6-argument
ones (sphere, capsule) read it as a stack pointer.

## Arguments at the call sites

The sphere test (FUN_005d1be0, call 005d1e72) and the capsule test (FUN_005d4f70, call 005d5202) push, in
this order (the last push is at the lowest address, [E+4]):
- `lea eax,[esp+0x30]` -> E+0x18: the shape output buffer (out_shape).
- `[ebp+0x24]` -> E+0x14: Y (the A matrix; it is the sphere test's entry +0x20, as the brief says).
- `ebx` -> E+0x10: X (the B matrix, where ebx was loaded is not traced).
- `edx = [ebp+0x14]` -> E+0x0c: the record {ptr R0, ptr R1} (entry +0x10, as the brief says).
- `lea eax,[esp+0x28]` -> E+0x08: v (a direction vector in the test's frame).
- `lea ecx,[esp+0xb4]` -> E+0x04: out_sum.
[game: push order read at 005d1e4a-005d1e72 and 005d51e1-005d5202.]

The cube test (FUN_005d35a0, call 005d3831), the cylinder test (FUN_005d6930, call 005d6bc1), the cone test
(FUN_005d8300, call 005d8591) and the pencil test (FUN_005d9cd0, call 005d9f61) push, in this order:
- `lea eax,[esp+0x30]` -> E+0x14: out_shape.
- `[ebp+0x24]` -> E+0x10: Y.
- `ebx` -> E+0x0c: X.
- `edx = [ebp+0x14]` -> E+0x08: the record.
- `lea eax,[esp+0xb0]` -> E+0x04: out_sum.
- EAX = `lea eax,[esp+0x2c]`: the pointer to v (not pushed).
[game: the same pushes read at 005d380a-005d3831, 005d6b9a-005d6bc1, 005d856a-005d8591, 005d9f3a-005d9f61.]

Inside each wrapper, `rec[0]` is a pointer to the moving sphere's radius (R0), and `rec[1]` is the shape
data pointer (R1). The sphere wrapper's `rec[1]` is a pointer to one radius float; the cube's is three
half extents; the capsule's is nine floats (B0..B8); the cylinder, cone and pencil's is four (r, h, a, c).
[game for the reads; the records' own layout was not read.]

Entry registers and stack (the offsets are from the wrapper's entry esp, E):
- 005e21a0 sphere sweep: `[E+4]` out_sum, `[E+8]` v pointer, `[E+0xc]` rec, `[E+0x10]` X (-> ESI in the
  shape helper), `[E+0x14]` Y (-> EAX), `[E+0x18]` out_shape. [game]
- 005e2250 cube sweep: EAX = v pointer; `[E+4]` out_sum, `[E+8]` rec, `[E+0xc]` X (-> EBP), `[E+0x10]` Y
  (-> EBX), `[E+0x14]` out_shape. [game]
- 005e2410 capsule sweep: as the sphere, with the shape data in ESI for the capsule helper. [game]
- 005e24c0, 005e2570, 005e2620: EAX = v pointer; `[E+4]` out_sum, `[E+8]` rec, `[E+0xc]` X, `[E+0x10]` Y,
  `[E+0x14]` out_shape. [game]

## 005e2dd0: the moving sphere's support (sphere_support)

Arguments: ESI = v (x +0, y +4, z +8). `[E+4]` = pointer to the radius; `*P` is read at 005e2e38. EDI = out.
[game]

1. `n2 = ((vx*vx + vy*vy) + vz*vz)`, float32, in that order (005e2dd3-005e2dfa). [game]
2. `comiss n2, 1e-6` (00b34610 = 0x358637bd); `jbe 005e2e35` taken when `n2 <= 1e-6`: `k = +0` (xorps). [game]
3. Else `s = float(sqrt(n2))` (call 00409a70, stored as float, 005e2e19), `k = float(1 / s)` (fld1,
   fdivrp at 53 bits, stored as float, 005e2e1e-005e2e2b). [game]
4. `keff = P * k` (005e2e38-005e2e40). [game]
5. out = `(vx*keff, vy*keff, vz*keff, 1.0)` (005e2e44, 005e2e4b, 005e2e50, 005e2e55-005e2e66, 1.0 at
   00b38834 = 0x3f800000, 005e2e6b). [game]

Meaning: the sphere's support point in direction v (the centre is at the origin; the caller adds the
other shape). Degenerate: `|v|^2 <= 1e-6` gives `keff = P * 0`, so the output is zero (its sign follows P
and v, a negative P flips it).

Check: 1200 cases, 0 differ (sp_nonzero 1166, sp_zero 34). Also exercised 1200 times inside each wrapper.

## 005e3140, 005e2fe0, 005e2e80, 005e32a0: the four transform helpers

Arguments: ECX = v pointer, EAX = A (Y), `[E+4]` = out, `[E+8]` = M (X), `[E+0xc]` = P (shape data, the
shape pointer `rec[1]`). 005e32a0 also needs ESI = the capsule's B (the wrapper sets it: ESI = `rec[1]`).
[game]

1. `w = v * A` (row vector, the same expression as 005e3570's first part):
   - `w0 = ((A3*vy + A6*vz) + A0*vx)` (005e32b3-005e32da; in 005e3140 at 005e3159-005e317b)
   - `w1 = ((A1*vx + A4*vy) + A7*vz)` (005e32e9-005e3303)
   - `w2 = ((A2*vx + A5*vy) + A8*vz)` (005e330d-005e3330)
   [game]
2. The shape support of w, with out in a local and P on the stack:
   - 005e32a0 -> capsule_support 005e3400 (EDI = &w, ESI = B, stack = local out) [game]
   - 005e3140 -> cylinder_support 005b0180 (ESI = &w, EDI = out, stack = P) [game]
   - 005e2fe0 -> cone_support 005b0060 (EDI = &w, ESI = out, stack = P) [game]
   - 005e2e80 -> pencil_support 005aff10 (ESI = &w, EDI = out, stack = P) [game]
3. `out = p * M + t` (row vector, M row-major at +0..0x20, t at +0x24, +0x28, +0x2c), with p = (px, py, pz)
   the support's output (005e3350-005e33d7 in 005e32a0; 005e31f5-005e325f in 005e3140, the same arithmetic):
   - `o0 = ((M6*pz + M3*py) + M0*px) + M9`
   - `o1 = ((M7*pz + M1*px) + M4*py) + M10`
   - `o2 = ((M8*pz + M2*px) + M5*py) + M11`
   [game]
4. `w = 1.0` at out+0xc (00b38834). [game]

Meaning: the shape's support, in the shape frame, along the shape-frame direction `v * A`, mapped to the
world by the affine M. A is the inverse of M's linear part in the game's usage (for a rotation, A = M^T);
the helper does not check this.

Check, each directly: 1200 cases per helper, 0 differ (cylinder transform 1200/0, cone 1200/0, pencil
1200/0, capsule 1200/0). Their branch paths are in the support helpers (`docs/collision-support-2.md`).

## 005e21a0: the sphere sweep

Stack as above: `[E+4]` out_sum, `[E+8]` v, `[E+0xc]` rec, `[E+0x10]` X, `[E+0x14]` Y, `[E+0x18]` out_shape.
[game]

1. `buf1 = sphere_swept(v, *rec[0])` (005e2dd0, 005e21be). [game]
2. `shape = FUN_005e3570(v, A = Y, P = rec[1], B = X)` (005e21de: ECX = v, EAX = Y, ESI = X, EDI = a
   local, stack = rec[1]). Proven copy: `support.sphere_support`. [game]
3. `out_shape = shape` (4 floats, w = 1.0), `out_sum = shape + buf1` (005e21e7-005e2237). [game]

Meaning: the Minkowski sum of the moving sphere (radius R0, at the origin along v) and the shape's sphere
(radius R1, transformed by X). The fourth lane is `1.0 + 1.0`.

Check: 1200 cases, 0 differ (sp_zero 44 cases in the sphere point, the shape's sphere cases counted by the
`support.sphere_support` branch in the proven note).

## 005e2250: the cube sweep (no shape helper)

Arguments: EAX = v pointer; `[E+4]` out_sum, `[E+8]` rec, `[E+0xc]` X (EBP), `[E+0x10]` Y (EBX),
`[E+0x14]` out_shape. [game]

1. `buf1 = sphere_swept(v, *rec[0])` (005e226c). [game]
2. `w = v * Y` (the same three sums as in wdir: 005e2271-005e22fc; w0 in xmm2, w1 in xmm1, w2 in xmm0). [game]
3. Sign per axis, with `h = rec[1]` (three floats `h0, h1, h2` at [eax], [eax+4], [eax+8]):
   - `s_k = +h_k if 0 <= w_k` (comiss 0 then jbe, 005e22fc-005e2324: taken when `0 <= w`), else
     `s_k = (-0.0) - h_k` (00b6a3ac = 0x80000000). [game]
   - w2 -> s_z (005e22fc), w1 -> s_y (005e2310), w0 -> s_x (005e2324).
4. The corner in the shape's frame, mapped by X (row vector, translation at +0x24, +0x28, +0x2c):
   - `o0 = ((X3*s_y + X6*s_z) + X0*s_x) + X9` (005e2333-005e2364) [game]
   - `o1 = ((X1*s_x + X4*s_y) + X7*s_z) + X10` (005e235f-005e2387) [game]
   - `o2 = ((X2*s_x + X5*s_y) + X8*s_z) + X11` (005e2382-005e23aa) [game]
   - `w = 1.0` at out+0xc. [game]
5. `out_sum = out_shape + buf1`. [game]

One-line meaning: the cube's support is the corner with each half extent signed by the sign of (v*Y)_k,
taken in the cube frame and then mapped by X; the sign of zero is taken as positive.

Check: 1200 cases, 0 differ. Branch hits: cube_pos 1758, cube_neg 1842 (every corner sign reached).

## 005e2410, 005e24c0, 005e2570, 005e2620: the capsule, cylinder, cone and pencil sweeps

Each does the same as 005e21a0 with the transform helper above in place of 005e3570:
`buf1 = sphere_swept(v, *rec[0])`; `shape = transform_out(support(w = v*Y, rec[1]), X)` with w = 1.0;
`out_sum = shape + buf1`. Stack layouts: 005e2410 is the sphere layout (6 args); the other three are the
5-arg layout with v in EAX.

Checks: capsule 1200/0 (endpoint 1 580, endpoint 2 620, degenerate 43); cylinder 1200/0 (rim 1155, axis 45);
cone 1200/0 (rim 868, tip 295, degenerate 37); pencil 1200/0 (tip 575, rim 607, degenerate 18). The
degenerate branches are the proven ones in `docs/collision-support-2.md`.

## Assumptions

- [assumed] x87 precision 53-bit (the emulator's control word 0x027f). The one-round x87 chain in
  005e2dd0 (1/s, times P) and in the cylinder/cone/pencil rim paths depends on it. Settled by: the game's
  control word.
- [assumed] `rec[0]` and `rec[1]` are pointers to the radius (R0) and the shape data (R1). The arithmetic
  is consistent with that, and the caller's records were not read. Settled by: the records' writers.
- [assumed] The matrices: X is the 3x4 shape-to-world transform and Y is its inverse's linear part, the
  pair the sphere test builds. The arithmetic does not need Y = X^-1 (the check covers unrelated
  matrices), but the physics does. Settled by: the sphere test's record writer (not read).
- [assumed] The direction `w = v*Y` is not normalised; the shape supports normalise internally where
  needed. The check covers all magnitudes including tiny v. Settled by: nothing needed; it is what the code does.

## Unsure

- The fourth lane of out_sum is 2.0 (1.0 + 1.0), not 1.0. The callers were not read, so it is not known
  whether they read lane 3 at all.
- The sphere test's X and Y at the call site: ebx and `[ebp+0x24]` are pushed, but where ebx is loaded in
  005d1be0 was not traced. The brief's entry offsets (+0x14 X, +0x18 Y) were not verified.
- The capsule's B pointer goes in ESI through the wrapper, not through the argument list; the
  capsule's helper relies on ESI surviving the call from 005e2410 (checked: the wrapper sets it at 005e243b).
- Hull4 (005afdf0): not read in this ticket.

## Corrections to the brief

- The cube has no helper in its own file, but the brief's note that its arithmetic is inline in 005e2250 is
  right; the cube's sign test reads w = v*Y, not v (the brief did not say so).
- The brief said the 005e2410 capsule wrapper "also calls 005e32a0 and 005e3400": yes, but 005e32a0 is a
  transform helper (direction w = v*Y, out = p*X + t), not a second capsule support.
- The brief said 005e3140, 005e2fe0 and 005e2e80 are unread. They are now read: each is the transform helper
  above, with its own callee. 005e3140 calls cylinder_support (005b0180), 005e2fe0 calls cone_support
  (005b0060), 005e2e80 calls pencil_support (005aff10).
- The brief's stack offsets for the sphere test (+0x14 X, +0x18 Y) do not match the pushes read here
  (Y at [ebp+0x24]); see Unsure.
