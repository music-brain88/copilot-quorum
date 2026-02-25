//! Output format value object

use serde::{Deserialize, Serialize};

/// Output format for Quorum results
///
/// This is a domain concept representing how the output should be formatted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// Full formatted output with all phases
    Full,
    /// Only the final synthesis (default)
    #[default]
    Synthesis,
    /// JSON output
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "full" => Ok(OutputFormat::Full),
            "synthesis" => Ok(OutputFormat::Synthesis),
            "json" => Ok(OutputFormat::Json),
            _ => Err(format!(
                "invalid output format '{}', valid: full, synthesis, json",
                s
            )),
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Full => write!(f, "full"),
            OutputFormat::Synthesis => write!(f, "synthesis"),
            OutputFormat::Json => write!(f, "json"),
        }
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
