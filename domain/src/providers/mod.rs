//! Provider configuration types (provider-neutral, serde-free).
//!
//! These types define the shape of provider settings without depending
//! on any serialization format (TOML, JSON, etc.).

use std::collections::HashMap;

/// Top-level provider configuration.
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    /// Default provider name: "copilot", "anthropic", "openai", "bedrock", "azure".
    pub default: Option<String>,
    /// Explicit model → provider routing overrides.
    pub routing: HashMap<String, String>,
    /// AWS Bedrock settings.
    pub bedrock: BedrockProviderConfig,
    /// Anthropic API settings.
    pub anthropic: AnthropicProviderConfig,
    /// OpenAI API settings.
    pub openai: OpenAiProviderConfig,
}

/// AWS Bedrock provider configuration.
#[derive(Debug, Clone)]
pub struct BedrockProviderConfig {
    /// AWS region (default: "us-east-1").
    pub region: String,
    /// AWS profile name for credentials.
    pub profile: Option<String>,
    /// Max tokens per response (default: 8192).
    pub max_tokens: u32,
    /// Enable cross-region inference.
    pub cross_region: Option<bool>,
}

impl Default for BedrockProviderConfig {
    fn default() -> Self {
        Self {
            region: "us-east-1".to_string(),
            profile: None,
            max_tokens: 8192,
            cross_region: None,
        }
    }
}

/// Anthropic API provider configuration.
#[derive(Debug, Clone)]
pub struct AnthropicProviderConfig {
    /// Environment variable name for the API key (default: "ANTHROPIC_API_KEY").
    pub api_key_env: String,
    /// Direct API key (not recommended — use env var instead).
    pub api_key: Option<String>,
    /// Base URL for the Anthropic API.
    pub base_url: String,
    /// Max tokens per response (default: 8192).
    pub max_tokens: u32,
    /// Anthropic API version header.
    pub api_version: String,
}

impl Default for AnthropicProviderConfig {
    fn default() -> Self {
        Self {
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
            api_key: None,
            base_url: "https://api.anthropic.com".to_string(),
            max_tokens: 8192,
            api_version: "2023-06-01".to_string(),
        }
    }
}

/// OpenAI API provider configuration.
#[derive(Debug, Clone)]
pub struct OpenAiProviderConfig {
    /// Environment variable name for the API key (default: "OPENAI_API_KEY").
    pub api_key_env: String,
    /// Direct API key (not recommended — use env var instead).
    pub api_key: Option<String>,
    /// Base URL for the OpenAI API.
    pub base_url: String,
    /// Max tokens per response (default: 8192).
    pub max_tokens: u32,
}

impl Default for OpenAiProviderConfig {
    fn default() -> Self {
        Self {
            api_key_env: "OPENAI_API_KEY".to_string(),
            api_key: None,
            base_url: "https://api.openai.com".to_string(),
            max_tokens: 8192,
        }
    }
}
