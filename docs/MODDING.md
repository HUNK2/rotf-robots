# Porting the robots into another game

This is the one porting guide, for every host engine. It is written for the AI agent doing the
port (people can follow it too): a way of working, then the port as a list of tasks, each with
where its inputs are, what to build from the host's own parts, and how to check it. There are no
per-engine guides: the host's own documentation tells you how to build a thing there; this tells
you what to build and how to know it is right.

Read `AGENTS.md` at the repository root first: its rules hold for the port too.

## The rules of a port

- **No game files leave the user's machine.** The mod reads the player's install at run time, or
  converts it on the player's machine with `rotf-export`. Never ship, upload or commit the game's
  files or anything made from them. Players need their own copy of the PC game.
- **The reference is not part of the mod.** `core/` (crate `tf2-core`) is what the port is built
  from and checked against. Do not link it into the mod or wrap it: the robot is rebuilt from the
  host's own systems (user decision, see "Pick the kind of mod").
- **Numbers come from the files, never by eye.** Every speed, time, angle and share is in the
  export (`<pack>.json`, `<pack>_attributes.json`). If a number you need is not there, read the
  reference code for it; do not tune one until it feels right.
- **Keep the tags.** Where the reference is `[assumed]` or `[stand-in]`, the port inherits a guess,
  not the game's behaviour. Copy the tag into the port's own notes.
- **Write down every difference.** Where the host cannot do what the original does, the port map
  (below) says what the port does instead and why.

## Pick the kind of mod

1. **The robot's look, the host game's movement.** Model, animation and sound inside another game,
   moved by its own character or vehicle code. Only task 1 below applies. Simplest, least faithful.
2. **A native port: the robot as he plays in the original, rebuilt in the host game.** His
   movement, change of form, car, flight, fighting and weapons made from the host's character
   controller, animation graph, vehicle system, damage and scripting, so he works with its
   physics, AI, saves and multiplayer. The rest of this guide is about this kind.
3. **A change to the robots themselves.** Work in this repository, check with its tests and with
   the Bevy game ([rotf-robots](https://github.com/HUNK2/rotf-robots)), and send a pull request.

## How to work

1. **Survey the host before building.** Read its documentation and the project you are porting
   into, and write down: its update rate, its axes and units, and what it has for each of these
   parts: character controller, animation graph or state machine, vehicle, flying or free
   movement, physics queries (rays, sphere sweeps), damage, camera, audio, particles. Note what
   the host's own player character already does, since a port often replaces or extends it.
2. **Start the port map.** A file in the mod's repository (`PORT.md` is a good name) with one row
   per task below:

   | Task | Host part that carries it | State | Differs from the original | Checked by |
   |---|---|---|---|---|
   | 4. On foot and the jump | `CharacterMovementComponent`, custom jump | built | air control applied per tick at 60 Hz, converted | `basics.json` updates 0-170 |

   Fill a row before building that task, and update it after. It is how the user, and the next
   agent, know where the port stands.
3. **Export the robot** you are porting (`cargo run --release -p rotf-export -- --character
   <name>`; `--list` gives the names). Every task below reads these files.
4. **Do the tasks in order, one at a time.** Each later task leans on the earlier ones. For each:
   read its inputs, fill its row, build it, check it, then go on. Do not build several systems
   and check them together: a wrong jump makes every later run differ, and the cause is lost.
5. **When the inputs leave a question,** read the reference code named in the task. It is the
   exact answer for what the reference does. The windowless examples in `core/examples/` print
   what a system does for a scripted input: run one rather than guess.
6. **Stop and ask the user** when the host cannot do something the original does and the choice
   changes how the robot plays (for example the host's physics cannot hold the car's suspension),
   when a fix would mean editing the host engine itself, and before anything that touches the
   rules of the port above.

## Conventions at the edge

Convert once, where the port meets the export, and keep everything inside the port in the host's
own terms.

- **Space.** The JSON and the formulas are right-handed, x right, y forward, z up, metres. Yaw 0
  faces +y and a positive yaw turns left. Each `.glb`'s root node already turns that into glTF's
  y up, so the models import like any glTF; positions and directions from the JSON and from a
  run need converting. Most engines differ from both.
- **Time.** The game updates every 0.032 s; clips tick at 1/60 s. Many rules close a share `c` of a
  gap **per update**. A host stepping `dt` seconds uses `1 - (1 - c) ^ (dt / 0.032)`, which gives
  the same result per second ([MECHANICS.md](MECHANICS.md), "Before anything: the update").
- **Rotations.** The game stores turns for row-vector matrices; the `.glb` clips are written
  already conjugated for glTF. Conjugate only if you read the game's clips yourself
  (`core/src/formats/anim.rs`).
- **Names.** Names in the data are CRC-32 hashes of the name (case sensitive); asset ids are a
  different hash over the lower-cased name. `core/src/formats/hash.rs` has both.
- **Buttons.** The rules name the game's buttons (`Jump`, `AttackFast`, `VehicleMode`...: `control.
  buttons` in `<pack>.json`), not the pad's. The pad's default: right trigger the vehicle form,
  throttle and fire; left trigger weapon mode and drift; left bumper the brake; bottom face button
  jump and turbo; left face button melee (the car's gun in the car); right face button action and
  climb; top face button the special. One pad button can raise several game buttons. The PC's
  keyboard and mouse defaults are in [input.md](input.md).

## The tasks

Each task gives its **inputs** (the export, the formulas, the notes, the reference code), what to
**build**, and how to **check** it. "Run" means a reference run ("Checking a port", below).

### 1. The assets

- **Inputs:** the exported `.glb` files, textures and WAVs; `files`, `clips`, `render_sets`,
  `collision`, `materials` and `sounds` in `<pack>.json` ([FORMAT.md](FORMAT.md)); the Blender
  add-on ([`blender/`](../blender/README.md)) if the host imports through Blender.
- **Build:** import the robot, the vehicle, the weapons and their clips. Keep each clip's own
  timing and its events (footsteps, hit windows, effects, weapon attach). Set up the render sets
  the game switches, and the hit boxes and attack bodies from `collision`.
- **Check:** he stands, plays every clip at its length, and his change of form plays, in the
  host's own viewer. The events land on the ticks `clips[].events` gives.

### 2. Units, axes and the update

- **Inputs:** `space` in `<pack>.json`; "Conventions at the edge" above.
- **Build:** one place in the port that converts positions, rotations and times, and the per-update
  share conversion.
- **Check:** a robot placed at the export's `(0, 10, 0)` facing yaw 0 stands 10 m ahead of the
  origin in the host, facing the host's forward.

### 3. The control states

- **Inputs:** `control` and `animation_rules` in `<pack>.json`; [state-machine.md](state-machine.md),
  [animation-rules.md](animation-rules.md); reference `control.rs`, `animrules.rs`, `animblend.rs`.
- **Build:** the host's animation graph or state machine from `control`: every state, its clip and
  motion mode, and every way out with its button, stick or condition and its priority. Inside
  each state, the clip rules from `animation_rules` (walk to run, landings, fidgets, which dodge
  plays) with their blends.
- **Check:** a run's `state`, `overlay` and `clip` (set, id, tick) match update by update.

### 4. On foot and the jump

- **Inputs:** [MECHANICS.md](MECHANICS.md) "On foot", "The jump"; `mechanics.on_foot`,
  `mechanics.jump`, `mechanics.constants`; [bumblebee-mechanics.md](bumblebee-mechanics.md);
  reference `sim.rs`.
- **Build:** the host's character controller following the formulas: the turn, the paces and their
  blends, which jump is picked, the launch, the air control, the landing.
- **Check:** `export/runs/basics.json`, the running and jumping part. For Bumblebee the run settles
  at 24 m/s and the running jump leaves at `vz` 25.338 (MECHANICS.md, "Checking").

### 5. The dodge and root motion

- **Inputs:** MECHANICS.md "The dodge", "When a clip moves him"; `mechanics.dodge`; reference
  `sim.rs`.
- **Build:** the dodge, and the rule for when a clip's root motion moves him instead of the
  controller.
- **Check:** a run with the dodge pressed while moving: `pos` and `mode` through the dodge.

### 6. The change of form and the car

- **Inputs:** MECHANICS.md "The car"; `mechanics.car`, `vehicle` in `<pack>.json`;
  [driving.md](driving.md), [physics.md](physics.md); reference `driving.rs`, `vehicle.rs`.
- **Build:** getting in and out (which jump comes out, when the unfold runs), then the drive rules
  (speed caps, turbo, brake, reverse, slide, steering). The rules are written apart from the wheels
  and springs so the host can keep the rules on its own vehicle physics: say in the port map which
  you did.
- **Check:** `basics.json`, the driving part. Bumblebee's speed along the ground settles near 37.5
  in the car (MECHANICS.md, "Checking", says why it is over the cap).

### 7. Flight and hover (the six flyers)

- **Inputs:** MECHANICS.md "Flight", "Hover"; `mechanics.fly`, `mechanics.hover`; `flight-*.md`;
  reference `flight.rs`, `flybound.rs`.
- **Build:** the jets', the helicopter's and the robot flyers' steering (the helicopter and the
  robot flyers lift and turn on the second stick), getting in and out, and the zone's flight
  boundary.
- **Check:** a run with `vehicle` held and the stick back, then centred. Starscream flying level
  with the stick centred holds exactly 45 m/s.

### 8. Climbing

- **Inputs:** MECHANICS.md "Climbing"; `mechanics.climb`; [climbing.md](climbing.md); reference
  `climb.rs`.
- **Build:** what counts as a wall, catching hold, the clips that move him on it, and the ways off.
  This needs the host's physics queries against its own level geometry.
- **Check:** a run with an `arena` box to climb (`arena.boxes`), holding `Climb` and the stick up.

### 9. Melee

- **Inputs:** MECHANICS.md "Melee"; `mechanics.melee`, `clips[].attack`; [melee.md](melee.md);
  reference `melee.rs`, `combat.rs`.
- **Build:** the chain and its button windows, the target search, the turn and slide toward the
  target, the hit, and the ground punch's blast, through the host's damage system.
- **Check:** `basics.json` has the three-hit combo; add `arena.targets` and compare `hits` (target,
  damage, knock-back) as well as `clip`.

### 10. Damage and health

- **Inputs:** MECHANICS.md "Damage and health"; `mechanics.damage`; [damage.md](damage.md);
  reference `damage.rs`.
- **Build:** taking a hit, the throw, the statuses, the reaction clips and regeneration, on the
  host's health and damage events.
- **Check:** `export/runs/fight.json`: a scout shoots and strikes him. Compare `taken` (damage and
  flags), `health`, and the `state` and `clip` of each reaction.

### 11. Weapons and aim

- **Inputs:** MECHANICS.md "Weapons"; `mechanics.weapons`, `weapons` and `weapons[].numbers` in
  `<pack>.json`; [weapon-mode.md](weapon-mode.md), [weapon-kinds.md](weapon-kinds.md),
  `weapon-*.md`; reference `arsenal.rs` (the weapon manager, which holds the rest together),
  `weapons.rs`, `gun.rs`, `aim.rs`, `lockon.rs`, and `driver.rs` for the order of an update.
- **Build:** weapon mode's movement and aim, firing and heat, each gun's round, missiles and the
  lock-on, grenades, the car's gun. Rounds leave from the weapon model's barrel nodes
  (`weapons[].barrel_nodes`) and fly along the camera's line. What a round shows, sounds and lights
  is `weapons[].round`, `alt_round` and `muzzle_light`.
- **Check:** `export/runs/weapons.json` and `fight.json`: compare `aim` and each line's `weapon`
  (`in_hand`, `heat`, `fired`, `lock`, `projectiles`, `struck`, `sounds`).

### 12. The special

- **Inputs:** MECHANICS.md "The special"; `mechanics.special`; [character-specials.md](character-specials.md),
  `special-*.md`; reference `special_weapon.rs`, `turret.rs`, `repair.rs`.
- **Build:** its clocks, what its trigger sets off, and the robot's own: Optimus's shield, the
  weapon specials, the deployed turrets (`turret_file`, `turret_missile_file`), CombatRepair's heal.
- **Check:** `fight.json` ends on `AttackSpecial`: `clip`, `triggers` and timing, and for the
  weapon specials and turrets the `weapon` block's `fired`, `projectiles` and `struck`. Devastator's
  pods and vortex are in `<pack>.json`'s `giant` and fire in his runs.

### 13. The cameras

- **Inputs:** MECHANICS.md "The cameras"; `mechanics.cameras`; [camera.md](camera.md),
  `camera-*.md`; reference `camera.rs`, `camera/`, `shake.rs`, `rumble.rs`.
- **Build:** the follow, driving and weapon cameras step by step on the host's camera, the shakes
  (a run's `shakes`) and the pad rumble (`rumbles`). A flyer uses the driving camera with its
  flight flag, on its own camera settings.
- **Check:** every run's `camera` (`eye`, `forward`, `fov_degrees`, `roll_degrees`): on foot in
  `basics.json`, weapon mode and the car in `weapons.json`; a flyer's in its own runs.

### 14. Sound

- **Inputs:** MECHANICS.md "Sound"; `mechanics.sound`, `sounds`, `character_sounds`, `sounds.voices`;
  [sound.md](sound.md); reference `sound.rs`, `chatter.rs`.
- **Build:** each sound event as the game plays it (its samples, volume, pitch and distance rules)
  on the host's audio, what plays when, the engine's gearbox sound and the battle chatter.
- **Check:** the clip events from task 1 start the right events; the weapons' sound calls are each
  run line's `weapon.sounds`; listen against the Bevy game.

### 15. Effects

- **Inputs:** MECHANICS.md "Effects"; `effects` in `<pack>.json`; [particles.md](particles.md);
  reference `particles.rs`, `ribbon.rs`, `skidmark.rs`, `fxstate.rs`.
- **Build:** what starts each effect (a run's `triggers` and `untriggers` name them), the particle
  update, its sources, domains and actions, how a particle is drawn, skid marks and ribbons, the
  car's knocks (`collision_effects`, `collision_sounds`), the weapons' rounds (muzzle flashes,
  trails, impacts, blasts, lights, debris: `weapons[].round`) and the turbo's screen blur
  (`turbo_blur`), on the host's particle system and lights.
- **Check:** the effects start and stop on the updates a run's `triggers` and `untriggers` give;
  look against the Bevy game.

## Checking a port

- **Reference runs.** `cargo run --release -p rotf-export -- --character <name> --run
  <script.json>` plays a script of pad inputs through the reference, the whole update as the Bevy
  game runs it, and writes `<pack>_run.jsonl`: one line per 0.032 s update with his place,
  velocity, facing, motion mode, control state, clip and tick, the camera, the aim, his weapons
  (heat, rounds, lock-on, projectiles, hits, sound calls), what hit him, the scouts, and the
  triggers, shakes, rumbles and melee hits of that update ([FORMAT.md](FORMAT.md), "Reference
  runs"). Feed the same inputs to the port, log the same values, and compare update by
  update. A host at another rate compares at the same times. The first update that differs is where
  to look; later differences usually follow from it.
- **Write a script per task.** `export/runs/basics.json` runs, jumps, does the three-hit combo,
  drives and changes back; `weapons.json` fires both weapons and the car's gun at scouts that hold
  their fire; `fight.json` fights a scout that fights back. A script for one task holds only that
  task's inputs, for example:

  ```json
  { "format_version": 2,
    "arena": { "targets": [ { "pos": [0, 4, 0] } ] },
    "steps": [ { "updates": 10 },
               { "press": ["AttackFast", "Dodge", "AttackVehicle"] },
               { "updates": 40 } ] }
  ```

- **What runs do not cover:** levels (the ground is flat, with boxes), sniping, and camera shake on
  the view. A run's muzzle can sit a few centimetres from the Bevy game's (FORMAT.md says why).
- **The Bevy game** ([rotf-robots](https://github.com/HUNK2/rotf-robots)) is the reference you can
  play: `cargo run --release -p bumblebee -- --character <name>`. Its `--shot out.png --after 2`
  captures one frame without playing; its `game/README.md` lists the other capture flags.

## How sure the reference is

The reference for a port is the Bevy game and the core it runs: match them. How close they are to
the original game is a separate question. Bumblebee's movement, car, cameras, climbing and strafing were compared with recordings of the
original game. Everything else, and every other robot, is checked only by tests and captures. The
tags in the formulas and the code say which parts are read from the game (`[game]`, `[data]`),
measured on it (`[trace]`), guessed (`[assumed]`) or placeholders (`[stand-in]`). What the export
leaves out is in [FORMAT.md](FORMAT.md), "Not in the export", and in each `<pack>.json`'s
`not_exported`.

## The robots

13 distinct robots (7 that drive, 6 that fly) and 5 reskins. `cargo run --release -p rotf-export
-- --list` prints them with the names `--character` takes. Port Bumblebee first if the choice is
yours: he is the most complete and the only one compared with recordings.
