//! The CombatRepair special (Ratchet and Long Haul): the cooldown, the heal zone the user puts
//! down, and the heal on each character inside it. Pure arithmetic, no engine types. Wired by
//! `game\src\targets.rs` (`heal_step`). Source: notes/special-combat-repair.md, sections "The heal
//! (second read)" and "The zone holder (third read)", and the stand-in pass of 2026-10-18
//! (`notes\standin-pass\body.md`).
//!
//! The zone is the `HealAreaEffect` thing of the character's own pack (vtable 00b60aa8), on
//! node index 0 of his object, named `CombatRepair` (`CombatRepairVehicle` in the car). [data]
//! Its `healRadius` and `specialHealingType` (the slot) are read by the caller from the pack
//! (`007ebf00` keeps the radius squared at `holder+0x48`). [game] The special's start sends
//! `Trigger` to that name (`0072afd0`), which the Lux dispatch (`0080df70`) turns into the
//! holder's On (vslot 6, `007eb6a0`); the special's end, when `specialAbilityDuration` runs out
//! (`0072a550` -> `0072b980`), sends `Untrigger` (9caa723d) to the same name, which the dispatch
//! turns into Off (vslot 7, `007eb6c0`: clears every aura source). So the zone lives exactly as
//! long as the special. Not ported: the pack's `EffectSpawner` (`r_repairpad_ratchet`) and the
//! heal-number UI (`send_ui_number` in `0072d540`); visuals are the effects package's.

use glam::Vec3;

use crate::formats::lxb::Node;
use crate::tuning::SpecialTuning;

/// Zone slot 0: the Long Haul aura. Its rate is `healthRegenRateFromLonghaul`. [game 0072d540]
pub const SLOT_LONGHAUL: usize = 0;
/// Zone slot 1: the Ratchet aura. Its rate is `healthRegenRateFromRatchet`. [game 0072d540]
/// [game 0072d540: the heal-number UI is sent for a healer named `Ratchet` (char+0x170) on slot
/// 1 and `Longhaul` on slot 0] [data: `specialHealingType` true in ratchet.str, false in
/// longhaul.str, read into holder+0x4c by 007ebf00]
pub const SLOT_RATCHET: usize = 1;

/// The receiver's four heal fields for this special, read from its own attributes (the
/// receiver's `attr`, not the healer's). [game 0072d540] [data]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rates {
    /// `[healthRegenRateFromLonghaul (attr+0x64), healthRegenRateFromRatchet (attr+0x68)]`.
    /// Hit points per second of the damage update's dt [game 0072d540: `rate * dt`].
    pub rate: [f32; 2],
    /// `[healthRegenSecsPerTicFromLonghaul (attr+0x6c), healthRegenSecsPerTicFromRatchet (attr+0x70)]`.
    /// -1 for all four characters [data]; a value above 0 selects the discrete branch of `heal`.
    pub secs_per_tic: [f32; 2],
}

impl Rates {
    /// Reads the four fields by name. [game 00726710 loader] [data] for the names.
    /// [data] global_states.txt lines 38434 to 38437 (and 39194 to 39197) hold all four fields for
    /// Ratchet and Long Haul (checked 2026-10-20), so the 0 for a missing field never runs here. It is
    /// a Rust fallback, not a game rule.
    pub fn read(attributes: Node<'_>) -> Self {
        let num = |name: &str| attributes.get(name).and_then(Node::float).unwrap_or(0.0);
        Self {
            rate: [
                num("healthRegenRateFromLonghaul"),
                num("healthRegenRateFromRatchet"),
            ],
            secs_per_tic: [
                num("healthRegenSecsPerTicFromLonghaul"),
                num("healthRegenSecsPerTicFromRatchet"),
            ],
        }
    }
}

/// One character as the zone sees it: its id, team, its centre, and the two flags the scan
/// reads. [game 007eb6f0] `present` is `c+0x234 != 0`; `flag` is bit 10 of `c+0x14c`.
/// `position` is the point the scan measures from: his target point's bottom (`0071a460` /
/// `0071a2e0`, the root for a character on foot) plus half his height (`0070abe0`) [game 007eb6f0,
/// both the enter and the leave test]; the caller adds the half height. `team` 0 is no team
/// [game 0117e760: equal NONZERO words at +0x50].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Member {
    pub id: u32,
    pub team: u32,
    pub position: Vec3,
    pub present: bool,
    pub flag: bool,
}

/// An enter or leave of the zone, for one member and one slot. [game 0072d0b0] [game 0072d1f0]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub id: u32,
    pub slot: usize,
    pub enter: bool,
}

/// The heal zone a user puts down: an active flag, a radius, a slot, the tracked members,
/// and the time it stays up. [game 007eb6f0] for the scan; the radius is in metres here.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub active: bool,
    /// The pack's `healRadius` (25 for both on foot) [data], in the game's world units: `007ebf00`
    /// stores it squared at `holder+0x48` with no scale [game 007ebf00 line 25], and the scan
    /// `007eb6f0` compares it with the squared distance between the game's own positions, with no
    /// scale either (lines 79-83, 183) [game 007eb6f0]. The port's positions are the game's
    /// coordinates (`assets::game_to_bevy` is an axis swap only), so the radius is the same
    /// number. Whether those units are metres is not read; the port needs no conversion.
    pub radius: f32,
    pub slot: usize,
    /// Seconds left: the special's `specialAbilityDuration` (`+0x8c`). [game] The holder has no
    /// timer of its own; the special's end sends it `Untrigger` -> Off (module doc).
    pub time_left: f32,
    inside: Vec<u32>,
}

impl Zone {
    /// The radius and slot are the pack's `healRadius` and `specialHealingType` [data], read by
    /// the caller (the game reads them once, at the holder's creation, `007ebf00`).
    pub fn new(radius: f32, slot: usize) -> Self {
        Self {
            active: false,
            radius,
            slot,
            time_left: 0.0,
            inside: Vec::new(),
        }
    }

    /// Switches the zone on for `duration` seconds: `Trigger` -> On (`007eb6a0`), and the
    /// special's end after `specialAbilityDuration` -> Off. [game] see the module doc.
    pub fn switch_on(&mut self, duration: f32) {
        self.active = true;
        self.time_left = duration;
    }

    /// Ids currently inside the zone.
    pub fn inside(&self) -> &[u32] {
        &self.inside
    }

    /// Re-tests every member against the zone and pushes the enters and leaves into `out`.
    /// [game 007eb6f0, the holder's update, read 2026-10-18 for the stand-in pass]
    /// `owner` is the user who put the zone down; `centre` is the holder's own position: the
    /// `HealAreaEffect` sits on node index 0 of the user's object [data], so it follows him and
    /// the caller passes its point each step. Each member's `position` is its centre (see
    /// `Member`).
    ///
    /// Enters first: a member is taken when `present` and `flag` are set, it is the owner or on
    /// the owner's (nonzero) team [game 0117e760], it is not tracked already (the scan walks the
    /// holder's ring `+0x50` and skips a member it finds there), and its distance squared to the
    /// holder is at most the squared radius (`<=`). [game]
    /// Then leaves: a tracked member leaves when its distance squared is above the squared radius
    /// (`r2 <= d && d != r2`); the flags and the team are NOT tested again. A member whose handle
    /// is gone, or whose status block (`present`) is gone, is dropped from the ring without a
    /// leave (`0072d1f0` is called only with `+0x234` set). [game]
    pub fn scan(&mut self, owner: Member, centre: Vec3, members: &[Member], out: &mut Vec<Change>) {
        if !self.active {
            return;
        }
        let r2 = self.radius * self.radius;
        let slot = self.slot;
        let same_team = |m: &Member| m.id == owner.id || (m.team != 0 && m.team == owner.team);
        for m in members {
            if m.present
                && m.flag
                && same_team(m)
                && !self.inside.contains(&m.id)
                && (m.position - centre).length_squared() <= r2
            {
                self.inside.push(m.id);
                out.push(Change { id: m.id, slot, enter: true });
            }
        }
        let mut i = 0;
        while i < self.inside.len() {
            let id = self.inside[i];
            match members.iter().find(|m| m.id == id) {
                None => {
                    self.inside.remove(i);
                }
                Some(m) if (m.position - centre).length_squared() > r2 => {
                    self.inside.remove(i);
                    if m.present {
                        out.push(Change { id, slot, enter: false });
                    }
                }
                Some(_) => i += 1,
            }
        }
    }

    /// Counts the zone's time down. When it runs out the zone switches off and every tracked
    /// member leaves. [game] the expiry is the special's end (`0072b980` Untrigger -> Off
    /// `007eb6c0`, which empties the ring through `007ebd40` -> `0072d1f0`).
    pub fn tick(&mut self, dt: f32, out: &mut Vec<Change>) {
        if !self.active {
            return;
        }
        self.time_left -= dt;
        if self.time_left <= 0.0 {
            self.active = false;
            let slot = self.slot;
            for id in self.inside.drain(..) {
                out.push(Change { id, slot, enter: false });
            }
        }
    }
}

/// The receiver's side of the zone: how many zones of each slot cover it (`comp+0xa8+4*s`)
/// and its discrete-tic timers (`comp+0xb0+4*s`). [game 0072d0b0, 0072d1f0, 0072d540]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Receiver {
    pub in_zone: [u32; 2],
    pub tic: [f32; 2],
}

impl Receiver {
    /// Applies one enter or leave. On the last leave of a slot the tic timer is reset to 0.
    /// [game 0072d1f0] for the reset.
    pub fn on_change(&mut self, change: Change) {
        let s = change.slot;
        if change.enter {
            self.in_zone[s] += 1;
        } else {
            self.in_zone[s] = self.in_zone[s].saturating_sub(1);
            if self.in_zone[s] == 0 {
                self.tic[s] = 0.0;
            }
        }
    }

    /// The hit points healed this step from every slot the receiver is in. Use the result
    /// with `apply`. [game 0072d540]
    pub fn heal(&mut self, rates: &Rates, dt: f32) -> f32 {
        let mut total = 0.0;
        for s in 0..2 {
            if self.in_zone[s] > 0 {
                total += heal(rates, s, dt, &mut self.tic[s]);
            }
        }
        total
    }
}

/// One slot's heal for one step of `dt` seconds. [game 0072d540] (lines of the second read):
/// - secs per tic off (`<= 0`, the case for all data): `rate * dt`, continuous.
/// - else: `tic += dt`; when `tic` reaches the interval, the interval is subtracted and one
///   tic's heal `rate * interval` is returned; otherwise 0.
/// [game 0072d540] the rate is hit points per second of the update's dt; a tic is due once
/// `tic` is no longer below the interval (`if (tic < secs) skip`, so `>=`), and one tic at most
/// per step (the interval is subtracted once).
pub fn heal(rates: &Rates, slot: usize, dt: f32, tic: &mut f32) -> f32 {
    let secs = rates.secs_per_tic[slot];
    if secs <= 0.0 {
        return rates.rate[slot] * dt;
    }
    *tic += dt;
    if *tic >= secs {
        *tic -= secs;
        rates.rate[slot] * secs
    } else {
        0.0
    }
}

/// Adds the heal to the health, never above `maximum`. [game 0071f5c0 -> 0071fbd0] for the
/// add and the clamp; the same ceiling is in `damage::regenerate` (`.min(maximum)`, and no
/// change once health is at or above it).
pub fn apply(health: f32, maximum: f32, amount: f32) -> f32 {
    if health >= maximum {
        return health;
    }
    (health + amount).min(maximum)
}

/// The script name the special sends to the user: the car's name when in the car, else on
/// foot. 0 means none. [game 0072afd0 lines 22-48: the branch is `char+0x31c == 3`, the car's
/// control mode]; the caller passes `State::car_mode` for it [assumed, as in `sim.rs`, which
/// sets `car_mode` where the game's mode becomes 3]. The name is sent, not run here.
/// settles: the caller of the script send in the game (which mode value it passes, and whether the send is the one that runs the repair).
pub fn sent_script(special: &SpecialTuning, in_car: bool) -> u32 {
    if in_car {
        special.vehicle_script
    } else {
        special.script
    }
}

/// The special as a whole: the cooldown and the zone it puts down.
#[derive(Clone, Debug, PartialEq)]
pub struct Ability {
    /// Seconds until the next press. [game 0072bc40] sets it to `attr+0xbc`; `0072a550` runs it
    /// down by the update's dt in seconds [game] (the same clock as `sim::State::special_cooldown`).
    pub cooldown: f32,
    pub zone: Zone,
}

impl Ability {
    /// The radius and the slot are the pack's (`Zone::new`) [data].
    pub fn new(radius: f32, slot: usize) -> Self {
        Self {
            cooldown: 0.0,
            zone: Zone::new(radius, slot),
        }
    }

    /// A press. Refused while the cooldown runs. Otherwise the cooldown starts at
    /// `specialAbilityCooldown` [game 0072bc40] and the zone switches on for
    /// `specialAbilityDuration` [game: the special's end switches it off, module doc]. Returns
    /// whether the press was taken.
    pub fn press(&mut self, special: &SpecialTuning) -> bool {
        if self.cooldown > 0.0 {
            return false;
        }
        self.cooldown = special.cooldown;
        self.zone.switch_on(special.duration);
        true
    }

    /// Counts the cooldown and the zone's time down by `dt`.
    pub fn tick(&mut self, dt: f32, out: &mut Vec<Change>) {
        self.cooldown = (self.cooldown - dt).max(0.0);
        self.zone.tick(dt, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test values only: the radius is not known from the source.
    const TEST_RADIUS: f32 = 10.0;
    const STEP: f32 = 0.032;

    fn member(id: u32, team: u32, x: f32) -> Member {
        Member {
            id,
            team,
            position: Vec3::new(x, 0.0, 0.0),
            present: true,
            flag: true,
        }
    }

    fn feed(receiver: &mut Receiver, changes: &[Change], id: u32) {
        for c in changes.iter().filter(|c| c.id == id) {
            receiver.on_change(*c);
        }
    }

    // Ratchet's own numbers [data]: 40 from Long Haul, 60 from Ratchet; secs per tic -1.
    fn ratchet_rates() -> Rates {
        Rates {
            rate: [40.0, 60.0],
            secs_per_tic: [-1.0, -1.0],
        }
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn teammate_inside_gains_rate_times_dt() {
        let owner = member(1, 1, 0.0);
        let mut zone = Zone::new(TEST_RADIUS, SLOT_LONGHAUL);
        zone.switch_on(5.34);
        let mut out = Vec::new();
        zone.scan(owner, Vec3::ZERO, &[owner, member(2, 1, 5.0)], &mut out);
        let mut rx = Receiver::default();
        feed(&mut rx, &out, 2);
        let healed = rx.heal(&ratchet_rates(), STEP);
        assert!(near(healed, 40.0 * STEP), "got {healed}");
    }

    #[test]
    fn teammate_outside_gains_nothing() {
        let owner = member(1, 1, 0.0);
        let mut zone = Zone::new(TEST_RADIUS, SLOT_LONGHAUL);
        zone.switch_on(5.34);
        let mut out = Vec::new();
        zone.scan(owner, Vec3::ZERO, &[owner, member(2, 1, 20.0)], &mut out);
        assert!(out.iter().all(|c| c.id != 2));
        let mut rx = Receiver::default();
        feed(&mut rx, &out, 2);
        assert_eq!(rx.heal(&ratchet_rates(), STEP), 0.0);
    }

    #[test]
    fn enemy_inside_gains_nothing() {
        let owner = member(1, 1, 0.0);
        let mut zone = Zone::new(TEST_RADIUS, SLOT_LONGHAUL);
        zone.switch_on(5.34);
        let mut out = Vec::new();
        zone.scan(owner, Vec3::ZERO, &[owner, member(3, 2, 5.0)], &mut out);
        assert!(out.iter().all(|c| c.id != 3));
        let mut rx = Receiver::default();
        feed(&mut rx, &out, 3);
        assert_eq!(rx.heal(&ratchet_rates(), STEP), 0.0);
    }

    #[test]
    fn owner_heals_himself_when_flagged() {
        let owner = member(1, 1, 0.0);
        let mut zone = Zone::new(TEST_RADIUS, SLOT_RATCHET);
        zone.switch_on(5.34);
        let mut out = Vec::new();
        zone.scan(owner, Vec3::ZERO, &[owner], &mut out);
        let mut rx = Receiver::default();
        feed(&mut rx, &out, 1);
        assert!(near(rx.heal(&ratchet_rates(), STEP), 60.0 * STEP));
    }

    #[test]
    fn ratchet_in_longhaul_zone_uses_40_and_in_his_own_60() {
        let owner = member(1, 1, 0.0);
        let rates = ratchet_rates();
        for (slot, want) in [(SLOT_LONGHAUL, 40.0), (SLOT_RATCHET, 60.0)] {
            let mut zone = Zone::new(TEST_RADIUS, slot);
            zone.switch_on(5.34);
            let mut out = Vec::new();
            zone.scan(owner, Vec3::ZERO, &[owner, member(2, 1, 1.0)], &mut out);
            let mut rx = Receiver::default();
            feed(&mut rx, &out, 2);
            assert!(near(rx.heal(&rates, 1.0), want), "slot {slot}");
        }
    }

    #[test]
    fn leaving_the_zone_stops_the_heal() {
        let owner = member(1, 1, 0.0);
        let mut zone = Zone::new(TEST_RADIUS, SLOT_LONGHAUL);
        zone.switch_on(5.34);
        let mut out = Vec::new();
        zone.scan(owner, Vec3::ZERO, &[owner, member(2, 1, 5.0)], &mut out);
        let mut rx = Receiver::default();
        feed(&mut rx, &out, 2);
        out.clear();
        zone.scan(owner, Vec3::ZERO, &[owner, member(2, 1, 50.0)], &mut out);
        feed(&mut rx, &out, 2);
        assert_eq!(rx.heal(&ratchet_rates(), STEP), 0.0);
    }

    #[test]
    fn cooldown_refuses_a_second_press() {
        let special = SpecialTuning {
            cooldown: 25.88,
            duration: 5.34,
            ..Default::default()
        };
        let mut ability = Ability::new(TEST_RADIUS, SLOT_LONGHAUL);
        assert!(ability.press(&special));
        assert!(ability.zone.active);
        assert!(!ability.press(&special));
        let mut out = Vec::new();
        ability.tick(25.0, &mut out);
        assert!(!ability.press(&special));
        ability.tick(1.0, &mut out);
        assert!(ability.press(&special));
    }

    #[test]
    fn zone_expiry_releases_every_member() {
        let owner = member(1, 1, 0.0);
        let mut zone = Zone::new(TEST_RADIUS, SLOT_LONGHAUL);
        zone.switch_on(1.0);
        let mut out = Vec::new();
        zone.scan(owner, Vec3::ZERO, &[owner, member(2, 1, 5.0)], &mut out);
        out.clear();
        zone.tick(1.0, &mut out);
        assert!(!zone.active);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|c| !c.enter && c.slot == SLOT_LONGHAUL));
        assert!(zone.inside().is_empty());
    }

    // [game 007eb6f0] the leave tests only the distance: a tracked member whose flag goes stays.
    #[test]
    fn leave_is_by_distance_only_and_a_vanished_member_has_no_leave() {
        let owner = member(1, 1, 0.0);
        let mut zone = Zone::new(TEST_RADIUS, SLOT_LONGHAUL);
        zone.switch_on(5.34);
        let mut out = Vec::new();
        zone.scan(owner, Vec3::ZERO, &[owner, member(2, 1, 5.0), member(3, 1, 6.0)], &mut out);
        assert_eq!(out.len(), 3);
        out.clear();
        // Member 2 loses its flag but stays in range: no leave. Member 3 is gone from the world:
        // dropped from the ring without a leave.
        let flagless = Member { flag: false, ..member(2, 1, 5.0) };
        zone.scan(owner, Vec3::ZERO, &[owner, flagless], &mut out);
        assert!(out.is_empty(), "{out:?}");
        assert!(zone.inside().contains(&2) && !zone.inside().contains(&3));
        // Exactly on the radius stays inside (the leave needs d2 > r2).
        zone.scan(owner, Vec3::ZERO, &[owner, member(2, 1, TEST_RADIUS)], &mut out);
        assert!(out.is_empty());
        zone.scan(owner, Vec3::ZERO, &[owner, member(2, 1, TEST_RADIUS + 0.01)], &mut out);
        assert_eq!(out, [Change { id: 2, slot: SLOT_LONGHAUL, enter: false }]);
    }

    // [game 0117e760] team 0 is no team: two team-0 characters are not allies.
    #[test]
    fn team_zero_is_not_a_team() {
        let owner = member(1, 0, 0.0);
        let mut zone = Zone::new(TEST_RADIUS, SLOT_RATCHET);
        zone.switch_on(5.34);
        let mut out = Vec::new();
        zone.scan(owner, Vec3::ZERO, &[owner, member(2, 0, 1.0)], &mut out);
        assert_eq!(out, [Change { id: 1, slot: SLOT_RATCHET, enter: true }]);
    }

    #[test]
    fn discrete_branch_heals_one_tic_per_interval() {
        let rates = Rates {
            rate: [40.0, 60.0],
            secs_per_tic: [0.5, -1.0],
        };
        let mut tic = 0.0;
        let mut total = 0.0;
        for _ in 0..10 {
            total += heal(&rates, SLOT_LONGHAUL, 0.1, &mut tic);
        }
        // Ten steps of 0.1 s make one second: two tics of 40 * 0.5.
        assert!(near(total, 40.0), "got {total}");
    }

    #[test]
    fn health_never_exceeds_the_maximum() {
        assert_eq!(apply(319.9, 320.0, 1.28), 320.0);
        assert_eq!(apply(320.0, 320.0, 5.0), 320.0);
        assert!(near(apply(100.0, 320.0, 1.28), 101.28));
    }
}
