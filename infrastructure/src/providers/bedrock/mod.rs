//! AWS Bedrock Converse API provider
//!
//! Provides access to Claude models via AWS IAM authentication
//! through the Bedrock Converse API.

mod adapter;
mod model_map;
mod session;
mod types;

pub use adapter::BedrockProviderAdapter;
