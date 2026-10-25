# Jetfire's missile dump / carpet bomb and the Audi's mines (reader ticket, 2026-10-12)

Scope: the two special abilities of Jetfire (ground missile dump, car carpet bomb) and of the
Audi / Sideways (ground mine, car mine). Read-only. Scratch: `work\survey\flight\jetfire0.txt`,
`work\survey\flight\audir8_0.txt` (dumps of `characters\jetfire.str#0` and `characters\audir8.str#0`).
Nothing was run. Tags: `[game]` read in code (address given), `[data]` read in
`export\data\global_states.txt` or the pack dump, `[assumed]` a reading not settled (the reason and what
would settle it are given), `[unsettled]` not found or not read.

Reuse: the grenade class (`GrenadeLauncher`, `GrenadeProjectile`) is already ported in
`core\src\weapons.rs` `Grenade` (the launch, arc, speed, lifetime, bounce, proximity, blast; see
`docs/weapon-kinds.md` sections "GrenadeProjectile: flight, bounce, blast" and "Port recipe").
The special start, overlay and cooldown are ported (`docs/weapon-mode.md` "The special ability",
`sim::start_special`). Missiles (`MissileLauncher`) are a different class and are not used here.

## 0. What is common [game]

- Special start: `FUN_0072afd0` (ledger `character_special_type_switch`). Type -1 (both Jetfire
  and the Audi) has no case in its switch, so nothing in that switch fires these specials
  (`docs/character-specials.md` table). The weapon is fired by the weapon-name path below.
- `FUN_0072afd0` calls the manager `FUN_007b3d80` (`special_weapon_fire_select`, read this ticket):
  1. character object lookup (`FUN_0070a3c0`); needs character `+0x140` (the damage object) non-zero
     and `scripted_health_controller` (`FUN_00711260`) zero.
  2. Form test: character `+0x31c` == 3 means the car. Car: `+0xe8` is the weapon name hash;
     ground: `+0xd4`. (A first test reads `+0xe4` in the car or `+0xd0` on foot, when the param is 0;
     if that switch is on the function returns without firing [game, meaning of the param: unsettled].)
  3. If the name hash is non-zero, `FUN_007b3c40(manager, hash)` fires that weapon [game; `FUN_007b3c40`
     itself NOT read: that it fires immediately, once, and how it finds the weapon, is [unsettled]].
- Projectile creation: the weapon's slot `+0x80` (`FUN_007aaeb0`, `weapon_spawn_projectile`) ->
  `FUN_00784170` (`weapon_spawn_dispatch`), kind 2 = grenade: `FUN_0078adc0` then `FUN_00789b80`
  (`GrenadeProjectile_launch`, ledger). [game]. `FUN_0078adc0` NOT read (it is the grenade's
  creation; `FUN_01184930` is the factory per `docs/weapon-kinds.md`).
- All four weapons are GrenadeLaunchers: each special's `#object: GrenadeLauncher` sits in its
  `jetfire_special_weapon` / `jetfire_vehicle_special_weapon` / `audir8_specialmine` object
  (dump lines 651504, 655417). [data]
- Weapon fire, one press: `weapon_fire` `FUN_007ae010` (per the note in `docs/weapon-mode.md`
  "The firing side"): for each of `numProjectilesPerFire` it takes the aim point (`FUN_007aa210`), makes
  a projectile (`+0x80`), then `FUN_00784170` / `FUN_00784080`. Ready = `rateOfFire` seconds since the
  last round (`FUN_007ad080`). Burst: `FUN_007ac370` after `timeBetweenEachBurst` if `burstPerFire` > 1
  (`docs/weapon-mode.md` line 339). [game]
- Muzzle: `FUN_007a7a40` (`weapon_muzzle_transforms`, read): `useFireDummyMatrix` false and
  `useOwnerRootMatrix` false means a frame built from the owner (weapon mode: the aim vector at owner
  `+0x260`; else the character or node `0xc8ec021e`). `fireDummyXYAngle` and `fireDummyZAngle` turn each
  barrel's frame [game, from `docs/weapon-mode.md` 356-359]. How the 8 angles are applied to the
  direction of each missile is [unsettled] (see step A4).

## A. Jetfire: missile dump (ground form)

Data [data] `JetFire_MissileDumpAttribute` (`global_states.txt` 90747), weapon `JetFireMissileDump`
(`ced31e93`, `jetfire_special_weapon`, dump line 651378), projectile `jetfiremissiledump` (embedded,
11 roots, dump line 651548). The AI twin `JetFire_AI_MissileDumpAttribute` (90354) has the same first
eight fields. MP twin `MP_JetFire_MissileDumpAttribute` (97155) not read.

Attribute values:

| field | value | note |
|---|---|---|
| numProjectilesPerFire | 8 | one press = 8 missiles |
| rateOfFire | 0.3 s | ready interval |
| automaticFiring | false | one press, one fire |
| burstPerFire / timeBetweenEachBurst | 1 / 0.01 | no bursts |
| fireDummyXYAngle | 0, 45, 90, 135, 180, -45, -90, -134 deg | one per projectile |
| fireDummyZAngle | -40 deg, all 8 | |
| useFireDummyMatrix / useOwnerRootMatrix | false / false | owner-frame muzzle |
| useFacingAsTarget | true | |
| isSpecial | true | meaning in code: [unsettled] |
| launchSpeed | 40 (m/s) | |
| launchArcAngle | 3 deg (the note's field is stored in radians) | |
| tackOnOwnerSpeed | false | speed = launchSpeed, no owner speed added |
| fireTowardsTarget | false | direction from the muzzle frame |
| grenadeMinLifetime / MaxLifetime | 3 s / 3 s | fuse |
| Zgravity | -32 | |
| oneOutAtATime | false | |
| isProximity | true | proximity arming |
| proximityActivateOnCollision | false | |
| proximityRadius | 10 | |
| proximityExplosionMin/MaxDelay | 0.05 / 0.05 s | use [unsettled] |
| isSticky | false | |
| bounciness / friction / dampeningOnCollision | 0.55 / 0.8 / 0.8 | |
| overwriteDirectDamage | 0 | |
| overwriteExplosiveDamage / overwriteExplosiveMinDamage | 30 / 30 | |
| specialCaseDamageStuff | stun 0, slow 0, dot 0, knockBackSpeed 15, knockBackAngle 35 | |
| damageFlags | [#bitsa00] | |
| randomSpinAxis / min,maxSpinSpeed | true / 0, 0 | no spin |
| velocityOrient | false | |

Projectile object [data] (`jetfiremissiledump`): `Destructable` (auto_delete_time 2, not targetable,
damage_filter list at dump 651602), `Collision` mass 5 with a cylinder of radius 0.468 (dump 655 area
of lines 451-500 of the embedded block), `DamageLink` > `DetachLink` objects: `detach` (a sound and
a timed `Light` range 8 for 1 s), `detach2` (`Explosion` damage 8, min radius fall-off 0.75,
minDamage 1, explosion_time 0.1, knockBackSpeed 30), `detach3` (`Explosion` damage 20). Overwrites
replace the explosion damage with 30 (`FUN_007e6af0`). The smoke and fire particles are
`w_smk_missile_dump`, `w_exp_homing_missiles_water` [data].

Steps (in order):

- A1. [game] Press (the fire trigger; not automatic): `FUN_007ae010` runs the ready test
  (`FUN_007ad080`, rate 0.3 s) and then the loop over 8 projectiles.
- A2. [game] For each projectile `i`: `FUN_007aa210` gives the aim point (scatter for a human player
  with `playerNoiseRadius` > 0; here 0, so none [game, notes]); `FUN_007aaeb0` builds the projectile and
  `FUN_00784170` (kind 2) calls `FUN_0078adc0` (creation, not read) and `FUN_00789b80` (launch).
- A3. [game] Launch (`FUN_00789b80`, the port recipe in `docs/weapon-kinds.md`): record +0xa4/+0xa8/+0xac
  = overwriteDirectDamage 0, overwriteExplosiveDamage 30, overwriteExplosiveMinDamage 30. Direction =
  holder basis (`fireTowardsTarget` false).
- A4. [game] + [assumed] Direction per projectile: the 8 XY/Z angle pairs are the barrel turns of the
  owner-frame muzzle (`FUN_007a7a40`). ASSUMED: projectile `i` takes its direction from the holder basis
  turned by (XY_i, Z_i) = (fireDummyXYAngle[i], -40 deg). This gives a ring of 8 missiles, 40 deg below
  the nose, spread around the axis (0 deg forward, 180 deg behind, 90 deg to the side). What would settle
  it: the disassembly of `FUN_007a7a40` for the `fireDummyXYAngle` loads, or a recording of the dump.
- A5. [game] Arc: `launchArcAngle` 3 deg tilts each direction about the horizontal axis (sign not checked,
  see `docs/weapon-kinds.md` 232).
- A6. [game] Speed: v = d * 40 m/s, velocity set once (`physics_body_set_velocity`, `00557000`).
- A7. [game] Body: bounciness 0.55 and friction 0.8 on the body; gravity Zgravity -32 applied through the
  body (`00789b80` lines 126-142).
- A8. [game] Lifetime: not sticky, so `FUN_00786a60` draws the fuse from `random_range(3, 3)` = 3 s,
  scheduled as message `54732c8d` (hit).
- A9. [game] Per frame (`FUN_00786ff0`): proximity test `FUN_007873f0` (armed only when +0x58, +0x59 set and
  +0x5a clear). `isProximity` is true, and `proximityActivateOnCollision` is false, so the grenade is armed
  at once [assumed: the arming at launch is not read; the notes say +0x59 is set on contact only when
  activateOnCollision is set, so +0x58 may be the launch arm].
- A10. [game] Contact (`FUN_007876b0`): a contact with a character always explodes it; a world contact
  explodes unless `FUN_007bb2c0` says no [unsettled].
- A11. [game] Hit (`FUN_007891e0`): sends the damage message (`send_damage_message` `00724700`) with the
  record's explosive damage 30 and min 30 (flags `[#bitsa00]`); no cluster (`clusterSpawnables` 0 from
  the default; the Jetfire missile's own value: not in the dump, [unsettled]); deletes the grenade
  (`DeleteAllProjectiles` `0078fa40`).
- A12. Blast radius: [unsettled]. The embedded object's `Explosion` (detach2 damage 8, detach3 damage 20)
  is the overwritten explosion; its sphere is not read.

Totals [data + game]: 8 projectiles per press, 0.3 s ready time, 30 explosive damage each (min 30, so no
fall-off), 3 s fuse, 40 m/s launch.

## B. Jetfire: carpet bomb (car form)

Data [data] `JetFire_CarpetBombAttribute` (`global_states.txt` 90459), weapon `JetFireCarpetBomb`
(`d9c853ff`, `jetfire_vehicle_special_weapon`, dump line 655417), projectile is a GrenadeLauncher too.
Trigger: character table `VehicleWeaponFireName` `JetFireCarpetBomb`, not from animation [data].

| field | value | note |
|---|---|---|
| numProjectilesPerFire | 1 | |
| rateOfFire | 4 | ready interval 4 s? Not the bomb interval: see below |
| automaticFiring | false | |
| burstPerFire / timeBetweenEachBurst | 8 / 0.5 s | 8 bombs, 0.5 s apart |
| isVehicleModeWeapon | true | |
| isSpecial | false | |
| fireDummyXYAngle / ZAngle | 90 x8 / 0 x8 | useFireDummyMatrix false, so probably unused |
| launchSpeed | 0 | drop, no throw |
| launchArcAngle | 0 | |
| grenadeMin/MaxLifetime | 3 s / 3 s | |
| Zgravity | -32 | |
| isProximity / proximityActivateOnCollision | true / false | |
| proximityRadius | 10 | |
| proximityExplosionMin/MaxDelay | 0.05 / 0.05 s | |
| isSticky / stickyMin / stickyMax | true / 0.1 / 0.1 s | |
| overwriteExplosiveDamage / overwriteExplosiveMinDamage | 30 / 25 | |
| knockBackSpeed / knockBackAngle | 15 / 35 | |

Steps:

- B1. [game] Start: the car weapon fires at the special start (`FUN_007b3d80`, car branch `+0xe8`), not
  from an animation event (`VehicleWeaponFireFromAnim` false in the character table).
- B2. [assumed] Burst: `burstPerFire` 8 with `timeBetweenEachBurst` 0.5 s gives 8 drops, one per 0.5 s,
  each a single `GrenadeLauncher` round (`FUN_007ac370` burst counter, `docs/weapon-mode.md` line 339).
  The `rateOfFire` 4 is the ready interval of the burst's first round, and whether the burst is held to
  the trigger or runs by itself is [unsettled]. What would settle it: the release path `FUN_007acb90`
  (the "launcher" clause) and `FUN_007ac370`'s burst branch.
- B3. [game] Each drop: launch `FUN_00789b80` with launchSpeed 0, so v = 0 and gravity -32 m/s^2 takes it
  down. Direction is the holder basis (`fireTowardsTarget` false), unused when speed is 0.
- B4. [game] Sticky: `isSticky` true with `stickyMinLifetime` 0.1 > 0, so `FUN_00786a60` takes the sticky
  branch (its timer is the 0.1 s; the message and the stick are `FUN_00787ea0` `sticky_attach`, read this
  ticket: on contact it zeroes the body velocity and the gravity term, and parks the body at the contact).
- B5. [assumed] The 0.1 s is the time from landing to the blast (a fuse after sticking). What would settle
  it: `FUN_00786a60` lines for the sticky branch.
- B6. [game] Hit `FUN_007891e0`: damage 30 (min 25), then delete.
- B7. [unsettled] Blast radius, the number of bombs left alive at once, and whether proximity arms it
  (`isProximity` true, activate-on-collision false; the same rule as A9).

## C. Audi (Sideways): ground mine

Data [data] `AudiR8_SpecialMine` (`global_states.txt` 79320), weapon `AudiR8SpecialMine` (`11151216`),
projectile `audir8_specialmine` (embedded, 3 roots, `characters\audir8.str#0`, dump line 581227). Character
table: `Weapon` not from animation; Cooldown 17.5, Duration -1, no self trigger, Icon #3.

| field | value | note |
|---|---|---|
| numProjectilesPerFire | 1 | |
| rateOfFire / automaticFiring | 0.01 / true | the trigger may lay mines fast; oneOutAtATime limits |
| oneOutAtATime | true | only one mine out at a time: where enforced is [unsettled] |
| isSpecial | true | |
| launchSpeed | 65 | |
| launchArcAngle | 0 | |
| grenadeMin/MaxLifetime | 60 / 60 s | the fuse, see C4 |
| Zgravity | -32 | |
| isProximity | true | |
| proximityActivateOnCollision | true | arms on first contact |
| proximityRadius | 15 | |
| proximityExplosionMin/MaxDelay | 0 / 0 s | |
| isSticky | true, stickOrient true | |
| stickyMin / stickyMax | -1 / -1 | |
| stickyOffsetFromCollision | 0.5 | |
| bounciness / friction / dampeningOnCollision | 0.6 / 1 / 0.995 | |
| velocityOrient | true | nose follows velocity |
| randomSpinAxis / spin | true / 180 to 360 deg/s | |
| overwriteDirectDamage / overwriteExplosiveDamage / overwriteExplosiveMinDamage | -1 / 30 / -1 | -1 keeps the object's own explosion |
| specialCaseDamageStuff | slowTime 8, stunTime 0, knockBackSpeed 15, knockBackAngle 35 | an 8 s slow |
| damageFlags | [#bits4a00] | |

Projectile object [data]: `Destructable` (auto_delete_time 2, `FadeTrigger` time 1), `Collision`
sphere radius 0.3, `Explosion` (start_scale 0.8, damage 5, min radius fall-off 0.75, minDamage 1,
explosion_time 0.01, slowTime 8) with a `Collision` sphere of radius 16 (the blast's sphere, [assumed]),
and a `Light` range 10 for 1.5 s. `Explosion` damage 5 is replaced by 30 only where the overwrite is
>= 0; here the explosive overwrite is 30, so the blast is 30.

Steps:

- C1. [game] Start: the ground weapon fires at once (no animation), `FUN_007b3d80` ground branch `+0xd4`.
- C2. [game] Launch (`FUN_00789b80`): v = d * 65 m/s (`tackOnOwnerSpeed` false, `fireTowardsTarget`
  false: direction = holder basis, so the mine is thrown forward); gravity -32; bounciness 0.6, friction 1.
- C3. [game] Spin: axis random, rate uniform(180, 360) deg/s with random sign (`FUN_00786a60`, `random_range`).
- C4. [assumed] Fuse: `isSticky` true with stickyMinLifetime -1 <= 0, so `FUN_00786a60` takes its
  "not sticky" branch, the random lifetime `random_range(60, 60)` = 60 s, message `54732c8d` (hit). The
  condition is quoted from `docs/weapon-kinds.md` 236-237 (`+0x18c <= 0`); the claim that +0x18c is
  stickyMinLifetime is [assumed] (the offset order). What would settle it: `FUN_00786a60` disassembly.
- C5. [game] Contact `FUN_007876b0`: `isProximity` true, so a contact with a character explodes it; with
  `proximityActivateOnCollision` true, the first contact arms it (+0x59 set, lines 117-120).
- C6. [game] Stick `FUN_00787ea0` (read this ticket): on contact the body velocity is zeroed (`00557000`,
  lines 91-95) and the gravity term (+0x18..+0x20) is zeroed (lines 96-110). The contact frame is built
  from the normal (`sqrt`). The `stickOrient` and `stickyOffsetFromCollision` (0.5) use is [unsettled]
  (not in the read lines 1-110; the rest of the function is 329 lines).
- C7. [game] Proximity: `FUN_007873f0`, per frame, armed and not already exploding; the nearest target within
  `proximityRadius` 15 detonates it (delay 0). The target set (characters only, or all?) and the test
  (squared distance, `FUN_00562970`) are [assumed]: `docs/weapon-kinds.md` 92-96.
- C8. [game] Hit `FUN_007891e0`: damage message (explosive 30, flags `[#bits4a00]`), then the blast object
  (Explosion 5 dmg as the object, overwritten to 30 for the damage, slowTime 8 for 8 s), delete.
- C9. [unsettled] `oneOutAtATime` enforcement (where the weapon refuses a second mine while one is out).

## D. Audi (Sideways): car mine

Data [data] `AudiR8_VehicleSpecialMine` (`global_states.txt` 79508). Character table: car weapon
`AudiR8VehicleSpecialMine` (`fcb068be`), VehicleWeaponFireFromAnim true (the Audi's car special is started
by the animation `Special`), car anim `Special`.

Differences from C [data]: `launchSpeed` 1 (the mine is dropped, not thrown), `isVehicleModeWeapon` false,
`listenToWeaponAnimEvent` false, `snipeCameraEnabled` false, `overwriteExplosiveDamage` 30,
`overwriteExplosiveMinDamage` -1, `slowTime` 8, `grenadeMin/Max` 60 s, `isProximity` true, `proximityRadius`
15, `isSticky` true, `stickyMin/Max` -1. Not read: the rest of the block (oneOutAtATime, bounciness, spin).

Steps:

- D1. [data] The fire time is the animation event named by the character (`WeaponFireFromAnim`), so
  the fire comes at the animation's point. [assumed] How the animation event reaches the weapon:
  `listenToWeaponAnimEvent` is false here, so the event is probably the character's `WeaponFireFromAnim`
  path (`docs/weapon-mode.md` "weapon layer"). [unsettled]
- D2. The rest as C2 to C8, with v = d * 1 m/s.

## C2. Sticky (second read, 2026-10-12, Haiku ticket)

Read: `FUN_00786a60` (whole, 88 cleaned lines), `FUN_00787ea0` lines 110-260, `FUN_007873f0` lines 1-60.
Ported in `core\src\weapons.rs` `Grenade` (`is_sticky`, `arms_on_collision`, `sticky_offset` in
`GrenadeTuning`); tests `a_sticky_mine_*`, `a_stuck_mine_still_goes_off_on_its_fuse`.

1. [game 00786a60 l.30-40] Launch fuse. The random lifetime (`random_range(+0x15c, +0x160)`, the grenade's
   grenadeMin/MaxLifetime) is scheduled as the hit timer (message 54732c8d) only when the weapon is not
   sticky (`+0x188` = 0) or its `+0x18c` (stickyMinLifetime) is <= 0. The Audi mine has stickyMin -1, so it
   gets the fuse: 60 s from the THROW, not from the stick (the stick does not reset it). This settles C4.
   The carpet bomb (stickyMin 0.1) gets NO launch fuse here; its sticky timer is not in this function.
2. [game 00786a60 l.41-44] Arming at launch: `+0x59` (armed) is set at once unless `+0x179` is set. The
   data's `proximityActivateOnCollision` (true for the Audi) is taken as `+0x179` [assumed, offset order].
   So the Audi's mine is NOT armed in the air; the contact arms it (C5).
3. [game 00787ea0 l.146-200, 209, 222-224] Contact on a sticky mine: the contact point plus the contact normal
   times the weapon's `+0x194` is the rest place; the velocity is zeroed (`00557000`) and the gravity term
   zeroed. The 0.18 test (l.146-151) and the character branch (l.201-209: when a character object exists,
   `GrenadeProjectile_hit` is called) are the rest of the contact logic; the port sticks on every world touch
   and leaves a character touch to the proximity test (see 5). [unsettled] the 0.18 test's meaning.
4. [game 00786a60 l.6-28] `oneOutAtATime` (assumed = `+0x16c`): when a mine arms, the other live mines of the
   same weapon are expired by `sticky_expire_fadeout` (`00789b10`), which fades and clears the proximity flag
   and does NOT explode. Not yet ported (the port has one mine per launcher limit in `Shot.one_out_at_a_time`
   only in special_weapon.rs). [assumed] that `+0x16c` is that flag.
5. [game 007873f0 l.1-60] Proximity test, per frame: needs `+0x58` (proximity on), owner `+0x178`, armed `+0x59`,
   not exploding `+0x5a`. Targets: objects with bit 10 of `+0x14c` set and bit 3 of `+0x148` clear, and not the
   thrower's own object; if the thrower's avatar exists, a further test `FUN_0117e6d0` decides (UNREAD: this is
   the team / hostility test, so "a teammate does not" is [unsettled]). Squared distance <= radius (`+0x17c`
   squared). On a hit: `+0x5a` set, then the hit at once when `+0x180` (proximityExplosionMinDelay) <= 0 (the Audi:
   0), else a timer.
6. [game 00786a60 l.45-79] Spin: axis from data or random, rate random_range(`+0x1bc`, `+0x1c0`) with a random
   sign (`011904f0`). Not ported (visual only; the port draws no model yet, see status "Phase 2, wave 3").

Port summary: a sticky grenade stops at the first world touch, offset 0.5 along the normal, armed by that
touch when `proximityActivateOnCollision`; the proximity test is gated on armed; the launch fuse always runs.
Not ported: the 0.18 contact test, the sticky timer for stickyMin > 0 (the carpet bomb: launch fuse kept as a
stand-in), `oneOutAtATime` expiry, the team test (`FUN_0117e6d0` is unread).

## E. Not found (do not invent)

- `FUN_007b3c40` (the fire-by-name routine behind `FUN_007b3d80`): not read. It decides how the named
  weapon is fired from the special (immediate or by the state).
- `FUN_0078adc0` (grenade creation in the kind-2 spawn): not read.
- Blast radius of each grenade blast (Jetfire missile, carpet bomb, Audi mine): the sphere
  collision in the Audi's embedded object (radius 16) is the likely blast, not confirmed; the Jetfire
  blast sphere is not read.
- `FUN_007bb2c0` (world-contact test that decides whether a world contact explodes a proximity grenade).
- `FUN_007873f0` target set for the proximity test (characters or any object).
- `FUN_00787ea0` lines 111-329 (`stickOrient`, offset 0.5, the sticky attach onto a body, `DestructableAttach`
  `00746000`).
- `FUN_00786a60` sticky branch (the 0.1 s and -1 handling; which field is +0x18c).
- The sign of the arc rotation (`FUN_00570770`), and the use of `proximityExplosionMinDelay` / `MaxDelay`.
- Where `oneOutAtATime` is enforced.
- Where `isSpecial` changes behaviour (no reader found; `fn.py known` gives none).
- Jetfire carpet bomb: whether the 8 bombs are a held burst or self-running (`FUN_007ac370` burst branch,
  `FUN_007acb90` launcher clause).
- The meaning of `useFacingAsTarget`.
- The Audi car mine's animation event path (`WeaponFireFromAnim` in the character table vs
  `listenToWeaponAnimEvent` false in the weapon).
- The MP and AI variants beyond the first eight fields.
- `characters\jetfire.str` and `audir8.str` have chunks other than `#0`; only `#0` was dumped.

## F. Ledger

Recorded this ticket: `sticky_attach` (`00787ea0`), `sticky_expire_fadeout` (`00789b10`),
`special_weapon_fire_select` (`007b3d80`).

Corrections to the notes: `docs/weapon-kinds.md` line 88 calls `FUN_00786ff0` the spawn; it is the update
(already corrected at line 210). the unpublished notes 158 says Sideways' special is a SLOW mine:
this agrees with `slowTime` 8 in both mine blocks [data].

## fireDummy angles (read)

Question: which way do `fireDummyXYAngle` and `fireDummyZAngle` turn a launch direction? Result: PARTLY
SETTLED. The loading and the per-barrel rotation are read; the final world direction of the launch
is not closed (the launch reads the holder frame, step 5). The `[assumed]` z-negation in the port is NOT
confirmed by this read.

1. [game] Name hashes: `fireDummyXYAngle` 0x10199156, `fireDummyZAngle` 0xbc71e419 (CRC-32 of the name).
   Used in the weapon attribute loader `FUN_007bbc30` (`weapon_attr_load_fire_dummy`, addresses
   007bc6f2 and 007bc78c). `tf2_names.py lookup` failed on `export\data\names.tsv` (parse error); the
   hashes were computed directly.
2. [game] Load: each field is a list. Both loops read `view_count` entries, capped at 15 (`if 0xe < i`
   break), and store into the attribute block: XY at `attr+0xa4+4*i`, Z at `attr+0xe0+4*i`. Every value
   is multiplied by `0x3c8efa35` = 0.0174533 (pi/180) at 007bc71a (`movss xmm1,[0xb6a40c]`). So the data
   is in DEGREES and the code holds RADIANS. No negation at load [game]. The data's -40 is stored as
   -0.698 rad. Missing field: value stays 0.
3. [game] Use, per barrel `i` (`FUN_007a7a40`, `weapon_muzzle_transforms`, the loop at 007a8298 to
   007a8690): `Z_i = [attr+0xe0+4i]`, `XY_i = [attr+0xa4+4i]` (007a8520 `ecx+edx-0x3c`). If both are 0 the
   barrel is left alone (007a851a-007a853b jump to 007a8690). Otherwise:
   a. [game] `R = Rodrigues(V, Z_i)` through `FUN_00570770` (`axis_angle_rotation_3x3`): `cos Z_i` into the
      `param_2` slot, `sin Z_i` into the `param_4` slot (007a85a3 to 007a85bb). V is the first basis row
      of the barrel's frame (copied from `[esp+0xb0..0xb8]` = frame `+0,+4,+8`, 007a855a-007a857e).
      Matrix (as stored): row0 = (s+v1^2(1-s), v1v2(1-s)+v3 t, v1v3(1-s)-v2 t), with s = cos Z, t = sin Z.
      This is a rotation about V by Z (sign of t as read; see the open point below).
   b. [game] `B` = a rotation about the THIRD matrix axis by XY_i: `B[2][2] = 1` (`1.0` at 007a8663),
      `B[0][2] = B[1][2] = B[2][0] = B[2][1] = 0`, and the 2x2 block holds cos XY and sin XY (007a8606-007a8631).
      `B` is premultiplied: `result = B * R` (`matrix3_premultiply` 0052eb40, EAX = A, ECX = B; in place
      into A).
   c. [game] The result goes through `FUN_00912b80` (`compose_affine`) into the barrel frame at
      `weapon+0x54+0x30*i` (007a8671-007a8688).
4. [game] So the direction is the frame `B * R(V, Z_i)`, where V is the barrel's own first basis row. Z
   tilts about V; XY turns about the third axis. Whether that is "down" or "up" depends on which row of
   the frame the launch reads (step 5) and on the sign of `t` in the Rodrigues matrix.
5. [game] Launch: `GrenadeProjectile_launch` (`00789b80`, ledger line) takes the holder basis as
   `dir` from `holder+0x1c, +0x20, +0x24` (00789b80 lines 17-23) when `fireTowardsTarget` is false. The
   holder is `param_1+0xc`, which is NOT the weapon object that `FUN_007a7a40` writes (`weapon+0x54`).
   The link between the barrel frame at `weapon+0x54+0x30i` and the holder at `+0x1c` was NOT read
   [unread]. The note `docs/special-weapons.md` 137 found the same gap for the turret. So the
   single-vector answer is open.
6. [game] Spread: each of the 8 projectiles has its own `i` (the loop in `FUN_007ae010`, step A2), so the
   8 barrels get their own XY_i and the same Z (-40 deg). The XY list is per index, NOT interpolated
   (no interpolation in the loader or in the muzzle loop) [game]. The data's 0, 45, 90, 135, 180, -45,
   -90, -134 degree list is used as given.
7. [assumed] Given step 3, the result for a barrel: frame = B(XY) * R(V, Z) with V the forward row.
   Z = -40 with the sign of `t` as read gives a tilt of 40 deg about V, and XY = -134 turns the barrel
   about the third axis; the left / right and up / down answers depend on steps 4-5. NOT settled. The
   status item "negate z" stays an assumption until the holder read (step 5) is done.

Open for the next reader (in order): (a) the holder struct behind `param_1+0xc` at the launch and what
`weapon+0x54+0x30i` feeds into (`FUN_00784170` / `FUN_0078adc0`); (b) the sign of the `t` term in
`FUN_00570770` (check the `fchs` at 007a85a0-007a85e0 against the `-0.0` constant at `[0xb6a3ac]`,
which the disasm shows is used on the second `sin`); (c) a recording of the Jetfire dump, which
would settle the direction without the disassembly.

## fireDummy angle signs (emulated)

Question: does `fireDummyZAngle = -40` tilt a launch direction UP or DOWN, and does `fireDummyXYAngle = -134`
swing it LEFT or RIGHT? Script: `work\emu\fire_angle_signs.py` (emulates FUN_00570770 and
FUN_0052eb40 in `emu.py`; output reproduced below). Forward is the row (0,1,0); x right, y forward, z up.

Calling convention of FUN_00570770 [game] (disasm 00570770..00570790, call site 007a85b4..007a85bb): ECX = axis
pointer (the barrel frame's first row, copied to the stack at 007a8554..007a8575), EDX = pointer to cos, stack
arg 1 = output pointer (9 floats), stack arg 2 = pointer to sin. Returns no value.

Matrix product FUN_0052eb40 [game]: with EAX = A and ECX = B it stores **A := A*B** (standard row-major product:
`in_EAX[1] = B01*A00 + B11*A01 + B21*A02`, checked by the emulated run against Python). The call at 007a866c has
EAX = R (esp+0x16c) and ECX = B (esp+0xdc), so the STORED frame matrix is **R*B**, not B*R as step 7 above has it.

B for XY is inline (007a85ed..007a8663), not a function. Transcribed [game]: stores at esp+0xdc = cos,
+0xe0 = sin, +0xe8 = -sin, +0xec = cos, +0xfc = 1, the rest 0. So B = [[c, s, 0], [-s, c, 0], [0, 0, 1]].

Printed (cos and sin from Python math, rounded to float32; [assumed] see below):
- R(axis x, Z = -40 deg = -0.698132 rad): cos 0.766044, sin -0.642788
  rows [1 0 0] [0 0.766044 -0.642788] [0 0.642788 0.766044]. forward * R = (0, 0.766, **-0.643**).
- R at +40 deg (mirror): forward * R = (0, 0.766, +0.643). Mirrors as expected.
- B(XY = -134 deg = -2.338741 rad): cos -0.694658, sin -0.719340. forward * B = (**+0.719**, -0.695, 0).
- Stored R*B (emulated 0052eb40, matches Python): rows [-0.694658 -0.719340 0] [0.551046 -0.532139 -0.642788]
  [0.462383 -0.446518 0.766044]. forward * (R*B) = (**+0.551**, -0.532, **-0.643**).
- For comparison, B*R (not what the game stores): forward * (B*R) = (+0.719, -0.532, +0.447).

Conclusion [game, emulated]: with x right, the Z = -40 term points the forward row DOWN (z = sin Z < 0, and
z of forward * (R*B) is sin Z whatever XY is); the XY = -134 term swings it RIGHT (x = -sin XY > 0 alone, and
x = cos Z * (-sin XY) > 0 in the composed frame).

Caveats:
- [assumed] The barrel frame's first row is the model's right axis and the frame is x right, y forward, z up.
  Not read in this ticket. Launch (00789b80) and FUN_00912b80 (which composes the stored matrix with the frame
  at weapon+0x54+0x30*i) are NOT read: the answer is about the matrix, not yet about the bullet's path.
- [assumed] cos and sin were taken from Python math, not run in the game's code: `_CIcos` / `_CIsin` are imports
  of MSVCR80.dll and `emu.py` stops on them (no stand-in). Only signs are used; a last-bit difference cannot flip them.
- [assumed] UNITS, not settled. The export (`export\data\global_states.txt` lines 90397-90399 and 76440-76442)
  holds the raw values -40 and -134 with the designer's unit, and the code at 007a8547 / 007a85ca feeds the stored
  field straight to sin and cos (no degree-to-radian step in this range). The analysis above takes the stored
  field as radians, i.e. the loader converted it (as the other angle fields in `bumblebee-mechanics.md` line 95).
  If the field is stored raw, the single-rotation answers do not change (sin(-40 rad) < 0, -sin(-134 rad) > 0),
  but the composed x flips: cos(-40 rad) = -0.667, -sin(-134 rad) = +0.886, so x = -0.59 (LEFT). Settled by: the
  loader that writes fireDummyXYAngle / fireDummyZAngle (degree-to-radian multiply, or not), which is not read here.
- The forward row is taken as v*M (row vectors). If the game applies M as columns, every x and z here flips.

## Barrel frames (read)

Question: what is the BASE frame that `fireDummyXYAngle` / `fireDummyZAngle` turn? Result: the base is the
CHARACTER's frame, not a dummy node's orientation. The node gives only the barrel's PLACE. The sanity check
FAILED (all eight directions point 40 deg below the horizon), so the port was NOT changed. Stopped here.

1. [data] Jetfire's `JetFire_MissileDumpAttribute` block (`global_states.txt` ~90380-90400): `useFireDummyMatrix`
   false, `useOwnerRootMatrix` false, `fireDummyXYAngle` [0, 45, 90, 135, 180, -45, -90, -134],
   `fireDummyZAngle` [-40 x8]. Flag byte is `attr+0xa1` = useFireDummyMatrix (`docs/weapon-mode.md` 310).
2. [game 007a7a40] The loop at 007a8298 matches each barrel i to a node of the owner's node list (`param_1[3]+0x74`)
   by name hash. The hash table is at 0xc0675c + 4i (007a82b5 `mov ecx,0xc0675c`; compare at 007a82d5...). So
   barrel i = node `FireDummy` / `FireDummy01..09` (names.tsv: FireDummy01 a82b3c5a, 02 31226de0, 03 46255d76,
   04 d841c8d5, 05 af46f843, 06 364fa9f9, 07 4148996f, 08 d1f784fe, 09 a6f0b468; bare `FireDummy` not checked).
   Its world translation goes to the slot's position (+0x24..+0x2c of the 0x30-byte slot at weapon+0x54+0x30i).
3. [game 007a7a75, 007a8298-007a8330] Since `attr+0xa1` = 0 for Jetfire, the ROTATION rows of the slot are the
   base frame built in the upper part of the function, not the node's rows. On foot (not in vehicle motion, not
   sniping) the copy is at 007a7e8b-007a7e96: `ecx = [esp+0x68] + 0x10`, 12 dwords into the local frame. The
   other copy at 007a808e-007a8098 is `ecx = [weapon+0xc] + 0x10` (the holder's affine). The camera branch
   (007a7e2d-007a7e42) uses the thunk result instead. `[esp+0x68]` is written by `FUN_00544db0` (not read), so
   which object it is: [assumed] the character's world affine. The 3x4 layout: row0 = +0x10..+0x18, row1 =
   +0x1c..+0x24, row2 = +0x28..+0x30, translation +0x34..+0x3c.
   Axes ASSUMED x right, row1 = forward, row2 = up. Support: the holder's row1 (+0x1c, +0x20, +0x24) is the
   throw direction of `GrenadeProjectile_launch` (`fireTowardsTarget` false), and the mine is "thrown forward".
4. [game, emulated] Rotation: R = Rodrigues(row0, Z) and B = (XY) about the third axis, stored as R*B (the
   `matrix3_premultiply` call at 007a866c, see "fireDummy angle signs (emulated)"; the earlier B*R in step 7 is
   wrong). For v = forward = (0,1,0): v*R = (0, c, s) with c = cos Z = 0.766, s = sin Z = -0.643, then times B:
   d(XY) = (-c sin XY, c cos XY, s) = (-0.766 sin XY, 0.766 cos XY, -0.643).
5. The eight directions in character space (x right, y forward, z up; [game, emulated] signs, [assumed] frame):

   | barrel | XY | direction (x, y, z) | heading |
   |---|---|---|---|
   | FireDummy | 0 | (0, 0.766, -0.643) | forward, down |
   | FireDummy01 | 45 | (-0.542, 0.542, -0.643) | left-forward, down |
   | FireDummy02 | 90 | (-0.766, 0, -0.643) | left, down |
   | FireDummy03 | 135 | (-0.542, -0.542, -0.643) | left-back, down |
   | FireDummy04 | 180 | (0, -0.766, -0.643) | back, down |
   | FireDummy05 | -45 | (0.542, 0.542, -0.643) | right-forward, down |
   | FireDummy06 | -90 | (0.766, 0, -0.643) | right, down |
   | FireDummy07 | -134 | (0.551, -0.532, -0.643) | right-back, down |

   (The node names for barrels 0..7 are the ones in step 2 as listed in the order of the table; the order of
   `FireDummy` to `FireDummy09` against the XY list is [assumed].) Every barrel has z = -0.643: a ring 40 deg
   BELOW the horizon, not "up and outward". That fails the sanity check.
6. Upstream suspects, in order:
   a. [game, open] The sign of `t` in `FUN_00570770` (the open point (b) above). Taken as read it gives s < 0. If
      the true sign is the other one, z = +0.643 and the fan is up and outward, which would make sense.
      Not settled; the read is at 007a85a0-007a85e0 against the `-0.0` constant at `[0xb6a3ac]`.
   b. [game, open] The LAUNCH does not read the barrel slot. `GrenadeProjectile_launch` (`00789b80`) takes the
      direction from the grenade's `+0xc` holder (`+0x1c, +0x20, +0x24`) and the weapon's `+0xc4` attribute block.
      The grenade's `+0xc` is never set in the code read: `weapon_spawn_dispatch` (`00784170`, kind 2) sets
      `+0x40`, `+0x44` and `+0xc4` only, and `FUN_0078adc0` is NOT a creation (it is `grenade_list_flag`: it writes
      `+0x1e0 = 8` on a list), so the earlier note "FUN_0078adc0 creation" is wrong. The factory `FUN_00745ec0`
      makes a `GrenadeProjectile` (class hash 0x939c18a4) and is not read further. So the path from the barrel
      frame to the missile's direction is OPEN; even the right angles might not reach the launch.
   c. [assumed] The forward row is row1 (the launch's throw uses row1). If the character's frame differs, every
      x and y here changes, but z = -0.643 stays for all eight.
7. Not changed: `Arsenal::tick_special` in `game\src\weapons.rs` still uses `turn(facing, xy, -z)`, so the
   `[assumed]` negation stays in the unpublished notes "Phase 2, wave 3". Next reader: read the `+0xc` setter of the
   grenade (the factory or its constructor past `FUN_00745ec0`) and settle point 6a; a recording of the dump
   would settle the whole question without the disassembly.

## Audi car-form mine: when it drops (survey)

Question: when does the Audi's car-form special drop its mine? Ticket answer, 2026-10-12. The
old debugger finding ("no animation named Special in any of audir8's 19 sets, so the mine never
drops") is RIGHT that no set holds it, and WRONG that no clip exists: the clip is in the car's own
model bundle, which the port reads but never puts in the sim's clip table.

**Answer:** the car mine drops at clip tick 14 of the car clip `Special` (62 ticks long), that is
14/60 s = 0.233 s after the car special's clip starts [data] [assumed: clip start = the special's
start, speed 1]. The clip exists; the port does not play it in the sim.

Evidence:

1. [data] Flag and field (`characterAttributesAudi`, global_states.txt 50720 to 51099; MP block
   70808 has the same values): `specialAbilityWeaponFireFromAnim` false (foot, weapon
   `286593558`), `specialAbilityVehicleWeaponFireFromAnim` TRUE, `specialAbilityVehicleWeaponFireName`
   `-55547714` (= `fcb068be`, `AudiR8VehicleSpecialMine`), `specialAbilityVehicleAnim` `-2083125633`
   (= CRC-32 of "Special", `83d6067f`), `vehicleWeaponTimeTilCanFire` 0.75.
   Comparison, same fields: Jetfire car `VehicleWeaponFireFromAnim` false, `VehicleAnim` 0 (carpet
   bomb fires at the start: the one that works). Blackout car: from-anim TRUE, `VehicleAnim`
   `Special`. Megatron car: from-anim TRUE, `VehicleAnim` 0 (his car gun uses `SpecialAnimSet`).
   So the Audi's flag is NOT misread: it is true in the data, as the port reads it
   (`core/src/special_weapon.rs` `read_forms`, line 45: `from_anim: on("specialAbilityVehicleWeaponFireFromAnim")`,
   the `[1]` form; `fires_at_start` is `!from_anim`, line 199).

2. [data] Where the clip is. The car's clips are NOT in `animSet` (the 19 sets of `audir8.str`, all
   read: none has `Special`; `DriveSet` is `Vehicle_Idle` and the R2V / V2R transitions only, as the
   debugger said). They sit in the car's model: `things[2]/transformModel/vehicleProxy/obj/attachments[3]/thing`,
   an `AnimRefBundle` with 18 `anims` in this order: `V_Idle`(0), **`Special`(1)**, `Trans_R2V`,
   `Trans_R2V_A`, `Trans_R2V_B`, `Trans_R2V_Run`, `Trans_R2V_Run_A`, `Trans_R2V_Run_B`, `Trans_V2R`,
   `Trans_V2R_A`, `Trans_V2R_B`, `Trans_V2R_Jump`, `Trans_V2R_Death`, `Trans_V2R_Run`,
   `Trans_V2R_Run_A`, `Trans_V2R_Run_B`, `V2R_Slam`, `Trans_V2R_GroundPunch`. Found by
   `work\survey\flight\audi_car_anims.py audir8.str` (the only leaf in the whole tree with the
   `Special` CRC is this `animID`). The clip: `length 62`, flags `ANIM_FLAG_LOOP` not set in the
   listing for `Special` [not checked], one `AnimKeysEvent` stream with 4 events:
   - t=0 `AnimPlaySoundEvent` (id -2066580498), t=0 `AnimPlaySoundEvent` (1784210822)
   - **t=14 `WeaponFireSpecialEvent`** (no fields)
   - t=52 `AnimPlaySoundEvent` (-651941728)
   Read by `work\survey\flight\audi_car_special_events.py audir8.str 3 1`.
   Control: Blackout's car `Special` (bundle `attachments[6]`, `anims[8]`, length 50) has one event,
   `WeaponFireSpecialEvent` at t=14 [data]. The same tick for both.

3. [game] Why the port never plays it. `core/src/character.rs` lines 228-233 read the car bundle
   into `vehicle_animations` (for the car's visuals, `game/src/main.rs` line 234, `VehicleAnimator`).
   The sim's clip table, `core/src/tuning.rs` `Actions::from_animations` (SETS, line 185-186), is
   built from the robot's `animSet` sets only (`CombatSet`, `DashSet`, `GroundPunchSet`, `ClimbSet`,
   `DriveSet`, `Vehicle2RobotSet`, `DeathSet`, `FlySet`, `HoverSet`, `SpecialAnimSet`). So
   `t.actions.clips` has no entry for CRC("Special"), `start_clip` (`core/src/sim.rs` 3302, first
   line `let Some(data) = t.actions.clips.get(&clip) else { return false }`) returns false, and no
   clip means no tick, no `WeaponFireSpecialEvent`, no mine. This matches the debugger's
   "the mine never drops", with the cause being the clip table, not the data.
   [assumed] The car special's clip id is CRC("Special") from `specialAbilityVehicleAnim`: the car
   special state (`stateAttackSpecial`, global_states.txt 127) has `animName` empty, so the name is
   not in the state; it must come from the attribute. Not read in game code; settles by reading the
   state's start in `0072afd0` or the `stateAttackSpecial` clip choice.

4. [data] The vehicle special state: no trigger for the car. `specialAbilityVehicleSelfTrigger` false,
   `specialAbilityVehicleLuxScriptNameToTrigger` 0 (Audi). So no message is sent at the car special's
   start, and the only fire path is the clip's event. The data as shipped has a trigger for the car
   mine: the event at tick 14 of `Special`. The Audi's `AudiR8_VehicleSpecialMine` is `launchSpeed`
   1 (dropped, as section D says). Searched `global_states.txt` for other state or connection names
   with "Special" and "Vehicle": only the weapon blocks and `Blackout_VehicleSpecialDeployableTurret` /
   `Ironhide_VehicleSpecial`; no separate car-special state exists for the Audi.

**Smallest port change (not made):** put the car bundle's clips in the sim's clip table, after the
robot sets: `Actions::from_animations` (tuning.rs) also takes `vehicle_animations` (character.rs
`vehicle_animations`) and inserts each clip under `crc32(id)` when the id is not already present,
because `Trans_R2V_A`, `Trans_R2V_B` and `V_Idle` also appear in `DriveSet` (the car's clips shadow
the same names: [assumed] the car form must use its own bundle, so it may need its own table). The
event then reaches `007ba870` / `special_weapon_fire_event`; `forms[1]` (car) has `from_anim` true, so
the mine drops at tick 14 of the clip, `14 / 60` s after the start [assumed: speed 1 and the clip
starts with the special].

Tags: [data] everything above read from `global_states.txt`, the pack bundles and the events;
[game] the port's read paths (`character.rs`, `tuning.rs`, `sim.rs` 3302, `special_weapon.rs`); [assumed]
the clip's start time and the state's clip choice (item 3). Not checked in game: the car mine
itself; no recording has a car special.

## Mines closed (2026-10-18)

- [game 0117e6d0] The proximity team test, read: single player counts a target only when thrower
  and target are the two opposite factions (`+0x244`, the attributes' `team`: `0x951af706` the
  Autobots, `0xa54dc247` the Decepticons); any other team is nobody's enemy. Multiplayer uses
  `ai_same_team`. Ported as `weapons::thrower_hostile` (+ test); the deployed turret's target
  filter uses it. The arena's targets carry no team and are all the player's enemies, so the
  mine's own proximity test needs no filter there.
- [game 00787ea0 l.144-151] The 0.18 test: the three pairwise dot products of the touched
  object's inverse-frame rows must each lie within +-0.18, i.e. its frame is near orthogonal;
  then the mine sticks to it. Level geometry passes, so the port's "every world touch sticks" is
  the game's behaviour for the world. A sheared frame falls to the character lookup and a hit.
- [game 00786a60 l.6-28] `oneOutAtATime` ported: `GrenadeTuning::one_out_at_a_time` (data), and in
  the game a mine that arms expires the weapon's other live mines with no blast and no sound
  (`SoundOut::projectile_stop`). [assumed] the flag is `+0x16c` (field order), carried.

## Carpet bomb settled: the sticky timer and the spacing (2026-10-18, third pass)

- B5 settled [game 00788da5-00788e2b]: at the end of `sticky_attach`, when stickyMin (`+0x18c`) > 0
  it draws `random_range_with_clock(+0x18c, +0x190)` (00551d60) and posts the hit message 54732c8d
  (`post_delayed_message` 005a9800) for that delay: the bomb goes off 0.1 s after it sticks. With
  stickyMin <= 0, a grenade that is not a proximity one (`+0x178` clear) is hit at once. The offsets
  are read off the loader 007bea00: `+0x16c` oneOutAtATime, `+0x178` isProximity, `+0x179`
  proximityActivateOnCollision, `+0x180/+0x184` the proximity delays, `+0x188` isSticky, `+0x18c/+0x190`
  stickyMin/MaxLifetime, `+0x194` stickyOffsetFromCollision.
- The contact 007876b0 (787c23): a proximity grenade not armed by collision is hit at once when the
  touched body has a character object (`[esp+0x1c]`, from 0070a3c0 / 00724520); a world touch goes on
  to the arming and `sticky_attach`.
- B2 settled [game 007ac370 l.38-84, 007ae010 l.54-63]: the burst is self-running. weapon_fire counts
  `0xfd` up (to 0 with the clock `0xfe` once it reaches burstPerFire `+0x1c`) and takes
  timeBetweenEachBurst (`+0x20`) off `0xfe` (kept >= 0); the weapon update adds dt to `0xfe` while
  `0xfd` > 0 and fires (vtable `+0xcc`, aim from the lock or the stored aim) once `+0x20 <= 0xfe`.
  At that moment, if the owner's `+0x148` bit 3 is clear: it goes on when `isSpecial` (`+0x5d`, the
  first reader of that field found; vtable `+0x9c` = 0074f250), or for `isVehicleModeWeapon` (`+0x61`,
  vtable `+0xac` = 0074f1d0) when `character_in_vehicle_motion` 0070ab90 (drive or fly mode), or else
  by the weapon's byte `+0x38` (unread); otherwise `0xfd = 0`, the rest of the burst dropped. So the
  8 bombs come 16, 16, 15, ... updates apart and stop if Jetfire changes back to the robot.
- Ported: `core\src\weapons.rs` `GrenadeTuning::sticky_lifetime`, `Grenade`;
  `core\src\special_weapon.rs` `SpecialFire::step` (whole game updates), `set_vehicle_motion`.
- Still open: the order of the special's press and the weapon update within one game update
  ([assumed] update first), and the bit 3 of the owner's `+0x148`.

## Jetfire's direction settled (2026-10-18)

- (a) [game] The sign of `t`: 007a8587 calls `cos_float` 0040b080 on the Z angle and its result
  is passed by pointer in EDX; 007a8597 calls `sin_float` 0040b0a0 and its result is the second
  stack argument; no `fchs` or `-0.0` between (disasm 007a854d..007a85bb). So the emulated
  matrix is the game's: the tilt is as read, `sin(-40)` < 0.
- (b) [game] The launch's frame: `weapon_fire` 007ae010 calls the projectile maker (vtable `+0x80`)
  with `param_1 + 0x15`, i.e. byte `+0x54`, the barrel slots `007a7a40` wrote; the dispatch 00784170
  then takes the GrenadeProjectile of the spawned object (`fn_00745ec0`) and launches it along its
  holder's forward row (`+0x1c`). So each round flies along its barrel frame's forward row.
- Result: R*B as stored, applied to the forward row, is `(-0.766 sin XY, 0.766 cos XY, -0.643)` in
  the character's frame: the missile dump is a ring of eight rounds 40 degrees BELOW the horizon,
  as the table in "Barrel frames (read)" has it. The port's `[assumed]` negation (a ring above) is
  withdrawn: `Arsenal::tick_special` now passes the data's Z as it is.
- Still [assumed]: the character frame is x right, row1 forward, row2 up, and the barrel order
  against the XY list. What would settle them: a recording of the dump.
