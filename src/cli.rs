use std::path::PathBuf;

use clap::{error::ErrorKind, CommandFactory, Parser, Subcommand};

pub const DEFAULT_INTERACTIVE_LIMIT: usize = 10;
const HELP_EXAMPLES: &str = r#"Examples:
  kv foo bar            # Add key/value implicitly
  kv foo bar @prod @api # Add with tags
  kv -n work foo bar    # Same command in 'work' namespace
  kv foo                # Get value implicitly
  kv foo @prod          # Add with empty value and tags
  kv                    # Interactive fuzzy finder

Explicit commands:
  kv add foo bar @prod  # Add/update with tags
  kv get foo            # Get a value
  kv remove foo         # Delete a key
  kv list               # List all keys
  kv search api -l 5    # Fuzzy search with limit
  kv interactive        # Live fuzzy finder mode
  kv export backup.json # Export to JSON
  kv import backup.json # Import from JSON
  kv html               # Generate browser view
  kv serve              # Run local live viewer (polling)
  kv put-file notes README.md @project # Save markdown file contents
  kv get-file notes out.md             # Write value to markdown file
  kv recent             # Show recently accessed keys
"#;

pub const RESERVED_KEYWORDS: &[&str] = &[
    "add",
    "a",
    "get",
    "g",
    "remove",
    "r",
    "list",
    "l",
    "search",
    "s",
    "interactive",
    "f",
    "export",
    "e",
    "import",
    "i",
    "html",
    "view",
    "browse",
    "serve",
    "sv",
    "put-file",
    "pf",
    "get-file",
    "gf",
    "recent",
];

/// Public CLI representation consumed by the application.
#[derive(Debug)]
pub struct Cli {
    pub data_file: Option<PathBuf>,
    pub namespace: Option<String>,
    pub command: Command,
}

#[derive(Debug, Parser)]
#[command(
    name = "kv",
    author,
    version,
    about = "Simple key-value store backed by SQLite with an in-memory cache",
    long_about = "Simple key-value store backed by SQLite with an in-memory cache.\n\
Supports implicit commands (kv <key> ...) and explicit subcommands for advanced usage.",
    disable_help_subcommand = true,
    propagate_version = true,
    after_long_help = HELP_EXAMPLES
)]
struct RawCli {
    /// Namespace for storage under $HOME/.kvstore/namespaces/<name>/...
    #[arg(short, long, global = true, value_name = "NAME")]
    namespace: Option<String>,

    /// Path to the SQLite database file (advanced override; bypasses namespace DB path)
    #[arg(long, global = true, value_name = "FILE")]
    data_file: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<RawCommand>,
}

/// Subcommands supported by the CLI. Used for both explicit and inferred commands.
#[derive(Debug, Subcommand)]
enum RawCommand {
    /// Adds or updates a key-value pair. Shortcut: `a`
    #[command(name = "add", alias = "a", trailing_var_arg = true)]
    Add {
        key: String,
        #[arg(value_name = "VALUE|@TAG", num_args = 0..)]
        rest: Vec<String>,
    },
    /// Retrieves the value stored for a key. Shortcut: `g`
    #[command(name = "get", alias = "g")]
    Get { key: String },
    /// Removes the given key and its value. Shortcut: `r`
    #[command(name = "remove", alias = "r", aliases = ["delete", "rm"])]
    Remove {
        /// Key to remove.
        key: String,
    },
    /// Lists all stored key-value pairs. Shortcut: `l`
    #[command(name = "list", alias = "l")]
    List,
    /// Performs fuzzy search on keys. Shortcut: `s`
    #[command(name = "search", alias = "s")]
    Search {
        /// Pattern to fuzzy match against stored keys.
        pattern: String,
        /// Maximum number of matches to display.
        #[arg(short, long, default_value_t = DEFAULT_INTERACTIVE_LIMIT)]
        limit: usize,
        /// Search only within tags.
        #[arg(long = "tags", conflicts_with = "keys_only")]
        tags_only: bool,
        /// Search only within keys.
        #[arg(long = "keys", conflicts_with = "tags_only")]
        keys_only: bool,
    },
    /// Opens live fuzzy search. Shortcut: `f`
    #[command(name = "interactive", alias = "f", aliases = ["live"])]
    Interactive {
        /// Maximum number of matches to display.
        #[arg(short, long, default_value_t = DEFAULT_INTERACTIVE_LIMIT)]
        limit: usize,
        /// Search only within tags.
        #[arg(long = "tags", conflicts_with = "keys_only")]
        tags_only: bool,
        /// Search only within keys.
        #[arg(long = "keys", conflicts_with = "tags_only")]
        keys_only: bool,
    },
    /// Exports all entries. Shortcut: `e`
    #[command(name = "export", alias = "e")]
    Export {
        /// Destination file path.
        path: PathBuf,
    },
    /// Imports entries from the provided JSON file, replacing current data. Shortcut: `i`
    #[command(name = "import", alias = "i")]
    Import {
        /// Source file path.
        path: PathBuf,
    },
    /// Generates a standalone HTML file to browse all entries.
    #[command(name = "html", aliases = ["view", "browse"])]
    Html {
        /// Destination HTML file path.
        #[arg(short, long, value_name = "FILE", default_value = "kvstore-view.html")]
        path: PathBuf,
    },
    /// Runs a local HTTP server with a live-updating viewer. Shortcut: `sv`
    #[command(name = "serve", alias = "sv")]
    Serve {
        /// Bind host.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Bind port.
        #[arg(short, long, default_value_t = 7878)]
        port: u16,
    },
    /// Stores key value from a local file (Markdown by default). Shortcut: `pf`
    #[command(name = "put-file", alias = "pf", trailing_var_arg = true)]
    PutFile {
        /// Target key.
        key: String,
        /// Source file path.
        path: PathBuf,
        /// Optional @tags (e.g. @project @docs)
        #[arg(value_name = "@TAG", num_args = 0..)]
        tags: Vec<String>,
        /// Allow non-markdown file paths.
        #[arg(long)]
        any_file: bool,
    },
    /// Writes key value into a local file (Markdown by default). Shortcut: `gf`
    #[command(name = "get-file", alias = "gf")]
    GetFile {
        /// Source key.
        key: String,
        /// Output file path.
        path: PathBuf,
        /// Allow non-markdown file paths.
        #[arg(long)]
        any_file: bool,
    },
    /// Displays the most recently accessed keys.
    #[command(name = "recent")]
    Recent {
        /// Maximum number of keys to display.
        #[arg(short, long, value_name = "COUNT", default_value_t = DEFAULT_INTERACTIVE_LIMIT)]
        limit: usize,
    },
    /// Captures any external/unknown subcommand for implicit inference.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug)]
pub enum Command {
    Add {
        key: String,
        value: String,
        tags: Vec<String>,
    },
    Get {
        key: String,
    },
    Remove {
        key: String,
    },
    List,
    Search {
        pattern: String,
        limit: usize,
        tags_only: bool,
        keys_only: bool,
    },
    Interactive {
        limit: usize,
        tags_only: bool,
        keys_only: bool,
    },
    Export {
        path: PathBuf,
    },
    Import {
        path: PathBuf,
    },
    Html {
        path: PathBuf,
    },
    Serve {
        host: String,
        port: u16,
    },
    PutFile {
        key: String,
        path: PathBuf,
        tags: Vec<String>,
        any_file: bool,
    },
    GetFile {
        key: String,
        path: PathBuf,
        any_file: bool,
    },
    Recent {
        limit: usize,
    },
}

impl Cli {
    pub fn parse() -> Self {
        let raw = RawCli::parse();
        let command = match raw.command {
            None => Command::Interactive {
                limit: DEFAULT_INTERACTIVE_LIMIT,
                tags_only: false,
                keys_only: false,
            },
            Some(raw_command) => convert_command(raw_command),
        };

        Self {
            data_file: raw.data_file,
            namespace: raw.namespace,
            command,
        }
    }
}

fn convert_command(raw: RawCommand) -> Command {
    match raw {
        RawCommand::Add { key, rest } => {
            let (value, tags) = parse_value_and_tags(&rest);
            Command::Add { key, value, tags }
        }
        RawCommand::Get { key } => Command::Get { key },
        RawCommand::Remove { key } => Command::Remove { key },
        RawCommand::List => Command::List,
        RawCommand::Search {
            pattern,
            limit,
            tags_only,
            keys_only,
        } => Command::Search {
            pattern,
            limit,
            tags_only,
            keys_only,
        },
        RawCommand::Interactive {
            limit,
            tags_only,
            keys_only,
        } => Command::Interactive {
            limit,
            tags_only,
            keys_only,
        },
        RawCommand::Export { path } => Command::Export { path },
        RawCommand::Import { path } => Command::Import { path },
        RawCommand::Html { path } => Command::Html { path },
        RawCommand::Serve { host, port } => Command::Serve { host, port },
        RawCommand::PutFile {
            key,
            path,
            tags,
            any_file,
        } => Command::PutFile {
            key,
            path,
            tags: parse_tags_only(&tags),
            any_file,
        },
        RawCommand::GetFile {
            key,
            path,
            any_file,
        } => Command::GetFile {
            key,
            path,
            any_file,
        },
        RawCommand::Recent { limit } => Command::Recent { limit },
        RawCommand::External(args) => infer_command(args),
    }
}

fn infer_command(args: Vec<String>) -> Command {
    match args.as_slice() {
        [] => Command::Interactive {
            limit: DEFAULT_INTERACTIVE_LIMIT,
            tags_only: false,
            keys_only: false,
        },
        [candidate] => {
            if is_reserved(candidate) {
                usage_error(
                    ErrorKind::InvalidSubcommand,
                    &format!(
                        "'{candidate}' is a reserved command keyword. Use `kv {candidate} ...` explicitly."
                    ),
                );
            }
            Command::Get {
                key: candidate.clone(),
            }
        }
        [key, rest @ ..] => {
            if is_reserved(key) {
                usage_error(
                    ErrorKind::InvalidSubcommand,
                    &format!(
                        "'{key}' is a reserved command keyword. Use the explicit command form."
                    ),
                );
            }
            let (value, tags) = parse_value_and_tags(rest);
            Command::Add {
                key: key.clone(),
                value,
                tags,
            }
        }
    }
}

fn parse_value_and_tags(rest: &[String]) -> (String, Vec<String>) {
    if rest.is_empty() {
        return (String::new(), Vec::new());
    }

    let mut value: Option<String> = None;
    let mut tags = Vec::new();
    let mut tags_started = false;

    for token in rest {
        if let Some(tag) = parse_tag(token) {
            tags.push(tag);
            tags_started = true;
            continue;
        }

        if tags_started {
            usage_error(
                ErrorKind::InvalidValue,
                "Tags (arguments starting with '@') must come after the value.",
            );
        }

        if value.is_none() {
            value = Some(token.clone());
        } else {
            usage_error(
                ErrorKind::TooManyValues,
                "Too many positional arguments. Expect `kv <key> <value> @tag...`.",
            );
        }
    }

    (value.unwrap_or_default(), tags)
}

fn parse_tag(token: &str) -> Option<String> {
    if !token.starts_with('@') {
        return None;
    }
    let tag = token.trim_start_matches('@');
    if tag.is_empty() {
        usage_error(
            ErrorKind::InvalidValue,
            "Tag names cannot be empty. Use '@name'.",
        );
    }
    Some(tag.to_string())
}

fn parse_tags_only(tokens: &[String]) -> Vec<String> {
    let mut tags = Vec::with_capacity(tokens.len());
    for token in tokens {
        let Some(tag) = parse_tag(token) else {
            usage_error(
                ErrorKind::InvalidValue,
                "Tags must start with '@' for this command (example: @docs).",
            );
        };
        tags.push(tag);
    }
    tags
}

fn is_reserved(word: &str) -> bool {
    RESERVED_KEYWORDS
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(word))
}

fn usage_error(kind: ErrorKind, message: &str) -> ! {
    RawCli::command().error(kind, message).exit()
}
