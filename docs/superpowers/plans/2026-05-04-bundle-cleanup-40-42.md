# Bundle Cleanup (#42, #40) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bundle two micro-cleanup items from epic #29 — standardize on inlined-format-args (`{var}`) for every `log::*` / `format!` / `eprintln!` / `println!` call across the crate (#42) plus add a one-line rationale comment at each `Unknown D-Bus method` site, and replace the remaining non-trivial `as` casts with `try_from` / `From` (#40).

**Architecture:** Both items are mechanical sweeps with no behavior change. #42 uses `cargo clippy --fix -- -W clippy::uninlined_format_args` to do the format-args rewrite (15 sites flagged today), then a manual pass for the rationale comment. #40 walks the inventoried list of `as` cast sites and applies one of three replacements (`u32::try_from(...).expect(...)`, `u64::from(...)`, `usize::try_from(...).expect(...)`) per site — pure layout-math casts whose bounds are obvious from a small constant (e.g. `popups.len() as i32` where `popups.len() <= max_popups`) get the `try_from(...).expect(...)` treatment so the safety case is documented inline.

**Tech Stack:** Rust 2024 edition, `cargo clippy --fix` for the mechanical format-arg sweep, `try_from` + `expect` for documenting the safety case on previously-`as` cast sites.

**Tracks:** Closes #42, #40. Both are children of epic #29 (the post-v0.3.4 polish pass).

---

## File Structure

| Task | Files modified | Test approach |
|------|----------------|---------------|
| #42 log/format style + UnknownMethod rationale comment | `src/dbus.rs`, `src/persistence.rs`, `src/ui/dnd_menu.rs`, `src/ui/panel_content.rs`, `src/waybar.rs` (15 sites flagged by `clippy::uninlined_format_args`); `src/dbus.rs` again for the warn-vs-error rationale comment near both `Unknown D-Bus method` arms | `cargo clippy -- -D warnings` clean; `cargo test` still 91 green; `cargo clippy -- -W clippy::uninlined_format_args` empty |
| #40 `as` → `try_from` / `From` | `src/main.rs` (5 sites in `--update` push dispatch), `src/ui/panel.rs` (1 site in `hide_panel` timeout), `src/ui/popup.rs` (4 sites: 2 layout-math, 1 timeout, 1 monitor index), `src/dbus.rs` (2 sites in `handle_set_max_*` lambdas) | `cargo build --release` + `cargo test` still green; clippy clean |

Each issue gets its own commit. No CHANGELOG entry — pure internal cleanup with zero user-visible impact.

---

## Pre-flight

- [ ] **Sync main and create branch**

```bash
cd /data/source/nwg-notifications
git checkout main && git pull --ff-only
git status
git checkout -b chore/cleanup-40-42
```

Expected: clean tree on `main`, then a fresh branch.

- [ ] **Commit the plan file as the first commit on the branch**

```bash
git add docs/superpowers/plans/2026-05-04-bundle-cleanup-40-42.md
git commit -m "docs: implementation plan for cleanup bundle (#40 #42)"
```

- [ ] **Baseline full cargo gambit**

```bash
make lint
```

Expected: every step exits 0; pre-existing `cargo deny` "unmatched skip" warnings are non-blocking. Test count is 91 going in.

---

## Task 1: #42 — Standardize on inlined-format-args + add rationale comment

15 `log::*` / `format!` / `eprintln!` calls use the legacy positional `{}` form (`log::warn!("Unknown method: {}", method)`); only one in the crate uses the interpolated form (`log::debug!("Failed to signal waybar: {e}")`). The interpolated form is what `clippy::uninlined_format_args` recommends, what `cargo clippy --fix` will rewrite to, and what the rest of the rust ecosystem is converging on. Standardize on it everywhere.

**Files:**
- Modify (mechanical): `src/dbus.rs`, `src/persistence.rs`, `src/ui/dnd_menu.rs`, `src/ui/panel_content.rs`, `src/waybar.rs` (the 15 sites `clippy::uninlined_format_args` currently flags).
- Modify (manual): `src/dbus.rs` — one rationale comment near each `_ => log::warn!("Unknown ... D-Bus method: ...")` arm explaining the daemon-side `warn` vs CLI-side `error` split.

- [ ] **Step 1: Apply the mechanical format-args sweep**

```bash
cargo clippy --fix --all-targets --allow-staged -- -W clippy::uninlined_format_args
```

Expected: clippy auto-applies the suggestion to all 15 sites. The `--allow-staged` flag is safe here because the only changes in the working tree are this plan file's commit (already staged + committed in pre-flight).

- [ ] **Step 2: Verify the sweep landed cleanly + lint stays green going forward**

```bash
cargo clippy --all-targets -- -D warnings -W clippy::uninlined_format_args 2>&1 | tail -10
cargo test
cargo fmt --all -- --check
```

Expected: `clippy` reports zero `uninlined_format_args` warnings; full 91-test suite still green; no fmt drift.

- [ ] **Step 3: Add the rationale comment near the `Unknown D-Bus method` arms in `dbus.rs`**

Two `_ => log::warn!(...)` fallthrough arms exist — one in `handle_method` (org.freedesktop.Notifications dispatch) and one in `handle_nwg_count_method` (org.nwg.Notifications dispatch). Both currently look like:

```rust
        _ => {
            log::warn!("Unknown D-Bus method: {method}");
            invocation.return_dbus_error(
                "org.freedesktop.DBus.Error.UnknownMethod",
                &format!("Unknown method: {method}"),
            );
        }
```

(Or for `handle_nwg_count_method`, the message is `"Unknown nwg-count D-Bus method: {method}"`.)

Insert a comment **above the first** of the two `_ =>` arms (the one in `handle_method`) explaining the warn-vs-error policy:

```rust
        // Daemon-side unknown-method dispatch is `warn`, not `error`:
        // the freedesktop `Notify` D-Bus surface is open enough that a
        // misbehaving client (or a forward-compat probe) calling a
        // method we don't implement is something to log but not page on.
        // The mirror site on the *client* side — `--update` against a
        // stale daemon in `main.rs`'s `is_unknown_method_error` branch —
        // is `error` because there it's an actionable failure for the
        // human running the CLI ("restart the daemon"). Same wire-level
        // condition, different side, different severity.
        _ => {
            log::warn!("Unknown D-Bus method: {method}");
```

The second `_ =>` arm in `handle_nwg_count_method` has the same shape and policy — leave it without a duplicate comment. The rationale lives at the `_ =>` arm in `handle_method` (the freedesktop dispatcher) just upthread; both are in `src/dbus.rs`, so a reader who sees the second arm without a comment can find the first one with a quick `grep` for `Unknown D-Bus method`.

- [ ] **Step 4: Build, test, clippy, fmt**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: clean build; 91 tests still green; clippy clean; no fmt drift.

- [ ] **Step 5: Commit**

```bash
git add src/
git commit -m "$(cat <<'EOF'
Standardize on inlined-format-args + warn-vs-error rationale (#42)

Two micro-inconsistencies the comprehensive code-quality review
called out:

1. Format style. 38 of 39 log/format/eprintln/println calls used
   the legacy positional `{}` form; only one (in waybar.rs)
   used the interpolated `{var}` form. clippy::uninlined_format_args
   prefers interpolated, the rest of the rust ecosystem is
   converging on it, and rustfmt will eventually default to
   pushing that direction. Picked interpolated; ran
   `cargo clippy --fix --all-targets -- -W clippy::uninlined_format_args`
   to do the 15-site sweep mechanically. After the sweep,
   re-running the lint reports zero warnings.

2. Daemon-side `Unknown D-Bus method: ...` is logged as `warn`
   (in dbus.rs) but the equivalent client-side path through
   --update against a stale daemon (in main.rs's
   is_unknown_method_error branch) is logged as `error`. Both
   are correct individually — the daemon side is a misbehaving
   or forward-compat client we shouldn't page on, the CLI side
   is an actionable user-facing failure ("restart the daemon").
   But reading both at once is jarring without context. Added
   a comment above the `_ =>` arm in handle_method explaining
   the split. The sibling `_ =>` arm in handle_nwg_count_method
   shares the policy and lives in the same file (src/dbus.rs);
   one comment covers both.

No behavioral change. Test count unchanged at 91; clippy clean
even with the stricter -W clippy::uninlined_format_args lint
turned on.

Closes #42.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: #40 — Replace remaining `as` casts with `try_from` / `From`

The codebase already established a precedent for documenting fallible casts: `unread_count_to_u32` in `dbus.rs` uses `try_from` rigorously. Apply that posture to the remaining non-trivial sites.

**Files:**
- Modify: `src/main.rs` — 5 sites in the `--update` push dispatch.
- Modify: `src/ui/panel.rs` — 1 site in `hide_panel`.
- Modify: `src/ui/popup.rs` — 4 sites in `restack` / `calculate_offset` / `resolve_timeout` / `focused_gdk_monitor`.
- Modify: `src/dbus.rs` — 2 sites in the `handle_set_max_popups` and `handle_set_max_history` lambdas.

**Investigation (already done; baked into this task):**
- The `--update` push sites in `main.rs` cast i32 / u64 / usize to u32. All values are validated by clap's range parser at parse time, so a successful CLI invocation has already checked the range. `try_from(...).expect("clap-validated")` documents the case.
- `PANEL_REVEAL_DURATION_MS` is `u32`; the panel timeout takes `u64`. Lossless — `From<u32> for u64` exists, so use `u64::from(...)`.
- The two `raw as usize` sites in `dbus.rs` are inside `handle_set_u32` lambdas where `raw: u32`. `From<u32> for usize` is **not** in std (usize might be 16-bit on rare targets), so use `usize::try_from(raw).expect(...)`.
- `notif.timeout_ms as u64` in `popup.rs::resolve_timeout` is guarded by `if notif.timeout_ms > 0` immediately above, so the cast is safe — `u64::try_from(notif.timeout_ms).expect("> 0 guard above")` documents that.
- The two layout-math casts in `popup.rs::restack` / `calculate_offset` (`i as i32`, `popups.len() as i32`) are bounded by `max_popups` which is small (default 5, max-validated). `i32::try_from(...).expect("bounded by max_popups")` is the same shape as the others and documents the bound.
- `focused_gdk_monitor`'s `focused_idx as u32` cast (usize from `position()` → u32 for `monitors.item(...)`) is bounded by the small list of monitors (typically 1-4). `u32::try_from(...).expect("monitor index fits in u32")`.

- [ ] **Step 1: Replace the 5 `as u32` casts in `src/main.rs`'s `--update` push dispatch**

In `src/main.rs`, find the `match name` block inside the `--update` arm. Currently:

```rust
                "popup_width" => dbus::push_popup_width(config.popup_width as u32),
                "panel_width" => dbus::push_panel_width(config.panel_width as u32),
                "popup_timeout" => dbus::push_popup_timeout(config.popup_timeout as u32),
                "max_popups" => dbus::push_max_popups(config.max_popups as u32),
                "max_history" => dbus::push_max_history(config.max_history as u32),
```

Replace with:

```rust
                "popup_width" => dbus::push_popup_width(
                    u32::try_from(config.popup_width)
                        .expect("popup-width validated by clap range parser"),
                ),
                "panel_width" => dbus::push_panel_width(
                    u32::try_from(config.panel_width)
                        .expect("panel-width validated by clap range parser"),
                ),
                "popup_timeout" => dbus::push_popup_timeout(
                    u32::try_from(config.popup_timeout)
                        .expect("popup-timeout validated by clap range parser"),
                ),
                "max_popups" => dbus::push_max_popups(
                    u32::try_from(config.max_popups)
                        .expect("max-popups validated by clap range parser"),
                ),
                "max_history" => dbus::push_max_history(
                    u32::try_from(config.max_history)
                        .expect("max-history validated by clap range parser"),
                ),
```

- [ ] **Step 2: Replace `PANEL_REVEAL_DURATION_MS as u64` in `src/ui/panel.rs`**

In `src/ui/panel.rs`, find the line:

```rust
        std::time::Duration::from_millis(PANEL_REVEAL_DURATION_MS as u64),
```

Replace with:

```rust
        std::time::Duration::from_millis(u64::from(PANEL_REVEAL_DURATION_MS)),
```

(`u32 → u64` is lossless on every target Rust supports, so `From` is the right trait.)

- [ ] **Step 3: Replace the 4 `as` casts in `src/ui/popup.rs`**

In `src/ui/popup.rs`, find:

```rust
            let offset = POPUP_TOP_MARGIN + (i as i32) * (self.estimated_height() + POPUP_GAP);
```

Replace with:

```rust
            let offset = POPUP_TOP_MARGIN
                + i32::try_from(i).expect("popup index bounded by max_popups")
                    * (self.estimated_height() + POPUP_GAP);
```

Then find:

```rust
        POPUP_TOP_MARGIN + (self.popups.len() as i32) * (self.estimated_height() + POPUP_GAP)
```

Replace with:

```rust
        POPUP_TOP_MARGIN
            + i32::try_from(self.popups.len()).expect("popup count bounded by max_popups")
                * (self.estimated_height() + POPUP_GAP)
```

Then find (inside `resolve_timeout`):

```rust
        if notif.timeout_ms > 0 {
            notif.timeout_ms as u64
```

Replace with:

```rust
        if notif.timeout_ms > 0 {
            u64::try_from(notif.timeout_ms).expect("> 0 guard above ensures non-negative")
```

Then find (inside `focused_gdk_monitor`):

```rust
    let item = monitors.item(focused_idx as u32)?;
```

Replace with:

```rust
    let item = monitors.item(
        u32::try_from(focused_idx).expect("monitor index fits in u32"),
    )?;
```

- [ ] **Step 4: Replace the 2 `raw as usize` casts in `src/dbus.rs`**

In `src/dbus.rs`, inside the `handle_nwg_count_method` dispatch arm for `SetMaxPopups`, find:

```rust
                cfg.max_popups = raw as usize;
```

Replace with:

```rust
                cfg.max_popups = usize::try_from(raw)
                    .expect("u32 fits in usize on every supported target");
```

Then in the `SetMaxHistory` arm, find:

```rust
                cfg.max_history = raw as usize;
```

Replace with:

```rust
                cfg.max_history = usize::try_from(raw)
                    .expect("u32 fits in usize on every supported target");
```

- [ ] **Step 5: Build, test, clippy, fmt**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: clean build; 91 tests still green; clippy clean; no fmt drift.

If `cargo fmt --check` reports drift (likely on `main.rs` after the multi-line replacement), run `cargo fmt --all` and re-verify.

- [ ] **Step 6: Confirm no remaining non-trivial `as` casts in the touched files**

```bash
grep -rn " as u32\b\| as i32\b\| as u64\b\| as usize\b" src/main.rs src/ui/popup.rs src/ui/panel.rs src/dbus.rs | grep -v tests
```

Expected: no matches outside test code. (Test code has 2 `as usize` casts in `dbus.rs`'s `unread_count_to_u32_*` tests for `u32::MAX as usize` arithmetic — those are the trivial/obvious cases and stay.)

- [ ] **Step 7: Commit**

```bash
git add src/
git commit -m "$(cat <<'EOF'
Replace as casts with try_from / From at the non-trivial sites (#40)

Codebase already had unread_count_to_u32 in dbus.rs as the
precedent for "document the fallible cast with try_from". Applied
that posture across the remaining non-trivial as cast sites,
12 in total:

- main.rs --update push dispatch (5 sites): config.popup_width as
  u32 etc. -> u32::try_from(...).expect("validated by clap range
  parser"). Each setter has its own message naming the validated
  CLI flag.
- ui/panel.rs hide_panel timeout (1 site): PANEL_REVEAL_DURATION_MS
  as u64 -> u64::from(...) since u32 -> u64 is lossless and
  From<u32> for u64 exists.
- ui/popup.rs (4 sites): restack's `i as i32` and calculate_offset's
  `popups.len() as i32` get i32::try_from(...).expect("bounded by
  max_popups"). resolve_timeout's `notif.timeout_ms as u64` (guarded
  by > 0 check immediately above) gets u64::try_from(...).expect("...
  guard above"). focused_gdk_monitor's `focused_idx as u32` gets
  u32::try_from(...).expect("monitor index fits in u32").
- dbus.rs handle_set_u32 lambdas (2 sites): raw as usize for
  max_popups / max_history -> usize::try_from(raw).expect(...).
  From<u32> for usize is intentionally NOT in std (usize might be
  16-bit on rare embedded targets), so try_from is the idiomatic
  choice even though u32 -> usize is lossless on every supported
  target.

The 2 remaining `as usize` casts in dbus.rs are inside the
unread_count_to_u32_* test module and operate on u32::MAX literals
for arithmetic — those are obvious-from-context and stay.

No behavioral change: every cast that used to silently truncate
or saturate either had a guarantee that the cast was lossless
(now documented via From or expect) or had the validation
upstream (now documented via expect message).

Closes #40.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Pre-PR gates + smoke install

Per repo convention. This bundle is internal cleanup (mechanical sweep + cast hygiene) with zero behavior change, so the smoke is symbolic.

- [ ] **Step 1: Install to user bin and confirm restart works**

```bash
make upgrade PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
```

Expected: build → install → kill running daemon → respawn. `pidof nwg-notifications` returns the new PID.

- [ ] **Step 2: Hand off to the user — STOP HERE**

Tell the user (verbatim or close):

> Installed and restarted via `make upgrade`. This bundle is
> mechanical cleanup — log-format-args sweep + `as` → `try_from`
> conversions. Smoke is short:
>
> 1. `notify-send "smoke" "test"` — popup appears.
> 2. `nwg-notifications --update --max-history 100` — should
>    print `Updated max_history` (verifies the `try_from`
>    replacements in main.rs and dbus.rs didn't break the
>    --update path).
> 3. `nwg-notifications --update --popup-width 50` — should
>    error with `popup-width 50 is not in 100..=2000`
>    (verifies clap's range parser still rejects pre-D-Bus,
>    so the .expect() in main.rs never fires).
>
> Reply when satisfied.

**Do not proceed to Task 4 until the user explicitly approves.**

- [ ] **Step 3: Full lint after smoke approval**

```bash
make lint
```

Expected: every step exits 0; total test count is 91.

- [ ] **Step 4: Push**

```bash
git push -u origin chore/cleanup-40-42
```

---

## Task 4: Open PR

- [ ] **Step 1: Open the PR**

```bash
gh pr create --base main --head chore/cleanup-40-42 \
  --title "Bundle cleanup: #40 #42" \
  --body "$(cat <<'EOF'
## Summary

Bundles two micro-cleanup items from epic #29 into one PR — closes the polish-pass slate down to just **#34** (the breaking-internal `mac-notifications-*` rename, queued as the final standalone PR + v0.4.0 release).

- **#42** — Standardized on inlined-format-args (`{var}`) for every `log::*` / `format!` / `eprintln!` / `println!` call. 38 of 39 sites used the legacy positional `{}` form; one in `waybar.rs` used the interpolated form. Ran `cargo clippy --fix --all-targets -- -W clippy::uninlined_format_args` to do the 15-site sweep mechanically. Added a one-paragraph comment above the first `_ => log::warn!("Unknown D-Bus method: {method}")` arm in `handle_method` explaining the daemon-side `warn` vs CLI-side `error` policy split (same wire-level condition, different side, different severity — daemon is logging a misbehaving client, CLI is reporting an actionable user-facing failure).
- **#40** — Replaced 12 `as` casts with `try_from` / `From` at the non-trivial sites: 5 in `main.rs`'s `--update` push dispatch (each `expect`'d as "validated by clap range parser"), 1 in `ui/panel.rs::hide_panel` (lossless `u32→u64` via `u64::from`), 4 in `ui/popup.rs` (layout-math + monitor-index, each `expect`'d with the bound), 2 in `dbus.rs::handle_nwg_count_method`'s `SetMax*` lambdas (`From<u32> for usize` isn't in std because of 16-bit targets, so `try_from`). The 2 `as usize` casts in the `unread_count_to_u32_*` test module operate on `u32::MAX` literals and stay.

One commit per issue. No CHANGELOG entry — internal cleanup with zero user-visible impact.

## Test plan

- [x] `make lint` clean locally (fmt + clippy + test + deny + audit). Test count unchanged at 91.
- [x] `cargo clippy -- -W clippy::uninlined_format_args` reports zero warnings (was 15 before #42's sweep).
- [x] Manual smoke test against the live compositor (installed via `make upgrade PREFIX=\$HOME/.local BINDIR=\$HOME/.cargo/bin`):
  - [x] `notify-send` produces a popup.
  - [x] `--update --max-history N` works (verifies the try_from replacements in main.rs + dbus.rs).
  - [x] `--update --popup-width 50` rejects with the range-parser error (so the `.expect()` in main.rs never fires in practice).

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 2: Wait for CodeRabbit + iterate**

Default-fix posture per repo convention. Inline reply per in-diff comment, single PR-level reply for outside-diff items, tag `@coderabbitai` every time. Do not respond to non-bot commenters under the maintainer's account.

---

## Notes

- **No CHANGELOG entry.** Pure internal cleanup, zero user-visible impact — same posture as PRs #54, #55, #56, #57, #58.
- **Why `try_from(...).expect(...)` everywhere instead of mixing `From` and `try_from`?** `From<u32> for u64` is the only `From` impl in this list — every other cast either crosses signedness (i32 ↔ u32, i32 → u64), narrows (u64 → u32), or has a target-dependent width (usize). For the one lossless conversion (`u64::from(PANEL_REVEAL_DURATION_MS)`), `From` reads cleaner than `try_from(...).unwrap()`. For all the others, `try_from` + `expect` documents the safety case better than `as`.
- **Why log::warn for daemon-side unknown method but log::error for CLI-side?** The daemon's freedesktop D-Bus interface is open enough that any client can call any method name; an unrecognized call is a misbehaving or forward-compat client, not a daemon bug. Logging at `error` would page operators on every client mistake. The CLI's `--update` against a stale daemon, by contrast, is an actionable failure for the human running the command — `error` plus the "restart the daemon" hint is the right severity.
