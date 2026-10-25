# CombatRepair special (Ratchet, Long Haul)

Reader ticket, 2026-10-12. Question: how the CombatRepair special heals. Result: the start
path is read and the heal itself is NOT found. The heal lives in a Lux effect script
that is not in the exported text, and the regeneration rates that look like the heal's
numbers are not read by any function found so far. Nothing was run, nothing written to
the install. Tags: [game] read in code or disassembly (address given), [data] from the
text exports, [assumed] stated with what would settle it.

## 1. Data (per character)

Source: `export\data\global_states.txt`, Ratchet block at line 67687
(`characterAttributesRatchet`), Long Haul block at line 60942
(`characterAttributesLongHaul`). Hashes resolved with `export\data\names.tsv` and CRC-32.
[data]

| Field (attribute, `attr+off` from the loader `00726710`) | Ratchet | Long Haul |
|---|---|---|
| specialAbilityCooldown (`+0xbc`) | 25.88 | 29.5 |
| specialAbilityType (`+0xc0`) | #ffffffff (-1, no switch case) | #ffffffff (-1) |
| specialAbilityDuration (`+0xc4`) | 5.34 | 5.34 |
| specialAbilitySelfTrigger (`+0xc8`) | true | true |
| specialAbilityLuxScriptNameToTrigger (`+0xcc`) | -1788599334 = 0x956423da = `CombatRepair` | same |
| specialAbilityWeaponFireFromAnim (`+0xd0`) | false | false |
| specialAbilityWeaponFireName (`+0xd4`) | 0 | 0 |
| specialAbilityVehicleAnim (`+0xd8`) | 0 | 0 |
| specialAbilityVehicleSelfTrigger (`+0xdc`) | true | true |
| specialAbilityVehicleLuxScriptNameToTrigger (`+0xe0`) | -1130744174 = 0xbc9a3692 = `CombatRepairVehicle` | -1788599334 = `CombatRepair` |
| specialAbilityVehicleWeaponFireFromAnim (`+0xe4`) | false | false |
| specialAbilityVehicleWeaponFireName (`+0xe8`) | 0 | 0 |
| specialAbilityIcon (`+0xec`) | #00000002 | #00000002 |

Health fields in the same blocks (these are the ones that look like the heal) [data]:

| Field (loader offset) | Ratchet | Long Haul |
|---|---|---|
| baseHealth (`+0x4c`) | 320 | 360 |
| healthRegenDuration (`+0x50`) | 5 | 5 |
| healthRegenRate (`+0x5c`) | 44 | 50 |
| healthRegenDelay (`+0x58`) | 1.5 | 1.5 |
| healthRegenSecsPerTic (`+0x60`) | -1 | -1 |
| healthRegenRateFromRatchet (`+0x68`) | 60 | 67 |
| healthRegenSecsPerTicFromRatchet (`+0x70`) | -1 | -1 |
| healthRegenRateFromLonghaul (`+0x64`) | 40 | 67 |
| healthRegenSecsPerTicFromLonghaul (`+0x6c`) | -1 | -1 |

Offsets come from the loader `00726710` (`decomp_lookup.py fields`). [game]

Hash checks [data]: `CombatRepair` = CRC-32 956423da, `CombatRepairVehicle` = bc9a3692
(`names.tsv` lines 52308 and 65926). Searching the code for the immediate 956423da
finds nothing (`work\find_imm.py`), so the name is used only as data.

Correction to `docs/character-specials.md`: its table is right for both characters. The
car trigger is the Long Haul's own `CombatRepair` name, while Ratchet's is `CombatRepairVehicle`.

## 2. Mechanism (what is known)

Start, when the special is pressed (the button path is `connectionAttackSpecial`, overlay
`stateAttackSpecial`, both [data] per `docs/character-specials.md`):

1. [game] `0072afd0` `character_special_type_switch(char)`. The character is `char`, its
   attributes are `char+0x140` (the code reads `local_148[0x50]`). It calls
   `FUN_0072bc40` first, then reads the specials.
2. [game] `0072bc40` (special cooldown start, disassembly `0072bc87..0072bcae`): reads
   `attr+0xbc` (cooldown) and writes it to `timer+0x84` and `timer+0x88`. So the
   cooldown is 25.88 s (Ratchet) and 29.5 s (Long Haul). The cooldown is not
   healing-specific.
3. [game] `0072afd0` sends the self-trigger message. If the character is in the car (the
   code reads `char+0x31c == 3`, meaning the car form [assumed]), it uses
   `attr+0xdc` (vehicle self trigger) and `attr+0xe0` (vehicle name). Otherwise it uses
   `attr+0xc8` and `attr+0xcc`. The name is sent by `FUN_00724940` with the Trigger
   hash `d5d636c1`, then `FUN_007254a0` runs. So on foot Ratchet sends `CombatRepair`,
   and in the car he sends `CombatRepairVehicle`. Long Haul sends `CombatRepair` in both
   forms. [game] for the branch, [data] for the names.
4. [game] `0072afd0` switches on `attr+0xc0` (-1 for both): no case runs. The special
   has no type-specific code. Weapon fire name is 0, so nothing else is fired.

The heal itself: the message `CombatRepair` goes to the character's own effects (the
Lux script system). Its body is not in the exports (section 4). Those scripts are what
does the healing, the radius, the duration and any cost.

Regeneration (the health path) [game], for context:

5. [game] `0071ffb0` `health_regenerate_update(char, dt)`: this is the character's
   self-regeneration. It reads only `attr+0x54` (reciprocal of duration), `attr+0x5c`
   (`healthRegenRate`, used only as an enable gate `> 0`) and `attr+0x60`
   (`healthRegenSecsPerTic`, -1 so off). It never reads the `FromRatchet` or
   `FromLonghaul` fields at `+0x64..+0x70`. The formula is already in `docs/damage.md`
   (cubic in the delay). The `FromRatchet` numbers are not part of self regeneration.
6. [game] `0072bdb0` (`char_status_change_effects`, unread before this ticket) is a
   status-change dispatcher. When the character's state changes (`param_1 != param_2`),
   it starts `FX_heal` (`0072d2e0`) for each of two counters at `obj+0xa8`, and
   `FX_regenerate` (bit 22 of `char+0x14c`), `FX_heal_OptimusF_special` (bit 30), `FX_stun`,
   `FX_frost`, `FX_hit_null_ray` and damage smoke. These are visuals. No CombatRepair
   name appears in it, and no heal amount.
7. [game] `0072d2e0` (`fx_heal_start`) sends `FX_heal` (`6845ec0f`) with Trigger; `0072d410`
   (`fx_heal_stop`) sends `FX_heal` with stop hash `9caa723d`. Both are visual.

End: [game] no end function for the special was found. The cooldown counts down in the
timer (`+0x84`, `+0x88`), and the overlay state ends on its own clock. The duration
(`attr+0xc4` = 5.34 s) has no reader found (section 4).

## 3. Pseudo-code, as far as it goes

START (special pressed, state `stateAttackSpecial`):
1. [game] `0072afd0`: get the character and its attributes; call `0072bc40`.
2. [game] `0072bc40`: `timer.cooldown = timer.cooldown_max = attr.+0xbc` (25.88 or 29.5).
3. [game] `0072afd0`: if car form, name = `attr.+0xe0`, enabled by `attr.+0xdc`; else name =
   `attr.+0xcc`, enabled by `attr.+0xc8`. If enabled and name != 0: send `{name, Trigger}`
   to the character (`FUN_00724940`, `FUN_007254a0`).
4. [game] `0072afd0`: switch on `attr.+0xc0` (-1): nothing.
5. [data] the Lux scripts `CombatRepair` / `CombatRepairVehicle` run now (not read).

UPDATE:
6. [unknown] Per-tick heal of allies or self. Not found. (The self regen `0071ffb0` does
   not use the special.)

END:
7. [unknown] Duration 5.34 s (`attr.+0xc4`): no reader found. Stop: not found.

## 4. Not found

- The heal itself: the Lux effect script named `CombatRepair` (`0x956423da`) and
  `CombatRepairVehicle` (`0xbc9a3692`). Searched in `export\data\global_states.txt`,
  `global_characters.txt`, `global_tuning.txt`, `bumblebee.txt`, `starscream.txt`, and
  `objects.tsv` (grep only): no match. The script is in the character's own `.str` pack
  (`ratchet`, `longhaul` in `the install`, read-only). Settling it: dump the pack's data objects with
  `work\tf2_lxb.py` (`dump pack.str#N`) and search the Lux effect for the name hash
  `956423da`.
- Readers of the `FromRatchet` / `FromLonghaul` rates (`attr+0x64..+0x70`). The loader
  `00726710` stores them; no function reads them by name hash (`find_imm` finds only the
  loader), and no function found reads them by offset in the decompile patterns tried
  (`+ 0x140) + 0x68`, etc.). Settling it: a decompile search for `0x64`..`0x70` offsets off
  a second attribute pointer, or the ally-regen path in the Lux effect.
- The cost of the special (meter or cooldown only). Only the cooldown is read.
- The heal radius, the amount per second or tick, the target set (self, allies).
- The Duration consumer (`attr+0xc4`). The overlay state's timer was not located.
- The animation or state the special plays (the overlay `stateAttackSpecial` was not read).
- What stops the repair (no end function found; damage interruption not checked).

## 5. Assumptions

- [assumed] `char+0x31c == 3` means the car form, because the Megatron and Starscream
  specials use the same branch for vehicle names. Settle by reading the form enum
  (`docs/state-machine.md`).
- [assumed] the `FromRatchet` and `FromLonghaul` rates are gate values like
  `healthRegenRate` (positive = on), not per-second rates. Reason: self regen reads its own
  rate only as a gate (`docs/damage.md`), and the same pattern is expected. Settle by reading
  the function that reads `+0x64..+0x70`.

## 6. Ledger

Recorded with `fn.py note` this ticket: `0072bc40 special_cooldown_start`,
`0072bdb0 char_status_change_effects`, `0072d2e0 fx_heal_start`, `0072d410 fx_heal_stop`.
Already in the ledger and read: `0072afd0 character_special_type_switch`,
`0071ffb0 health_regenerate_update`, `00726710 character_attributes_load`.

## The heal (second read)

Reader ticket, 2026-10-12, second pass. Question: where the heal happens. Result: FOUND in
code, except the part that lives in the Lux script (the zone's holder, its radius and its
end). Supersedes section 2 step 6-7 ("visual only") for the heal itself, and section 5
assumption 2 (the `FromRatchet` / `FromLonghaul` fields are NOT gate values; they are rates).
Nothing was run, nothing written to the install.

Lead A (state machine), [data]: no state carries the name. `grep` of
`export\data\global_states.txt` finds no `CombatRepair` text and no `956423da` or decimal
`-1788599334` in the state blocks; the name appears only in the attribute blocks (the
`specialAbilityLuxScriptNameToTrigger` lines). `stateAttackSpecial` (line 127) is an overlay
with `animSetName` and `animName` empty, `playAnimOnce: true`, `finishOnSpecialAnimSetExit:
false`, `robotForm: #5`. So the state carries no heal, radius or duration. The name is sent by
code (section 2 step 3) to the character, and the Lux script receives it.

Lead B (readers of attr+0x64..0x70), [game] all found by decompile, disassembly not needed
for the branch:

Attribute slots (loader `00726710`, [game]): `attr+0x64` healthRegenRateFromLonghaul,
`attr+0x68` healthRegenRateFromRatchet, `attr+0x6c` healthRegenSecsPerTicFromLonghaul,
`attr+0x70` healthRegenSecsPerTicFromRatchet. The per-slot readers index them as
`attr + 0x64 + 4*s` (rate) and `attr + 0x6c + 4*s` (secs per tic), with `s` = 0 or 1.

Numbered steps (the heal path):

1. [game] `0072a550` `damage_timers_update(comp, dt)` calls `0072d540(comp, dt)` every update
   (line 83 of its body). `comp` is the receiver's damage component (`char+0x234` side); its
   character is `comp`'s owner, read by `FUN_00544db0`. The attributes are read from the
   RECEIVER's own character (`char+0x140`), not from the healer's.
2. [game] `0072d540` (heal_zone_apply), per slot `s` with counter `comp+0xa8+4*s > 0`:
   - if `attr+0x6c+4*s <= 0` (secs per tic off; this is the case for all four characters'
     values, which are -1 [data]): `amount = attr[+0x64+4*s] * dt` (continuous).
   - else (discrete): `comp+0xb0+4*s += dt`; when it reaches `attr+0x6c+4*s`, subtract that
     and `amount = attr[+0x64+4*s] * attr[+0x6c+4*s]` (one tic); otherwise no heal this step.
3. [game] `0072d540` adds `amount` with `0071f5c0(char, amount)`, which calls `health_set`
   (`0071fbd0`) with `health + amount`, clamped at the maximum. `0071f5c0` first gates on
   `FUN_007246b0` and `FUN_00886390` (not read; probably a UI or online gate; [assumed] it is
   a no-op for this single-player path). The clamp and the UI send are in `0071fbd0`.
4. [game] `0072d540` then sends the heal-number UI message if the health actually rose (lines
   31-33, not read in full).

Who fills the slots (the aura, a per-frame zone):

5. [game] `007eb6f0` `heal_zone_scan(holder)` runs for a holder whose active flag is
   `holder+0x44` (a byte). Radius squared: `holder+0x48` (float). Slot: `holder+0x4c` (int, 0 or 1).
   Tracked-target ring list: `holder+0x50` (sentinel) with `holder+0x54` (head).
   - centre: the owner's point plus half its height (`flying_vehicle_camera_target_bottom`,
     `character_camera_height` and `0.5` at lines 75-77). Not checked in disassembly.
   - for each character in the global list (`PTR_LOOP_00c05d00`): candidate `c` needs
     `c+0x234 != 0`; `c` is accepted if it is the holder's own owner (self) or passes the team
     test `thunk_FUN_0117e760` (unread; it calls `FUN_0071f460`), AND flag bit 10 of `c+0x14c`
     is set (line 74).
   - inside: squared distance from the centre to the candidate's point `<= holder+0x48`
     (line 82) calls `0072d0b0(c, holder+0x4c, value, key)` (line 87): `0072d0b0`
     (aura_source_enter) increments `c+0xa8+4*slot`; the first one starts `FX_heal` (visual,
     `0072d2e0`); it stores `value` (param 3) at `c+0xb8+8*i` and `key` (param 4) at
     `c+0xbc+8*i` in an 8-entry list.
   - outside: when `dist^2 > holder+0x48` (line 183), `0072d1f0(c, key)` (aura_source_leave, line
     185) decrements the counter; at zero it sets the counter and its `+0xb0` to 0 and stops
     `FX_heal` (`0072d410`).

6. [data] Values for the four attribute cells (Ratchet / Long Haul), with the secs per tic
   at -1 in all cases, so the continuous branch is the one that runs:
   - a Ratchet receiver: in the Longhaul zone (slot 0) 40 per second; in the Ratchet zone
     (slot 1) 60 per second.
   - a Long Haul receiver: in either zone 67 per second.
   Units: "per second" is the reading the code allows, since amount = rate * dt with dt the
   0.032 s step. [assumed] the rate is hit points per second, because `health_set` takes hit
   points and `baseHealth` is 320 / 360 [data]. Settle with the recorder or a `-- --check`
   sample: read `health` before and after one second in a zone.

What the code does NOT give (remain unsettled):

- The holder's creation. The holder (the object with `+0x44`, `+0x48`, `+0x4c`, `+0x50`) is
  set up by the Lux script of `CombatRepair` / `CombatRepairVehicle` (the name is only data
  [data]). No code writes `holder+0x44` or `+0x48` in the functions read. The radius value
  and which script sets the slot are in the pack, not read. Settle: dump the character pack's
  Lux effect with `work\tf2_lxb.py` (lead C; not run in this ticket, because the exports already
  gave the heal's numbers).
- The duration 5.34 s (`attr+0xc4`). Still no reader. Most likely the Lux script's timer
  clears `holder+0x44` after 5.34 s, which then empties the zone (leave path step 5); [assumed]
  until the script is read.
- The slot-to-character mapping. Slot 0 is `attr+0x64` (Longhaul) and slot 1 is `attr+0x68`
  (Ratchet), and the holder's slot is set by the script; so slot 1 = the Ratchet aura is
  [assumed] from the attribute names, not checked against a script that sets `+0x4c` = 1.
- Bit 10 of `c+0x14c` (the candidate flag) and the team test `0117e760` (unread).
- Whether the self heal (a Ratchet healing himself) is the same zone: the candidate list
  includes self (`local_90 == local_98`), so yes [game] for the code, but whether it runs
  for the special's own owner depends on bit 10 [assumed].
- The attack's own owner: `0072bdb0` (status change, state 3) starts visuals only; its state
  3 meaning is not read.

Ledger (`fn.py note`, this ticket): `0072d540 heal_zone_apply`, `0072d0b0 aura_source_enter`,
`0072d1f0 aura_source_leave`, `007eb6f0 heal_zone_scan`.

## The zone holder (third read)

Reader ticket, 2026-10-12, third pass. Questions: the heal zone's radius, what creates the
holder and sets its slot, what switches it off. Pack data read from the install in place
(`<install>\characters\ratchet.str`, `longhaul.str`, read only); dumps in
`work\survey\flight\ratchet_str.txt` and `longhaul_str.txt` (scratch).

Answers:
- RADIUS: `healRadius` (name hash ea0ba462). Stored SQUARED at holder+0x48 [game]:
  `007ebf00` loads the field as float (or int via cvtsi2ss) and does `mulss xmm1, xmm0`
  then `movss [ebx+0x48], xmm1` (disasm 007ebf5f / 007ebf63). The radius is read once, at
  creation. Units: metres [assumed], as the rest of the world.
- SLOT: `specialHealingType` (hash 71df99e3) read by `view_scalar` into holder+0x4c [game]
  (`007ebf00`). Data: true for Ratchet, false for Long Haul [data]. Bool read as int 1/0
  [assumed]: true -> slot 1 (Ratchet, attr+0x68), false -> slot 0 (Long Haul, attr+0x64).
  This settles the slot mapping in section 5 for the pack data.
- OFF: the zone's off switch is the virtual Stop (vtable slot 7, `007eb6c0`): sets
  holder+0x44 = 0 and clears every aura source (`007ebd40` -> `0072d1f0`). The On is slot 6
  (`007eb6a0`): sets holder+0x44 = 1. NO timer in the zone object itself: the HealAreaEffect
  pack record has only LuxScriptName, acceptNullTriggerMessages, healRadius and
  specialHealingType [data]. Whether the 5.34 s `attr+0xc4` drives the Stop message: NOT
  found [assumed]; the callers of slots 6 and 7 are not read (generic Lux dispatch).

Steps (holder creation to heal):
1. [data] The character pack (`characters\ratchet.str`, the Ratchet object) holds, as its own
   things and under one Lux script name `CombatRepair` (hash 956423da, -1788599334 signed):
   an EffectSpawner (`r_repairpad_ratchet`, line ~4421) and a HealAreaEffect (line 4783).
   Both carry that name, so one trigger message reaches both. Long Haul's pack
   (`characters\longhaul.str`) has the same pair (lines ~4322, 4683).
2. [game] The class: HealAreaEffect, vtable 00b60aa8 (name record at 00b60a98). Slots: 0 dtor
   `007ebe60`, 2 update / heal_zone_scan `007eb6f0`, 6 On `007eb6a0`, 7 Off `007eb6c0`.
3. [game] Creation: the factory `011970b0` (pool DAT_00d0b470, size 0xb units) sets the
   vtable, holder+0x44 = 0, the aura ring head at +0x50/+0x54, then calls init `007ebf00`
   (healRadius -> +0x48 squared; specialHealingType -> +0x4c). The factory is reached through
   `007ebf90` (a tail jump to 011970b0); its caller was not found by `find_call` (only a self
   hit), so the loader that calls the factory is NOT read.
4. [game] Per frame: `007eb6f0` scans while holder+0x44 is set (see the second-read section).
5. [game] On: the Lux trigger `CombatRepair` reaches the holder and calls On (slot 6), which
   sets +0x44 = 1 and, unless flag +0x16 bit 2 is set, calls `005785d0` (unread).
   [assumed] the trigger is sent by the special's state (`0072afd0` path); not verified.
6. [game] Off: slot 7 (`007eb6c0`) clears +0x44 and empties the aura list via `007ebd40`.
   Who sends it, and when, is NOT found.

Values [data] (healRadius, specialHealingType, objects of class HealAreaEffect):
- Ratchet, `CombatRepair` on the body (line 4783): radius 25 (squared 625), slot true (1).
- Ratchet, `CombatRepairVehicle` (hash bc9a3692, -1130744174) in a sub-object of the same pack (line 569003; which form it belongs to is NOT checked):
  radius 18 (squared 324), slot true (1).
- Long Haul, `CombatRepair` on the body (line 4683): radius 25 (squared 625), slot false (0).
- Long Haul, `CombatRepair` in a sub-object of the same pack (line 637534; form not checked): radius 22 (squared 484), slot false (0).
- So the heal zone of Ratchet and Long Haul is 25 m on foot in both cases. [assumed] the
  pack value is in metres and is the zone's full radius; the rate values are in section 6.

Not settled:
- Off: which message or timer calls slot 7, and whether it is the 5.34 s duration. Settle with
  the Lux dispatcher's callers of vtable slots 6 and 7, or a recorder run with the zone on.
- Which form each second zone (lines 569003, 637534) belongs to: the parent object was not traced.
- The factory's caller (the loader that spawns the pack's HealAreaEffect): not read.
- Bool-as-int for specialHealingType: [assumed].
- Ledger added (this ticket): `011970b0 heal_area_effect_create`, `007ebf00 heal_area_effect_init`,
  `007eb6a0 heal_area_effect_activate`, `007eb6c0 heal_area_effect_deactivate`,
  `007ebd40 heal_area_effect_clear_auras`, `007ebe60 heal_area_effect_delete`.
