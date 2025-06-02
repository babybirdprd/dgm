use crate::DgmResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tracing::debug;

pub mod bash;
pub mod edit;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn info(&self) -> ToolInfo;
    async fn execute(&self, input: Value) -> DgmResult<String>;
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            tools: HashMap::new(),
        };

        // Register built-in tools
        registry.register_tool(Box::new(bash::BashTool::new()));
        registry.register_tool(Box::new(edit::EditTool::new()));

        registry
    }

    pub fn register_tool(&mut self, tool: Box<dyn Tool>) {
        let name = tool.info().name.clone();
        self.tools.insert(name, tool);
    }

    pub fn get_tool_info(&self, name: &str) -> Option<ToolInfo> {
        self.tools.get(name).map(|tool| tool.info())
    }

    pub fn list_tools(&self) -> Vec<ToolInfo> {
        self.tools.values().map(|tool| tool.info()).collect()
    }

    pub async fn execute_tool(&self, name: &str, input: Value) -> DgmResult<String> {
        let tool = self.tools.get(name)
            .ok_or_else(|| anyhow::anyhow!("Tool '{}' not found", name))?;

        debug!("Executing tool '{}' with input: {:?}", name, input);
        let result = tool.execute(input).await?;
        debug!("Tool '{}' completed with result length: {}", name, result.len());

        Ok(result)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
