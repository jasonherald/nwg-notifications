# Omarchy 4.0 "Quattro" compatibility + shell bar widget — design

**Date:** 2026-08-14
**Branch(es):** `feat/quattro-compat` (items 1–4 + deps), `feat/omarchy-shell-widget` (item 5)
**Context commits elsewhere:** nwg-dock PR #102 (Lua autostart docs), nwg-common 0.7.0 (Lua dispatcher fallback)

## Problem

Omarchy 4.0 (Quattro, Hyprland 0.56.2) changes three things that break or
orphan nwg-notifications:

1. **A Quickshell notification engine ships enabled.** The `omarchy-shell`
   process's `omarchy.notifications` plugin owns
   `org.freedesktop.Notifications`; our daemon starts (autostart.lua) but
   loses the name race and sits deaf, holding only `org.nwg.Notifications`.
2. **Lua-config Hyprland rejects legacy textual dispatchers.** Our
   click-to-focus / deep-linking path (`Compositor::focus_window` via
   nwg-common 0.6) fails on Lua sessions. nwg-common 0.7.0 fixes this with
   detect-and-retry in `hl.dsp.*` syntax (classic sessions unaffected).
3. **waybar is gone.** The status-JSON + `SIGRTMIN+11` badge integration
   has no consumer on Quattro; the bar is Quickshell (`shell.json`).

Also: `autostart.conf` is not read by Lua config, and the Quattro migration
does not carry `exec-once` lines across (dock PR #102 wording); and GTK4
≤ 4.22 crashes on DPMS cycles under Hyprland ≥ 0.56 unless
`GDK_WAYLAND_DISABLE=zwp_linux_dmabuf_v1` is set — the autostart.lua entry
carries the env, but D-Bus activation (our .service files) execs the bare
binary.

## Hard constraint

**No regression for pre-Quattro setups.** Classic hyprlang Hyprland, Sway,
waybar users, and conf-syntax autostart must all keep working exactly as
today. Everything below is additive or auto-detecting.

## Work item A — Quattro compatibility (PR 1, `feat/quattro-compat`)

### A1. nwg-common 0.6 → 0.7 + dependency refresh

- `nwg-common = "0.7"` in Cargo.toml. The 0.7.0 breaking change
  (`DockError` `#[non_exhaustive]`) touches nothing in this crate (zero
  `DockError` references in src/). The Lua dispatcher fallback is
  auto-detecting: classic sessions take the legacy path unchanged.
- Full `cargo update` lockfile refresh (last refreshed 2026-07-21).
  Re-verify the `deny.toml` `syn@2.0.119` pinned skip against the new
  tree (refresh the pin if syn moved; drop or add skips per what the
  resolver produces, same procedure as PR #76). `cargo deny` + `cargo
  audit` must pass; deny advisories/bans clean.
- Changelog: Fixed — notification click-to-focus / deep-linking on
  Hyprland Lua-config sessions (nwg-common 0.7 dispatcher fallback);
  Changed — dependency refresh.

### A2. In-daemon GDK dmabuf-crash guard

Instead of baking `env GDK_WAYLAND_DISABLE=…` into the D-Bus service
templates (which would also apply on Sway and non-broken setups), the
daemon sets the workaround itself, early in `app::run()` **before any GTK
call**, gated so it only applies where the bug exists:

```rust
// GTK4 <= 4.22 crashes (munmap typo in gdkdmabuf-wayland.c) when
// Hyprland >= 0.56 re-sends dmabuf feedback on DPMS cycles. Disable
// GTK's dmabuf path on Hyprland sessions until a fixed GTK is
// detected at runtime. Set BEFORE gtk4 initializes; never overrides
// an explicit user setting. Hyprland < 0.56 also takes this path —
// the only cost there is GTK falling back to shm buffers.
if std::env::var_os("GDK_WAYLAND_DISABLE").is_none()
    && std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
    && (gtk4::major_version(), gtk4::minor_version()) <= (4, 22)
{
    // SAFETY: single-threaded at this point — before the signal-listener
    // thread and before GTK spawns anything.
    unsafe { std::env::set_var("GDK_WAYLAND_DISABLE", "zwp_linux_dmabuf_v1") };
}
```

- Covers **every** launch path: autostart (with or without the env
  wrapper), D-Bus activation, manual runs. Service templates stay
  unchanged.
- Self-retiring: once GTK ships the fix (> 4.22), the guard no-ops.
- Sway/other compositors: untouched (env gate).
- `gtk4::major_version()` / `minor_version()` are callable before
  `gtk4::init` (plain getters over the linked library). Verify at
  implementation time; if they turn out to require init, fall back to a
  compile-time-independent runtime probe or drop the version gate and
  gate on Hyprland only (documented trade-off).
- Unit-testable seam: extract the predicate into
  `fn needs_dmabuf_workaround(existing: Option<&OsStr>, hyprland: bool, gtk: (u32, u32)) -> bool`
  with table tests; the env mutation stays in `run()`.
- Changelog: Fixed — daemon no longer crashes on monitor DPMS cycles
  under Hyprland ≥ 0.56 with GTK ≤ 4.22, regardless of launch path.

### A3. Docs: Lua autostart + Omarchy 4.0 section (README)

- New README subsection mirroring dock PR #102, adapted:

  ```lua
  -- ~/.config/hypr/autostart.lua
  o.launch_on_start([[nwg-notifications --persist]])
  ```

  plus the plain-Lua `hl.on("hyprland.start", …)` variant and the
  migration warning (autostart.conf not read; exec-once lines not
  carried across — though for us, D-Bus activation revives the daemon on
  first notification *once the name is free*, so the failure is masked
  rather than absent).
- New README section **"Omarchy 4.0 (Quattro)"**: Quattro ships its own
  Quickshell notification engine that owns
  `org.freedesktop.Notifications`; to use nwg-notifications run
  `omarchy plugin disable omarchy.notifications`. Our daemon requests
  the name with `REPLACE` and gio leaves the request queued, so a
  running daemon takes the name over the moment the shell releases it —
  no restart (verify live during install; if the takeover does NOT
  happen automatically, document the restart and consider a follow-up).
  Point at the bar-widget plugin (item B) as the waybar replacement.
- Existing conf-syntax autostart and waybar sections stay, labeled for
  classic setups. CLAUDE.md gets a short Quattro note (engine conflict +
  where the plugin lives).

## Work item B — Omarchy shell bar widget (PR 2, `feat/omarchy-shell-widget`)

A first-party Quickshell bar-widget plugin replacing the waybar module on
Quattro. Modeled on the shipped `omarchy.agents` plugin (manifest shape,
FileView pattern, qs.Commons styling).

### Layout (in-repo)

```
contrib/omarchy-plugin/nwg.notifications/
├── manifest.json      # schemaVersion 1, id "nwg.notifications",
│                      # kinds ["bar-widget"], entryPoints.barWidget: "Widget.qml"
├── Widget.qml         # the bar widget
└── README.md          # install/enable/click reference
```

### Widget behavior (mirrors the waybar module)

- Reads `nwg-notifications-status.json` from `$XDG_RUNTIME_DIR`
  (`Quickshell.env`), via `FileView { watchChanges: true }` — no
  polling, no signals. (`SIGRTMIN+11` keeps firing for waybar users;
  the widget simply doesn't need it.) If `XDG_RUNTIME_DIR` is unset the
  widget hides itself (the daemon's fallback dir is not worth chasing
  from QML; Omarchy always sets it).
- Renders the status file's `text` (bell glyph + unread count) — the
  daemon already composes the right glyph for empty/unread/DND states;
  the widget displays rather than recomputes. `tooltip` from the same
  file. Hidden (or dimmed bell) when the daemon isn't running / file
  absent.
- Clicks, exactly like the waybar config: left → toggle panel
  (`SIGRTMIN+4`), right → DND menu (`SIGRTMIN+6`), middle → DND toggle
  (`SIGRTMIN+5`). Implemented as
  `Quickshell.execDetached(["sh", "-c", "kill -s RTMIN+4 $(pidof nwg-notifications)"])`
  — `kill -s RTMIN+N` is glibc/musl-portable (the shell resolves the
  offset), and `pidof` avoids the pkill -f self-match class of bugs.
- Styling via the shell's `qs.Commons` theme so it matches the bar
  natively (reference: shipped widgets). Keep the widget dumb and small.

### Install / enable

- `make install-omarchy-plugin`: copies the plugin dir to
  `~/.config/omarchy/plugins/nwg.notifications/` (user scope, like
  install-dbus); runs `omarchy plugin validate` first when the
  `omarchy` CLI is present. `make uninstall-omarchy-plugin` mirrors it.
  Plain `make install` does NOT pull it in (Omarchy-specific; opt-in).
- Enable: `omarchy plugin enable nwg.notifications --section right`
  (documented, not automated — shell.json is user config).
- Changelog: Added — Omarchy shell bar widget (waybar-module successor
  on Quattro).

### Back-compat

Pure addition: nothing existing changes. Waybar module, status file
format, and signals all stay as-is (the widget is a second consumer of
the same contract).

## Verification / rollout

- Gates per PR: `make lint`, `make test-integration`, plus unit tests
  for the A2 predicate. The widget has no cargo surface; its gates are
  `omarchy plugin validate` + live behavior.
- **Live deployment on this machine (after both PRs):**
  1. `make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin` +
     `make install-omarchy-plugin`.
  2. `omarchy plugin disable omarchy.notifications` — verify our daemon
     takes the freedesktop name without restart (`gdbus GetNameOwner` →
     our pid); if not, restart via `make upgrade` and note it in docs.
  3. `omarchy plugin enable nwg.notifications --section right` — bell
     appears; `notify-send` round-trip; click/DND checks; count updates.
  4. Jason soak-tests for 1–2 days before any release/tag/publish
     (release ritual applies after soak, not at merge).
- Release versioning note (for later): Added feature + behavior fix →
  0.7.0, matching dock's parallel 0.7.0.

## Out of scope

- Changing the shell engine's own config/state (`~/.local/state/omarchy/`).
- Migrating history between the Omarchy engine and ours.
- Hyprland-version detection for the A2 gate (documented trade-off).
- Automating `omarchy plugin disable/enable` in make targets (user
  config; documented commands instead).
