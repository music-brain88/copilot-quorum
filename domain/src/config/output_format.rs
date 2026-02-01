//! Output format value object

use serde::{Deserialize, Serialize};

/// Output format for Quorum results
///
/// This is a domain concept representing how the output should be formatted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// Full formatted output with all phases
    Full,
    /// Only the final synthesis (default)
    Synthesis,
    /// JSON output
    Json,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Synthesis
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_synthesis() {
        assert_eq!(OutputFormat::default(), OutputFormat::Synthesis);
    }

    #[test]
    fn test_serialize_lowercase() {
        let json = serde_json::to_string(&OutputFormat::Full).unwrap();
        assert_eq!(json, "\"full\"");
    }

    #[test]
    fn test_deserialize_lowercase() {
        let format: OutputFormat = serde_json::from_str("\"json\"").unwrap();
        assert_eq!(format, OutputFormat::Json);
    }
}
