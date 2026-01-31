//! Question value object

use serde::{Deserialize, Serialize};

/// A question to be answered by the Quorum (Value Object)
///
/// Represents the input query that will be sent to multiple models
/// for collaborative discussion and synthesis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    content: String,
}

impl Question {
    /// Create a new question
    ///
    /// # Panics
    /// Panics if the content is empty or only whitespace
    pub fn new(content: impl Into<String>) -> Self {
        let content = content.into();
        assert!(!content.trim().is_empty(), "Question cannot be empty");
        Self { content }
    }

    /// Try to create a new question, returning None if invalid
    pub fn try_new(content: impl Into<String>) -> Option<Self> {
        let content = content.into();
        if content.trim().is_empty() {
            None
        } else {
            Some(Self { content })
        }
    }

    /// Get the question content
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Consume and return the inner content
    pub fn into_content(self) -> String {
        self.content
    }
}

impl std::fmt::Display for Question {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.content)
    }
}

impl From<&str> for Question {
    fn from(s: &str) -> Self {
        Question::new(s)
    }
}

impl From<String> for Question {
    fn from(s: String) -> Self {
        Question::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_question_creation() {
        let q = Question::new("What is Rust?");
        assert_eq!(q.content(), "What is Rust?");
    }

    #[test]
    fn test_question_from_str() {
        let q: Question = "What is Rust?".into();
        assert_eq!(q.content(), "What is Rust?");
    }

    #[test]
    #[should_panic]
    fn test_empty_question_panics() {
        Question::new("");
    }

    #[test]
    fn test_try_new_empty() {
        assert!(Question::try_new("").is_none());
        assert!(Question::try_new("   ").is_none());
    }

    #[test]
    fn test_try_new_valid() {
        assert!(Question::try_new("What is Rust?").is_some());
    }
}
