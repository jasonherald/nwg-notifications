# Rename `mac-notifications-*` → `nwg-notifications-*` + Release v0.4.0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close out epic #29's last open item (#34) by renaming every `mac-notifications-*` runtime artifact to `nwg-notifications-*` (history JSON, status JSON, singleton lock, GTK app_id, layer-shell namespaces) plus the README waybar snippet, ship a one-time history-file auto-migration so existing users don't lose their persisted history, install a one-release dual-check on the singleton lock so an in-flight daemon upgrade doesn't end up with two daemons fighting, and bump to v0.4.0 in the same PR for a single-cycle CodeRabbit review.

**Architecture:** Pure-string rename across `src/`, README, and a CHANGELOG breaking-change entry, plus two small live-system concerns: (a) `paths::migrate_history_if_needed()` runs once at startup before `load_history` and copies `mac-notifications-history.json` → `nwg-notifications-history.json` then unlinks the old file; (b) `main()` peeks for a v0.3.x daemon via `singleton::find_running_pid("mac-notifications")` before claiming the new `"nwg-notifications"` lock, refusing to start if one is found. The version bump to `0.4.0` and the dated CHANGELOG section both land in this PR (single-cycle constraint), so post-merge work is just `git tag v0.4.0 && cargo publish`.

**Tech Stack:** Rust 2024 edition, `nwg_common::singleton` (which exposes both `acquire_lock` and a peek-only `find_running_pid`), standard `std::fs::{copy, remove_file}` for the history migration.

**Tracks:** Closes #34, closes epic **#29** (post-v0.3.4 polish pass — the last remaining child issue). Bumps the crate to **v0.4.0** in the same PR.

---

## File Structure

| Task | Files modified | Test approach |
|------|----------------|---------------|
| Rename strings (production code) | `src/paths.rs` (2 filename strings + 3 docstring references + 3 test asserts), `src/main.rs` (singleton lock name, GTK `application_id`), `src/ui/dnd_menu.rs` (1 layer-shell namespace), `src/ui/panel.rs` (1 layer-shell namespace), `src/waybar.rs` (1 docstring reference) | Existing path tests assert the new filenames; full suite stays green |
| History-file auto-migration | `src/paths.rs` (new `migrate_history_if_needed()` helper + 1 unit test), `src/main.rs` (call it once in `activate_notifications` before `load_history`) | New unit test: pre-create a fake old history file in a tempdir, call the helper, assert the new path exists with the same contents and the old path is gone |
| Singleton lock dual-check | `src/main.rs` (call `find_running_pid("mac-notifications")` before `acquire_lock("nwg-notifications")`) | Smoke test (Task 7) — start a fake v0.3.x daemon by manually creating the old lockfile, then start the new daemon and confirm it refuses |
| README waybar snippet | `README.md` (3 references to `$XDG_RUNTIME_DIR/mac-notifications-status.json`) | Visual diff |
| CHANGELOG breaking-change entry | `CHANGELOG.md` (new `## [0.4.0] — 2026-05-04` section with Changed/Migration/Notes) | Renders as expected in the PR diff |
| Version bump | `Cargo.toml` (`0.3.5` → `0.4.0`), `Cargo.lock` (regenerated) | `cargo build` succeeds; `cargo publish --dry-run` clean |

One commit per task. **CHANGELOG entry IS required this time** — this is a breaking change for any user with custom waybar configs referencing the old path.

---

## Pre-flight

- [ ] **Sync main and create branch**

```bash
cd /data/source/nwg-notifications
git checkout main && git pull --ff-only
git status
git checkout -b release/0.4.0-rename
```

Expected: clean tree on `main`, then a fresh branch. The branch name combines `release/0.4.0` (matching the prior 0.3.x release-PR convention) with `-rename` so the branch's purpose is obvious from `git branch` listings.

- [ ] **Commit the plan file as the first commit on the branch**

```bash
git add docs/superpowers/plans/2026-05-04-rename-mac-to-nwg-and-release-040.md
git commit -m "docs: implementation plan for the mac-* -> nwg-* rename + 0.4.0 release (#34)"
```

- [ ] **Baseline full cargo gambit**

```bash
make lint
```

Expected: every step exits 0; pre-existing `cargo deny` "unmatched skip" warnings are non-blocking. Test count is 91 going in.

---

## Task 1: Rename every `mac-*` string in `src/`

The user-visible `mac-notifications-*` strings: history JSON, status JSON, singleton lock, GTK `application_id`. Plus two `mac-notification-*-backdrop` layer-shell namespace strings (visible to compositors that rule-match on namespace) and three docstring/comment references.

**Files:**
- Modify: `src/paths.rs` — both filename strings + 3 docstring references + 3 test assertions.
- Modify: `src/main.rs` — singleton lock name + GTK `application_id`. (The `acquire_lock` call gets the new name; the dual-check for the old name lands in Task 3.)
- Modify: `src/ui/dnd_menu.rs` — 1 layer-shell namespace string.
- Modify: `src/ui/panel.rs` — 1 layer-shell namespace string.
- Modify: `src/waybar.rs` — 1 docstring reference.

- [ ] **Step 1: Rename the two filename strings + their docstring/test references in `src/paths.rs`**

In `src/paths.rs`, find the two filenames inside `history_path()` and `status_path()`:

```rust
pub(crate) fn history_path() -> PathBuf {
    fallback_user_dir().join("mac-notifications-history.json")
}
```

Replace `"mac-notifications-history.json"` with `"nwg-notifications-history.json"`. Then in `status_path()`:

```rust
        .join("mac-notifications-status.json")
```

Replace `"mac-notifications-status.json"` with `"nwg-notifications-status.json"`.

Now the docstring/comment references. The module docstring near the top mentions:

```rust
//! waybar status JSON to `/tmp/mac-notifications-*.json` would
```

Change to:

```rust
//! waybar status JSON to `/tmp/nwg-notifications-*.json` would
```

And inside `fallback_user_dir`'s docstring:

```rust
///   filename `/tmp/mac-notifications-history.json` could
```

Change to:

```rust
///   filename `/tmp/nwg-notifications-history.json` could
```

And in the `#[cfg(test)] mod tests` block, three assertions that hardcode the old filenames:

```rust
        assert_eq!(actual, runtime.join("mac-notifications-status.json"));
```

```rust
        assert_eq!(actual, cache.join("mac-notifications-history.json"));
```

```rust
        assert_eq!(actual, cache.join("mac-notifications-status.json"));
```

Replace all three with their `nwg-notifications-*` counterparts.

- [ ] **Step 2: Rename the singleton lock name + GTK `application_id` in `src/main.rs`**

In `src/main.rs`, find the `acquire_lock` call:

```rust
    let _lock = match singleton::acquire_lock("mac-notifications") {
```

Replace `"mac-notifications"` with `"nwg-notifications"`. (The dual-check that *peeks* for an old `mac-notifications` daemon lands in Task 3 — this step just claims the new name as the daemon's actual lock.)

Then find the GTK application_id:

```rust
        .application_id("com.mac-notifications.hyprland")
```

Replace `"com.mac-notifications.hyprland"` with `"com.nwg-notifications.hyprland"`. (Reverse-DNS conventions normally use a stable domain identifier; `nwg-notifications` matches the binary name + matches the existing `org.nwg.Notifications` D-Bus surface, so it's the consistent choice.)

- [ ] **Step 3: Rename the two layer-shell namespace strings**

In `src/ui/dnd_menu.rs`, find:

```rust
            "mac-notification-dnd-backdrop",
```

Replace `"mac-notification-dnd-backdrop"` with `"nwg-notification-dnd-backdrop"`.

In `src/ui/panel.rs`, find:

```rust
            "mac-notification-backdrop",
```

Replace `"mac-notification-backdrop"` with `"nwg-notification-backdrop"`.

(These are the layer-shell `Namespace` strings that compositors see; renaming keeps users' window-rule configs consistent with the rest of the rename.)

- [ ] **Step 4: Update the `src/waybar.rs` module docstring reference**

In `src/waybar.rs`, find the module docstring:

```rust
//! Writes a small JSON status file at `$XDG_RUNTIME_DIR/mac-notifications-status.json`
```

Replace `mac-notifications-status.json` with `nwg-notifications-status.json`.

- [ ] **Step 5: Build, test, clippy, fmt**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: clean build; 91 tests still green (the path-helper tests now assert the new filenames); clippy clean; no fmt drift.

- [ ] **Step 6: Confirm no `mac-*` strings remain in `src/`**

```bash
grep -rn "mac-notification\|mac_notification\|com\.mac-notifications" src/ --include="*.rs"
```

Expected: zero matches.

- [ ] **Step 7: Commit**

```bash
git add src/
git commit -m "$(cat <<'EOF'
Rename runtime artifacts mac-notifications-* -> nwg-notifications-* (#34)

Four user-visible legacy identifiers carried over from the
mac-doc-hyprland monorepo era and were inconsistent with the
binary name + every user-facing doc:

- `mac-notifications-history.json` -> `nwg-notifications-history.json`
  (history persistence file in cache dir)
- `mac-notifications-status.json` -> `nwg-notifications-status.json`
  (waybar status file in $XDG_RUNTIME_DIR)
- `mac-notifications` -> `nwg-notifications`
  (singleton lock name; the dual-check peek for the OLD name lands
  in a follow-up commit so the upgrade window doesn't allow two
  daemons to start under different lock names)
- `com.mac-notifications.hyprland` -> `com.nwg-notifications.hyprland`
  (GTK application_id; matches the binary name + the existing
  org.nwg.Notifications D-Bus surface)

Plus two layer-shell namespace strings the compositor sees on the
DND menu + panel backdrop (`mac-notification-dnd-backdrop`,
`mac-notification-backdrop`) — renamed for consistency so users'
window-rule configs match the rest of the rename.

Plus three docstring/comment references in src/paths.rs and
src/waybar.rs that quoted the old paths.

Test assertions in src/paths.rs that hardcoded the old filenames
flipped to the new strings; full 91-test suite still green.

Pure rename; no behavior change yet (the migration helper for the
history file and the dual-check on the singleton lock land in
follow-up commits in this PR).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: One-time history-file auto-migration

Existing v0.3.x users have their history JSON at the old path. After Task 1 the daemon writes/reads at the new path, so without a migration their history would orphan. Add a one-time `migrate_history_if_needed()` helper in `src/paths.rs` that copies `mac-notifications-history.json` → `nwg-notifications-history.json` then unlinks the old file. Call it once at startup in `activate_notifications` before `load_history`.

**Files:**
- Modify: `src/paths.rs` — add `migrate_history_if_needed() -> Option<PathBuf>` helper + 1 new unit test.
- Modify: `src/main.rs` — call the helper once in `activate_notifications` before the `if config.borrow().persist` block.

- [ ] **Step 1: Add the migration helper to `src/paths.rs`**

In `src/paths.rs`, find `history_path()` (the function added/touched in Task 1). Below it, before `status_path()`, add:

```rust
/// Filename of the legacy v0.3.x history JSON. Used only by
/// [`migrate_history_if_needed`] for the one-time upgrade migration
/// — the daemon never writes here at runtime.
const LEGACY_HISTORY_FILENAME: &str = "mac-notifications-history.json";

/// One-time migration: if the new history path doesn't exist and
/// the legacy v0.3.x path *does* exist in the same directory, copy
/// the legacy file's contents to the new path and unlink the old
/// file. Returns `Some(new_path)` if a migration happened, `None`
/// otherwise.
///
/// Idempotent: a second call after a successful migration finds the
/// new file already present and returns `None`. Logs at info-level
/// on a successful migration so the operator sees the one-time event
/// in the journal; warns on partial failure (copy succeeded, unlink
/// failed) so a stale-file accumulation is observable rather than
/// silent.
///
/// Called once at startup from `activate_notifications` before
/// `persistence::load_history`. The waybar status file is *not*
/// migrated — it's a transient runtime artifact the daemon rewrites
/// on every state change.
pub(crate) fn migrate_history_if_needed() -> Option<PathBuf> {
    let new_path = history_path();
    if new_path.exists() {
        return None;
    }
    // Construct the legacy path inside the same parent directory the
    // new path lives in. fallback_user_dir resolution rules apply
    // identically so the legacy file is in the same XDG-resolved
    // location the new file would be.
    let parent = new_path.parent()?;
    let legacy_path = parent.join(LEGACY_HISTORY_FILENAME);
    if !legacy_path.exists() {
        return None;
    }
    if let Err(e) = std::fs::copy(&legacy_path, &new_path) {
        log::warn!(
            "Failed to migrate legacy history file {} -> {}: {}",
            legacy_path.display(),
            new_path.display(),
            e
        );
        return None;
    }
    log::info!(
        "Migrated legacy history file {} -> {}",
        legacy_path.display(),
        new_path.display()
    );
    if let Err(e) = std::fs::remove_file(&legacy_path) {
        log::warn!(
            "Migrated history file but failed to unlink legacy {}: {}",
            legacy_path.display(),
            e
        );
    }
    Some(new_path)
}
```

- [ ] **Step 2: Wire the helper into `src/main.rs::activate_notifications`**

In `src/main.rs`, find the line that currently reads:

```rust
    let history_path = paths::history_path();
```

Insert immediately above it:

```rust
    // One-time v0.3.x -> v0.4.0 history migration. Idempotent on
    // every subsequent startup once the migration has run.
    paths::migrate_history_if_needed();
```

- [ ] **Step 3: Add the unit test for the migration helper**

In `src/paths.rs`'s `#[cfg(test)] mod tests` block, append (after the existing tests, before the closing `}` of the test module):

```rust
    #[test]
    fn migrate_history_if_needed_copies_and_unlinks_legacy_file() {
        // Build a controlled XDG_CACHE_HOME pointing at a fresh
        // tempdir, pre-create the legacy history file there with
        // distinctive contents, then call migrate_history_if_needed
        // and assert (a) the new file exists with the same contents,
        // (b) the legacy file was unlinked, (c) a second call is a
        // no-op (idempotency).
        let tmpdir = std::env::temp_dir().join(format!(
            "nwg-paths-migration-test-{}",
            std::process::id()
        ));
        // Clean any leftover from a previous failed run.
        let _ = std::fs::remove_dir_all(&tmpdir);
        std::fs::create_dir_all(&tmpdir).expect("setup tmpdir");
        let cache = tmpdir.join("cache");
        std::fs::create_dir_all(&cache).expect("setup cache");
        let legacy = cache.join("mac-notifications-history.json");
        let new = cache.join("nwg-notifications-history.json");
        let payload = b"[{\"id\":1,\"summary\":\"legacy\"}]";
        std::fs::write(&legacy, payload).expect("seed legacy file");

        let result = with_env(
            &[("XDG_CACHE_HOME", Some(cache.to_str().unwrap()))],
            migrate_history_if_needed,
        );

        assert_eq!(
            result,
            Some(new.clone()),
            "migrate should report the new path on success"
        );
        assert!(new.exists(), "new history file should exist after migration");
        assert!(
            !legacy.exists(),
            "legacy history file should be unlinked after migration"
        );
        let migrated = std::fs::read(&new).expect("read migrated file");
        assert_eq!(
            migrated.as_slice(),
            payload,
            "migrated file should have the same contents as the legacy one"
        );

        // Idempotency: second call is a no-op.
        let result_again = with_env(
            &[("XDG_CACHE_HOME", Some(cache.to_str().unwrap()))],
            migrate_history_if_needed,
        );
        assert_eq!(
            result_again, None,
            "second call should be a no-op (new file already present)"
        );

        let _ = std::fs::remove_dir_all(&tmpdir);
    }
```

- [ ] **Step 4: Build, test, clippy, fmt**

```bash
cargo build
cargo test paths::tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: 1 new test passes; full path-tests block still green; clippy clean; no fmt drift. Total test count: 91 → 92.

- [ ] **Step 5: Commit**

```bash
git add src/paths.rs src/main.rs
git commit -m "$(cat <<'EOF'
One-time history-file migration mac-* -> nwg-* (#34 follow-up)

Without a migration, every v0.3.x user upgrading to 0.4.0 would
silently lose their persisted notification history because the
file moves from mac-notifications-history.json to
nwg-notifications-history.json under the cache dir.

Add migrate_history_if_needed() in src/paths.rs:
- If the new path already exists, no-op (idempotent on every
  subsequent startup).
- Otherwise, look in the same parent directory for the legacy
  mac-notifications-history.json. If found, copy contents to the
  new path and unlink the legacy file.
- Logs at info on successful migration; warns on partial failure
  (copy ok, unlink failed) so stale-file accumulation is
  observable rather than silent.

Wire it into activate_notifications in main.rs to run once at
startup, immediately before persistence::load_history.

Status file is intentionally NOT migrated — it's a transient
runtime artifact the daemon rewrites on every state change, so
the legacy /run/user/.../mac-notifications-status.json simply
goes stale and the new file appears on first state change.

New unit test pre-creates a fake legacy file in a tempdir,
overrides XDG_CACHE_HOME via the existing with_env serialization
helper, calls the migration, and asserts (a) new file exists
with same contents, (b) legacy file was unlinked, (c) second
call is a no-op.

Test count: 91 -> 92.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Singleton-lock dual-check (one-release transition)

When a user upgrades from v0.3.5 to v0.4.0 mid-session, the v0.3.5 daemon is still running under the lock name `mac-notifications`. Without a dual-check, the v0.4.0 daemon claims the new lock name `nwg-notifications`, sees no conflict, and starts — leaving two notification daemons concurrently fighting over `org.freedesktop.Notifications`. (D-Bus name ownership eventually resolves with `BusNameOwnerFlags::REPLACE`, but for one user-action cycle the wrong daemon could handle a notification.)

`nwg_common::singleton::find_running_pid(app_name)` is a peek-only helper that returns the PID of a running daemon under that name without acquiring or modifying anything. Use it to detect a v0.3.x daemon before claiming the new lock; refuse to start with a clear error if one is found.

**Files:**
- Modify: `src/main.rs` — call `find_running_pid("mac-notifications")` before `acquire_lock("nwg-notifications")`.

- [ ] **Step 1: Add the dual-check above the new-name lock acquisition**

In `src/main.rs`, find the block that currently reads (after Task 1's rename):

```rust
    let _lock = match singleton::acquire_lock("nwg-notifications") {
        Ok(lock) => lock,
        Err(existing_pid) => {
            if let Some(pid) = existing_pid {
                log::info!("Already running (pid {pid})");
            }
            std::process::exit(0);
        }
    };
```

Insert immediately above the `let _lock` line:

```rust
    // One-release backwards-compat: detect a v0.3.x daemon that's
    // still using the legacy "mac-notifications" singleton lock
    // name. If one is found, refuse to start so we don't end up
    // with two notification daemons fighting over the
    // org.freedesktop.Notifications D-Bus name during the upgrade
    // window. Will be removed in v0.5.0 (one-release deprecation
    // window per the CHANGELOG entry for #34).
    if let Some(legacy_pid) = singleton::find_running_pid("mac-notifications") {
        log::info!(
            "A legacy v0.3.x nwg-notifications daemon is running under the \
             'mac-notifications' singleton lock (pid {legacy_pid}). Refusing \
             to start. Stop the old daemon first: kill {legacy_pid}"
        );
        eprintln!(
            "nwg-notifications: a legacy v0.3.x instance is already running \
             (pid {legacy_pid}, under the old singleton-lock name 'mac-notifications')."
        );
        eprintln!("Stop it first:  kill {legacy_pid}");
        std::process::exit(0);
    }

```

(Blank line at the end keeps the existing `let _lock` block visually grouped with the original logic.)

- [ ] **Step 2: Build, test, clippy, fmt**

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: clean build; 92 tests still green; clippy clean; no fmt drift.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "$(cat <<'EOF'
Dual-check singleton lock for one-release v0.3.x upgrade safety (#34 follow-up)

When a user upgrades from v0.3.5 to v0.4.0 mid-session, the
v0.3.5 daemon is still running under the lock name
"mac-notifications" while v0.4.0 claims "nwg-notifications".
Without a dual-check, both daemons start successfully and
race for the org.freedesktop.Notifications D-Bus name —
BusNameOwnerFlags::REPLACE eventually resolves, but for one
user-action cycle the wrong daemon could handle a notification.

Add a pre-acquire peek using nwg_common::singleton::find_running_pid
("mac-notifications"). The helper returns the PID without
acquiring or modifying any lock file. If a v0.3.x daemon is
detected, refuse to start with a clear log + stderr message
naming the PID so the operator knows what to kill.

The dual-check is one-release; v0.5.0 drops it.

No new tests — exercising find_running_pid against a synthetic
lockfile would require either fork or a tempfile harness, neither
worth the complexity for a one-release transition. Smoke test
in Task 7 covers the end-to-end upgrade scenario.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Update the README waybar snippet

Three references in `README.md` to `$XDG_RUNTIME_DIR/mac-notifications-status.json` need to become `nwg-notifications-status.json`. Users following the README waybar config will get the right path going forward; users with existing waybar configs need to update theirs (covered in the CHANGELOG migration note in Task 5).

**Files:**
- Modify: `README.md` — 3 references.

- [ ] **Step 1: Find and replace all three references**

In `README.md`, find:

```text
"exec": "cat $XDG_RUNTIME_DIR/mac-notifications-status.json 2>/dev/null || echo '{\"text\":\"\",\"alt\":\"empty\",\"class\":\"empty\"}'",
```

Change `mac-notifications-status.json` to `nwg-notifications-status.json`.

Then find:

```text
The daemon writes its current state to `$XDG_RUNTIME_DIR/mac-notifications-status.json` and signals waybar (`SIGRTMIN+11`, which waybar receives as `signal: 11`) whenever the state changes — no polling.
```

Same rename.

Then find:

```text
jq -r .count "$XDG_RUNTIME_DIR/mac-notifications-status.json"
```

Same rename.

- [ ] **Step 2: Confirm no `mac-notifications` strings remain in README**

```bash
grep -n "mac-notifications" README.md ; echo "(no output = clean)"
```

Expected: no output.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "$(cat <<'EOF'
README: update waybar snippet for the nwg-notifications-* rename (#34 follow-up)

Three references in README.md pointed at the legacy
\$XDG_RUNTIME_DIR/mac-notifications-status.json path:
- The waybar config snippet's `"exec":` line
- The "Waybar integration" section's prose description
- The shell example showing `jq -r .count "$XDG_RUNTIME_DIR/..."`

All three now reference `nwg-notifications-status.json`. Users
with existing waybar configs need to update theirs; the CHANGELOG
breaking-change entry in this PR covers the migration story.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: CHANGELOG breaking-change entry for v0.4.0

Per the user-comms convention, this is the one bundle that requires a CHANGELOG entry — it's a breaking change for users with custom waybar configs. The entry needs to cover what changed, the auto-migration that runs (so users don't have to do anything for history), the manual step for waybar configs, and the one-release dual-check on the singleton lock.

Because the version bump lands in this same PR (Task 6), the CHANGELOG section uses the dated form `## [0.4.0] — 2026-05-04` directly — not the `## [Unreleased]` interim that prior multi-PR releases used.

**Files:**
- Modify: `CHANGELOG.md` — insert a new `## [0.4.0] — 2026-05-04` section above `## [0.3.5]`.

- [ ] **Step 1: Insert the new section**

In `CHANGELOG.md`, find the line:

```markdown
## [0.3.5] — 2026-05-03
```

Insert immediately above it (with a trailing blank line):

```markdown
## [0.4.0] — 2026-05-04

### Changed (Breaking)

- **Renamed runtime artifacts from `mac-notifications-*` to
  `nwg-notifications-*`** (#34). Four user-visible legacy
  identifiers carried over from the pre-split mac-doc-hyprland
  monorepo era and were inconsistent with the binary name + every
  user-facing doc. Specifically:
  - `$XDG_CACHE_HOME/mac-notifications-history.json` →
    `$XDG_CACHE_HOME/nwg-notifications-history.json` (history JSON).
    **Migrated automatically on first startup** — the daemon copies
    the legacy file to the new path then unlinks the legacy file,
    so users with persisted history don't lose anything. Idempotent
    on subsequent startups.
  - `$XDG_RUNTIME_DIR/mac-notifications-status.json` →
    `$XDG_RUNTIME_DIR/nwg-notifications-status.json` (waybar status
    JSON). **Manual update required** — anyone with a custom waybar
    config referencing the old path needs to point it at the new
    path. The README waybar snippet already shows the new path. The
    daemon writes the new file on every state change after upgrade;
    the legacy file becomes stale (clear it manually if you care).
  - Singleton lock name `mac-notifications` → `nwg-notifications`.
    **One-release transition.** v0.4.0 peeks for a v0.3.x daemon
    under the legacy lock name on startup and refuses to start if
    one is running, with a clear `kill <pid>` message — so
    upgrading mid-session is safe. v0.5.0 will drop this peek.
  - GTK `application_id` `com.mac-notifications.hyprland` →
    `com.nwg-notifications.hyprland`. Visible to D-Bus
    introspection and any compositor rule-matching on app-id.
  - Layer-shell namespaces `mac-notification-backdrop` and
    `mac-notification-dnd-backdrop` → `nwg-notification-backdrop`
    and `nwg-notification-dnd-backdrop`. Visible to compositors
    that rule-match on namespace.

### Notes

- Closes the post-v0.3.4 polish-pass epic (#29). Everything in
  v0.4.0 except the rename itself was internal cleanup; the rename
  is the one breaking change that pushed the version bump from
  patch to minor.
- `0.4.0` is still pre-1.0; the breaking change here is bounded to
  the runtime-artifact filenames + a one-release lock-name
  transition, all spelled out above.

```

(Trailing blank line preserves Keep-a-Changelog spacing.)

- [ ] **Step 2: Visual sanity-check the rendered CHANGELOG diff**

```bash
git diff CHANGELOG.md | head -60
```

Expected: a clean unified diff showing the new `## [0.4.0]` section inserted above `## [0.3.5]`, no unintended edits elsewhere.

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "$(cat <<'EOF'
docs(changelog): record v0.4.0 with the mac-* -> nwg-* breaking change (#34)

Closes the post-v0.3.4 polish-pass epic (#29). The whole epic
shipped over PRs #52, #54, #55, #56, #57, #58, #59 as internal
cleanup with no CHANGELOG entries — every commit was either pure
docs, internal refactor, test coverage, or fallible-cast hygiene
with zero user-visible impact. The mac-* -> nwg-* rename in this
PR is the one user-facing change in the entire epic, and it's
breaking enough (waybar configs need updating) to push the
version bump from patch to minor.

CHANGELOG entry covers:
- The four renamed identifiers + the two layer-shell namespaces.
- The auto-migration for the history JSON (idempotent on first
  startup; users don't have to do anything).
- The manual step for waybar configs (update the path in your
  config; daemon writes the new file going forward).
- The one-release dual-check on the singleton lock so upgrading
  mid-session is safe; v0.5.0 will drop the peek.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Bump `Cargo.toml` to `0.4.0` + regenerate `Cargo.lock`

Same shape as the prior 0.3.x release-PR commits.

**Files:**
- Modify: `Cargo.toml` (`version = "0.3.5"` → `"0.4.0"`).
- Modify: `Cargo.lock` (regenerated).

- [ ] **Step 1: Bump the version in `Cargo.toml`**

In `Cargo.toml`, find:

```toml
version = "0.3.5"
```

Replace with:

```toml
version = "0.4.0"
```

- [ ] **Step 2: Regenerate `Cargo.lock`**

```bash
cargo update --workspace -p nwg-notifications 2>&1 | tail -3
grep -A1 'name = "nwg-notifications"' Cargo.lock | head -4
```

Expected: `Updating nwg-notifications v0.3.5 -> v0.4.0` in the cargo output, and the lockfile entry now reads `version = "0.4.0"`.

- [ ] **Step 3: Build + dry-run publish**

```bash
cargo build --release
cargo publish --dry-run 2>&1 | tail -10
```

Expected: clean release build; dry-run publish succeeds with `aborting upload due to dry run` as the final line. No errors about manifest fields or version conflicts.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "$(cat <<'EOF'
chore: release 0.4.0

Bumps version 0.3.5 -> 0.4.0 and ships the mac-* -> nwg-*
runtime-artifact rename (#34) as the one breaking change in this
release. CHANGELOG entry in this same PR covers the migration
story.

Closes the post-v0.3.4 polish-pass epic (#29) — every other
issue in the epic shipped under v0.3.5 as internal-cleanup PRs
with no user-visible impact.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Pre-PR gates + smoke install + **STOP for user smoke**

This release is more substantive than the prior internal-cleanup bundles — the migration path needs a real upgrade scenario tested. The user has explicitly asked to smoke-test before the PR opens.

- [ ] **Step 1: Install to user bin and confirm restart works**

```bash
make upgrade PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
```

Expected: build → install → kill running daemon → respawn. `pidof nwg-notifications` returns the new PID.

- [ ] **Step 2: Capture the legacy file paths (for the smoke handoff)**

```bash
ls -la "$XDG_RUNTIME_DIR"/mac-notifications-status.json 2>&1 | head -2
ls -la "$XDG_CACHE_HOME"/mac-notifications-history.json 2>&1 || \
  ls -la ~/.cache/mac-notifications-history.json 2>&1 | head -2
ls -la "$XDG_RUNTIME_DIR"/nwg-notifications-status.json 2>&1 | head -2
ls -la "$XDG_CACHE_HOME"/nwg-notifications-history.json 2>&1 || \
  ls -la ~/.cache/nwg-notifications-history.json 2>&1 | head -2
```

This is informational — shows whether the legacy files still exist (depending on whether the user had history before the upgrade), and whether the new files appeared.

- [ ] **Step 3: Hand off to the user — STOP HERE**

Tell the user (verbatim or close):

> Installed and restarted via `make upgrade`. **This is a real upgrade
> scenario — please walk through these checks carefully before I open
> the PR**:
>
> **1. History migration:**
>
> - Check whether `~/.cache/mac-notifications-history.json` (or wherever `XDG_CACHE_HOME` points) used to exist with your pre-upgrade history. If it did, the daemon should have migrated it on startup — verify that the new file `~/.cache/nwg-notifications-history.json` exists with the same contents and the legacy one is gone.
> - `journalctl --user -u mac-doc-hyprland.service --since '5 minutes ago'` (or wherever the daemon's stdout goes) should show `Migrated legacy history file ... -> ...` if the migration ran.
> - Open the panel via `kill -RTMIN+4 $(pidof nwg-notifications)` — your old history should appear.
>
> **2. Waybar status file:**
>
> - `notify-send "smoke" "test after rename"` to generate a state change.
> - `ls -la "$XDG_RUNTIME_DIR"/nwg-notifications-status.json` should show a file that exists and was just written.
> - The legacy `$XDG_RUNTIME_DIR/mac-notifications-status.json` is not migrated (it's a transient artifact); it'll go stale until manually deleted. That's expected.
> - Your waybar config still references the old path — **expect the bell module to show empty / stale until you update the waybar config to the new filename**. This is the breaking change the CHANGELOG warns about. (Update your waybar config to match the new path + reload waybar to confirm the bell starts updating again.)
>
> **3. Singleton dual-check (the trickiest one):**
>
> This is hard to smoke-test post-hoc because the upgrade already replaced the running daemon. To exercise it: stop the new daemon, then synthesize a fake legacy lockfile, then try to start the new daemon. It should refuse:
>
> ```bash
> kill $(pidof nwg-notifications) 2>/dev/null || true
> sleep 1
> # Synthesize a fake v0.3.x lock holding our own shell's PID
> # (so it looks alive). The user-hash filename pattern matches
> # what nwg_common::singleton expects.
> USER_HASH=$(printf "%s" "$USER" | cksum | awk '{print $1}')
> LEGACY_LOCK="/tmp/mac-notifications-${USER_HASH}.lock"
> echo $$ > "$LEGACY_LOCK"
> # Try to start the new daemon — should refuse with an error
> # message naming the fake PID:
> nwg-notifications --persist 2>&1 | head -5
> # Cleanup:
> rm -f "$LEGACY_LOCK"
> ```
>
> Expected output: `nwg-notifications: a legacy v0.3.x instance is already running (pid <your-shell-pid>, under the old singleton-lock name 'mac-notifications').` followed by `Stop it first: kill <your-shell-pid>`. Daemon exits 0 without claiming the new lock.
>
> **This synthetic test depends on `nwg_common`'s user-hash function matching `cksum` — it might not.** If the synthetic lock doesn't trigger the dual-check (i.e. the daemon starts normally), don't worry — that's a test-rig issue, not a bug in the dual-check itself. Reply with the output and I'll investigate.
>
> Then `nwg-notifications --persist &` to start the new daemon for real and confirm the rest of the daemon still works.
>
> **4. End-to-end smoke (with the new daemon running):**
>
> - `notify-send "final" "smoke check"` — popup appears.
> - Open the panel, see the popup in history.
> - Right-click the waybar bell → DND menu opens.
>
> Reply with what worked and what didn't. **Do not let me open the PR until you've had a chance to verify the migration on your live system.**

**Do not proceed to Task 8 until the user explicitly approves the smoke results.** If anything fails, return to the broken task — the migration helper, the dual-check, or the path renames are the most likely places to need a fix.

- [ ] **Step 4: Full lint after smoke approval**

```bash
make lint
```

Expected: every step exits 0; total test count is 92.

- [ ] **Step 5: Push**

```bash
git push -u origin release/0.4.0-rename
```

---

## Task 8: Open PR

- [ ] **Step 1: Open the PR**

The outer fence below uses **four backticks** so the inner three-backtick `bash` block in the post-merge instructions doesn't terminate the outer fence prematurely (markdownlint MD040/MD031 flag the collision otherwise).

````bash
gh pr create --base main --head release/0.4.0-rename \
  --title "release: 0.4.0 — rename runtime artifacts to nwg-notifications-* (#34)" \
  --body "$(cat <<'EOF'
## Summary

Closes the post-v0.3.4 polish-pass epic (**#29**) by shipping the last open child issue (**#34**) plus the v0.4.0 release in a single PR (single-cycle CodeRabbit constraint). The minor-version bump is justified by the rename being a breaking change for users with custom waybar configs.

- **Rename** four user-visible legacy identifiers + two layer-shell namespaces from `mac-notifications-*` to `nwg-notifications-*` (history JSON, status JSON, singleton lock, GTK `application_id`, plus the `mac-notification-backdrop` and `mac-notification-dnd-backdrop` namespace strings the compositor sees).
- **Auto-migrate** the history JSON on first startup (`paths::migrate_history_if_needed()` runs once before `load_history`; copies `mac-notifications-history.json` → `nwg-notifications-history.json` then unlinks the legacy file; idempotent on subsequent startups). Users with persisted history don't have to do anything.
- **One-release dual-check** on the singleton lock: `main()` peeks via `singleton::find_running_pid("mac-notifications")` before claiming the new `"nwg-notifications"` lock and refuses to start if a v0.3.x daemon is detected, with a clear `kill <pid>` message. v0.5.0 will drop the peek.
- **README waybar snippet** updated (3 references to the old status-file path).
- **CHANGELOG** gets a `## [0.4.0] — 2026-05-04` section spelling out the breaking change + the migration story.
- **Cargo.toml + Cargo.lock** bumped from `0.3.5` to `0.4.0`.

## Test plan

- [x] `make lint` clean locally (fmt + clippy + test + deny + audit). Test count: 91 → 92 (+1 for the migration helper test).
- [x] `cargo publish --dry-run` clean.
- [x] **Manual upgrade smoke test against the live compositor** (installed via `make upgrade PREFIX=\$HOME/.local BINDIR=\$HOME/.cargo/bin`):
  - [x] History file migrated correctly (legacy file gone, new file with same contents, `Migrated legacy history file ... -> ...` in journal).
  - [x] Waybar bell updates against the new status-file path (after updating the waybar config).
  - [x] Singleton dual-check rejects a synthesized legacy v0.3.x lockfile with the expected error message.
  - [x] End-to-end: `notify-send` produces a popup, panel opens, DND menu opens.

## Post-merge

After merge, cut the release:

```bash
git checkout main && git pull --ff-only
git tag -a v0.4.0 -m "v0.4.0 — rename runtime artifacts to nwg-notifications-*"
git push origin v0.4.0
cargo publish
gh release create v0.4.0 --title "v0.4.0" --notes "$(... see CHANGELOG ...)"
```

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
````

- [ ] **Step 2: Wait for CodeRabbit + iterate**

Default-fix posture per repo convention. Inline reply per in-diff comment, single PR-level reply for outside-diff items, tag `@coderabbitai` every time. Do not respond to non-bot commenters under the maintainer's account.

---

## Notes

- **Why one PR for the rename + release?** The user is at the CodeRabbit per-day quota and can't afford two PR-review cycles. Bundling the rename + the version bump into the same PR cuts the review cost in half. The downside (a bigger diff for one review) is acceptable because the rename is mostly mechanical and the migration logic is well-scoped (~30 LOC + 1 test).
- **Why not migrate the waybar status file?** It's a transient runtime artifact the daemon rewrites on every state change. Migrating it would require either (a) writing the new file at startup before any state change (one-shot copy), or (b) keeping a dual-write path for one release. (a) is fragile (the status reflects current state, not legacy state — a copy would be wrong). (b) is more complexity than the per-release migration is worth. The CHANGELOG calls out the manual waybar-config update step instead.
- **Why a one-release dual-check on the lock instead of forever?** The cost of carrying the peek forever is one `find_running_pid` call at startup — basically free. The cost of dropping it after one release is one CHANGELOG note saying "if you're upgrading from <0.4.0, restart your session." Either is acceptable; the one-release window is the standard cleanup posture for a removed-name compatibility shim.
