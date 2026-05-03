# Bundle Docs Sweep (#44, #45, #49) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bundle three documentation items from epic #29 (post-v0.3.4 cleanup) into one PR — module-level `//!` docstrings on every `src/*.rs` and `src/ui/*.rs` file (#44), `# Errors` / `# Panics` sections on the `Result`-returning functions and `.expect()`-bearing register helpers in `src/dbus.rs` (#45), and an inline comment explaining the GTK `hold_guard` pattern in `src/main.rs` plus a draft sub-bullet for the user to paste onto issue #16 (#49).

**Architecture:** Pure documentation — no behavior changes, no test changes, no signature changes. The plan is mostly text editing. One commit per issue, smallest blast radius last (#49 is a one-line code comment + a draft for the user; #44 touches 16 files; #45 touches 1 file but with substantive docstring content).

**Tech Stack:** Rust rustdoc syntax (`//!` module docstrings, `# Errors` / `# Panics` sections inside `///` doc blocks). `cargo doc --document-private-items` as the verification gate.

**Tracks:** Closes #44, #45. #49 is partially closed (inline comment shipped); the second AC ("#16 gets a sub-bullet") is delegated to the user via a drafted snippet in the PR body, per the project convention that the user owns all GitHub-facing communication outside CodeRabbit.

---

## File Structure

| Task | Files modified | Test approach |
|------|----------------|---------------|
| #49 hold_guard inline comment | `src/main.rs` (one comment near the `let hold_guard` declaration; PR body provides the #16 sub-bullet draft) | `cargo build` succeeds |
| #45 `# Errors` / `# Panics` docs | `src/dbus.rs` (7 `pub(crate) fn` need `# Errors`; 2 register helpers need `# Panics`) | `cargo doc --document-private-items` succeeds; `cargo build` clean |
| #44 module docstrings | 16 files: `src/{main,config,notification,state,dbus,listeners,persistence}.rs` + `src/ui/{mod,css,dnd_menu,icons,notification_row,panel,panel_content,popup,window}.rs` (skips `src/ui/constants.rs` and `src/waybar.rs` — both already have `//!`) | `cargo doc --document-private-items` succeeds; `cargo build` clean |

Each issue gets its own commit. No CHANGELOG entry — pure internal documentation with zero user-visible impact (per the resolution from PR #54).

---

## Pre-flight

- [ ] **Sync main and create branch**

```bash
cd /data/source/nwg-notifications
git checkout main && git pull --ff-only
git status
git checkout -b chore/docs-sweep-44-45-49
```

Expected: clean tree on `main`, then a fresh branch.

- [ ] **Commit the plan file as the first commit on the branch**

```bash
git add docs/superpowers/plans/2026-05-03-bundle-docs-sweep-44-45-49.md
git commit -m "docs: implementation plan for docs-sweep bundle (#44 #45 #49)"
```

- [ ] **Baseline full cargo gambit + `cargo doc` to surface the missing-docs starting state**

```bash
make lint
cargo doc --document-private-items --no-deps 2>&1 | tail -5
```

Expected: `make lint` exits 0 (pre-existing `cargo deny` "unmatched skip" warnings are non-blocking; test count is 83). `cargo doc` succeeds; note any current warnings as the baseline.

---

## Task 1: #49 — Inline comment on the GTK `hold_guard`, plus a draft #16 sub-bullet

The smallest change in the bundle and the cleanest way to start the branch. The hold-guard pattern in `src/main.rs` keeps the GApplication alive past the activate-then-idle window — obvious to anyone who knows GTK, opaque otherwise. Two lines of `//` comment, then commit.

The second AC ("#16 gets a sub-bullet") is delegated to the user per the established convention that Jason owns all GitHub-facing communication outside CodeRabbit threads. The plan provides the drafted bullet text in the PR body; Jason pastes it onto #16 manually after merge.

**Files:**
- Modify: `src/main.rs` — add a 2-3 line `//` comment immediately above `let hold_guard: ...` near the top of `main()`.

- [ ] **Step 1: Locate the hold_guard declaration**

```bash
grep -n "hold_guard\|app.hold" src/main.rs
```

Expected: three matches — `let hold_guard`, `let hold_ref = Rc::clone(&hold_guard);`, and `*hold_ref.borrow_mut() = Some(app.hold());` inside the `connect_activate` closure.

- [ ] **Step 2: Add the inline comment above `let hold_guard`**

In `src/main.rs`, find:

```rust
    let config = Rc::new(RefCell::new(config));
    let hold_guard: Rc<RefCell<Option<gio::ApplicationHoldGuard>>> = Rc::new(RefCell::new(None));
    let hold_ref = Rc::clone(&hold_guard);
```

Replace with:

```rust
    let config = Rc::new(RefCell::new(config));
    // GApplication exits as soon as the activate handler returns idle. As a
    // notification daemon we need to stay resident — the popup manager,
    // panel, D-Bus server, and signal listener all rely on the glib main
    // loop continuing to run. `app.hold()` returns a guard that increments
    // GApplication's hold count; storing it in this RefCell keeps it
    // alive for the daemon's lifetime. Drop the guard to let the
    // application exit cleanly. See the GTK `GApplication` docs for
    // hold/release semantics.
    let hold_guard: Rc<RefCell<Option<gio::ApplicationHoldGuard>>> = Rc::new(RefCell::new(None));
    let hold_ref = Rc::clone(&hold_guard);
```

- [ ] **Step 3: Build + clippy + fmt**

```bash
cargo build
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: clean build, no clippy warnings, no fmt drift.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "$(cat <<'EOF'
Document the GTK hold_guard pattern inline (#49)

GApplication exits as soon as the activate handler returns idle.
For a notification daemon — popups, panel, D-Bus server, signal
listener all driven by the glib main loop — we need to stay
resident past the activate window. app.hold() returns a guard
that increments GApplication's hold count; storing it in a RefCell
keeps the daemon alive. Obvious from context if you know GTK, but
opaque otherwise.

Adds a comment above the hold_guard declaration in main()
explaining the pattern.

The second AC for #49 ("issue #16 gets a sub-bullet about the
daemon-doesn't-exit-on-idle case") is handled in the PR body — a
drafted bullet for Jason to paste onto #16, since the project
convention reserves all GitHub-facing communication outside
CodeRabbit threads to him.

Closes #49.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: #45 — `# Errors` / `# Panics` rustdoc sections in `src/dbus.rs`

7 `pub(crate) fn` returning `Result<_, glib::Error>` (the `query_count_via_dbus` query helper + 6 `push_*` setters) need `# Errors` sections. The 2 private `register_*` helpers contain 6 `.expect()` sites that, while unreachable in practice, deserve `# Panics` documentation.

**Files:**
- Modify: `src/dbus.rs` — add `# Errors` to 7 functions, `# Panics` to 2 functions.

**Investigation (already done; baked into this task):**
- 7 `pub(crate) fn -> Result<_, glib::Error>`: `query_count_via_dbus` plus the six `push_*` setters (`push_popup_position`, `push_popup_width`, `push_panel_width`, `push_popup_timeout`, `push_max_popups`, `push_max_history`). All exit with `glib::Error` from the underlying `connection.call_sync` — same shape per function, so the `# Errors` text is the same template per setter.
- 2 register helpers contain `.expect()`: `register_notification_object` (3 expects: parse XML, lookup interface, register object) and `register_nwg_count_object` (3 expects: same shape for the nwg-count surface). All panic sites trip only on build-time misconfiguration since the XML is `const` and the object path is a literal — explain that.

- [ ] **Step 1: Add `# Errors` to `query_count_via_dbus`**

In `src/dbus.rs`, find the `pub(crate) fn query_count_via_dbus` definition (look for the `query_count_via_dbus` symbol; it sits in the bottom half of the file, before the `push_*` block). Above the existing `pub(crate) fn` line, ensure there is a docstring block of this shape. If a docstring already exists, append the `# Errors` section to it; if there is no docstring, insert this whole block:

```rust
/// Queries the running `nwg-notifications` daemon for its current
/// unread-notification count via the `org.nwg.Notifications.GetCount`
/// D-Bus method. Used by `nwg-notifications --count`.
///
/// Uses `gio::DBusCallFlags::NO_AUTO_START` so the call never spawns
/// a fresh daemon instance — if no daemon is running, the call
/// errors out instead of starting one and timing out on the
/// initialization handshake.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable (no D-Bus, no `DBUS_SESSION_BUS_ADDRESS`).
/// - No daemon owns the `org.nwg.Notifications` name (`NO_AUTO_START` semantics).
/// - The call exceeds `QUERY_COUNT_TIMEOUT_MS`.
/// - The reply payload doesn't unpack to the expected `(u32,)` tuple.
pub(crate) fn query_count_via_dbus() -> Result<u32, glib::Error> {
```

(If `query_count_via_dbus` already has any prose docstring, keep it — only add the `# Errors` section at the end.)

- [ ] **Step 2: Add `# Errors` to each of the 6 `push_*` wrappers**

In `src/dbus.rs`, find the cluster of `pub(crate) fn push_*` definitions (they sit immediately below `query_count_via_dbus`; they're nearly identical thin wrappers over `connection.call_sync` for the live-config setters introduced in #20). For each of the six (`push_popup_position`, `push_popup_width`, `push_panel_width`, `push_popup_timeout`, `push_max_popups`, `push_max_history`), insert this docstring directly above the existing `pub(crate) fn` line.

`push_popup_position`:

```rust
/// Pushes a `--popup-position` change to the running daemon via
/// `org.nwg.Notifications.SetPopupPosition`. Used by
/// `nwg-notifications --update --popup-position <value>`.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable.
/// - No daemon owns the `org.nwg.Notifications` name (the
///   `NO_AUTO_START` flag means the call doesn't spawn one).
/// - The daemon rejects the value with
///   `org.freedesktop.DBus.Error.InvalidArgs` (for example, an
///   unrecognised position string).
/// - The daemon's running version doesn't expose `SetPopupPosition`
///   yet — surfaced as `org.freedesktop.DBus.Error.UnknownMethod`,
///   which the CLI's `--update` path translates into the
///   "restart-after-upgrade" hint.
pub(crate) fn push_popup_position(value: &str) -> Result<(), glib::Error> {
```

`push_popup_width`:

```rust
/// Pushes a `--popup-width <px>` change to the running daemon via
/// `org.nwg.Notifications.SetPopupWidth`. Used by
/// `nwg-notifications --update --popup-width <px>`.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable.
/// - No daemon owns the `org.nwg.Notifications` name.
/// - The daemon rejects the value with
///   `org.freedesktop.DBus.Error.InvalidArgs` (for example, a value
///   outside the 100..=2000 range).
/// - The daemon's running version doesn't expose `SetPopupWidth`
///   yet — surfaced as `org.freedesktop.DBus.Error.UnknownMethod`.
pub(crate) fn push_popup_width(value: u32) -> Result<(), glib::Error> {
```

`push_panel_width`:

```rust
/// Pushes a `--panel-width <px>` change to the running daemon via
/// `org.nwg.Notifications.SetPanelWidth`. Used by
/// `nwg-notifications --update --panel-width <px>`.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable.
/// - No daemon owns the `org.nwg.Notifications` name.
/// - The daemon rejects the value with
///   `org.freedesktop.DBus.Error.InvalidArgs` (for example, a value
///   outside the 200..=2000 range).
/// - The daemon's running version doesn't expose `SetPanelWidth`
///   yet — surfaced as `org.freedesktop.DBus.Error.UnknownMethod`.
pub(crate) fn push_panel_width(value: u32) -> Result<(), glib::Error> {
```

`push_popup_timeout`:

```rust
/// Pushes a `--popup-timeout <secs>` change to the running daemon via
/// `org.nwg.Notifications.SetPopupTimeout`. Used by
/// `nwg-notifications --update --popup-timeout <secs>`.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable.
/// - No daemon owns the `org.nwg.Notifications` name.
/// - The daemon rejects the value with
///   `org.freedesktop.DBus.Error.InvalidArgs` (for example, a
///   value outside the validated range for `--popup-timeout`).
/// - The daemon's running version doesn't expose `SetPopupTimeout`
///   yet — surfaced as `org.freedesktop.DBus.Error.UnknownMethod`.
pub(crate) fn push_popup_timeout(value: u32) -> Result<(), glib::Error> {
```

`push_max_popups`:

```rust
/// Pushes a `--max-popups <N>` change to the running daemon via
/// `org.nwg.Notifications.SetMaxPopups`. Used by
/// `nwg-notifications --update --max-popups <N>`.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable.
/// - No daemon owns the `org.nwg.Notifications` name.
/// - The daemon rejects the value with
///   `org.freedesktop.DBus.Error.InvalidArgs` (for example, a
///   value outside the validated range for `--max-popups`).
/// - The daemon's running version doesn't expose `SetMaxPopups`
///   yet — surfaced as `org.freedesktop.DBus.Error.UnknownMethod`.
pub(crate) fn push_max_popups(value: u32) -> Result<(), glib::Error> {
```

`push_max_history`:

```rust
/// Pushes a `--max-history <N>` change to the running daemon via
/// `org.nwg.Notifications.SetMaxHistory`. Used by
/// `nwg-notifications --update --max-history <N>`.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable.
/// - No daemon owns the `org.nwg.Notifications` name.
/// - The daemon rejects the value with
///   `org.freedesktop.DBus.Error.InvalidArgs` (for example, a
///   value outside the validated range for `--max-history`).
/// - The daemon's running version doesn't expose `SetMaxHistory`
///   yet — surfaced as `org.freedesktop.DBus.Error.UnknownMethod`.
pub(crate) fn push_max_history(value: u32) -> Result<(), glib::Error> {
```

- [ ] **Step 3: Add `# Panics` to the two `register_*` helpers**

In `src/dbus.rs`, find the two private `fn register_*` helpers (`register_notification_object` and `register_nwg_count_object`). Each contains three `.expect()` calls — XML parse, interface lookup, register-object build. Insert this docstring above each existing `fn` line.

`register_notification_object`:

```rust
/// Registers the daemon's `org.freedesktop.Notifications` D-Bus
/// object on the given connection. Wires `handle_method` as the
/// method-call dispatcher.
///
/// # Panics
///
/// Panics on three unreachable-in-practice failure modes, all of
/// which represent a build-time misconfiguration rather than a
/// runtime condition:
/// - `INTROSPECT_XML` fails to parse — the XML is a `const &str`
///   in this file, so a parse failure means we shipped malformed
///   XML and CI should have caught it.
/// - The `org.freedesktop.Notifications` interface name doesn't
///   resolve in the parsed `DBusNodeInfo` — same `const`-source
///   provenance as above.
/// - `register_object` fails to build — the object path is a
///   string literal and the interface info just came from the
///   parsed `const`, so any failure here would be a bug in `gio`'s
///   builder.
fn register_notification_object(
```

`register_nwg_count_object`:

```rust
/// Registers the daemon's `org.nwg.Notifications` D-Bus object on
/// the given connection. Backs `GetCount`, the six `Set*` live-config
/// setters, and the `CountChanged` signal source.
///
/// # Panics
///
/// Panics on three unreachable-in-practice failure modes, all of
/// which represent a build-time misconfiguration rather than a
/// runtime condition:
/// - `NWG_COUNT_INTROSPECT_XML` fails to parse — the XML is a
///   `const &str` in this file, so a parse failure means we
///   shipped malformed XML and CI should have caught it.
/// - The `org.nwg.Notifications` interface name doesn't resolve
///   in the parsed `DBusNodeInfo` — same `const`-source
///   provenance as above.
/// - `register_object` fails to build — the object path is a
///   string literal and the interface info just came from the
///   parsed `const`.
fn register_nwg_count_object(
```

- [ ] **Step 4: Build + clippy + fmt + cargo doc**

```bash
cargo build
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo doc --document-private-items --no-deps 2>&1 | tail -5
```

Expected: clean build, no clippy warnings, no fmt drift, `cargo doc` succeeds with the same warning count as the pre-flight baseline (we haven't introduced any new pub items).

- [ ] **Step 5: Commit**

```bash
git add src/dbus.rs
git commit -m "$(cat <<'EOF'
Document Result-returning fns and .expect() panic sites in dbus.rs (#45)

Seven pub(crate) fn returning Result<_, glib::Error> in src/dbus.rs
lacked # Errors rustdoc sections — query_count_via_dbus plus the
six push_* live-config setters introduced in #20. Two private
register_* helpers each contain three .expect() calls (XML parse,
interface lookup, register-object build) without # Panics docs.

Added # Errors sections covering the four error classes shared by
all setters (no bus, no daemon, InvalidArgs, UnknownMethod) and
the four classes for query_count_via_dbus (no bus, no daemon,
timeout, payload-shape). Added # Panics sections to both
register_* helpers explaining that the three .expect() sites are
unreachable in practice — they trip only on build-time
misconfiguration since the XML is const and the object path is
a literal.

cargo doc --document-private-items remains clean.

Closes #45.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: #44 — Module-level `//!` docstrings on every `src/*.rs` and `src/ui/*.rs`

Every `.rs` file under `src/` except `src/ui/constants.rs` and `src/waybar.rs` (both already have `//!`) gets a 1-3 line module docstring. The wording leans on CLAUDE.md's "What lives where" section — this is the place to lift those one-liners into the source tree itself.

**Files:**
- Modify: 16 files. Each gets a `//!` block prepended above the existing first line.

**Investigation (already done; baked into this task):**
- `grep -l "^//!" src/*.rs src/ui/*.rs` returns only `src/waybar.rs` and `src/ui/constants.rs`. All 16 other files need a `//!` opener.
- Each docstring should be a single short paragraph describing the file's responsibility, not a tutorial. Wording is taken from CLAUDE.md's "What lives where" plus per-file inspection.

- [ ] **Step 1: Add `//!` to `src/main.rs`**

Prepend at the very top of `src/main.rs`, above `mod config;`:

```rust
//! Coordinator: wires daemon state, popup manager, panel, D-Bus
//! server, and signal listener. Owns the GTK `Application` and the
//! short-circuit CLI modes (`--count`, `--update`) that exit before
//! claiming the singleton lock.

```

- [ ] **Step 2: Add `//!` to `src/config.rs`**

Prepend at the very top of `src/config.rs`, above the existing `use` lines:

```rust
//! clap CLI definition (`NotificationConfig`, `PopupPosition` enum)
//! and the `value_source`-based filter that lets `--update` push
//! only the flags the user actually passed rather than reset the
//! rest to their defaults.

```

- [ ] **Step 3: Add `//!` to `src/notification.rs`**

Prepend at the very top of `src/notification.rs`:

```rust
//! `Notification`, `Urgency`, and the freedesktop-spec helpers for
//! body-markup stripping (`clean_markup`) and action-list parsing
//! (`parse_actions`).

```

- [ ] **Step 4: Add `//!` to `src/state.rs`**

Prepend at the very top of `src/state.rs`:

```rust
//! `NotificationState`: the daemon's mutable state — notification
//! history, app-grouped views, DND mode (with optional expiry),
//! the active-popups set, and the `dbus_connection` slot used by
//! callbacks to emit signals back through the same connection that
//! handled the originating method call.

```

- [ ] **Step 5: Add `//!` to `src/dbus.rs`**

Prepend at the very top of `src/dbus.rs`:

```rust
//! D-Bus server for the notification daemon. Claims
//! `org.freedesktop.Notifications` (the freedesktop-spec interface)
//! and `org.nwg.Notifications` (the project-private interface used
//! by `nwg-shell-config` and `nwg-panel` for live config + count
//! IPC). Runs directly on the glib main loop via
//! `gio::bus_own_name`; no async bridge.

```

- [ ] **Step 6: Add `//!` to `src/listeners.rs`**

Prepend at the very top of `src/listeners.rs`:

```rust
//! Signal-thread → mpsc → glib timeout bridge. A dedicated thread
//! waits on `SIGRTMIN+4` (toggle panel), `SIGRTMIN+5` (toggle DND),
//! and `SIGRTMIN+6` (DND duration menu); the main glib loop polls
//! the receiver via `glib::timeout_add_local` so the side effects
//! happen on the GTK thread.

```

- [ ] **Step 7: Add `//!` to `src/persistence.rs`**

Prepend at the very top of `src/persistence.rs`:

```rust
//! Notification history serialization. Round-trips the
//! `Vec<Notification>` to a JSON file in the cache directory so
//! `--persist` mode survives daemon restarts. Tolerates missing
//! and corrupt files by returning an empty history.

```

- [ ] **Step 8: Add `//!` to `src/ui/mod.rs`**

Prepend at the very top of `src/ui/mod.rs`:

```rust
//! GTK4 widgets and layer-shell setup: the auto-dismissing popup
//! toasts, the slide-out history panel, the DND duration picker,
//! shared layout constants, and the icon / CSS helpers.

```

- [ ] **Step 9: Add `//!` to `src/ui/css.rs`**

Prepend at the very top of `src/ui/css.rs`, above `use nwg_common::config::css;`:

```rust
//! CSS loader. Reads the embedded default stylesheet at startup
//! and installs it on the default `gdk::Display` so every GTK
//! widget in the daemon picks it up. Hot reload comes from
//! `nwg_common::config::css` watching the user override path.

```

- [ ] **Step 10: Add `//!` to `src/ui/dnd_menu.rs`**

Prepend at the very top of `src/ui/dnd_menu.rs`:

```rust
//! Do-Not-Disturb duration picker. A small layer-shell popup
//! shown on right-click of the waybar bell — lets the user pick
//! "1 hour", "until tomorrow", "until I turn it off", etc., and
//! arms a glib timer that flips DND off when the chosen expiry
//! arrives.

```

- [ ] **Step 11: Add `//!` to `src/ui/icons.rs`**

Prepend at the very top of `src/ui/icons.rs`:

```rust
//! Notification-specific icon helpers. `resolve_popup_icon` and
//! `resolve_theme_icon` wrap `nwg_common::desktop::icons` with the
//! pixbuf / theme-variant logic that the popup and panel rendering
//! paths need. Kept local to this crate rather than promoted to
//! `nwg-common` because the helpers are notification-specific
//! (icon-name + app-name fallback chain, themed-image rendering).

```

- [ ] **Step 12: Add `//!` to `src/ui/notification_row.rs`**

Prepend at the very top of `src/ui/notification_row.rs`:

```rust
//! History-panel notification row widget. `build_row` composes the
//! per-notification GTK row (icon, summary, body, relative
//! timestamp, dismiss button) shown inside the slide-out panel's
//! grouped list. `relative_time_from_elapsed` formats the row's
//! age label.

```

- [ ] **Step 13: Add `//!` to `src/ui/panel.rs`**

Prepend at the very top of `src/ui/panel.rs`:

```rust
//! Slide-out history panel. A layer-shell window pinned to the
//! right edge of the screen showing every notification grouped by
//! app, with a backdrop click-out target. `NotificationPanel`
//! owns the show / hide / rebuild lifecycle.

```

- [ ] **Step 14: Add `//!` to `src/ui/panel_content.rs`**

Prepend at the very top of `src/ui/panel_content.rs`:

```rust
//! Panel-content builder. `build_grouped_list` consumes the
//! current `NotificationState` and produces the GTK widget tree
//! shown inside the history panel — one section per app, each
//! containing the notification rows produced by
//! `super::notification_row`.

```

- [ ] **Step 15: Add `//!` to `src/ui/popup.rs`**

Prepend at the very top of `src/ui/popup.rs`:

```rust
//! Auto-dismissing popup toasts. `PopupManager` owns the per-popup
//! windows, the position-aware stacking logic, and the timeout
//! that closes each popup after the configured `--popup-timeout`.
//! `focus_app` deep-links from a popup click to the notifying
//! application via the compositor's `focus_window` IPC.

```

- [ ] **Step 16: Add `//!` to `src/ui/window.rs`**

Prepend at the very top of `src/ui/window.rs`:

```rust
//! Layer-shell window setup helpers. `setup_popup_window` configures
//! a popup `ApplicationWindow` with the right `gtk4-layer-shell`
//! anchors and margins for its `PopupPosition`. Also exports the
//! backdrop-window helper used by panel and DND menu for
//! click-outside-to-close.

```

- [ ] **Step 17: Build + clippy + fmt + cargo doc**

```bash
cargo build
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo doc --document-private-items --no-deps 2>&1 | tail -5
```

Expected: clean build, no clippy warnings, no fmt drift, `cargo doc` succeeds. The 16 modules are now present in the rustdoc output with proper module-level summaries.

- [ ] **Step 18: Commit**

```bash
git add src/
git commit -m "$(cat <<'EOF'
Add //! module docstrings to every src/*.rs and src/ui/*.rs (#44)

Only src/ui/constants.rs and src/waybar.rs had module docstrings
(the latter added in #35 alongside the nerd-font const sweep).
The CLAUDE.md "What lives where" section already documented each
file's purpose in one line — lifted into //! openers so
rust-analyzer surfaces them on hover and
cargo doc --document-private-items reads as a real architecture
overview rather than a wall of unnamed modules.

16 files touched. No code change; pure rustdoc additions.

Closes #44.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Pre-PR gates + smoke install

Per repo convention. This bundle is documentation-only with no behavior changes, so the smoke is symbolic — just confirm the daemon still launches and a notification still appears.

- [ ] **Step 1: Install to user bin and confirm restart works**

```bash
make upgrade PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
```

Expected: build → install → kill running daemon → respawn. `pidof nwg-notifications` returns the new PID.

- [ ] **Step 2: Hand off to the user — STOP HERE**

Tell the user (verbatim or close):

> Installed and restarted via `make upgrade`. This bundle is
> pure rustdoc/comment additions — zero behavior change. Smoke
> is symbolic:
>
> 1. `notify-send "smoke" "test"` — popup still appears.
> 2. Open the panel — history entry shows.
>
> Reply when satisfied.

**Do not proceed to Task 5 until the user explicitly approves.**

- [ ] **Step 3: Full lint after smoke approval**

```bash
make lint
cargo doc --document-private-items --no-deps 2>&1 | tail -5
```

Expected: every step exits 0; pre-existing `cargo deny` "unmatched skip" warnings unchanged; total test count remains 83; `cargo doc` succeeds clean.

- [ ] **Step 4: Push**

```bash
git push -u origin chore/docs-sweep-44-45-49
```

---

## Task 5: Open PR

- [ ] **Step 1: Open the PR with the drafted #16 sub-bullet in the body**

The PR body includes a drafted bullet for Jason to paste onto issue #16, since the project convention reserves all GitHub-facing communication outside CodeRabbit threads to him. The drafted bullet sits at the bottom of the PR body under a clearly-labelled section.

```bash
gh pr create --base main --head chore/docs-sweep-44-45-49 \
  --title "Bundle docs sweep: #44 #45 #49" \
  --body "$(cat <<'EOF'
## Summary

Bundles three documentation items from epic #29 into one PR:

- **#49** — Inline comment above `let hold_guard` in `src/main.rs` explaining the GTK `app.hold()` pattern (why a notification daemon needs to keep `GApplication` alive past the activate-then-idle window).
- **#45** — Added `# Errors` rustdoc sections to the seven `pub(crate) fn` returning `Result<_, glib::Error>` in `src/dbus.rs` (`query_count_via_dbus` + the six `push_*` live-config setters from #20). Added `# Panics` sections to the two private `register_*` helpers explaining that their three `.expect()` sites each trip only on build-time misconfiguration since the XML is `const` and the object path is a literal.
- **#44** — Added module-level `//!` docstrings to every `src/*.rs` and `src/ui/*.rs` file that didn't already have one. 16 files touched. Wording lifted from CLAUDE.md's "What lives where" so the source tree reads consistently with the project README.

One commit per issue. No CHANGELOG entry — pure documentation, zero user-visible impact.

## Test plan

- [x] `make lint` clean locally (fmt + clippy + test + deny + audit). Test count unchanged at 83.
- [x] `cargo doc --document-private-items --no-deps` succeeds; `cargo doc` output is now organized around the new module docstrings.
- [x] Manual smoke test against the live compositor (installed via `make upgrade PREFIX=\$HOME/.local BINDIR=\$HOME/.cargo/bin`): `notify-send` produces a popup; the panel still opens.

## Drafted #16 sub-bullet (for Jason to paste manually)

The second AC for #49 asks that issue #16 (D-Bus integration tests) gain a sub-bullet covering the daemon-doesn't-exit-on-idle case. Per the project convention that the maintainer owns all user-facing GitHub edits outside CodeRabbit threads, the drafted bullet lives here for manual application:

> - At least one liveness test for the GTK \`hold_guard\` pattern: spawn the daemon, give it a moment to settle past activate, then assert the process is still running (i.e. \`gio::ApplicationHoldGuard\` is keeping \`GApplication\` resident as intended). Today this is only covered by manual smoke after \`make upgrade\`.

Paste under the existing "Acceptance" checklist on #16.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 2: Wait for CodeRabbit + iterate**

Default-fix posture per repo convention. Inline reply per in-diff comment, single PR-level reply for outside-diff items, tag `@coderabbitai` every time. Do not respond to non-bot commenters under the maintainer's account.

---

## Notes

- **No CHANGELOG entry.** Pure documentation, zero user-visible impact — same posture as PR #54 and #55.
- **#49's second AC is delegated to the user.** The drafted sub-bullet for issue #16 lives in the PR body. After merge, Jason pastes it onto #16 (or its successor when the integration-test infra arrives). Per memory: "draft text for him, never post under his account; CodeRabbit threads are the exception."
- **`# Errors` template re-use.** The six `push_*` setters share a common error-class set (no bus / no daemon / InvalidArgs / UnknownMethod). Each docstring is templated identically with only the method name and the validated-range hint changing — DRY at the documentation level isn't worth a macro here, just visual consistency.
- **Module docstring wording is short by design.** Each `//!` is one short paragraph, not a tutorial. The README + CLAUDE.md remain the long-form architecture docs; module docstrings exist so `rust-analyzer` and `cargo doc` give the next contributor a useful 30-second orientation.
