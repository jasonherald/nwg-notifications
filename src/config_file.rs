//! JSON config-file load + atomic-save for `NotificationConfig`.
//!
//! The file lives at `paths::config_path()` (typically
//! `~/.config/nwg-notifications/config.json`). Schema is the
//! `NotificationConfig` struct itself, with serde-derived
//! kebab-case enum encoding for `PopupPosition`. Every field is
//! `#[serde(default)]` so missing keys fall back to compiled
//! defaults.
//!
//! `save` uses `tempfile::NamedTempFile::persist` for an atomic
//! same-filesystem rename, with a `sync_all` fsync before the rename —
//! both a kill mid-write *and* a power loss leave a consistent file
//! (either the previous content or the new content, never half-written).

use crate::config::NotificationConfig;
use std::io;
use std::io::Write;
use std::path::Path;

/// Errors that can come out of [`load`] and [`save`].
#[derive(Debug)]
pub(crate) enum ConfigFileError {
    /// The config file doesn't exist at the requested path. Distinct
    /// from other I/O errors so the caller can distinguish "first
    /// run, write the default" from "I can't read this for some
    /// other reason."
    NotFound,
    /// `serde_json` couldn't parse the file. The original `serde_json::Error`
    /// is preserved so the operator can see the error in logs.
    Parse(serde_json::Error),
    /// Anything else — read failure, write failure, atomic-rename
    /// failure, etc.
    Io(io::Error),
}

impl std::fmt::Display for ConfigFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigFileError::NotFound => write!(f, "config file not found"),
            ConfigFileError::Parse(e) => write!(f, "config file parse error: {e}"),
            ConfigFileError::Io(e) => write!(f, "config file I/O error: {e}"),
        }
    }
}

impl std::error::Error for ConfigFileError {}

/// Load and parse the JSON config file at `path`. Missing keys fall
/// back to the `NotificationConfig::default()` impl that mirrors
/// clap's `default_value_t`s.
pub(crate) fn load(path: &Path) -> Result<NotificationConfig, ConfigFileError> {
    let contents = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(ConfigFileError::NotFound),
        Err(e) => return Err(ConfigFileError::Io(e)),
    };
    serde_json::from_str(&contents).map_err(ConfigFileError::Parse)
}

/// Atomically write `config` to `path`. Writes to a same-directory
/// temp file first, then renames into place. If the parent
/// directory doesn't exist, it's created.
pub(crate) fn save(path: &Path, config: &NotificationConfig) -> Result<(), ConfigFileError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(ConfigFileError::Io)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(ConfigFileError::Io)?;
    serde_json::to_writer_pretty(&mut tmp, config).map_err(ConfigFileError::Parse)?;
    // serde_json::to_writer_pretty doesn't add a trailing newline; add
    // one so editors that strip-trailing-whitespace don't fight us.
    tmp.write_all(b"\n").map_err(ConfigFileError::Io)?;
    // Force the data to disk before the rename so a power-loss
    // between the rename(2) commit and the data-block flush can't
    // leave a zero-byte file. Per spec: tempfile + fsync + rename.
    tmp.as_file().sync_all().map_err(ConfigFileError::Io)?;
    tmp.persist(path)
        .map_err(|e| ConfigFileError::Io(e.error))?;
    Ok(())
}

/// Boot-time loader: try to load the config from `path`. If the file
/// doesn't exist, write the compiled-in defaults to it and return
/// those (first-run UX). If the file exists but won't parse, log
/// the error and return defaults *without* writing — overwriting
/// a malformed file the user is mid-editing is the wrong thing.
///
/// Returns `NotificationConfig` unconditionally — by the time this
/// returns, the daemon either has a usable config or has logged
/// what went wrong.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "caller lands in Task 5 (main.rs startup wiring); this helper is dead-code only until then"
    )
)]
pub(crate) fn load_or_create_default(path: &Path) -> NotificationConfig {
    match load(path) {
        Ok(config) => config,
        Err(ConfigFileError::NotFound) => {
            log::info!(
                "Config file {} does not exist; writing defaults",
                path.display()
            );
            let defaults = NotificationConfig::default();
            if let Err(e) = save(path, &defaults) {
                log::warn!("Failed to write default config to {}: {e}", path.display());
            }
            defaults
        }
        Err(e) => {
            log::error!(
                "Failed to load config from {}: {e}; falling back to defaults",
                path.display()
            );
            NotificationConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nwg-config-file-test-{}-{suffix}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("setup test dir");
        dir.join("config.json")
    }

    #[test]
    fn load_round_trips_through_save() {
        let path = test_path("roundtrip");
        let config = NotificationConfig {
            popup_timeout: 12345,
            max_popups: 7,
            ..NotificationConfig::default()
        };

        save(&path, &config).expect("save succeeds");
        let loaded = load(&path).expect("load succeeds");

        assert_eq!(loaded.popup_timeout, 12345);
        assert_eq!(loaded.max_popups, 7);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn load_missing_file_returns_not_found() {
        let path = test_path("missing")
            .parent()
            .unwrap()
            .join("does-not-exist.json");
        match load(&path) {
            Err(ConfigFileError::NotFound) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn load_malformed_returns_parse_error() {
        let path = test_path("malformed");
        std::fs::write(&path, b"{not valid json}").expect("seed bad file");
        match load(&path) {
            Err(ConfigFileError::Parse(_)) => {}
            other => panic!("expected Parse, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn save_creates_parent_directory_if_missing() {
        let nested = test_path("nested-parent")
            .parent()
            .unwrap()
            .join("subdir")
            .join("nested.json");
        let config = NotificationConfig::default();
        save(&nested, &config).expect("save creates parent");
        assert!(nested.exists(), "nested file should exist after save");
        let _ = std::fs::remove_dir_all(nested.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn load_or_create_default_writes_defaults_when_missing() {
        let path = test_path("first-run");
        // Path doesn't exist yet (test_path created the parent dir
        // but no file).
        assert!(!path.exists());

        let config = load_or_create_default(&path);

        assert!(path.exists(), "default file should be created");
        // Reload to confirm the written file is parseable.
        let reloaded = load(&path).expect("written file should parse");
        assert_eq!(reloaded.popup_timeout, config.popup_timeout);
        assert_eq!(reloaded.max_popups, config.max_popups);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn load_or_create_default_returns_defaults_on_parse_error_without_overwriting() {
        let path = test_path("parse-error-preserved");
        let original = b"{not valid json}";
        std::fs::write(&path, original).expect("seed bad file");

        let _config = load_or_create_default(&path);

        // The bad file should not have been overwritten.
        let after = std::fs::read(&path).expect("read should still work");
        assert_eq!(
            after.as_slice(),
            original,
            "load_or_create_default must not overwrite a malformed file"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
