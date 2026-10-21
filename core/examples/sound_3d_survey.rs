//! What the robots' sound events ask of FMOD's 3D: mode bits, roll-off, distances, and the
//! event header's other words (sound stand-in pass, 2026-10-18).
//!
//!   cargo run -p tf2-core --example sound_3d_survey -- <file.fev>... [--words] [--event NAME]
//!
//! Per file: how many events are placed (mode 0x10) and with which roll-off bit (0x100000
//! logarithmic, 0x200000 linear, 0x4000000 custom), min / max distance ranges, and how many
//! samples are mono or stereo. `--words` tallies each of the 33 header words over all events;
//! `--event NAME` prints one event's header.

use std::collections::BTreeMap;
use tf2_core::formats::fev::Fev;

fn heads(data: &[u8], name: &str) -> Option<[u32; 33]> {
    // An event is `u32 kind | u32 length | name\0 | 33 words`.
    let needle: Vec<u8> = [&((name.len() + 1) as u32).to_le_bytes()[..], name.as_bytes(), &[0]].concat();
    let at = data.windows(needle.len()).position(|w| w == needle.as_slice())?;
    let kind = u32::from_le_bytes(data.get(at.checked_sub(4)?..at)?.try_into().ok()?);
    if kind != 8 && kind != 16 { return None; }
    let start = at + needle.len();
    let mut out = [0u32; 33];
    for (i, w) in out.iter_mut().enumerate() {
        *w = u32::from_le_bytes(data.get(start + i * 4..start + i * 4 + 4)?.try_into().ok()?);
    }
    Some(out)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words = args.iter().any(|a| a == "--words");
    let want = args.iter().position(|a| a == "--event").and_then(|i| args.get(i + 1)).cloned();
    let mut tally: Vec<BTreeMap<u32, usize>> = vec![BTreeMap::new(); 33];
    for path in args.iter().filter(|a| a.to_lowercase().ends_with(".fev")) {
        let Ok(data) = std::fs::read(path) else { println!("{path}: unreadable"); continue };
        let fev = match Fev::parse(&data) { Ok(f) => f, Err(e) => { println!("{path}: {e}"); continue } };
        let mut modes: BTreeMap<u32, usize> = BTreeMap::new();
        let (mut min_lo, mut min_hi, mut max_lo, mut max_hi) = (f32::MAX, 0f32, f32::MAX, 0f32);
        for event in &fev.events {
            *modes.entry(event.mode).or_default() += 1;
            if event.mode & 0x10 != 0 {
                min_lo = min_lo.min(event.min_distance); min_hi = min_hi.max(event.min_distance);
                max_lo = max_lo.min(event.max_distance); max_hi = max_hi.max(event.max_distance);
            }
            if let Some(h) = heads(&data, &event.name) {
                for (i, w) in h.iter().enumerate() { *tally[i].entry(*w).or_default() += 1; }
                // The 2D speaker levels (words 11 and 12: left, right) where they are not 1.
                if h[11] != 0x3f80_0000 || h[12] != 0x3f80_0000 {
                    let has_404 = event.layers.iter().any(|l| l.envelopes.iter().any(|e| e.effect.is_empty() && e.flags == tf2_core::formats::fev::Envelope::SPATIAL_BLEND));
                    println!("    speakers L {} R {} mode {:x} 3d-level envelope {has_404}: {}", f32::from_bits(h[11]), f32::from_bits(h[12]), event.mode, event.name);
                }
                if want.as_deref() == Some(event.name.as_str()) {
                    for (i, w) in h.iter().enumerate() { println!("  {i:2} {w:08x} {}", f32::from_bits(*w)); }
                }
            }
        }
        println!("{path}: {} events; modes {:x?}; placed min {min_lo}..{min_hi} max {max_lo}..{max_hi}",
            fev.events.len(), modes);
        // Per roll-off kind: min/max distances in use, and the doppler level (header word 23).
        let mut kinds: BTreeMap<(u32, u32, u32, u32), usize> = BTreeMap::new();
        for event in fev.events.iter().filter(|e| e.mode & 0x10 != 0) {
            let doppler = heads(&data, &event.name).map_or(u32::MAX, |h| h[23]);
            *kinds.entry((event.mode, event.min_distance.to_bits(), event.max_distance.to_bits(), doppler)).or_default() += 1;
        }
        for ((mode, min, max, doppler), n) in kinds {
            println!("    mode {mode:x} min {} max {} doppler {} x{n}", f32::from_bits(min), f32::from_bits(max), f32::from_bits(doppler));
        }
        // Sample channel counts in the file's banks.
        let dir = std::path::Path::new(path).parent().unwrap_or(std::path::Path::new("."));
        for bank in &fev.banks {
            if let Ok(fsb) = tf2_core::formats::fsb::Bank::open(&dir.join(format!("{bank}.fsb"))) {
                let mut channels: BTreeMap<usize, usize> = BTreeMap::new();
                for entry in &fsb.entries { *channels.entry(entry.channels).or_default() += 1; }
                println!("    bank {bank}: channels {channels:?}");
            }
        }
    }
    if words {
        for (i, t) in tally.iter().enumerate() {
            let mut v: Vec<_> = t.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1));
            let s: Vec<String> = v.iter().take(6).map(|(w, n)| format!("{w:x}({})x{n}", f32::from_bits(**w))).collect();
            println!("{i:2}: {}", s.join(", "));
        }
    }
}
