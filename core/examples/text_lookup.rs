//! Prints the English global text of each id given (`descStrId`-style hashes, signed or not) or key name.
//!
//! cargo run -p tf2-core --example text_lookup -- 1434185698 -1122498892
use tf2_core::formats::textbank::TextBank;

fn main() {
    let bank = TextBank::global(tf2_core::install::dir()).expect("text bank");
    for a in std::env::args().skip(1) {
        match a.parse::<i64>() {
            Ok(v) => println!("{a}: {:?}", bank.by_hash(v as u32)),
            Err(_) => println!("{a}: {:?}", bank.get(&a)),
        }
    }
}
