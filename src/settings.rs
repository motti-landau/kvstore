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
