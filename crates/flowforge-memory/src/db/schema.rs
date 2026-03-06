use flowforge_core::{Error, Result};

use super::{is_transient_sqlite, MemoryDb, SqliteExt};

/// Bump this whenever init_schema() changes (new tables, columns, indexes).
pub(crate) const SCHEMA_VERSION: u32 = 8;

impl MemoryDb {
    pub(crate) fn init_schema(&self) -> Result<()> {
        // Enable foreign key enforcement (SQLite doesn't enable by default)
        self.conn.execute_batch("PRAGMA foreign_keys = ON").sq()?;

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

            CREATE TABLE IF NOT EXISTS flowforge_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
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

            CREATE TABLE IF NOT EXISTS trust_scores (
                session_id TEXT PRIMARY KEY,
                score REAL DEFAULT 0.5,
                total_checks INTEGER DEFAULT 0,
                denials INTEGER DEFAULT 0,
                asks INTEGER DEFAULT 0,
                allows INTEGER DEFAULT 0,
                last_updated TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS gate_decisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                rule_id TEXT,
                gate_name TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                action TEXT NOT NULL,
                reason TEXT NOT NULL,
                risk_level TEXT NOT NULL,
                trust_before REAL,
                trust_after REAL,
                timestamp TEXT NOT NULL,
                hash TEXT NOT NULL,
                prev_hash TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS trajectories (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                work_item_id TEXT,
                agent_name TEXT,
                task_description TEXT,
                status TEXT DEFAULT 'recording',
                started_at TEXT NOT NULL,
                ended_at TEXT,
                verdict TEXT,
                confidence REAL,
                metadata TEXT,
                embedding_id INTEGER
            );

            CREATE TABLE IF NOT EXISTS trajectory_steps (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trajectory_id TEXT NOT NULL,
                step_index INTEGER NOT NULL,
                tool_name TEXT NOT NULL,
                tool_input_hash TEXT,
                outcome TEXT DEFAULT 'success',
                duration_ms INTEGER,
                timestamp TEXT NOT NULL,
                FOREIGN KEY (trajectory_id) REFERENCES trajectories(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_trajectory_steps_traj ON trajectory_steps(trajectory_id);
            CREATE INDEX IF NOT EXISTS idx_trajectories_session ON trajectories(session_id);
            CREATE INDEX IF NOT EXISTS idx_trajectories_status ON trajectories(status);

            CREATE TABLE IF NOT EXISTS pattern_clusters (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                centroid BLOB NOT NULL,
                member_count INTEGER NOT NULL DEFAULT 0,
                p95_distance REAL NOT NULL DEFAULT 0.0,
                avg_confidence REAL NOT NULL DEFAULT 0.0,
                created_at TEXT NOT NULL,
                last_recomputed TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS context_injections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                trajectory_id TEXT,
                injection_type TEXT NOT NULL,
                reference_id TEXT,
                similarity REAL,
                timestamp TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_ctx_inject_session ON context_injections(session_id);
            CREATE INDEX IF NOT EXISTS idx_ctx_inject_trajectory ON context_injections(trajectory_id);
        ",
            )
            .sq()?;

        // Migrations: add columns to existing tables if missing
        self.migrate_add_column("sessions", "transcript_path", "TEXT")?;
        self.migrate_add_column("agent_sessions", "transcript_path", "TEXT")?;

        // Work-stealing migrations
        self.migrate_add_column("work_items", "claimed_by", "TEXT")?;
        self.migrate_add_column("work_items", "claimed_at", "TEXT")?;
        self.migrate_add_column("work_items", "last_heartbeat", "TEXT")?;
        self.migrate_add_column("work_items", "progress", "INTEGER DEFAULT 0")?;
        self.migrate_add_column("work_items", "stealable", "INTEGER DEFAULT 0")?;

        // Work-stealing indexes (best-effort)
        let _ = self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_work_items_claimed ON work_items(claimed_by);
             CREATE INDEX IF NOT EXISTS idx_work_items_stealable ON work_items(stealable);
             CREATE INDEX IF NOT EXISTS idx_work_items_heartbeat ON work_items(last_heartbeat);",
        );

        // Clustering migrations
        self.migrate_add_column(
            "hnsw_entries",
            "cluster_id",
            "INTEGER REFERENCES pattern_clusters(id)",
        )?;
        let _ = self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_hnsw_entries_cluster ON hnsw_entries(cluster_id);",
        );

        // Effectiveness tracking migration
        self.migrate_add_column("context_injections", "effectiveness", "TEXT")?;
        // Routing breakdown metadata (stores serialized RoutingBreakdown JSON)
        self.migrate_add_column("context_injections", "metadata", "TEXT")?;

        // Pattern effectiveness table + columns
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS pattern_effectiveness (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    pattern_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    outcome TEXT NOT NULL,
                    similarity REAL DEFAULT 0.0,
                    timestamp TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_pattern_eff_pattern ON pattern_effectiveness(pattern_id);
                CREATE INDEX IF NOT EXISTS idx_pattern_eff_session ON pattern_effectiveness(session_id);",
            )
            .sq()?;
        self.migrate_add_column("patterns_long", "effectiveness_score", "REAL DEFAULT 0.0")?;
        self.migrate_add_column(
            "patterns_long",
            "effectiveness_samples",
            "INTEGER DEFAULT 0",
        )?;
        self.migrate_add_column("patterns_short", "effectiveness_score", "REAL DEFAULT 0.0")?;
        self.migrate_add_column(
            "patterns_short",
            "effectiveness_samples",
            "INTEGER DEFAULT 0",
        )?;

        // Anti-thrashing columns for work-stealing
        self.migrate_add_column("work_items", "steal_count", "INTEGER DEFAULT 0")?;
        self.migrate_add_column("work_items", "last_stolen_at", "TEXT")?;

        // Performance indexes (v4)
        let _ = self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_sessions_ended ON sessions(ended_at);
             CREATE INDEX IF NOT EXISTS idx_gate_decisions_session_ts ON gate_decisions(session_id, timestamp);
             CREATE INDEX IF NOT EXISTS idx_routing_weights_pattern ON routing_weights(task_pattern);",
        );

        // Additional indexes (v5)
        let _ = self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_agent_sessions_status ON agent_sessions(status);
             CREATE INDEX IF NOT EXISTS idx_agent_sessions_ended ON agent_sessions(ended_at);
             CREATE INDEX IF NOT EXISTS idx_trajectories_work_item ON trajectories(work_item_id);
             CREATE INDEX IF NOT EXISTS idx_gate_decisions_ts ON gate_decisions(timestamp);
             CREATE INDEX IF NOT EXISTS idx_patterns_short_last_used ON patterns_short(last_used);
             CREATE INDEX IF NOT EXISTS idx_patterns_long_last_used ON patterns_long(last_used);",
        );

        // Additional indexes (v6): support delete_vectors_for_source and get_agents_on_work_item
        let _ = self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_hnsw_source ON hnsw_entries(source_type, source_id);
             CREATE INDEX IF NOT EXISTS idx_agent_sessions_task ON agent_sessions(task_id);
             CREATE INDEX IF NOT EXISTS idx_edits_session ON edits(session_id);",
        );

        // v7 migration: add FK constraints to trajectories and context_injections
        // Disable FK checks during migration (required because trajectory_steps references trajectories)
        self.conn.execute_batch("PRAGMA foreign_keys = OFF").sq()?;

        // Migrate trajectories: add FK on work_item_id -> work_items(id) ON DELETE SET NULL
        self.conn
            .execute_batch(
                "
            CREATE TABLE IF NOT EXISTS trajectories_v7 (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                work_item_id TEXT REFERENCES work_items(id) ON DELETE SET NULL,
                agent_name TEXT,
                task_description TEXT,
                status TEXT DEFAULT 'recording',
                started_at TEXT NOT NULL,
                ended_at TEXT,
                verdict TEXT,
                confidence REAL,
                metadata TEXT,
                embedding_id INTEGER
            );
            INSERT OR IGNORE INTO trajectories_v7 SELECT * FROM trajectories;
            DROP TABLE trajectories;
            ALTER TABLE trajectories_v7 RENAME TO trajectories;
        ",
            )
            .sq()?;

        // Migrate context_injections: add FK on trajectory_id -> trajectories(id) ON DELETE SET NULL
        self.conn
            .execute_batch(
                "
            CREATE TABLE IF NOT EXISTS context_injections_v7 (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                trajectory_id TEXT REFERENCES trajectories(id) ON DELETE SET NULL,
                injection_type TEXT NOT NULL,
                reference_id TEXT,
                similarity REAL,
                timestamp TEXT NOT NULL,
                effectiveness TEXT,
                metadata TEXT
            );
            INSERT OR IGNORE INTO context_injections_v7 SELECT * FROM context_injections;
            DROP TABLE context_injections;
            ALTER TABLE context_injections_v7 RENAME TO context_injections;
        ",
            )
            .sq()?;

        // Recreate all indexes that were dropped with the old tables
        self.conn
            .execute_batch(
                "
            CREATE INDEX IF NOT EXISTS idx_trajectories_session ON trajectories(session_id);
            CREATE INDEX IF NOT EXISTS idx_trajectories_status ON trajectories(status);
            CREATE INDEX IF NOT EXISTS idx_trajectories_work_item ON trajectories(work_item_id);
            CREATE INDEX IF NOT EXISTS idx_trajectory_steps_traj ON trajectory_steps(trajectory_id);
            CREATE INDEX IF NOT EXISTS idx_ctx_inject_session ON context_injections(session_id);
            CREATE INDEX IF NOT EXISTS idx_ctx_inject_trajectory ON context_injections(trajectory_id);
        ",
            )
            .sq()?;

        
        // ── v14: Error recovery, discovered capabilities, recovery strategies,
        //         tool metrics, session metrics ──

        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS error_fingerprints (
                id TEXT PRIMARY KEY,
                fingerprint TEXT NOT NULL UNIQUE,
                category TEXT NOT NULL DEFAULT 'unknown',
                tool_name TEXT,
                error_preview TEXT NOT NULL DEFAULT '',
                first_seen TEXT NOT NULL,
                last_seen TEXT NOT NULL,
                occurrence_count INTEGER NOT NULL DEFAULT 1
            );
            CREATE INDEX IF NOT EXISTS idx_error_fp ON error_fingerprints(fingerprint);

            CREATE TABLE IF NOT EXISTS error_resolutions (
                id TEXT PRIMARY KEY,
                fingerprint_id TEXT NOT NULL REFERENCES error_fingerprints(id) ON DELETE CASCADE,
                resolution_summary TEXT NOT NULL,
                tool_sequence TEXT NOT NULL DEFAULT '[]',
                files_changed TEXT NOT NULL DEFAULT '[]',
                success_count INTEGER NOT NULL DEFAULT 0,
                failure_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                last_used TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_error_res_fp ON error_resolutions(fingerprint_id);

            CREATE TABLE IF NOT EXISTS session_tool_failures (
                id INTEGER PRIMARY KEY,
                session_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                input_hash TEXT NOT NULL DEFAULT '',
                error_preview TEXT NOT NULL DEFAULT '',
                timestamp TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_stf_session ON session_tool_failures(session_id);
            CREATE INDEX IF NOT EXISTS idx_stf_tool ON session_tool_failures(session_id, tool_name, input_hash);

            CREATE TABLE IF NOT EXISTS discovered_capabilities (
                id INTEGER PRIMARY KEY,
                agent_name TEXT NOT NULL,
                capability TEXT NOT NULL DEFAULT '',
                task_pattern TEXT NOT NULL,
                success_count INTEGER NOT NULL DEFAULT 0,
                failure_count INTEGER NOT NULL DEFAULT 0,
                confidence REAL NOT NULL DEFAULT 0.0,
                last_seen TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(agent_name, task_pattern)
            );
            CREATE INDEX IF NOT EXISTS idx_disc_cap_agent ON discovered_capabilities(agent_name);

            CREATE TABLE IF NOT EXISTS recovery_strategies (
                id INTEGER PRIMARY KEY,
                gate_name TEXT NOT NULL,
                trigger_pattern TEXT NOT NULL DEFAULT '',
                suggestion TEXT NOT NULL,
                alternative_command TEXT,
                success_count INTEGER NOT NULL DEFAULT 0,
                failure_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                last_used TEXT,
                UNIQUE(gate_name, trigger_pattern, suggestion)
            );
            CREATE INDEX IF NOT EXISTS idx_recovery_gate ON recovery_strategies(gate_name);

            CREATE TABLE IF NOT EXISTS tool_success_metrics (
                id INTEGER PRIMARY KEY,
                tool_name TEXT NOT NULL,
                agent_name TEXT NOT NULL DEFAULT '',
                success_count INTEGER NOT NULL DEFAULT 0,
                failure_count INTEGER NOT NULL DEFAULT 0,
                total_duration_ms INTEGER NOT NULL DEFAULT 0,
                last_updated TEXT NOT NULL,
                UNIQUE(tool_name, agent_name)
            );
            CREATE INDEX IF NOT EXISTS idx_tsm_tool ON tool_success_metrics(tool_name);

            CREATE TABLE IF NOT EXISTS session_metrics (
                id INTEGER PRIMARY KEY,
                session_id TEXT NOT NULL,
                metric_name TEXT NOT NULL,
                metric_value REAL NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL,
                UNIQUE(session_id, metric_name)
            );
            CREATE INDEX IF NOT EXISTS idx_sm_session ON session_metrics(session_id);

            CREATE TABLE IF NOT EXISTS routing_outcomes (
                id INTEGER PRIMARY KEY,
                session_id TEXT NOT NULL,
                agent_name TEXT NOT NULL,
                task_pattern TEXT NOT NULL,
                pattern_score REAL NOT NULL DEFAULT 0.0,
                capability_score REAL NOT NULL DEFAULT 0.0,
                learned_score REAL NOT NULL DEFAULT 0.0,
                priority_score REAL NOT NULL DEFAULT 0.0,
                context_score REAL NOT NULL DEFAULT 0.0,
                semantic_score REAL NOT NULL DEFAULT 0.0,
                outcome TEXT NOT NULL DEFAULT 'unknown',
                timestamp TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_ro_session ON routing_outcomes(session_id);
            CREATE INDEX IF NOT EXISTS idx_ro_agent ON routing_outcomes(agent_name);
        ").sq()?;

        // Seed default recovery strategies
        super::recovery_strategies::seed_default_strategies(self)?;

// Re-enable foreign keys
        self.conn.execute_batch("PRAGMA foreign_keys = ON").sq()?;

        // v8 migration: failure pattern detection and prevention
        self.conn
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS failure_patterns (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    pattern_name TEXT NOT NULL UNIQUE,
                    description TEXT NOT NULL,
                    trigger_tools TEXT NOT NULL,
                    prevention_hint TEXT NOT NULL,
                    occurrence_count INTEGER NOT NULL DEFAULT 0,
                    prevented_count INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    last_triggered TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_failure_patterns_name ON failure_patterns(pattern_name);
                ",
            )
            .sq()?;

        // Seed built-in failure patterns (idempotent via INSERT OR IGNORE)
        super::failure_patterns::seed_default_failure_patterns(self)?;

        // File co-edit dependency graph
        self.conn
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS file_co_edits (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    file_a TEXT NOT NULL,
                    file_b TEXT NOT NULL,
                    co_edit_count INTEGER NOT NULL DEFAULT 1,
                    last_seen TEXT NOT NULL,
                    UNIQUE(file_a, file_b)
                );
                CREATE INDEX IF NOT EXISTS idx_file_coedits_a ON file_co_edits(file_a);
                CREATE INDEX IF NOT EXISTS idx_file_coedits_b ON file_co_edits(file_b);
                ",
            )
            .sq()?;

        // Test co-occurrence suggestions
        self.conn
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS test_co_occurrences (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    edited_file TEXT NOT NULL,
                    test_file TEXT NOT NULL,
                    test_command TEXT,
                    occurrence_count INTEGER NOT NULL DEFAULT 1,
                    last_seen TEXT NOT NULL,
                    UNIQUE(edited_file, test_file)
                );
                CREATE INDEX IF NOT EXISTS idx_test_cooccur_edited ON test_co_occurrences(edited_file);
                CREATE INDEX IF NOT EXISTS idx_test_cooccur_test ON test_co_occurrences(test_file);
                ",
            )
            .sq()?;

        Ok(())
    }

    pub(crate) fn migrate_add_column(
        &self,
        table: &str,
        column: &str,
        col_type: &str,
    ) -> Result<()> {
        let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {col_type}");
        match self.conn.execute_batch(&sql) {
            Ok(()) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("duplicate column name") || msg.contains("already exists") {
                    Ok(()) // column already present
                } else {
                    Err(Error::Database {
                        message: msg,
                        transient: is_transient_sqlite(&e),
                    })
                }
            }
        }
    }
}
