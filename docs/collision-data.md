# Collision data in a pack: what the decoder reads (porter ticket, 2026-10-11)

Code: `core\src\formats\collision.rs`. Tags: `[data]` seen in the decoded file (counts from the
user's install); `[game]` from `docs/collision-loader.md`; `[assumed]` interpretation.
Scratch scripts: `work\survey\collision\`.

## Where it lives

- Not in chunk 0 of a level's zone pack. `levels\us_west_town\zone.str` chunk 0 has 8 Objects and
  no Collision [data]. Collision sits in several DATA chunks of the same pack: us_west_town
  chunks 14, 458, 524, 872, 993, 999, 1022; shanghai_01 chunks 14, 457, 527, 555, 575, 643, 715,
  1203 [data]. `collision.rs::load` walks every DATA chunk (`pack.of_type(pack::DATA)`).
- Path: data chunk -> root entry or a value inside it -> an Object (`attachments`, `hierarchy`,
  `sub_matrices`) -> `attachments[i].thing` is a `Collision` (`thing.type_name() == "Collision"`)
  -> `primitives` (list of `CollisionBatchPrimitive`) -> each `packet` is a typed
  `Collision*Packet` [data]. No element-level type (`CollisionMesh`, `CollisionCuboid`...) occurs
  in any of these packs; the element path in `collision-loader.md` 1a is not used by the level data [data].
- Object shape: the same three fields (`sub_matrices`, `hierarchy`, `attachments`) on every
  Object [data]. Root entries also carry the `Object` type name in Rust's reader, but
  `work\collision_dump.py` does not tag root values, so it misses them (6 in Bumblebee, 11 in
  us_west_town chunk 14, 618 in chunk 872) [data].

## Collision (fields)

- `parent`: index into the Object's `hierarchy.names` (the node the collision rides) [data].
- `dynamics`: a `DynamicsData` when present, else null [data].
- `primitives`: the list of `CollisionBatchPrimitive` [data]. `links` is not decoded.
- `object_path`, `object_typed`, `parent_node`, `parent_name`, `has_dynamics`: the decoder's own.

## CollisionBatchPrimitive (fields)

- `matrix`: 4 rows of 3 floats [data]. Rows 0 to 2 are the axes, row 3 the translation: a mesh
  with identity axes and a translation of (-32.27, 88.13, -9.99) in row 3 [data]. So points are
  row vectors (`p * M`), as `CLAUDE.md` says for quaternions.
- `material`, `maskPreset`: CRC-32 name hashes stored as integers; the file's name table holds
  their text: `Default` is stored as text `0 - Default`, `UpperBody`, `LowerBody`, `Head`,
  `Character`, `Attack`, `KillBox`, `DebrisSmall`, `DynamicObj`, `Metal_Hard`, `Concrete` [data].
- `sendMask`, `receiveMask`: integers (values seen: 0, 1, 2, 37, 128, 133, 175) [data]. Bit meanings not decoded.
- `packet`: the typed shape.

## Packet types, one real example each (user's install)

| packet | fields as stored | example |
|---|---|---|
| CollisionCuboidPacket | `extent`: 4 floats, the 4th 0 [data] | Bumblebee `Foot_L`: (0.41762, 0.74003, 0.53948, 0), parent 20 |
| CollisionSpherePacket | `radius` [data] | Bumblebee `Hand_L`: radius 0.93208, parent 34 |
| CollisionCylinderPacket | `radius`, `height12` [data] | shanghai_01 chunk 14: radius 1.55019, height12 5.61860 |
| CollisionConePacket | `radius`, `height14`, `height34`, `angle` [data] | shanghai_01 chunk 715: 5.92142, 0.45794, 1.37383, 0.95533 |
| CollisionMeshPacket | `nVerts`, `nFaces`, `verts` (x,y,z,w), `faces` (3 u32 words) [data] | shanghai_01 chunk 575: 13 verts, 13 faces |
| CollisionHull8Packet | `nVerts`, `nFaces`, `verts`, `faces` (struct: `pad`, `edges` of (`index`, `offset`)) [data] | shanghai_01 chunk 715: 5 verts, 6 faces |
| CollisionHull16Packet | as Hull8 [data] | shanghai_01 chunk 14: 22 verts, 40 faces |
| CollisionCapsulePacket | none in these packs [data] | |
| CollisionHull32Packet, CollisionHull4Packet, CollisionPencilPacket | none in these packs [data] | |

Mesh face words: 0x80000002 and similar: the top 5 bits are set on 114 faces of the 17 Shanghai
meshes; no index is at or past the vertex count (low 27 bits) [data].

## Counts (by object identity, Python, first visit)

- `levels\shanghai_01\zone.str`: 871 Collisions, 1791 primitives: Cuboid 1579, Cylinder 141,
  Sphere 41, Mesh 17, Hull8 6, Hull16 4, Cone 3. This is exactly the survey's Z-SH line
, so the walk reaches the same set the survey counted [data].
- `levels\us_west_town\zone.str`: 624 Collisions, 1777 primitives: Cuboid 1644, Cylinder 89,
  Sphere 13, Hull8 14, Hull16 12, Cone 5 [data]. The survey's Z-US line (2,452 primitives) does
  NOT match this zone; see UNSURE.
- `characters\bumblebee.str` chunk 0: 38 Collisions, 41 primitives: Cuboid 31, Sphere 8,
  Cylinder 2 [data]. collision_dump.py prints 32 of them.

Whether level geometry collision is covered: every CollisionBatchPrimitive the walk sees (1777,
1791) is inside a Collision's `primitives` list [data]. Whether the level's large static
collision (the world, not props) is in these attachments: the counts match the survey exactly for
Shanghai, so the survey's whole-level count is these attachments. Nothing else was found.

## Dedupe: NOT exact yet

`lxb::Node` has no storage offset, so the walk cannot tell one stored Object from the same object
reached by another path. The walk reaches shared objects many times (Bumblebee: 248 paths to 38
Collisions; Shanghai: about 22,000 paths to 871). `collisions()` keeps the first occurrence of each
key (object name, parent, primitive content). The name is not stable: a pointer-reached object
has its header name, an inline one has none. Result: Bumblebee 56 (identity 38), Shanghai 813
(identity 871). The count tests are `#[ignore]`d with the identity numbers. Fix: add
`pub fn offset(self) -> Option<usize>` to `Node` in `lxb.rs` (the `at` of a settled value), and key
a `HashSet<usize>` on it. Not done here: that file is not mine.

## UNSURE / NOT DECODED

- Dedupe (above). Undercount on Shanghai, overcount on Bumblebee.
- `links` of Collision: not read.
- `sendMask`/`receiveMask` bits; `material`/`maskPreset` meanings beyond their text.
- Hull `pad`, edge `index` and `offset` meanings: kept as stored [assumed: unread].
- Mesh face flags (top 5 bits): their meaning is not decoded.
- Cylinder, Cone, Capsule fields are all float fields of the packet by name; no conversion.
- us_west_town zone: 1777 primitives against the survey's 2,452 for Z-US. Not resolved; the
  survey may count another zone or several packs of the level.
- `matrix` row/column layout: checked on one mesh (identity axes, translation in row 3) only.
- Bumblebee's 6 root Objects: typed Objects by Rust's reader; not checked against the game's code.
