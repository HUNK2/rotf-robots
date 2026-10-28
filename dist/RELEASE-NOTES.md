The playable robots of *Transformers: Revenge of the Fallen* (PC, 2009), rebuilt in Bevy and Rust, with the
tools to take them into other games.

**You need your own copy of the PC game.** Nothing from it is in these downloads: the programs read the game's
files from your install every time they run, and never change them. A normal install is found by itself;
otherwise the game asks for the folder that holds `bnxglobal.str` the first time.

### Downloads

- `rotf-robots-0.2.0-windows-x64.zip`: unzip and run `bumblebee.exe` to play (Windows 64-bit).
  `rotf-export.exe`, beside it, writes a robot out for modding. `README.txt` has the keys and options.
- `rotf_import-0.2.0.zip`: the Blender add-on (4.4 or later) that imports an exported robot ready to animate.
  In Blender: Edit > Preferences > Get Extensions > Install from Disk.
- `SHA256SUMS.txt`: the checksums of both.

### What's in it

- **13 robots**, 7 that drive and 6 that fly, plus 5 reskins, each on its own attributes, health, speeds,
  weapons, specials and animation sets. F2 opens the robot menu; F6 / F7 step through the roster;
  `--character <name>` starts as one.
- A test arena (`--scouts` adds three enemy scouts to fight), or one of the game's levels as a stage
  (`--mission <LevelName>`).
- Modern keyboard and mouse controls, or the original's with `--classic-controls`; pads use the game's bindings.
- **rotf-export**: a robot as glTF (skeletons and animations), PNG and DDS textures, WAV sounds and a JSON file
  with the rest: animation events, the change of form, weapons and their rounds, sounds, hit boxes, the
  state machine and the mechanics' numbers. The format is `docs/FORMAT.md`; the porting guide for other
  engines is `docs/MODDING.md`.

### Since the last build

- The game and the modding kit are one repository now, [HUNK2/rotf-robots](https://github.com/HUNK2/rotf-robots).
- The weapons run in the engine-free core, so a port can replay a whole update (pad, aim, camera, weapons,
  damage) through `rotf-export --run`.
- Ironhide's deployed turret aims and fires its missiles, from the game's data.
- The exporter writes the HUD's pictures, the stage's lighting, weapon rounds with their effects and debris,
  the trails' materials and more (`docs/FORMAT.md`, "Changes").

### How sure it is

Bumblebee is the most complete: his movement, car, gearbox, cameras, climbing and strafing are compared with
recordings of the original. Firing, fighting, damage, effects, flight and the other robots are checked only by
tests and captures. Not the whole game: no missions, menus, saves or cutscenes.

MIT License. A fan project, not affiliated with or endorsed by Activision, Hasbro, Paramount, Luxoflux or
Beenox. Transformers and every character name are trademarks of their owners.
