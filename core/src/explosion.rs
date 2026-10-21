//! The `Explosion` object as any placed or spawned thing has it (1030 in the installed
//! data: destructibles, projectiles, the punches' blasts): a delay, a sphere that grows from
//! `start_scale` to its full size over `explosion_time`, and one damage message to each thing
//! its sphere touches.
//!
//! [game] create `FUN_007e5480` (pool 0x1d, constructor `FUN_007e5140`, loader
//! `FUN_007e5640`), update `FUN_007e5f00`, contact `FUN_007e60f0`, owner `FUN_007e6af0`;
//! `notes\melee.md` "Explosion" and `notes\effects-shockwave-tube.md` "Explosion".
//! `ExplosionDef` (`formats::scene`) holds the data and the growth and fall-off arithmetic;
//! this adds the delay, who is spared, the stun's fall-off and the push. The melee blasts
//! (`sim.rs`) and the missile's (`weapons::MissileBlast`) are older ports of the same object.
//!
//! Not in: the camera shake at the start (`FUN_007e5d30`, the caller's), the surface scorch
//! (`SSDImpactPreset`, the first part of the contact routine).

use crate::damage::{Actor, Packet};
use crate::formats::scene::ExplosionDef;
use crate::shockwave::SpecialCase;
use glam::Vec3;

/// The push: the 3D way from the explosion to the thing times the damage times 100. [game]
const FORCE_PER_DAMAGE: f32 = 100.0;
/// With damage flag bit 6 the stun time falls with the damage, to this share at the edge.
/// [game]
const STUN_EDGE: f32 = 0.6;
const STUN_FALLS: u32 = 1 << 6;

/// A body touching the explosion's sphere that sends `PUNCH` to it.
#[derive(Clone, Copy, Debug)]
pub struct Contact {
    /// The object the body belongs to: hit once.
    pub target: u32,
    /// Where that object stands (`+0x34`).
    pub target_pos: Vec3,
    /// The contact point and normal as the collision reports them.
    pub point: Vec3,
    pub normal: Vec3,
    /// A player's character (or what a player drives): spared unless `damage_player`.
    pub player: bool,
    /// Who owns the object: when it is the explosion's owner it is spared unless
    /// `can_damage_self`.
    pub owner: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub target: u32,
    pub point: Vec3,
    pub packet: Packet,
    pub force: Vec3,
}

#[derive(Clone, Debug)]
pub struct Explosion {
    pub def: ExplosionDef,
    pub centre: Vec3,
    pub source: Option<Actor>,
    /// The owner object's id (`+0x10`).
    pub owner: Option<u32>,
    special: SpecialCase,
    damage: f32,
    min_damage: f32,
    /// `+0x3c`: counts down while not negative; the explosion is live once it is.
    delay: f32,
    /// `+0x80`.
    age: f32,
    last_dt: f32,
    hit: Vec<u32>,
}

impl SpecialCase {
    /// The explosion's `specialCaseDamageStuff` block, as `ExplosionDef` reads it (the same six
    /// floats `FUN_00744f10` writes for every special case: stun, slow, dot, heat, throw speed,
    /// and the throw angle, stored here in radians). [game 00744f10] [data] Read by
    /// `formats::scene::read_explosion`; the slow, dot and heat were not read before.
    pub fn of_explosion(def: &ExplosionDef) -> Self {
        Self {
            stun_time: def.stun_time,
            slow_time: def.slow_time,
            dot_time: def.dot_time,
            heat: def.heat,
            knock_back_speed: def.knock_back_speed,
            knock_back_angle: def.knock_back_angle.to_radians(),
        }
    }
}

impl Explosion {
    /// `pound`: the owner's `groundPoundDamage` and `...Weak` when it is a character
    /// (`FUN_007e6af0`).
    pub fn new(def: ExplosionDef, centre: Vec3, source: Option<Actor>, owner: Option<u32>, pound: Option<(f32, f32)>) -> Self {
        let (damage, min_damage) = match pound {
            Some((strong, weak)) => def.owned_damage(strong, weak),
            None => (def.damage, def.min_damage),
        };
        Self {
            special: SpecialCase::of_explosion(&def),
            delay: def.delay,
            def,
            centre,
            source,
            owner,
            damage,
            min_damage,
            age: 0.0,
            last_dt: 0.0,
            hit: Vec::new(),
        }
    }

    /// Replaces the special case with the data's whole block (for a caller that read it).
    pub fn with_special(mut self, special: SpecialCase) -> Self {
        self.special = special;
        self
    }

    /// Live: past its delay. A body touches it only then. [game 007e5f00] the update clears bit
    /// 0x2 of the object's `+0x60` when the delay passes 0. [game 005be890 l.37] the world ray and
    /// sphere query takes a shape only while the owner's `+0x60` bit 0x2 is CLEAR, so until live
    /// the explosion is not in those queries. [game 007e60f0 l.1-35, read 2026-10-20] the contact
    /// tests neither the delay nor bit 0x2 (only the object's `+0x100` and the body's flag
    /// `+0x9c & 0x100`), so the live gate is the query's alone. [assumed] a contact reaches 007e60f0
    /// only through that query; the port's `contact` (l.152-153) tests `live()` itself, which is the
    /// same for a query contact. settles: the caller of 007e60f0 (no direct caller, no vtable slot, no
    /// immediate: `find_ptr`), i.e. the explosion object's class and its contact dispatch (the loader
    /// `007e5640` makes the object; not read).
    pub fn live(&self) -> bool {
        self.delay < 0.0
    }

    /// The sphere's radius now.
    pub fn radius(&self) -> f32 {
        self.def.radius * self.def.scale(self.age, self.last_dt)
    }

    /// One update (`FUN_007e5f00`); false once it is to be removed. While the delay is not
    /// negative it counts down, and the update it goes under 0 only makes it live. After
    /// that the age grows, and it goes once the age is past `explosion_time` both before and
    /// after the update. [game]
    pub fn update(&mut self, dt: f32) -> bool {
        if self.delay >= 0.0 {
            self.delay -= dt;
            return true;
        }
        let before = self.age;
        self.age += dt;
        self.last_dt = dt;
        !(self.def.time < self.age && self.def.time < before)
    }

    /// A body touching the sphere (`FUN_007e60f0`). [game]
    pub fn contact(&mut self, c: &Contact) -> Option<Hit> {
        if !self.live() || self.hit.contains(&c.target) {
            return None;
        }
        if c.player && !self.def.damage_player {
            return None;
        }
        if !self.def.can_damage_self && self.owner.is_some() && (Some(c.target) == self.owner || c.owner == self.owner) {
            return None;
        }
        let off = c.target_pos - self.centre;
        let direction = if off.x != 0.0 || off.y != 0.0 {
            off.truncate().normalize().extend(0.0)
        } else {
            -c.normal
        };
        let force = off.normalize_or_zero() * self.damage * FORCE_PER_DAMAGE;
        let radius = self.radius();
        let mut special = self.special;
        let amount = self.def.damage_at(self.damage, self.min_damage, c.point.distance(self.centre), radius);
        if self.def.damage_flags & STUN_FALLS != 0 && (0.0..=1.0).contains(&self.def.fall_off) {
            let inner = self.def.fall_off * radius;
            let d = c.point.distance(self.centre);
            let edge = special.stun_time * STUN_EDGE;
            if d >= radius {
                special.stun_time = edge;
            } else if d >= inner {
                let t = 1.0 - (d - inner) / (radius - inner);
                special.stun_time = (special.stun_time - edge) * t + edge;
            }
        }
        self.hit.push(c.target);
        Some(Hit {
            target: c.target,
            point: c.point,
            force,
            packet: Packet {
                source: self.source,
                amount,
                flags: self.def.damage_flags,
                direction,
                knockback_speed: special.knock_back_speed,
                knockback_angle: special.knock_back_angle,
                stun_time: special.stun_time,
                slow_time: special.slow_time,
                dot_time: special.dot_time,
                heat: special.heat,
                headshot: false,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def() -> ExplosionDef {
        ExplosionDef {
            radius: 10.0,
            start_scale: 0.5,
            time: 0.15,
            delay: 0.0,
            damage: 40.0,
            min_damage: 10.0,
            fall_off: 0.5,
            pound_override: false,
            damage_player: false,
            can_damage_self: false,
            damage_flags: 0x900,
            knock_back_speed: 25.0,
            knock_back_angle: 3.0,
            camera_shake: 0,
            stun_time: 2.0,
            slow_time: 0.0,
            dot_time: 0.0,
            heat: 0.0,
        }
    }

    fn touch(target: u32, x: f32) -> Contact {
        Contact { target, target_pos: Vec3::new(x, 0.0, 0.0), point: Vec3::new(x, 0.0, 0.0), normal: Vec3::X, player: false, owner: None }
    }

    #[test]
    fn delay_then_age_then_gone() {
        let mut e = Explosion::new(def(), Vec3::ZERO, None, None, None);
        assert!(!e.live());
        assert!(e.update(0.032), "the delay update only makes it live");
        assert!(e.live());
        assert_eq!(e.age, 0.0);
        let mut updates = 0;
        while e.update(0.032) {
            updates += 1;
        }
        // Ages 0.032 .. 0.16 stay (0.16 > 0.15 but 0.128 is not), 0.192 goes.
        assert_eq!(updates, 5);
    }

    #[test]
    fn hits_once_with_fall_off_and_spares() {
        let mut e = Explosion::new(def(), Vec3::ZERO, None, Some(9), None);
        e.update(0.032);
        e.update(0.2); // past its time: full size
        let hit = e.contact(&touch(1, 7.5)).unwrap();
        // inner 5, radius 10: 7.5 is half way: 10 + 30 * 0.5.
        assert!((hit.packet.amount - 25.0).abs() < 1e-5);
        assert_eq!(hit.packet.direction, Vec3::X);
        assert_eq!(hit.force, Vec3::X * 4000.0);
        assert!((hit.packet.knockback_angle - 3f32.to_radians()).abs() < 1e-6);
        assert_eq!(hit.packet.stun_time, 2.0, "no bit 6: the stun is whole");
        assert!(e.contact(&touch(1, 7.5)).is_none(), "once");
        assert!(e.contact(&Contact { player: true, ..touch(2, 1.0) }).is_none());
        assert!(e.contact(&touch(9, 1.0)).is_none(), "its owner");
        assert!(e.contact(&Contact { owner: Some(9), ..touch(3, 1.0) }).is_none(), "what the owner owns");
    }

    #[test]
    fn stun_falls_with_bit_six() {
        let mut d = def();
        d.damage_flags |= STUN_FALLS;
        let mut e = Explosion::new(d, Vec3::ZERO, None, None, None);
        e.update(0.032);
        e.update(0.2);
        let mid = e.contact(&touch(1, 7.5)).unwrap();
        assert!((mid.packet.stun_time - (1.2 + 0.8 * 0.5)).abs() < 1e-5);
        let edge = e.contact(&touch(2, 12.0)).unwrap();
        assert!((edge.packet.stun_time - 1.2).abs() < 1e-5);
        assert_eq!(edge.packet.amount, 10.0);
    }

    #[test]
    fn coincident_takes_the_contact_normal() {
        let mut e = Explosion::new(def(), Vec3::ZERO, None, None, None);
        e.update(0.032);
        let hit = e.contact(&Contact { target_pos: Vec3::new(0.0, 0.0, 3.0), normal: Vec3::Z, ..touch(1, 0.0) }).unwrap();
        assert_eq!(hit.packet.direction, -Vec3::Z);
    }

    #[test]
    fn pound_override_owner_damage() {
        let d = ExplosionDef { pound_override: true, damage: 65.0, ..def() };
        let mut e = Explosion::new(d, Vec3::ZERO, None, None, Some((100.0, 60.0)));
        e.update(0.032);
        e.update(0.2);
        assert_eq!(e.contact(&touch(1, 1.0)).unwrap().packet.amount, 100.0);
    }

    #[test]
    fn special_case_takes_all_six_values() {
        let d = ExplosionDef { stun_time: 2.5, slow_time: 3.0, dot_time: 4.0, heat: 30.0, knock_back_speed: 15.0, knock_back_angle: 35.0, ..def() };
        let s = SpecialCase::of_explosion(&d);
        assert_eq!((s.stun_time, s.slow_time, s.dot_time, s.heat, s.knock_back_speed), (2.5, 3.0, 4.0, 30.0, 15.0));
        assert!((s.knock_back_angle - 35f32.to_radians()).abs() < 1e-6);
    }

    /// Starscream's launcher special round (`starscream.str#0`): its `Explosion`'s
    /// `specialCaseDamageStuff` reads slowTime 5, knockBackSpeed 15, knockBackAngle 35 [data];
    /// damage 5 is the special's `explosion_for`. Skips without the install.
    #[test]
    fn starscream_special_blast_reads_its_slow() {
        let Ok(data) = crate::character::load(crate::install::dir(), "starscream") else { return };
        let launcher = -1877059191i32 as u32;
        let def = data.weapons.iter().find(|w| w.name == launcher).and_then(|w| w.explosion_for(true)).unwrap();
        println!(
            "starscream special blast: damage {} slow {} dot {} heat {} throw {} at {} degrees",
            def.damage, def.slow_time, def.dot_time, def.heat, def.knock_back_speed, def.knock_back_angle
        );
        assert_eq!((def.damage, def.slow_time, def.knock_back_speed, def.knock_back_angle), (5.0, 5.0, 15.0, 35.0));
        assert_eq!((def.dot_time, def.heat, def.stun_time), (0.0, 0.0, 0.0));
        let special = SpecialCase::of_explosion(&def);
        assert_eq!(special.slow_time, 5.0);
        assert!((special.knock_back_angle - 35f32.to_radians()).abs() < 1e-6);
    }
}
