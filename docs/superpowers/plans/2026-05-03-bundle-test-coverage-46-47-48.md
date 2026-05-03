# Bundle Test Coverage (#46, #47, #48) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bundle three test-coverage items from epic #29 (post-v0.3.4 cleanup) into one PR — unit tests for the `relative_time` helper (#46), branch + pluralization tests for the waybar status JSON (#47), and edge-case tests for `clean_markup` (#48).

**Architecture:** Each issue is purely additive `#[test]` blocks plus, for #46 and #47, a small refactor to extract a pure helper (so the test can target the pure half without faking out wall-clock or disk-I/O side effects). #48 needs no refactor — `clean_markup` is already pure. One commit per issue, in the order #46 → #47 → #48 because the refactors get progressively larger.

**Tech Stack:** Rust, `std::time::{SystemTime, Duration}`, `serde_json` for the existing waybar status round-trip test.

**Tracks:** Closes #46, #47, #48. All three are children of epic #29.

---

## File Structure

| Task | Files modified | Test approach |
|------|----------------|---------------|
| #46 `relative_time` tests | `src/ui/notification_row.rs` (extract `relative_time_from_elapsed(Duration)` pure helper + new `#[cfg(test)] mod tests`) | 7 unit tests covering 4 branches + 3 boundary cases |
| #47 `WaybarStatus` shape tests | `src/waybar.rs` (extract `build_status(unread, dnd) -> WaybarStatus` pure helper) | 4 unit tests: dnd, unread==1 singular, unread>1 plural, empty |
| #48 `clean_markup` edge cases | `src/notification.rs` (tests only, plus a doc comment on `clean_markup` documenting the unmatched-`<` swallow-to-EOF and numeric-entity policy) | 4 unit tests: nested tags, malformed nesting, unmatched `<`, unsupported numeric entities |

Each issue gets its own commit. No CHANGELOG entry — this bundle is internal coverage with zero user-visible impact (per the discussion on PR #54). Plan file commits first per repo convention.

---

## Pre-flight

- [ ] **Sync main and create branch**

```bash
cd /data/source/nwg-notifications
git checkout main && git pull --ff-only
git status
git checkout -b chore/test-coverage-46-47-48
```

Expected: clean tree on `main`, then a fresh branch.

- [ ] **Commit the plan file as the first commit on the branch**

```bash
git add docs/superpowers/plans/2026-05-03-bundle-test-coverage-46-47-48.md
git commit -m "docs: implementation plan for test-coverage bundle (#46 #47 #48)"
```

- [ ] **Baseline full cargo gambit**

```bash
make lint
```

Expected: every step exits 0; pre-existing `cargo deny` "unmatched skip" warnings are non-blocking. Note the test count (currently 68) for sanity check at the end.

---

## Task 1: #46 — Unit tests for `relative_time`

`relative_time` takes a `SystemTime` and returns `String` ("now" / "Nm" / "Nh" / "Nd"). The branches depend on `timestamp.elapsed()`. Testing against a wall-clock reading is flake-prone (the `.elapsed()` call itself adds nanoseconds), so we extract a pure `relative_time_from_elapsed(Duration)` helper and test that. The original `relative_time(SystemTime)` becomes a one-line wrapper that's still used by `build_row`.

**Files:**
- Modify: `src/ui/notification_row.rs` — extract pure helper, change wrapper, add `#[cfg(test)]` module.

- [ ] **Step 1: Locate the current `relative_time` implementation**

```bash
grep -n "fn relative_time\|fn build_row" src/ui/notification_row.rs
```

Expected: two matches — `relative_time` defined near the bottom of the file, and `build_row` (the call site that invokes `relative_time(notif.timestamp)` for the time label) defined as the file's primary `pub(crate)` entry point at the top.

- [ ] **Step 2: Extract the pure helper and rewrite the wrapper**

In `src/ui/notification_row.rs`, find:

```rust
fn relative_time(timestamp: SystemTime) -> String {
    let elapsed = timestamp.elapsed().unwrap_or_default();
    let secs = elapsed.as_secs();
    if secs < 60 {
        "now".into()
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}
```

Replace with:

```rust
/// Boundaries for the `relative_time_from_elapsed` thresholds. Named
/// rather than inline so the branch intent is explicit and the
/// constants can be referenced from the unit tests.
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 60 * SECONDS_PER_MINUTE;
const SECONDS_PER_DAY: u64 = 24 * SECONDS_PER_HOUR;

/// Pure helper: formats an elapsed `Duration` as the relative-time
/// string shown in the panel ("now" / "Nm" / "Nh" / "Nd"). Split out
/// so tests can pass exact `Duration` values rather than fight the
/// wall-clock via `SystemTime::now()`.
fn relative_time_from_elapsed(elapsed: std::time::Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < SECONDS_PER_MINUTE {
        "now".into()
    } else if secs < SECONDS_PER_HOUR {
        format!("{}m", secs / SECONDS_PER_MINUTE)
    } else if secs < SECONDS_PER_DAY {
        format!("{}h", secs / SECONDS_PER_HOUR)
    } else {
        format!("{}d", secs / SECONDS_PER_DAY)
    }
}

fn relative_time(timestamp: SystemTime) -> String {
    relative_time_from_elapsed(timestamp.elapsed().unwrap_or_default())
}
```

- [ ] **Step 3: Add the `#[cfg(test)]` test module at the bottom of the file**

`notification_row.rs` currently has no test module. Append at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn now_branch_under_a_minute() {
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(0)), "now");
        assert_eq!(relative_time_from_elapsed(Duration::from_secs(1)), "now");
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(SECONDS_PER_MINUTE - 1)),
            "now"
        );
    }

    #[test]
    fn minutes_branch_under_an_hour() {
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(SECONDS_PER_MINUTE)),
            "1m"
        );
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(2 * SECONDS_PER_MINUTE + 30)),
            "2m"
        );
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(SECONDS_PER_HOUR - 1)),
            "59m"
        );
    }

    #[test]
    fn hours_branch_under_a_day() {
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(SECONDS_PER_HOUR)),
            "1h"
        );
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(2 * SECONDS_PER_HOUR)),
            "2h"
        );
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(SECONDS_PER_DAY - 1)),
            "23h"
        );
    }

    #[test]
    fn days_branch_at_or_above_a_day() {
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(SECONDS_PER_DAY)),
            "1d"
        );
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(2 * SECONDS_PER_DAY)),
            "2d"
        );
        // Arbitrary large value — confirms no overflow surprise.
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(365 * SECONDS_PER_DAY)),
            "365d"
        );
    }

    #[test]
    fn boundary_at_one_minute_transitions_now_to_1m() {
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(SECONDS_PER_MINUTE - 1)),
            "now"
        );
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(SECONDS_PER_MINUTE)),
            "1m"
        );
    }

    #[test]
    fn boundary_at_one_hour_transitions_minutes_to_1h() {
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(SECONDS_PER_HOUR - 1)),
            "59m"
        );
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(SECONDS_PER_HOUR)),
            "1h"
        );
    }

    #[test]
    fn boundary_at_one_day_transitions_hours_to_1d() {
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(SECONDS_PER_DAY - 1)),
            "23h"
        );
        assert_eq!(
            relative_time_from_elapsed(Duration::from_secs(SECONDS_PER_DAY)),
            "1d"
        );
    }
}
```

- [ ] **Step 4: Build, test, clippy, fmt**

```bash
cargo test ui::notification_row::tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: 7 new tests pass; full suite count goes from 68 → 75; clippy clean; no formatting drift.

- [ ] **Step 5: Commit**

```bash
git add src/ui/notification_row.rs
git commit -m "$(cat <<'EOF'
Add unit tests for relative_time + extract pure Duration helper (#46)

relative_time was a pure-ish function but its only input was
SystemTime, making tests racy: SystemTime::now() - Duration::from_secs(N)
plus a subsequent .elapsed() call gives slightly more than N seconds
because the elapsed() read happens after the construction.

Extract relative_time_from_elapsed(Duration) -> String as the pure
inner function; relative_time(SystemTime) becomes a one-line wrapper
that converts via .elapsed().unwrap_or_default(). build_row's call
site is unchanged.

Add 7 unit tests in a new #[cfg(test)] module at the bottom of
notification_row.rs:
- 4 branch tests (now / Nm / Nh / Nd) covering each range
- 3 boundary tests verifying 60s, 3600s, and 86400s transition
  correctly between adjacent branches

Test count: 68 -> 75.

Closes #46.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: #47 — `WaybarStatus` branch + pluralization tests

`update_status` builds a `WaybarStatus`, serializes it to JSON, writes the file, then signals waybar. The build phase is pure but currently inlined inside the side-effect path. Extract `build_status(unread, dnd) -> WaybarStatus` so we can test the four output shapes (dnd, unread==1, unread>1, empty) without disk-I/O fakes.

The pluralization branch (`unread == 1` → "1 unread notification" vs `unread > 1` → "N unread notifications") is the off-by-one risk the issue calls out.

**Files:**
- Modify: `src/waybar.rs` — extract `build_status`, refactor `update_status` to call it, add 4 new tests in the existing `#[cfg(test)] mod tests` block.

- [ ] **Step 1: Extract `build_status` from `update_status`**

In `src/waybar.rs`, find the body of `update_status`:

```rust
pub(crate) fn update_status(unread: usize, dnd: bool) {
    let status = if dnd {
        WaybarStatus {
            text: ICON_BELL_OFF.into(),
            tooltip: "Do Not Disturb".into(),
            alt: "dnd".into(),
            class: "dnd".into(),
            count: unread,
        }
    } else if unread > 0 {
        WaybarStatus {
            text: format!("{ICON_BELL_BADGE} {unread}"),
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
            text: ICON_BELL_OUTLINE.into(),
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

Replace with:

```rust
/// Pure helper: builds the waybar status payload for the current
/// daemon state. Split out from `update_status` so the four-way
/// shape can be tested without going through disk I/O.
fn build_status(unread: usize, dnd: bool) -> WaybarStatus {
    if dnd {
        WaybarStatus {
            text: ICON_BELL_OFF.into(),
            tooltip: "Do Not Disturb".into(),
            alt: "dnd".into(),
            class: "dnd".into(),
            count: unread,
        }
    } else if unread > 0 {
        WaybarStatus {
            text: format!("{ICON_BELL_BADGE} {unread}"),
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
            text: ICON_BELL_OUTLINE.into(),
            tooltip: "No notifications".into(),
            alt: "empty".into(),
            class: "empty".into(),
            count: 0,
        }
    }
}

pub(crate) fn update_status(unread: usize, dnd: bool) {
    let status = build_status(unread, dnd);

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

- [ ] **Step 2: Add the four branch tests**

In `src/waybar.rs`, find the existing `#[cfg(test)] mod tests` block (it currently contains `status_json_includes_count_field` and `waybar_refresh_signal_is_sigrtmin_plus_offset`). Append these four tests inside it:

```rust
    #[test]
    fn build_status_dnd_branch_uses_bell_off_glyph() {
        // DND wins over unread count: even with 5 unread notifications,
        // the dnd flag forces the dnd glyph + class. The count field
        // is preserved in the JSON so consumers can still surface the
        // backlog count next to the bell-off glyph if they want.
        let s = build_status(5, true);
        assert_eq!(s.text, ICON_BELL_OFF);
        assert_eq!(s.tooltip, "Do Not Disturb");
        assert_eq!(s.alt, "dnd");
        assert_eq!(s.class, "dnd");
        assert_eq!(s.count, 5);
    }

    #[test]
    fn build_status_singular_unread_uses_singular_tooltip() {
        let s = build_status(1, false);
        assert_eq!(s.text, format!("{ICON_BELL_BADGE} 1"));
        assert_eq!(s.tooltip, "1 unread notification");
        assert_eq!(s.alt, "unread");
        assert_eq!(s.class, "unread");
        assert_eq!(s.count, 1);
    }

    #[test]
    fn build_status_plural_unread_uses_plural_tooltip() {
        let s = build_status(5, false);
        assert_eq!(s.text, format!("{ICON_BELL_BADGE} 5"));
        assert_eq!(s.tooltip, "5 unread notifications");
        assert_eq!(s.alt, "unread");
        assert_eq!(s.class, "unread");
        assert_eq!(s.count, 5);
    }

    #[test]
    fn build_status_empty_branch_uses_bell_outline() {
        let s = build_status(0, false);
        assert_eq!(s.text, ICON_BELL_OUTLINE);
        assert_eq!(s.tooltip, "No notifications");
        assert_eq!(s.alt, "empty");
        assert_eq!(s.class, "empty");
        assert_eq!(s.count, 0);
    }
```

- [ ] **Step 3: Build, test, clippy, fmt**

```bash
cargo test waybar::tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: 4 new tests pass + 2 existing waybar tests still pass; full suite count goes from 75 → 79; clippy clean; no formatting drift.

- [ ] **Step 4: Commit**

```bash
git add src/waybar.rs
git commit -m "$(cat <<'EOF'
Extract build_status() + add WaybarStatus branch tests (#47)

update_status mixed the (pure) status-building logic with the
(side-effecting) disk write + signal call, so the only existing
test (status_json_includes_count_field) had to construct a
WaybarStatus inline. The pluralization branch (unread == 1 vs > 1)
was completely uncovered.

Extract build_status(unread, dnd) -> WaybarStatus as the pure
helper; update_status now composes build_status + the side
effects. No behavioral change.

Add four tests covering every branch:
- DND wins over unread count (bell-off glyph, dnd class, count
  still set so consumers can surface it)
- unread == 1 produces the singular "1 unread notification" tooltip
- unread > 1 produces the plural "N unread notifications" tooltip
- unread == 0 (no DND) produces the empty/bell-outline branch

Test count: 75 -> 79.

Closes #47.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: #48 — `clean_markup` edge case coverage

`clean_markup` is already pure. Just add tests + a doc comment that records the spec policy.

The existing implementation:

```rust
pub(crate) fn clean_markup(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
}
```

Two known edge-case behaviors to document and test:

1. **Unmatched `<`**: An unclosed `<` flips `in_tag` to true and never resets — everything after is dropped. This isn't ideal, but it matches the spec ("body should be valid markup") and any other behavior would require lookahead or a real parser. Keep as-is, document, and test.
2. **Numeric entities**: The freedesktop notification spec lists only the five named entities (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`). We additionally support `&#39;` for legacy convenience. Other numeric entities (`&#34;`, `&#x27;`, etc.) are passed through verbatim. Keep as-is, document, and test.

Nested / interleaved tags work correctly already (the `in_tag` boolean handles them); add a test confirming.

**Files:**
- Modify: `src/notification.rs` — add a doc comment on `clean_markup` documenting the unmatched-`<` and numeric-entity policy; append four new tests inside the existing `#[cfg(test)] mod tests` block.

- [ ] **Step 1: Document the policy on `clean_markup`**

In `src/notification.rs`, find:

```rust
pub(crate) fn clean_markup(text: &str) -> String {
```

Replace with:

```rust
/// Strips HTML-ish tags and decodes entities in notification body
/// text per the [freedesktop notification spec][spec].
///
/// **Tag stripping:** uses a simple `in_tag` boolean state machine,
/// so nested and interleaved tags (`<b><i>foo</i></b>`,
/// `<b><i>foo</b></i>`) work correctly.
///
/// **Unmatched `<`:** an unclosed `<` flips the state machine to
/// `in_tag` and never resets, so everything from the unmatched `<`
/// to end-of-string is dropped. This matches the spec's expectation
/// that body markup is valid; any other behavior would require
/// lookahead or a real parser, which the daemon doesn't carry.
///
/// **Entities:** decodes the five spec-listed named entities
/// (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`) plus `&#39;` for
/// legacy convenience. Other numeric entities (`&#34;`, `&#x27;`,
/// etc.) are not in the spec and pass through verbatim.
///
/// [spec]: https://specifications.freedesktop.org/notification-spec/latest/
pub(crate) fn clean_markup(text: &str) -> String {
```

(Just the docstring is added — the function body is unchanged.)

- [ ] **Step 2: Append the four edge-case tests**

In `src/notification.rs`, find the existing `#[cfg(test)] mod tests` block. After the existing `clean_markup_combined` test, append:

```rust
    #[test]
    fn clean_markup_handles_nested_tags() {
        // Properly nested
        assert_eq!(clean_markup("<b><i>foo</i></b>"), "foo");
        // Inner content with siblings
        assert_eq!(clean_markup("<b>x<i>y</i>z</b>"), "xyz");
    }

    #[test]
    fn clean_markup_handles_interleaved_tags() {
        // Malformed but recoverable: in_tag is just a boolean,
        // so the order of close-tags doesn't matter.
        assert_eq!(clean_markup("<b><i>foo</b></i>"), "foo");
        assert_eq!(clean_markup("<a><b>x</a></b>tail"), "xtail");
    }

    #[test]
    fn clean_markup_unmatched_lt_swallows_to_end() {
        // Documented behavior: an unclosed `<` flips in_tag and
        // never resets, so everything after the `<` is dropped.
        // Spec says body markup must be valid; this is the
        // simplest deterministic fallback for invalid input.
        assert_eq!(clean_markup("hello < world"), "hello ");
        // Even if a `>` appears later inside what was meant to be
        // a comparison, it gets consumed as a tag-close.
        assert_eq!(clean_markup("a < b > c"), "a  c");
    }

    #[test]
    fn clean_markup_passes_unsupported_numeric_entities_through() {
        // Spec lists the five named entities. We additionally
        // decode &#39; for legacy convenience but no other numeric
        // form. Confirm that &#34; (would-be ") and &#x27; (hex
        // single quote) survive untouched so a misbehaving app's
        // text isn't silently mangled.
        assert_eq!(clean_markup("a &#34; b"), "a &#34; b");
        assert_eq!(clean_markup("a &#x27; b"), "a &#x27; b");
        // The spec-listed forms still decode for sanity.
        assert_eq!(clean_markup("a &quot; b"), "a \" b");
        assert_eq!(clean_markup("a &#39; b"), "a ' b");
    }
```

- [ ] **Step 3: Build, test, clippy, fmt**

```bash
cargo test notification::tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: 4 new tests pass; pre-existing 6 notification tests still pass; full suite count goes from 79 → 83; clippy clean; no formatting drift.

- [ ] **Step 4: Commit**

```bash
git add src/notification.rs
git commit -m "$(cat <<'EOF'
Add edge-case tests + policy docs for clean_markup (#48)

Existing tests covered the happy path (single tag, listed entities,
combined). Three edge classes were undocumented and uncovered:

1. Nested / interleaved tags. Already handled correctly by the
   in_tag boolean state machine — added two tests confirming.
2. Unmatched `<`. The unclosed `<` flips in_tag and never resets,
   so everything to end-of-string is dropped. This is the simplest
   deterministic fallback when an app sends invalid markup (the
   freedesktop spec says body markup must be valid). Documented
   on the function + added two tests covering the behavior.
3. Unsupported numeric entities. The spec lists five named
   entities; we additionally decode &#39; for legacy convenience.
   Other numeric entities (&#34;, &#x27;, etc.) are not in the
   spec and now have a test asserting they pass through verbatim
   so we don't accidentally start mangling apps' text.

The function gets a docstring covering all three policy choices
so the next reviewer doesn't have to re-derive them.

No code change to clean_markup itself — pure docs + tests.

Test count: 79 -> 83.

Closes #48.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Pre-PR gates

Per repo convention: smoke install for the user check, then full `make lint`, then push. This bundle is purely additive tests + two small extract-to-pure-helper refactors with no behavioral change, so the smoke is mostly "did anything break end-to-end?"

- [ ] **Step 1: Install to user bin and confirm restart works**

```bash
make upgrade PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
```

Expected: build → install → kill running daemon → respawn. `pidof nwg-notifications` returns the new PID.

- [ ] **Step 2: Hand off to the user — STOP HERE**

Tell the user (verbatim or close):

> Installed and restarted via `make upgrade`. This bundle is
> purely additive tests + two pure-helper extractions with no
> behavioral change, so the smoke is short:
>
> 1. `notify-send "smoke" "test"` — popup still appears.
> 2. Open the panel — entry shows up with the expected
>    "now" / "Nm" timestamp (verifies the relative_time
>    refactor didn't break the call site).
> 3. Toggle DND via right-click on the bell — bell-off glyph
>    still renders (verifies the build_status extraction
>    didn't break update_status).
>
> Reply when satisfied or with anything that needs fixing.

**Do not proceed to Task 5 until the user explicitly approves.** If they report issues, return to the broken task.

- [ ] **Step 3: Full lint after smoke approval**

```bash
make lint
```

Expected: every step exits 0; pre-existing `cargo deny` "unmatched skip" warnings are unchanged; total test count is 83.

- [ ] **Step 4: Push**

```bash
git push -u origin chore/test-coverage-46-47-48
```

---

## Task 5: Open PR

- [ ] **Step 1: Open the PR**

```bash
gh pr create --base main --head chore/test-coverage-46-47-48 \
  --title "Bundle test coverage: #46 #47 #48" \
  --body "$(cat <<'EOF'
## Summary

Bundles three test-coverage items from epic #29 into one PR:

- **#46** — Extracted `relative_time_from_elapsed(Duration) -> String` as a pure helper inside `src/ui/notification_row.rs`. The original `relative_time(SystemTime)` is now a one-line wrapper. Added 7 unit tests in a new `#[cfg(test)]` module: 4 branch tests (now / Nm / Nh / Nd) + 3 boundary tests at 60s, 3600s, 86400s.
- **#47** — Extracted `build_status(unread, dnd) -> WaybarStatus` as a pure helper inside `src/waybar.rs`. `update_status` now composes `build_status` + the disk-write/signal side effects. Added 4 branch tests covering DND, singular pluralization (`unread == 1`), plural (`unread > 1`), and empty.
- **#48** — Added 4 edge-case tests for `clean_markup` in `src/notification.rs` (nested tags, interleaved/malformed nesting, unmatched `<`, unsupported numeric entities) plus a policy docstring on the function recording the choices.

One commit per issue. No CHANGELOG entry — this bundle is internal coverage with zero user-visible impact.

## Test plan

- [x] `make lint` clean locally (fmt + clippy + test + deny + audit).
- [x] Test count: 68 → 83 (+15 new tests across the three issues).
- [x] Manual smoke test against the live compositor (installed via `make upgrade PREFIX=\$HOME/.local BINDIR=\$HOME/.cargo/bin`):
  - [x] `notify-send` still produces a popup.
  - [x] Panel timestamps render ("now" / "Nm" / etc.) — confirms `relative_time` refactor.
  - [x] DND toggle still works — confirms `build_status` extraction.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 2: Wait for CodeRabbit + iterate**

Default-fix posture per repo convention: address every CodeRabbit finding in-PR. Inline reply per in-diff comment, single PR-level reply for outside-diff items, tag `@coderabbitai` every time. Do not respond to non-bot commenters under the maintainer's account.

---

## Notes

- **No CHANGELOG entry.** Per the resolution from PR #54, internal-only changes (refactors, tests, docs without user-visible impact) don't pollute the Keep-a-Changelog log. The PR description, closed-issue links, and per-commit messages are the audit trail.
- **No new behavior.** The two refactors (#46, #47) are pure-helper extractions wrapped by the original public surface — call sites are unchanged.
- **Test naming.** Each new test is named for the property it asserts, not for the issue number, so the test list reads as a behavioral spec when someone runs `cargo test --list`.
