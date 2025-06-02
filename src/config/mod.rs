use crate::{DgmResult, Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub aws_region: Option<String>,
    pub aws_region_name: Option<String>,
    pub aws_access_key_id: Option<String>,
    pub aws_secret_access_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerConfig {
    pub image_name: String,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationConfig {
    pub max_workers: u32,
    pub timeout_seconds: u64,
    pub num_evals_parallel: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DgmConfig {
    pub api: ApiConfig,
    pub docker: DockerConfig,
    pub evaluation: EvaluationConfig,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            anthropic_api_key: env::var("ANTHROPIC_API_KEY").ok(),
            openai_api_key: env::var("OPENAI_API_KEY").ok(),
            aws_region: env::var("AWS_REGION").ok(),
            aws_region_name: env::var("AWS_REGION_NAME").ok(),
            aws_access_key_id: env::var("AWS_ACCESS_KEY_ID").ok(),
            aws_secret_access_key: env::var("AWS_SECRET_ACCESS_KEY").ok(),
        }
    }
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            image_name: "dgm".to_string(),
            timeout_seconds: 1800, // 30 minutes
        }
    }
}

impl Default for EvaluationConfig {
    fn default() -> Self {
        Self {
            max_workers: 5,
            timeout_seconds: 3600, // 1 hour
            num_evals_parallel: 5,
        }
    }
}

impl Default for DgmConfig {
    fn default() -> Self {
        Self {
            api: ApiConfig::default(),
            docker: DockerConfig::default(),
            evaluation: EvaluationConfig::default(),
        }
    }
}

impl DgmConfig {
    pub fn load() -> DgmResult<Self> {
        Ok(Self::default())
    }

    pub fn validate(&self) -> DgmResult<()> {
        if self.api.anthropic_api_key.is_none() && self.api.openai_api_key.is_none() {
            anyhow::bail!("At least one API key (ANTHROPIC_API_KEY or OPENAI_API_KEY) must be set");
        }
        Ok(())
    }
}
