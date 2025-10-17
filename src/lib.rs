pub mod cli;
pub mod interactive;
pub mod storage;

use std::path::PathBuf;

use cli::{Cli, Command};
use interactive::live_search;
use storage::{Entry, SearchScope, Storage};
use thiserror::Error;

pub const DEFAULT_DATA_FILE: &str = "data.json";

pub type KvResult<T> = Result<T, KvError>;

/// Application-level error type surfaced to the CLI.
#[derive(Debug, Error)]
pub enum KvError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("data format error: {0}")]
    DataFormat(#[from] serde_json::Error),
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    InvalidInput(String),
}

/// Executes the application logic for the provided CLI arguments.
pub fn run(cli: Cli) -> KvResult<()> {
    let data_path = cli
        .data_file
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_FILE));
    let mut storage = Storage::open(data_path)?;

    match cli.command {
        Command::Add { key, value, tags } => {
            let tag_option = if tags.is_empty() { None } else { Some(tags) };
            let previous = storage.add(key.clone(), value, tag_option)?;
            let current = storage.get(&key)?;
            match previous {
                Some(old) => println!(
                    "Updated '{}'. Previous: {}; Now: {}",
                    key,
                    describe_value(&old),
                    describe_value(current)
                ),
                None => println!("Added '{}'. {}", key, describe_value(current)),
            }
        }
        Command::Get { key } => {
            let entry = storage.get(&key)?;
            println!("{}", entry.value);
            if !entry.tags().is_empty() {
                println!("tags: {}", entry.tags().join(", "));
            }
        }
        Command::Remove { key } => {
            let removed = storage.delete(&key)?;
            println!(
                "Removed '{}'. Stored value was {}.",
                key,
                describe_value(&removed)
            );
        }
        Command::List => {
            if storage.len() == 0 {
                println!("No entries stored.");
            } else {
                for (key, entry) in storage.list() {
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
            let matches = storage.search(&pattern, limit, scope);
            if matches.is_empty() {
                println!("No matches found.");
            } else {
                for item in matches {
                    println!("{}", item.entry.summary(item.key));
                }
            }
        }
        Command::Export { path } => {
            storage.export_to(&path)?;
            println!("Exported {} entries to {}", storage.len(), path.display());
        }
        Command::Import { path } => {
            storage.import_from(&path)?;
            println!("Imported {} entries from {}", storage.len(), path.display());
        }
        Command::Live {
            limit,
            tags_only,
            keys_only,
        } => {
            let scope = resolve_scope(tags_only, keys_only)?;
            live_search(&storage, limit, scope)?;
        }
    }

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
    let suffix = tags_suffix(entry.tags());
    format!("'{}'{suffix}", entry.value)
}

fn tags_suffix(tags: &[String]) -> String {
    if tags.is_empty() {
        String::new()
    } else {
        format!(" (tags: {})", tags.join(", "))
    }
}
