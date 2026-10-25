# The finisher camera (startCamera of Trans_Vehicle2RobotSlam)

Read 2026-10-20, read-only research. Question: what the camera pushed by the car-to-robot slam's
FinishMoveEvent `startCamera` does each frame. Tags: `[game]` read in the disassembly or decompile,
`[assumed]` a reading that could be wrong, `[unsettled]` not settled by what was read.
The decompile was checked against the disassembly wherever the two disagreed or dropped something
(`work\raw_disasm.py`). Nothing here was run in the game.

## 1. Summary

- The behaviour is NOT the CameraSpectator. Its key is 0x768ce890 (the port calls this
  `OTHER_CINEMATIC_KEY` in `core\src\camera\spectator.rs:15`, "which class: [unknown]"; it is this
  camera). The spectator's key is 0xd63697a6, its constructor 00919510, its vtable 00b65f00.
- Entry: `007314d0` -> `0072df10` -> `009192c0` (pushes) -> `00917b40` (constructs, vtable 00b65e98)
  -> `0091db00` (switch: it becomes the active camera) -> slot 1 `00917c60` (enter).
- Enter (once, slot 0 `009186f0`): FOV 50 degrees, a focus point between the two characters, an eye
  point a few metres to one side of the focus (side chosen by a coin), pushed further along the
  A-to-B line by a random amount. The FOV is widened when the eye is closer than about 21 m to the
  focus. The camera's rate is set to 0 (a cut, no spring).
- Every frame (slot 4 `00918fa0`, called by `0091df50`): the FOCUS is recomputed from the two
  characters' current positions; the EYE is not moved; the camera's basis is rebuilt from eye and
  focus (horizon-level right vector); the eye is written to the camera. The FOV is not touched again.
- The per-frame update slot 3 (`00580aa0`) is `ret 4`: a no-op. So the entry code (slot 0) runs
  only at entry, not every frame.
- No blend in or out: it cuts in (`0091db00`) and out (`009193d0`, which restarts the parent with
  its enter). Exit (slot 2 `00544340`) is a no-op. It lasts until endCamera removes it (`009193d0`).
- The 0x2c byte (set to 1 by the caller): at entry, slot 1 requests a slow motion,
  `007581f0(1.25, 0.5, this, key 0)`: a scale of 0.5 with a 1.25 (duration? priority?) value. The
  timer that ends it is not read.
- The 60.0 at +0x14 is never read by this camera's code. It is the family's default FOV field.
- Ledger corrections (not made to the ledger, see section 10): `009186f0` is the finisher, not a
  spectator; the bit 0x200000 of `0072df10` goes on the object of the first handle, i.e. the
  attacker, not the target (asm 0072df52), `[assumed]` as him.

## 2. Who starts it, and when

`007314d0` (FinishMoveEvent fire, player only): `startCamera` set and `char+0x234` (his held target)
nonzero -> `0072df10`. `endCamera` set and held target -> `0072e080` (unlink; its ledger line says
it calls `009193d0` when the held target has bit 21). `killDude` -> `0072dfd0`.

`0072df10` (finish_move_link), asm:
- `0072e100` finds the target; its handle is stored at ESI+0x98 by `0074af20` (the held target).
- The handle at ESI is locked (`00544db0`) to EDI; `or [EDI+0x14c], 0x200000` (bit 21 of the
  character's flags 0x14c: stops the stick's turn in the attack mode, see `docs/melee.md`).
  EDI is the object of the handle at ESI, not the target (the target comes from `0072e100` and is
  not locked there). `[assumed]` ESI is the attacker, as `docs/melee.md` says; the ledger line for
  `0072df10` says "target+0x14c": that reading is doubtful.
- Then `009192c0(EDI+0xc, target+0xc, 1)`.

`009192c0` (the pusher), asm:
1. Refuses (returns 0, nothing pushed) when a camera behaviour with key 0x768ce890 is the active one
   or is found on the active one's parent chain (`007d1c40`).
2. Removes a running CameraSpectator (key 0xd63697a6) first, if it is the active one or on the chain
   (`009193d0`-style removal: `0091db80`, then slot 15 with flag 1).
3. Allocates the behaviour from pool `0xd0b470` (`005343a0(pool, 9)`) `[assumed]` allocator. If the
   allocation fails the asm jumps to 0x9193b4, which calls `0091db00` with EAX = 0 and no
   behaviour is pushed. `[unsettled]` what the switch does with a null argument (its body reads
   `in_EAX[3]` when a camera is active): not analysed further.
4. Constructs `00917b40` with ECX = the game camera (`DAT_00dae234`), EAX = `009192c0`'s param 1
   (EDI+0xc), stack arg 1 = param 2 (target+0xc), stack arg 2 = the byte 1.
5. `0091db00` (switch) makes it active.

## 3. Object layout (constructor 00917b40, asm)

| offset | value | source |
|---|---|---|
| +0x00 | vtable 00b65e98 | `mov [esi], 0xb65e98` |
| +0x04 | the game camera (`DAT_00dae234`) | ECX |
| +0x08 | parent link in the chain (0 at construction) | `0091db00` sets it |
| +0x0c | priority 7 | `mov [esi+0xc], 7` |
| +0x10 | key 0x768ce890 | `mov [esi+0x10], 0x768ce890` |
| +0x14 | wanted FOV 60.0 (0x42700000, from 00b34630) | `movss [esi+0x14], xmm0` |
| +0x18 | weak handle H, copy of EAX (the attacker, `[assumed]`) | `00917b40` 0x917b84..0x917bb1 |
| +0x20 | weak handle T, copy of the first stack argument (the target) | `00917b40` 0x917bbc..0x917bfb |
| +0x28 | FOV in degrees (written each entry by slot 0; 50 first) | `009186f0` line 5 |
| +0x2c | the byte: 1 from `0072df10`; gates the slow motion in slot 1 | `00917b40` 0x917bf2, `00917c60` 0x917c75 |
| +0x30..+0x38 | FOCUS (look-at point). Written by slot 0 at entry and by slot 4 every frame | `009186f0` 121..123, `00918fa0` 1..4 |
| +0x3c..+0x44 | EYE (camera position). Written by slot 0 at entry only | `009186f0` 118..120 |

The handles H and T are weak (ptr, ctrl) pairs with a reference count (`lock xadd` on the counter,
`00532c40` releases the old one). Slot 0 and slot 4 read them through `009180a0` ("the two
characters") via `refhandle_copy` (`009003e0`). `[assumed]` H is him, because the bit-21 object is
the one locked from ESI (see section 2).

## 4. Vtable 00b65e98 (25 slots)

| slot | offset | address | size | role | status |
|---|---|---|---|---|---|
| 0 | +0x00 | 009186f0 | 2218 | entry placement (framing) | READ (section 5) |
| 1 | +0x04 | 00917c60 | 117 | enter: calls slot 0, then the slow motion if +0x2c | READ |
| 2 | +0x08 | 00544340 | 1 | exit: `ret` (no-op) | READ |
| 3 | +0x0c | 00580aa0 | 3 | per-frame update: `ret 4` (no-op) | READ (asm) |
| 4 | +0x10 | 00918fa0 | 787 | aim, every frame: focus, basis, eye written to the camera | READ (section 6) |
| 11 | +0x2c | 00917b30 | 13 | `*arg = 0x57dc9078`: a type name hash, not in names.tsv | READ |
| 15 | +0x3c | 00917c10 | 68 | release: `917ce0`, then to pool 0xd0b470 when the flag arg is 1 `[assumed]` | read in part (decompile) |
| 16 | +0x40 | 0091c130 | 68 | like 15 with `0091c180` as base; lives in the CameraSpectator code range `[assumed]` | read in part |
| 22 | +0x58 | 0091c6d0 | 1008 | not read (not used by this camera's paths seen) | unread |

Other slots were not read: 5 (`0091fea0`, 19 bytes), 6 (`007cfc70`), 7 (`007cfc90`), 8 (`00544340`,
a `ret`), 9 (`005bbd50`), 10 (`007cfc90`), 12 (`0053a260`), 13 (`00544340`), 14 (`008b7f50`),
17 (`0091c6b0`), 18 (`00580aa0`, a `ret 4`), 19 (`0091cda0`), 20 (`007ba640`), 21 (`00544340`),
23 (`0091cd80`), 24 (`00580aa0`). Only the stubs of 2, 8, 13, 21 (`00544340`) and 3, 18, 24 (`00580aa0`)
are known to be no-ops; the rest are not reached by this camera's paths as read.
The per-frame dispatch reads only slot 3 (`0091d230`) and slot 4 (`0091df50`) of the active
behaviour: verified in the decompile (`0091d230` calls `+0xc`, `0091df50` calls `+0x10`).

## 5. Entry: slot 0 (009186f0), step by step

Inputs: the camera (`+0x4`), the handles H (+0x18) and T (+0x20), the random stream, the time.

5.1 FOV: `+0x28 = 50.0` (0x42480000). Half-width `hw = 25` (= FOV/2).

5.2 `009180a0` (two characters, section 7) gives the focus F, the unit direction dir = normalize(Tc - Hc)
(3D), and the top height Zmax.

5.3 Nominal distance: `D = 25 / tan(50 deg) = 20.98 m`, but at least 15 (`local_88`).

5.4 Side vector: S = (dir.y, -dir.x, 0) / |(dir.x, dir.y)|, so S is horizontal and perpendicular to
the horizontal part of A-to-B. Its z is 0.

5.5 Side eyes E1 = F + S*(D+1), E2 = F - S*(D+1). Both are tested with `00917dc0` (section 9). Their
heights are equal (S.z = 0), so the branch on their height difference (>= 0.01) never fires; the side
is chosen by a coin: `DAT_00c02ef0 = LCG`, keep +S when `((time64 low byte) ^ byte2(LCG)) & 1 == 0`,
flip to -S otherwise `[game]`. The port's `Random::coin(seconds)` (core\src\camera\spectator.rs:173)
computes `((state >> 16) ^ seconds) & 1`, the same bit `[assumed]` that the state is advanced before
the test, as the decompile shows.

5.6 Lateral distance: `t = clamp(F.z - 1, 1, D)` (F.z is the focus height). So the side point is
`P = F + S*t`. The focus height, not D, decides how far to the side: with a focus 4 m up, t = 3 m.

5.7 Height/depth search: lines 73-94 of the decompile set a floor-like value from the two characters'
`+0x3c` fields and the side point's height (-22 m); `c4` is derived from it. The variable reuse in
those lines makes the exact clamp `[unsettled]`.

5.8 Ray tests: `00917dc0` at P toward P - 50*dir and toward P + 12.5*dir. They store nothing (section 9).

5.9 The eye: `E = P + r*dir`, with `r = Random::between(-c4, Zmax)` (`00551d60`, the random
range). The decompile drops the random value's use (it comes back in st0); the form above is the
one the surrounding arithmetic needs `[assumed]`.

5.10 Focus-eye distance d = |F - E|. If `tan(50) < 25/d` (d < 20.98 m), FOV := `atan(25/d)` in degrees
(lines 128-130; `FUN_00466960` is taken to be atan `[assumed]`, its body was not read). So the FOV widens when the eye is closer than about 21 m to
the focus. With t = 3 m and r of several metres the usual d is well under 21 m, so the FOV is
usually wider than 50 degrees on entry `[assumed]` (not computed).

5.11 Written to the camera: camera `+0x94` = FOV (degrees), camera `+0x128` = 0 (no spring, a cut),
camera `+0x28` = E, and the basis (section 6). Also `+0x30..+0x38` = F and `+0x3c..+0x44` = E.

## 6. Every frame: slot 4 (00918fa0) and the aim

`0091df50` (camera aim) calls slot 4 of the active behaviour every frame. Slot 4:
1. `009180a0` -> focus F (from the characters now, not the entry frame) -> `+0x30..+0x38`.
2. Snap: if |F - E|^2 < 0.1 (under 0.32 m), F := E + 2 * (the camera's previous forward `+0x10..+0x18`).
3. Forward f = normalize(F - E) (from `+0x3c` to `+0x30`).
4. Right r = normalize(f_y, -f_x, 0): horizon-level, `[game]` (the z of the right vector is 0,
   so the camera never rolls).
5. Up u = r x f (camera `+0x1c..+0x24`).
6. Camera `+0x04..+0x0c` = r, `+0x10..+0x18` = f, `+0x1c..+0x24` = u, `+0x28..+0x30` = E.

Slot 4 does not write the eye `+0x3c`, the FOV, or the rate. So the eye is fixed from entry;
the look-at point follows both characters; the FOV is the entry value.

## 7. The two characters: 009180a0

For each of the two handles (H at +0x18, T at +0x20): the object's position `+0x34` (x), `+0x38` (y),
and bottom `+0x3c`. The branch tests `character_object_lookup` (0070a3c0), the registered Character
(`docs/camera.md`: not necessarily the character playing the clip):
- lookup null: top = bottom + 5 m, offset 0.
- lookup non-null: bottom from `flying_vehicle_camera_target_bottom` (0071a460), top = bottom +
  `character_camera_height` (0070abe0), and the offset `reach` = `char+0x104 object` `+0x170 +
  (+0x244 index) * 4` (0 when +0x104 is null).

Outputs: centre C = (x, y, (bottom + top)/2) for each; dir = normalize(Tc - Hc) in 3D;
F = 0.5*(Hc + Tc) + 0.5*dir*(reach_T - reach_H); Zmax = max(top_H, top_T). The routine reads no
hash itself; the reach value is the per-form value on the character's STEP record (its callees
were not all read).

If the second handle is null the first is used for both. If the characters coincide horizontally,
the horizontal normalisation is skipped (1e-8 guard).

## 8. The 0x2c byte and the slow motion

Slot 1 (`00917c60`), asm 0x917c6c..0x917cc8:
1. Calls slot 0 (`009186f0`): the entry placement (section 5).
2. If `[this+0x2c] != 0`: `007581f0(1.25 at 0x00b3450c, 0.5 at 0x00b38830, this, &key)` with the
   key = 0 and the list head 0x00daddcc.

`007581f0` (asm-level reading of the decompile):
- Finds the request node for the owner `this` in the list at 0xdaddcc (`00757fc0`).
- New node: if 1.25 >= the list's current maximum (the head's +8), the maximum becomes 1.25, the
  node is active, and `global_game_state()+0xc0` takes 0.5. So the largest request wins and sets
  the scale.
- Existing node: updates its scale (+0x10) and its value (+0xc) through `00758180`.

So 0.5 is the game speed while this request is the largest `[assumed]` (the port's
`Start::GameSpeed` in spectator.rs uses 0.5 too, from its own settings). What 1.25 measures is
`[unsettled]`: a duration in seconds (the port's own reading of the other call, 1.5, is called "a
blend time", `[assumed]`), or a priority. The list's timer and its decrement were not read, so the
slow motion's end is `[unsettled]`. The request is made at each enter (entry or restart of the
parent). Nothing in this camera removes it; the node is keyed by `this`.

Note: `assumptions.md:488` says 1.5 is passed to `007581f0` for the spectator. Here the constant at
0x00b3450c is 1.25 (`fld [0xb3450c]` at 0x917cbf, `1.25 / 0x3fa00000`). The two calls differ; the
spectator's call was not re-read here.

## 9. The ray helper (00917dc0)

`world_ray_sphere_query` (`005be890`) from P to Q with mask 0x20 (the blocking objects, flag bit 5 of
`+0x9c`), and the hit list freed. In the decompile the nearest flagged hit's index is kept in a local,
then the function returns: nothing is stored to its arguments or to the behaviour. The disassembly
of 0x917dc0..0x917ee0 shows no store through the argument pointers (only loads for comparisons and
stack stores for the query's far distance). So the ray tests in slot 0 have no visible effect on the points.
`[unsettled]`: a dead test as read, or a store the decompile does not show. The port's
`clear()` (spectator.rs:139, `00919800`) is a different helper.

## 10. What ends it, blends, and the priority rule

- Enter: `0091db00` (switch). Refused when the active behaviour has a parent and `00711ea0` answers
  (its argument is not visible in the decompile; read as "the new one is already on the chain"). Queued (not activated) when its priority (+0xc) is below the active one's (`0091d1b0`
  inserts it in the chain behind the entries whose priority is not below its own). So the finisher
  (7) does not interrupt a camera of priority 8 or more (the spectator is 8 in the port's
  spectator.rs; its key and priority are in its header).
- Exit: `009193d0` (endCamera's removal, called by `0072e080`) removes every camera behaviour with key
  0x768ce890 from the chain, calling slot 2 (exit), unlinking, slot 1 of the new active one when the
  active one was removed, and slot 15 with flag 1 (free). `[game]` (the key is set in ECX before the
  call to `007d1c60`, asm 0x919426).
- Blend: none. The switch sets the active pointer at once; the camera's rate (`+0x128`) is 0; the
  FOV is set directly. The parent restarts with a cut.
- Duration: until endCamera (the event's own timing, from the animation). The camera has no timer of
  its own. The slow motion's timer is `[unsettled]` (section 8).

## 11. Reads from other systems

- Characters: positions `+0x34..+0x3c`; the registered Character (`character_object_lookup`,
  0070a3c0); `flying_vehicle_camera_target_bottom` (0071a460); `character_camera_height` (0070abe0);
  the STEP reach `+0x104 -> +0x170 + (+0x244)*4`.
- Random: `00551d60` (`Random::between`, port spectator.rs:168) and the time (`_time64`); the coin
  is the port's `Random::coin` (spectator.rs:173).
- Game speed list: `007581f0` (0xdaddcc), `global_game_state()` `+0xc0` (005a9cc0).
- Camera: writes `+0x94` (FOV), `+0x128` (rate), `+0x04..+0x30` (basis and eye); reads nothing from
  the camera except the previous forward (slot 4 snap).
- Tuning hashes: none read by this camera's own code (its callees 0070abe0, 0071a460 and 0070a3c0
  were not all read for hashes).
- `+0x14` (60.0): not read by 009186f0, 00918fa0, 00917c60, 00917dc0, 009180a0, 00917b30. The follow
  camera's FOV spring reads it (`docs/camera.md` 346, "wanted field of view"), but this camera
  never reaches that spring. The port's `AnimatedCamera` has the same default (`camera\scripted.rs`,
  DEFAULT_FOV 60, "007cfd30 stores 0x42700000"), so 60 is the family's default, not a FOV of this shot.

## 12. Port recipe

Status: nothing of the finisher is ported (E6: "READ, not
ported"). `core\src\camera\spectator.rs` has the `World` trait (line 127), `clear()`, `Random`
(`between`, `coin`), and a `Target` enum (line 104) that the finisher can reuse.

Struct (new file `core\src\camera\finisher.rs`, `pub mod finisher;` added beside `spectator` in
`core\src\camera.rs` line 30):

```rust
/// What the camera needs of one character: bottom point and top height, and the flyer reach. [game]
pub struct Side {
    pub bottom: Vec3,   // x, y and bottom z (009180a0 +0x34, +0x38, +0x3c or 0071a460)
    pub top: Vec3,      // the top z is bottom.z + 5 (no lookup) or + camera_height (lookup)
    pub reach: f32,     // +0x170[+0x244] of the character's +0x104 object, 0 for ground
}

/// The finisher camera (behaviour 00917b40, key 0x768ce890, priority 7). [game]
pub struct Finisher {
    pub priority: u8,         // 7 (+0xc)
    pub wanted_fov: f32,      // 60.0 (+0x14): unused by this camera
    pub fov_deg: f32,         // +0x28: 50 on entry, widened (atan(25/d)) when d < 20.98
    pub focus: Vec3,          // +0x30: rebuilt every aim
    pub eye: Vec3,            // +0x3c: set once at entry
    pub slow_motion: Option<(f32, f32)>, // (1.25, 0.5) when the flag (+0x2c) is set; timer unsettled
}

pub struct Pose { pub eye: Vec3, pub focus: Vec3, pub fov_deg: f32, pub right: Vec3, pub forward: Vec3, pub up: Vec3 }

impl Finisher {
    /// Slot 0 + slot 1 (entry, once). Inputs: the two sides, a random stream, the seconds for the coin,
    /// and `world` (ray tests; no effect as read, section 9). dt is not used.
    pub fn enter(h: &Side, t: &Side, random: &mut Random, seconds: u32, flag: bool) -> Finisher;
    /// Slot 4 (every frame): focus from the sides, snap, basis; the eye is kept. dt is not used.
    pub fn aim(&mut self, h: &Side, t: &Side, previous_forward: Vec3) -> Pose;
}
```

Inputs: H and T as `Side` (built in the game from the two characters, with the lookup's branch);
the random stream; the time (`seconds` for `coin` and `between`); `previous_forward` (the camera's last
forward, for the snap only). dt: none needed (the slow motion is one request at entry).

Outputs: `Pose` (eye, focus, FOV in degrees, basis). The FOV is written to the camera without a spring
(camera `+0x128` = 0), and the eye and basis are written as `+0x04`, `+0x10`, `+0x1c`, `+0x28`.

Hook:
- Start: the FinishMoveEvent `startCamera` branch (`007314d0` -> `0072df10`), which is not yet ported.
  The port would build two `Side`s and call `enter`, after: removing any running spectator, refusing
  when a finisher is running, and requesting the slow motion (`(1.25, 0.5)`) through a speed-request
  list like `007581f0` (the port has `Start::GameSpeed` in spectator.rs:89 for its own, not a list).
- Priority: a single `Option<Finisher>` in the game camera is enough for the 2 cameras; a camera of
  priority 8 or more (the spectator) blocks it (`0091db00`).
- Apply: `game\src\camera.rs` `apply` (line 375) takes its `Transform` and `Projection` from the
  `Rig`; while a finisher is active, it should take `Pose` instead (eye as translation, basis as
  rotation, FOV in degrees), and the follow camera's FOV spring is off.
- End: the FinishMoveEvent `endCamera` (`0072e080`) clears it, then the parent is restored (the port's
  follow camera, from the current view, as `0091db80` does).

Tests to write: (a) the entry FOV is 50 when the eye is 21 m or more from the focus, and atan(25/d)
below that; (b) the eye does not change in aim while the
characters move; (c) the focus follows the midpoint; (d) the right vector has z 0; (e) the side coin
matches `Random::coin`.

## 13. Not settled

- The clamp on `r` (decompile lines 73-94, variable reuse) and the exact eye height; `c4` is derived
  from it.
- The meaning of 1.25 in `007581f0` (duration or priority) and the timer; the slow motion's end.
- The ray tests (no visible effect as read; section 9).
- The random's return value in `009186f0` (the decompile drops it, st0).
- Whether H is him (attacker) and T the target: inferred from the asm (the bit-21 object is the one
  locked from ESI); `docs/melee.md` agrees, the ledger line for `0072df10` does not.
- The FOV that results on entry in practice (section 5.10 is a formula, not a number).
- `009193d0`'s condition from `0072e080` (ledger: "when its +0x14c bit 21"), not read here.
- Slots 16 and 22 and the rest of the vtable.
- The pool allocator `005343a0` and the release `00917ce0` (assumed to be allocation and unlink).
- The name hash 0x57dc9078 (slot 11) and the type id 0x768ce890 (not in names.tsv; grep found none).

## 14. Ledger and code corrections found (not made; for the owner)

- the unpublished notes 009186f0: "spectator_camera_path_one" is wrong. This is the finisher's entry
  placement (key 0x768ce890, constructor 00917b40). The CameraSpectator is 00919510 / 0xd63697a6.
- the unpublished notes 0072df10: "target+0x14c" is doubtful: the asm sets bit 21 on the object locked
  from ESI (attacker, `[assumed]`).
- `the unpublished notes:488`: "1.5 passed beside it (007504b0 / 007581f0)": the finisher's call to
  007581f0 passes 1.25 (`0x00b3450c`); the 1.5 may be from 007504b0 (not checked here).
- `core\src\camera\spectator.rs:14-15`: `OTHER_CINEMATIC_KEY` is this finisher (key 0x768ce890).
- the unpublished notes E6 says the finisher is "unread" and gives 009186f0 and 00918fa0
  as the ones to port: this note reads them (sections 5 and 6).
