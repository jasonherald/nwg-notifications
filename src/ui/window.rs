//! Layer-shell window setup helpers. `setup_popup_window` configures
//! a popup `ApplicationWindow` with the right `gtk4-layer-shell`
//! anchors and margins for its `PopupPosition`. Also exports the
//! backdrop-window helper used by panel and DND menu for
//! click-outside-to-close.

use crate::config::PopupPosition;
use gtk4_layer_shell::LayerShell;

/// Which layer-shell edges a popup anchors to, plus whether it should center
/// horizontally on the unanchored axis.
struct Anchors {
    top: bool,
    bottom: bool,
    left: bool,
    right: bool,
    /// True when neither `left` nor `right` is anchored — the layer shell
    /// centers the surface horizontally in that case. Tracked explicitly so
    /// margin logic can skip side margins on the centered axis without having
    /// to re-derive the condition.
    horizontally_centered: bool,
}

/// Pure mapping from a `PopupPosition` to layer-shell edge anchors.
fn popup_anchors(position: PopupPosition) -> Anchors {
    match position {
        PopupPosition::TopRight => Anchors {
            top: true,
            bottom: false,
            left: false,
            right: true,
            horizontally_centered: false,
        },
        PopupPosition::TopCenter => Anchors {
            top: true,
            bottom: false,
            left: false,
            right: false,
            horizontally_centered: true,
        },
        PopupPosition::TopLeft => Anchors {
            top: true,
            bottom: false,
            left: true,
            right: false,
            horizontally_centered: false,
        },
        PopupPosition::BottomRight => Anchors {
            top: false,
            bottom: true,
            left: false,
            right: true,
            horizontally_centered: false,
        },
        PopupPosition::BottomCenter => Anchors {
            top: false,
            bottom: true,
            left: false,
            right: false,
            horizontally_centered: true,
        },
        PopupPosition::BottomLeft => Anchors {
            top: false,
            bottom: true,
            left: true,
            right: false,
            horizontally_centered: false,
        },
    }
}

/// Configures a popup window with layer-shell properties.
pub(crate) fn setup_popup_window(
    win: &gtk4::ApplicationWindow,
    position: PopupPosition,
    top_offset: i32,
) {
    win.init_layer_shell();
    win.set_namespace(Some("nwg-notification-popup"));
    win.set_layer(gtk4_layer_shell::Layer::Overlay);
    win.set_exclusive_zone(-1);

    let anchors = popup_anchors(position);
    win.set_anchor(gtk4_layer_shell::Edge::Top, anchors.top);
    win.set_anchor(gtk4_layer_shell::Edge::Bottom, anchors.bottom);
    win.set_anchor(gtk4_layer_shell::Edge::Left, anchors.left);
    win.set_anchor(gtk4_layer_shell::Edge::Right, anchors.right);

    // Vertical offset for stacking — applied on whichever vertical edge
    // is anchored.
    if anchors.top {
        win.set_margin(gtk4_layer_shell::Edge::Top, top_offset);
    } else {
        win.set_margin(gtk4_layer_shell::Edge::Bottom, top_offset);
    }

    // Side margin only applies for corner placements; centered placements
    // float to monitor center and don't need a side margin.
    if !anchors.horizontally_centered {
        let side_edge = if anchors.right {
            gtk4_layer_shell::Edge::Right
        } else {
            gtk4_layer_shell::Edge::Left
        };
        win.set_margin(side_edge, super::constants::POPUP_SIDE_MARGIN);
    }

    // No keyboard interactivity — popups shouldn't steal focus.
    win.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
}

// Backdrop helpers live in `nwg_common::layer_shell`; the panel and
// DND menu re-export-by-using them with their own CSS class so the
// stylesheet for each gets the right opacity.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_top_right() {
        let a = popup_anchors(PopupPosition::TopRight);
        assert_eq!(
            (a.top, a.bottom, a.left, a.right),
            (true, false, false, true)
        );
        assert!(!a.horizontally_centered);
    }

    #[test]
    fn anchors_top_left() {
        let a = popup_anchors(PopupPosition::TopLeft);
        assert_eq!(
            (a.top, a.bottom, a.left, a.right),
            (true, false, true, false)
        );
        assert!(!a.horizontally_centered);
    }

    #[test]
    fn anchors_bottom_right() {
        let a = popup_anchors(PopupPosition::BottomRight);
        assert_eq!(
            (a.top, a.bottom, a.left, a.right),
            (false, true, false, true)
        );
        assert!(!a.horizontally_centered);
    }

    #[test]
    fn anchors_bottom_left() {
        let a = popup_anchors(PopupPosition::BottomLeft);
        assert_eq!(
            (a.top, a.bottom, a.left, a.right),
            (false, true, true, false)
        );
        assert!(!a.horizontally_centered);
    }

    #[test]
    fn anchors_top_center() {
        // Centered: anchor only the top edge — gtk4-layer-shell centers the
        // surface horizontally when neither left nor right is anchored.
        let a = popup_anchors(PopupPosition::TopCenter);
        assert_eq!(
            (a.top, a.bottom, a.left, a.right),
            (true, false, false, false)
        );
        assert!(a.horizontally_centered);
    }

    #[test]
    fn anchors_bottom_center() {
        let a = popup_anchors(PopupPosition::BottomCenter);
        assert_eq!(
            (a.top, a.bottom, a.left, a.right),
            (false, true, false, false)
        );
        assert!(a.horizontally_centered);
    }
}
