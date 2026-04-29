# Configurable Popup Width + freedesktop UnknownMethod Fallthrough Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Two related changes in one PR:
1. **#11** — Add a `--popup-width <px>` clap flag (default `POPUP_WIDTH_DEFAULT = 380`, clamped to `100..=2000` at parse time) so `nwg-shell-config` can drive popup width via its `swaync-notification-window-width` preset. Plumb through to `gtk_widget_set_size_request()` on every popup, not just the first.
2. **#15** — Tighten the `_ =>` fallthrough in `handle_method` for the existing `org.freedesktop.Notifications` interface so unknown methods return the standard `org.freedesktop.DBus.Error.UnknownMethod` instead of leaving clients to wait out their reply timeout. Mirrors the fix already merged for `handle_nwg_count_method` in PR #14.

**Architecture:** `--popup-width` is a single clap flag whose value is read inside `PopupManager::show()` for each new popup, replacing the existing constant reference. Bounds checking happens declaratively via `clap::value_parser!(i32).range(100..=2000)` — out-of-range values reject at parse time with a clear message in `--help`. The freedesktop UnknownMethod fix is one new `invocation.return_dbus_error(...)` call alongside the existing `log::warn!`, copied from the nwg-count handler that landed in #14.

**Tech Stack:** Rust, `clap` (derive + range value_parser), gtk4 widget sizing, `gio::DBusMethodInvocation::return_dbus_error`.

**Tracks:** Closes [#11](https://github.com/jasonherald/nwg-notifications/issues/11) and [#15](https://github.com/jasonherald/nwg-notifications/issues/15). Part of the [nwg-shell-config integration epic (#8)](https://github.com/jasonherald/nwg-notifications/issues/8).

---

## File Structure

- **Modify:** `src/ui/constants.rs` — rename `POPUP_WIDTH` to `POPUP_WIDTH_DEFAULT` (the constant is the *default*, not "the" width once `--popup-width` exists). One-line rename + comment update.
- **Modify:** `src/config.rs` — add `popup_width: i32` to `NotificationConfig` with `value_parser` range `100..=2000` and default `POPUP_WIDTH_DEFAULT`. Add tests for default, in-range, and out-of-range parses.
- **Modify:** `src/ui/popup.rs` — replace the two `POPUP_WIDTH` reads in `PopupManager::show()` (lines 65–66) with `self.config.popup_width`. Drop `POPUP_WIDTH` from the `use super::constants::{...}` import block.
- **Modify:** `src/dbus.rs` — extend `handle_method`'s `_ =>` arm with `invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", ...)` mirroring the nwg-count handler.
- **Modify:** `CHANGELOG.md` — entries under unreleased: `### Added` for `--popup-width`, `### Fixed` for the freedesktop fallthrough.
- **Modify:** `README.md` — only if it enumerates flag values; flag-name-only mentions don't need updating.

No new files, no module restructuring. Both fixes touch the public daemon surface in different files; bundling them is justified because each is too small to merit its own PR (per user direction: bigger PRs give CodeRabbit better signal).

---

## Pre-flight

- [ ] **Confirm working directory and branch**

```bash
cd /data/source/nwg-notifications
git checkout main && git pull --ff-only
git status
git checkout -b feat/popup-width-and-fdo-fallthrough
```

Expected: clean tree on `main` synced to origin, then on a fresh branch `feat/popup-width-and-fdo-fallthrough`. If the tree isn't clean, stop and ask.

- [ ] **Commit the plan file as the first commit on the branch**

```bash
git add docs/superpowers/plans/2026-04-28-popup-width-and-fdo-fallthrough.md
git commit -m "docs: implementation plan for --popup-width + freedesktop UnknownMethod fallthrough (#11 #15)"
```

- [ ] **Baseline full cargo gambit before any change**

```bash
make lint
```

Expected: every step exits 0. `cargo deny` may print pre-existing warnings about stale `deny.toml` skip entries — non-blocking, out of scope. Fall back to running each piece directly (`cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check && cargo audit`) if `make lint` is unavailable.

---

## Task 1: #15 — Return UnknownMethod from `handle_method` fallthrough

The smallest, most isolated change. Lands first.

**Files:**
- Modify: `src/dbus.rs` — `handle_method` function, the `_ =>` match arm.

- [ ] **Step 1: Read the current fallthrough**

Run: `grep -n -A3 '"GetServerInformation" => handle_server_info' src/dbus.rs`

Confirm the structure: there's a `match method { ... _ => { log::warn!(...); } }` block. The plan only modifies the `_ =>` arm.

- [ ] **Step 2: Add `return_dbus_error` to the fallthrough**

Edit `src/dbus.rs::handle_method`. Replace:

```rust
        _ => {
            log::warn!("Unknown D-Bus method: {}", method);
        }
```

with:

```rust
        _ => {
            log::warn!("Unknown D-Bus method: {}", method);
            invocation.return_dbus_error(
                "org.freedesktop.DBus.Error.UnknownMethod",
                &format!("Unknown method: {method}"),
            );
        }
```

This mirrors the fix already in `handle_nwg_count_method` (PR #14, commit `fee56fe`). Same error name, same error-message shape.

- [ ] **Step 3: Build, test, clippy**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean. No new tests are added — this is a one-line change to a code path that requires a live D-Bus connection to exercise, and we explicitly tracked the integration-test gap as #16 (deferred). Manual smoke test (Task 5) covers it.

- [ ] **Step 4: Commit**

```bash
git add src/dbus.rs
git commit -m "$(cat <<'EOF'
Return UnknownMethod for org.freedesktop.Notifications fallthrough (#15)

Mirrors the fix that landed for handle_nwg_count_method in PR #14
(commit fee56fe): the fallthrough arm now logs *and* calls
invocation.return_dbus_error(...) with the standard
org.freedesktop.DBus.Error.UnknownMethod, so introspection-driven
clients see the error immediately instead of waiting out their reply
timeout.

Closes #15.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: #11 — Rename `POPUP_WIDTH` to `POPUP_WIDTH_DEFAULT`

Mechanical rename. Done before adding the flag so the import in `popup.rs` already points at the renamed default by the time we wire the flag.

**Files:**
- Modify: `src/ui/constants.rs`
- Modify: `src/ui/popup.rs` (import + two usages — both will be replaced again in Task 4 anyway, but this keeps the build green at every commit)

- [ ] **Step 1: Rename in `src/ui/constants.rs`**

Replace:

```rust
/// Width of popup notification windows.
pub const POPUP_WIDTH: i32 = 380;
```

with:

```rust
/// Default width of popup notification windows. Overridable via the
/// `--popup-width` CLI flag (clamped to 100..=2000 at parse time).
pub const POPUP_WIDTH_DEFAULT: i32 = 380;
```

- [ ] **Step 2: Update `src/ui/popup.rs` to use the renamed constant**

In the `use super::constants::{...}` block at the top of the file, change `POPUP_WIDTH` to `POPUP_WIDTH_DEFAULT`. Also update the two usages in `PopupManager::show()`:

```rust
        win.set_width_request(POPUP_WIDTH_DEFAULT);
        win.set_default_size(POPUP_WIDTH_DEFAULT, -1);
```

These will get replaced again in Task 4. The intermediate commit just keeps the constant rename atomic.

- [ ] **Step 3: Build, test, clippy**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean. No behavior change.

- [ ] **Step 4: Commit**

```bash
git add src/ui/constants.rs src/ui/popup.rs
git commit -m "$(cat <<'EOF'
Rename POPUP_WIDTH to POPUP_WIDTH_DEFAULT (#11)

Pure rename ahead of adding the --popup-width flag in the next commit.
The constant is the *default* width, not "the" width once user
configuration enters the picture; renaming it now keeps the next
commit's diff focused on the flag plumbing.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: #11 — Add `--popup-width` clap flag with TDD

**Files:**
- Modify: `src/config.rs` (add field + tests in the existing `#[cfg(test)] mod tests` block)

- [ ] **Step 1: Write the failing tests**

Add to the bottom of the `#[cfg(test)] mod tests` block in `src/config.rs`:

```rust
    #[test]
    fn popup_width_defaults_to_constant() {
        let config = NotificationConfig::parse_from(["test"]);
        assert_eq!(
            config.popup_width,
            crate::ui::constants::POPUP_WIDTH_DEFAULT
        );
    }

    #[test]
    fn popup_width_accepts_in_range_value() {
        let config = NotificationConfig::parse_from(["test", "--popup-width", "500"]);
        assert_eq!(config.popup_width, 500);
    }

    #[test]
    fn popup_width_rejects_below_minimum() {
        let result = NotificationConfig::try_parse_from(["test", "--popup-width", "50"]);
        assert!(result.is_err(), "expected --popup-width=50 to be rejected");
    }

    #[test]
    fn popup_width_rejects_above_maximum() {
        let result = NotificationConfig::try_parse_from(["test", "--popup-width", "5000"]);
        assert!(
            result.is_err(),
            "expected --popup-width=5000 to be rejected"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test popup_width_ 2>&1 | tail -10`

Expected: compile error — `popup_width` field doesn't exist on `NotificationConfig` yet.

- [ ] **Step 3: Add the field and bring `POPUP_WIDTH_DEFAULT` into scope**

In `src/config.rs`, add this `use` near the existing `use clap::{Parser, ValueEnum};`:

```rust
use crate::ui::constants::POPUP_WIDTH_DEFAULT;
```

Then add the field to `NotificationConfig`, alongside the other popup-related flags:

```rust
    /// Popup window width in pixels. Clamped to 100..=2000.
    #[arg(
        long,
        value_parser = clap::value_parser!(i32).range(100..=2000),
        default_value_t = POPUP_WIDTH_DEFAULT,
    )]
    pub popup_width: i32,
```

`clap`'s `value_parser!(i32).range(...)` rejects out-of-range values at parse time with a message that includes the range; `--help` shows `[default: 380]` and the range bounds.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test popup_width_ 2>&1 | tail -10`

Expected: all 4 tests pass.

- [ ] **Step 5: Verify `--help` shows the range**

```bash
cargo build
./target/debug/nwg-notifications --help 2>&1 | grep -B1 -A4 popup-width
```

Expected: `--help` block for `--popup-width` shows `[default: 380]` and the range `100..=2000` (or equivalent clap formatting).

- [ ] **Step 6: Build, test, clippy**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/config.rs
git commit -m "$(cat <<'EOF'
Add --popup-width clap flag (#11)

Range-validated 100..=2000 via clap's value_parser, defaults to the
existing POPUP_WIDTH_DEFAULT (380). Out-of-range values reject at parse
time with a clear message; --help shows both default and range.

Field is added to NotificationConfig but not yet read anywhere — next
commit threads it into PopupManager::show.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: #11 — Plumb `popup_width` through `PopupManager::show`

**Files:**
- Modify: `src/ui/popup.rs` — replace `POPUP_WIDTH_DEFAULT` reads in `show()` with `self.config.popup_width`; drop the constants import.

- [ ] **Step 1: Update `PopupManager::show`**

In `src/ui/popup.rs`, find the two width reads in `PopupManager::show()`:

```rust
        win.set_width_request(POPUP_WIDTH_DEFAULT);
        win.set_default_size(POPUP_WIDTH_DEFAULT, -1);
```

Replace with:

```rust
        win.set_width_request(self.config.popup_width);
        win.set_default_size(self.config.popup_width, -1);
```

`PopupManager` already holds `config: Rc<NotificationConfig>` (see `PopupManager::new`), so `self.config.popup_width` is in scope without further plumbing.

- [ ] **Step 2: Drop `POPUP_WIDTH_DEFAULT` from the constants import**

The top of `src/ui/popup.rs` has a multi-import like:

```rust
use super::constants::{
    POPUP_BODY_CHARS, POPUP_GAP, POPUP_ICON_SIZE, POPUP_MAX_BODY_LINES, POPUP_PADDING,
    POPUP_SUMMARY_CHARS, POPUP_TOP_MARGIN, POPUP_WIDTH_DEFAULT,
};
```

Remove `POPUP_WIDTH_DEFAULT` from that block (the constant is no longer referenced from this file — it lives in `config.rs` now as the clap default). Result:

```rust
use super::constants::{
    POPUP_BODY_CHARS, POPUP_GAP, POPUP_ICON_SIZE, POPUP_MAX_BODY_LINES, POPUP_PADDING,
    POPUP_SUMMARY_CHARS, POPUP_TOP_MARGIN,
};
```

If clippy or rustc complain that the import is now unused, you've removed the wrong one — re-check Step 1.

- [ ] **Step 3: Build, test, clippy**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean. The default-width behavior is unchanged because `config.popup_width` defaults to `POPUP_WIDTH_DEFAULT`.

- [ ] **Step 4: Commit**

```bash
git add src/ui/popup.rs
git commit -m "$(cat <<'EOF'
Apply --popup-width to every popup (#11)

PopupManager::show now reads self.config.popup_width instead of the
POPUP_WIDTH_DEFAULT constant. Width is applied per-popup, so the third
notification gets the configured width just like the first.

Closes #11.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Documentation

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `README.md` (only if it enumerates flag values)

- [ ] **Step 1: CHANGELOG entries**

Find the `## [X.Y.Z] — Unreleased` section. Under its `### Added` block (creating it if it doesn't exist), append:

```markdown
- `--popup-width <px>` flag controls popup window width. Defaults to 380px;
  range 100..=2000 enforced at parse time. (#11)
```

Then add a `### Fixed` block. Per Keep-a-Changelog conventional order (Added, Changed, Deprecated, Removed, Fixed, Security), `### Fixed` goes after `### Changed`. If the unreleased section currently only has `### Added` and `### Changed`, append `### Fixed` after `### Changed`. Body:

```markdown
- `org.freedesktop.Notifications` D-Bus handler now returns the standard
  `org.freedesktop.DBus.Error.UnknownMethod` for unknown methods instead of
  silently logging, so introspection-driven clients see the error
  immediately instead of waiting out their reply timeout. Mirrors the fix
  for the nwg-count handler in #14. (#15)
```

If `### Changed` is already in the section, place `### Fixed` after it. The conventional order is what Keep-a-Changelog prescribes; preserve whatever bucket order the file already uses for visual consistency.

- [ ] **Step 2: Check if `README.md` enumerates flag values**

```bash
grep -n 'popup-width\|--popup-' README.md | head -20
```

If `README.md` lists the available flags (e.g. in a "Usage" or "Configuration" section), add a one-liner for `--popup-width` matching the surrounding style. If it only references one or two flags by name without full enumeration, leave it alone.

- [ ] **Step 3: Build, test, clippy**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md README.md
git status
```

Stage only files actually modified.

```bash
git commit -m "$(cat <<'EOF'
docs: --popup-width + freedesktop UnknownMethod fix (#11 #15)

CHANGELOG entries under unreleased — Added for --popup-width, Fixed for
the freedesktop fallthrough error response.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: User smoke-test gate (HARD STOP)

The unit tests cover clap parsing and bounds; manual verification is needed for the live D-Bus path (#15) and visual width application (#11).

- [ ] **Step 1: Install to the user's `~/.cargo/bin`**

```bash
make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
```

Don't run `make install-dbus` — the D-Bus service file template is unchanged.

- [ ] **Step 2: Restart the user's session daemon so it picks up the new flag handling and the freedesktop fallthrough fix**

```bash
kill "$(pidof nwg-notifications)" 2>/dev/null || true
sleep 0.5
uwsm-app -- nwg-notifications --persist >/dev/null 2>&1 &
disown
sleep 1
pidof nwg-notifications && echo "(daemon up)"
```

(NEVER use `pkill -f nwg-notifications` — it self-matches the bash subprocess and kills the user's live daemon.)

- [ ] **Step 3: Hand off to the user — STOP HERE**

Tell the user (verbatim or close):
> Installed and restarted the daemon. Smoke-test paths:
>
> **#11 (popup width):**
> 1. `notify-send "default width" "should look the same as before"` — verify the popup is the existing 380px width.
> 2. Restart with `kill $(pidof nwg-notifications); nwg-notifications --persist --popup-width 600 &` and `notify-send "wide" "wider than default"` — popup should be visibly wider. Send 2-3 in quick succession to confirm every popup picks up the configured width, not just the first.
> 3. `nwg-notifications --popup-width 50` should reject with a clap error mentioning the range. `nwg-notifications --popup-width 5000` should also reject.
> 4. Restart back to default (`nwg-notifications --persist`) before continuing.
>
> **#15 (freedesktop UnknownMethod):**
> 5. `gdbus call --session --dest org.freedesktop.Notifications --object-path /org/freedesktop/Notifications --method org.freedesktop.Notifications.NoSuchMethod` — expect an immediate `org.freedesktop.DBus.Error.UnknownMethod` error, not a multi-second hang.
>
> Reply when satisfied or with anything that needs fixing.

**Do not proceed to Task 7 until the user explicitly approves.** If the user reports issues, return to the task that owns the broken behavior.

---

## Task 7: Full cargo gambit (CI parity)

- [ ] **Step 1: Run `make lint`**

```bash
make lint
```

If `cargo fmt --check` reformats anything, run `cargo fmt --all`, commit as `style: cargo fmt`, and re-run `make lint`. If `cargo deny`/`cargo audit` flags something *new* (not the pre-existing stale-skip warnings), stop and ask the user.

- [ ] **Step 2: Confirm clean working tree**

```bash
git status
```

Expected: clean.

---

## Task 8: Open the PR

Gated on Task 6 (user smoke-test approval) AND Task 7 (clean `make lint`).

- [ ] **Step 1: Push the branch**

```bash
git push -u origin feat/popup-width-and-fdo-fallthrough
```

- [ ] **Step 2: Create the PR**

```bash
gh pr create --title "Add --popup-width flag + freedesktop UnknownMethod fix (#11 #15)" --body "$(cat <<'EOF'
## Summary

Two related daemon-surface changes bundled together (per workflow guidance to give CodeRabbit larger PRs to review):

1. **#11 — `--popup-width <px>` flag.** clap-validated to `100..=2000`, defaults to 380px (the previous hardcoded value, now exposed as `POPUP_WIDTH_DEFAULT`). Read by `PopupManager::show()` per popup, so every popup picks up the configured width — not just the first.
2. **#15 — freedesktop UnknownMethod fallthrough fix.** Mirrors the fix already in `handle_nwg_count_method` (PR #14, commit `fee56fe`): the `_ =>` arm in `handle_method` now calls `invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", ...)` alongside the existing log line, so introspection-driven clients see the standard error immediately instead of timing out.

Part of the [nwg-shell-config integration epic (#8)](https://github.com/jasonherald/notifications/issues/8). Closes #11 and #15.

## Test plan

- [x] `make lint` — fmt + clippy + test + deny + audit, all green locally.
- [x] Unit tests (4 new for `--popup-width`):
  - Default value matches `POPUP_WIDTH_DEFAULT`.
  - In-range value (500) parses.
  - Below-minimum value (50) rejects at parse time.
  - Above-maximum value (5000) rejects at parse time.
- [x] Manual smoke test against the live compositor (installed via `make install PREFIX=\$HOME/.local BINDIR=\$HOME/.cargo/bin`):
  - [x] Default `--popup-width` produces the same visual as before.
  - [x] `--popup-width 600` produces visibly wider popups; verified across multiple consecutive popups (every one applies, not just the first).
  - [x] `--popup-width 50` and `--popup-width 5000` reject at parse time with clap's range error.
  - [x] `gdbus call ... org.freedesktop.Notifications.NoSuchMethod` returns `UnknownMethod` immediately instead of timing out.

## Notes

- `POPUP_WIDTH` was renamed to `POPUP_WIDTH_DEFAULT` for clarity once user-configurable widths exist. Pure rename in a separate commit ahead of the flag work, so the flag commit's diff stays focused.
- No new tests added for #15 itself — D-Bus dispatch isn't unit-testable in this repo without a fake-bus harness, which is tracked separately as [#16](https://github.com/jasonherald/nwg-notifications/issues/16). Manual smoke verification covers it.

The implementation plan (committed as \`docs/superpowers/plans/2026-04-28-popup-width-and-fdo-fallthrough.md\`) is on the branch for reviewer context.
EOF
)"
```

Expected: returns the PR URL.

- [ ] **Step 3: Hand off to CodeRabbit**

CodeRabbit reviews within minutes. Iterate per the per-finding reply protocol: inline replies for in-diff comments, single PR-level comment for outside-diff items, tag `@coderabbitai` every time so it learns from the responses.

**Default to fixing every finding in-PR.** Defer only when the fix needs new infrastructure or has wide blast radius — and when you do defer, open a tracking issue *first* before justifying it in the reply. See `feedback_track_deferrals.md`.

---

## Acceptance checklist

**#11:**
- [ ] `--popup-width <px>` flag accepts integers, defaults to current behavior. — Task 3
- [ ] Width is applied to all popups, not just the first. — Task 4 (read from `self.config.popup_width` per call to `show()`)
- [ ] Reasonable bounds: clamped to 100..=2000; documented in `--help`. — Task 3 (via `value_parser!(i32).range(...)`)
- [ ] CHANGELOG entry. — Task 5
- [ ] Unit test for the resolution function. — Task 3 (4 unit tests against clap parse)

**#15:**
- [ ] `handle_method`'s `_ =>` arm calls `invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", ...)`. — Task 1
- [ ] `gdbus call` smoke-test confirms unknown methods now return the error immediately. — Task 6
