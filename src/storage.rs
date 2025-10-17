use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::{KvError, KvResult};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use serde::{Deserialize, Serialize};

/// Owns the key-value data set and handles persistence to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub value: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Entry {
    pub fn summary(&self, key: &str) -> String {
        if self.tags.is_empty() {
            format!("{key} = {}", self.value)
        } else {
            format!("{key} = {} [tags: {}]", self.value, self.tags.join(", "))
        }
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }
}

#[derive(Debug, Copy, Clone)]
pub enum SearchScope {
    All,
    KeysOnly,
    TagsOnly,
}

pub struct Storage {
    path: PathBuf,
    entries: BTreeMap<String, Entry>,
}

impl Storage {
    /// Opens the storage file, creating it if needed, and loads all entries.
    pub fn open<P: Into<PathBuf>>(path: P) -> KvResult<Self> {
        let path = path.into();
        ensure_parent_exists(&path)?;
        if !path.exists() {
            fs::write(&path, "{}")?;
        }

        let entries = load_entries(&path)?;
        Ok(Self { path, entries })
    }

    /// Inserts or replaces a key-value pair (optionally updating tags) and persists the change.
    pub fn add(
        &mut self,
        key: String,
        value: String,
        tags: Option<Vec<String>>,
    ) -> KvResult<Option<Entry>> {
        let previous = self.entries.get(&key).cloned();
        let normalized_tags = tags
            .map(normalize_tags)
            .or_else(|| previous.as_ref().map(|entry| entry.tags.clone()))
            .unwrap_or_default();

        let entry = Entry {
            value,
            tags: normalized_tags,
        };

        let replaced = self.entries.insert(key, entry);
        self.persist()?;
        Ok(replaced)
    }

    /// Retrieves the entry for a key, if it exists.
    pub fn get(&self, key: &str) -> KvResult<&Entry> {
        self.entries
            .get(key)
            .ok_or_else(|| KvError::NotFound(key.to_string()))
    }

    /// Removes a key-value pair and persists the change.
    pub fn delete(&mut self, key: &str) -> KvResult<Entry> {
        let removed = self
            .entries
            .remove(key)
            .ok_or_else(|| KvError::NotFound(key.to_string()))?;
        self.persist()?;
        Ok(removed)
    }

    /// Returns an iterator over all stored entries in lexical key order.
    pub fn list(&self) -> impl Iterator<Item = (&String, &Entry)> {
        self.entries.iter()
    }

    /// Returns the total number of stored entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Performs fuzzy search across keys and/or tags, returning up to `limit` matches sorted by score.
    pub fn search(&self, pattern: &str, limit: usize, scope: SearchScope) -> Vec<MatchResult<'_>> {
        if pattern.is_empty() || limit == 0 {
            return Vec::new();
        }

        let matcher = SkimMatcherV2::default();
        let mut scored: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(key, entry)| {
                let key_score = if matches_keys(scope) {
                    matcher.fuzzy_match(key, pattern)
                } else {
                    None
                };

                let tag_score = if matches_tags(scope) {
                    entry
                        .tags
                        .iter()
                        .filter_map(|tag| matcher.fuzzy_match(tag, pattern))
                        .max()
                } else {
                    None
                };

                let best = match scope {
                    SearchScope::All => key_score.max(tag_score),
                    SearchScope::KeysOnly => key_score,
                    SearchScope::TagsOnly => tag_score,
                };

                best.map(|score| MatchResult { key, entry, score })
            })
            .collect();

        scored.sort_by(|a, b| b.score.cmp(&a.score));
        if scored.len() > limit {
            scored.truncate(limit);
        }
        scored
    }

    /// Exports all entries to the provided JSON file path.
    pub fn export_to<P: AsRef<Path>>(&self, path: P) -> KvResult<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let mut file = fs::File::create(path)?;
        write_json(&self.entries, &mut file)?;
        Ok(())
    }

    /// Imports entries from the provided JSON file path, replacing current data.
    pub fn import_from<P: AsRef<Path>>(&mut self, path: P) -> KvResult<()> {
        let entries = load_entries(path.as_ref())?;
        self.entries = entries;
        self.persist()
    }

    fn persist(&self) -> KvResult<()> {
        let mut file = fs::File::create(&self.path)?;
        write_json(&self.entries, &mut file)?;
        Ok(())
    }
}

/// Represents a fuzzy match result.
pub struct MatchResult<'a> {
    pub key: &'a String,
    pub entry: &'a Entry,
    score: i64,
}

fn load_entries(path: &Path) -> KvResult<BTreeMap<String, Entry>> {
    let contents = fs::read_to_string(path)?;
    if contents.trim().is_empty() {
        return Ok(BTreeMap::new());
    }

    match serde_json::from_str::<BTreeMap<String, Entry>>(&contents) {
        Ok(entries) => Ok(entries),
        Err(_) => {
            let legacy = serde_json::from_str::<BTreeMap<String, String>>(&contents)?;
            let converted = legacy
                .into_iter()
                .map(|(key, value)| {
                    (
                        key,
                        Entry {
                            value,
                            tags: Vec::new(),
                        },
                    )
                })
                .collect();
            Ok(converted)
        }
    }
}

fn write_json(map: &BTreeMap<String, Entry>, file: &mut fs::File) -> KvResult<()> {
    let json = serde_json::to_string_pretty(map)?;
    file.write_all(json.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

fn ensure_parent_exists(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

fn normalize_tags(raw: Vec<String>) -> Vec<String> {
    let mut set = BTreeSet::new();
    for tag in raw {
        let trimmed = tag.trim();
        if !trimmed.is_empty() {
            set.insert(trimmed.to_string());
        }
    }
    set.into_iter().collect()
}

fn matches_keys(scope: SearchScope) -> bool {
    matches!(scope, SearchScope::All | SearchScope::KeysOnly)
}

fn matches_tags(scope: SearchScope) -> bool {
    matches!(scope, SearchScope::All | SearchScope::TagsOnly)
}
