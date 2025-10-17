# kvstore

`kvstore` is a lightweight Rust CLI for storing and retrieving key–value pairs.  
Entries are durably stored in a bundled SQLite database while a fast in-memory cache backs fuzzy lookups.

## Features
- Bundled SQLite persistence (`data.db` by default) with automatic schema setup.
- Memory-cached reads for instant lookups and fuzzy search.
- Add, update, list, fetch, delete, export, and import entries.
- Optional tagging on every key; tags are stored and normalised.
- Fuzzy search over keys _and_ tags (configurable to target one or the other).
- Inline live-search mode (`kv f`) that refreshes results as you type in the same terminal buffer.

## Installation

```bash
cargo install --path .
```

This places the `kvstore` binary in `~/.cargo/bin`. Add an alias for convenient access:

```bash
echo 'alias kv=kvstore' >> ~/.zshrc   # or ~/.bashrc
source ~/.zshrc
```

Now `kv …` works from any directory.

## Storage Model
- On startup the app opens the SQLite database, applies migrations, and loads all rows into memory (`HashMap<String, Entry>` + cached key list).
- Reads (`get`, `list`, `search`, `f`) operate exclusively on the in-memory cache.
- Mutations (`add`, `remove`, `import`) are wrapped in SQLite transactions; once the write succeeds the cache is updated in lockstep.
- Export/import still work with JSON snapshots for easy backups or transfers.

## Logging
- Runtime logs are written to `logs/kvstore.log` relative to the working directory.
- The directory is created on demand; all levels default to `info` with timestamped entries.
- Logs are not echoed to stdout/stderr, so the CLI output stays clean.

## Configuration
- Optional settings can be supplied via `kvstore.toml` (project root) or `config/kvstore.toml`.
- Example:

```toml
[logging]
level = "warn"      # trace | debug | info | warn | error
file = "kvstore.log" # relative paths live under ./logs/
```

- Environment variable `KVSTORE_LOG_LEVEL` continues to override the configured level when present.

## Usage

```
kv <command> [args]
```

| Shortcut | Long form        | Description |
|----------|------------------|-------------|
| `kv a`   | `kv add`         | Add/update: `kv a key value -t tag1 -t tag2` |
| `kv g`   | `kv get`         | Print the stored value: `kv g key` |
| `kv r`   | `kv remove`      | Delete a key: `kv r key` |
| `kv l`   | `kv list`        | List all keys in lexical order |
| `kv s`   | `kv search`      | Fuzzy search keys & tags: `kv s pattern [-l 20] [--keys|--tags]` |
| `kv f`   | `kv interactive` | Live fuzzy search (updates on each keystroke) |
| `kv e`   | `kv export`      | Export to JSON: `kv e backup.json` |
| `kv i`   | `kv import`      | Import from JSON: `kv i backup.json` |

### Tagging
- Supply tags during add/update with `-t/--tag`.
- Repeating `-t` arguments replaces the tag set for that key.
- Omitting `-t` keeps existing tags when updating a value.

### Search Modes
- Default: search keys and tags simultaneously.
- `--keys` limits fuzzy matching to keys only.
- `--tags` limits fuzzy matching to tags only.
- Live mode (`kv f`) accepts the same `--keys`/`--tags` flags.

### Live Search Experience
- Launch with `kv f`.
- Results stay in the current terminal page; press `Esc`, `Enter`, or `Ctrl+C` to exit.
- `-l/--limit` controls how many matches appear (default: 10).

## Data Management
- Pass `--data-file path/to/store.db` to operate on an alternative SQLite database.
- `kv export` / `kv import` provide quick JSON backups or migrations between machines.

## Development

```bash
cargo fmt
cargo test
```

The workspace contains no additional tests by default, but the CLI is exercised via the integration workflow above.
