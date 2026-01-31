//! Session domain entities

use crate::core::model::Model;
use serde::{Deserialize, Serialize};

/// Role of a message in a conversation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
}

/// A message in a conversation (Entity)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// Represents an LLM session (Entity)
///
/// A session maintains conversation history and state with a specific model.
#[derive(Debug, Clone)]
pub struct Session {
    id: String,
    model: Model,
    messages: Vec<Message>,
}

impl Session {
    pub fn new(id: impl Into<String>, model: Model) -> Self {
        Self {
            id: id.into(),
            model,
            messages: Vec::new(),
        }
    }

    pub fn with_system_prompt(id: impl Into<String>, model: Model, system_prompt: String) -> Self {
        let mut session = Self::new(id, model);
        session.messages.push(Message::system(system_prompt));
        session
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn add_user_message(&mut self, content: impl Into<String>) {
        self.messages.push(Message::user(content));
    }

    pub fn add_assistant_message(&mut self, content: impl Into<String>) {
        self.messages.push(Message::assistant(content));
    }
}
