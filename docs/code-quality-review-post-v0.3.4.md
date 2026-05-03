# Code-Quality Polish Pass — Post-v0.3.4 Cleanup

> **Status:** Inventory of polish opportunities identified by a comprehensive code review of the `src/` tree on 2026-05-03 (commit `a128397`, v0.3.4 shipped). To be filed as GH issues under epic "Code-quality polish pass: post-v0.3.4 cleanup".

## Executive summary

Overall the `src/` tree is in good shape: 18 Rust files totalling ~3.7 KLOC, consistent module layout (`config` / `notification` / `state` / `dbus` / `listeners` / `persistence` / `waybar` / `ui::*`), well-documented, with named constants in `ui/constants.rs` and 65 passing unit tests. The CodeRabbit-shaped conventions baked in over v0.3.0–v0.3.4 (per-knob D-Bus methods, `Rc<dyn Fn()>` callback threading, `value_source()`-based filtering for `--update`, `org.freedesktop.DBus.Error.InvalidArgs` for validation failures) are followed cleanly. Lint passes with zero warnings, formatting is consistent, and `expect()` is used disciplined-ly only at structural-impossibility sites (parsing the embedded XML, registering the bus object, etc.).

The cleanup themes that emerge are mostly polish: a small number of **real correctness bugs** (the most notable is `state.max_history` and `config.max_history` being two copies of the same value where `SetMaxHistory` only updates the config copy, plus a `GetServerInformation` D-Bus response that still reports vendor `nwg-dock-hyprland` and version `0.1.0`), several **legacy-naming carryovers** from the mac-doc-hyprland monorepo era (`mac-notifications-*` filenames, `com.mac-notifications.hyprland` GTK app id), **callback-threading boilerplate** in the six `handle_set_*` D-Bus handlers (~30 LOC repeated six times that could collapse into a generic), and **modest test gaps** in pure helpers that have no GTK dependency (the D-Bus hint extractors, `relative_time`, the live-config setters end-to-end). Nothing here is urgent.

Recommended priority order: ship the correctness fixes first (1.1, 1.2, 2.1), then the user-facing protocol-string fix (1.3), then the legacy-naming cleanup (1.4, 1.5) — these are all under "small effort" and each unblocks a concrete user-reported scenario or pre-empts one. The boilerplate-collapse and test-gap items are pure hygiene and can land in any order.

## Themes

- **Bug-shaped: state/config knob duplication.** `state.max_history` is seeded from `config.max_history` at startup and never resynced; `SetMaxHistory` over D-Bus mutates only the config copy, so the daemon keeps trimming to the original limit. Same shape applies to a degree to the timed-DND expiry timer (latest scheduler doesn't cancel the previous one).
- **Bug-shaped: legacy identifiers from the monorepo split.** `GetServerInformation` returns `vendor="nwg-dock-hyprland", version="0.1.0"`; runtime files are still `mac-notifications-*`; GTK app id is `com.mac-notifications.hyprland`; singleton lock is named `mac-notifications`. Some of these are migration-risky (changing the singleton lock name lets two daemons co-exist for one upgrade); plan accordingly.
- **Boilerplate in `dbus.rs`.** The six `handle_set_*` functions for `SetPopupWidth` / `SetPanelWidth` / `SetPopupTimeout` / `SetMaxPopups` / `SetMaxHistory` / `SetPopupPosition` follow nearly identical "decode arg → validate → write to config → return → fire callback" shapes; `dbus.rs` at 716 lines is the file most affected by this. Same boilerplate exists for the six `push_*` client wrappers in the `--update` path.
- **Public surface is wider than it needs to be.** Twenty-plus `pub` items in this binary crate are only used inside the bin and would more accurately read as `pub(crate)` (the crate doesn't expose a library, so external visibility is a no-op that hides the actual coupling shape from readers).
- **Hardcoded signal arithmetic.** `WAYBAR_REFRESH_SIGNAL: i32 = 45` is hardcoded in `waybar.rs` while the rest of the codebase derives RT-signal numbers via `nwg_common::signals::sigrtmin()` precisely because glibc/musl differ — same mistake as the one CLAUDE.md warns about.
- **Test gaps in pure helpers.** Six pure helper functions have no test coverage despite being trivially testable: `extract_urgency`, `extract_string_hint`, `relative_time`, `parse_actions` cases for empty/whitespace, `WaybarStatus` shape for the dnd/empty/unread branches.
- **Callback-threading consistency.** Three independent code paths flip `state.dnd` (signal listener, panel header button, DND menu) with subtly different side effects (only the menu clears `dnd_expires`; only the panel header button updates an icon; only the menu does both). Worth one consolidating helper.
- **Format-string and log-level micro-inconsistency.** Mix of `{}` positional and `{e}` interpolated styles in `log::*` calls within the same file; mix of `log::warn!` / `log::error!` choices for similar failures (UnknownMethod is `warn` in some places, `error` in others).

## Items

### 1. Correctness fixes

#### Item 1.1: Sync `state.max_history` when `SetMaxHistory` mutates the config copy

**Files:** `src/dbus.rs`, `src/state.rs`, `src/main.rs`

**Scope:** The daemon stores `max_history` twice: once on `NotificationConfig` (read by clap, mutable via `SetMaxHistory`) and once on `NotificationState` (read by `trim_history()` after every `add()`). At startup `main.rs:157` seeds `state.max_history` from `config.max_history`, but `handle_set_max_history` in `dbus.rs:418-438` only writes the config copy. Result: a user who runs `nwg-notifications --update --max-history 50` against a daemon started with the default 200 will keep retaining 200 entries until the daemon restarts. Fix by either (a) eliminating the duplicated field on `NotificationState` and reading from the shared `Rc<RefCell<NotificationConfig>>`, or (b) propagating the new value through the on-state-change path. Option (a) is the right fix and matches the existing precedent for `popup_position` / `popup_width` / `popup_timeout` / `max_popups`, none of which keep a state-side copy.

**Rationale:** Silent behaviour drift between the documented `--update --max-history` semantics and the daemon's actual trimming. CodeRabbit reviewed `SetMaxHistory` in #20 but didn't catch this because the state field was added pre-#20 and the connection wasn't obvious.

**Acceptance criteria:**
- [ ] `state.max_history` field is removed; `trim_history()` reads from `config.max_history`.
- [ ] Existing tests for `trim_history_caps_at_max` keep passing (likely needs the test to construct a config too).
- [ ] New test: setting `config.max_history` lower than `history.len()` and triggering `add()` trims to the new value.
- [ ] Document smoke-tested: start daemon `--max-history 200`, fill to 200, `--update --max-history 5`, send one more notification, verify history is exactly 5.

**Effort:** small

**Suggested label:** bug

#### Item 1.2: Cancel previous timed-DND timer when scheduling a new one

**Files:** `src/ui/dnd_menu.rs`

**Scope:** `build_timed_dnd_button` at `dnd_menu.rs:212-237` schedules a `glib::timeout_add_local_once` for the requested duration. Re-clicking a different duration (e.g. user picks 1h, then 2h before the first fires) leaves both timers armed. The 1h timer fires first, sees `dnd_expires.is_some()` (still true — the 2h replaced it), and disables DND, breaking the user's expectation. Fix by storing the current expiry generation/token on `state.dnd_expires` (e.g. `(SystemTime, u64)` token) and having the timer compare its captured token against the live one before acting; or by holding the `SourceId` and removing the previous one in `state` before scheduling the new.

**Rationale:** Easy to reproduce, tied to a documented user-facing feature (timed DND), and the fix is local. Not a "polish" item per se, but it's small enough to include in the polish pass.

**Acceptance criteria:**
- [ ] Stacking two timed-DND choices in quick succession leaves only the most recent expiry effective.
- [ ] Test: simulate scheduling 1h then 2h, advance clock past 1h, verify DND still on.

**Effort:** small

**Suggested label:** bug

#### Item 1.3: Fix `GetServerInformation` to report the real name/vendor/version

**Files:** `src/dbus.rs`

**Scope:** `handle_server_info` at `dbus.rs:516-520` returns `("nwg-notifications", "nwg-dock-hyprland", "0.1.0", "1.2")`. The vendor is wrong (left over from the pre-split mac-doc-hyprland era — the dock isn't this daemon's vendor) and the version is wrong (we're on 0.3.4). Per the freedesktop notification spec, this is what client apps and notification debuggers (e.g. `dbus-send --print-reply`, `notify-send --print-id`, GNOME's notification troubleshooter) read to identify the running daemon. Replace with:

```rust
let info = ("nwg-notifications", "nwg-notifications", env!("CARGO_PKG_VERSION"), "1.2");
```

(`CARGO_PKG_VERSION` is already evaluated at compile time, no new dep, and stays in sync with `Cargo.toml` automatically.)

**Rationale:** Spec-visible misinformation; trivial to fix; matters for upstream tools that key off `GetServerInformation`. Vendor field per spec is meant to identify the implementer/maintainer — the daemon's own name is the conventional choice for single-vendor projects.

**Acceptance criteria:**
- [ ] `dbus-send --session --dest=org.freedesktop.Notifications --print-reply /org/freedesktop/Notifications org.freedesktop.Notifications.GetServerInformation` returns name `nwg-notifications`, version matching `Cargo.toml`.
- [ ] Test: assert the returned tuple uses `env!("CARGO_PKG_VERSION")`.

**Effort:** trivial

**Suggested label:** bug

#### Item 1.4: Replace hardcoded `WAYBAR_REFRESH_SIGNAL: i32 = 45` with `nwg_common::signals` derivation

**Files:** `src/waybar.rs`

**Scope:** `waybar.rs:5` hardcodes the waybar-refresh signal as `45` (SIGRTMIN+11 on glibc). On musl `SIGRTMIN` is 35 instead of 34, so `SIGRTMIN+11 = 46`, not 45. The rest of the codebase already derives RT-signal numbers via `nwg_common::signals::sigrtmin()` precisely to avoid this — see `listeners.rs:24-26` for the right pattern. Either: (a) add a `sig_waybar_refresh()` to `nwg_common::signals` mirroring the existing `sig_notification_*()` helpers and call it here, or (b) compute `nwg_common::signals::sigrtmin() + 11` inline. (a) is preferred since waybar-refresh is the same convention used by every nwg-* daemon — likely candidates for promotion to nwg-common.

**Rationale:** CLAUDE.md explicitly flags this gotcha ("Values approximate — glibc/musl differ; see `nwg_common::signals::sigrtmin()`") and we're violating it in the one place that matters for the waybar signaling round-trip.

**Acceptance criteria:**
- [ ] No hardcoded `45` (or `34`/`35`) in `waybar.rs`.
- [ ] If promoting to nwg-common: the helper is named consistently with `sig_notification_toggle()` etc.
- [ ] Local smoke test confirms waybar still refreshes.

**Effort:** trivial (in-tree); small (if also adding the helper to nwg-common)

**Suggested label:** bug, nwg-common-candidate

### 2. Legacy naming cleanup

#### Item 2.1: Rename runtime artifacts from `mac-notifications-*` to `nwg-notifications-*`

**Files:** `src/persistence.rs`, `src/waybar.rs`, `src/main.rs`, plus README and the waybar-config snippet

**Scope:** Three legacy identifiers carry over from the mac-doc-hyprland monorepo era:
- `mac-notifications-history.json` (history persistence file in cache dir)
- `mac-notifications-status.json` (waybar status file in `$XDG_RUNTIME_DIR`)
- `mac-notifications` (singleton lock name)
- `com.mac-notifications.hyprland` (GTK application_id)

Renaming the singleton lock is the highest-risk piece: during a daemon upgrade the old (lock-name `mac-notifications`) and new (lock-name `nwg-notifications`) processes won't see each other, allowing both to run concurrently for one user-action cycle. Plan the cutover with a one-release transition where the new daemon checks both lock names. The status-file rename also requires coordinated update to README's waybar config snippet.

The history-file rename is the most user-visible: existing users would lose persisted history on upgrade unless we add a one-time migration (read old path if present, write to new path, unlink old path). Worth doing; the migration is ~10 LOC.

**Rationale:** Internal-consistency and discoverability — every user-visible string in the user-facing docs already says "nwg-notifications", but a user looking for "where does the daemon store its history?" finds `mac-notifications-history.json` and that is confusing. Pre-1.0 is the right window to do this rename.

**Acceptance criteria:**
- [ ] All four `mac-notifications*` strings replaced.
- [ ] One-time history migration: if the new path doesn't exist and the old one does, copy then unlink.
- [ ] Lock-name transition strategy documented in CHANGELOG (or one-release dual-check, or "restart your session after upgrade").
- [ ] README waybar snippet updated for the new status-file path.
- [ ] Acceptance smoke: upgrade install over a v0.3.4 install with a populated history, confirm history shows up after restart.

**Effort:** medium (because of the migration design)

**Suggested label:** refactor, breaking-internal

#### Item 2.2: Drop the `// 󰛙 bell-off`-style comments next to nerd-font codepoints, or wrap them in named constants

**Files:** `src/waybar.rs`

**Scope:** `waybar.rs:28,36,47` use raw `\u{f06d9}`, `\u{f009a}`, `\u{f009c}` codepoints with a sidecar comment showing the rendered glyph and name. Either lift these to named constants (`const ICON_BELL_OFF: &str = "\u{f06d9}";`) per the CLAUDE.md "no magic numbers, named constants" convention, or document the codepoint scheme (Material Design Icons range, font requirement) in a module docstring. Currently the magic-codepoints rule has a quiet exception here.

**Rationale:** Minor consistency hit; named constants make it grep-able and document the icon-font dependency.

**Acceptance criteria:**
- [ ] Codepoints lifted to named `const`s with descriptive names.
- [ ] Module docstring on `waybar.rs` notes the nerd-font dependency.

**Effort:** trivial

**Suggested label:** refactor

### 3. Architecture and cohesion

#### Item 3.1: Collapse the six D-Bus `handle_set_*` handlers into a generic + per-field validators

**Files:** `src/dbus.rs`

**Scope:** `dbus.rs:270-438` defines six near-identical functions (`handle_set_popup_position` / `handle_set_popup_width` / `handle_set_panel_width` / `handle_set_popup_timeout` / `handle_set_max_popups` / `handle_set_max_history`) that all follow:

1. Decode the first arg from `glib::Variant` to the expected type.
2. (Sometimes) validate the range / non-zero constraint.
3. Write to the relevant `config.borrow_mut()` field.
4. `invocation.return_value(None); on_state_change();`

Refactor to a small generic helper `handle_set<T>(...)` that takes the decoder, the validator (or `Ok` for unconstrained types), and the writer closure. Drops ~110 LOC and makes the validation-on-each-knob policy uniform. The `SetPopupPosition` case is special (string → enum) so it might keep its own small wrapper, but the five `u32` variants collapse cleanly.

**Rationale:** The boilerplate has grown 6× since #20 added the live-config knobs. Dropping it makes adding a 7th knob a one-line change instead of a 20-line copy-paste. Same shape applies to the `push_*` client wrappers (`dbus.rs:602-624`) — six near-identical 3-line functions that could be one generic.

**Acceptance criteria:**
- [ ] Six `handle_set_*` functions collapsed into a single generic + small per-knob validator entries (a slice of `(method_name, handler)` pairs would also work).
- [ ] Six `push_*` functions collapsed into a generic.
- [ ] Existing test coverage stays green; add one parametric test per validator if the generic exposes a clean entry point.

**Effort:** medium

**Suggested label:** refactor

#### Item 3.2: Centralise the three `state.dnd` flip code paths into a `NotificationState::set_dnd(bool, expires)` helper

**Files:** `src/state.rs`, `src/listeners.rs`, `src/ui/panel.rs`, `src/ui/dnd_menu.rs`

**Scope:** Three places mutate `state.dnd`:
- `listeners.rs:96-102` (signal handler) — sets `dnd`, logs, fires `on_state_change`.
- `ui/panel.rs:250-261` (panel header button) — sets `dnd`, updates the button icon, logs, fires `on_state_change`. Does **not** clear `dnd_expires`.
- `ui/dnd_menu.rs:128-138` and `212-216` — sets `dnd` + `dnd_expires`, logs, fires `on_state_change`.

The three paths' subtle differences are bugs-of-omission (the panel header button leaves a stale `dnd_expires`, the signal handler doesn't either set or clear it, etc.). Add `NotificationState::set_dnd(enabled: bool, expires: Option<SystemTime>)` that handles the field write + log line; have all three callers route through it. Also: the panel header button's icon update should ideally be driven by a state-changed callback rather than baked into the click handler — separate concern, so probably out of scope for this item, but worth noting.

**Rationale:** Three places that flip the same flag with three slightly different side-effect sets is exactly the shape that grows future bugs.

**Acceptance criteria:**
- [ ] `NotificationState::set_dnd(enabled, expires)` exists and is the only place that writes `dnd` / `dnd_expires`.
- [ ] All three call sites updated.
- [ ] Test: enabling timed DND then toggling via signal correctly clears `dnd_expires`.

**Effort:** small

**Suggested label:** refactor

#### Item 3.3: Tighten the `pub` surface to `pub(crate)` for items only used inside the bin crate

**Files:** all `src/`

**Scope:** This is a binary crate (`[[bin]] name = "nwg-notifications"`), so `pub` and `pub(crate)` are functionally equivalent. But over twenty `pub` items (e.g. `Notification` struct fields, `NotificationState` fields, `clean_markup`, `parse_actions`, `unread_count_to_u32`, the six `push_*` wrappers, `LIVE_UPDATABLE_ARGS`, etc.) signal "external API" to a reader who doesn't yet know it's a bin crate. Switch everything to `pub(crate)` (or stricter where a tighter scope works) so the visibility actually documents the call graph. Conventional practice in single-binary Rust crates.

Special call-out: the field-level `pub` on `Notification` and `NotificationState` is the heaviest signal that we have an "external" data shape, when in fact the only consumers are sister modules. `pub(crate)` is the right call there too.

**Rationale:** Documentation-via-types. Also makes `cargo doc --document-private-items` clearer about what's actually internal.

**Acceptance criteria:**
- [ ] Every `pub` item in the bin crate is either `pub(crate)` or scoped tighter.
- [ ] Build still passes (`cargo build --release` — confirms nothing in `tests/` reaches across the boundary).
- [ ] If anything in `tests/integration/` reads a `pub` item, leave that one alone and note it in the PR.

**Effort:** small

**Suggested label:** refactor

#### Item 3.4: Move CSS and history file paths out of `persistence.rs` / `waybar.rs` and into a `paths` module

**Files:** new `src/paths.rs`, `src/persistence.rs`, `src/waybar.rs`

**Scope:** `persistence.rs:5-9` defines `history_path()` and `waybar.rs:17-22` defines `status_path()` — two near-identical functions that join a base dir against a fixed filename. Both will be touched by Item 2.1. Lift them to a `paths` module so the rename is one-file diff and the conventions (cache dir vs runtime dir, fallback to `/tmp`) are co-located. Small enough to land alongside Item 2.1 if preferred.

**Rationale:** Pure cohesion. Two of three filename constants in the codebase live next to the I/O code that uses them; lifting them out is the standard refactor.

**Acceptance criteria:**
- [ ] `src/paths.rs` exposes `history_path()` and `status_path()`.
- [ ] `persistence.rs` and `waybar.rs` import them.
- [ ] No behavioural change.

**Effort:** trivial

**Suggested label:** refactor

### 4. Rust idiom

#### Item 4.1: Replace `as` casts with `try_from` / `From` where the conversion is fallible or non-obvious

**Files:** `src/main.rs`, `src/ui/popup.rs`, `src/ui/panel.rs`, `src/dbus.rs`

**Scope:** Several `as` casts are doing real conversions where `try_from` would document the safety case:
- `main.rs:78-82`: `config.popup_width as u32` etc. — these converted i32→u32, validated upstream; could be `u32::try_from(...).expect("validated by clap")` or a `to_u32()` helper if it gets repetitive.
- `dbus.rs:413,435`: `raw as usize` for `max_popups` / `max_history` — u32→usize is always lossless on 32-bit-and-up, but `usize::from(raw)` reads cleaner and survives a hypothetical 16-bit target audit.
- `ui/popup.rs:171,187`: `i as i32` and `self.popups.len() as i32` for layout math — len is bounded by `max_popups` which is small, but the implicit truncation is non-obvious.
- `ui/popup.rs:319`: `focused_idx as u32` — `focused_idx` is a `usize` from `position()`; same shape.
- `ui/panel.rs:206`: `PANEL_REVEAL_DURATION_MS as u64` — the constant is `u32`, conversion is lossless. Could use `u64::from()`.

Stronger version of `unread_count_to_u32`'s precedent (the one place in the codebase that does this rigorously). Doesn't have to be 100% — just the cases where the cast is non-trivial.

**Rationale:** Codebase precedent for `try_from` exists (`unread_count_to_u32`); apply consistently.

**Acceptance criteria:**
- [ ] `as` casts in non-trivial sites replaced with `From`/`u64::from`/`try_from`.
- [ ] Pure layout-math casts left alone if the bound is obvious from context — note in PR which were skipped.

**Effort:** small

**Suggested label:** refactor

#### Item 4.2: Extract the D-Bus `Variant` hint extractors and add tests

**Files:** `src/dbus.rs`, possibly new `src/dbus_hints.rs`

**Scope:** `extract_urgency` (dbus.rs:651-664) and `extract_string_hint` (dbus.rs:666-676) walk a `glib::Variant` `a{sv}` dict by index. They're pure, fully testable with synthetic `glib::Variant` payloads, and currently untested. Either lift them to a sibling module or just add tests in the existing `#[cfg(test)] mod tests` block. While there: `extract_urgency` could itself use `extract_string_hint`'s shape (they both walk the same dict structure with different value-type extractors) — a generic `extract_hint::<T>(hints, key) -> Option<T>` would unify them.

**Rationale:** Pure helpers without coverage are the easiest test wins. Generic version also makes adding future hint extractors (e.g. `transient`, `image-data`) trivial.

**Acceptance criteria:**
- [ ] Generic `extract_hint::<T>(hints: &glib::Variant, key: &str) -> Option<T>` exists; both call sites use it.
- [ ] At least four tests covering: missing key, wrong-type value, well-formed Low/Normal/Critical urgency, well-formed `desktop-entry` string.

**Effort:** small

**Suggested label:** test, refactor

#### Item 4.3: Standardise log-message format style and log levels for the same kind of failure

**Files:** all `src/`

**Scope:** Two micro-inconsistencies:
- Format style: most `log::*` calls use `{}` positional (`log::warn!("Unknown method: {}", method)`), one uses `{e}` interpolated (`log::debug!("Failed to signal waybar: {e}")`). Pick one — the `{e}` interpolated form is what `eprintln!`/`format!` calls are using elsewhere in the file (consistent within `main.rs`'s `eprintln!`s), and rustfmt-clippy will eventually push that direction. Lean toward interpolated.
- Log-level: `Unknown D-Bus method` is `warn` (`dbus.rs:190`) but the equivalent path through `--update` against a stale daemon is `error` (`main.rs:87,101`). For unknown-method-from-client (the daemon-side handler) `warn` is right; for unknown-method-from-cli-against-stale-daemon (the client-side push) `error` is right because it's an actionable failure for the user. Currently both make sense individually but reading the code at the same time is jarring. Add a one-sentence rationale comment at each site.

**Rationale:** Cross-PR drift — the format style was set in early PRs, then `{e}` came in later; log-level choices were per-PR. Documenting the rule pre-empts the next CodeRabbit ping.

**Acceptance criteria:**
- [ ] One log format-style rule applied across all `log::*` calls.
- [ ] Comment near `dbus.rs:190` explaining why client-side unknown-method is `warn` here vs `error` in `main.rs`.

**Effort:** trivial

**Suggested label:** refactor

#### Item 4.4: Drop redundant `match` on `bool` in `state.rs:50` and similar

**Files:** `src/state.rs`

**Scope:** `state.rs:50`: `self.next_id = self.next_id.wrapping_add(1).max(1);` is correct but the `.max(1)` is doing two things at once (wrap-around protection + zero-protection). The accompanying test `id_wrapping_at_max` documents both. Worth a one-line comment (or an `assert_ne!(id, 0, "..")` guard in debug-only) explaining that we treat 0 as "no ID assigned" (a freedesktop spec convention — replaces_id=0 means "don't replace"). The current code is right, just opaque.

While there: extract `next_notification_id(&mut self) -> u32` since the wrapping-arithmetic comment is the kind of thing that should live next to the function, not the call site.

**Rationale:** Subtle invariant (`id != 0`) that matters for the freedesktop protocol and currently isn't documented at the function level.

**Acceptance criteria:**
- [ ] `next_notification_id()` helper extracted with a docstring documenting the `id != 0` invariant.
- [ ] Tests still pass; the existing `id_wrapping_at_max` test exercises the helper directly.

**Effort:** trivial

**Suggested label:** refactor, docs

### 5. Documentation

#### Item 5.1: Add module-level docstrings (`//!`) to every `src/*.rs` and `src/ui/*.rs`

**Files:** all `src/*.rs` except `src/ui/constants.rs` (which already has one)

**Scope:** Only `src/ui/constants.rs` has a `//!` module docstring today. The CLAUDE.md "What lives where" section already documents the rough purpose of each file; lift those one-liners into module docstrings. Targets:
- `src/main.rs` — "Coordinator: wires daemon state, popup manager, panel, D-Bus server, and signal listener."
- `src/config.rs` — "clap CLI definition and `--update` value-source filtering."
- `src/notification.rs` — "`Notification`, `Urgency`, and the markup-stripping / action-parsing helpers."
- `src/state.rs` — "`NotificationState`: history, groups, DND, and the `dbus_connection` slot used by callbacks."
- `src/dbus.rs` — "D-Bus server (`org.freedesktop.Notifications` + `org.nwg.Notifications`) running directly on the glib main loop."
- `src/listeners.rs` — "Signal-thread → mpsc → glib timeout bridge for SIGRTMIN+4/+5/+6."
- `src/persistence.rs` — "Notification history serialization (JSON in cache dir)."
- `src/waybar.rs` — "Status JSON + SIGRTMIN+11 refresh signal for the waybar bell module."
- `src/ui/mod.rs` — "GTK4 widgets and layer-shell setup (popup, panel, DND menu, icons)."
- (Each ui submodule)

`cargo doc --document-private-items` becomes meaningfully readable after this.

**Rationale:** Cheap discoverability for future contributors; tools like `rust-analyzer` surface module docstrings prominently.

**Acceptance criteria:**
- [ ] Every `.rs` file in `src/` has a `//!` opener.
- [ ] `cargo doc --document-private-items` builds without missing-docs warnings on the modules.

**Effort:** trivial

**Suggested label:** docs

#### Item 5.2: Document the public-API contracts with `# Errors` / `# Panics` sections where relevant

**Files:** `src/dbus.rs`, `src/state.rs`, `src/persistence.rs`

**Scope:** Functions returning `Result` (e.g. `query_count_via_dbus`, the six `push_*` wrappers) lack `# Errors` rustdoc sections. Functions that `.expect()` on infallible-in-practice cases (`register_object`, `register_nwg_count_object`) lack `# Panics` sections explaining why the panic case can't occur (the XML is `const`, so parsing failure is a build-time bug). Standard rustdoc convention; clippy has a `missing_errors_doc` lint that would flag these.

**Rationale:** Forms part of the broader doc-coverage push.

**Acceptance criteria:**
- [ ] Every `pub fn -> Result` has an `# Errors` section.
- [ ] Every `.expect()` site that's reachable as a panic source has an `# Panics` section on the enclosing function.
- [ ] Optionally: `#![warn(clippy::missing_errors_doc)]` at the crate root once the docs land.

**Effort:** small

**Suggested label:** docs

### 6. Tests

#### Item 6.1: Add unit tests for `notification_row::relative_time`

**Files:** `src/ui/notification_row.rs`

**Scope:** The `relative_time` helper at `notification_row.rs:105-117` is pure (takes a `SystemTime`, returns a `String`) and trivially testable by passing a `SystemTime::now() - Duration`. Currently zero coverage. Add four tests covering the four branches (`now`, `Nm`, `Nh`, `Nd`).

**Rationale:** Pure helper, no GTK dependency, no fixture cost.

**Acceptance criteria:**
- [ ] Four tests for the branches.
- [ ] Edge case: 60s exactly transitions from "now" to "1m"; 3600s exactly to "1h"; 86400s exactly to "1d".

**Effort:** trivial

**Suggested label:** test

#### Item 6.2: Add a `WaybarStatus` shape test for each branch (dnd / unread / empty)

**Files:** `src/waybar.rs`

**Scope:** The existing `status_json_includes_count_field` test covers shape-only. Add three branch tests: DND state produces `class="dnd"` with the right glyph; unread > 0 produces `class="unread"` with count interpolated; unread == 0 produces `class="empty"`. Refactor `update_status` to extract `build_status(unread, dnd) -> WaybarStatus` (pure) and have the public function compose `build_status(...)` + the file-write side effect. The pure half is then directly testable without disk I/O.

**Rationale:** The pluralization branch (`unread == 1` vs `> 1`) is an off-by-one risk that's currently uncovered. Easy to test once `build_status` is extracted.

**Acceptance criteria:**
- [ ] `build_status(unread, dnd) -> WaybarStatus` extracted.
- [ ] Tests cover all three branches and the singular/plural pluralization.

**Effort:** small

**Suggested label:** test, refactor

#### Item 6.3: Add coverage for `clean_markup` edge cases

**Files:** `src/notification.rs`

**Scope:** Existing tests cover the happy path (`<b>`, `<a>`, entities, combined). Missing edge cases:
- Unmatched `<` or `>` (pathological input — what does the daemon do with `5 < x > 3`?)
- Nested tags / malformed nesting
- Numeric entities other than `&#39;` (the spec only lists named entities, so this is a "do we care?" question — flag it explicitly, decide and either test or document the non-support)

**Rationale:** The freedesktop spec is permissive about body markup; pathological inputs from a misbehaving app shouldn't crash or strip wrong text.

**Acceptance criteria:**
- [ ] Tests for unmatched `<` (current code has subtle behaviour: an unclosed `<` swallows everything to EOF).
- [ ] Decide on numeric-entity policy and either implement or document.

**Effort:** small

**Suggested label:** test

#### Item 6.4: Add a hold-guard / startup test (or a code comment explaining why it's untestable)

**Files:** `src/main.rs`

**Scope:** `hold_guard` at `main.rs:133-139` is the GTK convention to keep the GApplication alive past the activate-then-idle window. There's no test for the daemon-doesn't-exit-immediately case. This is hard to test from inside a unit test (you'd need a full GTK environment), but the integration-test track (#16) is the right home — flag it for inclusion when the integration-test infra exists. In the meantime, add a code comment explaining the hold-guard's purpose in one sentence (currently it's "obvious from context if you know GTK"; not obvious otherwise).

**Rationale:** Don't paper over a coverage gap with a fake test — flag it for the integration-test track and document the intent inline.

**Acceptance criteria:**
- [ ] Inline comment on `hold_guard` explains why we hold past activate.
- [ ] #16 (or its successor) gets a sub-bullet to cover the daemon-doesn't-exit-on-idle case.

**Effort:** trivial

**Suggested label:** docs, integration-test-followup

### 7. Dependencies

#### Item 7.1: Audit unused `clap` features and tighten the feature set

**Files:** `Cargo.toml`

**Scope:** `clap = { version = "4", features = ["derive"] }`. Are we using anything beyond `derive`? `value_parser!` and `ValueEnum` are part of derive's transitive surface. `ArgMatches::value_source` may need an explicit feature, but we're using it and it works with current build, so probably already pulled in by `derive`. Run `cargo expand --bin nwg-notifications` and confirm; consider adding explicit `["std"]` if we need to be extra-conservative about no-default-features behaviour.

Same for `nix = { version = "0.31", features = ["signal", "process", "fs"] }` — `process` and `fs` may not be needed; signals are the only thing imported (and only `setup_sigterm_handler` is referenced via nwg_common). Actually nix may not be a direct dep at all by now — confirm.

**Rationale:** Smaller feature surface = faster builds and less audit-surface.

**Acceptance criteria:**
- [ ] `cargo build` works with the tightened feature set.
- [ ] Compile-time delta documented in PR (probably small).

**Effort:** small

**Suggested label:** dependencies

#### Item 7.2: Pin `nwg-common` to a specific 0.3.x minor or evaluate caret semantics

**Files:** `Cargo.toml`

**Scope:** `nwg-common = "0.3.1"` is a caret requirement (matches `>=0.3.1, <0.4.0`). Per the comment ("library + binaries all on the 0.3.x line"), the intent is the entire 0.3 train. That's already what cargo does — comment is accurate but redundant. No action needed; flag as "considered, no change". (Including this so the next reviewer doesn't suggest a pin without seeing it was already evaluated.)

**Rationale:** Pre-empts a future "shouldn't this be pinned more tightly?" review comment.

**Acceptance criteria:**
- [ ] No code change. Doc-only PR (or just a CHANGELOG note) confirming the policy.

**Effort:** trivial

**Suggested label:** dependencies, docs

## Items NOT included (scoped out)

- **Replacing `glib::Variant` index-based parsing with `serde_glib`-style derives.** The XML-driven `gio::DBusMethodInvocation` plumbing is the canonical glib idiom and changing it is a multi-week refactor with no behavioural payoff. Out of scope for a polish pass.
- **Async/zbus migration.** The CLAUDE.md note "no async bridge; runs directly on the glib main loop" is intentional and load-bearing; switching to zbus would require an executor and tokio integration.
- **Splitting `dbus.rs` into multiple files.** At 716 lines it's large but coherent (one server with two interfaces). Item 3.1's collapse-the-handlers pass should bring it down to ~600 LOC; if it's still bothersome after that, revisit.
- **A11y / keyboard navigation in the panel and DND menu.** Real concern but a feature track, not a polish item.
- **Replacing the in-thread `pkill -<n> waybar` invocation with a more direct `kill(pid_of_waybar, sig)`.** `pkill` is fine here because it's idempotent, well-understood, and the waybar process discovery isn't a hot path. Item 1.4's signal-number fix is the only real correctness piece.
- **Adding a config-file fallback (TOML/YAML) parallel to the CLI.** Would be useful but it's a feature, not polish.
- **Reworking the `Notification` struct to be `#[non_exhaustive]` for forward compatibility.** This is a bin crate; `#[non_exhaustive]` only matters for downstream library consumers, of which there are none.
- **Persistence migration tooling for backward-compat reading of older history JSON.** The schema hasn't shifted in any incompatible way; once Item 2.1's filename rename lands, migration is a straight copy. No version-tagged JSON needed yet.
- **Shipping a default systemd-user unit alongside the D-Bus service file.** Tracked separately (out of `src/` scope).

## Suggested epic acceptance

- [ ] All items above either filed as GH issues OR explicitly reclassified as won't-do.
- [ ] No new public API surface added without a corresponding `pub(crate)`-vs-`pub` audit.
- [ ] CHANGELOG entry per merged item, grouped under a "Code-quality polish (post-v0.3.4)" section.
- [ ] CodeRabbit reviews each item PR; any new findings get filed as follow-up issues (per the project's deferral protocol) rather than accumulated.
- [ ] After all items merge, re-run `cargo doc --document-private-items` and `cargo clippy --all-targets -- -W clippy::pedantic` once and document the residual warnings as either fixed or scoped-out in a follow-up issue.
