# Optimus blue ultimate mesh, 2026-10-15

The user identified the all-over blue shine in the render captures: it should appear only during
Optimus's ultimate. The renderer drew every RenderThing in his character Object, including the
normal body and a second blue SFX body at the same time.

## Data

characters/optimus.str's main Object contains an exclusive RenderSetEnabler with two sets:
NormalMesh (1b44a61f), first, 45 node links; SpecialAttackMesh (372917bd), second, 45 links.
The normal links match the local names of his regular model parts; the special links match the
separate *_SFX model parts. These use glass_plus materials tinted (0.502,0.698,1), giving the
blue reflective sheen. EyeGlow_L/R are outside both sets and must remain drawn.
The read-only diagnostic is core/examples/optimus_surface_audit.rs, output work/optimus_surface_audit.txt.
The vehicle Object has the same two named sets, with 50 links each.

## Original behavior

- 00add210 registers RenderSetEnabler's factory 008031a0.
- 008031a0 allocates the component, calls constructor 00802f10 and loader 00802cf0.
- 00802f10 initializes exclusive true and selected index 0, and installs listeners 00803530/40.
- 00802cf0 decodes exclusive (default true), set name, and node links into per-set tables.
- 00803150 initialization hides sets 1..N and enables set 0 when exclusive is true.
- 00803530 routes messages to 008031e0. A matching named Trigger hides other sets and enables
  the named set. Nonexclusive groups instead enable/disable only the addressed set.
- 00803540 routes reactivation to 008032d0, which restores the stored exclusive selection.
- 00803360 resolves named render nodes via 00803440, caches them, and applies enable flags
  through 00587ce0 (low two bits at render+0x54; declaration arguments are in registers).
- 0072afd0/0072b980 already read in docs/special-types.md: Optimus/OptimusFlight start sends
  SpecialAttackMesh; end sends NormalMesh. The shield and the mesh lifetime use the same active timer.

## Port

The scene decoder retains RenderSetEnabler groups. Character model spawn applies the initial
exclusive set and retains per-part membership. The player's live Optimus/OptimusFlight special
selects SpecialAttackMesh, returning to NormalMesh when inactive. Both robot and vehicle model
roots are handled; parts use inherited visibility so a hidden form stays hidden. Ungrouped parts,
including the eye lights, are preserved. Other named render-set triggers remain outside this adapter.

[assumed] Named-node links resolve to CRC-32 of the mesh's local name after its object prefix.
All 90 robot and 100 vehicle links match. The original render-node alias registration would settle this mapping
for other objects. This is also logged in the unpublished notes. No shader recolouring or lighting change
is used to suppress the blue mesh. The separate OptimusSpecial particle effect remains unported.

## Validation

`cargo test --manifest-path Cargo.toml -p tf2-core --test optimus_render_sets` passes:
both 45-node robot sets, both 50-node vehicle sets, every matched part, the exclusive visibility
swap, and the ungrouped eye lights. Log: work/optimus-render-tests.log.
The bumblebee build passes; log work/optimus-glow-build.log.

Captured on US_Test_FreeRoam_A with --no-sound --no-hud, read back visually:

- work/shots/optimus_glow_idle.png, --after 4: normal red/blue paint and silver metal; no blue SFX body.
- work/shots/optimus_glow_active.png, --special-at 1 --after 4: the blue special meshes are visible.
- work/shots/optimus_glow_expired.png, --special-at 1 --after 12: normal meshes restored after the
  ten-second ability expires. Pose differs from the idle capture; no exact pixel comparison.
- work/shots/optimus_glow_vehicle_idle.png, --vehicle --after 4: normal truck appearance, with
  no blue special body and no robot meshes leaking through the hidden form.

Matching logs contain no shader/naga/pipeline errors. These verify the requested gating in the port;
there is no recording of the original Optimus ultimate for a full visual comparison.
