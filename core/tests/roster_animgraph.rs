//! The static half of `core\examples\animgraph_check.rs` for the 13 robots: every exit and entry
//! rule of every animation set targets a set and a clip the character has (with an animation
//! file), every `AlternateModelEvent` starts a car-model clip that exists, and every control state
//! finds its clip. The example is included as a module, so the example and the test cannot drift
//! apart; run it for the dynamic half (what a scripted session reaches). Needs the install;
//! returns at once without the install.
//!
//! What holds today is asserted; the rest is printed (`cargo test -p tf2-core --test
//! roster_animgraph -- --nocapture`).

#[allow(dead_code)]
#[path = "../examples/animgraph_check.rs"]
mod checker;

use std::path::PathBuf;

/// Control states whose clip the character's own pack lacks (the control graph is shared by all
/// robots; these are the states of the OTHER variant of a form change, which the port never
/// enters, but the data names a clip the pack does not have). Found 2026-10-20; the test fails if
/// a character gains an error that is not here, and passes if one goes away.
const KNOWN_STATE_ERRORS: [(&str, &[&str]); 13] = [
    ("bumblebee", &["stateDriveExitRun"]),
    ("optimus", &["stateDriveExitRun"]),
    ("ironhide", &[]),
    ("ratchet", &["stateDriveExitRun"]),
    ("megatron", &[]),
    ("longhaul", &["stateDriveExitRun"]),
    ("audir8", &[]),
    ("jetfire", &["stateFlyExit"]),
    ("firestorm", &["stateFlyExit"]),
    ("starscream", &[]),
    ("blackout", &[]),
    ("optimusflight", &["stateFlyEnterA", "stateFlyEnterB", "stateFlyExitA", "stateFlyExitB"]),
    ("megatronflight", &[]),
];

#[test]
fn every_rule_target_model_event_and_state_clip_resolves() {
    let dir = PathBuf::from(tf2_core::install::dir_string());
    if !dir.join("characters/bumblebee.str").exists() {
        return;
    }
    let names = checker::Names::load();
    let (mut total_errors, mut total_warnings) = (0, 0);
    let mut failures: Vec<String> = Vec::new();
    for arg in checker::ROSTER {
        let (pack, t, data) = checker::load_character(&dir, arg).unwrap_or_else(|e| panic!("{arg}: {e}"));
        let r = checker::static_check(&t, &data, &names);
        println!(
            "{pack}: {} sets, {} clips ({} in scope, {} out of scope, {} car model), {} exit rules, {} entry rules, {} states; {} errors, {} warnings; car clips named by no model event {:?}",
            r.sets, r.clips, r.in_scope_clips, r.out_of_scope_clips, r.car_clips, r.exits, r.entries, r.states, r.errors.len(),
            r.warnings.len(), r.car_unplayed
        );
        for e in &r.errors {
            println!("  ERROR {e}");
        }
        for w in &r.warnings {
            println!("  warn  {w}");
        }
        total_errors += r.errors.len();
        total_warnings += r.warnings.len();

        // The data is read at all.
        if r.sets < 15 || r.in_scope_clips < 100 || r.exits < 150 || r.entries < 50 || r.states < 50 {
            failures.push(format!("{pack}: the animation data looks empty ({} sets, {} clips)", r.sets, r.in_scope_clips));
        }
        // Held today for all 13: no rule target is missing, and no model event names a car clip
        // the car model lacks.
        for e in &r.errors {
            if e.contains(": exit -> ") || e.contains(": entry -> ") {
                failures.push(format!("{pack}: a rule target does not resolve: {e}"));
            } else if e.contains("model event starts car clip") {
                failures.push(format!("{pack}: {e}"));
            } else if let Some(state) = e.strip_prefix("state ").and_then(|s| s.split(':').next()) {
                let known = KNOWN_STATE_ERRORS.iter().find(|(p, _)| *p == pack).map_or(&[][..], |(_, s)| *s);
                if !known.contains(&state) {
                    failures.push(format!("{pack}: a new control-state error: {e}"));
                }
            } else {
                failures.push(format!("{pack}: an error of a kind this test does not know: {e}"));
            }
        }
        // The control states' errors stay within the known list, and the list is not stale for a
        // character that has none now (printed, not failed).
        let known = KNOWN_STATE_ERRORS.iter().find(|(p, _)| *p == pack).map_or(&[][..], |(_, s)| *s);
        for k in known {
            if !r.state_errors.iter().any(|s| s == k) {
                println!("  note: {pack}: {k} is no longer a static error; it can come out of KNOWN_STATE_ERRORS");
            }
        }
    }
    println!("{total_errors} errors and {total_warnings} warnings over the roster");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
