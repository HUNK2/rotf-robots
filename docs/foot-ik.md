# Foot IK (2026-10-07)

## Original, checked statically

`00ad7a10` registers FootIK; factory thunk `0073f1b0` jumps to loader
`0073f000`. The loader resolves three `links` against hierarchy name CRCs and
reads `side`. Constructor `0073d8d0` stores the chain at +110/+114/+118, side
at +138, enables +128 and registers type 818ad2d7. Vtable b5d7ac +8 is
`0073db50`. `00740d60` updates the global IK list for nonzero dt and owner
flags bits 3/5 clear. FootStep is a separate contact/event component.

`0073db50`: probes at animated endpoint z +/-1.5; hit z gets +0.03;
vertical correction +11c approaches the contact correction halfway each
update. Lowering requires enabled +128, composed matrix component local_428
>0.95 and world endpoint
travel <=dt*0.3+0.001; positive corrections can apply while moving. Two-bone
solve uses the cosine law and caps distance to segment sum minus 0.001.
Foot angle +124 approaches its target by dt*24. Probe callback/filter, exact
bone axes, negative correction timing and state enable writes are unresolved.
Narrow disassembly at 0073dfc1..0073e058 and 0073e09e..0073e1bc confirms
the motion threshold, hit bias and halfway filter.

Bumblebee's main object (58 nodes) has SpineIK and two
FootSteps but **no FootIK attachment**; no FootIK appears in the decoded
export/data text files. Thus the generic original solver is evidence, but
its use on this character is not established. No original slope recording.

## Implementation assumptions / remaining fidelity debt

- [assumed] Bind Foot_L/R and their two ancestors as ankle/knee/hip, and
  shift Main for pelvis compensation; hierarchy is data, original FootIK
  links are absent. Explicit attachments or an original runtime pose would settle it.
- [assumed] A rectangular sole from the rest-space bounds of the rigid foot
  and midfoot meshes represents foot support. Mesh coordinates are data;
  centre plus corner probes select the strongest support plane, with clearance
  checked against every sampled plane to avoid toe penetration at joins.
  The proxy/probe positions are our adaptation. Original endpoint/probe binding
  and comparison footage would settle it.
- [assumed] Correct only grounded Idle/Move/Strafe, excluding transformations,
  forced clips and death. Preserve animated foot lift; adapt both feet to the
  supporting plane, including bank roll, and lower the visual body to avoid
  overextension. This is broader than the original stationary lowering gate.
  Original state writes and a walking-on-slopes recording would settle it.
- [stand-in] Synchronous vertical probes use the authored arena's boxes,
  ramp planes and ground; original async collision geometry is not available.
- [assumed] Smooth height by the game's halfway rule at fixed 0.032s steps,
  orientation by dt*24 with the factor capped at one, reset on inactive state
  or teleport/lost probe, and preserve each animated knee's bend plane. The original
  reset/axis semantics and render/update cadence need comparison.

## Implementation / validation

`core/src/footik.rs` loads the foot proxy from the user's meshes and implements
the pose solver; `game/src/assets.rs` retains the binding and
`game/src/animation.rs` applies it after weapon, hit and spine layers. The
cached crossfade pose stays uncorrected, so correction never feeds back into
the next animation. Movement/contact/camera heights are unchanged. Foot and
toe child transforms propagate through both rigid meshes and skin joints.

Eight tests use the installed rig/meshes: real feet against slopes/banks at
three facing angles, independent kerb support, constant segment lengths,
unreachable/folded targets, preserved Walk swing heights and toe keys,
inactive/missing support, teleport/reset, a raised toe corner at a ramp join,
flat ground/contact bias and render cadence. All 263 core tests passed;
final full workspace build succeeded.
Inspected final captures `work/shots/footik_orange_final_20261007.png`
(feet across ramp/ground join), `footik_walk_final_20261007.png` (walking
uphill) and `footik_bank_final_20261007.png` (bank stance). Earlier
`footik_*_20261007.png` captures are also retained. The normal standing/walking
poses remain recognisable and support adapts without stretched legs.
Original comparison and the fidelity debts above remain open.

## Any number of limbs; Devastator's arms and legs (2026-10-22)

The user asked for IK "in his arms and legs" on Devastator. `FootRig::legs` is now a `Vec<Leg>`
and `FootIk` keeps one offset and turn per limb; `FootRig::probe` carries the probe's half
extent (`PROBE`, 1.5 m [game], for every robot read by `FootRig::read`), and the teleport test is
twice it. Every robot's behaviour is unchanged (the eight tests above pass as they were).

- `Leg::planted` [assumed]: a limb whose sole is measured in a pose it stands in, not the bind
  pose: the rigid mesh points of the end node and every node under it within `reach` of their
  lowest, as a rectangle at that height, in the end node's space.
- `giant::limbs` [assumed] gives Devastator four: Thigh/Calf/Foot and Arm/Forearm/Hand on each
  side, measured in `IdleStandSet/Idle` tick 0 with a 1 m reach (his hands stand on their
  fingertips), the probe scaled by his height over 6 m (15.2 m). `game\src\animation.rs` uses his
  rig in place of `FootRig::read`'s and also runs it in `Mode::Action` (his turns, pound and
  vortex). The game has none on him: his pack holds no FootIK object (`docs/devastator.md`).
- Test `core\tests\devastator.rs` `his_arms_and_legs_are_planted_on_the_ground`: flat ground
  lifts each limb the 0.03 m bias only; a 2 m block under his right hand lifts that hand 2.03 m
  and leaves the other three, with no segment length changed.
