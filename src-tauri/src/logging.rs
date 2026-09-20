use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// Sets up both console and rolling file logging.
/// Returns WorkerGuard that must be held alive for the lifetime of the application.
pub fn setup_logging(logs_dir: &Path) -> Option<WorkerGuard> {
    let _ = std::fs::create_dir_all(logs_dir);

    // 1. Rolling file appender: daily rotation in logs_dir
    let file_appender = tracing_appender::rolling::daily(logs_dir, "browsory.log");
    let (non_blocking_file, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Console layer
    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_filter(EnvFilter::new("info"));

    // File layer
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(non_blocking_file)
        .with_filter(env_filter);

    let subscriber = tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer);

    if subscriber.try_init().is_ok() {
        Some(guard)
    } else {
        None
    }
}
