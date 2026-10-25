# The census's physics system: all 424 read, and the plan to port it (2026-10-16)

The user asked to read the whole census system `physics` (`fn.py census`: 424 functions, 285 engine and 139
gameplay) and get ready to port it. Reading only; no Rust was changed in this session. All 424 now have ledger
lines that point here (`fn.py census` shows physics 424 / 424). Tags: `[game]` read in code (address given),
`[assumed]` (why, and what settles it), `[stand-in]` for port debts.

Scripts (all static, capstone over the install's executable): `work\physics\pair_map.py` (the 121 cells down to
the shape supports; output `work\physics\pair_map.txt`), `diff_all.py` (normalised template diff of a family
against one reference; `diff_P.txt`), `diff_helpers.py` (`diff_H.txt`), `list.py` (census rows of a system with
the ledger state), `note_narrow.py` / `note_batch.py` (the ledger writes). Read dumps: `p_00616ed0.c` (the
reference primitive, cleaned), `asm_00616ed0.txt` (its disassembly), `p_0063c3a0.c`, `q_006bb1f0.c`,
`gameplay_dump.txt`.

## 0. What the 424 are

| Group | Count | What it is | Section |
|---|---|---|---|
| Pair matrix: wrappers W, soup selectors R0, soup wrappers W2 | 100 + 10 + 10 | the 121 cells of 00b8db28 (11 shape types squared, cell [0][0] empty) | 1 |
| Pair primitives P (89 + 11 inline) and Q (10, soup triangle) | 110 | ONE swept GJK algorithm, three code variants | 1, 2 |
| Pair support helpers H | 66 in the census (95 in all: 15 cells inline their supports, 29 helpers lie past 006d2ed0 in other buckets) | support(v) of B + support(v) of A, witness = A | 1.4 |
| Ray tests (table 00b8dd0c) | 8 | the GJK ray-cast template with one support each | 3 |
| Record copy 00612530 | 1 | | 2 |
| Gameplay physics components | about 50 | Link_BallJoint, Link_Spring, Link_Hinge (null), PhysicsOverride, the wind (reactor, direction, area, sphere), ActivateCollision, GroundPositioner, the registrars | 4 |
| Mislabelled by the census (unit taken from the address) | about 70 | Bink movie player, VO conversation seats, AIEngageDetector map, containers, listener lists, AI goal pieces | 5 |

## 1. The narrow phase: the 11 x 11 pair matrix [game]

Shape types (`docs/collision-shapes.md`): 0 Soup, 1 Sphere, 2 Cube, 3 Capsule, 4 Cylinder, 5 Cone, 6 Pencil,
7 Hull32, 8 Hull16, 9 Hull8, 10 Hull4. Row = shape A, column = shape B (the unpublished notes section 2
for the dispatcher 005e39a0, the bins 005e3d50 and the insert 0117ba30).

Every cell, mapped statically (`pair_map.txt`, one line per cell):

- Row 0 (soup A x B), cols 1-10: R0 selector (005e3f50 + 0x1b0 * (col-1)) -> W2 wrapper (00612790...006167b0)
  -> Q primitive (006bb1f0...006ca4f0) -> per-triangle helper (006d67d0 sphere, 006d6870 cube, inline for
  capsule/cylinder/cone/pencil, 006d6a30...006d6f70 hulls).
- Rows 1-10, cols 1-10: W wrapper (005e5030 + 0x740 * k) -> P primitive (00616ed0...006b9840) -> helper
  (006cbe80...006d6500) or two direct support calls.

### 1.1 The R0 selector (soup row) [game, 005e4100 read]

Loops the soup's 64-byte triangles (`(short)list[1]` of them). A triangle is tried only when its flag at +0x3c
is 0 and its normal (+0x30..+0x38) dotted with A's displacement (input +0x54..0x5c minus +0x24..0x2c) is
negative. It calls its W2 for that triangle; when that returns 1 and the result's t (+0xf4) is the least so far,
the 0x108 output is copied (00612530) to the caller's slot. Returns 1 when any triangle hit.
SETTLED 2026-10-17 (disassembly of 005e3f50, emulator references of all nine cells, `pair_sweep::soup_cell`):
- The soup is B, the second argument (row 0 = B's type). Its data is word 0 = the triangle array, word 1 (a short)
  = the count. The displacement is A's alone (the moving shape's), which is why a static soup's triangles are
  tried at all. [game]
- The flag test is an INTEGER compare of the word at +0x3c (so -0.0 also skips). The dot is
  (n.x dx + n.y dy) + dz n.z, and the triangle is tried when 0 > dot (NaN skips). [game]
- The best record is kept on lambda (+0xf4), strictly less, so the first of equals wins. The selector then writes
  over its copy the triangle's STORED normal at +0xcc and the triangle's index, a 16-bit count, at +0xf0 (the W2
  writes -1 there). This happens also on a start in contact, so the W2's 005be420 re-cast normal is lost. [game]
- The triangle record: vertices (x, y, z, w) at +0x00 / +0x10 / +0x20, the normal at +0x30, the flag at +0x3c.
  The triangle support 005e3710 takes the RAW vertices with the raw direction: no basis is applied (006d67d0 passes
  v straight to it). So the vertices are relative to B's origin, in world axes; B's end basis enters only the
  output's frames. The re-cast (005be420 with type 0 and its flag) runs 005bed70 on the triangle seeded at B's
  origin at t. [game]
- Helpers: sphere 006d67d0, cube 006d6870 (the pair cube's sums, `pair_cube_support`), hulls 006d6a30 / 006d6bf0 /
  006d6db0 (as the W cells' default rows). All of them run in the helper family (`gjk::PRIMITIVE`), B + A, with the
  triangle's point as the witness.
- The capsule, cylinder, cone and pencil copies (006be530, 006c0200, 006c1ec0, 006c3b80) inline A. Their direction
  rows are (Y[3+k] vy + Y[6+k] vz) + Y[k] vx and their point rows ((X[3+k] py + X[6+k] pz) + X[k] px) + X[9+k]
  (disassembly 006be8c0-006beb52). Their loop is in the inline family's orders (`gjk::INLINE`; v.w (x + z) + y).
OPEN for wiring (step 5) [assumed until read]: who builds these 64-byte records. The CollisionSoup block
(`collision-soup.md`) is 16-byte vertices and 12-byte index triples, a different layout. So the records are a
second form, built per query or at load. Also whether a level soup's B pose is the identity. Settles: the callers
of the bin insert 0117ba30 for a soup pair, and whatever writes +0x30 / +0x3c.

### 1.2 The wrapper (W, W2) [game, 005e5030 read; collision-pair-records.md has the record layout]

- Input 0xd0 record: A start basis+origin +0x00..0x2c, A end +0x30..0x5c, B start +0x60..0x8c, B end
  +0x90..0xbc, ids at +0xc0/+0xc4, index +0xc8.
- The cast: p0 = A0 - B0 (translations), p1 = p0 + (dA - dB). Orientations are the END bases of both shapes
  (+0x30 and +0x90); inverses taken with inverse_affine3 005301d0. No rotation sweep.
- Output 0x108 record (when the primitive's lambda >= 0): +0xf4 lambda, +0xf8 t = lambda / |dA - dB|,
  +0xe4..0xec contact point in B's frame, +0xf0 = -1, +0x104 = input +0xc8, +0xcc..0xd4 the normal, poses of A
  and B advanced to t (+0x24.. and +0x84..), their inverses, the contact in A's frame +0xd8..0xe0 and in world
  +0xc0..0xc8.
- Start in contact (primitive returned 1): 005be420 re-casts and keeps the least-lambda hit's normal.
  CORRECTED 2026-10-16 from the call site's disassembly (005e56fe-005e574a; the decompiler's two views of the
  arguments disagree): ECX = the record's world contact c (+0xc0), EDX = the record at +0x60 (B at t, its
  inverse at +0x90), stack: q = A's start origin - c, B's shape TYPE, B's data, a local point out, the normal
  out (+0xcc). The segment runs q + c .. c - q, i.e. from A's start origin through the contact, against B's RAY
  test (00b8dd0c[type of B]), seeded at B's origin. Only the normal of the record changes (every other field of
  the emulated contact rows equals the primitive's own).
- [game, by the pair helpers] cell [row][col] takes shape A (its first argument, posed by the A poses) of type
  COL and B of type ROW: 006cbf30 (cell [1][2]) inlines the cube from the shape list's first entry, 006ccb00
  (cell [2][1]) from the second. Each pair helper adds B's point to A's and keeps B's point as the witness.
- The output poses (+0x00, +0x60) copy the END bases (A1, B1), origins advanced to t; t = lambda / |dA - dB| with
  the root ROUNDED TO FLOAT first (bit-exact in every reference row; the unrounded root misses a quarter).
- The pair helpers' inline cube and the cube's ray test 005c3670 each use their own add order, different from
  the swept helpers' frame rule `gjk_world` started from (ledger lines 006cbf30, 005c3670).
- The 34 W cells that are not token-identical to 005e5030 (diff 95.7-96.8 %) differ only in the call set-up of
  their primitive (the inline family takes seven arguments, the hulls pass a cache word).
- Hull cache words [game, call sites 00623f0c, 0069b3ad, 006a80cf; bit-exact references]: the segment the
  wrapper builds carries the input's +0xc0 / +0xc4 at its +0x18 / +0x1c; the primitive copies them to its
  record's +0x34 / +0x38 (words [13], [14]) and passes &record+0x34 to an A hull's support, &record+0x38 to a
  B hull's, which walk them in place (the unit of the last answer). The wrapper copies [13] / [14] to the
  output's +0xfc / +0x100, so for a hull they are its walk unit after the cast. The 005be420 re-cast's query
  has +0x18 = +0x1c = 1: a B hull's ray test there starts at unit 1. The shape data of a hull is a word
  holding the block pointer (the helpers pass list[k] and the support reads *it).

### 1.3 The primitive: a GJK ray cast against the Minkowski sum [game, 00616ed0 read in full]

Arguments (00616ed0, cdecl): segment record (p0 at [0..2], p1 at [3..5], ids [6],[7],[8],[9]), &count, record
table (60-byte records, cap 298: index > 0x12a writes -1.0 at +0x28 and returns 0), the two shape descriptors,
the B descriptor (seed centre at +0x24..0x2c), a witness out.

1. d = p1 - p0, len2 = |d|^2 (order x*x + y*y + z*z as written: `local_88c*local_88c + local_888*local_888 +
   local_890*local_890`, i.e. y, z, x). len2 < 1e-4: miss (record +0x28 = -1, return 0).
2. Seed v = centre(B) - p0; if |v|^2 <= 1e-4, v = -d. len = sqrt(len2) (00409a70), dir = d * (1/len).
3. Loop, iteration k = 0..63 (counter incremented at the top):
   - history: v_k stored, and the float beside it = len2 for k = 0, else |v|^2 of the previous step.
   - p = pair_support(v) (helper; 4 floats, w = 1 or 2), witness q = the row shape's own support point.
   - w = x - p. vw = v.w.
   - If the simplex is full (bits == 15), it is emptied (bits = 0).
   - If vw > 1e-4: vr = v.dir; miss when vr >= 0; lambda -= vw / vr; miss when lambda > len; x = p0 + dir *
     lambda; w = x - p; bits = 0; the iteration index is kept as the LAST ADVANCE.
   - The new point goes into the LOWEST FREE slot of a fixed 4-slot simplex (last, last_bit); y[last] = w,
     yy[last] = |w|^2 as double; the support point p (4 floats) and the witness q are kept per slot; x too.
   - Johnson's sub-algorithm, SOLID style (edge form), all in x87 with the determinants stored as DOUBLE:
     - edges: e[i][last] = y[i] - y[last], e[last][i] = -e[i][last] for every i in bits;
     - det[last_bit][last] = 1; for j in 0..3 with bit j in bits: s2 = bit_j | last_bit;
       det[s2][j] = e[last][j] . y[last]; det[s2][last] = e[j][last] . y[j];
       for k < j with bit k in bits: s3 = s2 | bit_k, and the three cofactor updates of the edge form
       (det[s3][k], det[s3][j], det[s3][last], each = dot(e, y) * det[..] + dot(e, y) * det[..]);
     - when all_bits == 15, det[15][0..3] are computed (the 4-point block, lines 232-257 of p_00616ed0.c);
     - closest: for s from bits down to 1, for each subset s of bits, if valid(s | last_bit) take it; else if
       valid(last_bit) take {last}; else LEAVE THE LOOP (the hit is reported with the current v);
       valid(s): for every i in all_bits: if i in s, det[s][i] must be > 0; if not, det[s | bit_i][i] must be
       <= 0;
     - v = sum det[s][i] * y[i] / sum det[s][i] (divide only when the sum > 1e-8).
     Precision: the dot products are float loads multiplied and added on the x87 stack, multiplied by the
     double determinants (fmul qword) and stored with fstp qword (disassembly 006177a0-00617928). The
     decompiler's `(float)` casts on the determinants are artefacts. So the result depends on the x87
     precision control (the open item in the unpublished notes: 0x027f against 0x007f).
   - Stop when |v|^2 <= 0.001 or after 64 iterations. BOTH ends report a hit.
4. Record (60 bytes, at count; count++ on every hit):
   - [0..2] witness = sum det[bits][i] * q[i] / sum (the row shape's point at contact), [3..5] not written here;
   - [6..8] normal; [9] = -1 (written first); [10] lambda; [11],[12] = segment [8],[9]; [13],[14] = segment
     [6],[7].
   - Normal when lambda advanced: v at the iteration of the last advance, times 1/sqrt(its history float).
     Return 0.
   - Start in contact (never advanced), return 1: walk back from the last iteration to the newest history float
     > 0.001 and take that v, normalised by sqrt of the float (for k = 0 the float is len2, so the seed is
     divided by |d|, not by |v|). Then, by the number of points left in the simplex:
     3 points: n = (p1 - p0) x (p2 - p0) of the SUPPORT points p, normalised (005502f0); if n . walkback < 0
     the normal is -n, if n . walkback == 0 and n == 0 the walk-back stays, else n.
     2 points: the game computes e = p1 - p0 normalised and m = dir - e (e . dir), but then calls 005571b0 with
     ECX = (walk-back, |walk-back|) and EAX = &m (disassembly 00618768-00618770), and 005571b0 writes ECX's
     vector divided by its length to EAX. So m is OVERWRITTEN by the walk-back normalised again, and the edge
     plays no part (2026-10-16 correction of this note's first reading, which had m normalised). Oriented the
     same way.
     1 or 4 points: the normal is -dir.

`gjk.rs` against this (state after 2026-10-16, port plan steps 1 and 2 DONE):

- G1 Johnson: DONE. `gjk::Simplex` is the game's: four fixed slots, the lowest free slot for a new point, the
  persistent edge-form det table in f64, the 4-point block, the first valid subset in descending bit order, the
  newest point alone, and the loop left when nothing is valid (`Closest::Stuck`). The weights are rounded to
  float and summed in single as 00617d7a-00617fa4 do; the weight sum is double and divided only above 1e-8.
  The history float of iteration 0 is len2.
- G2 Start-in-contact normal: DONE (`gjk::contact_normal`, the rules above, with the 2-point correction).
- G3 Witness: DONE (`Simplex::witness`: det[bits][i] / sum over q, sum > 0). `gjk_swept::Swept` now gives the
  target's support as q, and `sphere_cast` reports that witness.
- Result against the emulator references (`cargo test -p tf2-core --lib gjk -- --nocapture`): ray / triangle and
  ray / sphere: hit/miss, contact flag and lambda BIT-EXACT on every row (largest lambda diff 0; it was up to
  centimetres), start-in-contact normals 344/344 and 463/463 (was 461). Sphere sweeps against sphere, cube,
  capsule, cone, cylinder: lambda exact except cube 1.1e-4, normals 0.00 degrees (start in contact was 21-68
  degrees mean), points within 0.1 mm (were 5-30 cm). So 005bed70 and the sphere tests run this step and these
  normal rules too ([assumed] before, now tested). World shapes (`gjk_world.rs`: cube, cylinder, cone, pencil,
  capsule): lambda bit-exact on about half the rows only, 1 to 5 grazing outliers per file (cube 1356, cone 739,
  cylinder 174 / 224 / 1316, pencil 547 / 815 / 1021 / 1361 / 1367): their supports and frame arithmetic are
  not bit-exact yet. Against the earlier table (`collision-ray-frames.md`) cone and pencil improved and cube
  (661 -> 660 within 2 mm) and cylinder (99.72 -> 99.57 %) slipped by one or two rows.
  SETTLED 2026-10-17: those outliers were the ray tests' own add orders (1.4). With `gjk_world::ray_test` all
  five world-shape files and both hull files are lambda bit-exact on every ordinary hit.
- G4 The pair tests themselves: none is ported. `collision_world.rs` `pencil_cast` runs the RAY cast against a
  grown target ([stand-in], its own comment), and the character's world contact is still probes
.

### 1.4 The three primitive variants and the helpers [game, by template diff]

`diff_P.txt`: 89 P primitives match the reference 00616ed0 at 90.8-100 % of its bytes (unmatched 0-604 bytes:
register allocation and the helper call set-up). The INLINE variant (11 cells: capsule, cylinder, cone and
pencil against each other; 7937 bytes, 7969 for capsule x capsule; read in 0063c3a0) matches 66.5-68.7 %
because it inlines both supports: B's support through its basis into local space and back, then A's, and sums
them (w = 2.0), with A's world point as the witness. The Q primitives (soup triangle) match 92.7 % (6529-6559)
and 74-76 % (the inline 7346/7363). The loop, the Johnson step and the record are the same in all three.

The helpers (`diff_H.txt`): every one is support_B(v) + support_A(v) in the SAME direction, each in world space
(direction rotated into the shape's frame, support, transformed back), with A's point written as the witness
(4 floats, w = 1). By shape: the cube is always inlined (sign of each basis row . v picks +-half extent); the
sphere is 005e3570; capsule, cylinder, cone, pencil go through their transform helpers 005e32a0, 005e3140,
005e2fe0, 005e2e80 (bit-exact in the emulator, `docs/collision-support-swept.md`); hulls are 005afa90 /
005afbb0 / 005afcd0 / 005afdf0 with their cache word; the soup triangle is 005e3710. Size classes are 100 %
token-identical within a pair type.

Consequence for the port: ONE generic routine `pair_sweep(seg, A, B) -> Record` over a trait with
`world_support(v) -> (point, w)` reproduces all 110 primitives; only the summation order and the w (1 or 2) of
the inline variant differ.

ADD ORDERS, settled 2026-10-17 (bit-exact against the emulator; `gjk::Orders`): the template is compiled in
two order families. Because the first add of a three-term sum commutes, a sum is fixed by its LAST term.
- `gjk::PRIMITIVE`: 00616ed0, the helper-calling pair primitives, and 005bed70 (triangle, sphere ray tests).
- `gjk::INLINE`: the eleven inline pair primitives AND every shape ray test of 00b8dd0c except the sphere
  and the triangle (cube 005c3670, capsule 005c5020, cylinder 005c6a50, cone 005c8480, pencil 005c9eb0, the
  four hulls). Read in 0063c3a0: |d|^2 ends on z, the seed test on y, v.w on y, v.dir on z, the closing
  |v|^2 on y; the 2-point contact normal's |walk|^2 on x; the three-point det updates and the four-point
  block have their own dot orders and the four-point block its own outer order (lines 228-311, the locals
  mapped to the edge / point / det tables by stack offset).
- The inline primitives' supports (all eleven, `work\inline_orders.py --dump` prints them): direction
  w_k = (Y[k] vx + Y[3+k] vy) + Y[6+k] vz, except B's row 1, (Y1 vx + Y7 vz) + Y4 vy; each point
  ((X[k] px + X[3+k] py) + X[6+k] pz) + X[9+k], then B + A. The decompile of the nine copies without a capsule
  prints A's terms folded one by one into B's point; that is the DECOMPILER re-associating: the disassembly
  of 0064c9c0 (0064d105-0064d1e4) sums A's point whole, and the folded form misses half the rows.
- The ray tests' supports: capsule / cylinder / cone / pencil end every direction row on vx and every point
  row on px (disassembly 005c6dc9-005c6ef5; the decompile DROPS the direction rows); the cube's point rows end
  on pz; the hulls read Y and X in place with w ending (y, y, z) and o ending (x, z, x) (005cec94-005cedea).
  `gjk_world::ray_test` / `ray_world_support`; `collision_world::HullLocal`.
- Inline cells (column = A, row = B): [3][3] [3][4] [4][4] [4][5] [4][6] [5][4] [5][5] [5][6] [6][4] [6][5]
  [6][6]. The capsule as B against a cone or pencil ([3][5], [3][6]) and a capsule A elsewhere ([4][3],
  [5][3], [6][3]) call helpers (`PRIMITIVE`).
- The HULL helpers (read in the disassembly, 2026-10-17; every hull cell is a helper cell, `PRIMITIVE`; sizes
  are token-identical, the Hull8 copies were read): the hull support is called with *data as the block and a
  cache pointer (1.2). Direction into the hull's frame, every helper: w = ((Y3 vy + Y6 vz) + Y0 vx,
  (Y1 vx + Y4 vy) + Y7 vz, (Y2 vx + Y5 vy) + Y8 vz), the pair cube's. Point out, two forms:
  standard ((X6 pz + X3 py) + X0 px) + X9, ((X1 px + X7 pz) + X4 py) + X10, ((X2 px + X8 pz) + X5 py) + X11
  for an A hull in every helper (006cc760 sphere, 006cdc70 cube, 006ce990 capsule, 006cf3f0 cylinder; cone,
  pencil the same) and a B hull against a sphere, capsule, cylinder, cone or pencil (006d3a10, 006d3eb0,
  006d4070); every row ((X[6+k] pz + X[3+k] py) + X[k] px) + X[9+k] for a B hull against a cube or a hull
  (006d3bd0, 006d4b50). The other shape is the simple helpers' (sphere 005e3570, transform helpers, the pair
  cube); B's point is the witness, sum B + A. `pair_sweep::pair_hull_support`.

### 1.5 One step's contact pass, as it moves a body (read 2026-10-17 for port step 5) [game unless tagged]

How a step uses the cells (`006df7c0`, `006e5e00`, `006e1bc0`, `006e2c10`; `docs/collision-response.md` and
`collision-broadphase.md` have the older reads, corrected here where marked).

1. Predict (`holder_predict_affine` 006db1f0): the holder's predicted affine goes to +0x168 (inverse +0x198),
   and the predicted translation minus the current one goes to +0x1c8..+0x1d0. That is the step's DISPLACEMENT, not
   a velocity. The separating test reads it.
2. Pair input (006e1bc0, read in full): A start = holder +0x108 (current), A end = +0x168 (005b03a0 copies it).
   B likewise for a body-body node (+0x32 == 0). For a SOUP node (+0x32 != 0), B start = B end = the identity
   (00c02990), and B's data is the node's pool object at +0x28060[i], built by 005bc430 (1.1: the soup's
   triangles moved into the world, 64-byte records); the row is 11 -> 0. Input +0xc0 / +0xc4 = node +0x18 / +0x1c
   (the hull cache words), +0xc8 = the node index, +0xcc = 0. Every pass resets both holders' +0x244 to
   FLT_MAX and clears their +0x1d4 bit 3.
3. Hits are drained from the bins, sorted by t (+0xf8) with 00584e20 (ascending, assumption C1), and taken in
   order. For each hit (006e2c10 lines 28-301):
   - Gate: t <= A+0x244 and t <= B+0x244, else the hit is skipped. The node is then left as it is and is tried
     again next pass.
   - On the gate passing: a body-body node is REMOVED from the pair list (`*slot = 0`), so that pair is tested at
     most once per step. A soup node is kept, but the hit triangle's flag byte (record +0x3c, the +0xf0'th
     record) is set to 1, so the selector skips it from then on (its integer test).
   - Masks: m = ((B+0x9c & A+0x9e) | (B+0x9e & A+0x9c)) & 0x4d. This is symmetric in the two words, so which one is
     "send" does not matter here. m == 0: no response (and no gate write).
   - Normal: flipped in the record when n . (A origin - point) < 0, so it faces A.
   - Roots (+0x1fc, +0x204): a root with +0x1d4 bit 0x400, or the same root or owner, drops the response.
   - FLOOR OVERRIDE (lines 100-138; one side an avatar, holder bit 0x1000, the other not): when B's +0x9c has bit
     0x10 (GROUND), A's root has +0x1d4 bit 0x2000 or its +0x9e bit 0x800, and B's root has no body (+0xa4 == 0),
     then h = point.z - (Az - |up| * wrapper+0x54) is the contact's height above the feet. Az is A's origin z, via
     006e95c0; |up| is the length of the root's basis row 2 (+0x120..0x128); wrapper+0x54 is the centre's height
     above the feet, 006e9550. If h <= |up| * wrapper+0x50 (006e94e0; 24/121 of the height) + tol, the contact is a
     FLOOR. tol is 0.01, or 1.0 when either root has bit 0x10. The normal used for the separating test then becomes
     (0,0,1), or (0,0,-1) when B is the avatar, and A's POSE IS NOT COMMITTED. The record's own normal is
     unchanged.
   - Otherwise the commit: A+0x1d4 |= 0x4000, A's pose := the record's A at t (+0x00, inverse +0x30), when the body
     guard holds (+0x38 == 0, +0x34 == 0.0; disassembly 006e316b). B likewise with +0x60 / +0x90, only if it has a
     body. This settles C2: the poses are the cell's A / B at t (`pair_sweep` makes them bit-exact).
   - Separating test, with the normal above (the override's or the record's): if (dA - dB) . n >= 0 then m = 0.
     dA and dB are the holders' +0x1c8 displacements. Then A+0x244 = B+0x244 = t, whether separating or not.
   - Contact generation 006e6a10 (collision-contact-generator.md), then, when m != 0 and a contact came back and
     contact+0x1e8 == 0 and A is dynamic, the SIMPLE PATH. It uses the RECORD's normal n, never the override's:
     - D = A+0x168 origin - A origin (+0x12c, the committed one); s = (nz Dz + ny Dy) + Dx nx;
       A+0x168 origin = O + (D - s n), i.e. the end is slid onto the contact plane;
     - V = A's body velocity; vn = (ny Vy + nz Vz) + Vx nx; if vn < 0: k = vn - f, V -= k n. f is the friction
       term, 0 when A has the avatar bit 0x1000 (lines 246-258).
     - 006e92f0 / 006e8f00 (unread) add arm terms; the B-dynamic mirror is lines 303-398.
4. After the pass: 006e1bc0 and the cells again (the removed nodes are skipped), until a pass has no hit. There is
   no cap: each accepted hit removes a node or flags a triangle, and the least-t hit of a pass always passes the
   gate.
5. 006df7c0 lines 171-397 (read in outline 2026-10-17): the persistent-contact solver, not ported. It walks each
   body's contact list (+0x1e4) and each slot (0x58 bytes, count +0x1b8) with a negative separation (slot +0x54),
   and builds rows with 006ed510 / 006eba50 / 006ec0e0, using bias terms -(+0x2819c) * c * dt and -360 dt (lines
   171-175). Joint rows come from 006eca20 / 006ece30 over the +0x1e8 list. Then a worker task solves them (005a6910
   with 006ead20; the join at line 398). All of it is unread (the "Lux contact solver" debt).
6. Pose pass (lines 405-444): a body with bit 0x4000 (committed this step) ends at its predicted, slid pose: +0x198 =
   inverse(+0x168) by 0052ff10, then commit(+0x168). Disassembly 006e03aa-006e0410. A body without the bit is
   integrated from its velocity (`holder_advance_pose`). So a body that only touched a floor keeps the floor's
   velocity cut and moves by v dt.

### 1.6 The persistent contact object and the control step's planes (read 2026-10-17) [game]

Ported in `core\src\physics_manifold.rs`; references from `work\emu\gen_refs_manifold.py <mode>` (planes, insert,
refresh, control; 1,000 cases each, `work\emu\ref\manifold_*.tsv`): every word bit-exact.

- The object (500 bytes, 006d7280 allocates from pool 00d0b470, 006d7130 sets it up): five 0x58-byte slots, +0x1b8
  count (4 at most; slot[count] is the AVERAGE), +0x1bc friction = max(A, B +0x218), 0 when both are avatars (bit
  0x1000); +0x1c0 A, +0x1c4 B, +0x1c8 feature (rec+0xf0: a soup triangle's index, so one object per triangle);
  +0x1cc / +0x1d0 next in A's / B's +0x1e4 list; +0x1d4 / +0x1d8 the world list (+0x28070); +0x1dc / +0x1e0 the
  pair list; +0x1e4 margin 0.05 (0.1 when either shape's type, vtable +4, is 0xe); +0x1e8 both bodies dynamic; +0x1e9
  refreshed this step; +0x1ea read by the ground rule and the friction (writer unread); +0x1eb deep; +0x1ec ceiling;
  +0x1f0 = 0x4e01902e.
- Slot: +0x00 point in A's frame (rec+0xd8), +0x0c in B's (rec+0xe4), +0x18 B's world point, +0x24 A's world point
  (both the record's point at insertion), +0x30 normal (B toward A), +0x3c ground mark (+1 / -1 from 006e6a10's
  avatar rule, else 0), +0x40..+0x50 solver words (+0x44 is not set by 006e2c10's block: stack data), +0x54
  separation n . (A point - B point), negative when overlapping.
- Who adds a slot: 006e2c10 builds the block (above), 006e6a10 creates the object only when node+0x31 is set and the
  pair has none yet and the masks pass (param_8), flattens steep normals for avatars, marks ground, and calls 006d7830.
- 006d7830 insert: block+0x50 = 0; the slot whose A-local point is within (margin + 0.01) of the new one (006d7650,
  nearest) is overwritten; else appended; with four, 006d73f0 picks the one to replace (largest |(p - s_j) x (s_k -
  s_l)|^2, the Bullet rule without its deepest-point keep). The average: mean of the world points, normal summed and
  normalised (> 1e-8), solver words 0, its own separation. The last slot's +0x50 moves to the average. Then 006d9030.
- 006d9030 flags: +0x1eb when a slot's separation < -0.8 (an avatar on either side); +0x1ec when an avatar A has slot
  0's normal z < -0.2 (B presses down on it) or an avatar B has z > 0.2; that avatar's contacts then all lose their
  ground marks (slots 0..=count, the count being THIS contact's, for every contact of the list).
- 006e7550 (after the pass, before the solver): 006d7c20 once per contact, then 006e78c0 destroys a contact with no
  slot and slot 0's separation > 6; then islands (+0x1f0 island id, +0x1f8 next) for the solver.
- 006d7c20 refresh: friction again (0 when +0x1ea and an avatar against a dynamic B, or an avatar B against a dynamic A);
  every slot's world points from its local ones through the CURRENT poses (+0x108), its separation. Then from the last
  slot to the first: dropped when separation > margin * 4 (margin * 0.24 when both have bodies); else when A's point
  projected onto the plane through B's point lies more than sqrt((m + 0.01)^2 + 0.01) from B's point, re-cast
  (005be420, segment c + 3n .. c - 3n): against B from the projected A point, unless B moves and its sideways speed is
  the larger, then against A from B's point projected the other way; a miss or a far hit drops the slot. A dropped slot
  wakes both holders (00556ca0 / 00556d20, sleep counter 0) and the LAST slot moves into it; the average then
  accumulates at the new count, which drops what was summed before (kept literally; bit-exact). Averages divided,
  normal normalised, solver words +0x40 / +0x48 / +0x4c restored, +0x50 and +0x3c zeroed. The first kept slot (the
  highest index) fires the holders' listeners (+0x2c lists) with a state code (1-5 as in 006e6a10; not ported).
  The re-cast branch is ported but not covered by the references (the driver keeps the drift small).
- 006db490 control step (bit 5 clear, ported whole): wanted = (animated origin - origin) / dt, animated =
  0052fbf0(owner +0x10, holder +0xa8); v += (wanted - v) * control per axis; control * 0 when any contact of the list
  has +0x1ec. Planes: for the holder (and its +0x200 children) with bit 0x1000, contacts with count != 0 against a
  holder without a body; slot 0's normal, negated when the holder is the contact's B. They are used ALL OR NONE: only
  when at least one of them has ground mark 0. So a body on floors alone is not cut, and with a wall present the floors
  cut too. 006dd900 orthonormalises them (residue^2 > 0.001, three at most); 006dd5b0 removes each component with v . e
  < 0 (one-sided). 00557000 clamps each component to (-100000, 100000]. (The ledger's earlier "only those with +0x3c
  == 0 when any has it" was a misreading.)
- So far nothing of this is wired: the manifold needs the solver (006df7c0 171-397) to act on floors, and the sim's
  contact pass would have to keep contact objects per body between steps.

### 1.7 The contact solver (006df7c0 lines 171-404, read 2026-10-17) [game]

Static path ported in `core\src\physics_solver.rs`; references `work\emu\gen_refs_solver.py build|static|friction`
(1,000 cases each): bit-exact under the rcpps / rsqrtps assumption below.

- Biases (disassembly 006dfc08-006dfc83): f = (1/dt) * (1/60) (the float at world +0x2819c); -0.0 - (f C) dt with C
  = 1440 (a slot against a static holder: -24), 3240 when the contact is deep (+0x1eb: -54), 720 between two bodies
  (-12); joints -360 dt.
- Rows, per island, per root and its +0x200 children, per contact of its +0x1e4 list with +0x1e9 (refreshed this
  step) and count != 0: first 006ed510, the AVERAGE slot's restitution row (only when every lane of the approach
  speed is <= -1 and neither holder is an avatar; e = max(+0x214) * min(1, -vn / 3); list +0xbc, applied once after
  the iterations by 006eda30). Then:
  - not both dynamic (+0x1e8 == 0): sign = -1 when A is keyframed (body +0x38) and B has a body (the moving holder is
    then B), else 1. Every slot with separation < 0: n = the slot's normal, or (0, 0, ground mark) when the slot has a
    mark and v.xy . n.xy < 0 (v the moving holder's velocity, negated with the sign): walking into a slope is not
    pushed back sideways. n *= sign. 006eba50 with ECX = the moving holder.
  - both dynamic: slots with separation < -margin, the same normal rule on vA - vB, 006ec0e0 (not ported).
  - Joints of the +0x1e8 list: 006eca20 / 006ece30 with -360 dt (not ported). +0x1e9 cleared.
- 006eba50 static row (0x80): +0x00 n, +0x10 r = slot A point - root origin, +0x30 1/k with k = inv mass + n . ((I^-1
  (r x n)) x r) (rcpps and one Newton step), +0x40 bias = min(0, separation + margin) * C-bias, +0x50 impulse =
  slot +0x40 (warm start), +0x60 pseudo impulse 0, +0x70 the solver body's index, +0x74 the slot. Friction (006e92f0:
  the root's +0x1d4 & 3 != 1; a holder with bit 0 alone, the controlled one, gets none): t1 = normalize(z - (z . n) n)
  (zero on a level floor; 00579740), t2 = n x t1, row 0x70 {friction +0x1bc, 1/k1, 1/k2, t1, t2, slot +0x48, +0x4c};
  the normal row goes to list +0x2c instead of +0xc. The warm start (impulse n + t1, t2 parts) is applied at once.
- Iterations: DAT_00d3fa2c, 4 when GetSystemInfo reports more than one processor, else 1 (00ad6df0). Each: 006e9ef0
  (pair rows), 006ea270 (pair with friction), 006e9960 (static), 006e9b70 (static with friction), 006ea940 / 006ea7f0
  (joints). Run as worker tasks over islands (006ead20, 006eac10, 0117c4f0).
- 006e9960 static row: vn = n . (v + r x w) (w is the game's negative angular velocity), impulse' = max(impulse - vn /
  k, 0), the change along n applied (v += inv mass * P, w += I^-1 (P x r)); then the PSEUDO pass on the pseudo pair
  (+0x20 / +0x30 of the solver body): pseudo' = max(pseudo + (bias - vpn) / k, 0). Split impulse: the real velocity
  only loses its approach, the overlap is pushed out by the pseudo velocity. 006e9b70 adds, after that, both tangents
  from the same point velocity, each clamped to +-friction * the new normal impulse.
- 006ead90 write-back: v clamped to (-1e5, 1e5] into +0xa0, the pseudo v into +0xb8 (the "solver extras"), w and
  pseudo w unless +0x1d4 bit 0x200000 or 0x40; keyframed bodies zeroed; impulses into the slots (+0x40, pseudo
  +0x44). The friction impulses are NOT written back (+0x48 / +0x4c keep the generator's 0), so friction never warm
  starts. Then the pose pass: centre += (v + pseudo v) dt (006ddf30), and the extras are cleared.
- What this means for an avatar on a floor: most steps the slot does not overlap (separation >= 0) and makes no row;
  the contact pass's velocity cut (1.5) is what holds it. The solver acts when it sinks in: the pseudo pass lifts it
  at 24 per metre past the margin, the velocity loses only the approach.
- 2026-10-17, later: the REST of the solver ported, bit-exact (`gen_refs_solver.py pair|pairfriction|pairbuild|
  bounce|wjbuild|pjbuild|wjsolve|pjsolve`, 1,000 cases each):
  - pair rows (0x80: +0x00 n, +0x10 rA, +0x20 rB, +0x30 1/k, +0x40 bias, +0x50 / +0x60 impulses, +0x70 / +0x72 bodies):
    006ec0e0 builds them like 006eba50 with k over both bodies (inverse masses summed B + A); 006e9ef0 and 006ea270
    solve on the relative point velocity, +P into A and -P into B. 006ec0e0 is stdcall (solver, slot, n, A, B, bias,
    margin, friction); friction from A's 006e92f0.
  - restitution: 006ed510 (EDI = A; stdcall solver, average slot, n, B, &(-1 x4)) and 006eda30 (after the iterations;
    each row's impulse less its parts along earlier rows of the same A). Row 0x60: rA, rB, n, target, 1/k, indices.
    The two-body relative velocity is summed ((ptA - vB) - B's rotation part).
  - joints (rows 0x80: +0x00 K^-1, +0x30 rA, +0x40 rB or the world target, +0x50 err, +0x60 impulse, +0x70 bodies):
    006eca20 (EAX = joint; a joint holding A's anchor J+0x30 to the world point J+0x70) and 006ece30 (A's J+0x30 to
    B's J+0x70, both local). err = (target - p) * (-360 dt) per lane; K = m I - [r] I^-1 [r] (summed over both for a
    pair) inverted by cofactors (00531aa0 multiply, 00531b50 inverse, SIMD); warm start J+0x90 * J+0x80. 006ea7f0 /
    006ea940 solve: vel = (r x wp + vp) + (r x w + v) + err (the pseudo velocity counts), lambda = vel K^-1, impulse +=
    lambda, -lambda into A (+lambda into B) on the REAL velocity. 006ead90 writes J+0x80 back. Joint objects: +0xa0 /
    +0xa4 holders, +0xb0 / +0xb4 list links, +0xc0 two-body flag, +0xc2 refreshed (006e7550 sets it and +0xbc = -1);
    who fills +0x30 / +0x70 / +0x90 (the Link_* components, 4.1) is not read.
- [assumed] rcpps / rsqrtps as the exact reciprocal (square root) rounded to float: the emulator's behaviour, which
  the references use. Hardware returns a 12-bit estimate that differs between Intel and AMD; after the Newton step
  the results differ by an ulp or two. Nothing can settle it short of the user's own CPU.

### 1.8 The body's step: gravity, forces, prediction, advance (read 2026-10-17) [game]

Ported in `core\src\physics_body.rs`; references `work\emu\gen_refs_body.py gravity|integrate|inertia|predict|advance|
holder` (1,000 cases each): every word bit-exact.

- Body words: +0x0c..+0x14 the inverse inertia's diagonal, +0x18 gravity (a force, added each step without dt),
  +0x28 inverse mass, +0x2c / +0x30 damping, +0x3c force, +0x48 torque, +0x54..+0x74 the unscaled basis, +0x78 centre,
  +0x84 axis scales, +0x90 row quaternion (x, y, z, w), +0xa0 v, +0xac w (the game's negative), +0xb8 / +0xc4 the
  solver's pseudo pair.
- Order in 006df7c0 (1.5): 006e96c0 gravity and damping (force += gravity; v, w times +0x2c / +0x30; tiny or huge
  speeds zeroed), 006de230 (v += inv mass dt force; w += I_world^-1 (torque dt), I_world = B^T diag B from 006de390),
  the control step (1.6), then 006db1f0: the holder's +0x168 = 006ddad0 (q' = normalize(q + q o) with o = (w + pseudo
  w) dt/2 in row form, basis 0052e700 each row normalised, rows times the scales, centre + (v + pseudo v) dt), +0x198
  its inverse (005301d0), +0x1c8 = predicted - current origin. Gate: a body, owner +0x60 & 0x90 == 0x10, and a root or
  +0x1d4 bit 10. Children (+0x200) compose their local affine (not ported). 006d93c0 then refreshes the holder's swept
  broad-phase bounds (outline). After the solver, 006ddf30 advances the body by the same step (stored).
- dt reaches 006ddad0 / 006ddf30 in XMM0 and 006de230 in XMM1 from the world step (not set by 006db1f0).

### 1.9 The robot's world step, wired (2026-10-17)

`core\src\physics_avatar.rs` `step` runs 1.5-1.8 in the order of 006df7c0 for one avatar against static bodies;
`LevelWorld::step_avatar` builds the nodes and the re-cast, `World::step_avatar` (sim.rs) takes it for the robot on
foot on a level (Stand, Move, Strafe, Jump, the actions that are not climbs): his position and velocity come from
it, no floor probe snaps him. The car, the climb and the arena without a level keep their old paths.

- Order: 006e96c0 (gravity as a force, damping +0x2c 0.9995), 006de230, 006db490 over the kept contacts (ceiling,
  planes, clamp), 006db1f0, the pass (`physics_contact::contact_pass_with`, the generator called on each responding
  hit), 006e7550 (refresh each contact whose +0x1e9 is clear, then set it; destroy when empty and slot 0's separation
  > 6), the solver (`solve_against_static`, no friction rows: the controlled root), 006ead90's clamp, the pose pass
  (committed: the slid predicted end; else 006ddf30 with the pseudo velocity).
- The generator's part for an avatar A and a static B (006e6a10, re-read 2026-10-17): the block of 006e2c10 lines
  171-187 (+0x00 rec+0xd8, +0x0c rec+0xe4, +0x18 / +0x24 the point, +0x30 the normal toward A, +0x3c 0, +0x54 0;
  `hit_block`); the ground mark +1 when B sends GROUND and A's point is within the floor reach + 0.01 above the
  feet (`ground_mark`, lines 41-56, after the commit); the contact: node +0xc, else 006d7230 (pair and feature), else
  a new one (006d7280); node +0xc then points at it (so a soup node's later triangles go into its FIRST triangle's
  contact while the node lives: 0117c3c0 clears +0xc when the contact is freed); the steep rule (|nz| < 0.9 and the
  contact a ceiling, both roots avatars, or nz < -0.001: z dropped, the rest divided by its length, 005571b0;
  `flatten_steep`); then 006d7830. Only a responding hit adds a slot (param_3 and param_8 are 0 otherwise).
- On Shanghai (`pencil_walk`): he stands with his feet 0.07-0.14 under the floor probe's height (the pencil's tip,
  2.5/121 H above the feet, rests on the floor; the overlap's pseudo push holds it), walks free in the open, stops or
  slides at walls, rides over kerbs, walks off a 246 m roof and lands. What the game adds on top (008524b0's push of 5
  x the contact normals, its landing correction) is not ported.
- [stand-in] the mode's blended x / y velocity is the control target with the control (1, 1, 0) (the sim keeps its
  own blend); the nodes are every primitive whose box meets the step's box, rebuilt each step. (The ground flag's
  stand-in was replaced the same day by the game's ground test, 1.10.)

### 1.10 The owner's ground test 008524b0, wired (2026-10-17) [game]

`physics_avatar::ground_test`, against `work\emu\gen_refs_ground.py` (008524b0 run whole; in emulator memory only, its
tail call 00853420 made a `ret` and the mode object a fake returning MoveMode's id): 1,000 cases, velocity, flags,
+0x334 and +0x340 bit-exact; every branch hit (403 pushes, 306 cone flags, 17 landing sets). The reading is
`docs/character-ground-test.md` and `docs/character-height-fields.md` section 7; what this pass added:

- The slippery normal is each slot's own (+0x30 + 0x58 k), not block 0's; the first one found is slot 0 of the first
  contact whose other body sends SLIPPERY (0x200), so the result is the same. The push is along (x - x k, y - y k, -z k),
  k = x^2 + y^2: down the slope's tangent, 5 m/s, on slippery ground only.
- STEP is the modes' owner (mode +4), so +0x3e4 is set by MoveMode's enter 00875eb0 and cleared by its exit 00875ec0:
  "MoveMode is the mode". The cone axis (STEP+0x2c's +0x1c) is the character transform's second row: his forward.
  The new contact goes to the head of the list (006d7280, character-ground-test.md 2): A-PH15 settled.
- Wired in `LevelWorld::step_avatar` after the world step (as the next update's start does it): the kept contacts
  with their static body's send mask, then the probe for the next test (00853420): a ray from 1.25 above the feet 10
  m down, mask 0x41, answered at once. The record lives in `AvatarBody::ground`; `FeetStep::standing` is its +0x26c.
- Seen on Shanghai: walking over bumps he no longer rides up (highest 4.21 -> 0.83 on one heading): the landing set
  pulls the vertical speed down to follow the ground while MoveMode holds the flag.
- [assumed] the probe's distance is in metres (record [10]); it starts at the feet (the transform's position); the
  static world gives no held handle; +0x26c starts clear; the port's Stand (the unnamed first mode) does not set +0x3e4.

### 1.11 The enemies and the car's chassis through the world step (2026-10-17)

- Enemies: `combat.rs` `FightWorld` forwards `World::step_avatar` and `step_chassis` with `ai` set, and answers
  `ai_bodies()`; the enemies' `sim::State`s keep their own `avatar_body` / `chassis_body`. An AI body uses the holder
  masks `AI_CHARACTER_MASK` (PUSH | AVATAR_PUSH | AI_PUSH, 0x0d) and the ground probe mask 0x09 (00853420's AI
  value), so the level's AI-only walls (`StaticAIPushOnlyObj`) stop them and not the player. Before this they kept the
  stand-in cylinder push. `pencil_walk --ai` walks with them (same as the player's on Shanghai).
- The car: `physics_avatar::step_chassis` (`LevelWorld::step_chassis`, `vehicle::Chassis::advance_kept`, the contacts in
  `State::chassis_body`). The car's own integration stays as it was (gravity, springs, tyre forces, damping, the
  rotation step: compared with the recordings); after it, the world step's rest: the prediction (centre + v dt at the
  integrated rotation), the pass with the generator, the refresh, the solver with the cube's arms (inverse mass
  1/5000, the world inverse inertia the car's integration uses, w the game's negative), the clamp, and the pose pass
  (committed: the slid end at the predicted rotation; else centre + (v + pseudo v) dt and the rotation advanced by the
  solved w + pseudo w). `chassis_drive` (Shanghai, eight headings): ends within a metre or a few of the pass-only
  path's (`--pass`), no fall-through, walls still stop it. The pass-only `move_chassis` stays as the fallback for a
  world without `step_chassis`.
- `pass_and_solve` is the shared middle (pass, generator, refresh, solver); the robot's `step` and `step_chassis`
  call it.
- [assumed] A-PH22 an AI's holder masks are 0x0d. A-PH23 the car's root gets no friction rows and the drive form's
  control step (006db490) is not run (its control vector is unread). Also A-PH8 / A-PH9 as before for the car.
  A-PH22 SETTLED the same day: the ledger's 006e7b60 line has "mask 0x45 player / 0x0d AI".

### 1.12 Body pairs: characters with each other and with the car (2026-10-17)

Read [game]:
- 006e2c10 lines 204-632: after the generator, a contact with +0x1e8 clear (B static or keyframed) takes the simple
  path (1.5); one with +0x1e8 set (both bodies dynamic) takes the TWO-BODY path (lines 400-632) and there is no slide:
  rA = point - A's origin, rB = point - B's (006e9460), n = the generator's block normal (z dropped when +0x1ea),
  vn = n . ((vA + rA x wA) - (vB + rB x wB)) (w the game's sign; 006e9250 is v + r x w), K = 1/mA + 1/mB +
  n . ((IA^-1 (rA x n)) x rA) + the same for B, P = max(-vn / K, 0); 006e8f00 applies +P n at rA to A and -P n at rB
  to B. The average slot's +0x50 takes -min(1, -vn/3) max(+0x214) vn (a restitution term), friction impulses only
  with 006e92f0.
- 006e8f00 (EDI holder, ESI impulse, stack arm): wakes a sleeping body when |P inv m|^2 > 10.1; 006e9070: v += P inv m
  (only when +0x38 == 0 and +0x34 == 0), w += I^-1 (P x r) via 006e9160; holder bit 0x40 or 0x200000: linear only
  (00556f80), and bit 0x40 zeroes w after. 006e9340 is empty.
- The pose pass, re-checked at 006e03aa: a committed body (bit 0x4000) commits +0x168 itself: it ends at its
  unslid prediction. 006e1bc0 sweeps a body-body pair from B's +0x108 to its +0x168.
- Masses: 006e7b60 -> 006e7e90 -> 006d9190 builds the pencil's mass properties (mass = the given one, else the
  volume; a cylinder and two cones' inertia), 006e7e40 copies them and 006e8e20 sets body +0x24 mass, +0x28 1/mass,
  +0x0..+0x8 the inertia diagonal and +0xc..+0x14 its inverse. The given mass is the avatar descriptor's +0x14
  (00851870 scales it): Bumblebee robot 1000, car 5000 [data] (`Tuning::robot_mass` / `vehicle_mass`, new).
- CORRECTION (2026-10-20, report T1): 006e7b60 sets more than the avatar bit 0x1000: its holder_control_set call
  (006e8da0, ECX the body) sets bit 0x40 (no 0x20). So 006e8f00 makes a robot's impulse linear only and zeroes its
  w, and solver_write_back (006ead90) skips w when 0x40 or 0x200000 is set: a robot NEVER turns from a push. The car
  (0x200000, drive_enter) also takes linear-only impulses but keeps the w it had. Pencil mass record 006d9190:
  rec[0] = rec[4] = mc (L^2/3 + r^2/4) + 2 mk (0.1 g^2 + 0.15 r^2), rec[8] = mc r^2/2 + 0.6 mk r^2 (cylinder L = 6/11 H,
  cones g = 5/11 H, mass split by volume); ported as `pencil_inverse_inertia`.

Ported (`physics_contact` `StaticTarget::Body`; `physics_avatar` `BodySnapshot`, `pass_and_solve`): a body pair in the
pass (B swept from its pose to its prediction, no floor rule, the separating test on dA - dB, no slide), the
two-body response in the generator's hook, both-dynamic contacts (friction 0 between avatars, steep normals
flattened: both roots avatars), the refresh against B's pose, and pair rows (006ec0e0 / 006e9ef0, bias -12 per
metre) before the static rows in each iteration. Hosts: `Arena::bodies` (the game's `targets::update` keeps one per
live enemy; `combat` adds the player's for each enemy's step), `State::body_snapshot`, and the target cylinder push
is skipped on a level for a target with a body.

[stand-in] the game runs ONE world step for all bodies; the port steps them one after another, each taking every
other body from its snapshot as the frame began (`BodySnapshot`), doing the pair from its own side (A) and keeping
only its own share; B is not committed and its changes are dropped. Both holders keep a contact for the pair, as
both lists do in the game. The re-cast against another body is not done (the slot is dropped). The bodies within
50 m stand in for the broad phase.

SETTLED 2026-10-17 by 1.13 (the rest of the broad phase): walkers are not swept against each other at all; the
discrete test gives the pair a contact each step and the solver holds them apart. The finding as it stood:
FOUND, open: as read, two WALKING characters work through each other. The ground control (1, 1, 0) [trace] gives
back the walking speed every step, the two-body response only cuts the approach for that step, a committed body
ends at its prediction, and the solver's pseudo push is unused by a committed body. The unit test
`walkers_work_through_each_other_as_read` and the capture `work\shots\body_pairs.png` (a scout standing inside him)
show it. Coasting bodies do stop each other (`two_coasting_bodies_stop_each_other`). What may change it is unread: the
rest of the broad phase 006e5e00 (which pairs it makes, in which order, with what +0x31), or a gameplay push.
- [assumed] (status.md, step 5 wiring entry): the avatar does not turn and has inverse mass 1; node +0x31 set for
  every static pair; nodes persist while their contact lives; a new contact goes to the head of the list; the
  avatar's and the static bodies' friction words are 0 (unused without friction rows); the robot's damping is the
  captured 0.9995.

### 1.13 The broad phase in full: swept pairs and DISCRETE pairs (read 2026-10-17) [game]

006e5e00 read in full (lines 83-258 were the unread part). For each holder of the world list (DAT_00cf59ec, next
+0x25c; children +0x200), over its pair nodes (+0x248; next +0x20 when the holder is the node's +0x10, else +0x24)
not yet seen this step (+0x30), kept when one holder is awake (+0x1d4 bit 3), the masks overlap RAW ((A+0x9c &
B+0x9e) | (A+0x9e & B+0x9c), no 0x4d), both owners enabled (+0xc -> +0x60 bit 1 clear) and the group matrix
(+0x10008, bit A+0x1e0 * 16 + B+0x1e0) allows it:
- EVERY kept node goes on the DISCRETE list: +0x2814c (a body pair, +0x32 == 0) or +0x2815c (a soup pair).
- It goes on the SWEPT list +0x28050 (the one 006e1bc0 / 006e2c10 walk, 1.5) as well only when the holder moves
  (has a body, bit 22 of +0x1d4 clear, |d|^2 > 0.0001, d = +0x1c8) and |d|^2 > r^2 or |d|^2 > 1.4. r is 006dcf00's
  (below) for the holder, min with B's when B has a shape but no body; a B with a body uses the holder's alone.
  So a walker (0.2 m a step, pencil radius 2: r = 1.6) is NEVER swept, against bodies or the world; a car is swept
  only past about 31 m/s (r = its 1.0 half height) or sqrt(1.4) = 1.18 m a step (37 m/s).
- Then +0x30 = 1. A soup node on the swept list gets its triangle records (005bc430) in the pool +0x28060.
After the loop: the probes (006df050, 006e0e50), 006e1bc0 + 005e3d50 + 006e2c10 (the sweep and its hits), then
006e2200 (body pairs) and 006e2110 (soup pairs) queue the discrete tests, 005b7150 runs them, 006e22f0 hands the
results to the generator, 006e2a10 collects the probes.

- 006dcf00 (returns xmm0): r = 0.8 x (support along unit d minus the shape's centre, dotted with unit d; 005b7260
  with table 00b8dd94); a soup (type 0) 0.01; a cube (type 2) min(r, least half extent x its row's length, 0056fa90).
- 006dd050: node +0x31 = 1 when either holder has a body (+0xa4), the holder with a body at +0x10.
- 006e2200: per body-pair node, +0x30 = 0, both holders' bit 3 cleared, 006dee10: the test of the two holders at
  their CURRENT poses (+0x108 and the inverse +0x138, after any commit of the sweep), queued by 005b6a70 into the
  CollisionBatch [typeA][typeB] (DAT_00cf5808, 11 x 11 pages of 256 tests; when typeB < typeA the pair is swapped:
  the axis +0x20 negated, +0x34 = -1 - index, +0x2c / +0x30 exchanged). 006e2110 likewise for soups (006def20: A's
  world box from the holder's +0x220, 006daa90; the soup's records; info +0x30 = -1).
- The batch (005b7150 -> 0117b520 "CollisionBatch::SpawnThreads" -> task 005b6e50): kernel 005b4aa0 when typeB != 0,
  005b61e0 for soup x type 1 (sphere), else 005b4d60 (soup x other); results 0x48 bytes, popped LIFO per page by
  005b71a0 in row-major page order.
- 005b4aa0 (cdecl, 11 args: supA, supB, A data, B data, info, &count, out, A world, A inverse, B world, B inverse):
  record +0x38 = info+0x34, +0x3c = info+0x2c, +0x40 = info+0x30, +0x44 = 0; cA = A origin, cB = B origin (w 1),
  v0 = cB - cA; MPR 005b3c50 (dir cA - cB, C = [cA, cB, v0]); count + 1 on a hit.
- 005b3c50 is XenoCollide's MPR on B - A (supports: A's at -n, B's at n): the |n0|^2 < 1e-5 guard (n0 = (0,0,0.001)),
  +0x34 = -1; v1 (miss when v1.n0 <= 0); n = v1 x v0, collinear (|n|^2 < 1e-5: contact v1's points, normal
  normalize(v1 - v0), +0x30 the depth, done); v2 (miss when v2.n <= 1e-5); n = (v1-v0) x (v2-v0), swapped when
  n.v0 > 0; portal discovery (255 tries: v3, miss when n.v3 <= 0; (v1 x v3).v0 < 0 -> v2 = v3; (v3 x v2).v0 < 0 -> v1 =
  v3); refinement (199 tries: n = (v2-v1) x (v3-v1), miss when |n|^2 <= 1e-5, hit once n.v1 >= 0, unit n by rsqrtps
  + Newton, v4; done when (v4-v3).n <= 0.001 or -(v4.n) >= 0; else v4 replaces v1 / v2 / v3 by (v4 x v0)); the
  contact: barycentrics (v1xv2).v3, (v3xv2).v0, (v0xv1).v3, (v2xv1).v0 (their sum <= 0: 0 and the three against
  n), rcpps + Newton, pA / pB from the weighted A / B parts. FIRST time (+0x44 clear): +0x44 = 1 and the test runs
  again from cA' = pA, cB' = pB, v0' = (pB - pA) - n along n (a miss there clears +0x44). SECOND time: +0x24 = n,
  pA and pB moved onto the plane through v1's points (p - n ((p - v1side) . n)), +0x18 = ((pB - pA) . n) n.
  Every dot is (y + x) + z; 00531a60(a, b) is b x a; the order of every sum is in `core\src\mpr.rs`.
- World supports (00b8dd94: 005b7310 triangle, 7330 sphere, 7360 cube, 7390 capsule, 73c0 cylinder, 73f0 cone, 7420
  pencil, 7450 / 7480 / 74b0 / 74e0 hulls): d with w = 0 through the inverse rows ((y R1 + x R0) + z R2) + 0 R3, the
  local support (w 1; the pencil's is the SSE 005b7cf0, not 005aff10), out through the world rows. 4x4s from
  00550a30 (rows [m0 m1 m2 0] [m3 m4 m5 0] [m6 m7 m8 0] [m9 m10 m11 1]).
- 006e22f0, per result with +0x44 set: the node (+0x38 into the list; negative: swapped); node +0x0..+0x8 = the
  record's +0x18 when its |.|^2 > 0.001, node +0x18 / +0x1c = +0x3c / +0x40; the block for 006e6a10: +0x00 A's point
  in node A's frame, +0x0c B's in node B's (the A and B transforms differ in the order of y and z), +0x18 B's world
  point, +0x24 A's, +0x30 the normal (negated when the node's contact has A = the node's B, xor the swap), +0x3c 0,
  +0x54 ((PA - PB)x nx + (PA - PB)z nz) + (PA - PB)y ny; feature -1 for a body pair (the triangle number for a
  soup); create when node +0x31 and no contact at +0xc; node +0xc = the contact.
- 005b4d60 (soup x shape, read, not ported): the soup's vertices to the world (soup matrix), per triangle (index
  triples after the vertices, 0x80000000 flags an edge): its box against A's world box (info +0..+0x1c), the plane
  distance of A's box centre within the box's half diagonal, then 005b4aa0 with B = the triangle (support 005b8650
  over its three world vertices; cB = vertex 0, B world = identity at vertex 0), the record's +0x34 = the triangle
  number; flagged edges tested by 005b4bc0 (the contact on that edge: the record keeps MPR's normal), else +0x24 =
  the face normal.

Ported (`core\src\mpr.rs`, bit-exact): `pair_test` (005b4aa0 + 005b3c50), the cube (005b8440) and pencil (005b7c20 /
005b7cf0) world supports, `sweep_radius` (006dcf00). References `work\emu\gen_refs_mpr.py <typeA> <typeB>`: 3,000
cases ([6][6] pencils, [2][6] cube x pencil, [2][2] cubes; upright and turned, overlapping or not): every word of
every record bit-exact, and the holders' inverses (0052ff10, run in the emulator) equal `inverse_affine3`.
Wired (`physics_avatar::pass_and_solve`, the robot, the enemies and the car alike): the gate (a body pair is swept
only past it), then the discrete test of every pair with another body at the poses after the pass, its block to
the generator (`discrete_block`, feature -1), the push left to the solver. Two walkers pushing at 6 m/s now stand
3.6 m apart (0.38 overlapped), still (`walkers_hold_each_other_apart`).

Left: the soup kernel 005b4d60 with the shapes' world boxes (00b8ddc0 per type: 005ae450 pencil, 005ad020 cube, x87
over compose_affine 0052fbf0) and 005b4bc0, the hull / other supports, 005b61e0; then the STATIC pairs too go
discrete (the walker is never swept against the world either; the port still sweeps every static pair).
- A-PH26 (2026-10-17) the sweep gate is the stepping body's: the game gates a node by the first holder of its world
  list that reaches it (+0x30), and a moving B's branch of the condition is garbled in the decompile. Settles: the
  disassembly of 006e5e00 lines 116-140.
- A-PH27 (2026-10-17) between two bodies of one shape type, the batch's A is the lower actor id (the game's is the
  node's +0x10, which 006dd050 orders by address when both have a body). Changes which side MPR starts from.
- A-PH28 (2026-10-17) the node's A (the transform order of +0x00 in the block) is the batch's A; and a node's
  contact, which the game shares between the two holders, is each body's own (the stand-in of 1.12).
- A-PH29 (2026-10-17) the pair is awake (+0x1d4 bit 3) and the group matrix allows it; the results of several pairs
  are taken in the order of the snapshots (the game's: batch pages row-major, LIFO within a page).
- [stand-in] the static pairs stay on the sweep for every speed (the soup kernel and the boxes are not ported).

## 2. Small engine helpers read on the way [game]

- 005502f0 `vec4_normalize_copy` and 005571b0 `vec4_normalize_to`: SSE, ECX = (x, y, z, len), EAX = out;
  out = xyz / len when len > 1e-8, else xyz unchanged (disassembly).
- 00612530 copies one 0x108 output record.
- 005be420 `contact_recast_nearest` (section 1.2).

## 3. Ray tests, table 00b8dd0c [game, by template diff]

005c5020 capsule, 005c6a50 cylinder, 005c8480 cone, 005c9eb0 pencil: 97.4 % token match with each other, each
calls only its own support. 005cb8e0 Hull32, 005cd1a0 Hull16, 005cea60 Hull8, 005d0320 Hull4: 100 % token
identical, each calls its hull support. They are the GJK ray-cast template of 005bed70 (the triangle test,
`docs/collision-core.md`) with the same Johnson step as 1.3, but compiled in the INLINE add orders (1.4,
2026-10-17): `gjk_world::ray_test`, lambda bit-exact on every ordinary hit of the cube, capsule, cylinder,
cone, pencil, hull8 and hull16 ray files, hit/miss and contact flag on every row.

## 4. Gameplay physics components [game unless tagged]

### 4.1 Joints

- Link_BallJoint (registrar 00ad5fc0, flag 2; factory 0057fc70 -> 0057f720): fields `parent`, `dampCoeff`
  (default 1.0), `linksCollide` (default 1, stored inverted as noCollide), the pivot vec3 (hash 462ce4f5, name
  unresolved), optional 88c449c2 and 6a57c0ea (read, not used here). Finds the two bodies through the physics
  registry (key cc837a29), takes the pivot from the parent node's frame into world and then into each body's
  local frame (body inverse at +0xd8..0x104), and calls 006e05f0, which writes the joint: pivot in A +0x30
  (w 1), pivot in B +0x70 (w 1), damping clamped to [0.2, 1] four times at +0x90..0x9c, noCollide +0xc1. The
  joint solver itself is in the world step (006df7c0's unread middle, census `vehicle`).
- Link_Spring (factory 00580180 -> 0057fc80): `parent`, a float 0709907d (default 1.0; [assumed] the stiffness,
  name unresolved), `dampCoeff` (default 1.0), the pivot; same body and pivot set-up; spring_create 006e06e0.
  spring_apply 006da400 runs in the world step (`docs/driving.md`).
- Link_Hinge (registrar 00ad6020): its factory is 008b7f50, which returns 0. The hinge has NO runtime in this
  build: data that asks for one gets nothing.

### 4.2 PhysicsOverride (registrar 00ad9b10, flag 5; factory 0077ee50 -> 0077eb30)

Per node of the object, the body from the physics registry (cc837a29):
- `startAsleep` (default 1): sleep counter to 1000 (asleep) or 0 on the body AND, recursively, on every body
  joined to it (0077eab0 walks the joint list +0x1e8: bodies at joint +0xa0/+0xa4, next +0xb0/+0xb4).
- d1156b91 (int), a2f0a70b (vec3), 4212fc4a (vec3): when any is non-zero, the vec3s go to the body WRAPPER
  (holder +0xa4) at +0x2c..0x34 and +0x38..0x40, and holder flags +0x1d4 become: clear 0x02 and 0x10, set 0x01,
  and 0x40 set when 0x20 is clear (cleared otherwise). [assumed] these are initial linear and angular
  velocities and "keyframed/controlled" flags; the names are not in names.tsv. Settles by the wrapper's field
  readers in the world step (006db490 body_control_step reads holder flags 0x01).

### 4.3 Wind

The wind state (007bfc00, a static) holds a global vector, a gust list (count at float index 12, records of
position and vector, 6 floats each) and a list of wind volumes (weak handles). The sample 007bfc70(state, out,
point, mask):
- mask 1: + the global vector;
- mask 2: for each gust, d = |point - gust|; f = 1 - d * (2/3) (so a radius of 1.5 m); when f > 0, + radial
  unit (point - gust) / d * f * |gust vector|;
- mask 4: for each live volume, if slot 6 (contains point) then + slot 7 (wind at point); dead handles pruned.

Gusts come from characters (007c0020: the character's camera target point, 0071a2e0, and its body velocity,
desc +0xa0): a moving character pushes loose things around it.

Volumes:
- WindDirection (registrar 00ade0f0; 0081a940): `strength`. Update 0081a9c0: when the object's first matrix row
  (+0x10..0x18) changes, the GLOBAL wind = that row * strength.
- WindArea (00ade150; 0081ac90): `strength` (+0x94), `area` element (its inverse, 0056f2d0, at +0x34..0x60).
  Activation 0081aa60 stores the object's inverse (+0x64) and registers the volume; contains 0081aec0: point
  through the object inverse then the area inverse, inside when all three coordinates are within [-1.01, 1.01];
  wind 0081a860: cached first row * strength.
- WindSphere (00ade1b0; 0081b170): `radius`, `strength`, `lifeTime`, age. Contains: |p - centre|^2 < r^2. Wind:
  radial unit with its z clamped to >= 0, times (1 - d / r) * strength, times (1 - age / lifeTime) when
  lifeTime > 0. Update: age += dt; at age >= lifeTime the object is removed.
- WindPhysicsReactor (00adaa90; 007c0430, ctor 007c0230): scale 7d09f598 (default 1.0). Update 007c0320(dt):
  wind = sample(mask 7) at the object's position (+0x34); force = wind * scale * mass (body +0x24) * dt * 60,
  added to the force accumulator (body +0x3c..0x44, 007c0710). A sleeping body (counter > 249) is woken and
  pushed only when |force|^2 > 8.1.

### 4.4 ActivateCollision (registrar 00adad00; 01186370)

A script object: `startActive`, and either `maskPreset` (resolved in the preset registry 0117b8e0 to a send
and a receive mask) or `sendMask` / `receiveMask` (u16 at +0x46 / +0x48). On activation (007c2300, once), when
startActive, the masks are broadcast to the object's body listeners (007c2130 -> physics_body_notify_list
00556050); 007c20b0 / 007c20f0 are the same message with the masks in the other slots (set against clear).
`docs/collision-query-masks.md` has what the masks mean to the queries.

### 4.5 GroundPositioner (registrar 00adc1a0; 007e8a10, ctor 01188870)

On activation (007e8f20) it submits a probe from the object's position straight down 1024 m (mask 0x11,
probe_submit 006e0a30). Update 007e8b10: on a hit (distance +0xe4 < FLT_MAX) it builds a frame from the ground
normal (|n.z| >= 0.9 gives the default frame; else cross products with x), sets the object's affine and
z = start z - distance. For Devastator (2839daf3, 007e8a40) it follows the character instead, with a pencil
collider (006e8da0).

### 4.6 Other registrars in the census

Collision (00ad6d80, factory 005bac50 -> 005b95e0, the loader of `docs/collision-loader.md`), Flippable
(00adc580, factory 007eeae0: the runtime's 85 functions are in other census systems), PhysicsSpawner (00adcc10,
factory 007f9770, the ObjectSpawner path of the unpublished notes).

## 5. Rows the census labelled physics by address only (read and noted, not physics)

- The Bink movie player: 005abaf0, 00702330, 0077ae20 (the startup logos), 0077e6b0..0077e9a0, 0077ea20,
  01184380, 011843c0 (pause movie start, stop, clock).
- VO conversation seats: 00818820, 00818a10, 0081a1d0..0081a3b0, 0081a430, 0081a730.
- AIEngageDetector registry (map 00c067bc): 007c0800, 007c0a50, 007c1b00, 007c1c70, 007c1e90..007c2060,
  01186190..011862e0, 00850760.
- Containers and listener lists: 0057ef20, 0057efe0, 0057f120, 0057f1f0 (an array of 16-byte items), the
  listener attachers 01176cb0, 01176d40, 01176dd0, 01176e60, 01176ef0, 01191490 and their thunks, 01176f80,
  00aa05f0, the map 00cae42c (0081a540..0081a6f0), 0081a480.
- AI goal pieces: 0081b3e0, 0081b5e0; a jittered spawner component 007c0460; stubs 0057f290, 0057f520.

## 6. Port plan (in order)

Each step lands in `core\` with tests; references come from the emulator (`work\emu\`), which already runs the
pair cells whole (`docs/collision-tests-emulated.md`, pair section: 36 cells of the six simple shapes).

1. DONE 2026-10-16 (see G1 above). `gjk.rs`, exact Johnson (G1): a fixed 4-slot simplex with bits / last / last_bit, the edge-form det table in
   f64, SOLID's closest with the descending subset order and the exit when nothing is valid; v divided only
   when the sum > 1e-8. Transliterate the 4-point block (006177a0..00617bb8) from the disassembly, not the
   decompile. Test: the existing ray references (`reference_ray_tri`, `reference_ray_sphere`) must not lose
   cases; the 4 % of "WHY A PORT CANNOT MATCH" (`collision-core.md`) is the target. Settle the x87 control word
   first (or test both), since the det arithmetic is x87.
2. DONE 2026-10-16. `gjk.rs`, start-in-contact normals (G2) and the witness weights (G3).
3-4. PARTLY DONE 2026-10-16 for sphere and cube (cells [1][1], [1][2], [2][1], [2][2]): `core\src\pair_sweep.rs`
   (`pair_cell`, `inverse_affine3` = 005301d0, the pair cube, the record, the re-cast) against
   `work\emu\gen_refs_pairs.py <row> <col>` (400 random swept cases each, rotations included,
   `work\emu\ref\pair_<row>_<col>.tsv`): hit/miss 400/400 per cell; every record field bit-exact on every
   ordinary hit (asserted); on start in contact all fields but the normal bit-exact, the normal bit-exact with B a
   sphere, within 8e-7 with B a cube (the cube ray test's remaining inline arithmetic). The primitive is
   `gjk::ray_cast` itself (no separate `pair_sweep` loop was needed: 00616ed0 and 005bed70 are one template).
   2026-10-17: ALL 36 cells of the six simple shapes DONE. `PairShape` has capsule (nine floats: ends at
   +0x0 / +0x10, radius +0x20), cylinder (r, h), cone and pencil (r, h, a, c), the layouts of
   `gen_refs_rayshapes.py`; the inline cells run `gjk::INLINE` with their own support sums (1.4); the re-cast
   goes through `gjk_world::ray_test`. 14,400 reference rows: hit/miss on every row and every field of every
   hit bit-exact, ordinary or in contact (`pair_sweep::tests::reference_pair_cells` asserts it).
   2026-10-17, later: the HULL rows and columns DONE for Hull32, Hull16, Hull8 (types 7-9): 45 cells,
   18,000 rows, every field of every hit bit-exact (ordinary and in contact, the walk units out included).
   `PairShape::Hull { width, block }` over the walk of `gjk_shapes::hull_support`; the generator builds each
   hull's block in emulator memory (`gen_refs_pairs.py` `place_hull`, gen_refs_rayhull.py's hull set) and
   appends the hull to the row. Detail: 1.2 (cache words) and 1.4 (the hull helpers' orders).
   2026-10-17, later still: the SOUP ROW DONE (cells [0][1]..[0][9], 1.1): `pair_sweep::soup_cell` over
   `PairShape::Triangle`, 9,000 rows (1,000 per cell, 1-6 random triangles each, `gen_refs_pairs.py` `soup_tris`):
   hit/miss on every row and every field of every hit bit-exact, ordinary and in contact.
   Left for 3-4: Hull4 (type 10: its support 005afdf0 is unread, links in nibbles, cache not divided), as a column
   and as a row, the soup cell [0][10] included.
3. (original text) New `core\src\pair_sweep.rs`: the primitive of 1.3 over a `WorldSupport` trait (point + w), the helper rule of
   1.4 (B + A, same direction, A as witness), the 60-byte record, cap 298. Shapes: reuse `gjk_shapes.rs` and the
   transform helpers' ports; add the cube's inline sign support. Tests: emulator references for all 36 simple
   cells (exist) plus new ones for the hull rows and the inline family (`work\emu\` drivers per cell).
4. The wrapper (1.2) and the soup selector (1.1): the 0xd0 input and 0x108 output records, end-pose bases,
   relative sweep, 005be420's re-cast on start in contact. Tests against the emulator's runs of 005e5030 and
   005e4100.
5. 2026-10-17, later: the contact objects and the control step's planes DONE in core (1.6, `physics_manifold.rs`,
   bit-exact), not wired. Later the same day the solver's static path (1.7, `physics_solver.rs`, bit-exact): rows,
   both static solvers, the per-body solve with the impulses back in the slots. Not ported: the pair rows (006ec0e0,
   006e9ef0, 006ea270), restitution (006ed510, 006eda30), joints. Later still: ALL of those ported bit-exact (1.7),
   and the body's step (1.8: gravity, forces, prediction, advance). Later still: WIRED for the robot on foot on a
   level (1.9, `physics_avatar.rs`): his contacts kept between steps, the whole step, floors in the pass; then the
   owner's ground test 008524b0 bit-exact and wired (1.10); then the enemies and the car's chassis (1.11); then body
   pairs (1.12: walkers work through each other as read, open). Then the broad phase in full (1.13): the sweep
   gate and the discrete MPR test (`mpr.rs`, bit-exact), wired for body pairs: walkers hold each other apart.
   Left: the soup kernel 005b4d60 and the shapes' boxes, so that static pairs go discrete too; the simultaneous
   world step for all bodies.
5. STARTED 2026-10-17 (1.5): `physics_contact::contact_pass` and `StaticWorld::pair_nodes`, wired for the robot's
   walls on levels (`World::move_pencil`). Later that day the car's chassis too (`World::move_chassis`,
   `LevelWorld::move_chassis`: the game's CollisionCube of the vehicle avatar, 00853a10 / 006e81e0, turned, in the
   same pass; `core\examples\chassis_drive.rs`). Left: the body's own prediction and control step, floors in the
   pass, the solver of 006df7c0 171-397, the arm terms (angular response). Original text:
   Wire: replace `collision_world.rs` `pencil_cast` and the character probes with the pair test through the
   dispatcher order of `collision-kernels.md` section 4 (bins row * 11 + col). This is where the response
   006e2c10 (`docs/collision-response.md`) and the world step's unread middle (006df7c0 lines 177-397) come in:
   read those before step 5, they are census `vehicle`, not physics.
6. Components, each a small module with tests from its formulas: `wind.rs` (state, gusts, the three volumes,
   the reactor's force), joints (`Link_BallJoint` and `Link_Spring` records; the solver comes with step 5),
   `PhysicsOverride` (sleep counters over joined bodies), `ActivateCollision` (mask messages into
   `collision_masks.rs`), `GroundPositioner` (one down probe and the frame from the normal). The level data
   that uses them: the unpublished notes (ActivateCollision 1 in M-CSA, WindDirection in Shanghai,
   GroundPositioner on blackout).

## 7. Assumptions made in this pass (also in the unpublished notes)

- A-PH1 R0 uses A's displacement in its back-face test (1.1). Settles: 0117ba30's callers for a soup pair.
- A-PH2 PhysicsOverride's three unresolved fields are initial velocities and control flags (4.2). Settles: the
  readers of holder +0x1d4 bits 0x01/0x40 and wrapper +0x2c/+0x38 in the world step.
- A-PH3 Link_Spring's 0709907d is the stiffness (4.1). Settles: spring_apply 006da400's use of the record.
- A-PH4 The ray tests and the inline / Q variants share 1.3's Johnson step exactly where the token diff matches;
  the unmatched bytes (support set-up, register allocation) were looked at in 0063c3a0 and 006bb1f0 only.
  CORRECTED 2026-10-17 for the inline pair primitives and the shape ray tests: same structure, OTHER add
  orders (1.4, `gjk::INLINE`), settled by bit-exact references. Settled for the Q (soup) primitives too
  (2026-10-17): the helper ones in `PRIMITIVE`, the four inline ones in `INLINE` (1.1).
- A-PH5 (2026-10-17) The hull pair references place each hull block with no pad between the vertices and the
  face records (Hull32's block at 16 mod 32 when its vertex count is odd), as the `HullBlock` walk form
  assumes. Real blocks' alignment is unread. Settles: the hull packet's loader, or a level hull with a pad.
- A-PH6 (2026-10-17) The soup pair references' triangle records have w = 1.0 and the face's unit normal at
  +0x30. Who builds these 64-byte records is unread (the CollisionSoup block is another layout), and so is
  whether a level soup's B pose is the identity. Settles: what fills a soup pair's B data (0117ba30's callers).
- A-PH6 SETTLED 2026-10-17 for the builder: 005bc430 writes the records (w passed through, unit face normal,
  flag 0.0), bit-exact by `pair_sweep::tests::reference_soup_records`.
- A-PH7..A-PH10 (2026-10-17, step 5): the moving body is the pair's A; the avatar's masks 0x45 in both words and
  +0x9c = send; the player's root takes the floor rule; hull cache words start at 1 per node, ties in t in node
  order. Detail and what settles each: the unpublished notes (step 5 entries).
- A-PH8 / A-PH9 extended (2026-10-17) to the car's chassis cube; and a chassis contact's velocity is the point's
  velocity before the pass (the car's step 5 entry).
- A-PH11 (2026-10-17, 1.7) rcpps / rsqrtps are the exact reciprocal (square root) rounded to float, as the
  emulator computes them. Hardware estimates differ between makers; after the game's Newton step the difference is
  an ulp or two. Settles: nothing short of the user's own CPU (a recording of solver output would show it).
- A-PH12..A-PH17 (2026-10-17, 1.9, the wiring): the avatar does not turn, inverse mass 1; node +0x31 set; nodes
  persist while their contact lives; new contacts at the list's head; friction words 0; damping 0.9995. Detail and
  what settles each: the unpublished notes (the wiring entry).
- A-PH15 SETTLED 2026-10-17: 006d7280 links a new contact at the head (character-ground-test.md 2).
- A-PH26..A-PH29 (2026-10-17, 1.13): the gate is the stepping body's; equal types order by actor id; the node's A
  is the batch's A; the pair is awake and allowed, results in snapshot order. Detail in 1.13.
- A-PH24 / A-PH25 (2026-10-17, 1.12): the restitution words +0x214 of characters and the car are 0; the avatar
  body does not turn (its inertia from 006d9190 is not used: A-PH12 stands, now with the descriptor's mass).
- A-PH22 / A-PH23 (2026-10-17, 1.11): an AI's holder masks 0x0d; the car gets no friction rows and no control step.
  Detail: the unpublished notes (the enemies and car entry).
- A-PH18..A-PH21 (2026-10-17, 1.10): the probe distance in metres, the probe from the feet, +0x26c starting clear,
  the port's Stand not setting +0x3e4. Detail: the unpublished notes (the ground test entry).
- Not new: the x87 control word (collision entry).

## 8. Ledger

All 424 physics rows, plus the helpers past 006d2ed0 and the support helpers used (about 470 lines mention this
note). The older notes for single cells stay valid: 005e5030 the unpublished notes; 0066b880 /
006d02a0 (pencil x cube) the unpublished notes; the dispatcher the unpublished notes.
