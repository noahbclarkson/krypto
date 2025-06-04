use chrono::Local;
use std::path::Path;
use tracing::info;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter, Registry};

/// Sets up tracing with INFO+ to console and DEBUG+ from your crate to a file.
/// Log levels can be further customized via the RUST_LOG environment variable.
/// E.g., RUST_LOG="info,krypto=trace"
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

    // Default directives: INFO globally, DEBUG for krypto crate
    let default_filter = "info";

    // Console layer: Controlled by RUST_LOG or default_filter
    let console_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_ansi(true) // Enable colored output
        .with_level(true)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter)),
        );

    let default_filter = "info,krypto=info";

    // File layer: Controlled by RUST_LOG or default_filter
    let file_layer = fmt::layer()
        .with_writer(non_blocking.clone())
        .with_ansi(false) // Disable ANSI colors in file
        .with_level(true)
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter)),
        );

    // Combine layers
    let subscriber = Registry::default().with(console_layer).with(file_layer);

    // Initialize subscriber
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Tracing initialized. Logs will be written to console and file.");
    info!(
        "Log level configured via RUST_LOG env var (default: '{}')",
        default_filter
    );

    Ok((non_blocking, guard))
}
