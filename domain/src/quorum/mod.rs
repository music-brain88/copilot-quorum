//! Quorum consensus domain
//!
//! This module contains the core concepts for Quorum-based decision making.
//!
//! # Core Concepts
//!
//! ## Quorum Discussion
//! Multiple models participate in an equal discussion on a topic.
//! Each model provides its perspective, and a synthesis is created.
//!
//! ## Quorum Consensus
//! Voting-based approval/rejection mechanism for plans and actions.
//! Used for safety-critical decisions in agent execution.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  Quorum Layers                                                   │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  ┌───────────────────────────────────────────────────────────┐  │
//! │  │  Knowledge Quorum (知識層) - Phase 3                       │  │
//! │  │  - Persistent knowledge-based consensus                    │  │
//! │  └───────────────────────────────────────────────────────────┘  │
//! │                          ↓                                       │
//! │  ┌───────────────────────────────────────────────────────────┐  │
//! │  │  Context Quorum (コンテキスト層) - Phase 2                 │  │
//! │  │  - Cross-session context sharing                          │  │
//! │  │  - Discussion history, decisions, patterns                │  │
//! │  └───────────────────────────────────────────────────────────┘  │
//! │                          ↓                                       │
//! │  ┌───────────────────────────────────────────────────────────┐  │
//! │  │  Decision Quorum (決定層) - Phase 1 (Current)              │  │
//! │  │  - Multi-model consensus for decisions                    │  │
//! │  │  - Quorum Discussion, Quorum Consensus                    │  │
//! │  └───────────────────────────────────────────────────────────┘  │
//! │                                                                  │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Mode Selection
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  Solo / Ensemble Mode                                           │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                                                                 │
//! │  【Solo Mode】                     【Ensemble Mode】             │
//! │  - Single model (Agent) driven    - Multi-model (Quorum) driven │
//! │  - Quick execution                - Multi-perspective discussion│
//! │  - /discuss on demand             - Always multi-model          │
//! │  - Simple tasks                   - Complex design/decisions    │
//! │  - Default                        - --ensemble flag             │
//! │                                                                 │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

pub mod consensus;
pub mod parsing;
pub mod rule;
pub mod vote;

// Re-export main types
pub use consensus::{ConsensusOutcome, ConsensusRound};
pub use parsing::{parse_final_review_response, parse_review_response, parse_vote_score};
pub use rule::QuorumRule;
pub use vote::{Vote, VoteResult};
