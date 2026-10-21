//! Prints which install of the game is used, and how it was found.
//! `cargo run -p tf2-core --example install_where`
fn main() {
    let how = if std::env::var_os("TF2_GAME_DIR").is_some() { "TF2_GAME_DIR" } else { "saved folder, registry, or this folder" };
    let dir = tf2_core::install::dir();
    if tf2_core::install::is_install(dir) {
        println!("install: {} (from {how})", dir.display());
    } else {
        println!("no install found{}; run the game once with --game-dir <folder>", if dir.as_os_str().is_empty() { String::new() } else { format!(" at {}", dir.display()) });
    }
}
