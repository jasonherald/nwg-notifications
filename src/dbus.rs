//! D-Bus server for the notification daemon. Claims
//! `org.freedesktop.Notifications` (the freedesktop-spec interface)
//! and `org.nwg.Notifications` (the project-private interface used
//! by `nwg-shell-config` and `nwg-panel` for live config + count
//! IPC). Runs directly on the glib main loop via
//! `gio::bus_own_name`; no async bridge.

use crate::config::NotificationConfig;
use crate::notification::{Notification, Urgency, clean_markup, parse_actions};
use crate::state::NotificationState;
use gtk4::gio;
use gtk4::glib;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::SystemTime;

/// D-Bus introspection XML for org.freedesktop.Notifications.
const INTROSPECT_XML: &str = r#"
<node>
  <interface name="org.freedesktop.Notifications">
    <method name="Notify">
      <arg name="app_name" type="s" direction="in"/>
      <arg name="replaces_id" type="u" direction="in"/>
      <arg name="app_icon" type="s" direction="in"/>
      <arg name="summary" type="s" direction="in"/>
      <arg name="body" type="s" direction="in"/>
      <arg name="actions" type="as" direction="in"/>
      <arg name="hints" type="a{sv}" direction="in"/>
      <arg name="expire_timeout" type="i" direction="in"/>
      <arg name="id" type="u" direction="out"/>
    </method>
    <method name="CloseNotification">
      <arg name="id" type="u" direction="in"/>
    </method>
    <method name="GetCapabilities">
      <arg name="capabilities" type="as" direction="out"/>
    </method>
    <method name="GetServerInformation">
      <arg name="name" type="s" direction="out"/>
      <arg name="vendor" type="s" direction="out"/>
      <arg name="version" type="s" direction="out"/>
      <arg name="spec_version" type="s" direction="out"/>
    </method>
    <signal name="NotificationClosed">
      <arg name="id" type="u"/>
      <arg name="reason" type="u"/>
    </signal>
    <signal name="ActionInvoked">
      <arg name="id" type="u"/>
      <arg name="action_key" type="s"/>
    </signal>
  </interface>
</node>
"#;

/// D-Bus introspection XML for the nwg-specific count IPC interface.
const NWG_COUNT_INTROSPECT_XML: &str = r#"
<node>
  <interface name="org.nwg.Notifications">
    <method name="GetCount">
      <arg name="count" type="u" direction="out"/>
    </method>
    <signal name="CountChanged">
      <arg name="count" type="u"/>
    </signal>
    <method name="SetPopupPosition">
      <arg name="position" type="s" direction="in"/>
    </method>
    <method name="SetPopupWidth">
      <arg name="width" type="u" direction="in"/>
    </method>
    <method name="SetPanelWidth">
      <arg name="width" type="u" direction="in"/>
    </method>
    <method name="SetPopupTimeout">
      <arg name="timeout_ms" type="u" direction="in"/>
    </method>
    <method name="SetMaxPopups">
      <arg name="max" type="u" direction="in"/>
    </method>
    <method name="SetMaxHistory">
      <arg name="max" type="u" direction="in"/>
    </method>
  </interface>
</node>
"#;

/// D-Bus name for the nwg-specific count IPC interface.
pub(crate) const NWG_COUNT_BUS_NAME: &str = "org.nwg.Notifications";
/// D-Bus object path for the nwg-specific count IPC interface.
pub(crate) const NWG_COUNT_OBJECT_PATH: &str = "/org/nwg/Notifications";

/// Callback invoked when a new notification arrives via D-Bus.
/// Implement this to show popups, update waybar, etc.
pub(crate) type OnNotify = Rc<dyn Fn(&Notification)>;

/// Callback invoked when a notification is closed via D-Bus.
pub(crate) type OnClose = Rc<dyn Fn(u32)>;

/// Registers the notification D-Bus server on the session bus.
///
/// Runs entirely on the glib main loop — no threads or async needed.
/// Acquires both `org.freedesktop.Notifications` (the standard notification
/// daemon interface) and `org.nwg.Notifications` (the nwg-specific count IPC).
pub(crate) fn register_server(
    state: &Rc<RefCell<NotificationState>>,
    config: &Rc<RefCell<NotificationConfig>>,
    on_state_change: Rc<dyn Fn()>,
    on_notify: OnNotify,
    on_close: OnClose,
) {
    let state_fdo = Rc::clone(state);
    let on_notify_fdo = Rc::clone(&on_notify);
    let on_close_fdo = Rc::clone(&on_close);

    gio::bus_own_name(
        gio::BusType::Session,
        "org.freedesktop.Notifications",
        gio::BusNameOwnerFlags::REPLACE,
        move |connection, _name| {
            log::info!("Acquired D-Bus name: org.freedesktop.Notifications");
            state_fdo.borrow_mut().dbus_connection = Some(connection.clone());
            register_object(&connection, &state_fdo, &on_notify_fdo, &on_close_fdo);
        },
        |_connection, _name| {
            log::debug!("D-Bus name acquired callback");
        },
        |_connection, _name| {
            log::error!(
                "Lost D-Bus name org.freedesktop.Notifications — is another daemon running?"
            );
        },
    );

    let state_nwg = Rc::clone(state);
    let config_nwg = Rc::clone(config);
    let on_change_nwg = Rc::clone(&on_state_change);
    gio::bus_own_name(
        gio::BusType::Session,
        NWG_COUNT_BUS_NAME,
        gio::BusNameOwnerFlags::REPLACE,
        move |connection, _name| {
            log::info!("Acquired D-Bus name: {}", NWG_COUNT_BUS_NAME);
            register_nwg_count_object(&connection, &state_nwg, &config_nwg, &on_change_nwg);
        },
        |_connection, _name| {
            log::debug!("nwg-count D-Bus name acquired callback");
        },
        |_connection, _name| {
            log::error!("Lost D-Bus name {} — another daemon?", NWG_COUNT_BUS_NAME);
        },
    );
}

/// Registers the daemon's `org.freedesktop.Notifications` D-Bus
/// object on the given connection. Wires `handle_method` as the
/// method-call dispatcher.
///
/// # Panics
///
/// Panics on three unreachable-in-practice failure modes, all of
/// which represent a build-time misconfiguration rather than a
/// runtime condition:
/// - `INTROSPECT_XML` fails to parse — the XML is a `const &str`
///   in this file, so a parse failure means we shipped malformed
///   XML and CI should have caught it.
/// - The `org.freedesktop.Notifications` interface name doesn't
///   resolve in the parsed `DBusNodeInfo` — same `const`-source
///   provenance as above.
/// - `register_object` fails to build — the object path is a
///   string literal and the interface info just came from the
///   parsed `const`, so any failure here would be a bug in `gio`'s
///   builder.
fn register_object(
    connection: &gio::DBusConnection,
    state: &Rc<RefCell<NotificationState>>,
    on_notify: &OnNotify,
    on_close: &OnClose,
) {
    let node_info = gio::DBusNodeInfo::for_xml(INTROSPECT_XML)
        .expect("Failed to parse notification introspection XML");

    let interface_info = node_info
        .lookup_interface("org.freedesktop.Notifications")
        .expect("Interface not found in XML");

    let state = Rc::clone(state);
    let on_notify = Rc::clone(on_notify);
    let on_close = Rc::clone(on_close);

    connection
        .register_object("/org/freedesktop/Notifications", &interface_info)
        .method_call(
            move |_conn, _sender, _path, _iface, method, params, invocation| {
                handle_method(method, params, invocation, &state, &on_notify, &on_close);
            },
        )
        .build()
        .expect("Failed to register D-Bus object");
}

fn handle_method(
    method: &str,
    params: glib::Variant,
    invocation: gio::DBusMethodInvocation,
    state: &Rc<RefCell<NotificationState>>,
    on_notify: &OnNotify,
    on_close: &OnClose,
) {
    match method {
        "Notify" => handle_notify(&params, invocation, state, on_notify),
        "CloseNotification" => handle_close(&params, invocation, state, on_close),
        "GetCapabilities" => handle_capabilities(invocation),
        "GetServerInformation" => handle_server_info(invocation),
        _ => {
            log::warn!("Unknown D-Bus method: {}", method);
            invocation.return_dbus_error(
                "org.freedesktop.DBus.Error.UnknownMethod",
                &format!("Unknown method: {method}"),
            );
        }
    }
}

/// Registers the daemon's `org.nwg.Notifications` D-Bus object on
/// the given connection. Backs `GetCount`, the six `Set*` live-config
/// setters, and the `CountChanged` signal source.
///
/// # Panics
///
/// Panics on three unreachable-in-practice failure modes, all of
/// which represent a build-time misconfiguration rather than a
/// runtime condition:
/// - `NWG_COUNT_INTROSPECT_XML` fails to parse — the XML is a
///   `const &str` in this file, so a parse failure means we
///   shipped malformed XML and CI should have caught it.
/// - The `org.nwg.Notifications` interface name doesn't resolve
///   in the parsed `DBusNodeInfo` — same `const`-source
///   provenance as above.
/// - `register_object` fails to build — the object path is a
///   string literal and the interface info just came from the
///   parsed `const`.
fn register_nwg_count_object(
    connection: &gio::DBusConnection,
    state: &Rc<RefCell<NotificationState>>,
    config: &Rc<RefCell<NotificationConfig>>,
    on_state_change: &Rc<dyn Fn()>,
) {
    let node_info = gio::DBusNodeInfo::for_xml(NWG_COUNT_INTROSPECT_XML)
        .expect("Failed to parse nwg-count introspection XML");

    let interface_info = node_info
        .lookup_interface(NWG_COUNT_BUS_NAME)
        .expect("nwg-count interface not found in XML");

    let state = Rc::clone(state);
    let config = Rc::clone(config);
    let on_state_change = Rc::clone(on_state_change);

    connection
        .register_object(NWG_COUNT_OBJECT_PATH, &interface_info)
        .method_call(
            move |_conn, _sender, _path, _iface, method, params, invocation| {
                handle_nwg_count_method(
                    method,
                    &params,
                    invocation,
                    &state,
                    &config,
                    &on_state_change,
                );
            },
        )
        .build()
        .expect("Failed to register nwg-count D-Bus object");
}

fn handle_nwg_count_method(
    method: &str,
    params: &glib::Variant,
    invocation: gio::DBusMethodInvocation,
    state: &Rc<RefCell<NotificationState>>,
    config: &Rc<RefCell<NotificationConfig>>,
    on_state_change: &Rc<dyn Fn()>,
) {
    match method {
        "GetCount" => {
            let count = unread_count_to_u32(state.borrow().unread_count());
            let result = glib::Variant::from((count,));
            invocation.return_value(Some(&result));
        }
        "SetPopupPosition" => {
            handle_set_popup_position(params, invocation, config, on_state_change)
        }
        "SetPopupWidth" => handle_set_popup_width(params, invocation, config, on_state_change),
        "SetPanelWidth" => handle_set_panel_width(params, invocation, config, on_state_change),
        "SetPopupTimeout" => handle_set_popup_timeout(params, invocation, config, on_state_change),
        "SetMaxPopups" => handle_set_max_popups(params, invocation, config, on_state_change),
        "SetMaxHistory" => handle_set_max_history(params, invocation, config, on_state_change),
        _ => {
            log::warn!("Unknown nwg-count D-Bus method: {}", method);
            invocation.return_dbus_error(
                "org.freedesktop.DBus.Error.UnknownMethod",
                &format!("Unknown method: {method}"),
            );
        }
    }
}

fn return_invalid_args(invocation: gio::DBusMethodInvocation, msg: &str) {
    invocation.return_dbus_error("org.freedesktop.DBus.Error.InvalidArgs", msg);
}

fn handle_set_popup_position(
    params: &glib::Variant,
    invocation: gio::DBusMethodInvocation,
    config: &Rc<RefCell<NotificationConfig>>,
    on_state_change: &Rc<dyn Fn()>,
) {
    let raw: String = match params.child_value(0).get() {
        Some(s) => s,
        None => {
            return_invalid_args(invocation, "SetPopupPosition expects a string argument");
            return;
        }
    };
    use clap::ValueEnum;
    match crate::config::PopupPosition::from_str(&raw, true) {
        Ok(pos) => {
            config.borrow_mut().popup_position = pos;
            invocation.return_value(None);
            on_state_change();
        }
        Err(_) => {
            return_invalid_args(
                invocation,
                &format!(
                    "Invalid popup-position '{raw}'. Expected one of: top-right, top-center, top-left, bottom-right, bottom-center, bottom-left."
                ),
            );
        }
    }
}

fn handle_set_popup_width(
    params: &glib::Variant,
    invocation: gio::DBusMethodInvocation,
    config: &Rc<RefCell<NotificationConfig>>,
    on_state_change: &Rc<dyn Fn()>,
) {
    let raw: u32 = match params.child_value(0).get() {
        Some(v) => v,
        None => {
            return_invalid_args(invocation, "SetPopupWidth expects a uint32 argument");
            return;
        }
    };
    let raw_i32 = match i32::try_from(raw) {
        Ok(v) => v,
        Err(_) => {
            return_invalid_args(invocation, &format!("popup-width {raw} exceeds i32::MAX"));
            return;
        }
    };
    if !(crate::ui::constants::POPUP_WIDTH_MIN..=crate::ui::constants::POPUP_WIDTH_MAX)
        .contains(&raw_i32)
    {
        return_invalid_args(
            invocation,
            &format!(
                "popup-width {raw_i32} is not in {min}..={max}",
                min = crate::ui::constants::POPUP_WIDTH_MIN,
                max = crate::ui::constants::POPUP_WIDTH_MAX,
            ),
        );
        return;
    }
    config.borrow_mut().popup_width = raw_i32;
    invocation.return_value(None);
    on_state_change();
}

fn handle_set_panel_width(
    params: &glib::Variant,
    invocation: gio::DBusMethodInvocation,
    config: &Rc<RefCell<NotificationConfig>>,
    on_state_change: &Rc<dyn Fn()>,
) {
    let raw: u32 = match params.child_value(0).get() {
        Some(v) => v,
        None => {
            return_invalid_args(invocation, "SetPanelWidth expects a uint32 argument");
            return;
        }
    };
    let raw_i32 = match i32::try_from(raw) {
        Ok(v) => v,
        Err(_) => {
            return_invalid_args(invocation, &format!("panel-width {raw} exceeds i32::MAX"));
            return;
        }
    };
    if !(crate::ui::constants::PANEL_WIDTH_MIN..=crate::ui::constants::PANEL_WIDTH_MAX)
        .contains(&raw_i32)
    {
        return_invalid_args(
            invocation,
            &format!(
                "panel-width {raw_i32} is not in {min}..={max}",
                min = crate::ui::constants::PANEL_WIDTH_MIN,
                max = crate::ui::constants::PANEL_WIDTH_MAX,
            ),
        );
        return;
    }
    config.borrow_mut().panel_width = raw_i32;
    invocation.return_value(None);
    on_state_change();
}

fn handle_set_popup_timeout(
    params: &glib::Variant,
    invocation: gio::DBusMethodInvocation,
    config: &Rc<RefCell<NotificationConfig>>,
    on_state_change: &Rc<dyn Fn()>,
) {
    let raw: u32 = match params.child_value(0).get() {
        Some(v) => v,
        None => {
            return_invalid_args(invocation, "SetPopupTimeout expects a uint32 argument");
            return;
        }
    };
    // 0 is a valid value (means "never auto-dismiss").
    config.borrow_mut().popup_timeout = u64::from(raw);
    invocation.return_value(None);
    on_state_change();
}

fn handle_set_max_popups(
    params: &glib::Variant,
    invocation: gio::DBusMethodInvocation,
    config: &Rc<RefCell<NotificationConfig>>,
    on_state_change: &Rc<dyn Fn()>,
) {
    let raw: u32 = match params.child_value(0).get() {
        Some(v) => v,
        None => {
            return_invalid_args(invocation, "SetMaxPopups expects a uint32 argument");
            return;
        }
    };
    if raw == 0 {
        return_invalid_args(invocation, "max-popups must be >= 1");
        return;
    }
    config.borrow_mut().max_popups = raw as usize;
    invocation.return_value(None);
    on_state_change();
}

fn handle_set_max_history(
    params: &glib::Variant,
    invocation: gio::DBusMethodInvocation,
    config: &Rc<RefCell<NotificationConfig>>,
    on_state_change: &Rc<dyn Fn()>,
) {
    let raw: u32 = match params.child_value(0).get() {
        Some(v) => v,
        None => {
            return_invalid_args(invocation, "SetMaxHistory expects a uint32 argument");
            return;
        }
    };
    if raw == 0 {
        return_invalid_args(invocation, "max-history must be >= 1");
        return;
    }
    config.borrow_mut().max_history = raw as usize;
    invocation.return_value(None);
    on_state_change();
}

fn handle_notify(
    params: &glib::Variant,
    invocation: gio::DBusMethodInvocation,
    state: &Rc<RefCell<NotificationState>>,
    on_notify: &OnNotify,
) {
    // Parse the Notify parameters: (susssasa{sv}i)
    let app_name: String = params.child_value(0).get().unwrap_or_default();
    let replaces_id: u32 = params.child_value(1).get().unwrap_or(0);
    let app_icon: String = params.child_value(2).get().unwrap_or_default();
    let summary: String = params.child_value(3).get().unwrap_or_default();
    let body: String = params.child_value(4).get().unwrap_or_default();
    let timeout: i32 = params.child_value(7).get().unwrap_or(-1);

    // Parse actions array
    let actions_variant = params.child_value(5);
    let actions: Vec<String> = (0..actions_variant.n_children())
        .filter_map(|i| actions_variant.child_value(i).get::<String>())
        .collect();

    // Parse hints dict for urgency and desktop-entry
    let hints_variant = params.child_value(6);
    let urgency = extract_urgency(&hints_variant);
    let desktop_entry = extract_string_hint(&hints_variant, "desktop-entry");

    let notif = Notification {
        id: 0, // assigned by state.add/replace
        app_name,
        app_icon,
        summary: clean_markup(&summary),
        body: clean_markup(&body),
        actions: parse_actions(&actions),
        urgency,
        timeout_ms: timeout,
        timestamp: SystemTime::now(),
        read: false,
        desktop_entry,
    };

    log::debug!(
        "Notify: app={}, summary={}, urgency={:?}",
        notif.app_name,
        notif.summary,
        notif.urgency
    );

    let id = state.borrow_mut().replace(replaces_id, notif.clone());

    // Update the notification with the assigned ID for the callback
    let mut notif_with_id = notif;
    notif_with_id.id = id;
    on_notify(&notif_with_id);

    // Return the assigned ID
    let result = glib::Variant::from((id,));
    invocation.return_value(Some(&result));
}

fn handle_close(
    params: &glib::Variant,
    invocation: gio::DBusMethodInvocation,
    state: &Rc<RefCell<NotificationState>>,
    on_close: &OnClose,
) {
    let id: u32 = params.child_value(0).get().unwrap_or(0);
    state.borrow_mut().remove(id);
    on_close(id);
    invocation.return_value(None);
}

fn handle_capabilities(invocation: gio::DBusMethodInvocation) {
    let caps = vec!["body", "body-markup", "actions", "icon-static"];
    let variant = glib::Variant::from((caps,));
    invocation.return_value(Some(&variant));
}

/// Returns the (name, vendor, version, spec_version) tuple reported by the
/// `org.freedesktop.Notifications.GetServerInformation` D-Bus method.
/// Vendor is the daemon's own name (single-vendor project convention);
/// version comes from `Cargo.toml` at compile time so it stays in sync
/// with releases automatically; spec_version tracks the freedesktop
/// notification specification level we implement.
fn server_info_tuple() -> (&'static str, &'static str, &'static str, &'static str) {
    (
        "nwg-notifications",
        "nwg-notifications",
        env!("CARGO_PKG_VERSION"),
        "1.2",
    )
}

fn handle_server_info(invocation: gio::DBusMethodInvocation) {
    let info = server_info_tuple();
    let variant = glib::Variant::from(info);
    invocation.return_value(Some(&variant));
}

/// Emits the ActionInvoked D-Bus signal to the sending app.
pub(crate) fn emit_action_invoked(connection: &gio::DBusConnection, id: u32, action_key: &str) {
    let params = glib::Variant::from((id, action_key));
    if let Err(e) = connection.emit_signal(
        None::<&str>,
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "ActionInvoked",
        Some(&params),
    ) {
        log::warn!("Failed to emit ActionInvoked: {}", e);
    }
}

/// Converts a usize unread count to the u32 expected by the
/// `org.nwg.Notifications` wire format. usize on 64-bit hosts is u64, so
/// in theory a count could exceed u32::MAX; in practice `max_history`
/// caps that long before the protocol cares. Logs and clamps to
/// `u32::MAX` if it ever does happen rather than silently truncating.
pub(crate) fn unread_count_to_u32(unread: usize) -> u32 {
    u32::try_from(unread).unwrap_or_else(|_| {
        log::error!(
            "Unread count {} exceeds u32::MAX; clamping for D-Bus payload",
            unread
        );
        u32::MAX
    })
}

/// Timeout for the `--count` CLI's D-Bus call, in milliseconds.
/// Local D-Bus calls to the running daemon are sub-millisecond when healthy;
/// 2s is generous enough to absorb transient bus contention while keeping
/// the CLI responsive when something is genuinely broken.
const QUERY_COUNT_TIMEOUT_MS: i32 = 2_000;

/// Queries the running daemon's `GetCount()` method over the session bus
/// and returns the unread count. Uses `NO_AUTO_START` so it never spawns
/// a daemon — if no daemon is running, this returns an error.
///
/// Used by the `--count` CLI subcommand.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable (no D-Bus, no `DBUS_SESSION_BUS_ADDRESS`).
/// - No daemon owns the `org.nwg.Notifications` name (`NO_AUTO_START` semantics).
/// - The call exceeds `QUERY_COUNT_TIMEOUT_MS`.
/// - The reply payload doesn't unpack to the expected `(u32,)` tuple.
pub(crate) fn query_count_via_dbus() -> Result<u32, glib::Error> {
    let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
    let result = connection.call_sync(
        Some(NWG_COUNT_BUS_NAME),
        NWG_COUNT_OBJECT_PATH,
        NWG_COUNT_BUS_NAME,
        "GetCount",
        None,
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        QUERY_COUNT_TIMEOUT_MS,
        gio::Cancellable::NONE,
    )?;
    result.child_value(0).get::<u32>().ok_or_else(|| {
        glib::Error::new(
            gio::IOErrorEnum::InvalidData,
            "GetCount returned unexpected payload type",
        )
    })
}

/// Generic D-Bus client helper used by all six `--update` push wrappers.
/// Same NO_AUTO_START + QUERY_COUNT_TIMEOUT_MS semantics as
/// `query_count_via_dbus`.
fn call_setter_sync(method: &str, payload: glib::Variant) -> Result<(), glib::Error> {
    let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
    connection.call_sync(
        Some(NWG_COUNT_BUS_NAME),
        NWG_COUNT_OBJECT_PATH,
        NWG_COUNT_BUS_NAME,
        method,
        Some(&payload),
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        QUERY_COUNT_TIMEOUT_MS,
        gio::Cancellable::NONE,
    )?;
    Ok(())
}

/// Pushes a `--popup-position` change to the running daemon via
/// `org.nwg.Notifications.SetPopupPosition`. Used by
/// `nwg-notifications --update --popup-position <value>`.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable.
/// - No daemon owns the `org.nwg.Notifications` name (the
///   `NO_AUTO_START` flag means the call doesn't spawn one).
/// - The daemon rejects the value with
///   `org.freedesktop.DBus.Error.InvalidArgs` (for example, an
///   unrecognised position string).
/// - The daemon's running version doesn't expose `SetPopupPosition`
///   yet — surfaced as `org.freedesktop.DBus.Error.UnknownMethod`,
///   which the CLI's `--update` path translates into the
///   "restart-after-upgrade" hint.
pub(crate) fn push_popup_position(value: &str) -> Result<(), glib::Error> {
    call_setter_sync("SetPopupPosition", glib::Variant::from((value,)))
}

/// Pushes a `--popup-width <px>` change to the running daemon via
/// `org.nwg.Notifications.SetPopupWidth`. Used by
/// `nwg-notifications --update --popup-width <px>`.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable.
/// - No daemon owns the `org.nwg.Notifications` name.
/// - The daemon rejects the value with
///   `org.freedesktop.DBus.Error.InvalidArgs` (for example, a value
///   outside the 100..=2000 range).
/// - The daemon's running version doesn't expose `SetPopupWidth`
///   yet — surfaced as `org.freedesktop.DBus.Error.UnknownMethod`.
pub(crate) fn push_popup_width(value: u32) -> Result<(), glib::Error> {
    call_setter_sync("SetPopupWidth", glib::Variant::from((value,)))
}

/// Pushes a `--panel-width <px>` change to the running daemon via
/// `org.nwg.Notifications.SetPanelWidth`. Used by
/// `nwg-notifications --update --panel-width <px>`.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable.
/// - No daemon owns the `org.nwg.Notifications` name.
/// - The daemon rejects the value with
///   `org.freedesktop.DBus.Error.InvalidArgs` (for example, a value
///   outside the 200..=2000 range).
/// - The daemon's running version doesn't expose `SetPanelWidth`
///   yet — surfaced as `org.freedesktop.DBus.Error.UnknownMethod`.
pub(crate) fn push_panel_width(value: u32) -> Result<(), glib::Error> {
    call_setter_sync("SetPanelWidth", glib::Variant::from((value,)))
}

/// Pushes a `--popup-timeout <secs>` change to the running daemon via
/// `org.nwg.Notifications.SetPopupTimeout`. Used by
/// `nwg-notifications --update --popup-timeout <secs>`.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable.
/// - No daemon owns the `org.nwg.Notifications` name.
/// - The daemon rejects the payload type with
///   `org.freedesktop.DBus.Error.InvalidArgs` (only fires on a
///   non-`u32` payload — `handle_set_popup_timeout` does not enforce
///   a value range, so any `u32` is accepted).
/// - The daemon's running version doesn't expose `SetPopupTimeout`
///   yet — surfaced as `org.freedesktop.DBus.Error.UnknownMethod`.
pub(crate) fn push_popup_timeout(value: u32) -> Result<(), glib::Error> {
    call_setter_sync("SetPopupTimeout", glib::Variant::from((value,)))
}

/// Pushes a `--max-popups <N>` change to the running daemon via
/// `org.nwg.Notifications.SetMaxPopups`. Used by
/// `nwg-notifications --update --max-popups <N>`.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable.
/// - No daemon owns the `org.nwg.Notifications` name.
/// - The daemon rejects the value with
///   `org.freedesktop.DBus.Error.InvalidArgs`. `handle_set_max_popups`
///   only rejects two cases: a non-`u32` payload, and the literal
///   value `0` ("max-popups must be >= 1"). No upper bound is enforced
///   daemon-side.
/// - The daemon's running version doesn't expose `SetMaxPopups`
///   yet — surfaced as `org.freedesktop.DBus.Error.UnknownMethod`.
pub(crate) fn push_max_popups(value: u32) -> Result<(), glib::Error> {
    call_setter_sync("SetMaxPopups", glib::Variant::from((value,)))
}

/// Pushes a `--max-history <N>` change to the running daemon via
/// `org.nwg.Notifications.SetMaxHistory`. Used by
/// `nwg-notifications --update --max-history <N>`.
///
/// # Errors
///
/// Returns the underlying `glib::Error` when:
/// - The session bus isn't reachable.
/// - No daemon owns the `org.nwg.Notifications` name.
/// - The daemon rejects the value with
///   `org.freedesktop.DBus.Error.InvalidArgs`. `handle_set_max_history`
///   only rejects two cases: a non-`u32` payload, and the literal
///   value `0` ("max-history must be >= 1"). No upper bound is enforced
///   daemon-side.
/// - The daemon's running version doesn't expose `SetMaxHistory`
///   yet — surfaced as `org.freedesktop.DBus.Error.UnknownMethod`.
pub(crate) fn push_max_history(value: u32) -> Result<(), glib::Error> {
    call_setter_sync("SetMaxHistory", glib::Variant::from((value,)))
}

/// Returns true if the given `glib::Error` is the standard D-Bus
/// `org.freedesktop.DBus.Error.UnknownMethod` error class. Used by the
/// `--update` CLI to give an actionable message when the running daemon
/// is from a release older than the CLI and doesn't recognise a method
/// the CLI is trying to call (#25).
pub(crate) fn is_unknown_method_error(err: &glib::Error) -> bool {
    err.matches(gio::DBusError::UnknownMethod)
}

/// Emits CountChanged on the org.nwg.Notifications interface.
///
/// Best-effort: a failure here doesn't affect anything else; we log and move on.
pub(crate) fn emit_count_changed(connection: &gio::DBusConnection, count: u32) {
    let params = glib::Variant::from((count,));
    if let Err(e) = connection.emit_signal(
        None::<&str>,
        NWG_COUNT_OBJECT_PATH,
        NWG_COUNT_BUS_NAME,
        "CountChanged",
        Some(&params),
    ) {
        log::warn!("Failed to emit CountChanged: {}", e);
    }
}

fn extract_urgency(hints: &glib::Variant) -> Urgency {
    // Look for "urgency" key in the a{sv} dict
    for i in 0..hints.n_children() {
        let entry = hints.child_value(i);
        let key: Option<String> = entry.child_value(0).get();
        if key.as_deref() == Some("urgency") {
            let val = entry.child_value(1).child_value(0);
            if let Some(u) = val.get::<u8>() {
                return Urgency::from(u);
            }
        }
    }
    Urgency::Normal
}

fn extract_string_hint(hints: &glib::Variant, key_name: &str) -> Option<String> {
    for i in 0..hints.n_children() {
        let entry = hints.child_value(i);
        let key: Option<String> = entry.child_value(0).get();
        if key.as_deref() == Some(key_name) {
            let val = entry.child_value(1).child_value(0);
            return val.get::<String>();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_count_to_u32_passes_through_small_values() {
        assert_eq!(unread_count_to_u32(0), 0);
        assert_eq!(unread_count_to_u32(1), 1);
        assert_eq!(unread_count_to_u32(42), 42);
    }

    #[test]
    fn unread_count_to_u32_passes_through_u32_max() {
        // u32::MAX as usize is always representable on every supported target.
        assert_eq!(unread_count_to_u32(u32::MAX as usize), u32::MAX);
    }

    /// Overflow only exists on targets where `usize > u32` (i.e. 64-bit).
    /// On 32-bit hosts `usize` *is* `u32`, so `try_from` can't fail and this
    /// test would be tautological.
    #[cfg(target_pointer_width = "64")]
    #[test]
    fn unread_count_to_u32_clamps_on_overflow() {
        assert_eq!(unread_count_to_u32(u32::MAX as usize + 1), u32::MAX);
        assert_eq!(unread_count_to_u32(usize::MAX), u32::MAX);
    }

    #[test]
    fn is_unknown_method_error_recognises_dbus_unknown_method() {
        let err = glib::Error::new(gio::DBusError::UnknownMethod, "method missing");
        assert!(is_unknown_method_error(&err));
    }

    #[test]
    fn is_unknown_method_error_rejects_other_dbus_errors() {
        let err = glib::Error::new(gio::DBusError::NoMemory, "out of memory");
        assert!(!is_unknown_method_error(&err));
    }

    #[test]
    fn server_info_tuple_uses_cargo_pkg_version() {
        let (name, vendor, version, spec) = server_info_tuple();
        assert_eq!(name, "nwg-notifications");
        assert_eq!(vendor, "nwg-notifications");
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
        assert_eq!(spec, "1.2");
    }
}
