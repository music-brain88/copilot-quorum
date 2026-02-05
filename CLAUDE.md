# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Core Concepts

### Quorum

**Quorum** is the central concept, inspired by distributed systems consensus:

- **Quorum Discussion**: Multi-model equal discussion (collecting perspectives)
- **Quorum Consensus**: Voting-based approval/rejection for plans and actions
- **Quorum Synthesis**: Merging multiple opinions, resolving contradictions

### Solo / Ensemble Modes

| Mode | Description | Use Case |
|------|-------------|----------|
| **Solo** (default) | Single model driven, quick execution | Simple tasks, bug fixes |
| **Ensemble** | Multi-model Quorum Discussion | Complex design, architecture decisions |

```bash
# Solo mode (default)
cargo run -p copilot-quorum -- "Fix this bug"

# Ensemble mode
cargo run -p copilot-quorum -- --ensemble "Design the auth system"

# REPL commands
/solo     # Switch to Solo mode
/ens      # Switch to Ensemble mode
/discuss  # Run Quorum Discussion (works in any mode)
```

## Build & Test Commands

```bash
# Build
cargo build

# Run tests
cargo test --workspace

# Run single package tests
cargo test -p quorum-domain

# Run with debug logging
RUST_LOG=debug cargo run -p copilot-quorum -- "Your question"

# Run in chat mode
cargo run -p copilot-quorum -- --chat -m claude-haiku-4.5

# Initialize project context (generates .quorum/context.md)
cargo run -p copilot-quorum -- /init
```

## Configuration

Project-level config: `quorum.toml` (or `~/.config/copilot-quorum/config.toml` for global)

```toml
# Quorum settings (new unified configuration)
[quorum]
rule = "majority"        # "majority", "unanimous", "atleast:2", "75%"
min_models = 2           # Minimum models for valid consensus

[quorum.discussion]
models = ["claude-sonnet-4.5", "gpt-5.2-codex", "gemini-3-pro-preview"]
moderator = "claude-opus-4.5"
enable_peer_review = true

# Legacy council settings (still supported)
[council]
models = ["claude-sonnet-4.5", "gpt-5.2-codex", "gemini-3-pro-preview"]
moderator = "claude-opus-4.5"

[behavior]
enable_review = true

[output]
format = "synthesis"  # "full", "synthesis", or "json"
```

## Architecture

**DDD + Onion Architecture with Vertical Domain Slicing**

### Dependency Flow (Inner layers have no dependencies)

```
           cli/                 # Entrypoint, DI assembly
             |
      presentation/             # CLI, chat REPL, output formatters
             |
infrastructure/ --> application/   # Adapters --> Use cases + ports
        |                |
        +----> domain/ <-+         # Pure business logic (no external deps)
```

### Layer Responsibilities

| Layer | Crate | Description |
|-------|-------|-------------|
| domain | `quorum-domain` | Entities, value objects, traits (Model, Question, Phase, QuorumResult, AgentState, Plan, Task, ToolCall) |
| application | `quorum-application` | Use cases (RunQuorumUseCase, RunAgentUseCase), port traits (LlmGateway, ProgressNotifier, ToolExecutorPort) |
| infrastructure | `quorum-infrastructure` | Copilot CLI adapter, LocalToolExecutor (file, command, search tools) |
| presentation | `quorum-presentation` | CLI commands, ChatRepl, ConsoleFormatter, ProgressReporter |
| cli | `copilot-quorum` | main.rs with dependency injection |

### Key Traits

- `LlmGateway` (application/ports) - Abstract LLM provider interface
- `LlmSession` (application/ports) - Active session with an LLM
- `ProgressNotifier` (application/ports) - Progress callback interface
- `ToolExecutorPort` (application/ports) - Tool execution interface
- `ToolValidator` (domain/tool) - Tool call validation logic

### Domain Modules

```
domain/src/
├── core/           # Model, Question, Error
├── quorum/         # Vote, QuorumRule, ConsensusRound (合意形成)
├── orchestration/  # Phase, QuorumRun, QuorumResult (オーケストレーション)
├── agent/          # AgentState, Plan, Task, AgentConfig (エージェント)
├── tool/           # ToolDefinition, ToolCall, ToolResult (ツール)
├── prompt/         # PromptTemplate, AgentPromptTemplate
├── session/        # Message, LlmSessionRepository
├── context/        # ProjectContext, KnownContextFile (/init用)
└── config/         # OutputFormat
```

### Adding New Features

- New LLM provider: Add to `infrastructure/` implementing `LlmGateway`
- New orchestration strategy: Add to `domain/orchestration/`
- New output format: Add to `presentation/output/`
- New model: Add variant to `domain/src/core/model.rs` Model enum
- New tool: Add to `infrastructure/tools/`, register in `default_tool_spec()`
- New agent capability: Extend `domain/agent/` and `RunAgentUseCase`
- New context file type: Add to `domain/context/` KnownContextFile enum

### Vertical Slicing Principle

When adding features, maintain the same domain structure across all layers:
```
domain/new_feature/      → entities, traits
application/new_feature/ → use cases
infrastructure/new_feature/ → adapters
presentation/new_feature/   → UI components
```

## Agent System

The agent system extends quorum to autonomous task execution with safety through multi-model consensus.

**Flow**: Context Gathering → Planning → Quorum Plan Review → Task Execution → (Optional) Final Review

**Tools**: `read_file`, `write_file`, `run_command`, `glob_search`, `grep_search`
- Low-risk (read-only): Direct execution
- High-risk (write/command): Requires quorum review before execution

**Key Components**:
- `domain/agent/`: AgentState, Plan, Task, AgentConfig
- `domain/tool/`: ToolDefinition, ToolCall, ToolResult, RiskLevel
- `application/use_cases/run_agent.rs`: RunAgentUseCase orchestrates the flow
- `infrastructure/tools/`: LocalToolExecutor implements ToolExecutorPort

詳細は [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) を参照。
