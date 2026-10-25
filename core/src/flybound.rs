//! Flight boundary: the volume a flyer must stay in, the state it sets when it leaves,
//! and the bounce off the boundary (the steer-back) while it is outside.
//! Read from the game's code: docs/flight-boundary.md sections 4, 6 and 12 (12.4 B0, B2).
//! Pure arithmetic, no engine types. Not wired into flight.rs or the game yet.
//! The wall stop 00868fb0 (helicopter and robot) is `wall_stop`, below. It feeds the helicopter
//! step 00867f20, ported as `steer_helicopter` in flight.rs (docs/flight-boundary.md 12.10).
//!
//! Units: metres, seconds; the game steps every 0.032 s. Offsets are written on the
//! FlyMode object as `mode+0x..` (the fields of `Steerback`) and on the tuning block as
//! `tun+0x..`. The zone's boundary numbers (the two affines) are zone data that has not
//! been decoded yet, so the caller supplies them.

use glam::{Mat3, Vec3};

/// Half size of the box in boundary units. [game 007e7c6f] `00b6ae3c`, and `-1.01` at `00b6ae38`.
pub const BOUNDARY_LIMIT: f32 = 1.01;
/// The `V.z` threshold for "pointing down" (ceiling case). [game 0086ab90 step 7;
/// 00868b00 disasm 868e44-868e4c; constant `00b6b194` = -0.95].
pub const CEILING_Z: f32 = -0.95;
/// Degrees to radians, the factor on `tun+0x60`. [game 00868b00 l.39, constant `00b6a40c`]
pub const DEG_TO_RAD: f32 = 0.017_453_292;
/// 2 pi. [game 00868b00, constant `00b387d8` = 6.2831855]
pub const TWO_PI: f32 = 6.283_185_5;
/// 1 / (2 pi). [game 00868b00, constant `00b6a958` = 0.15915494]
pub const INV_TWO_PI: f32 = 0.159_154_94;
/// The bounce keeps `1 - 0.9 = 0.1` of the normal speed. [game 00868c18, constant `00b6b19c` = -0.9]
pub const BOUNCE_DAMPING: f32 = 0.9;
/// `d = N . steer` above this turns the ramp off and arms the flag. [game 00868b00 l.27, `00b6a8e8` = 0.95]
pub const SCRAPE_LIMIT: f32 = 0.95;
/// Ramp rate of the yaw rate and the yaw ramp, per second, when `5*dt < 1`. [game `00b3476c` = 5.0]
pub const RAMP_RATE: f32 = 5.0;
/// Speed added along the push-back direction while pointing down. [game 00868b00, `00b34570` = 30.0]
pub const PUSH_SPEED: f32 = 30.0;
/// The PITCH target while pointing down, -pi/6 (nose 30 degrees down). [game 00868b00,
/// `00b6b198` = -0.5235988, written to mode+0x1c]
// The game's own literal, kept as written rather than as `-FRAC_PI_6`.
#[allow(clippy::approx_constant)]
pub const PITCH_TARGET_DOWN: f32 = -0.523_598_8;
/// The PITCH ramp rate, per second, while pointing down, when `2*dt < 1`. [game `00b38838` = 2.0]
pub const PITCH_RATE_DOWN: f32 = 2.0;

/// An affine map `out_i = rows[i] . p + offset[i]`.
/// [game 007e7b60] The A map reads `B+0x64/0x70/0x7c` (row 0), `+0x68/0x74/0x80` (row 1),
/// `+0x6c/0x78/0x84` (row 2), offsets `+0x88/0x8c/0x90`. The B map reads `+0x2c/0x38/0x44`
/// (u), `+0x30/0x3c/0x48` (v), `+0x34/0x40/0x4c` (s), offsets `+0x50/0x54/0x58`.
/// So the stored 12 floats are a row-vector affine read column-wise (stride 3 floats, 0xc
/// bytes, between one output's coefficients): that is what the read addresses above say. [game]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub rows: [Vec3; 3],
    pub offset: Vec3,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        rows: [Vec3::X, Vec3::Y, Vec3::Z],
        offset: Vec3::ZERO,
    };

    pub fn apply(&self, p: Vec3) -> Vec3 {
        Vec3::new(
            self.rows[0].dot(p) + self.offset.x,
            self.rows[1].dot(p) + self.offset.y,
            self.rows[2].dot(p) + self.offset.z,
        )
    }

    /// The 3x3 part as a matrix M, so that `M * p` is `(rows[0].p, rows[1].p, rows[2].p)`.
    pub fn linear(&self) -> Mat3 {
        Mat3::from_cols(self.rows[0], self.rows[1], self.rows[2]).transpose()
    }

    /// The inverse map: `inverse().apply(apply(p)) == p`. [game-free: plain algebra]
    pub fn inverse(&self) -> Affine {
        let mi = self.linear().inverse();
        Affine {
            rows: [mi.row(0), mi.row(1), mi.row(2)],
            offset: -(mi * self.offset),
        }
    }
}

/// The result of the boundary test on one point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundaryTest {
    /// True when the point is inside the volume.
    pub inside: bool,
    /// The local normal of the face the point left, `(-1,0,0)`, `(+1,0,0)`, `(0,-1,0)`,
    /// `(0,+1,0)`, `(0,0,-1)` for the ceiling, or radial for the ellipsoid variant.
    /// Zero while inside.
    pub normal: Vec3,
}

/// A flight boundary zone object: `A` takes the world point P to the boundary's own frame
/// L, and `B` takes L to the box coordinates (u, v, s).
/// [game 007e7b60] The `ellipsoid` flag is `+0x94`: 0 = box, 1 = the cylinder variant
/// (the class name says ellipsoid, the test is `u*u + v*v <= 1`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boundary {
    pub a: Affine,
    pub b: Affine,
    pub ellipsoid: bool,
}

impl Boundary {
    /// The boundary test, 007e7b60. [game]
    /// There is NO floor: a point far below the volume is inside.
    /// With both affines at identity-zero (the zone's constructor state) every test passes,
    /// so the default is inside. [game 007e7b60: the zero affines map every point to the origin,
    /// inside the 1.01 bounds, and the test's `bVar5 = true` start is never cleared.]
    /// A and B are set once, at activation: B by the loader 011882b0 (the inverse of the zone's
    /// `boundary` block), A by the class's activate 007e75c0 (the inverse of the owner's node
    /// matrix). [game 011882b0, 007e75c0, 0052ff10] See `read`.
    pub fn test(&self, p: Vec3) -> BoundaryTest {
        let q = self.b.apply(self.a.apply(p));
        let (u, v, s) = (q.x, q.y, q.z);
        let mut inside = true;
        let mut normal = Vec3::ZERO;
        // Box faces, in the game's order: u+, u-, v+, v-, then the ceiling s+.
        // [game 007e7c99-007e7d0e]
        let mut radial = true;
        if u > BOUNDARY_LIMIT {
            normal.x = -1.0;
            inside = false;
        } else if u < -BOUNDARY_LIMIT {
            normal.x = 1.0;
            inside = false;
        } else if v > BOUNDARY_LIMIT {
            normal.y = -1.0;
            inside = false;
        } else if v < -BOUNDARY_LIMIT {
            normal.y = 1.0;
            inside = false;
        } else if s > BOUNDARY_LIMIT {
            normal.z = -1.0;
            inside = false;
            radial = false; // the ceiling has no radial part. [game 007e7cf6]
        }
        if self.ellipsoid {
            // [game 007e7d15-007e7d5e, disassembly checked] If still inside the box, the
            // radius test decides; otherwise the point is outside.
            if inside {
                if u * u + v * v <= 1.0 {
                    return BoundaryTest { inside: true, normal: Vec3::ZERO };
                }
                inside = false;
            }
            if radial {
                // [game 007e7d3f-007e7d59] radial normal, not unit length; the caller normalises.
                normal = Vec3::new(-u, -v, 0.0);
            }
        }
        BoundaryTest { inside, normal }
    }

    /// True when the point is outside the volume (`mode+0x55` is set by the caller from this).
    pub fn outside(&self, p: Vec3) -> bool {
        !self.test(p).inside
    }

    /// Reads a zone's FlightBoundary from its data.
    ///
    /// `block`: the 12 floats of the zone field `boundary` (hash 3e7e0252), a 4x3 row-vector
    /// placement: rows 0..2 are the box's basis vectors, row 3 is the translation. [data] Real
    /// example: the us_test zone's FlightBoundary (dump work\survey\levels\dump_us_test_zone_14.txt
    /// line 180611): `[500,0,0, 0,500,0, 0,0,70, 0,0,0]`.
    /// The loader inverts this block into B [game 011882b0: `FUN_0056f2d0` (the inverse helper
    /// 0052ff10) then the 12-dword copy 00572110 to `obj+0x2c`]; see `invert_placement`.
    /// `a`: the affine A (world to the boundary's own frame L). [game 007e75c0] The class's
    /// activate writes A = inverse(M), M being the owner's node world matrix (`obj+0xc` node
    /// `+0x10..+0x3c` when the instance index `obj+0x14` is 0, else the instance matrix composed
    /// by 0052fbf0), inverted by 0052ff10. The loader 011882b0 writes only B (`obj+0x2c`) and
    /// dwords 0,1 to `obj+0x5c/+0x60`; A is not its job. [game 007e75c0] The port's `world`
    /// argument (level_things::each_thing) is that M when the attachment's bind is identity:
    /// each_thing composes bind with the owner's placement, and the us_test FlightBoundary's
    /// Object (obj6 in `work\survey\levels\dump_us_test_zone_14.txt` line 180611: no
    /// `sub_matrices`, no `hierarchy`) has one, so the composed world is the owner's placement,
    /// which is M [data]. A boundary whose attachment carries a bind would differ, since the
    /// game's M is the owner's node matrix alone; no such zone is read (OPEN: its bind in the
    /// dump). Pass `Affine::IDENTITY` only for a test.
    /// For the us_test example the owner is BG_Bldgs_F_17 at (156.043, -143.19, 68.2943).
    /// The push frame: 007e7830 calls 00550930 with EAX = the boundary object itself (EBX, not
    /// ECX; the ECX of the entry is not used there). 00550930 copies the object's node matrix
    /// `obj+0xc +0x10` when `obj+0x14` is 0 (00572110 from `[obj+0xc]+0x10`, read 2026-10-20),
    /// else the instance matrix composed as activate does. That is the same M that 007e75c0
    /// inverts, so the push frame is A^-1's linear part (`flight.rs` `boundary.a.inverse()`). [game]
    /// `ellipsoid`: from the class, FlightBoundaryEllipsoid (`+0x94 = 1`), not from the data. [game]
    pub fn read(block: &[f32; 12], a: Affine, ellipsoid: bool) -> Boundary {
        Boundary {
            a,
            b: invert_placement(block),
            ellipsoid,
        }
    }
}

/// The affine that the loader stores for a zone placement block, inverted: the 4x3 row-vector
/// block `m` (rows 0..2 basis, row 3 translation `t`) maps p to `p L + t`; its inverse maps
/// back. The stored result has, for output j, `rows[j] = column j of the inverse's 3x3 part`
/// and `offset[j] = -t . rows[j]`. [game 0056f2d0, read 2026-10-18] Two cases:
/// - a block whose third basis row is all zero (a flat boundary) takes the 2D inverse: output
///   j = 0, 1 is basis row j over its squared length, output 2 (`s`) is zero, offsets
///   -t . rows[j] and 0 (so the ceiling test always passes);
/// - otherwise 0052ff10, the full adjugate over the determinant with no guard: a singular
///   block gives non-finite values, as in the game.
pub fn invert_placement(m: &[f32; 12]) -> Affine {
    let t = Vec3::new(m[9], m[10], m[11]);
    if m[6] * m[6] + m[7] * m[7] + m[8] * m[8] == 0.0 {
        let r0 = Vec3::new(m[0], m[1], m[2]);
        let r1 = Vec3::new(m[3], m[4], m[5]);
        let rows = [r0 / r0.length_squared(), r1 / r1.length_squared(), Vec3::ZERO];
        return Affine { rows, offset: Vec3::new(-t.dot(rows[0]), -t.dot(rows[1]), 0.0) };
    }
    let l = Mat3::from_cols(
        Vec3::new(m[0], m[1], m[2]),
        Vec3::new(m[3], m[4], m[5]),
        Vec3::new(m[6], m[7], m[8]),
    );
    // from_cols takes the data's rows as columns, so `inv` is the inverse of the transpose.
    let inv = l.inverse();
    let rows = [inv.row(0), inv.row(1), inv.row(2)];
    Affine {
        rows,
        offset: Vec3::new(-t.dot(rows[0]), -t.dot(rows[1]), -t.dot(rows[2])),
    }
}

/// The push-back direction V (`mode+0x5c..0x64`): the violated face's local normal rotated
/// into the world by the boundary's basis and normalised. [game 0086ab90 caller, notes A3]
/// The basis (FUN_00550930) is not ported; the caller supplies it. [unread]
/// The normalising is 005571b0's (`unit_or_kept`). [game]
pub fn push_back_direction(basis: Mat3, local_normal: Vec3) -> Vec3 {
    unit_or_kept(basis * local_normal)
}

/// The direction N (`mode+0x68..0x70`) from the root to the boundary's centre, normalised
/// by 005502f0 the same way (`unit_or_kept`). [game caller notes A3]
pub fn toward_centre(centre: Vec3, root: Vec3) -> Vec3 {
    unit_or_kept(centre - root)
}

/// [game] 005571b0 / 005502f0 with the length precomputed as the fourth float (007e79e0,
/// 007e7a3e): divided by its length when that is over 1e-8, else left as it is (so a zero
/// vector stays zero, and a shorter one is NOT stretched).
pub fn unit_or_kept(v: Vec3) -> Vec3 {
    let length = v.length();
    if length > 1e-8 { v / length } else { v }
}

/// The contact message's vector, normalised if its squared length is above 1e-8, else kept.
/// [game 0086ada0 B0 step b] 0086ada0 normalises the message's three floats at +0x8, +0xc, +0x10
/// (guard 1e-8, kept below it) and stores them at the object's +0x94, +0x98, +0x9c, in that order.
/// Not used by the port's flight path (grep: no caller). The game's flight boundary check
/// 007e7830 calls 0086ab90, not 0086ada0, so this message is not the flyer's contact normal.
/// The z-component question is closed by that: nothing here feeds `Steerback`.
pub fn contact_normal(message: Vec3) -> Vec3 {
    if message.length_squared() > 1e-8 {
        message.normalize()
    } else {
        message
    }
}

/// Whether the bounce 00868b00 applies (outside) instead of the wall stop 00868fb0.
/// [game 00864320 l.157-158] Not helicopter and not robot: bounce. Otherwise: wall stop
/// (`wall_stop`).
pub fn uses_bounce(helicopter: bool, robot: bool) -> bool {
    !helicopter && !robot
}

/// The wall stop's result: the pair `(out, a)` for the helicopter step 00867f20, and the new
/// body velocity. [game 00868fb0, docs/flight-boundary.md section 12.7]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallStop {
    /// `out`, the first vector argument of 00867f20: (0,1,0) when aligned, else zero. [game 00868fb0]
    pub out: Vec3,
    /// `a`, the scalar of the pair: the body-frame sideways part `xc`, or `yc` when `yc < -0.9`.
    /// [game 00868fb0 008692bd-008692de]
    pub a: f32,
    /// The body linear velocity after the call. Only the fall-through path changes it. [game 00868fb0]
    pub velocity: Vec3,
    /// `b`, the second scalar of the pair (the helicopter's lift input P): +1 in the inverted
    /// branch (outside through the top, V.z < -0.95), else 0. [game 00868ff4 `mov [esp+0x20], 1.0`,
    /// constant 00b38834]. The game's -1 for the invert-pitch option (00869030-00869041) is applied
    /// by the caller, because `steer_helicopter` flips P on that option itself (see `step_in_bounded`).
    pub lift: f32,
}

/// The alignment gate: `c = N . w` at or above this holds the nose straight ahead. [game 00868fb0, constant `00b6a3f0` = 0.8]
pub const WALL_ALIGN_GATE: f32 = 0.8;
/// The velocity kept per call off the wall stop. Not dt-scaled. [game 00868fb0, constant `00b6a8e8` = 0.95]
pub const WALL_VELOCITY_KEEP: f32 = 0.95;
/// The `yc` threshold for taking `yc` as `a` instead of `xc`. [game 00868fb0, constant `00b6b19c` = -0.9]
pub const WALL_FORWARD_LIMIT: f32 = -0.9;

/// The wall stop 00868fb0, for a helicopter or robot outside the boundary (`uses_bounce` false).
/// Writes no FlyMode field. [game]
///
/// `n`: N, `mode+0x68..0x70`, the unit direction toward the boundary's centre. [game]
/// `v_z`: V.z, `mode+0x64`. Below `CEILING_Z` is the inverted branch (outside through the top).
/// [game 00868fb0 00868fe6-00869065, docs/flight-boundary.md 12.11]
/// `row0`: the holder body's first world row, `temp[0..3]` from 006de7e0 (r0x, r0y, r0z). [game]
/// [game 00868fb0 008690ef-00869131] row 0 is the body's right axis (006de7e0 out[0..2]), and
/// `w = (-r0y, r0x, 0)` is forward when level.
/// `inverse`: the holder's inverse 3x3 M, the stored copy 005e3820 returns. [game 00869203-0086928d,
/// 00868fb0's own reads] xc = dhat . (M0, M3, M6) and yc = dhat . (M1, M4, M7) (elements 0..8 in
/// memory order), which is `row(0)` and `row(1)` of this matrix when it is built from those
/// elements column-major (glam's `row(i)` is (col0[i], col1[i], col2[i])).
/// `velocity`: the body linear velocity (`+0xa0..0xa8`). [game]
/// `turn_input`: the owner's input point's first float (`look.x`, read raw, no dead zone), the
/// `a` of the inverted branch. [game 0086905b-0086905f] The owner's input slot +0x28 is the
/// camera stick, the port's `look`: in the player's pad reader (vtable 00b6251c) it is 0084fc60,
/// returning the pad pair d1e864/d1e868 that 009276e0 copies to the camera's stick. [game]
/// `_dt`: param_2 of 00868fb0. [game] Forwarded to 00867f20 and not used here.
pub fn wall_stop(n: Vec3, v_z: f32, row0: Vec3, inverse: Mat3, velocity: Vec3, turn_input: f32, _dt: f32) -> WallStop {
    if v_z < CEILING_Z {
        // [game 00868fb0 inverted branch, 00868ff4-00869065] out stays zero (never written here),
        // a is the input's first float, b = +1, and the velocity is NOT scaled (the 0.95 damp is
        // on the normal path only; 008693bd is reached by a jump at 00869065). Nothing else pulls
        // the body down: the helicopter step's lift does (12.11 "PLAIN ANSWER"). [game]
        return WallStop { out: Vec3::ZERO, a: turn_input, velocity, lift: 1.0 };
    }
    // Step 3, w = (-r0y, r0x, 0). [game 008690ef-00869131]
    let w = Vec3::new(-row0.y, row0.x, 0.0);
    // Step 4, c = N . w. [game]
    let c = n.dot(w);
    if c >= WALL_ALIGN_GATE {
        // Step 5a: aligned. Hold straight ahead, no velocity change. [game 00869135-0086913d]
        return WallStop { out: Vec3::Y, a: 0.0, velocity, lift: 0.0 };
    }
    // Step 5b.a-b: d = N - w; dhat = d / |d| when |d| > 1e-8, else the raw d. [game 00868fb0,
    // the `1e-8 < length` test before its reciprocal: below it the vector is kept as it is.]
    let d = n - w;
    let len = d.length();
    let dhat = if len > 1e-8 { d / len } else { d };
    // Step 5b.c: xc = dhat.x*M0 + dhat.y*M3 + dhat.z*M6; yc = dhat.x*M1 + dhat.y*M4 + dhat.z*M7.
    // [game 00869203-0086928d]
    let xc = dhat.dot(inverse.row(0));
    let yc = dhat.dot(inverse.row(1));
    // Step 5b.d: a = yc when -0.9 > yc, else xc. [game 008692bd-008692de]
    let a = if WALL_FORWARD_LIMIT > yc { yc } else { xc };
    // Step 5b.f: out stays zero; the velocity is damped, not reflected. [game 00869324-0086934b]
    WallStop { out: Vec3::ZERO, a, velocity: velocity * WALL_VELOCITY_KEEP, lift: 0.0 }
}

/// The FlyMode fields the boundary code reads and writes. Field names follow the offsets.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Steerback {
    /// `mode+0x55`: outside the boundary.
    pub outside: bool,
    /// `mode+0x56`: armed, set by the bounce when the scrape limit is passed or when armed.
    pub armed: bool,
    /// `mode+0x5c..0x64` V: the push-back direction.
    pub push: Vec3,
    /// `mode+0x68..0x70` N: toward the boundary's centre.
    pub normal: Vec3,
    /// `mode+0x58`: the target the yaw rate ramps to, `+-X`, with `X = tun+0x30`.
    pub target: f32,
    /// `mode+0x44`: the yaw rate ramp, radians a second. [game 00868b00] the heading integral
    /// mode+0x24 -= dt * mode+0x44 is in the same function, and heading is in radians.
    pub rate: f32,
    /// `mode+0x24`: the yaw integral, wrapped to [-pi, pi).
    pub yaw: f32,
    /// `mode+0x20`: ROLL (rotation about the nose), radians. The bounce ramps it toward
    /// `+-tun+0x60` (the yaw-rate sign) while outside, or to 0 when armed. [game 00868b00,
    /// settled in docs/flight-boundary.md 12.9]
    pub roll: f32,
    /// `mode+0x1c`: PITCH (nose up / down), radians. The bounce decays it to 0 at 5/s when
    /// not pointing down, or ramps it to -pi/6 at 2/s when pointing down (leaving through the
    /// top). [game 00868b00, settled in docs/flight-boundary.md 12.9]
    pub pitch: f32,
}

impl Steerback {
    /// 0086ab90, flight_contact_set. [game] The caller passes `outside` (the arg) and
    /// `x = tun+0x30`. `lateral_dot` is `V . (body lateral basis rotated by pi/2)`; the
    /// caller computes it because the axis pairing is settled (notes section 4 step 8).
    /// [game 0086ab90 else branch, disasm 0086ac20-0086ad78, third pass] The lateral dot is
    /// F . (Rz(pi/2) V), with F the body world matrix's row 1 (elements 3..5, forward, 006de7e0)
    /// and Rz(pi/2) the column matrix 0056fc50 writes. That is V . (Rz(-pi/2) F), and Rz(-pi/2) F
    /// is the body's right axis (x, y, z) -> (f_y, -f_x, f_z). So `lateral_dot` = V . right, the
    /// port's `push.dot(R * X)`, and the sign rule (negative lateral dot -> -X) is the game's.
    /// Settled; the earlier candidate of an inverted sign was a misreading of the matrix order.
    /// Does not write the yaw rate, the yaw, the yaw ramp, the roll, or the velocity. [game]
    pub fn set_contact(&mut self, push: Vec3, normal: Vec3, outside: bool, x: f32, lateral_dot: f32) {
        // [game] mode+0x55 == 0 and arg != 0: rising edge clears mode+0x56.
        if !self.outside && outside {
            self.armed = false;
        }
        self.outside = outside;
        self.push = push;
        self.normal = normal;
        if !outside {
            return;
        }
        // [game 0086ab90 step 6] mode+0x4c = 0 is not modelled here (its meaning is unread).
        // [game] step 7 (V.z < -0.95): the sign follows the rate; otherwise the lateral dot.
        // A negative sign gives -X, else +X. [game 0086ab90 else branch] the lateral dot's axis pairing (third pass).
        let negative = if push.z < CEILING_Z {
            self.rate < 0.0
        } else {
            lateral_dot < 0.0
        };
        self.target = if negative { -x } else { x };
    }

    /// 00868b00, flight_surface_bounce, for a flyer outside the boundary and not a
    /// helicopter or robot (see `uses_bounce`). Returns the new body velocity.
    ///
    /// `velocity`: the body's velocity (`+0xa0..0xa8` of the body object).
    /// `guard`: `body+0x38 == 0` and `body+0x34 == 0.0` (the game writes the body velocity
    /// only then; the same guard covers the push below). [game 00868c67-00868c7d]
    /// `steer`: the stick vector (`[ebp+0..8]`), not unit length in the game as far as read;
    /// Used raw: 00868b00 reads steer x and y and no normalising (`[game]`). `dt`: seconds.
    /// `yaw_rate_limit`: `tun+0x60` in deg/s, turned to radians by 0.017453292 (`[game]` step 3).
    pub fn bounce(
        &mut self,
        velocity: Vec3,
        guard: bool,
        steer: Vec3,
        dt: f32,
        yaw_rate_limit: f32,
    ) -> Vec3 {
        let mut v = velocity;

        // Step 1: the contact tangent n = (steer.y, -steer.x, 0); k = -0.9 (v . n); v += k n.
        // [game 00868b00 l.1-22; disasm 00868b0e-00868cab] NOT scaled by dt. The lever-arm
        // terms (body +0xac..0xb4) are multiplied by 0.0 in the disassembly and vanish. [game]
        let n = Vec3::new(steer.y, -steer.x, 0.0);
        if guard {
            let k = -BOUNCE_DAMPING * v.dot(n);
            v += k * n;
        }

        // Step 2: d = N . steer. Above the scrape limit, or when armed: rate = 0 and armed.
        // Otherwise the rate ramps to the target at 5/s. [game 00868b00 l.24-37, 868d0c-868d54]
        let d = self.normal.dot(steer);
        let zeroed = d > SCRAPE_LIMIT || self.armed;
        if zeroed {
            self.rate = 0.0;
            self.armed = true;
        } else {
            let mut rate = self.target;
            if dt * RAMP_RATE < 1.0 {
                rate = (self.target - self.rate) * dt * RAMP_RATE + self.rate;
            }
            self.rate = rate;
        }

        // Step 3: the yaw target for mode+0x20 is +-tun+0x60 (deg to rad), by the sign of
        // the new rate (0 counts as positive), or 0 in the zeroed branch (the stack slot is
        // left at 0.0). [game 00868b00 l.38-40, disasm 868d19-868d7f]
        let yaw_target = if zeroed {
            0.0
        } else {
            let limit = yaw_rate_limit * DEG_TO_RAD;
            if self.rate >= 0.0 {
                limit
            } else {
                -limit
            }
        };

        // Step 4: the yaw integral, mode+0x24 -= dt * mode+0x44, then wrapped to [-pi, pi).
        // [game 00868b00 disasm 868db0-868dc9 (dt from the frame) and 868dce-868e2a]
        self.yaw -= dt * self.rate;
        let y = self.yaw * INV_TWO_PI + 0.5;
        self.yaw = (y - y.floor() - 0.5) * TWO_PI;

        // Step 5: ROLL (mode+0x20) ramps toward the yaw target at 5/s; jumps when 5*dt >= 1.
        // [game 00868b00 disasm 868e0c-868e3f, mode+0x20 per notes 12.9 step 4]
        if dt * RAMP_RATE < 1.0 {
            self.roll = (yaw_target - self.roll) * dt * RAMP_RATE + self.roll;
        } else {
            self.roll = yaw_target;
        }

        // Step 6: PITCH (mode+0x1c). [game 00868b00 disasm 868e44-868e50, 868f63-868f96;
        // notes 12.9 step 5]
        if self.push.z >= CEILING_Z {
            // Not pointing down: the pitch decays to 0 at 5/s; jumps when 5*dt >= 1.
            // [game 00868b00 disasm 868e44 test, the 0 path]
            if dt * RAMP_RATE < 1.0 {
                self.pitch = (0.0 - self.pitch) * dt * RAMP_RATE + self.pitch;
            } else {
                self.pitch = 0.0;
            }
        } else {
            // Pointing down (leaving through the top): push the body along V at 30 * dt
            // (guarded), then the pitch ramps to -pi/6 at 2/s; jumps when 2*dt >= 1.
            // [game 00868b00 disasm 868e56-868ef8, 868f20-868f55, velocity push 30.0 at
            // 00b34570, target 00b6b198]
            if guard {
                v += self.push * PUSH_SPEED * dt;
            }
            if dt * PITCH_RATE_DOWN < 1.0 {
                self.pitch = (PITCH_TARGET_DOWN - self.pitch) * dt * PITCH_RATE_DOWN + self.pitch;
            } else {
                self.pitch = PITCH_TARGET_DOWN;
            }
        }

        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 0.032;

    fn identity_box() -> Boundary {
        Boundary {
            a: Affine::IDENTITY,
            b: Affine::IDENTITY,
            ellipsoid: false,
        }
    }

    fn ellipsoid() -> Boundary {
        Boundary {
            ellipsoid: true,
            ..identity_box()
        }
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn box_inside_up_to_the_margin_on_each_side() {
        let b = identity_box();
        assert!(b.outside(Vec3::new(1.0101, 0.0, 0.0)));
        assert!(!b.outside(Vec3::new(1.01, 0.0, 0.0)));
        assert!(!b.outside(Vec3::new(-1.01, 0.0, 0.0)));
        assert!(b.outside(Vec3::new(-1.0101, 0.0, 0.0)));
        assert!(!b.outside(Vec3::new(0.0, 1.01, 0.0)));
        assert!(b.outside(Vec3::new(0.0, -1.0101, 0.0)));
    }

    #[test]
    fn the_ceiling_is_limited_but_there_is_no_floor() {
        let b = identity_box();
        assert!(!b.outside(Vec3::new(0.0, 0.0, 1.01)));
        assert!(b.outside(Vec3::new(0.0, 0.0, 1.02)));
        // No floor: far below is inside.
        assert!(!b.outside(Vec3::new(0.0, 0.0, -1000.0)));
    }

    #[test]
    fn the_face_normals_point_back_into_the_volume() {
        let b = identity_box();
        assert_eq!(b.test(Vec3::new(2.0, 0.0, 0.0)).normal, Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(b.test(Vec3::new(-2.0, 0.0, 0.0)).normal, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(b.test(Vec3::new(0.0, 2.0, 0.0)).normal, Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(b.test(Vec3::new(0.0, -2.0, 0.0)).normal, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(b.test(Vec3::new(0.0, 0.0, 2.0)).normal, Vec3::new(0.0, 0.0, -1.0));
        assert_eq!(b.test(Vec3::ZERO).normal, Vec3::ZERO);
    }

    #[test]
    fn the_ellipsoid_variant_uses_a_radius_one_side() {
        let e = ellipsoid();
        assert!(e.outside(Vec3::new(0.8, 0.8, 0.0)));
        assert!(!e.outside(Vec3::new(0.7, 0.7, 0.0)));
        // Radial normal, not unit length.
        let t = e.test(Vec3::new(0.8, 0.8, 0.0));
        assert!(close(t.normal, Vec3::new(-0.8, -0.8, 0.0)));
        // The ceiling still applies, with no radial part.
        assert!(e.outside(Vec3::new(0.0, 0.0, 1.02)));
        assert_eq!(e.test(Vec3::new(0.0, 0.0, 1.02)).normal, Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn a_moved_boundary_is_tested_in_its_own_frame() {
        // A = identity; B maps the world x to u with a half size of 2 (u = x / 2).
        let b = Boundary {
            a: Affine::IDENTITY,
            b: Affine {
                rows: [Vec3::new(0.5, 0.0, 0.0), Vec3::Y, Vec3::Z],
                offset: Vec3::ZERO,
            },
            ellipsoid: false,
        };
        assert!(!b.outside(Vec3::new(2.0, 0.0, 0.0)));
        assert!(b.outside(Vec3::new(2.1, 0.0, 0.0)));
    }

    #[test]
    fn a_head_on_bounce_keeps_a_tenth_of_the_normal_speed() {
        let mut s = Steerback { outside: true, ..Default::default() };
        // steer (-1,0,0) gives n = (0,1,0).
        let v = s.bounce(Vec3::new(0.0, 10.0, 0.0), true, Vec3::new(-1.0, 0.0, 0.0), DT, 0.0);
        assert!(close(v, Vec3::new(0.0, 1.0, 0.0)), "{v:?}");
        let v = s.bounce(Vec3::new(0.0, -10.0, 0.0), true, Vec3::new(-1.0, 0.0, 0.0), DT, 0.0);
        assert!(close(v, Vec3::new(0.0, -1.0, 0.0)), "{v:?}");
    }

    #[test]
    fn a_glancing_bounce_keeps_the_tangential_part() {
        let mut s = Steerback { outside: true, ..Default::default() };
        let v = s.bounce(Vec3::new(5.0, 10.0, 0.0), true, Vec3::new(-1.0, 0.0, 0.0), DT, 0.0);
        assert!(close(v, Vec3::new(5.0, 1.0, 0.0)), "{v:?}");
    }

    #[test]
    fn no_velocity_change_without_the_guard() {
        let mut s = Steerback { outside: true, ..Default::default() };
        let v = s.bounce(Vec3::new(0.0, 10.0, 0.0), false, Vec3::new(-1.0, 0.0, 0.0), DT, 0.0);
        assert!(close(v, Vec3::new(0.0, 10.0, 0.0)));
    }

    #[test]
    fn the_pitch_decays_when_not_pointing_down() {
        // [game 00868b00, mode+0x1c decays to 0 at 5/s on the side exit]
        let mut s = Steerback { outside: true, pitch: 0.5, push: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
        s.bounce(Vec3::ZERO, true, Vec3::new(-1.0, 0.0, 0.0), DT, 0.0);
        // 0.5 * (1 - 5*0.032) = 0.42
        assert!((s.pitch - 0.42).abs() < 1e-5, "{}", s.pitch);
        assert!(s.roll.abs() < 1e-6, "the roll target is 0 with no yaw rate limit: {}", s.roll);
    }

    #[test]
    fn pointing_down_pushes_the_body_and_pitches_the_nose_to_minus_thirty_degrees() {
        // [game 00868b00] top exit: velocity += V * 30 * dt, mode+0x1c ramps to -pi/6 at 2/s.
        let mut s = Steerback { outside: true, push: Vec3::new(0.0, 0.0, -1.0), ..Default::default() };
        let v = s.bounce(Vec3::ZERO, true, Vec3::new(-1.0, 0.0, 0.0), DT, 0.0);
        assert!(close(v, Vec3::new(0.0, 0.0, -30.0 * DT)), "{v:?}");
        // pitch = (-pi/6 - 0) * 0.032 * 2 = -0.0335
        assert!((s.pitch - (PITCH_TARGET_DOWN * DT * 2.0)).abs() < 1e-5, "{}", s.pitch);
        // The roll is not touched by the top exit's pitch; it follows the yaw-rate target (here 0).
        assert!(s.roll.abs() < 1e-6, "{}", s.roll);
    }

    #[test]
    fn the_roll_ramps_to_the_signed_yaw_rate_limit_while_level() {
        // [game 00868b00 step 3-5] side exit: mode+0x20 ramps to +-tun+0x60 (deg to rad), the
        // sign of the yaw rate. Limit 90 deg; the rate ramps to -X from 0 (the stick is -x).
        let mut s = Steerback { outside: true, target: -1.0, ..Default::default() };
        s.bounce(Vec3::ZERO, true, Vec3::new(-1.0, 0.0, 0.0), DT, 90.0);
        // rate went negative, so the roll target is -90 deg; roll = (target - 0) * 5 * dt.
        assert!(s.rate < 0.0, "{}", s.rate);
        let expected = -90.0 * DEG_TO_RAD * DT * RAMP_RATE;
        assert!((s.roll - expected).abs() < 1e-5, "{} vs {}", s.roll, expected);
        assert!(s.pitch.abs() < 1e-6, "{}", s.pitch);
    }

    #[test]
    fn contact_sets_the_sign_of_the_target_from_the_rate_when_pointing_down() {
        let mut s = Steerback { rate: -1.0, ..Default::default() };
        s.set_contact(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, true, 2.0, 1.0);
        assert_eq!(s.target, -2.0);
        assert!(s.outside);
        let mut s = Steerback { rate: 1.0, ..Default::default() };
        s.set_contact(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, true, 2.0, -1.0);
        assert_eq!(s.target, 2.0);
    }

    #[test]
    fn the_yaw_wraps_into_minus_pi_to_pi() {
        let mut s = Steerback { outside: true, yaw: 4.0, ..Default::default() };
        s.bounce(Vec3::ZERO, false, Vec3::new(-1.0, 0.0, 0.0), DT, 0.0);
        assert!(s.yaw >= -std::f32::consts::PI && s.yaw < std::f32::consts::PI, "{}", s.yaw);
        assert!((s.yaw - (4.0 - std::f32::consts::TAU)).abs() < 1e-4, "{}", s.yaw);
    }

    #[test]
    fn a_zone_block_reads_as_a_box_with_the_real_half_sizes() {
        // [data] The us_test zone's FlightBoundary: field `boundary` = 4x3 block
        // [500,0,0 | 0,500,0 | 0,0,70 | 0,0,0]; dump work\survey\levels\dump_us_test_zone_14.txt
        // line 180611 (the object sits on BG_Bldgs_F_17). A = identity here (test only).
        let block = [500.0, 0.0, 0.0, 0.0, 500.0, 0.0, 0.0, 0.0, 70.0, 0.0, 0.0, 0.0];
        let b = Boundary::read(&block, Affine::IDENTITY, false);
        assert!(!b.outside(Vec3::new(500.0, 0.0, 0.0)));
        assert!(b.outside(Vec3::new(506.0, 0.0, 0.0)));
        assert!(b.outside(Vec3::new(-506.0, 0.0, 0.0)));
        assert!(!b.outside(Vec3::new(0.0, 0.0, 70.0)));
        assert!(b.outside(Vec3::new(0.0, 0.0, 71.0)));
        // No floor.
        assert!(!b.outside(Vec3::new(0.0, 0.0, -5000.0)));
        // The class, not the data, decides the variant.
        let e = Boundary::read(&block, Affine::IDENTITY, true);
        assert!(e.ellipsoid);
        assert!(e.outside(Vec3::new(400.0, 400.0, 0.0)));
    }

    #[test]
    fn a_translated_block_inverts_its_translation() {
        // A placement with translation (100, 0, 0) and unit basis: the box's centre is at x = 100.
        let block = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 100.0, 0.0, 0.0];
        let b = Boundary::read(&block, Affine::IDENTITY, false);
        // The box centre is at x = 100: u = x - 100, so the half size 1.01 is about that centre.
        assert!(close(b.b.apply(Vec3::new(100.0, 0.0, 0.0)), Vec3::ZERO));
        assert!(!b.outside(Vec3::new(100.0 + 1.0, 0.0, 0.0)));
        assert!(b.outside(Vec3::new(100.0 + 1.02, 0.0, 0.0)));
        assert!(!b.outside(Vec3::new(100.0, 0.0, 0.0)));
    }

    #[test]
    fn a_is_the_inverse_of_the_owner_placement_not_the_placement() {
        // [game 007e75c0] A maps the world to the owner's frame L: A = invert_placement(owner),
        // the owner's node matrix M (what level_things::each_thing hands over with an identity bind, [data] us_test),
        // block as B's source. The formula is the game's (see `Boundary::read`).
        // Owner at (1000, 0, 0), unit basis. Block: a 500 x 500 x 70 box about the owner's
        // origin, `[500,0,0 | 0,500,0 | 0,0,70 | 0,0,0]` (the us_test shape, [data]).
        let owner = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1000.0, 0.0, 0.0];
        let block = [500.0, 0.0, 0.0, 0.0, 500.0, 0.0, 0.0, 0.0, 70.0, 0.0, 0.0, 0.0];
        let b = Boundary::read(&block, invert_placement(&owner), false);
        // The owner's centre, 30 m up: inside the box.
        assert!(!b.outside(Vec3::new(1000.0, 0.0, 30.0)));
        // 100 m up: past the 70 m ceiling.
        assert!(b.outside(Vec3::new(1000.0, 0.0, 100.0)));
        // The world origin is 1000 m from the box: outside.
        assert!(b.outside(Vec3::ZERO));
        // Wrong direction (A = the placement itself) would put the box at x = 2000 and call the
        // owner's centre outside; the identity A puts the box at the origin and calls the owner's
        // centre outside too. Both are refused by the asserts above.
        let identity = Boundary::read(&block, Affine::IDENTITY, false);
        assert!(identity.outside(Vec3::new(1000.0, 0.0, 30.0)));
        assert!(!identity.outside(Vec3::ZERO));
    }

    #[test]
    fn an_aligned_nose_holds_straight_ahead_and_keeps_the_velocity() {
        // row0 = X gives w = (0,1,0); N = w, so c = 1 >= 0.8.
        let v = Vec3::new(3.0, 4.0, 0.0);
        let r = wall_stop(Vec3::Y, 0.0, Vec3::X, Mat3::IDENTITY, v, 0.0, DT);
        assert_eq!(r.out, Vec3::Y);
        assert_eq!(r.a, 0.0);
        assert_eq!(r.lift, 0.0);
        assert_eq!(r.velocity, v);
    }

    #[test]
    fn off_the_alignment_gate_the_velocity_is_damped_and_a_is_xc() {
        // row0 = X gives w = (0,1,0); N = X gives c = 0 < 0.8.
        // d = N - w = (1,-1,0), dhat = (1,-1,0)/sqrt2, identity M: xc = dhat.x, yc = dhat.y = -0.707,
        // not below -0.9, so a = xc = 0.707.
        let v = Vec3::new(0.0, 10.0, 0.0);
        let r = wall_stop(Vec3::X, 0.0, Vec3::X, Mat3::IDENTITY, v, 0.0, DT);
        assert_eq!(r.out, Vec3::ZERO);
        assert_eq!(r.lift, 0.0);
        assert!((r.a - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5, "{}", r.a);
        assert!(close(r.velocity, v * 0.95), "{:?}", r.velocity);
    }

    #[test]
    fn a_is_yc_when_yc_is_below_minus_point_nine() {
        // row0 = Z gives w = (0,0,0), so c = 0 and d = N. N = (0,-1,0): yc = -1 < -0.9, so a = -1.
        let r = wall_stop(Vec3::new(0.0, -1.0, 0.0), 0.0, Vec3::Z, Mat3::IDENTITY, Vec3::ZERO, 0.0, DT);
        assert_eq!(r.a, -1.0);
        assert_eq!(r.out, Vec3::ZERO);
    }

    /// [game] 0056f2d0: a flat block (zero third row) inverts in 2D, and nothing is above it.
    #[test]
    fn a_flat_boundary_block_inverts_in_two_dimensions() {
        let b = invert_placement(&[500.0, 0.0, 0.0, 0.0, 250.0, 0.0, 0.0, 0.0, 0.0, 10.0, 20.0, 5.0]);
        let q = b.apply(Vec3::new(510.0, 270.0, 9999.0));
        assert!((q - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-6, "{q}");
        // The full inverse on a proper block still maps the corner to (1, 1, 1).
        let full = invert_placement(&[500.0, 0.0, 0.0, 0.0, 250.0, 0.0, 0.0, 0.0, 70.0, 10.0, 20.0, 5.0]);
        assert!((full.apply(Vec3::new(510.0, 270.0, 75.0)) - Vec3::ONE).length() < 1e-6);
    }

    /// [game] 005571b0 / 005502f0: unit when longer than 1e-8, otherwise kept as it is.
    #[test]
    fn the_games_normalise_keeps_a_tiny_vector() {
        assert_eq!(unit_or_kept(Vec3::new(0.0, 3.0, 4.0)), Vec3::new(0.0, 0.6, 0.8));
        assert_eq!(unit_or_kept(Vec3::ZERO), Vec3::ZERO);
        let tiny = Vec3::new(1e-9, 0.0, 0.0);
        assert_eq!(unit_or_kept(tiny), tiny);
        assert_eq!(toward_centre(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO), Vec3::Z);
    }

    #[test]
    fn a_zero_difference_gives_no_nan() {
        // row0 = (0.5,0,0) gives w = (0,0.5,0); N = w gives c = 0.25 < 0.8 and d = 0.
        let v = Vec3::new(1.0, 0.0, 0.0);
        let r = wall_stop(Vec3::new(0.0, 0.5, 0.0), 0.0, Vec3::new(0.5, 0.0, 0.0), Mat3::IDENTITY, v, 0.0, DT);
        assert!(r.a.is_finite());
        assert_eq!(r.a, 0.0);
        assert!(close(r.velocity, Vec3::new(0.95, 0.0, 0.0)));
    }

    /// [game 00868fb0 inverted branch, docs/flight-boundary.md 12.11] Outside through the top:
    /// out = 0, a = the input's first float (raw), b = +1, and the velocity is not scaled.
    #[test]
    fn the_inverted_branch_takes_the_turn_input_lifts_down_and_keeps_the_velocity() {
        let v = Vec3::new(1.0, 2.0, 3.0);
        let r = wall_stop(Vec3::Y, -1.0, Vec3::X, Mat3::IDENTITY, v, 0.37, DT);
        assert_eq!(r.out, Vec3::ZERO);
        assert_eq!(r.a, 0.37);
        assert_eq!(r.lift, 1.0);
        assert_eq!(r.velocity, v);
    }
}
