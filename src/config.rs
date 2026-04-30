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

    /// Push the values of any *also-passed* flags to the running daemon
    /// over D-Bus, then exit. Used by nwg-shell-config and shell scripts
    /// to update live config without restarting the daemon. Only flags
    /// that are inherently runtime-mutable (popup-position, popup-width,
    /// panel-width, popup-timeout, max-popups, max-history) take effect;
    /// startup-only flags (--persist, --wm, --debug) are silently ignored
    /// in this mode.
    #[arg(long)]
    pub update: bool,
}

/// The set of clap arg IDs that are inherently live-updatable. Flags
/// outside this set (e.g. `--persist`, `--wm`) are skipped in `--update`
/// mode regardless of whether the user passed them, because pushing them
/// to a running daemon is meaningless or unsafe.
pub const LIVE_UPDATABLE_ARGS: &[&str] = &[
    "popup_position",
    "popup_width",
    "panel_width",
    "popup_timeout",
    "max_popups",
    "max_history",
];

/// Returns the subset of `LIVE_UPDATABLE_ARGS` whose value source on the
/// given matches is `CommandLine` (i.e., the user explicitly passed them
/// rather than relying on a default). Used by `--update` mode to push
/// only what the user asked to change, so e.g. `--update --popup-position
/// top-center` doesn't reset every other knob to its default.
pub fn user_set_live_args(matches: &clap::ArgMatches) -> Vec<&'static str> {
    LIVE_UPDATABLE_ARGS
        .iter()
        .filter(|name| {
            matches!(
                matches.value_source(name),
                Some(clap::parser::ValueSource::CommandLine)
            )
        })
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

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
    fn update_flag_defaults_false() {
        let config = NotificationConfig::parse_from(["test"]);
        assert!(!config.update);
    }

    #[test]
    fn update_flag_set() {
        let config = NotificationConfig::parse_from(["test", "--update", "--popup-width", "500"]);
        assert!(config.update);
        assert_eq!(config.popup_width, 500);
    }

    #[test]
    fn user_set_live_args_empty_when_only_defaults() {
        let matches = NotificationConfig::command()
            .try_get_matches_from(["test"])
            .expect("parse default");
        assert!(user_set_live_args(&matches).is_empty());
    }

    #[test]
    fn user_set_live_args_returns_only_explicit_flags() {
        let matches = NotificationConfig::command()
            .try_get_matches_from([
                "test",
                "--update",
                "--popup-position",
                "top-center",
                "--popup-width",
                "600",
            ])
            .expect("parse with two flags");
        let set = user_set_live_args(&matches);
        assert!(set.contains(&"popup_position"));
        assert!(set.contains(&"popup_width"));
        assert_eq!(set.len(), 2, "got {:?}", set);
    }

    #[test]
    fn user_set_live_args_ignores_startup_only_flags() {
        let matches = NotificationConfig::command()
            .try_get_matches_from(["test", "--update", "--persist", "--debug"])
            .expect("parse with startup-only flags");
        // --persist and --debug aren't in LIVE_UPDATABLE_ARGS, so the result is empty.
        assert!(user_set_live_args(&matches).is_empty());
    }

    #[test]
    fn live_updatable_args_contains_expected_six() {
        assert_eq!(LIVE_UPDATABLE_ARGS.len(), 6);
        for name in [
            "popup_position",
            "popup_width",
            "panel_width",
            "popup_timeout",
            "max_popups",
            "max_history",
        ] {
            assert!(
                LIVE_UPDATABLE_ARGS.contains(&name),
                "{name} missing from LIVE_UPDATABLE_ARGS"
            );
        }
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
