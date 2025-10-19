use std::fs;
use std::path::Path;

use log::LevelFilter;
use serde::Deserialize;

use crate::KvResult;

/// Represents the application configuration loaded from disk.
#[derive(Debug, Default, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    logging: LoggingSettings,
    #[serde(default)]
    history: HistorySettings,
}

impl AppSettings {
    const DEFAULT_PATHS: [&'static str; 2] = ["kvstore.toml", "config/kvstore.toml"];

    /// Attempts to load settings from the default locations, falling back to defaults.
    pub fn load() -> Self {
        for path in Self::DEFAULT_PATHS {
            if Path::new(path).exists() {
                match Self::load_from_path(path) {
                    Ok(settings) => return settings,
                    Err(error) => {
                        eprintln!("Failed to parse settings from '{path}': {error}");
                    }
                }
            }
        }
        AppSettings::default()
    }

    fn load_from_path(path: &str) -> KvResult<Self> {
        let data = fs::read_to_string(path)?;
        let settings = toml::from_str::<AppSettings>(&data)?;
        Ok(settings)
    }

    /// Returns an immutable reference to the logging configuration.
    pub fn logging(&self) -> &LoggingSettings {
        &self.logging
    }

    /// Returns an immutable reference to the history configuration.
    pub fn history(&self) -> &HistorySettings {
        &self.history
    }
}

/// Logging related settings parsed from the configuration file.
#[derive(Debug, Default, Deserialize)]
pub struct LoggingSettings {
    pub level: Option<String>,
    pub file: Option<String>,
}

impl LoggingSettings {
    /// Converts the textual level into a `LevelFilter`.
    pub fn level_filter(&self) -> Option<LevelFilter> {
        self.level.as_ref().and_then(|raw| parse_level(raw))
    }
}

/// Controls how the recent activity log behaves.
#[derive(Debug, Deserialize)]
pub struct HistorySettings {
    pub file: Option<String>,
    #[serde(default = "HistorySettings::default_limit")]
    limit: usize,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            file: None,
            limit: Self::default_limit(),
        }
    }
}

impl HistorySettings {
    const fn default_limit() -> usize {
        25
    }

    /// Maximum number of keys to retain in the recent log.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Optional path override for the recent log file.
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }
}

fn parse_level(raw: &str) -> Option<LevelFilter> {
    match raw.trim().to_uppercase().as_str() {
        "TRACE" => Some(LevelFilter::Trace),
        "DEBUG" => Some(LevelFilter::Debug),
        "INFO" => Some(LevelFilter::Info),
        "WARN" | "WARNING" => Some(LevelFilter::Warn),
        "ERROR" => Some(LevelFilter::Error),
        _ => None,
    }
}
