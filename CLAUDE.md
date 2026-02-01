# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

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
├── orchestration/  # Phase, QuorumRun, QuorumResult (合議)
├── agent/          # AgentState, Plan, Task, AgentConfig (エージェント)
├── tool/           # ToolDefinition, ToolCall, ToolResult (ツール)
├── prompt/         # PromptTemplate, AgentPromptTemplate
├── session/        # Message, LlmSessionRepository
└── config/         # OutputFormat
```

### Adding New Features

- New LLM provider: Add to `infrastructure/` implementing `LlmGateway`
- New orchestration strategy: Add to `domain/orchestration/`
- New output format: Add to `presentation/output/`
- New model: Add variant to `domain/src/core/model.rs` Model enum
- New tool: Add to `infrastructure/tools/`, register in `default_tool_spec()`
- New agent capability: Extend `domain/agent/` and `RunAgentUseCase`

### Vertical Slicing Principle

When adding features, maintain the same domain structure across all layers:
```
domain/new_feature/      → entities, traits
application/new_feature/ → use cases
infrastructure/new_feature/ → adapters
presentation/new_feature/   → UI components
```

## Agent System

### Overview

The agent system extends the quorum concept to autonomous task execution. It maintains quorum-based review at critical points while allowing single-model execution for routine tasks.

### Agent Flow

```
User Request
    │
    ▼
┌───────────────────┐
│ Context Gathering │  ← Project info collection (glob, read_file)
└───────────────────┘
    │
    ▼
┌───────────────────┐
│     Planning      │  ← Single model creates task plan
└───────────────────┘
    │
    ▼
┌───────────────────┐
│ 🗳️ QUORUM #1     │  ← All models review the plan (REQUIRED)
│   Plan Review     │     Majority vote to approve/reject
└───────────────────┘
    │
    ▼
┌───────────────────┐
│  Task Execution   │
│   ├─ Low-risk  ────▶ Direct execution
│   │
│   └─ High-risk ────▶ 🗳️ QUORUM #2 (Action Review)
│                        Review before write_file, run_command
└───────────────────┘
    │
    ▼
┌───────────────────┐
│ 🗳️ QUORUM #3     │  ← Optional final review
│  Final Review     │     (require_final_review: true)
└───────────────────┘
```

### Available Tools

| Tool | Description | Risk Level |
|------|-------------|------------|
| `read_file` | Read file contents | Low |
| `write_file` | Write/create file | High (quorum review) |
| `run_command` | Execute shell command | High (quorum review) |
| `glob_search` | Find files by pattern | Low |
| `grep_search` | Search file contents | Low |

### Key Types

**Agent Domain (`domain/agent/`)**
- `AgentState` - Current state of agent execution
- `AgentConfig` - Configuration (primary model, quorum models, etc.)
- `Plan` - Task plan with objective and reasoning
- `Task` - Single task with tool call and dependencies
- `AgentContext` - Gathered project context
- `Thought` - Agent reasoning record

**Tool Domain (`domain/tool/`)**
- `ToolDefinition` - Tool metadata (name, params, risk level)
- `ToolCall` - Tool invocation with arguments
- `ToolResult` - Execution result (success/failure, output)
- `ToolSpec` - Registry of available tools
- `RiskLevel` - Low (read-only) or High (modifying)

**Application Layer**
- `RunAgentUseCase` - Orchestrates the full agent flow
- `ToolExecutorPort` - Abstract tool execution interface
- `AgentProgressNotifier` - Progress callbacks for agent phases

**Infrastructure Layer**
- `LocalToolExecutor` - Implements ToolExecutorPort for local machine

### Quorum Review

The quorum system ensures safety through multi-model consensus:

1. **Plan Review (Required)**: All configured quorum models review the proposed plan
2. **Action Review (High-risk ops)**: Reviews `write_file` and `run_command` before execution
3. **Final Review (Optional)**: Reviews overall execution results

Approval requires majority vote. Rejected plans/actions include aggregated feedback.
