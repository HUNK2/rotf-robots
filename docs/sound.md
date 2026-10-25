# Sound (2026-10-03)

## Reverb (2026-10-22)

Asked for: the user said the audio "doesn't sound like it exists in a 3D space". This part is
FMOD's reverb only. Level ambience (`SoundAmbientSpawner`) is out of scope, and music and
ambience stay off. Code: `core\src\reverb.rs`, `Mixer::send` and `Library::reverb_presets` in
`core\src\sound.rs`, wired in `game\src\sound.rs`. Every function named here is in the ledger.

**How the game drives it** [game]:
- **Who sets the reverb.** A zone's `ReverbTrigger` (00770b20 / 00770b60) sets FMOD's *ambient*
  reverb to an event-system preset, picked by its index (field `2c5fe432`). The path is 00598bd0
  -> getReverbPresetByIndex 004d5e90 -> setReverbAmbientProperties 004d5fe0 / 00453ad0. Trigger
  sets the trigger's preset; Untrigger sets preset 0. A trigger whose preset is 0 does nothing
  either way.
- **ReverbArea** (loader 00770c50, activate 00770da0 -> 00598c30) makes a virtual 3D reverb at a
  point, with min and max distances. It is not in any pack of the install.
- **The morph.** Each FMOD update, 004305d0 blends the active 3D reverbs around the listener
  with the ambient:
  - the weight is 1 inside min and falls linearly (1 - f) to 0 at max (0041f630); a weight must
    be over 0.001 to count;
  - the ambient takes whatever weight the areas leave (1 - total);
  - levels are summed as `exp(mB x 0.002) x w`, the HF and LF references as their logarithms,
    and everything else linearly (0041f810);
  - the result is normalised by `trunc(ln(sum) x 500 / total + 0.5)` (0041f9a0);
  - an Off ambient (Environment -1) adds only `exp(-20)` to Room. So a listener at the edge of
    an area under an Off ambient hears that area's decay and delays scaled down by its weight.
- **The physical reverb.** The morph's result goes to 0042f740 -> setReverbProperties 0041ed70.
  That function clamps each field (the list is at `SfxParams::from_properties`), then sets
  parameters 1..14 of FMOD's **SFX Reverb** DSP (`lib\sfx\foreverb\aSfxDsp.cpp`).
  - Its DryLevel stays at -100000 mB (00475930): only the wet signal is added.
  - Environment -1 switches the DSP off.
- **The DSP** (process 0047e300; the setters are 004765c0..00476f20 and 0118a2c0):
  - **Input:** the channels summed, then two one-pole low-passes for RoomHF (`one_pole`,
    00476660), then the RoomLF shelf (004767e0), then a pre-delay line.
  - **Early part:** one tap at ReflectionsDelay, times `10^((Room+Reflections)/2000) x √0.125`.
    It goes through two all-passes (0.0059 s and 0.0077 s, gain 0.63) into a line with 7 taps at
    0.005 s x 1.59^k.
  - **Late part:** 8 feedback lines of 0.061 s x f x 1.32^i, where f = clamp(d³ x 0.9 + 0.1)
    from Density. Their pre-delay taps are at ReflectionsDelay + ReverbDelay (+ 0.0187 s x
    1.29^k).
    - Each line has its own decay gain and a one-pole HF damping (004768d0).
    - Three stages of rotations by tan(Diffusion x π/4) mix them, normalised by cos³.
    - The late gain is `10^((Room+Reverb)/2000) / sqrt(m/(1-m)+1) x √0.125`, where m is the
      lines' mean square gain.
  - **Output:** each channel sums 4 lines (read at 0.0015 s x 1.47^i) and 4 early taps,
    alternating left and right.
  - **Changes:** a parameter change glides linearly over one block, and a changed delay
    crossfades its old and new taps.
- **Event sends.** Each event's reverb wet and dry levels (head words 24 and 25, in dB) become
  its channels' reverb Room and Direct (004c7be0 / 004c7b70, x100 to mB).

**What the data holds** [data]:
- **Presets:** `audio\eng\global.fev` has 20, and no other file has any:
  - 0 is "Off".
  - 1 "Generic", 2 "Padded cell", 3 "Room" and 4 "Bathroom" carry their own values.
  - **5..19 ("Stone Room", "City", "Mountains", "Plain", "Parking Lot" ...) all carry Generic's
    exact values.** So every stage with reverb sounds the same: Room -1000, RoomHF -100,
    decay 1.49 s at HF ratio 0.83, Reflections -2602 at 0.007 s, Reverb +200 at 0.011 s,
    Diffusion and Density 100.
- **Zones:** each zone's ReverbTrigger is fired by a TriggerOnSpawn link (delay 0 or 0.1 s),
  read by `reverb::zone_triggers`.

  | Preset | Zones |
  |---|---|
  | City (13) | cairo_city, cairo_ruins, us_city_00, us_city_03, us_west_lariver, us_west_town, mp_bridge, mp_cairo_ruins, mp_usa_highway, mp_west_canals, mp_west_town |
  | Mountains (14) | cairo_pyramids |
  | Plain (16) | deep_six_01 (its link sends message 252ef147, which also triggers) |
  | Parking Lot (17) | shanghai_02, shanghai_03 |
  | Off (0) | shanghai_01, mp_shanghai, us_east_warehouse |

  - us_test has preset 13, but its spawn link has no resolved target, so it stays dry.
  - mp_skyscraper's trigger has no sender.
  - The war rooms' Stone Room (5) is fired by BGPlayer `trigWhenStarted`, part of the music,
    and is not ported.
- **Events:** all 5163 have wet 0 dB and dry 0 dB.
- **No ReverbArea anywhere.** A zone has one reverb for its whole length, so walking about a
  stage never changes it. Moving between areas can only be shown with the test aid
  `TF2_SOUND_REVERB_AREA`.

**In the port:**
- `--mission` names the stage (`sound::set_stage`). `sound::start` reads its zone on a thread
  and arms the spawn-fired trigger, which `tick` counts down. The listener is the camera.
- `Playing::render_reverb` sums the three mixers' sends and adds the reverb.
- With `TF2_SOUND_TRACE`, the dump journal gets `reverb` lines (the properties, at most one
  every 0.25 s) and `reverb wet` lines (the wet RMS with the master divided out, and the
  listener, every 0.5 s).
- Test aids: `TF2_SOUND_REVERB=off|<index>`, and `TF2_SOUND_REVERB_AREA=x,y,z,min,max,preset`.

**Dumps** (`sh work\sndrun\reverb_run.sh <name> <mission> [area]`, 2026-10-22; walk, melee,
fire):
- **Cairo_City_A_Boss:** the trigger sets 13 and the morph gives Room -999 (the game's own
  rounding). Wet RMS is 0.05..0.12 while the total is 0.17 (15..28 s): about 7 dB under the mix.
- **Shanghai_01_Construction_A_CSA:** the trigger is preset 0, so nothing happens and the wet
  stays 0.
- **US_Test_FreeRoam_A with a Bathroom area** (20..60 m around the spawn): the wet is 0.17
  inside, then 0.10, 0.06 and 0 as the camera leaves past 60 m. The properties glide from
  Bathroom's down to Off, decay 1.49 -> 0.02 s.
- **Nothing is compared with the original's output.** No recording of the game's sound has
  reverb isolated.

## The sound debug overlay (2026-10-22, the user asked for it)

F8 in the game lists every sound playing (`game\src\sounddebug.rs`; controls and columns in
`game\README.md` "Sound debug"). Not the game's: a debug aid. It reads `SoundOut::debug_view`,
which locks the mixers and takes `Mixer::view` (core): per instance its event, place from the
listener, fade, sustain hold, key-offs, voices, roll-off, and the last render's peak of its own
output (measured where the render already splits each instance's output for the reverb send,
master divided out). Who started a sound comes from the game side's handle lists (`others`,
`projectiles`, `engine`, `with_clip`, `sources`...); a handle in none of them shows as `player`.
Checked with a capture (`work\shots\sounddebug.png`, Bumblebee running and firing at the scouts):
14 sounds with their owners, distances and meters, ended ones grouped below.

## Other characters' sounds (2026-10-22)

How an enemy is heard (`game\src\targets.rs`, `game\src\sound.rs`; the bug list is the top entry of
the unpublished notes that day):
- Its gun: `EnemyGunSounds` per enemy, the player's kept-sound rule (fire and ring-off kept while
  `automaticFiring` and infinite; keyed off at `sounds_cut` or the overheat; the overheat sound kept
  until it cools), each kept handle following the muzzle.
- Its clips: `ClipClock::advance` gives the cue window (as `animation::animate` for the player) and
  `SoundOut::other_clip_cues` plays the cues at the enemy (`Source::Moving`, its speed on `speed`);
  `Playing::others` keeps each enemy's handles with their event ids (its Stop cues and clip changes
  touch only its own), `other_steps` its footsteps (its own `footStepFX`, probed from its posed feet),
  `follow` moves them, `other_gone` stops its loops.
- The scout's gun `WPN_SWIND_GUN_FIRE` is authored 2D (mode 0x180008, the one unplaced event of
  `dcomscout.fev`'s 67): heard at full level and centred wherever the scout is.
- Missing for the sense of space: the reverb areas (not ported; the user wants them next). Level
  ambience is not wanted (the user, 2026-10-22): it stays off, as the music does.

## Sound mix: presets and music (2026-10-16, sound session)

The game's mix above single events, read from code and ported to `core\src\audio_mix.rs`
(tested; not wired into the game, not heard against the original).

Audio presets [game, data]. `audio\presets.lxb` (asset a005201c, loader 01196290) is
`bnxglobal.str` data chunk 18 on the install: `presets`, an array of named `AudioPreset`
objects (id = the object's name hash, e.g. `SystemPausePreset` 0e73e231), each an array of
`AudioPresetEntry` with `category`, `volume`, `volumeDuration`, `volumeOutDuration`, `pitch`,
`pitchDuration`, `pitchOutDuration`, `pause`, `lifeSpan`, `fallOffMin`, `fallOffMax` (77
entries). Play (00771230): for each sound-system category (list 00d4f2f0) with an entry of
that exact name, the volume after fall-off (if fallOffMax > 0 and a position is given: skip
beyond max, else `1 - (1 - (d - min) / (max - min)) * (1 - volume)`, no clamp below min),
then 00595ff0 on the category record. Stop: 011832a0 -> 01178880 per category.

Category record (00595f30): base volume 1, base pitch 0, pause count, dirty flag, 8 slots
of 0x38 bytes. Claim (00595ff0): same id already held -> count + 1, hold timer 0, and when
distanced a re-aim of the fade (amount = (1 - current) / (1 - new), in-rate x (1 - amount),
phase in; the slot's target is not rewritten). Else the first free slot: rates 1 / max(d,
0.01), out duration < 0 = in duration, phase in, count 1; pause -> pause count + 1. Full
(8 held) -> nothing. Update (00596200, per frame while dirty, from 00596a00): phase in
(count 0 -> out; amounts rise to 1, both full -> hold), hold (timer; lifeSpan >= 0 and
exceeded, or count 0 -> count = 0, out; the pause count is NOT decremented there), out (count
>= 1 -> in; amounts fall to 0, both empty -> free). Volume = base x prod(lerp(1, target,
volume amount)); pitch = base + sum(pitch x pitch amount) (disassembly 00596350-0059637b:
the decompiler dropped the pitch sum); then category setVolume, setPitch(pitch, units 0),
setPaused(pause count > 0). Clear (00595fb0): frees all, pause count -= count.

Music queue (audio manager 00d4fa78; 007719d0 play, 00771540 update) [game]. MusicPlayer
(loader 007705c0: `playType` +0x134, `fadeTime` +0x138; trigger 007706d0) starts its event,
then hands it over. Mode 0: stop current, start new. Mode 1: if nothing plays, mode 0; else
fade the current out (volume 1 - t/T, t before the step), then stop it and start the queued
one. Mode 2: the new one starts at volume 0; current 1 - t/T and new t/T; at the end stop
the old and set the new to 1. The same update takes the listener from the player character
when the listener name is 9fb57f53, else from the camera; SoundListenerTrigger sets that
name (00770f30) or resets it to `Camera` (00770f80).

Music volume [game]: SetMusicVolume (00773cd0: `volume`, `duration`; 00773de0) sets the
sound system's target and remaining time; duration <= 0 applies at once; else 00599070 each
frame: dt >= remaining -> target, else current + (target - current) x dt / remaining.
UpdateMusicParameter (`id`, `value`) and CueMusic / UnCueMusic drive FMOD's interactive
music engine (MUSIC_SYSTEM.fev: parameter set, prompt begin / end, list sys+0x1590); that
engine is not decoded or ported.

Assumptions (also in the unpublished notes): category pitch units 0 = raw, 1.0 = four octaves;
a zero fade time's one NaN frame is dropped. Settled by disassembly: mode 0 makes the new
track current (ESI = +0x1c at 007719df); modes 1 and 2 write the next slot. Not read: BGPlayer, ReverbTrigger / ReverbArea, SoundOcclusion(Automatic),
SoundAmbientSpawner, SoundReciever / SoundTransmitter, SoundParameterModifier,
PedestrianSoundSettings, SetMusicSpeakerMix, VOPlayerSequence, VOAnimTimeStreamRefLuxScript,
and the FMOD event internals 004bf280-004c0b70.

## Weapon equip/switch cues restored (2026-10-09)

User reported missing equip and weapon-switch audio. The gun's object clips were
animated and emitted particle events, but their sound events were neither preloaded
by `game/src/sound.rs` nor dispatched. The installed WeaponPartialSet arm clips have
no AnimPlaySoundEvent keys; the sounds belong to the weapon object's own clips.

`core/src/gun.rs` now collects authored sound commands during each simulation update
for the arm and object players. It includes tick-zero keys, final intervals before
clips disappear, loop tails and `allowReRun`, and starts a fresh audio lifetime when
the same clip restarts. `Arsenal::tick` drains these into `SoundOut`; each player has
its own autoStop handle list, separate from the character's base clip. Startup also
prepares sounds named by the weapon-object clips. No replacement samples or invented
sound timing were added: IDs, times, autoStop and allowReRun are [data].

Installed-data regression drives equip cannon, switch to launcher, switch back,
stow cannon and equip launcher, and verifies the respective authored events
`ANIM_BUMB_WEPN_R2W_A`, `ANIM_BUMB_WEPN_A2B`, `ANIM_BUMB_WEPN_B2A`,
`ANIM_BUMB_WEPN_W2R_A`, `ANIM_BUMB_WEPN_R2W_B` produce nonzero mixed samples.
An adapter regression verifies base/arm clip changes cannot cancel the object's
autoStop sound; its own replacement can. A core regression checks tick-zero,
loop-tail, first-pass-only and overshot end keys. 344 core tests and 11 sound adapter
tests pass. Canonical `target/debug/bumblebee.exe` rebuilt successfully. A native
scripted capture (`--aim-from 0.5 --switch-at 2.5 --after 5`) starts 48 kHz stereo
output and logs `ANIM_BUMB_WEPN_R2W_A` and `ANIM_BUMB_WEPN_A2B` through the runtime
adapter; its PNG was inspected. Evidence: `work/weapon_sound_build_20261009.txt`,
`work/weapon_sound_capture_20261009.txt`, `work/shots/weapon_sound_20261009.png`.
No original gameplay/audio comparison has been recorded for this fix.

## Audio playback follow-up2026-10-08

Continued the prior gameplay-only pass using codebase-memory searches/traces and
coverage checks. The index reported changed metadata for the affected files; current
source was read directly before editing. Original code was accessed only through
tools/fn.py, with narrow x87 disassembly for pitch arithmetic. No original game run.

Applied to core/src/formats/fev.rs, core/src/sound.rs, game/src/sound.rs and the
existing animation adapter:

- Event and definition base pitch are now retained. 004bf110/004c1330 and 004df040
  verify four-octave units: multiplier = 2^((base + radius*(2*r-1))*4). The previous
  random spread was halved and base pitch was discarded. UI_REGEN_START's installed
  .245 base now supplies about 1.972x pitch. Event gain/pitch randomness is shared by
  its layers; definitions have independent draws. Exact RNG sequences remain assumed.
- Layer property 0x14 now multiplies pitch by 2^(envelope*8-4), as 004c5230 does.
  Disabled flag-bit0 envelopes are ignored. The loaded engine still uses its existing
  RPM auto-pitch and load filter; no gearbox or RPM constants changed.
- Definition play modes are retained. 0118aa60 verifies sequential modes0/3,
  weighted mode1, and definition-global weighted/no-repeat mode2: a repeated pick
  advances to the next sample, rather than redrawing. Shuffle modes4/6 consume one
  of each sample; 004deeb0 prevents a repeat across cycle boundaries. Programmer
  bindings retain their explicitly bound single waveform.
- Event fade-in (header word27) is retained, and both event fades follow authored
  durations. 004c0ca0 verifies linear elapsed/fade-time gain. Fade gain is applied
  across output frames, rather than subtracting a whole callback before its first
  sample. Early stopping preserves the current gain. Existing sample-span fades
  and FSB loop boundaries continue to apply separately.
- SoundPlayer attachments retain their object node index. 0076e100 composes that
  node's world source transform. Triggered robot/car component sounds now follow
  their final animated attachment, including turning and bone motion, independently
  of other copies of the event and other forms.

New assumptions, tagged in code and logged in status.md:

1. Sequential/local-shuffle history uses one persistent (file,definition,event,
   layer,sound) entry in place of FMOD's reusable EventSound instances. No-repeat and
   global-shuffle histories are definition-wide. Instance-pool/choice telemetry would
   settle interleaved multi-instance sequences; RNG choices remain deterministic
   xorshift under the earlier documented assumption.
2. Authored fade ramps use elapsed output samples and preserve current gain on an
   early stop. Original EventSystem update/DSP timestamps and early-stop telemetry
   would settle scheduling and the transition. Parameter seeking, sound gain ramps
   and DSP envelope updates still follow the callback; no full timing parity claim.
3. Component sources use the same final-pose frame ordering as foot/spawn cues.
   Original source-update telemetry would settle the update phase.

The read-only audit work/audio_playback_audit_20261008.py covers the five loaded
gameplay FEV files: definition pitch/volume random modes are all continuous, base
definition/category pitches are zero, and event pitch includes nonzero gameplay/UI
cues. The audited sound loop counts are all -1; no finite-loop approximation was
introduced. Audit results: work/audio_playback_audit_20261008.txt. Read ledger entries
and their bounded meanings: work/audio_followup_ledger_20261008.py and functions.tsv.

Full FMOD spatial speaker mixing, Doppler/reverb, callback/DSP scheduling parity,
projectile source offsets, destruction-scene lifecycles and synchronized original
combat listening comparison remain open. These require the wider audio/scene port
or user-played recordings. Music/ambience stay excluded under the prior user scope.

Verification: all 336 core tests and 12 game tests pass (nine gameplay audio adapter
tests). New regressions cover authored regeneration pitch, the full symmetric pitch
radius, pitch-envelope neutrality/disabled flags, no-repeat/sequence/shuffle rules,
fade duration across device callback sizes including early stop, and independently
moving component bones/forms. Installed SoundPlayer node indices are checked against
the object hierarchy. The first full game run hit an unrelated texture-cache temp-path
permission error; redirecting this test process's TEMP/TMP to
work/audio-followup-test-temp let the unchanged test pass. No texture code changed.
Canonical target/debug/bumblebee.exe and sound_render built successfully.

New offline renders (local verification outputs, not distributable assets):
work/sound/audio_engine_followup_20261008.wav: 40s, 48kHz stereo, peak .654012,
RMS .174215; audio_regeneration_followup_20261008.wav: 1.78s, 48kHz stereo,
peak .272408, RMS .043612. Both have zero clipped PCM16 samples. The engine uses
the existing recorded RPM/load trace; this is not a waveform match to original FMOD.
Logs: work/audio_followup_{core_tests,game_tests_workspace_temp,game_build,
engine_render,regeneration_render,render_metrics}_20261008.txt.

Native follow-up captures: audio_followup_combat_20261008_v1.png and
audio_followup_vehicle_20261008_v1.png under work/shots; both inspected. Device
output starts at 48kHz stereo. Cue traces confirm Bumblebee/scout gun fire, bullet
impacts, EMP, regeneration, engine/road/suspension, both boost players, transformation
and return-to-robot steps. Logs: work/audio_followup_{combat,vehicle}_smoke_20261008.txt.
Three old transformation IDs are still absent from the install. Startup's fourth
unresolved ID, f544010e, is UI_MP_ALLSPARK_ENERGY_LP: present in multiplayer mission
FEVs, including mp_bridge, but outside the selected us_city_00 sound level. It is an
inactive AllSpark component here (the arena has no multiplayer pickup mechanics),
not a newly missing gameplay cue. Prior final-pass traces already reported it.

## Audio completion2026-10-08

User requested finishing Bumblebee audio and confirmed **gameplay sounds only**:
no mission music or ambience. Sources remain the user's install, read at runtime.
This section supersedes the older missing/seam/category/footstep lists below.

Implemented in core/game sound, FEV/FSB/scene/clip readers, character and chatter:

- FSB loop start and inclusive end now define the sample period. Removed the invented
  512-frame crossfade and 30ms minimum stop fade; stop uses the event's authored fade.
- Category paths and parent gains are retained. Runtime FX/UI/voice gains multiply
  their authored category tree; master volume remains TF2_SOUND_VOLUME.
- 004c5230 sends property0x404 to00415e20 (Channel::set3DLevel, clamp0..1).
  Engine2dPanLevel1 evaluates to.4, rather than ignoring its envelope. This is now
  applied to each layer through the stereo adapter; full FMOD speaker mixing remains
  a stand-in. UI and programmer radio events retain their authored2D mode.
- UI_LOCK_ON crosses.25/.5/.7/.85/.95, UI_LOCKED_ON at1 (completion takes precedence,
  at most one cue in a frame). UI_REGEN_START plays on regeneration's rising edge.
  UI_OVERDRIVE_* belongs to the separate Overdrive mechanic, **not Bumblebee's EMP**;
  the EMP's own SoundPlayer/clip cues continue to supply its sound.
- SoundPhysicsReactor attachments load separately from CollisionFX. Bumblebee's car
  names VEH_CAR_IMP_OBJ and VEH_IMP_SKID_WALL, plus a bottom-impact override. Loader
  0077fd90/01184450 and contact0077f620 verify hard/soft/bottom fallback, disable,
  attachToObject and oneShot (consumes the preset entry; does not make a waveform
  one-shot). Roll/scrape handles persist by contact and stop on kind change, expiry,
  form exit or reset. Update0077f140 follows contact position at.01/update and speed.
- Sound sources have independent body-relative/fixed world positions. Tyres follow
  the selected rear wheel. FootStepEvent now retains side; FootStep attachments
  provide Midfoot_L/R nodes28/29 and heightFromGround1.5. Cues probe the final animated
  foot pose and select footStepFX by the hit material. The probe runs from 1.5 m above the
  foot's ground (bone z less heightFromGround) to 1.5 m below it [game 0073f420]; until
  2026-10-20 the port's ran z + h to z - h, which missed most of the large robots' steps (Long
  Haul 105 of 132 FootStep events, Ironhide 123 of 146: `ground_sound_check`). Arena ground still returns
  Default; no distinct dirt/metal surface coverage is claimed.
- Character.battleChatter has115 rows referencing109 unique installed samples.
  FSB lookup uses CRC32 of uppercase sample stem without.wav (all tableIDs resolve).
  ProgrammerSoundPlayer supplies VO_LOUD/CHATTER_PLAYER,2D mode, volume and fades.
  Attack/low-health/self-death lines are now played. 007cc720 matches action/category/
  either subcategory, chooses uniformly, then tests frequency (usually.333).
  007cd880 suppresses by action/category/subcategory tuple, rather than waveform.
  007cd7d0 maps history times from BattleChatter tuning: attack25, injury25, death20.
  007cd430 chooses speak delay.3..1.5; raw007cd794..be schedules game clock+delay,
  so the port queues before playback, rather than pausing after it.
  007cb180 gates while speaking/in vehicle, checks StateWeapon+firing,
  StateAttackSpecial (including active overlays), health below.333, and self death.
  Raw007cb887..ae selects ATTACK/GENERIC/GENERIC plus named target;007cb916..934
  selects ENEMY when a target exists. Bumblebee has four ENEMY and four Decepticon
  attack entries, no GENERIC-subcategory attack entries. The arena supplies its best
  aim-assist target presence; its generic scouts have no named-character identity.
  Low health outranks attack in this single-speaker adapter; original queue priorities
  are assigned by007cc400 and selected highest-first by007cc990.
- The scout's dcomscout.fev is loaded and his gun predecoded; it is placed at the
  scout's firing position rather than at Bumblebee. NPC animation/chatter orchestration
  remains outside this Bumblebee pass.
- SoundPlayer registry00ad9040 -> raw0076dca0 ->0076d320/vtable00b5e5c4.
  Loader0076d490 prefers nonzero idtext to id; fixed the ignored AllSpark pickup/energy
  bindings (these multiplayer cues have no pickup mechanics in the arena).
  0076e320/0076e500 verify trigger start/restart. Stored playOnce/allowRetrigger flags:
  robot players retrigger; Camaro turbo's player does not. Component handles are kept
  apart from animation cues. The current Bumblebee profiles all wait for a trigger and
  have playOnce=false; data tests check that closed set.
  0076e450 verifies UnTrigger normal-stop versus key-off (2026-10-20: the port now reads
  `keyOffOnUnTrigger` and keys off with it; the special's end Untrigger from 0072b980, carried in
  `State::untriggers`, reaches the players by name in both forms: Optimus's `WPN_OPTI_SPEC_FIRE`
  had looped for ever). Camaro turbo sound now receives
  fxstate::kept's Trigger/UnTrigger edges, including DriveMode exit, rather than only
  state.triggers pulses. Previously its continuously held FX_turbocharged sound was
  never started. SoundPlayer0076e100 updates attached speed each frame; added this for
  body sources and retained the unnamed d9a785a4 parameter's case-sensitive hash.
- SoundPlayer init0076dcf0 starts when waitForTrigger is false. The anonymous
  landing/punch ObjectSpawnerOnBone player IMP_CONC_LAND is now read even though its
  immediate object has no Explosion. Three spawners reference it; each starts at its
  final animated spawner bone.
  Missile flight WPN_OPTIMUS_MISSLE_LOOP has an independent moving handle per missile;
  destruction stops that handle with its authored fade and starts detached sibling
  EXP_OPTIMUS_MISSLE once at the endpoint. Bullet impact presets now request their
  material sounds at the same contacts used by damage/effects.
- ClimbStepEvent now queues its limb's sound probe. Raw0073fa79..0073fc20 verifies
  normalized object forward row+1c, offsetFromWall and a1.5m ray each way from the limb.
  Bumblebee's four components have offset0/spawnSlideEffectOnThisBone=true. Slide
  reaches all four; contact material selects climbLeft/RightHand/Foot/SlideFX.

New assumptions (also in status.md and code):

1. Collision impactForce uses the arena contact's relative speed magnitude as a proxy
   for Lux's max(relative normal speed, contact scalar+5c). Our solver exposes no Lux
   scalar. Original contact telemetry would settle its magnitude. The existing first
   push3/repeated slide5 arena adaptation applies to both visual and audio history.
2. Deterministic xorshift replaces the original clock-mixed RNG. Frequency/delay ranges
   are original; exact choices need original RNG state/time replay.
3. Footstep probes are synchronous from the final animated pose. Original asynchronous
   query/callback timing would settle the ordering and probe integration.
4. 3D level blends existing stereo projection/attenuation gains with unplaced gains.
   Full FMOD speaker-matrix gain parity needs synchronized original output while the
   listener moves around a source.
5. The best aim-assist target supplies chatter combat-target presence instead of Lux's
   target query. Original target-query telemetry would settle the selection.
6. Projectile flight/detached sounds use the simulated missile centre/endpoint;
   its model-node/DetachLink source offsets are not carried by the current projectile
   adapter. Original source telemetry or porting those matrices would settle this.
7. Climb probes and the ground-punch spawner's sound source use the final animated
   pose synchronously; bullets use the existing synchronous impact adapter. Original
   query/callback ordering or synchronized recordings would settle the timing.

Remaining fidelity limits: original FMOD3D speaker matrix, Doppler, reverb and scheduling
of parameters/DSP per original audio update (ours per device buffer); pitch units are
now verified in the follow-up above. Global NPC speaker arbitration/tactics/ally replies and named
characters require the wider encounter systems. Combat audio has not been listened to
against a synchronized original combat recording. No audible parity claim.
Destruction-scene FIRE_LG/EXP_OVERKILL players are authored under DamageData's
spawnOnDestruction scene; that fragment/fire object lifecycle is not ported here.
Current DeathSet animation cues and self-death chatter are connected. User requested
wrapping this pass for now; these wider-system limits remain recorded.

Verification:332 core tests;11 game tests, including eight gameplay audio adapter
tests covering delayed chatter/
regeneration edge/reset, contact scrape expiry, named-foot ground/air probe behavior,
component retrigger isolation, Camaro boost's held/off/Drive-exit lifecycle, independent
missile flight/stop/explosion handles, spawned sound source/trigger consumption, and
climbing wall/no-wall probes. Latest tests pass after the final spawn/climb additions.
Core tests cover actual115-row/109-sample chatter bindings and audible decoded lines,
collision/material/foot node profiles, authored category/runtime gains,3D-level envelope,
FSB wrapping, lock cue crossings and the earlier engine/recorded gearbox checks.
Logs:work/audio_{full_tests,adapter_tests,game_full_tests,build}_20261008.txt.
Native captures:audio_smoke_walk/combat/vehicle; output48000Hz/stereo starts normally.
Vehicle trace confirms actual RPM, road, suspension, transformation A/B/G1/parts/squeal,
car impact and wall scrape events. Combat trace confirms fire,overheat,refusal and scout
gun cues; scout's missing event/source is fixed. Work/shot files retain the captures.
Three old transformation refs (6064b207,7d571b4f,8ee729e9) remain absent from installed
event files, as already documented below; no replacement sound invented.
Offline renders:work/sound/audio_engine_final_20261008.wav,40s/48k stereo,
peak.654012,RMS.174215;audio_chatter_final_20261008.wav,1.16s/48k stereo,
peak.966704,RMS.193575. Both nonzero with0 clipped PCM16 samples.
Metrics:work/audio_render_metrics_20261008.txt. These renders are local verification
outputs from the user's install, not distributable assets or proof of original listening
parity. Game install remains untouched; original executable never run by this agent.

Other inspected functions, ledger pointers here:007249a0 component getter;
0079d490 tuning dispatch;007cadf0 constructor;007cc290/007cd990 clear;
0117e020 Character table binding;007cc400 table load/action priorities;
007cce60 global registry init;007ccb60 enqueue after history;
007ccc10 speaking/busy gate;007ce580 named-character identity mapping;
01186cc0 sound starter wrapper;007ce310 ally reservoir;
007cdbc0 reply category mapping;007ccff0 global action-count/distance arbitration;
007cc8f0 line by eventID;007cd6d0 history expiry;
007ce970 programmer event registry lookup;007ccb10 marks selected speaker busy;
007cd750 queues start clock+delay;008163f0 inserts scheduled record;
007cda80 active speaker insertion;007ce9d0 event-finished query;
00780080/01184740 reactor constructors;0077eef0 preset reset.
004c4380 was only outlined/selected, not fully understood.

Second audio round prepared2026-10-08: user authorized audio-only WASAPI loopback
with CE memory. Parked engine event and live rpm/load/2dPanLevel parameters verified,
including ranges and normalized current/target fields; see the unpublished notes
"Second-round audio and combat preparation". Pilot v3 confirms shared-QPC overlap
and valid nonzero stereo playback. Full-round audible parity has not been assessed.

## CE capture gearbox comparison (2026-10-08)

User requested engine-noise capture facts and comparison with this build.
Source:`work/traces/ce_rotf_20261007_164337_49792.bin`; no audio was captured.
`work/engine_audio_trace.py` exported17,509 parameter rows,7,692 Drive update observations.
Raw gearbox RPM min1820.8551,median6496.875,max12586.957;load min.0641079,
median.508213,max.936612.80 observed update rows exceed8800RPM; these are raw gearbox
values, not measured FMOD parameters or proof of audible pitch above the event range.
Rust Mixer::set_parameter clamps the engine event to its authored0..8800 range;
parameter seek smoothing then applies. No arbitrary gearbox RPM cap added.

`work/gearbox_fixture.py --all` extracts7,637 consecutive pairs (81 default spaced
cases also saved). Added optional TF2_GEARBOX_CAPTURE path to the existing core
regression test, preserving default fixture/tolerances; aggregates all errors before
assertions. All7,637 new cases pass:gear mismatches0,RPM median0,p95.0014648438,
max.00390625;max load error.0000028014183,max revs error.0000076293945.
Each case seeds the previous recorded gearbox state and uses recorded dt/inputs;
this verifies individual update equations, not an unseeded physics or audio replay.
Adapter game/src/sound.rs passes sim's vehicle_gearbox RPM/load to the engine event.
No game/sound runtime behavior changed here. Existing81-case default regression also
passes. Results:`.gearbox{,_all}_comparison.txt` beside source; full analysis updated.
Audible parity still needs synchronized original audio; previously documented spatial,
category/option volume,pan envelope,seam and scheduling debts remain.

## Engine playback verified and applied (2026-10-08)

User requested how Bumblebee's car engine noise works, and application to the Bevy build.
The engine is the continuous `VEH_BUMBLEBEE_CAMARO_RPM` event from his sound table,
using the installed idle/medium/high bank loops. The wheel-set gearbox supplies RPM
and load; it is not a pitch derived directly from road speed. The existing81 captured
gearbox-pair replay still passes (maxRPM error.005859); no gearbox tuning was changed.

Read the embedded FMOD player statically through fn.py, with narrow raw disassembly
where Ghidra dropped arguments. Applied in `core/src/formats/fev.rs` and
`core/src/sound.rs`, already consumed by `game/src/sound.rs`:

- **Crossfades corrected [game/data].** `004c5230`, instructions004c5b24..004c5b78
  and004c5d54..004c5dba, multiply each fade value by that SOUND's length. Previous
  code treated it as a length across the whole parameter and made broad, quiet fades.
  Actual idle/medium overlap1438..1845RPM; medium/high3847..5298RPM. Medium at3000
  now has full authored gain. Both active fades use the first curve word's low nibble;
  the engine's first words are2,0,0. Curve2 uses sqrt (equal-power); curve0 is the
  cubic Bezier default (smoothstep), checked through004bcfa0. Fade-in takes priority
  if fade regions overlap. Loader01167320, narrow01167570..0116760d, maps the two
  trailing words to+38 and+38<<4; this executable's two branches use the low nibble.
- **Low-pass corrected [game].** Descriptor00469150 /00bf8878 sets10..22000Hz;
  004cc7c0 interpolates logarithmically for DSP type0x13 parameter0. `004692a0`
  uses alpha=period/(1/(2*pi*cutoff)+period) below rate/pi, then interpolates alpha
  from2/3 to1 up to22000Hz, which bypasses. `00469370` applies TWO cascaded stages
  per channel and leaves history unchanged on bypass. Previous code used one stage
  and a different coefficient. Load still drives the original volume and filter
  envelopes; no fitted volume/filter numbers were introduced.
- **Seek speed confirmed [game].** `004cee40` dispatches parameter setter004ce900
  through vtable+10. Setter normalizes physical units;004cd990 advances that normalized
  value by seekSpeed*elapsed_ms/1000. Existing range-scaled seek implementation is
  correct: engine RPM1 means8800RPM/s. Fresh/not-playing values set immediately.
- **Auto pitch confirmed [game].** `004c5230` uses minimum+(1-minimum)*share/reference,
  then adds fine tune. Idle minimum1 makes its pitch constant; medium/high minimum0
  tracks RPM against3000/4000 reference RPM. Removed unverified pitch clamps and now
  retain/apply fine tune (zero for these engine sounds). CORRECTED 2026-10-20: fine tune is
  HERTZ added to default frequency x multipliers before setFrequency (00414bf0); it was added to
  the multiplier, which broke Megatron's `VEH_MEGA_TANK_RPM` (-10660 Hz) (status.md).
- **Loop-boundary adapter bug fixed.** Audible loops previously skipped an output
  frame when wrapping; they now wrap and read in that same frame. The512-frame seam
  blend remains a labelled stand-in, not a reading of FMOD's sample looping.

Verification:323 core tests pass including authored overlap/gain, equal-power idle,
two-stage impulse/upper transition/bypass history, audible wrap, and81 recorded gearbox
updates. Canonical `target/debug/bumblebee.exe` built successfully. Logs:
`work/engine_audio_{tests,full_tests,build,render}_20261008.txt`.
`work/engine_audio_trace.py` exports offline time/vehicle/RPM/load observations, keeping
the final observation of each drive update; `sound_render trace=<csv> start=10` replays
them with original event parameters and car entry/exit. Preview:
`work/sound/engine_recorded_20261008.wav`,40s/48kHz/stereo,peak.65460,RMS.17428,
zero clipped samples. This replays captured parameters through our corrected mixer,
not the original FMOD output, and is not evidence of waveform parity. Capture audio
includes tyres/turbo/other game sounds; no listening comparison is claimed.

NEW [assumed] adapter detail: omit FMOD's alternating denormal-prevention bias in
the Rust filter (00ca9580 is1e-20, read statically). It is a numerical safeguard rather than an audible component; exact
floating-point/DSP comparison would settle this. Logged also in status.md.
Remaining engine debts: full FMOD3D/Doppler/spatial mixing,0x404`2dPanLevel` property
envelope, runtime category/options volumes, seam blending, parameter/DSP updates per
audio buffer rather than original FMOD scheduling. These corrections do not claim
full audible parity.

Other functions inspected while locating playback (ledger entries point here):
004b90e0/004d23a0 serialization;004ce540 normalized automatic parameter assignment;
004d0bc0 EventSound initialization;004d0c40/004cc610/004cd300 release/stop cleanup;
004d16b0/01167250/01166a10 DSP scheduling;011671d0 loop-ended query;
0118a750/004d1e60/004d1ef0 source/channel parameter forwarding;
004d1f80 source spread assignment;004d2040/004d2170 reverb channel forwarding;
004cd390 effect updates;004ccdc0 effect setup;004cc2c0/004cc120/01167070 instance
allocation;011668e0 pool cleanup;011669a0 instance selection;004b9580 group metadata;
004b9620 group initialization;004c4150/004c4260 stop/release;
004c7340 active-channel count;004c4310 parameter update dispatch.
Only outlines were inspected for004d0d10/004d12e0/004d2cc0/004ca2e0/004c4380;
these remain unread beyond call relationships.004d26b0 initializes the EventSystem.

## Expanded vehicle capture (2026-10-07)

User reports audio differences. `vehicle_20261007_152811.audio.wav` is valid48kHz,
stereo float32,337.97s,peak.568;packet mapping spans.046672..338.012495s with only
the initial discontinuity flag. This is Windows output, not an isolated engine stem.

81 recorded consecutive updates are compared by `gearbox_replays_recorded_vehicle_updates`:
RPM median error.000244,p95 .001953,max.005859;gear/revs/load also pass. Fixture:
`core/fixtures/vehicle_20261007_gearbox.csv`,generator`work/gearbox_fixture.py`.
Each case seeds the previous state and actual DriveMode time delta, so this validates
per-update rules rather than an unseeded full run or mixer. Adapter now reads
`State::vehicle_gearbox`, replacing its separately advanced gearbox.

Hash5913246c is CRC32(`2dPanLevel`), named by the engine FEV's third parameter0..1.
Original00734630/007336f0/00733800 set1 for the player; adapter now sends it for
engine/turbo/ready. Earlier references to an engine `(distance)` envelope0x404 were
wrong: it indexes`2dPanLevel`,values1 at0 and.4 at.998682. Property meaning/mixing
remain unported; setting the parameter alone does not implement the envelope.
Low-pass,auto-pitch and crossfades verified/corrected2026-10-08 (above);category
volumes and stereo spatialisation remain debts. No waveform/audio-feel parity is claimed. Upgraded boost timing
also removes the duration mismatch against its already loaded sound cue.

How Bumblebee's sounds are found in the game's data and played by the mod. Everything here
comes from the data files; no code was read for it. Built and tested, NOT yet heard in
Arkham Knight by the user.

## Where the data is

- `characters\eng\bumblebee.fev` + `bumblebee0.fsb`: his own 38 events and 74 samples
  (transform parts, weapon changes, the engine, gun and missile fire).
- `levels\<level>\eng\mission.fev` + `mission0.fsb`: the shared events, 249 of them in
  `us_city_00`: footsteps by material (`AUTOBOT_SM_STEP_DEFAULT`...), jumps, landings,
  falls, hydraulics, dodge thrust, climbing, punches, impacts, explosions, road and skid
  loops, `VEH_SHARED_BOOST`, `UI_BOOST_READY`, `VEH_SUSPENSION_SM`. Every level carries
  its own copy; the mod reads one (setting `sound_level`, default `us_city_00`).
- `audio\eng\global.fev` + `Global_bank00.fsb`: UI sounds, and `ANIM_BUMB_CHARGE_FIRE` /
  `ANIM_BUMB_CHARGE_LP`.
- An event is named in the data by the CRC-32 of its name (the same hash as field names).
  `export\data\names.tsv` already resolves them.

Who asks for sounds:

- Clips, through event keys: `AnimPlaySoundEvent` (`id`, `autoStop`: ends with the clip),
  `AnimStopSoundEvent` (`id`), `FootStepEvent` (`side`). Bumblebee's clips hold 165 play
  events over 51 different sounds, 7 stops and 118 footsteps. Three of the 51 exist in no
  event file in the game (`ANIM_BUMB_TRANS_ROB`, `ANIM_BUMB_TRANS_VEH`,
  `ANIM_BUMB_TRANS_TIRE_SQUEAL`): leftovers; the `_A`, `_B`, `_G1` ones are what plays.
- `Character.soundTable`: `vehicleTransmission` = `VEH_BUMBLEBEE_CAMARO_RPM` (the engine),
  `vehicleDriftStart` = `VEH_BUMBLEBEE_CAMARO_REVS`, `vehicleTurbo` = `VEH_SHARED_BOOST`,
  `vehicleTurboReady` = `UI_BOOST_READY`, `vehicleSuspension` = `VEH_SUSPENSION_SM`;
  `vehicleDrive`, `vehicleDrift` and `hoverSound` are 0 for him.
- `Character.footStepFX` (`StepAutobotSmallPreset`): a sound and an effect per ground
  material; the first entry is the default material, `AUTOBOT_SM_STEP_DEFAULT`.
- `particleHitTable.meleeHitSparkPreset` (`MeleeHitSmallPreset`): the sound of a melee hit
  by the surface struck (`H2H_PUNCH_SM_*`, in each level's `mission.fev`); how it is picked
  is in `melee.md`. Played by the port since 2026-10-03.
- Not followed yet: `SoundPlayer` things on the scene tree, `SoundPhysicsReactor`.

## The event file format

The layout is in the header of `passthrough\tf2-core\src\formats\fev.rs`. Its test walks
all 172 `.fev` files in the install (24,085 events) to the end of their sound definitions.
In short: an event has a volume, randomisation, a "max playbacks" count and layers; a layer
lays sounds along a parameter (0 to 1 of its range) and may scale its volume by envelopes
over other parameters; a sound points at a sound definition; a definition is a weighted
list of samples in a bank. Kind 16 events ("simple") are one sound and nothing else.

The engine event `VEH_BUMBLEBEE_CAMARO_RPM` (parameters `rpm` 0 to 8800, `load` 0 to 1):

| sample | span on `rpm`  | recorded pitch at | volume |
|--------|----------------|-------------------|--------|
| idle   | 0 to 1845      | 850               | 0.75   |
| med    | 1438 to 5298   | 3000              | 0.81   |
| high   | 3847 to 8800   | 4000              | 1.0    |

Each loops, cross-fading at the ends of its span, its pitch following `rpm` ("auto pitch");
`load` scales the volume (0.26 at none, 1.0 from 0.55 up) and drives a low-pass effect.

Gear table `BumblebeeGears` in `bnxglobal.str#7`: six gears of `minRPM`, `maxRPM`, `ratio`
(1000 to 6000 at 2.97, up to 7000 to 8800 at 0.63).

Which table a robot uses (2026-10-20): the game takes it by the hash in his attributes'
`modeDrive.gearList` (007a2300 copies the drive mode's list, 007a4790 builds the gearbox) [game],
not by the name `<character>Gears`. Long Haul's list is `LonghaulGears`, the Audi's `AUDIGears`,
Bumblebee's `SportsCarA`; looked up by name, Long Haul and the Audi found none and their engines
never sounded. `tuning::load` now fills `drive_gears` from the hash (`sound::read_gears`);
`core\examples\ground_sound_check.rs` renders every ground robot's engine over its gears.

## What the mod does

- `tf2-core\src\formats\fev.rs`, `fsb.rs` (bank reader; MPEG decoding by the `symphonia`
  crate), `sound.rs` (`Library`, `Mixer`, `Engine`, `load_gears`). `anim.rs` gives each
  clip its `sound_events`; `character.rs` reads `Sounds` from the sound table.
- `ak-host\src\sound.rs`: decodes at start-up everything his data names (155 samples, under
  0.1 s), opens a shared 48 kHz stereo stream on the default device (WASAPI, about 40 ms
  queued) on its own thread. `animator.rs` reports each stretch of clip played
  (`clip_cues`) and clip changes (`clip_changed`); `takeover.rs` calls `tick` (engine,
  turbo, listener distance) and `silence` on leaving. Without a tick for 0.3 s (pause, map,
  load) the output fades to nothing.
- Settings in `tf2_passthrough.ini`: `sound_volume` (0 is off, default 1), `sound_level`.
- Listening without the game:
  `cargo run --release -p tf2-core --example sound_render -- <out.wav> <EVENT | list>
  [seconds] [rpm=a..b] [load=x] [level=name]`. Renders are in `work\sound\`.

## The gearbox (read from code and ported 2026-10-05)

The sections above were written for the Arkham Knight mod; the Bevy game uses the same
`core\src\sound.rs` from `game\src\sound.rs`.

- `FUN_00721f00` loads `soundTable` into the character: `+0x294` vehicleDrive, `+0x298`
  vehicleDrift, `+0x29c` vehicleDriftStart, `+0x2a0` vehicleTransmission, `+0x2a4`
  vehicleSuspension, `+0x2a8` vehicleTurbo, `+0x2ac` vehicleTurboReady, `+0x2b0` hoverSound.
  `FUN_00734630` starts the drive and transmission loops, `FUN_007336f0` / `FUN_00733800`
  play the turbo and its "ready". Parameter `5913246c` (name unknown) is set to 1 on these
  for a human player (controller kind 0; kind 2 is NOT a human: `FUN_00717be0` refuses to
  rumble for it).
- `FUN_007347c0` (the vehicle's effects and sounds, every update) ends by running the
  gearbox (`FUN_00731f80` -> `FUN_007a48e0`) and sending its `+0x18` as the engine event's
  `rpm` and `+0x30` as `load`. The gearbox is embedded in the drive mode's wheel set at
  `+0x28` and built with it (`FUN_007a4790`), so every change into the car starts it fresh.
- `FUN_007a48e0(turbo, dt)`, for a human player's car (wheel set `+0xc1` clear; with it set,
  an AI's, the throttle is speed / maxSpeed instead):
  - throttle `T` = 1 while a turbo ability runs, else the filtered trigger (`+0x24`);
  - the velocity is the body's; with `T` over 0.9 and no skid its flat part is held under
    `maxSpeed - |eased stick x| * turningVelocityDelta`. The speed is ONLY used to tell
    slowing (this update's less the last under -0.01) from not slowing;
  - `revs` (`+0x58`, kept between 1 and 1e9) and the two rates `up = engineTorque * 2 /
    (frontWheelRadius * mass)` and `down` the same with 7 (`FUN_007a2300`; 3.75 and 13.125
    for him);
  - stopped by the brake: gear 1, its minRPM, load 0.5, revs 1;
  - braking (not turbo, skid or reverse): `revs = down / sqrt(revs) * dt`, rpm =
    `T * revs * ratio * 600` but no less than the first gear's minRPM, load 0.25;
  - reversing: `revs += up / sqrt(revs) * dt`, rpm capped at the gear's maxRPM; the update
    after reversing ends: gear 1, its minRPM, revs 1;
  - not slowing (or off the ground): in the air rpm = the gear's maxRPM; on the ground
    `revs += up / sqrt(revs) * dt`, rpm = `T * revs * ratio * 600`; over the gear's maxRPM
    it changes up to the next gear's minRPM (not in a skid, where it stays at the max);
    out of gears, `revs` falls back to `maxRPM / (ratio * 600)` of the gear before the last
    and climbs again (flat out in sixth: 6497 up to 8800, about every 7 s);
  - slowing on the ground: `revs -= down / sqrt(revs) * dt / 2` (not in first, not in a
    skid), rpm as above; under the gear's minRPM it changes down to the lower gear's maxRPM
    (from first: its minRPM);
  - load = `clamp(rpm / rpm before - 1 + load, 0, 1)`, then moves `4 * dt` of the way to
    0.5.
  With the trigger up and the car still, rpm is 0 (the idle sample at the bottom of its
  pitch) and `revs` keeps climbing; that is what the code does.
- Ported as `sound::Gearbox` (test `gears_carry_the_revolutions_up_through_the_speed_range`);
  `sim.rs` hands its inputs out per update (`State::gearbox_inputs`). Assumed: the slide's
  boost counts as a turbo (the two timed abilities looked for are hashes `cee630d9` and
  `ea8020ae`, names unknown); the brake amount is over 0 exactly while the brake slows him.
  Listen without the game: `sound_render <out.wav> VEH_BUMBLEBEE_CAMARO_RPM 15 drive`
  (`work\sound\engine_drive.wav`).
- Event mode bits (word 7) are FMOD's: `0x08` 2D, `0x10` 3D, `0x100000` inverse roll-off,
  `0x04000000` custom roll-off. The roll-off and the left / right placement now apply to 3D
  events only (the interface's `UI_*` events were being faded by distance before).
- The engine layer's `FMOD Lowpass Simple` envelope on `load` (0.894 at no load, 0.855 at
  0.39, 1 from 0.5) is played as a one-pole low-pass. Assumed: 0 to 1 is 10 to 22000 Hz by
  ratio (about 9.7 kHz at no load, open from half load).

- Seek speed (added 2026-10-05, after the user found the gear changes "spliced"): an
  event parameter's record carries a seek speed (the word after its flags), and the
  engine's `rpm` and `load` both have 1.0. The mixer now glides a set value at that speed
  instead of jumping (test `a_change_of_gear_glides_at_the_events_seek_speed`). Assumed:
  the unit is shares of the parameter's range a second (8800 rpm a second, so a change
  from 6000 to 3000 takes a third of a second); the game's player was not read.

### Everything assumed in the 2026-10-05 work (none of it heard against the original)

The gearbox's inputs:

1. Mass. `FUN_007a2300`'s second argument (owner `+0x140` -> `+0x198` -> `+0x30`, 1000
   when that object is missing) is taken to be the vehicle body's mass, 5000 from run 1
   (`sim::VEHICLE_MASS`). The field itself was not identified. It sets both rates.
2. The trigger. The game gives the gearbox the FILTERED trigger (mode `+0x204`, eased by
   `triggerFilterStrength`); RESOLVED 2026-10-07: the installed strength is read,
   DriveMode enters with filter1 and now supplies the filtered trigger.
3. The brake. "Brake amount over 0" (wheel set `+0x14`) is taken as: the brake is held, a
   wheel is down, and he is not reversing. Where `+0x14` is written was not found.
4. Turbo. The flag is "one of two timed abilities is active" (hashes `cee630d9`,
   `ea8020ae`, names not in the data). RESOLVED 2026-10-07: `0085c2f0` constructs
   standard cee630d9 and drift ea8020ae. Both start `vehicleTurbo` via `00732e60`;
   the host now uses the actual start event, so drift plays it too.
5. The velocity. The game reads the body's velocity when the sound update runs; the port
   takes it inside the car's update, after the speed caps and before that update's tyre
   forces. The order of the two in the game's frame was not read.
6. A fresh gearbox on every change into the car. `FUN_0085c2f0` builds the wheel set and
   the gearbox with it; that it runs on each entry into the drive mode (not once per
   character) was not checked.
7. Human or not. The player's wheel set has `+0xc1` clear (it is set for controller kind
   2). That kind 0 is the human rests on `FUN_00717be0` refusing to rumble for kind 2 and
   on `FUN_00725a40`; the controller classes themselves were not read.
8. The first third of `FUN_007a48e0` (the velocity and the stick's cap on it) is from the
   pseudocode only; the rest was checked in disassembly (`007a4b60` to `007a4fd8`).
9. A gear's two middle fields (`+0x08`, `+0x0c`) are not in the data and not used by the
   routine; taken as nothing.

The mixer:

10. RESOLVED2026-10-08: seek speed is in shares of the parameter's range a second
    (1.0 =8800RPM/s), checked in004ce900/004cd990.
11. RESOLVED for the engine2026-10-08: logarithmic10..22000Hz, TWO cascaded stages
    with the game's coefficient/bypass. Engine has exactly one such effect envelope.
12. The mode bits' meanings (`0x08` 2D, `0x10` 3D, `0x100000` inverse roll-off,
    `0x04000000` custom roll-off) are FMOD's published constants, matched to the files
    (the `UI_*` events have `0x08`, his own `0x10`); the game's player was not read. An
    event with the custom roll-off bit gets no roll-off from the minimum distance.
13. Left / right: an equal-power balance from where he is across the camera's look, one
    value for every sound. A stand-in for FMOD's 3D, not a reading of it.
14. The values the mixer moves a parameter by are applied once per output block (about
    10 ms), not per sample.

Fade fractions/curves and auto-pitch resolved2026-10-08 (above). Pitch randomisation's
unit remains assumed; the engine event/definitions have no pitch randomisation.

Not an assumption but worth knowing: `work\sound\engine_drive.wav` is the gearbox fed a
made-up drive (speed rising to 35 over four seconds, then falling), not a recording.

## The tyres and the suspension (read from code and ported 2026-10-05)

The first 480 lines of `FUN_007347c0`, every update of the car, before the gearbox:

- `speed` = the length of the body's velocity.
- Skidding (checked in disassembly, `00734a53` to `00734b42`) is any of:
  - the slide: button 0x14 (VehicleDrift) held, mode `+0x1f5` set, and `speed` over 1;
  - the brake: mode `+0x40` over 0.5, not reversing (`+0x4d`) and not waiting to (`+0x4e`);
  - a hard turn at speed: |the controller's stick x| at 0.9 or more (its vtable `+0x24`)
    and `speed` at or over 0.85 x `FUN_0085f8b0(mode, 0)` (the speed the trigger allows).
- With no wheel on the ground: both loops and the tyres' effects stop.
- Otherwise for each of the four wheels (surface now = wheel `+0x98`, the material of what
  its ray hit, 0 for none; the one before = `+0x94`):
  - skidding and `speed` over 1 and a surface: the skid's effect for that surface at the
    wheel (`vehicleTireSkidFX`, character `+0x35c`, through `FUN_0077d5d0`) and the
    `TireSkidMark` is triggered; at ONE rear wheel (index 2 or 3: 2 when the velocity is
    to the body's left, else 3) the preset's SOUND for the surface is started
    (`FUN_0077d890`), and started again when that wheel's surface changes. Not skidding or
    slow: the loop stops and the mark ends;
  - `speed` over 1 and a surface: the same with `vehicleTireRoadFX` (`+0x360`): a rolling
    loop at that one rear wheel, an effect at every wheel. Slow: it stops.
  - `FUN_0077d890` starts the event with `speed` (and a second parameter, hash `d9a785a4`,
    name unknown) = the body's speed; `FUN_00595770` then ties the event to the body, and
    an event with a `speed` parameter follows the body it is tied to.
  - rear wheels only: `(wheel +0x70 - wheel +0x7c) / wheel +0x80` = how much the spring
    shortened in this update (`+0x7c` its length now, `+0x70` the update before, kept by the wheel's
    ray step `FUN_007a1460`) over `suspensionLength`; over 0.05, the largest is kept.
- After the wheels: if that amount is over 0, `vehicleSuspension` (`+0x2a4`) plays at that
  wheel and the code sets parameter hash `3283b7ff` = amount x 10. That hash is
  `impactForce`; the event's parameter is spelt `ImpactForce` and the lookup
  (`FUN_00594d90`) compares CRC-32s of the names as they are. So the force reaches nothing:
  the thump plays with its parameter where it starts.
- His presets ([data], test `his_tyres_sound_by_the_ground_under_them`): skid =
  `VEH_SPORTS_SKID_PAVE` on the default list of surfaces, `VEH_SPORTS_SKID_DIRT` on Dirt,
  Water, Mud, Sand; road = `VEH_ROAD_PAVE`, `VEH_ROAD_GRASS` (Dirt, Plaster, Wood),
  `VEH_ROAD_MUD` (Water and Mud), `VEH_ROAD_SAND`. Their `speed` runs 0 to 60 (road) and 0
  to 70 (skid).
- `vehicleDriftStart` (`+0x29c`, `VEH_BUMBLEBEE_CAMARO_REVS`) and `vehicleDrift` (`+0x298`)
  are written by the loader and read by NO function in the export (grep of `+ 0x29c)`:
  only the loader among the character's code). The rev burst is not "missing": as far as
  static reading shows the game never plays it.
- By the way: in `FUN_0085cad0` the slide is flag `+0x1f5` with button 0x14 HELD and a
  wheel down, and flag `+0x1f4` with button 0x16 (VehicleTurbo) is the turbo's test;
  `driving.md` "The slide" names 0x16 / `+0x1f4` for the slide. The sim is right either way.

Ported: `sound::tyre_sounds` (core, test
`the_tyres_skid_on_the_slide_the_brake_and_a_hard_turn_at_speed`), fed by
`State::tyre_inputs` (`sim.rs`, test `the_car_tells_its_tyres_what_to_sound_like`), played
by `game\src\sound.rs` (`tyre_loop`). Listen without the game:
`sound_render <out.wav> VEH_ROAD_PAVE 8 speed=1..40` (`work\sound	yre_road_pave.wav`,
`tyre_skid_pave.wav`, `suspension.wav`).

Assumed / stand-in in this part (none of it heard against the original):

15. Field mapping RESOLVED 2026-10-07: `007a3470` writes the wheel set's+0x14,
    DriveMode+0x40, from XMM0 and sets each wheel's+0x55 when it equals1.
    `0085dc96..0085dca5` supplies1 for the braking branch. Remaining assumption:
    the port still supplies the raw brake trigger to tyre sounds; the game's complete
    digital brake gate/zero branch and its interaction with turbo/reverse need checking.
16. The stick behind the hard-turn test is the steering stick's x (the controller's vtable
    `+0x24` was not read).
17. RESOLVED 2026-10-07: `FUN_0085f8b0(mode, 0)` skips the trigger (raw
    `0085fa3e..0085fa65`), so the tyre threshold uses maxSpeed. Its square-root
    corner branch is gated by a nonplayer controller; it does not apply to Bumblebee.
    The drive update uses dt>0 and does multiply by the filtered trigger.
18. That an event tied to a body has its `speed` kept at the body's speed every update
    (`FUN_00595770` finds the body; the update that writes the parameter was not read).
19. The suspension event's parameter starts at its minimum (-0.1), the mixer's rule for
    every parameter; FMOD's own starting value was not checked.
20. Stand-in: the ground is `0 - Default` everywhere (the arena has one ground).
21. Replaced 2026-10-07: actual rear-wheel previous/current spring length supplies
    compression, including bumps and weight transfer. The maximum-compression rear
    wheel places the suspension cue. The original impactForce hash casing is retained.
22. Replaced 2026-10-07: rolling/skid loops follow the selected rear wheel (2 for
    negative lateral velocity, else3). Individual mixer spatial values still use
    stereo distance/pan in place of FMOD 3D; original comparison is pending audio capture.

Ported 2026-10-05 from this routine: the tyres' particle effects by surface and the skid marks
(`TireSkidMark`): `docs/particles.md` "Skid marks and the tyres' effects".

## Fitted, assumed, stand-in, missing

- Replaced 2026-10-05: revolutions and load are the game's gearbox (above), no longer a
  rule of ours.
- Resolved2026-10-08: footsteps probe the named animated foot and select its material.
  The authored arena still supplies Default ground.
- Stand-in: independent source distance and stereo projection replace FMOD's full3D;
  no front/back speaker matrix, Doppler or reverb.
- Resolved2026-10-08: FSB loop endpoints replace the invented512-frame seam blend.
- Resolved2026-10-08: fade numbers are fractions of the sound's own length, with its
  curve; auto-pitch formula and low-pass are checked in the embedded FMOD player.
- Assumed: pitch randomisation units (1.0 = four octaves), inverse roll-off
  outside the minimum distance. None matters much except on the engine.
- Since 2026-10-05: the tyres' rolling and skid loops and the suspension's thump ("The
  tyres and the suspension").
- Resolved2026-10-08: car scenery impacts/scraping, player radio chatter, category
  gains/options and0x404 property dispatch. Stereo3D-level blend is explicitly assumed.
  Music/ambience excluded by user request. Global NPC chatter integration remains.
- Not checked: Arkham Knight keeps playing Batman's own sounds (footsteps, cape, grunts)
  for the hidden, carried Batman. Whether they are audible under Bumblebee's is for the
  first listen.

## What `<install>\audio` holds (looked at 2026-10-05, the user asked)

Listed with `cargo run -p tf2-core --example fev_list -- <file.fev>... [word]` (categories,
or the events whose name holds the word) and `tools\tf2_fsb.py list`. Only `global.fev` is
loaded by the game so far.

- `eng\global.fev` + `Global_bank00.fsb` (44 events): the interface (`UI_*`: menu, pause,
  `UI_LOCK_ON` / `UI_LOCKED_ON` / `LOCK_LOOP` with a `pitch` parameter, `UI_OVERDRIVE_READY`
  / `UI_OVERDRIVE_START`, `UI_REGEN_START`, `UI_PICKUP_ENERGON_A`), `ANIM_BUMB_CHARGE_LP` /
  `ANIM_BUMB_CHARGE_FIRE`, `WPN_SHARED_ELEC_STUN` (`param02` 0 to 10), `MX_STOP`.
  The code plays some of these by id: `FUN_00736790` (the special's meter full: event
  `94d431be`) and `FUN_00736060` (the special starts: `2338e19e`); not yet matched to
  names or played by the port.
- `eng\global_streaming.fev` + `Global_Streaming_bank00.fsb` (220 events): 81 `music`
  events (`MX_<level>_<side>_<mission>_P1 / _INTRO`, intros with `param00` 0 to 100), 33
  ambiences `BG_*` (most with a `cameraHeight` parameter, 0 to 100: `BG_US_CITY_01`,
  `BG_US_WEST_A`...), 83 announcer lines `ANNO_*`, UI stingers, and `ProgrammerSoundPlayer`
  (category `VO_LOUD/CHATTER_PLAYER`). Four music samples are Bumblebee's
  (`mx_hoover_int_bbee_1`, `mx_tran1_bbee_2`, `mx_tran1_bbee_chase`, `mx_tranbarr_bbee_chase`).
- `eng\chatter.fev` + `CHATTER.fsb` (3753 samples, 2 events): battle chatter. The events
  are dummies (`BattleChatterDummy`): a line is a SAMPLE picked by name and played through
  a programmer sound. Bumblebee has 111: `bumblebee_att_gen` 9, `tac_att` 9, `inj_cha` 25
  (hurt), `die_cha` 25, `spo_cha` 11, `tau_cha` 17, `tau_mp` 15 (his are radio / synth
  sounds, per character faced: `_sid_`, `_lon_`, `_sta_`...). His pack's
  `Character.battleChatter` (`bumblebee_battlechatter`, 115 entries of `action`,
  `category`, `subCategory`, `eventID`, `frequency`) says which line for what. The code
  that picks and plays them is not read.
- `music_system.fsb` (183 stereo loops, 201 MB) + `MUSIC_SYSTEM.fev` (no events: the
  interactive music's cues; our reader finds nothing in it).
- `eng\war_room.fev` (959 `VO_LOUD` lines: the mission-select hub) and `briefings\eng\`
  (57 briefings: a voice, a music and five `SFX_BRIEFING_*` events each): not gameplay.
- `fre`, `ger`, `ita`, `pol`, `rus`, `spa`: the same in other languages.

Usable next for Bumblebee: an ambience under the test arena (`BG_US_CITY_01`), the
lock-on and special's interface sounds, his hurt / attack chatter.
