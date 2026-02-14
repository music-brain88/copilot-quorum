//! Resource reference extraction and types.
//!
//! Extracts references to GitHub Issues, Pull Requests, and other resources
//! from text. Used by the context gathering phase to automatically resolve
//! referenced resources.

use std::collections::HashSet;
use std::fmt;

/// A reference to an external resource found in text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceReference {
    /// A GitHub Issue reference (e.g., `#123`, `owner/repo#123`)
    GitHubIssue { repo: Option<String>, number: u64 },
    /// A GitHub Pull Request reference (e.g., `PR #123`, GitHub PR URL)
    GitHubPullRequest { repo: Option<String>, number: u64 },
}

impl ResourceReference {
    /// Human-readable label for this reference.
    pub fn label(&self) -> String {
        match self {
            ResourceReference::GitHubIssue {
                repo: Some(r),
                number,
            } => {
                format!("Issue {}#{}", r, number)
            }
            ResourceReference::GitHubIssue { repo: None, number } => {
                format!("Issue #{}", number)
            }
            ResourceReference::GitHubPullRequest {
                repo: Some(r),
                number,
            } => {
                format!("PR {}#{}", r, number)
            }
            ResourceReference::GitHubPullRequest { repo: None, number } => {
                format!("PR #{}", number)
            }
        }
    }

    /// The issue/PR number.
    pub fn number(&self) -> u64 {
        match self {
            ResourceReference::GitHubIssue { number, .. }
            | ResourceReference::GitHubPullRequest { number, .. } => *number,
        }
    }

    /// The optional repository (owner/repo).
    pub fn repo(&self) -> Option<&str> {
        match self {
            ResourceReference::GitHubIssue { repo, .. }
            | ResourceReference::GitHubPullRequest { repo, .. } => repo.as_deref(),
        }
    }
}

impl fmt::Display for ResourceReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Extract resource references from text.
///
/// Recognizes (in specificity order):
/// 1. GitHub URLs: `github.com/{owner}/{repo}/(issues|pull)/{N}`
/// 2. Cross-repo refs: `{owner}/{repo}#{N}`
/// 3. Typed explicit: `Issue #N`, `PR #N`, `Pull Request #N`
/// 4. Range refs: `#N-M` (M-N <= 10)
/// 5. Bare refs: `#N`
///
/// `Discussion #N` is skipped (Phase 1 scope).
/// Results are deduplicated.
pub fn extract_references(text: &str) -> Vec<ResourceReference> {
    let mut seen = HashSet::new();
    // Track byte positions that have already been matched (to avoid double-matching)
    let mut matched_positions: Vec<(usize, usize)> = Vec::new();

    let is_matched = |pos: usize, matched: &[(usize, usize)]| -> bool {
        matched
            .iter()
            .any(|(start, end)| pos >= *start && pos < *end)
    };

    // === Pattern 1: GitHub URLs ===
    // github.com/{owner}/{repo}/(issues|pull)/{N}
    let github_prefix = "github.com/";
    let mut search_from = 0;
    while let Some(idx) = text[search_from..].find(github_prefix) {
        let abs_idx = search_from + idx;
        let after_prefix = abs_idx + github_prefix.len();
        if let Some(parsed) = parse_github_url(&text[after_prefix..]) {
            let end = after_prefix + parsed.consumed;
            matched_positions.push((abs_idx, end));
            seen.insert(parsed.reference);
            search_from = end;
        } else {
            search_from = after_prefix;
        }
    }

    // === Patterns 2-5: scan character by character ===
    let chars: Vec<char> = text.chars().collect();
    let mut char_idx = 0;
    let mut byte_idx = 0;

    while char_idx < chars.len() {
        let ch = chars[char_idx];
        let ch_len = ch.len_utf8();

        // Skip positions already matched by URL pattern
        if is_matched(byte_idx, &matched_positions) {
            byte_idx += ch_len;
            char_idx += 1;
            continue;
        }

        // === Pattern 2: Cross-repo `owner/repo#N` ===
        // Look for `{owner}/{repo}#{N}` — owner/repo must be alphanumeric + hyphens
        if (ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
            && let Some(parsed) = try_parse_cross_repo(&chars, char_idx, byte_idx, text)
                && !is_matched(byte_idx, &matched_positions) {
                    matched_positions.push((parsed.start_byte, parsed.end_byte));
                    seen.insert(parsed.reference);
                    byte_idx = parsed.end_byte;
                    char_idx = parsed.end_char;
                    continue;
                }

        // === Pattern 3: Typed explicit (Issue #N, PR #N, Pull Request #N) ===
        if (ch == 'I' || ch == 'i') && char_idx + 6 < chars.len()
            && let Some(parsed) = try_parse_typed_issue(&chars, char_idx, byte_idx, text) {
                matched_positions.push((parsed.start_byte, parsed.end_byte));
                seen.insert(parsed.reference);
                byte_idx = parsed.end_byte;
                char_idx = parsed.end_char;
                continue;
            }
        if (ch == 'P' || ch == 'p') && char_idx + 3 < chars.len()
            && let Some(parsed) = try_parse_typed_pr(&chars, char_idx, byte_idx, text) {
                matched_positions.push((parsed.start_byte, parsed.end_byte));
                seen.insert(parsed.reference);
                byte_idx = parsed.end_byte;
                char_idx = parsed.end_char;
                continue;
            }

        // === Skip Discussion #N ===
        if (ch == 'D' || ch == 'd') && char_idx + 11 < chars.len()
            && matches_word_ci(&chars, char_idx, "Discussion") {
                // Skip past "Discussion" and any following "#N"
                let skip_len = "Discussion".len();
                let mut skip_char = char_idx + skip_len;
                let mut skip_byte = byte_idx;
                for c in "Discussion".chars() {
                    skip_byte += c.len_utf8();
                }
                // skip whitespace
                while skip_char < chars.len() && chars[skip_char] == ' ' {
                    skip_byte += 1;
                    skip_char += 1;
                }
                // skip #N
                if skip_char < chars.len() && chars[skip_char] == '#' {
                    skip_byte += 1;
                    skip_char += 1;
                    while skip_char < chars.len() && chars[skip_char].is_ascii_digit() {
                        skip_byte += chars[skip_char].len_utf8();
                        skip_char += 1;
                    }
                    matched_positions.push((byte_idx, skip_byte));
                    byte_idx = skip_byte;
                    char_idx = skip_char;
                    continue;
                }
            }

        // === Pattern 4 & 5: Bare #N or range #N-M ===
        if ch == '#' && !is_matched(byte_idx, &matched_positions)
            && let Some(parsed) = try_parse_bare_ref(&chars, char_idx, byte_idx) {
                for r in parsed.references {
                    seen.insert(r);
                }
                matched_positions.push((parsed.start_byte, parsed.end_byte));
                byte_idx = parsed.end_byte;
                char_idx = parsed.end_char;
                continue;
            }

        byte_idx += ch_len;
        char_idx += 1;
    }

    seen.into_iter().collect()
}

// --- Internal parsing helpers ---

struct ParsedUrl {
    reference: ResourceReference,
    consumed: usize,
}

/// Parse a GitHub URL path after `github.com/`
/// Expected: `{owner}/{repo}/(issues|pull)/{N}`
fn parse_github_url(after_prefix: &str) -> Option<ParsedUrl> {
    let parts: Vec<&str> = after_prefix.splitn(5, '/').collect();
    if parts.len() < 4 {
        return None;
    }

    let owner = parts[0];
    let repo = parts[1];
    let kind = parts[2];
    let num_str = parts[3];

    if owner.is_empty() || repo.is_empty() {
        return None;
    }

    // Extract the number (might have trailing non-digit chars)
    let num_end = num_str
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(num_str.len());
    if num_end == 0 {
        return None;
    }
    let number: u64 = num_str[..num_end].parse().ok()?;
    if number == 0 {
        return None;
    }

    let full_repo = format!("{}/{}", owner, repo);
    let consumed = owner.len() + 1 + repo.len() + 1 + kind.len() + 1 + num_end;

    let reference = match kind {
        "issues" => ResourceReference::GitHubIssue {
            repo: Some(full_repo),
            number,
        },
        "pull" => ResourceReference::GitHubPullRequest {
            repo: Some(full_repo),
            number,
        },
        _ => return None,
    };

    Some(ParsedUrl {
        reference,
        consumed,
    })
}

struct ParsedRef {
    reference: ResourceReference,
    start_byte: usize,
    end_byte: usize,
    end_char: usize,
}

struct ParsedBareRef {
    references: Vec<ResourceReference>,
    start_byte: usize,
    end_byte: usize,
    end_char: usize,
}

/// Try to parse `owner/repo#N` starting at char_idx
fn try_parse_cross_repo(
    chars: &[char],
    start_char: usize,
    start_byte: usize,
    text: &str,
) -> Option<ParsedRef> {
    // Scan backward from current position to find the start of the owner segment
    // Actually, we should scan forward from start_char to find owner/repo#N
    let mut ci = start_char;
    let mut bi = start_byte;

    // Collect owner
    let owner_start = ci;
    while ci < chars.len()
        && (chars[ci].is_ascii_alphanumeric()
            || chars[ci] == '-'
            || chars[ci] == '_'
            || chars[ci] == '.')
    {
        bi += chars[ci].len_utf8();
        ci += 1;
    }
    if ci >= chars.len() || chars[ci] != '/' {
        return None;
    }
    let owner_end = ci;
    if owner_end == owner_start {
        return None;
    }

    // Skip '/'
    bi += 1;
    ci += 1;

    // Collect repo
    let repo_start = ci;
    while ci < chars.len()
        && (chars[ci].is_ascii_alphanumeric()
            || chars[ci] == '-'
            || chars[ci] == '_'
            || chars[ci] == '.')
    {
        bi += chars[ci].len_utf8();
        ci += 1;
    }
    if ci >= chars.len() || chars[ci] != '#' {
        return None;
    }
    let repo_end = ci;
    if repo_end == repo_start {
        return None;
    }

    // Skip '#'
    bi += 1;
    ci += 1;

    // Collect number
    let num_start = ci;
    while ci < chars.len() && chars[ci].is_ascii_digit() {
        bi += chars[ci].len_utf8();
        ci += 1;
    }
    if ci == num_start {
        return None;
    }

    let owner: String = chars[owner_start..owner_end].iter().collect();
    let repo: String = chars[repo_start..repo_end].iter().collect();
    let num_str: String = chars[num_start..ci].iter().collect();
    let number: u64 = num_str.parse().ok()?;
    if number == 0 {
        return None;
    }

    // Validate: owner and repo must look like GitHub identifiers
    // (not paths like "src/main" — require at least one letter)
    if !owner.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }

    // Check that owner doesn't start after another identifier char
    // (to avoid matching mid-path like "foo/bar/repo#1")
    if start_char > 0 {
        let prev = chars[start_char - 1];
        if prev.is_ascii_alphanumeric() || prev == '-' || prev == '_' || prev == '/' {
            // Check if there's a prior '/' that would indicate this is a longer path
            let prefix: String = chars[..start_char].iter().collect();
            if prefix.ends_with('/') {
                return None;
            }
        }
    }

    // Use text slice to get the owner/repo string with correct encoding
    let _ = text; // we already computed it from chars
    let full_repo = format!("{}/{}", owner, repo);

    Some(ParsedRef {
        reference: ResourceReference::GitHubIssue {
            repo: Some(full_repo),
            number,
        },
        start_byte,
        end_byte: bi,
        end_char: ci,
    })
}

/// Try to parse "Issue #N" or "Issue#N" starting at char_idx
fn try_parse_typed_issue(
    chars: &[char],
    start_char: usize,
    start_byte: usize,
    _text: &str,
) -> Option<ParsedRef> {
    if !matches_word_ci(chars, start_char, "Issue") {
        return None;
    }

    let mut ci = start_char + "Issue".len();
    let mut bi = start_byte;
    for c in "Issue".chars() {
        bi += c.len_utf8();
    }

    // Optional whitespace
    while ci < chars.len() && chars[ci] == ' ' {
        bi += 1;
        ci += 1;
    }

    // Must have '#'
    if ci >= chars.len() || chars[ci] != '#' {
        return None;
    }
    bi += 1;
    ci += 1;

    // Collect number
    let num_start = ci;
    while ci < chars.len() && chars[ci].is_ascii_digit() {
        bi += chars[ci].len_utf8();
        ci += 1;
    }
    if ci == num_start {
        return None;
    }

    let num_str: String = chars[num_start..ci].iter().collect();
    let number: u64 = num_str.parse().ok()?;
    if number == 0 {
        return None;
    }

    Some(ParsedRef {
        reference: ResourceReference::GitHubIssue { repo: None, number },
        start_byte,
        end_byte: bi,
        end_char: ci,
    })
}

/// Try to parse "PR #N", "PR#N", "Pull Request #N" starting at char_idx
fn try_parse_typed_pr(
    chars: &[char],
    start_char: usize,
    start_byte: usize,
    _text: &str,
) -> Option<ParsedRef> {
    // Try "Pull Request" first (longer match)
    if matches_word_ci(chars, start_char, "Pull") {
        let mut ci = start_char + "Pull".len();
        let mut bi = start_byte;
        for c in "Pull".chars() {
            bi += c.len_utf8();
        }

        // Require whitespace
        if ci < chars.len() && chars[ci] == ' ' {
            bi += 1;
            ci += 1;

            if matches_word_ci(chars, ci, "Request") {
                for c in "Request".chars() {
                    bi += c.len_utf8();
                }
                ci += "Request".len();

                // Optional whitespace
                while ci < chars.len() && chars[ci] == ' ' {
                    bi += 1;
                    ci += 1;
                }

                if ci < chars.len() && chars[ci] == '#' {
                    bi += 1;
                    ci += 1;

                    let num_start = ci;
                    while ci < chars.len() && chars[ci].is_ascii_digit() {
                        bi += chars[ci].len_utf8();
                        ci += 1;
                    }
                    if ci > num_start {
                        let num_str: String = chars[num_start..ci].iter().collect();
                        if let Ok(number) = num_str.parse::<u64>()
                            && number > 0 {
                                return Some(ParsedRef {
                                    reference: ResourceReference::GitHubPullRequest {
                                        repo: None,
                                        number,
                                    },
                                    start_byte,
                                    end_byte: bi,
                                    end_char: ci,
                                });
                            }
                    }
                }
            }
        }
    }

    // Try "PR #N"
    if matches_word_ci(chars, start_char, "PR") {
        let mut ci = start_char + "PR".len();
        let mut bi = start_byte;
        for c in "PR".chars() {
            bi += c.len_utf8();
        }

        // PR must not be followed by alphanumeric (avoid matching "PROCESS" etc.)
        if ci < chars.len() && chars[ci].is_ascii_alphanumeric() {
            return None;
        }

        // Optional whitespace
        while ci < chars.len() && chars[ci] == ' ' {
            bi += 1;
            ci += 1;
        }

        if ci < chars.len() && chars[ci] == '#' {
            bi += 1;
            ci += 1;

            let num_start = ci;
            while ci < chars.len() && chars[ci].is_ascii_digit() {
                bi += chars[ci].len_utf8();
                ci += 1;
            }
            if ci > num_start {
                let num_str: String = chars[num_start..ci].iter().collect();
                if let Ok(number) = num_str.parse::<u64>()
                    && number > 0 {
                        return Some(ParsedRef {
                            reference: ResourceReference::GitHubPullRequest { repo: None, number },
                            start_byte,
                            end_byte: bi,
                            end_char: ci,
                        });
                    }
            }
        }
    }

    None
}

/// Try to parse bare `#N` or range `#N-M` starting at char_idx (where chars[char_idx] == '#')
fn try_parse_bare_ref(
    chars: &[char],
    start_char: usize,
    start_byte: usize,
) -> Option<ParsedBareRef> {
    debug_assert_eq!(chars[start_char], '#');

    let mut ci = start_char + 1;
    let mut bi = start_byte + 1; // '#' is 1 byte

    // Collect first number
    let num_start = ci;
    while ci < chars.len() && chars[ci].is_ascii_digit() {
        bi += chars[ci].len_utf8();
        ci += 1;
    }
    if ci == num_start {
        return None;
    }

    let num_str: String = chars[num_start..ci].iter().collect();
    let first_num: u64 = num_str.parse().ok()?;
    if first_num == 0 {
        return None;
    }

    // Check for range: #N-M
    if ci < chars.len() && chars[ci] == '-' {
        let dash_bi = bi;
        let dash_ci = ci;
        bi += 1;
        ci += 1;

        let range_num_start = ci;
        while ci < chars.len() && chars[ci].is_ascii_digit() {
            bi += chars[ci].len_utf8();
            ci += 1;
        }

        if ci > range_num_start {
            let range_str: String = chars[range_num_start..ci].iter().collect();
            if let Ok(second_num) = range_str.parse::<u64>()
                && second_num > first_num && (second_num - first_num) <= 10 {
                    // Valid range
                    let refs: Vec<ResourceReference> = (first_num..=second_num)
                        .map(|n| ResourceReference::GitHubIssue {
                            repo: None,
                            number: n,
                        })
                        .collect();
                    return Some(ParsedBareRef {
                        references: refs,
                        start_byte,
                        end_byte: bi,
                        end_char: ci,
                    });
                }
        }

        // Not a valid range — fall back to single ref (reset past dash)
        bi = dash_bi;
        ci = dash_ci;
    }

    Some(ParsedBareRef {
        references: vec![ResourceReference::GitHubIssue {
            repo: None,
            number: first_num,
        }],
        start_byte,
        end_byte: bi,
        end_char: ci,
    })
}

/// Case-insensitive word match at position
fn matches_word_ci(chars: &[char], start: usize, word: &str) -> bool {
    let word_chars: Vec<char> = word.chars().collect();
    if start + word_chars.len() > chars.len() {
        return false;
    }
    for (i, wc) in word_chars.iter().enumerate() {
        if !chars[start + i].eq_ignore_ascii_case(wc) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut refs: Vec<ResourceReference>) -> Vec<ResourceReference> {
        refs.sort_by(|a, b| {
            let a_key = (a.repo().unwrap_or("").to_string(), a.number());
            let b_key = (b.repo().unwrap_or("").to_string(), b.number());
            a_key.cmp(&b_key)
        });
        refs
    }

    #[test]
    fn test_bare_ref() {
        let refs = extract_references("See #123 for details");
        assert_eq!(refs.len(), 1);
        assert!(refs.contains(&ResourceReference::GitHubIssue {
            repo: None,
            number: 123,
        }));
    }

    #[test]
    fn test_typed_issue_ref() {
        let refs = extract_references("Fixes Issue #42");
        assert_eq!(refs.len(), 1);
        assert!(refs.contains(&ResourceReference::GitHubIssue {
            repo: None,
            number: 42,
        }));
    }

    #[test]
    fn test_typed_issue_case_insensitive() {
        let refs = extract_references("See issue #99");
        assert_eq!(refs.len(), 1);
        assert!(refs.contains(&ResourceReference::GitHubIssue {
            repo: None,
            number: 99,
        }));
    }

    #[test]
    fn test_typed_pr_ref() {
        let refs = extract_references("Related to PR #55");
        assert_eq!(refs.len(), 1);
        assert!(refs.contains(&ResourceReference::GitHubPullRequest {
            repo: None,
            number: 55,
        }));
    }

    #[test]
    fn test_pull_request_ref() {
        let refs = extract_references("See Pull Request #78");
        assert_eq!(refs.len(), 1);
        assert!(refs.contains(&ResourceReference::GitHubPullRequest {
            repo: None,
            number: 78,
        }));
    }

    #[test]
    fn test_range_expansion() {
        let refs = sorted(extract_references("Issues #10-13"));
        assert_eq!(refs.len(), 4);
        for n in 10..=13 {
            assert!(refs.contains(&ResourceReference::GitHubIssue {
                repo: None,
                number: n,
            }));
        }
    }

    #[test]
    fn test_range_too_large_is_single() {
        // #1-100 range is > 10, so should be treated as #1 only
        let refs = extract_references("#1-100");
        assert_eq!(refs.len(), 1);
        assert!(refs.contains(&ResourceReference::GitHubIssue {
            repo: None,
            number: 1,
        }));
    }

    #[test]
    fn test_github_issue_url() {
        let refs = extract_references("See https://github.com/owner/repo/issues/456 for context");
        assert_eq!(refs.len(), 1);
        assert!(refs.contains(&ResourceReference::GitHubIssue {
            repo: Some("owner/repo".to_string()),
            number: 456,
        }));
    }

    #[test]
    fn test_github_pr_url() {
        let refs = extract_references("Check https://github.com/foo/bar/pull/789");
        assert_eq!(refs.len(), 1);
        assert!(refs.contains(&ResourceReference::GitHubPullRequest {
            repo: Some("foo/bar".to_string()),
            number: 789,
        }));
    }

    #[test]
    fn test_cross_repo_ref() {
        let refs = extract_references("See owner/other-repo#321");
        assert_eq!(refs.len(), 1);
        assert!(refs.contains(&ResourceReference::GitHubIssue {
            repo: Some("owner/other-repo".to_string()),
            number: 321,
        }));
    }

    #[test]
    fn test_dedup() {
        let refs = extract_references("#42 and also #42 again");
        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn test_empty_text() {
        let refs = extract_references("");
        assert!(refs.is_empty());
    }

    #[test]
    fn test_no_refs() {
        let refs = extract_references("Just some regular text without any references.");
        assert!(refs.is_empty());
    }

    #[test]
    fn test_discussion_skip() {
        let refs = extract_references("See Discussion #43 and #100");
        assert_eq!(refs.len(), 1);
        assert!(refs.contains(&ResourceReference::GitHubIssue {
            repo: None,
            number: 100,
        }));
    }

    #[test]
    fn test_label_format() {
        let issue = ResourceReference::GitHubIssue {
            repo: None,
            number: 42,
        };
        assert_eq!(issue.label(), "Issue #42");
        assert_eq!(format!("{}", issue), "Issue #42");

        let pr = ResourceReference::GitHubPullRequest {
            repo: Some("owner/repo".to_string()),
            number: 99,
        };
        assert_eq!(pr.label(), "PR owner/repo#99");

        let cross_issue = ResourceReference::GitHubIssue {
            repo: Some("foo/bar".to_string()),
            number: 10,
        };
        assert_eq!(cross_issue.label(), "Issue foo/bar#10");
    }

    #[test]
    fn test_multiple_refs_mixed() {
        let text = "Fixes #123, see Issue #456 and PR #789. Also check https://github.com/org/lib/issues/1";
        let refs = sorted(extract_references(text));
        assert_eq!(refs.len(), 4);
    }

    #[test]
    fn test_zero_number_ignored() {
        let refs = extract_references("#0");
        assert!(refs.is_empty());
    }
}
