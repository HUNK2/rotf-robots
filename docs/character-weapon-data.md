# Character weapon data: what each pack lists (survey, 2026-10-12)

Scope: the weapon lists, the weapon events and the melee weapon of packs bumblebee (reference), ironhide, ratchet, optimus, longhaul, blackout, starscream, dcomscout. Read from each pack's chunk 0 with `tools\tf2_lxb.py dump` (dumps in `work\survey\weapons\<pack>.txt`, Bumblebee's from `export\data\bumblebee.txt`). Line numbers below are dump lines of the file named in the row.

Tags: `[data]` read from the pack dump; `[assumed]` a guess; `[game]` read from the port's code.

## Point 1: where the weapons are, and their types

The `Character` thing's `weapons` field (a list, dump indent 6 `      weapons:`) holds one entry per weapon list. Each entry is an embedded data object `<pack>_<list>` with `obj` (an `Object`: `sub_matrices`, `hierarchy`, `attachments`). The weapons are the `thing`s in `obj.attachments` (`index`, `thing`). `core\src\weapons.rs` `read_defs` takes only `GunWeapon` and `MissileLauncher` (lines 95-98); every other type is skipped (`_ => continue`).

| pack | weapon lists (dump line) | weapon things (type, weaponName hash, object name) | read by the loader (`GunWeapon`, `MissileLauncher`) | skipped types |
|---|---|---|---|---|
| bumblebee | `bumblebee_weapons`, `bumblebee_vehicleweapons`, `bumblebee_meleeweapon` | GunWeapon (SolarPlasmaCannon, `solarplasmacannon`), MissileLauncher (ForwardLauncher, `micromissilescluster`), GunWeapon in vehicle list (`vehicleautobot`), MeleeWeapon | Gun 2, Missile 1 | none [data] |
| ironhide | `ironhide_weapons`, `ironhide_lefthandweapons`, `ironhide_grenadevehicleweapons`, `ironhide_vehiclespecial` | GunWeapon (1864782327, `dualpulselasercannons`, dump 648458), GrenadeLauncher (2128319374, `ironhidedirectgrenade`), GunWeapon (in left-hand list, dump 657792), MissileLauncher (dump 659653), TurretLauncher (`ironhide_turret`, in left-hand list and in `ironhide_vehiclespecial`), GrenadeLauncher (vehicle list, `ironhide_vehicleweapons`) | Gun 2, Missile 1 | Grenade 3, Turret 2 [data] |
| ratchet | `ratchet_weapons`, `ratchet_stickyvehicleweapons`, `ratchet_meleeweapons` | GunWeapon (1306769318, `energonbeammultibarrelgun`), GrenadeLauncher (384165122, `ratchetgrenade`), GrenadeLauncher in sticky list (-585260508), MeleeWeapon (dump 559105) | Gun 1 | Grenade 2 [data] |
| optimus | `optimus_weapons`, `optimus_lefthandweapons`, `optimus_vehicleweapons`, `optimus_meleeweapon`, `optimus_meleeweapon_lefthand` | GunWeapon x5 (`laserrifle`, `shellcannonbullet04`, others; dump 701135, 706631, 754286, 755007, 757236), MeleeWeapon x2 (dump 764641, 766722) | Gun 5 | none [data] |
| longhaul | `longhaul_weapons` (555100), `longhaul_leftweapons` (593294), `longhaul_vehicleweapons` (627894) | GrenadeLauncher (dump 583858 with `longhaulgrenade`; 622041; 627965), FlameThrower (dump 588893; 623560). NO GunWeapon, NO MissileLauncher, NO MeleeWeapon | 0 | Grenade 3, Flame 2 [data] |
| blackout | `blackout_weapons`, `blackout_vehiclemissileweapons`, `blackout_meleeweapon`, `blackout_vehiclespecial` | GunWeapon (235988636, `blackoutgatling`), MissileLauncher x2 (`energoncannon`; `flyingvehicle_homingmissiles`), TurretLauncher x2 (`blackout_turret`), MeleeWeapon | Gun 2, Missile 2 | Turret 2 [data] |
| starscream | `starscream_weapons`, `starscream_weapons_vehicle`, `starscream_meleeweapon` | GunWeapon x1 (-1197494719, objects `gatlinggun` and `gatlinggunspecial`), MissileLauncher x2 (`nullray`, `homingmissiles`), MissileLauncher in vehicle list (627156693), MeleeWeapon (+2 RibbonTrail) | Gun 1, Missile 2 | none [data] |
| dcomscout | `dcomscout_weapons`, `dcomscout_vehicleweapons` | GunWeapon (102814779, `machinegun`), GrenadeLauncher (-574132726, `grenadelauncher`), GunWeapon in vehicle list (86518966, `gatlinggun`) | Gun 2 | Grenade (count 3 by grep, per list not checked) [data] |

Each list entry has its own `obj` (rendered parts; for example `dualpulselasercannons.dualpulselasercannons` is in the ironhide pack's mesh list). The weapon's object is therefore present for every list. Whether a weapon's object shows is governed by events, see point 2 and by `alwaysAppear`.

`isVehicleModeWeapon`: 0 occurrences in `longhaul.txt` [data]. The loader's car/robot split (`game\src\weapons.rs` 236-244) relies on it.

## Point 2: events that show or hide a weapon

Event counts per pack, whole dump [data]:

| pack | WeaponAttachmentEvent | WeaponEnableFireEvent | WeaponFireSpecialEvent | AlternateModelEvent |
|---|---|---|---|---|
| bumblebee | 15 | 6 | 0 | 25 |
| ironhide | **0** | 6 | 3 | 25 |
| ratchet | 15 | 4 | 0 | 25 |
| optimus | 35 | 9 | 0 | 26 |
| longhaul | 12 | 6 | 2 | 25 |
| blackout | 10 | 8 | 3 | 20 |
| starscream | 15 | 4 | 1 | 28 |
| dcomscout | 0 | 0 | 0 | 13 |

Fields of `WeaponAttachmentEvent` [data] (text export labels the flag fields wrongly; raw masks checked with `work\flags_raw.py`: `hide` enum 0, `unhide` enum 1 in ratchet, 15 each): `weaponID` (name hash, or 0 with `weaponIndex` = place in the manager's list), `hide`, `unhide`, `animID` (clip to start on the object), `hideOnAnimDone`, `animToPlayOnAnimDoneID`, `useEventOnlyIfWeaponIsCurrentlyUsed`. Node: the event's `index` goes through the stream's `keyIndexMap` to the clip's `names` list (as `read_clip` in `core\src\formats\anim.rs` 626-660 does).

Where they sit: all in the animation sets of the robot, in clips of the arm and of the melee (`Idle`, `Trans_R2W`, `Trans_W2R`, `FireA`, `FireB`, `SwitchToWeaponA`, `SwitchToWeaponB`, `Trans_R2W_WeaponA`, `Trans_R2W_WeaponB`, `Idle_WeaponA`, `Idle_WeaponB`). Clip names for each event were NOT resolved per clip (only the node); the events are listed by dump line in the scratch output `work\survey\weapons\events.py`.

Ironhide specifically:
- No `WeaponAttachmentEvent` at all [data]. The only weapon events are `WeaponEnableFireEvent` (dump 644540, 644568, 648314, 648342, 842143, 842171) and `WeaponFireSpecialEvent` (dump 635889, 641630, 805728).
- The `WeaponFireSpecialEvent` clips are followed at tick 26 by an `AnimSpawnEffectEvent` of `w_muzzle_flash_turret` [data]. It is a turret-muzzle trigger, not a show.
- `AlternateModelEvent` (25) are the robot-to-vehicle form change (`Trans_R2V_B`, `Trans_V2R_B`, `Trans_V2R_GroundPunch`), not a gun swap [data].
- The robot's own body has `Weapon_L`, `Weapon_R`, `Weapon_L_Body`, `Weapon_R_Body`, `Weapon_R_Barrell`, `Weapon_R_Bracket` and the meshes `ironhide.weapon_l`, `ironhide.weapon_r` (and `_body`, `_barrell`, `_rearsupport`) in the robot object [data]. The guns may be part of the body in robot form [assumed].
- `alwaysAppear` is false for the GunWeapon [data].

Melee events (MeleeWeapon as `weaponID`) per pack [data]:
- bumblebee: 5 distinct melee-attach lines listed (dump 364233, 364280, 368374, 368532, 373081); node `ObjectAttachment_Hand_L` (keyIndexMap channel 15) in the two checked (`clipnodes.py`).
- ratchet: 7 distinct melee-attach lines listed (dump 395556, 395680, 401357, 401458, 406810, 410604, 424657); node `WeaponAttachment_Hand_R` in every one checked (`clipnodes.py`).
- ironhide, longhaul, dcomscout: no melee weapon, so no melee events.

## Point 3: the melee weapon (the axe)

`core\src\weapons.rs` `read_melee` (186-192) takes the first list entry whose thing is `MeleeWeapon`, and reads only `weaponName`, `exitAnimation`, the list's `obj` and its `AnimRefBundle`. It does NOT read `startBoneAttachment` [game].

| pack | MeleeWeapon list | weaponName | startBoneAttachment | object | clips in bundle | attach events |
|---|---|---|---|---|---|---|
| bumblebee | `bumblebee_meleeweapon` (563248) | 1696241262 (MeleeWeapon) | ObjectAttachment_Hand_R | hierarchy 12 nodes, RibbonTrail (links to Weapon_Blade_R_01 etc.), 11 RenderThings | Idle 2081823275 (20), Trans_R2W 1655067729 (20), Trans_W2R 336028213 (20) | 5 distinct lines listed |
| ratchet | `ratchet_meleeweapons` (554946) | 1696241262 | **WeaponAttachment_Hand_R** (-1819335853) | hierarchy 17 nodes, 16 RenderThings (`Ratchet_MeleeWeapons.Base`, `.Base_Tip`, `.BladeBase_01/02`, `.SawBlade_1..12`), no RibbonTrail | Idle 2081823275 (24 long), Trans_R2W 1655067729 (48), Trans_W2R 336028213 (48) | 7 distinct lines listed, node WeaponAttachment_Hand_R |
| optimus | `optimus_meleeweapon` (762860) and `optimus_meleeweapon_lefthand` (766722 MeleeWeapon) | not checked | not checked | not checked | not checked | 35 WeaponAttachment events in pack (melee subset not counted) |
| starscream | `starscream_meleeweapon` (644874 area) | 1696241262 | not checked | not checked | not checked | 15 WeaponAttachment in pack |
| blackout | `blackout_meleeweapon` | 1696241262 | not checked | not checked | not checked | 10 WeaponAttachment in pack |
| ironhide, longhaul, dcomscout | none | - | - | - | - | - |

Checks on ratchet's axe [data]:
- Its mesh names are in the pack (`render_list.py`: `ratchet_meleeweapons.base`, `.base_tip`, `.bladebase_01`, `.sawblade_1`... `.sawblade_12`; 64 MESH chunks counting LODs), so `Library` should find them.
- The robot's own hierarchy has `WeaponAttachment_Hand_R` (`938f2353`) in ratchet (60 nodes), where Bumblebee's robot has `WeaponAttachment_Arm_R` and the events name `ObjectAttachment_Hand_L`. So the axe's hang node (`show_axe`, `game\src\weapons.rs` 905-931) should be found in ratchet.
- The hierarchy parents and names lists agree in length (17 and 17), so `scene::read` (`core\src\formats\scene.rs` 220-245) should return an object.

Conclusion for point 3: nothing in the data stops the ratchet axe object from being read, found or hung. The one data difference the loader ignores is `startBoneAttachment` (ratchet `WeaponAttachment_Hand_R`, Bumblebee `ObjectAttachment_Hand_R`). Cause of the missing draw NOT found in the data. [assumed] it lies in the runtime (pose, visibility or material), which needs a capture to settle.

## Point 4: longhaul ("his pack lists none for the robot")

Longhaul has no `GunWeapon`, no `MissileLauncher` and no `MeleeWeapon` [data]. Its weapons are `GrenadeLauncher` (5 in total across three lists, with `grenadeLauncherAttribute`, `grenadeLauncherMPAttribute`, `grenadeLauncherAIAttribute`, and a `grenadeLauncherClusterAttribute` inside, dump 585437-586261) and `FlameThrower` (2, dump 588893 and 623560, with `minFireDummyIndex` 0). The loader's type filter (`weapons.rs` 96-98, and again at 141, 156 and `character.rs` 139) accepts only `GunWeapon` and `MissileLauncher`. So `read_defs` returns nothing, `Arsenal::new` gets an empty list, and the game prints `weapons: his pack lists none for the robot` (`game\src\weapons.rs` 244). The "all vehicle-mode" idea does not apply: no `isVehicleModeWeapon` anywhere in longhaul [data].

## What the loader should do differently

A. Ironhide: the gun is never shown (`gun.shown` stays false).
- Cause [data]: no `WeaponAttachmentEvent` in any Ironhide clip (0 of 0). `core\src\gun.rs` `Gun::arm_events` (330-349) and `take_aim` (357-369) show the gun ONLY from such an event, or from the car change (`from_car`). So from the ground the arm clip never shows it.
- Not the loader's fault: the GunWeapon is read (`weapons.rs` 96, 141) and its object is built (`game\src\main.rs` 401-407). The gap is the show rule.
- Fix needed: find what shows the gun in Ironhide's original (candidates: the body's own `Weapon_L/R` meshes stay visible and the dualpulse object stays hidden; or `WeaponFireSpecialEvent`, which `core\src\formats\anim.rs` `read_weapon_events` (494-512) ignores via `_ => continue`) [assumed]. Field names of `WeaponFireSpecialEvent` are NOT decoded (the unpublished notes line 195).

B. Ratchet: the axe is out but not drawn.
- Cause NOT found in the data. The loader's `read_melee` (`weapons.rs` 186-192) reads only `weaponName`, `exitAnimation`, `obj` and the bundle, and ignores `startBoneAttachment` (ratchet `WeaponAttachment_Hand_R`). Changing it to the weapon's start bone is a candidate [assumed], as the guns do (`weapons.rs` 116 `bone: startBoneAttachment`). But `show_axe` hangs the axe on the event's node, not on this field (`game\src\weapons.rs` 905-931), so the candidate may not matter.
- Next check: a capture or a Rust test on the ratchet pack with the axe's `shown` and its `hung` node printed each frame.

C. Longhaul: no robot weapons found.
- Cause [data]: longhaul's weapons are `GrenadeLauncher` and `FlameThrower`. `core\src\weapons.rs` `read_defs` (95-99) knows only `GunWeapon` (`gunAttribute`) and `MissileLauncher` (`missileLauncherAttribute`), and `read_objects` (141), `read_object_clips` (156) and `character.rs` `read_weapon_effects` (139) use the same filter.
- Fix: add the kinds `GrenadeLauncher` (attribute `grenadeLauncherAttribute`; the MP and AI variants exist too) and `FlameThrower` (attribute name NOT checked: likely `flameThrowerAttribute` [assumed]). `core\src\weapons.rs` `Kind` (only `Gun` and `Launcher` today) needs the new variants, and `tuning_from` (289-349) the attribute reads. Their firing is outside this note.
- Also decide: `isVehicleModeWeapon` is absent, so every longhaul weapon is a robot weapon [data].

## Unfinished

- Clip names of each weapon event (only nodes resolved).
- `WeaponFireSpecialEvent` fields; FlameThrower attribute field name; optimus, starscream and blackout melee nodes and startBoneAttachment.
- Ratchet axe cause: runtime check not done (no build or run allowed).
- Scratch tools: `work\survey\weapons\tree.py`, `events.py`, `clipnodes.py` (outline, event list, node resolve).

## Paired weapons, one model per list, the switch (2026-10-19)

The user saw Optimus with one gun where he has two. Read from the game and ported the same day.

- [data] A left-hand weapon is a CHILD: its attribute block names `weaponParent`, its parent's names it in
  `weaponChildren`, and both carry `alternatingFire`. Pairs in the roster: Optimus laser rifles
  (alternating) and shell cannons (together); Ironhide laser cannons and direct grenades (alternating);
  Long Haul flamethrowers and grenade launchers (together). None in Bumblebee, Ratchet, Megatron (all
  three packs), Starscream, Jetfire, Breakaway, Grindor, Sideways, Powered-Up Optimus. Enemies: AProtScout,
  DConstWarrior, Wheelbot.
- [game] `007b0f10` links a weapon with a parent to the weapon whose children name it (one child slot,
  `+0x394`, `007ab060`). The switches `007b3ea0` / `007b4080` skip a weapon with a parent AND a weapon
  whose attributes have `isSpecial` (`+0x5d`): the special's weapons (Megatron's beam, Jetfire's dump,
  the Audi's mines...) are never in hand.
- [game] Fire `007ae010` (007ae144..007ae2dd, 007af0ee..007af120): with a child and `alternatingFire`
  false, the child's fire (`+0xcc`) runs first, then the parent's; with it true, a turn counter `+0x39c`
  gives the firing to the parent (0) or the child (1). On the child's turn the parent makes no rounds
  but its rate clock and heat are still spent. The child's own fire adds the child's heat and is not
  tested against it; the child never charges (`007ad1c0`). Flamethrowers: the parent's held slot
  `0074cd20` calls the child's held slot, so both burn.
- [game] Show / hide (`007a7630` / `007a7750`) and the object's clip starts (`007ab0f0`...) also run on
  the partner: the child's object is out with its parent's and plays the same clip ids.
- [data] Each weapon LIST has one object, shared by the weapons in it (Optimus's right rifle and right
  cannon are one model in two poses, `Idle_WeaponA` / `Idle_WeaponB`, as Bumblebee's cannon and
  launcher). It hangs on the node of the clips' `WeaponAttachmentEvent`s for its weapons (by
  `weaponIndex`): Optimus `ObjectAttachment_Hand_R` (0, 1) and `_Hand_L` (2, 3); Bumblebee
  `WeaponAttachment_Arm_R`.
- In the port: `WeaponTuning::{children, parent, alternating_fire, is_special}`, `WeaponDef::list`;
  `game\src\weapons.rs` `link_children`, `Arsenal::in_hand` (switch and HUD), `partner_mount` (the
  child's muzzle, `player.rs`), the firing loop's shooters; one `WeaponModel` per list (`main.rs`),
  hung by `hang_node`, drawn when its list holds the weapon in hand or its child (`show_models`, which
  drew only the first weapon's model before for every character).
- Checked: test `a_left_hand_weapon_is_the_child_of_the_right`; captures `work\shots\optimus_dual.png`
  (both rifles, the left one's flash), `optimus_cannon.png` (after the switch, 2 of 2, two rounds a
  firing), `longhaul_dual.png` (two grenades a firing; his left arm not seen in the picture),
  `bb_gun_check.png` (Bumblebee unchanged). Not compared with the original.
- [assumed] The manager's slot order is `data.weapons`' order (the events' `weaponIndex`). The HUD's two
  icons are the first two weapons that can be in hand. A character whose every robot weapon is special
  keeps the first in hand.
- The roster audit behind this (four read-only agents, 2026-10-19): `work\weapon-audit\group-{a,b,c,d}.md`.
  Still open from it (bursts DONE 2026-10-19, "Bursts on held and car weapons" below): `burstPerFire` / `timeBetweenEachBurst` were not played on held or car weapons
  (Ironhide's laser 2 bursts of 5, Breakaway's minigun 5, Starscream's homing missile 3, Grindor's car
  gun 4); Jetfire's 8-dummy specials fire from 3.

## Bursts on held and car weapons (2026-10-19; core tested, game built, captured, not compared)

Read [game]: `weapon_fire` 007ae010 counts every firing in the weapon's burst: the word `0xfd`
(byte `+0x3f4`) goes up by one, and back to 0 with the clock `0xfe` (`+0x3f8`) once it reaches
`burstPerFire` (attributes `+0x1c`, compared signed); then `timeBetweenEachBurst` (`+0x20`) is taken
off the clock, kept at 0 or more (007aefc4 for the weapon's own firing; 007ae21d..007ae264 for the
alternating child's turn, so the CHILD's firing advances the PARENT's count). Each firing also zeroes
the time since the last round (`0xfb`) and adds a round's heat (slot `+0x100` = `weapon_add_heat`
007a90c0). The weapon update 007ac370 (l.38-84, before the charge and the clocks): while `0xfd` > 0 the
update goes on `0xfe`; once `+0x20 <= 0xfe` the weapon fires itself through slot `+0xcc` = 007ae010
with the "forced" argument 1 (no rate or heat test, no trigger needed). It goes on when `isSpecial`;
for an `isVehicleModeWeapon` while the owner is in the drive or fly mode (0070ab90); for any other
while the weapon's byte `+0x38` is set; else `0xfd = 0`, the rest dropped.

Byte `+0x38` [game]: "weapons out". Set by the weapon's slot `+0x1c` (007a75a0, vtable 00b5dae8;
forwards to the child), which the manager calls with 1 for all 8 weapons as the weapon or dodge state
starts (007b2b40 from 0087f4a0 / 0087a960) and with 0 as he leaves them for another state (007b2eb0
from 0087f6f0 / 0087abc0). So a robot's burst runs on after the trigger is let go and after a weapon
switch, and stops when he leaves weapon mode.

Ported: `core\src\weapons.rs` `WeaponTuning::burst_per_fire` / `time_between_bursts` (`tuning_from`),
`Weapon::update`'s burst branch, `count_burst`, `firing`, `set_burst_context`, `Fired::then`;
`game\src\weapons.rs` `Arsenal::tick` sets the context each tick (weapons out = `aiming`, the gun out
in StateWeapon / StateDash; vehicle motion = the car's control mode). The burst's firings go through
the same per-firing loop as the trigger's, so Ironhide's second burst is the left cannon's turn.
Readout: `bursts of N x M rounds ... (k of this one fired)`.

Data [data]: Ironhide_LaserCannonAttribute 2 x 0.12 s (5 rounds a firing; its child ...Attribute2 1),
Firestorm_BurstMiniGunAttribute 5 x 0.05, Starscream_MissileAttribute 3 x 0.16,
Blackout_VehicleMissileAttribute 4 x 0.2 (vehicle mode). Test `the_burst_weapons_read_their_counts_and_gaps`.

Checked: tests `a_press_lets_go_its_whole_burst` (Ironhide: firings on updates 0 and 3),
`a_burst_comes_on_whole_updates` (minigun: 0, 1, 3, 4, 6), `a_burst_stops_when_it_may_not_go_on`,
`a_held_trigger_and_its_burst_share_the_count`, `a_burst_heats_the_weapon_and_ignores_the_heat`;
capture `work\shots\ironhide_burst.png` (`--character ironhide --aim-from 0.5 --fire-from 1.6 --after
2.8`): 10 rounds fired for one press (5 before). Not compared with the original.

Assumptions [assumed]: the owner is a player's (an owner with a kind-2 controller does not count, the
`00711260` test); the owner's `+0x148` bit 3 is clear; a burst firing at the capacity restarts the
overheat period as a trigger's does (same 007a90c0); a charged firing does not count in a burst (its
path 007af210 not read for this); a child never bursts by itself (its own count is only advanced by
its own firing, which the port does not run through `Weapon::update`; no child in the data read has
`burstPerFire` > 1); weapons out = the port's `aiming` (a dodge started from outside the weapon state
does not take them out, unlike 0087a960's call, which is not read for its condition); the trigger's
firing comes before the burst branch in an update (as before).

## Round 2 audit: what the port does with the weapons (2026-10-19; core tested, game built, captured, not compared)

Four read-only agents audited firing, effects and sounds, models and animation, and car / special /
aim / HUD across the 13 robots: `work\weapon-audit\BRIEF-2.md`, reports `r2-fire.md`, `r2-fx.md`,
`r2-models.md`, `r2-vehicle.md`. Their findings were checked in the code before use.

Fixed this session:

1. Car and jet weapons fired every kind as an instant bullet (`Arsenal::tick`'s car loop called `shoot`
   with no kind test), so the launcher and grenade car weapons of Ironhide, Ratchet, Long Haul,
   Starscream, Breakaway, Grindor and Jetfire did nothing (their `bullet_range` and `damage` are 0). Now
   a car Launcher launches a `Missile` and a Grenade a `Grenade` from the barrel along the car, kept in
   the same flight lists under slot `CAR_SLOT + i` (`weapon_in` takes the car list). [game 007ae010,
   the same projectile makers] [assumed: launched along the car's forward; the lock is the manager's,
   armed only in weapon mode, see open 3]. Capture `work\shots\ironhide_car_fire.png`: a grenade shell in
   flight ahead of the car.
2. A charged shot did tap damage: `Weapon::charge_damage` (1 to 2.5) was computed and `shoot` passed 1.0.
   It now passes the firing weapon's `charge_damage` (Optimus's shell cannons, OptimusFlight's cannon,
   Megatron's guns with a charge block). A child never charges (1.0).
3. Ground waves (`SnakerProjectile`, `weapons::Snaker`, ported with tests but never used): Grindor's and
   Jetfire's robot-form energon cannons carry one in their projectile and no Explosion, so they did no
   damage. Exactly the roster's launchers with a `snakerInfo` speed (Blackout 0x0f135ad6 60, Jetfire
   0xc84ec6ea 100) now let one go where the missile ends, along its way; it slides and strikes each target
   it reaches once with `snakerInfo.damage` and its flags. [assumed: starts at the missile's end;
   `checkGroundHeight` 1 (the ctor's); `SNAKER_REACH` 1 m for its collision; once per target]
   [stand-in] nothing is drawn or heard for it.
4. Weapon show / hide events addressed by slot (weaponID 0, weaponIndex n; 11 of 13 packs, all with
   `useEventOnlyIfWeaponIsCurrentlyUsed`) never matched (`gun.rs` compared the name only). Read
   007b4680 [game]: slot i matches by name, or with no name by `weaponIndex == i`; "only if in use" passes
   the slot in hand, a slot that is never in hand (the melee weapon), or a slot whose parent (`+0xdc`) is
   the weapon in hand. Ported as `gun::Hand` (name, slot, child's name and slot; `Arsenal::hand_of`), and
   the pull-out now runs its tick-0 events with the hand. Before, the gun appeared only at the first shot
   (the recoil's tick-0 events matched every slot) and the object played the last event's clip, A or B,
   whichever weapon was in hand. A child's event does not move the shared node. Test
   `an_event_by_slot_counts_for_the_slot_in_hand_and_its_child`; capture `work\shots\optimus_pullout.png`:
   both guns out, no round fired.

Round 3 (2026-10-19, agent team): items 6-12 below are DONE (second melee weapon and blade boxes; car-form
special muzzles on the car; charge clips and the full-charge round; Bumblebee's cannon muzzle from its model;
kept fire loops, ring-offs, idle sounds, heat parameter; projectile ribbons, emitters, lights, blast rumble;
flame contact damage found (0074d610), `listenToWeaponAnimEvent` read, head shots from posed bodies). See
the unpublished notes "Weapons round 3" and `work\weapon-audit\r3-*-done.md`. Correction to r2 M2: Optimus-flight
HAS WeaponAttachment_Hand_L/_R (74/75); only Optimus lacks them.

Open (checked or plausible, not fixed; in order of what a player notices):

1. DONE 2026-10-19: held guns spawn their round's own trail emitter (every gun row that has one; Optimus's
   rifles have none in data and draw nothing). The gizmo line is a `TF_TRACERS=1` debug aid only.
   Capture `work\shots\optimus_held_trail.png` (shell cannon: 2 rounds, 2 trail emitters started).
2. DONE 2026-10-19: sniping ported (`docs/weapon-mode.md` "Sniping", `core\src\snipe.rs`): R3 in weapon
   mode, the sniper camera and its zoom, the sniping aim, the snipe clock and damage ramp, the round from
   the camera. Capture `work\shots\audi_snipe.png` (5 degrees; one round at full steady, x2.0, took a
   scout 225 -> 25). Not drawn: `SniperScope_Gfx` and the reticle's sniper base / icon.
3. DONE 2026-10-19: the lock runs in the car for a guided car launcher (`lockon::armed_in`; checked
   007b5030 / 007b43b0: in the car the weapon in hand is the car's) and keeps across the whole view
   (`lockon::keep_share`). Capture `work\shots\grindor_car_lock.png` (scout 1 locked, missiles on it).
4. DONE 2026-10-19: Megatron's fusion-arm special starts its blast emitter where the beam lands, and its
   gun model comes out on his right hand by the SpecialAnimSet events (`Arsenal::special_objects`,
   `show_special_models`; the unpublished notes top entry). Captures `work\shots\megatron_special_side.png`,
   `megatron_special_3.4.png`.
5. DONE 2026-10-19: Grindor's deployed turret fires with its gun's own fire sound, muzzle flash and impacts
   (`character::turret_gun_key` row read from `deployableTurretObj`), and the throw shows the launcher's
   flash (the unpublished notes top entry). Flame rows are still not read here.
6. The second melee weapon `MeleeWeapon_L` is never loaded (Optimus's AttackFast_1 has only left-blade
   events: no blade on the first swing; `r2-models` 3).
7. Car-form specials take their muzzle from the robot's nodes (`player.rs` ~551), so they leave from the
   hand, not the car (`r2-vehicle` 5).
8. `chargingAnimation` / `chargedAnimation` (WeaponB_Charging / _Charged) never play (007b03c0;
   `r2-models` 5). A full charge never takes `chargeMaxProjectileObj` (OptimusFlight's charged snaker;
   `r2-fire` 5).
9. Bumblebee's cannon muzzle is on ObjectAttachment_Hand_R but its model hangs on WeaponAttachment_Arm_R
   (`r2-models` H1).
10. Loops other than the flame are never stopped (Blackout's `WPN_BLAC_GAT_FIRE_LP`, the Audi's car gun
    loop: not confirmed as loops in the .fev); `fireRingOffSound`, `weaponIdleSound`, the `heat` sound
    parameter are unused (`r2-fx` 5, 8).
11. Projectile ribbon trails, extra trail emitters, blast and flight lights, the blast rumble (`r2-fx` 6, 7).
12. Flame damage per tick and burn time remain assumed (the contact-hook damage writer was not found).
    `listenToWeaponAnimEvent` (true on every held weapon) is not read. Headshot multipliers cannot fire on
    the cylinder targets.

Settled (no gap): `bulletDamageFallOffRange` is -1 in every roster gun; `tracerFirePerRound` >= 1
everywhere; Jetfire's 8 FireDummies sit within 1 mm (the 3-barrel cap is harmless); the unused
`Starscream_VehicleGunAttribute` / `MegatronFlight_VehicleGunAttribute` (and Ironhide's, Ratchet's,
Firestorm's, Blackout's, OptimusFlight's VehicleGun blocks) are referenced by nothing in the install;
every robot's weapon plays its own effects and sounds (none borrows Bumblebee's); the overheat sound is
played (the to-do "flame / charge overheat sound" is stale); every list's hang node is right, left hands
included (Long Haul's left object hangs on WeaponAttachment_Arm_L). Car, flight and Ironhide's robot weapon
objects are 944-byte placeholder meshes in the packs: nothing to draw there.
