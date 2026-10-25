# Melee: targets and hits (research notes, 2026-10-03)

Addresses are for the analysed `Transformers2.exe` and stay in these notes. The graph that
decides WHEN he attacks, and the attack mode's stick turn, are in `state-machine.md`.

Status: the hit's whole path, the attack mode's target turn and slide, the target search,
which of his bodies hit and when, the damage scalings, and the ground punch's blast (an
`Explosion`), the damage type flags and the hit's effect and sound (`FUN_0072d9c0`) are
read. NOT read: the target's damage routine past an outline (`FUN_00719830`), how the
physics finds a contact.

## How a hit happens in the original

A hit is a physics contact, not a query.

1. `AttackCollEvent` (field `attack`, handler `FUN_00747e60`) sends two messages:
   - `43990685` on the character, with the flag. A `CollisionDamageObject` (`enabled`,
     `damagePoints`, `damageType`) listens and switches itself on and off (its handler is
     the stub at `007d8020`); Bumblebee's pack has none, so this does nothing for him.
   - `3680e014` to the character's own collision bodies and, through `FUN_007b2e10`, to
     the bodies of each of his (up to 8) weapons. Its payload is `{id, set and clear bits
     for what the body sends, set and clear bits for what it takes, "all bodies", "from the
     defaults"}`. A `lux::PhysicsObject` takes it in `FUN_006dcda0` (stub `006dd4d0`):
     if "all" is clear, only the body whose node number (`+0x14`) equals `id`. What it
     sends is the word at `+0x9c`, what it takes at `+0x9e`. The event sets (or clears)
     value 2, `PUNCH`, in what the body TAKES.
   - The `id` is the node the event's channel belongs to. The event dispatcher
     (`FUN_0053adb0`) queues `{handler, key, node}` for each event key, the node being
     the player's channel-to-node map (`+0x28`) at the key's `index`; an event on a
     channel with no node in the object is dropped. So an event sits on a node's channel
     like any other key, and that node's body is the one switched. For a weapon
     (`FUN_007b2e10`) the node is compared with the weapon's own (`vtable +0x3c`), and all
     of that weapon's bodies are switched.
2. His robot object carries 15 `Collision` attachments (`work\collision_dump.py`), each
   with a `parent` node, a primitive (`matrix`, `maskPreset`, `packet`) and no dynamics:
   - preset `Attack`: a sphere on `Hand_L` and on `Hand_R` (radius 0.932 under a matrix
     scaled 1.967), and a cylinder on `Flipper` (radius 2.82, `height12` 3, centred 0.79
     under the node, which is 2.89 above his feet).
   - preset `Character`: boxes on `Spine_02`, both thighs, calves, feet, upper arms,
     forearms and the head.
   The axe (`MeleeWeapon`, class id `651a966e`) has no collision of its own.
   - The presets are in `bnxglobal.str#2` (`export\data\global_collision.txt`, root
     `a005201c`, list `presets`: name, `sendMask`, `receiveMask`; loader `FUN_00757c00`).
     The primitive loader (not a function in the analysis; `work\asm\
     collision_loader_005b95ab.asm`, from `work\raw_disasm.py`) looks the preset up and
     only falls back on the primitive's own `sendMask` / `receiveMask` if there is none.
     Bits: `PUSH` 1, `PUNCH` 2, `AVATAR_PUSH` 4, `AI_PUSH` 8, `GROUND` 0x10, `CAMERA`
     0x20, `PLAYER_PUSH` 0x40, `WATER` 0x80, `DETECT` 0x100, `SLIPPERY` 0x200,
     `AWAKE_STATIC` 0x400, `SPECIAL_BODY_PUSH` 0x1000. `Character` sends `PUNCH | WATER`
     and takes nothing; `Attack` sends `WATER | AWAKE_STATIC` and takes `DETECT`.
   - So a hit is: one of his bodies that the clip has let take `PUNCH`, touching a body
     that sends `PUNCH`: another character's limb boxes, or most of the world's objects.
   - The loader squares up a primitive's axes (`FUN_005715a0`) and multiplies their
     lengths back in, so the scale stays in the stored matrix. ASSUMED from that: a scaled
     sphere is a bigger sphere (hands 1.83). ASSUMED: `height12` is half the height (the
     name, and 2.89 +- 3 is him from foot to head) and the cylinder's axis is its z.
   - Which clip switches what (`work\attack_events.py <pack> CombatSet - AttackCollEvent`):
     `AttackFast_1` `Hand_R` ticks 10-20; `AttackFast_2` `Hand_L` and the axe's node
     `ObjectAttachment_Hand_L` 20-30; `AttackFastMP` the same two 20-42; the charge
     (`AttackFast_3_ChargeAttk`) both hands, both feet and the axe's node 8-46; the air
     attack `Hand_R` 6-20; the slam `Flipper` on at 14 and again at 20, off at 38; the
     ground punch `Hand_R` on at its last tick, 44, and never off.
3. The `MeleeWeapon` listens for contacts (message `f1e85af0`, stub `007bb7d0` ->
   `FUN_007a6710`) and passes each to its owner: `FUN_00719540(character, contact)`.
4. `FUN_00719540`: if what the other body sends and what this body takes share `PUNCH`,
   and the character is attacking (`+0xc8 == 1`, set with the start time `+0xc0` by
   `FUN_00748eb0` when the event's flag is on; 2 when it goes off), and the other object
   is not his own: `FUN_007189b0`.
5. `FUN_007189b0`, the hit:
   - A target already in the hit list (character `+0x228`) is skipped: ONE hit per target
     per attack state. The list is emptied when the attack state is left (`FUN_00878be0`),
     which also takes the attack bit off his weapons (`FUN_007b2d80`). So each swing of a
     chain can hit the same target once.
   - Skipped: himself; and a touch by his feet (bodies on `Foot_L`, `Foot_R`) with
     something that is not a character.
   - The record: contact point and normal, attack start time and the time of the first
     hit of this attack (`+0x1f4`), the damage, the damage type flags, the direction.
   - Damage (`FUN_00717c80`): the playing clip's attack entry (`animRef` type 6):
     `AI_damage` if it is not negative and the attacker is not a player's; else
     `MP_damage` if not negative and the game is multiplayer; else `damage`.
   - Direction (`FUN_00718230`): by the clip's `attack_dir`: a table at `00c05d08` in his
     own frame (0 forward, 1 left, 2 right, 3 down, 4 up), 5 from him to the target;
     flattened and made length 1. Every one of Bumblebee's clips has 0.
   - Type flags: the entry's `includeDamage` (or the `OnSmallChars` / `OnLargeChars` set
     by the target's size class, target attributes `+0x78`), less `excludeDamage`.
   - Scalings, each multiplying the damage:
     - `FUN_0079b680` / `FUN_0079b6d0`: Optimus's special ability. When the attacker's
       kind (attributes `+0xc0`) is 0 or 1 and his `+0x234` object's timer (`+0x8c`) runs:
       the shared `Character` value `optimusMeleeSpecialDamageMultiplier` (1.5; `MP_` 2)
       for kind 0, `optimusFlightMeleeSpecialDamageMultiplier` (1.75; `MP_` 1.5) for kind
       1 (block `00d40ad0` `+0x84`/`+0x88` and `+0x8c`/`+0x90`; the `MP_` one when
       `00dc967c` is not zero). Nothing for Bumblebee.
     - `FUN_0079d060`, for a human player in single player: the upgrade table
       (`Upgrades.upgradeParams` of `tuning.lxb`, at `00d40da4`) entry for the upgrade the
       clip belongs to, at the level the player has it (`thunk_FUN_01181eb0`); 1 at level
       0 or for a level the table lacks. Upgrade `62fffc0c` for AttackJump and
       AttackFast_1/2/3 (1.25, 1.5, 2), `084117aa` for the charge (1.25, 1.5, 2.5),
       `49ab1fe8` for the slam (1.25, 1.5, 2). The names behind the hashes are not in
       the game's files.
     - For an attacker that is not a player: the difficulty's `scaleEnemyDamage`
       (percent; `Easy/Medium/Hard/Expert` at `00d40aa0`).
   - Then message `59db091e`, the damage itself (`FUN_00724700` sends message `79de53b2`
     to the target), the hit's effect and sound by the surface hit (`FUN_0072d9c0`), and
     the target goes on the list.
6. `specialCaseDamageStuff` (`stunTime`, `slowTime`, `dotTime`, `heatIncrement`,
   `knockBackSpeed`, `knockBackAngle` in degrees) rides along (`FUN_00744f10` loads it).
   Hits 1 and 2 throw nothing; `AttackFast_3` 24 at 12 degrees, the charge 20 at 18, the
   air attack 15 at 35, the slam 25 at 45.

A second routine, `FUN_007190c0`, builds the same record with a damage handed in. It is
NOT the ground punch (an earlier guess here): its caller `FUN_00860870` is the drive
mode's contact handler, and the damage and throw are `modeDrive.damageData` (`amount`,
`knockBackSpeed`, `knockBackAngle`; mode tuning `+0x114`..`+0x120`): what the car does to
a character it runs into. Bumblebee's are all 0.

The ground punch's fist: `GroundPunch` and `GroundPunchWeak` switch `Hand_R` on from tick
0 to 34 (and the leap that leads in, `Trans_Vehicle2Robot2GroundPunch`, switches it on at
its last tick). The two punch clips have no attack entry (no damage, no throw).
`FUN_00717c80` gives 0 damage for a clip without one, so in the original the hand's touch
there is a hit worth nothing (what the rest of `FUN_007189b0` does without an entry is not
read); in the port it is no hit at all. What does the damage is the blast, below.

## The damage type flags (2026-10-03, later)

The text export names flag bits from the data file's own type table, and for several
fields that table is the wrong one (`includeDamage` prints as `AIDF_*`, `attack_dir` as an
animation channel). The raw masks are what the code tests; `work\flags_raw.py <pack#N>
<field>...` prints them with each type's bit order. The damage set, bit by bit:
0 `DAMAGE_BY_NONE`, 1 `BY_PLAYER`, 2 `BY_NPC`, 3 `BY_NON_CHARACTER`, 4 `OVERHEAT_CURR`,
5 `OVERHEAT_ALL`, 6 `FLASH_BANG`, 7 `WEAPON_HEAT_ZEROED`, 8 `BY_MELEE`, 9 `BY_PROJECTILE`,
10 `BY_OBJECT_IMPACT`, 11 `BY_EXPLOSION`, 12 `DISABLE_VEHICLE`, 13 `STUN`, 14 `SLOW`,
15 `DOT`, 16 `BY_FAST_FLAIL`, 17 `BY_STRONG_FLAIL`, 18 `BY_KNOCKBACK`,
19 `BY_PARTIAL_KNOCKBACK`, 20 `BY_CRASH`, 21 `NO_HIT_REACT`, 22 `DISABLE_TRANSFORM`,
23 `REMOTE`, 24 `HIT_THE_VEHICLE`, 25 `FRIENDS_ONLY`, 26 and 27 the spirit link pair,
28 `INSTANT_KILL`, 29 `PREDAMAGE`, 30 `NOTEAMFILTER`, 31 `BY_ALL`.

- Attack entry (`FUN_0070e5b0`): `includeDamage` `+0xa8`, `excludeDamage` `+0xac`,
  `includeDamageOnSmallChars` `+0xb0`, `includeDamageOnLargeChars` `+0xb4`. `FUN_007189b0`
  takes `+0xb0` for a character whose attributes `+0x78` is 0 and `+0xb4` for 1, else
  `+0xa8`. ASSUMED: `+0x78` is the attribute `size` (0 for Bumblebee, Ratchet and the
  scouts; 1 for Optimus, Blackout, the warriors); the loader was not checked.
- His clips: every attack has `BY_MELEE` (0x100). On top: `AttackFast_3` `BY_KNOCKBACK`
  on any target (0x40100); the charge `BY_STRONG_FLAIL` on small and `BY_FAST_FLAIL` on
  large characters; hits 1 and 2 nothing more.
- So the target's test "flags have any of 0xf0000" is "a flail or a knock-back kind":
  hits 1 and 2 never throw, whatever their throw speed. The same routine's bit 15 (no
  health taken) is `DOT`; the explosion's bit 6 (stun falls off) is `FLASH_BANG`.
- `FUN_007189b0`, on a hit with bit 8 (melee) under two flags of its own that were not
  followed, sets `+0x108` of his `+0x234` object to 4.0. Not read what that timer is.
- Explosions: the strong blast 0x20900, the weak one 0x10900 (`BY_MELEE | BY_EXPLOSION`
  and a flail).

## The hit's effect and sound (`FUN_0072d9c0`, 2026-10-03, later)

- The character's `+0x234` object (made by `FUN_00712d10`) gets from `thunk_FUN_0117ef10`:
  `+0x34` `particleHitTable.genericProjectileHitEffect`, `+0x38` the NAME of
  `particleHitTable.meleeHitSparkPreset`, a `MaterialImpactPreset`, which is registered
  under that name (`thunk_FUN_01184300`). His is `MeleeHitSmallPreset`.
- `FUN_0072d9c0(that object, surface)`, called at the end of every hit of
  `FUN_007189b0`: nothing unless there is another object, a preset, and the contact's
  normal is longer than 0.1. It fills `{surface, point, normal, the other object, |his
  body's velocity|}` (`FUN_0070a990` is the physics body's velocity) and calls
  `FUN_0077cd60(preset name, record)`.
- The surface is the OTHER body's material, physics body `+0x254`, a name hash.
- `FUN_0077cd60`: the preset's entry whose `material` list has the surface
  (`FUN_0077bff0`), else the entry listing `0 - Default` (37f718bf), else nothing. Then:
  - `soundOnImpact`: the event, at the point, with parameters `speed` (0f26fef6) and
    d9a785a4 (name not in the game's files) both set to the speed in the record.
  - `effectOnImpact` (a `ParticleTemplate`) by `effectOnImpactFlags`: bit 8
    `ATTACH_TO_HIT_OBJECT` puts it in the hit object's frame; else it is turned by
    `FUN_0077c180` (bits 0 to 7: z or y along the normal or up, random, along a forward).
  - `objectOnImpact`: one of the list at random, spawned (`objectOnImpactFlags`).
  - the mark on the surface (`ssdInfos`), unless the surface is `Water` or the object
    has bit 25 of `+0x60`; placed 0.2 (or 0.5 if the normal is nearly flat) back along
    the normal.
- His preset: `Default`, `Head`, `LowerBody`, `UpperBody`, `VehicleBody`, `Mud`, `Plaster`
  -> `H2H_PUNCH_SM`; `Asphault`, `Brick`, `Concrete`, `Stone` -> `H2H_PUNCH_SM_CONC`;
  `Glass` -> `_GLASS`; `MetalHard`, `MetalSoft` -> `_METAL`; `Sand`, `Dirt` -> `_SAND`;
  `Water` -> `_WATER`; `Wood` -> `_WOOD`. Every entry: effect `r_hit_melee`, flags 0x100
  (tied to the object hit), no object. The events are in each level's `mission.fev`.
- The same lookup serves `footStepFX` (`StepAutobotSmallPreset`), the climb's hand and
  foot presets and the tyres' (`FUN_0077d5d0`, `FUN_0077d890`; callers `FUN_008b79f0`,
  `FUN_008b7b60` not read).
- An explosion's hits do not come through `FUN_007189b0`, so the blast plays none of this.

## The ground punch's blast (2026-10-03, read from code and data)

An earlier version of these notes named attachment 14 (`road_rubble_spawnb`, with a
`KillBox`) as the blast. Wrong: no punch clip triggers it (clip `95533469` does, at tick 4;
`genericsplat` is triggered by clip `c875c7fa`; neither name is in the game's files).

- At tick 8 both punch clips send a trigger (`AnimSendTriggerEvent`, message `d5d636c1`)
  to a `LuxScriptName`: `FX_groundpunch` (`GroundPunch`) or `FX_groundpunchweak`
  (`GroundPunchWeak`). On node 35 (`Hand_R`) his object has, by those names: an
  `EffectSpawner` (`r_radialattack_optimus` / `r_radialattack_small`), for the strong one a
  `TriggerableParticle` too, and an `ObjectSpawnerOnBone` whose `spawnObject` is
  `genericgroundpound` / `genericgroundpoundweak`.
- `ObjectSpawnerOnBone` (loader `FUN_007fc140`: `spawnObject` `+0x44`, `attachToBone`
  `+0x48`, on when absent; trigger handler `FUN_007fc270`): puts the object at the node's
  place and, with `attachToBone`, ties it to the node (`FUN_00711f40`, not read).
- The spawned object: a `TriggerOnSpawn` that starts a `ControllerRumble` (strength 55 for
  0.7 s / 0.3 s), an `Explosion`, and a `Collision` sphere of radius 18 / 15 with preset
  `Explosion` (sends `AWAKE_STATIC`, takes `PUNCH | DETECT`).
- `Explosion` (loader `FUN_007e5640`): `start_scale` `+0x28`, `damage` `+0x2c`, `minDamage`
  `+0x30`, `min_radius_fall_off_damage` `+0x34`, `explosion_time` `+0x38`, `delay` `+0x3c`,
  `damage_flags` `+0x40`, `damage_player` `+0x44`, `can_damage_self` `+0x45`,
  `useGroundPoundDamageOverride` `+0x46`, `cameraShakeOnExplode` `+0x48`, the special case
  block `+0x4c`..`+0x60` (stun, slow, dot, heat, knock-back speed, knock-back angle).
  Strong: damage 65, throw 35 at 3 degrees, flags `DAMAGE_BY_MELEE | DAMAGE_BY_EXPLOSION |
  DAMAGE_BY_STRONG_FLAIL`. Weak: 40, 25 at 3 degrees, `..._FAST_FLAIL`. Both: start scale
  0.5, time 0.15, no delay, fall-off -1, `damage_player`, not self, the override on.
- Owner (`FUN_007e6af0`): with the override on and a character for an owner, damage and
  `minDamage` both become the owner's attribute `groundPoundDamage` (`+0xf4`, 100) if the
  data's damage is OVER 5 (constant `00b3476c`), else `groundPoundDamageWeak` (`+0xf8`,
  60). 65 and 40 are both over 5: both punches do 100. The weak value is what an explosion
  with damage 0 gets (his `genericsplat`). It is what the code does; whether it was meant
  is another matter. The same routine sets record field `+0x78` from the owner's `+0x234`
  object (`FUN_0072e800(0)`, not read).
- Update (`FUN_007e5f00`): while `delay` is not negative it counts down, and when it
  passes zero bit 2 of the object's `+0x60` is cleared and the start time kept. After
  that: age `+0x80` += dt; the object's matrix is its first one (`+0x88`) scaled by
  min(1, start_scale + (1 - start_scale) / explosion_time * age), or by 1 if
  explosion_time is not over dt; the object is deleted once the age is over
  explosion_time both before and after the update.
- Contact (`FUN_007e60f0`): for a body that sends `PUNCH` to this one that takes it, once
  per object (list `+0xb8`). Skipped: a player's character unless `damage_player`; the
  owner and what the owner owns unless `can_damage_self`. Direction: flat from the
  explosion's place to the other object's, length 1 (minus the contact normal if they
  coincide). Force: the 3D unit vector times damage times 100. Damage: whole; but with
  `min_radius_fall_off_damage` f in 0..=1 and R the sphere's radius, at distance d of the
  contact point from the middle: whole under f*R, `minDamage` from R on, and in between
  minDamage + (damage - minDamage) * (1 - (d - f*R) / (R - f*R)); with flag bit 6 the
  stun time falls the same way to 0.6 of itself. Then `FUN_00724700`, the same damage
  message a fist sends. No melee upgrade and no Optimus scaling here (those are in
  `FUN_007189b0`). A first part of the routine is the surface scorch (`SSDImpactPreset`)
  on bodies that send `DETECT`; not followed.
- `work\explosions.py` lists every `Explosion` of a pack.

## The target's side (outline only)

A character listens for message `79de53b2` (listener at `+0x3b4`, stub `007250f0`) and
takes it in `FUN_00719830` (2,635 bytes; `work\f_00719830.c`). `FUN_00718480`, which these
notes used to name as the target's side, is something else: it builds a "which part was
touched and from where" record for the handler at character `+0x344`. Read so far of
`FUN_00719830`, in order:

- Nothing if the target's flag word `+0x14c` lacks bit 10 or has bit 11.
- A share of a hit goes to another character under some condition (`00d40b4c` is the
  share; looks like a shield or a link between two characters). Not followed.
- The flags get who hit: 1 a player's character, 2 another character, 4 nothing.
- Scalings on the receiving side: `FUN_0079cdc0`, the difficulty (`FUN_0079ac30` on
  `00d40aa0`) when a computer's character is hit by a player's, the two Optimus special
  values (`FUN_0079b5e0`, `FUN_0079b630`), attributes `+0xfc` under `FUN_0070ab90`, and a
  doubling under a test of `FUN_00725750`.
- The hit reaction is chosen by `FUN_0072e640` (object at `+0x234`), the health goes down
  in `FUN_0071f650(target, damage)` unless the flags have bit 15 or `FUN_00711260` holds;
  with a special running the hit cannot take the last point of health.
- The throw: the blow's direction (record `+0x28`) is kept at `+0x110`. If the flags have
  any of `0xf0000`, or the target's `+0x148` has bit 3, and the record's knock-back angle
  (`+0x6c`) is at least `FUN_006f8950()`, the direction is tipped up by that angle; then it
  is multiplied by the knock-back speed (`+0x68`) and by the TARGET's attributes `+0x7c`.
  So a thrown target leaves at speed x its own scale, along the blow tipped up: what the
  dummies do, less the scale and the flag test.

The attack entry's fields (`FUN_0070e5b0`, offsets in the loaded object): damage `+0x84`,
MP `+0x88`, AI `+0x8c`, slideSpeed `+0x90`, minRange `+0x94`, maxRange `+0x98`, chance,
chain_id, chain_count, the damage sets, auto_target_turn_speed `+0xb8`, turn_speed `+0xbc`,
stick_choose_target_max_degrees `+0xc0`, auto_target_search_angle `+0xc4` (all four turned
to radians, the last two kept within 0 and pi), attack_type, attack_dir `+0xd4`, hitBones
`+0xd8`, stop_on_hit `+0xdc`, interrupt_in_air, the special case block `+0xe0`.

## The attack mode's target (`FUN_00855af0`, disassembly)

Character flags (`+0x14c`, tested through `FUN_00725990(index)`): 0x17 allowSlide, 0x18
allowFace, 0x19 allowStickMovement.

Each update, while the attack has not hit a character (mode `+0x18`) and slide or face is
allowed:

1. Direction: the stick if it is 0.001 or more, else his facing. If the clip's
   `stick_choose_target_max_degrees` is under 180 and the stick is further round than
   that, the direction is put at the limit: on the side AWAY from the stick, as the code
   is written (it takes facing minus stick, and for a positive difference ADDS the limit).
   Looks like a slip of theirs; only the slam (110) has a limit under 180.
2. If the mode holds no target (`+0x20`): search (below) with the clip's `maxRange` and
   `auto_target_search_angle`, and KEEP what it finds. The mode is not restarted between
   the states of a chain, so the target carries over; when an attack that hit a character
   has been left (hit list empty again) the mode lets the target go and resets `+0x18`.
3. With a target, unless it is a character in state `7374abee` (`FUN_007258d0`, not
   read): if it is flat further than touching (his radius plus, for a character, its
   radius; `FUN_0070ab60`: the physics holder's radius for the form he is in):
   - with face allowed and `auto_target_turn_speed` > 0: turn to it at that speed
     (`FUN_00717770`: only when more than 1 degree off). This also stops the stick's turn
     for the update.
   - with slide allowed and `slideSpeed` > 0, on the ground: if the 3D distance is
     between `minRange` and `maxRange` and its square over 0.1, the body is MOVED (its
     position written) toward the target by `slideSpeed * dt`, at most the distance. It
     does not stop short at touching distance: the next update simply finds him within
     it. In the air (mode `+0x2a`) the slide is flat, with a separate vertical step, and
     only if the height difference is under `FUN_0070abe0`.
4. The stick's turn (`turn_speed`): needs allowStickMovement, no target turn this update,
   on the ground, flag 0x15 CLEAR (not read what sets it), the stick over 0.1, and, if
   the clip has `stop_on_hit`, no hit on a character yet.
5. `stop_on_hit` also sets a value of the animation player's (`+0xc38`) to 2 once a
   character is hit, and puts the old one back when the hit list empties. Not read what
   it is (one of his clips has it).
6. Off the ground, the body's upward speed is capped at 2.

## The search (`FUN_0071efb0`, score `FUN_007996c0`, choice `FUN_0079a000`)

Nothing for a remote player's character. The query: his position, the direction as yaw
and pitch, the range (the clip's, or `autoTargetMeleeDistance` 25 when it gives none), the
height limit (`autoTargetMeleeHeight` 5), the angle for yaw and for pitch (the clip's, or
the shared `autoTargetAngRange`), and the shared `Character` values of `tuning.lxb`
(loader `FUN_0079ad00`, block at `00d40ad0`): `autoTargetBestDist` 1.5,
`autoTrgAngleWeight` 1, `autoTrgDistWeight` 0.5, `autoTrgMatchAtkWeight` 0.75. (A clip
whose `attack_type` is `87711403` uses a second distance and height pair, attributes
`+0x108` / `+0x10c`.)

A candidate, at offset `d` from him:

- distance = | flat length of d - its radius |; must be over 0 and under the range.
- |d.z| under the height limit.
- yaw off the direction and pitch off the direction, each under the angle.
- score = distWeight * |distance - best| / max(range - best, best)
        + angleWeight * (yaw off + pitch off) / angle,
  times `autoTrgMatchAtkWeight` if the candidate's kind matches the mask passed in (the
  attack mode passes none).

  Checked again2026-10-07 while settling weapon-entry aim weights: scorer query+0x34
  is distance, +0x38 yaw, +0x3c pitch (`00799958..0079997f`). Melee's query at
  `0071efb0` gives +0x34=autoTrgDistWeight and BOTH angular weights=autoTrgAngleWeight;
  +0x44 is their sum for the noncharacter penalty, +0x48 the match factor. The aim
  query gives distance1/yaw2.5/pitch1 instead. `Query::score_with_angles` handles those
  independently; the ordinary melee wrapper preserves the equal angular weights.
- Candidates are filtered by kind bits against a mask built from who is asking (teams in
  the top bits, objects in bits 1 and 2); a candidate that is an object and not a
  character has distWeight + angleWeight ADDED. Dead and untargetable ones are skipped
  (`FUN_0071f460`). The lowest score wins; the first on a tie.

## In the port

`core\src\melee.rs` (search, score, direction, the `Hit` record, his bodies and the
touch, the upgrade scaling) and `sim.rs` (`attack_aim`, `land_hits`, the hit list, the
kept target). `World::targets()` lists what can be hit; `State::hits` hands the hits of a
step to whoever owns the targets. `control_check` plays three taps and a hold at a target.

His bodies (2026-10-03): `formats\scene.rs` reads the `Collision` attachments;
`pose.rs` poses the node tree by a clip (the same arithmetic as the drawing side);
`melee::attack_bodies` keeps, for each clip, the bodies its `AttackCollEvent`s name at
every whole tick in his own frame (`ActionClip::bodies`). `ActionState::live` is the set
switched on; `State::attack_bodies` puts them in the world; `land_hits` tests each against
each target (`melee::touch`), a foot only against a character. The game draws them with B
or `--bodies`. With the real clips, `control_check`: hit 1 lands by `Hand_R` at tick 17
while he is still 5.6 from the target's middle, hits 2 and 3 by `Hand_L` (the axe hand),
the charge by `Hand_L` at tick 38, running straight at a target; a target 6 to the side
of the charge's line is missed (the old stand-in reached it).

Stand-ins and assumptions:

- The target's side of the touch: one upright cylinder (`Body::of_target`), where the
  original has whichever of the target's bodies send `PUNCH`.
- How a touch is found (`melee::touch`, nearest points back and forth; a discrete test at
  the clip's tick each update). The original's contact search is not read.
- The bodies are posed by the clip alone, without the blend out of the clip before
  (hit 1's window opens 0.15 s into a 0.2 s blend).
- A new state starts with nothing switched on. In the original the bit stays on a body
  until an event takes it off (the ground punch never does).
- Sphere scale, `height12`, the cylinder's axis: see above.
- Two bodies pushing apart (`Arena::push_out` against targets).
- The game's dummies and everything they do when hit (`game\src\targets.rs`).
- Upgrade levels: kept in `State::melee_levels`, all 0; nothing raises them.

The blast in the port: `formats\scene.rs` reads the spawners and their `Explosion`
(`SpawnerDef`, `ExplosionDef`), `anim.rs` the trigger event, `melee::clip_blasts` the path
of the spawner's node through the clip, `sim::blast_hits` the explosion's update and its
hits (`State::blasts`, `Hit::blast`). `control_check` punches the ground among three
targets: blast at the fist on the ground, radius 7.5 to 15 over six updates, 100 damage and
a throw of 25 at 3 degrees to the two in reach, nothing to the one 30 away. B or `--bodies`
draws the sphere (`work\shots\blast_1.80.png`). Assumed there: its first update is the one
after the trigger; it is at `start_scale` until its age first goes up; a target at its
very middle is thrown nowhere. Stand-ins: the touch and the target's cylinder, as for the
fists; once the clip that set it off is left the blast stays put (the original's stays on
the hand).

Flags and sound in the port (2026-10-03, later): `AttackRef::flags_for`, `Hit::flags`,
`Hit::throws` (the dummies are thrown only by a throwing kind), `Target::large` and
`Target::surface`, `character::ImpactPreset` (`CharacterData::melee_impact`,
`step_impact`), and the game's sound plays the preset's event for each hit of his body.
Stand-in: the dummies' surface (`UpperBody`). Not checked by ear by anyone yet.

## Rumble and the blast's camera shake (2026-10-03, read from code and data)

- The camera shake is in `docs/camera.md`, "Shakes": the explosion starts
  `cameraShakeOnExplode` at its place (`FUN_007e5d30`): `MassiveCameraShake` for the
  strong punch, `MedHardCameraShake` for the weak one.
- The pad's effect (`DAT_00d1e87c`, 0x20 bytes; a copy at `DAT_00d1e89c` is swapped in and
  out by `FUN_005aafe0` / `FUN_005ab060`, not followed): `+0x00` running, `+0x04` time,
  then two motors of {strength, hold, fade} at `+0x08` and `+0x14`.
  - Start (`FUN_005aada0`, type in EAX, then duration, strength): type 1: first motor a
    quarter of the strength fading over the duration, second motor all of it fading over
    half the duration; types 2 and 3: first motor at the strength held for the duration,
    then off; type 0 (and any type when rumble is switched off in the options): silence.
    A new effect replaces the one running.
  - Update (`FUN_005aae80`): each motor is its strength while time < hold, then
    (fade - (time - hold)) * strength / fade while that is inside a fade over 0, else 0;
    both go to the pad as 16-bit words (`FUN_0050ba00`; `FUN_004ecb60` hands them to
    `XInputSetState`); time += 1/60; when both are 0 the effect ends. The first word is
    taken to be the left (heavy) motor: XInput's order, not checked further.
- `ControllerRumble` (loader `FUN_007dbaa0`): `rumbleType` `+0x44`, `duration` `+0x48`,
  `strength` `+0x4c` (stored x 0.01), `scaleType` `+0x50`, `distanceNear` `+0x54`,
  `distanceFar` `+0x58`, `waitForTrigger` `+0x5c`, `playOnce` `+0x5d`, fired `+0x64`.
  Start (`FUN_007dbcb0`), for every player (`FUN_00731b90`): scaleType 0 as it is;
  scaleType 1 (`FUN_007dbdc0`) by the player's distance d from the object: nothing past
  `distanceFar`, whole under `distanceNear`, else times (far - d) / (far - near).
  With `playOnce` it does not start twice.
- His blasts: the spawned object's `TriggerOnSpawn` sends `RumbleMe` to its
  `ControllerRumble`: type 1, strength 55, near 1, far 25, scaleType 1, for 0.7 s
  (strong) or 0.3 s (weak).
- Presets (`effects\rumblefx.lxb`, which is `bnxglobal.str#19`, loader `FUN_007dbed0`
  into the table at `00ce4e08`; played by number through `FUN_007dc0a0`): 15 entries of
  id, type, duration, strength. The text export names the type from the wrong enum
  (`RUMBLE_BLOCK_DEFENDER` is 1, `RUMBLE_STRONG_HIT_DEFENDER` is 3). Among them:
  6 `RUMBLE_JUMP_LAND` (3, 0.2 s, 0.2), 8 `RUMBLE_CLIMB_ATTACHED` (1, 0.25, 0.8),
  9 `RUMBLE_CLIMB_MOVED` (3, 0, 0.4), 10 `RUMBLE_CLIMB_SLIDING` (3, 0.2, 0.2),
  2-5 the melee hits (1; strong 0.45 s, fast 0.25 s, strength 1), 12-14 the car's knocks
  and turbo. Who plays which, apart from the climb's (`FUN_00717c40`, in
  `docs/climbing.md`), is NOT read. `AnimControllerRumbleEvent` (`00ad8240`) is not read.
- In the port: `core\src\rumble.rs` (`Pad`, `RumbleDef`), `SpawnerDef::rumble`,
  `sim::State::rumbles`, `game\src\rumble.rs` (Bevy's rumble request, replaced every
  sixtieth). Assumed: the distance is from where he stands. Not felt by anyone.

## The preset rumbles: who plays which (2026-10-04, read from code)

`FUN_00717c40` (character in EDI, the preset's number on the stack) is the ONLY way a
preset is played (`work\find_call.py` finds every call to it and to `FUN_007dc0a0`, also
from code Ghidra left out of functions). It does nothing unless the character is a local
player's (corrected 2026-10-05: it RETURNS when the `+0x100` object's kind is 2, so 2 is
not a human; kind 0 is, as `FUN_00725a40` has it; `FUN_007212f0` gives its pad).

- 2 / 4 `RUMBLE_STRONG_HIT_ATTACKER` / `RUMBLE_FAST_HIT_ATTACKER`: `FUN_00717d80`, the
  attacker's side of a hit, called by the target's damage routine `FUN_00719830` when
  there is an attacker other than the target; for a hit whose flags have `DAMAGE_BY_MELEE`
  (bit 8). Strong when the attacker's clip (`+0xfc` -> `+0x2c` -> `+0x10`) is
  `AttackFast_3_ChargeAttk` or `Trans_Vehicle2RobotSlam`, else fast. Both type 1,
  strength 1; 0.45 s and 0.25 s. The same routine plays a sound of the attacker's
  (`+0x284` fast, `+0x288` strong, the damage as parameter `1664515a`; which sounds is
  NOT read) and, for an attack entry of type 6 in `StateAttack` under a flag of the mode,
  stops him dead and sets the animation value `+0xc38` to 2 (the `stop_on_hit` business;
  not ported). A second damage routine, `FUN_00743a40` (not a character's; class not
  identified), rumbles the attacker the same way.
- 3 / 5 the same for a player's character that is hit; 7 `RUMBLE_SPECIAL_CONNECTED` for a
  hit with flags `0xe0f0` (both in `FUN_00719830`). Not his side; not ported.
- 8, 9, 10 the climb (`docs/climbing.md`): 9 every update going up or sideways, 10
  going down. Preset 9 is type 3 for 0 SECONDS: silence, and it stops what was running.
- 11 `RUMBLE_VEHICLE_HIT_DESTRUCTABLE`, 12 `RUMBLE_VEHICLE_HIT_WALL`: the drive mode's
  contact handler `FUN_00860870` (and `FUN_0086ada0`, another mode's). 11: the other
  body's object has `+0xa4` and is no character. 12: otherwise, when his body's speed is
  over 5 and the contact's kind (`+0x3c`) is 3 (what the kinds are is not read).
- 13 `RUMBLE_VEHICLE_HIT_GROUND`: the drive mode's update `FUN_0085cad0`: no wheel was on
  the ground after the wheel set's last step (wheel set `+0xa8`, written by
  `FUN_007a3b00`), one is now, and the mode has run for over 0.1 s (`+0x1fc`).
- 14 `RUMBLE_VEHICLE_TURBO`: the turbo starting (`FUN_0085cad0`, `FUN_00733e10`, and
  `FUN_00864320` for another mode).
- 0, 1 (blocks) and 6 `RUMBLE_JUMP_LAND` are played by NOTHING, and none of his clips has
  an `AnimControllerRumbleEvent` (only Ratchet's, Long Haul's and some mission packs
  do): a landing on foot does not rumble in the original.

In the port (`rumble::preset`, `Tuning::rumble_presets`, `sim::preset_rumble`): 2 and 4
for every hit a dummy takes, the blast's included (its flags have the melee bit; ASSUMED
that its damage record names him as the attacker); 9 and 10; 12 for the arena's push at
over 5 (the contact kind assumed); 13; 14. Not ported: 11 (nothing breaks here), the
defender's. Tests: `the_car_rumbles_the_pad_for_its_turbo_and_its_knocks`,
`a_held_rumble_of_no_length_is_silence`, and the table itself in
`his_ground_punches_shake_the_camera_and_the_pad`. `control_check` prints the blast's
two rumbles (its own at 0.538, then the hit's at 1.0). Not felt by anyone.

## The hit's and the blast's effects (2026-10-04)

In `docs/particles.md`: the particle system, `r_hit_melee`, the punches' effect
spawners with their shock ring meshes. Drawn; the drawing is mostly assumed.

## The leftovers, closed (2026-10-04, later)

Read from disassembly and data; what each turned out to be:

- `stop_on_hit` (attack entry `+0xdc`, a yes/no; the text export prints it as an animation
  channel name, `work\flags_raw.py` gives the raw value). Only ONE of his clips has it:
  `Trans_Vehicle2RobotSlam`, the slam out of the car (damage 90). The attack mode
  (`FUN_00855af0`), on the update its hit list first holds a character and while on the
  ground, sets the animation player's `+0xc38` to 2 through `FUN_0084e670`, and puts the
  old value back (kept in the mode's `+0x1c` by its enter `FUN_008569a0`) when the hit
  list empties. `+0xc38` is the ROOT MODE of the pose writer (`FUN_00544...`, the block
  at `00540000.c:4145`): 0 writes the clip's whole root transform, 1 its axes only, 2
  nothing: the clip's root stops moving him. The attacker's side of the hit
  (`FUN_00717d80`) also zeroes his velocity (`+0x128..+0x130`) for a melee attack entry
  (class constant 6, every `AttackRef`) in `StateAttack` unless the hit is a knock-back
  kind on a target without bit 1 of `+0x14c`. PORTED: `step_action` stops the clip's
  travel once the attack has hit a character (test
  `a_clip_that_stops_on_a_hit_carries_him_no_further`). Not followed: owner `+0x26c`,
  the state hash `2839daf3` that is excepted, the mode's second way to mode 2 (its
  `+0x28`).
- The attacker's hit sound (`+0x284` fast, `+0x288` strong): loaded by `FUN_00721f00`
  from the `soundTable` fields `406e38ba` and `0ae3b908` (names not in the game's
  files). His pack has NEITHER field (the hashes occur nowhere in the chunk), so both are
  0 and he plays no such sound. Nothing to port.
- Character flag 0x15 (bit 21 of `+0x14c`), which stops the stick's turn in the attack
  mode: set by `FUN_0072df10` when he is linked to another object (`FUN_009192c0`),
  cleared by `FUN_0072e080` (which unlinks). Nothing in the port links him to anything,
  so it is never set. Nothing to port.
- State `7374abee` is `StateDash`: a target that is dodging is not turned to or slid at.
  No target in the port dodges.
- The air slide (the mode's `+0x2a`): only with `slideSpeed` over 0, and his air attack
  has none; its height limit `FUN_0070abe0` was looked at (the body shape's height less
  some of the form's values) and left. Nothing to port.
- Surface marks BUILT 2026-10-06; see `docs/surface-damage.md` for the code, shader
  arithmetic, data, tests and qualifications. The historical gap below is superseded:
  hand and ground-punch contacts now reach the arena; gun rounds and missile explosions
  use their own SSD presets. Drawing uses the original noise mask arithmetic with
  borrowed concrete maps on the stand-in arena. Noise transform order is still assumed.
- The mark of a hit and the blast's scorch (`ssdInfos`, `SSD_Step_S` for his fists:
  radius 2, 10 frames, noise thresholds for cracks and damage): put on the SURFACE struck,
  not on a character. Fists and the blast here touch only the dummies, never the arena,
  so there is nowhere for one to go until attacks are tested against the world (and the
  drawing is a noise shader that is not read). NOT built.
- Still not built: the fist's 0-damage hit during the ground punch, the car's ramming
  damage (0 for him), the target's damage routine `FUN_00719830` (with item 4), the
  particles' real drawing (`docs/particles.md`).

## The attacks out of the car (2026-10-04, later)

Two states of the graph were refused only because nothing pressed their buttons:

- `stateDriveAttackSlamRight` (`StateAttack`, attack mode, clip `Trans_Vehicle2RobotSlam`):
  from `stateDrive` by `connectionListDriveToSlamRight`: button 0x24
  (`AttackTransformSlam`, bound to the MELEE button) down, 0x15 (the vehicle trigger) not
  down, on the ground, `checkCanAdvTransform`.
- `stateDriveAttackAirPunch` (`StateAttack`, the first mode, clip
  `Trans_Vehicle2Robot2GroundPunch`): the same with button 0x23 (`AttackTransformPunch`,
  bound to the ACTION / climb button). Its clip carries him 8.4 forward and 3.08 up over
  44 ticks; the graph then gives `stateGroundPunchFall` and the STRONG `stateGroundPunch`.
- `checkCanAdvTransform` is character flag 0x20 (`FUN_0084c050`: `FUN_00725990` with 0x20
  in EDX, bit 0 of `+0x150`). The drive mode's update `FUN_0085cad0` sets it once the mode
  has run `advTransformDelay` (0.25 s; `00d40b74`, loader `FUN_0079ad00`), and
  `FUN_0085e130` / `FUN_0085ed50` clear it.
- In the port: `button::TRANSFORM_SLAM` / `TRANSFORM_PUNCH` fed from the melee and climb
  buttons, `Facts::can_adv_transform` from the car's time, `ActionState::lifted`.
  `control_check` ("car, slam out", "car, punch out", "slam out of the car at a target"):
  the slam hits with the drum on `Flipper` at tick 15 for 90, thrown at 25 and 45 degrees,
  and he stops where he hit; the punch rises to 3.05, falls, and plays the strong ground
  punch. Capture `work\shots\slam_a.png`, `slam_b.png`.
- Assumed: he drops the car's speed at once (the action carries the old velocity only
  over the clip's blend, which is 0); the punch's clip lifts him (the first mode's control
  vector was not read for this state). Not checked against the original: no recording
  has either.

## Animation completion pass (2026-10-06)

Character and axe cross-fades now keep outgoing clips moving, including interrupted
blends (`docs/animation-rules.md`). The original named arm/forearm/axe ribbon trails
now follow clip trigger and exit keys (`docs/weapon-mode.md`, "Melee ribbon trails").
The axe's blade glow was already ported in `fx::Glow`; the earlier standard-material
debt was stale. Collision bodies still sample the attack clip alone during a fade
(the existing debt under "What is left" is not closed by changing rendering).

Receiving hit animations now select full/partial reactions from game damage kinds
and control masks. Fast/slow flyback uses HitReactionSet recovery/exit rules, and the
completed state returns through the original default-state path. The 20-tick additive
HitFront layer honors partial base filters and preserves weapon firing. `receive_hit`
accepts pre-scaled damage and resolved impulse; H/`--hit-at` provide synthetic inputs
for isolated captures. Update2026-10-07: installed scouts now send gun/melee packets
through the shared receiver. Scaling, immunity gates, stun and wall-splat predicates
are implemented (`docs/damage.md`); target cylinders/contact classification and
original enemy AI remain debts. The flyback's 2.5m-drop release and basic hit's fall
gate are ported.
Evidence and tests:
`docs/animation-rules.md`, "Death and hit reaction layers".
