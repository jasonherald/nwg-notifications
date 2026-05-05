# JSON Config File Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the JSON config file design ratified in `docs/superpowers/specs/2026-05-05-json-config-design.md` (closes [#64](https://github.com/jasonherald/nwg-notifications/issues/64)) — file at `~/.config/nwg-notifications/config.json`, layered merge precedence (`defaults < json < CLI < D-Bus Set*`), `Set*` write-back persistence, and inotify hot-reload.

**Architecture:** Three new code units (`config_file.rs` for load/save/watcher, `paths::config_path()` helper, `config::user_set_args()` for the CLI-override merge) plus serde derive on the existing `NotificationConfig` struct + per-field merge in `main.rs`. D-Bus `Set*` handlers gain a write-back call and an override-set tracking step. The `notify = "8"` crate is added as a direct dep (matching `nwg-common`'s pin so cargo MVS dedupes).

**Tech Stack:** Rust 2024 edition, `serde` + `serde_json` (already in deps), `notify = "8"` (new direct dep), `tempfile` for atomic writes (new direct dep).

**Tracks:** Closes #64. Builds on the spec at `docs/superpowers/specs/2026-05-05-json-config-design.md`.

---

## File Structure

| Task | Files modified | Test approach |
|------|----------------|---------------|
| #1 serde on NotificationConfig | `src/config.rs` (`#[derive(Serialize, Deserialize)]` on `NotificationConfig` + `PopupPosition`; `#[serde(default)]` on each field) | Unit test: `serde_json::from_str("{}")` produces struct with all clap defaults |
| #2 `paths::config_path()` | `src/paths.rs` (new helper + test using existing `with_env` Mutex pattern) | Unit test: XDG_CONFIG_HOME resolution + fallback |
| #3 `config_file.rs` load + save | New `src/config_file.rs`; `src/main.rs` (declare `mod config_file`); `Cargo.toml` (add `tempfile` dep) | Unit tests: round-trip, malformed, missing |
| #4 First-run write-default | `src/config_file.rs` (extend) | Unit test: missing → file appears with defaults serialized |
| #5 Wire into main.rs startup | `src/main.rs` (load → merge CLI overrides over JSON); `src/config.rs` (new `user_set_args()` helper) | Manual smoke: edit JSON, restart daemon, value picked up; pass CLI flag, value overrides JSON |
| #6 `Set*` write-back + override tracking | `src/state.rs` (new `dbus_overrides: HashSet<&'static str>` field); `src/dbus.rs` (each `Set*` lambda inserts into override set + calls `config_file::save`) | Unit test on the override-set logic; manual smoke: `--update` then restart, value persists |
| #7 inotify hot-reload | New watcher function in `src/config_file.rs`; `src/main.rs` (start watcher + glib bridge); `Cargo.toml` (add `notify = "8"`) | Manual smoke: edit JSON while daemon runs, value reloads without restart |
| #8 README + CLAUDE.md docs | `README.md` (new Configuration section); `CLAUDE.md` (Configuration section); `CHANGELOG.md` (Added bullet under `[0.4.1] — Unreleased`) | Visual diff |
| #9 Pre-PR gates + smoke | (no code) | `make lint` + `make upgrade` + user smoke handoff |
| #10 Open PR | (no code) | `gh pr create` |

The plan is on the existing `feat/json-config` branch (spec committed there). Each task is its own commit. Smoke handoff before push.

---

## Pre-flight

- [ ] **Verify branch state**

```bash
cd /data/source/nwg-notifications
git branch --show-current  # expect: feat/json-config
git log --oneline main..HEAD  # expect: 1 commit (the spec)
git status  # expect: clean
```

- [ ] **Commit the plan file as the next commit on the branch**

```bash
git add docs/superpowers/plans/2026-05-05-json-config-implementation.md
git commit -m "docs: implementation plan for JSON config (#64)"
```

- [ ] **Baseline full cargo gambit**

```bash
make lint
```

Expected: every step exits 0; pre-existing `cargo deny` "unmatched skip" warnings non-blocking. Test count is 93 going in.

---

## Task 1: serde derive on `NotificationConfig` + `PopupPosition`

The current `NotificationConfig` is clap-derived only. Adding `serde::{Serialize, Deserialize}` makes the same struct the JSON schema. `#[serde(default)]` on each field makes every key optional — missing keys fall back to clap's `default_value_t` defaults.

`PopupPosition` is `clap::ValueEnum`-derived; we add `serde` derive too so it serializes as the kebab-case strings clap accepts (matches the JSON schema example in the spec).

**Files:**
- Modify: `src/config.rs` — derive serde on the struct + enum, attribute each `pub(crate)` field with `#[serde(default)]`.

- [ ] **Step 1: Add the serde derive + attributes**

In `src/config.rs`, find the `pub(crate) struct NotificationConfig` block. Currently:

```rust
#[derive(Parser, Debug, Clone)]
#[command(version, about)]
pub(crate) struct NotificationConfig {
    /// Popup display position
    #[arg(long, value_enum, default_value_t = PopupPosition::TopRight)]
    pub(crate) popup_position: PopupPosition,
    // ... other fields
}
```

Replace with:

```rust
#[derive(Parser, Debug, Clone, serde::Serialize, serde::Deserialize)]
#[command(version, about)]
#[serde(default)]
pub(crate) struct NotificationConfig {
    /// Popup display position
    #[arg(long, value_enum, default_value_t = PopupPosition::TopRight)]
    pub(crate) popup_position: PopupPosition,
    // ... other fields
}
```

(The struct-level `#[serde(default)]` is shorthand for "every field gets `#[serde(default)]`". Equivalent to per-field annotations; less noisy.)

Then find the `PopupPosition` enum definition. Currently:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum PopupPosition {
    TopRight,
    TopCenter,
    TopLeft,
    BottomRight,
    BottomCenter,
    BottomLeft,
}
```

Replace with:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PopupPosition {
    TopRight,
    TopCenter,
    TopLeft,
    BottomRight,
    BottomCenter,
    BottomLeft,
}
```

`#[serde(rename_all = "kebab-case")]` makes `TopRight` serialize as `"top-right"` to match the strings clap accepts on the CLI.

We also need `Default for NotificationConfig` so `serde(default)` has a struct-level fallback when the file is `{}` and individual fields use `#[serde(default)]` — but per-field defaults don't compose for the whole struct. The cleanest fix: derive `Default` on the struct via clap's existing default-handling. Since clap-derived defaults aren't auto-implemented as `Default::default()`, we need a hand-rolled `impl Default for NotificationConfig` that mirrors clap's `default_value_t`s. **Do this in Step 2.**

- [ ] **Step 2: Skip non-JSON fields with `#[serde(skip)]` and add the version field**

Per the spec, `debug`, `wm`, `count`, and `update` are CLI-only and shouldn't appear in the JSON. The first two are config-flavor knobs we explicitly excluded; the latter two are transient process modes. Mark all four with `#[serde(skip)]`:

```rust
    /// Enable debug logging
    #[arg(long, default_value_t = false)]
    #[serde(skip)]
    pub(crate) debug: bool,

    /// Force compositor backend (auto-detected by default)
    #[arg(long, value_enum)]
    #[serde(skip)]
    pub(crate) wm: Option<nwg_common::compositor::WmOverride>,

    /// Query the running daemon's unread count over D-Bus and exit
    #[arg(long, default_value_t = false)]
    #[serde(skip)]
    pub(crate) count: bool,

    /// Push the supplied flag values to the running daemon ...
    #[arg(long, default_value_t = false)]
    #[serde(skip)]
    pub(crate) update: bool,
```

(Splice each `#[serde(skip)]` line above the `pub(crate)` line; clap's `#[arg(...)]` and serde's `#[serde(skip)]` coexist on the same field.)

Add the version field at the top of the struct (just below the doc-comment that opens the struct, above `popup_position`):

```rust
    /// Schema version for forward-compatibility. Set to `1` for the
    /// initial JSON format. Not exposed as a CLI flag (the value is
    /// only meaningful on the JSON side).
    #[arg(skip)]
    #[serde(default = "default_config_version")]
    pub(crate) version: u32,
```

Then add the helper at module scope below the struct:

```rust
/// Default schema version when serde encounters a JSON file
/// without a `"version"` field — older v0.4.0 files written by
/// the manual jq-edit path predate the field. Treat them as
/// version 1 (the initial schema).
fn default_config_version() -> u32 {
    1
}
```

- [ ] **Step 3: Implement `Default for NotificationConfig` matching clap's defaults**

Add this `impl` block immediately after the struct definition in `src/config.rs`:

```rust
impl Default for NotificationConfig {
    fn default() -> Self {
        // Match clap's `default_value_t` annotations exactly. The
        // serde-deserialized "missing field" path uses these; the
        // CLI-parsed path uses clap's defaults. Both agree.
        Self {
            version: 1,
            popup_position: PopupPosition::TopRight,
            popup_timeout: 7000,
            popup_width: POPUP_WIDTH_DEFAULT,
            panel_width: PANEL_WIDTH_DEFAULT,
            max_popups: 5,
            max_history: 200,
            persist: false,
            dnd: false,
            debug: false,
            wm: None,
            count: false,
            update: false,
        }
    }
}
```

(Field list comes from the current struct; verify against `src/config.rs` before committing.)

- [ ] **Step 4: Add the unit tests**

In `src/config.rs`, find the existing `#[cfg(test)] mod tests` block (or create one if absent). Append:

```rust
    #[test]
    fn empty_json_produces_struct_with_clap_defaults() {
        // serde(default) on the struct + Default impl that mirrors
        // clap's default_value_t means an empty JSON object should
        // produce a NotificationConfig identical to one parsed from
        // an empty CLI invocation.
        let from_json: NotificationConfig =
            serde_json::from_str("{}").expect("empty JSON parses");
        let from_cli = NotificationConfig::parse_from(["test"]);

        assert_eq!(from_json.popup_position, from_cli.popup_position);
        assert_eq!(from_json.popup_timeout, from_cli.popup_timeout);
        assert_eq!(from_json.popup_width, from_cli.popup_width);
        assert_eq!(from_json.panel_width, from_cli.panel_width);
        assert_eq!(from_json.max_popups, from_cli.max_popups);
        assert_eq!(from_json.max_history, from_cli.max_history);
        assert_eq!(from_json.persist, from_cli.persist);
        assert_eq!(from_json.dnd, from_cli.dnd);
    }

    #[test]
    fn popup_position_serializes_as_kebab_case() {
        // Matches the strings clap's ValueEnum accepts on the CLI
        // (e.g. `--popup-position top-right`).
        assert_eq!(
            serde_json::to_string(&PopupPosition::TopRight).unwrap(),
            "\"top-right\""
        );
        let parsed: PopupPosition = serde_json::from_str("\"bottom-center\"").unwrap();
        assert_eq!(parsed, PopupPosition::BottomCenter);
    }

    #[test]
    fn empty_json_defaults_version_to_1() {
        // serde(default = "default_config_version") on the version
        // field means a JSON without it (e.g., old v0.4.0 files
        // written before the field existed) parses as version 1.
        let config: NotificationConfig =
            serde_json::from_str("{}").expect("empty JSON parses");
        assert_eq!(config.version, 1);
    }

    #[test]
    fn cli_only_fields_are_skipped_in_json() {
        // debug / wm / count / update should serialize to nothing
        // (the JSON should not have keys for them).
        let mut config = NotificationConfig::default();
        config.debug = true;
        config.count = true;
        let json = serde_json::to_string(&config).expect("serialize");
        assert!(!json.contains("debug"), "debug must not appear in JSON; got: {json}");
        assert!(!json.contains("\"wm\""), "wm must not appear in JSON; got: {json}");
        assert!(!json.contains("count"), "count must not appear in JSON; got: {json}");
        assert!(!json.contains("update"), "update must not appear in JSON; got: {json}");
    }
```

- [ ] **Step 5: Build, test, clippy, fmt**

```bash
cargo test config::tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: 4 new tests pass; full suite count goes 93 → 97; clippy clean; no fmt drift.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs
git commit -m "$(cat <<'EOF'
Add serde derive on NotificationConfig + PopupPosition (#64)

Same struct that's currently the clap-derived CLI parser becomes
the JSON schema for the config-file work. Three derives added:

- serde::{Serialize, Deserialize} on NotificationConfig with a
  struct-level #[serde(default)] so every field is optional
  in the JSON; missing keys fall back to a hand-rolled Default
  impl that mirrors clap's default_value_t exactly.
- serde::{Serialize, Deserialize} + #[serde(rename_all = "kebab-case")]
  on PopupPosition so the JSON encodes positions as the same
  kebab-case strings clap accepts on the CLI ("top-right" etc.).
- New `version: u32` field with #[arg(skip)] (CLI-invisible) +
  #[serde(default = "default_config_version")] (defaults to 1
  when missing) for forward-compat schema versioning.

CLI-only fields (debug, wm, count, update) marked
#[serde(skip)] so they don't appear in the JSON. count and
update are transient process modes; debug and wm are excluded
per spec (developer toggle / compositor-detection diagnostic).

4 new unit tests: empty JSON → struct matches empty CLI
invocation; PopupPosition round-trips as kebab-case; empty JSON
defaults version to 1; CLI-only fields don't appear in
serialized JSON.

Test count: 93 -> 97.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `paths::config_path()` helper

Lives next to the existing `history_path()` and `status_path()` helpers in `src/paths.rs`. Same XDG-aware shape: prefer `nwg_common::config::paths::config_dir("nwg-notifications")`; that helper already does the `$XDG_CONFIG_HOME` → `$HOME/.config/` → `/tmp` fallback chain.

**Files:**
- Modify: `src/paths.rs` — add `config_path()` + a unit test using the existing `with_env` Mutex pattern.

- [ ] **Step 1: Add the helper**

In `src/paths.rs`, find the existing `history_path()` definition. Below it, before `migrate_history_if_needed`, add:

```rust
/// Returns the path to the JSON config file
/// (`$XDG_CONFIG_HOME/nwg-notifications/config.json` by default).
/// Uses `nwg_common::config::paths::config_dir`, which walks
/// `$XDG_CONFIG_HOME` → `$HOME/.config/nwg-notifications/` → falls
/// back to `/tmp/nwg-notifications/` with a warn-log if neither
/// resolves. The fallback chain is the same one nwg-panel and the
/// other nwg-shell tools use.
pub(crate) fn config_path() -> PathBuf {
    nwg_common::config::paths::config_dir("nwg-notifications").join("config.json")
}
```

- [ ] **Step 2: Add the unit test**

In `src/paths.rs`'s existing `#[cfg(test)] mod tests` block, append (after `migrate_history_if_needed_copies_and_unlinks_legacy_file`):

```rust
    #[test]
    fn config_path_resolves_under_xdg_config_home_when_set() {
        let tmpdir = std::env::temp_dir().join(format!(
            "nwg-paths-config-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmpdir);
        std::fs::create_dir_all(&tmpdir).expect("setup tmpdir");

        let actual = with_env(
            &[("XDG_CONFIG_HOME", Some(tmpdir.to_str().unwrap()))],
            config_path,
        );

        let expected = tmpdir.join("nwg-notifications").join("config.json");
        assert_eq!(actual, expected);

        let _ = std::fs::remove_dir_all(&tmpdir);
    }
```

- [ ] **Step 3: Build, test, clippy, fmt + commit**

```bash
cargo test paths::tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
git add src/paths.rs
git commit -m "$(cat <<'EOF'
Add paths::config_path() helper (#64)

Returns the JSON config-file path
(\$XDG_CONFIG_HOME/nwg-notifications/config.json by default), via
nwg_common::config::paths::config_dir which already encapsulates
the XDG_CONFIG_HOME → \$HOME/.config → /tmp fallback chain that
the other nwg-shell tools share.

1 new unit test asserts the XDG_CONFIG_HOME-set branch lands at
the expected path, using the existing with_env Mutex
serialization helper.

Test count: 97 -> 98.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: New `src/config_file.rs` module — load + save

Owns the JSON-file lifecycle: parse on read, atomic-write on save (via `tempfile` crate). `ConfigFileError` typed enum so callers can distinguish missing-file from parse failure from I/O.

**Files:**
- Create: `src/config_file.rs`.
- Modify: `src/main.rs` — declare `mod config_file;` alphabetically.
- Modify: `Cargo.toml` — add `tempfile = "3"` direct dep (atomic write needs `NamedTempFile::persist`, which is the canonical "atomic on POSIX" rename).

- [ ] **Step 1: Add the `tempfile` dep**

In `Cargo.toml`, find the `[dependencies]` block. After the `nwg-common` line, add (alphabetically):

```toml
# Atomic file writes for the JSON config (NamedTempFile + persist
# = same-fs tempfile + rename(2), the canonical POSIX atomic write).
tempfile = "3"
```

Run `cargo build` to fetch + verify the dep compiles. Expected: `Compiling tempfile v3.x.y` then clean.

- [ ] **Step 2: Create `src/config_file.rs` with the load + save surface + the error type**

Create the new file at `src/config_file.rs`:

```rust
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
//! same-filesystem rename — a kill mid-write leaves the original
//! file unchanged.

use crate::config::NotificationConfig;
use std::io;
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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ConfigFileError::Io)?;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(ConfigFileError::Io)?;
    serde_json::to_writer_pretty(&mut tmp, config).map_err(ConfigFileError::Parse)?;
    // serde_json::to_writer_pretty doesn't add a trailing newline; add
    // one so editors that strip-trailing-whitespace don't fight us.
    use std::io::Write;
    tmp.write_all(b"\n").map_err(ConfigFileError::Io)?;
    tmp.persist(path)
        .map_err(|e| ConfigFileError::Io(e.error))?;
    Ok(())
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
        let mut config = NotificationConfig::default();
        config.popup_timeout = 12345;
        config.max_popups = 7;

        save(&path, &config).expect("save succeeds");
        let loaded = load(&path).expect("load succeeds");

        assert_eq!(loaded.popup_timeout, 12345);
        assert_eq!(loaded.max_popups, 7);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn load_missing_file_returns_not_found() {
        let path = test_path("missing").parent().unwrap().join("does-not-exist.json");
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
}
```

- [ ] **Step 3: Declare the new module in `src/main.rs`**

In `src/main.rs`, find the existing `mod` declarations at the top:

```rust
mod config;
mod dbus;
mod listeners;
mod notification;
mod paths;
mod persistence;
mod state;
mod ui;
mod waybar;
```

Insert `mod config_file;` alphabetically between `mod config;` and `mod dbus;`:

```rust
mod config;
mod config_file;
mod dbus;
mod listeners;
mod notification;
mod paths;
mod persistence;
mod state;
mod ui;
mod waybar;
```

- [ ] **Step 4: Build, test, clippy, fmt + commit**

```bash
cargo build
cargo test config_file::tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
git add src/config_file.rs src/main.rs Cargo.toml Cargo.lock
git commit -m "$(cat <<'EOF'
Add config_file module: load + atomic save (#64)

New src/config_file.rs owns the JSON config-file lifecycle:
load(path) -> Result<NotificationConfig, ConfigFileError> and
save(path, config) -> Result<(), ConfigFileError>. ConfigFileError
distinguishes NotFound (first-run signal for the caller) from
Parse (malformed JSON) from Io (everything else).

save uses tempfile::NamedTempFile::persist for an atomic
same-filesystem rename — kill mid-write leaves the original
file unchanged. Parent directory created on demand.

Adds tempfile = "3" as a direct dep for the atomic-write primitive.

4 new unit tests: load round-trips through save, missing file →
NotFound, malformed JSON → Parse, save creates parent dir if
missing.

Test count: 98 -> 102.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: First-run write-default-on-missing

The startup flow needs a "load existing or write defaults and load those" entry point. Add `load_or_create_default(path)` that:
- Calls `load(path)`. If `Ok(config)`, return it.
- If `Err(ConfigFileError::NotFound)`, call `save(path, &NotificationConfig::default())` then return `Ok(NotificationConfig::default())`.
- If `Err(Parse)` or `Err(Io)`, log and return defaults without writing (so we don't clobber a file that exists but we couldn't read).

**Files:**
- Modify: `src/config_file.rs` — add `load_or_create_default()` + 2 unit tests.

- [ ] **Step 1: Add the helper**

In `src/config_file.rs`, find the existing `pub(crate) fn save` (just below the `load` function). Below `save`, before the `#[cfg(test)] mod tests` block, add:

```rust
/// Boot-time loader: try to load the config from `path`. If the file
/// doesn't exist, write the compiled-in defaults to it and return
/// those (first-run UX). If the file exists but won't parse, log
/// the error and return defaults *without* writing — overwriting
/// a malformed file the user is mid-editing is the wrong thing.
///
/// Returns `NotificationConfig` unconditionally — by the time this
/// returns, the daemon either has a usable config or has logged
/// what went wrong.
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
                log::warn!(
                    "Failed to write default config to {}: {e}",
                    path.display()
                );
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
```

- [ ] **Step 2: Add the tests**

In `src/config_file.rs`'s `#[cfg(test)] mod tests` block, append:

```rust
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
```

- [ ] **Step 3: Build, test, clippy, fmt + commit**

```bash
cargo test config_file::tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
git add src/config_file.rs
git commit -m "$(cat <<'EOF'
config_file: load_or_create_default for first-run UX (#64)

Boot-time loader that bridges three cases into one
NotificationConfig return:

- File exists and parses: return loaded config.
- File doesn't exist: write defaults to disk, return defaults
  (first-run gives the user a real file to edit by hand).
- File exists but won't parse (or other Io error): log + return
  defaults *without* writing — overwriting a malformed file the
  user is mid-editing is worse than running with defaults for
  one session.

2 new unit tests: missing → file appears with parseable defaults,
malformed → defaults returned and original file not overwritten.

Test count: 102 -> 104.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Wire into main.rs startup + CLI override merge

Boot order changes to:
1. Parse CLI args (existing).
2. Load JSON config via `config_file::load_or_create_default(&paths::config_path())`.
3. Per-field merge: for each field, if the CLI flag's value source is `CommandLine` (user explicitly passed it), take from CLI; else take from JSON.
4. Continue with the existing `NotificationConfig` daemon-init flow.

The CLI override detection reuses clap's `ArgMatches::value_source` — same mechanism as the existing `user_set_live_args` helper for `--update`. Generalize that helper to cover all overridable fields (rename to `user_set_args`).

**Files:**
- Modify: `src/config.rs` — generalize `user_set_live_args` to `user_set_args` (covers all NotificationConfig fields, not just the `--update`-eligible subset).
- Modify: `src/main.rs` — load + merge in `main()`.

- [ ] **Step 1: Generalize the user-set-args detection in `src/config.rs`**

Find the existing `user_set_live_args` helper. It currently returns the subset of field names that are valid for `--update` push (popup_position, popup_width, panel_width, popup_timeout, max_popups, max_history). Add a sibling helper that covers all overridable fields including `persist`, `dnd`, `debug`:

In `src/config.rs`, just below `user_set_live_args`, add:

```rust
/// Returns the names of every CLI flag the user explicitly passed
/// on the command line (as opposed to clap's compiled defaults
/// kicking in). Used by the boot-time merge in main() to decide
/// which fields override the JSON config and which fall through to
/// it.
///
/// Distinct from `user_set_live_args` (which covers only the
/// `--update`-eligible subset for D-Bus push). This one is
/// boot-only and includes flags that aren't pushable at runtime
/// (e.g. `--debug`, `--wm`).
pub(crate) fn user_set_args(matches: &clap::ArgMatches) -> std::collections::HashSet<&'static str> {
    use clap::parser::ValueSource;
    let mut out = std::collections::HashSet::new();
    let candidates = [
        "popup_position",
        "popup_timeout",
        "popup_width",
        "panel_width",
        "max_popups",
        "max_history",
        "persist",
        "dnd",
        "debug",
        "wm",
    ];
    for name in candidates {
        if matches.value_source(name) == Some(ValueSource::CommandLine) {
            out.insert(name);
        }
    }
    out
}
```

- [ ] **Step 2: Wire the merge into `main.rs`**

In `src/main.rs`, find the existing parse block (it's around the start of `main()`):

```rust
let matches = NotificationConfig::command().get_matches();
let config = NotificationConfig::from_arg_matches(&matches)
    .expect("clap should produce a valid NotificationConfig from successful matches");
```

Replace with:

```rust
let matches = NotificationConfig::command().get_matches();
let cli_config = NotificationConfig::from_arg_matches(&matches)
    .expect("clap should produce a valid NotificationConfig from successful matches");

// Layered merge: defaults < JSON < CLI < (later: D-Bus Set*).
// CLI flags the user explicitly passed override the JSON; JSON
// fills in the rest from the file (or compiled defaults via
// load_or_create_default if the file doesn't exist).
let json_config = config_file::load_or_create_default(&paths::config_path());
let user_set = config::user_set_args(&matches);
let config = merge_cli_over_json(json_config, cli_config, &user_set);
```

Then add the `merge_cli_over_json` helper at the bottom of `main.rs` (next to the other free functions):

```rust
/// Per-field merge of CLI flags over JSON config. For each field
/// in NotificationConfig, take the CLI value if the user
/// explicitly passed it (`user_set` membership), else take the
/// JSON value. The `count` and `update` fields are always taken
/// from CLI — they're transient mode flags, not config knobs.
fn merge_cli_over_json(
    mut json: NotificationConfig,
    cli: NotificationConfig,
    user_set: &std::collections::HashSet<&'static str>,
) -> NotificationConfig {
    if user_set.contains("popup_position") {
        json.popup_position = cli.popup_position;
    }
    if user_set.contains("popup_timeout") {
        json.popup_timeout = cli.popup_timeout;
    }
    if user_set.contains("popup_width") {
        json.popup_width = cli.popup_width;
    }
    if user_set.contains("panel_width") {
        json.panel_width = cli.panel_width;
    }
    if user_set.contains("max_popups") {
        json.max_popups = cli.max_popups;
    }
    if user_set.contains("max_history") {
        json.max_history = cli.max_history;
    }
    if user_set.contains("persist") {
        json.persist = cli.persist;
    }
    if user_set.contains("dnd") {
        json.dnd = cli.dnd;
    }
    if user_set.contains("debug") {
        json.debug = cli.debug;
    }
    if user_set.contains("wm") {
        json.wm = cli.wm.clone();
    }
    // count and update are always CLI-driven — they're transient
    // mode flags, not config knobs.
    json.count = cli.count;
    json.update = cli.update;
    json
}
```

- [ ] **Step 3: Add a unit test for the merge**

In `src/main.rs`'s existing `#[cfg(test)] mod tests` block (it has the `should_emit_count_changed_*` tests), append:

```rust
    #[test]
    fn merge_cli_over_json_takes_cli_for_user_set_fields_and_json_for_rest() {
        let mut json = NotificationConfig::default();
        json.popup_timeout = 9999;
        json.max_popups = 99;
        json.max_history = 999;

        let mut cli = NotificationConfig::default();
        cli.popup_timeout = 1; // user passed --popup-timeout 1
        cli.max_popups = 2;    // user passed --max-popups 2
        // max_history not passed; cli.max_history is the default (200)

        let mut user_set = std::collections::HashSet::new();
        user_set.insert("popup_timeout");
        user_set.insert("max_popups");
        // max_history NOT in user_set

        let merged = merge_cli_over_json(json, cli, &user_set);

        assert_eq!(merged.popup_timeout, 1, "user-set CLI value wins");
        assert_eq!(merged.max_popups, 2, "user-set CLI value wins");
        assert_eq!(merged.max_history, 999, "JSON value wins when CLI not user-set");
    }
```

- [ ] **Step 4: Build, test, clippy, fmt + commit**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
git add src/config.rs src/main.rs
git commit -m "$(cat <<'EOF'
Wire JSON config into startup + per-field CLI override merge (#64)

Boot order changes to:
1. Parse CLI args (existing).
2. Load JSON via config_file::load_or_create_default at
   paths::config_path() — first-run writes the default file.
3. Per-field merge: CLI value wins when the user explicitly
   passed the flag (clap's ValueSource::CommandLine); otherwise
   take the JSON value.
4. Continue with the existing daemon-init flow.

CLI override detection reuses clap's ArgMatches::value_source —
same mechanism as the existing user_set_live_args helper for
--update push, generalized into user_set_args that covers every
overridable field (boot-only knobs like --debug and --wm
included).

merge_cli_over_json helper lives in main.rs next to the existing
free functions; explicit per-field copy keeps the merge logic
visible (vs. macro magic) and matches the codebase pattern.

count and update mode flags are always CLI-only — transient
process modes, not config knobs.

1 new unit test exercises the merge: user-set fields take CLI;
non-user-set fields take JSON.

Test count: 104 -> 105.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: D-Bus `Set*` write-back + override tracking

`Set*` calls now (a) mutate the in-memory `NotificationConfig` (existing behavior), (b) record the field name in a session-only `dbus_overrides: HashSet<&'static str>` so the hot-reload path can skip those fields, and (c) atomically write the whole config back to disk via `config_file::save`.

The override set lives on `NotificationState` so both the D-Bus dispatch and the hot-reload watcher can reach it through the existing `Rc<RefCell<NotificationState>>` plumbing.

**Files:**
- Modify: `src/state.rs` — add `dbus_overrides: HashSet<&'static str>` field + a `mark_dbus_override(field)` helper.
- Modify: `src/dbus.rs` — each `handle_set_u32` lambda + `handle_set_popup_position` call `state.mark_dbus_override(...)` and `config_file::save(...)` after the in-memory mutation.

- [ ] **Step 1: Add the override-set field to `NotificationState`**

In `src/state.rs`, find the `pub(crate) struct NotificationState` declaration. Add the new field at the bottom of the field list:

```rust
    /// Field names of D-Bus Set* calls made in this session. Used
    /// by the config-file watcher to know which fields to *not*
    /// overwrite from the JSON on hot-reload — Set* values are
    /// "sticky" for the session per spec, until daemon restart.
    /// In-memory only (not persisted).
    pub(crate) dbus_overrides: std::collections::HashSet<&'static str>,
```

Then in the `NotificationState::new` constructor, initialize the new field:

```rust
            dbus_overrides: std::collections::HashSet::new(),
```

(Insert next to the other field initializers.)

- [ ] **Step 2: Add the `mark_dbus_override` helper**

In `src/state.rs`, just below the existing `set_dnd` method, add:

```rust
    /// Marks `field_name` as overridden by a D-Bus Set* call this
    /// session. The hot-reload watcher in `config_file.rs` checks
    /// this set before applying a JSON value, so per-spec sticky
    /// Set* semantics hold until the daemon restarts.
    pub(crate) fn mark_dbus_override(&mut self, field_name: &'static str) {
        self.dbus_overrides.insert(field_name);
    }
```

- [ ] **Step 3: Wire write-back + override-marking into each `Set*` handler in `src/dbus.rs`**

Two handlers to update: `handle_set_popup_position` (the one-off) and the lambdas inside `handle_nwg_count_method`'s match arms (the five `handle_set_u32` calls).

In `src/dbus.rs`, find `handle_set_popup_position`. After the line `config.borrow_mut().popup_position = pos;`, insert:

```rust
            state.borrow_mut().mark_dbus_override("popup_position");
            persist_config(&config.borrow());
```

(The `state` reference needs to be available in scope. If `handle_set_popup_position` doesn't currently take `state`, add it as a parameter and update the dispatcher in `handle_nwg_count_method` to pass it. The same dispatcher already passes `state` to other handlers, so the threading is straightforward.)

Similarly, for each of the five lambdas inside `handle_nwg_count_method`'s match arms (`SetPopupWidth`, `SetPanelWidth`, `SetPopupTimeout`, `SetMaxPopups`, `SetMaxHistory`), after the in-memory mutation (the `cfg.<field> = ...;` line), insert:

```rust
            // Mark this field overridden so hot-reload skips it.
            state.borrow_mut().mark_dbus_override("<field_name>");
            persist_config(cfg);
            Ok(())
```

(Replace `<field_name>` with the actual field — `popup_width`, `panel_width`, `popup_timeout`, `max_popups`, `max_history`. The `cfg` is the `&mut NotificationConfig` already in scope inside the lambda.)

Then add the `persist_config` helper at the bottom of `src/dbus.rs` (next to other free functions, before the test module):

```rust
/// Atomically writes the current config to the JSON file. Logs
/// (warn-level) on failure but doesn't propagate the error — the
/// in-memory mutation already succeeded, so the live update is
/// visible immediately; persistence is best-effort. Operator sees
/// the warning in the journal.
fn persist_config(config: &crate::config::NotificationConfig) {
    let path = crate::paths::config_path();
    if let Err(e) = crate::config_file::save(&path, config) {
        log::warn!(
            "Failed to persist Set* update to {}: {e}",
            path.display()
        );
    }
}
```

- [ ] **Step 4: Add a unit test for the override set**

In `src/state.rs`'s `#[cfg(test)] mod tests` block, append:

```rust
    #[test]
    fn mark_dbus_override_records_field_name() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        assert!(state.dbus_overrides.is_empty());

        state.mark_dbus_override("popup_width");
        state.mark_dbus_override("max_popups");

        assert!(state.dbus_overrides.contains("popup_width"));
        assert!(state.dbus_overrides.contains("max_popups"));
        assert!(!state.dbus_overrides.contains("max_history"));
        assert_eq!(state.dbus_overrides.len(), 2);
    }
```

- [ ] **Step 5: Build, test, clippy, fmt + commit**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
git add src/state.rs src/dbus.rs
git commit -m "$(cat <<'EOF'
D-Bus Set* write-back + session-sticky override tracking (#64)

Each Set* handler now does three things in order: (1) mutate the
in-memory NotificationConfig (existing behavior), (2) record the
field name in NotificationState::dbus_overrides so the hot-reload
watcher knows to skip it, and (3) atomically persist the whole
config back to the JSON via config_file::save.

The override set is in-memory only — it doesn't survive a daemon
restart, so on the next boot the JSON-layer values from the last
Set*-driven write-back persist (the write-back is the persistence
mechanism; the in-memory override is just for the duration of
the session).

Why session-sticky? Per spec, layered Option<T> per source means
"each layer overrides the layer below." Without the override set,
a hot-reload after a Set* call would re-apply the JSON on top of
the in-memory Set* value, defeating the override. The override
set tracks which layer "wins" for each field across the session.

persist_config() helper logs warn on save failure but doesn't
propagate the error — in-memory mutation already succeeded, the
live update is visible immediately, persistence is best-effort.

1 new unit test on mark_dbus_override.

Test count: 105 -> 106.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: inotify hot-reload watcher

A `notify`-crate watcher in `config_file.rs` runs on a background thread, sends `NotificationConfig` values into an mpsc channel; `main.rs` polls the channel via the same `glib::timeout_add_local` pattern as `listeners.rs`. On each event, the daemon merges the new JSON into the in-memory config — but only for fields NOT in `state.dbus_overrides` (per Task 6).

**Files:**
- Modify: `Cargo.toml` — add `notify = "8"` (matching nwg-common's pin so cargo MVS dedupes).
- Modify: `src/config_file.rs` — add `start_watcher(path) -> Receiver<NotificationConfig>` plus a small `apply_reload(state, config, new_config)` helper.
- Modify: `src/main.rs` — start the watcher in `activate_notifications` after the D-Bus server registration.

- [ ] **Step 1: Add the `notify` dep**

In `Cargo.toml`, find the `[dependencies]` block. Below `tempfile`, add:

```toml
# Inotify-based config-file watcher for hot-reload. notify = "8"
# to match nwg-common's pin (cargo MVS dedupes).
notify = "8"
```

Run `cargo build` to fetch + verify. Expected: clean build.

- [ ] **Step 2: Add the watcher to `config_file.rs`**

In `src/config_file.rs`, append below `load_or_create_default`:

```rust
/// Starts an inotify-based watcher on `path` and returns the
/// receiver end of an mpsc channel. Each detected modification
/// triggers a reload + send of the parsed `NotificationConfig`
/// (or a logged-warning + skip if the reload fails). The watcher
/// thread runs detached; the returned channel keeps it alive as
/// long as the receiver exists.
///
/// Caller bridges the channel onto the glib main loop via
/// `glib::timeout_add_local` (see `main.rs::activate_notifications`
/// for the wiring). Same pattern `listeners.rs` uses for the
/// signal-thread bridge.
pub(crate) fn start_watcher(path: &Path) -> std::sync::mpsc::Receiver<NotificationConfig> {
    use notify::{Event, EventKind, RecursiveMode, Watcher};
    let (tx, rx) = std::sync::mpsc::channel::<NotificationConfig>();
    let watch_path = path.to_path_buf();

    std::thread::spawn(move || {
        let (notify_tx, notify_rx) = std::sync::mpsc::channel::<notify::Result<Event>>();
        let mut watcher = match notify::recommended_watcher(notify_tx) {
            Ok(w) => w,
            Err(e) => {
                log::warn!("Failed to construct config-file watcher: {e}");
                return;
            }
        };
        // Watch the parent dir, not the file itself: inotify's
        // IN_MODIFY on a renamed-replaced file (atomic-write
        // pattern) doesn't fire if we watch the file's inode
        // directly. Watching the dir + filtering by filename
        // catches both in-place modifies and rename-into-place.
        let parent = match watch_path.parent() {
            Some(p) => p,
            None => {
                log::warn!(
                    "Config path {} has no parent directory; cannot watch",
                    watch_path.display()
                );
                return;
            }
        };
        if let Err(e) = watcher.watch(parent, RecursiveMode::NonRecursive) {
            log::warn!(
                "Failed to start config-file watcher on {}: {e}",
                parent.display()
            );
            return;
        }

        for event in notify_rx {
            let event = match event {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("Config-file watcher event error: {e}");
                    continue;
                }
            };
            // Only react to writes/creates affecting our specific
            // config file (parent dir might see other files change).
            let touches_our_file = event.paths.iter().any(|p| p == &watch_path);
            if !touches_our_file {
                continue;
            }
            let is_modify_or_create = matches!(
                event.kind,
                EventKind::Modify(_) | EventKind::Create(_)
            );
            if !is_modify_or_create {
                continue;
            }
            match load(&watch_path) {
                Ok(config) => {
                    if tx.send(config).is_err() {
                        // Receiver dropped (daemon shutting down).
                        return;
                    }
                }
                Err(ConfigFileError::NotFound) => {
                    // File was deleted; treat as no-op (don't reset
                    // to defaults — the user might be mid-edit).
                }
                Err(e) => {
                    log::warn!("Config-file reload failed: {e}");
                }
            }
        }
    });

    rx
}
```

- [ ] **Step 3: Wire the watcher into `activate_notifications` in `main.rs`**

In `src/main.rs::activate_notifications`, after the `dbus::register_server(...)` call but before `listeners::poll_signals(...)`, insert:

```rust
    // Hot-reload config from disk. The watcher runs on a detached
    // thread; we poll the receiver from the glib main loop and
    // apply each reload into the live state, skipping fields that
    // are sticky per a Set* override this session.
    let config_watcher = config_file::start_watcher(&paths::config_path());
    let state_reload = Rc::clone(&state);
    let config_reload = Rc::clone(config);
    let on_change_reload = Rc::clone(&on_state_change);
    glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        while let Ok(new_config) = config_watcher.try_recv() {
            apply_config_reload(&state_reload, &config_reload, &new_config);
            on_change_reload();
        }
        glib::ControlFlow::Continue
    });
```

Then add the `apply_config_reload` helper at the bottom of `main.rs` (next to `merge_cli_over_json`):

```rust
/// Applies a hot-reloaded config into the live in-memory config,
/// per-field. Skips any field whose name is in
/// `state.dbus_overrides` (Set* sticky for the session).
fn apply_config_reload(
    state: &Rc<RefCell<NotificationState>>,
    config: &Rc<RefCell<NotificationConfig>>,
    new: &NotificationConfig,
) {
    let overrides = state.borrow().dbus_overrides.clone();
    let mut cfg = config.borrow_mut();
    if !overrides.contains("popup_position") {
        cfg.popup_position = new.popup_position;
    }
    if !overrides.contains("popup_timeout") {
        cfg.popup_timeout = new.popup_timeout;
    }
    if !overrides.contains("popup_width") {
        cfg.popup_width = new.popup_width;
    }
    if !overrides.contains("panel_width") {
        cfg.panel_width = new.panel_width;
    }
    if !overrides.contains("max_popups") {
        cfg.max_popups = new.max_popups;
    }
    if !overrides.contains("max_history") {
        cfg.max_history = new.max_history;
    }
    if !overrides.contains("persist") {
        cfg.persist = new.persist;
    }
    if !overrides.contains("dnd") {
        cfg.dnd = new.dnd;
    }
}
```

- [ ] **Step 4: Add a unit test for `apply_config_reload`**

In `src/main.rs`'s test module, append:

```rust
    #[test]
    fn apply_config_reload_skips_dbus_overridden_fields() {
        // Synthesize a state with one override + a starting config,
        // call apply_config_reload with a new config, assert only
        // the non-overridden fields changed.
        use crate::config::NotificationConfig;
        use crate::state::NotificationState;

        let starting = NotificationConfig::default();
        let config = Rc::new(RefCell::new(starting));
        let state_inner = NotificationState::new(
            vec![],
            Rc::new(RefCell::new(NotificationConfig::default())),
        );
        let state = Rc::new(RefCell::new(state_inner));

        // Simulate a Set* call that override popup_width earlier in
        // the session.
        state.borrow_mut().mark_dbus_override("popup_width");
        config.borrow_mut().popup_width = 999;

        // Hot-reload: new JSON has different values for both
        // popup_width (overridden) and max_popups (not).
        let mut new_config = NotificationConfig::default();
        new_config.popup_width = 500;
        new_config.max_popups = 42;

        apply_config_reload(&state, &config, &new_config);

        let after = config.borrow();
        assert_eq!(
            after.popup_width, 999,
            "popup_width was Set*-overridden; reload must not clobber it"
        );
        assert_eq!(
            after.max_popups, 42,
            "max_popups was not overridden; reload should apply"
        );
    }
```

- [ ] **Step 5: Build, test, clippy, fmt + commit**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
git add Cargo.toml Cargo.lock src/config_file.rs src/main.rs
git commit -m "$(cat <<'EOF'
Inotify hot-reload for config.json (#64)

start_watcher(path) in config_file.rs spawns a detached thread
running notify::recommended_watcher on the config file's parent
directory (watching the file's inode directly misses
atomic-rename writes — the watch_css helper in nwg-common takes
the same approach for the same reason). Each modify/create event
that touches the specific config file triggers a load + send into
an mpsc channel.

main.rs::activate_notifications polls the channel via
glib::timeout_add_local at 200ms and applies each reload through
apply_config_reload, which copies fields per-key from the new
config — skipping any field present in
state.dbus_overrides (Set* sticky per session, see Task 6).

apply_config_reload uses an explicit per-field if !override copy
rather than reflection or a macro — same pattern as
merge_cli_over_json from Task 5; keeps the merge logic visible in
one place.

Adds notify = "8" as a direct dep to match nwg-common's pin
(cargo MVS dedupes).

1 new unit test exercises the skip-overridden-fields behavior.

Test count: 106 -> 107.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: README + CLAUDE.md `Configuration` section + CHANGELOG entry

User-facing docs: README gets a Configuration section explaining the file location, schema, and precedence rule. CLAUDE.md gets the same but project-internal-flavored. CHANGELOG `[0.4.1] — Unreleased` gets the Added bullet.

**Files:**
- Modify: `README.md` — new `Configuration` section above `Signal control`.
- Modify: `CLAUDE.md` — new `Configuration` section.
- Modify: `CHANGELOG.md` — Added bullet.

- [ ] **Step 1: Add the README section**

In `README.md`, find `## Signal control`. Insert immediately above it:

```markdown
## Configuration

The daemon reads `~/.config/nwg-notifications/config.json` at startup. Every key is optional; missing keys fall back to the same defaults the CLI flags use.

```jsonc
{
  "version": 1,
  "popup_position": "top-right",
  "popup_width": 380,
  "panel_width": 380,
  "popup_timeout": 7000,
  "max_popups": 5,
  "max_history": 200,
  "persist": true,
  "dnd": false
}
```

**First run:** if the file doesn't exist, the daemon writes the defaults to it on first startup. You get a real file you can hand-edit.

**Hot reload:** edits to the file are picked up automatically (inotify-based). No daemon restart required.

**Precedence (lowest to highest):** compiled defaults < `config.json` < CLI flags < `org.nwg.Notifications.Set*` D-Bus calls. CLI flags override the JSON for one-shot diagnostic runs (`nwg-notifications --max-popups 1`); D-Bus `Set*` overrides for hot-update use cases (nwg-shell-config). `Set*` updates also write back to the JSON, so they persist across daemon restarts.

**Sticky `Set*` semantics:** within a session, a `Set*` call wins over subsequent JSON file edits for that field. Restart the daemon to reset.
```

- [ ] **Step 2: Add the CLAUDE.md section**

In `CLAUDE.md`, find the existing `## Conventions` section. The next heading after it is `## Key patterns`. Insert the new section between them — directly above the `## Key patterns` heading:

```markdown
## Configuration

`config.json` at `paths::config_path()` (typically `~/.config/nwg-notifications/config.json`). Schema = the `NotificationConfig` clap-derived struct, with `serde::{Serialize, Deserialize}` + struct-level `#[serde(default)]` and a hand-rolled `Default` impl mirroring clap's `default_value_t`s.

**Layered merge:** `defaults < config.json < CLI flags < D-Bus Set*`. Implemented in `main.rs::merge_cli_over_json` (boot) + `main.rs::apply_config_reload` (hot-reload). The D-Bus layer is in-memory only via `state.dbus_overrides: HashSet<&'static str>` — the field set there causes the hot-reload path to skip those fields. `Set*` calls also write back to `config.json` via `config_file::save` so the value survives daemon restart.

**Atomic writes:** `config_file::save` uses `tempfile::NamedTempFile::persist` for same-fs rename(2). Kill mid-write leaves the original file unchanged.

**Hot reload:** `config_file::start_watcher` runs `notify::recommended_watcher` on the parent dir (watching the file inode directly misses atomic-rename writes). Reloads bridge to glib via `mpsc::Receiver` + `glib::timeout_add_local` at 200ms — same pattern `listeners.rs` uses for SIGRTMIN signals.
```

- [ ] **Step 3: Add the CHANGELOG bullet**

In `CHANGELOG.md`'s `## [0.4.1] — Unreleased` section, find the existing `### Added` block (added in PR #66 for the `org.nwg.Notifications.service` file). Append a new bullet:

```markdown
- JSON config file at `~/.config/nwg-notifications/config.json`
  (#64). Loaded at startup with merge precedence
  `defaults < config.json < CLI flags < D-Bus Set*`. First-run
  writes defaults so the file is hand-editable. Inotify-based
  hot-reload picks up edits without daemon restart. `Set*` D-Bus
  calls (the nwg-shell-config push path) persist back to the
  JSON via atomic write, so live updates survive daemon
  restarts. See README's `Configuration` section for the schema.
```

- [ ] **Step 4: Visual diff sanity-check + commit**

```bash
git diff README.md CLAUDE.md CHANGELOG.md | head -120
git add README.md CLAUDE.md CHANGELOG.md
git commit -m "$(cat <<'EOF'
docs: README + CLAUDE.md Configuration section + CHANGELOG (#64)

Documents the JSON config-file feature for users (README) and
contributors (CLAUDE.md):

- File location, schema example, first-run write-default
  behavior, hot-reload semantics, layered-merge precedence rule,
  sticky Set* caveat — all documented in README's new
  Configuration section above Signal control.
- CLAUDE.md gets project-internal flavor: pointers at the
  paths::config_path() helper, the NotificationConfig serde
  derives, the merge_cli_over_json + apply_config_reload helpers,
  the dbus_overrides HashSet, the atomic-write primitive, the
  watcher's parent-dir-not-file-inode pattern.
- CHANGELOG [0.4.1] — Unreleased gets an Added bullet.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Pre-PR gates + smoke install + STOP for user smoke

This feature has multiple new behaviors (load, write-default, CLI override, write-back, hot-reload, sticky Set*). Real upgrade scenario; user smoke is essential before PR.

- [ ] **Step 1: Install to user bin and confirm restart works**

```bash
make upgrade PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
```

Expected: build → install → kill running daemon → respawn. `pidof nwg-notifications` returns the new PID.

- [ ] **Step 2: Capture file paths for the smoke handoff**

```bash
ls -la ~/.config/nwg-notifications/ 2>&1 | head -5
```

This is informational — confirms whether the config file appeared (first-run behavior should have written it on the just-respawned daemon).

- [ ] **Step 3: Hand off to the user — STOP HERE**

Tell the user (verbatim or close):

> Installed and restarted via `make upgrade`. **This feature has multiple paths — please walk through each before I open the PR**:
>
> **1. First-run default file:**
>
> - Look at `~/.config/nwg-notifications/config.json` — should exist with all keys + `"version": 1`. If you've never run with this branch before, it was written on first startup.
>
> **2. Edit + hot-reload:**
>
> - `jq '.popup_timeout = 12345' ~/.config/nwg-notifications/config.json > /tmp/x.json && mv /tmp/x.json ~/.config/nwg-notifications/config.json`
> - `notify-send "smoke" "should auto-dismiss after 12.3s now"` — the popup should stay up for ~12 seconds (verifying the reload picked up the new timeout).
> - Optional: `journalctl --user --since "1 minute ago" | grep -i config` for confirming logs.
>
> **3. CLI override:**
>
> - Restart the daemon with an explicit flag: `kill $(pidof nwg-notifications) && nwg-notifications --persist --popup-timeout 3000 &`
> - The CLI flag should override the JSON value. `notify-send "cli wins"` should auto-dismiss after 3s.
>
> **4. Set* write-back + sticky:**
>
> - `nwg-notifications --update --max-popups 7` — should print `Updated max_popups`.
> - `cat ~/.config/nwg-notifications/config.json | jq .max_popups` — should now read `7` (write-back persisted).
> - Edit the JSON to set `max_popups` to something else, like 4.
> - `cat ~/.config/nwg-notifications/config.json | jq .max_popups` — file shows 4, but the daemon's in-memory value should still be 7 because `Set*` is sticky for the session.
> - Restart the daemon. The override resets; daemon now reads `max_popups: 4` from JSON.
>
> Reply with what worked + what didn't. **Do not let me open the PR until you've verified each path.**

**Do not proceed to Task 10 until the user explicitly approves.**

- [ ] **Step 4: Full lint after smoke approval**

```bash
make lint
```

Expected: every step exits 0; total test count is 107.

- [ ] **Step 5: Push**

```bash
git push -u origin feat/json-config
```

---

## Task 10: Open PR

- [ ] **Step 1: Open the PR**

```bash
gh pr create --base main --head feat/json-config \
  --title "feat: JSON config file with hot-reload and write-back (#64)" \
  --body "$(cat <<'EOF'
## Summary

Closes **#64**. Implements the design ratified in [docs/superpowers/specs/2026-05-05-json-config-design.md](https://github.com/jasonherald/nwg-notifications/blob/feat/json-config/docs/superpowers/specs/2026-05-05-json-config-design.md).

Adds \`~/.config/nwg-notifications/config.json\` with:

- **Layered merge:** \`defaults < config.json < CLI flags < D-Bus Set*\`. CLI flags the user explicitly passes (clap \`ValueSource::CommandLine\`) override the JSON; otherwise JSON wins; otherwise compiled defaults via a \`Default\` impl mirroring clap's \`default_value_t\`s.
- **First-run write-default:** if the file doesn't exist on startup, the daemon writes a default file with all keys + \`"version": 1\`. Users get a real file to hand-edit.
- **\`Set*\` write-back:** \`org.nwg.Notifications.Set*\` calls (the nwg-shell-config push path) atomically write back to the JSON, so live updates survive daemon restart.
- **Inotify hot-reload:** edits to the JSON are picked up without daemon restart, via a \`notify\` watcher running on a detached thread bridged to the glib main loop.
- **Session-sticky Set*:** within a session, a \`Set*\` call wins over subsequent JSON edits for that field. \`NotificationState::dbus_overrides: HashSet<&'static str>\` tracks the override set; restart resets.

Atomic writes via \`tempfile::NamedTempFile::persist\` (same-fs rename(2)). Watcher targets the parent dir (watching the file inode misses atomic-rename writes).

## Test plan

- [x] \`make lint\` clean locally (fmt + clippy + test + deny + audit). Test count: 93 → 107 (+14).
- [x] **Manual smoke test against the live compositor** (installed via \`make upgrade PREFIX=\$HOME/.local BINDIR=\$HOME/.cargo/bin\`):
  - [x] First-run: config.json appears with all keys.
  - [x] Edit + hot-reload: \`jq\`-edit popup_timeout, next \`notify-send\` reflects the new value without restart.
  - [x] CLI override: \`--popup-timeout 3000\` overrides JSON value at boot.
  - [x] Set* write-back: \`--update --max-popups 7\` updates JSON; restart picks up the persisted value.
  - [x] Sticky Set*: edit JSON after Set*, daemon stays on Set* value until restart.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 2: Wait for CodeRabbit + iterate**

Default-fix posture per repo convention. Inline reply per in-diff comment, single PR-level reply for outside-diff items, tag `@coderabbitai` every time. Don't respond to non-bot commenters under the maintainer's account.

---

## Notes

- **Spec drift watch:** if implementation diverges from the spec at `docs/superpowers/specs/2026-05-05-json-config-design.md`, update the spec (or call out in the PR body why the divergence is intentional). The spec is the contract.
- **`merge_cli_over_json` + `apply_config_reload` are visually similar** — they both walk the same field list and copy values conditionally. The conditions differ (CLI-set vs not-overridden), so a single function would need a closure-based predicate. Two explicit functions keep both call sites readable; resist the urge to dedupe.
- **The `count` and `update` mode flags are excluded from JSON.** They're transient process-mode flags, not config knobs. The merge code treats them as always-CLI-driven.
- **`debug` and `wm` excluded from JSON per spec.** `debug` is a developer toggle, `wm` is a compositor-detection diagnostic; both stay CLI-only.
