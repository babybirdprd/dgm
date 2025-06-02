pub mod archive;
pub mod evolution;
pub mod runner;

pub use archive::*;
pub use evolution::*;
pub use runner::*;

use crate::{DgmResult, Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DgmMetadata {
    pub generation: u32,
    pub selfimprove_entries: Vec<(String, String)>, // (parent_commit, entry)
    pub children: Vec<String>,
    pub children_compiled: Vec<String>,
    pub archive: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfImproveEntry {
    pub parent_commit: String,
    pub entry: String,
}

#[derive(Debug, Clone)]
pub struct DgmConfig {
    pub max_generation: u32,
    pub selfimprove_size: u32,
    pub selfimprove_workers: u32,
    pub choose_selfimproves_method: String,
    pub continue_from: Option<PathBuf>,
    pub update_archive: String,
    pub num_swe_evals: u32,
    pub post_improve_diagnose: bool,
    pub shallow_eval: bool,
    pub polyglot: bool,
    pub eval_noise: f64,
    pub no_full_eval: bool,
    pub run_baseline: Option<String>,
}

impl DgmConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        max_generation: u32,
        selfimprove_size: u32,
        selfimprove_workers: u32,
        choose_selfimproves_method: String,
        continue_from: Option<PathBuf>,
        update_archive: String,
        num_swe_evals: u32,
        post_improve_diagnose: bool,
        shallow_eval: bool,
        polyglot: bool,
        eval_noise: f64,
        no_full_eval: bool,
        run_baseline: Option<String>,
    ) -> Self {
        Self {
            max_generation,
            selfimprove_size,
            selfimprove_workers,
            choose_selfimproves_method,
            continue_from,
            update_archive,
            num_swe_evals,
            post_improve_diagnose,
            shallow_eval,
            polyglot,
            eval_noise,
            no_full_eval,
            run_baseline,
        }
    }

    pub fn validate(&self) -> DgmResult<()> {
        if self.max_generation == 0 {
            return Err(anyhow::anyhow!("max_generation must be greater than 0"));
        }
        
        if self.selfimprove_size == 0 {
            return Err(anyhow::anyhow!("selfimprove_size must be greater than 0"));
        }
        
        if self.selfimprove_workers == 0 {
            return Err(anyhow::anyhow!("selfimprove_workers must be greater than 0"));
        }

        let valid_methods = ["random", "score_prop", "score_child_prop", "best"];
        if !valid_methods.contains(&self.choose_selfimproves_method.as_str()) {
            return Err(anyhow::anyhow!(
                "Invalid choose_selfimproves_method: {}. Must be one of: {:?}",
                self.choose_selfimproves_method,
                valid_methods
            ));
        }

        let valid_archive_methods = ["keep_better", "keep_all"];
        if !valid_archive_methods.contains(&self.update_archive.as_str()) {
            return Err(anyhow::anyhow!(
                "Invalid update_archive method: {}. Must be one of: {:?}",
                self.update_archive,
                valid_archive_methods
            ));
        }

        if self.eval_noise < 0.0 || self.eval_noise > 1.0 {
            return Err(anyhow::anyhow!(
                "eval_noise must be between 0.0 and 1.0, got: {}",
                self.eval_noise
            ));
        }

        Ok(())
    }
}

impl SelfImproveEntry {
    pub fn new(parent_commit: String, entry: String) -> Self {
        Self {
            parent_commit,
            entry,
        }
    }
}
