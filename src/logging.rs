use chrono::Local;
use std::path::Path;
use tracing::info;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter, Registry};

/// Sets up tracing with INFO+ to console and DEBUG+ from your crate to a file.
pub fn setup_tracing(
    log_dir: Option<&str>,
) -> Result<(NonBlocking, WorkerGuard), Box<dyn std::error::Error>> {
    // Use the provided log directory or default to "logs"
    let log_dir_str = log_dir.unwrap_or("logs");
    let log_dir = Path::new(log_dir_str);
    if !log_dir.exists() {
        std::fs::create_dir_all(log_dir)?;
    }

    // Generate a timestamped log file name
    let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();

    // Set up file appender with non-blocking writer
    let file_appender = tracing_appender::rolling::never(log_dir_str, format!("{}.log", timestamp));
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Console layer: INFO and above for all logs
    let console_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_ansi(true) // Enable colored output
        .with_level(true)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        // Set the global filter for console logging
        .with_filter(EnvFilter::from_default_env().add_directive("INFO".parse()?));

    // File layer: DEBUG and above **only** for your crate
    let file_layer = fmt::layer()
        .with_writer(non_blocking.clone())
        .with_ansi(false) // Disable ANSI colors in file
        .with_level(true)
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        // Set the filter to include only your crate's logs at DEBUG level
        .with_filter(EnvFilter::from_default_env().add_directive("krypto=DEBUG".parse()?));

    // Combine layers
    let subscriber = Registry::default().with(console_layer).with(file_layer);

    // Initialize subscriber
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Tracing initialized. Logs will be written to console and file.");

    Ok((non_blocking, guard))
}
