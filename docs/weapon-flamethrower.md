# FlameThrower: loaders, vtable, update, tick and what is not found (2026-10-12)

Ticket: READ only. Nothing built, no source edited. Tags: `[game]` read in code (address given),
`[data]` read in the data dump, `[assumed]` a guess, `[unread]` not read. Scratch: none written.
Follows `docs/weapon-kinds.md` (data and loaders); this note adds the vtable, the update, the
firing on/off, the damage tick and the hit test. Ledger entries added with `fn.py note`.

Summary of the answer: the flame is a continuous weapon. Its update (vtable slot 2, `0074c200`)
runs a 5 Hz damage tick (`1 / damageTicsPerSecond`), a list of three `FlameThrowerDummy`
physics volumes that switch on one at a time, and a ray test each frame. The damage amount per
tick (`damagePerSecond` / 5) is stored at attr+0x130 but NO reader of +0x130 is found in the
flame code. The cone, the reach, and how a hit becomes damage are NOT found.

## 1. Field tables

Attribute extension loader `FUN_007bf710` (census label TurretLauncherAttribute is WRONG) [game],
which calls the base loader `FUN_007bbc30` first (base offsets in `docs/weapon-kinds.md`; base
fields run to +0x12c). Offsets are on the attribute object `attr+0x..`:

| field | offset | note |
|---|---|---|
| (base fields, `FUN_007bbc30`) | +0x10 .. +0x12c | see weapon-kinds.md |
| damagePerSecond | attr+0x130 | default 100 in the attribute reader, longhaul 110 [data] |
| damageTicsPerSecond | attr+0x134 | longhaul 5 [data]; used as the tick rate (below) |
| damageFlags | attr+0x138 | `data_field_by_hash` 0x1e0c88ef |
| specialCaseDamageStuff | attr+0x13c | via `FUN_00744f10` |

Weapon object loader `FUN_0074b540` (component), per `decomp_lookup.py fields`:

| field | offset on weapon | note |
|---|---|---|
| streamEffect | weapon+0x3f0 | (`param_1[0xfc]`); spawned on start as the FireDummy emitter |
| impactEffect | weapon+0x3f4 | TriggerableParticle at hits (`FUN_00746dd0`) [game] |
| SSDImpactPreset | weapon+0x3f8 | SSD preset |
| SSDPeriodChecker | weapon+0x3fc | |

Attribute selector `FUN_0074b7a0` [game] (`decomp_lookup.py fields` gives only +0x28):

| field | offset on weapon | note |
|---|---|---|
| attribute name hash | weapon+0x28 | AI: `flameThrowerAIAttribute` 0xf43142e9 when the controller is not the player (`character_input_is_player` 00711210); else `flameThrowerAttribute` 0xa955c899, or the MP name 0x25907a0c when `DAT_00dc967c != 0` (the code adds 0x7c3ab173 to 0xa955c899, which is 0x25907a0c) [game] |
| resolved attribute object | weapon+0x3a0 and weapon+0x3ec | the update reads `attr = weapon+0x3ec` [game]; fallback `DefaultFlameThrower` 0x7dd845f0 |

Live-state fields of the weapon used by the update [game]:

| field | offset | role |
|---|---|---|
| firing flag | weapon+0x400 (byte) | 1 while firing (`flame_set_firing`) |
| dummy active index | weapon+0x404 | next dummy to switch on |
| dummy timer | weapon+0x408 | counts down to the next dummy; set from dummy+0x2c |
| dummy list head | weapon+0x424 (sentinel weapon+0x420) | list of the three FlameThrowerDummy objects |
| hit list | sentinel weapon+0x42c, head weapon+0x430 | per-hit entries with float timers (entry+0x10), freed when <= 0 [partly read] |
| tick serial | weapon+0x438 (`param_1[0x10e]`) | set from the global `DAT_00cf5cb8` each tick; the owner object's +0x438 is also written with it [game, decompile]. The disassembly at 0074c4e5 also writes `[ebx+0x438]` with a pointer: this CONFLICT with the decompile is NOT resolved |
| damage tick timer | weapon+0x43c | counts down to the next damage tick |
| glow ramp | weapon+0x458 | `t`; drives a light/glow with `2t` or `1-2t` [game] |

## 2. Vtable `00b5dae8` (the FlameThrower class)

Source: `pe_read.py vtable 00b5dae8` (64 entries, the table read from the executable; the
`export\vtables.tsv` file has no entry for it). Slot = offset / 4. Entry: slot, address, what it
is. "read" = read this session or already in the ledger.

| slot | address | what |
|---|---|---|
| 0 | 0074b2d0 | scalar deleting dtor (weapon-kinds) |
| 1 | 0074c110 | sends the hash 0xd11292ac (FlameThrower) via `FUN_00578a80` [game], outline only |
| 2 | 0074c200 | UPDATE (`flame_update`), read |
| 3 | 008b7f70 | 3-byte stub (lux) |
| 4 | 0074ba20 | generic WeaponObject_init (not flame-specific) |
| 5 | 00544340 | virtual_noop |
| 6 | 0053a250 | 5-byte stub (lux) |
| 7 | 007a75a0 | unread |
| 8 | 0074f320 | FlameThrowerDummy group, unread |
| 9 | 007a7630 | unread |
| 10 | 007a7750 | unread |
| 11 | 007a7830 | unread |
| 12 | 0074d100 | 3-byte stub |
| 13 | 0074f300 | FlameThrowerDummy group, unread |
| 14 | 0074f2d0 | FlameThrowerDummy group, unread |
| 15 | 0074f2c0 | FlameThrowerDummy group, unread |
| 16 | 0074f2b0 | 10 bytes, unread |
| 17 | 007a6460 | MeleeWeapon group, unread |
| 18 | 007a7a40 | weapon_muzzle_transforms, read (ledger) |
| 19 | 007a8770 | unread |
| 20 | 007a7470 | weapon_is_in_hand, read (ledger) |
| 21 | 007a9700 | weapon_heat_update, read; the update calls it each frame (`vtable+0x54`) |
| 22 | 007a9b40 | start hook: called by `flame_set_firing(1)` (`vtable+0x58`), unread |
| 23 | 007a9d40 | stop hook: called by `flame_set_firing(0)` (`vtable+0x5c`), unread |
| 24 | 007a9dd0 | fire step: called by the fire routine (`vtable+0x60`), unread; heat per shot is probably here [assumed] |
| 25 | 007a9ec0 | unread |
| 26 | 007a9fe0 | unread |
| 27 | 007aa070 | unread |
| 28 | 007aa110 | unread |
| 29 | 007aa210 | unread |
| 30 | 007aa8c0 | unread |
| 31 | 007aac80 | unread |
| 32 | 007aaeb0 | weapon_spawn_projectile (weapon-kinds; not used by the flame) |
| 33 | 007aab70 | unread |
| 34 | 0074f2a0 | FlameThrowerDummy group, unread |
| 35 | 007a8da0 | "may fire" check (`vtable+0x8c`), unread |
| 36 | 007aa190 | aim/ray source (`vtable+0x90`), unread; origin of the hit ray |
| 37 | 007a8de0 | "is trigger held" check (`vtable+0x94`), unread |
| 38 | 0074f270 | FlameThrowerDummy group, unread |
| 39 | 0074f250 | FlameThrowerDummy group, unread |
| 40 | 0074f230 | FlameThrowerDummy group, unread |
| 41 | 0074f210 | FlameThrowerDummy group, unread |
| 42 | 0074f1f0 | FlameThrowerDummy group, unread |
| 43 | 0074f1d0 | FlameThrowerDummy group, unread |
| 44 | 0074f1a0 | unread |
| 45 | 0074f170 | unread |
| 46 | 0074f160 | unread |
| 47 | 0074f150 | unread |
| 48 | 007bb820 | 3-byte stub |
| 49 | 0074cd20 | FIRE WHILE HELD (`flame_fire_held`), read |
| 50 | 0092a050 | 3-byte stub |
| 51 | 0074f140 | 3-byte stub |
| 52 | 0074d0c0 | set firing by a float at arg+8 > 50 (`flame_slot52`), read; meaning of arg+8 unread |
| 53 | 007cfc70 | 20 bytes, unread |
| 54 | 0074b7a0 | FlameThrowerAttribute_load, read |
| 55 | 0074f100 | "can fire" object check (`vtable+0xdc`), unread |
| 56 | 0074f0d0 | unread |
| 57 | 007ab010 | unread |
| 58 | 007ab060 | unread |
| 59 | 0074f0b0 | unread |
| 60 | 0074f0a0 | unread |
| 61 | 0074f070 | unread |
| 62 | 0074f050 | "stop/hold" check (`vtable+0xf8`), unread |
| 63 | 007a9610 | float scale for the update (`vtable+0xfc`, times dt), unread |
| (64) | (not in table) | the update calls `vtable+0x100` with `fVar14 * dt`; slot 64 is outside the 64-entry table [unread] |

Note on the vtable: most slots are the generic gun/launcher methods (shared with `00b5fba0`
per weapon-mode.md), and the flame's own methods are the `0074xxxx` group. The census label
"FlameThrower" on `0074ba20` is wrong (it is the generic init, see weapon-kinds.md).

## 3. Firing on and off

### 3a. Fire while held: `FUN_0074cd20` (slot 49) [game]

Pseudo-code (from the outline and lines 1-40 of the function):

1. If attr (`weapon+0x3ec`) != 0 and the weapon's fire timer `piVar4[9]` <= the value at `weapon+0xf2`:
   call `weapon_fire_interval` `007a8f20` (rateOfFire / slow-fire, `docs/damage.md`), then
   `vtable+0x60` (slot 24, fire step).
2. If attr != 0 and `vtable+0x8c` (slot 35) returns true, and `vtable+0xdc` (slot 55) returns an
   object with word 0 == 0: call the target's `+0xc4` with the args, and `FUN_00532c70`.
3. If the firing flag `weapon+0x400` is 0: `flame_set_firing(1)` (3b). Then if the owner's
   `scripted_health_controller` (`00711260`) is not set, `FUN_0074cec0(1, 1)` (the stream effect on,
   [unread]).

### 3b. `FUN_0074d110(weapon, on)`: start (on=1) and stop (on=0) [game]

Start (on=1), only if `weapon+0x400` is 0:
- `vtable+0x58` (slot 22) start hook.
- If the stream effect (`weapon+0x3f0`) != 0 and the FireDummy index (`FUN_00578440`, hash
  0xeffca07e) is in range: spawn the stream emitter (`FUN_0057b750` + `particle_emitter_spawn_attached`
  `0057e820`), `FUN_0074af20`, `FUN_00562970` (an accessor, no effect).
- Glow ramp `weapon+0x458 = 0.5 - weapon+0x458` [game].
- Reset: dummy index `+0x404 = 0`, dummy timer `+0x408 = 0`.
- Ammo/heat check: when `vtable+0xdc` returns an object with word 0 == 0: `weapon+0x43c = 1.0`
  (the first damage tick comes 1.0 s after start) [game, `0x3f800000`].
- Message: `send_ui_integer` with 0xa45a3365 (and 0x925ae355 / 0xcb5d83cd per the `+0xd0` flags)
  [game; the HUD meaning is in docs/hud.md].
- Finally `weapon+0x400 = 1`.

Stop (on=0), only if `weapon+0x400` is 1:
- `vtable+0x5c` (slot 23) stop hook.
- Each FlameThrowerDummy in the list: `FUN_0074e9b0(d, 0)` (off) [game].
- `DebugCountTrigger` `007e1b60` on the hit-list sentinel (`weapon+0x42c`, `param_1+0x10b`) [game].
- Glow ramp flipped as in start; `weapon+0x404 = 0`, `weapon+0x408 = 0`.
- `weapon+0x400 = 0`.

### 3c. Stop from the update [game, disasm 0074c3a0-0074c3d1]

In the update, when `vtable+0x94` (slot 37, "trigger held") returns false, or `vtable+0xf8`
(slot 62) returns true, the update calls `flame_set_firing(0)` and `FUN_0074cec0(0, 1)`
(the stream effect off). The disassembly quoted: `call vtable+0x94`, `test al, al`, `je`, then
`call vtable+0xf8`, `test al, al`, `je 0x74c477`, then `push 0; push ebx; call 0x74d110`
(stop) and `push 1; push 0; call 0x74cec0`.

Slot 52 (`0074d0c0`) also calls `flame_set_firing(1)` when a float at its argument +8 is > 50.0,
else `flame_set_firing(0)`. Its caller is the vtable and the meaning of the argument is [unread].
Read at face value this is a 50 m gate, which is doubtful for a 12 m weapon; treat as unknown.

## 4. The per-frame update: `FUN_0074c200` (slot 2) [game]

Numbered pseudo-code, dt = `param_2` (the game step; CLAUDE.md: 0.032 s).

1. `weapon_update_clock(dt)` `007a73c0` (the HUD clock, ledger).
2. Glow ramp: if `weapon+0x458 > 0`: `weapon+0x458 -= dt`, clamp to 0; a light value
   `2 * weapon+0x458` (or `1 - 2 * weapon+0x458` while firing) goes to `FUN_005560b0` [game,
   the disassembly at 0074c2f0-0074c31e quotes `movss [ebx+0x458]`, `mulss [0xb38838]` = 2.0,
   `movss [0xb38834]` = 1.0, `subss`].
3. If firing (`weapon+0x400`):
   a. `t = vtable+0xfc()` (slot 63, unread); the state advances by `t * dt` through
      `vtable+0x100` (slot 64, outside the table) [unread meaning].
   b. Stop test (3c above).
4. Dummy switch-on (only while firing, `weapon+0x400` set): `weapon+0x408 -= dt`; when it is
   <= 0 and the active index `weapon+0x404` < the number of dummies (the list is walked to count
   it, `linked_list_size` `005a0760`): take dummy #index, call `FUN_0074e9b0(d, 1)` (on), set
   `weapon+0x408 = d+0x2c` (the dummy's `delayTilTurningOnNextDummy`, 0.35 s in longhaul [data];
   the field offset +0x2c is the one the update reads [game]), and index += 1 [game].
   So the three dummies turn on one after another, 0.35 s apart (`FUN_0074e9b0` is the on/off;
   per-frame growth is `FUN_0074e730`, see section 6).
5. DAMAGE TICK (`weapon+0x43c`) [game, disasm 0074c484-0074c4cd]:
   a. `weapon+0x43c -= dt`. If it is < 0, reset it to `1 / attr->damageTicsPerSecond`
      (`1.0 / *(float*)(attr+0x134)`, the divss at 0074c4b3, attr from `weapon+0x3ec`). Longhaul:
      1/5 = 0.2 s per tick [data + game].
   b. Serial: `weapon+0x438 = DAT_00cf5cb8` (decompile line 55), then the owner ref from
      `FUN_0074ef50` (the object registered under the flame hash) gets `ref+0x438 = weapon+0x438`
      (line 60), then `DAT_00cf5cb8 += 1` [game, decompile; see the CONFLICT note in section 1].
   c. The tick continues only if `vtable+0xdc` returns an object whose word is 0 [game].
   d. The hit-count stat (`FUN_0088eb10`, `FUN_00897930`, `FUN_00897790`, `+0xc6` short += 1)
      is a multiplayer score counter (census: multiplayer). Not damage [game].
   e. THE DAMAGE AMOUNT IS NOT HERE. The update reads attr+0x134 only; there is no read of
      attr+0x130 (damagePerSecond) in `0074c200`-`0074cd1a` (disassembly search for `+0x130`
      found none). Per tick the implied value is 110 / 5 = 22 (longhaul) [assumed].
6. Hit test, every frame while firing and the 0.0 radius path (lines 211-279 of the decompile):
   a. Origin from `vtable+0x90` (slot 36) into `auStack_74`; direction from `FUN_0056bee0`
      into `auStack_80`.
   b. `world_ray_sphere_query` `005be890` with the arguments `(origin, dir, 3, &results, 0)`.
      The final float (radius) is 0, which takes the ray branch of that function
      (`if (param_6 == 0.0)`, `world_ray_sphere_query` lines 40-50) [game]. So the flame is a
      RAY, not a sphere, despite the name; the `3` is a shape mask / type [assumed].
      No reach or range argument is passed: the range is NOT found.
   c. Results are 0x24-byte records (stride `iVar7 * 0x24`); for each hit object with a nonzero
      word at +0x20 and not `character+0x134 & 0x1000` (the invulnerable flag, read as
      `test dword ptr [eax+0x134], 0x1000`, 0074cb1b [game]): spawn `TriggerableParticle`
      `FUN_00746dd0` at the hit frame (`align_z_matrix` `0056e7b0` from the hit normal), call
      `FUN_00562970` / `FUN_00557150` (accessors, no effect). The impact effect is
      `impactEffect` (weapon+0x3f4). Otherwise only the accessors run.
   d. If the flame has no hit, `FUN_0074b130` (AnimBranchPointEvent, `object_trigger_callbacks`)
      fires once when the byte at `weapon+0x41c` (`param_1[0x107]`) is clear [game].
7. Dummy list (`weapon+0x424` sentinel `+0x420`) and hit list (`+0x42c`): each entry has a float
   timer at entry+0x10 that counts down by dt; an entry with a null object or a timer <= 0 is
   removed (the `FUN_00534540` / pool path). The entry's use is [unread].
8. Final: `vtable+0x54(dt)` (slot 21, `weapon_heat_update` 007a9700, read) [game]. Heat and
   overheat are therefore advanced in the generic heat code (weapon-kinds.md data: 15 per round,
   33.3 per second, 4 s overheat; the per-round increment is applied in slot 24 [assumed]).

Note that the damage tick is not gated by the hit test: the update does not show which target
takes the 22 per tick. Candidate paths: (a) the three dummy physics volumes (section 6), (b) the
DoT or damage packets (`scripted_damage_packet` `00744670`, `dot_tick_packet` `0072aea0`), which
no flame function calls [unread].

## 5. What the tick and the timers do, in one list

- Firing starts when `vtable+0x94` says held and `vtable+0xdc` allows (3a); stops on the
  release test (3c). Automatic: `automaticFiring` true [data].
- Damage tick: every `1 / damageTicsPerSecond` = 0.2 s, first tick 1.0 s after start [game].
- Dummies: on in sequence 0.35 s apart; each has a physics body while on (`FUN_0074e8c0`).
- Heat: per-frame `weapon_heat_update` (slot 21). Per-round increment is in slot 24 [assumed].

## 6. The FlameThrowerDummy group [game]

- `FUN_0074e9b0(d, on)` (on/off): calls `FUN_0074e730` (update), `FUN_0074e8c0` (physics), the
  particle `particle_emitter_spawn_attached` `0057e820`, `FUN_00574920` [outline; full not read].
- `FUN_0074e8c0(d, on)`: on (1): if the flag `d+0x60` is 0, `FUN_00556050` and
  `physics_holder_register` `006dcaa0`, set `d+0x60 = 1`. Off (0): `FUN_00556050`,
  `FUN_0117bbb0`, `d+0x60 = 0`. So a dummy is a PHYSICS body while on [game; assumed that
  this is the damage volume].
- `FUN_0074e730(d, dt)` (per frame): takes the dummy basis at `d+0xc` (+0x10..+0x3c), normalises
  its axis, and scales it by `dt / (|axis| * d+0x64)` (`d+0x64` is the time to max scale,
  0.5 s in longhaul [data]; `minScale`/`maxScale` 1 to 1.25 / 1.35 [data]). This is the visual
  ball growth, not damage [game].
- `FUN_0074ef50()`: gets the flame object by hash 0xd11292ac through `FUN_005781b0` and adds a
  reference [game].

## 7. Data block (longhaul, `export\data\global_states.txt` 91368-91430) [data]

Copied in full from the dump (the values are the user's install data; only the values are kept):

```
LongHaul_FlameThrowerAttribute:
  numProjectilesPerFire: 1
  rateOfFire: 1
  automaticFiring: true
  burstPerFire: 1
  timeBetweenEachBurst: 0.01
  hasChargedShot: false
  timeTilChargeStarts: 0.5
  chargeDurationTilMaxEffect: 3
  chargeHoldDuration: -1
  fireOnlyFullyCharged: false
  minChargeDamageMultiplier: 1
  maxChargeDamageMultiplier: 2
  minHeatMeterIncrement: 5
  maxHeatMeterIncrement: 10
  chargeNumProjectilesMaxMultiplier: 1
  chargeHeatIncrementPerMultiplierIncrement: 10
  heatMeterCapacity: 100
  heatMeterIncrementPerRound: 15
  heatMeterDecrementPerSecond: 33.3333
  heatMeterCooldownTilDecrement: 0.2
  heatMeterOverHeatPeriod: 4
  heatMeterOverHeatCooldownRate: -1
  heatMeterOverHeatRateWhileCharging: 2
  rangedSwipeEnable: false
  rangedSwipeSpeed: 5
  rangedSwipeLockOn: false
  rangedSwipeHzAngleMin: 0
  rangedSwipeHzAngleMax: 0
  rangedSwipeVtAngleMin: 0
  rangedSwipeVtAngleMax: 0
  listenToWeaponAnimEvent: true
  isSpecial: false
  hasLaserSight: false
  snipeCameraEnabled: false
  useFacingAsTarget: false
  isVehicleModeWeapon: false
  weaponChildren:
    [1050274702]
  weaponParent: 0
  alternatingFire: false
  useFireDummyMatrix: false
  useOwnerRootMatrix: false
  fireDummyXYAngle: (empty)
  fireDummyZAngle: (empty)
  playerNoiseRadius: 0
  generalNoiseRadius: 0
  aimAssistMaxRange: 12
  rumbleFireDuration: 0.5
  rumbleFireStrength: 1
  damagePerSecond: 110
  damageTicsPerSecond: 5
  damageFlags:
    [STATE_CAN_GO_TO_GRABBED, #bits800]
  specialCaseDamageStuff:
    stunTime: 0
    slowTime: 0
    dotTime: 0
    heatIncrement: 30
    knockBackSpeed: 15
    knockBackAngle: 35
```

Other flame blocks: `DefaultFlameThrower` (83845): damagePerSecond 100, heat capacity 100
(weapon-kinds.md). `MP_LongHaul_FlameThrowerAttribute` (`global_states.txt` 97345) and
`LongHaul_AI_FlameThrowerAttribute` were not compared line by line.

## 8. Unread (what is missing for a port)

- THE DAMAGE AMOUNT: where attr+0x130 (damagePerSecond) becomes damage. No reader found in the
  flame update. Candidates: the dummy physics contacts (`FUN_0074e8c0`), the stream effect's own
  DamageLink objects (as the grenade's `DamageLink` in the data), `FUN_00744670` / `FUN_007449a0`.
- The cone and the reach: the ray has no range argument (section 4.6b). Range and any spread
  are NOT found. `aimAssistMaxRange` 12 is only the aim assist [data].
- The shape mask `3` and the output record (0x24 bytes) of `world_ray_sphere_query`.
- The fire step (slot 24 `007a9dd0`) and where the per-round heat increment is applied; the
  `FUN_0074cec0` (stream effect on/off) body; `FUN_0074af20`; `FUN_0074b130` in full.
- Slots 7, 8, 9, 10, 11, 13 to 16, 19, 22, 23, 25 to 47, 50, 51, 53, 55 to 63 (see the table):
  their bodies, and slot 64 (outside the table).
- The meaning of the float at argument +8 for slot 52 (`0074d0c0`).
- The hit list at weapon+0x42c: what entries are pushed (who calls the push) and what each timer
  does (a DoT or a per-target interval [assumed]).
- The `FlameThrowerDummy` full bodies (`FUN_0074e9b0`, `FUN_0074e730` beyond its scale) and the
  dummy field offsets other than +0x2c, +0x60, +0x64 (the others are from the data names).
- `FUN_00711af0` (name hash to object) and `FUN_00578440` (FireDummy index).
- The second FlameThrower in the dump (`work\survey\weapons\longhaul.txt` 623560) and the Left
  variant `LongHaul_FlameThrowerLeftAttribute` (91429).
- The MP and AI attribute values.
- Whether the damage tick is the only damage path (the dummies may deal damage on contact).
  ANSWERED in part by the second read below: the tick deals no damage; contact is the only candidate.

## Damage path (second read)

Ticket (2026-10-12, second reader): how the flame damages. Read: `0074c200` (lines 1-311 of the
decompile), `0074e9b0`, `0074e8c0`, `0074e730` (outline), `0074ec40` (full), `0074ef50`,
`0074e070`/`0074ee80` (outline), `009518f0`, `00725a40`, `0074b130`, `006dcaa0` (full), `00556050`,
`00554090` (full), `00553f40` (not read), `00744800`, `00744670` (full). Disassembly: not needed
for the points below except where marked. Ledger entries added for `0074ec40`, `00554090`,
`00744800`, `00556050`.

### A. What the flame update does with a target: nothing that is damage [game]

1. `0074c200` reads attr+0x134 only (`1.0 / *(float*)(param_1[0xfb]+0x134)`, decompile line 52).
   A search of its whole body (lines 1-311) finds NO read of attr+0x130 (damagePerSecond) and no
   call that takes an amount. The `+0x138` flags and the `+0x13c` special block are not read here.
2. The damage tick (weapon+0x43c, decompile lines 48-52) does, when it fires (every 0.2 s on
   longhaul, first one 1.0 s after start): (a) the tick serial `DAT_00cf5cb8` into weapon+0x438 and
   the owner's +0x438 (lines 55-62); (b) one of two branches on the owner `local_128` (the owner
   character ref copied by `009518f0` at lines 7-9): an MP stat branch when `DAT_00dc967c != 0`
   (lines 75-92), else a HUD send `send_ui_integer(0xa45a3365, 1)` plus a hit counter
   (owner +0xc6 short +1, lines 95-122) [game]. Neither branch applies health. The tick is a HUD
   and stats event, not damage.
3. The hit test (lines 209-296): origin `vtable+0x90` (slot 36) into `auStack_74`, direction
   `FUN_0056bee0` into `auStack_80`, then `world_ray_sphere_query 005be890(origin, dir, 3, &res, 0)`
   (line 246). The result stride is 0x24 (`iStack_10c + i*0x24`). The loop (lines 247-279) stops
   at the first hit whose object at `hit+0x20` is nonzero, is not invulnerable (`+0x134 & 0x1000`)
   and passes the `iStack_cc == 0` test; it then spawns the impact `TriggerableParticle`
   (`FUN_00746dd0`, line 266) and goes to the end. No damage call is made in this branch. The
   impact is cosmetic. Range: no length argument is passed (see section 8 of the first read).
4. Dummy switch-on (lines 127-152, inside the firing block): when the timer `+0x408` (weapon
   `param_1[0x102]`) runs out and index `+0x404` (`param_1[0x101]`) < count, the dummy at that index
   is fetched by `009518f0` (a ref copy) and switched on by `flame_dummy_on_off(d,1)`; the timer
   is set from `d+0x2c` (delayTilTurningOnNextDummy, 0.35 s). Timing [game]: dummy 0 on at the first
   firing update, dummy 1 at +0.35 s, dummy 2 at +0.70 s, then the index stops at the count (3).
   The chain runs only while firing (line 26 gate).
5. Hit-list pass (lines 153-208): entries at weapon+0x42c, each `[+0]` next, `[+4]` prev,
   `[+8]` a value that must be nonzero, `[+0xc]` a ref to an object, `[+0x10]` a float timer. An
   entry is removed when its object ref is dead, `[+8]` is 0, or the timer is <= 0 and nonzero;
   otherwise the timer drops by dt. WHAT PUSHES AN ENTRY IS NOT FOUND (no caller of the push was
   located). So the hit list is read-only here, and it is the only per-target list the flame has.
   [assumed] it is the per-target contact memory (a damage interval per victim), not written
   anywhere in the flame code read so far.

### B. The dummies are physics bodies with a sphere each [game + data]

6. `0074e9b0(d, 1)` (on): for each emitter in `d+0x3c` a particle is spawned (`0057e820`,
   `FUN_00574920`); `d+0x68 = d+0x38` (time to max scale, 0.5 s); if that is 0 at once, the update
   jumps to `d+0x34`; then `0074e730` (scale the visual basis, `d+0x30`..`d+0x34` over `d+0x38`) and
   `flame_dummy_physics(d,1)` (6b). `0074e9b0(d, 0)` (off): `0074e730`, `flame_dummy_physics(d,0)`.
7. `0074e8c0(d, on)`: on (1) and `d+0x60 == 0`: builds a 0x14-byte record with `*(short*)(d+0x14)`
   (the body id) and a flag byte set to 1, calls `FUN_00556050`, then `physics_holder_register
   006dcaa0`, and sets `d+0x60 = 1`. Off (0) and `d+0x60 != 0`: `FUN_00556050` with a zeroed record,
   `FUN_0117bbb0` (clears the body id, `unaff_ESI+0x250 = 0xffff`), `d+0x60 = 0` [game].
8. `006dcaa0` (full read): the body shape is `d+0x1d8`'s own shape (the vtable call pair at
   `+8` and `+0x10`, then `FUN_005ac830`), copied into `d+0x220..0x23c` (an 8-dword box);
   the body is then made by `FUN_005b3140(d+0x220, d, flags, preset-name, d+0xc, mask@+0x9c, mask@+0x9e)`
   and its id stored at `d+0x250`. So the dummy's physics volume is the Collision child in the
   parent `d+0x1d8` (6c below) [game].
9. `0074ec40` (full) loads only the dummy's own fields (listed in `0074ec40` line above and in
   section 6 of the first read); there is NO shape, radius or Collision field in the dummy's own
   data object [data: the `FlameThrowerDummy` blocks in `work\survey\weapons\longhaul.txt` lines
   591044, 591640, 592062 list only index, delayTilTurningOnNextDummy, minScale, maxScale,
   timeTilMaxScale, loopingParticles].

### C. The shapes: three spheres, one per dummy [data]

10. The sphere of each dummy is a SIBLING attachment of the dummy in the same `longhaul_weapons`
    attachment list (the dump's `- [N] index: K` entries, under the owner at dump line 555100):

| dummy (index) | attachment index it sits at | Collision sibling (dump line) | parent (attach idx) | sphere radius | matrix translation | maxScale |
|---|---|---|---|---|---|---|
| 0 | 5 (`- [4]` at 591044) | `- [5]` at ~591612 | 5 | 2.5 | (0, 0, 0) | 1.25 |
| 1 | 6 (`- [6]` at 591640) | `- [7]` at ~592032 | 6 | 3.0 | (0, -0.0292308, 0) | 1.35 |
| 2 | 7 (`- [8]` at 592062) | `- [9]` at ~592458 | 7 | 3.5 | (0, 0, 0) | not read |

    Each Collision: one `CollisionBatchPrimitive` with `matrix` (basis identity, z column
    `[1.6e-7, 0, 1]`), `material` 938940607 = `0 - Default`, `maskPreset` -211360833 =
    `Explosion` (names.tsv, f366e3bf), `sendMask` 0, `receiveMask` 2, a `CollisionSpherePacket`
    radius, `dynamics: None`, no links [data].
11. The preset `Explosion` in `export\data\global_collision.txt` lines 269-274: sendMask
    `AWAKE_STATIC_0x400`, receiveMask `PUNCH_0x2, DETECT_0x100` [data]. Per `docs/melee.md`
    lines 43-46 the preset wins over the primitive's own masks, so the EFFECTIVE masks of the
    dummy sphere are send 0x400 and receive 0x102 [assumed; the rule is from melee.md, not
    re-read]. The sphere therefore RECEIVES PUNCH-bit contacts (the characters' `Character` preset
    sends `PUNCH | WATER`, melee.md line 49-50).
12. Reach: the three spheres are at the dummies' basis, `d+0xc` (set from the attachment, not in the
    dump: the attachment entries carry only index and thing). Their positions along the barrel are
    NOT FOUND. The sphere radii 2.5 / 3.0 / 3.5 m and the visual scale (1 to 1.25/1.35 over 0.5 s)
    are all the reach data there is. The visual ball radii are 17.1 / 15.7 / 19.1 (ParticleTemplate
    `radius` for ball01..03 [data]); those are NOT physics.

### D. The damage itself: the physics contact hook (only candidate found) [game]

13. `00744800` (full read in the ledger) installs `scripted_damage_packet 00744670` as the global
    hook `DAT_00ce3a9c` (line 4 of its body). Its only caller is `00554090` (found by `work\find_ptr.py
    00ce3a9c`, at 005540bb and 0055410f).
14. `00554090` (full): on a body (`unaff_ESI`, which carries `+0x100 == DEBRIS_DIE_IMPACT` and so is
    a `lux::Debris`), if `+0x54 & 4 == 0`, `+0x60` float > 0, hook set, and `+0x50 == 0`: when
    `(other+0x9c) & (this+0x9e) & 2` (PUNCH) is nonzero it calls the hook with
    `(this+4, other+0xc, this+0x60 as amount, refcopy(this+0x70))`; then `+0x60 = 0`.
    `FUN_00556050` (a listener broadcast) is also called when `this+0x9e & 2` [game].
    THE DUMMY IS NOT SHOWN TO BE THIS BODY: the dummy's registration writes a flag byte at `d+0x60`
    and its body is made by `006dcaa0`, so a `lux::Debris` with a float at `+0x60` is a different
    object. [assumed] the contact damage of the flame goes through this hook, with `this` the flame
    sphere's body and `+0x60` its amount. NOT CONFIRMED: who writes `+0x60` (a float) to the body,
    and from which attribute (`attr+0x130`/`+0x134` or the `specialCaseDamageStuff`).
15. `scripted_damage_packet 00744670` (full read): builds a packet with `amount` = its 3rd
    argument, `flags` = 0x400 (`local_44`), a `local_2c = 0x8590b19d` tag, the source ref from the 4th
    argument, then `send_damage_message 00724700` [game]. The packet's damage flags for the flame's
    `damageFlags` (`STATE_CAN_GO_TO_GRABBED`, `#bits800`, attr+0x138) are NOT put in this packet.
    So the hook as read carries NO special block (stun, slow, dot, heat, knock-back).

### E. Pseudo-code of the damage path as far as it is read

1. Start: `flame_set_firing(1)`: tick timer 1.0 s, dummy index 0, timer 0, firing flag 1.
2. Each update while firing (`0074c200`): tick every 0.2 s (only serial and HUD/stat; no damage).
3. Each update: ray from slot 36 along `FUN_0056bee0`, `world_ray_sphere_query(origin, dir, 3, res, 0)`,
   first valid hit gets an impact particle (no damage) (A3).
4. Dummy chain: dummy k turned on at 0.35 k s (A4); its sphere (B10) joins the physics world
   (B7, `006dcaa0`), with masks send 0x400, receive 0x102 (C11).
5. A contact between the sphere and a body whose send mask has PUNCH (2) reaches the physics
   response `00554090` (D14). IF the sphere's body has `+0x60 > 0` and `+0x50 == 0`, the hook
   `scripted_damage_packet` is called with the amount `+0x60` and the victim ref `+0x70` (D15).
6. Nothing in the flame code sets that amount from `attr+0x130` (damagePerSecond) or
   `attr+0x134`. NOT FOUND: the writer of `+0x60`. Per-second damage would be 110 (longhaul,
   damagePerSecond) and per-tick 22 (110 / 5) IF the amount were per tick [assumed].
7. Heat per second if you meet it: NOT FOUND. The only heat number on the damage path is
   `specialCaseDamageStuff.heatIncrement` 30 (attr+0x13c, longhaul [data]), and no read applies it.
   The per-round heat (`heatMeterIncrementPerRound` 15, `heatMeterDecrementPerSecond` 33.3 [data])
   is the generic heat code (weapon-kinds.md), not damage.

### F. Data for a port (longhaul) [data]

- `damagePerSecond` 110, `damageTicsPerSecond` 5, `damageFlags` `[STATE_CAN_GO_TO_GRABBED, #bits800]`,
  `specialCaseDamageStuff` stun 0, slow 0, dot 0, heat 30, knockBackSpeed 15, knockBackAngle 35
  (`export\data\global_states.txt` 91368-91430; the first read's section 7).
- Dummy spheres: radii 2.5, 3.0, 3.5 m; offsets (0,0,0), (0,-0.0292,0), (0,0,0); masks Explosion
  (send AWAKE_STATIC, receive PUNCH | DETECT); on at 0, 0.35, 0.70 s after the start of fire.
- Dummy visual: `minScale` 1, `maxScale` 1.25 / 1.35 (dummies 0 / 1), `timeTilMaxScale` 0.5.
- Ray: origin slot 36, mask 3, radius 0 (a ray); no length; first hit only.
- The DefaultFlameThrower block (global_states.txt 83845) has damagePerSecond 100 and heat
  capacity 100 (weapon-kinds.md); its dummies and spheres were not compared.

### G. Not found (for the report)

- The writer of the hook amount `+0x60` of the flame's sphere body, and the attribute it reads.
- The push of entries into the hit list at weapon+0x42c.
- The cone (spread) and the reach (range). The sphere positions along the barrel (the attachment
  matrices of the dummy list are not in the dump).
- Who applies `damageFlags` (attr+0x138) and `specialCaseDamageStuff` (attr+0x13c) on a flame hit.
- The contact callback of a dummy's body (the dummy's vtable slot for contacts) and the body
  class's +0x60 writer. `00553f40` (the `DEBRIS_DIE_IMPACT` branch of `00554090`) not read.
- The heat cost per second of firing on a hit (see E7).
- The second FlameThrower (dump 623560, the Left variant) and the MP and AI attributes.
