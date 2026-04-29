# Pending Notification Count IPC Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose the current pending (unread) notification count via three IPC mechanisms so `nwg-panel` and similar consumers can build a notification-count widget on top of `nwg-notifications`: (1) a new `org.nwg.Notifications` D-Bus interface with `GetCount()` + `CountChanged(u32)` signal, (2) a `count` field added to the existing waybar status JSON, and (3) a `nwg-notifications --count` CLI subcommand for shell scripts.

**Architecture:** Add a sibling D-Bus interface alongside the existing `org.freedesktop.Notifications` server — same connection, second `bus_own_name` for the nwg-specific name, second `register_object` at `/org/nwg/Notifications`. Hook `CountChanged` emission into the existing `on_state_change` callback with delta-tracking (only emit when the count actually changes). Extend the waybar status struct with a `count: usize` field. The `--count` CLI flag short-circuits before GTK/singleton-lock setup and runs as a thin D-Bus client that calls `GetCount()` with `NO_AUTO_START` so it doesn't spawn a daemon.

**Tech Stack:** Rust, `gio` D-Bus (server + client), `clap`, `serde_json`, existing `nwg_common::singleton`.

**Tracks:** Issue [#9](https://github.com/jasonherald/nwg-notifications/issues/9) (part of epic #8). User confirmations on design (in conversation): D-Bus name `org.nwg.Notifications` ✓, CLI subcommand in scope ✓, delta-tracking emission strategy ✓.

---

## File Structure

- **Modify:** `src/waybar.rs` — add `count: usize` field to `WaybarStatus`; populate from `unread_count()`. Add a small unit test asserting JSON contains the field.
- **Modify:** `src/dbus.rs` — add a second introspection XML constant for `org.nwg.Notifications`, second `bus_own_name` + `register_object` call, `handle_get_count` method, `emit_count_changed` signal helper, and `query_count_via_dbus()` client function for the CLI.
- **Modify:** `src/config.rs` — add `--count` boolean flag; tests for parse.
- **Modify:** `src/main.rs` — early branch: if `config.count`, call `dbus::query_count_via_dbus()` and exit; otherwise run the daemon. Wire delta-tracking `Rc<Cell<u32>>` into the `on_state_change` callback so `CountChanged` only fires on actual deltas.
- **Modify:** `CHANGELOG.md` — entry under unreleased section.
- **Modify:** `README.md` — new "Querying notification count" section with examples for all three mechanisms.

No new files. All logic stays in existing modules; `dbus.rs` is the natural home for both the server-side and the thin client-side D-Bus calls.

---

## Pre-flight

- [ ] **Confirm working directory and branch**

```bash
cd /data/source/nwg-notifications
git status
git checkout main && git pull --ff-only
git checkout -b feat/pending-count-ipc
```

Expected: clean tree on `main` synced to origin, then a fresh branch `feat/pending-count-ipc`. If the tree isn't clean, stop and ask.

- [ ] **Commit the plan file as the first commit on the branch**

```bash
git add docs/superpowers/plans/2026-04-28-pending-count-ipc.md
git commit -m "docs: implementation plan for pending count IPC (#9)"
```

- [ ] **Baseline full cargo gambit before any change**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
cargo audit
```

Or `make lint`. Expected: every step exits 0. `cargo deny` may print pre-existing warnings about stale `deny.toml` skip entries — non-blocking, out of scope.

---

## Task 1: Add `count` field to waybar status JSON

The smallest, most isolated piece. Lands first so the rest of the work doesn't have to thread `count` into a struct that doesn't accept it yet.

**Files:**
- Modify: `src/waybar.rs` (add field + populate it; add a `#[cfg(test)] mod tests` block at the bottom)

- [ ] **Step 1: Write the failing test**

Append to the bottom of `src/waybar.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_json_includes_count_field() {
        let status = WaybarStatus {
            text: "x".into(),
            tooltip: "t".into(),
            alt: "a".into(),
            class: "c".into(),
            count: 7,
        };
        let json = serde_json::to_string(&status).expect("serialize");
        assert!(
            json.contains("\"count\":7"),
            "expected count field in JSON, got: {json}"
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test waybar:: 2>&1 | tail -10`

Expected: compile error — `WaybarStatus` doesn't have a `count` field yet.

- [ ] **Step 3: Add the field to the struct**

In `src/waybar.rs`, modify `WaybarStatus`:

```rust
#[derive(Serialize)]
struct WaybarStatus {
    text: String,
    tooltip: String,
    alt: String,
    class: String,
    count: usize,
}
```

- [ ] **Step 4: Populate `count` in `update_status`**

The function signature already takes `unread: usize`. Plumb it into every WaybarStatus literal:

```rust
pub fn update_status(unread: usize, dnd: bool) {
    let status = if dnd {
        WaybarStatus {
            text: "\u{f06d9}".into(),
            tooltip: "Do Not Disturb".into(),
            alt: "dnd".into(),
            class: "dnd".into(),
            count: unread,
        }
    } else if unread > 0 {
        WaybarStatus {
            text: format!("\u{f009a} {unread}"),
            tooltip: format!(
                "{unread} unread notification{}",
                if unread == 1 { "" } else { "s" }
            ),
            alt: "unread".into(),
            class: "unread".into(),
            count: unread,
        }
    } else {
        WaybarStatus {
            text: "\u{f009c}".into(),
            tooltip: "No notifications".into(),
            alt: "empty".into(),
            class: "empty".into(),
            count: 0,
        }
    };

    let path = status_path();
    match serde_json::to_string(&status) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::error!("Failed to write waybar status: {}", e);
            }
        }
        Err(e) => log::error!("Failed to serialize waybar status: {}", e),
    }

    signal_waybar();
}
```

Note that even in the DND branch, we report the unread count — DND only affects whether popups *show*, not whether notifications exist. nwg-panel widgets that read `count` should reflect that.

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test waybar:: 2>&1 | tail -10`

Expected: `status_json_includes_count_field` passes; nothing else broke.

- [ ] **Step 6: Run the full local checks**

Run: `cargo build && cargo test && cargo clippy --all-targets -- -D warnings`

Expected: clean across the board.

- [ ] **Step 7: Commit**

```bash
git add src/waybar.rs
git commit -m "$(cat <<'EOF'
Add count field to waybar status JSON (#9)

nwg-panel and other consumers can now read the pending notification
count from $XDG_RUNTIME_DIR/mac-notifications-status.json without going
through D-Bus.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Add `org.nwg.Notifications` D-Bus interface with `GetCount`

The first half of the D-Bus surface. We add a second introspection XML, a second `bus_own_name`, register a new object at `/org/nwg/Notifications`, and dispatch `GetCount()`. Signal emission comes in Task 3.

**Files:**
- Modify: `src/dbus.rs`

- [ ] **Step 1: Read the existing dbus.rs structure as orientation**

Run: `wc -l src/dbus.rs && grep -n '^fn \|^pub fn \|^const ' src/dbus.rs`

Confirm you understand: the existing `org.freedesktop.Notifications` server lives entirely in this file; `register_server` is the entry point and `register_object` does the per-interface dispatch wiring.

- [ ] **Step 2: Add the new introspection XML constant**

After the existing `INTROSPECT_XML` constant, add:

```rust
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
```

The constants are `pub` so the CLI client (Task 4) can use them without re-declaring strings.

- [ ] **Step 3: Acquire the second bus name in `register_server`**

Inside `register_server`, after the existing `gio::bus_own_name(...)` call for `org.freedesktop.Notifications`, add a second one for the nwg name. Keep the same pattern. The closure clones what it needs:

```rust
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
```

- [ ] **Step 4: Add `register_nwg_count_object` and the `GetCount` handler**

Add this function alongside the existing `register_object` in `src/dbus.rs`:

```rust
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
        .method_call(move |_conn, _sender, _path, _iface, method, _params, invocation| {
            handle_nwg_count_method(method, invocation, &state);
        })
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
            let count = state.borrow().unread_count() as u32;
            let result = glib::Variant::from((count,));
            invocation.return_value(Some(&result));
        }
        _ => {
            log::warn!("Unknown nwg-count D-Bus method: {}", method);
        }
    }
}
```

- [ ] **Step 5: Build and run all tests**

Run: `cargo build && cargo test && cargo clippy --all-targets -- -D warnings`

Expected: clean. No new tests yet — the introspection XML is only meaningfully testable in an integration setting; the parse step happens at runtime via `for_xml` and would panic on bad XML.

- [ ] **Step 6: Live D-Bus check**

This is *not* committed but is worth running locally to catch silly mistakes before the smoke test gate:

```bash
# Build and start a one-off daemon (no --persist):
cargo run -- --debug &
DAEMON_PID=$!
sleep 1

gdbus call --session \
  --dest org.nwg.Notifications \
  --object-path /org/nwg/Notifications \
  --method org.nwg.Notifications.GetCount

# Expected output: (uint32 0,)   # zero notifications outstanding

kill $DAEMON_PID
```

If `gdbus call` errors with "No such interface" or "Object does not exist", the registration is wrong — re-check Step 3 / Step 4. If it returns a uint32, you're good.

- [ ] **Step 7: Commit**

```bash
git add src/dbus.rs
git commit -m "$(cat <<'EOF'
Add org.nwg.Notifications D-Bus interface with GetCount (#9)

Adds a sibling D-Bus name and object alongside the existing
org.freedesktop.Notifications server. GetCount() returns the current
unread count. Signal CountChanged is declared in introspection XML; emission
wired up in the next commit.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `CountChanged` signal with delta-tracking emission

**Files:**
- Modify: `src/dbus.rs` — add `emit_count_changed` helper.
- Modify: `src/main.rs` — thread an `Rc<Cell<u32>>` into the `on_state_change` callback; emit when the count changes.

- [ ] **Step 1: Add `emit_count_changed` helper to `src/dbus.rs`**

Add next to the existing `emit_action_invoked`:

```rust
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
```

- [ ] **Step 2: Wire delta-tracking into `build_state_change_callback`**

In `src/main.rs`, the existing `build_state_change_callback` returns the `Rc<dyn Fn()>` that fires on every state mutation. We extend it to track the last emitted count and only emit when it changes.

Add the import at the top of `src/main.rs` (alongside `use std::cell::RefCell;`):

```rust
use std::cell::Cell;
```

Then change `build_state_change_callback` to:

```rust
fn build_state_change_callback(
    state: &Rc<RefCell<NotificationState>>,
    persist: bool,
    history_path: std::path::PathBuf,
) -> Rc<dyn Fn()> {
    let state_sync = Rc::clone(state);
    let last_emitted_count: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    Rc::new(move || {
        let s = state_sync.borrow();
        let count = s.unread_count() as u32;
        waybar::update_status(s.unread_count(), s.dnd);
        if persist {
            persistence::save_history(&history_path, &s.history);
        }
        if count != last_emitted_count.get()
            && let Some(conn) = &s.dbus_connection
        {
            last_emitted_count.set(count);
            dbus::emit_count_changed(conn, count);
        }
    })
}
```

**On the conditional ordering:** the chain is `count != last_emitted && Some(conn) = ...`. If the bus connection isn't ready yet (early startup, before `bus_own_name` resolves), the second clause short-circuits, `last_emitted_count` is *not* updated, and the next state change re-evaluates. That's the intended behavior: the first state change after the bus name is acquired emits unconditionally (because `last_emitted_count` is still its initial `0`), so any subscriber that comes online during startup gets the current count without polling.

The only tiny edge case: if the daemon starts with `count == 0` and stays at 0 across the bus-acquisition boundary, no signal fires. That's also fine — subscribers can call `GetCount()` once at subscribe time to get the initial value.

- [ ] **Step 3: Add a unit test for the comparator's predicate**

Strict TDD against a private function would be ideal, but the comparator is closed over inside the callback — it's not a separate function. Instead add a small integration-flavored unit test in `src/dbus.rs` that exercises the delta predicate logic separately, by extracting it:

In `src/main.rs`, just above `build_state_change_callback`, add:

```rust
/// Returns true if the count has changed since `last_emitted` and should
/// trigger a CountChanged signal. Pure helper to keep the predicate
/// unit-testable.
fn should_emit_count_changed(last_emitted: u32, current: u32) -> bool {
    last_emitted != current
}
```

And use it inside the callback in place of the inline `!=`:

```rust
        if should_emit_count_changed(last_emitted_count.get(), count)
            && let Some(conn) = &s.dbus_connection
        {
            last_emitted_count.set(count);
            dbus::emit_count_changed(conn, count);
        }
```

Then add tests near the bottom of `src/main.rs` (it does not currently have a `#[cfg(test)] mod tests` block — create one):

```rust
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
}
```

- [ ] **Step 4: Run tests and clippy**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`

Expected: 2 new tests pass; everything else still passes. clippy clean.

- [ ] **Step 5: Live D-Bus signal check**

```bash
cargo run -- --debug &
DAEMON_PID=$!
sleep 1

# In a second terminal (or background):
dbus-monitor --session "type='signal',interface='org.nwg.Notifications'" &
MONITOR_PID=$!
sleep 0.5

# Trigger a count delta:
notify-send "test" "should bump the count to 1"
sleep 1

# Expected: dbus-monitor prints a CountChanged signal with uint32 1.

kill $MONITOR_PID
kill $DAEMON_PID
```

If no signal appears, double-check the emit path (Step 1) and that `dbus_connection` is actually set on the state. Don't proceed if this fails — the smoke test won't reveal silent emission failures.

- [ ] **Step 6: Commit**

```bash
git add src/dbus.rs src/main.rs
git commit -m "$(cat <<'EOF'
Emit CountChanged signal on count deltas (#9)

Hooks emit_count_changed into the existing on_state_change callback
with delta-tracking via Rc<Cell<u32>> — only emits when the count
actually differs from the last emitted value, so e.g. mark_read on an
already-read notification stays quiet.

Pure should_emit_count_changed predicate is unit-tested.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `--count` CLI subcommand (D-Bus client mode)

**Files:**
- Modify: `src/config.rs` — add `count: bool` flag.
- Modify: `src/dbus.rs` — add `query_count_via_dbus()` client function.
- Modify: `src/main.rs` — early branch before singleton lock + GTK init.

- [ ] **Step 1: Add the `--count` flag with TDD**

Add to the bottom of `src/config.rs`'s `#[cfg(test)] mod tests` block:

```rust
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
```

Run: `cargo test count_flag 2>&1 | tail -10`. Expected: compile failure — `count` field doesn't exist on `NotificationConfig`.

Then add the field to `NotificationConfig`:

```rust
    /// Print the current pending notification count and exit.
    /// Useful in shell scripts; queries the running daemon over D-Bus
    /// (does not auto-start a daemon if none is running).
    #[arg(long)]
    pub count: bool,
```

Re-run: `cargo test count_flag`. Expected: both tests pass.

- [ ] **Step 2: Add `query_count_via_dbus` to `src/dbus.rs`**

Add at the bottom of `src/dbus.rs`:

```rust
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
        -1,
        gio::Cancellable::NONE,
    )?;
    let count: u32 = result.child_value(0).get().unwrap_or(0);
    Ok(count)
}
```

`NO_AUTO_START` is the key bit: without it, calling the method on a non-running daemon would auto-activate it via the D-Bus service file (heavyweight GTK startup just to print one integer). With it, the call fails fast with a sensible error.

- [ ] **Step 3: Add the early branch in `src/main.rs::main`**

At the very top of `main()`, after `nwg_common::process::handle_dump_args();` and `let config = NotificationConfig::parse();`, insert the count short-circuit *before* logger init, singleton lock, compositor init, and anything else:

```rust
fn main() {
    nwg_common::process::handle_dump_args();
    let config = NotificationConfig::parse();

    if config.count {
        match dbus::query_count_via_dbus() {
            Ok(count) => {
                println!("{}", count);
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("Failed to query count: {}", e);
                eprintln!("(is the nwg-notifications daemon running?)");
                std::process::exit(1);
            }
        }
    }

    if config.debug {
        // ... existing body unchanged from here
```

Important: `--count` runs *before* any side effect (logger init, singleton lock, GTK setup). It's a pure client invocation. Exit code 0 on success, 1 on failure.

- [ ] **Step 4: Run all tests and clippy**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: all tests pass; clippy clean.

- [ ] **Step 5: Live --count check**

```bash
# With a daemon already running (the dev session):
nwg-notifications --count
# Expected: an integer (probably 0 or whatever your unread count is).

# Verify the daemon is unaffected:
gdbus call --session --dest org.nwg.Notifications \
  --object-path /org/nwg/Notifications \
  --method org.nwg.Notifications.GetCount
# Expected: same integer, wrapped as (uint32 N,).

# With no daemon running:
kill "$(pidof nwg-notifications)" 2>/dev/null || true
sleep 0.5
nwg-notifications --count
echo "exit: $?"
# Expected: stderr message about D-Bus failure, exit 1.

# Restart the daemon for the user's session:
uwsm-app -- nwg-notifications --persist >/dev/null 2>&1 &
disown
```

(NEVER use `pkill -f nwg-notifications` here — it self-matches the bash subprocess and kills the user's live waybar daemon. Use `pidof` + `kill`.)

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/dbus.rs src/main.rs
git commit -m "$(cat <<'EOF'
Add --count CLI subcommand (#9)

Short-circuits before daemon initialization and queries the running
daemon over D-Bus with NO_AUTO_START, so shell scripts can read the
unread count without spawning a daemon they don't want.

Exits 0 with the count on stdout when a daemon responds; exits 1 with
a stderr error when no daemon is running.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Documentation

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `README.md`

- [ ] **Step 1: CHANGELOG entry**

Find the active `## [X.Y.Z] — Unreleased` section. Under its `### Added` block (creating it if missing), append:

```markdown
- Pending notification count IPC for nwg-panel and similar consumers (#9):
  - New `org.nwg.Notifications` D-Bus interface with `GetCount() -> u32`
    method and `CountChanged(u32)` signal (delta-only).
  - `count: usize` field added to the waybar status JSON at
    `$XDG_RUNTIME_DIR/mac-notifications-status.json`.
  - `nwg-notifications --count` CLI subcommand that queries the running
    daemon over D-Bus and prints the count to stdout (does not auto-start
    a daemon).
```

- [ ] **Step 2: README section**

Add a new H2 section to `README.md` (near the existing waybar integration section, or just above it). Title: "Querying notification count". Body:

````markdown
## Querying notification count

`nwg-notifications` exposes the current pending (unread) count via three
mechanisms — pick whichever fits the consumer.

### CLI

```bash
nwg-notifications --count
# Prints a single integer to stdout, e.g. `3`.
# Exits 1 with a stderr error if no daemon is running.
```

### D-Bus

```bash
gdbus call --session \
  --dest org.nwg.Notifications \
  --object-path /org/nwg/Notifications \
  --method org.nwg.Notifications.GetCount
```

Subscribers can also listen for the `org.nwg.Notifications.CountChanged`
signal to avoid polling:

```bash
dbus-monitor --session "type='signal',interface='org.nwg.Notifications'"
```

### Status file (waybar-friendly)

The daemon writes `$XDG_RUNTIME_DIR/mac-notifications-status.json` on every
state change. The file includes a `count` field:

```bash
jq -r .count "$XDG_RUNTIME_DIR/mac-notifications-status.json"
```

Combined with `SIGRTMIN+11` waybar refresh, this is zero-cost polling for
status-bar widgets.
````

(If the README already has a "Querying" or "Status file" section, integrate the new content there instead of duplicating.)

- [ ] **Step 3: Verify nothing else regressed**

```bash
cargo build && cargo test && cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md README.md
git commit -m "$(cat <<'EOF'
docs: pending count IPC (#9)

CHANGELOG entry under unreleased; new README section "Querying
notification count" with examples for CLI, D-Bus method/signal, and
status-file query paths.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: User smoke-test gate (HARD STOP)

The unit tests cover the JSON shape and the delta predicate, but not D-Bus dispatch over a real bus or the `--count` client against a live daemon. Pause for user verification before pushing.

- [ ] **Step 1: Install to the user's `~/.cargo/bin`**

```bash
make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
```

(Don't run `make install-dbus` — the freedesktop service file hasn't changed, and there's no service file for `org.nwg.Notifications` in this PR. The nwg name doesn't need a service file because the only consumer that *originates* a call is the `--count` subcommand, which uses `NO_AUTO_START` deliberately. nwg-panel will subscribe to a daemon that's already running for `org.freedesktop.Notifications`.)

- [ ] **Step 2: Restart the user's session daemon so it picks up the new D-Bus interface**

The live daemon owns the existing `org.freedesktop.Notifications` name but doesn't know about `org.nwg.Notifications`. Restart needed.

```bash
kill "$(pidof nwg-notifications)" 2>/dev/null || true
sleep 0.5
uwsm-app -- nwg-notifications --persist >/dev/null 2>&1 &
disown
sleep 1
pidof nwg-notifications && echo "(daemon up)"
```

- [ ] **Step 3: Hand off to the user — STOP HERE**

Tell the user (verbatim or close):
> Installed and restarted the daemon. Smoke-test paths:
> - `nwg-notifications --count` (CLI)
> - `gdbus call --session --dest org.nwg.Notifications --object-path /org/nwg/Notifications --method org.nwg.Notifications.GetCount` (D-Bus method)
> - `dbus-monitor --session "type='signal',interface='org.nwg.Notifications'" &` then `notify-send "x" "y"` (signal)
> - `jq .count "$XDG_RUNTIME_DIR/mac-notifications-status.json"` (status file)
> - `notify-send` then click the popup → count should drop, all four mechanisms should reflect the new value.
> - DND toggle (waybar right-click) shouldn't break any of the four.
> Reply when satisfied or with anything that needs fixing.

**Do not proceed to Task 7 until the user explicitly approves.** If the user reports issues, return to the task that owns the broken behavior, fix, re-install (back to Task 6 Step 1).

---

## Task 7: Full cargo gambit (CI parity)

- [ ] **Step 1: Run `make lint`**

```bash
make lint
```

Per `CLAUDE.md`, that's `fmt + clippy + test + deny + audit`. Fallback to running each piece directly if `make lint` is unavailable.

If `cargo fmt --check` reformats anything, run `cargo fmt --all`, commit as `style: cargo fmt`, and re-run `make lint`.

If `cargo deny` or `cargo audit` flags something *new* (not the pre-existing stale-skip warnings), stop and ask the user before suppressing.

- [ ] **Step 2: Confirm clean working tree**

```bash
git status
```

Expected: clean (after any fmt commit).

---

## Task 8: Open the PR

Gated on Task 6 (user smoke-test approval) AND Task 7 (clean `make lint`).

- [ ] **Step 1: Push the branch**

```bash
git push -u origin feat/pending-count-ipc
```

- [ ] **Step 2: Create the PR**

```bash
gh pr create --title "Add pending notification count IPC (#9)" --body "$(cat <<'EOF'
## Summary

Exposes the pending (unread) notification count via three IPC mechanisms so `nwg-panel` and similar consumers can build a notification-count widget on top of `nwg-notifications`:

1. **D-Bus** — new `org.nwg.Notifications` interface at `/org/nwg/Notifications` with `GetCount() -> u32` method and `CountChanged(u32)` signal. Sibling to the existing `org.freedesktop.Notifications` server; same daemon, separate name.
2. **Status file** — `count: usize` field added to `$XDG_RUNTIME_DIR/mac-notifications-status.json`. Combined with the existing `SIGRTMIN+11` waybar refresh, this gives zero-cost polling for status-bar widgets.
3. **CLI** — `nwg-notifications --count` short-circuits before daemon init and queries the running daemon with `NO_AUTO_START`, printing the integer to stdout. Useful in shell scripts.

`CountChanged` uses delta-tracking — emits only when the count actually changes — so e.g. `mark_read` on an already-read notification stays quiet on the bus.

Part of the [nwg-shell-config integration epic](https://github.com/jasonherald/nwg-notifications/issues/8). Closes #9.

## Test plan

- [x] `make lint` — fmt + clippy + test + deny + audit, all green locally.
- [x] Unit tests:
  - `WaybarStatus` JSON serialization includes `count` field.
  - `--count` clap flag parses correctly (default false, set to true with `--count`).
  - `should_emit_count_changed` delta predicate (emits on change, skips on equal).
- [x] Manual smoke test against the live compositor (installed via `make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin`):
  - [x] `nwg-notifications --count` prints the count.
  - [x] `gdbus call ... GetCount` returns the same count.
  - [x] `dbus-monitor` shows a `CountChanged` signal on `notify-send` and on click-to-mark-read.
  - [x] `jq .count $XDG_RUNTIME_DIR/mac-notifications-status.json` matches the D-Bus result.
  - [x] DND toggle doesn't break any of the four mechanisms.
  - [x] `--count` exits 1 with a sensible error when no daemon is running (via `NO_AUTO_START`).

## Notes

- Choice of name `org.nwg.Notifications` follows OG's recommendation in the issue — matches the consumer (`nwg-panel`) ecosystem naming. We don't formally own the `org.nwg.*` namespace; happy to revisit if anyone in nwg-piotr's project pushes back.
- No D-Bus service file is shipped for `org.nwg.Notifications`. The only client that originates a call is `--count`, which uses `NO_AUTO_START` deliberately. Subscribers like nwg-panel rely on a daemon already running for `org.freedesktop.Notifications`.

The implementation plan (committed as `docs/superpowers/plans/2026-04-28-pending-count-ipc.md`) is on the branch for reviewer context.
EOF
)"
```

Expected: returns the PR URL.

- [ ] **Step 3: Hand off to CodeRabbit**

CodeRabbit reviews within minutes. Iterate per the per-finding reply protocol: inline replies for in-diff comments, single PR-level comment for outside-diff items, tag `@coderabbitai` every time so it learns from the responses. Repeat until CodeRabbit approves (or user explicitly accepts remaining findings as won't-do).

---

## Acceptance checklist (cross-reference to issue #9)

- [ ] D-Bus method `GetCount()` returns the current pending count. — Task 2
- [ ] D-Bus signal `CountChanged(u32)` emits on every count delta. — Task 3 (delta-tracked, skips no-op state changes)
- [ ] Status JSON gains a `count` integer field. — Task 1
- [ ] OG can build the nwg-panel module against either mechanism without performance hit. — All three mechanisms verified in smoke test (Task 6).
- [ ] Documentation in the README explains both query paths. — Task 5 (covers all three: CLI, D-Bus method/signal, status file).
