# Hulls placed in the world (Hull8, Hull16)

Porter note, 2026-10-11. Code: `core\src\collision_world.rs` (`hull_block`, `HullLocal`, `hull_box`,
`WorldShape::Hull`, the tests named below). Emulator references: `work\emu\gen_refs_rayhull.py`,
`work\emu\ref\ray_shape_hull8.tsv`, `ray_shape_hull16.tsv`. Support helper: `docs/collision-support-hull.md`.
Tags: `[data]` seen in the decoded file, `[game]` by emulator run or a read note, `[assumed]` not settled.

## 1. How the faces map to the walk's pairs [data], checked

A `CollisionHull8Packet` in `levels\shanghai_01\zone.str` chunk 715 (the dump showed a 5-vertex and an
8-vertex hull there; `DirtPile_A_01/things[0]` is the 36-vertex hull of section 2) stores `verts` (x, y, z, w) and `faces`. Each face has `pad: [0, 0]`
and three `edges`, each `(index, offset)`.

- The pad is 2 bytes: a Hull8 record is 8 bytes, 2 bytes of pad and three 2-byte pairs. Consistent.
- `pairs[3k + j]` = `(index, offset)` of the j-th edge of face k: `index` is the corner's vertex,
  `offset` is the signed link, in PAIR units from the end of the pair, to the corner at the same vertex
  in the neighbouring face (the walk's `q + 1 + s` in units; `wrap` off the unused slot).
- Checked in `hull_block`: every link must land on a corner at the same vertex, after the wrap. All 10
  stored hulls build, so every link does. For DirtPile, also: all 68 faces are wound the same way and
  every link target's previous vertex is the source's next vertex (0 mismatches).
- [assumed] `offset` is in pair units, not bytes. The checks above are what settle it, and they pass.

## 2. Real hulls: the support property

Test `hull_support_is_farthest_on_real_hulls`: 10 stored hulls, 26 directions, every corner as the
start: 24,804 calls.

- 9 of 10 pass. Every direction and start gives the largest dot product.
- FAILS: `#715/DirtPile_A_01/things[0]` (36 vertices, 68 faces, convex: plane violation 2e-16, one fan
  per vertex). 4 calls fail, all for direction +z from start unit 223. The walk returns a vertex at
  z = -2.75; the true top is at z = +0.89 (gap 3.6).
- Cause (diagnosed): a vertex of the bottom facet (vertex 27) lies in the middle of that flat facet.
  Its four neighbours (25, 26, 29, 32) are all on the plane z = -2.75, so no neighbour is strictly
  higher. The walk moves only on a strict improvement (`ja` at 005afd87), so it stays on the plateau.
  The hull is not a true polytope at that vertex (a vertex inside a face), so the local-maximum
  argument does not apply.
- [assumed] The original walk gives the same answer on this block: the port matches it bit for bit on
  1025 synthetic cases per width (`check_support_hull.py`), and the same rule is in the code. NOT run in
  the emulator on this block. Settles it: run 005afcd0 (Hull8) or 005afbb0 (Hull16) on DirtPile's block
  with v = (0, 0, 1) and the cache at unit 223.
- Consequence [assumed]: in directions where a facet like this is the extreme, support and ray results
  near DirtPile may be wrong. The test accepts only this hull as a failure (`PLATEAU_HULLS`); any other
  failure fails the test.

## 3. Emulator references (the hull ray tests, 005cea60 Hull8, 005cd1a0 Hull16)

`gen_refs_rayhull.py`: 500 cases per width, every ray test run in the emulator with no stops.
Hulls: a box (half 1.5, 1.0, 0.75), a roof wedge (6 corners), and 8 random convex hulls (6 to 14
corners), each centred on its own origin. Placement and rays as `gen_refs_rayshapes.py`. The cache
(query +0x18) is a random valid corner unit; the sanity check (box, ray down z) gives z = 0.75.

- Hull8: 500 / 500 hit/miss agree; contact flags 294 / 294; ordinary hits (213) within 2 mm in lambda
  (largest 3.8e-6), 3 mm in point, 0.02 in normal: all 213.
- Hull16: 499 / 500 agree. Row 229 (`random7`, 13 corners): the emulator misses and the port hits at
  lambda 3.07. The ray passes 2.2 cm outside the hull (measured in the hull's frame), inside the GJK's
  distance tolerance (about 3 cm, `EPS_DIST2` 0.001). [assumed] a rounding difference on a grazing ray
  decides it. Not run to the end.
- Hull16 ordinary hits (209): within 2 mm in lambda (largest 4.3e-6), point, normal: all 209.

## 4. The world

- `hull_block` builds the block from the decoded packet; `WorldShape::Hull(Placed<HullLocal>)` is
  placed through `Placed` and `hull_support`; `cast_one` and `sweep_one` ray-test and sphere-sweep it,
  like the convex shapes.
- `HullLocal` keeps the walk's start unit in an `AtomicU32`, carried between support calls within a
  test (as the game carries rec+0x34), and reset to `HULL_CACHE_START` (unit 1) before each test.
  [assumed] the game seeds it from query +0x18 (meaning not read); the reset keeps grid and brute-force
  results bit-identical.
- Box: the hull's vertices carried by the placement (`hull_box`), widened as for the other shapes.
  [assumed] the sums are those of `gjk_world::point_to_world`; the hull's own transform is not read.
- Hull32 stays `Unsupported` (no level data, no reference). Failed links also stay `Unsupported`.
- Shanghai: 64 hull placements, all now `Hull`; `Unsupported` placements 0 (was 64). All 64 are hit by a
  ray down at the centre of their box, from just above to just below.
- Stored hull counts are unchanged (6 Hull8, 4 Hull16), so `shanghai_load_counts_and_bounds` needed no
  change. The `kind_name` helper gained an arm for `WorldShape::Hull`.
- The game crate only pattern-matches `WorldShape::Cuboid` with `let ... else` (`game\src\world.rs:401`),
  so the new variant does not affect it.

## 5. Tests (`core\src\collision_world.rs`, all pass: `collision_world` filter, 12 tests)

- `hull_support_is_farthest_on_real_hulls`: builds all 10 stored hulls; 9 pass; DirtPile is the one known
  failure (above).
- `hull8_ray_matches_emulator_refs`, `hull16_ray_matches_emulator_refs`: the references above; asserted
  >= 99% hit/miss (Hull8 100%, Hull16 99.8%).
- `shanghai_hull_placements_are_placed_and_hit`: 64 placed, 0 unsupported, 64 / 64 box-centre hits.

## 6. Assumptions to add to the unpublished notes (not editable from this ticket)

1. Hull `offset` is a link in pair units (section 1). Settled by the link check, which passes.
2. The original walk gives the plateau answer on DirtPile (section 2). Settles: emulator run of the
   support helper on that block.
3. Hull16 row 229 is a grazing-ray rounding difference (section 3). Settles: run the original ray
   test to its last step on that row and compare the decision steps.
4. The world's hull ray tests start from unit 1 (section 4). Settles: the query +0x18 meaning in the
   game's ray test caller.
5. Hull box uses the placement sums of `point_to_world` (section 4). Settles: the game's hull bounding
   box, if one is read.
6. Hull32 stays unsupported (section 4). Settles: a level with a Hull32 and a reference.
