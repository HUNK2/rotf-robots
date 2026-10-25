# Clip selection (the animation rules): research notes

Addresses are for the analysed `Transformers2.exe` and stay in these notes.

Status (2026-10-02): the data model and the two rule evaluators are read, and the rule
variables are named. The controller that calls them each update (which layer, when a set
change is asked for, how blends start) is located but NOT read.

## The data (already decoded; `python work\animset_dump.py <pack.str> [set]` prints it)

- A rule (`animRule*` objects): `rule` (the variable, by name hash), `logic`, and one value
  (`floatValue`, `crcValue` or `intValue`, whichever is not zero). In memory 12 bytes:
  variable, logic, value (`FUN_0070b250`).
- A rule list (`AnimGenericRuleList`): several rules, all of which must hold.
- A set has `animIncomingSetRules.incRules`: entries `{toAnimSet, toAnimID, incRule:
  {fromAnimSet, fromAnimID, rules or ruleList}}`. They choose the clip a set starts on.
- Each `animRef` (a clip in a set) has `animExitRules.exitRules`: entries `{toAnimSet,
  toAnimID, exitRule: {finishAnim, animBranchPoint, rules or ruleList}}`. They say when the
  clip hands over, and to which clip of which set.
- An `animRef` also carries `xfadeTime`, `xfadeTimeOnExit`, `xfadeTimeOnExitToAnimID`, `loop`,
  `animSpeed`, `chance`, `minWalkSpd`/`maxWalkSpd`, `minStrafeAng`/`maxStrafeAng`, `walkFlags`,
  `group_type`. How the last six pick between clips is NOT read (`FUN_0070acc0` is the likely
  place).
- The text dumps print some yes/no fields as `FULL_BODY_ANIM_CHANNEL` (no) and
  `WEAPON_ANIM_CHANNEL` (yes): an enum name was attached to plain 0 and 1.

## Comparisons (`logic`)

0 equal, 1 not equal, 2 greater, 3 greater or equal, 4 less, 5 less or equal
(`FUN_00710650` floats, `FUN_00710560` hashes, `FUN_007105e0` integers). So
`animRuleMovement` is "left stick 0.01 or more", `animRuleMovementRun` "0.7 or more",
`animRuleNoMovement` "under 0.01".

## Variables

Each has a type (0 float, 1 integer, 2 hash) and is worked out when a rule first asks for it.

Floats (`FUN_0070c380`):

| name | value |
|---|---|
| `F_L_STICK_AMP` | length of the move stick vector (0 while a character flag is set) |
| `F_R_STICK_AMP` | length of the second vector (`FUN_007112b0`) |
| `F_L_STICK_DIR` | degrees between the move direction and the aim direction |
| `F_R_STICK_DIR` | the same for the second vector (reversed in one flying case) |
| `F_STRAFE_ANG` | always 0 here |
| `F_Z_VEL` | vertical velocity |
| `F_FALL_DUR` | owner `+0x3f4`: time falling (counts while moving down with no jump asked) |
| `F_TIME_IN_ANIM` | the current clip's time in seconds |
| `F_TIME_SINCE_DAMAGED` | now minus the time stored at character `+0x1f0` |
| `F_HP_PERCENT` | health over maximum |
| `F_STUN_TIME` | from the object at character `+0x234` |
| `F_RANDOM` | a fresh random number from 0 to 100 |

Hashes (`FUN_0070c900`): `C_JUMP_TYPE` (set by JumpMode, see `bumblebee-mechanics.md`),
`C_VEHICLE_STUNT`, `C_INTERACT_TYPE`, `C_CHAR_MESSAGE`, `C_DAMAGE_TYPE`, and others set by
code (`C_WEAPON_ACTION`, `C_WEAPON_EQUIP`, `C_INTERACT_ACTION_TYPE`). The integer getter is
`FUN_0070cae0` (not read).

Integers (`FUN_0070cae0`, read again 2026-10-20) and what the rest of this section left out:

| name | value |
|---|---|
| `F_STUN_TIME` | character `+0x234` object `+0x54`: the stun clock `0072a550` counts down (0 with no object). StunSet leaves on `< 0.5`. Port: `damage_timers.stunned` |
| `I_IS_HOVERING` | the getter says 0; a written variable overrides a getter, and HoverMode's enter (`008713c0`) writes 1, its exit (`00871580`) 0. So: 1 while HoverMode runs. Port: `Mode::Hover` or `State::hover_weapon` |
| `I_WEAPON_INDEX` | 0 with no weapon in hand, else `FUN_007b51b0`: the manager slot of the weapon in hand, or of its child when the weapon's alternating turn counter (`+0x39c`) is not 0 (the child fires). Port: `gun::Hand::index`, `Gun::value` |
| `I_SPECIAL_ON` | character `+0x234` object `+0x8c` (the special timer) above 0 |
| `I_IS_STRAFING` | getter 0; StrafeMode writes it |
| `I_ATTK_BRANCH` | getter 0 (no roster rule reads it) |
| `I_ATTK_BTN_HELD` | the input object's slot `+8` called with 1 (no roster rule reads it) |
| `I_PREV_CHAR_FORM` / `I_CHAR_FORM` | character `+0x320` / `+0x31c` |
| `I_IN_AIR` / `I_ON_GROUND` | the controller's ground byte (`+0x104` -> `+0x26c`), negated / as is |
| `C_VEHICLE_STUNT` (`0070c900`) | default 0; FlyMode's enter and exit write 0; no code writes a stunt kind and the six `VEHICLE_STUNT_*` hashes are not in the executable, so the jets' `Vehicle_Idle` exits on them never hold (the stunts go through the `stateFlyStunt*` states). Port: `Hash(0)` |

A variable that was never written is worked out by its getter when asked; one that was written
(`anim_variable_set` `0084e8d0` or the direct record writes in the mode enters) keeps what was written (the lookup `007106f0` and the order were not read: [assumed] from
`I_IS_STRAFING`, whose getter also says 0 and which StrafeMode writes, and which the port's
recorded strafing agrees with).
An unanswered variable in the PORT compares as float 0 (`animrules::holds`): for a hash or integer
rule that wrongly holds when the rule's own float is 0, so every rule variable the data reads should
be answered (`animgraph_audit` lists the gaps).

## Choosing the first clip of a set (`FUN_0070acc0`, `FUN_0070b710`)

Corrected 2026-10-03 (`weapon-mode.md` has the controller): an entry's filters are the set
and clip it comes FROM (0 fits anything), compared with what the layer is playing; its
`toAnimSet` and `toAnimID` are the result, not filters. `FUN_0070acc0` is the base layer's
walk; `FUN_0070b710` the other layers', with a second pair of filters for what the base
layer plays. Walk the set's entries in stored order; the first whose filters fit and whose
rules all hold gives the set and clip to start. Nothing fits: no change.

`F_STRAFE_ANG` is not "always 0": StrafeMode writes it (and `I_IS_STRAFING`).

## Leaving a clip (`FUN_0070d540`)

Walk the playing clip's exit entries in stored order. An entry is allowed when:

- `finishAnim` is set: the clip has finished;
- otherwise: the layer's branch count (animation player `+0xdc`: 1 as a clip starts, one
  more for each `AnimBranchPointEvent` passed; a finished clip counts as 100) is at least
  `animBranchPoint`, so 0 means "at any time" and 2 "once the clip's branch point has
  passed". It is the same count the state machine's connections test. (Corrected
  2026-10-03: this said "progress in percent". The field is stored as an enum of 0 or 2;
  only the eight landings of the weapon set that run on have 2, with their branch point
  at tick 20 of 26 or 24 of 30.)

and every one of its rules holds. The first allowed entry gives the set and clip to go to.

## Where the rest is

Loaders: `FUN_0070af50` (incoming rules), `FUN_0070b340` (rule lists), `FUN_0070ccd0` (an
`animRef`), `FUN_0070d380` (exit rules). The controller that runs the layers and calls the two
evaluators is among `FUN_00706450`, `FUN_007079f0`, `FUN_007088f0`, `FUN_00709100`,
`FUN_00709870` (not read). What the test's hand-written machine should be replaced with is
therefore: the data's own entries, evaluated as above, once the controller's order of
operations is read.

2026-10-09 correction: `00706450` in that historical candidate list writes
custom input bindings; it is not the animation controller. See `docs/input.md`
and its bounded read in the function ledger.

## In the port (2026-10-04, `core\src\animrules.rs` `Base`, `sim.rs`, `game\src\animation.rs`)

The base layer now runs on the sets' own rules for the sets a control state starts without
naming a clip: `IdleStandSet`, `WalkSet`, `JumpSet`, `FallSet` (and what leads into them:
the landings, `Run2Idle`). `animrules::Base` is the controller's base layer: `enter(set, from)`
is `FUN_0084ddb0` with no clip named (entry walk from the set and clip playing; a result with
no clip walks that set's entries in turn), `update` is `FUN_0084e3c0`'s walk of the playing
clip's exit rules (`finishAnim`, branch count, rules), `advance` plays the clip at its own
speed. A clip starts with its own `xfadeTime`, or the old clip's `xfadeTimeOnExit` where that
names it (so a landing runs on into the run with no blend, and the run after the change from
the car also starts with none: `Trans_V2R_*` ask 0 to `Run`). The state enters hook in as the
notes say: Idle (set, then mode), Move (set, then mode), Jump (mode, then set), Fall (set,
then mode); anything else clears it. The variables the four sets read are registered in
`sim::rule_value` (`C_JUMP_TYPE` is written by the jump mode and outlives it; the landing
into a run is the big one at `F_FALL_DUR` 1, or 0.75 after a long jump, which the hand
chooser did not know). The picture takes the clip and tick from `State::base_clip`; the
movement's own pace layers (`step_pace`, matched to run 1) are unchanged and
`examples\base_check` and a test show the two lead with the same clip on 845 updates of
walking, running, stopping, jumping and falling. The dodge's direction is now `DashSet`'s
entry rules (front from -45 up to 45, left from 45 up to 135, right from -135 up to -45, back
beyond); the old thresholds took +45 for the front and -135 for the back. The other sets
(`DriveSet`, `Vehicle2RobotSet`, `ClimbSet`, `GroundPunchSet`) have no exit rules, so their
clips are the control states' own, as before. Death, hit reactions and fidget were added
2026-10-06 below. Carrying remains outside this implementation.

## Moving blends and animation clocks (2026-10-06)

The original retains animation players during a fade (`FUN_00748f00`,
`FUN_0053d600`), including an unfinished earlier blend. `core::animblend::Blend` now
keeps that moving tree; character and gun/axe renderers sample every player instead of
fading from a frozen pose. Character clips fall back to 0.1s for a negative fade;
object clips (`FUN_007c3870`, through `007c37e0`/`007c3830`) use their own time directly,
replacing immediately at <=0.001s. Outgoing character cues run only when
`allowEventsOnBlendFromThisAnim` permits them.

`FUN_0053cc40` increments elapsed ticks (`+0xe0`) without wrapping loops; the next
loop boundary advances instead. `FUN_0053a2b0` returns elapsed time. Nonloop clips
clamp sample time and accumulate overrun at `+0xe4`; `FUN_0053a2c0` returns overrun.
`continueTimeOnExit` in `FUN_00748f00` carries that into the next clip; keeping the
previous frame carries elapsed time instead. `animrules::Base` now keeps sample phase,
elapsed time and overrun separately. Loop rule timers can finally reach eight seconds.
The weapon set's separate `sim::RootLayer` players now keep the same two clocks;
`strafe_start` carries landing overrun or the unwrapped previous frame into the new
player. This is tested through the install's actual weapon landing-to-run rules.

Character constructor `FUN_00713300` enables special idle (flags `+0x150`, bit4) and
sets `+0x198` to 8 (`00cad478`). Update `FUN_0071dc20` decrements it until negative;
getter `FUN_0070cae0` then exposes `I_SPECIAL_IDLE_ON`. Movement does not reset this
lifetime timer. IdleStandSet also checks idle duration and time since damage. These
variables now drive Fidget and its return through the original rules.

## Death and hit reaction layers (2026-10-06)

StateDeath enter/update/exit/done: `0087b070`, `0087af40`, `0087b510`, `0087b610`.
Enter starts BasicMotionMode and DeathSet. Entries prioritize control form3 (car)
Death_V2R, then headshot kind `4782c9ce`, explosion `f7128ccc`, then Death. Exits select
Death_Explode_Fall in the air and Death_Explode_HitGround at ground height <=0.01.
These entries/exits now run. Update2026-10-07: original restart clocks (3s
grounded/finished or10s) now request NPC removal/player reset. Explosion deaths use
XYZ roots. Arena reset replaces a mission reload; campaign exclusions remain open.
See `docs/damage.md` for the receiving path and exclusions.

StateHitReact enter/reset, exit, may-leave, done and update: `0087cd10` (wrappers
`0087ccd0`/`0087cd00`), `0087d190`, `0087d2b0`, `0087d400`, `0087d510`. It starts
HitReactionSet, sets robot form, faces against hit velocity and starts a four-second
reaction cooldown. Fast/slow flails choose KnockBackMode (`00875100`, `00874f20`,
`00874f30`); its grounded switch at height <=0.01 returns XY control to BasicMotionMode
(raw `00875088..008750c9`). Supplied velocity drives flight; recovery roots drive
grounded recovery. Rules choose Recover_Fast/Slow, then IdleStandSet or FallSet.
A completed ordinary state with no transition requests owner `+0x318` (`00877d90`,
raw `00878020..00878056`). Setup `00850a70` binds that default to stateIdle
(`00716d8b..00716d9b`), now releasing the full hit control state. May-leave permits a
flyback to leave after a 2.5m drop; a basic reaction refuses Fall until its clip
finishes. Both are ported/tested. Update2026-10-07: wall-splat predicates and
Recover_WallSplat are ported; contact classification uses arena estimates.

Partial entries carry `currAnimBaseSet`/`currAnimBaseID`, previously discarded.
HitReactPartialSet rejects form3 and CombatSet (`c697c917`), otherwise plays the
20-tick additive HitFront on layer2, then emptyAnimSet. Handler `0084dfa0` starts it
immediately; forced-state handler `00850c80` checks `canGoToStates` before requesting
full state or overlay. The overlay continues weapon firing (`0087d510`) and completes
when its clip finishes (`0087d400`); ordinary exit clears overlays (`00878310`).
These paths are ported: a partial hit keeps movement and gun recoil.

`0072ddf0` selects animation kinds from damage flags (strong flail, fast flail,
knockback, crash, partial, melee in priority order). Receiving dispatch `00729120`
uses full reactions for mask `0x170100`, with cooldown suppressing repeats except
flails; `0072de70` selects death kinds. `receive_hit` accepts already resolved damage
and impulse. Update2026-10-07: shared receiver scaling, gates, status/stun and installed
scout combat are implemented (`docs/damage.md`); original enemy AI remains a debt. Other
timer/helper sections read: `0072a550`, `0072e640`, `0072e800`, `00711d90`, `008781a0`,
`0084e8d0`, `0070c380`, `0070c900`. No multiplayer behavior is claimed.

Tests cover moving/interrupted fades and cue flags, partial filters, overrun, fidget,
firing through a partial hit, fast/slow recovery and release, airborne and car death
landing, the hit state's fall permission and weapon landing overrun. All 221 core tests
pass; `base_check` still reports zero differences on 845 updates, and `control_check`
replays melee/weapon sequences. Checked against install data/code, not an original
fighting recording. Captures inspected: see status item3.

## In the port: the graph audit of the clips the control states start (2026-10-20)

How `animation_set_start` (`0084ddb0`) treats a start: a set is always given; with a clip named
(`thunk 01193520`, a lookup in THAT set's list: `0117db90` finds the set, `01193520` the clip in it) the
clip plays and nothing else is walked; with no clip the set's entry rules pick (`0070acc0` base layer,
`0070b710` others) and a result naming a set but no clip walks that set in turn. A clip named in a
set that lacks it starts nothing. Every playing clip, however it started, has its animRef's exit
rules walked each update (`0084e3c0` -> `0070d540`).

Port, one controller per started thing:
- on foot: `animrules::Base` (`base_enter`/`base_step`) for Idle, Walk, Jump, Fall, Death, HitReaction,
  Stun, FlySet's hub; the weapon set's `strafe_entry`; the partial layers `hit_partial`, `fly_partial`.
- clips started BY NAME (`start_clip`, the action player): `sim::action_exits` walks their exit rules
  after the state machine each update (`action_rule_clip` picks the animRef by the state's set, the
  fly states' constant `FlySet`, the drive states' `DriveSet`, else the set the name was found in);
  `special_chain` keeps `SpecialAnimSet` and the car model's clips (before the machine).
- `climb::catch_clip` walks `ClimbSet`'s entry rules.

Findings (`core\examples\state_exits.rs`, `set_exits.rs` print them; the roster test
`the_rosters_named_clips_with_exit_rules_are_the_jets_stunts_and_the_specials` keeps them):
- Named-start clips with exit rules, 13 packs: only Starscream's and Firestorm's four stunts
  (finished -> `FlySet/Vehicle_Idle`). Jetfire's pack has no stunt clips. Nothing in CombatSet, DashSet,
  GroundPunchSet, ClimbSet, DriveSet or Vehicle2RobotSet has exit rules. Specials (Megatron, MegatronFlight
  `SpecialAnimSet`, 8 clips) were already `special_chain`'s; their exits into `IdleStandSet`, `DriveSet`
  and `HoverSet` (clip 0) set no clip in the action player: the machine ends the state.
- Hover: `stateHoverEnter` = `Vehicle2RobotSet/Trans_V2R_Hover` (no exits, blend 0), `stateHover` =
  `HoverSet/Hover_Idle` (explicit). `HoverSet/Trans_V2R_Hover` is a second animRef (same 60 / 20 ticks,
  blend 0.1, finished -> `Hover_Idle`), reached only by HoverSet's entry walk.
- Robot flyers (OptimusFlight, MegatronFlight): no `StateDrive`; the hub walks FlySet's entry rules
  (`Trans_RobotRun2Vehicle` with `F_L_STICK_AMP` >= 0.01, else `Trans_Robot2Vehicle`, then finished ->
  `Fly_Idle`); `DriveSet` has the same two entries and is never started.
- `V2RPartialSet` (MegatronFlight only): FlyMode enter/exit start it when attributes `+0x390`
  (`specialAbilityVehicleWeaponFireFromAnim`, `00726710`) is set; MegatronFlight's is false, so it is dead
  data; Blackout's is true and lacks the set (the lookup fails).
- Not walked, by decision: InteractSet, CSAInteractSet, PlayAnimSet, GrabSet, FaceSet, WarRoomSet.
