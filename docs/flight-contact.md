# Flight form (FlyMode): contact with the world, port recipe

Status: read from the decompile and checked in disassembly (`work\raw_disasm.py`). Ticket scope:
the probes (0086b230), the contact response (0086e020), the contact branch of 00864320, and
who sets the contact flag mode+0x55 (0086ab90, 007e7830). Tags: `[game]` read in code (address
given), `[data]`, `[assumed]`, `[trace]` (open, needs a recording or a callee read).

The port today flies through everything because none of this is written. Summary of what the
game does, in order, is section 5.

## Manager's corrections (2026-10-12, checked by me)

- Section 2 is WRONG in its heading: `mode+0x78` is a HEIGHT, not a speed. The four probes
  are identical blocks 0xc0 apart: segment at base-0x60, hit normal at base-0x24, handle at
  base+0x40, hit distance at base+0x54 (probe 1: base 0x130, segment 0xd0, normal 0x10c,
  handle 0x170, distance 0x184; probes 2 and 3 the same at 0x1f0 and 0x2b0). The block at
  base 0x370 has its segment at 0x310..0x328 (the 35 m drop from pos + 2*nose), its handle
  at 0x3b0, its normal at 0x34c..0x354 and its distance at 0x3c4. So `mode+0x3c4` is the
  drop ray's hit distance, `mode+0x78` the height over the ground less the body's extent
  (the sine terms), `|mode+0x354| > 0.5` the test that the ground is not steep, and
  `mode+0x74` the on-ground latch. `[game]` by the layout; this settles the port's
  assumption "mode+0x78 is the height over the ground".
- Section 3 step 8: the strength is `max((1 - 0.1 d) * 100 * (1 - c), 500)`, confirmed
  (0086e99b-0086e9dd: `comiss k, 500; jae keep; else k = 500`). The push is
  `unit(M - O) * k * dt`, eased at 3 a second into mode+0x544 and added to the velocity
  (0086ea8b-0086eb48); then 95% of the velocity along the normal is taken off
  (0086eb56-0086ec12). Confirmed. What `M - O` is (the ray's direction laid along the
  wall) is the reader's reading of the decompile and stays `[assumed]`.
- The "-y" of the corner path is most likely the writer's stack order and the axis is z;
  not traced, `[assumed]`.
- Ported: `core\src\flight.rs` `Fly::meet_walls`, `Fly::wall_rays`.

## 0. Frame and layouts used below

- Matrix layout `[game]` (checked in 0086e020 and 0086b230): a 3x4 affine stored as 12 floats.
  Basis vectors: x-basis `p[0..2]`, y-basis `p[3..5]` (the forward axis: thrust and the probe
  ray use it), z-basis `p[6..8]`, translation `p[9..11]`. World point of local `(a,b,c)` is
  `a*x + b*y + c*z + t` (0086e020 l.~108-115: `pfVar9[3]*fy + pfVar9[6]*fz + pfVar9[0]*fx + pfVar9[9]`).
- The probe code (0086b230) reads the SAME affine from `owner+0x2c` at offsets: x-basis `+0x10`,
  y-basis (forward) `+0x1c`, z-basis `+0x28`, translation `+0x34,+0x38,+0x3c`. `[game]`
  (0086b230 disasm: `0086b2xx` readbacks use `[iVar8+0x1c]`, `[+0x28]`, `[+0x10]`, `[+0x34]`).
- Vertical axis: NOT SETTLED. Two readings in the code disagree (section 6, item V):
  the 35 m drop of 0086b230 and the wall test `|n.z|` treat z as up; the floor probe of the
  spring path (0086e020, `0x86e35b..0x86e3ae`) lifts `t.y` by 5 and casts along `-y`.
  Port with z-up as the brief says, and do not trust the floor-path axis until it is traced.

## 1. Probes: 0086b230 (called from 00864320 at its start, line 10)

Order inside one call `[game]`:

1. Readback of the PREVIOUS submission (gate `mode+0x130`, `0086b252-0086b25a`). A handle
   `mode+0x170` is copied (`refhandle_copy@009003e0`); if it has a hit (`local_2e8 != 0`)
   and the hit is not of the excluded types (`FUN_0070a460`, `FUN_007c8be0`, both unread),
   the copy gives `mode+0xc4 = mode+0x184` (distance) and `mode+0xa0..a8 = mode+0x10c..114`
   (normal). No hit: `mode+0xc4 = FLT_MAX` (`0x7f7fffff`). `[game]`
   Consequence: the answer is one update late. The submit below runs after the readback in
   the same call, so the value read at update N was submitted at N-1. `[game]` (order) plus
   `[assumed]` that the world resolves the queue between updates.
2. Probe 1 submit (gate `mode+0x131 == 0 or mode+0x130 != 0`, `0086b3xx`): origin
   `O1 = t` (`+0x34,+0x38,+0x3c`), direction `D1 = y-basis` (`+0x1c,+0x20,+0x24`, forward),
   segment `O1 .. O1 + 35*D1` stored at `mode+0xd0..0xec` (origin, then end, w=1), then
   `probe_submit@006e0a30`. `[game]` (0086b230 decompile: `*(param+0xe0) = row1.x*35 + t.x`).
   Length 35 `[game]` (constant `35.0`, `00b6b3b4`).
3. Probe 2 readback (gate `mode+0x1f0`): hit distance `mode+0xc8 = mode+0x244`, normal
   `mode+0xac..b4 = mode+0x1cc..1d4`. `[game]` Probe 2 submit (gate `mode+0x1f1 == 0 or 0x1f0 != 0`):
   origin `O2 = L2 transformed` with `L2 = (x=mode+0x52c, y=mode+0x530, z=mode+0x534)` in the
   basis of 0x0 above; direction `D2 = y-basis`; segment at `mode+0x190..0x1ac`. `[game]`
   Note: the decompile prints `+0x194`; the first store is at `+400 = 0x190`.
4. Probe 3 readback (gate `mode+0x2b0`): distance `mode+0xcc = mode+0x304`, normal
   `mode+0xb8..c0 = mode+0x28c..294`. Probe 3 submit (gate `mode+0x2b1 == 0 or 0x2b0 != 0`):
   origin `O3` from `L3 = (x=mode+0x538, y=mode+0x53c, z=mode+0x540)`, direction `D3 = y-basis`,
   segment at `mode+0x250..0x26c`. `[game]` (the gate flag 0x2b1 is the submit gate of this ray,
   not a fourth ray; the decompile's "probe 4" comment is the same segment).
5. Speed block (only if `mode+0x370`, see section 2) and the drop probe at the end (l.322-353):
   origin `pos + 2*D1`, straight down 35 m, end `mode+0x328 = 2*D1.z + pos.z - 35`, `mode+0x31c = 1.0`. `[game]`
   This is NOT the height used by the jet lift; see section 2.

Hit filter and mask: the probe layer's mask / filter is inside `probe_submit` (006e0a30, unread)
and the excluded types in `FUN_0070a460`, `FUN_007c8be0` (unread). `[unread]`.

Distance units: the hit value is compared with 10 m and used against 35 m segments, so it is
metres along the ray `[assumed]` (not a 0..1 fraction).

Local points `mode+0x52c..0x540`: NO WRITER FOUND in 00864d60 (enter), 00864320, 0086b230 or
0086e020 (the grep for `0x52c` and the `fn.py ... | grep` on the enter found none). They are
read only. Where they are set (constructor, or the tuning copy) is `[unread]`.

## 2. mode+0x78 is a SPEED, not a height

**Correction 2026-10-20 (read again in disassembly, `work\raw_disasm.py 0086b230 0086c770`).**
The heading above is wrong in its reading of what the number is, and the two earlier readings
disagree only in the corrected part:
- `mode+0x78` is written RAW in both branches that set it (asm 0086c469 and 0086c64a: `movss
  [ebx+0x78], xmm0` with xmm0 = `[ebx+0x3c4]`). Nothing stores the corrected value.
- `mode+0x3c4` is the DROP probe's distance S (its readback in 0086b230 is the hit distance of the
  straight-down ray, origin `pos + 2*nose`, so S = pos.z + 2 sin(pitch) - ground). It is a height,
  not a speed. (Its writer was not found in the flight code, and is not read today: `[unread]`.)
- The corrected value S' = S - 2 e[-0x10] |sin roll| - 2 e[-0xc] |sin pitch| (e = char+0x140 row,
  00867364-00867394 and 0086c49c-0086c504) is used ONLY for the latch test (2 m, 3 m with mode+0x4bc)
  and, in 00866ef0, for the low-speed lift (1.85 / 2.85). `[game]`
- Ported 2026-10-20 in `core\src\flight.rs` (`Fly::height_over`, `Fly::on_ground_latch`); the extent
  is `[assumed]` as the vehicle avatar's width (roll) and length (pitch), see `FlyTuning::extent_roll`.
- The 3 m variant is NOT helicopter versus jet: mode+0x4bc is bit 0x80 of the `+0x9e` byte of the
  object the handle at mode+0x3b8 locks (0086bcb2-0086bd12), a collision mask bit on what the drop
  ray hit `[assumed]` what that bit means. The line below ("char+0x9e") named the character; it is
  the locked object.
- The slope test (|mode+0x354| > 0.5) is in the hit branch only (asm 0086c53e-0086c562). Ported with
  the normal passed by the caller (`Fly::ground_normal_z`, default flat).

Settled `[game]` (0086b230 l.245-283, disasm-checked for the sin terms):

- `mode+0x78 = mode+0x3c4` (copied at `0086b2xx`, l.246 and l.268-269). `mode+0x3c4` is the speed
  value the flight form is given; its writer is NOT in the flight code (grep of 00864320,
  00869df0, 0086c970, 0086cbf0, 00864d60, 0086a590 found none). `[unread]`. Settle by finding
  the writer of mode+0x3c4 in the owner or the mode base class.
- Table correction, only on the branch where the speed probe handle (`mode+0x3b0`) gave a hit
  (`0086b230` l.264-283):
  `row = e.+0x198 + e.+0x194 * 0x1c` with `e = char.+0x140` (`00b?`, 28-byte rows);
  `S' = S - ( 2*row[-0x10]*|sin(mode.+0x20)| + 2*row[-0xc]*|sin(mode.+0x1c)| )`
  (`0086b2xx` `sin_float@0040b0a0`, `float_abs@00409a50`). Note the pairing: roll `+0x20` goes with
  `row[-0x10]`, pitch `+0x1c` with `row[-0xc]`. `[game]`
- Thresholds: `thr = 2.0`, or `3.0` if `mode+0x4bc != 0` (`mode+0x4bc` = character flag bit 0x80 at
  `char+0x9e`, l.168-172, `[game]`). Then `mode+0x74 = 1` (speed latch) iff `S' < thr` (and,
  on the hit branch only, `|mode+0x354| > 0.5`), with `mode+0x7c..0x90` = segment copy from
  `mode+0x340..0x354`. `[game]`
- So S is a speed. The brief's reading "height over the ground" is WRONG. The ground-distance
  probe result is `mode+0xc4` (wall) and `mode+0x328` (drop end). `[game]`
- The jet lift that uses S (00866ef0 l.73-98 and 008676c0 l.106-119; the port recipe in
  flight-steering.md section "low speed"): `k = (1 - S'/thr)*50` for `S' < thr` (thr 1.85 or
  2.85), then `vz += 3*k*dt`. `[game]` constants `50.0` (`00b3485c`), `3.0` (`00b39c48`),
  `1.85` (`00b312b8`), `2.85` (`00b6b3b0`). In 00866ef0 the vector goes to `00556b40` as
  `(0,0,3*k*dt)`. `[game]`

## 3. Contact response: 0086e020 `flight_contact_response(mode, dt)`

Called from 00864320 at l.151, ONLY inside the branch `mode+0x55 == 0` and `char+0x84 == 0`,
after the stick step (`00866ef0` or `008676c0`). With `mode+0x55 != 0` it is not called. `[game]`

Numbered steps:

1. Gate `[game]` (asm 0086e020-0086e05e): if any of `mode+0xc, +0x10, +0x14` (the control
   vector) is non-zero: `mode+0x4b4 = 0`, return.
2. Wall tests per probe `i` in {1,2,3} with distance `d_i` and normal `n_i`:
   `c_i = (0 < d_i) and (d_i < 10) and (|n_i.z| < 0.1)`
   Probe 1: `d1 = mode+0xc4`, `n1 = mode+0xa0..a8`. Probe 2: `d2 = mode+0xc8`, `n2 = mode+0xac..b4`.
   Probe 3: `d3 = mode+0xcc`, `n3 = mode+0xb8..c0`. Constants `10.0` (`00b34278`, asm 0086e087) and
   `0.1` (`00b345ec`, asm 0086e0a3), `|x|` via `00409a50`. `[game]`
   The test is on the normal's z. A normal with |z| < 0.1 is a near-horizontal surface, a WALL.
   So the three probes are wall probes. The ground is not one of them.
3. `mode+0x4b4 = c1 or c2 or c3`. If none: `mode+0x544 = mode+0x548 = mode+0x54c = 0`, return.
   `[game]` (0086e0ae-0086e198, `0086e198 mov [ebp+0x4b4], al`).
4. Corner flag E (two contacts whose normals differ, dot <= 0.1):
   - `E = (c1 and c2 and dot(n1,n2) <= 0.1) or (c1 and c3 and dot(n1,n3) <= 0.1) or (c2 and c3 and dot(n2,n3) <= 0.1)`
   - `dot(n1,n2) = n2.z*n1.z + n2.y*n1.y + n2.x*n1.x` (mode+0xb4*a8 + b0*a4 + ac*a0). `[game]`
5. Path selection `[game]` (decompile flow, cross-checked against the branch labels):
   - If E: FLOOR SPRING path (step 6).
   - Else the NEAREST contacting probe wins: among the probes with c_i true, the one with the
     smallest d_i (ties give no contact; `0086e2d9`, `0086e2d0`, `joined` label). Probe 3 when
     the minimum is d3, probe 1 for d1, probe 2 for d2. The chosen probe gives
     origin `O` (probe 1: `t`; probes 2/3: the local point in the basis, section 1), normal `n`
     and distance `d` (the chosen `d_i`).
   - Step 7 (WALL path) uses it.
6. FLOOR SPRING path (E set), disasm `0086e316-0086e56e`:
   - origin `Of = (t.x, t.y + 5.0, t.z)` (`0086e35b..0086e368`; `00b3476c` = 5.0), direction
     `Df = (-0.0, -1.0, -0.0)` (`0086e382..0086e3a8`), probe call `FUN_0086ec70(mode, Of, Df)` (unread).
   - sign `s` from the owner's `+0x20` object (vtable +0x30, unread): if `y > -0.25` then `s = +1`
     (`0086e3d8-0086e3e6`); else `FUN_00855910` (unread) and if its `y > 0.25` then `s = -1`
     (`0086e3f4-0086e402`). Then `s = -s` if `FUN_007504b0()+0x1f4f` is non-zero (`0086e415-0086e41c`).
     `[game]` The default when neither test fires is the value of `[esp+0x18]` (not traced) `[trace]`.
   - target force `Z = s * 500.0 * dt` (`0086e434`, constant `500` at `00cad46c`; sign applied `0086e41e`).
     x and y targets are `0`.
7. Spring state (both paths), disasm `0086e445-0086e4d6`:
   - `tgt = (0, 0, Z)` (floor) or the normal vector below (wall). The state `mode+0x544..54c` is
     lerped toward the target: if `3*dt < 1`: `v = v + (tgt - v)*dt*3` else `v = tgt`
     (rate constant `3.0` at `00b39c48`, `dt*3 < 1` test at `0086e47c`). `[game]`
   - the lerped vector IS the velocity increment: `body_add_velocity(body, &mode+0x544)`
     (`0086e4f2`: `call 00556b40` with `edx = esi = mode+0x544`). `[game]` (floor path).
   - Who reads `mode+0x544..54c`: only 0086e020 (its own lerp history) and the add call above
     (also at `0086eb48`, the wall path: `edx = esi`). Nothing else in 00864320 reads it. `[game]`
   - FLOOR path only: horizontal velocity damping `vx *= 0.95`, `vy *= 0.95`, `vz` unchanged
     (`0086e51d-0086e55e`: `00556c00` reads velocity into `[esp+0x28]`, multiplies `[+0x28]` and
     `[+0x2c]` by `00b6a8e8` = 0.95, `00557000` sets it). `[game]`
8. WALL path (not E), with the chosen probe (step 5). `[game]`
   - Contact point `Pc = O + d * D` (the decompile's `*in_EAX*d + origin`; `D` = the probe direction,
     the y-basis `[assumed]`; `in_EAX` is not traced).
   - Mirror: `w = O - Pc = -d*D`; `M = O - 2*(w - n*(w.n))` (tangential reflection, decompile
     l.~127-133). `[game]` (structure).
   - Direction `u = unit(M - O)` (decompile l.~138-146; `if |M-O|^2 > 1e-8` else unnormalised).
   - Cosine `c = u . n` (`local_90`, l.~150). If `|c| < 1`:
     `k = (1 - 0.1*d) * 100 * (1 - c)`; `k = max(k, 500)` (`local_90 < DAT_00cad46c` clamp).
     KEY: for `0 < d < 10` and `-1 <= c <= 1` the product is at most `200`, so the clamp always
     wins and `k = 500` in practice. `[game]` (the clamp is the 500 constant; the 100*(1-0.1d)*(1-c)
     formula in the ledger never exceeds it). `[assumed]` that `|n| = 1` (no normalisation seen).
   - Target vector `tgt = u * k * dt` (decompile `fVar14 = u.x*local_90*param_2`, etc.). Lerp per step 7.
   - Then if `d < 10`: remove the normal velocity at 95 %: `vp = (vx,vy,vz).n`, then
     `v = v - n * (0.95 * vp)` (`0086ebef-0086ec12`, `00557000`; decompile l.~306-317). `[game]`
9. Done. `FUN_0086ec70` at the end is the probe for the floor path, its result is unread. `[trace]`

Bounce / wall-stop are NOT called from here. The branch between `0086e020` and `00868b00` /
`00868fb0` is in 00864320 (section 4).

## 4. Who sets and clears mode+0x55, and the branch in 00864320

- 00864320 l.130-163 `[game]`:
  ```
  if mode.55 == 0:
      if char.84 == 0:
          if button 0x1f not held: mode.4c = 0; FUN_0086aa60(0); ... flight_control_step_stick (00866ef0)
          else:                    ... flight_control_step_hold (008676c0)
          flight_contact_response (0086e020)            # l.151
      else:
          flight_control_step_grounded (00867f20)       # l.154
  else:                                                 # mode.55 != 0
      if char.84 == 0 and char.b0 == 0: flight_surface_bounce (00868b00)   # l.159
      else:                              flight_wall_stop    (00868fb0)    # l.162
  ```
  So with mode+0x55 set, the stick steering and the probe contact response are both skipped.
- Writer: `FUN_0086ab90` (`flight_contact_set`, ledger note added). It sets `mode+0x55 = arg`,
  and on a rising edge clears `mode+0x56`; it sets `mode+0x4c = 0`; it stores the vector
  `in_EAX` into `mode+0x5c..0x64` and the normal `param_1` into `mode+0x68..0x70`. `[game]`
  Only two callers, both in `007e7830` (census: `flight/FlightBoundaryEllipsoid`):
  - `FUN_0086ab90(1)` (`007e7830` l.51) when the test `FUN_007e7b60(param_1) == 0` and the flight
    object exists (`piVar6 != 0`); `param+0x28 = 1`. `[game]`
  - `FUN_0086ab90(0)` (`007e7830` l.75) in the else branch (`FUN_007e7b60 != 0`); `param+0x28 = 0`. `[game]`
  So mode+0x55 is the FLIGHT BOUNDARY contact flag, set and cleared by a per-frame check of the
  boundary volume, not by the probes. `[game]` (the boundary test `FUN_007e7b60` and the
  caller of `007e7830` are unread: `[trace]`).
- The other grep hit for `+0x55) =` in 0086f520 is the foot-IK constructor (its own object), not
  the mode. `[game]` (false match).
- Clear: the same `FUN_0086ab90(0)` path; no other writer found.

## 5. Summary for the programmer (one update, in order)

0. Inputs: stick (deadzone 0086d5c0), euler angles from the body matrix.
1. `probes()` (0086b230): read last update's hits into `d1,n1 / d2,n2 / d3,n3`; then submit
   forward rays of length 35 from `t`, from `L2` and from `L3` (section 1). Speed `S = mode+0x3c4`
   (copy), corrected by the table (section 2).
2. Boundary flag `mode+0x55` comes from the boundary check (007e7830, unread). It is not a probe.
3. If `mode+0x55 != 0`: bounce (00868b00) or wall-stop (00868fb0) INSTEAD of steering. Those are
   in flight-forces.md and flight-helpers.md. Stop here.
4. Else: steering (00866ef0 / 008676c0 / 00867f20; the low-speed lift `vz += 3*k*dt` when `S' < thr`).
5. Then `contact()` (0086e020):
   - gate: any control-vector component non-zero: clear contacts, stop.
   - contacts `c_i = 0<d_i<10 and |n_i.z|<0.1`; none: zero the spring state, stop.
   - corner E: two contacts with `dot(n_a,n_b) <= 0.1`: floor path (ray from `t+(0,5,0)` down `-y`,
     spring `vz += lerp(s*500*dt)`, horizontal velocity `*0.95`) [axis: section 6 V].
   - else nearest contact: `u = unit(M-O)`, `k = max(500, ...) = 500`, add `u*500*dt` (lerped, rate 3)
     to velocity, and if `d < 10` remove 95 % of the normal velocity along `n`.
6. Angles and position: the contact path changes velocity only. The position is not moved
   directly (the "other branch" in the ledger is the same velocity add). Body angles are changed by
   the steering and the bounce path, not here.

What a port needs from the world: `ray(from, to) -> Option<(t_metres, normal)>` with a
mask/filter (unknown, section 1) and a rule for the excluded surfaces (FUN_0070a460,
FUN_007c8be0, unread). One ray result per probe per update, read one update late (or the port
may read it the same update: a choice, state which).

Minimum port (what the game does, with the open items marked):
- three forward rays of length 35 m from `t`, `L2`, `L3`;
- `c_i` test on `d_i` (0,10) and `|n_i.z|` (< 0.1);
- nearest-wins, dot-corner flag, spring `500*dt` along `u` (wall) or `-y`/`z` (floor, see V);
- velocity `*0.95` tangential (floor) or `-0.95` normal component (wall, `d<10`);
- the boundary flag from 007e7830 (unread) gates all of it.

## 6. Open items

- V: vertical axis. The floor probe uses `-y` (`0086e35b-0086e3a8`), the drop probe and the
  wall test use z, and `00868b00` / `0086ab90` use `z <= -0.95` as "inverted". Needs a recording
  of a wall or floor contact or a check of the world up axis in the Luxoflux frame. `[trace]`
- `mode+0x3c4` writer (the speed source). `[unread]`
- Local points `mode+0x52c..0x540` writer. `[unread]`
- `FUN_0086ec70` (floor probe), `FUN_00855910`, `FUN_007504b0` and the vtable at owner+0x20+0x30
  (the sign test): `[unread]`.
- `FUN_007e7b60` (the boundary test) and the caller of `007e7830`. `[unread]`
- probe mask and excluded-type filters (`probe_submit`, `FUN_0070a460`, `FUN_007c8be0`). `[unread]`
- The default value of `[esp+0x18]` on the floor sign path. `[trace]`
- Whether hits are read the same update or the next one (section 1 says next; the world
  resolution order is `[assumed]`).
- Reader of `mode+0x4b4` (written here; not found in 00864320). `[unread]`
- 00556b40 call in the wall path: `edx = esi` was seen at `0086eb46-0086eb48`; that `esi` is set to
  `mode+0x544` in the decompile but the `lea` was not traced in that block. `[assumed]`.

## 7. Not read / not sure

- Not read: 00556b40 (body_add_velocity) body, 00556c00 (velocity read, `[game]` by ledger only),
  00557000 (velocity set, ledger), 006e0a30 (probe_submit), 0086ec70, 00855910, 007504b0,
  007e7b60, 0070a460, 007c8be0, 00562970, FUN_00557150 (frame push/pop), 0086ab90 full body
  after the branch (only the first 40 lines), 007e7830 (only its branch structure).
- Not sure: the vertical axis (V), the sign default (`[esp+0x18]`), the normalisation of `n`,
  the exact base register of `in_EAX` in the wall path, the probe direction of `Pc` (y-basis
  assumed), the meaning of mode+0x130/0x131 (readback / submit gates, read as flags only).

## 8. Fields seen (mode base unless stated)

| field | read / write | by which function |
|---|---|---|
| +0x4 | read (owner) | 0086b230, 0086e020, 0086ab90 |
| +0x18 | read (char) | 00864320 (char+0x84, +0xb0) |
| +0x1c, +0x20 | read | 0086b230 (sin terms), 0086ab90 |
| +0x34..0x3c of owner+0x2c | read (translation t) | 0086b230 |
| +0x10..0x30 of owner+0x2c | read (basis) | 0086b230 |
| +0x52c..0x540 | read only (writer not found) | 0086b230, 0086e020 |
| +0x55 | write (via 0086ab90) / read | 007e7830 (write), 00864320 (read) |
| +0x56 | write (0086ab90, rising edge clear) / read (00868b00) | 0086ab90, 00868b00 |
| +0x5c..0x64 | write (0086ab90) / read (00868b00, 00868fb0) | |
| +0x68..0x70 | write (0086ab90) / read (00868fb0) | |
| +0x74 | write (speed latch) | 0086b230 |
| +0x78 | write (= +0x3c4, table-corrected) / read | 0086b230 (write), 00866ef0 etc. (read) |
| +0x7c..0x90 | write (segment copy) | 0086b230 |
| +0x4b4 | write / reader not found | 0086e020 |
| +0x4b8 | write (zero on speed latch) | 0086b230 |
| +0x4bc | write (char flag) / read | 0086b230 (write), 0086b230 (threshold) |
| +0x544..0x54c | read + write (lerp) and add to velocity | 0086e020 (only), passed to 00556b40 |
| +0x130, +0x131 | read (gate) | 0086b230 |
| +0x170 (handle), +0x184, +0x10c..0x114 | read (readback copy) | 0086b230 |
| +0xa0..0xa8 | write (probe 1 normal, readback) / read | 0086b230, 0086e020, 00868b00 |
| +0xc4 | write (probe 1 distance) / read | 0086b230, 0086e020, 00866ef0 |
| +0xd0..0xec | write (probe 1 segment) | 0086b230 |
| +0x1f0, +0x1f1 | read (gate) | 0086b230 |
| +0x1cc..0x1d4, +0x244 | read (readback) | 0086b230 |
| +0xac..0xb4 | write (probe 2 normal) / read | 0086b230, 0086e020 |
| +0xc8 | write (probe 2 distance) / read | 0086b230, 0086e020 |
| +0x190..0x1ac | write (probe 2 segment) | 0086b230 |
| +0x2b0, +0x2b1 | read (gate) | 0086b230 |
| +0x28c..0x294, +0x304 | read (readback) | 0086b230 |
| +0xb8..0xc0 | write (probe 3 normal) / read | 0086b230, 0086e020 |
| +0xcc | write (probe 3 distance) / read | 0086b230, 0086e020 |
| +0x250..0x26c | write (probe 3 segment) | 0086b230 |
| +0x328, +0x31c, +0x324 | write (drop probe end) | 0086b230 |
| +0x3c4 | read (speed source; writer outside flight code) | 0086b230 |
| +0x3b0 | read (speed probe handle) | 0086b230 |
| +0x354, +0x340..0x354 | read | 0086b230 |
| +0x370 | read (gate for the speed/ground block) | 0086b230 |
| +0x504 | write (surface FX dummy) | 0086b230 |
| +0x4c | write 0 | 0086ab90 |
| +0x28 (of 007e7830's param) | write 1/0 | 007e7830 |
| char +0x84, +0xb0 | read | 00864320 |
| char +0x140 (table), +0x194, +0x198 | read | 0086b230 |
| body +0xa0..0xa8 (velocity) | read/write | 0086e020 (via 00556c00/00557000) |
| owner+0x2c basis +0x10..0x3c | read | 0086b230 |

## Ledger

Re-noted in the unpublished notes: 0086e020 `flight_contact_response`, 0086b230
`flight_probe_submit_and_readback` (both corrected, see sections 1 and 3), and two new entries:
0086ab90 `flight_contact_set` (the mode+0x55 setter), 007e7830 `flight_boundary_contact_check`
(its caller).
