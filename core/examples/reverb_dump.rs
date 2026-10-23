//! Every ReverbTrigger / ReverbArea of the given packs: its preset, its holders, and every link that
//! reaches it or one of its holders (who triggers it). [data] Measurement only.
//!
//! cargo run -p tf2-core --example reverb_dump -- <pack.str relative to the game folder>...

use std::path::Path;

use tf2_core::mission_script::{ObjectId, Script};

fn main() {
    let install = tf2_core::install::dir();
    for pack in std::env::args().skip(1) {
        let script = match Script::load(install, Path::new(&pack)) {
            Ok(s) => s,
            Err(e) => {
                println!("{pack}: {e}");
                continue;
            }
        };
        for (i, o) in script.objects.iter().enumerate() {
            if o.type_name != "ReverbTrigger" && o.type_name != "ReverbArea" {
                continue;
            }
            let fields: Vec<String> = script
                .node(ObjectId(i))
                .map(|n| n.fields().iter().map(|(h, v)| format!("{h:08x}={:?}", v.int())).collect())
                .unwrap_or_default();
            let mut chain = vec![ObjectId(i)];
            let mut at = o.parent;
            while let Some(p) = at {
                chain.push(p);
                at = script.object(p).and_then(|x| x.parent);
            }
            let names: Vec<String> = chain
                .iter()
                .filter_map(|&c| script.object(c))
                .map(|x| format!("{}:{}", x.type_name, x.name))
                .collect();
            println!("{pack}: {} id {i} fields {fields:?} holders {names:?} instance {:?}", o.type_name, o.instance);
            let hashes: Vec<u32> = chain.iter().filter_map(|&c| script.object(c)).map(|x| x.name_hash).filter(|&h| h != 0).collect();
            for (j, s) in script.objects.iter().enumerate() {
                for site in &s.links {
                    for l in &site.array.0 {
                        let hit = l.target.is_some_and(|t| chain.contains(&t))
                            || (l.target.is_none() && hashes.contains(&l.target_name))
                            || (s.type_name == "TriggerOnSpawn" && l.target.is_none());
                        if hit {
                            println!(
                                "  <- {}:{} (id {j}) {} to {:08x} msg {:08x} filter {:08x} delay {}-{} resolved {}",
                                s.type_name,
                                s.name,
                                site.path,
                                l.target_name,
                                l.msg_type,
                                l.name_filter,
                                l.delay_min,
                                l.delay_max,
                                l.target.is_some()
                            );
                        }
                    }
                }
            }
        }
    }
}
