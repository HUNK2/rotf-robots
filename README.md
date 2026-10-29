# rotf-robots: the robots of Transformers: Revenge of the Fallen

The playable robots of *Transformers: Revenge of the Fallen* (PC, 2009) rebuilt in Rust: their
movement, forms and changes of form, fighting, weapons, specials, cameras, animation, damage,
effects and sound, read from the game's own data, its code and recordings of it. One repository
holds both halves:

- **The game** (`game/`): a Bevy program that plays the robots, in an authored test arena or
  standing on one of the game's levels.
- **The modding kit**, for bringing the robots into another game (Unreal, Unity, Godot, Blender,
  or a host game), with no engine in it:
  - **`rotf-export`**: writes a robot from your own install as standard files: glTF models with
    their skeletons and animations, PNG and DDS textures, WAV sounds, and a JSON file with
    everything a model cannot hold (animation events, the change of form, weapons and where they
    mount, meshes that start hidden, hit boxes, sound events, the robot's attributes). The JSON's
    format is written down and versioned in [`docs/FORMAT.md`](docs/FORMAT.md).
  - **`blender/`**: a Blender add-on that imports an exported robot ready to animate: 60 fps, the
    hidden meshes hidden, events as markers, the vehicle's clips holding their pose, weapons on
    their nodes, collision shapes, and a panel that plays a change of form
    ([`blender/README.md`](blender/README.md)).
  - **`tf2-core`**: the game's file-format readers and the robots' mechanics (movement, the
    change of form, the car, flight, climbing, fighting, weapons, damage, cameras), as a Rust
    library with no engine types. The Bevy game is built on it, and it is the reference a port
    is checked against.
  - **`docs/`**: the research notes behind all of it, and `docs/MODDING.md`, the porting guide:
    one for every host engine, written as tasks for the AI agent doing the port.

This is not the whole game. There are no missions, menus, saves, cutscenes or traffic. The enemy
AI exists only so the robots have something to fight.

## You need the game

Nothing from the game is in this repository, and nothing is downloaded. Everything is read from
**your own install of the PC game**, which is never written to. Without it nothing runs.

The install can be anywhere. A normal install is found by itself, through the entry its
installer leaves in the Windows registry. If yours is not found (a copied folder, or another
system), pass `--game-dir` with the folder that holds `bnxglobal.str` (or the folder just above
it). The game also asks for the folder the first time it starts and saves the answer (in
`%APPDATA%\rotf-robots\game-dir.txt`). The order it is looked for in: `--game-dir`, the
`TF2_GAME_DIR` environment variable, the saved folder, the registry, then the program's own
folder and the working folder.

## Download

The [Releases page](https://github.com/HUNK2/rotf-robots/releases) has a Windows build: unzip it and
run `bumblebee.exe` to play, or `rotf-export.exe` to export a robot (its `README.txt` has the keys
and options). The Blender add-on is there too, as `rotf_import-<version>.zip`.

## Playing

To build it yourself instead, install Rust (stable) and run, from this folder:

```
cargo run --release -p bumblebee
cargo run --release -p bumblebee -- --character optimus
cargo run --release -p bumblebee -- --game-dir "D:\Games\Transformers Revenge of the Fallen"
```

The first build compiles Bevy and takes a while. In the game, F2 opens a robot menu and F6 / F7
step through the roster. F1 shows the controls. The keyboard and mouse use a modern layout
(WASD, the mouse turns the camera, right mouse for weapon mode); `--classic-controls` gives the
original's own bindings. A pad always has the game's bindings. `--mission <Name>` loads one of
the game's levels as a stage to stand on.

`game/README.md` has the controls, every command-line flag, the capture flags used for checking
without playing, and what is real and what is a stand-in.

The roster: 13 distinct robots, 7 that drive and 6 that fly: Bumblebee, Optimus Prime, Ratchet,
Ironhide, Megatron, Long Haul, Sideways, Jetfire, Breakaway, Starscream, Grindor, Powered-Up
Optimus and Megatron's flying form, plus 5 reskins. Bumblebee is the most complete. The others
play on their own attributes, health, speeds, weapons, specials and animation sets.

## Exporting a robot

```
cargo run --release -p rotf-export -- --list
cargo run --release -p rotf-export -- --character bumblebee
cargo run --release -p rotf-export -- --all
```

`--character` writes one robot into `exported/<pack>`; `--all` writes the whole roster (20 packs,
about 2 GB). `--out <folder>` picks another folder. Each robot's folder has a `README.txt`
describing its files.

To check a native port against the reference, `--character <name> --run <script.json>` plays a
script of inputs through it and writes every update's place, state, clip and events as JSON lines
(`docs/FORMAT.md`, "Reference runs"; the example scripts are `export/runs/basics.json`,
`weapons.json` and `fight.json`).

The exported files are made from the game and belong to its owners. Keep them on your own
machine. A mod you share should tell its users to run `rotf-export` themselves.

## The core: the reference a port is built from

```toml
[dependencies]
tf2-core = { git = "https://github.com/HUNK2/rotf-robots", rev = "<commit>" }
```

The core is not meant to ship inside a mod: a native port rebuilds the robot from the host game's
own parts, and the core is what it is built from and checked against (`docs/MODDING.md`). Pin a
commit or a release tag (`tag = "v0.2.0"`) if you use it: the code still changes, and the core is
not published on crates.io.

```
cargo test --workspace
```

Many tests read the install; they skip themselves when it is missing.

## How sure it is

The code marks where each piece comes from: `[game]` (read in the executable), `[data]` (read
from the game's files), `[trace]` (measured in a recording of the running game), `[assumed]` (a
guess, with what would settle it) and `[stand-in]` (a placeholder to replace).

- Compared with recordings of the original: on-foot movement, the car, the gearbox, the cameras,
  climbing and strafing (all for Bumblebee).
- Checked only by tests and captures, never against the original: firing, aiming, fighting,
  damage, effects, flight, and everything for the other robots.

## Layout

- `game/` (`bumblebee`): the Bevy program: input, drawing, shaders, sound output, the arena.
- `core/` (`tf2-core`): file readers, tuning, the control state machine, movement, the car,
  flight, climbing, animation, melee, weapons, damage, cameras, particles, sound mixing.
- `export/` (`rotf-export`): the exporter.
- `blender/`: the Blender add-on.
- `docs/`: the research notes the code cites, the JSON format and the porting guide.

## For AI agents

`AGENTS.md` is the guide for AI coding agents (Codex, Claude Code and others): the rules, the
commands and where each part lives. Claude Code reads it through `CLAUDE.md`.

## Legal

A fan project, not affiliated with or endorsed by Activision, Hasbro, Paramount, Luxoflux or
Beenox. Transformers and every character name are trademarks of their owners. This repository
holds only original source code. You must own the game to use it.

## License

The source code is under the MIT License (`LICENSE`). It covers only this repository's own code:
it gives no rights to the game's files or to the Transformers names and trademarks.
