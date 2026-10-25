# The narrow-phase core: a GJK ray cast (manager's read of 005bed70, 2026-10-11)

Read by the manager in disassembly, using the unpublished notes (block ids B0xx
are that map's). Scope: 005bed70, the ray-against-one-triangle test called by the soup
driver 005bcb30 (`collision-ray-soup.md`). Support helpers: `collision-support.md`.
STATE: entry, set-up, the support step and the simplex bookkeeping are read line by line
(005bed70-005bf530); the rest is identified by structure, and the parts marked UNREAD
below are not yet read.

## What it is

[game] A GJK ray cast with Johnson's sub-algorithm, in the classic layout (the one
published by van den Bergen, "Ray casting against general convex objects"): four simplex
points, a bit mask of the points in use, a table of edge differences, and a table of
determinants indexed [subset][point] kept in DOUBLE precision, everything else in float.
The frame slots (esp-relative in the body, where esp = entry - 0x8b4):

| slot | what |
| --- | --- |
| +0x188 + 12*i | simplex point y[i], i = 0..3 (float xyz) |
| +0x3b8 + 48*i + 12*j | edge[i][j] = y[i] - y[j] (float xyz), filled both ways when a point is added |
| +0x478 + 8*i | |y[i]|^2 as a double (x87: z*z + y*y, then + x*x) |
| +0x1b8 + 8*(point + 4*subset) | det[subset][point], doubles; det[last_bit][last] = 1.0 on add |
| +0x4a0 | bits: mask of points in the current simplex |
| +0x4a4 | last: index of the point being added (lowest clear bit of bits) |
| +0x4a8 | last_bit = 1 << last |
| +0x4ac | all_bits = bits | last_bit |
| +0x4b0 + 4*k, +0x5b0 + 12*k | per-iteration history: a float and the direction v (k = iteration) |
| +0x84 | lambda, the distance travelled along the ray so far (starts 0) |
| +0x90 | the ray's length |

## Arguments

[game] cdecl stack: a1 = the segment record (P0 at +0, P1 at +0xc, two ints at +0x18 and
+0x1c copied to the result), a2 = pointer to the result count n, a3 = the result table
(60-byte records), a4 = the triangle (three rows of four floats, passed to the support
helper 005e3710 in ECX). Register argument: EDX = a record whose +0x24..+0x2c is a
point (read at 005bee90). [game] It is the soup's 3x4 transform, whose last three floats
(+0x24) are its origin (`collision-ray-soup.md` step 6: EDX = matrix); it only seeds the
first search direction.

## Steps read so far

1. [game] 005bed80: if n >= 299 (0x12b): write -1.0 at record[n]+0x28, return 0.
2. [game] record = table + 60*n; record+0x24 = -1 (int), +0x34 and +0x38 = the segment's
   two ints, +0x28 = -1.0 (miss until proven otherwise).
3. [game] d = P1 - P0; len2 = d.y^2 + d.z^2 + d.x^2 (that order). If len2 < 1e-4
   (00b6a404; `comiss 1e-4, len2; jbe continue`): return 0 (a segment shorter than 1 cm
   never hits).
4. [game] v = centre - P0 (y, z, x subtracted separately). If |v|^2 <= 1e-4 (`jb` skips
   when 1e-4 < |v|^2): v = -d (unnormalised: -0.0 - d per component).
5. [game] len = sqrt(len2) through 00409a70; inv = 1.0 / len (divss); dir = d * inv.
   lambda = 0; x = P0.
6. LOOP (B008, 005befc0): store v in the history; p = support(triangle, v) (005e3710);
   w = x - p; vw = v.w (summed z, y, then x). If bits == 0xf (simplex full) the simplex
   is emptied first (005bf06c).
7. [game] if vw > 1e-4 (00b6a404; note: not 0 as in the textbook): vr = v.dir; if
   vr >= 0: MISS (jump 005bffad). lambda -= vw / vr; if lambda > len: MISS. x = P0 +
   dir * lambda (005bf0ed); w = x - p again; the simplex is emptied (bits = 0).
8. [game] add w as y[last] (last = lowest clear bit), |w|^2 into the double table, the
   edge differences against every point already in the simplex (four unrolled cases,
   005bf206-005bf4eb), det[last_bit][last] = 1.0 (005bf4f2).

## The loop's end and the exits (read 005bfe40-005c0148)

9. [game] 005bff08: when no proper sub-simplex is closer, the new point alone is kept:
   bits = last_bit only, v = y[last], and its squared norm is taken from the double
   table. (The search above it, 005bfdfb-005bff02, compares det[subset|bit][point]
   doubles with `fcompi`; not read line by line.)
10. [game] 005bff63: dist2 = v.z^2 + v.y^2 + v.x^2 (float). The loop ENDS when dist2 <=
    0.001 (00b38890; an ABSOLUTE bound on the squared distance, about 3.2 cm, not the
    textbook's relative error) or when the iteration count reaches 64 (005bff95). Both
    ends go to the same hit path: running out of iterations counts as a hit.
11. [game] MISS exit 005bffad: record+0x28 = -1.0, return 0.
12. [game] HIT path 005bfe40: record+0..+8 = x (the point reached on the ray), record
    +0x28 = lambda (metres from P0). Normal, 005bfe6f: k = the iteration at which lambda
    last advanced (slot +0x68, set at 005bf160); normal = history_v[k] * (1 / sqrt(
    history_f[k])) (sqrt by 00409a70, reciprocal by x87 `fdivr`, then stored as float),
    written to record+0x18..+0x20. history_f[k] is the float saved beside v at the top
    of iteration k, [assumed] |v|^2. So the hit normal is the normalised GJK direction
    from the last advance, not the triangle's face normal (the driver 005bcb30 then
    flips its own face normal to agree with it).
13. [game] A second hit path, 005bffc7, taken on a sign flag set before 005bfe40 (the
    setter is in the unread part): it marks byte +0x17 = 1, walks the history back to the
    last iteration whose float is > 0.001 and takes ITS v as the normal, then collects
    the support points of the simplex's members (rows at +0x108 + 16*i, chosen by bits)
    into a list at +0x98 and branches on whether there are exactly 3 (B103) or not
    (B109). [assumed] this is the case where the ray starts in contact (lambda never
    advanced), and B103 / B109 build a normal from the simplex itself.

## Run in the emulator (2026-10-11, `work\emu\ray_tri.py`)

The whole function runs offline in `work\emu\emu.py` (Unicorn; approved by the user the
same day), with the arguments above. Its only import is `_CIsqrt` (through 00409a70),
stood in by FSQRT. What the runs show, all [game] by execution rather than by reading:

- Hand cases against a triangle in z = 0: a ray down or up through it hits at lambda =
  5.0 exactly, normal = (0, 0, +1) for the ray going down and (0, 0, -1) going up (the
  normal OPPOSES the ray); a ray lying in the triangle's plane hits its edge (lambda 4,
  normal -x); a ray that stops short, passes beside, is shorter than 1 cm, or arrives
  with n = 299 writes lambda = -1.0 and leaves the count alone.
- THE COUNT IS THE VERDICT, NOT EAX: a hit increments `*a2`. EAX's low byte was 0 on
  every ordinary hit and every miss, and 1 only for the ray that STARTS on the triangle
  (lambda 0.0). So step 13's second path is confirmed as "starts in contact", and the
  return value flags it. Its normal (344 emulated triangle cases, `work\emu\ref\`): when
  the simplex ends with three points it is the FACE normal on the side P0 lies on
  (315 of 315; it has nothing to do with the ray's direction: the hand case above gave
  (0, 0, -1) for a ray going down); otherwise (29 cases, contact with an edge or
  corner) it is the walk-back direction of step 13. `gjk.rs` does this and agrees in
  344 of 344 (triangle) and 460 of 463 (sphere) within 0.02.
- A hit record: +0..+8 point, +0x18..+0x20 normal, +0x28 lambda, +0x24 = -1, +0x2c and
  +0x30 = 0, +0x34 / +0x38 the segment's two ints; +0x0c..+0x14 are not written.
- 5,000 random rays against the exact ray / triangle answer (float64): every exact hit
  is a hit in the game (343), and the game hits 14 more (rays passing within the 0.001
  squared-distance bound, about 3 cm). lambda differs from the exact distance by up to
  0.13 m (grazing rays stop when they come within 3 cm). This is the game's behaviour,
  and a port that returns the exact intersection would be WRONG.
- The x87 precision setting matters at bit level only: under control word 0x027f
  (53-bit) and 0x007f (24-bit), 889 of 943 hits differ in some bit (lambda by at most
  2 mm) and no hit / miss verdict differs (`work\emu\fpcw_check.py`). Which one the game
  runs under is OPEN, see the unpublished notes.

- WHY A PORT CANNOT MATCH EVERY CASE WITHOUT BEING BIT-EXACT (`work\emu\trace_tri.py
  <row>` against `GJK_TRACE_ROWS=<row> cargo test ... gjk::tests::trace_rows`): in row
  586 both reach iteration 3 with the same v to six digits, but |v|^2 = 0.00116, just
  above the 0.001 bound, so the loop goes on and advances with v.w = 0.0011 formed
  from products of size 1.2. A difference of 2e-7 in v changes v.w by 1% and which
  vertex the support returns, and lambda lands 9.7 cm away. These cases (grazing rays,
  huge spheres) are where the original's own answer depends on its last bits.
- [game] the history float of iteration 0 is the segment's length squared (144.0107 in
  row 586 = |P1 - P0|^2), not |v|^2; from iteration 1 on it is |v|^2. [assumed] it is
  the routine's running "distance squared" variable, initialised to len2. No emulated
  hit has a non-unit normal, so [assumed] lambda never advances at iteration 0 with it
  in use; unproven.

The port is tested against records produced this way (bit for bit once the control word
is known; within 2 mm until then).

## UNREAD (next)

- 005bf530-005bfdfb: Johnson's determinant update (B035, B040: the 146-instruction
  block) and the sub-simplex search. Expected to follow the published algorithm; what
  must be CHECKED is each comparison's direction, and where float and double mix.
- 005bfe26-005bfe40: what sets the sign flag for the second hit path.
- 005c0148-005c0520 (B103, B109 and the tail): the normal built from the simplex, the
  calls to 005502f0 and 005571b0, and the common end at 005c04fc (count increment and
  return value).

## Consequences

- The ten other ray tests (6.0-6.7 KB each) and the ten sphere tests are, [assumed] from
  the template ticket's skeleton (same caps and constants), this same routine with
  another support function inlined or called; and the pair tests are likely its
  two-object form. If so, the port is ONE generic routine over a support trait, plus the
  per-shape support functions, each small.
- Deviations from the textbook found so far, which the port must keep: the 1e-4
  thresholds (segment length squared, v.w, the seed direction), lambda in metres along a
  normalised direction, the full-simplex reset, summation orders, doubles only in the
  determinant and norm tables.
