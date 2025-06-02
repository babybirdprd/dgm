use super::{Tool, ToolInfo};
use crate::DgmResult;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::timeout;
// use tracing::debug;

pub struct BashTool {
    session: Arc<Mutex<Option<BashSession>>>,
}

impl BashTool {
    pub fn new() -> Self {
        Self {
            session: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl Tool for BashTool {
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "bash".to_string(),
            description: r#"Run commands in a bash shell

* When invoking this tool, the contents of the "command" parameter does NOT need to be XML-escaped.
* You don't have access to the internet via this tool.
* You do have access to a mirror of common linux and python packages via apt and pip.
* State is persistent across command calls and discussions with the user.
* To inspect a particular line range of a file, e.g. lines 10-25, try 'sed -n 10,25p /path/to/the/file'.
* Please avoid commands that may produce a very large amount of output.
* Please run long lived commands in the background, e.g. 'sleep 10 &' or start a server in the background."#.to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to run."
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute(&self, input: Value) -> DgmResult<String> {
        let command = input["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' parameter"))?;

        let mut session_guard = self.session.lock().await;
        
        // Initialize session if it doesn't exist
        if session_guard.is_none() {
            let mut session = BashSession::new();
            session.start().await?;
            *session_guard = Some(session);
        }

        let session = session_guard.as_mut().unwrap();
        let result = session.run(command).await;
        
        match result {
            Ok((output, error)) => {
                let filtered_error = filter_error(&error);
                let mut result = String::new();
                if !output.is_empty() {
                    result.push_str(&output);
                }
                if !filtered_error.is_empty() {
                    result.push_str("\nError:\n");
                    result.push_str(&filtered_error);
                }
                Ok(result.trim().to_string())
            }
            Err(e) => {
                // Session might be broken, reset it
                *session_guard = None;
                Err(e)
            }
        }
    }
}

struct BashSession {
    process: Option<Child>,
    timeout_duration: Duration,
    sentinel: String,
    output_delay: Duration,
    timed_out: bool,
}

impl BashSession {
    fn new() -> Self {
        Self {
            process: None,
            timeout_duration: Duration::from_secs(120),
            sentinel: "<<exit>>".to_string(),
            output_delay: Duration::from_millis(200),
            timed_out: false,
        }
    }

    async fn start(&mut self) -> DgmResult<()> {
        if self.process.is_some() {
            return Ok(());
        }

        let child = Command::new("/bin/bash")
            .arg("-i")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to start bash process: {}", e))?;

        self.process = Some(child);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(mut process) = self.process.take() {
            if process.try_wait().unwrap_or(None).is_none() {
                let _ = process.kill();
            }
        }
    }

    async fn run(&mut self, command: &str) -> DgmResult<(String, String)> {
        let process = self.process.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Bash session not started"))?;

        if self.timed_out {
            return Err(anyhow::anyhow!(
                "Timed out: bash has not returned in {} seconds and must be restarted",
                self.timeout_duration.as_secs()
            ));
        }

        // Check if process is still alive
        if let Ok(Some(_)) = process.try_wait() {
            return Err(anyhow::anyhow!("Bash process has exited"));
        }

        let stdin = process.stdin.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Failed to get stdin"))?;

        // Send command with sentinel
        let command_with_sentinel = format!("{}; echo '{}'\n", command, self.sentinel);
        stdin.write_all(command_with_sentinel.as_bytes()).await
            .map_err(|e| anyhow::anyhow!("Failed to write command: {}", e))?;
        stdin.flush().await
            .map_err(|e| anyhow::anyhow!("Failed to flush stdin: {}", e))?;

        // Read output with timeout
        let read_operation = async {
            let stdout = process.stdout.as_mut()
                .ok_or_else(|| anyhow::anyhow!("Failed to get stdout"))?;
            let stderr = process.stderr.as_mut()
                .ok_or_else(|| anyhow::anyhow!("Failed to get stderr"))?;

            let mut stdout_reader = BufReader::new(stdout);
            let mut stderr_reader = BufReader::new(stderr);
            
            let mut output = String::new();
            let mut error = String::new();
            let mut stdout_line = String::new();
            let mut stderr_line = String::new();

            loop {
                tokio::time::sleep(self.output_delay).await;

                // Try to read from stdout
                match stdout_reader.read_line(&mut stdout_line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if stdout_line.trim() == self.sentinel {
                            break;
                        }
                        output.push_str(&stdout_line);
                        stdout_line.clear();
                    }
                    Err(_) => {} // Continue on error
                }

                // Try to read from stderr
                match stderr_reader.read_line(&mut stderr_line).await {
                    Ok(0) => {} // EOF, continue
                    Ok(_) => {
                        error.push_str(&stderr_line);
                        stderr_line.clear();
                    }
                    Err(_) => {} // Continue on error
                }

                if output.contains(&self.sentinel) {
                    // Remove sentinel from output
                    if let Some(pos) = output.find(&self.sentinel) {
                        output.truncate(pos);
                    }
                    break;
                }
            }

            Ok::<(String, String), anyhow::Error>((output.trim().to_string(), error.trim().to_string()))
        };

        match timeout(self.timeout_duration, read_operation).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(e)) => {
                self.timed_out = true;
                Err(e)
            }
            Err(_) => {
                self.timed_out = true;
                Err(anyhow::anyhow!(
                    "Timed out: bash has not returned in {} seconds",
                    self.timeout_duration.as_secs()
                ))
            }
        }
    }
}

impl Drop for BashSession {
    fn drop(&mut self) {
        self.stop();
    }
}

fn filter_error(error: &str) -> String {
    let mut filtered_lines = Vec::new();
    let error_lines: Vec<&str> = error.lines().collect();
    let mut i = 0;

    while i < error_lines.len() {
        let line = error_lines[i];

        // Skip ioctl errors and related lines
        if line.contains("Inappropriate ioctl for device") {
            i += 3;
            if i < error_lines.len() && error_lines[i].contains("<<exit>>") {
                i += 1;
            }
            while i < error_lines.len() - 1 {
                filtered_lines.push(error_lines[i]);
                i += 1;
            }
            i += 1;
            continue;
        }

        filtered_lines.push(line);
        i += 1;
    }

    filtered_lines.join("\n").trim().to_string()
}
