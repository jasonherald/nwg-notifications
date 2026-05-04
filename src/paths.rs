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
///
/// An empty `XDG_RUNTIME_DIR` is treated as unset — `PathBuf::from("")`
/// would otherwise give a relative path that resolves against the
/// process's CWD (whatever `gtk4::Application::run` happened to set
/// it to), which is never what we want for a runtime artifact.
pub(crate) fn status_path() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(fallback_user_dir)
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
    let preferred = PathBuf::from("/tmp").join(format!("nwg-notifications-{uid}"));
    if try_create_or_validate(&preferred, uid) {
        return preferred;
    }
    // The preferred per-UID path failed safety checks (foreign owner,
    // symlink, regular file, or wrong mode that we couldn't fix).
    // Fall through to a per-PID variant to avoid following an
    // attacker-controlled path. Persistence across daemon restarts
    // is sacrificed in this branch — but this whole function only
    // runs in the truly degraded "no XDG dirs at all" environment,
    // where losing history-file persistence is the better trade.
    let pid_suffixed =
        PathBuf::from("/tmp").join(format!("nwg-notifications-{uid}-{}", std::process::id()));
    if try_create_or_validate(&pid_suffixed, uid) {
        log::warn!(
            "Falling back to per-process dir {} because {} failed safety checks; \
             persisted history will not survive daemon restarts in this configuration.",
            pid_suffixed.display(),
            preferred.display()
        );
        return pid_suffixed;
    }
    log::error!(
        "Both {} and {} failed safety checks; subsequent file writes will likely fail.",
        preferred.display(),
        pid_suffixed.display()
    );
    preferred
}

/// Tries to create `dir` with mode `0700` atomically (single
/// `mkdir(2)` call, never `mkdir -p`-style pre-creation). If `dir`
/// already exists, validates that it's a real directory owned by
/// `uid` with mode `0700` — refuses (returns false) if any check
/// fails. Returns true on a successful create or a successful
/// validate of a pre-existing dir.
///
/// Refusing on validation failure is what bounds the symlink-clobber
/// and foreign-owned-dir attacks: if `/tmp/nwg-notifications-<uid>`
/// already exists as a symlink to `/etc/`, or as a directory owned
/// by another user, we won't return its path for subsequent file
/// writes. The caller falls through to a per-PID alternative or
/// logs an error.
fn try_create_or_validate(dir: &std::path::Path, uid: libc::uid_t) -> bool {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            match std::fs::symlink_metadata(dir) {
                Ok(meta) => {
                    let is_dir = meta.file_type().is_dir();
                    let owned = meta.uid() == uid;
                    let mode = meta.permissions().mode() & 0o777;
                    if is_dir && owned && mode == 0o700 {
                        return true;
                    }
                    log::error!(
                        "Fallback dir {} fails safety checks (is_dir={is_dir}, \
                         owned_by_us={owned}, mode={mode:o} expected 0700). Refusing to use.",
                        dir.display()
                    );
                    false
                }
                Err(e) => {
                    log::error!("Failed to stat fallback dir {}: {}", dir.display(), e);
                    false
                }
            }
        }
        Err(e) => {
            log::error!("Failed to create fallback dir {}: {}", dir.display(), e);
            false
        }
    }
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

    #[test]
    fn try_create_or_validate_refuses_when_path_is_a_regular_file() {
        // Pre-create the path as a regular file, then ask
        // try_create_or_validate to handle it. It should refuse
        // (return false) rather than treat the file as a directory
        // — that's the symlink-clobber-class defense at work.
        let tmpdir =
            std::env::temp_dir().join(format!("nwg-paths-safety-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmpdir).expect("setup tmpdir");
        let trap = tmpdir.join("trap-as-file");
        std::fs::write(&trap, b"i am a regular file, not a directory").expect("setup trap");

        // SAFETY: getuid() is always-succeed POSIX FFI.
        let uid = unsafe { libc::getuid() };
        assert!(
            !try_create_or_validate(&trap, uid),
            "try_create_or_validate must refuse a path that exists as a non-directory"
        );

        // Cleanup.
        let _ = std::fs::remove_dir_all(&tmpdir);
    }
}
