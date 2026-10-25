# Collision shape types 0-10 (ticket: name the 11 shape types, reader pass, 2026-10-11)

Tags: [game] address or raw bytes, [data] file, [assumed] reason and what would settle it. Static reads only. Scripts: `work\kernels\shape_candidates.py` (vtable candidates and slot bytes), `work\kernels\dump_table.py` (table entries). Ledger: the 7 slot-4 functions not yet in the ledger were added (`shape_type_slot4_*`, ledger entries 1071-1077).

## 1. Table: type number -> class

The type number is the value the class returns from its vtable SLOT 4 (offset +0x10). This settles the conflict with `docs/camera.md` (see section 4).

| type | class | vtable | slot 4 function | stride (00b8dd64) | ray test (00b8dd0c) size | sphere test (00b8dd38) size |
|---|---|---|---|---|---|---|
| 0 | lux::CollisionSoup | 00b5cdec | 008b7f50 | 4 | 005bcb30, 2014 | 005bd320, 2043 |
| 1 | lux::CollisionSphere | 00b5cdc4 | 004d84e0 | 4 | 005c1ed0, 6036 | 005d1be0, 6569 |
| 2 | lux::CollisionCube | 00b5ce3c | 004e9cc0 | 16 | 005c3670, 6559 | 005d35a0, 6582 |
| 3 | lux::CollisionCapsule | 00b5ce14 | 0053a390 | 48 | 005c5020, 6689 | 005d4f70, 6569 |
| 4 | lux::CollisionCylinder | 00b5ce64 | 005bbee0 | 8 | 005c6a50, 6689 | 005d6930, 6582 (ledger name: convex_sweep) |
| 5 | lux::CollisionCone | 00b5ce8c | 005bbed0 | 16 | 005c8480, 6689 | 005d8300, 6582 |
| 6 | lux::CollisionPencil | 00b5cfa8 | 0081e620 | 16 | 005c9eb0, 6689 | 005d9cd0, 6582 |
| 7 | lux::CollisionHull32 | 00b5ceb4 | 005bbe80 | 4 | 005cb8e0, 6324 | 005db6a0, 6521 |
| 8 | lux::CollisionHull16 | 00b5cedc | 006e8d20 | 4 | 005cd1a0, 6324 | 005dd030, 6521 |
| 9 | lux::CollisionHull8 | 00b5cf04 | 005bbe60 | 4 | 005cea60, 6324 | 005de9c0, 6521 |
| 10 | lux::CollisionHull4 | 00b5cf2c | 005bbe50 | 4 | 005d0320, 6324 | 005e0350, 6521 |

Sizes are from the function size in census (column `size`); they match the dump of the table.

## 2. Evidence per row

`[game]` slot 4 bytes and raw_disasm: every slot 4 below is `mov eax, N; ret` (8 bytes read) except CollisionSoup's, which is `xor eax, eax; ret` (0).

Slot 3 is the stride. Slot 3 of each class equals the stride table at 00b8dd64 for its type number, in all 11 rows: 4, 4, 16, 48, 8, 16, 16, 4, 4, 4, 4 (slot 3 values: Soup 4, Sphere 4, Cube 16, Capsule 48 (`b830000000`), Cylinder 8, Cone 16, Pencil 16, Hulls 4). `[game]` the equality; `[assumed]` that the stride table is per type and not per shape count.

Slot 1 is type + 2 in all 11 rows: Soup 2, Sphere 3, Cube 4, Capsule 5, Cylinder 6, Cone 7, Pencil 8, Hull32 9, Hull16 10, Hull8 11, Hull4 12. `[game]` values. So slot 1 is NOT the type index (see section 4).

Slots 5 and 7 (`[game]` bytes): Soup slot 5 is `fld [00cad454]` (0.0199999996); slot 7 is `fld [00b2d970]` (-1.0). Hulls and CollisionBody use the same two functions (005bbd40, 005bbd50). Cone, Sphere and Cylinder slot 5 read a float through the pointer at +4. Cylinder slot 7 is `mov eax,[ecx+4]; fld [eax+4]; fadd st0,st0`. Pencil slot 5 is `fld [ecx+4]` and slot 7 is `fld [ecx+0xc]`. Capsule slot 5 is `fld [[ecx+4]+0x20]`. Cube slot 5 reads through the pointer at +4 and adds a second float (bytes only; 8-byte window).

## 3. Not in the 0-10 set, checked

- CollisionBody (00b5cd9c, no type): slot 4 005bbd30 is `or eax,-1`, so -1. It is the base class, not a shape type.
- apsSimpleMeshRenderer (00b69168): slot 4 is 008b7f50, returns 0, same stub as Soup. It is a renderer, not a collision shape (its slot 0 is the apsSphereDomain family). Not a type.
- GFx Shape classes: slot 4 is a `jmp` thunk, not a constant. Not shapes.
- lux::DeferredLighting::CapsuleRenderNode and lux::SSD::DestructionSphereRenderNode: slot 4 007bb820 is `xor al,al`. Not shapes.
- The 11 classes with slot 1 values 11 and 12 (Hull8, Hull4): outside 0-10 only for slot 1, which is not the index.

## 4. The slot 1 / slot 4 conflict (settled here)

`docs/camera.md` (audit) says the shape type is slot 1 (cuboid type 4, pencil type 8, "twelve edges" for type 4). the unpublished notes says slot 4 is the index read by 005be890. Settled for slot 4 by three facts:

1. The tables at 00b8dd0c, 00b8dd38 and 00b8dd64 have 11 entries each. Slot 1 gives 12 for Hull4, which overflows them.
2. Slot 3 (the stride) matches the stride table only under slot-4 numbering. Under slot-1 numbering the Cube (slot 1 = 4) would take stride[4] = 8, but its slot 3 is 16.
3. Slot 1 is type + 2 in all 11 rows, so it is a second numbering.

So the camera audit's "shape type 4" for the cuboid is slot-1 numbering. In the table's numbering the cube is type 2. The "twelve edges" of a box fits the cube, type 2. `[assumed]` that the camera audit's other uses of slot 1 are also slot-1 numbering: not checked. `docs/camera.md` is not edited here.

## 5. One line per class (layout only where the stride or a read makes it plain)

- 0 Soup: stride 4; no data layout read here (see collision-soup.md).
- 1 Sphere: stride 4 [data]; slot 5/7 read a float through the pointer at +4. Layout not read.
- 2 Cube: stride 16; layout not read.
- 3 Capsule: stride 48 = [assumed] three 16-byte records (the example in the ticket); slot 5 reads the float at +0x20 of the pointer at +4. Not read further.
- 4 Cylinder: stride 8; layout not read.
- 5 Cone: stride 16; layout not read; slot 7 is the constant -1.0.
- 6 Pencil: stride 16; floats at this+4 and this+0xc (slots 5 and 7, [game]).
- 7-10 Hulls: stride 4 [data]; slots 5 and 7 are the shared constants; layout not read (the name suffix 4/8/16/32 is not checked against vertex count).

## Assumptions

- [assumed] The row and column of the 11 x 11 pair matrix at 00b8db28, and the ray and sphere entries, index the same type numbering as slot 4. Settled by: the pair cells having a row-to-class pairing (not checked here) and the bins in the unpublished notes.
- [assumed] Class names are the RTTI names in `export\vtables.tsv`. Settled by a constructor or the RTTI record.
- [assumed] Stride 48 for Capsule is three 16-byte records. Settled by reading the Capsule constructor or a recording.
- [assumed] The ray and sphere entries are the functions named in census. Some have a ledger name that is not the census name (convex_sweep at sphere entry 4).

## Not read

- Every ray test (00b8dd0c entries) and sphere test (00b8dd38 entries): census sizes only. Their bodies are not read.
- The 11 pair cells at 00b8db28 per type pair.
- The constructors of each shape class (the layouts in section 5 are from strides and slot bytes only).
- Slot 1 functions 00820540, 005bc1b0 (type+2 values) and slot 5/7 bodies beyond the byte windows.
- The value of the float constant at 00cad454 beyond 0.0199999996 and at 00b2d970 beyond -1.0 (read with pe_read floats).
