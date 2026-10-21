//! One control state's definition as the port loads it (Debug print), with its connections.
//! `cargo run -p tf2-core --example state_def -- <AttributesName> <stateName>`
fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_else(|| "Megatron".into());
    let state = args.next().unwrap_or_else(|| "stateAttackSpecialMegatronVehicle".into());
    let t = tf2_core::tuning::load(std::path::Path::new("C:/Games2"), &name).expect("tuning");
    for s in t.control.states.iter().filter(|s| s.name == state) {
        println!("{s:#?}");
        for tr in &s.transitions {
            println!("connection {}: {:#?}", tr.connection, t.control.connections.get(tr.connection));
            if let Some(out) = tr.output {
                println!("  -> {}", t.control.states[out].name);
            }
        }
    }
}
