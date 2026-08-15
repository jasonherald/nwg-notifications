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
