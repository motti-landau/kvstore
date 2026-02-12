# kvstore

`kvstore` is a lightweight Rust CLI for storing and retrieving key-value pairs.
Data is persisted in SQLite, cached in memory for fast reads, and exposed through both CLI and HTML viewers.

## Highlights
- Namespace-first storage model (`default`, `work`, `live`, `investments`, etc.).
- SQLite persistence + in-memory cache for fast `get`, `list`, and fuzzy search.
- Implicit command inference:
  - `kv` -> interactive fuzzy mode
  - `kv <key>` -> get
  - `kv <key> <value> [@tag ...]` -> add/update
- Explicit commands for add/get/remove/list/search/recent/export/import.
- Markdown file workflows:
  - `put-file` stores full file content in a key
  - `get-file` writes key content back to a file
- Optional TTL per record (in minutes), default is permanent
- HTML UI:
  - Static export (`kv html`)
  - Live local server with polling (`kv serve`)
  - Runtime output shows `namespace` + `data source` path to avoid confusion
- Rich HTML experience:
  - records-first layout
  - recents panel
  - tag explorer with counts and last update time
  - grouped-by-tag mode
  - client-side filtering and tag chips
  - live CRUD for records and tags when running `kv serve`

## Installation
```bash
cargo install --path .
```

Make sure Cargo bin is on `PATH`:
```bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

Optional alias:
```bash
echo 'alias kv=kvstore' >> ~/.zshrc
source ~/.zshrc
```

## Storage Model

### Namespaces (default behavior)
If you do not pass `--data-file`, kvstore stores data under:
- `~/.kvstore/namespaces/<namespace>/data.db`
- `~/.kvstore/namespaces/<namespace>/logs/recent.log`

Default namespace is `default`.

Examples:
```bash
kv -n work add roadmap "Q2 goals" @planning
kv -n live list
kv -n investments serve --port 7888
```

### Namespace Selection
Precedence:
1. `--namespace/-n <name>`
2. `KVSTORE_NAMESPACE` environment variable
3. `default`

Valid namespace characters:
- letters, numbers, `_`, `-`, `.`
- namespace cannot be `.` or `..`

### Advanced Override
You can still bypass namespace DB resolution with:
```bash
kv --data-file /path/to/custom.db ...
```

This is intended for advanced/custom workflows.

## Commands

### Implicit
- `kv` -> interactive mode
- `kv <key>` -> get
- `kv <key> <value> [@tag ...]` -> add/update

### Explicit
- `kv add <key> [value] [@tag ...]`
- `kv get <key>`
- `kv remove <key>`
- `kv list`
- `kv search <pattern> [--keys|--tags] [-l <limit>]`
- `kv interactive`
- `kv recent [-l <count>]`
- `kv export <path.json>`
- `kv import <path.json>`
- `kv html [-o|--path <file.html>]`
- `kv serve [--host 127.0.0.1] [-p|--port 7878]`
- `kv put-file <key> <path.md> [@tag ...] [--any-file]`
- `kv get-file <key> <path.md> [--any-file]`

## HTML UI

### Static Export
```bash
kv -n work html --path work-view.html
```

### Live Server (Polling)
```bash
kv -n work serve
# open http://127.0.0.1:7878
```

Live page behavior:
- serves UI from `/`
- polls `/data` every few seconds
- applies updates in-place when payload changes (no manual refresh required)
- exposes write endpoints under `/api/*` for UI mutations
- runs TTL cleanup with up to ~1 hour cleanup SLA after expiry

### Current UI Capabilities
- Main records table is shown first.
- Records sorted by latest update.
- Recents panel shows latest updated keys.
- Tag explorer sorted by tag activity (latest update first), with count and pagination.
- Toggle between list view and grouped-by-tag view.
- Search filters key/value/tags client-side.
- Record CRUD in-page (create/update/delete) from the editor and row actions.
- TTL in-page:
  - set TTL in minutes during save (blank means permanent)
  - view remaining time in records table
  - extend TTL from row action
- Tag CRUD from the live UI:
  - add/remove per record
  - rename/delete globally from Tag Explorer

Note: static HTML export (`kv html`) is read-only by design.

### Live Update Troubleshooting
If you run `kv serve` and updates do not appear:
1. Ensure writer and server use the same namespace (for example both with `-n work`).
2. Ensure writer and server use the same database override (if using `--data-file`).
3. Check server startup output (`Namespace:` and `Data source:` lines) and compare to your writer command.

## File Workflows (Codex-friendly)

Store full markdown content under a key:
```bash
kv -n work put-file project_summary ./notes/project_summary.md @codex @summary
```

Write key content back to a file:
```bash
kv -n work get-file project_summary ./notes/project_summary_out.md
```

By default, `put-file` and `get-file` require `.md` paths.
Use `--any-file` to disable that guard.

## Makefile Shortcuts
A `Makefile` is included for common commands.

Examples:
```bash
make help
make list NS=work
make add NS=work KEY=todo VALUE="ship v1" TAGS="@roadmap @priority"
make serve NS=work PORT=7878
make html NS=work HTML_OUT=work-view.html
make audit
make put-file NS=work KEY=project_summary FILE=./notes/summary.md TAGS="@codex @summary"
```

Main variables:
- `NS` (default: `default`)
- `DATA_FILE` (advanced override)
- `HOST` / `PORT` for `serve`
- `HTML_OUT` for `html`
- `KEY`, `VALUE`, `FILE`, `TAGS`, `QUERY`, `LIMIT` for command-specific targets

## Interactive Mode
Launch with:
```bash
kv -n work
# or
kv -n work interactive
```

Interactive output now uses compact previews:
- multiline values are flattened to one line
- long keys/values/tags are truncated for readability

## Configuration (`kvstore.toml`)
```toml
[logging]
level = "warn"       # trace | debug | info | warn | error
file = "kvstore.log" # relative paths live under ./logs/

[history]
file = "logs/recent.log" # optional override; default is namespace path
limit = 25
```

## Development
```bash
cargo fmt
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## Runtime Artifacts
These files are runtime outputs and should not be committed:
- `kvstore-view.html`
- `data.db`, `data.db-shm`, `data.db-wal`
- `logs/`
