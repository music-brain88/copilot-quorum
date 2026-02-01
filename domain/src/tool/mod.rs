//! Tool domain module
//!
//! Contains tool definitions, calls, results and validator trait.
//! The ToolExecutorPort is defined in the application layer (ports).

pub mod entities;
pub mod traits;
pub mod value_objects;

pub use entities::{ToolCall, ToolDefinition, ToolSpec};
pub use traits::{DefaultToolValidator, ToolValidator};
pub use value_objects::{ToolError, ToolResult};
