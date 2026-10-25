//! Where a zone's loading spends its time: each data chunk decoded on its own, with its size.
//!
//! cargo run -p tf2-core --release --example zone_load_probe -- <zone.str>
use std::io::Write;
use tf2_core::formats::lxb::DataFile;
use tf2_core::formats::pack::{self, Pack};

fn main() {
    let path = std::env::args().nth(1).expect("usage: zone_load_probe <zone.str>");
    let pack = Pack::open(std::path::Path::new(&path)).expect("pack");
    for chunk in pack.of_type(pack::DATA) {
        let data = pack.data(chunk).to_vec();
        print!("chunk {:5}  {:9} bytes  ", chunk.index, data.len());
        std::io::stdout().flush().ok();
        let started = std::time::Instant::now();
        match DataFile::parse(data) {
            Ok(file) => {
                print!("decoded in {:.2?}, {} root fields; ", started.elapsed(), file.root().fields().len());
                std::io::stdout().flush().ok();
                let started = std::time::Instant::now();
                let found = tf2_core::formats::collision::collisions(&file);
                println!("{} collisions found in {:.2?}", found.len(), started.elapsed());
            }
            Err(e) => println!("FAILED: {e}"),
        }
    }
}
