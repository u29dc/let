#![forbid(unsafe_code)]

use tracing_subscriber::{EnvFilter, fmt};

pub fn init_logging(default_directive: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_directive));
    let _ = fmt().with_env_filter(filter).try_init();
}
