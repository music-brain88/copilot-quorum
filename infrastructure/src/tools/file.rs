//! File operation tools: read_file, write_file

use quorum_domain::tool::{
    entities::{RiskLevel, ToolCall, ToolDefinition, ToolParameter},
    value_objects::{ToolError, ToolResult, ToolResultMetadata},
};
use std::fs;
use std::path::Path;
use std::time::Instant;

/// Tool name constants
pub const READ_FILE: &str = "read_file";
pub const WRITE_FILE: &str = "write_file";

/// Maximum file size to read (10 MB)
const MAX_READ_SIZE: u64 = 10 * 1024 * 1024;

/// Get the tool definition for read_file
pub fn read_file_definition() -> ToolDefinition {
    ToolDefinition::new(
        READ_FILE,
        "Read the contents of a file at the specified path",
        RiskLevel::Low,
    )
    .with_parameter(ToolParameter::new("path", "Path to the file to read", true).with_type("path"))
    .with_parameter(
        ToolParameter::new(
            "offset",
            "Line number to start reading from (0-indexed)",
            false,
        )
        .with_type("number"),
    )
    .with_parameter(
        ToolParameter::new("limit", "Maximum number of lines to read", false).with_type("number"),
    )
}

/// Get the tool definition for write_file
pub fn write_file_definition() -> ToolDefinition {
    ToolDefinition::new(
        WRITE_FILE,
        "Write content to a file at the specified path. Creates the file if it doesn't exist, or overwrites if it does.",
        RiskLevel::High,
    )
    .with_parameter(ToolParameter::new("path", "Path to the file to write", true).with_type("path"))
    .with_parameter(ToolParameter::new("content", "Content to write to the file", true).with_type("string"))
    .with_parameter(
        ToolParameter::new("create_dirs", "Create parent directories if they don't exist", false)
            .with_type("boolean"),
    )
}

/// Execute the read_file tool
pub fn execute_read_file(call: &ToolCall) -> ToolResult {
    let start = Instant::now();

    // Get the path argument
    let path_str = match call.require_string("path") {
        Ok(p) => p,
        Err(e) => return ToolResult::failure(READ_FILE, ToolError::invalid_argument(e)),
    };

    let path = Path::new(path_str);

    // Check if file exists
    if !path.exists() {
        return ToolResult::failure(READ_FILE, ToolError::not_found(path_str));
    }

    // Check if it's a file (not a directory)
    if !path.is_file() {
        return ToolResult::failure(
            READ_FILE,
            ToolError::invalid_argument(format!("'{}' is not a file", path_str)),
        );
    }

    // Check file size
    let metadata = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            return ToolResult::failure(
                READ_FILE,
                ToolError::execution_failed(format!("Failed to get file metadata: {}", e)),
            );
        }
    };

    if metadata.len() > MAX_READ_SIZE {
        return ToolResult::failure(
            READ_FILE,
            ToolError::invalid_argument(format!(
                "File too large ({} bytes). Maximum size is {} bytes",
                metadata.len(),
                MAX_READ_SIZE
            )),
        );
    }

    // Read the file
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                return ToolResult::failure(READ_FILE, ToolError::permission_denied(path_str));
            }
            return ToolResult::failure(
                READ_FILE,
                ToolError::execution_failed(format!("Failed to read file: {}", e)),
            );
        }
    };

    // Handle offset and limit
    let offset = call.get_i64("offset").unwrap_or(0) as usize;
    let limit = call.get_i64("limit");

    let output = if offset > 0 || limit.is_some() {
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        if offset >= total_lines {
            String::new()
        } else {
            let end = match limit {
                Some(l) => (offset + l as usize).min(total_lines),
                None => total_lines,
            };
            lines[offset..end].join("\n")
        }
    } else {
        content
    };

    let duration_ms = start.elapsed().as_millis() as u64;
    let bytes = output.len();

    ToolResult::success(READ_FILE, output).with_metadata(ToolResultMetadata {
        duration_ms: Some(duration_ms),
        bytes: Some(bytes),
        path: Some(path_str.to_string()),
        ..Default::default()
    })
}

/// Execute the write_file tool
pub fn execute_write_file(call: &ToolCall) -> ToolResult {
    let start = Instant::now();

    // Get the path argument
    let path_str = match call.require_string("path") {
        Ok(p) => p,
        Err(e) => return ToolResult::failure(WRITE_FILE, ToolError::invalid_argument(e)),
    };

    // Get the content argument
    let content = match call.require_string("content") {
        Ok(c) => c,
        Err(e) => return ToolResult::failure(WRITE_FILE, ToolError::invalid_argument(e)),
    };

    let path = Path::new(path_str);

    // Create parent directories if requested
    let create_dirs = call.get_bool("create_dirs").unwrap_or(false);
    if create_dirs
        && let Some(parent) = path.parent()
            && !parent.exists()
                && let Err(e) = fs::create_dir_all(parent) {
                    return ToolResult::failure(
                        WRITE_FILE,
                        ToolError::execution_failed(format!(
                            "Failed to create parent directories: {}",
                            e
                        )),
                    );
                }

    // Check if parent directory exists
    if let Some(parent) = path.parent()
        && !parent.exists() {
            return ToolResult::failure(
                WRITE_FILE,
                ToolError::not_found(format!(
                    "Parent directory does not exist: {}",
                    parent.display()
                )),
            );
        }

    // Write the file
    let bytes = content.len();
    if let Err(e) = fs::write(path, content) {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            return ToolResult::failure(WRITE_FILE, ToolError::permission_denied(path_str));
        }
        return ToolResult::failure(
            WRITE_FILE,
            ToolError::execution_failed(format!("Failed to write file: {}", e)),
        );
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    ToolResult::success(
        WRITE_FILE,
        format!("Successfully wrote {} bytes to {}", bytes, path_str),
    )
    .with_metadata(ToolResultMetadata {
        duration_ms: Some(duration_ms),
        bytes: Some(bytes),
        path: Some(path_str.to_string()),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_file_success() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "Hello, World!").unwrap();
        let path = temp_file.path().to_str().unwrap();

        let call = ToolCall::new(READ_FILE).with_arg("path", path);
        let result = execute_read_file(&call);

        assert!(result.is_success());
        assert!(result.output().unwrap().contains("Hello, World!"));
    }

    #[test]
    fn test_read_file_not_found() {
        let call = ToolCall::new(READ_FILE).with_arg("path", "/nonexistent/file.txt");
        let result = execute_read_file(&call);

        assert!(!result.is_success());
        assert_eq!(result.error().unwrap().code, "NOT_FOUND");
    }

    #[test]
    fn test_read_file_with_offset_and_limit() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "line1\nline2\nline3\nline4\nline5").unwrap();
        let path = temp_file.path().to_str().unwrap();

        let call = ToolCall::new(READ_FILE)
            .with_arg("path", path)
            .with_arg("offset", 1i64)
            .with_arg("limit", 2i64);
        let result = execute_read_file(&call);

        assert!(result.is_success());
        let output = result.output().unwrap();
        assert!(output.contains("line2"));
        assert!(output.contains("line3"));
        assert!(!output.contains("line1"));
        assert!(!output.contains("line4"));
    }

    #[test]
    fn test_write_file_success() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.txt");
        let path_str = path.to_str().unwrap();

        let call = ToolCall::new(WRITE_FILE)
            .with_arg("path", path_str)
            .with_arg("content", "Hello, World!");
        let result = execute_write_file(&call);

        assert!(result.is_success());
        assert_eq!(fs::read_to_string(&path).unwrap(), "Hello, World!");
    }

    #[test]
    fn test_write_file_create_dirs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("subdir").join("test.txt");
        let path_str = path.to_str().unwrap();

        let call = ToolCall::new(WRITE_FILE)
            .with_arg("path", path_str)
            .with_arg("content", "content")
            .with_arg("create_dirs", true);
        let result = execute_write_file(&call);

        assert!(result.is_success());
        assert!(path.exists());
    }

    #[test]
    fn test_write_file_parent_not_exists() {
        let call = ToolCall::new(WRITE_FILE)
            .with_arg("path", "/nonexistent/dir/file.txt")
            .with_arg("content", "content");
        let result = execute_write_file(&call);

        assert!(!result.is_success());
        assert_eq!(result.error().unwrap().code, "NOT_FOUND");
    }
}
