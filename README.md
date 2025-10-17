# kvstore

`kvstore` is a lightweight Rust CLI for storing and retrieving key–value pairs in a local JSON file.  
Keys can carry optional tags, and fuzzy search spans both keys and tags for quick recall.

## Features
- Plain JSON persistence (`data.json` by default) with automatic file bootstrapping.
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

## Storage Format

Entries are stored as:

```json
{
  "example": {
    "value": "Some detail",
    "tags": ["note", "personal"]
  }
}
```

Existing flat JSON files (mapping keys straight to string values) are upgraded automatically on load.

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
- Pass `--data-file path/to/file.json` to operate on an alternative store.
- `kv export` / `kv import` provide quick backups or migrations between machines.

## Development

```bash
cargo fmt
cargo test
```

The workspace contains no additional tests by default, but the CLI is exercised via the integration workflow above.
