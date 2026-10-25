# Grindor's cinematic blast wave: the plan (2026-10-24; asked by the user)

## Progress

- 2026-10-24, the user chose: EVERY shot of the cannon is the big wave (no charge), 70 degrees
  wide, the original reach (about 105 m).
- Step 1 DONE (core only, not wired into the game yet): `core\src\energon_wave.rs` and, in
  `core\src\shockwave.rs`, an optional `Arc` on a ring and a `Falloff` on the object (both
  `[port design]`; with neither set a ring is the game's, Devastator's tests unchanged).
  The wave is Devastator's ring scaled to Grindor's snaker run: the band's OUTER edge (the seen
  front, and what reaches a target first) runs at the snaker's 60 m/s and ends 105 m on, so the
  ring's own radius runs slower (about 48 m/s) and ends at 87 m with a 48 m deep band. Front 4 m
  across at the impact, about 127 m at the end. 120 damage for the first third, falling to a
  third; push weakens with it. Tests: 7 in `energon_wave.rs`; core 1450 and Devastator 24 pass.
- Steps 2-5 DONE (2026-10-24, the user: "Just do all steps"). In the game, waiting on the user's
  play test. What a shot does now, every piece from the install:
  - **The ring** (`game\src\weapons.rs` `open_energon`, `EnergonWave`): where the cannon's missile
    ends, after the original snaker is let go, the arc opens on the ground the snaker found (else
    the ring's own probe, else the missile's end) and lives as long as the snaker would. Each
    update it hurts what it reaches inside the arc once (`ShockWave::update` with the targets as
    candidates, the packet straight into `struck`: 120 and Devastator's knock-back 15 at 35
    degrees and 1 s stun, flags 0x40800); the snaker in its middle keeps its look and sound and
    hurts no one. Drawn by `game\src\shockwaves.rs` in Devastator's material, borrowed at load
    (`energon_wave::read`: his ring and the material with its one texture; 0.3 s, on a thread).
  - **The impact** (`game\src\energon.rs`): his ground slam's blast `r_melee_endblast_blackout`
    at 4 times its size (an energon lightning ball), his heavy landing's dust
    `r_contactdust_biglanding` at 2 times, the wave object's Light (blue, range 15) 4 times as
    wide and 2.5 times as bright for 0.6 s, then `EXP_HUGE` with Devastator's `ANIM_DEVA_POUND`
    and the game's `MassiveCameraShake` arriving distance / 340 m/s later. The original's own
    `WPN_BLAC_CAN_IMP`, wave emitter, rumble and lingering smoke are unchanged.
  - **The front**: the wave object's own emitter `w_exp_energoncannon` (a 20 m line of spark
    streamers and glow) laid 5 times across the arc on the band's outer edge, its particles 1.5
    times their size, each copy spread over its share of the arc (`front_frame`): the original's
    20 m energy line becomes a 4 to 127 m arc of it. Dust puffs (`r_contactdust_biglanding`, 1.2
    to 2 times as the ring runs, at most 6 alive) and a building's sparks (`exp_bldg_electricity01`,
    every 0.6 s) along it; Devastator's `ANIM_DEVA_VORTEX` (11.8 s, not a loop) rides the front's
    middle and fades over the ring's last 0.5 s.
- Why the energy wall: Devastator's band material is additive smoke at half opacity, close to
  invisible over the arena's light floor (his own captures say so, `docs/devastator.md`); in the
  first captures the arc did not read at all. The wave's own emitter along the front does.
- Cost (uncapped, `--fps --no-vsync`, the cannon firing every 4 s for 14 s, facing open floor):
  first version (12 puffs, 9 wall copies) 4.4-4.7 ms a frame and 9-14 ms slowest while a wave
  lived, against 3.2 / 5-6 ms aiming only: each element of each effect is its own mesh rebuilt
  every frame (`effects::run`). Cut to 6 puffs and 5 copies: 3.3-4.4 ms, slowest 4.7-6.9 ms; the
  original wave (`--original-wave`) measured 3.4-4.2 ms, slowest 4.9-6.8 ms. Within the plan's
  +1.5 ms. The page faults that jump with each shot (300-700k per 2 s) are the original weapon
  path's: the original wave makes more of them.
- Scouts (`--scouts --passive-enemies --enemy-at`, `--combat-report`): 120 to one 31 m past the
  impact, 97.9 to one 66 m past it, none to the two outside the arc.
- Captures (`work\shots\wave_*.png`, `--character grindor --start 0,0,180 --aim-from 0.5
  --switch-at 1.0 --fire-from 2.0 --look -0.6 --look-from 0.6 --look-until 1.1 --zoom 0.5`):
  `wave_final_t3.0.png` and `wave_final_t3.5.png` (the front at 30 and 55 m), `wave_sound.png`
  (the sound overlay: `EXP_HUGE` and the pound 0.1 s after the impact, the vortex at the front).
- The user played it (2026-10-24): "Feels good", but it went through walls, did not climb
  objects, and a shot into a wall left the wave floating in mid air. Fixed the same day
  (`energon_wave.rs`, all `[port design]`):
  - **Ground** (`Ground`): 25 lines out from the impact across the arc, each sampled every 1.5 m
    just ahead of the front (`advance`, before each ring update). A line climbs anything up to
    2.5 m above its last sample (`CLIMB`) and goes down drops of up to 6 m (`DROP`). A wall
    taller than that (a level ray at the climb height hits it), or a deeper drop, stops it.
    Rays use mask 1 (PUSH), as the snaker's do.
  - The band is laid on that ground (`mesh`: a grid, up to 16 rows across the band, each point
    pulled in to its line's stop) instead of drawn flat. Targets pass only in front of their
    angle's stop and within the ring's 2 m height check of the ground there (`candidates`). The
    energy wall's copies ride the ground, wait 3 m at a wall, then go (`front_frame`); the dust
    and sparks only come up where the front still runs (`Front::step`); the roll sound waits at
    the wall. Once every line is stopped, the wave ends 3 m after its furthest stop (`end_radius`).
  - **A shot into a wall** (`start`): the surface the missile ended on is looked for 1 m each side
    of its end. A wall (normal under 0.5 up, the snaker's wall test) opens the wave 1 m out from
    it, at its foot, its way the shot's mirrored off the wall (square into it: straight back out).
    No ground within 8 m below (high up a wall, or a shot that ended in the air): no wave, only
    the original's impact. The snaker in the middle goes from the same place. The blast, flash
    and boom stay where the missile ended (`EnergonWave::impact`).
  - Checked in the giant yard's lane (walls 2, 5, 8, 12 m tall, `world::giants_yard`;
    `--character grindor --start -420,-95,0 ... --look -0.6`): the front climbed the 2 m wall and
    ran on behind it (`work\shots\qa_lane_t2.9.png`). Its middle stopped at the 5 m wall's face,
    77.1 m out (`qa_lane_t3.8.png`), and the outer copies went round the wall's ends. Shot into
    the 8 m wall from 25 m (`--start -420,25,0`), it opened on the floor 1 m out from the wall
    and ran back past Grindor, who was not hurt (400 of 400), stopping at the 5 m wall behind
    him, 46.1 m out (`qa_wallshot_t2.7.png`, `t3.3`). Tests: 3 new (`a_wall_stops_it`,
    `it_climbs_what_is_low_enough`, `a_shot_into_a_wall_opens_at_its_foot`). Core 1456 pass;
    the game 33, and the 1 known roster-switcher failure.
  - The user: "looks good", but too many explosion effects, "reduced significantly" (2026-10-24).
    Cut: impact blast 4 -> 2 times, impact dust 2 -> 1.3 times; front dust a puff every 0.6 s
    (was 0.3, up to 2 a row), at most 3 alive (was 6), 1 to 1.5 times (was 1.2 to 2); sparks every
    1.2 s (2 bursts a wave, was 3). The energy wall, flash and sounds are unchanged
    (`work\shots\qa_less_t2.9.png`, `t3.6`, against `qa_lane_t3.8.png`).
  - Still open: a thin pillar between two lines is missed (the band runs through it); the climb
    and drop limits are chosen, not tuned in play; no level collision on mission maps was tried
    (only the arena is tested).

The user wants Grindor's second weapon more cinematic and closer to the film than the original: a
much larger blast wave (the picture they gave: one wave clearing a whole street), at a fair frame
cost and still balanced. This is a design beyond the game: its choices are tagged `[port design]`,
not `[assumed]`.

The user's rules for it (2026-10-24):
- Every asset, effect and sound comes from the game's own library in the install: no generated
  sounds or effects. Scale, retime, combine and place the game's own material; nothing new.
- Scope is the wave itself, made more cinematic. NOT in scope: a test street, props, wrecking,
  physics for objects, putting Grindor into another game.

## What the wave is now (read 2026-10-24)

- `BlackoutEnergonCannon` (0f135ad6), attribute `Blackout_MissileAttribute` (global_states.txt
  80472): a round every 4 s, heat 99.6 of 100 a round (one shot, then it cools); the missile does no
  damage; `snakerInfo` speed 60, damage 120, `timeTilDies` 1.75. [data]
- Its projectile's Destructable spawns `radial_attack` where it ends: a `SnakerProjectile` that hugs
  the ground (`core\src\weapons.rs` `Snaker`, `game\src\weapons.rs` `let_wave_go`), 60 m/s for
  1.75 s (about 105 m), stops at a wall or a drop. It is a LINE: its reach is the object's Collision
  sphere, 1.37 m; each character it reaches takes 120 once, pushed along its way.
- Its look and sound today: emitter `w_exp_energoncannon` (textures `fx_sparkstreamer04e`, `fx_gen`),
  a Light, a ControllerRumble, SoundPlayers `ANIM_BUMB_CHARGE_LP` and `WPN_BLAC_CAN_IMP`.

## The game's own material that fits (all in the install)

- The ring: Devastator's `ShockWave` (`core\src\shockwave.rs`, drawn by `game\src\shockwaves.rs`):
  a ground-following ring, 3 to 160 m at 75 m/s, band 5 to 70 m, 25 to 100 segments, material
  `devastator.fx_shockwave` (emissive, additive), with the game's own damage query and knock-back.
  The best base for a wide front: the same object, limited to an arc.
- Grindor's pack (`characters\blackout.str`): `w_exp_energoncannon` (the wave's own),
  `r_contactdust_biglanding`, `r_contactdust_mediumland`, `smk_impact_bldg_debris`,
  `smk_impact_bldg_ash`, `exp_bldg_electricity01`, `r_melee_endblast_blackout`, `r_attack_radial_small`,
  his `Explosion` objects (7) and Lights (10), camera shakes (`AnimCameraShakeEvent`, 28).
- Sounds: his `WPN_BLAC_CAN_FIRE` / `WPN_BLAC_CAN_IMP`; the shared level bank already loaded by the
  port (`us_city_00` mission.fev): `EXP_HUGE`, `EXP_LRG`, `EXP_FIREBALL`, `EXP_OVERKILL` (heard to
  500 m); Devastator's bank (`cairo_city_a_boss`): `ANIM_DEVA_POUND`, `ANIM_DEVA_POUND_MTL`,
  `ANIM_DEVA_VORTEX`, the `DEST_*` destruction events, `EXP_DCONSTWARRIOR_NRGWPN` (an energy blast).

## The design

1. **A wide front instead of a line.** Where the missile lands, an arc of Devastator's ring opens
   along the shot's way: about 70 degrees wide, so about 4 m across at the start and 120 m across at
   the end of its run. It grows at 60 m/s (the original speed) for 1.75 s, follows the ground with
   the ring's own height check, and is drawn with `devastator.fx_shockwave`. The original snaker
   still runs at the front's middle, with its own emitter, light and sound.
2. **The moment of impact.** The impact gets the large treatment, all from the lists above:
   - `w_exp_energoncannon` and `r_melee_endblast_blackout` scaled up;
   - a bright Light flash from his wave Light;
   - `EXP_HUGE` layered on `WPN_BLAC_CAN_IMP`, heard later the further the camera is (sound at about
     340 m/s, using the port's existing distance model);
   - a camera shake and rumble by distance (his own shake data).
3. **Along the front.** Dust and debris kicked up as it passes:
   - `r_contactdust_biglanding` and `smk_impact_bldg_debris` / `ash` spawned along the arc;
   - `exp_bldg_electricity01` sparks now and then (energon);
   - the low roll of `ANIM_DEVA_VORTEX` or `ANIM_DEVA_POUND` travelling with it;
   - a scorch line on the ground, if one of his surface-damage presets fits.
4. **What it does to robots.** The ring's own damage query, limited to the arc, with the game's
   knock-back (speed and angle from the special-case damage block, as Devastator's): a robot it
   reaches is thrown and knocked down. Grindor himself is never hit. Damage falls with distance
   (full for the first third, a third at the end), and each target is hit once.
5. **Cost caps.** One ring arc (at most 100 segments, as Devastator's), dust spawns on a budget
   (proposal: 12 along the front at a time), effects thinned with distance from the camera. Budget:
   the worst frame no more than +1.5 ms over today's 2.8-4.2 ms (`--fps`).

## Order of work

1. The arc ring in `core\src\shockwave.rs` (an angular limit, its hit test, the falloff), with tests.
2. Wire it to the cannon's wave in `game\src\weapons.rs`; draw it with Devastator's material
   (read from his pack at run time).
3. Impact: scaled explosion, flash, `EXP_HUGE`, delayed sound, shake and rumble.
4. Along the front: dust, debris, sparks, travelling roll, scorch.
5. Measure (`--fps`), tune damage and knock-back against the scouts, captures, then the user plays it.

## Open (the user's decisions)

- Always the big wave, or a charged shot (hold to charge = the big wave, a tap = the original)? The
  player's cannon has no charge in the data (`hasChargedShot` false; only the AI's version charges).
- The width (70 degrees proposed), speed and reach (proposed: the original 60 m/s and about 105 m).
