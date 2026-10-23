//! What each `.fev` holds after its sound definitions: its reverb presets (with `--reverbs`, every
//! property), and the events' reverb wet / dry levels (with `--levels`, a count per value).
//! Measurement only.
//!
//!   cargo run -p tf2-core --example fev_tail -- <file.fev>... [--reverbs] [--levels]

use std::collections::BTreeMap;

use tf2_core::formats::fev::Fev;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let show = args.iter().any(|a| a == "--reverbs");
    let levels = args.iter().any(|a| a == "--levels");
    let mut counts: BTreeMap<(i32, i32, u32), usize> = BTreeMap::new();
    for path in args.iter().filter(|a| a.to_lowercase().ends_with(".fev")) {
        let fev = match std::fs::read(path).map_err(|e| e.to_string()).and_then(|d| Fev::parse(&d)) {
            Ok(f) => f,
            Err(e) => {
                println!("{path}: {e}");
                continue;
            }
        };
        for e in &fev.events {
            *counts.entry(((e.reverb_dry_db * 100.0) as i32, (e.reverb_wet_db * 100.0) as i32, e.mode & 0x18)).or_default() += 1;
        }
        if fev.reverbs.is_empty() {
            continue;
        }
        println!("{path}: project {}, {} reverbs", fev.project, fev.reverbs.len());
        for (i, r) in fev.reverbs.iter().enumerate() {
            if show {
                println!("  {i:2} {:?} {:?}", r.name, r.properties);
            } else {
                println!("  {i:2} {:?}", r.name);
            }
        }
    }
    if levels {
        println!("events by (dry dB x100, wet dB x100, mode & 0x18):");
        for (k, n) in counts {
            println!("  {k:?} {n}");
        }
    }
}
