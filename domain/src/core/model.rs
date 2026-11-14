//! Model value object representing an LLM model

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Available LLM models (Value Object)
///
/// This is a domain concept representing the different AI models
/// that can participate in a Quorum discussion.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Model {
    // Claude models offered by Copilot CLI
    ClaudeSonnet55,
    ClaudeSonnet5,
    ClaudeOpus55,
    ClaudeOpus5,
    ClaudeOpus48,
    ClaudeHaiku55,
    // Claude models that Copilot CLI no longer offers (except Haiku 4.5).
    // They stay because the Bedrock provider maps them to Bedrock model IDs.
    ClaudeSonnet46,
    ClaudeOpus46,
    ClaudeSonnet45,
    ClaudeHaiku45,
    ClaudeOpus45,
    ClaudeSonnet4,
    // GPT models
    Gpt61Sol,
    Gpt61Terra,
    Gpt6Sol,
    Gpt6Luna,
    Gpt56Sol,
    Gpt56Terra,
    Gpt56Luna,
    Gpt54,
    Gpt53Codex,
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
    Gemini4Carbon,
    Gemini4Hydrogen,
    Gemini3Pro,
    Gemini31Pro,
    // Grok models
    Grok47,
    Grok46,
    // Microsoft AI models
    MaiCode11Flash,
    // Custom
    Custom(String),
}

impl Model {
    /// Get the string identifier for this model
    pub fn as_str(&self) -> &str {
        match self {
            Model::ClaudeSonnet55 => "claude-sonnet-5.5",
            Model::ClaudeSonnet5 => "claude-sonnet-5",
            Model::ClaudeOpus55 => "claude-opus-5.5",
            Model::ClaudeOpus5 => "claude-opus-5",
            Model::ClaudeOpus48 => "claude-opus-4.8",
            Model::ClaudeHaiku55 => "claude-haiku-5.5",
            Model::ClaudeSonnet46 => "claude-sonnet-4.6",
            Model::ClaudeOpus46 => "claude-opus-4.6",
            Model::ClaudeSonnet45 => "claude-sonnet-4.5",
            Model::ClaudeHaiku45 => "claude-haiku-4.5",
            Model::ClaudeOpus45 => "claude-opus-4.5",
            Model::ClaudeSonnet4 => "claude-sonnet-4",
            Model::Gpt61Sol => "gpt-6.1-sol",
            Model::Gpt61Terra => "gpt-6.1-terra",
            Model::Gpt6Sol => "gpt-6-sol",
            Model::Gpt6Luna => "gpt-6-luna",
            Model::Gpt56Sol => "gpt-5.6-sol",
            Model::Gpt56Terra => "gpt-5.6-terra",
            Model::Gpt56Luna => "gpt-5.6-luna",
            Model::Gpt54 => "gpt-5.4",
            Model::Gpt53Codex => "gpt-5.3-codex",
            Model::Gpt52Codex => "gpt-5.2-codex",
            Model::Gpt51CodexMax => "gpt-5.1-codex-max",
            Model::Gpt51Codex => "gpt-5.1-codex",
            Model::Gpt52 => "gpt-5.2",
            Model::Gpt51 => "gpt-5.1",
            Model::Gpt5 => "gpt-5",
            Model::Gpt51CodexMini => "gpt-5.1-codex-mini",
            Model::Gpt5Mini => "gpt-5-mini",
            Model::Gpt41 => "gpt-4.1",
            Model::Gemini4Carbon => "gemini-4-carbon",
            Model::Gemini4Hydrogen => "gemini-4-hydrogen",
            Model::Gemini3Pro => "gemini-3-pro-preview",
            Model::Gemini31Pro => "gemini-3.1-pro-preview",
            Model::Grok47 => "grok-4.7",
            Model::Grok46 => "grok-4.6",
            Model::MaiCode11Flash => "mai-code-1.1-flash",
            Model::Custom(s) => s,
        }
    }

    /// Get the default set of models for a Quorum discussion
    ///
    /// One model per vendor, so the discussion is heterogeneous by default.
    ///
    /// Note: Copilot CLI は提供モデルを入れ替えていく。1.0.65 で gpt-5.2-codex /
    /// gemini-3-pro-preview が (#262)、1.0.91 までに claude-sonnet-4.5 /
    /// gemini-3.1-pro-preview が利用不可になったため、その時点の後継モデルを
    /// デフォルトとしている。
    pub fn default_models() -> Vec<Model> {
        vec![Model::Gpt61Sol, Model::ClaudeSonnet55, Model::Gemini4Carbon]
    }

    /// Check if this is a Claude model
    pub fn is_claude(&self) -> bool {
        matches!(
            self,
            Model::ClaudeSonnet55
                | Model::ClaudeSonnet5
                | Model::ClaudeOpus55
                | Model::ClaudeOpus5
                | Model::ClaudeOpus48
                | Model::ClaudeHaiku55
                | Model::ClaudeSonnet46
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
            Model::Gpt61Sol
                | Model::Gpt61Terra
                | Model::Gpt6Sol
                | Model::Gpt6Luna
                | Model::Gpt56Sol
                | Model::Gpt56Terra
                | Model::Gpt56Luna
                | Model::Gpt54
                | Model::Gpt53Codex
                | Model::Gpt52Codex
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
        matches!(
            self,
            Model::Gemini4Carbon | Model::Gemini4Hydrogen | Model::Gemini3Pro | Model::Gemini31Pro
        )
    }
}

impl Default for Model {
    /// Returns the default model (Claude Sonnet 5.5)
    fn default() -> Self {
        Model::ClaudeSonnet55
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
            "claude-sonnet-5.5" => Model::ClaudeSonnet55,
            "claude-sonnet-5" => Model::ClaudeSonnet5,
            "claude-opus-5.5" => Model::ClaudeOpus55,
            "claude-opus-5" => Model::ClaudeOpus5,
            "claude-opus-4.8" => Model::ClaudeOpus48,
            "claude-haiku-5.5" => Model::ClaudeHaiku55,
            "claude-sonnet-4.6" => Model::ClaudeSonnet46,
            "claude-opus-4.6" => Model::ClaudeOpus46,
            "claude-sonnet-4.5" => Model::ClaudeSonnet45,
            "claude-haiku-4.5" => Model::ClaudeHaiku45,
            "claude-opus-4.5" => Model::ClaudeOpus45,
            "claude-sonnet-4" => Model::ClaudeSonnet4,
            "gpt-6.1-sol" => Model::Gpt61Sol,
            "gpt-6.1-terra" => Model::Gpt61Terra,
            "gpt-6-sol" => Model::Gpt6Sol,
            "gpt-6-luna" => Model::Gpt6Luna,
            "gpt-5.6-sol" => Model::Gpt56Sol,
            "gpt-5.6-terra" => Model::Gpt56Terra,
            "gpt-5.6-luna" => Model::Gpt56Luna,
            "gpt-5.4" => Model::Gpt54,
            "gpt-5.3-codex" => Model::Gpt53Codex,
            "gpt-5.2-codex" => Model::Gpt52Codex,
            "gpt-5.1-codex-max" => Model::Gpt51CodexMax,
            "gpt-5.1-codex" => Model::Gpt51Codex,
            "gpt-5.2" => Model::Gpt52,
            "gpt-5.1" => Model::Gpt51,
            "gpt-5" => Model::Gpt5,
            "gpt-5.1-codex-mini" => Model::Gpt51CodexMini,
            "gpt-5-mini" => Model::Gpt5Mini,
            "gpt-4.1" => Model::Gpt41,
            "gemini-4-carbon" => Model::Gemini4Carbon,
            "gemini-4-hydrogen" => Model::Gemini4Hydrogen,
            "gemini-3-pro-preview" => Model::Gemini3Pro,
            "gemini-3.1-pro-preview" => Model::Gemini31Pro,
            "grok-4.7" => Model::Grok47,
            "grok-4.6" => Model::Grok46,
            "mai-code-1.1-flash" => Model::MaiCode11Flash,
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
        assert_eq!(model, Model::ClaudeSonnet55);
    }

    #[test]
    fn test_current_lineup_roundtrip_and_family() {
        // (identifier, variant, is_claude, is_gpt, is_gemini)
        let lineup = [
            (
                "claude-sonnet-5.5",
                Model::ClaudeSonnet55,
                true,
                false,
                false,
            ),
            ("claude-sonnet-5", Model::ClaudeSonnet5, true, false, false),
            ("claude-opus-5.5", Model::ClaudeOpus55, true, false, false),
            ("claude-opus-5", Model::ClaudeOpus5, true, false, false),
            ("claude-opus-4.8", Model::ClaudeOpus48, true, false, false),
            ("claude-haiku-5.5", Model::ClaudeHaiku55, true, false, false),
            ("claude-haiku-4.5", Model::ClaudeHaiku45, true, false, false),
            ("gpt-6.1-sol", Model::Gpt61Sol, false, true, false),
            ("gpt-6.1-terra", Model::Gpt61Terra, false, true, false),
            ("gpt-6-sol", Model::Gpt6Sol, false, true, false),
            ("gpt-6-luna", Model::Gpt6Luna, false, true, false),
            ("gpt-5.6-sol", Model::Gpt56Sol, false, true, false),
            ("gpt-5.6-terra", Model::Gpt56Terra, false, true, false),
            ("gpt-5.6-luna", Model::Gpt56Luna, false, true, false),
            ("gpt-5.3-codex", Model::Gpt53Codex, false, true, false),
            ("gemini-4-carbon", Model::Gemini4Carbon, false, false, true),
            (
                "gemini-4-hydrogen",
                Model::Gemini4Hydrogen,
                false,
                false,
                true,
            ),
            ("grok-4.7", Model::Grok47, false, false, false),
            ("grok-4.6", Model::Grok46, false, false, false),
            (
                "mai-code-1.1-flash",
                Model::MaiCode11Flash,
                false,
                false,
                false,
            ),
        ];
        for (id, variant, claude, gpt, gemini) in lineup {
            assert_eq!(variant.as_str(), id);
            // A known identifier must parse to its variant, not to `Custom`.
            assert_eq!(id.parse::<Model>().unwrap(), variant);
            assert_eq!(variant.is_claude(), claude, "{id}");
            assert_eq!(variant.is_gpt(), gpt, "{id}");
            assert_eq!(variant.is_gemini(), gemini, "{id}");
        }
    }
}
