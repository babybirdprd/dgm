pub mod agent;
pub mod config;
pub mod dgm;
pub mod evaluation;
pub mod llm;
pub mod prompts;
pub mod tools;
pub mod utils;

pub use anyhow::{Context, Result};
pub use serde::{Deserialize, Serialize};
pub use tracing::{debug, error, info, warn};

/// Common result type used throughout the application
pub type DgmResult<T> = Result<T, anyhow::Error>;

/// Initialize logging for the application
pub fn init_logging() -> DgmResult<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .map_err(|e| anyhow::anyhow!("Failed to initialize logging: {}", e))?;
    Ok(())
}

/// Load environment variables from .env file if present
pub fn load_env() {
    if let Err(e) = dotenv::dotenv() {
        tracing::debug!("No .env file found or failed to load: {}", e);
    }
}
