rotf-robots: the robots of Transformers: Revenge of the Fallen (PC, 2009)
https://github.com/HUNK2/rotf-robots

YOU NEED YOUR OWN COPY OF THE PC GAME. Nothing from it is in this download: both programs read
the game's files from your install each time they run, and never change them.


PLAYING: bumblebee.exe
----------------------

Double-click bumblebee.exe. A normal install is found by itself (through the registry entry its
installer leaves). If yours is not found, the program asks for the folder the first time: pick the
one that holds bnxglobal.str. It is remembered. To choose again, run it with --pick-game-dir.

The first start of each robot takes a little longer: the game's textures are enlarged 4x and kept
in %LOCALAPPDATA%\Bumblebee\texture-cache (--original-textures turns that off).

Keyboard and mouse (modern layout; --classic-controls gives the original game's own keys):

  W A S D                move; steer the car
  mouse                  camera and aim (click the window first; Esc frees the mouse)
  Space                  jump; turbo in the car
  Left Shift, held       change into the vehicle and drive; let go to change back
  right mouse, held      weapon mode; the slide in the car
  left mouse             melee (the car's gun in the car); with the right mouse held, fire
  Left Ctrl              melee; the dodge in weapon mode
  mouse wheel, R         next weapon
  E                      action, climb
  Q                      special ability
  C                      camera distance
  Z                      strafe mode
  F1                     help panel        F2  robot menu      F6 / F7  next / previous robot
  F5                     reset

A pad uses the game's own bindings (R2 vehicle and fire, L2 weapon mode and slide, bottom face
button jump and turbo, left face button melee, right face button climb, top face button special).

Options (from a command prompt in this folder):

  bumblebee.exe --character optimus      start as another robot (F2 lists them)
  bumblebee.exe --scouts                 put three enemy scouts in the arena to fight
  bumblebee.exe --mission <LevelName>    stand on one of the game's levels instead of the arena
  bumblebee.exe --game-dir "D:\Games\Transformers Revenge of the Fallen"


MODDING: rotf-export.exe
------------------------

Writes a robot from your install as standard files for Blender, Unreal, Unity or Godot: glTF
models with skeletons and animations, PNG and DDS textures, WAV sounds, and a JSON file with
everything else (animation events, the change of form, weapons, sounds, the robot's numbers).

  rotf-export.exe --list
  rotf-export.exe --character bumblebee           into exported\bumblebee
  rotf-export.exe --all                           every robot (about 2 GB)

The exported files are made from the game and belong to its owners: keep them on your own machine.
The Blender add-on (rotf_import-*.zip on the release page) imports an exported robot ready to
animate. The JSON format and the porting guide are in the repository's docs folder.


HOW SURE IT IS
--------------

Bumblebee is the most complete: his movement, car, gearbox, cameras, climbing and strafing were
compared with recordings of the original. Firing, fighting, damage, effects, flight and the other
robots are checked only by tests and captures. This is not the whole game: no missions, menus,
saves or cutscenes.

Bugs and questions: https://github.com/HUNK2/rotf-robots/issues


LEGAL
-----

A fan project, not affiliated with or endorsed by Activision, Hasbro, Paramount, Luxoflux or
Beenox. Transformers and every character name are trademarks of their owners. The programs are
under the MIT License (LICENSE.txt); the libraries they are built from are listed in
THIRD-PARTY-NOTICES.html. You must own the game to use them.
