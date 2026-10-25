# Climbing: research notes

Read 2026-10-03 from the executable and checked against the 12 climbs in run 1
(`work\climb_trace.py work\traces\rotf_run1.bin`, all of them listed in `work\climbs_run1.txt`).
Addresses are for the analysed `Transformers2.exe`.

## Level roof and corner follow-up (2026-10-14, Codex)

[measured, OUR port] `work/shots/escort_climb_roof_20261014.png` shows Bumblebee
standing on the escort roof at z 55.6, IdleStandSet/Idle, after the full pull-up.
Inputs: `--mission USA_East_LTrain_A_Escort --start -220.678,-65.623,0 --climb-from 1
--stick 1 --stick-from 2.5 --stick-until 7.5 --after 10` with `--shot`. The earlier
8 s capture stopped the stick at 6 s and showed hanging instead. No source change
was needed for that roof exit.

[game] Endpoint extensions rechecked in 007d6b70 (full 105 cleaned lines, disassembly
007d6b93..007d6ba7 and 007d6d85..007d6ea0). Each saved valid corner normal dot the
wall normal must be STRICTLY >0.70710677; the qualifying side gains radius+2.
Base width stays the sum of the unextended distances. The frame origin shifts by
the left extension, which is stored separately. Exactly 45 degrees does not extend.

[game] Sweep 006f7130, endpoint portions 150..370, rotates through twin half-edges
around either endpoint through low-side triangles until a higher floor is found.
Open or canNotClimb cells terminate it. Adjoining normals with dot >0.98 continue
the straight wall; the first other valid boundary supplies the saved corner normal
and Euclidean endpoint distance (006f799d threshold, 006f7823 climb flag).
This replaces the port's former whole-mesh scan and projected interval join.

[code] NavMesh exposes `edge_twin`; LevelWorld finds each endpoint through the fan;
Ledge carries corner normals; Wall::from_ledge takes the avatar radius and retains
base_width and left_extension. Box-arena faces are right-angle corners, hence no
extensions there. Tests cover shallow, exactly-45 and right-angle corners, frame
origin/width, and split straight edges. Game check and full suite follow in status.md.

[stand-in] Collision heights/probe placement remain as before (`ai-nav-floor.md`).
The optional ceiling/layer gate controlled by sweep param_5 is not represented in
World's query API; its caller meaning must be settled before AI/layer-boundary parity
is claimed. Corner rules have no original-game recording comparison yet.

Later on 2026-10-14: the collision-height workaround above was removed after
the uneven-facade diagnosis below. Climbing now uses authored nav-cell heights.

## Uneven facade/contact follow-up (2026-10-14, Codex)

The user saw the active shimmy pin against the building's non-flat facade.
`work/shots/escort_corner_active_20261014.png` and its log show the correct
ShimRight clip/root speed (9.8 m/s), with position near (-219.733,-67.117,15.299).
Longer input did not advance it. Endpoint extensions did not complete complex
surface climbing. The roof exit remains a separate successful check.

[game] Reread 00856d00 66..87: STEP+0x34d skips the 10% pullback to the nav wall.
The original's contact scan sets that bit from actual points in a strict forward
45-degree cone (008524b0, 00852d66..00852dbf; `character-ground-test.md` S5).
The port had assumed there could be no contact while farther than radius+0.1
from the nav plane. That assumption fails where facade collision protrudes.
`climb::Body` now carries front_contact, and World/Arena/LevelWorld/FightWorld
provide it. [stand-in] LevelWorld currently samples three body-width spheres,
rather than retaining the original pencil's contact manifold.

[measured, OUR port] `core/examples/climb_contact_check.rs` uses installed clips
and the escort collision/nav mesh. At dt 0.008 the old pullback requested an extra
0.045 m toward the wall each frame, opposing the 0.078 m shimmy so that collision
accepted approximately zero movement. At dt 0.016/0.032 the same replay advances.
`work/climb_contact_before_20261014.log`. Pullback was being applied at the render
rate, multiplying its strength relative to run 1's 0.032 s updates.

[code] Retain 0.9^(dt/GAME_UPDATE) of the old distance from -radius, keeping exactly
10% per recorded update. [assumed] Exponential composition across fractional host
updates, as with the existing air control; compare contact ordering with an
original uneven-wall recording to settle it. Logged in status.md. The fixed replay
advances past the pin at all three step sizes (`work/climb_contact_fixed_20261014.log`).
It then loses the ledge; some queries return no wall, and the next returned normal
(0.74367,-0.66855) is outside the original facing gate at yaw 0. That is still open.

Trials that did NOT establish a fix: projecting ring-probe shortage onto the
horizontal normal, and a continuous slide using the original pencil support/dimensions.
Both still pinned at dt 0.008 before the cadence correction. The ring projection
was removed and the trial mover is diagnostic only (`--pencil`).
`StaticWorld::pencil_cast` is tested query groundwork: original dimensions and
centre from 006e7e90 / 006e7b60 (`character-ground-rule.md` slot mapping), existing
emulated support helper 005aff10. [stand-in] Minkowski ray-GJK and per-triangle
dispatch replace the original swept-pair/manifold driver. [assumed] Support under
the normal approximates a contact point; settle with the manifold generator.
It is not connected to movement and does not complete pencil physics.

[game] 00723f50 reread in full: forced refresh (param2=1) bypasses the already-valid
avatar early return; it does not bypass the player facing gate in 007d6b70.
006f7130 initial 1..149 reread: walks the centre segment through triangles and
chooses the first higher floor, subject to its layer selector. No evidence here
supports bypassing the 45-degree gate to make an arbitrary corner traversable.

[game/code] That initial read exposed another omitted rule: 006f7130 stamps the
current triangle and excludes stamped neighbours (40..53). The port used only
intersection distance. At a real escort replay point it crossed low triangles
33817 -> 34308 -> 33817: the reversed edge rounded a few micrometres farther,
then the walk falsely ended. A regression at
(-217.97708,-67.953964,16.367075), facing +y, fails before and passes after the
visited guard. `work/climb_ledge_visited_before_20261014.log`, `_after_20261014.log`.
The earlier fixed replay's no-wall results precede this guard. The separate
5 mm point-location band/persistent query seed and complex collision remain
open; restored guards are not an original-game corner comparison.

[game/code] Also restored the original higher-floor candidate gate (006f7130
31..34/68). The apparent 48-degree wall above was a low-floor internal diagonal,
floor -18.108 on BOTH sides. The collision-height workaround incorrectly promoted
it when a probe saw facade detail at 17.8 m. It was not a real nav wall or evidence
of an original limit. Candidate selection now requires the original nav step.
[assumed] An initial on-edge classification with no forward exit retries on the
exact containing side; reason/settlement in status.md, code tag in navmesh.rs.

[measured, OUR port] Final capture at 4.5 s has ShimRight still playing, with
position (-204.817,-67.150,15.257), 14.916 m beyond the earlier pin at that same
time. Reviewed `work/shots/escort_climb_facade_final_20261014.png`; log of the same
name under work. Final headless replay at 0.008/0.016/0.032 reaches the side limit
and hangs without losing the ledge (`work/climb_contact_surface_final_20261014.log`).
Two installed-position regressions fail before and pass after the candidate and
edge-band corrections. Full core suite: 964 pass, 9 existing ignores. This verifies
the reported facade in our build; it is not full original-game climbing parity.

[game/code, latest] Restored the original height source (006f7130 99..102):
bottom = near cell floor, top = far cell floor. The height workaround still lost
the wall at dt 0.016 near (-204.034,-67.369): its local probes gave bottom 15.42,
top 17.90, hence a 2.48 m wall below the minimum. The authored values there are
bottom -18.10835, top 17.86849, a valid 35.98 m nav wall. The earlier report that
ALL three step sizes reached the limit was incorrect: 0.016 had this remaining
loss. A third installed-position regression now asserts the authored values.
All 964 core tests pass with the height proxy removed. Final replay/captures
after that removal are recorded in status.md. Contact solver, original query seed
and layer selector remain incomplete; body collision still uses the ring proxy.
Latest same-input facade capture remains ShimRight at x -204.883, 14.850 m beyond
the old pin. All three final replay steps reach the side limit without release.
Roof regression still ends IdleStandSet/Idle at z 55.635 after 10 s; reviewed
`work/shots/escort_climb_roof_nav_heights_final_20261014.png`.

## What starts a climb

The graph (machine Climb, `state-machine.md`): from Idle, Move, Jump, Fall (and a few more),
`connectionClimbStart` (the Climb button down) together with `connectionEnvClimbable`.

`connectionEnvClimbable` is `ConnectionEnvironment` with `envTest` 96c5dbae, handled in
`FUN_0084cee0` (1a079e6b is the same test negated):

1. `FUN_00723f50` on the character's physics proxy fills a climb record (layout below). The
   query radius is the avatar's radius (owner `+0x170`, indexed by form `+0x244`; 2.0 for
   the robot); for characters not controlled by a player (`FUN_00711210`, the controller
   type at `+0x100`) it is 1.2 times that.
2. The record is copied to the owner at `+0x350`, and the test passes when it is valid and
   owner byte `+0x3f8` is clear.

The search (`FUN_007d6b70`, sweep `FUN_006f7130`) does not look at collision geometry. It
walks the level's 2.5D ledge mesh (triangles with a floor height, a ceiling height, flags;
at `[DAT_00cf59f4 + 0x4f8]`):

- From (x, y, z + `owner+0x140`[form]) along the facing for (radius + 2) * sqrt 2 (5.66 for
  the player's robot). `+0x140` is how far the body shape rides over the ground: the shape
  builder (`FUN_00853550` -> `FUN_006e7b60` -> `FUN_006e7e90`, given the form's height
  and radius, which go to `+0x160` and `+0x170`) makes a `CollisionPencil` out of the top
  6/11 of the height, takes 5/11 of THAT as the gap under it (`+0x150`, 1.339 for the robot)
  and stores 0.8 of the gap at `+0x140`: height * 24/121 = 1.071 for the robot's 5.4, and
  1.07 in run 1. (The ground test `FUN_008524b0` compares the distance to the ground with
  0.3 and 0.9 of it.) This also confirms 5.4 as the robot's height at `+0x160`.
- It crosses triangle edges until the floor on the far side is above the start height: that
  edge is the wall. Bottom = the floor on the near side, top = the floor on the far side.
- The wall is valid ("climbable", byte `+0x80` of the sweep's record) when the top minus
  the bottom is at least `modeJump.jumpHeight` (6 for Bumblebee; anything lower is "a ledge
  to jump onto", byte `+0x86`), the top cell is not flagged (bit 0 of its `+0x24`) and the top
  is not open (FLT_MAX).
- Along the wall it walks neighbouring edges while they are within 0.98 (cos 11.5 degrees)
  of the same line and still a wall, giving the distance to each end. At an end where the
  ledge turns by less than 45 degrees (`dot > 0.7071`), that side is extended by radius + 2.
- Then (`FUN_007d6b70`): invalid if the start point is radius + 2 or more in front of the
  wall plane; for a player-controlled character also invalid unless he faces within 45
  degrees of straight at the wall (`forward . n <= -0.7071`, `n` pointing out of the wall).
- The top cell carries a value (the sweep's `+0x2c`, from the cell's `+0x18`) that becomes
  "extra above the top" (`+0x394` below). Where there is another ledge above
  (byte `+0x84`), the height runs on to it.

## The climb record (`+0x350` on the owner, 0x48 bytes)

| offset | what |
|---|---|
| `+0x350` | valid |
| `+0x354` | world to wall, 3 rows and a translation row, row vectors: `local = p * M` |
| `+0x384` | width, with the extensions |
| `+0x388` | width without them |
| `+0x38c` | the extension on the left end |
| `+0x390` | height (top minus bottom) |
| `+0x394` | extra above the top (0 for every wall in run 1) |

The wall frame F (whose inverse is M): X along the wall from its left end (`(-n.y, n.x, 0)`),
Y into the wall (`-n`), Z up, origin at the left end of the extended wall at the bottom.
So in wall space x runs 0..width along it, y is negative in front of it (-radius when
touching), z is the height above the bottom. Checked on every climb of run 1.

Two places in ClimbMode use ROWS of M as if they were axes (row 0 at the side limits, row 1
in enter). For a wall along x or y the row and the axis are the same line; for any other
wall they are not. The port keeps the original's arithmetic.

## ClimbMode (class d7068c31, table `00b626d0`)

Tuning `modeClimb` (loader `FUN_0085ad50`, block `+0x1a0`): `pullUpDist` 8,
`edgeLimitDist` 3, `wallOffset` 0 (not read by the update), and a fourth value (hash
bbc71449, name unknown, not in Bumblebee's data, default 50) that is the deceleration of
the speed he arrives with. Mode object: `+0x18` state, `+0x1c` hand, `+0x20` speed he
arrived with, `+0x2c` top target, `+0x38` facing at the top, `+0x44` clip phase, `+0x48`
hand already swapped this cycle, `+0x49` first update, `+0x4c` tuning.

States and clips (hand 0 is L, 1 is R; the names resolve from the hashes in the code):

| state | clip | |
|---|---|---|
| 0 | none of its own (see "The catch clip") | entered |
| 1 | `WallL/R_ClimbUp` | 46 ticks, +7.3 up, not looping |
| 2 | `WallL/R_ClimbDown` | 60 ticks, -26.65 up, looping |
| 3 | `WallL/R_Idle` | |
| 4 | `WallL/R_ShimLeft` | 46 ticks, 7.5 left, looping |
| 5 | `WallL/R_ShimRight` | 46 ticks, 7.5 right, looping |
| 6 | `WallL/R_PullUp` | 56 ticks, 7.45 up and 5.3 forward |

Clips are started only on a change (`FUN_00748f00` with blend -1, i.e. the clip's own).
The movement on the wall is the clips' root motion; ClimbMode's control vector is (1, 1, 1).

Enter (`FUN_00857dd0`): state 0 (9 for the length of the function's first lines). Carrying
something whose flag bit 12 (`+0x148`) is set: `Carry_Wall_PlaceOnBack`, hand 1, rumble 8.
Carrying something else: hand 0 and no clip. Carrying nothing: the hand is left as the last
climb left it and the clip 685a280c is asked for (below). The body's velocity minus its
part along M's row 1 is kept as the arrival speed.
If he arrives within the side limits nothing moves; outside them he is put at the limit
(walls narrower than two limits: at the middle). Arriving higher than height - pullUpDist,
the arrival speed is set to straight down at sqrt((excess) * 100): with the 50 deceleration
that runs out exactly at the pull-up line. Sets the control vector to (1, 1, 1) and the
first-update flag.

Update (`FUN_00856d00`), with the wall position (x, y, z) and the raw pad stick (input
source slot `+0x30`, the one the car steers with):

1. In state 6 with the clip finished: message "exit pull-up" (98799ecd), done.
2. The climb record is refreshed. If it went invalid: message "exit bottom" (a884a9c4)
   unless in state 6.
3. The arrival speed loses 50 a second. Outside the side limits (x < edgeLimitDist or
   x > width - edgeLimitDist) its part along M's row 0 is removed.
4. First update: he is put at (x, -radius, z) in wall space and turned square to the wall
   (seen in run 1: facing snapped from (-0.92, 0.40) to (-1, 0)). Otherwise, if he is more
   than radius + 0.1 off the wall (and owner `+0x34d` is clear), y goes 10% of the way back
   to -radius each update (seen in run 1 at 665 s).
5. He moves by the arrival speed.
6. State 0: when the clip finishes or the stick is pushed (squared over 0.01): state 3.
7. Stick centred: idle (3); but above height - pullUpDist he goes over the top by himself.
8. Stick by angle `atan2(-x, y)` (up 0):
   - up, within 45 degrees: below height - pullUpDist climb up (1); above it the top: with
     no extra above the top, message "exit pull-up" and state 6 (`PullUp`); with some,
     message "exit jump-up" (0230a6e3). The top point (his x, y and the wall's top z, plus
     the extra) and his facing are kept for those.
   - down, beyond 135 degrees: if z < 0.1 - 1.07 or he is on the ground, message "exit
     bottom"; else climb down (2).
   - left (45 to 135): x at or above edgeLimitDist shimmies left (4), else idle (3).
   - right: x at or below width - edgeLimitDist shimmies right (5), else idle (3).
   - left and right also turn him toward square to the wall at pi radians a second
     (`FUN_00717770`).
   States 1 and 2 restart their clip only when it has finished (state 1) or keep it
   (state 2 loops).
9. In states 1 and 2 the hand swaps once each time the clip passes 30% (`FUN_00858490`),
   so the next clip is the other hand's. Seen in run 1 at phase 0.34.
10. A pad effect (`FUN_00717c40`, a table of rumble presets) is started every update by
    the state the update leaves him in: 9 for up and the shimmies (1, 4, 5), 10 for down
    (2). Preset 9 is type 3 for 0 seconds, which is silence (and stops whatever was
    running); preset 10 holds the heavy motor at 0.2. Ported 2026-10-04
    (`sim::step_climb`; the table is in `docs/melee.md`).

Exit (`FUN_008583f0`): only animation bookkeeping.

### The catch clip (2026-10-03)

- `stateClimb`'s enter (`FUN_0087a700`) starts the set `ClimbSet` (`FUN_0084ddb0` with
  7048543e) and then the climb mode. A set starts on the clip its entry rules pick
  (`animation-rules.md`), and `ClimbSet` has two: `Wall_Catch` under `animRuleListInAir`
  (`I_IN_AIR` equal 1), else `Wall_Catch_Ground`. Both ids use the same `AnimFile` (34
  ticks, no root travel, a sound at tick 26); `Wall_Catch` has blend 0, `Wall_Catch_Ground`
  0.2. [data]
- The mode's enter then asks the set for the clip 685a280c (`thunk_FUN_01193520`, a walk of
  the set's clips by id). That hash is the id of no clip in any pack (it is not in
  `names.tsv`, which holds every name of every data file) and none of some 500 likely
  spellings gives it, so the lookup returns nothing and no clip is started: the set's own
  catch clip keeps playing. `Wall_Catch` and `Wall_Catch_Ground` are named by no code
  (`work\find_imm.py`: their hashes are nowhere in the executable). [game]
- State 0 ends when the player's current clip has finished (`FUN_0074ad50`), whatever it is,
  or the stick is pushed; the hand is then set from the carried thing's flag (0 with none).

### `owner+0x34d` (2026-10-03)

Cleared at the start of the body's contact scan (`FUN_008524b0`) and set when a contact lies
ahead of him: its offset dotted with his forward is positive and the offset's horizontal
length squared is under twice that dot squared (within 45 degrees). Step 4's pull back to
the wall is skipped while it is set, which is why run 1 hangs at 2.10 to 2.20 where the
collision mesh stands in front of the ledge mesh: he is touching it, so nothing pulls him in.

## Out of the climb (the graph)

- "exit pull-up" -> `stateClimbPullup` (ClimbMode again, so the PullUp clip plays out) ->
  Fall when the clip has finished. In run 1 he is in Move or Jump (falling a few tenths) on
  the top right after.
- "exit bottom" -> Fall.
- "exit jump-up", or Jump pressed -> `stateClimbJumpUp` (`Wall_JumpUp`, 12 ticks, 3.6 up,
  first mode), then Jump when the clip is done. Stick back with Jump -> `stateClimbJumpOff`
  (`Wall_JumpOff`), then Jump.
- Climb let go and pressed again -> Fall.
- JumpMode entered while ClimbSet plays is the climb jump (type dfb7146d): vertical velocity
  cleared and a push of 20 along the facing, height `climbJumpOffHeight` (15), speed
  `climbJumpOffSpeed` (30), control 0 (`bumblebee-mechanics.md`). Run 1 has no wall jumps,
  so none of this is checked.
- Type 9c689146 (`climbJumpUpHeight`, 15) has no start of its own: the constant is in the
  executable once, in the height table `FUN_00874b30`, as a comparison; nothing stores it
  (`work\find_imm.py 9c689146`), and no data file holds it. So the jump up off a wall is the
  same climb type as the jump off (JumpMode's enter sees `ClimbSet` playing), and
  `climbJumpUpHeight` is dead in this build. [game]
- The top target (`+0x2c`) and facing (`+0x38`) are read by the Jump state's update
  `FUN_0087dde0`, only when the state before was `stateClimbJumpUp` (fba80d7b): falling
  (vz < 0), the facing set (squared length over 0.9; enter zeroes it), below the target's
  z, less than his radius past the target along that facing, and facing within 0.98 of it,
  a ball of his radius is swept (`FUN_005be5a0`) straight down at target + facing * 2.25
  radii, from the target's z plus his height (`FUN_0070abe0`: owner `+0x160`, plus how far
  the body's foot stands over his position when positive) to his own z. A hit above his
  feet, less than 1.5 above them, under his feet plus his height, and more than 0.25 below
  the sweep's start: he is put there with the kept facing (`FUN_00854790`) and no velocity
  (`FUN_00855940`). Since only "over the top" sets the target, this is the landing of the
  "exit jump-up" from a wall whose top cell carries extra height. Ported in `sim.rs`
  (`ledge_catch`).

## Run 1, numbers to replay

- Hanging distance 2.00 (2.10 to 2.20 on a few walls where the collision mesh stands a little
  in front of the ledge mesh).
- ClimbUp: 7.1 to 7.3 up per 0.775 s, vertical speed peaking at 16.82; hand swap at 0.34.
- ClimbDown: -26.65 a second (looping).
- Shimmy: 9.78 a second (7.5 per 46 ticks).
- Pull-up starts the update z passes height - 8 (35.3 on the 43.3 wall) and ends 0.94 s later
  (56 ticks at 60) on the top, about 4.7 past the wall.
- Arrival speed: (0.13, -1.72) -> (0.01, -0.11) in one update (50 a second).

## In the port (`core\src\climb.rs`, `sim.rs`, 2026-10-03)

- The climbable test, the wall frame (the matrix kept as the original keeps it, rows and all),
  the climb mode's enter and update, the messages, and the wall jumps' launch: [game].
- The ledge mesh: the arena's boxes stand in for it (`Arena::ledge`). Every box rises from
  the ground; the first side crossed into ground above the start height is the wall, joined
  with in-line sides of touching boxes. No corner extensions (boxes have none), no "extra
  above the top" (always pull-up, never jump-up at the top). [stand-in]
- The catching clip is the set's: `Wall_Catch` caught in the air, `Wall_Catch_Ground` from
  the ground (`Climb::enter`'s `in_air`). [data] That `I_IN_AIR` is the state machine's
  ground flag is [assumed] (the integer getter `FUN_0070cae0` is not read).
- The ledge catch after a jump up (`sim.rs`, `ledge_catch`) is [game], but the arena never
  gives a wall "extra above the top", so only its test reaches it. [stand-in: the arena]
- The wall jumps' clips move him by their root in all three axes, gravity off until the jump
  mode starts; `Wall_JumpOff` turns him with its root (124 degrees), which is what makes its
  push of 20 leave the wall. [assumed]
- The jump mode after a wall jump is always the climb type (height `climbJumpOffHeight`),
  as in the original; `climbJumpUpHeight` is read but unused there too. [game]
- Root motion blends across a change of clip linearly over the new clip's blend time, as the
  actions do. In run 1 the vertical speed after a change fell a little more slowly. [assumed]
- The pad rumble is in since 2026-10-04 (step 10 above).
- Checks: `cargo test -p tf2-core climb` (frame layout, the test's limits, arrival speed,
  hand change, pull-up point, shimmy and climb speeds, side limit, letting go at the
  bottom, the pull back), `cargo run -p tf2-core --example climb_check` (the real graph and
  clips: grabbed at 9.40 from a running jump, ClimbUp 7.27 a cycle, pull-up starting at 36.11
  against 36.13 in run 1, 0.99 s against 0.94, ending on top), captures in `work\shots\climb_*`.
- The other robots' hands against the wall (2026-10-20, the user's screenshots of Ratchet and Long
  Haul climbing): the stand-off is each robot's own radius [game, owner +0x170], and the game has
  no hand IK for climbing (only FootIK and SpineIK), so a hand reaches the wall only if its clip
  puts it there. `core\examples\climb_reach.rs` prints each ground robot's radius and the
  farthest forward place of each hand and foot over the ClimbSet's Idle / ClimbUp / Shimmy clips.
  Long Haul: radius 4.0, hands at most 3.3 (about 0.7 m short); Megatron: radius 3.4, hands
  2.0-2.8. Ratchet's raised arm is his clip's (capture `work\shots\os_ratchet_climb_2.0.png`).
  The port does what the game does as read; whether the original shows the same gap is OPEN,
  settles: Long Haul or Megatron seen climbing in the original (screenshot or recording).

## Not read

- The name behind the clip hash 685a280c (a clip the shipped data does not have).
- The integer rule variables' getter `FUN_0070cae0` (`I_IN_AIR`, `I_ON_GROUND`).
- Where a level's ledge mesh gives a top cell its "extra above the top".
- Preset 8 on the climb's enter while carrying something (carrying is not built).
