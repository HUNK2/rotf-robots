//! The control states a character's `stateMachineList` gives it; with `Name:stateX` also the
//! transitions out of that state (connection, target, priority, in list order).
//! `cargo run -p tf2-core --example graph_states -- <AttributesName>[:<state>]...` (reads `C:\Games2`).

use std::path::Path;

fn main() {
    let dir = std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into());
    for arg in std::env::args().skip(1) {
        let (name, state) = arg.split_once(':').map_or((arg.as_str(), None), |(n, s)| (n, Some(s)));
        match tf2_core::tuning::load(Path::new(&dir), name) {
            Ok(t) => {
                let g = &t.control;
                println!("== {name}: {} states, {} connections; stateIdle {:?}", g.states.len(), g.connections.len(), g.find("stateIdle"));
                if let Some(state) = state {
                    let Some(index) = g.find(state) else {
                        println!("  no state {state}");
                        continue;
                    };
                    for tr in &g.states[index].transitions {
                        let target = tr.output.map_or("(back)".to_owned(), |o| g.states[o].name.clone());
                        println!("  {} -> {target}  priority {}", g.connections[tr.connection].name, tr.priority);
                    }
                    continue;
                }
                let names: Vec<String> = g.states.iter().map(|s| format!("{}({})", s.name, s.class)).collect();
                println!("  {}", names.join(", "));
                println!("  missing: {:?}", t.missing);
            }
            Err(e) => println!("== {name}: {e}"),
        }
    }
}
