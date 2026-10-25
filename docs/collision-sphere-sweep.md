# The sphere sweep (the sphere tests), ported and checked against the original

Porter's ticket, 2026-10-11. Background: `collision-tests-emulated.md` (the 22 ray and sphere
tests), `collision-support-swept.md` (the swept helpers, `work\kernels\support_swept.py`).

Files: `work\emu\gen_refs_sphereshapes.py` (the generator), `work\emu\ref\sphere_shape_<shape>.tsv`
(sphere, cube, capsule, cylinder, cone: 1000 cases each, seeded 404 + table index), `core\src\gjk_swept.rs`
(the port and its tests), `core\src\collision_world.rs` (`StaticWorld::sphere_cast`, the sweep helpers,
two Shanghai tests). Run: `cargo test ... -p tf2-core --release -- gjk_swept collision_world --nocapture`,
15 tests, all pass.

## What the game does

[game, by emulator run] Table 00b8dd38 entries used: sphere 005d1be0, cube 005d35a0, capsule 005d4f70,
cylinder 005d6930, cone 005d8300 (the generator asserts these against the table). Argument 4 is the
two-word record {&radius of the moving sphere, shape data}. The shape data read: sphere +0; cube +0, +4,
+8; cylinder +0, +4; cone +0, +4, +0xc (a probe with all-ones fill; the ray test also reads +8, so all four
are passed); capsule +0..+0x20 as the ray test. The support is the shape's support plus r * v / |v|
(005e2dd0), so the GJK search runs against the shape grown by r. Hit lambda and verdict follow from that.

[game] The reported point is on the target's surface, about r from the centre, not at the centre.
It is NOT `centre - r * normal` (see "Points" below).

## Numbers (final run, 1000 cases per shape)

| shape | hit/miss | contact flag | ordinary hits | lambda within 2 mm | largest \|dlambda\| | mean / largest normal angle, ordinary | hit point mean / largest, ordinary | start-in-contact rows | largest point distance, contact | mean / largest normal angle, contact |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| sphere | 1000/1000 | 849/849 | 599 | 100.00% | 8.6e-6 | 0.00 / 0.03 deg | 0.061 / 0.264 m | 250 | 2.957 m | 3.6 / 180 deg |
| cube | 1000/1000 | 612/612 | 432 | 100.00% | 9.8e-5 | 0.00 / 0.02 deg | 0.083 / 0.300 m | 180 | 4.609 m | 68.3 / 158 deg |
| cylinder | 1000/1000 | 697/697 | 481 | 99.58% | 6.4e-3 | 0.02 / 3.05 deg | 0.093 / 0.371 m | 216 | 3.730 m | 47.5 / 180 deg |
| cone | 1000/1000 | 537/537 | 372 | 99.73% | 9.7e-3 | 0.02 / 5.72 deg | 0.098 / 0.311 m | 165 | 4.085 m | 55.6 / 180 deg |
| capsule | 1000/1000 | 543/543 | 381 | 100.00% | 1.8e-5 | 0.00 / 0.03 deg | 0.098 / 0.285 m | 162 | 4.193 m | 64.1 / 180 deg |

(Ordinary = neither side reports a start in contact. "Contact flag" counts all agreed hits.)

Hand case (identity cube h = (1.3, 0.7, 2.1), sphere 0.5, sweep down from z = 5): lambda 2.399603
(exact 2.4), hit on the top face z = 2.1003, normal (0, 0, 1). The emulator's own sanity run gave the same
lambda and a point on the top face.

## Port

- `sphere_point` (005e2dd0): n2 = (vx*vx + vy*vy) + vz*vz; zero when n2 <= 1e-6 (00b34610); else
  k = 1/sqrt(n2) (sqrt as float, then 53-bit divide, stored as float); out = v * (r * k). [game]
- `Swept<S>`: support = shape support + sphere_point (out_sum = shape + buf1). [game]
- `sphere_cast`: `gjk::ray_cast` on `Swept`, then point = centre - r * normal. [assumed] the point rule.
  Lambda and the verdict need nothing else.
- The zero-direction case: the sphere term is zero for |v|^2 <= 1e-6 (the game's helper); the shape's own
  zero-direction behaviour (cube: 0 counts as positive; the others as their supports) is unchanged. No
  separate fix was needed: the verdict agrees in every reference case, and lambda agrees within 2 mm in
  every ordinary case except 1 cone and 2 cylinder rows (largest 9.7e-3).

## Points and normals: what is not reproduced

- Ordinary hits: the game's point is at distance r from the centre (median +0.7 to +1.3 cm), but it is
  2.4 to 3.5 degrees off the centre-to-point direction the game's own normal implies. `centre - r * normal`
  therefore differs by a median 4.6 to 9.3 cm. The GJK normal itself matches to 0.00 to 0.02 degrees for
  the sphere, cube and capsule; for cone and cylinder the mean is 0.02 degrees, the largest 5.7 and 3.1.
- [assumed] The game's point is the GJK witness point on the target (a barycentre of the simplex's
  support points), which needs the simplex and Johnson's step. `gjk::johnson` is private and not read in
  the original, so it is not ported. Settled by: reading the sphere test's output writer (after
  005e2dd0's callers) and the simplex's final step.
- Tried and rejected: `centre - r * (last search direction)` is worse (sphere median 0.25 m; cube 0.14 m).
- Start-in-contact sweeps: the game's point is not `start - r * normal` (for the sphere and cube its distance
  from the start is a median 0.45 r and 0.37 r; for the sphere it lies about 180 degrees from the normal). The normals there
  disagree too (table above). Both come from the contact branch (step 13, the walk-back), which is the
  ray port's unverified part. [assumed] the same witness and walk-back rules as above; not settled.

## World (`collision_world.rs`)

- `StaticWorld::sphere_cast(p0, p1, radius)`: the ray grid, with the segment's box and the grid query
  widened by RAY_TOL + radius. A box of a shape meets the swept sphere's path only if it meets the
  segment box widened by the radius, so no hit is cut away. [assumed] this bound; checked only by the
  two tests below.
- Cuboid, Sphere, Cylinder, Cone: `gjk_swept::sphere_cast`. Mesh: each triangle of the soup, placed by the
  same transform as `collision_soup.rs` (copied, both loop orders), swept as a triangle grown by the radius
  [assumed]: the game's sphere-against-soup driver is not read (the sphere test's soup entry, 005bd320, is
  not run). The face normal is oriented by the soup driver's rule.
- Shanghai, sphere r = 1 from (300, 300, 50) to (300, 300, -10): one hit, Cuboid primitive 4405, point
  z = 7.6e-6, normal (0, 2.6e-6, 1), lambda 48.99999, centre z = 1.0000076. Pass (within 5 cm). The game's
  own answer at this spot is not known; it is only the sweep's own consistency.
- 200 random segments (seed 0x5eed_0002) with radius 0.001 against `ray_cast`: 200 of 200 hit the same
  primitive sets. The grid version and the brute loop were not compared for the sweep.

## Assumptions (all `[assumed]` in the code too)

1. Point rule `centre - r * normal` (see "Points"). Not the game's point: ordinary median 4.6 to 9.3 cm,
   contact up to 4.6 m. Settled by the emulated witness, not yet read.
2. Start-in-contact points and normals not reproduced (mean normal angle 3.6 to 68 degrees). Same cause.
3. Mesh sweep per triangle (no game driver read).
4. Grid widening RAY_TOL + radius bounds the hit error at any radius; tested at radius 1 (Shanghai) and
   0.001 (200 segments) only.
5. Cone and cylinder packet: the sphere test reads +0, +4, +0xc (cone), the ray test also +8; the
   references pass all four, as the ray test does. Settled by a probe with distinct values.
6. Radii 0.1 to 3.0 and the shape sizes of the references are generator choices, not game data.
7. Precision: the x87 control word 0x027f (`emu.py`); the 53-bit divide in sphere_point depends on it.

## Known gaps for the next porter

- The sphere test's output writer (the tail of 005d1be0 and the cube/cylinder/cone/capsule versions) is
  the place to read the point rule.
- Contact branch (ray port step 13) is the shared source of the contact normal error; the ray references
  count contact rows but do not compare their normals (`gjk.rs` tests).
