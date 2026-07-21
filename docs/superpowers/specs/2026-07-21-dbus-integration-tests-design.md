# D-Bus integration tests — design

**Date:** 2026-07-21
**Issue:** [#16](https://github.com/jasonherald/nwg-notifications/issues/16)
**Branch:** `feat/dbus-integration-tests`

## Goal

Integration coverage for the D-Bus helpers that need a live
`gio::DBusConnection` and therefore have no unit-test seam today:
`query_count_via_dbus` (client side of `GetCount`), `emit_count_changed`,
and `emit_action_invoked` — plus a liveness test for the
`gio::ApplicationHoldGuard` pattern that keeps the daemon resident past
GTK `activate`. Maps 1:1 to #16's acceptance checklist.

## Constraints

- **Must run on GitHub-hosted runners** (`ubuntu-latest`). The bus tests
  therefore use only gio + an isolated session bus — no GTK init, no
  display, no compositor. The one test that needs a real daemon (hold-guard
  liveness) skips gracefully off-CI-capable machines and is a dev-machine
  test for now (follow-up issue if we ever want Sway in Actions).
- Plain `cargo test` (and the existing test.yml job) must stay green on
  machines with no session bus at all.
- The integration harness stays reusable-shaped for the rest of the nwg-*
  family (this is the seed for the broader integration-testing initiative).

## Crate restructure: lib + bin

`tests/` integration tests can only link a library target, and the crate is
binary-only today. So:

- New `src/lib.rs` declares the existing module tree and re-exports the
  coordinator as `pub fn run() -> glib::ExitCode`. The coordinator body
  moves from `main.rs` to a new `src/app.rs`; `lib.rs` itself stays
  declarations-only.
- `src/main.rs` shrinks to a shim: `fn main() -> glib::ExitCode { nwg_notifications::run() }`.
- Test seams go `pub` but `#[doc(hidden)]`, everything else stays
  crate-private:
  - `dbus::query_count_via_dbus`
  - `dbus::emit_count_changed`, `dbus::emit_action_invoked`
  - `dbus::NWG_COUNT_BUS_NAME`, `dbus::NWG_COUNT_OBJECT_PATH` (fixture
    reuses the crate's own strings instead of duplicating them)
  - `dbus::QUERY_COUNT_TIMEOUT_MS` (timeout assertion margin)
  - `paths::status_path` only if a test ends up needing it (avoid
    widening preemptively).
- Crate-level lib doc: the library target exists to give the integration
  suite a linkable seam; it is **not a public API** and carries **no
  semver guarantees**. CHANGELOG entry says the same.
- Publishing note: the lib target will ship to crates.io alongside the
  binary. `#[doc(hidden)]` + the disclaimer is the standard pattern
  (rustc/tokio internals do the same); acceptable trade-off for real
  integration tests.

## Harness mechanics

- `tests/dbus_integration.rs`, every test marked
  `#[ignore = "needs isolated session bus — run via make test-integration"]`.
  Plain `cargo test` compiles the file (so clippy/fmt cover it) but skips
  every case.
- New Makefile target:

  ```make
  test-integration:
      dbus-run-session -- cargo test --test dbus_integration -- --ignored --test-threads=1
  ```

  plus a `command -v dbus-run-session` guard with an actionable error.
  `dbus-run-session` provides the isolated bus (child sees
  `DBUS_SESSION_BUS_ADDRESS`, which `gio::bus_get_sync(Session)` honors)
  and tears it down on exit. `--test-threads=1` because tests own the one
  well-known `org.nwg.Notifications` name; parallel owners would collide.
- Fixture (in `tests/` support code, not shipped): `bus_own_name` on
  `NWG_COUNT_BUS_NAME` + `register_object` at `NWG_COUNT_OBJECT_PATH`
  with a minimal introspection XML exposing `GetCount() -> (u)` and the
  `CountChanged(u)` signal, backed by a controlled count value. A
  "hanging" variant registers the method but never calls
  `invocation.return_value(..)`, so the client times out.
- Signal assertions: `connection.signal_subscribe(..)` on the same bus,
  call the `emit_*` helper under test directly, then pump
  `glib::MainContext::default()` iterations with a wall-clock deadline
  until the subscription fires or the deadline passes. Generous deadlines
  (seconds, not tight bounds) — GitHub runners are slow and shared.

## Test inventory

| # | Test | Asserts | Acceptance box |
|---|------|---------|----------------|
| 1 | `get_count_round_trip` | fixture count N → `query_count_via_dbus()` returns `Ok(N)` | round-trip vs fixture |
| 2 | `get_count_no_daemon_errors` | no name owner → `Err` (NO_AUTO_START semantics; no daemon spawned) | NO_AUTO_START |
| 3 | `get_count_times_out_on_hung_daemon` | hanging fixture → `Err` after ~`QUERY_COUNT_TIMEOUT_MS`, well before 2× | timeout behavior |
| 4 | `emit_count_changed_wire_payload` | subscriber sees `CountChanged` with the exact `(u,)` payload | signal emission |
| 5 | `emit_action_invoked_wire_payload` | subscriber on `/org/freedesktop/Notifications` sees `ActionInvoked` with `(u, s)` payload | signal emission (plural) |
| 6 | `daemon_stays_resident_hold_guard` | spawn the real binary under headless Sway (`WLR_BACKENDS=headless`, throwaway config, isolated bus), wait past activate, process still alive; SIGTERM cleans up. **Runtime-skips with a message when `sway` is not on PATH.** | hold-guard liveness |

Test 6 lives in the same file behind the same `#[ignore]`, with the sway
check at the top — one make target everywhere, CI simply reports it
skipped.

## CI wiring

New job in `test.yml` (same trigger matrix):

```yaml
integration:
  name: D-Bus integration tests
  runs-on: ubuntu-latest
  timeout-minutes: 20
  steps:
    - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4, same pin as the test job
    - uses: ./.github/actions/setup-gtk4
    - run: sudo apt-get install -y --no-install-recommends dbus dbus-bin
    - run: make test-integration
```

- Reuses the `setup-gtk4` composite action (GTK dev headers are needed to
  *compile* the crate even though bus tests never init GTK).
- `dbus`/`dbus-bin` provide `dbus-daemon` + `dbus-run-session`; installing
  explicitly rather than assuming the runner image ships them.
- Liveness test auto-skips (no sway on the runner) — by design per the
  CI-scope decision.

## Docs + changelog

- CLAUDE.md Build & test: add
  `make test-integration  # D-Bus integration tests (isolated bus via dbus-run-session; liveness test needs sway)`
  and rewrite the harness paragraph to describe the local runner
  (superseding the "#16 tracks adding one" wording from PR #80 — rebase
  over #80's merge before touching that paragraph).
- CHANGELOG under the active unreleased section, `### Added`: integration
  suite + the lib-target note (no semver guarantees on the lib surface).
  The lib-target addition likely argues for `0.6.0` over `0.5.1` at
  release time — release PR's call, flagged there.
- README: no change (contributor-only surface).

## Error handling

- Missing `dbus-run-session` → make target fails fast with install hint.
- Fixture registration failures → test panics with the gio error (loud).
- Deadline-based pumps always have an explicit failure branch
  (`panic!("signal not received within {}s", ..)`) — no silent hangs; the
  suite can't wedge CI past its timeout-minutes.
- Liveness test always reaps the spawned sway + daemon processes
  (kill-on-drop guard struct), including on assertion failure — no orphan
  compositors on dev machines.

## Out of scope

- Sway-in-Actions for the liveness test (follow-up issue if wanted).
- The freedesktop `Notify` path integration tests (separate surface, own
  issue when tackled).
- Legacy-lock-peek removal (#81) — separate PR even though it touches the
  same startup path the liveness test exercises.
