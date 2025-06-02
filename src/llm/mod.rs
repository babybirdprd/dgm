use crate::DgmResult;
use async_trait::async_trait;
use backoff::{ExponentialBackoff, Error as BackoffError, future::retry};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tracing::info;

// Maximum output tokens for LLM responses
const MAX_OUTPUT_TOKENS: u32 = 4096;

// Available LLM models
pub const AVAILABLE_LLMS: &[&str] = &[
    // Anthropic models
    "claude-3-5-sonnet-20240620",
    "claude-3-5-sonnet-20241022",
    // OpenAI models
    "gpt-4o-mini-2024-07-18",
    "gpt-4o-2024-05-13",
    "gpt-4o-2024-08-06",
    "o1-preview-2024-09-12",
    "o1-mini-2024-09-12",
    "o1-2024-12-17",
    "o3-mini-2025-01-31",
    // OpenRouter models
    "llama3.1-405b",
    // Bedrock models
    "bedrock/anthropic.claude-3-sonnet-20240229-v1:0",
    "bedrock/anthropic.claude-3-5-sonnet-20240620-v1:0",
    "bedrock/anthropic.claude-3-5-sonnet-20241022-v2:0",
    "bedrock/anthropic.claude-3-haiku-20240307-v1:0",
    "bedrock/anthropic.claude-3-opus-20240229-v1:0",
    "bedrock/us.anthropic.claude-3-5-sonnet-20241022-v2:0",
    // Vertex AI models
    "vertex_ai/claude-3-opus@20240229",
    "vertex_ai/claude-3-5-sonnet@20240620",
    "vertex_ai/claude-3-5-sonnet-v2@20241022",
    "vertex_ai/claude-3-sonnet@20240229",
    "vertex_ai/claude-3-haiku@20240307",
    // DeepSeek models
    "deepseek-chat",
    "deepseek-coder",
    "deepseek-reasoner",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Value,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub message_history: Vec<Message>,
}

#[async_trait]
pub trait LlmClient {
    async fn send_message(
        &self,
        message: &str,
        system_message: &str,
        message_history: Option<Vec<Message>>,
        temperature: f32,
    ) -> DgmResult<LlmResponse>;

    async fn send_batch_messages(
        &self,
        message: &str,
        system_message: &str,
        message_history: Option<Vec<Message>>,
        temperature: f32,
        n_responses: u32,
    ) -> DgmResult<Vec<LlmResponse>>;
}

pub struct AnthropicClient {
    client: Client,
    model: String,
    api_key: String,
}

impl AnthropicClient {
    pub fn new(model: String, api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            model,
            api_key,
        }
    }

    fn format_message_history(&self, message_history: &[Message]) -> Vec<Value> {
        message_history
            .iter()
            .map(|msg| {
                json!({
                    "role": msg.role,
                    "content": [
                        {
                            "type": "text",
                            "text": msg.content.as_str().unwrap_or("")
                        }
                    ]
                })
            })
            .collect()
    }
}

#[async_trait]
impl LlmClient for AnthropicClient {
    async fn send_message(
        &self,
        message: &str,
        system_message: &str,
        message_history: Option<Vec<Message>>,
        temperature: f32,
    ) -> DgmResult<LlmResponse> {
        let msg_history = message_history.unwrap_or_default();
        let mut formatted_history = self.format_message_history(&msg_history);

        // Add the current message
        formatted_history.push(json!({
            "role": "user",
            "content": [
                {
                    "type": "text",
                    "text": message
                }
            ]
        }));

        let request_body = json!({
            "model": self.model,
            "max_tokens": MAX_OUTPUT_TOKENS,
            "temperature": temperature,
            "system": system_message,
            "messages": formatted_history
        });

        let operation = || async {
            let response = self
                .client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&request_body)
                .send()
                .await
                .map_err(|e| BackoffError::transient(anyhow::anyhow!("Request failed: {}", e)))?;

            if !response.status().is_success() {
                let error_text = response.text().await.unwrap_or_default();
                return Err(BackoffError::transient(anyhow::anyhow!("API request failed: {}", error_text)));
            }

            let response_json: Value = response.json().await
                .map_err(|e| BackoffError::transient(anyhow::anyhow!("JSON parse failed: {}", e)))?;
            Ok(response_json)
        };

        let backoff = ExponentialBackoff {
            max_elapsed_time: Some(Duration::from_secs(120)),
            ..Default::default()
        };

        let response_json = retry(backoff, operation).await?;

        let content = response_json["content"][0]["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid response format"))?
            .to_string();

        // Update message history
        let mut new_history = msg_history;
        new_history.push(Message {
            role: "user".to_string(),
            content: json!(message),
        });
        new_history.push(Message {
            role: "assistant".to_string(),
            content: json!(content.clone()),
        });

        Ok(LlmResponse {
            content,
            message_history: new_history,
        })
    }

    async fn send_batch_messages(
        &self,
        message: &str,
        system_message: &str,
        message_history: Option<Vec<Message>>,
        temperature: f32,
        n_responses: u32,
    ) -> DgmResult<Vec<LlmResponse>> {
        // For Anthropic, we need to make multiple individual requests
        let mut responses = Vec::new();

        for _ in 0..n_responses {
            let response = self
                .send_message(message, system_message, message_history.clone(), temperature)
                .await?;
            responses.push(response);
        }

        Ok(responses)
    }
}

pub struct OpenAiClient {
    client: Client,
    model: String,
    api_key: String,
    base_url: String,
}

impl OpenAiClient {
    pub fn new(model: String, api_key: String) -> Self {
        Self::new_with_base_url(model, api_key, "https://api.openai.com/v1".to_string())
    }

    pub fn new_with_base_url(model: String, api_key: String, base_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            model,
            api_key,
            base_url,
        }
    }

    fn format_message_history(&self, message_history: &[Message]) -> Vec<Value> {
        message_history
            .iter()
            .map(|msg| {
                json!({
                    "role": msg.role,
                    "content": msg.content.as_str().unwrap_or("")
                })
            })
            .collect()
    }
}

#[async_trait]
impl LlmClient for OpenAiClient {
    async fn send_message(
        &self,
        message: &str,
        system_message: &str,
        message_history: Option<Vec<Message>>,
        temperature: f32,
    ) -> DgmResult<LlmResponse> {
        let msg_history = message_history.unwrap_or_default();
        let mut formatted_history = self.format_message_history(&msg_history);

        // Add the current message
        formatted_history.push(json!({
            "role": "user",
            "content": message
        }));

        let mut request_body = json!({
            "model": self.model,
            "messages": formatted_history,
            "temperature": temperature,
            "max_tokens": MAX_OUTPUT_TOKENS,
            "n": 1,
            "stop": null,
            "seed": 0
        });

        // Handle O1 models differently (they don't support system messages)
        if self.model.starts_with("o1-") || self.model.starts_with("o3-") {
            // Prepend system message to user message for O1 models
            let combined_message = format!("{}\n{}", system_message, message);
            formatted_history.pop(); // Remove the last user message
            formatted_history.push(json!({
                "role": "user",
                "content": combined_message
            }));

            request_body = json!({
                "model": self.model,
                "messages": formatted_history,
                "temperature": 1.0, // O1 models use fixed temperature
                "n": 1,
                "seed": 0
            });
        } else {
            // Add system message for regular models
            formatted_history.insert(0, json!({
                "role": "system",
                "content": system_message
            }));
        }

        let operation = || async {
            let response = self
                .client
                .post(&format!("{}/chat/completions", self.base_url))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("content-type", "application/json")
                .json(&request_body)
                .send()
                .await
                .map_err(|e| BackoffError::transient(anyhow::anyhow!("Request failed: {}", e)))?;

            if !response.status().is_success() {
                let error_text = response.text().await.unwrap_or_default();
                return Err(BackoffError::transient(anyhow::anyhow!("API request failed: {}", error_text)));
            }

            let response_json: Value = response.json().await
                .map_err(|e| BackoffError::transient(anyhow::anyhow!("JSON parse failed: {}", e)))?;
            Ok(response_json)
        };

        let backoff = ExponentialBackoff {
            max_elapsed_time: Some(Duration::from_secs(120)),
            ..Default::default()
        };

        let response_json = retry(backoff, operation).await?;

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid response format"))?
            .to_string();

        // Update message history
        let mut new_history = msg_history;
        new_history.push(Message {
            role: "user".to_string(),
            content: json!(message),
        });
        new_history.push(Message {
            role: "assistant".to_string(),
            content: json!(content.clone()),
        });

        Ok(LlmResponse {
            content,
            message_history: new_history,
        })
    }

    async fn send_batch_messages(
        &self,
        message: &str,
        system_message: &str,
        message_history: Option<Vec<Message>>,
        temperature: f32,
        n_responses: u32,
    ) -> DgmResult<Vec<LlmResponse>> {
        // For certain OpenAI models, we can use the n parameter for batch responses
        if self.model.starts_with("gpt-4o-") && !self.model.starts_with("o1-") && !self.model.starts_with("o3-") {
            let msg_history = message_history.unwrap_or_default();
            let mut formatted_history = self.format_message_history(&msg_history);

            // Add system message and user message
            formatted_history.insert(0, json!({
                "role": "system",
                "content": system_message
            }));
            formatted_history.push(json!({
                "role": "user",
                "content": message
            }));

            let request_body = json!({
                "model": self.model,
                "messages": formatted_history,
                "temperature": temperature,
                "max_tokens": MAX_OUTPUT_TOKENS,
                "n": n_responses,
                "stop": null,
                "seed": 0
            });

            let operation = || async {
                let response = self
                    .client
                    .post(&format!("{}/chat/completions", self.base_url))
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .header("content-type", "application/json")
                    .json(&request_body)
                    .send()
                    .await
                    .map_err(|e| BackoffError::transient(anyhow::anyhow!("Request failed: {}", e)))?;

                if !response.status().is_success() {
                    let error_text = response.text().await.unwrap_or_default();
                    return Err(BackoffError::transient(anyhow::anyhow!("API request failed: {}", error_text)));
                }

                let response_json: Value = response.json().await
                    .map_err(|e| BackoffError::transient(anyhow::anyhow!("JSON parse failed: {}", e)))?;
                Ok(response_json)
            };

            let backoff = ExponentialBackoff {
                max_elapsed_time: Some(Duration::from_secs(120)),
                ..Default::default()
            };

            let response_json = retry(backoff, operation).await?;

            let choices = response_json["choices"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("Invalid response format"))?;

            let mut responses = Vec::new();
            for choice in choices {
                let content = choice["message"]["content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid choice format"))?
                    .to_string();

                let mut new_history = msg_history.clone();
                new_history.push(Message {
                    role: "user".to_string(),
                    content: json!(message),
                });
                new_history.push(Message {
                    role: "assistant".to_string(),
                    content: json!(content.clone()),
                });

                responses.push(LlmResponse {
                    content,
                    message_history: new_history,
                });
            }

            Ok(responses)
        } else {
            // For other models, make individual requests
            let mut responses = Vec::new();

            for _ in 0..n_responses {
                let response = self
                    .send_message(message, system_message, message_history.clone(), temperature)
                    .await?;
                responses.push(response);
            }

            Ok(responses)
        }
    }
}

/// Create an LLM client based on the model name using environment variables
pub fn create_client(model: &str) -> DgmResult<Box<dyn LlmClient + Send + Sync>> {
    use crate::config::DgmConfig;
    let config = DgmConfig::load()?;
    create_client_with_config(model, &config.api)
}

/// Create an LLM client based on the model name and API configuration
pub fn create_client_with_config(model: &str, api_config: &crate::config::ApiConfig) -> DgmResult<Box<dyn LlmClient + Send + Sync>> {
    info!("Creating LLM client for model: {}", model);

    if model.starts_with("claude-") {
        let api_key = api_config.anthropic_api_key.clone()
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .ok_or_else(|| anyhow::anyhow!("ANTHROPIC_API_KEY not found in config or environment"))?;
        Ok(Box::new(AnthropicClient::new(model.to_string(), api_key)))
    } else if model.starts_with("bedrock") && model.contains("claude") {
        // For now, we'll use the Anthropic client with Bedrock endpoint
        // In a full implementation, you'd want a separate Bedrock client
        let api_key = api_config.aws_access_key_id.clone()
            .or_else(|| std::env::var("AWS_ACCESS_KEY_ID").ok())
            .ok_or_else(|| anyhow::anyhow!("AWS_ACCESS_KEY_ID not found in config or environment"))?;
        let client_model = model.split('/').next_back().unwrap_or(model);
        Ok(Box::new(AnthropicClient::new(client_model.to_string(), api_key)))
    } else if model.starts_with("vertex_ai") && model.contains("claude") {
        // For now, we'll use the Anthropic client with Vertex AI endpoint
        // In a full implementation, you'd want a separate Vertex AI client
        let api_key = std::env::var("GOOGLE_APPLICATION_CREDENTIALS")
            .map_err(|_| anyhow::anyhow!("GOOGLE_APPLICATION_CREDENTIALS environment variable not set"))?;
        let client_model = model.split('/').next_back().unwrap_or(model);
        Ok(Box::new(AnthropicClient::new(client_model.to_string(), api_key)))
    } else if model.contains("gpt") || model.starts_with("o1-") || model.starts_with("o3-") {
        let api_key = api_config.openai_api_key.clone()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY not found in config or environment"))?;
        Ok(Box::new(OpenAiClient::new(model.to_string(), api_key)))
    } else if model.starts_with("deepseek-") {
        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .map_err(|_| anyhow::anyhow!("DEEPSEEK_API_KEY environment variable not set"))?;
        Ok(Box::new(OpenAiClient::new_with_base_url(
            model.to_string(),
            api_key,
            "https://api.deepseek.com".to_string(),
        )))
    } else if model == "llama3.1-405b" {
        let api_key = std::env::var("OPENROUTER_API_KEY")
            .map_err(|_| anyhow::anyhow!("OPENROUTER_API_KEY environment variable not set"))?;
        Ok(Box::new(OpenAiClient::new_with_base_url(
            model.to_string(),
            api_key,
            "https://openrouter.ai/api/v1".to_string(),
        )))
    } else {
        Err(anyhow::anyhow!("Model {} not supported", model))
    }
}

/// Extract JSON content from LLM output between ```json markers
pub fn extract_json_between_markers(llm_output: &str) -> Option<Value> {
    let mut inside_json_block = false;
    let mut json_lines = Vec::new();

    // Split the output into lines and iterate
    for line in llm_output.split('\n') {
        let stripped_line = line.trim();

        // Check for start of JSON code block
        if stripped_line.starts_with("```json") {
            inside_json_block = true;
            continue;
        }

        // Check for end of code block
        if inside_json_block && stripped_line.starts_with("```") {
            // We've reached the closing triple backticks
            break;
        }

        // If we're inside the JSON block, collect the lines
        if inside_json_block {
            json_lines.push(line);
        }
    }

    // If we never found a JSON code block, fallback to any JSON-like content
    if json_lines.is_empty() {
        // Fallback: Try a regex that finds any JSON-like object in the text
        use regex::Regex;
        let fallback_pattern = Regex::new(r"\{.*?\}").ok()?;

        for capture in fallback_pattern.find_iter(llm_output) {
            let candidate = capture.as_str().trim();
            if !candidate.is_empty() {
                if let Ok(json) = serde_json::from_str::<Value>(candidate) {
                    return Some(json);
                }
                // Attempt to clean control characters and re-try
                let candidate_clean = candidate.chars()
                    .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                    .collect::<String>();
                if let Ok(json) = serde_json::from_str::<Value>(&candidate_clean) {
                    return Some(json);
                }
            }
        }
        return None;
    }

    // Join all lines in the JSON block into a single string
    let json_string = json_lines.join("\n").trim().to_string();

    // Try to parse the collected JSON lines
    if let Ok(json) = serde_json::from_str::<Value>(&json_string) {
        return Some(json);
    }

    // Attempt to remove invalid control characters and re-parse
    let json_string_clean = json_string.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>();

    serde_json::from_str::<Value>(&json_string_clean).ok()
}
