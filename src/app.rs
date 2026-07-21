//! Coordinator: wires daemon state, popup manager, panel, D-Bus
//! server, and signal listener. Owns the GTK `Application` and the
//! short-circuit CLI modes (`--count`, `--update`) that exit before
//! claiming the singleton lock. Lives in the library (rather than
//! `main.rs`) so the integration suite in `tests/` links the same
//! code path; the `main.rs` shim just calls [`run`].

use crate::config::NotificationConfig;
use crate::state::NotificationState;
use crate::ui::panel::NotificationPanel;
use crate::ui::popup::PopupManager;
use crate::{config, config_file, dbus, listeners, paths, persistence, ui, waybar};
use clap::{CommandFactory, FromArgMatches};
use gtk4::gio;
use gtk4::prelude::*;
use nwg_common::desktop::dirs::get_app_dirs;
use nwg_common::singleton;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Daemon + CLI entry point. Called by the `main.rs` shim.
pub fn run() {
    nwg_common::process::handle_dump_args();
    // Use the lower-level entry point so we have ArgMatches available for
    // value_source filtering in --update mode (so we only push flags the
    // user actually passed, not their defaults).
    let matches = NotificationConfig::command().get_matches();
    let cli_config = NotificationConfig::from_arg_matches(&matches)
        .expect("clap should produce a valid NotificationConfig from successful matches");

    // Layered merge: defaults < JSON < CLI < (later: D-Bus Set*).
    // CLI flags the user explicitly passed override the JSON; JSON
    // fills in the rest from the file (or compiled defaults via
    // load_or_create_default if the file doesn't exist).
    let json_config = config_file::load_or_create_default(&paths::config_path());
    let user_set = config::user_set_args(&matches);
    let config = merge_cli_over_json(json_config, cli_config, &user_set);

    // Initialize logging before any early-exit CLI mode (--count, --update) so
    // their error paths can reach log::error! per the project's "log errors"
    // convention. Idempotent here — daemon mode reads the same config.debug.
    if config.debug {
        env_logger::Builder::from_default_env()
            .filter_level(log::LevelFilter::Debug)
            .init();
    } else {
        env_logger::init();
    }

    if config.count {
        match dbus::query_count_via_dbus() {
            Ok(count) => {
                println!("{count}");
                std::process::exit(0);
            }
            Err(e) => {
                log::error!("Failed to query count: {e}");
                eprintln!("Failed to query count: {e}");
                eprintln!("(is the nwg-notifications daemon running?)");
                std::process::exit(1);
            }
        }
    }

    if config.update {
        let to_push = crate::config::user_set_live_args(&matches);
        if to_push.is_empty() {
            eprintln!(
                "--update requires at least one of: --popup-position, --popup-width, --panel-width, --popup-timeout, --max-popups, --max-history"
            );
            std::process::exit(1);
        }
        let mut had_error = false;
        for name in &to_push {
            let push_result: Result<(), gtk4::glib::Error> = match *name {
                "popup_position" => {
                    use clap::ValueEnum;
                    let raw = config
                        .popup_position
                        .to_possible_value()
                        .expect("derived ValueEnum yields possible value")
                        .get_name()
                        .to_string();
                    dbus::push_popup_position(&raw)
                }
                "popup_width" => dbus::push_popup_width(
                    u32::try_from(config.popup_width)
                        .expect("popup-width validated by clap range parser"),
                ),
                "panel_width" => dbus::push_panel_width(
                    u32::try_from(config.panel_width)
                        .expect("panel-width validated by clap range parser"),
                ),
                "popup_timeout" => dbus::push_popup_timeout(
                    u32::try_from(config.popup_timeout)
                        .expect("popup-timeout validated by clap range parser"),
                ),
                "max_popups" => dbus::push_max_popups(
                    u32::try_from(config.max_popups)
                        .expect("max-popups validated by clap range parser"),
                ),
                "max_history" => dbus::push_max_history(
                    u32::try_from(config.max_history)
                        .expect("max-history validated by clap range parser"),
                ),
                _ => unreachable!("user_set_live_args returns only known names"),
            };
            if let Err(e) = push_result {
                if dbus::is_unknown_method_error(&e) {
                    log::error!(
                        "Failed to update {name} (unknown D-Bus method on running daemon): {e}"
                    );
                    eprintln!(
                        "Failed to update {name}: the running daemon doesn't recognise this D-Bus method.\n\
                         This usually means the daemon is from a release older than this CLI.\n\
                         Restart it to pick up the new methods, e.g.:\n  \
                         kill $(pidof nwg-notifications) 2>/dev/null || true\n\
                         and let your session manager respawn it (or just run `nwg-notifications --persist &` yourself).\n\
                         Underlying error: {e}"
                    );
                } else {
                    log::error!("Failed to update {name}: {e}");
                    eprintln!("Failed to update {name}: {e}");
                }
                had_error = true;
            } else {
                println!("Updated {name}");
            }
        }
        std::process::exit(if had_error { 1 } else { 0 });
    }

    let _lock = match singleton::acquire_lock("nwg-notifications") {
        Ok(lock) => lock,
        Err(existing_pid) => {
            if let Some(pid) = existing_pid {
                log::info!("Already running (pid {pid})");
            }
            std::process::exit(0);
        }
    };

    let compositor: Rc<dyn nwg_common::compositor::Compositor> =
        Rc::from(nwg_common::compositor::init_or_exit(config.wm));

    // Signal listener — BEFORE GTK, same pattern as the dock
    let sig_rx = listeners::start_signal_listener();

    let app = gtk4::Application::builder()
        .application_id("com.nwg-notifications.hyprland")
        .build();

    let config = Rc::new(RefCell::new(config));
    // GApplication exits as soon as the activate handler returns idle. As a
    // notification daemon we need to stay resident — the popup manager,
    // panel, D-Bus server, and signal listener all rely on the glib main
    // loop continuing to run. `app.hold()` returns a guard that increments
    // GApplication's hold count; storing it in this RefCell keeps it
    // alive for the daemon's lifetime. Drop the guard to let the
    // application exit cleanly. See the GTK `GApplication` docs for
    // hold/release semantics.
    let hold_guard: Rc<RefCell<Option<gio::ApplicationHoldGuard>>> = Rc::new(RefCell::new(None));
    let hold_ref = Rc::clone(&hold_guard);

    app.connect_activate(move |app| {
        *hold_ref.borrow_mut() = Some(app.hold());
        activate_notifications(app, &config, &compositor, &sig_rx);
    });

    app.run_with_args::<String>(&[]);
}

/// Sets up the notification daemon: state, popup manager, panel, D-Bus server, and listeners.
fn activate_notifications(
    app: &gtk4::Application,
    config: &Rc<RefCell<NotificationConfig>>,
    compositor: &Rc<dyn nwg_common::compositor::Compositor>,
    sig_rx: &Rc<std::sync::mpsc::Receiver<listeners::NotificationCommand>>,
) {
    ui::css::load_notification_css();

    // State
    let app_dirs = get_app_dirs();
    let state = Rc::new(RefCell::new(NotificationState::new(
        app_dirs,
        Rc::clone(config),
    )));
    // Initialize DND from CLI flag. Routes through set_dnd so the
    // (dnd, dnd_expires) write stays atomic now that the fields
    // are private; the helper's log line at startup is harmless and
    // confirms in the journal whether the user passed --dnd.
    let initial_dnd = config.borrow().dnd;
    state.borrow_mut().set_dnd(initial_dnd, None);

    // One-time v0.3.x -> v0.4.0 history migration. Idempotent on
    // every subsequent startup once the migration has run.
    paths::migrate_history_if_needed();

    // Load persisted history
    let history_path = paths::history_path();
    if config.borrow().persist {
        let loaded = persistence::load_history(&history_path);
        if !loaded.is_empty() {
            log::info!("Loaded {} notifications from history", loaded.len());
            let mut s = state.borrow_mut();
            for notif in loaded {
                s.history.push(notif);
            }
            s.history.sort_by_key(|n| std::cmp::Reverse(n.timestamp));
            let max = s.config.borrow().max_history;
            s.history.truncate(max);
        }
    }

    // Write initial waybar status
    let s = state.borrow();
    waybar::update_status(s.unread_count(), s.is_dnd_enabled());
    drop(s);

    // Shared callback for any state change -> save history + update waybar
    let on_state_change =
        build_state_change_callback(&state, config.borrow().persist, history_path);

    // Popup manager
    let popup_mgr = Rc::new(RefCell::new(PopupManager::new(
        app,
        config,
        Rc::clone(&on_state_change),
        Rc::clone(compositor),
    )));

    // Panel
    let on_panel_click = build_panel_click_callback(&state, compositor);
    // Closing visible popups when the panel opens — see #3.
    let on_panel_open: Rc<dyn Fn()> = {
        let popup_mgr = Rc::clone(&popup_mgr);
        let state = Rc::clone(&state);
        Rc::new(move || {
            popup_mgr.borrow_mut().dismiss_all_popups(&state);
        })
    };
    let panel = Rc::new(RefCell::new(NotificationPanel::new(
        app,
        &state,
        config,
        on_panel_click,
        Rc::clone(&on_state_change),
        on_panel_open,
    )));

    // D-Bus callbacks
    let on_notify = build_on_notify_callback(&state, &popup_mgr, &panel, &on_state_change);
    let on_change_close = Rc::clone(&on_state_change);
    let popup_mgr_close = Rc::clone(&popup_mgr);
    let on_close: dbus::OnClose = Rc::new(move |id| {
        log::debug!("Notification {id} closed via D-Bus");
        popup_mgr_close.borrow_mut().dismiss(id);
        on_change_close();
    });

    dbus::register_server(
        &state,
        config,
        Rc::clone(&on_state_change),
        on_notify,
        on_close,
    );

    // Hot-reload config from disk. The watcher runs on a detached
    // thread; we poll the receiver from the glib main loop and
    // apply each reload into the live state, skipping fields that
    // are sticky per a Set* override this session.
    let config_watcher = config_file::start_watcher(&paths::config_path());
    let state_reload = Rc::clone(&state);
    let config_reload = Rc::clone(config);
    let on_change_reload = Rc::clone(&on_state_change);
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        while let Ok(new_config) = config_watcher.try_recv() {
            apply_config_reload(&state_reload, &config_reload, &new_config);
            on_change_reload();
        }
        gtk4::glib::ControlFlow::Continue
    });

    // DND menu (right-click waybar bell)
    let dnd_menu = Rc::new(RefCell::new(ui::dnd_menu::DndMenu::new(
        app,
        &state,
        Rc::clone(&on_state_change),
    )));

    listeners::poll_signals(sig_rx, &panel, &state, &on_state_change, &dnd_menu);

    log::info!(
        "Notification daemon started (panel: SIGRTMIN+4, DND: SIGRTMIN+5, menu: SIGRTMIN+6)"
    );
}

/// Returns true if the count has changed since `last_emitted` and should
/// trigger a CountChanged signal. Pure helper to keep the predicate
/// unit-testable.
fn should_emit_count_changed(last_emitted: u32, current: u32) -> bool {
    last_emitted != current
}

/// Creates the shared on_state_change callback that persists history and updates waybar.
fn build_state_change_callback(
    state: &Rc<RefCell<NotificationState>>,
    persist: bool,
    history_path: std::path::PathBuf,
) -> Rc<dyn Fn()> {
    let state_sync = Rc::clone(state);
    let last_emitted_count: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    Rc::new(move || {
        let s = state_sync.borrow();
        let unread = s.unread_count();
        let count = dbus::unread_count_to_u32(unread);
        waybar::update_status(unread, s.is_dnd_enabled());
        if persist {
            persistence::save_history(&history_path, &s.history);
        }
        if should_emit_count_changed(last_emitted_count.get(), count)
            && let Some(conn) = &s.dbus_connection
        {
            last_emitted_count.set(count);
            dbus::emit_count_changed(conn, count);
        }
    })
}

/// Creates the callback invoked when a notification row is clicked in the panel.
fn build_panel_click_callback(
    state: &Rc<RefCell<NotificationState>>,
    compositor: &Rc<dyn nwg_common::compositor::Compositor>,
) -> Rc<dyn Fn(u32)> {
    let state_click = Rc::clone(state);
    let compositor_panel = Rc::clone(compositor);
    Rc::new(move |id| {
        let s = state_click.borrow();
        if let Some(notif) = s.history.iter().find(|n| n.id == id) {
            let app_name = notif.app_name.clone();
            let desktop_entry = notif.desktop_entry.clone();
            drop(s);
            ui::popup::focus_app(
                &app_name,
                desktop_entry.as_deref(),
                &state_click,
                &*compositor_panel,
            );
            state_click.borrow_mut().mark_read(id);
        }
    })
}

/// Creates the D-Bus on_notify callback that shows popups and refreshes the panel.
fn build_on_notify_callback(
    state: &Rc<RefCell<NotificationState>>,
    popup_mgr: &Rc<RefCell<PopupManager>>,
    panel: &Rc<RefCell<NotificationPanel>>,
    on_state_change: &Rc<dyn Fn()>,
) -> dbus::OnNotify {
    let state_notify = Rc::clone(state);
    let popup_mgr_notify = Rc::clone(popup_mgr);
    let panel_notify = Rc::clone(panel);
    let on_change_notify = Rc::clone(on_state_change);
    Rc::new(move |notif| {
        log::info!("[{}] {}: {}", notif.app_name, notif.summary, notif.body);

        if state_notify.borrow().should_show_popup(notif.urgency) {
            popup_mgr_notify.borrow_mut().show(notif, &state_notify);
        }

        if panel_notify.borrow().is_visible() {
            panel_notify.borrow().rebuild();
        }

        on_change_notify();
    })
}

/// Per-field merge of CLI flags over JSON config. For each field
/// in NotificationConfig, take the CLI value if the user
/// explicitly passed it (`user_set` membership), else take the
/// JSON value. The `count` and `update` fields are always taken
/// from CLI — they're transient mode flags, not config knobs.
///
/// **Coupling:** the per-field ladder below must enumerate every
/// name in [`config::OVERRIDABLE_FIELDS`]. Drift is detected by
/// the `overridable_fields_match_clap_arg_set` test in `config.rs`:
/// adding a new field to `NotificationConfig` without registering
/// it there will fail CI; the registry is then your prompt to add
/// a matching `if user_set.contains(...)` arm here.
fn merge_cli_over_json(
    mut json: NotificationConfig,
    cli: NotificationConfig,
    user_set: &std::collections::HashSet<&'static str>,
) -> NotificationConfig {
    for field in config::OVERRIDABLE_FIELDS {
        if !user_set.contains(field) {
            continue;
        }
        copy_overridable_field(field, &cli, &mut json);
    }
    // count and update are always CLI-driven — they're transient
    // mode flags, not config knobs, and aren't tracked in
    // OVERRIDABLE_FIELDS.
    json.count = cli.count;
    json.update = cli.update;
    json
}

/// Copies the named field from `src` to `dst`. Backing dispatch for
/// the merge ladders that drive off [`config::OVERRIDABLE_FIELDS`].
/// Panics if asked for a name not in `OVERRIDABLE_FIELDS` — the
/// drift-detection tests in `config.rs` ensure that condition is
/// unreachable in practice.
fn copy_overridable_field(field: &str, src: &NotificationConfig, dst: &mut NotificationConfig) {
    match field {
        "popup_position" => dst.popup_position = src.popup_position,
        "popup_timeout" => dst.popup_timeout = src.popup_timeout,
        "popup_width" => dst.popup_width = src.popup_width,
        "panel_width" => dst.panel_width = src.panel_width,
        "max_popups" => dst.max_popups = src.max_popups,
        "max_history" => dst.max_history = src.max_history,
        "persist" => dst.persist = src.persist,
        "dnd" => dst.dnd = src.dnd,
        "debug" => dst.debug = src.debug,
        "wm" => dst.wm = src.wm,
        other => unreachable!(
            "copy_overridable_field called with unregistered field: {other}. \
             Drift between config::OVERRIDABLE_FIELDS and this match — \
             register the new field name in config.rs."
        ),
    }
}

/// Applies a hot-reloaded config into the live in-memory config,
/// per-field. Skips any field whose name is in
/// `state.dbus_overrides` (Set* sticky for the session).
///
/// **Coupling:** the per-field ladder below must enumerate every
/// name in [`config::RELOADABLE_FIELDS`]. The CLI-only fields
/// (`debug`, `wm`) are deliberately absent — they don't appear in
/// the JSON, so there's nothing to reload.
fn apply_config_reload(
    state: &Rc<RefCell<NotificationState>>,
    config: &Rc<RefCell<NotificationConfig>>,
    new: &NotificationConfig,
) {
    let overrides = state.borrow().dbus_overrides.clone();
    let mut cfg = config.borrow_mut();
    for field in config::RELOADABLE_FIELDS {
        if overrides.contains(field) {
            continue;
        }
        copy_overridable_field(field, new, &mut cfg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_changed_predicate_emits_on_delta() {
        assert!(should_emit_count_changed(0, 1));
        assert!(should_emit_count_changed(5, 4));
        assert!(should_emit_count_changed(2, 0));
    }

    #[test]
    fn count_changed_predicate_skips_when_equal() {
        assert!(!should_emit_count_changed(0, 0));
        assert!(!should_emit_count_changed(7, 7));
    }

    #[test]
    fn merge_cli_over_json_takes_cli_for_user_set_fields_and_json_for_rest() {
        let json = NotificationConfig {
            popup_timeout: 9999,
            max_popups: 99,
            max_history: 999,
            ..NotificationConfig::default()
        };

        let cli = NotificationConfig {
            popup_timeout: 1, // user passed --popup-timeout 1
            max_popups: 2,    // user passed --max-popups 2
            // max_history not passed; cli.max_history is the default (200)
            ..NotificationConfig::default()
        };

        let mut user_set = std::collections::HashSet::new();
        user_set.insert("popup_timeout");
        user_set.insert("max_popups");
        // max_history NOT in user_set

        let merged = merge_cli_over_json(json, cli, &user_set);

        assert_eq!(merged.popup_timeout, 1, "user-set CLI value wins");
        assert_eq!(merged.max_popups, 2, "user-set CLI value wins");
        assert_eq!(
            merged.max_history, 999,
            "JSON value wins when CLI not user-set"
        );
    }

    #[test]
    fn apply_config_reload_skips_dbus_overridden_fields() {
        // Synthesize a state with one override + a starting config,
        // call apply_config_reload with a new config, assert only
        // the non-overridden fields changed.
        use crate::config::NotificationConfig;
        use crate::state::NotificationState;

        let config = Rc::new(RefCell::new(NotificationConfig::default()));
        let state_inner =
            NotificationState::new(vec![], Rc::new(RefCell::new(NotificationConfig::default())));
        let state = Rc::new(RefCell::new(state_inner));

        // Simulate a Set* call that overrode popup_width earlier in
        // the session.
        state.borrow_mut().mark_dbus_override("popup_width");
        config.borrow_mut().popup_width = 999;

        // Hot-reload: new JSON has different values for both
        // popup_width (overridden) and max_popups (not).
        let new_config = NotificationConfig {
            popup_width: 500,
            max_popups: 42,
            ..NotificationConfig::default()
        };

        apply_config_reload(&state, &config, &new_config);

        let after = config.borrow();
        assert_eq!(
            after.popup_width, 999,
            "popup_width was Set*-overridden; reload must not clobber it"
        );
        assert_eq!(
            after.max_popups, 42,
            "max_popups was not overridden; reload should apply"
        );
    }
}
