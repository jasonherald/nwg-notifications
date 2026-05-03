# Changelog

All notable changes to `nwg-notifications` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> **Pre-split note:** Prior to v0.3.0, this crate lived inside the
> [`mac-doc-hyprland`](https://github.com/jasonherald/mac-doc-hyprland) monorepo
> at `crates/nwg-notifications/`. v0.3.0 is the first release in its own repo.
> The full pre-split history is preserved in the monorepo's git log; this
> file only documents changes from v0.3.0 onward.

## [0.3.5] — Unreleased

### Fixed

- Four post-v0.3.4 correctness bugs surfaced by the comprehensive
  code-quality review (epic #29):
  - `--update --max-history N` now actually changes the trim cap
    instead of waiting for daemon restart. `trim_history()` reads
    `max_history` from the live config rather than a state-side copy
    seeded once at startup. (#30)
  - Re-clicking a different timed-DND duration before the first
    timer fires no longer leaves the older one armed and clearing
    DND early. Each scheduled timer now captures its expiry as a
    token and no-ops silently if the live expiry has been replaced
    by a newer schedule. (#31)
  - `org.freedesktop.Notifications.GetServerInformation` now returns
    the real vendor (`nwg-notifications`) and version (from
    `CARGO_PKG_VERSION`). Previously reported vendor
    `nwg-dock-hyprland` and version `0.1.0` — both pre-split
    monorepo leftovers visible to any client app or notification
    debugger. (#32)
  - Waybar refresh signal is now computed from `libc::SIGRTMIN()` at
    runtime instead of hardcoded to 45 (which was wrong on musl,
    where `SIGRTMIN+11 = 46`). musl users were silently sending the
    wrong signal to waybar. (#33)

## [0.3.4] — 2026-05-03

### Fixed

- `--update` now prints an actionable error when it calls a D-Bus
  method the running daemon doesn't recognise — typically because the
  daemon is from a release older than the CLI. Previously the raw
  `GDBus.Error:org.freedesktop.DBus.Error.UnknownMethod` text bubbled
  to the user, which didn't hint at the restart-after-upgrade fix.
  Other error classes (no daemon, timeout, payload type) keep their
  existing format. (#25)

## [0.3.3] — 2026-04-29

### Fixed

- Opening the notification panel now closes any visible popup toasts
  instead of leaving them on screen alongside the slide-out. Popups
  were redundant once the panel showed the same notifications, and
  overlapping popups on the panel's edge looked tacky. Closing the
  popups on panel-open is purely a UI dedup — it doesn't mark them
  read or touch history, so a user who hadn't yet clicked a popup can
  still see and act on it from inside the panel. (#3)

## [0.3.2] — 2026-04-29

Adds a live config update mechanism so consumers like `nwg-shell-config` can change runtime settings without restarting the daemon.

### Added

- Live config updates (#20). Six new D-Bus methods on
  `org.nwg.Notifications` let consumers like `nwg-shell-config` push
  runtime config changes without restarting the daemon:
  `SetPopupPosition`, `SetPopupWidth`, `SetPanelWidth`,
  `SetPopupTimeout`, `SetMaxPopups`, `SetMaxHistory`. Each setter
  validates against the same ranges as the matching CLI flag and
  returns `org.freedesktop.DBus.Error.InvalidArgs` on bad input.
- `nwg-notifications --update <flags>` CLI subcommand wraps the
  D-Bus setters as a thin client (mirrors the existing `--count`
  pattern). Uses `clap::ArgMatches::value_source` to push only flags
  the user explicitly passed, so `--update --popup-position
  top-center` doesn't reset other knobs to their defaults. (#20)

## [0.3.1] — 2026-04-28

Closes the [nwg-shell-config integration epic](https://github.com/jasonherald/nwg-notifications/issues/8) on the daemon side: adds the flags and IPC surface that `nwg-shell-config` needs to drive `nwg-notifications` directly (replacing swaync), plus a small D-Bus protocol fix surfaced during review.

### Added

- `--popup-position` accepts `top-center` and `bottom-center` in addition to
  the existing four corners. Centered placements anchor only the top or
  bottom edge; gtk4-layer-shell centers the surface horizontally on the
  unanchored axis. (#10)
- Pending notification count IPC for nwg-panel and similar consumers (#9):
  - New `org.nwg.Notifications` D-Bus interface with `GetCount() -> u32`
    method and `CountChanged(u32)` signal (delta-only; emits when the count
    actually changes).
  - `count: usize` field added to the waybar status JSON at
    `$XDG_RUNTIME_DIR/mac-notifications-status.json`.
  - `nwg-notifications --count` CLI subcommand that queries the running
    daemon over D-Bus and prints the count to stdout (uses `NO_AUTO_START`,
    so it never spawns a daemon).
- `--popup-width <px>` flag controls popup window width. Defaults to 380px;
  range 100..=2000 enforced at parse time. Applied per-popup so every
  popup picks up the configured width, not just the first. (#11)
- `--panel-width <px>` flag controls history panel width. Defaults to 380px;
  range 200..=2000 enforced at parse time. (#12)

### Fixed

- `org.freedesktop.Notifications` D-Bus handler now returns the standard
  `org.freedesktop.DBus.Error.UnknownMethod` for unknown methods instead of
  silently logging, so introspection-driven clients see the error
  immediately instead of waiting out their reply timeout. Mirrors the fix
  applied to the new `org.nwg.Notifications` handler. (#15)

## [0.3.0] — 2026-04-21

First standalone release. Extracts the D-Bus notification daemon from
[`mac-doc-hyprland`](https://github.com/jasonherald/mac-doc-hyprland) as its
own repo + crates.io crate.

### Changed

- Dependency: `nwg-common` now consumed from crates.io at `"0.3"` rather than
  as a workspace path dependency.
- D-Bus service file now ships as a committed template
  (`data/org.freedesktop.Notifications.service.in`) that `make install-dbus`
  substitutes `@BIN_PATH@` in, rather than being generated via `echo` at
  install time. Easier to inspect and version-control.

### Added

- crates.io metadata (`description`, `readme`, `keywords`, `categories`,
  `repository`) wired up.
