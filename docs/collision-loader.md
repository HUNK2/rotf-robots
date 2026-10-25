# Collision loader: data to shape objects (ticket, reader pass, 2026-10-11)

Tags: `[game]` address read in the disassembly or decompile; `[data]` pack, chunk or survey count; `[assumed]` interpretation, with the reason. Partial work: the fill functions that copy packet fields are NOT read (see "Not read"). Scripts used (scratch only): CRC-32 name checks, a jump-table decode of 005b92... (switch 005b9250), find_ptr, find_call, raw_disasm.

## 0. Shape of the code

The loader is not one Ghidra function. `FUN_005b9250` (Ghidra) holds only a top switch (005b9250-005b95a4). The rest is code in no function: the Collision body fields (005b972d-005b9b1f), a per-element dispatch (005b9d70-005ba263), and the batch and packet branches. `work\find_call.py` reports the dispatch's calls as "in FUN_005b9250" [game].

Two dispatch layers:
- **Element layer** (outer): the type-name hash of the list element (`ebx`), compared at 005b9d95 to 005b9dc8 and 005ba20a to 005ba220.
- **Packet layer** (inner): for a `CollisionBatchPrimitive` element (hash b11f4a4b, branch 005b9e27), the `packet` field (hash 3c41a474, read at 005b9e36 through FUN_00573940, then FUN_00573bd0) gives a child whose type hash is compared at 005b9e6a to 005ba1f4.

## 1. Packet and element type -> handler -> shape class -> constructor

### 1a. Element layer (names without the `Packet` suffix)

| element type | hash | branch | handler | class | vtable store |
|---|---|---|---|---|---|
| CollisionMesh | b7683ef2 | 005ba1f6 | FUN_005b8fa0 (soup builder; reads faces and vertices from the element) | lux::CollisionSoup | 00b5cdec at 005b9118 (in FUN_005b8fa0 +0x178) |
| CollisionSphere | 98ff8d74 | 005b9e0c | FUN_005b88a0 | lux::CollisionSphere | 00b5cdc4 at 005b893f |
| CollisionCylinder | 934d1f62 | 005b9dd4 | FUN_005b89f0 | lux::CollisionCylinder | 00b5ce64 at 005b8aeb |
| CollisionCuboid | efd29e21 | 005ba222 | FUN_005b8740 | lux::CollisionCube | 00b5ce3c at 005b87f6 |
| CollisionCapsule | e61104ce | 005ba23a | FUN_005b8da0 | lux::CollisionCapsule | 00b5ce14 at 005b8ee1 |
| CollisionCone | dbfddcb6 | 005ba24e | FUN_005b8ba0 | lux::CollisionCone | 00b5ce8c at 005b8ce1 |
| CollisionHull | 56a7a737 | 005b9deb | FUN_005b9250 (loader top: third field picks Hull4/8/16/32) | see 1c | see 1c |
| CollisionBatchPrimitive | b11f4a4b | 005b9e27 | packet layer (1b) | by packet | by packet |

[game] hashes by CRC-32 of the name (checked on known names). The element type `CollisionCuboid` (efd29e21) is used for the Cube; the name `CollisionCube` (66f9b51c) is not in the dispatch.

### 1b. Packet layer (`CollisionXPacket`, from the `packet` field of a batch)

| packet | hash | branch | vtable stored inline | class | fill call after the store | note |
|---|---|---|---|---|---|---|
| CollisionMeshPacket | 432528ce | 005b9ef4 | 00b5cdec at 005b9f17 | lux::CollisionSoup | FUN_005bb110 (005b9f2e) | pool block of 0xd units; +0x40 = data, +0x44 = 0, +0x48 = 0 |
| CollisionConePacket | 4eabd7d9 | 005b9e9a | 00b5ce8c at 005b9ebd | lux::CollisionCone | FUN_005baf80 (005b9ed4) | |
| CollisionCylinderPacket | 0a4a8909 | 005b9f4e | 00b5ce64 at 005b9f71 | lux::CollisionCylinder | FUN_005baea0 (005b9f88) | |
| CollisionHull32Packet | 65fd93d4 | 005b9fa8 | 00b5ceb4 at 005b9fcb | lux::CollisionHull32 | FUN_005bb3a0 (005b9fe2) | |
| CollisionHull16Packet | d397b0ba | 005ba145 | 00b5cedc at 005ba168 | lux::CollisionHull16 | FUN_005bb640 (005ba17f) | |
| CollisionHull8Packet | c9739dd8 | 005ba0d9 | 00b5cf04 at 005ba0fc | lux::CollisionHull8 | FUN_005bb8d0 (005ba113) | |
| CollisionSpherePacket | cf2d750f | 005ba19f | 00b5cdc4 at 005ba1c2 | lux::CollisionSphere | FUN_005badc0 (005ba1d9) | |
| CollisionCapsulePacket | a045e4ae | 005ba07f | 00b5ce14 at 005ba0a2 | lux::CollisionCapsule | FUN_005bb060 (005ba0b9) | no level data uses it |
| CollisionCuboidPacket | b7c8082f | 005ba025 | 00b5ce3c at 005ba048 | lux::CollisionCube | FUN_005bace0 (005ba05f): wrap only, see 3 | |
| CollisionHull4Packet | 0ed3f7ac | none | none | none | none | no compare against this hash in the packet chain: nothing is built [game] |
| CollisionPencilPacket | ad5fae15 | none | none | none | none | no compare seen: nothing built [game] |

Each inline branch: pool allocation (FUN_005343a0 with one unit, or 0xd for the soup), then the vtable store, then `+4` = the packet's data pointer (see 2), `+8` = 0, `+0xc` = 0. For the soup the same three words are at +0x40, +0x44, +0x48 [game].

### 1c. Hull4/8/16/32 from the element `CollisionHull` (switch in FUN_005b9250)

- The switch value is `view_scalar` (FUN_00573c20) of the field hash 1464c25c. Cases (jump table decoded from the executable at 005b95ac and 005b95c0): value 4 -> 005b94f5 (Hull4), 8 -> 005b943d (Hull8), 0x10 -> 005b9388 (Hull16), 0x20 -> 005b92d3 (Hull32); every other value -> 005b95a4 (return) [game].
- Vtable stores are in four small functions called from the switch after a successful pool allocation: Hull4 00b5cf2c in FUN_005bc170 (+2), Hull8 00b5cf04 in FUN_005bc1c0 (+2), Hull16 00b5cedc in FUN_005bc200 (+2), Hull32 00b5ceb4 in FUN_005bc240 (+2) [game, find_ptr].
- The Hull8, Hull16 and Hull32 packet branches also store their vtables inline (1b), so the hull class is decided by the PACKET type in that path.

### 1d. Confirmed or refuted

- **CollisionMeshPacket = Soup: confirmed** for the packet path (1b, 005b9ef4). The soup builder FUN_005b8fa0 is the element path `CollisionMesh` (1a), which reads faces and vertices from the element itself; the packet path does not call it.
- **CollisionCuboidPacket = Cube: confirmed** (1b, b7c8082f -> 00b5ce3c).
- **What decides Hull4/8/16/32:** for a `CollisionHull` element, the scalar in field 1464c25c (not a vertex count: nothing in the switch counts vertices; the name suffix matches the case values only). For packets, the packet type: Hull8, Hull16, Hull32 each have their own branch, Hull4 has none. [assumed] that the field 1464c25c is a shape size or kind, not a count: name not found (see Not read).

## 2. Fields the loader reads for the level packets

Level packet types with survey counts [data] (Z-US / Z-SH / M-CSA): Cuboid 2,045 / 1,579 / 84; Sphere 36 / 41 / 21; Cylinder 291 / 141 / 1; Cone 0 / 3 / 1; Mesh 69 / 17 / 0; Hull8 9 / 6 / 0; Hull16 2 / 4 / 0. Capsule and Pencil: none.

| packet | field names [data, survey] | how the loader stores the packet | stored at |
|---|---|---|---|
| CollisionCuboidPacket | 4 floats (last 0) | pointer only: `+4` = data pointer; no copy; wrap only | Cube `+4` |
| CollisionSpherePacket | radius | pointer only (fill FUN_005badc0 not read) | Sphere `+4` |
| CollisionCylinderPacket | radius, height12 | pointer only (fill FUN_005baea0 not read) | Cylinder `+4` |
| CollisionConePacket | radius, height14, height34, angle | pointer only (fill FUN_005baf80 not read) | Cone `+4` |
| CollisionMeshPacket | nVerts, nFaces, verts (x,y,z,w), faces | pointer; soup block built by FUN_005bb110 (not read) | Soup `+0x40` |
| CollisionHull8Packet | nVerts, nFaces, verts, faces | pointer; fill FUN_005bb8d0 (not read) | Hull8 `+4` |
| CollisionHull16Packet | nVerts, nFaces, verts, faces | pointer; fill FUN_005bb640 (not read) | Hull16 `+4` |

The `+4` pointer: the store computes `ecx = edx + [edx]` when both the word and the base are non-zero, else 0 [game, 005b9ebd-005b9ec9]; so `+4` is the address of the packet block, reached through a relative word [assumed: the packet view's offset word; see Assumptions]. No float is converted at this store. A conversion (half extent, inverted transform, scale) can only happen in the fill functions, none of which is read. `decomp_lookup.py fields` returns nothing for them (they are not in the export).

Field hashes the loader itself reads (faces 0xc8dd2f8b, vertices 0x4112bc6d, the unresolved 1464c25c): see 4 and Not read.

## 3. The Collision body and the batch: fields, owner, what the loader does next

Body (`Collision`), field-name stores at [game] addresses (the reading function is not settled):
- parent 005b972d; dynamics 005b974a; mass 005b97c6; inertia 005b97f8; gravity 005b9899; startAsleep 005b98e8; linearDamping 005b991a; angularDamping 005b993f; anchorForce 005b9998; material 005b99c5; matrix 005b9b1f; cgRot 005ba433 and 005ba44f; center 005ba4c8 and 005ba4e4; links 005ba5f8; linkedThing 005ba649; parentMatrix 005ba695.
- primitives (hash 0a2e9b9a, [data] name) is read in the body range: its address was not located by this pass.

Batch (`CollisionBatchPrimitive`):
- packet 005b9e36 (branch 005b9e27).
- maskPreset 005ba2f4; sendMask 005ba394; receiveMask 005ba3b9; material 005ba3de (a second read of `material`, after the body's 005b99c5). The survey's batch list says matrix, material, maskPreset, sendMask, receiveMask, packet [data]; `matrix` is read at 005b9b1f, before the dispatch: the owner of that read is not settled.
- Before the dispatch, for every element, the loader reads five floats at element `+0x1c..+0x2c` (005b9d70-005b9da0) and, after each shape, stores three of them into a 12-byte array at `[ebp+0x30]` indexed by the element number (005ba263-005ba28c) [game]. Meaning not settled.

What happens to a finished shape:
- Each branch calls `FUN_009518f0` with the element's end pointer and then `FUN_00557150` with the pair `{shape, owner}` in ECX (the pair is written by the wrapper FUN_005bace0, read in full: ledger 005bace0). The Cube branch goes straight to the wrapper; the others first call their fill function.
- FUN_005bace0 pool-allocates a 0x10-byte control block: vtable 00b5b9c4 (`ltl2::control_block`, [data] vtables.tsv) and then 00b5ccac (not in vtables.tsv), with a refcount word of 1 and a flag word `|0x80000000` [game].
- The top switch ends with the same pair of calls (009518f0 with param_1, 00557150, 00562970) [game].
- The registry or list that receives the pair is NOT identified: 009518f0 and 00557150 are unread (census labels 'ui' and 'camera/lux::Debris' are not relied on).

## 4. Shape classes built outside this loader

Capsule (00b5ce14): stores in FUN_005b8da0 (+0x141), called from the element dispatch at 005ba247 (this loader family), and inline at 005ba0a2 (packet path). No store outside the loader. Census of FUN_005b8da0: gameplay:game, state unread.

Pencil (00b5cfa8): one store, FUN_006e7e90 +0x8d, `pencil_avatar_shape` (census gameplay:game, movement, read; docs/climbing.md, core\src\climb.rs). Not the loader.

Also stored outside the loader (for the record):
- Cube 00b5ce3c: FUN_006e81e0 +0x76, `cuboid_avatar_shape` (census gameplay:game, camera, read; docs/camera.md).
- Sphere 00b5cdc4: FUN_006e7ff0 +0x57 (census gameplay:game, unread); FUN_0091fee0 +0xe9, `camera_proxy_construct` (census camera, read; docs/camera.md); FUN_005bc280 +2 (census labels it `lux::CollisionHull4`, state unread: [assumed] mislabel, not checked).

## Assumptions

- [assumed] The element types without `Packet` (CollisionSphere, CollisionCylinder, CollisionCuboid, CollisionCapsule, CollisionCone, CollisionMesh, CollisionHull) are shapes that appear as list elements without a batch. Reason: the dispatch is by hash and the survey shows only CollisionBatchPrimitive wrapping packets. Settles by: finding a level with a direct element of one of these types.
- [assumed] `+4` (the packet data pointer) is an address resolved from a relative word of the packet view. Reason: the store computes `edx + [edx]`. Settles by: reading the view code (FUN_00573bd0 and the packet view) and one fill function.
- [assumed] The switch value of field 1464c25c is a shape kind or size, not a vertex count. Reason: the hash resolves to no name here and the case values are 4, 8, 16, 32. Settles by: the name, or the packet data of a `CollisionHull` element.
- [assumed] 00b5ccac is a deleter variant of ltl2::control_block. Reason: it is an ltl2 control block layout. Settles by: its slot 0 in the vtable table.
- [assumed] The outer-element and packet-path shapes are the same class with the same vtable; only the way the data is held differs.
- [game] Inconsistency, NOT resolved: the top switch's second count (`lea eax,[esp+0x10]`, 005b92a7) reads a stack slot that none of the three field lookups fills (faces is written to the slot at esp+0x18 relative to entry; the count reads esp+0x4 relative to entry). The first count reads the vertices pair. Either the count takes its pair from another slot, or the view convention is different from this reading. Not settled.

## Not read

- Fill functions: FUN_005bb110 (soup), FUN_005baf80 (cone), FUN_005baea0 (cylinder), FUN_005badc0 (sphere), FUN_005bb060 (capsule), FUN_005bb3a0 (hull32), FUN_005bb640 (hull16), FUN_005bb8d0 (hull8), and the top hull fills FUN_005bb930, FUN_005bb6a0, FUN_005bb400, FUN_005bb170. The Hull vtable functions FUN_005bc170, FUN_005bc1c0, FUN_005bc200, FUN_005bc240 are read only by their vtable store.
- The element-level constructors FUN_005b8740, FUN_005b88a0, FUN_005b89f0, FUN_005b8ba0, FUN_005b8da0 (bodies); FUN_005b8fa0 body was read earlier (collision-soup.md).
- FUN_009518f0, FUN_00557150, FUN_00562970, FUN_005343a0 (pool), FUN_005bace0 is read.
- The loop head: which function walks the elements and sets `ebx` (the element hash) and the `[ebp+0x68]`/`[ebp+0x6c]` view pair before 005b9d70.
- Field 1464c25c: not in names.tsv; 14 candidate names tried (type, hullType, vertexCount, numVertices, stride, size, kind, and others): no match.
- Range 005ba2c0-005ba400 beyond the batch fields listed in 3 (remaining body fields, links).

## Next tickets (in order)

1. Read the fill functions in 1b and the top hull fills (list under Not read): record which packet fields each copies and how (conversion, inversion, scale). Use `work\raw_disasm.py` on each, not decompile.
2. Find the loop head above 005b9d70: the element walk, `ebx`, and the view pair. Read it with `work\raw_disasm.py` from the function's start.
3. Read FUN_00557150 and FUN_009518f0 (the registration of `{shape, owner}`), and FUN_005bace0's caller contract (esi/edi).
4. Resolve field 1464c25c and the primitives address (0a2e9b9a) from data: a pack's field table, not a grep of objects.tsv (2 GB).
5. Check FUN_005bc280 +2 (Sphere store inside a CollisionHull4 function) and the 00b5ccac vtable slots.
6. Read 005ba2c0-005ba400 for the remaining body fields (links, linkedThing, parentMatrix) and where the body object is stored.
