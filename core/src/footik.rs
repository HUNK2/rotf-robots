//! Foot support applied after animation layers, in game space (z up).
//! Original constants/solver evidence: docs/foot-ik.md. Character binding,
//! sole proxy, activation and plane adaptation are [assumed], documented there. settles: a decision on whether the
//! proxy stays for the robots at all (roster_survey, `docs/foot-ik.md`: no FootIK object in their packs).
//! NOT the game's for the playable robots: none of their packs holds a FootIK object, and the
//! solver is built only from data (0073f000 through the factory 0073f1b0) [game, data,
//! `roster_survey` below]. The game crate's `animation.rs` leaves it off unless `--foot-ik`.
use glam::{Affine3A, Quat, Vec3};

use crate::camera::Sight;
use crate::formats::{
    hash::{crc32, lower33},
    model::Library,
    scene::ObjectDef,
};
use crate::pose::affine;

/// [game: 0073db50] Probe extent, contact bias, reach margin and angular rate.
pub const PROBE: f32 = 1.5;
const BIAS: f32 = 0.03;
const REACH_MARGIN: f32 = 0.001;
const TURN_RATE: f32 = 24.0;
const UPDATE: f32 = 0.032; // [game] Simulation cadence.
/// The share of its way back up the body's lowering takes an update (`FootIk::apply`). [assumed port design]
const BODY_RISE: f32 = 0.15;

#[derive(Clone, Debug)]
pub struct Leg {
    pub hip: usize,
    pub knee: usize,
    pub foot: usize,
    /// settles: the game's own foot support, which the 0073f000 loader would give (none for these robots: `docs/foot-ik.md`).
    /// [assumed] Rest-space rectangular sole proxy, expressed in ankle space.
    pub sole: [Vec3; 4],
}

impl Leg {
    /// The limb ending at `foot` (its parent the knee, the knee's the hip), its sole measured in
    /// `pose` (every node in the object's space, as `pose::Rig::pose` gives it) instead of the bind
    /// pose: the rectangle round the rigid mesh points of `foot` and every node under it that lie
    /// within `reach` of their lowest, at that lowest height.
    /// [assumed] For a limb that stands on more than a foot mesh, in a pose it stands in
    /// (Devastator's hands on their fingertips: `crate::giant::limbs`). settles: none in the game,
    /// which has no foot IK on him (no FootIK object in his pack, `docs/devastator.md`).
    pub fn planted(object: &ObjectDef, library: &Library, pose: &[Affine3A], foot: usize, reach: f32) -> Option<Self> {
        let knee = object.nodes[foot].parent?;
        let hip = object.nodes[knee].parent?;
        let under = |mut n: usize| loop {
            if n == foot {
                return true;
            }
            match object.nodes[n].parent {
                Some(p) => n = p,
                None => return false,
            }
        };
        let mut points = Vec::new();
        for render in object.renders.iter().filter(|r| under(r.node)) {
            // Skinned meshes are in model space and need a different bounds path.
            let Some(mesh) = library.meshes.get(&lower33(&render.model)).filter(|m| m.skeleton.is_none()) else {
                continue;
            };
            let at = pose[render.node];
            points.extend(mesh.parts.iter().flat_map(|p| &p.positions).map(|p| at.transform_point3(Vec3::from(*p))));
        }
        let low = points.iter().map(|p| p.z).fold(f32::INFINITY, f32::min);
        if !low.is_finite() {
            return None;
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for p in points.iter().filter(|p| p.z <= low + reach) {
            min = min.min(*p);
            max = max.max(*p);
        }
        let inverse = pose[foot].inverse();
        let sole = [
            Vec3::new(min.x, min.y, low),
            Vec3::new(max.x, min.y, low),
            Vec3::new(min.x, max.y, low),
            Vec3::new(max.x, max.y, low),
        ]
        .map(|p| inverse.transform_point3(p));
        Some(Leg { hip, knee, foot, sole })
    }
}

#[derive(Clone, Debug)]
pub struct FootRig {
    /// The supported limbs: a robot's two legs, Devastator's legs and arms (`crate::giant::limbs`).
    pub legs: Vec<Leg>,
    pub body: usize,
    /// The vertical probe's half extent: `PROBE` [game], scaled for a giant [assumed].
    pub probe: f32,
    /// A heavy body's planting (Devastator's, `crate::giant::WEIGHT`); `None` is the game's filter.
    pub weight: Option<Weight>,
}

/// How a heavy body's limbs and body follow the ground [assumed port design: the user, 2026-10-23,
/// Devastator "feels weightless when he's pushing his feet up over a small obstacle"; the game's
/// filter halves the gap every 0.032 s, so a giant's limb pops up a 2 m block in 0.1 s and his body
/// never moves]. Each limb's lift is a critically damped spring (slower up than down), aimed at the
/// higher of the ground under it and the ground where it will be `lead` seconds on at its present
/// speed (so a swinging limb rises as it comes to an edge, not after it is in it). The body rises a
/// share of the limbs' lift on an under-damped spring, and a limb landing on its height pushes it
/// down by a share of the limb's speed: the weight settling onto it.
#[derive(Clone, Copy, Debug)]
pub struct Weight {
    /// The limbs' spring rate (rad/s) going up and coming down.
    pub rise: f32,
    pub fall: f32,
    /// Seconds a moving limb looks ahead, and the farthest it looks (m).
    pub lead: f32,
    pub lead_reach: f32,
    /// The share of the supported limbs' mean lift the body rises by.
    pub heave: f32,
    /// The body's spring rate (rad/s) and damping ratio.
    pub body_rate: f32,
    pub body_damping: f32,
    /// The share of a landing limb's speed (m/s) the body is pushed down at.
    pub settle: f32,
}

impl FootRig {
    /// settles: the original link endpoints (the 0073f000 loader's three `links` resolved by name CRC); absent for Bumblebee.
    /// [data] Hierarchy/rigid mesh coordinates. [assumed] Named ancestor chains,
    /// rectangular sole and Main body compensation: Bumblebee has no FootIK links.
    pub fn read(object: &ObjectDef, library: &Library) -> Option<Self> {
        let node = |name: &str| {
            object
                .nodes
                .iter()
                .position(|n| n.name_hash == crc32(name.as_bytes()))
        };
        let leg = |side: &str| -> Option<Leg> {
            let foot = node(&format!("Foot_{side}"))?;
            let knee = object.nodes[foot].parent?;
            let hip = object.nodes[knee].parent?;
            let toe = node(&format!("Midfoot_{side}"))?;
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for render in object
                .renders
                .iter()
                .filter(|r| r.node == foot || r.node == toe)
            {
                let mesh = library.meshes.get(&lower33(&render.model))?;
                // Skinned meshes are in model space and need a different bounds path.
                if mesh.skeleton.is_some() {
                    continue;
                }
                let bind = affine(&object.nodes[render.node].bind);
                for p in mesh.parts.iter().flat_map(|p| &p.positions) {
                    let p = bind.transform_point3(Vec3::from(*p));
                    min = min.min(p);
                    max = max.max(p);
                }
            }
            if !min.is_finite() || !max.is_finite() {
                return None;
            }
            let inverse = affine(&object.nodes[foot].bind).inverse();
            let sole = [
                Vec3::new(min.x, min.y, min.z),
                Vec3::new(max.x, min.y, min.z),
                Vec3::new(min.x, max.y, min.z),
                Vec3::new(max.x, max.y, min.z),
            ]
            .map(|p| inverse.transform_point3(p));
            Some(Leg {
                hip,
                knee,
                foot,
                sole,
            })
        };
        Some(Self {
            legs: vec![leg("L")?, leg("R")?],
            body: node("Main")?,
            probe: PROBE,
            weight: None,
        })
    }
}

/// Parent accumulation shared by the pose pass and engine-independent tests.
pub fn accumulate(local: &[Affine3A], parents: &[Option<usize>]) -> Vec<Affine3A> {
    let mut out = Vec::with_capacity(local.len());
    for (i, &at) in local.iter().enumerate() {
        out.push(parents[i].map_or(at, |p| out[p] * at));
    }
    out
}

/// Solve a two-segment chain without stretching; retain the animated bend plane.
/// [game] Cosine-law geometry and sum-minus-.001 cap (0073db50).
/// settles: the bend-plane rule in 0073db50's two-bone solve (its exact bone axes are unresolved: `docs/foot-ik.md`).
/// [assumed] Bend plane follows the incoming animation; singular fallback is geometric.
pub fn solve_knee(hip: Vec3, knee: Vec3, ankle: Vec3, target: Vec3) -> Option<(Vec3, Vec3)> {
    let a = hip.distance(knee);
    let b = knee.distance(ankle);
    if a <= REACH_MARGIN || b <= REACH_MARGIN || !target.is_finite() {
        return None;
    }
    let direction = (target - hip)
        .try_normalize()
        .or_else(|| (ankle - hip).try_normalize())?;
    let distance = hip
        .distance(target)
        .clamp((a - b).abs() + REACH_MARGIN, a + b - REACH_MARGIN);
    let along = (a * a - b * b + distance * distance) / (2.0 * distance);
    let height = (a * a - along * along).max(0.0).sqrt();
    let bend = knee - hip - direction * (knee - hip).dot(direction);
    let bend = bend.try_normalize().unwrap_or_else(|| {
        let axis = if direction.y.abs() < 0.9 {
            Vec3::Y
        } else {
            Vec3::X
        };
        (axis - direction * axis.dot(direction)).normalize()
    });
    Some((
        hip + direction * along + bend * height,
        hip + direction * distance,
    ))
}

/// Rotate a joint in object space, converting back to its parent's local frame.
fn rotate_joint(local: &mut [Affine3A], parents: &[Option<usize>], joint: usize, turn: Quat) {
    let world = accumulate(local, parents);
    let (scale, rotation, position) = world[joint].to_scale_rotation_translation();
    let at =
        Affine3A::from_scale_rotation_translation(scale, (turn * rotation).normalize(), position);
    local[joint] = parents[joint].map_or(at, |p| world[p].inverse() * at);
}

fn apply_leg(
    local: &mut [Affine3A],
    parents: &[Option<usize>],
    leg: &Leg,
    target: Vec3,
    rotation: Quat,
) {
    let world = accumulate(local, parents);
    let h = world[leg.hip].translation.into();
    let k = world[leg.knee].translation.into();
    let f = world[leg.foot].translation.into();
    let Some((new_knee, end)) = solve_knee(h, k, f, target) else {
        return;
    };
    rotate_joint(
        local,
        parents,
        leg.hip,
        Quat::from_rotation_arc((k - h).normalize(), (new_knee - h).normalize()),
    );
    let world = accumulate(local, parents);
    let k: Vec3 = world[leg.knee].translation.into();
    let f: Vec3 = world[leg.foot].translation.into();
    rotate_joint(
        local,
        parents,
        leg.knee,
        Quat::from_rotation_arc((f - k).normalize(), (end - k).normalize()),
    );
    let world = accumulate(local, parents);
    let (scale, _, position) = world[leg.foot].to_scale_rotation_translation();
    let at = Affine3A::from_scale_rotation_translation(scale, rotation, position);
    local[leg.foot] = parents[leg.foot].map_or(at, |p| world[p].inverse() * at);
}

/// The filtered state, one entry per limb of the rig it is applied with.
#[derive(Default)]
pub struct FootIk {
    offsets: Vec<f32>,
    turns: Vec<Quat>,
    remainder: f32,
    previous: Option<Vec3>,
    /// A heavy body's state (`Weight`): each limb's lift speed, the fastest it rose in its present
    /// rise, where the clip drew it last frame; the body's lift and its speed.
    speeds: Vec<f32>,
    peaks: Vec<f32>,
    drawn: Vec<Option<Vec3>>,
    body: f32,
    body_speed: f32,
}

/// A limb's lift for the ground under its sole with its ankle at `ankle` (world), and the sole's turn
/// onto that ground (`None` with no ground in the probe's reach).
fn limb_lift(rig: &FootRig, leg: &Leg, ankle: Affine3A, origin: Vec3, ground: &impl Sight) -> Option<(f32, Quat)> {
    let (_, rotation, p) = ankle.to_scale_rotation_translation();
    let centre = leg.sole.iter().copied().sum::<Vec3>() / 4.0;
    // settles: the original probe binding to the animated endpoint (0073db50; not bound for these robots).
    // [game] Vertical probe +/-1.5. [assumed] Centre and corners of the
    // mesh-derived sole instead of the original linked endpoint. Sampling
    // corners prevents a toe sinking into a higher surface at a join.
    let contacts: Vec<_> = std::iter::once(centre)
        .chain(leg.sole)
        .filter_map(|s| {
            let probe = ankle.transform_point3(s);
            ground.sight(probe + Vec3::Z * rig.probe, probe - Vec3::Z * rig.probe)
        })
        .filter(|h| h.normal.z > 1e-6)
        .collect();
    let before = leg
        .sole
        .iter()
        .map(|s| (rotation * *s).z)
        .fold(f32::INFINITY, f32::min);
    let correction = |hit: &crate::camera::Hit, rotated: Quat| {
        let after = leg
            .sole
            .iter()
            .map(|s| hit.normal.dot(rotated * *s) / hit.normal.z)
            .fold(f32::INFINITY, f32::min);
        let height = hit.point.z
            - hit.normal.truncate().dot((p - hit.point).truncate()) / hit.normal.z;
        height - origin.z + before - after + BIAS
    };
    let candidate = |hit: &crate::camera::Hit| {
        correction(
            hit,
            Quat::from_rotation_arc(Vec3::Z, hit.normal.normalize()) * rotation,
        )
    };
    let hit = contacts
        .iter()
        .max_by(|a, b| candidate(a).total_cmp(&candidate(b)))?;
    let turn = Quat::from_rotation_arc(Vec3::Z, hit.normal.normalize());
    // settles: 0073db50's foot-turn selection (only its halfway and dt*24 lines are read).
    // [assumed] Respect every sampled plane after selecting the foot turn.
    let lift = contacts
        .iter()
        .map(|h| correction(h, turn * rotation))
        .fold(f32::NEG_INFINITY, f32::max);
    Some((lift, turn))
}

impl FootIk {
    pub fn reset(&mut self) {
        self.offsets.iter_mut().for_each(|o| *o = 0.0);
        self.turns.iter_mut().for_each(|t| *t = Quat::IDENTITY);
        self.remainder = 0.0;
        self.previous = None;
        self.speeds.iter_mut().for_each(|s| *s = 0.0);
        self.peaks.iter_mut().for_each(|s| *s = 0.0);
        self.drawn.iter_mut().for_each(|d| *d = None);
        self.body = 0.0;
        self.body_speed = 0.0;
    }

    /// The body's lift this frame (a heavy body's spring, else the lowering to the lowest limb).
    pub fn body_lift(&self) -> f32 {
        self.body
    }

    /// The limbs' lifts this frame.
    pub fn lifts(&self) -> &[f32] {
        &self.offsets
    }

    /// A heavy body's filter (`Weight`), in steps of at most 1/120 s.
    fn weigh(&mut self, w: &Weight, desired: &[f32], supported: &[bool], dt: f32) {
        let steps = (dt * 120.0).ceil().max(1.0);
        let h = dt / steps;
        for _ in 0..steps as usize {
            let mut settle = 0.0;
            for i in 0..self.offsets.len() {
                if !supported[i] {
                    continue;
                }
                let gap = desired[i] - self.offsets[i];
                let rate = if gap > 0.0 { w.rise } else { w.fall };
                self.speeds[i] += (rate * rate * gap - 2.0 * rate * self.speeds[i]) * h;
                self.offsets[i] += self.speeds[i] * h;
                // A rise ends when the limb has slowed to a fifth of its fastest: it has landed.
                if self.speeds[i] > self.peaks[i] {
                    self.peaks[i] = self.speeds[i];
                } else if self.peaks[i] > 0.5 && self.speeds[i] < self.peaks[i] * 0.2 {
                    settle += self.peaks[i];
                    self.peaks[i] = 0.0;
                }
            }
            let lowest = self.offsets.iter().copied().fold(0.0_f32, f32::min);
            let high = self
                .offsets
                .iter()
                .zip(supported)
                .filter(|(_, s)| **s)
                .map(|(o, _)| o.max(0.0))
                .fold(0.0_f32, f32::max);
            let target = lowest + w.heave * high;
            self.body_speed -= w.settle * settle;
            self.body_speed += (w.body_rate * w.body_rate * (target - self.body)
                - 2.0 * w.body_damping * w.body_rate * self.body_speed)
                * h;
            self.body += self.body_speed * h;
        }
    }

    /// settles: 0073db50's lowering gate (enabled, the composed local_428 > 0.95, endpoint travel <= dt*0.3 + 0.001),
    /// which the whole-body lowering here is broader than (`docs/foot-ik.md`).
    /// [assumed] Preserve animated lift and adapt it to local terrain; whole-body
    /// visual lowering prevents extending the downhill leg. Never changes movement.
    /// `root` maps the object's z-up space to the world's z-up space.
    pub fn apply(
        &mut self,
        rig: &FootRig,
        local: &mut [Affine3A],
        parents: &[Option<usize>],
        root: Affine3A,
        active: bool,
        dt: f32,
        ground: &impl Sight,
    ) {
        if !active || !dt.is_finite() || dt <= 0.0 {
            self.reset();
            return;
        }
        let count = rig.legs.len();
        self.offsets.resize(count, 0.0);
        self.turns.resize(count, Quat::IDENTITY);
        let origin: Vec3 = root.translation.into();
        // settles: 00740d60's and 0073db50's reset / teleport rule (unresolved in `docs/foot-ik.md`).
        // [assumed] A move larger than the probe diameter in one frame is a teleport.
        if self
            .previous
            .is_some_and(|p| p.distance(origin) > rig.probe * 2.0)
        {
            self.reset();
        }
        self.previous = Some(origin);
        let inverse = root.inverse();
        let world = accumulate(local, parents);
        let mut desired = vec![0.0; count];
        let mut turns = vec![Quat::IDENTITY; count];
        let mut supported = vec![false; count];
        self.speeds.resize(count, 0.0);
        self.peaks.resize(count, 0.0);
        self.drawn.resize(count, None);
        for (i, leg) in rig.legs.iter().enumerate() {
            let ankle = root * world[leg.foot];
            let drawn: Vec3 = ankle.translation.into();
            let was = self.drawn[i].replace(drawn);
            let Some((lift, turn)) = limb_lift(rig, leg, ankle, origin, ground) else {
                continue;
            };
            desired[i] = lift;
            // A heavy body's swinging limb also takes the ground where it is going (`Weight`).
            if let (Some(w), Some(was)) = (rig.weight, was) {
                let ahead = ((drawn - was).truncate() / dt * w.lead).clamp_length_max(w.lead_reach);
                if ahead.length() > 0.05 {
                    let there = Affine3A::from_translation(ahead.extend(0.0)) * ankle;
                    if let Some((lift, _)) = limb_lift(rig, leg, there, origin, ground) {
                        desired[i] = desired[i].max(lift);
                    }
                }
            }
            turns[i] = inverse.to_scale_rotation_translation().1
                * turn
                * root.to_scale_rotation_translation().1;
            supported[i] = true;
        }
        if let Some(w) = rig.weight {
            self.weigh(&w, &desired, &supported, dt);
        } else {
            // settles: the dt that 00740d60 passes to 0073db50 (the frame dt or the 0.032 s step): the call site, not read.
            // [assumed] Apply the game's half-per-update filter at the simulation cadence,
            // independent of display refresh. Angular dt*24 is capped to avoid overshoot.
            self.remainder += dt;
            while self.remainder >= UPDATE {
                self.remainder -= UPDATE;
                for i in 0..count {
                    self.offsets[i] += (desired[i] - self.offsets[i]) * 0.5;
                }
                // [assumed port design] The body follows its lowest limb down at the limbs' own
                // halving, and back up at `BODY_RISE` an update: going up a slope the lowest foot is
                // the one behind, which changes every stride, so set straight to it the body jumped
                // 0.2 to 0.4 m twice a stride on long legs (Breakaway, Starscream; the user's report,
                // 2026-10-25). The game, as read, lowers no body (`docs/foot-ik.md`).
                let target = self.offsets.iter().copied().fold(0.0_f32, f32::min);
                self.body += (target - self.body) * if target < self.body { 0.5 } else { BODY_RISE };
            }
        }
        for (i, turn) in turns.iter().enumerate() {
            self.turns[i] = self.turns[i].slerp(*turn, (dt * TURN_RATE).min(1.0));
            // settles: 0073db50's lost-probe branch (the callback / filter path is unresolved: `docs/foot-ik.md`).
            // [assumed] A lost probe (a foot swung more than the probe's 1.5 m over the ground) lets
            // its lift go back to none by the same halving as any other (`desired` is 0 for it), its
            // sole turning back level: released at once, a downhill foot's -0.35 to -0.44 m came off
            // in one update as it lifted, and the body's lowering with it, so long-legged robots'
            // legs jumped on every stride up a slope (Breakaway, Starscream; the user's report,
            // 2026-10-25, `core\examples\foot_ik_ramp.rs`). A heavy body's limb is still released.
            if !supported[i] && rig.weight.is_some() {
                self.offsets[i] = 0.0;
                self.turns[i] = Quat::IDENTITY;
                self.speeds[i] = 0.0;
                self.peaks[i] = 0.0;
            }
        }
        if !supported.iter().any(|&s| s) {
            self.reset();
            return;
        }
        let lower = self.body;
        let body_parent = parents[rig.body].map_or(Affine3A::IDENTITY, |p| world[p]);
        local[rig.body].translation +=
            glam::Vec3A::from(body_parent.inverse().transform_vector3(Vec3::Z * lower));
        for (i, leg) in rig.legs.iter().enumerate() {
            // A released foot still easing back is solved to its easing lift (above).
            if !supported[i] && (rig.weight.is_some() || self.offsets[i].abs() < 1e-4) {
                continue;
            }
            let (_, rotation, position) = world[leg.foot].to_scale_rotation_translation();
            apply_leg(
                local,
                parents,
                leg,
                position + Vec3::Z * self.offsets[i],
                self.turns[i] * rotation,
            );
        }
    }
}

#[cfg(test)]
mod roster_survey {
    /// Survey (run by hand): which playable packs attach a `FootIK` (or `SpineIK`) to an object.
    #[test]
    #[ignore]
    fn survey_roster_foot_ik_attachments() {
        use std::collections::HashSet;
        use crate::formats::lxb::Node;
        fn walk(node: Node, depth: u32, seen: &mut HashSet<usize>, foot: &mut usize, spine: &mut usize) {
            if depth > 60 { return; }
            if let Some(at) = node.offset() && !seen.insert(at) { return; }
            match node.type_name() { Some("FootIK") => *foot += 1, Some("SpineIK") => *spine += 1, _ => {} }
            for (_, child) in node.fields() { walk(child, depth + 1, seen, foot, spine); }
            if !node.is_empty() { for child in node.items() { walk(child, depth + 1, seen, foot, spine); } }
        }
        for pack in ["bumblebee", "optimus", "ratchet", "ironhide", "megatron", "longhaul", "audir8", "jetfire", "firestorm", "starscream", "blackout", "optimusflight", "megatronflight"] {
            let Ok(p) = crate::formats::pack::Pack::open(&crate::install::dir().join("characters").join(format!("{pack}.str"))) else { continue };
            let (mut foot, mut spine) = (0, 0);
            for chunk in p.of_type(crate::formats::pack::DATA) {
                let file = crate::formats::lxb::DataFile::parse(p.data(chunk).to_vec()).unwrap();
                walk(file.root(), 0, &mut HashSet::new(), &mut foot, &mut spine);
            }
            println!("{pack}: FootIK {foot}, SpineIK {spine}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        character,
        pose::Rig,
        sim::{Arena, Box3},
        terrain::Ramp,
    };
    use glam::Vec2;
    use std::sync::OnceLock;

    fn data() -> &'static character::CharacterData {
        static DATA: OnceLock<character::CharacterData> = OnceLock::new();
        DATA.get_or_init(|| character::load(crate::install::dir(), "bumblebee").unwrap())
    }
    fn fixture() -> (FootRig, Vec<Affine3A>, Vec<Option<usize>>) {
        let data = data();
        let rig =
            FootRig::read(&data.robot, &data.library).expect("both rigid foot meshes and chains");
        let parents: Vec<_> = data.robot.nodes.iter().map(|n| n.parent).collect();
        let rest = Rig::new(&data.robot).rest();
        let local = rest
            .iter()
            .enumerate()
            .map(|(i, &w)| parents[i].map_or(w, |p| rest[p].inverse() * w))
            .collect();
        (rig, local, parents)
    }
    fn settled(
        rig: &FootRig,
        base: &[Affine3A],
        parents: &[Option<usize>],
        root: Affine3A,
        arena: &Arena,
        dt: f32,
    ) -> Vec<Affine3A> {
        let mut ik = FootIk::default();
        let mut pose = base.to_vec();
        for _ in 0..(1.6 / dt) as usize {
            pose.clone_from_slice(base);
            ik.apply(rig, &mut pose, parents, root, true, dt, arena);
        }
        pose
    }
    fn assert_lengths(
        rig: &FootRig,
        base: &[Affine3A],
        actual: &[Affine3A],
        parents: &[Option<usize>],
    ) {
        let before = accumulate(base, parents);
        let after = accumulate(actual, parents);
        for leg in &rig.legs {
            for (a, b) in [(leg.hip, leg.knee), (leg.knee, leg.foot)] {
                let len =
                    |w: &[Affine3A]| Vec3::from(w[a].translation).distance(w[b].translation.into());
                assert!((len(&before) - len(&after)).abs() < 1e-4);
            }
        }
        assert!(after.iter().all(|p| p.is_finite()));
    }
    #[test]
    fn cosine_solver_preserves_lengths_and_handles_unreachable_and_folded_targets() {
        let (h, k, f) = (
            Vec3::ZERO,
            Vec3::new(0.0, 1.0, -1.0),
            Vec3::new(0.0, 0.0, -2.0),
        );
        for target in [Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 0.0, -20.0), h] {
            let (k2, f2) = solve_knee(h, k, f, target).unwrap();
            assert!((h.distance(k2) - h.distance(k)).abs() < 1e-4);
            assert!((k2.distance(f2) - k.distance(f)).abs() < 1e-4);
            assert!(k2.is_finite() && f2.is_finite());
        }
        assert!(solve_knee(h, h, f, f).is_none());
    }
    #[test]
    fn real_feet_clear_slopes_and_banks_at_different_character_yaws() {
        if crate::install::missing() { return; }
        let (rig, base, parents) = fixture();
        for slope in [
            Vec2::new(0.0, 0.28),
            Vec2::new(0.35, 0.0),
            Vec2::new(-0.2, 0.2),
        ] {
            let ramp = Ramp {
                min: Vec2::splat(-10.0),
                max: Vec2::splat(10.0),
                base: 0.0,
                height: 8.0,
                slope,
            };
            let arena = Arena {
                ramps: vec![ramp],
                ..Default::default()
            };
            for yaw in [0.0, 0.8, 2.4] {
                let root = Affine3A::from_rotation_translation(
                    Quat::from_rotation_z(yaw),
                    Vec3::new(0.0, 0.0, ramp.top(Vec2::ZERO)),
                );
                let actual = settled(&rig, &base, &parents, root, &arena, 0.016);
                let world = accumulate(&actual, &parents);
                for leg in &rig.legs {
                    let lowest = leg
                        .sole
                        .iter()
                        .map(|p| (root * world[leg.foot]).transform_point3(*p))
                        .map(|p| p.z - ramp.top(p.truncate()))
                        .fold(f32::INFINITY, f32::min);
                    assert!(
                        (lowest - BIAS).abs() < 0.06,
                        "slope={slope:?}, yaw={yaw}, clearance={lowest}"
                    );
                }
                assert_lengths(&rig, &base, &actual, &parents);
            }
        }
    }
    #[test]
    fn uneven_support_solves_each_leg_without_stretching() {
        if crate::install::missing() { return; }
        let (rig, base, parents) = fixture();
        let arena = Arena {
            boxes: vec![Box3 {
                min: Vec3::new(0.0, -5.0, 0.0),
                max: Vec3::new(5.0, 5.0, 0.4),
            }],
            ..Default::default()
        };
        let actual = settled(&rig, &base, &parents, Affine3A::IDENTITY, &arena, 0.016);
        let world = accumulate(&actual, &parents);
        for (i, leg) in rig.legs.iter().enumerate() {
            let z = leg
                .sole
                .iter()
                .map(|p| world[leg.foot].transform_point3(*p).z)
                .fold(f32::INFINITY, f32::min);
            let expected = if i == 0 { 0.0 } else { 0.4 };
            assert!((z - expected - BIAS).abs() < 0.04, "foot={i}, z={z}");
        }
        assert_lengths(&rig, &base, &actual, &parents);
    }
    #[test]
    fn inactive_and_missing_support_leave_animation_unchanged() {
        if crate::install::missing() { return; }
        let (rig, base, parents) = fixture();
        let mut ik = FootIk::default();
        for active in [true, false] {
            let mut pose = base.clone();
            ik.apply(
                &rig,
                &mut pose,
                &parents,
                Affine3A::IDENTITY,
                active,
                0.032,
                &crate::camera::Open,
            );
            assert_eq!(pose, base);
        }
        let mut pose = base.clone();
        ik.apply(
            &rig,
            &mut pose,
            &parents,
            Affine3A::IDENTITY,
            true,
            0.032,
            &Arena::default(),
        );
        pose.clone_from_slice(&base);
        ik.apply(
            &rig,
            &mut pose,
            &parents,
            Affine3A::IDENTITY,
            false,
            0.032,
            &Arena::default(),
        );
        assert_eq!(pose, base);
    }
    #[test]
    fn flat_support_preserves_pose_except_contact_bias_and_render_cadence_agrees() {
        if crate::install::missing() { return; }
        let (rig, base, parents) = fixture();
        let slow = settled(
            &rig,
            &base,
            &parents,
            Affine3A::IDENTITY,
            &Arena::default(),
            0.032,
        );
        let fast = settled(
            &rig,
            &base,
            &parents,
            Affine3A::IDENTITY,
            &Arena::default(),
            0.008,
        );
        let before = accumulate(&base, &parents);
        let a = accumulate(&slow, &parents);
        let b = accumulate(&fast, &parents);
        for leg in &rig.legs {
            assert!(
                Vec3::from(a[leg.foot].translation).distance(b[leg.foot].translation.into()) < 1e-4
            );
            assert!(
                (a[leg.foot].translation.z - before[leg.foot].translation.z - BIAS).abs() < 1e-4
            );
        }
    }
    #[test]
    fn raised_toe_corner_at_a_ramp_join_is_not_missed_by_the_centre_probe() {
        if crate::install::missing() { return; }
        let (rig, base, parents) = fixture();
        let rest = accumulate(&base, &parents);
        let points = rig.legs[0]
            .sole
            .map(|p| rest[rig.legs[0].foot].transform_point3(p));
        let low = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let high = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
        let seam = low + (high - low) * 0.75;
        let ramp = Ramp {
            min: Vec2::new(-5.0, seam),
            max: Vec2::new(5.0, 10.0),
            base: 0.0,
            height: 0.0,
            slope: Vec2::new(0.0, 0.28),
        };
        let arena = Arena {
            ramps: vec![ramp],
            ..Default::default()
        };
        let actual = settled(&rig, &base, &parents, Affine3A::IDENTITY, &arena, 0.016);
        let world = accumulate(&actual, &parents);
        for leg in &rig.legs {
            for p in leg.sole {
                let p = world[leg.foot].transform_point3(p);
                let height = if ramp.contains(p.truncate()) {
                    ramp.top(p.truncate())
                } else {
                    0.0
                };
                assert!(
                    p.z - height >= -0.001,
                    "sole corner penetrates join: {p:?}, surface={height}"
                );
            }
        }
        assert_lengths(&rig, &base, &actual, &parents);
    }
    #[test]
    fn walking_retains_animated_swing_height_and_toe_keys() {
        if crate::install::missing() { return; }
        let (rig, _, parents) = fixture();
        let data = data();
        let clip = data
            .animations
            .find("WalkSet", "Walk")
            .unwrap()
            .clip
            .as_ref()
            .unwrap();
        let pose_rig = Rig::new(&data.robot);
        let toe = data
            .robot
            .nodes
            .iter()
            .position(|n| n.name == "Midfoot_L")
            .unwrap();
        for tick in [0.0, 10.0, 25.0, 40.0, 55.0] {
            let world = pose_rig.pose(clip, true, tick);
            let base: Vec<_> = world
                .iter()
                .enumerate()
                .map(|(i, &w)| parents[i].map_or(w, |p| world[p].inverse() * w))
                .collect();
            let actual = settled(
                &rig,
                &base,
                &parents,
                Affine3A::IDENTITY,
                &Arena::default(),
                0.016,
            );
            let result = accumulate(&actual, &parents);
            for leg in &rig.legs {
                assert!(
                    (result[leg.foot].translation.z - world[leg.foot].translation.z - BIAS).abs()
                        < 0.005,
                    "tick={tick}, foot={}",
                    leg.foot
                );
            }
            assert_eq!(actual[toe], base[toe]);
            assert_lengths(&rig, &base, &actual, &parents);
        }
    }
    #[test]
    fn teleport_and_lost_probe_do_not_carry_previous_slope_offsets() {
        if crate::install::missing() { return; }
        let (rig, base, parents) = fixture();
        let ramp = Ramp {
            min: Vec2::splat(-10.0),
            max: Vec2::splat(10.0),
            base: 0.0,
            height: 8.0,
            slope: Vec2::new(0.3, 0.0),
        };
        let arena = Arena {
            ramps: vec![ramp],
            ..Default::default()
        };
        let mut ik = FootIk::default();
        let root = Affine3A::from_translation(Vec3::new(0.0, 0.0, 11.0));
        let mut pose = base.clone();
        for _ in 0..20 {
            pose.clone_from_slice(&base);
            ik.apply(&rig, &mut pose, &parents, root, true, 0.032, &arena);
        }
        assert!(ik.offsets[0] < 0.0);
        pose.clone_from_slice(&base);
        ik.apply(
            &rig,
            &mut pose,
            &parents,
            Affine3A::from_translation(Vec3::new(100.0, 0.0, 0.0)),
            true,
            0.032,
            &arena,
        );
        assert!((ik.offsets[0] - BIAS * 0.5).abs() < 1e-5);
        pose.clone_from_slice(&base);
        ik.apply(
            &rig,
            &mut pose,
            &parents,
            root,
            true,
            0.032,
            &crate::camera::Open,
        );
        assert_eq!(pose, base);
        assert_eq!(ik.offsets, vec![0.0; 2]);
    }

    /// [assumed port design] A foot that swings out of the probe's reach eases its lift back by the
    /// halving, and the body comes back up after it at `BODY_RISE`: neither jumps in one update
    /// (Breakaway's and Starscream's legs on the arena's wedges, the user's report, 2026-10-25).
    #[test]
    fn a_foot_lifted_off_a_slope_does_not_snap_the_body() {
        if crate::install::missing() { return; }
        let (rig, base, parents) = fixture();
        let ramp = Ramp { min: Vec2::splat(-10.0), max: Vec2::splat(10.0), base: 0.0, height: 8.0, slope: Vec2::new(0.3, 0.0) };
        let arena = Arena { ramps: vec![ramp], ..Default::default() };
        let mut ik = FootIk::default();
        let root = Affine3A::from_translation(Vec3::new(0.0, 0.0, 11.0));
        let mut pose = base.clone();
        for _ in 0..20 {
            pose.clone_from_slice(&base);
            ik.apply(&rig, &mut pose, &parents, root, true, 0.032, &arena);
        }
        let body = ik.body_lift();
        let lowest = ik.offsets.iter().copied().fold(0.0_f32, f32::min);
        assert!(body < -0.05 && (body - lowest).abs() < 0.01, "settled: body {body}, lowest foot {lowest}");
        // The lower foot swings 3 m up, out of the probe's 1.5 m.
        let low = if ik.offsets[0] < ik.offsets[1] { 0 } else { 1 };
        let world = accumulate(&base, &parents);
        let parent = parents[rig.legs[low].foot].map_or(Affine3A::IDENTITY, |p| world[p]);
        let mut lifted = base.clone();
        lifted[rig.legs[low].foot].translation += glam::Vec3A::from(parent.inverse().transform_vector3(Vec3::Z * 3.0));
        let was = ik.offsets[low];
        pose.clone_from_slice(&lifted);
        ik.apply(&rig, &mut pose, &parents, root, true, 0.032, &arena);
        assert!((ik.offsets[low] - was * 0.5).abs() < 1e-4, "eased, not released: {} from {was}", ik.offsets[low]);
        assert!(ik.body_lift() <= body * (1.0 - BODY_RISE) + 1e-4, "the body rose {} from {body}", ik.body_lift());
    }
}
