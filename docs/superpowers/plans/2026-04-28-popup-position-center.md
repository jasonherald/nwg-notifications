# 6-Position Popup Placement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `top-center` and `bottom-center` to `--popup-position`, taking the flag from 4 corners to 6 placements, so `nwg-shell-config` can drive nwg-notifications using its existing horizontal/vertical alignment presets.

**Architecture:** Extend the `PopupPosition` enum in `src/config.rs` with two new variants. Refactor the anchor/margin logic in `src/ui/window.rs::setup_popup_window` so a small pure helper (`popup_anchors`) returns which layer-shell edges to anchor, making the placement math unit-testable. For centered placements, anchor only the top OR bottom edge — gtk4-layer-shell centers automatically on the unanchored axis. Side margins (`POPUP_SIDE_MARGIN`) are skipped on the centered axis.

**Tech Stack:** Rust, `clap` (derive ValueEnum), `gtk4-layer-shell`.

**Tracks:** Issue [#10](https://github.com/jasonherald/nwg-notifications/issues/10) (part of epic #8).

---

## File Structure

- **Modify:** `src/config.rs` — add `TopCenter`, `BottomCenter` to `PopupPosition`; add clap parse tests for both.
- **Modify:** `src/ui/window.rs` — extract `popup_anchors(PopupPosition) -> Anchors` pure helper, rewrite `setup_popup_window` to use it, handle margin behavior for centered cases.
- **Modify:** `CHANGELOG.md` — entry under `[0.3.0] — Unreleased` (or whatever the current unreleased section is — read it first).
- **Modify:** `README.md` — if it documents `--popup-position` values, list the two new ones.

No new files. The helper lives in `window.rs` next to its only caller.

---

## Pre-flight

- [ ] **Confirm working directory and branch**

Run:
```bash
cd /data/source/nwg-notifications
git status
git checkout -b feat/popup-position-center
```
Expected: clean tree on `main`, then on a fresh branch `feat/popup-position-center`. If the tree isn't clean, stop and ask the user.

- [ ] **Commit the plan file as the first commit on the branch**

Plans live alongside the work so PR readers see the thinking. Do this before touching any code so all subsequent commits build on top.

```bash
git add docs/superpowers/plans/2026-04-28-popup-position-center.md
git commit -m "docs: implementation plan for 6-position popup placement (#10)"
```

Expected: one commit on the branch, the plan file tracked.

- [ ] **Baseline build + tests pass before any change**

Run:
```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```
Expected: all green. If anything fails on `main`, stop — the plan assumes a healthy baseline.

---

## Task 1: Extend `PopupPosition` enum (TDD via clap)

**Files:**
- Modify: `src/config.rs` (the `PopupPosition` enum at lines 4-10, and add tests in the `#[cfg(test)] mod tests` block at the bottom)

- [ ] **Step 1: Write the failing tests**

Add these tests to the `#[cfg(test)] mod tests` block at the bottom of `src/config.rs`:

```rust
    #[test]
    fn popup_position_top_center() {
        let config = NotificationConfig::parse_from(["test", "--popup-position", "top-center"]);
        assert_eq!(config.popup_position, PopupPosition::TopCenter);
    }

    #[test]
    fn popup_position_bottom_center() {
        let config =
            NotificationConfig::parse_from(["test", "--popup-position", "bottom-center"]);
        assert_eq!(config.popup_position, PopupPosition::BottomCenter);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests::popup_position_top_center config::tests::popup_position_bottom_center`

Expected: compile error — `PopupPosition::TopCenter` and `PopupPosition::BottomCenter` don't exist yet. (Or, if the test binary builds, the test fails at parse with "invalid value 'top-center'".)

- [ ] **Step 3: Add the variants**

In `src/config.rs`, change the enum to:

```rust
/// Popup display position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PopupPosition {
    TopRight,
    TopCenter,
    TopLeft,
    BottomRight,
    BottomCenter,
    BottomLeft,
}
```

`clap`'s `ValueEnum` derive auto-kebab-cases — `TopCenter` becomes `top-center` in CLI args and in `--help` output. No extra `#[value(name = "...")]` attributes needed.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config::`

Expected: all 6 config tests pass (the original 4 plus the 2 new ones).

- [ ] **Step 5: Verify `--help` shows the new values**

Run: `cargo run -- --help 2>&1 | grep -A1 popup-position`

Expected: output mentions `top-right`, `top-center`, `top-left`, `bottom-right`, `bottom-center`, `bottom-left`.

- [ ] **Step 6: Compile the rest of the crate to find non-exhaustive matches**

Run: `cargo build`

Expected: compile error in `src/ui/window.rs::setup_popup_window` (`match position` is non-exhaustive — the new variants aren't handled). Possibly also in `src/ui/popup.rs::PopupManager::restack` (the `matches!` against top/left positions). This is intentional — Task 2 fixes it. **Do not commit yet.** Hold the changes in working tree until Task 2 lands the placement logic, so the branch never has a broken intermediate commit.

---

## Task 2: Extract `popup_anchors` helper + handle centered placements

**Files:**
- Modify: `src/ui/window.rs` (entire `setup_popup_window` function; add a private `Anchors` struct, a pure `popup_anchors` helper, and a `#[cfg(test)] mod tests` block at the bottom)

The current `setup_popup_window` body builds the anchor + margin logic inline with three separate `match`/`matches!` branches. We'll consolidate the anchor decision into a pure helper, leaving GTK calls in the main function.

- [ ] **Step 1: Write the failing test for the helper**

Add to the bottom of `src/ui/window.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_top_right() {
        let a = popup_anchors(PopupPosition::TopRight);
        assert_eq!((a.top, a.bottom, a.left, a.right), (true, false, false, true));
        assert!(!a.horizontally_centered);
    }

    #[test]
    fn anchors_top_left() {
        let a = popup_anchors(PopupPosition::TopLeft);
        assert_eq!((a.top, a.bottom, a.left, a.right), (true, false, true, false));
        assert!(!a.horizontally_centered);
    }

    #[test]
    fn anchors_bottom_right() {
        let a = popup_anchors(PopupPosition::BottomRight);
        assert_eq!((a.top, a.bottom, a.left, a.right), (false, true, false, true));
        assert!(!a.horizontally_centered);
    }

    #[test]
    fn anchors_bottom_left() {
        let a = popup_anchors(PopupPosition::BottomLeft);
        assert_eq!((a.top, a.bottom, a.left, a.right), (false, true, true, false));
        assert!(!a.horizontally_centered);
    }

    #[test]
    fn anchors_top_center() {
        // Centered: anchor only the top edge — gtk4-layer-shell centers the
        // surface horizontally when neither left nor right is anchored.
        let a = popup_anchors(PopupPosition::TopCenter);
        assert_eq!((a.top, a.bottom, a.left, a.right), (true, false, false, false));
        assert!(a.horizontally_centered);
    }

    #[test]
    fn anchors_bottom_center() {
        let a = popup_anchors(PopupPosition::BottomCenter);
        assert_eq!(
            (a.top, a.bottom, a.left, a.right),
            (false, true, false, false)
        );
        assert!(a.horizontally_centered);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::window::tests`

Expected: compile error — `Anchors` struct and `popup_anchors` function don't exist.

- [ ] **Step 3: Implement the helper and rewrite `setup_popup_window`**

Replace the entire contents of `src/ui/window.rs` with:

```rust
use crate::config::PopupPosition;
use gtk4_layer_shell::LayerShell;

/// Which layer-shell edges a popup anchors to, plus whether it should center
/// horizontally on the unanchored axis.
struct Anchors {
    top: bool,
    bottom: bool,
    left: bool,
    right: bool,
    /// True when neither `left` nor `right` is anchored — the layer shell
    /// centers the surface horizontally in that case. Tracked explicitly so
    /// margin logic can skip side margins on the centered axis without having
    /// to re-derive the condition.
    horizontally_centered: bool,
}

/// Pure mapping from a `PopupPosition` to layer-shell edge anchors.
fn popup_anchors(position: PopupPosition) -> Anchors {
    match position {
        PopupPosition::TopRight => Anchors {
            top: true,
            bottom: false,
            left: false,
            right: true,
            horizontally_centered: false,
        },
        PopupPosition::TopCenter => Anchors {
            top: true,
            bottom: false,
            left: false,
            right: false,
            horizontally_centered: true,
        },
        PopupPosition::TopLeft => Anchors {
            top: true,
            bottom: false,
            left: true,
            right: false,
            horizontally_centered: false,
        },
        PopupPosition::BottomRight => Anchors {
            top: false,
            bottom: true,
            left: false,
            right: true,
            horizontally_centered: false,
        },
        PopupPosition::BottomCenter => Anchors {
            top: false,
            bottom: true,
            left: false,
            right: false,
            horizontally_centered: true,
        },
        PopupPosition::BottomLeft => Anchors {
            top: false,
            bottom: true,
            left: true,
            right: false,
            horizontally_centered: false,
        },
    }
}

/// Configures a popup window with layer-shell properties.
pub fn setup_popup_window(win: &gtk4::ApplicationWindow, position: PopupPosition, top_offset: i32) {
    win.init_layer_shell();
    win.set_namespace(Some("nwg-notification-popup"));
    win.set_layer(gtk4_layer_shell::Layer::Overlay);
    win.set_exclusive_zone(-1);

    let anchors = popup_anchors(position);
    win.set_anchor(gtk4_layer_shell::Edge::Top, anchors.top);
    win.set_anchor(gtk4_layer_shell::Edge::Bottom, anchors.bottom);
    win.set_anchor(gtk4_layer_shell::Edge::Left, anchors.left);
    win.set_anchor(gtk4_layer_shell::Edge::Right, anchors.right);

    // Vertical offset for stacking — applied on whichever vertical edge
    // is anchored.
    if anchors.top {
        win.set_margin(gtk4_layer_shell::Edge::Top, top_offset);
    } else {
        win.set_margin(gtk4_layer_shell::Edge::Bottom, top_offset);
    }

    // Side margin only applies for corner placements; centered placements
    // float to monitor center and don't need a side margin.
    if !anchors.horizontally_centered {
        let side_edge = if anchors.right {
            gtk4_layer_shell::Edge::Right
        } else {
            gtk4_layer_shell::Edge::Left
        };
        win.set_margin(side_edge, super::constants::POPUP_SIDE_MARGIN);
    }

    // No keyboard interactivity — popups shouldn't steal focus.
    win.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
}

// Backdrop helpers live in `nwg_common::layer_shell`; the panel and
// DND menu re-export-by-using them with their own CSS class so the
// stylesheet for each gets the right opacity.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_top_right() {
        let a = popup_anchors(PopupPosition::TopRight);
        assert_eq!((a.top, a.bottom, a.left, a.right), (true, false, false, true));
        assert!(!a.horizontally_centered);
    }

    #[test]
    fn anchors_top_left() {
        let a = popup_anchors(PopupPosition::TopLeft);
        assert_eq!((a.top, a.bottom, a.left, a.right), (true, false, true, false));
        assert!(!a.horizontally_centered);
    }

    #[test]
    fn anchors_bottom_right() {
        let a = popup_anchors(PopupPosition::BottomRight);
        assert_eq!((a.top, a.bottom, a.left, a.right), (false, true, false, true));
        assert!(!a.horizontally_centered);
    }

    #[test]
    fn anchors_bottom_left() {
        let a = popup_anchors(PopupPosition::BottomLeft);
        assert_eq!((a.top, a.bottom, a.left, a.right), (false, true, true, false));
        assert!(!a.horizontally_centered);
    }

    #[test]
    fn anchors_top_center() {
        let a = popup_anchors(PopupPosition::TopCenter);
        assert_eq!((a.top, a.bottom, a.left, a.right), (true, false, false, false));
        assert!(a.horizontally_centered);
    }

    #[test]
    fn anchors_bottom_center() {
        let a = popup_anchors(PopupPosition::BottomCenter);
        assert_eq!(
            (a.top, a.bottom, a.left, a.right),
            (false, true, false, false)
        );
        assert!(a.horizontally_centered);
    }
}
```

- [ ] **Step 4: Run the new tests**

Run: `cargo test --lib ui::window::tests`

Expected: all 6 tests pass.

- [ ] **Step 5: Build and run the full test suite**

Run: `cargo build && cargo test`

Expected: no compile errors, all tests pass.

If `cargo build` still complains about a non-exhaustive match or stale `matches!` predicate elsewhere, find each callsite with:

```bash
grep -rn 'PopupPosition::' src/
```

The known callsite to fix is in `src/ui/popup.rs::PopupManager::restack` — it computes `is_top` via:

```rust
let is_top = matches!(
    self.config.popup_position,
    crate::config::PopupPosition::TopRight | crate::config::PopupPosition::TopLeft
);
```

That predicate decides whether the stacking offset is applied to the top or bottom edge, and `TopCenter` is also a top placement. Replace it with:

```rust
let is_top = matches!(
    self.config.popup_position,
    crate::config::PopupPosition::TopRight
        | crate::config::PopupPosition::TopCenter
        | crate::config::PopupPosition::TopLeft
);
```

For any other `match` on `PopupPosition` your grep finds, add explicit arms for `TopCenter` and `BottomCenter` that mirror the corresponding top/bottom corner behavior.

Re-run `cargo build && cargo test` and confirm green.

- [ ] **Step 6: Lint**

Run: `cargo clippy --all-targets -- -D warnings`

Expected: zero warnings. If clippy flags a redundant pattern or suggests `matches!`, take the suggestion as long as it preserves semantics.

- [ ] **Step 7: Commit Task 1 + Task 2 together**

The two tasks form one coherent change — the enum extension and its placement logic. Since Task 1 deliberately left the tree in a build-broken state, this single commit is the first green checkpoint.

```bash
git add src/config.rs src/ui/window.rs src/ui/popup.rs
git status
```

Confirm only those three files (or fewer if `popup.rs` didn't need a change) are staged.

```bash
git commit -m "$(cat <<'EOF'
Add top-center and bottom-center popup positions

Extends --popup-position from 4 corners to 6 placements (adds top-center,
bottom-center) so nwg-shell-config can drive popup placement using its
horizontal/vertical alignment presets.

For centered placements gtk4-layer-shell centers the surface horizontally
when neither Left nor Right is anchored. Side margin is skipped on the
centered axis.

Refactors setup_popup_window so the anchor decision lives in a pure
popup_anchors helper, unit-tested for all 6 variants.

Closes #10
EOF
)"
```

Expected: commit succeeds; `git status` clean.

---

## Task 3: Documentation

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `README.md` (only if it lists `--popup-position` values)

Doing docs before the install/smoke-test gate so the user has a complete change to look at when they exercise the binary.

- [ ] **Step 1: Read `CHANGELOG.md` to find the active unreleased section**

Run: `head -40 CHANGELOG.md`

Find the `## [X.Y.Z] — Unreleased` heading. Note the version number — you'll add an entry under its `### Added` subsection (creating that subsection if it doesn't exist).

- [ ] **Step 2: Add the changelog entry**

Under the active unreleased section's `### Added` block, add:

```markdown
- `--popup-position` accepts `top-center` and `bottom-center` in addition to
  the existing four corners. Centered placements anchor only the top or
  bottom edge; gtk4-layer-shell centers the surface horizontally on the
  unanchored axis. (#10)
```

If `### Added` doesn't exist in that section, create it before any other subsection (Keep-a-Changelog conventional order is Added, Changed, Deprecated, Removed, Fixed, Security).

- [ ] **Step 3: Check `README.md` for `--popup-position` documentation**

Run: `grep -n 'popup-position' README.md`

If there are matches that enumerate the accepted values, update those sites to include `top-center` and `bottom-center`. If `README.md` only mentions the flag name without listing values, leave it alone.

- [ ] **Step 4: Commit docs**

```bash
git add CHANGELOG.md README.md
git status
```

Stage only files actually modified (`README.md` may not be in this list).

```bash
git commit -m "docs: changelog + readme for top-center/bottom-center popup positions"
```

---

## Task 4: User smoke-test gate

This is a **mandatory pause point.** The unit tests in Task 2 prove the anchor decision is correct, but they don't prove the live compositor actually centers the surface and that nothing visually regressed. The user smoke-tests the binary against their own desktop session before we push the PR.

- [ ] **Step 1: Install to the user's `~/.cargo/bin`**

Run from the repo root:
```bash
make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
```

Per `CLAUDE.md`, this is the no-sudo, user-scoped install path. Do **not** run `make install-dbus` — the D-Bus service file hasn't changed, and re-running that step is unnecessary.

Expected: `make` finishes without errors; the freshly built binary is at `~/.cargo/bin/nwg-notifications`.

- [ ] **Step 2: Verify the binary is the new one**

Run: `~/.cargo/bin/nwg-notifications --help 2>&1 | grep -A1 popup-position`

Expected: the `popup-position` line lists `top-center` and `bottom-center` among the accepted values. If they aren't there, the wrong binary got installed — investigate before continuing.

- [ ] **Step 3: Stop any running daemon so the new binary takes over on next notification**

Run: `pkill -f nwg-notifications || true`

(D-Bus auto-activates the new binary on the next `notify-send`.)

- [ ] **Step 4: Hand off to the user for smoke testing — STOP HERE**

Tell the user (verbatim or close to it):
> Installed to `~/.cargo/bin/nwg-notifications`. Smoke-test at your leisure — try `notify-send` with `--popup-position top-center` and `bottom-center`, plus a corner placement to make sure nothing regressed. Reply when you're satisfied (or with anything that needs fixing) and I'll run the full cargo gambit and open the PR.

Suggested commands the user can copy-paste:
```bash
nwg-notifications --popup-position top-center &
notify-send "top-center" "centered horizontally on top edge"
sleep 3
notify-send "top-center #2" "should stack below"
# Ctrl+C the daemon, then:
nwg-notifications --popup-position bottom-center &
notify-send "bottom-center" "centered horizontally on bottom edge"
# Then a corner regression check:
nwg-notifications --popup-position bottom-left &
notify-send "bottom-left" "should still hug the bottom-left corner"
```

**Do not proceed to Task 5 until the user explicitly approves.** If the user reports issues, return to Task 2 to fix and re-install.

---

## Task 5: Full cargo gambit (CI parity)

After user smoke-test approval, run the same battery CI will run. CodeRabbit gets engaged once the PR opens, so we don't want to burn a review cycle on something the local lint catches.

- [ ] **Step 1: Run the full lint rollup**

Run from the repo root:
```bash
make lint
```

Per `CLAUDE.md`, this is `fmt + clippy + test + deny + audit` rolled into one. If `make lint` isn't a valid target here for some reason, fall back to running each piece directly:
```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check
cargo audit
```

Expected: every step exits 0. If `cargo deny` or `cargo audit` flags something pre-existing on `main`, ask the user how to handle it before proceeding — don't suppress findings unilaterally.

- [ ] **Step 2: Confirm a clean working tree**

Run: `git status`

Expected: working tree clean. If `cargo fmt --check` reformatted, run `cargo fmt --all`, commit the formatting fix as `style: cargo fmt`, and re-run `make lint`.

---

## Task 6: Open the PR

This task is gated on user smoke-test approval (Task 4) AND a clean `make lint` (Task 5). Don't push without both.

- [ ] **Step 1: Push the branch**

Run:
```bash
git push -u origin feat/popup-position-center
```

- [ ] **Step 2: Create the PR**

Run:
```bash
gh pr create --title "Add top-center and bottom-center popup positions (#10)" --body "$(cat <<'EOF'
## Summary

- Extends `--popup-position` from 4 corners to 6 placements: adds `top-center` and `bottom-center`.
- Refactors `setup_popup_window` so the anchor decision lives in a pure `popup_anchors` helper, unit-tested for all six variants.
- Updates `PopupManager::restack` to treat `top-center` as a top-anchored position for stacking-offset purposes.

Part of the [nwg-shell-config integration epic](https://github.com/jasonherald/nwg-notifications/issues/8). Closes #10.

## Test plan

- [x] `make lint` — fmt + clippy + test + deny + audit, all green locally.
- [x] 6 unit tests in `src/ui/window.rs` cover the anchor mapping for every `PopupPosition` variant.
- [x] Manual smoke test against the live compositor (installed via `make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin`):
  - [x] `--popup-position top-center` — popup centers horizontally on the top edge, stacks vertically below.
  - [x] `--popup-position bottom-center` — popup centers horizontally on the bottom edge.
  - [x] `--popup-position bottom-left` — existing corner placement still hugs the bottom-left corner (regression check).

## Notes

`gtk4-layer-shell` centers a surface horizontally when neither `Left` nor `Right` is anchored — that's how the centered placements work without any explicit `(monitor_width - popup_width) / 2` math. Side margins (`POPUP_SIDE_MARGIN`) are skipped on the centered axis so the popup doesn't get pushed off-center.
EOF
)"
```

Expected: returns the PR URL. Open it in the browser to confirm the body rendered correctly.

- [ ] **Step 3: Hand off to CodeRabbit**

CodeRabbit will start reviewing within minutes of PR creation. Wait for its first pass, then iterate on any comments — push fixup commits to the same branch (no new PR). Repeat until CodeRabbit is satisfied. Per repo workflow: one PR per issue, many commits OK, never split into a follow-up PR.

- [ ] **Step 4: Reply to every CodeRabbit finding after pushing the fix**

The point of per-discussion replies isn't just thread hygiene — it's how CodeRabbit *learns* what's signal vs. noise on this repo. Bulk replies short-circuit that. Reply to every individual finding (including pushbacks where we disagree) so future reviews get smarter.

For each round of CodeRabbit feedback:

1. Address the findings on the branch (commit + push the fixes).
2. **Inside-diff findings** (the ones anchored to specific code lines in the PR diff): reply directly to each individual review comment, tagging `@coderabbitai`. The reply explains what changed (or pushes back with reasoning if disagreeing) so CodeRabbit's resolver picks it up *and* learns from the response. The thread closes cleanly as a side effect.
3. **Outside-diff findings** (general PR-level remarks, summary nits, doc/process notes): reply via a **single PR-level comment** (not on the diff), tagging `@coderabbitai` and addressing each outside-diff item by reference.

Don't bulk-reply with one comment for everything inside-diff — CodeRabbit's per-line threads stay open *and* the bot doesn't learn from collapsed feedback.

Reply via `gh`:
```bash
# Inside-diff inline replies (one per comment ID — find IDs via `gh api repos/jasonherald/nwg-notifications/pulls/<PR#>/comments`):
gh api -X POST "repos/jasonherald/nwg-notifications/pulls/<PR#>/comments/<COMMENT_ID>/replies" \
  -f body="@coderabbitai addressed in <commit-sha>: <one-line explanation>"

# Outside-diff PR-level comment:
gh pr comment <PR#> --body "@coderabbitai responses to non-inline findings: ..."
```

Repeat the push → reply cycle until CodeRabbit gives an approving review (or the user explicitly accepts remaining findings as won't-do).

---

## Acceptance checklist (cross-reference to issue #10)

- [ ] `--popup-position` accepts `top-center` and `bottom-center` in addition to the existing four corners. — Task 1
- [ ] Popup placement logic centers the popup horizontally on the focused monitor for the new values, respecting any existing margin flags. — Task 2 (side margin skipped on centered axis)
- [ ] `clap`'s `value_enum` derive output includes the new variants in `--help`. — Task 1, Step 5
- [ ] CHANGELOG entry in the next release. — Task 3
- [ ] Unit test for the placement math. — Task 2, Step 1 (6 tests against `popup_anchors`)
