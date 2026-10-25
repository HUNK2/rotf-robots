# The 22 ray and sphere tests, probed in the emulator (manager, 2026-10-11)

Script: `work\emu\ray_shapes.py` (`probe`: what each test reads of its arguments and
what it returns; `calls`: which functions it calls). Everything here is [game] BY
EXECUTION of the original's code offline (`work\emu\emu.py`), not by reading, unless
tagged otherwise. Background: `collision-core.md` (the routine), `collision-support.md`.

## Result: one routine, one support helper per shape

All 22 table entries (ray tests at 00b8dd0c, sphere tests at 00b8dd38) take the same
arguments and write the same 60-byte record. Each is a copy of the GJK ray cast with a
different support helper called (the cube's is inlined). The sphere tests add the
query sphere's support 005e2dd0 and a second per-shape helper.

| type | shape | ray test | support called by the ray test | sphere test | extra helpers in the sphere test | shape data read |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | Soup | 005bcb30 (driver; per triangle 005bed70, support 005e3710) | | 005bd320 | not run yet | +0 = block pointer |
| 1 | Sphere | 005c1ed0 | 005e3570 | 005d1be0 | 005e21a0, 005e3570 | +0 |
| 2 | Cube | 005c3670 | inlined | 005d35a0 | 005e2250 | +0, +4, +8 |
| 3 | Capsule | 005c5020 | 005e3400 | 005d4f70 | 005e2410, 005e32a0, 005e3400 | +0, +4, +8, +0x10, +0x14, +0x18, +0x20 |
| 4 | Cylinder | 005c6a50 | 005b0180 | 005d6930 | 005e24c0, 005e3140, 005b0180 | +0, +4 |
| 5 | Cone | 005c8480 | 005b0060 | 005d8300 | 005e2570, 005e2fe0, 005b0060 | +0, +4, +0xc |
| 6 | Pencil | 005c9eb0 | 005aff10 (and 00409a50) | 005d9cd0 | 005e2620, 005e2e80, 005aff10 | +0, +4, +0xc |
| 7 | Hull32 | 005cb8e0 | 005afa90 | 005db6a0 | 005e26d0 | +0 = pointer (not run further) |
| 8 | Hull16 | 005cd1a0 | 005afbb0 | 005dd030 | 005e2890 | +0 = pointer |
| 9 | Hull8 | 005cea60 | 005afcd0 | 005de9c0 | 005e2a50 | +0 = pointer |
| 10 | Hull4 | 005d0320 | 005afdf0 | 005e0350 | 005e2c10 | +0 = pointer |

"Shape data" is the shape object's packet (the object keeps the packet pointer at +4
and the tests read the packet's fields in place: `collision-fill-simple.md`), so the
offsets are packet offsets: Sphere radius; Cube three half extents; Cylinder radius,
height; Cone and Pencil +0, +4, +0xc. [assumed] which float is which beyond the
packet's field order; settled by running each with distinct values.

## Arguments (all 22)

cdecl, EIGHT stack arguments ([game] read at the dispatcher 005be9e6-005bea62, `add
esp, 0x20`; `collision-ray-soup.md` says seven, which is what the soup driver reads):

1. query record: P0 at +0, P1 at +0xc, two ints at +0x18 / +0x1c (copied to the hit
   record's +0x34 / +0x38); +0x20 and +0x24 are also read by the ray tests.
2. pointer to the hit count (incremented on a hit: the verdict).
3. the result table (00ddd618 in the game), 60-byte records.
4. ray tests: the shape's data pointer (vtable slot 2 of the shape). Sphere tests: a
   two-word record {pointer to the query radius, the shape's data pointer}.
5. and 7. X: the same pointer twice, a 3x4 transform, 12 floats read (+0..+0x2c); its
   last three (+0x24) are the origin that seeds the search direction.
6. and 8. Y: the same pointer twice, read as a 3x3 (+0..+0x20). [game] X is the
   holder's current affine (holder +0x108, copied by 005e3890) and Y its inverse
   (holder +0x138, copied by 005e3820 at 005be9a2; `driving.md` "Holder108..134").
   Five and six are passed twice because the routine is the two-object form with the
   ray as a degenerate second object [assumed].

## What ran (identity transforms, every shape float = 1.0, ray (0.25, 0.125, 5) to (0.25, 0.125, -5))

| test | count | point | normal | lambda | exact answer |
| --- | --- | --- | --- | --- | --- |
| ray, Sphere r = 1 | 1 | (0.25, 0.125, 0.96064) | (0.22318, 0.11159, 0.96837) | 4.039358 | lambda 4.039857, normal (0.25, 0.125, 0.96014) |
| ray, Cube | 1 | (0.25, 0.125, 1) | (0, 0, 1) | 4.0 | 4.0 |
| ray, Cylinder | 1 | (0.25, 0.125, 1) | (0, 0, 1) | 4.0 | |
| ray, Cone | 1 | (0.25, 0.125, -1) | (0, 0, 1) | 6.0 | |
| ray, Pencil | 1 | (0.25, 0.125, 1) | (0, 0, 1) | 4.0 | |
| ray, Capsule | 0 | | | | (a miss is right if +0..+8 and +0x10..+0x18 are end points) |
| sphere r = 0.5, Sphere | 1 | (0.16603, 0.08302, 0.97194) | (0.22319, 0.11159, 0.96837) | 3.523025 | 3.52626 |
| sphere r = 0.5, Cube | 1 | (0.26585, 0.10771, 1) | (0.00997, -0.01424, 0.99985) | 3.476425 | 3.5 |
| sphere, Capsule / Cylinder / Cone / Pencil | 1 each | | | 3.030525 / 3.500539 / 5.498812 / 3.500539 | |

What this says about the game, for the port:
- lambda stops short of the exact contact by up to a few centimetres (the 0.001
  squared-distance bound): 0.5 mm on the unit sphere, 2.4 cm for the swept sphere on
  the cube's face.
- THE NORMAL IS APPROXIMATE: on the unit sphere it is 3 degrees off the true normal,
  and for a sphere swept onto a flat face it is not exactly the face normal. It is the
  last search direction. Anything downstream (sliding, camera collision) sees that.
- The sphere test's hit point lies on the TARGET's surface, not at the sphere's centre.
- A cone's hit at z = -1 with normal +z for a ray coming from +z is odd and unexplained
  ([assumed] the fill of 1.0 gives a degenerate cone); to be rerun with real packet
  values.

## The 11 x 11 pair matrix (00b8db28), first probe (`work\emu\pair_probe.py <row> <col>`)

[game] A cell is called by 005e39a0 as cell(shape A data, shape B data, input record of
0xd0 bytes, output record of 0x108 bytes) and returns a byte: 1 = the output record
was written. [game, by execution] All 36 cells of rows and columns 1 to 6 (Sphere, Cube,
Capsule, Cylinder, Cone, Pencil) run whole on a guessed input (2,200 to 14,700
instructions each); row 0 (Soup) returns 0 at once on a dummy block; rows 7 to 10
(hulls) stop in the hull support for want of a valid block and cache.

Sphere against Sphere, 005e5030, both radii 1.0:
- It is a SWEPT test (time of impact between two moving shapes).
- Input bytes read: +0x24..+0x5c and +0x84..+0xc8. With four 3x4 matrices laid at +0,
  +0x30, +0x60, +0x90 that is: the ORIGIN of the first and third, the whole of the
  second and fourth, and three floats at +0xc0. [assumed] A's start pose, A's end pose,
  B's start pose, B's end pose (only the start origins are used: the rotation comes from
  the end pose), then a vector.
- Run: A at the origin throughout; B's origin from (0, 0, 3) to (0, 0, -3). Output:
  +0xf8 = 0.166667 = (3 - 2) / 6, the fraction of the step at which the spheres touch;
  the output's third matrix has origin (0, 0, 2) (B's pose at contact) and its fourth
  (0, 0, -2) (the inverse); +0xc0 (0, 0, 1), +0xcc (0, 0, -1), +0xd8 (0, 0, 1), +0xe4
  (0, 0, -1) ([assumed] contact points on each shape and normals); +0xf0 = -1 (int);
  +0xf4 = 1.0; +0xfc..+0x104 = 0.
- It calls 005e3570 (the sphere support, x4), inverse_affine3 005301d0 (x4), 00572110
  (x4), 00616ed0, 006cbe80 (x2) and the square root. 00616ed0 and 006cbe80 are unread.
- NOT established: the input record's real layout (who builds it: 006e1bc0), what
  +0xc0 of the input is, every output field's meaning, and how this relates to the ray
  cast routine (same GJK core or another).

## Not run yet

- Soup tests (need a mesh block: `collision-soup.md`) and the four hull tests in both
  tables (need the hull data layout: `collision-fill-mesh.md` when written).
- The 11 x 11 pair matrix at 00b8db28.
- Real packet values from a level for each shape, and non-identity transforms.
