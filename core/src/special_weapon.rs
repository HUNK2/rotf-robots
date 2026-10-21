//! The weapon-type specials: Megatron's special gun (`MegatronSpecialGun`), Jetfire's missile
//! dump (on foot) and carpet bomb (as a jet), and the Audi's mines (on foot and as a car).
//! No engine types; space is the game's (x right, y forward, z up).
//!
//! The special's cooldown, duration and type are read in `tuning.rs` (`SpecialTuning`) and
//! started by `sim::start_special`; this file is the weapon part. The shot comes from one of
//! two places, decided per form by `specialAbility*WeaponFireFromAnim`:
//!
//!   - the clip's `WeaponFireSpecialEvent` (handler `007ba870`, which calls `FUN_007b3d80(mgr, 1)`
//!     and so skips the from-animation gate), or
//!   - the special's start (`FUN_007b3d80(mgr, 0)`), only when the form's weapon is NOT from the
//!     animation (`notes\special-weapons.md` S2, `notes\special-jetfire-mines.md` 0).
//!
//! The weapon itself is the existing one: its def and numbers come from `weapons::read_defs`
//! and `weapons::load`, and this file only adds the few fields those readers do not keep
//! (`burstPerFire`, `timeBetweenEachBurst`, `isSticky`, the sticky lifetimes, `oneOutAtATime`,
//! `proximityActivateOnCollision`, the fire-dummy angles).
//!
//! Tags as in `weapons.rs`: [game <address>] read in code, [data] read from the character's or
//! the global data (`export\data\global_states.txt`), [assumed] our reading (why and what would
//! settle it are in `notes\status.md`, "Phase 2, wave 3"), [stand-in] invented.

use crate::formats::lxb::Node;
use crate::weapons::{Kind, WeaponDef, WeaponTuning};

/// One form's special weapon, by the character's data. [data] (`characterAttributes<Name>`,
/// `notes\character-specials.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpecialForm {
    /// `specialAbilityWeaponFireName` (on foot) or `specialAbilityVehicleWeaponFireName` (car):
    /// the weapon's name hash, 0 for none. [data]
    pub weapon: u32,
    /// `specialAbilityWeaponFireFromAnim` (`VehicleWeaponFireFromAnim` for the car): true means
    /// the clip's event fires it; false means the special's start fires it. [data] and
    /// [game 007b3d80] (the gate of step S2b).
    pub from_anim: bool,
}

/// The two forms' weapons, `[0]` on foot and `[1]` in the car. [data]
pub fn read_forms(attributes: Node) -> [SpecialForm; 2] {
    let id = |name: &str| attributes.get(name).and_then(Node::int).map_or(0, |h| h as u32);
    let on = |name: &str| attributes.get(name).is_some_and(Node::is_true);
    [
        SpecialForm { weapon: id("specialAbilityWeaponFireName"), from_anim: on("specialAbilityWeaponFireFromAnim") },
        SpecialForm { weapon: id("specialAbilityVehicleWeaponFireName"), from_anim: on("specialAbilityVehicleWeaponFireFromAnim") },
    ]
}

/// The numbers of one round's weapon: the existing `WeaponTuning` (rate, projectiles, grenade
/// launch, fuse, proximity, explosive damage) plus the extra fields read here. [data]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    pub kind: Kind,
    /// `rateOfFire`: seconds from one press to the next one that is allowed. [data] (the
    /// ready interval, `FUN_007ad080`; `notes\special-jetfire-mines.md` A1).
    pub rate_of_fire: f32,
    /// `automaticFiring`. [data]
    pub automatic: bool,
    /// `numProjectilesPerFire`: rounds let go together in one burst. [data] [game FUN_007ae010]
    pub projectiles_per_fire: u32,
    /// `burstPerFire`: how many bursts one press lets go. [data] [game FUN_007ac370]
    pub burst_per_fire: u32,
    /// `timeBetweenEachBurst`, seconds. [data]
    pub time_between_bursts: f32,
    /// `isSpecial` (attributes `+0x5d`) and `isVehicleModeWeapon` (`+0x61`): what keeps a burst
    /// going when its next round is due (`FUN_007ac370` l.41-52, `SpecialFire::step`). [data] [game]
    pub is_special: bool,
    pub vehicle_mode: bool,
    /// `oneOutAtATime` (the mines). [data] Where it is enforced: [unsettled]; the caller counts
    /// the rounds it has out.
    pub one_out_at_a_time: bool,
    /// `launchSpeed`, m/s (`GrenadeLauncher`). [data] [game 00789b80]
    pub launch_speed: f32,
    /// `launchArcAngle`, radians (`weapons.rs` converts). [data]
    pub launch_arc: f32,
    /// `Zgravity`, m/s^2. [data]
    pub z_gravity: f32,
    /// `grenadeMinLifetime`, `grenadeMaxLifetime`: the fuse, seconds. [data] [game 00786a60]
    pub fuse: [f32; 2],
    /// `isProximity`, `proximityRadius` (metres). [data]
    pub is_proximity: bool,
    pub proximity_radius: f32,
    /// `proximityActivateOnCollision`: the first contact arms it. [data] [game 007873f0 rule]
    pub proximity_on_contact: bool,
    /// `isSticky`, and `stickyMinLifetime`, `stickyMaxLifetime`. [data] [game 00786a60]
    pub sticky: bool,
    pub sticky_lifetime: [f32; 2],
    /// `overwriteExplosiveDamage` and `overwriteExplosiveMinDamage` (-1 keeps the object's own).
    /// [data] [game 00789b80 overwrite]
    pub explosive_damage: f32,
    pub explosive_min_damage: f32,
    /// A gun's `bulletSpeed` and `damage` (Megatron's special gun). [data]
    pub bullet_speed: f32,
    pub damage: f32,
    /// `useFacingAsTarget`: the round goes straight ahead of its barrel. [data] (weapons.rs)
    pub use_facing_as_target: bool,
    /// `fireDummyXYAngle` and `fireDummyZAngle`, degrees; the first `fire_angles` are used. [data]
    /// Round `i` of a burst takes entry `i` (see `SpecialFire::step`). [game 007a7a40]
    pub fire_xy: [f32; 8],
    pub fire_z: [f32; 8],
    pub fire_angles: usize,
}

/// The shot of the weapon `def` (with its numbers `t`), its attribute block `attr` read here
/// for the fields `weapons::tuning_from` does not keep. [data]
pub fn shot(def: &WeaponDef, t: &WeaponTuning, attr: Node) -> Shot {
    let num = |name: &str| attr.get(name).and_then(Node::float).unwrap_or(0.0);
    let flag = |name: &str| attr.get(name).is_some_and(Node::is_true);
    let list = |name: &str| attr.get(name).map(Node::floats).unwrap_or_default();
    let (xy, z) = (list("fireDummyXYAngle"), list("fireDummyZAngle"));
    let mut fire_xy = [0.0; 8];
    let mut fire_z = [0.0; 8];
    let fire_angles = xy.len().min(8);
    for i in 0..fire_angles {
        fire_xy[i] = xy[i];
        fire_z[i] = z.get(i).copied().unwrap_or(0.0);
    }
    Shot {
        kind: def.kind,
        rate_of_fire: t.rate_of_fire,
        automatic: t.automatic_firing,
        projectiles_per_fire: t.projectiles_per_fire,
        burst_per_fire: num("burstPerFire").max(1.0) as u32,
        time_between_bursts: num("timeBetweenEachBurst"),
        is_special: flag("isSpecial"),
        vehicle_mode: flag("isVehicleModeWeapon"),
        one_out_at_a_time: flag("oneOutAtATime"),
        launch_speed: t.grenade.launch_speed,
        launch_arc: t.grenade.launch_arc_angle,
        z_gravity: t.grenade.z_gravity,
        fuse: t.grenade.lifetime,
        is_proximity: t.grenade.is_proximity,
        proximity_radius: t.grenade.proximity_radius,
        proximity_on_contact: flag("proximityActivateOnCollision"),
        sticky: flag("isSticky"),
        sticky_lifetime: [num("stickyMinLifetime"), num("stickyMaxLifetime")],
        explosive_damage: t.explosive_damage,
        explosive_min_damage: t.explosive_min_damage,
        bullet_speed: t.bullet_speed,
        damage: t.damage,
        use_facing_as_target: t.use_facing_as_target,
        fire_xy,
        fire_z,
        fire_angles,
    }
}

/// The shot of a form's weapon, found among the character's `defs` by name. `tunings` must be
/// `weapons::load` of the same `defs` (the same order). `attribute_of` gives the attribute
/// block of a weapon from its `WeaponDef::attribute` hash (in `bnxglobal.str`). `None` when the
/// form has no weapon or it is not among the defs.
pub fn shot_for_form<'a>(
    form: &SpecialForm,
    defs: &[WeaponDef],
    tunings: &[WeaponTuning],
    attribute_of: impl Fn(u32) -> Option<Node<'a>>,
) -> Option<Shot> {
    if form.weapon == 0 {
        return None;
    }
    let i = defs.iter().position(|d| d.name == form.weapon)?;
    let attr = attribute_of(defs[i].attribute)?;
    Some(shot(&defs[i], tunings.get(i)?, attr))
}

/// One round to spawn this update: its weapon's numbers and the barrel's turn for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Round {
    pub shot: Shot,
    /// The round's place in its burst (0 for the first).
    pub index: usize,
    /// The turn of this round's barrel, degrees (`fireDummyXYAngle[index]`, `fireDummyZAngle[index]`;
    /// 0 when the weapon has none or the list is shorter). [game] see `SpecialFire::step`.
    pub xy_deg: f32,
    pub z_deg: f32,
}

/// The special's weapon state for one character: when a press may happen, and its bursts.
/// A clip's `WeaponFireSpecialEvent` is one press (`on_fire_event`); there is no stream that
/// runs for the special's duration [data: Megatron's clip has one event, see below].
#[derive(Clone, Debug)]
pub struct SpecialFire {
    forms: [SpecialForm; 2],
    shots: [Option<Shot>; 2],
    /// The form of the press in progress (0 foot, 1 car).
    using: usize,
    /// Seconds until a press is allowed again (`rateOfFire`). [game FUN_007ad080]
    ready_in: f32,
    /// A press taken and not yet fired (its first burst goes in the next `step`).
    pressed: bool,
    /// The weapon's burst count (word `0xfd`: bursts fired of the press, 0 when none is going)
    /// and its burst clock (`0xfe`, seconds). [game 007ae010, 007ac370]
    bursts_done: u32,
    burst_clock: f32,
    /// Time not yet made into whole game updates.
    clock: f32,
    /// The owner is in the drive or the fly mode (`character_in_vehicle_motion` 0070ab90), as
    /// the caller last said (`set_vehicle_motion`).
    vehicle_motion: bool,
}

impl SpecialFire {
    /// `forms` and `shots` are `read_forms` and `shot_for_form` for `[foot, car]`.
    pub fn new(forms: [SpecialForm; 2], shots: [Option<Shot>; 2]) -> Self {
        Self { forms, shots, using: 0, ready_in: 0.0, pressed: false, bursts_done: 0, burst_clock: 0.0, clock: 0.0, vehicle_motion: false }
    }

    /// Whether the owner is in the car's (drive) or the flying form's motion mode now: a
    /// vehicle-mode weapon's burst stops when he is not (`step`). [game 007ac370, 0070ab90]
    pub fn set_vehicle_motion(&mut self, on: bool) {
        self.vehicle_motion = on;
    }

    /// Whether the special's start fires this form's weapon itself, with no animation event:
    /// the form has a weapon and it is not from the animation. [game 007b3d80 param 0, gate S2b]
    /// Jetfire's missiles and the ground mine fire here; Megatron's gun and the car's mine do not.
    pub fn fires_at_start(&self, car: bool) -> bool {
        let f = car as usize;
        self.shots[f].is_some() && !self.forms[f].from_anim
    }

    /// The clip's `WeaponFireSpecialEvent`, or the start for a form that fires at the start
    /// (`fires_at_start`). Each event is one press of the form's weapon: one round, or
    /// `numProjectilesPerFire` rounds if that is above 1, through the normal fire path of
    /// `step`. Whatever its from-animation flag says. [game 007ba870 -> 007b3d80(mgr, 1) ->
    /// 007b3c40] Does nothing while the press is not ready.
    ///
    /// [data] Megatron's `SpecialAttack_Exit` has ONE event, at tick 2 (the gun fires as the
    /// special ends), and no stream follows it. [game 007ac370, 007b3c40] An automatic special
    /// weapon (`automaticFiring` true) does not fire by itself: the weapon's update 007ac370
    /// fires only the rest of a burst (`0xfd > 0`), and the special path calls the held-trigger
    /// `+0xc4` once per event (007b3c40). Megatron's gun has `burstPerFire` 1, so after its
    /// event nothing else fires it. [game 007ac370] the update reads no held flag (its outline:
    /// the burst count, the burst clock, the charge and the since-round clock), and the held path
    /// 007aca50 is called once per press (007b3c40). Not read: the interval gate 007a8f20, which
    /// the held path calls.
    pub fn on_fire_event(&mut self, car: bool) {
        let f = car as usize;
        let Some(shot) = self.shots[f] else { return };
        self.press(f, shot);
    }

    fn press(&mut self, f: usize, shot: Shot) {
        if self.ready_in > 0.0 {
            return;
        }
        self.using = f;
        self.pressed = true;
        self.ready_in = shot.rate_of_fire;
    }

    /// One firing of the weapon (`weapon_fire` 007ae010 l.54-63): its `projectiles_per_fire`
    /// rounds, then the burst count goes up (back to 0, with the clock, once it reaches
    /// `burstPerFire`) and the gap is taken off the clock, which stays at 0 or more. [game]
    fn burst(&mut self, shot: Shot, rounds: &mut Vec<Round>) {
        for index in 0..shot.projectiles_per_fire as usize {
            // Barrel `index` takes the lists' entry `index`; past the lists' end the angles read 0
            // and the barrel is left as it is (the loader leaves missing entries 0; 007a851a-007a853b
            // skips a barrel with both 0). [game 007bbc30, 007a7a40]
            let (xy_deg, z_deg) = if index < shot.fire_angles {
                (shot.fire_xy[index], shot.fire_z[index])
            } else {
                (0.0, 0.0)
            };
            rounds.push(Round { shot, index, xy_deg, z_deg });
        }
        self.bursts_done += 1;
        if self.bursts_done >= shot.burst_per_fire {
            self.bursts_done = 0;
            self.burst_clock = 0.0;
        }
        self.burst_clock = (self.burst_clock - shot.time_between_bursts).max(0.0);
    }

    /// Advances by `dt` seconds and returns the rounds to spawn this update, in order. Each
    /// burst lets go `projectiles_per_fire` rounds at once; bursts are `time_between_bursts`
    /// apart and there are `burst_per_fire` of them. [game FUN_007ae010, FUN_007ac370]
    ///
    /// Jetfire's missile dump: one press = 8 rounds in one burst, 0.3 s ready time.
    /// Carpet bomb: 1 round a burst, 8 bursts 0.5 s apart. Ground mine: one round.
    /// Megatron's gun: one round per clip event (`on_fire_event`).
    ///
    /// A burst runs by itself once pressed, with no trigger held: the weapon's update
    /// (`FUN_007ac370` l.38-84), once per game update while the burst count is above 0, adds
    /// the update to the burst clock and, when the clock has reached `timeBetweenEachBurst`,
    /// fires again (`burst`). So the rounds come on whole updates, the gap's remainder carried:
    /// the carpet bomb's 8 drops 16, 16, 15... updates apart (0.512, 0.512, 0.48 s). When a round
    /// is due the burst goes on only if the weapon `isSpecial`, or, for an `isVehicleModeWeapon`,
    /// while the owner is in the drive or fly mode (`set_vehicle_motion`); else the burst is
    /// dropped (the count set to 0). [game] So Jetfire's bombs stop when he changes back to the
    /// robot. A weapon that is neither goes on while the owner's weapons are out (byte `+0x38`,
    /// `007a75a0`; `weapons::Weapon::set_burst_context`) [game 2026-10-19]; here it goes on, as
    /// none of the specials with more than one burst is such a weapon. [assumed] in the step of
    /// a press, this step's updates run before the press's own firing. [game] 007b3d80 is called
    /// by the special's start (0072afd0) and by the event handler 007ba870, which 00ada700
    /// dispatches; 007ac370 has no direct caller: it is slot `+0x8` (index 2) of the gun and
    /// launcher vtable `00b5fba0` (`find_ptr`, 2026-10-20), and the call through `+0x8` is not
    /// found yet. 2026-10-20: `ability_list_walk` 0071eb50 calls slot `+0x8` of every entry of
    /// char `+0x378` with dt, after the presses (0071dc20 l.146), so a weapon that is an entry of
    /// that list gets 007ac370 after the press in the same update. Whether the gun is such an
    /// entry is not read (no writer of the list head `+0x37c` has been matched to a weapon).
    /// settles: the
    /// caller of the weapon manager's tick that calls the vtable slot holding 007ac370, and its
    /// order against 0072afd0 in the character update (0071dc20).
    /// Third pass: in `character_update` (0071dc20) the state machine's update (`+0x314`, l.79)
    /// runs before the animation rules (l.80) and the clip object's update (l.82), and the weapon
    /// manager's update 007b1850 runs after them (l.137). So the special's start (in the state
    /// machine) and its clip event both come before `manager_update`. Not settled: 007b1850 does
    /// NOT call the weapon's update (it calls no `+0x8`), so the burst tick 007ac370 runs from
    /// some other walk, and its place against 0071dc20 is unknown. The port's order (bursts, then
    /// the press) is therefore [assumed] for the tick.
    /// [game 007ae010, 007a7a40] Round `i` of a burst takes `fireDummyXYAngle[i]` and
    /// `fireDummyZAngle[i]`, as given, not interpolated nor repeated (notes\special-jetfire-mines.md
    /// "fireDummy angles (read)" step 6: the per-barrel index is read; the launch direction is
    /// PARTLY settled there (its steps 5 and 7: the holder read and the Rodrigues sign are open),
    /// so the direction is not claimed here.
    pub fn step(&mut self, dt: f32) -> Vec<Round> {
        let mut rounds = Vec::new();
        let f = self.using;
        let Some(shot) = self.shots[f] else { return rounds };
        self.ready_in = (self.ready_in - dt).max(0.0);
        self.clock += dt;
        while self.clock >= crate::sim::GAME_UPDATE {
            self.clock -= crate::sim::GAME_UPDATE;
            if self.bursts_done == 0 {
                continue;
            }
            self.burst_clock += crate::sim::GAME_UPDATE;
            if shot.time_between_bursts <= self.burst_clock {
                if shot.is_special || !shot.vehicle_mode || self.vehicle_motion {
                    self.burst(shot, &mut rounds);
                } else {
                    self.bursts_done = 0;
                }
            }
        }
        if self.pressed {
            self.pressed = false;
            self.burst(shot, &mut rounds);
        }
        rounds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jetfire's missile dump, from `notes\special-jetfire-mines.md` A (values as the note
    /// records them; not read from the data here).
    fn jetfire_missile_dump() -> Shot {
        Shot {
            kind: Kind::Grenade,
            rate_of_fire: 0.3,
            automatic: false,
            projectiles_per_fire: 8,
            burst_per_fire: 1,
            time_between_bursts: 0.01,
            is_special: false,
            vehicle_mode: false,
            one_out_at_a_time: false,
            launch_speed: 40.0,
            launch_arc: 3.0_f32.to_radians(),
            z_gravity: -32.0,
            fuse: [3.0, 3.0],
            is_proximity: true,
            proximity_radius: 10.0,
            proximity_on_contact: false,
            sticky: false,
            sticky_lifetime: [0.0, 0.0],
            explosive_damage: 30.0,
            explosive_min_damage: 30.0,
            bullet_speed: 0.0,
            damage: 0.0,
            use_facing_as_target: true,
            fire_xy: [0.0, 45.0, 90.0, 135.0, 180.0, -45.0, -90.0, -134.0],
            fire_z: [-40.0; 8],
            fire_angles: 8,
        }
    }

    /// Carpet bomb, `notes\special-jetfire-mines.md` B: 8 bursts of one round, 0.5 s apart.
    fn carpet_bomb() -> Shot {
        Shot {
            rate_of_fire: 4.0,
            projectiles_per_fire: 1,
            burst_per_fire: 8,
            time_between_bursts: 0.5,
            vehicle_mode: true,
            launch_speed: 0.0,
            launch_arc: 0.0,
            is_proximity: true,
            proximity_radius: 10.0,
            sticky: true,
            sticky_lifetime: [0.1, 0.1],
            explosive_min_damage: 25.0,
            fire_xy: [90.0; 8],
            fire_z: [0.0; 8],
            fire_angles: 8,
            use_facing_as_target: false,
            ..jetfire_missile_dump()
        }
    }

    /// Audi's ground mine, `notes\special-jetfire-mines.md` C: one round, a proximity sticky.
    fn audi_mine() -> Shot {
        Shot {
            rate_of_fire: 0.01,
            automatic: true,
            projectiles_per_fire: 1,
            burst_per_fire: 1,
            time_between_bursts: 0.01,
            one_out_at_a_time: true,
            launch_speed: 65.0,
            z_gravity: -32.0,
            fuse: [60.0, 60.0],
            proximity_radius: 15.0,
            proximity_on_contact: true,
            sticky: true,
            sticky_lifetime: [-1.0, -1.0],
            explosive_damage: 30.0,
            explosive_min_damage: -1.0,
            fire_angles: 0,
            ..jetfire_missile_dump()
        }
    }

    /// Megatron's special gun, `notes\special-weapons.md` Megatron: rate 1, automatic, one round
    /// a burst. [data] The gun's damage is not read by the note: 0 here.
    fn megatron_gun() -> Shot {
        Shot {
            kind: Kind::Gun,
            rate_of_fire: 1.0,
            automatic: true,
            projectiles_per_fire: 1,
            burst_per_fire: 1,
            time_between_bursts: 0.01,
            is_proximity: false,
            use_facing_as_target: false,
            fire_angles: 0,
            ..audi_mine()
        }
    }

    fn forms(foot_from_anim: bool, car_from_anim: bool) -> [SpecialForm; 2] {
        [
            SpecialForm { weapon: 1, from_anim: foot_from_anim },
            SpecialForm { weapon: 2, from_anim: car_from_anim },
        ]
    }

    #[test]
    fn special_weapon_jetfire_press_gives_eight_rounds_at_once() {
        // The note: one press loops over numProjectilesPerFire = 8 at once, not 8 spaced out.
        let mut fire = SpecialFire::new(forms(false, true), [Some(jetfire_missile_dump()), None]);
        assert!(fire.fires_at_start(false), "from_anim false: the start fires it");
        fire.on_fire_event(false);
        let rounds = fire.step(0.25);
        assert_eq!(rounds.len(), 8);
        assert_eq!(rounds[5].xy_deg, -45.0);
        assert_eq!(rounds[5].z_deg, -40.0);
        assert!(rounds.iter().all(|r| r.index < 8 && r.shot.explosive_damage == 30.0));
        // Ready time 0.3 s: 0.05 s left after the first step, so a press now is refused;
        // after the next step (0.25 s more) the press is taken.
        fire.on_fire_event(false);
        assert!(fire.step(0.25).is_empty(), "press before ready is refused");
        fire.on_fire_event(false);
        assert_eq!(fire.step(0.25).len(), 8);
    }

    #[test]
    fn special_weapon_carpet_bomb_drops_eight_half_a_second_apart() {
        let mut fire = SpecialFire::new(forms(true, false), [None, Some(carpet_bomb())]);
        assert!(fire.fires_at_start(true) && !fire.fires_at_start(false));
        fire.set_vehicle_motion(true);
        fire.on_fire_event(true);
        let mut updates = Vec::new();
        for update in 0..200 {
            let n = fire.step(crate::sim::GAME_UPDATE).len();
            if n > 0 {
                assert_eq!(n, 1, "one bomb a burst");
                updates.push(update);
            }
        }
        // [game 007ac370, 007ae010] On whole updates, the 0.5 s gap's remainder carried: the first
        // at the press, then 16, 16, 15, 16, 16, 15, 16 updates apart (0.512 s, 0.512 s, 0.48 s...).
        let gaps: Vec<i32> = updates.windows(2).map(|w| w[1] - w[0]).collect();
        println!("carpet bomb drops at updates {updates:?}, gaps {gaps:?}");
        assert_eq!(updates.len(), 8);
        assert_eq!(updates[0], 0);
        assert!(gaps.iter().all(|&g| g == 15 || g == 16), "{gaps:?}");
        let span = (updates[7] - updates[0]) as f32 * crate::sim::GAME_UPDATE;
        assert!((span - 3.5).abs() < 0.04, "seven gaps of 0.5 s: {span}");
    }

    /// A vehicle-mode weapon's burst is dropped when its next round is due and the owner is no
    /// longer in the drive or fly mode: Jetfire turning back into the robot. [game 007ac370 l.44-52, 83]
    #[test]
    fn special_weapon_carpet_bomb_stops_when_he_leaves_the_jet() {
        let mut fire = SpecialFire::new(forms(true, false), [None, Some(carpet_bomb())]);
        fire.set_vehicle_motion(true);
        fire.on_fire_event(true);
        let mut total = 0;
        for update in 0..200 {
            if update == 40 {
                fire.set_vehicle_motion(false);
            }
            if update == 60 {
                // Back in the jet: the dropped burst does not come back.
                fire.set_vehicle_motion(true);
            }
            total += fire.step(crate::sim::GAME_UPDATE).len();
        }
        assert_eq!(total, 3, "the press's bomb and the two due before update 40");
    }

    #[test]
    fn special_weapon_mine_lays_one_round() {
        // Ground mine: fires at the start (from_anim false), one press, one round.
        let mut fire = SpecialFire::new(forms(false, true), [Some(audi_mine()), Some(audi_mine())]);
        assert!(fire.fires_at_start(false));
        fire.on_fire_event(false);
        let mut total = 0;
        for _ in 0..200 {
            total += fire.step(0.25).len();
        }
        assert_eq!(total, 1, "one mine a press, no stream");
        // Car mine: from the animation, not at the start.
        let mut car = SpecialFire::new(forms(false, true), [Some(audi_mine()), Some(audi_mine())]);
        assert!(!car.fires_at_start(true));
        car.on_fire_event(true);
        assert_eq!(car.step(0.25).len(), 1);
    }

    #[test]
    fn special_weapon_megatron_gun_fires_once_per_event() {
        // [data] One event in SpecialAttack_Exit (tick 2): one round, and no stream after it.
        let mut fire = SpecialFire::new(forms(true, false), [Some(megatron_gun()), None]);
        assert!(!fire.fires_at_start(false), "from_anim true: only the event fires it");
        fire.on_fire_event(false);
        let mut rounds = 0;
        for _ in 0..40 {
            rounds += fire.step(0.25).len();
        }
        assert_eq!(rounds, 1, "one event, one round");
        // Events every 0.25 s with rate 1 s: the press at the start fires, the ones before the
        // ready time are refused, and the one at 1.0 s (step 4) fires again.
        let mut fire = SpecialFire::new(forms(true, false), [Some(megatron_gun()), None]);
        let mut hits = Vec::new();
        for step in 0..8 {
            fire.on_fire_event(false);
            if !fire.step(0.25).is_empty() {
                hits.push(step);
            }
        }
        assert_eq!(hits, vec![0, 4]);
    }

    #[test]
    fn special_weapon_megatron_gun_event_with_projectiles_fires_them_together() {
        // numProjectilesPerFire above 1: one event lets go that many rounds at once.
        let mut two = megatron_gun();
        two.projectiles_per_fire = 2;
        let mut fire = SpecialFire::new(forms(true, false), [Some(two), None]);
        fire.on_fire_event(false);
        assert_eq!(fire.step(0.25).len(), 2);
    }

    /// Past the angle lists' end a barrel reads 0 and is left straight: no wrap round the list.
    /// [game 007bbc30, 007a7a40]
    #[test]
    fn special_weapon_rounds_past_the_angle_list_go_straight() {
        let mut ten = jetfire_missile_dump();
        ten.projectiles_per_fire = 10;
        let mut fire = SpecialFire::new(forms(false, true), [Some(ten), None]);
        fire.on_fire_event(false);
        let rounds = fire.step(0.25);
        assert_eq!(rounds.len(), 10);
        assert_eq!((rounds[7].xy_deg, rounds[7].z_deg), (-134.0, -40.0));
        assert_eq!((rounds[8].xy_deg, rounds[8].z_deg), (0.0, 0.0));
        assert_eq!((rounds[9].xy_deg, rounds[9].z_deg), (0.0, 0.0));
    }

    #[test]
    fn special_weapon_form_without_a_weapon_fires_nothing() {
        let mut fire = SpecialFire::new(forms(false, false), [Some(audi_mine()), None]);
        assert!(!fire.fires_at_start(true));
        fire.on_fire_event(true);
        assert!(fire.step(0.25).is_empty());
    }
}
