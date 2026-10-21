//! A physics body's pose step (`notes\physics.md` 1.8): the prediction the contact pass sweeps to
//! (006ddad0, through the holder's 006db1f0) and the advance of the pose pass (006ddf30).
//!
//! [game] The body keeps a row quaternion (+0x90 x, y, z, +0x9c w), its axis scales (+0x84..+0x8c),
//! its centre (+0x78), the linear velocity (+0xa0) and the game's NEGATIVE angular velocity (+0xac),
//! and the solver's pseudo pair (+0xb8 linear, +0xc4 angular). One step, both functions alike:
//! h = dt / 2, o = (w + pseudo w) h, q' = q + q o (row form, o's w lane 0), normalised; the basis rows
//! come from q' (0052e700, each row normalised on its own) times the scales; the centre moves by
//! (v + pseudo v) dt. The prediction writes the result out; the advance stores q', the basis and the
//! centre in the body.

/// The body fields the step reads (word layout of the game's body).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyState {
    /// +0x84..+0x8c.
    pub scale: [f32; 3],
    /// +0x78.
    pub centre: [f32; 3],
    /// +0x90..+0x9c: x, y, z, w.
    pub q: [f32; 4],
    pub v: [f32; 3],
    pub w: [f32; 3],
    pub vp: [f32; 3],
    pub wp: [f32; 3],
}

/// [game] the first-order step of the row quaternion (006ddad0 / 006ddf30), not yet normalised.
fn quat_step(b: &BodyState, dt: f32) -> [f32; 4] {
    let h = dt * 0.5;
    let oz = (b.w[2] + b.wp[2]) * h;
    let ox = (b.wp[0] + b.w[0]) * h;
    let oy = (b.w[1] + b.wp[1]) * h;
    let o0 = h * 0.0;
    let [qx, qy, qz, qw] = b.q;
    let x = ((oz * qy + qw * ox + o0 * qx) - oy * qz) + qx;
    let y = (((oy * qw - oz * qx) + o0 * qy) + qz * ox) + qy;
    let z = (((oz * qw + oy * qx) - qy * ox) + o0 * qz) + qz;
    let w = (((o0 * qw - ox * qx) - oy * qy) - oz * qz) + qw;
    [x, y, z, w]
}

fn normalize_quat(q: [f32; 4]) -> [f32; 4] {
    let len = (((q[3] * q[3] + q[2] * q[2]) + q[1] * q[1]) + q[0] * q[0]).sqrt();
    let inv = 1.0 / len;
    [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
}

/// [game] 0052e700: the basis rows of a row quaternion (x, y, z, w), each row normalised.
pub fn quat_basis(q: [f32; 4]) -> [f32; 9] {
    let x = q[0];
    let y2 = q[1] * 2.0;
    let z2 = q[2] * 2.0;
    let w2 = q[3] * 2.0;
    let yw = q[1] * w2;
    let x2 = x * 2.0;
    let zw = q[2] * w2;
    let ww = q[3] * w2 - 1.0;
    let a = x * x2 + ww;
    let c = x * z2 + yw;
    let b = x * y2 - zw;
    let inv = 1.0 / ((c * c + a * a) + b * b).sqrt();
    let mut m = [0.0f32; 9];
    m[0] = inv * a;
    m[1] = inv * b;
    m[2] = c * inv;
    let y = q[1];
    let r0 = y * x2 + zw;
    let r1 = y * y2 + ww;
    let r2 = y * z2 - x * w2;
    let inv = 1.0 / ((r2 * r2 + r1 * r1) + r0 * r0).sqrt();
    m[3] = inv * r0;
    m[4] = r1 * inv;
    m[5] = r2 * inv;
    let z = q[2];
    let s0 = z * x2 - yw;
    let s2 = z * z2 + ww;
    let s1 = z * y2 + x * w2;
    let inv = 1.0 / ((s2 * s2 + s1 * s1) + s0 * s0).sqrt();
    m[6] = inv * s0;
    m[7] = s1 * inv;
    m[8] = s2 * inv;
    m
}

/// [game] 006ddad0: the body's pose after `dt` (basis rows scaled, then the centre), the affine the
/// holder takes as its prediction (+0x168).
pub fn predict(b: &BodyState, dt: f32) -> [f32; 12] {
    let q = normalize_quat(quat_step(b, dt));
    let m = quat_basis(q);
    let s = b.scale;
    let mut out = [0.0f32; 12];
    for row in 0..3 {
        for col in 0..3 {
            out[row * 3 + col] = m[row * 3 + col] * s[row];
        }
    }
    for i in 0..3 {
        out[9 + i] = b.centre[i] + (b.v[i] + b.vp[i]) * dt;
    }
    out
}

/// [game] 006ddf30: the pose pass's advance: the body itself takes q', the basis (unscaled rows, its
/// +0x54..+0x74) and the new centre.
pub fn advance(b: &mut BodyState, dt: f32) -> [f32; 9] {
    let q = normalize_quat(quat_step(b, dt));
    b.q = q;
    for i in 0..3 {
        b.centre[i] += (b.v[i] + b.vp[i]) * dt;
    }
    quat_basis(q)
}

/// The holder's prediction (006db1f0): the predicted affine (+0x168), its inverse (+0x198) and the
/// step's displacement (+0x1c8, predicted origin - current origin).
#[derive(Clone, Copy, Debug)]
pub struct HolderPrediction {
    pub predicted: [f32; 12],
    pub inverse: [f32; 12],
    pub displacement: [f32; 3],
}

/// [game] 006db1f0: only for a holder with a body whose owner's +0x60 & 0x90 == 0x10 and that is a
/// root (+0x204 == 0) or has +0x1d4 bit 10. Children (+0x200) take the predicted affine composed with
/// their local one (not ported: the port's bodies have no children). Clears +0x1d4 bit 8.
pub fn predict_holder(b: &BodyState, current_origin: [f32; 3], dt: f32) -> HolderPrediction {
    let predicted = predict(b, dt);
    let inverse = crate::pair_sweep::inverse_affine3(&predicted);
    let displacement = [predicted[9] - current_origin[0], predicted[10] - current_origin[1], predicted[11] - current_origin[2]];
    HolderPrediction { predicted, inverse, displacement }
}

/// The body's force side (the words gravity, damping and the force integration use).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyForces {
    /// +0x0c..+0x14: the inverse inertia's diagonal (body frame).
    pub inv_inertia: [f32; 3],
    /// +0x18..+0x20: the constant force gravity adds each step (no dt).
    pub gravity: [f32; 3],
    /// +0x28: the inverse mass.
    pub inv_mass: f32,
    /// +0x2c / +0x30: the linear and angular damping factors per step (0.9995 for Bumblebee, [trace]).
    pub damping: [f32; 2],
    /// +0x3c force, +0x48 torque (the game's sign, like the angular velocity).
    pub force: [f32; 3],
    pub torque: [f32; 3],
}

/// [game] 006e96c0: force += gravity; v and w damped; a speed whose square is under 2.5e-5 (w: 1e-8)
/// or over 100000 is zeroed.
pub fn gravity_and_damping(b: &mut BodyState, f: &mut BodyForces) {
    f.force[1] = f.gravity[1] + f.force[1];
    f.force[2] = f.gravity[2] + f.force[2];
    f.force[0] = f.force[0] + f.gravity[0];
    let d = f.damping[0];
    b.v = [b.v[0] * d, d * b.v[1], d * b.v[2]];
    let d = f.damping[1];
    b.w = [b.w[0] * d, b.w[1] * d, b.w[2] * d];
    let l = (b.v[0] * b.v[0] + b.v[1] * b.v[1]) + b.v[2] * b.v[2];
    let a = (b.w[0] * b.w[0] + b.w[1] * b.w[1]) + b.w[2] * b.w[2];
    if l < 2.5e-5 || 100_000.0 < l {
        b.v = [0.0; 3];
    }
    if a < 9.999_999e-9 || 100_000.0 < a {
        b.w = [0.0; 3];
    }
}

/// [game] 0052f1c0: the unnormalised basis of a row quaternion (x, y, z, w).
fn quat_basis_raw(q: [f32; 4]) -> [f32; 9] {
    let x = q[0];
    let w2 = q[3] * 2.0;
    let ww = q[3] * w2 - 1.0;
    let x2 = x * 2.0;
    let yw = q[1] * w2;
    let y2 = q[1] * 2.0;
    let zw = q[2] * w2;
    let z2 = q[2] * 2.0;
    let y = q[1];
    let z = q[2];
    [x * x2 + ww, x * y2 - zw, x * z2 + yw, y * x2 + zw, y * y2 + ww, y * z2 - x * w2, z * x2 - yw, z * y2 + x * w2, z * z2 + ww]
}

/// [game] 006de390: the world inverse inertia, B^T diag(d) B with B the body's basis (0052f1c0) and the
/// product summed as 0052f310 sums it. Rows of the result, row-major.
pub fn world_inverse_inertia(q: [f32; 4], d: [f32; 3]) -> [f32; 9] {
    let b = quat_basis_raw(q);
    let t = [b[0], b[3], b[6], b[1], b[4], b[7], b[2], b[5], b[8]];
    let m = [b[0] * d[0], b[1] * d[1], b[2] * d[2], b[3] * d[0], b[4] * d[1], b[5] * d[2], b[6] * d[0], b[7] * d[1], b[8] * d[2]];
    let mut out = [0.0f32; 9];
    for i in 0..3 {
        for j in 0..3 {
            out[3 * i + j] = (t[j] * m[3 * i] + t[3 + j] * m[3 * i + 1]) + t[6 + j] * m[3 * i + 2];
        }
    }
    out
}

/// [game] 006de230 (an awake dynamic body, through 006daca0): v += (inv mass dt) force; w += I^-1
/// (torque dt) with the world inverse inertia of 006de390.
pub fn integrate_forces(b: &mut BodyState, f: &BodyForces, dt: f32) {
    let k = f.inv_mass * dt;
    b.v = [b.v[0] + k * f.force[0], b.v[1] + f.force[1] * k, b.v[2] + f.force[2] * k];
    let t = [f.torque[0] * dt, f.torque[1] * dt, f.torque[2] * dt];
    let m = world_inverse_inertia(b.q, f.inv_inertia);
    // [game] disassembly 006de2ca-006de363: w + ((M6 tz + M3 ty) + tx M0)
    b.w = [
        b.w[0] + ((m[6] * t[2] + m[3] * t[1]) + t[0] * m[0]),
        b.w[1] + ((m[7] * t[2] + m[4] * t[1]) + m[1] * t[0]),
        b.w[2] + ((m[8] * t[2] + m[5] * t[1]) + m[2] * t[0]),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    const REF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../work/emu/ref/");

    fn rows(name: &str) -> Option<Vec<Vec<f32>>> {
        let text = std::fs::read_to_string(format!("{REF}{name}")).ok()?;
        Some(text.lines().filter(|l| !l.is_empty()).map(|l| l.split('\t').map(|w| f32::from_bits(u32::from_str_radix(w, 16).unwrap())).collect()).collect())
    }

    fn state(w: &[f32]) -> BodyState {
        let v3 = |i: usize| [w[i], w[i + 1], w[i + 2]];
        BodyState { scale: v3(0), centre: v3(3), q: [w[6], w[7], w[8], w[9]], v: v3(10), w: v3(13), vp: v3(16), wp: v3(19) }
    }

    /// 006ddad0 against the emulator (`gen_refs_body.py predict`): dt | state (22) | out (12).
    #[test]
    fn reference_predict() {
        let Some(cases) = rows("body_predict.tsv") else { return };
        for (case, c) in cases.iter().enumerate() {
            let dt = c[0];
            let b = state(&c[1..23]);
            let got = predict(&b, dt);
            for i in 0..12 {
                assert_eq!(got[i].to_bits(), c[23 + i].to_bits(), "case {case}: word {i}: {got:?} vs {:?}", &c[23..35]);
            }
        }
    }

    /// 006ddf30 against the emulator (`gen_refs_body.py advance`): dt | state (22) | q (4), basis (9),
    /// centre (3) after.
    #[test]
    fn reference_advance() {
        let Some(cases) = rows("body_advance.tsv") else { return };
        for (case, c) in cases.iter().enumerate() {
            let dt = c[0];
            let mut b = state(&c[1..23]);
            let basis = advance(&mut b, dt);
            let want = &c[23..];
            for i in 0..4 {
                assert_eq!(b.q[i].to_bits(), want[i].to_bits(), "case {case}: q {i}");
            }
            for i in 0..9 {
                assert_eq!(basis[i].to_bits(), want[4 + i].to_bits(), "case {case}: basis {i}");
            }
            for i in 0..3 {
                assert_eq!(b.centre[i].to_bits(), want[13 + i].to_bits(), "case {case}: centre {i}");
            }
        }
    }

    /// dt | state (22) | forces (15: inertia diag, gravity, inv mass, damping 2, force, torque).
    fn forces(c: &[f32]) -> BodyForces {
        let v3 = |i: usize| [c[i], c[i + 1], c[i + 2]];
        BodyForces { inv_inertia: v3(23), gravity: v3(26), inv_mass: c[29], damping: [c[30], c[31]], force: v3(32), torque: v3(35) }
    }

    fn eq(case: usize, what: &str, got: &[f32], want: &[f32]) {
        for i in 0..got.len() {
            assert_eq!(got[i].to_bits(), want[i].to_bits(), "case {case}: {what} {i}: {got:?} vs {want:?}");
        }
    }

    /// 006e96c0 (`gen_refs_body.py gravity`): ... | force (3), v (3), w (3) after.
    #[test]
    fn reference_gravity() {
        let Some(cases) = rows("body_gravity.tsv") else { return };
        for (case, c) in cases.iter().enumerate() {
            let mut b = state(&c[1..23]);
            let mut f = forces(c);
            gravity_and_damping(&mut b, &mut f);
            eq(case, "force", &f.force, &c[38..41]);
            eq(case, "v", &b.v, &c[41..44]);
            eq(case, "w", &b.w, &c[44..47]);
        }
    }

    /// 006de390 (`gen_refs_body.py inertia`): ... | the 9 words.
    #[test]
    fn reference_inertia() {
        let Some(cases) = rows("body_inertia.tsv") else { return };
        for (case, c) in cases.iter().enumerate() {
            let b = state(&c[1..23]);
            let f = forces(c);
            eq(case, "inertia", &world_inverse_inertia(b.q, f.inv_inertia), &c[38..47]);
        }
    }

    /// 006de230 (`gen_refs_body.py integrate`): ... | v (3), w (3) after.
    #[test]
    fn reference_integrate() {
        let Some(cases) = rows("body_integrate.tsv") else { return };
        for (case, c) in cases.iter().enumerate() {
            let mut b = state(&c[1..23]);
            let f = forces(c);
            integrate_forces(&mut b, &f, c[0]);
            eq(case, "v", &b.v, &c[38..41]);
            eq(case, "w", &b.w, &c[41..44]);
        }
    }

    /// 006db1f0 whole against the emulator (`gen_refs_body.py holder`): dt | state (22) | origin (3) |
    /// +0x168 (12) | +0x198 (12) | +0x1c8 (3).
    #[test]
    fn reference_holder() {
        let Some(cases) = rows("body_holder.tsv") else { return };
        for (case, c) in cases.iter().enumerate() {
            let dt = c[0];
            let b = state(&c[1..23]);
            let p = predict_holder(&b, [c[23], c[24], c[25]], dt);
            let want = &c[26..];
            for i in 0..12 {
                assert_eq!(p.predicted[i].to_bits(), want[i].to_bits(), "case {case}: predicted {i}");
                assert_eq!(p.inverse[i].to_bits(), want[12 + i].to_bits(), "case {case}: inverse {i}");
            }
            for i in 0..3 {
                assert_eq!(p.displacement[i].to_bits(), want[24 + i].to_bits(), "case {case}: displacement {i}");
            }
        }
    }
}
