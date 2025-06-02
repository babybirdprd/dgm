use crate::{
    dgm::DgmMetadata,
    utils::{load_json_file, EvaluationResult},
    DgmResult,
};
use std::path::Path;
use tracing::{debug, info};

pub struct Archive {
    pub commits: Vec<String>,
}

impl Archive {
    pub fn new() -> Self {
        Self {
            commits: vec!["initial".to_string()],
        }
    }

    pub fn from_commits(commits: Vec<String>) -> Self {
        Self { commits }
    }

    pub fn add_commit(&mut self, commit: String) {
        if !self.commits.contains(&commit) {
            self.commits.push(commit);
            info!("Added commit to archive: {}", self.commits.last().unwrap());
        }
    }

    pub fn add_commits(&mut self, commits: Vec<String>) {
        for commit in commits {
            self.add_commit(commit);
        }
    }

    pub fn get_commits(&self) -> &[String] {
        &self.commits
    }

    pub fn len(&self) -> usize {
        self.commits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commits.is_empty()
    }

    pub fn contains(&self, commit: &str) -> bool {
        self.commits.contains(&commit.to_string())
    }

    /// Update archive based on the specified method
    pub fn update(
        &mut self,
        new_commits: Vec<String>,
        method: &str,
        output_dir: &Path,
        noise_leeway: f64,
    ) -> DgmResult<()> {
        match method {
            "keep_better" => {
                self.update_keep_better(new_commits, output_dir, noise_leeway)?;
            }
            "keep_all" => {
                self.add_commits(new_commits);
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown archive update method: {}", method));
            }
        }
        Ok(())
    }

    fn update_keep_better(
        &mut self,
        new_commits: Vec<String>,
        output_dir: &Path,
        noise_leeway: f64,
    ) -> DgmResult<()> {
        // Get the original score from the initial version
        let initial_metadata_path = output_dir.join("initial").join("metadata.json");
        let initial_metadata: EvaluationResult = load_json_file(&initial_metadata_path)?;
        
        let original_score = initial_metadata
            .overall_performance
            .as_ref()
            .map(|p| p.accuracy_score)
            .unwrap_or(0.0);
        
        let threshold = original_score - noise_leeway;

        for commit in new_commits {
            let metadata_path = output_dir.join(&commit).join("metadata.json");
            
            if let Ok(metadata) = load_json_file::<EvaluationResult, _>(&metadata_path) {
                if let Some(performance) = &metadata.overall_performance {
                    if performance.accuracy_score >= threshold {
                        self.add_commit(commit.clone());
                        debug!(
                            "Added commit {} to archive (score: {:.3} >= threshold: {:.3})",
                            commit, performance.accuracy_score, threshold
                        );
                    } else {
                        debug!(
                            "Rejected commit {} (score: {:.3} < threshold: {:.3})",
                            commit, performance.accuracy_score, threshold
                        );
                    }
                } else {
                    debug!("Rejected commit {} (no performance data)", commit);
                }
            } else {
                debug!("Rejected commit {} (no metadata)", commit);
            }
        }

        Ok(())
    }

    /// Load archive from DGM metadata file
    pub fn load_from_metadata(metadata_path: &Path) -> DgmResult<Self> {
        if !metadata_path.exists() {
            return Ok(Self::new());
        }

        // Read the last line of the JSONL file
        let content = std::fs::read_to_string(metadata_path)?;
        let lines: Vec<&str> = content.lines().collect();
        
        if let Some(last_line) = lines.last() {
            let metadata: DgmMetadata = serde_json::from_str(last_line)?;
            Ok(Self::from_commits(metadata.archive))
        } else {
            Ok(Self::new())
        }
    }

    /// Save archive to DGM metadata file
    pub fn save_to_metadata(
        &self,
        metadata_path: &Path,
        generation: u32,
        selfimprove_entries: Vec<(String, String)>,
        children: Vec<String>,
        children_compiled: Vec<String>,
    ) -> DgmResult<()> {
        let metadata = DgmMetadata {
            generation,
            selfimprove_entries,
            children,
            children_compiled,
            archive: self.commits.clone(),
        };

        // Append to JSONL file
        let json_line = serde_json::to_string(&metadata)?;
        let mut content = String::new();
        
        if metadata_path.exists() {
            content = std::fs::read_to_string(metadata_path)?;
        }
        
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&json_line);
        content.push('\n');

        std::fs::write(metadata_path, content)?;
        
        info!("Saved archive metadata for generation {}", generation);
        Ok(())
    }
}

impl Default for Archive {
    fn default() -> Self {
        Self::new()
    }
}
