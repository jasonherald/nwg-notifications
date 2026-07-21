//! D-Bus integration tests (#16). Every test is #[ignore]d: they need an
//! isolated session bus and are run ONLY via `make test-integration`,
//! which wraps them in dbus-run-session. NEVER run them against the real
//! session bus — they own org.nwg.Notifications and would fight the live
//! daemon. Single-threaded (--test-threads=1 in the make target) because
//! the tests share that one well-known name.

use gtk4::{gio, glib};
use nwg_notifications::dbus::{NWG_COUNT_BUS_NAME, NWG_COUNT_OBJECT_PATH};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Minimal introspection XML for the fixture: just the surface the
/// tests exercise, mirroring the daemon's NWG_COUNT_INTROSPECT_XML.
const FIXTURE_XML: &str = r#"
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

/// A fixture daemon owning org.nwg.Notifications on the isolated bus.
///
/// Runs on its OWN thread with a private MainContext + MainLoop. This is
/// not optional: `query_count_via_dbus` uses `call_sync`, which blocks
/// the calling thread and pumps a private internal context for the
/// reply. A fixture registered on the calling thread's context would
/// never get its method handler dispatched while the call blocks —
/// every round-trip would falsely time out.
struct CountFixture {
    main_loop: glib::MainLoop,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CountFixture {
    /// `hang: true` registers a GetCount handler that never answers
    /// (parks the invocation), for the timeout test.
    fn spawn(count: u32, hang: bool) -> Self {
        let (ready_tx, ready_rx) = mpsc::channel::<glib::MainLoop>();
        let thread = std::thread::spawn(move || {
            let ctx = glib::MainContext::new();
            ctx.with_thread_default(|| {
                // Callbacks from bus_own_name land on the thread-default
                // context at call time, i.e. `ctx`.
                let parked: std::rc::Rc<std::cell::RefCell<Vec<gio::DBusMethodInvocation>>> =
                    std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                let main_loop = glib::MainLoop::new(Some(&ctx), false);
                let loop_for_acquired = main_loop.clone();
                let ready_tx_acquired = ready_tx.clone();
                let owner_id = gio::bus_own_name(
                    gio::BusType::Session,
                    NWG_COUNT_BUS_NAME,
                    gio::BusNameOwnerFlags::NONE,
                    move |connection, _name| {
                        let node =
                            gio::DBusNodeInfo::for_xml(FIXTURE_XML).expect("fixture XML parses");
                        let iface = node
                            .lookup_interface(NWG_COUNT_BUS_NAME)
                            .expect("fixture interface present");
                        let parked = std::rc::Rc::clone(&parked);
                        connection
                            .register_object(NWG_COUNT_OBJECT_PATH, &iface)
                            .method_call(
                                move |_conn,
                                      _sender,
                                      _path,
                                      _iface,
                                      method,
                                      _params,
                                      invocation| {
                                    assert_eq!(
                                        method, "GetCount",
                                        "fixture only implements GetCount"
                                    );
                                    if hang {
                                        // Park forever: the client must hit
                                        // QUERY_COUNT_TIMEOUT_MS on its own.
                                        parked.borrow_mut().push(invocation);
                                    } else {
                                        invocation.return_value(Some(&(count,).into()));
                                    }
                                },
                            )
                            .build()
                            .expect("fixture object registers");
                    },
                    move |_conn, _name| {
                        // Name acquired: hand the main loop to the test
                        // thread so it knows the fixture is live.
                        let _ = ready_tx_acquired.send(loop_for_acquired.clone());
                    },
                    |_conn, _name| {
                        // Log-only: panicking in a glib callback during
                        // teardown aborts the whole test binary.
                        eprintln!("fixture lost org.nwg.Notifications mid-test");
                    },
                );
                main_loop.run();
                gio::bus_unown_name(owner_id);
                // One last spin so the unown reaches the bus daemon
                // before the next test asserts on name absence.
                while ctx.iteration(false) {}
            })
            .expect("fixture thread acquires its private main context");
        });
        let main_loop = ready_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("fixture failed to acquire org.nwg.Notifications within 10s");
        CountFixture {
            main_loop,
            thread: Some(thread),
        }
    }
}

impl Drop for CountFixture {
    fn drop(&mut self) {
        // quit() is invoked cross-thread (test thread → fixture loop);
        // g_main_loop_quit is thread-safe and wakes the fixture's context.
        self.main_loop.quit();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Iterates `ctx` until `cond` returns true or `deadline` passes.
/// Deadlines are generous — GitHub runners are slow and shared; tight
/// bounds are how CI gets flaky.
fn pump_until(ctx: &glib::MainContext, deadline: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < deadline {
        while ctx.iteration(false) {}
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Subscribes to one signal on `connection` and returns a closure that
/// yields the first received payload, pumping `ctx` up to `deadline`.
/// The caller must have pushed `ctx` as the thread-default context
/// before subscribing so that signal callbacks land on it.
fn catch_signal(
    connection: &gio::DBusConnection,
    object_path: &str,
    interface: &str,
    member: &str,
    ctx: glib::MainContext,
) -> impl FnMut(Duration) -> Option<glib::Variant> {
    let received: std::rc::Rc<std::cell::RefCell<Option<glib::Variant>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let sink = std::rc::Rc::clone(&received);
    let sub = connection.subscribe_to_signal(
        None,
        Some(interface),
        Some(member),
        Some(object_path),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            *sink.borrow_mut() = Some(signal.parameters.clone());
        },
    );
    // `sub` (SignalSubscription) must be captured by the closure — dropping it
    // calls signal_unsubscribe immediately, which would race the emit.
    move |deadline| {
        let _keep = &sub;
        pump_until(&ctx, deadline, || received.borrow().is_some());
        received.borrow_mut().take()
    }
}

#[test]
#[ignore = "needs isolated session bus — run via `make test-integration`"]
fn get_count_round_trip() {
    let _fixture = CountFixture::spawn(7, false);
    let count = nwg_notifications::dbus::query_count_via_dbus()
        .expect("GetCount round-trip against the fixture");
    assert_eq!(count, 7);
}

#[test]
#[ignore = "needs isolated session bus — run via `make test-integration`"]
fn get_count_no_daemon_errors() {
    // No fixture is constructed here: nothing owns the name on the
    // isolated bus (relies on RAII fixture teardown in the other tests
    // having released it), and dbus-run-session registers no service
    // files, so nothing can activate either — the bus reports
    // NameHasNoOwner. Exercising NO_AUTO_START's activation-suppression
    // specifically would need a service file on the isolated bus.
    let err = nwg_notifications::dbus::query_count_via_dbus()
        .expect_err("NO_AUTO_START with no owner must error, not activate");
    assert!(
        err.matches(gio::DBusError::NameHasNoOwner),
        "expected NameHasNoOwner, got: {err}"
    );
}

#[test]
#[ignore = "needs isolated session bus — run via `make test-integration`"]
fn get_count_times_out_on_hung_daemon() {
    let _fixture = CountFixture::spawn(0, true);
    let start = Instant::now();
    let err = nwg_notifications::dbus::query_count_via_dbus()
        .expect_err("a hung daemon must produce a timeout error");
    let elapsed = start.elapsed();
    assert!(
        err.matches(gio::IOErrorEnum::TimedOut),
        "expected TimedOut, got: {err}"
    );
    // Sanity-bound the elapsed window around QUERY_COUNT_TIMEOUT_MS
    // (2s). Generous upper bound: shared CI runners stall.
    assert!(
        elapsed >= Duration::from_millis(1_500),
        "returned before the timeout window: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(8),
        "took far longer than QUERY_COUNT_TIMEOUT_MS ({}) allows: {elapsed:?}",
        nwg_notifications::dbus::QUERY_COUNT_TIMEOUT_MS
    );
}

#[test]
#[ignore = "needs isolated session bus — run via `make test-integration`"]
fn emit_count_changed_wire_payload() {
    let ctx = glib::MainContext::new();
    ctx.with_thread_default(|| {
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
            .expect("isolated session bus reachable");
        let mut recv = catch_signal(
            &connection,
            NWG_COUNT_OBJECT_PATH,
            NWG_COUNT_BUS_NAME,
            "CountChanged",
            ctx.clone(),
        );

        nwg_notifications::dbus::emit_count_changed(&connection, 42);

        let params = recv(Duration::from_secs(5)).expect("CountChanged not received within 5s");
        let count: u32 = params.child_value(0).get().expect("payload arg0 is u32");
        assert_eq!(count, 42);
    })
    .expect("test thread acquires private main context");
}

#[test]
#[ignore = "needs isolated session bus — run via `make test-integration`"]
fn emit_action_invoked_wire_payload() {
    let ctx = glib::MainContext::new();
    ctx.with_thread_default(|| {
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
            .expect("isolated session bus reachable");
        let mut recv = catch_signal(
            &connection,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "ActionInvoked",
            ctx.clone(),
        );

        nwg_notifications::dbus::emit_action_invoked(&connection, 7, "default");

        let params = recv(Duration::from_secs(5)).expect("ActionInvoked not received within 5s");
        let id: u32 = params.child_value(0).get().expect("payload arg0 is u32");
        let action: String = params.child_value(1).get().expect("payload arg1 is String");
        assert_eq!((id, action.as_str()), (7, "default"));
    })
    .expect("test thread acquires private main context");
}
