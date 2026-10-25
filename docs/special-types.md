# Special types 0 to 3 (Optimus, OptimusFlight, DComScout, DComSniper) (read, 2026-10-12)

Scope: the four `specialAbilityType` values that `character_special_type_switch@0072afd0` (the special start) reads and that are not yet ported. Type 4 (Megatron) is in `docs/special-weapons.md`; -1 (weapon or script specials) in `docs/character-specials.md`. Bumblebee (type -1) is not covered. Nothing was run; nothing was written to the install; no Rust was edited.

Tags: `[game]` read in code (address given), `[data]` read from `export\data\global_states.txt` or `names.tsv` (line given), `[assumed]` a guess (why; what would settle it), `[unknown]` not found.

Ledger notes written this pass (`fn.py note`): `0071f4d0` character_speed_scale_set, `00730e90` special_dash_flag_set, `0072b980` special_timer_end, `0079b5e0` and `0079b630` (the Optimus multiplier getters), `0072e540` special_status_clear.

## Shared parts (all four types) [game]

S0. `0072afd0` is entered from the overlay `stateAttackSpecial` (`stateMachineGenericSpecial`, global_states 8287; `overlayState` true, `playAnimOnce` true, no finish flags, empty `animSetName`, so the clip is the character's own) [data]. The button is AttackSpecial (`connectionAttackSpecial`, character-specials.md).

S1. Gate `FUN_007290a0(&local_140)`: the start runs only when it returns a non-zero handle (lines 5 to 17 of the decompile); what the gate tests is [unknown].

S2. Cooldown: `FUN_0072bc40` copies `attr+0xbc` (`specialAbilityCooldown`) into the timer's `+0x84` and `+0x88`. Data: Optimus 29.4, OptimusFlight 29.4, DComScout 20, DComSniper 15 [data, global_states 65817, 66200, 52925, 53483].

S3. Weapon: `special_weapon_fire_select` (`FUN_007b3d80`) with name `attr+0xd4`. All four have name 0 [data], so nothing is fired from the start.

S4. Self trigger (walk `attr+0xc8`, car `attr+0xdc`): when true, message `attr+0xcc` (walk) or `attr+0xe0` (car) with `Trigger` (`0xd5d636c1`) is sent through `FUN_00724940` / `FUN_007254a0` [game, lines 39 to 56]. Data: Optimus and OptimusFlight use `specialAttack` (`666a6ad0`, decimal 1718250192, names.tsv); DComScout uses `FX_superjump` (`b2c37158`, decimal -1295814312, names.tsv); DComSniper has no self trigger [data].

S5. The switch on `attr+0xc0` (the Type). Cases below.

S6. HUD: since `attr+0xc0 != -1` for these types, `send_ui_integer` is sent with `0x84be0e49` and `0x302517b4` (both 1) [game, lines 138 to 151]. The two hashes are not in `names.tsv` [unknown]. Same as the Megatron path.

S7. Duration: if `attr+0xc4 > 0`, `char+0x8c = attr+0xc4` [game, line 152 to 154]. Data: Optimus 10, OptimusFlight 10, DComScout 5, DComSniper 9 [data]. `+0x8c` is the special timer: `I_SPECIAL_ON` is 1 while it is above 0 (`animation_integer_variables` 0070cae0, ledger), the melee scaling reads it (melee.md 87 to 90), and the shield reads it (below).

S8. End: the timer runs down in `damage_timers_update` (`0072a550`, notes only), and `0072b980` (`special_timer_end`) runs at the end (step E below) [game; the caller of `0072b980` at timer zero is per docs/weapon-mode.md 976 to 978].

S9. The end (`0072b980`, read in full) [game]:
- If the walk self-trigger is on and `attr+0xcc` is set, send `9caa723d` (the stop hash, "off") to `attr+0xcc`; same for the car (`attr+0xdc`, `attr+0xe0`) [lines 9 to 22 of the function].
- Switch on `+0xc0`: per type below. Then `+0x8c = 0` [line 146].

## Type 0: Optimus, ground (`characterAttributesOptimus` 65775, type `false` = 0)

Who [data]:
- `characterAttributesOptimus` (65775): cooldown 29.4, Type `false` (0), duration 10, SelfTrigger true, trigger `specialAttack` (1718250192); car: same; no weapon; `specialAbilityIcon` true.
- `AI_characterAttributesOptimus` (43913 block; the line of `specialAbilityCooldown` is 43959): cooldown 42, duration 8, self on. Not checked further: [unknown] whether the AI variant is a different Type (grep shows type `false`).
- `MP_characterAttributesOptimus` (74546): cooldown 24, duration 4, self on (the MP values; `MP_` set used in multiplayer only).

Steps:
- O0-start [game] `0072afd0` S1 to S4 (cooldown, self trigger `specialAttack`).
- O1 [game, case 0, lines 58 to 73]: `FUN_0072e540` (`special_status_clear`, noted): clears the character's own statuses at the start: `stun_effect(0)`, `FUN_0072c120(0)`, `FUN_0072c340(0)`, `flashbang_effect(0)`, `slow_clear` (clock, speed, animation scale), `FUN_0072adb0`. So Optimus's special cancels stun, flashbang and slow [game]. Which statuses `FUN_0072c120`, `FUN_0072c340` and `FUN_0072adb0` are: [unknown] (only the first three names are known from the ledger).
- O2 [game]: message `SpecialAttackMesh` (`372917bd`) with `Trigger`, through `FUN_00724940`.
- O3 [game]: if `FUN_00725a40` (true when the character's object at `char+0x100` exists and its `+4` test passes, i.e. it is a live effect holder) then spawn the effect `OptimusSpecial` (`d2ff125a`, names.tsv "OptimusSpecial") with `thunk_FUN_01194410` and `FUN_00771230`. The effect's own content is [unknown] (it is in the pack).
- O4 [game]: `FUN_007254a0` releases the message handle.
- O5 [game, timer]: `+0x8c = 10`.
- O6 [game, shield, `00719830` lines 104 to 126 and 216 to 220; `00721b90`]: while `ground_shield_active` is true (`attr+0xc0 == 0` and `char+0x234` non-null and `+0x8c > 0`), incoming damage is multiplied by `optimusSpecialDamageMultiplier` (data 0.5, `MP_` 0.5; global_tuning.txt 45 and 46) unless the damage flag bit 23 is set. The absorbed part (before minus after) is sent to the UI as `GAME_STAT_OPTIMUS_ABSORB_W_SHIELD` (`31276a8d`). Also: damage is clamped so that health does not go below 1.0 (`health - damage <= 1.0` then `damage = health - 1`), the same clamp as the damage notes give for types 0 and 1 (damage.md 33 to 36). The bit 28 test in front of the ground branch (`local_60 >> 0x1c`) is [unknown].
- O7 [game, melee]: the attacker's damage x `optimusMeleeSpecialDamageMultiplier` (1.5; `MP_` 2) while kind 0 and `+0x8c > 0` (`FUN_0079b680` / `FUN_0079b6d0`, melee.md 87 to 90).
- O8 [game, end, `0072b980` case 0]: send `specialAttackEnd` (`874d4b6c`, names.tsv not checked [unknown]) with Trigger; send `NormalMesh` (`1b44a61f`) with Trigger; if `local_68[0x96]` (the second-object flag) is non-zero, send `NormalMesh` again; stop the effect `OptimusSpecial` (`thunk_FUN_01194410`, `thunk_FUN_011832a0`) when `FUN_00725a40`. Then `FUN_007254a0`.

Notes on the numbers: duration 10 and cooldown 29.4 are data [data]; the 0.5 shield and 1.5 melee are data (global_tuning.txt 45, 51), read through the getters `0079b5e0` (`[assumed]` mapping: the getter's float return is dropped by the decompiler; the value it returns is the shared SP or MP value; settled by reading its disassembly or the float at `0x0079b5e0`).

Port needs: the cancel of stun / flashbang / slow at start; the spawn of the `OptimusSpecial` effect (the effect data is in the pack, not read); the shield (x0.5 damage, health floor 1.0) and the melee x1.5 while `+0x8c > 0`; the absorb stat (optional); the end messages.

Not found: the state the overlay enters (generic `stateMachineGenericSpecial`; whether Optimus uses `stateMachineSpecialCustomAnim` (`SpecialAnimSet`, global_states 11183) is [unknown] because `stateMachineList` of Optimus is `<see object at 0x9510>` and its pointers are not resolvable from the export); the content of `OptimusSpecial` and `SpecialAttackMesh`; `specialAttack` script handler; the meaning of `FUN_0072c120` / `FUN_0072c340` / `FUN_0072adb0`.

## Type 1: OptimusFlight (`characterAttributesOptimusFlight` 66155, type `true` = 1)

Who [data]:
- `characterAttributesOptimusFlight` (66155): cooldown 29.4, Type `true` (1), duration 10, SelfTrigger true (`specialAttack`), car same; `specialAbilityIcon` false; `canFly` true is not in this block (not checked).
- `MP_characterAttributesOptimusFlight` (74916): cooldown 30, duration 4, self on.
- The Type value `true` taken as 1 [assumed]: the switch needs an integer, and the Bumblebee notes read booleans as 0/1 (character-specials.md line 76 says the same). Settled by the loader's bool store (`formats` of the attribute loader, not read).

Steps:
- F0 [game] same as O0 to O4 (the start clears statuses, sends `SpecialAttackMesh`, spawns `OptimusSpecial`). Case 1 (lines 74 to 89) differs by:
- F1 [game, case 1, line 75]: `FUN_00730e90(1)` (`special_dash_flag_set`): `FUN_00544db0` gives the character pair; the bit `0x20000000` of `+0x14c` is set on the object at `local_10`, and cleared on `FUN_00730f40`'s object (`local_8`). [assumed] `local_10` is the character and `local_8` the other form object (the Dash/Fly mode holder); what bit 29 of `+0x14c` does is [unknown] (`FX_heal_OptimusF_special` uses bit 30 of the same word, `0072bdb0`, a visual).
- F2 [game] `+0x8c = 10`.
- F3 [game, shield, `00721bd0`]: while `flight_shield_active` (`attr+0xc0 == 1` and `char+0x234` non-null and `+0x8c > 0`), damage x `optimusFlightSpecialDamageMultiplier` (data 0.5; `MP_` 0.6; global_tuning.txt 47 and 48) via `0079b630`, unless bit 23 is set. Then the same absorb stat and the same health floor 1.0 as O6.
- F4 [game, transfer]: `optimusFlightSpecialDamageTransferMultiplier` 0.75 and `optimusFlightSpecialDamageTransferRadius` 200 (global_tuning.txt 49, 50) exist in the data, but their hashes (`5ad6d7c0`, `29f19f5f`) do not appear in any read function; the flight damage branch at 0x719830 lines 128 to 150 calls `0079b630` for the scale and the absorb only. Where the transfer is applied: [unknown].
- F5 [game, melee]: melee x `optimusFlightMeleeSpecialDamageMultiplier` (1.75; `MP_` 1.5) for kind 1 (melee.md 87 to 90).
- F6 [game, end, `0072b980` case 1]: `FUN_00730f40(local_60)`; if its object equals the character (`local_60[0] == local_68`), `FUN_00730e90(0)` clears the bit (F1). Then `FUN_00562970()` (called here and in the explode / destroy path in notes; meaning [unknown] in this function), then the same messages as O8 (`specialAttackEnd`, `NormalMesh`, stop `OptimusSpecial`).

Port needs: the same as Optimus ground, plus the dash-bit flag on the flight holder, and the 0.5 flight shield and 1.75 melee. The transfer (F4) and the bit-29 meaning are open.

Not found: the flight special's own motion (`UseSpecialFlightMode` is in the block, `specialAbility...` field 14; value not read; [unknown] what it selects); the state (generic `stateMachineListOptimusFlight`, 16803, list of pointers only; [unknown]); `FUN_00562970` in this context.

## Type 2: DComScout (`characterAttributesDComScout` 52883, type `#00000002` = 2)

Who [data]:
- `characterAttributesDComScout` (52883): cooldown 20, Type 2, duration 5, SelfTrigger true with `FX_superjump` (`b2c37158`), car same; `specialAbilityIcon` false.
- AI scouts (`targets.rs` stand-in brains; `DComScout_01` in mission-join.md) use the same block [assumed].

Steps:
- D0 [game] `0072afd0` S1 to S4: cooldown 20; self trigger `FX_superjump` with `Trigger` (the walk message is sent before the switch, lines 39 to 56).
- D1 [game, case 2, lines 91 to 93]: `(**(code **)(*local_148 + 0x1c))(0x3fc00000)`: the vtable slot 0x1c (index 7) of the character object is called with 1.5 (`0x3fc00000` is the float 1.5, checked by the literal). Base-class vtable `00b5d56c` (the constructor `00713300` writes it, `pe_read.py vtable`) slot 7 = `0071f4d0` (`character_speed_scale_set`) [game]. The DComScout's runtime vtable is assumed to be the base one [assumed]: settled by its own constructor (not read).
- D2 [game] `0071f4d0` (disassembly checked, `raw_disasm.py 0x71f4d0 0x71f520`): `char+0xd8 = s` (1.5); if child `+0x3c` is non-null, `child+0x1c = char+0xcc * s`; if child `+0x54`, `child+0x1c = char+0xd0 * char+0xcc * s`; if child `+0x5c`, `child+0x1c = char+0xd4 * char+0xcc * s`; if `char+0x258` (`600`) and `FUN_00724850` returns an object, that object's `+0x5c = s` and its children `+0x44` `+0x1c = s`, `+0x3c` `+0x1c = +0x5c`. So the call sets the character's speed scale to 1.5: the three child objects (probably the animation, the move, the motion blend; their names are [unknown]) get 1.5 times their base rates [game; the names of the children are [assumed]].
- D3 [game] `+0x8c = 5`.
- D4 [game, end, `0072b980` case 2]: `(**(code **)(*local_68 + 0x1c))(0x3f800000)`: the same slot with 1.0 (`0x3f800000`) resets the scale; then `+0x8c = 0` and the `FX_superjump` off message (S9).

Meaning: DComScout's special, as the code shows, is a 5 s speed-up to 1.5 times, plus the `FX_superjump` self trigger. The code does NOT set a jump height or a jump velocity in this switch. The name suggests a jump, so the jump is in the effect script `FX_superjump` (b2c37158, in the pack, not read). [assumed] the super jump is that script's job.

Port needs: a 1.5 speed scale on the scout's animation rates (the three children) for 5 s, reset to 1.0 at the end; the `FX_superjump` script is needed to know whether the jump itself changes height, and whether motion (not just animation) is scaled by `char+0xd8` (not checked: `+0xd8` readers not found in this pass).

Not found: the state the scout uses (`stateMachineGenericSpecial`-family, list `<see object at 0x9450>`, unresolvable; [unknown]); the scripted jump (`FX_superjump` script in the pack; [unknown]); whether `char+0xd8` scales the move speed.

## Type 3: DComSniper (`characterAttributesDComSniper` 53441, type `#00000003` = 3)

Who [data]:
- `characterAttributesDComSniper` (53441): cooldown 15, Type 3, duration 9, SelfTrigger false, no weapon, `specialAbilityIcon` false; `canFly` true (53476).
- `brainDComSniper` (32721, AI brain; stand-in in the scouts of the code [assumed]).

Steps:
- R0 [game] `0072afd0` S1 and S2, S4 skipped (no self trigger): cooldown 15.
- R1 [game, case 3, lines 94 to 100]: message `b6ff7243` (decimal -1224773053; NOT in names.tsv [unknown]) with `Trigger`, through `FUN_00724940`, then `FUN_007254a0` (the goto `LAB_0072b446`).
- R2 [game] `+0x8c = 9` (line 152 to 154).
- R3 [game, end, `0072b980` case 3]: `NormalMesh` (`1b44a61f`) with `Trigger` (and again if `local_68[0x96]` is set). No `specialAttackEnd`, no off-message (there is no self trigger). `+0x8c = 0`.

Meaning: the function does not change the character's own state, speed, shield or camera. All of the effect is in what listens for `b6ff7243` (the character's scripts, in the pack). [unknown] that it is a sniper scope: the camera call `camera_is_sniping` (`0117e860`, ledger: "true when the sniper camera is in use") is not called from this path, and the sniper camera is the player's; the candidate is that `b6ff7243` starts the DComSniper's rail-gun weapon's aim mode (`DComSniper_RailGunAttribute` 81890) [assumed, not checked].

Port needs: the cooldown 15, the duration 9, the `NormalMesh` end; the effect of `b6ff7243` (script, [unknown]).

Not found: the script behind `b6ff7243`; the state (no `stateAttackSpecial`-family state named for DComSniper in the data; its machine is not resolved); whether the rail gun is involved.

## Summary: what is settled and what is not

| Type | Who | Start (game) | Shield / multiplier (data) | End (game) | Settled | Open |
|---|---|---|---|---|---|---|
| 0 Optimus | Optimus, AI and MP | clear statuses, SpecialAttackMesh, OptimusSpecial effect, timer 10 | ground shield x0.5 (MP 0.5), health floor 1, melee x1.5 (MP 2) | specialAttackEnd, NormalMesh, stop effect | start, shield, end | state; effect content; FUN_0072c120/c340/adb0 |
| 1 OptimusFlight | OptimusFlight, MP | as type 0, plus dash bit 0x20000000 | flight shield x0.5 (MP 0.6), melee x1.75 (MP 1.5); transfer x0.75 r200 not found | as type 0, dash bit cleared, FUN_00562970 | start, shield, end | transfer; bit 29; state; flight motion |
| 2 DComScout | DComScout | FX_superjump self trigger; slot 0x1c with 1.5 (speed 1.5x) | none | slot 0x1c with 1.0; off message | the 1.5x speed (asm checked) | FX_superjump script (jump?); vtable of the scout; move speed |
| 3 DComSniper | DComSniper | message b6ff7243 + Trigger | none | NormalMesh only | start and end | b6ff7243 meaning; state; rail gun |

## Port recipe (for the Rust port of types 0 to 3)

1. On start: cooldown = attr cooldown; self trigger message if set; then per type: 0/1 clear statuses, send SpecialAttackMesh, spawn OptimusSpecial (if the holder is live), timer = duration; 1 also set the dash flag on the flight holder; 2 speed scale 1.5 on the child rates, timer = duration; 3 send b6ff7243, timer = duration.
2. While timer > 0: type 0 ground shield (damage x0.5, health floor 1.0; bit 23 exempts); type 1 flight shield (x0.5 with MP 0.6); melee x1.5 (type 0) or x1.75 (type 1), MP 2 and 1.5.
3. On timer zero or stop: 0/1 send specialAttackEnd and NormalMesh, stop the effect, clear the dash flag (type 1); 2 speed scale back to 1.0; 3 send NormalMesh; every type sends the off message to its self-trigger name.

## Not found / not settled (all types)

- The overlay state for each type: all use the generic `stateMachineGenericSpecial` [data] or a family member, but the per-character `stateMachineList` pointers (`<see object at 0x...>`) cannot be resolved from the export. [unknown]
- `FUN_007290a0` (the start gate), `FUN_0072c120`, `FUN_0072c340`, `FUN_0072adb0`, `FUN_00562970`, `FUN_00724850`: names known from the ledger only [unknown].
- The meaning of `char+0x14c` bit 29 (the dash flag) and bit 28 of the damage flags (`local_60 >> 0x1c`) [unknown].
- Hashes not in `names.tsv` (checked in this pass): `b6ff7243` (type 3 message), `0x84be0e49` and `0x302517b4` (HUD integers). `specialAttackEnd` (`874d4b6c`), `NormalMesh` (`1b44a61f`) and `GAME_STAT_OPTIMUS_ABSORB_W_SHIELD` (`31276a8d`) are only named by the decompiler's comments, not checked against the table. [unknown in data]
- `FX_superjump` and `specialAttack` scripts, `OptimusSpecial` and `SpecialAttackMesh` effects: in the pack, not read.
- The AI and MP variants: the AI Optimus has cooldown 42 and duration 8, the MP Optimus 24 and 4, MP OptimusFlight 30 and 4 [data]; which of them the game uses in single player is [assumed] the plain block.
- `optimusFlightSpecialDamageTransferMultiplier` (0.75) and `...TransferRadius` (200): not applied in any read function [unknown].
- The meaning of the Icon fields (`specialAbilityIcon` true for Optimus only) [unknown].

## Reading record

Functions read in full this pass: `0072afd0` (cases 0 to 4, lines 1 to 200), `0072b980` (whole), `0071f4d0` (whole, disassembly 0x71f4d0 to 0x71f520), `00730e90` (whole), `0072e540` (whole), `00719830` (lines 101 to 150 and 216 to 227, of the full body), `00721b90`, `00721bd0` (whole), `0079b5e0`, `0079b630` (whole; their float returns are dropped by the decompiler). Vtables: `00b5d56c` (character, pe_read).
