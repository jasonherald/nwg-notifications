use serde::Serialize;
use std::path::PathBuf;

/// Offset above `SIGRTMIN` that waybar's notification module listens on.
/// Kept as a single named constant so the implementation and its test
/// can't drift apart.
const WAYBAR_REFRESH_SIGNAL_OFFSET: i32 = 11;

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

/// Returns the path to the waybar status file.
fn status_path() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join("mac-notifications-status.json")
}

/// Writes the waybar status file and signals waybar to refresh.
pub fn update_status(unread: usize, dnd: bool) {
    let status = if dnd {
        WaybarStatus {
            text: "\u{f06d9}".into(), // 󰛙 bell-off
            tooltip: "Do Not Disturb".into(),
            alt: "dnd".into(),
            class: "dnd".into(),
            count: unread,
        }
    } else if unread > 0 {
        WaybarStatus {
            text: format!("\u{f009a} {unread}"), // 󰂚 bell-badge + count
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
            text: "\u{f009c}".into(), // 󰂜 bell-outline
            tooltip: "No notifications".into(),
            alt: "empty".into(),
            class: "empty".into(),
            count: 0,
        }
    };

    let path = status_path();
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
