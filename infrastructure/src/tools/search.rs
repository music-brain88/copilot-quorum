//! Search tools: glob_search, grep_search

use glob::glob;
use quorum_domain::tool::{
    entities::{RiskLevel, ToolCall, ToolDefinition, ToolParameter},
    value_objects::{ToolError, ToolResult, ToolResultMetadata},
};
use regex::Regex;
use std::fs;
use std::path::Path;
use std::time::Instant;

/// Tool name constants
pub const GLOB_SEARCH: &str = "glob_search";
pub const GREP_SEARCH: &str = "grep_search";

/// Maximum number of results to return
const MAX_RESULTS: usize = 1000;

/// Maximum file size for grep (5 MB)
const MAX_GREP_FILE_SIZE: u64 = 5 * 1024 * 1024;

/// Get the tool definition for glob_search
pub fn glob_search_definition() -> ToolDefinition {
    ToolDefinition::new(
        GLOB_SEARCH,
        "Search for files matching a glob pattern (e.g., '**/*.rs', 'src/*.txt')",
        RiskLevel::Low,
    )
    .with_parameter(
        ToolParameter::new("pattern", "Glob pattern to match files", true).with_type("string"),
    )
    .with_parameter(
        ToolParameter::new(
            "base_dir",
            "Base directory to search from (default: current dir)",
            false,
        )
        .with_type("path"),
    )
    .with_parameter(
        ToolParameter::new(
            "max_results",
            "Maximum number of results to return (default: 1000)",
            false,
        )
        .with_type("number"),
    )
}

/// Get the tool definition for grep_search
pub fn grep_search_definition() -> ToolDefinition {
    ToolDefinition::new(
        GREP_SEARCH,
        "Search for a pattern within file contents using regex",
        RiskLevel::Low,
    )
    .with_parameter(
        ToolParameter::new("pattern", "Regex pattern to search for", true).with_type("string"),
    )
    .with_parameter(
        ToolParameter::new("path", "File or directory to search in", true).with_type("path"),
    )
    .with_parameter(
        ToolParameter::new(
            "file_pattern",
            "Glob pattern to filter files (e.g., '*.rs')",
            false,
        )
        .with_type("string"),
    )
    .with_parameter(
        ToolParameter::new(
            "context_lines",
            "Number of context lines before and after match",
            false,
        )
        .with_type("number"),
    )
    .with_parameter(
        ToolParameter::new("case_insensitive", "Perform case-insensitive search", false)
            .with_type("boolean"),
    )
}

/// Execute the glob_search tool
pub fn execute_glob_search(call: &ToolCall) -> ToolResult {
    let start = Instant::now();

    // Get the pattern argument
    let pattern = match call.require_string("pattern") {
        Ok(p) => p,
        Err(e) => return ToolResult::failure(GLOB_SEARCH, ToolError::invalid_argument(e)),
    };

    // Get optional base directory
    let base_dir = call.get_string("base_dir").unwrap_or(".");

    // Get max results
    let max_results = call
        .get_i64("max_results")
        .map(|n| n as usize)
        .unwrap_or(MAX_RESULTS)
        .min(MAX_RESULTS);

    // Build the full pattern
    let full_pattern = if pattern.starts_with('/') || pattern.starts_with("./") {
        pattern.to_string()
    } else {
        format!("{}/{}", base_dir, pattern)
    };

    // Execute glob search
    let entries = match glob(&full_pattern) {
        Ok(paths) => paths,
        Err(e) => {
            return ToolResult::failure(
                GLOB_SEARCH,
                ToolError::invalid_argument(format!("Invalid glob pattern: {}", e)),
            )
        }
    };

    let mut results = Vec::new();
    let mut error_count = 0;

    for entry in entries {
        if results.len() >= max_results {
            break;
        }

        match entry {
            Ok(path) => {
                results.push(path.display().to_string());
            }
            Err(_) => {
                error_count += 1;
            }
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    let match_count = results.len();

    let mut output = results.join("\n");
    if results.len() >= max_results {
        output.push_str(&format!("\n... (limited to {} results)", max_results));
    }
    if error_count > 0 {
        output.push_str(&format!("\n({} paths could not be accessed)", error_count));
    }

    if results.is_empty() {
        output = "No files found matching the pattern".to_string();
    }

    ToolResult::success(GLOB_SEARCH, output).with_metadata(ToolResultMetadata {
        duration_ms: Some(duration_ms),
        match_count: Some(match_count),
        ..Default::default()
    })
}

/// Execute the grep_search tool
pub fn execute_grep_search(call: &ToolCall) -> ToolResult {
    let start = Instant::now();

    // Get the pattern argument
    let pattern_str = match call.require_string("pattern") {
        Ok(p) => p,
        Err(e) => return ToolResult::failure(GREP_SEARCH, ToolError::invalid_argument(e)),
    };

    // Get the path argument
    let path_str = match call.require_string("path") {
        Ok(p) => p,
        Err(e) => return ToolResult::failure(GREP_SEARCH, ToolError::invalid_argument(e)),
    };

    let path = Path::new(path_str);
    if !path.exists() {
        return ToolResult::failure(GREP_SEARCH, ToolError::not_found(path_str));
    }

    // Get optional parameters
    let file_pattern = call.get_string("file_pattern");
    let context_lines = call.get_i64("context_lines").unwrap_or(0) as usize;
    let case_insensitive = call.get_bool("case_insensitive").unwrap_or(false);

    // Build regex
    let regex_pattern = if case_insensitive {
        format!("(?i){}", pattern_str)
    } else {
        pattern_str.to_string()
    };

    let regex = match Regex::new(&regex_pattern) {
        Ok(r) => r,
        Err(e) => {
            return ToolResult::failure(
                GREP_SEARCH,
                ToolError::invalid_argument(format!("Invalid regex pattern: {}", e)),
            )
        }
    };

    // Collect files to search
    let files = if path.is_file() {
        vec![path.to_path_buf()]
    } else {
        collect_files(path, file_pattern)
    };

    let mut results = Vec::new();
    let mut total_matches = 0;

    for file_path in files {
        if results.len() >= MAX_RESULTS {
            break;
        }

        // Check file size
        if let Ok(metadata) = fs::metadata(&file_path) {
            if metadata.len() > MAX_GREP_FILE_SIZE {
                continue;
            }
        }

        // Read and search file
        if let Ok(content) = fs::read_to_string(&file_path) {
            let lines: Vec<&str> = content.lines().collect();
            let file_display = file_path.display().to_string();

            for (line_num, line) in lines.iter().enumerate() {
                if results.len() >= MAX_RESULTS {
                    break;
                }

                if regex.is_match(line) {
                    total_matches += 1;

                    if context_lines > 0 {
                        // Add context
                        let start_line = line_num.saturating_sub(context_lines);
                        let end_line = (line_num + context_lines + 1).min(lines.len());

                        let mut context_result = format!("{}:", file_display);
                        for (i, ctx_line) in lines[start_line..end_line].iter().enumerate() {
                            let actual_line_num = start_line + i + 1;
                            let marker = if actual_line_num == line_num + 1 {
                                ">"
                            } else {
                                " "
                            };
                            context_result.push_str(&format!(
                                "\n{}{}: {}",
                                marker, actual_line_num, ctx_line
                            ));
                        }
                        results.push(context_result);
                    } else {
                        results.push(format!("{}:{}: {}", file_display, line_num + 1, line));
                    }
                }
            }
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    let mut output = results.join("\n");
    if total_matches >= MAX_RESULTS {
        output.push_str(&format!("\n... (limited to {} matches)", MAX_RESULTS));
    }

    if results.is_empty() {
        output = "No matches found".to_string();
    }

    ToolResult::success(GREP_SEARCH, output).with_metadata(ToolResultMetadata {
        duration_ms: Some(duration_ms),
        match_count: Some(total_matches),
        path: Some(path_str.to_string()),
        ..Default::default()
    })
}

/// Collect files from a directory, optionally filtered by a glob pattern
fn collect_files(dir: &Path, file_pattern: Option<&str>) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    let pattern = file_pattern.unwrap_or("**/*");
    let full_pattern = format!("{}/{}", dir.display(), pattern);

    if let Ok(paths) = glob(&full_pattern) {
        for entry in paths.flatten() {
            if entry.is_file() && files.len() < MAX_RESULTS {
                files.push(entry);
            }
        }
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{tempdir, NamedTempFile};

    #[test]
    fn test_glob_search_basic() {
        let temp_dir = tempdir().unwrap();
        let file1 = temp_dir.path().join("test1.txt");
        let file2 = temp_dir.path().join("test2.txt");
        fs::write(&file1, "content1").unwrap();
        fs::write(&file2, "content2").unwrap();

        let call = ToolCall::new(GLOB_SEARCH)
            .with_arg("pattern", "*.txt")
            .with_arg("base_dir", temp_dir.path().to_str().unwrap());
        let result = execute_glob_search(&call);

        assert!(result.is_success());
        let output = result.output().unwrap();
        assert!(output.contains("test1.txt"));
        assert!(output.contains("test2.txt"));
    }

    #[test]
    fn test_glob_search_no_matches() {
        let temp_dir = tempdir().unwrap();

        let call = ToolCall::new(GLOB_SEARCH)
            .with_arg("pattern", "*.xyz")
            .with_arg("base_dir", temp_dir.path().to_str().unwrap());
        let result = execute_glob_search(&call);

        assert!(result.is_success());
        assert!(result.output().unwrap().contains("No files found"));
    }

    #[test]
    fn test_grep_search_basic() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "line one").unwrap();
        writeln!(temp_file, "line two with pattern").unwrap();
        writeln!(temp_file, "line three").unwrap();
        let path = temp_file.path().to_str().unwrap();

        let call = ToolCall::new(GREP_SEARCH)
            .with_arg("pattern", "pattern")
            .with_arg("path", path);
        let result = execute_grep_search(&call);

        assert!(result.is_success());
        let output = result.output().unwrap();
        assert!(output.contains("line two with pattern"));
        assert!(output.contains(":2:"));
    }

    #[test]
    fn test_grep_search_case_insensitive() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "Hello World").unwrap();
        writeln!(temp_file, "hello world").unwrap();
        let path = temp_file.path().to_str().unwrap();

        let call = ToolCall::new(GREP_SEARCH)
            .with_arg("pattern", "HELLO")
            .with_arg("path", path)
            .with_arg("case_insensitive", true);
        let result = execute_grep_search(&call);

        assert!(result.is_success());
        let output = result.output().unwrap();
        // Should match both lines
        assert!(output.contains(":1:"));
        assert!(output.contains(":2:"));
    }

    #[test]
    fn test_grep_search_with_context() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "before1").unwrap();
        writeln!(temp_file, "before2").unwrap();
        writeln!(temp_file, "MATCH").unwrap();
        writeln!(temp_file, "after1").unwrap();
        writeln!(temp_file, "after2").unwrap();
        let path = temp_file.path().to_str().unwrap();

        let call = ToolCall::new(GREP_SEARCH)
            .with_arg("pattern", "MATCH")
            .with_arg("path", path)
            .with_arg("context_lines", 1i64);
        let result = execute_grep_search(&call);

        assert!(result.is_success());
        let output = result.output().unwrap();
        assert!(output.contains("before2"));
        assert!(output.contains("MATCH"));
        assert!(output.contains("after1"));
    }

    #[test]
    fn test_grep_search_directory() {
        let temp_dir = tempdir().unwrap();
        let file1 = temp_dir.path().join("file1.txt");
        let file2 = temp_dir.path().join("file2.txt");
        fs::write(&file1, "hello world").unwrap();
        fs::write(&file2, "goodbye world").unwrap();

        let call = ToolCall::new(GREP_SEARCH)
            .with_arg("pattern", "world")
            .with_arg("path", temp_dir.path().to_str().unwrap())
            .with_arg("file_pattern", "*.txt");
        let result = execute_grep_search(&call);

        assert!(result.is_success());
        let output = result.output().unwrap();
        assert!(output.contains("hello world"));
        assert!(output.contains("goodbye world"));
    }

    #[test]
    fn test_grep_search_no_matches() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "some content").unwrap();
        let path = temp_file.path().to_str().unwrap();

        let call = ToolCall::new(GREP_SEARCH)
            .with_arg("pattern", "nonexistent")
            .with_arg("path", path);
        let result = execute_grep_search(&call);

        assert!(result.is_success());
        assert!(result.output().unwrap().contains("No matches found"));
    }

    #[test]
    fn test_grep_search_invalid_regex() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        let call = ToolCall::new(GREP_SEARCH)
            .with_arg("pattern", "[invalid")
            .with_arg("path", path);
        let result = execute_grep_search(&call);

        assert!(!result.is_success());
        assert_eq!(result.error().unwrap().code, "INVALID_ARGUMENT");
    }
}
