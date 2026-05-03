//! `NotificationState`: the daemon's mutable state — notification
//! history, app-grouped views, DND mode (with optional expiry),
//! the active-popups set, and the `dbus_connection` slot used by
//! callbacks to emit signals back through the same connection that
//! handled the originating method call.

use crate::config::NotificationConfig;
use crate::notification::{Notification, Urgency};
use gtk4::gio;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

/// A group of notifications from the same application.
pub(crate) struct AppGroup {
    pub(crate) app_name: String,
    pub(crate) app_icon: String,
    pub(crate) notifications: Vec<Notification>,
}

/// Mutable state for the notification daemon.
pub(crate) struct NotificationState {
    /// All notifications, newest first.
    pub(crate) history: Vec<Notification>,
    /// Next ID to assign (starts at 1).
    next_id: u32,
    /// Do Not Disturb mode. Private — readers go through
    /// `is_dnd_enabled`, writers through `set_dnd`. Privatized in
    /// the #37 cleanup so the `(dnd, dnd_expires)` invariant can't
    /// drift via stray direct field writes.
    dnd: bool,
    /// When timed DND expires (None = permanent or off). Private
    /// for the same reason as `dnd`; readers go through the
    /// `dnd_expires` accessor, writers through `set_dnd`.
    dnd_expires: Option<std::time::SystemTime>,
    /// App directories for icon resolution.
    pub(crate) app_dirs: Vec<PathBuf>,
    /// IDs of notifications currently showing as popups.
    pub(crate) active_popups: HashSet<u32>,
    /// Shared daemon configuration. trim_history() reads max_history
    /// from here so live `--update --max-history` changes take effect
    /// without a daemon restart (see #30).
    pub(crate) config: Rc<RefCell<NotificationConfig>>,
    /// D-Bus connection for emitting ActionInvoked signals.
    pub(crate) dbus_connection: Option<gio::DBusConnection>,
}

impl NotificationState {
    pub(crate) fn new(app_dirs: Vec<PathBuf>, config: Rc<RefCell<NotificationConfig>>) -> Self {
        Self {
            history: Vec::new(),
            next_id: 1,
            dnd: false,
            dnd_expires: None,
            app_dirs,
            active_popups: HashSet::new(),
            config,
            dbus_connection: None,
        }
    }

    /// Returns the next notification ID, advancing `next_id` for the
    /// following caller.
    ///
    /// The freedesktop notification spec treats `id == 0` as "no ID
    /// assigned" — for example, `Notify`'s `replaces_id = 0` means
    /// "don't replace; allocate a fresh ID." So we must never hand
    /// out 0 as a real notification ID. The `.max(1)` after
    /// `wrapping_add(1)` is the zero-protection: when `next_id` wraps
    /// from `u32::MAX` back to 0, we skip 0 and return 1 instead.
    ///
    /// `wrapping_add` (rather than checked arithmetic) is intentional:
    /// 4 billion notifications is well past the panel's useful
    /// lifetime, but if someone hits it, we'd rather quietly recycle
    /// IDs than panic.
    fn next_notification_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    /// Adds a notification and returns its assigned ID.
    pub(crate) fn add(&mut self, mut notif: Notification) -> u32 {
        let id = self.next_notification_id();
        notif.id = id;
        self.history.insert(0, notif);
        self.trim_history();
        id
    }

    /// Replaces an existing notification or adds a new one.
    pub(crate) fn replace(&mut self, replaces_id: u32, mut notif: Notification) -> u32 {
        if replaces_id > 0
            && let Some(existing) = self.history.iter_mut().find(|n| n.id == replaces_id)
        {
            notif.id = replaces_id;
            *existing = notif;
            return replaces_id;
        }
        self.add(notif)
    }

    /// Removes a notification by ID.
    pub(crate) fn remove(&mut self, id: u32) -> Option<Notification> {
        if let Some(pos) = self.history.iter().position(|n| n.id == id) {
            self.active_popups.remove(&id);
            Some(self.history.remove(pos))
        } else {
            None
        }
    }

    /// Dismisses all notifications from a specific app.
    pub(crate) fn dismiss_app(&mut self, app_name: &str) {
        self.history.retain(|n| n.app_name != app_name);
    }

    /// Clears all notifications.
    pub(crate) fn dismiss_all(&mut self) {
        self.history.clear();
        self.active_popups.clear();
    }

    /// Returns whether DND is currently active.
    pub(crate) fn is_dnd_enabled(&self) -> bool {
        self.dnd
    }

    /// Returns the current timed-DND expiry, if any. `None` means
    /// either permanent DND (toggled on without an expiry) or DND
    /// is off entirely.
    pub(crate) fn dnd_expires(&self) -> Option<std::time::SystemTime> {
        self.dnd_expires
    }

    /// Sets DND mode and (optionally) the timed-DND expiry, then
    /// logs the transition. The single writer for the `dnd` and
    /// `dnd_expires` fields — every UI / signal / timer call site
    /// routes through this so the two fields can never drift.
    ///
    /// Pass `expires = None` for a permanent toggle (the panel
    /// header button, the signal handler, the menu's "until I turn
    /// it off" entry, and the timer-fire that clears expired DND).
    /// Pass `expires = Some(deadline)` only for the timed-DND menu
    /// buttons that arm a `glib::timeout_add_local_once`.
    pub(crate) fn set_dnd(&mut self, enabled: bool, expires: Option<std::time::SystemTime>) {
        self.dnd = enabled;
        self.dnd_expires = expires;
        log::info!("DND {}", if enabled { "enabled" } else { "disabled" });
    }

    /// Marks a notification as read.
    pub(crate) fn mark_read(&mut self, id: u32) {
        if let Some(notif) = self.history.iter_mut().find(|n| n.id == id) {
            notif.read = true;
        }
    }

    /// Returns the count of unread notifications.
    pub(crate) fn unread_count(&self) -> usize {
        self.history.iter().filter(|n| !n.read).count()
    }

    /// Whether a popup should be shown for this urgency given current DND state.
    pub(crate) fn should_show_popup(&self, urgency: Urgency) -> bool {
        if !self.dnd {
            return true;
        }
        // In DND, only critical notifications show popups
        urgency == Urgency::Critical
    }

    /// Groups notifications by app, preserving insertion order.
    pub(crate) fn grouped_by_app(&self) -> Vec<AppGroup> {
        let mut index: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        let mut groups: Vec<AppGroup> = Vec::new();

        for notif in &self.history {
            if let Some(&idx) = index.get(notif.app_name.as_str()) {
                groups[idx].notifications.push(notif.clone());
            } else {
                index.insert(&notif.app_name, groups.len());
                groups.push(AppGroup {
                    app_name: notif.app_name.clone(),
                    app_icon: notif.app_icon.clone(),
                    notifications: vec![notif.clone()],
                });
            }
        }

        groups
    }

    fn trim_history(&mut self) {
        let max_history = self.config.borrow().max_history;
        if self.history.len() > max_history {
            self.history.truncate(max_history);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn test_config_with_max_history(max_history: usize) -> Rc<RefCell<NotificationConfig>> {
        // Build a default NotificationConfig via clap, then mutate
        // max_history. Avoids re-deriving the entire default-set inline.
        use clap::Parser;
        let mut config = NotificationConfig::parse_from(["test"]);
        config.max_history = max_history;
        Rc::new(RefCell::new(config))
    }

    fn test_notif(app: &str, summary: &str) -> Notification {
        Notification {
            id: 0,
            app_name: app.into(),
            app_icon: String::new(),
            summary: summary.into(),
            body: String::new(),
            actions: Vec::new(),
            urgency: Urgency::Normal,
            timeout_ms: -1,
            timestamp: SystemTime::now(),
            read: false,
            desktop_entry: None,
        }
    }

    #[test]
    fn add_assigns_sequential_ids() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        let id1 = state.add(test_notif("app1", "first"));
        let id2 = state.add(test_notif("app2", "second"));
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn replace_reuses_id() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        let id = state.add(test_notif("app", "original"));
        let replaced = state.replace(id, test_notif("app", "updated"));
        assert_eq!(replaced, id);
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history[0].summary, "updated");
    }

    #[test]
    fn dismiss_app_removes_only_matching() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        state.add(test_notif("firefox", "tab1"));
        state.add(test_notif("discord", "msg1"));
        state.add(test_notif("firefox", "tab2"));
        state.dismiss_app("firefox");
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history[0].app_name, "discord");
    }

    #[test]
    fn unread_count() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        let id1 = state.add(test_notif("app", "one"));
        state.add(test_notif("app", "two"));
        assert_eq!(state.unread_count(), 2);
        state.mark_read(id1);
        assert_eq!(state.unread_count(), 1);
    }

    #[test]
    fn grouped_by_app_groups_correctly() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        state.add(test_notif("firefox", "tab1"));
        state.add(test_notif("discord", "msg"));
        state.add(test_notif("firefox", "tab2"));
        let groups = state.grouped_by_app();
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn dnd_suppresses_normal_popups() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        state.set_dnd(true, None);
        assert!(!state.should_show_popup(Urgency::Normal));
        assert!(!state.should_show_popup(Urgency::Low));
        assert!(state.should_show_popup(Urgency::Critical));
    }

    #[test]
    fn set_dnd_clears_stale_expiry_when_toggling_off() {
        // Bug fix from #37: before the set_dnd helper landed, the
        // signal handler in listeners.rs and the panel-header button
        // in ui/panel.rs both flipped state.dnd directly without
        // touching dnd_expires. So a user who armed timed DND from
        // the menu (sets dnd=true + Some(expiry)) and then toggled
        // DND off via the waybar bell signal would end up with
        // dnd=false but dnd_expires=Some(stale). The set_dnd helper
        // makes (enabled, expires) a single atomic write — the
        // signal-handler path passes None, which clears the expiry.

        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));

        // Arm timed DND (mirrors the dnd_menu timed-DND button path).
        let expiry = SystemTime::now() + std::time::Duration::from_secs(3600);
        state.set_dnd(true, Some(expiry));
        assert!(state.dnd);
        assert_eq!(state.dnd_expires, Some(expiry));

        // Toggle off via signal (mirrors the listeners.rs ToggleDnd path).
        state.set_dnd(false, None);
        assert!(!state.dnd);
        assert_eq!(
            state.dnd_expires, None,
            "toggling DND off via signal must clear stale dnd_expires"
        );
    }

    #[test]
    fn trim_history_caps_at_max() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(3));
        state.add(test_notif("app", "1"));
        state.add(test_notif("app", "2"));
        state.add(test_notif("app", "3"));
        state.add(test_notif("app", "4"));
        assert_eq!(state.history.len(), 3);
    }

    #[test]
    fn id_wrapping_at_max() {
        // Drives next_notification_id through a real wrap to verify the
        // freedesktop "id != 0" invariant holds against the actual add()
        // path, not just the arithmetic in isolation. Pre-set next_id to
        // u32::MAX so the next allocation hands out u32::MAX, and the
        // one after that wraps from 0 → 1 (skipping 0 per the .max(1)).
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        state.next_id = u32::MAX;

        let id_max = state.add(test_notif("app", "max"));
        let id_wrapped = state.add(test_notif("app", "wrapped"));
        let id_after = state.add(test_notif("app", "after"));

        assert_eq!(
            id_max,
            u32::MAX,
            "first add should consume next_id == u32::MAX"
        );
        assert_eq!(
            id_wrapped, 1,
            "wrap from u32::MAX must skip 0 (replaces_id sentinel) and yield 1"
        );
        assert_eq!(
            id_after, 2,
            "subsequent adds continue sequentially after the wrap"
        );
        assert_ne!(id_max, 0);
        assert_ne!(id_wrapped, 0);
        assert_ne!(id_after, 0);
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        assert!(state.remove(999).is_none());
    }

    #[test]
    fn dismiss_all_clears_everything() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        let id1 = state.add(test_notif("app1", "one"));
        let id2 = state.add(test_notif("app2", "two"));
        state.add(test_notif("app3", "three"));
        state.active_popups.insert(id1);
        state.active_popups.insert(id2);

        state.dismiss_all();

        assert!(state.history.is_empty());
        assert!(state.active_popups.is_empty());
    }

    #[test]
    fn mark_read_nonexistent_no_panic() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        state.add(test_notif("app", "exists"));
        // Marking a non-existent ID should not panic.
        state.mark_read(999);
        // Original notification should remain unread.
        assert_eq!(state.unread_count(), 1);
    }

    #[test]
    fn active_popups_tracking() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        state.active_popups.insert(42);
        assert!(state.active_popups.contains(&42));
        state.active_popups.remove(&42);
        assert!(!state.active_popups.contains(&42));
    }

    #[test]
    fn empty_state_operations() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        assert_eq!(state.unread_count(), 0);
        assert!(state.grouped_by_app().is_empty());
        // dismiss_all on empty state should not panic.
        state.dismiss_all();
        assert!(state.history.is_empty());
    }

    #[test]
    fn replace_nonexistent_creates_new() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        // Replace with an ID that doesn't exist falls through to add().
        let id = state.replace(999, test_notif("app", "new"));
        // Should have created a new entry with a fresh ID (1, since state is new).
        assert_eq!(id, 1);
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history[0].summary, "new");
    }

    #[test]
    fn history_ordering_newest_first() {
        let mut state = NotificationState::new(vec![], test_config_with_max_history(100));
        state.add(test_notif("app", "first"));
        state.add(test_notif("app", "second"));
        state.add(test_notif("app", "third"));
        // history[0] should be the most recently added.
        assert_eq!(state.history[0].summary, "third");
        assert_eq!(state.history[1].summary, "second");
        assert_eq!(state.history[2].summary, "first");
    }

    #[test]
    fn add_respects_live_config_max_history_change() {
        // Bug fix #30: trim_history must read max_history from the live
        // config, not a state-side copy.
        let config = test_config_with_max_history(5);
        let mut state = NotificationState::new(vec![], Rc::clone(&config));
        for i in 0..5 {
            state.add(test_notif("app", &format!("notif {i}")));
        }
        assert_eq!(state.history.len(), 5);

        // Simulate `--update --max-history 2` lowering the cap.
        config.borrow_mut().max_history = 2;
        state.add(test_notif("app", "trigger"));

        assert_eq!(
            state.history.len(),
            2,
            "trim_history should have read the new max from config; \
             stuck at 5 means the bug isn't fixed"
        );
    }
}
