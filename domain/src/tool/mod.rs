//! Tool domain module
//!
//! This module defines the core abstractions for the agent's **Tool System** —
//! how autonomous agents interact with the local environment (file I/O, commands,
//! web access) in a validated, risk-aware manner.
//!
//! # Overview
//!
//! The tool system provides agents with concrete capabilities beyond text generation.
//! Every tool is defined by a [`ToolDefinition`] (name, parameters, risk level),
//! invoked via a [`ToolCall`], and returns a [`ToolResult`] with structured metadata.
//!
//! ```text
//! ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
//! │ ToolSpec     │───▶│ ToolCall     │───▶│ ToolResult   │
//! │ (registry)   │    │ (invocation) │    │ (output)     │
//! └──────┬───────┘    └──────────────┘    └──────────────┘
//!        │
//!        ├─ aliases: "bash" → "run_command"
//!        └─ tools:   "run_command" → ToolDefinition
//! ```
//!
//! # Tool Name Alias System
//!
//! LLMs frequently hallucinate tool names (e.g. `bash` instead of `run_command`,
//! `grep` instead of `grep_search`). The **alias system** in [`ToolSpec`] provides
//! zero-cost name resolution without an extra LLM round-trip:
//!
//! - [`ToolSpec::resolve_alias`] — resolves alias → canonical name (aliases only)
//! - [`ToolSpec::resolve`] — resolves any name (canonical or alias)
//! - [`ToolSpec::get_resolved`] — looks up a [`ToolDefinition`] by canonical or alias name
//!
//! This is used by the application layer's `resolve_tool_call()` and
//! `resolve_plan_aliases()` to transparently correct LLM mistakes.
//!
//! # Risk-Based Execution
//!
//! Each tool has a [`RiskLevel`](entities::RiskLevel) that determines whether
//! Quorum review is required before execution:
//!
//! | Risk | Examples | Quorum Review |
//! |------|----------|---------------|
//! | **Low** | `read_file`, `glob_search`, `web_fetch` | No (direct execution) |
//! | **High** | `write_file`, `run_command` | Yes (multi-model consensus) |
//!
//! # Key Types
//!
//! - [`ToolSpec`] — Registry of available tools + alias mappings
//! - [`ToolDefinition`] — Schema for a single tool (name, params, risk level)
//! - [`ToolCall`] — An invocation request with arguments
//! - [`ToolResult`] — Execution outcome with structured [`ToolResultMetadata`](value_objects::ToolResultMetadata)
//! - [`ToolValidator`] — Pure domain trait for parameter validation
//! - [`ToolProvider`] — Abstraction for external tool providers (MCP, etc.)
//!
//! # Architecture
//!
//! The tool domain follows the Onion Architecture principle:
//!
//! - **Domain** (this module): Pure definitions, no I/O
//! - **Application** (`ToolExecutorPort`): Port trait for tool execution
//! - **Infrastructure** (`LocalToolExecutor`): Concrete execution with file I/O,
//!   process spawning, and HTTP requests (web tools)
//!
//! # See Also
//!
//! - [`crate::agent`] — Agent system that orchestrates tool usage
//! - [`crate::orchestration`] — Quorum consensus for high-risk tool review

pub mod entities;
pub mod provider;
pub mod traits;
pub mod value_objects;

pub use entities::{ToolCall, ToolDefinition, ToolSpec};
pub use provider::{ProviderError, ToolProvider};
pub use traits::{DefaultToolValidator, ToolValidator};
pub use value_objects::{ToolError, ToolResult};
