use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use log::{debug, info};
use serde::{Deserialize, Serialize};

use crate::KvResult;

/// In-memory representation of a single entry loaded from SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    value: String,
    tags: Vec<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Entry {
    pub fn new(value: String, tags: Vec<String>) -> Self {
        let now = Utc::now();
        Self {
            value,
            tags,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_timestamps(
        value: String,
        tags: Vec<String>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            value,
            tags,
            created_at,
            updated_at,
        }
    }

    pub fn from_persisted(
        value: String,
        tags_json: &str,
        created_at: &str,
        updated_at: &str,
    ) -> KvResult<Self> {
        let tags: Vec<String> = if tags_json.trim().is_empty() {
            Vec::new()
        } else {
            serde_json::from_str(tags_json)?
        };

        let created_at = DateTime::parse_from_rfc3339(created_at)?.with_timezone(&Utc);
        let updated_at = DateTime::parse_from_rfc3339(updated_at)?.with_timezone(&Utc);

        Ok(Self {
            value,
            tags,
            created_at,
            updated_at,
        })
    }

    pub fn for_update(existing: Option<&Entry>, value: String, tags: Vec<String>) -> Self {
        let now = Utc::now();
        let created_at = existing
            .map(|entry| entry.created_at)
            .unwrap_or_else(|| now);
        Self {
            value,
            tags,
            created_at,
            updated_at: now,
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
}

impl Store {
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
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.get(key)
    }

    pub fn insert(&mut self, key: String, entry: Entry) -> Option<Entry> {
        if !self.entries.contains_key(&key) {
            self.search_keys.push(key.clone());
            self.search_keys.sort();
        }
        let previous = self.entries.insert(key, entry);
        info!("cache updated; total_entries={}", self.entries.len());
        previous
    }

    pub fn remove(&mut self, key: &str) -> Option<Entry> {
        let removed = self.entries.remove(key);
        if removed.is_some() {
            self.search_keys.retain(|candidate| candidate != key);
            info!(
                "cache removed key={}; total_entries={}",
                key,
                self.entries.len()
            );
        }
        removed
    }

    /// Replaces the cached entries with a new data set (used during import).
    pub fn reset(&mut self, entries: Vec<(String, Entry)>) {
        self.entries.clear();
        self.search_keys.clear();
        for (key, entry) in entries {
            self.search_keys.push(key.clone());
            self.entries.insert(key, entry);
        }
        self.search_keys.sort();
        info!("cache reset; total_entries={}", self.entries.len());
    }

    pub fn ordered(&self) -> Vec<(&String, &Entry)> {
        let mut items: Vec<_> = self.entries.iter().collect();
        items.sort_by(|(a, _), (b, _)| a.cmp(b));
        items
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
