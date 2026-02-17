//! Logging infrastructure — structured conversation logging.
//!
//! Provides [`JsonlConversationLogger`], a JSONL file writer that implements
//! the [`ConversationLogger`](quorum_application::ConversationLogger) port.

mod jsonl_logger;

pub use jsonl_logger::JsonlConversationLogger;
