//! SQLite storage. We own this database entirely.
//!
//! Tables:
//!   segments        immutable, append-only ground truth (one row per app focus stretch)
//!   projects        user-created buckets
//!   day_assignments per-(local-day, app) tags linking time to projects, many-to-many
//!                   (drag = "just that day", #7); one app/title can be tagged with
//!                   several projects and each is billed the full duration
//!   ignored_entries app/title rules excluded from activity totals and projects
//!   assignment_rules standing app-or-title-pattern -> project instructions,
//!                   resolved when time is read (ADR 0001, ADR 0003)
//!   assignment_exceptions dated "this is NOT that project" decisions (ADR 0002)
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
/// `projects` are the title's OWN assignments — empty means it inherits the
/// app-level ones. Multiple ids = tagged with several projects.
#[derive(Debug, Serialize)]
pub struct TitleUsage {
    pub title: String, // "" = untitled / app has no window title
    pub seconds: i64,
    pub projects: Vec<RowProject>,
}

/// One app's total time within a day, broken down by window title.
#[derive(Debug, Serialize)]
pub struct AppUsage {
    pub app_key: String,  // bundle id, else name — the stable assignment key
    pub app_name: String, // display name
    pub bundle_id: Option<String>,
    pub seconds: i64,
    pub hours: Vec<i64>, // 24 buckets, seconds per hour of the local day
    pub projects: Vec<RowProject>, // what the app-level row itself says
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
    /// Why this title is in the project — `Direct` when explicit title-level
    /// assignments exist to delete, `Rule` when a standing rule is responsible.
    /// Never `Excluded`: an excluded title is not in the project at all.
    pub state: LinkState,
    pub rule_id: Option<i64>,
}

/// One app contributing time to a project (for the project view's app breakdown).
#[derive(Debug, Serialize)]
pub struct ProjectApp {
    pub app_key: String,
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub seconds: i64,
    /// Why this app is in the project, at the app level. `Rule` means the
    /// remove action belongs on the rule, not on per-day rows.
    pub state: LinkState,
    pub rule_id: Option<i64>,
    pub titles: Vec<ProjectTitle>,
}

/// A standing instruction that an app's time belongs to a project (ADR 0001).
/// `effective_from` is a *gate*: `None` reaches all history, `Some(date)` holds
/// only from that local date onward. Subject is the app plus, optionally, a
/// *title pattern*: `""` is an app rule, anything else matches a window title
/// case-insensitively by substring and outranks the app rules (ADR 0003).
/// Mirrors `AssignmentRule` in src/api.ts; keep the two in sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssignmentRule {
    pub id: i64,
    pub project_id: i64,
    pub app_key: String,
    pub app_name: Option<String>,
    pub title: String, // "" = the whole app; else a case-insensitive substring
    pub effective_from: Option<String>,
}

/// One window title an app has actually shown, offered to seed a rule's title
/// pattern. Mirrors `TrackedTitle` in src/api.ts; keep the two in sync.
#[derive(Debug, Serialize)]
pub struct TrackedTitle {
    pub title: String,
    pub seconds: i64,
}

/// An app that has been tracked, offered as a subject when writing a rule.
/// Mirrors `TrackedApp` in src/api.ts; keep the two in sync.
#[derive(Debug, Serialize)]
pub struct TrackedApp {
    pub app_key: String,
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub seconds: i64,
}

/// One line of a project's per-day receipt: what made up this day, and why.
/// Mirrors `ReceiptEntry` in src/api.ts; keep the two in sync.
#[derive(Debug, Serialize)]
pub struct ReceiptEntry {
    pub app_key: String,
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub title: String, // "" = untitled
    pub seconds: i64,
    pub state: LinkState,
    pub rule_id: Option<i64>,
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
    migrate_add_rule_tables(&conn)?;
    migrate_assignment_rules_title(&conn)?;
    Ok(conn)
}

/// DBs predating assignment rules have neither table. Both are additive — no
/// existing row moves — so creating them is the whole migration.
fn migrate_add_rule_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "-- Standing 'this app is always that project' instructions (ADR 0001).
        -- Resolved when time is read; never written into day_assignments.
        CREATE TABLE IF NOT EXISTS assignment_rules (
            id             INTEGER PRIMARY KEY,
            project_id     INTEGER NOT NULL,
            app_key        TEXT NOT NULL,
            app_name       TEXT,           -- display only; the key is what matches
            title          TEXT NOT NULL DEFAULT '',  -- '' = whole app; else a
                                           -- case-insensitive substring (ADR 0003)
            effective_from TEXT,           -- NULL = reaches all history
            created_at     INTEGER NOT NULL
        );

        -- Dated 'this is NOT that project' decisions (ADR 0002). Mutually
        -- exclusive with a day_assignments row on the same key.
        CREATE TABLE IF NOT EXISTS assignment_exceptions (
            date       TEXT NOT NULL,   -- local 'YYYY-MM-DD'
            app_key    TEXT NOT NULL,
            title      TEXT NOT NULL DEFAULT '',  -- '' = the app-level row
            project_id INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (date, app_key, title, project_id)
        );",
    )
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

/// Rule tables predating title patterns (ADR 0003) key rules by app alone.
/// Existing rows become app rules, which is what they already were.
fn migrate_assignment_rules_title(conn: &Connection) -> rusqlite::Result<()> {
    let mut has_title = false;
    {
        let mut stmt = conn.prepare("PRAGMA table_info(assignment_rules)")?;
        let cols = stmt.query_map([], |r| r.get::<_, String>(1))?;
        for c in cols {
            if c? == "title" {
                has_title = true;
            }
        }
    }
    if !has_title {
        conn.execute_batch(
            "ALTER TABLE assignment_rules ADD COLUMN title TEXT NOT NULL DEFAULT '';",
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
    conn.execute("DELETE FROM assignment_rules WHERE project_id = ?1", [id])?;
    conn.execute(
        "DELETE FROM assignment_exceptions WHERE project_id = ?1",
        [id],
    )?;
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
///
/// Clears any exception on the same key: a key holds a yes or a no, never both
/// (ADR 0002), so assigning is also how a recorded "no" is taken back.
pub fn add_assignment(
    conn: &Connection,
    date: &str,
    app_key: &str,
    title: &str,
    project_id: i64,
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM assignment_exceptions
         WHERE date = ?1 AND app_key = ?2 AND title = ?3 AND project_id = ?4",
        rusqlite::params![date, app_key, title, project_id],
    )?;
    tx.execute(
        "INSERT INTO day_assignments (date, app_key, title, project_id) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(date, app_key, title, project_id) DO NOTHING",
        rusqlite::params![date, app_key, title, project_id],
    )?;
    tx.commit()
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

/// Take an app — or one of its titles — out of a project entirely, from the
/// project view's folded all-day rows.
///
/// Deletes the assignments that put it there and then, for every day where it
/// would still be inherited from an app-level assignment or a standing rule,
/// records an exception. Removal means the same thing here as it does on a day
/// row: take back what put it there, and stop it coming back (ADR 0002). The
/// rule itself is left alone — it is a separate object with its own controls,
/// and deleting it would un-bill every other app it covers.
///
/// `title = None` removes the whole app; `Some(t)` removes just that title.
/// Returns how many days had to be excepted, so the interface can say whether
/// a rule is still standing behind the row.
pub fn remove_from_project(
    conn: &Connection,
    project_id: i64,
    app_key: &str,
    title: Option<&str>,
) -> rusqlite::Result<usize> {
    let tx = conn.unchecked_transaction()?;
    match title {
        Some(title) => tx.execute(
            "DELETE FROM day_assignments
             WHERE project_id = ?1 AND app_key = ?2 AND title = ?3",
            rusqlite::params![project_id, app_key, title],
        )?,
        None => tx.execute(
            "DELETE FROM day_assignments WHERE project_id = ?1 AND app_key = ?2",
            rusqlite::params![project_id, app_key],
        )?,
    };

    // Whatever the assignments no longer cover, inheritance might. Walk the
    // segments once and veto the days that still bill.
    let res = Resolver::load(&tx)?;
    let ignores = Ignores::load(&tx)?;
    let mut vetoed: HashSet<(String, String)> = HashSet::new();
    for seg in read_segments(&tx, None)? {
        let key = seg.key();
        if key != app_key {
            continue;
        }
        let seg_title = seg.title();
        if title.is_some_and(|t| t != seg_title) || ignores.matches(&key, &seg_title) {
            continue;
        }
        let date = seg.local_date();
        if !res.bills(&date, &key, &seg_title, project_id) {
            continue;
        }
        // An app-scope removal vetoes the app row, which covers every title
        // under it; a title-scope removal vetoes only that title's row.
        let veto_title = title.unwrap_or("").to_string();
        vetoed.insert((date, veto_title));
    }

    for (date, veto_title) in &vetoed {
        tx.execute(
            "INSERT INTO assignment_exceptions (date, app_key, title, project_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(date, app_key, title, project_id) DO NOTHING",
            rusqlite::params![
                date,
                app_key,
                veto_title,
                project_id,
                Local::now().timestamp()
            ],
        )?;
    }
    let n = vetoed.len();
    tx.commit()?;
    Ok(n)
}

// ---- assignment rules -----------------------------------------------------

/// Create a standing rule sending an app's time to a project. `effective_from`
/// is `None` for a retroactive rule (the default) or a local 'YYYY-MM-DD' for a
/// forward-only one. `app_name` is display only — the key is what matches.
/// Rules are additive: two rules may name the same app and different projects,
/// and both bill.
pub fn create_rule(
    conn: &Connection,
    project_id: i64,
    app_key: &str,
    app_name: Option<&str>,
    title: &str,
    effective_from: Option<&str>,
) -> rusqlite::Result<AssignmentRule> {
    // Surrounding whitespace is never what the user meant to match on, and a
    // blank pattern is how an app rule is written.
    let title = title.trim();
    conn.execute(
        "INSERT INTO assignment_rules
            (project_id, app_key, app_name, title, effective_from, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            project_id,
            app_key,
            app_name,
            title,
            effective_from,
            Local::now().timestamp()
        ],
    )?;
    Ok(AssignmentRule {
        id: conn.last_insert_rowid(),
        project_id,
        app_key: app_key.to_string(),
        app_name: app_name.map(str::to_string),
        title: title.to_string(),
        effective_from: effective_from.map(str::to_string),
    })
}

pub fn list_rules(conn: &Connection) -> rusqlite::Result<Vec<AssignmentRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, app_key, app_name, title, effective_from
         FROM assignment_rules ORDER BY id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(AssignmentRule {
            id: r.get(0)?,
            project_id: r.get(1)?,
            app_key: r.get(2)?,
            app_name: r.get(3)?,
            title: r.get(4)?,
            effective_from: r.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn delete_rule(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM assignment_rules WHERE id = ?1", [id])?;
    Ok(())
}

// ---- exceptions -----------------------------------------------------------

/// Write the two statements behind an exception without owning a transaction.
/// `exclude_for_day` owns the whole gesture's transaction; test fixtures may
/// call this directly on their isolated in-memory connection.
fn write_exception(
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
    conn.execute(
        "INSERT INTO assignment_exceptions (date, app_key, title, project_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(date, app_key, title, project_id) DO NOTHING",
        rusqlite::params![date, app_key, title, project_id, Local::now().timestamp()],
    )?;
    Ok(())
}

/// Clear a recorded "no", returning the row to Undecided so rules apply again.
pub fn remove_exception(
    conn: &Connection,
    date: &str,
    app_key: &str,
    title: &str,
    project_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM assignment_exceptions
         WHERE date = ?1 AND app_key = ?2 AND title = ?3 AND project_id = ?4",
        rusqlite::params![date, app_key, title, project_id],
    )?;
    Ok(())
}

/// Take a row out of a project for one day — the gesture behind clicking a
/// project dot (ADR 0002). Deletes whatever assignment put it there, then
/// records an exception *only if* the row would still be inherited from an
/// app-level assignment or a standing rule. So an ordinary hand-made link just
/// disappears, storing nothing, exactly as it did before rules existed, while
/// a rule-covered row lands on Excluded instead of springing back.
pub fn exclude_for_day(
    conn: &Connection,
    date: &str,
    app_key: &str,
    title: &str,
    project_id: i64,
) -> rusqlite::Result<()> {
    // One transaction: if the exception write failed after the assignment was
    // already deleted, a rule-covered row would spring straight back to
    // Included — the exact failure ADR 0002 exists to prevent.
    let tx = conn.unchecked_transaction()?;
    remove_assignment(&tx, date, app_key, title, project_id)?;
    if Resolver::load_day(&tx, date)?.bills(date, app_key, title, project_id) {
        write_exception(&tx, date, app_key, title, project_id)?;
    }
    tx.commit()
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

/// How a project came to be attached to a row, as shown in the UI.
/// `Excluded` is not a link at all — it is a recorded decision that the row is
/// *not* in the project (see ADR 0002), carried alongside the links so the
/// interface can tell a deliberate "no" apart from an unreviewed gap.
/// `Inherited` belongs to the project view — both its folded all-day rows and
/// its per-day receipts — where a row is in the project solely because an
/// app-level assignment covers it. Activity rows render inheritance by absence
/// instead, so `row_projects` never emits it; conversely the project view
/// never emits `Excluded`, because an excluded row is not in the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkState {
    Direct,
    Rule,
    Inherited,
    Excluded,
}

/// One project's standing against a row, and why it holds.
/// Mirrors `RowProject` in src/api.ts; keep the two in sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RowProject {
    pub project_id: i64,
    pub state: LinkState,
    /// The rule responsible, when `state` is `Rule` — lets the UI name it.
    pub rule_id: Option<i64>,
}

/// Everything needed to decide which projects a segment bills to, loaded once
/// and consulted in one place. Resolution walks the four rungs of ADR 0001:
/// `day-title > day-app > rule-title > rule-app`, stopping at the first that
/// speaks. A title rule therefore *shadows* the app rules of the same app for
/// the titles it matches, rather than stacking with them (ADR 0003).
///
/// A row's own statements come in two polarities that are mutually exclusive
/// per project: an assignment (yes) and an exception (no). Writing one clears
/// the other, so a key never holds both.
struct Resolver {
    /// (date, app_key, title) → projects assigned, ascending.
    assigned: HashMap<(String, String, String), Vec<i64>>,
    /// (date, app_key, title) → projects excepted, ascending.
    excepted: HashMap<(String, String, String), Vec<i64>>,
    /// Date-less standing app rules (rung four) grouped by subject app. Each
    /// group is deduped so one project bills once.
    app_rules: HashMap<String, Vec<AssignmentRule>>,
    /// Date-less standing title rules (rung three) grouped by subject app,
    /// each paired with its pattern pre-lowercased for matching.
    title_rules: HashMap<String, Vec<(String, AssignmentRule)>>,
}

impl Resolver {
    /// Everything — project rollups span all days.
    fn load(conn: &Connection) -> rusqlite::Result<Self> {
        Self::build(
            conn.prepare("SELECT date, app_key, title, project_id FROM day_assignments")?
                .query_map([], Self::row)?,
            conn.prepare("SELECT date, app_key, title, project_id FROM assignment_exceptions")?
                .query_map([], Self::row)?,
            list_rules(conn)?,
        )
    }

    /// Just one day — the day view never looks outside it.
    fn load_day(conn: &Connection, date: &str) -> rusqlite::Result<Self> {
        Self::build(
            conn.prepare(
                "SELECT date, app_key, title, project_id FROM day_assignments WHERE date = ?1",
            )?
            .query_map([date], Self::row)?,
            conn.prepare(
                "SELECT date, app_key, title, project_id FROM assignment_exceptions
                 WHERE date = ?1",
            )?
            .query_map([date], Self::row)?,
            list_rules(conn)?,
        )
    }

    fn row(r: &rusqlite::Row) -> rusqlite::Result<(String, String, String, i64)> {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    }

    fn build<A, E>(assigned: A, excepted: E, rules: Vec<AssignmentRule>) -> rusqlite::Result<Self>
    where
        A: Iterator<Item = rusqlite::Result<(String, String, String, i64)>>,
        E: Iterator<Item = rusqlite::Result<(String, String, String, i64)>>,
    {
        // Rules are grouped by subject once, so the hot path — resolving every
        // segment in the database — is a single map probe rather than a scan,
        // sort and dedup of every rule per segment. The two rungs are split
        // here too, so resolving never re-partitions them per segment.
        let mut by_app: HashMap<String, Vec<AssignmentRule>> = HashMap::new();
        let mut by_title: HashMap<String, Vec<(String, AssignmentRule)>> = HashMap::new();
        for rule in rules {
            if rule.title.is_empty() {
                by_app.entry(rule.app_key.clone()).or_default().push(rule);
            } else {
                by_title
                    .entry(rule.app_key.clone())
                    .or_default()
                    .push((rule.title.to_lowercase(), rule));
            }
        }
        for group in by_app.values_mut() {
            group.sort_by_key(|rule| (rule.project_id, rule.id));
            // Several rules may name the same project; bill it once.
            group.dedup_by_key(|rule| rule.project_id);
        }
        for group in by_title.values_mut() {
            group.sort_by(|a, b| (&a.0, a.1.project_id, a.1.id).cmp(&(&b.0, b.1.project_id, b.1.id)));
            // Same pattern, same project, twice over: bill it once.
            group.dedup_by(|a, b| a.0 == b.0 && a.1.project_id == b.1.project_id);
        }
        Ok(Self {
            assigned: Self::collect(assigned)?,
            excepted: Self::collect(excepted)?,
            app_rules: by_app,
            title_rules: by_title,
        })
    }

    fn collect<I>(rows: I) -> rusqlite::Result<HashMap<(String, String, String), Vec<i64>>>
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
        Ok(by_key)
    }

    /// True when nothing anywhere could attach a project to a segment, letting
    /// the project rollups skip reading segments at all.
    fn is_empty(&self) -> bool {
        self.assigned.is_empty() && self.app_rules.is_empty() && self.title_rules.is_empty()
    }

    fn lookup<'a>(
        map: &'a HashMap<(String, String, String), Vec<i64>>,
        date: &str,
        app_key: &str,
        title: &str,
    ) -> &'a [i64] {
        map.get(&(date.to_string(), app_key.to_string(), title.to_string()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The projects assigned to exactly this row — no inheritance, no rules.
    fn assigned_to(&self, date: &str, app_key: &str, title: &str) -> &[i64] {
        Self::lookup(&self.assigned, date, app_key, title)
    }

    /// The projects this row has been excepted from — no inheritance.
    fn excepted_from(&self, date: &str, app_key: &str, title: &str) -> &[i64] {
        Self::lookup(&self.excepted, date, app_key, title)
    }

    /// Is this rule's gate open on `date`? A rule with no `effective_from`
    /// reaches all history (ADR 0001).
    fn gate_open(rule: &AssignmentRule, date: &str) -> bool {
        rule.effective_from
            .as_deref()
            .is_none_or(|from| date >= from)
    }

    fn rule_link(rule: &AssignmentRule) -> RowProject {
        RowProject {
            project_id: rule.project_id,
            state: LinkState::Rule,
            rule_id: Some(rule.id),
        }
    }

    /// Rung four: standing rules whose subject is the whole app.
    fn app_rule_links(&self, date: &str, app_key: &str) -> Vec<RowProject> {
        let Some(group) = self.app_rules.get(app_key) else {
            return Vec::new();
        };
        group
            .iter()
            .filter(|rule| Self::gate_open(rule, date))
            .map(Self::rule_link)
            .collect()
    }

    /// Rung three: standing rules whose subject is a title pattern of this app.
    /// Matching is case-insensitive and partial, because window titles are
    /// transient (ADR 0003). A pattern is never empty, so an untitled segment
    /// matches nothing here and falls through to the app rules.
    fn title_rule_links(&self, date: &str, app_key: &str, title: &str) -> Vec<RowProject> {
        let Some(group) = self.title_rules.get(app_key) else {
            return Vec::new();
        };
        // Only allocated for apps that actually carry a title rule.
        let haystack = title.to_lowercase();
        group
            .iter()
            .filter(|(needle, rule)| haystack.contains(needle) && Self::gate_open(rule, date))
            .map(|(_, rule)| Self::rule_link(rule))
            .collect()
    }

    /// The projects a segment bills to, walking the rungs of ADR 0001. The
    /// title's own assignments answer outright; failing that the app-level ones
    /// answer, less anything the title was excepted from; failing that the
    /// rules answer, less anything either row was excepted from.
    fn resolve(&self, date: &str, app_key: &str, title: &str) -> Vec<RowProject> {
        let own = self.assigned_to(date, app_key, title);
        if !own.is_empty() {
            return direct_links(own);
        }

        let title_vetoes = self.excepted_from(date, app_key, title);
        let app_level = self.assigned_to(date, app_key, "");
        if !app_level.is_empty() {
            let mut links = direct_links(app_level);
            links.retain(|link| !title_vetoes.contains(&link.project_id));
            return links;
        }

        let app_vetoes = self.excepted_from(date, app_key, "");
        // Rung three speaks *instead of* rung four, never alongside it.
        let mut links = self.title_rule_links(date, app_key, title);
        if links.is_empty() {
            links = self.app_rule_links(date, app_key);
        }
        links.retain(|link| {
            !title_vetoes.contains(&link.project_id) && !app_vetoes.contains(&link.project_id)
        });
        links
    }

    /// Does this segment bill to `project_id`?
    fn bills(&self, date: &str, app_key: &str, title: &str, project_id: i64) -> bool {
        self.resolve(date, app_key, title)
            .iter()
            .any(|link| link.project_id == project_id)
    }

    /// What the day view draws against a row: the projects the row itself
    /// speaks for, plus the ones it has been excepted from. Inheritance is
    /// rendered by *absence*, exactly as before rules existed — each row
    /// additionally shows the rules whose subject *is* that row: app rules on
    /// the app row, title rules on the title rows they match. Without the
    /// latter a shadowed title would draw nothing while billing elsewhere.
    ///
    /// A project appears at most once. An exception must *replace* the link it
    /// cancels rather than sit beside it, or Excluded would render as Included
    /// and the dot would stop responding (ADR 0002: the three states must never
    /// render alike).
    fn row_projects(&self, date: &str, app_key: &str, title: &str) -> Vec<RowProject> {
        let vetoes = self.excepted_from(date, app_key, title);
        let mut out = direct_links(self.assigned_to(date, app_key, title));
        if out.is_empty() {
            out = if title.is_empty() {
                self.app_rule_links(date, app_key)
            } else {
                self.title_rule_links(date, app_key, title)
            };
        }
        out.retain(|link| !vetoes.contains(&link.project_id));
        for &project_id in vetoes {
            out.push(RowProject {
                project_id,
                state: LinkState::Excluded,
                rule_id: None,
            });
        }
        out.sort_by_key(|link| link.project_id);
        out
    }

    /// Why `project_id` holds this segment — as the title row sees it, and as
    /// the app row sees it. `None` when the segment does not bill to the
    /// project at all. Both come from one resolution pass, and live here so
    /// provenance is never re-derived from the raw tables at a call site.
    fn origins_for(
        &self,
        date: &str,
        app_key: &str,
        title: &str,
        project_id: i64,
    ) -> Option<(Origin, Origin)> {
        let link = self
            .resolve(date, app_key, title)
            .into_iter()
            .find(|link| link.project_id == project_id)?;
        Some((
            Origin::of(&link, self.assigned_to(date, app_key, title), project_id),
            Origin::of(&link, self.assigned_to(date, app_key, ""), project_id),
        ))
    }
}

fn direct_links(ids: &[i64]) -> Vec<RowProject> {
    ids.iter()
        .map(|&project_id| RowProject {
            project_id,
            state: LinkState::Direct,
            rule_id: None,
        })
        .collect()
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
/// the same `Resolver::resolve` used everywhere else, so a title covered only
/// by an app-level tag — or by a standing rule — is NOT untagged, while a row
/// excepted from every project it would otherwise bill to IS.
fn untagged_segment_ids(conn: &Connection, before: Option<i64>) -> rusqlite::Result<Vec<i64>> {
    let res = Resolver::load(conn)?;
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
        if res
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

/// Clear all *tracking* data — segments, the per-day judgements about them, and
/// period notes — while keeping user-defined projects, standing rules and
/// ignore rules. Exceptions go with the assignments: they are the two
/// polarities of one dated statement (ADR 0002) and must not diverge.
pub fn clear_tracking_data(conn: &Connection) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM segments", [])?;
    tx.execute("DELETE FROM day_assignments", [])?;
    tx.execute("DELETE FROM assignment_exceptions", [])?;
    tx.execute("DELETE FROM project_period_notes", [])?;
    tx.commit()
}

/// Wipe every data table. Used by "Reset everything". Leaves `app_settings`
/// (theme lives in localStorage; auto-delete config is intentionally preserved
/// so a reset doesn't silently re-enable/disable purging).
///
/// Rules and exceptions reference projects by id, and SQLite reuses rowids —
/// leaving them behind would silently graft the old project's rules onto the
/// next project created.
pub fn reset_everything(conn: &Connection) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM segments", [])?;
    tx.execute("DELETE FROM day_assignments", [])?;
    tx.execute("DELETE FROM assignment_exceptions", [])?;
    tx.execute("DELETE FROM assignment_rules", [])?;
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
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        [key],
        |r| r.get(0),
    )
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
    let res = Resolver::load_day(conn, date)?;
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
                    projects: res.row_projects(date, &key, &title),
                    title,
                    seconds,
                })
                .collect();
            titles.sort_by_key(|title| std::cmp::Reverse(title.seconds));
            AppUsage {
                projects: res.row_projects(date, &key, ""),
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
    let res = Resolver::load(conn)?;
    if res.is_empty() {
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
        if res.bills(&date, &key, &title, project_id) {
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
    let res = Resolver::load(conn)?;
    if res.is_empty() {
        return Ok(vec![]);
    }
    let ignores = Ignores::load(conn)?;

    struct AppAcc {
        name: String,
        bundle: Option<String>,
        seconds: i64,
        origin: Origin,
        titles: HashMap<String, TitleAcc>,
    }
    struct TitleAcc {
        seconds: i64,
        origin: Origin,
    }
    let mut by_app: HashMap<String, AppAcc> = HashMap::new();

    for seg in read_segments(conn, None)? {
        let date = seg.local_date();
        let key = seg.key();
        let title = seg.title();
        if ignores.matches(&key, &title) {
            continue;
        }
        let Some((title_origin, app_origin)) = res.origins_for(&date, &key, &title, project_id)
        else {
            continue;
        };

        let acc = by_app.entry(key).or_insert_with(|| AppAcc {
            name: seg.display_name(),
            bundle: seg.bundle_id.clone(),
            seconds: 0,
            origin: Origin::default(),
            titles: HashMap::new(),
        });
        acc.seconds += seg.duration();
        acc.origin = acc.origin.strongest(app_origin);
        let title_acc = acc.titles.entry(title).or_insert(TitleAcc {
            seconds: 0,
            origin: Origin::default(),
        });
        title_acc.seconds += seg.duration();
        title_acc.origin = title_acc.origin.strongest(title_origin);
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
                    state: acc.origin.state,
                    rule_id: acc.origin.rule_id,
                })
                .collect();
            titles.sort_by_key(|title| std::cmp::Reverse(title.seconds));
            ProjectApp {
                app_key: key,
                app_name: acc.name,
                bundle_id: acc.bundle,
                seconds: acc.seconds,
                state: acc.origin.state,
                rule_id: acc.origin.rule_id,
                titles,
            }
        })
        .collect();
    out.sort_by_key(|app| std::cmp::Reverse(app.seconds));
    Ok(out)
}

/// Why an *aggregate* project row (all days folded together) is in the project,
/// and therefore what acting on it should do. Folding many days into one row
/// needs a winner: something directly assigned outranks a rule, because there
/// are rows to delete; a rule outranks bare inheritance, because there is at
/// least a rule to edit.
#[derive(Debug, Clone, Copy)]
struct Origin {
    state: LinkState,
    rule_id: Option<i64>,
}

impl Default for Origin {
    fn default() -> Self {
        Self {
            state: LinkState::Inherited,
            rule_id: None,
        }
    }
}

impl Origin {
    /// `assigned` is what the row in question says for *itself*. When it names
    /// the project there is an explicit record to delete; otherwise the row is
    /// only along for the ride — via a standing rule if one is responsible,
    /// else by inheriting an assignment made further up.
    fn of(link: &RowProject, assigned: &[i64], project_id: i64) -> Self {
        if assigned.contains(&project_id) {
            Self {
                state: LinkState::Direct,
                rule_id: None,
            }
        } else if link.state == LinkState::Rule {
            Self {
                state: LinkState::Rule,
                rule_id: link.rule_id,
            }
        } else {
            Self::default()
        }
    }

    fn rank(self) -> u8 {
        match self.state {
            LinkState::Direct => 3,
            LinkState::Rule => 2,
            LinkState::Inherited => 1,
            LinkState::Excluded => 0,
        }
    }

    fn strongest(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

/// One day of a project, decomposed into the activity that made it up and the
/// reason each line is there. This is the receipt behind a day's total: after
/// rules, a project can contain time nobody linked by hand, and a number with
/// no way to interrogate it is not reviewable (ADR 0002).
pub fn project_day_entries(
    conn: &Connection,
    project_id: i64,
    date: &str,
) -> rusqlite::Result<Vec<ReceiptEntry>> {
    let res = Resolver::load_day(conn, date)?;
    if res.is_empty() {
        return Ok(vec![]);
    }
    let ignores = Ignores::load(conn)?;
    let start = day_start_ts(date);

    let mut by_row: HashMap<(String, String), ReceiptEntry> = HashMap::new();
    for seg in read_segments(conn, Some((start, start + 86_400)))? {
        let key = seg.key();
        let title = seg.title();
        if ignores.matches(&key, &title) {
            continue;
        }
        let Some((origin, _)) = res.origins_for(date, &key, &title, project_id) else {
            continue;
        };
        let entry = by_row
            .entry((key.clone(), title.clone()))
            .or_insert_with(|| ReceiptEntry {
                app_key: key,
                app_name: seg.display_name(),
                bundle_id: seg.bundle_id.clone(),
                title,
                seconds: 0,
                state: origin.state,
                rule_id: origin.rule_id,
            });
        entry.seconds += seg.duration();
    }

    let mut out: Vec<ReceiptEntry> = by_row.into_values().collect();
    out.sort_by(|a, b| {
        b.seconds
            .cmp(&a.seconds)
            .then_with(|| a.title.cmp(&b.title))
    });
    Ok(out)
}

/// Every app that has been tracked, busiest first — the candidates for a rule.
/// Ignored apps are left out: they are not activity, so they cannot belong to
/// a project.
pub fn tracked_apps(conn: &Connection) -> rusqlite::Result<Vec<TrackedApp>> {
    let ignores = Ignores::load(conn)?;
    let mut by_key: HashMap<String, TrackedApp> = HashMap::new();
    for seg in read_segments(conn, None)? {
        let key = seg.key();
        let title = seg.title();
        if ignores.matches(&key, &title) {
            continue;
        }
        let entry = by_key.entry(key.clone()).or_insert_with(|| TrackedApp {
            app_key: key,
            app_name: seg.display_name(),
            bundle_id: seg.bundle_id.clone(),
            seconds: 0,
        });
        entry.seconds += seg.duration();
    }
    let mut out: Vec<TrackedApp> = by_key.into_values().collect();
    out.sort_by(|a, b| {
        b.seconds
            .cmp(&a.seconds)
            .then_with(|| a.app_name.cmp(&b.app_name))
    });
    Ok(out)
}

/// Every window title one app has shown, busiest first — the candidates for a
/// rule's title pattern. Untitled time is left out: a pattern is never empty,
/// so it could not match it anyway. Ignored time is left out for the same
/// reason as in `tracked_apps`.
pub fn tracked_titles(conn: &Connection, app_key: &str) -> rusqlite::Result<Vec<TrackedTitle>> {
    let ignores = Ignores::load(conn)?;
    let mut by_title: HashMap<String, i64> = HashMap::new();
    for seg in read_segments(conn, None)? {
        let key = seg.key();
        if key != app_key {
            continue;
        }
        let title = seg.title();
        if title.is_empty() || ignores.matches(&key, &title) {
            continue;
        }
        *by_title.entry(title).or_insert(0) += seg.duration();
    }
    let mut out: Vec<TrackedTitle> = by_title
        .into_iter()
        .map(|(title, seconds)| TrackedTitle { title, seconds })
        .collect();
    out.sort_by(|a, b| b.seconds.cmp(&a.seconds).then_with(|| a.title.cmp(&b.title)));
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

    /// Just the project ids from a resolution, for assertions that care about
    /// *which* projects rather than why.
    fn ids(links: &[RowProject]) -> Vec<i64> {
        links.iter().map(|link| link.project_id).collect()
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
        assert_eq!(ids(&app.projects), vec![work.id]); // app-level tag
        let tx = app.titles.iter().find(|t| t.title == "x").unwrap();
        let ty = app.titles.iter().find(|t| t.title == "y").unwrap();
        assert!(tx.projects.is_empty()); // inherits app-level, no own tag
        assert_eq!(ids(&ty.projects), vec![side.id]); // explicit title tag
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
        assert_eq!(tx.state, LinkState::Inherited);
        let ty = app.titles.iter().find(|t| t.title == "y").unwrap();
        assert_eq!(ty.seconds, 50);
        assert_eq!(ty.state, LinkState::Inherited);
    }

    #[test]
    fn project_apps_reports_explicit_title_rows_as_direct() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        let d = "2026-03-10";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("com.a"), Some("A"), Some("x"));
        add_assignment(&conn, d, "com.a", "x", work.id).unwrap();

        let apps = project_apps(&conn, work.id).unwrap();
        let title = apps[0].titles.iter().find(|t| t.title == "x").unwrap();
        assert_eq!(title.state, LinkState::Direct);
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
        // The untitled row *is* the app-level row, so it reports the app-level
        // assignment directly; the interface, not the data, suppresses a
        // separate remove control for it.
        assert_eq!(untitled.state, LinkState::Direct);
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

        let a = Resolver::load(&conn).unwrap();
        assert_eq!(ids(&a.resolve(d, "com.a", "x")), [work.id]); // app-level fallback
        assert_eq!(ids(&a.resolve(d, "com.a", "y")), [other.id]); // title tag overrides
        assert!(ids(&a.resolve(d, "com.z", "x")).is_empty()); // unknown app
    }

    #[test]
    fn assignments_links_are_exact_with_no_fallback() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        let d = "2026-03-10";
        add_assignment(&conn, d, "com.a", "", work.id).unwrap();

        let a = Resolver::load(&conn).unwrap();
        assert_eq!(a.assigned_to(d, "com.a", ""), [work.id]); // explicit app-level
        assert!(a.assigned_to(d, "com.a", "x").is_empty()); // no fallback to app-level
    }

    #[test]
    fn assignments_load_day_scopes_to_one_date() {
        let conn = mem();
        let work = create_project(&conn, "Work", "#fff").unwrap();
        add_assignment(&conn, "2026-03-10", "com.a", "", work.id).unwrap();
        add_assignment(&conn, "2026-03-11", "com.b", "", work.id).unwrap();

        let a = Resolver::load_day(&conn, "2026-03-10").unwrap();
        assert_eq!(a.assigned_to("2026-03-10", "com.a", ""), [work.id]);
        assert!(a.assigned_to("2026-03-11", "com.b", "").is_empty()); // other day not loaded
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
        assert_eq!(ids(&tx.projects), vec![lo, hi]);

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

        let a = Resolver::load(&conn).unwrap();
        assert_eq!(a.assigned_to(d, "com.a", "x"), [p2.id]); // only p1's tag removed
    }

    #[test]
    fn removing_an_app_from_a_project_clears_all_its_rows() {
        let conn = mem();
        let p1 = create_project(&conn, "P1", "#fff").unwrap();
        let p2 = create_project(&conn, "P2", "#000").unwrap();
        let d = "2026-03-10";
        add_assignment(&conn, d, "com.a", "", p1.id).unwrap();
        add_assignment(&conn, d, "com.a", "x", p1.id).unwrap();
        add_assignment(&conn, d, "com.a", "x", p2.id).unwrap();
        add_assignment(&conn, d, "com.b", "", p1.id).unwrap();

        remove_from_project(&conn, p1.id, "com.a", None).unwrap();

        let a = Resolver::load(&conn).unwrap();
        assert!(a.assigned_to(d, "com.a", "").is_empty());
        assert_eq!(a.assigned_to(d, "com.a", "x"), [p2.id]);
        assert_eq!(a.assigned_to(d, "com.b", ""), [p1.id]);
    }

    #[test]
    fn removing_a_title_from_a_project_clears_only_that_title() {
        let conn = mem();
        let p1 = create_project(&conn, "P1", "#fff").unwrap();
        let p2 = create_project(&conn, "P2", "#000").unwrap();
        let d = "2026-03-10";
        add_assignment(&conn, d, "com.a", "", p1.id).unwrap();
        add_assignment(&conn, d, "com.a", "x", p1.id).unwrap();
        add_assignment(&conn, d, "com.a", "x", p2.id).unwrap();

        remove_from_project(&conn, p1.id, "com.a", Some("x")).unwrap();

        let a = Resolver::load(&conn).unwrap();
        assert_eq!(a.assigned_to(d, "com.a", ""), [p1.id]);
        assert_eq!(a.assigned_to(d, "com.a", "x"), [p2.id]);
    }

    #[test]
    fn resolve_title_tags_override_app_level_no_union() {
        let conn = mem();
        let p1 = create_project(&conn, "P1", "#fff").unwrap();
        let p2 = create_project(&conn, "P2", "#000").unwrap();
        let d = "2026-03-10";
        add_assignment(&conn, d, "com.a", "", p1.id).unwrap(); // app-level
        add_assignment(&conn, d, "com.a", "x", p2.id).unwrap(); // title

        let a = Resolver::load(&conn).unwrap();
        // Title's own tag wins outright; the app-level tag is NOT unioned in.
        assert_eq!(ids(&a.resolve(d, "com.a", "x")), [p2.id]);
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
        migrate_add_rule_tables(&conn).unwrap();

        // Existing rows survive the rebuild.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM day_assignments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
        let a = Resolver::load(&conn).unwrap();
        assert_eq!(a.assigned_to("2026-03-10", "com.a", "x"), [1]);
        assert_eq!(a.assigned_to("2026-03-10", "com.a", ""), [2]);

        // After: a second project for the same key is now insertable.
        add_assignment(&conn, "2026-03-10", "com.a", "x", 9).unwrap();
        let a = Resolver::load(&conn).unwrap();
        assert_eq!(a.assigned_to("2026-03-10", "com.a", "x"), [1, 9]);

        // A second migration run is a no-op (schema already new).
        migrate_assignments_multi(&conn).unwrap();
        let a = Resolver::load(&conn).unwrap();
        assert_eq!(a.assigned_to("2026-03-10", "com.a", "x"), [1, 9]);
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
        migrate_add_rule_tables(&conn).unwrap();

        // Both rows survived with title backfilled to ''.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM day_assignments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
        let a = Resolver::load(&conn).unwrap();
        assert_eq!(a.assigned_to("2026-01-05", "com.x", ""), [10]);
        assert_eq!(a.assigned_to("2026-01-05", "com.y", ""), [20]);

        // The final schema has project_id in the PK, so two projects on the
        // same (date, app_key, title) are insertable (migrate_assignments_multi
        // was effectively a no-op because migrate_assignments_title already
        // wrote the 4-column PK form).
        add_assignment(&conn, "2026-01-05", "com.x", "", 99).unwrap();
        let a = Resolver::load(&conn).unwrap();
        assert_eq!(a.assigned_to("2026-01-05", "com.x", ""), [10, 99]);
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
        seg(
            &conn,
            s + 30,
            s + 40,
            Some("com.a"),
            Some("A"),
            Some("keep"),
        ); // app-level tag
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

    // ---- assignment rules -------------------------------------------------

    #[test]
    fn rule_bills_matching_activity_without_any_assignment() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("dev.warp"), Some("Warp"), Some("t"));
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();

        create_rule(&conn, p.id, "dev.warp", None, "", None).unwrap();

        let rows = project_breakdown(&conn, p.id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].date, d);
        assert_eq!(rows[0].seconds, 100);
    }

    // ---- title rules (ADR 0003) -------------------------------------------

    #[test]
    fn title_rule_shadows_the_app_rule_for_the_titles_it_matches() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("com.chrome"), Some("Chrome"), Some("ScriptR \u{2014} Dashboard"));
        seg(&conn, s + 200, s + 250, Some("com.chrome"), Some("Chrome"), Some("Hacker News"));
        let reading = create_project(&conn, "Reading", "#fff").unwrap();
        let echo = create_project(&conn, "Echo", "#000").unwrap();

        create_rule(&conn, reading.id, "com.chrome", None, "", None).unwrap();
        create_rule(&conn, echo.id, "com.chrome", None, "ScriptR", None).unwrap();

        // The matched title bills Echo *instead of* Reading: rung three speaks,
        // so rung four stays silent. Everything else still inherits the app rule.
        let res = Resolver::load(&conn).unwrap();
        assert_eq!(
            ids(&res.resolve(d, "com.chrome", "ScriptR \u{2014} Dashboard")),
            vec![echo.id]
        );
        assert_eq!(ids(&res.resolve(d, "com.chrome", "Hacker News")), vec![reading.id]);

        assert_eq!(project_breakdown(&conn, echo.id).unwrap()[0].seconds, 100);
        assert_eq!(project_breakdown(&conn, reading.id).unwrap()[0].seconds, 50);
    }

    #[test]
    fn title_rule_matches_case_insensitively_and_partially() {
        let conn = mem();
        let d = "2026-04-01";
        let p = create_project(&conn, "Echo", "#fff").unwrap();
        create_rule(&conn, p.id, "com.chrome", None, "  ScriptR ", None).unwrap();
        let res = Resolver::load(&conn).unwrap();

        // Surrounding whitespace is trimmed at write time, the rest is a plain
        // case-folded substring test.
        assert_eq!(ids(&res.resolve(d, "com.chrome", "scriptr.io | Docs")), vec![p.id]);
        assert_eq!(ids(&res.resolve(d, "com.chrome", "SCRIPTR")), vec![p.id]);
        assert!(res.resolve(d, "com.chrome", "Hacker News").is_empty());
        // A pattern is never empty, so untitled activity never matches one.
        assert!(res.resolve(d, "com.chrome", "").is_empty());
    }

    #[test]
    fn dated_assignment_still_outranks_a_title_rule() {
        let conn = mem();
        let d = "2026-04-01";
        let echo = create_project(&conn, "Echo", "#fff").unwrap();
        let admin = create_project(&conn, "Admin", "#000").unwrap();
        create_rule(&conn, echo.id, "com.chrome", None, "ScriptR", None).unwrap();
        add_assignment(&conn, d, "com.chrome", "ScriptR \u{2014} Dashboard", admin.id).unwrap();

        // Rungs one and two sit above both rule rungs (ADR 0001).
        let res = Resolver::load(&conn).unwrap();
        assert_eq!(
            ids(&res.resolve(d, "com.chrome", "ScriptR \u{2014} Dashboard")),
            vec![admin.id]
        );
        assert_eq!(ids(&res.resolve(d, "com.chrome", "ScriptR Docs")), vec![echo.id]);
    }

    #[test]
    fn excluding_a_day_takes_back_a_title_rule_link() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("com.chrome"), Some("Chrome"), Some("ScriptR Docs"));
        let p = create_project(&conn, "Echo", "#fff").unwrap();
        create_rule(&conn, p.id, "com.chrome", None, "ScriptR", None).unwrap();

        exclude_for_day(&conn, d, "com.chrome", "ScriptR Docs", p.id).unwrap();

        // The exception is dated and keyed to the row, not to the rule: this day
        // stops billing, the rule keeps standing for every other day.
        let res = Resolver::load(&conn).unwrap();
        assert!(res.resolve(d, "com.chrome", "ScriptR Docs").is_empty());
        assert_eq!(ids(&res.resolve("2026-04-02", "com.chrome", "ScriptR Docs")), vec![p.id]);
    }

    #[test]
    fn day_view_draws_a_title_rule_on_the_title_row_it_matches() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("com.chrome"), Some("Chrome"), Some("ScriptR Docs"));
        seg(&conn, s + 200, s + 250, Some("com.chrome"), Some("Chrome"), Some("Hacker News"));
        let p = create_project(&conn, "Echo", "#fff").unwrap();
        create_rule(&conn, p.id, "com.chrome", None, "ScriptR", None).unwrap();

        let view = day_view(&conn, d).unwrap();
        let app = &view.apps[0];
        // A title rule's subject is the title, so it draws there, not on the app
        // row — otherwise a shadowed title would show nothing while billing.
        assert!(app.projects.is_empty());
        let matched = app.titles.iter().find(|t| t.title == "ScriptR Docs").unwrap();
        assert_eq!(ids(&matched.projects), vec![p.id]);
        assert_eq!(matched.projects[0].state, LinkState::Rule);
        let other = app.titles.iter().find(|t| t.title == "Hacker News").unwrap();
        assert!(other.projects.is_empty());
    }

    #[test]
    fn tracked_titles_lists_busiest_first_and_skips_untitled_and_ignored() {
        let conn = mem();
        let s = day_start_ts("2026-04-01");
        seg(&conn, s, s + 100, Some("com.chrome"), Some("Chrome"), Some("Docs"));
        seg(&conn, s + 200, s + 500, Some("com.chrome"), Some("Chrome"), Some("News"));
        seg(&conn, s + 600, s + 650, Some("com.chrome"), Some("Chrome"), None);
        seg(&conn, s + 700, s + 999, Some("com.chrome"), Some("Chrome"), Some("Secret"));
        seg(&conn, s + 1000, s + 9999, Some("dev.warp"), Some("Warp"), Some("shell"));
        add_ignored_entry(&conn, "com.chrome", None, "Secret").unwrap();

        let titles = tracked_titles(&conn, "com.chrome").unwrap();
        let got: Vec<(&str, i64)> = titles
            .iter()
            .map(|t| (t.title.as_str(), t.seconds))
            .collect();
        assert_eq!(got, vec![("News", 300), ("Docs", 100)]);
    }

    #[test]
    fn rules_can_be_listed_and_deleted() {
        let conn = mem();
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        let id = create_rule(&conn, p.id, "dev.warp", None, "", None).unwrap().id;
        create_rule(&conn, p.id, "com.zen", None, "", Some("2026-04-01")).unwrap();

        let rules = list_rules(&conn).unwrap();
        assert_eq!(rules.len(), 2);
        let warp = rules.iter().find(|r| r.app_key == "dev.warp").unwrap();
        assert_eq!(warp.project_id, p.id);
        assert_eq!(warp.effective_from, None);
        let zen = rules.iter().find(|r| r.app_key == "com.zen").unwrap();
        assert_eq!(zen.effective_from.as_deref(), Some("2026-04-01"));

        delete_rule(&conn, id).unwrap();
        let rules = list_rules(&conn).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].app_key, "com.zen");
    }

    #[test]
    fn forward_only_rule_ignores_days_before_it_takes_effect() {
        let conn = mem();
        let before = "2026-04-01";
        let after = "2026-04-03";
        seg(
            &conn,
            day_start_ts(before),
            day_start_ts(before) + 100,
            Some("dev.warp"),
            Some("Warp"),
            None,
        );
        seg(
            &conn,
            day_start_ts(after),
            day_start_ts(after) + 60,
            Some("dev.warp"),
            Some("Warp"),
            None,
        );
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();

        create_rule(&conn, p.id, "dev.warp", None, "", Some("2026-04-02")).unwrap();

        let rows = project_breakdown(&conn, p.id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].date, after);
        assert_eq!(rows[0].seconds, 60);
    }

    #[test]
    fn day_assignment_outranks_a_standing_rule() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("dev.warp"), Some("Warp"), None);
        let flow = create_project(&conn, "Flowstate", "#fff").unwrap();
        let side = create_project(&conn, "Side", "#000").unwrap();
        create_rule(&conn, flow.id, "dev.warp", None, "", None).unwrap();

        add_assignment(&conn, d, "dev.warp", "", side.id).unwrap();

        // Layer-first: the dated act answers, and the rule is not unioned in.
        assert!(project_breakdown(&conn, flow.id).unwrap().is_empty());
        assert_eq!(project_breakdown(&conn, side.id).unwrap()[0].seconds, 100);
    }

    #[test]
    fn two_rules_on_one_app_bill_both_projects() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("dev.warp"), Some("Warp"), None);
        let a = create_project(&conn, "A", "#fff").unwrap();
        let b = create_project(&conn, "B", "#000").unwrap();
        create_rule(&conn, a.id, "dev.warp", None, "", None).unwrap();
        create_rule(&conn, b.id, "dev.warp", None, "", None).unwrap();

        // Rules at one rung union, and each bills the full duration.
        assert_eq!(project_breakdown(&conn, a.id).unwrap()[0].seconds, 100);
        assert_eq!(project_breakdown(&conn, b.id).unwrap()[0].seconds, 100);
    }

    #[test]
    fn exception_withdraws_one_day_from_a_rule() {
        let conn = mem();
        let kept = "2026-04-01";
        let dropped = "2026-04-02";
        for d in [kept, dropped] {
            seg(
                &conn,
                day_start_ts(d),
                day_start_ts(d) + 100,
                Some("dev.warp"),
                Some("Warp"),
                None,
            );
        }
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        create_rule(&conn, p.id, "dev.warp", None, "", None).unwrap();

        write_exception(&conn, dropped, "dev.warp", "", p.id).unwrap();

        let rows = project_breakdown(&conn, p.id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].date, kept);

        // Clearing it returns the day to Undecided, so the rule applies again.
        remove_exception(&conn, dropped, "dev.warp", "", p.id).unwrap();
        assert_eq!(project_breakdown(&conn, p.id).unwrap().len(), 2);
    }

    #[test]
    fn assigning_clears_an_exception_on_the_same_key() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("dev.warp"), Some("Warp"), None);
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        write_exception(&conn, d, "dev.warp", "", p.id).unwrap();

        add_assignment(&conn, d, "dev.warp", "", p.id).unwrap();

        // A key holds a yes or a no, never both.
        let res = Resolver::load(&conn).unwrap();
        assert!(res.excepted_from(d, "dev.warp", "").is_empty());
        assert_eq!(project_breakdown(&conn, p.id).unwrap()[0].seconds, 100);
    }

    #[test]
    fn exception_carves_one_title_out_of_an_app_level_assignment() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(
            &conn,
            s,
            s + 100,
            Some("com.zen"),
            Some("Zen"),
            Some("work"),
        );
        seg(
            &conn,
            s + 100,
            s + 160,
            Some("com.zen"),
            Some("Zen"),
            Some("personal"),
        );
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        add_assignment(&conn, d, "com.zen", "", p.id).unwrap();

        write_exception(&conn, d, "com.zen", "personal", p.id).unwrap();

        // The app-level tag still covers "work"; "personal" is carved out.
        assert_eq!(project_breakdown(&conn, p.id).unwrap()[0].seconds, 100);
    }

    #[test]
    fn rule_covered_activity_is_not_untagged() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("dev.warp"), Some("Warp"), None);
        seg(
            &conn,
            s + 100,
            s + 200,
            Some("com.other"),
            Some("Other"),
            None,
        );
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        create_rule(&conn, p.id, "dev.warp", None, "", None).unwrap();

        // Auto-delete must not eat time a standing rule classifies.
        let removed = clear_untagged(&conn).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(count(&conn, "segments"), 1);
    }

    #[test]
    fn deleting_a_project_takes_its_rules_and_exceptions_with_it() {
        let conn = mem();
        let d = "2026-04-01";
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        create_rule(&conn, p.id, "dev.warp", None, "", None).unwrap();
        write_exception(&conn, d, "dev.warp", "", p.id).unwrap();

        delete_project(&conn, p.id).unwrap();

        assert!(list_rules(&conn).unwrap().is_empty());
        assert_eq!(count(&conn, "assignment_exceptions"), 0);
    }

    #[test]
    fn day_view_shows_rule_provenance_and_exceptions() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("dev.warp"), Some("Warp"), Some("x"));
        seg(
            &conn,
            s + 100,
            s + 200,
            Some("com.zen"),
            Some("Zen"),
            Some("y"),
        );
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        create_rule(&conn, p.id, "dev.warp", None, "", None).unwrap();
        write_exception(&conn, d, "com.zen", "", p.id).unwrap();

        let view = day_view(&conn, d).unwrap();
        let warp = view.apps.iter().find(|a| a.app_key == "dev.warp").unwrap();
        assert_eq!(warp.projects.len(), 1);
        assert_eq!(warp.projects[0].state, LinkState::Rule);
        assert!(warp.projects[0].rule_id.is_some());
        // Titles render inheritance by absence — the dot lives on the app row.
        assert!(warp.titles[0].projects.is_empty());

        let zen = view.apps.iter().find(|a| a.app_key == "com.zen").unwrap();
        assert_eq!(zen.projects[0].state, LinkState::Excluded);
    }

    #[test]
    fn project_day_receipt_decomposes_one_day_with_reasons() {
        let conn = mem();
        let d = "2026-04-01";
        let other = "2026-04-02";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("dev.warp"), Some("Warp"), Some("x"));
        seg(
            &conn,
            s + 100,
            s + 160,
            Some("com.zen"),
            Some("Zen"),
            Some("y"),
        );
        seg(
            &conn,
            day_start_ts(other),
            day_start_ts(other) + 500,
            Some("dev.warp"),
            Some("Warp"),
            Some("x"),
        );
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        create_rule(&conn, p.id, "dev.warp", None, "", None).unwrap();
        add_assignment(&conn, d, "com.zen", "y", p.id).unwrap();

        let entries = project_day_entries(&conn, p.id, d).unwrap();

        // Only this day, biggest first, each saying why it is here.
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].app_key, "dev.warp");
        assert_eq!(entries[0].seconds, 100);
        assert_eq!(entries[0].state, LinkState::Rule);
        assert!(entries[0].rule_id.is_some());
        assert_eq!(entries[1].app_key, "com.zen");
        assert_eq!(entries[1].title, "y");
        assert_eq!(entries[1].state, LinkState::Direct);
    }

    #[test]
    fn excluding_a_hand_made_link_just_deletes_it() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("dev.warp"), Some("Warp"), None);
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        add_assignment(&conn, d, "dev.warp", "", p.id).unwrap();

        exclude_for_day(&conn, d, "dev.warp", "", p.id).unwrap();

        // Nothing would bring it back, so no "no" is worth recording: the row
        // is Undecided again and behaves exactly as it did before rules.
        assert!(project_breakdown(&conn, p.id).unwrap().is_empty());
        assert_eq!(count(&conn, "assignment_exceptions"), 0);
    }

    #[test]
    fn excluding_a_rule_covered_row_records_an_exception() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("dev.warp"), Some("Warp"), None);
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        create_rule(&conn, p.id, "dev.warp", None, "", None).unwrap();

        exclude_for_day(&conn, d, "dev.warp", "", p.id).unwrap();

        assert!(project_breakdown(&conn, p.id).unwrap().is_empty());
        assert_eq!(count(&conn, "assignment_exceptions"), 1);
    }

    #[test]
    fn excluding_an_inherited_title_records_an_exception() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(
            &conn,
            s,
            s + 100,
            Some("com.zen"),
            Some("Zen"),
            Some("keep"),
        );
        seg(
            &conn,
            s + 100,
            s + 160,
            Some("com.zen"),
            Some("Zen"),
            Some("go"),
        );
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        add_assignment(&conn, d, "com.zen", "", p.id).unwrap();

        exclude_for_day(&conn, d, "com.zen", "go", p.id).unwrap();

        assert_eq!(count(&conn, "assignment_exceptions"), 1);
        assert_eq!(project_breakdown(&conn, p.id).unwrap()[0].seconds, 100);
    }

    #[test]
    fn tracked_apps_lists_busiest_first_and_skips_ignored() {
        let conn = mem();
        let s = day_start_ts("2026-04-01");
        seg(&conn, s, s + 50, Some("com.small"), Some("Small"), None);
        seg(
            &conn,
            s + 50,
            s + 250,
            Some("com.big"),
            Some("Big"),
            Some("a"),
        );
        seg(
            &conn,
            s + 250,
            s + 300,
            Some("com.big"),
            Some("Big"),
            Some("b"),
        );
        seg(
            &conn,
            s + 300,
            s + 900,
            Some("com.noise"),
            Some("Noise"),
            None,
        );
        add_ignored_entry(&conn, "com.noise", Some("Noise"), "").unwrap();

        let apps = tracked_apps(&conn).unwrap();

        // Ignored activity is not a candidate for a rule, and the busiest app
        // is the one you are most likely to want a rule for.
        assert_eq!(
            apps.iter().map(|a| a.app_key.as_str()).collect::<Vec<_>>(),
            vec!["com.big", "com.small"]
        );
        assert_eq!(apps[0].app_name, "Big");
        assert_eq!(apps[0].seconds, 250);
    }

    #[test]
    fn an_excluded_rule_row_reads_as_excluded_not_as_included() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(&conn, s, s + 100, Some("dev.warp"), Some("Warp"), Some("x"));
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        create_rule(&conn, p.id, "dev.warp", None, "", None).unwrap();

        exclude_for_day(&conn, d, "dev.warp", "", p.id).unwrap();

        // The exception must REPLACE the rule link, not sit beside it: two
        // entries for one project would fold back to "included via rule" and
        // the dot would stop responding.
        let view = day_view(&conn, d).unwrap();
        let warp = view.apps.iter().find(|a| a.app_key == "dev.warp").unwrap();
        assert_eq!(warp.projects.len(), 1);
        assert_eq!(warp.projects[0].state, LinkState::Excluded);
        assert!(project_breakdown(&conn, p.id).unwrap().is_empty());
    }

    #[test]
    fn removing_an_inherited_title_from_a_project_makes_it_stick() {
        let conn = mem();
        let d = "2026-04-01";
        let s = day_start_ts(d);
        seg(
            &conn,
            s,
            s + 100,
            Some("com.zen"),
            Some("Zen"),
            Some("keep"),
        );
        seg(
            &conn,
            s + 100,
            s + 160,
            Some("com.zen"),
            Some("Zen"),
            Some("go"),
        );
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        add_assignment(&conn, d, "com.zen", "", p.id).unwrap();

        // "go" is in the project only by inheriting the app-level tag, so there
        // is no row to delete — removal has to record an exception instead.
        let vetoed = remove_from_project(&conn, p.id, "com.zen", Some("go")).unwrap();

        assert_eq!(vetoed, 1);
        assert_eq!(project_breakdown(&conn, p.id).unwrap()[0].seconds, 100);
    }

    #[test]
    fn removing_a_rule_covered_app_from_a_project_makes_it_stick() {
        let conn = mem();
        for d in ["2026-04-01", "2026-04-02"] {
            seg(
                &conn,
                day_start_ts(d),
                day_start_ts(d) + 100,
                Some("dev.warp"),
                Some("Warp"),
                None,
            );
        }
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        create_rule(&conn, p.id, "dev.warp", None, "", None).unwrap();

        let vetoed = remove_from_project(&conn, p.id, "dev.warp", None).unwrap();

        // Both days excepted; the rule survives for anything else it covers.
        assert_eq!(vetoed, 2);
        assert!(project_breakdown(&conn, p.id).unwrap().is_empty());
        assert_eq!(list_rules(&conn).unwrap().len(), 1);
    }

    #[test]
    fn reset_everything_leaves_no_rules_or_exceptions_behind() {
        let conn = mem();
        let d = "2026-04-01";
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        create_rule(&conn, p.id, "dev.warp", None, "", None).unwrap();
        write_exception(&conn, d, "dev.warp", "", p.id).unwrap();

        reset_everything(&conn).unwrap();

        // SQLite reuses rowids, so a surviving rule would graft itself onto
        // whatever project is created next.
        assert_eq!(count(&conn, "assignment_rules"), 0);
        assert_eq!(count(&conn, "assignment_exceptions"), 0);
    }

    #[test]
    fn clearing_tracking_data_clears_both_polarities() {
        let conn = mem();
        let d = "2026-04-01";
        let p = create_project(&conn, "Flowstate", "#fff").unwrap();
        add_assignment(&conn, d, "com.a", "", p.id).unwrap();
        write_exception(&conn, d, "com.b", "", p.id).unwrap();

        clear_tracking_data(&conn).unwrap();

        // A yes and a no are the same dated statement; neither may outlive it.
        assert_eq!(count(&conn, "day_assignments"), 0);
        assert_eq!(count(&conn, "assignment_exceptions"), 0);
        assert_eq!(list_projects(&conn).unwrap().len(), 1);
    }
}
