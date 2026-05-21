use tracing_subscriber::EnvFilter;

/// Initialises the global tracing subscriber. Reads `RUST_LOG` for level filter and `LOG_FORMAT=json` for structured JSON output. Safe to call multiple times.
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    if std::env::var("LOG_FORMAT").as_deref() == Ok("json") {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .with_target(true)
            .try_init()
            .ok();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .try_init()
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        init();
        init(); // second call must not panic
        tracing::info!("logging init test");
    }
}
