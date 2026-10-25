# Flight boundary: the volume a flyer must stay inside, port recipe

Status: read from the decompile, checked in disassembly where marked. Ticket scope: the
boundary object, its inside/outside test (007e7b60), its per-update check (007e7830), the
setter 0086ab90 (full body), the registrars, and the state-machine condition
INSIDE_FLIGHT_BOUNDARY (hash f65c039d). Tags: `[game]` read in code (address given),
`[data]`, `[assumed]`, `[trace]` (needs a recording or an unread callee), `[unread]`.

Summary for the programmer, in two lines:
- SHAPE: a box in a boundary-local frame (|u| <= 1.01, |v| <= 1.01, s <= 1.01), or the
  "ellipsoid" variant where the side test is a radius-1 circle u*u+v*v <= 1 (flag +0x94 = 1)
  with the same s ceiling. There is no floor test in s. `[game]`
- TEST: the character's ROOT point P (character_camera_target_point) is mapped into (u,v,s);
  OUTSIDE sets mode+0x55 = 1 on the FlyMode and feeds the bounce / wall-stop path; INSIDE clears it.

## 0. Frame and layout conventions used below

- Affine 3x4 read as in flight-contact.md section 0: `L = a*X + b*Y + c*Z + t`, the basis
  vectors stored column-wise with stride 3 (`[game]`, checked in 007e7b60 disasm: the three
  coefficients of the x-input are at +0x64, +0x68, +0x6c; of the y-input at +0x70, +0x74, +0x78;
  of the z-input at +0x7c, +0x80, +0x84; translation at +0x88, +0x8c, +0x90).
- Boundary object is `B` (the object whose vtable slot +0x8 is 007e7830). Its fields are
  written `B+0x..`. Character is `char`, the flying mode is `mode` (FlyMode, class id
  fb9f3262; HoverMode class id 70776cef).
- Point test input: `P` = root position (camera target point, 0071a2e0). The test reads
  `P.x = [eax]`, `P.y = [eax+4]`, `P.z = [eax+8]` (007e7b64..007e7b6e). `[game]`

## 1. The data: what a FlightBoundary object is

### 1.1 Two classes, one object shape

Both are the same C++ class (constructor 007e7540, vtable `PTR_FUN_00b60908`, slot +0x8 =
007e7830 `flight_boundary_contact_check`, `find_ptr` hit at 00b60910). `[game]`

| class | name hash (names.tsv) | registrar | factory | `B+0x94` (variant) |
|---|---|---|---|---|
| FlightBoundary | fdc81a37 "FlightBoundary" | 00adbf70 (registers factory at 00adbfa4 = 007e7780) | 007e7780 (no store to +0x94) | 0 = BOX |
| FlightBoundaryEllipsoid | (not in names.tsv) | 00adbfd0 (factory LAB_007e7810) | 007e7810 -> thunk 01196e50 | 1 = ELLIPSOID |

- 01196e50 (FlightBoundaryEllipsoid factory): same allocation id 0x13 as 007e7780, builds the
  object with 007e7540, runs the loader thunk 011882b0, then `*(char*)(obj+0x94) = 1`. `[game]`
- 007e7780 (FlightBoundary factory): same, without the +0x94 store, so +0x94 keeps the ctor's 0. `[game]`
- The ctor 007e7540 zeroes `+0x28` (dword index 0xb..0x18 = bytes 0x2c..0x60), `+0x94` (byte,
  via `param_2+0x25`), and calls the base ctor 00577b40 and 00578550 if flag `+0x16 & 1` is clear. `[game]`

Survey counts (the unpublished notes line 96, `survey-level-contents.md` line 179):
FlightBoundary is a ZONE object, one per zone, registered through 00574cd0. `[data]`
(the count is from the survey, not checked here).

### 1.2 Fields (B = boundary object)

| field | what | writer | reader | tag |
|---|---|---|---|---|
| +0x2c..+0x58 | affine B: rows u, v, s (see 1.3) | not found in 011882b0 (ctor zeroes it) | 007e7b60 | `[game]` reader; writer `[trace]` |
| +0x5c, +0x60 | two dwords copied from data field `boundary` (hash 3e7e0252) | 011882b0 l.11-12 (`*(B+0x5c)=puVar3[0]`, `*(B+0x60)=puVar3[1]`) | not read by the test or by 007e7830 | `[game]` |
| +0x64..+0x90 | affine A: world point P -> local L (see 1.3) | not found in 011882b0 (not zeroed by ctor) | 007e7b60 | `[game]` reader; writer `[trace]` |
| +0x94 (byte) | variant: 0 box, 1 ellipsoid | factories 007e7780 / 01196e50 | 007e7b60 l.`cmp byte [ecx+0x94]` (007e7d15) | `[game]` |
| +0x28 (byte) | last state: 1 when the character was found OUTSIDE, 0 when INSIDE | 007e7830 with param_1 = B itself (ECX = B, `mov ebx,ecx` at 007e7847; stores at 007e7a8d, 007e7aac, 007e7b1b) | not read here | `[game]` |

Unknown: where the 12-dword copy of 011882b0 (`FUN_00572110`, "source ECX, dest EAX",
`docs/character-world-contact.md` s.4) lands. The call site has no explicit ECX/EAX setup in
the decompile. The fields that the test reads (+0x2c..+0x58, +0x64..+0x90) are each 12 dwords,
so the "boundary" data field is probably one of these two 3x4 affines; that is `[assumed]`.

### 1.3 The two affines (checked in disassembly 007e7b60-007e7c80)

A (world P to local L), `[game]` (007e7b60..007e7c03):
```
a = B.64*P.x + B.70*P.y + B.7c*P.z + B.88
b = B.68*P.x + B.74*P.y + B.80*P.z + B.8c
c = B.6c*P.x + B.78*P.y + B.84*P.z + B.90
```
B (local L to box coordinates u, v, s), `[game]` (007e7c03..007e7c6f):
```
u = B.2c*a + B.38*b + B.44*c + B.50
v = B.30*a + B.3c*b + B.48*c + B.54
s = B.34*a + B.40*b + B.4c*c + B.58
```
Constant tolerance `1.01` at `00b6ae3c` (007e7c6f, comiss at 007e7c77) and `-1.01` at `00b6ae38`
(007e7cb0). `1.0` at `00b38834` (xmm6). `-1.0` at `00b2d970`. `[game]`

What L and (u, v, s) are geometrically: L is P in the boundary's own axes (A is the inverse of
its placement, `[assumed]`); (u,v,s) is L scaled and shifted so that the volume is the unit
region. If the scale is non-uniform (radii) the frame is not orthonormal, which matters for
the normal (section 3, item 8). `[assumed]`

### 1.4 Instances in the install (data)

- The class-level data is not in the global dumps. `grep -l` over `export\data\*.txt` finds
  FlightBoundary only as a state-machine connection name (`global_states.txt` 1933, 3743,
  `connectionEnvInsideFlightBoundary`). No instance numbers. `[data]`
- Zone instances hold the object; the key is data field `boundary` (3e7e0252). Which pack holds
  it: `[unread]` (the survey counts it as one per zone; `<install>\CLAUDE.md` has no entry).
- Real numbers of one instance: NOT FOUND in this ticket. The decoding of the pack's objects
  (`tools\tf2_lxb.py dump`) was not run because the ticket does not allow the 2 GB objects file.
  Next step: `python tools/tf2_lxb.py dump <zone pack>.str#N` on a mission zone
  that contains a FlightBoundary (the mission with flight, `--mission` list), and read the
  two 12-float blocks named `boundary`. `[trace]`

## 2. Where the boundary lives on the character side (who it is checked against)

Object chain used by 007e7830, `[game]` (007e7830 l.5-16, asm 007e7847-007e78c0):

```
P    = character_camera_target_point(char)           (0071a2e0, root position)
char = FUN_00712190(&local, 0)                        (the character, index 0)  [assumed] = the player
mode = *(*(char+0x314)+0x14)+0x10                     (flight-mode holder of the zone/char)
```
The check runs only when `char+0x104` holds a FlyMode (class fb9f3262) or FUN_0071a570 (the
current form class) is HoverMode (70776cef). Otherwise it returns at once with
`FUN_00562970()` (release). `[game]` (asm 007e7857-007e78c0).

## 3. The per-update check 007e7830 `flight_boundary_contact_check(B)`

Called through the vtable: `B`'s vtable slot +0x8 (find_ptr: 00b60910 = 00b60908+8).
Direct calls: none (find_call finds none). Who calls the vtable is `[trace]`; it is per
update of whatever list holds the boundary (`[assumed]`). It is not per flyer: it takes the
single character `FUN_00712190(..., 0)` and only that character's FlyMode is written. `[game]`

Pseudo-code (numbered, instruction addresses in brackets):

1. `ebx = B` (this) `[007e7847]`. `char = FUN_00712190(&tmp, 0)`; if char == 0 return `[007e7849-007e7857]`.
2. `P = root(char)` via 0071a2e0 `[007e785d-007e7864]` (the 3 floats at `E+0x28`).
3. If `char+0x314 == 0` or `*(char+0x314)+0x14 == 0`: return `[007e7869-007e7877]`.
4. Gate (the character is flying or hovering): if `char+0x104 == 0`, or the object at
   `*(char+0x104)+0x10` is null, or its class != fb9f3262, AND `FUN_0071a570(char) != 70776cef`:
   release and return `[007e7889-007e78b4]`. `[game]` (decompile l.7-15; the asm of the
   class test at 007e7899..007e78a9).
5. `mode = *(*(char+0x314)+0x14)+0x10`; if null: release and return `[007e78c3-007e78d1]`.
6. Classify `mode`: class fb9f3262 (FlyMode) -> `edi = mode`; class 70776cef (HoverMode) ->
   `ebp = mode` `[007e78d3-007e78fb]`. (Decompile: `piVar6` = FlyMode, `piVar5` = Hover.)
7. `cVar = FUN_007e7b60(...)` with EAX = `&P` (`lea eax,[esp+0x2c]` at 007e7902), EDX = `&N0`
   (the output normal buffer, `lea edx,[esp+0x20]` at 007e78fe), stack arg = `B` (`push ebx`
   at 007e78fd). `[game]` (asm). ECX is passed but not read by the test (its param_1 is unused).
8. If `cVar == 1` (INSIDE) go to step 12. If `cVar == 0` (OUTSIDE):
   a. `R = FUN_00550930(B)` (`mov eax,ebx; call 0x550930`, 007e7918): the boundary's world
      affine (basis at R+0,3,6; translation at R+0x24, +0x28, +0x2c). `[game]`
   b. Rotate the normal into world: `N_w.x = R[0]*n.x + R[3]*n.y + R[6]*n.z` (and y, z with
      R[1,4,7], R[2,5,8]); i.e. `N_w = n_local . basis`. `[game]` (decompile l.25-31; asm
      007e791f-007e7999 is the same arithmetic, checked by the mulss/addss order).
   c. `|N_w| = sqrt(N_w.x^2 + N_w.y^2 + N_w.z^2)` (007e79e0, 0x409a70), stored as the 4th float. `[game]`
   d. `V = N_w / |N_w|` (`FUN_005571b0`, 007e79f4): the out vector stored at `mode+0x5c..0x64`.
      `[game]` (005571b0 reads param[3] and divides if > 1e-8).
   e. `C = FUN_00550930(B)` translation: `D = C - P`, `|D|` (007e7a03-007e7a3e, l.36-42).
   f. `N = D / |D|` (`FUN_005502f0`, 007e7a75; divides if param[3] > 1e-8, the 4th float is |D|).
      Stored at `mode+0x68..0x70` by 0086ab90. `[game]`
   g. If `edi != 0` (FlyMode found): `flight_contact_set(mode, arg=1)`: `ECX = &N`, `EAX = &V`,
      push 1, call 0086ab90 `[007e7a7e-007e7a88]`; then `param_1+0x28 = 1` `[007e7a8d]`.
   h. Else if `ebp != 0` (Hover only): `FUN_00871ca0(1, &V)` `[007e7a96-007e7aa7]` (not read);
      then `param_1+0x28 = 1`. `[game]`
9. If `cVar == 1` (INSIDE): if `edi != 0`: zero the stack N/V and `flight_contact_set(mode,
   arg=0)` `[007e7ab4-007e7ae7]`; `param_1+0x28 = 0` `[007e7b1b]`.
10. Else if `ebp != 0` (Hover): clear `ebp+0x55, +0x54`, and `ebp+0x58..+0x6c` to zero
    `[007e7af2-007e7b17]` (this is the Hover's own flag clear; `[game]`).
11. The `B+0x28` (`param_1+0x28`) store is at `007e7a8d`/`007e7aac`/`007e7b1b`: 1 when outside,
    0 when inside. `[game]`
12. Return.

Note: step 8.e uses `C = R+0x24..0x2c` where `R` is the same `FUN_00550930(B)` used in 8.a.
Both are read from the object returned by 0x550930 on `B` (not from `B` itself), so the
affine returned for `B` is the world placement of the boundary. `[game]` (decompile l.25
and l.36, asm 007e7a01-007e7a08).

## 4. 0086ab90 `flight_contact_set(N, arg)` in full

Entry: `EDI = mode` (FlyMode; `unaff_EDI`), `EAX = &V` (in_EAX), `ECX = &N` (param_1), stack
`arg` (char, 0 or 1). `[game]` (the caller's registers at 007e7a80-007e7a88 and 007e7ae7).

Writes, in order (`[game]`, decompile of 0086ab90 full read):

1. If `mode+0x55 == 0` and `arg != 0`: `mode+0x56 = 0` (rising edge). `[game]`
2. `mode+0x55 = arg`.
3. `mode+0x5c = V.x; mode+0x60 = V.y; mode+0x64 = V.z`. `[game]`
4. `mode+0x68 = N.x; mode+0x6c = N.y; mode+0x70 = N.z`. `[game]`
5. If `arg == 0` (the decompile's `if (mode.55 != 0)` is false here): return. The block below
   runs only when `arg != 0`.
6. `mode+0x4c = 0`. `[game]`
7. If `V.z <= -0.95` and `V.z != -0.95` (the vector points straight down: an inverted/steep
   push): `X = *(mode+0x18) + 0x30` (a float in the tuning block, `[assumed]`);
   - if `mode+0x44 < 0`: `mode+0x58 = -X` else `mode+0x58 = +X`. `[game]` (asm 0086ab90 l.~40,
     `(float)(*(mode+0x18))[0xc]` = byte offset 0x30)
   - return the `mode+0x18` pointer (EAX). `[game]`
8. Else: `rotate_z_matrix(pi/2)` (0056fc50, `0x3fc90fdb` = 1.5707963 at l.~43), `body_world_matrix`
   (006de7e0) of the owner `mode+4`, and a dot product of `V` with the rotated body basis
   (`fVar7*(V.z*Ly + V.y*...) + ...`). If that dot is `< 0`: `mode+0x58 = -X`, else `mode+0x58 = +X`.
   `[game]` (decompile l.~60-70; the sign test is not checked in disassembly: `[assumed]` the
   exact axis pairing).

What it does NOT write: mode+0x44, +0x1c, +0x20, +0x24 (those are written by 00868b00 later),
and not the body velocity.

The value stored at `mode+0x58` (`X`) is the same field 00868b00 uses as the roll/yaw target ramp
(`docs/flight-forces.md` step 2, "ramp = mode+0x58"). `[game]` (the read) .

## 5. The two vectors: what they are

- `V = mode+0x5c..0x64` = `unit(R_B . n_local)`: the boundary's violated face normal (or the
  radial/ceiling normal, section 6), rotated into world with the boundary's own world basis
  and normalised. It points from OUTSIDE back INTO the volume. It is the push-back direction.
  Test case: `V.z = -1` means "the boundary's ceiling was hit from above" (the character is
  above the top). `[game]` for the construction; the geometric reading "push-back direction"
  is `[assumed]` from the signs in section 6 (n = (-1,0,0) for u > 1.01 points back to u smaller).
- `N = mode+0x68..0x70` = `unit(C - P)`: the direction from the character's root P toward the
  boundary's world centre C (the translation of the boundary's world affine, `FUN_00550930(B)+0x24..`).
  This IS "the direction back to the centre". `[game]` (the subtraction order at 007e7a03-007e7a2f
  is `C - P`: `fStack_84 = C.x - P.x`). In 00868fb0 it is compared with the forward direction
  `w` (`dot(N, w) >= 0.8` gives out = (0,1,0)).
- The earlier brief question "is it the ellipsoid's surface normal at the exit point?": NO. `N`
  is the centre direction, not the surface normal. `V` is the surface (face) normal, but it is
  the normal of the box face in the boundary's own axes, rotated, not the true ellipsoid normal.

## 6. 007e7b60 `flight_boundary_inside(P, out_n, B)` (the inside / outside test)

Inputs: `P` (EAX), `out_n` (EDX, 3 floats), `B` (stack). Returns `al`: 1 INSIDE, 0 OUTSIDE.
(The `ecx` argument is unused.)

Numbered pseudo-code (asm 007e7b60-007e7d15, decompile for the cylinder part):

1. `a, b, c = L(P)` (A, section 1.3). `[game]` 007e7b64-007e7c03.
2. `u, v, s` (B, section 1.3). `[game]` 007e7c03-007e7c6f.
3. `out_n = (0,0,0)`; `al = 1`; `bl = 1` (bVar5, "not ceiling"). `[game]` 007e7c80-007e7c9b.
4. If `u > 1.01`: `out_n.x = -1`; `al = 0`. `[game]` 007e7c99-007e7cac.
   Else if `u < -1.01`: `out_n.x = +1`; `al = 0`. `[game]` 007e7cb0-007e7cc6.
   Else if `v > 1.01`: `out_n.y = -1`; `al = 0`. `[game]` 007e7cca-007e7ce2.
   Else if `v < -1.01`: `out_n.y = +1`; `al = 0`. `[game]` 007e7ce4-007e7cf4.
   Else if `s > 1.01`: `out_n.z = -1`; `al = 0`; `bl = 0` (the ceiling: no radial). `[game]`
   007e7cf6-007e7d0e.
   Else: inside the box: nothing else.
5. `if B.+0x94 != 0` (ellipsoid variant, 007e7d15-007e7d1c):
   - if `al == 1` (still inside the box): if `u*u + v*v <= 1.0` return 1; else `al = 0`.
   - if `bl == 1` (not the ceiling): `out_n = (-u, -v, 0)`. `[game]` (decompile only; the 1.0
     compare and the `-0.0 - u` stores are in the decompile, not disassembled: `[assumed]` the
     exact compare at `u*u+v*v <= 1.0`).
6. Return `al`.

Consequences, `[game]`:
- The BOX variant (+0x94 = 0): inside iff `|u| <= 1.01`, `|v| <= 1.01`, `s <= 1.01`. There is
  NO lower limit in s (no floor): a character far below the boundary is INSIDE.
- The ELLIPSOID variant (+0x94 = 1): inside iff `u*u + v*v <= 1` (a radius-1 cylinder, not an
  ellipsoid: the name is the class name only) and `s <= 1.01`. The side normal is the radial
  `-(u,v,0)`, not unit length (the caller normalises it).
- With B's affine A/B left at zero (the ctor's state, section 1.2), `u = v = s = 0`, every test
  passes and the answer is INSIDE: the "no boundary data" default is INSIDE. `[game]` (derived
  from the test) and `[assumed]` that the zone sets B (section 1.2).

Units: u, v, s are the normalised box units (1.0 = the face). The true size in metres is in
the B affine (scale in its rows), not read here. `[unread]`

## 7. Is there more than one boundary kind? Default when none is present

- Two kinds, one test: the BOX (`FlightBoundary`, +0x94 = 0) and the cylinder-or-"ellipsoid"
  (`FlightBoundaryEllipsoid`, +0x94 = 1). `[game]`
- Census: FlightBoundary (007e7540, 007e75c0, 007e7700, 007e7780, 007e77c0, 011882b0, 01196e50,
  00adbf70) and FlightBoundaryEllipsoid (007e7810, 007e7830, 007e7b60, 00adbfd0). `[game]` (census).
  007e7540/75c0 are not read; 007e75c0 (309 bytes) is `[unread]`.
- No "no boundary" branch in the test; the default is set by the caller: if no FlyMode and no
  Hover (step 6), or the zone has no boundary object, nothing is written and mode+0x55 keeps its
  last value (0 by default, so INSIDE). `[game]` (steps 3-5).
- The character's boundary (zone object) is found through the zone's list; the search for it
  is not read. `[unread]`

## 8. The state-machine condition INSIDE_FLIGHT_BOUNDARY (hash f65c039d)

The env test is inside the environment dispatcher `FUN_0084cee0` (the ledger name
`env_test_370a5fba_floor_wall` is for the 370a5fba branch only; the boundary case is the
`cmp eax, 0xf65c039d` at 0084d34f). `[game]` (`find_imm` hit at 0084d350; asm 0084d339-0084d3f2).

Code, `[game]`:
```
case 0xf65c039d (0084d34f):
  owner = state_context_owner(this)                              (0084c750)
  edi = owner's character  (0084d37e: mov edi,[esp+0x58])
  if edi == 0                         -> return TRUE              (0084d382)
  if !character_input_is_player(...)  -> return TRUE              (0084d386-0084d38b; 00711210)
  if FUN_00725890(edi)                (0084d391)  [unread]
      if edi.+0x104 == 0              -> return (uninitialised byte [esp+0x13])  (0084d39a-0084d3a2)
      class = FlyMode fb9f3262
  else if !FUN_00736c10(edi)          (0084d3b1)  [unread] -> return TRUE  (0084d3b6-0084d3b8)
      if edi.+0x104 == 0              -> return (uninitialised)   (0084d3ba-0084d3c2)
      class = HoverMode 70776cef
  mode = FUN_008521e0(edi.+0x104, &class)                         (0084d3cd)
  if mode == 0                        -> return (uninitialised)    (0084d3d4-0084d3d4)
  return (mode.+0x55 == 0)            -> sete                      (0084d3d6-0084d3e3)
```
So INSIDE_FLIGHT_BOUNDARY = `mode.+0x55 == 0` for the player's current flying form (FlyMode if
725890 holds, else HoverMode if 736c10 holds), TRUE when the character is not the player or
not in flight, and an uninitialised byte when the mode is missing. `[game]` (asm). Readers of
the flag: this test only; the flag itself is written by 0086ab90 (section 4).

Facts checked against the notes: the connections using it (`flight-states.md` lines 177-183,
`connectionListFlyRelease`, `connectionListFlyToAirPunch`, `...ReleaseAndAnimDone`) use
`EnvInsideFlightBoundary` as a required env test; so "outside" (`mode+0x55 != 0`) blocks the
release/punch transitions. `[game]` (the notes' table) .

`[unread]`: `FUN_00725890`, `FUN_00736c10` (the form tests), `FUN_008521e0` (mode by class),
and the uninitialised-byte path (the value of `[esp+0x13]` at entry to the case; `[trace]`).

## 9. What 00868b00 and 00868fb0 do with V and N once OUTSIDE (from the existing notes)

From `docs/flight-forces.md` lines 96-180 (not re-read in full here; read, not solid):

- Taken INSTEAD of the stick steering when `mode+0x55 != 0` (`flight-contact.md` section 4,
  00864320 l.159 / l.162). `[game]`
- 00868b00 (`flight_surface_bounce`) when `char+0x84 == 0` and `char+0xb0 == 0`, otherwise 00868fb0
  (`flight_wall_stop`). `[game]` (branch)
- 00868b00 steps 1-6, as the notes read them: (1) the velocity is reflected about a 2D normal
  built from the steer vector, with coefficient -0.9; (2) a latch on `d = dot(N, steer)` with
  `d > 0.95` or `mode+0x56 != 0` sets `mode+0x44 = 0`, `mode+0x56 = 1`; (5) if `V.z >= -0.95`
  the roll `mode+0x1c` decays (rate 5*dt), else (steep or inverted) body velocity is pushed by
  `V * 30 * dt` and the roll lerps to -pi/6 at rate 2*dt. `[assumed]` on the sign and the pairing
  of steer components (the notes say so).
- 00868fb0 (`flight_wall_stop`): if `V.z >= -0.95`: `c = dot(N, w)` with `w` the forward-based
  horizontal vector; `c >= 0.8` gives out = (0,1,0); else the body velocity is reflected (-0.9)
  and scaled 0.95 along `d = N - w`. Then `FUN_00867f20(out, dt)` (not read). Inverted (`V.z < -0.95`):
  a once-only UI tip and a virtual call on the input source. `[game]` (branch); the vector algebra
  of the else branch is `[trace]` (the notes say so: `local_a4..` not traced).

Plain reading, for the programmer (not solid, see the marks above): when the character is outside,
the game does not steer or contact-probe; it turns the body so the forward direction goes back
toward the centre of the boundary (N), reflects the velocity off the face (V, -0.9 of the
normal velocity), and when the face is steep or upside-down it pushes the body along V at
30 m/s-units*dt (`[assumed]` units) and leans the roll to -30 degrees. Whether it is a bounce, a
turn-back or a stop depends on the flags above; the notes call it "bounce" (00868b00) and
"wall stop" (00868fb0). The two functions are the "turned back / bounced / stopped" behaviour
the brief asks for, and they are not fully checked.

## 10. Not read / not sure

- Not read: 007e75c0 (309 bytes), 007e7540 body beyond the ctor, 00adbf70/00adbfd0 registrar
  bodies beyond their 3 stores, 011882b0 beyond the two dwords (the 12-dword copy destination),
  00550930's branch (`FUN_005509d0`, `compose_affine@0052fbf0`), 005571b0 and
  005502f0 read in decompile only (short, checked by their own text), 0056fc50, 006de7e0 (ledger),
  00871ca0 (hover notice), 0071a570, 00712190, 00725890, 00736c10, 008521e0, FUN_00562970.
- Not sure: (a) the writers of affines A and B (the zone load), so the real numbers of a boundary;
  (b) the caller of the vtable slot +0x8 and its rate; (c) whether the ellipsoid's (u,v) scale is in B
  (non-uniform scale and the normal); (d) the sign pairing in 0086ab90 step 8 (not disassembled);
  (e) the uninitialised byte of the env test on the "mode missing" path; (f) which character
  FUN_00712190(..., 0) returns (player or first of the list).
- Not checked in disassembly: the cylinder's `u*u+v*v <= 1.0` (decompile only) and the constant
  `1.0` for it; the sign of the normal for branch 4 (decompile and asm agree on the signs above).

## 11. Field table (boundary object B and mode)

| field | base | meaning | tag |
|---|---|---|---|
| B+0x00 | vtable | 00b60908 (slot +0x8 = contact check) | `[game]` |
| B+0x28 | B | 1 outside / 0 inside (set by 007e7830 on the caller's object `param_1+0x28`) | `[game]` |
| B+0x2c..+0x58 | B | affine rows u, v, s (box coords) | `[game]` (read), writer `[trace]` |
| B+0x5c, +0x60 | B | two dwords of data field `boundary` | `[game]` |
| B+0x64..+0x90 | B | affine A: P to L | `[game]` (read), writer `[trace]` |
| B+0x94 | B | byte: 0 box, 1 ellipsoid (cylinder) | `[game]` |
| mode+0x55 | FlyMode | contact (outside) flag, 1 outside | `[game]` |
| mode+0x56 | FlyMode | cleared on the rising edge of +0x55 | `[game]` |
| mode+0x4c | FlyMode | 0 when set | `[game]` |
| mode+0x58 | FlyMode | roll/yaw target sign * X, X = *(mode+0x18)+0x30 | `[game]` (X `[assumed]` units) |
| mode+0x5c..+0x64 | FlyMode | V = unit(R_B . n_local): push-back (inward face) normal, world | `[game]` (construction) |
| mode+0x68..+0x70 | FlyMode | N = unit(C - P): direction from root to boundary centre | `[game]` |
| mode+0x18 | FlyMode | pointer to a tuning block (+0x30 used here) | `[assumed]` |
| char+0x104 | char | current form mode (FlyMode / Hover test) | `[game]` |
| char+0x314 | char | zone/holder, +0x14 -> +0x10 is the flight-mode object | `[game]` |
| R = FUN_00550930(B) | returned | world basis of B (+0,3,6 basis rows, +0x24.. translation) | `[game]` |

## 12. Reader ticket (2026-10-12): (A) the flight boundary, (B) contact, bounce and wall stop

Read this ticket: 00864320 l.1-163 (the branch), 0086ada0 (FlyMode vtable slot 8, the contact
message; new), 00866760 (slot 9, the flight loop sound; new, no physics), 00868b00 l.1-30 (the
bounce's normal and reflection), 011882b0 (`--full`), 00572110 (header). Sections 1-11 stand
except the corrections in 12.1. Tags as before; `[trace]` = not closed.

### 12.1 Corrections to sections 1-11

- The names `char+0x84`, `char+0xb0`, `char+0xb8` used in sections 3, 8 and in
  docs/flight-forces.md (00868b00 / 00868fb0 gate) are fields of the TUNING BLOCK at `mode+0x18`,
  not of the character. `[game]`: 00864320 l.8 tests `*(param_1+0x18)+0xb8` (canDoStunts),
  l.157-158 test `*(param_1+0x18)+0x84` (helicopter) and `+0xb0` (robot). 0086ada0 tests
  `*(mode+0x18)+0xb0` first. The flag names match the modeFly loader (docs/flight-attributes.md:
  helicopter byte 0x84, robot 0xb0, canDoStunts 0xb8).
- The bounce's velocity change is NOT a reflection. 00868b00 l.16-22: `k = -0.9 * (v . n)`,
  `v = v + k*n`, so the normal component becomes `(1 - 0.9) = 0.1` of what it was (90 % lost).
  `n = (steer.y, -steer.x, 0)`, horizontal, with `steer` the stick vector (not the boundary normal;
  see 12.4 item 3). Whether `steer` is unit length is `[unread]`, so the strength is `[assumed]`.
- `mode+0x74` is called an "on-ground latch" in the unpublished notes (section 0 header) and a
  "speed latch" in docs/flight-contact.md (section 2) and flight-helpers section 0. Conflict,
  not settled here (see 12.5).

### 12.2 (A) The boundary: data

- Class and object: `FlightBoundary` (name hash fdc81a37) and its ellipsoid variant (+0x94 = 1, no
  name in names.tsv). Both ZONE objects (section 1.1). The zone field is `boundary` (hash 3e7e0252,
  names.tsv). `[data]`
- Other names in names.tsv that may be the zone-side object or the env test: `Flight_Boundary`
  (56228408), `_FLIGHTBOUNDARY` (fe8b270b), `connectionEnvInsideFlightBoundary` (a1062b8b). Which
  of them is a zone-object class and which is only a name is `[unread]`: no instance was decoded.
- Loader 011882b0 `[game]`: `iVar2 = data_field_by_hash(0x3e7e0252)`; `piVar1 = iVar2+4` (the field's
  pointer). If `piVar1 != 0`, `*piVar1 != 0` and `piVar1 + *piVar1 != 0`: call 0056f2d0 and
  00572110 (a 12-dword copy, source ECX, destination EAX; the decompile does not set EAX, so the
  destination is `[trace]`). Then `obj+0x5c = field[0]`, `obj+0x60 = field[1]` (dwords).
  So the 12-float boundary affine that the test reads (+0x2c..+0x58 and +0x64..+0x90) is written
  by 00572110 to a destination not yet traced: the zone's `boundary` data is `[assumed]` to be
  those 24 floats plus 2 dwords. A port needs the real numbers: decode one zone that holds a
  FlightBoundary with `tools\tf2_lxb.py dump` (not run: the 2 GB objects file is off limits).
- Test constants `[game]` (section 6): box half-size 1.01 (`00b6ae3c`), side tests on u and v,
  ceiling on s; no floor. Ellipsoid variant: `u*u+v*v <= 1` (decompile) and `s <= 1.01`.

### 12.3 (A) Pseudo-code: what turns a flyer back

Caller of the check 007e7830 (through the boundary vtable +8) is `[trace]`; it runs once per update
of the zone's list `[assumed]`, for the player's flyer only (section 3, step 1).

A1 `[game]` P = root of the character (0071a2e0; for a flyer its x,y are the root's and z is
   the lower of the root and a bottom point, 0071a460, see docs/flight-camera-exit.md).
A2 `[game]` L = A(P); (u, v, s) = B(L) (section 1.3). OUTSIDE if any of: `u > 1.01`, `u < -1.01`,
   `v > 1.01`, `v < -1.01`, `s > 1.01` (box). The local normal of the violated face is
   `(-1,0,0)`, `(+1,0,0)`, `(0,-1,0)`, `(0,+1,0)` or `(0,0,-1)` for the ceiling (section 6).
A3 `[game]` V = unit(R_B . n_local), R_B = FUN_00550930(B) basis (section 3, 8.a-d): the push-back
   direction, pointing back into the volume. N = unit(C - P), C = R_B translation: the direction
   from the root to the boundary's centre.
A4 `[game]` 0086ab90(mode, V, N, 1): `mode+0x55 = 1`; on a rising edge `mode+0x56 = 0`;
   `mode+0x5c..0x64 = V`; `mode+0x68..0x70 = N`; `mode+0x4c = 0`; then `mode+0x58 = +X` or `-X`,
   `X = *(mode+0x18) + 0x30` (a float of the tuning block; its unit is `[unread]`). The sign:
   if `V.z <= -0.95` (ceiling) by the sign of `mode+0x44`; otherwise by the sign of
   `V . (body lateral basis rotated by pi/2)` (the axis pairing is `[assumed]`, section 4 item 8).
A5 `[game]` INSIDE again (test passes): 0086ab90(mode, ., ., 0) clears `mode+0x55`. Outside the
   stick and the contact probes do not run (12.4 B0).
A6 `[game]` Effect on exit: the release and the punch are blocked while outside. connectionListFlyRelease
   and connectionListFlyToAirPunch require `EnvInsideFlightBoundary` (global_states; flight-states
   table), which is `mode+0x55 == 0` (section 8). So a flyer outside cannot leave flight by the
   release button. connectionFlyToHover and connectionFlyToSlam have no boundary test in the table.
   The boundary therefore steers the flyer back and locks the exit; it does not itself exit it.
   `[game]` (the table is from docs/flight-states.md; the connection lines were not re-read here).

What the boundary does NOT do `[game]`: no floor test, so a flyer below the volume is inside; no
  position is moved by the boundary code; the turn is the yaw ramp of 12.4 B3, not a teleport.

### 12.4 (B) Contact, bounce and wall stop: pseudo-code

Order in one FlyMode update (00864320, `[game]` l.1-163):
1. l.8-10: if `block+0xb8` (canDoStunts) and `00866a70` returns true (stunt / impact frame),
   return. Stunts and the impact pending path are in flight-stunts.md section 3.
2. l.12-29: the steer buttons and the turbo sound are read (not part of this ticket).
3. l.130-163 (the branch):
   - `mode+0x55 == 0` (not outside): if `block+0x84 == 0` (not helicopter): stick steering
     (00866ef0) or hold steering (008676c0), then the probe contact response 0086e020 (l.151).
     If helicopter: 00867f20 (l.154).
   - `mode+0x55 != 0` (outside): if `block+0x84 == 0` AND `block+0xb0 == 0` (not helicopter, not
     robot): bounce 00868b00 (l.159). Otherwise: wall stop 00868fb0 (l.162).
   So a robot or a helicopter flyer that leaves the boundary is wall-stopped, every other flyer bounced.
   `[game]` (l.157-158 are exact in the decompile).

B0 `[game]` Contact message (vtable slot 8 of the FlyMode, 0086ada0). Sender and message hash are
   `[trace]` (no name found). The message gives a vector at +0x8, +0xc, +0x10 and a kind at +0x3c.
   a. If `block+0xb0` (robot) != 0: return the block pointer. Nothing else is written. `[game]`
   b. `mode+0x94, +0x98, +0x9c` = the message vector, normalised if its squared length > 1e-8
      (`[game]`). The `+0x9c` value is the vector's third component: `[assumed]` the contact
      normal, z up (the closing value of docs/flight-stunts.md, which is this vector's z).
   c. If the other body's object (FUN_0070a460, `[trace]`) is non-null and it is a character
      object: rumble preset 11 (RUMBLE_VEHICLE_HIT_DESTRUCTABLE, docs/melee.md), return.
   d. Otherwise, with a character object: if kind (`+0x3c`) == 3, rumble preset 12
      (RUMBLE_VEHICLE_HIT_WALL). Then, ONLY when `mode+0x54` is set (a stunt is running, set by
      00866a70 for `mode+0x520 != 0`): the body velocity is read and written back (argument
      sources `[trace]`, so no change is established), `mode+0x9c > 0.1` zeroes control z
      (`+0x14`), `mode+0x9c < 0.1` zeroes control x and y (`+0xc`, `+0x10`), and `mode+0x514 = 1`
      (impact pending, consumed by 00866a70 section 3.2b). `[game]` (asm not checked).
   So outside a stunt, a contact changes NO flight state: it sets the normal (`+0x94..0x9c`) and the
   rumble. The impact pending flag exists only during a stunt. `[game]`

B1 `[game]` Ordinary flight contact, walls (not outside, not helicopter), 0086e020 (section 3): probes
   with `0 < d < 10` and `|n.z| < 0.1` are walls. The nearest wall gives `u = unit(M - O)` (mirror
   of the probe end about the wall normal) and the velocity gets `u * 500 * dt` lerped at rate 3
   (lerp state `mode+0x544..0x54c`); if `d < 10` the normal velocity is cut by 95 %
   (`v -= n*(0.95 * v.n)`). Two walls meeting (dot <= 0.1): the floor spring path, `vz` pushed by
   `s*500*dt` (s from the owner's test, `[trace]`), and horizontal velocity `*0.95`. `[game]`
   ("500" is the clamp `00cad46c`; the formula's own value never exceeds it, section 3 step 8.)
   The probes come from 0086b230 (35 m rays along the nose and two local points; section 1).

B2 `[game]` Bounce 00868b00 (outside, not helicopter, not robot). Arguments: `steer` = `&iStack_128`
   (3 floats), `dt`. `mode` is EDI.
   1. `n = (steer.y, -steer.x, 0)`; `k = -0.9 * (v . n)` with v = the body's velocity (+0xa0, +0xa4,
      +0xa8 of the body object `**(mode+4)+0x3c`'s body; `[game]` l.3-18). If the body's `+0x38 == 0`
      and `+0x34 == 0.0` (l.19): `v = v + k*n` (l.20-22). Net: the normal component times 0.1.
   2. `d = N . steer` with `N = mode+0x68..0x70` (decompile l.24-25). If `d > 0.95` or
      `mode+0x56 != 0`: `mode+0x44 = 0`, `mode+0x56 = 1`. Otherwise `mode+0x44` ramps to `mode+0x58`
      at rate `5*dt` (if `5*dt < 1`).
   3. Yaw: `mode+0x24 -= dt * mode+0x44`, wrapped to [-pi, pi). `mode+0x20` ramps toward the yaw
      target at `5*dt` (docs/flight-forces.md 00868b00 step 3-4). `[game]` constants; `[assumed]` the
      pairing of steer components (the decompile's `-0.0 - x` sign).
   4. Roll (`V` = `mode+0x5c..0x64`): if `V.z >= -0.95` the roll `mode+0x1c` decays at `5*dt`.
      Else (flyer above a ceiling, `V` points down): if body `+0x34 == 0.0` and `+0x38 == 0`, body
      velocity `+= V * 30 * dt` (the 30 m/s constant `0x868e59`), and roll lerps to -pi/6 at `2*dt`.
   `[game]` for the structure and constants; the sign convention is `[assumed]`.

B3 `[game]` Wall stop 00868fb0 (outside, helicopter or robot). `mode` in EAX, `out` = `steer`, `dt`.
   1. If `V.z >= -0.95`: `w` = horizontal forward (from body row 0 by 006de7e0; `w = (-f.y, 0, f.x)`
      after `*0.0` terms, `[assumed]` name); `c = N . w`. If `c >= 0.8` (constant `0x869135`):
      `out = (0, 1, 0)`. Else: `d = N - w` normalised (if |d| > 1e-8); the body velocity is reflected
      with `-0.9` on the normal and scaled `0.95` along `d` (per docs/flight-forces.md; the vector
      algebra of this branch is `[trace]`, `local_a4..`).
   2. Common tail: `00867f20(out, dt)` (helicopter control, `[game]` read in flight-helicopter.md).
   3. If `V.z < -0.95` (inverted / ceiling): a once-only UI tip and a virtual call on the input source
      (`[game]` branch; treated as non-physics).

B4 `[game]` Ground. Flight code has no floor boundary. Ground under a flyer is found by probe: 35 m
   straight down from `pos + 2*nose` (0086b230 base +0x370); its distance goes to `mode+0x78` (height
   over ground, less the body extent) and `mode+0x3c4`. `[game]` The ground's bounce is in the
   physics solver, not in the flight code (notes: 007891e0 says "body restitution in the solver"):
   `[unread]`, the Lux contact solver. A flyer hitting the ground therefore takes the solver's
   bounce and then the contact message of B0 (outside a stunt: only the normal and the rumble).
   The floor path of 0086e020 is a lift, not a bounce (B1).

B5 `[game]` Forced exit from flight. NOT FOUND in the read code. The flight code never changes the
   state machine. The exits from stateFly are: ReleaseFly (boundary-gated, A6), the punch (gated),
   the hover and the slam (buttons), the stunt connections, and connectionListNoInputAndAnimDone
   (`ExitB` -> stateFall, after an exit anim with no input). The contact message type
   connectionMessageHit (msg b8590f58, global_states.txt 2714) is NOT referenced by any list in
   global_states.txt, so it does not force an exit in the global data `[data]`; a character pack
   could still use it `[unread]`.

### 12.5 Port recipe, short

Per update, for a flyer:
1. Read the boundary: P = root; L = A(P); (u,v,s) = B(L); outside test (A2). [needs zone data, 12.2]
2. If outside and it changed: V, N, sign (A3, A4). If inside: clear (A5).
3. If outside: steer-back (B2 or B3 by block+0x84/+0xb0). Else: stick or hold steering, then the
   wall probes (B1). Helicopter: its own steering.
4. Contact message: normal into mode+0x94..0x9c, rumble; pending impact only during a stunt (B0).
5. No state change for a contact. Exits by the state machine only; release blocked while outside (A6).

Inputs a port needs: the boundary affine(s) and the variant flag (zone data, unread); the body basis
(FUN_00550930) and the root; the body velocity and yaw/roll; the stick vector; ray casts (35 m) with
normal and distance; the contact message (normal, kind); tuning fields `tun+0x30` (X), `+0x60`
(yaw rate, deg/s), and the block flags `+0x84`, `+0xb0`, `+0xb8`.

### 12.5b Decoded fields (porter, 2026-10-12)

- `[data]` The zone field `boundary` is a 12-float 4x3 row-vector placement (rows 0..2 basis, row 3
  translation). Real example: us_test zone FlightBoundary, `work\survey\levels\dump_us_test_zone_14.txt`
  line 180611: `[500,0,0 | 0,500,0 | 0,0,70 | 0,0,0]`, attached to object `obj6` (building BG_Bldgs_F_17
  at (156.04, -143.19, 68.29)). `[data]`
- `[game 011882b0 + 0052ff10 via 0056f2d0]` the loader inverts that block and copies the inverse to B at
  `obj+0x2c` (12 floats, `lea edx,[ebx+0x2c]` then 00572110). So B's u row is `(B.2c, B.38, B.44)`, offset `B.50`.
- `[game]` the loader writes only B and the two dwords `obj+0x5c/0x60` (from the field record, meaning
  unread). A (`obj+0x64..0x90`) is NOT written by 011882b0: its source is untraced. `[assumed]` A = inverse
  of the owning object's placement.
  SETTLED 2026-10-16 (flight session): A is written by 007e75c0, the class's vtable slot 4 (00b60918), the
  once-only activate of thing classes. `[game]` M = the owner node's (`obj+0xc`) world matrix
  (`node+0x10..+0x3c`) when the instance index `obj+0x14` is 0, else the owner's instance matrix
  (005509d0) composed (0052fbf0); A = inverse(M) (0052ff10). It also registers a tick (005785d0, unread)
  when flag `+0x16` bit 2 is clear. So A is the owner's placement inverted, taken once at activation.
  Still `[assumed]` in the port: that `level_things::each_thing`'s `world` is that M (the composed
  instance branch included).
- `[game]` FlightBoundaryEllipsoid differs only by class: the data has no flag; `+0x94` comes from the ctor.
- Port: `flybound::Boundary::read(block, a, ellipsoid)`; `flight::Fly::step_in_bounded` uses it.

### 12.6 Not found / open

1. The zone instances of FlightBoundary: the us_test one is decoded (12.5b); the shanghai_01 one and
   the owner of A are open. The destination of 00572110 is B+0x2c (traced now). Which pack holds them: the
   us_test zone (`us_test\zone.str`, chunk 14 per the dump name `dump_us_test_zone_14`) `[assumed]` (the
   name's number is taken as the chunk id; not re-checked with tf2_lxb).
2. The caller of the boundary vtable slot +8 (007e7830) and its rate: `[trace]`.
3. The steer vector (`iStack_128`, `fStack_110`): line 101 of 00864320 writes `iStack_128` from
   `**(mode+0x18)` (a double dereference, likely a decompiler artefact); the other two components
   come from 0086d5c0 (stick). Whether the outside branch's steer is the stick or something else: `[trace]`.
   This matters because 00868b00 builds its bounce normal from it.
4. The message name and sender for slot 8 (0086ada0), and FUN_0070a460 (the other body's object): `[trace]`.
5. Meaning of mode+0x74: on-ground latch (flight-helpers header) or speed latch (flight-contact
   section 2). `[unread]` which; the probe 0086b230 writes it.
6. Floor probe in 0086e020 casts along -y (`0086e382`), while the game is z-up with y forward:
   the axis of the floor probe is `[unresolved]`. Needs a recording or a test.
7. The wall-stop vector algebra (00868fb0 else branch) and 00867f20: `[trace]`.
8. The unit of `X = tun+0x30`, the 30 m/s push and the -pi/6 roll: `[assumed]` from the constants.
9. The steer vector's length: whether the bounce strength scales with the stick (`[unread]`).
10. The bounce's and the wall stop's sign conventions: `[assumed]`, to be tested with a steer and a
    velocity before porting (docs/flight-forces.md says the same).
11. The solver's restitution for ground and wall hits (Lux solver, not ported): `[unread]`.

## Ledger

Recorded with `fn.py note`: 007e7830 (rewritten), 007e7b60, 0086ab90, 00adbf70, 00adbfd0,
007e7780, 01196e50, 011882b0 (rewritten), 007e7540, 007e7810. The env case 0084d34f sits in
0084cee0, whose ledger name is for the 370a5fba branch; it is NOT re-noted (the name would be
wrong). See section 8.

## 12.7 Wall stop 00868fb0 (read)

Reader ticket 2026-10-12. Read from the decompile (`fn.py 00868fb0 --full`) and the disassembly
(`work\raw_disasm.py` 00868fb0-00869400). Where the two disagree the disassembly wins. This section
SUPERSEDES the "-0.9 reflection" wording of 12.4 B3 and section 9: there is no velocity reflection.

Calling form `[game]`: EAX = FlyMode (`mode`). Stack args (`ret 8`): param_1 (forwarded unchanged to
00867f20) and param_2 = dt. Mode fields read: `mode+0x64` (V.z), `mode+0x68`, `+0x6c`, `+0x70` (N).
The function writes NO mode field. Its outputs are the body velocity and the arguments to 00867f20.

Constants `[game]` (all read in the disassembly at the address given):
- -0.95 at 00b6b194: the inverted gate (`comiss` at 00868fe6-00868fee, `jbe 0x86906a`).
- 1.0 at 00b38834: out.y in branch A; the numerator of 1/|d|.
- 0.8 at 00b6a3f0: the alignment gate (00869135-0086913d).
- 1e-8 at 00b345d8: the zero guard on |d| (0086919f).
- -0.9 at 00b6b19c: the threshold for choosing the scalar a (008692bd-008692c8).
- 0.95 at 00b6a8e8: the velocity damp (00869324-0086932c).
- -1.0 at 00b2d970: the sign in the inverted branch only (00869039).

Steps, for the main path V.z >= -0.95 (the branch at 00868fe0-00868ff2, which jumps to 0086906a):

1. `[game]` 0086906a-0086908e: the owner's body (owner+0x3c) gets a reference taken (+4 on its count;
   release at the end). Owner = `[mode+4]`.
2. `[game]` 00869091-008690e9: call body_world_matrix 006de7e0 into a 12-float temp. Read ONLY its
   first three floats: r0 = (r0x, r0y, r0z) = temp[0..2]. (006de7e0 copies holder+0x108.., or the
   holder+0x1fc object's +0x108 when that pointer is nonzero.)
3. `[game]` 008690ef-00869131: horizontal vector `w = (-r0y, r0x, 0)`. Components: w.x = -r0y
   (008690ef-00869101 region: `f2*0 - f1`), w.y = r0x (`f0 - f2*0`), w.z = 0 (`f1*0 - f0*0`).
   The `*0.0` terms are kept by the compiler and evaluate to 0 for finite values. `[game]`
4. `[game]` c = N . w = N.x*w.x + N.y*w.y + N.z*w.z (mode+0x68 x w.x, +0x6c x w.y, +0x70 x w.z).
   [assumed] w is the body's forward direction when the body is level: this needs row 0 to be the
   body's RIGHT axis. Ledger 006de7e0 says row 1 (elements 3-5) is forward; that fits (right rotated
   +90 degrees about z is forward). Not settled by a recording; when the body pitches or rolls, w is
   only the horizontal projection of the right axis, and it is not normalised.

5a. `[game]` If c >= 0.8 (00869135-0086913d, `jbe 0x8693af`): out = (0, 1, 0), a = 0. Then go to 9.
   No velocity change in this branch (the damp at 00869324 is on the fall-through only).
5b. `[game]` Else (c < 0.8):
   a. d = N - w (per component: x = N.x - w.x, y = N.y - w.y, z = N.z - w.z), |d| by sqrt (00409a70).
   b. `[game]` if |d| > 1e-8: dhat = d / |d|. Else dhat stays the raw d (about zero). `[assumed]` the
      result then is about zero, so a is about zero (not traced further).
   c. `[game]` the holder's inverse 3x3 M (12 floats from 005e3820, the same inverse the collision
      tests use per the ledger; elements used: 0,1,3,4,6,7). Row-vector convention `[assumed]` from the
      column pairing: xc = dhat.x*M0 + dhat.y*M3 + dhat.z*M6 ; yc = dhat.x*M1 + dhat.y*M4 + dhat.z*M7.
      So xc, yc are the x and y components of dhat in the holder's body frame. `[game]` for the
      arithmetic (00869203-0086928d; the call to 5e3820 at 0086922f is partly lost, see gaps).
   d. `[game]` a = yc if -0.9 > yc (i.e. yc < -0.9); else a = xc (008692bd-008692de: comiss at
      008692bd, `ja` skips the load of xc).
   e. `[game]` out = (0, 0, 0) (zeroed at the top and not written here).
   f. `[game]` body velocity v (body_linear_velocity 00556c00: +0xa0/+0xa4/+0xa8 of the body, or zero)
      is set to v = 0.95 * v (00869324-0086934b) through physics_body_set_velocity 00557000. Not
      dt-scaled. This is the ONLY velocity write in the function: there is no reflection (the -0.9
      only appears as the threshold in 5b.d). `[game]`
6. `[game] 008693bd-008693d9`: the call 00867f20 (flight_control_step_helicopter): EAX = &(a, b)
   where b = 0 (zeroed at the top, never written on this path); stack = (param_1, &out, dt).
   00867f20 is NOT read here: what it does with out, a and dt is `[unread]`.
7. `[game]` release of the body reference taken at step 1 (refcount; FUN_00532c70 when it reaches zero).

Inverted branch, V.z < -0.95 (00868ff4-00869065, not traced in full): `[trace]`. The owner's
controller is tested with controller_is_ai (00854df0); if the controller is not AI a once-only tip is
set up (0750660, a8b36e); `[esp+0x20]` is +1, or -1 when byte 00dad8ae is set. Then a virtual call on
the owner's input source (vtable at owner+0x20, slot +0x28, argument &(the local vector)) returns a
float pointer whose value becomes a. Then the same call 00867f20 (step 6), with no velocity damp
(branch end at 008693bd). `[trace]` for the meaning of the virtual call. (This is the path the ledger
called "inverted gate ... virtual call".)

Gaps in the disassembly (not closed):
- 00869234-0086923b (8 bytes after the call at 0086922f, which is holder_inverse_matrix by the
  decompile): the decoder lost sync; the bytes are unseen. `[trace]`. Expected: arguments or a
  register setup for the matrix read at 0086923c.
- 00869304-00869323: the release code of step 7 and the call to body_linear_velocity (at 008692ff) are
  mis-decoded by the disassembler; the decompile shows the same order. `[game]` (decompile) `[trace]`
  (disassembly).

Behaviour, plainly `[game]` for the arithmetic, `[assumed]` for the meaning:
A helicopter or a robot that is outside the flight boundary does not reflect off the wall. If its
nose points within about 37 degrees of the direction to the boundary centre (c >= 0.8), it asks the
helicopter steering to hold straight ahead (out = (0,1,0)) and keeps its velocity. Otherwise it bleeds
5 percent of its velocity per call (0.95, not dt-scaled, so the rate depends on how often this runs,
`[trace]`), and passes the helicopter steering a single scalar `a` (the body-frame sideways component
of the unit vector from the wall-side direction to the centre, or its forward component when that is
below -0.9) with zero out, so the helicopter steering turns the nose. "Stops dead" is wrong. Whether
the nose turns toward the centre or away depends on the sign conventions `[assumed]` (to test with a
steer and a velocity, as in 12.6 item 10).

Port inputs (in the terms of flybound.rs `Steerback`):
- Steerback is NOT the wall stop's state. This function writes none of `outside, armed, push, normal,
  target, rate, yaw, yaw_ramp, roll`. It READS `push.z` (= V.z, the `mode+0x64` field) and `normal`
  (= N, `mode+0x68..0x70`). A port needs a separate `wall_stop(...)` returning (out: Vec3, a: f32)
  and then the helicopter step with (a, out, dt).
- Other inputs: the holder's body row 0 (r0x, r0y, r0z; with the holder+0x1fc indirection); the
  holder's inverse 3x3 (elements 0,1,3,4,6,7); the body linear velocity (read and written back); dt.
- Routing: `uses_bounce(helicopter, robot)` (flybound.rs line 224) must send helicopter and robot to
  wall stop, jets to the bounce (per 00864320 l.157-162). Check that function when wiring.

Not read / open:
1. 00867f20 (helicopter steering) with its (a, b) pair in EAX, out, and dt: `[unread]` here.
2. The virtual call of the inverted branch, and the controller tip: `[trace]`.
3. The meaning of b (= [esp+0x20]): zero on the main path, +1 or -1 in the inverted path only.
4. The caller's rate (how often this runs per second): `[trace]`.
5. The gaps above.
6. The row convention of the holder's matrix (row 0 = right): `[assumed]`, settle with a helicopter
   recording (heading vs row 0 and row 1).
7. The ledger entry for 00868fb0 said "-0.9 reflection" and this section corrects it; the same
   wording in 12.4 B3 and section 9 is wrong for the same reason.

### 12.8 Top exit (debugged, 2026-10-12)

Evidence: the unpublished notes "Third check" item 5. Test
`flight::step_tests::a_jet_climbing_out_through_the_top_is_brought_back_inside` (ignored, see below).
Trace in the port (Starscream, owner placement, stick -1 for 4 s): the jet leaves at 4.10 s, V = (0,0,-1),
N tends to (0,0,-1), the nose stays at pitch 58.8 deg, height 1928 m at 44.6 s, no re-entry. The
mechanism: outside, the thrust (50 * dt along the nose, every update, before steering) beats the push
(30 * dt along V) whenever sin(pitch) > 30/50, i.e. pitch above 36.9 deg. Nothing lowers the nose while
outside, because the stick steering 00866ef0 is skipped (00864320 l.157-163). `[game]` for each piece;
the escape as a whole is what the game as read gives too, so the test stays ignored.
Changed: the bounce's X is `tun+0x30` (quickTurnSpeed), not `tun+0x28` (turnSpeed): flight.rs
`boundary_contact`, tagged `[game 0086ab90 step 4]`. The sideways test still passes.
Settled by 12.9 (mode+0x20 is roll, mode+0x1c is pitch) and fixed; in game (2026-10-12, Sixth check in
the unpublished notes): a jet leaving through the top comes back inside (left at about 5 s, re-entered by 8 s, level at 132 m by 20 s).

12.9 mode+0x1c and mode+0x20 (settled, reader pass 2026-10-12)
Evidence, all from the code, offsets off the FlyMode object (mode):
- matrix_to_euler_asin 00570250 writes EDI[0] = asin(col1.z) to mode+0x1c (00864320 l.~130),
  EDI[1] = atan2(-col0.z, col2.z) to mode+0x20, EDI[2] = atan2(-col1.x, col1.y) to mode+0x24.
  Disasm 00570250-00570390: the gimbal test (0.9999) is on |col1.z| (esi+0x14..0x20 region), so the
  asin is the forward column's z. [game]
- euler_matrix 00570470 builds Rz(mode+0x24) * Rx(mode+0x1c) * Ry(mode+0x20). For that product the
  forward column (y) is (0, cos x, sin x) with x = +0x1c, so +0x1c is the rotation about the
  lateral axis: PITCH, nose up for positive. The Ry angle is the rotation about the nose: ROLL. [game
  algebra on 00570250/00570470; the sign of a positive angle assumes standard right-hand Rx: assumed]
- Sound 00864b66 pitch = +0x1c / (tun+0x58 or tun+0x5c, deg) and 00864ba1 roll = +0x20 / (tun+0x64,
  deg) (flight-sound.md). Sound pitch divides by a pitch limit, sound roll by a roll limit. [game]
- Stunt 00866a70 rolls mode+0x20 (flight-forces.md "00866a70"), and the stunt's roll test in
  flight.rs is on the same field. [game]
Verdict: +0x1c = PITCH (rotation about the lateral x axis, nose up/down); +0x20 = ROLL (rotation about
the nose; banks the wings). mode+0x24 = HEADING (yaw about the vertical z). The flight-forces.md
lines that call +0x20 the yaw-rate target and +0x1c the roll are wrong (corrected with a pointer).
The yaw RATE is mode+0x44 (00868b00 writes mode+0x24 -= dt*mode+0x44). [game]

Bounce 00868b00, every write, in order (mode+0x1c = pitch, +0x20 = roll):
1. Velocity push-back: if body+0x38 == 0 and body+0x34 == 0.0: velocity += k * n where the
   damping k = -0.9*(steer . n) (no dt). [game] The push vector V = (mode+0x5c, +0x60, +0x64).
2. Gate: d = (mode+0x68,+0x6c,+0x70) . steer. If d > 0.95 or mode+0x56 != 0: mode+0x44 = 0 and
   mode+0x56 = 1 (the yaw rate is zeroed and the flag is latched). Else mode+0x44 ramps to
   mode+0x58 (X = tun+0x30) at 5/s (lerp if 5*dt < 1, else set). [game]
3. Yaw integral: mode+0x24 -= dt * mode+0x44, then wrap to [-pi, pi). [game]
4. ROLL target: mode+0x20 ramps to target at 5/s (lerp if 5*dt < 1, else set), where target = 0 when
   the step-2 test latched, else sign(mode+0x44) * tun+0x60 deg->rad (0.0174533). This is the bank
   into the turn: a yaw rate one way gives a roll target of the same sign. [game]
5. Gate on V.z = mode+0x64:
   a) V.z > -0.95 (or equal): PITCH mode+0x1c decays to 0 at 5/s (lerp if 5*dt < 1, else 0). [game
      0x868e44 test; 00868f...: the uVar11 = 0 path]
   b) V.z < -0.95 (leaving through the TOP): body velocity += V * 30 * dt, same guard as step 1
      (`30.0` at 00b34570). Then PITCH mode+0x1c ramps to -0.5235988 (-pi/6, constant 00b6b198 =
      0xbf060a92 as written at 868f3b-868f63) at 2/s (lerp if 2*dt < 1, else set). [game]
   The -pi/6 goes to mode+0x1c (PITCH), not to roll. The note "rolls to -pi/6" was wrong.
Side exit (V.z > -0.95): the nose is levelled (pitch to 0), the bank follows the yaw rate (mode+0x20 to
+-tun+0x60), and the heading integrates (mode+0x24). Top exit (V.z < -0.95): the nose is pushed to
30 deg DOWN at 2/s and the velocity gets 30*dt along V; the roll goes to 0 by step 4 (the flag path)
or to the yaw-rate target.

Step 3 (nothing else while outside): 00864320 skips the stick steering 00866ef0, the hold 008676c0
and the contacts 0086e020 (they sit in the else of mode+0x55). Only two things touch mode+0x1c while
outside: the euler re-read from the body matrix (matrix_to_euler at l.~130, before the branch, which
only reads it back), and the bounce above. Nothing moves the pitch toward zero or a limit other than
the bounce's own step 5a (side exit) and 5b (top exit: toward -30 deg). The 0x1c write in 00864320
is only at l.~130 (the euler re-read). [game]. (A wall stop 00868fb0 is used instead when tun+0xb0 is
set, and it is not read here.)

Step 4, the plain answer: a jet that climbs out through the top at a steep angle has its nose PUSHED
DOWN, not held up: while its push vector points down (V.z < -0.95) the game drives the pitch to
-30 deg at 2/s and adds 30*dt of velocity along V each step, and it does not bring the pitch back up
while it stays above the ceiling. Thrust is along the nose, so a nose at -30 deg drives it back down
through the boundary; the escape is not the game's. [game for the writes; the eventual re-entry is
[assumed] from the thrust and push, not simulated.]

Port (what differs, read 2026-10-12): core/src/flybound.rs Steerback::bounce ports the same formulas
but stores the results in the wrong fields: its `roll` (-pi/6 target, 2/s ramp, 5/s decay) is the
game's mode+0x1c (PITCH), and its `yaw_ramp` (+-tun+0x60 target) is the game's mode+0x20 (ROLL). In
core/src/flight.rs step_in_bounded (l.502-508) `self.roll = self.steer.roll` puts the pitch into the
roll axis (Quat Ry in `orientation`, flight.rs l.370), and `yaw_ramp` is not used anywhere. So the
port's jet keeps its own stick pitch, is rolled to -30 deg, and never has its nose pushed down.
Tests in flybound.rs (`pointing_down_pushes_the_body_and_rolls_to_minus_thirty_degrees`) enshrine the
wrong field. Fix (for the port author, not done here): write `steer.roll` to the pitch, `steer.yaw_ramp`
to the roll, and make the stick pitch yield to it while outside.

## 12.10 Helicopter step 00867f20 (2026-10-12)

Already read and ported: `docs/flight-helicopter.md` (sections 1 to 4) and `steer_helicopter` in
core/src/flight.rs. The wall stop's call maps onto it as follows. `[game]` unless marked.

1. Arguments: arg1 = the nose pointer (the wall stop's param_1, forwarded), arg2 = `Vc` (three floats:
   Q0 = Vc0, Q1 = Vc1, Vc2 = 0), arg3 = dt. EAX = the linear stick pair (L, P), not squared.
2. Wall stop call (008693bd-008693d9): EAX = &(a, 0), so L = a and P = 0; Vc = `out`, so Q0 = out.x and
   Q1 = out.y, with out = (0,1,0) when aligned (c >= 0.8) and 0 otherwise.
3. So the port call is `steer_helicopter(input, tun, out.x, out.y, a, 0, turbo=false, ground, dt)`.
   The turn (Y, step 9) uses L = a; the lift (N4) uses P = 0; the forward push (N3) uses Q1 = out.y.
4. Boost is off outside (mode+0x55 != 0 gates it), so the turbo branches do not run here. [game] for the gate
   (00867f77-00867fb0), [assumed] that the same flag reaches the velocity cap in `finish`.
5. Not in the port: the gate G (mode+0xc, 0x10, 0x14 all zero, else contact heading) and the inverted branch.
6. [assumed] The holder's inverse 3x3 is the body's R^T and its row 0 is R*X. The game's holder is not traced.
   Port: `wall_stop(steer.normal, steer.push.z, R*X, R^T, vel, dt)` in `step_in_bounded`.
7. Test (flight.rs `a_helicopter_flying_out_sideways_is_slowed_turned_toward_the_centre_and_comes_back`):
   at 10 m/s out of the +x face, one update gives vel.x = 10 * 0.95 * 0.95 (wall stop, then the sideways
   damping D1), the nose turns toward the centre, and the helicopter is back inside at 2.08 s.

## 12.11 Wall stop, top exit (read)

Reader ticket 2026-10-12. Scope: the inverted branch of 00868fb0 (V.z < -0.95, outside through
the TOP) and the caller 00864320. Checked in disassembly; the decompile agrees on the order.

Steps, inverted branch `[game]` (raw_disasm 00868fe0-00869070; EDI = mode, `[edi+0x64]` = V.z):
1. 00868fe6-00868fee: gate, `comiss` of -0.95 (const 00b6b194) against `[mode+0x64]`; `jbe 0x86906a`
   skips to the normal branch when V.z >= -0.95. The inverted path falls through at 00868ff4.
2. 00868ff4-00868ffc: `b = [esp+0x20] = 1.0` (const 00b38834).
3. 00869002-00869009: `call 00854df0` (controller_is_ai). If it returns nonzero (AI) it jumps to
   00869047, skipping steps 4 and 5 entirely. [game]
4. Not AI only, 0086900b-0086902d: `once` flag bit 0 of byte 00db0414; if not yet set, set it and
   call 00750660 (options_static_init) and `push 0xae74f0; call 00a8b36e` (the one-time UI tip; the
   tip's text is not read). [game]
5. Not AI only, 00869030-00869041: if byte `[00dad8ae]` (the invert-pitch option, as in 00867f20
   step I) is nonzero, `b = [esp+0x20] = -1.0` (const 00b2d970). [game]
6. 00869047-00869059: `eax = [mode+4]` (owner); `ecx = [eax+0x20]` (owner's input source); `ecx =
   [ecx]` (its vtable); `edx = [ecx+0x28]` (slot +0x28). Call it with `lea eax,[esp+0x28]` pushed
   (a scratch vector). The returned pointer is in EAX. [game] Owner+0x20 and slot +0x28 are the
   same object and slot that 00864320 l.115-118 reads, and flight-helpers.md lists +0x28 as "point".
   The function behind slot +0x28 is `[unread]`; its vtable is not identified (the owner input
   object is the `lux::Controller`-family object; no vtable row matched).
7. 0086905b-0086905f: `a = [eax]` (the FIRST float of the returned point, raw). Stored to
   `[esp+0x1c]`. No 0086d5c0 deadzone call on this path. [game]
8. 00869065: `jmp 0x8693bd`. This skips the velocity damp at 00869324 (the 0.95 body-velocity
   scale is on the normal path only), so the body velocity is NOT changed. [game]
9. 008693bd-008693d9: `EAX = &[esp+0x1c]` (the pair (a, b)), the stack carries the nose
   pointer, `&out` and dt; `call 00867f20` (steer_helicopter). `out` = zero on this path (it is
   zeroed at the top and never written here). [game]
10. Release of the body reference, as on the normal path (step 7 of 12.7). [game]

Constants: -0.95 (00b6b194), 1.0 (00b38834), -1.0 (00b2d970). The gap 00869234-0086923b that
12.7 calls "the inverted path" is NOT on the inverted path: it lies in the NORMAL branch, after the
call to 005e3820 (holder inverse) at 0086922f (5c). The inverted path does not call 005e3820. The
disassembler loses sync from 0086921c; the bytes at 00869234 onward (test esi,esi; movss xmm4,[esp+0x18]
at 00869236) are readable from 00869234 on. [game]

What 00867f20 does with the pair, from docs/flight-helicopter.md (already read, `[game]`):
- Gate G (00867f20-00867f5b): mode+0xc, +0x10, +0x14 all zero, else flight_contact_heading. For a
  helicopter those three words are never written: 00866a70 returns at once when tun+0x84 != 0, so
  G passes. [game] (flight-common-step.md "The control vector")
- Step I (00868017-00868062): `if not controller_is_ai() and option [00dad8ae]: P = -P`. So the
  inverted path's P is +1 in both settings (b=+1 unflipped; b=-1 flipped to +1), and it is also +1
  for AI (no flip, b=+1). Net P = +1 in every case. [game for the flip, trace for the net]
- Lift N4 (0086869b-008686b8): `Vb2 += -lift * P * dt`, lift = `[tun+0x10]` (ecx = [mode+0x18];
  `movss xmm1,[ecx+0x10]`). With P = +1: the body's vertical velocity drops by lift*dt each update.
  So the helicopter is pushed DOWN at lift per second. [game]
- Horizontal: Vc = 0, so D1 scales V0 and V1 by 0.95 per update (no boost). D2 does not run
  (|P| = 1). [game]

PLAIN ANSWER: a helicopter above the ceiling is brought down by its own lift: the wall stop
passes P = +1 (full lift stick, downward) with a = the raw first float of the input point and
out = 0, so the helicopter step lowers the body's vertical velocity by tun+0x10 * dt every
update (no reflection, no 0.95 on V), and the sideways velocity bleeds 0.95 per update.
This CONTRADICTS the game capture (a helicopter sat at about 146 m for 20 s above the box top
at 138 m). The read gives no ceiling or height cap: the flight-helicopter notes have none, and
the lift tuning for the helicopter is not in flying-forms.md (its table is jets). So either the
helicopter's tun+0x10 is zero or near zero (`[assumed]`, check the helicopter's tuning block), or
the capture's hover came from something not read (the outside flag mode+0x55 clearing, or V.z
never reaching -0.95 when the nose is level) `[trace]`. Settle with the tuning value and a
recording of V.z and V2 while the helicopter is above the top.

Caller, 00864320 (after the stick and Euler reads at l.100-129, which run for every flyer):
- 130 `if mode+0x55 == 0`: the INSIDE block. The thrust block (tun+4 / tun+0xc, read at 0086461d
  for the jet, skipped for the helicopter at 008645xx) and the stick steer (00866ef0), the impact
  state (00866a70), contact (0086e020) all run only here. [game]
- 157-163 `else` (outside): `if tun+0x84 == 0 && tun+0xb0 == 0` then flight_surface_bounce (00868b00,
  jets), else flight_wall_stop (00868fb0, helicopters and robots). [game]
- So for every flyer the outside path has NO thrust, NO stick steer and NO impact step. The
  30 * dt push along V is inside 00868b00 (jets only), not before the choice. [game] (l.157-163)
  (the decompile lines l.14-30 and 100-163 are the reference; docs/flight-boundary.md 12.8.)

Port inputs for `wall_stop` and `steer_helicopter`:
- `wall_stop`: `V.z` (push.z), N, R's row 0 (for the normal branch only), the inverse 3x3 (normal
  branch only), body velocity, dt, and the input point's first component `a` supplied by the caller
  (the owner's input, slot +0x28). For the inverted branch it returns (out = 0, a = input.x, b = +1)
  and does NOT touch velocity.
- `steer_helicopter(input, tun, out.x, out.y, L = a, P = b, ...)` with b = +1 in the inverted
  branch; P is flipped by step I when not AI and the option is set. Port the flip, and the inverted
  b = +1 (option off) or -1 (option on), so the net is +1.

Not read / open:
1. The function behind owner-input slot +0x28 (the "point"): `[unread]`; which device it is.
2. The tip function 00a8b36e (text) and option byte 00dad8ae's name: `[trace]`.
3. The helicopter's lift value (tun+0x10 for the helicopter): not in flying-forms.md; `[unread]`.
4. The capture contradiction (146 m hover) above: `[trace]`.
5. The disassembler gap 00869234: it is on the normal branch, not this one (see above).
