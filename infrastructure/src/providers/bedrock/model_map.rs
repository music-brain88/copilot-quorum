//! Bedrock model ID mapping
//!
//! Maps domain `Model` variants to Bedrock model identifiers,
//! with optional cross-region inference prefix.

use quorum_domain::Model;

/// Convert a domain Model to a Bedrock model ID string.
///
/// Returns `None` for unsupported models (GPT, Gemini, etc.).
///
/// - Models that require inference profiles (e.g. Claude 4.6) always use
///   the region-group prefix (`us.`, `eu.`, etc.) regardless of `cross_region`.
/// - When `cross_region` is true, other models are prefixed with `"{region}."`.
pub fn to_bedrock_model_id(model: &Model, cross_region: bool, region: &str) -> Option<String> {
    let base_id = match model {
        Model::ClaudeSonnet46 => "anthropic.claude-sonnet-4-6",
        Model::ClaudeOpus46 => "anthropic.claude-opus-4-6-v1",
        Model::ClaudeSonnet45 => "anthropic.claude-sonnet-4-5-20250929-v1:0",
        Model::ClaudeHaiku45 => "anthropic.claude-haiku-4-5-20250929-v1:0",
        Model::ClaudeOpus45 => "anthropic.claude-opus-4-20250514-v1:0",
        Model::ClaudeSonnet4 => "anthropic.claude-sonnet-4-20250514-v1:0",
        Model::Custom(id) => return Some(id.clone()),
        _ => return None,
    };

    if requires_inference_profile(model) {
        // Models that don't support on-demand throughput must use inference profiles
        let prefix = inference_profile_prefix(region);
        Some(format!("{prefix}.{base_id}"))
    } else if cross_region {
        Some(format!("{region}.{base_id}"))
    } else {
        Some(base_id.to_string())
    }
}

/// Whether a model requires an inference profile (cannot use on-demand throughput).
fn requires_inference_profile(model: &Model) -> bool {
    matches!(model, Model::ClaudeSonnet46 | Model::ClaudeOpus46)
}

/// Derive the inference profile region group from an AWS region string.
///
/// Cross-region inference profiles use continent-level prefixes:
/// `us-east-1` → `us`, `eu-west-1` → `eu`, `ap-northeast-1` → `ap`, etc.
fn inference_profile_prefix(region: &str) -> &str {
    match region.split('-').next() {
        Some(prefix @ ("us" | "eu" | "ap" | "me" | "sa" | "ca" | "af")) => prefix,
        _ => "us", // safe fallback
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
    fn test_claude_sonnet46_uses_inference_profile() {
        // 4.6 models always require inference profiles, even with cross_region=false
        let id = to_bedrock_model_id(&Model::ClaudeSonnet46, false, "us-east-1").unwrap();
        assert_eq!(id, "us.anthropic.claude-sonnet-4-6");
    }

    #[test]
    fn test_claude_opus46_uses_inference_profile() {
        let id = to_bedrock_model_id(&Model::ClaudeOpus46, false, "us-east-1").unwrap();
        assert_eq!(id, "us.anthropic.claude-opus-4-6-v1");
    }

    #[test]
    fn test_claude_sonnet46_eu_region() {
        let id = to_bedrock_model_id(&Model::ClaudeSonnet46, false, "eu-west-1").unwrap();
        assert_eq!(id, "eu.anthropic.claude-sonnet-4-6");
    }

    #[test]
    fn test_claude_opus46_ap_region() {
        let id = to_bedrock_model_id(&Model::ClaudeOpus46, false, "ap-northeast-1").unwrap();
        assert_eq!(id, "ap.anthropic.claude-opus-4-6-v1");
    }

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
        assert!(is_bedrock_supported(&Model::ClaudeSonnet46));
        assert!(is_bedrock_supported(&Model::ClaudeOpus46));
        assert!(is_bedrock_supported(&Model::ClaudeSonnet45));
        assert!(is_bedrock_supported(&Model::ClaudeHaiku45));
        assert!(is_bedrock_supported(&Model::ClaudeOpus45));
        assert!(is_bedrock_supported(&Model::ClaudeSonnet4));
        assert!(is_bedrock_supported(&Model::Custom("anything".to_string())));
        assert!(!is_bedrock_supported(&Model::Gpt52Codex));
        assert!(!is_bedrock_supported(&Model::Gemini3Pro));
    }
}
