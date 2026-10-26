# Weapon-type specials: Megatron's gun, Ironhide's turret, Blackout's turret (read, 2026-10-12)

Scope: the three characters whose special (`specialAbility*`) fires a weapon, read for a port. Out of scope and left to others: CombatRepair, Jetfire, the Audi mines. Scratch dump: `work\survey\flight\megatron.txt` (Megatron pack chunk 0, `tools\tf2_lxb.py dump`). Nothing was run; nothing was written to the install.

Tags: `[game]` read in code (address given), `[data]` read from the export or the pack dump (file and line given), `[assumed]` a guess (why, and what would settle it), `[unknown]` not found.

## Shared mechanism (all three)

1. The button is AttackSpecial (2) (`connectionAttackSpecial`, docs/character-specials.md). It enters the overlay `stateAttackSpecial` (`overlayState: true`, `playAnimOnce: true`) [data, global_states.txt:127]. Its enter, `FUN_00879110`, calls `character_special_type_switch@0072afd0` [game, docs/weapon-mode.md 963-964].
2. The special's weapon is NOT fired from the start function for these three. Each has `specialAbilityWeaponFireFromAnim` true (walk) and `specialAbilityVehicleWeaponFireFromAnim` true (car) [data]. The shot comes from an animation event, `WeaponFireSpecialEvent` (see step E1).
3. None has a self trigger (`SelfTrigger` false, `VehicleSelfTrigger` false) [data]. No message goes to the character.
4. The duration field is `specialAbilityDuration` (Megatron 2.75, Ironhide -1, Blackout -1) [data].
5. Their weapon special attributes are one template: heat capacity 100, heat per round 5 (charge min/max 5/10), heat decrement 2 per second after a 1 s wait, overheat period 5 s, `isSpecial: true` [data, global_states.txt 80486, 89587, 92394]. The Ironhide turret differs only in the rest (see below).

## Start: `character_special_type_switch@0072afd0` [game]

Ledger entry already exists (`0072afd0`). The steps below are the parts read for these three; `char` = character, `attr` = `char+0x140` (the attribute block, `local_148[0x50]` in the decompile), `mgr` = `char+0x238` (weapon manager, `local_148[0x8e]`).

- S1 [game] `FUN_0072bc40`: cooldown, `+0x84` and `+0x88` = `attr+0xbc` (Megatron 29.4, Ironhide and Blackout 23.5). Scaled by an upgrade in single player (`FUN_0079d060`, docs/weapon-mode.md 966).
- S2 [game] `FUN_007b3d80(mgr, 0)` (ledger: `special_fire_by_attribute`), with `param_2 == 0` from the start:
  a. If `scripted_health_controller` is true, stop.
  b. If `attr+0xd0` (walk, WeaponFireFromAnim) or `attr+0xe4` (car) is non-zero, stop. All three have it true, so NOTHING is fired here.
  c. Otherwise fire `attr+0xd4` (walk) or `attr+0xe8` (car) through `FUN_007b3c40`. Not reached for these three.
- S3 [game] Self trigger (`attr+0xc8`, car `attr+0xdc`): false, so no message. `attr+0xcc` (LuxScriptName) is `0` for all three.
- S4 [game] `switch(attr+0xc0)` (the Type):
  - Ironhide and Blackout: type -1, no case. Nothing.
  - Megatron (type 4): in the car only, send message `0x030a07db` (`VehicleCannonRotator`) then `Trigger` (`0xd5d636c1`). Both forms then call `FUN_0084e390` (ledger `special_clear_clip_state`: writes zero to two words after `default_blend_time`; what they are is [unknown]).
- S5 [game] HUD: since `attr+0xc0 != -1` (Megatron) or `attr+0xd4 != 0` (all three), it calls `send_ui_integer` with `0x84be0e49` and `0x302517b4`, both 1. These two hashes are NOT in `export\data\names.tsv`: meaning [unknown].
- S6 [game] Duration: if `attr+0xc4 > 0`, `+0x8c` = `attr+0xc4`. Megatron: `+0x8c` = 2.75. Ironhide and Blackout (-1): `+0x8c` is not set [data, game].
- S7 [game] Stats only: `ABILITYUSED_UPDATE` and a per-character `USED_SA_*` call (Megatron `USED_SA_MEGATRON`, Ironhide `USED_SA_IRONHIDE`; Blackout's branch not read). Not gameplay [game].

## Event: `WeaponFireSpecialEvent`, handler `007ba870` [game]

- E1 [game] `007ba870` (ledger `special_weapon_fire_event`): gets the character, then `FUN_007b3d80(mgr, 1)`. The `1` skips the WeaponFromAnim gate of S2b, so the shot happens.
- E2 [game] `FUN_007b3d80(mgr, 1)` takes the same scripted-health check, then fires name `attr+0xd4` (walk) or `attr+0xe8` (car) through `FUN_007b3c40` (ledger `special_fire_weapon_by_name`). That walks the 8 weapon slots of `mgr+0x34` (pairs of object and state), and for the slot whose name (object `+0x2c`) matches and whose readiness test (vtable `+0x30`) passes, calls its fire (vtable `+0xc4`) at the aim from `FUN_007b36c0` and `FUN_007b34a0`. [game]
- E3 [data] Names fired, from the attribute blocks (signed values converted to hex):
  - Megatron walk `270503727` = `0x101F8F2F` `MegatronSpecialGun`; car `1656905474` = `0x62C25F02` `MegatronSpecialVehicleWeapon` (`names.tsv` 5647, 34571).
  - Ironhide walk `-268772908` = `0xEFFAD9D4` `Ironhide_SpecialDeployableTurret`; car `468929525` = `0x1BF34BF5` `Ironhide_VehicleSpecial` (`names.tsv` 83824, 9778).
  - Blackout walk `1597335112` = `0x5F356648` `Blackout_SpecialDeployableTurret`; car `535218735` = `0x1FE6CA2F` `Blackout_VehicleSpecial` (`names.tsv` 33331, 11185).
  - Car anims: Ironhide and Blackout `attr+0xd8` = `Special` [data, docs/character-specials.md].
- E4 [assumed] The event fires once per clip event. Whether the clip repeats it while the overlay runs is not checked. Settles by reading the `SpecialAnimSet` clips' events (`work\survey\weapons\clipnodes.py` route) or by the user's recording.

## WeaponFireSpecialEvent in clips (read 2026-10-12, port in `formats::anim`)

- Class hash: CRC-32 of "WeaponFireSpecialEvent" = `0x461c3ad4` [data, computed with zlib CRC-32, the same CRC as `names.tsv`].
- Payload: none. `tools\tf2_lxb.py` and the Rust decoder both give no fields for it (`fields={}`) on every event found [data]. Only the time and the channel carry meaning.
- Megatron (`characters\megatron.str`, `SpecialAnimSet`): ONE event, in `SpecialAttack_Exit` (length 66 ticks), at tick 2, channel `WeaponAttachment_Hand_R`. The same clip re-holsters the guns at ticks 16 and 18 (`WeaponAttachmentEvent`, `Trans_W2R`) [data]. None in Intro, Idle or the car clips. A raw scan finds 2 objects of the class in the pack; the second sits in no clip of any set [unknown: where].
- Blackout (`characters\blackout.str`, `SpecialAnimSet`): ONE event each in `TurretDeploy` and `TurretDeployJump` (length 56), at tick 24, channel `Root` [data].
- Starscream (`characters\starscream.str`): one object of the class, in no clip of any set [unknown: where].
- Jetfire (`characters\jetfire.str`) and Audi R8 (`characters\audir8.str`): no event in any set. The Audi has no `SpecialAnimSet` [data].
- Start / stop: none. No clip read has a pair or a repeat. So each event is one press [data]: one round (`numProjectilesPerFire` rounds if above 1), no stream after it. [assumed] an automatic special weapon does not keep firing after its event; no reader found. Settles by the user's recording or the game's handler `007ba870` read to its end. (Earlier text here said the gun streams for the special's 2.75 s; that was an assumption and is replaced.)
- Port: `formats::anim::SpecialFireEvent { time, node }`, `Clip.special_fire_events` (time order), `read_special_fire_events` via `special_fire_event`. Tests `special_fire_event_megatron_exit_tick_2` and `special_fire_event_blackout_turret_deploy_tick_24` (values from the clips above).
- Delivery for the integrator: the arm's events are walked by `Axe::events` (`core\src\gun.rs` line 235, per-window `from` / `to` ticks); clip sound events by `advance` (`gun.rs` line 153). Add the same window loop over `clip.special_fire_events` and call `SpecialFire::on_fire_event(car)` (`core\src\special_weapon.rs` line 209) there.

## Megatron: `MegatronSpecialGun` (type 4)

Data [data]
- Block: `characterAttributesMegatron` (global_states.txt 61322). `specialAbilityCooldown` 29.4; `Type` `#00000004`; `Duration` 2.75; `SelfTrigger` false; `WeaponFireFromAnim` true; `WeaponFireName` 270503727; car `VehicleAnim` 0; car `VehicleWeaponFireFromAnim` true; car `VehicleWeaponFireName` 1656905474; `Icon` #3; `vehicleWeaponTimeTilCanFire` 0.75 (the car gun, not read).
- Gun: `Megatron_SpecialGunAttribute` (global_states.txt 92394; hash `e1e253da` in `names.tsv` 78853): `numProjectilesPerFire` 1; `rateOfFire` 1; `automaticFiring` true; `burstPerFire` 1; `timeBetweenEachBurst` 0.01; `hasChargedShot` false; `timeTilChargeStarts` 0.5; `chargeDurationTilMaxEffect` 3; `minChargeDamageMultiplier` 1; `maxChargeDamageMultiplier` 2; `minHeatMeterIncrement` 5; `maxHeatMeterIncrement` 10; `heatMeterCapacity` 100; `heatMeterIncrementPerRound` 5; `heatMeterDecrementPerSecond` 2; `heatMeterCooldownTilDecrement` 1; `heatMeterOverHeatPeriod` 5; `heatMeterOverHeatCooldownRate` -1; `listenToWeaponAnimEvent` true; `isSpecial` true; `useFacingAsTarget` false; `isVehicleModeWeapon` false; `rumbleFireDuration` 1.2; `rumbleFireStrength` 1.
- Weapon thing: `GunWeapon`, `weaponName` 270503727, `gunAttribute` `-505261094` = `0xE1E253DA` (the gun attribute above), `bulletProjectileObj` = embedded `megatronspecialbeamgun` (7 roots) at `work\survey\flight\megatron.txt` 805140-805572 and onward [data]. Inside: `BulletTrace` `lifeTime` 0.5, radii 2 and 2; a `DamageLink` whose `Explosion` has `damage` 200, `min_radius_fall_off_damage` 0.75, `minDamage` 1, `damage_player` true, `can_damage_self` false, `specialCaseDamageStuff.knockBackSpeed` 30; `Destructable` `collateralDamageValue` 1 [data]. Which event starts that DamageLink is not read [unknown].
- Car: `Megatron_SpecialVehicleGunAttribute` (global_states.txt 92479) and `MegatronSpecialVehicleWeapon` (`names.tsv` 34571): not read.
- States: machine `stateMachineSpecialMegatronAttack` (11433): `stateAttackSpecialMegatron` (`overlayState` false, `robotForm` 2, `SpecialAnimSet`, `playAnimOnce` true, `finishOnSpecialAnimSetExit` true, motion mode `#3c956e02`) and `stateAttackSpecialMegatronVehicle` (`robotForm` 3, motion `#e2b22eb4`, same flags) [data, global_states.txt 210, 238].

Steps [game] unless tagged
- M-start: S1 to S7 above. Walk: S4 runs `FUN_0084e390`. Car: S4 also sends `VehicleCannonRotator`. No shot yet (S2b).
- M-shot: the `SpecialAnimSet` clip hits `WeaponFireSpecialEvent`: E1 to E2 fires `MegatronSpecialGun` (walk) or `MegatronSpecialVehicleWeapon` (car). Each shot: gun attribute numbers above; `rateOfFire` 1 and `automaticFiring` true mean a continuous stream while the event repeats [assumed: meaning of rateOfFire 1 (shots per second?), settled by the user's recording].
- M-heat: heat per round 5, capacity 100, drops 2 per second after 1 s without shooting; overheat 5 s [data; whether the special shares the normal gun's heat meter is [unknown]].
- M-duration: `+0x8c` = 2.75 s. The damage object's update `FUN_0072a550` counts `+0x84` and `+0x8c` down, and at zero `FUN_0072b980` sends the scripts an "off" message `9caa723d` (docs/weapon-mode.md 976-978; those functions not read here). [game, per notes]
- M-end: the state `stateAttackSpecialMegatron` finishes when the `SpecialAnimSet` clip exits (`finishOnSpecialAnimSetExit` true) [data]. Which of the clip end or the 2.75 s timer ends first is [unknown] (the timer's effect on the state is not read).

## Ironhide: `Ironhide_SpecialDeployableTurret` (deployable turret, type -1)

Data [data]
- Block: `characterAttributesIronhide` (global_states.txt 60187). `specialAbilityCooldown` 23.5; `Type` `#ffffffff` (-1); `Duration` -1; `SelfTrigger` false; `WeaponFireFromAnim` true; `WeaponFireName` -268772908 (effad9d4); car `VehicleAnim` -2083125633 (`Special`); car `VehicleWeaponFireName` 468929525 (1bf34bf5, `Ironhide_VehicleSpecial`); `Icon` #4.
- Attribute: `Ironhide_SpecialDeployableTurret` (global_states.txt 89587): the template above, plus `launchSpeed` 50, `zGravity` -64, `lifeTime` -1, `maxTurretsAtOnce` 1, `fireDummyXYAngle` [180], `rumbleFireDuration` 0.6, `listenToWeaponAnimEvent` false.
- Weapon thing: `TurretLauncher` in `ironhide.txt` line 659171 (dump in `work\survey\weapons\ironhide.txt`): `weaponName` -268772908, `turretLauncherAttribute` -268772908 (the block above), `deployableTurretObj` = embedded `ironhide_turret` (3 roots) [data].
- Deployed object `ironhide_turret` [data]: `DestructInWater`; `TargetLockable` (`radius` 2, `height` 3.8, `targetableByDecepticons` true, `targetableByAutobots` false, `startEnabled` true); `MissileLauncher` (`weaponName` 0, `fireAnimation` -118708495, `overheatAnimation` 1510801051, `missileLauncherAttribute` -1792689951 = `Ironhide_TurretMissileAttribute` `9525b8e1`, `missileProjectileObj` with `PhysicsDamage` `knockBackSpeed` 15 and an `Explosion` `damage` 20, `min_radius_fall_off_damage` 0.75; `AnimRefBundle`). No `DeployableTurret`, `TurretRotator` or `FireTurret` thing was found in this object (grep of its things, lines 659214-680000) [data].
- `Ironhide_TurretMissileAttribute` (global_states.txt 89726): `rateOfFire` 0.8; `automaticFiring` false; `heatMeterIncrementPerRound` 20; `heatMeterDecrementPerSecond` 25; `heatMeterCooldownTilDecrement` 1.5; `heatMeterCapacity` 100; `heatMeterOverHeatPeriod` 5; `aimAssistMaxRange` 500; `useFireDummyMatrix` true; `isSpecial` false; `rumbleFireDuration` 0.25.
- States: list `stateMachineListIronhide` (16275) has only back-references, so its special state is NOT identified. Turret state assumed to be `stateAttackSpecialDeployTurret` (global_states.txt 155: `overlayState` false, `robotForm` true, `SpecialAnimSet`, `playAnimOnce` true, `finishOnSpecialAnimSetExit` false) [assumed: the same kind as Blackout's, which uses it. Settled by reading the state list's back-references (the unnamed `<see object at 0x...>` entries).]

Steps [game] unless tagged
- I-start: S1 to S7. S2b stops (walk `attr+0xd0` true). S4: type -1, nothing. S6: no duration set.
- I-shot: `SpecialAnimSet` clip event `WeaponFireSpecialEvent` (ironhide.txt has these at dump lines 635889, 641630, 805728; each followed at tick 26 by an `AnimSpawnEffectEvent` of `w_muzzle_flash_turret`, docs/character-weapon-data.md 46-47) fires E1 to E2: `Ironhide_SpecialDeployableTurret`. The thing is a `TurretLauncher`; the code path that places the `ironhide_turret` object (`deployableTurretObj`), with `launchSpeed` 50 and `zGravity` -64 for the throw, and the anims `animOnStart`/`animOnLand` [assumed: the launch is the deployable's use; NOT read; settles by the launcher's function, not yet found].
- I-turret: the placed turret has `TargetLockable` (enemy team: targetable by Decepticons, so a Decepticon's lock) and its `MissileLauncher` fires missiles at 0.8 per second, automatic off, heat +20 per round, -25 per second after 1.5 s, overheat 5 s [data]. What makes it fire (its own AI or the turret manager) is [unknown].
- I-end: state `stateAttackSpecialDeployTurret` runs `playAnimOnce` only; no finish flag is set [data]. Its end is [unknown]. The turret lasts `lifeTime` -1, one at a time (`maxTurretsAtOnce` 1) [data]: how long it stays is [unknown].

## Blackout: `Blackout_SpecialDeployableTurret` (deployable turret, type -1)

Data [data]
- Block: `characterAttributesBlackout` (global_states.txt 51100). `specialAbilityCooldown` 23.5; `Type` -1; `Duration` -1; `SelfTrigger` false; `WeaponFireFromAnim` true; `WeaponFireName` 1597335112 (5f356648); car `VehicleAnim` -2083125633 (`Special`); car `VehicleWeaponFireName` 535218735 (1fe6ca2f, `Blackout_VehicleSpecial`); `Icon` #4.
- Attribute: `Blackout_SpecialDeployableTurret` (global_states.txt 80486): the template, plus `launchSpeed` 50, `zGravity` -64, `lifeTime` -1, `maxTurretsAtOnce` 1, `rumbleFireDuration` 0.6, `listenToWeaponAnimEvent` false. Its `fireDummyXYAngle` list was not read.
- Weapon thing: `TurretLauncher` at `blackout.txt` 701823: `weaponName` 1597335112, `turretLauncherAttribute` 1597335112, `deployableTurretObj` = embedded `blackout_turret` (3 roots, line 702342) [data].
- Deployed object `blackout_turret` [data, blackout.txt 702342 to ~721110]: things in order: `TurretRotator` (720700: `turretBoneName` 660760739, `trackElevation` true, `trackYaw` true, `trackParent` false, pitch -180..180, yaw -180..180, `maxTurnRateLeftRight` 70, `turnAccelerationLeftRight` 70, `maxTurnRateUpDown` 50, `turnAccelerationUpDown` 50, `startEnabled` false); `FireTurret` (720721: `fireRate` 100, `minFireRange` 0, `maxFireRange` 300, `maxFireAngle` 360, `maxFireElevation` 360, `enemyTeam` `#951af706` (the `team` of Ironhide's block; Megatron's `team` is `#a54dc247`; Autobot as a name is [assumed]), `coneAngleToAllowFire` 15, `checkLineOfSight` true, `weaponNameImResponsibleFor` 0); `Networked`; `DeployableTurret` (721068: `animOnStart` -100423389, `animOnLand` -1785250356, `animIdle` 2081823275, `animOnUnDeploy` -2026338020); `Collision` (721-ish, `DynamicsData` `mass` 13669.3, `startAsleep` true; taken as written, not checked).
- States: `stateMachineSpecialDeployTurret` (11258, list 13931): three entries by form: `stateAttackSpecialDeployTurret` (robot, global_states 155), `stateAttackSpecialDeployTurretJump` (183), `stateAttackSpecialDeployTurretHover` (169); entry 0 `connection` is `<see object at 0x8e14>` to `outputState` `<see object at 0x706c>` [data]. Which connection leads into this machine from `stateAttackSpecial` is [unknown].

Steps [game] unless tagged
- B-start: S1 to S7. S2b stops (walk `attr+0xd0` true). S4: type -1, nothing.
- B-shot: `WeaponFireSpecialEvent` in the `SpecialAnimSet` clip fires E1 to E2: `Blackout_SpecialDeployableTurret`. Launch and deploy as Ironhide's [assumed: same kind of object; NOT read].
- B-turret: the placed `blackout_turret` turns its `TurretRotator` with `maxTurnRateLeftRight` 70 and `maxTurnRateUpDown` 50 (the unit is not checked; read as degrees per second [assumed]) (`trackYaw` and `trackElevation` both on, starts disabled). `FireTurret` fires at targets of team `#951af706` within `maxFireRange` 300 (metres [assumed]), in `coneAngleToAllowFire` 15, with line of sight, at `fireRate` 100 [data]. What it fires, and the unit of `fireRate`, are [unknown]: `weaponNameImResponsibleFor` is 0. Candidate: the turret's gun attribute `Blackout_TurretGatlingAttribute` (global_states.txt 80540) [assumed, not checked].
- B-end: the state `stateAttackSpecialDeployTurret` plays once (`playAnimOnce`); no finish flag is set [data]. Its end is [unknown], as for Ironhide.

## What is shared between the three

- The start and the event code (S1 to S7, E1 to E2): one path for all. Only the type (Megatron 4, the others -1) and the names differ.
- The gun attributes: the same heat numbers, `isSpecial` true, the same rumble strength; the turrets' attribute blocks are identical to each other except `fireDummyXYAngle`.
- Cooldown: 23.5 (both turrets) and 29.4 (Megatron). No self trigger. WeaponFromAnim true, so the shot is always from the animation.
- The turrets share their object kind (`TurretLauncher` with `deployableTurretObj`), and differ in what is in the deployed object: Ironhide's has a missile launcher and no rotator, Blackout's has a rotator and a fire-turret.

## Not found / not settled

- `FUN_0084e390` (Megatron walk, S4): what it clears [unknown].
- `0x84be0e49` and `0x302517b4` (S5, HUD integers): not in `names.tsv` [unknown].
- `FUN_0072a550` and `FUN_0072b980` (the timer for `+0x8c` and the "off" message): notes only, not read here [game per notes].
- The launch of a `TurretLauncher`'s `deployableTurretObj` (Ironhide and Blackout): no function read. The port reads only `GunWeapon` and `MissileLauncher` (`core\src\weapons.rs` 95-98; docs/character-weapon-data.md 93); `TurretLauncher`, `DeployableTurret`, `FireTurret` and `TurretRotator` are not read [data].
- Whether a turret stays after the state ends, and for how long (`lifeTime` -1): [unknown].
- Blackout's `FireTurret` shot (what weapon; unit of `fireRate` 100): [unknown].
- Ironhide's special state (list back-references only): [assumed] turret state.
- Which DamageLink event starts Megatron's 200 damage explosion [unknown].
- Megatron's `rateOfFire` 1 unit (shots per second?) [assumed].
- Whether the special's heat is the normal gun's heat meter [unknown].
- Megatron's car gun, `Megatron_SpecialVehicleGunAttribute`, and the Ironhide and Blackout car weapons: not read.
- Blackout's stat branch (`USED_SA_*`, 0072afd0 around line 235): hash not matched [unknown].
- The `Collision` of `blackout_turret` (mass 13669.3): taken as written, not checked [data].
- Notes and ledger: each function read was recorded (`fn.py note`): 007b3d80, 007b3c40, 007ba870, 0084e390. 0072afd0 keeps its ledger entry.

## Turret code (second read, 2026-10-12)

Reader pass on the code behind the two deployable turrets. Tags as above. Addresses are the game's; offsets are `rec+` (the spawned turret record, `param_1` of 0078c2e0 and friends), `wpn+` (the TurretLauncher weapon object, reached through `rec+0x58`), `body+` (the physics body, `thing+0xa4`), `rot+` (TurretRotator), `ft+` (FireTurret). Everything here was read with `fn.py`, and the float arithmetic checked in disassembly where it is marked. Ledger notes are written for each function read (`fn.py note`, tag `game`, or `assumed` where the meaning is a guess).

### 1. TurretLauncher: the throw (`Ironhide_SpecialDeployableTurret`, `Blackout_SpecialDeployableTurret`)

1. [game] Factory `011860b0`: allocates 0x458 bytes, constructs the weapon with `007ab580`, loads it with `007ab9e0` (the weapon loader; kind 4 reads `turretLauncherAttribute` (hash 77fd0d00) into `wpn+0x28`, and `deployableTurretObj` (hash 4b7ff1e8) as the template when the kind is 4). The same weapon class as GunWeapon (`docs/weapon-kinds.md`). The attribute block is loaded by `007bfa30` (below).
2. [game] Shot: the `WeaponFireSpecialEvent` path (`007ba870`, S/E in the shared section above) reaches the weapon's fire. Spawn dispatch `00784170`: `vtable+0x10` of the weapon returns the kind; kind 4 runs: `00745f10` (lookup of the DeployableTurret class, hash ab348ec5, returns a new instance pair), then `FUN_0078c9d0(rec, owner)` (sets `rec+0x38/+0x3c` = owner pair), `rec+0x40 = flags`, `rec+0x44 = param_7`, `rec+0x58 = weapon`, `FUN_0078cab0` (walks a list and sets `+0x1e0 = 1` on each entry: purpose [assumed]), then `0078c2e0` (the launch). Note the dispatch's own kind test order: kind 4 is not followed by the `(param_3 & 4)` gun path, because `iVar2 != 4` guards it.
3. [game] Launch `0078c2e0` (in order):
   a. `FUN_00556000()` makes the object (`thing`), and `thing+0xa4` is its body (`body`). Flag `thing+0x1d4 |= 0x40`. [assumed] `00556000` creates the deployed object from the `deployableTurretObj` template; its tree (rotator, fire turret, missile launcher) is attached there, not read.
   b. Direction: `FUN_00544db0()` gives a dummy `d`; if it is non-null, `v = (d+0x1c, d+0x20, d+0x24)`, `dir = v / |v|` when `|v| > 1e-8`, else `dir = 0`. [game] for the reads; [assumed] `d` is the launcher's fire dummy.
   c. Frame: `align_z_matrix((0,0,1))`, then `rotate_z_matrix(0x40490fdb)` (a literal: +pi, 3.14159274), then `compose_affine`, then `holder_set_affine`. So the spawn frame is turned 180 degrees about z. The data's `fireDummyXYAngle` [180] is NOT read here: the code uses the literal [game]. Where `fireDummyXYAngle` is read is [unknown].
   d. Speed: `s = wpn+0x130` (launchSpeed, data 50). Velocity `= s * dir` (x, y, z), set with `physics_body_set_velocity` (`00557000`) [game for the arithmetic; the body argument is passed in a register, [assumed] it is the body].
   e. Gravity: if `body+0x28 == 0` then `body+0x18..0x20 = (0,0,0)`. Otherwise `body+0x18 = 0`, `body+0x1c = 0`, `body+0x20 = wpn+0x134 * body+0x24` (zGravity, data -64; `body+0x24` multiplies it; [assumed] it is a per-body gravity scale, 1 for a normal body, settled by reading `00557000` or the body's integrator). Also `body+0xac/+0xb0/+0xb4 = 0` [game].
   f. Lifetime: if `wpn+0x138 > 0` then a timer is set with the hash `9dd59fb6` (lifeTime) through `005a9800` on `global_game_state()`. With data `lifeTime -1`, `wpn+0x138 > 0` is FALSE, so no timer is set [game]. The turret is not removed by this path.
   g. `rec+0x4c = 0` (in flight). The holder (`rec+0xc`) gets `+0x60 |= 2` (flags). Also one call `005a9800(&DAT_00dadde0, &pos, 0.1, 0xddf4c407)` (hash `ddf4c407` unknown) [unknown].
4. [game] Limit: `maxTurretsAtOnce` is `wpn+0x13c` (loader `007bfa30` reads the hash `0665af64` at `007bfb3d` and stores it after `+0x138`; the store itself is not in the read part, so the offset is [assumed] from the order and from `0078b0a0`'s compare, which reads `[wpn+0x13c]` in the disassembly at `0078b1ca`).
5. [game] Enforcement `0078b0a0` (per step, through the vtable; no direct caller found): walks the global list of DeployableTurret records (`PTR_LOOP_00c06728`, joined by the ctor `0078af30`). It counts the records with the same owner and `rec+0x4c != 2` (`local_34`), and keeps the one with the smallest `rec+0x48` (`local_2c`). If `wpn+0x13c < count` and `local_2c != 0`, it calls `0078cc10` with `ESI = [esp+0x24]` (the nearest, verified in disassembly `0078b1bf`, `0078b1d6`). So the EXCESS turret is removed. [assumed] the count includes the turret itself, and `rec+0x48` is a time: the ctor `0078af30` sets it from `global_game_state()+0xc` (so the smallest is the OLDEST). Settled by reading `global_game_state` (`005a9cc0`) for `+0xc` and the owner test in `FUN_00544db0`.
6. [game] Removal `0078cc10`: if `rec+0x34` (the object handle) is zero, it sends a damage message (`send_damage_message` `00724700`) with source hash `ce4e7a13` ("Turret") and the message hash `9caa723d` (the "off" message in docs/weapon-mode.md). Else it sends `007247f0` with `-1.0` and sets `rec+0x4c = 2` (removed). [assumed] the first branch removes the object and the second deletes a live one; settled by `007247f0`.
7. [unknown] Owner death, a timer elsewhere, or a world-contact timeout removing a turret: not found. The only removal read is step 5 (the count cap). `lifeTime -1` gives no timer (step 3f).
8. [unknown] Landing / becoming active. In flight `0078b3b0` (below) runs only while `rec+0x4c == 0`. It has no write to `rec+0x4c` in the read part, and `rec+0x4c` is set to 0 only at the launch (3g). So nothing in the code read marks the turret as deployed, and the `animOnLand` anim is not played by any read function.

### 2. The in-flight contact step (both turrets)

9. [game] `0078b3b0(rec, contact)`: returns at once if `rec+0x4c != 0`. Otherwise it reads the contact normal (`contact+0x8..0x10`, normalised), the body up vector (`rec+0xc` body, `+0x28..0x30`, normalised), and if `dot(normal, up) > 0.5` (the surface is level enough) it calls `0078b8e0(rec, contact, 1)`. The other branch (`rec+0x54` set) calls `virtual_noop`, `00745fb0`, and `00562970` (an explode / destroy helper in notes), [unknown] which of these the `rec+0x54` flag is.
10. [assumed] `0078b8e0`: builds an orthonormal contact frame (the sqrt calls, the cross products), then calls `physics_body_set_velocity`. It looks like the generic projectile bounce / stick response (the grenade's bounce, docs/weapon-kinds.md). Not fully read; the mode argument is `param_4`.

### 3. DeployableTurret record (the component)

11. [game] Ctor `0078af30`: vtable `00b5ebf8`, class hash `ab348ec5` registered (`00578ae0`), joins the global list `PTR_LOOP_00c06728`.
12. [game] Factory `0078b2e0`: `FUN_005343a0(&DAT_00d0b470, 0xd)` allocates it, `rec+0x48 = global_game_state()+0xc`, and reads four anim fields (`view_scalar`): `animOnStart` (hash 96faa86e) into `rec+0x28`, `animOnLand` (9db11fd4) into `rec+0x2c`, `animIdle` (b1e160ad) into `rec+0x30`, `animOnUnDeploy` (03cd1cbe) into `rec+0x34`. The values are the numbers stored in the data (for example `animOnStart` -100423389 in the data is an anim id, not a CRC of its name).
13. [unknown] Who plays `rec+0x28..0x34` (the anim ids) is not read. The `DeployableTurret` vtable is not in `export/vtables.tsv`, so its slots (the deploy, undeploy and update methods) are not identified.
14. [game] The census label `DeployableTurret` for `0078adc0` is WRONG: it is the grenade list helper (sets `+0x1e0 = 8`, kind 2 path of `00784170`). `0078b260` and `0078b2d0` are small; not read.

### 4. TurretRotator (Blackout's deployed turret: `rot`)

15. [game] Ctor `00813470` (defaults in radians): min and max pitch `-pi` and `+pi` (`rot+0x50`, `+0x54`), min and max yaw `-pi` and `+pi` (`+0x58`, `+0x5c`), `maxTurnRateLeftRight` 0.5236 (`+0x60`), `turnAccelerationLeftRight` 0.0873 (`+0x64`), `maxTurnRateUpDown` 0.2618 (`+0x68`), `turnAccelerationUpDown` 0.0873 (`+0x6c`). These are defaults only; the data overrides them.
16. [game] Loader `008139b0`: field names by hash; each limit and rate is multiplied by `0.017453292` (pi/180, read from `0xb6a40c` = `0x3c8efa35` in disassembly), so the data's 70 and 50 are DEGREES per second (and the acceleration is degrees per second squared). Stores: `+0x50` minPitch, `+0x58` maxPitch(ordered as the loader calls, [assumed]), `+0x60` maxTurnRateLR, `+0x64` accelLR, `+0x68` maxTurnRateUD, `+0x6c` accelUD. Reads `trackYaw`, `trackElevation`, `startEnabled` and `turretBoneName` too (names in the loader).
17. [game] Update `00813e30` (partly read, 4205 bytes):
   a. Target: `FUN_00815550` (the FireTurret getter, class hash c8990671) gives the FireTurret list. The first FireTurret with its enabled byte `ft+0x4c != 0` gives the target point `ft+0x80, +0x84, +0x88` (written by `0079e920`, step 22). Otherwise the bone frame is used (`vehicle_special_active` branch, not read).
   b. Angles: with the vector `d = target - turret position`, yaw and pitch come from `FUN_0046be80(-dx, dy)` and `FUN_0046be80(-dz, sqrt(dx*dx+dy*dy))`, stored in `rot+0x70` (yaw) and `rot+0x74` (pitch). [assumed] `FUN_0046be80` is atan2 (the argument pattern is atan2 (-x, y); the census label `platform:fmod` is not trusted).
   c. Rates: if the target is absent or `rot+0x60 <= 0` or `rot+0x68 <= 0` (no cap set), the angles are used directly. Otherwise the rates `rot+0x78` (yaw) and `rot+0x7c` (pitch) move toward the error with an acceleration limit: `rate' = clamp(rate - accel*dt, error)` when the error is below the rate, and `rate' = clamp(rate + accel*dt, cap)` otherwise, with `accel = rot+0x64` (yaw) or `rot+0x6c` (pitch), `cap = rot+0x60` or `rot+0x68` [game for the stores and reads at lines 146-196 of the decompile; SETTLED 2026-10-12, see "TurretRotator update, checked": the rate is NOT bounded by the error; it runs to the cap and the move is cut to the error].
   d. The turret moves by the rate times dt (`rot+0x78 * dt`, `rot+0x7c * dt`) and the affine is rebuilt with `rotate_z_matrix(rot+0x70)` (yaw about z) and `rotate_x_matrix` (pitch) (`0056fc50`, `0056fb50`) [game].
   So the unit of the two rates 70 and 50 is DEGREES per second, and the turret's yaw and pitch are limited by the rotator's own min and max (`+0x50..0x5c`, the data gives -180..180, so unlimited in practice). The clamp of the limits is not read.

### TurretRotator update, checked (2026-10-12)

Read: decompile of `00813e30` lines 1-240 (`fn.py --lines`), and the disassembly `0x813e30..0x814eb0` (`work\raw_disasm.py`; float ops and calls in `work\survey\flight\rot_disasm.txt`). The disassembly's esp offsets shift by 4 inside `push ecx` regions (0x814517..0x81452f, and the one before 0x814497), so each store was matched to the decompile's slot by that shift. Steps per axis (yaw uses `+0x60/+0x64/+0x78/+0x70`, pitch `+0x68/+0x6c/+0x7c/+0x74`; `dt` is the step, `[ebp+8]`):

1. [game] Desired: `want_yaw = atan2(-dx, dy)`, `want_pitch = atan2(-dz, hypot(dx, dy))`, `d` = target minus turret point (`FUN_0046be80` is `_CIatan2`, confirmed in its body).
2. [game] Gate: if `+0x60 <= 0` or `+0x68 <= 0` (checked at `0x8143c3..0x8143d4`, `0x8143da..0x8143e8`), the angles are set to the desired ones with no rate and no wrap.
3. [game] Error: `err = wrap(want - angle)` on both axes; `wrap` is `FUN_0056f290`: `2*pi*(frac(x/(2*pi) + 0.5) - 0.5)` via `floor` (`FUN_00542f80`), range [-pi, pi).
4. [game] Rate, per axis: if `|err| <= 0.0001` (`0xb6a404`) the rate is 0. If `err < 0`: `rate = min(0, max(-cap, rate - accel*dt))`. If `err > 0`: `rate = max(0, min(cap, rate + accel*dt))`. `cap` = `+0x60` / `+0x68`, `accel` = `+0x64` / `+0x6c`, both in rad (and rad/s^2). The error is NOT a bound on the rate: no slowing by error here.
5. [game] Move: `mv = rate*dt` (rate per second times the step, so the move is radians per step). If `|mv| < |err|` the move is `mv`, else it is `err` (lands on the target, never past).
6. [game] Angle: `angle = wrap(angle + mv)`, for both axes (`0x814614`, `0x81462d`). No min or max clamp is read in `00813e30` (no use of `+0x50..+0x5c`).

Units: the rates and caps are stored in radians (the loader's 0.017453292), so 70 and 50 are degrees per second and the update is in rad/s with `dt` in seconds. [game]

Port check: `Rotator::step` (core/src/turret.rs) was WRONG in three places and is fixed: (a) the rate was bounded by `|err|` (slowing within 70 degrees), which the game does not do; (b) the angles were clamped to the rotator's min and max, where the game wraps; (c) the pitch error was not wrapped. Also added: the 1e-4 zero rule and the no-cap snap. Tests: `the_rotator_does_not_slow_for_the_error_and_stops_on_the_target`, `the_rotator_with_no_rate_cap_snaps_to_the_target`, the two old rate tests still pass. Test result: `cargo test -p tf2-core turret`: 11 passed, 0 failed.

Still open: the no-target branch (the game aims at the bone frame, lines 45-106, not ported; the port holds the current angles, [assumed]); the cap gate `bVar5` (target present) and the `FUN_00814eb0` rebuild of the affine are not read in full; what sets `+0x80`/`+0xcc` (the aim-reached flag at line 215) is not read.

### 5. FireTurret (Blackout's `blackout_turret` `ft`)

18. [game] Ctor `0079dbc0` (header only): vtable `00b5eed4`. Its slot `+0x24` is the FIRE call (`0079e670`, step 21), NOT identified: the vtable is not in `export/vtables.tsv`.
19. [game] Loader `0079e250`: `fireRate` `ft+0x5c`, `minFireRange` `+0x60`, `maxFireRange` `+0x64`, `maxFireAngle` `+0x68` and `maxFireElevation` `+0x6c` (degrees to radians, `0.017453292`), `enemyTeam` `+0x70`, `coneAngleToAllowFire` `+0x74` stored as `cos(cone in rad)` (`cos_float` at line 128 of the decompile: the cosine is kept, not the angle), `weaponNameImResponsibleFor` `+0x78`, `checkLineOfSight` `+0x7c`. The offsets follow the loader's order [assumed] (each store is a sequence step); the types are by the data field tags.
20. [game] `enemyTeam` is compared with two constants: `0x951af706` (-0x6ae508fa) gives the target-mask bit `0x40000000`, `0xa54dc247` (-0x5ab23db9) gives `0x20000000`, else `0x80000000` (`0079e920` lines 51-56). So Blackout's turret (team `951af706`) takes the bit `0x40000000`; by the notes this is the Autobot side [assumed]. The mask goes into `target_candidates` (`00799cd0`, not read in full) with a query volume built by `FUN_00419e00(…, 8, 5, …)` from `minFireRange`/`maxFireRange` and the angles (the range check is INSIDE that query, not read).
21. [game] Tick `0079e670`: if `ft+0x94` and `ft+0x4c` are set, `0079e920` (target) runs first. Then, if `ft+0x4c` is set, `ft+0x7d == 0`, and the owner pair's object (`00745fb0`) is null or has `+0x40 != 0`:
   - `ft+0x50 = ft+0x50 - dt` (disassembly `0079e6e7-0079e6f7`: `movss`, `subss`).
   - If `ft+0x50 <= 0` (the `jb` at `0079e6fc`): call the fire (vtable `+0x24`, `0079e700-0079e705`), then `ft+0x50 = 1 / fireRate` (`ft+0x5c`) (`0079e707-0079e724`, the `divss` at `0079e720`); the decompile says `fireRate != 0 ? 1/fireRate : 0`.
   - So `fireRate 100` means a reset to `0.01 s`, not `100 shots per second`: the countdown only goes once per tick and the reset discards the remainder (no carry). With the step `dt = 0.032 s` [assumed: `dt` is the game step] the turret fires at most once per tick, about 31 shots per second. The fire rate is `1/fireRate` seconds of countdown, and the real rate is the tick rate once `fireRate >= 31.25`.
22. [game] Target `0079e920`: `FUN_0079f5f0` and `FUN_006e85e0` (not read). The candidates come from `thunk_FUN_01184df0` and `target_candidates(…)`. Each candidate that is not the turret's own owner (`iVar5 != iStack_12c`) and not invincible (`character_invincible` `00724180`) is tested for line of sight when `ft+0x7c` is set: the aim point is the Head bone (`FUN_00578440`, hash 07c159a2) if it exists, else `FUN_0056bee0`, and `FUN_006f0950(…, 0x10)` is the ray test. The nearest (`afStack_78[i] < fStack_110`) is kept. Result: `ft+0x80`, `ft+0x84`, `ft+0x88` = the chosen candidate's position `+0x34, +0x38, +0x3c` [game].
   With NO candidate the aim point is `1000 m` along the owner's basis: `owner+0x1c..0x24 * 1000.0` added to the owner position (`0079ef77`, the `1000.0` constant) [game; the meaning of the basis axis is [assumed]].
   For a flying-vehicle owner (`character_object_lookup` nonzero) the aim is replaced by `0071a460` (not a turret rule) [game].
23. [game] Cone and range: the cosine `ft+0x74` and the range `ft+0x64` are NOT tested in `0079e670`, `0079e920` or `0079dda0`. The range is in the query volume (21). The cone test was not found in the read functions [unknown].
24. [game] Weapon binding `0079dda0` (TurretManager): if `ft+0x78` (`weaponNameImResponsibleFor`) is non-zero, it walks the scene list for the object whose name (`+0x2c`) equals `ft+0x78`, and stores it at `+0x44`/`+0x48`. If the name is 0 (Blackout's turret), the search is skipped. [assumed] the weapon fired by the tick (step 21, vtable `+0x24`) is that bound weapon; with name 0 the bound object is none, so how Blackout's turret fires is [unknown] (the candidate `Blackout_TurretGatlingAttribute` (global_states 80540) is untested).

### 6. Ironhide's turret: the missile launcher (`ironhide_turret`)

25. [unknown] No DeployableTurret, TurretRotator or FireTurret is in Ironhide's deployed object (data). What fires its `MissileLauncher` (`automaticFiring` false, `rateOfFire` 0.8) is not found: no function reads the MissileLauncher class hash `86393933` as an immediate (`find_imm` empty), and the TargetLockable hash `487111f5` is read in `007079f0`, `0070a410`, `007bab80`, `007bacd0` (not read here; they are the lock-on code for characters, probably not the turret's). Candidate: the missile launcher's own owner trigger (the weapon manager with the turret as a weapon), [assumed], not checked.

26. [game] (weapons session, 2026-10-16) Turret states 1 and 3 exist: `0078c7e0` (no direct
    caller found) places a turret with `rec+0x4c = 3`, the holder hidden (`+0x60 |= 1`), its
    physics off and the just-launched bit (`|= 2`) set, the lifeTime timer as in the throw, zero
    velocity. Its contact (`0078b860`, called from `008cf400`) acts in state 1 or 3: for 3 it
    shows the holder, turns physics back on and clears bit 2, then runs `0078b8e0(rec, contact,
    0)`; in both it sets `rec+0x54` (the flag `0078b3b0`'s other branch tests: [assumed] "has
    landed"). Who sets state 1 is still [unknown].

### What a Rust port would need (from the read parts)

- Launch: the dummy direction `v = (d+1c, d+20, d+24)` (normalised), `speed = wpn+130` (50), `zGravity = wpn+134` (-64) scaled by `body+24` (gravity only when `body+28 != 0`), `lifeTime = wpn+138` (-1: none), `maxTurretsAtOnce = wpn+13c` (1), the 180-degree z turn (literal).
- Cap: count the owner's live turrets; if above the max, remove the nearest-to-key one (`+48`, oldest if a time).
- Rotator: limits in degrees (`-180..180`), rates 70 and 50 deg/s, accelerations as stored, target from the FireTurret.
- FireTurret: `fireRate` countdown per tick (reset `1/fireRate`), team mask `0x40000000` for `951af706`, candidates, nearest, Head-bone LOS when `checkLineOfSight`, 1000 m default aim.

### Not found / not settled

- Landing and "deployed" state (step 8); the anim play calls (`rec+0x28..0x34`).
- How the deployed object's tree is instanced (`00556000` assumed; `0078cab0` and `0078c9d0` not fully read).
- The turret's removal other than the count cap (owner death, timers).
- `fireDummyXYAngle` read site; the literal pi is used instead.
- The fire call (FireTurret vtable `+0x24`, vtable `00b5eed4` not identified) and which weapon it fires for Blackout (name 0).
- The cone test (`ft+0x74`) and the range test (inside `target_candidates`/`00419e00`, not read).
- The clamp order of the rotator's rate limit (step 17c) and `FUN_0046be80` identity (atan2 assumed).
- `0078be90` (list cleanup, `[assumed]`), `0078cab0` (`+0x1e0`), `0078b8e0` (contact response, assumed).
- Ironhide's missile trigger (step 25).

## Turret gaps (third read, 2026-10-12)

Reader pass on the three open questions. Vtables are read from the executable with `tools\pe_read.py vtable` (they are NOT in `export\vtables.tsv`, which has neither 00b5ebf8 nor 00b5eed4). Ledger notes were written for each function read (`fn.py note`).

### Q1. Landing and life of the DeployableTurret (`rec`, vtable 00b5ebf8)

1. [game] Vtable 00b5ebf8, slots used: `+0x04` 0078b260 (register the component with the holder, `turret_register`); `+0x08` 0078cd50 (per-step update, `turret_update`); `+0x0c` 0078c2b0 (message handler, `turret_message`); `+0x10` 0078b0a0 (count cap, step 5 of the second read). Slot `+0x08` is the per-step update that does all removal below.
2. [game] The in-flight contact step 0078b3b0 is NOT in this vtable (no slot holds it). `fn.py callers` finds no direct caller. Who calls it: [unknown].
3. [game] "Landing" in the code read: 0078b3b0 runs only while `rec+0x4c == 0`, and calls 0078b8e0 when `dot(normal, up) > 0.5` (the bounce/stick response, [assumed]). The only writes to `rec+0x4c` are 0 at launch (second read 3g) and 2 at removal (0078cc10). No read function sets a "deployed" state, so the state from in-flight to deployed is [unknown].
4. [game] Timed event at launch: 3g calls `005a9800(&DAT_00dadde0, &pos, 0.1, 0xddf4c407)`. 005a9800's body (read) inserts a message node holding `FUN_005aa020(hash)` into a list; the float argument (0.1) is NOT used in the read body, so the delay is [unknown] (the delivery code is not read). When the message ddf4c407 arrives, 0078c2b0 clears bit 1 (`0x2`) of the holder's `+0x60` (`holder = rec+0xc`). That is the only "landed" effect found, and it does not change `rec+0x4c`.
5. [game] Anim ids: factory 0078b2e0 stores `animOnStart` at `rec+0x28`, `animOnLand` at `+0x2c`, `animIdle` at `+0x30`, `animOnUnDeploy` at `+0x34` (hashes 96faa86e, 9db11fd4, b1e160ad, 03cd1cbe). A body grep for those four hashes finds ONLY 0078b2e0. Which id plays when, and any duration: [unknown]. No animation length or number is in the read code.
6. [game] Every removal read (corrects the second read's step 7, which found none beyond the count cap):
   a. Count cap: 0078b0a0 (vtable `+0x10`) calls 0078cc10 on the oldest of the owner's live turrets when the count exceeds `maxTurretsAtOnce` (`wpn+0x13c`).
   b. Owner test in the update 0078cd50 (after the `rec+0x4c == 2` early return): the owner pair at `rec+0x38` (`lea eax,[ebx+0x38]`, 0078ce37; `FUN_00544db0` copies it). If the owner's first word is null, the turret is removed (`je 0x78ceaa` at 0078ce53 jumps to `mov esi,ebx; call 0x78cc10` at 0078ceaa). Otherwise `character_object_lookup` (0070a3c0) is called, and if the character is non-null and `char+0x148` bit 3 is set, the turret is removed (disasm 0078ce9c `mov edx,[eax+0x148]`, 0078cea2 `shr edx,3`, 0078cea5 `test dl,1`, 0078cea8 `je` skips). [game]. What bit 3 of `char+0x148` means: [assumed] a dead or out-of-play flag (weapon-mode.md lines 740 and 1037 use the same bit to exclude a character from targeting). Settled by finding the writer of `+0x148`.
   c. Message 9dd59fb6 (lifeTime) to 0078c2b0 calls 0078cc10. The timer is set only when `lifeTime > 0`; both turrets have `lifeTime -1`, so this never fires [game].
   d. 0078cc10 itself: `+0x34` (object handle) zero sends the damage message (source ce4e7a13, "Turret"; message 9caa723d) and removes; else sends 007247f0 with -1.0 and sets `rec+0x4c = 2` [game, second read].
   e. Not found in any read function: the turret's own health, a timer other than lifeTime, distance from the owner, an owner form change. The owner's form change may reach the bit-3 test (b) but this is [unknown].
7. [game] Removal correction: the update also runs while `rec+0x4c == 0` (in flight), so an owner gone or flagged before landing removes the turret at once.

### Q2. Blackout's shot (FireTurret vtable `00b5eed4`)

8. [game] Slot map of 00b5eed4 (pe_read): `+0x08` 0079e670 (tick, countdown), `+0x10` 0079dda0 (weapon bind), `+0x20` 0079e4d0 (not read), `+0x24` 0079f0f0 (FIRE), `+0x3c` 0079fc10 (not read). Slot `+0x24` is index 9, so the fire is 0079f0f0 (ledger `fire_turret_fire`).
9. [game] 0079f0f0 spawns nothing itself. Its order: return if `ft+0x4c == 0` (0079f100); the owner pair is `ft+0x44` (`lea eax,[ebx+0x44]`, 0079f10c), which is the BOUND WEAPON (set by 0079dda0, step 24 of the second read). It calls the weapon's vtable `+0xa8` (slot 42 = 0074f1f0, not read). If that is zero: the target is picked (`fire_turret_target` 0079e920 when `ft+0x94 == 0`, else a manager scan 007a0d50), and the weapon's vtable `+0xc4` is called with `(ft+0x80 aim point, out handle)`.
10. [game] The cone test is in disassembly: `0079f547 comiss xmm0,[ebx+0x74]` then `0079f54b jbe 0x79f561`. So the weapon's trigger is called only when `dot(aim direction, weapon direction) > ft+0x74` (`ft+0x74` is the cosine of the cone, loader 0079e250). Both directions are normalised first (0079f4fc, 0079f516 `sqrt`, `FUN_005571b0`).
11. [game] Weapon `+0xc4` = `weapon_trigger_held` (007aca50, vtable 00b5fba0 slot 49). Its offsets are C array indices (`param_1[n]`, byte offset `4n`): the aim is stored at `param_1[0x108..0x10a]` (byte `+0x420..0x428`), and if the attribute `param_1[0xe8]` (byte `+0x3a0`; its flag `+0x62` clear, readiness `vtbl+0x10`, `+0x18` / `+0x19a` tests, `+0x24 <= param_1[0xf2]` (byte `+0x3c8`) clock) it calls `weapon_fire_interval` (007a8f20) and `vtbl+0x60`. Then if `vtbl+0x8c` is true it calls `vtbl+0xcc` = `weapon_fire` (007ae010), which spawns through `weapon_spawn_dispatch` (00784170, the kind's path). So Blackout's turret shot is the BOUND WEAPON's own normal fire, with that weapon's projectile and damage data.
12. [data] Which weapon: `ft+0x78` (`weaponNameImResponsibleFor`) is 0 for Blackout's turret (second read, step 24). With 0, 0079dda0 skips the scene scan and takes `FUN_0078fc80` (new ledger note `scene_class_3d2c29e2`): the lux::Scene component of class hash `3d2c29e2` (005781b0 lookup). That hash is not in `names.tsv` (checked, `tf2_names.py lookup`), so which class it is: [unknown]. Therefore the shot's weapon, its node, and its damage are NOT settled. The candidate `Blackout_TurretGatlingAttribute` (global_states 80540) is [assumed], untested.
13. [game] Numbers: the reload between shots is `1/fireRate` (`ft+0x5c`, the countdown in 0079e670, second read step 21). `fireRate` for Blackout's turret is not read (the data value is not in the notes).

### Q3. Ironhide's trigger (`ironhide_turret`, TargetLockable + MissileLauncher)

14. [data] Embedded `ironhide_turret` in `work\survey\weapons\ironhide.txt` (line 659214, "3 roots"). Its direct components (indent 24-28): Hierarchy (659551), DestructInWater (659636), TargetLockable (659640), MissileLauncher (659653). The MissileLauncher's projectile is `flyingvehicle_homingmissiles` (660327). No AI, brain, controller, FireTurret, or TurretRotator component is on the turret. ControllerRumble (662304) sits deeper in the missile's tree, not on the turret.
15. [game] The MissileLauncher's weapon vtable is 00b5fba0 (the same as GunWeapon / missile launchers). Trigger slots: `+0xc4` (49) weapon_trigger_held 007aca50; `+0xc8` (50) weapon_trigger_released 007acb90; `+0xcc` (51) weapon_fire 007ae010; `+0xd0` (52) 007acd30 (not read). Callers of the `+0xcc` slot (decomp grep): 007acd30, 007acb90, 007aca50, 007ac370 and others not weapon code.
16. [game] Who calls the trigger for a non-player owner, read so far:
   a. The special path: animation event WeaponFireSpecialEvent, 007ba870 (`special_weapon_fire_event`) -> the character's weapon manager (`char+0x238`) -> 007b3d80 (`special_weapon_fire_select`: the name hash at `char+0xd4` on foot, `+0xe8` in a car) -> 007b3c40 (`special_fire_weapon_by_name`): scans the manager's 8 slot pairs (`manager+0x34`) for a weapon whose name (`+0x2c`) equals that name, and calls its `+0xc4` with the aim.
   b. The FireTurret path: 0079f0f0 (Blackout only, step 9).
   c. The player path: 007b37f0 (`manager_trigger_held`, from stateWeaponModeAttack). Not a non-player path.
17. [game] For Ironhide the special name is the TurretLauncher's `Ironhide_SpecialDeployableTurret` (-268772908). Path 16a fires the TurretLauncher, and that launches the deployed turret (kind 4, 0078c2e0). So the turret's OWN MissileLauncher is reached by path 16a only if it sits in the manager's 8 slots. Nothing read adds the deployed object's weapons to `manager+0x34`. [unknown]: the trigger of `ironhide_turret`'s MissileLauncher is NOT FOUND.
18. [assumed] Candidate: the deployed object's weapons are added to the owner's manager when the object is built (`00556000` or `0078cab0`, neither read in full). Settled by reading 00556000 and the writers of `manager+0x34` (the manager's add function, not yet located). Its `automaticFiring` false and `rateOfFire` 0.8 mean it needs an external trigger, as in 16a.

### Not settled (this pass)

- Q1: the states and anim plays (step 5); the 0.1 s delay (step 4); the contact step's caller (step 2); the meaning of `char+0x148` bit 3 (step 6b); health, timers, distance (6e).
- Q2: the weapon for name 0 (class `3d2c29e2`, step 12); the damage; `fireRate` value; vtable slots 42 (0074f1f0) and 52 (007acd30), and the owner `+0x44` set-up for name 0.
- Q3: the trigger of Ironhide's turret MissileLauncher (steps 17-18); manager add function.
- Not read: 00556000, 0078cab0, 0078c9d0 (the deployed tree and its slots), 007ba870's full body, 007b37f0's callers.
- `ddf4c407` and the `005a9800` calls (unknown names).
- `rec+0x54` flag meaning (step 9).
- Ledger: 19 functions noted this pass (`fn.py note`), including `0078adc0` relabelled as grenade list helper.

## Turret weapons from the data (fourth read, 2026-10-12)

Data-first pass (the object trees and attribute blocks), then the trigger. Dumps: `work\survey\weapons\blackout.txt` (`blackout_turret` at 702343, end ~721195), `work\survey\weapons\ironhide.txt` (`ironhide_turret` at 659214, end ~664348 plus its anims to ~680000). Global data: `export\data\global_states.txt`. Tags as above.

### 1. The class hash 3d2c29e2 is not a class

- [data] Not in `names.tsv` (89,368 names). A CRC-32 and an h*33 test of every name in it, and of every exported string (`export\strings.tsv`), with and without `lux::`, `Lux` and `Scene` prefixes: no match. The earlier candidate names (GunWeapon, Weapon, MissileLauncher, FireTurret...) are not it either.
- [game] `0078fc80` (read this pass) calls `005781b0` with the key 0x3d2c29e2. `005781b0` is a hash-table lookup over the scene's node table (bucket = key mod size, match where node+8 == key), returning the node's object with refcount+1. So the key is a scene-table key, not a component class. The ledger label "lux::Scene component of class 3d2c29e2" is WRONG; the note in the third read's Q2 step 12 is superseded.
- [unknown] Which scene object carries that key. Not in the turret data, not in `bumblebee.txt`, `global_states.txt` or `global_tuning.txt`. [assumed] it is the script name of some level object, looked up when a FireTurret has `weaponNameImResponsibleFor` 0.

### 2. Blackout's turret: object tree (`blackout_turret`)

Direct children of the embedded object, in order [data]:
- `Hierarchy` (702635).
- `DestructInWater` (702710) and `TargetLockable` (702714; not read in full).
- `AnimRefBundle` (702727): the six clips (animID, length in ticks): `animOnStart` -100423389 (24), `animOnLand` -1785250356 (90), `fireAnimation` -118708495 (14), idle 2081823275 (120), `overheatAnimation` 1510801051 (60), `animOnUnDeploy` -2026338020 (70).
- `Destructable` (716718): `strength` 60, `destructOnKilled` true, `targetable` true, `targetableByAI` true, `LuxScriptName` 0, `on_destroy_self_trigger` true.
- `TurretRotator` (720700): as the second read (`trackYaw`, `trackElevation`, rates 70 and 50 degrees per second).
- `FireTurret` (720721): `LuxScriptName` -833717741 (= hash ce4e7a13 "Turret"), `fireRate` 100, `minFireRange` 0, `maxFireRange` 300, angles 360 and 360, `enemyTeam` 951af706, `coneAngleToAllowFire` 15, `weaponNameImResponsibleFor` 0, `checkLineOfSight` true.
- `Networked` (720736).
- `GunWeapon` (720742): `weaponName` 0, `startBoneAttachment` -1093492442, dummies 0..14, `fireSound` -278946228, muzzle `w_muzzle_flash_blackout_gatling`, `fireAnimation` -118708495, `overheatAnimation` 1510801051, `gunAttribute` 454803683 = `Blackout_TurretGatlingAttribute` (hash 1b1bc0e3), `gunMPAttribute` -672461154 = `MP_Blackout_TurretGatlingAttribute` (d7eb0e9e), `gunAIAttribute` 34620490 = `Blackout_AI_TurretGatlingAttribute` (0210444a), `bulletProjectileObj` = `blackoutgatling` (an embedded object with `BulletTrace`, SolarPlasma impact; the dump prints it as `<see object at 0x2181a0>`, the same instance first printed at blackout.txt 693012 in the main gatling's GunWeapon, so [data] but only by the dump's back-reference).
- `DeployableTurret` (721068): the four anim ids (second read).
- `Collision` (721076, 721142), `RenderThing` (x4).

So the turret carries its OWN gatling GunWeapon in its tree. That is the answer to "what does Blackout's turret shoot" as far as the data goes. The third read's candidate (`Blackout_TurretGatlingAttribute`) is confirmed as the turret's own gun attribute, not the main gatling's (`Blackout_GatlingGunAttribute`, hash cdc5d9f9, which is the one on Blackout himself at blackout.txt 693012).

### 3. Blackout's turret gun numbers (`Blackout_TurretGatlingAttribute`, global_states.txt 80540) [data]

- Fire: `numProjectilesPerFire` 1, `rateOfFire` 0.666667, `automaticFiring` true, `burstPerFire` 3, `timeBetweenEachBurst` 0.08, `hasChargedShot` false, `isSpecial` false, `hasLaserSight` true.
- Heat: capacity 100, `heatMeterIncrementPerRound` 6, `heatMeterDecrementPerSecond` 20, `heatMeterCooldownTilDecrement` 0.666667, `heatMeterOverHeatPeriod` 5.
- Shot: `bulletSpeed` 1000, `bulletRange` 200, `bulletDamageFallOffRange` -1, `damage` 8, `minDamage` 1, `positionalDamageMultiplier` head 2, others 1, `knockBackSpeed` 15, `knockBackAngle` 35, `tracerFirePerRound` 1, `aimAssistMaxRange` 200, `rumbleFireDuration` 0.25, `rumbleFireStrength` 1, `playerNoiseRadius` 0.01, `generalNoiseRadius` 0.1.
- `snakerInfo` (a snaker side-effect, not a shot): damage 10, `timeTilDies` 10.
- AI variant `Blackout_AI_TurretGatlingAttribute` (global_states.txt 80021): `rateOfFire` 0.16, `burstPerFire` 1, damage 1, `heatMeterIncrementPerRound` 3, `heatMeterDecrementPerSecond` 100, `heatMeterOverHeatPeriod` 2, `generalNoiseRadius` 1.
- [unknown] Which of the player or the AI attribute the turret's gun uses. Its owner is the player's side, so the player block is the likely one [assumed]. Also unknown: the unit of `rateOfFire` (the shot interval is in `weapon_fire_interval` 007a8f20, not read), and whether `FireTurret.fireRate` 100 or the gun's `rateOfFire` limits the shots (the second read's `1/fireRate` reset is the turret's own countdown).

### 4. Ironhide's turret: object tree (`ironhide_turret`)

- [data] Direct children: `Hierarchy` (659551), `DestructInWater` (659636), `TargetLockable` (659640), `MissileLauncher` (659653), `AnimRefBundle` (664349). No `GunWeapon`, `FireTurret`, `TurretRotator`, `Destructable`, AI or brain component in the tree (checked by the shallow object listing and by a grep of the tree for auto, lock, fire, trigger, target, script, ai).
- `TargetLockable` (659640): `LuxScriptName` -833717741 (ce4e7a13 "Turret"), `targetableByDecepticons` true, `targetableByAutobots` false, `startEnabled` true, `acceptNullTriggerMessages` true, `radius` 2, `height` 3.8, `zOffset` 0, `onlyMakeCrosshairTurnRed` false.
- `MissileLauncher` (659653): `weaponName` 0, `minFireDummyIndex` 0, `maxFireDummyIndex` 1, muzzle `w_muzzle_flash_homing_missiles`, `fireAnimation` -118708495, `overheatAnimation` 1510801051, `missileLauncherAttribute` -1792689951 = `Ironhide_TurretMissileAttribute`, `missileLauncherMPAttribute` 1507161756 = `MP_Ironhide_TurretMissileAttribute` (59d5769c), `missileLauncherAIAttribute` 354638326 = `Ironhide_AI_TurretMissileAttribute` (152359f6), `missileProjectileObj` = `flyingvehicle_homingmissiles` (660326). No `LuxScriptName` on the launcher.
- Anim clips: the same six ids as Blackout's (the `AnimRefBundle` at 664349).

### 5. Ironhide's missile numbers [data]

- Player block `Ironhide_TurretMissileAttribute` (global_states 89726): `rateOfFire` 0.8, `automaticFiring` false, `burstPerFire` 1, heat +20 per round, -25 per second after 1.5 s, overheat 5, `aimAssistMaxRange` 500, `overwriteDirectDamage` 0, `overwriteExplosiveDamage` 24, `overwriteExplosiveMinDamage` -1, `isSpecial` false.
- AI block `Ironhide_AI_TurretMissileAttribute` (global_states 89008): `rateOfFire` 1.8, `automaticFiring` false, heat +20, -25/s, cooldown 1.5, overheat 6.5, `overwriteDirectDamage` 3, `overwriteExplosiveDamage` 6, `overwriteExplosiveMinDamage` 3, `aimAssistMaxRange` 500.
- CONFLICT with the earlier note (line 78-ish, "explosion damage 20"): that 20 is the projectile's `Explosion` `damage` in the projectile tree. The attribute's `overwriteExplosiveDamage` 24 (player) or 6 (AI) may replace it. Which applies: [unknown] (the overwrite semantics are not read).
- `automaticFiring` is false in both blocks, so the launcher does NOT fire itself; something external must pull its trigger (section 6).

### 6. What fires each turret: the animation's trigger (both turrets) [data]

- The `animOnLand` clip (`-1785250356`) of BOTH turrets ends with two `AnimSendTriggerEvent`s, the only trigger events in either tree:
  - Blackout (length 90): tick 88, `LuxScriptName` -1061270203 (= c0be4d45 "TurretDamage"), `triggerMsg` f40ccd04 ("Enable"); tick 90, `LuxScriptName` -833717741 (ce4e7a13 "Turret"), `triggerMsg` d5d636c1 ("Trigger").
  - Ironhide (length 86): tick 84, the same "Enable" to "TurretDamage"; tick 86, the same "Trigger" to "Turret".
- So the landing animation ends by sending "Trigger" to the object named "Turret". In Blackout's tree that object is the `FireTurret` (its `LuxScriptName` is "Turret"). In Ironhide's tree the object named "Turret" is the `TargetLockable`; the `MissileLauncher` has no name, so the message cannot reach it by name.
- [game] The message hash d5d636c1 is compared in many engine dispatch functions (`find_imm.py 0xd5d636c1`: 0071fdd0, 007204a0, 00723320, 00729120, 0072a270, 0072a550, 0072afd0, 0072ba89, 00789b10 and others). None of them is in the FireTurret's vtable (`00b5eed4` slots read by `pe_read.py vtable`: 0079dc90, 0079e1b0, 0079e670, 0079dda0, 0079e4d0, 0079f0f0, 0079fae0, 0079fe60, 0079fc10, 007a06c0, 007a06f0). So the Trigger is handled by the generic object messaging, which is NOT read.
- [assumed] "Trigger" starts the turret: Blackout's FireTurret gets it and its enabled byte `ft+0x4c` goes on, so `0079f0f0` starts firing (second read, step 9). For Ironhide, the trigger reaches the TargetLockable, and the missile launcher's fire would need a relay that is not found. SUPERSEDED: the fifth read found every turret component ignores Trigger.
- [unknown] Ironhide's trigger for the `MissileLauncher`: NOT FOUND. Also unknown: the relay from the named object to the weapon, and the `TurretDamage` object (c0be4d45) which is in neither tree.

### 7. Corrections and loose ends

- [game] `0079fe60` (read this pass, noted as `fire_timer_step`) is a per-step countdown on `+0x5c` and `+0x60`. It is NOT the FireTurret's message handler. Its owner is [assumed] a weapon-like object.
- [data] The third read said Blackout's turret had no shot data beyond FireTurret. The turret does carry its own GunWeapon (section 2), so the "what it fires" question has a data answer (section 3). The path from FireTurret to that GunWeapon is still [unknown], since `weaponNameImResponsibleFor` 0 points at the unresolved scene key (section 1).
- Not found: the class or object behind 3d2c29e2; the Trigger receiver code for the two turrets; the Ironhide missile trigger; the unit of `rateOfFire`; which gun attribute (player or AI) applies to a turret; the damage the missile really does (24 or 20 or 6).

## Ironhide's turret trigger (fifth read, 2026-10-12)

Question: what makes Ironhide's deployed turret fire its missiles? Answer below: NOT FOUND. Nothing read starts the turret or its MissileLauncher, and the Trigger message does nothing on either component. Tags as before.

### 1. Who handles Trigger (d5d636c1): the component's own message slot is a stub

1. [game] The vtable slot at `+0x0c` is the message handler (the DeployableTurret 00b5ebf8 uses it, `turret_message` 0078c2b0, third read Q1). Slot index 3 in the three component vtables read with `pe_read.py vtable`:
   - TargetLockable 00b5fd68 (9 entries): index 3 = `008b7f70`.
   - MissileLauncher weapon 00b5fba0: index 3 = `008b7f70`.
   - FireTurret 00b5eed4 (20 entries): index 3 = `008b7f70`.
2. [game] `008b7f70` is 3 bytes: `ret 0x10` (disasm 008b7f70). It does nothing. So a message sent to one of these components at `+0x0c` is ignored, Trigger included. The same stub is shared by all three.
3. [game] The `d5d636c1` compare sites (find_imm) are generic dispatch or other classes. None lies in the TargetLockable slots 007bacd0, 007bafe0, 007bafc0, 007bb230, 007bb290, 007bb7e0, or in 007bab80 / 007bb0b0 / 007bb010. Hits near the turrets: `00723320` (character-level handler, compares Trigger at 0072333f, see step 5). `0072a270`, `0072a550`, `0072afd0`, `0072b980`, `00729120`: not read.
4. [game] Correction of the fourth read's section 6: the FireTurret also does NOT take Trigger at `+0x0c`. So the "Trigger starts Blackout's FireTurret" reading is unsupported. A relay that calls FireTurret slot `+0x24` (0079f0f0) from a message is [unknown].
5. [game] `00723320` (a character's message handler, `param_1` = character, `+0x258` child, `+0x238` weapon manager): a Trigger (`-0x2a29c93f`) goes to the children only when the byte at `+0x258 +0x60` bit 0 is clear (disasm of the test at 00723320's first branch). Then it loops the 8 slot pairs of the manager (`+0x238`, `+0x34 + i*8`) calling their vtable `+0x30`. A Trigger for a deployed object reaches this path only if the object is a character: [unknown].
6. [game] `00724940` is the child broadcast: it calls the child vtable `+0x0c` on every child in `obj+0x10`. So a Trigger sent to the deployed root would reach the MissileLauncher's stub (step 2) and stop there.

### 2. What TargetLockable does (its one real job)

7. [game] Constructor `007bab80` sets vtable 00b5fd68 and flags: `+0x51` = 1, `+0x52` = 0, `+0x53` (enabled) = 0, `+0x54` = 0. It registers class `487111f5` (TargetLockable) through 00578ae0 when the owner param is set.
8. [game] Factory: `007bb410` (a thunk) -> `01196790` -> `007bab80`, then `007badf0`.
9. [game] Slot `+0x04` `007bacd0`: adds the lockable to the global ring `DAT_00d52608` (test `00799ba0`) and calls `00577bb0`.
10. [game] Slot `+0x08` `007bafe0` (per-step): calls `007bb0b0` (step 11), then `011767b0` when `+0x54` and the flag `+0x16` bit 1 are set.
11. [game] `007bb0b0` (the lock setup): gets the owner pair (`00712190`), finds the deployable turret (`00745f10`) and the character (`0070a3c0`). If the character is the owner, the flags are `0x40000020`; else `0x20000020`, and `0x40000000` is added when `+0x51` is set. It then adds the lockable to the lock list (`00799a50` with flags, `01184df0` for the handle), sets the `+0x58` value from the player attributes (`iVar1+0x42` or `+0x50`), and sets `+0x54` = 1.
12. [game] `007bb010` (enable/disable, param 1 or 0): adds or removes the lockable from the lock list (`00799a50` / `00799ba0`) and sets `+0x53`.
13. [game] Slots `+0x10` `007bafc0` (re-enable), `+0x18` `007bb230` (enable), `+0x1c` `007bb290` (disable). Slot `+0x20` 007bb7e0 calls 007bb010.
14. [assumed] `0x20000020` / `0x40000020` are targetable-by-team flags for the lock-on list (the `0x40000000` bit set from `+0x51`, which the constructor defaults to 1). The data fields `targetableByDecepticons` (true) and `targetableByAutobots` (false) are the likely source of `+0x50`/`+0x51`, but the loader that sets them is not read: [assumed].
15. [data] Ironhide's data (`targetableByDecepticons` true, `targetableByAutobots` false, `startEnabled` true, `acceptNullTriggerMessages` true, `radius` 2, `height` 3.8) fit a DECOY-like lock target: enemies can lock on it, it never shoots. [assumed]. Its `acceptNullTriggerMessages` true field is not read in any function found here.

So for Ironhide the TargetLockable is a lock-on target, not a trigger relay.

### 3. The MissileLauncher's own fire path (weapon vtable 00b5fba0)

16. [game] Per-step update `007ac370` (weapon_tick_outline, outline read): burst logic when `weapon+0xfd` > 0 (the burst count, written by `weapon_fire` 007ae010 with the second burst at `+0xfe` clock), with the owner character from `character_object_lookup`, the owner flag `char+0x148` bit 3 test, `scripted_health_controller` (00711260), then `vtable +0xcc` (`weapon_fire`, slot 51) with `(aim, dir, 1, 0, 0)`. So the update never starts a first shot: with `+0xfd` 0 it does nothing. [game]
17. [game] First shot: `weapon_trigger_held` 007aca50 (slot 49, `+0xc4`). It stores the aim at `weapon+0x420..0x428`, calls `weapon_fire_interval` (007a8f20) and `vtable +0x60` when the attribute at `weapon+0x3a0` allows (readiness `vtbl+0x10`, flag `+0x62` clear, `+0x18` / `+0x19a` tests, clock `weapon+0x3c8`), then calls `weapon_fire` (`+0xcc`) when `vtable +0x8c` is true. [game]
18. [assumed] `automaticFiring` false blocks a self-start because the weapon only fires from a held trigger (17) or a burst (16). Where `automaticFiring` is read is not found: [unknown] (possibly in the `+0x8c` test).
19. [game] Who calls `+0xc4` for a non-player owner: the special path (the animation event `WeaponFireSpecialEvent`, 007ba870 -> `007b3d80` -> `007b3c40`, third read step 16a), which scans the owner's manager (`char+0x238`, `+0x34`) for a weapon whose name equals the special's name. For Ironhide the special's name is `Ironhide_SpecialDeployableTurret` (TurretLauncher). The turret's own MissileLauncher is reached only if it sits in that manager. Nothing read adds it: the candidate writers `00556000` and `0078cab0` are NOT the manager add. `00556000` looks up scene key `dd566c80` (`005781b0`, returns the object with refcount+1). `0078cab0` walks a list and sets `+0x1e0` = 1. [game]
20. [unknown] The manager add function (the writer of `manager+0x34`), the deployed object's build (`0078c9d0`, the deployed tree's slots), and the `+0x8c` test.

### 4. Plain answer

- [game] Nothing read makes Ironhide's deployed turret fire by itself. Its MissileLauncher has `automaticFiring` false (data, 89726 and 89008). Its update (16) fires only after a shot has been started, and the start is the held-trigger path (17), reached through the owner's special (19) if the launcher is in the owner's weapon manager.
- [game] The Trigger message does NOT start it: TargetLockable and the MissileLauncher both ignore messages at `+0x0c` (step 2). The 'Trigger' sent at landing ends at the TargetLockable and stops.
- [game] The TargetLockable is a lock-on target for the enemy side (step 15, [assumed]). Read as the code shows it, Ironhide's turret does not shoot at anyone; it is a target that can be locked on. Its missiles (rateOfFire 0.8 for the player block, 1.8 for the AI block) would fire only if the owner's special fires the launcher, which is not shown.
- Not found: the relay from the Trigger message to any fire path; the manager add; the `+0x8c` test; whether the AI block (1.8) is ever used for the turret (the owner is the player's side, so the player block is the likely one [assumed]).

### 5. Corrections to earlier notes

- The fourth read's section 6 ("Trigger starts Blackout's FireTurret") is unsupported: the FireTurret ignores `+0x0c` messages (step 1, 4).
- `docs/weapon-kinds.md` line 83 says `TargetLockable` is `FUN_007bba30`. `007bba30` reads `speed`, `damage` and `timeTilDies` (a projectile attribute loader). The TargetLockable component's constructor is `007bab80`, its factory `007bb410`.
- The census label of `007bab80` (PlayCharPartialAnim) and `007bacd0` (PlayCharPartialAnim) is wrong: they are TargetLockable functions (step 7, 9).

### 6. Still open

- The writer of `manager+0x34` (the add), and whether the deployed object's MissileLauncher is added to the owner's manager.
- Who sends the Trigger in a form that reaches a fire path (is there any?), and whether the generic message dispatch maps Trigger to a vtable slot (candidate: FireTurret `+0x24`).
- `acceptNullTriggerMessages` and what `+0x50`/`+0x51` are set from.
- `0078c9d0` (deployed tree slots) and the `+0x8c` test.


## Ironhide's turret does not fire: the manager writer found (sixth read, 2026-10-18)

- [game] The writer of `manager+0x34` is `007b0f10` (`weapon_manager_fill`): it fills the 8 slot
  pairs from the owner's scene `Weapon` components (`scene_find_weapon` 007bb780) and stamps each
  weapon's slot index at `+0x30`. Its only caller is `00713180` (`character_weapons_setup`), which
  makes the manager at `char+0x238` when the CHARACTER's data has `weapons`. A deployed turret is
  not a character, so its MissileLauncher is in no manager and the special scanner 007b3c40 can
  never reach it.
- [game] Every caller of a weapon's held trigger (vtable `+0xc4`, decompile grep over the whole
  export): FireTurret `0079f0f0` (Grindor's turret), `007b37f0` (the player's trigger),
  `007b3c40` (the special by name), `0074cd20` (the flamethrower's own held slot). The rest are
  fmod / d3d / std / scaleform library code. None reaches `ironhide_turret`'s launcher, whose
  `automaticFiring` is false.
- So, as read: Ironhide's deployed turret never shoots. It is a lock-on target for the
  Decepticon side (`TargetLockable`, `targetableByDecepticons` true, `targetableByAutobots`
  false): a decoy. The port's gunless Ironhide turret is the game's behaviour, not a stand-in.
  [assumed] no indirect call the decompiler missed. What would settle it: a recording of
  Ironhide's special, or the user's play test.
- Open, and not robot work: enemy AI taking the decoy as a target (the lock list 00799a50).

## Correction: Ironhide's turret HAS a FireTurret (2026-10-27, found by the SDK's exporter)

- [data] Walking `characters\ironhide.str` for `FireTurret` things (the SDK's `mechanics.special.
  turret`) finds one, in the deployed turret's thing list itself. The list, in order:
  `DestructInWater`, `TargetLockable` (LuxScriptName `Turret`), `MissileLauncher`,
  `AnimRefBundle`, `Destructable` (`TurretDamage`), `TurretRotator` (`Turret`), `FireTurret`
  (`Turret`: `fireRate` 25, `maxFireRange` 300, `weaponNameImResponsibleFor` 0),
  `Networked`, `DeployableTurret`, then the collisions and render things. Blackout's has the same
  parts, with a `GunWeapon` where Ironhide's has the `MissileLauncher`. The 2026-10-12 grep of
  `ironhide.txt` lines 659214-680000 stopped before them (items 25 and 14 above, and the sixth
  read, are wrong on this point).
- So the landing clip's Trigger at tick 86 to `Turret` reaches a `FireTurret` and a
  `TurretRotator` as Blackout's does, and FireTurret `0079f0f0` is the held-trigger caller the
  sixth read lists for Grindor's turret: by the same path it fires the `MissileLauncher`
  (`automaticFiring` false is no obstacle: the FireTurret presses it).
- SETTLED 2026-10-27: the turret was seen firing in the game, much like Grindor's. The reference
  fires its MissileLauncher through the turret's own FireTurret and TurretRotator numbers, read
  from the data (`turret::read_aiming`, `weapons::read_turret_parts`), on foot and in the car form.
  Still [assumed]: which target the missile locks on (taken as the FireTurret's).
