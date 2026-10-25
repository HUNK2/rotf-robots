//! The type-coded special abilities 0 to 3: Optimus on foot (0), OptimusFlight (1), the DComScout
//! (2) and the DComSniper (3). What the start and the end send and set, the timer, the shield and
//! melee multipliers while the timer runs, and the scout's speed scale. Pure arithmetic, no engine
//! types. Not wired into the game yet. Source: docs/special-types.md (read 2026-10-12).
//!
//! The caller carries out the `Effect` lists (the messages, the effect spawn, the flags, the
//! speed scale). Not ported: the content of the effects (`OptimusSpecial`, `SpecialAttackMesh`,
//! the `FX_superjump` script, the listener of the sniper's message), the flight damage transfer
//! (x0.75 within 200 units, not applied in any read function), the states the overlay enters, the
//! meaning of the dash flag (`+0x14c` bit 29), and the three status clears other than stun,
//! flashbang and slow (`FUN_0072c120`, `FUN_0072c340`, `FUN_0072adb0`). Types 4 and -1 are in
//! special_weapon.rs and repair.rs.

use crate::formats::hash::crc32;
use crate::formats::lxb::Node;
use crate::tuning::SpecialTuning;

/// The scout's speed scale while its timer runs. [game 0072afd0 case 2, lines 91-93; the literal
/// 0x3fc00000 is 1.5, checked]. The game applies it to the animation rates (0071f4d0 and
/// 00748dc0: every layer and the vehicle form's animator, read 2026-10-20). Whether it also scales
/// movement is OPEN, see `speed_factor`.
pub const SCOUT_SPEED_FACTOR: f32 = 1.5;
/// The scale at the end. [game 0072b980 case 2; the literal 0x3f800000 is 1.0]
pub const NORMAL_SPEED_FACTOR: f32 = 1.0;

/// Sent with Trigger at the start of the Optimus specials. [game 0072afd0 case 0, O2]
pub const SPECIAL_ATTACK_MESH: u32 = crc32(b"SpecialAttackMesh");
/// The effect spawned at the start and stopped at the end. [game 0072afd0 case 0, O3] [data]
pub const OPTIMUS_SPECIAL: u32 = crc32(b"OptimusSpecial");
/// Sent at the end of the Optimus specials. [game 0072b980 case 0, O8] [data: decompiler comment,
/// the hash is checked in the tests but the name is not in names.tsv]
pub const SPECIAL_ATTACK_END: u32 = crc32(b"specialAttackEnd");
/// Sent at the end of every type (the normal mesh again). [game 0072b980 cases 0, 1, 3]
pub const NORMAL_MESH: u32 = crc32(b"NormalMesh");
/// The DComSniper's start message. [game 0072afd0 case 3, R1] Not in names.tsv [unknown name];
/// what listens for it is in the pack and not read.
pub const SNIPER_MESSAGE: u32 = 0xb6ff_7243;
/// The stop message sent to a self-trigger's name at the end. [game 0072b980 lines 9 to 22]
/// Its name is the note's "off" [unknown name, not in names.tsv].
pub const OFF_MESSAGE: u32 = 0x9caa_723d;

/// The four types this module ports, from the character attribute `specialAbilityType`.
/// [game 0072afd0 switch on `attr+0xc0`] [data] `Optimus` 0, `OptimusFlight` 1, `DComScout` 2,
/// `DComSniper` 3. The bool `true` of OptimusFlight's block is read as 1 [game 00726710 at
/// 00727134: the attribute loader stores the scalar of `specialAbilityType` into +0xc0 through
/// view_scalar 00573c20; data global_states.txt line 66201 `true`, Optimus `false` = 0].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialType {
    Optimus,
    OptimusFlight,
    Scout,
    Sniper,
}

impl SpecialType {
    /// The type number, or None for the types that are elsewhere (4 Megatron, -1 none or script).
    pub fn from_kind(kind: i32) -> Option<Self> {
        match kind {
            0 => Some(Self::Optimus),
            1 => Some(Self::OptimusFlight),
            2 => Some(Self::Scout),
            3 => Some(Self::Sniper),
            _ => None,
        }
    }
}

/// One step the caller carries out. Every one is [game] from the start or end function named.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// `stun_effect(0)`. [game 0072e540] (the start of types 0 and 1)
    ClearStun,
    /// `flashbang_effect(0)`. [game 0072e540]
    ClearFlashbang,
    /// `slow_clear` (clock, speed and animation scale). [game 0072e540]
    ClearSlow,
    /// The dash bit (`0x20000000` of `+0x14c`): the OptimusFlight damage-transfer mark. [game 00730e90]
    /// sets it on the character that owns the status block the special runs on: the call passes the
    /// status block as EAX, `00544db0` locks the weak handle at its +0 and +4 (the owner). It clears
    /// it on the object `00730f40` finds (the first marked character). [game 00730e90, 00730f40, read
    /// 2026-10-20; call sites 0072b21a (start) and 0072bb08 (end)]. The transfer itself (x0.75 within
    /// 200 to the marked one) is in `damage::flight_transfer`; the port has no ally to apply it to.
    DashFlag(bool),
    /// The character's speed scale (`char+0xd8`) and its child rates. [game 0071f4d0]
    SpeedScale(f32),
    /// A message with Trigger to the character, by name hash. The walk self-trigger's name is
    /// passed in by the caller (the car's is its own, `specialAbilityVehicleSelfTrigger`).
    Send(u32),
    /// Spawn the named effect on the character's holder. [game 0072afd0 O3, via 00725a40 test]
    /// The caller spawns it only when the holder exists [game 00725a40].
    Spawn(u32),
    /// Stop the effect spawned at the start. [game 0072b980 case 0, O8]
    StopSpawned,
    /// The stop message `OFF_MESSAGE` sent to the self-trigger's name. [game 0072b980 lines 9-22]
    Off(u32),
}

/// The list the start produces, and the list the end produces.
pub type Start = Vec<Effect>;
pub type End = Vec<Effect>;

/// One type's numbers for the shield and the melee, in the two sets. [data] from
/// global_tuning.txt (the shared `Character` root), lines 45 to 54.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Factors {
    /// `optimusSpecialDamageMultiplier` (0.5). Type 0, on the ground branch of damage.
    pub shield: f32,
    /// `optimusFlightSpecialDamageMultiplier` (0.5). Type 1, the flight branch of damage.
    pub flight_shield: f32,
    /// `optimusMeleeSpecialDamageMultiplier` (1.5). Melee of type 0.
    pub melee: f32,
    /// `optimusFlightMeleeSpecialDamageMultiplier` (1.75). Melee of type 1.
    pub flight_melee: f32,
}

/// Both sets: the single-player numbers and the `MP_` ones. [data]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Multipliers {
    pub single: Factors,
    pub multi: Factors,
}

impl Multipliers {
    /// No change at all: every factor 1.0. Used where the data is missing.
    pub const NONE: Multipliers = Multipliers {
        single: Factors { shield: 1.0, flight_shield: 1.0, melee: 1.0, flight_melee: 1.0 },
        multi: Factors { shield: 1.0, flight_shield: 1.0, melee: 1.0, flight_melee: 1.0 },
    };

    /// Reads the eight fields from the shared `Character` root, the same node damage.rs reads its
    /// shield factors from. [data] global_tuning.txt lines 45 to 54: all eight names are there
    /// (checked 2026-10-20), so the 1.0 for a missing field never runs with this data. It is a
    /// Rust fallback, not a game rule: what the game does with a missing field is not read.
    pub fn read(shared: Option<Node<'_>>) -> Self {
        let num = |name: &str| {
            shared
                .and_then(|n| n.get(name))
                .and_then(Node::float)
                .unwrap_or(1.0)
        };
        Self {
            single: Factors {
                shield: num("optimusSpecialDamageMultiplier"),
                flight_shield: num("optimusFlightSpecialDamageMultiplier"),
                melee: num("optimusMeleeSpecialDamageMultiplier"),
                flight_melee: num("optimusFlightMeleeSpecialDamageMultiplier"),
            },
            multi: Factors {
                shield: num("MP_optimusSpecialDamageMultiplier"),
                flight_shield: num("MP_optimusFlightSpecialDamageMultiplier"),
                melee: num("MP_optimusMeleeSpecialDamageMultiplier"),
                flight_melee: num("MP_optimusFlightMeleeSpecialDamageMultiplier"),
            },
        }
    }

    fn pick(&self, multiplayer: bool) -> Factors {
        if multiplayer { self.multi } else { self.single }
    }
}

/// A special that is running. `left` is the timer (`char+0x8c`) in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Active {
    pub kind: SpecialType,
    /// Seconds left on the timer. [game 0072afd0 lines 152-154] set from the duration.
    pub left: f32,
    /// False once `step` has returned the end.
    live: bool,
    /// The self-trigger's name hash for this form (0 for none). [game 0072afd0 lines 39-56]
    trigger: u32,
    mults: Multipliers,
}

impl Active {
    /// The start. `tuning` gives the duration (`specialAbilityDuration`, attr+0xc4) and the
    /// cooldown is the caller's to run (`specialAbilityCooldown`, attr+0xbc) [game 0072bc40].
    /// `trigger` is the form's self-trigger name (`tuning.script` on foot, `tuning.vehicle_script`
    /// in the car), 0 for none. [game 0072afd0 lines 22-56] The game picks the form itself: in the
    /// car (`char+0x31c == 3`) it uses `attr+0xdc` (flag) and `attr+0xe0` (name), on foot
    /// `attr+0xc8` and `attr+0xcc`; the name is sent only when its flag is set, so the caller passes
    /// 0 when the flag is off. Returns the running special and the start list. [game 0072afd0]
    pub fn start(
        kind: SpecialType,
        tuning: &SpecialTuning,
        trigger: u32,
        mults: Multipliers,
    ) -> (Self, Start) {
        let mut start = Vec::new();
        if trigger != 0 {
            start.push(Effect::Send(trigger));
        }
        match kind {
            SpecialType::Optimus | SpecialType::OptimusFlight => {
                start.extend([
                    Effect::ClearStun,
                    Effect::ClearFlashbang,
                    Effect::ClearSlow,
                    Effect::Send(SPECIAL_ATTACK_MESH),
                    Effect::Spawn(OPTIMUS_SPECIAL),
                ]);
                if kind == SpecialType::OptimusFlight {
                    start.push(Effect::DashFlag(true));
                }
            }
            SpecialType::Scout => start.push(Effect::SpeedScale(SCOUT_SPEED_FACTOR)),
            SpecialType::Sniper => start.push(Effect::Send(SNIPER_MESSAGE)),
        }
        // [game 0072afd0 lines 152-154] the timer takes the duration only when it is above 0; with
        // 0 the timer is left as it was (0 for a fresh special) and the end below never runs.
        let left = tuning.duration.max(0.0);
        (
            Self { kind, left, live: true, trigger, mults },
            start,
        )
    }

    /// Runs the timer down by `dt` seconds. Returns the end list once, when it reaches 0.
    /// [game 0072a550 notes only; the end at zero per docs/weapon-mode.md 976-978]
    pub fn step(&mut self, dt: f32) -> Option<End> {
        // [game 0072a550 lines 201-205] the end runs only when the timer is above 0 before the step.
        if !self.live || self.left <= 0.0 {
            return None;
        }
        self.left -= dt;
        if self.left > 0.0 {
            return None;
        }
        self.left = 0.0;
        self.live = false;
        Some(self.end())
    }

    /// Whether the special's timer is running (`+0x8c > 0`).
    pub fn is_on(&self) -> bool {
        self.live && self.left > 0.0
    }

    /// The end list. [game 0072b980] The off message first, then the type's own steps.
    fn end(&self) -> End {
        let mut end = Vec::new();
        if self.trigger != 0 {
            end.push(Effect::Off(self.trigger));
        }
        match self.kind {
            SpecialType::Optimus => end.extend([
                Effect::Send(SPECIAL_ATTACK_END),
                Effect::Send(NORMAL_MESH),
                Effect::StopSpawned,
            ]),
            SpecialType::OptimusFlight => end.extend([
                Effect::DashFlag(false),
                Effect::Send(SPECIAL_ATTACK_END),
                Effect::Send(NORMAL_MESH),
                Effect::StopSpawned,
            ]),
            SpecialType::Scout => end.push(Effect::SpeedScale(NORMAL_SPEED_FACTOR)),
            SpecialType::Sniper => end.push(Effect::Send(NORMAL_MESH)),
        }
        end
    }
}

/// Hit flag bit 23 (`0x0080_0000`): the hit skips the shields. [game 00719830 lines 104 and 129]
pub const FLAG_NO_SHIELD: u32 = 1 << 23;
/// Hit flag bit 27 (`0x0800_0000`): the flight block runs for the hit even with no flight special,
/// and the health floor applies. [game 00719830 lines 128 and 218] Its meaning is [unknown].
pub const FLAG_BIT_27: u32 = 1 << 27;
/// Hit flag bit 28 (`0x1000_0000`): both shield blocks are skipped for the hit. [game 00719830 line
/// 103] Its meaning is [unknown].
pub const FLAG_BIT_28: u32 = 1 << 28;

/// The damage after the shields, as the game's receiver `00719830` does it for a hit with
/// `hit_flags`. [game 00719830 lines 103 to 150] With bit 28 clear: the ground block multiplies by
/// the type-0 factor while the Optimus timer runs (`00721b90`); the flight block multiplies by the
/// flight factor when bit 27 is set or the flight timer runs (`00721bd0`). Both can apply, one
/// after the other. A hit with bit 23 set is not multiplied. `mults` is the global numbers (read
/// through `0079b5e0` / `0079b630`); they apply whether or not a special is running.
/// The caller passes the shield through this function only, not also through damage.rs `resolve`.
pub fn incoming_damage_flags(
    active: Option<&Active>,
    mults: &Multipliers,
    amount: f32,
    hit_flags: u32,
    multiplayer: bool,
) -> f32 {
    if hit_flags & FLAG_BIT_28 != 0 {
        return amount;
    }
    let f = mults.pick(multiplayer);
    let exempt = hit_flags & FLAG_NO_SHIELD != 0;
    let mut damage = amount;
    let ground_on = active.is_some_and(|a| a.kind == SpecialType::Optimus && a.is_on());
    let flight_on = active.is_some_and(|a| a.kind == SpecialType::OptimusFlight && a.is_on());
    if ground_on && !exempt {
        damage *= f.shield;
    }
    if (hit_flags & FLAG_BIT_27 != 0 || flight_on) && !exempt {
        damage *= f.flight_shield;
    }
    damage
}

/// The health floor the game's receiver applies to a hit: 1.0 while either timer runs or the hit
/// has bit 27. [game 00719830 lines 216 to 220: `ground_shield_active || flight_shield_active ||
/// bit 27`, then `health - damage <= 1.0` sets the damage to `health - 1`]
pub fn health_floor_flags(active: Option<&Active>, hit_flags: u32) -> Option<f32> {
    let on = active.is_some_and(|a| {
        a.is_on() && matches!(a.kind, SpecialType::Optimus | SpecialType::OptimusFlight)
    });
    if on || hit_flags & FLAG_BIT_27 != 0 { Some(1.0) } else { None }
}

/// The attacker's melee damage factor while the timer runs. [game 0079b680 / 0079b6d0 read in
/// melee.md 87-90] kind 0 uses the melee factor, kind 1 the flight melee factor; 1.0 otherwise.
pub fn melee_multiplier(active: &Active, multiplayer: bool) -> f32 {
    if !active.is_on() {
        return 1.0;
    }
    let f = active.mults.pick(multiplayer);
    match active.kind {
        SpecialType::Optimus => f.melee,
        SpecialType::OptimusFlight => f.flight_melee,
        _ => 1.0,
    }
}

/// The scout's speed scale: 1.5 while its timer runs, 1.0 otherwise. [game 0071f4d0, 0072b980]
/// The game's `0071f4d0` (the character's vtable slot 0x1c) sets `char+0xd8` and calls the
/// animation controller's `00748dc0`, which rescales each of the three layer players' rates and the
/// vehicle form's animator [game 00748dc0, read 2026-10-20]. So the animation of every layer and of
/// the car or jet model is scaled, not only the action clip. Movement: a literal search of the
/// decompile for `+0xd8` (and `[0x36]`) in the movement units 0071xxxx and 0084xxxx to 0087xxxx finds
/// only the writer 0071f4d0, the attribute reads at 0072afd0 and in the attribute loader 00726100 /
/// 0072656e (attribute +0xd8, not the character's), and 00871d50 (the hover's own surface effect on
/// another object) [game, negative search, 2026-10-20]. So no movement reader of `char+0xd8` is
/// found there. A literal search is not a dataflow: a reader in another unit is not excluded [OPEN].
/// settles: a search over the rest of the executable (every function that loads `char+0xd8`).
pub fn speed_factor(active: &Active) -> f32 {
    if active.kind == SpecialType::Scout && active.is_on() {
        SCOUT_SPEED_FACTOR
    } else {
        NORMAL_SPEED_FACTOR
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test values, copied from global_tuning.txt lines 45 to 54 and the data durations.
    const WALK_TRIGGER: u32 = 0x1234_5678;

    fn mults() -> Multipliers {
        Multipliers {
            single: Factors { shield: 0.5, flight_shield: 0.5, melee: 1.5, flight_melee: 1.75 },
            multi: Factors { shield: 0.5, flight_shield: 0.6, melee: 2.0, flight_melee: 1.5 },
        }
    }

    fn tuning(kind: i32, duration: f32, script: u32) -> SpecialTuning {
        SpecialTuning { cooldown: 0.0, duration, kind, icon: 0, script, vehicle_script: 0, vehicle_anim: 0 }
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn from_kind_maps_the_four_types_only() {
        assert_eq!(SpecialType::from_kind(0), Some(SpecialType::Optimus));
        assert_eq!(SpecialType::from_kind(1), Some(SpecialType::OptimusFlight));
        assert_eq!(SpecialType::from_kind(2), Some(SpecialType::Scout));
        assert_eq!(SpecialType::from_kind(3), Some(SpecialType::Sniper));
        assert_eq!(SpecialType::from_kind(4), None);
        assert_eq!(SpecialType::from_kind(-1), None);
    }

    #[test]
    fn names_hash_to_the_values_in_the_notes() {
        assert_eq!(SPECIAL_ATTACK_MESH, 0x3729_17bd);
        assert_eq!(OPTIMUS_SPECIAL, 0xd2ff_125a);
        assert_eq!(SPECIAL_ATTACK_END, 0x874d_4b6c);
        assert_eq!(NORMAL_MESH, 0x1b44_a61f);
    }

    #[test]
    fn optimus_takes_half_damage_for_ten_seconds_then_full() {
        let (mut a, start) = Active::start(SpecialType::Optimus, &tuning(0, 10.0, 0), 0, mults());
        assert_eq!(
            start,
            vec![
                Effect::ClearStun,
                Effect::ClearFlashbang,
                Effect::ClearSlow,
                Effect::Send(SPECIAL_ATTACK_MESH),
                Effect::Spawn(OPTIMUS_SPECIAL),
            ]
        );
        let dmg = |a: &Active| incoming_damage_flags(Some(a), &a.mults, 100.0, 0, false);
        assert!(near(dmg(&a), 50.0));
        assert!(a.step(9.5).is_none());
        assert!(near(dmg(&a), 50.0));
        let end = a.step(0.5).expect("the timer ends at ten seconds");
        assert!(near(dmg(&a), 100.0));
        assert_eq!(
            end,
            vec![Effect::Send(SPECIAL_ATTACK_END), Effect::Send(NORMAL_MESH), Effect::StopSpawned]
        );
        assert!(a.step(1.0).is_none());
    }

    #[test]
    fn optimus_health_stops_at_one_during_the_shield() {
        let (mut a, _) = Active::start(SpecialType::Optimus, &tuning(0, 10.0, 0), 0, mults());
        let floor = health_floor_flags(Some(&a), 0).expect("shield on");
        assert_eq!(floor, 1.0);
        // 40 health, a 100 hit: 50 after the shield, then clamped to health - 1.
        let dealt = incoming_damage_flags(Some(&a), &a.mults, 100.0, 0, false).min(40.0 - floor);
        assert!(near(dealt, 39.0));
        a.step(10.0);
        assert_eq!(health_floor_flags(Some(&a), 0), None);
    }

    #[test]
    fn melee_is_scaled_only_while_the_timer_runs() {
        let (mut a, _) = Active::start(SpecialType::Optimus, &tuning(0, 10.0, 0), 0, mults());
        assert!(near(melee_multiplier(&a, false), 1.5));
        assert!(near(melee_multiplier(&a, true), 2.0));
        a.step(10.0);
        assert_eq!(melee_multiplier(&a, false), 1.0);
    }

    #[test]
    fn flight_melee_is_one_point_seven_five_and_its_shield_applies_in_flight() {
        let (mut b, start) = Active::start(SpecialType::OptimusFlight, &tuning(1, 10.0, 0), 0, mults());
        assert!(start.contains(&Effect::DashFlag(true)));
        assert!(near(melee_multiplier(&b, false), 1.75));
        assert!(near(melee_multiplier(&b, true), 1.5));
        // In any form: the receiver gates on the running special, not the form. [game 00719830]
        assert!(near(incoming_damage_flags(Some(&b), &b.mults, 100.0, 0, false), 50.0));
        assert!(near(incoming_damage_flags(Some(&b), &b.mults, 100.0, 0, true), 60.0));
        assert_eq!(health_floor_flags(Some(&b), 0), Some(1.0));
        let end = b.step(10.0).expect("ends");
        assert_eq!(
            end,
            vec![
                Effect::DashFlag(false),
                Effect::Send(SPECIAL_ATTACK_END),
                Effect::Send(NORMAL_MESH),
                Effect::StopSpawned,
            ]
        );
        assert!(near(melee_multiplier(&b, false), 1.0));
    }

    #[test]
    fn scout_speed_is_one_point_five_for_five_seconds_then_one() {
        let (mut a, start) = Active::start(SpecialType::Scout, &tuning(2, 5.0, 0), 0, mults());
        assert_eq!(start, vec![Effect::SpeedScale(1.5)]);
        assert_eq!(speed_factor(&a), 1.5);
        assert!(a.step(4.5).is_none());
        assert_eq!(speed_factor(&a), 1.5);
        let end = a.step(0.5).expect("ends at five seconds");
        assert_eq!(end, vec![Effect::SpeedScale(1.0)]);
        assert_eq!(speed_factor(&a), 1.0);
    }

    #[test]
    fn sniper_timer_runs_nine_seconds_and_sends_only_its_message() {
        let (mut a, start) = Active::start(SpecialType::Sniper, &tuning(3, 9.0, 0), 0, mults());
        assert_eq!(start, vec![Effect::Send(SNIPER_MESSAGE)]);
        assert!(a.step(8.5).is_none());
        assert!(a.is_on());
        let end = a.step(0.5).expect("ends at nine seconds");
        assert_eq!(end, vec![Effect::Send(NORMAL_MESH)]);
        assert!(!a.is_on());
        assert!(near(incoming_damage_flags(Some(&a), &a.mults, 100.0, 0, false), 100.0));
    }

    #[test]
    fn end_list_undoes_the_start_list() {
        // Each one-off effect the start sets has its undo in the end.
        let (mut b, start) = Active::start(SpecialType::OptimusFlight, &tuning(1, 10.0, 0), 0, mults());
        let end = b.step(10.0).unwrap();
        assert!(start.contains(&Effect::DashFlag(true)) && end.contains(&Effect::DashFlag(false)));
        assert!(start.contains(&Effect::Spawn(OPTIMUS_SPECIAL)) && end.contains(&Effect::StopSpawned));

        let (mut s, start) = Active::start(SpecialType::Scout, &tuning(2, 5.0, 0), 0, mults());
        let end = s.step(5.0).unwrap();
        assert_eq!(start, vec![Effect::SpeedScale(SCOUT_SPEED_FACTOR)]);
        assert_eq!(end, vec![Effect::SpeedScale(NORMAL_SPEED_FACTOR)]);
    }

    #[test]
    fn self_trigger_is_sent_at_start_and_stopped_at_the_end() {
        let (mut s, start) =
            Active::start(SpecialType::Scout, &tuning(2, 5.0, WALK_TRIGGER), WALK_TRIGGER, mults());
        assert_eq!(start, vec![Effect::Send(WALK_TRIGGER), Effect::SpeedScale(1.5)]);
        let end = s.step(5.0).unwrap();
        assert_eq!(end, vec![Effect::Off(WALK_TRIGGER), Effect::SpeedScale(1.0)]);
    }

    #[test]
    fn a_zero_duration_never_runs_and_never_ends() {
        // [game 0072afd0 lines 152-154, 0072a550 lines 201-205] with duration 0 the timer is not
        // set, so it is not on, and the end (which needs the timer above 0) never runs.
        let (mut a, start) = Active::start(SpecialType::Sniper, &tuning(3, 0.0, 0), 0, mults());
        assert_eq!(start, vec![Effect::Send(SNIPER_MESSAGE)]);
        assert!(!a.is_on());
        assert!(a.step(0.032).is_none());
    }

    #[test]
    fn hit_flags_gate_the_shields_as_the_receiver_does() {
        let (a, _) = Active::start(SpecialType::Optimus, &tuning(0, 10.0, 0), 0, mults());
        let (b, _) = Active::start(SpecialType::OptimusFlight, &tuning(1, 10.0, 0), 0, mults());
        let m = mults();
        // Bit 28 skips both blocks.
        assert!(near(incoming_damage_flags(Some(&a), &m, 100.0, FLAG_BIT_28, false), 100.0));
        // Ground block for the type 0 timer.
        assert!(near(incoming_damage_flags(Some(&a), &m, 100.0, 0, false), 50.0));
        // Flight block for the type 1 timer; the ground block is not for type 1.
        assert!(near(incoming_damage_flags(Some(&b), &m, 100.0, 0, false), 50.0));
        assert!(near(incoming_damage_flags(Some(&b), &m, 100.0, 0, true), 60.0));
        // Bit 23 exempts the hit.
        assert!(near(incoming_damage_flags(Some(&a), &m, 100.0, FLAG_NO_SHIELD, false), 100.0));
        // Bit 27 adds the flight factor even with no special running.
        assert!(near(incoming_damage_flags(None, &m, 100.0, FLAG_BIT_27, false), 50.0));
        assert!(near(incoming_damage_flags(None, &m, 100.0, 0, false), 100.0));
        // Health floor: while a timer runs, or with bit 27.
        assert_eq!(health_floor_flags(Some(&a), 0), Some(1.0));
        assert_eq!(health_floor_flags(Some(&b), 0), Some(1.0));
        assert_eq!(health_floor_flags(None, 0), None);
        assert_eq!(health_floor_flags(None, FLAG_BIT_27), Some(1.0));
    }

    #[test]
    fn no_data_means_no_change() {
        let (a, _) = Active::start(SpecialType::Optimus, &tuning(0, 10.0, 0), 0, Multipliers::NONE);
        assert!(near(incoming_damage_flags(Some(&a), &Multipliers::NONE, 100.0, 0, false), 100.0));
        assert!(near(melee_multiplier(&a, false), 1.0));
    }
}
