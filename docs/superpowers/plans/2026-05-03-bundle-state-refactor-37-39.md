# Bundle State Refactor (#39, #37) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bundle two internal-state refactors from epic #29 — extract the two filename-construction helpers into a new `src/paths.rs` module (#39), and centralize the five `state.dnd` / `state.dnd_expires` write sites into a single `NotificationState::set_dnd(enabled, expires)` helper (#37). The latter also fixes a latent stale-`dnd_expires` bug in two of the call sites.

**Architecture:** Both refactors are pure "pull repeated logic into one place." #39 creates a new `paths` module that owns both filename conventions; `persistence.rs` and `waybar.rs` import them. #37 adds an `impl` method that becomes the only writer of the `dnd` and `dnd_expires` fields; all five existing write sites route through it (each passing the expiry it wants — signal/header pass `None`, timed-DND passes `Some(expiry)`, the timer-fire passes `None`). One commit per issue, #39 first because it's mechanical with no behavior change.

**Tech Stack:** Rust modules, `Rc<RefCell<NotificationState>>` shared-state pattern, `std::time::SystemTime` for the expiry token.

**Tracks:** Closes #39, #37. Both are children of epic #29.

---

## File Structure

| Task | Files modified | Test approach |
|------|----------------|---------------|
| #39 paths module | New `src/paths.rs` (lifts both helpers, **with per-UID `/tmp/nwg-notifications-<uid>/` mode-0700 sandbox + symlink/foreign-owner refusal as the degraded-environment fallback** — not raw `/tmp`); `src/main.rs` (declares `mod paths;`); `src/persistence.rs` (drops `history_path`); `src/waybar.rs` (drops `status_path`); `src/main.rs` (call site `persistence::history_path()` → `paths::history_path()`) | New `#[cfg(test)] mod tests` covers the four fallback cases (XDG_RUNTIME_DIR / XDG_CACHE_HOME / cache_dir-falls-through / per-UID sandbox) plus a `try_create_or_validate` safety-check refusal test; `make lint` green |
| #37 set_dnd helper | `src/state.rs` (add `set_dnd` method, update existing test write site, add new unit test); `src/listeners.rs` (`ToggleDnd` arm); `src/ui/panel.rs` (panel header DND button); `src/ui/dnd_menu.rs` (3 sites: toggle, timed-DND button, timer-fire) | New unit test asserts the bug fix: enabling timed DND then toggling via signal correctly clears `dnd_expires` |

Each issue gets its own commit. No CHANGELOG entry — internal refactor; the bug fix in #37 is a latent issue that's never been reported (the stale `dnd_expires` is mostly harmless because the timer-fire token check makes it a no-op if the user cycles quickly enough; worst case the timer fires later and re-clears DND that's already off, which is invisible).

---

## Pre-flight

- [ ] **Sync main and create branch**

```bash
cd /data/source/nwg-notifications
git checkout main && git pull --ff-only
git status
git checkout -b chore/state-refactor-37-39
```

Expected: clean tree on `main`, then a fresh branch.

- [ ] **Commit the plan file as the first commit on the branch**

```bash
git add docs/superpowers/plans/2026-05-03-bundle-state-refactor-37-39.md
git commit -m "docs: implementation plan for state-refactor bundle (#37 #39)"
```

- [ ] **Baseline full cargo gambit**

```bash
make lint
```

Expected: every step exits 0; pre-existing `cargo deny` "unmatched skip" warnings are non-blocking. Test count is 87 going in.

---

## Task 1: #39 — Extract `history_path` and `status_path` into a new `paths` module

`history_path()` in `persistence.rs` and `status_path()` in `waybar.rs` are two near-identical "join a base dir against a fixed filename" helpers. Lift them into a new `src/paths.rs` so the filename conventions live together and the eventual `mac-notifications-*` → `nwg-notifications-*` rename in #34 is a one-file diff.

**Files:**
- Create: `src/paths.rs`
- Modify: `src/main.rs` — add `mod paths;` declaration alongside the existing `mod` lines + update the `persistence::history_path()` call site
- Modify: `src/persistence.rs` — delete the local `history_path()` function (its body moves to `paths`); the file shouldn't import `paths` itself since `load_history` and `save_history` already take a `&Path` parameter
- Modify: `src/waybar.rs` — delete the local `status_path()` function; the call site in `update_status` switches to `crate::paths::status_path()`

**Note on the implementation that actually shipped:** the snippet below shows the bare-bones initial cut to keep the step focused. Two follow-up CodeRabbit findings on PR #58 hardened it before merge:
- The `/tmp` fallback became a per-UID sandbox `/tmp/nwg-notifications-<uid>/` created with mode `0700` via atomic `mkdir(2)`. On `AlreadyExists` the helper validates `is_dir + owned_by_uid + mode == 0700` and refuses (falling through to a per-PID variant) if any check fails. Defends against symlink-clobber and foreign-owned-dir attacks the bare `/tmp/...` form was vulnerable to.
- `XDG_RUNTIME_DIR=""` is filtered as if unset (otherwise `PathBuf::from("")` resolves status path against CWD).
- A `#[cfg(test)] mod tests` covers all four fallback cases serially via a `Mutex<()>`, plus a `try_create_or_validate` safety-check refusal test.

If you're re-running this plan from scratch on a fresh branch, prefer the hardened shape that's currently in `src/paths.rs` over the snippet below — the snippet stays as the *minimal first commit* that this plan originally produced; the hardening shipped as follow-up commits in the same PR.

- [ ] **Step 1: Create `src/paths.rs`**

Create the new file at `src/paths.rs` with this content:

```rust
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
```

- [ ] **Step 2: Declare the new module in `src/main.rs`**

In `src/main.rs`, find the existing `mod` declarations at the top:

```rust
mod config;
mod dbus;
mod listeners;
mod notification;
mod persistence;
mod state;
mod ui;
mod waybar;
```

Insert `mod paths;` alphabetically between `mod notification;` and `mod persistence;`:

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

- [ ] **Step 3: Update the `persistence::history_path()` call site in `main.rs`**

In `src/main.rs`, find the line in `activate_notifications`:

```rust
    let history_path = persistence::history_path();
```

Replace with:

```rust
    let history_path = paths::history_path();
```

- [ ] **Step 4: Delete `history_path()` from `src/persistence.rs`**

In `src/persistence.rs`, delete the function definition entirely:

```rust
/// Returns the path to the notification history file.
pub(crate) fn history_path() -> PathBuf {
    nwg_common::config::paths::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("mac-notifications-history.json")
}
```

The remaining functions in `persistence.rs` (`load_history`, `save_history`) already take a `&Path` parameter, so no other edits are needed in this file.

- [ ] **Step 5: Replace `status_path()` callers in `src/waybar.rs`**

In `src/waybar.rs`, find the function definition:

```rust
/// Returns the path to the waybar status file.
fn status_path() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join("mac-notifications-status.json")
}
```

Delete it entirely.

Then find the only call site, inside `update_status`:

```rust
    let path = status_path();
```

Replace with:

```rust
    let path = crate::paths::status_path();
```

- [ ] **Step 6: Drop the now-unused `PathBuf` import from `src/waybar.rs` if it's no longer referenced**

After deleting `status_path()`, check whether `waybar.rs` still uses `PathBuf`:

```bash
grep -n "PathBuf" src/waybar.rs
```

If the only remaining hit is the `use std::path::PathBuf;` import line (no actual uses), delete that import line. If `PathBuf` is still used elsewhere in the file, leave the import alone.

- [ ] **Step 7: Build, test, clippy, fmt**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: clean build; full 87-test suite still green (no test changes); clippy clean; no fmt drift.

- [ ] **Step 8: Commit**

```bash
git add src/paths.rs src/main.rs src/persistence.rs src/waybar.rs
git commit -m "$(cat <<'EOF'
Extract history_path + status_path into a new paths module (#39)

history_path() in persistence.rs and status_path() in waybar.rs
were two near-identical "join a base dir against a fixed filename"
helpers. Lifting them into a new src/paths.rs module puts the two
runtime-artifact filenames in one place, so #34's eventual
mac-notifications-* -> nwg-notifications-* rename is a single-file
change instead of two-file synchronization.

Both helpers keep their existing fallback semantics: history_path
falls back to /tmp when nwg_common can't resolve a cache dir;
status_path falls back to /tmp when XDG_RUNTIME_DIR isn't set.

The persistence.rs callers (load_history, save_history) take a
&Path parameter, so they didn't need an import update — only the
single persistence::history_path() call site in main.rs moves to
paths::history_path(). status_path's only caller is inside
waybar::update_status, which now reads crate::paths::status_path().

No behavioral change: cargo doc + cargo build clean, full 87-test
suite still green, manual smoke confirms the daemon writes to the
same paths it always did.

Closes #39.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: #37 — Centralize `state.dnd` writes into `NotificationState::set_dnd`

Five places currently mutate `state.dnd` (and sometimes `state.dnd_expires`). The signal handler in `listeners.rs::poll_signals` and the panel-header DND button in `ui/panel.rs::build_header` both leave `dnd_expires` stale when toggling — that's the latent bug the issue's AC test calls out. Routing every write through a single helper that takes `(enabled, expires)` makes the policy uniform and fixes the bug.

**Files:**
- Modify: `src/state.rs` — add `set_dnd` method, update existing test that direct-writes the field, add new test for the bug-fix path.
- Modify: `src/listeners.rs` — replace the `ToggleDnd` arm's direct write with `set_dnd`.
- Modify: `src/ui/panel.rs` — replace the panel-header button's direct write with `set_dnd`.
- Modify: `src/ui/dnd_menu.rs` — replace 3 direct-write sites (toggle button, timed-DND button, timer-fire callback) with `set_dnd`.

Note: `src/main.rs` initially kept the startup write as a direct field assignment (`state.borrow_mut().dnd = config.borrow().dnd;`) — it runs before the daemon is wired up, has no `on_state_change` to fire, and `dnd_expires` is already `None` from `NotificationState::new`. **That decision was reversed by a CodeRabbit follow-up on PR #58:** the `dnd` and `dnd_expires` fields are now fully private (the rabbit's "enforce the invariant at the type boundary" finding), so the startup init now reads `state.borrow_mut().set_dnd(initial_dnd, None)`. Cost: the daemon logs a `"DND enabled/disabled"` line at startup confirming whether `--dnd` was passed. Harmless and arguably useful.

- [ ] **Step 1: Add `set_dnd` to `NotificationState` in `src/state.rs`**

In `src/state.rs`, find the `impl NotificationState` block. Add this method just after `dismiss_all` and before `mark_read` (it sits with the other state-mutation methods):

```rust
    /// Sets DND mode and (optionally) the timed-DND expiry, then
    /// logs the transition. The single writer for the `dnd` and
    /// `dnd_expires` fields — every UI / signal / timer call site
    /// routes through this so the two fields can never drift.
    ///
    /// Pass `expires = None` for a permanent toggle (the panel
    /// header button, the signal handler, the menu's "until I turn
    /// it off" entry, and the timer-fire that clears expired DND).
    /// Pass `expires = Some(deadline)` only for the timed-DND menu
    /// buttons that arm a `glib::timeout_add_local_once`.
    pub(crate) fn set_dnd(&mut self, enabled: bool, expires: Option<std::time::SystemTime>) {
        self.dnd = enabled;
        self.dnd_expires = expires;
        log::info!("DND {}", if enabled { "enabled" } else { "disabled" });
    }
```

- [ ] **Step 2: Update the existing `dnd_suppresses_normal_popups` test to use the new helper**

In `src/state.rs`, find the test (it sits in the `#[cfg(test)] mod tests` block):

```rust
    #[test]
    fn dnd_suppresses_normal_popups() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        state.dnd = true;
        assert!(!state.should_show_popup(Urgency::Normal));
        assert!(!state.should_show_popup(Urgency::Low));
        assert!(state.should_show_popup(Urgency::Critical));
    }
```

Change the direct field assignment to a `set_dnd` call:

```rust
    #[test]
    fn dnd_suppresses_normal_popups() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        state.set_dnd(true, None);
        assert!(!state.should_show_popup(Urgency::Normal));
        assert!(!state.should_show_popup(Urgency::Low));
        assert!(state.should_show_popup(Urgency::Critical));
    }
```

- [ ] **Step 3: Add the new test for the bug-fix scenario**

In `src/state.rs`, append this test inside the `#[cfg(test)] mod tests` block (right after `dnd_suppresses_normal_popups`):

```rust
    #[test]
    fn set_dnd_clears_stale_expiry_when_toggling_off() {
        // Bug fix from #37: before the set_dnd helper landed, the
        // signal handler in listeners.rs and the panel-header button
        // in ui/panel.rs both flipped state.dnd directly without
        // touching dnd_expires. So a user who armed timed DND from
        // the menu (sets dnd=true + Some(expiry)) and then toggled
        // DND off via the waybar bell signal would end up with
        // dnd=false but dnd_expires=Some(stale). The set_dnd helper
        // makes (enabled, expires) a single atomic write — the
        // signal-handler path passes None, which clears the expiry.

        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));

        // Arm timed DND (mirrors the dnd_menu timed-DND button path).
        let expiry = SystemTime::now() + std::time::Duration::from_secs(3600);
        state.set_dnd(true, Some(expiry));
        assert!(state.dnd);
        assert_eq!(state.dnd_expires, Some(expiry));

        // Toggle off via signal (mirrors the listeners.rs ToggleDnd path).
        state.set_dnd(false, None);
        assert!(!state.dnd);
        assert_eq!(
            state.dnd_expires, None,
            "toggling DND off via signal must clear stale dnd_expires"
        );
    }
```

- [ ] **Step 4: Update `src/listeners.rs` to use `set_dnd`**

In `src/listeners.rs`, find the `ToggleDnd` arm (or the equivalent — look for the block where `state.borrow_mut().dnd = new_dnd;` is followed by a `log::info!("DND ..."` line). The current shape is roughly:

```rust
                    let new_dnd = !state.borrow().dnd;
                    state.borrow_mut().dnd = new_dnd;
                    log::info!(
                        "DND {}",
                        if new_dnd { "enabled" } else { "disabled" }
                    );
                    on_state_change();
```

Replace with:

```rust
                    let new_dnd = !state.borrow().dnd;
                    state.borrow_mut().set_dnd(new_dnd, None);
                    on_state_change();
```

The `log::info!` call is now redundant — `set_dnd` does the same log line internally — so it's deleted along with the direct-write line.

- [ ] **Step 5: Update `src/ui/panel.rs` to use `set_dnd`**

In `src/ui/panel.rs`, find the panel-header DND button click handler (`dnd_btn.connect_clicked(move |btn| { ... })`). The current block:

```rust
    dnd_btn.connect_clicked(move |btn| {
        let new_dnd = !state_dnd.borrow().dnd;
        state_dnd.borrow_mut().dnd = new_dnd;
        let icon = if new_dnd {
            "notifications-disabled-symbolic"
        } else {
            "preferences-system-notifications-symbolic"
        };
        btn.set_icon_name(icon);
        log::info!("DND {}", if new_dnd { "enabled" } else { "disabled" });
        on_change_dnd();
    });
```

Replace the `state_dnd.borrow_mut().dnd = new_dnd;` line and the trailing `log::info!` line with a single `set_dnd` call. The icon-update lines stay (UI-specific, out of `set_dnd`'s scope). New shape:

```rust
    dnd_btn.connect_clicked(move |btn| {
        let new_dnd = !state_dnd.borrow().dnd;
        state_dnd.borrow_mut().set_dnd(new_dnd, None);
        let icon = if new_dnd {
            "notifications-disabled-symbolic"
        } else {
            "preferences-system-notifications-symbolic"
        };
        btn.set_icon_name(icon);
        on_change_dnd();
    });
```

- [ ] **Step 6: Update the DND-menu toggle button in `src/ui/dnd_menu.rs`**

In `src/ui/dnd_menu.rs`, find the toggle-button click handler. Current shape:

```rust
        toggle_btn.connect_clicked(move |_| {
            let new_dnd = !state_toggle.borrow().dnd;
            state_toggle.borrow_mut().dnd = new_dnd;
            state_toggle.borrow_mut().dnd_expires = None;
            log::info!("DND {}", if new_dnd { "enabled" } else { "disabled" });
            on_change_toggle();
            win_toggle.set_visible(false);
            for b in &backdrops_toggle {
                b.set_visible(false);
            }
        });
```

Replace the two `state_toggle.borrow_mut().dnd = ...` / `dnd_expires = ...` writes and the `log::info!` line with one `set_dnd` call:

```rust
        toggle_btn.connect_clicked(move |_| {
            let new_dnd = !state_toggle.borrow().dnd;
            state_toggle.borrow_mut().set_dnd(new_dnd, None);
            on_change_toggle();
            win_toggle.set_visible(false);
            for b in &backdrops_toggle {
                b.set_visible(false);
            }
        });
```

- [ ] **Step 7: Update the timed-DND button (the one that arms the timer)**

In `src/ui/dnd_menu.rs::build_timed_dnd_button`, find the `btn.connect_clicked(move |_| { ... })` block. The change reorders two existing lines: the `expiry` calculation moves *up* so it can be passed to `set_dnd` and reused by the captured token. Current shape:

```rust
    btn.connect_clicked(move |_| {
        state_btn.borrow_mut().dnd = true;
        let expiry = std::time::SystemTime::now() + std::time::Duration::from_secs(minutes * 60);
        state_btn.borrow_mut().dnd_expires = Some(expiry);
        log::info!("DND enabled for {} minutes", minutes);

        // Capture the expiry we just stored. If the user clicks a
        // different duration before this timer fires, the stored
        // expiry will have been replaced; this timer's `expiry` token
        // won't match and the firing will no-op. Cleaner than
        // tracking a glib::SourceId and removing the previous source
        // (no removal lifetime concerns).
        let captured_expiry = expiry;
        let state_timer = Rc::clone(&state_btn);
        let on_change_timer = Rc::clone(&on_change);
        gtk4::glib::timeout_add_local_once(
            std::time::Duration::from_secs(minutes * 60),
            move || {
                let current = state_timer.borrow().dnd_expires;
                if current == Some(captured_expiry) {
                    state_timer.borrow_mut().dnd = false;
                    state_timer.borrow_mut().dnd_expires = None;
                    log::info!("Timed DND expired");
                    on_change_timer();
                }
                // else: a newer schedule replaced this one; no-op silently.
            },
        );

        on_change();
        win_btn.set_visible(false);
        for b in &backdrops_btn {
            b.set_visible(false);
        }
    });
```

Replace the two `state_btn.borrow_mut().*` writes plus the `log::info!` line at the top with: hoist the `expiry` calculation, then one `set_dnd` call. Keep the duration-specific `log::info!("DND enabled for {} minutes", minutes)` because `set_dnd`'s generic `"DND enabled"` log doesn't capture which duration the user chose. New shape (only the top of the closure changes; the `captured_expiry` / timer-arm / visibility-cleanup tail in this step remains as-is — Step 8 updates the timer-fire body):

```rust
    btn.connect_clicked(move |_| {
        let expiry = std::time::SystemTime::now() + std::time::Duration::from_secs(minutes * 60);
        state_btn.borrow_mut().set_dnd(true, Some(expiry));
        log::info!("DND enabled for {} minutes", minutes);

        // Capture the expiry we just stored. If the user clicks a
        // different duration before this timer fires, the stored
        // expiry will have been replaced; this timer's `expiry` token
        // won't match and the firing will no-op. Cleaner than
        // tracking a glib::SourceId and removing the previous source
        // (no removal lifetime concerns).
        let captured_expiry = expiry;
        let state_timer = Rc::clone(&state_btn);
        let on_change_timer = Rc::clone(&on_change);
        gtk4::glib::timeout_add_local_once(
            std::time::Duration::from_secs(minutes * 60),
            move || {
                let current = state_timer.borrow().dnd_expires;
                if current == Some(captured_expiry) {
                    state_timer.borrow_mut().dnd = false;
                    state_timer.borrow_mut().dnd_expires = None;
                    log::info!("Timed DND expired");
                    on_change_timer();
                }
                // else: a newer schedule replaced this one; no-op silently.
            },
        );

        on_change();
        win_btn.set_visible(false);
        for b in &backdrops_btn {
            b.set_visible(false);
        }
    });
```

- [ ] **Step 8: Update the timer-fire callback inside `build_timed_dnd_button`**

Still in `src/ui/dnd_menu.rs::build_timed_dnd_button`, find the `gtk4::glib::timeout_add_local_once` block. Current shape:

```rust
        gtk4::glib::timeout_add_local_once(
            std::time::Duration::from_secs(minutes * 60),
            move || {
                let current = state_timer.borrow().dnd_expires;
                if current == Some(captured_expiry) {
                    state_timer.borrow_mut().dnd = false;
                    state_timer.borrow_mut().dnd_expires = None;
                    log::info!("Timed DND expired");
                    on_change_timer();
                }
                // else: a newer schedule replaced this one; no-op silently.
            },
        );
```

Replace the two `state_timer.borrow_mut().*` writes plus the `log::info!` line with one `set_dnd` call. Keep the explicit `"Timed DND expired"` log line since `set_dnd`'s generic `"DND disabled"` doesn't capture the "via expiry timer" provenance:

```rust
        gtk4::glib::timeout_add_local_once(
            std::time::Duration::from_secs(minutes * 60),
            move || {
                let current = state_timer.borrow().dnd_expires;
                if current == Some(captured_expiry) {
                    state_timer.borrow_mut().set_dnd(false, None);
                    log::info!("Timed DND expired");
                    on_change_timer();
                }
                // else: a newer schedule replaced this one; no-op silently.
            },
        );
```

- [ ] **Step 9: Build, test, clippy, fmt**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: clean build; full suite passes with the new test bringing the count to 88 (87 → 88); clippy clean; no fmt drift.

The new `set_dnd_clears_stale_expiry_when_toggling_off` test should appear in the test output and pass.

- [ ] **Step 10: Confirm the `dnd` and `dnd_expires` fields have no remaining direct writers in production code**

```bash
grep -rn "\.dnd\s*=\s*\|\.dnd_expires\s*=\s*" src/ --include="*.rs" | grep -v "tests"
```

Expected: only writes inside the `set_dnd` body itself (`self.dnd = enabled;` and `self.dnd_expires = ...;`). The startup initialization in `activate_notifications` (`src/main.rs`) was originally planned to keep its direct field write, but a follow-up CodeRabbit finding on PR #58 made the fields fully private and added accessors — the startup init now reads `state.borrow_mut().set_dnd(initial_dnd, None)` so the privatization is enforced everywhere. The field privacy is module-scoped (`#[cfg(test)] mod tests` in `src/state.rs` can still touch the fields directly).

- [ ] **Step 11: Commit**

```bash
git add src/state.rs src/listeners.rs src/ui/panel.rs src/ui/dnd_menu.rs
git commit -m "$(cat <<'EOF'
Centralize state.dnd writes via NotificationState::set_dnd (#37)

Five places currently mutate state.dnd (sometimes alongside
state.dnd_expires) with three slightly-different side-effect
patterns. The signal handler in listeners.rs and the panel-header
DND button in ui/panel.rs both leave dnd_expires stale when
toggling — a latent bug because a user who armed timed DND from
the menu and then toggled DND off via the waybar bell would end
up with dnd=false but dnd_expires=Some(stale).

Add NotificationState::set_dnd(enabled, expires) as the single
writer of both fields. Each caller passes the expiry it wants:
- listeners.rs ToggleDnd: passes None (signal toggles always
  clear timed expiry — fixes the bug)
- ui/panel.rs panel-header DND button: passes None (same fix)
- ui/dnd_menu.rs toggle button: passes None (already cleared
  expiry, now does it via the helper)
- ui/dnd_menu.rs timed-DND button: passes Some(expiry)
- ui/dnd_menu.rs timer-fire callback: passes None

Logging moves into the helper so all sites get a consistent "DND
enabled/disabled" line. Two sites keep an additional log::info
call for context the helper can't capture (the timed-DND duration
in minutes; the "Timed DND expired" provenance for the timer fire).

The startup initialization in main.rs (state.borrow_mut().dnd =
config.borrow().dnd) intentionally stays as a direct field write
— it runs before the daemon is wired up, has no on_state_change
to fire, and dnd_expires is already None from
NotificationState::new.

New unit test set_dnd_clears_stale_expiry_when_toggling_off pins
the bug-fix scenario: arm timed DND, toggle off via the
signal-handler path, assert dnd_expires is cleared. The existing
dnd_suppresses_normal_popups test now exercises set_dnd as well.

Test count: 87 -> 88.

Closes #37.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Pre-PR gates + smoke install

Per repo convention. This bundle is internal refactor + one latent-bug fix, so the smoke focuses on the DND state machine — particularly the bug-fix path that the unit test exercises.

- [ ] **Step 1: Install to user bin and confirm restart works**

```bash
make upgrade PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
```

Expected: build → install → kill running daemon → respawn. `pidof nwg-notifications` returns the new PID.

- [ ] **Step 2: Hand off to the user — STOP HERE**

Tell the user (verbatim or close):

> Installed and restarted via `make upgrade`. This bundle adds the
> `set_dnd` helper + the `paths` module, no behavior change except
> the latent stale-`dnd_expires` bug fix. Smoke:
>
> 1. `notify-send "smoke" "test"` — popup appears (verifies
>    nothing in the I/O paths broke from the `paths` module move).
> 2. Right-click the waybar bell to open the DND menu, click
>    "1 hour". Bell glyph flips to bell-off.
> 3. Now toggle DND off via the SIGRTMIN+5 signal (or the panel
>    header DND button if you have it). Bell glyph flips back.
>    Then re-enable timed DND from the menu — should still work
>    cleanly without an old timer racing to clear it (this is the
>    #37 bug-fix scenario).
> 4. Open the panel — history entries still load from the
>    persisted file (verifies the `history_path` move).
>
> Reply when satisfied.

**Do not proceed to Task 4 until the user explicitly approves.**

- [ ] **Step 3: Full lint after smoke approval**

```bash
make lint
```

Expected: every step exits 0; total test count is 88.

- [ ] **Step 4: Push**

```bash
git push -u origin chore/state-refactor-37-39
```

---

## Task 4: Open PR

- [ ] **Step 1: Open the PR**

```bash
gh pr create --base main --head chore/state-refactor-37-39 \
  --title "Bundle state refactor: #37 #39" \
  --body "$(cat <<'EOF'
## Summary

Bundles two internal-state refactors from epic #29 into one PR:

- **#39** — Lifted \`history_path\` (was in \`src/persistence.rs\`) and \`status_path\` (was in \`src/waybar.rs\`) into a new \`src/paths.rs\` module. The two filename conventions now sit together; #34's eventual \`mac-notifications-*\` → \`nwg-notifications-*\` rename is a single-file change. \`persistence.rs\` and \`waybar.rs\` shrink slightly; the only call-site change in \`main.rs\` is \`persistence::history_path()\` → \`paths::history_path()\`.
- **#37** — Added \`NotificationState::set_dnd(enabled, expires)\` as the single writer for the \`dnd\` and \`dnd_expires\` fields. Routed all 5 write sites through it (\`listeners.rs::poll_signals\` ToggleDnd arm, \`ui/panel.rs\` panel-header DND button, and 3 sites in \`ui/dnd_menu.rs\`: toggle button, timed-DND button, timer-fire callback). Logging moves into the helper. **Latent bug fix:** the signal-handler and panel-header paths used to leave \`dnd_expires\` stale when toggling off — now they pass \`None\` and clear it atomically. The startup initialization in \`main.rs\` keeps its direct field write (no \`on_state_change\` to fire, runs before the daemon is wired).

One commit per issue. No CHANGELOG entry — the bug fix is a latent issue (mostly harmless because the timer-fire token check makes the stale expiry a no-op).

## Test plan

- [x] \`make lint\` clean locally (fmt + clippy + test + deny + audit). Test count: 87 → 88 (+1 for the #37 bug-fix test).
- [x] Manual smoke test against the live compositor (installed via \`make upgrade PREFIX=\$HOME/.local BINDIR=\$HOME/.cargo/bin\`):
  - [x] \`notify-send\` produces a popup; persisted history still round-trips.
  - [x] Arm timed DND from the menu, toggle off via signal, re-enable from menu — no stale timer races to clear it.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 2: Wait for CodeRabbit + iterate**

Default-fix posture per repo convention. Inline reply per in-diff comment, single PR-level reply for outside-diff items, tag \`@coderabbitai\` every time. Do not respond to non-bot commenters under the maintainer's account.

---

## Notes

- **No CHANGELOG entry.** Pure internal refactor + a latent bug fix that's never been reported (the stale `dnd_expires` is mostly harmless because of the timer-fire token check landed in #31).
- **Why `set_dnd` and not `toggle_dnd`?** The callers each compute `new_dnd = !current` themselves and pass it in explicitly. Keeps the helper free of "what does toggle mean if expiry is also passed?" ambiguity. Each caller's intent reads cleanly (`set_dnd(true, Some(expiry))` is unambiguous; `toggle_dnd(Some(expiry))` would beg the question of what happens to the expiry on the OFF half of the toggle).
- **Why doesn't `set_dnd` fire `on_state_change` itself?** The callback is a `Rc<dyn Fn()>` owned by the call site, not by `NotificationState`. Threading it through every `set_dnd` call would mean either storing it on the state struct (couples state-mutation to the signal-out side) or passing it as a third parameter (clutters the signature). Leaving the call sites to fire `on_state_change` after `set_dnd` is the simpler shape.
