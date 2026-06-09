# Time Tracker — Design

A lightweight macOS app that watches what you work on, auto-sorts it into projects,
and gives you **weekly hours per project**. Personal use. Hours only — no clients,
rates, or invoices.

---

## Core decisions (resolved)

| # | Decision | Choice |
|---|----------|--------|
| 1 | Data source | **Own poller → own SQLite.** Not `knowledgeC.db` (pruned, app-only, reverse-engineered, breaks on macOS updates). |
| 2 | Stack | **Tauri** (Rust shell + TS/web UI). Lightweight vs Electron; keeps logic in TS. |
| 3 | Residency | App runs while open or in tray; **polls only while running**; killing it stops tracking (acceptable). Tray icon = "alive" indicator. No launchd, no auto-relaunch. |
| 4 | Capture | **App + window title** (title optional). App-level needs no permission; title needs Accessibility. Fine to fall back to app-only. |
| 5 | Bucketing | **Immutable segments + rules as a derived layer.** First-match-wins ordered rules. Unassigned triage bucket. |
| 6 | Sampling | **Segments, not point samples.** Poll every ~2s; write a row only when app+title changes. |
| 6b | Idle | **No input-idle detection** (waiting on agents / reading must count). Auto-stop only on **sleep / lid-close / display-off**. |
| 7 | Editing | **Aggregate-level only.** Move groups in/out of projects + manual entries. No per-segment splitting, no free-typed hour overrides. |
| 8 | Scope | **Projects only.** No clients, rates, money, or invoices. |
| 9 | Primary flow | **Auto-sorted weekly view + Unassigned triage that breeds rules.** |
| 10 | Freeze model | **Confirmation watermark** (see below). No retroactive rule rewriting of past weeks. |
| 11 | Note | **One note per week** (global weekly journal). |
| — | Week | **Monday–Sunday, local timezone.** |

---

## Architecture

```
┌─────────────────────────────────────────┐
│ Tauri app (runs while open / in tray)    │
│                                          │
│  Rust side                               │
│   • Poller: every ~2s read frontmost app │
│     (+ window title via AX if available) │
│   • Sleep/lid/display-off listeners      │
│     → close current segment              │
│   • Writes segments to SQLite            │
│                                          │
│  TS/web UI                               │
│   • Weekly view (triage + confirm)       │
│   • Project view (history + drill-in)    │
│   • Rules editor, notes, manual entries  │
│                                          │
│  SQLite (owned by us)  ◄── single source │
└─────────────────────────────────────────┘
```

**Poller logic (per ~2s tick):**
1. Read frontmost app (bundle id + name); read window title via Accessibility if granted, else null.
2. If `(app, title)` unchanged → do nothing.
3. If changed → close current in-memory segment (`end = now`), write it, open a new one.
4. On `willSleep` / display-off / lid-close → close current segment (`end = event time`); on wake → start fresh. (Prevents phantom overnight blocks.)

**Technical risk:** reading window title via the Accessibility API from Rust (AX crates are
rougher than native AppKit). Mitigated by titles being optional — app-level works with no
permission and is acceptable as the fallback.

---

## Data model

```
segments          immutable, append-only ground truth
  id, start, end, app_bundle_id, app_name, window_title (nullable)

projects
  id, name, archived

rules             ordered; first match wins
  id, order, match_app (nullable), match_title_substring_or_regex (nullable), project_id

week_assignments  materialized on "Save/Confirm"; frozen
  id, iso_week, group_key (app or app+title), project_id, confirmed_at
  -- holds the in/out tweaks; later rule edits don't touch confirmed rows

week_notes
  iso_week, text

manual_entries    basic off-computer additions
  id, iso_week, name, hours
```

Derived view = run current rules over `segments` for unconfirmed groups; use
`week_assignments` for confirmed groups. Totals = confirmed tracked time + manual entries.

---

## The confirmation-watermark model (Q10/11) — DROPPED

> **Superseded / not built.** We decided we don't need weekly confirm/freeze at all.
> Assignments stay live; notes (app/title level, in the project view) replace the per-week
> note idea. Kept below for history only.

- A week accumulates segments → current rules **auto-categorize** them → shown as **pending**.
- Review → pull stragglers in / take stuff out → **Save** → current state becomes
  **confirmed** for that week (materialized into `week_assignments`, frozen).
- New time that same week → fresh **pending** layer on top → Save again to confirm.
- A week "closes" naturally when it's over and everything's confirmed.
- **No retroactive rule rewriting of past weeks** — editing rules only affects pending time.
- Assigning a straggler offers: *"always send titles containing `X` to this project?"* → new rule.
  Over weeks the Unassigned pile trends to empty and the ritual becomes a ~30s glance.

---

## Navigation (two axes)

**By week** — the triage/sort surface:
- Week picker (← →). Per-project totals for the week. Unassigned section grouped by
  app → expandable titles, one-click assign (offers a rule). Weekly note. **Save**.

**By project** — the review/history surface:
- Pick a project → list of its weeks each with hours → **grand total + number of weeks** at top
  → drill into a week → app/title detail + that week's note.

---

## Out of scope (v1)

Clients, rates, money, invoices, PDF/Xero export, per-segment splitting, free-typed hour
overrides, input-idle detection, background daemon / launchd, browser URL capture,
historical backfill, the "you were away, assign this?" resume prompt.

---

## Suggested build order

1. **Tauri skeleton** + tray icon + SQLite, app-level poller only (no AX). Prove segments land.
2. **Sleep/lid/display-off** segment-closing.
3. **Projects + rules engine** (first-match-wins) + derived weekly view.
4. **Weekly view**: totals + Unassigned triage + assign-creates-rule.
5. **Confirm/Save** → materialize `week_assignments`; pending vs confirmed layering.
6. **Project history view** (weeks list, totals, drill-in).
7. **Weekly note** + **manual entries**.
8. *(Later)* Window-title capture via Accessibility API.

---

## Build log & refinements (what's actually built)

**Session 1 — Step 1 (done & verified):** Tauri 2 + React + TS + Bun scaffold in `app/`.
`db.rs` (segments) · `poller.rs` (2s `NSWorkspace` poller, segment-on-change) · tray icon.
Segments land correctly at runtime.

**Sessions 2–3 — Steps 2 & 3 (done & verified), with two model refinements after seeing
reference UIs (macOS Screen Time + Timing):**

- **Drag = per-day assignment, NOT rules.** When you drag an app onto a project it links
  *just that day's* app-time: a `day_assignments(date, app_key, project_id)` row
  (`app_key` = bundle id, else app name). No rule engine yet. This replaces the
  rules/overrides/`week_assignments` layers above for now — simpler, matches "drag it to a
  project and it's linked." Rules + window-titles can refine later.
- **Daily "Activities" view = Screen Time style.** Per-app totals for the selected day
  (sorted desc), a big day total, a 24-hour usage bar chart, and `‹ Today ›` date nav.
  App rows are draggable; assigned apps show a project tag (click to unassign).
- **Project view auto-groups by Day / Week / Month** (segmented toggle), with grand total +
  bucket count. Week = Monday-start, local tz.
- **Sleep/lid detection via wall-clock gap:** if two poll ticks are >60s apart the machine
  was suspended, so the open segment is closed at the last good tick and the gap is unbilled.
  *Known limitation:* catches sleep / lid-close (the phantom-overnight source) but not
  display-off-without-sleep or screen-lock-without-sleep — add NSWorkspace notification
  observers later if needed.

Current schema: `segments` · `projects` · `day_assignments`.
Commands: `get_day_view` · `list_projects` · `create_project` · `delete_project` ·
`set_assignment` · `project_breakdown`.

**Still deferred:** the rules engine, Screen Time-style weekly bar chart.

**Bug fixes / notes:**
- Drag-and-drop of app rows onto projects required `dragDropEnabled: false` on the window
  in `tauri.conf.json` — Tauri v2's default OS-level drag-drop handler intercepts HTML5 DnD
  inside the macOS WKWebView, so app-row drags never reached React.
- Real app icons added (`app_icon_data_url` command + per-app 24h mini timeline).
- Title-drag bug: the **Untitled** bucket shared the app-level key (`""`), so dragging it
  assigned the whole app. Fixed by making the Untitled row **non-draggable** (assign it via
  the app row) and by making the drop handler prefer the **live drag ref** over possibly
  stale React state / a WKWebView that dropped the custom dataTransfer payload.
- Project view now shows a **per-app breakdown** under the total (`project_apps` command),
  with the same title-aware resolution as the day/week/month buckets.

---

## Next up (agreed roadmap)

1. ~~**Window titles (Accessibility).**~~ **DONE.** `ax.rs` reads `AXFocusedWindow → AXTitle`
   via a self-contained ApplicationServices FFI (only the `core-foundation` crate added);
   `ax_status` / `ax_request` commands + an in-app permission banner. Segments now carry
   `window_title`; app-level stays the fallback when ungranted/untitled. Activities view
   rolls up **App → Titles** (collapsed by default, title-count badge, expandable); both app
   rows and title rows are draggable. Assignments are now keyed `(date, app_key, title)` with
   `title=""` = app-level; resolution is title-specific → app-level → Unassigned. Dots show
   *explicit* links only (app-level dot on the app row, title dot on the title row).
2. ~~**Weekly confirm + per-week note.**~~ **DROPPED** (not needed) — see the superseded
   confirmation-watermark section above. Replaced by **entry notes** (done): in the project
   view, the app breakdown now **drills down into the titles assigned to that project**, and
   a free-text **note** can be attached at **app level or per title** — one note per entry,
   editable (empty clears). New table `entry_notes(project_id, app_key, title, note)`,
   `title=""` = app-level note; untitled rows aren't separately notable. Commands:
   `project_apps` now returns nested titles + notes; `set_note` upserts/clears.
3. **Manual entries** (off-computer time) — *still deferred*. Basic name + hours added to a
   day/week (calls, in-person meetings, work on another machine). New table: `manual_entries`,
   merged into day/period/project totals.
