# Weapon kinds GrenadeLauncher and FlameThrower: data, code, and what is not read (2026-10-12)

Ticket: READ only. No source edited, nothing built. Tags: `[game]` read in code, `[data]` read in
data dumps, `[assumed]` a guess, `[trace]` none used. Scratch: `work\survey\weapons\longhaul.txt`.

## GrenadeLauncher: data

Component (longhaul dump, `work\survey\weapons\longhaul.txt` line 583858; the grep also finds
the same type at 622041 in the vehicle list) [data]:

- weaponName, startBoneAttachment, randomizedSpawnPoints, minFireDummyIndex, maxFireDummyIndex,
  genericWeaponSoundTable, muzzleFlashEffect, muzzleFlashObject, weaponIcon, idleAnimation,
  exitAnimation, exitOverheatAnimation, exitCharacterAnimation, fireAnimation, overheatAnimation,
  alwaysAppear, chargeMaxProjectileObj, chargeId, chargeMaxReadyId, minChargeEmissionRate,
  maxChargeEmissionRate, chargingAnimation, chargedAnimation, gunWeaponSoundTable
- grenadeLauncherAttribute: -1778075375 = `LongHaul_GrenadeLauncherAttribute` (CRC checked)
- grenadeLauncherMPAttribute: 1883872749 = `MP_LongHaul_GrenadeLauncherAttribute`
- grenadeLauncherAIAttribute: 1493767877 = `LongHaul_AI_GrenadeLauncherAttribute`
- grenadeProjectileObj: an embedded `longhaulgrenade` object (7 roots; model `LongHaulGrenade`)
- Cluster attribute inside the projectile: grenadeLauncherClusterAttribute 932435982 =
  `DefaultGrenadeLauncher` (`tf2_names.py lookup`) [data]

Attribute block `LongHaul_GrenadeLauncherAttribute` (`export\data\global_states.txt` 91489) [data]:

- Base fields (shared with every gun/launcher, read by `FUN_007bbc30`): numProjectilesPerFire 1,
  rateOfFire 0.4, automaticFiring false, burstPerFire 1, timeBetweenEachBurst 0.03,
  hasChargedShot true, timeTilChargeStarts 0.2, chargeDurationTilMaxEffect 4,
  chargeHoldDuration -1, fireOnlyFullyCharged false, minChargeDamageMultiplier 1,
  maxChargeDamageMultiplier 2, minHeatMeterIncrement 5, maxHeatMeterIncrement 10,
  chargeNumProjectilesMaxMultiplier 3, chargeHeatIncrementPerMultiplierIncrement 14,
  heatMeterCapacity 100, heatMeterIncrementPerRound 8, heatMeterDecrementPerSecond 22,
  heatMeterCooldownTilDecrement 2.2, heatMeterOverHeatPeriod 7, heatMeterOverHeatCooldownRate -1,
  heatMeterOverHeatRateWhileCharging 9, rangedSwipe* (all off or 0), listenToWeaponAnimEvent true,
  isSpecial false, hasLaserSight false, snipeCameraEnabled false, useFacingAsTarget false,
  isVehicleModeWeapon false, weaponChildren [1118806479], weaponParent 0, alternatingFire false,
  useFireDummyMatrix false, useOwnerRootMatrix false, fireDummyXYAngle and fireDummyZAngle (empty),
  playerNoiseRadius 0.22, generalNoiseRadius 0, aimAssistMaxRange 100, rumbleFireDuration 0.4,
  rumbleFireStrength 0.85, damageFlags [#bitsa00], specialCaseDamageStuff {stunTime 0, slowTime 0,
  dotTime 0, heatIncrement 0, knockBackSpeed 15, knockBackAngle 35}
- Grenade fields (`FUN_007bea00` names them, in this order): overwriteDirectDamage (0),
  overwriteExplosiveDamage 16, overwriteExplosiveMinDamage 7, damageFlags, specialCaseDamageStuff,
  launchArcAngle 3, grenadeMinLifetime 0.8, grenadeMaxLifetime 1.1, fireTowardsTarget true,
  Zgravity -32, oneOutAtATime false, clusterSpawnables 0, maxClusterAngle 30, isProximity true,
  proximityActivateOnCollision false, proximityRadius 10, proximityExplosionMinDelay 0.02,
  proximityExplosionMaxDelay 0.04, isSticky false, stickOrient false, stickyMinLifetime,
  stickyMaxLifetime, stickyOffsetFromCollision (all 0), bounciness 0.55, friction 0,
  dampeningOnCollision 0.995, launchSpeed 270, tackOnOwnerSpeed false, velocityOrient false,
  spinAxis, randomSpinAxis true, minSpinSpeed 180, maxSpinSpeed 360, snakerInfo.
  Values are the longhaul ones above (the grenade fields are in that block after the base fields;
  the dump was cut at 90 lines, the bounce and spin values come from `global_states.txt`
  91489-91600) [data].
- Offsets (`decomp_lookup.py fields 007bea00`): overwriteDirectDamage +0x130, overwriteExplosiveDamage
  +0x134, launchArcAngle +0x158 (stored times 0.017453292, degrees to radians), grenadeMin/MaxLifetime
  +0x15c/+0x160, Zgravity +0x168, isProximity +0x178, proximityRadius +0x17c, isSticky +0x188,
  bounciness +0x198, friction +0x19c, dampeningOnCollision +0x1a0, launchSpeed +0x1a4, spinAxis
  +0x1ac..+0x1b8, minSpinSpeed +0x1bc, maxSpinSpeed +0x1c0 (degrees to radians) [game].

Other longhaul grenade attributes [data]:

- `LongHaul_AI_GrenadeLauncherAttribute` (91058): numProjectilesPerFire 3, rateOfFire 1.6,
  automaticFiring false, hasChargedShot false, heatMeterIncrementPerRound 12.5,
  heatMeterOverHeatRateWhileCharging 2, isVehicleModeWeapon false, weaponChildren [1118806479].
- `LongHaul_AI_GrenadeLauncherAttribute2` (91162): same numbers, weaponChildren (empty),
  weaponParent -62878951 (a second launcher as a child).
- `MP_LongHaul_GrenadeLauncherAttribute` (97466): same as the single-player block (rateOfFire 0.4,
  charge on, chargeNumProjectilesMaxMultiplier 3). Its grenade fields were not compared line by line.
- `DefaultGrenadeLauncher` (83905): numProjectilesPerFire 1, rateOfFire 1, automatic true,
  overwriteExplosiveDamage -1, launchArcAngle 0, grenadeMin/MaxLifetime 4, Zgravity -32,
  clusterSpawnables 0, maxClusterAngle 30, isProximity false, proximityRadius 3, bounciness 0.55,
  launchSpeed 30 [data].

## GrenadeLauncher: code

- `FUN_01186060` (01186060) [game]: makes the weapon. Allocates 0x458 bytes, calls the gun and
  launcher constructor `FUN_007ab580` (sets vtable `00b5fba0`, the class that `MissileLauncher`
  uses; see `docs/weapon-mode.md` line 299), then `FUN_007ab9e0`. So a GrenadeLauncher is the
  same weapon class as the launcher and gun, not a separate class. Its fire path is the shared one:
  `weapon_fire` `FUN_007ae010`, `weapon_spawn_projectile` `FUN_007aaeb0` (both in the ledger).
  [assumed for the grenade: the vtable slots are the same as the launcher's, from the constructor].
- `FUN_007bea00` (007bea00) [game]: the grenade attribute loader. Calls the shared attribute loader
  `FUN_007bbc30` (the base fields), then reads the 34 grenade fields listed above through
  `data_field_by_hash` (34 calls). Also calls `TriggerToDamage` (`FUN_00744f10`) and
  `TargetLockable` (`FUN_007bba30`).
- `FUN_007866e0` (007866e0) [game]: the GrenadeProjectile constructor. Sets vtable `00b5ebe0`, a
  default scale 1.0, and clears flags at +0x59, +0x5a, +0x5b and the fields after them.
- `FUN_01184930` (01184930) [game]: the projectile factory. Calls `FUN_005343a0`, then
  `FUN_007866e0`, then reads one data field into the object (+0x28, a view scalar).
- `FUN_00786ff0` (00786ff0) [unread, outline only]: the grenade spawn. Calls look-at-y
  (`FUN_0056ede0`), holder affine (`FUN_006dbd30`), `EffectTimeoutDeleter` (`FUN_007772c0`), and
  `GrenadeProjectile` `FUN_007873f0`. The launch arithmetic (arc, speed, lifetime) is probably here
  but was not read.
- `FUN_007873f0` (007873f0) [game]: per-frame proximity test. Returns early unless flags +0x58 and
  +0x59 are set and +0x5a is clear, and the owner's +0xc4 weapon has +0x178 set (the isProximity
  byte). Then it compares the squared distance against the owner's `+0x17c` (the proximityRadius
  field, squared). Explode-on-proximity uses `FUN_00562970` and `scripted_health_controller`
  (`FUN_00711260`). [assumed: the +0x17c field is proximityRadius, from the attribute offsets above].
- `FUN_007891e0` (007891e0) [unread, outline only]: calls `sqrt`, `send_damage_message`
  (`FUN_00724700`), `DeleteAllProjectiles` (`FUN_0078fa40`), and `FUN_007898a0`. Probably the hit,
  bounce and explosion step (bounciness 0.55, dampening 0.995) and the explosion damage
  (overwriteExplosiveDamage). Not read; the bounce arithmetic is unchecked.

Grenade vs MissileLauncher, what the data says [data] and what the code says [game]:

- Same class and same fire path (`FUN_01186060` -> `FUN_007ab580`).
- Arc and gravity: `launchArcAngle` (3 degrees in longhaul) and `Zgravity` -32 (also the default).
- Fuse: `grenadeMinLifetime` and `grenadeMaxLifetime` (0.8 to 1.1 s in longhaul; 4 s default). The
  missile has a lock-on instead (`docs/weapon-mode.md` "lock-on and the missile").
- Bounce: `bounciness` 0.55, `dampeningOnCollision` 0.995, `friction` 0, and `spinAxis` random with
  180 to 360 deg/s spin. [data] only; the bounce code `FUN_007891e0` is not read.
- Blast radius: `overwriteExplosiveDamage` 16 with `overwriteExplosiveMinDamage` 7 (longhaul) and
  `proximityRadius` 10 for the proximity burst (the default is 3). The radius of the blast itself
  is not a field here; it is not found [unread].
- Charge: `hasChargedShot` true with up to 3 projectiles (`chargeNumProjectilesMaxMultiplier`) for
  the longhaul single-player launcher; the AI variant fires 3 per shot at rate 1.6 without charge.

## FlameThrower: data

Component (longhaul dump, `work\survey\weapons\longhaul.txt` 588893; the grep also finds one at
623560) [data]:

- weaponName, startBoneAttachment, randomizedSpawnPoints, minFireDummyIndex 0, maxFireDummyIndex 0,
  muzzleFlashEffect (None), weaponIcon, idleAnimation, exitAnimation, exitOverheatAnimation,
  exitCharacterAnimation, fireAnimation 0, overheatAnimation, alwaysAppear, chargeMaxProjectileObj
  None, chargeId None, chargeMaxReadyId None, chargingAnimation 0, chargedAnimation 0
- flameThrowerAttribute: 98446350 = `LongHaul_FlameThrowerAttribute`
- flameThrowerMPAttribute: -110639942 = `MP_LongHaul_FlameThrowerAttribute` (its own block at 97345)
- flameThrowerAIAttribute: 477135007 = `LongHaul_AI_FlameThrowerAttribute`
- streamEffect (`w_flamethrower_streama`, a ParticleTemplate using `fx_flamebseq.dds`), impactEffect
  (`w_flamethrower_impact`), SSDImpactPreset (`FlamethrowerBurnSSDPreset`, SSD `SSD_FlameThrower`),
  SSDPeriodChecker 1 [data]
- Three `FlameThrowerDummy` objects (index 0, 1, 2; `delayTilTurningOnNextDummy` 0.35,
  `minScale` 1, `maxScale` 1.25 / 1.35; `timeTilMaxScale` 0.5) with `w_flamethrower_ball01..03`
  particle templates [data]
- The loader `FUN_0074b540` names `DefaultFlameThrower` as a default [game]; the second FlameThrower at dump 623560 was not opened [unread]

Attribute `LongHaul_FlameThrowerAttribute` (`global_states.txt` 91368) [data]: numProjectilesPerFire
1, rateOfFire 1, automaticFiring true, burstPerFire 1, timeBetweenEachBurst 0.01, hasChargedShot
false, heatMeterCapacity 100, heatMeterIncrementPerRound 15, heatMeterDecrementPerSecond 33.3333,
heatMeterCooldownTilDecrement 0.2, heatMeterOverHeatPeriod 4, heatMeterOverHeatCooldownRate -1,
heatMeterOverHeatRateWhileCharging 2, rangedSwipe* off, listenToWeaponAnimEvent true, isSpecial
false, hasLaserSight false, snipeCameraEnabled false, useFacingAsTarget false, isVehicleModeWeapon
false, weaponChildren [1050274702], weaponParent 0, alternatingFire false, useFireDummyMatrix false,
useOwnerRootMatrix false, fireDummyXYAngle/ZAngle (empty), playerNoiseRadius 0, generalNoiseRadius
0, aimAssistMaxRange 12, rumbleFireDuration 0.5, rumbleFireStrength 1, **damagePerSecond 110**,
**damageTicsPerSecond 5**, damageFlags [STATE_CAN_GO_TO_GRABBED, #bits800],
specialCaseDamageStuff (stun/slow/dot 0, heatIncrement 30, knockBackSpeed 15, knockBackAngle 35).

Default `DefaultFlameThrower` (83845) [data]: numProjectilesPerFire 1, rateOfFire 1, automatic true,
heatMeterCapacity 100, damagePerSecond 100.

## FlameThrower: code

- `FUN_0074b7a0` (0074b7a0) [game]: the attribute loader. Reads `flameThrowerAttribute` and
  `flameThrowerAIAttribute` (offset +0x28 of the weapon).
- `FUN_0074b540` (0074b540) [game]: the component loader (598 bytes). Reads `flameThrowerAttribute`,
  `DefaultFlameThrower`, `streamEffect` (+0x3f0), `impactEffect` (+0x3f4), `SSDImpactPreset`
  (+0x3f8), `SSDPeriodChecker` (+0x3fc). Makes `MeleeWeapon`-type calls (`FUN_01185ae0`, `FUN_01184300`)
  and particle template loads (`FUN_0057c4b0`), so it is not a plain field reader.
- `FUN_0074b2d0` (0074b2d0) [game]: scalar deleting destructor. Calls `FUN_0074b320`, then frees
  the block if bit 1 of the argument is set.
- `FUN_0074b320` (0074b320) [game]: destructor body. Sets vtable `00b5dae8`. It is NOT the gun class
  (`00b5fba0`), so the flame is a separate weapon class. Its constructor is not found yet.
- `FUN_0074ba20` (0074ba20) [game]: the generic weapon init, NOT flame-specific. Sets flag 0x400 at
  +0x340 (the note in `docs/weapon-mode.md` line 922 says it is every weapon's constructor, which
  matches). The census label `FlameThrower` on it is wrong. It also calls the `FlameThrowerDummy`
  helpers (`FUN_0074eec0`, `FUN_0117fe30`).
- The per-frame fire and damage routine for the flame is NOT found. Vtable `00b5dae8` is not in
  `export\vtables.tsv`; the slots must come from disassembly. The fields `damagePerSecond` and
  `damageTicsPerSecond` are the only evidence of a continuous damage model [data].

Flame vs GunWeapon, what the data says: no projectile (numProjectilesPerFire 1, timeBetweenEachBurst
0.01, automatic); damage is per second in 5 ticks (110 DPS longhaul, 100 default) [data]; heat 15 per
round, 33.3 per second decrease, 4 s overheat; range is only the `aimAssistMaxRange` of 12 [data].
The cone angle and the range that limits the damage are NOT found [unread].

## Which other packs use them

Counts are line counts of `grep -c` in the dumps (a type name also appears in list and object
headers, so these are not component counts). The per-character list is in
`docs/character-weapon-data.md`.

| dump | GrenadeLauncher lines | FlameThrower lines |
|---|---|---|
| `work\survey\weapons\longhaul.txt` | 3 | 10 |
| `work\survey\weapons\dcomscout.txt` | 4 | 0 |
| `work\survey\weapons\ironhide.txt` | 3 | 0 |
| `work\survey\weapons\ratchet.txt` | 2 | 0 |
| `export\data\starscream.txt` | 0 | 0 |

The other weapon dumps in `work\survey\weapons\` have none of the two names [data]. Only longhaul
uses FlameThrower [data].

## Not read

- The GrenadeProjectile per-frame flight and bounce (`FUN_007891e0`, outline only) and the spawn's
  launch arithmetic (`FUN_00786ff0`, outline only). The explosion blast radius is not found.
- The flame's constructor (vtable `00b5dae8` slots), its per-frame update, its fire routine, the
  damage-per-second tick, the cone and range. Not found in this ticket.
- The MP and AI flame and grenade attribute values beyond the listed ones; the Left variants
  (`LongHaul_FlameThrowerLeftAttribute` 91429, `LongHaul_GrenadeLauncherAttribute2` 91593).
- `ironhide`, `ratchet` and `dcomscout` grenade components were counted, not read.
- Vtable `00b5fba0` slot list for the grenade is taken from the gun/launcher note, not checked.
- The `DefaultGrenadeLauncher` cluster details (the cluster projectile's fields).

## GrenadeProjectile: flight, bounce, blast (2026-10-12)

Ticket: READ only. Scratch: `work\survey\weapons\fn_00789b80.txt`, `fn_007876b0.txt`, `fn_007e6af0.txt`.
Attribute offsets from `decomp_lookup.py fields 007bea00` (the table in the first part of this note).

Corrections to the section above: `FUN_00786ff0` is NOT the spawn. It is the per-frame update, vtable
`00b5ebe0` slot 2 (`pe_read.py vtable 00b5ebe0`) [game]. The launch is `FUN_00789b80`, called from the
spawn dispatcher `FUN_00784170` for kind 2 [game]. `FUN_007891e0` does not reflect the velocity; the
bounce is in the physics solver (question 3). Callers of `00786ff0` are none in the ledger: it is reached
only through the vtable.

Vtable `00b5ebe0` [game] (`pe_read.py vtable`): slot 0 `007868c0`, slot 2 `00786ff0` (update), slot 3
`00787650` (message dispatch), slot 4 `00786a60` (arm: lifetime and spin).

### 1. Launch: `FUN_00789b80` (called by `FUN_00784170` when the kind is 2), and `FUN_00786a60`

- Record copies [game] (`00789b80` lines 7-9): record +0xa4 = attr +0x130 (overwriteDirectDamage),
  +0xa8 = attr +0x134 (overwriteExplosiveDamage), +0xac = attr +0x138 (overwriteExplosiveMinDamage).
- Aim direction [game] (lines 16-49): if attr `fireTowardsTarget` (+0x164) is nonzero, d = normalize(aim
  target at record +0x4c..+0x54, minus the holder position at holder +0x34/+0x38/+0x3c). Otherwise d =
  normalize(holder basis triple at +0x1c, +0x20, +0x24) (line 22-32; the code's order is (x, y, z) = (+0x1c,
  +0x20, +0x24)) [assumed: this is the holder's forward basis, from the offsets only].
- Arc [game] (lines 50-87): if `launchArcAngle` (+0x158, already radians) != 0, axis = normalize(cross(d,
  ref)), ref = (0,0,1), or (1,0,0) when |d.z| > 0.999 (line 53-57). Then d = R(axis, angle) * d with R
  from `FUN_005708f0` (`cos`, `sin` of the angle, then `FUN_00570770`). Quoted disassembly (`raw_disasm`
  00789d9c-00789ef5): `movss xmm3, [edi+0x158]` (edi = attr), `add edi, 0x158`, then `call 0x5708f0` with
  `eax = edi`, so the angle is `*(float*)(attr+0x158)`. Sign of the rotation (up or down) is NOT checked
  [unread]: the axis is horizontal, so the direction tilts up or down by the angle.
- Speed [game] (lines 88-120): `tackOnOwnerSpeed` (+0x1a8) == 0: v = d * `launchSpeed` (+0x1a4). If 1: speed
  = launchSpeed + dot(owner body velocity, d) (`body_linear_velocity` `00556c00`).
- Velocity is set once: `physics_body_set_velocity` `00557000` (line 121). No hand integration.
- Lifetime [game] (`FUN_00786a60`, lines in its body): if not sticky (`isSticky` +0x188 zero, or +0x18c
  <= 0): `random_range_with_clock(+0x15c, +0x160)` (`grenadeMinLifetime`, `grenadeMaxLifetime`; generator
  `00551d60`, LCG `343fd / 269ec3` mixed with `time64`, see `docs/camera.md`), scheduled with
  `FUN_005a9800(..., 0x54732c8d)`. Message `54732c8d` is the hit (dispatch `00787650`, line 1-3).
- Initial spin [game] (`FUN_00786a60`): axis = spinAxis (+0x1ac, +0x1b0, +0x1b4) when `randomSpinAxis`
  (+0x1b8) is 0; else three `random_range(-1, 1)` values normalized (or (0,0,1) if zero). Rate:
  `random_range_with_clock(minSpinSpeed +0x1bc, maxSpinSpeed +0x1c0)` (radians, converted in the loader)
  into record +0x9c; sign flipped if `FUN_011904f0` (the same LCG, `DAT_00c02ef0`) returns < 0.
- Body set-up [game] (`00789b80` lines 126-142): body +0x214 = `bounciness` (+0x198), body +0x218 =
  `friction` (+0x19c). Gravity: if body +0x28 != 0, body +0x18..+0x20 = (0, 0, `Zgravity` (+0x168) * body
  +0x24); else zero. What body +0x18 is for the solver, and what body +0x24 / +0x28 are, is [unread].
- `oneOutAtATime`, `stickOrient`, `velocityOrient` (+0x1a9): read in update, see question 2.

### 2. Flight per frame: `FUN_00786ff0` (vtable slot 2)

- The flight is by the physics engine, as a rigid body with the velocity from question 1 [game]. The update
  does not integrate position: it reads the body velocity (`00556c00`) only when `velocityOrient` (+0x1a9)
  is set (look-at toward v*10; lines 'local_194 = v*10 + pos', `look_at_y_matrix` `0056ede0`), and otherwise
  spins the mesh: `FUN_005708f0`-style rotation of record +0x90 axis by `record +0x9c * dt` (line
  `local_1cc = +0x9c * param_2`) [game].
- `dt` is the update argument `param_2`. Its value is not checked here (CLAUDE.md says the game steps every
  0.032 s) [assumed].
- Timer: the update decrements record +0x64 by dt while it is > 0 (lines 'if (0 < +0x64) +0x64 -= param_2').
  This timer is the contact cooldown (question 3).
- `dampeningOnCollision` is applied PER CONTACT, not per frame: see question 3 (`00786ff0` only ticks it).

### 3. Bounce: `FUN_007876b0` (contact), not `FUN_007891e0`

- The reflection is not in the grenade code. The body's restitution is `bounciness` (+0x214, 0.55 in
  longhaul) and its friction is `friction` (+0x218, 0). The Lux contact solver does the bounce [game for
  the store; the solver itself is not read, CLAUDE.md "Lux contact solver" is still open]. The reflection
  formula is therefore NOT FOUND. [assumed] the standard restitution v' = v - (1 + e)(v.n)n on the normal
  component, with e = bounciness.
- Contact handler `FUN_007876b0` [game] (lines 86-116): the grenade acts on a contact only when attr
  `isProximity` (+0x178) is set. Then: if the other object is not a character (`local_144 == 0`) or
  `proximityActivateOnCollision` (+0x179) is set, it calls `FUN_007bb2c0` (not read) on the other object
  unless that object has flag 0x2000 (line 91); a nonzero `FUN_007bb2c0` means no hit. Every other path
  calls `GrenadeProjectile_hit` (lines 110, 114). So for a proximity grenade, a contact with a character
  always explodes it, and a contact with the world explodes it unless `FUN_007bb2c0` says otherwise [unread].
  The longhaul grenade has `isProximity` true, so its world bounce depends on `FUN_007bb2c0`. Not resolved.
- Armed-on-contact [game]: flag +0x59 is set on a contact when `proximityActivateOnCollision` (+0x179) is set (line 117-120).
- Dampening [game] (lines 125-147): when record +0x64 <= 0 and the grenade is not sticky: +0x64 = 0.25 s
  (0x3e800000), the contact normal is stored (+0x2c..+0x34), the body's velocity is multiplied by
  `dampeningOnCollision` (+0x1a0, 0.995) and written back (`00557000`), and the partner body's velocity
  (`FUN_00556ba0`) is multiplied by the same value (lines 139-146). So: one multiply per contact, at most
  every 0.25 s.
- Proximity range [game]: `007873f0` (per frame, via update line 1) compares the squared distance with
  `proximityRadius` (+0x17c, 10 in longhaul, 3 default) [assumed from the offset, as in the earlier section].
  `proximityExplosionMinDelay` (+0x180) and `MaxDelay` (+0x184) (0.02 to 0.04 in longhaul): their use is
  [unread].

### 4. Explosion: `FUN_007891e0` (hit), damage `FUN_007e6af0`

- Trigger [game]: (a) the lifetime timer (message `54732c8d`, dispatch `00787650` slot 3) calls the hit; (b)
  the contact (question 3) calls the hit; (c) the proximity test (`007873f0`) calls the hit.
- The hit [game] (`007891e0`): sets the contact frame from the normal (+0x2c..+0x34) and calls
  `thunk_FUN_01190d70` when the normal flag +0x28 is set; sends a damage message (`send_damage_message`
  `00724700`) with a record whose float at local+0x4c is 100000.0 (`0x47c35000`, line `local_4c = 0x47c35000`)
  and flags 0x10000000 [game]; if clusterSpawnables (attr +0x170) > 0 it spawns that many children with
  angles from `FUN_007898a0(count, maxClusterAngle +0x174)` [assumed for the spread]; then `DeleteAllProjectiles`
  `0078fa40` removes the grenade.
- Explosion damage [game] (`FUN_007e6af0`, grenade branch, kind 2 check at `00745ec0`, lines 85-98 of
  `fn_007e6af0.txt`): the explosion's damage = record +0xa8 (overwriteExplosiveDamage) if >= 0, and its
  minDamage = record +0xac (overwriteExplosiveMinDamage) if >= 0 (-1 keeps the default). Its flags =
  attr damageFlags (+0x13c) with `& 0xf27fffff` (line 98). Attr +0x140..+0x158 fill the explosion's own
  fields (lines 100-110; names not in the loader table, probably `specialCaseDamageStuff` [assumed]).
  The missile branch (kind 1, lines 119-123) reads record +0x88/+0x8c instead (`docs/weapon-mode.md` 850-870).
- Blast radius: NOT FOUND. No grenade attribute is a blast radius. The hit calls `FUN_005781b0(0x958a5dc0)` and
  then a vtable call (`+0x1c`), which may spawn the explosion object; the hash `958a5dc0` is not in
  `export\data\names.tsv` (`tf2_names.py lookup`), nor are `54732c8d`, `ddf4c407` (the 0.05 s arming
  timer message in arm/launch, `FUN_005a9800` with `0x3d4ccccd`) or `56ebd7c6` (6.0 s for sticky grenades).
  The explosion object the missile uses (radius 5, `docs/weapon-mode.md` 850-870) is a guess for the grenade [assumed].

### Port recipe (numbered)

1. On spawn, copy the attribute: direct damage = overwriteDirectDamage (0x130), explosive damage =
   overwriteExplosiveDamage (0x134), explosive min = overwriteExplosiveMinDamage (0x138). Damage flags = 0x13c.
2. Aim: if fireTowardsTarget (0x164) then d = normalize(target - muzzle) else d = normalize(holder forward).
3. Arc: if launchArcAngle (0x158, radians) != 0, axis = normalize(cross(d, up)) (up = (0,0,1); use (1,0,0)
   when |d.z| > 0.999), d = rotate(d, axis, launchArcAngle). Check the sign against the recording/emulator.
4. Speed: if tackOnOwnerSpeed (0x1a8) is 0, v = d * launchSpeed (0x1a4); else v = d * (launchSpeed + dot(ownerVel, d)).
5. Body: velocity v; restitution = bounciness (0x198); friction = friction (0x19c); gravity = (0, 0, Zgravity 0x168).
6. Lifetime (not sticky): T = uniform(grenadeMinLifetime 0x15c, grenadeMaxLifetime 0x160); call hit at T.
7. Spin: axis = spinAxis (0x1ac..0x1b4) unless randomSpinAxis (0x1b8), then a random unit axis; rate = uniform(minSpinSpeed 0x1bc, maxSpinSpeed 0x1c0) rad/s with a random sign; each frame rotate the mesh by rate * dt.
8. Per frame: cooldown = max(0, cooldown - dt). If isProximity (0x178) and the nearest target is within proximityRadius (0x17c), call hit (delays 0x180..0x184: [unread]).
9. On contact: if isProximity and the contact qualifies (step 10 notes), call hit. If cooldown <= 0: cooldown = 0.25 s; v *= dampeningOnCollision (0x1a0).
10. Hit: if clusterSpawnables (0x170) > 0, spawn children at angles spread within +-maxClusterAngle (0x174). Deal explosive damage = overwriteExplosiveDamage (0x134) with min overwriteExplosiveMinDamage (0x138) when >= 0, with flags 0x13c. Blast radius: STAND-IN (no source). Delete the grenade.

### Manager's addition (2026-10-12)

The blast radius IS found, in the data: the grenade object (`grenadeProjectileObj`) carries
`DamageLink` > `DetachLink` > an object with an `Explosion` (damage 25, start_scale 0.5,
explosion_time 0.1, knockBack 15 / 35) and a `Collision` sphere of radius 12: the same
nesting as a missile's, read by `scene::read_projectile_explosion`. Ported:
`core\src\weapons.rs` `Grenade`.

### Not found

- (found since, see above) The blast radius and the explosion object (hash `958a5dc0`, unnamed; `FUN_005781b0` lookup, vtable `+0x1c`).
- The bounce formula: the Lux solver's restitution (not read).
- `FUN_007bb2c0` (the proximity / world-contact test), `FUN_00787ea0` and `FUN_00789b10` (sticky branches), `FUN_005a9800` (the timer's semantics).
- The sign of the arc rotation (`FUN_00570770`), `FUN_011904f0` (its sign use), the meaning of body +0x18 / +0x24 / +0x28.
- The use of `proximityExplosionMinDelay` / `MaxDelay`, and the cluster child's fields (`DefaultGrenadeLauncher`).
- `dt` value (`param_2` of the update).
