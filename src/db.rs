use std::path::Path;

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
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(3))?;
        let mut db = Self { conn };
        db.initialize_schema()?;
        info!("database connection open");
        Ok(db)
    }

    /// Loads every entry from the database so the in-memory cache can be primed.
    pub fn load_entries(&self) -> KvResult<Vec<(String, Entry)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value, tags, created_at, updated_at FROM kv ORDER BY key ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok(Row {
                key: row.get(0)?,
                value: row.get(1)?,
                tags: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            let row = row?;
            let entry =
                Entry::from_persisted(row.value, &row.tags, &row.created_at, &row.updated_at)?;
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
            "INSERT INTO kv (key, value, tags, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(key)
             DO UPDATE SET value = excluded.value,
                           tags = excluded.tags,
                           updated_at = excluded.updated_at",
            params![
                key,
                entry.value(),
                tags_json,
                entry.created_at().to_rfc3339(),
                entry.updated_at().to_rfc3339()
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
            let tx = self.conn.transaction()?;
            tx.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS kv (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    tags TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                PRAGMA user_version = 1;
            ",
            )?;
            tx.commit()?;
            info!("initialized kv schema (user_version=1)");
        }

        Ok(())
    }
}

struct Row {
    key: String,
    value: String,
    tags: String,
    created_at: String,
    updated_at: String,
}
