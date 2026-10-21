//! The AI nodes of packs, as `ai_node` reads and links them: counts by flag, link counts, unresolved
//! connection names, and a sample route. Usage:
//!   cargo run -p tf2-core --example ai_node_dump -- <pack.str> [<pack.str> ...] [--route x y z x y z]
//! Paths are relative to the install (`TF2_GAME_DIR` or `C:\Games2`).

use std::path::PathBuf;

use tf2_core::ai_node::{self, AiNodeGraph, NODE_JUMP, NODE_PATH, NODE_SNIPER, NODE_TELEPORT, NODE_WAYPOINT};

fn main() {
    let dir = PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| r"C:\Games2".into()));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (packs, route): (Vec<&String>, Option<Vec<f32>>) = match args.iter().position(|a| a == "--route") {
        Some(i) => (args[..i].iter().collect(), Some(args[i + 1..].iter().filter_map(|a| a.parse().ok()).collect())),
        None => (args.iter().collect(), None),
    };
    let mut data = Vec::new();
    for p in packs {
        match ai_node::load(&dir.join(p)) {
            Ok(n) => {
                println!("{p}: {} nodes", n.len());
                data.extend(n);
            }
            Err(e) => println!("{p}: {e}"),
        }
    }
    let missing: usize = data.iter().map(|d| d.missing.len()).sum();
    if missing > 0 {
        let first: Vec<&String> = data.iter().flat_map(|d| d.missing.iter()).take(5).collect();
        println!("reader reports: {missing} (first: {first:?})");
    }
    let unresolved: Vec<u32> = {
        let names: std::collections::HashSet<u32> = data.iter().map(|d| d.instance).collect();
        data.iter()
            .flat_map(|d| d.connections.iter().chain(&d.auto_connections).chain(&d.disconnections))
            .copied()
            .filter(|h| !names.contains(h))
            .collect()
    };
    let g = AiNodeGraph::new(data);
    let count = |f: u32| g.nodes.iter().filter(|n| n.data.flags & f != 0).count();
    println!(
        "{} nodes: path {}, waypoint {}, jump {}, sniper {}, teleport {}",
        g.nodes.len(),
        count(NODE_PATH),
        count(NODE_WAYPOINT),
        count(NODE_JUMP),
        count(NODE_SNIPER),
        count(NODE_TELEPORT)
    );
    let links: usize = g.nodes.iter().map(|n| n.links.len()).sum();
    let linked = g.nodes.iter().filter(|n| !n.links.is_empty()).count();
    println!("links {links} on {linked} nodes; unresolved connection targets {}", unresolved.len());
    for (i, n) in g.nodes.iter().enumerate().take(12) {
        let next = g.next_waypoint(i).map(|j| g.nodes[j].data.name.clone());
        println!(
            "  {:<20} {:>8.1} {:>8.1} {:>6.1}  flags {:04x}  r {:>5.0} h {:>4.0}  links {:>2}  next wp {:?}",
            n.data.name, n.data.pos[0], n.data.pos[1], n.data.pos[2], n.data.flags, n.data.radius, n.data.height, n.links.len(), next
        );
    }
    for n in g.nodes.iter().filter(|n| !n.data.missing.is_empty()) {
        println!("  reported: flags {:04x} {:?}", n.data.flags, n.data.missing);
    }
    for h in &unresolved {
        println!("  unresolved target #{h:08x}");
    }
    // Each waypoint chain, from a waypoint node no other node links to.
    let linked_to: std::collections::HashSet<usize> = g.nodes.iter().flat_map(|n| n.links.iter().copied()).collect();
    for (i, n) in g.nodes.iter().enumerate() {
        if n.data.flags & NODE_WAYPOINT != 0 && !linked_to.contains(&i) && g.next_waypoint(i).is_some() {
            let mut chain = vec![n.data.name.clone()];
            let mut at = i;
            while let Some(j) = g.next_waypoint(at) {
                chain.push(g.nodes[j].data.name.clone());
                at = j;
                if chain.len() > 200 {
                    chain.push("...".into());
                    break;
                }
            }
            println!("  chain ({}): {}", chain.len(), chain.join(" > "));
        }
    }
    if let Some(r) = route.filter(|r| r.len() == 6) {
        let route = g.route([r[0], r[1], r[2]], [r[3], r[4], r[5]]);
        let names: Vec<&str> = route.nodes.iter().map(|&i| g.nodes[i].data.name.as_str()).collect();
        println!("route {:?} next {:?}", names, route.next);
    }
}
