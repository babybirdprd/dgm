use crate::DgmResult;
use anyhow::{anyhow, Context};
use git2::{DiffOptions, Oid, Repository, ResetType, StatusOptions};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{info, warn};

/// Git repository manager for version control operations
pub struct GitManager {
    repo: Repository,
    repo_path: PathBuf,
}

impl GitManager {
    /// Create a new Git manager for the repository at the given path
    pub fn new(path: &Path) -> DgmResult<Self> {
        let repo = Repository::open(path)
            .with_context(|| format!("Failed to open git repository at {:?}", path))?;

        let repo_path = path.to_path_buf();

        Ok(Self { repo, repo_path })
    }

    /// Get the current commit hash
    pub fn get_current_commit_hash(&self) -> DgmResult<String> {
        let head = self.repo.head().context("Failed to get HEAD reference")?;
        let commit = head.peel_to_commit().context("Failed to peel HEAD to commit")?;
        Ok(commit.id().to_string())
    }

    /// Get diff between current state and a specific commit
    pub fn diff_versus_commit(&self, commit_hash: &str) -> DgmResult<String> {
        let commit_oid = Oid::from_str(commit_hash)
            .with_context(|| format!("Invalid commit hash: {}", commit_hash))?;

        let commit = self.repo.find_commit(commit_oid)
            .with_context(|| format!("Failed to find commit: {}", commit_hash))?;

        let commit_tree = commit.tree().context("Failed to get commit tree")?;

        // Get diff between commit and working directory
        let mut diff_opts = DiffOptions::new();
        diff_opts.include_untracked(true);
        diff_opts.recurse_untracked_dirs(true);

        let diff = self.repo.diff_tree_to_workdir_with_index(Some(&commit_tree), Some(&mut diff_opts))
            .context("Failed to create diff")?;

        // Convert diff to string
        let mut diff_output = String::new();
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            match line.origin() {
                '+' | '-' | ' ' => diff_output.push(line.origin()),
                _ => {}
            }
            diff_output.push_str(std::str::from_utf8(line.content()).unwrap_or(""));
            true
        }).context("Failed to format diff")?;

        Ok(diff_output)
    }

    /// Apply a patch to the repository
    pub fn apply_patch(&self, patch_str: &str) -> DgmResult<()> {
        info!("Applying patch to repository");

        // Use git command line for patch application as it's more reliable
        let output = Command::new("git")
            .args(&["-C", &self.repo_path.to_string_lossy(), "apply", "--reject", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn git apply command")?;

        let mut child = output;
        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write;
            stdin.write_all(patch_str.as_bytes())
                .context("Failed to write patch to git apply stdin")?;
        }

        let output = child.wait_with_output()
            .context("Failed to wait for git apply command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            warn!("Patch did not fully apply. stdout: {}, stderr: {}", stdout, stderr);
            return Err(anyhow!("Failed to apply patch: {}", stderr).into());
        }

        info!("Patch applied successfully");
        Ok(())
    }

    /// Reset repository to a specific commit
    pub fn reset_to_commit(&self, commit_hash: &str) -> DgmResult<()> {
        info!("Resetting repository to commit: {}", commit_hash);

        let commit_oid = Oid::from_str(commit_hash)
            .with_context(|| format!("Invalid commit hash: {}", commit_hash))?;

        let commit = self.repo.find_commit(commit_oid)
            .with_context(|| format!("Failed to find commit: {}", commit_hash))?;

        // Hard reset to the commit
        self.repo.reset(commit.as_object(), ResetType::Hard, None)
            .context("Failed to reset repository")?;

        // Clean untracked files and directories
        let output = Command::new("git")
            .args(&["-C", &self.repo_path.to_string_lossy(), "clean", "-fd"])
            .output()
            .context("Failed to run git clean command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to clean untracked files: {}", stderr);
        }

        info!("Repository reset to commit: {}", commit_hash);
        Ok(())
    }

    /// Filter patch to only include changes for specific files
    pub fn filter_patch_by_files(&self, patch_str: &str, target_files: &[&str]) -> String {
        let lines: Vec<&str> = patch_str.lines().collect();
        let mut filtered_lines = Vec::new();
        let mut include_block = false;

        for line in lines {
            // Check if this is a new diff block header
            if line.starts_with("diff --git") {
                include_block = target_files.iter().any(|target| {
                    line.contains(&format!("a/{}", target)) && line.contains(&format!("b/{}", target))
                });
            }

            if include_block {
                filtered_lines.push(line);
            }
        }

        filtered_lines.join("\n")
    }

    /// Remove patch blocks for files containing a keyword
    pub fn remove_patch_by_files(&self, patch_str: &str, keyword: &str) -> String {
        let lines: Vec<&str> = patch_str.lines().collect();
        let mut filtered_lines = Vec::new();
        let mut include_block = true;

        for line in lines {
            // Check if this is a new diff block header
            if line.starts_with("diff --git") {
                include_block = !line.to_lowercase().contains(&keyword.to_lowercase());
            }

            if include_block {
                filtered_lines.push(line);
            }
        }

        filtered_lines.join("\n")
    }

    /// Get repository status (modified, untracked files)
    pub fn get_status(&self) -> DgmResult<Vec<(String, git2::Status)>> {
        let mut status_opts = StatusOptions::new();
        status_opts.include_untracked(true);
        status_opts.include_ignored(false);

        let statuses = self.repo.statuses(Some(&mut status_opts))
            .context("Failed to get repository status")?;

        let mut result = Vec::new();
        for entry in statuses.iter() {
            if let Some(path) = entry.path() {
                result.push((path.to_string(), entry.status()));
            }
        }

        Ok(result)
    }

    /// Create a commit with the current changes
    pub fn create_commit(&self, message: &str, author_name: &str, author_email: &str) -> DgmResult<String> {
        let signature = git2::Signature::now(author_name, author_email)
            .context("Failed to create git signature")?;

        // Get the current index
        let mut index = self.repo.index().context("Failed to get repository index")?;

        // Add all changes to the index
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .context("Failed to add changes to index")?;

        index.write().context("Failed to write index")?;

        // Create tree from index
        let tree_id = index.write_tree().context("Failed to write tree")?;
        let tree = self.repo.find_tree(tree_id).context("Failed to find tree")?;

        // Get parent commit
        let parent_commit = match self.repo.head() {
            Ok(head) => Some(head.peel_to_commit().context("Failed to peel HEAD to commit")?),
            Err(_) => None, // Initial commit
        };

        let parents: Vec<&git2::Commit> = match &parent_commit {
            Some(commit) => vec![commit],
            None => vec![],
        };

        // Create the commit
        let commit_id = self.repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        ).context("Failed to create commit")?;

        info!("Created commit: {}", commit_id);
        Ok(commit_id.to_string())
    }

    /// Check if the repository has uncommitted changes
    pub fn has_changes(&self) -> DgmResult<bool> {
        let statuses = self.get_status()?;
        Ok(!statuses.is_empty())
    }
}
