# nwg-notifications

[![crates.io](https://img.shields.io/crates/v/nwg-notifications.svg)](https://crates.io/crates/nwg-notifications)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A D-Bus notification daemon and notification center for [Hyprland](https://hyprland.org/) and [Sway](https://swaywm.org/), written in Rust.

Claims `org.freedesktop.Notifications`, shows popup toasts, and ships a slide-out history panel with Do-Not-Disturb controls and optional waybar integration. Built alongside [`nwg-dock`](https://github.com/jasonherald/nwg-dock) and [`nwg-drawer`](https://github.com/jasonherald/nwg-drawer) to replace [mako](https://github.com/emersion/mako) in the mac-doc-hyprland stack, but runs standalone.

## Features

- **D-Bus notification daemon** — replaces mako; claims `org.freedesktop.Notifications`
- **Popup toasts** — top-right corner, auto-dismiss, click-to-focus sending app
- **Deep-linking** — clicking a notification tells the app to open the specific item
- **Auto-dismiss** — popups dismissed when app calls CloseNotification (e.g., Slack read)
- **Action buttons** — shows Reply/Open/etc. buttons, emits `ActionInvoked` D-Bus signal
- **History panel** — slide-out from right, grouped by app with collapse/expand
- **Click-outside-to-close** — backdrop overlay + Escape key
- **Dismiss controls** — per-notification, per-app group, or clear all
- **Do Not Disturb** — toggle via panel button, signal, or waybar right-click menu
- **Timed DND** — 1 hour, 2 hours, until tomorrow with expiry countdown
- **Waybar integration** — bell icon with unread count, left-click toggles panel, right-click opens DND menu
- **Persistence** — notification history saved across restarts with `--persist`
- **Focused monitor** — popups appear on the currently focused monitor

## Install

### Requirements

- **Rust 1.95** or later (pinned in `rust-toolchain.toml`; rustup picks it up automatically)
- **GTK4** and **gtk4-layer-shell** system libraries
- A Wayland compositor with `wlr-layer-shell` support (Hyprland, Sway)

### Install system dependencies

```bash
# Arch Linux
sudo pacman -S gtk4 gtk4-layer-shell

# Ubuntu/Debian
sudo apt install libgtk-4-dev libgtk4-layer-shell-dev

# Fedora
sudo dnf install gtk4-devel gtk4-layer-shell-devel
```

### From crates.io (recommended for end users)

```bash
cargo install nwg-notifications
```

Lands the binary at `~/.cargo/bin/nwg-notifications`. `cargo install` doesn't ship the D-Bus service file — you'll need to write that yourself (see [D-Bus service](#d-bus-service) below; it's a ~5-line file pointing at the installed binary). Once the service file is in place, the daemon auto-activates the first time any app calls `org.freedesktop.Notifications`.

**After upgrading**, restart any long-running daemon process so it picks up new D-Bus surface introduced by the upgrade. The CLI on `PATH` will be the new binary immediately, but the daemon process started by your session manager (or auto-activated by D-Bus before the upgrade) keeps running the old code until it exits. Quickest restart:

````bash
kill $(pidof nwg-notifications)
# Your session manager (or D-Bus auto-activation on the next notify-send)
# spawns the new binary. Or run `nwg-notifications --persist &` directly.
````

Without this, `--update` and `gdbus call` against newly-shipped methods fail with `org.freedesktop.DBus.Error.UnknownMethod`.

### `make install` — for source builds, distro packagers, and the `install-dbus` helper

The Makefile install path drops both the binary and the D-Bus service file (the latter always to user-scope, regardless of `PREFIX` — D-Bus user services are per-user by convention).

**Default — system-wide binary + user-scope service:**

```bash
sudo make install
make install-dbus
```

Writes:
- `nwg-notifications` → `/usr/local/bin/nwg-notifications`
- D-Bus service file → `~/.local/share/dbus-1/services/org.freedesktop.Notifications.service` (no sudo)

**No-sudo, dev workflow:**

```bash
make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
make install-dbus
```

**Distro-parity:**

```bash
sudo make install PREFIX=/usr
make install-dbus
```

## Usage

```bash
# With history persistence
nwg-notifications --persist

# Force Sway backend (usually auto-detected)
nwg-notifications --wm sway --persist
```

## D-Bus service

`make install-dbus` installs this file into `~/.local/share/dbus-1/services/`. If you're cargo-installing, create it manually:

```ini
# ~/.local/share/dbus-1/services/org.freedesktop.Notifications.service
[D-BUS Service]
Name=org.freedesktop.Notifications
Exec=/home/YOU/.cargo/bin/nwg-notifications --persist
```

Once registered, the daemon auto-starts the first time any app calls `org.freedesktop.Notifications`.

## Hyprland autostart

```ini
# ~/.config/hypr/autostart.conf
exec-once = uwsm-app -- nwg-notifications --persist
```

Autostart isn't strictly required thanks to D-Bus auto-activation, but it makes the daemon ready before the first notification arrives (avoids a few-hundred-millisecond delay on your first toast).

## Signal control

```bash
# Toggle notification panel
pkill -f -38 nwg-notifications     # SIGRTMIN+4

# Toggle DND
pkill -f -39 nwg-notifications     # SIGRTMIN+5

# Open DND duration menu
pkill -f -40 nwg-notifications     # SIGRTMIN+6
```

## Waybar integration

Add to `~/.config/waybar/config.jsonc`:

```jsonc
"custom/notifications": {
    "exec": "cat $XDG_RUNTIME_DIR/mac-notifications-status.json 2>/dev/null || echo '{\"text\":\"\",\"alt\":\"empty\",\"class\":\"empty\"}'",
    "return-type": "json",
    "format": "{}",
    "on-click": "pkill -f -38 nwg-notifications",
    "on-click-right": "pkill -f -40 nwg-notifications",
    "signal": 11,
    "interval": "once"
}
```

The daemon writes its current state to `$XDG_RUNTIME_DIR/mac-notifications-status.json` and signals waybar (`SIGRTMIN+11`, which waybar receives as `signal: 11`) whenever the state changes — no polling.

## Querying notification count

Three mechanisms expose the current pending (unread) count for status-bar
widgets, scripts, and external panels (e.g. nwg-panel):

### CLI

```bash
nwg-notifications --count
# Prints the integer count to stdout. Exits 1 with a stderr error if no
# daemon is running (NO_AUTO_START — won't spawn a daemon).
```

### D-Bus

```bash
gdbus call --session \
  --dest org.nwg.Notifications \
  --object-path /org/nwg/Notifications \
  --method org.nwg.Notifications.GetCount
```

For push-mode subscribers, listen on the `CountChanged` signal:

```bash
dbus-monitor --session "type='signal',interface='org.nwg.Notifications'"
```

The signal emits only when the count actually changes (delta-tracking),
so subscribers don't receive spurious wakeups for no-op state mutations.

### Status file

The waybar status JSON includes a `count` field — useful when you already
have `SIGRTMIN+11` wired up:

```bash
jq -r .count "$XDG_RUNTIME_DIR/mac-notifications-status.json"
```

## Live config updates

Six knobs take runtime updates without restarting the daemon. Two surfaces:

### CLI

```bash
# Push individual settings:
nwg-notifications --update --popup-position top-center
nwg-notifications --update --popup-width 600

# Push multiple in one call:
nwg-notifications --update --popup-position top-center --popup-width 600
```

`--update` short-circuits before daemon init and uses `NO_AUTO_START`, so it never spawns a daemon. Exits 1 with a useful error when no daemon is running. Only flags you explicitly pass are pushed — defaults are never sent.

### D-Bus

For tooling that prefers the D-Bus surface directly (e.g. `nwg-shell-config` from Python via `pydbus` / `gi.repository.Gio`):

```bash
gdbus call --session \
  --dest org.nwg.Notifications \
  --object-path /org/nwg/Notifications \
  --method org.nwg.Notifications.SetPopupPosition '"top-center"'

gdbus call --session \
  --dest org.nwg.Notifications \
  --object-path /org/nwg/Notifications \
  --method org.nwg.Notifications.SetPopupWidth 600
```

Each setter validates against the same ranges as the matching CLI flag and returns `org.freedesktop.DBus.Error.InvalidArgs` on bad input. The full set:

| Setter             | Type | Validation                                                                |
|--------------------|------|---------------------------------------------------------------------------|
| `SetPopupPosition` | `s`  | One of: `top-right`, `top-center`, `top-left`, `bottom-right`, `bottom-center`, `bottom-left` |
| `SetPopupWidth`    | `u`  | `100..=2000`                                                              |
| `SetPanelWidth`    | `u`  | `200..=2000`                                                              |
| `SetPopupTimeout`  | `u`  | Any uint32 (ms; `0` = never auto-dismiss)                                 |
| `SetMaxPopups`     | `u`  | `>= 1`                                                                    |
| `SetMaxHistory`    | `u`  | `>= 1`                                                                    |

### What can't be live-updated

`--persist`, `--wm`, and `--debug` are inherently startup-only — restart the daemon to change those.

## Theming

Styling is embedded via `include_str!`; there's no user-writable `notifications.css` today. If you need to customize appearance, fork the crate and edit `assets/notifications.css`, or open an issue to discuss exposing it.

## Contributing

PRs welcome. `main` is protected — open from a feature branch. Run `make lint` (fmt + clippy + test + deny + audit) locally before requesting review.

User-visible PRs add a CHANGELOG bullet under `## [x.y.z] — Unreleased` in `CHANGELOG.md`, following [Keep a Changelog](https://keepachangelog.com).

## Background: why not mako?

Mako is great, but:

1. The mac-doc-hyprland stack wanted a single look/feel across the dock, drawer, and notification center — GTK4 layer-shell surfaces make theming coherent across all three.
2. We wanted history + grouping + click-to-focus as first-class features, not add-ons.
3. Writing a D-Bus notification server in Rust on `gio::bus_own_name` turned out to be less code than expected — no async bridge, no external crate, directly on the glib main loop.

Run `nwg-notifications` instead of mako, or alongside (they'll race for the name — whichever claimed it first wins).

## License

MIT. See `LICENSE`.
