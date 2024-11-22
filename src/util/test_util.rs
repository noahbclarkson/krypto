use std::path::Path;
use tracing::{info, subscriber::set_default};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt;

use crate::{config::KryptoConfig, data::dataset::Dataset};

pub struct TracingGuards {
    _subscriber_guard: tracing::subscriber::DefaultGuard,
    _worker_guard: WorkerGuard,
}

pub fn setup_test_tracing(test_name: &str) -> TracingGuards {
    // Create logs directory if it doesn't exist
    let log_dir = Path::new("tests/logs");
    if !log_dir.exists() {
        std::fs::create_dir_all(log_dir).unwrap();
    }

    // Set up file appender with non-blocking writer
    let log_file = format!("tests/logs/{}.log", test_name);
    let file_appender = tracing_appender::rolling::never("", &log_file);
    let (non_blocking, worker_guard) = tracing_appender::non_blocking(file_appender);

    // Set up subscriber
    let subscriber = fmt::Subscriber::builder()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_level(true)
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_max_level(tracing::Level::DEBUG)
        .finish();

    // Set as default subscriber for this thread
    let subscriber_guard = set_default(subscriber);

    // Return guards to keep the subscriber and writer alive
    TracingGuards {
        _subscriber_guard: subscriber_guard,
        _worker_guard: worker_guard,
    }
}

pub fn setup_default_data(
    test_name: &str,
    config: Option<KryptoConfig>,
) -> (Dataset, TracingGuards) {
    let guards = setup_test_tracing(test_name);
    info!("-----------------");
    info!("Test: {}", test_name);
    info!("-----------------");
    let config = config.unwrap_or_default();
    let dataset = Dataset::load(&config).unwrap();
    (dataset, guards)
}
