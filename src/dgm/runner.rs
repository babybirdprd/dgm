use crate::{
    config::DgmConfig as Config,
    dgm::{Archive, DgmConfig, EvolutionStrategy},
    utils::{ensure_dir_exists, generate_run_id, load_json_file},
    DgmResult,
};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

#[derive(Debug)]
pub struct DgmRunner {
    config: DgmConfig,
    api_config: Config,
    output_dir: PathBuf,
    run_id: String,
}

impl DgmRunner {
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
    ) -> DgmResult<Self> {
        let config = DgmConfig::new(
            max_generation,
            selfimprove_size,
            selfimprove_workers,
            choose_selfimproves_method,
            continue_from.clone(),
            update_archive,
            num_swe_evals,
            post_improve_diagnose,
            shallow_eval,
            polyglot,
            eval_noise,
            no_full_eval,
            run_baseline,
        );

        config.validate()?;

        let api_config = Config::load()?;
        api_config.validate()?;

        let run_id = if let Some(ref continue_from) = config.continue_from {
            continue_from
                .file_name()
                .and_then(|name| name.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(generate_run_id)
        } else {
            generate_run_id()
        };

        let output_dir = PathBuf::from("output_dgm").join(&run_id);

        Ok(Self {
            config,
            api_config,
            output_dir,
            run_id,
        })
    }

    pub async fn run(&self) -> DgmResult<()> {
        info!("Starting DGM run: {}", self.run_id);
        info!("Output directory: {}", self.output_dir.display());

        // Ensure output directory exists
        ensure_dir_exists(&self.output_dir)?;

        // Initialize the run
        let (mut archive, start_gen_num) = self.initialize_run().await?;

        // Load task lists
        let (_swe_issues_sm, _swe_issues_med) = self.load_task_lists()?;

        info!("Starting evolution from generation {}", start_gen_num);
        info!("Archive: {:?}", archive.get_commits());

        let evolution_strategy = EvolutionStrategy::new(self.config.choose_selfimproves_method.clone());

        // Run the DGM evolution loop
        for gen_num in start_gen_num..self.config.max_generation {
            info!("=== Generation {} ===", gen_num);

            // Choose self-improvement entries
            let selfimprove_entries = evolution_strategy.choose_selfimproves(
                &archive,
                self.config.selfimprove_size,
                &self.output_dir,
                self.config.run_baseline.as_deref(),
                self.config.polyglot,
            )?;

            info!("Self-improve entries for generation {}: {:?}", gen_num, selfimprove_entries);

            if selfimprove_entries.is_empty() {
                warn!("No self-improvement entries selected for generation {}", gen_num);
                continue;
            }

            // TODO: Implement self-improvement execution
            // For now, just create placeholder results
            let selfimprove_ids: Vec<String> = (0..selfimprove_entries.len())
                .map(|_| generate_run_id())
                .collect();

            let selfimprove_ids_compiled = selfimprove_ids.clone();

            // Update archive
            archive.update(
                selfimprove_ids_compiled.clone(),
                &self.config.update_archive,
                &self.output_dir,
                self.config.eval_noise,
            )?;

            // Save DGM state
            let metadata_path = self.output_dir.join("dgm_metadata.jsonl");
            archive.save_to_metadata(
                &metadata_path,
                gen_num,
                selfimprove_entries.into_iter().map(|e| (e.parent_commit, e.entry)).collect(),
                selfimprove_ids,
                selfimprove_ids_compiled,
            )?;

            info!("Completed generation {}. Archive size: {}", gen_num, archive.len());
        }

        info!("DGM run completed: {}", self.run_id);
        Ok(())
    }

    async fn initialize_run(&self) -> DgmResult<(Archive, u32)> {
        let start_gen_num = 0;

        let archive = if let Some(ref prevrun_dir) = self.config.continue_from {
            // Load previous run's archive
            let metadata_path = prevrun_dir.join("dgm_metadata.jsonl");
            Archive::load_from_metadata(&metadata_path)?
        } else {
            Archive::new()
        };

        // Copy cached initial version into experiment dir
        let initial_folder_name = if self.config.polyglot {
            "initial_polyglot"
        } else {
            "initial"
        };

        let initial_src = Path::new(initial_folder_name);
        let initial_dst = self.output_dir.join("initial");

        if self.config.continue_from.is_none() && !initial_dst.exists() {
            if initial_src.exists() {
                self.copy_directory(initial_src, &initial_dst).await?;
            } else {
                return Err(anyhow::anyhow!(
                    "Error: Need to properly configure evaluation results for the initial version."
                ));
            }
        }

        Ok((archive, start_gen_num))
    }

    fn load_task_lists(&self) -> DgmResult<(Vec<String>, Vec<String>)> {
        let (small_path, medium_path) = if self.config.polyglot {
            ("./polyglot/subsets/small.json", "./polyglot/subsets/medium.json")
        } else {
            ("./swe_bench/subsets/small.json", "./swe_bench/subsets/medium.json")
        };

        let swe_issues_sm: Vec<String> = load_json_file(small_path)?;
        let swe_issues_med: Vec<String> = load_json_file(medium_path)?;

        Ok((swe_issues_sm, swe_issues_med))
    }

    async fn copy_directory(&self, src: &Path, dst: &Path) -> DgmResult<()> {
        // Simple directory copy implementation
        tokio::fs::create_dir_all(dst).await?;

        let mut entries = tokio::fs::read_dir(src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if src_path.is_dir() {
                Box::pin(self.copy_directory(&src_path, &dst_path)).await?;
            } else {
                tokio::fs::copy(&src_path, &dst_path).await?;
            }
        }

        Ok(())
    }

    /// Get the API configuration for use in LLM client creation
    pub fn get_api_config(&self) -> &Config {
        &self.api_config
    }
}
