use crate::DgmResult;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;

/// Prompt template with placeholders
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub name: String,
    pub template: String,
    pub placeholders: Vec<String>,
}

/// Prompt manager for handling LLM prompts and templates
pub struct PromptManager {
    templates: HashMap<String, PromptTemplate>,
}

impl PromptManager {
    /// Create a new prompt manager
    pub fn new() -> Self {
        let mut manager = Self {
            templates: HashMap::new(),
        };

        // Initialize built-in templates
        manager.init_builtin_templates();
        manager
    }

    /// Initialize built-in prompt templates
    fn init_builtin_templates(&mut self) {
        // Coding agent summary template
        let coding_agent_summary = PromptTemplate {
            name: "coding_agent_summary".to_string(),
            template: r#"# Coding Agent Summary

- **Main File**: `coding_agent.py`
  - Primary Class: `AgenticSystem`
  - The `forward()` function is the central entry point.
  - Prompts are located either within the `forward()` function or in the `prompts/` directory.
- **Tools**: `tools/`
  - The `tools/` directory contains various tools that LLMs can use to perform specific tasks.
  - Each tool must have a `tool_info()` function that returns a JSON object containing 'name', 'description', and 'input_schema'. The 'input_schema' should be a JSON object containing 'type', 'properties', and 'required'.
  - Each tool must have a `tool_function()` function that takes the arguments defined in input_schema, performs the tool's task, and returns a string.
  - See other tools for reference.
- **Utilities**: `utils/`
  - The `utils/` directory contains utility functions used across the codebase.

- **Additional Details**:
  - The agent is very good at automatically utilizing the right available tools at the right time. So do not have an agentic flow that explicitly forces a tool's usage.
  - Common tools, such as file editing and bash commands, are easy for the agent to recognize and use appropriately. However, more complex and niche tools may require explicit instructions in the prompt.
  - Tools should be designed to be as general as possible, ensuring they work across any GitHub repository. Avoid hardcoding repository-specific details or behaviors (e.g., paths).
  - Do not use 'while True' loops in the agent's code. This can cause the agent to get stuck and not respond.
  - Verify the implementation details of helper functions prior to usage to ensure proper integration and expected behavior.
  - Do not install additional packages or dependencies directly. Update `requirements.txt` if new dependencies are required and install them using `pip install -r requirements.txt`.

"#.to_string(),
            placeholders: vec![],
        };

        // Polyglot coding agent summary template
        let coding_agent_summary_polyglot = PromptTemplate {
            name: "coding_agent_summary_polyglot".to_string(),
            template: r#"# Coding Agent Summary

- **Main File**: `coding_agent.py`
  - Primary Class: `AgenticSystem`
  - The `forward()` function is the central entry point.
  - Prompts are located either within the `forward()` function or in the `prompts/` directory.
- **Tools**: `tools/`
  - The `tools/` directory contains various tools that LLMs can use to perform specific tasks.
  - Each tool must have a `tool_info()` function that returns a JSON object containing 'name', 'description', and 'input_schema'. The 'input_schema' should be a JSON object containing 'type', 'properties', and 'required'.
  - Each tool must have a `tool_function()` function that takes the arguments defined in input_schema, performs the tool's task, and returns a string.
  - See other tools for reference.
- **Utilities**: `utils/`
  - The `utils/` directory contains utility functions used across the codebase.

- **Additional Details**:
  - The coding agent trying to solve a programming task. A task is in one programming language, but the coding agent needs to deal with different languages including C++, Go, Java, JavaScript, Python, and Rust.
  - The agent is very good at automatically utilizing the right available tools at the right time. So do not have an agentic flow that explicitly forces a tool's usage.
  - Be detailed in the prompt about what steps (e.g. implementing tests, refining solutions, etc.) you would like the agent to execute.
  - Common tools, such as file editing and bash commands, are easy for the agent to recognize and use appropriately. However, more complex and niche tools may require explicit instructions in the prompt.
  - Tools should be designed to be as general as possible, ensuring they work across any task. Avoid hardcoding task-specific details or behaviors (e.g., paths or solutions).
  - DO NOT use 'while True' loops in the agent's code IN ANY CASE!! This can cause the agent to get stuck and not respond.
  - Verify the implementation details of helper functions prior to usage to ensure proper integration and expected behavior.
  - **DO NOT create parsing errors tools or functions, collecting raw error messages and letting the agent analyze them will be more efficient.**

### DOC: tool function schema

Carefully consider whether to add/enhance the current tool or edit the workflow in forward()

Pay special attention to making sure that "required" and "type" are always at the correct level of nesting. For example, "required" should be at the same level as "properties", not inside it.
Make sure that every property, no matter how short, has a type and description correctly nested inside it.
Other arguments than you have seen are not permitted. For example, in "edit_line_ranges" with "type": "array", arguments like "minItems" and "maxItems" are not permitted.

"#.to_string(),
            placeholders: vec![],
        };

        // Diagnostic system message template
        let diagnose_system_message = PromptTemplate {
            name: "diagnose_system_message".to_string(),
            template: r#"Here is the implementation of the coding agent.

# Coding Agent Implementation
----- Coding Agent Implementation Start -----
{code}
----- Coding Agent Implementation End -----

Your task is to identify ONE detailed plan that would improve the agent's coding ability. The improvement should not be specific to any particular GitHub issue or repository."#.to_string(),
            placeholders: vec!["code".to_string()],
        };

        // Diagnostic prompt template
        let diagnose_prompt = PromptTemplate {
            name: "diagnose_prompt".to_string(),
            template: r#"
# Agent Running Log
----- Agent Running Log Start -----
{md_log}
----- Agent Running Log End -----

# GitHub Issue
The GitHub issue that the agent is trying to solve.
----- GitHub Issue Start -----
{github_issue}
----- GitHub Issue End -----

# Predicted Patch
The agent's predicted patch to solve the issue.
----- Predicted Patch Start -----
{predicted_patch}
----- Predicted Patch End -----

# Private Test Patch
SWE-bench's official private tests to detect whether the issue is solved. This is not available to the agent during evaluation. The agent should try to implement its own tests.
----- Private Test Patch Start -----
{test_patch}
----- Private Test Patch End -----

# Issue Test Results
The test results from SWE-bench using the above official private tests.
----- Issue Test Results Start -----
{eval_log}
----- Issue Test Results End -----

Respond precisely in the following format including the JSON start and end markers:

```json
<JSON>
```

In <JSON>, provide a JSON response with the following fields:
- "log_summarization": Analyze the above logs and summarize how the agent tried to solve the GitHub issue. Note which tools and how they are used, the agent's problem-solving approach, and any issues encountered.
- "potential_improvements": Identify potential improvements to the coding agent that could enhance its coding capabilities. Focus on the agent's general coding abilities (e.g., better or new tools usable across any repository) rather than issue-specific fixes (e.g., tools only usable in one framework). All necessary dependencies and environment setup have already been handled, so do not focus on these aspects.
- "improvement_proposal": Choose ONE high-impact improvement from the identified potential improvements and describe it in detail. This should be a focused and comprehensive plan to enhance the agent's overall coding ability.
- "implementation_suggestion": Referring to the coding agent's summary and implementation, think critically about what feature or tool could be added or improved to best implement the proposed improvement. If the proposed feature can be implemented by modifying the existing tools, describe the modifications needed, instead of suggesting a new tool.
- "problem_description": Phrase the improvement proposal and implementation suggestion as a GitHub issue description. It should clearly describe the feature so that a software engineer viewing the issue and the repository can implement it.

Your response will be automatically parsed, so ensure that the string response is precisely in the correct format. Do NOT include the `<JSON>` tag in your output."#.to_string(),
            placeholders: vec![
                "md_log".to_string(),
                "github_issue".to_string(),
                "predicted_patch".to_string(),
                "test_patch".to_string(),
                "eval_log".to_string(),
            ],
        };

        // Empty patches diagnostic prompt
        let diagnose_prompt_emptypatches = PromptTemplate {
            name: "diagnose_prompt_emptypatches".to_string(),
            template: r#"There are some empty patches when attempting to solve GitHub issues. Since the coding agent is stochastic, it may not always produce a patch. Handle cases where the coding agent fails to generate a patch or generates one that only modifies the test cases without editing the primary source code. For example, the simplest solution is to ask the agent to try again.

Respond precisely in the following format including the JSON start and end markers:

```json
<JSON>
```

In <JSON>, provide a JSON response with the following fields:
- "potential_improvements": Identify potential improvements to the coding agent's system. All necessary dependencies and environment setup have already been handled, so do not focus on these aspects.
- "improvement_proposal": Choose ONE high-impact improvement from the identified potential improvements and describe it in detail. This should be a focused and comprehensive plan to enhance the agent's overall coding ability.
- "implementation_suggestion": Referring to the coding agent's summary and implementation, think critically about what feature could be added or improved to best implement the proposed improvement.
- "problem_description": Phrase the improvement proposal and implementation suggestion as a GitHub issue description. It should clearly describe the feature so that a software engineer viewing the issue and the repository can implement it.

Your response will be automatically parsed, so ensure that the string response is precisely in the correct format. Do NOT include the `<JSON>` tag in your output."#.to_string(),
            placeholders: vec![],
        };

        self.templates.insert("coding_agent_summary".to_string(), coding_agent_summary);
        self.templates.insert("coding_agent_summary_polyglot".to_string(), coding_agent_summary_polyglot);
        self.templates.insert("diagnose_system_message".to_string(), diagnose_system_message);
        self.templates.insert("diagnose_prompt".to_string(), diagnose_prompt);
        self.templates.insert("diagnose_prompt_emptypatches".to_string(), diagnose_prompt_emptypatches);
    }

    /// Get a template by name
    pub fn get_template(&self, name: &str) -> Option<&PromptTemplate> {
        self.templates.get(name)
    }

    /// Render a template with the given context
    pub fn render_template(&self, name: &str, context: &HashMap<String, String>) -> DgmResult<String> {
        let template = self.templates.get(name)
            .ok_or_else(|| anyhow::anyhow!("Template '{}' not found", name))?;

        let mut rendered = template.template.clone();

        // Replace placeholders with context values
        for placeholder in &template.placeholders {
            let placeholder_key = format!("{{{}}}", placeholder);
            if let Some(value) = context.get(placeholder) {
                rendered = rendered.replace(&placeholder_key, value);
            } else {
                return Err(anyhow::anyhow!("Missing context value for placeholder '{}'", placeholder).into());
            }
        }

        Ok(rendered)
    }

    /// Get self-improvement prompt for SWE-bench
    pub fn get_self_improvement_prompt_swe(
        &self,
        md_log: &str,
        github_issue: &str,
        predicted_patch: &str,
        test_patch: &str,
        eval_log: &str,
        code: &str,
    ) -> DgmResult<(String, String)> {
        let mut context = HashMap::new();
        context.insert("code".to_string(), code.to_string());

        let system_message = self.render_template("diagnose_system_message", &context)?;
        let coding_summary = self.get_template("coding_agent_summary")
            .ok_or_else(|| anyhow::anyhow!("coding_agent_summary template not found"))?;

        let full_system_message = format!("{}\n{}", coding_summary.template, system_message);

        let mut prompt_context = HashMap::new();
        prompt_context.insert("md_log".to_string(), md_log.to_string());
        prompt_context.insert("github_issue".to_string(), github_issue.to_string());
        prompt_context.insert("predicted_patch".to_string(), predicted_patch.to_string());
        prompt_context.insert("test_patch".to_string(), test_patch.to_string());
        prompt_context.insert("eval_log".to_string(), eval_log.to_string());

        let user_prompt = format!(
            "Here is the log for the coding agent trying to solve the GitHub issues but failed.\n{}",
            self.render_template("diagnose_prompt", &prompt_context)?
        );

        Ok((full_system_message, user_prompt))
    }

    /// Get self-improvement prompt for Polyglot
    pub fn get_self_improvement_prompt_polyglot(
        &self,
        md_log: &str,
        github_issue: &str,
        predicted_patch: &str,
        test_patch: &str,
        eval_log: &str,
        code: &str,
    ) -> DgmResult<(String, String)> {
        let mut context = HashMap::new();
        context.insert("code".to_string(), code.to_string());

        let system_message = self.render_template("diagnose_system_message", &context)?;
        let coding_summary = self.get_template("coding_agent_summary_polyglot")
            .ok_or_else(|| anyhow::anyhow!("coding_agent_summary_polyglot template not found"))?;

        let full_system_message = format!("{}\n{}", coding_summary.template, system_message);

        let mut prompt_context = HashMap::new();
        prompt_context.insert("md_log".to_string(), md_log.to_string());
        prompt_context.insert("github_issue".to_string(), github_issue.to_string());
        prompt_context.insert("predicted_patch".to_string(), predicted_patch.to_string());
        prompt_context.insert("test_patch".to_string(), test_patch.to_string());
        prompt_context.insert("eval_log".to_string(), eval_log.to_string());

        let user_prompt = format!(
            "Here is the log for the coding agent trying to solve a programming task. A task is in one programming language, but the coding agent needs to deal with different languages including C++, Go, Java, JavaScript, Python, and Rust.\n{}",
            self.render_template("diagnose_prompt", &prompt_context)?
        );

        Ok((full_system_message, user_prompt))
    }

    /// Get empty patches diagnostic prompt
    pub fn get_empty_patches_prompt(&self, code: &str, is_polyglot: bool) -> DgmResult<(String, String)> {
        let mut context = HashMap::new();
        context.insert("code".to_string(), code.to_string());

        let system_message = self.render_template("diagnose_system_message", &context)?;
        let template_name = if is_polyglot {
            "coding_agent_summary_polyglot"
        } else {
            "coding_agent_summary"
        };

        let coding_summary = self.get_template(template_name)
            .ok_or_else(|| anyhow::anyhow!("{} template not found", template_name))?;

        let full_system_message = format!("{}\n{}", coding_summary.template, system_message);
        let user_prompt = self.get_template("diagnose_prompt_emptypatches")
            .ok_or_else(|| anyhow::anyhow!("diagnose_prompt_emptypatches template not found"))?
            .template.clone();

        Ok((full_system_message, user_prompt))
    }

    /// Add a custom template
    pub fn add_template(&mut self, template: PromptTemplate) {
        self.templates.insert(template.name.clone(), template);
    }

    /// Load templates from a JSON file
    pub async fn load_templates_from_file(&mut self, path: &Path) -> DgmResult<()> {
        let content = fs::read_to_string(path).await
            .with_context(|| format!("Failed to read templates file: {:?}", path))?;

        let templates: Vec<PromptTemplate> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse templates JSON: {:?}", path))?;

        for template in templates {
            self.add_template(template);
        }

        Ok(())
    }

    /// Save templates to a JSON file
    pub async fn save_templates_to_file(&self, path: &Path) -> DgmResult<()> {
        let templates: Vec<&PromptTemplate> = self.templates.values().collect();
        let content = serde_json::to_string_pretty(&templates)
            .context("Failed to serialize templates")?;

        fs::write(path, content).await
            .with_context(|| format!("Failed to write templates file: {:?}", path))?;

        Ok(())
    }

    /// Get tool use prompt for LLMs without built-in tool calling
    pub async fn get_tooluse_prompt(&self, tools_dir: &Path) -> DgmResult<String> {
        let mut tool_contents = Vec::new();

        let mut entries = fs::read_dir(tools_dir).await
            .context("Failed to read tools directory")?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("py")
                && path.file_name().and_then(|s| s.to_str()) != Some("__init__.py") {

                let content = fs::read_to_string(&path).await
                    .with_context(|| format!("Failed to read tool file: {:?}", path))?;

                tool_contents.push(format!("```python\n{}\n```", content.trim()));
            }
        }

        let tools_available = tool_contents.join("\n\n");

        let tooluse_prompt = format!(r#"Here are the available tools:
{}

Use the available tools in this format:
```
<tool_use>
{{
    'tool_name': ...,
    'tool_input': ...
}}
</tool_use>
```"#, tools_available);

        Ok(tooluse_prompt)
    }

    /// Get problem description prompt for self-improvement
    pub fn get_problem_description_prompt(
        &self,
        implementation_suggestion: &str,
        problem_description: &str,
        is_polyglot: bool,
    ) -> DgmResult<String> {
        let template_name = if is_polyglot {
            "coding_agent_summary_polyglot"
        } else {
            "coding_agent_summary"
        };

        let coding_summary = self.get_template(template_name)
            .ok_or_else(|| anyhow::anyhow!("{} template not found", template_name))?;

        let problem_template = format!(
            "# To Implement\n\n{}\n\n{}",
            implementation_suggestion,
            problem_description
        );

        Ok(format!("{}\n{}", coding_summary.template, problem_template))
    }

    /// List all available template names
    pub fn list_templates(&self) -> Vec<String> {
        self.templates.keys().cloned().collect()
    }

    /// Check if a template exists
    pub fn has_template(&self, name: &str) -> bool {
        self.templates.contains_key(name)
    }

    /// Remove a template
    pub fn remove_template(&mut self, name: &str) -> Option<PromptTemplate> {
        self.templates.remove(name)
    }

    /// Clear all templates
    pub fn clear_templates(&mut self) {
        self.templates.clear();
    }

    /// Get template count
    pub fn template_count(&self) -> usize {
        self.templates.len()
    }
}
