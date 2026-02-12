use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};

use crate::KvResult;

/// In-memory representation of a single entry loaded from SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    value: String,
    tags: Vec<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
}

impl Entry {
    pub fn new(value: String, tags: Vec<String>) -> Self {
        let now = Utc::now();
        Self {
            value,
            tags,
            created_at: now,
            updated_at: now,
            expires_at: None,
        }
    }

    pub fn with_timestamps(
        value: String,
        tags: Vec<String>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            value,
            tags,
            created_at,
            updated_at,
            expires_at,
        }
    }

    pub fn from_persisted(
        value: String,
        tags_json: &str,
        created_at: &str,
        updated_at: &str,
        expires_at: Option<&str>,
    ) -> KvResult<Self> {
        let tags: Vec<String> = if tags_json.trim().is_empty() {
            Vec::new()
        } else {
            serde_json::from_str(tags_json)?
        };

        let created_at = DateTime::parse_from_rfc3339(created_at)?.with_timezone(&Utc);
        let updated_at = DateTime::parse_from_rfc3339(updated_at)?.with_timezone(&Utc);
        let expires_at = expires_at
            .filter(|text| !text.trim().is_empty())
            .map(|text| DateTime::parse_from_rfc3339(text).map(|dt| dt.with_timezone(&Utc)))
            .transpose()?;

        Ok(Self {
            value,
            tags,
            created_at,
            updated_at,
            expires_at,
        })
    }

    pub fn for_update(existing: Option<&Entry>, value: String, tags: Vec<String>) -> Self {
        let now = Utc::now();
        let created_at = existing
            .map(|entry| entry.created_at)
            .unwrap_or_else(|| now);
        let expires_at = existing.and_then(|entry| entry.expires_at);
        Self {
            value,
            tags,
            created_at,
            updated_at: now,
            expires_at,
        }
    }

    pub fn tags_json(&self) -> KvResult<String> {
        Ok(serde_json::to_string(&self.tags)?)
    }

    pub fn summary(&self, key: &str) -> String {
        let suffix = if self.tags.is_empty() {
            String::new()
        } else {
            format!(" [tags: {}]", self.tags.join(", "))
        };
        format!("{key} = {}{}", self.value, suffix)
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    pub fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.expires_at
    }

    pub fn set_ttl_minutes(&mut self, ttl_minutes: Option<u64>) {
        self.expires_at =
            ttl_minutes.map(|minutes| Utc::now() + chrono::Duration::minutes(minutes as i64));
    }

    pub fn extend_ttl_minutes(&mut self, ttl_minutes: u64) {
        let now = Utc::now();
        let base = self
            .expires_at
            .map(|expires_at| expires_at.max(now))
            .unwrap_or(now);
        self.expires_at = Some(base + chrono::Duration::minutes(ttl_minutes as i64));
    }

    pub fn ttl_remaining_minutes(&self) -> Option<i64> {
        self.expires_at
            .map(|expires_at| (expires_at - Utc::now()).num_minutes())
    }
}

/// Determines how fuzzy searches evaluate stored data.
#[derive(Debug, Copy, Clone)]
pub enum SearchScope {
    All,
    KeysOnly,
    TagsOnly,
}

/// Cached entries plus pre-computed key ordering for fast fuzzy searching.
pub struct Store {
    entries: HashMap<String, Entry>,
    search_keys: Vec<String>,
    recent: VecDeque<String>,
    recent_capacity: usize,
    recent_file: Option<PathBuf>,
}

impl Store {
    const RECENT_CAPACITY: usize = 50;

    pub fn from_entries(entries: Vec<(String, Entry)>) -> Self {
        let mut map = HashMap::new();
        let mut search_keys = Vec::with_capacity(entries.len());

        for (key, entry) in entries {
            search_keys.push(key.clone());
            map.insert(key, entry);
        }

        search_keys.sort();

        Self {
            entries: map,
            search_keys,
            recent: VecDeque::with_capacity(Self::RECENT_CAPACITY),
            recent_capacity: Self::RECENT_CAPACITY,
            recent_file: None,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.get(key)
    }

    pub fn insert(&mut self, key: String, entry: Entry) -> Option<Entry> {
        if !self.entries.contains_key(&key) {
            if let Err(position) = self.search_keys.binary_search(&key) {
                self.search_keys.insert(position, key.clone());
            }
        }
        let previous = self.entries.insert(key, entry);
        info!("cache updated; total_entries={}", self.entries.len());
        previous
    }

    pub fn remove(&mut self, key: &str) -> Option<Entry> {
        let removed = self.entries.remove(key);
        if removed.is_some() {
            if let Ok(position) = self
                .search_keys
                .binary_search_by(|candidate| candidate.as_str().cmp(key))
            {
                self.search_keys.remove(position);
            } else {
                // Keep index recoverable even if the key list ever gets out of sync.
                self.search_keys.retain(|candidate| candidate != key);
            }
            self.recent.retain(|candidate| candidate != key);
            info!(
                "cache removed key={}; total_entries={}",
                key,
                self.entries.len()
            );
            self.prune_recent();
        }
        removed
    }

    /// Replaces the cached entries with a new data set (used during import).
    pub fn reset(&mut self, entries: Vec<(String, Entry)>) {
        self.entries.clear();
        self.search_keys.clear();
        self.recent.clear();
        for (key, entry) in entries {
            self.search_keys.push(key.clone());
            self.entries.insert(key, entry);
        }
        self.search_keys.sort();
        info!("cache reset; total_entries={}", self.entries.len());
        self.prune_recent();
    }

    pub fn ordered(&self) -> Vec<(&String, &Entry)> {
        self.search_keys
            .iter()
            .filter_map(|key| self.entries.get_key_value(key))
            .collect()
    }

    pub fn search<'a>(
        &'a self,
        pattern: &str,
        limit: usize,
        scope: SearchScope,
    ) -> Vec<SearchResult<'a>> {
        if pattern.is_empty() || limit == 0 {
            return Vec::new();
        }

        let matcher = SkimMatcherV2::default();
        let mut scored = Vec::new();

        for key in &self.search_keys {
            if let Some(entry) = self.entries.get(key) {
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

                let best_score = match scope {
                    SearchScope::All => key_score.max(tag_score),
                    SearchScope::KeysOnly => key_score,
                    SearchScope::TagsOnly => tag_score,
                };

                if let Some(score) = best_score {
                    scored.push(Scored {
                        score,
                        key: key.as_str(),
                        entry,
                    });
                }
            }
        }

        scored.sort_by(|a, b| b.score.cmp(&a.score));
        if scored.len() > limit {
            scored.truncate(limit);
        }

        let results: Vec<_> = scored
            .into_iter()
            .map(|scored| SearchResult {
                key: scored.key,
                entry: scored.entry,
            })
            .collect();

        debug!(
            "fuzzy search pattern='{}' scope={:?} results={}",
            pattern,
            scope,
            results.len()
        );

        results
    }

    pub fn normalize_tags(raw: Vec<String>) -> Vec<String> {
        let mut set = BTreeSet::new();
        for tag in raw {
            let trimmed = tag.trim();
            if !trimmed.is_empty() {
                set.insert(trimmed.to_string());
            }
        }
        set.into_iter().collect()
    }

    pub fn record_access(&mut self, key: &str) {
        if !self.entries.contains_key(key) {
            return;
        }

        self.recent.retain(|candidate| candidate != key);
        self.recent.push_front(key.to_string());
        self.recent.truncate(self.recent_capacity);
        self.persist_recent();
    }

    pub fn recent(&self, limit: usize) -> Vec<String> {
        self.recent.iter().take(limit).cloned().collect()
    }

    /// Enables persistence for the recent history using the provided configuration.
    pub fn enable_recent_history(&mut self, config: RecentConfig) {
        self.recent_capacity = config.capacity;
        self.recent_file = Some(config.path.clone());
        self.recent = load_recent_history(&config.path, self.recent_capacity);
        self.prune_recent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn sample_entries() -> Vec<(String, Entry)> {
        vec![
            ("alpha".to_string(), Entry::new("A".to_string(), vec![])),
            ("beta".to_string(), Entry::new("B".to_string(), vec![])),
            ("gamma".to_string(), Entry::new("C".to_string(), vec![])),
        ]
    }

    #[test]
    fn record_access_persists_recent_history() {
        let temp = tempdir().unwrap();
        let recent_path = temp.path().join("recent.log");

        {
            let mut store = Store::from_entries(sample_entries());
            store.enable_recent_history(RecentConfig::new(recent_path.clone(), 3));
            store.record_access("alpha");
            store.record_access("beta");
            store.record_access("gamma");
        }

        let mut store = Store::from_entries(sample_entries());
        store.enable_recent_history(RecentConfig::new(recent_path.clone(), 3));
        assert_eq!(
            store.recent(3),
            vec!["gamma".to_string(), "beta".to_string(), "alpha".to_string()]
        );

        let contents = fs::read_to_string(recent_path).unwrap();
        let lines: Vec<_> = contents.lines().collect();
        assert_eq!(lines, vec!["gamma", "beta", "alpha"]);
    }

    #[test]
    fn unknown_keys_are_dropped_when_loading_recent_history() {
        let temp = tempdir().unwrap();
        let recent_path = temp.path().join("recent.log");
        fs::write(&recent_path, "delta\nalpha\n").unwrap();

        let mut store = Store::from_entries(sample_entries());
        store.enable_recent_history(RecentConfig::new(recent_path.clone(), 5));
        assert_eq!(store.recent(5), vec!["alpha".to_string()]);

        let contents = fs::read_to_string(recent_path).unwrap();
        assert_eq!(contents.trim(), "alpha");
    }

    #[test]
    fn removing_keys_updates_recent_history_file() {
        let temp = tempdir().unwrap();
        let recent_path = temp.path().join("recent.log");

        let mut store = Store::from_entries(sample_entries());
        store.enable_recent_history(RecentConfig::new(recent_path.clone(), 5));
        store.record_access("alpha");
        store.record_access("beta");
        store.remove("beta");
        drop(store);

        let contents = fs::read_to_string(recent_path).unwrap();
        assert_eq!(contents.trim(), "alpha");
    }

    #[test]
    fn stale_prefix_does_not_drop_later_valid_recent_keys() {
        let temp = tempdir().unwrap();
        let recent_path = temp.path().join("recent.log");
        fs::write(&recent_path, "missing-a\nmissing-b\nalpha\nbeta\n").unwrap();

        let mut store = Store::from_entries(sample_entries());
        store.enable_recent_history(RecentConfig::new(recent_path.clone(), 2));
        assert_eq!(
            store.recent(2),
            vec!["alpha".to_string(), "beta".to_string()]
        );

        let contents = fs::read_to_string(recent_path).unwrap();
        let lines: Vec<_> = contents.lines().collect();
        assert_eq!(lines, vec!["alpha", "beta"]);
    }

    #[test]
    fn duplicate_recent_keys_are_deduplicated_on_load() {
        let temp = tempdir().unwrap();
        let recent_path = temp.path().join("recent.log");
        fs::write(&recent_path, "alpha\nalpha\nbeta\nalpha\n").unwrap();

        let mut store = Store::from_entries(sample_entries());
        store.enable_recent_history(RecentConfig::new(recent_path.clone(), 5));
        assert_eq!(
            store.recent(5),
            vec!["alpha".to_string(), "beta".to_string()]
        );

        let contents = fs::read_to_string(recent_path).unwrap();
        let lines: Vec<_> = contents.lines().collect();
        assert_eq!(lines, vec!["alpha", "beta"]);
    }

    #[test]
    fn extending_expired_ttl_starts_from_now() {
        let mut entry = Entry::new("value".to_string(), vec![]);
        entry.expires_at = Some(Utc::now() - chrono::Duration::minutes(10));

        entry.extend_ttl_minutes(5);

        let remaining = entry.ttl_remaining_minutes().unwrap_or_default();
        assert!(remaining >= 4, "remaining ttl too small: {remaining}");
    }
}

pub struct SearchResult<'a> {
    pub key: &'a str,
    pub entry: &'a Entry,
}

struct Scored<'a> {
    score: i64,
    key: &'a str,
    entry: &'a Entry,
}

fn matches_keys(scope: SearchScope) -> bool {
    matches!(scope, SearchScope::All | SearchScope::KeysOnly)
}

fn matches_tags(scope: SearchScope) -> bool {
    matches!(scope, SearchScope::All | SearchScope::TagsOnly)
}

fn load_recent_history(path: &Path, capacity: usize) -> VecDeque<String> {
    if capacity == 0 {
        return VecDeque::new();
    }

    match fs::read_to_string(path) {
        Ok(contents) => {
            let mut deque = VecDeque::with_capacity(capacity);
            let mut seen = HashSet::with_capacity(capacity);
            for line in contents.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let key = trimmed.to_string();
                if seen.insert(key.clone()) {
                    deque.push_back(key);
                }
            }
            deque
        }
        Err(error) if error.kind() == ErrorKind::NotFound => VecDeque::with_capacity(capacity),
        Err(error) => {
            warn!(
                "failed to read recent history file '{}': {}",
                path.display(),
                error
            );
            VecDeque::with_capacity(capacity)
        }
    }
}

#[derive(Debug, Clone)]
pub struct RecentConfig {
    path: PathBuf,
    capacity: usize,
}

impl RecentConfig {
    pub fn new(path: PathBuf, capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self { path, capacity }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl Store {
    fn persist_recent(&self) {
        let Some(path) = &self.recent_file else {
            return;
        };

        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                warn!(
                    "failed to create recent history directory '{}': {}",
                    parent.display(),
                    error
                );
                return;
            }
        }

        let payload = self
            .recent
            .iter()
            .take(self.recent_capacity)
            .map(|key| key.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        if let Err(error) = fs::write(path, payload) {
            warn!(
                "failed to write recent history file '{}': {}",
                path.display(),
                error
            );
        }
    }

    fn prune_recent(&mut self) {
        if self.recent.is_empty() {
            self.persist_recent();
            return;
        }

        let mut seen = HashSet::with_capacity(self.recent.len());
        self.recent
            .retain(|key| self.entries.contains_key(key) && seen.insert(key.clone()));
        if self.recent.len() > self.recent_capacity {
            self.recent.truncate(self.recent_capacity);
        }
        self.persist_recent();
    }
}
