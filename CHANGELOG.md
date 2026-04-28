# Changelog

All notable changes to `nwg-notifications` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> **Pre-split note:** Prior to v0.3.0, this crate lived inside the
> [`mac-doc-hyprland`](https://github.com/jasonherald/mac-doc-hyprland) monorepo
> at `crates/nwg-notifications/`. v0.3.0 is the first release in its own repo.
> The full pre-split history is preserved in the monorepo's git log; this
> file only documents changes from v0.3.0 onward.

## [0.3.0] — Unreleased

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
