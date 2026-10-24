//! The `ShockWave` object: each Trigger starts a ring at the object that grows at `speed`,
//! hurts what it passes over (each thing once while any ring of the object lives), fades in
//! and out by its radius, and is drawn as a flat band of quads.
//!
//! [game] read from the class's code: create `FUN_008053c0` (constructor `FUN_00804e00`,
//! loader `FUN_00804f70`), the Trigger `FUN_00805ed0` (vtable `00b61448` slot 6) with the ring
//! set-up `FUN_00806190`, the update `FUN_00805410` (slot 2), the ring's removal
//! `FUN_00806030`, and the drawing `FUN_00806ac0` / `FUN_00806380`. The target query is
//! `target_candidates` (`FUN_00799cd0`) with its test `FUN_007996c0`. Notes:
//! `notes\effects-shockwave-tube.md`.
//!
//! Users in the data: Devastator's two in `cairo_city_a_boss` (damage 25, drawn), the
//! power core's in `shanghai_03_waterfront_a_core`, others in the Cairo and Deep Six missions,
//! and one each in `megatronflight` / `optimusflight` (no material and no damage: they do
//! nothing a player sees).
//!
//! Beyond the game [port design]: a ring may be limited to an `Arc` (it then hurts and draws
//! only inside it) and the object may carry a `Falloff` (damage and push weaken as the ring
//! runs). Neither is in the game's data; with neither set a ring is the game's. They are for
//! Grindor's cinematic wave (`energon_wave.rs`, `notes\grindor-cinematic-wave.md`).

use crate::damage::{Actor, Packet};
use crate::formats::lxb::Node;
use glam::{Vec2, Vec3};

/// `cos 45` as the code writes it for the push (`0.70714`, not `0.70710677`). [game]
const PUSH_TILT: f32 = 0.70714;
/// The drawing's normal tilt (`0.70710677`). [game]
const NORMAL_TILT: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// How many things one ring may hurt in one update: the query's list has four slots. [game]
pub const MAX_TARGETS: usize = 4;
/// Height above the ground probe's hit that a ring is drawn at. [game]
pub const GROUND_LIFT: f32 = 0.15;
/// The ground probe: a sweep from 1 m above the object to 2 m below, radius 0.5. [game]
pub const PROBE_UP: f32 = 1.0;
pub const PROBE_DOWN: f32 = 2.0;
pub const PROBE_RADIUS: f32 = 0.5;

/// `specialCaseDamageStuff` (`FUN_00744f10`): the angle is stored in radians. [game]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpecialCase {
    pub stun_time: f32,
    pub slow_time: f32,
    pub dot_time: f32,
    pub heat: f32,
    pub knock_back_speed: f32,
    pub knock_back_angle: f32,
}

impl SpecialCase {
    pub fn read(node: Option<Node>) -> Self {
        let f = |name: &str| node.and_then(|n| n.get(name)).and_then(Node::float).unwrap_or(0.0);
        Self {
            stun_time: f("stunTime"),
            slow_time: f("slowTime"),
            dot_time: f("dotTime"),
            heat: f("heatIncrement"),
            knock_back_speed: f("knockBackSpeed"),
            knock_back_angle: f("knockBackAngle").to_radians(),
        }
    }
}

/// A `ShockWave`'s data (`FUN_00804f70`). The defaults are the loader's. [game]
#[derive(Clone, Debug, PartialEq)]
pub struct Def {
    pub name: u32,
    /// The material's asset id; none: the rings are not drawn (they still hurt).
    pub material: Option<u32>,
    /// `heightCheck` (+0x6c, 1): a thing more than this above or below the ring is left
    /// alone; a negative value turns the test off.
    pub height_check: f32,
    /// `startRadius` (+0x48, 0.5): a new ring's radius.
    pub start_radius: f32,
    /// `fadeInRadius` (+0x4c, 1), `fadeOutRadius` (+0x50, 10), `fullRadius` (+0x54, 15): the
    /// ring is see-through from start to fade-in, fades from... see `alpha`; it is removed at
    /// `fullRadius`.
    pub fade_in_radius: f32,
    pub fade_out_radius: f32,
    pub full_radius: f32,
    /// `ringRadius` (+0x58, 1) and `fullRingRadius` (+0x5c, 2): the band's width at the
    /// start and at the full radius.
    pub ring_radius: f32,
    pub full_ring_radius: f32,
    /// `speed` (+0x60, 7.5): metres a second.
    pub speed: f32,
    /// `startSegments` (+0x64, 10) and `fullSegments` (+0x68, 20): quads around the ring.
    pub start_segments: i32,
    pub full_segments: i32,
    /// `damage_type` (+0x70), `..._on_small_chars` (+0x74), `..._on_large_chars` (+0x78): all
    /// 0x40800 (`BY_EXPLOSION | BY_KNOCKBACK`) when absent; an empty set reads 0.
    pub damage_type: u32,
    pub damage_type_small: u32,
    pub damage_type_large: u32,
    /// +0x80 .. +0x94.
    pub special: SpecialCase,
    /// `damage` (+0x7c, 10).
    pub damage: f32,
}

impl Default for Def {
    fn default() -> Self {
        Self {
            name: 0,
            material: None,
            height_check: 1.0,
            start_radius: 0.5,
            fade_in_radius: 1.0,
            fade_out_radius: 10.0,
            full_radius: 15.0,
            ring_radius: 1.0,
            full_ring_radius: 2.0,
            speed: 7.5,
            start_segments: 10,
            full_segments: 20,
            damage_type: 0x40800,
            damage_type_small: 0x40800,
            damage_type_large: 0x40800,
            special: SpecialCase::default(),
            damage: 10.0,
        }
    }
}

pub fn read(node: Node) -> Def {
    let d = Def::default();
    let float = |name: &str, or: f32| node.get(name).and_then(Node::float).unwrap_or(or);
    let int = |name: &str, or: i64| node.get(name).and_then(Node::int).unwrap_or(or);
    Def {
        name: int("LuxScriptName", 0) as u32,
        material: node.get("material").and_then(Node::asset_text).map(|(id, _)| id),
        height_check: float("heightCheck", d.height_check),
        start_radius: float("startRadius", d.start_radius),
        fade_in_radius: float("fadeInRadius", d.fade_in_radius),
        fade_out_radius: float("fadeOutRadius", d.fade_out_radius),
        full_radius: float("fullRadius", d.full_radius),
        ring_radius: float("ringRadius", d.ring_radius),
        full_ring_radius: float("fullRingRadius", d.full_ring_radius),
        speed: float("speed", d.speed),
        start_segments: int("startSegments", d.start_segments as i64) as i32,
        full_segments: int("fullSegments", d.full_segments as i64) as i32,
        damage_type: int("damage_type", d.damage_type as i64) as u32,
        damage_type_small: int("damage_type_on_small_chars", d.damage_type_small as i64) as u32,
        damage_type_large: int("damage_type_on_large_chars", d.damage_type_large as i64) as u32,
        special: SpecialCase::read(node.get("specialCaseDamageStuff")),
        damage: float("damage", d.damage),
    }
}

/// One ring (`FUN_00806190`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    /// Where the object was when it was triggered; once the ground probe hits, z is the hit
    /// plus `GROUND_LIFT` (the drawing writes it back into the ring, so the damage's height
    /// test uses it too).
    pub centre: Vec3,
    /// The radius (ring `+0xdc`).
    pub radius: f32,
    /// The game time it was started (ring `+0xd0`), carried in its damage messages.
    pub start_time: f32,
    /// [port design] Only this part of the ring is there; none: the whole ring (the game's).
    pub arc: Option<Arc>,
}

/// [port design] Part of a ring: the angles within `half_angle` of `heading`, a flat unit
/// direction from the ring's centre. Not in the game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    pub heading: Vec2,
    pub half_angle: f32,
}

impl Arc {
    /// Around the flat part of `way`; none when `way` has no flat part.
    pub fn new(way: Vec3, half_angle: f32) -> Option<Self> {
        let flat = way.truncate();
        (flat.length() > 1e-6).then(|| Self { heading: flat.normalize(), half_angle })
    }

    /// The heading as the drawing measures angles: a point at angle a lies at (sin a, cos a).
    pub fn angle(&self) -> f32 {
        self.heading.x.atan2(self.heading.y)
    }

    /// Whether a thing at flat offset `off` from the centre, `radius` wide, is inside: its
    /// angle off the heading, less the angle its radius covers at that distance.
    pub fn contains(&self, off: Vec2, radius: f32) -> bool {
        let distance = off.length();
        if distance <= radius {
            return true;
        }
        let off_heading = self.heading.angle_to(off).abs();
        let covers = (radius / distance).clamp(0.0, 1.0).asin();
        off_heading - covers <= self.half_angle
    }
}

/// [port design] Damage and push by how far a ring has run (`t`, 0 at `startRadius` and 1 at
/// `fullRadius`): full up to `full_until`, then straight down to `end_scale` at the end. Not in
/// the game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Falloff {
    pub full_until: f32,
    pub end_scale: f32,
}

impl Falloff {
    pub fn scale(&self, t: f32) -> f32 {
        if t <= self.full_until || self.full_until >= 1.0 {
            return 1.0;
        }
        let s = ((t - self.full_until) / (1.0 - self.full_until)).min(1.0);
        1.0 + (self.end_scale - 1.0) * s
    }
}

/// A character's size class (`character +0x140 -> +0x78`): 0 small, 1 large. [game]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Small,
    Large,
}

/// What the query may return (`target_candidates` reads the object's place, `+0x34`, and
/// its radius, record `+0x14`).
#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub id: u32,
    pub position: Vec3,
    pub radius: f32,
    /// Some for a character.
    pub size: Option<Size>,
}

/// One damage message a ring sends (`send_damage_message` `FUN_00724700`).
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub target: u32,
    /// The ring's centre: where the message says the damage came from.
    pub origin: Vec3,
    pub start_time: f32,
    /// Type, amount, special case and the push direction: flat from the ring's centre to
    /// the thing, tilted up 45 degrees.
    pub packet: Packet,
    /// The direction times 10000 when the damage is over 0, else nothing. [game]
    pub force: Vec3,
}

/// A vertex of the drawn band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: Vec2,
    /// White, alpha the ring's fade.
    pub alpha: f32,
}

#[derive(Clone, Debug, Default)]
pub struct ShockWave {
    pub def: Def,
    pub rings: Vec<Ring>,
    /// Who any living ring has hurt (`+0xa4`), cleared when the last ring goes.
    hurt: Vec<u32>,
    /// [port design] Weakens damage and push as a ring runs; none: the game's, all the way.
    pub falloff: Option<Falloff>,
}

impl ShockWave {
    pub fn new(def: Def) -> Self {
        Self { def, rings: Vec::new(), hurt: Vec::new(), falloff: None }
    }

    /// The Trigger: a new ring at the object's place. The caller sweeps the ground probe
    /// (`PROBE_*`) and passes what it hit to `ground`.
    pub fn trigger(&mut self, position: Vec3, time: f32) {
        self.rings.push(Ring { centre: position, radius: self.def.start_radius, start_time: time, arc: None });
    }

    /// [port design] A Trigger whose ring is only `arc`.
    pub fn trigger_arc(&mut self, position: Vec3, time: f32, arc: Arc) {
        self.rings.push(Ring { centre: position, radius: self.def.start_radius, start_time: time, arc: Some(arc) });
    }

    /// How far a ring of radius `r` has run, 0 at `startRadius` and 1 at `fullRadius`.
    fn run(&self, r: f32) -> f32 {
        let d = &self.def;
        (r - d.start_radius) / (d.full_radius - d.start_radius)
    }

    /// [port design] The share of the damage and push a ring of radius `r` deals (1 without a
    /// `Falloff`).
    pub fn strength(&self, r: f32) -> f32 {
        self.falloff.map_or(1.0, |f| f.scale(self.run(r)))
    }

    /// The ground probe of ring `i` hit at height `z`.
    pub fn ground(&mut self, i: usize, z: f32) {
        if let Some(ring) = self.rings.get_mut(i) {
            ring.centre.z = z + GROUND_LIFT;
        }
    }

    pub fn active(&self) -> bool {
        !self.rings.is_empty()
    }

    /// The band of a ring of radius `r`: inner and outer edge. The width runs from
    /// `ringRadius` at `startRadius` to `fullRingRadius` at `fullRadius`, not clamped. [game]
    pub fn band(&self, r: f32) -> (f32, f32) {
        let d = &self.def;
        let t = (r - d.start_radius) / (d.full_radius - d.start_radius);
        let half = ((d.full_ring_radius - d.ring_radius) * t + d.ring_radius) * 0.5;
        (r - half, r + half)
    }

    /// One update (`FUN_00805410`). `owner` is the object's owner (the damage's source),
    /// `this` the object itself (never hurt). `deliver` sends a message and says whether the
    /// thing took it; only those are kept from being hurt again.
    pub fn update(
        &mut self,
        dt: f32,
        this: u32,
        owner: Option<Actor>,
        candidates: &[Candidate],
        mut deliver: impl FnMut(&Hit) -> bool,
    ) {
        let d = self.def.clone();
        let mut i = 0;
        while i < self.rings.len() {
            self.rings[i].radius += d.speed * dt;
            let ring = self.rings[i];
            if ring.radius >= d.full_radius {
                self.rings.remove(i);
                continue;
            }
            i += 1;
            if !(d.damage > 0.0 || d.damage_type != 0) {
                continue;
            }
            for c in self.query(&ring, candidates) {
                if c.id == this || self.hurt.contains(&c.id) {
                    continue;
                }
                if d.height_check >= 0.0 && (c.position.z - ring.centre.z).abs() >= d.height_check {
                    continue;
                }
                let flat = (c.position - ring.centre).truncate();
                let flat = if flat.length() > 1e-8 { flat / flat.length() } else { flat };
                let direction = Vec3::new(flat.x, flat.y, 1.0) * PUSH_TILT;
                let flags = match c.size {
                    Some(Size::Small) if d.damage_type_small != 0 => d.damage_type_small,
                    Some(Size::Large) if d.damage_type_large != 0 => d.damage_type_large,
                    _ => d.damage_type,
                };
                let strength = self.strength(ring.radius);
                let hit = Hit {
                    target: c.id,
                    origin: ring.centre,
                    start_time: ring.start_time,
                    packet: Packet {
                        source: owner,
                        amount: d.damage * strength,
                        flags,
                        direction,
                        knockback_speed: d.special.knock_back_speed * strength,
                        knockback_angle: d.special.knock_back_angle,
                        stun_time: d.special.stun_time,
                        slow_time: d.special.slow_time,
                        dot_time: d.special.dot_time,
                        heat: d.special.heat,
                        headshot: false,
                    },
                    force: if d.damage > 0.0 { direction * 10000.0 * strength } else { Vec3::ZERO },
                };
                if deliver(&hit) {
                    self.hurt.push(c.id);
                }
            }
        }
        if self.rings.is_empty() {
            self.hurt.clear();
        }
    }

    /// The query a ring makes (`FUN_007996c0` with the ring's values): a thing whose flat
    /// distance less its own radius, taken absolutely, is inside the band, and whose height
    /// off the ring is under the ring's radius. The yaw and pitch limits are pi: any way.
    /// At most `MAX_TARGETS`. The shockwave's update calls `target_candidates` with max 4 [game
    /// 00805410]; `target_candidates` [game 00799cd0 l.5-79] walks the candidate list in its
    /// order and keeps the first ones whose score is under 1000, stopping when the capacity is
    /// reached. So the kept ones are the first in the candidates' order, not the nearest. The
    /// score is `|distance - ring radius|` [game 007996c0 l.27 fabs of (dist - ESI[7])]; the
    /// query's ESI[7] is the ring's current radius, stored by the update [game 00805410 l.44-50:
    /// `local_4c = local_48 = fVar19`, the radius]. The under-1000 test is kept.
    /// [port design] A ring with an `Arc` also leaves out what is outside it, before the four
    /// are taken, so things behind the arc do not use up its slots.
    fn query<'a>(&self, ring: &Ring, candidates: &'a [Candidate]) -> Vec<&'a Candidate> {
        let (inner, outer) = self.band(ring.radius);
        candidates
            .iter()
            .filter(|c| {
                let off = c.position - ring.centre;
                let distance = (off.truncate().length() - c.radius).abs();
                let score = (distance - ring.radius).abs();
                inner < distance && distance < outer && off.z.abs() < ring.radius && score < 1000.0
                    && ring.arc.is_none_or(|arc| arc.contains(off.truncate(), c.radius))
            })
            .take(MAX_TARGETS)
            .collect()
    }

    /// The ring's alpha (`FUN_00806380`): the lesser of the fade in, from `startRadius` to
    /// `fadeInRadius`, and the fade out, from `fadeOutRadius` to `fullRadius`; each is 1
    /// when its two radii are the wrong way round. [game]
    pub fn alpha(&self, ring: &Ring) -> f32 {
        let d = &self.def;
        let r = ring.radius;
        let out = if d.full_radius <= d.fade_out_radius {
            1.0
        } else {
            ((r - d.full_radius) / (d.fade_out_radius - d.full_radius)).min(1.0)
        };
        let fade_in = if d.fade_in_radius <= d.start_radius {
            1.0
        } else {
            ((r - d.start_radius) / (d.fade_in_radius - d.start_radius)).min(1.0)
        };
        out.min(fade_in)
    }

    /// Quads around the ring: `startSegments` growing to `fullSegments`, rounded. [game]
    /// [port design] An arc keeps the ring's density: its share of that count, at least one.
    pub fn segments(&self, ring: &Ring) -> i32 {
        let d = &self.def;
        let t = (ring.radius - d.start_radius) / (d.full_radius - d.start_radius);
        let n = ((d.full_segments - d.start_segments) as f32 * t + d.start_segments as f32 + 0.5) as i32;
        match ring.arc {
            Some(arc) if n > 0 => ((n as f32 * arc.half_angle * 2.0 / std::f32::consts::TAU + 0.5) as i32).max(1),
            _ => n,
        }
    }

    /// The band's mesh (`FUN_00806380`), nothing when the ring is invisible or the object
    /// has no material. From angle -pi, a point at angle a lies at (sin a, cos a) x radius;
    /// each quad is inner and outer at a, then at the next angle, the normals leaning in and
    /// out at 45 degrees, u around the ring 0..1, v 0 inside and 1 outside; the triangles
    /// are 0 2 1, 1 2 3. [game] [port design] An arc runs from its heading less its half angle
    /// to its heading plus it, u 0..1 across the arc.
    pub fn mesh(&self, ring: &Ring, vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>) {
        let alpha = self.alpha(ring);
        if alpha <= 0.0 || self.def.material.is_none() {
            return;
        }
        let n = self.segments(ring);
        if n <= 0 {
            return;
        }
        let d = &self.def;
        let t = (ring.radius - d.start_radius) / (d.full_radius - d.start_radius);
        let width = (d.full_ring_radius - d.ring_radius) * t + d.ring_radius;
        let inner = ring.radius - width * 0.5;
        let outer = inner + width;
        let c = ring.centre;
        let step = 1.0 / n as f32;
        let (mut angle, span) = match ring.arc {
            Some(arc) => (arc.angle() - arc.half_angle, arc.half_angle * 2.0),
            None => (-std::f32::consts::PI, std::f32::consts::TAU),
        };
        let (mut s0, mut c0) = angle.sin_cos();
        let mut u0 = 0.0f32;
        for _ in 0..n {
            angle += step * span;
            let (s1, c1) = angle.sin_cos();
            let u1 = u0 + step;
            let at = |s: f32, co: f32, r: f32| Vec3::new(s * r + c.x, co * r + c.y, c.z);
            let lean = |s: f32, co: f32, sign: f32| Vec3::new(sign * s, sign * co, 1.0) * NORMAL_TILT;
            let base = vertices.len() as u32;
            for (position, normal, uv) in [
                (at(s0, c0, inner), lean(s0, c0, -1.0), Vec2::new(u0, 0.0)),
                (at(s0, c0, outer), lean(s0, c0, 1.0), Vec2::new(u0, 1.0)),
                (at(s1, c1, inner), lean(s1, c1, -1.0), Vec2::new(u1, 0.0)),
                (at(s1, c1, outer), lean(s1, c1, 1.0), Vec2::new(u1, 1.0)),
            ] {
                vertices.push(Vertex { position, normal, uv, alpha });
            }
            indices.extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
            (s0, c0, u0) = (s1, c1, u1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devastator() -> Def {
        // cairo_city_a_boss mission.str#1 [data]
        Def {
            material: Some(1),
            height_check: 2.0,
            start_radius: 3.0,
            fade_in_radius: 20.0,
            fade_out_radius: 70.0,
            full_radius: 160.0,
            ring_radius: 5.0,
            full_ring_radius: 70.0,
            speed: 75.0,
            start_segments: 25,
            full_segments: 100,
            damage_type: 0,
            damage: 25.0,
            ..Def::default()
        }
    }

    fn target(id: u32, x: f32) -> Candidate {
        Candidate { id, position: Vec3::new(x, 0.0, 0.0), radius: 0.0, size: None }
    }

    #[test]
    fn ring_grows_and_dies_at_full_radius() {
        let mut w = ShockWave::new(Def::default());
        w.trigger(Vec3::ZERO, 0.0);
        w.update(1.0, 0, None, &[], |_| true);
        assert_eq!(w.rings[0].radius, 0.5 + 7.5);
        w.update(1.0, 0, None, &[], |_| true);
        assert!(!w.active(), "15.5 >= 15: removed");
    }

    #[test]
    fn band_hits_once_per_living_set() {
        let mut w = ShockWave::new(devastator());
        w.trigger(Vec3::ZERO, 0.0);
        let near = [target(7, 10.0), target(8, 100.0)];
        let mut hits = Vec::new();
        // radius 3 + 75 * 0.1 = 10.5; band width 5 + 65 * 7.5 / 157 = 8.1: 6.45 .. 14.55.
        w.update(0.1, 0, None, &near, |h| {
            hits.push(*h);
            true
        });
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].target, 7);
        assert_eq!(hits[0].packet.amount, 25.0);
        assert!((hits[0].packet.direction - Vec3::new(PUSH_TILT, 0.0, PUSH_TILT)).length() < 1e-6);
        assert_eq!(hits[0].force, hits[0].packet.direction * 10000.0);
        // Still inside the band next update: not hurt twice.
        hits.clear();
        w.update(0.01, 0, None, &near, |h| {
            hits.push(*h);
            true
        });
        assert!(hits.is_empty());
    }

    #[test]
    fn refused_hit_is_tried_again() {
        let mut w = ShockWave::new(devastator());
        w.trigger(Vec3::ZERO, 0.0);
        let mut n = 0;
        w.update(0.1, 0, None, &[target(7, 10.0)], |_| {
            n += 1;
            false
        });
        w.update(0.01, 0, None, &[target(7, 10.0)], |_| {
            n += 1;
            false
        });
        assert_eq!(n, 2);
    }

    #[test]
    fn height_check_and_self() {
        let mut w = ShockWave::new(devastator());
        w.trigger(Vec3::ZERO, 0.0);
        let mut high = target(7, 10.0);
        high.position.z = 2.5;
        let mut hits = 0;
        w.update(0.1, 9, None, &[high, target(9, 10.0)], |_| {
            hits += 1;
            true
        });
        assert_eq!(hits, 0, "2.5 over heightCheck 2; 9 is the object itself");
    }

    #[test]
    fn size_class_picks_damage_type() {
        let mut def = devastator();
        def.damage_type = 0x800;
        def.damage_type_large = 0x40800;
        def.damage_type_small = 0;
        let mut w = ShockWave::new(def);
        w.trigger(Vec3::ZERO, 0.0);
        let mut small = target(1, 10.0);
        small.size = Some(Size::Small);
        let mut large = target(2, 0.0);
        large.position.y = 10.0;
        large.size = Some(Size::Large);
        let mut flags = Vec::new();
        w.update(0.1, 0, None, &[small, large], |h| {
            flags.push((h.target, h.packet.flags));
            true
        });
        flags.sort();
        assert_eq!(flags, vec![(1, 0x800), (2, 0x40800)]);
    }

    #[test]
    fn no_damage_no_type_sends_nothing() {
        let mut def = devastator();
        def.damage = 0.0;
        let mut w = ShockWave::new(def);
        w.trigger(Vec3::ZERO, 0.0);
        let mut hits = 0;
        w.update(0.1, 0, None, &[target(7, 10.0)], |_| {
            hits += 1;
            true
        });
        assert_eq!(hits, 0);
    }

    #[test]
    fn at_most_four_a_ring() {
        let mut w = ShockWave::new(devastator());
        w.trigger(Vec3::ZERO, 0.0);
        let many: Vec<_> = (0..6).map(|i| target(i + 1, 8.0 + i as f32)).collect();
        let mut hit = Vec::new();
        w.update(0.1, 0, None, &many, |h| {
            hit.push(h.target);
            true
        });
        assert_eq!(hit.len(), MAX_TARGETS);
        // [game 00799cd0] the kept ones are the first in the candidates' walk order, so the hits
        // come in the list's order (the old nearest-to-radius sort broke this).
        assert!(hit.windows(2).all(|w| w[0] < w[1]), "walk order: {hit:?}");
    }

    #[test]
    fn fades_and_mesh() {
        let w = ShockWave::new(devastator());
        let ring = |r| Ring { centre: Vec3::ZERO, radius: r, start_time: 0.0, arc: None };
        assert_eq!(w.alpha(&ring(3.0)), 0.0);
        assert!((w.alpha(&ring(11.5)) - 0.5).abs() < 1e-6);
        assert_eq!(w.alpha(&ring(50.0)), 1.0);
        assert!((w.alpha(&ring(115.0)) - 0.5).abs() < 1e-6);
        assert_eq!(w.segments(&ring(3.0)), 25);
        assert_eq!(w.segments(&ring(160.0)), 100);
        let (mut v, mut i) = (Vec::new(), Vec::new());
        w.mesh(&ring(50.0), &mut v, &mut i);
        let n = w.segments(&ring(50.0)) as usize;
        assert_eq!((v.len(), i.len()), (4 * n, 6 * n));
        // The first point: angle -pi, (sin, cos) = (0, -1).
        let (inner, outer) = w.band(50.0);
        assert!((v[0].position - Vec3::new(0.0, -inner, 0.0)).length() < 1e-4);
        assert!((v[1].position - Vec3::new(0.0, -outer, 0.0)).length() < 1e-4);
        assert!((v[1].normal - Vec3::new(0.0, -NORMAL_TILT, NORMAL_TILT)).length() < 1e-5);
        assert!((v[v.len() - 1].uv.x - 1.0).abs() < 1e-4);
        // No material: nothing drawn.
        let bare = ShockWave::new(Def { material: None, ..devastator() });
        let (mut v, mut i) = (Vec::new(), Vec::new());
        bare.mesh(&ring(50.0), &mut v, &mut i);
        assert!(v.is_empty());
    }

    /// The installed `megatronflight` ShockWave reads as the dump shows it. [data]
    #[test]
    fn installed_megatron_flight_shockwave() {
        use crate::formats::{lxb::DataFile, pack::{self, Pack}};
        let Ok(pack) = Pack::open(crate::install::dir().join("characters/megatronflight.str").as_path()) else {
            return;
        };
        let chunk = pack.of_type(pack::DATA).next().unwrap();
        let file = DataFile::parse(pack.data(chunk).to_vec()).unwrap();
        fn walk<'a>(node: Node<'a>, seen: &mut std::collections::HashSet<usize>, out: &mut Vec<Node<'a>>) {
            if let Some(at) = node.offset() {
                if !seen.insert(at) {
                    return;
                }
            }
            if node.type_name() == Some("ShockWave") {
                out.push(node);
            }
            for (_, child) in node.fields() {
                walk(child, seen, out);
            }
            for child in node.items() {
                walk(child, seen, out);
            }
        }
        let mut found = Vec::new();
        walk(file.root(), &mut Default::default(), &mut found);
        let defs: Vec<Def> = found.into_iter().map(read).collect();
        assert_eq!(defs.len(), 1);
        let d = &defs[0];
        assert_eq!(d.material, None);
        assert_eq!((d.start_radius, d.fade_in_radius, d.fade_out_radius), (2.5, 5.0, 35.0));
        assert_eq!((d.full_radius, d.ring_radius, d.full_ring_radius, d.speed), (20.0, 20.0, 20.0, 40.0));
        assert_eq!((d.start_segments, d.full_segments), (12, 50));
        // "(empty)" sets: present and 0, not the absent default.
        assert_eq!((d.damage_type, d.damage), (0, 0.0));
    }

    #[test]
    fn defaults_are_the_loaders() {
        let d = Def::default();
        assert_eq!((d.start_radius, d.full_radius, d.speed, d.damage), (0.5, 15.0, 7.5, 10.0));
        assert_eq!((d.start_segments, d.full_segments, d.damage_type), (10, 20, 0x40800));
    }
}
