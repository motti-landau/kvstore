use std::path::Path;

use chrono::{Duration, Utc};
use log::{debug, info};
use rusqlite::{params, Connection, Transaction};

use crate::store::Entry;
use crate::{KvError, KvResult};

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Opens or creates the SQLite database, ensuring the schema is up to date.
    pub fn connect<P: AsRef<Path>>(path: P) -> KvResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    KvError::io_path("creating database directory", parent.to_path_buf(), error)
                })?;
            }
        }

        let conn = Connection::open(path).map_err(|source| KvError::DbPath {
            path: path.to_path_buf(),
            source,
        })?;
        conn.busy_timeout(std::time::Duration::from_secs(3))?;
        let mut db = Self { conn };
        db.initialize_schema()?;
        info!("database connection open");
        Ok(db)
    }

    /// Loads every entry from the database so the in-memory cache can be primed.
    pub fn load_entries(&self) -> KvResult<Vec<(String, Entry)>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, value, tags, created_at, updated_at, expires_at FROM kv ORDER BY key ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Row {
                key: row.get(0)?,
                value: row.get(1)?,
                tags: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                expires_at: row.get(5)?,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            let row = row?;
            let entry = Entry::from_persisted(
                row.value,
                &row.tags,
                &row.created_at,
                &row.updated_at,
                row.expires_at.as_deref(),
            )?;
            entries.push((row.key, entry));
        }

        info!("loaded {} entries from sqlite", entries.len());
        Ok(entries)
    }

    /// Persists the provided entry using an UPSERT wrapped in a transaction for atomicity.
    pub fn upsert_entry(&mut self, key: &str, entry: &Entry) -> KvResult<()> {
        let tx = self.conn.transaction()?;
        Self::execute_upsert(&tx, key, entry)?;
        tx.commit()?;
        info!(
            "stored key={} updated_at={}",
            key,
            entry.updated_at().to_rfc3339()
        );
        Ok(())
    }

    /// Deletes the matching entry inside a transaction.
    pub fn delete_entry(&mut self, key: &str) -> KvResult<()> {
        let tx = self.conn.transaction()?;
        let affected = tx.execute("DELETE FROM kv WHERE key = ?1", params![key])?;
        if affected == 0 {
            return Err(KvError::NotFound(key.to_string()));
        }
        tx.commit()?;
        info!("deleted key={}", key);
        Ok(())
    }

    /// Replaces the database contents with the provided entries atomically.
    pub fn replace_all(&mut self, entries: &[(String, Entry)]) -> KvResult<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM kv", [])?;
        for (key, entry) in entries {
            Self::execute_upsert(&tx, key, entry)?;
        }
        tx.commit()?;
        info!("replaced all entries (count={})", entries.len());
        Ok(())
    }

    fn execute_upsert(tx: &Transaction<'_>, key: &str, entry: &Entry) -> KvResult<()> {
        let tags_json = entry.tags_json()?;
        tx.execute(
            "INSERT INTO kv (key, value, tags, created_at, updated_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(key)
             DO UPDATE SET value = excluded.value,
                           tags = excluded.tags,
                           updated_at = excluded.updated_at,
                           expires_at = excluded.expires_at",
            params![
                key,
                entry.value(),
                tags_json,
                entry.created_at().to_rfc3339(),
                entry.updated_at().to_rfc3339(),
                entry.expires_at().map(|ts| ts.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    fn initialize_schema(&mut self) -> KvResult<()> {
        self.conn
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;

        let user_version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        debug!("database user_version={}", user_version);

        if user_version == 0 {
            let kv_table_exists: i64 = self.conn.query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = 'kv'",
                [],
                |row| row.get(0),
            )?;
            if kv_table_exists > 0 {
                return Err(KvError::InvalidInput(
                    "unsupported legacy database detected; delete the database file to recreate it"
                        .to_string(),
                ));
            }

            let tx = self.conn.transaction()?;
            tx.execute_batch(
                "
                CREATE TABLE kv (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    tags TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    expires_at TEXT
                );
                PRAGMA user_version = 2;
            ",
            )?;
            tx.commit()?;
            info!("initialized kv schema (user_version=2)");
            return Ok(());
        }

        if user_version != 2 {
            return Err(KvError::InvalidInput(format!(
                "unsupported database schema version {user_version}; delete the database file to recreate it"
            )));
        }

        Ok(())
    }

    pub fn cleanup_expired_entries(&mut self) -> KvResult<usize> {
        let tx = self.conn.transaction()?;
        let threshold = (Utc::now() - Duration::hours(1)).to_rfc3339();
        let deleted = tx.execute(
            "DELETE FROM kv WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            params![threshold],
        )?;
        tx.commit()?;
        if deleted > 0 {
            info!("cleaned {} ttl-expired entries", deleted);
        }
        Ok(deleted)
    }
}

struct Row {
    key: String,
    value: String,
    tags: String,
    created_at: String,
    updated_at: String,
    expires_at: Option<String>,
}
