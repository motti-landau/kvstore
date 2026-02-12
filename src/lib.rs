pub mod cli;
pub mod db;
pub mod interactive;
pub mod settings;
pub mod store;

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

use chrono::Utc;
use log::{info, warn};
use serde::{Deserialize, Serialize};

use cli::{Cli, Command};
use db::Database;
use interactive::live_search;
use settings::AppSettings;
use store::{Entry, RecentConfig, SearchScope, Store};
use thiserror::Error;

const APP_DIR: &str = ".kvstore";
const NAMESPACES_DIR: &str = "namespaces";
const DEFAULT_DATA_FILE_NAME: &str = "data.db";
const DEFAULT_RECENT_LOG_NAME: &str = "recent.log";
const DEFAULT_NAMESPACE: &str = "default";

pub type KvResult<T> = Result<T, KvError>;

/// Application-level error type surfaced to the CLI.
#[derive(Debug, Error)]
pub enum KvError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("I/O error while {action} '{path}': {source}")]
    IoPath {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("data format error: {0}")]
    DataFormat(#[from] serde_json::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("database error while opening '{path}': {source}")]
    DbPath {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("config format error: {0}")]
    ConfigFormat(#[from] toml::de::Error),
    #[error("time parse error: {0}")]
    Time(#[from] chrono::ParseError),
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    InvalidInput(String),
}

impl KvError {
    pub(crate) fn io_path(
        action: &'static str,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self::IoPath {
            action,
            path: path.into(),
            source,
        }
    }
}

/// Executes the application logic for the provided CLI arguments.
pub fn run(cli: Cli, settings: &AppSettings) -> KvResult<()> {
    let namespace = resolve_namespace(cli.namespace.as_deref())?;
    let db_path = cli
        .data_file
        .unwrap_or_else(|| default_data_file_path(&namespace));
    info!("opening store at {}", db_path.display());

    if let Command::Serve { host, port } = &cli.command {
        let database = Database::connect(&db_path)?;
        serve_viewer(&database, &db_path, &namespace, host, *port)?;
        return Ok(());
    }

    let mut database = Database::connect(&db_path)?;
    let entries = database.load_entries()?;
    let mut store = Store::from_entries(entries);

    let history_settings = settings.history();
    let recent_path = history_settings
        .file()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_recent_log_path(&namespace));
    let recent_limit = history_settings.limit();
    if recent_limit > 0 {
        let config = RecentConfig::new(recent_path, recent_limit);
        store.enable_recent_history(config);
    }

    match cli.command {
        Command::Add { key, value, tags } => {
            handle_add(&mut database, &mut store, key, value, tags)?
        }
        Command::Get { key } => {
            let entry = store
                .get(&key)
                .ok_or_else(|| KvError::NotFound(key.clone()))?
                .clone();
            store.record_access(&key);
            println!("{}", entry.value());
            if !entry.tags().is_empty() {
                println!("tags: {}", entry.tags().join(", "));
            }
        }
        Command::Remove { key } => {
            handle_remove(&mut database, &mut store, key)?;
        }
        Command::List => {
            if store.is_empty() {
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
        Command::Html { path } => {
            export_html_view(&store, &path)?;
            println!(
                "Generated HTML view at {} (namespace: {}, data source: {})",
                path.display(),
                namespace,
                db_path.display()
            );
        }
        Command::Serve { .. } => unreachable!("serve is handled before cache loading"),
        Command::PutFile {
            key,
            path,
            tags,
            any_file,
        } => {
            handle_put_file(&mut database, &mut store, key, &path, tags, any_file)?;
        }
        Command::GetFile {
            key,
            path,
            any_file,
        } => {
            let key_for_message = key.clone();
            handle_get_file(&mut store, key, &path, any_file)?;
            println!("Wrote '{}' to {}", key_for_message, path.display());
        }
        Command::Interactive {
            limit,
            tags_only,
            keys_only,
        } => {
            let scope = resolve_scope(tags_only, keys_only)?;
            live_search(&store, limit, scope)?;
        }
        Command::Recent { limit } => {
            let recent = store.recent(limit);
            if recent.is_empty() {
                println!("No recent keys recorded.");
            } else {
                for (idx, key) in recent.iter().enumerate() {
                    println!("{:>2}. {}", idx + 1, key);
                }
            }
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
    store.record_access(&key);

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
    let contents = fs::read_to_string(path)
        .map_err(|error| KvError::io_path("reading import file", path.to_path_buf(), error))?;
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

fn handle_put_file(
    database: &mut Database,
    store: &mut Store,
    key: String,
    path: &Path,
    tags: Vec<String>,
    any_file: bool,
) -> KvResult<()> {
    validate_markdown_path(path, any_file, "source file")?;
    let contents = fs::read_to_string(path)
        .map_err(|error| KvError::io_path("reading source file", path.to_path_buf(), error))?;
    handle_add(database, store, key, contents, tags)
}

fn handle_get_file(store: &mut Store, key: String, path: &Path, any_file: bool) -> KvResult<()> {
    validate_markdown_path(path, any_file, "destination file")?;
    let entry = store
        .get(&key)
        .ok_or_else(|| KvError::NotFound(key.clone()))?;

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|error| {
                KvError::io_path(
                    "creating destination directory",
                    parent.to_path_buf(),
                    error,
                )
            })?;
        }
    }

    fs::write(path, entry.value())
        .map_err(|error| KvError::io_path("writing destination file", path.to_path_buf(), error))?;
    store.record_access(&key);
    Ok(())
}

fn export_to_path(store: &Store, path: &Path) -> KvResult<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|error| {
                KvError::io_path("creating export directory", parent.to_path_buf(), error)
            })?;
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
    fs::write(path, format!("{json}\n"))
        .map_err(|error| KvError::io_path("writing export file", path.to_path_buf(), error))?;
    Ok(())
}

fn export_html_view(store: &Store, path: &Path) -> KvResult<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|error| {
                KvError::io_path(
                    "creating html output directory",
                    parent.to_path_buf(),
                    error,
                )
            })?;
        }
    }

    let html = render_html_view(store)?;
    fs::write(path, html)
        .map_err(|error| KvError::io_path("writing html output file", path.to_path_buf(), error))?;
    Ok(())
}

fn render_html_view(store: &Store) -> KvResult<String> {
    render_html_with_options(store, "")
}

fn render_html_with_options(store: &Store, poll_endpoint: &str) -> KvResult<String> {
    let json = serialize_html_records(store)?;
    let safe_json = json.replace("</", "<\\/");
    let poll_endpoint_json = serde_json::to_string(poll_endpoint)?;
    let template = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width,initial-scale=1" />
  <title>kvstore viewer</title>
  <style>
    :root {
      --bg: #f4f1eb;
      --panel: #fffdf8;
      --ink: #1f2a2e;
      --muted: #66757d;
      --line: #d8d0c6;
      --accent: #bf4f2d;
      --accent-soft: #f5d6cc;
      --chip: #e8f2ef;
      --chip-ink: #21564a;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      font-family: "Avenir Next", "Segoe UI", sans-serif;
      color: var(--ink);
      background:
        radial-gradient(1200px 600px at 10% -10%, #f7e8dc 0%, transparent 55%),
        radial-gradient(900px 500px at 100% 0%, #dfefe9 0%, transparent 50%),
        var(--bg);
      min-height: 100vh;
    }
    .wrap {
      max-width: 1260px;
      margin: 2rem auto;
      padding: 0 1rem 2rem;
    }
    .card {
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 16px;
      padding: 1rem;
      box-shadow: 0 10px 24px rgba(0, 0, 0, 0.05);
    }
    .head h1 {
      margin: 0 0 0.8rem;
      font-size: 1.6rem;
      letter-spacing: 0.02em;
    }
    .toolbar {
      display: grid;
      grid-template-columns: 1fr auto auto;
      gap: 0.65rem;
      align-items: center;
    }
    input {
      width: 100%;
      border: 1px solid var(--line);
      border-radius: 10px;
      padding: 0.7rem 0.8rem;
      font-size: 0.95rem;
      color: var(--ink);
      background: #fff;
    }
    button {
      border: 1px solid transparent;
      background: var(--accent-soft);
      color: var(--accent);
      border-radius: 10px;
      cursor: pointer;
      padding: 0.62rem 0.8rem;
      font-size: 0.85rem;
      font-weight: 600;
    }
    button.secondary {
      background: #fff;
      color: #425158;
      border-color: var(--line);
    }
    button.active {
      background: var(--accent);
      color: #fff;
    }
    .summary {
      margin-top: 0.8rem;
      display: flex;
      gap: 0.5rem;
      flex-wrap: wrap;
    }
    .pill {
      background: #f4ede1;
      border: 1px solid var(--line);
      border-radius: 999px;
      padding: 0.28rem 0.62rem;
      font-size: 0.8rem;
      color: #3f4f55;
    }
    .selected {
      margin-top: 0.7rem;
      min-height: 1.6rem;
      display: flex;
      gap: 0.4rem;
      flex-wrap: wrap;
    }
    .chip {
      display: inline-flex;
      align-items: center;
      gap: 0.35rem;
      font-size: 0.78rem;
      background: var(--chip);
      color: var(--chip-ink);
      border-radius: 999px;
      padding: 0.2rem 0.55rem;
      border: 1px solid #cde2dc;
    }
    .chip button {
      padding: 0;
      border: 0;
      background: transparent;
      color: inherit;
      line-height: 1;
      font-size: 0.9rem;
    }
    .view-toggle {
      display: inline-flex;
      gap: 0.45rem;
    }
    .panels {
      margin-top: 1rem;
      display: grid;
      grid-template-columns: 1fr 1.35fr;
      gap: 1rem;
    }
    .subhead {
      display: flex;
      justify-content: space-between;
      align-items: center;
      margin-bottom: 0.6rem;
      gap: 0.6rem;
    }
    .subhead h2 {
      margin: 0;
      font-size: 1rem;
    }
    .meta {
      color: var(--muted);
      font-size: 0.82rem;
    }
    .recent-list {
      margin: 0;
      padding: 0;
      list-style: none;
      display: grid;
      gap: 0.45rem;
    }
    .recent-item {
      border: 1px solid var(--line);
      background: #fff;
      border-radius: 10px;
      padding: 0.45rem 0.55rem;
      display: grid;
      gap: 0.28rem;
    }
    .recent-item button {
      text-align: left;
      background: transparent;
      border: 0;
      color: #314044;
      padding: 0;
      font-size: 0.9rem;
    }
    .tag-search {
      margin-bottom: 0.6rem;
    }
    .table-wrap {
      overflow: auto;
      border: 1px solid var(--line);
      border-radius: 12px;
      background: #fff;
    }
    table {
      width: 100%;
      border-collapse: collapse;
    }
    th, td {
      text-align: left;
      padding: 0.7rem 0.8rem;
      border-bottom: 1px solid #efe8de;
      vertical-align: top;
      font-size: 0.9rem;
    }
    th {
      position: sticky;
      top: 0;
      background: #faf6ef;
      color: #3a4a50;
      z-index: 1;
      font-size: 0.82rem;
      text-transform: uppercase;
      letter-spacing: 0.03em;
    }
    .tag-row-btn {
      font-size: 0.84rem;
      border: 0;
      background: transparent;
      color: #224840;
      padding: 0;
      cursor: pointer;
      text-align: left;
    }
    .records {
      margin-top: 1rem;
    }
    .value {
      white-space: pre-wrap;
      max-width: 52ch;
      word-break: break-word;
      color: #253236;
    }
    .tag-badges {
      display: flex;
      flex-wrap: wrap;
      gap: 0.3rem;
    }
    .tag-badge {
      font-size: 0.74rem;
      background: #f0f6f4;
      color: #2e5c50;
      border-radius: 999px;
      padding: 0.15rem 0.48rem;
      border: 1px solid #d8e8e3;
    }
    .groups {
      display: grid;
      gap: 0.85rem;
    }
    .group {
      border: 1px solid var(--line);
      border-radius: 12px;
      background: #fff;
      overflow: hidden;
    }
    .group-head {
      padding: 0.55rem 0.75rem;
      background: #faf6ef;
      border-bottom: 1px solid #efe8de;
      display: flex;
      justify-content: space-between;
      gap: 0.5rem;
      align-items: center;
    }
    .group-head strong { font-size: 0.9rem; }
    .empty {
      color: var(--muted);
      font-size: 0.88rem;
      padding: 0.75rem 0.1rem;
    }
    .hidden { display: none; }
    @media (max-width: 980px) {
      .panels { grid-template-columns: 1fr; }
    }
    @media (max-width: 760px) {
      .toolbar {
        grid-template-columns: 1fr;
      }
      th:nth-child(4),
      td:nth-child(4) {
        display: none;
      }
      .value {
        max-width: 34ch;
      }
    }
  </style>
</head>
<body>
  <div class="wrap">
    <section class="card head">
      <h1>kvstore records</h1>
      <div class="toolbar">
        <input id="query" placeholder="Search key, value, or tag..." />
        <div class="view-toggle">
          <button id="list-mode" class="active" type="button">List View</button>
          <button id="group-mode" class="secondary" type="button">Grouped by Tag</button>
        </div>
        <button id="clear-all" class="secondary" type="button">Clear Filters</button>
      </div>
      <div class="summary" id="summary"></div>
      <div class="selected" id="selected-tags"></div>
    </section>

    <section class="card records" id="records-card">
      <div class="subhead">
        <h2>Records</h2>
        <span class="meta" id="record-meta"></span>
      </div>

      <div id="list-view" class="table-wrap">
        <table>
          <thead>
            <tr>
              <th style="width:17%">Key</th>
              <th style="width:43%">Value</th>
              <th style="width:18%">Tags</th>
              <th style="width:22%">Updated</th>
            </tr>
          </thead>
          <tbody id="record-rows"></tbody>
        </table>
      </div>

      <div id="group-view" class="groups hidden"></div>
    </section>

    <section class="panels">
      <article class="card">
        <div class="subhead">
          <h2>Recent Updates</h2>
          <span class="meta" id="recent-meta"></span>
        </div>
        <ul id="recent-list" class="recent-list"></ul>
      </article>

      <article class="card">
        <div class="subhead">
          <h2>Tag Explorer</h2>
          <span class="meta" id="tag-meta"></span>
        </div>
        <input id="tag-search" class="tag-search" placeholder="Find tags..." />
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th style="width:50%">Tag</th>
                <th style="width:15%">Count</th>
                <th style="width:35%">Last Updated</th>
              </tr>
            </thead>
            <tbody id="tag-rows"></tbody>
          </table>
        </div>
        <div style="margin-top:0.65rem;">
          <button id="tag-more" class="secondary hidden" type="button">Show More Tags</button>
        </div>
      </article>
    </section>
  </div>

  <script id="kv-data" type="application/json">__KV_DATA__</script>
  <script>
    const payloadText = document.getElementById("kv-data").textContent || "[]";
    const liveEndpoint = __KV_POLL__;
    const pollIntervalMs = 3000;
    let lastNormalizedPayload = normalizePayload(payloadText);
    let records = normalizeRecords(JSON.parse(payloadText));

    const queryInput = document.getElementById("query");
    const listModeBtn = document.getElementById("list-mode");
    const groupModeBtn = document.getElementById("group-mode");
    const clearAllBtn = document.getElementById("clear-all");
    const summaryEl = document.getElementById("summary");
    const selectedTagsEl = document.getElementById("selected-tags");
    const recentListEl = document.getElementById("recent-list");
    const recentMetaEl = document.getElementById("recent-meta");
    const tagSearchEl = document.getElementById("tag-search");
    const tagMetaEl = document.getElementById("tag-meta");
    const tagRowsEl = document.getElementById("tag-rows");
    const tagMoreEl = document.getElementById("tag-more");
    const recordMetaEl = document.getElementById("record-meta");
    const recordRowsEl = document.getElementById("record-rows");
    const listViewEl = document.getElementById("list-view");
    const groupViewEl = document.getElementById("group-view");
    const recordsCardEl = document.getElementById("records-card");

    const selectedTags = new Set();
    const tagPageSize = 20;
    let tagLimit = tagPageSize;
    let viewMode = "list";

    let tagStats = buildTagStats(records);

    function buildTagStats(items) {
      const map = new Map();
      for (const record of items) {
        const uniqueTags = new Set(record.tags || []);
        for (const tag of uniqueTags) {
          const existing = map.get(tag) || { tag, count: 0, lastUpdatedMs: 0, lastUpdatedIso: "" };
          existing.count += 1;
          if (record.updatedMs >= existing.lastUpdatedMs) {
            existing.lastUpdatedMs = record.updatedMs;
            existing.lastUpdatedIso = record.updated_at;
          }
          map.set(tag, existing);
        }
      }
      return Array.from(map.values()).sort((a, b) =>
        b.lastUpdatedMs - a.lastUpdatedMs
        || b.count - a.count
        || a.tag.localeCompare(b.tag)
      );
    }

    function escapeHtml(value) {
      return String(value)
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;");
    }

    function normalizePayload(text) {
      try {
        return JSON.stringify(JSON.parse(text || "[]"));
      } catch (_) {
        return "[]";
      }
    }

    function normalizeRecords(items) {
      return items
        .map((item, index) => {
          const updatedMs = Date.parse(item.updated_at) || 0;
          return {
            ...item,
            id: `${item.key}::${index}`,
            updatedMs,
            searchBlob: [item.key, item.value, ...(item.tags || [])].join("\n").toLowerCase()
          };
        })
        .sort((a, b) => b.updatedMs - a.updatedMs || a.key.localeCompare(b.key));
    }

    function formatDate(isoText) {
      const date = new Date(isoText);
      if (Number.isNaN(date.getTime())) {
        return isoText;
      }
      return new Intl.DateTimeFormat(undefined, {
        year: "numeric",
        month: "short",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit"
      }).format(date);
    }

    function filteredRecords() {
      const query = queryInput.value.trim().toLowerCase();
      return records.filter((record) => {
        const matchesQuery = !query || record.searchBlob.includes(query);
        const matchesTags = selectedTags.size === 0
          || Array.from(selectedTags).every((tag) => (record.tags || []).includes(tag));
        return matchesQuery && matchesTags;
      });
    }

    function setViewMode(mode) {
      viewMode = mode;
      listModeBtn.classList.toggle("active", mode === "list");
      listModeBtn.classList.toggle("secondary", mode !== "list");
      groupModeBtn.classList.toggle("active", mode === "group");
      groupModeBtn.classList.toggle("secondary", mode !== "group");
      listViewEl.classList.toggle("hidden", mode !== "list");
      groupViewEl.classList.toggle("hidden", mode !== "group");
      render();
    }

    function toggleTag(tag) {
      if (selectedTags.has(tag)) {
        selectedTags.delete(tag);
      } else {
        selectedTags.add(tag);
      }
      render();
    }

    function renderSummary(filtered) {
      const latestIso = records.length ? records[0].updated_at : "-";
      summaryEl.innerHTML = [
        `<span class="pill">${filtered.length} shown</span>`,
        `<span class="pill">${records.length} total records</span>`,
        `<span class="pill">${tagStats.length} tags</span>`,
        `<span class="pill">latest update: ${escapeHtml(formatDate(latestIso))}</span>`
      ].join("");

      if (selectedTags.size === 0) {
        selectedTagsEl.innerHTML = "";
        return;
      }

      const chips = Array.from(selectedTags).sort((a, b) => a.localeCompare(b));
      selectedTagsEl.innerHTML = chips.map((tag) => `
        <span class="chip">
          ${escapeHtml(tag)}
          <button type="button" data-tag-remove="${escapeHtml(tag)}">x</button>
        </span>
      `).join("");
    }

    function renderRecents() {
      const recents = records.slice(0, 12);
      recentMetaEl.textContent = `${recents.length} most recently updated`;
      recentListEl.innerHTML = recents.map((record) => {
        const tagText = record.tags && record.tags.length ? `#${record.tags.join(" #")}` : "(untagged)";
        return `
          <li class="recent-item">
            <button type="button" data-focus-key="${escapeHtml(record.key)}">${escapeHtml(record.key)}</button>
            <span class="meta">${escapeHtml(formatDate(record.updated_at))}</span>
            <span class="meta">${escapeHtml(tagText)}</span>
          </li>
        `;
      }).join("");
    }

    function renderTagExplorer() {
      const tagQuery = tagSearchEl.value.trim().toLowerCase();
      const visibleTags = tagStats.filter((item) => !tagQuery || item.tag.toLowerCase().includes(tagQuery));
      const limited = visibleTags.slice(0, tagLimit);

      tagMetaEl.textContent = `${visibleTags.length} matching tags`;
      tagRowsEl.innerHTML = limited.map((item) => {
        const active = selectedTags.has(item.tag);
        return `
          <tr>
            <td>
              <button type="button" class="tag-row-btn" data-tag-toggle="${escapeHtml(item.tag)}">
                ${active ? "[-]" : "[+]"} ${escapeHtml(item.tag)}
              </button>
            </td>
            <td>${item.count}</td>
            <td>${escapeHtml(formatDate(item.lastUpdatedIso))}</td>
          </tr>
        `;
      }).join("");

      tagMoreEl.classList.toggle("hidden", visibleTags.length <= tagLimit);
    }

    function renderList(filtered) {
      recordRowsEl.innerHTML = filtered.map((record) => {
        const tags = record.tags && record.tags.length
          ? `<div class="tag-badges">${record.tags.map((tag) => `<span class="tag-badge">${escapeHtml(tag)}</span>`).join("")}</div>`
          : '<span class="meta">-</span>';

        return `
          <tr>
            <td><strong>${escapeHtml(record.key)}</strong></td>
            <td class="value">${escapeHtml(record.value)}</td>
            <td>${tags}</td>
            <td>${escapeHtml(formatDate(record.updated_at))}</td>
          </tr>
        `;
      }).join("");
    }

    function renderGrouped(filtered) {
      if (filtered.length === 0) {
        groupViewEl.innerHTML = `<div class="empty">No records match current filters.</div>`;
        return;
      }

      const groups = new Map();
      const untagged = [];

      for (const record of filtered) {
        const tags = record.tags || [];
        if (tags.length === 0) {
          untagged.push(record);
          continue;
        }
        for (const tag of tags) {
          if (!groups.has(tag)) {
            groups.set(tag, []);
          }
          groups.get(tag).push(record);
        }
      }

      const orderedTags = tagStats
        .map((item) => item.tag)
        .filter((tag) => groups.has(tag));

      let html = "";
      for (const tag of orderedTags) {
        const groupRecords = groups.get(tag) || [];
        html += `
          <section class="group">
            <div class="group-head">
              <strong>${escapeHtml(tag)}</strong>
              <span class="meta">${groupRecords.length} record(s)</span>
            </div>
            <div class="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th style="width:20%">Key</th>
                    <th style="width:52%">Value</th>
                    <th style="width:28%">Updated</th>
                  </tr>
                </thead>
                <tbody>
                  ${groupRecords.map((record) => `
                    <tr>
                      <td><strong>${escapeHtml(record.key)}</strong></td>
                      <td class="value">${escapeHtml(record.value)}</td>
                      <td>${escapeHtml(formatDate(record.updated_at))}</td>
                    </tr>
                  `).join("")}
                </tbody>
              </table>
            </div>
          </section>
        `;
      }

      if (untagged.length > 0) {
        html += `
          <section class="group">
            <div class="group-head">
              <strong>(untagged)</strong>
              <span class="meta">${untagged.length} record(s)</span>
            </div>
            <div class="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th style="width:20%">Key</th>
                    <th style="width:52%">Value</th>
                    <th style="width:28%">Updated</th>
                  </tr>
                </thead>
                <tbody>
                  ${untagged.map((record) => `
                    <tr>
                      <td><strong>${escapeHtml(record.key)}</strong></td>
                      <td class="value">${escapeHtml(record.value)}</td>
                      <td>${escapeHtml(formatDate(record.updated_at))}</td>
                    </tr>
                  `).join("")}
                </tbody>
              </table>
            </div>
          </section>
        `;
      }

      groupViewEl.innerHTML = html;
    }

    function render() {
      const filtered = filteredRecords();
      recordMetaEl.textContent = `${filtered.length} record(s), sorted by latest update`;
      renderSummary(filtered);
      renderRecents();
      renderTagExplorer();
      renderList(filtered);
      renderGrouped(filtered);
    }

    function applyPayload(nextPayloadText) {
      const normalized = normalizePayload(nextPayloadText);
      if (normalized === lastNormalizedPayload) {
        return;
      }

      let parsed;
      try {
        parsed = JSON.parse(nextPayloadText || "[]");
      } catch (_) {
        return;
      }

      lastNormalizedPayload = normalized;
      records = normalizeRecords(parsed);
      tagStats = buildTagStats(records);

      // Drop selected tags that no longer exist in current dataset.
      const availableTags = new Set(tagStats.map((item) => item.tag));
      for (const tag of Array.from(selectedTags)) {
        if (!availableTags.has(tag)) {
          selectedTags.delete(tag);
        }
      }

      render();
    }

    queryInput.addEventListener("input", render);
    listModeBtn.addEventListener("click", () => setViewMode("list"));
    groupModeBtn.addEventListener("click", () => setViewMode("group"));
    clearAllBtn.addEventListener("click", () => {
      queryInput.value = "";
      tagSearchEl.value = "";
      selectedTags.clear();
      tagLimit = tagPageSize;
      render();
    });
    tagSearchEl.addEventListener("input", () => {
      tagLimit = tagPageSize;
      renderTagExplorer();
    });
    tagMoreEl.addEventListener("click", () => {
      tagLimit += tagPageSize;
      renderTagExplorer();
    });

    selectedTagsEl.addEventListener("click", (event) => {
      const target = event.target;
      if (!(target instanceof HTMLElement)) {
        return;
      }
      const tag = target.getAttribute("data-tag-remove");
      if (tag) {
        selectedTags.delete(tag);
        render();
      }
    });

    tagRowsEl.addEventListener("click", (event) => {
      const target = event.target;
      if (!(target instanceof HTMLElement)) {
        return;
      }
      const button = target.closest("[data-tag-toggle]");
      if (!button) {
        return;
      }
      const tag = button.getAttribute("data-tag-toggle");
      if (tag) {
        toggleTag(tag);
      }
    });

    recentListEl.addEventListener("click", (event) => {
      const target = event.target;
      if (!(target instanceof HTMLElement)) {
        return;
      }
      const button = target.closest("[data-focus-key]");
      if (!button) {
        return;
      }
      const key = button.getAttribute("data-focus-key");
      if (!key) {
        return;
      }
      queryInput.value = key;
      setViewMode("list");
      recordsCardEl.scrollIntoView({ behavior: "smooth", block: "start" });
    });

    setViewMode("list");

    if (liveEndpoint) {
      window.setInterval(async () => {
        try {
          const response = await fetch(liveEndpoint, { cache: "no-store" });
          if (!response.ok) {
            return;
          }
          const nextText = await response.text();
          applyPayload(nextText);
        } catch (_) {}
      }, pollIntervalMs);
    }
  </script>
</body>
</html>
"#;
    Ok(template
        .replace("__KV_DATA__", &safe_json)
        .replace("__KV_POLL__", &poll_endpoint_json))
}

fn serialize_html_records(store: &Store) -> KvResult<String> {
    let records: Vec<_> = store
        .ordered()
        .into_iter()
        .map(|(key, entry)| HtmlEntry {
            key: key.as_str(),
            value: entry.value(),
            tags: entry.tags(),
            created_at: entry.created_at().to_rfc3339(),
            updated_at: entry.updated_at().to_rfc3339(),
        })
        .collect();
    Ok(serde_json::to_string(&records)?)
}

fn default_storage_dir() -> PathBuf {
    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(APP_DIR);
    }
    if let Some(profile) = env::var_os("USERPROFILE").filter(|value| !value.is_empty()) {
        return PathBuf::from(profile).join(APP_DIR);
    }
    PathBuf::from(APP_DIR)
}

fn default_data_file_path(namespace: &str) -> PathBuf {
    if let Some(explicit) = env::var_os("KVSTORE_DATA_FILE").filter(|value| !value.is_empty()) {
        return PathBuf::from(explicit);
    }
    namespace_dir(namespace).join(DEFAULT_DATA_FILE_NAME)
}

fn default_recent_log_path(namespace: &str) -> PathBuf {
    if let Some(explicit) = env::var_os("KVSTORE_RECENT_FILE").filter(|value| !value.is_empty()) {
        return PathBuf::from(explicit);
    }
    namespace_dir(namespace)
        .join("logs")
        .join(DEFAULT_RECENT_LOG_NAME)
}

fn namespace_dir(namespace: &str) -> PathBuf {
    default_storage_dir().join(NAMESPACES_DIR).join(namespace)
}

fn resolve_namespace(raw: Option<&str>) -> KvResult<String> {
    let namespace = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            env::var("KVSTORE_NAMESPACE")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| DEFAULT_NAMESPACE.to_string());

    validate_namespace(&namespace)?;
    Ok(namespace)
}

fn validate_namespace(namespace: &str) -> KvResult<()> {
    let is_valid = namespace
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'));

    if is_valid {
        Ok(())
    } else {
        Err(KvError::InvalidInput(format!(
            "invalid namespace '{namespace}'; use letters, numbers, '_', '-', or '.'"
        )))
    }
}

fn snapshot_store(database: &Database) -> KvResult<Store> {
    let entries = database.load_entries()?;
    Ok(Store::from_entries(entries))
}

fn render_live_html(database: &Database) -> KvResult<String> {
    let store = snapshot_store(database)?;
    render_html_with_options(&store, "/data")
}

fn render_live_data(database: &Database) -> KvResult<String> {
    let store = snapshot_store(database)?;
    serialize_html_records(&store)
}

fn serve_viewer(
    database: &Database,
    data_path: &Path,
    namespace: &str,
    host: &str,
    port: u16,
) -> KvResult<()> {
    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr)?;
    println!("Serving kvstore viewer at http://{addr}");
    println!("Namespace: {namespace}");
    println!("Data source: {}", data_path.display());
    println!("Press Ctrl+C to stop.");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle_http_connection(stream, database) {
                    warn!("viewer request failed: {}", error);
                }
            }
            Err(error) => {
                warn!("failed to accept viewer connection: {}", error);
            }
        }
    }

    Ok(())
}

fn handle_http_connection(mut stream: TcpStream, database: &Database) -> KvResult<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }

    loop {
        let mut header_line = String::new();
        let bytes = reader.read_line(&mut header_line)?;
        if bytes == 0 || header_line == "\r\n" {
            break;
        }
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let raw_path = parts.next().unwrap_or("/");
    let path = raw_path.split('?').next().unwrap_or("/");

    match (method, path) {
        ("GET", "/") => {
            let body = render_live_html(database)?;
            write_http_response(&mut stream, "200 OK", "text/html; charset=utf-8", &body)?;
        }
        ("GET", "/data") => {
            let body = render_live_data(database)?;
            write_http_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                &body,
            )?;
        }
        ("GET", "/health") => {
            write_http_response(&mut stream, "200 OK", "text/plain; charset=utf-8", "ok\n")?;
        }
        ("GET", "/favicon.ico") => {
            write_http_response(
                &mut stream,
                "204 No Content",
                "text/plain; charset=utf-8",
                "",
            )?;
        }
        _ => {
            write_http_response(
                &mut stream,
                "404 Not Found",
                "text/plain; charset=utf-8",
                "not found\n",
            )?;
        }
    }

    Ok(())
}

fn write_http_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> KvResult<()> {
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn validate_markdown_path(path: &Path, any_file: bool, label: &str) -> KvResult<()> {
    if any_file {
        return Ok(());
    }

    let is_markdown = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));

    if is_markdown {
        Ok(())
    } else {
        Err(KvError::InvalidInput(format!(
            "{label} must end with '.md' (or pass --any-file): {}",
            path.display()
        )))
    }
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

#[derive(Serialize)]
struct HtmlEntry<'a> {
    key: &'a str,
    value: &'a str,
    tags: &'a [String],
    created_at: String,
    updated_at: String,
}
