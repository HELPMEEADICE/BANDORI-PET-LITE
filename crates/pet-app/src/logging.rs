//! Logging setup shared by both executables.
//!
//! Logs go to `logs/pet.<date>.log` (daily rolling) and to stderr so a console run
//! stays debuggable. `RUST_LOG` overrides the default filter.

use std::path::Path;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Initialises logging. Returns the file-appender guard which must stay alive.
pub fn init(
    log_dir: &Path,
    default_level: &str,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_ansi(true);

    let (file_layer, guard) = match std::fs::create_dir_all(log_dir) {
        Ok(()) => {
            let appender = tracing_appender::rolling::Builder::new()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("pet")
                .filename_suffix("log")
                .build(log_dir)
                .ok();
            match appender {
                Some(appender) => {
                    let (writer, guard) = tracing_appender::non_blocking(appender);
                    let layer = tracing_subscriber::fmt::layer()
                        .with_writer(writer)
                        .with_target(true)
                        .with_ansi(false);
                    (Some(layer), Some(guard))
                }
                None => (None, None),
            }
        }
        Err(_) => (None, None),
    };

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init();

    guard
}
