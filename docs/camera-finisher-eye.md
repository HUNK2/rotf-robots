# Finisher camera entry: the eye, from the disassembly (009186f0)

Read-only research, 2026-10-20. Source: disassembly of 009186f0 (size 0x8aa, 0x9186f0..0x918f9a)
and its callees, read with `work\raw_disasm.py`; pseudocode only to name things. Every address
below is an instruction address in the disassembly. The earlier note `docs/camera-finisher.md`
(sections 5.5 to 5.9 and 13) is SUPERSEDED where this file says so; that note was not edited.

Tags: [game] read in the disassembly; [assumed] not checked; [unsettled] not settled here.

## 0. Summary of what changed

| Claim in camera-finisher.md / the port | Disassembly says |
|---|---|
| t (lateral distance) = clamp(F.z - 1, 1, D) | t = min(max(Y - 1, 1), D), where Y = free length of the chosen side (a ray), NOT F.z. In open ground t = D = 20.98. |
| "heights equal" test on the two side eyes | The test compares the two side RAY LENGTHS (ray1, ray2), not heights. |
| c4 = P.z - 24 (decompile, xmm0 assumed kept) | c4 = ray3 result - 2, a DISTANCE from P down to the floor end (0x918a97, 0x918a9c). The ray helper returns a float in xmm0, so the decompile's value was wrong. |
| one random r: E = P + r*dir | TWO random draws. First: vertical offset of P (P'.z = P.z + rnd1). Second: along dir (E = P' + rnd2*dir). |
| 00551d60 returns in st0 | It returns in xmm0 (0x551dc3, ret 8). |
| 00466960 is assumed atan | Confirmed: `_CIatan` (0x46696c). 00466980 is `_CItan`. |
| 00917dc0 stores nothing and has no visible effect | It RETURNS a float in xmm0: the nearest flagged hit distance, else the segment length. Its value is used. |
| hw = FOV/2 = 25 (decompile line 7: local_94 = fVar4 * 0.5) | NOT settled. The disassembly multiplies by 0.5 the xmm0 that camera_two_character_frame (009180a0) returns (0x918723). 009180a0 does not return 50. See section 6. |

## 1. Frame and inputs at entry

Q = the stack frame of 009186f0 after `sub esp,0xc8` (0x9186f6). The offsets below are the
frame addresses the code uses, after accounting for the pushes (d = bytes pushed at that point).

- 0x9186fc: xmm0 = 50.0. 0x918719: `[esi+0x28] = 50.0` (behaviour FOV). [game]
- 0x91871e: call 009180a0 (callee cleans 12 bytes), outputs:
  - F, the focus, at Q+0x00 (x), Q+0x04 (y), Q+0x08 (z). [game]
  - dir, the unit direction him -> it, at Q+0x1C, Q+0x20, Q+0x24. [game]
  - Zmax, the higher top, at Q+0x38. [game] (`*param_3 = max(top)`, 0x918600-0x918617)
  - The RETURN VALUE is xmm0 (see section 6). The caller reads it at 0x918723.
- 0x918723: Q+0x3C = 0.5 * xmm0 =: hw. [game] (value: section 6)
- 0x918736: angle = FOV * (pi/180) (0.0174533). 0x918744: tan = 00466980 (`_CItan`). Result Q+0x50. [game]

## 2. Dmax (D) and the tangent

- 0x918753: D0 = hw / tan(50 deg). 0x918764 to 0x918771: Dmax = max(D0, 15.0), stored at Q+0x48. [game]
- With hw = 25 this is 20.98 m. (Depends on hw; section 6.)

## 3. The side unit S

- 0x918777 to 0x9187a9: hlen = |(dir.x, dir.y)| via 00409a70 (sqrt, st0). [game]
- 0x9187ea: `jbe` skips the normalisation when hlen <= 1e-8. Otherwise S = (dir.y, -dir.x, 0)/hlen.
  Stored at Q+0x10 (x), Q+0x14 (y), Q+0x18 (z = 0). [game]
- If hlen <= 1e-8, S is left UN-normalised (dir.y, -dir.x, 0). The port uses Vec3::X there. [game] (difference)

## 4. The two side rays and the choice of side

- 0x918816 to 0x91882a and 0x918833 to 0x918851: E1 = F + S*(Dmax+1), stored Q+0x8C..0x94. [game]
- 0x918876: r1 = ray(from F, to E1) (call 00917dc0, section 7). Result xmm0 -> Q+0x40 (0x91888d). [game]
- 0x9188a5 to 0x9188e7: E2 = F - S*(Dmax+1), stored Q+0x74..0x7C. 0x9188e7: r2 = ray(from F, to E2). [game]
- 0x9188f7: Q+0x0C = r2. 0x9188ec to 0x918902: |r1 - r2| via 00409a50 (fabs). [game]
- 0x918907 to 0x918914: `fcompi` against 0.01. If |r1 - r2| >= 0.01 the jump goes to 0x91896e (no draw).
  Otherwise the code falls through to the COIN at 0x918916. [game]

**Heights differ (|r1 - r2| >= 0.01), 0x91896e to 0x918982:**
  - if r2 > r1 (0x918977 to 0x91897d): flip, S := -S, Y := r2 (0x91894c path). [game]
  - else: keep S, Y := r1 (0x91898a path). [game]

**Coin (|r1 - r2| < 0.01), 0x918916 to 0x91893e:**
  - LCG advance: state := state*0x343FD + 0x269EC3 (0x918916 to 0x91892a), then `_time64` (0x918930).
  - al = low byte of time64 XOR byte 2 of the advanced state (0x918935). `test al,1` (0x91893e).
  - bit 0 clear (0x918984): keep S, Y := r1 (`movss xmm3,[esp+0x48]` = Q+0x40). [game]
  - bit 0 set (0x918946): flip, S := -S, Y := r2 (Q+0x0C). [game]

So the chosen side is the one with the longer clear ray. Y is its free length.

## 5. Lateral point P, the floor ray c4, and the eye

**5.1 Lateral distance** (0x91899c to 0x9189bb): t = min(max(Y - 1, 1), Dmax). [game]
  So t is about 20.98 m in open ground (Y = Dmax+1 = 21.98), and shorter when the side is blocked
  (t = blocking distance - 1, at least 1). F.z does NOT enter t.

**5.2 P** (0x9189c1 to 0x9189f0): P = F + S_chosen * t, stored Q+0x28..0x30 (P.z = F.z since S.z = 0). [game]

**5.3 Handles** (0x9189f6 to 0x918a03): refhandle copies of [esi+0x18] (H) to Q+0x40 and [esi+0x20] (T) to
  Q+0x48. These OVERWRITE r1 and Dmax; both are no longer needed. [game]

**5.4 The floor depth Bfloor** (0x918a08 to 0x918a85):
  - minB = min(H.bottom, T.bottom), where bottom = [object+0x3c]. A null handle gives -10000. (0x918a08 to 0x918a2f) [game]
  - Bfloor = max(minB - 1, P.z - 22). P.z - 22 is `(P.z - 20) - 2`: the -22 is a DEPTH CAP, not c4. (0x918a37 to 0x918a85; stored Q+0x18.) [game]
  - Both handles null gives Bfloor = P.z - 22.

**5.5 c4** (0x918a8f to 0x918aa4):
  - r3 = ray(from P, to (P.x, P.y, Bfloor)) (call at 0x918a97, result xmm0). [game]
  - c4 = r3 - 2.0 (0x918a9c), stored Q+0x0C (0x918aa4). [game]
  - So c4 is the distance from P down to the floor end, less 2 m. It is NOT P.z - 24 (the decompile's value; its xmm0 was assumed kept by the call).

**5.6 r4 (the ceiling ray)** (0x918aaa to 0x918aeb):
  - r4 = ray(from P, to (P.x, P.y, Zmax)) (call at 0x918ae6; the vertex uses Zmax from Q+0x38, loaded at 0x918ad4). [game]
  - r4 result stored to Q+0x38 (0x918aeb, overwriting Zmax). [game]

**5.7 The first draw, rnd1** (0x918afd to 0x918b0f): rnd1 = random_between(-c4, r4) (call at 0x918b0f).
  - Arguments: a = -c4 (xmm0 at 0x918afd), b = r4 (st0 from 0x918af1, stored [esp+4] at 0x918b06).
  - Result in xmm0 (section 8).

**5.8 P'** (0x918b14 to 0x918b64): P'.z = P.z + rnd1, written to Q+0x30 (0x918b2e, 0x918b64). P'.x, P'.y = P.x, P.y. [game]
  So the first draw moves the eye's base VERTICALLY, between 2 m above the floor (P.z - c4) and the ceiling end (P.z + r4).

**5.9 The back and forward rays** (0x918b37 to 0x918be7), with W = 2*hw and the half value 0.5*hw:
  - r5 = ray(from P', to P' - W*dir) (call at 0x918b81). Vertex V5 at Q+0x64..0x6C. [game]
  - r5 -> Q+0x0C (0x918ba0). [game]
  - r6 = ray(from P', to P' + 0.5*hw*dir) (call at 0x918be7). Vertex at Q+0x54..0x5C. [game]
  - r6 -> Q+0x38 (0x918bec). [game]

**5.10 The second draw, rnd2** (0x918bf6 to 0x918c10): rnd2 = random_between(-r5, r6) (call at 0x918c10). [game]

**5.11 The eye** (0x918c15 to 0x918c4e):
  E = P' + rnd2 * dir, written to the behaviour at +0x3c, +0x40, +0x44. [game]
  E.x = P.x + rnd2*dir.x (0x918c2f), E.y = P.y + rnd2*dir.y (0x918c15), E.z = P'.z + rnd2*dir.z (0x918c1b, 0x918c48).

**5.12 Look and FOV** (0x918c56 to 0x918cfb):
  - +0x30..0x38 = F (0x918c56 to 0x918c72). [game]
  - d = |F - E| (0x918c77 to 0x918cb8, sqrt via 00409a70, st0). [game]
  - 0x918cbd: st0 = hw / d. 0x918cc4 to 0x918cca: compare with tan(50). [game]
  - FOV update only when tan(50) < hw/d, i.e. d < D0 (0x918cce `jbe` skip). [game]
  - 0x918cd4: atan via 00466960 (`_CIatan`), then x 57.2958 (0xb6b464) = degrees. Stored at +0x28. [game]
  - 0x918ce9 to 0x918cfb: camera +0x94 = FOV, camera +0x128 = 0. [game]

## 6. The half width hw is NOT settled (the largest open item)

- The decompile (line 4 to 7 of 009186f0) reads hw = 50 * 0.5 = 25, because Ghidra assumes xmm0 is
  still 50 after the call to 009180a0. The disassembly shows the call returns in xmm0 (009180a0 ends
  with `movss xmm0,[esp+0x14]` and `ret 0xc` at 0x9186ec; the value at [esp+0x14] is written at 0x918682).
- 0x918674 to 0x918682 (009180a0): xmm0 = max(a - b, S), where S = sqrt(...) + two slot values
  (0x918653 fadd [esp+0x18]; 0x918663 fadd [esp+0x4c]). The sqrt argument is dx^2 + dy^2 of the two
  characters' centres (the decompile's last nav_sqrt call, 0x91864e).
- The slots are REUSED in that tail (handle references at [esp+0x4c] and [esp+0x54] earlier, floats
  later). The exact a, b and the two added terms were NOT settled, so R is not settled.
- Consequence if R is the pair's span (about |B - A| plus reach terms, as the form suggests): hw = R/2
  is a few metres, Dmax = max(hw/tan50, 15) is then 15 (not 20.98), the back ray W = 2*hw is short,
  and the FOV widening rarely fires. This would change everything in sections 2, 4, 5.5, 5.9 and 5.12.
  [unsettled] Next step: read 009180a0 0x918140 to 0x918682 with its own slot map, or call it
  offline (work\emu) with two synthetic handles; then fix hw in the port.

## 7. The ray helper 00917dc0 (settled as to its return)

- Args: from = arg1 (the first pointer), to = arg2 (esi at 0x917de5 = [E+8], ebx = [E+4]). Returns in xmm0, `ret 8` at 0x917fba and 0x918096. [game]
- It copies the two handles H and T into a list (0x917dd1, 0x917dde) and releases them at the end.
  Whether the query 005be890 excludes them is not read here. [unsettled] (the hit loop does not test them).
- Query: 005be890 with mask 0x20 (0x917e25). The hit list is 0x24 bytes per hit: distance at +0x1c, object at +0x20 (camera.md, not checked here). [assumed]
- Hit path (0x917e35 to 0x917f20): xmm0 = min over hits whose object has bit 5 of +0x9c; initial FLT_MAX. Returns the nearest flagged hit distance (0x917fad).
- No hit, or no flagged hit (0x917e2f `je`, 0x917f28 `jl`): returns |to - from| (0x917fc3 to 0x918002, sqrt via 00409a70). Returns in xmm0 at 0x918089.
- So: ray(from, to) = distance to the first flagged hit along the segment from `from`, else |to - from|.
  The port's `World::first_block(from, to) -> Option<f32>` (core\src\camera\spectator.rs) is exactly this with `None` -> length.

## 8. The random 00551d60 (settled)

- `random_range_with_clock(a, b)`: stdcall, `ret 8`. a = [E+4], b = [E+8]. [game]
- Step (0x551d63 to 0x551d73): state := state*0x343FD + 0x269EC3 (mod 2^32), stored at 0xC02EF0 BEFORE the clock. [game]
- `_time64` (0x551d7a). Its low 32 bits are in eax.
- Bits (0x551d7f to 0x551d9f): ecx = ((state >> 8) XOR (time_low << 8)) & 0x7FFF00 | 0x3F800000, so u = mantissa/2^23 in [0, 1), in steps of 2^-15 (the low 8 bits of the mantissa are 0).
  In bits: m_i = state_bit(16+i) XOR time_bit(i), for i = 0..14.
- u = float(ecx) - 1.0 (0x551daf). Result (0x551db7 to 0x551dc3): xmm0 = a + (b - a)*u. [game]
- RETURN IN xmm0, not st0 (the notes said st0; the caller 009186f0 reads xmm0 at 0x918b14).
- The stray `mov [esp+8], edx` at 0x551dbf is the high word of time64 written into a dead slot. [game]
- The state is the global 0xC02EF0, shared with other systems (camera.md). [game]

Port comparison (`core\src\camera\spectator.rs`, `Random`):
- `step`: `state*0x343fd + 0x269ec3` (wrapping). Same LCG, same order (advance, then the clock). MATCH.
- `between(low, high, seconds)`: `((s >> 8) ^ seconds << 8) & 0x7f_ff00 | 0x3f80_0000`, minus 1, then low + (high - low)*share. Operator precedence in Rust (`&` before `|`) gives the same bits as the asm. MATCH.
- `coin(seconds)`: `((step() >> 16) ^ seconds) & 1 != 0`. Game: bit 0 of (time_low XOR byte 2 of state), state advanced first. MATCH; coin true means flip (-S).
- Differences: the port's stream is its own (`[stand-in]`, not the game's global 0xC02EF0). And the port calls `coin` ALWAYS;
  the game calls it only when |r1 - r2| < 0.01 (section 4). Also the game calls time64 once per draw; the port passes one `seconds`.

## 9. 00466960 and 00466980 (settled)

- 00466960 = `_CIatan` wrapper: `fld [arg]; call _CIatan; ret` (0x466969 to 0x46697c). Single argument, returns st0. It is atan, not atan2. [game]
- 00466980 = `_CItan` wrapper (0x466989 to 0x46699c). Used for tan(50 deg) at 0x918744.
- Used in 009186f0 at 0x918cd4 only (plus 00466980 at 0x918744). The ledger line at functions.tsv 3766 (flight_contact_heading) uses 00466960 the same way.

## 10. Not settled

1. hw (= R/2, R = the xmm0 returned by 009180a0): section 6. This is the main item. All of D, W, 0.5*hw and the FOV test depend on it.
2. The exact expression of R in 009180a0 (0x918634 to 0x918682; slot reuse).
3. Whether the segment query 005be890 excludes the two characters' handles (section 7).
4. The hit record fields (distance at +0x1c, object at +0x20): taken from camera.md, not read here. [assumed]
5. The port's `Side` has no null handles. The game's null defaults (-10000 bottoms) are not in the port's Rust.
6. The game's `seconds` is per draw (time64 read at each draw); the port uses one value for the call.
7. hlen <= 1e-8: the game keeps S unnormalised; the port uses X. Not changed here.

## 11. Rust for Finisher::enter (not applied: this session edited no .rs file)

The current port (core\src\camera\finisher.rs, `Finisher::enter`) takes r = 0 (stand-in), uses
clamp(F.z - 1, 1, D) for t, and calls `random.coin` unconditionally. The version below follows
sections 2 to 9. `half_width` is the stand-in 25.0 until section 6 is settled. The ray is
`World::first_block` with the length fallback.

```rust
use glam::{Vec2, Vec3};

use super::spectator::{Random, World};

/// The ray helper 00917dc0 (section 7): the distance from `from` to the first blocking object on the
/// segment to `to`, else the segment's length. [game] returns xmm0.
fn ray(world: &dyn World, from: Vec3, to: Vec3) -> f32 {
    world.first_block(from, to).unwrap_or_else(|| (to - from).length())
}

impl Finisher {
    /// Its entry (slot 0, 009186f0). [game] except `half_width` [unsettled: section 6, stand-in 25.0].
    /// Side by the longer clear ray (or a coin when the two are within 0.01), lateral t = min(max(free - 1, 1), D),
    /// the floor ray c4 = r3 - 2, rnd1 lifts P (random between -c4 and r4), rnd2 slides the eye along dir
    /// (random between -r5 and r6), then the FOV widens to atan(hw / d) when d < hw / tan 50.
    pub fn enter(him: &Side, it: &Side, random: &mut Random, seconds: u32, world: &dyn World, half_width: f32) -> Self {
        let (focus, dir, top) = pair(him, it);
        let tan = FOV.to_radians().tan();
        let reach = (half_width / tan).max(15.0); // Dmax, Q+0x48 (0x918753 to 0x918771)

        // S = (dir.y, -dir.x, 0) / hlen, left unnormalised when hlen <= 1e-8 (0x918777 to 0x9187ea).
        let hlen = Vec2::new(dir.x, dir.y).length();
        let mut side = Vec3::new(dir.y, -dir.x, 0.0);
        if hlen > 1e-8 {
            side /= hlen;
        }

        // The two side rays from the focus (0x918816 to 0x9188e7).
        let r1 = ray(world, focus, focus + side * (reach + 1.0));
        let r2 = ray(world, focus, focus - side * (reach + 1.0));
        let (side, free) = if (r1 - r2).abs() >= 0.01 {
            if r2 > r1 { (-side, r2) } else { (side, r1) }
        } else if random.coin(seconds) {
            (-side, r2)
        } else {
            (side, r1)
        };

        // t = min(max(free - 1, 1), D) (0x91899c to 0x9189bb); P = F + S*t (0x9189c1 to 0x9189f0).
        let lateral = (free - 1.0).max(1.0).min(reach);
        let p = focus + side * lateral;

        // Floor: Bfloor = max(min bottom - 1, P.z - 22); c4 = ray down to it, less 2 (0x918a08 to 0x918a9c).
        let bottom = him.bottom.z.min(it.bottom.z);
        let floor = (bottom - 1.0).max(p.z - 22.0);
        let c4 = ray(world, p, Vec3::new(p.x, p.y, floor)) - 2.0;
        let r4 = ray(world, p, Vec3::new(p.x, p.y, top));

        // rnd1 lifts P (0x918afd to 0x918b64).
        let p2 = p + Vec3::Z * random.between(-c4, r4, seconds);

        // Back and forward rays (0x918b37 to 0x918bec), then rnd2 slides the eye (0x918bf6 to 0x918c4e).
        let r5 = ray(world, p2, p2 - dir * (2.0 * half_width));
        let r6 = ray(world, p2, p2 + dir * (0.5 * half_width));
        let eye = p2 + dir * random.between(-r5, r6, seconds);

        // FOV (0x918c77 to 0x918ce2): widened to atan(hw / d) when tan 50 < hw / d.
        let d = (focus - eye).length();
        let fov = if tan < half_width / d {
            (half_width / d).atan().to_degrees()
        } else {
            FOV
        };
        Self { eye, fov }
    }
}
```

Notes for applying it (not done):
- The `enter` signature gains `world: &dyn World` and `half_width: f32`; the caller (the behaviour's slot 1, `camera_finisher_enter`, 00917c60) must pass the camera's world (the same one `spectator.rs` uses) and 25.0 until section 6 is settled.
- `Side::bottom.z` is the bottom; the game takes min over both handles (no nulls in the port).
- The existing unit test `the_eye_is_beside_the_pair_and_stays_while_the_look_follows` expects clamp(F.z - 1) and must be rewritten; a test world with `first_block` returning None gives free = 21.98 on both sides, so the coin decides.
- `pair()` is unchanged (its `top` is Zmax = the game's out3).

## 12. Ledger

Lines recorded with `tools\fn.py note` (existing text kept, corrections appended): 009186f0, 00917dc0, 00551d60, 009180a0, 00466980 (`_CItan`), 00466960 (new line, atan).
