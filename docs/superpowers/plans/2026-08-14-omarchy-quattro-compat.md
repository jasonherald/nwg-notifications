# Omarchy Quattro Compatibility + Shell Bar Widget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make nwg-notifications fully functional and polished on Omarchy 4.0 "Quattro" (Hyprland 0.56, Lua config, Quickshell shell) with zero regression for classic-hyprlang, Sway, and waybar setups.

**Architecture:** PR 1 (`feat/quattro-compat`): nwg-common 0.7 restores Lua-session dispatchers, a gated in-daemon `GDK_WAYLAND_DISABLE` guard fixes the DPMS crash on every launch path, docs gain Lua-autostart + Omarchy sections. PR 2 (`feat/omarchy-shell-widget`, branched off main **after PR 1 merges** — both touch CHANGELOG/README): a Quickshell bar-widget plugin consuming the existing status-JSON contract. Deployment last: disable the shell's `omarchy.notifications` plugin, verify our daemon's queued `REPLACE` request takes the name live.

**Tech Stack:** Rust (gtk4 0.11 / gio 0.22), nwg-common 0.7, Quickshell QML (`BarWidget`/`WidgetButton` from `qs.Ui`, `FileView`), Omarchy plugin manifest schemaVersion 1.

**Spec:** `docs/superpowers/specs/2026-08-14-omarchy-quattro-compat-design.md`

## Global Constraints

- **Zero regression for pre-Quattro setups**: classic hyprlang, Sway, waybar, conf-syntax autostart all keep working unchanged. Everything is additive or auto-detecting.
- NEVER run the `#[ignore]`d integration tests outside `make test-integration`; never kill processes you didn't spawn; a live daemon + live Omarchy shell run on this desktop.
- Gates per code task: `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (109 unit tests today; Task 2 adds 4), `make lint`, `make test-integration` (6 passed).
- CHANGELOG: active section heading `## [0.7.0] — Unreleased` (path-instruction format); user-visible bullets only.
- Every commit ends with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Reference material on this machine (read-only, safe): `/usr/share/omarchy/shell/plugins/bar/widgets/KeyboardLayout.qml` (simple bar widget), `/usr/share/omarchy/shell/plugins/agents/` (manifest + FileView pattern), `/usr/share/omarchy/shell/Ui/WidgetButton.qml`, `/usr/share/omarchy/shell/Commons/Util.qml` (`execDetached` = `bash -lc`).

---

### Task 1: nwg-common 0.7 + dependency refresh

**Files:**
- Modify: `Cargo.toml` (nwg-common line)
- Modify: `Cargo.lock` (cargo update)
- Modify: `deny.toml` (refresh pinned skips per the new tree)
- Modify: `CHANGELOG.md` (new active section)

**Interfaces:**
- Produces: `## [0.7.0] — Unreleased` CHANGELOG section that Tasks 2/3 append into.

- [ ] **Step 1: Bump nwg-common**

In `Cargo.toml`: `nwg-common = "0.6"` → `nwg-common = "0.7"`.

- [ ] **Step 2: Refresh the lockfile**

Run: `cargo update`
Capture the summary. Confirm `nwg-common v0.7.0` resolves. Note any major-version duplicates the update introduces or resolves.

- [ ] **Step 3: Reconcile deny.toml skips against the new tree**

Current `[bans]` skip pins `syn@2.0.119` ("proc-macro deps split between syn 2 and syn 3", with a comment saying the pin tracks the lockfile). Check reality:

Run: `grep -A1 -E '^name = "(syn|anstream|toml_datetime)"$' Cargo.lock | grep -E 'name|version' | paste - - | sort -u`

- If syn 2.x moved (e.g. 2.0.121), update the pin to the exact resolved version.
- If the 2/3 split disappeared, remove the skip and its comment.
- If new duplicate splits appeared, run `cargo deny check bans 2>&1 | head -30` and add exact-version skips with reasons, same style.

Run: `cargo deny check` → `advisories ok, bans ok, licenses ok, sources ok` (the two `license-not-encountered` warnings for BSD-3-Clause/Zlib are pre-existing and fine). Run `cargo audit` → clean.

- [ ] **Step 4: Verify the dispatcher-fallback claim compiles clean**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: clean build, zero warnings, all unit tests pass. (`DockError` appears nowhere in src/, so 0.7's `non_exhaustive` change should require no code edits — if the compiler disagrees, the fix is adding a wildcard arm where it says, and report it.)

- [ ] **Step 5: CHANGELOG**

Insert above `## [0.6.0] — 2026-07-21`:

```markdown
## [0.7.0] — Unreleased

### Changed

- Dependency refresh: `nwg-common` `0.6` → `0.7` plus a routine
  lockfile update.

### Fixed

- Notification click-to-focus / deep-linking now works on Hyprland
  0.55+ sessions using the Lua configuration (Omarchy 4.0 "Quattro").
  Such sessions reject the legacy textual IPC dispatchers;
  `nwg-common 0.7` detects the rejection and retries in the session's
  `hl.dsp.*` syntax. Classic hyprlang sessions are unaffected.
```

(Keep-a-Changelog order within the section as bullets accumulate: Added, Changed, Fixed. Later tasks insert their sections accordingly.)

Note: the "routine lockfile update" phrasing under Changed is one line inside a user-facing dependency bullet — acceptable; do NOT add a standalone implementation-detail bullet (path instructions).

- [ ] **Step 6: Gates + commit**

Run: `make lint` → exit 0. Run: `make test-integration` → 6 passed.

```bash
git add -A
git commit -m "chore(deps): nwg-common 0.7 + lockfile refresh

nwg-common 0.7.0 adds the Lua dispatcher fallback: Hyprland 0.55+
Lua-config sessions (Omarchy 4.0) reject legacy textual dispatchers,
which broke notification click-to-focus / deep-linking there. The
fallback auto-detects; classic hyprlang sessions take the old path
unchanged. DockError going non_exhaustive in 0.7 touches nothing in
this crate.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: In-daemon GDK dmabuf-crash guard (TDD)

**Files:**
- Modify: `src/app.rs` (predicate + wiring at top of `run()`; tests at bottom of file per convention)
- Modify: `CHANGELOG.md` (Fixed bullet)

**Interfaces:**
- Produces: `fn needs_dmabuf_workaround(existing_setting: Option<&std::ffi::OsStr>, hyprland_session: bool, gtk_version: (u32, u32)) -> bool` — module-private in `app.rs`.

- [ ] **Step 1: Write the failing tests**

Append inside the existing `#[cfg(test)] mod tests` block in `src/app.rs`:

```rust
    #[test]
    fn dmabuf_workaround_applies_on_hyprland_with_affected_gtk() {
        assert!(needs_dmabuf_workaround(None, true, (4, 22)));
        assert!(needs_dmabuf_workaround(None, true, (4, 21)));
    }

    #[test]
    fn dmabuf_workaround_respects_explicit_user_setting() {
        use std::ffi::OsStr;
        assert!(!needs_dmabuf_workaround(
            Some(OsStr::new("zwp_linux_dmabuf_v1")),
            true,
            (4, 22)
        ));
        // Even an empty explicit setting is a user decision.
        assert!(!needs_dmabuf_workaround(Some(OsStr::new("")), true, (4, 22)));
    }

    #[test]
    fn dmabuf_workaround_skips_non_hyprland_sessions() {
        assert!(!needs_dmabuf_workaround(None, false, (4, 22)));
    }

    #[test]
    fn dmabuf_workaround_retires_on_fixed_gtk() {
        assert!(!needs_dmabuf_workaround(None, true, (4, 23)));
        assert!(!needs_dmabuf_workaround(None, true, (5, 0)));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test dmabuf 2>&1 | tail -5`
Expected: compile error — `needs_dmabuf_workaround` not found.

- [ ] **Step 3: Implement the predicate**

Add to `src/app.rs` (below `run()`, above the other helpers):

```rust
/// True when the GDK dmabuf workaround should be applied at startup:
/// no explicit user `GDK_WAYLAND_DISABLE` setting, a Hyprland session,
/// and a linked GTK still affected by the bug (<= 4.22). Pure so the
/// decision table is unit-testable; the env mutation stays in [`run`].
fn needs_dmabuf_workaround(
    existing_setting: Option<&std::ffi::OsStr>,
    hyprland_session: bool,
    gtk_version: (u32, u32),
) -> bool {
    existing_setting.is_none() && hyprland_session && gtk_version <= (4, 22)
}
```

- [ ] **Step 4: Wire it into `run()`**

In `src/app.rs`, directly after the `nwg_common::process::handle_dump_args();` line (i.e. before clap parsing and long before any GTK call):

```rust
    // GTK4 <= 4.22 crashes (munmap typo in gdkdmabuf-wayland.c) when
    // Hyprland >= 0.56 re-sends dmabuf feedback on DPMS cycles — see
    // README "Known issue". Disable GTK's dmabuf path on Hyprland
    // sessions while the linked GTK is still affected, so every launch
    // path (autostart with or without an env wrapper, D-Bus activation,
    // manual runs) is covered. Never overrides an explicit user
    // setting; Sway and fixed-GTK setups are untouched. Hyprland < 0.56
    // also takes this path — the only cost there is GTK falling back
    // to shm buffers. The version gate self-retires once GTK > 4.22 is
    // installed.
    let gdk_disable = std::env::var_os("GDK_WAYLAND_DISABLE");
    if needs_dmabuf_workaround(
        gdk_disable.as_deref(),
        std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some(),
        (gtk4::major_version(), gtk4::minor_version()),
    ) {
        // SAFETY: single-threaded at this point — before the signal
        // listener thread starts and before GTK spawns any workers.
        unsafe {
            std::env::set_var("GDK_WAYLAND_DISABLE", "zwp_linux_dmabuf_v1");
        }
    }
```

`gtk4::major_version()` / `gtk4::minor_version()` are plain getters over the linked library and do not require `gtk4::init` — verified against gtk4-rs 0.11.4, whose wrappers explicitly skip the initialization assertion. Use them directly.

- [ ] **Step 5: Run tests + full suite**

Run: `cargo test 2>&1 | grep 'test result'` → all pass (4 new).
Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --all` → clean.

- [ ] **Step 6: Live sanity check (safe — client mode only, no daemon spawned)**

`--count` is a short-lived D-Bus client query (`NO_AUTO_START`) that runs through the new guard code and exits — safe to run directly. Verify both guard branches exit cleanly:

Run: `GDK_WAYLAND_DISABLE= cargo run --quiet -- --count` → prints the count; the explicitly-empty user setting is respected (guard no-ops).
Run: `cargo run --quiet -- --count` → prints the count; on this Hyprland+GTK-4.22 machine the guard fires before the query (harmless for a client call). No panics either way.

- [ ] **Step 7: CHANGELOG**

Add to the `### Fixed` section of `## [0.7.0] — Unreleased`:

```markdown
- The daemon no longer crashes on monitor DPMS cycles under
  Hyprland ≥ 0.56 with GTK ≤ 4.22 (a GTK dmabuf-feedback bug). The
  daemon now sets `GDK_WAYLAND_DISABLE=zwp_linux_dmabuf_v1` itself on
  Hyprland sessions with an affected GTK — covering D-Bus activation
  and un-wrapped autostart lines, not just launches that carried the
  env manually. An explicit `GDK_WAYLAND_DISABLE` setting is always
  respected, Sway sessions are untouched, and the workaround
  self-retires once a fixed GTK (> 4.22) is installed.
```

- [ ] **Step 8: Commit**

```bash
git add src/app.rs CHANGELOG.md
git commit -m "fix: apply GDK dmabuf workaround in-daemon on affected Hyprland+GTK

GTK4 <= 4.22 crashes on DPMS cycles under Hyprland >= 0.56 unless
GDK_WAYLAND_DISABLE=zwp_linux_dmabuf_v1 is set. Setting it inside
run() (gated: no explicit user setting, Hyprland session, GTK <= 4.22)
covers every launch path including D-Bus activation, which execs the
bare binary from the .service files. Predicate extracted and
table-tested; Sway and fixed-GTK setups take no change.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Docs — Lua autostart + Omarchy 4.0 (README, CLAUDE.md)

**Files:**
- Modify: `README.md` (inside the existing `## Hyprland autostart` section; and a new section immediately before `## Waybar integration`)
- Modify: `CLAUDE.md` (Run locally / integration notes area)
- Modify: `CHANGELOG.md` (docs bullet)

- [ ] **Step 1: README — Lua autostart subsection**

The existing `## Hyprland autostart` section shows `autostart.conf` `exec-once` forms. Append this subsection to it (keep the existing content untouched):

````markdown
### Hyprland Lua config (Omarchy 4.0 "Quattro" and other Lua setups)

Hyprland 0.55+ Lua configurations don't read `autostart.conf`. On
Omarchy 4.0 the equivalent lives in `~/.config/hypr/autostart.lua`:

```lua
-- ~/.config/hypr/autostart.lua
o.launch_on_start([[nwg-notifications --persist]])
```

(On plain Lua setups without Omarchy's helpers, register the command
on the start hook:
`hl.on("hyprland.start", function() hl.exec_cmd([[nwg-notifications --persist]]) end)`.)

**Migrating to Omarchy 4.0:** the Quattro migration generates the new
`.lua` config files but does **not** carry custom `exec-once` lines
across from `autostart.conf`. For nwg-notifications the loss is partly
masked — the D-Bus service files still auto-activate the daemon on the
first notification once the name is free — but activation only wins if
nothing else owns `org.freedesktop.Notifications` (see the Omarchy 4.0
section below), so re-add the autostart line explicitly.
````

- [ ] **Step 2: README — Omarchy 4.0 section**

Insert a new top-level section immediately BEFORE `## Waybar integration`:

````markdown
## Omarchy 4.0 "Quattro"

Omarchy 4.0 ships its own notification engine inside the Quickshell
shell (`omarchy-shell`), and it owns `org.freedesktop.Notifications`
from session start — nwg-notifications will start but sits idle
without the name. To use nwg-notifications as your daemon, disable
the shell's engine:

```bash
omarchy plugin disable omarchy.notifications
```

nwg-notifications requests the name with the D-Bus `REPLACE` flag and
the request stays queued at the bus, so an already-running daemon
takes the name over the moment the shell releases it — no restart
needed. Re-enable the shell engine any time with
`omarchy plugin enable omarchy.notifications`; whichever owns the name
receives the notifications.

Quattro has no waybar — the bell/unread badge for the Quickshell bar
ships as an Omarchy shell plugin in this repo: see
`contrib/omarchy-plugin/nwg.notifications/` (install with
`make install-omarchy-plugin`, then
`omarchy plugin enable nwg.notifications --section right`). The waybar
module below keeps working unchanged for waybar setups.
````

(The `contrib/…` path and make target land in PR 2 — writing the pointer now is fine; both PRs merge before any release.)

- [ ] **Step 3: README — label the classic sections**

In `## Hyprland autostart`, after the section heading, add one orienting line: `The conf-syntax forms below apply to classic (pre-Lua / pre-Omarchy-4.0) Hyprland configs; for Lua configs see the subsection at the end.` Do not alter the snippets themselves.

- [ ] **Step 4: CLAUDE.md note**

In CLAUDE.md's "Run locally" section, append:

```markdown
On Omarchy 4.0 (Quattro) the Quickshell shell's own notification
engine owns `org.freedesktop.Notifications`; disable it with
`omarchy plugin disable omarchy.notifications` or the daemon starts
but never receives notifications. The Quickshell bar badge lives in
`contrib/omarchy-plugin/nwg.notifications/` (waybar's successor there;
waybar integration is unchanged for waybar setups).
```

- [ ] **Step 5: CHANGELOG**

Add an `### Added` section ABOVE `### Changed` in `## [0.7.0] — Unreleased`:

```markdown
### Added

- README documents Hyprland Lua-config autostart (Omarchy 4.0
  "Quattro"): `autostart.conf` is not read there and the Quattro
  migration does not carry custom `exec-once` lines across. A new
  "Omarchy 4.0" section covers disabling the shell's built-in
  notification engine (`omarchy plugin disable omarchy.notifications`)
  so nwg-notifications can own `org.freedesktop.Notifications`.
```

- [ ] **Step 6: Gates + commit**

Run: `make lint` → exit 0 (docs-only diff; ritual gate).

```bash
git add README.md CLAUDE.md CHANGELOG.md
git commit -m "docs: Lua autostart + Omarchy 4.0 Quattro section

Mirrors nwg-dock PR #102 for the autostart migration, plus the
Quattro-specific engine conflict: the Quickshell shell's notification
plugin owns org.freedesktop.Notifications, and disabling it hands the
name to the already-running daemon (queued REPLACE request). Classic
conf-syntax and waybar sections stay unchanged, labeled.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: PR 1 — gates, issue, pull request

- [ ] **Step 1: Full gates**

Run in order, each must succeed: `make lint` (exit 0) · `make test-integration` (6 passed) · `make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin` · `~/.cargo/bin/nwg-notifications --version` · `~/.cargo/bin/nwg-notifications --count` (returns a number — the daemon's count IPC works).

- [ ] **Step 2: Tracking issue**

```bash
gh issue create --title "Omarchy 4.0 Quattro compatibility: Lua dispatchers, DPMS crash guard, autostart + engine-conflict docs" --body "Quattro breaks/orphans three things: (1) Lua-config Hyprland rejects legacy textual dispatchers — click-to-focus/deep-linking fails (nwg-common 0.7 has the fallback); (2) GTK <= 4.22 crashes on DPMS cycles under Hyprland >= 0.56 — the D-Bus activation path execs the bare binary with no env workaround; (3) autostart.conf is not read by Lua configs and the migration drops exec-once lines, plus the shell's own notification engine owns org.freedesktop.Notifications, so the daemon starts deaf. Spec: docs/superpowers/specs/2026-08-14-omarchy-quattro-compat-design.md. The Quickshell bar widget (waybar successor) is tracked separately."
```

Note the issue number for the PR body.

- [ ] **Step 3: Push + PR**

```bash
git push -u origin feat/quattro-compat
gh pr create --title "fix: Omarchy 4.0 Quattro compatibility" --body "<summary of the three fixes + back-compat statement + testing evidence>

Closes #<issue-from-step-2>

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

PR body must state the back-compat guarantees explicitly (classic hyprlang unaffected via auto-detect; Sway untouched by the env gate; waybar/conf docs unchanged). Then the standard CodeRabbit cycle (controller handles).

---

### Task 5: Omarchy shell bar-widget plugin

**Branch:** `feat/omarchy-shell-widget` off fresh main (after PR 1 merges).

**Files:**
- Create: `contrib/omarchy-plugin/nwg.notifications/manifest.json`
- Create: `contrib/omarchy-plugin/nwg.notifications/Widget.qml`
- Create: `contrib/omarchy-plugin/nwg.notifications/README.md`

**Interfaces:**
- Consumes: `$XDG_RUNTIME_DIR/nwg-notifications-status.json` — `{"text","tooltip","alt","class","count"}` (existing waybar contract, unchanged).
- Produces: plugin id `nwg.notifications` that Task 6's make targets install.

- [ ] **Step 1: manifest.json**

```json
{
  "schemaVersion": 1,
  "id": "nwg.notifications",
  "name": "nwg-notifications",
  "version": "1.0.0",
  "author": "Jason Herald",
  "license": "MIT",
  "description": "Bell + unread count for the nwg-notifications daemon",
  "kinds": ["bar-widget"],
  "entryPoints": {
    "barWidget": "Widget.qml"
  },
  "barWidget": {
    "displayName": "nwg-notifications",
    "description": "Bell icon with unread count; left-click toggles the panel, right-click the DND menu, middle-click DND",
    "category": "Notifications",
    "allowMultiple": false
  }
}
```

- [ ] **Step 2: Widget.qml**

```qml
import QtQuick
import Quickshell
import Quickshell.Io
import qs.Ui
import qs.Commons

// Bar badge for the nwg-notifications daemon. A second consumer of the
// same status-file contract the waybar module reads: the daemon writes
// nwg-notifications-status.json under XDG_RUNTIME_DIR on every state
// change (the SIGRTMIN+11 waybar ping keeps firing but is not needed
// here — FileView watches the file directly). The daemon composes the
// glyph/tooltip for every state (empty / unread / DND), so the widget
// displays rather than recomputes.
BarWidget {
  id: root
  moduleName: "nwg.notifications"

  readonly property string runtimeDir: Quickshell.env("XDG_RUNTIME_DIR") || ""
  readonly property string statusPath: runtimeDir ? runtimeDir + "/nwg-notifications-status.json" : ""
  property var status: null

  FileView {
    path: root.statusPath
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.parse(text())
    onLoadFailed: root.status = null
  }

  function parse(content) {
    try {
      var parsed = JSON.parse(String(content || ""))
      root.status = parsed && typeof parsed === "object" ? parsed : null
    } catch (e) {
      root.status = null
    }
  }

  // The daemon listens on realtime signals (panel: RTMIN+4, DND toggle:
  // RTMIN+5, DND menu: RTMIN+6) — same contract as the waybar module's
  // click bindings. bar.run executes via `bash -lc`, so the RTMIN name
  // (portable across glibc/musl offsets) and $(pidof) both resolve
  // there; pidof matches the exact binary rather than pkill -f's
  // any-cmdline-substring.
  function signalDaemon(offset) {
    if (root.bar) root.bar.run("kill -s RTMIN+" + offset + " $(pidof nwg-notifications)")
  }

  // Hidden until the daemon has written a status file: no runtime dir,
  // no file, or unparseable content all mean there is nothing truthful
  // to show.
  visible: statusPath !== "" && status !== null
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.status ? String(root.status.text || "") : ""
    tooltipText: root.status ? String(root.status.tooltip || "") : ""
    horizontalMargin: 6
    onPressed: function() { root.signalDaemon(4) }
  }

  // WidgetButton handles the styled left-click; right/middle arrive
  // here. acceptedButtons excludes LeftButton so hover/press styling
  // stays with WidgetButton underneath.
  MouseArea {
    anchors.fill: parent
    acceptedButtons: Qt.RightButton | Qt.MiddleButton
    onClicked: function(mouse) {
      if (mouse.button === Qt.RightButton) root.signalDaemon(6)
      else if (mouse.button === Qt.MiddleButton) root.signalDaemon(5)
    }
  }
}
```

**Contract reconciliation (required, before calling this step done):** read `/usr/share/omarchy/shell/Ui/WidgetButton.qml` and `/usr/share/omarchy/shell/plugins/bar/widgets/KeyboardLayout.qml`. If `WidgetButton` exposes multi-button press info (e.g. `onPressed(mouse)` with button, or a `rightPress` signal), prefer that over the MouseArea overlay and delete the overlay. If `BarWidget` requires other mandatory properties, add them matching KeyboardLayout's usage. Keep the FileView/parse/signalDaemon structure as-is.

- [ ] **Step 3: Plugin README.md**

```markdown
# nwg.notifications — Omarchy shell bar widget

Bell + unread count for the [nwg-notifications](../../..) daemon on the
Omarchy 4.0+ Quickshell bar. Successor to the waybar module on Omarchy;
reads the same status file, drives the daemon over the same signals.

| Action | Effect |
|--------|--------|
| Left-click | Toggle notification panel (`SIGRTMIN+4`) |
| Right-click | DND duration menu (`SIGRTMIN+6`) |
| Middle-click | Toggle DND (`SIGRTMIN+5`) |

## Install

From the repo root:

    make install-omarchy-plugin
    omarchy plugin enable nwg.notifications --section right

The widget hides itself until the daemon has written its status file
(`$XDG_RUNTIME_DIR/nwg-notifications-status.json`). Remember to disable
Omarchy's built-in engine so the daemon actually receives
notifications: `omarchy plugin disable omarchy.notifications` — see the
repo README's "Omarchy 4.0" section.

## Uninstall

    omarchy plugin disable nwg.notifications
    make uninstall-omarchy-plugin
```

- [ ] **Step 4: Validate**

Run: `omarchy plugin validate contrib/omarchy-plugin/nwg.notifications`
Expected: passes. Fix schema complaints exactly as reported (the shipped `agents` and `keyboard-layout` manifests are the reference).

- [ ] **Step 5: Commit**

```bash
git add contrib/
git commit -m "feat: Omarchy shell bar-widget plugin (nwg.notifications)

Quickshell bar badge consuming the existing waybar status-file
contract via FileView watch (no polling, no daemon changes). Clicks
mirror the waybar module: panel / DND menu / DND toggle over the
existing realtime signals, sent as kill -s RTMIN+N \$(pidof ...) so
glibc/musl offsets and exact-binary matching are both handled.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: Make targets + repo docs for the plugin

**Files:**
- Modify: `Makefile` (.PHONY, HELP_TEXT, two targets after `uninstall-dbus`)
- Modify: `CHANGELOG.md` (Added bullet)
- Modify: `README.md` (fill the Omarchy section's install pointer if PR 1's wording needs the final paths corrected — verify, likely no-op)

- [ ] **Step 1: Makefile targets**

Add `install-omarchy-plugin uninstall-omarchy-plugin` to the `.PHONY` line (alongside install-dbus/uninstall-dbus). Add to HELP_TEXT after the `make uninstall-dbus` line:

```
  make install-omarchy-plugin    Install the Omarchy shell bar widget to ~/.config/omarchy/plugins (user-scope)
  make uninstall-omarchy-plugin  Remove the Omarchy shell bar widget
```

Add after the `uninstall-dbus` recipe (tab-indented recipe lines):

```make
# Omarchy shell (Quickshell) bar-widget plugin — Omarchy 4.0+. Always
# user-scope, like install-dbus: the shell loads user plugins from
# ~/.config/omarchy/plugins/ and hot-reloads on change. Validated via
# the omarchy CLI when present (skipped otherwise so the target works
# on non-Omarchy machines, e.g. packagers). Enabling the widget on the
# bar is user config, left to the user:
#   omarchy plugin enable nwg.notifications --section right
OMARCHY_PLUGIN_SRC := contrib/omarchy-plugin/nwg.notifications
OMARCHY_PLUGIN_DIR := $(HOME)/.config/omarchy/plugins/nwg.notifications

install-omarchy-plugin:
	@if command -v omarchy >/dev/null 2>&1; then \
		omarchy plugin validate "$(OMARCHY_PLUGIN_SRC)" || exit 1; \
	else \
		echo "note: omarchy CLI not found — skipping manifest validation"; \
	fi
	@mkdir -p "$(OMARCHY_PLUGIN_DIR)"
	@cp -r $(OMARCHY_PLUGIN_SRC)/. "$(OMARCHY_PLUGIN_DIR)/"
	@echo "Installed to $(OMARCHY_PLUGIN_DIR)"
	@echo "Enable with: omarchy plugin enable nwg.notifications --section right"

uninstall-omarchy-plugin:
	rm -rf "$(OMARCHY_PLUGIN_DIR)"
	@echo "Removed $(OMARCHY_PLUGIN_DIR) (disable in the bar with: omarchy plugin disable nwg.notifications)"
```

- [ ] **Step 2: Verify the targets**

Run: `make install-omarchy-plugin` → validates + installs to `~/.config/omarchy/plugins/nwg.notifications/`. Run: `omarchy plugin list --json | python3 -c "import json,sys; print([p['id'] for p in json.load(sys.stdin) if 'nwg' in p.get('id','')])"` (adjust jq-style path to actual output shape) → shows `nwg.notifications` discovered. Do NOT enable it on the bar (deployment step does that). Leave the copy installed.

- [ ] **Step 3: CHANGELOG**

Add to `### Added` under the active `## [0.7.0] — Unreleased` section (create the section if PR 1's entries moved — after PR 1's merge the section exists):

```markdown
- Omarchy shell bar widget (`contrib/omarchy-plugin/nwg.notifications`,
  installed via `make install-omarchy-plugin`): bell + unread count on
  the Omarchy 4.0 Quickshell bar, the waybar module's successor there.
  Left-click toggles the panel, right-click the DND menu, middle-click
  DND — the same signal contract as the waybar module, driven by the
  same status file. Waybar setups are unaffected.
```

- [ ] **Step 4: Gates + commit**

Run: `make lint` → exit 0. Run: `make test-integration` → 6 passed (no code change; ritual).

```bash
git add Makefile CHANGELOG.md README.md
git commit -m "feat: make install-omarchy-plugin target + docs

User-scope like install-dbus; validates via the omarchy CLI when
present and degrades to a plain copy elsewhere. Enabling on the bar
stays a documented user step (shell.json is user config).

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: PR 2 — gates, issue, pull request

- [ ] **Step 1: Gates** — `make lint` exit 0; `make test-integration` 6 passed; `omarchy plugin validate contrib/omarchy-plugin/nwg.notifications` passes.

- [ ] **Step 2: Tracking issue**

```bash
gh issue create --title "Omarchy shell bar widget: waybar-module successor for the Quattro Quickshell bar" --body "Quattro has no waybar, so the bell/unread badge (status JSON + SIGRTMIN contract) has no consumer there. Ship a first-party Omarchy shell bar-widget plugin (contrib/omarchy-plugin/nwg.notifications) reading the same status file via FileView watch, clicks mirroring the waybar module. Additive: waybar setups unchanged. Spec: docs/superpowers/specs/2026-08-14-omarchy-quattro-compat-design.md."
```

- [ ] **Step 3: Push + PR** — same shape as Task 4 Step 3: title `feat: Omarchy shell bar widget (nwg.notifications)`, body with summary/testing/back-compat + `Closes #<issue>` + the Claude Code footer. CodeRabbit cycle follows (controller).

---

### Task 8: Live deployment + takeover verification (controller-led, after both PRs merge)

- [ ] **Step 1:** Sync main; `make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin`; `make install-omarchy-plugin`; `make upgrade PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin` (bounce the daemon onto the merged binary — the guard now sets GDK_WAYLAND_DISABLE itself).
- [ ] **Step 2:** Record current owner: `gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus --method org.freedesktop.DBus.GetNameOwner org.freedesktop.Notifications`, then `omarchy plugin disable omarchy.notifications`, then re-query and map the unique name to a PID (`GetConnectionUnixProcessID`) — expect our daemon's PID **without a restart**. If the takeover does not happen automatically, `make upgrade` again and amend the README wording (the spec's documented contingency).
- [ ] **Step 3:** `omarchy plugin enable nwg.notifications --section right`; verify the bell renders; `notify-send "nwg-notifications" "Quattro round-trip"` → OUR toast appears (not the shell's) and the badge count updates; left-click opens the panel; middle-click toggles DND (glyph changes); right-click opens the DND menu; verify a notification click focuses the sending app (the nwg-common 0.7 dispatcher fix, live).
- [ ] **Step 4:** Confirm `GDK_WAYLAND_DISABLE` is in the daemon's environ (`tr '\0' '\n' < /proc/$(pidof nwg-notifications)/environ | grep GDK`) — guard active. Report everything; Jason soaks 1–2 days. Release ritual (0.7.0: stamp, bump, tag, publish) only after the soak.

---

## Self-review notes (completed)

- **Spec coverage:** A1→Task 1, A2→Task 2, A3→Task 3, B→Tasks 5-6, verification/rollout→Tasks 4/7/8. Deps rider→Task 1. No gaps.
- **Placeholder scan:** PR bodies in Tasks 4/7 are summarized by intent rather than verbatim (controller writes them from the merged commit set — consistent with how this repo's PR bodies are built from evidence); all code/doc content is verbatim.
- **Type consistency:** `needs_dmabuf_workaround` signature identical in test/impl/wiring; plugin id `nwg.notifications` consistent across manifest/Makefile/README/commands; status-file field names match `waybar.rs` (`text`, `tooltip`, `alt`, `class`, `count`).
- **Adjust-on-contact points (not placeholders):** `WidgetButton` multi-button capability (fallback MouseArea provided), `omarchy plugin list --json` output shape (verification step), `gtk4::*_version()` pre-init callability (ffi fallback named).
