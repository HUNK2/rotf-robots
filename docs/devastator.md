# Devastator as a playable walking giant (2026-10-22)

The game never lets anyone play him: he is the Cairo City boss, driven by his brain and the
mission. The port makes him playable from his own data with a small player binding in place of
the brain. `--character devastator` plays him. Code: `core\src\giant.rs` (binding, fit),
`core\src\character.rs` (his `ShockWave`s), `core\src\formats\characterlist.rs` (where his pack
is), `sim.rs` (`shockwave_hits`, the set-exit and attack-done changes), `game\src\shockwaves.rs`
(the rings drawn), `game\src\charmenu.rs` (calls `giant::reroot`, `giant::fit`, `giant::limbs`),
`game\src\animation.rs` (his limbs' IK). Tests: `core\tests\devastator.rs`. Look-around:
`cargo run -p tf2-core --example devastator_clips [-- --graph | --rules | --frames [--all] |
--turn <id> --body [--watch=A,B] | --ik | --limbs | --reroot]`.

## Where he comes from [data]

- No `characters\devastator.str`. His `CharDef_Devastator`, model (blob `572b354e`, 96 nodes, top
  of the bind pose 60.6 m), skeleton and animation set (data object `#1`) are in
  `levels\cairo_city\missions\cairo_city_a_boss\mission.str`; his sounds in that mission's
  `eng\mission.fev`. `characterlist::ELSEWHERE` names the pack, scene `devastator_basepose` and
  the sound file; `character::load` reads him from there.
- `characterAttributesDevastator` (global_states.txt 55331): baseHealth 100000, invincible,
  canWalkOnly, cannot drive or fly, mass 400000, avatar height 1 and radius 1, specialAbilityType
  -1, team Decepticon.
- His robot object carries two `ShockWave` attachments, on `F_Thumb_Base_L` (node 77, name
  3401044346) and `F_Thumb_Base_R` (node 78, name 817425433). Both: damage 25, stun 1,
  knock-back 15 at 35 degrees, heightCheck 2, radius 3 to 160 at 75 m/s, fade in to 20, fade out
  from 70, band 5 to 70, segments 25 to 100, material `devastator.fx_shockwave` (technique
  `emissive`, additive).

## His machine (`stateMachineDevastator`, global_states.txt 6484) [data]

| State | Class | Clip | How it ends |
|---|---|---|---|
| `stateIdle` | StateIdle | base layer | messages, `connectionMove` (priority 4), jump, fall |
| `stateTurnLeft` / `Right` | StateBasic, finishAnim + playAnimOnce | `TurnSet/TurnLeft` / `TurnRight` (240 ticks, 90 degrees) | `connectionWaitForAnimDone` to `stateMove` once the clip finished |
| `stateAttackDevastator` | StateAttack, animName 0, finishOnCombatAnimSetExit | `CombatSet` entry: `Idle2Attack1` (56) then `Attack1` (360) | no transitions: done once the layer leaves `CombatSet` (Attack1 exits to `IdleStandSet`), then the default `stateIdle` |
| `stateAttackSpecialDevastator` | StateBasic, finishAnim, playAnimOnce, finishOnAnimSetExit | `PlayAnimSet/Idle2AttackSpecial` (56) then `AttackSpecial` (726) | `AttackSpecial` exits to `WalkSet`: the set is left, `connectionIsFinished` to `stateMove` |
| `stateMove` | StateMove | base layer (`IdleToWalk`, `Walk`) | the same messages, `connectionReleaseMove` to idle |

The messages: `DevastatorAttack` 2874311729, `AttackSpecial` 3458736789, `TurnLeft` 2447847285,
`TurnRight` 3609696517. The shared jump, fall, hit-react and death states are in his list too.
`TurnSet/Turn180` (500 ticks) is in no state. His set has no Run, no jump, no vehicle clips.

The attack state's done is the game's `00878d20` [game], ported 2026-10-22 for every character
(it was missing: an attack state with no winning transition stayed in its finished clip). The
plain state's `finishOnAnimSetExit` now also sees an exit rule into a set the action player does
not play (`ActionState::left_set`), which the port used to filter out. All 1442 core tests passed
after both.

## The player binding [assumed] (`giant::Giant::ask`)

- Melee button (press): `DevastatorAttack`, the pound. Special button (press): `AttackSpecial`,
  the vortex.
- In `stateIdle`, a stick pushed past 0.5 and more than 45 degrees off his facing sends
  `TurnLeft` (stick to his left) or `TurnRight`, and the machine does not see the stick that
  update (else `connectionMove`, priority 4, would win). Under 45 degrees he walks, and the walk
  turns him at his `TurnLeft`'s 22.5 degrees a second (not `modeMove.turnSpeed`'s 360 [data]:
  "Made a behemoth" below).
- The jump states are switched off (`StateJump`): he has no JumpSet.

What would settle the binding: `brainDevastator` / `goalDevastatorBuildingAttack`'s code (when the
brain turns rather than walks), or a choice by the user.

## His clips, as the port moves him (re-rooted on his chest, 2026-10-22)

**Which frame the root path is in.** `Clip::root_path` is in the clip's start frame, and the
port applies it rigidly (turn and travel composed onto his object in the object's own frame),
which is what the game does: `00544e00` root mode 0 composes the root's key onto the object
[game]. Integrating the path the port's way keeps his feet planted, checked tick by tick. So the
sideways slide of the turns was not a frame mistake.

**Where the slide came from.** His clips are authored in two body frames [data
`devastator_clips --frames`]:

- walking, the root sits under his chest, about 21 m ahead of his hips (`Walk`, the end of
  `IdleToWalk`, the start of `Idle2Attack1` / `Idle2AttackSpecial` / the turns / the hit reacts);
- standing, it sits by his hind feet, hips at +5.3 (`Idle`, `Attack1`, `AttackSpecial`, the ends
  of all those).

Where a clip goes from one to the other the root crosses his body while he stays put:
`Idle2Attack1` ticks 52..56 by 35.2 m (only the root jumps, the body is continuous), and the
turns all through: `TurnLeft` pivots his body about his chest while the root swings 22.5 m. The
game's object sits at the root too, but nothing follows it; the port's position is what the
camera follows and where the next clip starts, so in the port he visibly slid.

**The fix [assumed port reference point]: `giant::reroot`** moves every clip's root onto his
chest (`giant::CENTRE` = `Spine_03`) before anything is measured on the clips: the new root key is
`p + R h` (h = the chest's held xy, z 0; R the root's turn), each root child's key moves by `-h`
(channels made where missing), and the start / end transforms move the same way. Every node
stays where the clip puts it (15 clips, at most 1.5e-5 m: `rerooting_keeps_him_where_his_clips_put_him`).
After it: `Idle`'s chest is at (0.16, 14.73), `IdleToWalk` ends at (2.19, 30.32) = `Walk`'s start =
the windups' start, and the windups end at `Idle`'s point.

How he moves now (tests, `core\tests\devastator.rs`):

- `TurnLeft`: his chest ends 3.8 m from its start (1.0, 3.6), yaw 90 degrees: on the spot.
- The pound and the vortex from `Idle` step his chest back about 13.6 m ((-1.84, -13.60)): the
  windups start in the walking frame, and the authored loop is Idle, IdleToWalk, Idle2Attack1,
  Attack1, Idle, which closes exactly; started from `Idle` (which the machine allows: `stateIdle`
  takes both messages [data]) he rears back. What the game shows there is not known (open below).
- `Attack1` lurches 1.59 m in one tick at tick 96, the first blow; nothing else steps over 1 m.

The old `giant::flatten_path` (it cut every root step over 4 m a tick) is gone: it popped his
visible body 35 m.

## Foot IK on his arms and legs (the port's own, 2026-10-22) [assumed]

The game puts none on him: his pack holds no `FootIK` object, one `SpineIK`, like the playable
robots' (`devastator_clips --ik`). The port's planting (`core\src\footik.rs`, `docs/foot-ik.md`)
now takes any number of limbs, and `giant::limbs` gives him four:

- legs `Thigh`, `Calf`, `Foot` and arms `Arm`, `Forearm`, `Hand` (each side). He stands on his
  fingertips (`F_*_Tip` lowest in `Walk` and `Idle`, z -0.5..0.3: `devastator_clips --limbs`);
- each sole is measured where it stands in `IdleStandSet/Idle` at tick 0 (`footik::Leg::planted`):
  the rigid mesh points of the hand or foot and every node under it within 1 m of their lowest,
  as a rectangle at that height (the forearm treads are not under the hand);
- the probe's reach is the game's 1.5 m times his height over 6 m: 15.2 m;
- it runs in Stand and Move as for everyone, and in his actions too (turns, pound, vortex).

On flat ground every limb is lifted the contact bias (0.03 m) and nothing else; a 2 m block under
his right hand lifts that hand 2.03 m, the other three stay, and no limb stretches
(`his_arms_and_legs_are_planted_on_the_ground`). It only adapts the clips to the ground under
them: a hand the clip raises stays raised, and the pound's fists still go into the floor (the
clip puts them at z -7). `--no-foot-ik` turns it off.

## The pound's shockwaves (`sim::shockwave_hits`)

`Attack1`'s Trigger events: ticks 96 (R), 136 (L), 180 (R), 222 (L), 300 (both). Each starts a
ring at its thumb's drawn place (else the thumb's place in the clip); a probe from 1 m above to
2 m below the thumb sets it on the floor (+0.15). Each wave hurts a thing once while any of its
rings lives (`docs/effects-shockwave-tube.md`), so a scout 40 m out took two hits of 25 in the
test. [stand-in] the candidates are the arena's targets, not every object with a collision
record; the probe is the floor under the thumb, not a swept sphere; every target takes the hit.
Drawn by `game\src\shockwaves.rs`: [assumed] the ring's fade as the material's object alpha.

His team is Decepticon: the arena's default scouts are Decepticons and refuse his damage, as the
game would. Use `--enemy aprotscoutmp:APrtScout` to have something to pound.

## Made a behemoth (2026-10-22, second pass; the user: "you have free will to customize him")

The user played him and saw the walk slide ("sliding rather than pushing off the ground"), asked
for his abilities ("his sucking ability"), and gave leave to customise him into "a proper playable
behemoth robot, while also making it believable". Everything here is port design, tagged
`[assumed port design]` in the code and listed in the unpublished notes.

**The walk by his clips' root** (`giant::Giant::roots`, `giant::Root`, `sim::giant_root_motion`).
He walked at the walk's even speed (`Clip::forward_speed`, 10.96 m/s) while his clips surge and
check with each step (0.12 to 0.27 m a tick on his chest), so his planted hands slipped up to
0.23 m a tick (`devastator_clips --reroot --slide Walk`). Now every base clip moves him by its own
root: the travel from the tick he was last moved at (`Root::travel`, in the frame at that tick, a
loop composed through its end), turned onto his heading, plus the root's turn. The stillest of
his four limbs, drawn as the game draws him, moves 0.018 m a step on average and at most 0.079
(`his_walk_pushes_off_his_planted_limbs`, outside the clips' cross-fades). The pace is the clips':
11.5 m/s over 8 s. His chest sways 5.3 m left to 2 m right of its line over each loop and the
camera follows it.

**The walk's turn**: `modeMove.turnSpeed` is 360 degrees a second [data], a 60 m body spinning
on a stick. He turns at his `TurnLeft`'s rate instead (90 degrees in 4 s: 22.5 a second,
`Giant::walk_turn`).

**Running down into his stand**: he has no clip from the walk to the stand, and `Idle`'s blend is
0.1 s [data]. Its blend is raised to `giant::IDLE_BLEND` (0.6 s) for him, and the clip he leaves
goes on moving him, less and less, over the new clip's blend (`giant::RootMotion::carry`), as the
two are drawn: letting go at 16 m/s he is at 13 m/s a tenth of a second on, about 3 at half a
second (`his_steps_thud_and_he_runs_down_into_his_stand`). In `Idle` his chest rocks over his
planted limbs at up to 4 m/s, and his position goes with it (the clip's own sway).

**Footfalls**: his walk's effects are the dust at each landing, on `Hand_L` (tick 42), `Foot_R`
(94), `Hand_R` (170), `Foot_L` (212) [data]. Each landing (`Root::steps`, any base clip's effects
on those four nodes) starts `SoftCameraShake` at his place (the robots' plain landing shake: full
to 47 m, gone at 90, so the scaled camera feels it at about half) and the pad's
`VEHICLE_HIT_GROUND` rumble. The boss's clips start none on the walk (the game never follows
him). The pound's own shakes (`WideMassiveCameraShake` on each blow [data]) were always there.

## The vortex (2026-10-22, port design on the mission's data)

In the boss fight the vortex is not his: it is a placed `VortexObj` Instance the mission script
triggers [data `work\devastator\mission_data_1.txt` 544905, `devastator_clips --vortex`]:

- nodes `Root`, `VacuumAttack_SFX` (scale 0.01), `Vortex_OfficialVersion` (renders
  `Devastator_Vortex.Vortex_OfficialVersion`: a tube 2.02 long along +y, pointed at y 0, up to
  0.67 across; material `devastator_vortex.vortex`, technique 559460883);
- its own 520-tick clip: the funnel grows from 1 to 50 times by tick 40, spins about its +y, and
  shrinks back from tick 460;
- a `KillBox`: damage 50, damageType 0x802 (by player, explosion), knock-back 15 at 35 degrees;
- a `SoundPlayer` (event 0x6a93b16b) a `ForwardTrigger` starts 5 s after the trigger.

The port puts it on him (`giant::read_vortex`, `giant::vortex`, `sim::vortex_pull`,
`game\src\vortex.rs`): it starts at `AttackSpecial`'s first effect on `Head` (tick 86, the dust
drawn into his head [data]), its point on his head and its axis LEVEL, along where the head looks
on the ground (the head's +y with its tilt taken out: the line the clip's dust comes in along from
100 m out [data: the effect's source disc at (20, 100, 0) in the head's frame], and level as the
mission places the `VortexObj`, identity rotation at z 0 [data]; the head's own +y dips about 25
degrees in the crouch, which put the first funnel into the floor behind him), so at 50 times it
reaches about as far; it plays its own clip from there. Whatever of the arena's targets is inside
its hull (the model's radius by tenths of its length: `Vortex::radius`, which skips the first
band, the point alone, so the hull does not start as a needle of radius 0 that stalled a scout 5 m
short) or within its radius plus a tenth of the funnel's length of the point (a held ball,
[assumed port design]) is dragged toward the point, 30 m/s near it and 8 m/s at the mouth, times
the funnel's scale over its full, with a swirl of 0.4 of that round the axis, never faster than
reaches the point in the step, and held there; the host moves the target by it (`State::pulls`,
`game\src\targets.rs`). It is hit with the KillBox's damage, flags and knock-back at most every
0.5 s. In the test the scout 40 m in front of him (the special's windup carries him about 30 m
back first) was dragged for 262 steps, held at his head, and hit 17 times from 2.7 s to 10.9 s,
about every 0.51 s (`his_vortex_drags_in_and_hits_what_is_in_front_of_him`). The funnel is drawn with its nodes at
rest (their binds are identity but for the render node's 4 mm), so what is seen is what drags and
hits. Its sound starts 5 s into the funnel's own clip [assumed: that the trigger starts the clip
and the `ForwardTrigger` together], at the funnel's point, and stops with it.

## His body (2026-10-22, third pass; asked for by the user: "so walls stop him and enemy shots hit him")

His attributes' avatar is height 1, radius 1 [data], a scripted boss's placeholder under a model
about 50 m tall in his stand: walls stopped only the point under his chest, and others aim at his
height times his `aimingHeightPercentage` of 3500, 35 m up [data], over a 1 m post, so no round
ever hit him (the scout's `target` was None every time). The port gives him a body from his
model (`giant::body`, set in `giant::fit`) [assumed port design]:

- radius 25.6: his four limbs' mean distance from his root across the ground in `Idle` tick 0
  (feet 18 and 26, hands 26 and 33);
- height 60.6: `model_height`, his bind pose's top (the same number the camera is scaled by);
- stride 9.3: the cylinder starts at his ankles' height (`Foot_L` / `Foot_R` in `Idle`), so what
  is lower passes between his legs (the arena's push takes a lifted pencil, `sim::step_move`'s
  push; `separate_targets` skips scouts shorter than that, so they walk under him) and he stays on
  the ground: the limb IK plants his hands and feet on it;
- the aim point stays the game's 35 m (his `aimingHeightPercentage` scaled to the new height).

Tests: `walls_stop_his_body_and_low_ones_pass_under_his_stride` (a 40 m wall at y 80 stops his
root at 54.35, a body's radius short; a 7.3 m one he walks over at z 0) and
`an_autobot_scouts_rounds_hit_his_body` (38 rounds in 12 s land on his cylinder's side about
23 m up). The vortex still drags, holds and hits (the scout is held at the cylinder's edge,
26 m out, within the held ball). Captures: `body_wall.png` (the arena's 22 m block at y 205
stops him at 179.35, his hands against it), `body_shots.png` (scouts in range; impacts too small
to see at that distance).

Not pursued: on a level the level's physics moves him (`World::step_avatar`) with the full
cylinder and no stride, so low clutter stops him (Cairo boss stage, LTrain rooftop). The robots are
tested on the arena, not on levels (user, 2026-10-22), so this stays as it is.

## The giant's yard (2026-10-22, on the user's ask: "add things to the test map to test him")

`giants_yard()` in `game\src\world.rs`, left of the start at x -560..-275, authored for his body
[stand-in: no game dimensions]. `--start -420,-120,0` stands him at its stride lane, facing it.
- **Stride lane** (x -460..-380): walls across it 2, 5 and 8 m high at y -50, 0 and 50, which he
  steps over with his feet never leaving the ground, and one 12 m high at y 100, which stops him
  at y 74.35 (100 - his radius).
- **Gates** at y 205..235 between 40 m towers: 40 m wide at x -480 stops him at y 188.99 (his
  cylinder meets the towers' corners, 20 m either side: 205 - sqrt(25.6^2 - 20^2)); 70 m wide at
  x -340 lets him through.
- **Clutter field** (y -250..-170): four rows of cars 1.5 m high and lamp posts 7 m high; he walks
  through them.
- **Hill** (x -560..-500, y -160..-40): 12 m up and down at 1 in 5; his feet top out at 11.98.
Test: `world::giants_yard_tests` (game crate) walks him through each on the whole arena and
asserts those numbers. Captures: `yard_lane.png` (`--start -420,-40,0 --stick 1 --after 7`, past
the low walls), `yard_gate.png` (`--start -480,150,0`, stopped at the narrow gate),
`yard_cars.png` (`--start -500,-275,0`, among the cars).

## Captures (2026-10-22, `work\shots\devastator\`)

Taken with the game exe, `--character devastator --shot <png>` and the flags below. A capture
prints `capture shockwave <name>: <n> rings (centre) r <radius> a <alpha>` for each of his waves.

- `idle.png` (`--after 1.5`): he stands with his back to the camera, 165.6 m behind and 36 m up
  (the scaled camera), about a quarter of the picture high. His crane arm's cables shimmer at
  this distance (alpha-cut cable meshes).
- `walk.png` (`--stick 1 --after 5`): `WalkSet/Walk`, 11 m/s, 50.8 m on, walking away from the
  camera with foot dust.
- `turn.png` (`--stick-x -1 --stick-from 0.5 --stick-until 0.8 --after 3`): `stateTurnLeft`,
  `TurnLeft` at tick 150, yaw 57.5 degrees, already at (13.8, -6.9): the sideways slide is real
  in the port, its rightness still open (above).
- `pound.png` (`--melee-at 1 --after 4 --enemy aprotscoutmp:APrtScout --scouts`): `Attack1` at
  tick 124; one ring of the right thumb, centre (32.7, 40.0, 4.2), radius 38.3. That first ring
  starts 4 m up: the thumb is above the probe's 2 m reach at tick 96, so it stays at the
  thumb's height (z varied 2.9 to 4.2 between runs with the frame the trigger fell on).
- `pound_late.png` (the same at `--after 7.5`, tick 333): three rings (one at radius 142.7 fading,
  two at 45.3 on the floor); the scouts at 175, 200, 175 of 225, so the waves hurt in the game.
- The rings are drawn but faint: additive smoke (`fx_smokecloud07`, opacity 0.5, tiled 6 times
  round) over the light beige floor. A frame diff against `idle.png` shows the bands clearly; the
  picture alone barely does. Not known whether the game's are as faint.
- `special.png` (`--special-at 1 --after 5`): `AttackSpecial` at tick 184, crouched, the sand
  plume rising over him; he stays put.

After the re-root and the limb IK (same day, the game exe rebuilt):

- `turn_reroot.png` (the turn flags above, `--after 3`): `TurnLeft` tick 150, yaw 57.5 degrees,
  his chest at (3.95, -8.12): the body swings out about 9 m mid-turn as the clip pivots him, and
  the camera stays on him (he is centred in the frame).
- `turn_end.png` (`--after 5.5`): back in `Idle`, yaw 90 degrees, chest at (0.99, 3.68), 3.8 m from
  where he started; centred.
- `pound_reroot.png` (`--after 4`, the pound flags above): `Attack1` tick 124, chest at (3.66,
  -14.08) after the windup's rear-back; one right-thumb ring at (30.9, 7.2, 4.8) r 38.2, the dust at
  his right fist in the picture.
- `pound_reroot_late.png` (`--after 7.5`): tick 334, rings at (26.6, 7.0, 0.2) and (-26.4, -10.9,
  0.2) on the floor and one fading at r 142.9; dust at both fists; scouts at 175, 175, 200 of 225.
- The limb IK is not visible on the arena's flat floor (0.03 m); it is checked by the test only.

The vortex, after the level axis (same day; `--special-at 1 --enemy aprotscoutmp:APrtScout
--scouts --enemy-at 0,40 --look-x 1 --look-from 1`):

- `vortex_side.png` (`--look-until 2 --after 4`): `AttackSpecial` tick 124, his root at (-0.95,
  -29.6) after the windup; the camera ended in front of him, so the funnel is seen end on: a
  swirling grey-brown cone over his head with green specks, the scouts below it. The capture line
  `capture vortex: tip (1.8, -6.0, 13.2) axis (0.26, 0.96, 0.00) scale 49.8 tick 38` and one scout
  pulled; scouts at 175, 225, 225.
- `vortex_held.png` (`--look-until 1.5 --after 7`): seen from his left side, tick 304; the funnel
  is a faint green translucent cone widening away from his head to the left of the picture, about
  100 m long; one scout down, the others at 125 and 75; `axis (0.02, 1.00, 0.00) scale 50.0 tick
  218`, one scout pulled (up and in, `(5.9, -17.7, 12.6)` m/s).
- The funnel's own sound starts at its clip's 5 s; both captures end before that, and no capture
  has sound: not heard.

## Made a player character (2026-10-22, fourth pass; the user: "you do not have to make it 1:1 ... improving the player belief that they are controlling a behemoth")

Everything here is [assumed port design] on the game's data unless tagged otherwise: the game
never lets anyone play him.

- **Shadow and light** (`game\src\giant.rs` `shadows`, `fill`): the stage's 60 m shadow range is
  for a 6 m robot seen from 5 m; while he is played the sun's two cascades reach 900 m (the first
  to 320 m) on a 4096 map. His own Surface materials get an ambient lift of (0.2, 0.19, 0.17), as
  a film lights a giant from the camera's side; seen from behind under one sun he was a black
  mass. His shaders are his own (Surface) and were checked; nothing else changed.
- **Cameras** (`giant::scale_cameras`): the follow camera is the robot camera scaled to 0.62 of his
  height over 6 m (about 94 to 107 m back), held 5 degrees below his look point at 68% of his
  height, 18% of his height over his right shoulder, turning at 0.6 of the rate, 5 degrees wider.
  The aim camera (L2) is the weapon camera at 0.38 of that scale (about 45 m back), looking at 95%
  of his height, 36% of it to his right: a gunner's view past his shoulder.
- **Guns** (`giant::read_pods`, `Arsenal::load_giant`): his pods are read from the boss mission
  [data]: four gatling pods (`Devastator_GunPod01..04`, four barrels each, on `AttachPt_GunPod*`),
  three missile pods (`Devastator_MissilePodProxied01..03`) and the two bug-bomb launchers (root
  fields `BugBomb_Launcher_Front` / `_Back`, on `AttachPt_Vortex` / `AttachPt_BugBombLauncher`),
  each with its weapon, attribute, barrels and effects [data]. They ride the car weapons' path:
  R2 (or the mouse) fires the selected kind, R1 cycles gatling, missiles, bug bombs; the rounds
  go from each barrel in turn, across all the pods of the kind, to the aim camera's point. The
  gatling's heat per round is a quarter of the data's (`GIANT_HEAT`; at the data's it locked after
  20 rounds for 4 s); a launcher fires a volley of one round per pod. His fire sound is the
  pods' `fireSoundRemote` (`WPN_DEVASTATOR_GUN_FIRE`, `_MISSLE_FIRE`) [data], since the pods have no
  own one. The gatling's round (`laserrifle`) has no emitter [data], so the port draws a tracer:
  a 14 m glowing slug at the round's 300 m/s (`giant::tracers`). The pods are drawn on their bones
  at scale 1 [assumed: the mission's instances are 0.5 before their positioner].
- **Hits, hit reacts and death** (`giant::fit`, `giant::Stagger`, `sim::giant_react`): the boss is
  `invincible` with 100000 health [data]; played, he takes damage over `GIANT_HEALTH` 6400 (twenty
  times Bumblebee's 320), is `immovable`, and mends after 4 s unhurt (`GIANT_REGEN_DELAY`; his data's
  0 restarted it, and its UI sound, on every round). The shared hit reacts and stun never start (they
  play `HitReactionSet`, which he lacks). Damage of the last 1.5 s adds up; past 2.5% of his health
  he staggers (at most once in 2.5 s): standing or walking, he plays one of his own four hit reacts
  [data `PlayAnimSet`] by where the push comes from (ahead `HitReactMissilePod`, which reels him
  17.6 m back; left `HitReactArmLt`; right `HitReactArmRt`; behind `HitReactMissilePodLt`) in
  `stateTurnLeft`, his plain one-clip state, which hands him to `stateMove` when it ends (4.6 s);
  busy pounding, in his vortex or turning, his model sways about his feet on a damped spring
  (2.6 degrees, 2.4 rad/s). A stagger adds his grunt (`DEVA_REAC_EXERT`) and `MedHardCameraShake`;
  the clips cue the heave and steps themselves [data]. At no health he falls in `DeathSet/Death`
  [data], which cues `ANIM_DEVA_FALL` at 274 [data]; the port adds `MassiveCameraShake` at ticks 178
  and 296 and `BuildingCollapseCameraShake` at 300, where the clip's dust bursts.
  (The 2026-10-22 note that he had no hit reacts was wrong: the listing it came from was cut off.)
- **Sound**: his clips' cues, the stagger's and the vortex's all play from the boss bank. The
  vortex's four sounds were heard in a recording for the first time (`work\devastator\snd3.wav`):
  `ANIM_DEVA_VORTEX_START`, `_VORTEX`, the funnel's `WPN_DEVASTATOR_VORTEX_FIRE` (0x6a93b16b, 6.4 s
  after the start) and `_VORTEX_END`. The pound's stacked `ANIM_DEVA_POUND`s (five at once, 3 s each)
  peaked at 1.8 times full scale and were hard-clipped; the mixer now ends in a limiter
  (`Mixer::limit`: ceiling 0.95, instant attack, 0.25 s release) instead of a clamp, for everyone,
  acting only where the clamp would have clipped. Recorded after it (`snd2.wav`): no overs, the pound
  at about -9 dB. The listener stays at the camera (about 105 m off): the loud category carries.
- **Test aid**: `--hurt-at <s>:<damage>[,...]` (`giant::hurt_aid`) lands an enemy hit from ahead on
  any character at those times. Captures: `stagger.png` (`--hurt-at 3:300 --after 4.5`,
  `HitReactMissilePod` tick 90, health 6100) and `fall.png` (`--hurt-at 3:10000 --after 9`, `Death`
  tick 360 in its dust). `gun_aim.png` and `gun_missiles.png` show the aim camera, the tracers and
  a missile's trail.

## The bug bombs erupt (2026-10-22, fifth pass)

The user: the bomb volley should be a volcano, fired up unaimed and falling at random all round
him. [assumed port design, none of it the game's: the boss's bombs are `MissileLauncher` rounds
sent at a target] `tf2_core::giant::Lob`: each pull of the bug bombs throws `ERUPTION_BOMBS` (12)
bombs from the launchers' barrels, each on a ballistic arc (pull 40 m/s², heavier than the world's
so the rocks drop fast) climbing 40 to 95 m over its launcher (drawn per bomb), steeper than 45
degrees, onto a random spot of the ring 38 to 120 m from his root (even over the ring's area). The
bomb is the launcher's own missile (model, trail, flight sound, blast and its damage) flown along
the arc (`Arsenal::lobs`, `volcano`); it goes off on the first thing it touches or 3 s past its
landing time. Flights run 3.5 to 4.9 s, so a pull rains for about 1.5 s. Test
`bug_bombs_erupt_and_rain_round_him` (200 arcs: all land on the ring, all round him, and end on
their spot). Captures `volcano_rise.png` (`--switch-at 1,1.4 --fire-from 2 --fire-until 2.3 --after
4`: 48 bombs up, four pulls) and `volcano_land.png` (`--after 6.8`: blasts all round him).
`--switch-at` now takes several times.

One vent (2026-10-23, the user saw three spawn points): every bomb leaves from the middle of the
launcher on `AttachPt_BugBombLauncher` (on his back; `ERUPTION_VENT`), the one on `AttachPt_Vortex`
at his mouth stays still, and each bomb draws ONE trail at its centre. The game's bug-bomb object
is a cluster of three, each with its own trail emitter [data: 3 emitters in its effects row], which
drew three streaks a bomb. [assumed port design] Capture `volcano_vent.png` (`--after 2.35`): one fan
of single trails from above his back.

The vent moved the same day (the user: "way too high", and it did not turn with him). Measured in
his frame (`giant: bug-bomb mount` in the log): the bone `AttachPt_BugBombLauncher` is on his centre
line, 4.6 m ahead and 49.7 m up; its launcher's barrels ring it 14 m higher (59.6 to 68.8 m up), so
their middle sat over his head ON the axis he turns about. The vent is now the bone itself,
`ERUPTION_VENT_BACK` (10 m) behind it along his facing: on his back behind the missile pods, and
swinging round as he turns. [assumed port design] Captures `vent_a.png` (`--after 2.15`, facing ahead)
and `vent_b.png` (the same after `--stick-x 1 --stick-from 0.2 --stick-until 1.8`, turned).

Then the launch angle (the user: still "90 degrees in the sky", chose "the launcher's angle, all
round", height kept at 40-95 m). [assumed port design] A bomb now leaves along the back launcher's
axis as posed (from `AttachPt_BugBombLauncher` to the middle of its barrels, `Arsenal::vent_axis`,
set each frame by the player; never under z 0.25), then curves over and down to its spot of the
ring: `Lob` is a cubic curve (vent, a point along the axis, a point over the spot, the spot) whose
control height is solved so the peak is the drawn climb, flown evenly in the time a thrown rock of
that climb takes. Measured (`giant: bug-bomb launcher points`): standing, the launcher points 0.27
back and 0.96 up (about 16 degrees off upright: still nearly straight up, that is its angle in the
model); in his turn clip 0.37 right, 0.58 back, 0.73 up. Captures `vent_a.png` / `vent_b.png`
re-taken (`--after 2.4`): the turned plume leans with his body.

Then the spread (the user: they did not like it "coming from one focused point"). [assumed port
design] `giant::vent_spots`: a pull's 12 bombs leave from 12 spots of a disc 14 m in radius
(`ERUPTION_VENT_SPREAD`) round the vent, lying across his back (squashed to 0.6 front to back),
sunflower-spaced, dipping 0.15 m per metre out from the middle, turned with his facing; each bomb
takes the next spot. Test `bombs_leave_from_all_over_his_back`; capture `vent_a.png` (`--after 2.3`):
twelve separate trails rising across his back's width.

## Only the miniguns on the shot's side fire (2026-10-23, sixth pass)

The user: facing forward, the gatlings on his back legs fired too, and their trails crossed his
body and other guns; only the pods on the side he shoots to should fire. [assumed port design]
`tf2_core::giant::pods_facing`: each gatling pod's side is its centre's way out from the middle of
all four, across the ground; it fires when that way and the shot's are on the same half (cosine 0
or more, `POD_SIDE`), and the best one fires if none is. A shot straight up or down lets all fire.
Measured from his middle (`giant: gatling pods` prints it when the set changes): the shoulder pods
sit about 33 m out and 11-20 m ahead, the leg pods about 8 m out and 16 m behind. So ahead fires
the shoulders, left the left shoulder and left leg, behind the two legs. Only the gatling is
gated; the missiles and the bombs fire from all their pods. Test `only_the_pods_on_the_shot_side_fire`
(the measured layout, eight directions); captures `pods_ahead.png`, `pods_left.png`,
`pods_behind.png` (`--aim-from 1.5 --fire-from 2.2 --look-x <-1|1> --look-from 0.5 --look-until
<1.2|2> --after 3.2`).

## Balanced as a player (2026-10-23)

The user: he was never meant to be played, he is weak and his weapons are not good. [assumed port
design throughout] `core\src\giant.rs` holds the table; the unpublished notes has the before and after.
- Measured first (`--scouts --combat-report --after 30`): three scouts land every 6.8 round on him
  (a 60 m target), 64 a second: 1800 of his 6400 in 28 s.
- Now: 12000 health and armour x0.4 on every hit (`GIANT_HEALTH`, `GIANT_ARMOUR`, applied at the top
  of `sim::receive_damage`): 780 in 30 s from the same squad. Scouts have 225, Bumblebee 2400,
  Optimus 3000, Megatron 3600 [data `characterAttributes*` baseHealth].
- Pound rings 150 a hit (data 25), vortex 120 a touch (data 50).
- Pods (`POD_BALANCE`, in `Arsenal::load_giant`, printed as `giant: pod kind N balanced`): gatling 9 a
  round every 0.05 s; missile volley (3) every 2 s, 60 direct and a 15 m blast of 140; eruption (12
  bombs) every 1.2 s, 40 direct and a 12 m blast of 100. Blasts' least damage 35% of the full; his
  own blasts do not hurt him.
- Seen in play: bomb blasts hit scouts for 100 (76-83 at the edge). Not seen: the gatling or the
  missiles on a scout. Open: from his 50 m eye the aim camera looks over scouts' heads; a scripted
  capture with `--look-y` swung it above him. The eruption's ring is wide, so it rarely catches a
  squad: tune the ring or the blast by play.

## Audit (2026-10-23)

The user asked for everything missing (audio, effects...). Method: his 14 clips' events
(`devastator_clips` example, `work\devastator\clips_audit.txt`), the boss mission's event file's
names, and one sound-traced run per move (`TF2_SOUND_TRACE=1`, `work\shots\devastator\aud_*.log`:
pound, special, gatling, missiles, bombs, walk, turn, hurt and death, overheat).

Heard: every `ANIM_DEVA_*` the clips cue (POUND, POUND_MTL, HYD_LIFT_A/B, STEP, STEP_BACK,
STEP_LAND, LEAN, FALL, VORTEX_START / VORTEX / VORTEX_END), `DEVA_REAC_EXERT`, `DEV_ROAR`, and
`WPN_DEVASTATOR_GUN_FIRE` / `_MISSLE_FIRE` / `_VORTEX_FIRE`; the bullets' impacts; the bombs' flight
loop and blast. Seen earlier: footfall dust, pound rings and dust, vortex funnel and dust, death dust,
muzzle flashes, trails, blasts. The two unresolved cues at start are not his (`ANIM_BUMB_TRANS_ROB`,
`UI_MP_ALLSPARK_ENERGY_LP`, from the shared tables).

Missing or weak:
1. The gatling overheats (and locks 2 s) silently: the pods' weapon data has no overheat or cool
   sound and no overheat smoke. The boss bank has `WPN_HMG_OVERHEAT` (start, mid).
2. Small blast sounds for a giant: the missiles end in `WPN_MIS_SM_EXPLO`, the bombs in
   `EXP_OPTIMUS_MISSLE` [data]; the bank has `EXP_LRG`, `EXP_HUGE`, `EXP_FIREBALL`.
3. Never cued: `ANIM_DEVA_ROAR_02` (four roar samples) and the cinematics' sound beds
   `Devastator_Intro_Cinematic_fx` / `Devastator_Death_Cinematic_fx` (cutscene sound, in his bank).
4. The eruption has one fire sound a pull and no launch flash, smoke or shake.
5. Missiles aimed from his 50 m eye fly level and mostly never land in the arena (no blast heard
   in the 7 s run); the aim camera cannot look down at scouts near him (both: aiming, not audio).
6. No damage state on him: no smoke, sparks or fire as his health falls (the boss's pods and
   `Devastator_CranialCircuits` are destructible script objects, not read).
7. Known before: the muzzle light (all robots); the HUD names a stagger `stateTurnLeft`; the vortex
   does not lift loose things.

Correction found while fixing: `ANIM_DEVA_ROAR_02` is not an event but the sound definition
(`/ANIM_DEVA_ROAR_02`, samples `anim_deva_roar_01..04`) of the `DEV_ROAR` event, which his stagger
already played. `Devastator_Death_Cinematic_fx` IS an event (category VO/CHATTER, one 16.3 s stereo
sample in `mission0.fsb`).

## Audit fixed (2026-10-23) [assumed port design]

The user: "fix all of the missing stuff". From the boss mission's own data (`giant::Dressing`,
`giant::SOUNDS`, `game\src\giant.rs` `dress`):
- Overheat: the gatling pods' weapon gets `WPN_HMG_OVERHEAT` (`OVERHEAT_SOUND`, through the
  ordinary `KeptSounds::overheat`), and every gatling pod smokes (`smk_deva_gunpod`, the pods' own
  `DMGSMOKE` TriggerableParticle on node 1) while the lock lasts. Heard and seen (`fx_heat.log`,
  `fx_heat2.png`).
- Blasts: the missile rows end in `EXP_HUGE`, the bombs' in `EXP_LRG` (`read_pods`). Heard: 3 missile
  blasts in a 7 s run (none before), 12 bomb blasts for one eruption.
- Eruption: each firing bursts at the middle of its launch points (`r_devastator_damaged`, the pods'
  broken-object burst: flash, fireball, ring, sparks), pours `smk_deva_gunpod` there for 1.5 s, plays
  `EXP_FIREBALL` and starts `MediumCameraShake` (`Arsenal::eruptions` / `eruption_at`). Seen
  (`fx_bomb.png`).
- Damage: his pods break in three stages, under 75, 50 and 25 % of his health
  (`giant::damage_stage`, left only 5 % above a stage since he mends): a third of the pods each time
  (`pod_breaks_at`: in turns) burst, spark (`dest_sparkshower`, the pods' `DMGFX` on node 10), play
  `EXP_LRG`, and smoke from then on; each stage roars (`DEV_ROAR`) and shakes (`MedHardCameraShake`).
  Broken pods spark again every 1.5 to 4.5 s (over the stage) with `DEST_PHONELINE_SPARK`. Under the
  last stage his back (`AttachPt_BugBombLauncher`) burns: `smk_plumesmokelarge` (his animation set's
  60 s plume), `FIRE_LG` as it catches. Seen (`fx_hurt.png`, `fx_burn.png`): the smoke reads; the
  plume's flames do not show from behind him.
- Death: `DEV_ROAR` and `Devastator_Death_Cinematic_fx` (16.3 s, over his 12 s fall).
- Missiles: launched toward the aim point, and when his look meets nothing within 350 m
  (`giant::MISSILE_REACH`) they come down on the ground under the look's line there
  (`giant::missile_point`).
- Muzzle light: `point_light::load_flash_lights` reads his pods' (`muzzleflash_orange_big`, only the
  gatling pods have one [data]; the launchers flash with it), range x3.
- HUD: a stagger reads "staggered (<its hit react>)".
- Not done: the vortex lifting loose things (the arena has no loose objects); the aim camera looking
  further down was not changed (the missiles' landing covers what it was needed for).

Second pass, after the user played it (2026-10-23):
- Smoke and sparks floating off his body at 50 % and 25 %: the bug-bomb launchers (4 nodes) have no
  smoke or spark node, and their nodes 1 and 3 are launch dummies 20 to 40 m out in front of him
  (pod 7's sparks printed at 41 m ahead, 11 m up). Their effects now sit on the launcher's root
  (`giant::pod_effect_node`); seen right from the side (`fx_place.png`).
- One roar per damaging hit, however many stages it crosses (was one per stage).
- More roar [assumed port design]: at the windup of the pound and the vortex, and at the start of a
  burst of fire (after 1.5 s quiet, 60 % of the time), never within 5 s of the last.
- Ambience [assumed port design; the bank has no idle loop of his]: `VEH_IRON_TRUCK_RPM` kept on
  him, rpm 700 idling to 2400 with walk and attacks, load with effort; `ANIM_SHARED_WIND_LP` at his
  head, speed 14; while he stands, `ANIM_DEVA_HYD_LIFT_A/B` or `DEST_AIR_HISS` every 3 to 8 s. The
  loops stop as he dies. Measured: his quiet standing sits at about -25 dB (it was silence).
- The death cinematic's sound bed has crowd screams in it (the user heard women screaming): it is the
  cutscene's own mix [data]; kept, the user does not mind.

## Polish (2026-10-23; the user: "very close to shipping ... polish devastator up")

- His shakes never reached his camera (found here): a placed shake is whole within its
  `shakeFullDist` and gone at `shakeMaxDist` [data: `SoftCameraShake` 47 / 90 m], and his follow
  camera stands 94 to 107 m from him, so every footfall shake since 2026-10-22 did nothing.
  `scale_cameras` now scales every placed shake's distances by his camera's scale (x6.3: the
  footfall's whole to 294 m, gone at 564 m). Tested (`his_cameras_frame_a_giant`).
- His blasts shake the camera where they land [assumed port design; blasts start no shake in the
  port for anyone, `cameraShakeOnExplode` is read and unused]: a bomb's `SoftCameraShake`, a missile's
  `MediumCameraShake`, placed (`Arsenal::giant_blasts`, `giant::dress`).
- His reticle is the weapon mode's, not the car's two bases [assumed port design]: the pod kind in
  use comes up as its icon for 1.5 s on a switch (gatling the rapid icon, missiles the missile's, the
  bug bombs the grenade's base and icon, as they are thrown), and it shows the missiles' lock (bracket
  and the four lines), the target under the reticle and the hit mark. Seen: the bomb icon
  (`ret_bomb.png`). NOT seen: a lock (scripted aiming swings the aim straight down in 0.2 s, so no
  capture framed a scout); his missiles lock at once on what is in the box (`lockOnTime` 0 [data]).
  The bug bombs lock on nothing.
- A switch of pod kind whirs (`ANIM_DECPTICON_SRVO_FDG`, the bank's) [assumed port design]. Heard.
- Terminal noise: the gatling's pod selection and the pod breaks print only under `TF_DBG`.
- Seen, not changed: in his aim camera the camera stick turns the aim very fast (half stick for
  0.2 s took it from level to looking straight down on him); not checked with a mouse.

## Performance (measured 2026-10-23, `--fps --no-vsync`, 1280x720, the arena with the Autobot squad)

Mean frame time over 2 s windows (`work\perf\*.log`): Bumblebee walking and firing 3.2-3.6 ms;
Devastator idle 3.8, walking 3.4-3.8, gatling 3.7-4.6 (steady over 40 s and 320 rounds: nothing
builds up), bug bombs 4.4-5.3, missiles 4.5-5.6, vortex 3.9-5.4, pound 3.9-5.9. With vsync on he
holds the 240 Hz cap. One early vsync run sat at 60 fps from its 8th second; it did not come back in
a rerun (cause not found; the user's window had just been closed).
- Hitches: the slowest frame of a window reaches 20-72 ms now and then, Bumblebee's too (65 ms), so
  it is shared, not his. Chased the same day (`--fps` now prints each frame over 15 ms with the time
  the frame's First..Last took):
  - the "flat 60 fps" was Bevy's default for an unfocused window (60 updates a second); `--fps` now
    keeps full speed unfocused (`WinitSettings::continuous`). Not a problem in play.
  - the spikes happen with Bumblebee standing idle, no enemies, no sound, at a different time every
    run (3-18 s in, a cluster of 20-90 ms frames, then quiet). Not sound (they stay with `--no-sound`).
  - a trace (`bevy/trace_chrome`, one run) shows no slow system of ours: in a spike every schedule is
    slower together, and Bevy's render-world bind-group work (`prepare_preprocess_bind_groups` 12-32
    ms, `prepare_mesh_view_bind_groups` 25 ms) holds the shared worker threads the game's PostUpdate
    waits on. No pipeline compile shows in the trace.
  - fixed on the way: `ssd::run` re-uploaded its sphere buffer every frame (a new GPU buffer, its
    materials rebuilt); now only on a change. The spikes stayed.
  - a CPU sample during a run found the machine lightly loaded; the spikes fell near bursts of
    other programs' activity. Rerun with them closed: the spikes stayed (worst 48-53
    ms), at random times, with vsync on too (1-3 bursts of 30-55 ms in 35 s).
  - also ruled out: the textures (`--original-textures`: the same), page faults (the process takes a
    burst of ~120k soft faults about every 13 s, but spread over frames, and the slow frames have 0;
    `--fps` logs the faults), the Vulkan driver path (`WGPU_BACKEND=dx12`: the same).
  - SETTLED: a bare Bevy scene with none of the game's code (`game\examples\bare_hitch.rs`: a cube, a
    light, a camera) has the same spikes (52 and 55 ms bursts in two 35 s runs, means 2.2 ms). They
    are the machine's or Bevy 0.19's, not the game's. Many come about 1.02 s apart, which points at
    something polling once a second outside the game (not found). Not ours to fix in the game.
- Memory: his 39 textures at the 4x upscale take 1671 MiB with mips (Bumblebee's 18: 428 MiB);
  `--texture-scale 2` quarters it.
- His limb IK now probes about 40 rays a frame (4 limbs x 5 points, doubled by the look-ahead); no
  cost seen (`--no-foot-ik` changed nothing measurable).

## Heavy limb planting (2026-10-23; the user: "feels weightless when he's pushing his feet up over a small obstacle")

The game's filter (`footik`, half the gap every 0.032 s) lifted a limb onto a 2 m block in about
0.1 s whatever his size, and his body only ever lowered. For him (`footik::Weight`, `giant::WEIGHT`,
[assumed port design]; other robots keep the game's filter):
- each limb's lift is a critically damped spring, rate 9 rad/s up and 16 down: 0.48 m of a 2 m block
  in 0.1 s, on it by about half a second;
- a moving limb also probes where it will be 0.35 s on (at most 6 m), so a swinging hand or foot
  starts up before it reaches an edge (2.4 m short of it at 8 m/s) instead of sinking into it first;
- his body (`Main`) rises 0.35 of the highest limb's lift (0.7 m for a 2 m block) on an under-damped
  spring (5 rad/s, damping 0.55), and a limb ending a rise pushes it down by 0.35 of that limb's
  fastest speed: the weight settling onto it (top 0.75, rest 0.71 m in the test).
Tested: `his_limbs_climb_onto_a_block_with_weight`; the old planting test still passes. Seen: a walk
on the arena (`ik_walk.png`) unchanged on flat ground. Not seen: a step onto a block in a picture.

## Top-attack missiles (2026-10-23; the user: the missiles clip through him, "something similar to a javelin")

- His missiles fly a top attack (`giant::TopAttack`) [assumed port design]: each leaves its pod near
  straight up (leaning 0.3 out from his body, 0.2 toward the point), climbs 45-70 m (drawn) plus 0.15 of
  the range over the higher of its pod and its point, turns over and dives on it. The path is a cubic
  curve like the bombs' `Lob`, flown at a mean 105 m/s (1.6 s at least), eased (`u^1.6`): slow out of
  the pod, fastest in the dive. A locked missile's end follows its target's middle every frame, so it
  lands on a walking scout; past the end it flies on straight for 2 s. The game's own steering
  (`Missile::step`) is not used for his.
- Tested (`his_missiles_climb_then_dive_on_their_point`): leaves at z > 0.9 leaning out, climbs at
  least 45 m, ends on the point (also a target walking 8 m/s), comes in at 79-89 degrees, no step over
  12 m. Seen: `mis_up.png` (three trails straight up out of the pods), `mis_dive.png` (a volley coming
  down near vertically far ahead while the next climbs). Not seen: one landing on a scout.
- Open: the numbers are picked, not played; a pod under an overhang of his body would still climb into
  it (none seen).

## Not done

- The pound and vortex from standing: his chest steps back about 13.6 m through the pound's
  windup and his root about 30 m through the special's (the re-root above). Kept on purpose
  (2026-10-22): cancelling the travel would slide his planted feet as far instead, and the step
  back reads as a wind-up. Settles, if wanted: the boss fight in the game.
- The vortex's hull is the model's profile, not the KillBox's own 14-point hull (not read);
  the targets are the arena's, not every object; what it drags is moved by the host as a velocity
  and a held fall, not through the target's physics; the funnel does not lift loose things.
- The bug bombs' eruption numbers (12 a pull, ring 38 to 120 m, climb 40 to 95 m, pull 40 m/s²) are
  picked, not played against: tune by feel. No shake on the eruption or the landings beyond the
  blasts' own rumble.
- Not captured: the missile volley; whether the pods sit right on his
  body at scale 1 (they are spawned; not looked at closely); the stagger's sway when it cannot play
  its clip; the shakes and the stagger's sounds in a picture (the sound recording has them).
- The stagger sounds an offset in the recording (mean 0.12 over 0.1 s as `ANIM_DEVA_LEAN` and
  `DEV_ROAR` start): likely the samples' own low rumble, not checked.
- The HUD names a stagger `stateTurnLeft` (its host state).
