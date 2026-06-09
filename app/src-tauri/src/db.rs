//! SQLite storage. We own this database entirely (see DESIGN.md, decision #1).
//!
//! Tables:
//!   segments        immutable, append-only ground truth (one row per app focus stretch)
//!   projects        user-created buckets
//!   day_assignments per-(local-day, app) link to a project (drag = "just that day", #7)
//!
//! All time aggregation attributes a segment to the LOCAL date of its start, so the
//! daily view and project breakdown stay consistent.

use chrono::{Datelike, Local, NaiveDate, TimeZone};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Shared handle to our SQLite connection (poller thread + commands + tray).
pub type DbState = Arc<Mutex<Connection>>;

#[derive(Debug, Serialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub color: String,
}

/// One window-title's time within an app, with its explicit project link (if any).
/// `project_id` is the title's OWN assignment — None means it inherits the app-level one.
#[derive(Debug, Serialize)]
pub struct TitleUsage {
    pub title: String, // "" = untitled / app has no window title
    pub seconds: i64,
    pub project_id: Option<i64>,
}

/// One app's total time within a day, broken down by window title.
#[derive(Debug, Serialize)]
pub struct AppUsage {
    pub app_key: String, // bundle id, else name — the stable assignment key
    pub app_name: String, // display name
    pub bundle_id: Option<String>,
    pub seconds: i64,
    pub hours: Vec<i64>,         // 24 buckets, seconds per hour of the local day
    pub project_id: Option<i64>, // app-level (title = "") assignment
    pub titles: Vec<TitleUsage>,
}

/// Everything the daily ("Activities") view needs for one local day.
#[derive(Debug, Serialize)]
pub struct DayView {
    pub date: String,
    pub total_seconds: i64,
    pub apps: Vec<AppUsage>,
    pub hours: Vec<i64>, // 24 buckets, seconds per hour of the local day
}

/// One bucket of a project breakdown (a single day; the frontend rolls up to week/month).
#[derive(Debug, Serialize)]
pub struct DayTotal {
    pub date: String,
    pub seconds: i64,
}

/// One title contributing time to a project (drill-down under an app).
#[derive(Debug, Serialize)]
pub struct ProjectTitle {
    pub title: String, // "" = untitled
    pub seconds: i64,
    pub note: Option<String>,
}

/// One app contributing time to a project (for the project view's app breakdown).
#[derive(Debug, Serialize)]
pub struct ProjectApp {
    pub app_key: String,
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub seconds: i64,
    pub note: Option<String>, // app-level note (title = "")
    pub titles: Vec<ProjectTitle>,
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS segments (
            id            INTEGER PRIMARY KEY,
            start_ts      INTEGER NOT NULL,
            end_ts        INTEGER NOT NULL,
            app_bundle_id TEXT,
            app_name      TEXT,
            window_title  TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_segments_start ON segments(start_ts);

        CREATE TABLE IF NOT EXISTS projects (
            id         INTEGER PRIMARY KEY,
            name       TEXT NOT NULL,
            color      TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS day_assignments (
            date          TEXT NOT NULL,   -- local 'YYYY-MM-DD'
            app_key       TEXT NOT NULL,   -- bundle id, else app name
            title         TEXT NOT NULL DEFAULT '',  -- '' = app-level (all titles)
            project_id    INTEGER NOT NULL,
            PRIMARY KEY (date, app_key, title)
        );

        CREATE TABLE IF NOT EXISTS entry_notes (
            project_id INTEGER NOT NULL,
            app_key    TEXT NOT NULL,
            title      TEXT NOT NULL DEFAULT '',  -- '' = app-level note
            note       TEXT NOT NULL,
            PRIMARY KEY (project_id, app_key, title)
        );",
    )?;
    migrate_assignments_title(&conn)?;
    Ok(conn)
}

/// Older DBs keyed day_assignments by (date, app_key) with no `title`. Rebuild the
/// table with the title column; existing rows become app-level ('').
fn migrate_assignments_title(conn: &Connection) -> rusqlite::Result<()> {
    let mut has_title = false;
    {
        let mut stmt = conn.prepare("PRAGMA table_info(day_assignments)")?;
        let cols = stmt.query_map([], |r| r.get::<_, String>(1))?;
        for c in cols {
            if c? == "title" {
                has_title = true;
            }
        }
    }
    if !has_title {
        conn.execute_batch(
            "ALTER TABLE day_assignments RENAME TO day_assignments_old;
             CREATE TABLE day_assignments (
                date       TEXT NOT NULL,
                app_key    TEXT NOT NULL,
                title      TEXT NOT NULL DEFAULT '',
                project_id INTEGER NOT NULL,
                PRIMARY KEY (date, app_key, title)
             );
             INSERT INTO day_assignments (date, app_key, title, project_id)
                SELECT date, app_key, '', project_id FROM day_assignments_old;
             DROP TABLE day_assignments_old;",
        )?;
    }
    Ok(())
}

pub fn insert_segment(
    conn: &Connection,
    start_ts: i64,
    end_ts: i64,
    app_bundle_id: Option<&str>,
    app_name: Option<&str>,
    window_title: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO segments (start_ts, end_ts, app_bundle_id, app_name, window_title)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![start_ts, end_ts, app_bundle_id, app_name, window_title],
    )?;
    Ok(())
}

// ---- projects -------------------------------------------------------------

pub fn list_projects(conn: &Connection) -> rusqlite::Result<Vec<Project>> {
    let mut stmt =
        conn.prepare("SELECT id, name, color FROM projects ORDER BY created_at ASC")?;
    let rows = stmt.query_map([], |r| {
        Ok(Project {
            id: r.get(0)?,
            name: r.get(1)?,
            color: r.get(2)?,
        })
    })?;
    rows.collect()
}

pub fn create_project(conn: &Connection, name: &str, color: &str) -> rusqlite::Result<Project> {
    let now = Local::now().timestamp();
    conn.execute(
        "INSERT INTO projects (name, color, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![name, color, now],
    )?;
    Ok(Project {
        id: conn.last_insert_rowid(),
        name: name.to_string(),
        color: color.to_string(),
    })
}

pub fn delete_project(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM projects WHERE id = ?1", [id])?;
    conn.execute("DELETE FROM day_assignments WHERE project_id = ?1", [id])?;
    Ok(())
}

// ---- assignments ----------------------------------------------------------

/// Link (or, with `project_id = None`, unlink) a day's app/title-time to a project.
/// `title = ""` is the app-level link covering every title not assigned on its own.
pub fn set_assignment(
    conn: &Connection,
    date: &str,
    app_key: &str,
    title: &str,
    project_id: Option<i64>,
) -> rusqlite::Result<()> {
    match project_id {
        Some(pid) => conn.execute(
            "INSERT INTO day_assignments (date, app_key, title, project_id) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(date, app_key, title) DO UPDATE SET project_id = excluded.project_id",
            rusqlite::params![date, app_key, title, pid],
        )?,
        None => conn.execute(
            "DELETE FROM day_assignments WHERE date = ?1 AND app_key = ?2 AND title = ?3",
            rusqlite::params![date, app_key, title],
        )?,
    };
    Ok(())
}

// ---- aggregation ----------------------------------------------------------

fn app_key(bundle: &Option<String>, name: &Option<String>) -> String {
    bundle
        .clone()
        .or_else(|| name.clone())
        .unwrap_or_else(|| "unknown".to_string())
}

fn local_date_string(ts: i64) -> String {
    let dt = Local.timestamp_opt(ts, 0).single().expect("valid ts");
    format!("{:04}-{:02}-{:02}", dt.year(), dt.month(), dt.day())
}

/// Local midnight (unix seconds) for a 'YYYY-MM-DD' date string.
fn day_start_ts(date: &str) -> i64 {
    let nd = NaiveDate::parse_from_str(date, "%Y-%m-%d").expect("valid date");
    let naive = nd.and_hms_opt(0, 0, 0).unwrap();
    Local
        .from_local_datetime(&naive)
        .single()
        .expect("unambiguous midnight")
        .timestamp()
}

pub fn day_view(conn: &Connection, date: &str) -> rusqlite::Result<DayView> {
    let start = day_start_ts(date);
    let end = start + 86_400;

    // Assignments for this day: (app_key, title) -> project_id. title "" = app-level.
    let mut assigns: HashMap<(String, String), i64> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT app_key, title, project_id FROM day_assignments WHERE date = ?1")?;
        let rows = stmt.query_map([date], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (k, t, p) = row?;
            assigns.insert((k, t), p);
        }
    }

    struct AppAcc {
        app_name: String,
        bundle_id: Option<String>,
        seconds: i64,
        hours: Vec<i64>,
        titles: HashMap<String, i64>, // title -> seconds
    }

    let mut by_app: HashMap<String, AppAcc> = HashMap::new();
    let mut hours = vec![0i64; 24];
    let mut total = 0i64;

    let mut stmt = conn.prepare(
        "SELECT start_ts, end_ts, app_bundle_id, app_name, window_title
         FROM segments WHERE start_ts >= ?1 AND start_ts < ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![start, end], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;

    for row in rows {
        let (s_ts, e_ts, bundle, name, window_title) = row?;
        let dur = (e_ts - s_ts).max(0);
        total += dur;

        let key = app_key(&bundle, &name);
        let display = name
            .clone()
            .or_else(|| bundle.clone())
            .unwrap_or_else(|| key.clone());
        let title = window_title.unwrap_or_default();

        let entry = by_app.entry(key.clone()).or_insert_with(|| AppAcc {
            app_name: display,
            bundle_id: bundle.clone(),
            seconds: 0,
            hours: vec![0i64; 24],
            titles: HashMap::new(),
        });
        entry.seconds += dur;
        *entry.titles.entry(title).or_insert(0) += dur;

        // Spread across hour buckets of this local day (cap overflow into hour 23).
        let mut t = s_ts;
        while t < e_ts {
            let idx = (((t - start) / 3600).clamp(0, 23)) as usize;
            let hour_end = start + ((idx as i64) + 1) * 3600;
            let chunk_end = e_ts.min(hour_end);
            let chunk = (chunk_end - t).max(0);
            hours[idx] += chunk;
            entry.hours[idx] += chunk;
            if chunk_end <= t {
                break;
            }
            t = chunk_end;
        }
    }

    let mut apps: Vec<AppUsage> = by_app
        .into_iter()
        .map(|(key, acc)| {
            let mut titles: Vec<TitleUsage> = acc
                .titles
                .into_iter()
                .map(|(title, seconds)| {
                    let project_id = assigns.get(&(key.clone(), title.clone())).copied();
                    TitleUsage {
                        title,
                        seconds,
                        project_id,
                    }
                })
                .collect();
            titles.sort_by(|a, b| b.seconds.cmp(&a.seconds));
            AppUsage {
                project_id: assigns.get(&(key.clone(), String::new())).copied(),
                app_key: key,
                app_name: acc.app_name,
                bundle_id: acc.bundle_id,
                seconds: acc.seconds,
                hours: acc.hours,
                titles,
            }
        })
        .collect();
    apps.sort_by(|a, b| b.seconds.cmp(&a.seconds));

    Ok(DayView {
        date: date.to_string(),
        total_seconds: total,
        apps,
        hours,
    })
}

/// Per-day totals for everything that resolves to `project_id` (newest day first).
/// Resolution per segment: its own (app, title) link, else the app-level (app, "") link.
pub fn project_breakdown(conn: &Connection, project_id: i64) -> rusqlite::Result<Vec<DayTotal>> {
    // All assignments: (date, app_key, title) -> project_id.
    let mut assigns: HashMap<(String, String, String), i64> = HashMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT date, app_key, title, project_id FROM day_assignments")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        for row in rows {
            let (d, a, t, p) = row?;
            assigns.insert((d, a, t), p);
        }
    }
    if assigns.is_empty() {
        return Ok(vec![]);
    }

    let mut totals: HashMap<String, i64> = HashMap::new();
    let mut stmt = conn
        .prepare("SELECT start_ts, end_ts, app_bundle_id, app_name, window_title FROM segments")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;
    for row in rows {
        let (s_ts, e_ts, bundle, name, window_title) = row?;
        let date = local_date_string(s_ts);
        let key = app_key(&bundle, &name);
        let title = window_title.unwrap_or_default();
        let resolved = assigns
            .get(&(date.clone(), key.clone(), title))
            .or_else(|| assigns.get(&(date.clone(), key.clone(), String::new())))
            .copied();
        if resolved == Some(project_id) {
            *totals.entry(date).or_insert(0) += (e_ts - s_ts).max(0);
        }
    }

    let mut out: Vec<DayTotal> = totals
        .into_iter()
        .map(|(date, seconds)| DayTotal { date, seconds })
        .collect();
    out.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(out)
}

/// Which apps (and their titles) make up a project's total, with any notes attached.
/// Same per-segment resolution as the breakdown.
pub fn project_apps(conn: &Connection, project_id: i64) -> rusqlite::Result<Vec<ProjectApp>> {
    let mut assigns: HashMap<(String, String, String), i64> = HashMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT date, app_key, title, project_id FROM day_assignments")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        for row in rows {
            let (d, a, t, p) = row?;
            assigns.insert((d, a, t), p);
        }
    }
    if assigns.is_empty() {
        return Ok(vec![]);
    }

    // Notes for this project: (app_key, title) -> note.
    let mut notes: HashMap<(String, String), String> = HashMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT app_key, title, note FROM entry_notes WHERE project_id = ?1")?;
        let rows = stmt.query_map([project_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (a, t, n) = row?;
            notes.insert((a, t), n);
        }
    }

    struct AppAcc {
        name: String,
        bundle: Option<String>,
        seconds: i64,
        titles: HashMap<String, i64>,
    }
    let mut by_app: HashMap<String, AppAcc> = HashMap::new();

    let mut stmt = conn
        .prepare("SELECT start_ts, end_ts, app_bundle_id, app_name, window_title FROM segments")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;
    for row in rows {
        let (s_ts, e_ts, bundle, name, window_title) = row?;
        let date = local_date_string(s_ts);
        let key = app_key(&bundle, &name);
        let title = window_title.unwrap_or_default();
        let resolved = assigns
            .get(&(date.clone(), key.clone(), title.clone()))
            .or_else(|| assigns.get(&(date.clone(), key.clone(), String::new())))
            .copied();
        if resolved == Some(project_id) {
            let display = name
                .clone()
                .or_else(|| bundle.clone())
                .unwrap_or_else(|| key.clone());
            let acc = by_app.entry(key.clone()).or_insert_with(|| AppAcc {
                name: display,
                bundle: bundle.clone(),
                seconds: 0,
                titles: HashMap::new(),
            });
            let dur = (e_ts - s_ts).max(0);
            acc.seconds += dur;
            *acc.titles.entry(title).or_insert(0) += dur;
        }
    }

    let mut out: Vec<ProjectApp> = by_app
        .into_iter()
        .map(|(key, acc)| {
            let mut titles: Vec<ProjectTitle> = acc
                .titles
                .into_iter()
                .map(|(title, seconds)| {
                    // Untitled rows aren't independently notable (key collides with app-level).
                    let note = if title.is_empty() {
                        None
                    } else {
                        notes.get(&(key.clone(), title.clone())).cloned()
                    };
                    ProjectTitle {
                        title,
                        seconds,
                        note,
                    }
                })
                .collect();
            titles.sort_by(|a, b| b.seconds.cmp(&a.seconds));
            ProjectApp {
                note: notes.get(&(key.clone(), String::new())).cloned(),
                app_key: key,
                app_name: acc.name,
                bundle_id: acc.bundle,
                seconds: acc.seconds,
                titles,
            }
        })
        .collect();
    out.sort_by(|a, b| b.seconds.cmp(&a.seconds));
    Ok(out)
}

/// Add, edit, or (with an empty note) clear the note on a project entry.
/// `title = ""` is the app-level note.
pub fn set_note(
    conn: &Connection,
    project_id: i64,
    app_key: &str,
    title: &str,
    note: &str,
) -> rusqlite::Result<()> {
    let trimmed = note.trim();
    if trimmed.is_empty() {
        conn.execute(
            "DELETE FROM entry_notes WHERE project_id = ?1 AND app_key = ?2 AND title = ?3",
            rusqlite::params![project_id, app_key, title],
        )?;
    } else {
        conn.execute(
            "INSERT INTO entry_notes (project_id, app_key, title, note) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(project_id, app_key, title) DO UPDATE SET note = excluded.note",
            rusqlite::params![project_id, app_key, title, trimmed],
        )?;
    }
    Ok(())
}
