# D-Bus Integration Tests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integration coverage for `query_count_via_dbus`, `emit_count_changed`, `emit_action_invoked`, and the hold-guard residency pattern, on an isolated session bus that works on GitHub-hosted runners (closes #16).

**Architecture:** Split the binary-only crate into lib + bin so `tests/` can link the code. Tests run `#[ignore]`d under a new `make test-integration` target that wraps them in `dbus-run-session` (isolated bus). The GetCount fixture runs on its own thread with a private `glib::MainContext` + `MainLoop` — a same-thread fixture would deadlock: `call_sync` pumps a private context while the fixture's method handler waits on the caller's context, so every call would falsely time out. The hold-guard liveness test spawns headless Sway + the real binary inside a throwaway `TMPDIR`/`XDG_RUNTIME_DIR` (isolates the singleton lock, which lives in `std::env::temp_dir()`, and the waybar status file) and runtime-skips when `sway` is absent.

**Tech Stack:** Rust, gio/glib (via `gtk4` re-exports), `dbus-run-session`, headless Sway (`WLR_BACKENDS=headless`), GitHub Actions `ubuntu-latest`.

**Spec:** `docs/superpowers/specs/2026-07-21-dbus-integration-tests-design.md`

**Safety rule (repeat everywhere):** NEVER run the ignored tests outside `dbus-run-session` — on a desktop they would hit the real session bus and the live daemon. The make target is the only supported entry point. Never `pkill` anything; the tests manage their own children.

---

### Task 1: Lib/bin split

**Files:**
- Create: `src/lib.rs`
- Create: `src/app.rs` (via `git mv src/main.rs src/app.rs` + edits)
- Create: `src/main.rs` (new shim)
- No `Cargo.toml` change: cargo auto-detects `src/lib.rs`; the existing explicit `[[bin]]` entry keeps pointing at `src/main.rs`. Lib crate name is `nwg_notifications` (hyphen→underscore).

- [ ] **Step 1: Move the coordinator**

```bash
git mv src/main.rs src/app.rs
```

- [ ] **Step 2: Edit `src/app.rs`**

Replace the file header + module declarations + `fn main()` signature. The old top of file is:

```rust
//! Coordinator: wires daemon state, popup manager, panel, D-Bus
//! server, and signal listener. Owns the GTK `Application` and the
//! short-circuit CLI modes (`--count`, `--update`) that exit before
//! claiming the singleton lock.

mod config;
mod config_file;
mod dbus;
mod listeners;
mod notification;
mod paths;
mod persistence;
mod state;
mod ui;
mod waybar;

use crate::config::NotificationConfig;
```

New top of file (module declarations move to `lib.rs`; everything from the first `use` down is untouched):

```rust
//! Coordinator: wires daemon state, popup manager, panel, D-Bus
//! server, and signal listener. Owns the GTK `Application` and the
//! short-circuit CLI modes (`--count`, `--update`) that exit before
//! claiming the singleton lock. Lives in the library (rather than
//! `main.rs`) so the integration suite in `tests/` links the same
//! code path; the `main.rs` shim just calls [`run`].

use crate::config::NotificationConfig;
```

Then change the entry-point signature only (body untouched):

```rust
// old:
fn main() {
// new:
/// Daemon + CLI entry point. Called by the `main.rs` shim.
pub fn run() {
```

`run` must be `pub` (not `pub(crate)`) so `lib.rs` can re-export it from the private `app` module. The `#[cfg(test)] mod tests` block at the bottom moves along with the file unchanged — its `use super::*` still resolves.

- [ ] **Step 3: Create `src/lib.rs`**

```rust
//! Library target for `nwg-notifications`.
//!
//! This is a binary-first project; the library target exists so the
//! integration suite in `tests/` has a linkable seam. It is **not a
//! public API**: everything is `#[doc(hidden)]`, semver guarantees do
//! not apply to the library surface, and items may change or vanish in
//! any release. Use the binary, not this library.

mod app;
mod config;
mod config_file;
mod dbus;
mod listeners;
mod notification;
mod paths;
mod persistence;
mod state;
mod ui;
mod waybar;

#[doc(hidden)]
pub use app::run;
```

- [ ] **Step 4: Create the new `src/main.rs`**

```rust
//! Binary shim. All coordinator logic lives in the library's `app`
//! module so the integration suite in `tests/` can link the same code —
//! see `src/lib.rs` for the no-public-API disclaimer on the library
//! target.

fn main() {
    nwg_notifications::run();
}
```

- [ ] **Step 5: Verify the split compiles and existing tests pass**

Run: `cargo build && cargo test`
Expected: build OK; all 109 unit tests pass (the 4 in the moved `app` module included). If anything fails it will be a visibility error from the move — fix by keeping items module-private (they were file-private before; same file, same access).

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor: split lib + bin so tests/ can link the crate

Coordinator moves verbatim from main.rs to app.rs in the new library
target; main.rs becomes a shim calling nwg_notifications::run(). The
lib exists solely as a linkable seam for the integration suite (#16)
and is documented as not-a-public-API."
```

---

### Task 2: Expose the test seams

**Files:**
- Modify: `src/lib.rs` (one line)
- Modify: `src/dbus.rs` (visibility on six items)

- [ ] **Step 1: Make the dbus module public**

In `src/lib.rs`: `mod dbus;` → `pub mod dbus;`

- [ ] **Step 2: Widen the six seam items in `src/dbus.rs`**

Each keeps its doc comment; add `#[doc(hidden)]` and flip to `pub`:

```rust
// constants (near line 89):
#[doc(hidden)]
pub const NWG_COUNT_BUS_NAME: &str = "org.nwg.Notifications";
#[doc(hidden)]
pub const NWG_COUNT_OBJECT_PATH: &str = "/org/nwg/Notifications";

// timeout (near line 627; currently private `const`):
#[doc(hidden)]
pub const QUERY_COUNT_TIMEOUT_MS: i32 = 2_000;

// functions (near lines 598, 651, 839):
#[doc(hidden)]
pub fn emit_action_invoked(connection: &gio::DBusConnection, id: u32, action_key: &str) {
#[doc(hidden)]
pub fn query_count_via_dbus() -> Result<u32, glib::Error> {
#[doc(hidden)]
pub fn emit_count_changed(connection: &gio::DBusConnection, count: u32) {
```

Everything else in `dbus.rs` stays `pub(crate)`/private.

- [ ] **Step 3: Verify**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: clean; 109 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/lib.rs src/dbus.rs
git commit -m "feat: expose doc(hidden) D-Bus test seams on the lib target

query_count_via_dbus, the two emit_* helpers, the org.nwg name/path
constants, and QUERY_COUNT_TIMEOUT_MS go pub for tests/; all marked
doc(hidden) per the lib's no-public-API stance."
```

---

### Task 3: Harness + make target + round-trip test (red-green)

**Files:**
- Create: `tests/dbus_integration.rs`
- Modify: `Makefile` (`.PHONY` line, `HELP_TEXT`, new target after `test:`)

- [ ] **Step 1: Write the harness support code + first test**

Create `tests/dbus_integration.rs`. The fixture's **own thread + private context** is load-bearing (see plan header). Initial fixture deliberately returns a **hardcoded `0`** — the first run must FAIL to prove the harness detects failures before we trust it in CI.

```rust
//! D-Bus integration tests (#16). Every test is #[ignore]d: they need an
//! isolated session bus and are run ONLY via `make test-integration`,
//! which wraps them in dbus-run-session. NEVER run them against the real
//! session bus — they own org.nwg.Notifications and would fight the live
//! daemon. Single-threaded (--test-threads=1 in the make target) because
//! the tests share that one well-known name.

use gtk4::prelude::*;
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

/// Iterates the invoking thread's default main context until `cond`
/// returns true or `deadline` passes. Deadlines are generous — GitHub
/// runners are slow and shared; tight bounds are how CI gets flaky.
fn pump_until(deadline: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let ctx = glib::MainContext::default();
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
                        let node = gio::DBusNodeInfo::for_xml(FIXTURE_XML)
                            .expect("fixture XML parses");
                        let iface = node
                            .lookup_interface(NWG_COUNT_BUS_NAME)
                            .expect("fixture interface present");
                        let parked = std::rc::Rc::clone(&parked);
                        connection
                            .register_object(NWG_COUNT_OBJECT_PATH, &iface)
                            .method_call(move |_conn, _sender, _path, _iface, method, _params, invocation| {
                                assert_eq!(method, "GetCount", "fixture only implements GetCount");
                                if hang {
                                    // Park forever: the client must hit
                                    // QUERY_COUNT_TIMEOUT_MS on its own.
                                    parked.borrow_mut().push(invocation);
                                } else {
                                    invocation.return_value(Some(&(0u32,).into()));
                                }
                            })
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
            });
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
        self.main_loop.quit();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
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
```

Note the deliberate bug: the handler returns `(0u32,)` and ignores `count`. Step 3 catches it.

- [ ] **Step 2: Add the make target**

`Makefile` changes — add `test-integration` to the `.PHONY` line:

```make
.PHONY: all build build-release test test-integration lint check-tools \
```

Add to `HELP_TEXT` after the `make test` line:

```
  make test-integration  D-Bus integration tests on an isolated bus (liveness test needs sway)
```

Add the target after the `test:` recipe:

```make
# Isolated-bus D-Bus integration tests (#16). dbus-run-session spawns a
# private session bus, exports DBUS_SESSION_BUS_ADDRESS to the child,
# and tears the bus down on exit — the desktop session's bus and the
# live daemon are never touched. --test-threads=1 because the tests own
# the one well-known org.nwg.Notifications name; parallel owners would
# collide. The hold-guard liveness test additionally needs `sway` on
# PATH and self-skips with a message when it's absent (e.g. GitHub
# runners). NEVER run the ignored tests outside dbus-run-session: on a
# desktop they would hit the real session bus.
test-integration:
	@command -v dbus-run-session >/dev/null 2>&1 || { \
		echo "ERROR: dbus-run-session not found — install your distro's dbus package"; \
		exit 1; \
	}
	dbus-run-session -- $(CARGO) test --test dbus_integration -- --ignored --test-threads=1
```

- [ ] **Step 3: Run — expect RED (harness must be able to fail)**

Run: `make test-integration`
Expected: `get_count_round_trip` FAILS with `assertion ... left: 0, right: 7`. This proves the isolated bus, fixture thread, name acquisition, and round-trip all work AND that a wrong answer is detected — the harness is trustworthy.

If it instead fails with a 2s stall and a timeout error, the fixture thread's context isn't receiving dispatch — re-check the `with_thread_default` block.

- [ ] **Step 4: Fix the fixture to honor `count` — expect GREEN**

In the method_call closure: `invocation.return_value(Some(&(0u32,).into()));` → `invocation.return_value(Some(&(count,).into()));`

Run: `make test-integration`
Expected: `test get_count_round_trip ... ok`, `1 passed`.

Also run: `cargo test`
Expected: unit tests pass, integration test listed as `ignored` — plain runs stay bus-free.

- [ ] **Step 5: Commit**

```bash
git add tests/dbus_integration.rs Makefile
git commit -m "feat: isolated-bus integration harness + GetCount round-trip test

dbus-run-session make target, own-thread fixture (private MainContext;
a same-thread fixture deadlocks against call_sync), red-green verified
end to end."
```

---

### Task 4: NO_AUTO_START and timeout tests

**Files:**
- Modify: `tests/dbus_integration.rs` (append two tests)

- [ ] **Step 1: Append both tests**

```rust
#[test]
#[ignore = "needs isolated session bus — run via `make test-integration`"]
fn get_count_no_daemon_errors() {
    // No fixture: nothing owns the name on the isolated bus. The call
    // uses NO_AUTO_START, so even a service file visible to the bus
    // must not spawn a daemon; the bus reports NameHasNoOwner.
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
```

The error-matching pattern (`err.matches(gio::DBusError::...)`) is the same one `dbus.rs::is_unknown_method_error` already uses.

- [ ] **Step 2: Run**

Run: `make test-integration`
Expected: `3 passed` (round-trip, no-daemon, timeout; the timeout test takes ~2s by design).

- [ ] **Step 3: Commit**

```bash
git add tests/dbus_integration.rs
git commit -m "test: NO_AUTO_START error path + hung-daemon timeout for GetCount"
```

---

### Task 5: Signal emission tests

**Files:**
- Modify: `tests/dbus_integration.rs` (append helper + two tests)

- [ ] **Step 1: Append the subscription helper and both tests**

These don't block in `call_sync`, so subscribing on the test thread's default context and pumping it is safe — no fixture thread involved.

```rust
/// Subscribes to one signal on `connection` and returns a closure that
/// yields the first received payload, pumping the default context up to
/// `deadline`. Subscription callbacks land on the subscribing thread's
/// default context, which pump_until iterates.
fn catch_signal(
    connection: &gio::DBusConnection,
    object_path: &str,
    interface: &str,
    member: &str,
) -> impl FnMut(Duration) -> Option<glib::Variant> {
    let received: std::rc::Rc<std::cell::RefCell<Option<glib::Variant>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let sink = std::rc::Rc::clone(&received);
    let _sub_id = connection.signal_subscribe(
        None,
        Some(interface),
        Some(member),
        Some(object_path),
        None,
        gio::DBusSignalFlags::NONE,
        move |_conn, _sender, _path, _iface, _signal, params| {
            *sink.borrow_mut() = Some(params.clone());
        },
    );
    move |deadline| {
        pump_until(deadline, || received.borrow().is_some());
        received.borrow_mut().take()
    }
}

#[test]
#[ignore = "needs isolated session bus — run via `make test-integration`"]
fn emit_count_changed_wire_payload() {
    let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .expect("isolated session bus reachable");
    let mut recv = catch_signal(
        &connection,
        NWG_COUNT_OBJECT_PATH,
        NWG_COUNT_BUS_NAME,
        "CountChanged",
    );

    nwg_notifications::dbus::emit_count_changed(&connection, 42);

    let params = recv(Duration::from_secs(5)).expect("CountChanged not received within 5s");
    let count: u32 = params.child_value(0).get().expect("payload arg0 is u32");
    assert_eq!(count, 42);
}

#[test]
#[ignore = "needs isolated session bus — run via `make test-integration`"]
fn emit_action_invoked_wire_payload() {
    let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .expect("isolated session bus reachable");
    let mut recv = catch_signal(
        &connection,
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "ActionInvoked",
    );

    nwg_notifications::dbus::emit_action_invoked(&connection, 7, "default");

    let params = recv(Duration::from_secs(5)).expect("ActionInvoked not received within 5s");
    let id: u32 = params.child_value(0).get().expect("payload arg0 is u32");
    let action: String = params.child_value(1).get().expect("payload arg1 is String");
    assert_eq!((id, action.as_str()), (7, "default"));
}
```

- [ ] **Step 2: Run**

Run: `make test-integration`
Expected: `5 passed`.

- [ ] **Step 3: Commit**

```bash
git add tests/dbus_integration.rs
git commit -m "test: CountChanged + ActionInvoked wire-payload assertions"
```

---

### Task 6: Hold-guard liveness test

**Files:**
- Modify: `tests/dbus_integration.rs` (append guard struct + test)

- [ ] **Step 1: Append the process guard and the test**

Isolation model: a `tempfile::TempDir` (mode 0700, which Wayland requires) becomes both `XDG_RUNTIME_DIR` (Sway's socket, daemon's status file) and `TMPDIR` (`nwg_common::singleton` builds its lock path from `std::env::temp_dir()`, which honors `TMPDIR`) — so the spawned daemon can't collide with the live desktop daemon's lock, status file, or compositor, and the isolated bus (inherited `DBUS_SESSION_BUS_ADDRESS`) keeps the D-Bus names private. `tempfile` is already a dependency.

```rust
/// Kills and reaps a child on drop — no orphan compositors or daemons
/// on dev machines, including when an assertion fails mid-test.
struct ProcGuard(std::process::Child);

impl Drop for ProcGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Polls `dir` for an entry whose name passes `pred`, up to `deadline`.
fn wait_for_entry(
    dir: &std::path::Path,
    deadline: Duration,
    pred: impl Fn(&str) -> bool,
) -> Option<String> {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if pred(&name) {
                    return Some(name);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

#[test]
#[ignore = "needs isolated session bus — run via `make test-integration`"]
fn daemon_stays_resident_hold_guard() {
    if !std::process::Command::new("sway")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        eprintln!("SKIP: sway not on PATH — hold-guard liveness test needs a headless compositor");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir for isolated XDG_RUNTIME_DIR/TMPDIR");

    let sway = std::process::Command::new("sway")
        .args(["--config", "/dev/null"])
        .env("XDG_RUNTIME_DIR", tmp.path())
        .env("WLR_BACKENDS", "headless")
        .env("WLR_LIBINPUT_NO_DEVICES", "1")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("SWAYSOCK")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn headless sway");
    let _sway_guard = ProcGuard(sway);

    let wayland_display = wait_for_entry(tmp.path(), Duration::from_secs(10), |n| {
        n.starts_with("wayland-") && !n.ends_with(".lock")
    })
    .expect("headless sway created a wayland socket within 10s");
    let swaysock = wait_for_entry(tmp.path(), Duration::from_secs(10), |n| {
        n.starts_with("sway-ipc.") && n.ends_with(".sock")
    })
    .expect("headless sway created its IPC socket within 10s");

    let daemon = std::process::Command::new(env!("CARGO_BIN_EXE_nwg-notifications"))
        .args(["--wm", "sway"])
        .env("XDG_RUNTIME_DIR", tmp.path())
        .env("TMPDIR", tmp.path())
        .env("WAYLAND_DISPLAY", &wayland_display)
        .env("SWAYSOCK", tmp.path().join(&swaysock))
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn nwg-notifications under headless sway");
    let mut daemon_guard = ProcGuard(daemon);

    // Past GTK init + activate + hold(). Without the hold guard,
    // GApplication exits as soon as activate returns idle — 3s is far
    // beyond that window.
    std::thread::sleep(Duration::from_secs(3));

    let status = daemon_guard.0.try_wait().expect("try_wait on daemon");
    assert!(
        status.is_none(),
        "daemon exited within 3s (status: {status:?}) — ApplicationHoldGuard is not keeping GApplication resident"
    );
}
```

`env!("CARGO_BIN_EXE_nwg-notifications")` is set by cargo for integration tests and guarantees the binary is built.

- [ ] **Step 2: Run the full suite locally (sway present)**

Run: `make test-integration`
Expected: `6 passed`. The liveness test takes ~5-15s (sway startup + settle).

- [ ] **Step 3: Verify the skip path (simulates GitHub runner)**

Run: `dbus-run-session -- env PATH=/usr/bin:/bin sh -c 'command -v sway >/dev/null && echo "sway visible — pick a PATH without it for this check" || true'` — if your sway lives in /usr/bin, instead verify the skip by reading the guard clause; the CI run (Task 7) is the authoritative skip-path check.
Expected: the guard clause returns early with the SKIP message when `sway --version` can't run.

- [ ] **Step 4: Commit**

```bash
git add tests/dbus_integration.rs
git commit -m "test: hold-guard liveness under headless sway, isolated TMPDIR/XDG_RUNTIME_DIR

Runtime-skips with a message when sway is absent (GitHub runners)."
```

---

### Task 7: CI job

**Files:**
- Modify: `.github/workflows/test.yml`

- [ ] **Step 1: Append the integration job**

```yaml
  integration:
    name: D-Bus integration tests
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4
      - uses: ./.github/actions/setup-gtk4
      - name: Install dbus-run-session
        run: |
          sudo apt-get update
          sudo apt-get install -y --no-install-recommends dbus dbus-bin
      - run: make test-integration
```

Same checkout pin and composite action as the existing `test` job (GTK dev headers are needed to compile the crate even though the bus tests never init GTK). `dbus`/`dbus-bin` are installed explicitly rather than assumed present on the runner image. The liveness test self-skips there (no sway) — by design per the spec's CI-scope decision.

- [ ] **Step 2: Verify locally what CI will run**

Run: `make test-integration` once more.
Expected: `6 passed` locally (CI will show 5 passed + the liveness SKIP message on stderr; both are success).

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/test.yml
git commit -m "ci: run the D-Bus integration suite on ubuntu-latest"
```

---

### Task 8: Docs + changelog

**Files:**
- Modify: `CLAUDE.md` (Build & test block + harness paragraph, Configuration section refs, What-lives-where tree)
- Modify: `CHANGELOG.md` (Added section under `## [0.5.1] — Unreleased`)

- [ ] **Step 1: CLAUDE.md Build & test block**

After the `make test` line add:

```
make test-integration         # D-Bus integration tests (isolated bus via dbus-run-session; liveness test needs sway)
```

Replace the harness paragraph (the one starting "Per [tests/integration/CLASSIFICATION.md]"):

```markdown
Per [tests/integration/CLASSIFICATION.md](https://github.com/jasonherald/mac-doc-hyprland/blob/main/tests/integration/CLASSIFICATION.md) in the monorepo, this repo owns daemon-launch + signal-resilience tests (SIGRTMIN+4 panel toggle, SIGRTMIN+5 DND toggle); those still run from the monorepo's headless-Sway harness. Locally, `make test-integration` (#16) runs the D-Bus suite in `tests/dbus_integration.rs` on an isolated bus via `dbus-run-session` — GetCount round-trip/`NO_AUTO_START`/timeout, `CountChanged` + `ActionInvoked` wire payloads, and a hold-guard liveness test that spawns the real binary under headless Sway (self-skips without sway, e.g. in CI). Never run the `#[ignore]`d tests outside the make target: on a desktop they'd hit the real session bus.
```

- [ ] **Step 2: CLAUDE.md structure references**

In the Configuration section, change both `main.rs::merge_cli_over_json` and `main.rs::apply_config_reload` to `app.rs::merge_cli_over_json` / `app.rs::apply_config_reload`.

In the "What lives where" tree, replace the `main.rs` line:

```text
├── main.rs            # Coordinator (~160 lines)
```

with:

```text
├── main.rs            # Binary shim → nwg_notifications::run()
├── lib.rs             # Lib target: test seam only, not a public API
├── app.rs             # Coordinator (CLI modes, GTK app, hold guard)
```

and add under the `src/` tree listing, after the `waybar.rs` line's section (i.e. as a sibling top-level entry in the same code block):

```text
tests/
└── dbus_integration.rs  # #[ignore]d isolated-bus suite (make test-integration)
```

- [ ] **Step 3: CHANGELOG**

Under `## [0.5.1] — Unreleased`, add an `### Added` section ABOVE the existing `### Fixed` (Keep-a-Changelog category order):

```markdown
### Added

- D-Bus integration test suite (#16): `make test-integration` runs
  GetCount round-trip / `NO_AUTO_START` / timeout coverage plus
  `CountChanged` and `ActionInvoked` wire-payload assertions on an
  isolated session bus via `dbus-run-session`, and a hold-guard
  liveness test under headless Sway (skips when `sway` is absent —
  CI runs the bus tests only). To give the suite a linkable seam the
  crate now ships a library target alongside the binary; the library
  surface is `#[doc(hidden)]`, is **not a public API**, and carries no
  semver guarantees.
```

Note for the release PR (not this one): the new lib target may argue for `0.6.0` over `0.5.1`.

- [ ] **Step 4: Verify + commit**

Run: `make lint`
Expected: exit 0 (docs don't affect it, but the gate is cheap).

```bash
git add CLAUDE.md CHANGELOG.md
git commit -m "docs: document make test-integration, lib/bin split, changelog entry (#16)"
```

---

### Task 9: Gates + PR

- [ ] **Step 1: Full local gates**

Run, in order, each expecting success:
- `make lint` → exit 0 (fmt, clippy `-D warnings` across lib+bin+tests, unit tests, deny, audit)
- `make test-integration` → 6 passed
- `make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin` → smoke-install for the user (do NOT restart the live daemon; the user decides)
- `~/.cargo/bin/nwg-notifications --version` → prints version (binary works post-split)
- `~/.cargo/bin/nwg-notifications --count` → prints the live daemon's unread count (real-world query_count_via_dbus still works after the seam changes)

- [ ] **Step 2: Push + PR**

```bash
git push -u origin feat/dbus-integration-tests
gh pr create --title "feat: D-Bus integration tests on an isolated bus" --body "..."
```

PR body: summary of the six tests + lib/bin split + CI job, testing evidence from Step 1, `Closes #16` (plain, no bold), and the Claude Code footer. Then the standard CodeRabbit cycle: monitor for the review, verify each finding, fix or push back, inline replies tagging @coderabbitai.

---

## Self-review notes (completed)

- **Spec coverage:** restructure → Task 1-2; harness/make → Task 3; acceptance tests → Tasks 3-6 (six tests inc. both signals); CI → Task 7; docs/changelog → Task 8; gates/PR → Task 9. The spec's "paths::status_path only if needed" — no test ended up needing it; not exposed (YAGNI).
- **Type consistency:** `CountFixture::spawn(count, hang)` used in Tasks 3/4; `pump_until`/`catch_signal`/`ProcGuard`/`wait_for_entry` defined before first use; seam names match Task 2 exactly.
- **Known adjust-on-contact points (not placeholders):** exact gio-rs closure arities (`signal_subscribe`, `method_call`) and `with_thread_default` signature may need small compiler-driven adjustments against gio 0.22 — the daemon's own `register_server` in `src/dbus.rs` is the in-repo reference for the register pattern.
