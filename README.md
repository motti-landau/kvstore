# kvstore

`kvstore` is a lightweight Rust CLI for storing and retrieving key–value pairs.  
Entries are durably stored in a bundled SQLite database while a fast in-memory cache backs fuzzy lookups, implicit command inference, and persisted recent history.

## Features
- Bundled SQLite persistence (`data.db` by default) with automatic schema setup.
- Memory-cached reads for instant lookups and fuzzy search.
- Implicit command inference: type just `kv`, `kv key`, or `kv key value [@tag …]` for the common flows.
- Add, update, list, fetch, delete, recent history, export, and import entries (explicit subcommands still work when you need them).
- Optional tagging on every key; tags are stored and normalised.
- Fuzzy search over keys _and_ tags (configurable to target one or the other).
- Inline live-search mode (`kv f`) that refreshes results as you type in the same terminal buffer.
- Recent command (`kv recent`) that remembers the last accesses across CLI sessions.

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

## Logging & History
- Runtime logs default to `logs/kvstore.log` relative to the working directory (directory created on demand).
- Recent key usage is persisted to `logs/recent.log` so `kv recent` survives across runs.
- Both files and the retention limit (default 25 entries) are configurable in `kvstore.toml`.
- Environment variable `KVSTORE_LOG_LEVEL` continues to override the configured log level when present.

### Configuration snippet
```toml
[logging]
level = "warn"       # trace | debug | info | warn | error
file = "kvstore.log" # relative paths live under ./logs/

[history]
file = "logs/recent.log"
limit = 25
```

## Usage

### Implicit commands
- `kv` (no arguments) launches interactive mode.
- `kv <key>` fetches the value for `<key>`.
- `kv <key> <value> [@tag …]` stores/updates `<key>` with `<value>` and optional tags.
- Prefix any tags with `@` (e.g., `@prod @api`). Supplying only tags records an entry with an empty value.
- Reserved words (`add`, `get`, `recent`, etc.) are always treated as explicit commands; access literal keys with `kv get <reserved>`.

### Explicit subcommands (still available when you need them)
| Command           | Description |
|-------------------|-------------|
| `kv remove <key>` | Delete a key |
| `kv list`         | List keys in lexical order |
| `kv search …`     | Fuzzy search keys & tags (`--keys`/`--tags`, `-l` limit) |
| `kv recent`       | Show the last accessed keys (persisted across sessions) |
| `kv interactive`  | Live fuzzy search (same as running bare `kv`) |
| `kv export …`     | Export to JSON |
| `kv import …`     | Import from JSON |

Side note: legacy aliases like `kv add`, `kv get`, and their single-letter forms remain functional for scripts that depend on them.

### Tagging
- Append `@tag` arguments during implicit or explicit add/update (`kv foo bar @prod @api`).
- Repeating tag arguments replaces the tag set for that key.
- Omitting tags keeps the existing tag set when updating a value.

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
