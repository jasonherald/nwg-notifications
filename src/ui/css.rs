//! CSS loader. Reads the embedded default stylesheet at startup
//! and installs it on the default `gdk::Display` so every GTK
//! widget in the daemon picks it up. Hot reload comes from
//! `nwg_common::config::css` watching the user override path.

use nwg_common::config::css;

/// Embedded notification CSS, loaded at compile time.
const NOTIFICATION_CSS: &str = include_str!("../assets/notifications.css");

/// Loads the notification CSS styling.
pub(crate) fn load_notification_css() {
    css::load_css_from_data(NOTIFICATION_CSS);
}
