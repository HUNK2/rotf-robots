//! Decodes sounds from one of the game's FSB banks to 16-bit PCM WAV files, for listening.
//!
//!   cargo run -p tf2-core --example fsb_wav -- <bank.fsb> <out_dir> <name substring>...
//!
//! Every sample whose name contains one of the substrings (case-insensitive) is written as
//! `<out_dir>/<name>.wav`. The banks hold MPEG audio, so this is the decoded stream, not a
//! lossless original (the game ships none).

use std::io::Write;
use std::path::Path;

use tf2_core::formats::fsb::Bank;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: fsb_wav <bank.fsb> <out_dir> <name substring>...");
        return;
    }
    let bank = Bank::open(Path::new(&args[0])).expect("open bank");
    let out_dir = Path::new(&args[1]);
    std::fs::create_dir_all(out_dir).expect("create out_dir");
    let wanted: Vec<String> = args[2..].iter().map(|s| s.to_lowercase()).collect();
    for (index, entry) in bank.entries.iter().enumerate() {
        if !wanted.iter().any(|w| entry.name.to_lowercase().contains(w)) {
            continue;
        }
        let sample = bank.sample(index).expect("decode sample");
        let stem = entry.name.trim_end_matches(|c| c != '.').trim_end_matches('.');
        let stem = if stem.is_empty() { entry.name.as_str() } else { stem };
        let path = out_dir.join(format!("{stem}.wav"));
        write_wav(&path, sample.rate, sample.channels, &sample.data);
        println!("{} -> {} ({} Hz, {} ch, {:.2} s)", entry.name, path.display(), sample.rate, sample.channels,
            sample.frames() as f32 / sample.rate as f32);
    }
}

fn write_wav(path: &Path, rate: u32, channels: usize, data: &[f32]) {
    let bytes = (data.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels as u16 * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&bytes.to_le_bytes());
    for &s in data {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    std::fs::File::create(path).and_then(|mut f| f.write_all(&out)).expect("write wav");
}
