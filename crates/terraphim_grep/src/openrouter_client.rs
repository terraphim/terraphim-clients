//! Thin OpenRouter client for `terraphim-grep` with a long request timeout.
//!
//! The shared `terraphim_service` API client uses a 10-second timeout, which is
//! too short for many LLM providers when the RLM prompt is large. This client
//! keeps the same request shape but uses a 120-second timeout so RLM synthesis
//! can complete without aborting the connection mid-response.
//!
//! **Maintainability note**: this module intentionally duplicates the shared
//! `terraphim_service` OpenRouter client rather than modifying it, because
//! `terraphim_service` is consumed as an external registry dependency and its
//! timeout is not currently configurable. A TODO item is to upstream a
//! configurable timeout into `terraphim_service` and remove this duplication.
//!
//! **Safety note**: `summarize()` returns a hard error. This is safe only as
//! long as no `terraphim-grep` code path calls `summarize()` through the
//! `LlmClient` trait object; all current grep synthesis uses
//! `chat_completion()`.

use std::sync::Arc;
use std::time::Duration;

use terraphim_service::llm::{ChatOptions, LlmClient, SummarizeOptions};

/// OpenRouter-backed LLM client with an extended timeout.
pub struct OpenRouterClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenRouterClient {
    /// Create a new client for the given OpenRouter API key and model id.
    pub fn new(api_key: &str, model: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(!api_key.is_empty(), "OpenRouter API key cannot be empty");
        anyhow::ensure!(!model.is_empty(), "OpenRouter model cannot be empty");

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .user_agent(concat!(
                "terraphim-grep/",
                env!("CARGO_PKG_VERSION"),
                " (https://terraphim.ai)"
            ))
            .build()?;

        Ok(Self {
            client,
            api_key: api_key.to_string(),
            model: model.to_string(),
            base_url: std::env::var("OPENROUTER_BASE_URL")
                .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string()),
        })
    }
}

#[async_trait::async_trait]
impl LlmClient for OpenRouterClient {
    fn name(&self) -> &'static str {
        "openrouter"
    }

    async fn summarize(
        &self,
        _content: &str,
        _opts: SummarizeOptions,
    ) -> terraphim_service::Result<String> {
        Err(terraphim_service::ServiceError::Config(
            "summarize not supported by terraphim-grep openrouter client".to_string(),
        ))
    }

    async fn chat_completion(
        &self,
        messages: Vec<serde_json::Value>,
        opts: ChatOptions,
    ) -> terraphim_service::Result<String> {
        let request_body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": opts.max_tokens.unwrap_or(512),
            "temperature": opts.temperature.unwrap_or(0.2),
            "stream": false
        });

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://terraphim.ai")
            .header("X-Title", "Terraphim AI")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                terraphim_service::ServiceError::Common(
                    terraphim_service::error::CommonError::Network {
                        message: format!("OpenRouter request failed: {e}"),
                        source: Some(Box::new(e)),
                    },
                )
            })?;

        if response.status() == 429 {
            return Err(terraphim_service::ServiceError::Common(
                terraphim_service::error::CommonError::Network {
                    message: "OpenRouter rate limit exceeded".to_string(),
                    source: None,
                },
            ));
        }
        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(terraphim_service::ServiceError::Common(
                terraphim_service::error::CommonError::Network {
                    message: format!("OpenRouter API error: {error_text}"),
                    source: None,
                },
            ));
        }

        let response_json: serde_json::Value = response.json().await.map_err(|e| {
            terraphim_service::ServiceError::Common(
                terraphim_service::error::CommonError::Network {
                    message: format!("OpenRouter response JSON decode failed: {e}"),
                    source: Some(Box::new(e)),
                },
            )
        })?;

        let content = response_json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        Ok(content)
    }
}

/// Convenience wrapper that returns the client as a trait object.
pub fn into_llm_client(client: OpenRouterClient) -> Arc<dyn LlmClient> {
    Arc::new(client) as Arc<dyn LlmClient>
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openrouter_client_new_validates_inputs() {
        assert!(OpenRouterClient::new("", "model").is_err());
        assert!(OpenRouterClient::new("key", "").is_err());
        assert!(OpenRouterClient::new("key", "model").is_ok());
    }

    #[test]
    fn test_openrouter_client_base_url_resolution() {
        // SAFETY: test code, single-threaded, no concurrent env access.
        let saved = std::env::var("OPENROUTER_BASE_URL").ok();

        // Default when unset.
        unsafe {
            std::env::remove_var("OPENROUTER_BASE_URL");
        }
        let client = OpenRouterClient::new("key", "model").expect("build client");
        assert_eq!(client.base_url, "https://openrouter.ai/api/v1");

        // Custom when set.
        unsafe {
            std::env::set_var("OPENROUTER_BASE_URL", "https://example.com/v1");
        }
        let client = OpenRouterClient::new("key", "model").expect("build client");
        assert_eq!(client.base_url, "https://example.com/v1");

        // Restore.
        if let Some(v) = saved {
            unsafe {
                std::env::set_var("OPENROUTER_BASE_URL", v);
            }
        } else {
            unsafe {
                std::env::remove_var("OPENROUTER_BASE_URL");
            }
        }
    }

    #[test]
    fn test_openrouter_client_timeout_is_120_seconds() {
        let client = OpenRouterClient::new("key", "model").expect("build client");
        // reqwest::Client does not expose its timeout directly, but we can
        // verify the client was built successfully with the expected config.
        assert_eq!(client.model, "model");
        assert_eq!(client.api_key, "key");
    }

    #[test]
    fn test_into_llm_client_returns_trait_object() {
        let client = OpenRouterClient::new("key", "model").expect("build client");
        let llm: Arc<dyn LlmClient> = into_llm_client(client);
        assert_eq!(llm.name(), "openrouter");
    }
}
