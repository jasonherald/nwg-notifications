# Actionable `--update` Error When Daemon Predates the Method Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When `nwg-notifications --update` calls a D-Bus method the running daemon doesn't have (because the daemon is from a release older than the CLI), print an actionable error that names the missing method and suggests restarting the daemon, instead of bubbling the raw `GDBus.Error:org.freedesktop.DBus.Error.UnknownMethod` to the user. Add a README paragraph reminding users to restart the daemon after upgrading the binary.

**Architecture:** Add a small pure helper `dbus::is_unknown_method_error(&glib::Error) -> bool` that wraps `glib::Error::matches::<gio::DBusError>(gio::DBusError::UnknownMethod)` for unit-testability. Use it in `src/main.rs::main`'s `--update` error handler to branch on the error class: when `UnknownMethod` fires, print a multi-line actionable message (method name, "daemon may be older than this CLI", restart recipe). For all other error classes (no-such-name, timeout, payload type), keep the current generic format unchanged.

**Tech Stack:** Rust, `gio::DBusError`, `glib::Error::matches`.

**Tracks:** Closes [#25](https://github.com/jasonherald/nwg-notifications/issues/25). Surfaced from [OG's v0.3.2 testing on #2](https://github.com/jasonherald/nwg-notifications/issues/2#issuecomment-4350866611).

---

## File Structure

- **Modify:** `src/dbus.rs` — add `pub fn is_unknown_method_error(err: &glib::Error) -> bool` next to the existing `push_*` client helpers; add a `#[cfg(test)] mod tests` block with two unit tests (positive case + negative case). The dbus.rs file already has its `unread_count_to_u32` test scaffolding from #9, so the test module pattern is established.
- **Modify:** `src/main.rs` — extend the `--update` error handler to call `dbus::is_unknown_method_error(&e)` and print the actionable multi-line message when true. Other errors keep the current single-line `Failed to update X: <err>` format.
- **Modify:** `CHANGELOG.md` — entry under `[0.3.4] — Unreleased` Fixed.
- **Modify:** `README.md` — short note added near the existing install/upgrade content explaining the restart-after-upgrade requirement, citing the same `kill $(pidof nwg-notifications)` recipe.

No new files. Helper lives in `dbus.rs` since it classifies errors that originate from D-Bus calls; same module as the `push_*` helpers that produce those errors.

---

## Pre-flight

- [ ] **Sync main and create branch**

```bash
cd /data/source/nwg-notifications
git checkout main && git pull --ff-only
git status
git checkout -b fix/actionable-update-error
```

Expected: clean tree on `main` synced to origin, then a fresh branch `fix/actionable-update-error`.

- [ ] **Commit the plan file as the first commit**

```bash
git add docs/superpowers/plans/2026-05-03-actionable-update-error.md
git commit -m "docs: implementation plan for actionable --update error (#25)"
```

- [ ] **Baseline full cargo gambit**

```bash
make lint
```

Expected: every step exits 0; pre-existing `cargo deny` "unmatched skip" warnings are non-blocking.

---

## Task 1: Add `is_unknown_method_error` helper with TDD

**Files:**
- Modify: `src/dbus.rs` — new `pub fn` + tests in the existing `#[cfg(test)] mod tests` block.

- [ ] **Step 1: Write failing tests**

In `src/dbus.rs`, find the existing `#[cfg(test)] mod tests { ... }` block at the bottom. Append these tests:

```rust
    #[test]
    fn is_unknown_method_error_recognises_dbus_unknown_method() {
        let err = glib::Error::new(gio::DBusError::UnknownMethod, "method missing");
        assert!(is_unknown_method_error(&err));
    }

    #[test]
    fn is_unknown_method_error_rejects_other_dbus_errors() {
        let err = glib::Error::new(gio::DBusError::NoMemory, "out of memory");
        assert!(!is_unknown_method_error(&err));
    }
```

- [ ] **Step 2: Run tests to verify failure**

```bash
cargo test is_unknown_method_error 2>&1 | tail -10
```

Expected: compile error — `is_unknown_method_error` doesn't exist yet.

- [ ] **Step 3: Add the helper**

Add this function to `src/dbus.rs`. Place it near the existing client helpers (after the `push_max_history` definition, before the `emit_count_changed` signal helper):

```rust
/// Returns true if the given `glib::Error` is the standard D-Bus
/// `org.freedesktop.DBus.Error.UnknownMethod` error class. Used by the
/// `--update` CLI to give an actionable message when the running daemon
/// is from a release older than the CLI and doesn't recognise a method
/// the CLI is trying to call (#25).
pub fn is_unknown_method_error(err: &glib::Error) -> bool {
    err.matches(gio::DBusError::UnknownMethod)
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test is_unknown_method_error 2>&1 | tail -10
```

Expected: both tests pass.

- [ ] **Step 5: Build, full test, clippy**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean. Test count grows by 2 (was 63, now 65).

If clippy complains about `dead_code` for the new `pub fn` (because the bin crate has no consumer yet), the next task wires it in — leave the commit until Task 2 lands so the branch never has a clippy-broken intermediate. Mirror the pattern used in #20's PopupManager::dismiss_all_popups commit.

- [ ] **Step 6: Hold the commit**

Don't commit yet — wait for Task 2 to land the consumer. The combined commit from Task 2 makes the branch always-buildable under `-D warnings`.

---

## Task 2: Wire the actionable message in `--update`'s error handler

**Files:**
- Modify: `src/main.rs` — update the `--update` error path to branch on `is_unknown_method_error`.

- [ ] **Step 1: Update the error handler**

In `src/main.rs::main`, the current `--update` error path is:

```rust
            if let Err(e) = push_result {
                eprintln!("Failed to update {}: {}", name, e);
                had_error = true;
            } else {
                println!("Updated {}", name);
            }
```

Replace it with:

```rust
            if let Err(e) = push_result {
                if dbus::is_unknown_method_error(&e) {
                    eprintln!(
                        "Failed to update {name}: the running daemon doesn't recognise this D-Bus method.\n\
                         This usually means the daemon is from a release older than this CLI.\n\
                         Restart it to pick up the new methods, e.g.:\n  \
                         kill $(pidof nwg-notifications)\n\
                         and let your session manager respawn it (or just run `nwg-notifications --persist &` yourself).\n\
                         Underlying error: {e}"
                    );
                } else {
                    eprintln!("Failed to update {name}: {e}");
                }
                had_error = true;
            } else {
                println!("Updated {name}");
            }
```

The actionable branch covers UnknownMethod specifically. All other error classes (no daemon owning the name, timeout, payload-type errors) keep their existing single-line format — those aren't usually about version mismatch and the restart hint would be misleading.

- [ ] **Step 2: Build, full test, clippy**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean. The 65 tests still pass.

- [ ] **Step 3: Commit Tasks 1 + 2 together**

The two tasks form one coherent change — the helper and its consumer. Single commit keeps the branch always-buildable.

```bash
git add src/dbus.rs src/main.rs
git commit -m "$(cat <<'EOF'
Actionable error when --update hits UnknownMethod (#25)

When --update calls a D-Bus method the running daemon doesn't have
(typically because the daemon is from a release older than the CLI),
the previous error path bubbled the raw GDBus.Error...UnknownMethod
text — technically correct but not actionable. A user couldn't tell
they needed to restart the daemon.

Add a small pure helper dbus::is_unknown_method_error that wraps
glib::Error::matches(gio::DBusError::UnknownMethod). Use it in main()'s
--update error handler to print a multi-line actionable message naming
the flag, explaining the likely cause (CLI is newer than daemon), and
suggesting the kill+respawn recipe. Other error classes (no-such-name,
timeout, payload type) keep the existing single-line format — the
restart hint would mislead in those cases.

2 new unit tests cover the predicate (positive case for UnknownMethod,
negative case for a different DBusError). Test count: 63 -> 65.

Closes #25.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Documentation

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `README.md` — add a paragraph near the install/upgrade content.

- [ ] **Step 1: CHANGELOG entry**

The `CHANGELOG.md` currently has `## [0.3.3] — 2026-04-29` at the top of the version sections (after the pre-split note block). Add a new `## [0.3.4] — Unreleased` block above it:

```markdown
## [0.3.4] — Unreleased

### Fixed

- `--update` now prints an actionable error when it calls a D-Bus
  method the running daemon doesn't recognise — typically because the
  daemon is from a release older than the CLI. Previously the raw
  `GDBus.Error:org.freedesktop.DBus.Error.UnknownMethod` text bubbled
  to the user, which didn't hint at the restart-after-upgrade fix.
  Other error classes (no daemon, timeout, payload type) keep their
  existing format. (#25)
```

(Place this above the `## [0.3.3] — 2026-04-29` section.)

- [ ] **Step 2: README addition**

Find the "From crates.io" install subsection in `README.md`. After its existing paragraph (the "Lands the binary at `~/.cargo/bin/nwg-notifications` ..." block), append a new paragraph:

````markdown
**After upgrading**, restart any long-running daemon process so it picks up new D-Bus surface introduced by the upgrade. The CLI on `PATH` will be the new binary immediately, but the daemon process started by your session manager (or auto-activated by D-Bus before the upgrade) keeps running the old code until it exits. Quickest restart:

```bash
kill $(pidof nwg-notifications)
# Your session manager (or D-Bus auto-activation on the next notify-send)
# spawns the new binary. Or run `nwg-notifications --persist &` directly.
```

Without this, `--update` and `gdbus call` against newly-shipped methods fail with `org.freedesktop.DBus.Error.UnknownMethod`.
````

(Same restart recipe applies to anyone using `make install` — the principle is "after replacing the binary, restart the daemon process." The README's `make install` subsections don't need a separate copy of this note since the principle is the same.)

- [ ] **Step 3: Build, test, clippy sanity**

```bash
cargo build && cargo test && cargo clippy --all-targets -- -D warnings
```

Expected: clean (CHANGELOG + README don't compile, but running these confirms nothing else slipped).

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md README.md
git commit -m "$(cat <<'EOF'
docs: actionable --update error + restart-after-upgrade note (#25)

CHANGELOG entry under new [0.3.4] — Unreleased Fixed section.

README "From crates.io" subsection grows a paragraph explaining that
after upgrading the binary, any long-running daemon process needs to
restart so it picks up new D-Bus surface — otherwise --update and
gdbus call against newly-shipped methods hit UnknownMethod. Cites
the same kill-and-respawn recipe the new --update error message uses.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: User smoke-test gate (HARD STOP)

The unit tests cover the predicate; manual verification confirms the actual error text in both branches against a real daemon.

- [ ] **Step 1: Install to the user's `~/.cargo/bin`**

```bash
make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
```

- [ ] **Step 2: Restart the user's session daemon so it has the new binary**

```bash
kill $(pidof nwg-notifications) 2>/dev/null || true
sleep 0.5
uwsm-app -- nwg-notifications --persist >/dev/null 2>&1 &
disown
sleep 1
pidof nwg-notifications && echo "(daemon up)"
```

- [ ] **Step 3: Hand off to the user — STOP HERE**

Tell the user (verbatim or close):
> Installed and restarted the daemon. Smoke-test paths for #25:
>
> 1. **Happy path still works (regression check):**
>    - `nwg-notifications --update --popup-position top-center` → expect `Updated popup_position`, exit 0.
>    - `notify-send "test" "msg"` → popup at top-center.
>
> 2. **Trigger the new actionable error path.** Easiest reliable way is to call a method that doesn't exist via `gdbus`:
>    - `gdbus call --session --dest org.nwg.Notifications --object-path /org/nwg/Notifications --method org.nwg.Notifications.SetNonexistentMethod 42` → expect raw `UnknownMethod` from gdbus (this confirms the daemon's dispatcher returns it; we already shipped that).
>
>    To exercise the *CLI* error path against the same condition, we'd need an old-binary daemon — most reliable repro is to ask OG to test against his old daemon, or to temporarily run the previous-release binary as the daemon while invoking the new `--update`. Skip if you'd rather just trust the unit tests for the predicate; the message wording is straightforward to eyeball in `src/main.rs`.
>
> 3. **Other error classes still use the generic format (no actionable hint):**
>    - `kill $(pidof nwg-notifications) 2>/dev/null` → daemon stops.
>    - `nwg-notifications --update --popup-position top-center` → expect `Failed to update popup_position: <NoSuchName error>` *without* the multi-line restart hint (because this is a no-daemon error, not UnknownMethod).
>    - Restart back to default: `uwsm-app -- nwg-notifications --persist >/dev/null 2>&1 & disown`
>
> 4. **README reads cleanly:**
>    - `cat README.md | grep -A12 "After upgrading"` → confirm the new paragraph reads well in the install context.
>
> Reply when satisfied or with anything that needs fixing.

**Do not proceed to Task 5 until the user explicitly approves.** If they report issues, return to the broken task.

---

## Task 5: Full cargo gambit (CI parity)

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

## Task 6: Open the PR

Gated on Task 4 (user smoke-test approval) AND Task 5 (clean `make lint`).

- [ ] **Step 1: Push the branch**

```bash
git push -u origin fix/actionable-update-error
```

- [ ] **Step 2: Create the PR**

```bash
gh pr create --title "Actionable --update error when daemon predates the called method (#25)" --body "$(cat <<'EOF'
## Summary

Surfaced from [OG's v0.3.2 testing on #2](https://github.com/jasonherald/nwg-notifications/issues/2#issuecomment-4350866611): when his CLI was on ≥0.3.2 (with \`--update\`) but his running daemon was still on an older release, \`nwg-notifications --update --popup-position top-center\` failed with the raw \`GDBus.Error:org.freedesktop.DBus.Error.UnknownMethod\` text. Technically correct, but not actionable — a user reading "method missing" can't tell they need to restart the daemon.

This PR has two pieces:

1. **CLI error message.** \`--update\` now detects \`UnknownMethod\` errors specifically and prints a multi-line actionable message naming the flag, explaining the likely cause, and suggesting the \`kill $(pidof nwg-notifications)\` recipe. Other error classes (no daemon at all, timeouts, payload-type mismatches) keep their existing single-line format — the restart hint would mislead in those cases.
2. **README addition.** New paragraph under the "From crates.io" install subsection explaining that after upgrading the binary, any long-running daemon process needs to restart so it picks up new D-Bus surface.

The same restart-after-upgrade principle applies to anyone using \`make install\` too; the README's principle paragraph is single-source under the crates.io subsection rather than copy-pasted across each install variant.

Closes #25.

## Test plan

- [x] \`make lint\` — fmt + clippy + test + deny + audit, all green locally.
- [x] 2 new unit tests for \`dbus::is_unknown_method_error\` (positive case for \`UnknownMethod\`, negative case for a different \`DBusError\` variant). Test count: 63 → 65.
- [x] Manual smoke test against the live compositor:
  - [x] Happy path: \`--update --popup-position top-center\` against a current-version daemon still works (\`Updated popup_position\`, exit 0).
  - [x] Other error classes: with no daemon running, \`--update\` shows the *existing* generic single-line format (no spurious restart hint).
  - [x] README's new paragraph reads cleanly in the install section.
  - [x] The new actionable-message branch is hard to repro against a current-version daemon end-to-end (you'd need to run a deliberately-old daemon while invoking the new CLI). Predicate is unit-tested; the message wording is small enough to eyeball.

## Notes

- A richer error-classification helper (e.g. an enum covering \`NoSuchName\`, \`Timeout\`, \`UnknownMethod\`, etc.) was considered and skipped — YAGNI for this PR. If we end up needing per-class messages for more failure modes later, expand then.
- D-Bus integration tests (#16) would let us exercise the actionable-message branch end-to-end. That work is parked pending the broader nwg-* integration-testing infrastructure initiative.

The implementation plan (committed as \`docs/superpowers/plans/2026-05-03-actionable-update-error.md\`) is on the branch for reviewer context.
EOF
)"
```

Expected: returns the PR URL.

- [ ] **Step 3: Hand off to CodeRabbit**

CodeRabbit reviews within minutes. **Default to fixing every finding in-PR.** Defer only when the fix needs new infrastructure or has wide blast radius — and when you do defer, open a tracking issue *first*. Reply protocol: inline replies for in-diff comments, single PR-level comment for outside-diff items, tag `@coderabbitai` every time so it learns from the responses.

---

## Acceptance checklist (cross-reference to issue #25)

- [ ] `--update` failures whose underlying `glib::Error` is `UnknownMethod` print an actionable message including the suggested restart command. — Task 2 (with helper from Task 1)
- [ ] Other failure classes (no daemon owning the name, timeout, payload-type errors) still surface their current generic message. — Task 2 (else-branch unchanged from existing `eprintln!`)
- [ ] README has a paragraph under the install/upgrade section about restart-after-upgrade. — Task 3
- [ ] Unit test for the new error-classification helper. — Task 1 (2 tests)
- [ ] CHANGELOG entry under unreleased Fixed. — Task 3
