//! Test aid, not the game: F6 / F7 change to the next / previous robot of the roster in place.
//! The game is started again with the other `--character` and a `--start` at where he stands,
//! facing where he faces, every other argument kept; then this one quits. Starting again is
//! the honest way: a robot is its own pack, model, animation sets, weapons and sound banks, all
//! loaded at the start (`main`), and nothing here unloads one.

use bevy::prelude::*;

use crate::player::Player;

/// The 13 distinct playable robots, in the unpublished notes's order: the pack, then the
/// name the game's character list gives it (either is accepted by `--character`); then
/// Devastator, whom the game never lets anyone play (the walking giant, `tf2_core::giant`).
pub const ROSTER: [(&str, &str); 14] = [
    ("bumblebee", "BUMBLEBEE"),
    ("optimus", "OPTIMUS PRIME"),
    ("ratchet", "RATCHET"),
    ("ironhide", "IRONHIDE"),
    ("megatron", "MEGATRON"),
    ("longhaul", "LONG HAUL"),
    ("audir8", "SIDEWAYS"),
    ("jetfire", "JETFIRE"),
    ("firestorm", "BREAKAWAY"),
    ("starscream", "STARSCREAM"),
    ("blackout", "GRINDOR"),
    ("optimusflight", "POWERED-UP OPTIMUS"),
    ("megatronflight", "MEGATRON (FLIGHT)"),
    ("devastator", "DEVASTATOR"),
];

/// Where `--character` (a pack or a name, any case) sits in `ROSTER`; no flag is Bumblebee,
/// the default character. Anything else (a reskin, an enemy pack) is not in the cycle: None.
pub fn index_of(character: Option<&str>) -> Option<usize> {
    let Some(character) = character else { return Some(0) };
    let wanted = character.split_once(':').map_or(character, |(pack, _)| pack).to_ascii_lowercase();
    ROSTER.iter().position(|(pack, name)| *pack == wanted || name.to_ascii_lowercase() == wanted)
}

/// The arguments to start again with: `--character` and `--start` replaced, the rest kept.
/// `yaw` in radians, the sim's convention; `--start` takes degrees.
pub fn next_args(args: &[String], step: isize, x: f32, y: f32, yaw: f32) -> Vec<String> {
    let current = args.iter().position(|a| a == "--character").and_then(|i| args.get(i + 1)).map(String::as_str);
    let from = index_of(current).unwrap_or(ROSTER.len() - 1);
    let to = (from as isize + step).rem_euclid(ROSTER.len() as isize) as usize;
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        // `--robot-switch-at` is used once: the game started again does not switch again.
        if args[i] == "--character" || args[i] == "--start" || args[i] == "--robot-switch-at" {
            i += 2;
            continue;
        }
        out.push(args[i].clone());
        i += 1;
    }
    out.extend(["--character".into(), ROSTER[to].0.into(), "--start".into(), format!("{x:.2},{y:.2},{:.1}", yaw.to_degrees())]);
    out
}

pub fn run(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    shot: Option<Res<crate::Shot>>,
    player: Single<&Player>,
    mut exit: MessageWriter<AppExit>,
    mut done: Local<bool>,
) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Test aid for captures: `--robot-switch-at <s>` switches to the next robot once, at that time
    // (a capture's keys are its own script, so F6 / F7 are not read under one).
    let scripted = args.iter().position(|a| a == "--robot-switch-at").and_then(|i| args.get(i + 1)).and_then(|s| s.parse::<f32>().ok());
    let step = match scripted {
        Some(at) if !*done && time.elapsed_secs() >= at => 1,
        _ if shot.is_some() || scripted.is_some() => return,
        _ if keys.just_pressed(KeyCode::F6) => 1,
        _ if keys.just_pressed(KeyCode::F7) => -1,
        _ => return,
    };
    *done = true;
    let state = &player.state;
    let next = next_args(&args, step, state.pos.x, state.pos.y, state.yaw);
    let Ok(exe) = std::env::current_exe() else {
        eprintln!("switch: cannot find this program to start again");
        return;
    };
    match std::process::Command::new(exe).args(&next).spawn() {
        Ok(_) => {
            eprintln!("switch: starting again as {}", next[next.len() - 3]);
            exit.write(AppExit::Success);
        }
        Err(e) => eprintln!("switch: could not start again ({e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_switch_cycles_the_roster_and_keeps_the_other_arguments() {
        // No flag is Bumblebee: next is Optimus, previous wraps to Devastator (last since 2026-10-22).
        let next = next_args(&args(&["--fps"]), 1, 1.0, 2.0, 0.0);
        assert_eq!(next, args(&["--fps", "--character", "optimus", "--start", "1.00,2.00,0.0"]));
        assert_eq!(next_args(&[], -1, 0.0, 0.0, 0.0)[1], "devastator");
        assert_eq!(next_args(&args(&["--character", "devastator"]), 1, 0.0, 0.0, 0.0)[1], "bumblebee");
        // A name is found as well as a pack; the old --start is replaced, not doubled.
        let from_name = next_args(&args(&["--character", "Long Haul", "--start", "9,9,9", "--mission", "X"]), 1, 3.0, 4.0, std::f32::consts::FRAC_PI_2);
        assert_eq!(from_name, args(&["--mission", "X", "--character", "audir8", "--start", "3.00,4.00,90.0"]));
        assert_eq!(next_args(&args(&["--character", "megatronflight"]), 1, 0.0, 0.0, 0.0)[1], "devastator");
        // Something outside the cycle starts it again from the top.
        assert_eq!(index_of(Some("optimusgold")), None);
        assert_eq!(next_args(&args(&["--character", "optimusgold"]), 1, 0.0, 0.0, 0.0)[1], "bumblebee");
        // The scripted switch is not passed on.
        assert_eq!(next_args(&args(&["--robot-switch-at", "0.5", "--shot", "a.png"]), 1, 0.0, 0.0, 0.0)[..2], args(&["--shot", "a.png"])[..]);
    }
}
