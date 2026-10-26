# Guide for AI coding agents

Read this before changing anything. It is written for agents (Codex, Claude Code and others)
helping a modder bring the robots of *Transformers: Revenge of the Fallen* (PC, 2009) into another
game, working in this repository or with its output. People can read it too.

For the step-by-step port, read `docs/MODDING.md` after this file.

## The rules (not negotiable)

- **Never add game files to the repository.** No models, textures, animation, sound, packs,
  executables, or files converted from them (that includes everything `rotf-export` writes). Not
  in commits, not in tests, not as "small samples". The game's data is read from the user's own
  install, and that is the only way it gets in.
- **Never write to the install.** Everything under it is read only.
- **Exported files stay on the user's machine.** Do not upload them, put them in a mod's
  download, or commit them anywhere. A shared mod tells its users to run `rotf-export`
  themselves.
- **Do not decompile, copy or paste the game's code.** Behaviour is reimplemented in Rust from
  notes. The addresses in comments (`00867f20`) say where a fact was read; they are not code to
  bring in.
- **Keep the provenance tags honest** (below). Never remove an `[assumed]` or `[stand-in]` tag
  unless you replaced the guess with the game's own behaviour and can say how you know.
- The user must own the PC game. Do not help anyone get it some other way, and do not add
  support for console disc images.

## Build, test, check

From the repository root (Rust stable, Windows is the tested platform). No engine is built here,
so a build takes about a minute.

```
cargo run --release -p rotf-export -- --list                  # the roster, as --character takes it
cargo run --release -p rotf-export -- --character bumblebee   # one robot into exported/bumblebee
cargo run --release -p rotf-export -- --all                   # every robot into exported/
cargo test --workspace                                        # the core's and the exporter's tests
cargo clippy --workspace --all-targets -- -D warnings         # must stay clean
```

- Many tests read the install. Without it they print `skipped: the game's install was not
  found` and pass. A green run on a machine without the game proves little: say so when you
  report.
- `cargo run -p tf2-core --example install_where` prints where the install was found.
- The examples in `core/examples/` are windowless checks (`control_check`, `vehicle_check`,
  `combat_check`, `fly_check`, `melee_check`, `weapons_check`, `camera_replay`...). Each says
  what it does in its first lines. Run one instead of guessing what a system does.

## How the repository is laid out

- `export/` (crate `rotf-export`): the exporter. `src/model.rs` writes the glTF files,
  `src/describe.rs` the JSON, `src/sounds.rs` the sound events and WAVs, `src/lib.rs` ties a
  robot's pieces together. Its tests (`export/tests/`) read every written file back.
- `blender/`: the Blender add-on (`rotf_import/`, one Python file) and `check_import.py`, which
  checks an import against the JSON without a window (`blender/README.md` says how to run it).
  A change to the JSON that the add-on reads is checked with it too.
- `core/` (crate `tf2-core`): **the file readers and all the robot logic, with no engine types**
  (only `glam` for maths). File-format readers in `core/src/formats/`.
- `core/fixtures/`: physics values recorded from the running game, replayed by tests.
- `docs/`: the research notes, cited from the code as `docs/<name>.md`. `docs/README.md`
  explains how to read them.

The Bevy game that plays the robots is a separate repository,
[rotf-robots](https://github.com/HUNK2/rotf-robots). It depends on this one's `tf2-core` at a
pinned commit and is the only complete example of driving the core. Paths written `game/...`
below are in that repository.

## The exported files

Each robot's folder holds a glb per piece (`<pack>_robot.glb`, `_vehicle.glb`, weapons,
projectiles), `textures/`, `sounds/<bank>/*.wav`, `<pack>.json`, `<pack>_attributes.json` and a
`README.txt`. The JSON's format is `docs/FORMAT.md`, versioned by `format_version`
(`rotf_export::FORMAT_VERSION`). A change to the JSON changes that document in the same commit:
the `json` test fails when the JSON has a key or event type the document does not name. Bump the
version when a key is removed or renamed or changes its type, unit or meaning, and add a line to
the document's "Changes". Points that trip importers:

- Clips are at 60 ticks per second; event `tick`s count in those ticks.
- A mesh node with `extras.render_set_hidden` starts hidden in the game (it also carries
  `KHR_node_visibility`, which not every importer reads). `render_sets` in the JSON lists the
  groups.
- Collision bodies are empty glTF nodes `<node>.collision.<n>` on the node they ride (not its
  `.frame`), the shape in their `rotf_collision` extras; the JSON's `collision` lists them. Their
  `scaled` sizes are the port's reading of the stored scale, not read in the game.
- A vehicle clip lists `unkeyed_nodes`: nodes it has no channel for. The game holds them where
  the previous clip left them; glTF puts them back to rest.
- A clip's sound events name events in the JSON's `sounds` section, which lists each event's
  layers, sounds and the WAVs they play.

## Where each mechanic lives in `core/src/`

| Mechanic | Files |
|---|---|
| The per-frame step, on foot and in the car | `sim.rs` (`State`, `Input`, `step`, the `World` trait) |
| Character data and tuning | `character.rs` (the robot's pack), `tuning.rs` (attributes, speeds, rules) |
| Control state machine | `control.rs` |
| The car | `driving.rs`, `vehicle.rs`, `vehicle_clip.rs`, `terrain.rs` |
| Flight | `flight.rs`, `flightsurface.rs`, `flybound.rs`, `flycam.rs` |
| Climbing | `climb.rs` |
| Animation | `animrules.rs` (which clip), `animblend.rs`, `pose.rs`, `footik.rs` |
| Melee, damage, health | `melee.rs`, `combat.rs`, `damage.rs`, `repair.rs` |
| Weapons and aiming | `weapons.rs`, `gun.rs`, `aim.rs`, `lockon.rs`, `reticle.rs`, `snipe.rs` |
| Specials | `special_weapon.rs`, `special_types.rs`, `turret.rs`, `energon_wave.rs`, `giant.rs` (Devastator) |
| Cameras and feel | `camera.rs`, `camera/`, `shake.rs`, `rumble.rs`, `speedblur.rs` |
| Effects | `particles.rs`, `ribbon.rs`, `fxstate.rs`, `stepfx.rs`, `collisionfx.rs`, `skidmark.rs`, `ssd.rs` |
| Sound | `sound.rs`, `audio_mix.rs`, `chatter.rs`, `reverb.rs` |
| Physics and collision | `physics_*.rs`, `gjk*.rs`, `mpr.rs`, `pair_sweep.rs` |
| Finding the install | `install.rs` |

**Support code, not a base to build on:** the enemy AI (`ai_*`, `navmesh`, `navpath`) and the
level code (`level_*`, `collision_world`, `collision_soup`, `mission_script`). They exist so the
robots have something to fight and somewhere to stand in the Bevy game, and may change or go.

## Running the reference

The core is the reference that native ports are built from and checked against
(`docs/MODDING.md`), not a library to ship inside a mod: this repository makes porting tools, not
a runtime that drives the robot inside another game. To run it, for example to print what a
system does for a scripted input:

1. `tuning::load(install, "Bumblebee")` reads the character's attributes and rules.
2. `character::load(install, "bumblebee")` reads its pack: models, animations, weapons.
3. The tuning is completed from the animations (`Locomotion::from_animations`,
   `Actions::from_animations`, `RuleSet::from_animations`). `core/examples/control_check.rs` does
   the short version, rotf-robots' `game/src/charmenu.rs` (`load`) the full one.
4. `sim::State::new(&tuning)`, then every frame `sim::step(&mut state, &input, &tuning, &world, dt)`.
   `Input` is the pad, already mapped (stick in world space, triggers 0 to 1, buttons).
   `world` is anything that implements `sim::World`: it answers floor heights, wall pushes and
   ray hits. `sim::Arena` is a flat test world.
5. The rest of a frame (the camera, the weapons, animation, effects and sound reading the state)
   is in rotf-robots' `game/src/player.rs` (`tick`), `animation.rs`, `camera.rs`, `weapons.rs`,
   `effects.rs` and `sound.rs`. The order they run in is part of the rules a port rebuilds.

The game steps at 0.032 s; animation clips tick at 1/60 s. Space is right-handed, x right,
y forward, z up, metres. Quaternions in clips are for row-vector matrices: conjugate them for
glam and most engines (the exported glTF is already converted). Names in the data are CRC-32
hashes; `formats::hash` computes them.

## Provenance tags

Every claim in the code says where it came from:

- `[game]`: read in the executable (an address follows).
- `[data]`: read from the game's files.
- `[trace]`: measured in a recording of the running game.
- `[assumed]`: a guess. The comment says what would settle it.
- `[stand-in]`: a placeholder still to replace.
- `[port]`: a choice of this project with no counterpart in the game.

When you add code, tag it the same way. When you report to the user, say which parts of your
work rest on `[assumed]` or `[stand-in]` code.

## Working well here

- Read the note before the code: `grep -rl <topic> docs/` and read the section you need. The
  notes are long; read by heading, not whole files.
- `core/src/sim.rs` is very large. Search it for the function you need instead of reading it
  through.
- Check an exporter change with `cargo test -p rotf-export` and by opening the glb in Blender.
- Keep `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `docs/` is copied from the author's own notes. Edits are fine in a fork. In a pull request
  back here, explain the change in the description, because the folder is regenerated.
- Comments say what the game does and why, with the tag. Do not leave comments about your own
  process.
