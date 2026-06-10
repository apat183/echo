# Echo

Echo is a personal macOS time tracker that reflects back where your time went after
the fact. It runs quietly in the menu bar, records the active app and window title,
and lets you group tracked activity into projects.

This is a personal project first. The goal is to make time review lightweight:
open Echo, scan the day or week, drag apps or window titles into projects, and use
the project view to understand where the time went. It is not built for teams,
timesheets, billing, clients, rates, or invoices.

## Features

- Menu-bar tracking with pause, resume, open, and quit controls.
- Local app and window-title capture on macOS.
- Day, week, and month activity views.
- Per-project totals and app/title breakdowns.
- Drag app or title rows onto projects to assign time.
- Entry notes for project activity.
- Local SQLite storage owned by the app.

## Privacy

Echo stores tracking data locally. It does not send activity, project names, window
titles, or notes to a server.

Window-title capture uses macOS Accessibility permission. Without that permission,
Echo falls back to app-level tracking.

## Project Status

Echo is early-stage software and is being built around a personal workflow. Expect
rough edges, schema changes, and macOS-first assumptions.

## Requirements

- macOS
- Bun
- Rust
- Tauri prerequisites for macOS, including Xcode Command Line Tools

## Development

Install dependencies:

```sh
bun install
```

Run the app in development:

```sh
bun run tauri dev
```

Build the frontend:

```sh
bun run build
```

Run Rust tests:

```sh
cd src-tauri
cargo test
```

Run frontend tests:

```sh
bun run test
```

## Repository Layout

- `src/` - React UI.
- `src-tauri/` - Rust shell, tracker, SQLite storage, tray integration, and macOS capture.

## License

No license has been added yet. Add one before accepting external contributions or
publishing this as an open-source project.
