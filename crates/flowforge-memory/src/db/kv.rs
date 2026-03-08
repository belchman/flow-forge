use chrono::Utc;
use rusqlite::{params, OptionalExtension};

use flowforge_core::Result;

use super::{MemoryDb, SqliteExt};

impl MemoryDb {
    pub fn kv_get(&self, key: &str, namespace: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM key_value WHERE key = ?1 AND namespace = ?2",
                params![key, namespace],
                |row| row.get(0),
            )
            .optional()
            .sq()
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
            .sq()?;
        Ok(())
    }

    pub fn kv_delete(&self, key: &str, namespace: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM key_value WHERE key = ?1 AND namespace = ?2",
                params![key, namespace],
            )
            .sq()?;
        Ok(())
    }

    pub fn kv_list(&self, namespace: &str) -> Result<Vec<(String, String)>> {
        self.kv_list_limited(namespace, usize::MAX)
    }

    pub fn kv_list_limited(
        &self,
        namespace: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT key, value FROM key_value WHERE namespace = ?1 ORDER BY key LIMIT ?2",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![namespace, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn kv_search(&self, query: &str, limit: usize) -> Result<Vec<(String, String, String)>> {
        // Extract keywords (4+ chars) and search for any match in key or value
        let keywords: Vec<String> = query
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .filter(|w| w.len() >= 4)
            .collect();

        if keywords.is_empty() {
            return Ok(Vec::new());
        }

        let conditions: Vec<String> = keywords
            .iter()
            .enumerate()
            .map(|(i, _)| format!("(LOWER(key) LIKE ?{0} OR LOWER(value) LIKE ?{0})", i + 1))
            .collect();
        let where_clause = conditions.join(" OR ");
        let sql = format!(
            "SELECT key, value, namespace FROM key_value WHERE {} ORDER BY updated_at DESC LIMIT ?{}",
            where_clause,
            keywords.len() + 1
        );

        let mut stmt = self.conn.prepare(&sql).sq()?;
        let like_params: Vec<String> = keywords.iter().map(|k| format!("%{}%", k)).collect();
        let mut params_vec: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
        for p in &like_params {
            params_vec.push(p as &dyn rusqlite::types::ToSql);
        }
        let limit_val = limit as i64;
        params_vec.push(&limit_val);

        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_vec), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn count_kv(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM key_value", [], |row| row.get(0))
            .sq()
    }
}
