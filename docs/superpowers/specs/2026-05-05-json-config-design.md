# JSON Config File Design Spec

> **Tracks:** [#64](https://github.com/jasonherald/notifications/issues/64)
>
> **Status:** Approved design (ratified 2026-05-05); ready for implementation plan.

## Problem

Settings today come from two ephemeral sources only: clap-parsed CLI flags (lost on daemon restart) and `org.nwg.Notifications.Set*` D-Bus methods (also lost on daemon restart). A user who runs `nwg-notifications --update --max-popups 3` (directly or via nwg-shell-config) has no way for that change to survive their next session. Every other notification daemon on Linux (swaync, mako, dunst) reads a config file at startup; nwg-shell tools (nwg-panel, nwg-dock) all read JSON. Adding a JSON config file closes the persistence gap and matches the broader nwg-shell convention.

## Architecture

A new `src/config_file.rs` module owns load + write-back of the JSON file. `NotificationConfig` (current clap-derived struct in `src/config.rs`) gains `serde::{Serialize, Deserialize}` so the same struct is the schema. The new `paths.rs` already-shipping module gets a `config_path()` helper to locate the file. `main.rs::main` loads the file early in startup, applies CLI flags as overrides on top, and stashes the resolved `NotificationConfig` exactly as it does today. The runtime D-Bus `Set*` handlers in `dbus.rs` add a write-back step after mutating the in-memory config. A `notify`-crate watcher (added as a direct dep — `nwg-common` uses `notify` internally but doesn't re-export it as public surface) runs on the glib main loop and reloads the file when it changes from any source.

The load and write-back paths each have one clear job and live in `config_file.rs`; the main-loop wiring stays in `main.rs`. The watcher is small enough to live in `config_file.rs` too (one function returning a glib timeout-receiver pair, mirroring the existing `listeners.rs` signal-bridge pattern).

## Six design decisions (ratified)

1. **Location:** `~/.config/nwg-notifications/config.json` (per-tool dir, matches `nwg-panel/` and `nwg-dock/` convention). XDG-resolved via `nwg_common::config::paths::config_dir("nwg-notifications")`. Falls back to `$HOME/.config/nwg-notifications/` if `XDG_CONFIG_HOME` isn't set; `nwg_common` already handles that fallback.

2. **Schema:** snake_case JSON keys matching Rust field names (so `#[serde(default)]` on every field works without `#[serde(rename_all = ...)]` glue). Every field optional with `#[serde(default)]`; missing keys fall back to compiled-in defaults the same way clap's `default_value_t` already encodes. Top-level `"version": 1` field for forward-compat. Plain JSON; no JSON5/jsonc — `nwg-shell-config` reads/writes plain JSON.

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

   Excluded from the JSON: `debug` (developer toggle, CLI-only), `wm` (compositor-detection diagnostic, CLI-only).

3. **Merge precedence (lowest → highest):** compiled-in defaults < `config.json` < CLI flags < D-Bus `Set*` methods. CLI overrides JSON because `--max-popups 1` for one diagnostic run shouldn't have to round-trip through editing the file. D-Bus `Set*` overrides everything because nwg-shell-config's hot-update pushes are the most-recent in-session user intent.

   **Layered `Option<T>` model:** each source maintains its own `Option<T>` per field. Resolved runtime value = topmost `Some`. CLI overrides set their field's CLI-layer Option to `Some(v)` at parse time; `Set*` calls set the D-Bus-layer Option to `Some(v)` at call time. The D-Bus layer is in-memory only — it doesn't survive a daemon restart, so on the next boot only the JSON-layer values from the last `Set*`-driven write-back persist (the write-back is the persistence mechanism; the in-memory D-Bus override is just for the duration of the session). This keeps "Set* sticky during the session" + "manual JSON edit doesn't fight a still-active session-only override" unambiguous.

4. **`Set*` write-back:** in-memory mutation **and** persistent write to the JSON. Closes the gap: a `SetMaxPopups(3)` call from nwg-shell-config now survives the next daemon restart. Idempotent: if nwg-shell-config also writes the JSON itself (its existing pattern), our write-back is a no-op against the same value. Write atomically: `tempfile + fsync + rename` so a kill mid-write doesn't leave half a file.

5. **Hot-reload mechanism:** inotify watcher via the `notify` crate (added as a direct dep). Watches the config-file path; on `Modified` events, reloads the JSON layer of the merge and re-resolves each field. Per the layered `Option<T>` model in #3, JSON-layer values only "win" for fields where the in-memory D-Bus layer is `None` — a `Set*` call earlier in the session keeps overriding the field even after a JSON edit, until daemon restart. Most users won't hit this conflict (nwg-shell-config drives both sides; hand-editors rarely also have a `Set*` outstanding). Fires the existing `on_state_change` callback so waybar updates if the resolved config affects it. Lives in `config_file.rs`; bridged to the glib main loop via the same mpsc + `glib::timeout_add_local` pattern as `listeners.rs`.

6. **First-run behavior:** if the config file doesn't exist when the daemon starts, write a default file (compiled-in defaults serialized). New users see all available keys immediately and can edit by hand; nwg-shell-config has a real file to read on first connect. The write happens after the load attempt fails with `NotFound` — never overwrites an existing file.

## Components

| Module | Responsibility |
|---|---|
| `src/config.rs` | Existing `NotificationConfig` clap struct + `PopupPosition` enum. Add `serde` derive + `#[serde(default)]` on each field. Becomes the schema. |
| `src/paths.rs` | Add `config_path() -> PathBuf` helper using `nwg_common::config::paths::config_dir("nwg-notifications").join("config.json")`. Same fallback chain as the other path helpers. |
| `src/config_file.rs` | **New.** `load(path) -> Result<NotificationConfig, ConfigFileError>`, `save(path, config) -> Result<(), ConfigFileError>` (atomic write), `start_watcher(path) -> mpsc::Receiver<NotificationConfig>` (inotify bridge). |
| `src/main.rs::main` | Boot wiring: load config → apply CLI overrides → start watcher → reload-on-event. |
| `src/dbus.rs` | `Set*` handlers gain a write-back call to `config_file::save` after mutating in-memory state. |

## Error handling

- **Load failure (parse error or unreadable):** log `error!`, fall back to compiled-in defaults, **don't** create a default file (a parse error means a file we can't understand exists and we shouldn't clobber it). Daemon continues with defaults.
- **Load failure (NotFound):** first-run path. Write the default file via `config_file::save` (which doesn't error if the parent dir is missing — creates it).
- **Save failure (write-back from `Set*`):** log `warn!`, in-memory mutation still happens. The CLI/nwg-shell-config caller already saw the `Set*` D-Bus call succeed; degrading to "live update happened, persistence didn't" is a friendlier failure than rejecting the call. Operator sees the warning in the journal.
- **Watcher error:** log `warn!` and stop watching. Daemon continues; live `Set*` updates still work; just no hot-reload from external edits.

## Testing approach

- **Unit (in `config_file.rs`):** load with valid JSON returns expected struct. Load with malformed JSON returns `ConfigFileError::Parse` and doesn't write. Load with missing file returns `ConfigFileError::NotFound`. Save round-trips through load. Save uses `tempfile + rename` (asserted by inspecting the implementation, not by killing mid-write — that's integration territory).
- **Unit (in `paths.rs`):** `config_path()` resolves under XDG_CONFIG_HOME, falls back to `~/.config/nwg-notifications/config.json`, lands at the right filename. Same `with_env` mutex serialization pattern as the existing path tests.
- **Unit (in `config.rs`):** `serde_json::from_str("{}")` produces a `NotificationConfig` matching all clap defaults — the `#[serde(default)]` annotations work as expected.
- **Integration (manual smoke):** edit `~/.config/nwg-notifications/config.json` while the daemon runs → values reload without restart. `--update --max-popups 3` then restart daemon → max-popups still 3 (write-back persisted).

## Out of scope

- **Config schema versioning beyond `"version": 1`.** No migration code yet; we'll cross that bridge when version 2 happens. The version field exists so we can detect future incompatibility, not so we have to handle it now.
- **Config validation beyond what clap's range parsers already do.** `popup_width` parsed from JSON gets the same `100..=2000` range check as the CLI flag (handled by routing through clap's `value_parser` or duplicating the bound check in `config_file::load` — implementation detail decided at plan time).
- **JSON5/JSONC support.** Plain JSON. `nwg-shell-config` reads/writes plain JSON; we match.
- **Multi-file config / includes / overlay dirs.** Single `config.json`, no `config.d/`. Adding overlay dirs later is non-breaking; YAGNI for v1.

## Acceptance criteria

- [ ] `~/.config/nwg-notifications/config.json` loads at startup with defaults applied for missing keys.
- [ ] Compiled-in defaults < config.json < CLI flags merge precedence verified by unit tests.
- [ ] `org.nwg.Notifications.Set*` methods persist their updates to the JSON file via atomic write; concurrent writes from nwg-shell-config don't crash either side.
- [ ] inotify-based hot-reload picks up file edits without daemon restart.
- [ ] First-run writes a default config file with all keys + `"version": 1`.
- [ ] CHANGELOG `[Unreleased]` entry under `Added` documenting the file location, schema, and precedence rule.
- [ ] CLAUDE.md gets a `Configuration` section.
- [ ] README gets a `Configuration` section.

## Notes for the cross-repo port

This is the canonical reference for the pattern; same shape applies to nwg-panel/nwg-dock when their configs need similar live + persistent merge semantics. The `config_file.rs` module is small enough to copy verbatim with the type name swapped.
