//! Path conventions for the daemon's runtime artifacts.
//!
//! Two helpers live here:
//! - [`history_path`] — the JSON file under `$XDG_CACHE_HOME` that
//!   `--persist` mode round-trips.
//! - [`status_path`] — the JSON file under `$XDG_RUNTIME_DIR` that
//!   the waybar bell module reads.
//!
//! Co-located so the filename conventions sit in one place rather
//! than spreading across the I/O modules that consume them.
//!
//! **Fallbacks are per-user, not `/tmp`.** Both helpers prefer the
//! XDG-resolved directory, then fall through to
//! `nwg_common::config::paths::cache_dir()` (which itself walks
//! `XDG_CACHE_HOME` → `dirs::cache_dir()`). Only when even that
//! fails do we reach for `/tmp`, and even then we sandbox into a
//! per-UID subdirectory with mode `0700` — never the world-writable
//! root of `/tmp` directly. Writing notification history or the
//! waybar status JSON to `/tmp/mac-notifications-*.json` would
//! expose them to symlink-clobber attacks (an attacker pre-creating
//! the path as a symlink to a victim file) and to cross-user reads
//! under permissive umasks. The per-UID subdir bounds both.

use std::path::PathBuf;

/// Returns the path to the persisted notification history JSON file.
/// Prefers `nwg_common`'s XDG-aware cache dir; falls back to a
/// per-UID sandbox under `/tmp` if no XDG cache dir resolves.
pub(crate) fn history_path() -> PathBuf {
    fallback_user_dir().join("mac-notifications-history.json")
}

/// Returns the path to the waybar status JSON file.
/// Prefers `$XDG_RUNTIME_DIR`; falls back to `cache_dir()` and
/// finally to a per-UID sandbox under `/tmp` if neither resolves.
pub(crate) fn status_path() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| fallback_user_dir())
        .join("mac-notifications-status.json")
}

/// Returns a directory the current user owns and can write to.
/// Tries `cache_dir()` first; if that fails (no `$HOME`, no
/// `$XDG_CACHE_HOME`), creates and returns
/// `/tmp/nwg-notifications-<uid>/` with mode `0700`. The per-UID
/// subdirectory bounds two attacks that the previous bare-`/tmp`
/// fallback was vulnerable to:
/// - **Symlink-clobber on write.** An attacker who can predict the
///   filename `/tmp/mac-notifications-history.json` could
///   pre-create it as a symlink to a victim file before the daemon
///   writes. Putting the file inside `/tmp/nwg-notifications-<uid>/`
///   means the attacker would have to win the same race for a
///   directory they don't own.
/// - **Cross-user reads.** `std::fs::write` honors the process
///   umask, which on permissive systems leaves the file
///   world-readable. Mode `0700` on the parent dir blocks reads
///   regardless of file mode.
fn fallback_user_dir() -> PathBuf {
    if let Some(dir) = nwg_common::config::paths::cache_dir() {
        return dir;
    }
    // SAFETY: `getuid()` is documented to always succeed and never
    // return an error per POSIX. The `unsafe` is purely the FFI tag.
    let uid = unsafe { libc::getuid() };
    let dir = PathBuf::from("/tmp").join(format!("nwg-notifications-{uid}"));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::error!(
            "Failed to create per-user fallback dir {}: {}",
            dir.display(),
            e
        );
    }
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)) {
        log::warn!(
            "Failed to set 0700 perms on fallback dir {}: {}",
            dir.display(),
            e
        );
    }
    dir
}
