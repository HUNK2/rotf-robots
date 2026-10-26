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
change of form, weapons and sounds, which any engine or Blender can import. The asset readers it
uses are in `core/src/formats/` (`pack`, `model`, `texture`, `anim`, `scene`, `fsb`, `fev`) if you
would rather read the install yourself. Simplest, least faithful.

**2. The robot as it plays in the original.** The movement, the change of form, the car, flight,
fighting and weapons, run by this repository's code inside the host game. This is what the code is
for. The rest of this guide is about it.

**3. A change to the robots here.** New behaviour, a fix, another character: work in this
repository, check with its tests and with the Bevy game
([rotf-robots](https://github.com/HUNK2/rotf-robots)), then send a pull request.

## The parts of a port

Whatever the engine, a port of kind 2 has the same five pieces.

**The core.** `core/` (crate `tf2-core`) has no engine types. A Rust host links it directly. A host
in another language needs a thin wrapper around it (a C-compatible library is planned).

**Loading a robot.** The tuning (`tuning::load`) and the pack (`character::load`) are read from the
install, then the tuning is completed from the animations. `core/examples/control_check.rs` shows
the short form; rotf-robots' `game/src/charmenu.rs` (`load`) the full one, with the specials and Devastator.

**The world.** Implement `sim::World` over the host's collision. Only two methods are required:

- `floor(x, y, z)`: the height to stand on at a spot, for feet at height `z`.
- `push_out(pos, radius, height)`: move an upright cylinder out of what it cannot step onto.

Everything else has a default that falls back to these. Add `wheel_ray` and `vehicle_sweep` for the
car, and the rest as the port gets more faithful. Each method says what it is for in `core/src/sim.rs`.

**The frame.** Every frame: turn the player's controls into `sim::Input`, call `sim::step`, then run
the camera, the weapons, the animation, the effects and the sound from the new state. The Bevy game
([rotf-robots](https://github.com/HUNK2/rotf-robots)) does this in `game/src/player.rs` (`tick`) and the files it hands over to (`animation.rs`,
`camera.rs`, `weapons.rs`, `effects.rs`, `sound.rs`). The order matters: copy it.

**Drawing and playing.** The host draws the robot's pose and plays its sounds and effects. The Bevy game
shows how the pose is built from the clips (`game/src/animation.rs`, over `core/src/animblend.rs`,
`pose.rs` and `footik.rs`) and how sound events become sound (`game/src/sound.rs`, over
`core/src/sound.rs` and `audio_mix.rs`).

## A port, step by step

1. **Build and play the Bevy game first** ([rotf-robots](https://github.com/HUNK2/rotf-robots),
   `cargo run --release -p bumblebee`). Know what the robot should feel like before moving it.
2. **Load the robot headless in your host.** Print its states and clips, as `control_check` does.
3. **Stand it on a flat world.** Implement `floor` and `push_out` as a flat plane and step the state
   with no input. It should stand still, on the ground.
4. **Drive it.** Map the host's controls into `sim::Input` and walk, jump and dodge. Compare with
   the Bevy game side by side.
5. **Show it.** Pose the model from the state and play the clip events.
6. **Add the rest a system at a time:** the car, the change of form, the camera, weapons, effects,
   sound. After each, check against the Bevy game.
7. **Report what is uncertain.** Systems built on `[assumed]` or `[stand-in]` code (see
   rotf-robots' `game/README.md`, "What is real") behave as this port guesses, not as proven.

## Checking a port

- The Bevy game is the reference. Run the same input in both and compare.
- In rotf-robots, `cargo run --release -p bumblebee -- --shot out.png --after 2` captures one frame
  without playing. Other capture flags are in its `game/README.md`, "Checking it without playing".
- The windowless examples in `core/examples/` print what a system does for a scripted input
  (`control_check`, `vehicle_check`, `fly_check`, `combat_check`, `melee_check`...). Run the same
  script through your port and compare the output.
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

## Coming

- A small robot API: load a robot, step it, read back its pose, state and events, instead of
  copying the game's per-frame code.
- A C-compatible library over it, for hosts not written in Rust, with a worked example.
- A written spec of the exporter's JSON, with a format version.
- A Blender add-on that imports an exported robot ready to animate.
- Conformance checks: recorded input replayed through a port and compared with the expected output.
