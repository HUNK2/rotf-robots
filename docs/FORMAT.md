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
| `<pack>_run.jsonl` | A reference run: a scripted input played through the reference, one line per update ("Reference runs"). |

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
| `collision` | object | The collision bodies on each file's nodes: hit boxes, attack bodies (below). |
| `vehicle` | object or null | The vehicle form's wheels and steering; null for a robot without a vehicle form. |
| `materials` | array | Every material with the game's technique and parameters (below). |
| `summary` | object | The numbers a mod needs most (below). |
| `character_sounds` | object | Sound events named outside the clips: engine, steps by surface... (below). |
| `sounds` | object | Every sound event the robot can play, and every sample written as WAV (below). |
| `control` | object | The control state machine: what decides when he moves, jumps, attacks, dodges, climbs, aims or changes form (below). |
| `animation_rules` | object | Within each animation set, which clip plays and which follows it (below). |
| `mechanics` | object | Each mechanic's numbers for this robot, for the formulas in MECHANICS.md (below). |
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
| `unkeyed_node_names` | array of strings | *Optional; with `unkeyed_nodes`.* Those nodes, by name. |
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

### `collision`

The game's collision bodies: simple shapes that ride the nodes of an object and move with its
animation. On a robot they are its hit boxes (where a shot or a blow lands, and on which part)
and its attack bodies (the hands, feet or blade that do the hitting). The vehicle, the melee
weapons and the projectiles have their own.

| Key | Type | What it is |
| --- | --- | --- |
| `files` | array | One entry per file that has bodies: `file` (the `.glb`) and `bodies`. |
| `note` | string | Prose: the rules below. |

A **body**:

| Key | Type | What it is |
| --- | --- | --- |
| `node` | string | The node it rides. |
| `gltf_node` | string | Its own node in the `.glb`: `<node>.collision.<n>`, where `<n>` is its place in the file's `bodies`. |
| `shape` | string | `sphere`, `box` or `cylinder`. |
| `radius` | number | A sphere's or a cylinder's radius, as stored. *Sphere and cylinder only.* |
| `half_height` | number | Half a cylinder's height, as stored. The cylinder stands along its own z, from -`half_height` to +`half_height`. *Cylinder only.* |
| `half_extents` | [x, y, z] | Half a box's size along each of its axes, as stored. *Box only.* |
| `translation` | [x, y, z] | Where it sits in its node's space, in metres, game axes. |
| `rotation` | [x, y, z, w] | Its turn in its node's space, a quaternion. |
| `scale` | [x, y, z] | The scale its data gives it (Bumblebee's hand spheres: 1.97). |
| `scaled` | object | Its sizes with `scale` in them: `radius`, `half_height` or `half_extents`, as above. |
| `mask_preset` | string or null | What it hits and what hits it: `Character` (a hit box), `Attack` (a body that hits), `VehicleHittablePart`, `Missile`... The presets are in the game's `bnxglobal.str`. |
| `material` | string or null | On a character, the part a hit on this body strikes: `Head`, `UpperBody`, `LowerBody`, `VehicleBody`. The game weighs damage by it. |

Rules:

- **A body rides its node, not the node's `.frame`.** The game places it with the node's matrix
  without the node's own scale. Its glTF node is a child of that node, with `translation`,
  `rotation` and `scale` as its transform. So in any importer it stands where the game puts it,
  and it follows the animation.
- **Its glTF node is an empty node.** The body is in the node's custom properties (glTF extras)
  as `rotf_collision`, an object with the same keys as the body above, without `node` and
  `gltf_node`. A host makes its own collider from it: a sphere, box or capsule of that size, on
  that node.
- **`scaled` is the port's reading, not the game's.** The data stores the shape raw and the scale
  apart. The port sizes a sphere's and a cylinder's radius by the x scale and a half height by
  the z scale. How the game's collision test uses the scale has not been read. `radius`,
  `half_height`, `half_extents` and `scale` are as stored, for a reader that wants to decide
  for itself. In all the packs, every sphere and cylinder has the same scale on all three axes
  (only boxes have uneven ones), so the reading only decides that the scale applies at all.
- **A scale can be negative**: a mirrored box (Grindor's blade). The box is the same box; its
  size is the absolute value, as `scaled` gives it.
- **An attack body counts only while a clip's `attack_hit` event on its node is on.** A melee
  weapon's bodies move with the weapon's own clips; the port poses them unblended, and that too
  is the port's reading.

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
  nodes. Starscream's `V_Idle` alone shows the jet folded small. Only the nodes in
  `unkeyed_node_names` hold: a node the glTF does not animate but that list leaves out has a
  channel in the game that keys nothing, and it stays at rest.

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

### `control`

What decides when the robot moves, jumps, attacks, dodges, climbs, aims or changes form is not
code in the original: it is a graph of states and connections in the game's data. Each state
plays a clip and puts the body in a motion mode; each transition out of it names a connection,
which tests the pad, the body and the world, and a priority. This is that graph, as the game
joins it: a state that several of the character's machines list has every machine's transitions.
It maps onto an animation graph or a state machine in another engine. `docs/state-machine.md` has
the research behind it; `core/src/control.rs` runs it.

If the attribute block cannot be read, `control` is `{ "error": "..." }` instead.

| Key | Type | What it is |
| --- | --- | --- |
| `update_seconds` | number | 0.032: the machine runs once per game update. |
| `start_state` | string or null | `stateIdle`: where the machine starts, and where a finished state with nowhere to go returns. |
| `buttons` | array | The pad's buttons as connections name them: each has `number` (the game's) and `name` (`Jump`, `AttackFast`, `VehicleMode`...). |
| `states` | array | Every state (below). Names are unique. |
| `connections` | array | Every connection a transition names (below). Names are unique. |

The button names are the game's binding names; `docs/MODDING.md` and `docs/input.md` say which
pad and keyboard button each is. Several share one button (`AttackFast`, `Dodge` and
`AttackTransformSlam` are all the left face button): the state decides which one is meant.

#### `control.states`

| Key | Type | What it is |
| --- | --- | --- |
| `name` | string | The state (`stateIdle`, `stateAttack1`, `stateDriveEnterA`...). |
| `class` | string | What kind of state it is: what its code does beyond this data (below). |
| `overlay` | boolean | It runs on top of another state instead of replacing it (the special, the firing). |
| `clip` | string or null | The clip it plays, by its name in `clips`. null when it names none, or names one this robot does not have (then `clip_id` says which). |
| `clip_id` | string or null | The clip's id as the state names it, whether or not the robot has it. |
| `anim_set` | string or null | The animation set it names. With no clip, the set's own entry rules pick one (`animation_rules`). |
| `robot_form` | string or number | The control mode it puts the character in as it is entered: `robot`, `weapon`, `vehicle`, or `unchanged`. The mode decides the camera and the weapon. The drive states set `vehicle` and the weapon states `weapon` in their own code whatever this says. |
| `motion_mode` | string or null | How the body moves while in it: `MoveMode`, `JumpMode`, `AttackMode`, `StrafeMode`, `DriveMode`, `DriveCompanionMode` (the slowing roll while unfolding out of the car), `ClimbMode`, `DashMode`, `FlyMode`, `HoverMode`, `FirstMode` (standing: the clip moves him), and the rest of `docs/bumblebee-mechanics.md` "Modes". A hash names one the executable's list does not have. |
| `finish_anim` | boolean | It may not be left until its clip has finished. |
| `play_anim_once` | boolean | It is done when its clip has finished. |
| `finish_on_set_exit` | boolean | It may be left, and is done, only once the animation has left its set. |
| `finish_on_special_set_exit` | boolean | The same for the special's set. |
| `allow_fall` | boolean | Gravity acts while in it, though its motion mode is `FirstMode`. |
| `ranged` | boolean | An attack state that works the weapon instead of playing a clip. |
| `fire_button` | string or null | With `ranged`: the button it watches. |
| `can_go_to` | array of strings | What damage may force on it: `STATE_CAN_GO_TO_DEATH`, `..._HIT_REACT`, `..._HIT_REACT_OVERLAY`, `..._STUN`, `..._GRABBED`. A hit or death that the list lacks does not interrupt the state. |
| `transitions` | array | The ways out, in the game's order: each has `connection` (a name in `connections`), `to` (a state, or null for "back to the state under this overlay") and `priority`. |

#### `control.connections`

| Key | Type | What it is |
| --- | --- | --- |
| `name` | string | The connection (`connectionJump`, `connectionListDrive`...). |
| `test` | string | `detector`, `ground_distance`, `time_in_state`, `environment`, `message`, `all` or `none` (below). |
| `checks` | object | *Not on `all`.* What every connection also checks (below). |

What each test adds:

| `test` | Keys | Passes when |
| --- | --- | --- |
| `detector` | `stages`, `stays_on_seconds`, `hold_with_last_stage` | Its detector is on: the pad has done what the stages ask, in order (below). |
| `ground_distance` | `limit`, `above` | His height over the ground (0 standing) is over `limit` with `above`, under it without. |
| `time_in_state` | `seconds` | He has been in the state at least that long. |
| `environment` | `environment` | The world test of that name passes: `Climbable` (a climbable wall ahead, `docs/climbing.md`), `CanSwitchAvatar`, `InsideFlightBoundary`, `NotHoldVIP`. The others are hashes; the connection's name says what they test (`connectionEnvFlippable`). |
| `message` | `message` | The character got that message this update: from a mission (`PlayCustomAnim1`), from the climb (its exits) or from the game's code. A hash names a message the data gives no name; the connection's name says what it is (`#c51a4425` is `connectionMessageHoverForced`'s). |
| `all` | `connections` | Every connection listed passes. |
| `none` | | Only the checks. |

`checks`, all of which must hold:

| Key | Holds when |
| --- | --- |
| `in_air`, `on_ground` | When true: he is off the ground, or on it. |
| `holding`, `not_holding` | When true: he carries something (a human), or does not. |
| `min_z_velocity`, `max_z_velocity` | His vertical speed is strictly between them. null is no limit. |
| `branch_begin`, `branch_end` | The playing clip's branch count is at least both. The count is 1 when a clip starts, goes up by one at each `branch_point` event, and is 100 once the clip has finished: so 100 means "the clip has finished". |
| `attack_can_branch` | When true: the attack button was pressed while an `attack_button_window` was open, and the window has closed. This is the melee chain. |
| `can_adv_transform` | When true: 0.25 s have passed since the change into the car (`advTransformDelay`): the attacks out of the car. |

`stages` of a detector, each with:

| Key | What it is |
| --- | --- |
| `after_seconds` | Seconds since the stage before (or since the detector started) before this stage may pass. |
| `held_seconds` | Seconds the `held` buttons must have been down together. |
| `held`, `not_held` | Buttons that must be down, and must not be. |
| `pressed` | A button that must have gone down this update, or null. |
| `stick` | null for any stick. Else the left stick's length must be within `min` and `max` (a full push is 1) and its angle within `angle_min_degrees` and `angle_max_degrees`, measured `against` the `camera` or the `character`'s facing: 0 straight ahead, positive to the left. The angle is taken between -180 and 180; when `angle_max_degrees` is not negative and `angle_min_degrees` is positive, a negative angle has 360 added; when `angle_max_degrees` is negative, a positive angle has 360 taken off. |

#### Running the graph

Every update (`update_seconds`), in this order:

1. **Detectors.** Every connection on the current state's transitions steps its detector by the
   update's time. A connection that two transitions share is stepped once for each. A detector:
   the time since its stage started goes up; the time the stage's `held` buttons have been down
   together goes up while they are, and goes back to 0 when they are not. When the stage passes,
   `held_seconds` have been held and `after_seconds` have gone by, both times are cut by those
   amounts and the next stage starts. After the last stage the detector is on, for
   `stays_on_seconds` (100000 is for good) and back to its first stage; with
   `hold_with_last_stage` it is on instead for as long as the last stage still passes. A detector
   with no stages is on at once.
2. **The transition.** Of the current state's transitions whose connection passes and whose
   target may be entered now, the one with the HIGHEST `priority` wins, the first listed on a
   tie. Only transitions to a state that is not an overlay count here, and one with `to` null
   counts only when the current state is itself an overlay. "May be entered" is the target's
   class: a dodge while its cooldown runs, or the car while it is disabled by a hit, may not.
3. **Changing state.** The old state is left: every detector on its transitions goes back to its
   start, and its overlay ends. The new one is entered (its clip, its motion mode, its
   `robot_form`, its time in state 0) and its update runs in the same update.
4. **Overlays.** When no transition won and no overlay is running, the best transition into an
   overlay state is found the same way. That overlay starts at the start of the NEXT update and
   runs on top. An overlay runs its own transitions: one that wins with `to` null, or back to the
   state under it, ends it; with none winning it may start an overlay of its own (the special on
   top of the firing), or ends when it is done. Leaving an overlay resets the connection that
   started it, so a press that stays on for good has to be made again.
5. **Done.** A state that is not an overlay, with no transition winning, that is done (a
   `StateBasic` with `play_anim_once` whose clip has finished, a hit reaction that has played
   out) goes to `start_state`.

The motion mode then moves the body, in the same update. A state may not be left while it says
so (`finish_anim`, `finish_on_set_exit`, a jump for its first 0.1 s): its transitions wait.

#### State classes

What each `class` does beyond its data, as read from the game (`docs/state-machine.md`):

| Class | On entering, and while in it |
| --- | --- |
| `StateIdle` | Standing: `IdleStandSet`, `FirstMode`. His vertical speed is zeroed. |
| `StateMove` | Walking and running: `WalkSet`, `MoveMode`. His vertical speed is zeroed. |
| `StateJump` | `JumpMode`, then `JumpSet`. May not be left for 0.1 s. After a climb's jump-up it catches a ledge. |
| `StateFall` | `FallSet`, then `JumpMode`, with no launch. |
| `StateDash` | The dodge: `DashSet`, whose rules pick the clip by the stick against his aim (within 45 degrees `DodgeFront`, 45 to 135 left `DodgeLeft`, right `DodgeRight`, else `DodgeBack`); `DashMode` keeps his facing. Not entered for `groundDashCooldown` (0.5 s) after one. Puts the gun away on leaving unless the next state is a `StateWeapon`. |
| `StateAttack` | Melee: its clip from `CombatSet` (with no clip, `CombatSet`'s rules pick one) and its motion mode (`AttackMode`, which slides him toward and turns him to a target by the clip's `attack` numbers and `attack_face_and_slide` events; `docs/melee.md`). With `ranged`, it fires the weapon on `fire_button` instead. |
| `StateAttackSpecial` | The special (`docs/character-specials.md`). With no clip, `SpecialAnimSet`'s rules pick one. As an overlay it is done after one update. |
| `StateWeapon` | Weapon mode: its motion mode first (`StrafeMode`), then `WeaponSet`; the control mode becomes `weapon` and the gun comes out, unless he comes from a `StateDash` or another `StateWeapon` (`docs/weapon-mode.md`). |
| `StateDrive` | The car: a `DriveSet` clip and `DriveMode` (`docs/driving.md`). During the first part of the change, on the ground with the stick well off his motion, `MoveMode` until it lines up. Not entered while a hit has disabled the vehicle. |
| `StateBasic` | Plays its clip (or its set's pick), then its motion mode. The ways out of the car, the climb's pull-up, the hover's start, the scripted clips. |
| `StateClimb` | `ClimbMode` (`docs/climbing.md`). |
| `StateFly`, `StateFlyExit` | Flight: `FlySet` whatever the data names (`docs/flight-states.md`). |
| `StateHitReact`, `StateStun`, `StateDeath` | Forced by damage when the current state's `can_go_to` allows it (`docs/damage.md`). |
| `StateInteract` | Picking up a human and flipping a car: mission interactions, not ported. |

### `animation_rules`

Within an animation set, which clip plays and which follows it is data too: each set has entry
rules (the clip it starts on, by where it comes from) and each clip has exit rules (when it hands
over, and to what). A control state that names a clip plays it; one that names only a set, and
every clip that finishes or branches, goes through these rules. Walking to running, the landings,
the idle's fidgets, which dodge and which hit reaction plays, and most of weapon mode are decided
here. They are the transitions of the host's animation graph inside each state.
`docs/animation-rules.md` has the research; `core/src/animrules.rs` runs them.

| Key | Type | What it is |
| --- | --- | --- |
| `default_crossfade` | number | 0.1: the blend, in seconds, for a clip whose own `crossfade` is negative (none given). |
| `sets` | array | Every set of the robot (below), by name in alphabetical order. |

Each set:

| Key | Type | What it is |
| --- | --- | --- |
| `set` | string | Its name (`WalkSet`, `IdleStandSet`, `WeaponSet`...): the part of a clip's name before the dot. |
| `entries` | array | Its entry rules, in the order they are tried (below). |
| `clips` | array | Each of its clips that the robot has, with its exit rules (below), in stored order. |

An entry rule:

| Key | Type | What it is |
| --- | --- | --- |
| `from_set`, `from_clip_id` | string or null | It fits only when the set and the clip being left are these. null fits any. |
| `base_set`, `base_clip_id` | string or null | For a set played on a layer over the base (the partial sets): it fits only while the base layer plays these. null fits any. |
| `rules` | array | Comparisons that must all hold (below). |
| `to_set` | string | The set it leads into: this set, or another. `emptyAnimSet` stops the layer. |
| `to_clip` | string or null | The clip it starts, by its name in `clips`. null with `to_clip_id` null: walk `to_set`'s entries in turn. null with `to_clip_id` set: this robot lacks that clip. |
| `to_clip_id` | string or null | The clip's id as the rule names it. |

A clip, with what decides how it hands over:

| Key | Type | What it is |
| --- | --- | --- |
| `clip` | string | Its name in `clips` (its `looping`, `speed`, `crossfade` and events are there). |
| `keeps_time` | boolean | It starts at the time the clip before it had reached, not at 0 (the weapon set's strafing runs, `Weapon_PrevRun*`, keep the stride as the direction changes). |
| `continue_time` | boolean | The time it runs past its end is carried into the clip that follows it. |
| `exit_events` | boolean | Its events still run while it fades out. |
| `exit_crossfade` | number or null | The blend, in seconds, into the clip that follows it, in place of that clip's own. null: none. |
| `exit_crossfade_to` | string or null | With `exit_crossfade`: only into this clip id. null: into any. |
| `exits` | array | Its exit rules, in the order they are tried: each has `to_set`, `to_clip` and `to_clip_id` as an entry rule's, `finish`, `branch_point` and `rules`. |

An exit rule may be taken when `finish` is true and the clip has finished, or when `finish` is
false and the clip's branch count has reached `branch_point` (the count is 1 as the clip starts,
one more at each `branch_point` event, 100 once it has finished; so 0 is "at any time"), and
every one of its `rules` holds.

A rule:

| Key | Type | What it is |
| --- | --- | --- |
| `variable` | string | What it compares (below). |
| `compare` | string | `==`, `!=`, `>`, `>=`, `<` or `<=`: the variable on the left. |
| `value` | number, string or null | What it is compared with: a number for an `F_` or `I_` variable; a name for a `C_` one, null standing for "none" (the hash 0). A variable of no known kind gives an object with all three: `float`, `int`, `hash`. |

The variables, as the game works them out (`docs/animation-rules.md`, "Variables"):

| Variable | What it is |
| --- | --- |
| `F_L_STICK_AMP` | How far the move stick is pushed, 0 to 1 (0.01 is "moving", 0.7 "running"). |
| `F_L_STICK_DIR` | Degrees between where the stick points and where he aims. |
| `F_R_STICK_AMP`, `F_R_STICK_DIR` | The same for the camera stick. |
| `F_STRAFE_ANG`, `I_IS_STRAFING` | Set by weapon mode's strafing: the angle he moves at against his aim, and 1 while strafing. |
| `F_Z_VEL` | His vertical speed. |
| `F_FALL_DUR` | Seconds he has been falling. |
| `F_TIME_IN_ANIM` | Seconds the clip has played, counting on round its loops. |
| `F_TIME_SINCE_DAMAGED` | Seconds since he was last hurt. |
| `F_HP_PERCENT` | Health over its maximum. |
| `F_STUN_TIME` | The stun clock. |
| `F_RANDOM` | A fresh random number from 0 to 100 each time it is read. |
| `I_IN_AIR`, `I_ON_GROUND` | 1 off the ground, or on it. |
| `I_CHAR_FORM`, `I_PREV_CHAR_FORM` | The control mode, and the one before (1 robot, 2 weapon, 3 vehicle). |
| `I_WEAPON_INDEX` | The weapon in hand (0 none). |
| `I_SPECIAL_ON`, `I_SPECIAL_IDLE_ON` | 1 while the special runs; the idle's special variant. |
| `I_IS_HOVERING` | 1 while hovering. |
| `C_JUMP_TYPE` | The kind of jump being made (`LONG_JUMP`...). |
| `C_DAMAGE_TYPE` | The kind of hit or death to react to (`ANIM_DEATH_EXPLODE`...). |
| `C_WEAPON_ACTION`, `C_WEAPON_EQUIP` | What the weapon is doing (`WEAPON_ACTION_OVERHEAT`, `..._RECOIL`...). |
| `C_INTERACT_TYPE`, `C_INTERACT_ACTION_TYPE`, `C_CHAR_MESSAGE`, `C_VEHICLE_STUNT` | Carrying, interactions, messages, stunts. |

A variable is worked out when a rule first asks for it in an update, unless the game's code has
set it (the modes set `I_IS_STRAFING`, `F_STRAFE_ANG`, `I_IS_HOVERING`, `C_JUMP_TYPE`): then it
keeps what was set. A variable nothing answers compares as 0.

#### Running the rules

Every update, for each layer playing a clip (the base layer, and the layers over it that the
partial sets play on), after the clip has moved on by the update's time:

1. **Exit.** Its `exits` are tried in order; the first that may be taken wins. With a `to_clip`,
   that clip starts. With none, `to_set`'s entries are walked, coming from this clip.
2. **Entry walk.** A set's `entries` are tried in order; the first that fits (its `from_`
   filters match the set and clip being left, its `base_` filters what the base layer plays, and
   its `rules` hold) wins. With a `to_clip`, that clip starts; with none, `to_set`'s entries are
   walked in turn; `emptyAnimSet` stops the layer. Nothing fits: nothing changes.
3. **Starting a clip.** It blends in over its own `crossfade` (`default_crossfade` when that is
   negative), or over the `exit_crossfade` of the clip it replaces when that names it or any
   clip. It starts at 0, or where the clip before had reached with `keeps_time`, or at the time
   the clip before ran past its end with that clip's `continue_time`.

A control state that names a set and no clip starts the set by an entry walk from the clip that
was playing (`control`'s "State classes").

### `mechanics`

Each mechanic's numbers for this robot, as the reference uses them. The formulas they go into are
in [MECHANICS.md](MECHANICS.md). So far on foot, the jump, the car, flight, hover and climbing. Speeds are metres a second, times
seconds, turn speeds degrees a second; a value that does not apply is null.

`constants`: the game's own numbers, the same for every robot.

| Key | What it is |
| --- | --- |
| `update_seconds` | The game's update, 0.032. |
| `stick_dead_squared` | The stick counts as centred at or below this squared length. |
| `full_stick_squared` | Past this squared length a running jump takes off at exactly `max_speed`. |
| `move_air_control` | The share of the gap to the clips' pace that moving closes each update off the ground. |
| `vehicle_exit_slow_squared` | Out of the car at or below this speed squared: the high standing jump. |
| `fly_exit_long_squared` | Out of flight at or above this speed squared: the long jump. |
| `climb_jump_push` | The push along his facing of a jump off a wall. |
| `jump_leave_seconds` | The least time in the jump before the state may leave it. |
| `drive_line_up` | Changing into the car, he keeps turning on foot until the stick's direction against his motion has at least this cosine. |
| `unfold_run_stick` | Out of the car, a stick longer than this unfolds him into a run. |
| `car_air_push` | How hard the stick pushes the car with no wheel down. |
| `fly_drop_ray` | The length of a flyer's ray down to the ground. |
| `fly_wall_ray`, `fly_side_rays` | The length of a flyer's three rays along its nose, and the two side rays' starts in the body's frame. |
| `climb_stick_dead_squared` | On a wall the stick counts as centred at or below this squared length. |
| `climb_hand_swap_at` | Climbing up or down, the leading hand changes as the clip passes this share of its length. |
| `climb_default_crossfade` | The blend time, in seconds, of a climb clip that has none. |
| `ledge_catch_reach_radii`, `ledge_catch_below`, `ledge_catch_least_drop`, `ledge_catch_facing` | The ledge catch after the jump up a wall: how many radii past the top it looks, how far above his feet it may put him, the least drop that counts, and how square to the top's facing he must be. |

`on_foot`:

| Key | What it is |
| --- | --- |
| `turn_speed_degrees` | How fast moving turns him toward the stick (`modeMove.turnSpeed`). |
| `max_speed` | The running jump's take-off speed at full stick (`modeMove.maxSpeed`). |
| `walk_run_speed` | The speed a landing into moving is capped at (`modeMove.walkRunSpeed`). |
| `move_stick`, `run_stick` | Stick lengths past which he walks and runs (`animRuleMovement`, `animRuleMovementRun`). `run_stick` is null when he has no run. |
| `walk_speed`, `run_speed` | The walk and run clips' root speed. |
| `idle_blend`, `walk_blend`, `run_blend` | Seconds the idle, walk and run clips take to blend in. |
| `stop_clip`, `land_run_clip`, `big_land_run_clip` | The one-off clips that set the pace: run to idle and the two landings into a run, by `clips` name. |
| `long_fall_seconds` | Falling this long gives the big landing (`animRuleLongFall`). |

`jump`:

| Key | What it is |
| --- | --- |
| `gravity` | Gravity, metres a second squared (`Character.gravity`). |
| `types` | One entry per jump type (below). |
| `takeoff_clip`, `in_air_speed` | The take-off clip whose root pushes a standing or moving jump, and the in-air clip's root speed after it. |
| `climb_up_height` | `climbJumpUpHeight`. Not used: every jump off a wall takes the `climb` type's height. |
| `ground_punch_acceleration`, `ground_punch_speed_limit` | The ground punch's extra pull, per update, and the most gravity and pull reach. |
| `weapon_dilation_start`, `weapon_dilation`, `weapon_dilation_rate` | The slow motion while aiming in the air. |
| `strafe_speed` | Weapon mode's speed (`modeStrafe.strafeSpeed`): a jump from it takes off at this, a landing into it is capped at it. |

Each of `types`:

| Key | What it is |
| --- | --- |
| `type` | `standing`, `moving`, `high_standing`, `long`, `fall`, `climb`. |
| `jump_type` | The value written to the animation variable `C_JUMP_TYPE` (hex hash). `fall` shares the standing jump's. |
| `height`, `launch_speed` | How high it goes and the upward speed that does it; null for `fall`, which does not launch. |
| `speed`, `control`, `turn_speed_degrees` | What the stick asks for in the air, the share of the gap closed each update, and the turn toward the stick. |

`car`: the robot's `modeDrive` block and vehicle body, null for a robot that cannot drive. Each
key is the game's field of the same meaning (`max_speed` is `maxSpeed`); MECHANICS.md, "The car",
says where each goes.

| Key | What it is |
| --- | --- |
| `max_speed`, `trigger_filter_strength` | The top speed at full trigger, and how fast the trigger's filter follows it. |
| `throttle`, `engine_torque` | The engine's throttle and the torque each wheel gets per unit of it. |
| `turn_speed`, `max_turn_speed` | The front wheels' steering rate in radians a second at top speed and at a standstill. |
| `max_turbo_speed`, `max_turbo_time`, `max_turbo_cooldown_time` | The turbo's cap, how long it runs and its cooldown. |
| `turbo_time_upgrades`, `turbo_cooldown_upgrades` | The multipliers of the turbo's time and cooldown at upgrade levels 1, 2 and 3. |
| `turbo_throttle_boost`, `post_turbo_deceleration` | The throttle's multiplier in turbo, and how fast the cap comes back down after it. |
| `braking_deceleration`, `min_reverse_brake_time`, `max_reverse_speed`, `post_reverse_throttle` | Braking, how long the brake is held at a stop before reversing, the reverse cap, and the throttle when rolling backward with no brake. |
| `skid_out_deceleration`, `wheel_drift_factor`, `drift_steering_factor` | The slide: its slowing, and the tyres' sideways and forward grip while it lasts. |
| `min_drift_time`, `min_drift_turbo_speed`, `drift_turbo_time`, `drift_turbo_boost_factor`, `max_drift_turbo_speed` | The drift turbo: the slide that arms it, how long it runs, its throttle multiplier and its cap. |
| `bullet_slow_speed_percent` | The caps' scale while slowed by bullets. |
| `front_axle_length`, `rear_axle_length`, `long_axle_length` | Where the wheels hang: the two axles' widths and the distance between them. |
| `front_wheel_radius`, `rear_wheel_radius`, `wheel_friction` | The tyres. |
| `suspension_length`, `suspension_k`, `suspension_damp`, `suspension_wt_scale` | The springs, and how much the rest length moves with weight transfer. |
| `mass`, `inertia`, `body_half_size`, `body_offset_z`, `avatar_height` | The body: mass, its inertia about x, y and z, the box's half size, its centre's height above his root, and the height the wheel rays work from. |
| `angular_vel_damp_x`, `angular_vel_damp_y`, `max_angular_vel` | The body's turning: the damping of pitch and roll each update, and the limits about x, y and z. |
| `collision_forward_dot` | A hit whose normal dotted with his forward is under this stops the wheels. |
| `exit_to_run_min_speed`, `exit_to_run_deceleration`, `exit_to_run_turn_speed_degrees` | Unfolding into a run: the speed it slows to, how fast, and the turn toward the stick. |
| `exit_to_idle_min_speed`, `exit_to_idle_deceleration`, `exit_to_idle_turn_speed_degrees` | The same, unfolding to a stop. |

`fly`: the robot's `modeFly` block, null for a robot that cannot fly; the AI's own numbers are
left out. Angles are degrees, turn rates radians a second, `*_lag` and `*_strength` rates a
second (MECHANICS.md, "Flight").

| Key | What it is |
| --- | --- |
| `helicopter`, `robot`, `can_do_stunts` | Which kind of flyer: a helicopter (and both robot flyers) steers by its own rules; `robot` is a robot that flies as itself; stunts are allowed. |
| `l_stick_deadzone` | The stick's dead zone. |
| `thrust`, `turbo_thrust`, `lateral_thrust`, `lift` | The pushes: along the nose (in turbo), across it (helicopters), and up or down (helicopters). |
| `max_speed`, `max_turbo_speed`, `max_turbo_time`, `max_turbo_cooldown_time`, `post_turbo_deceleration` | The caps and the turbo. |
| `brake_duration`, `brake_cooldown`, `brake_speed_scale`, `braking_speed` | The air brake: how long, its cooldown, the lowest share of `max_speed` it brakes to, and how fast. |
| `bullet_slow_speed_percent` | The caps' scale while slowed by bullets. |
| `turn_speed`, `max_turn_speed`, `quick_turn_speed`, `turn_lag`, `turn_restore_lag` | The yaw rate the stick asks for, the helicopters' limit on it, the tight turn's, and how fast the rate follows and comes back. |
| `pitch_lag_auto_level`, `pitch_strength`, `max_pitch`, `min_pitch` | The pitch: how fast it follows (jets, helicopters) and its limits. |
| `max_roll`, `max_roll_quick`, `roll_strength`, `quick_roll_lag` | The roll in a turn and in the tight turn, and how fast it follows. |
| `helicopter_turbo_turn_factor` | A helicopter's turbo push against its motion is multiplied by this. |
| `stunt_move_button_press_time`, `stunt_move_stick_threshold_x`, `stunt_move_stick_threshold_y` | How long the action button arms the stunts, and the stick it takes to roll or loop. |
| `extent_roll`, `extent_pitch` | The body's width and length, taken off its height over the ground as it rolls and pitches. |

`hover`: the robot's `modeHover` block, null for a robot that cannot fly (MECHANICS.md, "Hover").

| Key | What it is |
| --- | --- |
| `max_hover_time` | Seconds before the hover ends; -1 is no limit. |
| `move_acceleration`, `max_move_speed`, `hover_deceleration` | The stick's push, the speed limit, and what the velocity keeps each update with the stick centred. |
| `max_lean_degrees`, `lean_strength`, `hover_done_particles_pct` | The lean drawn and the effect; not used for movement. |

`climb`: the climbable test's sizes, the robot's `modeClimb` block and the climb clips, null for a
robot whose pack has no `ClimbSet` (MECHANICS.md, "Climbing"). Each clip is named as in `clips`,
or null when the pack lacks it.

| Key | What it is |
| --- | --- |
| `radius`, `height` | The robot form's radius and height: he hangs `radius` off the wall, and the ledge catch uses both. |
| `body_offset` | How far above his feet the climbable test starts, `height * 24/121`; he lets go at the bottom by it. |
| `reach` | How far along his facing the test looks, `(radius + 2) * sqrt 2`. |
| `min_height` | The least height, top less bottom, of a wall to climb (`modeJump.jumpHeight`). |
| `pull_up_dist` | He goes over the top within this of it. |
| `edge_limit_dist` | He stops shimmying, and is moved in on catching hold, this close to a side. |
| `arrival_deceleration` | How fast the speed he catches hold with dies away. |
| `catch_clip`, `catch_clip_ground` | The clip he catches hold with from the air and from the ground (the climb set's entry rules). |
| `clips` | `idle`, `climb_up`, `climb_down`, `shim_left`, `shim_right`, `pull_up`: each `[left hand, right hand]`. |
| `jump_up_clip`, `jump_off_clip` | The clips of the jump up the wall and the jump off it. |

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

## Reference runs

`rotf-export --character <name> --run <script.json> [--out <file>]` plays a script of inputs
through the reference (`tf2-core`, the same code the Bevy game runs) and writes `<pack>_run.jsonl`:
a header line, then one line of JSON per update with where the robot is and what he is doing. A
native port plays the same script through its own controller and compares its numbers with these,
update by update. `export/runs/` has example scripts.

### The script

| Key | Type | What it is |
| --- | --- | --- |
| `format_version` | integer | 1. A script of another version is refused. |
| `start` | object | Optional. `pos` (three numbers, metres) and `yaw_degrees` where he starts; the origin, facing +y, if left out. |
| `arena` | object | Optional. `boxes`, each with `min` and `max` (three numbers), and `targets`, each with `pos`: inert dummies his size, with no health limit, for the attacks to hit. The ground is the flat plane at height 0. |
| `steps` | list | The input, in order. |

Each step holds its input for `updates` updates (1 if left out). Every key is optional:

| Key | Type | What it is |
| --- | --- | --- |
| `updates` | integer | How many updates the step lasts. |
| `stick` | two numbers | The move stick in world space, x and y, length up to 1. |
| `steer` | number | The stick's left and right as the pad gives it, which steers the car. Defaults to `stick`'s x. |
| `pad_stick` | two numbers | The stick as the pad gives it, which climbing reads. Defaults to `stick`. |
| `look` | two numbers | The camera stick, which the helicopter turns and climbs by. |
| `vehicle` | number | The vehicle trigger, 0 to 1: held, the car or flying form and the throttle. |
| `brake` | number | The brake, 0 to 1. |
| `drift` | boolean | The slide button held. |
| `aim` | two numbers | Weapon mode: the direction he aims along the ground. Left out, he is not aiming. |
| `hold` | list | Buttons held through the step, by `control.buttons` names. |
| `press` | list | Buttons pressed: down through the step, and going down on its first update only. |

The buttons are the game's, not the pad's. One pad button can raise several: the melee button
raises `AttackFast`, `Dodge` and `AttackVehicle` together, as the game's default binding does, so a
melee tap is `"press": ["AttackFast", "Dodge", "AttackVehicle"]`. `Jump` and `VehicleTurbo` go in
as the reference's own jump and turbo inputs.

### The header line

| Key | Type | What it is |
| --- | --- | --- |
| `format_version` | integer | As above. |
| `character`, `pack`, `attributes` | string | Who ran: the display name, the pack and the `characterAttributes` block. |
| `update_seconds` | number | 0.032, the game's update. |
| `updates` | integer | How many lines follow. |
| `missing` | list | Values the reference looked for in the game data and did not find. Devastator's names the sets and clips he does not have (no car, jump, dash or climb); the other robots' is empty. |

### An update line

| Key | Type | What it is |
| --- | --- | --- |
| `update` | integer | 0 for the first update. |
| `time` | number | Seconds at the end of this update. |
| `pos`, `vel` | three numbers | His place (metres) and velocity (metres a second). |
| `yaw_degrees`, `pitch_degrees`, `roll_degrees` | number | Which way he faces (0 is +y, positive turns left), and the body's pitch and roll. |
| `speed` | number | Forward speed along his facing on the ground, metres a second; negative when the car reverses. |
| `mode` | string | The motion mode moving him: `Stand`, `Move`, `Strafe`, `Jump(<kind>)`, `Drive`, `Fly`, `Unfold`, `Hover`, `Climb`, `Action`. |
| `gait` | string | What the stick asks for on the ground: `Idle`, `Walk`, `Run`. |
| `vehicle` | boolean | In the vehicle form. |
| `health` | number | His health. |
| `state`, `overlay` | string or null | The control state and the overlay state, by `control.states` names. |
| `clip` | object or null | The clip that decides his pose: `layer` (`action` when a control state, climb, death or weapon clip plays it, `base` when the set's own rules do), `set`, `id`, `tick` (1/60 s), and for `base` the `blend` into it. |
| `triggers`, `untriggers` | list | Trigger and Untrigger messages his clips sent this update, by the name of what they start or stop (the effects of that name on him). |
| `shakes` | list | Camera shakes started this update, by name. |
| `rumbles` | list | Pad rumbles started: `kind`, `duration` (seconds) and `strength`. |
| `hits` | list | Hits his attacks landed: `target` (the arena target's number, from 1), `damage`, `point`, `body` (the collision body that hit), `knock_back_speed`, `knock_back_angle_degrees`, `flags`. |

Numbers are written as the shortest decimal that gives back the same 32-bit float.

What a run does not cover yet: the guns and their aim (the weapons are run by the Bevy game around
the reference), the cameras, Devastator's vortex and pods, levels, and anything fighting back.

## Not in the export

The game has these, and the export does not. `not_exported` lists them too.

- The voice lines (the game picks them in code).
- The particle effects, ribbons and lights: only their names at their events.
- The clip fields the game reads but the port does not (`chance`, `minWalkSpd` and `maxWalkSpd`,
  `minStrafeAng` and `maxStrafeAng`, `walkFlags`, `group_type`): how they pick between clips is
  not read.
- What each state class does in code beyond its data. `control`'s "State classes" sums it up;
  `docs/state-machine.md` and `core/src/sim.rs` have the detail.
- What the game moves in code: flight's surfaces, the wheels' spin, the suspension.

## Changes

- **1** (2026-10-26): the first version. It describes what rotf-export 0.1.0 writes.
- **1**, added (2026-10-26): `collision`, and a `<node>.collision.<n>` node in the `.glb` for
  each body. Only new keys, so the version stays 1.
- **1**, added (2026-10-26): `unkeyed_node_names` on vehicle clips.
- **1**, added (2026-10-26): `control`, the control state machine: states, transitions,
  connections and the button names, with the rules for running it.
- **1**, added (2026-10-26): `animation_rules`, each set's entry rules and each clip's exit
  rules, with the variables they compare and the rules for running them.
- **1**, added (2026-10-26): `mechanics`, on foot, the jump, the car, flight and hover, with the formulas in
  MECHANICS.md.
- **1**, added (2026-10-26): reference runs, `--run` and `<pack>_run.jsonl`, with the script
  that drives them.
- **1**, added (2026-10-27): `mechanics.climb` and the climb constants, with MECHANICS.md
  "Climbing".
