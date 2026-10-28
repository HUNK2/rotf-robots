# Bumblebee (Bevy)

Bumblebee from Transformers: Revenge of the Fallen (PC), rebuilt in Bevy and Rust from the
game's own data, code and recordings of it. The aim is his whole set of mechanics, as close
to the original as they can be made.

It reads the game's packs from your install when it starts and ships none of them. Nothing is
written to the game folder, and the game's executable is never run.

The core (`tf2-core`, in `core/`) and the research notes (`docs/`) are in this repository too.

## Running it

```
cargo run -p bumblebee
```

The install is found through its installer's registry entry. Elsewhere, a dialog asks for the
folder on the first run, or run once with `--game-dir <folder>` (the folder that holds
`bnxglobal.str`): either is saved and used from then on. `--pick-game-dir` opens the dialog
again, to change the saved folder.
`TF2_GAME_DIR` overrides both (`tf2_core::install`). The first build compiles Bevy and takes a long time; later ones are quick.

Bumblebee's robot, Camaro and gun textures now use the user's selected faithful **4× upscale**
by default: the main robot colour/normal maps are 4096×4096, the car colour/normal maps
4096×2048. This resamples existing detail; it does not invent detail. Paint/transparency
alpha is filtered separately, normal/specular/glow data uses linear filtering, and the
original authored mip levels are retained for distant views. Cube reflections and shared
effects stay at their original resolution. Local results are cached under
`work/texture-cache/v1`; missing or corrupt entries are regenerated from the installed pack.
`--original-textures` (or `--texture-scale 1`) restores originals; `--texture-scale 2`
uses less texture memory. No files in the game install are changed.
The 4× maps use about 422 MiB including mip levels; the first launch generates the cache.

Inspect/export the maps with
`cargo run -p tf2-core --example texture_upscale -- work/textures/bumblebee-4x 4`.
The generated PNGs, cache and comparison originals are local game-derived files, not
redistributable assets; they are never bundled into the executable.

The keyboard and mouse use the port's own **modern layout** since 2026-10-21 (asked for by the
user; `tf2_core::input::modern_map`): the mouse turns the camera and the aim directly by its angle
(`--mouse-sens <degrees per count>`, default 0.065), with no dead zone. `--classic-controls` gives
back the original's shipped bindings, in the right-hand column of the second table, with the mouse
as a camera stick. The pad always has the game's own bindings.

| Keyboard and mouse (modern) | Pad (the game's own bindings) |
|---|---|
| W A S D | left stick |
| mouse (click the window first); arrows | right stick |
| Space | bottom face button |
| Left Shift, held | R2, held |
| left mouse, while the right is held | R2 (fire) |
| left mouse; Left Ctrl | left face button (melee, the car's gun; Ctrl for the dodge in weapon mode) |
| right mouse, held | L2, held |
| Shift + S | L1 (brake) |
| mouse wheel, R | R1 (next weapon) |
| E | right face button (action, climb) |
| Q | top face button (special) |
| middle mouse | R3 (camera reset) |
| Z | L3 (strafe mode) |
| C | D-pad Up (camera distance) |

| Classic keyboard (`--classic-controls`) | Pad | Does |
|---|---|---|
| W A S D | left stick | move on foot, steer the car |
| mouse (click the window first) | right stick | camera |
| Space | bottom face button | jump; turbo in the car |
| Left Shift or left mouse, held | R2, held | vehicle form and throttle; let go to change back; in weapon mode, fire |
| R, or S in the car | L1 | brake, then reverse |
| Right mouse, held | L2, held (more than half way) | weapon mode on foot; the slide in the car |
| T | R1 | next weapon |
| C | D-pad Up | cycle the three camera distances; the selected level applies to robot, weapon and car forms |
| E | R3 (press right stick) | recentre the camera behind Bumblebee; clear the added driving-camera pitch |
| middle mouse | left face button | melee (tap again in the window to chain three hits; hold for the charged hit); in weapon mode with the stick, dodge; in the air, tap for the air attack, hold for the ground punch; held as the car's trigger is let go (after 0.25 s of driving), the slam out of the car; held in the car, the car's gun (two rounds a firing, straight ahead, from 0.75 s after the change into the car starts) |
| F | top face button | the special ability: a shockwave that stuns what is within 15 for 2.5 s (scouts play their stun animations); 23.5 s before the next |
| Left Ctrl | right face button | climb, facing a wall at least a jump high; on it the stick climbs up, down and along (the pad's own directions, not the camera's), jump leaps up it, jump with the stick back leaps off it, and pressing again lets go; held as the car's trigger is let go, the punch out of the car (a hop into the strong ground punch) |
| H | | take a synthetic bullet hit, playing the partial reaction while health comes back |
| B | | draw the bodies his attacks hit with, while they are live |
| F5 | | reset Bumblebee and the scout encounter |
| F3 | | a second robot in a second window, for two players (below); F3 again closes it |
| F4 | | move the focus to the other window (the window with focus gets the keys, mouse and pads) |
| F8 | | the sound debug panel: every sound in the mix, who started it, how far and how loud; F9 freezes it and prints it to the console |
| F1 | | hide or show the help panel |
| Esc | | free the mouse |

Press jump within a quarter of a second of letting go of the vehicle trigger for the two jumps
out of vehicle form (the long jump when moving, the high standing jump when nearly stopped).

The slide's boost now follows the original rule: hold the slide while moving sideways
faster than `minDriftTurboSpeed` long enough to charge, then release. Forward speed alone
does not earn it. Standard turbo keeps its timer/cooldown when changing back to robot;
its effects stop at that change. These rules are read from the executable;
the feel of drift still needs comparison with the original.

Standard boost now applies the original duration and cooldown upgrades. The default
matches the user's2026-10-07 capture: level3, **4.8s boost /1.6s cooldown**, compared
with the base **2.4s /3.2s**. `--turbo-upgrade 0` selects base values;1,2 and3 select
the installed upgrade tables. This is a capture-matched starting selection; reading
the original save's upgrades is still pending. Reset/death preserves the selection.

The arena is not game content: ground with a 10-unit grid, three platforms just under the
running, standing and out-of-vehicle jump heights, a kerb, a wall, pillars, some blocks, and a
climbing wall as high as the first one climbed in the recording (43.3) with a lower block in
line with it. The tall blocks provide climbable walls.

Behind the start are continuous slopes with matching collision, camera obstruction and
surface damage (custom test geometry, not an original level):

- Left: a **blue gentle ramp** at x -60, from y -40 to -90, to a 5-high deck ending at
  y -110. The old staircase has been replaced by a smooth slope.
- Next to it: an **orange launch ramp** at x -95, from y -40 to -90, rising to 14 with
  an open drop at the lip. Approach along negative y in car form to launch.
- Right: a **green hill** at x 82, from y -35 to -115, rising to 8 at y -75 and descending
  smoothly to ground. Test climbing and descending it in either form.
- Farther right: a **purple bank** at x 125 to 165, y -35 to -130, rising sideways from
  0 to 6. Enter from the low edge at x 125, then turn along the lane to test car roll.

Two suspension lanes are **to the right of the start**, approached along positive y:

- **Cyan rollers**, centred at x 98: three 12m-long bumps, 0.4 / 0.7 / 1.1m high,
  from y 4 to 52, with flat gaps. Drive slowly first to see compression and rebound,
  then faster to test unloading and landing.
- **Yellow alternating bumps**, centred at x 122: staggered 0.65m-high bumps on
  each half of the lane, from y 4 to 66. Straddle the centre seam to load the left
  and right wheels separately and test body roll. Start straight before the first bump.

These are custom test fixtures using the same visible and collision surfaces. The
stepped climbing tower, gate, slalom and long turbo avenue remain. Ramp walls do
not supply climbing ledges; use the existing blocks/tower for climbing.

The **Forked Crown climbing building** is farther right and forward from the spawn,
centred near x 221, y 110. Look for the teal/purple wings and gold overhead bridge.
This custom U-shaped building has an open courtyard, projecting orange buttresses,
four setback terraces (12 / 26 / 40 / 54 high) and a narrow roof wall reaching 62.

- Enter the courtyard under the bridge, then climb the blue terrace faces one after
  another. Each landing leaves room to walk forward and grab the next wall.
- Try climbing up/down and shimmying along the long wing walls; approach their inside
  and outside corners from both directions. The orange buttresses offer short faces
  and intermediate roofs for wall jumps, re-grabs and pull-ups.
- Test the bridge's underside and the wing walls beneath it for clearance, then the
  narrow gold wall on the highest roof for sideways limits.

Press **E** facing a wall; **W/S** climb up/down, **A/D** move sideways, **Space**
jumps up the wall, **S + Space** jumps off, and **E** again lets go. Sharp corners
currently end a climbing face; automatic corner wrapping, curved/slanted climbing
walls and floating ledge catches remain unsupported by the arena's box ledge adapter.
Start at its first terrace with `target\debug\bumblebee.exe --start 221,97.5,0`,
or at the courtyard entrance with `--start 221,70,0`.

**Climbing on a mission level** (`--mission <name>`): the wall, its heights and its corner extensions come from the authored nav mesh (`LevelWorld::ledge_from_mesh`, `core\src\level_world.rs`). The former collision-height workaround was removed on 2026-10-14 after correcting nav decoding and restoring the original candidate/visited rules. Checked escort spot: `--mission USA_East_LTrain_A_Escort --start -220.678,-65.623,0`; hold Climb with `--climb-from 1`, then forward with `--stick 1 --stick-from 2.5 --stick-until 7.5`. He finishes the roof pull-up by 10 s at z 55.635. Horizontal test (`--jump-at 0.5 --climb-from 1 --stick-x 1 --stick-from 2.5 --stick-until 5.5`) now traverses the uneven facade rather than pinning: about 15 m farther at 4.5 s than the old build. See `docs/climbing.md` facade follow-up and status for captures and assumptions.

Complex-surface fidelity remains incomplete: level body collision is a ring proxy, front-contact points are sampled, and the original pencil contact solver/query seed/layer selector are still owed. The pullback preserves the recorded 32 ms decay rate across render steps; its interpolation and an initial edge-band fallback are tagged assumptions. These fixes verify our reported facade, not full original-game parity. `core\examples\ledge_check.rs` lists spots; `climb_contact_check` replays the facade with installed clips at three step sizes.

Cars now have independent wheel contacts and spring-driven pitch/roll, with continuous
oriented body collision against the wedges and blocks. Suspension now follows the
verified physics handoff: wheel
queries use a temporary ground frame, springs retain full3D force, body angular
velocity uses world axes, and all forces accumulate before one body pose update.
The former projected spring-force bridge is removed. Recorded-frame tests cover
6,760 seeded spring updates and101 body rotations. The original .4m render radius
and +/- .1m wheel travel are retained; shallow mesh overlap remains a comparison item.

Original Lux contact response and wheel sphere interactions still need verification
(`docs/driving.md`).

The arena starts empty. With `--scouts` (or `--enemy-at`), three Decepticon scouts stand to
the left of the start. They shoot and attack in melee,
take damage through the shared character receiver, and play their original hit, stun and
death clips. A dead scout stays removed until reset. Bumblebee's completed death resets
the encounter. Their arena steering is a stand-in for the original enemy AI.

Spawners (`--mission <name> --spawners [name]`, with `--mission` only; `targets.rs` `spawn_run`,
`core\src\ai_spawn.rs`): the mission's CharacterSpawnManagers are read from its `mission.str`
and logged at load (name, records, maxAtOnce, lifetime, startEnabled). One second after the
load a stand-in Enable goes to every spawner, or only to those whose name contains `[name]`.
A spawner makes its enemies at its spawn points. Real: the spawn records and point rules, the
characterDefinition to pack table (`ai_spawn::character_inputs`: the hunt's AProt scouts and
the tutorial's DComWarrior are built; AProtSniper and AProtWarrior, which have no pack in the
install, and the player's side Optimus, Bumblebee and Firestorm are skipped, each logged once),
messageListOnSpawn delivered at spawn, and the game's brain for each spawned enemy. Spawned
enemies never reset: they are removed 3 s after death (or when he restarts), and a death is
what the spawner counts. Send and DeleteAll actions are logged or carried out, but no mission
script consumes them. Stand-in: the Enable message (no mission scripting, phase 4); the update
runs once a frame with the frame's dt, not on a fixed step; the ground is the mission collision
(`Arena::floor`); the straight corridor is taken as open without the nav mesh; homeRegions are
not applied; a Send's targets are not routed; the spawn facing and the id range (from 0x1000)
are assumed; a team test (`combat.rs` brain_input: foe when the team differs) is assumed, and
it has a consequence: the hunt's AProt scouts share Bumblebee's team value (2501572358, both
read by `team_values_of_the_spawn_and_player_characters`), so they are not his foes until the
team relation is read.

For deterministic checks, `--passive-enemies` prevents their attack requests,
the scouts run the game's own brain by default since 2026-10-20 (`--stand-in-brain` gives the old stand-in brain, which holds fire nonstop; `--ai-brain` is accepted and changes nothing); with the game's brain, `--combat-report` it prints the brain's state once a second per enemy (position, his position, flat distance, the tactics point and whether it is valid, path source mesh or straight, shots fired, group, and whether it has a home point) and each new tactics point as `tactics: scout N point (x, y, z) Dt .. Dc ..` (Dt: point to him, Dc: point to the scout, both flat),
`--combat-report` logs hits and resets, and `--keep-dead` preserves Bumblebee's corpse
for a capture. `--difficulty easy|medium|hard|expert` selects the install's difficulty
scales (default Medium); importing the original save's selection is not implemented.
`cargo run -p tf2-core --example combat_check`
checks an installed scout's gunfire, EMP response and death without opening a window.
`cargo run --release -p tf2-core --example pvp_duel -- [gun|melee|both] [--at <m>] [--strafe] [--no-snipe] [--head] [pack:Name...]`
fights every pair of the roster to the death (robot against robot, multiplayer weapon attributes)
and prints win/loss tables; with exactly two robots it prints each duel's damage, rounds and
health. Rounds go through the port's gun path, spread and projectile flight against the posed hit
bodies; the Audi snipes from 40 m. `PVP_DBG=1` prints the first shots, `PVP_DBG=2` a fire log.
Scripted bots [assumed]: the unpublished notes "PvP duel harness".

## What is real

Read from the install at runtime:

- Both models (robot and Camaro): meshes, the skinned torso, materials, textures, node trees.
- 137 animation clips for the robot and 22 for the vehicle, with loop flags, blend times and
  the events that show and hide each model during the change of form.
- Every tuning number used: health and regeneration, turn speed, jump values, gravity, drive
  speeds, turbo time and cooldown, braking, camera distances, angles and field of view.

Following from that data, or read from the original's code:

- On foot he moves at the pace of the clip that plays: walk 5.6, run 24. The stick threshold
  between them (70%) is the animation rules' own.
- Turning rate on foot (360 degrees a second).
- Which jump he does, how high each goes (standing 12, running 10, out of vehicle form 14),
  the launch speed, the take-off speed of a running jump, turning in the air, what the stick
  asks for in the air, and the speed cap on landing.
- Hold-to-drive, and the change of form: both models' transform clips, played together and
  switched by the clips' own events.

Since the first version, matched to a recording of the game or read from its code (all in
`core\`):

- On-foot pace and stopping, air control, landings; the car's rules (speed caps, braking,
  reverse, turbo, steering, wheels, the slide, leaving the car); the follow camera and the
  driving camera with their collision.
- Camera audit completed2026-10-07: automatic Climb recentre, airborne wheel-clearance
  tilt, ground-punch/death framing, distance-change handovers, overhead response,
  sphere sweeps and actual corner geometry, near-plane correction and occluder fades.
  Shake defaults, per-draw clock mixing and final render-matrix composition are also
  checked and implemented. The shake RNG remains camera-local; the original's complete
  shared random-call sequence is not reconstructed.
  Follow/drive/weapon behaviour is replayed against both original recordings. The
  authored arena supplies different collision geometry, so this does not establish
  exact original-level or shake-motion parity. Details and limits are in `docs/camera.md`.
- His sounds: the cues in his clips, the engine, turbo, and weapon sounds. A melee hit
  sounds by the surface it struck, from his pack's table (flesh-and-metal, concrete, glass,
  metal, sand, water, wood), picked as the game's code picks it. Scout target cylinders count as a
  character's torso (a stand-in).
- A hit's damage type flags, from each clip: a target is thrown only by a hit of a
  throwing kind, so the first two hits of the chain never throw; the third, the charge
  and the blast do (the air attack and the slam were not looked at one by one).
- The trigger, from the game's code: a round leaves when more than the weapon's rate of
  fire has passed since the last, looked at once every update of the game (so the cannon
  fires every 0.16 s); the heat meter and overheating (a round's heat, the wait before it
  falls, the lock when it fills, empty after it); a full meter refuses the trigger with
  its sound, again every rate of fire while held; shots scatter over a small disc in the
  picture that is the same size on screen at any range; the missile launcher fires when
  the trigger is LET GO after being held (the hold is the lock-on: a target in the
  128 x 128 box in the middle of the picture for 0.4 s is locked, and the missile steers
  at it; read from the game's code 2026-10-04, `docs/weapon-mode.md` "The lock-on and
  the missile"; the bracket round the target is the game's own, see the HUD below); a
  round shakes the pad. No recording of the original has any firing to
  check this against.
- Firing, from the game's code (2026-10-04): the trigger goes through the state machine's
  own firing state, an overlay state on top of the weapon states (it starts one update
  after the button goes down and ends one after it is let go; a jump or a landing
  interrupts it for two updates). The aim point is what the camera's line meets from
  beside him out to 500 (the scouts are in its way too). A gun's round is traced from the
  MUZZLE towards the scattered aim point over the weapon's range and lands at once, as in
  the original (`tracerFirePerRound`); an earlier version traced it from the camera, on a
  misreading (only a sniping shot starts at the camera, and he has no sniper camera). Its
  damage is the data's (15, twice on a head, no fall-off for his cannon). Rounds hurt the
  scouts. The muzzle is the weapon's own `FireDummy` node.
  Stand-ins: target collision (a cylinder, all of it "upper body") and bullet tracers.
- The car's gun, from the game's code and data (2026-10-04, `docs/weapon-mode.md` "The
  car's gun"): its own firing state on the left face button; the weapon manager takes it
  in hand as the change into the car starts and holds its trigger for 0.75 s
  (`vehicleWeaponTimeTilCanFire`); a firing is two rounds, one from each of its two barrels
  on the front wheel wells, each sent straight ahead of its barrel (not at the camera's
  aim point), scattered and traced like the cannon's; 7 damage a round, a firing every
  0.2 s (0.224 with the game's update), its own heat. Its model is the game's.
- The special ability (same day, "The special ability"): the top face button starts the
  game's own state for it, which sends his object the script his data names; that sets
  off a stun shockwave (a sphere of 15, no damage, 2.5 s of stun), its dome mesh, its
  particles and its sound, and starts a cooldown of 23.5 s. It works on top of firing, and
  in the car. Scouts use the install's StunSet and its break-damage budget.
- The aim assist (same day, "The aim assist"), on by default in the original's options:
  over a target in a small box in the middle of the picture the aim turns at three
  quarters of its speed (unless it is following a target that crosses faster, or is pushed
  hard away from one); while he moves and the aim stick is not pushed hard the aim drifts
  toward the target at 5 degrees a second; and on taking aim it turns by itself, at 270
  degrees a second, to a target within 30 degrees of where the camera looks.
- Weapon mode on foot (`docs/weapon-mode.md`): the game's StrafeMode and its weapon set.
  With the stick pushed he faces the aim at once and one of eight run clips, picked by the
  set's own rules from the stick's angle to the aim, moves him at 20; the clips cross-fade
  as in the recording (a start is one update of the forward clip, then the side one fading
  in over 0.3 s). Standing, his spine twists to the aim up to 45.5 degrees and a turning
  clip takes his feet round past 45. Matched update by update to the strafing in the
  recording (starts, stops, changes of direction, going into the mode at a run).
- Weapon mode in the air, read from the game's code and his data: a jump while aiming
  keeps him facing the aim (he is not turned to the stick, so he jumps sideways and
  backwards), with the weapon set's own take-off to the left, right or straight, its rise
  and fall, and its landings on the spot or running on one of four ways. Near the top of
  the jump time slows: gravity goes down to a quarter and comes back as he passes the
  height it began at, so an aimed jump hangs for about a quarter of a second longer.
  Taking up the aim in a plain jump does the same from where he is. NOT checked against
  the original: the recording has no jump in weapon mode.
- Melee, the charged hit, the dodge, the air attack and the ground punch: when they start,
  chain and end comes from the game's own control state machine (`bnxglobal.str`), run with
  the original's rules for its button detectors and transitions; they move him by their
  clips. The dodge is only possible in weapon mode, as in the original.
- Character and gun/axe fades keep outgoing clips playing, including interrupted fades;
  loop rule clocks, landing overrun and the idle fidget use the original rules. The
  melee arm/forearm/axe trails use the install's links, texture and clip trigger/stop keys.
- Full fast/slow flyback and recovery, additive partial hits and airborne/vehicle death
  exits use the real animation sets. `--hit-at s --hit-flags hex` supplies a synthetic
  receiving hit for captures (10000 fast flail, 20000 slow flail, 0 partial).
  Scouts now supply incoming gun/melee packets through the shared damage receiver.
- Receiving damage (2026-10-07, `docs/damage.md`): the original health, team/self and
  invincibility gates, difficulty/shield/vehicle/overdrive scaling, cubic regeneration,
  stun and its break budget, disable timers, DOT, slow movement/fire/heat, received heat,
  knockback and wall-splat selection. Explosion deaths use XYZ root motion; ordinary
  deaths use XY roots and gravity. Completed deaths request NPC removal or player reset.
  The scouts load their own HP200, gun, attack clips, rules and tuning from the install.
- The ground punch's blast: the explosion the punch sets off at his fist, growing to a
  sphere of 15 (18 for the strong punch) in 0.15 s and hitting everything in it once for
  100, read from the game's code and his data. Its effects are not there.
- Camera shakes: the game's own shake (its settings from `camera.lxb`, its fade, its reach
  and its random chase read from the code). The ground punch's blast starts one where it is
  (`MassiveCameraShake` for the strong punch, `MedHardCameraShake` for the weak one), and
  his landing clips and the stop from a run start the ones their events name. The shake
  events on the punch clips themselves name shakes the game does not have, so they do
  nothing, as in the original. Runtime mixes clock seconds into each draw; deterministic
  offline checks inject a fixed clock. Its final affine composition matches the render
  path. Original shake motion still needs comparison; a still frame cannot establish it.
- Pad rumble: the game's own effect (two motors, each a strength, a hold and a fade), and
  the ground punch's rumble from his data, weaker with distance. Only the punch starts one
  so far; landing, climbing and the car's knocks (the game's `rumbleEffects` table) do not
  yet. Not felt by anyone yet.
- Every change of what he is doing goes through that state machine: standing, moving,
  jumping, falling, weapon mode and the change of form both ways, with what each state does
  when entered read from the game's code. Folding into the car keeps him on foot, turning,
  until the stick lines up with his motion; unfolding slows him at the exit rate for the
  clips' 60 ticks; the leap out needs the jump button held as the trigger goes up (jumping
  while he unfolds does nothing). The ground flag the machine reads trails the body by an
  update, so a landing ends the jump two updates after touchdown, as in the recording.
- Climbing (`docs/climbing.md`): what counts as a wall (a step at least a jump high, along
  his facing, faced within 45 degrees), the climb mode's rules (the clips for up, down, along,
  hanging and the pull-up in a left and a right hand, the hand change, the side limits, the
  pull-up within 8 of the top, the arriving speed dying away, the pull back to the wall), the
  wall jumps, and the camera standing off 15 while he climbs. Matched to the 12 climbs in the
  recording: hanging 2 off the wall, the climbing speeds, the hand change and the point
  where the pull-up starts.

Assumed or stand-in:

- States not built are never entered: carrying and hover. The
  state the machine starts in (standing) and the drive states' own entry test are assumed.
- Weapon mode on foot, what is assumed: which bones share the spine's twist (the two his
  `SpineIK` links, equally); how much of a turning clip's turn he gets while it fades in;
  that the update the mode starts keeps the stick's angle from the facing he had (the
  recording shows the two updates this gives; the reason is not read). In the air: that
  the clips play at their own rate during the slow motion, and that a standing aimed
  jump lands with the long-fall landing (its fall comes to 1.06 s against the rule's
  1.0).
- Melee against the three scouts left of the start (`docs/melee.md`). From the game: which
  target an attack goes for (along the stick, or his facing), the turn and the slide up to
  it, one hit per target per swing, each clip's damage and throw, and what he hits WITH: his
  own collision bodies from his pack (a sphere on each hand, the boxes on his feet, a drum
  round him for the slam), switched on and off by each clip's events and placed where the
  clip has that limb. B (or `--bodies`) draws them while they are live, and a white dot
  where a hit touched. This toggle also shows weapon explosion radii and shot impact
  spheres; those overlays are hidden during normal play. The melee upgrades' damage scaling is there, at level 0 (nothing
  raises it yet). Stand-ins: the target's side of the touch (one upright cylinder, where
  the original has the target's own limb boxes) and how the touch is found. Scout health,
  reactions, throws and death use the shared receiver and controller. Assumed: the
  hand spheres are 1.83 across (stored as 0.93 under a scale of 1.97); the drum stands
  upright. Clips with no blend time of their own get 0.1 s, the game's value.
- The axe (2026-10-04): his melee weapon's own model, brought out, unfolded and folded
  by the attack clips' events on his left hand's node and put away when he leaves the
  attack states, as the original does it. The blade uses the original emissive shader;
  its ribbon trail and the robot's arm trails follow the original timed/exit keys.
  Trail message forwarding and blend-exit callback timing still have logged assumptions.
- The aim clips (2026-10-04): every weapon-set clip carries two offset clips, `Down` and
  `Up`; the aim's rise lays one on (in full at 65 degrees), which bends his spine, arms
  and head so the gun points where he aims. From the game: which clip, how much, and
  how offsets are added. The weapon layer's blending (the arm clips' fades) is the
  game's too. Character and weapon-object fades retain moving outgoing players,
  including interrupted blends. Melee collision bodies still sample the attack clip
  alone during a blend.
- Carrying remains unfinished; wall-splat contact classification uses the arena's
  estimated normal and body kind. Jump, fall,
  landing, fidget, death and hit recovery use the install's animation rule sets.
- What weapons' shots look like (lines, a ball; the missile's trail is the game's
  effect); aim assist still uses scout target cylinders rather than per-limb contacts.
- Scout placement, steering, engagement and attack-delay scheduling. Original brains,
  tactics points, navigation, shared kung-fu slots and NPC aim smoothing are not ported.
  Scout effects/audio cues are incomplete; the installed fire sound event is requested.
  Arena reset stands in for a mission reload. Campaign stats/drops/exclusions and enemy
  vehicle/special/weapon-selection AI remain outside this arena implementation.
- Foot IK is ON again (user's call, 2026-10-20) [stand-in]: no playable pack has a FootIK object
  and the game builds the solver only from data, so the original does not plant the robots'
  feet. `--no-foot-ik` turns the port's own planting off, to match the original.
- Footsteps probe each named animated foot and select its ground-material preset.
  The arena ground still supplies Default; source positions have independent stereo
  projection in place of FMOD's full3D renderer.
  (The engine's revolutions are the game's gearbox since 2026-10-05.)
  Engine playback corrected2026-10-08 from embedded FMOD code: authored sample-span
  crossfades/curves, original two-stage low-pass, verified RPM seek/pitch, and no skipped
  frame at a loop wrap. The invented seam blend is removed; loops retain their authored
  FSB endpoints. Category gains, lock/regen cues, collision impacts/scraping and
  Bumblebee radio chatter now play. Chatter uses authored frequency, repeat history
  and delayed start. Full FMOD spatial mixing/Doppler/reverb remain incomplete;
  listening parity with the original is pending (`docs/sound.md`).
  Camaro boost's additional sound follows the original turbo FX on/off signal, including
  leaving driving mode; trigger players retain authored retrigger flags/idtext precedence.
  Missiles have independent flight loops and detached explosion cues. Material bullet
  impacts, climbing wall contacts, and the ground-punch's spawned sound now play at
  their sources. Projectile source offsets and asynchronous cue timing remain assumed.
  The audio follow-up retains authored base pitch, full pitch-randomness ranges,
  pitch envelopes, sequential/no-repeat/shuffle sample selection and event fade-in.
  Event fades keep their authored duration across device buffer sizes. Triggered
  component sounds follow their animated robot/car attachment node. Full spatial
  audio and original listening comparison remain open as documented above.
- The car's tyres (a rolling loop, a skid loop on the slide, a hard brake or a hard turn
  at speed) and the suspension's thump follow the game's rules since 2026-10-05, but the
  arena ground counts as the default surface everywhere. Since 2026-10-07 the thump
  follows actual rear-wheel spring compression and tyre loops follow the selected
  rear-wheel source; the stereo mixer still stands for FMOD 3D.
- [port] By default (2026-10-21) the keyboard and mouse are the port's modern layout, not the
  original's: the mouse turns the follow and driving cameras by angle through the camera's
  `nudge` (applied at the camera's 0.032 s updates) and the weapon aim by angle every frame,
  past the aim's ramp, curve and assist slow-down; moving it lets go of the aim's snap to a
  target. Not yet played by the user. With `--classic-controls` the rest of this bullet holds.
- The classic mouse is the original's since 2026-10-04: a camera stick (counts in a frame over 10,
  stopping at 1), so it turns at the stick's rates and is slower than a free mouse. Since
  2026-10-16 it also passes the original's stick dead zone (`core/src/input.rs`): one or two
  counts in a frame do nothing. Keyboard, mouse and pad combine by the larger value per
  action, as the original does.
- Climbing: the arena's blocks stand in for the level's ledge mesh (so no wall has a ledge
  above its top, and the jump up from the top with its catch never happens in play); the
  wall jumps moving him by their
  clip without gravity until they take off, and the jump off turning with its clip's root.
- The pad's preset rumbles (2026-10-04), as the game plays them: a melee hit that lands
  (0.25 s, or 0.45 s for the charge and the slam out of the car; the ground punch's blast
  too, on each target it reaches), climbing DOWN (going up and sideways the game plays a
  rumble of no length, which is silence), the car's turbo starting, the car coming down
  onto its wheels, the car hitting a wall at over 5. A landing on foot does NOT rumble:
  nothing in the game plays that preset. Assumed: that every touch of a wall counts.
  Nobody has felt any of it yet.
- Particle effects (2026-10-05, `docs/particles.md`): the game's own particle system,
  run on the game's data and drawn with the game's own quads and particle shaders: the
  spark of a melee hit, the ground punches' dust and shock rings, the dust of a landing
  and of the change of form, the dodge's jets, the charge on his hand, a walking step's
  dust, the special's stun, the cannon's and the launcher's muzzle flash, a round's
  impact by the surface, a missile's explosion, the turbo's burst and the exhaust. From
  the game: when particles are made, how many, how they move, grow, turn, change colour
  and fade, the random numbers, the textures, the quads' layout, the blending, the soft
  edge against the scene. Footstep contacts select the surface preset; the car's
  effect senders are read. Assumed: synchronous foot/climb probe timing, some weapon
  effect anchoring, which way the camera's two vectors point, the original's near plane.
  Smoke and dust are lit by the
  original's lit particle shader and the heat haze bends the picture behind it. The
  arena borrows the light and look of a real level (`us_west_town`: sun, ambient, fog,
  bloom, colour filter numbers), through Bevy's own bloom, fog and grading
  (`--no-level-look` turns it off). Tyre dust, skid smoke and skid marks, the exhaust,
  headlights, the damage smoke (under three quarters of his health), the glow of health
  coming back, the gun's overheat smoke and the climbing dust start as the game's code
  starts them. Added2026-10-08: the original0.05 particle alpha cutoff, vertex depth
  fade/fog, correct shared-material fade ranges and foot/climb dust from animated
  surface contacts. Lit coefficients' fourth component is verified as zero. Fog's
  environment transfer remains assumed; original bloom/exposure passes are still open.
  Added2026-10-09: muzzle particles follow MuzzleFlashDummy or the car's firing barrel
  with the node's rotation. Particle playback samples emitters at60Hz, so exhaust
  does not disappear at high render rates. The clock's1/60 base is verified; desktop
  resampling remains assumed. The installed exhaust has no stationary source.
  Missing: muzzleflash_orange's0.05s omni light; lit corner normals remain assumed.
  Added 2026-10-07: skid marks' dull_vc shader, CollisionFX impact/roll/
  scrape presets and the car gun's moving BulletTrace trail. Contact-kind mapping and
  synchronous trace endpoints are assumed. Turbo screen blur now uses the original
  speed mask, quarter-size target, Gaussian13 kernel and .98 centre stretch; it stops
  on turbo expiry and transformation even if the turbo timer continues. Bevy pass
  ordering and colour domain remain assumed. Gaps: the other original post passes,
  exposure and general depth of field; heal, stun and frost. Not compared with the original.
- Camera shakes: stepped every 0.032 s (the original steps them once a frame, and how they
  look depends on the frame rate); the turn is taken about the camera's own axes; a clip's
  shake is placed where he stands.
- His surfaces use the game's own shader arithmetic (`surface.rs`, `surface.wgsl`: paint,
  metals, rubber, glass, and since 2026-10-14 the level's Surface techniques (dull, cheap, basic
  and blend2 with their _ssd forms, cheap_plus_boost, the second diffuse layer; cut-out and additive
  blends; the sky is in `sky.rs`), but in the arena the light's colour and the ambient are ours; on a mission
  the light and the ambient come from the level's Environment (the unpublished notes
  LS-20), with the level's fog (LS-29) and its omni, spot and capsule lights (LS-37, zone pack
  only; `CAPSULE_LIGHTS`). What the paint reflects is still ours in both. No bloom. What glows (his eyes,
  the weapon's lights, the axe's blade, the shock rings) uses the game's two unlit
  shaders (`fx.rs`, `glow.wgsl`). The arena uses Bevy's standard lighting with an SSD
  material extension (`ssd.rs`, `ssd.wgsl`).

The HUD (2026-10-05, `docs/hud.md`), from the game's own art and code: the reticle (the
executable's own drawing: the base for the weapon in hand, red on a target; the lock
lines and the bracket round a missile's target; the hit mark; the two heat meters; the
weapons' icons; the car gun's two bases) and, from the Flash movie `hud.gfx`, the health
bar, the ability hexagon and the turbo indicator, drawn from the movie's shapes and moved
as its scripts move them, fed the values the game's code feeds them (`UI\uiroot.lxb`
binds them). Assumed: that the turbo indicator shows only in the car. Not there: the HUD's text, radar, counters and
messages, the meters' icons, the arrows towards an attacker. The readout at the top left
and the "DESTROYED" message are this build's own. Never compared with the original.

Surface damage (2026-10-06, `docs/surface-damage.md`): fists, both guns, missiles and
ground punches leave persistent marks on the arena. Preset radius, frame growth, volume
merging/cap, noise texture and crack/damage mask arithmetic are from the game. Arena
contacts, borrowed concrete maps/UVs and Bevy lighting are stand-ins; seeded noise
transform order remains assumed. No comparison with the original has been recorded.

Foot IK (2026-10-07, `docs/foot-ik.md`): grounded standing, walking/running and
weapon stance adapt the feet and legs to ramps, banks and uneven support. Animated
foot lift and toe motion are retained; the visual body lowers to keep leg lengths.
The original generic solver supplies probe/solve/smoothing constants. Bumblebee's
leg binding, sole proxy, activation and body/plane adaptation are assumed; arena
probes are stand-ins. Eight IK tests and captures checked; no original comparison.

Not there at all: what a hit does to a real target (hit reactions), the game's own post
effects, enemies.

Captures can script the new moves: `--melee-at 1.0,1.4` taps the melee button at those
times, `--melee-hold-from <s>` holds it and `--melee-hold-until <s>` lets go (the charge
needs the tap too: `--melee-at 1 --melee-hold-from 1`; after a `--jump-at`, that is the ground punch,
whose blast `--bodies` draws as an orange sphere), `--aim-from <s>` holds weapon mode
(`--look-y <-1..1>` holds the camera stick up or down while it does, `--aim-cycle <s>` turns
weapon mode on for that long, then off as long, over and over, `--switch-at <s>[,<s>...]`
taps the next weapon button at each time), `--stick-x <-1..1>` pushes the stick sideways,
`--climb-from <s>` holds Climb, `--jump-hold-from <s>` holds the jump button (a jet's turbo, or the hover when the flying trigger is let go), and `--start x,y,yaw` puts him somewhere to begin with
(`--start 60,87.5,0` stands him at the climbing wall, facing it).
Sniping (the Audi's PlasmaLongRifle, Breakaway's LongBeamRifle; `core\src\snipe.rs`): R3 (E) in
weapon mode toggles it, D-pad up / down (I / K, the wheel) zooms; in a capture `--recentre-at <s>`
presses R3 and `--zoom-in-from <s>` holds the zoom in (`--passive-enemies` keeps the Audi alive in
the arena). `TF_TRACERS=1` brings back the old yellow line along each gun round, as a debug aid.
`--enemy-at x,y` puts the first arena enemy at that world position (the fixed ones are about (-25,25), (-34,31), (-25,42)); the start prints `spawn: pos (x, y, z) yaw deg forward (fx, fy)` so a capture can pick positions.
`--stick-from <s>` and `--stick-until <s>` bound the forced stick (`--stick`, `--stick-x`) to that window; without them it applies throughout. `--look-from <s>` and `--look-until <s>` bound the second stick (`--look`, `--look-x`) to its own window; without them it uses the `--stick-from` / `--stick-until` window, as before. For example, `--look -1 --look-from 0 --look-until 6` lifts him for six seconds while the forced stick (if any) keeps its own window, so he can climb and then fly level. In flight (`core\src\flight.rs`, the pitch target is `-maxPitch * stick.y`, and positive pitch is nose up): `--stick -1 --stick-from 0 --stick-until 4` climbs for four seconds, and `--stick 1` dives; leaving the window at zero flies level (the pitch lags back to level). `--look <-1..1>` and `--look-x <-1..1>` hold the second stick (`FlyInput.look`, y and x) inside the same `--stick-from` / `--stick-until` window, and without them nothing changes. For a helicopter, `--look -1` lifts it (`core\src\flight.rs` steer_helicopter: `vel.z -= tun.lift * look.y`, so a negative value climbs and a positive one descends), and `--look-x` turns it. Note the camera reads the same stick, so a forced `--look` also moves the camera.
`--level levels/shanghai_01/zone.str --start -30,200,0` puts him on one of the game's own
zones in place of the arena: its drawn models and its collision ([stand-in]s listed in
the unpublished notes "wave 7"). `--collision-boxes` also draws the collision's cuboids,
`--no-level-models` draws only those.
`--mission Shanghai_01_Construction_A_CSA` (or `US_Test_FreeRoam_A`; any name of the
game's mission list, case sensitive) loads every pack that mission names (its zone and
its own pack), lights it with the zone's own environment and starts him at the mission's
player spawn point. It is a stage only: nothing of the mission runs (no script, enemies or
objectives). `--level-all` also draws the barrier cards (the sky models are drawn by default since 2026-10-13).
Robot switcher (test aid, `game\src\switcher.rs`, 2026-10-18): F6 / F7 change to the next / previous
of the 13 robots (Bumblebee, Optimus, Ratchet, Ironhide, Megatron, Long Haul, Sideways, Jetfire,
Breakaway, Starscream, Grindor, Powered-Up Optimus, Megatron flight), then Devastator (below). The game starts itself again
with the other `--character` and a `--start` where he stood, facing the same way, every other
argument kept (a robot's pack, sets, weapons and banks load only at the start). He comes back on
the ground at that spot, out of any form or flight; under `--mission` the spot overrides the
mission's spawn. `--robot-switch-at <s>` does one switch at that time in a capture (checked: Bumblebee
at (10, 20) facing 90 came back as Optimus there, `work\shots\switch_check.png`).
Robot menu (test aid, `game\src\charmenu.rs`, 2026-10-19): F2 opens the list of the same 13 robots;
Up / Down (or W / S) choose, Enter swaps to him in the running game, where you stand (no restart;
about 1.5 s of loading while you keep playing), F2 or Backspace closes. He comes back on foot at full
state; the arena's enemies stay the ones loaded at the start (the menu says when they are on his side:
F6 then restarts with the other scouts). `--swap-at <s>,<pack>` does one swap in a capture.
Second robot (debug aid, `game\src\duel.rs`, 2026-10-21): F3 puts a second robot 20 m in front of
him, facing him, with its own follow and weapon camera in a second window ("Player 2"). Whichever
window has focus takes the keyboard, the mouse and the pads, with the same default bindings; click a
window to take it and hold its mouse, Esc frees the mouse, F4 hands the focus over. The two are on
opposite sides, so each one's rounds and blows hurt the other. When it dies it comes back where it
started. `--p2 <name>` says who it is (as `--character`; Optimus by default, Bumblebee when he is
Optimus). It is an enemy driven by input (`combat::Driven`), so it has only its first robot gun: no
other weapons, no special, no HUD beyond its health, and in car form its robot model is still drawn.
`--passive-enemies` keeps the scouts still. Capture aids: `--duel` (on at start), `--p2-stick <y>`,
`--p2-fire` (weapon mode and trigger, aimed at his chest), `--duel-shot <png> [--duel-shot-after <s>]`
(saves the second window), `DUEL_DBG=1` (its state each frame).
`--character <name>` starts another character: a name or pack of the game's own
character list (`optimus`, `"long haul"`, `grindor`), or a pack with its attributes name
(`dcomscout:DComScout`). `--enemy <name>` puts one in the scouts' place. A character that
flies (Starscream, Breakaway, Jetfire, Grindor) changes form and flies while the vehicle
trigger is held; the stick steers, the jump button is the turbo, the weapon-mode trigger
held turns a jet tighter. The helicopter has its own steering: the move stick pushes it
along and across, the camera stick turns it and (pulled down) lifts it, the weapon-mode
trigger stops it. Walls: the flight boundary is read from the level's zone (`--mission` loads it; `flybound.rs`, wired into the fly step) and a jet bounces off its box. Checked in the game on US_Test_FreeRoam_A: Starscream leaves through the top and is brought back inside (149.8 m at 5 s, 131 m at 8 s, level near 136 m at 45 s); the sideways turn-back is in the core tests. A helicopter outside the box is slowed and steered back (wall stop `00868fb0`, core tests only: the helicopter never lifted in the capture, so it is unseen in the game). Only the first zone box in a level is used. A flyer is followed by the ordinary DRIVING camera, and both its look-at target and its FlightMode camera flag are wired: a flying character's drivingCamera names a driving-camera settings block whose 39 fields are the identical set to bbeeDriveCamera's, and the camera byte +0x195 is character_in_flight_mode (007256e0, motion mode fb9f3262 = sim::Mode::Fly), copied by the driving camera's slot 17 (00916030) and gating the elevation and eye rules of 009145b0 [game/data, 2026-10-18]. Measured: Starscream climbing at pitch 58 was all but out of the bottom of the frame and is now centred (work/shots/starscream_fly_cam_2.png against starscream_flycam_wired_2.png); level flight is unchanged (24.0 m from him against 22.8); Bumblebee is unaffected because he cannot enter FlightMode. Not compared with the original: no recording has a flying character. Still unread: whether a flyer's pitch or roll feeds tiltAmount/leanAmount, and the look-aside pair 00916610/009162d0. Flight sound is wired (a drive loop from the character's data with speed, pitch and roll; a helicopter loop with speed) but has not been heard, and the hover sound branch never ran in the game. The charge and flame effects are ported: the charge effect is scaled by the charge level, with the four charge sounds; the flame stream, three fireballs and their loop sound are wired. Not done: the flame impact effect and the overheat sound. Ported from the game's code and checked by tests and captures only, none against the original: hover (the jump button held as the flying trigger is let go; no gravity while hovering is assumed), stunts (the stick past 0.5 rolls to a side, past 0.9 loops; the action button arming them is assumed), landing (a fly exit hands over to the drive companion once he is on the ground), Long Haul's charged grenades (a held trigger throws up to three), and the flamethrower (three spheres, 22 damage a tick on what they touch, which is assumed).

Weapon specials (`special_weapon.rs`, wired through `game\src\weapons.rs`): Megatron's gun, Jetfire's missile dump and carpet bomb, and the Audi's mines. Optimus's special (`special_types.rs`) is seen working: scout hits of 2.0 become 1.0 for its 10 s, then 2.0 again. Seen in captures: Megatron's special plays its intro, holds the idle until its 2.75 s timer runs out, then the exit clip fires one gun round, which hits once; the special now deals its 200 explosion (the gun's explosion had not been read from a bullet's projectile data). Megatron's gun fires one round per event and does not stream. Jetfire's missile dump counts 8 rounds, and the port has them fly up and land 20 to 50 m out; the checker's captures did not show the missiles. The Audi's mine is thrown, comes to rest on the ground, and detonates near an enemy of the other team for 30. The car-form mine now fires from the car model's own `Special` clip (set `VehicleModel`; the car's mine drops at tick 14 of the 62-tick clip), and Blackout's car special starts the same clip and launches his turret (see below). Not checked or not settled: the direction of Jetfire's rounds (the port's sign is an assumption, and an offline emulation of the game's rotation disagrees with it, `docs/special-jetfire-mines.md`); the carpet bomb's spacing and fall; and Megatron's beam, which leaves from a stand-in muzzle 8.4 m up and is not clearly visible. Grenades and mines are drawn with the game's projectile model from a pool of 8 per weapon, a green sphere beyond that [stand-in]. Not ported: the mine's contact test (every world touch sticks), the team test (a mine's proximity counts teammates), and the expiry of an older mine (`oneOutAtATime`).

CombatRepair (`repair.rs`, wired for Ratchet and Long Haul): the heal zone is read from the character's pack (25 m on foot for both; `specialHealingType` read as a bool) and heals at the receivers' attribute rates. Seen in a capture: Ratchet's health rose 30 in 0.5 s (60 per second) and stopped at his maximum of 320. Long Haul's zone switched on and off, but his heal was not measured. [assumed] the zone runs for the special's 5.34 s; the car's zone (18 m for Ratchet) is not checked for Long Haul. Scouts heal from their own attribute rates [stand-in], with no heal numbers or visuals.

Deployable turrets (`turret.rs`, `weapons.rs` `launch_turret` and `tick_turrets`). Blackout's is wired: it is launched, lands about 14 m ahead, is Active after an assumed 1.5 s deploy, and fires its gatling (damage 8 a round, [assumed] bursts of 3) at the nearest visible enemy in range, the 15 degree cone gating the trigger. It is drawn as a box [stand-in]. Ironhide's turret launches, lands and is Active, but fires nothing: its missile trigger is not found and its damage values conflict. The car-form turret fires from the car model's `Special` clip, as the car-form mine does (see the Audi line above); its gun is not read from its own tree [assumed, `weapons.rs` setup]. Not checked or not ported: the team test for a turret's target, and removal by lifetime or owner's death. The scout's super jump is not built.

Devastator (`--character devastator`, or F6 / F7; `core\src\giant.rs`, `game\src\giant.rs`): the
60 m walking giant of the Cairo City boss mission, which the game never lets anyone play. His
machine is driven in the game by his brain and the mission script; here the pad sends the same
messages in their place [assumed port design]: the stick walks him and, pushed well off his
facing, turns him on the spot with his own turn clips; melee is the hydraulic pound (its
shockwave rings, from his clips' Trigger events), the special is the vortex that drags what is
near him in. His walk, turns, attacks, sounds, shakes and effects are his own data. His pods are
his weapons, read from the boss mission's pack: the right trigger fires them (he has no car), the
next-weapon button cycles gatling, missiles and bug bombs. The missiles fly a top attack down onto
the aim point; the bug bombs erupt a dozen at a time from his back. Their numbers are rebalanced
for a player (`giant::POD_BALANCE`) and every such choice is tagged `[assumed port design]`. Also
the port's own: his legs and arms planted on the ground with a heavy limb IK, the camera and
shakes scaled to his size, a sun shadow that reaches past his far camera, staggers when he takes
a share of his health in a short time (the help panel's state line says so), pod smoke and sparks, damage stages, and
his death. Not compared with the original: the game has no player camera or controls for him.
`--hurt-at <s>:<damage>[,...]` lands a hit on him at those times (a test aid for staggers and his
fall). `cargo run -p tf2-core --example devastator_clips` and `devastator_materials` print his
clips and materials; `cargo test -p tf2-core --test devastator` checks him against the install.

Grindor's cinematic energon wave (`core\src\energon_wave.rs`, `game\src\energon.rs`; a design
beyond the original): every shot of his energon cannon (weapon 2) opens a 70-degree arc of
Devastator's ground ring where the missile lands, running about 105 m over the ground: it climbs
what is low enough, stops at walls, and a shot into a wall opens at the wall's foot. Its effects
and sounds are the game's own, blown up round it. `--original-wave` keeps the original's narrow
wave alone, to compare. Capture: `--character grindor --start 0,0,180 --aim-from 0.5
--switch-at 1.0 --fire-from 2.0 --look -0.6 --look-from 0.6 --look-until 1.1 --zoom 0.5
--after 3.2`. `TF_BODY_LOG=1` prints the drawn place, velocity and angles every frame (used to
check the change of form back from a flyer, which no longer springs him up).

Capture flags for these (`game\src\main.rs`): `--special-at <s>` presses the special button at that time, and `--hit-at <s>` with `--hit-flags <hex>` gives him a synthetic hit (line 252). `--passive-enemies` stops the scouts hitting him, and `--combat-report` prints the rounds fired and the hits. `--enemy-at`, `--stick-from` and `--stick-until` (in the list above) are the flags for placing an enemy and windowing the forced stick. Tip, opposite team: the arena's default scouts are Decepticons, so a Decepticon character's damage on them is refused, as the game refuses it; for a Decepticon character use `--enemy aprotscoutmp:APrtScout` (Autobot), and say so in any check. Still missing: a way to script a second special press (`--special-at` takes one value), and the helicopter's lift is on the second stick, so use `--look` for it (the first stick forces only forward and sideways). `--hit-at` bypasses the damage rules, so it cannot test a shield.
`cargo run -p tf2-core --release --example level_walk` runs him
on it without a window; `--example zone_map` draws a zone from above.
`cargo run -p tf2-core --example climb_check` climbs through the real state machine and clips.

Performance (debug aids): `--fps` prints the frame rate, the average and slowest frame time and
the page faults every two seconds, and the time of each step of the start; it also keeps the
window at full speed when it loses focus. `--no-vsync` draws frames as fast as they come, so
`--fps` shows the true frame time. `cargo run -p bumblebee --example bare_hitch -- [seconds]`
runs a bare Bevy scene (none of the game's code) and logs every frame over 15 ms: when it stutters
too, the hitches are the machine's or the engine's, not the game's.

Stage reverb: on a `--mission` stage, sounds take the reverb of the zone's own reverb triggers
and areas, with the presets from `audio\eng\global.fev` (`core\src\reverb.rs`; the zone's
script is read for its objects and links only, `core\src\mission_script.rs`). The arena has none.
`cargo run -p tf2-core --example reverb_dump -- <pack.str>` lists a pack's reverb triggers and
who sets them off; `--example fev_tail -- <file.fev> [--reverbs] [--levels]` lists a sound
file's reverb presets and its events' reverb levels.

`--no-sound` runs silent; `TF2_SOUND_VOLUME` and `TF2_SOUND_LEVEL` set the volume and the
level whose shared sounds are used. `TF2_SOUND_FX_VOLUME`, `TF2_SOUND_UI_VOLUME` and
`TF2_SOUND_VOICE_VOLUME` independently scale those categories (default1,0 mutes).
`TF2_SOUND_TRACE=1` logs requested gameplay cues, every event the mixer starts, and any
waveform decoded while playing ("decoded while playing": one the start-up decode missed, which
would sound late), and once a second every event older than 4 s ("sound still": its age, how many
of its sounds are audible, its key-offs and parameters; for sounds that never leave). Each start
and change of form carries the output's time ("at output 5.07s", "sound form"), a slow output
callback is flagged ("sound output"), and each sound's own output is checked every quarter second
("sound fault": a constant offset, a level over 4x full scale, or values that are not numbers).
`TF2_SOUND_DUMP=<file.wav>` records everything mixed (stereo float WAV at volume 1, unclamped;
playable even if the game is killed) and a journal `<file.wav>.events.tsv` (every start, stop,
key-off, steal, end, fault, clip at volume 1, change of form and control state, slow callback,
at its output time). `python work\sound_dump.py <file.wav>` lists offsets, clipping, gaps and
non-numbers with the journal around each; `timeline`, `events [regex]`, `lives` and
`heard <out.wav>` (what the speakers got, clamped) are its other modes. Music/ambience
are off by user request.
Sound and animation checks without a window (`cargo run -p tf2-core --example <name>`):
`ground_sound_check [pack:Attributes...]` (each ground robot's engine over its gears, footsteps
and their probe, clip cues missing from the banks, weapon fire sounds), `sound_render ... bank=<name>`
(another character's bank), `muzzle_peek <pack>` (muzzle templates and dummies), `climb_reach`
(hands and feet against the climb stand-off), `scale_survey [pack...]` (clips keying either scale).
`--robot-camera` and `--drive-camera` (0 to 2) pick the
game's camera distance levels.
`--camera-cycle-at <s>` scripts one D-pad Up press in a capture.
`--recentre-at <s>` scripts R3. `--camera-sensitivity <0..1>` defaults to0.5;
`--invert-camera-x` and `--invert-camera-y` affect both camera and weapon aim.
`--separate-camera-distances` makes C/D-pad Up change only the current form's level.
`--disable-camera-stick` disables looking; `--camera-4-3` selects the original proxy's
4:3 near-plane extent (default16:9). Reset keeps these options.
`--camera-fade-test` adds a nonphysical orange test occluder behind the starting point;
its alpha drops to0.45 while obstructing the view and recovers at4/sec. This optional
object is an authored fixture; default arena blocks remain solid. `--no-level-look`
makes its transparent colour easier to inspect. Only the rebuilt game is run.

## Checking it without playing

The level render integration (2026-10-15) draws authored sky domes/cylinders, sky vertex colours,
fogged `bgnd_vc` backdrops and `emissive_vc` parts. Street lights are loaded per placement from the
zone and mission packs. Uncapped 32px screen tiles schedule all placed lights (311 in US_Test);
`--no-light-tiles` evaluates the entire light list per fragment as a validation reference, and
`--no-point-lights` disables it. Visibility boxes/masks, actual shadow tags, rotating fans/signs/clouds,
IFL billboard/flasher textures, three-layer surfaces and flatcard foliage are integrated.
Native LOD chain/box distance/draw limit selection, distance-faded two-layer barriers, direct
scene particle emitters and static native colour controls/LUT are integrated. Destruction/debris
are excluded from the current user request. Remaining rendering work: decals/distortion,
native fade coverage and shadow LOD, moving/triggered scene lifecycle, animation/Colorizer,
environment transitions and original bloom/exposure/SSAO/deferred lighting.
See the unpublished notes. Diagnostic switches: `--no-level-lod`, `--hide-barriers`,
`--legacy-level-post`; `--start-height <z>` chooses the floor below that height with `--start`.
Since 2026-10-16 the Colorizer, the iris (exposure), SSAO, the native bloom and the soft depth of field are ported
(the unpublished notes top); each has a comparison switch: `--no-iris`, `--no-ssao`, `--bevy-bloom` (Bevy's old
stand-in bloom), `--no-dof`, `--no-depth-bias`, `--no-ocean-reflection` (the sea's planar reflection off: black,
as the first cut; `game\src\ocean.rs`). `--fps` logs the frame rate every two seconds. Judge level pictures with `--mission <Name>`: `--level` runs use
another level's environment and no light rig or point lights.

From the repository's root folder:

```
cargo run -p bumblebee -- --check bumblebee      load a pack, print counts and checks
cargo test -p tf2-core                           the movement, cameras, sound and decoders
cargo run -p bumblebee -- --shot out.png [--after 2.5] [--yaw 160] [--elev 8] [--no-hud]
             [--clip WalkSet/Run@10] [--stick 1] [--jump-at 1.0] [--fire-from 1.5]
             [--vehicle | --vehicle-from 1.5 [--vehicle-until 3.0]]
```

`--shot` opens the window, optionally holds a pose or feeds input, saves one frame and quits.
`--orbit <deg>` swings the drawn picture round the point `--orbit-pivot <m>` ahead of the camera
(11 by default); a negative pivot swings it round him instead, at that height over his feet, looking at
him (`--orbit 100 --orbit-pivot -7` shows a robot's gun side-on in weapon mode). `TF_MUZZLE_LOG=1` prints
each muzzle flash's node against where its round leaves; `TF_NO_EVEN=1` leaves Grindor's compensated
clip scales as the data keys them (`Clip::even_out_compensated_scales`, a deviation), `TF_EVEN_LOG=1` counts
the spans evened at load; with `TF_DBG` a weapon model's object clip changes are printed.
`--fx-stats` also reports cumulative bullet visuals, bullet sprite submissions and
collision visuals for that capture, plus currently emitting gun-smoke/exhaust effects and
their remaining particles (including particles fading after the source stops).
`--no-speed-blur` disables the turbo screen effect for comparison, while keeping the
turbo's gameplay rules. `--speed-blur-report` logs effect weight changes and the three
GPU passes with their intermediate dimensions.

Examples that print the game's names by their hashes (`muzzle_peek`, `special_check`,
`form_fx_names`, `robot_material_audit`, `devastator_materials`) read an optional hash-to-name
table, one `hash<TAB>name` per line, from the file `TF2_NAMES` names; without it they print hashes.

## Layout

- `tf2-core` (`core\`) has no engine types: `formats\` reads the game's files,
  `sim.rs` is the movement and the car, `camera.rs` the two cameras, `sound.rs` the mixer,
  `weapons.rs` the weapon rules, `tuning.rs` the numbers. Each rule says whether it is from
  the game's code, its data, a recording, assumed, or a stand-in.
- `src\` here is the Bevy side: `player.rs` (input and the per-frame step), `camera.rs`,
  `animation.rs`, `sound.rs` (output device), `weapons.rs`, `hud.rs`, `world.rs`, `assets.rs`.
