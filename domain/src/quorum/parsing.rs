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
//! | [`parse_debate_verdict`] | Debate moderator checkpoint | VERDICT: SETTLED / CONTINUE |

/// Parse a review response to extract approval status and feedback.
///
/// Checks for explicit APPROVE/REJECT keywords in the response text.
/// Conservative: defaults to rejection when ambiguous.
///
/// # Approval Keywords
///
/// `APPROVE`, `PROCEED`, `LGTM`, `LOOKS SAFE`, `ACCEPTABLE`
/// (excluding negated forms like "NOT APPROVE", "CANNOT APPROVE")
///
/// # Rejection Keywords
///
/// `REJECT`, `REVISE THIS`, `NEEDS REVISION`, `SHOULD REVISE`,
/// `UNSAFE`, `DANGEROUS`, `DO NOT APPROVE`, `SHOULD NOT APPROVE`,
/// `NOT APPROVE`, `CANNOT APPROVE`
///
/// Note: `"REVISED"` (past tense / adjective) is NOT treated as rejection.
/// This prevents false positives like `"The revised plan looks good. APPROVE."`
///
/// # Returns
///
/// `(approved, full_response_as_feedback)`
pub fn parse_review_response(response: &str) -> (bool, String) {
    let response_upper = response.to_uppercase();

    // --- Approval detection ---
    let approved = (response_upper.contains("APPROVE")
        || response_upper.contains("PROCEED")
        || response_upper.contains("LGTM")
        || response_upper.contains("LOOKS SAFE")
        || response_upper.contains("ACCEPTABLE"))
        && !response_upper.contains("NOT APPROVE")
        && !response_upper.contains("DON'T APPROVE")
        && !response_upper.contains("CANNOT APPROVE")
        && !response_upper.contains("DO NOT APPROVE")
        && !response_upper.contains("SHOULD NOT APPROVE");

    // --- Rejection detection ---
    // "REVISE" requires action-oriented context to avoid false positives
    // with "REVISED" (adjective/past tense, e.g. "the revised plan looks good")
    let has_actionable_revise = response_upper.contains("REVISE THIS")
        || response_upper.contains("NEEDS REVISION")
        || response_upper.contains("SHOULD REVISE")
        || response_upper.contains("MUST REVISE")
        || response_upper.contains("PLEASE REVISE");

    let rejected = response_upper.contains("REJECT")
        || has_actionable_revise
        || response_upper.contains("NOT APPROVE")
        || response_upper.contains("CANNOT APPROVE")
        || response_upper.contains("DO NOT APPROVE")
        || response_upper.contains("SHOULD NOT APPROVE")
        || response_upper.contains("UNSAFE")
        || response_upper.contains("DANGEROUS");

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

/// Parse a Debate moderator's round checkpoint response.
///
/// The moderator is instructed to open its response with a `VERDICT: SETTLED`
/// or `VERDICT: CONTINUE` line, followed by the conclusion (if settled) or
/// guidance for the next round (if not). This extracts that verdict and
/// strips the verdict line from the returned body.
///
/// Conservative: if the first line doesn't clearly declare `VERDICT: SETTLED`
/// (e.g. the model ignored the format), this returns `settled = false` so the
/// debate keeps going rather than cutting off on a misread response — the
/// caller's `max_rounds` cap is what ultimately bounds the debate.
///
/// # Returns
///
/// `(settled, body)` — `body` has the leading verdict line removed when present.
///
/// # Examples
///
/// ```
/// use quorum_domain::quorum::parsing::parse_debate_verdict;
///
/// let (settled, body) = parse_debate_verdict("VERDICT: SETTLED\n\nThe proponent's design wins.");
/// assert!(settled);
/// assert_eq!(body, "The proponent's design wins.");
///
/// let (settled, body) = parse_debate_verdict("VERDICT: CONTINUE\n\nStill unresolved: caching.");
/// assert!(!settled);
/// assert_eq!(body, "Still unresolved: caching.");
/// ```
pub fn parse_debate_verdict(response: &str) -> (bool, String) {
    let mut lines = response.lines();
    let first_line = lines.next().unwrap_or("");
    let first_upper = first_line.to_uppercase();

    if !first_upper.contains("VERDICT") {
        return (false, response.trim().to_string());
    }

    let settled = first_upper.contains("SETTLED") && !first_upper.contains("CONTINUE");
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    let body = if body.is_empty() {
        response.trim().to_string()
    } else {
        body
    };

    (settled, body)
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

    #[test]
    fn test_proceed_response() {
        let (approved, _) = parse_review_response("PROCEED with this plan. It looks fine.");
        assert!(approved);
    }

    #[test]
    fn test_lgtm_response() {
        let (approved, _) = parse_review_response("LGTM, this command is safe to execute.");
        assert!(approved);
    }

    #[test]
    fn test_looks_safe_response() {
        let (approved, _) = parse_review_response("This LOOKS SAFE to me. Go ahead.");
        assert!(approved);
    }

    #[test]
    fn test_acceptable_response() {
        let (approved, _) = parse_review_response("This is ACCEPTABLE. No concerns.");
        assert!(approved);
    }

    #[test]
    fn test_revised_adjective_not_rejection() {
        // "REVISED" as adjective — should NOT be treated as rejection
        let (approved, _) =
            parse_review_response("The revised plan looks good. I APPROVE this approach.");
        assert!(approved);
    }

    #[test]
    fn test_revise_this_is_rejection() {
        let (approved, _) = parse_review_response("Please REVISE THIS approach. It has issues.");
        assert!(!approved);
    }

    #[test]
    fn test_needs_revision_is_rejection() {
        let (approved, _) = parse_review_response("This plan NEEDS REVISION before proceeding.");
        assert!(!approved);
    }

    #[test]
    fn test_should_revise_is_rejection() {
        let (approved, _) = parse_review_response("You SHOULD REVISE the error handling.");
        assert!(!approved);
    }

    #[test]
    fn test_dangerous_is_rejection() {
        let (approved, _) = parse_review_response("This command is DANGEROUS. Do not run it.");
        assert!(!approved);
    }

    #[test]
    fn test_unsafe_is_rejection() {
        let (approved, _) = parse_review_response("The operation is UNSAFE for production.");
        assert!(!approved);
    }

    #[test]
    fn test_do_not_approve() {
        let (approved, _) = parse_review_response("I DO NOT APPROVE this action.");
        assert!(!approved);
    }

    #[test]
    fn test_should_not_approve() {
        let (approved, _) = parse_review_response("We SHOULD NOT APPROVE this plan.");
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

    // ==================== parse_debate_verdict Tests ====================

    #[test]
    fn test_debate_verdict_settled() {
        let (settled, body) =
            parse_debate_verdict("VERDICT: SETTLED\n\nThe proponent's design wins.");
        assert!(settled);
        assert_eq!(body, "The proponent's design wins.");
    }

    #[test]
    fn test_debate_verdict_continue() {
        let (settled, body) =
            parse_debate_verdict("VERDICT: CONTINUE\n\nStill unresolved: caching strategy.");
        assert!(!settled);
        assert_eq!(body, "Still unresolved: caching strategy.");
    }

    #[test]
    fn test_debate_verdict_case_insensitive() {
        let (settled, _) = parse_debate_verdict("verdict: settled\n\nDone.");
        assert!(settled);
    }

    #[test]
    fn test_debate_verdict_missing_format_defaults_to_continue() {
        // Model ignored the VERDICT format entirely — conservative default: keep debating.
        let (settled, body) = parse_debate_verdict("I think the proponent has a stronger case.");
        assert!(!settled);
        assert_eq!(body, "I think the proponent has a stronger case.");
    }

    #[test]
    fn test_debate_verdict_settled_with_no_body_falls_back_to_full_response() {
        let (settled, body) = parse_debate_verdict("VERDICT: SETTLED");
        assert!(settled);
        assert_eq!(body, "VERDICT: SETTLED");
    }
}
