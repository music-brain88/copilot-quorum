//! Provider configuration from TOML (`[providers]` section)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileBedrockConfig {
    /// AWS region for Bedrock models (default: "us-east-1")
    pub region: String,
    /// AWS profile name for credentials (default: "default")
    pub profile: Option<String>,
    /// Max Tokens per response (default: 8192)
    pub max_tokens: u32,
}

impl Default for FileBedrockConfig {
    fn default() -> Self {
        Self {
            region: "us-east-1".to_string(),
            profile: None,
            max_tokens: 8192,
        }
    }
}

/// Anthropic API provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileAnthropicConfig {
    /// Environment variable name for the API key (default: "ANTHROPIC_API_KEY").
    pub api_key_env: String,
    /// Direct API key (not recommended — use env var instead).
    pub api_key: Option<String>,
    /// Base URL for the Anthropic API.
    pub base_url: String,
    /// Default max tokens per response.
    pub max_tokens: u32,
    /// Anthropic API version header.
    pub api_version: String,
}

impl Default for FileAnthropicConfig {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileOpenAiConfig {
    /// Environment variable name for the API key (default: "OPENAI_API_KEY").
    pub api_key_env: String,
    /// Direct API key (not recommended — use env var instead).
    pub api_key: Option<String>,
    /// Base URL for the OpenAI API (can be overridden for Azure OpenAI).
    pub base_url: String,
    /// Default max tokens per response.
    pub max_tokens: u32,
}

impl Default for FileOpenAiConfig {
    fn default() -> Self {
        Self {
            api_key_env: "OPENAI_API_KEY".to_string(),
            api_key: None,
            base_url: "https://api.openai.com".to_string(),
            max_tokens: 8192,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileProvidersConfig {
    /// Default provider: "copilot", "anthropic", "openai", "bedrock", "azure".
    pub default: Option<String>,
    /// Anthropic API settings.
    pub anthropic: FileAnthropicConfig,
    /// OpenAI API settings.
    pub openai: FileOpenAiConfig,
    /// AWS Bedrock settings.
    pub bedrock: FileBedrockConfig,
    /// Explicit model → provider routing overrides.
    pub routing: HashMap<String, String>,
}
