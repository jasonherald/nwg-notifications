# Live Config Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a runtime push mechanism for live-updatable config knobs so `nwg-shell-config` can change settings without restarting the daemon. Two layered surfaces: six per-knob D-Bus setter methods on `org.nwg.Notifications` (primary, what shell-config calls from Python), and a `nwg-notifications --update <flags>` CLI subcommand (thin client that wraps the D-Bus methods, useful for shell scripts and quick interactive testing).

**Architecture:** Wrap `NotificationConfig` in `Rc<RefCell<>>` so the daemon can mutate it at runtime. Per-knob D-Bus handlers validate inputs against the same ranges as the clap parser and either commit the change or return `org.freedesktop.DBus.Error.InvalidArgs`. The `--update` CLI uses `clap::ArgMatches::value_source()` to push only the flags the user explicitly set, never resetting other knobs to their defaults. Six knobs are live-updatable (popup-position, popup-width, panel-width, popup-timeout, max-popups, max-history); inherently startup-only flags (`--persist`, `--wm`, `--debug`) are explicitly excluded.

**Tech Stack:** Rust, `gio` D-Bus (server + client), `clap` (derive + ArgMatches::value_source), `gtk4` widget sizing.

**Tracks:** Closes [#20](https://github.com/jasonherald/nwg-notifications/issues/20). Surfaced from in-progress conversation with OG on [#2](https://github.com/jasonherald/nwg-notifications/issues/2#issuecomment-4347480676). After this lands, Jason replies to OG on #2 with the new surface.

---

## File Structure

- **Modify:** `src/main.rs` — wrap config in `Rc<RefCell<>>`; add `--update` early branch before daemon init; thread the new wrapped config into `PopupManager`, `NotificationPanel`, and the D-Bus registration.
- **Modify:** `src/ui/popup.rs` — change `config: Rc<NotificationConfig>` to `Rc<RefCell<NotificationConfig>>`; all reads become `self.config.borrow().<field>`.
- **Modify:** `src/ui/panel.rs` — revisit the #12 YAGNI: replace the `panel_width: i32` constructor parameter with `Rc<RefCell<NotificationConfig>>`. Apply `panel_width` from config on every toggle so width changes pick up on the next open. Add `panel_box` as a struct field so we can update both `win` and `panel_box` widgets together.
- **Modify:** `src/dbus.rs` — extend `NWG_COUNT_INTROSPECT_XML` with 6 new setter methods; extend `register_nwg_count_object` and `handle_nwg_count_method` to take a `Rc<RefCell<NotificationConfig>>` and dispatch each setter. Add 6 new pub setter helpers (one per knob) for the CLI client to call.
- **Modify:** `src/config.rs` — add a `pub update: bool` flag with documentation. No change to existing flag definitions; the `value_source` filtering happens at use-site in main.rs.
- **Modify:** `CHANGELOG.md` — entry under unreleased Added.
- **Modify:** `README.md` — new "Live config updates" section under "Querying notification count".

No new files. The CLI client logic lives next to the existing `query_count_via_dbus` in `src/dbus.rs` since it's the same pattern (thin D-Bus client wrapped by the binary in mode-switch mode).

---

## Pre-flight

- [ ] **Sync main and create branch**

```bash
cd /data/source/nwg-notifications
git checkout main && git pull --ff-only
git status
git checkout -b feat/live-config-updates
```

Expected: clean tree on `main` synced to origin, then a fresh branch `feat/live-config-updates`.

- [ ] **Commit the plan file as the first commit**

```bash
git add docs/superpowers/plans/2026-04-29-live-config-updates.md
git commit -m "docs: implementation plan for live config updates (#20)"
```

- [ ] **Baseline full cargo gambit**

```bash
make lint
```

Expected: every step exits 0. Pre-existing `cargo deny` "unmatched skip" warnings are non-blocking.

---

## Task 1: Refactor `Rc<NotificationConfig>` → `Rc<RefCell<NotificationConfig>>`

The foundational refactor. Internal-only — no observable behavior change yet, but makes config mutable at runtime. Land this first so subsequent tasks have something to mutate.

**Files:**
- Modify: `src/main.rs` (config construction + the two callsites that pass config to PopupManager and panel)
- Modify: `src/ui/popup.rs` (struct field type + `new()` signature + 5 reads in `show()` and `restack()`)

- [ ] **Step 1: Update `PopupManager` struct and constructor**

In `src/ui/popup.rs`, change the field type:

```rust
pub struct PopupManager {
    popups: Vec<ActivePopup>,
    config: Rc<RefCell<NotificationConfig>>,
    app: gtk4::Application,
    on_state_change: Rc<dyn Fn()>,
    compositor: Rc<dyn Compositor>,
}
```

And the `new()` signature:

```rust
    pub fn new(
        app: &gtk4::Application,
        config: &Rc<RefCell<NotificationConfig>>,
        on_state_change: Rc<dyn Fn()>,
        compositor: Rc<dyn Compositor>,
    ) -> Self {
        Self {
            popups: Vec::new(),
            config: Rc::clone(config),
            app: app.clone(),
            on_state_change,
            compositor,
        }
    }
```

- [ ] **Step 2: Update PopupManager's config reads**

There are five `self.config.<field>` reads in `src/ui/popup.rs`. Each becomes `self.config.borrow().<field>`. Specifically:

In `show()`:
- `while self.popups.len() >= self.config.max_popups {` → `while self.popups.len() >= self.config.borrow().max_popups {`
- `window::setup_popup_window(&win, self.config.popup_position, top_offset);` → `window::setup_popup_window(&win, self.config.borrow().popup_position, top_offset);`
- `win.set_width_request(self.config.popup_width);` → `win.set_width_request(self.config.borrow().popup_width);`
- `win.set_default_size(self.config.popup_width, -1);` → `win.set_default_size(self.config.borrow().popup_width, -1);`

In `restack()`:
- `let is_top = matches!(self.config.popup_position, ...)` → `let is_top = matches!(self.config.borrow().popup_position, ...)`

In `resolve_timeout()`:
- `self.config.popup_timeout` → `self.config.borrow().popup_timeout`

`grep -n 'self\.config\.' src/ui/popup.rs` after the edits should return nothing.

- [ ] **Step 3: Update main.rs to construct `Rc<RefCell<NotificationConfig>>`**

In `src/main.rs::main()`, after `let config = NotificationConfig::parse();`:

```rust
    let config = Rc::new(RefCell::new(config));
```

(replacing whatever the current `let config = Rc::new(config);` line says).

Then `activate_notifications` and the `connect_activate` closure already work with `Rc<NotificationConfig>` — change every signature/parameter that currently has `&Rc<NotificationConfig>` to `&Rc<RefCell<NotificationConfig>>`. Specifically:

In the `connect_activate` closure body, replace:
```rust
        activate_notifications(app, &config, &compositor, &sig_rx);
```
(no change to that line — it already passes `&config` by reference)

In `activate_notifications` signature:
```rust
fn activate_notifications(
    app: &gtk4::Application,
    config: &Rc<RefCell<NotificationConfig>>,
    compositor: &Rc<dyn nwg_common::compositor::Compositor>,
    sig_rx: &Rc<std::sync::mpsc::Receiver<listeners::NotificationCommand>>,
) {
```

And the `PopupManager::new` call inside `activate_notifications` already passes `config` correctly (the trait of `&Rc<RefCell<NotificationConfig>>`). One read of config in main.rs still happens directly (the `config.dnd` and `config.persist` reads when seeding state and the persistence callback). Those become `config.borrow().dnd` etc.

`grep -n 'config\.' src/main.rs` should show every read going through `.borrow()` (or `.borrow_mut()` where applicable, though main.rs only reads at startup so all should be `.borrow()`).

- [ ] **Step 4: Build and run tests**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean. Behavior unchanged — this is a pure refactor. The 57 existing tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/ui/popup.rs
git commit -m "$(cat <<'EOF'
Wrap NotificationConfig in RefCell for runtime mutability (#20)

Foundation for live config updates: changes Rc<NotificationConfig>
to Rc<RefCell<NotificationConfig>> across the daemon. PopupManager
reads via self.config.borrow().<field> instead of self.config.<field>.
main.rs's startup reads (dnd, persist) become config.borrow().<field>.

No observable behavior change — pure refactor. NotificationPanel
still takes panel_width: i32 by value; that change is in the next
commit (revisits the #12 YAGNI now that config needs to be readable
at toggle time).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Revisit #12 YAGNI — `NotificationPanel` takes config

`NotificationPanel::new(panel_width: i32)` was the right call when only the constructor read panel_width once. Live updates require reading `panel_width` per toggle, which means storing config in the panel.

**Files:**
- Modify: `src/ui/panel.rs` (struct field, constructor signature, hide/show paths)
- Modify: `src/main.rs` (the call site that passes `config.panel_width`)

- [ ] **Step 1: Add config + panel_box to `NotificationPanel` struct**

In `src/ui/panel.rs`:

```rust
pub struct NotificationPanel {
    pub win: gtk4::ApplicationWindow,
    backdrops: Vec<gtk4::ApplicationWindow>,
    revealer: gtk4::Revealer,
    list_box: gtk4::Box,
    panel_box: gtk4::Box,
    state: Rc<RefCell<NotificationState>>,
    config: Rc<RefCell<crate::config::NotificationConfig>>,
    on_notification_click: Rc<dyn Fn(u32)>,
    on_state_change: Rc<dyn Fn()>,
}
```

(Adds `panel_box` and `config` fields. `panel_box` is needed because we set `set_width_request` on it too — both widgets need to update on a width change.)

- [ ] **Step 2: Update `new()` signature**

Replace `panel_width: i32` parameter with `config: &Rc<RefCell<NotificationConfig>>`. Read `panel_width` once at construction time as the initial width:

```rust
    pub fn new(
        app: &gtk4::Application,
        state: &Rc<RefCell<NotificationState>>,
        config: &Rc<RefCell<crate::config::NotificationConfig>>,
        on_notification_click: Rc<dyn Fn(u32)>,
        on_state_change: Rc<dyn Fn()>,
    ) -> Self {
        // ... unchanged backdrop / win / revealer / list_box / panel_box construction,
        // except the two set_width_request calls now read from config:
        //
        let initial_width = config.borrow().panel_width;
        win.set_width_request(initial_width);
        // ...
        panel_box.set_width_request(initial_width);
        // ...
        let panel = Self {
            win,
            backdrops,
            revealer,
            list_box,
            panel_box: panel_box.clone(),  // store for later width updates
            state: Rc::clone(state),
            config: Rc::clone(config),
            on_notification_click,
            on_state_change,
        };
        // ... rest unchanged
    }
```

- [ ] **Step 3: Apply current panel_width on every toggle-open**

The `toggle()` method has a "reveal" branch that runs when the panel is being shown. Add a width-refresh there so each open picks up any config changes since the last open:

In `toggle()`'s `else` branch (the show path), inside the `idle_add_local_once`:

```rust
            gtk4::glib::idle_add_local_once(move || {
                rebuild_list(&list, &state, on_click, on_change);
                let width = config.borrow().panel_width;
                win.set_width_request(width);
                panel_box.set_width_request(width);
                for backdrop in &backdrops {
                    backdrop.set_visible(true);
                }
                win.set_visible(true);
                revealer.set_reveal_child(true);
            });
```

Capture `panel_box` and `config` into the closure alongside the existing captures. The toggle method needs:
```rust
            let list = self.list_box.clone();
            let state = Rc::clone(&self.state);
            let config = Rc::clone(&self.config);
            let panel_box = self.panel_box.clone();
            let on_click = Rc::clone(&self.on_notification_click);
            let on_change = Rc::clone(&self.on_state_change);
            let win = self.win.clone();
            let backdrops = self.backdrops.clone();
            let revealer = self.revealer.clone();
```

- [ ] **Step 4: Update the `NotificationPanel::new` call in `src/main.rs`**

Current:
```rust
    let panel = Rc::new(RefCell::new(NotificationPanel::new(
        app,
        &state,
        on_panel_click,
        Rc::clone(&on_state_change),
        config.panel_width,
    )));
```

Change to (note the new positional config argument before the callbacks):
```rust
    let panel = Rc::new(RefCell::new(NotificationPanel::new(
        app,
        &state,
        config,
        on_panel_click,
        Rc::clone(&on_state_change),
    )));
```

(`config` here is `&Rc<RefCell<NotificationConfig>>` per Task 1.)

- [ ] **Step 5: Build and run tests**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean. Default behavior unchanged.

- [ ] **Step 6: Commit**

```bash
git add src/ui/panel.rs src/main.rs
git commit -m "$(cat <<'EOF'
NotificationPanel reads panel_width from config per toggle (#20)

Revisits the #12 YAGNI decision: NotificationPanel was given panel_width
as an i32 at construction time when only one read site existed. Live
updates require reading the current value per toggle-open. Replace the
i32 parameter with Rc<RefCell<NotificationConfig>>, store panel_box as
a struct field so both win and panel_box widgets pick up width changes
together.

Width changes apply on the next panel open after the update — no
forced re-show of the panel if it's currently visible. That's the
right ergonomic for shell-config: user changes the slider, next time
they open the panel it's the new width.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: D-Bus introspection XML and method dispatch (6 setters)

**Files:**
- Modify: `src/dbus.rs`

- [ ] **Step 1: Extend `NWG_COUNT_INTROSPECT_XML` with 6 setter methods**

Replace:

```rust
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
```

with:

```rust
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
```

- [ ] **Step 2: Thread `Rc<RefCell<NotificationConfig>>` through the dbus.rs handlers**

`register_nwg_count_object` currently takes `connection` and `state`. Add a third parameter for config:

```rust
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
        .method_call(move |_conn, _sender, _path, _iface, method, params, invocation| {
            handle_nwg_count_method(method, &params, invocation, &state, &config, &on_state_change);
        })
        .build()
        .expect("Failed to register nwg-count D-Bus object");
}
```

The `on_state_change` callback is threaded through so setters can fire it after a successful update — that flushes any waybar/persistence side effects that depend on the new config (none today, but cheap insurance and consistent with how state mutations elsewhere notify).

Note the `params` capture in the closure — Get methods don't need params, but setters consume their input arg from params, so the dispatcher now passes `&params` rather than ignoring it.

- [ ] **Step 3: Update `register_server` to pass config and on_state_change**

In `register_server`, the `bus_own_name` call for the nwg name builds a closure that calls `register_nwg_count_object`. That closure needs config and on_state_change too:

```rust
pub fn register_server(
    state: &Rc<RefCell<NotificationState>>,
    config: &Rc<RefCell<NotificationConfig>>,
    on_state_change: Rc<dyn Fn()>,
    on_notify: OnNotify,
    on_close: OnClose,
) {
    // ... existing org.freedesktop.Notifications bus_own_name unchanged ...

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
```

The `pub fn register_server` signature gains two new parameters; main.rs's call site needs updating to match (Task 3 Step 5 below).

- [ ] **Step 4: Implement `handle_nwg_count_method` dispatch with 6 setters**

Replace the existing `handle_nwg_count_method` with:

```rust
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
        "SetPopupPosition" => handle_set_popup_position(params, invocation, config, on_state_change),
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
```

- [ ] **Step 5: Implement the 6 setter handlers**

Add to `src/dbus.rs` after `handle_nwg_count_method`:

```rust
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
    let parsed = crate::config::PopupPosition::from_str(&raw, true);
    match parsed {
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
            return_invalid_args(
                invocation,
                &format!("popup-width {raw} exceeds i32::MAX"),
            );
            return;
        }
    };
    if !(crate::ui::constants::POPUP_WIDTH_MIN..=crate::ui::constants::POPUP_WIDTH_MAX).contains(&raw_i32) {
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
            return_invalid_args(
                invocation,
                &format!("panel-width {raw} exceeds i32::MAX"),
            );
            return;
        }
    };
    if !(crate::ui::constants::PANEL_WIDTH_MIN..=crate::ui::constants::PANEL_WIDTH_MAX).contains(&raw_i32) {
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
```

- [ ] **Step 6: Update main.rs to pass config + on_state_change to register_server**

In `src/main.rs::activate_notifications`, the existing call:

```rust
    dbus::register_server(&state, on_notify, on_close);
```

becomes:

```rust
    dbus::register_server(&state, config, Rc::clone(&on_state_change), on_notify, on_close);
```

(The `on_state_change` Rc has been built earlier in the function; just clone it for the dbus side. The `config` is the `&Rc<RefCell<NotificationConfig>>` from the function's parameter.)

Update `dbus::register_server`'s signature to accept the new args (Step 3 already covered this).

- [ ] **Step 7: Build and run tests**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: 57 existing tests still pass; clippy clean. No new tests yet — the setter handlers require a live D-Bus connection to exercise (tracked under #16). Manual smoke test (Task 6) covers it.

- [ ] **Step 8: Live D-Bus smoke check (optional, costs disrupting your live daemon)**

This step is **optional** and disrupts your live notification daemon — skip if you'd rather catch issues at the smoke-test gate (Task 6). If you want a quick sanity check now:

```bash
cargo run -- --debug &
DAEMON_PID=$!
sleep 1

gdbus call --session --dest org.nwg.Notifications \
  --object-path /org/nwg/Notifications \
  --method org.nwg.Notifications.SetPopupPosition '"top-center"'

# Expected: returns "()" (success). Now confirm the change took effect:
notify-send "after SetPopupPosition" "should be top-center"

kill "$DAEMON_PID"
```

If the `gdbus call` errors with anything other than `()`, the dispatch is wrong — re-check the introspection XML and the method-name strings.

- [ ] **Step 9: Commit**

```bash
git add src/dbus.rs src/main.rs
git commit -m "$(cat <<'EOF'
Add 6 D-Bus setters on org.nwg.Notifications (#20)

Per-knob setters: SetPopupPosition, SetPopupWidth, SetPanelWidth,
SetPopupTimeout, SetMaxPopups, SetMaxHistory. Each validates against
the same ranges the clap parser uses (or basic sanity for unranged
flags like max-popups >= 1). Returns org.freedesktop.DBus.Error.InvalidArgs
on bad input — no return value on success per D-Bus convention
(success = no error).

The dispatcher now threads config and on_state_change into the nwg
handler closure, so setters can mutate config and fire the existing
state-change pipeline (waybar refresh, history persist).

CLI client wrapping these comes in the next commit.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `--update` CLI mode using `value_source` filtering

**Files:**
- Modify: `src/config.rs` — add `--update` boolean flag and 4 unit tests for the value_source filtering helper.
- Modify: `src/dbus.rs` — add 6 client wrappers (one per setter) and a small `push_config_update` helper used by the CLI.
- Modify: `src/main.rs` — early-branch handling: if `config.update`, run the push path and exit.

- [ ] **Step 1: Add `--update` flag with TDD**

In `src/config.rs`'s `#[cfg(test)] mod tests`, add:

```rust
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
```

Run `cargo test update_flag_ 2>&1 | tail -10`. Expected: compile error — `update` field doesn't exist.

Add the field to `NotificationConfig`:

```rust
    /// Push the values of any *also-passed* flags to the running daemon
    /// over D-Bus, then exit. Used by nwg-shell-config and shell scripts
    /// to update live config without restarting the daemon. Only flags
    /// that are inherently runtime-mutable (popup-position, popup-width,
    /// panel-width, popup-timeout, max-popups, max-history) take effect;
    /// startup-only flags (--persist, --wm, --debug) are silently ignored
    /// in this mode.
    #[arg(long)]
    pub update: bool,
```

Run `cargo test update_flag_`. Expected: both tests pass.

- [ ] **Step 2: Add the value_source filter helper + tests**

Still in `src/config.rs`, but at the module level (not in tests yet — this is real code that the tests below cover):

```rust
/// The set of clap arg IDs that are inherently live-updatable. Flags
/// outside this set (e.g. `--persist`, `--wm`) are skipped in `--update`
/// mode regardless of whether the user passed them, because pushing them
/// to a running daemon is meaningless or unsafe.
pub const LIVE_UPDATABLE_ARGS: &[&str] = &[
    "popup_position",
    "popup_width",
    "panel_width",
    "popup_timeout",
    "max_popups",
    "max_history",
];

/// Returns the subset of `LIVE_UPDATABLE_ARGS` whose value source on the
/// given matches is `CommandLine` (i.e., the user explicitly passed them
/// rather than relying on a default). Used by `--update` mode to push
/// only what the user asked to change, so e.g. `--update --popup-position
/// top-center` doesn't reset every other knob to its default.
pub fn user_set_live_args(matches: &clap::ArgMatches) -> Vec<&'static str> {
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
```

Verify the arg IDs match clap's derive output. `clap` 4 with `#[arg(long)]` on a field named `popup_width` gives an arg ID equal to the field name (with underscores), and a long option `--popup-width`. The helper uses field names (with underscores).

Add tests:

```rust
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
                "--popup-position", "top-center",
                "--popup-width", "600",
            ])
            .expect("parse with two flags");
        let set = user_set_live_args(&matches);
        assert!(set.contains(&"popup_position"));
        assert!(set.contains(&"popup_width"));
        assert_eq!(set.len(), 2, "got {:?}", set);
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
        // Sanity check: the canonical list is the six knobs documented in the issue.
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
```

Run `cargo test user_set_live_args live_updatable_args 2>&1 | tail -15`. Expected: all 4 pass.

- [ ] **Step 3: Add D-Bus client wrappers in `src/dbus.rs`**

Add at the bottom of the existing client section (after `query_count_via_dbus`):

```rust
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

pub fn push_popup_position(value: &str) -> Result<(), glib::Error> {
    call_setter_sync("SetPopupPosition", glib::Variant::from((value,)))
}

pub fn push_popup_width(value: u32) -> Result<(), glib::Error> {
    call_setter_sync("SetPopupWidth", glib::Variant::from((value,)))
}

pub fn push_panel_width(value: u32) -> Result<(), glib::Error> {
    call_setter_sync("SetPanelWidth", glib::Variant::from((value,)))
}

pub fn push_popup_timeout(value: u32) -> Result<(), glib::Error> {
    call_setter_sync("SetPopupTimeout", glib::Variant::from((value,)))
}

pub fn push_max_popups(value: u32) -> Result<(), glib::Error> {
    call_setter_sync("SetMaxPopups", glib::Variant::from((value,)))
}

pub fn push_max_history(value: u32) -> Result<(), glib::Error> {
    call_setter_sync("SetMaxHistory", glib::Variant::from((value,)))
}
```

Reuses the same `NO_AUTO_START` and `QUERY_COUNT_TIMEOUT_MS` constants as `query_count_via_dbus` — single-bus, single-pattern, single-timeout.

- [ ] **Step 4: Add the `--update` early branch in `src/main.rs::main`**

The existing `main()` already has an early branch for `--count`. Add a parallel early branch for `--update` right after the `--count` block:

```rust
fn main() {
    nwg_common::process::handle_dump_args();

    // Use the lower-level entry point so we have access to ArgMatches for
    // value_source filtering in --update mode.
    let matches = NotificationConfig::command().get_matches();
    let config = NotificationConfig::from_arg_matches(&matches)
        .expect("clap should produce a valid NotificationConfig from successful matches");

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

    if config.update {
        let to_push = crate::config::user_set_live_args(&matches);
        if to_push.is_empty() {
            eprintln!("--update requires at least one of: --popup-position, --popup-width, --panel-width, --popup-timeout, --max-popups, --max-history");
            std::process::exit(1);
        }
        let mut had_error = false;
        for name in &to_push {
            let push_result: Result<(), glib::Error> = match *name {
                "popup_position" => {
                    let pos = config.popup_position;
                    use clap::ValueEnum;
                    let raw = pos.to_possible_value().expect("derived ValueEnum yields str").get_name().to_string();
                    dbus::push_popup_position(&raw)
                }
                "popup_width" => dbus::push_popup_width(config.popup_width as u32),
                "panel_width" => dbus::push_panel_width(config.panel_width as u32),
                "popup_timeout" => dbus::push_popup_timeout(config.popup_timeout as u32),
                "max_popups" => dbus::push_max_popups(config.max_popups as u32),
                "max_history" => dbus::push_max_history(config.max_history as u32),
                _ => unreachable!("user_set_live_args returns only known names"),
            };
            if let Err(e) = push_result {
                eprintln!("Failed to update {}: {}", name, e);
                had_error = true;
            } else {
                println!("Updated {}", name);
            }
        }
        std::process::exit(if had_error { 1 } else { 0 });
    }

    // ... rest of main unchanged. Note that the surrounding code now has
    // `config` (NotificationConfig) instead of `let config = ...parse()`.
    // The remaining lines need to wrap config in Rc<RefCell<>> as before:

    if config.debug {
        // ... existing logger init ...
    } else {
        // ... existing logger init ...
    }

    let _lock = match singleton::acquire_lock("mac-notifications") {
        // ... existing lock handling ...
    };

    // ... existing compositor + signal-listener setup ...

    let app = gtk4::Application::builder()
        .application_id("com.mac-notifications.hyprland")
        .build();

    let config = Rc::new(RefCell::new(config));
    // ... rest unchanged ...
}
```

The key changes: use `command().get_matches()` + `from_arg_matches` instead of `parse()` (so we have `matches` for value_source); add the `--update` branch right after the `--count` branch; then continue with the rest of `main()` essentially unchanged.

The `glib::Error` import needs to be added at the top of main.rs:

```rust
use gtk4::glib;
```

(It's already imported elsewhere; if not, add it.)

- [ ] **Step 5: Build and run all tests**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: clean. Test count: 57 + 4 (value_source helpers) + 2 (update_flag_defaults_false / set) = 63 tests pass.

- [ ] **Step 6: Live --update sanity check (uses installed binary, doesn't disrupt anything)**

Just confirms the CLI parses and reaches the D-Bus call (which will fail with no-such-name if the running daemon doesn't have the new interface yet — that's the expected dev-time output):

```bash
./target/debug/nwg-notifications --update --popup-position top-center
echo "exit: $?"
```

Expected (against an old daemon that doesn't know the new methods): error about UnknownMethod, exit 1. Against a daemon built from this branch: `Updated popup_position` to stdout, exit 0.

- [ ] **Step 7: Commit**

```bash
git add src/config.rs src/dbus.rs src/main.rs
git commit -m "$(cat <<'EOF'
Add --update CLI for live config updates over D-Bus (#20)

Mode-switch flag mirroring the --count pattern: short-circuits before
daemon initialization, walks the user-set live-updatable flags via
clap::ArgMatches::value_source(), and pushes each one through the
matching D-Bus setter on org.nwg.Notifications.

LIVE_UPDATABLE_ARGS is the canonical list of the six knobs that take
runtime updates (popup-position, popup-width, panel-width,
popup-timeout, max-popups, max-history). user_set_live_args() filters
those down to the ones whose value source is CommandLine — so e.g.
\`--update --popup-position top-center\` doesn't accidentally reset
the other five to their clap defaults.

Six pub client helpers in dbus.rs (push_popup_position,
push_popup_width, etc.) wrap the gio::DBusConnection::call_sync calls
with the same NO_AUTO_START + QUERY_COUNT_TIMEOUT_MS semantics as
query_count_via_dbus.

4 new unit tests cover the value_source filtering predicate; D-Bus
dispatch itself remains unit-untestable per #16.

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

Find the active `## [X.Y.Z] — Unreleased` section. If 0.3.1 is already shipped (it is, as of this plan), there's no Unreleased section yet — add one above the `## [0.3.1] — 2026-04-28` heading:

```markdown
## [Unreleased]

### Added

- Live config updates (#20). Six new D-Bus methods on
  `org.nwg.Notifications` let consumers like `nwg-shell-config`
  push runtime config changes without restarting the daemon:
  `SetPopupPosition`, `SetPopupWidth`, `SetPanelWidth`,
  `SetPopupTimeout`, `SetMaxPopups`, `SetMaxHistory`. Each setter
  validates against the same ranges as the matching CLI flag and
  returns `org.freedesktop.DBus.Error.InvalidArgs` on bad input.
- `nwg-notifications --update <flags>` CLI subcommand wraps the
  D-Bus setters as a thin client (mirrors the existing `--count`
  pattern). Uses `clap::ArgMatches::value_source` to push only
  flags the user explicitly passed, so `--update --popup-position
  top-center` doesn't reset other knobs to their defaults. (#20)
```

(Place this block above the existing `## [0.3.1] — 2026-04-28` heading.)

- [ ] **Step 2: README "Live config updates" section**

Find the "Querying notification count" H2 section. Add a new H2 immediately after it called "Live config updates":

````markdown
## Live config updates

Six knobs take runtime updates without restarting the daemon. Two surfaces:

### CLI

```bash
# Push individual settings:
nwg-notifications --update --popup-position top-center
nwg-notifications --update --popup-width 600

# Push multiple in one call:
nwg-notifications --update --popup-position top-center --popup-width 600
```

`--update` short-circuits before daemon init and uses `NO_AUTO_START`, so it never spawns a daemon. Exits 1 with a useful error when no daemon is running. Only flags you explicitly pass are pushed — defaults are never sent.

### D-Bus

For tooling that prefers the D-Bus surface directly (e.g. `nwg-shell-config` from Python):

```bash
gdbus call --session \
  --dest org.nwg.Notifications \
  --object-path /org/nwg/Notifications \
  --method org.nwg.Notifications.SetPopupPosition '"top-center"'

gdbus call --session \
  --dest org.nwg.Notifications \
  --object-path /org/nwg/Notifications \
  --method org.nwg.Notifications.SetPopupWidth 600
```

Each setter validates against the same ranges as the matching CLI flag and returns `org.freedesktop.DBus.Error.InvalidArgs` on bad input. The full set:

| Setter             | Type    | Validation                                |
|--------------------|---------|-------------------------------------------|
| `SetPopupPosition` | `s`     | One of: top-right, top-center, top-left, bottom-right, bottom-center, bottom-left |
| `SetPopupWidth`    | `u`     | `100..=2000`                              |
| `SetPanelWidth`    | `u`     | `200..=2000`                              |
| `SetPopupTimeout`  | `u`     | Any uint32 (ms; 0 = never auto-dismiss)   |
| `SetMaxPopups`     | `u`     | `>= 1`                                    |
| `SetMaxHistory`    | `u`     | `>= 1`                                    |

### What can't be live-updated

`--persist`, `--wm`, and `--debug` are inherently startup-only — restart the daemon to change those.
````

- [ ] **Step 3: Build, test, clippy**

```bash
cargo build && cargo test && cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md README.md
git commit -m "$(cat <<'EOF'
docs: live config updates (#20)

CHANGELOG entry under Unreleased Added; new README section "Live
config updates" with examples for the CLI subcommand and the D-Bus
methods, plus a table of validation ranges per setter.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: User smoke-test gate (HARD STOP)

The unit tests cover the value_source filtering and clap parse; manual verification confirms the live path actually mutates daemon state and that subsequent popup/panel renders pick up the new values.

- [ ] **Step 1: Install to the user's `~/.cargo/bin`**

```bash
make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
```

Don't run `make install-dbus` — D-Bus service file unchanged.

- [ ] **Step 2: Restart the user's session daemon so it owns the new D-Bus interface**

```bash
kill $(pidof nwg-notifications) 2>/dev/null || true
sleep 0.5
uwsm-app -- nwg-notifications --persist >/dev/null 2>&1 &
disown
sleep 1
pidof nwg-notifications && echo "(daemon up)"
```

- [ ] **Step 3: Hand off to the user — STOP HERE**

Tell the user (verbatim or close):
> Installed and restarted the daemon. Smoke-test paths for #20:
>
> **CLI mode (`--update`):**
> 1. `nwg-notifications --update --popup-position top-center` — expect `Updated popup_position`, exit 0. Then `notify-send "test" "msg"` should appear top-center.
> 2. `nwg-notifications --update --popup-width 600` — expect `Updated popup_width`, exit 0. Next `notify-send` is wider.
> 3. `nwg-notifications --update --panel-width 700` — opens-the-panel-via-waybar should now slide in 700px wide.
> 4. `nwg-notifications --update --popup-position bottom-left --popup-width 380` — both pushed; `notify-send` lands bottom-left at 380px.
> 5. `nwg-notifications --update --popup-width 50` — should fail at parse time (clap range check, before D-Bus dispatch).
> 6. `nwg-notifications --update` (no other flags) — should error "requires at least one of …", exit 1.
>
> **D-Bus mode (`gdbus call`):**
> 7. `gdbus call --session --dest org.nwg.Notifications --object-path /org/nwg/Notifications --method org.nwg.Notifications.SetPopupPosition '"top-right"'` — returns `()`. Notification sent next lands top-right.
> 8. `gdbus call ... .SetPopupWidth 5000` — should return `org.freedesktop.DBus.Error.InvalidArgs` ("popup-width 5000 is not in 100..=2000").
> 9. `gdbus call ... .SetPopupPosition '"nonsense"'` — should return InvalidArgs.
>
> **Doesn't break the existing surface:**
> 10. `nwg-notifications --count` — still works.
> 11. `gdbus call ... .GetCount` — still works.
> 12. Send `notify-send` and click → mark-read still emits CountChanged.
>
> Reply when satisfied or with anything that needs fixing.

**Do not proceed to Task 7 until the user explicitly approves.** If they report issues, return to the broken task.

---

## Task 7: Full cargo gambit (CI parity)

- [ ] **Step 1: `make lint`**

```bash
make lint
```

If `cargo fmt --check` reformats anything, run `cargo fmt --all`, commit as `style: cargo fmt`, re-run `make lint`. Stop on new deny/audit findings.

- [ ] **Step 2: Confirm clean working tree**

```bash
git status
```

Expected: clean.

---

## Task 8: Open the PR

Gated on Task 6 (user smoke-test approval) AND Task 7 (clean `make lint`).

- [ ] **Step 1: Push the branch**

```bash
git push -u origin feat/live-config-updates
```

- [ ] **Step 2: Create the PR**

```bash
gh pr create --title "Live config updates: D-Bus setters + --update CLI (#20)" --body "$(cat <<'EOF'
## Summary

Adds a runtime push mechanism for the six live-updatable config knobs so consumers like \`nwg-shell-config\` can change settings without restarting the daemon.

**Two surfaces (per [#20](https://github.com/jasonherald/nwg-notifications/issues/20)):**

1. **D-Bus methods on \`org.nwg.Notifications\`** — primary; what shell-config calls from Python:
   - \`SetPopupPosition(s)\`, \`SetPopupWidth(u)\`, \`SetPanelWidth(u)\`, \`SetPopupTimeout(u)\`, \`SetMaxPopups(u)\`, \`SetMaxHistory(u)\`
   - Each validates against the same ranges as the matching CLI flag; bad input returns \`org.freedesktop.DBus.Error.InvalidArgs\`.
2. **\`nwg-notifications --update\` CLI** — thin client wrapping the D-Bus methods. Same mode-switch pattern as \`--count\`. Uses \`clap::ArgMatches::value_source()\` to push only the flags the user explicitly set, never resetting other knobs to their defaults.

**Inherently startup-only flags** (\`--persist\`, \`--wm\`, \`--debug\`) are explicitly excluded from the live-updatable set.

Surfaced from in-progress conversation with OG on [#2](https://github.com/jasonherald/nwg-notifications/issues/2#issuecomment-4347480676). Closes #20.

## Test plan

- [x] \`make lint\` — fmt + clippy + test + deny + audit, all green locally.
- [x] Unit tests (6 new for #20, 63 total):
  - \`update\` clap flag parses (default false, set with \`--update\`).
  - \`user_set_live_args\` returns empty when only defaults.
  - \`user_set_live_args\` returns only flags whose value source is CommandLine.
  - \`user_set_live_args\` ignores startup-only flags (\`--persist\`, \`--debug\`).
  - \`LIVE_UPDATABLE_ARGS\` contains exactly the six canonical knobs.
- [x] Manual smoke test against the live compositor:
  - [x] CLI: \`--update\` for each knob individually, multiple knobs in one call, error on no-flags-passed, range rejection at parse time.
  - [x] D-Bus: \`gdbus call\` for each setter; bad input returns \`InvalidArgs\` with a useful message.
  - [x] Existing surface (\`GetCount\`, \`CountChanged\`, \`--count\`) still works.

## Design notes

- **Why per-knob methods over single \`SetConfig(a{sv})\`**: introspectable, type-safe, maps cleanly to one Python function per setting in shell-config. CodeRabbit suggested this on the issue's ancestor PR (#14, count IPC).
- **Why \`value_source\` over \`Option<T>\` per field**: smaller blast radius. No struct duplication, no separate parser for update mode. The only pattern change is using \`command().get_matches()\` + \`from_arg_matches()\` in main() instead of \`parse()\` so we have access to ArgMatches.
- **Refactor: \`Rc<NotificationConfig>\` → \`Rc<RefCell<NotificationConfig>>\`**: foundational. Lands as Task 1's commit so subsequent commits' diffs stay focused on the new setter / client logic.
- **Revisits the #12 YAGNI**: \`NotificationPanel::new(panel_width: i32)\` becomes \`NotificationPanel::new(..., config: &Rc<RefCell<NotificationConfig>>)\`. Width changes apply on the next panel open after the update — natural ergonomic for a settings-UI use case.

The implementation plan (committed as \`docs/superpowers/plans/2026-04-29-live-config-updates.md\`) is on the branch for reviewer context.
EOF
)"
```

Expected: returns the PR URL.

- [ ] **Step 3: Hand off to CodeRabbit**

CodeRabbit reviews within minutes. **Default to fixing every finding in-PR.** Defer only when the fix needs new infrastructure or has wide blast radius — and when you do defer, open a tracking issue *first*.

Reply protocol: inline replies for in-diff comments, single PR-level comment for outside-diff items, tag `@coderabbitai` every time so it learns from the responses.

---

## After merge — pending follow-up

After this PR merges, **Jason** (not me) replies to OG on [#2](https://github.com/jasonherald/nwg-notifications/issues/2#issuecomment-4347480676) with a summary of the new surface (six D-Bus setters + `--update` CLI). I'll draft response text when we're at that point — never post under Jason's account on threads with users.

---

## Acceptance checklist (cross-reference to issue #20)

- [ ] Six new D-Bus methods land on `org.nwg.Notifications`, each validating against the same ranges as the clap parser, returning `org.freedesktop.DBus.Error.InvalidArgs` on bad input. — Task 3
- [ ] `nwg-notifications --update <flags>` works as a thin client: mode-switch in `main()`, uses `value_source()` to push only what the user set, `NO_AUTO_START` semantics, exits 0/1. — Task 4
- [ ] Live update is verified end-to-end for all six knobs (popup-position, popup-width, panel-width, popup-timeout, max-popups, max-history). — Task 6
- [ ] README "Querying notification count" section grows a sibling section documenting all six setters with copy-paste `gdbus call` and `--update` examples. — Task 5
- [ ] CHANGELOG entry under unreleased Added. — Task 5
- [ ] Unit tests cover the value_source filtering logic. — Task 4 (4 unit tests)
