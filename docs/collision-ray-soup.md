# lux::CollisionSoup ray driver FUN_005bcb30 (reader pass, 2026-10-11)

Tags: [game] instruction address (asm read), [dec] decompiler only, [data] file, [assumed] reason and what settles it, [stand-in] code that is not the game's.

Scope: 005bcb30 (2014 bytes, `0x5bcb30`..`0x5bd31e`) read in disassembly end to end. Its triangle test is NOT in this function: it is the callee FUN_005bed70 (6055 bytes), which is only partly read (section 6). The transliteration `work\kernels\ray_soup.py` therefore covers the driver, with the callee as an injected stand-in.

## 1. Arguments and calling convention

- cdecl: seven stack arguments, `ret` with no immediate ([game] 005bd31e). No register input is read. The function reads `[ebp+8]`, `[ebp+0xc]`, `[ebp+0x10]`, `[ebp+0x14]`, `[ebp+0x20]` only; `[ebp+0x18]` and `[ebp+0x1c]` are never referenced [game, whole body].
- Dispatch site: world query 005be890, through the table PTR_FUN_00b8dd0c[type] at 005bea4d (`mov edx,[eax*4+0xb8dd0c]`), type from slot 4 (`mov eax,[esp+0x54]`). Direct callers: none (the call is through the table, so `fn.py callers` is empty) [game].
- The query object is fetched first by vtable slot 2 (`mov edx,[esi]; mov edx,[edx+8]; mov ecx,esi; call edx`, 005bea31..005bea3c) and pushed at 005bea3e.

| arg | ebp offset | value | evidence |
|---|---|---|---|
| param_1 | +8 | ray record, `float*`: P0 = [0..2], P1 = [3..5], payload [6..9] | pushed from `lea ecx,[esp+0x84]` (005bea54..005bea5b); read by the callee at 005bed..; [8],[9] copied in its epilogue [dec] |
| param_2 | +0xc | `int*` result count (hits so far; the callee increments it) | pushed from `lea eax,[esp+0x24]` (005bea3f..005bea48); `iVar24 = param_3 + *param_2*0x3c` at 005bd257..005bd263 |
| param_3 | +0x10 | result table base | pushed as the constant `0xddd618` (005bea43) = DAT_00ddd618 |
| param_4 | +0x14 | `int**`: the soup's query object (this+0x40); `*param_4` = block pointer | slot 2 result (005bea3c); `mov eax,[ebp+0x14]; mov ecx,[eax]` (005bcb3c..005bcb3f) |
| param_5, param_6 | +0x18, +0x1c | unused by this function | no reference in 005bcb30..005bd31e [game] |
| param_7 | +0x20 | `float*` 3x4 transform (12 floats), in esi | `mov esi,[ebp+0x20]` (005bcb43); pushed earlier in 005be890, not traced [assumed: the object's transform] |

Block layout used (from 005bcb3c..005bcb49 and 005bd044..005bd070; matches collision-soup.md):
- word 0 = A (vec4 count), word 1 = B (triangle count); the loop over triangles runs B times (`test edx,edx; je` at 005bd04f..005bd05d).
- vec4 vertices at block+0x10, A of them, 16 bytes each.
- triangles at block+0x10+16*A (`lea eax,[edi+1]; shl eax,4; add eax,ecx` at 005bd044..005bd04a, so start = 16*(A+1)), B of them, 12 bytes each: three uint32 words. [game]

## 2. Transform (005bcb55..005bcee5, then 005bcee3..005bd040)

The matrix (param_7) is a column-major 3x4 affine: p[0..2] image of x, p[3..5] image of y, p[6..8] image of z, p[9..11] origin. w passes through unchanged (`mov [ecx+4], w`, 005bcc9c..005bcca1 region). Output vertices (x', y', z', w) are written at esp+0xa0+16*i (the local array `local_810`).

Two loops, two summation orders (both [game], checked in asm):

4-wide unrolled loop, vertices 0 .. 4*(A//4)-1, entered when A >= 4 (`cmp edi,4; jl`, 005bcb4b..005bcb55):
- out0 = ((p0*x + p3*y) + p6*z) + p9*w   (005bcc06..005bcc98: xmm0 = p0*x; += p3*y; += p6*z; += p9*w)
- out1 = ((p1*x + p4*y) + p7*z) + p10*w  (005bcc5a..005bcc7e)
- out2 = ((p2*x + p5*y) + p8*z) + p11*w  (005bcc84..005bccb9)

Remainder loop, the vertices after that, entered at 005bcf7e (for A < 4 it is the whole array):
- out0 = ((p3*y + p6*z) + p9*w) + p0*x   (005bcf7e..005bcfbe; p0*x is added last, at 005bcfb4..005bcfba)
- out1 = ((p4*y + p7*z) + p10*w) + p1*x  (decompile agrees; asm 005bcfd1..005bd01a)
- out2 = ((p5*y + p8*z) + p11*w) + p2*x  (asm 005bcfda..005bd01a, decompile agrees)

FINDING: the two loops round differently. In a random test (section 5) out0 of the remainder order and the unrolled order differ in the last bit for 4400 of 10091 vertices. A bit-exact port must use both orders, chosen by index (i < 4*(A//4) unrolled, else remainder).

Vertex buffer: local array at esp+0xa0, 16 bytes per vertex, inside the 0x894-byte frame (005bcb36). About 127 vertices fit before the frame top [assumed: from the offsets; the loader's vertex limit is not read].

## 3. Triangle loop (005bd070..005bd312)

For triangle t (0-based, `[esp+0x14]`), words (w0, w1, w2) at triangle start + 12t:

1. Index: `i = w & 0x7ffffff` (005bd073, 005bd0ad, 005bd0e8): the low 27 bits; the top 5 bits are flags and are ignored here. Vertex address = esp+0xa0 + 16*i (`shl eax,4`). [game]
2. Vertices a, b, c = transformed (x,y,z,w). Edges [game 005bd12a..005bd179]:
   e1 = b - a, e2 = c - a.
3. Normal, unnormalised [game 005bd185..005bd1b5]:
   Nx = e1y*e2z - e1z*e2y
   Ny = e1z*e2x - e2z*e1x
   Nz = e2y*e1x - e1y*e2x
   (= cross(e1, e2) in right-handed order.)
4. Length [game 005bd1a8..005bd1cc]: len2 = ((Nz*Nz + Ny*Ny) + Nx*Nx); ln = float32(sqrt(len2)) via 00409a70 (`fld; call _CIsqrt; fstp float32`, 00409a70..00409a8c) [game].
5. Normalise only if ln > 1e-8 [game 005bd1ed..005bd220, constant 00b345d8 = 1e-8f]: inv = 1.0f / ln (005bd222, constant 00b38834 = 1.0f); N = (inv*Nx, inv*Ny, inv*Nz) (005bd233..005bd24e). Otherwise N is the raw cross product (length 2*area, which is zero for a degenerate triangle).
6. Call the callee (section 6) with ECX = param_2, EDX = matrix, stack args (ray, count*, table, &a) (005bd254..005bd273). Note `&a` is `esp+0x60` = the start of the triangle's three vec4s (a at +0x00, b at +0x10, c at +0x20 from the frame's view: a.x 0x60, b.x 0x70, c.x 0x80), and the normal is at &a-0x18 (0x48, 0x4c, 0x50). [game 005bd266 lea edx,[esp+0x60]; 005bd1d0..005bd21a stores]
7. The entry read is the one at index count BEFORE the call (005bd257..005bd263: entry = param_3 + count*0x3c). [game]
8. Hit test: skip unless entry+0x28 (t) >= 0.0 (`comiss xmm1,xmm0; jb`, 005bd278..005bd286). NaN is skipped (jb). [game]
9. On a hit (005bd288..005bd2fd):
   - entry+0x24 = triangle index t [game 005bd298].
   - dot = ((E.z*N.z + E.y*N.y) + N.x*E.x), with E = entry+0x18..0x20 (the callee's vector) [game 005bd29b..005bd2c3].
   - if dot > 0: entry+0x18..0x20 = N [game 005bd2c8..005bd2d2]; else = -N computed as (-0.0 - N) [game 005bd2d9..005bd2fd].
   So the stored normal faces the same way as the callee's vector E, whatever E is. [game]
10. Loop: next triangle, B times (005bd302..005bd312).

The driver never selects the nearest hit, never rejects back faces and never checks A or the table capacity. All selection is in the caller (005be890) or the callee.

## 4. Result record (stride 0x3c = 60 bytes = 15 words; base DAT_00ddd618 from 005be890)

| offset | field | written by | meaning |
|---|---|---|---|
| +0x00..+0x14 | not read | callee hit path (not read) | probably the contact point [assumed] |
| +0x18, +0x1c, +0x20 | normal (x,y,z) | callee writes a vector (GJK direction, [dec] pfVar3[6..8]); driver overwrites with the oriented unit normal on a hit | unit normal of the triangle (see step 5) when the hit is kept; a zero or raw normal in the degenerate case |
| +0x24 | triangle index (int) | callee writes -1 (005bedcf); driver writes the triangle index t on a hit | index of the soup triangle [game] |
| +0x28 | t (float) | callee writes -1 at start (005bede7) and a value on a hit (hit branch not read) | hit if >= 0; units not confirmed [assumed] |
| +0x2c, +0x30 | ray[8], ray[9] | callee epilogue [dec] | ray payload pass-through |
| +0x34, +0x38 | ray[6], ray[7] | callee prologue (005bedd7..005bede1) [game] | ray payload pass-through |

count: the callee adds 1 to *param_2 at the end of its normal path [dec `*param_4 = *param_4 + 1`]. Its early exits (count > 298, or |P1-P0|^2 < 1e-4) do not add 1 (005bed80 `cmp ecx,0x12b; jl`, the early `return 0`).

So a query with B triangles that all take the normal path leaves count = B (the callee does not skip misses, only degenerate rays and full tables).

## 5. Test (work\kernels\test_ray_soup.py, output checked)

`ray_soup.py` runs the driver with the STAND-IN triangle test `stub_tri_test` (Moller-Trumbore in float64, writes the same fields, direction E = P1-P0). The reference is float64 Moller-Trumbore on the world-space vertices (matrix applied in float64) with orientation normal flipped so dot(N, P1-P0) > 0. All answers below are the same for the hit tests of the stub and the reference; they test the driver, not FUN_005bed70.

| case | expected | got | |
|---|---|---|---|
| 1 middle, from above (tri z=0, ray z 1 to -1 at (0.5,0.5)) | tri 0, t 0.5, N (0,0,-1) | same | PASS |
| 2 through the edge v=0 at (1,0) | tri 0, t 0.5, N (0,0,-1) (inclusive edge) | same | PASS |
| 3 parallel, above the plane | no hit | no hit (det = 0) | PASS |
| 4 from behind (ray from z=-1 up) | tri 0, t 0.5, N (0,0,1) | same | PASS |
| 5 stops short (ends at z=0.5) | no hit | no hit | PASS |
| 6 two triangles at z=0 and z=-0.5 | tri 0 t 0.5; tri 1 t 0.75; both N (0,0,-1) | same | PASS |
| 7 translated (origin x=10), triangle uses vertex 4 of 5 (remainder path) | tri 0, t 0.5, N (0,0,-1) | same | PASS |
| 8 degenerate (collinear) triangle | no hit; normal left raw (zero) | no hit | PASS |

Transform: max error vs float64 = 2.5e-6 over 10091 random vertices (all three components; the test label says x-column, which is wrong). out0 remainder order differs from unrolled order in 4400 of 10091.

Process note: the first run failed case 7. The cause was the TEST's reference, which ignored the matrix. It was fixed in the reference (test side), not in the driver. No mismatch against the driver is open.

## 6. The callee FUN_005bed70 (NOT transliterated; partly read)

What is read [game]: prologue 005bed70..005bee70 (asm); count read from the stack arg (`mov ecx,[esp+8]; mov ecx,[ecx]`); ray at [esp+4] (the driver's param_1); table at ebp = [esp+0xc]; triangle pointer at [esp+0x10] (= &a, used at 005befd6); matrix in EDX. Early exit if |P1-P0|^2 < 1e-4 (005bee49, constant 00b6a404, writes -1 at +0x28). Entry +0x24 = -1 and +0x28 = -1 at start. The GJK direction starts from matrix origin (EDX+0x24, +0x28, +0x2c) minus P0, or -(P1-P0) if that is tiny (decompile). The triangle support call is FUN_005e3710 with ECX = &a (005befd6..005beff7). The main loop runs at most ~299 steps with a reset at 15 (`cmp esi,0xf`, 005bf04e). The epilogue writes +0x18..+0x20 (the direction), +0x2c, +0x30, +0x34, +0x38, then `*param_4 += 1` [dec].

What is NOT read: the body of the GJK loop, the simplex cases (uVar8 = 1..4), FUN_005e3710, FUN_005502f0, FUN_005571b0, the hit branch that writes t and the contact point, and the meaning of t's units. The sign test on the triangle normal (a plane test) and the inside test are therefore not confirmed here. The stand-in uses Moller-Trumbore; it is not the game's rule.

## Constants

| address | value | used at |
|---|---|---|
| 00b345d8 | 1e-8f (0x322bcc77) | 005bd1ed, normalise threshold (driver) |
| 00b38834 | 1.0f | 005bd222, inv = 1/ln (driver) |
| 00b6a3ac | -0.0f (0x80000000) | 005bd2d9, flip (driver) |
| 00b2d970 | -1.0f | callee 005bed93, 005bedb8 |
| 00b6a404 | 1e-4f | callee 005bee49, 005bf083 |
| 0x7ffffff | index mask | 005bd073, 005bd0ad, 005bd0e8 |
| 0x3c | entry stride (60 bytes) | 005bcb..., 005bd25b..005bd263 |
| 299 (0x12b) | callee capacity (count < 299) | 005bed80 |

## Assumptions

1. The soup is only the triangle list at block+0x10+16*A: confirmed by the driver's reads. The loader's limit on A is not read; the frame allows about 127 vertices [assumed; settled by the loader 005b8fa0 or a soup with more vertices].
2. param_7 is the object's transform (pushed earlier in 005be890) [assumed; settled by reading 005be890 around 005be9d3 for its pushes].
3. t's units (fraction of the segment or distance) [assumed; settled by reading the hit branch of 005bed70].
4. The sqrt: float32 result of _CIsqrt in extended precision; np.sqrt on float32 is used (same in all but rare double-rounding cases) [assumed].
5. The unit normal's orientation rule (keep if dot > 0 with the callee's vector E) is the driver's rule [game]; what E means (the GJK direction, not the triangle's plane) is [assumed] until 005bed70 is read.
6. Back faces are not rejected by the driver [game]; whether 005bed70 rejects them is unknown.

## Not read / unsure

- 005bed70 body from 005bee70 to 005bfb0b (only prologue, the early exits and the epilogue; the triangle/segment test itself is here, not in 005bcb30).
- FUN_005e3710 (support), FUN_005502f0, FUN_005571b0.
- 005be890 pushes for params 5 to 7 and the matrix source.
- Vertex buffer capacity against the frame (about 127 vertices, from the offsets only).
- The callee's writes to +0x00..+0x14 (contact point?).
- The sqrt wrapper's rounding path (00409a70 calls `_CIsqrt` at 0xa8b5c6, not read).

Ledger: 005bcb30 tagged `game` (whole body read in asm). 005bed70 tagged `unread` (partial). See functions.tsv.
