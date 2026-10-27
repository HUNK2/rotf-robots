# rotf-robots-sdk: the robots of Transformers: Revenge of the Fallen, for other games

The engine-free half of [rotf-robots](https://github.com/HUNK2/rotf-robots), for modders who want
the playable robots of *Transformers: Revenge of the Fallen* (PC, 2009) in another game: Unreal,
Unity, Godot, Blender, or a host game. Nothing here uses Bevy or any other engine.

- **`rotf-export`**: writes a robot from your own install as standard files, ready for modding
  tools: glTF models with their skeletons and animations, PNG and DDS textures, WAV sounds, and a
  JSON file with everything a model cannot hold (animation events, the change of form, weapons
  and where they mount, meshes that start hidden, hit boxes, sound events, the robot's
  attributes). The JSON's format is written down and versioned in [`docs/FORMAT.md`](docs/FORMAT.md).
- **`blender/`**: a Blender add-on that imports an exported robot ready to animate: 60 fps, the
  hidden meshes hidden, events as markers, the vehicle's clips holding their pose, weapons on their
  nodes, collision shapes, and a panel that plays a change of form ([`blender/README.md`](blender/README.md)).
- **`tf2-core`**: the game's file-format readers and the robots' mechanics (movement, the change
  of form, the car, flight, climbing, fighting, weapons, damage, cameras), as a Rust library
  with no engine types.
- **`docs/`**: the research notes behind both, and `docs/MODDING.md`, the porting guide: one for every host
  engine, written as tasks for the AI agent doing the port.

The Bevy game that plays the robots is [rotf-robots](https://github.com/HUNK2/rotf-robots). It
uses this repository's `tf2-core` and is the reference for how the robots should play.

## You need the game

Nothing from the game is in this repository, and nothing is downloaded. Everything is read from
**your own install of the PC game**, which is never written to. A normal install is found through
the Windows registry; otherwise pass `--game-dir` with the folder that holds `bnxglobal.str`.

## Exporting a robot

Install Rust (stable) and run, from this folder:

```
cargo run --release -p rotf-export -- --list
cargo run --release -p rotf-export -- --character bumblebee
cargo run --release -p rotf-export -- --all
```

`--character` writes one robot into `exported/<pack>`; `--all` writes the whole roster (20 packs,
about 1 GB). `--out <folder>` picks another folder. Each robot's folder has a `README.txt`
describing its files.

To check a native port against the reference, `--character <name> --run <script.json>` plays a
script of inputs through it and writes every update's place, state, clip and events as JSON lines
(`docs/FORMAT.md`, "Reference runs"; an example script is `export/runs/basics.json`).

The exported files are made from the game and belong to its owners. Keep them on your own
machine. A mod you share should tell its users to run `rotf-export` themselves.

## The core: the reference a port is built from

```toml
[dependencies]
tf2-core = { git = "https://github.com/HUNK2/rotf-robots-sdk", rev = "<commit>" }
```

The core is not meant to ship inside a mod: a native port rebuilds the robot from the host game's
own parts, and the core is what it is built from and checked against (`docs/MODDING.md`). Pin a
commit if you use it: there are no releases yet and the code still changes.

```
cargo test --workspace
```

Many tests read the install; they skip themselves when it is missing.

## How sure it is

The code marks where each piece comes from: `[game]` (read in the executable), `[data]` (read
from the game's files), `[trace]` (measured in a recording of the running game), `[assumed]` (a
guess, with what would settle it) and `[stand-in]` (a placeholder to replace). Bumblebee's
movement, car, cameras, climbing and strafing were compared with recordings of the original;
everything else is checked only by tests and captures.

## For AI agents

`AGENTS.md` is the guide for AI coding agents: the rules, the commands and where each part lives.
Claude Code reads it through `CLAUDE.md`.

## Legal

A fan project, not affiliated with or endorsed by Activision, Hasbro, Paramount, Luxoflux or
Beenox. Transformers and every character name are trademarks of their owners. This repository
holds only original source code. You must own the game to use it.

## License

The source code is under the MIT License (`LICENSE`). It covers only this repository's own code:
it gives no rights to the game's files or to the Transformers names and trademarks.
