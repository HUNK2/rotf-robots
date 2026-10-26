# rotf-robots-sdk: the robots of Transformers: Revenge of the Fallen, for other games

The engine-free half of [rotf-robots](https://github.com/HUNK2/rotf-robots), for modders who want
the playable robots of *Transformers: Revenge of the Fallen* (PC, 2009) in another game: Unreal,
Unity, Godot, Blender, or a host game. Nothing here uses Bevy or any other engine.

- **`rotf-export`**: writes a robot from your own install as standard files, ready for modding
  tools: glTF models with their skeletons and animations, PNG and DDS textures, WAV sounds, and a
  JSON file with everything a model cannot hold (animation events, the change of form, weapons
  and where they mount, meshes that start hidden, sound events, the robot's attributes). The JSON's
  format is written down and versioned in [`docs/FORMAT.md`](docs/FORMAT.md).
- **`tf2-core`**: the game's file-format readers and the robots' mechanics (movement, the change
  of form, the car, flight, climbing, fighting, weapons, damage, cameras), as a Rust library
  with no engine types.
- **`docs/`**: the research notes behind both, and `docs/MODDING.md`, the porting guide.

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

The exported files are made from the game and belong to its owners. Keep them on your own
machine. A mod you share should tell its users to run `rotf-export` themselves.

## Using the core from Rust

```toml
[dependencies]
tf2-core = { git = "https://github.com/HUNK2/rotf-robots-sdk", rev = "<commit>" }
```

Pin a commit: there are no releases yet and the API still changes. `docs/MODDING.md` describes how
to load a robot and step it in a host game.

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
