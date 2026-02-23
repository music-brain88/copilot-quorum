//! Bedrock model ID mapping
//!
//! Maps domain `Model` variants to Bedrock model identifiers,
//! with optional cross-region inference prefix.

use quorum_domain::Model;

/// Convert a domain Model to a Bedrock model ID string.
///
/// Returns `None` for unsupported models (GPT, Gemini, etc.).
/// When `cross_region` is true, the model ID is prefixed with `"{region}."`.
pub fn to_bedrock_model_id(model: &Model, cross_region: bool, region: &str) -> Option<String> {
    let base_id = match model {
        Model::ClaudeSonnet45 => "anthropic.claude-sonnet-4-5-20250929-v1:0",
        Model::ClaudeHaiku45 => "anthropic.claude-haiku-4-5-20250929-v1:0",
        Model::ClaudeOpus45 => "anthropic.claude-opus-4-20250514-v1:0",
        Model::ClaudeSonnet4 => "anthropic.claude-sonnet-4-20250514-v1:0",
        Model::Custom(id) => return Some(id.clone()),
        _ => return None,
    };

    if cross_region {
        Some(format!("{region}.{base_id}"))
    } else {
        Some(base_id.to_string())
    }
}

/// Check if a model is supported by the Bedrock provider.
pub fn is_bedrock_supported(model: &Model) -> bool {
    model.is_claude() || matches!(model, Model::Custom(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_sonnet45_mapping() {
        let id = to_bedrock_model_id(&Model::ClaudeSonnet45, false, "us-east-1").unwrap();
        assert_eq!(id, "anthropic.claude-sonnet-4-5-20250929-v1:0");
    }

    #[test]
    fn test_claude_haiku45_mapping() {
        let id = to_bedrock_model_id(&Model::ClaudeHaiku45, false, "us-east-1").unwrap();
        assert_eq!(id, "anthropic.claude-haiku-4-5-20250929-v1:0");
    }

    #[test]
    fn test_claude_opus45_mapping() {
        let id = to_bedrock_model_id(&Model::ClaudeOpus45, false, "us-east-1").unwrap();
        assert_eq!(id, "anthropic.claude-opus-4-20250514-v1:0");
    }

    #[test]
    fn test_claude_sonnet4_mapping() {
        let id = to_bedrock_model_id(&Model::ClaudeSonnet4, false, "us-east-1").unwrap();
        assert_eq!(id, "anthropic.claude-sonnet-4-20250514-v1:0");
    }

    #[test]
    fn test_cross_region_prefix() {
        let id = to_bedrock_model_id(&Model::ClaudeSonnet45, true, "us-west-2").unwrap();
        assert_eq!(id, "us-west-2.anthropic.claude-sonnet-4-5-20250929-v1:0");
    }

    #[test]
    fn test_custom_model_passthrough() {
        let model = Model::Custom("my-fine-tuned-model".to_string());
        let id = to_bedrock_model_id(&model, false, "us-east-1").unwrap();
        assert_eq!(id, "my-fine-tuned-model");
    }

    #[test]
    fn test_custom_model_ignores_cross_region() {
        let model = Model::Custom("my-model".to_string());
        let id = to_bedrock_model_id(&model, true, "us-west-2").unwrap();
        // Custom models are passed through as-is (user manages the full ID)
        assert_eq!(id, "my-model");
    }

    #[test]
    fn test_unsupported_gpt_model() {
        assert!(to_bedrock_model_id(&Model::Gpt52Codex, false, "us-east-1").is_none());
    }

    #[test]
    fn test_unsupported_gemini_model() {
        assert!(to_bedrock_model_id(&Model::Gemini3Pro, false, "us-east-1").is_none());
    }

    #[test]
    fn test_is_bedrock_supported() {
        assert!(is_bedrock_supported(&Model::ClaudeSonnet45));
        assert!(is_bedrock_supported(&Model::ClaudeHaiku45));
        assert!(is_bedrock_supported(&Model::ClaudeOpus45));
        assert!(is_bedrock_supported(&Model::ClaudeSonnet4));
        assert!(is_bedrock_supported(&Model::Custom("anything".to_string())));
        assert!(!is_bedrock_supported(&Model::Gpt52Codex));
        assert!(!is_bedrock_supported(&Model::Gemini3Pro));
    }
}
