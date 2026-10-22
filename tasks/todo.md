# Manual time entries: Task breakdown

Status: Plan and tasks approved. Tasks 1–4 implemented; Task 5 remains open for native Tauri/macOS verification. Available checks and limitations are recorded in [plan.md](plan.md). Scope: [approved spec](../docs/SPEC-manual-entries.md).

## Task 1: Create manual entries through the typed API

- [x] Complete task 1 implementation and core-module checks; native IPC verification remains in Task 5.

**Description:** Add manual provenance, validated creation, and conflict acknowledgement through the existing Rust/Tauri/TypeScript layers.

**Acceptance:**
- [x] Fresh and upgraded databases distinguish manual entries; automatic rows and tracker inserts are unchanged.
- [x] Invalid apps/times and future intervals write nothing; unacknowledged overlaps return conflicts across stored, ignored, cross-day, and open activity; confirmed overlaps retain both durations.
- [x] Creation preserves the open session, paused state, and lock order; command registration, result types, and client arguments agree by inspection and typechecking. Native IPC remains unverified.

**Verify:** Add failing Rust tests first using in-memory databases and controlled clocks; run `RUSTUP_TOOLCHAIN=1.97.0 cargo test --manifest-path src-tauri/Cargo.toml manual`, `bun run test api`, and `bun run build`. Check adjacent intervals and automatic-session preservation explicitly.

**Dependencies:** None. **Scope:** Medium, five files.

**Files:** `src-tauri/src/db.rs`, `src-tauri/src/tracker.rs`, `src-tauri/src/lib.rs`, `src/api.ts`, `src/api.test.ts`.

## Task 2: Keep manual activity correct in local-day views

- [x] Complete task 2.

**Description:** Integrate manual intervals through the existing aggregation and cleanup path, correcting the shared local-day calculations required for daylight-saving dates.

**Acceptance:**
- [x] A manual interval contributes exactly once to day/app/hour totals and applicable project receipts; existing assignments, rules, exceptions, and ignores apply normally.
- [x] Day bounds follow the local calendar; missing/repeated clock hours have correct buckets; automatic cross-midnight attribution is preserved.
- [x] Clear/reset, untagged retention, and reopening the database handle manual entries consistently; regression tests preserve old automatic records.

**Verify:** Run the focused manual tests, then run the Rust suite in fresh processes using `TZ=UTC RUSTUP_TOOLCHAIN=1.97.0 cargo test --manifest-path src-tauri/Cargo.toml` and `TZ=Pacific/Auckland RUSTUP_TOOLCHAIN=1.97.0 cargo test --manifest-path src-tauri/Cargo.toml`. Transition-specific tests must assert that the intended timezone/offset was actually used. Run `bun run test merge period` to verify unchanged period wire contracts.

**Dependencies:** Task 1. **Scope:** Small, one file; tests live in its existing test module.

**Files:** `src-tauri/src/db.rs`.

## Checkpoint: Backend and totals

- [x] Tasks 1–2 implementation and core checks meet their acceptance criteria; migration and tracking regression tests pass.
- [x] Direct calls to the backend operation reject future entries, and unacknowledged overlap writes nothing; native command dispatch remains in Task 5.
- [x] Day totals, project receipts, and chart buckets agree for ordinary and transition dates.

## Task 3: Add manual entries from Activities

- [x] Complete task 3.

**Description:** Add the day-view form and explicit overlap confirmation using the typed API.

**Acceptance:**
- [x] Add works on an empty day, offers previously tracked apps and local date/start/stop fields, previews duration, and makes the saved day's totals visible immediately.
- [x] Warnings list conflicts and explain double-counting; Go back, Save anyway, edits, cancellation, failure/retry, and pending submission follow the approved flow.
- [x] Labeled fields, focus, keyboard access, accessible errors, async navigation, narrow layouts, and both themes work without altering week/month controls.

**Verify:** Add React Testing Library tests for success and failure flows; run `bun run test ManualEntries DayPane` and `bun run build`. Exercise date changes and a pending request while navigating away. Verify keyboard focus and visual layout in a real runtime when available.

**Dependencies:** Tasks 1–2. **Scope:** Medium, five files.

**Files:** `src/components/ManualEntries.tsx`, `src/components/ManualEntries.test.tsx`, `src/components/DayPane.tsx`, `src/components/DayPane.test.tsx`, `src/App.css`.

## Task 4: Remove mistaken manual entries

- [x] Complete task 4.

**Description:** Complete the persisted day list and manual-only deletion path, so a user can delete and re-add an incorrect entry.

**Acceptance:**
- [x] The day list shows identifiable entries with app, date, start/stop, duration, and a delete action, including after reopening the database and after an app is ignored. Native app restart remains in Task 5.
- [x] Deleting removes only the selected manual row and refreshes totals; automatic ids are refused, missing ids are harmless, and project decisions remain intact.
- [x] Loading/deletion failures remain visible and retryable; pending actions cannot trigger repeated requests, and the UI uses the new registered typed commands.

**Verify:** Extend Rust and component tests; run `RUSTUP_TOOLCHAIN=1.97.0 cargo test --manifest-path src-tauri/Cargo.toml manual`, `bun run test ManualEntries DayPane`, and `bun run build`. Test an automatic id, an already-deleted id, failure/retry, and cleanup of the final entry on a day.

**Dependencies:** Tasks 1 and 3. **Scope:** Medium, five files.

**Files:** `src-tauri/src/db.rs`, `src-tauri/src/lib.rs`, `src/api.ts`, `src/components/ManualEntries.tsx`, `src/components/ManualEntries.test.tsx`.

## Checkpoint: Complete user flow

- [x] Tasks 3–4 pass; add, warn, go back, confirm, list, and delete work together in component tests and the browser fixture.
- [x] Day totals refresh immediately; project/period aggregation includes manual activity.
- [ ] Verify the paused/active native tray total and project navigation through real IPC on macOS.
- [x] Frontend build and focused regression tests pass without lowered coverage or suppressed checks.

## Task 5: Document and verify the finished feature

- [ ] Complete task 5.

**Description:** Document the shipped behavior and verify the full approved spec, including native tracking behavior.

**Acceptance:**
- [x] README and domain vocabulary explain manual entries, past-only timing, confirmed overlaps, delete/re-add corrections, and existing cleanup behavior.
- [ ] All automated quality gates pass with existing thresholds intact; review the diff for unintended changes to monitoring, assignment resolution, and deletion.
- [ ] macOS acceptance checks cover paused/active add, open-session warning, restart persistence, deletion, charts/projects/tray, keyboard access, themes, and narrow layouts; record any unavailable checks explicitly without claiming completion.

**Verify:** Run `bun run test:coverage`, `bun run build`, `RUSTUP_TOOLCHAIN=1.97.0 cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`, `RUSTUP_TOOLCHAIN=1.97.0 cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`, and `RUSTUP_TOOLCHAIN=1.97.0 cargo test --manifest-path src-tauri/Cargo.toml`. Launch `bun run tauri dev` on macOS and work through the spec's success-criteria table.

**Dependencies:** Tasks 2–4. **Scope:** Small, two product documentation files, plus progress updates to these planning artifacts.

**Files:** `README.md`, `CONTEXT.md`; update `docs/SPEC-manual-entries.md`, `tasks/plan.md`, and `tasks/todo.md` with final status and verification results.

## Checkpoint: Complete

- [ ] All spec success criteria are met, including documented runtime verification.
- [ ] Automated checks and code review are complete; material limits are reported.
- [x] No changes have been merged or deployed.
