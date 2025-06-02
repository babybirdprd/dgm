use super::{Tool, ToolInfo};
use crate::DgmResult;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::process::Command;
// use tracing::debug;

pub struct EditTool;

impl EditTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for EditTool {
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "editor".to_string(),
            description: r#"Custom editing tool for viewing, creating, and editing files

* State is persistent across command calls and discussions with the user.
* If `path` is a file, `view` displays the entire file with line numbers. If `path` is a directory, `view` lists non-hidden files and directories up to 2 levels deep.
* The `create` command cannot be used if the specified `path` already exists as a file.
* If a `command` generates a long output, it will be truncated and marked with `<response clipped>`.
* The `edit` command overwrites the entire file with the provided `file_text`.
* No partial/line-range edits or partial viewing are supported."#.to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "enum": ["view", "create", "edit"],
                        "description": "The command to run: `view`, `create`, or `edit`."
                    },
                    "path": {
                        "description": "Absolute path to file or directory, e.g. `/repo/file.py` or `/repo`.",
                        "type": "string"
                    },
                    "file_text": {
                        "description": "Required parameter of `create` or `edit` command, containing the content for the entire file.",
                        "type": "string"
                    }
                },
                "required": ["command", "path"]
            }),
        }
    }

    async fn execute(&self, input: Value) -> DgmResult<String> {
        let command = input["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' parameter"))?;
        
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;
        
        let file_text = input["file_text"].as_str();

        match command {
            "view" => self.view_path(path_str).await,
            "create" => {
                let text = file_text
                    .ok_or_else(|| anyhow::anyhow!("Missing 'file_text' for create command"))?;
                self.create_file(path_str, text).await
            }
            "edit" => {
                let text = file_text
                    .ok_or_else(|| anyhow::anyhow!("Missing 'file_text' for edit command"))?;
                self.edit_file(path_str, text).await
            }
            _ => Err(anyhow::anyhow!("Unknown command: {}", command)),
        }
    }
}

impl EditTool {
    async fn view_path(&self, path_str: &str) -> DgmResult<String> {
        let path = self.validate_path(path_str, "view").await?;

        if path.is_dir() {
            self.view_directory(&path).await
        } else {
            self.view_file(&path).await
        }
    }

    async fn create_file(&self, path_str: &str, content: &str) -> DgmResult<String> {
        let path = self.validate_path(path_str, "create").await?;
        
        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await
                .map_err(|e| anyhow::anyhow!("Failed to create parent directories: {}", e))?;
        }

        fs::write(&path, content).await
            .map_err(|e| anyhow::anyhow!("Failed to create file: {}", e))?;

        Ok(format!("File created successfully at: {}", path_str))
    }

    async fn edit_file(&self, path_str: &str, content: &str) -> DgmResult<String> {
        let path = self.validate_path(path_str, "edit").await?;

        fs::write(&path, content).await
            .map_err(|e| anyhow::anyhow!("Failed to edit file: {}", e))?;

        Ok(format!("File at {} has been overwritten with new content.", path_str))
    }

    async fn validate_path(&self, path_str: &str, command: &str) -> DgmResult<PathBuf> {
        let path = PathBuf::from(path_str);

        // Check if it's an absolute path
        if !path.is_absolute() {
            return Err(anyhow::anyhow!(
                "The path {} is not an absolute path (must start with '/').",
                path_str
            ));
        }

        match command {
            "view" => {
                // Path must exist
                if !path.exists() {
                    return Err(anyhow::anyhow!("The path {} does not exist.", path_str));
                }
            }
            "create" => {
                // Path must not exist
                if path.exists() {
                    return Err(anyhow::anyhow!(
                        "Cannot create new file; {} already exists.",
                        path_str
                    ));
                }
            }
            "edit" => {
                // Path must exist and must be a file
                if !path.exists() {
                    return Err(anyhow::anyhow!("The file {} does not exist.", path_str));
                }
                if path.is_dir() {
                    return Err(anyhow::anyhow!(
                        "{} is a directory and cannot be edited as a file.",
                        path_str
                    ));
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown or unsupported command: {}", command));
            }
        }

        Ok(path)
    }

    async fn view_file(&self, path: &Path) -> DgmResult<String> {
        let content = fs::read_to_string(path).await
            .map_err(|e| anyhow::anyhow!("Failed to read file: {}", e))?;

        let formatted = self.format_output(&content, path.to_string_lossy().as_ref(), 1);
        Ok(formatted)
    }

    async fn view_directory(&self, path: &Path) -> DgmResult<String> {
        // Use find command to list files up to 2 levels deep, excluding hidden files
        let output = Command::new("find")
            .arg(path)
            .arg("-maxdepth")
            .arg("2")
            .arg("-not")
            .arg("-path")
            .arg("*/\\.*")
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list directory: {}", e))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Error listing directory: {}", error));
        }

        let listing = String::from_utf8_lossy(&output.stdout);
        Ok(format!(
            "Here's the files and directories up to 2 levels deep in {}, excluding hidden items:\n{}",
            path.display(),
            listing
        ))
    }

    fn format_output(&self, content: &str, path: &str, init_line: usize) -> String {
        let content = self.maybe_truncate(content);
        let content = content.replace('\t', "    "); // Expand tabs

        let numbered_lines: Vec<String> = content
            .lines()
            .enumerate()
            .map(|(i, line)| format!("{:6}\t{}", i + init_line, line))
            .collect();

        format!(
            "Here's the result of running `cat -n` on {}:\n{}\n",
            path,
            numbered_lines.join("\n")
        )
    }

    fn maybe_truncate(&self, content: &str) -> String {
        const MAX_LENGTH: usize = 10000;
        if content.len() > MAX_LENGTH {
            format!("{}\n<response clipped>", &content[..MAX_LENGTH])
        } else {
            content.to_string()
        }
    }
}
