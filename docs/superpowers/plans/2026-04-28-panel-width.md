# Configurable History Panel Width Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `--panel-width <px>` clap flag (default 380px, range `200..=2000`) so `nwg-shell-config` can drive the slide-out history panel width via its `swaync-control-center-width` preset. Mirrors the `--popup-width` pattern that just landed in PR #17.

**Architecture:** Add `PANEL_WIDTH_MIN`, `PANEL_WIDTH_MAX`, and rename `PANEL_WIDTH` → `PANEL_WIDTH_DEFAULT` in `src/ui/constants.rs` (no-magic-numbers convention from `CLAUDE.md`). Add a clap-validated `panel_width: i32` field to `NotificationConfig` using `value_parser!(i32).range(...)` with the constants. Thread the chosen width into `NotificationPanel::new` as a new `panel_width: i32` parameter; the panel currently doesn't accept `&Rc<NotificationConfig>` and adding it just for one width field would over-abstract — passing the value directly is the YAGNI fit.

**Tech Stack:** Rust, `clap` (derive + range value_parser), gtk4 widget sizing.

**Tracks:** Closes [#12](https://github.com/jasonherald/nwg-notifications/issues/12). After this lands, all four child issues of the [nwg-shell-config integration epic (#8)](https://github.com/jasonherald/nwg-notifications/issues/8) are done — only deferred backlog (`#16` D-Bus integration tests) remains.

---

## File Structure

- **Modify:** `src/ui/constants.rs` — rename `PANEL_WIDTH` → `PANEL_WIDTH_DEFAULT`, add `PANEL_WIDTH_MIN: i32 = 200` and `PANEL_WIDTH_MAX: i32 = 2000`.
- **Modify:** `src/config.rs` — add `panel_width: i32` field with `value_parser!(i32).range((MIN as i64)..=(MAX as i64))` and default `PANEL_WIDTH_DEFAULT`. Tests for default, mid-range, both inclusive endpoints, and `MIN-1`/`MAX+1` rejection (6 tests, mirroring the popup-width pattern).
- **Modify:** `src/ui/panel.rs` — add `panel_width: i32` parameter to `NotificationPanel::new`; replace the two `PANEL_WIDTH` reads at lines 44 and 57 with the new parameter; drop `PANEL_WIDTH` from the constants import.
- **Modify:** `src/main.rs` — `NotificationPanel::new` call gains `config.panel_width` as a fifth argument.
- **Modify:** `CHANGELOG.md` — entry under unreleased `### Added`.

No new files. Mirrors the structure of the merged #11 PR almost exactly; the only architectural deviation is that `NotificationPanel` takes the width directly (i32) rather than the whole config, since it doesn't read any other config field.

---

## Pre-flight

- [ ] **Confirm working directory and branch**

```bash
cd /data/source/nwg-notifications
git checkout main && git pull --ff-only
git status
git checkout -b feat/panel-width
```

Expected: clean tree on `main` synced to origin, then a fresh branch `feat/panel-width`. If the tree isn't clean, stop and ask.

- [ ] **Commit the plan file as the first commit on the branch**

```bash
git add docs/superpowers/plans/2026-04-28-panel-width.md
git commit -m "docs: implementation plan for --panel-width (#12)"
```

- [ ] **Baseline full cargo gambit before any change**

```bash
make lint
```

Or fall back to `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check && cargo audit`.

Expected: every step exits 0. The pre-existing `cargo deny` "unmatched skip" warnings are non-blocking, out of scope.

---

## Task 1: Rename `PANEL_WIDTH` and add MIN/MAX bounds

Combined into a single commit: rename + new constants land together so the next commit's diff focuses purely on the clap flag.

**Files:**
- Modify: `src/ui/constants.rs`
- Modify: `src/ui/panel.rs` (import + two usages)

- [ ] **Step 1: Update `src/ui/constants.rs`**

Replace:

```rust
/// Width of the notification history panel.
pub const PANEL_WIDTH: i32 = 380;
```

with:

```rust
/// Minimum accepted value for the `--panel-width` CLI flag.
pub const PANEL_WIDTH_MIN: i32 = 200;

/// Maximum accepted value for the `--panel-width` CLI flag.
pub const PANEL_WIDTH_MAX: i32 = 2000;

/// Default width of the notification history panel. Overridable via the
/// `--panel-width` CLI flag (validated against
/// `PANEL_WIDTH_MIN..=PANEL_WIDTH_MAX` at parse time).
pub const PANEL_WIDTH_DEFAULT: i32 = 380;
```

- [ ] **Step 2: Update `src/ui/panel.rs` to use the renamed constant**

In the `use super::constants::{...}` line at the top, change `PANEL_WIDTH` to `PANEL_WIDTH_DEFAULT`:

```rust
use super::constants::{PANEL_REVEAL_DURATION_MS, PANEL_WIDTH_DEFAULT};
```

In `NotificationPanel::new`, replace the two reads:

```rust
        win.set_width_request(PANEL_WIDTH_DEFAULT);
```

```rust
        panel_box.set_width_request(PANEL_WIDTH_DEFAULT);
```

These will get replaced again in Task 3 with the parameterized value; the intermediate commit just keeps the rename atomic.

- [ ] **Step 3: Build, test, clippy**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean. No behavior change, no test additions.

- [ ] **Step 4: Commit**

```bash
git add src/ui/constants.rs src/ui/panel.rs
git commit -m "$(cat <<'EOF'
Rename PANEL_WIDTH and add PANEL_WIDTH_MIN/MAX bounds (#12)

Pure rename + bounds constants ahead of adding the --panel-width flag
in the next commit. PANEL_WIDTH becomes PANEL_WIDTH_DEFAULT (the
constant is the *default*, not "the" width once user configuration
exists) and PANEL_WIDTH_MIN=200 / PANEL_WIDTH_MAX=2000 land alongside
so the parser commit's diff stays focused on the new field.

Mirrors the POPUP_WIDTH_* trio from PR #17.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Add `--panel-width` clap flag (TDD, 6 tests)

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write the failing tests**

Append to the bottom of the `#[cfg(test)] mod tests` block in `src/config.rs`:

```rust
    #[test]
    fn panel_width_defaults_to_constant() {
        let config = NotificationConfig::parse_from(["test"]);
        assert_eq!(
            config.panel_width,
            crate::ui::constants::PANEL_WIDTH_DEFAULT
        );
    }

    #[test]
    fn panel_width_accepts_mid_range_value() {
        let mid =
            (crate::ui::constants::PANEL_WIDTH_MIN + crate::ui::constants::PANEL_WIDTH_MAX) / 2;
        let config = NotificationConfig::parse_from(["test", "--panel-width", &mid.to_string()]);
        assert_eq!(config.panel_width, mid);
    }

    #[test]
    fn panel_width_accepts_inclusive_minimum() {
        let min = crate::ui::constants::PANEL_WIDTH_MIN;
        let config = NotificationConfig::parse_from(["test", "--panel-width", &min.to_string()]);
        assert_eq!(config.panel_width, min);
    }

    #[test]
    fn panel_width_accepts_inclusive_maximum() {
        let max = crate::ui::constants::PANEL_WIDTH_MAX;
        let config = NotificationConfig::parse_from(["test", "--panel-width", &max.to_string()]);
        assert_eq!(config.panel_width, max);
    }

    #[test]
    fn panel_width_rejects_below_minimum() {
        let below = (crate::ui::constants::PANEL_WIDTH_MIN - 1).to_string();
        let result = NotificationConfig::try_parse_from(["test", "--panel-width", &below]);
        assert!(
            result.is_err(),
            "expected --panel-width={below} to be rejected"
        );
    }

    #[test]
    fn panel_width_rejects_above_maximum() {
        let above = (crate::ui::constants::PANEL_WIDTH_MAX + 1).to_string();
        let result = NotificationConfig::try_parse_from(["test", "--panel-width", &above]);
        assert!(
            result.is_err(),
            "expected --panel-width={above} to be rejected"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test panel_width_ 2>&1 | tail -10
```

Expected: compile error — `panel_width` field doesn't exist on `NotificationConfig`.

- [ ] **Step 3: Bring `PANEL_WIDTH_*` into scope**

In `src/config.rs`, the existing import of constants is:

```rust
use crate::ui::constants::{POPUP_WIDTH_DEFAULT, POPUP_WIDTH_MAX, POPUP_WIDTH_MIN};
```

Extend it to include the panel constants:

```rust
use crate::ui::constants::{
    PANEL_WIDTH_DEFAULT, PANEL_WIDTH_MAX, PANEL_WIDTH_MIN, POPUP_WIDTH_DEFAULT, POPUP_WIDTH_MAX,
    POPUP_WIDTH_MIN,
};
```

(`rustfmt` will sort the import list alphabetically; the exact line shape after fmt may differ slightly. That's fine.)

- [ ] **Step 4: Add the field**

In `NotificationConfig`, add the field next to `popup_width`:

```rust
    /// History panel width in pixels. Must be within
    /// `PANEL_WIDTH_MIN..=PANEL_WIDTH_MAX`; out-of-range values are rejected
    /// at parse time.
    #[arg(
        long,
        value_parser = clap::value_parser!(i32).range((PANEL_WIDTH_MIN as i64)..=(PANEL_WIDTH_MAX as i64)),
        default_value_t = PANEL_WIDTH_DEFAULT,
    )]
    pub panel_width: i32,
```

The doc comment is phrased the same way as `popup_width`'s post-CodeRabbit revision: "Must be within ... rejected at parse time" — accurate to clap's actual behavior (validate, not clamp).

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test panel_width_ 2>&1 | tail -10
```

Expected: all 6 tests pass.

- [ ] **Step 6: Verify `--help` reflects the new flag**

```bash
cargo build
./target/debug/nwg-notifications --help 2>&1 | grep -A4 panel-width
```

Expected: `--help` shows `--panel-width` with `[default: 380]` and the doc-comment text mentioning `PANEL_WIDTH_MIN..=PANEL_WIDTH_MAX`.

- [ ] **Step 7: Build, test, clippy**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add src/config.rs
git commit -m "$(cat <<'EOF'
Add --panel-width clap flag (#12)

Range-validated 200..=2000 via clap's value_parser, defaults to
PANEL_WIDTH_DEFAULT (380). Out-of-range values reject at parse time.

6 unit tests cover: default, mid-range derived from constants, both
inclusive endpoints accept, and MIN-1 / MAX+1 reject. Mirrors the
popup_width test pattern from PR #17.

Field is added to NotificationConfig but not yet read anywhere — next
commit threads it into NotificationPanel::new.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Plumb `panel_width` into `NotificationPanel::new`

**Files:**
- Modify: `src/ui/panel.rs` — add `panel_width: i32` parameter to `new()`; use it in the two `set_width_request` calls; drop `PANEL_WIDTH_DEFAULT` from the constants import.
- Modify: `src/main.rs` — pass `config.panel_width` to the `NotificationPanel::new` call.

- [ ] **Step 1: Add the parameter to `NotificationPanel::new` and update callers**

In `src/ui/panel.rs`, the current signature is:

```rust
    pub fn new(
        app: &gtk4::Application,
        state: &Rc<RefCell<NotificationState>>,
        on_notification_click: Rc<dyn Fn(u32)>,
        on_state_change: Rc<dyn Fn()>,
    ) -> Self {
```

Add `panel_width: i32` as the last parameter:

```rust
    pub fn new(
        app: &gtk4::Application,
        state: &Rc<RefCell<NotificationState>>,
        on_notification_click: Rc<dyn Fn(u32)>,
        on_state_change: Rc<dyn Fn()>,
        panel_width: i32,
    ) -> Self {
```

Replace the two `PANEL_WIDTH_DEFAULT` reads inside `new()` with `panel_width`:

```rust
        win.set_width_request(panel_width);
```

```rust
        panel_box.set_width_request(panel_width);
```

Drop `PANEL_WIDTH_DEFAULT` from the constants import at the top of the file. After Task 1 the import line reads:

```rust
use super::constants::{PANEL_REVEAL_DURATION_MS, PANEL_WIDTH_DEFAULT};
```

After this task `PANEL_WIDTH_DEFAULT` is no longer referenced from this file, so the import collapses to a single-name form:

```rust
use super::constants::PANEL_REVEAL_DURATION_MS;
```

(If `rustfmt` has wrapped the import into a multi-line form, drop just the `PANEL_WIDTH_DEFAULT` entry rather than rewriting the whole line.)

- [ ] **Step 2: Update the call site in `src/main.rs`**

The current `NotificationPanel::new` call in `activate_notifications`:

```rust
    let panel = Rc::new(RefCell::new(NotificationPanel::new(
        app,
        &state,
        on_panel_click,
        Rc::clone(&on_state_change),
    )));
```

Add `config.panel_width` as the fifth argument:

```rust
    let panel = Rc::new(RefCell::new(NotificationPanel::new(
        app,
        &state,
        on_panel_click,
        Rc::clone(&on_state_change),
        config.panel_width,
    )));
```

- [ ] **Step 3: Build, test, clippy**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean. The default-width behavior is unchanged because `config.panel_width` defaults to `PANEL_WIDTH_DEFAULT`.

- [ ] **Step 4: Commit**

```bash
git add src/ui/panel.rs src/main.rs
git commit -m "$(cat <<'EOF'
Apply --panel-width to the history panel (#12)

NotificationPanel::new now takes a panel_width: i32 parameter instead
of reading PANEL_WIDTH_DEFAULT directly, and main.rs passes
config.panel_width when constructing it.

Passing the width as i32 rather than the whole &Rc<NotificationConfig>
keeps the panel layer free of config-shape coupling — it currently
needs only this one knob.

Closes #12.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Documentation

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: CHANGELOG entry**

Find the active `## [X.Y.Z] — Unreleased` section. In its `### Added` block, append:

```markdown
- `--panel-width <px>` flag controls history panel width. Defaults to 380px;
  range 200..=2000 enforced at parse time. (#12)
```

- [ ] **Step 2: README check**

```bash
grep -n 'panel-width\|PANEL_WIDTH' README.md
```

If `README.md` enumerates flags, add a one-liner for `--panel-width` matching the surrounding style. If it doesn't enumerate flags (which is what happened for `--popup-width` in PR #17), leave it alone.

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
docs: --panel-width (#12)

CHANGELOG entry under unreleased Added.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: User smoke-test gate (HARD STOP)

The unit tests cover clap parsing and bounds; manual verification confirms the actual visual width applies to the panel.

- [ ] **Step 1: Install to the user's `~/.cargo/bin`**

```bash
make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
```

Don't run `make install-dbus` — D-Bus service file unchanged.

- [ ] **Step 2: Restart the user's session daemon**

```bash
kill "$(pidof nwg-notifications)" 2>/dev/null || true
sleep 0.5
uwsm-app -- nwg-notifications --persist >/dev/null 2>&1 &
disown
sleep 1
pidof nwg-notifications && echo "(daemon up)"
```

(Never `pkill -f nwg-notifications` — self-match + kills user's live daemon.)

- [ ] **Step 3: Hand off to the user — STOP HERE**

Tell the user (verbatim or close):
> Installed and restarted the daemon. Smoke-test paths:
>
> 1. Click the waybar bell icon to toggle the panel — verify it slides in at the existing 380px width.
> 2. Restart with a wider panel by running `kill "$(pidof nwg-notifications)" 2>/dev/null && sleep 0.5 && nwg-notifications --persist --panel-width 600 &`. Send a test notification (`notify-send "test" "msg"`), then toggle the panel — should be visibly wider.
> 3. `nwg-notifications --panel-width 100` should reject (below 200 minimum). `nwg-notifications --panel-width 5000` should reject. Both with clap range messages.
> 4. Restart back to default (`uwsm-app -- nwg-notifications --persist`) before continuing.
>
> Reply when satisfied.

**Do not proceed to Task 6 until the user explicitly approves.** If the user reports issues, return to the broken task.

---

## Task 6: Full cargo gambit (CI parity)

- [ ] **Step 1: `make lint`**

```bash
make lint
```

If `cargo fmt --check` reformats anything, run `cargo fmt --all`, commit as `style: cargo fmt`, re-run `make lint`. Stop on new deny/audit findings.

- [ ] **Step 2: Confirm clean working tree**

```bash
git status
```

Expected: clean.

---

## Task 7: Open the PR

Gated on Task 5 (user smoke-test approval) AND Task 6 (clean `make lint`).

- [ ] **Step 1: Push the branch**

```bash
git push -u origin feat/panel-width
```

- [ ] **Step 2: Create the PR**

```bash
gh pr create --title "Add --panel-width flag (#12)" --body "$(cat <<'EOF'
## Summary

Adds a \`--panel-width <px>\` clap flag (default 380, range \`200..=2000\`) so \`nwg-shell-config\` can drive the slide-out history panel width via its \`swaync-control-center-width\` preset. Mirrors the \`--popup-width\` pattern that landed in PR #17.

After this lands, all four child issues of the [nwg-shell-config integration epic (#8)](https://github.com/jasonherald/nwg-notifications/issues/8) are done — only the deferred backlog (#16) remains.

Closes #12.

## Test plan

- [x] \`make lint\` — fmt + clippy + test + deny + audit, all green locally.
- [x] Unit tests (6 new for \`--panel-width\`):
  - Default value matches \`PANEL_WIDTH_DEFAULT\` (380).
  - Mid-range \`(MIN + MAX) / 2\` parses correctly.
  - \`PANEL_WIDTH_MIN\` accepts (inclusive endpoint).
  - \`PANEL_WIDTH_MAX\` accepts (inclusive endpoint).
  - \`MIN - 1\` rejects at parse time.
  - \`MAX + 1\` rejects at parse time.
- [x] Manual smoke test against the live compositor (installed via \`make install PREFIX=\$HOME/.local BINDIR=\$HOME/.cargo/bin\`):
  - [x] Default produces the same visual as before.
  - [x] \`--panel-width 600\` produces a visibly wider panel.
  - [x] \`--panel-width 100\` and \`--panel-width 5000\` reject at parse time.

## Notes

- \`PANEL_WIDTH\` was renamed to \`PANEL_WIDTH_DEFAULT\` for clarity once user-configurable widths exist; \`PANEL_WIDTH_MIN\` and \`PANEL_WIDTH_MAX\` were added alongside as named constants (no-magic-numbers convention from \`CLAUDE.md\`).
- \`NotificationPanel::new\` takes \`panel_width: i32\` directly rather than \`&Rc<NotificationConfig>\`. The panel only consumes this one config field and adding a config dependency would be over-abstraction.
- This is the third near-identical pattern (positions, popup-width, panel-width). If a future flag needs the same treatment, the established shape is in this PR's plan.

The implementation plan (committed as \`docs/superpowers/plans/2026-04-28-panel-width.md\`) is on the branch for reviewer context.
EOF
)"
```

Expected: returns the PR URL.

- [ ] **Step 3: Hand off to CodeRabbit**

CodeRabbit reviews within minutes. **Default to fixing every finding in-PR.** Defer only when the fix needs new infrastructure — and when you do defer, open a tracking issue *first*.

Reply protocol: inline replies for in-diff comments, single PR-level comment for outside-diff items, tag `@coderabbitai` every time so it learns from the responses.

---

## Acceptance checklist (cross-reference to issue #12)

- [ ] `--panel-width <px>` flag accepts integers, defaults to current behavior. — Task 2
- [ ] Width is applied on every panel open, not just the first. — Task 3 (panel widget reads `panel_width` at construction; widget retains it for subsequent `present()` calls)
- [ ] Reasonable bounds: validated/rejected outside 200..=2000 at parse time; documented in `--help`. — Task 2 (via `value_parser!(i32).range(...)`)
- [ ] CHANGELOG entry. — Task 4
