//! Model value object representing an LLM model

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Available LLM models (Value Object)
///
/// This is a domain concept representing the different AI models
/// that can participate in a Quorum discussion.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Model {
    // Claude models
    ClaudeSonnet46,
    ClaudeOpus46,
    ClaudeSonnet45,
    ClaudeHaiku45,
    ClaudeOpus45,
    ClaudeSonnet4,
    // GPT models
    Gpt52Codex,
    Gpt51CodexMax,
    Gpt51Codex,
    Gpt52,
    Gpt51,
    Gpt5,
    Gpt51CodexMini,
    Gpt5Mini,
    Gpt41,
    // Gemini models
    Gemini3Pro,
    // Custom
    Custom(String),
}

impl Model {
    /// Get the string identifier for this model
    pub fn as_str(&self) -> &str {
        match self {
            Model::ClaudeSonnet46 => "claude-sonnet-4.6",
            Model::ClaudeOpus46 => "claude-opus-4.6",
            Model::ClaudeSonnet45 => "claude-sonnet-4.5",
            Model::ClaudeHaiku45 => "claude-haiku-4.5",
            Model::ClaudeOpus45 => "claude-opus-4.5",
            Model::ClaudeSonnet4 => "claude-sonnet-4",
            Model::Gpt52Codex => "gpt-5.2-codex",
            Model::Gpt51CodexMax => "gpt-5.1-codex-max",
            Model::Gpt51Codex => "gpt-5.1-codex",
            Model::Gpt52 => "gpt-5.2",
            Model::Gpt51 => "gpt-5.1",
            Model::Gpt5 => "gpt-5",
            Model::Gpt51CodexMini => "gpt-5.1-codex-mini",
            Model::Gpt5Mini => "gpt-5-mini",
            Model::Gpt41 => "gpt-4.1",
            Model::Gemini3Pro => "gemini-3-pro-preview",
            Model::Custom(s) => s,
        }
    }

    /// Get the default set of models for a Quorum discussion
    pub fn default_models() -> Vec<Model> {
        vec![Model::Gpt52Codex, Model::ClaudeSonnet45, Model::Gemini3Pro]
    }

    /// Check if this is a Claude model
    pub fn is_claude(&self) -> bool {
        matches!(
            self,
            Model::ClaudeSonnet46
                | Model::ClaudeOpus46
                | Model::ClaudeSonnet45
                | Model::ClaudeHaiku45
                | Model::ClaudeOpus45
                | Model::ClaudeSonnet4
        )
    }

    /// Check if this is a GPT model
    pub fn is_gpt(&self) -> bool {
        matches!(
            self,
            Model::Gpt52Codex
                | Model::Gpt51CodexMax
                | Model::Gpt51Codex
                | Model::Gpt52
                | Model::Gpt51
                | Model::Gpt5
                | Model::Gpt51CodexMini
                | Model::Gpt5Mini
                | Model::Gpt41
        )
    }

    /// Check if this is a Gemini model
    pub fn is_gemini(&self) -> bool {
        matches!(self, Model::Gemini3Pro)
    }
}

impl Default for Model {
    /// Returns the default model (GPT-5.2-Codex)
    fn default() -> Self {
        Model::Gpt52Codex
    }
}

impl std::fmt::Display for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Model {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "claude-sonnet-4.6" => Model::ClaudeSonnet46,
            "claude-opus-4.6" => Model::ClaudeOpus46,
            "claude-sonnet-4.5" => Model::ClaudeSonnet45,
            "claude-haiku-4.5" => Model::ClaudeHaiku45,
            "claude-opus-4.5" => Model::ClaudeOpus45,
            "claude-sonnet-4" => Model::ClaudeSonnet4,
            "gpt-5.2-codex" => Model::Gpt52Codex,
            "gpt-5.1-codex-max" => Model::Gpt51CodexMax,
            "gpt-5.1-codex" => Model::Gpt51Codex,
            "gpt-5.2" => Model::Gpt52,
            "gpt-5.1" => Model::Gpt51,
            "gpt-5" => Model::Gpt5,
            "gpt-5.1-codex-mini" => Model::Gpt51CodexMini,
            "gpt-5-mini" => Model::Gpt5Mini,
            "gpt-4.1" => Model::Gpt41,
            "gemini-3-pro-preview" => Model::Gemini3Pro,
            other => Model::Custom(other.to_string()),
        })
    }
}

impl Serialize for Model {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Model {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(s.parse().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_roundtrip() {
        let models = Model::default_models();
        for model in models {
            let s = model.to_string();
            let parsed: Model = s.parse().unwrap();
            assert_eq!(model, parsed);
        }
    }

    #[test]
    fn test_custom_model() {
        let model: Model = "custom-model-v1".parse().unwrap();
        assert_eq!(model, Model::Custom("custom-model-v1".to_string()));
        assert_eq!(model.to_string(), "custom-model-v1");
    }

    #[test]
    fn test_model_family_detection() {
        assert!(Model::ClaudeSonnet45.is_claude());
        assert!(Model::Gpt52.is_gpt());
        assert!(Model::Gemini3Pro.is_gemini());
        assert!(!Model::ClaudeSonnet45.is_gpt());
    }

    #[test]
    fn test_model_default() {
        let model = Model::default();
        assert_eq!(model, Model::Gpt52Codex);
    }
}
