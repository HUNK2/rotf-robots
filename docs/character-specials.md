# Character specials (data survey, 2026-10-12)

Scope: each playable character's special ability ("overdrive" for Bumblebee) and its
form-specific specials, from the data (`export\data\global_states.txt`) and the
state machine. Scratch: `work\survey\specials\`. Read only: the attribute dump, the state
definitions, and one function (`0072afd0`, recorded in the ledger as
`character_special_type_switch`). Nothing was run.

## How the special is laid out (all characters)

Each `characterAttributes<Name>` block has 15 `specialAbility*` fields in this order (the
line numbers below are the `specialAbilityCooldown` line N; the rest follow at N+1..N+14):
Cooldown, Type, Duration, SelfTrigger, LuxScriptNameToTrigger, WeaponFireFromAnim,
WeaponFireName, VehicleAnim, VehicleSelfTrigger, VehicleLuxScriptNameToTrigger,
VehicleWeaponFireFromAnim, VehicleWeaponFireName, Icon, UseSpecialFlightMode,
attackSpecialDistance. [data]

- Button: `connectionAttackSpecial` (global_states.txt:1445) detects `includeHitButton
  #00000002` (AttackSpecial, button 2, per `docs/weapon-mode.md`), held 0.5 s
  (`timeActivationStaysOn: 0.5`), no ground or air check. [data] [game, weapon-mode.md]
- Overlay state for all kinds: `stateAttackSpecial` (line 127), reached through
  `connectionListAttackSpecial` (3942). [data]
- Start: `0072afd0` (ledger `character_special_type_switch`). It sets the cooldown from
  attribute `+0xbc`, fires `SpecialAbilityWeaponFireName` if non-zero, sends the
  `specialAbilityLuxScriptNameToTrigger` message to the character if SelfTrigger is on,
  then switches on attribute `+0xc0` (the Type) [game, `0072afd0` lines ~60-120]:
  - 0: Optimus ground: `OptimusSpecial` effect, spawn `SpecialAttackMesh`, message `Trigger`.
  - 1: Optimus flight: as 0, plus `FUN_00730e90(1)` (DashEvent).
  - 2: vtable slot 0x1c called with 1.5 (0x3fc00000). Only DComScout has type 2 here. [assumed]
  - 3: message `b6ff7243` then `Trigger` (DComSniper). [assumed mapping]
  - 4: if in the car, `VehicleCannonRotator` (Megatron types).
  - 5: ground search within 5 m (25 = 5^2); sets `+0x94` to 0.25 if the search is empty.
    No listed character has type 5.
  - -1 and 6: no case, nothing in this switch.
- Form-specific states come from each character's state machine list (below).

## Table

Trigger names are resolved from `export\data\names.tsv` (hash in hex, lower case).
`#0000000N` values are the Type / Icon enums as the dump prints them; their meaning is
[unknown] except where the switch above fixes Type.

| Character (block line) | Special (kind) | Fields and values [data] | States used | Differs from Bumblebee |
|---|---|---|---|---|
| Bumblebee (51550) | overdrive, Type -1 | Cooldown 23.5, Duration 5, self trigger on, trigger `specialAttack` (666a6ad0); car: `specialAttackVehicle` (1b5e3550); no weapon | `stateAttackSpecial` overlay [game] | (reference) |
| Optimus (65817) | ground special, Type `false` (0) | Cooldown 29.4, Duration 10, self on, `specialAttack`; Icon true; no weapon | `OptimusSpecial` effect path; state not identified [unknown] | Longer cooldown and duration; a melee damage multiplier 1.5 (`docs/melee.md` 87-90); no weapon |
| OptimusFlight (66200) | flight special, Type `true` (1) | Cooldown 29.4, Duration 10, self on, `specialAttack` | as Optimus, plus DashEvent | melee multiplier 1.75 in flight (`docs/melee.md`) |
| Starscream (68479) | `FX_starscreamSpecial`, Type #6 | Cooldown 23.5, Duration 5, self on, trigger `FX_starscreamSpecial` (9037144b), car same; Icon #3 | `stateMachineGenericSpecialFlyWithAnim` (state list at 16822, entry 5) | Type 6 has no switch case: effect is the trigger's, not the switch. Flight-form state |
| Megatron (61364) | `MegatronSpecialGun`, Type 4 | Cooldown 29.4, Duration 2.75, no self trigger, WeaponFireFromAnim true, weapon `MegatronSpecialGun` (101f8f2f); car weapon `MegatronSpecialVehicleWeapon` (62c25f02), from anim; Icon #3 | `stateMachineSpecialMegatronAttack` (16506): `stateAttackSpecialMegatron` (robotForm 2, SpecialAnimSet) and `...MegatronVehicle` (robotForm 3) | Fired by animation, not a trigger; short duration |
| MegatronFlight (61739) | `MegatronFlightSpecialGun`, Type 4 | Cooldown 29.4, Duration 2.75, no self trigger, weapon from anim, `MegatronFlightSpecialGun` (ea79e9fc) | `stateMachineSpecialMegatronFlightAttack` (16628): `stateAttackSpecialMegatronHover` | Hover form of Megatron's gun special |
| Ironhide (60229) | deployable turret, Type -1 | Cooldown 23.5, Duration -1, no self trigger, weapon from anim, `Ironhide_SpecialDeployableTurret` (effad9d4); car: anim `Special`, `Ironhide_VehicleSpecial` (1bf34bf5); Icon #4 | list `stateMachineListIronhide` (16275, pointers only); turret states [assumed] | Turret, not a self effect |
| Blackout / Grindor (51142) | deployable turret, Type -1 | Cooldown 23.5, Duration -1, weapon from anim, `Blackout_SpecialDeployableTurret` (5f356648); car `Blackout_VehicleSpecial` (1fe6ca2f), anim `Special`; Icon #4 | `stateMachineSpecialDeployTurret` (14120, list 13931): `stateAttackSpecialDeployTurret`, `...Hover`, `...Jump` | Same turret kind as Ironhide, with three states by form |
| Jetfire (60609) | missile dump / carpet bomb, Type -1 | Cooldown 23.5, Duration -1, no self trigger, weapon `JetFireMissileDump` (ced31e93) not from anim; car `JetFireCarpetBomb` (d9c853ff); Icon false | `stateMachineListJet` (16278, pointers only) [unknown] | Weapon special, no trigger |
| Firestorm / Breakaway (59074) | self special, Type -1 | Cooldown 23.5, Duration -1, self on, trigger `specialAttack` (666a6ad0), same as Bumblebee; no weapon | `stateMachineListFireStorm` (16029) [unknown] | Same trigger as Bumblebee, no duration, no weapon |
| Ratchet (67729) | `CombatRepair`, Type -1 | Cooldown 25.88, Duration 5.34, self on, `CombatRepair` (956423da); car `CombatRepairVehicle` (bc9a3692); Icon #2; repairs (the effect is not read) | [unknown] | A heal, not a stun/fire |
| LongHaul (60984) | `CombatRepair`, Type -1 | Cooldown 29.5, Duration 5.34, self on, `CombatRepair`; car same; Icon #2 | [unknown] | Same heal as Ratchet, longer cooldown |
| Audi / Sideways (50762) | AudiR8 mine, Type -1 | Cooldown 17.5, Duration -1, no self trigger, weapon `AudiR8SpecialMine` (11151216) not from anim; car anim `Special`, `AudiR8VehicleSpecialMine` (fcb068be) from anim; Icon #3 | `stateMachineGenericSpecialWithAnim` (13158, list 12189) | Drops a mine |
| DComScout (52925) | `FX_superjump`, Type 2 | Cooldown 20, Duration 5, self on, trigger `FX_superjump` (b2c37158); Icon false | `stateMachineGenericSpecial`-family [unknown] | Jump special (AI); switch case 2 |
| DComSniper (53483) | Type 3, Duration 9 | Cooldown 15, Duration 9, no self trigger, no weapon, Icon false | [unknown] | Switch case 3 |
| DComWarrior (54005) | melee special, Type -1 | Cooldown 10, Duration -1, no self trigger, attackSpecialDistance 9 | list `stateMachineListEnemyLandWSpecialMelee` (15636): `stateMachineSpecialMeleeAttack` (15655) → `stateAttackSpecialMelee` (SpecialAttackSet, robotForm 5) | AI melee special, no self trigger |
| MilitaryHuman (62619) | none, Type -1 | Cooldown 2, Duration -1, no self trigger, no weapon | list `stateMachineListHumanMilitary` (16269) | No special |

Other characters, for reference: Devastator uses `stateAttackSpecialDevastator`
(`Idle2AttackSpecial`, line 197). Wheelbot (`Wheelbot_Special`) and SemiTruck are not in the wanted list.

## Kinds of special in the data

- Type switch (fixed by `0072afd0`): 0 Optimus, 1 Optimus flight, 2 DComScout, 3 DComSniper, 4 Megatron forms, 5 none listed. -1 and 6 do nothing in the switch. [game]
- Self-trigger effect: the `specialAttack` message (Bumblebee, Optimus, OptimusFlight, Firestorm), the `FX_*` messages (Starscream, DComScout), or `CombatRepair` (Ratchet, LongHaul). The message is caught by the character's own effects. [data]
- Weapon from animation: Megatron, MegatronFlight, Blackout, Ironhide and Audi (car form only). Turrets (Ironhide, Blackout), mines (Audi), gun (Megatron). Jetfire's weapons are not from animation (WeaponFromAnim false, both forms). [data]
- Melee: DComWarrior's `stateAttackSpecialMelee`. [data]

## Unknown

- Meaning of the `#0000000N` Icon values (HUD icon index?). [unknown]
- Optimus's Type printed as `false` and OptimusFlight's as `true`: taken as 0 and 1. [assumed]
- Which state machine Optimus, Ratchet, LongHaul, Firestorm, Jetfire, DComScout and DComSniper use: their lists hold shared pointers (`<see object at 0x...>`) not printed here. [unknown]
- Type 5 (ground search): no listed character has it. [unknown]
- `msgUserDefined` -836230507 in `connectionMessageAttackSpecial` (2532): name not resolved. [unknown]
- Whether DComSniper's Type 3 message `b6ff7243` is its shot. [assumed]
- Which special the Jetfire and Ironhide turrets are in flight. Not read.
