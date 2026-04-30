# Panel-Open Dismisses Visible Popups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the notification panel is opened (waybar bell click or `SIGRTMIN+4`), close any popup toasts currently on screen — they're now redundant with the panel showing the same notifications, and the orphaned popup overlapping the slide-out edge looks tacky.

**Architecture:** Add a `dismiss_all_popups()` method to `PopupManager` that closes every active popup window and clears the matching `state.active_popups` tracking set. Wire an `on_panel_open: Rc<dyn Fn()>` callback into `NotificationPanel::new` (same shape as the existing `on_state_change` callback per `CLAUDE.md`'s "shared `Rc<dyn Fn()>`" convention). Fire the callback in `toggle()` when transitioning from hidden → visible. `main.rs` wires the callback to call `popup_mgr.borrow_mut().dismiss_all_popups(&state)` so the panel layer stays decoupled from `PopupManager`.

**Tech Stack:** Rust, gtk4 widget close/visibility, existing `Rc<dyn Fn()>` callback pattern.

**Tracks:** Closes [#3](https://github.com/jasonherald/nwg-notifications/issues/3).

---

## File Structure

- **Modify:** `src/ui/popup.rs` — add `pub fn dismiss_all_popups(&mut self, state: &Rc<RefCell<NotificationState>>)`. Closes every window in `self.popups`, clears the vec, clears `state.active_popups`. Crucially does NOT mark anything read or touch history (per the issue: popups auto-dismissing doesn't mean read; same applies here — user is just deduping the UI).
- **Modify:** `src/ui/panel.rs` — add `on_panel_open: Rc<dyn Fn()>` parameter to `NotificationPanel::new`. Store as struct field. Fire from `toggle()`'s show branch (the `else` arm where the panel is going hidden → visible). Fire *before* the `idle_add_local_once` so popups disappear at the same moment the panel starts sliding in.
- **Modify:** `src/main.rs` — construct an `on_panel_open` callback that closes over `popup_mgr` and `state`, then pass it into `NotificationPanel::new`.
- **Modify:** `CHANGELOG.md` — entry under `[Unreleased]` Fixed.

No new files. No README change — this is invisible behavior fix to existing user-facing surface.

**On unit tests:** the fix is GTK-bound (closing windows, panel visibility transitions). There's no pure helper to extract — `dismiss_all_popups` is intrinsically about side-effecting GTK widgets, and the panel-toggle wiring runs through `idle_add_local_once`. The existing 63 tests ensure no regressions; the smoke-test gate (Task 5) confirms the actual fix. D-Bus integration testing (#16, deferred) would eventually cover this category, but that's a separate initiative.

---

## Pre-flight

- [ ] **Sync main and create branch**

```bash
cd /data/source/nwg-notifications
git checkout main && git pull --ff-only
git status
git checkout -b fix/panel-open-dismisses-popups
```

Expected: clean tree on `main`, then a fresh branch.

- [ ] **Commit the plan file as the first commit**

```bash
git add docs/superpowers/plans/2026-04-29-panel-open-dismisses-popups.md
git commit -m "docs: implementation plan for panel-open dismisses popups (#3)"
```

- [ ] **Baseline full cargo gambit**

```bash
make lint
```

Expected: every step exits 0; pre-existing `cargo deny` "unmatched skip" warnings are non-blocking.

---

## Task 1: Add `PopupManager::dismiss_all_popups`

**Files:**
- Modify: `src/ui/popup.rs` — new pub method, alongside the existing `dismiss(id: u32)`.

- [ ] **Step 1: Add the method**

In `src/ui/popup.rs`, immediately after the existing `dismiss(&mut self, id: u32)` method, add:

```rust
    /// Closes every visible popup window without marking anything read
    /// or touching history. Called when the panel opens — popups are
    /// redundant with the panel showing the same notifications, and the
    /// user opening the panel is purely a UI dedup, not an
    /// acknowledgement of the popups themselves.
    ///
    /// Mirrors the per-popup `dismiss(id)` shape but clears in bulk and
    /// also resets `state.active_popups` synchronously so the daemon's
    /// own bookkeeping doesn't go out of sync. (Auto-dismiss timers
    /// scheduled for these popups will still fire later — they no-op
    /// against an already-closed window and an already-empty set.)
    pub fn dismiss_all_popups(&mut self, state: &Rc<RefCell<NotificationState>>) {
        for popup in self.popups.drain(..) {
            popup.win.close();
        }
        state.borrow_mut().active_popups.clear();
    }
```

- [ ] **Step 2: Build, test, clippy**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean. No new tests — the method is GTK-bound, smoke test covers it.

- [ ] **Step 3: Commit**

```bash
git add src/ui/popup.rs
git commit -m "$(cat <<'EOF'
Add PopupManager::dismiss_all_popups (#3)

Closes every visible popup window without marking notifications as
read or touching history. Used by the panel-open callback in the
next commit — when the user opens the panel, popups become redundant
(panel shows the same notifications) and overlapping the slide-out
edge looks tacky.

Clears state.active_popups synchronously alongside self.popups so the
daemon's bookkeeping stays consistent. Auto-dismiss timers scheduled
for these popups still fire later but no-op against already-closed
windows and an already-empty set.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Add `on_panel_open` callback to `NotificationPanel`

**Files:**
- Modify: `src/ui/panel.rs` — add field + constructor parameter + fire in `toggle()`.

- [ ] **Step 1: Add the struct field**

In `src/ui/panel.rs`, add `on_panel_open: Rc<dyn Fn()>` to the `NotificationPanel` struct, alongside the existing `on_notification_click` and `on_state_change`:

```rust
pub struct NotificationPanel {
    pub win: gtk4::ApplicationWindow,
    backdrops: Vec<gtk4::ApplicationWindow>,
    revealer: gtk4::Revealer,
    list_box: gtk4::Box,
    panel_box: gtk4::Box,
    state: Rc<RefCell<NotificationState>>,
    config: Rc<RefCell<NotificationConfig>>,
    on_notification_click: Rc<dyn Fn(u32)>,
    on_state_change: Rc<dyn Fn()>,
    on_panel_open: Rc<dyn Fn()>,
}
```

- [ ] **Step 2: Add the constructor parameter**

Update `pub fn new(...)` to accept `on_panel_open: Rc<dyn Fn()>` as a new parameter (place it after `on_state_change` so the existing arg order is preserved). The full new signature:

```rust
    pub fn new(
        app: &gtk4::Application,
        state: &Rc<RefCell<NotificationState>>,
        config: &Rc<RefCell<NotificationConfig>>,
        on_notification_click: Rc<dyn Fn(u32)>,
        on_state_change: Rc<dyn Fn()>,
        on_panel_open: Rc<dyn Fn()>,
    ) -> Self {
```

In the `Self { ... }` literal at the bottom of `new()`, store the new field:

```rust
        let panel = Self {
            win,
            backdrops,
            revealer,
            list_box,
            panel_box,
            state: Rc::clone(state),
            config: Rc::clone(config),
            on_notification_click,
            on_state_change,
            on_panel_open,
        };
```

- [ ] **Step 3: Fire the callback in `toggle()`'s show branch**

In `toggle()`'s `else` arm (the path where the panel is going from hidden to visible), fire `on_panel_open` *before* the `idle_add_local_once` so popups start closing at the same moment the panel starts sliding in. Updated method:

```rust
    /// Toggles panel visibility with slide animation.
    pub fn toggle(&self) {
        if self.revealer.reveals_child() {
            hide_panel(&self.revealer, &self.win, &self.backdrops);
        } else {
            // Panel is going from hidden -> visible. Notify subscribers
            // (PopupManager dismisses any visible popups so the panel and
            // popups don't show the same notifications side-by-side).
            (self.on_panel_open)();

            // Rebuild, show backdrops + window, then slide in
            let list = self.list_box.clone();
            let state = Rc::clone(&self.state);
            let config = Rc::clone(&self.config);
            let panel_box = self.panel_box.clone();
            let on_click = Rc::clone(&self.on_notification_click);
            let on_change = Rc::clone(&self.on_state_change);
            let win = self.win.clone();
            let backdrops = self.backdrops.clone();
            let revealer = self.revealer.clone();
            gtk4::glib::idle_add_local_once(move || {
                rebuild_list(&list, &state, on_click, on_change);
                let width = config.borrow().panel_width;
                win.set_width_request(width);
                panel_box.set_width_request(width);
                for backdrop in &backdrops {
                    backdrop.set_visible(true);
                }
                win.set_visible(true);
                revealer.set_reveal_child(true);
            });
        }
    }
```

- [ ] **Step 4: Build (expecting compile error in main.rs)**

```bash
cargo build 2>&1 | tail -10
```

Expected: compile error in `src/main.rs` because `NotificationPanel::new` is called there with five arguments and now requires six. That's intentional — the next task fixes the call site. Hold the commit until Task 3 lands so the branch never has a broken intermediate.

---

## Task 3: Wire the `on_panel_open` callback in `main.rs`

**Files:**
- Modify: `src/main.rs` — construct the callback, pass it into `NotificationPanel::new`.

- [ ] **Step 1: Construct the callback**

In `src/main.rs::activate_notifications`, just before the existing `let panel = Rc::new(...)` block, build the `on_panel_open` closure that closes over `popup_mgr` and `state`:

```rust
    // Closing popups when the panel opens — see #3.
    let on_panel_open: Rc<dyn Fn()> = {
        let popup_mgr = Rc::clone(&popup_mgr);
        let state = Rc::clone(&state);
        Rc::new(move || {
            popup_mgr.borrow_mut().dismiss_all_popups(&state);
        })
    };
```

(Place this immediately after the `popup_mgr` construction and before the `on_panel_click` / `panel` construction.)

- [ ] **Step 2: Pass the callback into `NotificationPanel::new`**

Update the existing panel construction. Current:

```rust
    let panel = Rc::new(RefCell::new(NotificationPanel::new(
        app,
        &state,
        config,
        on_panel_click,
        Rc::clone(&on_state_change),
    )));
```

becomes:

```rust
    let panel = Rc::new(RefCell::new(NotificationPanel::new(
        app,
        &state,
        config,
        on_panel_click,
        Rc::clone(&on_state_change),
        on_panel_open,
    )));
```

- [ ] **Step 3: Build, test, clippy**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean. Existing 63 tests still pass.

- [ ] **Step 4: Commit Tasks 2 + 3 together**

The two tasks form one coherent change — the panel notifies PopupManager via the callback, and main.rs wires the callback. Single commit keeps the branch always-buildable.

```bash
git add src/ui/panel.rs src/main.rs
git commit -m "$(cat <<'EOF'
Dismiss visible popups when panel opens (#3)

Wires an on_panel_open: Rc<dyn Fn()> callback into NotificationPanel,
fired from toggle() on the hidden -> visible transition. main.rs
constructs the callback to call popup_mgr.dismiss_all_popups(state),
keeping the panel layer decoupled from PopupManager (the same shared
Rc<dyn Fn()> pattern as on_state_change).

Closes #3.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Documentation

**Files:**
- Modify: `CHANGELOG.md` — entry under `[Unreleased]` Fixed.

- [ ] **Step 1: CHANGELOG entry**

The current `CHANGELOG.md` has the `[0.3.2] — 2026-04-29` section at the top (just shipped). Add a new `[Unreleased]` block above it with a Fixed entry:

```markdown
## [Unreleased]

### Fixed

- Opening the notification panel now closes any visible popup toasts
  instead of leaving them on screen alongside the slide-out. Popups
  were redundant once the panel showed the same notifications, and
  overlapping popups on the panel's edge looked tacky. Closing the
  popups on panel-open is purely a UI dedup — it doesn't mark them
  read or touch history, so a user who hadn't yet clicked a popup can
  still see and act on it from inside the panel. (#3)

## [0.3.2] — 2026-04-29
```

(Keep the rest of the file unchanged.)

- [ ] **Step 2: Build, test, clippy sanity**

```bash
cargo build && cargo test && cargo clippy --all-targets -- -D warnings
```

Expected: clean (CHANGELOG isn't compiled, but running these confirms nothing else slipped).

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "$(cat <<'EOF'
docs: panel-open dismisses popups (#3)

CHANGELOG entry under new [Unreleased] Fixed section.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: User smoke-test gate (HARD STOP)

The fix is GTK-side; smoke test is the only meaningful validation.

- [ ] **Step 1: Install to the user's `~/.cargo/bin`**

```bash
make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
```

- [ ] **Step 2: Restart the user's session daemon**

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
> Installed and restarted the daemon. Smoke-test paths for #3:
>
> 1. **Bug reproducer should now show fixed behavior:**
>    - `notify-send "test 1" "should appear top-right"` — popup appears.
>    - Before it auto-dismisses (default 7s), click the waybar bell icon to open the panel.
>    - Expected: popup goes away the moment the panel starts sliding in. Panel slides in clean, no overlapping toast on the edge.
>
> 2. **Multiple popups stacked, panel open clears all:**
>    - Send three notifications in quick succession (`for i in 1 2 3; do notify-send "test $i" "msg"; sleep 0.3; done`).
>    - All three popups visible.
>    - Open the panel — all three popups disappear together.
>
> 3. **Panel close → new notify shows popup again:**
>    - Close the panel (click outside / Escape / waybar bell again).
>    - `notify-send "test 4" "fresh popup"` — new popup appears as before. The fix didn't break popup display.
>
> 4. **Popups are NOT marked read by the panel-open dedup:**
>    - Close all popups via panel-open. Open the panel again — the dismissed-from-screen notifications should still be visible in the history list, still showing as unread (the unread badge in waybar should still match the count).
>    - Click one inside the panel — that should mark it read normally.
>
> 5. **Regression check — existing surfaces unaffected:**
>    - DND toggle (waybar right-click menu) still works.
>    - `nwg-notifications --count` still works.
>    - Panel "Clear All" button still wipes history (different code path).
>
> Reply when satisfied or with anything that needs fixing.

**Do not proceed to Task 6 until the user explicitly approves.** If they report issues, return to the broken task.

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
git push -u origin fix/panel-open-dismisses-popups
```

- [ ] **Step 2: Create the PR**

```bash
gh pr create --title "Dismiss visible popups when panel opens (#3)" --body "$(cat <<'EOF'
## Summary

When the notification panel was opened (waybar bell click or \`SIGRTMIN+4\`), any popup toasts on screen stayed visible alongside the slide-out — same notification showing in two places, and the orphaned popup overlapping the panel's edge looked tacky.

Fix: panel-open now dismisses every visible popup, purely as a UI dedup. The popups don't get marked read and history isn't touched, so a user who opens the panel can still see and act on every notification from inside it.

**How it's wired:**
- New \`PopupManager::dismiss_all_popups(&state)\` closes every popup window and clears \`state.active_popups\`.
- New \`on_panel_open: Rc<dyn Fn()>\` callback parameter on \`NotificationPanel::new\`, fired from \`toggle()\` on the hidden → visible transition. Same shape as the existing \`on_state_change\` callback per \`CLAUDE.md\`'s shared-callback pattern.
- \`main.rs\` constructs the callback to call \`popup_mgr.borrow_mut().dismiss_all_popups(&state)\`, keeping the panel layer decoupled from \`PopupManager\`.

Closes #3.

## Test plan

- [x] \`make lint\` — fmt + clippy + test + deny + audit, all green locally.
- [x] All 63 unit tests still pass — pure refactor + new wiring, no behavior change to anything except the panel-open path.
- [x] Manual smoke test against the live compositor (5 checks):
  - [x] Single popup visible → open panel → popup goes away cleanly as panel slides in.
  - [x] Three popups stacked → open panel → all three dismissed together.
  - [x] Close panel → \`notify-send\` → new popup appears (popup display path unaffected).
  - [x] Dismissed popups are NOT marked read — they still show as unread in the panel and the waybar count is consistent.
  - [x] Regression: DND toggle, \`--count\`, panel "Clear All" all still work.

## Notes

- No unit tests added — the fix is GTK-bound (closing windows, panel toggle wiring through \`idle_add_local_once\`); there's no pure helper to extract. D-Bus / GTK integration testing is tracked separately in [#16](https://github.com/jasonherald/nwg-notifications/issues/16).
- Auto-dismiss timers scheduled for the dismissed popups still fire later but no-op against already-closed windows and an already-empty \`active_popups\` set.

The implementation plan (committed as \`docs/superpowers/plans/2026-04-29-panel-open-dismisses-popups.md\`) is on the branch for reviewer context.
EOF
)"
```

Expected: returns the PR URL.

- [ ] **Step 3: Hand off to CodeRabbit**

CodeRabbit reviews within minutes. **Default to fixing every finding in-PR.** Defer only when the fix needs new infrastructure or has wide blast radius — and when you do defer, open a tracking issue *first*. Reply protocol: inline replies for in-diff comments, single PR-level comment for outside-diff items, tag `@coderabbitai` every time so it learns from the responses.

---

## Acceptance checklist (cross-reference to issue #3)

- [ ] Panel-open closes every visible popup. — Task 1 (`dismiss_all_popups`) + Task 2 (callback fired from `toggle()`)
- [ ] Popups are not marked read by the panel-open dedup. — Task 1 (method explicitly skips `mark_read`; matches existing auto-dismiss semantics from `src/ui/popup.rs:131`)
- [ ] CHANGELOG entry. — Task 4
