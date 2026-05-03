//! Path conventions for the daemon's runtime artifacts.
//!
//! Two helpers live here:
//! - [`history_path`] — the JSON file under `$XDG_CACHE_HOME` (with
//!   `/tmp` fallback) that `--persist` mode round-trips.
//! - [`status_path`] — the JSON file under `$XDG_RUNTIME_DIR` (with
//!   `/tmp` fallback) that the waybar bell module reads.
//!
//! Co-located so the filename conventions sit in one place rather
//! than spreading across the I/O modules that consume them.

use std::path::PathBuf;

/// Returns the path to the persisted notification history JSON file.
/// Falls back to `/tmp` if the system can't resolve a cache dir.
pub(crate) fn history_path() -> PathBuf {
    nwg_common::config::paths::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("mac-notifications-history.json")
}

/// Returns the path to the waybar status JSON file.
/// Falls back to `/tmp` if `XDG_RUNTIME_DIR` isn't set.
pub(crate) fn status_path() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join("mac-notifications-status.json")
}
