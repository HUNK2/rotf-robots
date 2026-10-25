//! Every placed model of the given packs, one per line: name, position, source. For diffs
//! between two rules of the placement walk.
//!
//! cargo run -p tf2-core --release --example identity_probe -- <pack.str>...
fn main() {
    for path in std::env::args().skip(1) {
        let path = std::path::PathBuf::from(path);
        let render = tf2_core::level_render::load(&path).expect("render");
        for p in &render.placed {
            println!("{}\t{:.1} {:.1} {:.1}\t{}", p.model, p.world[9], p.world[10], p.world[11], p.source);
        }
    }
}
