use crate::config::DgmConfig;
use crate::utils::docker::DockerManager;
use crate::DgmResult;
use anyhow::Context;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Semaphore;
use tracing::{error, info, warn};

/// Evaluation result for a single instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub instance_id: String,
    pub model_name_or_path: String,
    pub model_patch: String,
    pub proposed_model_patches: Vec<String>,
    pub eval_result: String,
    pub success: bool,
    pub error: Option<String>,
}

/// SWE-bench dataset entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SWEBenchEntry {
    pub instance_id: String,
    pub problem_statement: String,
    pub base_commit: String,
    pub test_patch: Option<String>,
    pub patch: Option<String>,
    pub repo: String,
    pub version: Option<String>,
    pub environment_setup_commit: Option<String>,
}

/// Polyglot benchmark entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolyglotEntry {
    pub instance_id: String,
    pub problem_statement: String,
    pub base_commit: String,
    pub test_commit: String,
    pub language: String,
    pub files: PolyglotFiles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolyglotFiles {
    pub solution: Vec<String>,
}

/// Evaluation harness for running benchmarks
pub struct EvaluationHarness {
    config: DgmConfig,
    docker_manager: Arc<DockerManager>,
    max_workers: usize,
}

impl EvaluationHarness {
    /// Create a new evaluation harness
    pub fn new(config: DgmConfig, max_workers: usize) -> DgmResult<Self> {
        let docker_manager = Arc::new(DockerManager::new()?);

        Ok(Self {
            config,
            docker_manager,
            max_workers,
        })
    }

    /// Run SWE-bench evaluation
    pub async fn run_swe_bench_evaluation(
        &self,
        entries: Vec<SWEBenchEntry>,
        model_name_or_path: &str,
        model_patch_paths: Option<Vec<PathBuf>>,
        output_dir: &Path,
    ) -> DgmResult<Vec<EvaluationResult>> {
        info!("Starting SWE-bench evaluation with {} entries", entries.len());

        // Create output directory
        fs::create_dir_all(output_dir).await
            .context("Failed to create output directory")?;

        // Create semaphore to limit concurrent workers
        let semaphore = Arc::new(Semaphore::new(self.max_workers));

        // Process entries in parallel
        let tasks: Vec<_> = entries
            .into_iter()
            .map(|entry| {
                let semaphore = semaphore.clone();
                let docker_manager = self.docker_manager.clone();
                let config = self.config.clone();
                let model_name = model_name_or_path.to_string();
                let model_patches = model_patch_paths.clone();
                let output_path = output_dir.to_path_buf();

                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();
                    Self::process_swe_bench_entry(
                        entry,
                        &output_path,
                        &model_name,
                        model_patches,
                        docker_manager,
                        config,
                    ).await
                })
            })
            .collect();

        // Wait for all tasks to complete
        let results = join_all(tasks).await;

        // Collect results
        let mut evaluation_results = Vec::new();
        for result in results {
            match result {
                Ok(eval_result) => evaluation_results.push(eval_result),
                Err(e) => {
                    error!("Task failed: {}", e);
                    // Create error result
                    evaluation_results.push(EvaluationResult {
                        instance_id: "unknown".to_string(),
                        model_name_or_path: model_name_or_path.to_string(),
                        model_patch: String::new(),
                        proposed_model_patches: Vec::new(),
                        eval_result: "error".to_string(),
                        success: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        info!("SWE-bench evaluation completed with {} results", evaluation_results.len());
        Ok(evaluation_results)
    }

    /// Process a single SWE-bench entry
    async fn process_swe_bench_entry(
        entry: SWEBenchEntry,
        output_dir: &Path,
        model_name_or_path: &str,
        model_patch_paths: Option<Vec<PathBuf>>,
        docker_manager: Arc<DockerManager>,
        config: DgmConfig,
    ) -> EvaluationResult {
        let instance_id = &entry.instance_id;
        info!("Processing SWE-bench entry: {}", instance_id);

        // Check if result already exists
        let result_file = output_dir.join(format!("{}.json", instance_id));
        if result_file.exists() {
            info!("Skipping existing entry: {}", instance_id);
            if let Ok(content) = fs::read_to_string(&result_file).await {
                if let Ok(result) = serde_json::from_str::<EvaluationResult>(&content) {
                    return result;
                }
            }
        }

        let mut result = EvaluationResult {
            instance_id: instance_id.clone(),
            model_name_or_path: model_name_or_path.to_string(),
            model_patch: String::new(),
            proposed_model_patches: Vec::new(),
            eval_result: "incomplete".to_string(),
            success: false,
            error: None,
        };

        // Create container name with timestamp
        let run_id = chrono::Utc::now().format("%Y%m%d_%H%M%S_%f").to_string();
        let container_name = format!("swe_bench_{}_{}", instance_id, run_id);

        match Self::run_swe_bench_container(
            &entry,
            &container_name,
            model_patch_paths,
            &docker_manager,
            &config,
            output_dir,
        ).await {
            Ok((model_patch, proposed_patches)) => {
                result.model_patch = model_patch;
                result.proposed_model_patches = proposed_patches;
                result.eval_result = "completed".to_string();
                result.success = true;
                info!("Successfully processed SWE-bench entry: {}", instance_id);
            }
            Err(e) => {
                result.error = Some(e.to_string());
                result.eval_result = "error".to_string();
                error!("Failed to process SWE-bench entry {}: {}", instance_id, e);
            }
        }

        // Save result to file
        if let Ok(json_content) = serde_json::to_string_pretty(&result) {
            if let Err(e) = fs::write(&result_file, json_content).await {
                warn!("Failed to write result file for {}: {}", instance_id, e);
            }
        }

        result
    }

    /// Run SWE-bench evaluation in a Docker container
    async fn run_swe_bench_container(
        entry: &SWEBenchEntry,
        container_name: &str,
        model_patch_paths: Option<Vec<PathBuf>>,
        docker_manager: &DockerManager,
        _config: &DgmConfig,
        output_dir: &Path,
    ) -> DgmResult<(String, Vec<String>)> {
        // Build or get the appropriate Docker image for this entry
        let image_name = format!("swe_bench_{}", entry.repo.replace("/", "_"));

        // Create and start container
        let container_id = docker_manager.create_container(
            &image_name,
            container_name,
            Some("/dgm"),
            Some(Self::get_environment_variables()),
        ).await?;

        docker_manager.start_container(&container_id).await?;

        // Copy necessary files to container
        Self::copy_dgm_files_to_container(docker_manager, &container_id).await?;

        // Apply model patches if provided
        if let Some(patch_paths) = model_patch_paths {
            for patch_path in patch_paths {
                Self::apply_model_patch_to_container(docker_manager, &container_id, &patch_path).await?;
            }
        }

        // Install requirements
        docker_manager.exec_command(
            &container_id,
            &["python", "-m", "pip", "install", "-r", "/dgm/requirements.txt"],
            Some(300), // 5 minute timeout
        ).await?;

        // Run the coding agent
        let agent_cmd = vec![
            "timeout", "32400", // 9 hour timeout
            "python", "/dgm/coding_agent.py",
            "--problem_statement", &entry.problem_statement,
            "--git_dir", "/testbed/",
            "--base_commit", &entry.base_commit,
            "--outdir", "/dgm/",
            "--instance_id", &entry.instance_id,
        ];

        let (_output, exit_code) = docker_manager.exec_command(
            &container_id,
            &agent_cmd.iter().map(|s| *s).collect::<Vec<_>>(),
            Some(32400), // 9 hour timeout
        ).await?;

        info!("Agent execution completed with exit code: {}", exit_code);

        // Get model patch
        let (model_patch, _) = docker_manager.exec_command(
            &container_id,
            &["cat", "/dgm/model_patch.diff"],
            Some(30),
        ).await.unwrap_or_else(|_| (String::new(), 1));

        // Get proposed model patches
        let (patch_files_output, _) = docker_manager.exec_command(
            &container_id,
            &["find", "/dgm/", "-name", "model_patch_*.diff"],
            Some(30),
        ).await.unwrap_or_else(|_| (String::new(), 1));

        let mut proposed_patches = Vec::new();
        for patch_file in patch_files_output.lines() {
            if !patch_file.trim().is_empty() {
                let (patch_content, _) = docker_manager.exec_command(
                    &container_id,
                    &["cat", patch_file.trim()],
                    Some(30),
                ).await.unwrap_or_else(|_| (String::new(), 1));
                proposed_patches.push(patch_content);
            }
        }

        // Copy output files back to host
        let chat_history_container = format!("/dgm/{}.md", entry.instance_id);
        let chat_history_host = output_dir.join(format!("{}.md", entry.instance_id));

        if let Err(e) = docker_manager.copy_from_container(
            &container_id,
            Path::new(&chat_history_container),
            &chat_history_host,
        ).await {
            warn!("Failed to copy chat history: {}", e);
        }

        // Cleanup container
        if let Err(e) = docker_manager.stop_container(&container_id, 10).await {
            warn!("Failed to stop container: {}", e);
        }
        if let Err(e) = docker_manager.remove_container(&container_id, true).await {
            warn!("Failed to remove container: {}", e);
        }

        Ok((model_patch, proposed_patches))
    }

    /// Get environment variables for container execution
    fn get_environment_variables() -> Vec<String> {
        let mut env_vars = Vec::new();

        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            env_vars.push(format!("ANTHROPIC_API_KEY={}", key));
        }
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            env_vars.push(format!("OPENAI_API_KEY={}", key));
        }
        if let Ok(region) = std::env::var("AWS_REGION") {
            env_vars.push(format!("AWS_REGION={}", region));
        }
        if let Ok(region) = std::env::var("AWS_REGION_NAME") {
            env_vars.push(format!("AWS_REGION_NAME={}", region));
        }
        if let Ok(key_id) = std::env::var("AWS_ACCESS_KEY_ID") {
            env_vars.push(format!("AWS_ACCESS_KEY_ID={}", key_id));
        }
        if let Ok(secret) = std::env::var("AWS_SECRET_ACCESS_KEY") {
            env_vars.push(format!("AWS_SECRET_ACCESS_KEY={}", secret));
        }

        env_vars
    }

    /// Copy DGM files to container
    async fn copy_dgm_files_to_container(
        docker_manager: &DockerManager,
        container_id: &str,
    ) -> DgmResult<()> {
        let files_to_copy = vec![
            ("coding_agent.py", "/dgm/coding_agent.py"),
            ("requirements.txt", "/dgm/requirements.txt"),
            ("pytest.ini", "/dgm/pytest.ini"),
            ("llm.py", "/dgm/llm.py"),
            ("llm_withtools.py", "/dgm/llm_withtools.py"),
        ];

        let dirs_to_copy = vec![
            ("tools/", "/dgm/tools/"),
            ("utils/", "/dgm/utils/"),
            ("tests/", "/dgm/tests/"),
            ("prompts/", "/dgm/prompts/"),
        ];

        // Copy individual files
        for (src, dst) in files_to_copy {
            if Path::new(src).exists() {
                docker_manager.copy_to_container(
                    container_id,
                    Path::new(src),
                    Path::new(dst),
                ).await.with_context(|| format!("Failed to copy {} to container", src))?;
            }
        }

        // Copy directories
        for (src, dst) in dirs_to_copy {
            if Path::new(src).exists() {
                docker_manager.copy_to_container(
                    container_id,
                    Path::new(src),
                    Path::new(dst),
                ).await.with_context(|| format!("Failed to copy {} to container", src))?;
            }
        }

        Ok(())
    }

    /// Apply model patch to container
    async fn apply_model_patch_to_container(
        docker_manager: &DockerManager,
        container_id: &str,
        patch_path: &Path,
    ) -> DgmResult<()> {
        // Copy patch file to container
        docker_manager.copy_to_container(
            container_id,
            patch_path,
            Path::new("/dgm/parent_patch.txt"),
        ).await?;

        // Apply patch
        docker_manager.exec_command(
            container_id,
            &["patch", "-p1", "-i", "/dgm/parent_patch.txt"],
            Some(60),
        ).await?;

        // Remove patch file
        docker_manager.exec_command(
            container_id,
            &["rm", "/dgm/parent_patch.txt"],
            Some(30),
        ).await?;

        Ok(())
    }

    /// Run Polyglot evaluation
    pub async fn run_polyglot_evaluation(
        &self,
        entries: Vec<PolyglotEntry>,
        model_name_or_path: &str,
        model_patch_paths: Option<Vec<PathBuf>>,
        output_dir: &Path,
    ) -> DgmResult<Vec<EvaluationResult>> {
        info!("Starting Polyglot evaluation with {} entries", entries.len());

        // Create output directory
        fs::create_dir_all(output_dir).await
            .context("Failed to create output directory")?;

        // Create semaphore to limit concurrent workers
        let semaphore = Arc::new(Semaphore::new(self.max_workers));

        // Process entries in parallel
        let tasks: Vec<_> = entries
            .into_iter()
            .map(|entry| {
                let semaphore = semaphore.clone();
                let docker_manager = self.docker_manager.clone();
                let config = self.config.clone();
                let model_name = model_name_or_path.to_string();
                let model_patches = model_patch_paths.clone();
                let output_path = output_dir.to_path_buf();

                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();
                    Self::process_polyglot_entry(
                        entry,
                        &output_path,
                        &model_name,
                        model_patches,
                        docker_manager,
                        config,
                    ).await
                })
            })
            .collect();

        // Wait for all tasks to complete
        let results = join_all(tasks).await;

        // Collect results
        let mut evaluation_results = Vec::new();
        for result in results {
            match result {
                Ok(eval_result) => evaluation_results.push(eval_result),
                Err(e) => {
                    error!("Task failed: {}", e);
                    evaluation_results.push(EvaluationResult {
                        instance_id: "unknown".to_string(),
                        model_name_or_path: model_name_or_path.to_string(),
                        model_patch: String::new(),
                        proposed_model_patches: Vec::new(),
                        eval_result: "error".to_string(),
                        success: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        info!("Polyglot evaluation completed with {} results", evaluation_results.len());
        Ok(evaluation_results)
    }

    /// Process a single Polyglot entry
    async fn process_polyglot_entry(
        entry: PolyglotEntry,
        output_dir: &Path,
        model_name_or_path: &str,
        model_patch_paths: Option<Vec<PathBuf>>,
        docker_manager: Arc<DockerManager>,
        config: DgmConfig,
    ) -> EvaluationResult {
        let instance_id = &entry.instance_id;
        info!("Processing Polyglot entry: {}", instance_id);

        // Check if result already exists
        let result_file = output_dir.join(format!("{}.json", instance_id));
        if result_file.exists() {
            info!("Skipping existing entry: {}", instance_id);
            if let Ok(content) = fs::read_to_string(&result_file).await {
                if let Ok(result) = serde_json::from_str::<EvaluationResult>(&content) {
                    return result;
                }
            }
        }

        let mut result = EvaluationResult {
            instance_id: instance_id.clone(),
            model_name_or_path: model_name_or_path.to_string(),
            model_patch: String::new(),
            proposed_model_patches: Vec::new(),
            eval_result: "incomplete".to_string(),
            success: false,
            error: None,
        };

        // Create container name with timestamp
        let run_id = chrono::Utc::now().format("%Y%m%d_%H%M%S_%f").to_string();
        let container_name = format!("polyglot_{}_{}", instance_id, run_id);

        match Self::run_polyglot_container(
            &entry,
            &container_name,
            model_patch_paths,
            &docker_manager,
            &config,
            output_dir,
        ).await {
            Ok((model_patch, eval_result)) => {
                result.model_patch = model_patch;
                result.eval_result = eval_result;
                result.success = true;
                info!("Successfully processed Polyglot entry: {}", instance_id);
            }
            Err(e) => {
                result.error = Some(e.to_string());
                result.eval_result = "error".to_string();
                error!("Failed to process Polyglot entry {}: {}", instance_id, e);
            }
        }

        // Save result to file
        if let Ok(json_content) = serde_json::to_string_pretty(&result) {
            if let Err(e) = fs::write(&result_file, json_content).await {
                warn!("Failed to write result file for {}: {}", instance_id, e);
            }
        }

        result
    }

    /// Run Polyglot evaluation in a Docker container
    async fn run_polyglot_container(
        entry: &PolyglotEntry,
        container_name: &str,
        model_patch_paths: Option<Vec<PathBuf>>,
        docker_manager: &DockerManager,
        _config: &DgmConfig,
        output_dir: &Path,
    ) -> DgmResult<(String, String)> {
        // Build or get the appropriate Docker image for this entry
        let image_name = format!("polyglot_{}", entry.language);

        // Create and start container
        let container_id = docker_manager.create_container(
            &image_name,
            container_name,
            Some("/testbed"),
            Some(Self::get_environment_variables()),
        ).await?;

        docker_manager.start_container(&container_id).await?;

        // Copy necessary files to container
        Self::copy_dgm_files_to_container(docker_manager, &container_id).await?;

        // Apply model patches if provided
        if let Some(patch_paths) = model_patch_paths {
            for patch_path in patch_paths {
                Self::apply_model_patch_to_container(docker_manager, &container_id, &patch_path).await?;
            }
        }

        // Install requirements
        docker_manager.exec_command(
            &container_id,
            &["python", "-m", "pip", "install", "-r", "/dgm/requirements.txt"],
            Some(300), // 5 minute timeout
        ).await?;

        // Run the coding agent
        let agent_cmd = vec![
            "timeout", "600", // 10 minute timeout
            "python", "/dgm/coding_agent.py",
            "--problem_statement", &entry.problem_statement,
            "--git_dir", "/testbed/",
            "--base_commit", &entry.base_commit,
            "--outdir", "/dgm/",
            "--language", &entry.language,
        ];

        let (_output, exit_code) = docker_manager.exec_command(
            &container_id,
            &agent_cmd.iter().map(|s| *s).collect::<Vec<_>>(),
            Some(600), // 10 minute timeout
        ).await?;

        info!("Agent execution completed with exit code: {}", exit_code);

        // Get model patch
        let (model_patch, _) = docker_manager.exec_command(
            &container_id,
            &["cat", "/dgm/model_patch.diff"],
            Some(30),
        ).await.unwrap_or_else(|_| (String::new(), 1));

        // If no patch was generated, return early
        if model_patch.trim().is_empty() {
            // Cleanup container
            Self::cleanup_container(docker_manager, &container_id).await;
            return Ok((model_patch, "empty_patch".to_string()));
        }

        // Stash solution files and reset to test commit
        let stash_files = entry.files.solution.join(" ");
        docker_manager.exec_command(
            &container_id,
            &["git", "-C", "/testbed", "stash", "push", &stash_files],
            Some(60),
        ).await?;

        docker_manager.exec_command(
            &container_id,
            &["git", "-C", "/testbed", "reset", "--hard", &entry.test_commit],
            Some(60),
        ).await?;

        docker_manager.exec_command(
            &container_id,
            &["git", "-C", "/testbed", "clean", "-fd"],
            Some(60),
        ).await?;

        docker_manager.exec_command(
            &container_id,
            &["git", "-C", "/testbed", "stash", "pop"],
            Some(60),
        ).await?;

        // Run evaluation based on language
        let eval_result = Self::run_language_evaluation(docker_manager, &container_id, &entry.language).await?;

        // Copy output files back to host
        let chat_history_container = format!("/dgm/{}.md", entry.instance_id);
        let chat_history_host = output_dir.join(format!("{}.md", entry.instance_id));

        if let Err(e) = docker_manager.copy_from_container(
            &container_id,
            Path::new(&chat_history_container),
            &chat_history_host,
        ).await {
            warn!("Failed to copy chat history: {}", e);
        }

        // Cleanup container
        Self::cleanup_container(docker_manager, &container_id).await;

        Ok((model_patch, eval_result))
    }

    /// Run language-specific evaluation
    async fn run_language_evaluation(
        docker_manager: &DockerManager,
        container_id: &str,
        language: &str,
    ) -> DgmResult<String> {
        let test_command = match language {
            "python" => vec!["python", "-m", "pytest", "-xvs"],
            "javascript" => vec!["npm", "test"],
            "java" => vec!["mvn", "test"],
            "cpp" => vec!["make", "test"],
            "rust" => vec!["cargo", "test"],
            _ => vec!["echo", "No test command defined for language"],
        };

        let (_output, exit_code) = docker_manager.exec_command(
            container_id,
            &test_command,
            Some(120), // 2 minute timeout
        ).await.unwrap_or_else(|_| (String::new(), 1));

        if exit_code == 0 {
            Ok("resolved".to_string())
        } else {
            Ok("unresolved".to_string())
        }
    }

    /// Cleanup container
    async fn cleanup_container(docker_manager: &DockerManager, container_id: &str) {
        if let Err(e) = docker_manager.stop_container(container_id, 10).await {
            warn!("Failed to stop container: {}", e);
        }
        if let Err(e) = docker_manager.remove_container(container_id, true).await {
            warn!("Failed to remove container: {}", e);
        }
    }

    /// Load dataset from JSON file
    pub async fn load_dataset<T>(path: &Path) -> DgmResult<Vec<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        let content = fs::read_to_string(path).await
            .with_context(|| format!("Failed to read dataset file: {:?}", path))?;

        let dataset: Vec<T> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse dataset JSON: {:?}", path))?;

        Ok(dataset)
    }

    /// Generate evaluation report
    pub fn generate_report(results: &[EvaluationResult]) -> HashMap<String, serde_json::Value> {
        let total_instances = results.len();
        let completed_instances = results.iter().filter(|r| r.success).count();
        let resolved_instances = results.iter().filter(|r| r.eval_result == "resolved").count();
        let unresolved_instances = results.iter().filter(|r| r.eval_result == "unresolved").count();
        let empty_patch_instances = results.iter().filter(|r| r.eval_result == "empty_patch").count();
        let error_instances = results.iter().filter(|r| r.eval_result == "error").count();

        let mut report = HashMap::new();
        report.insert("total_instances".to_string(), serde_json::Value::Number(total_instances.into()));
        report.insert("completed_instances".to_string(), serde_json::Value::Number(completed_instances.into()));
        report.insert("resolved_instances".to_string(), serde_json::Value::Number(resolved_instances.into()));
        report.insert("unresolved_instances".to_string(), serde_json::Value::Number(unresolved_instances.into()));
        report.insert("empty_patch_instances".to_string(), serde_json::Value::Number(empty_patch_instances.into()));
        report.insert("error_instances".to_string(), serde_json::Value::Number(error_instances.into()));

        // Add ID lists
        let completed_ids: Vec<_> = results.iter().filter(|r| r.success).map(|r| &r.instance_id).collect();
        let resolved_ids: Vec<_> = results.iter().filter(|r| r.eval_result == "resolved").map(|r| &r.instance_id).collect();
        let unresolved_ids: Vec<_> = results.iter().filter(|r| r.eval_result == "unresolved").map(|r| &r.instance_id).collect();
        let empty_patch_ids: Vec<_> = results.iter().filter(|r| r.eval_result == "empty_patch").map(|r| &r.instance_id).collect();
        let error_ids: Vec<_> = results.iter().filter(|r| r.eval_result == "error").map(|r| &r.instance_id).collect();

        report.insert("completed_ids".to_string(), serde_json::to_value(completed_ids).unwrap());
        report.insert("resolved_ids".to_string(), serde_json::to_value(resolved_ids).unwrap());
        report.insert("unresolved_ids".to_string(), serde_json::to_value(unresolved_ids).unwrap());
        report.insert("empty_patch_ids".to_string(), serde_json::to_value(empty_patch_ids).unwrap());
        report.insert("error_ids".to_string(), serde_json::to_value(error_ids).unwrap());

        report
    }
}
