# Character world probes: the ground and the walls of a character on foot

Read-only static reading (Haiku reader, 2026-10-11). Source: `fn.py` pseudocode, `work\raw_disasm.py`
bytes, `docs/collision-query-masks.md`, `docs/climbing.md`, `docs/state-machine.md`.
Tags: `[game]` (address given), `[assumed]` (what would settle it). Not settled is stated as such.

Query facts used throughout:
- `005be5a0` is `__fastcall`: ECX = world, EDX = mask, stack: start*, end*, list (0 at every site
  here), out*, radius (float). [game] `fn.py 005be5a0 --lines 1-11`.
- Radius 0 means a ray. At the sites below the decompiler shows radius 0 and the disassembly
  confirms it where checked: `fldz` then `fstp [esp]` overwrites the `push ecx` placed before it.
  [game] 00737120 at 0073719a / 007371b7; 007c84c0 at 007c84fd / 007c8512; 007c86a0 at 007c870f /
  007c8712; 0084cee0 radius literal 0 in the pseudocode (fldz at 0084d276 per the masks note).
- Out record (out* = `param_6`): `param_6[7] = 0x7f7fffff` (FLT_MAX) before the query, so FLT_MAX
  means no hit. [game] line 12. Hit point at out[0..2] [assumed] from the copies in the callers
  (00737120 copies out[0..2]; 0087a200 reads out[2] as z).

## Summary

| function | purpose | segment | radius | mask | use of result | how sure |
|---|---|---|---|---|---|---|
| 00737120 | per-character latched ground snap (`+0x38` latch, runs when `+0xa4` set) | vertical: (x, y, z+5) down to (x, y, z-20), x y z of the object's position | 0 (ray) | 0x14 GROUND \| AVATAR_PUSH | hit point copied to locals; then `FUN_00737720` (unread) | segment and mask [game]; purpose [assumed] |
| 0084cee0 (env test 370a5fba) | "is there walkable floor ahead and no wall": env test for a ConnectionEnvironment | A: pos + 0.2 up to B = A + 5 forward (wall ray); C = B - 3 z (floor ray) | 0 (ray) x2 | 0x1d GROUND \| AI_PUSH \| AVATAR_PUSH \| PUSH (both) | result true iff navmesh test (`FUN_006f0950`, mask 0x10) and no wall hit and floor hit | [game] segments; axis convention [assumed]; env test name not resolved |
| 0087a200 (stateClimbPullup) and 0087dde0 (stateJump) | ledge catch: landing on the top of a climbable wall | sphere sweep at (X, Y, T.z + facing*2.25r + height) down to (X, Y, his z) | his radius (sphere) | 0x5d | hit z in (his z, his z+1.5), below start, > 0.25 from start: reposition | [game]; already in climbing.md:189 and state-machine.md:315 |
| 00872980 (INTERACT_TYPE_VIP branch) | snap an interaction object's z to the ground | vertical: (x, y, z+5) to (x, y, z-5) | 0 (ray) | 0x55 PLAYER_PUSH \| GROUND \| AVATAR_PUSH \| PUSH | if hit, z := hit z (no fraction check) | [game] |
| 007c84c0 | avatar step test: contact and events along one motion step | P to P + V*dt (dt = XMM3) | 0 (ray) | 0x45 PLAYER_PUSH \| AVATAR_PUSH \| PUSH | hit object checked (FUN_0070a460, FUN_007c8be0, unread), events, returns 0 or 1 | segment and mask [game]; meaning not settled |
| 007c86a0 | can the avatar go from its position to a target point (a move-block test) | this position to param_2 (target) | 0 (ray) | 0x45 | returns 1 when hit, out fraction not 0 and not FLT_MAX, plus character_object_lookup check | [game] segment and mask; meaning [assumed] |

## 00737120: the ground snap of a character (best ground-probe candidate)

Purpose [assumed]: a per-update latch. `[esi+0x38]` (per character) is tested and set to 1 at once;
the body runs only when `[esi+0xa4]` is nonzero (its meaning not read). [game] 00737120-0073713a.
The two `FUN_00550930` calls take the object itself (`eax = esi`) and return a pointer whose
`+0x24,+0x28,+0x2c` is the position (the same as `[game]` in `fn.py`'s pseudocode). One call's
result is discarded [game] at 00737151. [assumed] `FUN_00550930` returns the object's transform.

Sweep [game] 0073715d-007371e2:
- start = (P.x, P.y, P.z + 5.0) with the constant `[0xb3476c] = 5.0` (0x40a00000);
- end   = (P.x, P.y, P.z - 20.0) with `[0xb347d4] = 20.0` (0x41a00000);
- mask EDX = 0x14 (GROUND | AVATAR_PUSH), `mov edx, 0x14` at 007371cb;
- radius 0 (fldz at 0073719a, fstp at 007371b7; `push ecx` before it is overwritten);
- out = a local record (`local_bc`, 0x1c bytes before `local_a0`).

Use of result [game]: if the sweep hits and `out[7] != FLT_MAX`, the hit point (out[0..2]) is copied
to `local_74..local_6c`. Then `FUN_00737720(param_1, local_dc, [param_1+0x4c], 0,0,0, [param_1+0x94])`
is called unconditionally (unread). The copied point is not visibly passed on; `local_dc` is the
argument that may carry it [assumed]. Whether the hit z sets the character's height is NOT visible
here; it is settled by reading `FUN_00737720` (unread).

Callers: `fn.py callers 00737120` printed nothing in this run; not settled.

## 0084cee0: the environment test with a floor-ahead and wall-ahead pair

Purpose: the `ConnectionEnvironment` tests of `FUN_0084cee0`, already written for `envTest`
96c5dbae (climb). This block is the branch for the hash `0x370a5fba` [game] (line 108 of the
pseudocode). Name not resolved: `tools\tf2_names.py lookup 370a5fba` printed its help, not a name.

Matrix [assumed]: `character_transform` gives a row-vector 4x3 matrix: row0 = m[0..2], row1 = m[3..5],
row2 = m[6..8], translation m[9..11]. Row1 is taken as forward (y), row2 as up (z), per Space in CLAUDE.md.
Points [game] (pseudocode lines 113-121, `*0.0` terms dropped by the decompiler):
- A = T + 0.2 * row2 (0.2 m above the position);
- B = A + 5.0 * row1 (5 m ahead, same height).

Sweeps [game]:
1. `FUN_005be5a0(A, B, 0, out, 0)`: mask EDX = 0x1d (0084d295), radius 0. The WALL test:
   a ray at 0.2 m above the feet, 5 m forward.
2. `FUN_005be5a0(B, C, 0, out, 0)` with C = (B.x, B.y, B.z - 3.0): mask 0x1d (0084d2f4), radius 0.
   The FLOOR test: a ray 3 m down from the far point.

Use [game] (pseudocode lines 122-131): `local_215 = navmesh && !wall_hit && floor_hit`, where
`navmesh = FUN_006f0950(world, 0x10, 0, 0)` (mask 0x10 = GROUND only; a NavMesh query by its ledger
name; unread). So the env test is true when there is a walkable floor 5 m ahead, with nothing at
0.2 m in front. This is a walk-or-drop test for a forward step; not the step-up or the foot probe.

Caveat [assumed]: the test is gated by its hash, so it applies only to the connection that uses
`0x370a5fba` (which connection is not read; `bnxglobal.str#16` would settle it).

## 0087a200 and 0087dde0: the ledge catch (already written; checked here)

Both are the same catch: `0087dde0` for stateJump (written up: `climbing.md:189`,
`state-machine.md:315`), `0087a200` for stateClimbPullup (its test is hash 0x61e13555 `stateClimbPullup`, first line of the function [game]).
The 0087a200 sweep [game] (pseudocode lines 42-53 of `--full`): start = (X, Y, Z + h), end = (X, Y, his z),
with (X, Y, Z) = T + F*2.25*r, h from `FUN_0070abe0`, radius = his radius (`fStack_54`) [assumed from
the 0087dde0 note], mask 0x5d (0087a4ed; `mov edx,0x5d`). The hit test is the same: hit z in
(his z, his z + 1.5), below his z + h, and more than 0.25 from the start. Then `FUN_00854790` and
(in 0087dde0) `FUN_00855940`. Not a ground or wall probe for a walking character: it runs only in
the climb states.

## 00872980: the VIP interaction snap

`INTERACT_TYPE_VIP` branch [game] (pseudocode lines 35-46): a vertical ray from (x, y, z+5) to
(x, y, z-5) (start is the `uStack_70` point, end `uStack_88`), radius literal 0 (ray), mask 0x55,
out = `auStack_2c`. If it hits (no check of the fraction), `param_1+0x24` (z) := `uStack_24`, which is
out[2]. [game] by offsets. This is the snap of an interaction object (VIP), not the avatar's
ground probe. The function's other branches (FLIPPABLE, MegatronFlight) are not read.

## 007c84c0: one motion step's contact test (not a ground or wall probe, meaning not settled)

Segment [game] (pseudocode line 5-8; disasm 007c84ff-007c855a): start = P (`param_2`), end =
P + V * dt (`param_3` is V, `in_XMM3` is dt). The decompiler's radius (`[this+0x2c]+0xc`) is
pushed, but `fldz` / `fstp` [esp] overwrite it: radius 0 (ray) [game]. Mask EDX = 0x45 (007c8549).
If the hit record (`local_4`) is nonzero, the hit's owner (`local_4+0xc`, `+0x10`) goes to
`FUN_0070a460` and `FUN_007c8be0` (both unread), then `FUN_00562970` / `FUN_00557150` (event
calls, unread) and the return is 1 or 0. Verdict: a per-step contact or event sweep for the avatar;
not settled whether it is a wall test.

## 007c86a0: a move-target obstruction test (meaning [assumed])

Segment [game]: start = this position (`in_EAX`), end = `param_2` (the target). Returns 0 when the
target equals the position. Mask EDX = 0x45 (007c871d); radius 0 (fldz at 007c870f, fstp at 007c8712).
If the hit record `local_4` is nonzero and the owner's fields pass (`character_object_lookup`,
height check), and `local_8` (fraction) is neither 0 nor FLT_MAX, it returns 1 [game]. [assumed]
meaning: "the way to the target is blocked by a solid at a distance" (a wall test for a move). Callers
not read; settled by reading one caller (for example `fn.py callers 007c86a0`, not run).

## Answers

- Ground probe of a walking character: 00737120. Segment (x, y, z+5) to (x, y, z-20), radius 0, mask
  0x14 (GROUND | AVATAR_PUSH). Its use beyond the copied hit point is in the unread `FUN_00737720`. [assumed]
  that it is the walking ground probe, because it is the only vertical ground-mask ray around the
  character's own position.
- Wall test: 0084cee0 (inside env test 370a5fba), the ray from pos + 0.2 up to 5 m ahead, radius 0, mask
  0x1d; with a floor ray 3 m below the far point, under a NavMesh gate. Second candidate: 007c86a0 (ray
  from the position to a target, mask 0x45). Neither is confirmed as a walker's wall test: the callers
  of the env test and of 007c86a0 are not read.

Not settled: `FUN_00737720` (the consumer of the ground hit), `FUN_00550930`'s identity, the meaning
of `+0xa4` and `+0x38` in 00737120, the env test's name and connection, 007c84c0's return meaning,
007c86a0's callers, `FUN_006f0950` (NavMesh, mask 0x10).

## Leads not read

- `FUN_00737720` (called by 00737120 with the ground hit's locals): settles what the ground probe does
  with the hit (height snap, grounded flag).
- Callers of 00737120: which character state runs the latch.
- The hash 370a5fba's name (data object `bnxglobal.str#16` or `names.tsv`) and the connection that uses it.
- `FUN_006f0950` (NavMesh query with mask 0x10).
