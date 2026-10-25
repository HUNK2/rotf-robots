# Flight attributes: field to offset (modeFly and modeHover), 2026-10-12

Ticket: work/survey/flight_brief.txt. Tags: `[data]` read in export/data/global_states.txt,
`[game]` read in the executable's decompile or disassembly (address given), `[assumed]` not checked.

## The data

Blocks: `characterAttributesBlackout` (line 51100), `characterAttributesBumblebee` (51508),
`characterAttributesStarscream` (68437; next top-level block TestAI at 68812). `[data]`

modeFly: Blackout line 51270, Bumblebee 51678, Starscream 68607.
modeHover: Blackout 51415, Bumblebee 51795, Starscream 68724.

Values, in file order (all `[data]`). Byte offset = 4 x the index the loader stores at.

| off | field | Starscream | Blackout | Bumblebee |
|---|---|---|---|---|
| 0x00 | lStickDeadzone | 0 | 0 | 0 |
| 0x04 | thrust | 50 | 40 | 20 |
| 0x08 | lateralThrust | 55 | 60 | 55 |
| 0x0c | turboThrust | 75 | 40 | 40 |
| 0x10 | lift | 30 | 60 | 30 |
| 0x14 | liftQuick | 40 | 50 | 50 |
| 0x18 | maxSpeed | 45 | 40 | 100 |
| 0x1c | maxTurboSpeed | 65 | 60 | 125 |
| 0x20 | maxTurboTime | 2.5 | 2 | 3 |
| 0x24 | maxTurboCooldownTime | 5.5 | 5 | 3 |
| 0x28 | turnSpeed | 2.8 | 2.5 | 1 |
| 0x2c | maxTurnSpeed | 1.5 | 2 | 1.5 |
| 0x30 | quickTurnSpeed | 3.8 | 2 | 2 |
| 0x34 | turnLag | 1.5 | 4 | 0.15 |
| 0x38 | turnRestoreLag | 4 | 1.95 | 4 |
| 0x3c | turnStickThreshold | 0.25 | 0.25 | 0.25 |
| 0x40 | pitchSpeed | 3 | 1 | 1 |
| 0x44 | maxPitchSpeed | 1.5 | 1.5 | 1.5 |
| 0x48 | quickPitchSpeed | 2 | 2 | 2 |
| 0x4c | pitchLag | 3 | 0.5 | 0.15 |
| 0x50 | pitchLagAutoLevel | 1.5 | 0.5 | 0.15 |
| 0x54 | pitchRestoreLag | 4 | 4 | 4 |
| 0x58 | maxPitch | 65 | 30 | 20 |
| 0x5c | minPitch | -65 | -30 | -20 |
| 0x60 | maxRoll | 70 | 40 | 25 |
| 0x64 | maxRollQuick | 90 | 50 | 50 |
| 0x68 | pitchStrength | 2.2 | 1.8 | 1.5 |
| 0x6c | rollStrength | 2.5 | 2 | 1.5 |
| 0x70 | rollQuickStrength | 2.5 | 3 | 3 |
| 0x74 | quickRollLag | 4 | 5 | 4 |
| 0x78 | quickClimbScale | 0.6 | 0.5 | 0.5 |
| 0x7c | quickClimbLag | 2.2 | 4 | 2 |
| 0x80 | bulletSlowSpeedPercent | 0.8 | 0.8 | 0.8 |
| 0x84 | helicopter (bool) | false | true | false |
| 0x88 | helicopterTurboTurnFactor | 5 | 5 | 5 |
| 0x8c | aiTargetHeightOffset | 1 | 1 | 1 |
| 0x90 | aiApproachTargetDistance | 25 | 25 | 25 |
| 0x94 | aiApproachTargetHeight | 25 | 25 | 25 |
| 0x98 | aiApproachTargetDeceleration | 2 | 2 | 2 |
| 0x9c | aiStrafeTargetDistance | 100 | 100 | 100 |
| 0xa0 | aiFeelerLength | 30 | 30 | 30 |
| 0xa4 | stuntMoveButtonPressTime | 0.2 | 0.2 | 0.2 |
| 0xa8 | stuntMoveStickThresholdX | 0.5 | 0.5 | 0.5 |
| 0xac | stuntMoveStickThresholdY | 0.9 | 0.9 | 0.9 |
| 0xb0 | robot (bool) | false | false | false |
| 0xb4 | robotAdditiveSteerStrength | 2 | 2 | 2 |
| 0xb8 | canDoStunts (bool) | true | true | true |
| 0xbc | aileronScale | 0.25 | 0.3 | 0.3 |
| 0xc0 | elevatorScale | 0.1 | 0.2 | 0.2 |
| 0xc4 | rudderScale | 0.25 | 0.2 | 0.2 |
| 0xc8 | brakeDuration | 3 | 3 | 3 |
| 0xcc | brakeCooldown | 1 | 1 | 1 |
| 0xd0 | brakeSpeedScale | 0.5 | 0.5 | 0.5 |
| 0xd4 | postTurboDeceleration | 20 | 20 | 20 |
| 0xd8 | brakingSpeed | 20 | 20 | 20 |
| 0xdc | minRotorAlpha | 0.25 | 0.25 | 0.25 |
| - | collisionSSD {Radius, NumFrames, NoiseMapSizeRatio, Chaos, Cracks/Damage Ext/Int Threshold and Softness} | R 5, N 5, NMSR 0.418, Chaos 2 | R 7.7, N 2, NMSR 1.022, Chaos 2.291 | R 7.7, N 2, NMSR 1.022, Chaos 2.291 |
| - | bladeList | empty | 14 blade names | empty |

modeHover (struct at attr+0x560, loader 0085bf90):

| off | field | Starscream | Blackout | Bumblebee |
|---|---|---|---|---|
| 0x00 | maxHoverTime | 20 | 20 | 10 |
| 0x04 | moveAcceleration | 100 | 80 | 50 |
| 0x08 | maxMoveSpeed | 10 | 10 | 10 |
| 0x0c | hoverDoneParticlesPct | 0.9 | 0.9 | 0.9 |
| 0x10 | hoverDeceleration | 0.95 | 0.95 | 0.95 |
| 0x14 | maxLeanDegrees (stored x 0.017453292) | 7 | 7 | 7 |
| 0x18 | leanStrength | 5 | 5 | 5 |

`[data]` The data also has the Bumblebee `helicopter: false`, `canFly` false (Bumblebee has the
block but FlyMode is not created for him, per docs/bumblebee-mechanics.md). The earlier survey
table in the unpublished notes agrees with these values where they overlap.

## The loader

- `FUN_00726710` (character_attributes_load), thiscall: `this` (ecx) is saved in ebx, so
  ebx is the attributes object. `[game]` raw disassembly 00726710-00726727.
- Flight: `FUN_0085aea0` is called at `00728326`. Before the call: `00728319 call 0x573940`
  (data lookup of the hash `0x7a638529` = `modeFly`, set at `0072830a`), `00728325 push edx` with
  `0072831f lea edx,[ebx+0x2e0]`. So the struct is `attr+0x2e0`. `[game]`
- Hover: `FUN_0085bf90` is called at `0072841b`, after `00728412 lea edx,[ebx+0x560]`
  and `00728400` (hash `0x98155998` = `modeHover`). Struct is `attr+0x560`. `[game]`
- The loaders are cdecl with the struct as the first argument. The data node is the second
  argument (in the flight case, from the lookup at 00728319). `[game]`

How the hash was found: `work/find_imm.py 0x7a638529` gives 00728310 in FUN_00726710;
`0x6f1c1425` (maxHoverTime) gives 0085bf9e in FUN_0085bf90; `0xc400c43e` (maxPitch) gives
0085b4ef in FUN_0085aea0. Field hashes come from the `data_field_by_hash` constants in the
loader's decompile (`fn.py 0085aea0 --full`), and match `export/data/names.tsv`.

## Field to offset

Hash is CRC-32 of the name, as stored in the loader (case sensitive).

modeFly (loader FUN_0085aea0, base = struct at attr+0x2e0; loader's stores are `param_1[n]`, so
byte offset is 4n):

| field | hash | off in struct | type |
|---|---|---|---|
| lStickDeadzone | 710c337c | 0x00 | float |
| thrust | 48458f44 | 0x04 | float |
| lateralThrust | 21d6ad33 | 0x08 | float |
| turboThrust | fa528509 | 0x0c | float |
| lift | 737d1e0c | 0x10 | float |
| liftQuick | 610b2d48 | 0x14 | float |
| maxSpeed | ecb98411 | 0x18 | float |
| maxTurboSpeed | b84d9f9a | 0x1c | float |
| maxTurboTime | b9a9cf3b | 0x20 | float |
| maxTurboCooldownTime | 6bef734a | 0x24 | float |
| turnSpeed | ee3a9044 | 0x28 | float |
| maxTurnSpeed | 332e2154 | 0x2c | float |
| quickTurnSpeed | de08bb0d | 0x30 | float |
| turnLag | 50ff5e12 | 0x34 | float |
| turnRestoreLag | 7e650b26 | 0x38 | float |
| turnStickThreshold | 392578f8 | 0x3c | float |
| pitchSpeed | 9b25b8db | 0x40 | float |
| maxPitchSpeed | 864fbc0e | 0x44 | float |
| quickPitchSpeed | 941573c4 | 0x48 | float |
| pitchLag | b15f00d0 | 0x4c | float |
| pitchLagAutoLevel | a7d0d914 | 0x50 | float |
| pitchRestoreLag | 0d8755c8 | 0x54 | float |
| maxPitch | c400c43e | 0x58 | float |
| minPitch | 22606bdf | 0x5c | float |
| maxRoll | d2c4c746 | 0x60 | float |
| maxRollQuick | 5587d384 | 0x64 | float |
| pitchStrength | d9068dc6 | 0x68 | float |
| rollStrength | 4be97153 | 0x6c | float |
| rollQuickStrength | 8248fbf7 | 0x70 | float |
| quickRollLag | 38b8962c | 0x74 | float |
| quickClimbScale | e761abd3 | 0x78 | float |
| quickClimbLag | f942f610 | 0x7c | float |
| bulletSlowSpeedPercent | 0a13fbfa | 0x80 | float |
| helicopter | a5602651 | 0x84 | bool (byte) |
| helicopterTurboTurnFactor | e8145ab2 | 0x88 | float |
| aiTargetHeightOffset | 9e270201 | 0x8c | float |
| aiApproachTargetDistance | 6eca3715 | 0x90 | float |
| aiApproachTargetHeight | 3ad42b36 | 0x94 | float |
| aiApproachTargetDeceleration | 258b77a4 | 0x98 | float |
| aiStrafeTargetDistance | aa6db19b | 0x9c | float |
| aiFeelerLength | 6ea4073a | 0xa0 | float |
| stuntMoveButtonPressTime | e5eb9f9b | 0xa4 | float |
| stuntMoveStickThresholdX | eb4c17a4 | 0xa8 | float |
| stuntMoveStickThresholdY | 9c4b2732 | 0xac | float |
| robot | d82b7ee4 | 0xb0 | bool (byte) |
| robotAdditiveSteerStrength | 2c631951 | 0xb4 | float |
| canDoStunts | c2a39210 | 0xb8 | bool (byte) |
| aileronScale | 744457f6 | 0xbc | float |
| elevatorScale | 5897a635 | 0xc0 | float |
| rudderScale | fc4658fe | 0xc4 | float |
| brakeDuration | 1d087997 | 0xc8 | float |
| brakeCooldown | 4534d58e | 0xcc | float |
| brakeSpeedScale | 0689f208 | 0xd0 | float |
| postTurboDeceleration | 3c44d487 | 0xd4 | float |
| brakingSpeed | f0f83ad8 | 0xd8 | float |
| minRotorAlpha | 0bc4e1da | 0xdc | float |
| collisionSSD | e520d8c8 | (sub-struct, offset not traced) | struct via ssd_info_load |
| bladeList / bladeName | 39107262 / 23fbf47c | (not traced) | array |

Caution: the `decomp_lookup.py fields` output labels the three bool fields with the array
index (+0x21 helicopter, +0x2c robot, +0x2e canDoStunts). The byte offsets are 0x84, 0xb0,
0xb8 (index x 4). `[game]` 0085aea0 lines 668, 871, 894.

modeHover (loader FUN_0085bf90, base attr+0x560): maxHoverTime 6f1c1425 +0x00; moveAcceleration
d2e53ba7 +0x04; maxMoveSpeed 40be2c4d +0x08; hoverDoneParticlesPct 1fce54d6 +0x0c;
hoverDeceleration 744530fa +0x10; maxLeanDegrees e399ab70 +0x14 (x 0.017453292 at load);
leanStrength 9767b403 +0x18. All floats. `[game]` from the loader's constants and the
decompile output.

The mode object's tuning pointer: in FlyMode's constructor and update the tuning block is
reached as `mode+0x18`, with `mode` the first argument (`param_1`, the owner at mode+4).
`[game]` FUN_00864d60 `*(char*)(*(int*)(param_1+0x18)+0x84)`, `+0xb0`; FUN_00864320
`*(int*)(param_1+0x18)`. The same struct is the one the flight loader writes (the bytes
match). That the value at mode+0x18 IS `attr+0x2e0` (the pointer copied at mode creation) is
`[assumed]`: the creation site (FUN_00851a40 or the mode table code) was not read.

## Proof

Reads in the flight update FUN_00864320 (`ebx` = the FlyMode object, `ebx+0x18` = tuning):

- thrust (+0x04): `008645fa mov eax,[ebx+0x18]`; `0086461d cmp byte ptr [eax+0x84],0` (helicopter
  test); `00864618 movss xmm0,[eax+4]` (the `je 0x864618` at 0086460f picks this path when the
  flag is clear), `0086461d` after. The decompile at lines 80-89 is the same: `fVar10 = *(iVar6+4)`,
  else `*(iVar6+0xc)` (turboThrust), then `if (*(char*)(iVar6+0x84)==0) fVar10 *= param_2`. `[game]`
- maxPitch (+0x58), minPitch (+0x5c): `00864b3e mov ecx,[ebx+0x18]`; `00864b41 movss xmm1,[ecx+0x5c]`
  (minPitch) for the negative branch; `00864b4d mov edx,[ebx+0x18]`; `00864b50 movss xmm1,[edx+0x58]`
  (maxPitch) for the positive branch; then `00864b55 mulss xmm1,xmm2` (xmm2 = 0.0174533) and
  `00864b59 divss xmm0,xmm1`. The result is pitch / (maxPitch x deg-to-rad), passed to the sound
  parameter `pitch` (event_parameter_by_hash 279fbed9). `[game]`
- Extra checks: `00864349 mov eax,[ebx+0x18]`; `0086434c cmp byte ptr [eax+0xb8],0` (canDoStunts).
  `00864b86 mov eax,[ebx+0x18]`; `00864b89 movss xmm0,[eax+0x64]`; `00864b91 mulss xmm0,[0xb6a40c]`
  (roll sound, maxRollQuick x 0.0174533). `[game]`
- Roll: `FUN_008693f0` (decompile only, not checked in asm) reads `[ebx...+0x6c]` (rollStrength) and
  `+0x60` (maxRoll) x 0.017453292 times `*param_2`. `[game]` (decompile)

Oddity: `FUN_008676c0` (flight_steer_player) at decompile lines 58 and 63 reads
`*(float*)(tuning+100)` = 0x64 = maxRollQuick, not maxRoll (0x60), for the roll target. The
ledger's wording "+-maxRoll" is wrong for that line, or the quick roll is meant. Not checked in asm.
`[game]` (decompile).

`FUN_00868b00` reads `unaff_EDI+0x58` and `+0x60` through `unaff_EDI+0x18`; its base register is not
identified (EDI is not the mode object here), so it is not used as proof.

## Not found

- maxSpeed (+0x18): no read found in FUN_00864320, FUN_0086b230, FUN_008676c0, FUN_00868b00 or
  the decompile of FUN_0086aa60 (its body was not printed). The three-field proof is two of three.
  FUN_0086aa60 has notes in docs/flight-update.md; not read here.
- The creation site that copies attr+0x2e0 into mode+0x18 (FUN_00851a40 or the mode-table code).
  `[assumed]` it is a plain pointer copy.
- collisionSSD sub-struct offset and the bladeList layout (ssd_info_load and the array loop
  inside FUN_0085aea0 were not traced).
- Any reader of the modeHover block (attr+0x560): the hover mode (HoverMode 0086f960) was not read.
- `FUN_0086b230`'s use of tuning: it reads other struct fields (`+0x2c`, `+0x30` of the owner), not
  tuning fields, in the lines grepped.
