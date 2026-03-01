#![forbid(unsafe_code)]

use std::path::PathBuf;

use reqwest::Client;

use crate::errors::{ErrorCode, LetError, Result};

#[derive(Debug, Clone)]
pub struct RuntimeContext {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub sources_dir: PathBuf,
    pub http: Client,
}

impl RuntimeContext {
    pub fn new(
        config_dir: PathBuf,
        data_dir: PathBuf,
        cache_dir: PathBuf,
        sources_dir: PathBuf,
    ) -> Result<Self> {
        let http = Client::builder()
            .user_agent("let-rust/0.0.1")
            .build()
            .map_err(|err| {
                LetError::new(
                    ErrorCode::Internal,
                    format!("failed to initialize http client: {err}"),
                    "check tls/network runtime dependencies",
                )
            })?;

        Ok(Self {
            config_dir,
            data_dir,
            cache_dir,
            sources_dir,
            http,
        })
    }
}
