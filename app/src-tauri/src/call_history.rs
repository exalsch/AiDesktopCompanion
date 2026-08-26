//! Persistent record of Assistant Mode voice calls.
//!
//! A realtime call is the most expensive thing this app does - it is billed per
//! audio token, roughly one per 100ms heard and one per 50ms spoken - and until
//! now every one of them vanished the moment the panel was closed. What it
//! cost, what was said and how long it ran were visible while the session was
//! on screen and then gone.
//!
//! Kept in SQLite rather than the JSON files the rest of the app uses. Calls
//! accumulate one row at a time and are read back newest-first in pages, which
//! is exactly what a rewrite-the-whole-file store is bad at; and the running
//! total spend is a `SUM()` rather than a full parse.
//!
//! The database lives beside `settings.json` in `%APPDATA%\AiDesktopCompanion`,
//! as `calls.db`. Transcripts are stored as plain text: anyone who can read that
//! directory can read them, which is the same bargain `settings.json` already
//! makes with the API key.

use once_cell::sync::OnceCell;
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Schema history. Append only - `rusqlite_migration` tracks how far a database
/// has been brought forward in SQLite's `user_version`, so editing an existing
/// entry silently skips the change on every machine that already ran it.
static MIGRATIONS: &[M] = &[M::up(
  "CREATE TABLE IF NOT EXISTS calls (
     id              INTEGER PRIMARY KEY AUTOINCREMENT,
     started_at      INTEGER NOT NULL,
     duration_ms     INTEGER NOT NULL,
     model           TEXT    NOT NULL,
     voice           TEXT    NOT NULL,
     saved           INTEGER NOT NULL DEFAULT 0,
     title           TEXT    NOT NULL,
     transcript      TEXT    NOT NULL,
     turns           INTEGER NOT NULL DEFAULT 0,
     responses       INTEGER NOT NULL DEFAULT 0,
     audio_in        INTEGER NOT NULL DEFAULT 0,
     audio_in_cached INTEGER NOT NULL DEFAULT 0,
     audio_out       INTEGER NOT NULL DEFAULT 0,
     text_in         INTEGER NOT NULL DEFAULT 0,
     text_in_cached  INTEGER NOT NULL DEFAULT 0,
     text_out        INTEGER NOT NULL DEFAULT 0,
     cost_usd        REAL,
     tools_enabled   INTEGER NOT NULL DEFAULT 0,
     supervisor      TEXT
   );
   CREATE INDEX IF NOT EXISTS idx_calls_started_at ON calls (started_at DESC);",
)];

/// One recorded call.
///
/// Field names are the wire format in both directions, so the frontend sends
/// and receives snake_case. Tauri rewrites *command arguments* to camelCase but
/// leaves the insides of a serialized struct alone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallEntry {
  /// Assigned by SQLite. Ignored on the way in.
  #[serde(default)]
  pub id: i64,
  /// Unix seconds the call connected.
  pub started_at: i64,
  pub duration_ms: i64,
  pub model: String,
  pub voice: String,
  /// Pinned by the user. Pinned calls survive every retention sweep.
  #[serde(default)]
  pub saved: bool,
  /// First thing the user said, trimmed to a line. The list has to show
  /// something recognisable without loading every transcript.
  pub title: String,
  /// The conversation as a JSON array of `{ "role": ..., "content": ... }`.
  /// Stored as text rather than a table: it is only ever read back whole.
  pub transcript: String,
  pub turns: i64,
  pub responses: i64,
  pub audio_in: i64,
  pub audio_in_cached: i64,
  pub audio_out: i64,
  pub text_in: i64,
  pub text_in_cached: i64,
  pub text_out: i64,
  /// `None` when the model had no known rates, which is deliberately different
  /// from `Some(0.0)` - one means "not priced here", the other "free".
  pub cost_usd: Option<f64>,
  #[serde(default)]
  pub tools_enabled: bool,
  /// `"always"`, `"needed"`, or `None` when no supervisor was used.
  pub supervisor: Option<String>,
}

/// A page of calls, newest first.
#[derive(Clone, Debug, Serialize)]
pub struct CallPage {
  pub entries: Vec<CallEntry>,
  /// Whether another page exists after this one.
  pub has_more: bool,
}

/// Lifetime totals, for the "what has this cost me" line.
#[derive(Clone, Debug, Default, Serialize)]
pub struct CallStats {
  pub calls: i64,
  pub duration_ms: i64,
  /// Sum over the calls that had known rates. Calls priced as `None` are
  /// counted in `unpriced` instead of being quietly folded in as zero.
  pub cost_usd: f64,
  pub unpriced: i64,
}

// ---------------------------------------------------------------------------
// Retention
// ---------------------------------------------------------------------------

/// How long unpinned calls are kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retention {
  /// Delete nothing.
  KeepAll,
  /// Delete anything older than this many days.
  Days(i64),
  /// Keep only this many, newest first.
  Last(usize),
}

/// Normalize a persisted retention value, defaulting to `"days_30"`.
///
/// A month is long enough for "what did that cost last week" to still be
/// answerable, and short enough that a year of recorded conversation does not
/// pile up on disk because nobody ever looked at the setting.
pub fn normalize_retention(value: &str) -> &'static str {
  match value.trim() {
    "keep_all" => "keep_all",
    "days_7" => "days_7",
    "months_3" => "months_3",
    "last_50" => "last_50",
    _ => "days_30",
  }
}

pub fn parse_retention(value: &str) -> Retention {
  match normalize_retention(value) {
    "keep_all" => Retention::KeepAll,
    "days_7" => Retention::Days(7),
    "months_3" => Retention::Days(90),
    "last_50" => Retention::Last(50),
    _ => Retention::Days(30),
  }
}

/// Whether calls are recorded at all, and for how long.
///
/// Both live inside the `assistant_realtime` blob in `settings.json`, which is
/// persisted wholesale - so neither needs its own entry in the `save_settings`
/// allowlist, and both are written by the same panel that reads them.
fn settings_blob() -> serde_json::Value {
  crate::config::load_settings_json()
    .get("assistant_realtime")
    .cloned()
    .unwrap_or(serde_json::Value::Null)
}

pub fn recording_enabled() -> bool {
  settings_blob()
    .get("history_enabled")
    .and_then(|x| x.as_bool())
    .unwrap_or(true)
}

pub fn retention_from_settings() -> Retention {
  let raw = settings_blob()
    .get("history_retention")
    .and_then(|x| x.as_str())
    .unwrap_or("")
    .to_string();
  parse_retention(&raw)
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

fn db_file() -> Result<PathBuf, String> {
  let settings = crate::config::settings_config_path()
    .ok_or_else(|| "Unsupported platform for the config path".to_string())?;
  let dir = settings
    .parent()
    .ok_or_else(|| "The config path has no parent directory".to_string())?;
  std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
  Ok(dir.join("calls.db"))
}

/// Resolved once. Migrations run on the first open of the process and not
/// again, so the per-command opens below are just `Connection::open`.
static READY: OnceCell<Result<PathBuf, String>> = OnceCell::new();

fn open() -> Result<Connection, String> {
  let path = READY
    .get_or_init(|| {
      let path = db_file()?;
      let mut conn =
        Connection::open(&path).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
      migrate(&mut conn)?;
      Ok(path)
    })
    .clone()?;
  Connection::open(&path).map_err(|e| format!("Could not open the call history database: {e}"))
}

fn migrate(conn: &mut Connection) -> Result<(), String> {
  Migrations::new(MIGRATIONS.to_vec())
    .to_latest(conn)
    .map_err(|e| format!("Call history migration failed: {e}"))?;
  Ok(())
}

// ---------------------------------------------------------------------------
// Queries
//
// Each takes a `&Connection` rather than opening its own, so all of the
// behaviour below is reachable from a test against an in-memory database.
// ---------------------------------------------------------------------------

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<CallEntry> {
  Ok(CallEntry {
    id: row.get("id")?,
    started_at: row.get("started_at")?,
    duration_ms: row.get("duration_ms")?,
    model: row.get("model")?,
    voice: row.get("voice")?,
    saved: row.get::<_, i64>("saved")? != 0,
    title: row.get("title")?,
    transcript: row.get("transcript")?,
    turns: row.get("turns")?,
    responses: row.get("responses")?,
    audio_in: row.get("audio_in")?,
    audio_in_cached: row.get("audio_in_cached")?,
    audio_out: row.get("audio_out")?,
    text_in: row.get("text_in")?,
    text_in_cached: row.get("text_in_cached")?,
    text_out: row.get("text_out")?,
    cost_usd: row.get("cost_usd")?,
    tools_enabled: row.get::<_, i64>("tools_enabled")? != 0,
    supervisor: row.get("supervisor")?,
  })
}

const SELECT_COLUMNS: &str = "id, started_at, duration_ms, model, voice, saved, title, transcript, \
   turns, responses, audio_in, audio_in_cached, audio_out, text_in, text_in_cached, text_out, \
   cost_usd, tools_enabled, supervisor";

pub fn insert(conn: &Connection, entry: &CallEntry) -> Result<i64, String> {
  conn
    .execute(
      "INSERT INTO calls (
         started_at, duration_ms, model, voice, saved, title, transcript, turns, responses,
         audio_in, audio_in_cached, audio_out, text_in, text_in_cached, text_out,
         cost_usd, tools_enabled, supervisor
       ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
      params![
        entry.started_at,
        entry.duration_ms,
        entry.model,
        entry.voice,
        entry.saved as i64,
        entry.title,
        entry.transcript,
        entry.turns,
        entry.responses,
        entry.audio_in,
        entry.audio_in_cached,
        entry.audio_out,
        entry.text_in,
        entry.text_in_cached,
        entry.text_out,
        entry.cost_usd,
        entry.tools_enabled as i64,
        entry.supervisor,
      ],
    )
    .map_err(|e| format!("Could not record the call: {e}"))?;
  Ok(conn.last_insert_rowid())
}

/// Newest first, paged by id.
///
/// The cursor is an id rather than an offset. Ids only ever increase, so a page
/// boundary stays put even if a call is deleted or recorded while the list is
/// open - an offset would skip or repeat a row in exactly that case.
pub fn list(conn: &Connection, cursor: Option<i64>, limit: usize) -> Result<CallPage, String> {
  let limit = limit.clamp(1, 200);
  // One extra row, purely to answer "is there more" without a second query.
  let fetch = (limit + 1) as i64;

  let sql = match cursor {
    Some(_) => format!("SELECT {SELECT_COLUMNS} FROM calls WHERE id < ?1 ORDER BY id DESC LIMIT ?2"),
    None => format!("SELECT {SELECT_COLUMNS} FROM calls ORDER BY id DESC LIMIT ?1"),
  };
  let mut stmt = conn
    .prepare(&sql)
    .map_err(|e| format!("Could not read the call history: {e}"))?;

  let mapped = match cursor {
    Some(id) => stmt.query_map(params![id, fetch], row_to_entry),
    None => stmt.query_map(params![fetch], row_to_entry),
  }
  .map_err(|e| format!("Could not read the call history: {e}"))?;

  let mut entries: Vec<CallEntry> = Vec::new();
  for row in mapped {
    entries.push(row.map_err(|e| format!("Could not read a call: {e}"))?);
  }

  let has_more = entries.len() > limit;
  entries.truncate(limit);
  Ok(CallPage { entries, has_more })
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<CallEntry>, String> {
  conn
    .query_row(
      &format!("SELECT {SELECT_COLUMNS} FROM calls WHERE id = ?1"),
      params![id],
      row_to_entry,
    )
    .optional()
    .map_err(|e| format!("Could not read call {id}: {e}"))
}

pub fn set_saved(conn: &Connection, id: i64, saved: bool) -> Result<(), String> {
  conn
    .execute(
      "UPDATE calls SET saved = ?1 WHERE id = ?2",
      params![saved as i64, id],
    )
    .map_err(|e| format!("Could not pin call {id}: {e}"))?;
  Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
  conn
    .execute("DELETE FROM calls WHERE id = ?1", params![id])
    .map_err(|e| format!("Could not delete call {id}: {e}"))?;
  Ok(())
}

/// Delete everything, or everything the user has not pinned.
pub fn clear(conn: &Connection, keep_saved: bool) -> Result<usize, String> {
  let sql = if keep_saved {
    "DELETE FROM calls WHERE saved = 0"
  } else {
    "DELETE FROM calls"
  };
  conn
    .execute(sql, [])
    .map_err(|e| format!("Could not clear the call history: {e}"))
}

pub fn stats(conn: &Connection) -> Result<CallStats, String> {
  conn
    .query_row(
      "SELECT COUNT(*),
              COALESCE(SUM(duration_ms), 0),
              COALESCE(SUM(cost_usd), 0),
              COALESCE(SUM(cost_usd IS NULL), 0)
       FROM calls",
      [],
      |row| {
        Ok(CallStats {
          calls: row.get(0)?,
          duration_ms: row.get(1)?,
          cost_usd: row.get(2)?,
          unpriced: row.get(3)?,
        })
      },
    )
    .map_err(|e| format!("Could not total the call history: {e}"))
}

/// Apply the retention policy. Returns how many calls were removed.
///
/// Pinned calls are never touched, by either policy. That is the whole point of
/// the pin: a call worth keeping should not also need the user to remember to
/// change a global setting before the next sweep runs.
pub fn cleanup(conn: &Connection, retention: Retention, now: i64) -> Result<usize, String> {
  match retention {
    Retention::KeepAll => Ok(0),
    Retention::Days(days) => {
      let cutoff = now - days * 24 * 60 * 60;
      conn
        .execute(
          "DELETE FROM calls WHERE saved = 0 AND started_at < ?1",
          params![cutoff],
        )
        .map_err(|e| format!("Could not apply the retention policy: {e}"))
    }
    Retention::Last(keep) => conn
      .execute(
        // Rank unpinned calls newest-first and drop everything past the limit.
        // Pinned calls are excluded from the ranking as well as from the
        // delete, so pinning one does not push an unpinned call out of the
        // window it was already inside.
        "DELETE FROM calls WHERE saved = 0 AND id NOT IN (
           SELECT id FROM calls WHERE saved = 0 ORDER BY id DESC LIMIT ?1
         )",
        params![keep as i64],
      )
      .map_err(|e| format!("Could not apply the retention policy: {e}")),
  }
}

fn now_secs() -> i64 {
  chrono::Utc::now().timestamp()
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Record a finished call, then apply the retention policy.
///
/// Returns the new row id, or `None` when recording is switched off - the
/// frontend does not have to check the setting before offering to save.
#[tauri::command]
pub fn call_history_save(entry: CallEntry) -> Result<Option<i64>, String> {
  if !recording_enabled() {
    return Ok(None);
  }
  let conn = open()?;
  let id = insert(&conn, &entry)?;
  // Sweeping here rather than only at startup keeps an app that is left running
  // for weeks from holding months of calls it was supposed to have dropped.
  let _ = cleanup(&conn, retention_from_settings(), now_secs());
  Ok(Some(id))
}

#[tauri::command]
pub fn call_history_list(cursor: Option<i64>, limit: Option<u32>) -> Result<CallPage, String> {
  let conn = open()?;
  list(&conn, cursor, limit.unwrap_or(25) as usize)
}

#[tauri::command]
pub fn call_history_get(id: i64) -> Result<Option<CallEntry>, String> {
  let conn = open()?;
  get(&conn, id)
}

#[tauri::command]
pub fn call_history_set_saved(id: i64, saved: bool) -> Result<(), String> {
  let conn = open()?;
  set_saved(&conn, id, saved)
}

#[tauri::command]
pub fn call_history_delete(id: i64) -> Result<(), String> {
  let conn = open()?;
  delete(&conn, id)
}

#[tauri::command]
pub fn call_history_clear(keep_saved: bool) -> Result<usize, String> {
  let conn = open()?;
  clear(&conn, keep_saved)
}

#[tauri::command]
pub fn call_history_stats() -> Result<CallStats, String> {
  let conn = open()?;
  stats(&conn)
}

/// Run the retention sweep now. Called once at startup.
#[tauri::command]
pub fn call_history_cleanup() -> Result<usize, String> {
  let conn = open()?;
  cleanup(&conn, retention_from_settings(), now_secs())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().expect("in-memory database");
    migrate(&mut conn).expect("migrations apply");
    conn
  }

  fn entry(started_at: i64, cost: Option<f64>) -> CallEntry {
    CallEntry {
      id: 0,
      started_at,
      duration_ms: 60_000,
      model: "gpt-realtime-2.1-mini".into(),
      voice: "alloy".into(),
      saved: false,
      title: "What is the build status".into(),
      transcript: "[]".into(),
      turns: 2,
      responses: 1,
      audio_in: 100,
      audio_in_cached: 10,
      audio_out: 200,
      text_in: 5,
      text_in_cached: 0,
      text_out: 7,
      cost_usd: cost,
      tools_enabled: true,
      supervisor: None,
    }
  }

  #[test]
  fn round_trips_a_call() {
    let conn = memory_db();
    let id = insert(&conn, &entry(1_000, Some(0.0421))).unwrap();
    let back = get(&conn, id).unwrap().expect("the call is there");
    assert_eq!(back.id, id);
    assert_eq!(back.model, "gpt-realtime-2.1-mini");
    assert_eq!(back.cost_usd, Some(0.0421));
    assert!(back.tools_enabled);
    assert!(!back.saved);
  }

  #[test]
  fn an_unpriced_call_stays_unpriced() {
    // Some(0.0) means the call was free; None means we do not know what it
    // cost. Collapsing the second into the first would report real spend as
    // zero, so the column has to survive the round trip as NULL.
    let conn = memory_db();
    let id = insert(&conn, &entry(1_000, None)).unwrap();
    assert_eq!(get(&conn, id).unwrap().unwrap().cost_usd, None);
  }

  #[test]
  fn lists_newest_first_and_pages_by_id() {
    let conn = memory_db();
    for i in 0..5 {
      insert(&conn, &entry(1_000 + i, Some(0.01))).unwrap();
    }
    let first = list(&conn, None, 2).unwrap();
    assert_eq!(first.entries.len(), 2);
    assert!(first.has_more);
    assert!(first.entries[0].id > first.entries[1].id);

    let cursor = first.entries.last().unwrap().id;
    let second = list(&conn, Some(cursor), 2).unwrap();
    assert_eq!(second.entries.len(), 2);
    assert!(second.entries.iter().all(|e| e.id < cursor));

    let third = list(&conn, Some(second.entries.last().unwrap().id), 2).unwrap();
    assert_eq!(third.entries.len(), 1);
    assert!(!third.has_more);
  }

  #[test]
  fn stats_separate_unpriced_calls_from_free_ones() {
    let conn = memory_db();
    insert(&conn, &entry(1_000, Some(0.25))).unwrap();
    insert(&conn, &entry(2_000, Some(0.25))).unwrap();
    insert(&conn, &entry(3_000, None)).unwrap();
    let s = stats(&conn).unwrap();
    assert_eq!(s.calls, 3);
    assert_eq!(s.unpriced, 1);
    assert!((s.cost_usd - 0.5).abs() < 1e-9);
    assert_eq!(s.duration_ms, 180_000);
  }

  #[test]
  fn retention_by_age_spares_pinned_calls() {
    let conn = memory_db();
    let now = 10_000_000_i64;
    let old = now - 40 * 24 * 60 * 60;
    let stale = insert(&conn, &entry(old, Some(0.01))).unwrap();
    let pinned = insert(&conn, &entry(old, Some(0.01))).unwrap();
    let recent = insert(&conn, &entry(now - 60, Some(0.01))).unwrap();
    set_saved(&conn, pinned, true).unwrap();

    assert_eq!(cleanup(&conn, Retention::Days(30), now).unwrap(), 1);
    assert!(get(&conn, stale).unwrap().is_none());
    assert!(get(&conn, pinned).unwrap().is_some());
    assert!(get(&conn, recent).unwrap().is_some());
  }

  #[test]
  fn retention_by_count_keeps_the_newest_unpinned() {
    let conn = memory_db();
    let mut ids = Vec::new();
    for i in 0..5 {
      ids.push(insert(&conn, &entry(1_000 + i, Some(0.01))).unwrap());
    }
    // Pin the oldest. It must survive, and must not consume one of the two
    // slots the policy reserves for recent calls.
    set_saved(&conn, ids[0], true).unwrap();

    assert_eq!(cleanup(&conn, Retention::Last(2), 10_000).unwrap(), 2);
    assert!(get(&conn, ids[0]).unwrap().is_some());
    assert!(get(&conn, ids[1]).unwrap().is_none());
    assert!(get(&conn, ids[2]).unwrap().is_none());
    assert!(get(&conn, ids[3]).unwrap().is_some());
    assert!(get(&conn, ids[4]).unwrap().is_some());
  }

  #[test]
  fn keep_all_deletes_nothing() {
    let conn = memory_db();
    insert(&conn, &entry(1, Some(0.01))).unwrap();
    assert_eq!(cleanup(&conn, Retention::KeepAll, 10_000_000).unwrap(), 0);
    assert_eq!(stats(&conn).unwrap().calls, 1);
  }

  #[test]
  fn clearing_can_spare_pinned_calls() {
    let conn = memory_db();
    let a = insert(&conn, &entry(1, Some(0.01))).unwrap();
    let b = insert(&conn, &entry(2, Some(0.01))).unwrap();
    set_saved(&conn, b, true).unwrap();

    assert_eq!(clear(&conn, true).unwrap(), 1);
    assert!(get(&conn, a).unwrap().is_none());
    assert!(get(&conn, b).unwrap().is_some());

    assert_eq!(clear(&conn, false).unwrap(), 1);
    assert_eq!(stats(&conn).unwrap().calls, 0);
  }

  #[test]
  fn retention_values_normalize_to_a_known_set() {
    for v in ["keep_all", "days_7", "days_30", "months_3", "last_50"] {
      assert_eq!(normalize_retention(v), v);
    }
    assert_eq!(normalize_retention("  days_7 \n"), "days_7");
    assert_eq!(normalize_retention(""), "days_30");
    assert_eq!(normalize_retention("forever"), "days_30");
    assert_eq!(parse_retention("months_3"), Retention::Days(90));
    assert_eq!(parse_retention("nonsense"), Retention::Days(30));
  }
}
