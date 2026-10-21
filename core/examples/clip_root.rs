//! A clip's root path: its place every `step` ticks (the clip's own frame, x right, y forward).
//! `cargo run -p tf2-core --example clip_root -- <pack> <Set> <clip> [step]`
fn main() {
    let mut a = std::env::args().skip(1);
    let (pack, set, id) = (a.next().unwrap(), a.next().unwrap(), a.next().unwrap());
    let step: usize = a.next().and_then(|s| s.parse().ok()).unwrap_or(10);
    let data = tf2_core::character::load(tf2_core::install::dir(), &pack).expect("load");
    let r = data.animations.find(&set, &id).expect("clip");
    let clip = r.clip.as_ref().unwrap();
    let path = clip.root_path();
    println!("{id}: {} ticks, speed {}", clip.length, r.speed);
    for (i, p) in path.iter().enumerate().step_by(step) {
        println!("  {i:4}  {:8.2} {:8.2} {:8.2}", p.x, p.y, p.z);
    }
    if let Some(p) = path.last() { println!("  last  {:8.2} {:8.2} {:8.2}", p.x, p.y, p.z); }
}
