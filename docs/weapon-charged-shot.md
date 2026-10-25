# Weapon charged shot (the charge of Long Haul's grenade launcher)

Read by the charged-shot ticket (`work\survey\charge\`). Scope: the charge only, not the lock-on.
Tags: `[game]` read in code (address given), `[data]` from the export, `[assumed]` inferred, `[unread]` not read.

## Where it sits

- The weapon update `007ac370` (vtable `+0x08` of class `00b5fba0`, the gun/launcher class) calls
  the charge update through vtable `+0x12c` = `007ad1c0` (`charged_shot_update`) with `(dt, held)`,
  at its line 106 `(**(param_1+300))(param_2,(char)local_3c)` [game]. `local_3c` is set before that
  point and was not traced [unread].
- Vtable slots of `00b5fba0` used by the charge (byte offset = slot index x 4; from
  `tools\pe_read.py vtable`, read with limit 200) [game]:
  - `+0x8c` can fire = `007a8da0` (heat under capacity, or a linked weapon via `+0xdc`)
  - `+0x94` trigger down = `007a8de0`
  - `+0xdc` weapon parent = `0074f100`
  - `+0x100` add heat = `007a90c0` (`-1` = the weapon's own per-round heat)
  - `+0x124` charge level, read by the reticle = `007b0b00` (see section 4)
  - `+0x130` start charging = `007adae0`
  - `+0x134` charge step = `007addc0`
  - `+0x138` fire with charge = `007af210`
  - `+0x14c` charge reached full = `007b0520`
  - `+0x7c` fire sound, `+0x80` make projectile = `007aaeb0`, `+0x84` = `007aab70`, `+0x74` aim point = `007aa210`
  - `+0x4c` returns the shot count used by the loop in `007af210` [unread; its body is `007a8770`? not confirmed]
- The `+0x124` in the reticle is a VIRTUAL CALL (vtable offset), not a field of the weapon. No
  code writes a charge value into the object for it: the value is computed on each read.

## 1. Attribute fields (loader `007bbc30`, `tools\decomp_lookup.py fields 007bbc30`) and Long Haul's values

Base: `attr+0x..`, the pointer at weapon `+0x3a0` (word 0xe8) [game, `007ad1c0` disasm 007ad20a].
Data: `export\data\global_states.txt` line 91489, `LongHaul_GrenadeLauncherAttribute`
(a copy `LongHaul_GrenadeLauncherAttribute2` at 91593 has the same charge fields) [data].

| attr offset | field | Long Haul | read in |
|---|---|---|---|
| +0x10 | numProjectilesPerFire | 1 | `007af210` (local_1d4) |
| +0x14 | rateOfFire | 0.4 | fire interval |
| +0x18 | automaticFiring | false | - |
| +0x24 | heatMeterCapacity | 100 | `007ad1c0` line 155, `007a8da0` |
| +0x28 | heatMeterIncrementPerRound | 8 | `007a90c0` with `-1` |
| +0x3c | heatMeterOverHeatRateWhileCharging | 9 | `007addc0` (heat per second) |
| +0x62 | hasChargedShot | true | `007ad1c0` line 15, `007b0b00` |
| +0x68 | timeTilChargeStarts | 0.2 | `007ad1c0` |
| +0x6c | chargeDurationTilMaxEffect | 4 | `007ad1c0`, `007b0b00`, `007addc0` |
| +0x70 | chargeHoldDuration | -1 | `007ad1c0` line 72: only used when > 0, so OFF |
| +0x74 | fireOnlyFullyCharged | false | NOT read in the four functions above [unread elsewhere] |
| +0x78 | minChargeDamageMultiplier | 1 | `007af210`, only when attr+0x88 < 2 |
| +0x7c | maxChargeDamageMultiplier | 2 | `007af210`, only when attr+0x88 < 2 |
| +0x80 | minHeatMeterIncrement | 5 | `007af210` line 444, only when attr+0x88 < 2 |
| +0x84 | maxHeatMeterIncrement | 10 | `007af210` line 444, only when attr+0x88 < 2 |
| +0x88 | chargeNumProjectilesMaxMultiplier | 3 | `007af210`, `007addc0` (N) |
| +0x8c | chargeHeatIncrementPerMultiplierIncrement | 14 | `007addc0` (heat per step) |
| +0x130 .. | grenade fields (launchArcAngle 3, launchSpeed 270, Zgravity -32, bounciness 0.55 ...) | | `00789b80`, not charge |

Weapon object words used by the charge (base `this`, word index x 4) [game]:

| offset | word | meaning |
|---|---|---|
| +0x3a0 | 0xe8 | attr pointer |
| +0x3c8 | 0xf2 | heat |
| +0x3ec | 0xfb | seconds since the last round |
| +0x40c | 0x103 | charging flag (byte) |
| +0x410 / +0x414 | 0x104 / 0x105 | emitter scale min / max for the charge (`007addc0`) [assumed setter unread] |
| +0x418 | 0x106 | seconds the trigger has been held (charge clock) |
| +0x420..+0x428 | 0x108..0x10a | fire position (aim point) |
| +0x404 | 0x101 | charge emitter template id (`007adae0`) |
| +0x408 | 0x102 | charge-max emitter template id (`007b0520`) |
| +0x448 | 0x112 | charge-max sound event (`007b0520`) |

## 2. Numbered pseudo-code: the charge per step

Calls: `007ac370` calls `charged_shot_update(this, dt, held)`. `held` = the weapon's trigger state
for this step (`local_3c`, see above) [assumed from the `+0x94` slot and from the two uses below].

Per step, `007ad1c0(this, dt, held)`:

1. `if this.weaponParent (vtbl+0xdc) points at a live object: return` [game 007ad1c9..007ad1e9].
2. `canFire = vtbl+0x8c()` (heat under capacity, or linked weapon) [game 007ad1f2].
3. `if held: flag = this.charging (byte +0x40c) else flag = canFire` [game lines 8-10].
4. `attr = this.attr; if attr == 0: return` [game].
5. `if attr.hasChargedShot == 0 or flag == 0:`
   - `if attr.hasChargedShot == 0: return`
   - `if flag != 0: return` (unreachable here)
   - `this.acc = 0; return`   (charge is LOST: not charging, or the weapon cannot fire) [game lines 15-27].
6. `acc = this.acc (+0x418)`. `interval = fire_interval(this)` (`007a8f20`: rateOfFire / slow factor) [game
   007ad24c]. The decompiler drops the compare; the disassembly (007ad24c-007ad264) compares the
   interval with the since-last-round value (+0x3ec) and, when that holds, subtracts since-last-round
   from acc. [assumed: it removes the time since the last round from the charge clock, so the shot's
   own rounds are not counted]. The same subtraction is in the getter `007b0b00`.
7. `if (acc <= attr.timeTilChargeStarts) and not held: return` [game 007ad26a-007ad27a]. A held
   trigger continues even before the threshold (see 8).
8. `level = (acc - attr.timeTilChargeStarts) / attr.chargeDurationTilMaxEffect` [game 007ad280-007ad290:
   `1/dur` times `(acc - start)`]. Clamped above at 1.0; NOT clamped below, so it is NEGATIVE while held
   before the start (a fact of the code: level < 0 possible) [game 007ad2b9-007ad2d0; the `1.0 <=` clamp
   at lines 40-46].
9. `prev = ((acc - start) - dt) / dur`, clamped as in 8 [game line 42: `fStack_80 = (fVar11 - param_2)/dur`].
10. If `prev < 1.0 and level >= 1.0`: the charge reached full this step: `vtbl+0x14c()` (`007b0520`, step 22
    below) and the matching components get the same call [game lines 47-68].
11. If `held`: if not charging: `this.charging = 1; vtbl+0x130(0)` (start, `007adae0`); else
    `vtbl+0x134(level, prev, dt)` (step, `007addc0`) [game lines 84-117, LAB_007ad446].
12. If `held`: return after the step (no shot). If released (`held == 0`): fall through to the shot
    (line 158) [game].
13. Aim point: the target character's position if there is one (`character_object_lookup 0070a3c0`, and
    not flagged `0x1000` and `scripted_health_controller` = 0), else the weapon's `+0x420` [game lines
    161-175].
14. `vtbl+0x138(level, &aim, &aux, 0, 0)` = FIRE WITH CHARGE (`007af210`, below) [game line 230]. Then
    `this.acc = 0` in the fire function (line 456) [game].

Fire with charge `007af210(this, level, aim, transform, ..)`, read in `--full` lines 1-30, 92-200, 436-458:

F1. `this.charging = 0` [game line 7]. If not `from-slot` (`param_5 == 0`): `vtbl+0x158(0)` [game line 9].
F2. `count0 = attr.numProjectilesPerFire (+0x10)`. If `attr.N (+0x88) > 1`: `count0 = (int)((N-1)*level) + 1` times that,
    i.e. `count0 = numProjectilesPerFire * (floor((N-1)*level) + 1)` [game lines 15-21].
    Long Haul (N = 3): 1 shot at level < 0.5, 2 at 0.5 <= level < 1, 3 at level = 1 (one per 0.5 of charge).
F3. The loop runs `iStack_1e4` times (set from a call with `count0`, `vtbl+0x4c`, not read [unread]) [game lines 68-90, 294-295].
    Per projectile j:
    - the muzzle transform is `this + j*0xc + 0x54` (the words from `0x15`) [game line 137].
    - damage multiplier `m = attr.min + (attr.max - attr.min) * level` (`attr+0x78`, `+0x7c`) **only if
      N < 2**; otherwise `m = 1.0` [game lines 124-126: `piVar2[0x22] < 2 && (...)`]. **For Long Haul
      (N = 3) the charge does NOT change the damage multiplier**: `m = 1.0`.
    - `make projectile = vtbl+0x80(record, uVar10, transform_j, &aim, bVar12)`; `uVar10 = 1` only when N < 2
      and level >= 1 [game lines 123-137]. For Long Haul `uVar10 = 0`.
    - `weapon_spawn_dispatch 00784170(record, ..., transform, attr, ..)`: grenade kind 2 -> `0078adc0` then
      `00789b80` (`GrenadeProjectile_launch`) [game line 180-190, docs/weapon-kinds.md].
    - The bullet path (`00784080`, `bullet_trace_setup`) receives `m` (line 268) and is not the grenade.
    - `vtbl+0x158`, the sound `0x7c` (fire sound) once after the loop [game lines 291, 372].
    No delay between the shots is coded in the loop: all of a fire's grenades are spawned in one update
    [game]. The spread comes from the muzzle transforms, not a timer.
F4. Heat: if `N < 2`: `heat = min + (max-min)*level` (`0x80`, `0x84`); else `heat = -1` (the weapon's own
    per-round heat, `heatMeterIncrementPerRound`, 8 here) and then `vtbl+0x100(heat)` [game lines 444-451].
F5. `if (pad owner, not a kind-2 controller) and (N < 2 or level >= 1)`: `pad_rumble(1, rumbleFireDuration,
    rumbleFireStrength)` in the fire path [game 007af210 lines 406-411: `attr+0x128` 0.4, `attr+0x12c` 0.85].
F6. `this.acc = 0` (+0x418) [game line 456].

Per step while held: `007addc0(this, level, prev, dt)` (`vtbl+0x134`), read fully in lines 1-50 [game]:

S1. `if dt <= 0: return` [game].
S2. Rumble: `if (N < 2 or level >= 1) and owner pad: pad_rumble(2 (partial) or 3 (full), dt, 0.1 or 1.0)`
    [game 007addc0 lines 36-38].
S3. Emitter scale for the charge effect: `scale = (this+0x414 - this+0x410) * level + this+0x410`, with
    `scale <= 0 -> 0.01` [game lines 19-27 of `007addc0`, particle_emitter_scale `0057d670`].
S4. Heat while charging:
    - if N < 2: `heat += attr.heatOverHeatRateWhileCharging (+0x3c) * dt` [game].
    - else let `s(x) = int((N-1)*x)`. If `s(level) <= s(prev)` (no new step): if `level < 1.0`: nothing; if
      `level >= 1.0`: `heat += attr.+0x3c * dt`.
    - else (a new step was crossed): `heat += attr.chargeHeatIncrementPerMultiplierIncrement (+0x8c)`,
      once (not times dt) [game lines 40-46].
    Long Haul: a full charge crosses steps at 0.5 and 1.0, so `+14` twice (28 total) [assumed from the
    integer steps of N-1 = 2], then `+9` per second while held at full.

Full-charge notice `007b0520` (`vtbl+0x14c`), read as an outline [game]:
M1. Spawns the charge-max emitter with template id `this+0x408` at the `ChargeDummy` node of the
    owner's model (name hash `0x1d4f4f28` = `ChargeDummy`; search of `owner+0x74`) [game].
M2. `sound_event_wrapper_start(..., this+0x448, owner+0x34, ...)` and `sound_tie_to_object(...)` [game].
    The event id stored at `+0x448` is set by code not read [unread].

Start of charge `007adae0` (`vtbl+0x130`), read in full [game]:
C1. `this.charging = 1` (byte +0x40c).
C2. Spawns the charge emitter with template id `this+0x404` attached to the `ChargeDummy` node
    (particle `0057e820`, trigger `0057dd30`) [game].
C3. Calls `vtbl+0x144`, `vtbl+0x154`; `vtbl+0x158(1)` if the flag argument is 0 [game]. Those three are
    not read [unread].

## 3. The grenade launcher and the count

- One call of the fire with charge (`vtbl+0x138`, `007af210`) per RELEASE, with `level` as an argument.
  The COUNT is computed inside that call from `level` and `attr+0x88`: `count = numProjectilesPerFire *
  (floor((N-1)*level) + 1)`. So for Long Haul: 1, 2 or 3 grenades per release, all in one call, no delay
  between them. [game]
- The grenade launcher is the same class as the missile launcher (`00b5fba0`, constructor `007ab580`,
  `GrenadeLauncher_make` `01186060`) [game, docs/weapon-kinds.md]. The charge does not change the
  grenade's own explosion damage (the record's `overwriteExplosiveDamage` 30) in the code read [game,
  `007af210` passes `m` only to the bullet path]. Whether the grenade record gets `level` anywhere is
  in `007aaeb0`/`007ace80` [unread].

## 4. The reticle value (`007b0b00`, vtbl+0x124, read in full) [game]

R1. `if attr == 0 or attr.hasChargedShot == 0: return -1`.
R2. `acc = this+0x418 - attr.timeTilChargeStarts`; the same since-last-round subtraction as step 6.
R3. `if acc <= attr.chargeDurationTilMaxEffect: level = acc / dur; return max(0, level)` else `return 1`.
So the reticle's charge is `-1` for no charged shot, `0` before the start, then `0..1`. No store at +0x124.

## Heat and ammo

- No ammo. The cost is heat: per release `+8` (`heatMeterIncrementPerRound`, via `vtbl+0x100(-1)`) for
  Long Haul (N = 3); while held, `+14` per step crossed and `+9/s` at full. Cancels: `this.acc = 0` when the
  weapon cannot fire (`canFire == 0`, step 5) or has a linked weapon (step 1 returns without reset).
- Cancel by hold: `chargeHoldDuration` (+0x70) > 0 sets `cStack_c9 = 0` when the hold past full exceeds it
  (step 9 of the code, `007ad1c0` lines 69-77). Long Haul has -1, so this does not apply.
- `fireOnlyFullyCharged` (+0x74) is NOT read in these functions [game, grep]. A release before full still
  fires at its level [game: no gate on the level before `vtbl+0x138`].

## Answers in short

- What starts: the trigger held (`held`). The charge starts (start effect, flag set) on the first held step,
  even before 0.2 s; the level counts from 0.2 s (`timeTilChargeStarts`). A release before 0.2 s does
  nothing (step 7) [game].
- Growth: `level = (acc - 0.2) / 4` per step, `acc` the held seconds (`+0x418`, += dt in `007ac370`
  line 124, minus the since-last-round term) [game].
- Levels: 1 shot below 0.5, 2 at 0.5..1, 3 at 1 (Long Haul). Damage multiplier 1 (not applied for N >= 2).
- Release: fire with `level` (step 14); cleared at `acc = 0` (F6).

## Unread

- `vtbl+0x4c` (the shot count used by the loop in `007af210`) and its clamp to the muzzle transforms [unread].
- The sinks of `level` in `007aaeb0` (make projectile, `uVar10`) and `007ace80` (line 196 of the fire) [unread].
- The grenade's use of a charge level in `00789b80` [unread]; `00789b80` was not read for it.
- `vtbl+0x144`, `+0x154`, `+0x158` (start) and `+0x148`, `+0x150` (step) [unread].
- The setters of `+0x404`, `+0x408`, `+0x410`, `+0x414`, `+0x448` (charge emitter and sound ids) [unread].
- `local_3c` in `007ac370` (the `held` argument) [unread].
- The sound events and the hash names of the charge sounds (they come from `+0x448` and the emitter ids) [unread].
- `fireOnlyFullyCharged` (+0x74): no read found in the functions above [unread elsewhere].
- The lock-on (`mustHoldFireTriggerToLockOn`) is not part of this note.
