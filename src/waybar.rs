//! Waybar notification-module integration.
//!
//! Writes a small JSON status file at `$XDG_RUNTIME_DIR/mac-notifications-status.json`
//! and signals waybar (`SIGRTMIN+11`) to refresh after every state change.
//!
//! The icon glyphs in the status text come from the [Material Design Icons]
//! range of [Nerd Fonts] (Private Use Area `f0xxx`). Waybar must be running
//! a font that includes these glyphs (e.g. `JetBrainsMono Nerd Font`,
//! `Symbols Nerd Font`) or the icons render as `tofu` boxes.
//!
//! [Material Design Icons]: https://pictogrammers.com/library/mdi/
//! [Nerd Fonts]: https://www.nerdfonts.com/

use serde::Serialize;

/// Offset above `SIGRTMIN` that waybar's notification module listens on.
/// Kept as a single named constant so the implementation and its test
/// can't drift apart.
const WAYBAR_REFRESH_SIGNAL_OFFSET: i32 = 11;

/// Material Design Icons (nerd-font) glyph used in the waybar status text
/// when Do-Not-Disturb is active. `\u{f06d9}` → 󰛙 `bell-off`.
const ICON_BELL_OFF: &str = "\u{f06d9}";

/// Material Design Icons (nerd-font) glyph used in the waybar status text
/// when there are unread notifications. `\u{f009a}` → 󰂚 `bell-badge`.
/// Rendered with the unread count as a trailing decimal.
const ICON_BELL_BADGE: &str = "\u{f009a}";

/// Material Design Icons (nerd-font) glyph used in the waybar status text
/// when the inbox is empty (no unread, no DND). `\u{f009c}` → 󰂜
/// `bell-outline`.
const ICON_BELL_OUTLINE: &str = "\u{f009c}";

/// Returns the runtime signal number for the waybar refresh signal
/// (SIGRTMIN+11). Computed from `libc::SIGRTMIN()` rather than hardcoded
/// because the value differs across libc implementations: glibc reserves
/// the first two RT signals (so `SIGRTMIN` = 34, hence SIGRTMIN+11 = 45),
/// while musl reserves three (so `SIGRTMIN` = 35, hence SIGRTMIN+11 = 46).
/// The nwg-common crate uses the same `libc::SIGRTMIN()` lookup
/// internally; we duplicate the call here rather than depend on a
/// (currently private) helper there. See #33.
fn waybar_refresh_signal() -> i32 {
    libc::SIGRTMIN() + WAYBAR_REFRESH_SIGNAL_OFFSET
}

#[derive(Serialize)]
struct WaybarStatus {
    text: String,
    tooltip: String,
    alt: String,
    class: String,
    count: usize,
}

/// Pure helper: builds the waybar status payload for the current
/// daemon state. Split out from `update_status` so the four-way
/// shape can be tested without going through disk I/O.
fn build_status(unread: usize, dnd: bool) -> WaybarStatus {
    if dnd {
        WaybarStatus {
            text: ICON_BELL_OFF.into(),
            tooltip: "Do Not Disturb".into(),
            alt: "dnd".into(),
            class: "dnd".into(),
            count: unread,
        }
    } else if unread > 0 {
        WaybarStatus {
            text: format!("{ICON_BELL_BADGE} {unread}"),
            tooltip: format!(
                "{unread} unread notification{}",
                if unread == 1 { "" } else { "s" }
            ),
            alt: "unread".into(),
            class: "unread".into(),
            count: unread,
        }
    } else {
        WaybarStatus {
            text: ICON_BELL_OUTLINE.into(),
            tooltip: "No notifications".into(),
            alt: "empty".into(),
            class: "empty".into(),
            count: 0,
        }
    }
}

/// Writes the waybar status file and signals waybar to refresh.
pub(crate) fn update_status(unread: usize, dnd: bool) {
    let status = build_status(unread, dnd);

    let path = crate::paths::status_path();
    match serde_json::to_string(&status) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::error!("Failed to write waybar status: {}", e);
            }
        }
        Err(e) => log::error!("Failed to serialize waybar status: {}", e),
    }

    signal_waybar();
}

/// Sends SIGRTMIN+11 to waybar to refresh the notification module.
fn signal_waybar() {
    let signal_num = waybar_refresh_signal();
    match std::process::Command::new("pkill")
        .arg(format!("-{signal_num}"))
        .arg("waybar")
        .status()
    {
        Err(e) => log::debug!("Failed to signal waybar: {e}"),
        Ok(s) if !s.success() => log::debug!("No waybar process to signal"),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_json_includes_count_field() {
        let status = WaybarStatus {
            text: "x".into(),
            tooltip: "t".into(),
            alt: "a".into(),
            class: "c".into(),
            count: 7,
        };
        let json = serde_json::to_string(&status).expect("serialize");
        assert!(
            json.contains("\"count\":7"),
            "expected count field in JSON, got: {json}"
        );
    }

    #[test]
    fn build_status_dnd_branch_uses_bell_off_glyph() {
        // DND wins over unread count: even with 5 unread notifications,
        // the dnd flag forces the dnd glyph + class. The count field
        // is preserved in the JSON so consumers can still surface the
        // backlog count next to the bell-off glyph if they want.
        let s = build_status(5, true);
        assert_eq!(s.text, ICON_BELL_OFF);
        assert_eq!(s.tooltip, "Do Not Disturb");
        assert_eq!(s.alt, "dnd");
        assert_eq!(s.class, "dnd");
        assert_eq!(s.count, 5);
    }

    #[test]
    fn build_status_singular_unread_uses_singular_tooltip() {
        let s = build_status(1, false);
        assert_eq!(s.text, format!("{ICON_BELL_BADGE} 1"));
        assert_eq!(s.tooltip, "1 unread notification");
        assert_eq!(s.alt, "unread");
        assert_eq!(s.class, "unread");
        assert_eq!(s.count, 1);
    }

    #[test]
    fn build_status_plural_unread_uses_plural_tooltip() {
        let s = build_status(5, false);
        assert_eq!(s.text, format!("{ICON_BELL_BADGE} 5"));
        assert_eq!(s.tooltip, "5 unread notifications");
        assert_eq!(s.alt, "unread");
        assert_eq!(s.class, "unread");
        assert_eq!(s.count, 5);
    }

    #[test]
    fn build_status_empty_branch_uses_bell_outline() {
        let s = build_status(0, false);
        assert_eq!(s.text, ICON_BELL_OUTLINE);
        assert_eq!(s.tooltip, "No notifications");
        assert_eq!(s.alt, "empty");
        assert_eq!(s.class, "empty");
        assert_eq!(s.count, 0);
    }

    #[test]
    fn waybar_refresh_signal_is_sigrtmin_plus_offset() {
        let s = waybar_refresh_signal();
        let base = libc::SIGRTMIN();
        assert_eq!(
            s,
            base + WAYBAR_REFRESH_SIGNAL_OFFSET,
            "expected SIGRTMIN({base}) + {WAYBAR_REFRESH_SIGNAL_OFFSET}, got {s}"
        );
        // Cross-check that the value is in the RT-signal range. SIGRTMIN
        // is at minimum 33 on Linux; SIGRTMAX is at most 64. SIGRTMIN+11
        // must fit comfortably below SIGRTMAX even on the more
        // restrictive musl layout.
        assert!(s < libc::SIGRTMAX(), "signal {s} exceeds SIGRTMAX");
    }
}
