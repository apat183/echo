//! SQLite storage. We own this database entirely.
//!
//! Tables:
//!   segments        immutable, append-only ground truth (one row per app focus stretch)
//!   projects        user-created buckets
//!   day_assignments per-(local-day, app) tags linking time to projects, many-to-many
//!                   (drag = "just that day", #7); one app/title can be tagged with
//!                   several projects and each is billed the full duration
//!   ignored_entries app/title rules excluded from activity totals and projects
//!   project_period_notes per-project notes attached to day/week/month rollups
//!
//! All time aggregation attributes a segment to the LOCAL date of its start, so the
//! daily view and project breakdown stay consistent.

use chrono::{Datelike, Local, NaiveDate, TimeZone};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
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

/// One window-title's time within an app, with its explicit project tags (if any).
/// `project_ids` are the title's OWN assignments — empty means it inherits the
/// app-level ones. Multiple ids = tagged with several projects.
#[derive(Debug, Serialize)]
pub struct TitleUsage {
    pub title: String, // "" = untitled / app has no window title
    pub seconds: i64,
    pub project_ids: Vec<i64>,
}

/// One app's total time within a day, broken down by window title.
#[derive(Debug, Serialize)]
pub struct AppUsage {
    pub app_key: String,  // bundle id, else name — the stable assignment key
    pub app_name: String, // display name
    pub bundle_id: Option<String>,
    pub seconds: i64,
    pub hours: Vec<i64>,       // 24 buckets, seconds per hour of the local day
    pub project_ids: Vec<i64>, // app-level (title = "") tags; empty = unassigned
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

/// One app/title rule excluded from activity totals and projects.
#[derive(Debug, Serialize)]
pub struct IgnoredEntry {
    pub app_key: String,
    pub app_name: Option<String>,
    pub title: String,
    pub created_at: i64,
}

/// A note attached to a project's day/week/month rollup bucket.
#[derive(Debug, Serialize)]
pub struct ProjectPeriodNote {
    pub granularity: String,
    pub period_key: String,
    pub note: String,
}

/// One title contributing time to a project (drill-down under an app).
#[derive(Debug, Serialize)]
pub struct ProjectTitle {
    pub title: String, // "" = untitled
    pub seconds: i64,
    pub can_remove: bool,
}

/// One app contributing time to a project (for the project view's app breakdown).
#[derive(Debug, Serialize)]
pub struct ProjectApp {
    pub app_key: String,
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub seconds: i64,
    pub titles: Vec<ProjectTitle>,
}

/// Auto-delete-untagged configuration.
/// Mirrors `AutodeleteConfig` in src/api.ts; keep the two in sync.
#[derive(Debug, Serialize)]
pub struct AutodeleteConfig {
    pub enabled: bool,
    pub days: u32,
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
            created_at INTEGER NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS day_assignments (
            date          TEXT NOT NULL,   -- local 'YYYY-MM-DD'
            app_key       TEXT NOT NULL,   -- bundle id, else app name
            title         TEXT NOT NULL DEFAULT '',  -- '' = app-level (all titles)
            project_id    INTEGER NOT NULL,
            -- many-to-many tags: one (date, app_key, title) may link to several projects
            PRIMARY KEY (date, app_key, title, project_id)
        );

        CREATE TABLE IF NOT EXISTS ignored_entries (
            app_key    TEXT NOT NULL,
            app_name   TEXT,
            title      TEXT NOT NULL DEFAULT '',  -- '' = app-level ignore
            created_at INTEGER NOT NULL,
            PRIMARY KEY (app_key, title)
        );

        CREATE TABLE IF NOT EXISTS project_period_notes (
            project_id  INTEGER NOT NULL,
            granularity TEXT NOT NULL, -- day | week | month
            period_key  TEXT NOT NULL,
            note        TEXT NOT NULL,
            PRIMARY KEY (project_id, granularity, period_key)
        );

        CREATE TABLE IF NOT EXISTS app_settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;
    migrate_assignments_title(&conn)?;
    migrate_assignments_multi(&conn)?;
    migrate_projects_sort_order(&conn)?;
    migrate_ignored_entries_app_name(&conn)?;
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
                PRIMARY KEY (date, app_key, title, project_id)
             );
             INSERT INTO day_assignments (date, app_key, title, project_id)
                SELECT date, app_key, '', project_id FROM day_assignments_old;
             DROP TABLE day_assignments_old;",
        )?;
    }
    Ok(())
}

/// Older DBs keyed day_assignments by (date, app_key, title) — one project per
/// entry. Tag semantics need many-to-many, so project_id joins the primary key.
/// The column list is identical pre/post, so we detect the old schema by the
/// `pk` flag of the project_id column (index 5 of PRAGMA table_info): it is 0
/// when project_id isn't part of the PK (old) and 4 in the new 4-column PK.
/// On detection we rebuild, carrying every existing row across unchanged.
fn migrate_assignments_multi(conn: &Connection) -> rusqlite::Result<()> {
    let mut project_id_in_pk = false;
    {
        let mut stmt = conn.prepare("PRAGMA table_info(day_assignments)")?;
        let cols = stmt.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(5)?)))?;
        for c in cols {
            let (name, pk) = c?;
            if name == "project_id" && pk != 0 {
                project_id_in_pk = true;
            }
        }
    }
    if !project_id_in_pk {
        conn.execute_batch(
            "ALTER TABLE day_assignments RENAME TO day_assignments_old;
             CREATE TABLE day_assignments (
                date       TEXT NOT NULL,
                app_key    TEXT NOT NULL,
                title      TEXT NOT NULL DEFAULT '',
                project_id INTEGER NOT NULL,
                PRIMARY KEY (date, app_key, title, project_id)
             );
             INSERT INTO day_assignments (date, app_key, title, project_id)
                SELECT date, app_key, title, project_id FROM day_assignments_old;
             DROP TABLE day_assignments_old;",
        )?;
    }
    Ok(())
}

/// Older DBs have no `sort_order` column on projects. Add it and backfill
/// 0..N-1 in `created_at` order (with `id` as the tiebreaker for equal
/// timestamps) so the existing project list order is preserved.
fn migrate_projects_sort_order(conn: &Connection) -> rusqlite::Result<()> {
    let mut has_sort_order = false;
    {
        let mut stmt = conn.prepare("PRAGMA table_info(projects)")?;
        let cols = stmt.query_map([], |r| r.get::<_, String>(1))?;
        for c in cols {
            if c? == "sort_order" {
                has_sort_order = true;
            }
        }
    }
    if !has_sort_order {
        conn.execute_batch(
            "ALTER TABLE projects ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;
             UPDATE projects SET sort_order = (
               SELECT COUNT(*) FROM projects p2
               WHERE p2.created_at < projects.created_at
                  OR (p2.created_at = projects.created_at AND p2.id < projects.id)
             );",
        )?;
    }
    Ok(())
}

/// Older ignore tables did not store a display name. Keep the rules and add a
/// nullable display-name column so the ignored view can show app names.
fn migrate_ignored_entries_app_name(conn: &Connection) -> rusqlite::Result<()> {
    let mut has_app_name = false;
    {
        let mut stmt = conn.prepare("PRAGMA table_info(ignored_entries)")?;
        let cols = stmt.query_map([], |r| r.get::<_, String>(1))?;
        for c in cols {
            if c? == "app_name" {
                has_app_name = true;
            }
        }
    }
    if !has_app_name {
        conn.execute_batch("ALTER TABLE ignored_entries ADD COLUMN app_name TEXT;")?;
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
        conn.prepare("SELECT id, name, color FROM projects ORDER BY sort_order ASC, id ASC")?;
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
        "INSERT INTO projects (name, color, created_at, sort_order)
         VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM projects))",
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

/// Persist a new display order for projects. `ids` must be the **complete**
/// ordered list of project ids; each project's `sort_order` is set to its
/// index in the slice. An omitted project keeps its previous `sort_order` and
/// may collide with a newly-assigned value; unknown ids are silently ignored.
/// All updates are applied atomically in a single transaction.
pub fn set_project_order(conn: &Connection, ids: &[i64]) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    for (i, &id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE projects SET sort_order = ?1 WHERE id = ?2",
            rusqlite::params![i as i64, id],
        )?;
    }
    tx.commit()
}

// ---- assignments ----------------------------------------------------------

/// Tag a day's app/title-time with a project. Tags are additive: an entry may
/// carry several projects, each billed the full duration. Idempotent — adding the
/// same tag twice is a no-op. `title = ""` is the app-level tag covering every
/// title not tagged on its own.
pub fn add_assignment(
    conn: &Connection,
    date: &str,
    app_key: &str,
    title: &str,
    project_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO day_assignments (date, app_key, title, project_id) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(date, app_key, title, project_id) DO NOTHING",
        rusqlite::params![date, app_key, title, project_id],
    )?;
    Ok(())
}

/// Remove one project tag from a day's app/title-time, leaving any other tags
/// on the same entry intact. `title = ""` is the app-level tag.
pub fn remove_assignment(
    conn: &Connection,
    date: &str,
    app_key: &str,
    title: &str,
    project_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM day_assignments
         WHERE date = ?1 AND app_key = ?2 AND title = ?3 AND project_id = ?4",
        rusqlite::params![date, app_key, title, project_id],
    )?;
    Ok(())
}

/// Remove every assignment connecting this app to the project, across all days
/// and titles. Used by the project view's app-level remove action.
pub fn remove_project_app_assignments(
    conn: &Connection,
    project_id: i64,
    app_key: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM day_assignments WHERE project_id = ?1 AND app_key = ?2",
        rusqlite::params![project_id, app_key],
    )?;
    Ok(())
}

/// Remove explicit title-level assignments connecting this title to the project.
/// App-level assignments may still make the title inherit the project.
pub fn remove_project_title_assignments(
    conn: &Connection,
    project_id: i64,
    app_key: &str,
    title: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM day_assignments
         WHERE project_id = ?1 AND app_key = ?2 AND title = ?3",
        rusqlite::params![project_id, app_key, title],
    )?;
    Ok(())
}

// ---- ignored entries ------------------------------------------------------

/// Exclude an app/title from all activity totals. `title = ""` ignores the
/// whole app; a non-empty title ignores only that title.
pub fn add_ignored_entry(
    conn: &Connection,
    app_key: &str,
    app_name: Option<&str>,
    title: &str,
) -> rusqlite::Result<()> {
    let now = Local::now().timestamp();
    conn.execute(
        "INSERT INTO ignored_entries (app_key, app_name, title, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(app_key, title) DO UPDATE SET
            app_name = COALESCE(excluded.app_name, ignored_entries.app_name)",
        rusqlite::params![app_key, app_name, title, now],
    )?;
    Ok(())
}

pub fn list_ignored_entries(conn: &Connection) -> rusqlite::Result<Vec<IgnoredEntry>> {
    let mut stmt = conn.prepare(
        "SELECT app_key, app_name, title, created_at
         FROM ignored_entries
         ORDER BY created_at DESC, app_key ASC, title ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(IgnoredEntry {
            app_key: r.get(0)?,
            app_name: r.get(1)?,
            title: r.get(2)?,
            created_at: r.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn remove_ignored_entry(conn: &Connection, app_key: &str, title: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM ignored_entries WHERE app_key = ?1 AND title = ?2",
        rusqlite::params![app_key, title],
    )?;
    Ok(())
}

// ---- aggregation ----------------------------------------------------------

/// One row of the immutable `segments` table, with the domain derivations
/// (stable key, display name, duration, local date) defined once so every
/// aggregation reads a segment the same way.
struct Segment {
    start_ts: i64,
    end_ts: i64,
    bundle_id: Option<String>,
    name: Option<String>,
    title: Option<String>,
}

impl Segment {
    /// Stable assignment key: bundle id, else app name, else "unknown".
    fn key(&self) -> String {
        self.bundle_id
            .clone()
            .or_else(|| self.name.clone())
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Human-facing name: app name, else bundle id, else the key.
    fn display_name(&self) -> String {
        self.name
            .clone()
            .or_else(|| self.bundle_id.clone())
            .unwrap_or_else(|| self.key())
    }

    /// Window title, "" when the app exposed none.
    fn title(&self) -> String {
        self.title.clone().unwrap_or_default()
    }

    /// Non-negative duration in seconds.
    fn duration(&self) -> i64 {
        (self.end_ts - self.start_ts).max(0)
    }

    /// Local calendar date (YYYY-MM-DD) the segment is billed to — its start.
    fn local_date(&self) -> String {
        local_date_string(self.start_ts)
    }
}

/// Read segments from the table. `Some((start, end))` keeps only those *starting*
/// in `[start, end)` (the day view); `None` reads every segment (project rollups).
fn read_segments(conn: &Connection, range: Option<(i64, i64)>) -> rusqlite::Result<Vec<Segment>> {
    fn row(r: &rusqlite::Row) -> rusqlite::Result<Segment> {
        Ok(Segment {
            start_ts: r.get(0)?,
            end_ts: r.get(1)?,
            bundle_id: r.get(2)?,
            name: r.get(3)?,
            title: r.get(4)?,
        })
    }
    match range {
        Some((start, end)) => {
            let mut stmt = conn.prepare(
                "SELECT start_ts, end_ts, app_bundle_id, app_name, window_title
                 FROM segments WHERE start_ts >= ?1 AND start_ts < ?2",
            )?;
            let rows = stmt.query_map(rusqlite::params![start, end], row)?;
            rows.collect()
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT start_ts, end_ts, app_bundle_id, app_name, window_title FROM segments",
            )?;
            let rows = stmt.query_map([], row)?;
            rows.collect()
        }
    }
}

/// Project assignments with the resolution rule in one place. Keyed
/// (date, app_key, title) → the projects tagged onto it; `title = ""` is the
/// app-level entry covering every title that has no tag of its own. Each entry's
/// project list is sorted ascending for deterministic output.
struct Assignments {
    by_key: HashMap<(String, String, String), Vec<i64>>,
}

impl Assignments {
    /// Every assignment — project rollups span all days.
    fn load(conn: &Connection) -> rusqlite::Result<Self> {
        let mut stmt =
            conn.prepare("SELECT date, app_key, title, project_id FROM day_assignments")?;
        let rows = stmt.query_map([], Self::row)?;
        Self::collect(rows)
    }

    /// Just one day's assignments — the day view only needs the one date.
    fn load_day(conn: &Connection, date: &str) -> rusqlite::Result<Self> {
        let mut stmt = conn.prepare(
            "SELECT date, app_key, title, project_id FROM day_assignments WHERE date = ?1",
        )?;
        let rows = stmt.query_map([date], Self::row)?;
        Self::collect(rows)
    }

    fn row(r: &rusqlite::Row) -> rusqlite::Result<(String, String, String, i64)> {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    }

    fn collect<I>(rows: I) -> rusqlite::Result<Self>
    where
        I: Iterator<Item = rusqlite::Result<(String, String, String, i64)>>,
    {
        let mut by_key: HashMap<(String, String, String), Vec<i64>> = HashMap::new();
        for row in rows {
            let (date, app_key, title, project_id) = row?;
            by_key
                .entry((date, app_key, title))
                .or_default()
                .push(project_id);
        }
        // Deterministic order regardless of row arrival order.
        for ids in by_key.values_mut() {
            ids.sort_unstable();
        }
        Ok(Self { by_key })
    }

    fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// The explicit tags for exactly this (date, app_key, title) — no fallback.
    /// The day view shows these per row so inheritance can be rendered.
    fn links(&self, date: &str, app_key: &str, title: &str) -> &[i64] {
        self.by_key
            .get(&(date.to_string(), app_key.to_string(), title.to_string()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The projects a segment is billed to: its own title tags if any, else the
    /// app-level (title = "") tags. The title's tags OVERRIDE the app-level ones
    /// (no union) — an explicitly-tagged title is not also billed app-level.
    fn resolve(&self, date: &str, app_key: &str, title: &str) -> &[i64] {
        let own = self.links(date, app_key, title);
        if own.is_empty() {
            self.links(date, app_key, "")
        } else {
            own
        }
    }
}

/// Global app/title ignore rules. An app-level rule (`title = ""`) hides every
/// title for that app; a title-level rule hides only an exact title.
struct Ignores {
    by_key: HashSet<(String, String)>,
}

impl Ignores {
    fn load(conn: &Connection) -> rusqlite::Result<Self> {
        let mut stmt = conn.prepare("SELECT app_key, title FROM ignored_entries")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut by_key = HashSet::new();
        for row in rows {
            by_key.insert(row?);
        }
        Ok(Self { by_key })
    }

    fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    fn matches(&self, app_key: &str, title: &str) -> bool {
        self.by_key.contains(&(app_key.to_string(), String::new()))
            || self
                .by_key
                .contains(&(app_key.to_string(), title.to_string()))
    }
}

pub(crate) fn local_date_string(ts: i64) -> String {
    let dt = Local.timestamp_opt(ts, 0).single().expect("valid ts");
    format!("{:04}-{:02}-{:02}", dt.year(), dt.month(), dt.day())
}

/// Local midnight (unix seconds) for a 'YYYY-MM-DD' date string.
pub(crate) fn day_start_ts(date: &str) -> i64 {
    let nd = NaiveDate::parse_from_str(date, "%Y-%m-%d").expect("valid date");
    let naive = nd.and_hms_opt(0, 0, 0).unwrap();
    Local
        .from_local_datetime(&naive)
        .single()
        .expect("unambiguous midnight")
        .timestamp()
}

// ---- storage maintenance --------------------------------------------------

/// The ids of segments that resolve to **zero** projects ("untagged"). `before`
/// = `Some(ts)` restricts to segments *starting* before `ts` (auto-delete);
/// `None` considers every segment ("clear untagged"). Untagged is defined by
/// the same `Assignments::resolve` used everywhere else — the app-level
/// (`title = ""`) fallback is included, so a title covered only by an app-level
/// tag is NOT untagged.
fn untagged_segment_ids(conn: &Connection, before: Option<i64>) -> rusqlite::Result<Vec<i64>> {
    let assigns = Assignments::load(conn)?;
    let sql = match before {
        Some(_) => {
            "SELECT id, start_ts, end_ts, app_bundle_id, app_name, window_title \
             FROM segments WHERE start_ts < ?1"
        }
        None => "SELECT id, start_ts, end_ts, app_bundle_id, app_name, window_title FROM segments",
    };
    fn row(r: &rusqlite::Row) -> rusqlite::Result<(i64, Segment)> {
        Ok((
            r.get(0)?,
            Segment {
                start_ts: r.get(1)?,
                end_ts: r.get(2)?,
                bundle_id: r.get(3)?,
                name: r.get(4)?,
                title: r.get(5)?,
            },
        ))
    }
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<(i64, Segment)> = match before {
        Some(ts) => stmt
            .query_map(rusqlite::params![ts], row)?
            .collect::<rusqlite::Result<_>>()?,
        None => stmt.query_map([], row)?.collect::<rusqlite::Result<_>>()?,
    };
    let mut ids = Vec::new();
    for (id, seg) in rows {
        if assigns
            .resolve(&seg.local_date(), &seg.key(), &seg.title())
            .is_empty()
        {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Delete a set of segment ids atomically.
fn delete_segment_ids(conn: &Connection, ids: &[i64]) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    for &id in ids {
        tx.execute("DELETE FROM segments WHERE id = ?1", [id])?;
    }
    tx.commit()
}

/// Delete every untagged segment (any age). Keeps projects, assignments, and
/// ignore rules. Returns the number of segments removed.
pub fn clear_untagged(conn: &Connection) -> rusqlite::Result<usize> {
    let ids = untagged_segment_ids(conn, None)?;
    let n = ids.len();
    delete_segment_ids(conn, &ids)?;
    Ok(n)
}

/// Delete untagged segments that *started* more than `days` days ago. Used by
/// the auto-delete scheduler. Returns the number removed.
pub fn purge_untagged_older_than(conn: &Connection, days: u32) -> rusqlite::Result<usize> {
    let cutoff = Local::now().timestamp() - i64::from(days) * 86_400;
    let ids = untagged_segment_ids(conn, Some(cutoff))?;
    let n = ids.len();
    delete_segment_ids(conn, &ids)?;
    Ok(n)
}

/// Clear all *tracking* data — segments, day assignments, and period notes —
/// while keeping user-defined projects and ignore rules.
pub fn clear_tracking_data(conn: &Connection) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM segments", [])?;
    tx.execute("DELETE FROM day_assignments", [])?;
    tx.execute("DELETE FROM project_period_notes", [])?;
    tx.commit()
}

/// Wipe everything — all five data tables. Used by "Reset everything". Leaves
/// `app_settings` (theme lives in localStorage; auto-delete config is intentionally
/// preserved so a reset doesn't silently re-enable/disable purging).
pub fn reset_everything(conn: &Connection) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM segments", [])?;
    tx.execute("DELETE FROM day_assignments", [])?;
    tx.execute("DELETE FROM project_period_notes", [])?;
    tx.execute("DELETE FROM ignored_entries", [])?;
    tx.execute("DELETE FROM projects", [])?;
    tx.commit()
}

// ---- app settings (key/value config) --------------------------------------

const AUTODELETE_ENABLED: &str = "autodelete_enabled";
const AUTODELETE_DAYS: &str = "autodelete_days";
const DEFAULT_AUTODELETE_DAYS: u32 = 30;

pub fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM app_settings WHERE key = ?1", [key], |r| {
        r.get(0)
    })
    .optional()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

/// The saved auto-delete config. Defaults: disabled, 30 days.
pub fn get_autodelete_config(conn: &Connection) -> rusqlite::Result<AutodeleteConfig> {
    let enabled = get_setting(conn, AUTODELETE_ENABLED)?.as_deref() == Some("1");
    let days = get_setting(conn, AUTODELETE_DAYS)?
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|&d| d >= 1)
        .unwrap_or(DEFAULT_AUTODELETE_DAYS);
    Ok(AutodeleteConfig { enabled, days })
}

pub fn set_autodelete_config(conn: &Connection, enabled: bool, days: u32) -> rusqlite::Result<()> {
    set_setting(conn, AUTODELETE_ENABLED, if enabled { "1" } else { "0" })?;
    set_setting(conn, AUTODELETE_DAYS, &days.max(1).to_string())?;
    Ok(())
}

pub fn day_view(conn: &Connection, date: &str) -> rusqlite::Result<DayView> {
    let start = day_start_ts(date);
    let end = start + 86_400;
    let assigns = Assignments::load_day(conn, date)?;
    let ignores = Ignores::load(conn)?;

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

    for seg in read_segments(conn, Some((start, end)))? {
        let key = seg.key();
        let title = seg.title();
        if ignores.matches(&key, &title) {
            continue;
        }

        let dur = seg.duration();
        total += dur;

        let entry = by_app.entry(key.clone()).or_insert_with(|| AppAcc {
            app_name: seg.display_name(),
            bundle_id: seg.bundle_id.clone(),
            seconds: 0,
            hours: vec![0i64; 24],
            titles: HashMap::new(),
        });
        entry.seconds += dur;
        *entry.titles.entry(title).or_insert(0) += dur;

        // Spread across hour buckets of this local day (cap overflow into hour 23).
        let mut t = seg.start_ts;
        while t < seg.end_ts {
            let idx = (((t - start) / 3600).clamp(0, 23)) as usize;
            let hour_end = start + ((idx as i64) + 1) * 3600;
            let chunk_end = seg.end_ts.min(hour_end);
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
                .map(|(title, seconds)| TitleUsage {
                    project_ids: assigns.links(date, &key, &title).to_vec(),
                    title,
                    seconds,
                })
                .collect();
            titles.sort_by_key(|title| std::cmp::Reverse(title.seconds));
            AppUsage {
                project_ids: assigns.links(date, &key, "").to_vec(),
                app_key: key,
                app_name: acc.app_name,
                bundle_id: acc.bundle_id,
                seconds: acc.seconds,
                hours: acc.hours,
                titles,
            }
        })
        .collect();
    apps.sort_by_key(|app| std::cmp::Reverse(app.seconds));

    Ok(DayView {
        date: date.to_string(),
        total_seconds: total,
        apps,
        hours,
    })
}

/// Total tracked seconds for one local day (segments attributed by start_ts,
/// same convention as day_view).
pub fn day_total_seconds(conn: &Connection, date: &str) -> rusqlite::Result<i64> {
    let start = day_start_ts(date);
    let end = start + 86_400;
    let ignores = Ignores::load(conn)?;
    let mut total = 0;
    for seg in read_segments(conn, Some((start, end)))? {
        let key = seg.key();
        let title = seg.title();
        if !ignores.matches(&key, &title) {
            total += seg.duration();
        }
    }
    Ok(total)
}

/// Per-day totals for everything that resolves to `project_id` (newest day first).
/// Resolution per segment: its own (app, title) tags, else the app-level (app, "") tags.
pub fn project_breakdown(conn: &Connection, project_id: i64) -> rusqlite::Result<Vec<DayTotal>> {
    let assigns = Assignments::load(conn)?;
    if assigns.is_empty() {
        return Ok(vec![]);
    }
    let ignores = Ignores::load(conn)?;

    let mut totals: HashMap<String, i64> = HashMap::new();
    for seg in read_segments(conn, None)? {
        let date = seg.local_date();
        let key = seg.key();
        let title = seg.title();
        if ignores.matches(&key, &title) {
            continue;
        }
        if assigns.resolve(&date, &key, &title).contains(&project_id) {
            *totals.entry(date).or_insert(0) += seg.duration();
        }
    }

    let mut out: Vec<DayTotal> = totals
        .into_iter()
        .map(|(date, seconds)| DayTotal { date, seconds })
        .collect();
    out.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(out)
}

/// Per-day totals for ignored activity (newest day first).
pub fn ignored_breakdown(conn: &Connection) -> rusqlite::Result<Vec<DayTotal>> {
    let ignores = Ignores::load(conn)?;
    if ignores.is_empty() {
        return Ok(vec![]);
    }

    let mut totals: HashMap<String, i64> = HashMap::new();
    for seg in read_segments(conn, None)? {
        let key = seg.key();
        let title = seg.title();
        if ignores.matches(&key, &title) {
            *totals.entry(seg.local_date()).or_insert(0) += seg.duration();
        }
    }

    let mut out: Vec<DayTotal> = totals
        .into_iter()
        .map(|(date, seconds)| DayTotal { date, seconds })
        .collect();
    out.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(out)
}

/// Which apps (and their titles) make up a project's total.
/// Same per-segment resolution as the breakdown.
pub fn project_apps(conn: &Connection, project_id: i64) -> rusqlite::Result<Vec<ProjectApp>> {
    let assigns = Assignments::load(conn)?;
    if assigns.is_empty() {
        return Ok(vec![]);
    }
    let ignores = Ignores::load(conn)?;

    struct AppAcc {
        name: String,
        bundle: Option<String>,
        seconds: i64,
        titles: HashMap<String, TitleAcc>,
    }
    struct TitleAcc {
        seconds: i64,
        can_remove: bool,
    }
    let mut by_app: HashMap<String, AppAcc> = HashMap::new();

    for seg in read_segments(conn, None)? {
        let date = seg.local_date();
        let key = seg.key();
        let title = seg.title();
        if ignores.matches(&key, &title) {
            continue;
        }
        if !assigns.resolve(&date, &key, &title).contains(&project_id) {
            continue;
        }
        let can_remove =
            !title.is_empty() && assigns.links(&date, &key, &title).contains(&project_id);
        let acc = by_app.entry(key).or_insert_with(|| AppAcc {
            name: seg.display_name(),
            bundle: seg.bundle_id.clone(),
            seconds: 0,
            titles: HashMap::new(),
        });
        acc.seconds += seg.duration();
        let title_acc = acc.titles.entry(title).or_insert(TitleAcc {
            seconds: 0,
            can_remove: false,
        });
        title_acc.seconds += seg.duration();
        title_acc.can_remove |= can_remove;
    }

    let mut out: Vec<ProjectApp> = by_app
        .into_iter()
        .map(|(key, acc)| {
            let mut titles: Vec<ProjectTitle> = acc
                .titles
                .into_iter()
                .map(|(title, acc)| ProjectTitle {
                    title,
                    seconds: acc.seconds,
                    can_remove: acc.can_remove,
                })
                .collect();
            titles.sort_by_key(|title| std::cmp::Reverse(title.seconds));
            ProjectApp {
                app_key: key,
                app_name: acc.name,
                bundle_id: acc.bundle,
                seconds: acc.seconds,
                titles,
            }
        })
        .collect();
    out.sort_by_key(|app| std::cmp::Reverse(app.seconds));
    Ok(out)
}

// ---- project period notes -------------------------------------------------

pub fn list_project_period_notes(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<ProjectPeriodNote>> {
    let mut stmt = conn.prepare(
        "SELECT granularity, period_key, note
         FROM project_period_notes
         WHERE project_id = ?1
         ORDER BY granularity ASC, period_key DESC",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(ProjectPeriodNote {
            granularity: r.get(0)?,
            period_key: r.get(1)?,
            note: r.get(2)?,
        })
    })?;
    rows.collect()
}

pub fn set_project_period_note(
    conn: &Connection,
    project_id: i64,
    granularity: &str,
    period_key: &str,
    note: &str,
) -> rusqlite::Result<()> {
    if !matches!(granularity, "day" | "week" | "month") {
        return Err(rusqlite::Error::InvalidQuery);
    }

    let trimmed = note.trim();
    if trimmed.is_empty() {
        conn.execute(
            "DELETE FROM project_period_notes
             WHERE project_id = ?1 AND granularity = ?2 AND period_key = ?3",
            rusqlite::params![project_id, granularity, period_key],
        )?;
    } else {
        conn.execute(
            "INSERT INTO project_period_notes (project_id, granularity, period_key, note)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(project_id, granularity, period_key)
             DO UPDATE SET note = excluded.note",
            rusqlite::params![project_id, granularity, period_key, trimmed],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn mem() -> Connection {
        open(Path::new(":memory:")).unwrap()
    }

    /// Insert a segment at `start`..`end` (absolute unix seconds).
    fn seg(
        conn: &Connection,
        start: i64,
        end: i64,
        bundle: Option<&str>,
        name: Option<&str>,
        title: Option<&str>,
    ) {
        insert_segment(conn, start, end, bundle, name, title).unwrap();
    }

    #[test]
    fn day_total_sums_only_segments_starting_that_day() {
        let conn = open(Path::new(":memory:")).unwrap();
        let start = day_start_ts("2026-01-15");

        // inside the day: 60s + 30s
        insert_segment(&conn, start + 100, start + 160, None, Some("A"), None).unwrap();
        insert_segment(&conn, start + 200, start + 230, None, Some("B"), None).unwrap();
        // previous day and next day: excluded
        insert_segment(&conn, start - 50, start - 10, None, Some("A"), None).unwrap();
        insert_segment(
            &conn,
            start + 86_400 + 5,
            start + 86_400 + 25,
            None,
            Some("A"),
            None,
        )
        .unwrap();

        assert_eq!(day_total_seconds(&conn, "2026-01-15").unwrap(), 90);
        assert_eq!(day_total_seconds(&conn, "2026-01-14").unwrap(), 40);
    }

    #[test]
    fn day_total_excludes_ignored_apps() {
        let conn = mem();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        seg(
            &conn,
            s,
            s + 100,
            Some("com.apple.loginwindow"),
            Some("loginwindow"),
            None,
        );
        seg(&conn, s + 100, s + 160, Some("com.a"), Some("A"), None);
        add_ignored_entry(&conn, "com.apple.loginwindow", Some("loginwindow"), "").unwrap();

        assert_eq!(day_total_seconds(&conn, d).unwrap(), 60);
    }

    #[test]
    fn ignored_entries_can_be_listed_and_removed() {
        let conn = mem();
        add_ignored_entry(&conn, "com.a", Some("App A"), "").unwrap();
        add_ignored_entry(&conn, "com.b", Some("App B"), "noise").unwrap();

        let entries = list_ignored_entries(&conn).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| {
            e.app_key == "com.a" && e.app_name.as_deref() == Some("App A") && e.title.is_empty()
        }));
        assert!(entries
            .iter()
            .any(|e| e.app_key == "com.b" && e.title == "noise"));

        remove_ignored_entry(&conn, "com.b", "noise").unwrap();
        let entries = list_ignored_entries(&conn).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].app_key, "com.a");
    }

    #[test]
    fn ignored_breakdown_totals_matching_ignored_segments() {
        let conn = mem();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("com.a"), Some("A"), Some("keep"));
        seg(
            &conn,
            s + 100,
            s + 140,
            Some("com.a"),
            Some("A"),
            Some("noise"),
        );
        seg(&conn, s + 140, s + 200, Some("com.b"), Some("B"), None);
        add_ignored_entry(&conn, "com.a", Some("A"), "noise").unwrap();
        add_ignored_entry(&conn, "com.b", Some("B"), "").unwrap();

        let rows = ignored_breakdown(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].date, d);
        assert_eq!(rows[0].seconds, 100);
    }

    // ---- day_view ---------------------------------------------------------

    #[test]
    fn day_view_rolls_up_apps_and_titles_sorted_by_time() {
        let conn = mem();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("com.a"), Some("AppA"), Some("x"));
        seg(
            &conn,
            s + 100,
            s + 300,
            Some("com.a"),
            Some("AppA"),
            Some("y"),
        );
        seg(&conn, s + 300, s + 350, Some("com.b"), Some("AppB"), None);

        let view = day_view(&conn, d).unwrap();
        assert_eq!(view.total_seconds, 350);
        // Apps sorted by time desc: A (300) before B (50).
        assert_eq!(view.apps.len(), 2);
        assert_eq!(view.apps[0].app_key, "com.a");
        assert_eq!(view.apps[0].seconds, 300);
        assert_eq!(view.apps[1].app_key, "com.b");
        assert_eq!(view.apps[1].seconds, 50);
        // Titles within A sorted by time desc: y (200) before x (100).
        let titles: Vec<(&str, i64)> = view.apps[0]
            .titles
            .iter()
            .map(|t| (t.title.as_str(), t.seconds))
            .collect();
        assert_eq!(titles, vec![("y", 200), ("x", 100)]);
        // An app with no window title reports a single "" title row.
        assert_eq!(view.apps[1].titles.len(), 1);
        assert_eq!(view.apps[1].titles[0].title, "");
        assert_eq!(view.apps[1].titles[0].seconds, 50);
    }

    #[test]
    fn day_view_uses_name_as_key_when_bundle_missing() {
        let conn = mem();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, None, Some("Finder"), None);

        let view = day_view(&conn, d).unwrap();
        assert_eq!(view.apps[0].app_key, "Finder");
        assert_eq!(view.apps[0].app_name, "Finder");
        assert_eq!(view.apps[0].bundle_id, None);
    }

    #[test]
    fn day_view_excludes_ignored_app_and_title_rules() {
        let conn = mem();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        seg(
            &conn,
            s,
            s + 100,
            Some("com.apple.loginwindow"),
            Some("loginwindow"),
            None,
        );
        seg(
            &conn,
            s + 100,
            s + 170,
            Some("com.a"),
            Some("A"),
            Some("keep"),
        );
        seg(
            &conn,
            s + 170,
            s + 200,
            Some("com.a"),
            Some("A"),
            Some("noise"),
        );
        add_ignored_entry(&conn, "com.apple.loginwindow", Some("loginwindow"), "").unwrap();
        add_ignored_entry(&conn, "com.a", Some("A"), "noise").unwrap();

        let view = day_view(&conn, d).unwrap();
        assert_eq!(view.total_seconds, 70);
        assert_eq!(view.hours.iter().sum::<i64>(), 70);
        assert_eq!(view.apps.len(), 1);
        assert_eq!(view.apps[0].app_key, "com.a");
        assert_eq!(view.apps[0].titles.len(), 1);
        assert_eq!(view.apps[0].titles[0].title, "keep");
    }

    #[test]
    fn day_view_splits_a_segment_across_hour_buckets() {
        let conn = mem();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        // 30 min in hour 0, 30 min in hour 1.
        seg(&conn, s + 1800, s + 5400, Some("com.a"), Some("A"), None);

        let view = day_view(&conn, d).unwrap();
        assert_eq!(view.hours[0], 1800);
        assert_eq!(view.hours[1], 1800);
        assert_eq!(view.hours.iter().sum::<i64>(), 3600);
        assert_eq!(view.apps[0].hours[0], 1800);
        assert_eq!(view.apps[0].hours[1], 1800);
    }

    #[test]
    fn day_view_surfaces_app_level_link_and_explicit_title_link() {
        let conn = mem();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        let work = create_project(&conn, "Work", "#fff").unwrap();
        let side = create_project(&conn, "Side", "#000").unwrap();
        seg(&conn, s, s + 100, Some("com.a"), Some("A"), Some("x"));
        seg(&conn, s + 100, s + 200, Some("com.a"), Some("A"), Some("y"));
        add_assignment(&conn, d, "com.a", "", work.id).unwrap();
        add_assignment(&conn, d, "com.a", "y", side.id).unwrap();

        let view = day_view(&conn, d).unwrap();
        let app = &view.apps[0];
        assert_eq!(app.project_ids, vec![work.id]); // app-level tag
        let tx = app.titles.iter().find(|t| t.title == "x").unwrap();
        let ty = app.titles.iter().find(|t| t.title == "y").unwrap();
        assert!(tx.project_ids.is_empty()); // inherits app-level, no own tag
        assert_eq!(ty.project_ids, vec![side.id]); // explicit title tag
    }

    // ---- project_breakdown ------------------------------------------------

    #[test]
    fn project_breakdown_is_empty_without_assignments() {
        let conn = mem();
        let s = day_start_ts("2026-03-10");
        seg(&conn, s, s + 100, Some("com.a"), Some("A"), None);
        assert!(project_breakdown(&conn, 1).unwrap().is_empty());
    }

    #[test]
    fn project_breakdown_resolves_via_app_level_fallback_per_day() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        let (d1, d2) = ("2026-03-10", "2026-03-11");
        let (s1, s2) = (day_start_ts(d1), day_start_ts(d2));
        // Day 1: two titles, only an app-level assignment -> both count.
        seg(&conn, s1, s1 + 100, Some("com.a"), Some("A"), Some("x"));
        seg(
            &conn,
            s1 + 100,
            s1 + 150,
            Some("com.a"),
            Some("A"),
            Some("y"),
        );
        add_assignment(&conn, d1, "com.a", "", work.id).unwrap();
        // Day 2: only a title-level assignment on x -> only x counts.
        seg(&conn, s2, s2 + 200, Some("com.a"), Some("A"), Some("x"));
        seg(
            &conn,
            s2 + 200,
            s2 + 260,
            Some("com.a"),
            Some("A"),
            Some("y"),
        );
        add_assignment(&conn, d2, "com.a", "x", work.id).unwrap();

        let bd = project_breakdown(&conn, work.id).unwrap();
        // Newest day first.
        assert_eq!(bd.len(), 2);
        assert_eq!(bd[0].date, d2);
        assert_eq!(bd[0].seconds, 200);
        assert_eq!(bd[1].date, d1);
        assert_eq!(bd[1].seconds, 150);
    }

    #[test]
    fn project_breakdown_title_link_overrides_app_level() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        let other = create_project(&conn, "Other", "#000").unwrap();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("com.a"), Some("A"), Some("x"));
        seg(&conn, s + 100, s + 130, Some("com.a"), Some("A"), Some("y"));
        add_assignment(&conn, d, "com.a", "", work.id).unwrap();
        add_assignment(&conn, d, "com.a", "y", other.id).unwrap();

        let work_bd = project_breakdown(&conn, work.id).unwrap();
        assert_eq!(work_bd.len(), 1);
        assert_eq!(work_bd[0].seconds, 100); // only x

        let other_bd = project_breakdown(&conn, other.id).unwrap();
        assert_eq!(other_bd.len(), 1);
        assert_eq!(other_bd[0].seconds, 30); // only y
    }

    #[test]
    fn project_breakdown_excludes_ignored_entries() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("com.a"), Some("A"), Some("keep"));
        seg(
            &conn,
            s + 100,
            s + 150,
            Some("com.a"),
            Some("A"),
            Some("noise"),
        );
        add_assignment(&conn, d, "com.a", "", work.id).unwrap();
        add_ignored_entry(&conn, "com.a", Some("A"), "noise").unwrap();

        let bd = project_breakdown(&conn, work.id).unwrap();
        assert_eq!(bd.len(), 1);
        assert_eq!(bd[0].seconds, 100);
    }

    // ---- project_apps -----------------------------------------------------

    #[test]
    fn project_apps_groups_apps_and_titles() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("com.a"), Some("A"), Some("x"));
        seg(&conn, s + 100, s + 150, Some("com.a"), Some("A"), Some("y"));
        add_assignment(&conn, d, "com.a", "", work.id).unwrap();

        let apps = project_apps(&conn, work.id).unwrap();
        assert_eq!(apps.len(), 1);
        let app = &apps[0];
        assert_eq!(app.app_key, "com.a");
        assert_eq!(app.seconds, 150);
        let tx = app.titles.iter().find(|t| t.title == "x").unwrap();
        assert_eq!(tx.seconds, 100);
        assert!(!tx.can_remove);
        let ty = app.titles.iter().find(|t| t.title == "y").unwrap();
        assert_eq!(ty.seconds, 50);
        assert!(!ty.can_remove);
    }

    #[test]
    fn project_apps_marks_explicit_title_rows_removable() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("com.a"), Some("A"), Some("x"));
        add_assignment(&conn, d, "com.a", "x", work.id).unwrap();

        let apps = project_apps(&conn, work.id).unwrap();
        let title = apps[0].titles.iter().find(|t| t.title == "x").unwrap();
        assert!(title.can_remove);
    }

    #[test]
    fn project_apps_keeps_untitled_title_row() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("com.a"), Some("A"), None);
        add_assignment(&conn, d, "com.a", "", work.id).unwrap();

        let apps = project_apps(&conn, work.id).unwrap();
        let app = &apps[0];
        let untitled = app.titles.iter().find(|t| t.title.is_empty()).unwrap();
        assert_eq!(untitled.seconds, 100);
        assert!(!untitled.can_remove);
    }

    #[test]
    fn project_apps_excludes_ignored_entries() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("com.a"), Some("A"), Some("keep"));
        seg(
            &conn,
            s + 100,
            s + 140,
            Some("com.a"),
            Some("A"),
            Some("noise"),
        );
        add_assignment(&conn, d, "com.a", "", work.id).unwrap();
        add_ignored_entry(&conn, "com.a", Some("A"), "noise").unwrap();

        let apps = project_apps(&conn, work.id).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].seconds, 100);
        assert_eq!(apps[0].titles.len(), 1);
        assert_eq!(apps[0].titles[0].title, "keep");
    }

    // ---- project_period_notes ---------------------------------------------

    #[test]
    fn project_period_notes_can_be_saved_listed_and_deleted() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        set_project_period_note(&conn, work.id, "day", "2026-03-10", "day note").unwrap();
        set_project_period_note(&conn, work.id, "week", "2026-03-09", "week note").unwrap();
        set_project_period_note(&conn, work.id, "month", "2026-03", "month note").unwrap();

        let notes = list_project_period_notes(&conn, work.id).unwrap();
        assert_eq!(notes.len(), 3);
        assert!(notes.iter().any(|n| {
            n.granularity == "day" && n.period_key == "2026-03-10" && n.note == "day note"
        }));
        assert!(notes.iter().any(|n| {
            n.granularity == "week" && n.period_key == "2026-03-09" && n.note == "week note"
        }));
        assert!(notes.iter().any(|n| {
            n.granularity == "month" && n.period_key == "2026-03" && n.note == "month note"
        }));

        set_project_period_note(&conn, work.id, "day", "2026-03-10", "").unwrap();
        let notes = list_project_period_notes(&conn, work.id).unwrap();
        assert_eq!(notes.len(), 2);
        assert!(!notes.iter().any(|n| n.granularity == "day"));
    }

    // ---- Segment ----------------------------------------------------------

    fn segment(bundle: Option<&str>, name: Option<&str>) -> Segment {
        Segment {
            start_ts: 0,
            end_ts: 0,
            bundle_id: bundle.map(str::to_string),
            name: name.map(str::to_string),
            title: None,
        }
    }

    #[test]
    fn segment_key_prefers_bundle_then_name_then_unknown() {
        assert_eq!(segment(Some("com.a"), Some("A")).key(), "com.a");
        assert_eq!(segment(None, Some("A")).key(), "A");
        assert_eq!(segment(None, None).key(), "unknown");
    }

    #[test]
    fn segment_display_name_prefers_name_then_bundle() {
        assert_eq!(segment(Some("com.a"), Some("A")).display_name(), "A");
        assert_eq!(segment(Some("com.a"), None).display_name(), "com.a");
    }

    #[test]
    fn segment_duration_is_clamped_non_negative() {
        let s = Segment {
            start_ts: 100,
            end_ts: 130,
            ..segment(None, None)
        };
        assert_eq!(s.duration(), 30);
        let backwards = Segment {
            start_ts: 130,
            end_ts: 100,
            ..segment(None, None)
        };
        assert_eq!(backwards.duration(), 0);
    }

    #[test]
    fn segment_title_defaults_to_empty_string() {
        assert_eq!(segment(None, None).title(), "");
        let titled = Segment {
            title: Some("doc".into()),
            ..segment(None, None)
        };
        assert_eq!(titled.title(), "doc");
    }

    // ---- Assignments ------------------------------------------------------

    #[test]
    fn assignments_resolve_prefers_title_link_then_app_level() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        let other = create_project(&conn, "Other", "#000").unwrap();
        let d = "2026-03-10";
        add_assignment(&conn, d, "com.a", "", work.id).unwrap();
        add_assignment(&conn, d, "com.a", "y", other.id).unwrap();

        let a = Assignments::load(&conn).unwrap();
        assert_eq!(a.resolve(d, "com.a", "x"), [work.id]); // app-level fallback
        assert_eq!(a.resolve(d, "com.a", "y"), [other.id]); // title tag overrides
        assert!(a.resolve(d, "com.z", "x").is_empty()); // unknown app
    }

    #[test]
    fn assignments_links_are_exact_with_no_fallback() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        let d = "2026-03-10";
        add_assignment(&conn, d, "com.a", "", work.id).unwrap();

        let a = Assignments::load(&conn).unwrap();
        assert_eq!(a.links(d, "com.a", ""), [work.id]); // explicit app-level
        assert!(a.links(d, "com.a", "x").is_empty()); // no fallback to app-level
    }

    #[test]
    fn assignments_load_day_scopes_to_one_date() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        add_assignment(&conn, "2026-03-10", "com.a", "", work.id).unwrap();
        add_assignment(&conn, "2026-03-11", "com.b", "", work.id).unwrap();

        let a = Assignments::load_day(&conn, "2026-03-10").unwrap();
        assert_eq!(a.links("2026-03-10", "com.a", ""), [work.id]);
        assert!(a.links("2026-03-11", "com.b", "").is_empty()); // other day not loaded
    }

    // ---- tag semantics (many-to-many) -------------------------------------

    #[test]
    fn tagging_two_projects_on_same_entry_bills_both_in_full() {
        let conn = mem();
        // Create P_high first so ids are unordered relative to insertion below,
        // proving day_view sorts the project ids.
        let p2 = create_project(&conn, "P2", "#000").unwrap();
        let p1 = create_project(&conn, "P1", "#fff").unwrap();
        let lo = p1.id.min(p2.id);
        let hi = p1.id.max(p2.id);

        let d = "2026-03-10";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("com.a"), Some("A"), Some("x"));
        // Tag the same (date, key, title) with both projects (insert hi first).
        add_assignment(&conn, d, "com.a", "x", hi).unwrap();
        add_assignment(&conn, d, "com.a", "x", lo).unwrap();

        // day_view surfaces both ids, sorted ascending.
        let view = day_view(&conn, d).unwrap();
        let tx = view.apps[0].titles.iter().find(|t| t.title == "x").unwrap();
        assert_eq!(tx.project_ids, vec![lo, hi]);

        // Each project is billed the FULL duration (overlap is by design).
        let bd1 = project_breakdown(&conn, p1.id).unwrap();
        assert_eq!(bd1.len(), 1);
        assert_eq!(bd1[0].seconds, 100);
        let bd2 = project_breakdown(&conn, p2.id).unwrap();
        assert_eq!(bd2.len(), 1);
        assert_eq!(bd2[0].seconds, 100);
    }

    #[test]
    fn remove_assignment_removes_only_the_named_project() {
        let conn = mem();
        let p1 = create_project(&conn, "P1", "#fff").unwrap();
        let p2 = create_project(&conn, "P2", "#000").unwrap();
        let d = "2026-03-10";
        add_assignment(&conn, d, "com.a", "x", p1.id).unwrap();
        add_assignment(&conn, d, "com.a", "x", p2.id).unwrap();

        remove_assignment(&conn, d, "com.a", "x", p1.id).unwrap();

        let a = Assignments::load(&conn).unwrap();
        assert_eq!(a.links(d, "com.a", "x"), [p2.id]); // only p1's tag removed
    }

    #[test]
    fn remove_project_app_assignments_clears_all_app_rows_for_project() {
        let conn = mem();
        let p1 = create_project(&conn, "P1", "#fff").unwrap();
        let p2 = create_project(&conn, "P2", "#000").unwrap();
        let d = "2026-03-10";
        add_assignment(&conn, d, "com.a", "", p1.id).unwrap();
        add_assignment(&conn, d, "com.a", "x", p1.id).unwrap();
        add_assignment(&conn, d, "com.a", "x", p2.id).unwrap();
        add_assignment(&conn, d, "com.b", "", p1.id).unwrap();

        remove_project_app_assignments(&conn, p1.id, "com.a").unwrap();

        let a = Assignments::load(&conn).unwrap();
        assert!(a.links(d, "com.a", "").is_empty());
        assert_eq!(a.links(d, "com.a", "x"), [p2.id]);
        assert_eq!(a.links(d, "com.b", ""), [p1.id]);
    }

    #[test]
    fn remove_project_title_assignments_clears_only_exact_title_rows() {
        let conn = mem();
        let p1 = create_project(&conn, "P1", "#fff").unwrap();
        let p2 = create_project(&conn, "P2", "#000").unwrap();
        let d = "2026-03-10";
        add_assignment(&conn, d, "com.a", "", p1.id).unwrap();
        add_assignment(&conn, d, "com.a", "x", p1.id).unwrap();
        add_assignment(&conn, d, "com.a", "x", p2.id).unwrap();

        remove_project_title_assignments(&conn, p1.id, "com.a", "x").unwrap();

        let a = Assignments::load(&conn).unwrap();
        assert_eq!(a.links(d, "com.a", ""), [p1.id]);
        assert_eq!(a.links(d, "com.a", "x"), [p2.id]);
    }

    #[test]
    fn resolve_title_tags_override_app_level_no_union() {
        let conn = mem();
        let p1 = create_project(&conn, "P1", "#fff").unwrap();
        let p2 = create_project(&conn, "P2", "#000").unwrap();
        let d = "2026-03-10";
        add_assignment(&conn, d, "com.a", "", p1.id).unwrap(); // app-level
        add_assignment(&conn, d, "com.a", "x", p2.id).unwrap(); // title

        let a = Assignments::load(&conn).unwrap();
        // Title's own tag wins outright; the app-level tag is NOT unioned in.
        assert_eq!(a.resolve(d, "com.a", "x"), [p2.id]);
    }

    // ---- migration --------------------------------------------------------

    #[test]
    fn migrate_assignments_multi_preserves_rows_and_enables_tagging() {
        // Build an OLD-schema table by hand: project_id is NOT in the PK.
        let conn = Connection::open(":memory:").unwrap();
        conn.execute_batch(
            "CREATE TABLE day_assignments (
                date       TEXT NOT NULL,
                app_key    TEXT NOT NULL,
                title      TEXT NOT NULL DEFAULT '',
                project_id INTEGER NOT NULL,
                PRIMARY KEY (date, app_key, title)
             );
             INSERT INTO day_assignments (date, app_key, title, project_id)
                VALUES ('2026-03-10', 'com.a', 'x', 1),
                       ('2026-03-10', 'com.a', '', 2);",
        )
        .unwrap();

        // Before: a second project on the same (date, key, title) collides.
        assert!(conn
            .execute(
                "INSERT INTO day_assignments (date, app_key, title, project_id)
                 VALUES ('2026-03-10', 'com.a', 'x', 9)",
                [],
            )
            .is_err());

        migrate_assignments_multi(&conn).unwrap();

        // Existing rows survive the rebuild.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM day_assignments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
        let a = Assignments::load(&conn).unwrap();
        assert_eq!(a.links("2026-03-10", "com.a", "x"), [1]);
        assert_eq!(a.links("2026-03-10", "com.a", ""), [2]);

        // After: a second project for the same key is now insertable.
        add_assignment(&conn, "2026-03-10", "com.a", "x", 9).unwrap();
        let a = Assignments::load(&conn).unwrap();
        assert_eq!(a.links("2026-03-10", "com.a", "x"), [1, 9]);

        // A second migration run is a no-op (schema already new).
        migrate_assignments_multi(&conn).unwrap();
        let a = Assignments::load(&conn).unwrap();
        assert_eq!(a.links("2026-03-10", "com.a", "x"), [1, 9]);
    }

    #[test]
    fn migrate_ancient_schema_title_then_multi_converges() {
        // Simulate the ANCIENT schema: no title column, PK is (date, app_key).
        // This is the shape that existed before migrate_assignments_title was added.
        let conn = Connection::open(":memory:").unwrap();
        conn.execute_batch(
            "CREATE TABLE day_assignments (
                date       TEXT NOT NULL,
                app_key    TEXT NOT NULL,
                project_id INTEGER NOT NULL,
                PRIMARY KEY (date, app_key)
             );
             INSERT INTO day_assignments (date, app_key, project_id)
                VALUES ('2026-01-05', 'com.x', 10),
                       ('2026-01-05', 'com.y', 20);",
        )
        .unwrap();

        // Run migrations in the same order as open().
        migrate_assignments_title(&conn).unwrap();
        migrate_assignments_multi(&conn).unwrap();

        // Both rows survived with title backfilled to ''.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM day_assignments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
        let a = Assignments::load(&conn).unwrap();
        assert_eq!(a.links("2026-01-05", "com.x", ""), [10]);
        assert_eq!(a.links("2026-01-05", "com.y", ""), [20]);

        // The final schema has project_id in the PK, so two projects on the
        // same (date, app_key, title) are insertable (migrate_assignments_multi
        // was effectively a no-op because migrate_assignments_title already
        // wrote the 4-column PK form).
        add_assignment(&conn, "2026-01-05", "com.x", "", 99).unwrap();
        let a = Assignments::load(&conn).unwrap();
        assert_eq!(a.links("2026-01-05", "com.x", ""), [10, 99]);
    }

    // ---- project ordering -------------------------------------------------

    #[test]
    fn list_projects_returns_creation_order() {
        let conn = mem();
        let a = create_project(&conn, "A", "#aaa").unwrap();
        let b = create_project(&conn, "B", "#bbb").unwrap();
        let c = create_project(&conn, "C", "#ccc").unwrap();
        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects.len(), 3);
        assert_eq!(projects[0].id, a.id);
        assert_eq!(projects[1].id, b.id);
        assert_eq!(projects[2].id, c.id);
    }

    #[test]
    fn set_project_order_reorders_projects() {
        let conn = mem();
        let a = create_project(&conn, "A", "#aaa").unwrap();
        let b = create_project(&conn, "B", "#bbb").unwrap();
        let c = create_project(&conn, "C", "#ccc").unwrap();

        set_project_order(&conn, &[c.id, a.id, b.id]).unwrap();

        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects[0].id, c.id);
        assert_eq!(projects[1].id, a.id);
        assert_eq!(projects[2].id, b.id);
    }

    #[test]
    fn new_project_appends_after_reorder() {
        let conn = mem();
        let a = create_project(&conn, "A", "#aaa").unwrap();
        let b = create_project(&conn, "B", "#bbb").unwrap();
        let c = create_project(&conn, "C", "#ccc").unwrap();

        // Reorder to C, A, B.
        set_project_order(&conn, &[c.id, a.id, b.id]).unwrap();

        // Newly created project should appear at the end.
        let d = create_project(&conn, "D", "#ddd").unwrap();
        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects.len(), 4);
        assert_eq!(projects[0].id, c.id);
        assert_eq!(projects[1].id, a.id);
        assert_eq!(projects[2].id, b.id);
        assert_eq!(projects[3].id, d.id);
    }

    #[test]
    fn set_project_order_unknown_id_is_ignored() {
        let conn = mem();
        let a = create_project(&conn, "A", "#aaa").unwrap();
        let b = create_project(&conn, "B", "#bbb").unwrap();

        // Establish a known order first.
        set_project_order(&conn, &[a.id, b.id]).unwrap();

        // Passing only an unknown id should succeed and leave existing order intact.
        set_project_order(&conn, &[999]).unwrap();

        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects[0].id, a.id);
        assert_eq!(projects[1].id, b.id);
    }

    #[test]
    fn migrate_projects_sort_order_backfills_and_is_idempotent() {
        // Build an OLD-schema projects table without sort_order.
        let conn = Connection::open(":memory:").unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (
                id         INTEGER PRIMARY KEY,
                name       TEXT NOT NULL,
                color      TEXT NOT NULL,
                created_at INTEGER NOT NULL
             );
             -- Two rows with distinct created_at.
             INSERT INTO projects (id, name, color, created_at) VALUES (1, 'First',  '#111', 1000);
             INSERT INTO projects (id, name, color, created_at) VALUES (2, 'Second', '#222', 2000);
             INSERT INTO projects (id, name, color, created_at) VALUES (3, 'Third',  '#333', 3000);
             -- Two rows with equal created_at (id tiebreaker: 4 < 5).
             INSERT INTO projects (id, name, color, created_at) VALUES (4, 'TieA',   '#444', 4000);
             INSERT INTO projects (id, name, color, created_at) VALUES (5, 'TieB',   '#555', 4000);",
        )
        .unwrap();

        migrate_projects_sort_order(&conn).unwrap();

        // Now list_projects should work and respect the backfilled order.
        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects.len(), 5);
        assert_eq!(projects[0].name, "First");
        assert_eq!(projects[1].name, "Second");
        assert_eq!(projects[2].name, "Third");
        // TieA (id=4) < TieB (id=5), so TieA is index 3, TieB is index 4.
        assert_eq!(projects[3].name, "TieA");
        assert_eq!(projects[4].name, "TieB");

        // Running the migration again is a no-op — order is unchanged.
        migrate_projects_sort_order(&conn).unwrap();
        let projects2 = list_projects(&conn).unwrap();
        assert_eq!(
            projects.iter().map(|p| p.id).collect::<Vec<_>>(),
            projects2.iter().map(|p| p.id).collect::<Vec<_>>()
        );
    }

    // ---- storage maintenance ----

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    /// A tracked segment + a project assignment + a note + an ignore rule.
    fn seed_all(conn: &Connection, date: &str) {
        let s = day_start_ts(date);
        seg(conn, s, s + 100, Some("com.a"), Some("A"), None);
        let p = create_project(conn, "Work", "#fff").unwrap();
        add_assignment(conn, date, "com.a", "", p.id).unwrap();
        set_project_period_note(conn, p.id, "day", date, "note").unwrap();
        add_ignored_entry(conn, "com.b", Some("B"), "").unwrap();
    }

    #[test]
    fn clear_tracking_data_keeps_projects_and_ignores() {
        let conn = mem();
        seed_all(&conn, "2026-02-01");

        clear_tracking_data(&conn).unwrap();

        assert_eq!(count(&conn, "segments"), 0);
        assert_eq!(count(&conn, "day_assignments"), 0);
        assert_eq!(count(&conn, "project_period_notes"), 0);
        assert_eq!(count(&conn, "projects"), 1); // kept
        assert_eq!(count(&conn, "ignored_entries"), 1); // kept
    }

    #[test]
    fn reset_everything_wipes_all_five_tables() {
        let conn = mem();
        seed_all(&conn, "2026-02-01");

        reset_everything(&conn).unwrap();

        for t in [
            "segments",
            "day_assignments",
            "project_period_notes",
            "projects",
            "ignored_entries",
        ] {
            assert_eq!(count(&conn, t), 0, "table {t} not empty");
        }
    }

    #[test]
    fn clear_untagged_deletes_only_unassigned_segments() {
        let conn = mem();
        let d = "2026-02-02";
        let s = day_start_ts(d);
        // com.a title "doc" is covered by an app-level assignment → tagged, kept.
        seg(&conn, s, s + 100, Some("com.a"), Some("A"), Some("doc"));
        // com.b has no assignment → untagged, removed.
        seg(&conn, s + 100, s + 200, Some("com.b"), Some("B"), None);
        let p = create_project(&conn, "Work", "#fff").unwrap();
        add_assignment(&conn, d, "com.a", "", p.id).unwrap();

        let removed = clear_untagged(&conn).unwrap();

        assert_eq!(removed, 1);
        assert_eq!(count(&conn, "segments"), 1);
        let survivor: String = conn
            .query_row("SELECT app_name FROM segments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(survivor, "A");
    }

    #[test]
    fn untagged_segment_ids_respects_age_cutoff_and_app_level_fallback() {
        let conn = mem();
        let d = "2026-02-03";
        let s = day_start_ts(d);
        seg(&conn, s + 10, s + 20, Some("com.b"), Some("B"), None); // untagged
        seg(&conn, s + 30, s + 40, Some("com.a"), Some("A"), Some("keep")); // app-level tag
        let p = create_project(&conn, "Work", "#fff").unwrap();
        add_assignment(&conn, d, "com.a", "", p.id).unwrap();

        // Whole DB: only com.b resolves to zero projects.
        assert_eq!(untagged_segment_ids(&conn, None).unwrap().len(), 1);
        // Cutoff before both starts: nothing qualifies.
        assert!(untagged_segment_ids(&conn, Some(s)).unwrap().is_empty());
        // Cutoff after com.b's start only: just com.b.
        assert_eq!(untagged_segment_ids(&conn, Some(s + 25)).unwrap().len(), 1);
    }

    // ---- app settings + auto-delete ----

    #[test]
    fn get_and_set_setting_round_trip() {
        let conn = mem();
        assert_eq!(get_setting(&conn, "k").unwrap(), None);
        set_setting(&conn, "k", "v1").unwrap();
        assert_eq!(get_setting(&conn, "k").unwrap(), Some("v1".to_string()));
        set_setting(&conn, "k", "v2").unwrap(); // upsert, not a second row
        assert_eq!(get_setting(&conn, "k").unwrap(), Some("v2".to_string()));
        assert_eq!(count(&conn, "app_settings"), 1);
    }

    #[test]
    fn autodelete_config_defaults_off_30_and_round_trips() {
        let conn = mem();
        let def = get_autodelete_config(&conn).unwrap();
        assert!(!def.enabled);
        assert_eq!(def.days, 30);

        set_autodelete_config(&conn, true, 7).unwrap();
        let cfg = get_autodelete_config(&conn).unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.days, 7);
    }

    #[test]
    fn purge_untagged_older_than_respects_cutoff_and_spares_tagged() {
        let conn = mem();
        let now = Local::now().timestamp();
        let old = now - 40 * 86_400; // 40 days ago
        let recent = now - 86_400; // 1 day ago

        seg(&conn, old, old + 100, Some("com.b"), Some("B"), None); // old + untagged
        seg(&conn, recent, recent + 100, Some("com.c"), Some("C"), None); // recent + untagged
        seg(&conn, old, old + 100, Some("com.a"), Some("A"), None); // old but tagged
        let p = create_project(&conn, "Work", "#fff").unwrap();
        add_assignment(&conn, &local_date_string(old), "com.a", "", p.id).unwrap();

        let removed = purge_untagged_older_than(&conn, 30).unwrap();

        assert_eq!(removed, 1); // only com.b
        let mut stmt = conn
            .prepare("SELECT app_name FROM segments ORDER BY app_name")
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(names, vec!["A".to_string(), "C".to_string()]);
    }
}
