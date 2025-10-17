use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Command-line interface for the kvstore application.
#[derive(Debug, Parser)]
#[command(
    name = "kvstore",
    about = "Simple key-value store backed by a JSON file"
)]
#[command(author, version)]
pub struct Cli {
    /// Path to the JSON data file. Defaults to ./data.json
    #[arg(long, global = true, value_name = "FILE")]
    pub data_file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Adds or updates a key-value pair. Shortcut: `a`
    #[command(name = "a", alias = "add")]
    Add {
        key: String,
        value: String,
        #[arg(short, long, value_name = "TAG")]
        tags: Vec<String>,
    },
    #[command(name = "g", alias = "get")]
    Get {
        key: String,
    },
    /// Removes the given key and its value. Shortcut: `r`
    #[command(name = "r", aliases = ["remove", "delete", "rm"])]
    Remove {
        /// Key to remove.
        key: String,
    },
    /// Lists all stored key-value pairs. Shortcut: `l`
    #[command(name = "l", alias = "list")]
    List,
    /// Performs fuzzy search on keys. Shortcut: `s`
    #[command(name = "s", alias = "search")]
    Search {
        /// Pattern to fuzzy match against stored keys.
        pattern: String,
        /// Maximum number of matches to display.
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
        /// Search only within tags.
        #[arg(long = "tags", conflicts_with = "keys_only")]
        tags_only: bool,
        /// Search only within keys.
        #[arg(long = "keys", conflicts_with = "tags_only")]
        keys_only: bool,
    },
    /// Opens live fuzzy search. Shortcut: `f`
    #[command(name = "f", aliases = ["interactive", "live"])]
    Live {
        /// Maximum number of matches to display.
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
        /// Search only within tags.
        #[arg(long = "tags", conflicts_with = "keys_only")]
        tags_only: bool,
        /// Search only within keys.
        #[arg(long = "keys", conflicts_with = "tags_only")]
        keys_only: bool,
    },
    /// Exports all entries. Shortcut: `e`
    #[command(name = "e", alias = "export")]
    Export {
        /// Destination file path.
        path: PathBuf,
    },
    /// Imports entries from the provided JSON file, replacing current data. Shortcut: `i`
    #[command(name = "i", alias = "import")]
    Import {
        /// Source file path.
        path: PathBuf,
    },
}
