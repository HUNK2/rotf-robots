# Convex-hull support helpers: Hull8 (005afcd0), Hull16 (005afbb0), Hull32 (005afa90), Hull4 (005afdf0)

Read from disassembly (`work\raw_disasm.py`). Python copy: `work\kernels\support_hull.py`
(`support(width, ...)`, `build(...)`, `pair_positions(...)`). Emulator check:
`work\emu\check_support_hull.py`. Tags: [game] read at an address, or confirmed by the
emulator (named); [assumed] not settled.

## Result in one paragraph

Each helper is a hill climb over the hull's vertex graph, started from a position remembered
in the caller. The block stores, for each face, three pairs (corner): a vertex index, and a
signed link to the corner at the same vertex in the neighbouring face. The helper keeps the
best vertex so far (position m), tests the next corner of the current face (candidate), and
when no candidate is strictly better it follows the link to the next corner around the same
vertex. It stops when the link returns to m. The result is a vertex of the hull. The walk
writes the position of the answer back into the caller's cache, so the next call starts there.
The four helpers are the same walk with different index widths (see the table).

## 1. Arguments [game]

- ECX = v: three floats, x, y, z at ECX+0, +4, +8 (read at 005afcd0-005afcd8).
- EAX = out: three floats x, y, z written at EAX+0, +4, +8 (005afdb0-005afdc0 for Hull8).
  No w is written.
- [entry+4] = arg1 = P, a POINTER to a dword that holds the block address B (005afcd0 `mov edx,[esp+4]`,
  005afce6 `mov edi,[edx]`). So B = *arg1, not arg1.
- [entry+8] = arg2 = C, a POINTER to a dword cache (005afce8 `mov edx,[esp+0x18]` after the four pushes).
  The helper reads it (005afcec) and writes it back (005afdca). It is NOT a count.
- `ret` (005afdcd) pops nothing: cdecl. The caller's cleanup is UNSURE (see section 6).
- Saved and restored: ebx, ebp, esi, edi.

Call site, caller FUN_005cea60 (the Hull8 ray test), call at 005ced57 [game]:
- Before the call (005ced17-005ced33): `mov eax,[esp+0x24]` then `add eax,0x34` pushes the address
  C = rec + 0x34, where rec is the caller's 60-byte record for this ray test (005ceb3a stores rec
  into [esp+0x24]; rec = param_3 + 60*n, n = *param_2, 005ceab4). Then `push [ebp+0x14]` pushes
  P = the caller's fourth parameter (arg1).
- The record is filled at 005ceab7-005ceac1 (only on the branch that runs when n < 0x12b,
  005cea81): rec+0x24 = -1 (not read by the helper), rec+0x34 = [query+0x18], rec+0x38 =
  [query+0x1c], query = [ebp+8]. So the cache starts as query+0x18 and persists in the record
  over the GJK loop of that ray test. Meaning of query+0x18: UNSURE.
- ECX = lea [esp+0x84] (after the pushes) = the caller's v vector (components written at
  005ced01, 005ced25, 005ced2a, 005ced4e). EAX = lea [esp+0x64] = the output (read back at
  005ced5c-005ced82 as [esp+0x6c], [esp+0x68], [esp+0x64]).

## 2. Algorithm, Hull8 (005afcd0-005afdcd) [game]

Notation: B = block, N = word 0 of the block, F = B + 16(N+1), v = (vx, vy, vz).
dot(i) = ((y*vy + z*vz) + x*vx) + w*0 for vertex i (x,y,z,w at B+16+16i).

1. Read v: 005afcd0-005afcd8. Push ebx, ebp, esi, edi: 005afce2-005afce5.
2. B = *arg1 (005afce6); N = [B] (005afcee); s = *arg2 (005afcec).
3. Start position A0 = F + 2s (005afcf0-005afcf8: edx = s + 8N + 8, then edx = B + 2*edx).
   Start vertex = byte at A0 (005afcf4: movzx ecx, byte [edi+edx*2]).
4. best = dot(start vertex) (005afcfe-005afd34). m = A0 (edx). q = A0 (esi).
5. Loop head (005afd40-005afd4b):
   c = q + 2; if (c & 7) == 0 then c = c - 6 (005afd40-005afd48). Candidate vertex = byte at c.
   Candidate is the NEXT pair of the same face record (the record wraps: the pair at +6 leads to +2).
6. d = dot(candidate vertex) (005afd4b-005afd7d; the same operations in the same order).
7. Comparison (005afd84 `comiss xmm0,xmm1`, 005afd87 `ja`): improvement only if d > best,
   strictly. Ties keep the old corner.
8. Improvement (005afdce-005afde4): best = d; q = c; if c == m stop (005afde0 `je`), else m = c and loop.
9. No improvement (005afd89-005afd9d): link t = q + 2 + 2*sbyte[q+1] (movsx at 005afd89,
   lea at 005afd8d); if (t & 7) == 0 then t = t - 6; q = t; if q == m stop (005afd99
   `cmp ecx,edx`), else loop.
10. Exit (005afd9f-005afdc6): vertex index = byte at m; out = (x, y, z) of that vertex, copied
    as floats (fld/fstp, 005afdac-005afdbd); no w.
11. Cache write (005afdc4-005afdca): *C = (m - F) >> 1 (m - B - 16N - 16, then shr 1).
    Then ret.

Constants [game]: there are no data constants (no memory operand is a constant) in Hull8.
The only numbers are the pair size 2, the record mask 7 (test cl,7), the pad step 6, the link
base 2, the link scale 2, and 0.0 from xorps (005afd2d) for w*0. The tie rule is in the `ja`
at 005afd87 (strict). Hull16/32 are the same with their own numbers (table, section 4).

What is the search: a scan of the neighbours of the current vertex (all corners at it,
found by walking the links), with the walk moving to the first strictly better neighbour and
restarting around the new vertex. It is NOT a scan of all vertices. The stop is "the links
return to m", i.e. the whole fan of the current vertex was checked without an improvement.
For a convex polytope a local maximum is the global maximum, so the answer is the farthest
vertex (checked, section 5).

## 3. Block layout the helper requires [game] unless marked

- [B+0] = N (u32). Only N is read (to find F). [B+4] (face count) is never read by this helper.
- Vertex i at B + 0x10 + 16 i: x, y, z floats at +0, +4, +8; +0xc is read as w and multiplied by 0,
  so it must be FINITE (the fill writes 1.0; the emulator used 1.0). [game] (005afd01-005afd30)
- Face records start at F = B + 16(N+1) [game], and EACH record must start at an ABSOLUTE address
  that is a multiple of 8 (Hull8): the candidate test `test cl,7` (005afd43) works on absolute
  addresses. For Hull8 this holds whenever B is 8-aligned (F = B + 16(N+1)). [game for the test; the
  fill's B alignment assumed]
- Record = 8 bytes at F + 8k: bytes +0, +1 are never read [game: the adjust steps send every
  candidate and link to +2, +4 or +6]. Pairs at +2, +4, +6, each two bytes:
  - byte 0 (even offset) = vertex index, unsigned [game, 005afd4b / 005afd6f region].
  - byte 1 (odd offset) = link, SIGNED, in units of 2 bytes from the byte after the pair:
    link target = q + 2 + 2*s, where q is the pair's first byte [game, 005afd89-005afd8d].
- The link is the corner at the same vertex in the neighbouring face across the edge
  (vertex, next vertex): this follows from the helper's walk and is what the check builds [game
  for the walk; the topology is what the check needs, confirmed by the emulator].
- Cache C: a dword whose value s satisfies A0 = F + 2s = a pair's first byte. Any other value
  reads garbage and faults. [game]. Valid s: 4k + 1, 4k + 2, 4k + 3 for face k.
- Output: *C = (m - F)/2, always a valid pair position again.

FAILED-RUN FINDING [game]: the brief's fault at 005afcf4 (about block - 0x82000000) follows from
an invalid cache value at *C (for example a pointer or a garbage dword). The helper's first
reads come from *C; the value it read was not a corner position. The block itself is not the
cause.

## 4. The four helpers compared [game, read whole; Hull8/16/32 checked in the emulator]

| | Hull4 005afdf0 (not checked) | Hull8 005afcd0 | Hull16 005afbb0 | Hull32 005afa90 |
|---|---|---|---|---|
| vertex base | B+0x10 | B+0x10 | B+0x10 | B+0x20 |
| vertex index | low nibble (and 0xf) of one byte | byte | word (movzx word) | dword |
| pair size | 1 byte | 2 bytes | 4 bytes | 8 bytes |
| link | q + 1 + sar4(byte at q) (high nibble, byte units) | q + 2 + 2*s8 at q+1 | q + 4 + 4*s16 at q+2 | q + 8 + 8*s32 at q+4 |
| record stride (pad) | 4 (1) | 8 (2) | 16 (4) | 32 (8) |
| align test / step | test cl,3 ; sub 3 | test cl,7 ; sub 6 | test cl,0xf ; sub 0xc | test cl,0x1f ; sub 0x18 |
| F | B + 16(N+1) | B + 16(N+1) | B + 16(N+1) | B + 0x20 + 16N |
| A0 | F + s (bytes) | F + 2s | F + 4s | F + 8s |
| output cache | m - F (no shift; 005afee9-005afef0) | (m - F) >> 1 | (m - F) >> 2 | (m - F) >> 3 |

Addresses: Hull8 loop 005afd40, candidate 005afd40-005afd4b, compare 005afd84-005afd87,
link 005afd89-005afd9d, exit 005afd9f-005afdcd. Hull16 loop 005afc20, link 005afc69-005afc7d,
exit 005afc7f-005afcae. Hull32 loop 005afb00, link 005afb48-005afb5b, exit 005afb5d-005afb8c.
Hull4 loop 005afe63, link 005afeaf-005afec5, exit 005afec7-005afef3.

Differences that are NOT just the index width [game]:
- Hull32: the vertex base is B+0x20 (not B+0x10), so F = B+0x20+16N. The same walk otherwise.
- Hull4: the index is a nibble and the link is the high nibble in BYTE units (not pair units),
  and the cache is not divided by the pair size. Hull4 is therefore not covered by the Python copy.
  Its loop is the same shape.
- The alignment test of Hull32 is a 32-byte test on absolute addresses: see section 6.

## 5. The check [game: emulator]

`python work/emu/check_support_hull.py` (run 2026-10-11):
- Hull8 (005afcd0): 1025 cases, 0 differ (bit for bit, the three outputs and the cache write).
  0 answers were not a farthest vertex (float64 check against all vertices). 910 answers differ
  from the start vertex (so the walk does move).
- Hull16 (005afbb0): 1025 cases, 0 differ; 0 not farthest; 896 moved.
- Hull32 (005afa90): 1025 cases, 0 differ; 0 not farthest; 909 moved.
- Cases: the cube (12 triangles) and 40 random convex hulls with 6 to 20 vertices (points on a
  sphere, facets by brute force), 25 random directions each (one zero direction in every 97),
  and a random valid start position per case. Blocks are built at the emulator's heap addresses.
- The emulator stopped nowhere (no EmuStop).
- Earlier failure, fixed: the first Hull32 run faulted at 005afb0b. The cause was alignment:
  F = B+0x20+16N is a multiple of 32 only when N is even (for 32-aligned B), so the candidate
  test (absolute, mod 32) read a vertex slot. The block builder now pads between the vertices
  and the first record (`_records_at`), with the origin of the cache still unpadded.

Not tested: Hull4; blocks with zero faces; non-convex or open data; NaN or infinite w.

## 6. Assumptions and UNSURE

- [assumed] The fill writes the link in the odd byte (Hull8), the high nibble (Hull4), the
  word (Hull16), the dword (Hull32). collision-fill-mesh.md says the second field is an edge
  index; the helper reads it as a signed relative link. See section 7.
- [assumed] The game's blocks are aligned so that the records start at a multiple of the stride.
  F = B + 16(N+1) is a multiple of 16 whenever B is, so Hull8 needs B 8-aligned and Hull16 needs B
  16-aligned, for any N. For Hull32 (stride 32) F = B + 0x20 + 16N is a multiple of 32 only when N is
  even and B is 32-aligned. The fill's Hull32 arm
  (not read) and the allocator's alignment (FUN_00533630, pool sizes: not read) decide this. If the
  game puts Hull32 records off this grid, the helper faults in the game too: the same fault as
  section 5.
- UNSURE: the meaning of query+0x18 (the caller's start value; 005ceabe-005ceac1). If it is not a
  corner position the helper faults.
- UNSURE: the caller's cleanup of the two pushed arguments. The helper returns with plain `ret`, and
  the call site (005ced57) reads the output at [esp+0x64] with both arguments still pushed, with
  no `add esp` directly after the call. Where they are removed was not read.
- UNSURE: the fill's writer for the link bytes (collision-fill-mesh.md, FUN_005bb6a0, 005bb400,
  005bb930). Not read here.
- UNSURE: whether the walk is complete for all valid hulls (the emulator checked convex hulls
  only: cube and random point sets).
- The tie rule is strict (005afd87 `ja`): on equal dots the earlier corner is kept.
- The w term: w*0 is added, so w = NaN or infinity would break the answer (game behaviour: [game]).

## 7. Against collision-fill-mesh.md (lines 54-128)

Confirmed [game]:
- Vertex part: x, y, z at +0, +4, +8 of a 16-byte vertex from +0x10, w = 1.0 at +0xc (Hull4, 8, 16).
- Face area F = block + (nVerts+1)*16 (Hull4, 8, 16).
- Stride 4 (Hull4), 8 (Hull8), 16 (Hull16) per face record, with the unused leading bytes
  (Hull4 F+4i; Hull8 F+8i and F+8i+1; Hull16 F+16i .. +3).
- Pair positions: the first index byte at F+8i+2, +4, +6 for Hull8; halfwords at +4 .. +12 for
  Hull16 (the note's "F+16i+4 .. F+16i+15"); the nibble at F+4i+1, +2, +3 for Hull4.
- The index of the first field (A) is a vertex index.

Contradicted [game]:
- The second field of each edge (B) is NOT a second vertex index. The helper reads it as a SIGNED
  RELATIVE LINK to the next corner (in units of the pair size; Hull4: the high nibble, in bytes).
  Note line 62-63 ("three edges of two indices each") and line 70-73 ("edge e writes nibble A
  ... nibble B") are wrong about B. Note's own UNSURE (d) was right to doubt it.
- Hull32 is not the same vertex part: its vertices start at B+0x20 (the note marks Hull32 unread, so
  this extends the note rather than contradicting a read claim).

Brief (this ticket) claims that were wrong [game]:
- "+8 count pointer": arg2 is a per-record start cache (rec+0x34), not a count.
- "a last-vertex cache in the shape data": the cache is in the CALLER's record, not in the block.
- "+0x10 shape data pointer": it is a pointer to the block pointer (B = *arg1).
- "faults because of the block": the fault is an invalid cache value (section 3).

UNSURE: see section 6.
