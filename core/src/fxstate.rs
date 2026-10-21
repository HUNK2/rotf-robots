//! The effect messages a state of his keeps on: who sends `Trigger` and the stop
//! (`9caa723d`) for each, read from the game's code (`notes\particles.md` "The senders").
//!
//! A message goes to his object by a `LuxScriptName` (`FUN_00724940`, and again to the
//! object at `+600`); the things that answer to the name start, or stop.

use crate::formats::hash::crc32;
use crate::sim::{Mode, State};
use crate::tuning::Tuning;

pub const EXHAUST: u32 = crc32(b"FX_smoke_exhaust");
pub const HEADLIGHT_L: u32 = crc32(b"Headlight_L");
pub const HEADLIGHT_R: u32 = crc32(b"Headlight_R");
pub const TURBO: u32 = crc32(b"FX_turbocharged");
pub const DAMAGE: [u32; 3] = [crc32(b"FX_Damage_1"), crc32(b"FX_Damage_2"), crc32(b"FX_Damage_3")];
pub const REGENERATE: u32 = crc32(b"FX_regenerate");

/// Every name `kept` can give: an effect running under one of these and not listed is
/// told to stop.
pub const STUN: u32 = crc32(b"FX_stun");
pub const FROST: u32 = crc32(b"FX_frost");
/// FlyMode's brake (`ability_effects` 007325e0). [game]
pub const BRAKE: u32 = crc32(b"FX_brake");
/// HoverMode's thrusters (`hover_fx_start` 00735bb0). [game]
pub const HOVER: u32 = crc32(b"FX_hover");
pub const KEPT: [u32; 12] = [EXHAUST, HEADLIGHT_L, HEADLIGHT_R, TURBO, DAMAGE[0], DAMAGE[1], DAMAGE[2], REGENERATE, STUN, FROST, BRAKE, HOVER];

/// The damage level of `FUN_0072a270`, from health over the most: 0 under a quarter, 1
/// under half, 2 under three quarters, else 3. [game]
pub fn damage_level(fraction: f32) -> u8 {
    if fraction < 0.25 {
        0
    } else if fraction < 0.5 {
        1
    } else if fraction < 0.75 {
        2
    } else {
        3
    }
}

/// The messages that are on.
///
/// - Entering DriveMode triggers both headlights and the exhaust; leaving it stops them
///   (`FUN_0085e130`, `FUN_0085ed50`). [game] A headlight answers only where the level
///   says so (`FUN_00815790`: a switch at `+0x100` of the level's entry in the list at
///   `00dad9a0`): `headlights`.
/// - The turbo: `FX_turbocharged` is triggered when it goes on and stopped when it goes
///   off (`FUN_00733290`, `FUN_00733ae0`, the character's two virtual "on / off"
///   routines, with `Environment_SpeedBlur`). [game: 00b5d61c/00b5d630 slot4, called
///   by 00732e60 and the timers' expiry; DriveMode exit stops FX without resetting
///   the standard timer]. Keep the last on/off message, including independent stops
///   if the two turbo abilities overlap.
/// - The damage smoke (`FUN_0072a270`): as the level falls `FX_Damage_3` is triggered at
///   level 0, `FX_Damage_2` under 2, `FX_Damage_1` under 3; as it rises each is stopped
///   again. [game]
/// - `FX_regenerate` from the start of the health coming back (`FUN_0071fdd0`: not yet
///   regenerating and health under the most) until it is full or he is hurt
///   (`FUN_0071ff00`). [game]
///
/// In the flying form (FlyMode, 00864320 and the timed abilities it runs) [game]:
/// - `FX_smoke_exhaust` is NOT sent by the mode's enter: the take-off clip sends it (the jets'
///   model clip `Trans_R2V_B`, the robot flyers' `FlySet/Trans_Robot2Vehicle`, an
///   `AnimSendTriggerEvent`) [data]. The brake's start stops it and its end sends it again
///   (`ability_effects` 007325e0, from 00864320 and `timed_ability_update` 007324d0); the exit
///   stops it (00865b80). `fly_exhaust` is that latch, kept by the host, which sees the model
///   clip's triggers.
/// - `FX_brake` from the brake's start to the end of its time (007325e0 on / off).
/// - `FX_turbocharged`: the fly turbo is the same turbo ability class, whose on / off routine
///   (00733290) sends it; the exit turns it off (00865b80 l.199-201).
///
/// In HoverMode: `FX_hover` from the enter (008713c0 -> 00735bb0(1, 0)) until the hover timer
/// is parked (its slot 1, 007359e0 -> 00735bb0(0, 1): stops `FX_hover` and `FX_hover_sput`),
/// which the exit (00871580) and `HoverDone` do. [game] `FX_hover_sput` (the sputter) is sent
/// only when 00735bb0(1, 0) runs while the motion mode is not HoverMode (the timer's first
/// half second, 00735a00): not modelled [assumed: never seen in play; settles: when the hover
/// timer ticks outside HoverMode].
pub fn kept(s: &State, t: &Tuning, headlights: bool, fly_exhaust: bool) -> Vec<u32> {
    let mut on = Vec::new();
    if s.mode == Mode::Drive {
        on.push(EXHAUST);
        if headlights {
            on.extend([HEADLIGHT_L, HEADLIGHT_R]);
        }
        if s.turbo_fx_on {
            on.push(TURBO);
        }
    }
    if s.mode == Mode::Fly {
        let braking = s.fly_timers.brake_left > 0.0;
        if braking {
            on.push(BRAKE);
        } else if fly_exhaust {
            on.push(EXHAUST);
        }
        if s.fly_timers.turbo_left > 0.0 {
            on.push(TURBO);
        }
    }
    if s.mode == Mode::Hover {
        on.push(HOVER);
    }
    let level = damage_level(s.health / t.base_health.max(1e-5)) as usize;
    on.extend(DAMAGE.iter().enumerate().filter(|(i, _)| level < 3 - i).map(|(_, name)| *name));
    if s.damage_timers.regenerating && s.health > 0.0 && s.health < t.base_health {
        on.push(REGENERATE);
    }
    if s.damage_timers.stunned > 0.0 || s.damage_timers.disabled_vehicle > 0.0 || s.damage_timers.disabled_transform > 0.0 { on.push(STUN); }
    if s.damage_timers.slowed>0.0 {on.push(FROST);}
    on
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_damage_smoke_goes_by_quarters_of_his_health() {
        assert_eq!([0.1, 0.25, 0.49, 0.5, 0.74, 0.75, 1.0].map(damage_level), [0, 1, 1, 2, 2, 3, 3]);
    }
}
