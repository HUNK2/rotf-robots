# Porting the robots into another game

This guide is for modders, and for the AI agents most of them will use, who want to bring the
robots of *Transformers: Revenge of the Fallen* into another game. Agents: read `AGENTS.md` at the
repository root first; it has the rules, the commands and the map of the code.

## What you can take, and what you cannot

The repository holds only original source code. The game's models, textures, animation and sound
are read from the user's own install of the PC game every time the program runs. A port works the
same way:

- Your mod reads the player's install at run time, or converts it on the player's own machine.
- You never ship, upload or share files from the game, or files made from them.
- Players need their own copy of the PC game.

## Pick the kind of mod first

**1. The robot's look, the host game's movement.** The robot's model, animation and sound inside
another game, which moves it with its own character or vehicle code. Run `rotf-export` (see the
README): it writes the robot as glTF, PNG, DDS and WAV with a JSON file of its animation events,
change of form, weapons and sounds, which any engine or Blender can import. The JSON's format is
[FORMAT.md](FORMAT.md): write your importer against it and check `format_version`. In Blender, the
add-on in [`blender/`](../blender/README.md) imports a robot with all of that applied. The asset readers it
uses are in `core/src/formats/` (`pack`, `model`, `texture`, `anim`, `scene`, `fsb`, `fev`) if you
would rather read the install yourself. Simplest, least faithful.

**2. A native port: the robot as he plays in the original, rebuilt in the host game.** His
movement, change of form, car, flight, fighting and weapons, made from the host's own parts: its
character controller, animation graph, vehicle system, damage and scripting. He then works with
the host's physics, AI, saves and multiplayer like anything else in that game. This repository's
code does not go into the mod: it is the reference the port is built from and checked against.
The rest of this guide is about this kind.

**3. A change to the robots here.** New behaviour, a fix, another character: work in this
repository, check with its tests and with the Bevy game
([rotf-robots](https://github.com/HUNK2/rotf-robots)), then send a pull request.

## What a native port is built from

**The assets.** `rotf-export` and, in Blender, the add-on: the model, every clip at its own timing,
the change of form, the weapons and where they hang, the hit boxes, the sounds, and the events
that say when a footstep, a hit window or an effect happens.

**The robot's numbers.** `<pack>_attributes.json` from the exporter: speeds, jumps, turning,
health, the car's numbers. Take them from the file, never by eye.

**The control states.** `control` in `<pack>.json`: every state the robot can be in, the clip it
plays and its motion mode, and every way out of it with the button, stick or condition that takes
it and its priority, with the rules for running them ([FORMAT.md](FORMAT.md), `control`). This is
the skeleton of the host's animation graph or state machine.

**The clip rules.** `animation_rules` in `<pack>.json`: inside each animation set, the clip it
starts on and when each clip hands over to which, by the stick, the fall, the weapon's state and
the other variables the game reads, with the blends. These are the transitions inside each state
of the host's graph (walk to run, the landings, the fidgets, which dodge plays).

**The formulas.** [MECHANICS.md](MECHANICS.md) gives what the reference does each update as
formulas, and `mechanics` in `<pack>.json` gives each robot's numbers for them. So far on foot,
the jump and the car: the turn, the paces and their blends, which jump is picked, the launch, the
air control, the landing; the car's speed caps, turbo, brake, reverse, slide and steering, and its
wheels and springs, split so a host can keep the rules and use its own vehicle physics; the jets',
helicopters' and robot flyers' flight, and the hover.

**The rules, written down.** Each system's notes in `docs/`, read from the game's code and data,
say what it does, in what order, with which numbers, and tag what is fitted or guessed:

| System | Notes | Reference code (`core/src/`) |
|---|---|---|
| The control states and what moves between them | `state-machine.md`, `animation-rules.md` | `control.rs`, `animrules.rs`, `animblend.rs` |
| On foot: walking, running, jumps, the dodge | `bumblebee-mechanics.md` | `sim.rs` |
| The car | `driving.md`, `physics.md` | `driving.rs`, `vehicle.rs` |
| Flight and hover | `flight-*.md` | `flight.rs`, `flybound.rs` |
| Climbing | `climbing.md` | `climb.rs` |
| Melee | `melee.md` | `melee.rs`, `combat.rs` |
| Weapon mode and the weapons | `weapon-mode.md`, `weapon-kinds.md`, `weapon-*.md` | `weapons.rs`, `gun.rs` |
| Specials | `character-specials.md`, `special-*.md` | `special_weapon.rs`, `repair.rs` |
| Damage and health | `damage.md` | `damage.rs` |
| Cameras | `camera*.md` | `camera.rs`, `camera/` |
| Feet on the ground | `foot-ik.md` | `footik.rs` |
| Sound and effects | `sound.md`, `particles.md` | `sound.rs`, `particles.rs` |
| The controls | `input.md` | |

**The reference code.** `core/` (crate `tf2-core`) is the same rules as running, tested Rust with
no engine in it. Read it where a note leaves a question; the code is the exact answer. Do not link
it into the mod.

## A port, step by step

1. **Play the Bevy game first** ([rotf-robots](https://github.com/HUNK2/rotf-robots),
   `cargo run --release -p bumblebee`). Know what the robot should feel like before rebuilding him.
2. **Bring in the assets.** Export the robot and import him into the host's tools, through Blender
   if that is the host's way in. He should stand, play his clips and change form there first.
3. **Map each system onto the host's.** The control states (`control`) become the host's
   animation graph or state machine; on foot becomes its character controller; the car its vehicle setup; weapons and
   damage its own. Write the map down before building: which of the host's parts carries which
   rule, and what the host cannot do the same way.
4. **Convert at the edge.** Axes, units, the game's 0.032 s update and the clips' 1/60 s tick (see
   below). A host that updates at another rate has to give the same result per second, not per
   update.
5. **Build one system at a time and check each** against the reference (next section) before the
   next: on foot, then the change of form, the car, the camera, fighting, weapons, effects, sound.
6. **Say what differs.** Where the host's physics or animation cannot do what the original does,
   write down how the port differs. Systems built on `[assumed]` or `[stand-in]` code (see
   rotf-robots' `game/README.md`, "What is real") are the reference's guesses, not proven.

## Checking a port

- **Reference runs.** `rotf-export --character <name> --run <script.json>` plays a script of
  inputs through the reference and writes, for every 0.032 s update, his place, velocity,
  facing, motion mode, control state, clip and tick, and the triggers, shakes and hits his clips
  set off ([FORMAT.md](FORMAT.md), "Reference runs"). Play the same script in the port and compare
  line by line. `export/runs/basics.json` runs, jumps, does the three-hit combo, drives and
  changes back.
- The Bevy game is the reference you can play. Run the same input in both and compare.
- The windowless examples in `core/examples/` print what a system does for a scripted input
  (`control_check`, `vehicle_check`, `fly_check`, `combat_check`, `melee_check`...). Run the same
  script in your port and compare the numbers.
- In rotf-robots, `cargo run --release -p bumblebee -- --shot out.png --after 2` captures one frame
  without playing. Other capture flags are in its `game/README.md`, "Checking it without playing".
- Recorded physics from the running game are in `core/fixtures/`. The car's tests replay them.

## Facts every port needs

- Space: right-handed, x right, y forward, z up, metres. Most engines differ: convert at the edge.
- The game updates every 0.032 s. Animation clips tick at 1/60 s.
- Quaternions in the clips are for row-vector matrices: conjugate them for glam and most engines.
- Names in the data are CRC-32 hashes of the name (case sensitive); asset ids are a different hash
  over the lower-cased name. `core/src/formats/hash.rs` has both.
- The pad: right trigger is the vehicle form, throttle and fire; left trigger weapon mode and drift;
  left bumper the brake; the bottom face button jump and turbo; the left face button melee (the car's
  gun in the car); the right face button action and climb; the top face button the special.
  `docs/input.md` has the PC keyboard and mouse defaults.

## The robots

13 distinct robots (7 that drive, 6 that fly) and 5 reskins. `cargo run -p tf2-core --example
list_roster` prints them with the names `--character` takes. Bumblebee is the most complete and the
only one compared with recordings; the others play on their own data and are checked only by tests
and captures.

## Coming: porting tools

- **The rest of the formulas.** On foot, the jump, the car, flight and hover are in MECHANICS.md
  and `mechanics`. Still to come: the flight boundary, climbing, the dodge, melee, the weapons and
  damage.
- **More in the reference runs.** Runs cover movement, the change of form, the car, flight, melee
  and the clips. Still to come: the guns and their aim, the cameras, and targets that fight back.
- **Guides per host engine.** How each part of a robot maps onto one engine's own systems, written
  against that engine's real tools and checked there.
