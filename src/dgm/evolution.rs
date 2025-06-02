use crate::{
    dgm::{Archive, SelfImproveEntry},
    utils::{load_json_file, EvaluationResult},
    DgmResult,
};
use rand::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info};

pub struct EvolutionStrategy {
    method: String,
}

impl EvolutionStrategy {
    pub fn new(method: String) -> Self {
        Self { method }
    }

    /// Choose self-improvement entries for the current generation
    pub fn choose_selfimproves(
        &self,
        archive: &Archive,
        selfimprove_size: u32,
        output_dir: &Path,
        run_baseline: Option<&str>,
        polyglot: bool,
    ) -> DgmResult<Vec<SelfImproveEntry>> {
        let mut entries = Vec::new();

        // Get parent candidates with their performance metrics
        let candidates = self.get_candidates(archive, output_dir)?;
        
        if candidates.is_empty() {
            return Ok(entries);
        }

        // Choose parents based on method and baseline
        let parent_commits = match run_baseline {
            Some("no_darwin") => {
                // Always take the last commit
                let commits: Vec<String> = candidates.keys().cloned().collect();
                vec![commits.last().unwrap().clone(); selfimprove_size as usize]
            }
            _ => self.choose_parents(&candidates, selfimprove_size)?,
        };

        // Choose entries for each parent
        for parent_commit in parent_commits {
            if let Some(candidate) = candidates.get(&parent_commit) {
                let entry = self.choose_entry_for_parent(candidate, polyglot)?;
                if let Some(entry) = entry {
                    entries.push(SelfImproveEntry::new(parent_commit, entry));
                }
            }
        }

        info!("Selected {} self-improvement entries", entries.len());
        Ok(entries)
    }

    fn get_candidates(
        &self,
        archive: &Archive,
        output_dir: &Path,
    ) -> DgmResult<HashMap<String, CandidateInfo>> {
        let mut candidates = HashMap::new();

        for commit in archive.get_commits() {
            match self.load_candidate_info(commit, output_dir) {
                Ok(info) => {
                    candidates.insert(commit.clone(), info);
                }
                Err(e) => {
                    debug!("Commit {} not eligible for being a parent: {}", commit, e);
                }
            }
        }

        // Update children count
        for commit in archive.get_commits() {
            if commit != "initial" {
                if let Ok(metadata) = load_json_file::<EvaluationResult, _>(
                    &output_dir.join(commit).join("metadata.json")
                ) {
                    if let Some(parent) = candidates.get_mut(&metadata.parent_commit) {
                        parent.children_count += 1;
                    }
                }
            }
        }

        Ok(candidates)
    }

    fn load_candidate_info(
        &self,
        commit: &str,
        output_dir: &Path,
    ) -> DgmResult<CandidateInfo> {
        let metadata_path = output_dir.join(commit).join("metadata.json");
        let metadata: EvaluationResult = load_json_file(&metadata_path)?;

        let performance = metadata
            .overall_performance
            .ok_or_else(|| anyhow::anyhow!("No performance data"))?;

        Ok(CandidateInfo {
            accuracy_score: performance.accuracy_score,
            total_unresolved_ids: performance.unresolved_ids,
            total_emptypatch_ids: performance.empty_patch_ids,
            total_resolved_ids: performance.resolved_ids,
            children_count: 0,
        })
    }

    fn choose_parents(
        &self,
        candidates: &HashMap<String, CandidateInfo>,
        selfimprove_size: u32,
    ) -> DgmResult<Vec<String>> {
        let commits: Vec<String> = candidates.keys().cloned().collect();
        let mut rng = thread_rng();

        match self.method.as_str() {
            "score_prop" => {
                let scores: Vec<f64> = commits
                    .iter()
                    .map(|c| candidates[c].accuracy_score)
                    .map(|score| 1.0 / (1.0 + (-10.0 * (score - 0.5)).exp()))
                    .collect();

                let total_score: f64 = scores.iter().sum();
                let probabilities: Vec<f64> = scores.iter().map(|s| s / total_score).collect();

                Ok(self.weighted_sample(&commits, &probabilities, selfimprove_size, &mut rng))
            }
            "score_child_prop" => {
                let scores: Vec<f64> = commits
                    .iter()
                    .map(|c| candidates[c].accuracy_score)
                    .map(|score| 1.0 / (1.0 + (-10.0 * (score - 0.5)).exp()))
                    .collect();

                let children_weights: Vec<f64> = commits
                    .iter()
                    .map(|c| 1.0 / (1.0 + candidates[c].children_count as f64))
                    .collect();

                let combined_scores: Vec<f64> = scores
                    .iter()
                    .zip(children_weights.iter())
                    .map(|(s, c)| s * c)
                    .collect();

                let total_score: f64 = combined_scores.iter().sum();
                let probabilities: Vec<f64> = combined_scores.iter().map(|s| s / total_score).collect();

                Ok(self.weighted_sample(&commits, &probabilities, selfimprove_size, &mut rng))
            }
            "best" => {
                let mut sorted_commits = commits.clone();
                sorted_commits.sort_by(|a, b| {
                    candidates[b].accuracy_score
                        .partial_cmp(&candidates[a].accuracy_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                let selected_count = std::cmp::min(selfimprove_size as usize, sorted_commits.len());
                let mut result = sorted_commits[..selected_count].to_vec();

                // Fill remaining slots with random selection from the best ones
                while result.len() < selfimprove_size as usize {
                    result.push(sorted_commits[rng.gen_range(0..selected_count)].clone());
                }

                Ok(result)
            }
            _ => {
                // Random selection
                Ok((0..selfimprove_size)
                    .map(|_| commits[rng.gen_range(0..commits.len())].clone())
                    .collect())
            }
        }
    }

    fn weighted_sample(
        &self,
        items: &[String],
        weights: &[f64],
        count: u32,
        rng: &mut ThreadRng,
    ) -> Vec<String> {
        let mut result = Vec::new();
        
        for _ in 0..count {
            let r: f64 = rng.gen();
            let mut cumulative = 0.0;
            
            for (i, &weight) in weights.iter().enumerate() {
                cumulative += weight;
                if r <= cumulative {
                    result.push(items[i].clone());
                    break;
                }
            }
        }
        
        result
    }

    fn choose_entry_for_parent(
        &self,
        candidate: &CandidateInfo,
        polyglot: bool,
    ) -> DgmResult<Option<String>> {
        let mut rng = thread_rng();

        if polyglot {
            let entry_ids = [
                candidate.total_emptypatch_ids.clone(),
                candidate.total_unresolved_ids.clone(),
            ]
            .concat();

            if entry_ids.is_empty() {
                let all_ids = [
                    candidate.total_resolved_ids.clone(),
                    candidate.total_emptypatch_ids.clone(),
                    candidate.total_unresolved_ids.clone(),
                ]
                .concat();
                
                if all_ids.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(all_ids[rng.gen_range(0..all_ids.len())].clone()));
            }

            return Ok(Some(entry_ids[rng.gen_range(0..entry_ids.len())].clone()));
        }

        let total_ids = candidate.total_emptypatch_ids.len() +
                        candidate.total_resolved_ids.len() +
                        candidate.total_unresolved_ids.len();

        // Solve empty patches
        if candidate.total_emptypatch_ids.len() >= (total_ids as f64 * 0.1) as usize && rng.gen::<f64>() < 0.25 {
            return Ok(Some("solve_empty_patches".to_string()));
        }

        // Solve stochasticity
        if rng.gen::<f64>() < 0.25 {
            return Ok(Some("solve_stochasticity".to_string()));
        }

        // Choose a random unresolved entry
        if candidate.total_unresolved_ids.is_empty() {
            return Ok(None);
        }

        let entry = &candidate.total_unresolved_ids[rng.gen_range(0..candidate.total_unresolved_ids.len())];
        Ok(Some(entry.clone()))
    }
}

#[derive(Debug, Clone)]
struct CandidateInfo {
    accuracy_score: f64,
    total_unresolved_ids: Vec<String>,
    total_emptypatch_ids: Vec<String>,
    total_resolved_ids: Vec<String>,
    children_count: u32,
}
