pub mod cli;
pub mod db;
pub mod interactive;
pub mod settings;
pub mod store;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use log::{info, warn};
use serde::{Deserialize, Serialize};

use cli::{Cli, Command};
use db::Database;
use interactive::live_search;
use store::{Entry, SearchScope, Store};
use thiserror::Error;

pub const DEFAULT_DATA_FILE: &str = "data.db";

pub type KvResult<T> = Result<T, KvError>;

/// Application-level error type surfaced to the CLI.
#[derive(Debug, Error)]
pub enum KvError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("data format error: {0}")]
    DataFormat(#[from] serde_json::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("config format error: {0}")]
    ConfigFormat(#[from] toml::de::Error),
    #[error("time parse error: {0}")]
    Time(#[from] chrono::ParseError),
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    InvalidInput(String),
}

/// Executes the application logic for the provided CLI arguments.
pub fn run(cli: Cli) -> KvResult<()> {
    let db_path = cli
        .data_file
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_FILE));
    info!("opening store at {}", db_path.display());

    let mut database = Database::connect(&db_path)?;
    let entries = database.load_entries()?;
    let mut store = Store::from_entries(entries);

    match cli.command {
        Command::Add { key, value, tags } => {
            handle_add(&mut database, &mut store, key, value, tags)?;
        }
        Command::Get { key } => {
            let entry = store
                .get(&key)
                .ok_or_else(|| KvError::NotFound(key.clone()))?;
            println!("{}", entry.value());
            if !entry.tags().is_empty() {
                println!("tags: {}", entry.tags().join(", "));
            }
        }
        Command::Remove { key } => {
            handle_remove(&mut database, &mut store, key)?;
        }
        Command::List => {
            if store.len() == 0 {
                println!("No entries stored.");
            } else {
                for (key, entry) in store.ordered() {
                    println!("{}", entry.summary(key));
                }
            }
        }
        Command::Search {
            pattern,
            limit,
            tags_only,
            keys_only,
        } => {
            let scope = resolve_scope(tags_only, keys_only)?;
            let matches = store.search(&pattern, limit, scope);
            if matches.is_empty() {
                println!("No matches found.");
            } else {
                for item in matches {
                    println!("{}", item.entry.summary(item.key));
                }
            }
        }
        Command::Export { path } => {
            export_to_path(&store, &path)?;
            println!("Exported {} entries to {}", store.len(), path.display());
        }
        Command::Import { path } => {
            handle_import(&mut database, &mut store, &path)?;
            println!("Imported entries from {}", path.display());
        }
        Command::Live {
            limit,
            tags_only,
            keys_only,
        } => {
            let scope = resolve_scope(tags_only, keys_only)?;
            live_search(&store, limit, scope)?;
        }
    }

    Ok(())
}

fn handle_add(
    database: &mut Database,
    store: &mut Store,
    key: String,
    value: String,
    tags: Vec<String>,
) -> KvResult<()> {
    let existing = store.get(&key).cloned();
    let tags = if tags.is_empty() {
        existing
            .as_ref()
            .map(|entry| entry.tags().to_vec())
            .unwrap_or_default()
    } else {
        Store::normalize_tags(tags)
    };
    let entry = Entry::for_update(existing.as_ref(), value, tags);

    database.upsert_entry(&key, &entry)?;
    let previous = store.insert(key.clone(), entry.clone());

    match previous {
        Some(old) => println!(
            "Updated '{}'. Previous: {}; Now: {}",
            key,
            describe_value(&old),
            describe_value(&entry)
        ),
        None => println!("Added '{}'. {}", key, describe_value(&entry)),
    }

    Ok(())
}

fn handle_remove(database: &mut Database, store: &mut Store, key: String) -> KvResult<()> {
    let existing = store
        .get(&key)
        .cloned()
        .ok_or_else(|| KvError::NotFound(key.clone()))?;

    database.delete_entry(&key)?;
    store
        .remove(&key)
        .ok_or_else(|| KvError::NotFound(key.clone()))?;

    println!(
        "Removed '{}'. Stored value was {}.",
        key,
        describe_value(&existing)
    );
    Ok(())
}

fn handle_import(database: &mut Database, store: &mut Store, path: &Path) -> KvResult<()> {
    let contents = fs::read_to_string(path)?;
    if contents.trim().is_empty() {
        warn!("import file {} is empty; clearing database", path.display());
    }

    let map: BTreeMap<String, ImportEntry> = if contents.trim().is_empty() {
        BTreeMap::new()
    } else {
        serde_json::from_str(&contents)?
    };

    let mut entries = Vec::with_capacity(map.len());

    for (key, item) in map {
        let tags = Store::normalize_tags(item.tags.unwrap_or_default());
        let tags_json = serde_json::to_string(&tags)?;

        let created_at = item.created_at.unwrap_or_else(|| Utc::now().to_rfc3339());
        let updated_at = item.updated_at.unwrap_or_else(|| Utc::now().to_rfc3339());

        let entry = Entry::from_persisted(item.value, &tags_json, &created_at, &updated_at)?;
        entries.push((key, entry));
    }

    database.replace_all(&entries)?;
    store.reset(entries);

    Ok(())
}

fn export_to_path(store: &Store, path: &Path) -> KvResult<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let mut map = BTreeMap::new();
    for (key, entry) in store.ordered() {
        map.insert(
            key.clone(),
            ExportEntry {
                value: entry.value().to_string(),
                tags: entry.tags().to_vec(),
                created_at: entry.created_at().to_rfc3339(),
                updated_at: entry.updated_at().to_rfc3339(),
            },
        );
    }

    let json = serde_json::to_string_pretty(&map)?;
    fs::write(path, format!("{json}\n"))?;
    Ok(())
}

fn resolve_scope(tags_only: bool, keys_only: bool) -> KvResult<SearchScope> {
    if tags_only && keys_only {
        Err(KvError::InvalidInput(
            "Cannot search keys-only and tags-only at the same time.".into(),
        ))
    } else if tags_only {
        Ok(SearchScope::TagsOnly)
    } else if keys_only {
        Ok(SearchScope::KeysOnly)
    } else {
        Ok(SearchScope::All)
    }
}

fn describe_value(entry: &Entry) -> String {
    if entry.tags().is_empty() {
        format!("'{}'", entry.value())
    } else {
        format!("'{}' (tags: {})", entry.value(), entry.tags().join(", "))
    }
}

#[derive(Serialize)]
struct ExportEntry {
    value: String,
    tags: Vec<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct ImportEntry {
    value: String,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
}
