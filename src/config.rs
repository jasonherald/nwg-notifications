use crate::ui::constants::{
    PANEL_WIDTH_DEFAULT, PANEL_WIDTH_MAX, PANEL_WIDTH_MIN, POPUP_WIDTH_DEFAULT, POPUP_WIDTH_MAX,
    POPUP_WIDTH_MIN,
};
use clap::{Parser, ValueEnum};

/// Popup display position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PopupPosition {
    TopRight,
    TopCenter,
    TopLeft,
    BottomRight,
    BottomCenter,
    BottomLeft,
}

/// A macOS-style notification daemon for Hyprland/Sway.
#[derive(Parser, Debug, Clone)]
#[command(name = "nwg-notifications", version, about)]
pub struct NotificationConfig {
    /// Popup display position
    #[arg(long, value_enum, default_value_t = PopupPosition::TopRight)]
    pub popup_position: PopupPosition,

    /// Default popup timeout in ms (macOS uses ~7 seconds)
    #[arg(long, default_value_t = 7000)]
    pub popup_timeout: u64,

    /// Popup window width in pixels. Must be within
    /// `POPUP_WIDTH_MIN..=POPUP_WIDTH_MAX`; out-of-range values are rejected
    /// at parse time.
    #[arg(
        long,
        value_parser = clap::value_parser!(i32).range((POPUP_WIDTH_MIN as i64)..=(POPUP_WIDTH_MAX as i64)),
        default_value_t = POPUP_WIDTH_DEFAULT,
    )]
    pub popup_width: i32,

    /// History panel width in pixels. Must be within
    /// `PANEL_WIDTH_MIN..=PANEL_WIDTH_MAX`; out-of-range values are rejected
    /// at parse time.
    #[arg(
        long,
        value_parser = clap::value_parser!(i32).range((PANEL_WIDTH_MIN as i64)..=(PANEL_WIDTH_MAX as i64)),
        default_value_t = PANEL_WIDTH_DEFAULT,
    )]
    pub panel_width: i32,

    /// Maximum simultaneous popups
    #[arg(long, default_value_t = 5)]
    pub max_popups: usize,

    /// Maximum history entries to retain
    #[arg(long, default_value_t = 200)]
    pub max_history: usize,

    /// Start in Do Not Disturb mode
    #[arg(long)]
    pub dnd: bool,

    /// Persist notification history across restarts
    #[arg(long)]
    pub persist: bool,

    /// Turn on debug messages
    #[arg(long)]
    pub debug: bool,

    /// Window manager override (auto-detected from environment if not specified)
    #[arg(long, value_enum)]
    pub wm: Option<nwg_common::compositor::WmOverride>,

    /// Print the current pending notification count and exit.
    /// Queries the running daemon over D-Bus; does not auto-start one if none is running.
    #[arg(long)]
    pub count: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let config = NotificationConfig::parse_from(["test"]);
        assert_eq!(config.popup_position, PopupPosition::TopRight);
        assert_eq!(config.popup_timeout, 7000);
        assert_eq!(config.max_history, 200);
        assert!(!config.dnd);
    }

    #[test]
    fn dnd_flag() {
        let config = NotificationConfig::parse_from(["test", "--dnd"]);
        assert!(config.dnd);
    }

    #[test]
    fn wm_flag_hyprland() {
        let config = NotificationConfig::parse_from(["test", "--wm", "hyprland"]);
        assert_eq!(
            config.wm,
            Some(nwg_common::compositor::WmOverride::Hyprland)
        );
    }

    #[test]
    fn wm_flag_uwsm() {
        let config = NotificationConfig::parse_from(["test", "--wm", "uwsm"]);
        assert_eq!(config.wm, Some(nwg_common::compositor::WmOverride::Uwsm));
    }

    #[test]
    fn wm_flag_default_none() {
        let config = NotificationConfig::parse_from(["test"]);
        assert_eq!(config.wm, None);
    }

    #[test]
    fn popup_position_top_center() {
        let config = NotificationConfig::parse_from(["test", "--popup-position", "top-center"]);
        assert_eq!(config.popup_position, PopupPosition::TopCenter);
    }

    #[test]
    fn popup_position_bottom_center() {
        let config = NotificationConfig::parse_from(["test", "--popup-position", "bottom-center"]);
        assert_eq!(config.popup_position, PopupPosition::BottomCenter);
    }

    #[test]
    fn count_flag_defaults_false() {
        let config = NotificationConfig::parse_from(["test"]);
        assert!(!config.count);
    }

    #[test]
    fn count_flag_set() {
        let config = NotificationConfig::parse_from(["test", "--count"]);
        assert!(config.count);
    }

    #[test]
    fn popup_width_defaults_to_constant() {
        let config = NotificationConfig::parse_from(["test"]);
        assert_eq!(
            config.popup_width,
            crate::ui::constants::POPUP_WIDTH_DEFAULT
        );
    }

    #[test]
    fn popup_width_accepts_mid_range_value() {
        let mid =
            (crate::ui::constants::POPUP_WIDTH_MIN + crate::ui::constants::POPUP_WIDTH_MAX) / 2;
        let config = NotificationConfig::parse_from(["test", "--popup-width", &mid.to_string()]);
        assert_eq!(config.popup_width, mid);
    }

    #[test]
    fn popup_width_accepts_inclusive_minimum() {
        let min = crate::ui::constants::POPUP_WIDTH_MIN;
        let config = NotificationConfig::parse_from(["test", "--popup-width", &min.to_string()]);
        assert_eq!(config.popup_width, min);
    }

    #[test]
    fn popup_width_accepts_inclusive_maximum() {
        let max = crate::ui::constants::POPUP_WIDTH_MAX;
        let config = NotificationConfig::parse_from(["test", "--popup-width", &max.to_string()]);
        assert_eq!(config.popup_width, max);
    }

    #[test]
    fn popup_width_rejects_below_minimum() {
        let below = (crate::ui::constants::POPUP_WIDTH_MIN - 1).to_string();
        let result = NotificationConfig::try_parse_from(["test", "--popup-width", &below]);
        assert!(
            result.is_err(),
            "expected --popup-width={below} to be rejected"
        );
    }

    #[test]
    fn popup_width_rejects_above_maximum() {
        let above = (crate::ui::constants::POPUP_WIDTH_MAX + 1).to_string();
        let result = NotificationConfig::try_parse_from(["test", "--popup-width", &above]);
        assert!(
            result.is_err(),
            "expected --popup-width={above} to be rejected"
        );
    }

    #[test]
    fn panel_width_defaults_to_constant() {
        let config = NotificationConfig::parse_from(["test"]);
        assert_eq!(
            config.panel_width,
            crate::ui::constants::PANEL_WIDTH_DEFAULT
        );
    }

    #[test]
    fn panel_width_accepts_mid_range_value() {
        let mid =
            (crate::ui::constants::PANEL_WIDTH_MIN + crate::ui::constants::PANEL_WIDTH_MAX) / 2;
        let config = NotificationConfig::parse_from(["test", "--panel-width", &mid.to_string()]);
        assert_eq!(config.panel_width, mid);
    }

    #[test]
    fn panel_width_accepts_inclusive_minimum() {
        let min = crate::ui::constants::PANEL_WIDTH_MIN;
        let config = NotificationConfig::parse_from(["test", "--panel-width", &min.to_string()]);
        assert_eq!(config.panel_width, min);
    }

    #[test]
    fn panel_width_accepts_inclusive_maximum() {
        let max = crate::ui::constants::PANEL_WIDTH_MAX;
        let config = NotificationConfig::parse_from(["test", "--panel-width", &max.to_string()]);
        assert_eq!(config.panel_width, max);
    }

    #[test]
    fn panel_width_rejects_below_minimum() {
        let below = (crate::ui::constants::PANEL_WIDTH_MIN - 1).to_string();
        let result = NotificationConfig::try_parse_from(["test", "--panel-width", &below]);
        assert!(
            result.is_err(),
            "expected --panel-width={below} to be rejected"
        );
    }

    #[test]
    fn panel_width_rejects_above_maximum() {
        let above = (crate::ui::constants::PANEL_WIDTH_MAX + 1).to_string();
        let result = NotificationConfig::try_parse_from(["test", "--panel-width", &above]);
        assert!(
            result.is_err(),
            "expected --panel-width={above} to be rejected"
        );
    }
}
