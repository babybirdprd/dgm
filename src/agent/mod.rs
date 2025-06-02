use crate::llm::{create_client, LlmClient, Message};
use crate::tools::ToolRegistry;
use crate::DgmResult;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tracing::info;

pub struct AgenticSystem {
    problem_statement: String,
    git_tempdir: PathBuf,
    base_commit: String,
    chat_history_file: PathBuf,
    test_description: Option<String>,
    self_improve: bool,
    instance_id: String,
    llm_client: Box<dyn LlmClient + Send + Sync>,
    tool_registry: Arc<ToolRegistry>,
}

impl AgenticSystem {
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
            self_improve,
            instance_id,
            llm_client,
            tool_registry,
        })
    }

    pub async fn forward(&self) -> DgmResult<()> {
        info!("Starting agentic system for instance: {}", self.instance_id);

        let instruction = self.build_instruction();
        let mut message_history = Vec::new();

        let response = self
            .llm_client
            .send_message(&instruction, "", Some(message_history.clone()), 0.7)
            .await?;

        message_history = response.message_history;

        // Log the conversation
        self.log_conversation(&message_history).await?;

        info!("Agentic system completed for instance: {}", self.instance_id);
        Ok(())
    }

    fn build_instruction(&self) -> String {
        let mut instruction = format!(
            "I have uploaded a Python code repository in the directory {}. Help solve the following problem.\n\n",
            self.git_tempdir.display()
        );

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
            "Your task is to make changes to the files in the {} directory to address the <problem_description>. I have already taken care of the required dependencies.",
            self.git_tempdir.display()
        ));

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
}
