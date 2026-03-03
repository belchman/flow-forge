use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use flowforge_core::{
    work_tracking::WorkDb, AgentSession, AgentSessionStatus, Checkpoint, ConversationMessage,
    EditRecord, Error, LongTermPattern, MailboxMessage, Result, RoutingWeight, SessionFork,
    SessionInfo, ShortTermPattern, WorkEvent, WorkFilter, WorkItem,
};

/// (db_id, source_type, source_id, vector)
type VectorEntry = (i64, String, String, Vec<f32>);

pub struct MemoryDb {
    conn: Connection,
}

impl MemoryDb {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Sqlite(e.to_string()))?;
        }
        let conn = Connection::open(path).map_err(|e| Error::Sqlite(e.to_string()))?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        // Enable foreign key enforcement (SQLite doesn't enable by default)
        self.conn
            .execute_batch("PRAGMA foreign_keys = ON")
            .map_err(|e| Error::Sqlite(e.to_string()))?;

        self.conn
            .execute_batch(
                "
            CREATE TABLE IF NOT EXISTS key_value (
                key TEXT NOT NULL,
                value TEXT,
                namespace TEXT DEFAULT 'default',
                created_at TEXT,
                updated_at TEXT,
                PRIMARY KEY (key, namespace)
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                started_at TEXT,
                ended_at TEXT,
                cwd TEXT,
                edits INTEGER DEFAULT 0,
                commands INTEGER DEFAULT 0,
                summary TEXT
            );

            CREATE TABLE IF NOT EXISTS edits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT,
                timestamp TEXT,
                file_path TEXT,
                operation TEXT,
                file_extension TEXT
            );

            CREATE TABLE IF NOT EXISTS patterns_short (
                id TEXT PRIMARY KEY,
                content TEXT,
                category TEXT,
                confidence REAL DEFAULT 0.5,
                usage_count INTEGER DEFAULT 0,
                created_at TEXT,
                last_used TEXT,
                embedding_id INTEGER
            );

            CREATE TABLE IF NOT EXISTS patterns_long (
                id TEXT PRIMARY KEY,
                content TEXT,
                category TEXT,
                confidence REAL,
                usage_count INTEGER DEFAULT 0,
                success_count INTEGER DEFAULT 0,
                failure_count INTEGER DEFAULT 0,
                created_at TEXT,
                promoted_at TEXT,
                last_used TEXT,
                embedding_id INTEGER
            );

            CREATE TABLE IF NOT EXISTS hnsw_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_type TEXT,
                source_id TEXT,
                vector BLOB,
                created_at TEXT
            );

            CREATE TABLE IF NOT EXISTS routing_weights (
                task_pattern TEXT,
                agent_name TEXT,
                weight REAL DEFAULT 0.5,
                successes INTEGER DEFAULT 0,
                failures INTEGER DEFAULT 0,
                updated_at TEXT,
                PRIMARY KEY (task_pattern, agent_name)
            );

            CREATE TABLE IF NOT EXISTS work_items (
                id TEXT PRIMARY KEY,
                external_id TEXT,
                backend TEXT NOT NULL,
                item_type TEXT DEFAULT 'task',
                title TEXT NOT NULL,
                description TEXT,
                status TEXT DEFAULT 'pending',
                assignee TEXT,
                parent_id TEXT,
                priority INTEGER DEFAULT 2,
                labels TEXT,
                created_at TEXT,
                updated_at TEXT,
                completed_at TEXT,
                session_id TEXT,
                metadata TEXT
            );

            CREATE TABLE IF NOT EXISTS work_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                work_item_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                old_value TEXT,
                new_value TEXT,
                actor TEXT,
                timestamp TEXT NOT NULL,
                FOREIGN KEY (work_item_id) REFERENCES work_items(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS work_tracking_config (
                key TEXT PRIMARY KEY,
                value TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_work_items_status ON work_items(status);
            CREATE INDEX IF NOT EXISTS idx_work_items_backend ON work_items(backend);
            CREATE INDEX IF NOT EXISTS idx_work_items_parent ON work_items(parent_id);
            CREATE INDEX IF NOT EXISTS idx_work_events_item ON work_events(work_item_id);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_work_items_external_id
                ON work_items(external_id) WHERE external_id IS NOT NULL;

            CREATE TABLE IF NOT EXISTS agent_sessions (
                id TEXT PRIMARY KEY,
                parent_session_id TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                agent_type TEXT NOT NULL DEFAULT 'general',
                status TEXT NOT NULL DEFAULT 'active',
                started_at TEXT NOT NULL,
                ended_at TEXT,
                edits INTEGER DEFAULT 0,
                commands INTEGER DEFAULT 0,
                task_id TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_agent_sessions_parent ON agent_sessions(parent_session_id);
            CREATE INDEX IF NOT EXISTS idx_agent_sessions_agent ON agent_sessions(agent_id);

            CREATE TABLE IF NOT EXISTS conversation_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                message_index INTEGER NOT NULL,
                message_type TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                model TEXT,
                message_id TEXT,
                parent_uuid TEXT,
                timestamp TEXT NOT NULL,
                metadata TEXT,
                source TEXT DEFAULT 'transcript',
                UNIQUE(session_id, message_index)
            );
            CREATE INDEX IF NOT EXISTS idx_conv_session ON conversation_messages(session_id);
            CREATE INDEX IF NOT EXISTS idx_conv_type ON conversation_messages(message_type);
            CREATE INDEX IF NOT EXISTS idx_conv_timestamp ON conversation_messages(timestamp);

            CREATE TABLE IF NOT EXISTS checkpoints (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                name TEXT NOT NULL,
                message_index INTEGER NOT NULL,
                description TEXT,
                git_ref TEXT,
                created_at TEXT NOT NULL,
                metadata TEXT
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_checkpoint_name ON checkpoints(session_id, name);

            CREATE TABLE IF NOT EXISTS session_forks (
                id TEXT PRIMARY KEY,
                source_session_id TEXT NOT NULL,
                target_session_id TEXT NOT NULL,
                fork_message_index INTEGER NOT NULL,
                checkpoint_id TEXT,
                reason TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_forks_source ON session_forks(source_session_id);
            CREATE INDEX IF NOT EXISTS idx_forks_target ON session_forks(target_session_id);

            CREATE TABLE IF NOT EXISTS agent_mailbox (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                work_item_id TEXT NOT NULL,
                from_session_id TEXT NOT NULL,
                from_agent_name TEXT NOT NULL,
                to_session_id TEXT,
                to_agent_name TEXT,
                message_type TEXT DEFAULT 'text',
                content TEXT NOT NULL,
                priority INTEGER DEFAULT 2,
                read_at TEXT,
                created_at TEXT NOT NULL,
                metadata TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_mailbox_work ON agent_mailbox(work_item_id);
            CREATE INDEX IF NOT EXISTS idx_mailbox_to ON agent_mailbox(to_session_id);
            CREATE INDEX IF NOT EXISTS idx_mailbox_unread ON agent_mailbox(to_session_id, read_at);
            CREATE INDEX IF NOT EXISTS idx_mailbox_from ON agent_mailbox(from_session_id);
        ",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;

        // Migrations: add columns to existing tables if missing
        self.migrate_add_column("sessions", "transcript_path", "TEXT")?;
        self.migrate_add_column("agent_sessions", "transcript_path", "TEXT")?;

        Ok(())
    }

    fn migrate_add_column(&self, table: &str, column: &str, col_type: &str) -> Result<()> {
        let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {col_type}");
        match self.conn.execute_batch(&sql) {
            Ok(()) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("duplicate column name") || msg.contains("already exists") {
                    Ok(()) // column already present
                } else {
                    Err(Error::Sqlite(msg))
                }
            }
        }
    }

    // ── Key-Value ──

    pub fn kv_get(&self, key: &str, namespace: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM key_value WHERE key = ?1 AND namespace = ?2",
                params![key, namespace],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn kv_set(&self, key: &str, value: &str, namespace: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO key_value (key, value, namespace, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)
                 ON CONFLICT(key, namespace) DO UPDATE SET value = ?2, updated_at = ?4",
                params![key, value, namespace, now],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn kv_delete(&self, key: &str, namespace: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM key_value WHERE key = ?1 AND namespace = ?2",
                params![key, namespace],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn kv_list(&self, namespace: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM key_value WHERE namespace = ?1 ORDER BY key")
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![namespace], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn kv_search(&self, query: &str, limit: usize) -> Result<Vec<(String, String, String)>> {
        let pattern = format!("%{query}%");
        let mut stmt = self
            .conn
            .prepare(
                "SELECT key, value, namespace FROM key_value
                 WHERE key LIKE ?1 OR value LIKE ?1
                 ORDER BY updated_at DESC LIMIT ?2",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![pattern, limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn count_kv(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM key_value", [], |row| row.get(0))
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    // ── Sessions ──

    pub fn create_session(&self, session: &SessionInfo) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO sessions (id, started_at, ended_at, cwd, edits, commands, summary, transcript_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    session.id,
                    session.started_at.to_rfc3339(),
                    session.ended_at.map(|t| t.to_rfc3339()),
                    session.cwd,
                    session.edits,
                    session.commands,
                    session.summary,
                    session.transcript_path,
                ],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn end_session(&self, id: &str, ended_at: DateTime<Utc>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET ended_at = ?1 WHERE id = ?2",
                params![ended_at.to_rfc3339(), id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn get_current_session(&self) -> Result<Option<SessionInfo>> {
        self.conn
            .query_row(
                "SELECT id, started_at, ended_at, cwd, edits, commands, summary, transcript_path
                 FROM sessions WHERE ended_at IS NULL ORDER BY started_at DESC LIMIT 1",
                [],
                |row| {
                    Ok(SessionInfo {
                        id: row.get(0)?,
                        started_at: parse_datetime(row.get::<_, String>(1)?),
                        ended_at: row.get::<_, Option<String>>(2)?.map(parse_datetime),
                        cwd: row.get(3)?,
                        edits: row.get(4)?,
                        commands: row.get(5)?,
                        summary: row.get(6)?,
                        transcript_path: row.get(7).ok().flatten(),
                    })
                },
            )
            .optional()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionInfo>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, started_at, ended_at, cwd, edits, commands, summary, transcript_path
                 FROM sessions ORDER BY started_at DESC LIMIT ?1",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok(SessionInfo {
                    id: row.get(0)?,
                    started_at: parse_datetime(row.get::<_, String>(1)?),
                    ended_at: row.get::<_, Option<String>>(2)?.map(parse_datetime),
                    cwd: row.get(3)?,
                    edits: row.get(4)?,
                    commands: row.get(5)?,
                    summary: row.get(6)?,
                    transcript_path: row.get(7).ok().flatten(),
                })
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn increment_session_edits(&self, session_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET edits = edits + 1 WHERE id = ?1",
                params![session_id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn increment_session_commands(&self, session_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET commands = commands + 1 WHERE id = ?1",
                params![session_id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    // ── Agent Sessions ──

    pub fn create_agent_session(&self, session: &AgentSession) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO agent_sessions
                 (id, parent_session_id, agent_id, agent_type, status, started_at, ended_at, edits, commands, task_id, transcript_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    session.id,
                    session.parent_session_id,
                    session.agent_id,
                    session.agent_type,
                    session.status.to_string(),
                    session.started_at.to_rfc3339(),
                    session.ended_at.map(|t| t.to_rfc3339()),
                    session.edits,
                    session.commands,
                    session.task_id,
                    session.transcript_path,
                ],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn end_agent_session(&self, agent_id: &str, status: AgentSessionStatus) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE agent_sessions SET ended_at = ?1, status = ?2
                 WHERE agent_id = ?3 AND ended_at IS NULL",
                params![now, status.to_string(), agent_id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn update_agent_session_status(
        &self,
        agent_id: &str,
        status: AgentSessionStatus,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE agent_sessions SET status = ?1
                 WHERE agent_id = ?2 AND ended_at IS NULL",
                params![status.to_string(), agent_id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn get_agent_sessions(&self, parent_session_id: &str) -> Result<Vec<AgentSession>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, parent_session_id, agent_id, agent_type, status,
                        started_at, ended_at, edits, commands, task_id, transcript_path
                 FROM agent_sessions WHERE parent_session_id = ?1
                 ORDER BY started_at DESC",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![parent_session_id], |row| {
                Ok(parse_agent_session_row(row))
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_active_agent_sessions(&self) -> Result<Vec<AgentSession>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, parent_session_id, agent_id, agent_type, status,
                        started_at, ended_at, edits, commands, task_id, transcript_path
                 FROM agent_sessions WHERE ended_at IS NULL
                 ORDER BY started_at DESC",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| Ok(parse_agent_session_row(row)))
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn increment_agent_edits(&self, agent_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE agent_sessions SET edits = edits + 1
                 WHERE agent_id = ?1 AND ended_at IS NULL",
                params![agent_id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn increment_agent_commands(&self, agent_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE agent_sessions SET commands = commands + 1
                 WHERE agent_id = ?1 AND ended_at IS NULL",
                params![agent_id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    // ── Edits ──

    pub fn record_edit(&self, edit: &EditRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO edits (session_id, timestamp, file_path, operation, file_extension)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    edit.session_id,
                    edit.timestamp.to_rfc3339(),
                    edit.file_path,
                    edit.operation,
                    edit.file_extension,
                ],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn get_edits_for_session(&self, session_id: &str) -> Result<Vec<EditRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT session_id, timestamp, file_path, operation, file_extension
                 FROM edits WHERE session_id = ?1 ORDER BY timestamp",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                Ok(EditRecord {
                    session_id: row.get(0)?,
                    timestamp: parse_datetime(row.get::<_, String>(1)?),
                    file_path: row.get(2)?,
                    operation: row.get(3)?,
                    file_extension: row.get(4)?,
                })
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn count_edits(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM edits", [], |row| row.get(0))
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    // ── Patterns (Short-term) ──

    pub fn store_pattern_short(&self, pattern: &ShortTermPattern) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO patterns_short
                 (id, content, category, confidence, usage_count, created_at, last_used, embedding_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    pattern.id,
                    pattern.content,
                    pattern.category,
                    pattern.confidence,
                    pattern.usage_count,
                    pattern.created_at.to_rfc3339(),
                    pattern.last_used.to_rfc3339(),
                    pattern.embedding_id,
                ],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn search_patterns_short(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ShortTermPattern>> {
        let pattern = format!("%{query}%");
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, content, category, confidence, usage_count, created_at, last_used, embedding_id
                 FROM patterns_short WHERE content LIKE ?1 OR category LIKE ?1
                 ORDER BY confidence DESC LIMIT ?2",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![pattern, limit], |row| {
                Ok(ShortTermPattern {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    category: row.get(2)?,
                    confidence: row.get(3)?,
                    usage_count: row.get(4)?,
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                    last_used: parse_datetime(row.get::<_, String>(6)?),
                    embedding_id: row.get(7)?,
                })
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_all_patterns_short(&self) -> Result<Vec<ShortTermPattern>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, content, category, confidence, usage_count, created_at, last_used, embedding_id
                 FROM patterns_short ORDER BY last_used DESC",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ShortTermPattern {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    category: row.get(2)?,
                    confidence: row.get(3)?,
                    usage_count: row.get(4)?,
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                    last_used: parse_datetime(row.get::<_, String>(6)?),
                    embedding_id: row.get(7)?,
                })
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn update_pattern_short_usage(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE patterns_short SET usage_count = usage_count + 1, last_used = ?1,
                 confidence = MIN(1.0, confidence + 0.05) WHERE id = ?2",
                params![now, id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn delete_pattern_short(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM patterns_short WHERE id = ?1", params![id])
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn count_patterns_short(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM patterns_short", [], |row| row.get(0))
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    // ── Patterns (Long-term) ──

    pub fn store_pattern_long(&self, pattern: &LongTermPattern) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO patterns_long
                 (id, content, category, confidence, usage_count, success_count, failure_count,
                  created_at, promoted_at, last_used, embedding_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    pattern.id,
                    pattern.content,
                    pattern.category,
                    pattern.confidence,
                    pattern.usage_count,
                    pattern.success_count,
                    pattern.failure_count,
                    pattern.created_at.to_rfc3339(),
                    pattern.promoted_at.to_rfc3339(),
                    pattern.last_used.to_rfc3339(),
                    pattern.embedding_id,
                ],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn search_patterns_long(&self, query: &str, limit: usize) -> Result<Vec<LongTermPattern>> {
        let pattern = format!("%{query}%");
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, content, category, confidence, usage_count, success_count, failure_count,
                        created_at, promoted_at, last_used, embedding_id
                 FROM patterns_long WHERE content LIKE ?1 OR category LIKE ?1
                 ORDER BY confidence DESC LIMIT ?2",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![pattern, limit], |row| {
                Ok(LongTermPattern {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    category: row.get(2)?,
                    confidence: row.get(3)?,
                    usage_count: row.get(4)?,
                    success_count: row.get(5)?,
                    failure_count: row.get(6)?,
                    created_at: parse_datetime(row.get::<_, String>(7)?),
                    promoted_at: parse_datetime(row.get::<_, String>(8)?),
                    last_used: parse_datetime(row.get::<_, String>(9)?),
                    embedding_id: row.get(10)?,
                })
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_pattern_long(&self, id: &str) -> Result<Option<LongTermPattern>> {
        self.conn
            .query_row(
                "SELECT id, content, category, confidence, usage_count, success_count, failure_count,
                        created_at, promoted_at, last_used, embedding_id
                 FROM patterns_long WHERE id = ?1",
                params![id],
                |row| {
                    Ok(LongTermPattern {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        category: row.get(2)?,
                        confidence: row.get(3)?,
                        usage_count: row.get(4)?,
                        success_count: row.get(5)?,
                        failure_count: row.get(6)?,
                        created_at: parse_datetime(row.get::<_, String>(7)?),
                        promoted_at: parse_datetime(row.get::<_, String>(8)?),
                        last_used: parse_datetime(row.get::<_, String>(9)?),
                        embedding_id: row.get(10)?,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_pattern_short(&self, id: &str) -> Result<Option<ShortTermPattern>> {
        self.conn
            .query_row(
                "SELECT id, content, category, confidence, usage_count, created_at, last_used, embedding_id
                 FROM patterns_short WHERE id = ?1",
                params![id],
                |row| {
                    Ok(ShortTermPattern {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        category: row.get(2)?,
                        confidence: row.get(3)?,
                        usage_count: row.get(4)?,
                        created_at: parse_datetime(row.get::<_, String>(5)?),
                        last_used: parse_datetime(row.get::<_, String>(6)?),
                        embedding_id: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn update_pattern_long_feedback(&self, id: &str, success: bool) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        if success {
            self.conn
                .execute(
                    "UPDATE patterns_long SET success_count = success_count + 1,
                     confidence = MIN(1.0, confidence + 0.03), last_used = ?1 WHERE id = ?2",
                    params![now, id],
                )
                .map_err(|e| Error::Sqlite(e.to_string()))?;
        } else {
            self.conn
                .execute(
                    "UPDATE patterns_long SET failure_count = failure_count + 1,
                     confidence = MAX(0.0, confidence - 0.05), last_used = ?1 WHERE id = ?2",
                    params![now, id],
                )
                .map_err(|e| Error::Sqlite(e.to_string()))?;
        }
        Ok(())
    }

    pub fn update_pattern_short_confidence(&self, id: &str, confidence: f64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE patterns_short SET confidence = ?1 WHERE id = ?2",
                params![confidence, id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn update_pattern_long_confidence(&self, id: &str, confidence: f64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE patterns_long SET confidence = ?1 WHERE id = ?2",
                params![confidence, id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn get_all_patterns_long(&self) -> Result<Vec<LongTermPattern>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, content, category, confidence, usage_count, success_count, failure_count,
                        created_at, promoted_at, last_used, embedding_id
                 FROM patterns_long ORDER BY last_used DESC",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(LongTermPattern {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    category: row.get(2)?,
                    confidence: row.get(3)?,
                    usage_count: row.get(4)?,
                    success_count: row.get(5)?,
                    failure_count: row.get(6)?,
                    created_at: parse_datetime(row.get::<_, String>(7)?),
                    promoted_at: parse_datetime(row.get::<_, String>(8)?),
                    last_used: parse_datetime(row.get::<_, String>(9)?),
                    embedding_id: row.get(10)?,
                })
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_vectors_for_source(
        &self,
        source_type: &str,
    ) -> Result<Vec<(i64, String, Vec<f32>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source_id, vector FROM hnsw_entries WHERE source_type = ?1")
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![source_type], |row| {
                let blob: Vec<u8> = row.get(2)?;
                Ok((row.get(0)?, row.get(1)?, blob_to_vector(&blob)))
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn count_patterns_long(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM patterns_long", [], |row| row.get(0))
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn count_patterns(&self) -> Result<u64> {
        let short = self.count_patterns_short()?;
        let long = self.count_patterns_long()?;
        Ok(short + long)
    }

    pub fn get_top_patterns(&self, limit: usize) -> Result<Vec<ShortTermPattern>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, content, category, confidence, usage_count, created_at, last_used, embedding_id
                 FROM patterns_short ORDER BY confidence DESC, usage_count DESC LIMIT ?1",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok(ShortTermPattern {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    category: row.get(2)?,
                    confidence: row.get(3)?,
                    usage_count: row.get(4)?,
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                    last_used: parse_datetime(row.get::<_, String>(6)?),
                    embedding_id: row.get(7)?,
                })
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    // ── Routing Weights ──

    pub fn get_routing_weight(
        &self,
        task_pattern: &str,
        agent_name: &str,
    ) -> Result<Option<RoutingWeight>> {
        self.conn
            .query_row(
                "SELECT task_pattern, agent_name, weight, successes, failures, updated_at
                 FROM routing_weights WHERE task_pattern = ?1 AND agent_name = ?2",
                params![task_pattern, agent_name],
                |row| {
                    Ok(RoutingWeight {
                        task_pattern: row.get(0)?,
                        agent_name: row.get(1)?,
                        weight: row.get(2)?,
                        successes: row.get(3)?,
                        failures: row.get(4)?,
                        updated_at: parse_datetime(row.get::<_, String>(5)?),
                    })
                },
            )
            .optional()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_all_routing_weights(&self) -> Result<Vec<RoutingWeight>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT task_pattern, agent_name, weight, successes, failures, updated_at
                 FROM routing_weights ORDER BY weight DESC",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(RoutingWeight {
                    task_pattern: row.get(0)?,
                    agent_name: row.get(1)?,
                    weight: row.get(2)?,
                    successes: row.get(3)?,
                    failures: row.get(4)?,
                    updated_at: parse_datetime(row.get::<_, String>(5)?),
                })
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn record_routing_success(&self, task_pattern: &str, agent_name: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO routing_weights (task_pattern, agent_name, weight, successes, failures, updated_at)
                 VALUES (?1, ?2, 0.6, 1, 0, ?3)
                 ON CONFLICT(task_pattern, agent_name) DO UPDATE SET
                   successes = successes + 1,
                   weight = MIN(1.0, weight + 0.05),
                   updated_at = ?3",
                params![task_pattern, agent_name, now],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn record_routing_failure(&self, task_pattern: &str, agent_name: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO routing_weights (task_pattern, agent_name, weight, successes, failures, updated_at)
                 VALUES (?1, ?2, 0.4, 0, 1, ?3)
                 ON CONFLICT(task_pattern, agent_name) DO UPDATE SET
                   failures = failures + 1,
                   weight = MAX(0.0, weight - 0.05),
                   updated_at = ?3",
                params![task_pattern, agent_name, now],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn count_routing_weights(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM routing_weights", [], |row| row.get(0))
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    // ── HNSW Entries ──

    pub fn store_vector(&self, source_type: &str, source_id: &str, vector: &[f32]) -> Result<i64> {
        let blob = vector_to_blob(vector);
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO hnsw_entries (source_type, source_id, vector, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![source_type, source_id, blob, now],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_all_vectors(&self) -> Result<Vec<VectorEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source_type, source_id, vector FROM hnsw_entries")
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let blob: Vec<u8> = row.get(3)?;
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, blob_to_vector(&blob)))
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn delete_vectors_for_source(&self, source_type: &str, source_id: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM hnsw_entries WHERE source_type = ?1 AND source_id = ?2",
                params![source_type, source_id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    // ── Work Items ──

    pub fn create_work_item(&self, item: &WorkItem) -> Result<()> {
        let labels_json = serde_json::to_string(&item.labels).unwrap_or_else(|_| "[]".to_string());
        self.conn
            .execute(
                "INSERT OR REPLACE INTO work_items
                 (id, external_id, backend, item_type, title, description, status, assignee,
                  parent_id, priority, labels, created_at, updated_at, completed_at, session_id, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    item.id,
                    item.external_id,
                    item.backend,
                    item.item_type,
                    item.title,
                    item.description,
                    item.status,
                    item.assignee,
                    item.parent_id,
                    item.priority,
                    labels_json,
                    item.created_at.to_rfc3339(),
                    item.updated_at.to_rfc3339(),
                    item.completed_at.map(|t| t.to_rfc3339()),
                    item.session_id,
                    item.metadata,
                ],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn get_work_item(&self, id: &str) -> Result<Option<WorkItem>> {
        self.conn
            .query_row(
                "SELECT id, external_id, backend, item_type, title, description, status, assignee,
                        parent_id, priority, labels, created_at, updated_at, completed_at, session_id, metadata
                 FROM work_items WHERE id = ?1",
                params![id],
                |row| Ok(parse_work_item_row(row)),
            )
            .optional()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_work_item_by_external_id(&self, external_id: &str) -> Result<Option<WorkItem>> {
        self.conn
            .query_row(
                "SELECT id, external_id, backend, item_type, title, description, status, assignee,
                        parent_id, priority, labels, created_at, updated_at, completed_at, session_id, metadata
                 FROM work_items WHERE external_id = ?1",
                params![external_id],
                |row| Ok(parse_work_item_row(row)),
            )
            .optional()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn update_work_item_status(&self, id: &str, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let completed_at = if status == "completed" {
            Some(now.clone())
        } else {
            None
        };
        self.conn
            .execute(
                "UPDATE work_items SET status = ?1, updated_at = ?2, completed_at = COALESCE(?3, completed_at)
                 WHERE id = ?4",
                params![status, now, completed_at, id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn update_work_item_assignee(&self, id: &str, assignee: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE work_items SET assignee = ?1, updated_at = ?2 WHERE id = ?3",
                params![assignee, now, id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn list_work_items(&self, filter: &WorkFilter) -> Result<Vec<WorkItem>> {
        let mut sql = String::from(
            "SELECT id, external_id, backend, item_type, title, description, status, assignee,
                    parent_id, priority, labels, created_at, updated_at, completed_at, session_id, metadata
             FROM work_items WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref status) = filter.status {
            param_values.push(Box::new(status.clone()));
            sql.push_str(&format!(" AND status = ?{}", param_values.len()));
        }
        if let Some(ref item_type) = filter.item_type {
            param_values.push(Box::new(item_type.clone()));
            sql.push_str(&format!(" AND item_type = ?{}", param_values.len()));
        }
        if let Some(ref backend) = filter.backend {
            param_values.push(Box::new(backend.clone()));
            sql.push_str(&format!(" AND backend = ?{}", param_values.len()));
        }
        if let Some(ref assignee) = filter.assignee {
            param_values.push(Box::new(assignee.clone()));
            sql.push_str(&format!(" AND assignee = ?{}", param_values.len()));
        }
        if let Some(ref parent_id) = filter.parent_id {
            param_values.push(Box::new(parent_id.clone()));
            sql.push_str(&format!(" AND parent_id = ?{}", param_values.len()));
        }

        sql.push_str(" ORDER BY updated_at DESC");

        let limit = filter.limit.unwrap_or(100);
        param_values.push(Box::new(limit as i64));
        sql.push_str(&format!(" LIMIT ?{}", param_values.len()));

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| Error::Sqlite(e.to_string()))?;

        let params_slice: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(params_slice.as_slice(), |row| Ok(parse_work_item_row(row)))
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn update_work_item_backend(&self, id: &str, backend: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE work_items SET backend = ?1, updated_at = ?2 WHERE id = ?3",
                params![backend, now, id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn delete_work_item(&self, id: &str) -> Result<()> {
        self.conn
            .execute_batch("BEGIN")
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let result = (|| {
            self.conn
                .execute(
                    "DELETE FROM work_events WHERE work_item_id = ?1",
                    params![id],
                )
                .map_err(|e| Error::Sqlite(e.to_string()))?;
            self.conn
                .execute("DELETE FROM work_items WHERE id = ?1", params![id])
                .map_err(|e| Error::Sqlite(e.to_string()))?;
            Ok(())
        })();
        if result.is_ok() {
            self.conn
                .execute_batch("COMMIT")
                .map_err(|e| Error::Sqlite(e.to_string()))?;
        } else {
            self.conn
                .execute_batch("ROLLBACK")
                .map_err(|e| Error::Sqlite(e.to_string()))?;
        }
        result
    }

    pub fn count_work_items_by_status(&self, status: &str) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM work_items WHERE status = ?1",
                params![status],
                |row| row.get(0),
            )
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    // ── Work Events ──

    pub fn record_work_event(&self, event: &WorkEvent) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO work_events (work_item_id, event_type, old_value, new_value, actor, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    event.work_item_id,
                    event.event_type,
                    event.old_value,
                    event.new_value,
                    event.actor,
                    event.timestamp.to_rfc3339(),
                ],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_work_events(&self, work_item_id: &str, limit: usize) -> Result<Vec<WorkEvent>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, work_item_id, event_type, old_value, new_value, actor, timestamp
                 FROM work_events WHERE work_item_id = ?1 ORDER BY timestamp DESC LIMIT ?2",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![work_item_id, limit], |row| {
                Ok(WorkEvent {
                    id: row.get(0)?,
                    work_item_id: row.get(1)?,
                    event_type: row.get(2)?,
                    old_value: row.get(3)?,
                    new_value: row.get(4)?,
                    actor: row.get(5)?,
                    timestamp: parse_datetime(row.get::<_, String>(6)?),
                })
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_recent_work_events(&self, limit: usize) -> Result<Vec<WorkEvent>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, work_item_id, event_type, old_value, new_value, actor, timestamp
                 FROM work_events ORDER BY timestamp DESC LIMIT ?1",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok(WorkEvent {
                    id: row.get(0)?,
                    work_item_id: row.get(1)?,
                    event_type: row.get(2)?,
                    old_value: row.get(3)?,
                    new_value: row.get(4)?,
                    actor: row.get(5)?,
                    timestamp: parse_datetime(row.get::<_, String>(6)?),
                })
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_recent_work_events_since(
        &self,
        since: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<WorkEvent>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, work_item_id, event_type, old_value, new_value, actor, timestamp
                 FROM work_events WHERE timestamp >= ?1 ORDER BY timestamp DESC LIMIT ?2",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![since.to_rfc3339(), limit], |row| {
                Ok(WorkEvent {
                    id: row.get(0)?,
                    work_item_id: row.get(1)?,
                    event_type: row.get(2)?,
                    old_value: row.get(3)?,
                    new_value: row.get(4)?,
                    actor: row.get(5)?,
                    timestamp: parse_datetime(row.get::<_, String>(6)?),
                })
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    // ── Work Tracking Config ──

    pub fn get_work_config(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM work_tracking_config WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn set_work_config(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO work_tracking_config (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    // ── Session Transcript Path ──

    pub fn update_session_transcript_path(&self, session_id: &str, path: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET transcript_path = ?1 WHERE id = ?2",
                params![path, session_id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn update_agent_session_transcript_path(&self, agent_id: &str, path: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE agent_sessions SET transcript_path = ?1
                 WHERE agent_id = ?2 AND ended_at IS NULL",
                params![path, agent_id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    // ── Conversation Messages ──

    pub fn store_conversation_message(&self, msg: &ConversationMessage) -> Result<i64> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO conversation_messages
                 (session_id, message_index, message_type, role, content, model,
                  message_id, parent_uuid, timestamp, metadata, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    msg.session_id,
                    msg.message_index,
                    msg.message_type,
                    msg.role,
                    msg.content,
                    msg.model,
                    msg.message_id,
                    msg.parent_uuid,
                    msg.timestamp.to_rfc3339(),
                    msg.metadata,
                    msg.source,
                ],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn ingest_transcript(&self, session_id: &str, transcript_path: &str) -> Result<u32> {
        let latest = self.get_latest_message_index(session_id)?;
        let messages = flowforge_core::transcript::parse_transcript(transcript_path, session_id)?;

        let mut count = 0u32;
        for msg in &messages {
            if msg.message_index >= latest {
                self.store_conversation_message(msg)?;
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn get_conversation_messages(
        &self,
        session_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ConversationMessage>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, message_index, message_type, role, content,
                        model, message_id, parent_uuid, timestamp, metadata, source
                 FROM conversation_messages WHERE session_id = ?1
                 ORDER BY message_index ASC LIMIT ?2 OFFSET ?3",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![session_id, limit, offset], |row| {
                Ok(parse_conversation_message_row(row))
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_conversation_message_count(&self, session_id: &str) -> Result<u32> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM conversation_messages WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_conversation_messages_range(
        &self,
        session_id: &str,
        from: u32,
        to: u32,
    ) -> Result<Vec<ConversationMessage>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, message_index, message_type, role, content,
                        model, message_id, parent_uuid, timestamp, metadata, source
                 FROM conversation_messages
                 WHERE session_id = ?1 AND message_index >= ?2 AND message_index <= ?3
                 ORDER BY message_index ASC",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![session_id, from, to], |row| {
                Ok(parse_conversation_message_row(row))
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_latest_message_index(&self, session_id: &str) -> Result<u32> {
        self.conn
            .query_row(
                "SELECT COALESCE(MAX(message_index) + 1, 0) FROM conversation_messages WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn search_conversation_messages(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ConversationMessage>> {
        let pattern = format!("%{query}%");
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, message_index, message_type, role, content,
                        model, message_id, parent_uuid, timestamp, metadata, source
                 FROM conversation_messages
                 WHERE session_id = ?1 AND content LIKE ?2
                 ORDER BY message_index ASC LIMIT ?3",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![session_id, pattern, limit], |row| {
                Ok(parse_conversation_message_row(row))
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    // ── Checkpoints ──

    pub fn create_checkpoint(&self, cp: &Checkpoint) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO checkpoints (id, session_id, name, message_index, description, git_ref, created_at, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    cp.id,
                    cp.session_id,
                    cp.name,
                    cp.message_index,
                    cp.description,
                    cp.git_ref,
                    cp.created_at.to_rfc3339(),
                    cp.metadata,
                ],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn get_checkpoint(&self, id: &str) -> Result<Option<Checkpoint>> {
        self.conn
            .query_row(
                "SELECT id, session_id, name, message_index, description, git_ref, created_at, metadata
                 FROM checkpoints WHERE id = ?1",
                params![id],
                |row| Ok(parse_checkpoint_row(row)),
            )
            .optional()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_checkpoint_by_name(
        &self,
        session_id: &str,
        name: &str,
    ) -> Result<Option<Checkpoint>> {
        self.conn
            .query_row(
                "SELECT id, session_id, name, message_index, description, git_ref, created_at, metadata
                 FROM checkpoints WHERE session_id = ?1 AND name = ?2",
                params![session_id, name],
                |row| Ok(parse_checkpoint_row(row)),
            )
            .optional()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn list_checkpoints(&self, session_id: &str) -> Result<Vec<Checkpoint>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, name, message_index, description, git_ref, created_at, metadata
                 FROM checkpoints WHERE session_id = ?1 ORDER BY message_index ASC",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![session_id], |row| Ok(parse_checkpoint_row(row)))
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn delete_checkpoint(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM checkpoints WHERE id = ?1", params![id])
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    // ── Session Forks ──

    pub fn fork_conversation(
        &self,
        source_id: &str,
        target_id: &str,
        up_to_index: u32,
    ) -> Result<u32> {
        let count: u32 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM conversation_messages
                 WHERE session_id = ?1 AND message_index <= ?2",
                params![source_id, up_to_index],
                |row| row.get(0),
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;

        self.conn
            .execute(
                "INSERT OR IGNORE INTO conversation_messages
                 (session_id, message_index, message_type, role, content, model,
                  message_id, parent_uuid, timestamp, metadata, source)
                 SELECT ?1, message_index, message_type, role, content, model,
                        message_id, parent_uuid, timestamp, metadata, 'forked'
                 FROM conversation_messages
                 WHERE session_id = ?2 AND message_index <= ?3",
                params![target_id, source_id, up_to_index],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;

        Ok(count)
    }

    pub fn create_session_fork(&self, fork: &SessionFork) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO session_forks
                 (id, source_session_id, target_session_id, fork_message_index,
                  checkpoint_id, reason, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    fork.id,
                    fork.source_session_id,
                    fork.target_session_id,
                    fork.fork_message_index,
                    fork.checkpoint_id,
                    fork.reason,
                    fork.created_at.to_rfc3339(),
                ],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn get_session_forks(&self, session_id: &str) -> Result<Vec<SessionFork>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source_session_id, target_session_id, fork_message_index,
                        checkpoint_id, reason, created_at
                 FROM session_forks
                 WHERE source_session_id = ?1 OR target_session_id = ?1
                 ORDER BY created_at DESC",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![session_id], |row| Ok(parse_session_fork_row(row)))
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_session_lineage(&self, session_id: &str) -> Result<Vec<SessionFork>> {
        // Trace fork chain to root: follow source_session_id backwards
        let mut lineage = Vec::new();
        let mut current = session_id.to_string();
        for _ in 0..50 {
            // safety limit
            let fork: Option<SessionFork> = self
                .conn
                .query_row(
                    "SELECT id, source_session_id, target_session_id, fork_message_index,
                            checkpoint_id, reason, created_at
                     FROM session_forks WHERE target_session_id = ?1",
                    params![current],
                    |row| Ok(parse_session_fork_row(row)),
                )
                .optional()
                .map_err(|e| Error::Sqlite(e.to_string()))?;
            match fork {
                Some(f) => {
                    current = f.source_session_id.clone();
                    lineage.push(f);
                }
                None => break,
            }
        }
        lineage.reverse();
        Ok(lineage)
    }

    // ── Agent Mailbox ──

    pub fn send_mailbox_message(&self, msg: &MailboxMessage) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO agent_mailbox
                 (work_item_id, from_session_id, from_agent_name, to_session_id, to_agent_name,
                  message_type, content, priority, created_at, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    msg.work_item_id,
                    msg.from_session_id,
                    msg.from_agent_name,
                    msg.to_session_id,
                    msg.to_agent_name,
                    msg.message_type,
                    msg.content,
                    msg.priority,
                    msg.created_at.to_rfc3339(),
                    msg.metadata,
                ],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_unread_messages(&self, session_id: &str) -> Result<Vec<MailboxMessage>> {
        // Get messages targeted at this session OR broadcasts (to_session_id IS NULL)
        // for work items this agent is on
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, work_item_id, from_session_id, from_agent_name, to_session_id,
                        to_agent_name, message_type, content, priority, read_at, created_at, metadata
                 FROM agent_mailbox
                 WHERE read_at IS NULL
                   AND (to_session_id = ?1 OR (to_session_id IS NULL AND from_session_id != ?1))
                 ORDER BY priority ASC, created_at ASC",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                Ok(parse_mailbox_message_row(row))
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn mark_messages_read(&self, session_id: &str) -> Result<u32> {
        let now = Utc::now().to_rfc3339();
        let count = self
            .conn
            .execute(
                "UPDATE agent_mailbox SET read_at = ?1
                 WHERE read_at IS NULL
                   AND (to_session_id = ?2 OR (to_session_id IS NULL AND from_session_id != ?2))",
                params![now, session_id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(count as u32)
    }

    pub fn mark_message_read(&self, id: i64) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE agent_mailbox SET read_at = ?1 WHERE id = ?2",
                params![now, id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn get_mailbox_history(
        &self,
        work_item_id: &str,
        limit: usize,
    ) -> Result<Vec<MailboxMessage>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, work_item_id, from_session_id, from_agent_name, to_session_id,
                        to_agent_name, message_type, content, priority, read_at, created_at, metadata
                 FROM agent_mailbox WHERE work_item_id = ?1
                 ORDER BY created_at DESC LIMIT ?2",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![work_item_id, limit], |row| {
                Ok(parse_mailbox_message_row(row))
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn get_agents_on_work_item(&self, work_item_id: &str) -> Result<Vec<AgentSession>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, parent_session_id, agent_id, agent_type, status,
                        started_at, ended_at, edits, commands, task_id, transcript_path
                 FROM agent_sessions WHERE task_id = ?1
                 ORDER BY started_at DESC",
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![work_item_id], |row| {
                Ok(parse_agent_session_row(row))
            })
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Sqlite(e.to_string()))
    }

    pub fn update_agent_session_work_item(&self, agent_id: &str, work_item_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE agent_sessions SET task_id = ?1
                 WHERE agent_id = ?2 AND ended_at IS NULL",
                params![work_item_id, agent_id],
            )
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        Ok(())
    }
}

impl WorkDb for MemoryDb {
    fn create_work_item(&self, item: &WorkItem) -> Result<()> {
        self.create_work_item(item)
    }
    fn get_work_item(&self, id: &str) -> Result<Option<WorkItem>> {
        self.get_work_item(id)
    }
    fn get_work_item_by_external_id(&self, external_id: &str) -> Result<Option<WorkItem>> {
        self.get_work_item_by_external_id(external_id)
    }
    fn update_work_item_status(&self, id: &str, status: &str) -> Result<()> {
        self.update_work_item_status(id, status)
    }
    fn update_work_item_assignee(&self, id: &str, assignee: &str) -> Result<()> {
        self.update_work_item_assignee(id, assignee)
    }
    fn update_work_item_backend(&self, id: &str, backend: &str) -> Result<()> {
        self.update_work_item_backend(id, backend)
    }
    fn list_work_items(&self, filter: &WorkFilter) -> Result<Vec<WorkItem>> {
        self.list_work_items(filter)
    }
    fn delete_work_item(&self, id: &str) -> Result<()> {
        self.delete_work_item(id)
    }
    fn count_work_items_by_status(&self, status: &str) -> Result<u64> {
        self.count_work_items_by_status(status)
    }
    fn record_work_event(&self, event: &WorkEvent) -> Result<i64> {
        self.record_work_event(event)
    }
    fn get_work_events(&self, work_item_id: &str, limit: usize) -> Result<Vec<WorkEvent>> {
        self.get_work_events(work_item_id, limit)
    }
    fn get_recent_work_events(&self, limit: usize) -> Result<Vec<WorkEvent>> {
        self.get_recent_work_events(limit)
    }
}

fn parse_conversation_message_row(row: &rusqlite::Row) -> ConversationMessage {
    ConversationMessage {
        id: row.get(0).unwrap_or(0),
        session_id: row.get(1).unwrap_or_default(),
        message_index: row.get(2).unwrap_or(0),
        message_type: row.get(3).unwrap_or_default(),
        role: row.get(4).unwrap_or_default(),
        content: row.get(5).unwrap_or_default(),
        model: row.get(6).ok().flatten(),
        message_id: row.get(7).ok().flatten(),
        parent_uuid: row.get(8).ok().flatten(),
        timestamp: parse_datetime(row.get::<_, String>(9).unwrap_or_default()),
        metadata: row.get(10).ok().flatten(),
        source: row.get(11).unwrap_or_else(|_| "transcript".to_string()),
    }
}

fn parse_checkpoint_row(row: &rusqlite::Row) -> Checkpoint {
    Checkpoint {
        id: row.get(0).unwrap_or_default(),
        session_id: row.get(1).unwrap_or_default(),
        name: row.get(2).unwrap_or_default(),
        message_index: row.get(3).unwrap_or(0),
        description: row.get(4).ok().flatten(),
        git_ref: row.get(5).ok().flatten(),
        created_at: parse_datetime(row.get::<_, String>(6).unwrap_or_default()),
        metadata: row.get(7).ok().flatten(),
    }
}

fn parse_session_fork_row(row: &rusqlite::Row) -> SessionFork {
    SessionFork {
        id: row.get(0).unwrap_or_default(),
        source_session_id: row.get(1).unwrap_or_default(),
        target_session_id: row.get(2).unwrap_or_default(),
        fork_message_index: row.get(3).unwrap_or(0),
        checkpoint_id: row.get(4).ok().flatten(),
        reason: row.get(5).ok().flatten(),
        created_at: parse_datetime(row.get::<_, String>(6).unwrap_or_default()),
    }
}

fn parse_mailbox_message_row(row: &rusqlite::Row) -> MailboxMessage {
    MailboxMessage {
        id: row.get(0).unwrap_or(0),
        work_item_id: row.get(1).unwrap_or_default(),
        from_session_id: row.get(2).unwrap_or_default(),
        from_agent_name: row.get(3).unwrap_or_default(),
        to_session_id: row.get(4).ok().flatten(),
        to_agent_name: row.get(5).ok().flatten(),
        message_type: row.get(6).unwrap_or_else(|_| "text".to_string()),
        content: row.get(7).unwrap_or_default(),
        priority: row.get(8).unwrap_or(2),
        read_at: row
            .get::<_, Option<String>>(9)
            .ok()
            .flatten()
            .map(parse_datetime),
        created_at: parse_datetime(row.get::<_, String>(10).unwrap_or_default()),
        metadata: row.get(11).ok().flatten(),
    }
}

fn parse_work_item_row(row: &rusqlite::Row) -> WorkItem {
    let labels_str: String = row
        .get::<_, String>(10)
        .unwrap_or_else(|_| "[]".to_string());
    let labels: Vec<String> = serde_json::from_str(&labels_str).unwrap_or_default();
    WorkItem {
        id: row.get(0).unwrap_or_default(),
        external_id: row.get(1).unwrap_or_default(),
        backend: row.get(2).unwrap_or_default(),
        item_type: row.get(3).unwrap_or_else(|_| "task".to_string()),
        title: row.get(4).unwrap_or_default(),
        description: row.get(5).unwrap_or_default(),
        status: row.get(6).unwrap_or_else(|_| "pending".to_string()),
        assignee: row.get(7).unwrap_or_default(),
        parent_id: row.get(8).unwrap_or_default(),
        priority: row.get(9).unwrap_or(2),
        labels,
        created_at: parse_datetime(row.get::<_, String>(11).unwrap_or_default()),
        updated_at: parse_datetime(row.get::<_, String>(12).unwrap_or_default()),
        completed_at: row
            .get::<_, Option<String>>(13)
            .unwrap_or_default()
            .map(parse_datetime),
        session_id: row.get(14).unwrap_or_default(),
        metadata: row.get(15).unwrap_or_default(),
    }
}

fn parse_agent_session_row(row: &rusqlite::Row) -> AgentSession {
    AgentSession {
        id: row.get(0).unwrap_or_default(),
        parent_session_id: row.get(1).unwrap_or_default(),
        agent_id: row.get(2).unwrap_or_default(),
        agent_type: row.get(3).unwrap_or_default(),
        status: row
            .get::<_, String>(4)
            .unwrap_or_default()
            .parse()
            .unwrap_or(AgentSessionStatus::Active),
        started_at: parse_datetime(row.get::<_, String>(5).unwrap_or_default()),
        ended_at: row
            .get::<_, Option<String>>(6)
            .ok()
            .flatten()
            .map(parse_datetime),
        edits: row.get(7).unwrap_or(0),
        commands: row.get(8).unwrap_or(0),
        task_id: row.get(9).ok().flatten(),
        transcript_path: row.get(10).ok().flatten(),
    }
}

fn parse_datetime(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn vector_to_blob(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn blob_to_vector(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_db() -> MemoryDb {
        MemoryDb::open(Path::new(":memory:")).unwrap()
    }

    fn test_work_item(id: &str, title: &str) -> WorkItem {
        WorkItem {
            id: id.to_string(),
            external_id: None,
            backend: "flowforge".to_string(),
            item_type: "task".to_string(),
            title: title.to_string(),
            description: None,
            status: "pending".to_string(),
            assignee: None,
            parent_id: None,
            priority: 2,
            labels: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            session_id: None,
            metadata: None,
        }
    }

    #[test]
    fn test_work_item_crud() {
        let db = test_db();
        let item = test_work_item("wi-1", "Fix login bug");

        db.create_work_item(&item).unwrap();
        let fetched = db.get_work_item("wi-1").unwrap().unwrap();
        assert_eq!(fetched.title, "Fix login bug");
        assert_eq!(fetched.status, "pending");

        db.update_work_item_status("wi-1", "in_progress").unwrap();
        let updated = db.get_work_item("wi-1").unwrap().unwrap();
        assert_eq!(updated.status, "in_progress");

        db.update_work_item_assignee("wi-1", "agent:coder").unwrap();
        let assigned = db.get_work_item("wi-1").unwrap().unwrap();
        assert_eq!(assigned.assignee, Some("agent:coder".to_string()));

        db.update_work_item_status("wi-1", "completed").unwrap();
        let completed = db.get_work_item("wi-1").unwrap().unwrap();
        assert_eq!(completed.status, "completed");
        assert!(completed.completed_at.is_some());
    }

    #[test]
    fn test_work_item_external_id_lookup() {
        let db = test_db();
        let mut item = test_work_item("wi-2", "External task");
        item.external_id = Some("kbs-123".to_string());
        item.backend = "kanbus".to_string();

        db.create_work_item(&item).unwrap();
        let fetched = db.get_work_item_by_external_id("kbs-123").unwrap().unwrap();
        assert_eq!(fetched.id, "wi-2");
        assert_eq!(fetched.backend, "kanbus");
    }

    #[test]
    fn test_work_item_unique_external_id() {
        let db = test_db();
        let mut item1 = test_work_item("wi-3", "First");
        item1.external_id = Some("ext-dup".to_string());
        db.create_work_item(&item1).unwrap();

        let mut item2 = test_work_item("wi-4", "Second");
        item2.external_id = Some("ext-dup".to_string());
        // INSERT OR REPLACE will overwrite due to unique index
        // This is acceptable — duplicates are prevented at the schema level
        let result = db.create_work_item(&item2);
        // Either fails or overwrites — both are acceptable for preventing duplicates
        if result.is_ok() {
            // Should only have one item with this external_id
            let found = db.get_work_item_by_external_id("ext-dup").unwrap().unwrap();
            assert_eq!(found.title, "Second");
        }
    }

    #[test]
    fn test_work_item_list_filter() {
        let db = test_db();
        db.create_work_item(&test_work_item("wi-a", "Task A"))
            .unwrap();
        db.create_work_item(&test_work_item("wi-b", "Task B"))
            .unwrap();
        db.update_work_item_status("wi-b", "completed").unwrap();

        let all = db.list_work_items(&WorkFilter::default()).unwrap();
        assert_eq!(all.len(), 2);

        let pending = db
            .list_work_items(&WorkFilter {
                status: Some("pending".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "wi-a");

        let count = db.count_work_items_by_status("completed").unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_work_item_delete_cascades_events() {
        let db = test_db();
        db.create_work_item(&test_work_item("wi-del", "To delete"))
            .unwrap();

        let event = WorkEvent {
            id: 0,
            work_item_id: "wi-del".to_string(),
            event_type: "created".to_string(),
            old_value: None,
            new_value: Some("To delete".to_string()),
            actor: Some("test".to_string()),
            timestamp: Utc::now(),
        };
        db.record_work_event(&event).unwrap();

        let events_before = db.get_work_events("wi-del", 10).unwrap();
        assert_eq!(events_before.len(), 1);

        db.delete_work_item("wi-del").unwrap();
        assert!(db.get_work_item("wi-del").unwrap().is_none());

        let events_after = db.get_work_events("wi-del", 10).unwrap();
        assert_eq!(events_after.len(), 0);
    }

    #[test]
    fn test_work_events() {
        let db = test_db();
        db.create_work_item(&test_work_item("wi-ev", "Event test"))
            .unwrap();

        let event1 = WorkEvent {
            id: 0,
            work_item_id: "wi-ev".to_string(),
            event_type: "created".to_string(),
            old_value: None,
            new_value: Some("Event test".to_string()),
            actor: Some("user".to_string()),
            timestamp: Utc::now(),
        };
        let event2 = WorkEvent {
            id: 0,
            work_item_id: "wi-ev".to_string(),
            event_type: "status_changed".to_string(),
            old_value: Some("pending".to_string()),
            new_value: Some("in_progress".to_string()),
            actor: Some("agent:coder".to_string()),
            timestamp: Utc::now(),
        };

        db.record_work_event(&event1).unwrap();
        db.record_work_event(&event2).unwrap();

        let events = db.get_work_events("wi-ev", 10).unwrap();
        assert_eq!(events.len(), 2);

        let recent = db.get_recent_work_events(1).unwrap();
        assert_eq!(recent.len(), 1);
        // Both events have near-identical timestamps; just verify we got one back
        assert!(recent[0].event_type == "created" || recent[0].event_type == "status_changed");
    }

    #[test]
    fn test_work_item_backend_update() {
        let db = test_db();
        db.create_work_item(&test_work_item("wi-push", "Push test"))
            .unwrap();

        let item = db.get_work_item("wi-push").unwrap().unwrap();
        assert_eq!(item.backend, "flowforge");

        db.update_work_item_backend("wi-push", "kanbus").unwrap();
        let updated = db.get_work_item("wi-push").unwrap().unwrap();
        assert_eq!(updated.backend, "kanbus");
    }

    #[test]
    fn test_session_lifecycle() {
        let db = test_db();
        let session = SessionInfo {
            id: "sess-1".to_string(),
            started_at: Utc::now(),
            ended_at: None,
            cwd: "/tmp".to_string(),
            edits: 0,
            commands: 0,
            summary: None,
            transcript_path: None,
        };

        db.create_session(&session).unwrap();
        let current = db.get_current_session().unwrap().unwrap();
        assert_eq!(current.id, "sess-1");

        db.increment_session_edits("sess-1").unwrap();
        db.increment_session_commands("sess-1").unwrap();
        let updated = db.get_current_session().unwrap().unwrap();
        assert_eq!(updated.edits, 1);
        assert_eq!(updated.commands, 1);

        db.end_session("sess-1", Utc::now()).unwrap();
        assert!(db.get_current_session().unwrap().is_none());

        let sessions = db.list_sessions(10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].ended_at.is_some());
    }

    #[test]
    fn test_kv_operations() {
        let db = test_db();
        db.kv_set("test-key", "test-value", "default").unwrap();

        let val = db.kv_get("test-key", "default").unwrap();
        assert_eq!(val, Some("test-value".to_string()));

        let missing = db.kv_get("missing", "default").unwrap();
        assert!(missing.is_none());

        db.kv_delete("test-key", "default").unwrap();
        assert!(db.kv_get("test-key", "default").unwrap().is_none());
    }

    #[test]
    fn test_foreign_keys_enabled() {
        let db = test_db();
        let fk_status: i32 = db
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk_status, 1);
    }
}
