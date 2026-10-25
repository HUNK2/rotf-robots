# Collision support helpers 2: cylinder, cone, pencil, capsule

Read in full from disassembly (`work\raw_disasm.py`). Transliteration: `work\kernels\support2.py`.
Check: `work\emu\check_support2.py` (runs the original bytes in the offline emulator, `work\emu\emu.py`).
Model for this note: the unpublished notes (sphere and triangle).

| helper | address | size | ray test (call site) | check result |
|---|---|---|---|---|
| cylinder_support | 005b0180 | 005b0180-005b025e (ret at 005b025e) | 005c6a50 (call 005c6e8c) | 2000 cases, 0 differ |
| cone_support | 005b0060 | ret at 005b0158 and 005b0173 | 005c8480 (call 005c88bc) | 2000 cases, 0 differ |
| pencil_support | 005aff10 | ret at 005b0024 and 005b0057 | 005c9eb0 (call 005ca2ec) | 2000 cases, 0 differ |
| capsule_support | 005e3400 | ret at 005e356c | 005c5020 (call 005c545c) | 2000 cases, 0 differ |

Branch hits in the check (all branches reached): cylinder rim 1858, axis 142; cone tip 493, rim 1423,
degenerate 84; pencil tip 887, rim 1073, degenerate 40; capsule endpoint 1 964, endpoint 2 1036,
degenerate 59.

## Shared facts

- 00409a70 (the sqrt wrapper) is `fld [arg]; call _CIsqrt; fstp dword [tmp]; fld [tmp]`: the result is
  rounded to float32 and returned in ST0. [game] Its body is the one already noted.
- 00409a50 (read now, tiny) is `fld [arg]; fabs; fstp dword; fld`: returns |arg| in ST0 (exact). [game]
- The threshold constant at 00b34610 is 1e-6 (float32 0x358637bd). [game]
- The constant -0 at 00b6a3ac (0x80000000) is used as `-0 - x` to negate a float. [game]
- Compare convention: `comiss a, b; jb` takes the branch when a < b. Every branch below is stated
  with its exact comparison.
- x87 precision: the emulator runs with control word 0x027f, 53-bit mantissa. [assumed] that the game
  runs the same. In the rim paths `t = round24( round53( round53(1/s) * r ) )`: the 1/s is not stored
  as a float before the multiply. If the game ran at 24-bit (0x007f) the last bit of t could differ.
  Not tested.
- Stack argument: each helper takes one stack argument at [E+4] (E = entry esp). Cone, pencil and
  capsule use that slot as scratch (they store intermediate floats there after the first load of the
  pointer). The ray tests push a fresh argument for every call, so this does not affect the callers.
  Cylinder does not touch it.
- Degenerate test: all four compare the squared 2D or 3D length with 1e-6, so |v| below about 1e-3 is
  degenerate.

## Call-site arguments (the same in all four ray tests)

Let S = esp after the `push eax` in the ray test (the stack argument is pushed as `eax = [ebp+0x14]`,
the shape data pointer).
- v = the ray test's frame slots S+0x78 (x), S+0x7c (y), S+0x80 (z); the stores are at 005c6df8 /
  005c6e2b / 005c6e83 (cylinder), 005c88a3-005c8828 / 005c885b / 005c88b3 (cone), 005ca258 / 005ca28b /
  005ca2e3 (pencil), and 005c53cb / 005c53fe / 005c5453 (capsule). [game]
- Cylinder 005c6e8c: `esi = S+0x78` (= v), `edi = S+0x54` (output), stack arg = [ebp+0x14] (P). [game]
- Cone 005c88bc: `edi = S+0x78` (= v), `esi = S+0x54` (output), stack arg = [ebp+0x14] (P). [game]
- Pencil 005ca2ec: `esi = S+0x78` (= v), `edi = S+0x54` (output), stack arg = [ebp+0x14] (P). [game]
- Capsule 005c545c: `edi = S+0x78` (= v), `esi = [ebp+0x14]` (shape B, loaded at 005c53a6), stack
  arg = `S+0x50`, the output pointer (`lea eax,[esp+0x50]` at 005c5434). [game]

## 1. cylinder_support, FUN_005b0180

Arguments: ESI = v (x at +0, y at +4, z at +8). Stack [E+4] = P, pointer to r (+0) and h (+4).
EDI = output (3 floats written; eax = edi returned). [game]

Arithmetic, in order:
1. L = x*x + y*y (float32). Instructions 005b0188-005b01a5. [game]
2. If 1e-6 < L (comiss at 005b01a9, jb 005b01fe): rim path.
   Else axis path.
3. Rim path: s = float(sqrt(L)) (005b0206), t = float(round53(1/s) * r) with r = [P+0]
   (005b020b-005b021b). Then out.x = x*t, out.y = y*t (005b0241, 005b0245). [game]
4. Axis path: out.x = out.y = +0 (005b01db, xorps at 005b01b9). [game]
5. out.z = +h if z >= 0 (comiss 0, z at 005b01bc / 005b021b, jbe), else 0 - h (005b01c2 / 005b0221). [game]

Meaning [assumed]: the cylinder axis is z. r = P+0 is the radius, h = P+4 the half height. The support
point is the rim point in the direction of (x,y), at height +h or -h depending on the sign of z. When
(x,y) is (near) zero, the point is the centre of the cap. The direction is in the shape's frame.
The z sign only sets the cap; the cylinder has no tip.

Check: 2000 cases, 0 differ.

UNSURE: r and h are read from the code's use (r scales the rim, h is the cap height). Their names are
[assumed]. The caller's frame (is v in the cylinder's own local frame?) was not checked.

## 2. cone_support, FUN_005b0060

Arguments: EDI = v (x +0, y +4, z +8). Stack [E+4] = P: r (+0), h (+4), a (+8), c (+0xc).
ESI = output (3 floats; eax = esi). [game]

Arithmetic, in order:
1. Q = (y*y + z*z) + x*x (float32). Instructions 005b0061-005b0091. [game]
2. nv = float(sqrt(Q)) (005b00a0, the sqrt of |v| in 3D).
3. A = nv * c (005b00a5, exact in x87; c = [P+0xc]). [game]
4. If A < z (fcompi at 005b00b0, jb 005b0159): tip. out = (+0, +0, +a) with a = [P+8] (005b0159-005b0173). [game]
5. Else L2 = y*y + x*x (float32, 005b00c7-005b00cb). If 1e-6 < L2 (comiss 005b00d1, jb 005b0103):
   rho = float(sqrt(L2)) (005b010b); t = float(round53(1/rho) * r) (005b0110-005b011f);
   out = (x*t, y*t, -h) (005b012b-005b0151). [game]
6. Else (degenerate): out = (r, +0, -h) (005b00dc-005b00f9). [game]

Meaning [assumed]: the cone's apex is at (0,0,a) and its base disc is at z = -h with radius r. The
apex is the support when A >= z, i.e. when z > c * |v|. c = P+0xc is the cosine of the cone's half angle
measured from the z axis. [assumed] c = r / sqrt(r^2 + (a+h)^2) makes the tip test exact against the
rim (derived below). With that c the tip/rim cut is the exact support; the code does not check it.
Derivation: apex value z*a, rim value r*rho - h*z. The apex wins when z*(a+h) >= r*rho, i.e. when the
angle from z satisfies tan = (a+h)/r. In that case cos = c = r/sqrt(r^2+(a+h)^2).

Check: 2000 cases, 0 differ.

UNSURE: c = P+0xc is the cosine, only as [assumed] from the tip test and the geometry above; the
caller's records were not read. Whether the game's cone is exactly a base at -h with apex +a was not
checked against the cone's shape object (the collision cone note is `docs/collision-shapes.md`).

## 3. pencil_support, FUN_005aff10

Arguments: ESI = v (x +0, y +4, z +8). Stack [E+4] = P: r (+0), h (+4), a (+8), c (+0xc).
EDI = output (3 floats; eax = edi). [game]

Arithmetic, in order:
1. Q = (y*y + z*z) + x*x (float32, 005aff13-005aff79). nv = float(sqrt(Q)) (005aff7e). [game]
2. A = nv * c (005aff83, exact). |z| (005aff51, fabs, exact). [game]
3. If A < |z| (005aff8d-005aff93, fcompi, jb 005b0025): tip. out = (+0, +0, +a) if z >= 0,
   else (+0, +0, 0 - a). Tip at 005b002f-005b0054. [game]
4. Else L2 = y*y + x*x (float32, 005affa5-005affa8). If 1e-6 < L2 (comiss at 005affa8, jbe 005affd9 is
   the other way): rho = float(sqrt(L2)) (005affbf), t = float(round53(1/rho) * r) (005affc4-005affcd).
   Else (degenerate): t = +0 (005affd9).
5. out.x = x*t, out.y = y*t (005affdc-005afffa; x*0 keeps the sign of x). out.z = +h if z >= 0 (005affdf,
   jbe 005afff5), else 0 - h (005affe6-005affee). [game]

Meaning [assumed]: the pencil is a cylinder-like base disc of radius r at z = +h (or -h for z < 0), with a
tip on the axis at +a (or -a). The tip is the support when |z| > c*|v| (the same cut as the cone, with
the cap at h, so c = r / sqrt(r^2 + (a-h)^2) makes the cut exact [assumed]). The pencil is symmetric
about z = 0.

Check: 2000 cases, 0 differ. The degenerate branch gives -0.0 when x or y is negative (x*0), and the
emulator matches that bit for bit.

UNSURE: c = P+0xc, as for the cone, is [assumed]. The brief said the pencil reads +0, +4, +0xc: it also
reads +8 (the tip, 005b0037-005b003e). The sign of r and h is not made positive (a negative r reflects
the rim, the same as the sphere helper).

## 4. capsule_support, FUN_005e3400

Arguments: EDI = v (x +0, y +4, z +8). ESI = shape B (9 floats: B0..B8 at +0..+0x20; B3 and B7 are
not read). Stack [E+4] = output pointer, written at +0, +4, +8 (eax = ebx returned). [game]

Arithmetic, in order:
1. Q3 = (x*x + y*y) + z*z (float32, 005e3411-005e342a). [game]
2. If 1e-6 < Q3 (comiss 005e342e, jbe 005e346e otherwise): nv = float(sqrt(Q3)) (005e3450),
   u = float(round53(1/nv)) (005e3455-005e3462). Else u = +0 (005e346e). [game]
3. W = B8 * u (005e3480-005e3485, float32). [game]
4. X = x*W, Y = y*W, Z = z*W (float32, 005e34a6-005e34b4). [game]
5. S1 = ((B2*z + B1*y) + B0*x) (005e34b8-005e34d5); S2 = ((B6*z + B5*y) + B4*x) (005e34d9-005e34f8). [game]
6. comiss S1, S2 (005e34fc): if S1 > S2 (fallthrough), out = (B0+X, B1+Y, B2+Z) (005e3505-005e3525,
   005e354c-005e3556). Else (S1 <= S2, jbe 005e352d), out = (X+B4, B5+Y, B6+Z) (005e352d-005e354c). [game]

Meaning [assumed]: a capsule is the segment between endpoints E1 = (B0,B1,B2) and E2 = (B4,B5,B6) with
radius B8. The support point is the endpoint with the larger dot product with v (S1 against S2), plus
the radius times the unit direction of v (W = B8/|v|). B3 and B7 are padding (the fourth lane of each
vec4) [assumed]. The degenerate case returns the chosen endpoint (u = 0).

Check: 2000 cases, 0 differ. B8 was random positive (0.05 to 2.0), as the brief asked for shape values.
No matrices enter this helper, so the brief's "random rotations" did not apply.

UNSURE: the endpoint layout (B0..B2 and B4..B6 as the two ends) and the radius at B8 are [assumed] from
the arithmetic; the caller's capsule record was not read. The sign of B8 is not made positive. The
output pointer is written through ebx, which was loaded from the stack argument (the argument slot is
then used as scratch).

## Corrections to the brief

- Pencil and cone also read +0x8 (the tip height a): cone at 005b0159, pencil at 005b0037-005b003e.
- "Random rotations for the matrices": none of these four helpers takes a matrix. Shape data is
  P (cylinder, cone, pencil) and the 9-float B (capsule).
- 00409a50 is fabs, not a sqrt helper.
- The ray tests' v is at S+0x78 (x), S+0x7c (y), S+0x80 (z) for all four; the brief's "+0x18 X" and
  "+0x1c Y" were not needed for these helpers.

## Assumptions to settle

- [assumed] x87 precision 53-bit (emu.py, control word 0x027f). Settled by: the game's own control word
  (run it, or read the Direct3D device creation flags).
- [assumed] c = P+0xc is the cosine of the half angle for cone and pencil. Settled by: a cone or pencil
  record from the game with a known tip and rim, and the ray test's own tip result.
- [assumed] capsule endpoints at B0..B2 and B4..B6, radius B8. Settled by: the capsule's collision
  record and the function that writes it (not read).
- [assumed] cylinder axis z, radius P+0, half height P+4. Settled by: the same.
- [assumed] v is in the shape's local frame. Settled by: the ray test's transform of v (not read here).
