//! Prompt templates for the Quorum flow

/// Templates for generating prompts at each stage
pub struct PromptTemplate;

impl PromptTemplate {
    /// System prompt for initial query phase
    pub fn initial_system() -> &'static str {
        r#"You are a knowledgeable expert participating in a collaborative discussion.
Your task is to provide a thoughtful, well-reasoned response to the question.
Be concise but comprehensive. Support your points with reasoning and examples where appropriate.
Focus on accuracy and clarity."#
    }

    /// User prompt for initial query
    pub fn initial_query(question: &str) -> String {
        format!(
            r#"Please answer the following question:

{}

Provide a clear, well-structured response."#,
            question
        )
    }

    /// System prompt for peer review phase
    pub fn review_system() -> &'static str {
        r#"You are a critical reviewer evaluating responses from other experts.
Your task is to objectively assess the quality, accuracy, and completeness of responses.
Be fair but thorough in your evaluation. Identify both strengths and weaknesses.
Provide constructive feedback that would help improve the response."#
    }

    /// User prompt for peer review
    pub fn review_prompt(question: &str, responses: &[(String, String)]) -> String {
        let mut prompt = format!(
            r#"Original question: {}

Please review the following responses from other experts. Evaluate each response for:
1. Accuracy and correctness
2. Completeness
3. Clarity and organization
4. Practical usefulness

Responses to review:
"#,
            question
        );

        for (id, content) in responses {
            prompt.push_str(&format!("\n--- {} ---\n{}\n", id, content));
        }

        prompt.push_str(
            r#"
For each response, provide:
1. A brief assessment (2-3 sentences)
2. Key strengths
3. Areas for improvement
4. A score from 1-10

Format your review clearly with headers for each response."#,
        );

        prompt
    }

    /// System prompt for synthesis phase
    pub fn synthesis_system() -> &'static str {
        r#"You are a moderator synthesizing multiple expert opinions into a coherent conclusion.
Your task is to:
1. Identify areas of consensus among the responses
2. Note significant disagreements and evaluate which positions are better supported
3. Synthesize the best elements into a comprehensive final answer
4. Highlight key insights that emerged from the discussion

Be balanced and objective. Give weight to well-reasoned arguments regardless of source."#
    }

    /// User prompt for synthesis
    pub fn synthesis_prompt(
        question: &str,
        responses: &[(String, String)],
        reviews: &[(String, String)],
    ) -> String {
        let mut prompt = format!(
            r#"Original question: {}

Expert responses:
"#,
            question
        );

        for (model, content) in responses {
            prompt.push_str(&format!("\n--- {} ---\n{}\n", model, content));
        }

        if !reviews.is_empty() {
            prompt.push_str("\nPeer reviews:\n");

            for (reviewer, review) in reviews {
                prompt.push_str(&format!("\n--- Review by {} ---\n{}\n", reviewer, review));
            }
        }

        prompt.push_str(
            r#"
Based on all responses and reviews above, please provide:

1. **Conclusion**: A synthesized answer that incorporates the strongest elements from all responses

2. **Key Points**: The most important points that emerged (bullet list)

3. **Consensus**: Areas where experts agreed (bullet list)

4. **Disagreements**: Significant disagreements and your assessment of which position is better supported (bullet list)

Format your response with clear markdown headers."#,
        );

        prompt
    }

    /// User prompt for synthesis when there are no reviews
    pub fn synthesis_prompt_no_reviews(
        question: &str,
        responses: &[(String, String)],
    ) -> String {
        Self::synthesis_prompt(question, responses, &[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_query_format() {
        let question = "What is Rust?";
        let prompt = PromptTemplate::initial_query(question);
        assert!(prompt.contains(question));
    }

    #[test]
    fn test_review_prompt_format() {
        let question = "What is Rust?";
        let responses = vec![
            ("Response A".to_string(), "Rust is a systems programming language.".to_string()),
            ("Response B".to_string(), "Rust focuses on safety and performance.".to_string()),
        ];
        let prompt = PromptTemplate::review_prompt(question, &responses);
        assert!(prompt.contains("Response A"));
        assert!(prompt.contains("Response B"));
        assert!(prompt.contains("systems programming"));
    }

    #[test]
    fn test_synthesis_prompt_format() {
        let question = "What is Rust?";
        let responses = vec![
            ("GPT-4".to_string(), "Rust is a systems language.".to_string()),
        ];
        let reviews = vec![
            ("Claude".to_string(), "Good response, accurate.".to_string()),
        ];
        let prompt = PromptTemplate::synthesis_prompt(question, &responses, &reviews);
        assert!(prompt.contains("GPT-4"));
        assert!(prompt.contains("Claude"));
        assert!(prompt.contains("Conclusion"));
    }

    #[test]
    fn test_synthesis_without_reviews() {
        let question = "What is Rust?";
        let responses = vec![
            ("GPT-4".to_string(), "Rust is a systems language.".to_string()),
        ];
        let prompt = PromptTemplate::synthesis_prompt_no_reviews(question, &responses);
        assert!(prompt.contains("GPT-4"));
        assert!(!prompt.contains("Peer reviews:"));
    }
}
