# Shaders and his surfaces (2026-10-04)

Started from the user's remark that he looked like a lower level of detail. He was not:
`Bumblebee.Thigh_L` and the other `RenderThing` models are the full meshes (42,616 bytes
of buffers; `_lod1` 24,274, `_lod2` 15,292, `_lod3` 2,610; the mesh's own table at `+0x1c`
points at those, taking over at 30, 65 and 150). What was wrong was the shading: Bevy's
standard material with made-up roughness, the colour texture shown as it is, the normal
map's two axes the wrong way round, no specular map, no reflection.

## Optional faithful texture upscale (2026-10-07)

2026-10-19: extended to every robot's own maps (`characters\<robot>\maps`, `global\weapons\<robot>\maps`),
colour or data by file name; see the the unpublished notes entry "Robot surfaces" of that date. The
paragraph below describes the original Bumblebee-only selection.

User-selected rendering enhancement: `core/src/texture_upscale.rs` decodes his installed
texture maps and adds 4x/2x levels; `game/src/texture_upscale.rs` caches and applies them
before material creation. 14 own robot/car/gun/glow maps are selected by full parent
path and filename, excluding shared effects and outputcube. Normal xy and specular/glow
are shader data (bilinear); colour RGB uses Lanczos3 and alpha uses bilinear separately.
The shader's existing reconstruction/normalisation of normal xy remains authoritative;
normal axes, material parameters, colour space and UVs are unchanged. Original authored
mips follow the new levels. BC1 transparency is recovered explicitly because image's
DDS decoder returns RGB for that format. The filtering choice is [assumed] enhancement
quality, logged in the unpublished notes; no claim of original-game behaviour or new detail.

`--original-textures`/`--texture-scale 1` bypass it; `--texture-scale 2` lowers memory
use. Local derived pixels live under work/texture-cache and work/textures only, with
no changes to the install and no embedded game assets.

Verified: 279 core tests, three Bevy upload/cache/quality tests, 14 PNGs checked against
independent DDS decoding (RGB rounding <=1/255, exact alpha). Canonical build and cold/
cached 4x launches succeeded, and original mode renders. Robot/car captures in
work/shots/texture_4x_*_close_20261007.png were inspected. 4x uncompressed maps plus mips
use 422.0 MiB. The improvement is interpolation and subtle; no new detail is claimed.

## `shaders.lsa`

`LSA\0`, version 2, then `shader count, shader table, entry count, entry table`.

- Shader record, 32 bytes: `count, offset of a list of count hashes, kind (1909beb3
  vertex, 88e6e692 pixel), size, offset of the code`. The code is compiled Direct3D 9
  (`vs_3_0` / `ps_3_0`) with its constant table, so the names of the parameters are there.
- Entry record, 0xa0 bytes: `technique hash, variant hash`, three `{count, offset}` lists
  of words, a 64-byte technique name and the variant's name (`Base`, `Skin`, each also as
  `ZOnly`, `GBuffer`, `Deferred`, `ShadowGen`). List a is the vertex shader's hash list,
  list b the pixel shader's: the shader whose own list is the same is the one used. List
  c is not read (defaults?).
- 354 shaders, 888 entries. `effects.lsa` is the same format (69 shaders).

`work\lsa_disasm.py <lsa> <dir>` writes each shader's listing (the system's
`d3dcompiler_47.dll` does the disassembly; nothing of the game runs).
`work\lsa_list.py <lsa> [technique]` prints the entries and which listing each uses.
`work\lsa_peek.py` was the first look. Listings are in `work\shaders\asm` (regenerable).

A material's `+0x14` hash is the technique's name. His:

| material | technique | pixel shader |
|---|---|---|
| `carpaint`, `carpaint_u` | `aniso_paint_x` | 0ab3b0 |
| `autoglass` | `aniso_paint` | 0a83d0 |
| `warm_metal`, `cool_metal` (and `_u`) | `glass_plus` | 09a0d0 |
| `rubberplastic` (and `_u`) | `cheap_plus` | 05cb30 |
| `autoglass_u` | `glass_cheap` | 098a20 |
| (not his: the other robots' main metal and paint) | `glass_plus_x` | 09b330 (2026-10-19: glass_plus + the paint's edge and sphere reflection) |
| `blue_eyeglow`, `fx_melee1` | `emissive` | 094420 (not ported) |

Whether the PC game draws with the forward variants (`Base`, `Skin`) or the deferred
ones is not read; the forward ones are what is ported. The surface arithmetic is the same
in both as far as was compared (constants and samplers).

## What the pixel shaders do [game]

All five, in this order (`game\src\surface.wgsl`):

1. Normal: the map's xy times 2 minus 1 (w and y instead when `Normal_Map_DXT5_NM`; 0 in
   all his materials), z = sqrt(|x² + y² - 1|), xy times `Normal_Map_HeightScale`; the
   normal is `N z + TANGENT y + BINORMAL x`, normalised. Note y goes with the mesh's
   tangent stream and x with its binormal. `glass_cheap` has no normal map.
2. Colour. Paint (`aniso_paint*`): with h = saturate(N.H), H half way between eye and
   sun: `p = mix(Paint_Color0, Paint_Color1, h^Paint_Pow1)`, `p = mix(p, Paint_Color2,
   h^Paint_Pow2)`, and the colour is `mix(p, texture.rgb, texture.a)`: the colour
   texture's alpha is how much it covers the paint (13/15 on his painted panels, 1
   elsewhere). The others: the texture's rgb. Both times `Diffuse_Color`.
3. Light: `colour * (saturate(N.L) LightColor shadow + ambient) + specular map *
   Specular_Color * (Specular_Power / 30 + 1) * h^Specular_Power * shadow * LightColor`.
   The ambient comes from the vertex shader: nine `AmbientIrradiance` terms by the
   normal, plus `AmbientColor`. Two omni and two spot lights add the same two terms.
4. `aniso_paint_x` only: `+ (1 - |V.N|)^Edge_Power * LightColor * Edge_Color`.
5. Reflection: the reflected eye line looked up in `ReflectionMap_Texture` (a cube map;
   his is `outputcube.dds`, 128) or, for `aniso_paint_x`, in `RealtimeSphereMap` (the
   level drawn into a sphere map as the game runs, looked up in view space); times
   `Reflection_Color`, times the specular map; the paints also times
   `(|V.N| (Reflection_Falloff - 1) + 1)^Reflection_Falloff_Power`; clamped to 0..1 and
   added. `cheap_plus` has none.
6. Fog, then times `IrisParams.x` (exposure). Alpha: the paints `AlphaBoost * colour.a`,
   the others also the texture's alpha times `Diffuse_Texture1_Opacity`.

## In the Bevy game

`game\src\surface.rs` (`Surface`, a Bevy `Material`) and `surface.wgsl`; `assets.rs`
gives a part this material when its technique is one of the Surface techniques
(the unpublished notes), Bevy's
standard one otherwise. The sky techniques have their own material (`sky.rs`). Cube maps are loaded now (`assets::image`, six faces).

- [assumed] the arithmetic is on stored texture values, not decoded from sRGB. With it a
  lit orange texel times more than 1 clips in red first and turns yellow, which is the
  original's look; decoded, it stays orange.
- `Reflection_Falloff_Power`: `#58c3933a` is the CRC-32 of `reflection_falloffpower`, and
  it is 0 in his materials: the falloff factor is 1, the reflection is at full strength
  whatever the angle. [data] Used since 2026-10-05 (it was assumed 1 before).
- "Bits of blue in him" (the user, 2026-10-05): NOT the shader's arithmetic. Bevy's depth
  prepass (the camera has one for the particles' soft edges) draws both faces of a custom
  material's mesh, the main pass only the front ones; a panel seen from behind left its
  depth with no colour, so the clear colour showed through the floor. `Surface` and
  `Glow` now cull back faces in both passes (`specialize`). [assumed] the original does
  not draw his panels from behind; its cull state (maybe the materials' `#ffa11f47`) is
  not read.
- [stand-in] the light rig: in the arena, Bevy's sun for direction and shadow, constants for
  its colour and for a two-colour ambient (`surface.rs`). On a mission (2026-10-13, LS-20) the sun colour and direction, the ambient colour and the
  nine ambient terms come from the level's default Environment (`world.rs` `mission_rig`).
  Bevy's shadow keeps its own direction; the c22 term is left out (LS-14, LS-20).
- [stand-in] the paint's sphere map: his own cube map (`outputcube.dds`, a grey studio,
  what his other materials reflect) since 2026-10-05; the flat sky above and ground
  below only where that cube is not found.
- Not ported: omni, spot and capsule lights, fog, exposure, bloom (`bloom` 1 on his
  materials), `bgnd_vc`, the second layer of `basic`, the other blend2 variants and the `_ssd`
  damage maps (2026-10-13, the unpublished notes), and the two that glow (see below).

## The glowing techniques and the particle shaders (2026-10-05)

Vehicle skid update (2026-10-07): `dull_vc` is now handled by Surface. Original
Base VS044aa0 / PS038900 (deferred PS03a790) multiply diffuse texture, material colour,
object colour and vertex colour with diffuse-only lighting. Alpha also uses texture
opacity/AlphaBoost; UV is X*u+Y*v+Z+T*time. Installed skid material `ace50b6e` has
white diffuse colour, identity X/Y, zero Z/T and Normal blend. Surface uses stored
texture values and the existing stand-in sun/ambient. Original object tint and cull
state remain assumed (status vehicle assumption6); original fog/exposure/omni/spot
integration is not added by this shader port.

- `emissive` (pixel 094420; his eyes, the weapon's lights, the axe's blade) and
  `dull_fx_emissive` (vertex 043aa0, pixel 043f90; the punches' shock rings) are ported:
  `game\src\fx.rs` (`Glow`), `glow.wgsl`. [game]: the texture's place is `X u + Y v + Z +
  T time` (`Diffuse_Texture1_X/Y/Z/T`: the rings' noise scrolls); colour = texel x
  `Diffuse_Color` x the object's colour; alpha = texel's x `Diffuse_Texture1_Opacity` x
  colour's x `AlphaBoost`; `dull_fx_emissive` adds `(1 - |V.N|)^Edge_Power x LightColor x
  Edge_Color` and multiplies the alpha by saturate(s x (`outTransFalloff` -
  `inTransFalloff`) + `inTransFalloff`), s the length of the normal across the view's
  line (0 facing the camera, 1 edge on). [assumed] `AlphaBoost` 1; an effect's object
  fades by its object colour's alpha. [stand-in] `LightColor`.
- The particle shaders are not in the `.lsa` files but in the executable: see
  `docs/particles.md` "Drawing".
- `effects.lsa` is the post effects (bloom extract, depth of field, the colour filter,
  SSAO, the deferred lights) and the executable holds more of them (bloom's `BloomLevel`
  / `Brightness` at 724450, the fog pass 724890, the colour filter 7210f0). Read
  2026-10-05, stood in for by Bevy's own passes: "The environment" below. `emissive_vc`,
  the `glow_*` and `flatcard_*` techniques and `dull_vc` (the skid marks') are not read.

## The environment (2026-10-05): a level's light, fog, bloom, colour filter, exposure

Turbo speed-blur research (2026-10-07; ported below): the owned west-town zone's
chunk0 Environment_SpeedBlur enables DOF, blurAmount1/maxBlur1, zero near/far focus and
falloff, kernelTap1, speedStretch.98, mask speed_mask (256x256 DXT1, texture chunk8),
blendDuration0 and blendOutDuration-1. `00565c00` inserts/restarts a priority-sorted
active environment with blend rate 1/max(duration,1e-5). `00733290` turns it off using
-1/max(outDuration,1e-5); its call to00565c00 is an environment blend, not a camera call.

`00551670` builds DOFParams=(farFocus,nearFocus-nearFalloff,
1/max(farFalloff,.0001),1/max(nearFalloff,.0001)). Hard-mask shader
effects.lsa PS0032d0 mixes scene/blur by max(nearCoC,farCoC)*MaxBlur*mask.r*MaskTrans.
`005839a0` builds thirteen Gaussian taps (integer x/y -2..2, |x|+|y|<3), normalizes
their weights and uses the stretch variant for a masked blur. The compiled stretch
pixel shader is VA00b213c0, file offset71fdc0 (`work/shaders/exe/71fdc0_ps.asm`):
sample centre=(uv-.5)*Stretch+.5, add each normalized texel offset, sum weighted
samples and mix with the unstretched sample by Blend. `00582d70` copies the input
with linear sampling and clamp addressing. `01177040` creates the filter shaders;
`00581660` releases them. Environment DOF loader fields run +1d0..+1f0 (speedStretch
at+1f0, +1ec is selected from flag0x200). Intermediate sizing is verified below;
the original global post-pass ordering and colour domain remain unread.

### Turbo speed blur port (2026-10-07)

`core/src/speedblur.rs` loads `Environment_SpeedBlur` and its mask from the owned
level pack. `00567660` at 00568265..00568340 resolves **dofMaskName**, hashed as a
full source path (`83cd4aac`), through `005747f0`; the editor's short dofMask reference
(`4c1bff1c`, speed_mask) is not the runtime texture key. Pack chunk8 has its own
unrelated chunk id (`5b933ace`). No textures are bundled or install files changed.

`00551670` at 00551a8a..00551add uses quarter target0 (`EDI=0`), copies the scene,
then calls Gaussian13 with sigma=dofBlurAmount, weight scale1, stretch=.98, Blend1.
`01177140` allocates width/height = ((screen_dimension /4)+15)&~15, integer division
before alignment: 1280x720 -> 320x192, 1920x1080 -> 480x272. `00550810` only assigns
the target's debug string. `00546350` is the bloom path (three Gaussian passes),
not this DOF path. These newly read functions are recorded in the ledger.

`game/src/speedblur.rs` and `speedblur.wgsl` implement copy -> Gaussian/stretch ->
masked composite, using separate input/output textures. Copy and Gaussian use
bilinear/clamp sampling; composite ColorBuffer uses point sampling while BlurBuffer
and Mask use bilinear/clamp/point mip (`00551670` / `005839a0`). The installed zero
focus/falloff values imply farCoC=saturate(depth*10000)=1 for every visible depth,
including sky (camera near0.2); loading rejects nonzero focus/falloff or other kernels
instead of approximating general DOF. The shader returns alpha1 as PS0032d0 does.
Scene edges blur according to the original mask; the mask's centre and HUD stay sharp.

The last independent `sim::State::turbo_fx_on` message controls environment weight;
it does not use remaining turbo time. Thus expiry, independent turbo stops and
DriveMode exit clear the effect even while the standard timer continues in robot form.
Blend rates are +/-1/max(duration,.00001); this level's 0/-1 durations switch immediately.
`--no-speed-blur` disables only this pass for comparison; `--speed-blur-report` logs
weight transitions and the actual GPU pass submission/quarter dimensions.

Rendering assumptions (also logged in the unpublished notes):
- [assumed] The Bevy pass runs after transparent scene objects and before bloom and
  tonemapping, retaining the current scene's HDR/non-HDR format and colour domain.
  The original global scheduler and render-target colour/format handling must be
  read, or compared with an original turbo recording, to settle their fidelity.
- [assumed] The last fixed-simulation FX latch is sampled once per render frame and
  environment weight advances by that frame's delta. Original environment scheduler
  ordering or a turbo/transform recording would settle it; current durations are instant.

Validation: all 283 core tests pass, including four new installed-mask/kernel/size/blend
tests; canonical bumblebee.exe builds without errors. Five captures inspected in
work/shots (`turbo_blur_{on,disabled,expired,transform,no_level}_20261007_v1.png`).
Active HDR and --no-level-look captures each log all three GPU passes at320x192/weight1.
Expiry and transform captures log weight0; robot capture still has0.8s turbo time left.
HUD remains sharp, masked centre preserves the car, off captures have sharp scene edges.
No shader validation/render errors. Logs: work/turbo_blur_*_20261007.txt; build/test logs
work/turbo_blur_build_20261007.txt and turbo_blur_core_tests_20261007.txt.
Not compared with the original; run1 has no pixels to verify speed blur.

The class is `Environment` (string `00b5c09c`, factory `FUN_005690a0`, loader
`FUN_00567660`: 71 fields). Environments sit in a list by `priority` (`00c02d80`), are
found by name (`FUN_0056a790`) and blend in and out (`blendDuration`,
`blendOutDuration`). The current values: `DAT_00ce4abc` (bloom level at `+0x20`),
brightness `DAT_00ce4ab8`, iris `DAT_00ce3ab4` (the debug text of `FUN_00563190`).

- WHERE: roots of a zone pack's data chunks. The zone's FIRST data chunk holds the special
  ones every level has: `Environment_FlashBang` (bloom, colour filter), `_SpeedBlur`
  (depth of field: the turbo's), `_Overdrive` (bloom), `_BarrierCollide`, `_LowHeath` (an
  overlay and a lookup table). The level's own is in another data chunk, with the flag
  `DEFAULT` and the lights: `us_west_town\zone.str` chunk 983, `Environment01`.
  `core\src\formats\environment.rs` reads it (test
  `the_west_town_has_a_low_warm_sun_and_a_blue_ambient`); `python tools\tf2_lxb.py dump
  <zone.str#N>` prints one (grep `bloomThreshold`).
- `us_west_town`'s: ambient 35 54 71; fog 117 110 102, hotspot 164, range 700, falloff
  95, height 150; bloom threshold 58.6, brightness 0.2, saturation 100, tint 249 210 128,
  blur 1.8; colour filter brightness 142, contrast 150, saturation 126, lightness 127.5,
  every tint 127.5, a lookup table `West_LUT`; iris target 0.368, min 0.96, max 1.163,
  speed 0.05, bloom gain 1.616; SSAO on. Lights: `shadowLight` `LT_Sun` (directional,
  colour 1.29 0.954 0.707, `range` 1.016, at -879 2946 1899 so 32 degrees up) and five
  `sceneLights` (`LT_bounce`, `LT_fill`, `LT_high`, `LT_Back`, one more).
- The loader's scales [game]: `bloomThreshold` x 0.01, `bloomSaturation` x 0.01,
  `bloomTint` / 255, `bloomBrightness` as is. The colour filter's and the fog's were not
  followed.
- The passes [game] (compiled into the executable, `work\shaders\exe`):
  - Bloom extract 724450: c = texel.rgb x 2 x texel.a; grey = (r + g + b) x 0.33333;
    c = grey + `BloomLevel.z` x (c - grey); out = saturate(c - (1 - `BloomLevel.x`)) x
    `BloomLevel.y` x `Brightness`.
  - Fog 724890: depth from the depth texture (`RecalcZ`), amount = saturate(depth x
    `FogValues.y` + `FogValues.x`) x `FogValues.w` + `FogValues.z`, drawn as `FogColor`
    with that alpha.
  - Colour filter 7210f0: c = scene + 1.5 x bloom; c = saturate((c - 0.5) x `Params.z` +
    0.5); c = c x `Params.x` + (`Params.w` - 1); by its grey g (t = 1 - 2g: shadows
    saturate(t), highlights saturate(-t), midtones the rest) a colour and a saturation
    are mixed from the three rows of `ColorFilterSMH_RGB`, times `ColorFilterTint`;
    c = g + (c x that colour - g) x saturation²; its length is put back to the old one
    x `Params.w`; then mixed toward `ColorFilterFade.rgb` by its w.
  - How `BloomLevel`, `Brightness`, `FogValues` and `ColorFilterParams` are made of the
    environment's fields is NOT read (the readers of `+0x190`.. were not found).
- In the port (`game\src\world.rs`, `LEVEL`, `camera_look`; `--no-level-look` turns it
  off): the arena borrows `us_west_town`'s environment. [data] the sun's direction and
  colour, the ambient colour (also what the lit particles take). [stand-in] Bevy's
  bloom, distance fog and colour grading stand for the three passes, with [assumed]
  mappings: fog from `fogHotspot` to `fogRange`, at most `fogFalloff` percent; bloom
  over `bloomThreshold` times `bloomBrightness`, added; saturation and brightness over
  127.5. `colorFilterContrast` is not applied (as Bevy's contrast it turned every shadow
  black). The brightness of sun and ambient are Bevy's, as before.
  NOT ported: the passes themselves, the bloom's tint and saturation, the lookup table,
  the iris (exposure that adapts), general depth of field (turbo's masked blur is above), the
  radial blur, SSAO, the overlay, the four other lights, blending between environments.
  Captures: `work\shots\look_final_skid.png`, `look_b1.png`. Not compared with the
  original.

Checked by captures only (`work\shots\surf_z1.png` in shade, `surf_z2.png`,
`surf_z3.png` lit; `--zoom n` magnifies a capture). NOT compared pixel for pixel with the
original; the user's reference picture (a cutscene frame) is brighter and yellower still,
which is the light rig's doing as far as can be told.
