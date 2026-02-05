//! Quorum rules for consensus determination
//!
//! This module defines the rules used to determine whether a Quorum vote passes.

use serde::{Deserialize, Serialize};

/// Rule for determining Quorum consensus
///
/// Different rules can be used depending on the criticality of the decision:
/// - `Majority`: More than half must approve (default)
/// - `Unanimous`: All must approve (strictest)
/// - `AtLeast(n)`: At least n votes must approve
/// - `Percentage(p)`: At least p% must approve
///
/// # Example
///
/// ```
/// use quorum_domain::quorum::QuorumRule;
///
/// let rule = QuorumRule::Majority;
/// assert!(rule.is_satisfied(2, 3));  // 2/3 > 50%
/// assert!(!rule.is_satisfied(1, 3)); // 1/3 < 50%
///
/// let strict = QuorumRule::Unanimous;
/// assert!(strict.is_satisfied(3, 3));  // 3/3 = 100%
/// assert!(!strict.is_satisfied(2, 3)); // 2/3 < 100%
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum QuorumRule {
    /// More than half must approve (n/2 + 1)
    #[default]
    Majority,

    /// All participants must approve
    Unanimous,

    /// At least n votes must approve
    AtLeast(usize),

    /// At least this percentage must approve (0-100)
    Percentage(u8),
}

impl QuorumRule {
    /// Check if the rule is satisfied given approval count and total votes
    pub fn is_satisfied(&self, approvals: usize, total: usize) -> bool {
        if total == 0 {
            return false;
        }

        match self {
            QuorumRule::Majority => approvals > total / 2,
            QuorumRule::Unanimous => approvals == total,
            QuorumRule::AtLeast(n) => approvals >= *n,
            QuorumRule::Percentage(p) => {
                let required = (total as f64 * (*p as f64 / 100.0)).ceil() as usize;
                approvals >= required
            }
        }
    }

    /// Get a human-readable description of this rule
    pub fn description(&self) -> String {
        match self {
            QuorumRule::Majority => "majority (more than half)".to_string(),
            QuorumRule::Unanimous => "unanimous (all must approve)".to_string(),
            QuorumRule::AtLeast(n) => format!("at least {} approvals", n),
            QuorumRule::Percentage(p) => format!("at least {}% approval", p),
        }
    }

    /// Get the minimum approvals needed for this rule given a total count
    pub fn min_approvals_needed(&self, total: usize) -> usize {
        match self {
            QuorumRule::Majority => total / 2 + 1,
            QuorumRule::Unanimous => total,
            QuorumRule::AtLeast(n) => *n,
            QuorumRule::Percentage(p) => (total as f64 * (*p as f64 / 100.0)).ceil() as usize,
        }
    }
}

impl std::fmt::Display for QuorumRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl std::str::FromStr for QuorumRule {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "majority" => Ok(QuorumRule::Majority),
            "unanimous" => Ok(QuorumRule::Unanimous),
            s if s.starts_with("atleast:") || s.starts_with("at_least:") => {
                let n: usize = s
                    .split(':')
                    .nth(1)
                    .ok_or("Missing number after atleast:")?
                    .parse()
                    .map_err(|_| "Invalid number for atleast")?;
                Ok(QuorumRule::AtLeast(n))
            }
            s if s.starts_with("percentage:") || s.ends_with('%') => {
                let num_str = s.trim_start_matches("percentage:").trim_end_matches('%');
                let p: u8 = num_str.parse().map_err(|_| "Invalid percentage")?;
                Ok(QuorumRule::Percentage(p))
            }
            _ => Err(format!(
                "Unknown quorum rule: {}. Valid: majority, unanimous, atleast:N, percentage:N or N%",
                s
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_majority_rule() {
        let rule = QuorumRule::Majority;

        // 3 total: need > 1.5, so 2 approvals
        assert!(!rule.is_satisfied(1, 3));
        assert!(rule.is_satisfied(2, 3));
        assert!(rule.is_satisfied(3, 3));

        // 4 total: need > 2, so 3 approvals
        assert!(!rule.is_satisfied(2, 4));
        assert!(rule.is_satisfied(3, 4));
    }

    #[test]
    fn test_unanimous_rule() {
        let rule = QuorumRule::Unanimous;

        assert!(!rule.is_satisfied(2, 3));
        assert!(rule.is_satisfied(3, 3));
        assert!(rule.is_satisfied(1, 1));
    }

    #[test]
    fn test_at_least_rule() {
        let rule = QuorumRule::AtLeast(2);

        assert!(!rule.is_satisfied(1, 5));
        assert!(rule.is_satisfied(2, 5));
        assert!(rule.is_satisfied(5, 5));
    }

    #[test]
    fn test_percentage_rule() {
        let rule = QuorumRule::Percentage(75);

        // 4 total: need 75% = 3
        assert!(!rule.is_satisfied(2, 4));
        assert!(rule.is_satisfied(3, 4));

        // 5 total: need 75% = ceil(3.75) = 4
        assert!(!rule.is_satisfied(3, 5));
        assert!(rule.is_satisfied(4, 5));
    }

    #[test]
    fn test_zero_total() {
        // All rules should return false for zero total
        assert!(!QuorumRule::Majority.is_satisfied(0, 0));
        assert!(!QuorumRule::Unanimous.is_satisfied(0, 0));
        assert!(!QuorumRule::AtLeast(1).is_satisfied(0, 0));
        assert!(!QuorumRule::Percentage(50).is_satisfied(0, 0));
    }

    #[test]
    fn test_min_approvals_needed() {
        assert_eq!(QuorumRule::Majority.min_approvals_needed(3), 2);
        assert_eq!(QuorumRule::Majority.min_approvals_needed(4), 3);
        assert_eq!(QuorumRule::Unanimous.min_approvals_needed(3), 3);
        assert_eq!(QuorumRule::AtLeast(2).min_approvals_needed(5), 2);
        assert_eq!(QuorumRule::Percentage(75).min_approvals_needed(4), 3);
    }

    #[test]
    fn test_parse_rule() {
        assert_eq!(
            "majority".parse::<QuorumRule>().ok(),
            Some(QuorumRule::Majority)
        );
        assert_eq!(
            "unanimous".parse::<QuorumRule>().ok(),
            Some(QuorumRule::Unanimous)
        );
        assert_eq!(
            "atleast:2".parse::<QuorumRule>().ok(),
            Some(QuorumRule::AtLeast(2))
        );
        assert_eq!(
            "at_least:3".parse::<QuorumRule>().ok(),
            Some(QuorumRule::AtLeast(3))
        );
        assert_eq!(
            "percentage:75".parse::<QuorumRule>().ok(),
            Some(QuorumRule::Percentage(75))
        );
        assert_eq!(
            "80%".parse::<QuorumRule>().ok(),
            Some(QuorumRule::Percentage(80))
        );
    }

    #[test]
    fn test_display() {
        assert_eq!(
            QuorumRule::Majority.to_string(),
            "majority (more than half)"
        );
        assert_eq!(
            QuorumRule::Unanimous.to_string(),
            "unanimous (all must approve)"
        );
    }

    #[test]
    fn test_default() {
        assert_eq!(QuorumRule::default(), QuorumRule::Majority);
    }
}
