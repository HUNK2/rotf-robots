# Receiving damage, health, death and combat (2026-10-07)

## Executable findings

`00719830` is the shared receiver, not a bullet-only routine. Its early gates are
character flag10 (enabled), flag11 (reject damage), and `00724180` (attribute/player
invincibility); invincibility still permits self packets with mask `0x40f0f0`.
`00717f90` rejects ordinary self damage, teammates unless `0x42000000`, and enemies
for FRIENDS_ONLY (bit25). It also calls the controller's damage veto. Controller
type2 (`00711260`) routes health elsewhere. Multiplayer shield sharing is a distinct
branch and is outside the single-player arena.

The receiver rewrites source bits0..2 to player/NPC/noncharacter, scales NPC targets
of player attacks by the difficulty helper, applies shield multipliers (ability
types0/1 only), vehicle attribute `damageMultiplierInVehicle` unless PREDAMAGE
(bit23), and the double-damage cheat (indices10/11/15). Ability types0/1 and a
passed spirit-link packet cannot take the final health point. Bumblebee's EMP is
type -1, so it does **not** grant that protection. Attacker character flag30 scales
damage to 0.1. DOT (bit15) schedules ticks and bypasses immediate subtraction.
`0071f650` clamps health, kills below 0.1 (not just zero), and dispatches death.

Throw velocity: retain the packet direction's elevation when it already exceeds
the requested knockback angle; otherwise replace elevation with that angle. Multiply
by packet speed and the receiver's `knockBackMultiplier` (`00719830`, narrow raw
`0071a10f..0071a24b`; pitch helper `006f8950`). Without throw flags, the character
stores the direction for facing but does not acquire that velocity.

`00729120`: STUN is bit13 (bit6 is FLASH_BANG). Positive stunTime replaces the
stun clock. Subsequent positive damage consumes a break budget; it ends strictly
below zero. Stuns caused by Bumblebee use `bumbleBeeStunBreakThreshold` (80), other sources
use `dComWarriorStunBreakThreshold` (100, shared tuning). DISABLE_VEHICLE bit12 and
DISABLE_TRANSFORM bit22 replace their timers. SLOW bit14 replaces slowTime. DOT
stores `ceil(dotTime)-0.01` and packet damage, then `0072a550` applies damage when
integer boundaries are crossed, including the final tick. Shield abilities suppress
these statuses and full/partial reaction dispatch. NO_HIT_REACT bit21 suppresses
reactions; invulnFlagsBase selects individual reaction immunity.

Regeneration is **not constant-rate**. `00726710` loads duration to +0x50 and its
reciprocal to +0x54; healthRegenRate is only the positive enable gate here.
`0071ffb0` stops/resets delay at full health or while DOT runs. Otherwise it reduces
the delay below zero and, when enabled, adds
`3 * maxHealth / duration^3 * (dt/2 - remainingDelay)^2 * dt`.
For positive healthRegenSecsPerTic, use that interval instead of dt when the
accumulator crosses it (one tick per update, retaining remainder). The prior build's
39/s interpretation was wrong. Confirmed in raw `00720057..00720243`.

Explosion death enters BasicMotionMode without the ordinary (1,1,0) root-control
override (`0087b070`); ordinary death uses XY root motion. Death clocks count from
entry: finished and grounded after 3s, or 10s timeout (`0087b610`). Player restart is
blocked only in multiplayer, or by character flag35. Single-player asks the mission
to restart; NPC completion requests removal (`0087af40`).

KnockBackMode wall contacts (`00875630`) reject a finished animation, a contact
with a character, object flag0x1000, or an already processed splat. Normalize the
normal, reject abs(z)>=0.5, require body forward dot normal >= -0.5, then set
ANIM_DAMAGE_REACT_WALL_SPLAT, face the normal, and return to BasicMotionMode XY.
The vector used in this dot is the physics body's **forward matrix row**
(`006de7e0` copies a 12-float transform; raw `008757e2..00875844`).

## Implementation and validation

`core/src/damage.rs`: shared single-player packet receiver, source/team/self gates,
controller veto/external-health hooks, invincibility, reciprocal NPC difficulty health
scaling, vehicle and shield multipliers, overdrive/immune-to-overdrive, double-damage
and reduced-attacker hooks, instant kill, resolved throw direction/speed, status clocks,
stun break budget, DOT boundary ticks and cubic regeneration. `Runtime` receives the
original packet's amount (the receiver's health scale is local, not written back into
that packet), as `00729120` does. Cheat/scripted-controller hooks are disabled for the
arena's Bumblebee/scouts; no original save or cheat state is imported.

`sim::State::receive_damage` is now the gameplay entry for both teams. Melee contacts,
bullets and missile explosions keep flags/direction/special-case fields on their way
to the receiver. Full/partial/stun reactions go through the real state and animation
graphs. Generic NPC `StateAttack` with animName0 starts CombatSet through entry rules
(`008788b0`); the old explicit-clip-only gate prevented the scout from attacking.
Death uses XYZ roots for explosion deaths, XY plus gravity otherwise; completion
asks the host to remove an NPC or restart the player. The test arena resets its
encounter on the player's request or R; no NPC resurrects after three seconds.

SLOW: `0072a550` / raw `0072a8eb..0072a927` set controller speed to slowMultiplier
and animation delta +308 to that value minus1. `007a8f20` divides rateOfFire by
slowFireRate (raw `007a8f73..007a8f8b`); `007a9610` multiplies heat increment by
slowHeatMeterIncrementRate (raw `007a9663..007a9678`). These numbers are loaded
(0.5 / 0.65 / 0.65 single player). Flags4/5 dispatch received heat through the weapon
manager, with CURRENT taking precedence over ALL, as `00729120` lines302..326.
FX_regenerate / FX_stun / FX_frost now follow the actual runtime clocks.

`core/src/combat.rs` and `game/src/targets.rs`: three installed DComScout/Swindle
models replace the cylinders. Each has its own State, loaded tuning, real attack/hit/
stun/death clips and machine gun. HP200, attack melee distance7, height10, attack delay2,
ranged distance500, backpedal15, sight500; gun damage2, interval0.1 (0.128 at32ms),
flags0x200200, heat3/100, overheat7s, generalNoiseRadius1.5. Gunfire uses the existing
weapon/partial-animation machinery and the non-player disc scatter from `007aa210`.
NPC melee uses AI_damage and scaleEnemyDamage; gun damage comes from its weapon
attributes (00782b30 has no extra difficulty multiplier). Difficulty health scaling
is reciprocal and affects only NPC recipients of player attacks. Player aim assist
receives each scout's simulated body velocity; the old zero-speed dummy input is gone.

Original AI attack path read: 0081b790 loads ignoreAttackDelay/ignoreKungFuFighting/
weaponIndex; 0081b850 evaluates perception priority and the foe's kung-fu slot;
008297e0 / 00822360 request controller buttons. Scout combat goal has melee priority
.92 versus ranged .6. Those actions run the shared state machine here; the entire
brain/goal/navigation framework has NOT been ported (see explicit debts below).

Validation: `cargo test -p tf2-core` passed all 238 tests (9 receiver and 8 installed
combat integration tests), and `cargo build -p bumblebee` passed. Installed tests cover
a scout killing Bumblebee, original clip
melee contacts, EMP stun/break, disable-transform expiry, slow movement/fire/heat,
airborne wall splat and Recover_WallSplat, reciprocal difficulty data, explosion Z
roots, corpse completion and no timed resurrection. `core/examples/combat_check.rs`
runs a compact deterministic installed encounter without a window; passed with 25
rounds/25 hits, Bumblebee HP270, EMP StateStun/no rounds, and completed NPC removal.

Inspected Bevy captures in `work/shots/`: `combat_scout_01.png` (incoming scout
rounds, HP280); `combat_emp_20261007.png` (two scouts stunned, zero damage);
`combat_melee_20261007.png` (30+30+40 chain damage, scout HP100);
`combat_scout_death_20261007.png` (14 cannon hits kill HP200 scout, rounds then hit
the scout behind it); `combat_removed_20261007.png` (corpse removed after3s,
remaining scout HP50); `combat_death_20261007.png` (Bumblebee DeathSet, HP0);
`combat_reset_20261007.png` (automatic encounter reset, player HP320/scouts HP200).
Final integration capture `combat_moving_aim_20261007.png`: moving scout, player
cannon hits15, incoming scout hits2, frame player HP286/scout HP125 (log HP284 by exit);
rebuilt after connecting
aim assist to the scout's actual velocity.
These run the Bevy executable only, never the original game.

## Assumptions and stand-ins

No new fitted gameplay constants. Existing collision/animation assumptions remain
in their system the unpublished notes. Combat has no original recording to compare against.

- [stand-in] Arena enemy placement, engagement/steering and individual attack-delay
  bookkeeping. Full original brains/goals, tactics points, navigation, shared kung-fu
  slots and smoothed NPC aiming remain unported. The scout is a functioning opponent,
  not a claim that the original enemy AI is complete.
- [stand-in] Enemy target collision is still the arena cylinder with UpperBody surface;
  original per-limb surfaces/head hits are not represented by the host. The receiver
  has headshot/external-controller paths, but the sandbox does not create those inputs.
- [stand-in] Arena push supplies wall normal and estimates character contacts; original
  physics contact flags/body exclusions require a level collision host.
- [stand-in] Arena reset stands in for a mission reload, and --difficulty stands in
  for reading the original save's selection. Defaults to Medium; R/death preserve it.
- [stand-in] Enemy rounds draw arena tracers. Scout particle/sound-cue/footstep rendering
  is incomplete; actual fire sound event is requested through the installed library.
  Damage/stun/death feedback is animated rather than the old flash/blue/hide behaviour.
- Campaign death stats, energon drops, mission exclusions, multiplayer damage sharing
  and enemy vehicle/special/weapon-selection AI are outside this arena implementation.

## Damage objects of levels and missions (2026-10-16)

Ported in `core/src/damage_objects.rs` (12 tests). Data use (export/data/objects.tsv): DamageLink 2757,
PhysicsDamage 389, TriggerToDamage 261, DamageTrigger 57, GroupDead 53, HealAreaEffect 33, KillBox 17,
GroupHealth 2, CollisionDamageObject 1. Packet layout as these objects fill it [game]: point +0, normal +0x10,
impulse +0x1c, direction +0x28, amount +0x34, time +0x38 (game time, global_game_state +0xc), flags +0x3c,
attacker +0x40, destroy key +0x54, specialCaseDamageStuff +0x58..+0x6c (stun, slow, dot, heat, kbSpeed, kbAngle).

- TriggerToDamage (00744880, Trigger 007449a0): dmgTarget, dmgAmount (100000), dmgType (INSTANT_KILL; negative
  = all bits; mask 0xf27fffff). On Trigger: amount/flags/time to the target, or to every character of a
  CharacterSpawnManager target.
- CollisionDamageObject (007d7aa0, contact 007d7cc0): enabled by message 43990685 (the attack collision
  event); a PUNCH contact at most every 0.5 s; direction owner -> contact; damagePoints < 0 = push 10000, no
  damage; knock-back 20 at 45 degrees.
- KillBox (007f0ae0, contact 007f0810): damage 1e6, INSTANT_KILL, damageTarget mask 0xf (1 player, 2 NPC,
  4 TeamNeutral NPC, 8 non-character), autoTrigger; direction box -> contact.
- PhysicsDamage (007ff110, contact 007ff650): baseAmt (10, replaced by the owner projectile's damage), body
  speed >= 1 m/s, direction = body velocity, impulse = (owner -> hit) * baseAmt * 100, flags 0x400|0x800
  (+0x200 from a MissileProjectile), breakable Destructable 100000, Water key a6ea6ec8. Self kill once
  (dmgSelf > 0) within damageSelfRadius, not through a passThruDestructables hit: 100000 INSTANT_KILL.
- GroupDead (007ea290): arms when the group first has a live member; then once when all are dead (re-arms
  when one lives), or with triggerPerDeath once per death (group +0x64, raised by character_kill and
  destructable_damage_apply through 007e9f90) and stops when none lives.
- GroupHealth (007ea7b0): total max health of the members at the first update with members; mark starts
  at 1; each update f = sum(current)/total; mark += step once if f > mark; one send if (f <= 0 and mark > 0)
  or mark - f >= step, then mark = max(mark - step, 0).
- Low-health screen effect (0072c750 from 0071dc20): on below max * 0.35, off above 1.1 times that.
- Status FX: 0072c120 / 0072c340 are the DISABLE_VEHICLE / DISABLE_TRANSFORM effects (FX_stun plus stun
  sound b6336c69 tied to the character), 0072adb0 stops FX_hit_null_ray.

Not ported: target resolution and trigger sends (mission runner), GroupHealth's HUD, PhysicsDamage's
re-orientation on death (007ffd80: rows lost), the stun sound b6336c69 in the port's status FX,
StateThrown (the victim of a grab; no grabs in the port). Uncompared: no recording.
