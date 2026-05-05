//! clap CLI definition (`NotificationConfig`, `PopupPosition` enum)
//! and the `value_source`-based filter that lets `--update` push
//! only the flags the user actually passed rather than reset the
//! rest to their defaults.

use crate::ui::constants::{
    PANEL_WIDTH_DEFAULT, PANEL_WIDTH_MAX, PANEL_WIDTH_MIN, POPUP_WIDTH_DEFAULT, POPUP_WIDTH_MAX,
    POPUP_WIDTH_MIN,
};
use clap::{Parser, ValueEnum};

/// Popup display position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PopupPosition {
    TopRight,
    TopCenter,
    TopLeft,
    BottomRight,
    BottomCenter,
    BottomLeft,
}

/// A macOS-style notification daemon for Hyprland/Sway.
#[derive(Parser, Debug, Clone, serde::Serialize, serde::Deserialize)]
#[command(name = "nwg-notifications", version, about)]
#[serde(default)]
pub(crate) struct NotificationConfig {
    /// Schema version for forward-compatibility. Set to `1` for the
    /// initial JSON format. Not exposed as a CLI flag (the value is
    /// only meaningful on the JSON side).
    #[arg(skip)]
    #[serde(default = "default_config_version")]
    pub(crate) version: u32,

    /// Popup display position
    #[arg(long, value_enum, default_value_t = PopupPosition::TopRight)]
    pub(crate) popup_position: PopupPosition,

    /// Default popup timeout in ms (macOS uses ~7 seconds)
    #[arg(long, default_value_t = 7000)]
    pub(crate) popup_timeout: u64,

    /// Popup window width in pixels. Must be within
    /// `POPUP_WIDTH_MIN..=POPUP_WIDTH_MAX`; out-of-range values are rejected
    /// at parse time.
    #[arg(
        long,
        value_parser = clap::value_parser!(i32).range((POPUP_WIDTH_MIN as i64)..=(POPUP_WIDTH_MAX as i64)),
        default_value_t = POPUP_WIDTH_DEFAULT,
    )]
    pub(crate) popup_width: i32,

    /// History panel width in pixels. Must be within
    /// `PANEL_WIDTH_MIN..=PANEL_WIDTH_MAX`; out-of-range values are rejected
    /// at parse time.
    #[arg(
        long,
        value_parser = clap::value_parser!(i32).range((PANEL_WIDTH_MIN as i64)..=(PANEL_WIDTH_MAX as i64)),
        default_value_t = PANEL_WIDTH_DEFAULT,
    )]
    pub(crate) panel_width: i32,

    /// Maximum simultaneous popups
    #[arg(long, default_value_t = 5)]
    pub(crate) max_popups: usize,

    /// Maximum history entries to retain
    #[arg(long, default_value_t = 200)]
    pub(crate) max_history: usize,

    /// Start in Do Not Disturb mode
    #[arg(long)]
    pub(crate) dnd: bool,

    /// Persist notification history across restarts
    #[arg(long)]
    pub(crate) persist: bool,

    /// Turn on debug messages
    #[arg(long)]
    #[serde(skip)]
    pub(crate) debug: bool,

    /// Window manager override (auto-detected from environment if not specified)
    #[arg(long, value_enum)]
    #[serde(skip)]
    pub(crate) wm: Option<nwg_common::compositor::WmOverride>,

    /// Print the current pending notification count and exit.
    /// Queries the running daemon over D-Bus; does not auto-start one if none is running.
    #[arg(long)]
    #[serde(skip)]
    pub(crate) count: bool,

    /// Push the values of any *also-passed* flags to the running daemon
    /// over D-Bus, then exit. Used by nwg-shell-config and shell scripts
    /// to update live config without restarting the daemon. Only flags
    /// that are inherently runtime-mutable (popup-position, popup-width,
    /// panel-width, popup-timeout, max-popups, max-history) take effect;
    /// startup-only flags (--persist, --wm, --debug) are silently ignored
    /// in this mode.
    #[arg(long)]
    #[serde(skip)]
    pub(crate) update: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        // Match clap's `default_value_t` annotations exactly. The
        // serde-deserialized "missing field" path uses these; the
        // CLI-parsed path uses clap's defaults. Both agree.
        Self {
            version: default_config_version(),
            popup_position: PopupPosition::TopRight,
            popup_timeout: 7000,
            popup_width: POPUP_WIDTH_DEFAULT,
            panel_width: PANEL_WIDTH_DEFAULT,
            max_popups: 5,
            max_history: 200,
            persist: false,
            dnd: false,
            debug: false,
            wm: None,
            count: false,
            update: false,
        }
    }
}

/// Default schema version when serde encounters a JSON file
/// without a `"version"` field. Treat as version 1 (the initial
/// schema). Single source of truth for the version constant —
/// `Default for NotificationConfig` reuses this so the serde-default
/// path and the first-run-write path agree by construction.
const fn default_config_version() -> u32 {
    1
}

/// Central field-name registry for the JSON config + D-Bus + CLI
/// merge plumbing. Three nested categories, each a strict superset of
/// the next (verified by `field_registry_subsets_are_consistent`):
///
/// - [`OVERRIDABLE_FIELDS`] (10) — every CLI flag the boot-time
///   merge tracks via `user_set_args`. Includes `debug` and `wm`,
///   which are CLI-only and never appear in the JSON.
/// - [`RELOADABLE_FIELDS`] (8) — `OVERRIDABLE_FIELDS` minus the two
///   CLI-only flags. The set of fields the inotify hot-reload path
///   (`apply_config_reload`) reapplies from disk.
/// - [`LIVE_UPDATABLE_ARGS`] (6) — `RELOADABLE_FIELDS` minus the two
///   startup-only flags (`persist`, `dnd`). The set of fields the
///   D-Bus `Set*` handlers can mutate at runtime, and the only fields
///   `--update` is allowed to push.
///
/// Adding a field to `NotificationConfig`: append the snake-cased
/// arg name to whichever lists apply, then update the per-field
/// ladders in `merge_cli_over_json` (uses `OVERRIDABLE_FIELDS`),
/// `apply_config_reload` (uses `RELOADABLE_FIELDS`), and the D-Bus
/// dispatcher (uses `LIVE_UPDATABLE_ARGS`). The
/// `overridable_fields_match_clap_arg_set` test catches a missing
/// `OVERRIDABLE_FIELDS` entry at CI time.
///
/// The set of clap arg IDs that are inherently live-updatable. Flags
/// outside this set (e.g. `--persist`, `--wm`) are skipped in `--update`
/// mode regardless of whether the user passed them, because pushing them
/// to a running daemon is meaningless or unsafe.
pub(crate) const LIVE_UPDATABLE_ARGS: &[&str] = &[
    "popup_position",
    "popup_width",
    "panel_width",
    "popup_timeout",
    "max_popups",
    "max_history",
];

/// JSON-visible config fields the inotify hot-reload path
/// (`apply_config_reload`) can re-apply from disk. Strict superset
/// of [`LIVE_UPDATABLE_ARGS`]: adds `persist` and `dnd`, which are
/// startup-only flags but legitimate JSON keys.
pub(crate) const RELOADABLE_FIELDS: &[&str] = &[
    "popup_position",
    "popup_width",
    "panel_width",
    "popup_timeout",
    "max_popups",
    "max_history",
    "persist",
    "dnd",
];

/// Every CLI flag the boot-time merge (`merge_cli_over_json`) and
/// `user_set_args` track. Strict superset of [`RELOADABLE_FIELDS`]:
/// adds `debug` and `wm`, which are CLI-only knobs and never appear
/// in the JSON.
pub(crate) const OVERRIDABLE_FIELDS: &[&str] = &[
    "popup_position",
    "popup_width",
    "panel_width",
    "popup_timeout",
    "max_popups",
    "max_history",
    "persist",
    "dnd",
    "debug",
    "wm",
];

/// Returns the subset of `LIVE_UPDATABLE_ARGS` whose value source on the
/// given matches is `CommandLine` (i.e., the user explicitly passed them
/// rather than relying on a default). Used by `--update` mode to push
/// only what the user asked to change, so e.g. `--update --popup-position
/// top-center` doesn't reset every other knob to its default.
pub(crate) fn user_set_live_args(matches: &clap::ArgMatches) -> Vec<&'static str> {
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

/// Returns the names of every CLI flag the user explicitly passed
/// on the command line (as opposed to clap's compiled defaults
/// kicking in). Used by the boot-time merge in main() to decide
/// which fields override the JSON config and which fall through to
/// it.
///
/// Distinct from `user_set_live_args` (which covers only the
/// `--update`-eligible subset for D-Bus push). This one is
/// boot-only and includes flags that aren't pushable at runtime
/// (e.g. `--debug`, `--wm`).
pub(crate) fn user_set_args(matches: &clap::ArgMatches) -> std::collections::HashSet<&'static str> {
    use clap::parser::ValueSource;
    OVERRIDABLE_FIELDS
        .iter()
        .filter(|name| matches.value_source(name) == Some(ValueSource::CommandLine))
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
        assert_eq!(set.len(), 2, "got {set:?}");
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

    #[test]
    fn empty_json_produces_struct_with_clap_defaults() {
        // serde(default) on the struct + Default impl that mirrors
        // clap's default_value_t means an empty JSON object should
        // produce a NotificationConfig identical to one parsed from
        // an empty CLI invocation.
        let from_json: NotificationConfig = serde_json::from_str("{}").expect("empty JSON parses");
        let from_cli = NotificationConfig::parse_from(["test"]);

        assert_eq!(from_json.popup_position, from_cli.popup_position);
        assert_eq!(from_json.popup_timeout, from_cli.popup_timeout);
        assert_eq!(from_json.popup_width, from_cli.popup_width);
        assert_eq!(from_json.panel_width, from_cli.panel_width);
        assert_eq!(from_json.max_popups, from_cli.max_popups);
        assert_eq!(from_json.max_history, from_cli.max_history);
        assert_eq!(from_json.persist, from_cli.persist);
        assert_eq!(from_json.dnd, from_cli.dnd);
    }

    #[test]
    fn popup_position_serializes_as_kebab_case() {
        // Matches the strings clap's ValueEnum accepts on the CLI
        // (e.g. `--popup-position top-right`).
        assert_eq!(
            serde_json::to_string(&PopupPosition::TopRight).unwrap(),
            "\"top-right\""
        );
        let parsed: PopupPosition = serde_json::from_str("\"bottom-center\"").unwrap();
        assert_eq!(parsed, PopupPosition::BottomCenter);
    }

    #[test]
    fn empty_json_defaults_version_to_1() {
        // serde(default = "default_config_version") on the version
        // field means a JSON without it (e.g., old v0.4.0 files
        // written before the field existed) parses as version 1.
        let config: NotificationConfig = serde_json::from_str("{}").expect("empty JSON parses");
        assert_eq!(config.version, 1);
    }

    #[test]
    fn cli_only_fields_are_skipped_in_json() {
        // debug / wm / count / update should serialize to nothing
        // (the JSON should not have keys for them).
        let config = NotificationConfig {
            debug: true,
            count: true,
            ..NotificationConfig::default()
        };
        let json = serde_json::to_string(&config).expect("serialize");
        assert!(
            !json.contains("debug"),
            "debug must not appear in JSON; got: {json}"
        );
        assert!(
            !json.contains("\"wm\""),
            "wm must not appear in JSON; got: {json}"
        );
        assert!(
            !json.contains("count"),
            "count must not appear in JSON; got: {json}"
        );
        assert!(
            !json.contains("update"),
            "update must not appear in JSON; got: {json}"
        );
    }

    #[test]
    fn field_registry_subsets_are_consistent() {
        // The three lists are nested supersets:
        //   LIVE_UPDATABLE_ARGS ⊂ RELOADABLE_FIELDS ⊂ OVERRIDABLE_FIELDS
        // Each ⊂ relationship is a real semantic claim — the boot-time
        // merge's set of tracked fields is a superset of the
        // hot-reload path's set, which is a superset of the D-Bus
        // Set*-able set. Drift breaks the JSON-config contract.
        let live: std::collections::HashSet<&str> = LIVE_UPDATABLE_ARGS.iter().copied().collect();
        let reloadable: std::collections::HashSet<&str> =
            RELOADABLE_FIELDS.iter().copied().collect();
        let overridable: std::collections::HashSet<&str> =
            OVERRIDABLE_FIELDS.iter().copied().collect();

        for field in &live {
            assert!(
                reloadable.contains(field),
                "{field} is in LIVE_UPDATABLE_ARGS but missing from RELOADABLE_FIELDS"
            );
        }
        for field in &reloadable {
            assert!(
                overridable.contains(field),
                "{field} is in RELOADABLE_FIELDS but missing from OVERRIDABLE_FIELDS"
            );
        }

        // The CLI-only fields are exactly the difference between
        // OVERRIDABLE and RELOADABLE (debug, wm). If a future spec
        // change moves one of these into the JSON, both lists need
        // to grow in sync.
        let cli_only: std::collections::HashSet<&str> =
            overridable.difference(&reloadable).copied().collect();
        let expected_cli_only: std::collections::HashSet<&str> =
            ["debug", "wm"].iter().copied().collect();
        assert_eq!(
            cli_only, expected_cli_only,
            "OVERRIDABLE - RELOADABLE should be exactly {{debug, wm}}"
        );

        // Same shape: the difference between RELOADABLE and LIVE is
        // the startup-only flags (persist, dnd). Adding a `SetPersist`
        // or `SetDnd` D-Bus method later would shift these.
        let startup_only: std::collections::HashSet<&str> =
            reloadable.difference(&live).copied().collect();
        let expected_startup_only: std::collections::HashSet<&str> =
            ["persist", "dnd"].iter().copied().collect();
        assert_eq!(
            startup_only, expected_startup_only,
            "RELOADABLE - LIVE should be exactly {{persist, dnd}}"
        );
    }

    #[test]
    fn overridable_fields_match_clap_arg_set() {
        // Drift detector for the most common case: someone adds a
        // new field to NotificationConfig + a `#[arg(...)]` line,
        // but forgets to register the snake-case name in
        // OVERRIDABLE_FIELDS. clap exposes the registered arg IDs
        // (which match clap's auto-derived snake_case form for our
        // struct), so we compare the registry directly.
        let cmd = NotificationConfig::command();
        let clap_args: std::collections::HashSet<&str> = cmd
            .get_arguments()
            .map(|arg| arg.get_id().as_str())
            .filter(|id| {
                // Exclude transient mode flags (count, update) that
                // are intentionally outside the merge plumbing, and
                // clap's auto-added help/version.
                !matches!(*id, "count" | "update" | "help" | "version")
            })
            .collect();

        let registry: std::collections::HashSet<&str> =
            OVERRIDABLE_FIELDS.iter().copied().collect();

        assert_eq!(
            clap_args,
            registry,
            "OVERRIDABLE_FIELDS drifted from clap's arg set. \
             Missing from registry: {:?}; extra in registry: {:?}.",
            clap_args.difference(&registry).collect::<Vec<_>>(),
            registry.difference(&clap_args).collect::<Vec<_>>(),
        );
    }
}
