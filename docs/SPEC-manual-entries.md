# Spec: Manual time entries

Status: Spec, implementation plan, and task breakdown approved by the user. Implementation is on this feature branch; native Tauri/macOS verification remains pending. See `tasks/plan.md` for verification results.

## Objective

Let an Echo user fill in time missed while tracking was paused. Choose an app, a local calendar day, and start/stop times; Echo records the interval and includes it in the existing activity and project views.

Success means recovering missing time without changing recorded monitoring sessions or scheduling future tracking.

## Approved decisions

Confirmed by the user:

- Earlier today is allowed, provided the stop time has already passed.
- Overlapping recorded or manual time is allowed after a warning and explicit confirmation.

Approved scope defaults:

- One app and one interval per entry, using apps Echo has previously tracked. Reuse the existing app picker candidates, which exclude ignored activity.
- Start and stop must fall on the same local calendar date. Add separate entries for separate days.
- Enter start/stop times to the minute and show the resulting duration; no separate duration input.
- Entries are app-level, with no window title or project field. Existing app assignments and rules classify them; normal project assignment remains available afterward.
- Correct mistakes by deleting a manual entry and adding it again. Editing entries is outside this version.
- Manual entries follow the same clear/reset and untagged cleanup settings as other activity.

## User experience

1. In the Activities day view, choose **Add manual entry**, including on a day with no activity. The selected day pre-fills the date.
2. Select an app and enter the date, start time, and stop time. Show the calculated duration and keep the form editable until saving succeeds.
3. Save validates the interval. Invalid fields receive clear messages; saving is disabled during submission.
4. If the interval overlaps existing activity, show the conflicting app(s) and time range(s). Explain that saving counts both durations and can make totals exceed elapsed time. Offer **Go back** and **Save anyway**. No entry is created before confirmation; changing any field resets that confirmation.
5. On success, refresh the selected day's totals, chart, and app rows immediately. Project views reflect the entry when opened, and today's tray total updates on the normal poll tick, including while paused.
6. A manual entries list in the day view shows each entry's app, date, start/stop, duration, and a delete action. It remains accessible after restart. Deleting refreshes totals and removes only that manual entry.

Provide labeled fields, keyboard access, visible focus, accessible validation and warning messages, and usable layout in the existing light/dark themes. Loading or save failures must be visible and preserve entered values for retry. Canceling creates no data.

## Time and overlap rules

- Validate in Rust as well as the UI: a valid selectable app, valid local date/time values, `start < stop`, the same local date for both endpoints, and `stop <= current backend time` at save. Do not require tracking to be paused.
- Interpret inputs in the computer's local timezone. Persist absolute timestamps and calculate duration from their difference. Reject nonexistent or ambiguous daylight-saving times with an explanatory message rather than silently choosing or shifting a time.
- Intervals are half-open: `[start, stop)`. Ending exactly when another interval starts is not an overlap.
- Detect overlaps across every app, including ignored activity, other manual entries, sessions that started on an earlier day, and the currently open monitoring session through the current time. An overlap warning concerns activity intervals; Echo's existing overlapping project assignments remain valid.
- Check conflicts against current backend state immediately before insertion. With an unacknowledged conflict, return warning information without writing. Save anyway revalidates the same form values before writing.
- Keep overlap checking and insertion consistent with concurrent tracker writes. Preserve the existing `current` then `db` lock order and the open session; adding an entry must not pause, resume, close, or replace monitoring.
- Acknowledged overlaps retain both intervals in full. Do not trim, merge, replace, or deduplicate them. A repeated deliberate submission is subject to the same warning; prevent accidental repeated clicks while a request is pending.

## Persistence and integration

- Persist each entry's stable id, manual origin, app identity/display metadata, and absolute start/stop timestamps. Preserve existing automatic sessions unchanged and classified as automatic when upgrading an existing database.
- Include manual activity exactly once in the existing aggregation and project resolution path. Use the existing app key and untitled representation so app assignments, rules, exceptions, ignore behavior, and day/week/month totals retain their current meaning.
- Manual-entry deletion must verify manual origin in the backend. Passing an automatic session id must never delete monitored activity. Deletion does not alter app assignments, rules, exceptions, notes, or other sessions.
- Clear tracking data and reset remove manual entries too. Untagged clearing and automatic retention use the existing project resolver for manual entries as well. No new retention policy is introduced.
- Keep data local. Reuse the existing stack and app catalog; no dependency or installed-app discovery is required.

## Tech stack

Existing repository dependencies: Tauri 2, React 19.1, TypeScript ~7.0.2, Rust edition 2021, SQLite through rusqlite 0.32, and chrono 0.4. Frontend tests use Vitest 4.1 and React Testing Library; Rust tests use the built-in test framework and in-memory SQLite.

## Commands

Run from the repository root, with Bun and the platform's Tauri prerequisites installed:

```sh
bun install --frozen-lockfile
bun run tauri dev
bun run test
bun run test:coverage
bun run build
RUSTUP_TOOLCHAIN=1.97.0 cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
RUSTUP_TOOLCHAIN=1.97.0 cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
RUSTUP_TOOLCHAIN=1.97.0 cargo test --manifest-path src-tauri/Cargo.toml
```

The Rust toolchain matches CI. There is no frontend linter; `bun run build` typechecks. Native runtime verification targets macOS.

## Project structure and affected surfaces

| Location | Responsibility relevant to this feature |
|---|---|
| `src/components/DayPane.tsx`, `src/App.css` | Add form, conflict warning, manual entry list, refresh, and styling |
| `src/api.ts` | Typed manual-entry commands and shared wire types; sole location for `invoke` |
| `src/App.tsx` | Existing pane refresh callbacks, if needed for updated activity |
| `src-tauri/src/lib.rs` | Async Tauri command wrappers and command registration |
| `src-tauri/src/db.rs` | Persistence, validation, conflict queries, deletion, aggregation, and cleanup |
| `src-tauri/src/tracker.rs`, `poller.rs`, `tray.rs` | Active-session safety, lock order, and today's total; preserve existing lifecycle |
| `src/components/DayPane.test.tsx`, `src/api.test.ts`, Rust test modules | UI/API mocks, in-memory fixtures, and behavior checks |
| `README.md`, `CONTEXT.md`, `docs/` | Feature documentation and manual-entry vocabulary |

This table identifies affected contracts, not a requirement to modify every listed file. Keep component tests beside their source and Rust tests in existing `#[cfg(test)]` modules. No config or export changes are required by the scope.

## Code style

Follow the existing named React components, two-space TypeScript indentation, camelCase frontend names, and snake_case Rust commands. Wire structs use serde and mirror the types in `src/api.ts`. Reuse the current API pattern, for example:

```ts
trackedApps: () => invoke<TrackedApp[]>("tracked_apps"),
```

Use `#[tauri::command(async)]` for new Rust command wrappers, map errors to user-visible results, and keep shared validation and aggregation in the existing backend layers. Apply Rust formatting with rustfmt.

## Testing strategy

- Rust tests with in-memory SQLite prove input validation, half-open overlap boundaries, cross-app/manual/ignored/cross-day conflicts, acknowledgement before insertion, persistence, and deletion restricted to manual data. Test active-session integration and preservation using the existing tracker fixtures.
- Cover empty, equal, reversed, future, invalid-date, same-day, and daylight-saving inputs. Use controlled timestamps and deterministic timezone cases rather than tests dependent on the machine's current date or timezone.
- Assert manual time reaches day totals and hour buckets, week/month rollups, app-level project resolution, exceptions, ignores, and cleanup exactly once. Test migration of an existing database without changing its automatic records.
- Vitest and React Testing Library cover the empty-day add flow, duration display, validation errors, overlap back/confirm paths, confirmation reset after edits, failure/retry, pending submission, listing, and deletion. Update API mocks and fixtures where the wire contract changes.
- Keep existing coverage thresholds at or above statements/lines 40% and branches/functions 35%. Run the frontend build, coverage suite, Rust formatting, Clippy, and Rust tests before completion.
- Manually verify the native macOS app while paused and while tracking: save earlier-today entries, confirm an active-session overlap, reopen the app, delete a manual entry, and check project/chart/tray refresh plus keyboard and theme behavior.

## Boundaries

- **Always:** Validate at the backend boundary, require explicit overlap confirmation, preserve monitored sessions and lock order, persist manual provenance, reuse project resolution, maintain accessibility, and run the applicable checks.
- **Ask first:** Expand to never-tracked apps, custom activity names, title input, inline editing, duration-only input, cross-midnight entries, bulk entry, new dependencies, CI changes, or a retention policy different from existing activity. Review the implementation plan after this spec is approved.
- **Never:** Schedule future entries, overwrite monitored time, silently accept overlap, delete automatic data through a manual-entry action, count a manual entry twice in aggregation, weaken tests/checks, or upload activity data.

## Success criteria

| Scenario | Expected result |
|---|---|
| A missing 09:00–10:30 interval yesterday | Save 90 minutes for the chosen app; show it in the day list, chart, period totals, and applicable project totals |
| Earlier today with stop time already passed | Save succeeds while paused or tracking, without changing tracking state |
| Future date or stop time | Reject without writing, even through direct command invocation |
| Stop equals/before start, crosses midnight, or local time is invalid/ambiguous | Reject with a useful validation message |
| Existing activity ends at 09:00; manual entry starts at 09:00 | Save without an overlap warning |
| Any app, manual entry, or open session overlaps | Warn and write nothing; Go back preserves the form; Save anyway records the full interval after revalidation |
| Monitored 09:00–10:00 plus confirmed manual 09:30–10:30 | Retain both and count 120 minutes; the warning explains the double-counting |
| Restart and return to the day | Manual entries remain identifiable with their individual times and delete actions |
| Delete one manual entry or attempt deletion of an automatic id | Remove only the targeted manual entry; automatic data and assignments remain intact |
| Existing rules, exceptions, ignores, or cleanup apply | Manual activity follows the established behavior without special project or retention semantics |

## Open questions

None. Previously tracked apps only, app-level entries, deletion instead of editing, and existing untagged cleanup behavior were approved with this spec.
