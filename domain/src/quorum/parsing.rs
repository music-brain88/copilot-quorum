//! Vote response parsing for Quorum Consensus.
//!
//! These functions extract structured approval/rejection decisions from
//! free-form LLM review responses. They are pure domain logic — no I/O,
//! no session management, just text pattern matching.
//!
//! # Functions
//!
//! | Function | Use Case | Keywords |
//! |----------|----------|----------|
//! | [`parse_review_response`] | Plan/action review | APPROVE / REJECT |
//! | [`parse_final_review_response`] | Final outcome review | SUCCESS / FAILURE |
//! | [`parse_vote_score`] | Ensemble plan voting | Numeric score 1-10 |

/// Parse a review response to extract approval status and feedback.
///
/// Checks for explicit APPROVE/REJECT keywords in the response text.
/// Conservative: defaults to rejection when ambiguous.
///
/// # Returns
///
/// `(approved, full_response_as_feedback)`
pub fn parse_review_response(response: &str) -> (bool, String) {
    let response_upper = response.to_uppercase();

    // Check for explicit approval/rejection keywords
    let approved = response_upper.contains("APPROVE")
        && !response_upper.contains("NOT APPROVE")
        && !response_upper.contains("DON'T APPROVE")
        && !response_upper.contains("CANNOT APPROVE");

    let rejected = response_upper.contains("REJECT")
        || response_upper.contains("REVISE")
        || response_upper.contains("NOT APPROVE")
        || response_upper.contains("CANNOT APPROVE");

    // If explicitly rejected, return false
    // If explicitly approved and not rejected, return true
    // Otherwise, default to false (conservative)
    let is_approved = approved && !rejected;

    (is_approved, response.to_string())
}

/// Parse a final review response looking for SUCCESS/FAILURE indicators.
///
/// # Returns
///
/// `(success, full_response_as_feedback)`
pub fn parse_final_review_response(response: &str) -> (bool, String) {
    let response_upper = response.to_uppercase();

    // Look for SUCCESS/PARTIAL/FAILURE
    let success = response_upper.contains("SUCCESS")
        && !response_upper.contains("PARTIAL")
        && !response_upper.contains("FAILURE");

    (success, response.to_string())
}

/// Parse a vote score from ensemble voting response.
///
/// Parses the model's voting response to extract a numerical score (1-10).
/// Supports multiple response formats for robustness.
///
/// # Supported Formats
///
/// 1. **JSON** (preferred): `{"score": 8, "reasoning": "..."}`
/// 2. **Fraction**: `8/10` or `Score: 7/10`
/// 3. **Standalone number**: `9` (if in valid range 1-10)
///
/// # Return Value
///
/// - Returns the parsed score clamped to 1.0-10.0
/// - Returns 5.0 (neutral) if parsing fails
///
/// # Examples
///
/// ```
/// use quorum_domain::quorum::parsing::parse_vote_score;
///
/// assert_eq!(parse_vote_score(r#"{"score": 8, "reasoning": "Good"}"#), 8.0);
/// assert_eq!(parse_vote_score("I rate this 7/10"), 7.0);
/// assert_eq!(parse_vote_score("Score: 9"), 9.0);
/// assert_eq!(parse_vote_score("No numbers here"), 5.0); // fallback
/// ```
pub fn parse_vote_score(response: &str) -> f64 {
    // Try to find JSON in the response
    if let Some(start) = response.find('{')
        && let Some(end) = response[start..].rfind('}')
    {
        let json_str = &response[start..start + end + 1];
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str)
            && let Some(score) = parsed.get("score").and_then(|v| v.as_f64())
        {
            // Clamp to valid range
            return score.clamp(1.0, 10.0);
        }
    }

    // Fallback: try to find a number that looks like a score
    // Look for patterns like "8/10" or "score: 8" or just a standalone number
    for word in response.split_whitespace() {
        // Check for "N/10" pattern
        if let Some(num_str) = word.strip_suffix("/10")
            && let Ok(num) = num_str.parse::<f64>()
        {
            return num.clamp(1.0, 10.0);
        }
        // Check for standalone number (1-10)
        if let Ok(num) = word
            .trim_matches(|c: char| !c.is_ascii_digit())
            .parse::<f64>()
            && (1.0..=10.0).contains(&num)
        {
            return num;
        }
    }

    // Default to middle score if parsing fails
    5.0
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== parse_vote_score Tests ====================

    #[test]
    fn test_parse_vote_score_json() {
        // Standard JSON response
        let response = r#"{"score": 8, "reasoning": "Good plan"}"#;
        assert_eq!(parse_vote_score(response), 8.0);

        // With markdown code block
        let response = r#"
Here is my evaluation:
```json
{"score": 7, "reasoning": "Solid but could improve"}
```
"#;
        assert_eq!(parse_vote_score(response), 7.0);
    }

    #[test]
    fn test_parse_vote_score_pattern() {
        // "N/10" pattern
        assert_eq!(parse_vote_score("I rate this 8/10"), 8.0);
        assert_eq!(parse_vote_score("Score: 6/10"), 6.0);

        // Standalone number
        assert_eq!(parse_vote_score("My score is 9"), 9.0);
    }

    #[test]
    fn test_parse_vote_score_clamp() {
        // Clamps to valid range
        let response = r#"{"score": 15, "reasoning": "Too high"}"#;
        assert_eq!(parse_vote_score(response), 10.0);

        let response = r#"{"score": -5, "reasoning": "Too low"}"#;
        assert_eq!(parse_vote_score(response), 1.0);
    }

    #[test]
    fn test_parse_vote_score_fallback() {
        // Fallback to 5.0 when parsing fails
        assert_eq!(parse_vote_score("No numbers here"), 5.0);
        assert_eq!(parse_vote_score(""), 5.0);
    }

    // ==================== parse_review_response Tests ====================

    #[test]
    fn test_approve_response() {
        let (approved, _) = parse_review_response("I APPROVE this plan. It looks good.");
        assert!(approved);
    }

    #[test]
    fn test_reject_response() {
        let (approved, _) = parse_review_response("I REJECT this plan. It needs changes.");
        assert!(!approved);
    }

    #[test]
    fn test_cannot_approve() {
        let (approved, _) = parse_review_response("I CANNOT APPROVE this plan.");
        assert!(!approved);
    }

    #[test]
    fn test_revise_response() {
        let (approved, _) = parse_review_response("Please REVISE this approach.");
        assert!(!approved);
    }

    #[test]
    fn test_ambiguous_defaults_to_reject() {
        let (approved, _) = parse_review_response("This plan has some issues.");
        assert!(!approved);
    }

    // ==================== parse_final_review_response Tests ====================

    #[test]
    fn test_final_review_success() {
        let (success, _) = parse_final_review_response("Overall assessment: SUCCESS");
        assert!(success);
    }

    #[test]
    fn test_final_review_partial() {
        let (success, _) = parse_final_review_response("PARTIAL SUCCESS — some tasks failed");
        assert!(!success);
    }

    #[test]
    fn test_final_review_failure() {
        let (success, _) = parse_final_review_response("FAILURE — major issues found");
        assert!(!success);
    }
}
