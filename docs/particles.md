# The particle system (`aps`), 2026-10-04, finished 2026-10-05

## Muzzle attachment and exhaust timing (2026-10-09)

Static reads through fn.py, checked with narrow raw disassembly:

- 007ae010 lines100..107 finds MuzzleFlashDummy on the weapon object. Lines182..184
 and378..379 call weapon vtable+7c for fallback barrel indices when that node is absent.
 Table00b5fba0+7c points to007aac80. That routine passes template+338, flags0x3a,
 owner+0xc and the selected node to0057e820. It also spawns muzzleFlashObject+33c
 at the node's matrix. Installed muzzleflash_orange contains only an omni Light
 (colour0.8/0.749804/0.611765, range10, attenuate6) and TimeoutDeleter0.05s.
- 0057e820/0057b940/0057d370 retain object/node and emitter parameters.
 0057dd30 registers an attached emitter when flag0x20 is set. The0057f480 callback
 calls0057dca0; message5 calls0057d760, which obtains the current node matrix and
 normalizes its axes for flags&0xc==8 before00a9ea90 updates the particle system.
 Thus the muzzle particle effect follows the node with rotation and without scale.
 The port's fixed world anchor/shot orientation were wrong, especially for the cannon
 template whose five elements are local-space. Smoke has two particles, radii0.8..1,
 lives0.7..0.9s and drifts along local+z (SpSu centre0/0/1, radius0.6).
- 00747120 is only a jump to00746fc0, the TriggerableParticle loader. Missing useScale
 defaults0; missing useRot defaults1. The installed exhaust explicitly has useRot1,
 useScale0. Its node39 template v_smoke_exhaust_bumblebee has two Ejct sources,
 positions x=-0.7..-0.69 and0.69..0.7, y0.05..0.1, z+-0.01. Each rate85/s,
 SpeedCoeff0.5, MinSpeedToSpawn0.2, lifetime0.05..0.07s; growth2.5m/s,
 alpha0.4 fading from half-life, max26 particles, world-space, smk_gen01 2x2 atlas.
- 008394e0 advances the shared fractional count even when the motion gate rejects
 emission.00838740 lines17..39 compares the distance from group+50 to+30 directly
 with MinSpeedToSpawn; lines123..175 distribute new particles along that segment
 and velocity is segment*SpeedCoeff/dt. No speed-unit conversion belongs in the gate.
- 0057e3d0 is UpdateParticles, registered at priority19 by00ad5fa0 (raw).
 Its private double clock00d546f8 advances by00dade98*00dadea4;005a6d00 initializes
 the base to1/60.007797b0 writes frameRatio in mode5, and initializes it to1 in
 the ordinary path; that path's smoothed ratio instead goes to shared game-state+c4.
 0057e260 iterates the systems through0057c030/00a9f190. Template+68 defaults1
 (00aa0c30): it scales time, not a fixed update interval.00a9ff00 may throttle distant
 or culled groups through00a9d490/00964a50; near visible effects take the update delta.
 Earlier notes did not read the particle clock and their per-render exhaust threshold
 was a port limitation, not established original behavior.

[assumed] The desktop adapter uses60Hz emitter samples, interpolating between rendered
 transforms, to reproduce the nominal PC cadence independently of Bevy frame rate.
 Exact original resampling under varying frame rates needs an original timing capture.
 This assumption is also logged in status.md. No idle smoke, rate/radius/alpha override
 or replacement texture is justified by the data. The muzzle omni light, lit normals,
other-light binding, post processing, sorting/DropOffDist/nanoGraph remain open.

Port: Effect::advance samples at60Hz; Effect::step remains the direct original update
for deterministic tests. Muzzle events retain the firing barrel index. The hand weapons
use the shared posed weapon model's MuzzleFlashDummy; the car falls back to FireDummy
and FireDummy01. Running effects follow these nodes, retaining the installed local/world
space choices. TriggerableParticle useRot reads numerically with the verified default1.
Regression validation:343 core tests,3 game effect tests; installed exhaust reproduces
the prior144FPS suppression and passes at30/60/144/240FPS with both outlets, no idle
particles and natural draining. Installed muzzle smoke follows translation/rotation,
then expires without a fitted timeout. Canonical executable built. Inspected:
work/shots/exhaust_drive_20261009.png (both outlets visible,1 source/11 particles),
muzzle_fire_20261009.png (node-bound smoke), exhaust_robot_20261009.png (0/0 after
leaving the car). Matching logs in work/ have no shader/runtime errors. No original
visual comparison. Validation logs: smoke_core_all_20261009.txt,
smoke_game_tests_20261009.txt, smoke_build_20261009.txt.

## Original implementation overview

Read from disassembly and ported: `core\src\particles.rs` (the simulation, with tests),
`core\src\formats\scene.rs` (`EffectDef`: the effects on an object),
`core\src\formats\anim.rs` (`EffectEvent`: the clips' effect events),
`game\src\effects.rs` (starting and the quads), `game\src\fx.rs` and `fx.wgsl` (the
particle shaders). Since 2026-10-05 the simulation runs everything his pack's templates
hold but one source (`Intt`), and the drawing is the game's too: the quad builders and
the particle shaders were read (the sections below marked 2026-10-05). `cargo run -p
tf2-core --example effects_dump` lists every effect his pack starts, who starts it and
what of it the port does not run.

## Where it is in the executable

The library's classes carry their names (`export\vtables.tsv`, `aps*`). Every class has a
"what am I" method returning a four-letter code; the factories switch on the code:
actions `FUN_00ac13b0`, domains `FUN_00ac2190`, renderers `FUN_00ac2980`. The template's
loader is `FUN_0057c4b0` (`ParticleEmitter`'s side). An action's parameters are an array
of floats in the data's field order, `StartTime` and `EndTime` first (`FUN_008384b0`
fetches number n).

- Group (one element's particles, `apsGroup`): `+0x80` the sources' carry, `+0x84` the
  mask of values its particles have, `+0x88` a particle's size, `+0x8c + n` the offset of
  value n in a particle, `+0xac` `maxParticles`, `+0xb6` flags (bit 1 `localspace`, bit 2
  done, bits 12-15 the two alignments), `+0xcc` the particles, `+0xd0` how many.
- Values by number: 0 place, 1 radius, 2 width, 3 colour, 4 alpha, 5 a facing, 6 turn,
  7 height, 8 frame, 9 age, 10 lifetime, 14 velocity, 15 the most alpha, 16 turning rate.

## The update

System (`FUN_00a9f190`), for each element: no group yet and the time inside `start` ..
`end` (and the effect not stopping): the group is made (`FUN_00aca320`) and updated at
once. A group at or past `end` is marked, and removed with its particles at the next
update.

Group (`FUN_00a9ff00`): the actions at the head of the list that are sources run first
(skipped when the effect is stopping); then every other action is applied to every
particle; then every age goes up by the step (`FUN_00a9d000`); then the particles the
lifetime action listed are overwritten from the end (`FUN_00a9caf0`). An action runs only
while `StartTime <= time <= EndTime`, the time being the EFFECT's, not the particle's.
The same routine thins the updates of groups that are far away or out of sight
(`FUN_00a9d490`, `FUN_00964a50`); not ported.

## Sources

- `Brst` (`apsBurstAction`; count `FUN_00ac5910`, emit `FUN_00ac5c20`): if the group's
  carry is under 0, nothing; else the carry becomes -1 and it makes
  max(1, trunc(`NumParticles` x share)), share 1 when `DAT_00cfff98` is set and 0.33 when
  not. So a burst goes off once.
- `SrcA` (`apsSourceAction`; `FUN_00ac3470`): `EmissionRate` x share (1 or 0.6) x step +
  carry; the whole part is made, the fraction carried.
- Both make at most what room `maxParticles` leaves. (The flash of `r_hit_melee` asks for
  2 and has room for 1; its sparks ask for 80 and have room for 3.)
- `DAT_00cfff98` is a detail level, 0 to 2, written by `set_detail_level` (`00a92b00`) from the
  registry's `VideoQuality` at start-up (`00a96430`) and from the display menu (`00952d70`).
  The user's install is `VideoQuality` 2, so the share is 1 (settled 2026-10-20, wave 19 G3;
  the static word is 0 only until start-up writes it).
- The random draws (`FUN_0095bff0`, uniform over a range) come from one generator whose seed
  word `00d44940` is 0 in the executable; the port's effects use `Rng(0)` (game\src\effects.rs).
- `Ejct` (2026-10-05; the game's own class, not the library's: table `00b622f8`, count
  `FUN_008394e0`, emit `FUN_00838740`): a steady source for an emitter that moves. The
  count is `EmissionRate` x share (1 or 0.33) x step + carry, like `SrcA`'s. Nothing is
  made in an update the emitter came less than `MinSpeedToSpawn` in (a DISTANCE in one
  update, not a speed). What is made is laid evenly from the emitter's last place (group
  `+0x50`) to this one and leaves with (way come / step) x `SpeedCoeff`. It has 13
  domain slots and no facing or velocity slot: 0 place, 1 radius, 2 width, 3 colour,
  4 alpha, 5 turn, 6 height, 7 frame, 8 lifetime, 9 most alpha, 10 turning rate, 11, 12
  (`FUN_00838590`). Used by the exhaust, the tyres' dust, the turbo, the missile's trail.
- `RSpn` (2026-10-05; count `FUN_00ac4390`): a clock (the group's carry) runs down by the
  step; each time it is at or under 0 one particle is made and a wait drawn from the
  SEVENTEENTH domain is added, ten times at most. `EmissionRate` is not used. Its emit
  (`FUN_00ac46b0`) is taken to be `SrcA`'s, not gone through.
- `Intt` (the game's own, table `00b62310`, count `FUN_0083ab60`, emit `FUN_00839a00`):
  ported 2026-10-05 (`Kind::Intermittent`). It is `SrcA` that goes on and off: a clock on
  the group runs down by the step; when it is out, a time is drawn from domain 0 (going
  on) or domain 1 (going off), added, and it changes over; particles are made only while
  on (count: `EmissionRate` x step + carry). The other domains are `SrcA`'s slots, two
  further on (`FUN_00839830`). The damage smoke's: on 0.1 to 0.2 s, off 3 to 5 s.
  [assumed] the clock starts at 0 and off; its constructor is unresolved (the old
  `0083ae50` citation was a tree insertion, corrected2026-10-08).
- A new particle, slot by slot of the source's 16 domains, in this order (the order
  matters for the random numbers): 0 place (none: the origin; turned into the world by
  the emitter's matrix unless `localspace`), 1 radius (none: 1), 2 width (1), 3 colour
  (1, 1, 1), 4 alpha (1), 5 a facing, 6 turn (1), 7 height (1), 8 frame (0), age 0,
  9 lifetime (1), 10 the most alpha (1), 11 velocity (none; turned into the world unless
  `localspace`), 13, 12 turning rate (0), 14, 15.
- With more than one particle in an update the original spreads their places between the
  emitter's last place and this one. Not ported.

## Domains

Random numbers: `seed = seed * 0xc1c64e6d + 0x3039`, 31 bits kept; the float is the bits
`(seed >> 8) | 0x3f800000` minus 1 (`DAT_00d44940`, one seed for everything).

- `1dPt`, `3dPt`: the value. `1dBx`, `3dBx`: min + (max - min) x u, each axis its own u.
- `1dBu`, `3dBu`: the same with u bent: u >= 0.5 gives u x 2 x (u - 1) + 1, else
  u x 2 x (1 - u).
- `Crcl` (`FUN_00a9bd00`): a point on the rim, at an angle drawn evenly from 0 to 2 pi.
- 2026-10-05:
  - `Disc` (`FUN_00a9bfa0`): two numbers in -r..r, drawn again while outside the circle
    (100 tries), laid along the disc's two axes (made from the normal when it is loaded).
  - `Sphe` (`FUN_00a9c710`): x and y in -r..r, z in 0..r (the code adds a 0.0 where the
    other two add -r: it is the UPPER HALF of a ball), drawn again while outside (30
    tries), plus the centre.
  - `SpSu` (`FUN_00a9c310`): three numbers in -1..1, made a unit vector, times r, plus
    the centre. Not even over the sphere.
  - `Line` (`FUN_00a9bb40`): `Min` + (`Max` - `Min`) x one number.
- Not read: the two hemispheres (none in his pack).

## Actions read

- `Move` (`FUN_00ac0ab0`): place += velocity x step; turn += turning rate x step.
- `Life` (`FUN_00ac3240`): lifetime < age: removed (it sees the age before this update
  adds to it).
- `LnSc` (`FUN_00ac0b60`), `LScH` (`FUN_00ac6f00`), `LScW` (`FUN_00ac70d0`, read
  2026-10-05): radius, height, width += delta x step.
- `AlFd` (`FUN_00ac3310`): alpha = `BeginAlpha` while age < lifetime x `FadeBegin`; then
  (1 - age / lifetime) x `BeginAlpha` / (1 - `FadeBegin`); 0 from the lifetime on.
- `AFIO` (`FUN_00ac3090`), f = age / lifetime: f < `EndFadeIn`: f x `MaxAlpha` /
  `EndFadeIn`; f < `BeginFadeOut`: `MaxAlpha`; f < 1: (1 - f) x `MaxAlpha` /
  (1 - `BeginFadeOut`); else 0.
- `RFIO` (`FUN_00ac74c0`): the same with the particle's own most (value 15, slot 10).
- 2026-10-05 (x is the step times something; the game cuts the exponential's series
  short, and so does the port):
  - `VlDr` (`FUN_00ac6af0`), `AVDr` (`FUN_00ac6820`): velocity, or turning rate, times
    `1 + x + x²/2 + x³/6`, x = -`Exponent` x step.
  - `ExSc` (`FUN_00ac73c0`): radius times `1 + x + x²/2`, x = ln(`Exponent`) x step.
  - `ClSh` (`FUN_00ac6910`): while lifetime x `BeginShiftTime` <= age <= lifetime x
    `EndShiftTime`, the colour is begin + (end - begin) x (age - lifetime x begin time)
    / (end time - begin time) / lifetime. Outside that stretch the colour is left alone.
  - `Forc` (`FUN_00acc500`): velocity += (`AccX`, `AccY`, `AccZ`) x step; with
    `LocalSpace` the push is turned by the group's matrix first.
  - `PAtr` (`FUN_00acb5f0`): velocity -= direction from the centre x `Force` x step /
    distance², the distance² no less than 0.001.
  - `UVFr` (`FUN_00ac71a0`): frame = max(frame, `BeginFrame`) + step, wrapped inside
    `BeginFrame` .. `EndFrame`; the step is `FPS` x time, or with `FrameCycles` over 0
    (range x cycles x time / lifetime) while the particle has more than a step to live.
  - `AngT` (`FUN_00acbde0`): the turn is set from the velocity as it lies in the
    picture (an arctangent of its two components across the view, plus a constant).
    Ported as "the quad lies along the velocity", the angle arithmetic not followed.
- Not read: `Intt`'s emit, the `modifiers` lists, the actions his pack has none of
  (`Traj`, `SpAc`, `SpDe`, `PsMv`, `OMov`, `MFxV`, `LAtr`, `DLAt`, `KTau`, `EvCl`,
  `VAVD`, `EScW`, `EScH`, `WPRf`, `ATEX`, `ATEY`, `ATEZ`).

## Drawing (2026-10-05) [game]

The quad builders: `FUN_00971fc0` (plain billboard) and `FUN_0097a3c0` (colour rectangle)
were read in disassembly; the other six (`FUN_009773b0`, `FUN_00978030`, `FUN_0097afa0`,
`FUN_0096e160`, `FUN_0096ede0`) are the same routine for other vertex layouts.

- A builder gets two camera vectors in the group's frame (`FUN_00a9e0c0`; taken to be
  the camera's LEFT and UP, the names the distortion shader gives them) and, per
  particle, cos and sin of the turn x 2 pi (`FUN_00548390` is a four-wide cosine; the
  phase vector at `00c9a850` is 3 pi / 2, 0). The picture's top lies along
  `sin L + cos U`, its left edge along `cos L - sin U`: a positive turn is anticlockwise.
- Corners: place ± top x reach ± side x reach. The reach is `radius`, or for a rectangle
  `height` along the top and `width` along the side: width and height are HALF sizes (the
  port had them as whole sizes before). Each reach is times one of four numbers on the
  render node (`+0x90` left, `+0x94` right, `+0x98` top, `+0x9c` bottom), set from
  `hAlign` and `vAlign` by `FUN_008365f0` / `FUN_00836650`: 1 and 1 when centred, 0 and 2
  for an alignment of 1, 2 and 0 for 2. So `vAlign` 2 stands the quad on its place.
- A vertex is 64 bytes: place, colour, a constant vector, u, v. Top left is (0, 0), bottom
  left (0, 1), top right (1, 0), bottom right (1, 1).
- A particle whose ALPHA is under a least value (`FUN_0083aff0`'s answer, not read) is
  left out. (The earlier note said "radius": it is the fourth value of a plain billboard's
  stream, which is the alpha.)
- The render node's flags (`+0xbc`): bit 1 the velocity-tracked branch (the quad's one
  axis is the unit velocity times the group's scale, the other across it and a camera
  vector at `DAT_00cfff38 + 0x260`); bit 0 a branch that takes fixed axes
  (`FUN_00a9e2c0`). Read 2026-10-24: it copies three rows of the camera block
  (`DAT_00cfff38 + 0xd0..0xfc`), so it is NOT the renderer's `Normal`; that reader is not found.
  The port turns a `localspace` element's `Normal` by its emitter's frame (Blackout's rotor
  blur proves it in the data: the unpublished notes "rotor blur", 2026-10-24) and leaves a world
  element's in the world [assumed].

The shaders are NOT in `shaders.lsa` or `effects.lsa` (`effects.lsa` is the post
effects: bloom, depth of field, colour filter, the deferred lights). They are compiled
into the executable: `python work\lsa_disasm.py <install>\Transformers2.exe
work\shaders\exe` writes all 69 (the file names are offsets in the executable's file).

- Vertex 728708 (frames) / 72a648 (plain), feathered twins 728f30 / 72ac70, and the same
  four with a normal for the lit pixel shaders. Constants: `UVSCALE` (frames across, 1 /
  across, 1 / down), `ALPHAFADE`, `BLENDCOLOR` (the renderer's `Tint`), `FOG`. For a
  sheet the vertex has a corner number 0..3 and the frame: the two frames mixed are
  floor and ceiling of the frame, row = frame / across whole, and the pixel shader gets
  the fraction. Alpha = colour's x saturate(depth x `ALPHAFADE.x` + `ALPHAFADE.y`) x
  `ALPHAFADE.z`; `FUN_00a9dce0` makes x = 1 / (end - start), y = -start / (end - start)
  from `AlphaFadeStart` / `AlphaFadeEnd` (0 and 1 when they are equal): a fade IN with
  the depth in front of the camera.
- Pixel 729968 / 72b4c8: texel (the two frames mixed by the fraction) times the colour;
  rgb mixed toward `FOGCOLOR`, both cut off at 1.
- Feathered pixel 729db8 / 72b8d0 (used when `ZFeatherDistance` > 0 and the scene's depth
  is at hand, `FUN_00973060`): alpha times mix(saturate((scene depth - own depth - 0.0005)
  x 1000), 1, saturate((scene depth - 0.99) x 100)). The feather DISTANCE goes to the
  vertex shader (`Z_FEATHER_RANGE`) and the pixel shader does not use what is made of it.
- Lit pixel 729ad8 / 72a020 (`AMBIENT_COLOR`, `DIFFUSE_COLOR`, `DIR_TO_LIGHT`,
  `INV_COLOR_COEFF`): light = diffuse x max(N.L, (1 - |N.L|) x (1 - alpha) + back-lit) +
  ambient + the inverse colour. Ported 2026-10-05 (`fx.wgsl`, flag `LIT`):
  - WHO: a renderer whose `BlendMode` is 3. The node's draw (`FUN_00acdc00`) asks the
    renderer (vtable `+0x18`, `FUN_0095c310`: `+0x44 == 3`) and takes the builder with
    normals (`FUN_00971170`, binding `FUN_00972da0`) or the plain one. [game]
  - Exactly, with a = texel alpha x colour alpha and d = N . `DIR_TO_LIGHT`:
    light = saturate(max((1 - |d|)(1 - a) + `INV_COLOR_COEFF.w`, max(d, 0)));
    rgb = texel x colour x (`DIFFUSE_COLOR` x light + `AMBIENT_COLOR` +
    saturate(`INV_COLOR_COEFF.rgb`)).
  - The constants (`FUN_00973310`): the direction is on the render node (`+0xc0`);
    diffuse = the renderer's `DiffuseCoeff` x the node's `+0xd0`; ambient = `AmbientCoeff`
    x `+0xe0`; the inverse colour = (1, 1, 1, `BackLitCoeff`) - (`AmbientCoeff` +
    `DiffuseCoeff`). [game] The coefficients' fourth numbers are0: `005431f0`
    explicitly copies RGB with zero w (checked2026-10-08), so w is the back-light.
    [assumed] the node's three vectors are the level's sun direction, sun colour
    and ambient (who fills them is not read): the port gives it the borrowed level's
    (`world::level_light`).
  - [assumed] the corner normals: the way to the camera leant toward the corner (the
    builder's arithmetic was not followed).
- Distortion 731560 / 731810 (`Dstr`, `IsShimmer`): bends the picture behind. Ported
  2026-10-05 (`warp.wgsl`, `fx::Warp`): the picture is a map (red, green: which way,
  0.5 none; alpha: how much); push = (0.5 - rg) x a x the faded alpha; the pixel's
  place in the world is moved by A x push.x + B x push.y (A = left x n.y - up x n.x,
  B = left x n.y + up x n.x) and the scene is read where that lands on the screen.
  [assumed] the vertex's two numbers n are the particle's radius. [stand-in] the scene
  is Bevy's copy taken before the see-through things (smoke behind the haze is not
  bent). Drawn without error in `work\shots\fx6_turbo_warp.png`; the bend itself is too
  slight there to be sure of by eye.
- Blending (`FUN_00a9db20`, by `BlendMode`): 0 and 3 source alpha / inverse source
  alpha; 1 source alpha / one (adds); 2 the same, subtracted; 4 one / zero. Alpha test
  on, culling off.

In the port (`game\src\fx.rs`, `fx.wgsl`, `effects::quads`): the quads as above; the
depth fade on the vertices; the frame mix; additive and alpha blending; the feather
against Bevy's depth prepass. [assumed] the two camera vectors; the original's near plane
(0.2, ours), on which the feather's depth scale hangs; a frame is handed over whole
unless the renderer has `UVBlending` (where the builders use that switch was not found);
a tracked quad's picture points along the velocity. [stand-in] Bevy blends in linear
light. Done since (2026-10-05): the lit shaders and distortion (below).
Fog/cutoff completed2026-10-08 below. Not done: `bloomCoeff`, `DropOffDist`, the
`nanoGraph`, sorting.

## Who starts an effect

- A hit: `FUN_0077cd60` (see `melee.md`): the impact preset's `effectOnImpact` by the
  surface; with flag bit 8 in the frame of the object hit. His: `r_hit_melee` (a flash, an
  electric flicker, three sparks).
- `EffectSpawner` on a node (loader `FUN_007fb2b0`, trigger `FUN_007fb470`, follow
  `FUN_007fbf50`): a trigger message with its `LuxScriptName` puts its `effectProxy`
  object at the node. Switches, each ON when absent: `removeTilt` (`+0x50`),
  `removeFacing` (`+0x51`), `removeScale` (`+0x52`), `useRootFacing` (`+0x53`),
  `attachToBone` (`+0x54`: the object's place follows the node every update). The proxy
  is a small object: a `ParticleEmitter` (the template), often a `RenderThing` mesh with
  an `AnimBundle` clip, and a `TimeoutDeleter` (`time`, `fadeStart`).
- `TriggerableParticle` on a node: the template alone, with `useRot` and `useScale`. Its
  code (loader `FUN_00747120`) is not read; taken to follow its node.
- His ground punches, both on `Hand_R`, all upright:
  - weak (`FX_groundpunchweak`): a spawner, not following: `r_attack_radial_small` (two
    rings of dust) and the mesh `R_RadialAttack_small.RadialAttack_small`, scaled 1 to
    2.2 over 20 ticks, gone at 0.5 s, fading from 0.2.
  - strong (`FX_groundpunch`): a spawner with a mesh alone
    (`R_RadialAttack_Optimus.RadialAttack_Optimus_a`, 70 ticks, gone at 0.7, fading from
    0.5), and a `TriggerableParticle`, `r_attack_radial_optimus` (bolts, sheet lightning,
    the two dust rings).
  - The meshes' material: technique `dull_fx_emissive`, additive, `radial_noise.dds`,
    with edge and scrolling parameters nothing here uses.

## Who starts the rest (2026-10-05)

- `AnimSpawnEffectEvent` / `AnimStopEffectEvent` on a clip (loader `FUN_00747410`), 68
  and 5 in his set: the template is in the event (`effect`), the event sits on the
  node's channel. With no `eventId` and no `trackObject` the effect is put into the
  world at the node once (flag 2, `FUN_00747d70`); otherwise it is tied to the node
  (`FUN_0057e820`) and, with an `eventId`, kept under that id for a stop event.
  `useRotation` (on when absent) gives it the node's turn, `useScale` its scale.
  [game]
  CORRECTED 2026-10-05 (the user saw the charge's lightning missing during the charge and
  stuck on after it): both ARE used. `allowReRun` (`FUN_0053ab90`, the animator's event
  runner, every kind of event): an event with it at 0 runs only in a looping clip's first
  time round. `autoStop` (`FUN_0070ccd0`, the reference's loader): a spawn event with it
  on has its id put in a list on the reference (`+0x50`; sounds likewise at `+0x5c`,
  where absent counts as on). [assumed] the list is walked when the clip is left, ending
  those effects; that code is not read. The charge-up (`r_melee_chargeup`, id 349366fd,
  node `FX_hand_R`) is in `AttackFast_3_ChargeLoop` at tick 2 with autoStop 1 and
  allowReRun 0, and its only stop events are in `AttackFast_1` (tick 0) and
  `AttackFastMP` (tick 4): the port restarted it every loop (each restart ending the one
  before it had grown visible) and never ended the last. Ported: `EffectEvent::rerun`,
  `Spawn::auto_stop`, `SoundEvent::rerun` (core `formats\anim.rs`), `Effects::clip_changed`
  and the `rerun` argument of `clip_events` / `clip_cues`. Checked by captures
  (`work\shots\chg_during.png`, `chg_after.png`). Camera shake events do not yet honour
  `allowReRun`.
  These are the landings' dust (`r_contactdust_landing`, `_mediumland`, `_biglanding`),
  the change of form (`r_bumblebee_trans` on the slam, `r_transitionfx_generic`,
  `r_contactdusta`), the dodge's jets, the charge-up on the hand, the deaths. Ported.
- `FootStepEvent` (handler `FUN_0073f780`): sends his object a message with the side;
  what the character does with it (`FUN_00724940`) is not read. The step preset
  (`footStepFX`, `StepAutobotSmallPreset`) has an effect per ground. Ported as: the
  preset's effect for the default ground, on the ground under the foot the event sits
  on. [assumed] `Run` has no such events in his set (`Walk` and the landings have).
- On his objects, by message (`effects_dump` lists them): `FX_turbocharged` (the car,
  node 39), `FX_smoke_exhaust` (39), `FX_exhaust_smoke_skid` (40, 41), `specialAttack` /
  `specialAttackVehicle`, `FX_regenerate`, `FX_heal`, `FX_stun`, `FX_frost`,
  `FX_Damage_1..3`, `overdrive`, `eggshield`, `allspark1`. What sends each is not read
  but for the special and the punches. Ported with assumed senders: the turbo's start
  sends `FX_turbocharged`, and `FX_smoke_exhaust` runs while he is the car. The turbo's
  template has no end (its sources run for ever), so something must stop it: the port
  stops it when the turbo runs out or he leaves the car [assumed] (before this fix,
  2026-10-05, its fire stayed on the robot after the change of form, as the user saw). A
  stopped effect's group is cleared once its last particle is gone. With the
  exhaust's `MinSpeedToSpawn` of 0.2 a update and the port stepping particles once a
  frame, it smokes above 12 a second at 60 frames a second; the original's particle step
  is not read.
- The weapons (`character::WeaponEffects`): `muzzleFlashEffect` on the weapon's thing;
  a gun's `bulletProjectileObj`'s `BulletTrace.impactFX` (a `MaterialImpactPreset`,
  flags 0x11: z along the surface's normal, a random turn about it); the projectile
  object's own `ParticleEmitter` (a trail) and its `DamageLink` object's
  (`w_exp_micro_missiles2`, the missile's explosion). [data] Who starts them is not read;
  ported as: the flash where a round leaves, x along the shot; the impact where it lands;
  the explosion where a missile ends; and (2026-10-05) the missile's trail: its emitter
  follows the missile, x along its flight, and makes no more once the missile ends
  (`effects::Anchor::Missile`). Car-gun BulletTrace visual trails were added 2026-10-07,
  independently of instant damage (section below). Overheat smoke uses weapon clip events.
- Not started at all: heal / stun / frost / overdrive /
  eggshield / allspark (see "The senders": nothing in this build heals, stuns or freezes
  him).

## The senders (2026-10-05): who starts and stops the effects on his objects

Found with `work\find_imm.py <the name's CRC-32>`. A message is {name, 0, 0, `Trigger`
or `9caa723d` (stop)} given to `FUN_00724940` on his object and again on the object at
`+600`. Ported as `core\src\fxstate.rs` (`kept`: the names that are on) and
`game\src\effects.rs`.

- DriveMode enter (`FUN_0085e130`): Trigger `Headlight_L`, `Headlight_R`,
  `FX_smoke_exhaust`. Exit (`FUN_0085ed50`): stop `TireSkidMark`, both headlights,
  `FX_smoke_exhaust`. [game] (So the exhaust "for as long as he is the car" was right.)
- `VehicleHeadlight` (class string `00b618b8`, trigger `FUN_00815790`): on `Trigger`,
  if the level allows headlights (a byte at `+0x100` of the level's entry in the list
  `00dad9a0`), its `spawnObject` (`fx_v_headlight`: a `ParticleEmitter`, `v_headlight`)
  is spawned as `FX_headlight` and tied to the node `linkName` names. Its stop
  (`FUN_008158a0`) DELETES the spawned object, so the glow goes at once (the port let
  its particles live out, and they never die: the user saw the car's two beams on the
  robot, 2026-10-05; fixed, `Running::gone`, capture `fx7_lights_gone.png`). Ported as an
  effect on that node (`scene.rs`). [stand-in] the level's switch is ON here
  (`Effects::headlights`) so they can be seen; a daylight level may well have it off.
- The turbo: `FUN_00733290` / `FUN_00733ae0` (character virtuals, vtable `00b5d62c` /
  `00b5d640`), given on / off: Trigger or stop `FX_turbocharged`, and the environment
  `Environment_SpeedBlur` is looked up (`FUN_0056a790`) and switched. [game] RESOLVED
  2026-10-07: the standard/drift ability vtables00b5d61c/00b5d630 slot4 dispatch these
  from `00732e60` and timer expiry. DriveMode exit stops FX but keeps the standard
  timer registered. `sim::State::turbo_fx_on` preserves those independent messages;
  drift start now plays sound/rumble/FX too, and timer activity does not restart FX
  after changing out of the car. The
  speed blur itself is ported 2026-10-07 (`docs/shaders.md` "Turbo speed blur port"),
  driven by that independent FX latch rather than the remaining turbo timer.
- The timed ability's own routine `FUN_007325e0(on)`: on: Trigger `FX_brake`, stop
  `FX_smoke_exhaust`; off: stop `FX_brake`, Trigger `FX_smoke_exhaust`. Called with on
  only by FlyMode (`FUN_00864320`); the base ability calls it with off when its timer
  runs out (`FUN_007324d0`). CORRECTION 2026-10-07: this is the base timed ability;
  DriveMode constructs the standard turbo's subclass (`00733180`, different virtual
  callback) and the drift subclass (`00733e10`). His car has no `FX_brake`.
  The flyers (2026-10-20, ported in `fxstate::kept`): in flight the exhaust is first sent by the
  take-off clip's `AnimSendTriggerEvent` (the jets' vehicle-model `Trans_R2V_B`, the robot flyers'
  `FlySet/Trans_Robot2Vehicle`), then this routine's on / off around the fly brake; HoverMode sends
  `FX_hover` (00735bb0, read: on in HoverMode = Trigger `FX_hover`; otherwise stop `FX_hover`, and
  Trigger `FX_hover_sput` or stop it by the second argument). `0x9caa723d` is the stop message.
  the unpublished notes "Flyers' vehicle-mode effects".
- `FX_exhaust_smoke_skid` (the two `TriggerableParticle`s by his rear tyres): triggered
  only by FlyMode's enter (`FUN_00864d60`); FlyMode's exit stops `FX_exhaust _smoke_skid`
  (with a space: hash `9176b0a4`, which no thing answers to). Never sent for a car: the
  rear tyres' smoke he does have is the tyre preset's (above).
- The damage smoke (`FUN_0072a270`): level = 0 under a quarter of his health, 1 under
  half, 2 under three quarters, else 3 (kept at `+0xa0`). Falling (or forced): Trigger
  `FX_Damage_3` at level 0, `FX_Damage_2` under 2, `FX_Damage_1` under 3; rising: each is
  stopped again. [game] Ported; its source `Intt` too (above).
- `FX_regenerate`: `FUN_0071fdd0` (not regenerating, health under the most: a sound
  `c2c85c6b`, Trigger, flag `0x400000` of `+0x14c`), `FUN_0071ff00` (stop; called when he
  is hurt, `FUN_00719830`, and from the health update `FUN_0071ffb0`). [game] Ported from
  the port's own regeneration state (`since_damage`); the sound is not played.
- `FX_heal` (`FUN_0072d2e0` / `FUN_0072d410`), `FX_stun` (`FUN_0072c120` /
  `FUN_0072c340`), `FX_frost` (`FUN_00729120`, `FUN_0072ac70`), `overdrive`
  (`FUN_00736060` / `FUN_007362c0`), `eggshield` (`FUN_0072cca0`), `allspark1`
  (`FUN_0072cb30`), `FX_heal_OptimusF_special` (`FUN_007310b0`): found, NOT read: each
  belongs to a state this build cannot reach (another character's heal or freeze, an
  enemy's stun, multiplayer pickups). `FUN_008b7c70` numbers these names 0 to 10 and
  `FUN_008b7d30` turns the numbers back (a network message, by the look of it).
- The weapon's overheat smoke (`w_overheat_smoke_small`): an `AnimSpawnEffectEvent` of the
  weapon OBJECT's own overheat clip, on the weapon's node. [data] Ported:
  `Effects::thing_clip_events`, fed by `weapons::show_models`; capture
  `work\shots\fx5_overheat.png`. [assumed] the object's clips run their effect events
  as his own do.
- Climbing (`docs/climbing.md` has the movement): a clip's `ClimbStepEvent` carries a
  `side`; the `ClimbStep` thing of his with that side (five, on the hands and feet;
  handler `FUN_0073fa20`) casts a ray 1.5 either way along its node to find the wall and
  sends the character a step with the point, the normal and the surface; the character
  (`FUN_00718720`) starts the limb's preset's effect for the surface there
  (`FUN_0077cd60`). Sides to presets: `85e60aab` `climbLeftHandFX`, `65091969`
  `climbRightHandFX`, `6b0b71b7` `climbLeftFootFX`, `8be46275` `climbRightFootFX`,
  `85bb2cd2` `climbSlideFX` (`+0x348` to `+0x358`; the side names are not in the files).
  [game] Ported (`anim::CLIMB_SIDES`, `CharacterData::climb_impact`,
  `ObjectDef::climb_steps`). [stand-in] at the limb's node, upright, default surface
  (no ray; the arena's walls have one material). `spawnSlideEffectOnThisBone` and
  `offsetFromWall` are not used.
- A footstep (`FootStepEvent`, `FUN_0073f780`) sends Trigger with its `side` as the
  name (`439eb7a4`, `4cf20adc`: the two feet); the receiver of those two was not found
  (`FUN_00716890` uses `439eb7a4` to tell the feet's IK apart). The port's step dust
  stays as it was. [assumed]

## Skid marks and the tyres' effects (read from code and ported 2026-10-05)

`TireSkidMark` (class string `00b61594`, vtable `00b615a4`; ledger `skidmark_*`). His car
has four, on the effect nodes 40 to 43 (`FX_smoke_exhaust_skid`, `..01` to `..03`), each
with a `tireName` (`Tire_Rear_L`, `Tire_Rear_R`, `Tire_Front_R`, `Tire_Front_L`),
`skidPointLength` 2, `skidPointWidth` 0.3, `skidPointLifetime` 10, `skidColor` 191 x 4,
material `bumblebeevehicle.fx_tireskid01` (id `ace50b6e`: technique `dull_vc`, blend
Normal, picture `fx_skidtexture01.dds` 128 x 256 DXT5). The constructor's defaults are 5,
0.25 and 10.

- Init (`FUN_0080acb0`): joins a global list (`00c06cf8`; its walkers `FUN_0080bcd0`
  update and `FUN_0080bd10` draw every mark), allocates 50 points, finds its wheel by
  `tireName` (`+0x13c`). A point: place, the clock when put down, a "strip starts here"
  flag (`+0x10`), a free flag (`+0x11`), next, previous.
- `Trigger` (vtable `+0x18`, `FUN_0080bb30`): if not going, going (`+0x44`), and a first
  point at the tyre's NODE (the one `tireName` names, `FUN_0080bc30`), its z the wheel's
  contact point (wheel `+0x14`) + 0.01, with the start flag. The stop (vtable `+0x1c`,
  `FUN_007ca780`) clears `+0x44`.
- Update (`FUN_0080afa0`): going, with a point, and the wheel on the ground (`+0x90`):
  when the node (x, y) and contact z + 0.1 are MORE than `skidPointLength` from the last
  point, a new point; its start flag is set when the wheel was in the air the update
  before (wheel `+0x92`).
- A new point takes a free one, else the oldest by its clock (`FUN_0080bbe0`); the point
  after a removed first one gets the start flag (`FUN_0080bab0`).
- `skidPointLifetime` (`+0x54`) is loaded and read by NOTHING in the class (disassembly
  `0080aca9` to `0080bdc0`): a mark does not fade with time. It goes only as its 50
  points are taken again, so each tyre keeps at most about 100 m of marks.
- Draw (`FUN_0080b0f0`): for each point without the start flag, a quad back to the point
  before: side = (dy, -dx, 0) of the unit step x `skidPointWidth`, to BOTH sides (0.6
  wide). A quad after another starts on the corners that one ended on. Colour
  `skidColor` / 255; alpha 0 at a strip's first point and at its last (so a strip of one
  quad is invisible and the newest end always fades). uv (0,0) (1,0) (1,1) (0,1): the
  picture once over each quad. Normal +z. Triangles 0 1 2, 2 3 0.
- Who sends the messages: `FUN_007347c0`, in its loop over the wheels (`docs/sound.md`
  "The tyres and the suspension"), by the class's name `TireSkidMark` to his object (so
  to all four): `Trigger` while skidding above 1 m/s on a surface, `9caa723d` (name
  unknown) when not skidding or slow.
- The tyres' particle effects, same loop: per wheel a kept handle for the skid's effect
  (`+0x30 + wheel x 8`) and the road's (`+0x50 + wheel x 8`). With none kept, or when
  the wheel's surface changed, `preset_effect_by_surface` starts the preset's effect for
  the surface at the wheel's node (found by the wheel's name, else `Tread_Front_L`,
  `Tread_Front_R`, `Tread_Back_L`, `Tread_Back_R` for wheels 0 to 3); it is stopped when
  its condition ends (skid: skidding and over 1 m/s; road: over 1 m/s; both: a surface).
  His presets [data]: skid `v_smoke_exhaust_skid_bumblebee` on the default surfaces,
  `v_dirt_skid`, `v_water_skid`, `v_mud_skid`, `v_sand_skid`; road `v_asphalt_constant`,
  `v_dirt_constant`, `v_water_constant`, `v_mud_constant`, ...; every entry's flags are
  0x100 (tied to the object).

Ported: `core\src\skidmark.rs` (five tests), `ObjectDef::skid_marks` (`formats\scene.rs`;
data test `his_car_has_a_skid_mark_and_smoke_for_each_tyre`), `game\src\skidmarks.rs`
(the strips), `game\src\effects.rs` (the tyres' effects, `Running::tyre`). Capture:
`work\shots\skid_a_4.5.png` (`--vehicle --aim-from 2.5 --stick-x 0.6`): four curved marks
behind the sliding car and smoke at the tyres. Not compared with the original.

Assumed / stand-in:

1. [assumed] `9caa723d` reaches vtable `+0x1c` (the base's dispatcher `FUN_00812750` is
   not read; `FUN_007ca780` is the only code that clears `+0x44`).
2. [assumed] an effect "tied to the object" (flag bit 8) follows the wheel's node,
   upright. Settled by reading `FUN_0077d5d0`'s bit-8 branch.
3. Replaced 2026-10-07: dull_vc vertex/texture/diffuse/opacity arithmetic and UV transform
   are ported in Surface. See `docs/shaders.md`; its existing light rig remains a stand-in.
4. Replaced 2026-10-07: independent wheel contact height/ground/history, each update's
   tyre samples and binding by tireName. [stand-in] tyre nodes use the last rendered transform.
5. [stand-in] one surface (`0 - Default`) under every wheel, as for the sounds.
6. [assumed] out of the car the marks are stopped and stay (what the change of form does
   to the car's things is not read).

## Vehicle collision and bullet visuals (2026-10-07)

`core/src/collisionfx.rs` implements CollisionTrigger's keyed ten-update history,
roll/scrape thresholds +/-7 and .25s expiry (`007dab90`, `007da7b0`). CollisionFX
(`007d9e40`, `007d94d0`) picks hard/soft impact/roll/scrape particles by physics
material, ends/replaces kept loops and moves contact offsets at most .01/update.
`0056e7b0` aligns effect +z to normal with a projected +y reference, preserving twist.
The material registry (`011964c0`, loader `00757c00`) defaults missing hardness to soft.
Installed Bumblebee data resolves all six templates; all six SFX overrides are zero.

`00782310` moves BulletTrace visuals at bulletSpeed until the endpoint is crossed,
separately from instant damage. Its object's emitter follows; instantKillBulletFX
deletes it with the projectile. `core/src/weapons.rs::BulletVisual` and
`game/src/effects.rs::Anchor::Bullet` now replace the car gun's fitted line tracer.

Assumptions are logged in status: arena contacts supply initial push3 then slide5;
the original lower physics dispatcher remains unread. Async bullet endpoints use the
host's synchronous trace. Skid object tint/culling and the shared light rig are open.
Turbo speed blur is ported 2026-10-07 (`docs/shaders.md`); other original post passes
and speed-blur render-order/colour-domain assumptions remain open. No
original-game vehicle FX/audio capture exists yet; the new recorder supplies those
observations when the user runs it.

## Particle renderer completion (2026-10-08)

Read through fn.py and narrow raw disassembly:

- `0083aff0` returns `00b8d220`, default0.05. The quad builders compare raw
  particle alpha>=threshold before tint/depth fading. The port filters the draw
  list only, leaving particle simulation/lifetime intact.
- `00a9b620` sets node fog bit3 unless renderer+44 (BlendMode) is1 or2.
  `009736a0` builds FOG=(slope,intercept,min,max), with slope=(max-min)/(far-near)
  for far>near, else0/intercept=min. Disabled fog supplies all zeros.
- `0083afb0/afc0/afd0/afe0` are global near/far/min/max accessors.
  `00aa0500` stores colour, near inXMM0, far first stack argument, min0 and max
  second stack argument. `0057e670` supplies them from shared game-state+194 colour,
  +1a4 near,+1a8 far,+1ac maximum.
- `728708_vs` computes camera-forward depth from clip w, clamps depth*slope+offset,
  and interpolates it to the pixel shader. `729968/729ad8/729db8/72a020` mix RGB
  toward FOGCOLOR after lighting; alpha is independent. `00a9dce0` supplies the
  alpha fade slope/offset, with0/1 for equal endpoints.

Implemented in core particles and game Fx: cutoff, fog mode selection/constants,
per-corner vertex alpha fade/fog and pixel RGB mixing. The Fx material cache includes
the fade endpoints. Warp retains its previous CPU centre-depth fade.
Existing [assumed] environment mapping fogHotspot/range/falloff is retained until
the transfer into shared game-state is read; the loader does scale falloff by0.01.
No original screenshot/recording comparison is available. Validation:341 core tests,
9 affected game audio tests pass; canonical game builds. Inspected
`work/shots/effects_fire_20261008_a.png` (impact/muzzle),
`effects_climb_20261008_a.png` (debris at hand/wall),
`effects_skid_20261008_a.png` (lit smoke), `effects_walk_20261008_a.png` (walk).
Runtime logs contain no shader error; the existing gamepad-id warning persists.

Foot/climb dust: `EffectEventKind::FootStep` now retains side, and the installed
Midfoot attachments supply node/height instead of the event's Foot node/root height.
`stepfx` shares final-pose contacts with audio. A missed probe suppresses dust;
material comes from the contact. Climb slide reaches all four components. Installed
step presets have flags0x100 (attach to the struck object, static arena here),
climb presets have flags1. `0077c180`'s flags1 use +z along normal and +y from
projected world up, with +y reference if |normal.z|>0.999; this differs from
CollisionFX's reference axes. This removes the previous missing-ray/root-height
stand-ins. [assumed] synchronous final-pose callbacks adopt the already logged
audio timing assumption; original async scheduling remains open. Geometry/material
diversity and moving hit objects remain beyond this authored arena.
`00973310 ->005431f0` explicitly zeros coefficient w; that old assumption is resolved.

Static side findings, not claimed fully read: `0057c4b0` defaults bloomCoeff0.5,
rounds clamp(coeff*255,0,255) and stores it in renderer+14 high byte. Bevy bloom
still has no equivalent scene-alpha channel carrying this value. `0083ae50` is
a tree insertion, so the older note citing it as the Intt constructor is incorrect;
Intt's initial-off timing assumption is unresolved. `00971170`'s normals remain
unresolved. These are next renderer debts along with sorting, DropOffDist and nanoGraph.

Other lookup scope:00563580/70/60 set vector y/z/w fromXMM0;005722a0 transposes
a4x4 matrix;0083b2f0 is an alternate fog binder;00ac0c90 queues particle draw
records by priority sign.005a6f20's fog-input slice reads near1a4/far1a8/alpha1ac
and colour194, but its remaining body was not reread.005a7f10 copies the render
camera-input block.00585880 was inspected as an outline only;00593040 only in
matched plane-field context. Their wider meanings remain unread.

## Gun smoke cleanup (2026-10-08)

User report: smoke remains at the gun after firing. The installed shared weapon
object has Overheat_WeaponA/B (nonloop) and Overheat_Weapon_A/B (loop) references.
Smoke spawns on OverHeatDummy at ticks2/0, ids898679215/180175782; explicit stops
are at78/158 in80/160-tick clips. autoStop is FALSE for all four references.
The text export's enum labels on boolean fields are misleading: ANIM_TYPE_POS means
zero here. The Rust decoder and regression checks read the numeric fields directly.

Two adapter problems: show_models reset the event window to zero on wrapping, losing
the previous loop's stop key; leaving the overheat clip could skip its only stop.
The adapter now passes the previous phase through wraps. Effects records the weapon
object root as event owner and scopes replacement/stops/clip exits to that owner.
Stop events do not require resolving an animation node. autoStop is retained for
object events. [assumed] Weapon effects paired with a later explicit stop also end
when interrupted, rather than emitting forever. This interruption rule is an
assumption, not an original-code finding; see status.md for what would settle it.
Particles already emitted live out using the installed template, without an invented
particle timeout or immediate deletion.

Static checks through fn.py (ledger updated):007c2ea0 lines115-284 collects effect
eventIds only for nonzero autoStop;007c3870 lines10-54 walks the outgoing reference's
effect list via0074ae10 (which gets the object's effect attachment and calls00747bf0).
The other two lists are sound events through005987e0, not effects.00747bf0 finds the
keyed effect, calls0057e000 and removes the entry.007c4170 disposes a completed
outgoing nonloop player via007c4050 and starts a queued next clip/hides when requested.
007ab320/007ab3e0 send exit-overheat/overheat clips to the object and its partner;
007c35d0 releases the three event-list heads. These do not prove paired-stop handling
on an interrupted autoStop=0 clip.

Regression checks in game effects cover all four installed smoke references, live
particles draining on interruption, stop keys across loop wraps and missing nodes,
and effect/clip id isolation between character and weapon objects.

Validation:341 core tests and2 game effects regressions pass; canonical
target/debug/bumblebee.exe builds. Captures inspected:
work/shots/gun_smoke_during_20261008.png (6s, fire1..6.5s):1 emitting effect,
11 particles; gun_smoke_stowed_20261008.png (10s, same input):0 emitters/particles;
gun_smoke_cooled_20261008.png (16s, aim held from1s, fire1..6.5s):0 emitters/particles.
Matching work/gun_smoke_*_20261008.log files contain --fx-stats diagnostics.
The existing gamepad-id warning remains. No comparison to the original was made.

## Checks

- `cargo test -p tf2-core particles` (twelve tests: the generator, a burst, a lifetime,
  a steady source, world against local space, the circle, the fades, the round domains,
  the ejector, the later actions, the random spawner, the alignments).
- `character::tests::his_hit_and_punch_effects_are_read_whole` and
  `his_clips_steps_and_weapons_name_their_effects` (need the install).
- Captures of 2026-10-05, `work\shots\fx3_*.png`, `fx4_land.png`: a hit, the weak punch
  (`fx2_punch.png`), a landing's dust, the cannon's flash, the special, the turbo. NOT
  compared with the original.
- Captures: `work\shots\fx_hit_0.95.png` (a hit on a dummy), `fx_punch_1.86.png` and
  `fx_punch_1.92.png` (the weak punch). The strong punch is reached only from the
  transform attacks and has not been captured. Nothing is compared with the original:
  there is no recording or picture of these effects from the game.
- `work\particles.py <pack> [name | --survey]` prints templates; `work\tex_peek.py` splits
  an exported texture into colour and alpha.
