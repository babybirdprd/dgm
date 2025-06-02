use crate::{DgmResult, Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub accuracy_score: f64,
    pub total_resolved_instances: u32,
    pub total_unresolved_instances: u32,
    pub total_empty_patch_instances: u32,
    pub total_submitted_instances: u32,
    pub resolved_ids: Vec<String>,
    pub unresolved_ids: Vec<String>,
    pub empty_patch_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub run_id: String,
    pub parent_commit: String,
    pub entry: Option<String>,
    pub problem_statement: Option<String>,
    pub model_patch_exists: bool,
    pub model_patch_notempty: bool,
    pub overall_performance: Option<PerformanceMetrics>,
    pub swe_dnames: Vec<String>,
    pub is_compiled: Option<bool>,
    pub improvement_diagnosis: Option<serde_json::Value>,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            accuracy_score: 0.0,
            total_resolved_instances: 0,
            total_unresolved_instances: 0,
            total_empty_patch_instances: 0,
            total_submitted_instances: 0,
            resolved_ids: Vec::new(),
            unresolved_ids: Vec::new(),
            empty_patch_ids: Vec::new(),
        }
    }
}

impl PerformanceMetrics {
    pub fn calculate_accuracy(&mut self) {
        if self.total_submitted_instances > 0 {
            self.accuracy_score = self.total_resolved_instances as f64 / self.total_submitted_instances as f64;
        } else {
            self.accuracy_score = 0.0;
        }
    }

    pub fn update_totals(&mut self) {
        self.total_resolved_instances = self.resolved_ids.len() as u32;
        self.total_unresolved_instances = self.unresolved_ids.len() as u32;
        self.total_empty_patch_instances = self.empty_patch_ids.len() as u32;
        self.total_submitted_instances = self.total_resolved_instances + 
                                       self.total_unresolved_instances + 
                                       self.total_empty_patch_instances;
        self.calculate_accuracy();
    }
}

/// Get model patch paths for a given commit
pub fn get_model_patch_paths(
    _root_dir: &Path,
    output_dir: &Path,
    commit: &str,
) -> DgmResult<Vec<String>> {
    let mut patch_files = Vec::new();

    if commit == "initial" {
        // No patches for initial commit
        return Ok(patch_files);
    }

    // Look for model_patch.diff in the commit's output directory
    let patch_path = output_dir.join(commit).join("model_patch.diff");
    if patch_path.exists() {
        patch_files.push(patch_path.to_string_lossy().to_string());
    }

    Ok(patch_files)
}

/// Check if a self-improvement attempt compiled successfully
pub fn is_compiled_self_improve(
    metadata: &EvaluationResult,
    num_swe_issues: Option<&[usize]>,
) -> bool {
    // Check if model patch exists and is not empty
    if !metadata.model_patch_exists || !metadata.model_patch_notempty {
        return false;
    }

    // Check if we have performance metrics
    let performance = match &metadata.overall_performance {
        Some(p) => p,
        None => return false,
    };

    // Check if we have minimum number of submitted instances
    if let Some(issues) = num_swe_issues {
        let min_required = issues.iter().min().unwrap_or(&0);
        if performance.total_submitted_instances < (*min_required as u32) {
            return false;
        }
    }

    // Check if accuracy is reasonable (not all empty patches)
    performance.accuracy_score > 0.0 || performance.total_resolved_instances > 0
}
