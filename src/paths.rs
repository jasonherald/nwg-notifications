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
/// `$XDG_CACHE_HOME`, and no `/etc/passwd` entry resolvable via
/// `getpwuid_r`), creates and returns `/tmp/nwg-notifications-<uid>/`
/// with mode `0700`. The per-UID subdirectory bounds two attacks
/// that the previous bare-`/tmp` fallback was vulnerable to:
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
    fallback_user_dir_from(nwg_common::config::paths::cache_dir())
}

/// Inner helper: parameterized over the resolved cache dir so unit
/// tests can simulate the "no cache dir" branch without mocking
/// the global `dirs::cache_dir()` lookup (which traverses
/// `/etc/passwd` and is hard to neutralize via env vars alone).
fn fallback_user_dir_from(cache_dir: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = cache_dir {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Env-var manipulation is process-global, so the four cases
    /// must run serially. A single test function with an internal
    /// mutex guard is simpler than pulling in `serial_test` and
    /// keeps every assertion in one self-contained scope.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env<R>(history_overrides: &[(&str, Option<&str>)], body: impl FnOnce() -> R) -> R {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        // Snapshot the vars we touch so we can restore them.
        let mut snapshot: Vec<(&str, Option<String>)> = Vec::new();
        for (key, _) in history_overrides {
            snapshot.push((key, std::env::var(key).ok()));
        }
        // Apply overrides.
        for (key, value) in history_overrides {
            // SAFETY: env mutation is unsafe in Rust 2024; the
            // ENV_LOCK Mutex serializes our four test cases so no
            // other thread is racing. Other tests in the crate
            // don't touch these vars.
            unsafe {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
        let result = body();
        // Restore.
        for (key, original) in snapshot {
            unsafe {
                match original {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
        result
    }

    #[test]
    fn path_helpers_use_xdg_dirs_when_available_and_per_user_fallback_otherwise() {
        let tmpdir = std::env::temp_dir().join(format!("nwg-paths-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmpdir).expect("setup tmpdir");
        let runtime = tmpdir.join("runtime");
        let cache = tmpdir.join("cache");
        std::fs::create_dir_all(&runtime).expect("setup runtime");
        std::fs::create_dir_all(&cache).expect("setup cache");

        // Case 1: XDG_RUNTIME_DIR set → status_path uses it.
        let actual = with_env(
            &[("XDG_RUNTIME_DIR", Some(runtime.to_str().unwrap()))],
            status_path,
        );
        assert_eq!(actual, runtime.join("mac-notifications-status.json"));

        // Case 2: XDG_CACHE_HOME set → history_path uses it.
        // (cache_dir() reads XDG_CACHE_HOME first.)
        let actual = with_env(
            &[("XDG_CACHE_HOME", Some(cache.to_str().unwrap()))],
            history_path,
        );
        assert_eq!(actual, cache.join("mac-notifications-history.json"));

        // Case 3: XDG_RUNTIME_DIR unset, XDG_CACHE_HOME set →
        // status_path falls back through cache_dir.
        let actual = with_env(
            &[
                ("XDG_RUNTIME_DIR", None),
                ("XDG_CACHE_HOME", Some(cache.to_str().unwrap())),
            ],
            status_path,
        );
        assert_eq!(actual, cache.join("mac-notifications-status.json"));

        // Case 4: cache_dir resolves to None (truly degraded — no
        // $HOME, no $XDG_CACHE_HOME, no /etc/passwd entry) → per-UID
        // /tmp sandbox via fallback_user_dir_from(None). We can't
        // induce that None reliably with env-var manipulation
        // (dirs::cache_dir falls back through /etc/passwd via
        // getpwuid_r, so unsetting HOME doesn't suffice), so test
        // the parameterized inner helper directly.
        // SAFETY: getuid() is always-succeed POSIX FFI.
        let uid = unsafe { libc::getuid() };
        let actual = fallback_user_dir_from(None);
        let expected = PathBuf::from("/tmp").join(format!("nwg-notifications-{uid}"));
        assert_eq!(actual, expected);

        // Sanity: the fallback dir was created with mode 0700.
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&actual)
            .expect("fallback dir created")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode, 0o700,
            "fallback dir must be mode 0700 to bound cross-user reads, got {:o}",
            mode
        );

        // Cleanup.
        let _ = std::fs::remove_dir_all(&tmpdir);
    }
}
