use super::constants::{PANEL_BODY_CHARS, PANEL_BODY_LINES, PANEL_ICON_SIZE, PANEL_SUMMARY_CHARS};
use crate::notification::Notification;
use gtk4::prelude::*;
use std::path::PathBuf;
use std::time::SystemTime;

/// Builds a single notification row for the history panel.
pub(crate) fn build_row(
    notif: &Notification,
    app_dirs: &[PathBuf],
    on_click: impl Fn(u32) + 'static,
    on_dismiss: impl Fn(u32) + 'static,
) -> gtk4::Box {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    row.add_css_class("notification-row");
    if !notif.read {
        row.add_css_class("unread");
    }
    row.set_margin_start(4);
    row.set_margin_end(4);
    row.set_margin_top(2);
    row.set_margin_bottom(2);

    // Unread indicator dot
    let dot = gtk4::Label::new(Some("●"));
    dot.add_css_class("unread-dot");
    dot.set_valign(gtk4::Align::Center);
    if notif.read {
        dot.set_opacity(0.0);
    }
    row.append(&dot);

    // Clickable area: icon + text (separate from dismiss button to avoid propagation)
    let clickable = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    clickable.set_hexpand(true);

    // App icon (small for panel)
    let icon = super::icons::resolve_theme_icon(
        &notif.app_icon,
        &notif.app_name,
        app_dirs,
        PANEL_ICON_SIZE,
    );
    icon.set_valign(gtk4::Align::Start);
    icon.set_margin_top(4);
    clickable.append(&icon);

    // Text column
    let text_box = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
    text_box.set_hexpand(true);

    // Summary + time on same line
    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    let summary = gtk4::Label::new(Some(&notif.summary));
    summary.add_css_class("row-summary");
    summary.set_halign(gtk4::Align::Start);
    summary.set_hexpand(true);
    summary.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    summary.set_max_width_chars(PANEL_SUMMARY_CHARS);
    header.append(&summary);

    let time_str = relative_time(notif.timestamp);
    let time_label = gtk4::Label::new(Some(&time_str));
    time_label.add_css_class("row-time");
    header.append(&time_label);
    text_box.append(&header);

    // Body
    if !notif.body.is_empty() {
        let body = gtk4::Label::new(Some(&notif.body));
        body.add_css_class("row-body");
        body.set_halign(gtk4::Align::Start);
        body.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        body.set_max_width_chars(PANEL_BODY_CHARS);
        body.set_lines(PANEL_BODY_LINES);
        body.set_wrap(true);
        text_box.append(&body);
    }

    clickable.append(&text_box);

    // Click icon/text area → focus app
    let click_id = notif.id;
    let gesture = gtk4::GestureClick::new();
    gesture.connect_released(move |gesture, _, _, _| {
        gesture.set_state(gtk4::EventSequenceState::Claimed);
        on_click(click_id);
    });
    clickable.add_controller(gesture);

    row.append(&clickable);

    // Dismiss button (outside clickable area — no event propagation conflict)
    let dismiss_btn = gtk4::Button::from_icon_name("window-close-symbolic");
    dismiss_btn.add_css_class("row-dismiss");
    dismiss_btn.set_valign(gtk4::Align::Start);
    dismiss_btn.set_margin_top(4);
    let dismiss_id = notif.id;
    dismiss_btn.connect_clicked(move |_| on_dismiss(dismiss_id));
    row.append(&dismiss_btn);

    row
}

/// Pure helper: formats an elapsed `Duration` as the relative-time
/// string shown in the panel ("now" / "Nm" / "Nh" / "Nd"). Split out
/// so tests can pass exact `Duration` values rather than fight the
/// wall-clock via `SystemTime::now()`.
fn relative_time_from_elapsed(elapsed: std::time::Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        "now".into()
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

fn relative_time(timestamp: SystemTime) -> String {
    relative_time_from_elapsed(timestamp.elapsed().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn now_branch_under_60s() {
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(0)), "now");
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(1)), "now");
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(59)), "now");
    }

    #[test]
    fn minutes_branch_60s_to_under_3600s() {
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(60)), "1m");
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(150)), "2m");
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(3599)), "59m");
    }

    #[test]
    fn hours_branch_3600s_to_under_86400s() {
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(3600)), "1h");
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(7200)), "2h");
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(86399)),
            "23h"
        );
    }

    #[test]
    fn days_branch_at_or_above_86400s() {
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(86400)), "1d");
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(172800)),
            "2d"
        );
        // Arbitrary large value — confirms no overflow surprise.
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(86400 * 365)),
            "365d"
        );
    }

    #[test]
    fn boundary_60s_transitions_now_to_1m() {
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(59)), "now");
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(60)), "1m");
    }

    #[test]
    fn boundary_3600s_transitions_minutes_to_1h() {
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(3599)), "59m");
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(3600)), "1h");
    }

    #[test]
    fn boundary_86400s_transitions_hours_to_1d() {
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(86399)),
            "23h"
        );
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(86400)), "1d");
    }
}
