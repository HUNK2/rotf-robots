# Collision query masks at the direct call sites

Scope: every direct call of `005be890` (world_ray_sphere_query) and `005be5a0`
(second world query). Read-only static work from the disassembly
(`work\raw_disasm.py`, `tools\fn.py`). Address of each mask instruction is given.

## Argument layout used

- `005be890`: `__thiscall`, ECX = world. Stack args, as the brief states: start*, end*,
  MASK, out*, radius (start on top of the stack at the call, radius deepest). [assumed] from
  the brief, not re-read in `fn.py`; the push order at every site below is consistent with it.
- `005be5a0`: `__fastcall` (decompile header, `fn.py 005be5a0 --lines 1-11`).
  `param_1` = ECX (world), `param_2` = **EDX**, then stack args: `param_3` start*,
  `param_4` end*, `param_5` list, `param_6` out*, `param_7` radius (float).
  Line 11 of that decompile passes `param_2` as the third argument of
  `world_query_gather` (`005b37f0`), so **the mask of `005be5a0` is in EDX, not on the
  stack.** This differs from the brief, which expected a stack argument.
- Radius for `005be5a0` is the float stack argument; stored by `push ecx` + `fstp [esp]`
  (`[game]`) where the value came from an x87 `fldz`.

## 1. Table

Radius column: "0.0 (fldz at X)" means the x87 stack top stored into the radius slot was
loaded by `fldz` at X with no x87 operation in between (checked in the disassembly).
"not determined" means the radius push or its source is outside the disassembled bytes.

### 005be890 (world_ray_sphere_query), 14 call sites

| Call site | Calling function (ledger name) | Mask | Mask bits | Radius | Sure |
|---|---|---|---|---|---|
| 0070842d | 007079f0 (aim_assist_search) | 0x0002 | PUNCH | 0.0 (fldz 00708407, fstp 0070840a) | [game] push 2 at 00708415 |
| 0073016d | 0072fcd0 (collision_attack_search) | 0x0023 | PUSH \| PUNCH \| CAMERA | not determined (no fifth push found in the 0xb0 bytes before the call) | [game] push 0x23 at 00730134 |
| 0073069c | 0072fcd0 (collision_attack_search) | 0x0023 | PUSH \| PUNCH \| CAMERA | not an immediate: fstp at 0073062e takes st0 from `fld [esp+0x94]` at 0073060c (other x87 ops not checked) | [game] push 0x23 at 00730636 |
| 0074caba | 0074c200 (no ledger name) | 0x0003 | PUSH \| PUNCH | 0.0 (fldz 0074ca96, fstp 0074ca9c) | [game] push 3 at 0074caa4 |
| 0074daa7 | 0074d610 (no ledger name) | 0x0003 | PUSH \| PUNCH | 0.0 (fldz 0074da54, fstp 0074da5f) | [game] push 3 at 0074da75 |
| 007837a0 | 007832b0 (gun_round_trace) | 0x0002 | PUNCH | 0.0 (fldz 00783766, fstp 00783769) | [game] push 2 at 00783775 |
| 0078d793 | 0078d560 (no ledger name) | 0x0001 | PUSH | 0.0 (fldz 0078d701, fstp 0078d710) | [game] push 1 at 0078d72a |
| 0078db43 | 0078da10 (snaker_b) | 0x0001 | PUSH | not determined (the out* push is at 0078dabc, the fifth push is before it, outside the disassembled bytes) | [game] push 1 at 0078daed |
| 0078e4cc | 0078e300 (no ledger name) | 0x0001 | PUSH | not determined (first push in the window is out* at 0078e441) | [game] push 1 at 0078e474 |
| 007b26f2 | 007b2310 (weapon_aim_point) | 0x0003 | PUSH \| PUNCH | 0.0 (fldz 007b26c9, fstp 007b26cc) | [game] push 3 at 007b26d4 |
| 00917e25 | 00917dc0 (no ledger name) | 0x0020 | CAMERA | 0.0 (fldz 00917de3, fstp 00917dfa) | [game] push 0x20 at 00917e06 |
| 00919998 | 00919800 (no ledger name) | 0x0020 | CAMERA | 0.0 (fldz 0091997f, fstp 00919982) | [game] push 0x20 at 0091998a |
| 0091bb26 | 0091b770 (no ledger name) | 0xFFFF | all bits | 0.5 (fld [0x00b38830] at 0091bb06, constant 0.5) | [game] push 0xffff at 0091bb19 |
| 00925880 | 009256c0 (camera_collision) | 0xFFFF | all bits | 0.65 (constant 00b6b380) or 0.0 (xorps), chosen by the je at 0092584c; the flags come from before 00925830 and were not traced | [game] push 0xffff at 00925879 |

### 005be5a0 (second world query), 14 call sites; mask in EDX

| Call site | Calling function (ledger name) | Mask (EDX) | Mask bits | Radius | Sure |
|---|---|---|---|---|---|
| 007371e2 | 00737120 (no ledger name) | 0x0014 | GROUND \| AVATAR_PUSH | 0.0 (fldz 0073719a, fstp 007371b7) | [game] mov edx,0x14 at 007371cb |
| 007c855a | 007c84c0 (no ledger name) | 0x0045 | PLAYER_PUSH \| AVATAR_PUSH \| PUSH | not determined (fstp at 007c8512 takes st0 loaded before 007c84bd) | [game] mov edx,0x45 at 007c8549 |
| 007c8722 | 007c86a0 (no ledger name) | 0x0045 | PLAYER_PUSH \| AVATAR_PUSH \| PUSH | 0.0 (fldz 007c870f, fstp 007c8712) | [game] mov edx,0x45 at 007c871d |
| 0084d29a | 0084cee0 (no ledger name) | 0x001D | GROUND \| AI_PUSH \| AVATAR_PUSH \| PUSH | 0.0 (fldz 0084d276, fstp 0084d27e) | [game] mov edx,0x1d at 0084d295 |
| 0084d302 | 0084cee0 (no ledger name) | 0x001D | GROUND \| AI_PUSH \| AVATAR_PUSH \| PUSH | not determined (radius pushed before 0084d2c8, outside the window) | [game] mov edx,0x1d at 0084d2f4 |
| 00872c2a | 00872980 (no ledger name) | 0x0055 | PLAYER_PUSH \| GROUND \| AVATAR_PUSH \| PUSH | not determined (radius pushed before 00872bf5, outside the window) | [game] mov edx,0x55 at 00872c1c |
| 0087a4f2 | 0087a200 (no ledger name) | 0x005D | PLAYER_PUSH \| GROUND \| AI_PUSH \| AVATAR_PUSH \| PUSH | not an immediate: xmm0 returned by the call to 0070ab60 at 0087a4cf, stored at 0087a4d5 | [game] mov edx,0x5d at 0087a4ed |
| 0087e0c8 | 0087dde0 (no ledger name) | 0x005D | PLAYER_PUSH \| GROUND \| AI_PUSH \| AVATAR_PUSH \| PUSH | not an immediate: xmm0 returned by the call to 0070ab60 at 0087e0a5, stored at 0087e0ab | [game] mov edx,0x5d at 0087e0c3 |
| 00915f6e | 00915740 (driving_camera_update) | 0x0020 | CAMERA | not determined (fstp at 00915f24, st0 loaded before the window) | [game] mov edx,0x20 at 00915f51 |
| 00916d3b | 00916c40 (no ledger name) | 0x0031 | CAMERA \| GROUND \| PUSH | not determined (radius pushed before 00916d09) | [game] lea edx,[ecx+0x31] at 00916d32 with ecx = 0 (xor at 00916d2b) |
| 0091701e | 00916c40 (no ledger name) | 0x0031 | CAMERA \| GROUND \| PUSH | not determined (fstp at 00916fdb, st0 loaded before the window) | [game] lea edx,[ecx+0x31] at 00917012 with ecx = 0 (xor at 0091700f) |
| 00917280 | 00916c40 (no ledger name) | 0x0031 | CAMERA \| GROUND \| PUSH | 0.0 (fldz 00917253, fstp 00917256) | [game] lea edx,[ecx+0x31] at 0091726b with ecx = 0 (xor at 00917268) |
| 00925583 | 00924310 (follow_camera_update) | 0x0020 | CAMERA | not determined (fstp at 00925536, st0 loaded before the window) | [game] mov edx,0x20 at 00925566 |
| 00928b8b | 009284e0 (weapon_camera_sponge) | 0x0020 | CAMERA | not determined (radius and out* pushed before 00928b3b, outside the window) | [game] mov edx,0x20 at 00928b62 |

Ledger lookup: names come from the first line of `fn.py <address>`. "no ledger name" means
the ledger had no entry for that function (the header line printed with no name).

## 2. Summary: distinct masks and their callers

005be890 (stack mask):
- 0x0001 PUSH: 0078d560, snaker_b (0078da10), 0078e300. Three sites.
- 0x0002 PUNCH: aim_assist_search (007079f0), gun_round_trace (007832b0). Two sites.
- 0x0003 PUSH \| PUNCH: 0074c200, 0074d610, weapon_aim_point (007b2310). Three sites.
- 0x0020 CAMERA: 00917dc0, 00919800. Two sites.
- 0x0023 PUSH \| PUNCH \| CAMERA: collision_attack_search (0072fcd0), two sites.
- 0xFFFF (all): 0091b770, camera_collision (009256c0). Two sites.

005be5a0 (EDX mask):
- 0x0014 GROUND \| AVATAR_PUSH: 00737120. One site.
- 0x0020 CAMERA: driving_camera_update (00915740), follow_camera_update (00924310),
  weapon_camera_sponge (009284e0). Three sites.
- 0x0031 CAMERA \| GROUND \| PUSH: 00916c40 (three sites).
- 0x0045 PLAYER_PUSH \| AVATAR_PUSH \| PUSH: 007c84c0, 007c86a0. Two sites.
- 0x001D GROUND \| AI_PUSH \| AVATAR_PUSH \| PUSH: 0084cee0 (two sites).
- 0x0055 PLAYER_PUSH \| GROUND \| AVATAR_PUSH \| PUSH: 00872980 (one site).
- 0x005D PLAYER_PUSH \| GROUND \| AI_PUSH \| AVATAR_PUSH \| PUSH: 0087a200, 0087dde0. Two sites.

Site counts: 14 sites for 005be890 and 14 for 005be5a0 (the brief expected about 13 and 11).
All 28 masks are immediates (no register or memory mask). None needed the second lookup.

## 3. Not determined

Mask: none. Every site's mask is an immediate or a constant lea/xor pair.

Radius (not determined, with the reason):
- 005be890: 0073016d (no fifth push found), 0078db43 and 0078e4cc (radius push before the window),
  0073069c (x87 source loaded at 0073060c, intervening ops not checked), 00925880 (branch flags not traced).
- 005be5a0: 007c855a, 00915f6e, 0091701e, 00925583 (fstp takes st0 loaded before the window),
  0084d302, 00872c2a, 00916d3b, 00928b8b (pushed before the window). 0087a4f2 and 0087e0c8
  take xmm0 from the call to 0070ab60, whose meaning was not read.

Callers without a ledger name: 0074c200, 0074d610, 0078d560, 0078e300, 00917dc0, 00919800,
0091b770, 00737120, 007c84c0, 007c86a0, 0084cee0, 00872980, 0087a200, 0087dde0, 00916c40.
Their names were not added (`fn.py note` was not used, per the brief).
