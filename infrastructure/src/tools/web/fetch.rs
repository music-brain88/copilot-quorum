//! web_fetch tool: Fetch a URL and extract text content

use quorum_domain::tool::{
    entities::{RiskLevel, ToolCall, ToolDefinition, ToolParameter},
    value_objects::{ToolError, ToolResult, ToolResultMetadata},
};
use std::time::Instant;

/// Tool name constant
pub const WEB_FETCH: &str = "web_fetch";

/// Maximum response body size (5 MB)
const MAX_BODY_SIZE: usize = 5 * 1024 * 1024;

/// Default max output text size (50 KB)
const DEFAULT_MAX_TEXT: usize = 50 * 1024;

/// Get the tool definition for web_fetch
pub fn web_fetch_definition() -> ToolDefinition {
    ToolDefinition::new(
        WEB_FETCH,
        "Fetch a web page and extract its text content. Returns the readable text from the page.",
        RiskLevel::Low,
    )
    .with_parameter(
        ToolParameter::new("url", "The URL to fetch", true).with_type("string"),
    )
    .with_parameter(
        ToolParameter::new(
            "max_length",
            "Maximum length of extracted text in bytes (default: 51200)",
            false,
        )
        .with_type("number"),
    )
}

/// Execute the web_fetch tool
pub async fn execute_web_fetch(
    client: &reqwest::Client,
    call: &ToolCall,
) -> ToolResult {
    let start = Instant::now();

    let url = match call.require_string("url") {
        Ok(u) => u,
        Err(e) => {
            return ToolResult::failure(WEB_FETCH, ToolError::invalid_argument(e));
        }
    };

    let max_length = call
        .get_i64("max_length")
        .map(|v| v as usize)
        .unwrap_or(DEFAULT_MAX_TEXT);

    // Fetch the URL
    let response = match client
        .get(url)
        .header("User-Agent", "CopilotQuorum/0.1 (Agent Tool)")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return ToolResult::failure(
                WEB_FETCH,
                ToolError::execution_failed(format!("Failed to fetch URL: {}", e)),
            );
        }
    };

    let status = response.status();
    if !status.is_success() {
        return ToolResult::failure(
            WEB_FETCH,
            ToolError::execution_failed(format!(
                "HTTP error: {} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown")
            )),
        );
    }

    // Check content length
    let content_length = response.content_length().unwrap_or(0);
    if content_length > MAX_BODY_SIZE as u64 {
        return ToolResult::failure(
            WEB_FETCH,
            ToolError::execution_failed(format!(
                "Response too large: {} bytes (max: {} bytes)",
                content_length, MAX_BODY_SIZE
            )),
        );
    }

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Read body
    let body = match response.bytes().await {
        Ok(b) => {
            if b.len() > MAX_BODY_SIZE {
                return ToolResult::failure(
                    WEB_FETCH,
                    ToolError::execution_failed(format!(
                        "Response too large: {} bytes",
                        b.len()
                    )),
                );
            }
            b
        }
        Err(e) => {
            return ToolResult::failure(
                WEB_FETCH,
                ToolError::execution_failed(format!("Failed to read response body: {}", e)),
            );
        }
    };

    let body_str = String::from_utf8_lossy(&body);

    // Extract text based on content type
    let text = if content_type.contains("text/html") || content_type.contains("application/xhtml")
    {
        html_to_text(&body_str)
    } else {
        // For non-HTML content (JSON, plain text, etc.), return as-is
        body_str.to_string()
    };

    // Truncate if needed
    let (output, truncated) = if text.len() > max_length {
        let truncated_text = &text[..text.floor_char_boundary(max_length)];
        (
            format!(
                "{}\n\n[... truncated at {} bytes, total: {} bytes]",
                truncated_text,
                max_length,
                text.len()
            ),
            true,
        )
    } else {
        (text.clone(), false)
    };

    let elapsed = start.elapsed();

    let mut result = ToolResult::success(
        WEB_FETCH,
        format!(
            "## Fetched: {}\n\nStatus: {} | Content-Type: {} | Size: {} bytes{}\n\n{}",
            url,
            status.as_u16(),
            content_type,
            text.len(),
            if truncated { " (truncated)" } else { "" },
            output,
        ),
    );
    result.metadata = ToolResultMetadata {
        duration_ms: Some(elapsed.as_millis() as u64),
        bytes: Some(body.len()),
        ..Default::default()
    };
    result
}

/// Extract readable text from HTML, stripping tags, scripts, and styles
pub fn html_to_text(html: &str) -> String {
    use scraper::{Html, Selector};

    let document = Html::parse_document(html);

    // Tags whose entire subtree should be ignored
    let skip_tags = ["script", "style", "noscript", "svg"];

    // Try to use <body>, fall back to the whole document
    let body_selector = Selector::parse("body").unwrap();
    let root = document
        .select(&body_selector)
        .next();

    let mut text_parts: Vec<String> = Vec::new();

    // Collect text from all elements, skipping script/style/etc.
    let elements = if let Some(body) = root {
        collect_element_text(body, &skip_tags)
    } else {
        // No body found, use root element
        collect_element_text(document.root_element(), &skip_tags)
    };

    text_parts.extend(elements);

    // Join and clean up whitespace
    let raw = text_parts.join(" ");
    clean_whitespace(&raw)
}

/// Recursively collect text from an element, skipping elements matching skip_tags
fn collect_element_text(element: scraper::ElementRef, skip_tags: &[&str]) -> Vec<String> {
    let tag_name = element.value().name();
    if skip_tags.contains(&tag_name) {
        return Vec::new();
    }

    let mut parts = Vec::new();

    for child in element.children() {
        match child.value() {
            scraper::Node::Text(text) => {
                let t = text.trim();
                if !t.is_empty() {
                    parts.push(t.to_string());
                }
            }
            scraper::Node::Element(_) => {
                if let Some(child_el) = scraper::ElementRef::wrap(child) {
                    parts.extend(collect_element_text(child_el, skip_tags));
                }
            }
            _ => {}
        }
    }

    parts
}

/// Clean up excessive whitespace
fn clean_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_was_whitespace = false;
    let mut newline_count = 0;

    for ch in text.chars() {
        if ch == '\n' {
            newline_count += 1;
            if newline_count <= 2 {
                result.push('\n');
            }
            prev_was_whitespace = true;
        } else if ch.is_whitespace() {
            if !prev_was_whitespace {
                result.push(' ');
            }
            prev_was_whitespace = true;
            newline_count = 0;
        } else {
            result.push(ch);
            prev_was_whitespace = false;
            newline_count = 0;
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_to_text_basic() {
        let html = "<html><body><h1>Hello</h1><p>World</p></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }

    #[test]
    fn test_html_to_text_strips_script_and_style() {
        let html = r#"
        <html><body>
            <script>var x = 1;</script>
            <style>.foo { color: red; }</style>
            <p>Visible text</p>
            <noscript>No JS</noscript>
        </body></html>
        "#;
        let text = html_to_text(html);
        assert!(text.contains("Visible text"));
        assert!(!text.contains("var x = 1"));
        assert!(!text.contains("color: red"));
        assert!(!text.contains("No JS"));
    }

    #[test]
    fn test_html_to_text_empty() {
        let text = html_to_text("");
        assert!(text.is_empty() || text.trim().is_empty());
    }

    #[test]
    fn test_clean_whitespace() {
        assert_eq!(clean_whitespace("  hello   world  "), "hello world");
        assert_eq!(clean_whitespace("a\n\n\n\nb"), "a\n\nb");
    }
}
