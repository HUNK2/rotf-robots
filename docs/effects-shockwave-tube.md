# ShockWave and EffectTube (effects census, 2026-10-16)

Read from the executable by the effects session. Port: `core\src\shockwave.rs` (tests in
the file). EffectTube is not ported: nothing in the installed data places one.

## ShockWave [game]

Class: vtable `00b61448` (9 slots): 0 `00804e80` delete, 1 `00804f40` teardown (frees the
rings), 2 `00805410` update, 6 `00804f60` Trigger (`00805ed0`). Create `008053c0` (pool
0x15, ctor `00804e00`, loader `00804f70`).

Data (`00804f70`, defaults in brackets): `material` +0x44 (none: not drawn),
`heightCheck` +0x6c (1; negative turns it off), `startRadius` +0x48 (0.5), `fadeInRadius`
+0x4c (1), `fadeOutRadius` +0x50 (10), `fullRadius` +0x54 (15), `ringRadius` +0x58 (1),
`fullRingRadius` +0x5c (2), `speed` +0x60 (7.5), `startSegments` +0x64 (10),
`fullSegments` +0x68 (20), `damage_type` +0x70, `damage_type_on_small_chars` +0x74,
`damage_type_on_large_chars` +0x78 (each 0x40800 = `BY_EXPLOSION | BY_KNOCKBACK` when
ABSENT; an empty set in the data reads 0), `specialCaseDamageStuff` +0x80..+0x94
(`00744f10`: stun, slow, dot, heat, knock-back speed, knock-back angle converted to
RADIANS), `damage` +0x7c (10), then `LuxScriptName` / `acceptNullTriggerMessages`.

Trigger (`00805ed0`, ring set-up `00806190`): a ring at the object's place, radius
`startRadius`, start time kept (+0xd0); a ground probe is sent (sweep from z + 1 to
z - 2, radius 0.5). The ring is linked on the object (+0x98) and on the global draw list
(`00c06ce4`), and the object starts updating.

Update (`00805410`), per ring:

1. radius += speed x dt. At or past `fullRadius` the ring is removed (`00806030`).
2. Otherwise, if damage > 0 or `damage_type` != 0: the query (`target_candidates` with the
   test `007996c0`): centre the ring's, best distance r, band r +- w / 2 where
   w = lerp(`ringRadius`, `fullRingRadius`, (r - start) / (full - start)) (not clamped),
   height limit r, yaw and pitch limits pi. A candidate passes when
   inner < |flat distance - its radius| < outer and |dz| < r. Four slots.
3. Each found thing: not the object itself, not on the object's hit list (+0xa4), not the
   `AlternateModel` scene object's owner; with `heightCheck` >= 0, |its z - ring z| <
   `heightCheck`. Then a damage message (`00724700`): origin the ring's centre, the ring's
   start time, amount `damage`, type `damage_type` unless the thing is a character whose
   size class (+0x140 -> +0x78) is 0 / 1 and the small / large type is not 0, the special
   case block, direction (flat unit to the thing, z 1) x 0.70714, force the direction x
   10000 when damage > 0. A thing that took the message is put on the hit list.
4. With no rings left the hit list is emptied and the object stops updating.

So a thing is hurt once while any ring of the object lives, not once per ring.

Drawing (`00806ac0` over the list, `00806380` per ring): alpha = min(fade in, fade out),
fade in = (r - start) / (fadeIn - start), fade out = (r - full) / (fadeOut - full), each 1
when its radii are the wrong way round and capped at 1. Nothing when alpha <= 0 or no
material. When the ground probe hit, the ring's z becomes the hit + 0.15 (written into the
ring, so the damage test uses it too). Segments = int(lerp(startSegments, fullSegments,
t) + 0.5). From angle -pi, a point is (sin a, cos a) x radius; per segment four vertices
(inner, outer at a; inner, outer at the next), normals (-+sin, -+cos, 1) x 0.7071, u = i /
n around, v 0 inner 1 outer, colour (1, 1, 1, alpha), triangles 0 2 1 / 1 2 3.

In the data: Devastator's two (`cairo_city_a_boss` #1: damage 25, material
`devastator.fx_shockwave`, radius 3 to 160 at 75 m/s, band 5 to 70); the power core's in
`shanghai_03_waterfront_a_core` (damage 60, stun 2, slow 1, flags 0x46000); one with damage
500 and no material in the same mission; others in the Cairo pyramids / ruins and Deep Six
missions; one each in `megatronflight` and `optimusflight` (no material, damage 0, empty
type: they neither draw nor hurt).

In the port: `shockwave::{Def, read, ShockWave}`; `update` takes the candidates and a
`deliver` closure (true when the thing took the message), `mesh` builds the band.
[assumed] when more than four things are in the band, the four kept are those nearest the
ring's radius (how `target_candidates` orders and cuts was not read). The ground probe is
the caller's (`ShockWave::ground`). Not wired into the game or the mission runner yet.

## EffectTube [game, unused]

Vtable `00b6146c`: 0 `00807e90`, 1 `00807f80`, 2 `00807ff0` (feeds the object's place to
the tube, `00807390`), 6 `00808030` Trigger (makes the tube `00804cc0`), 7 `008080c0` stop.
Loader `008080f0`: material, a sample rate (30 a second, at least 1), start / end radius,
four percentages, gravity, a count (5, at most 12), two switches, `autoStart`. The tube is
a ring buffer of frames sampled along the object's path and drawn as a tube (`00806b00`,
outline only). No installed pack places an `EffectTube` (`export\data\objects.tsv`), so it
is not ported. Five of its field names are in no name table (no data uses them).

The census also put `SoundAmbientSpawner`'s update and slots (`00808700`..`008089c0`,
vtable `00b614a4`, ctor `00808e00`) under EffectTube: they are sound, noted in the ledger
for the sound session.
