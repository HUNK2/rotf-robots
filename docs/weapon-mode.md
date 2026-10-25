# Weapon mode: research notes

Addresses are for the analysed `Transformers2.exe` and stay in these notes.

Status (2026-10-03): StrafeMode (weapon mode's movement on foot) is read, ported and
matched to run 1. The animation controller's order of work and its two rule walks are read
and ported for the weapon set. The weapon's heat meter is read. Weapon mode in the air
(the jump mode's weapon branch and the set's air clips) is read and ported, with NO
recording to check it against: see "Weapon mode in the air". The firing side's trigger,
rate, refusal and scatter are read and ported ("The firing side").

Status (2026-10-04, end of day): everything else of weapon mode is read and ported too,
each in its own section below: the aim and the weapon camera, the aim point, the round,
the firing state, the gun in his hand with its clips, the axe, the lock-on and the
missile with its explosion, and last "The car's gun", "The special ability", "The aim
assist" and "Loose ends closed" (which also CORRECTS where a round is traced from). What
is still open is listed at the very end, under "What is left".

## StrafeMode (class 3c956e02, table `00b6289c`)

Object: `+0x18` speed and `+0x1c` heading (code-driven only), `+0x20` code driven, `+0x21`
a byte the constructor sets to 1 and nothing else writes, `+0x24` the spine's twist,
`+0x28` its tuning (`modeStrafe`: `useCodeDrivenStrafing`, `strafeSpeed`; Bumblebee false
and 20). Run 1 reads `+0x20` = 0 and `+0x21` = 1 throughout.

Enter (`FUN_00876990`):

- `+0x20` = `FUN_00876fe0`: true for a character no player controls, else the data's flag.
- `I_IS_STRAFING` = 1.
- Code driven only: speed = min(his speed, the strafe speed), heading from the stick.
- Unless the mode before is the dash (517c0a3b) or the jump (6da1d12d): the aim heading
  (owner `+0x25c`, and the vector at `+0x260`) becomes his own facing; `FUN_008545e0` only
  normalises the vector.
- Stick squared at least 0.0001: `F_STRAFE_ANG` = the stick's heading minus the aim's, in
  degrees, wrapped to half a turn either side.
- Owner `+0x3e4` = 1; twist 0.

Update (`FUN_00875ef0`). The stick comes from the input source in his own frame and is
turned by his matrix. The stick counts as zero while a StateAttackSpecial with value 4
runs.

- Aim yaw `+0x25c` = atan2(-x, y) of the aim vector.
- Stick squared under 0.0001 (or `+0x21` clear), "standing": off = aim yaw - body yaw,
  wrapped. `F_STRAFE_ANG` = off in degrees. If |off| > 0.79412484 (45.5 degrees) his
  matrix is turned by what is past it. The twist is off clamped to that. For a character
  no player controls (`FUN_00854d50`) it is smoothed (`FUN_00876950`: 0.2 of the new, 0.8
  of the old). Twist -> `FUN_0084e4f0`. Control vector (1, 1, 1 if his height over the
  ground is within 0.01, else 0).
- Otherwise, "moving": his matrix is rebuilt from the aim yaw (`FUN_0056fc50`, keeping
  his scale), so he faces the aim at once. Code driven: the heading goes to the stick's at
  10.47 rad/s (600 degrees), the speed from `FUN_008770d0`, and he is moved by heading *
  speed * dt. Not code driven: nothing moves him here. `F_STRAFE_ANG` = stick heading
  minus aim yaw, degrees (`FUN_008771c0`). Twist 0. Control vector (1, 1, 0).

Exit (`FUN_00876ea0`): `I_IS_STRAFING` = 0, twist 0, `F_STRAFE_ANG` = 0, `+0x3e4` = 0.

The twist goes to an object with a count at `+0x58` and a matrix at `+0x28`: a turn about
the vertical of twist / count. His pack has one `SpineIK` (attachment on node 7) with
`links` `Spine_02` and `Spine_03`. The SpineIK code itself is not read: that the count is
the links and each gets its share is assumed.

## The weapon set and the animation controller

`cargo run -p tf2-core --example rules_dump -- WeaponSet` prints the set's rules.

- Eight runs (`Weapon_RunFwd`, `FwdL`, `Left`, `BackL`, `Back`, `BackR`, `Right`, `FwdR`)
  and a `Prev` twin of each (`usePrevAnimFrame`: starts at the time the clip before had
  reached). Each is 50 ticks and travels 16.67 (20 a second) along a root turned its own
  way; the pose under the root carries the hips' turn, and the head stays on the aim
  (`facing_dump`). Blend in 0.3 s.
- A run's exit rules go to the `Prev` twin of the sector `F_STRAFE_ANG` is in (45 degrees
  each, edges at 22.5, 67.5, 112.5, 157.5) or to `Weapon_Idle` when the stick is under
  0.01. `Weapon_Idle` (blend 0.25) goes to "any" when the stick is 0.01 or more, which
  walks the set's entry rules (the same sectors, to the plain runs); to `Weapon_IdleTurnL`
  at `F_STRAFE_ANG` >= 45 and `R` under -45 (20 ticks, root turns 67.5 degrees).
- The air half: `Weapon_JumpTakeOff(_L, _R)`, `JumpRising`, `Weapon_Fall`, and from the
  fall the landings `Weapon_FallLand`, `FallBigLand` (`F_FALL_DUR` >= 1) and four
  `...2Run` ones by 90 degree sectors (back from 100), which hand over to the run with a
  0.07 s blend (`xfadeTimeOnExit`). See "Weapon mode in the air".

The controller (corrects `animation-rules.md`, which had the entry walk's filters wrong):

- Each update `FUN_0084e3c0`: forget the worked-out variables (`FUN_0070c2b0`); for each
  of three layers with a clip, walk its exit rules (`FUN_0070d540`); the first that fits
  gives a set and a clip to `FUN_0084ddb0`.
- `FUN_0084ddb0(set, clip, layer)`: with a clip, play it. Without: walk the set's entry
  rules with the set and clip playing on that layer (base layer `FUN_0070acc0`; other
  layers `FUN_0070b710`, which also has a pair of filters for what the base layer plays,
  filled from nothing seen in the data). An entry's two filters are FROM set and FROM
  clip (0 fits anything); its `toAnimSet` and `toAnimID` are the result. A result with no
  clip walks that set's entries in turn.
- A variable nobody registered compares as the float 0.
- `FUN_0084dfa0` is the message handler that starts the weapon layer's clips: 68479e46
  (overheat) sets `C_WEAPON_ACTION` 8d1faff8 and plays set 84ff5f30 on layer 1, only in
  control mode 2 (weapon). Not ported.

## The aim and the stick (read 2026-10-04; `core\src\aim.rs`)

The aim is the movement owner's unit vector `+0x260` (its heading at `+0x25c`). Setters:
`FUN_00854320` and `FUN_00854350` (a vector), `FUN_008543a0` (a heading, keeping z).
`FUN_0071af10` (called by `FUN_0071dc20` with the update's time) runs each update: in
control mode 2 (weapon) it calls `FUN_0071b990`; otherwise `FUN_00854350` hands the aim a
vector. Originally assumed to be the camera's direction from weapon entry in run1;
raw `0071af5c..0071af62` checks it as the object's forward on2026-10-07. Weapon entry
itself puts the aim on the camera's direction, which explains that recording.

`FUN_0071b990(character, dt)`, with no target locked (listing `work\asm\cam3\0071b990.txt`):

1. s = the camera stick (input source slot `+0x28`); pushed = |s|^2 > 0.001. The other
   stick (slot `+0x24`) only matters for the pull toward a target.
2. Aim assist (`+0x23c`, not sniping, `FUN_00709870`): over a target the factor `+0x324`
   goes toward `cameraSpeedReduction` (0.75), else toward 1, at `cameraSpeedAcceleration`
   (4) a second; it may also name a target to pull toward.
3. Not pushed: the ramp `+0x328` = 0 and, unless the other stick is pushed, nothing else.
4. Pushed: m = |s|; for a player not sniping (`FUN_00711210`: input source type 0 or 2),
   m = m^3 when m > 0.8 (`FUN_00475850` with 3), else m * 0.3125. The ramp rises toward m
   by `aimWeaponFormAcceleration` * dt and drops to m at once; never over 1.
5. rate = lowest + (highest - lowest) * the aim sensitivity option (options `+0x1f44`).
6. yaw = atan2(-x, y) of the aim; yaw += (s.x / |s|) * ramp * `+0x324` * rate * sign * dt,
   wrapped. The sign is +1, or -1 when `FUN_00913340` is false (input types 0 and 2 only).
7. The pull: with a target from step 2, the other stick pushed and |s|^2 < 0.81, the yaw
   goes toward the target's by `lookAssistTurnRate` * dt.
8. pitch = atan2(-z, |xy|) (down is positive); pitch += `+0x324` * (s.y / |s|) * ramp *
   rate * sign * dt, kept in [ZDirMinAngle, ZDirMaxAngle]. Sign -1, or +1 when
   `FUN_00913310` is false.
9. aim = (-sin yaw cos pitch, cos yaw cos pitch, -sin pitch), then `FUN_008545e0`.

With a target to turn to (the handle at character `+0x27c`: NOT the missile's lock but the
target taken as weapon mode starts, see "The aim assist") the aim turns to it at 4.712389
rad/s in both angles. Last, in every
case, the pitch (within 1.134464, 65 degrees) is handed to an object of the animation as
a side (0 down, 1 up; slot `+0x20`) and an amount 0 to 1 (slot `+0x1c`): the additive
aim clips of the weapon layer.

`AimAssist` (shared tuning; loader `FUN_0079bea0`, base `00d40c54`): every angle and BOTH
sensitivities AND the acceleration are multiplied by pi/180. His values: 85 and 360
degrees a second, acceleration 144 (2.51 a second, so a full push is let through after
0.4 s), pitch -85 to 85. Sniping has its own five.

In the port: `aim::Aim` (`step`, `set`, `direction`), `Tuning::aim`; the game steps it
with the look stick in weapon mode and keeps it on the body's direction otherwise
(corrected 2026-10-07, below).
Steps 2 and 7 and the turn to a target are in since later the same day ("The aim
assist"), and the pitch is handed to the aim clips. The sensitivity option's 0.5 is the
options menu's default [game: `FUN_008e21a0` sets the global `00dad8a0`, which is options
`+0x1f44`, to 0.5]. As weapon mode starts the weapon state's enter puts the aim on the
camera's forward itself (`FUN_0071cad0`) [game]. Outside weapon mode the direction is
the body's forward [game: 0071af5c..0071af62]. Assumed: the sign of the pitch after
the host input-axis mapping. Not ported: sniping.

## What run 1 shows (`python work\strafe_trace.py <trace> <t0> <t1>`)

Ten stretches of StrafeMode; 489.79 to 502.8 has every case.

- Steady: 20.00 exactly, at 0, 90, 180 or -90 degrees from the aim; his body yaw equals
  the aim yaw whenever he moves.
- A start from standing (490.31, 493.86): 2.13 straight ahead, then 4.39 at 29.4, 6.64 at
  40.5, 8.71 at 48.0 ... 19.34 at 88.0, 20.00 at 90. That is one update of the forward
  clip (weight 0.032 / 0.3) and then the left clip fading in over it: the rules saw
  `F_STRAFE_ANG` as the standing branch left it (the twist, about 0) and the stick
  already pushed. So the exit rules are walked before the mode's update writes the angle.
- A stop: 17.43, 14.87 ... : the idle fading in over 0.25 s.
- Through the middle to the other side (494.63): idle two updates, forward one, then the
  side clip: 17.43, 14.87, 11.16 at 78.8, 6.04 at 50.5, 5.09 at 5.5, 7.46 at -38.7.
- Entering at a run (489.79; body -3.8, aim 84.8): the body is at the aim on the first
  record, and the speed is 23.57 then 23.13 straight along the NEW facing (the run's 24
  fading under the forward clip's 20), and only the third update has the sideways clip.
  So the update the mode starts leaves `F_STRAFE_ANG` as the enter wrote it (stick against
  the old facing). Why is not read; the stick being kept in his own frame and a frame old
  there would do it.
- Standing with the aim moving: the twist equals aim minus body at once (no smoothing for
  the player).

The stick itself is not in the trace (the input block holds no stick); it is inferred
from the settled direction.

## In the port (`core\src\animrules.rs`, `sim.rs` `Mode::Strafe`, `tuning::RuleSet`)

The rules and clips are read from his pack (`formats\anim.rs`: `ExitRule`,
`IncomingRule`); `animrules` is the two walks; `step_strafe` is the mode's update with the
exit walk before it and the root motion after, the clips cross-fading in a stack as on
foot. Tests replay the start, the change of side and the entry at a run from run 1.
`control_check` runs a strafing script on the real data.

Assumed or stand-in, all marked in the code: the entering update's angle (above); what
fades out underneath when the mode starts (a dodge's clip plays on; otherwise the
velocity he had, held); a turning clip's turn blended by its fade; code-driven strafing
not built.

## Weapon mode in the air (read and ported 2026-10-03)

Run 1 has NO jump or fall in weapon mode (`python work\weapon_air_trace.py <trace>` lists
every stretch of JumpMode with WeaponSet playing or next to StrafeMode: none). An earlier
note said it had. So everything here is code and data, checked by tests and
`control_check` only.

The graph (`work\bb_machines.txt`, machine Weapon): `stateWeaponMode` -> jump pressed [5]
`stateWeaponModeJump`; falling [4] `stateWeaponModeFall`. Both are `StateWeapon` with
JumpMode. Jump -> `stateWeaponMode` after 0.2 s and on the ground; Fall -> the same on the
ground; either -> `stateFall` when WeaponMode is let go. `stateJump`, `stateFall` and the
dodge go to `stateWeaponModeFall` when WeaponMode is held in the air.

JumpMode with the weapon set playing (`FUN_00872fe0`, disassembly in
`work\asm\jump_update_a.txt`; the test is the base layer's set `+0x18` == `WeaponSet`
af5106dc, or `FUN_00721b50`: a state of class dc24f161 with value 4 on character `+0x314`,
not Bumblebee's weapon states):

- Facing: it does not turn him toward the stick. Each update it takes the aim yaw
  (`+0x25c` = atan2(-x, y) of the aim vector `+0x260`) and rebuilds his matrix from it
  (`FUN_0056fc50`, his scale kept), with the stick's step added to his place when there is
  one. So he jumps sideways and backwards facing the aim.
- Slow motion. The mode keeps a time factor (`+0x28`, 1 at enter and exit) and the height
  the slow motion began at (`+0x30`, -FLT_MAX at enter). Each update, with the weapon set:
  if no height is noted, it dilates while |vertical speed| < `jumpWeaponZVelStartDilation`
  (5) and notes his height; if one is noted, it dilates while he is ABOVE that height,
  whatever his speed. Dilating: factor -= `jumpWeaponDilationAcceleration` (4) * dt, not
  below `jumpWeaponDilation` (0.25). Otherwise: factor += 4 * dt, not above 1.
- What the factor does, with the value the update before left: the body's gravity is its
  own times the factor, and when that changes the vertical speed is rescaled to keep the
  height still to go (`FUN_006e7aa0`: speed * sqrt(new / old), sign kept); the mode's own
  dt is dt * sqrt(factor) (`FUN_00543fd0` is a square root), which scales the stick's step
  and the turn. So vertically time runs at sqrt(factor): half speed at 0.25.
- In numbers (`control_check`, "weapon jumps"): a running jump of height 10 slows from
  9.76 up, is at 0.25 for six updates over the top, and is back at 1 by 8.8 on the way
  down; 1.76 s in the air instead of 1.51. The standing jump (12 high) falls for 1.06 s
  instead of 0.83.
- Enter (`FUN_008740e0`): a moving jump out of StrafeMode with the stick over 0.99 is
  rescaled to the strafe speed (`FUN_008770d0`), as out of MoveMode to its top speed.
  Exit into StrafeMode caps the ground speed at the strafe speed (already in).

The fall time `F_FALL_DUR` (owner `+0x3f4`, written by the owner's update `FUN_00852220`):
in the air it counts real dt while the vertical speed is negative and is zeroed while it
is not; on the ground it is kept (zeroed only in ClimbMode). Real time, so the slow motion
lengthens it: the standing weapon jump's 1.06 s is past the long-fall rule's 1.0 and lands
with `Weapon_FallBigLand`. That is two updates past the line; whether the original lands
so is not known.

The set's clips in the air (`rules_dump WeaponSet`, entry rules in stored order):

- `StateWeapon`'s enter sets the mode first, then plays `WeaponSet` with no clip named,
  so the entry rules are walked from what was playing. StrafeMode's exit has by then set
  `I_IS_STRAFING` to 0. From a weapon-set clip: `Weapon_JumpTakeOff_L` with the stick 45
  to 135 degrees left of the aim (`F_L_STICK_DIR`), `_R` the same to the right, else
  `Weapon_JumpTakeOff`. From `JumpSet` or `FallSet` (aim taken up in a plain jump): always
  `Weapon_Fall`, rising or not. From anything while in the air and going down:
  `Weapon_Fall`.
- Take-off (10 ticks) -> `Weapon_JumpRising(_L, _R)` when finished; either -> `Weapon_Fall`
  once in the air and going down. `Weapon_Fall` loops and has no exit.
- Landing: `stateWeaponMode`'s enter (StrafeMode, `I_IS_STRAFING` 1, `F_STRAFE_ANG` from
  the stick) walks the entries from `Weapon_Fall`: `Weapon_FallLand` with the stick
  centred, else `...2RunFwd / Left / Right / Back`; the `FallBigLand` twins after a fall
  of a second.
- The landings that run on are left early only when the stick has moved to another
  sector; otherwise their last exit waits for branch count 2, the clip's
  `AnimBranchPointEvent` (tick 20 of 26, or 24 of 30), then the entry rules pick the run
  and it fades in over 0.07 s. The Rust reader had this field as 0 ("at any time"): see
  `animation-rules.md`.
- Root motion: the take-offs lift the root 1.5 and the rise drifts 0.5 forward; the fall
  has none. The landings that run on travel 6.7 to 8 along their way.

In the port: `sim.rs` `step_jump` (the factor, the facing, the set's clips through
`weapon_rules` and `weapon_layers`), `enter_state` (`StateWeapon` with JumpMode walks the
entries from `playing_clip`), `enter_strafe` (lands from the clip that was playing),
`State::weapon_air`, `dilation`. Three tests; `control_check` prints three jumps.

Assumed: the clips play at their own rate during the slow motion (whether the animation
player is slowed is not read); the clips' root motion is not scaled by the factor; what
fades out under `Weapon_Fall` when the aim is taken up in a plain jump is nothing.
`continueTimeOnExit` (true on the landings that run on) is read since 2026-10-04 (clip
starter `FUN_00748f00`): when the clip LEFT has it, the new clip is advanced, as it
starts, by the left clip's player's `+0xe4` (slot 6 of `lux::BasicAnimation`), which is
the time a clip that does not loop was asked to go past its own end (its update
`FUN_0053cc40` holds the time at the length and adds the rest there). So a clip that ran
out mid-update hands the unused part of that update to the next; a clip left at a branch
point hands on nothing. `usePrevAnimFrame` on the new clip wins over it. Ported
2026-10-06 in `animrules::Base` and weapon mode's separate `RootLayer` players;
elapsed loop clocks and sample phases are separate. An install-data regression test
checks the weapon landing-to-run handover.
The landing clips' sounds and shakes have not been compared with the original.

## The firing side (read 2026-10-03; part ported)

`handoff\result-01.md` has `FUN_007a7a40` as "Weapon::Fire". It is not: it only builds
the muzzle transforms (below). The chain, found from the state machine down:

- Firing is the overlay state `stateWeaponModeAttack` (`StateAttack` with `attackRanged`,
  `fireButton` 0x17 WeaponFire). Its update (`FUN_00878740`), each update, with the
  character's weapon manager (`+0x238`): button down -> `FUN_007b37f0` ("trigger held");
  else button let go this update (input slot `+0x10`) -> `FUN_007b3a20` ("released").
- The manager (current weapon index `+0x74`, weapons at `+0x34 + 8 * i`) passes that to
  the current weapon's slot `+0xc4` (held) or `+0xc8` (released) with the aim point
  (`FUN_007b34a0`) and the target (`FUN_007b36c0`), provided the weapon's slots `+0x30`
  and `+0x20` agree and three things about the weapon LAYER are not so (read later: they
  are the trigger's gate of "The weapon layer and the gun's clips": the layer playing
  `Trans_R2W`, or `Weapon_Switch`, or in its 0.133 s cut; not character states). A weapon
  with `isVehicleModeWeapon` (slot `+0xac`) also needs `FUN_007b44e0`: "The car's gun".
- Weapon classes: three tables share the base's routines. `00b5fa68` is the base (slots
  `+0xc4` and `+0xc8` are stubs), `00b5fba0` the gun and launcher class used here (set by
  the constructor `FUN_007ab580`), `00b5dae8` another (not looked at). No RTTI names.
  Slots of `00b5fba0`: `+0x08` update `FUN_007ac370`, `+0x48` muzzle transforms
  `FUN_007a7a40`, `+0x54` heat update `FUN_007a9700`, `+0x60` refusal sound
  `FUN_007a9dd0`, `+0x74` scattered aim point `FUN_007aa210`, `+0x80` make the projectile
  `FUN_007aaeb0`, `+0x8c` can fire `FUN_007a8da0`, `+0x94` trigger is down
  `FUN_007a8de0`, `+0xc4` held `FUN_007aca50`, `+0xc8` released `FUN_007acb90`, `+0xcc`
  fire `FUN_007ae010`, `+0x100` a round's heat `FUN_007a90c0`, `+0x108` ready
  `FUN_007ad080`.
- Attributes (loader `FUN_007bbc30`; `python tools\decomp_lookup.py fields 007bbc30`):
  `+0x10` numProjectilesPerFire, `+0x14` rateOfFire, `+0x18` automaticFiring, `+0x1c`
  burstPerFire, `+0x20` timeBetweenEachBurst, `+0x24` heatMeterCapacity, `+0x62`
  hasChargedShot, `+0xa0` alternatingFire, `+0xa1` useFireDummyMatrix, `+0xa2`
  useOwnerRootMatrix, `+0x11c` playerNoiseRadius, `+0x120` generalNoiseRadius, `+0x128`
  rumbleFireDuration, `+0x12c` rumbleFireStrength. Launcher (`FUN_007bd900`): `+0x150`
  guided, `+0x199` breakLockOnAfterFire, `+0x19a` mustHoldFireTriggerToLockOn.
- Weapon words: 0xd8 refusal sound wait, 0xf2 heat, 0xfb seconds since the last round
  (100000 in a new weapon), 0xfd and 0xfe burst count and time, 0x106 seconds the trigger
  has been held on, 0x107 "this pull has fired", 0xc9 muzzle transforms built, 0x15... the
  transforms (12 words each).

The rules:

- Held (`FUN_007aca50`), for a weapon without a charged shot and not a launcher with
  `mustHoldFireTriggerToLockOn`: if the heat is at the capacity or over, and it is
  automatic or the held time is 0: the refusal sound, which plays only when its wait
  (0xd8) has run out and sets it to the rate of fire (`FUN_007a8f20`, the rate with the
  player's upgrade). Then, if it can fire (`FUN_007a8da0`: heat under the capacity, or a
  linked weapon through slot `+0xdc`): fire.
- Fire (`FUN_007ae010`) first asks ready (`FUN_007ad080`): seconds since the last round
  MORE than the rate of fire; and, for a weapon that is not automatic and has no charged
  shot, held time 0 (unless it is that launcher). Then for each of
  `numProjectilesPerFire`: the aim point through `FUN_007aa210`, the projectile through
  slot `+0x80`, launched by `FUN_00784170` / `FUN_00784080`. After: 0xfb = 0, 0x107 = 1,
  the heat once (`FUN_007a90c0(-1)`, i.e. the weapon's own per round), the fire sound
  (slot `+0x58`), for a human player's pad `FUN_00717be0(1, rumbleFireDuration,
  rumbleFireStrength)` (the pad effect of `melee.md`, type 1), and the burst counters.
- Released (`FUN_007acb90`): only a charged shot or that launcher does anything: with held
  time over 0 and the heat at the capacity, the refusal; else if it can fire, fire. So
  Bumblebee's missiles (`mustHoldFireTriggerToLockOn: true`) leave when the trigger is
  LET GO after being held (the hold is the lock-on), never while it is held.
- The weapon's update (`FUN_007ac370`): bursts (0xfd > 0: after `timeBetweenEachBurst`
  another firing; his weapons have `burstPerFire` 1, so none); slot `+0x12c`
  (`FUN_007ad1c0`, not read: charge or lock-on); 0xfb += dt; then, if the trigger is not
  down for this weapon (`FUN_007a8de0`: control mode 2 and button 0x17 down, or the car's
  button 3, and it is the current weapon), or this pull has not fired (and no charged
  shot, and not that launcher): 0x106 = 0, 0x107 = 0, sounds stopped; else 0x106 += dt.
  Last the heat update.
- So the rate is quantised by the game's update: 0.15 s needs five updates of 0.032 s,
  a round every 0.16 s (6.25 a second, not 6.67); the cannon overheats on its 24th round
  3.68 s after the first.
- Scatter (`FUN_007aa210`), for a human player not sniping with `playerNoiseRadius` > 0:
  d = aim point - camera position; most = |d| * tan(fov / 2) * playerNoiseRadius (the fov
  is what `FUN_00557aa0` gives, in degrees; taken to be the camera's); angle drawn evenly
  from -pi to pi, way = camera up * sin + camera right * cos; k drawn evenly from -1 to 1;
  the point is the aim point + way * k * most. A disc in the picture plane whose size on
  screen does not depend on range. Otherwise with `generalNoiseRadius` > 0: a disc of
  that radius across the line from the muzzle (slot `+0x90`) to the aim point.
- Muzzle transforms (`FUN_007a7a40`): with `useFireDummyMatrix` the weapon model's fire
  dummies as they are; without (his cannon), a frame built from the owner: in weapon mode
  (control mode 2) with `useOwnerRootMatrix` clear, from the aim vector at owner
  `+0x260`; else from the character or a node 0xc8ec021e. `fireDummyXYAngle` and
  `fireDummyZAngle` turn each barrel's. Not ported.

In the port (`core\src\weapons.rs`): `Weapon::step` runs these rules in whole updates of
0.032 s (the trigger, ready, can fire, the refusal's repeat, the launcher on release, the
heat), `scattered_point` is the scatter, and the game crate starts the fire rumble. Tests
in `weapons.rs`. The trigger it is given is the firing state's (`State::firing`), since
2026-10-04.

### The weapon's model (2026-10-04; shown in the port, its clips not played)

- Each entry of the character's `weapons` is a file whose `obj` is the weapon's own
  `Object`, with the `GunWeapon` or `MissileLauncher` attached to it. His cannon and his
  launcher both carry the SAME model: 33 nodes, 27 rendered parts
  (`Bumblebee_Weapons.Body`, four sets of barrels, `Rear`, `MissleLauncherBase`, ten
  `MissleTube_*`), all in his pack. Nodes without a mesh: `FireDummy`, `FireDummy01`,
  `MuzzleFlashDummy`, `OverHeatDummy`. In the rest pose `MissleLauncherRoot` is scaled to
  0.28: the launcher is folded small inside the cannon. `weapons_check` prints it all.
- It is ONE object (the file `bumblebee_weapons`) with both the `GunWeapon` and the
  `MissileLauncher` attached to it, not two copies of a model.
- The object carries 14 clips of its own (`core\examples\weapon_pose.rs` lists them and
  prints the nodes at rest and in a clip): `Trans_R2W_WeaponA/B`, `Trans_W2R_WeaponA/B`,
  `FireA/B`, `SwitchToWeaponA/B`, `Overheat_WeaponA/B`, `Overheat_Weapon_A/B`,
  `Idle_WeaponA/B` (A the cannon, B the launcher). Read by `weapons::read_object_clips`
  into `CharacterData::weapon_animations`; NOT played. In `Idle_WeaponA` the cannon's
  parts sit as at rest and the launcher's root is at 0.47 instead of 0.28. Every clip's
  root matrix is a quarter turn about z; the robot's are the identity. Not applied here
  (the root's matrix is the object's own, which the link below overwrites).
- Which node of his it hangs from (corrected 2026-10-04, after the user saw the gun
  "sideways"): `startBoneAttachment` (weapon `+0x50`, looked up by `FUN_007a6380` into
  `+0x4c`, default `ObjectAttachment_Hand_R`) is only where the weapon is linked when it
  is made (cannon: `ObjectAttachment_Hand_R`; launcher: `WeaponAttachment_Arm_R`; the
  manager's `FUN_007b2b40` links each of its 8 weapons in turn with `FUN_007b4f20`). In
  play the clips move it: `Trans_R2W` (ticks 4 and 6), `Weapon_Switch` (2 and 4) and
  `Weapon_Recoil` (0 and 2) each carry a `WeaponAttachmentEvent` for the cannon and one
  for the launcher on the channel of `WeaponAttachment_Arm_R` (`work\attack_events.py
  <pack> WeaponPartialSet "" WeaponAttachmentEvent`). The event's handler (around
  `007b46f0`: fields `useEventOnlyIfWeaponIsCurrentlyUsed`, `hide`, `unhide`, `animID`,
  `hideOnAnimDone`, `animToPlayOnAnimDoneID`) calls `FUN_007b4f20(weapon, node)` with the
  node the event sits on, then hides or unhides and starts the weapon object's clip
  `animID` (`Trans_R2W_WeaponA`, `SwitchToWeaponA`, `FireA`...). The melee weapon's
  events sit on `ObjectAttachment_Hand_L` the same way. So out, the gun is on
  `WeaponAttachment_Arm_R`.
- How it sits on the node: `FUN_007b4f20` -> `FUN_0056c9b0` -> `FUN_0056c010` makes a
  `lux::LinkConstraint` (owner object, node number at `+0x60`, an offset matrix at
  `+0x28`, here the identity from `FUN_00557210`). Its update `FUN_0056c2f0` (run for
  every link by `FUN_0056c8d0`) gives the object: offset * node matrix * owner matrix
  (a sheared result is squared up and given the mean scale). So the gun has the node's
  own axes. `WeaponAttachment_Arm_R` has a channel of its own in the weapon clips: x
  along the forearm, z up with a roll of 11 degrees in `Weapon_Idle`; the gun's parts
  are modelled tilted by the same 11.25 degrees, and the two cancel exactly (the body's
  axis comes out at (0.005, 0.055, 1.0)).
- Shown and hidden by the weapon's slots 9 and 10 (`FUN_007a7630`, `FUN_007a7750`): they
  clear and set bit 0 of the object's `+0x60` (hide is skipped while word 0xf1 or 0xec is
  set). Who calls them (the manager's pull out and put away) was not followed.
- In the port (`game\src\weapons.rs` `attach_models`) the model is a child of
  `WeaponAttachment_Arm_R` with no transform of its own [game]. (Before, it hung on
  `ObjectAttachment_Hand_R` turned back by that node's rest pose, a guess that put it
  beside the arm and rolled.) Checked from above and behind
  (`work\shots\gunfix_new_75.png`, `gunfix_new_10.png`): on the end of the forearm,
  along it. It is shown while he is in weapon mode and hidden otherwise, at once, in its
  rest pose [stand-in for the events above: the pull out at tick 4 of `Trans_R2W`, the
  put away, and the object's own clips].

### The weapon layer and the gun's clips (read and ported 2026-10-04; `core\src\gun.rs`)

The data (`work\events_full.py <pack> <Set> "" WeaponAttachmentEvent` prints events with
every field; `layer_peek` and `offsets_check` in `core\examples` look at a layer clip):

- `WeaponLayer` plays the sets in its `animSets` (`WeaponPartialSet`); it has no node
  list. A layer clip moves the nodes it has channels for: the right arm and hand, and
  for some the neck, head or spine.
- `WeaponPartialSet`: `Trans_R2W` (84 ticks), `Trans_W2R` (48), `Weapon_Switch` (98),
  `Weapon_Recoil` (8, blend -1), `Weapon_Overheat` (160, a loop). Its entry rules are
  under `animPartialIncomingSetRules` (the decoder missed them before) and choose by
  `C_WEAPON_ACTION`: PULL_OUT_GUN -> `Trans_R2W`, SWITCH -> `Weapon_Switch`, OVERHEAT ->
  `Weapon_Overheat`, b90a70bd (unnamed; the recoil) -> `Weapon_Recoil`. Every exit leads
  to the empty set: when finished, or for the overheat at any time once the action is
  no longer OVERHEAT.
- `animPlayType` 1 (`Weapon_Recoil`, `Weapon_Overheat`, the partial hit reaction): the
  keys are OFFSETS. The recoil is no turn at tick 0 and a few degrees after; positions
  are zero. Laid over `Weapon_Idle` with each turn in the node's own frame the hand keeps
  its direction exactly while the forearm folds back 0.28, and the overheat raises the gun
  beside his head; taken in the parent's frame both come out skewed. So: own frame.
- Events on the arm clips, all on `WeaponAttachment_Arm_R`: `Trans_R2W` tick 0 fire off,
  4 cannon (unhide, `Trans_R2W_WeaponA`), 6 launcher (`Trans_R2W_WeaponB`), 8 fire on;
  `Weapon_Switch` 0 fire off, 2 and 4 (`SwitchToWeaponA` / `B`), 8 and 20 fire on;
  `Weapon_Recoil` 0 (`FireA`) and 2 (`FireB`), neither hiding nor showing. All have
  `useEventOnlyIfWeaponIsCurrentlyUsed`. `Trans_W2R` and `Weapon_Overheat` have none.
- The weapon's data names the rest: `idleAnimation` `Idle_WeaponA`, `exitAnimation`
  `Trans_W2R_WeaponA`, `exitCharacterAnimation` `Trans_W2R` (his arm's clip),
  `exitOverheatAnimation` `Idle_WeaponA`, `overheatAnimation` `Overheat_Weapon_A`,
  `fireAnimation` none, `alwaysAppear` false (loader `FUN_01185ae0`: weapon `+0x3ac`,
  `+0x3b0`, `+0x3b4`, `+0x3b8`, `+0x3bc`, `+0x3c0`, `+0x3c4`).
- The axe works the same way: the combat set's attack clips carry events for
  `MeleeWeapon` on `ObjectAttachment_Hand_L` (`Trans_R2W` then `Idle`, `Trans_W2R` with
  `hideOnAnimDone`). Not built.

The code:

- `FUN_0084dfa0`, the character's message handler for the layer, in weapon mode only
  (control mode 2): 68479e46 (overheated; sent by `FUN_007a90c0` / `FUN_007a7830`) sets
  the action OVERHEAT and walks the set's entries on layer 1; ab304c20 (a round left;
  sent by the fire routine `FUN_007ae010`) the recoil; f800db01 (weapon changed; sent by
  the manager) SWITCH; c29eb8df (cooled; sent by the heat update `FUN_007a9700`) clears
  the action if it is OVERHEAT; 409f0e77 ends the layer's clip over 0.133 s (the layer's
  `+0x94` = 0x3e083127).
- The event handler `FUN_007b4680(manager, event, node)`: for the weapon the event names
  (or, unnamed, the one at `weaponIndex`), if it is the one in hand or the event is not
  only for that: link it to the node (`FUN_007b4f20`), `hide` or `unhide` it (bit 0 of
  the object's `+0x60`; unhiding with no `animID` starts the idle clip), and send the
  object "play `animID`, then `animToPlayOnAnimDoneID`, hide when done". Two further
  cases apply an event for a weapon not in hand: a weapon whose slot `+0x30` says no
  (the melee weapon), and one whose slot `+0xdc` gives the id of the weapon in hand
  (not followed; taken to be a shared object).
- The object's clip player (the `AnimRefBundle` attachment, constructor `FUN_007c2ea0`;
  message 0805bf75 -> `FUN_007c37e0`): start the clip unless the "not if already
  playing" byte is set and it is the one playing; remember the clip for then and the
  hide flag. Its update `FUN_007c4170`: a blend-in weight; when a clip that is not a loop
  has ended, start the clip for then, and hide the object if asked. Otherwise it holds.
- The weapon's own starts (table base `00b5dae8`): slot `+0x10c` `FUN_007ab0f0` idle;
  `+0x110` `FUN_007ab1b0` exit (his arm's `exitCharacterAnimation` on layer 1, the
  object's `exitAnimation`, hidden when done unless `alwaysAppear`); `+0x118`
  `FUN_007ab320` exit overheat; `+0x11c` `FUN_007ab4a0` fire; `+0x120` `FUN_007ab3e0`
  overheat. Called by: the manager's `FUN_007b2eb0` on leaving the weapon states (exit),
  `FUN_007a9ec0` (overheat), `FUN_007a9700` (exit overheat), `FUN_007a9b40` (fire),
  `FUN_007aa070` (idle). Each also calls the same slot on a partner weapon (`+0x394`).
- The trigger's gate, in the manager's "held" `FUN_007b37f0`: the trigger reaches the
  weapon only if the weapon allows fire (slot `+0x20`: the `WeaponEnableFireEvent`s),
  and the layer is not playing `Trans_R2W` or `Weapon_Switch`, and is not in the 0.133 s
  blend (unless sniping). With fire allowed and one of the two clips playing, it sends
  409f0e77: the trigger ends the clip, and the weapon fires when the blend is over.

In the port: `gun::Gun` (`take_aim`, `put_away`, `fired`, `switched`, `overheated`,
`cooled`, `holds_trigger`, `step`), driven by `Arsenal::tick`; `animation::animate` lays
the arm clip over his pose; `weapons::show_models` shows and poses the one weapon object.
Five tests on his pack's lengths and events; captures `work\shots\gun_a.png` to
`gun_e.png` (`--orbit 100` swings the picture round to his side).

The layer's blending was assumed here at first and is read since (next section). Not
ported: the event case for a weapon that shares the object of the one in hand; the
object's own blend between clips; the launcher's separate look (one object is drawn,
posed by whichever clips ran).

### The aim clips, the layer's blending, the axe, the gun after the car (2026-10-04, later)

The four things the section above left. All read from disassembly and ported; checked by
tests, by node positions and by captures; nobody has seen them move.

**The aim clips.** Every clip of `WeaponSet` (all 36; no other set) has an
`additiveAnimFileList`: two `AnimFile`s, `Down` then `Up`, 4 ticks long, holding ONE pose
of offsets (no positions): `Spine_01/02/03` 15 degrees each, `Arm_L`, `Arm_R` 20, `Neck`
and `Head` 2.6. `aim_peek <Set> [Id]` (core example) lists them and prints where the
gun's node points with each laid on.

- The clip starter `FUN_00748f00(controller, animRef, layer, the animRef left, fade)`
  wraps the base layer's clip player in a `lux::MetaAdditiveAnimation` (`FUN_0053ebe0`;
  with no offset clips `FUN_0053eb10`): the clip player, a list of
  `lux::AdditiveAnimation` children (one per offset clip, each with its own player), a
  current child (`+0x34`) and a weight handed to every child. A new clip's wrapper takes
  the weight and the child from the one before (slots `+0x24`, `+0x28`).
- The aim's update `FUN_0071b990` ends by handing the wrapper the child (slot `+0x20`: 1
  when the aim points above level, else 0) and the weight (slot `+0x1c`: the aim's rise
  over 1.134464, 65 degrees, no more than 1). So `Up` is child 1, the list's second.
- `AdditiveAnimation::BlendOutputs` (`FUN_0053e180`), for a node both have: position =
  base + offset * w; turn = offset' applied in the node's own frame after the base's,
  where offset' is the straight blend from no turn to the offset by w, normalised
  (`FUN_00542ff0`: a + (b - a) * w the short way, then `FUN_00543e10`); scale = base *
  ((offset - 1) * w + 1). `MetaAdditiveAnimation::BlendOutputs` (`FUN_0053ed40`) copies
  the current child's output.
- Checked on `Weapon_Idle`: the gun's node points 2.5 degrees down with no offset, 66.9
  down with `Down` in full, 62.0 up with `Up` in full, and stays in its vertical plane;
  the head's forward goes 45 either way. (A turn in the parent's frame would skew it.)

In the port: `AnimRef::aim_offsets`, `aim::Aim::offset`, `pose::offset_turn` /
`offset_position` / `offset_scale` / `blend_turn`, `Rig::pose_offset`;
`animation::lay_aim` lays the clip on before the cross-fade, so it fades with its clip.
This also settles the aim's pitch as "up is `Up`"; the STICK's sign is still assumed.

**The clip starter's blend time** (same routine): the animRef's `xfadeTime`; the
`xfadeTimeOnExit` of the clip left if that is 0 or more and its
`xfadeTimeOnExitToAnimID` is 0 or this clip; the caller's if 0 or more; and 0.1 if the
result is under 0. So "no blend time given" is 0.1 s, not the 0.2 assumed before
(`DEFAULT_CROSSFADE` in `sim.rs` and `animation.rs`). `BlendAnimation::BlendOutputs`
(`FUN_0053d600`, cross-fades and layers) blends turns with `FUN_00542ff0` too.
AnimRef offsets: `+0x10` id, `+0x18` usePrevAnimFrame, `+0x1c` xfadeTime, `+0x20`
animSpeed, `+0x24` animFile, `+0x28` the offset clip list, `+0x34` loop, `+0x40`
animPlayType, `+0x44` continueTimeOnExit, `+0x45` allowEventsOnBlendFromThisAnim, `+0x48`
xfadeTimeOnExit, `+0x4c` xfadeTimeOnExitToAnimID (loader `FUN_0070ccd0`).

**The layers.** The controller's chain (`FUN_007482c0` builds it): the base layer's
cross-fade, then for each of the two layers above a `BlendAnimation` (`+0x74 + 8L`;
weight `+0x30` is the share of what is UNDER the layer) and an `AdditiveAnimation`
(`+0x64 + 8L`); a layer's clip goes into one or the other by its `animPlayType` (0
poses, 1 offsets).

- Start, `FUN_0074a180(controller, layer, animRef)`: the layer's clip is replaced at
  once; fade in time `+0x84` = xfadeTime, fade out time `+0x8c` = xfadeTimeOnExit; the
  clip starts at no share if xfadeTime is over 0, else in full. With no animRef (an
  exit rule leading to the empty set): if the clip left is a LOOP with xfadeTimeOnExit
  over 0, the layer fades out over it (`+0x9c`, counted in `+0xa4`); else it is taken
  off at once (`FUN_0074a610`).
- Update, `FUN_0074a970(controller, dt)`, with t the clip's time in seconds:
  cut by the trigger (`+0x94` = 0.133, set by message 409f0e77, which also zeroes
  `+0xa4`): off after 0.133 s or when a clip that is not a loop has ended, the share
  dropping with the time; else fading out after a loop was left: the same over `+0x9c`;
  else, for a clip that is not a loop with a fade out time, in its LAST fade-out
  seconds the share is what is left of the clip over that time; else while t is under
  the fade in time the share is t over it. A fade out never raises the share. Nothing
  sets the share to 1 when the fade in is over: it stays at the last value it gave.
- So `Trans_R2W` (84 ticks, 0.3 / 0.3) fades in over ticks 0 to 18 and out over 66 to
  84 and is gone as it ends; `Weapon_Overheat` (a loop) fades out over 0.3 s after the
  weapon has cooled, still playing; `Weapon_Recoil` (-1 / -1) is on in full for its 8
  ticks.

In the port: `gun::Arm::blend`. The assumptions the section above lists for the layer
are gone.

**The axe.** The third entry of the character's `weapons` (`bumblebee_meleeweapon`): an
object of 12 nodes (`Root`, `Weapon_Handle_01` to `06`, `Weapon_Blade_Center`,
`Weapon_Blade_L/R_01/02`), 11 parts, 2.0 long, a `MeleeWeapon` (`weaponName`
`MeleeWeapon`, `startBoneAttachment` `ObjectAttachment_Hand_R`, `exitAnimation`
`Trans_W2R`), a `RibbonTrail`, and three clips of 20 ticks: `Trans_R2W`, `Idle`,
`Trans_W2R` (`weapon_pose axe [clip] [tick]`). Class table `00b5fa20`.

- It is one of the manager's 8 weapons, never "in hand" (slot `+0x30`, `FUN_007bb820`,
  says no), so `FUN_007b4680` applies every event that names it. The events (all on
  `ObjectAttachment_Hand_L`): `AttackFast_1` tick 14 unhide, `Trans_R2W` then `Idle`;
  tick 36 `Trans_W2R`, hide when done. `AttackFast_2` and `AttackFastMP` tick 0 unhide,
  `Idle`; 42 / 46 `Trans_W2R`, hide when done. `AttackFast_3_Charging` (both) tick 46
  `Trans_R2W`. `AttackFast_3_ChargeAttk` tick 44 `Trans_W2R` with NO weapon name and
  index -1: it matches no weapon and does nothing.
- What folds it after the charge: `StateAttack`'s exit `FUN_00878be0`, when the next
  state is not a `StateAttack`, calls the manager's `FUN_007b2d80`, which calls slot
  `+0x28` of every weapon that is not a hand weapon. The axe's (`FUN_007a67b0`): if it
  is in use (`+0x38`, set by an unhiding event, slot `+0x1c`), play `exitAnimation`
  unless that is the clip playing, hide when done; with no such clip hide at once.
  The attack state's flag `+0x48`, which skips this, is `attackRanged` (read since: the
  two firing states do not fold the axe). `FUN_00879700` is the special state's exit
  (`StateAttackSpecial`, table `00b629cc`): it does the same only for special type 4.

In the port: `weapons::MeleeWeapon` / `read_melee`, `CharacterData::melee_weapon`,
`gun::Axe` (`events`, `put_away`, `step`), `Arsenal::tick_axe`, `weapons::show_axe`
(hung on the event's node with the node's own axes, as the gun). Captures
`work\shots\axe_y180.png`, `axe_y250.png`. The blade's `emissive` material was ported
2026-10-05 (`docs/shaders.md`). RibbonTrail added 2026-10-06 below.

**The gun after the car.** `StateWeapon`'s enter `FUN_0087f4a0`, when the state before
is neither a `StateDash` nor a `StateWeapon`: flag 0x100000; then, if the set the base
layer played until now is `Vehicle2RobotSet`, `FUN_007b2b40(manager, 1)` (every weapon
linked to its node again and enabled) and the weapon in hand's slot `+0x24` with 1
(`FUN_007a7630`: shown, idle clip started); no arm clip. Otherwise, if the weapon is
not out (slot `+0x2c`), `C_WEAPON_ACTION` = PULL_OUT, `WeaponPartialSet` on layer 1,
and the variable set back to 0. `FUN_007b4f20` stores the node it links to in the
weapon (`+0x4c`, slots `+0x3c` / `+0x40`), so `FUN_007b2b40` on every entry links each
weapon to the node it was on, not back to `startBoneAttachment`. The cannon and the
launcher share one object; linked in turn at the start it ends on the launcher's node,
`WeaponAttachment_Arm_R` [assumed: the manager's order is the data's].

In the port: `Gun::take_aim(clips, weapon, from_car)`; the game now takes the gun out
and puts it away by the control state's class (`StateWeapon`, or a `StateDash` with the
gun out), not by the button. `work\shots\guncar_b.png` (readout: gun out, no arm clip).

### The aim point, the round and the firing state (read and ported 2026-10-04)

- The aim point is written every update by the weapon manager's `FUN_007b2310` (its
  table slot at `00b5fb98`; argument: a flag that forces the trace). For a player in
  weapon mode (owner `+0x31c` == 2): the camera's matrix (`FUN_005a9cc0` ->
  `FUN_0059b370`: rows right, forward, up, position); far = position + forward * 500
  (`DAT_00cad46c`); the line starts at the point of camera..far nearest to the character's
  position (`FUN_0071a2e0`, his object's place; `FUN_006fa7b0` clamps to the segment), so
  nothing between the camera and him is aimed at. `FUN_005be890(start, far, mask 3,
  results, 0)` leaving out his own body (`+0x258`); the first result that is not a thing
  flagged 0x1000 (owner `+0x134`) without a character is the aim point (`+0x94`), with its
  normal (`+0xa0`) and the distance from the line's start (`+0x90`). Nothing hit: the far
  end, normal 0. Out of weapon mode (and for those the first test sends there) the point
  keeps the stored distance along the line from the muzzle (`FUN_007b45b0`, the weapon's
  slot `+0x90`) to a target at `+0x7c`. `FUN_007b34a0` hands the stored point out, except
  while a game mode of type 768ce890, d63697a6 or fc36eb23 runs (`FUN_0091ce20`; names
  not in the dictionary), when it is the muzzle + aim vector (owner `+0x260`) * 1000.
  The target (`FUN_007b36c0`) is a handle the current weapon gives when its slot `+0x88`
  returns 1. Ported: `weapons::aim_point`. [stand-in: the arena is what the line meets;
  the dummies are not in it]
- Launching (`FUN_00784170`) switches on the attributes' kind (their slot `+0x10`): 1 the
  missile (`FUN_00785de0`), 2 and 4 two others; a gun's round goes through `FUN_00784080`
  -> `FUN_007832b0`:
  - The round object is made at the muzzle; its speed (`+0x28`) is its forward *
    `bulletSpeed`, its start (`+0x6c`) the muzzle.
  - The trace's start is the round's place, the MUZZLE; only while SNIPING is it the
    camera's position. CORRECTED 2026-10-04 (later): the test here, `FUN_00721ac0`, is a
    thunk of `FUN_0117e860`, "the character's sniper camera (`+0x1e0`) is the camera in
    use", which this note first took for "a player's round". The character's four
    cameras sit at `+0x1d4` normal, `+0x1d8` weapon, `+0x1dc` driving, `+0x1e0` sniper
    (`FUN_0071aa50` frees them in that order; `FUN_0071ab70` switches to `+0x1d8` for
    control mode 2 and `+0x1dc` for 3), and Bumblebee's `sniperCamera` is 0. So his
    rounds are traced from the muzzle, and the port now does so (`weapons::bullet_line`).
    The way is towards the scattered aim point; the end (`+0x34`) is start + way *
    `bulletRange`.
  - With `tracerFirePerRound` (`+0x164`) under 1 and the round's flag `+0x81` clear, only
    `bulletSpeed` * the update's time is traced and the round flies on; otherwise the
    whole range is traced now. His cannon and car gun have 1: INSTANT. (So the old
    "treated as instant" stand-in was right, but it traced from the muzzle.)
  - `FUN_005be890(start, end, mask 2, ...)`; the result kept is the nearest that is not a
    0x1000 thing without a character; it gives the hit point (`+0x40`), normal (`+0x4c`),
    surface number (`+0x60`) and body part (`+0x64`, the body's `+0x254`). Then, being
    instant, the hit at once: `FUN_00782b30`.
- The hit (`FUN_00782b30`): the impact effect by surface (the round's `+0x7c` preset,
  `FUN_0077cd60`); damage = `damage` * the round's multiplier (the charge; 1) * the part's
  `positionalDamageMultiplier` (hashes `Head` 07c159a2, `UpperBody` 1f0107fa, `LowerBody`
  7f5eeed9, `VehicleBody` 2a39eb67; a head counts as a head only if the one who fired
  passes `FUN_00711210`, a player or the second controller kind, else as an upper body);
  with `bulletDamageFallOffRange` >= 0 and the hit at least that far from the round's
  start: `minDamage` + (1 - (d - falloff) / (range - falloff)) * (damage - `minDamage`),
  or `minDamage` at the range and beyond. The record handed to `FUN_00724700` carries
  `damageFlags`, `specialCaseDamageStuff`, the round's way, and a push of way * `damage`
  * 10. Ported: `weapons::bullet_line`, `weapons::bullet_damage`. His cannon: 15, head
  x 2, no fall-off (-1), range 300, `damageFlags` bit 9.
- The muzzle (`FUN_007a7a40`, more than the first reading had): barrels are the weapon
  object's own nodes `FireDummy`, `FireDummy01`, `FireDummy02`... (name table at
  `00c0675c`), from `minFireDummyIndex` to `maxFireDummyIndex` (weapon words 0xcc, 0xcd).
  Without `useFireDummyMatrix` (his) each barrel's PLACE is its node's and its axes are
  the frame built from the aim vector (weapon mode) or the character. With the
  attributes' `snipeCameraEnabled` (slot `+0xa4` -> `+0x5f`; not his) a player's barrels
  are the camera's matrix lowered by 1.5. His cannon: `FireDummy` at (0.786, -0.017,
  -0.054) in the weapon object, which hangs on `ObjectAttachment_Hand_R`; the launcher:
  `FireDummy01` at (1.080, -0.017, -0.054), on `WeaponAttachment_Arm_R`. In the port
  the muzzle only starts the drawn line and measures the fall-off, as in the original
  for a player. [assumed: the weapon object sits on its node with no offset; item c]
- The firing state: overlay states are how the machine runs it (`docs/state-machine.md`
  "Overlay states"). `stateWeaponMode`, `stateWeaponModeJump`, `stateWeaponModeFall`, the
  dash and the hit reaction on top all lead to `stateWeaponModeAttack` by
  `connectionWeaponModeAttack` (button 0x17 down, not holding anything); its only way out
  is `connectionReleaseWeaponModeAttack` (0x17 not down) with no output. So: the update
  the trigger goes down asks for it, the next enters it and calls "held"; the update it
  is let go calls "released" and asks to end, the next leaves. A jump or landing in
  between ends it with the weapon state and the new one takes it up again two updates
  later. Ported: `control::Runner` (`overlay`, `pending`), `sim::State::firing`.
- The car's gun is the same thing with another button: `stateWeaponModeVehicleAttack`
  (`fireButton` 3, AttackVehicle), entered from `stateDrive` and the change states by
  `connectionWeaponModeVehicleAttack`. AttackVehicle is bound to `DPAD_R_LEFT`, the left
  face button, the same as melee. So the car does NOT fire on the trigger that
  accelerates. [data]
- `FUN_0078d4a0` / `FUN_0078da10` are NOT "the instant bullet's raycast" as
  `handoff\result-01.md` has them. `FUN_0078d4a0` is the update of an object with three
  ways of being: flag `+0x29`, `FUN_0078da10`, which keeps it on the surface under it (a
  ray from above to below it along its own up, turning it to the surface's normal, ended
  when there is none): the attributes' `snakerInfo`; flag `+0x2a`, `FUN_0078ef60`; and
  neither: it is removed after three updates (a trace that only has to be seen).

Read since (see "Loose ends closed"): `FUN_007ad1c0` (the charged shot's update), the
"three states" (the weapon layer's gate), and the ranged attack state's enter, exit and
may-enter. NOT read yet: a round that flies (`tracerFirePerRound`
0: the round's own update), what `FUN_00724700` does with the record on the target's side,
the impact effect, what the fire does to the animation (`FUN_007236c0` messages). Not
checked against the original: no recording has any firing. Checked here by tests
(`weapons.rs`, `sim.rs` "the firing state...") and a capture
(`work\shots\fire_dummy.png`: 2 s of fire at a dummy, 13 rounds, 13 hits, 300 -> 105).

### The lock-on and the missile (read and ported 2026-10-04; `core\src\lockon.rs`, `weapons::Missile`)

All from disassembly (`work\asm\007b1bb0_raw.txt`, `007847e0_raw.txt`; pseudocode in
`work\007079f0.c`, `007847e0.c`, `00785de0.c`). `FUN_007ad1c0` (weapon slot `+0x12c`) is
the CHARGED SHOT's update, not the lock-on: it returns at once for a weapon without
`hasChargedShot` (attributes `+0x62`), which is all of his.

**The candidates: the character's aim-assist object** (owner `+0x23c`, made by
`FUN_00707860`; words: `+0x00`..`+0x14` three target handles, `+0x1c` the best one,
`+0x28` the owner, `+0x30` range, `+0x34` / `+0x38` the box's middle, 640 and 360 at
first). Its update `FUN_0070a0d0` runs the search `FUN_007079f0` and then makes the
first of the three that is live the best (`+0x1c`).

- The box (`FUN_0070a220`, `FUN_0070a2f0`): for a weapon of kind 1 (launcher) with a lock
  time over 0, half-sizes `missileLockOnBoxWidth` / 1280 and `Height` / 720 (the
  constants 0.00078125 and 0.00138889), times 640 and 360: a box of 128 x 128 pixels in
  a 1280 x 720 picture. Any other weapon: the shared `AimAssist.targetBoxPixelWidth` /
  `Height` (0.025, 0.0444), likewise times 640 and 360. Its middle is the picture's in
  weapon mode (the HUD's `FUN_007b8420` writes `+0x34` / `+0x38`: 640, 360, or in the
  other case the projected aim point).
- The range (`+0x30`) is set when a weapon is taken in hand (`FUN_007b2c60`): the
  weapon's `aimAssistMaxRange` (slot `+0x98`; his missiles 350), or with none
  `AimAssist.defaultMaxRange` (100).
- A character's test (`FUN_007088f0`; the list at `00c05cfc`): not himself, flag `+0x148`
  bit 3 clear, `thunk_FUN_0117e6d0` true (not read; taken to be "an enemy"); ahead of
  the camera (the way from the camera to its place . the camera's forward > 0); ahead
  of HIM (the way from his place to its . his object's forward row > 0); nearer the
  camera than the range. Then five points are projected (`FUN_00a98da0`, scaled by 1280
  / width and 720 / height): its place and its place + (0, 0, one size) for y, its
  place -/+ the camera's right * another size for x, and its `Head` node (or, in car
  form or with no such node, its place + 0.75 of the first size). The rectangle of the
  first four must overlap the box (`FUN_00707980`). Scores: (a) the squared distance
  from the box's middle to (halfway across, least y + 0.8 of the y range); (b) the
  squared distance of the fifth point from (640, 360). The list `00c067b0` holds other
  targetable objects, tested by `FUN_00709100` (not read).
- The three with the lowest score (a) are kept, but the one that is the best already
  (`+0x20`) goes first whatever its score. Each kept one then needs a clear line
  (`FUN_005be890`, mask 2) from the camera's place to its point: anything in the way
  that is not its own body, not his, and not a thing flagged 0x1000 empties its place.
  Nothing found: all three and the best emptied.

**The lock: the weapon manager's `FUN_007b1bb0(manager, dt)`** (manager words: `+0xb8`
the lock, `+0xc0` the candidate, `+0xc8` seconds held, `+0xcc` seconds of stay left):

1. No weapon in hand: nothing. A weapon that is not a launcher (attributes' kind slot
   `+0x10` == 1), not `guided` (`+0x150`), or held out of weapon mode (owner `+0x31c`
   != 2, unless `FUN_007b44e0`: the car's): seconds held 0, lock and candidate cleared.
2. `missileCrosshairLockOnTime` (`+0x14c`) 0 or less: the lock is the aim assist's best.
3. For a weapon with `mustHoldFireTriggerToLockOn` (`FUN_007ab550`): unless it is ready
   (slot `+0x108`, `FUN_007ad080`), its heat share (slot `+0xf4`) is under 1 and its
   trigger is down (slot `+0x94`), NOTHING is done: the lock, the candidate and the
   seconds held stay. That is how the missile let go of still has its target, and why
   a second pull on the same target locks at once.
4. The candidate is still one of the aim assist's three: seconds held += dt; at the
   lock time or over (`FUN_007ab4f0`), the lock = the candidate and the stay =
   `lockOnBracketStayOnTime` (`+0x19c`; 1 for a weapon of another kind).
5. Else (none, or it has left the three): the candidate becomes the first of the three
   that is alive (`FUN_00724180` false; an object: its `+0x52` clear). The lock is
   cleared if it is dead or flagged, if the stay is 0 or less, if there is a candidate
   and it is another, or if the way from the camera to its place is further from the
   camera's forward than 0.33 * the field of view (`FUN_00557aa0`, degrees; the whole of
   it for an `isVehicleModeWeapon`, slot `+0xac`). With no lock: seconds held = 0.
6. Every update: the stay, if over 0, -= dt.

What the missile gets: `FUN_007b36c0` hands out the manager's `+0xb8` when the weapon
in hand's slot `+0x88` (the attributes' kind) is 1.

**The launch (`FUN_00785de0(missile, aim point, target)`)**: damage words from the
attributes (`overwriteDirectDamage` / `ExplosiveDamage` / `ExplosiveMinDamage`, scaled
by an upgrade through `FUN_0079b770` for one kind of owner); the target's handle to
`+0x48`; the point to `+0x3c` (for a weapon that is not guided: 10000 along the line to
it); velocity = its object's forward * `missileStartSpeed`; guidance delay `+0x74` drawn
from `minGuidanceDelay`..`maxGuidanceDelay`, the faster-acceleration delay `+0x7c` from
`minAccelFactorDelay`..`maxAccelFactorDelay`; a timer event after 0.05 s (`ddf4c407`,
not read). The constructor (`FUN_007843a0`) has guidance on (`+0x80` = 1), "has been in
the cone" off (`+0x81`), the give-up timer `+0x78` = -1. The lifetime is another timer
event (`54732c8d` after `missileLifetime`).

**The update (`FUN_007847e0(missile, dt)`)**, in its order:

1. Wobble time left (`+0x60`) -= dt; when out, the wait for the next (`+0x5c`) -= dt.
2. Guidance delay -= dt; when out and guidance is on and `+0x78` is negative and
   `minGuidanceOffDelay` is 0 or more: `+0x78` drawn from the off-delay range. Then
   `+0x78`, if 0 or more with guidance on, -= dt, and at 0 the guidance ends
   (`FUN_00785690(0)`). His off-delay is -1: never.
3. `+0x7c` -= dt.
4. Once the guidance delay is out: the point is `+0x3c`, or with a target, a guided
   weapon and guidance on, the target's place + its up axis * half its height (an object
   that is not a character: its node's place, or a size its `+0x1d8` gives). c = the way
   to the point . its way (its velocity's, or its forward when still).
   - Guided, not yet in the cone, and c >= cos(`pastTargetConeAngleActivated`): `+0x81`.
   - `+0x81` and guidance on: c <= 0 ends the guidance flying straight on
     (`FUN_00785690(1)`); c > 0 with the target's damage object's `+0x94` over 0 ends it
     keeping the way to the target (that word not read).
   - Not wobbling: c > cos(`guidedViewAngle`) (both stored as cosines by the loader):
     its way IS the way to the point, at once. Else, unless nearly straight ahead or
     behind (|c| >= 0.999), it turns toward it by min(acos c, `outOfViewTurnSpeed` * dt).
   - Not wobbling, the wait out, and `minWobbleTimer` > 0: a new wobble (`FUN_00786440`):
     wait from `minWobbleTimer`..`max`, time from `minWobbleDuration`..`max`, an axis
     with each part from -1 to 1 (not made unit), a rate of + or - (0..`maxWobbleAngle`).
5. While wobbling: a turn of axis * dt * rate, made only if its square is over 0.0001.
6. Speed + `missileAcceleration` * dt while `+0x7c` runs, then `accelFactor` times that,
   capped at `missileTopSpeed`; velocity = way * speed; the object turned to its way.

`FUN_00785690(missile, straight)`: guidance off, the target let go; the point becomes
10000 along its forward (`straight`) or along the way to the target's middle.

His numbers: lock time 0.4, stay 1, box 128 x 128, range 350; start 70, top 140,
acceleration 70 for 0.2 s then x 2; view angle 15, turn 135 a second, past-target cone
30; wobble wait 0.2 to 0.8, for 0.02 to 0.2, up to 20 degrees a second; no guidance
delay, no off delay, no twirl; `breakLockOnAfterFire` false.

In the port: `lockon::candidates` and `lockon::LockOn` (the manager's rules in whole
updates of 0.032), `weapons::Missile::launch` / `step`; the game (`game\src\weapons.rs`)
runs the search over the dummies each frame, the lock while the launcher's trigger is
held, launches along the camera's forward at the aim point with the lock, and steers
each missile at its target's middle. A ring closes on the target being locked and goes
red when it is. Captures take `--fire-until s` (the trigger let go).

Assumed: which of a character's two sizes is height and which radius (by use); the two
"place" routines give one point; y runs down the picture; the field of view is the
camera's top to bottom; the picture is 16:9; the camera's forward stands for his aim at
the launch; the missile is moved by its new velocity; the lifetime's end removes it.
Stand-ins: the dummies (no head node: the 0.75 point; nothing but the arena hides
one); the lock's ring (the original's bracket is the HUD's); what a missile does where
it lands (a dummy it flies into takes `overwriteExplosiveDamage`; the explosion and its
radius are not read). Not read: `FUN_00709100`, `thunk_FUN_0117e6d0`, the two timer
events, where `breakLockOnAfterFire` and `guidanceDisabledWhenPastTarget` (`+0x190`)
are used, the target's `+0x234` -> `+0x94`, `twirl`. The aim assist's slow-down and pull
and the car's weapon are in since later the same day (their own sections below). The
aim does NOT turn to the missile's lock: the turn in `FUN_0071b990` is to another handle
("The aim assist").

**The explosion (2026-10-04)**: the missile projectile object (`missileProjectileObj`,
`micromissilescluster`) holds a `DamageLink` whose `damage` object carries a `DamageData`
(`destroy_type` 8590b19d) and a `DetachLink` to an object with an `Explosion` and a sphere
(radius 5, preset `Explosion`), a light, a sound player and particles (`w_exp_micro_missiles2`).
The explosion: damage 8, `minDamage` 1, fall-off from 0.75 of the radius, start scale 0.5 over
0.1 s, no delay, throw 30 at 15 degrees, flags `DAMAGE_BY_NPC | DAMAGE_BY_EXPLOSION` (0x804:
NOT a throwing kind, so nothing is thrown), `damage_player` on, not self, no camera shake. Its
rules are the ground punch's (`docs/melee.md` "The ground punch's blast"). The owner
routine `FUN_007e6af0` has a branch for a missile (owner record kind 1, `FUN_00745e70`): the
explosion's damage becomes the launch's word `+0x88` and `minDamage` its `+0x8c`, each only if
0 or more: `overwriteExplosiveDamage` 75 replaces the 8, `overwriteExplosiveMinDamage` -1
leaves the 1. The lifetime's end (`FUN_00785c00`, the message `54732c8d`) is a kill: it sends
the missile's own damage object 100000 damage of that destroy kind, so a missile that runs
out its time ALSO explodes, where it is. Ported: `weapons::MissileBlast`,
`scene::read_projectile_explosion`, `WeaponDef::explosion`; a touch or the lifetime's end
makes the explosion, a dummy takes its damage by distance. Direct damage (`overwriteDirectDamage`,
his 0) is not applied. Not read: what makes the missile end on a touch (the collision
handler: [assumed] any contact), the release of the detach objects, the missile's own
`Collision` mask. Not ported: the light, sound, particles (`w_exp_micro_missiles2`), the
scorch (`SSDImpactPreset`; ported 2026-10-06, `docs/surface-damage.md`). The sphere is drawn as a gizmo [stand-in]. Checked by two tests
and `weapons_check` (the data reads: 5, 0.5, 0.1, 8, 1, 0.75, 0x804, 30, 15); a capture of it
was not obtained (the shot flew into the sky).

Checked by 6 tests (3 in `lockon.rs`, 3 on the missile in `weapons.rs`) and captures
(`work\shots\lock_a.png` to `lock_d.png`: locking, locked, the missile in flight, the
dummy at 225 of 300). NOT compared with the original: no recording has any firing.

### The car's gun (read and ported 2026-10-04, later; `weapons::VehicleWait`, `facing_target`)

The data: the third entry of his `weapons` is a `GunWeapon` `VehicleWeapon` with
`Bumblebee_VehicleGunAttribute` (`isVehicleModeWeapon`, `useFacingAsTarget`,
`numProjectilesPerFire` 2, `rateOfFire` 0.2, damage 7, range 500, `tracerFirePerRound` 1,
heat 5 a firing of 100, falling 50 a second after 0.7 s, overheat 5.2 s, scatter 0.01666,
rumble 0.35 s at 0.5). Its object: three nodes (`Root`, `FireDummy` at (-0.137, 1.137,
0.188), `FireDummy01` at (-1.777, 1.136, 0.188)) and one mesh
(`Bumblebee_VehicleWeapons`); `startBoneAttachment` is the CAR's node `WheelWell_Front_R`;
barrels `minFireDummyIndex` 0 to `maxFireDummyIndex` 2, of which the object has two.
The character's attributes add `vehicleWeaponTimeTilCanFire` 0.75 (`+0xf0`).

The code:

- The state is `stateWeaponModeVehicleAttack` (machine DriveAttacksAdvance): an overlay
  `StateAttack` with `attackRanged` and `fireButton` 3 (AttackVehicle, on `DPAD_R_LEFT`
  with melee and the dodge), entered from `stateDrive` and the four drive-enter states
  by `connectionWeaponModeVehicleAttack` (button 3 down) and left by
  `connectionReleaseVehicleWeapon`. Its update is the firing state's: `FUN_00878740`
  gives the button to the manager's "held" / "released".
- Which weapon is in hand: the control mode's setter `FUN_0071ab70` ends with the
  manager's `FUN_007b43b0(manager, new, old)`. Into mode 3 (the car's; set by the drive
  state's enter `FUN_0087ba10`, so from the START of the change into the car):
  `FUN_007b3ea0` steps the weapon in hand (`+0x74`) on to the next that is a hand weapon
  whose `isVehicleModeWeapon` matches the mode, remembering the one before in `+0x78`
  and showing the new one (`FUN_007b2c60` -> the weapon's slot `+0x24`), and the wait
  `+0xd0` = attributes `+0xf0`. Out of mode 3: every weapon's slot `+0x1c`(0), `+0x74`
  back to `+0x78`, the wait zeroed. The exits (the unfold states, the slam, the punch)
  all have `robotForm` 1, which their enter hands to `FUN_0071ab70`.
- The wait: the manager's update `FUN_007b1850` counts `+0xd0` down. The manager's
  "held" and "released" pass the trigger to a weapon with `isVehicleModeWeapon` (slot
  `+0xac`) only if `FUN_007b44e0`: he is in the car's form (`FUN_0070ab90`: the motion
  mode is the drive or the fly mode) and the wait is 0 or less.
- "The trigger is down for this weapon" (`FUN_007a8de0`, for the held time): in the
  car's form, attributes `+0x390` clear (not read which field; his is clear), button 3.
- Where it aims: for each projectile i of a firing (`FUN_007ae010`), a weapon with
  `useFacingAsTarget` (slot `+0xa8`, attributes `+0x60`) takes slot `+0x84`(i),
  `FUN_007aab70`: barrel i's place plus its forward row times 100 (`DAT_00b3487c`); a
  projectile past the last barrel uses the first. The barrels' axes are the character's
  own outside weapon mode (`FUN_007a7a40` without `useFireDummyMatrix`). That point is
  then scattered like any player's (`FUN_007aa210`) and the round launched and traced
  as the cannon's. So the two rounds fly side by side, straight ahead of the car.
- The manager's aim point (`FUN_007b2310`) is NOT used by this gun and is not even
  refreshed by the camera's line in the car: that branch needs control mode 2, or the
  weapon's flag 0x400 clear (`+0x340`, set in every weapon's constructor
  `FUN_0074ba20`).
- The attack state's enter for a ranged state (`FUN_008788b0` with `+0x48`): character
  flag 0x10; if the weapon in hand has no snipe camera and the character has a target
  object (`+0x108`), `FUN_007b2f00` writes the manager's target `+0x7c` (an AI's aim,
  jittered). No clip, no motion mode, no fight timer (the 4.0 at damage `+0x108` is
  only for melee). Exit (`FUN_00878be0`): the flags off; the axe is not folded.
  May-enter (slot 11, `FUN_0076c530`) and may-leave: always yes.

In the port: `State::car_mode` (control mode 3, by the state's class and `robotForm`),
`State::firing` for either firing state, `weapons::VehicleWait`, `facing_target`,
`WeaponDef::barrels`, `Arsenal` (`Car`, the car's weapons, `shoot`), the model hung on
the car's node and shown in car mode. Checked by a `sim` test (the overlay on the drive
state), a `weapons` test (the wait: 24 updates; the two parallel lines) and
`work\shots\cargun_a.png` (1.6 s of fire at a dummy 100 ahead: 7 firings, 14 rounds, 14
hits, 300 -> 202, heat 35%). NOT compared with the original; nobody has played it.
Assumed: the round is made at the barrel it is aimed from; the car's pitch is the
barrels' (its roll is left out). Not ported: the muzzle flash
(`w_muzzle_flash_vehicle_autobot`), the R1 switch between car weapons (he has one).

### The special ability (read and ported 2026-10-04, later; `sim::start_special`)

The data (`characterAttributesBumblebee`): `specialAbilityType` -1, `Cooldown` 23.5,
`Duration` 5, `SelfTrigger` on with `LuxScriptNameToTrigger` `specialAttack` (666a6ad0),
and for the car `VehicleSelfTrigger` on with `specialAttackVehicle` (1b5e3550); no weapon
fire name, no vehicle clip. On his own object four things answer to `specialAttack`: a
`SoundPlayer` (node 4, event e696ca03), an `ObjectSpawnerOnBone` on node 8 that spawns
`bumblebeestunshockwave` (an `Explosion`: damage 0, flag `DAMAGE_STUN`, `stunTime` 2.5,
`explosion_time` 0.25, `start_scale` 0.5; sphere 15), a `TriggerableParticle`
(`r_attack_emp_stun_bumblebee`) and an `EffectSpawner` (`r_empstun_bumblebee`: the mesh
`R_EMPStun_Bumblebee.EMPStun_Bumblebee`, a dome whose clip scales it 1 to 3 over 30
ticks, deleted after 0.5 s, fading from 0.4). The car's object has the same sound,
spawner and particles under `specialAttackVehicle`. Button: AttackSpecial (2) on
`DPAD_R_UP`, the top face button.

The code (`StateAttackSpecial`, table `00b629cc`):

- `stateAttackSpecial` is an overlay state reached by `connectionAttackSpecial` (button
  2 pressed, kept 0.5 s; priority 10) from Idle, Move, the dash, the weapon states, the
  drive states and BOTH firing states (machine GenericSpecial).
- May-enter `FUN_008799c0`: no while his damage object's `+0x84` (the cooldown) is over
  0 (and in a multiplayer mode that turns specials off).
- Enter `FUN_00879110`: for an overlay state, only `FUN_0072afd0(damage object)` (the
  rest is for type 4 and for the states that play a clip). `FUN_0072afd0`: the cooldown
  `+0x84` = `+0x88` = attributes `+0xbc` (`FUN_0072bc40`; scaled by an upgrade in single
  player, `FUN_0079d060`); the manager's `FUN_007b3d80` (fires `specialAbilityWeaponFireName`
  if there is one); in control mode 3 the car's switch and script (`+0xdc`, `+0xe0`),
  else `+0xc8`, `+0xcc`: with the switch on and a script, a trigger message (d5d636c1)
  with that name to himself; then a switch on the type, 0 to 5 (his -1: nothing); HUD
  messages; and `+0x8c` = the duration.
- The state ends at once: an overlay state with no transition that wins asks its slot 9
  (`FUN_00879950`: yes for an overlay special) and clears the state under it's `+0x34`
  (`FUN_00877d90`). So it lives one update.
- On top of a firing state it becomes that overlay's OWN overlay (the same two words on
  the overlay state), so the firing goes on.
- The damage object's update `FUN_0072a550` counts `+0x84` and `+0x8c` down; when the
  duration ends `FUN_0072b980` sends the scripts an "off" message (9caa723d), which
  nothing of his needs.

In the port: `control::Runner` (`nested`, the "done" rule, `ask_overlay`),
`sim::start_special`, `State::special_cooldown` / `special_left`, `Tuning::special`,
`Actions::spawners` / `vehicle_spawners` (each explosion spawner at its node's rest
place), `Blast::script`, `Hit::stun_time`, `ObjectDef::sound_players`; the game starts
the effects and the sound by the trigger's name and a stunned dummy turns blue. Checked
by two `sim` tests and `work\shots\special_a.png` (the dome, the sphere, the dummy hit)
and `special_b.png` (the dummy blue, its health untouched). NOT compared with the
original; nobody has played it. Assumed: the node's rest place stands for where it is
when the script goes off. Stand-ins: what a stunned target does (`stateStun`); the
particles' drawing, and `r_attack_emp_stun_bumblebee` needs actions and domains the
particle port does not run yet (`ClSh`, `VlDr`, `SpSu`, the `UVAR` renderer). Not
ported: the effects in the car's form (the stun and the sound work there), the upgrade
that shortens the cooldown, the HUD's cooldown meter.

## The aim assist (read and ported 2026-10-04, later; `core\src\aim.rs`)

On by default: the options menu's "defaults" (`FUN_008e21a0`, `FUN_008e1550`) set the
global `00dad8ad` to 1 with the two sensitivities at 0.5; every routine below returns at
once when it is 0. `AimAssist` (base `00d40c54`): `+0x08` cameraSpeedReduction 0.75,
`+0x0c` cameraSpeedAcceleration 4, `+0x10` lookAssistTurnRate 5 (stored in radians),
`+0x14` defaultMaxRange 100, `+0x1c` lockOnEnable false, `+0x1d` r2wLockOnEnable true,
`+0x20` / `+0x24` r2wLockOnConeAngle / ConeElv 30 (radians).

**The slow-down and the pull.** `FUN_00709870(assist object, &slow, &has target)`
(disassembly `work\asm\00709870_raw.txt`), called from `FUN_0071b990` for a player not
sniping. Needs the aim-assist object's best target (its `+0x1c`; "The lock-on and the
missile" has the search: for a weapon that is not a launcher the box is
`targetBoxPixelWidth` x 640 by `targetBoxPixelHeight` x 360, 16 pixels either way, and
the range the weapon's `aimAssistMaxRange`: cannon 300, missiles 350, car gun 500).

- The stick (`FUN_0070ab20`) squared 0.01 or less: slow, target.
- turn = wrap(0.75 x sign of the stick's x x the invert switch x the sensitivity's rate
  x dt): what one update would turn at the reduced speed. drift = the change of the
  target's bearing from the camera in one update: the bearing of (its point + (its
  velocity - his) x dt) less the bearing of its point; the point is its `Head` node, or
  its place + 0.75 of its height. If turn and drift have the same sign and |drift| >=
  |turn|: NOT slow (the aim may keep up with it), target.
- Else the stick squared 0.81 or less: slow, target.
- Else the assist object's `+0x24` (the squared distance of the target's point from the
  picture's middle, pixels) 576 or less: slow, target.
- Else: A = the stick laid in the picture (the camera's axes times the stick, turned a
  quarter about the camera's right), B = forward x (forward x the way to the point),
  which points from the target toward the middle. A . B over 0 (pushing away from it):
  neither; else slow, target.

`FUN_0071b990` then: the share `+0x324` goes to 0.75 (slow) or 1 by 4 a second, and
multiplies both turns; and with a target, the OTHER stick pushed and this one under 0.81
squared, the heading goes toward the heading from his place to the target's place by
`lookAssistTurnRate` x dt (the two compared as they are, not the short way round).

**The turn to a target on taking aim (`r2wLockOn`).** `StateWeapon`'s enter, from a
state that is neither a dash nor a weapon state, raises character flag 0x100000, puts
the aim on the camera's forward (`FUN_0071cad0`) and resets the assist's share to 1 and
the ramp to the camera stick's length (`FUN_0071ca70`). While the flag is up the
character's update runs `FUN_00715c50`: the game's target search (`FUN_00799cd0`, the
melee one's) from the camera's place along its forward, range the weapon in hand's
assist range, the two cones for the angles, other words 5, 100 and weights 1, 2.5, 1;
of up to five results the best-scored character that is not flagged (`+0x148` bit 3),
in clear sight from the camera, goes into the handle at `+0x27c`. `FUN_0071b990` with
that handle set: the aim's heading and pitch each turn toward the way from the camera
to the target's point by at most 4.712389 x dt (the heading's way takes its height from
where he stands), the stick unread; once the aim is within 0.9999 of the way, the flag
and the handle are cleared. With no handle its ordinary branch clears the flag, so the
search runs once.

In the port: `aim::assist`, `aim::snap_target`, `Aim::enter` / `step_assisted` /
`step_snap`, `AimTuning`'s new values; the game runs the assist's box search every
frame of weapon mode (`Arsenal::assist_target`). Eight tests in `aim.rs` (four of them
new); captures
`work\shots\snap_a.png` (before taking aim: the dummies right of the middle) and
`snap_b.png` (after: the nearest in the cone in the middle). NOT compared with the
original; nobody has played it. Resolved 2026-10-07: `FUN_0070a990` is a body velocity,
and the query weights are distance1/yaw2.5/pitch1. Assumed: the stick laid
in the picture is right x + up y; taking aim
with the button is the weapon state's enter. Stand-ins: the dummies as enemies (no head
node, no speed); the assist's target is the one the box held the frame before.

### Loose ends closed (2026-10-04, later)

- `StateAttack`'s `+0x48` is `attackRanged`. For a ranged state its enter, exit and
  may-enter are as "The car's gun" has them.
- The "three states that stop the manager" are the weapon layer's gate (the layer's
  set and clip `Trans_R2W` 62a65451 or `Weapon_Switch` e6ce2afc, or its cut running).
- `FUN_00721ac0` = `FUN_0117e860` = "sniping": a round is traced from the MUZZLE (above).
  `work\shots\fire_muzzle.png`: 2 s of the cannon at a dummy 35 ahead with the rounds
  traced from the muzzle: 11 rounds, 11 hits, 300 -> 135.
- The weapon's slot `+0xb8` is its flags `+0x340`; `+0xbc` is `snipeCameraEnabled`;
  `+0xc0` its byte `+0x40c` (charging); `+0xdc` `weaponParent`; `+0xac`
  `isVehicleModeWeapon`; `+0xa8` `useFacingAsTarget`.
- An overlay state that is left has the connection it came by reset (`FUN_00878370` ->
  `FUN_0084c520`), so a fire button's detector (on for 100000 s once met) has to be met
  again; the port did not do this and re-entered the firing state for an update every
  other update while the button was up (harmless: the trigger was not down).
- `continueTimeOnExit`: "Weapon mode in the air".
- The dummies are in the aim point's trace now (the original's mask 3 meets characters).

## What is left (2026-10-04, end of day)

- A recording to check any of it against: no recording has firing, aiming, a jump in
  weapon mode, the car's gun or the special. Nobody has played these in the port.
- Drawing: the beam, the muzzle flashes, impact effects and marks, the missile and its
  explosion's light, sound and particles (`w_exp_micro_missiles2`), the axe's
  `RibbonTrail`, the lock's bracket (the HUD's). These wait for the particle system's
  other actions and the effect shaders (`docs/particles.md`).
  Update 2026-10-06: flashes, impacts, missile particles and shader drawing were built
  in the particle sessions; persistent marks are `docs/surface-damage.md`; ribbon
  trails are now built below. Their remaining assumptions stay in the unpublished notes.
- The target's side: what a round, a blast or a stun does to the one it hits
  (`FUN_00724700`, `FUN_00719830`, `stateStun`, the hit reaction on top).
- Closed 2026-10-06: `continueTimeOnExit`, moving character cross-fades and the
  weapon object's own blends, including interrupted nested fades (`animation-rules.md`).
- Not read: the SpineIK code (the twist's split), `FUN_00709100` (objects the assist can
  target that are not characters), the missile's collision handler, `twirl`,
  `breakLockOnAfterFire`, attributes `+0x390`, the invert options' defaults (the signs
  of the stick), a round that flies (`tracerFirePerRound` 0; none of his).
- Not his, so not built: sniping, charged shots, bursts, code-driven strafing, special
  types 0 to 5.

## The heat meter (weapon object, word offsets: 0xe8 attributes, 0xf2 heat, 0xf3 wait,
0xf4 overheat time left, 0xf5 cooling after an overheat)

- `FUN_007a90c0(amount)`, a round's heat: the amount, or the weapon's own per round
  (slot `+0xfc`); sniping, between `minHeatMeterIncrement` and `maxHeatMeterIncrement` by
  the charge; a player's upgrade scales it. Heat += it. Under `heatMeterCapacity`: wait =
  `heatMeterCooldownTilDecrement`. Else: overheat time = `heatMeterOverHeatPeriod` (scaled
  by an upgrade for a player), wait 0, the overheat's start (slots `+100`, `+0x70`) and
  message 68479e46.
- `FUN_007a9700(dt)`, the update: overheat time running: count it down; at zero, if
  `heatMeterOverHeatCooldownRate` > 0 the heat stays and falls at that rate (flag 0xf5),
  else heat = 0; then the end of the overheat (slot `+0x68`, message c29eb8df). Else wait
  running: count it down. Else heat > 0 and slot `+0xc0` false: heat falls by
  `heatMeterDecrementPerSecond` (or the overheat rate while 0xf5).
- All three of his weapons have the rate at -1: empty after the period.
- What refuses the trigger while overheated: "can fire" (`FUN_007a8da0`) is the heat
  under the capacity, and the heat stays at the capacity until the period ends.
- Slot `+0xc0` (`FUN_007bb810`) is the weapon's byte `+0x40c`, which `FUN_007b01a0`
  sets while a charged shot is being charged: none of his weapons has one, so his heat
  always falls.

## Aim assumptions checked (2026-10-07)

- `0071af5c..0071af62`: outside weapon mode, ESI is character object's +0x1c,
  its forward row, passed to `00854350`. The old camera-direction assumption was
  wrong. The host now uses `State::body_forward()`; the camera's initial aim does
  too. Weapon entry still uses camera forward, as `0071cad0` specifies.
- `0070a990`: copies physics velocity XYZ at +0xa0/+0xa4/+0xa8 (or zeros). Its
  meaning in the moving-target slowdown test is no longer assumed.
- Snap query built at `00715c50` has distance/yaw/pitch weights at +0x34/+0x38/+0x3c
  =1/2.5/1. The score arithmetic `00799958..0079997f` confirms the mapping: distance
  normalization stored esp+0x14, yaw esp+0xc, pitch esp+0x10. The previous code applied
  2.5 to BOTH angles. Pitch now has weight1 and its own cone limit. Added a target
  competition test and an unequal-cones test; ordinary melee scoring is preserved.
- `00913310` and `00913340` read inversion bits4 and3 of 00dad890. Base init
  `01181b60` clears options +0x1f34. Inversion switches are therefore off initially;
  the original pitch-down variable uses +stick.y while this port's pitch-up variable
  uses its host y-up input. Update2026-10-09:docs/input.md traces the physical
  mouse through directional actions and the XInput-shaped bridge into engine
  normalization, confirming engine y-positive-down. The remaining host-axis/sign
  and assist picture-vector assumptions are separate; `0070a4b0` rotates via a
  matrix helper, and this input audit alone does not settle those questions.

## Melee ribbon trails (2026-10-06)

Loader `00790ef0`, constructor `0078ff30`, update `00790180`, draw `00790390`,
trigger/stop `00790160`/`00790170`; table `00b5eca4`. The axe links
Weapon_Blade_R_01, Weapon_Handle_06, Weapon_Handle_04 and Weapon_Handle_01,
using `bumblebee_meleeweapon.fx_melee1`, eight history rows, four subdivisions,
threshold0.5, disabled initially. The robot also has named arm/forearm trails.

Update accumulates 1/60 seconds but takes only one sample per update that crosses
the boundary. `triggerSpeed` is a displacement threshold against the latest row,
not a speed divided by time. Enabled/moving histories grow to numTrails; disabled
or slow ones shrink to three support rows. The latest linked world points are inserted
even during this decay. Drawing requires four rows and a visible owner. The draw
adjusts subdivisions by camera distance / (bounding radius*10), then makes a grid
across links and time, with two opposite-normal strips and normalized UVs. Its cubic
matrix at D40970 is initialized by `00ad9f60`: Bernstein/Bezier weights. Sampling
uses index clamp(floor(v*(rows-1)-1),0,rows-4), t=(v*(rows-1)-index)/3.
History/point utilities `007915f0`, `007917b0`, `007916b0` manage the arrays.

`AnimSendTriggerEvent` registration `00adda80` installs a timed handler;
`AnimExitTriggerEvent` registration `00addae0` installs that handler for both time
and player exit. Handler `0080dfd0` reads target `name`, LuxScriptName and triggerMsg;
name0 sends to the character through `00724940`'s listener list. AttackFast_1 starts
Trail_Forearm_R at tick10 and stops at26. AttackFast_2 starts Trail_Arm_L and
Trail_Weapon_L at18 and stops at32. The charged hit starts right trails at2, left
at8, stops left at40 and right at48. The air attack starts right arm at4, stops at20.
An interrupted clip also sends its stop messages.

Port: `core::ribbon`, `Clip::trigger_events`, `ObjectDef::ribbons`, `game::trails`.
World points are sampled after this frame's transform propagation; original emissive
material/texture and UV arithmetic are reused. Four tests include runtime link/material
resolution and attack keys, growth/decay, displacement and mesh endpoints/indices.
Assumptions: character messages are forwarded to the attached axe; exit stops occur
when the leading clip is replaced; normal tangents approximate the original neighbor
math (emissive ignores normals). Details and what settles them: status item3.

## Sniping (read and ported 2026-10-19; `core\src\snipe.rs`; core tested, game built, captured, not compared)

Who: the two weapons with `snipeCameraEnabled` that a hand holds, the Audi's PlasmaLongRifle
(damage 100, `maxSnipeDamage` 200, 0.9 s to start, 0.9 s to full, range 1500) and Breakaway's
LongBeamRifle (85, 255, 0.5, 2.7, range 1000), with their characters' `sniperCamera`
(`audir8SniperCamera` 8a47b5c5, `firestormSniperCamera` 9e6a66c2; Starscream also names the
second but has no snipe weapon). [data]

- In and out (`character_update` 0071dc20, before the manager's update): sniping ends
  (`FUN_00721ce0(char, 0, 0)`) when the weapon in hand has no snipe camera (slot `+0xa4`, the
  attributes' byte `+0x5f`), he is not in `StateWeapon`, or the options blocker / input gate is up.
  A human player in control mode 2 with a snipe weapon and a sniper camera (`+0x1e0`) who presses
  button 7 (`CameraReset`, R3; the detector's slot `+0xc` "pressed") toggles it. Going in:
  `SniperScope_Gfx` Activate, `camera_behaviour_switch` to the sniper camera, the character's
  vtable `+0x1c` with 0.2 [unread]. Going out: Deactivate, `camera_behaviour_remove`, the same slot
  with 1.0. `camera_is_sniping` (0117e860) = the sniper camera is the one in use. [game]
- The camera (`00929b20` loader: the weapon camera's `00927cd0`, then `zoomFOV` 5, `zoomFullTime`
  1.5 by default; class `CameraSniper`): `zoom_camera_fov_update` 00929d50 runs the weapon camera's
  update (00929470) and then steps `1/tan(fov)` by `(1/tan zoom - 1/tan default) / fullTime *
  input * dt` within the two ends (fov in degrees, the whole field). The input (behaviour `+0x78`)
  is the camera message's `+0x28` (0071a5c0): while sniping +1 with button 4
  (`CameraDebugFollowCloser`, D-pad up) held, else -1 with button 5 (`Farther`, D-pad down). It
  also sets the sound roll-off scale to fov / 60 (not ported). [game]
- The aim (`aim_stick_update` 0071b990 with `cStack_129` = sniping): the aim assist is not asked
  (the share `+0x324` goes back to 1), no fine-aim curve, the ramp at `aimSnipingAcceleration`
  (DAT_00d40c98), the rate between `aimSnipingLowest/HighestSensitivity` (c90 / c94) by the option,
  times `(1 - m) * (sin fov - sin 5deg) / (sin 15deg - sin 5deg) + m` within [m, 1], m =
  `aimSnipingSensitivityMultiplierOnFullZoom` 0.19 (ca4); the pitch within c9c / ca0 (-80 / 80).
  [game] [assumed] the fov read (`scene_camera_fov`, its return dropped) is the zoomed one.
- The shot: the weapon's clock `+0x454` (007ac370 l.30-40) runs while sniping and the weapon may
  fire (slot `+0x50`, [assumed] heat under the capacity), else only up to 0; a shot fired while
  sniping sets it to `-timeTilSnipeChargeStarts` (007ae010 l.505-512, after the round is made).
  The steady (007ac920, slot `+0x128`) is `(clock / snipeChargeDurationTilMaxEffect)^2` up to 1;
  at 1 the round is STEADIED (fire flag 0x2000) and takes `chargeMaxProjectileObj` (neither rifle
  has one). The round's damage multiplier (007ac760, slot `+0x160`, into `bullet_trace_setup`):
  `(steady * (maxSnipeDamage - damage) + damage) / damage` while sniping (out of it: the type-6
  special's upgrade scale or 1). No scatter (007aa210); the barrels are the camera's matrix lowered
  by 1.5 (007a7a40) and the trace starts at the camera (007832b0). [game]
- The reticle: slot `+0xbc` (0074f150) just calls `+0xa4`, so the sniper base and icon
  (007b7f40, 007b7560) show for a snipe weapon in hand whether sniping or not. Not ported.

In the port: `snipe::{clock_after, clock_after_shot, steady, damage_multiplier, zoom_step,
zoom_input, zoom_rate_share, must_leave, toggles, BARREL_DROP}`, `Weapon::{set_sniping,
snipe_steady, snipe_clock, snipe_steady_shot}`, `Aim::step_sniping`, `camera::Sniper` (the core
camera's `sniper`, used in place of the weapon camera's settings), `Tuning::sniper_camera`,
`AimTuning::sniping`; the game toggles it in `player::tick` and fires the round in `Arsenal::tick`.
[stand-in] `SniperScope_Gfx` is not drawn and the reticle keeps its gun base. [assumed] the zoom
starts at `defaultFOV` each time (the camera's start, vtable `00b6603c`, is not read); D-pad up
does not step the camera distance while sniping.
