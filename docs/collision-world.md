# Static collision in the world (porter ticket, 2026-10-11)

Code: `core\src\collision_world.rs` (`StaticWorld::load`, `ray_cast`, `nearest`). Scratch
scripts: `work\survey\collision\` (`placement2.py`, `dump_prims.py`, `prim_stats.py`,
`split_stats.py`, `factors.py`, `cyl_test.py`, `bind_relative.py`, `world_big.py`, ...).
Tags: `[data]` seen in the decoded file (pack and chunk named), `[game]` from a note with an
address, `[assumed]` interpretation (reason and what would settle it).

Test (run 2026-10-11): `cargo test -p tf2-core collision_world -- --nocapture`: 5 passed, about
4 s (the downward grid is every 20 m over the mission box, 1,296 rays). Log:
`work\survey\collision\test3.log`.

## 1. The placement rule

A primitive's world matrix (row vectors, `p * M`; rows 0..2 axes, row 3 position):

    world = primitive.matrix * bind(parent node) * ACC

- `bind(node)` is ABSOLUTE in the Object's own space: NOT chained to the parent node. Node 0 is
  the object's origin; node i > 0 is `sub_matrices[i-1]`. [data] + [game]: `formats\scene.rs`
  says "rest transform in the object's own space", and that reading is right (section 2, A2).
- ACC is the composition, outermost last, of every enclosing placement:
  1. each `Instance` `matrix` on the way down (a root entry of the zone, or an Instance inside a
     `Scene`; a `Scene` is a container of Instances, e.g. `BlueZone_Building_F_prx` > `scene` >
     `BlueZone_Building_F_01` [data]);
  2. for an Object nested in another Object's attachment (`DamageLink` > `Object`, `DetachLink` >
     `Object`, [data]), the absolute bind of the attachment's node `index` in the enclosing Object.
     [assumed] that such an object is placed at its node this way: not checked against the game.
- One stored Collision is placed once per DISTINCT ACC. [data] Shanghai: 871 stored Collisions
  are placed 14,708 times in all (instancing: a building is one stored Object placed at many
  spots, e.g. `RedZone_Building_A_04` at 6 different places). The decoder's counts are per stored
  primitive; the world holds every placed copy (`WorldPrimitive` per placement, `source` string
  shared by the copies).

How sure: [data] the composition is consistent: the Python walk (`placement2.py`) and the Rust
walk (`collision_world.rs`) give the same 14,708 placed primitives and the same per-kind counts.
[assumed] that the game draws and collides with every placement (an Instance is a placed object).
To settle it: a recording of a level with a repeated building (its collision under the player).

## 2. Answers to A1 to A5

### A1: Where are the Objects? (Shanghai, `levels\shanghai_01\zone.str`)

- Root entries by type (`work\survey\collision\roots_by_type.py`) [data]: `Instance` 3,581,
  `Object` 937, `Scene` 14, `Environment` 8, one untyped.
- A root `Instance` carries `matrix` (its placement) and `things` (its Objects). Example, chunk 14
  `ConstructionZone_Building_05`: translation (494.076, -149.743, 0), `things` = `obj : Object`
  whose `sub_matrices` are the building's own parts, e.g. (-1.19, -2.42, 16), (-18.58, -5.10,
  18.59) [data]. So the Object's transforms are in the building's space, and the Instance's
  matrix places it.
- `Scene` roots hold Instances: `scene` (chunk 14) has 115 Collisions under its Instances [data].
- Coverage: every one of the 871 stored Collisions is reached through an Instance on some path
  (0 at identity) [data].
- Object roots (937) carry no matrix of their own. [assumed] they are the same Objects the
  Instances refer to (the walk reaches them through Instances as well); not checked by pointer.
  Placements reached from an Object root alone would be at identity: none in Shanghai (0).

Sample (5 of the Objects; not the 20 the ticket asked for, the rest are the same shape):
`ConstructionZone_Building_05` (chunk 14): Instance translation (494.076, -149.743, 0).
`BlueZone_Building_F_prx` (chunk 14): Instance translation (-66.190, 244.946, 0.110), nested
Scene, Instance `BlueZone_Building_F_01` with identity matrix. `RedZone_Building_A_04`: placed at
(282.5, 209.7, 0.2), (456.2, 317.0, 0.2), (460.9, 212.5, 0.2), (459.4, 226.6, 0.1), ...

### A2: Is a node's transform relative to its parent or in object space?

ABSOLUTE in the object's space. [data] `characters\bumblebee.str` chunk 0 (`bind_relative.py`):

| Skeleton | absolute z | chained z (parent * bind) |
|---|---|---|
| humanoid (58 nodes, depth 11) | -0.05 .. 5.42 | -7.60 .. 15.13 |
| car form (72 nodes, depth 7) | 0.00 .. 1.58 | -2.04 .. 5.06 |

Humanoid, absolute: Foot_L (-1.007, -0.058, 0.796); Head (0.000, 0.390, 5.258); Hand_L
(-3.104, 0.049, 5.223); Forearm_L (-2.058, -0.098, 5.223). Chained, Foot_L z = 14.1 and Head
z = 11.8: impossible for a 5 m figure. In the absolute reading a child sits 0.6 to 1.8 m from its
parent (Head vs Spine_03: 0.6 m; Calf_L vs Foot_L: 1.8 m). `[data]`.

[assumed] the same for zone Objects: their node binds are in the building's space (buildings
have z 0..18 m with parts 16 to 18.6 m up, consistent with absolute) [data], not separately
tested against a deep zone Object. To settle it: a zone Object with a skinned or deep hierarchy.

### A3: Sanity checks (Shanghai)

- Counts (Rust `shanghai_load_counts_and_bounds`) [data]: stored primitives = Cuboid 1579,
  Cylinder 141, Sphere 41, Mesh 17, Cone 3, Hull8 6 + Hull16 4 = 10 unsupported; total 1791.
  Placed: Cuboid 12,845, Cylinder 1,239, Sphere 504, Mesh 33, Cone 23, Hull16 37, Hull8 27;
  total 14,708.
- (a) Bounding box of placed centres [data]: x -688.6 .. 840.1; y -522.4 .. 689.0; z -34.20 ..
  323.57 (`prim_stats.py`). Mission bounds for `Shanghai_01_FreeRoam_A` (stream files include
  `Levels/Shanghai_01/zone`) [data, Python dump of `bnxglobal.str#5`]: min (-108, -273, -73),
  max (602, 428, 226). Placements with centre inside: at rest 2,601 of 3,001; destructible 6,938
  of 8,216; dynamics 2,703 of 3,491; total 12,242 of 14,708 (83%). The out-of-box placements are
  at the edges of the box, not a separate cluster (`split_stats.py`).
  THE RUST MISSION LIST DOES NOT GIVE THESE BOUNDS: `MissionList` reads `worldMin` / `worldMax`
  as `[0, 0, 0]` for this mission (see section 6). The Rust test skips the comparison.
- (b) z distribution [data]: a ground exists at z = 0. Centre histogram (5 m bins): -5..0 holds
  1,984 and 0..5 holds 6,571 of 14,708 placements; then falls off (2,151 at 5..10, 1,293 at 10..15).
  Ground-slab top faces are at exactly z 0 (section 2, A4).
- (c) Near-duplicates [data]: 1,339 extra copies with the same kind, params and centre (within
  0.05 m) over all placements. Of these, 41 are at rest (not destructible, not dynamics; 37 of
  them from different stored Collisions). The rest (about 1,300) are under `DamageLink` >
  `DetachLink` (two stored Collisions with the same debris cuboid at the same place; [assumed]
  the damaged and detached variants of one building). At rest, the 41 are [assumed] real double
  placements: not resolved (section 7).
- (d) Largest shapes [data, `world_big.py`, sized by WORLD extent, not stored extent]: the 15
  largest are all `StaticGround` slabs (`Concrete`, `Asphault`), 350 to 430 m across, centred at
  z -8 to 0, 0.2 to 12 m thick. So the GROUND IS IN THIS DATA (not absent). The stored extent alone
  misleads: the largest by stored number (`Asphault` 214 x 189 stored; a `Default` cuboid stored
  220 x 75 x 27 at z 323) are scaled, see section 3. The tall `obj15` cuboids near z 200 are
  scaled to a few metres in the world (axis images of length 0.19 and 0.015), not 310 m.

### A4: Cuboid extent: full or half?

HALF. [data] Three checks:
1. Ground slabs: `Concrete` (177.9 x 92.5 x 4.0), mask `StaticGround`, two copies: one centre at
   z -4.00 (top at z 0 with the half reading; z -2 with the full reading), one at z -4.21 (top at
   z -0.21 with the half reading). The first copy is the one the hand check uses (25 x 25 x 4, centre
   z -4.000, `zone.str#575`).
2. Histogram of the lowest corner of the 2,668 at-rest cuboids (`split_stats.py`): half reading
   puts 516 within 2.5 cm of z 0 (a sharp peak in the 0.00 bin); the full reading puts 1. Boxes
   rest on the ground at z 0 only under the half reading.
3. Hand check in `collision_world.rs` (`shanghai_hand_check_ground_slab_top_face`): a ray from
   z +100 at the centre of an axis-aligned slab (half z 4.0, centre z -4.0) hits at z 0.0000 with
   normal (0, 0, 1). Predicted top: centre z + half z = 0.0000.
Bumblebee's foot cuboid (extent 0.4176, 0.7400, 0.5395) is consistent with half (a foot box
1.5 m tall) and is not used as a check here.

### A5: Cylinder height12, cone heights

- Cylinder: NOT SETTLED. Lowest point of each at-rest cylinder (174), axis = local z
  (`cyl_test.py`): with height12 as the half height the most common lowest point is z 0.05 (63);
  as the full height z 0.10 (63). Neither is 0. [assumed] half (the game's helper puts caps at
  +-h with h = the stored value, [game] `docs/collision-support-2.md`; `gjk_shapes::cylinder_support`
  uses the stored value as h). To settle it: a level cylinder whose base is known (a pillar on
  the ground), or the cap positions in the game's loader.
- Cone: [game + data] base disc at z = -height14 with radius `radius`; apex at z = +height34;
  `angle` = `c`, used as stored (`gjk_shapes::cone_support`). Stored values (only 3 in Shanghai):
  radius 5.921, height14 0.458, height34 1.374, angle 0.955 (`DirtPile_Grp_A_01`). The cone's
  placement is not settled by a resting test: [assumed] as the helper says.

## 3. Scale and shear (not normalised)

`factors.py` [data], Shanghai:
- Node binds: none scaled, none sheared.
- Primitive matrices: 294 of 1,791 stored primitives have an axis length off 1 (largest
  deviation 3.154: an axis of 4.154). Shear: |cos| up to 0.295 (count not printed by the script).
- Instance chain (the ACC): 1,337 of 14,708 placements scaled (largest deviation 1.597). Shear
  none (|cos| at most 0.0013, orthogonal within float rounding).
Not normalised. `Placed` takes the full world matrix. [assumed] the inverse (`y`) is used as the
ray test's direction map (`gjk_world.rs`, `direction_to_local`: w = v * Y); for a non-uniform
scale with rotation the inverse and the transpose differ, and that choice is the game's (`[game]`
in `gjk_world.rs`), not checked here. Cuboid support depends only on the sign of w, so the
difference is at its smallest for boxes. The inverse is computed in f64 and rounded to f32.

## 4. What is unsupported

- Hulls (Hull8 6, Hull16 4 stored; placed 27 and 37): `WorldShape::Unsupported`, counted, never
  ray-tested. [game] Hull support is in `gjk_shapes::hull_support`, not wired here.
- Capsules, Pencils, Hull32, primitives with no packet: none in this zone. A degenerate
  (singular) placement would be `Unsupported { "degenerate placement" }`: none found.
- Mesh and hull vertices: world bounding box x -47.7..601.7, y -297.1..360.4, z -25.6..85.9
  (2,071 vertices, from the placed matrices).
- Dynamics: 3,491 placements are flagged `has_dynamics` (a `DynamicsData`) [data]: included.

## 5. The `released` flag (a change to the ticket's struct)

8,216 placements (7,094 cuboids) sit under `DamageLink` or `DetachLink` of their Object [data].
These are damage variants and debris, released when the parent breaks. The ticket's struct has no
field for it, so `WorldPrimitive` has `released: bool` as an addition. [assumed] they are not
solid at rest; the game's release rule is not read. A consumer must skip `released` for at-rest
collision.

## 6. The mission list bounds (formats\missionlist.rs: NOT FIXED)

`MissionList::load` gives `world_min` = `world_max` = `[0, 0, 0]` for `Shanghai_01_FreeRoam_A`.
The Python dump of `bnxglobal.str#5` (`tf2_lxb.py dump`) gives `worldMin: [-108, -273, -73]`,
`worldMax: [602, 428, 226]`. `vec3()` reads `ints(node)` = `items().filter_map(Node::int)`; the
cause is not found in this ticket (the values are negative, and the list is a plain list). Fix
needed in `formats\missionlist.rs` (or `lxb.rs` if `Node::int` drops negative values there). Not
edited (not my file). The world-bounds test in `collision_world.rs` skips when both are zero.

## 7. UNSURE / OPEN

- Multiplicity: every placement is taken to be real geometry. [assumed] Settle with a recording
  (a repeated building under the player).
- At-rest double placements: 41 extra copies (37 from different stored Collisions). Not resolved:
  a building and its proxy (`_prx`) may both be placed. [assumed] both real.
- Destructible pieces: `released` is a flag only. Their release rule is not read.
- Inverse vs transpose for scaled placements (section 3): the choice is the game's, not checked.
- Cylinder height12: half or full, NOT settled (section 2, A5). Cone placement untested by a resting test.
- Mission bounds: missionlist reads zeros (section 6).
- Object nesting: a child Object placed at its parent's node with the absolute bind (section 1)
  is [assumed]: the game's loader is not read for nested objects.
- The walk: `StaticWorld::load` takes EVERY placement (not the decoder's first occurrence). The
  decoder's `collisions()` keeps the first occurrence of each stored Collision for its counts
  (`collision.rs`); the stored counts here match it exactly, the placed counts are separate.
- Chunks: zone data chunks only (`DATA`); the mission pack (`mission.str`) is not loaded here.

## 8. Brief items that were wrong or needed care

- The brief expects the world's primitives to equal the decoder's counts (1579 etc.). They
  equal the STORED counts (per distinct source). The placed world has 14,708 primitives.
- "Chained" node transforms (the brief's wording "node it rides") is the wrong reading: binds
  are absolute (A2).
- The "largest shapes" check by stored extent is misleading (scale in the matrices); by world
  extent the ground slabs are the largest (section 2, A3d).
- Mission bounds from `MissionList` are wrong for this mission (section 6).
- Hull counts: stored 10 unsupported, as the brief says (placed 64).

## 9. Functions needed from other files

None to finish this ticket. For the bounds: `formats\missionlist.rs` `vec3()` / `ints()` (or the
`Node::int` behaviour for negative list items in `formats\lxb.rs`) must return the stored values.
