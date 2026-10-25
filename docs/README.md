# Research notes on the robots

These are the working notes behind the robots in this repository: what was read in the PC game's
executable and data, what was measured in recordings of it running, and how each piece was ported.
The code cites them as `docs/<name>.md`. They are published as they were written during the work
(dated, in the order things were found), with machine paths removed. Where a note and the code
disagree, the code and its tests are the later word.

The notes describe behaviour; they contain no game files and no decompiled code. Running anything
here still needs your own install of the game.

To port the robots into another game, start with [MODDING.md](MODDING.md) instead.

## How to read them

- **Addresses** (`00867f20`, `FUN_005bcb30`, `+0x324`) are functions, data and structure offsets in
  the PC executable, `Transformers2.exe` (the Beenox port). A range such as `00867f20-00867f5b` is a
  stretch of its code.
- **Tags** say where each claim comes from, as in the code: `[game]` read in the executable,
  `[data]` read from the game's files, `[trace]` measured in a recording of the running game,
  `[assumed]` a guess (with what would settle it), `[stand-in]` a placeholder still to replace.
- **Names** are CRC-32 hashes of the game's own names (case sensitive); a note gives the hash and
  the name where the name is known.
- **Space** is right-handed, x right, y forward, z up, in metres. The game updates every 0.032 s;
  animation clips tick at 1/60 s.
- `<install>` is the game's install folder (the one that holds `bnxglobal.str`).

## What is not published

- **Other notes.** The project's notes on the enemy AI, missions, levels and level rendering, its
  status diary, its plans and its check logs are not here. Where the code or a note leaned on one,
  it now says "the unpublished notes".
- **The workspace.** References to `work\`, `export\`, `tools\`, `trace\` and `handoff\` name the
  author's own workspace: analysis scripts, the offline emulator, decoded data tables, recordings of
  the game and their readers. They are kept as a record of how a fact was found; the files
  themselves are not in this repository. `CLAUDE.md` is that workspace's guide for the agents that
  helped with the work.
- **The ledger and census** (`functions.tsv`, `census.tsv`), the list of every function read.

## The notes

### Movement and control
- [state-machine.md](state-machine.md): the control state machine.
- [bumblebee-mechanics.md](bumblebee-mechanics.md): Bumblebee on foot, the first research notes.
- [animation-rules.md](animation-rules.md): how clips are chosen (the animation rules).
- [clip-events-traction-motion.md](clip-events-traction-motion.md), [clip-events-traction-modes.md](clip-events-traction-modes.md): the clip events that change traction and motion mode.
- [climbing.md](climbing.md): climbing.
- [foot-ik.md](foot-ik.md): foot IK.
- [input.md](input.md): the PC keyboard and mouse path and the game's default bindings.

### The body against the world
- [character-world-contact.md](character-world-contact.md), [character-world-probes.md](character-world-probes.md): a character on foot, its collision body and how it meets the world.
- [character-ground-rule.md](character-ground-rule.md), [character-ground-test.md](character-ground-test.md), [character-height-fields.md](character-height-fields.md): height over the ground and the grounded flag.
- [physics.md](physics.md): the physics system (the world step, contacts, the solver).
- Collision: [collision-core.md](collision-core.md) (GJK), [collision-shapes.md](collision-shapes.md), [collision-support-2.md](collision-support-2.md), [collision-support-hull.md](collision-support-hull.md), [collision-support-swept.md](collision-support-swept.md), [collision-sphere-sweep.md](collision-sphere-sweep.md), [collision-ray-soup.md](collision-ray-soup.md), [collision-hull-placed.md](collision-hull-placed.md), [collision-response.md](collision-response.md), [collision-query-masks.md](collision-query-masks.md), [collision-world.md](collision-world.md), [collision-world-query.md](collision-world-query.md), [collision-loader.md](collision-loader.md), [collision-data.md](collision-data.md), [collision-tests-emulated.md](collision-tests-emulated.md).

### The vehicle form and flight
- [driving.md](driving.md): the car (gearbox, suspension, drift, turbo).
- Flight: [flight-update.md](flight-update.md), [flight-states.md](flight-states.md), [flight-attributes.md](flight-attributes.md), [flight-forces.md](flight-forces.md), [flight-contact.md](flight-contact.md), [flight-boundary.md](flight-boundary.md), [flight-helicopter.md](flight-helicopter.md), [flight-hover.md](flight-hover.md), [flight-stunts.md](flight-stunts.md), [flight-camera-exit.md](flight-camera-exit.md).

### Fighting
- [melee.md](melee.md): melee targets and hits.
- [weapon-mode.md](weapon-mode.md): weapon mode (strafing, aim, aim assist, firing, heat, lock-on).
- [weapon-kinds.md](weapon-kinds.md), [weapon-charged-shot.md](weapon-charged-shot.md), [weapon-flamethrower.md](weapon-flamethrower.md): grenade launchers, the charged shot, the flamethrower.
- [character-weapon-data.md](character-weapon-data.md): what each character's pack lists.
- [damage.md](damage.md): receiving damage, health and death.
- Specials: [character-specials.md](character-specials.md), [special-types.md](special-types.md), [special-weapons.md](special-weapons.md), [special-jetfire-mines.md](special-jetfire-mines.md), [special-combat-repair.md](special-combat-repair.md).

### Characters
- [devastator.md](devastator.md): Devastator as a playable walking giant.
- [optimus-mesh-visibility.md](optimus-mesh-visibility.md): Optimus's meshes.
- [grindor-cinematic-wave.md](grindor-cinematic-wave.md): Grindor's blast wave, a design beyond the original built from the game's own effects.

### Cameras
- [camera.md](camera.md): the follow, driving and weapon cameras.
- [camera-finisher.md](camera-finisher.md), [camera-finisher-eye.md](camera-finisher-eye.md): the finisher camera.

### Effects, sound and the HUD
- [particles.md](particles.md): the particle system.
- [effects-shockwave-tube.md](effects-shockwave-tube.md): ShockWave and EffectTube.
- [surface-damage.md](surface-damage.md): surface damage.
- [shaders.md](shaders.md): the robots' surface shaders.
- [sound.md](sound.md): sound events, banks and the mixer.
- [hud.md](hud.md): the HUD.

## Publishing them again

The notes are kept in the author's workspace and copied here by a script that removes machine
paths and rewrites the references; the same script points the code's comments at `docs/`.
