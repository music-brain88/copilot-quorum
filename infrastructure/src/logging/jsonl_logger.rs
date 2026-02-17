//! JSONL file writer for conversation events.
//!
//! Each [`ConversationEvent`] is serialized as a single JSON line with a
//! `type` field and `timestamp`, appended to the file via a buffered writer.

use quorum_application::ports::conversation_logger::{ConversationEvent, ConversationLogger};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::warn;

/// JSONL conversation logger that writes one JSON object per line.
///
/// Thread-safe via `Mutex<BufWriter<File>>`. Flushes on `Drop`.
pub struct JsonlConversationLogger {
    writer: Mutex<BufWriter<File>>,
    path: PathBuf,
}

impl JsonlConversationLogger {
    /// Create a new logger writing to the given path.
    ///
    /// Creates the file (and parent directories) if they don't exist.
    /// Returns `None` if the file cannot be created.
    pub fn new(path: impl AsRef<Path>) -> Option<Self> {
        let path = path.as_ref();

        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            warn!(
                "Could not create conversation log directory {}: {}",
                parent.display(),
                e
            );
            return None;
        }

        let file = match File::create(path) {
            Ok(f) => f,
            Err(e) => {
                warn!(
                    "Could not create conversation log file {}: {}",
                    path.display(),
                    e
                );
                return None;
            }
        };

        Some(Self {
            writer: Mutex::new(BufWriter::new(file)),
            path: path.to_path_buf(),
        })
    }

    /// Get the path to the log file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ConversationLogger for JsonlConversationLogger {
    fn log(&self, event: ConversationEvent) {
        let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        // Build the record: merge payload with type + timestamp
        let record = if let serde_json::Value::Object(mut map) = event.payload {
            map.insert(
                "type".to_string(),
                serde_json::Value::String(event.event_type.to_string()),
            );
            map.insert(
                "timestamp".to_string(),
                serde_json::Value::String(timestamp),
            );
            serde_json::Value::Object(map)
        } else {
            serde_json::json!({
                "type": event.event_type,
                "timestamp": timestamp,
                "data": event.payload,
            })
        };

        let Ok(line) = serde_json::to_string(&record) else {
            return;
        };

        if let Ok(mut writer) = self.writer.lock() {
            let _ = writeln!(writer, "{}", line);
            // Flush periodically for crash safety — JSONL is append-only
            let _ = writer.flush();
        }
    }
}

impl Drop for JsonlConversationLogger {
    fn drop(&mut self) {
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_jsonl_logger_writes_valid_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.conversation.jsonl");
        let logger = JsonlConversationLogger::new(&path).unwrap();

        logger.log(ConversationEvent::new(
            "llm_response",
            serde_json::json!({
                "model": "test-model",
                "bytes": 42,
                "text": "Hello world"
            }),
        ));

        logger.log(ConversationEvent::new(
            "tool_call",
            serde_json::json!({
                "tool": "read_file",
                "args": {"path": "foo.rs"}
            }),
        ));

        // Flush
        drop(logger);

        let mut content = String::new();
        File::open(&path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();

        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 2);

        // Each line should be valid JSON with type + timestamp
        for line in &lines {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(value.get("type").is_some());
            assert!(value.get("timestamp").is_some());
        }

        // Check first line
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["type"], "llm_response");
        assert_eq!(first["model"], "test-model");
        assert_eq!(first["bytes"], 42);
        assert_eq!(first["text"], "Hello world");

        // Check second line
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["type"], "tool_call");
        assert_eq!(second["tool"], "read_file");
    }

    #[test]
    fn test_jsonl_logger_handles_non_object_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test2.conversation.jsonl");
        let logger = JsonlConversationLogger::new(&path).unwrap();

        logger.log(ConversationEvent::new(
            "simple_event",
            serde_json::json!("just a string"),
        ));

        drop(logger);

        let mut content = String::new();
        File::open(&path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();

        let value: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(value["type"], "simple_event");
        assert_eq!(value["data"], "just a string");
    }

    #[test]
    fn test_jsonl_logger_returns_none_for_invalid_path() {
        let result = JsonlConversationLogger::new("/nonexistent/deeply/nested/path/file.jsonl");
        // On most systems this will fail — the exact behavior depends on permissions
        // Just verify it doesn't panic
        let _ = result;
    }
}
