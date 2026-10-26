# The robot JSON format, version 1

`rotf-export` writes each robot as glTF models, PNG and DDS textures, WAV sounds and two JSON
files. glTF holds the meshes, skeletons, materials and animations. The JSON holds everything glTF
has no place for: what each animation does at each tick, where the weapons hang, the render sets,
the game's own material parameters, the robot's numbers and its sound events.

This document is the contract for those two files. A tool that reads them (a Blender add-on, an
importer for Unreal, Unity or Godot, a script an agent writes) can rely on what is written here
for as long as `format_version` stays the same.

| File | What it is |
| --- | --- |
| `<pack>.json` | The robot: its files, clips and their events, weapons, render sets, vehicle, materials, numbers and sounds. |
| `<pack>_attributes.json` | The robot's attribute block exactly as the game stores it, field by field. |

`<pack>` is the character's pack name (`bumblebee`, `optimus`, `starscream`...). Both files sit in
the export folder beside the `.glb` files, `textures/` and `sounds/`.

## Versioning

Both files start with `format_version`, an integer. This document describes version **1**.

- The version goes **up** when a key is removed or renamed, or when a key changes its type, its
  unit or its meaning.
- The version stays **the same** when a key, an event type or a new value of an enumerated
  field is added.

So a reader should:

1. Check `format_version`. Refuse a version it does not know, or warn and carry on at its own
   risk.
2. Ignore keys it does not know.
3. Skip event types it does not know.
4. Treat a value it does not know in an enumerated field (a `kind`, a `play_mode`) as unknown,
   not as an error.

`exporter.version` is the version of the program that wrote the file. It is for reports only.
Never branch on it: branch on `format_version`.

## Conventions

- **Names.** Clips, nodes, events, effects, weapons and sounds are named by strings, as the game's
  files name them. The game stores most names as the CRC-32 of the name. When the exporter cannot
  find the name itself, it writes the hash as `#` and eight lowercase hex digits (`#3c956e02`).
  Such a name is still a stable identifier: the same thing always gets the same string.
- **Absent values.** `null` means "none" or "not there" (a weapon with no missile model has
  `"missile_file": null`). Some keys are left out instead of being null. Each such key is marked
  *optional* below.
- **Numbers.** Floats are written as the shortest decimal that reads back as the same 32-bit
  float, so `0.3` means 0.3f. A float that is not finite is written as `null`.
- **Ticks.** Clip time is in ticks of 1/60 s (`space.clip_tick_seconds`). Ticks are floats, but
  every event of the robots checked so far falls on a whole tick. The game itself updates every
  0.032 s (`space.game_step_seconds`), so in the game an event runs on the first update that
  passes its tick.
- **Space.** The game's space is x right, y forward, z up, in metres, right-handed. glTF is
  y up, facing +Z, in metres. Each `.glb`'s root node turns the game's space into glTF's.
  Positions in the JSON are in the game's space unless a field says otherwise.
- **Paths.** File paths are relative to the JSON's folder, with forward slashes
  (`bumblebee_robot.glb`, `textures/...png`, `sounds/<bank>/<name>.wav`). Paths into the game's
  install (`file` of a sound event, `bank` of a sample) are relative to the install folder.
- **Prose.** A few keys hold sentences for people (`space.game`, `vehicle.note`, `sounds.note`,
  `not_exported`...). They are explanations only: never parse them. This document says the same
  things as rules.
- **Key order** means nothing (the exporter writes keys in alphabetical order). The order of
  arrays does mean something where this document says so.

## `<pack>.json`

| Key | Type | What it is |
| --- | --- | --- |
| `format_version` | integer | The format's version (see above). |
| `exporter` | object | `name` (`"rotf-export"`) and `version` of the program that wrote the file. |
| `character` | object | Who this is (below). |
| `space` | object | Units and axes, as numbers and prose (below). |
| `files` | array | Every `.glb` written (below). |
| `clips` | array | Every animation clip, with its events (below). |
| `weapons` | array | The ranged weapons (below). |
| `melee_weapons` | array | The melee weapons (below). |
| `render_sets` | array | Groups of meshes the game switches on and off (below). |
| `vehicle` | object or null | The vehicle form's wheels and steering; null for a robot without a vehicle form. |
| `materials` | array | Every material with the game's technique and parameters (below). |
| `summary` | object | The numbers a mod needs most (below). |
| `character_sounds` | object | Sound events named outside the clips: engine, steps by surface... (below). |
| `sounds` | object | Every sound event the robot can play, and every sample written as WAV (below). |
| `attributes_file` | string | The file name of `<pack>_attributes.json`. |
| `not_exported` | array of strings | Prose: what the game has that is not in the export. |

### `character`

| Key | Type | What it is |
| --- | --- | --- |
| `name` | string | The display name (`"BUMBLEBEE"`), or the pack name when the game's list has none. |
| `pack` | string | The pack name: `characters/<pack>.str` in the install, and the files' prefix. |
| `attributes` | string | The attribute block's suffix: the block is `characterAttributes<attributes>`. |
| `name_id` | string | The character's name in the game's data. |

### `space`

| Key | Type | What it is |
| --- | --- | --- |
| `clip_tick_seconds` | number | 1/60: the length of one clip tick. |
| `game_step_seconds` | number | 0.032: the game's update step. |
| `game` | string | Prose: the game's axes. |
| `gltf` | string | Prose: glTF's axes. |
| `event_ticks` | string | Prose: what an event's `tick` counts. |
| `root_travel` | string | Prose: what a clip's `root_travel` and `root_turn_degrees` hold. |

### `files`

One entry per `.glb`, robot first, then the vehicle, then the other pieces.

| Key | Type | What it is |
| --- | --- | --- |
| `file` | string | The file's name. |
| `kind` | string | `robot`, `vehicle`, `weapon`, `grenade`, `missile`, `turret` or `melee`. |
| `weapons` | array of strings | The weapons this file is the model of. Weapons of one list share a model (Bumblebee's cannon and launcher are one object). |
| `animations` | integer | How many clips this file holds. |

### `clips`

One entry per clip, for every file. The robot's clips are named `<set>.<id>` (`DriveSet.Trans_R2V_B`).
The vehicle's are `VehicleModel.<id>`. A weapon's or a turret's own clips are named `<id>` alone.
A weapon-set clip may have two additive aim clips. They follow it as `<name>.aim_down` and
`<name>.aim_up`.

A clip of the robot's sets that moves no node of the robot goes with the first file whose nodes
it moves (the vehicle's idle in `DriveSet` goes with the vehicle file). Names repeat across files
(several weapons have an `Idle`), so **a clip is identified by its `file` and `name` together**.

| Key | Type | What it is |
| --- | --- | --- |
| `name` | string | The clip's name, as above. |
| `file` | string | The `.glb` it belongs to. |
| `animation` | string or null | The glTF animation's name in `file` (the same as `name`). null when the clip moves no node of its file, so it is not in the glTF; its events still are here. |
| `set` | string or null | The animation set (`DriveSet`, `CombatSet`...). null for an object's own list. |
| `id` | string | The clip's id within its set. |
| `looping` | boolean | Whether the game loops it. *Not on aim clips.* |
| `speed` | number | The game's play rate (1 is 60 ticks a second). *Not on aim clips.* |
| `crossfade` | number | Seconds to blend in from whatever played before. *Not on aim clips.* |
| `additive` | boolean | The keys are offsets laid over the pose underneath, not poses of their own. The glTF writes them laid over the rest pose. |
| `length_ticks` | number | The clip's length in ticks. |
| `aim` | string | *Optional.* `aim_down` or `aim_up` on an aim clip. |
| `attack` | object | *Optional.* The attack numbers of a combat clip (below). |
| `root_travel` | array of [x, y, z] | *Optional; only when the root moves.* The root's place at each tick from 0 to the clip's length (`round(length_ticks) + 1` entries), in metres, in the root's own frame at tick 0 (x right, y forward, z up). |
| `root_turn_degrees` | array of numbers | *Optional; with `root_travel`.* The root's turn about z at each tick, in degrees, positive toward -x (to the left). |
| `unkeyed_nodes` | integer | *Optional; vehicle clips only.* How many of the vehicle's nodes this clip has no key for (see `vehicle`). |
| `events` | array | The clip's events in tick order (below). |

**Root motion.** The glTF keeps the root in place, as the game draws it. The character's movement
during a clip is `root_travel` and `root_turn_degrees`. To move a character with the clip, take
the difference between two ticks' entries and apply it in the character's frame at the clip's start.

### `clips[].attack`

The numbers an attack clip carries, as the game stores them.

| Key | Type | What it is |
| --- | --- | --- |
| `damage` | number | The hit's damage. |
| `multiplayer_damage` | number | The damage in multiplayer; negative means "use `damage`". |
| `ai_damage` | number | The damage when the computer runs the attacker; negative means "use `damage`". |
| `direction` | integer | The way the blow goes: 0 the attacker's forward, 1 its left, 2 its right, 3 straight down, 4 straight up, 5 from the attacker toward the target; 6 and above, no direction. |
| `stop_on_hit` | boolean | Once the attack has hit, the stick no longer turns the attacker. |
| `knock_back_speed` | number | The speed of the throw a hit gives (metres per second). |
| `knock_back_angle_degrees` | number | The throw's angle above the ground. |
| `slide_speed`, `min_range`, `max_range`, `auto_target_turn_speed`, `turn_speed`, `stick_choose_target_max_degrees`, `auto_target_search_angle_degrees` | number | How the attack slides, picks and turns toward a target, as the game stores them (metres, degrees). `tf2_core::melee` shows how the game uses each. |
| `damage_flags` | string | The damage type bits the hit carries, as hex (`"0x00000200"`). |
| `damage_flags_small` | string | The same, against small characters. |
| `damage_flags_large` | string | The same, against large characters. |
| `damage_flags_excluded` | string | The bits taken off whichever set applies. |

### `clips[].events`

Every event has `tick` (number) and `type` (string). Events are sorted by `tick`. Events at the
same tick come in a fixed order of kinds: `model`, the sound events, the effect and step events,
the weapon events, `special_fire`, `camera_shake`, the attack and motion events, then `trigger`.

An event with a `node` names a node of the clip's file. A clip shared by two weapons can name a
node that only the other weapon has. Such an event carries `node_in_file`, set to false. Without that
key, the node is in the file.

How to run them: play the clip, and each frame run the events whose tick lies after the last
frame's position and at or before this frame's. At the clip's very start, also run the events
at tick 0. When a looping clip wraps around, run the events past the old position and those up
to the new one. An event with `"rerun": false` runs only on a looping clip's first time round.

| `type` | Fields | What it does |
| --- | --- | --- |
| `model` | `show_robot` (bool), `show_vehicle` (bool), `vehicle_animation` (string or null), `vehicle_then` (string or null), `vehicle_file` (string or null) | The change of form. It shows or hides the robot model and the vehicle model. When `vehicle_animation` is set, it starts that clip on the vehicle (in `vehicle_file`), and `vehicle_then` follows it when it ends. |
| `sound_play` | `event` (string), `auto_stop` (bool), `held` (bool), `rerun` (bool) | Plays the sound event `event` (a key of `sounds.events`). With `auto_stop`, the sound ends with the clip. With `held`, the object keeps the sound by name, and a later play of the same name reuses the playing sound instead of starting another. |
| `sound_stop` | `event` (string) | Stops that sound event. |
| `effect_start` | `node`, `effect` (string), `id` (string or null), `follows_node` (bool), `use_rotation` (bool), `use_scale` (bool), `auto_stop` (bool), `rerun` (bool) | Starts the particle effect `effect` at `node`. With `follows_node`, it moves with the node; otherwise it stays where it started. `use_rotation` and `use_scale` take the node's turn and scale. `id` names the effect so that an `effect_stop` can end it. With `auto_stop`, it is stopped when the clip is left. The effects themselves are not exported. |
| `effect_stop` | `node`, `id` (string or null) | Stops the effect started with that `id`. |
| `footstep` | `node`, `side` (string: `LEFT` or `RIGHT`) | A foot comes down at `node`. The game probes the ground there and plays the step's sound and effect for the surface (`character_sounds.by_surface.step`). |
| `climb_step` | `node`, `limb` (`left_hand`, `right_hand`, `left_foot`, `right_foot` or `slide`) | A limb touches the wall while climbing. The sound and effect come from `character_sounds.by_surface.climb`, in that order of limbs. |
| `weapon_attach` | `node`, `weapon` (string or null), `index` (integer or null), `weapon_file` (string or null), `only_if_current` (bool), `hide` (bool), `unhide` (bool), `weapon_animation` (string or null), `weapon_then` (string or null), `hide_when_done` (bool) | Moves a weapon to `node`: the weapon named `weapon` or, when that is null, the one at `index` in the robot's list. With `only_if_current`, it acts only if that weapon is the one in hand. `hide` and `unhide` hide or show it. `weapon_animation` starts that clip on the weapon's own model (`weapon_file`), and `weapon_then` follows it. With `hide_when_done`, the weapon is hidden when the clip ends instead. |
| `weapon_fire_enabled` | `on` (bool) | Whether the trigger reaches the weapon from here on. |
| `special_fire` | `node` | The character's special weapon fires now, from `node`. |
| `camera_shake` | `shake` (string), `positional` (bool) | Starts the named camera shake: at the object's place when `positional`, wherever the camera is when not. |
| `branch_point` | none | From here on, the clip may be left for another one. |
| `attack_button_window` | `open` (bool) | While the window is open, a press of the attack button counts toward the next hit. |
| `attack_face_and_slide` | `slide`, `face`, `stick` (bool) | What the attack may do from here: slide toward the target, turn to face it, follow the stick. |
| `attack_hit` | `on` (bool), `node` | The hit lands while this is on. The collision body on `node` is the one that hits. |
| `trigger` | `name` (string); *optional* `message` (string), `on_exit` (bool) | Sends a trigger to whatever on the robot goes by `name` (a spawner, an effect, a sound). `message` is the message sent. With `on_exit`, it is sent also when the clip is left. |
| `traction` | `axes` ([x, y, z]) | How much of each axis the clip's motion drives against the body, from here on. |
| `motion_mode` | `mode` (string) | Switches the body's motion mode (the names are mostly hashes). |
| `dash` | `timer` (number) | Sets the character's dash clock, which homing missiles read. |
| `force_up` | `up` ([x, y, z]) | Turns the body so that its up axis is this. |
| `partial_animation` | `animation` (string, `<set>.<id>`), `layer` (integer) | Starts that clip of the robot file on the given layer, over the clip below. |
| `finish_move` | `start_camera`, `end_camera`, `kill` (bool) | The finishing move's hold of a target it would kill, with its camera. |

### `weapons`

One entry per ranged weapon, in the robot's own order. That order is what a `weapon_attach`
event's `index` counts.

| Key | Type | What it is |
| --- | --- | --- |
| `weapon` | string | The weapon's name. |
| `kind` | string | `gun`, `launcher`, `grenade`, `flame` or `turret`. |
| `list` | integer | The weapon list it belongs to. Weapons of one list share one model. |
| `file` | string or null | The weapon's model. null for a turret launcher, whose turret is thrown, not held. |
| `start_node` | string | The node it hangs on until a `weapon_attach` event moves it. |
| `hangs_on` | string or null | `robot` or `vehicle`: which model has `start_node`. null when neither has it; then the weapon hangs nowhere until a clip's `weapon_attach` names a node. |
| `barrel_nodes` | array of strings | The weapon model's barrel nodes (`FireDummy`, `FireDummy01`...). A firing's projectiles leave from each in turn. |
| `barrels` | array of [x, y, z] | The barrel nodes' rest translations as the weapon's data stores them (game axes, metres). The `.glb`'s nodes of the same names are the place to fire from. |
| `missile_file` | string or null | The missile's model. |
| `grenade_file` | string or null | The grenade's model. |
| `turret_file` | string or null | The thrown turret's model. |
| `always_appear` | boolean | The weapon stays visible when it is put away. |
| `object_clips` | object | The weapon model's own clips by role: any of `idle`, `exit`, `exit_overheat`, `overheat`, `charging`, `charged`. Missing roles are left out. |
| `robot_exit_clip` | string or null | The robot's clip for putting the weapon away. |
| `sounds` | object | Sound events by role: any of `fire`, `overheat`, `refused`, `ring_off`, `idle`, `charge_start`, `charge_loop`, `charge_full`, `charge_fire`. Missing roles are left out. |
| `numbers_from` | string | The name of the weapon's data block in the game. |
| `numbers` | object or null | That block as the game stores it, every field by the game's own name (damage, heat, ranges...). It is the game's data, not part of this format: its keys can be anything the game has. |

**Hanging a weapon.** Every bone in the glTF has a child node named `<bone>.frame`. The skins bind
to those, as the game's drawing does. To hang a weapon on a node, put the weapon file's root's
children under that node's `.frame`, without the weapon's root node.

### `melee_weapons`

| Key | Type | What it is |
| --- | --- | --- |
| `weapon` | string | The weapon's name. |
| `file` | string or null | Its model. |
| `start_node` | string | The node it hangs on. |
| `hangs_on` | string or null | `robot`, `vehicle` or null, as for `weapons`. |
| `exit_clip` | string or null | The robot's clip for putting it away. |

### `render_sets`

Some models switch groups of meshes on and off (Optimus's special-attack parts). One entry per
file that has such groups.

| Key | Type | What it is |
| --- | --- | --- |
| `file` | string | The `.glb`. |
| `groups` | array | Each with `exclusive` (bool), `default` (string or null) and `sets`. |
| `groups[].exclusive` | boolean | Only one set of the group shows at a time. |
| `groups[].default` | string or null | The set an exclusive group starts on (its first); null for a group that is not exclusive. |
| `groups[].sets` | array | Each with `name` (string) and `meshes` (array of mesh names). |

A mesh named here is a node of the file with that name, or `<name>.<n>` when the mesh has several
parts. The meshes outside an exclusive group's default set start hidden. Their nodes say so twice:
with `KHR_node_visibility`, and with the custom property (glTF extras) `render_set_hidden: true`.
Blender reads only the second.

### `vehicle`

| Key | Type | What it is |
| --- | --- | --- |
| `file` | string or null | The vehicle's `.glb`. |
| `wheels` | array | Each has `node` (a tyre node) and `wheel` (`front_left`, `front_right`, `rear_left`, `rear_right`). |
| `steering_nodes` | array of strings | The nodes that turn with the steering. |
| `note` | string | Prose: what the game moves in code (see below). |
| `clips_hold_unkeyed_nodes` | string | Prose: the rule on unkeyed nodes (see below). |

Two rules for the vehicle:

- **The game moves the wheels in code**, on top of the vehicle's clip, every frame. Each tyre
  spins with its wheel's travel and drops with its suspension to the ground, and the steering
  nodes turn with the steering. The clips alone leave the tyres where the change of form puts
  them, off the ground.
- **A vehicle clip leaves the nodes it does not key where the clip before left them.** glTF puts
  them back at rest instead. A clip with `unkeyed_nodes` needs the clip before it: play the change
  of form (`Trans_R2V_A`, then `Trans_R2V_B`) before `V_Idle`, or hold the last pose for those
  nodes. Starscream's `V_Idle` alone shows the jet folded small.

### `materials`

Every material the files use. glTF's metallic-roughness material in the `.glb` is only an
approximation of the game's shaders. These are the game's own settings.

| Key | Type | What it is |
| --- | --- | --- |
| `name` | string | The material's name, as in the glTF. |
| `technique` | string | The game's shader technique (`aniso_paint_x`, `glass_plus`, `emissive`, `cheap`...). |
| `blend` | string or null | `Opaque`, `Additive`, `Normal`... |
| `parameters` | array | Each has `name` and `value`, and `of` when the parameter belongs to a sub-part (*optional*). |
| `parameters[].value` | number, array of numbers, string or null | A number, a vector, a word, or a texture's path under `textures/`. A texture the glTF does not use (a specular or reflection map) is written there too. |

The vertex colour is kept in the glTF as the attribute `_COLOR0`. Most techniques read it as
blend weights, not as a tint.

### `summary`

The numbers a mod needs most, from the attribute block, in metres, seconds and metres per second.

| Key | What it is |
| --- | --- |
| `health`, `health_regen_rate`, `health_regen_delay` | Health, how fast it comes back, and after how long. |
| `robot`, `vehicle` | Each has `height`, `radius` and `mass`: the body of each form. |
| `gravity` | The fall's acceleration. |
| `walk_speed`, `run_speed` | Measured on the walk and run clips' root travel. |
| `strafe_speed` | The speed in weapon mode. |
| `jumps` | `standing`, `running`, `high_standing`, `long`: each with `height` and `speed`. |
| `drive` | The vehicle form: `max_speed`, `max_turbo_speed`, `max_reverse_speed`, `wheel_radius`. |
| `units` | Prose: the units. |
| `error` | *Optional.* Only when the numbers could not be read, instead of the keys above. |

### `character_sounds`

Sound events the robot's data names outside its clips. Each is a key of `sounds.events`, or null.

| Key | What it is |
| --- | --- |
| `engine`, `drift_start`, `suspension`, `turbo`, `turbo_ready` | The vehicle's sounds. |
| `drive`, `hover` | A flyer's flight loops. |
| `by_surface` | The sounds and effects chosen by the surface touched (below). |

`by_surface` holds `step`, `melee_hit`, `tyre_skid` and `tyre_road` (each a list of presets) and
`climb` (one such list per limb, in `climb_step`'s order of limbs). A preset is
an object with `surfaces` (a list of surface names), `sound` (an event or null) and `effect` (an effect or null). The preset for a
surface is the first that lists it; if none does, it is the first that lists the default surface.
`note` says this in prose.

### `sounds`

The sound events, as FMOD Designer stores them in the game, and the samples they play.

| Key | Type | What it is |
| --- | --- | --- |
| `folder` | string | `"sounds"`: where the WAVs are. |
| `files` | array | The event files read: each has `file` and `all_events`. With `all_events`, every event of that file was taken; otherwise only the events the robot names. |
| `shared_level` | string | The level whose copy of the shared events (footsteps, jumps, punches) was taken. Every level has its own copy. |
| `not_found` | array of strings | Events the clips name that no event file holds. The game plays nothing for them. |
| `note` | string | Prose. |
| `events` | object | Event name → event (below). |
| `samples` | object | WAV path → sample (below). |

An **event**:

| Key | What it is |
| --- | --- |
| `file` | The event file in the install that holds it. |
| `category` | FMOD's category (`FX/FOLEY/...`). |
| `volume`, `volume_random` | Its volume as FMOD stores it (1 is full) and the random part. |
| `pitch_octaves`, `pitch_random_octaves` | Its pitch in octaves (0 plays the sample as recorded) and the random part. |
| `positional` | It is placed in the world (3D). |
| `min_distance`, `max_distance` | The 3D rolloff's range, in metres. |
| `max_playbacks` | How many may play at once. |
| `fade_in_ms`, `fade_out_ms` | Fades, in milliseconds. |
| `reverb_dry_db`, `reverb_wet_db` | Reverb levels. |
| `loops` | It plays until it is stopped. |
| `parameters` | Each has `name`, `min`, `max`, `velocity`, `seek_speed`, `primary` and `sustain` (FMOD's own settings of the parameter). The name `(distance)` is the listener's distance. |
| `layers` | Each has `parameter`, `sounds` and `envelopes`. |

A layer's `sounds` are laid along its `parameter`: each covers `start` to `start + length` of the
parameter's range, as shares of it. A layer's `envelopes` each have `drives`, `parameter` and
`points`: `points` are `[x, y]` pairs over the parameter. `drives` is `volume`, `pitch`,
`spatial_blend`, or the name of an FMOD effect (`FMOD Lowpass Simple`...).

A layer's **sound**:

| Key | What it is |
| --- | --- |
| `start`, `length` | Its stretch of the layer's parameter, as shares. |
| `one_shot` | It plays once rather than looping. |
| `volume`, `fine_tune`, `fade_in`, `fade_out` | Its own settings. |
| `definition` | The sound definition's name. *The keys from here on are left out when the definition is missing.* |
| `play_mode` | How it picks its sample: `sequential`, `random`, `random_no_repeat`, `shuffle`, `shuffle_global`, `programmer` or `unknown`. |
| `definition_volume`, `definition_volume_floor` | The definition's volume range. |
| `definition_pitch_octaves`, `definition_pitch_random_octaves` | The definition's pitch, in octaves. |
| `samples` | Each has `file`, a WAV path (a key of `sounds.samples`, or null when it could not be written), and `weight`, its weight in the pick. |

A **sample**:

| Key | What it is |
| --- | --- |
| `bank` | The `.fsb` bank in the install. |
| `index`, `name` | Its place and name in the bank. |
| `rate`, `channels`, `frames`, `seconds` | The WAV's format and length. |
| `loop` | `[start, end]` in frames (end exclusive), or null when the sample does not loop. |

## `<pack>_attributes.json`

| Key | Type | What it is |
| --- | --- | --- |
| `format_version` | integer | As above. |
| `root` | string | The block's name in `bnxglobal.str`: `characterAttributes<attributes>`. |
| `attributes` | object or null | The block, every field by the game's own name. null when it was not found. |

The block is the game's data, not part of this format. It holds the moves' numbers, the avatar
bodies, the control state machine (`stateMachineList`) and the brains of computer-run characters
(`brainList`). Values are written as follows:

- An enumerated value or a boolean is written as its name (`"true"`, `"false"`).
- A flag field is written as a list of flag names.
- An object reached through a pointer carries its `_type` and `_name`.
- A reference the exporter did not follow is written as `{ "_link": name }`.
- A chain deeper than 12 levels is cut off as `"..."`.

## Not in the export

The game has these, and the export does not. `not_exported` lists them too.

- The voice lines (the game picks them in code).
- The particle effects, ribbons and lights: only their names at their events.
- The animation state machine: which clip follows which, and its rules.
- What the game moves in code: flight's surfaces, the wheels' spin, the suspension.
- The collision bodies.

## Changes

- **1** (2026-10-26): the first version. It describes what rotf-export 0.1.0 writes.
