# Surface damage (2026-10-06)

The next task from status item 1. Static analysis of the user's executable and packs;
the original game was not run.

## Placement and lifetime [game]

`0077cd60` (all material impact effects) and `0077da20` (SSD only) pick a random
`ssdInfos` entry independently of `materialInfos`. Water and object flag bit 25 suppress
SSD. Centre = contact - normal * (abs(normal.z) >= 0.1 ? 0.2 : 0.5).
`007e60f0` invokes the SSD-only path once per contacted object sending DETECT (0x100),
without mask bit 7. This is separate from its PUNCH damage contacts.

`0059ddd0` loads SSDInfo: current/initial radius at +0xc/+0x10, maximum = 5*Radius
at +0x14, NumFrames >= 1 at +0x18, noise ratio +0x1c, chaos +0x20, four
threshold/softness pairs +0x24..+0x40. Noise scale +0x44 = 1/(Radius*ratio).
Frame +0x54 starts at 1; initial seed is -1, replaced with an incrementing id by
`0059ecd0`. `01178c90`/`0059f250` draws first, then increments the frame up to
NumFrames and recalculates constants. There is no lifetime fade.

`0059ecd0` repeatedly merges the newest sphere with the sphere having greatest
positive r1^2+r2^2-distance^2. `0055f250` is the enclosing sphere calculation;
`0059e8d0` keeps the larger source's thresholds, noise scale, seed and radius cap,
caps the union radius, and preserves the greatest frame fraction (rounded) using the
older record's frame count. At more than 512 records remove the farthest from the eye.

## Drawing [game]

`0059ea30` loads NoiseVolume2D from textures/noisevolume.apk (packed in the install)
and `destruction_sphere` (91bf7977), effects.lsa Base: vertex 00ee80, pixel 00efa0.
`0059f970` draws a screen SSD buffer using scene depth; `0059f830` selects sphere
culling according to camera position. Callback 0059f4d0 sets ONE/ONE additive blending.

`0059e190`: every threshold gets +0.133*(1-frame/NumFrames). Each ramp is
saturate((value - threshold + softness)/(2*max(softness,1e-7))).
The shader averages NoiseTex red sampled on the transformed xy, xz, yz planes (0.33333).
Let edge = saturate(1-distance/radius), feather = saturate(5*edge),
outer = edge+0.5+(noise-0.5)*Chaos. SSD red = feather^2 * outerCracks(outer)
* innerCracks(noise); green = feather * outerDamage(outer) * innerDamage(noise).
The surface shaders (e.g. shaders.lsa 037e60) use
remaining = (1-SSD.r*Cracks_Map_Texture.r)*(1-SSD.g), then mix the original colour
with Damage_Map_Texture by remaining.

## Port scope and qualifications

`core/src/ssd.rs`: data, placement, frame ramps, masks, merging/cap, arena contacts.
`game/src/ssd.rs` and `ssd.wgsl`: the mask arithmetic on arena surface positions,
with runtime-loaded game textures. Material extensions apply the masks before Bevy lighting.
This evaluates the same world-position masks directly instead of building a depth-based
screen buffer. Arena boxes and ground remain stand-ins for the level's collision mesh.

The seeded noise transform's Euler/translation assumption was replaced on 2026-10-07
("Seeded noise transform checked" below). The noise sampler is read:
0059f4d0 -> 01174460 -> 00546c20 sets wrap U/V, bilinear min/mag, point
mip filtering, anisotropy 1. NoiseVolume.apk is loose in Textures, id 1881f96c,
128x128 DXT1 with eight mips; the shader preserves mip selection with explicit gradients.
Stand-in: borrow one level material's crack/damage textures for all arena surfaces; arena
surfaces have no original materials. Dummies are excluded because their cylinder is a
character stand-in. Surface collision contacts use the existing primitive contact solver.

Data checked by core/examples/ssd_check.rs: the fist preset is SSD_Step_S (radius 2,
10 frames); both guns SSD_Default_S (radius 2, 6 frames); missiles AND both ground
punches use SSD_MicroMissle (radius 7, 4 frames). Special shockwave has no SSD preset.
These are read at runtime. Projectile SSD reader follows nested DamageLink/DetachLink.
The existing explosion particle reader still only follows the first DamageLink; fixing
that unrelated reader belongs with the particle leftovers.

Surface contacts from hands and the ground-punch explosion are emitted once per
attacked object by the core simulation, even with no AttackRef damage record. Missile
surface contacts are stepped per frame alongside the host's missile damage spheres.
All arena collision surfaces stand in for DETECT-capable objects; no per-surface masks
or water exist in the arena yet. The core suppression gates are tested separately.

## Seeded noise transform checked (2026-10-07) [game]

Replaced the XYZ/inverse-transform assumption with the matrix helpers' arithmetic.
Narrow disassembly: 00570470..00570608; 0059e31b..0059e5ac;
005a0a00..005a0a6b; 0053059d..005306d8. The function ledger records the helpers.

- 0040b080 and 0040b0a0 return float32 cosine and sine.
- 00570470 constructs positive column-major Rz(z), Rx(x), Ry(y); its two
  0052ed70 calls compose Rz * (Rx * Ry). The packed input is x, y, z.
- 0059e190 draws six MSVC-LCG samples. It converts the first to
  `(u1*360)*f32(0x3c8efa35)` radians; y/z use `(u*180-90)*that constant`.
  Samples 4/5/6, multiplied by 1000, go to translation Z/Y/X, respectively.
- 005a0a00 expands the rotation to 4x4 with zero translation. The caller's other
  matrix is identity plus the sampled translation. 00530590 composes R*T:
  its first output column is the rotation's columns weighted by the other
  matrix's first column. There is no inverse operation.
- 0059e5b1..0059e6b2 gathers the result's columns into three shader dot-product
  rows. effects/00efa0_ps.asm confirms dp4 against world position with w=1,
  followed by multiplication by noise scale. Thus noise position is
  `R*(world_position+offset)*noise_scale`. The shader parameter's name
  `mDestructionSphereInv` does not describe how this matrix is built.

`Sphere::noise_transform` now returns that affine transform; the Bevy upload uses
it without changing the shader or sampler. Two independent expanded-matrix numeric
fixtures (seeds 0 and 0xbeef) cover basis axes, translation and a nonzero point,
with 1e-6 basis / 1e-3 point tolerances for float arithmetic. All seven SSD tests
and all 249 core tests pass. Workspace build passes
(`work/ssd_noise_build_20261007.txt`). No new assumptions or fitted constants.
Inspected capture `work/shots/ssd_noise_20261007_pass1.png`: four gun impacts
on the climbing wall, with a persistent dark damage patch visible behind the reticle.
Command: `target/debug/bumblebee.exe --shot work/shots/ssd_noise_20261007_pass1.png
--start 60,75,0 --aim-from 0.2 --fire-from 0.5 --fire-until 1.2 --after 2.5
--ssd-report`. The log (`work/ssd_noise_capture_20261007.txt`) reports four SSD
contacts at y=90; render and exit succeed. The existing missing sound event
0x22977e39 and controller mapping warning remain.
No comparison to an original-game combat recording was added.

## Validation (2026-10-06)

Eight new tests cover suppression/depth, finite frame ramps/persistence, overlap merging
and cap, 512-volume eviction, exterior wall/ground contacts, real pack values and noise
header, once-only explosion contacts, and a hand hit with no AttackRef damage record.
All 206 core tests pass. Bumblebee builds without warnings.

Captures inspected: `ssd_gun_20261006.png` (two cannon impacts on a wall),
`ssd_hand_checked_20261006.png` (one melee contact on wall 5),
`ssd_pound_checked_20261006.png` (the punch out of the car marks ground and both
platform fronts), `ssd_missile_checked_20261006.png` (missile marks wall 5 and ground).
They live in work/shots; optional `--ssd-report` writes contact diagnostics to the console.
No shader/runtime errors in their logs. No recording covers original-game firing/fighting.

The earlier captures named ssd_punch/groundblast/slam did not reach the intended
ground-punch state and are not evidence of explosion rendering. The checked car-punch
capture holds vehicle until 2.0 s, climb from 1.9 s, and captures at 4.5 s; its log
shows one zero-damage hand contact and the explosion's once-only surface contacts.
