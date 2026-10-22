# Implementation plan: Manual time entries

Status: Plan and task breakdown approved by the user. Tasks 1–4 are implemented; documentation and available verification are complete. Task 5 remains open for the native Tauri build and macOS acceptance checks.

Source of truth: [approved spec](../docs/SPEC-manual-entries.md). Task checklist: [todo.md](todo.md).

## Approach

Store manual entries alongside monitored sessions, with one additive `is_manual` flag. Reuse the existing aggregation, app identity, project resolver, ignore rules, cleanup, and refresh callbacks. Add three Tauri commands and one day-view component. Keep the existing day/week/month wire shapes.

## Storage and compatibility

- Add `is_manual INTEGER NOT NULL DEFAULT 0` to `segments`, following the existing idempotent column-migration pattern. Old rows and existing tracker inserts remain automatic; manual inserts explicitly set the flag to 1.
- Use the existing row id, app metadata, timestamps, and untitled representation. Do not create a second activity table or aggregation path. Automatic session rows remain immutable; only explicitly manual rows gain individual deletion.
- List manual entries for a validated local day, ordered by start timestamp and id. Include entries even if later ignored, so they remain removable; normal totals continue to respect ignores.
- Delete with both the id and manual flag in the predicate. Refuse deletion of an automatic id; a missing id is a no-op. Leave assignments, rules, exceptions, and notes alone.
- Existing clear/reset and resolver-based untagged deletion already operate on `segments`; verify their behavior with manual records rather than adding separate cleanup logic.
- Update the segment vocabulary in `CONTEXT.md` to distinguish immutable monitored sessions from user-entered intervals. The migration is additive; reverting application code must not drop the new column or activity.

## Command contract

Use `#[tauri::command(async)]`, register every command in `generate_handler!`, and expose it only through `src/api.ts`. Keep the existing command argument conventions and `Result<_, String>` errors for invalid input or failures.

| Command / frontend method | Input | Successful response |
|---|---|---|
| `add_manual_entry` / `addManualEntry` | App key, local date, start/end `HH:mm` strings, `allowOverlap` boolean | Tagged result: `saved` with the entry, or `overlap` with conflicts and no write |
| `list_manual_entries` / `listManualEntries` | Local date | Manual entries for that day |
| `delete_manual_entry` / `deleteManualEntry` | Entry id | No content |

The manual-entry response contains `id`, `app_key`, `app_name`, `bundle_id`, `start_ts`, and `end_ts`. Conflict responses contain app display name and absolute start/end timestamps. Keep Rust and TypeScript types in sync; use a discriminated union for the add result.

Resolve app metadata from `tracked_apps`; do not trust names or bundle ids supplied by the frontend. Parse strict local dates/times in Rust, reject ambiguous/nonexistent local timestamps, and validate positive same-day intervals ending no later than the backend clock. Revalidate on Save anyway.

Creation is not automatically retried or idempotent: another deliberate submission is another entry. A failed request clears overlap acknowledgement while preserving form values, so retrying checks conflicts again. No idempotency-key subsystem is needed for this local form.

## Tracking safety and overlaps

Keep creation coordination in `tracker.rs`, where the state and lock convention already live, with the Tauri wrapper delegating to a testable method. Acquire `current` and then `db`; hold both through validation, conflict checking, and insertion. Take the current timestamp after acquiring the locks. Do not flush or mutate the open session or paused state.

Query every stored interval with `existing.start < proposed.end` and `existing.end > proposed.start`, without filtering by start date, app, project, or ignore status. Include only positive-duration activity. Add any intersecting open session through the current timestamp. Without `allowOverlap`, return conflicts before writing; with explicit acknowledgement, insert the full interval once. Use the existing shared connection mutex to serialize this operation with tracker and cleanup writes; do not add another lock.

## Local-day totals

`day_view`, `day_total_seconds`, and `project_day_entries` currently use midnight plus 86,400 seconds. That can omit the last hour of a long daylight-saving day or include the following day on a short one. Use the next local calendar day's boundary in these queries and in manual listing.

Keep the existing 24 chart buckets, assigning elapsed seconds by local clock hour: the missing hour is empty, and both occurrences of a repeated hour contribute to the same bucket. Preserve the existing rule that an automatic session spanning midnight is attributed to its start date, with overflow assigned to its final chart bucket. Limit the change to these shared day calculations; no timezone settings or calendar redesign.

Use fresh Rust test processes with a controlled `TZ` for transition fixtures rather than changing the timezone environment of a running parallel test suite.

## Day-view UI

- Mount `ManualEntries.tsx` in `DayPane` only for day granularity. It owns the add form, warning, and persisted manual-entry list. Reuse existing controls, icons, CSS variables, app candidates, and `fmtDur`; no new dependency or global state.
- Default the form date to the selected day. Use labeled native date/time fields, minute precision, and a duration preview based on elapsed timestamps. Rust remains authoritative when local-time interpretation is invalid or ambiguous.
- On save, navigate to the entry's date if the form date changed, reload the day, and use the existing change callback. On deletion, reload the current day. Project panes load current data when opened; the existing poller updates the tray even while paused.
- Conflicts appear as a list of apps and date/time ranges with Go back and Save anyway. Editing a field, canceling, navigating away, or a failed request clears acknowledgement. Keep form values on validation, save, and loading failures; show retryable errors.
- Disable duplicate submissions/deletions while pending. Guard async loads when changing day or unmounting. Provide keyboard access, focus handling, accessible errors/warnings, and styles for both themes and narrow windows.

## Dependency order and checkpoints

| Task | Deliverable | Depends on |
|---|---|---|
| 1 | Guarded creation through the typed API | — |
| 2 | Correct local-day aggregation of manual activity | 1 |
| 3 | Add entries from the day view, including overlap confirmation | 1, 2 |
| 4 | List and delete individual manual entries | 1, 3 |
| 5 | Documentation and full acceptance verification | 2, 3, 4 |

After tasks 1–2, verify migration, tracking invariants, project/cleanup integration, and timezone cases. After tasks 3–4, verify add/warn/confirm/list/delete end to end and run the frontend build. Task 5 runs the full quality gates and native acceptance checks from the spec. Detailed criteria and executable commands are in `todo.md`.

Work will run sequentially: the tasks share the DB, API, and component. Once contracts are fixed, documentation and frontend/backend tests could be prepared independently, but extra agents are not required.

## Risks and verification limits

| Risk | Mitigation |
|---|---|
| Adding time while a monitoring session is open | Test creation through the tracking coordinator, preserve lock order, and assert unchanged open/paused state |
| Warning acknowledged for stale form values or retried after a failed response | Reset acknowledgement on every edit/failure; revalidate in Rust; never retry a write automatically |
| Deletion reaches monitored activity | Restrict the backend delete query by manual origin and test an automatic id |
| Daylight-saving timestamps produce wrong dates or buckets | Reject invalid/ambiguous endpoints and test actual local-day bounds and hour buckets in controlled timezones |
| Old databases or untagged cleanup treat manual records differently | Migration fixtures plus existing resolver/cleanup integration tests |
| Confirmed overlaps inflate totals | Explain this before confirmation and retain/count both intervals as the approved spec requires |

Bun is available. Rust 1.97.0, rustfmt, and Clippy were installed under `/tmp/echo-cargo` and `/tmp/echo-rustup`, without changing shell configuration or project dependencies. This host is Linux. Native macOS tracking, tray, and restart checks remain completion requirements.

The supplied repository instructions reference `RTK.md`; that file was not found in the working tree, ancestors checked, or source checkout. No extra rules were inferred from it.

## Open questions

None. No additional feature decisions or approvals are needed for the accepted implementation.

## Implementation and verification results

- Reused `segments` with the additive manual flag and the existing resolver/cleanup path. Added guarded creation, day listing, manual-only deletion, typed commands, and the day-view component. No project dependencies or CI settings changed.
- Corrected shared local-day bounds and hour buckets. Monitored sessions retain start-date attribution; manual times are validated at the Rust boundary, including daylight-saving ambiguity.
- `bun run test:coverage`: **87 tests passed**. Coverage: statements 63.59%, branches 60.89%, functions 54.98%, lines 64.27%; existing thresholds unchanged. The installed Vitest/coverage plugin version mismatch warning predates this change and did not prevent the run.
- `bun run build`: **passed**.
- `CARGO_HOME=/tmp/echo-cargo RUSTUP_HOME=/tmp/echo-rustup /tmp/echo-cargo/bin/cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`: **passed**.
- A temporary crate at `/tmp/echo-core-tests/Cargo.toml` imports the actual `db.rs` and `tracker.rs` source files and runs their existing test modules. With `CARGO_HOME=/tmp/echo-cargo RUSTUP_HOME=/tmp/echo-rustup CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`, `/tmp/echo-cargo/bin/cargo test --manifest-path /tmp/echo-core-tests/Cargo.toml --quiet` passed **86 tests** under both `TZ=UTC` and `TZ=Pacific/Auckland`. This does not compile the native Tauri command wrappers, poller, tray, or updater.
- With the same temporary Cargo/Rustup homes, `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 /tmp/echo-cargo/bin/cargo clippy --manifest-path /tmp/echo-core-tests/Cargo.toml --all-targets -- -D warnings`: **passed**.
- Mutation review: an isolated copy with the overlap acknowledgement guard inverted fails the regression test as expected. Product source was not changed by that check.
- Isolated Chromium loaded the actual `DayPane` and manual-entry components with an in-memory API fixture. Warning/back/confirm/list/delete, totals refresh, warning and return focus, light/dark themes, and a 420px-wide viewport passed with no console errors. The form had no horizontal overflow. Screenshots are `/tmp/echo-manual-warning-light.png`, `/tmp/echo-manual-saved-light.png`, and `/tmp/echo-manual-warning-dark-narrow.png`. The fixture and browser tooling are temporary and are not product code or dependencies.
- `git diff --check`: **passed**. Review confirmed manual-only deletion, parameterized queries, backend validation, existing tracking lock order, and no mutation of the open session or paused flag.

**Still pending:** The full native crate test/build attempt (`cargo test --manifest-path src-tauri/Cargo.toml --locked` with the temporary Cargo/Rustup homes) stopped in `glib-sys`: this host has no `pkg-config` command and lacks the configured Linux desktop prerequisites. Native crate Clippy is likewise unverified. The core-module harness is a partial check, not a substitute for these gates. Run the spec's full Rust checks and native macOS acceptance table on a prepared host before considering Task 5 complete. Native pause/resume capture, tray updates, restart persistence through the app, and real IPC remain unverified here.
