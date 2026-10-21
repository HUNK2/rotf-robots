//! Where the user's install of the game is.
//!
//! Looked for, in order: `TF2_GAME_DIR` (when set it is used as it is, found or not, so a run
//! can be pointed away from the install), the folder saved by [`remember`] (the game's
//! `--game-dir` saves it), the installer's registry entry on Windows, then the program's own
//! folder and the working folder. A folder counts as an install when it holds [`MARKER`].

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The pack at the root of every install, used to recognise one.
pub const MARKER: &str = "bnxglobal.str";

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// True when `dir` holds an install.
pub fn is_install(dir: &Path) -> bool {
    dir.join(MARKER).is_file()
}

/// Looks for the install (see the module), without caching.
pub fn find() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("TF2_GAME_DIR") {
        return Some(PathBuf::from(dir));
    }
    let saved = saved_file().and_then(|f| std::fs::read_to_string(f).ok()).map(|s| PathBuf::from(s.trim()));
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_owned));
    saved.into_iter().chain(registry()).chain(beside).chain(std::env::current_dir().ok()).find(|d| is_install(d))
}

/// The install, looked for once. An empty path when nothing was found: what is read under it
/// is then missing, and the tests that need the install skip themselves.
pub fn dir() -> &'static Path {
    DIR.get_or_init(|| find().unwrap_or_default())
}

/// [`dir`] as a string, for the places that build their paths from one.
pub fn dir_string() -> String {
    dir().to_string_lossy().into_owned()
}

/// For the tests that read the install: true, with a note, when it was not found, so the test
/// returns instead of failing on a machine without the game.
pub fn missing() -> bool {
    let gone = !is_install(dir());
    if gone {
        eprintln!("skipped: the game's install was not found");
    }
    gone
}

/// Uses `dir` for the rest of the run. False when the install was already looked up.
pub fn set(dir: PathBuf) -> bool {
    DIR.set(dir).is_ok()
}

/// Saves `dir` so later runs find it without being told.
pub fn remember(dir: &Path) -> std::io::Result<PathBuf> {
    let file = saved_file().ok_or_else(|| std::io::Error::other("no settings folder (APPDATA, XDG_CONFIG_HOME or HOME)"))?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&file, dir.to_string_lossy().as_bytes())?;
    Ok(file)
}

/// `rotf-robots\game-dir.txt` under the user's settings folder.
fn saved_file() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("XDG_CONFIG_HOME")).map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("rotf-robots").join("game-dir.txt"))
}

/// The installer's `InstallPath` under `Activision\Transformers2` (read through `reg`, so no
/// crate is needed; only that one value is asked for).
#[cfg(windows)]
fn registry() -> Option<PathBuf> {
    ["HKLM\\SOFTWARE\\WOW6432Node\\Activision\\Transformers2", "HKLM\\SOFTWARE\\Activision\\Transformers2"].iter().find_map(|key| {
        let out = std::process::Command::new("reg").args(["query", key, "/v", "InstallPath"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.lines().find(|l| l.contains("REG_SZ"))?;
        Some(PathBuf::from(line.split("REG_SZ").nth(1)?.trim()))
    })
}

#[cfg(not(windows))]
fn registry() -> Option<PathBuf> {
    None
}
