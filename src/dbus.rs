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
  </interface>
</node>
"#;

/// D-Bus name for the nwg-specific count IPC interface.
pub const NWG_COUNT_BUS_NAME: &str = "org.nwg.Notifications";
/// D-Bus object path for the nwg-specific count IPC interface.
pub const NWG_COUNT_OBJECT_PATH: &str = "/org/nwg/Notifications";

/// Callback invoked when a new notification arrives via D-Bus.
/// Implement this to show popups, update waybar, etc.
pub type OnNotify = Rc<dyn Fn(&Notification)>;

/// Callback invoked when a notification is closed via D-Bus.
pub type OnClose = Rc<dyn Fn(u32)>;

/// Registers the notification D-Bus server on the session bus.
///
/// Runs entirely on the glib main loop — no threads or async needed.
/// Acquires both `org.freedesktop.Notifications` (the standard notification
/// daemon interface) and `org.nwg.Notifications` (the nwg-specific count IPC).
pub fn register_server(
    state: &Rc<RefCell<NotificationState>>,
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
    gio::bus_own_name(
        gio::BusType::Session,
        NWG_COUNT_BUS_NAME,
        gio::BusNameOwnerFlags::REPLACE,
        move |connection, _name| {
            log::info!("Acquired D-Bus name: {}", NWG_COUNT_BUS_NAME);
            register_nwg_count_object(&connection, &state_nwg);
        },
        |_connection, _name| {
            log::debug!("nwg-count D-Bus name acquired callback");
        },
        |_connection, _name| {
            log::error!("Lost D-Bus name {} — another daemon?", NWG_COUNT_BUS_NAME);
        },
    );
}

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
        }
    }
}

fn register_nwg_count_object(
    connection: &gio::DBusConnection,
    state: &Rc<RefCell<NotificationState>>,
) {
    let node_info = gio::DBusNodeInfo::for_xml(NWG_COUNT_INTROSPECT_XML)
        .expect("Failed to parse nwg-count introspection XML");

    let interface_info = node_info
        .lookup_interface(NWG_COUNT_BUS_NAME)
        .expect("nwg-count interface not found in XML");

    let state = Rc::clone(state);

    connection
        .register_object(NWG_COUNT_OBJECT_PATH, &interface_info)
        .method_call(
            move |_conn, _sender, _path, _iface, method, _params, invocation| {
                handle_nwg_count_method(method, invocation, &state);
            },
        )
        .build()
        .expect("Failed to register nwg-count D-Bus object");
}

fn handle_nwg_count_method(
    method: &str,
    invocation: gio::DBusMethodInvocation,
    state: &Rc<RefCell<NotificationState>>,
) {
    match method {
        "GetCount" => {
            let count = unread_count_to_u32(state.borrow().unread_count());
            let result = glib::Variant::from((count,));
            invocation.return_value(Some(&result));
        }
        _ => {
            log::warn!("Unknown nwg-count D-Bus method: {}", method);
            invocation.return_dbus_error(
                "org.freedesktop.DBus.Error.UnknownMethod",
                &format!("Unknown method: {method}"),
            );
        }
    }
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

fn handle_server_info(invocation: gio::DBusMethodInvocation) {
    let info = ("nwg-notifications", "nwg-dock-hyprland", "0.1.0", "1.2");
    let variant = glib::Variant::from(info);
    invocation.return_value(Some(&variant));
}

/// Emits the ActionInvoked D-Bus signal to the sending app.
pub fn emit_action_invoked(connection: &gio::DBusConnection, id: u32, action_key: &str) {
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
pub fn unread_count_to_u32(unread: usize) -> u32 {
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
pub fn query_count_via_dbus() -> Result<u32, glib::Error> {
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

/// Emits CountChanged on the org.nwg.Notifications interface.
///
/// Best-effort: a failure here doesn't affect anything else; we log and move on.
pub fn emit_count_changed(connection: &gio::DBusConnection, count: u32) {
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
}
