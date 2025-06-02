use crate::llm::{create_client, LlmClient, Message};
use crate::tools::ToolRegistry;
use crate::DgmResult;
use anyhow::Context;
use regex::Regex;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tracing::{debug, info, warn};

/// Structure to hold tool use information
#[derive(Debug, Clone)]
struct ToolUse {
    tool_name: String,
    tool_input: Value,
}

pub struct AgenticSystem {
    problem_statement: String,
    git_tempdir: PathBuf,
    base_commit: String,
    chat_history_file: PathBuf,
    test_description: Option<String>,
    instance_id: String,
    llm_client: Box<dyn LlmClient + Send + Sync>,
    tool_registry: Arc<ToolRegistry>,
}

impl AgenticSystem {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        problem_statement: String,
        git_tempdir: PathBuf,
        base_commit: String,
        chat_history_file: PathBuf,
        test_description: Option<String>,
        self_improve: bool,
        instance_id: Option<String>,
        model: &str,
    ) -> DgmResult<Self> {
        let instance_id = if self_improve {
            "dgm".to_string()
        } else {
            instance_id.unwrap_or_else(|| "unknown".to_string())
        };

        let llm_client = create_client(model)?;
        let tool_registry = Arc::new(ToolRegistry::new());

        // Clear the chat history file
        if let Some(parent) = chat_history_file.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&chat_history_file, "").await?;

        Ok(Self {
            problem_statement,
            git_tempdir,
            base_commit,
            chat_history_file,
            test_description,
            instance_id,
            llm_client,
            tool_registry,
        })
    }

    pub async fn forward(&self) -> DgmResult<()> {
        info!("Starting agentic system for instance: {}", self.instance_id);

        let instruction = self.build_instruction();
        let mut message_history = Vec::new();

        // Start the conversation with tool calling support
        let mut current_message = instruction;
        let max_iterations = 50; // Prevent infinite loops
        let mut iteration = 0;

        loop {
            iteration += 1;
            if iteration > max_iterations {
                warn!("Maximum iterations reached, stopping conversation");
                break;
            }

            // Send message to LLM
            let response = self
                .llm_client
                .send_message(&current_message, "", Some(message_history.clone()), 0.7)
                .await?;

            message_history = response.message_history;

            // Check for tool use in the response
            if let Some(tool_use) = self.check_for_tool_use(&response.content).await? {
                // Execute the tool
                let tool_result = self.execute_tool(&tool_use).await?;

                // Prepare the tool result message for the next iteration
                current_message = format!(
                    "Tool Used: {}\nTool Input: {:?}\nTool Result: {}",
                    tool_use.tool_name, tool_use.tool_input, tool_result
                );

                // Log tool usage
                self.log_tool_usage(&tool_use, &tool_result).await?;
            } else {
                // No tool use detected, conversation is complete
                info!("No tool use detected, conversation complete");
                break;
            }
        }

        // Log the final conversation
        self.log_conversation(&message_history).await?;

        info!("Agentic system completed for instance: {} after {} iterations",
              self.instance_id, iteration);
        Ok(())
    }

    fn build_instruction(&self) -> String {
        let mut instruction = String::from("You are a coding agent.\n\n");

        // Add tools information
        instruction.push_str(&self.get_tools_prompt());

        instruction.push_str(&format!(
            "I have uploaded a Python code repository in the directory {}. Help solve the following problem.\n\n",
            self.git_tempdir.display()
        ));

        instruction.push_str(&format!(
            "<problem_description>\n{}\n</problem_description>\n\n",
            self.problem_statement
        ));

        if let Some(test_desc) = &self.test_description {
            instruction.push_str(&format!(
                "<test_description>\n{}\n</test_description>\n\n",
                test_desc
            ));
        }

        instruction.push_str(&format!(
            "Your task is to make changes to the files in the {} directory to address the <problem_description>. I have already taken care of the required dependencies.\n\n",
            self.git_tempdir.display()
        ));

        instruction.push_str("Use the available tools to explore the repository, understand the problem, and implement a solution. Start by examining the repository structure and understanding the codebase.");

        instruction
    }

    async fn log_conversation(&self, message_history: &[Message]) -> DgmResult<()> {
        let mut log_content = String::new();

        for message in message_history {
            log_content.push_str(&format!("## {}\n\n", message.role.to_uppercase()));

            // Extract text content from the message
            let content = if let Some(text) = message.content.as_str() {
                text.to_string()
            } else if let Some(array) = message.content.as_array() {
                array
                    .iter()
                    .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                message.content.to_string()
            };

            log_content.push_str(&content);
            log_content.push_str("\n\n---\n\n");
        }

        fs::write(&self.chat_history_file, log_content).await?;
        Ok(())
    }

    pub async fn get_current_edits(&self) -> DgmResult<String> {
        // Use git diff to get current changes
        let output = tokio::process::Command::new("git")
            .arg("diff")
            .arg(&self.base_commit)
            .current_dir(&self.git_tempdir)
            .output()
            .await?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Git diff failed: {}", error));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub async fn get_regression_tests(&self) -> DgmResult<String> {
        let instruction = format!(
            "I have uploaded a Python code repository in the directory {}.\n\n<problem_description>\n{}\n</problem_description>\n\n",
            self.git_tempdir.display(),
            self.problem_statement
        );

        let instruction = if let Some(test_desc) = &self.test_description {
            format!("{}<test_description>\n{}\n</test_description>\n\n", instruction, test_desc)
        } else {
            instruction
        };

        let instruction = format!(
            "{}Your task is to identify regression tests in the {} directory that should pass both before and after addressing the <problem_description>. I have already taken care of the required dependencies.\nAt the end, please provide a summary that includes where the regression tests are located, what they are testing, and how they can be executed.",
            instruction,
            self.git_tempdir.display()
        );

        let response = self
            .llm_client
            .send_message(&instruction, "", None, 0.7)
            .await?;

        Ok(response.content)
    }

    pub async fn run_regression_tests(&self, regression_tests_summary: &str) -> DgmResult<String> {
        let code_diff = self.get_current_edits().await?;

        let instruction = format!(
            "I have uploaded a Python code repository in the directory {}. There is an attempt to address the problem statement. Please review the changes and run the regression tests.\n\n",
            self.git_tempdir.display()
        );

        let instruction = format!(
            "{}<problem_description>\n{}\n</problem_description>\n\n<attempted_solution>\n{}\n</attempted_solution>\n\n",
            instruction, self.problem_statement, code_diff
        );

        let instruction = if let Some(test_desc) = &self.test_description {
            format!("{}<test_description>\n{}\n</test_description>\n\n", instruction, test_desc)
        } else {
            instruction
        };

        let instruction = format!(
            "{}<regression_tests_summary>\n{}\n</regression_tests_summary>\n\nYour task is to run the regression tests in the {} directory to ensure that the changes made to the code address the <problem_description>.",
            instruction, regression_tests_summary, self.git_tempdir.display()
        );

        let response = self
            .llm_client
            .send_message(&instruction, "", None, 0.7)
            .await?;

        Ok(response.content)
    }

    /// Check if the response contains tool use
    async fn check_for_tool_use(&self, response: &str) -> DgmResult<Option<ToolUse>> {
        // Look for <tool_use> tags in the response (for models without built-in tool calling)
        let pattern = r"<tool_use>(.*?)</tool_use>";
        let re = Regex::new(pattern).context("Failed to compile regex")?;

        if let Some(captures) = re.captures(response) {
            if let Some(tool_use_str) = captures.get(1) {
                let tool_use_str = tool_use_str.as_str().trim();

                // Try to parse the tool use as JSON
                if let Ok(tool_use_json) = serde_json::from_str::<Value>(tool_use_str) {
                    if let (Some(tool_name), Some(tool_input)) = (
                        tool_use_json.get("tool_name").and_then(|v| v.as_str()),
                        tool_use_json.get("tool_input")
                    ) {
                        return Ok(Some(ToolUse {
                            tool_name: tool_name.to_string(),
                            tool_input: tool_input.clone(),
                        }));
                    }
                }

                // Try to parse as Python dict-like format
                if let Ok(parsed) = self.parse_python_dict(tool_use_str) {
                    if let (Some(tool_name), Some(tool_input)) = (
                        parsed.get("tool_name").and_then(|v| v.as_str()),
                        parsed.get("tool_input")
                    ) {
                        return Ok(Some(ToolUse {
                            tool_name: tool_name.to_string(),
                            tool_input: tool_input.clone(),
                        }));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Parse Python dict-like string to JSON Value
    fn parse_python_dict(&self, dict_str: &str) -> Result<Value, serde_json::Error> {
        // Simple conversion from Python dict format to JSON
        let json_str = dict_str
            .replace("'", "\"")  // Replace single quotes with double quotes
            .replace("True", "true")
            .replace("False", "false")
            .replace("None", "null");

        serde_json::from_str(&json_str)
    }

    /// Execute a tool with the given input
    async fn execute_tool(&self, tool_use: &ToolUse) -> DgmResult<String> {
        debug!("Executing tool: {} with input: {:?}", tool_use.tool_name, tool_use.tool_input);

        let result = self.tool_registry
            .execute_tool(&tool_use.tool_name, tool_use.tool_input.clone())
            .await
            .with_context(|| format!("Failed to execute tool '{}'", tool_use.tool_name))?;

        debug!("Tool '{}' result: {}", tool_use.tool_name, result);
        Ok(result)
    }

    /// Log tool usage to the chat history file
    async fn log_tool_usage(&self, tool_use: &ToolUse, result: &str) -> DgmResult<()> {
        let log_entry = format!(
            "\n## TOOL USE\n\n**Tool:** {}\n**Input:** ```json\n{}\n```\n**Result:**\n```\n{}\n```\n\n---\n\n",
            tool_use.tool_name,
            serde_json::to_string_pretty(&tool_use.tool_input)?,
            result
        );

        // Append to chat history file
        let mut current_content = String::new();
        if self.chat_history_file.exists() {
            current_content = fs::read_to_string(&self.chat_history_file).await?;
        }

        current_content.push_str(&log_entry);
        fs::write(&self.chat_history_file, current_content).await?;

        Ok(())
    }

    /// Get available tools as a formatted string for the system prompt
    fn get_tools_prompt(&self) -> String {
        let tools = self.tool_registry.list_tools();
        let mut prompt = String::from("Here are the available tools:\n\n");

        for tool in tools {
            prompt.push_str(&format!(
                "**{}**: {}\n\nInput Schema:\n```json\n{}\n```\n\n",
                tool.name,
                tool.description,
                serde_json::to_string_pretty(&tool.input_schema).unwrap_or_default()
            ));
        }

        prompt.push_str("Use the available tools in this format:\n");
        prompt.push_str("```\n<tool_use>\n{\n    \"tool_name\": \"tool_name_here\",\n    \"tool_input\": {\n        \"parameter\": \"value\"\n    }\n}\n</tool_use>\n```\n\n");

        prompt
    }
}
