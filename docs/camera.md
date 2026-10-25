# Camera: research notes

## Complete Bumblebee behaviour audit (2026-10-07)

This section supersedes historical gaps below. Audited the follow, drive and weapon
vtables (00b65d28/00b65d88/00b65fe0), including inherited slots, constructors,
reset/start, target messages, settings loaders, collision and final view update.
Helicopter and sniper behaviours are separate classes; Bumblebee's installed data
has canFly=false and sniperCamera=0. They are outside this character's mechanic.

New findings, checked against narrow assembly where decompilation drops arithmetic:

- 009224f0/00922d10: recentre heads behind the requested facing, choosing the nearer
  current-facing heading in the same direction; pace PI*dt plus .1 of the remainder.
  Entering Climb requests recentre. R3 is CameraReset (global_tuning Controller).
- 0071a5c0: Climb=3efeac6d, weak-ground-punch-fall=88875297, death flag148 bit3
  requests7d110862 (007204a0 sets it). Owner248 nonzero requests5e89f579.
  007256e0 checks FlightMode, not dodge; Bumblebee cannot enter that mode.
- GroundPunch adds45deg to the automatic elevation target and stands off15m;
  wanted FOV=min(default*1.5,70). Death mode doubles extra-distance gain/rate.
  00922580 dead-message bit200 overrides Climb/GroundPunch and bypasses bit100's
  attack stand-off refresh. The character attack-controller +108 timer requests
  stand-off; camera +13c holds it for3.7s after those requests stop.
- 009145b0: airborne needs raw horizontal physics speed>5, no height blend,
  non-transition mode, minimum wheel50>2 (or>.1 while already airborne).
  Tilt=max(bodyTilt, max(oldElevation,bodyTilt)+.25*(velocityTilt-from));
  velocityTilt=atan2(-velocity.z,horizontalSpeed)-PI/8. Airborne retains user offset
  even at elevation limits. 007a1460 wheel50=clamp(hitDistance-2*avatarHeight-restLength,0,100).
- FOV springs to the previous request BEFORE heading computes the next request.
  009153f0 constructs an FOV-scaled temporary eye; 009145b0 rebuilds the normal car's
  final eye from unscaled distance, including its zero-change case. This resolves
  the omitted-factor issue above without fitting a compensating number.
- 0070abe0 uses holder centreZ-rootZ-wrapper54*holder.upZ, clamped>=0, plus form
  height. Holder a4 points to a WRAPPER, whose first word points to the physics body.
  00853a10/006e81e0 set vehicle wrapper54=avatarHeight*.5 (not cuboid half-extent).
  00853cb0 copies owner130 for the selected form. Driving floor is holder centreZ.
  Roll is atan2(-bodyRight.z,bodyRight.xy length), rather than an Euler roll.
- 00924310 overhead: actual previous target-to-eye direction.z>.83; target XY
  movement squared>.0001; blend=clamp((z-.83)*19.999996,0,1).
- 00923460 accepts pitch only inside user limits; default settling targets clamped
  default. The automatic offset is rewritten after final elevation clipping.
- 00923c30 uses sin(actualStickTurn)*cross, not sign(turn)*cross; threshold .001,
  half-degree still gate. Box corner selection is nearest of12 infinite edge LINES.
  00923c90 supports level and slanted upright edges (quadratic elevation solve),
  plus type6 axis/radius. Ordinary hits project contact onto the requested eye line,
  with no invented follow margin. Driving sweeps radius.65.
- 005be890/005d6930/005e24c0/005e3140: a convex sphere cast returns the OBSTACLE
  support/contact point and, separately, centre travel. The camera projects the
  obstacle point, using travel for the <.1 too-near test. It does not use sphere centre
  as the eye. The authored arena now queries actual faces, round edges and vertices.
- 00921f60: start inherits horizontal sponge; view-ray height uses horizontal
  distance magnitude (not dot projection), clamped[-20,20]; initial distance is not
  clamped. 00922750 retains pre-clamp sponge rate; minHeightGain is the active gain.
- 00927f60 inherits offsets but shared start cleared rates before weapon reads them.
  00928460 retains prior aim XY for almost-vertical input. 00928f40's one-update
  distance bias is cleared by00929280; it is not handed to a different follow object.
  Weapon defaults: maxRotSpd1080, aheadOffset1.5, elevGain.2 clamped[.001,1].
- 00913980's handover control fade is0 in first half, left/(duration/2) in second,
  then1. Follow freeze wakes on stick or horizontal target movement.
- 0091e930's third point drives a CAMERA COLLISION PROXY, not a sound listener:
  0091e360 creates0091fee0 with a radius1 CollisionSphere. 009202b0 places it at
  thirdPoint+nearClip*direction, clamped inside level bounds by1. 00920530 (raw)
  collects at most16 contact planes, excludes9c bit15 and fadable9e bit5.
  0091e690/00920590 adjust the follow eye for near-plane corners; driving uses144
  directly. Side extents tan(FOV)*near, vertical factor.75 or.5625; forward
  correction capped+-1.25, excess spills sideways/up. Do NOT change render FOV
  based solely on this conservative collision rectangle; projection needs its
  renderer consumer if conversion is in question.
- 0091ecb0/0091eb10: nonblocking9e bit5 objects fade to.45 at4/sec; missing objects
  recover at4/sec (same frame), retouch preserves alpha. Excludes proxy object.
- Shake renderer closure: raw0077a721..748 passes camera+4 and shake+64 into
  0052fbf0, then hands the composed affine to005a6f20 World::Draw. The basis is
  camera*shake, resolving the former own-axis assumption. Its translation is
  cameraEye+cameraBasis*storedShakeOffset: the world offset constructed from right/up
  in0091df50 is transformed again. Jolt::apply now preserves that behaviour.
- 00551d60 draws from bits((LCGstate>>8) XOR(time64Seconds<<8))&7fff00|3f800000,
  minus1; runtime reads Unix seconds per draw. Deterministic offline step injects0.
  [stand-in] Camera-local RNG stream: the original shares LCG state with other systems;
  the complete cross-system random-call order is outside this camera reconstruction.
- Shake loader0091f0f0 defaults: full/max distanceFLT_MAX, duration1, halfDuration.5,
  gains1, offsetAccel100; remaining fields0. These replace the old all-zero fallback.
  CameraShakeTrigger:00adb490 registers factory007d0f00;007d0e20 loads name44 and
  positional48(default true);007d0f50 arms byte38;007d0f60 calls the named starter
  at the scene node's world position or nonpositionally. No new shake algorithm.
  Damage00729120's shake branch requires sniping and is outside Bumblebee's settings.
- Mouse-option closure:008e8bb0 uses float32 sub/multiply and cvttss2si to write
  divisor=trunc((1.05-SETTING_AIM_MOUSE)*20). Options008e21a0 clamps the menu value
  to0..1;00753790 applies saved character settings.004f0e30 returns0 for negative
  directional counts,1 above the divisor, otherwise counts/divisor. The executable
  default is10, retained by the runtime; original options/save import is not added.
  At setting1 float truncation can yield0. Correction2026-10-09:004f0e60 resets a
  zero divisor to1 before querying mouse values; the normal device path therefore
  guards this endpoint. The converter alone lacks the guard. See docs/input.md.

- Original loader defaults are now used even for omitted optional fields: base FOV60,
  leash12..15, elevation5/15/45, stick180/180, sensitivity scales80/120, minHeightGain.02
  (00cad454), interp gains.05/.075, face gain.5; applicable gains clamp0..1.
  Driving min/max speed.5/10 (max at least min+2), align/user gain.02 and damp.3,
  maxSpdFOVScale75 (00cad464), extra distance5, tilt85/.25, lean50/.25.
- 00573f20 passes tan(FOV/2) and its viewport width/height multiple to0055ea60;
  the conservative proxy rectangle is separate. Base clip planes are.5/1500,
  now used by Bevy. The proxy's widescreen flag is configurable (`--camera-4-3`).
- Avatar transition: pencil type8 (006e8d20), cuboid type4 (005bbee0), virtual slot1.
  00854160 decrements owner248 by dt to0 and blends the avatar affine.
  Drive enter0085e130 sets owner3fc=fc2cb442, which exempts its avatar switches
  from that blend in00853cb0. No positive248 sample occurs in the expanded capture.
  The core accepts the Transition message/gate; do not invent an animation-based timer.
- 0071a460's optional vehicle-model target is rootXY and min(rootZ,holder bottomZ).
  Its caller0071a2e0 gates it on vehicle model attributes.canFly (00726710 field+a5;
  +a4 is canDrive). It is a flying-vehicle path, outside Bumblebee's canFly=false
  data. Bumblebee's camera target is his root. It is not an unported collision flag.

Runtime integration: Home/R3 sends recentre; Climb requests it automatically. The
settings-cycle restarts from the current view and supports shared/separate form levels.
CLI sensitivity, inversion and stick-disable feed the camera; the same inversions feed
weapon aim. Reset preserves options and places the initial view at the midpoint leash.
Raw physics velocity and minimum wheel clearance feed airborne tilt; capped velocity
feeds FOV and alignment. Target height/floor use the car's rotated holder offset.
Core Scene exposes fadeable collision objects without making them blocking or proxy
contacts. The Bevy scene adapter applies and restores each object's material alpha/mode.
`--camera-fade-test` is an optional [stand-in] nonphysical object for render verification;
no original level objects or flags are inferred for the authored arena.

Validation:320 core tests pass (`work/camera_core_tests_20261007.log`), including
recentre/climb, freeze/reset/options, ground-punch/death, airborne thresholds/velocity,
slanted edge solving, swept rounded corners, wedge proxy contacts, fade recovery,
death priority/stand-off expiry, clock mixing and final shake render composition.
Canonical build: `work/camera_game_build_final_20261007.log`.
Inspected captures in `work/shots/`: camera_audit_drive_20261007.png,
camera_audit_climb_20261007.png, camera_audit_weapon_20261007.png,
camera_audit_fade_plain_20261007.png (alpha.450) and
camera_audit_fade_opaque_20261007.png (alpha1.000). Fixture logs print opacity.
Final executable captures: camera_audit_drive_final_20261007.png (clear,14.7m/FOV64),
camera_audit_ground_punch_final_20261007.png (weak punch,15.5m/FOV59, view settling
after the fall). Two earlier punch-script attempts instead show AttackJump and a
post-landing Idle; those images are not evidence for the ground-punch mode itself.
Display check `work/camera_display_final_20261007.txt` passes60/120/144Hz and irregular
frames:0 held interpolated frames, camera-relative jump.0131..0570m.

Latest replay `work/camera_capture_complete_20261007.txt`:9,149 scored updates,
165 wall-collision updates excluded,57 error-based reseats at2m. Driving6,755:
eye mean.31m/median.17m/p95.90m/worst4.81m;view mean1.42deg/worst19.8deg;
FOV worst.8deg. Follow1,182:eye mean.29m/p951.02m; weapon1,212:mean.07m/p95.57m.
Compared with the previous same-export driving baseline.40m/p951.76m, this resolves
several real implementation gaps; it does not prove continuous/pixel parity.
`work/camera_capture_continuous_20261007.txt` disables ERROR-based reseating:
drive mean.33m/p95.90m/worst8.08m;view mean1.48deg/worst24.9deg. It still starts
fresh at settings/stretch changes and after excluded collisions; camera_run says so.
Run1 re-export/replay `work/camera_run1_complete_20261007.txt`:16,610 scored,
698 wall updates excluded,248 reseats. Driving3,507:mean.21m/p95.89m; follow11,955:
mean.30m/p951.46m; weapon1,148:mean.05m/p95.37m. Both exports now carry raw velocity,
holder height and message mode separately, beyond legacy44 columns. Replay also sends
the recorded R3/recentre requests (weapon d7 means swinging and is not treated as R3).
`work/camera_run1_continuous_20261007.txt` has no error reseats: driving mean.23m/
p95.93m/worst5.05m; follow mean1.01m/p959.84m/worst12.53m, view worst33.6deg.
The trace omits the independent character attack-controller +108 timer: replay leaves
fighting=false rather than feeding back the camera's own recorded +13c hold state.
The resulting missing10m stand-off around the Attack sequence is a replay input gap;
runtime feeds State::fight, and the original3.7s tail is regression-tested.

Limits: original level geometry is not supplied to the replay; trace reads can catch
partly written updates, and the older0x160 behaviour block omits driving spring state.
The fixed.032 camera/shake stepping remains the recorded-update convention; original
Main update has a smoothed variable frame duration. A still image cannot validate
shake motion, the clock/shared RNG sequence or clip-emitter placement. Original scene
camera overrides/triggers require level import; this audit establishes their shake
interface but does not fabricate that content. No guessed camera constants were added.

### Audit exclusions

All24 slots of each installed Bumblebee follow/drive/weapon behaviour, shared
start/reset/settings/message/collision/proxy/fade paths and final camera-manager aim
were covered. Adjacent look-aside, sniping, flying and spectator classes are outside
Bumblebee's installed settings. Generic physics solvers, allocation/reference helpers,
UI/billboard rendering and audio-filter code encountered during searches were read
only where they establish a camera interface. The ledger marks partial/outline reads
`unread` explicitly; this is not a claim that the whole Lux engine was read.

## Cheat Engine capture comparison (2026-10-08)

Source:`work/traces/ce_rotf_20261007_164337_49792.bin`,25,768 samples/408.531s.
Read-only CE capture; behavior0x1a0 present for every sample. User requested stop/analysis.
`work/vehicle_compare.py --camera-updates` collapses identical eye/forward/FOV states
because CE's16ms sampling defeats the legacy12ms heuristic; raw samples remain intact.
Survey limits now reflect observed block lengths instead of always claiming0x160.
11,050 exported states; current camera_run built and run twice.10,671 updates scored,
322 collision-flagged excluded. Bounded replay138 error reseats:eye mean driving.47m,
follow.44m,weapon.27m. RESYNC=0:driving.51m,follow.86m,weapon.25m;worst8.30/5.34/3.92m.
The latter still initializes settings/stretches and after wall exclusions; not fully
unseeded parity. Weapon view mean.02deg,Strafe eye mean.31m. Eight complete airborne
WeaponSet stretches add reference evidence beyond run1 but are not a movement replay.
FOV worst16.8deg driving at102.34s (turbo, near265ms capture gap at101.593s),19.8deg
follow at240.69s (GroundPunchSet landing into First,recorded hold3.67s,FOV64.84deg).
Attack-controller timer is still missing from independent replay inputs; no hold-value
feedback added. Need timing/independent-input investigation, not guessed tuning changes.
Distance cycle:259.859 far->normal,274.437 normal->very-near,275.687 very-near->far.
Outputs:`.camera_updates.csv`,`.compare_updates.json`,`.camera_replay.txt`,
`.camera_continuous.txt` beside source; full analysis `.analysis.md`.

## Expanded capture and distance control (2026-10-07)

User confirms D-pad Up changes distance. Offline export from
`work/traces/vehicle_20261007_152811.bin`:drive level1->0 at57.867s,then robot inherits0;
drive0->2 at96.147s,then robot and weapon inherit2. Capture uses the shared option.
Raw00924388..009243d1 computes `(old + signed request +3)%3`. `00766660` writes
all form indices when option bit8 is clear; otherwise only selected form. `00924310`
and`00915740` select new settings then call virtual start. Implemented D-pad Up/C
step-1 for all forms; `Camera::settings_changed` starts from the existing view and
eases to the new leash. The audit above adds the per-form option; original options/save
import remains missing.
C is an added keyboard binding. A no-cut/easing regression checks the core change.
Runtime capture`work/shots/capture_camera_cycle_20261007_v1.png` scripts Up at2s;
at4.5s the view is8.3m from him versus12.2m without the press. Inspected both.

`work/vehicle_compare.py` writes camera_run's44 columns plus minimum wheel+50 in44.
Before-change replay `work/camera_capture_before_20261007.txt`:6,755 driving updates,
eye mean.40m/median.20m/p95 1.76m/worst4.81m;view mean1.37deg/worst19.8deg.
Runner reseats errors above2m and excludes flagged collisions: NOT continuous full-run
parity. Original geometry is unavailable; memory can be sampled partly written.
Airborne elevation/tilt and collision were subsequently implemented in the audit above.

Behaviour capture ended at0x160, before drive direction,align rate,user elevation,
speed smoothing and airborne hysteresis. Recorder now reads0x1a0 for future captures.
Existing trace still supplies transforms,pitch/roll,capped velocity,wheel probes;
missing fields cannot be recovered. `007a3a60` is minimum wheel+50, not root altitude.
At this earlier stage no guessed airborne replacement was added. `009153f0` stores unscaled distance104
and applies an FOV ratio to its output; survey horizontal eye/internal distance at
turbo has median1.00000015. The audit above resolves the omitted factor by following
the elevation routine's final unscaled eye construction.

Addresses are for the analysed `Transformers2.exe` and stay in these notes (never in mashup code).

Status (2026-10-02): the follow camera (robot form) and the driving camera are read end to end
except the shared collision routine. The weapon camera is located, its settings mapped, its
code NOT read. Nothing in `bevy_test\` uses any of this yet.

Listings for every routine named here are in `work\asm\` (format explained at the end).

## Where it is

- Two scheduler tasks drive the camera. `FUN_0091d230` ("UpdateGameCamera", priority 13) calls
  slot 3 (update) of the active behaviour, `[DAT_00dae234 + 0x110]`. `FUN_0091d2c0` (priority 18)
  calls `FUN_0091df50`, which calls slot 4 (aim) and then runs the shakes.
- `DAT_00dae234` is the game camera object (0x18c bytes, built by `FUN_0091d8d0`, created in
  `FUN_0091d320`, which also loads `camera.lxb`). On it: `+0x04` its transform (three axis rows,
  the second is forward, then position at `+0x28`), `+0x94` field of view, `+0x128` its rate,
  `+0x120` and `+0x124` pitch and heading rates, `+0x138` look-at point, `+0x11c` flags.
- Time step: `FUN_007799f0` ("Main update") passes `ratio * (1/60) * game speed`. The ratio
  (`FUN_007797b0`) is the measured frame time in sixtieths, smoothed by 0.3 and kept within 0 to
  3. So the step is the real frame time, and every "per update" gain below runs faster at a
  higher frame rate in the original. A port should step the camera at a fixed rate; 60 per
  second matches the PC game with vertical sync, 30 the consoles. Which one the values were
  tuned at is not known.
- Behaviour classes, by function table:

| table | what | settings type | update (slot 3) | own slots |
|---|---|---|---|---|
| 00b65d28 | follow camera (robot form), 23 slots | 08e5a4ac | 00924310 | all below |
| 00b65d88 | driving camera, 24 slots | f88e590b | 00915740 | 1: 00914260, 16: 009143c0, 17: 00916030, 18: 00914c90, 19: 009153f0, 23: 009145b0 |
| 00b65de8 | driving with look-aside | 405de163 | 00915740 | 18: 00916610, 23: 009162d0, 12: 00913ae0 |
| 00b65f80 | follow variant | ? | 009277d0 | 17: 009276e0 |
| 00b65fe0 | weapon (aim) camera, 24 slots | 83767373 | 00929470 | 1: 00927f60, 17: 00928460, 18: 00928c60, 20: 00928f40 |
| 00b66040 | weapon with zoom | 446d9c5e | 00929d50 | 1: 00929c50, 2: 00929cb0 |

  Tables 00b65e58, 00b65e98, 00b65f00 and 00b65ed8 are another family (not looked at).
  `FUN_0091ee00` builds the settings objects by type; shakes are type e49598c6.

## Which settings a character uses

`characterAttributes<Name>` names them: `normalCamera`, `weaponCamera`, `drivingCamera`,
`sniperCamera`, and three look-at height offsets (`cameraRobotTargetHeightOffset`,
`cameraWeaponTargetHeightOffset`, `cameraDriveTargetHeightOffset`; Bumblebee 0, 0.8, 0).
Bumblebee's are `bbeeRobotnearCamera`, `bbeeWepshoulderCamera`, `bbeeDriveVeryNearCamera`.

Distance levels: each settings object's `next` names another; `FUN_0091ee00` numbers them along
that chain (the named one 0, then 1, 2). The options block (`FUN_007504b0()`, `DAT_00dab960`)
holds the wanted level per form at `+0x1f34 + form * 4`, all 0 by default, so a fresh game uses
the three named above. Bit 8 of the flags at `+0x1f30` makes the level per form instead of
shared. Other option bits read by the camera: bit 7 no stick input, bit 1 and bit 2 clear
negate yaw and pitch, and the float at `+0x1f40` is the sensitivity (0 to 1).

## Settings object (byte offsets; loaders in brackets)

`+0x0c` name, `+0x10` next, `+0x18` level, `+0x1c` defaultFOV (`0091f010`). Base (`00920fb0`):
`+0x20` maxDist, `+0x24` minDist, `+0x28` faceExtraDist, `+0x2c` faceExtraDistGain, `+0x30`
climbExtraDist, `+0x34` minElevation, `+0x38` maxElevation, `+0x3c` defaultElev, `+0x40`
userRotSpdZ, `+0x44` userRotSpdX (angles stored in radians), `+0x48` scaleRotSpdMin, `+0x4c`
scaleRotSpdMax (stored x 0.01), `+0x50` spongeHeight (at least 0.1), `+0x54` minHeightGain,
`+0x58` maxHeightGain (never read by the follow camera), `+0x5c` interpElevGain, `+0x60`
interpAngGain, `+0x64` interpTargetHeightTime, `+0x68` heightOffset.

Driving (`00913af0`) continues at `+0x6c` in this order: minSpdAlignMove, maxSpdAlignMove,
alignGain, alignDamp, maxSpdFOVScale, maxSpdDistAdd, (unnamed 568fb10e), alignFaceMaxSpeed,
alignFaceMinSpeed, alignAutoMinSpeed, sideOffset, userRotSclMaxSpeed, maxFaceAngOffs,
userRotGain, userRotDamp, tiltAmount, tiltGain, leanAmount, leanGain (`+0xb4`); then
(`00916070`) userLookAside, userLookAsideMinSpd, userLookAsideGain, holdControlExitTime (`+0xc4`).
Weapon (`00927cd0`): `+0x6c` maxRotSpd, sideOffset, aheadOffset, elevGain, minHeight, (unnamed
5297b970); then (`00929b20`) zoomFOV, zoomFullTime. Shake: `0091f0f0`.
`python tools\decomp_lookup.py fields <loader>` prints these as word numbers (multiply by 4).

## What the character hands the camera each update

`FUN_00922580` sends the target message ca291e26; the character answers in `FUN_0071a5c0` by
filling a block: the camera stick (x, y) and a second stick pair into what becomes `+0x6c..+0x78`
on the behaviour, a mode name (`Normal` 26296dd2 by default; 3efeac6d, 7d110862 and 88875297
are other modes whose names are not resolved), requests (recentre, step the distance level,
clear the freeze) and the look-at height: a per-form value on the movement owner
(`[owner + 0x160 + form * 4]`, taken to be the avatar's `height`, 5.4 robot and 1 vehicle for
Bumblebee; 5.4 confirmed 2026-10-03 through `+0x140`, see `climbing.md`) plus how far the body sits above the transform, plus the character's
height offset for the form. The camera then adds the settings' heightOffset.

## Behaviour object fields

`+0x04` camera, `+0x14` wanted field of view, `+0x18..+0x20` target offset (z is the look-at
height), `+0x24..+0x2c` look-at offset ("sponge"), `+0x38` its vertical rate, `+0x3c` target
velocity, `+0x48` last direction target-to-camera (valid when `+0xe3`), `+0x54` last target
position, `+0x60` minus that direction, `+0x6c`/`+0x70` stick, `+0x94/+0xa0/+0xac` a point and
two normals for the corner logic (`+0xdc`), `+0xd0` settings, `+0xd6` aim lag needs
initialising, `+0xd7` recentring, `+0xd8` pending level step, `+0xd9` returning to default
elevation, `+0xdb` ride along with the target, `+0xdd` shortened by an obstacle, `+0xe1` update
has run, `+0xe2` frozen until something moves, `+0xe4` recentre heading, `+0xe8` height-blend
time left, `+0xf4`/`+0xf8` pitch and yaw applied this update, `+0xfc` default elevation,
`+0x100` its spring rate, `+0x104` distance, `+0x108` distance spring step, `+0x10c`/`+0x110`
aim lag in pitch and heading, `+0x114` elevation E, `+0x118` automatic part A, `+0x11c` its
rate, `+0x120` extra distance, `+0x124` its rate, `+0x12c`/`+0x130` a timer and its length,
`+0x13c` face-extra hold time, `+0x140` mode, `+0x144` form (0 robot, 1 weapon, 2 vehicle),
`+0x148` weak pointer to the target.

## The follow camera, step by step

Shared pieces: `spring(diff, prev, gain) = prev + gain * diff - 2 * sqrt(gain) * prev`
(`FUN_00913900`; `prev` is the last update's change, so this is a critically damped spring run
once per update). Headings are `atan2(-x, y)`; `FUN_0056f290` wraps to plus or minus pi.

Starting (slot 1, `FUN_00921f60`, given the previous behaviour): distance and elevation are
taken from where the camera is now (`E = atan2(-forward.z, |forward.xy|)`), the sponge offset
is set so the look-at point starts on the current view ray (kept within 20), the height is
inherited from the previous behaviour when it had the same target (then blended over
interpTargetHeightTime), and the elevation is armed to drift to defaultElev (`+0xd9`). If the
previous behaviour returns a time from its slot 12 (holdControlExitTime on the look-aside
driving camera), the timer starts with it.

Update (`FUN_00924310`), every frame:

1. Level change if the option differs (`FUN_0091dc00`, then slot 1).
2. Field of view: `spring(wanted - current, last change, 0.02)`. Skipped while frozen.
3. Target point T = target position + (0, 0, height). A new height is reached linearly over
   interpTargetHeightTime. T's x and y are then replaced by the position of the object from
   `FUN_00923a70` (taken to be the physics holder: its velocity is read as a body's).
4. Frozen (`+0xe2`): if the stick is under 0.01 and T moved under 0.01, return.
5. Sponge (slot 16, `FUN_00922750`): `e = offset.z - T's rise this update`;
   `offset.z = e + 0.2 * (last change) - minHeightGain * e`, kept between `-height` and 20.
   Horizontal offset decays by 0.95 per update. Look-at point P = T + offset.
6. Direction D from P to the camera: heading from the two positions, pitch from the camera's
   own forward. If the last D pointed up more than 0.83 and T moved, the heading is blended
   back toward the last one (fully at 0.88), so the view does not spin when the target passes
   underneath.
7. Yaw (slot 18, `FUN_00922d10`): the offset P-to-camera is turned about the vertical by
   `fade * stick.x * userRotSpdZ * (scaleRotSpdMin + (scaleRotSpdMax - scaleRotSpdMin) *
   sensitivity) * dt`. `fade` is 1 with no timer; with one, 0 in its first half then
   `time left / half` (as read; odd, re-check before porting). Recentring turns by pi per
   second plus a tenth of the remainder until within one step, toward the target's heading
   (or the nearer of that and the requested one). In mode 3efeac6d the heading relative to the
   target's facing is held softly within 0.35 to 0.4 pi.
8. Distance (slot 19, `FUN_009228d0`). Extra distance: wanted `faceExtraDist` when the target
   faces the camera (`dot(target forward, offset) > 0`) or while a hold timer runs, 15 in modes
   3efeac6d and 7d110862, else 0; it moves by `spring(wanted - extra, last, faceExtraDistGain)`
   with its rate limited to 10 per second (gain doubled and 20 in mode 7d110862). Then
   `wanted distance = clamp(|offset|, minDist + extra, maxDist + extra)` and the distance moves
   by `spring(wanted - distance, last step, 0.02)`. So between minDist and maxDist the camera
   does not move at all: it is dragged when the target walks away and pushed when it comes
   closer. When an obstacle shortened it last update (`+0xdd`) and the offset is now shorter,
   the distance is `|offset| + 0.2` at once.
9. Elevation (slot 20, `FUN_00923460`). User part U = E - A. Stick: `p = fade * stick.y *
   userRotSpdX * (same scale) * dt`, negated unless option bit 2 is set. If `|p|` is over 2
   degrees per second it applies directly, `U -= p`, unless that would leave the range
   [minElevation, maxElevation] further; and it ends the drift to defaultElev. Otherwise, while
   that drift is armed, U moves by `spring(default - U, last, 0.02)`. Automatic part A springs
   with gain 0.03 toward what keeps `U + A` inside the range (plus 45 degrees in mode
   88875297). `E = clamp(U + A)`.
10. Position = P + distance * (heading of the offset, elevation E).
11. Corner logic (`+0xdc`, `FUN_00923c30`, `FUN_00923c90`): NOT read.
12. A downward ray from just above P (`FUN_005be5a0`) lowers P under a ceiling.
13. `FUN_009256c0`: sweeps from P to the position (`FUN_005be890`, radius 0, or 0.65 in one
    mode), takes the nearest hit on an object flagged as blocking, and hands look-at and eye
    to the camera object. Only its first part is read; how it slides along walls is not.

Aim (slot 4, `FUN_00927190`, from the second task): the position is used as it is. The
orientation is not: wanted pitch and heading are those of the line from the eye to P, and the
camera keeps a lag to each (`+0x10c`, `+0x110`) that is driven to zero by
`spring(-lag, last, interpElevGain)` and `spring(-lag, last, interpAngGain)`. The lag is
initialised from the camera's current orientation whenever the behaviour starts (`+0xd6`), so
a change of camera swings round smoothly. Roll is `+0x128`.

## The driving camera, step by step

Same family, same fields; these are its own. Extra fields: `+0x134` speed distance add,
`+0x150` field of view used last update, `+0x154` vehicle velocity, `+0x160` direction D
(target to camera, kept between updates), `+0x16c` align rate, `+0x170` a scalar from the
character (`FUN_007a3a60`; behaves like height above ground), `+0x174`/`+0x178` user yaw
offset and rate (`+0x174` is never written by this class), `+0x17c` user elevation offset,
`+0x184` roll rate, `+0x188`/`+0x18c` smoothed speed and its rate, `+0x190` speed factor,
`+0x194` airborne, `+0x195` the FlightMode flag (`FUN_007256e0` character_in_flight_mode: motion mode type fb9f3262; identified and wired 2026-10-18, see the section at the end).

Starting (slot 1, `FUN_00914260`): clears all of those, then the follow camera's start, then
roll 0, D = minus the camera's forward, and no aim lag is set up (`+0xd6` cleared).

Update (`FUN_00915740`):

1. Level change and field-of-view spring as in the follow camera. No height blend.
2. Smoothed speed: `rate = 0.9 * rate + 0.01 * (|velocity.xy| - s)`, `s += rate`. Speed factor
   f moves 15% per update toward `clamp((s - minSpdAlignMove) / (maxSpdAlignMove -
   minSpdAlignMove), 0, 1)`. For Bumblebee that is 35 to 43: it only rises in turbo.
3. Slot 16 (`FUN_009143c0`): the follow camera's sponge, then the horizontal look-at offset
   moves at 5 per second toward `sideOffset` along the vehicle's right (0 for Bumblebee).
4. The offset used is minus the camera's forward times the current distance, not the real
   position (so the aim lag does not feed back).
5. Slot 18 (`FUN_00914c90`), heading:
   - wanted field of view = `defaultFOV * (1 + (maxSpdFOVScale - 1) * f)`;
   - stick: `u = fade * stick.x * userRotSpdZ * (1 - f + userRotSclMaxSpeed * f) * dt`, sign as
     in the follow camera. When the camera is more than 90 degrees off "behind the vehicle" and
     the stick pushes further, u fades to nothing at 135 degrees;
   - roll: `spring(leanAmount * (vehicle's roll angle) - roll, last, leanGain)`;
   - align: `a` = angle from D's heading to "behind the vehicle". Two per-update steps:
     `stepA = alignGain * a + (1 - alignDamp) * last stepA` and
     `stepU = userRotGain * (offset + u) + (1 - userRotDamp) * last stepU`.
     D and the offset are turned about the vertical by `stepA + stepU`.
6. Slot 19 (`FUN_009153f0`), distance: `k = sin(defaultFOV) / sin(wanted field of view)`; the
   speed add is `0.9 * add + 0.1 * maxSpdDistAdd * f`; the wanted distance is the same leash,
   `clamp(length, minDist + add, maxDist + add)` on the length with last update's k taken out,
   sprung with 0.02, and the result is multiplied by k. So the camera closes in as the field
   of view widens.
7. Slot 23 (`FUN_009145b0`), elevation: user offset `+= stick.y * userRotSpdX * dt` (no
   sensitivity scale). Wanted = `defaultElev + tiltAmount * (vehicle's pitch) + user offset`,
   kept in [minElevation, maxElevation] (the user offset is trimmed to fit). While airborne
   (the scalar over 2, then until under 0.1, horizontal speed over 5) the tilt term becomes
   the larger of itself and a quarter of the way from the current elevation to the velocity's
   pitch minus 22.5 degrees. Elevation moves by `spring(wanted - E, last, tiltGain)`.
   Position = P + D * distance, with its height kept at least `max(sin E, -forward.z) *
   distance` above P and above the body's own height.
8. Look-at = target point with z at least the body's height; then `FUN_009256c0` as before.

Not used by this class as far as read: alignFaceMaxSpeed, alignFaceMinSpeed, alignAutoMinSpeed,
maxFaceAngOffs, the unnamed 568fb10e (the look-aside variant `00916610` / `009162d0` is unread).

## Corrections from the live trace (2026-10-03; ported in `passthrough\tf2-core\src\camera.rs`)

Run 1 recorded the camera object, the behaviour and its settings every update
(`work\traces\run1_cam.pkl`; `camera_replay` in `tf2-core\examples` replays them).

- The sponge's "last change" is the last correction, `new offset - carried offset` (checked
  on a jump: 0.058, 0.412, 0.375 behind for rises of 0.14, 1.02, 0.86).
- Stick signs as stored on the behaviour (`+0x6c`, `+0x70`): heading changes by
  `-stick.x x rate x dt`; elevation by `+stick.y x rate x dt` (`+0xf4` and `+0xf8` hold minus
  the pitch and plus the yaw applied this update). 0.1025 radians an update at full stick:
  180 degrees a second times a scale of 1.0, so the sensitivity option sits mid-range.
- The elevation the user sets stays (no drift back to `defaultElev` after the stick is used;
  `+0xd9` reads 0 in play).
- The aim lag (`+0x10c`, `+0x110`) reads zero all through stick turns and running: it is only
  set when a behaviour starts. Re-deriving it each update throws him to the edge of the
  picture on a turn (seen in the mod, fixed).
- Extra distance: its rate (`+0x124`) is in units per second, limit 10. It rises whenever
  `dot(facing, offset) > 0`, even slightly; it is 15 while climbing (`climbExtraDist`); and
  during and after melee it is 10 with the hold timer `+0x13c` counting down from 3.67 s.
- The distance level is the player's: the recording has level 0 on foot
  (`bbeeRobotnearCamera`) and level 2 in the car (`bbeeDrivefarCamera`, type `f88e590b`),
  and one stretch where the on-foot level was stepped (distance 11 to 20 and back).
- Driving: the speed that feeds the view's widening is the capped one (35, or 43 in turbo),
  so the view stays at 45 at cruise and reaches 64 in turbo. The distance does NOT shrink
  as the view widens (14.0 held at 64 degrees), against step 6 as read above.
- The settings object in memory, after `interpTargetHeightTime`: heightOffset, minSpdAlignMove,
  maxSpdAlignMove, alignGain, alignDamp, maxSpdFOVScale (x 0.01), maxSpdDistAdd, the unnamed
  one (-0.1745 for Bumblebee's far camera), alignFaceMaxSpeed (x 0.01), alignFaceMinSpeed,
  alignAutoMinSpeed (x 0.01), sideOffset, userRotSclMaxSpeed (x 0.01), maxFaceAngOffs,
  userRotGain, userRotDamp, tiltAmount (x 0.01), tiltGain, leanAmount (x 0.01), leanGain.

## Collision and corners (partly read, 2026-10-03; NOT in the port)

`FUN_009256c0(behaviour, out, P, eye, again)`, from pseudocode, not checked on a trace:

- Sweep `FUN_005be890` from P to the eye, radius 0 (0.65 when `+0xe0`). Hits are 0x24 bytes:
  point, normal, then distance at `+0x1c` and the object at `+0x20`. Only objects with bit 5
  of `+0x9c` block; an object with bit 5 of `+0x9e` goes to `FUN_0091ecb0` instead (unread;
  probably hides or fades it). The nearest blocking hit is used.
- No hit: `+0x7c` = P, `+0x88` = eye, and `FUN_0091e930` stores on the camera object P at
  `+0x138`, the eye at `+0x144` and the unit direction eye-to-P at `+0x15c` (the recording
  agrees: `+0x144` equals the camera's position).
- Hit nearer than 0.1: the camera keeps its last eye; `+0xd6` is set (aim lag restarts).
- Ordinary hit: `+0xdd` = 1 and the eye is the hit point projected onto the line P-to-eye,
  with no margin. Because the next update measures its offset from the real camera, the
  distance then eases back out through the 0.02 spring instead of popping (and step 8's
  "`|offset| + 0.2` at once" applies while still blocked). The port keeps an unshortened
  position and pops back: wrong.
- Corner: taken when the last eye was more than 0.1 behind the hit surface, the hit is more
  than 1 nearer than the last distance, last P and last eye are on opposite sides of the
  surface, and the stick is not turning away (`FUN_00923c30` on `+0xf8` and `+0xf4`: true
  when the turn this update is under half a degree or is toward the corner). The corner
  point (`+0x94`) is where the old line crossed the surface, pushed 0.15 out along the
  normal (`+0xa0`). The nearest edge of the hit box (shape type 4, twelve edges) or the
  axis of a type 6 shape (these two numbers are the shape's vtable slot 1 value, which is
  the dispatch-table type + 2: so `CollisionCube`, type 2, and `CollisionCylinder`, type
  4; `docs/collision-shapes.md`, 2026-10-11) is kept at `+0xb8` (origin) and `+0xc4` (direction), signed
  distance `+0xf0`, flags `+0xde`/`+0xdf`. Then `+0xdc` = 1, the eye is put on the line from
  P through the corner point at the same distance, E is recomputed, and the routine runs
  once more with `again` = 1. So he rounds a corner and the camera swings to look past it
  instead of zooming in.
- `FUN_00923c90` slides the corner point along the kept edge each update: along a mostly
  level edge to stay in the vertical plane of the view; along a mostly upright edge to the
  height that matches the elevation E.
- In the update (`FUN_00924310`, after the position is set) corner mode holds while P and
  the eye stay on opposite sides of the kept plane, the corner is nearer than the eye, and
  the stick is not turning away; otherwise `+0xdc` clears. A ray up from P
  (`FUN_005be5a0`) lowers P and the sponge under a ceiling.

### Read to the end and ported (2026-10-03, second pass)

`FUN_009256c0`, `FUN_00923c30`, `FUN_00923c90` and the end of `FUN_00924310` are now read
in full (pseudocode, with the listings for the register arguments). Ported in
`tf2-core\src\camera.rs` (`collide`, `hold_corner`, `Corner::slide`, `pulled_in`), not
checked on a recording. What the reading added to the list above:

- `+0xac` is the unit vector from the hit point to the corner point (`FUN_005571b0` is
  "normalise into"): the open side. The hold test in the update is: P and the eye on
  opposite sides of the kept plane; the corner nearer P than the eye is; both stick tests;
  and the point where the straight line P-to-eye crosses the plane still on the hit side
  of the corner point (`dot(+0xac, crossing - corner) < 0`). Then the eye is put on the
  line P-through-corner at the same distance and E is taken from it. Otherwise `+0xdc`
  clears.
- `FUN_00923c30(turn; ESI = a, EDI = b)`: true when `|turn|` is at most half a degree or
  `sign(turn) * (a.x * b.y - a.y * b.x) > 0.001`. For yaw a and b are the level unit
  directions P-to-corner and eye-to-P and `turn` is `+0xf8`; for pitch they are the
  (length over ground, rise) pairs of the same two and `turn` is `+0xf4`.
- `FUN_00923c90(eye; ESI = behaviour, EDI = P)`. Level edge (direction more level than
  upright): the corner point moves along it to the upright plane through P and the eye.
  Upright edge: the edge is first shifted sideways by the kept signed distance `+0xf0`,
  across the view; then the point on it is found whose rise from P matches `tan(E)` (a
  quadratic; with no solution the point keeps its height). For a type 6 shape `+0xa0`
  and `+0xac` are rewritten there (the normal becomes the level direction eye-to-P).
- The driving camera's constructor (`FUN_009141c0`) sets `+0xe0` (sweep radius 0.65) and
  `+0xd5`, and `+0xd5` switches the corner logic off: it is only pulled in, and because
  its offset is rebuilt from the kept distance (step 4) it is back at full distance as
  soon as the way is clear. With `+0xd5`, a hit nearer than 0.1 puts the eye at P.
- The ceiling ray runs from (P.x, P.y, middle of the body) up to 1 above P, when P is no
  more than 1 below that middle; a hit lowers P, the eye and the sponge offset so that P
  is 1 under it, and a rising sponge rate is zeroed.
- `FUN_0091e930` also takes a third point (0.2 from the eye toward P, or 0.1 off the wall
  along its normal when the wall faces the view by less than 0.25) and hands it to
  `FUN_009202b0` (unread).

Stand-ins in the port: Arkham Knight's trace gives a point and a normal and no shape, so
the edge is made up (an upright wall gets an upright edge through the corner point, or a
level one if the open side is above; other surfaces get none, which is the original's own
path for shapes that are not boxes or type 6); the eye is kept 0.3 short of a wall; the
driving camera's sphere is a line kept 0.65 short.

## Still to do for the camera, in order

Recentring in the port, the
driving camera's tilt, the sniping camera (`00929d50`; not his), slot 0 (`00921b10`), the
mode names, what `FUN_007a3a60` and `FUN_007256e0` report (the last three have unchecked
answers in `handoff\result-03.md`).

## The weapon camera, step by step (read 2026-10-04 from disassembly; ported)

Listings in `work\asm\cam3\` (`raw_disasm.py`). `handoff\result-03.md` Part A describes
this camera too and is wrong in most of it; see "Result 03, checked" below.

It never reads the stick. The stick turns the character's AIM (owner `+0x260`,
`FUN_0071b990`, in `weapon-mode.md` "The aim and the stick"); the message `ca291e26`
hands the aim to the camera, and the camera follows it.

Behaviour fields of its own: `+0x150..+0x158` the aim, `+0x15c..+0x164` V, the unit vector
from the point it turns about to the camera, `+0x168`/`+0x16c` the rates of that point to
his side and ahead of him, `+0x170` elevation lag (its rate is the shared `+0x100`),
`+0x174` his radius (`FUN_0070ab60`), `+0x178` lag needs setting. Settings: `+0x6c`
maxRotSpd (radians), `+0x70` sideOffset, `+0x74` aheadOffset, `+0x78` elevGain; minHeight
and 5297b970 are read by nothing in these routines.

Start (slot 1, `00927f60`): the follow camera's start, then (unless it is a change of
distance level) the two rates from the old sponge rates, the point looked at put on the
current view ray (`+0x2c`, within 20, plus the old behaviour's look height minus its own),
V = minus the camera's forward, `+0xd9` = 0 (no drift to a default elevation), `+0xd6` = 0
(no view lag), `+0x178` = 1.

Slot 17 (`00928460`): copies the aim from the message's `+0x38..+0x40`, NOT normalised;
if its xy length squared is under 0.0001 the old x and y are kept.

Update (`00929470`), each frame:

1. Level change, the timer `+0x12c`, and the field of view on its 0.02 spring, as on foot.
2. T = target position + the offset `+0x18..+0x20` (z is the look height, rewritten each
   update by `FUN_00922580`: his height + the character's `cameraWeaponTargetHeightOffset`
   + the settings' heightOffset; 5.4 + 0.8 + 0 for him). x and y from the physics holder
   (`FUN_00923a70`). No blend to a new height.
3. `FUN_009284e0(behaviour, T - last T, dt, T)`:
   - vertical offset `+0x2c`, when T rose or fell or it is over 0.001: `+= spring(-it,
     last, minHeightGain)`, kept in [-height, 20]. T's rise is NOT taken out of it, unlike
     on foot: it only springs away what the start left.
   - F = the aim's xy, unit; S = (F.y, -F.x), his right. The horizontal offset
     (`+0x24`, `+0x28`) is read in that frame as (side, ahead). Wanted: (sideOffset,
     aheadOffset). If |aheadOffset| < 0.1 and radius * 0.25 > sideOffset (his: both 0):
     wanted ahead = -radius * aim.z (unit aim), and the look height `+0x20` is lowered by
     radius * (1 - |aim.xy|) for the rest of this update (it only feeds step 5's floor).
   - each of the two moves by `spring(wanted - now, rate * dt, 0.5)`; the change of rate
     that asks for is cut to 50 when it is larger and either the point is at rest or it
     speeds the point toward where it is wanted.
   - with |sideOffset| > radius - 0.7 and heightOffset < 0: a ray (`FUN_005be5a0`, mask
     0x20) from T + heightOffset to the side keeps the point 0.7 short of a wall. Not his.
   - the offset is written back as S * side + F * ahead.
4. P = T + offset. O = minus the camera's forward * |camera - P|. Heading, slot 18
   (`00928c60`): off = wrap(heading of minus the aim - heading of V). If |off| >
   maxRotSpd * dt, `+0xd7` is set. While `+0xd7`: step = pi * dt; if step > |off| it is
   cleared and the turn is all of off, else the turn is step + 0.1 * (|off| - step) toward
   it. Not set: the turn is all of off. V and O are turned about z by it. For him 720
   degrees a second: 23 in an update, so in play the heading is the aim's at once.
5. Distance, `FUN_00929280`: wanted = clamp(|camera - P|, minDist, maxDist); the distance
   `+0x104` moves by `spring(wanted - it, last, 0.02)`; if V.z * distance < 1 - height it
   becomes (1 - height) / V.z (the eye no nearer the ground than 1 above his feet). Eye =
   P + V * distance; its z is at least P.z - height + 1.
6. Elevation, slot 20 (`00928f40`): Ev = that of V, Ea = atan2(-aim.z, |aim.xy|). When
   `+0x178`: lag `+0x170` = Ev - Ea. The lag moves by `spring(-lag, last, elevGain)`.
   E = Ev + wrap(lag + Ea - Ev), which is Ea + lag: the aim's elevation at once, plus
   only what the take-over left. V is rebuilt from its heading and E; eye = P + V *
   distance. Below -10 degrees `+0x138` = -maxDist (if positive): a one-off the FOLLOW
   camera's distance slot reads when it next runs; nothing here reads it.
7. Last T, `+0x60` = -V, then the shared collision `FUN_009256c0(behaviour, out, T +
   (0, 0, offset.z), eye, 0)`: from the point on his axis, not from P. V is stored.

The view itself is the shared slot 4 (`FUN_00927190`): it looks at the camera's stored
look-at point plus the offset's xy (or along `+0x60` when `+0xd5` is set or `+0xda` is
clear), with the view lag, which this camera never sets.

In the port: `Camera::update_weapon`, `Target::aim` and `radius`, `Tuning::weapon_camera`
and `weapon_look_height`. Left out: the side ray, the `+0x138` hand-over, `+0xdb` (ride
along). Assumed: the point starts on his axis at rest; the vertical offset at the start is
only "on the view ray". Checked by three tests and `work\shots\aimcam_3.png`; no recording
has weapon mode's camera in a form that was compared (run 1 has ten stretches of weapon
mode: `camera_replay` could be pointed at them).

## Result 03, checked (2026-10-04)

`handoff\result-03.md` tags everything [read]. Checked against the disassembly:

- Part A (weapon camera): the routine list and slot 1 are right. Wrong: the order (it
  has heading, elevation, then distance; it is heading, distance, elevation); "turn rate
  clamped to maxRotSpd" (it snaps, or swings at pi a second); "elevation clamped at -10
  degrees" (it is the `+0x138` hand-over); "offsets scaled by his radius"; "50 is a speed
  limit" (it limits the change of speed); `+0x160`/`+0x164` as the sponge (they are V);
  slot 17 "normalises"; the collision's last argument as "radius 0.25" (it is `again` =
  0); the aim point "1000 from the eye" (`FUN_007b2310` is in `weapon-mode.md`: 500, from
  beside him). Missed altogether: that the stick turns the aim in `FUN_0071b990`, and the
  swing of the point for a camera with no offsets, which is Bumblebee's case.
- Part C: the mouse as a stick is right in substance (`FUN_004f0e30`: counts over
  `DAT_00bf361c`, an INTEGER 10 that the options screens rewrite, 0 below zero and 1
  above the divisor; one control per direction, table at `004f0e60`). The dead zone is
  right: `FUN_005aa890` takes each axis to -1..1, zeroes it under `+0x18` and rescales
  (|v| - d) / (1 - d); `FUN_005aa700` sets 0.25 for the sticks (constant at `00b387f8`,
  not the address the result gives) and 0.1 for what follows. Update2026-10-09:
  `0050b6b0` bridges mapped actions, including mouse, into the normal `005aa890`
  path. The gamepad dispatch cases at `004ec6c0` remain unread. See docs/input.md.
- Parts B, D, E, F: NOT checked. Nothing of them is used. B and the zoom camera are not
  Bumblebee's by the data, which was checked (`sniperCamera` is 0; none of his three
  driving cameras has the `userLookAside` fields).

## Shakes (2026-10-03, read from code and data; `core\src\shake.rs`)

Listings: `work\asm\shake_update.txt` (`0091f5d0`, `0091fc80`), `shake_run.txt` and
`shake_run2.txt` (`0091df50`).

- Settings: the 14 `*CameraShake` roots of `camera.lxb` (`bnxglobal.str#0`), type e49598c6,
  loader `0091f0f0`: `+0x10` shakeAngHoriz, `+0x14` shakeAngVert, `+0x18` shakePosHoriz,
  `+0x1c` shakePosVert, `+0x20` shakeTilt, `+0x24` shakeFullDist, `+0x28` shakeMaxDist,
  `+0x2c` shakeDuration, `+0x30` shakeHalfDuration, `+0x34` shakeMaxAngAcc, `+0x38`
  shakeAngGain, `+0x3c` shakeAngDamp, `+0x40` shakeMaxOffsAcc, `+0x44` shakeOffsGain,
  `+0x48` shakeOffsDamp. The three angles and shakeMaxAngAcc are stored in radians. They
  sit in a list searched by name hash (`DAT_00d434f4`).
- Starting one: `FUN_0091dc70(camera, name)` with no place, `FUN_0091dd30(camera, name)`
  with one (the position in EDI). A name not in the list starts NOTHING. The instance is
  0x58 bytes on the camera's list at `+0x184` (new ones go on the far end: the list is
  walked oldest first): `+0x08` settings, `+0x0c` place, `+0x18` three angles (up and
  down, tilt, to the side), `+0x24` their speeds, `+0x30` two offsets (side, up), `+0x38`
  their speeds, `+0x50` age, `+0x54` "use the real frame time" (never set by the two
  starters), `+0x55` has a place. Shakes add up; none replaces another.
- Who starts them: `AnimCameraShakeEvent` (`FUN_007d0ff0`: `name`, `positional`, on when
  absent; positional takes the place of the object the event is handed), `Explosion`
  (`FUN_007e5d30`, slot 4 of its table at `00b60898`: `cameraShakeOnExplode` at its own
  place), `CameraShakeTrigger` (registration00adb490, load007d0e20, fire007d0f60,
  checked2026-10-07; same named/positional starters). Damage00729120 starts a shake
  only while sniping, outside Bumblebee's installed settings.
- Update (`0091f5d0`, once per camera frame with the frame's time; a step under 0.0001
  does nothing):
  - strength = (k t - (k + 1)) t + 1 kept in 0..=1, with h = halfDuration / duration,
    t = age / duration, k = (h - 1) / h * (h - 0.5). (Not what the name suggests: it is
    0.5 at half time only when h is 0.5, where it is a straight line.)
  - with a place and maxDist over 0.01: d = camera position (`camera + 0x58`) to the
    place; if maxDist - fullDist is over 0.01 the strength is times
    1 - (d - fullDist) / (maxDist - fullDist) kept in 0..=1, else 0 past maxDist.
  - three random angles are drawn in +-(angVert, tilt, angHoriz) * strength, in that
    order, then two offsets in +-(posHoriz, posVert) * strength (`FUN_00551d60(low, high)`:
    the C library generator, fifteen bits, with the clock's seconds stirred in).
  - each value chases its draw: wanted change of speed
    ((1 - damp) * dt * speed + draw * gain - value) / dt - speed, limited to
    +-maxAcc * dt when maxAcc is over 0.0001; speed += that; value += speed * dt, kept in
    +-limit (the speed is not touched by that). With gain 1 and damp 0 the value lands on
    draw + speed * dt, so the frame rate sets how fast it jitters.
  - age += dt; over the duration the shake is removed, and that update adds nothing.
- Putting them on the camera (`0091fc80`, `0091df50`): each shake's turn is tilt (about
  the view's line), then up and down (about x), then to the side (about z, up), as row
  matrices tilt * pitch * yaw; all shakes' turns are multiplied oldest first; the offsets
  are summed and laid along the camera's right (`+0x04`) and up (`+0x1c`) rows. The 3x4
  result is kept at `camera + 0x64` (`FUN_00920f50`). Render composition checked
  2026-10-07 at0077a72c: camera*shake via0052fbf0, including a second basis transform
  of that stored world offset; the former own-axis assumption is resolved.
- His clips' events (`work` one-liner over `bumblebee.txt`): `Jump_Land`, `Run2Idle`,
  `Carry_BigJump_Land`, the weapon set's small landings: `SoftCameraShake`; `BigJump_Land`,
  `Jump_LandRun`, `Weapon_Fall`, `Weapon_FallBigLand*` (but `...2RunRight` is the soft
  one), `Recover_Slow`, `CustomAnim3`: `MediumCameraShake`; all at tick 2 (the weapon set's run landings at 4 and 6),
  positional. `GroundPunch` and `GroundPunchFall` name `MediumShake1_16f` (tick 8) and
  `StrongCameraShake` (tick 10), `Death` and `Death_Explode` `MediumShake1_16f`: no object
  of either name exists in any file of the game, so those events do nothing. The punch's
  shake is its explosion's: `MassiveCameraShake` (strong), `MedHardCameraShake` (weak).
- In the port: `shake.rs` (`ShakeDef`, `Shakes`, `Jolt`), `Tuning::shakes`,
  `ExplosionDef::camera_shake`, `Clip::shake_events`, `sim::State::shakes`; the game's
  `Rig` runs them and `camera::apply` draws with them. Assumed: updates of 0.032 s; the
  clip's shake is placed where he stands; the explosion's routine runs as the object
  is put there. Clock mixing and final matrix composition are now implemented,2026-10-07.
  [stand-in] Camera-local RNG state instead of the original cross-system stream.
  Checked by tests and captures; still not compared against original shake motion.

## AnimatedCamera (2026-10-16, read end to end; ported in `core\src\camera\scripted.rs`)

A level component (74 objects in 3 levels) whose scene object (the rig) carries an animation. Ledger:
`fn.py known animated_camera`. [game] unless tagged.

- Load (01186d50): `buttonThrough` (+0xc2), `trigDone` links (+0xf4), loaded flag +0xc3 = 1. The constructor
  (007cfe50) keeps a copy of the rig's start affine at +0xc4 and seven message rings.
- Trigger (slot 6, 007d03a0), once while off: the rig goes back to its start affine and its animation
  restarts (0053d420); a behaviour (007cfd30, vtable 00b60328, FOV 60, name CameraGame) is pushed onto the
  game camera (0091db00); +0xc1 set; per-step updates asked for when `buttonThrough`.
- Behaviour update (00913440): while it is the camera's active behaviour and dt > 0 the camera FOV (+0x94) is
  the behaviour's; the rig's world rows are taken with forward normalised, then if `|f.up| <= |f.right|` up is
  multiplied by `1 - f.up` (its own components, not forward's: a quirk of the original, disassembly
  009136f1) and normalised and right = f x up; else right is squared to forward and up = right x f. Dead rig:
  the behaviour removes itself (0091db80, which restarts the parent from the current view).
- Untrigger (slot 7, 007d05d0) and teardown (007d0200): behaviour off and deleted; the depth-of-field globals
  back to 0 / 100,000 / 0.001 / 0.001 with the override flag clear. No `trigDone`.
- Update (slot 2, 007d0290): pad record (007212f0) +0x94 bit 0x1000 skips: Untrigger, then `trigDone`.
  Which button that bit is: [unknown].
- Messages: 9388ead0 (after a Trigger: Untrigger and `trigDone`), 77ed0811 (bool: on without rig reset / off),
  e78310ed (FOV = value x 42.97183, i.e. radians x 0.75 x degrees per radian), and four floats into the
  globals 00ce4ae4, 00ce4ae0, 00ce4adc, 00ce4ad8 with flag 00ced676, which the light update (00569cd0 at
  0056a2cd) copies over the environment's DOF words 0x72..0x75. [assumed] those are nearFocus, farFocus,
  nearFalloff, farFalloff. The seven names are not in `names.tsv` or the executable's strings.

CameraTracker: started, see the unpublished notes.

## Not started

Air control (the routine that applies a mode's control vector), car handling, the animation
state machine, and everything on the "not there at all" list. See `bumblebee-mechanics.md`.

## Reading the listings

`tools\ghidra_scripts\DumpDisasm.java` prints, per instruction: `L+xx`, the stack slot as its
displacement at the function's resting depth (the same local keeps one name through every
push); `d=n` when the line runs n bytes deeper; and for register-to-register x87 arithmetic
the direction spelled out (Ghidra prints "FMUL ST1" for both `st0 = st0 * st1` and
`st1 = st1 * st0`). Extra arguments: `vt:<table address>` resolves calls through that table and
prints the callee, and `<call address>:<bytes>` states what one call removes. A colon, because
the batch wrapper splits arguments at `=`. Lines marked `!!` or "assumed" need a look.
`work\find_ptr.py <address>` finds where an address is stored (callbacks are never called by
name, so the pseudocode has no reference to them).


## A flyer's camera is the driving camera, and the flight flag is now wired (2026-10-18)

Asked because `game\README.md` claimed the flyer camera was "written but NOT wired" and the
roster plan made wiring it step 1. The README was wrong twice over, and so was a 2026-10-12
decision recorded in the unpublished notes.

### 1. A flyer is followed by the driving camera [data]

A flying character's `drivingCamera` attribute names a **driving-camera settings block**, so a
flyer is followed by the driving camera class 00b65d88, whose elevation and eye rules are step 7
of this note (`FUN_009145b0`). From `<install>\bnxglobal.str#0`:

- Starscream's `drivingCamera` is `starscreamDriveNearCamera` (hash 1414596080;
  `docs/flight-camera-exit.md` A4).
- That block's field set is **identical** to `bbeeDriveCamera`'s: both 39 fields, diff empty
  (`tf2_lxb.py dump "<install>\bnxglobal.str#0" 2`, field names compared). The robot follow block
  `bbeeRobotnearCamera` has 20 fields and a different set, so the types are easy to tell apart.
- The behaviour class table above has no flight camera. The only variant in it still unread is
  the look-aside pair 00916610 / 009162d0, which is not a flight class.

What differs between a flyer and Bumblebee is therefore the DATA, not the class:
Starscream has `tiltAmount 80` and elevation range [-65, 65] where Bumblebee's drive block has
`tiltAmount 0` and [-8, 35]. That is why the flight flag below can matter for one and not the
other.

### 2. What the flag is, and that it is now set [game]

`core\src\camera.rs` `Target::flying` is the behaviour's byte `+0x195`:

- 00916030 (the driving camera's slot 17, `driving_camera_direction`) copies it off the target
  structure: `*(byte *)(param_1 + 0x195) = *param_2 >> 7`, i.e. bit 7 of its first byte, next to
  the capped velocity (+0x2c..+0x34) and the minimum wheel clearance (+0x54) the port already
  maps.
- 007256e0 `character_in_flight_mode` is what produces it: the character's motion mode type is
  `fb9f3262`, FlightMode. `sim::Mode::Fly` stands for that same class (`core\src\sim.rs`), so
  the port's `state.is_flying()` IS the flag, not an approximation. Line 403 of this note had it
  as "a character flag (`FUN_007256e0`, not identified)"; it is identified now.
- It is distinct from `+0x194`, the airborne flag. Bumblebee cannot enter FlightMode, so it is
  false for him throughout and nothing about him changes.

What it gates in 009145b0, all three now reachable in the port:

1. `if ((fVar11 == 0.0) && (*(char *)(param_1 + 0x195) != '\0')) { +0x17c = fVar8 - fVar8 * 0.5; }`
   -- with no stick push the user elevation is halved each update.
2. The eye's rise becomes the plain `sin(E) * distance`, without the nose-down floor and the
   body floor that the ground path keeps.
3. Once the elevation has settled (rate <= 1.7453293e-7) the FOV ratio scales the rise and the
   flat offset.

`game\src\camera.rs` now passes `flying` instead of `false`. The 2026-10-12 decision to keep it
false was made because it "would change upright flight"; that was cautious rather than wrong at
the time, but the branches are 009145b0's own code and the flag is the game's own flag.

### 3. What it does, measured

Captures on `US_Test_FreeRoam_A`, Starscream, vehicle trigger from 0.5 s and the stick pulled
back to climb (`work\shots\starscream_fly_cam_*.png` before, `starscream_flycam_wired_*.png`
after):

| t | pitch | height | camera before | camera after | framing |
|---|---|---|---|---|---|
| 2 s | 58 deg | 37 m | 18.3 m | 22.6 m | before: only the nose tip at the bottom edge. After: centred |
| 5 s | 5 deg | 141 m | 22.8 m | 24.0 m | unchanged to the eye |
| 8 s | - | 133 m | - | - | boundary left and re-entered in both |

So the flag bites under pitch and barely at all in level flight, which is exactly what the
2026-10-12 note was worried about. The flight path itself is unchanged (positions within 1 m).

The mechanism, pinned by `camera::tests::the_flight_flag_drops_the_eye_below_a_steeply_climbing_flyer`:
with Starscream's block at pitch 58 the eye drops BELOW the flyer instead of being held near his
own height by `.max(floor - hang.z)`. The view direction is identical either way -- it is never
re-aimed at the look-at point -- so he rises in the frame purely because the eye went under him.

Bumblebee driving is unchanged (`work\shots\bbee_car_regress.png`, camera 15.0 m behind as
before); by construction, since `is_flying()` cannot be true for him.

### 4. Still open

- Whether a flyer's pitch or roll feeds `tiltAmount` / `leanAmount` is still not read
  (`docs/flight-camera-exit.md` A3 assumes the body's pitch). The port feeds pitch, and with
  `tiltAmount 80` that is what drives the elevation to about -34 degrees in a steep climb.
- The airborne test in 009145b0 also forces "not airborne" when the behaviour's `+0x140` is
  `0x5e89f579` (a camera request id, this note line 17). The port's `airborne` does not have
  that condition. Unported, newly noticed.
- The look-aside pair 00916610 / 009162d0 is unread.
- SETTLED (2026-10-18, third pass): the 38-42 m at which the robot flyers and Grindor were
  followed is the FAR (third) level of their own `drivingCamera` chain
  (`OpFlightDriveFlightVeryNearCamera` 17-19 -> `OpFlightDriveFlightCamera` 30-32 ->
  `OpFlightDriveFlightfarCamera` 40-42; Grindor 24-26 / 32-34 / 38-40; Starscream 12-14 / 16-18 /
  22-24), and `game\src\main.rs` starts every character on level 2, the user's recorded setting.
  Printed at levels 0 / 1 / 2: Powered-Up Optimus 17.9 / 30.8 / 40.8 m. Not a port fault;
  the unpublished notes "third pass".
- NOT COMPARED with the original: no recording has a flying character in it. Everything in
  section 3 is the port against itself.

## The hover's camera (searched 2026-10-21): there is no camera of its own

Asked because the user remembered a camera made for hover. Searched three ways:
- **Data** [data]: `bnxglobal.str#0` (`export\data\global_cameras.txt`) has no hover block; the
  attributes have only `normalCamera`, `weaponCamera`, `drivingCamera`, `sniperCamera`; no name or
  string pairs hover with camera.
- **Code** [game]: the character's camera message (0071a5c0) asks for a mode name from char
  `+0x1d0`, or 5e89f579 while the avatar blend (owner +0x248) runs. Writers and readers of those
  names: `Normal` 26296dd2, climb 3efeac6d (climb_mode_enter), ground-punch fall 88875297
  (0071a5c0 itself), death 7d110862 (00922580, bit 0x200). HoverMode's enter (008713c0) writes no
  camera mode. 5e89f579 is read only by the driving camera's elevation (009145b0), so the follow
  camera ignores it.
- **Recording** [trace] (`ce_rotf_starscream_20261021_112840.bin`, behaviour block: class, `+0xd0`
  settings, `+0x140` mode, `+0x144` form, grouped by movement mode and control state):
  plain hover (control state Basic) runs the follow camera 00b65d28, form 0, with the SAME settings
  object he used on foot just before (2316dc10 / 2316dc80, his robot chain); weapon hover (state
  Weapon) runs the weapon camera 00b65fe0, form 1, with his weapon settings (04ea93f0, 04ea9360,
  04ea9480). Entering hover from flight, the follow camera gets 5e89f579 for about 1 s (no effect,
  see above). Most of the recording's hover time (about 2985 of 3700 samples) was weapon hover, so
  the over-the-shoulder weapon camera is what was on screen.
- The port does the same: plain hover is the robot follow camera; weapon hover holds the aim, so
  `camera::target` hands him to the weapon camera. Nothing to change.
- Not looked at: the camera classes 00b65e58, 00b65e98, 00b65f00, 00b65ed8 and the follow variant
  00b65f80. None was active at any point of the recording. A fourth mode value, 415dfcf6, fills
  `+0x140` for most of the recording before 464 s; its name is not resolved.

### The aiming-hover camera against the recording (2026-10-21)

The user's "hover camera" is the camera while aiming in hover: the weapon camera above. Replayed
through the port with `work\weapon_camera_export.py <capture> <out.csv> Hover|Strafe` and
`core\examples\weapon_camera_replay.rs -- Starscream <out.csv> [stretch]` (recorded position,
look height and aim in, the port's eye against the recorded eye; open world, no collision).
- [trace] The recorded point it turns about is exactly the code's: side 0.5 (sideOffset), ahead
  = -radius * aim.z (radius 3.4, the swing on his radius), in hover and on foot alike; the aim
  reaches +-85 degrees in hover (AimAssist ZDir limits), and the camera's elevation follows it.
- Result: hover, 11 stretches, 2209 samples after a 1 s settle: eye off by 0.41 m on average, no
  bias (farther -0.37..+0.06, higher -0.13..+0.13 per stretch). On foot (Strafe), most stretches
  0.0-0.6 m; stretches 16-17 (513-520 s) are 1.9-4.4 m nearer in the original, taken to be a
  wall the open replay world lacks (not checked against the level).
- Two replay pitfalls, not port faults: (1) the recorded `+0x20` is the look height AFTER the
  update lowered it by radius * (1 - |aim.xy|) for the floor test (3 m when aiming straight down),
  so the replay adds that back; (2) a fresh camera has none of the point's side/ahead offset and
  keeps the shorter distance its start measures (the distance only moves outside [minDist,
  maxDist]), so the replay settles 1 s and reseats on the recorded view before counting.
- Nothing in the port changed. Not compared: the take-over from the follow camera into weapon
  hover (the replay starts on the recorded weapon view), and collision in hover.
