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
//! | [`parse_opponent_rebuttals`] | Debate opponent rebuttals | CLAIM: / EVIDENCE: / SEVERITY: |
//! | [`parse_moderator_rulings`] | Debate moderator per-rebuttal ruling | REBUTTAL_ID: / RULING: / REASON: |
//! | [`parse_divergence_check`] | Panel divergence checkpoint | DIVERGENT: YES / NO |
//! | [`parse_decomposition_request`] | Opponent decomposition request | DECOMPOSE_REQUEST: |

use super::objection::ObjectionSeverity;

/// Check whether `line` (after trimming leading whitespace) starts with
/// `label`, case-insensitively, and return the trimmed rest if so.
///
/// Compares as byte slices (`[u8]::eq_ignore_ascii_case`) rather than
/// slicing `&str` by byte length: `label` is always ASCII, but `line` may
/// not be (e.g. CJK text before a label appears, or in the label's value).
/// Slicing a `&str` at an arbitrary byte offset panics if that offset falls
/// inside a multi-byte character; comparing raw bytes via `.get(..n)` never
/// panics, and a successful case-insensitive match against an all-ASCII
/// label guarantees every matched byte is itself ASCII — so the following
/// `&trimmed[label.len()..]` slice always lands on a valid char boundary.
fn label_rest<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let trimmed = line.trim_start();
    let prefix = trimmed.as_bytes().get(..label.len())?;
    if prefix.eq_ignore_ascii_case(label.as_bytes()) {
        Some(trimmed[label.len()..].trim_start())
    } else {
        None
    }
}

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

/// Parse the opponent's structured rebuttals out of a Debate response.
///
/// The opponent is instructed (`opponent_system`) to raise each rebuttal as a
/// `CLAIM:` / `EVIDENCE:` / `SEVERITY:` block. A single response may contain
/// multiple such blocks back-to-back; each is returned as one tuple. Field
/// values may span multiple lines — a value is collected until the next
/// recognized label (`CLAIM:`, `EVIDENCE:`, or `SEVERITY:`) appears.
///
/// # Conservative Defaults
///
/// - A block missing its `SEVERITY:` line is still returned, defaulting to
///   [`ObjectionSeverity::Minor`] — under-classifying an unclassified rebuttal
///   is safer than dropping it entirely (it stays visible to the moderator,
///   just without inflated urgency).
/// - A block missing `CLAIM:` or `EVIDENCE:` entirely (i.e. no concrete
///   attack was ever opened) is dropped — there's nothing falsifiable to act
///   on.
/// - Completely unstructured text (no recognized labels at all) yields an
///   empty vec rather than guessing at intent.
///
/// # Examples
///
/// ```
/// use quorum_domain::quorum::parsing::parse_opponent_rebuttals;
/// use quorum_domain::quorum::ObjectionSeverity;
///
/// let text = "\
/// CLAIM: the cache never expires
/// EVIDENCE: TTL is set to None in config.rs:42
/// SEVERITY: MAJOR
///
/// CLAIM: concurrent writes are unguarded
/// EVIDENCE: no mutex around the shared counter in worker.rs
/// SEVERITY: CRITICAL";
///
/// let rebuttals = parse_opponent_rebuttals(text);
/// assert_eq!(rebuttals.len(), 2);
/// assert_eq!(rebuttals[0].2, ObjectionSeverity::Major);
/// assert_eq!(rebuttals[1].2, ObjectionSeverity::Critical);
/// ```
pub fn parse_opponent_rebuttals(text: &str) -> Vec<(String, String, ObjectionSeverity)> {
    #[derive(Clone, Copy, PartialEq)]
    enum Field {
        None,
        Claim,
        Evidence,
        Severity,
    }

    fn severity_from_str(value: &str) -> Option<ObjectionSeverity> {
        let upper = value.to_uppercase();
        if upper.contains("CRITICAL") {
            Some(ObjectionSeverity::Critical)
        } else if upper.contains("MAJOR") {
            Some(ObjectionSeverity::Major)
        } else if upper.contains("MINOR") {
            Some(ObjectionSeverity::Minor)
        } else {
            None
        }
    }

    let mut results = Vec::new();
    let mut claim: Option<String> = None;
    let mut evidence: Option<String> = None;
    let mut severity: Option<ObjectionSeverity> = None;
    let mut current = Field::None;
    let mut buf = String::new();

    // Flush whatever is in `buf` into the field named by `current`.
    macro_rules! flush_current {
        () => {
            let value = buf.trim().to_string();
            match current {
                Field::Claim => claim = Some(value),
                Field::Evidence => evidence = Some(value),
                Field::Severity => severity = severity_from_str(&value),
                Field::None => {}
            }
            buf.clear();
        };
    }

    // Push the accumulated triple (if it has at least claim+evidence) and
    // reset for the next block.
    macro_rules! push_if_complete {
        () => {
            if let (Some(c), Some(e)) = (claim.take(), evidence.take()) {
                results.push((c, e, severity.take().unwrap_or(ObjectionSeverity::Minor)));
            } else {
                severity.take();
            }
        };
    }

    for line in text.lines() {
        if let Some(rest) = label_rest(line, "CLAIM:") {
            flush_current!();
            push_if_complete!();
            current = Field::Claim;
            buf.push_str(rest);
        } else if let Some(rest) = label_rest(line, "EVIDENCE:") {
            flush_current!();
            current = Field::Evidence;
            buf.push_str(rest);
        } else if let Some(rest) = label_rest(line, "SEVERITY:") {
            flush_current!();
            current = Field::Severity;
            buf.push_str(rest);
        } else if current != Field::None {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(trimmed);
            }
        }
    }
    flush_current!();
    push_if_complete!();

    results
}

/// Parse the moderator's per-rebuttal rulings from a Debate checkpoint response.
///
/// The moderator is instructed (`moderator_system`) to rule on each rebuttal
/// with a `REBUTTAL_ID:` / `RULING: ACCEPTED|REJECTED` / `REASON:` block,
/// repeated once per rebuttal under consideration. This extracts each block
/// as a `(rebuttal_id, accepted, reason)` tuple.
///
/// Matching a returned `rebuttal_id` against a known [`ObjectionLedger`]
/// (e.g. via `ObjectionLedger::apply_ruling`) is the caller's responsibility
/// and uses **exact string equality only** — no fuzzy matching is performed
/// here or by the ledger. If the moderator references an ID that doesn't
/// exist in the ledger, the caller should log a warning (`tracing::warn!`)
/// and skip it; this parser does not have access to the ledger and cannot
/// validate IDs itself.
///
/// [`ObjectionLedger`]: super::objection::ObjectionLedger
///
/// # Conservative Defaults
///
/// - A block whose `RULING:` line doesn't clearly say `ACCEPTED` (or says
///   both/neither) defaults to `accepted = false` — an unclear ruling should
///   not silently dismiss an objection.
/// - A block missing `REBUTTAL_ID:` is dropped — there's no ID to apply the
///   ruling to.
/// - Unstructured text with no recognized blocks yields an empty vec.
///
/// # Examples
///
/// ```
/// use quorum_domain::quorum::parsing::parse_moderator_rulings;
///
/// let text = "\
/// REBUTTAL_ID: R1-1
/// RULING: ACCEPTED
/// REASON: the counterexample holds
///
/// REBUTTAL_ID: R1-2
/// RULING: REJECTED
/// REASON: no concrete evidence was given";
///
/// let rulings = parse_moderator_rulings(text);
/// assert_eq!(rulings.len(), 2);
/// assert_eq!(rulings[0], ("R1-1".to_string(), true, "the counterexample holds".to_string()));
/// assert!(!rulings[1].1);
/// ```
pub fn parse_moderator_rulings(text: &str) -> Vec<(String, bool, String)> {
    #[derive(Clone, Copy, PartialEq)]
    enum Field {
        None,
        RebuttalId,
        Ruling,
        Reason,
    }

    fn ruling_accepted(value: &str) -> bool {
        let upper = value.to_uppercase();
        upper.contains("ACCEPTED") && !upper.contains("REJECTED")
    }

    let mut results = Vec::new();
    let mut rebuttal_id: Option<String> = None;
    let mut accepted: Option<bool> = None;
    let mut reason: Option<String> = None;
    let mut current = Field::None;
    let mut buf = String::new();

    macro_rules! flush_current {
        () => {
            let value = buf.trim().to_string();
            match current {
                Field::RebuttalId => rebuttal_id = Some(value),
                Field::Ruling => accepted = Some(ruling_accepted(&value)),
                Field::Reason => reason = Some(value),
                Field::None => {}
            }
            buf.clear();
        };
    }

    macro_rules! push_if_complete {
        () => {
            if let Some(id) = rebuttal_id.take() {
                results.push((
                    id,
                    accepted.take().unwrap_or(false),
                    reason.take().unwrap_or_default(),
                ));
            } else {
                accepted.take();
                reason.take();
            }
        };
    }

    for line in text.lines() {
        if let Some(rest) = label_rest(line, "REBUTTAL_ID:") {
            flush_current!();
            push_if_complete!();
            current = Field::RebuttalId;
            buf.push_str(rest);
        } else if let Some(rest) = label_rest(line, "RULING:") {
            flush_current!();
            current = Field::Ruling;
            buf.push_str(rest);
        } else if let Some(rest) = label_rest(line, "REASON:") {
            flush_current!();
            current = Field::Reason;
            buf.push_str(rest);
        } else if current != Field::None {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(trimmed);
            }
        }
    }
    flush_current!();
    push_if_complete!();

    results
}

/// Parse a panel moderator's divergence-check response.
///
/// Mirrors [`parse_debate_verdict`]'s structure: the moderator opens with a
/// `DIVERGENT: YES` or `DIVERGENT: NO` line, followed by either the shared
/// premise (if not divergent) or a note on what diverges (if divergent).
///
/// Conservative: if the first line doesn't clearly declare `DIVERGENT: YES`,
/// this returns `divergent = false` — an ambiguous or malformed response is
/// treated as "no divergence detected" rather than triggering a possibly
/// unwarranted decomposition.
///
/// # Returns
///
/// `(divergent, shared_premise_or_note)` — the note has the leading
/// `DIVERGENT:` line removed when present.
///
/// # Examples
///
/// ```
/// use quorum_domain::quorum::parsing::parse_divergence_check;
///
/// let (divergent, note) = parse_divergence_check(
///     "DIVERGENT: YES\n\nThe two sides are answering different questions."
/// );
/// assert!(divergent);
/// assert_eq!(note, "The two sides are answering different questions.");
///
/// let (divergent, note) = parse_divergence_check(
///     "DIVERGENT: NO\n\nBoth sides agree the API should be idempotent."
/// );
/// assert!(!divergent);
/// assert_eq!(note, "Both sides agree the API should be idempotent.");
/// ```
pub fn parse_divergence_check(text: &str) -> (bool, String) {
    let mut lines = text.lines();
    let first_line = lines.next().unwrap_or("");
    let first_upper = first_line.to_uppercase();

    if !first_upper.contains("DIVERGENT") {
        return (false, text.trim().to_string());
    }

    let divergent = first_upper.contains("YES") && !first_upper.contains("NO");
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    let body = if body.is_empty() {
        text.trim().to_string()
    } else {
        body
    };

    (divergent, body)
}

/// Detect an opponent's request to decompose the debate into sub-questions.
///
/// The opponent may open its response with a `DECOMPOSE_REQUEST: <target>`
/// line near the top when it believes the current question conflates
/// multiple independent claims that should be debated separately. This
/// scans only the first few lines of the response (not the whole body) so a
/// later, incidental mention of the phrase in the rebuttal text itself isn't
/// mistaken for an actual request.
///
/// # Returns
///
/// `Some(target)` with the trimmed text following the label if a
/// `DECOMPOSE_REQUEST:` line is found near the top; `None` otherwise
/// (including when the label appears only later in the response).
///
/// # Examples
///
/// ```
/// use quorum_domain::quorum::parsing::parse_decomposition_request;
///
/// let text = "DECOMPOSE_REQUEST: whether caching AND retry policy should be separate\n\nRest of the rebuttal...";
/// assert_eq!(
///     parse_decomposition_request(text),
///     Some("whether caching AND retry policy should be separate".to_string())
/// );
///
/// assert_eq!(parse_decomposition_request("CLAIM: the cache never expires"), None);
/// ```
pub fn parse_decomposition_request(text: &str) -> Option<String> {
    const LABEL: &str = "DECOMPOSE_REQUEST:";
    const NEAR_TOP_LINES: usize = 5;

    for line in text.lines().take(NEAR_TOP_LINES) {
        if let Some(rest) = label_rest(line, LABEL) {
            let target = rest.trim().to_string();
            if !target.is_empty() {
                return Some(target);
            }
            return None;
        }
    }
    None
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

    // ==================== parse_opponent_rebuttals Tests ====================

    #[test]
    fn test_opponent_rebuttals_single_block() {
        let text = "CLAIM: the cache never expires\nEVIDENCE: TTL is None in config.rs:42\nSEVERITY: MAJOR";
        let rebuttals = parse_opponent_rebuttals(text);
        assert_eq!(rebuttals.len(), 1);
        assert_eq!(rebuttals[0].0, "the cache never expires");
        assert_eq!(rebuttals[0].1, "TTL is None in config.rs:42");
        assert_eq!(rebuttals[0].2, ObjectionSeverity::Major);
    }

    #[test]
    fn test_opponent_rebuttals_multiple_blocks() {
        let text = "\
CLAIM: claim one
EVIDENCE: evidence one
SEVERITY: CRITICAL

CLAIM: claim two
EVIDENCE: evidence two
SEVERITY: MINOR";
        let rebuttals = parse_opponent_rebuttals(text);
        assert_eq!(rebuttals.len(), 2);
        assert_eq!(rebuttals[0].2, ObjectionSeverity::Critical);
        assert_eq!(rebuttals[1].2, ObjectionSeverity::Minor);
    }

    #[test]
    fn test_opponent_rebuttals_multiline_evidence() {
        let text = "\
CLAIM: concurrent writes are unguarded
EVIDENCE: no mutex around the shared counter.
This can be reproduced by spawning two writers.
SEVERITY: CRITICAL";
        let rebuttals = parse_opponent_rebuttals(text);
        assert_eq!(rebuttals.len(), 1);
        assert_eq!(
            rebuttals[0].1,
            "no mutex around the shared counter.\nThis can be reproduced by spawning two writers."
        );
    }

    #[test]
    fn test_opponent_rebuttals_missing_severity_defaults_to_minor() {
        let text = "CLAIM: claim only\nEVIDENCE: evidence only";
        let rebuttals = parse_opponent_rebuttals(text);
        assert_eq!(rebuttals.len(), 1);
        assert_eq!(rebuttals[0].2, ObjectionSeverity::Minor);
    }

    #[test]
    fn test_opponent_rebuttals_missing_evidence_is_dropped() {
        // No concrete evidence ever given — not falsifiable, so it's dropped.
        let text = "CLAIM: claim without evidence\nSEVERITY: MAJOR";
        let rebuttals = parse_opponent_rebuttals(text);
        assert!(rebuttals.is_empty());
    }

    #[test]
    fn test_opponent_rebuttals_unstructured_text_is_empty() {
        let rebuttals = parse_opponent_rebuttals("This plan has some vague issues.");
        assert!(rebuttals.is_empty());
    }

    #[test]
    fn test_opponent_rebuttals_case_insensitive_labels() {
        let text = "claim: lowercase claim\nevidence: lowercase evidence\nseverity: minor";
        let rebuttals = parse_opponent_rebuttals(text);
        assert_eq!(rebuttals.len(), 1);
        assert_eq!(rebuttals[0].2, ObjectionSeverity::Minor);
    }

    #[test]
    fn test_opponent_rebuttals_multibyte_line_before_label_does_not_panic() {
        // Regression: a non-ASCII line preceding a label used to panic when
        // slicing `trimmed[..label.len()]` landed mid-character (e.g. inside
        // 'こ') instead of returning "no match" for the byte comparison.
        let text = "→ この主張は誤りです\nCLAIM: キャッシュは無効\nEVIDENCE: 根拠あり\nSEVERITY: MAJOR";
        let rebuttals = parse_opponent_rebuttals(text);
        assert_eq!(rebuttals.len(), 1);
        assert_eq!(rebuttals[0].0, "キャッシュは無効");
        assert_eq!(rebuttals[0].1, "根拠あり");
        assert_eq!(rebuttals[0].2, ObjectionSeverity::Major);
    }

    #[test]
    fn test_opponent_rebuttals_emoji_line_before_label_does_not_panic() {
        // Same class of bug, exercised with a 4-byte emoji character instead
        // of a 3-byte CJK one.
        let text = "🎉 disagree\nCLAIM: 🎉 emoji claim\nEVIDENCE: 🎉 emoji evidence\nSEVERITY: CRITICAL";
        let rebuttals = parse_opponent_rebuttals(text);
        assert_eq!(rebuttals.len(), 1);
        assert_eq!(rebuttals[0].0, "🎉 emoji claim");
        assert_eq!(rebuttals[0].1, "🎉 emoji evidence");
        assert_eq!(rebuttals[0].2, ObjectionSeverity::Critical);
    }

    // ==================== parse_moderator_rulings Tests ====================

    #[test]
    fn test_moderator_rulings_single_block_accepted() {
        let text = "REBUTTAL_ID: R1-1\nRULING: ACCEPTED\nREASON: the counterexample holds";
        let rulings = parse_moderator_rulings(text);
        assert_eq!(rulings.len(), 1);
        assert_eq!(rulings[0].0, "R1-1");
        assert!(rulings[0].1);
        assert_eq!(rulings[0].2, "the counterexample holds");
    }

    #[test]
    fn test_moderator_rulings_multiple_blocks() {
        let text = "\
REBUTTAL_ID: R1-1
RULING: ACCEPTED
REASON: solid evidence

REBUTTAL_ID: R1-2
RULING: REJECTED
REASON: unfalsifiable impression";
        let rulings = parse_moderator_rulings(text);
        assert_eq!(rulings.len(), 2);
        assert_eq!(
            rulings[0],
            ("R1-1".to_string(), true, "solid evidence".to_string())
        );
        assert_eq!(
            rulings[1],
            (
                "R1-2".to_string(),
                false,
                "unfalsifiable impression".to_string()
            )
        );
    }

    #[test]
    fn test_moderator_rulings_ambiguous_ruling_defaults_to_rejected() {
        let text = "REBUTTAL_ID: R2-1\nRULING: unclear\nREASON: model ignored the format";
        let rulings = parse_moderator_rulings(text);
        assert_eq!(rulings.len(), 1);
        assert!(!rulings[0].1);
    }

    #[test]
    fn test_moderator_rulings_missing_id_is_dropped() {
        let text = "RULING: ACCEPTED\nREASON: no id given";
        let rulings = parse_moderator_rulings(text);
        assert!(rulings.is_empty());
    }

    #[test]
    fn test_moderator_rulings_unstructured_text_is_empty() {
        let rulings = parse_moderator_rulings("No structured rulings here.");
        assert!(rulings.is_empty());
    }

    #[test]
    fn test_moderator_rulings_exact_id_match_no_fuzzy() {
        // Distinct-but-similar IDs must remain distinct — no fuzzy collapsing.
        let text = "\
REBUTTAL_ID: R1-1
RULING: ACCEPTED
REASON: first

REBUTTAL_ID: R1-10
RULING: REJECTED
REASON: second";
        let rulings = parse_moderator_rulings(text);
        assert_eq!(rulings.len(), 2);
        assert_eq!(rulings[0].0, "R1-1");
        assert_eq!(rulings[1].0, "R1-10");
    }

    #[test]
    fn test_moderator_rulings_multibyte_line_before_label_does_not_panic() {
        // Regression: see test_opponent_rebuttals_multibyte_line_before_label_does_not_panic.
        let text = "→ 裁定は以下の通り\nREBUTTAL_ID: R1-1\nRULING: ACCEPTED\nREASON: 妥当";
        let rulings = parse_moderator_rulings(text);
        assert_eq!(rulings.len(), 1);
        assert_eq!(rulings[0].0, "R1-1");
        assert!(rulings[0].1);
        assert_eq!(rulings[0].2, "妥当");
    }

    // ==================== parse_divergence_check Tests ====================

    #[test]
    fn test_divergence_check_yes() {
        let (divergent, note) = parse_divergence_check(
            "DIVERGENT: YES\n\nThe sides are answering different questions.",
        );
        assert!(divergent);
        assert_eq!(note, "The sides are answering different questions.");
    }

    #[test]
    fn test_divergence_check_no() {
        let (divergent, note) =
            parse_divergence_check("DIVERGENT: NO\n\nBoth sides agree on the shared premise.");
        assert!(!divergent);
        assert_eq!(note, "Both sides agree on the shared premise.");
    }

    #[test]
    fn test_divergence_check_case_insensitive() {
        let (divergent, _) = parse_divergence_check("divergent: yes\n\nSplit detected.");
        assert!(divergent);
    }

    #[test]
    fn test_divergence_check_missing_format_defaults_to_not_divergent() {
        let (divergent, note) = parse_divergence_check("The two sides seem to agree overall.");
        assert!(!divergent);
        assert_eq!(note, "The two sides seem to agree overall.");
    }

    #[test]
    fn test_divergence_check_yes_with_no_body_falls_back_to_full_response() {
        let (divergent, note) = parse_divergence_check("DIVERGENT: YES");
        assert!(divergent);
        assert_eq!(note, "DIVERGENT: YES");
    }

    // ==================== parse_decomposition_request Tests ====================

    #[test]
    fn test_decomposition_request_found_near_top() {
        let text = "DECOMPOSE_REQUEST: split caching from retry policy\n\nRest of rebuttal...";
        assert_eq!(
            parse_decomposition_request(text),
            Some("split caching from retry policy".to_string())
        );
    }

    #[test]
    fn test_decomposition_request_absent_returns_none() {
        assert_eq!(
            parse_decomposition_request("CLAIM: the cache never expires"),
            None
        );
    }

    #[test]
    fn test_decomposition_request_case_insensitive() {
        let text = "decompose_request: split the question";
        assert_eq!(
            parse_decomposition_request(text),
            Some("split the question".to_string())
        );
    }

    #[test]
    fn test_decomposition_request_ignored_when_not_near_top() {
        // The label only appears deep in the body — not a genuine request,
        // just incidental text (e.g. quoting instructions back).
        let mut text = String::new();
        for i in 0..10 {
            text.push_str(&format!("filler line {i}\n"));
        }
        text.push_str("DECOMPOSE_REQUEST: buried mention\n");
        assert_eq!(parse_decomposition_request(&text), None);
    }

    #[test]
    fn test_decomposition_request_empty_target_returns_none() {
        assert_eq!(parse_decomposition_request("DECOMPOSE_REQUEST:   "), None);
    }

    #[test]
    fn test_decomposition_request_multibyte_line_before_label_does_not_panic() {
        // Regression: see test_opponent_rebuttals_multibyte_line_before_label_does_not_panic.
        let text = "a主張の分解を要求します\nDECOMPOSE_REQUEST: この複合主張";
        assert_eq!(
            parse_decomposition_request(text),
            Some("この複合主張".to_string())
        );
    }
}
