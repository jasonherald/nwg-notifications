//! UI layout constants for the notification daemon.

/// Minimum accepted value for the `--popup-width` CLI flag.
pub(crate) const POPUP_WIDTH_MIN: i32 = 100;

/// Maximum accepted value for the `--popup-width` CLI flag.
pub(crate) const POPUP_WIDTH_MAX: i32 = 2000;

/// Default width of popup notification windows. Overridable via the
/// `--popup-width` CLI flag (clamped to `POPUP_WIDTH_MIN..=POPUP_WIDTH_MAX`
/// at parse time).
pub(crate) const POPUP_WIDTH_DEFAULT: i32 = 380;

/// Margin from the top edge of screen.
pub(crate) const POPUP_TOP_MARGIN: i32 = 12;

/// Margin from the right/left edge of screen.
pub(crate) const POPUP_SIDE_MARGIN: i32 = 16;

/// Vertical gap between stacked popups.
pub(crate) const POPUP_GAP: i32 = 10;

/// Icon size in popup notifications.
pub(crate) const POPUP_ICON_SIZE: i32 = 48;

/// Vertical padding around popup content (used for stacking height estimate).
pub(crate) const POPUP_PADDING: i32 = 24;

/// Maximum lines of body text shown in popup.
pub(crate) const POPUP_MAX_BODY_LINES: i32 = 3;

/// Max chars for popup summary line.
pub(crate) const POPUP_SUMMARY_CHARS: i32 = 40;

/// Max chars for popup body text.
pub(crate) const POPUP_BODY_CHARS: i32 = 50;

/// Minimum accepted value for the `--panel-width` CLI flag.
pub(crate) const PANEL_WIDTH_MIN: i32 = 200;

/// Maximum accepted value for the `--panel-width` CLI flag.
pub(crate) const PANEL_WIDTH_MAX: i32 = 2000;

/// Default width of the notification history panel. Overridable via the
/// `--panel-width` CLI flag (validated against
/// `PANEL_WIDTH_MIN..=PANEL_WIDTH_MAX` at parse time).
pub(crate) const PANEL_WIDTH_DEFAULT: i32 = 380;

/// Panel slide animation duration in ms.
pub(crate) const PANEL_REVEAL_DURATION_MS: u32 = 200;

/// Icon size in panel notification rows.
pub(crate) const PANEL_ICON_SIZE: i32 = 36;

/// Icon size in panel group headers.
pub(crate) const GROUP_ICON_SIZE: i32 = 24;

/// Max chars for panel row summary.
pub(crate) const PANEL_SUMMARY_CHARS: i32 = 35;

/// Max chars for panel row body.
pub(crate) const PANEL_BODY_CHARS: i32 = 45;

/// Max lines for panel row body.
pub(crate) const PANEL_BODY_LINES: i32 = 2;
