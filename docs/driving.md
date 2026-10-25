# Driving: research notes (in progress)

## Fixed-step display jitter correction (2026-10-07)

User reported constant self-rubberbanding after the suspension physics handoff.
The whole car pose now advances only every .032s; player::present, the tyre nodes,
and camera::target were consuming this held pose at render frequency. The camera's
existing continuous extrapolation then moved relative to the held car between steps.
This was a presentation regression caused by the suspension update timing change.

sim::CarPresentation now retains the previous completed car frame and interpolates
position, quaternion, steering, continuous wheel-model spin and supported contact
planes using the physics remainder/.032. The root, animation suspension offsets
and camera target consume the SAME presentation frame. Camera capped velocity and
full motion remain distinct. Gameplay, springs, collision, sound and rules continue
to use authoritative physics. Drive entry/reset clears history; non-car modes use
their current pose. Quaternion interpolation handles the +/-pi heading seam.

[assumed] This is a render adapter one update behind the authoritative car; the
original render scheduler/interpolator has not been decoded. Vehicle assumption8
in status.md records the reason and the evidence needed for original timing parity.

Installed-tuning reproduction: core/examples/vehicle_presentation_check.rs, log
work/vehicle_jitter_diagnostic_20261007.txt. 1,499 measured straight-cruise frames
per case after settling; both paths run the existing camera on the same physics:
- 60Hz: old held frames718, displayed speed0..72.323m/s, max relative camera
  jump .9436m; new held frames0, speed37.310..37.672, max relative jump .0169m.
- 120Hz: old holds1109/jump1.1628m; new holds0/jump.0131m.
- 144Hz: old holds1174/jump1.2121m; new holds0/jump.0134m.
- Irregular7/21/12/45/9ms frames: old holds748/jump2.0322m; new holds0/jump.0570m.
The small remaining speed variation is authoritative tyre-force response, not
held frames. Relative camera jumps fall by97.2..98.9%; no original parity claim.

297 core tests pass, including steady movement/wheel spin at60/120/144Hz and
irregular/hitch frames, turn/reversal heading continuity, reset/re-entry, and the
actual-radius sharp-turn clearance checks for BOTH physics and displayed poses.
Canonical game build succeeds. Inspected work/shots/vehicle_jitter_turn_20261007_v1.png:
body/wheel pose remains correct in the turn; a still cannot verify temporal jitter.
Build/test/capture logs: work/vehicle_jitter_{build,tests,capture}_20261007.txt.

## Suspension physics handoff, verified and applied (2026-10-07)

User asked to learn the original suspension before applying it. Static reads below
and offline `work/suspension_frames.py` supersede the earlier approximation.

The original update is: DriveMode adjusts velocity/caps; wheel-pre temporarily
aligns the holder for queries, tyre forces, weight transfer and collider targets;
00862000 damps/limits WORLD angular velocity; wheel-support-finish restores the
character's frame and applies predictive support correction; world physics then
accumulates springs, gravity and tyre forces, integrates velocities, solves contacts,
and integrates the actual pose. The temporary aligned holder is not the persistent
sprung body. Weight transfer sees velocity before accumulated tyre forces take effect.

007a34c0 has one exception: previous support normal (wheelset+90) dot current (+9c)
<.98 calls 00570d80/00545900 to write the aligned character frame. These normals are
filtered WITHOUT renormalisation. This threshold can fire while their lengths recover
after a terrain change. Otherwise ordinary spring lean survives between frames.

Verified physics fields, separate from holder fields:
- Holder108..134=current affine;138..164=inverse;168..194=predicted affine;
  198..1c4=predicted inverse;1c8..1d0=predicted-current translation delta.
- Physics body54..74=normalised basis,78..80=centre,84..8c=axis scales,
  90..9c=row-vector quaternion; a0..a8=world linear velocity,ac..b4=NEGATIVE
  conventional world angular velocity;3c..44=force,48..50=negative conventional torque.
  b8..c0/c4..cc are contact-solver extra linear/angular velocities.
- 006de840 imports the holder affine, extracts scales, normalises axes and builds
  its quaternion with0052efb0. 006dde60 exports scaled axes and centre.
- 006dbd30 sets holder current/inverse and synchronises its body; param1 also updates
  predicted transforms. For character placement,0052fbf0 composes the input with
  the holder's local avatar transform. 006dc3b0 commits an affine to current/body.
- 006db1f0/006ddad0 predict the affine;006dae00/006ddf30 advance/commit it.
  Quaternion update is normalised `q + q*storedWorldOmega*dt/2` in row convention;
  glam's conjugate is `q + conventionalWorldOmega*q*dt/2`.0052e700 normalises each
  exported axis;0052f1c0 exports the unnormalised basis used for inertia.
  Position advances the
  BODY CENTRE, then the root removes the newly rotated avatar offset.
- 006e96c0 adds constant gravity, applies body linear/angular multipliers, zeros
  tiny velocities (squared2.5e-5/1e-8) and invalid squared speeds>100000.
  Both multipliers are .9995 in all6,869 stable captured Bumblebee bodies [trace].
- 006daca0 calls006de230 for awake dynamic bodies; it does NOT always zero omega.
  006de230 adds force*inverseMass*dt and worldInverseInertia*torque*dt.
  006de390, raw390..506 plus0052f1c0/0052f310, constructs R^T*diag(invInertia)*R
  in the port's column convention. This unusual order is VERIFIED in the recording:
  conventional R*diag*R^T yields median .0217rad/s spring omega error; actual order
  yields .0000303rad/s. Preserve the engine's arithmetic instead of correcting it.
- 006da400 splits only force accumulation/damping at<=1/60, weighting each force
  by split/full_dt. It does NOT integrate the body between splits. All spring target
  positions stay fixed; body anchors are evaluated AFTER support correction.
  Full 3D force is retained. There is no post-spring angular clamp.
- 006df7c0 read as outline and lines1..176/398..460 ONLY: spring_apply then
  006e96c0/006daca0, prediction, collision solver, pose commit, accumulator clear.
  The contact solver remains unread; do not treat this partial read as full parity.
  006db430 clears force/torque/solver extras.00556d20 wakes/inserts the active list;
  00556ca0 sleeps/removes;00556f20 sets the sleep counter;00556c60 tests>249.
  0117bea0 is a world collision manager allocator/init, not a per-car body constructor.
  006db490/006dd5b0/006dd800/006dd900 are the existing character contact/control
  projection described in bumblebee-mechanics.md; not the suspension spring.
  006e5e00 read as outline plus1..111: moving-body broadphase; rest remains unread.

Offline evidence:6,855 consecutive captured drive-frame pairs. The single-step
quaternion update with captured final world omega has matrix error median2.29e-7,
p95 2.42e-6. Resetting to ground EVERY frame fails (median .0571). Accounting for
the .98 conditional ground reset improves the ordinary-frame replay; remaining
outliers include phase-crossing/terrain changes.6,423 internally consistent,
four-grounded pairs are eligible for rotation fixtures;101 spaced cases are exported
as numbers only in core/fixtures/vehicle_20261007_rotation.csv. This is seeded
rotation replay, not a complete unseeded suspension/contact replay.

Full spring handoff:6,776 four-grounded pairs, with pre-physics centre recovered
from the recorded final centre/velocity (therefore a SEEDED force check, not an
independent position prediction). Current spring lengths and previous aligned
collider targets reconstruct velocity error median .0000777m/s,p95 .0625m/s and
angular error median .0000303rad/s,p95 .0001336rad/s. Projecting away spring force
raises median velocity error to .1114m/s,p95 .2934. Capture/contact outliers remain,
including some high errors.6,760 cases with consistent holder/body bases and no
contact extra velocity are eligible; ALL6,760 numeric cases are in
core/fixtures/vehicle_20261007_springs.csv. Regression uses median/p95, retaining
outliers; it does not claim full wall/contact fidelity. Script/output:
work/suspension_frames.py, work/suspension_frames_20261007_v8.txt.
Independent pre-physics centre reconstruction using the contact plane and captured
support-correction scalar (mode+d8) agrees with the integrated-centre reconstruction
at median .0000138m,p95 .00818m. This additionally checks support-correction order;
phase/contact outliers remain. The fixture still explicitly seeds that centre.

The old run1 pull-away speed test compared a level zero-height start with a
non-level recorded start: at621.656 rootz .114 and support normal xy .0087,-.0048;
by622.857 support normal xy .0528,-.0415. Missing wheel probes meant the original
terrain wasn't replayed. Its speed-curve assertion failed when full spring force
was retained. Replaced by seeded recorded spring velocity/omega regression above,
while the unseeded flat test retains smooth acceleration and original steady cap.
Do not silently reinstate the projected force to satisfy that unmatched curve.

The old visual regression accidentally left radius/travel at zero in its synthetic
tuning. It now uses the installed .4 radius and +/- .1 travel. Applying the original
visual rule to the recording's flat-contact subset gives nominal clearance p05
-.0417m (median approximately0); mesh radius is .4308m. Shallow overlap is possible
with the original authored visual limits; the deep .24m lean failure is separate.

Core now uses queued forces, world omega/inertia, fixed physics updates, pre-physics
support correction, fixed world spring targets and centre/quaternion pose integration.
The former projected-net-force bridge (assumption7) is removed. Synchronous rays and
the arena's contact impulse remain existing assumptions; no new fitted game constant.
Also corrected the visual steering sign:007a10d0 uses NEGATIVE mode steer for tyre
axes,008601d0 uses -3 times MODE steer for wheel meshes. The core previously exposed
the negated physical angle to animation; it now exposes the original mode angle.
Physical steering direction is unchanged.

Final installed-model20s turn diagnostics: peak roll .1009rad (5.8deg), lowest
nominal tyre clearance -.0192m left/-.0219m right, versus initial .3506rad/-.2419m.
This is deterministic arena motion, not matched original inputs. Ramp launch vz6.08,
peak15.386m, one departure/landing; bank roll .149rad; thin-wall sweep stays outside.
Both sharp turns, steering reversal at3 step rates, bank/ramps/landing pass. Core
tests include6,760 seeded spring updates and101 seeded rotation updates.
Canonical build and capture/test logs are in status.md. Initial v3 captures inspected
both turn directions and both bump lanes; latest v4 uses corrected visual steer.
Latest v4 captures inspected: left full turn at4.5s, rollers and split bumps at1.8s
(body height .2/.4m respectively), with individual wheels unloading over the bumps.
Final verification:295 core tests,2 recorder tests, canonical game/recorder build pass.

## Sharp-turn wheel penetration (2026-10-07, initial correction superseded above)

User's screenshot shows the low-side tyres sunk into the floor during a sharp turn.
Reproduced in `work/shots/suspension_sharp_before_20261007_v1.png`. The installed-model
diagnostic `core/examples/suspension_check.rs` originally reached .3506rad body roll
and -.2419m nominal tyre clearance during sustained full steering on flat ground.

Rechecked `007a34c0` with raw34dd..377c: it obtains the holder transform, aligns its
frame to the wheelset support normal (projected forward), and installs that aligned
basis before wheel queries, tyre forces and collider placement. The old port used
the leaning body frame for both spring anchors, dropping the restoring component.
`vehicle::support_rotation` now provides the independent wheel frame. Probes and tyre
directions use it; spring displacement retains the separately transformed body anchor.
Support normal filtering runs after support correction rather than during probing.
Rechecking006da400 lines54..57/100..107 also found a dropped split weight in damping:
the force displacement is weighted by split/full dt before projection, and the
projection uses that weighted axis twice with the original unclamped length. The
adapter now follows that arithmetic instead of applying full damping to every split.

The complete Lux holder/solver frame composition is still missing. The arena uses an
explicit [assumed][stand-in] bridge: full anchor torque, support-normal net spring force.
Applying the temporary frame difference as longitudinal force failed the recorded
pull-away comparison. This bridge preserves that regression; it is not proof of full
original suspension parity. See status.md vehicle assumption7 for the replacement work.

Offline `work/suspension_survey.py` checks the user's expanded recording. In8,869
four-grounded-wheel samples whose contacts differ in height by<=.03m, original roll
ranges -.1820..+.1889rad (p05/95 -.0987/+.1004). This is a sampled flat-contact subset,
not a matched input replay. Holder centre matches root+bodyUp*1.6 with median error
.0000079m,p95 .0254m; update-crossing/outlier samples remain. This supports the existing
root/body mapping. Wheel ray basis usually differs from the visible body lean.

New tests cover projected support axes, restoring anchor torque, both turn directions
and abrupt reversal at .032,1/60 and1/120 seconds. Nominal .4-radius tyre clearance is
within .012m of the flat floor; the old code failed by .24m. The installed wheel mesh
radius is approximately .431m, so the original .4 visual tuning can still give shallow
mesh overlap; this is distinct from the original large roll failure and remains a
visual-contact comparison item. The target-separation test now checks cylinder distance
on every update, allowing the car to pass around a target instead of requiring a final
y-only parking position. Recorded pull-away/braking and existing terrain tests pass.

## Suspension and vehicle comparison work (2026-10-07)

User priority: suspension/wheel contacts, slope and collision replacements, remaining
vehicle effects, and automatic observations while the user plays ROTF. Original-game
execution remains the user's job. Run1 and the expanded2026-10-07 capture are available.

### Expanded capture comparison (2026-10-07)

User reports tuning/audio differences and shorter boost in the rebuild. Capture:
`work/traces/vehicle_20261007_152811.bin`,15,226 samples/332.8s;11,013 drive samples
with wheels/abilities. Weapon blocks absent: this recording cannot settle car-gun parity.
`work/vehicle_compare.py` writes its survey and camera CSV. Horizontal speed median
37.583m/s,p95 47.983,max48.256;35/43 caps plus wheel push remain consistent with run1.
This does not settle handling/terrain/contact response; no arbitrary handling fit was added.

Completed standard turbos last4.771..4.799s between observed state edges. Base block15
still has2.4s/3.2s; ability block13 duration14=2.4,active timer18 up to4.7682 after the
first update,cooldown20 up to1.6. Missing rule: player upgrades. Raw00732f27..4c
multiplies activation duration by upgrade f3bd1a64;00733058..7d multiplies cooldown
by95118079. Installed multipliers:duration1.25/1.5/2,cooldown.85/.65/.5. Both level3
selections match this capture. `DriveTuning::turbo_upgrades`, `driving::turbo_times`,
`State::turbo_upgrades` implement them. Game defaults[3,3] for the captured setup;
`--turbo-upgrade 0..3` changes it. Original save import remains missing. Reset/death
retains the selection; HUD uses effective durations. Core State defaults remain level0.

Audio regression: `work/gearbox_fixture.py` exports81 spaced consecutive update pairs
from6,841 usable pairs to `core/fixtures/vehicle_20261007_gearbox.csv` (numbers only).
Core matches gear/revs/load and RPM (median.000244,p95 .001953,max.005859).
Each case seeds the previous state: this validates individual updates, not a full
unseeded replay. Adapter now consumes the sim's gearbox, removing the second copy.
Mixer/waveform parity remains open (docs/sound.md).
Final check:287 core/2 recorder tests pass,canonical game/recorder builds succeed.
Inspected level3 and level0 captures at4.5s:level3 still boosting1.3s of4.8;
level0 cooling after2.4s. Logs/images listed in the unpublished notes.

New static reads (ledger entries):
- `006da400`: active spring local anchors are transformed to world space. For squared
  displacement > .001, clamp displacement length to 1 and apply K * displacement.
  Substeps <= 1/60 weight the force by substep/full_dt. Damping removes axial *linear*
  velocity times suspensionDamp/60 each substep. This is not critical spring damping.
- `007a1960`, raw `007a1b44..007a1baf`: body anchor is wheel mount minus avatar height;
  collider anchor zero. `007a2cd0`, raw `007a2dc3..007a3003`: collider placed at that
  transformed mount plus the wheelset's support normal * current suspension value.
- `007a1460`: grounded reach L*1.5 - min(vz*dt*2,0), normal.z > .1, ray distance>.001;
  spring value L-(rayDistance-2*avatarHeight), ungrounded relaxes at dt*10. Ray starts
  at transformed mount + support normal * avatarHeight, extends 15 along -normal.
- `007a30c0`: horizontal velocity delta is rotated to local coordinates; sign of its
  dot with mount chooses L +/- (L-L*WTScale)*clamp(delta.length,0,1). At >=.95 maxSpeed
  with delta.length<.1, front/rear coasting uses gearbox rev progress * offset * .3,
  inverted while reversing. `007a4fe0` computes progress against the previous gear's
  shifted maximum RPM; first gear uses RPM/maxRPM.
- `007a3b00`: support plane from contact points; with four contacts discard the lowest,
  with one/two use the first contact normal. Filter normal at dt*2 (3+) or dt*10 (1/2).
  Predicted plane penetration: slow correction .4 and dt*10 filter; inward normal
  velocity below -5 loses .75 of that velocity before position correction.
- `00862000`: damp pitch/roll velocity per update using data and clamp all three axes.
- `007d9e40`: CollisionFX kind1 impact, kind2 roll, kind3 scrape, kind4 end. Material
  hardness chooses hard/soft triplets; alignToNormal places the effect at the contact.
  `007dab90`/`007da7b0` retain keyed contact history: initial push (types3/4), ten
  signed updates (roll+1, scrape-1), thresholds +/-7, .25s expiry and an end event.
  `007d94d0` smooths a loop's body-relative contact offset by at most .01/update.
  `0056e7b0` aligns +z to normal with projected reference +y, preserving emitter twist.
- `008601d0`: wheel models rotate about +x by axial velocity*dt directly as radians,
  clamped [.2,.375] forward (.55 turbo) and at least -.3 reverse; waiting gives zero.
  Front steering is -3*steer about +z. Grounded node correction is nodeZ-radius*upZ-
  contactZ, clamped by min/maxRenderWheelOffset. Physical tyre spin is separate.
- `00782310`: car-gun damage is instant, but its BulletTrace visual moves at bulletSpeed
  until it crosses the hit endpoint. The emitter follows the moving visual and is deleted
  with it when instantKillBulletFX is enabled (Bumblebee's installed data).

Implementation assumptions (also recorded in status): root-to-body offset follows
avatarDescArray[1].offsetZ; arena collision supplies static convex contacts instead of
Lux's solver/manifolds. Wheel queries are synchronous rather than its async probes.
These must be checked with the expanded recorder before claiming original parity.

Implemented in `core/src/vehicle.rs`, `collisionfx.rs`, `terrain.rs`, `sim.rs` and the
game adapters. Springs drive pitch/roll; independent support replaces centre snapping,
slope fitting and ramp-rise smoothing. Continuous cuboid-versus-convex sweeps replace
the arena car's box/cylinder wall push and blanket speed halving. Original Lux material
friction/restitution and wheel sphere contacts remain unverified; the arena response is
an assumed inelastic solver. Non-car primitive collision is unchanged.

Recorder: `trace/record-vehicle.ps1` starts the read-only executable from the user's
administrator terminal. It logs per-wheel springs/contacts, body motion, gear RPM/load,
boost states, drift charge and gun heat/shot age, with automatic event timestamps and
Windows output audio. `--analyze` replays binary samples offline. Run1 analysis: 25,665
records, 5,222 driving samples; zero available wheel/ability/weapon samples, so it cannot
settle these comparisons. Audio was not captured in run1.

Verification (2026-10-07): deterministic installed-tuning diagnostic over 12 seconds
each: flat chassis settles within .024m of root zero; orange lip launches at 2.624s
with vz7.30m/s, reaches z15.743m and lands; sideways bank reaches roll.192rad;
thin-wall drive stays at y26.987m against the y30m wall. These are rebuild tests,
not measurements of ROTF.

Addresses are for the analysed `Transformers2.exe` and stay in these notes.

Status (2026-10-02): the top level of DriveMode's update, the body rules and the wheel model's
first step (steering, engine, ground query, tyre force) are read. The wheel set's second
step, drift, the suspension spring's formula and the exit are not (list at the end).

## Where it is

DriveMode: table 00b62730. Update `FUN_0085cad0` (given dt), exit `FUN_0085ed50`, slot 1
`FUN_0085c880` (creates a helper object once), slot 8 `FUN_00860870`, slot 9 `FUN_0085f780`.
On the mode: `+0x04` owner, `+0x18` the tuning block (offsets below are from it), `+0x20` and
`+0x24` two button-state objects (turbo and drift turbo; byte `+0x24` on each is "was active"),
`+0x2c` the wheel set (pointer and count; each wheel has "on ground" at `+0x90`), `+0x3c`
throttle value, `+0x1c`/`+0x44` steer value, `+0x48` stick x, `+0x50` trigger, `+0x4d`
reversing, `+0x4e` waiting to reverse, `+0x1f8` brake-held time, `+0x1fc` time in the mode,
`+0x200` a speed being ramped (braking, after turbo), `+0x204` filtered trigger, `+0x208`
post-turbo ramp running, `+0x210` speed after the caps.

Tuning block = `characterAttributes<Name>.modeDrive`, in memory at attributes `+0x1b8`
(`FUN_00859710`). From the block's start: `+0x04` maxSpeed, `+0x08` maxTurboSpeed, `+0x0c`
maxTurboTime, `+0x10` maxTurboCooldownTime, `+0x14` throttle, `+0x18` turboThrottleBoost,
`+0x1c` turnSpeed, `+0x20` maxTurnSpeed, `+0x24` engineTorque, `+0x28` brakingTorque, `+0x2c`
wheelFriction, `+0x30` wheelDriftFactor, `+0x34` frontAxleLength, `+0x38` rearAxleLength,
`+0x3c` longAxleLength, `+0x40` frontWheelRadius, `+0x44` rearWheelRadius, `+0x50`
minDriftTime, `+0x54` minDriftTurboSpeed, `+0x58` driftTurboTime, `+0x5c`
driftTurboBoostFactor, `+0x60` suspensionLength, `+0x64` suspensionK, `+0x68` suspensionDamp,
`+0x6c` suspensionWTScale, `+0x70` postTurboDeceleration, `+0x74` skidOutDeceleration, `+0x78`
driftSteeringFactor, `+0x7c` maxDriftTurboSpeed, `+0x80..+0x94` the exit values, `+0x98`
angularVelDampX, `+0x9c` angularVelDampY, `+0xa0..+0xa8` maxAngularVel X Y Z, `+0xac`
bulletSlowSpeedPercent, `+0xc4` aiCorneringCoefFriction, `+0xd4` collisionTurnRate, `+0xf0`
postReverseThrottle, `+0xf4` minReverseBrakeTime, `+0xf8` maxReverseSpeed, `+0xfc`
brakingDeceleration, `+0x100` triggerFilterStrength, `+0x104` hasReverseAnim.

## The update's top level (`FUN_0085cad0`)

It works on the body's velocity directly; "speed" below is horizontal speed, and "set speed"
means the horizontal velocity is rescaled to that length with the vertical part kept.

1. Trigger: `t = (trigger + 1) / 2`; the filtered value moves toward it by
   `triggerFilterStrength * dt` of the gap (or jumps when that is 1 or more). Digital input: 1.
2. Speed caps, first that applies:
   - turbo held: if speed is over `maxTurboSpeed * factor`, set it to that. The factor
     (`FUN_0085fc80`) is 1 unless an upgrade or a slow effect is active;
   - drift turbo: cap at maxDriftTurboSpeed;
   - reversing: cap at maxReverseSpeed;
   - braking (brake held, a wheel on the ground, not reversing): the speed at the moment the
     brake went down is remembered and brought down by `brakingDeceleration * dt` each update,
     and the speed is set to it. Linear, straight to zero;
   - after a turbo, while faster than maxDriftTurboSpeed: a remembered speed starts at the turbo
     cap and comes down by `postTurboDeceleration * dt` until it meets the normal cap;
   - otherwise: cap at `FUN_0085f8b0` = `maxSpeed * filtered trigger * factor`. So the trigger
     scales the top speed, not the push.
3. Reverse: holding the brake at a stop for minReverseBrakeTime sets "reversing"; it ends when
   the brake is up and the forward speed is not negative.
4. Skid out (a flag and a button test): the velocity is pushed back along itself by
   `skidOutDeceleration * dt`.
5. With a wheel on the ground, moving along the body's up, and the body upright (up.z over
   0.9): 95% of the velocity along up is removed. This is what keeps it from hopping.
6. Throttle value: `throttle * turboThrottleBoost` in turbo, `throttle *
   driftTurboBoostFactor` in drift turbo, plus or minus `throttle` while braking,
   `postReverseThrottle` when rolling backward with no brake, else `throttle`.
7. Steer value: `(maxTurnSpeed - (maxTurnSpeed - turnSpeed) * clamp(speed / maxSpeed, 0, 1)) *
   stick.x * dt` when steering is allowed (`FUN_00862e50`), else 0. So 8 at a standstill
   falling to 4 at top speed for Bumblebee.
8. Then, in order: `FUN_007a2860(wheels, body, dt)`, `FUN_00862000(mode, dt)`,
   `FUN_00861c20(mode)`, `FUN_0085fd90(mode, dt)`, `FUN_007a3b00(wheels, body, dt)`.

## Body rotation, air and walls (`FUN_00862000`)

- Every update the angular velocity is damped and limited: the x and y parts are multiplied by
  angularVelDampX and angularVelDampY and kept within maxAngularVelX and maxAngularVelY; the z
  part (yaw) is only limited, to maxAngularVelZ.
- With no wheel on the ground:
  - if the body is tipped or rolled more than 10 degrees it is turned back toward level
    (`FUN_0056e7b0`, `FUN_0056dfe0`, `FUN_00545900`);
  - the stick pushes the velocity: `(right * stick.x + forward * stick.y) * 10 * dt`;
  - yaw rate decays: `z -= z * dt`.
- A helper with six feelers (`+0x214`, class built by `FUN_007c5bb0`): when more than one is
  touching, the yaw rate is zeroed.

## The wheel set (embedded in the mode at `+0x2c`)

Its fields are the mode's: `+0x10` throttle value, `+0x14` brake amount, `+0x18` steer value,
`+0x20` skid, `+0x22` "stopped by brake", `+0xbc` the tuning block. Built by `FUN_007a2300`:

- four wheels (`FUN_007a0df0`), at `(-+frontAxleLength / 2, +longAxleLength / 2, 0)` and
  `(-+rearAxleLength / 2, -longAxleLength / 2, 0)` in the body's space;
- per wheel: `+0x3c` radius (front or rear), `+0x48` inertia = radius squared, `+0x4c` =
  wheelFriction, `+0x58` = wheelDriftFactor, `+0x5c` = driftSteeringFactor, `+0x80`
  suspensionLength, `+0x84` suspensionK, `+0x88` suspensionDamp, `+0x8c` suspensionWTScale,
  `+0x40` torque this update, `+0x44` spin (angular speed), `+0x90` on the ground.

Pre-step (`FUN_007a2860(wheels, body, dt)`), every update:

1. Steering (`FUN_007a10d0`): the two front wheels are turned by the steer value as an angle in
   radians. Since that value already contains dt, the wheel angle depends on the frame time
   (about 7.6 degrees at a standstill and 3.8 at top speed at 60 updates a second).
2. Engine: unless stopped by the brake, every wheel gets `engineTorque * throttle value` added
   to its torque. All four wheels are driven.
3. Per wheel, the ground query (`FUN_007a1460`): a ray 15 long from above the wheel's mount,
   down along the wheel set's support normal (corrected 2026-10-07). On the ground when it hits within about one and a half suspension
   lengths and the surface normal's z is over 0.1. The suspension value is
   `suspensionLength - (hit distance - 2 * height of the vehicle avatar)`; off the ground it
   relaxes back toward suspensionLength at 10 per second.
4. Per grounded wheel: brake torque `brakingTorque * spin * brake amount` is taken off (0 for
   Bumblebee), then the tyre force (below) is applied to the body at the wheel's position
   (`FUN_006de600`: adds to the body's force and torque for this step).
5. Each wheel's own small collider is placed at its mount, moved along up by the suspension
   value (`FUN_007a2cd0`). The suspension itself is a spring between the body and that
   collider, made once by `FUN_006e06e0(body, collider, suspensionK, suspensionDamp)`
   (`FUN_007a1960`). The spring's formula is now read in `006da400`, documented above.

Tyre force (`FUN_007a1150`), with `v` the velocity of the body at the wheel in the wheel's
frame, `f` the wheel's rolling direction, `s` its sideways direction, `w` its spin, `r` radius:

    slip = v - f * (w * r)
    lat = dot(slip, s)        (taken as 0 under 0.01)
    lon = dot(slip, f)
    force = -s * lat * wheelFriction * kLat  -  f * lon * kLon
    w += (r * lon + torque) / (r * r) * dt ;  torque = 0

`kLat` and `kLon` are 1, or wheelDriftFactor and driftSteeringFactor while the wheel's skid flag
is set. So: sideways speed is opposed in proportion (5000 per unit of speed per wheel on a
body of mass 5000), and forward push comes only through the wheel spinning faster than the
ground. In a steady pull the spin settles where `lon = -torque / r`, giving a push of
`torque / r` per wheel, and it gets there with a time constant of one second.
For Bumblebee (engineTorque 15000, throttle 12, radii 1.6 and 2, mass 5000) that is about 81
units per second squared with all four wheels down, so the speed caps above are what set his
speed; the engine is strong enough to follow them.

## The slide (read 2026-10-03 from `FUN_0085cad0`'s pseudocode)

The skid test is a flag on the mode (`+0x1f5`) and button 0x14 held, with a wheel down. While it holds: the
velocity is pushed back along itself by `skidOutDeceleration * dt` (step 4 above), the wheel
set's `+0x20` is set, and EVERY wheel's skid flag (`wheel + 0x54`) is set, front and rear
alike; otherwise all are cleared. So all four tyres drop to `wheelDriftFactor` sideways and
`driftSteeringFactor` forward. In the sim this makes the car carry straight on and hardly
steer. `FUN_0085fd90` is not drift: it places per-wheel effects. How the second button
object (drift turbo, `+0x24`) is now read; its former guessed rule is replaced below.

## The pad's rumbles (2026-10-04)

Read and ported; the table and every caller are in `docs/melee.md` ("The preset
rumbles"). For the car: the turbo starting (preset 14), a wheel on the ground when none
was after the wheel set's last step and the mode is over 0.1 s old (13; the wheel set's
`+0xa8`, mode `+0xd4`, is "a wheel was on the ground", written at the end of
`FUN_007a3b00`), and from the contact handler `FUN_00860870` a touch at a speed over 5
with contact kind 3 (12) or of something that can break (11). Of `FUN_00860870` this
much more is read: for kind 3 against a character it calls the ramming routine
`FUN_007190c0` with the tuning block's `+0x114`..`+0x120`; for kind 3 it calls
`FUN_007a38e0` when the contact's normal dotted with one of his body's axes (which one
is not worked out) is under tuning `+0xc8`; and it takes the part of his velocity going
into a character back out. Seen in the pseudocode only, not checked in disassembly.

## Not read yet

The wheel post-step, weight transfer, forward probe and spring creation/update were
read during the 2026-10-07 work above. Still open: original Lux contact solver/material
friction/resting manifolds, wheel sphere interactions, the unread physics restoration
in `FUN_0085ed50`, the rest of slot 8 `FUN_00860870`,
and the companion mode (`FUN_00863070`).
Listings are in `work\asm\`.

## Assumptions replaced (2026-10-07)

`core/src/driving.rs` replaces the guessed drift boost with the original timed
ability. The constructor `0085c2f0` identifies cee630d9 as standard turbo and
ea8020ae as drift turbo (vtable00b5d630). It gives the latter +0x38=minDriftTime,
+0x40=minDriftTurboSpeed and +0x14=driftTurboTime. Reset `00733ab0` sets state3
(uncharged), zeros elapsed charge +0x3c, active duration +0x18 and ended latch +0x24.

Ghidra wrongly discards most of `00733e10` as unreachable. Narrow raw reads establish:

- `00733f11..00734096`: obtains body velocity (`00556c00`), the object's first/right
  matrix row (`00725a70`) and drift button0x14. `00409a50` is fabs, not acos. It tests
  abs(dot(velocity,right)) STRICTLY GREATER than minDriftTurboSpeed; total forward
  speed does not charge the boost. There is no wheel/ground test in this ability.
- `00734098..007340c6`: held and qualifying increments elapsed charge; >=minDriftTime
  sets state0, ready. Otherwise elapsed charge resets, but a previously ready state
  remains ready while held. Before ready, qualifying stretches cannot accumulate
  across an interruption.
- `00734170`: ready only in state0 and with no standard turbo active (and no vehicle
  special of kind4; Bumblebee's EMP is not that kind).
- `007340c6..00734151`: release from ready calls `00732e60(1)`, fills duration and
  rumbles preset14. A blocked release sets state3 and consumes the charge. Pressing
  drift again does not suspend an active boost.
- `00733e80..00733f0e`: active duration counts down. Zero still counts active; only
  a STRICTLY NEGATIVE remainder ends it, resets to state3, sends off and sets the
  ended latch. There is no drift cooldown. Its ended cap uses maxDriftTurboSpeed.

The trigger filter is `0085cd56..0085cda0`: from the previous +0x204 toward the
trigger by triggerFilterStrength*dt; copies immediately when strength<=0 or the
product>=1. The tuning loader now reads that installed field and the sim uses the
filtered value for the normal speed cap and the gearbox. `0085e130` sets +0x204=1
on entry; starting at zero would wrongly slow a fresh car's acceleration. The sim
now initializes it to1, matching run1's first driving update. His installed strength
is1 (and the driving regression fixtures now use it).
`0085f8b0`'s corner square-root branch is gated by NOT `00711210` (input controller
types0/2), so it does not apply to player Bumblebee. Crucial call distinction:
`0085fa3e..0085fa65` multiplies by filtered trigger only when dt!=0. The tyre sound
calls with dt0, so its hard-turn threshold uses maxSpeed, not trigger-scaled speed.
Other status/navigation branches remain outside this replacement.

Standard turbo's duration/cooldown rule is confirmed by `00733180`/`00732e60`;
`00721b50` means vehicle special kind4, NOT brake. `007324d0` is the base ability
update, not the standard turbo's virtual update. `0085ed50` unregisters the drift
ability and vehicle FX ability, stops turbo FX, but does not unregister/reset the
standard turbo timer. The sim now preserves that timer across transformation,
ticks active/cooldown outside the car and stops only its FX. Drift start sound,
rumble and FX are wired; the last independent turbo on/off message is kept instead
of deriving it from timer activity. Its physics restoration is not claimed ported here.

The former planar body solver, fitted slope tilt and absent spring were replaced in
the 2026-10-07 section above. Original collision solver parity and capture comparison
remain open, along with arena ground-material fidelity and the sound update's sample
order relative to movement. Ability scheduling relative to the mode's fixed update has
not been matched to a drift recording; run1 contains no comparison established here.
## CE suspension comparison: the outliers are solver contacts (2026-10-16)

`python work\spring_outliers.py <springs.csv> 0.1` replays the 7,460 seeded cases (same
numbers as the Rust test: velocity median 1.06e-4, p95 1.48, max 47.4 m/s). 633 cases are
over 0.1 m/s:
- 468 have the residual along ONE world axis (>0.99 of it; x 342, y 78, z 48), and on that
  axis the recorded velocity ends at about 0 (median 0.0003 m/s): an inelastic contact
  against axis-aligned level faces, not a spring error. The longest run (43.67..49.38 s,
  176 updates) is the car pushing into a +x-facing wall with -46k..-190k N of tyre force.
- Taking the residual direction as the normal, 316 of 633 end with that component at 0
  (|v.n| < 0.05) and omega UNCHANGED (median 4e-5 rad/s; a point impulse at a corner
  would change it by ~0.9 rad/s per m/s). That is the solver's simple path in 006e2c10
  (lines 159-170 separating test on holder +0x1c8; 212-301: position step keeps only its
  tangential part, approaching normal velocity of the CENTRE removed, plus a bounce
  mu*min(1,|vn|/3)*|vn| with mu = max(+0x214); no angular term). Ported as
  `vehicle::static_contact_velocity` (replaces the point impulse with lever arm).
- The other 317 do not fit a single contact; their omega change is small too (median
  0.037 rad/s). Multi-contact or unread solver terms (006e8f00, 006e9250 are unread).
- The capture cannot mark contact frames: holder block 6 is 512 bytes, so +0x244 (hit
  time) and +0x214 are not in it, and +0x1d4 bit 0x4000 is never set at sample time.
  To settle: record holder +0x1c0..+0x260 (trace\) and filter on +0x244 != FLT_MAX.
The env-gated CE test therefore still fails, as expected without the world; the default
fixture passes. Not loosened.

Also read 2026-10-16: the drive mode's contact handler `00860870` (ledger
`drive_contact_handler`): a kind-3 contact with normal . body forward (holder row 1,
disasm 00860c2c..00860d6b) under modeDrive +0xc8 `collisionForwardDot` (-0.85 for him)
calls 007a38e0 with AL = 0 (throttle, every wheel's spin and torque to 0, "stopped by
brake" cleared). Ported (`vehicle::head_on`, `Chassis::stop_wheels`, sim step_drive).
Its velocity projection is for CHARACTERS only (not static walls).

## CE suspension comparison (2026-10-08)

User asked whether suspension matches. Source CE capture
`work/traces/ce_rotf_20261007_164337_49792.bin`; all15,848 Drive samples have wheels.
Offline suspension_frames.py exported7,460 eligible seeded spring cases and101 sampled
rotation cases. Existing current Rust spring regression FAILS on this new capture:
velocity error median.000120719,p95 1.4794922,max47.439087m/s;angular error median
.000045387,p95.016547272,max2.2435853rad/s. Existing limits were median.0005/p95.1
velocity and median.0002/p95.0005 angular; retained, not loosened. Good median agreement
does not establish full parity. Cannot yet separate asynchronous mixed snapshots,
contact/terrain transitions and missing physics rules; no speculative correction made.
The101 sampled rotation cases pass matrix tolerance.00005. Full offline rotation survey
has7,138 eligible pairs,median matrix error2.62e-7,p95 2.19e-6 but max.06839;
sampled test success does not cover these outliers. Existing earlier-capture default
rotation/spring tests also pass. This is seeded force/integration comparison, with
pre-physics centre reconstructed from recorded output; no independent terrain/world replay.

Flat four-grounded survey8,739 samples:holder/root offset median error8.19e-6m,
p95 2.47e-5m,max1.55m;wheel-length residual median-.000102m,p05-.149m,p95.145m.
Offsets/contact samples remain phase-sensitive. Full logs and numeric fixtures beside
capture:`.suspension_frames.txt`,`.suspension_survey.txt`,`.suspension_comparison.txt`,
`.rotation.csv`,`.springs.csv`. Optional TF2_ROTATION_CAPTURE/TF2_SPRING_CAPTURE inputs
added to existing tests, default fixtures/tolerances preserved; runtime unchanged.
SciPy unavailable in bundled Python:work/rotation_math.py provides standard quaternion
conversion fallback (1,000 deterministic random roundtrips <1.64e-15 matrix error,
known90deg Z rotation checked). No new gameplay assumption introduced.
