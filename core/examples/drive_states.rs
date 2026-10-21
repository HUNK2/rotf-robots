//! The drive and fly control states of a character and the clip and set each names.
//! `cargo run -p tf2-core --example drive_states -- <AttributesName>`
fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "Bumblebee".into());
    let t = tf2_core::tuning::load(std::path::Path::new("C:/Games2"), &name).expect("tuning");
    for s in &t.control.states {
        if matches!(s.class.as_str(), "StateDrive" | "StateFly" | "StateFlyExit") {
            println!("{:32} {:14} anim {:08x} set {:?}", s.name, s.class, s.anim, s.anim_set);
        }
    }
}
