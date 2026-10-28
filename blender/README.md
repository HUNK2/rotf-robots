# RotF robot import, a Blender add-on

Imports a robot written by `rotf-export` into Blender, with what the robot's JSON says and glTF
cannot hold, so it is ready to animate, render or re-export. Blender 4.4 or newer (checked on 4.5
and 5.0).

## Install

Build the add-on's zip from this folder:

```
blender --command extension build --source-dir rotf_import
```

It writes `rotf_import-<version>.zip`; each release also has that zip on the Releases page.
(Zipping by hand works too, with `blender_manifest.toml` and `__init__.py` at the top of the
zip.) Then in Blender: Edit > Preferences > Get Extensions > the menu at the top right > Install
from Disk, and pick the zip.

## Use

File > Import > RotF robot (.json), and pick `exported/<pack>/<pack>.json` (not the
`_attributes.json`). The robot comes in standing in its idle; the vehicle, the weapons it puts
away and the projectiles come in hidden, each in its own collection under the robot's.

The **RotF** tab of the 3D view's sidebar plays a clip: pick the robot and the clip (type to
search, `robot/DriveSet.Trans_R2V_A`), then Play clip. It puts the clip's action on its model,
sets the scene's frame range to the clip and its events as timeline markers. On a change of form
it also runs the vehicle's clips at the ticks the clip's `model` events start them, and shows and
hides the robot and the vehicle as those events say, while the animation plays or renders.

## What the import does

- **60 fps.** The scene runs at 60 frames a second, so one frame is one clip tick, and every
  clip starts at frame 0.
- **Hidden meshes.** Meshes the game starts hidden (`render_set_hidden`, Optimus's
  special-attack parts) are hidden in the viewport and in renders.
- **Clips.** Each action has its clip's numbers as custom properties (`rotf_looping`,
  `rotf_speed`, `rotf_crossfade`, `rotf_additive`, `rotf_set`, `rotf_id`, `rotf_length_ticks`,
  `rotf_attack` and the events as JSON in `rotf_events`), its frame range (cyclic when the clip
  loops) and its events as pose markers (`footstep LEFT`, `sound_play <event>`, `attack_hit on
  Hand_R`...). Pose markers show in the Action editor with View > Show Pose Markers.
- **The vehicle's unkeyed nodes.** In the game a vehicle clip leaves the nodes it has no channel
  for where the clip before left them; glTF would put them back at rest, which folds Starscream's
  jet in `V_Idle`. The import keys those nodes (`unkeyed_node_names` in the JSON) with the pose
  the clip that leads into this one leaves them in, read from the robot clips' `model` events
  (`Trans_R2V_A`, then `Trans_R2V_B`, then `V_Idle`). Where no clip leads in, and where the game's
  channel keys nothing, they are keyed at rest. Each action lists them in `rotf_held_nodes` and
  `rotf_rest_nodes`. Every vehicle action then poses the whole vehicle on its own.
- **Weapons.** Each weapon's model hangs on its start node's `.frame` (Bumblebee's cannon on his
  right hand). Weapons of one list share a model, which hangs where the first of them does. A
  clip's `weapon_attach` events, which move weapons between nodes, are markers only.
- **Collision bodies.** Each body's empty gets a wire shape (sphere, box or cylinder) in a
  `collision` collection inside its model's: hit boxes blue, attack bodies red, other bodies
  yellow (viewport shading by object colour). The shapes do not render. The attack bodies are
  large on purpose: the game counts one only while a clip's `attack_hit` on its node is on.
- **Names.** Object names are unique in Blender. A node whose name another model already took
  (the robot and the vehicle both have `Flipper.collision.0`) gets its file's name in front:
  `bumblebee_vehicle:Flipper.collision.0`. Every node object keeps its glb name in the custom
  property `rotf_node`, and its file in `rotf_file`.

## Not done

- No particle effects, sounds or camera shakes: they are markers.
- Root motion stays out of the actions, as the glTF has it: the robot animates in place. The
  JSON's `root_travel` is the movement.
- The wheels' spin, steering and suspension, which the game moves in code, do not move.
- Additive clips (`.aim_down`, `.aim_up` and the other additive ones) are laid over the rest
  pose, as in the glTF, not over a clip below.

## Checking it

```
blender -b --factory-startup --python check_import.py -- ../exported/bumblebee/bumblebee.json
```

imports the robot without a window and checks the import against the JSON: the frame rate, the
hidden meshes, an action, markers and frame range for every clip, a shape for every body, every
node's rest place against the glb, the weapons on their nodes, the held vehicle nodes, and the
robot and vehicle shown and hidden during a change of form. It prints one line per check and
exits non-zero on a failure. Run it after changing the add-on or the exporter.
