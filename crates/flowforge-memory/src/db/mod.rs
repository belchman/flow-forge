mod adaptive_routing;
mod agent_sessions;
mod batching;
mod checkpoints;
pub mod code_index;
pub mod complexity;
mod conversations;
mod discovered_capabilities;
mod edits;
mod effectiveness;
mod error_recovery;
pub mod failure_patterns;
pub mod file_dependencies;
mod guidance;
mod injection_cache;
pub(crate) mod helpers;
mod kv;
mod mailbox;
mod meta;
mod patterns;
pub mod project_intelligence;
pub mod recovery_strategies;
mod retention;
mod routing;
mod row_parsers;
mod schema;
mod session_metrics;
mod session_reads;
mod sessions;
pub mod task_decomposition;
pub mod test_suggestions;
pub mod tool_metrics;
mod trajectories;
pub mod vectors;
mod work_events;
mod work_items;

#[cfg(test)]
mod tests;

pub use effectiveness::PatternEffectiveness;
pub use tool_metrics::ToolMetric;
pub(crate) use helpers::{blob_to_vector, parse_datetime, vector_to_blob, VectorEntry};
pub(crate) use schema::SCHEMA_VERSION;

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use flowforge_core::{Error, Result};

/// Returns `true` if a rusqlite error represents a transient condition (e.g. SQLITE_BUSY).
fn is_transient_sqlite(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseBusy
                    | rusqlite::ffi::ErrorCode::DatabaseLocked,
                ..
            },
            _,
        )
    )
}

/// Extension trait to convert `rusqlite::Error` into `flowforge_core::Error::Database`
/// with transient classification.
pub(crate) trait SqliteExt<T> {
    fn sq(self) -> Result<T>;
}

impl<T> SqliteExt<T> for std::result::Result<T, rusqlite::Error> {
    fn sq(self) -> Result<T> {
        self.map_err(|e| {
            let transient = is_transient_sqlite(&e);
            Error::Database {
                message: e.to_string(),
                transient,
            }
        })
    }
}

pub struct MemoryDb {
    pub(crate) conn: Connection,
    /// Track transaction nesting depth for savepoint-based nesting.
    txn_depth: std::cell::Cell<u32>,
}

impl MemoryDb {
    /// Execute a closure inside a SQLite transaction (BEGIN/COMMIT).
    /// Supports nesting: depth 0 uses BEGIN/COMMIT, depth > 0 uses SAVEPOINT/RELEASE.
    /// Automatically rolls back on error.
    pub fn with_transaction<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        let depth = self.txn_depth.get();

        if depth == 0 {
            self.conn.execute_batch("BEGIN").sq()?;
        } else {
            self.conn
                .execute_batch(&format!("SAVEPOINT sp_{depth}"))
                .sq()?;
        }
        self.txn_depth.set(depth + 1);

        match f() {
            Ok(val) => {
                self.txn_depth.set(depth);
                if depth == 0 {
                    self.conn.execute_batch("COMMIT").sq()?;
                } else {
                    self.conn
                        .execute_batch(&format!("RELEASE sp_{depth}"))
                        .sq()?;
                }
                Ok(val)
            }
            Err(e) => {
                self.txn_depth.set(depth);
                if depth == 0 {
                    let _ = self.conn.execute_batch("ROLLBACK");
                } else {
                    let _ = self.conn.execute_batch(&format!("ROLLBACK TO sp_{depth}"));
                }
                Err(e)
            }
        }
    }
}

impl MemoryDb {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Database {
                message: e.to_string(),
                transient: false,
            })?;
        }
        let conn = Connection::open(path).sq()?;

        // WAL mode + relaxed sync for better write throughput
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA wal_autocheckpoint=100;",
        )
        .sq()?;

        let db = Self {
            conn,
            txn_depth: std::cell::Cell::new(0),
        };

        // Skip full DDL if schema is already at the current version
        let stored_version: Option<u32> = db
            .conn
            .query_row(
                "SELECT value FROM flowforge_meta WHERE key = 'schema_version'",
                [],
                |row| {
                    let s: String = row.get(0)?;
                    Ok(s.parse::<u32>().unwrap_or(0))
                },
            )
            .optional()
            .unwrap_or(None);

        if stored_version != Some(SCHEMA_VERSION) {
            db.init_schema()?;
            // Stamp version after successful init
            db.conn
                .execute(
                    "INSERT OR REPLACE INTO flowforge_meta (key, value) VALUES ('schema_version', ?1)",
                    params![SCHEMA_VERSION.to_string()],
                )
                .sq()?;
        } else {
            // Still need foreign keys even when skipping DDL
            db.conn.execute_batch("PRAGMA foreign_keys = ON").sq()?;
        }

        Ok(db)
    }
}
