use kvstore::cli::Cli;
use kvstore::settings::AppSettings;

fn main() {
    let settings = AppSettings::load();
    init_logging(&settings);

    let cli = Cli::parse();
    if let Err(error) = kvstore::run(cli, &settings) {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

fn init_logging(settings: &AppSettings) {
    use simplelog::{ConfigBuilder, LevelFilter, WriteLogger};
    use std::env;
    use std::fs::{create_dir_all, OpenOptions};
    use std::path::Path;

    const LOG_DIR: &str = "logs";
    const LOG_FILE: &str = "kvstore.log";
    const LOG_LEVEL_ENV: &str = "KVSTORE_LOG_LEVEL";

    let configured_path = settings
        .logging()
        .file
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .unwrap_or(LOG_FILE);

    let log_path = if Path::new(configured_path).is_absolute() {
        configured_path.to_string()
    } else {
        if let Err(error) = create_dir_all(LOG_DIR) {
            eprintln!("Failed to create log directory '{LOG_DIR}': {error}");
            return;
        }
        format!("{LOG_DIR}/{configured_path}")
    };

    if let Some(parent) = Path::new(&log_path).parent() {
        if let Err(error) = create_dir_all(parent) {
            eprintln!(
                "Failed to create log directory '{}': {error}",
                parent.display()
            );
            return;
        }
    }

    let file = match OpenOptions::new().create(true).append(true).open(&log_path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("Failed to open log file '{log_path}': {error}");
            return;
        }
    };

    let mut config_builder = ConfigBuilder::new();
    let _ = config_builder.set_time_offset_to_local();
    let config = config_builder.build();

    let level = env::var(LOG_LEVEL_ENV)
        .ok()
        .and_then(|value| match value.to_uppercase().as_str() {
            "TRACE" => Some(LevelFilter::Trace),
            "DEBUG" => Some(LevelFilter::Debug),
            "INFO" => Some(LevelFilter::Info),
            "WARN" | "WARNING" => Some(LevelFilter::Warn),
            "ERROR" => Some(LevelFilter::Error),
            _ => None,
        })
        .or_else(|| settings.logging().level_filter())
        .unwrap_or(LevelFilter::Warn);

    if let Err(error) = WriteLogger::init(level, config, file) {
        eprintln!("Failed to initialise file logger: {error}");
    }
}
