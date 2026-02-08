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

[agent]
consensus_level = "solo"  # "solo" or "ensemble"
phase_scope = "full"      # "full", "fast", "plan-only"
strategy = "quorum"       # "quorum" or "debate"
hil_mode = "interactive"  # "interactive", "auto_reject", "auto_approve"
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
| domain | `quorum-domain` | Entities, value objects, traits (Model, Question, Phase, QuorumResult, AgentState, Plan, Task, ToolCall, ConsensusLevel, PhaseScope, OrchestrationStrategy) |
| application | `quorum-application` | Use cases (RunQuorumUseCase, RunAgentUseCase), port traits (LlmGateway, ProgressNotifier, ToolExecutorPort) |
| infrastructure | `quorum-infrastructure` | Copilot CLI adapter, LocalToolExecutor (file, command, search tools) |
| presentation | `quorum-presentation` | CLI commands, ChatRepl, ConsoleFormatter, ProgressReporter |
| cli | `copilot-quorum` | main.rs with dependency injection |

### Key Traits

- `LlmGateway` (application/ports) - Abstract LLM provider interface
- `LlmSession` (application/ports) - Active session with an LLM
- `ProgressNotifier` (application/ports) - Progress callback interface
- `ToolExecutorPort` (application/ports) - Tool execution interface
- `StrategyExecutor` (domain/orchestration) - Orchestration strategy execution interface
- `ToolValidator` (domain/tool) - Tool call validation logic

### Domain Modules

```
domain/src/
├── core/           # Model, Question, Error
├── quorum/         # Vote, QuorumRule, ConsensusRound (合意形成)
├── orchestration/  # ConsensusLevel, PhaseScope, OrchestrationStrategy, StrategyExecutor, Phase, QuorumRun, QuorumResult (オーケストレーション)
├── agent/          # AgentState, Plan, Task, AgentConfig (エージェント)
├── tool/           # ToolDefinition, ToolCall, ToolSpec (with aliases), ToolResult (ツール)
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
- New tool: Add to `infrastructure/tools/`, register in `default_tool_spec()`, add aliases in same file
- New agent capability: Extend `domain/agent/` and `RunAgentUseCase`
- New context file type: Add to `domain/context/` KnownContextFile enum

### Tool Name Alias System

LLM のツール名間違い（`bash` → `run_command` 等）を API 呼び出しなしで自動解決する仕組み。
- `ToolSpec::register_alias()` / `register_aliases()` でエイリアス登録
- `resolve_tool_call()` でエイリアスファストパス（LLM に聞く前に解決）
- `resolve_plan_aliases()` で Plan 段階のツール名も自動変換
- `has_tool()` / `get()` は正規名のみ（exact match）— executor のルーティングを壊さない

### Tool Output Format Specification

**Supported LLM Response Formats** (for tool calls):

| Format | Priority | Multi-Tool? | Example |
|--------|----------|-------------|---------|
| ` ```tool ` block | 1 (Highest) | ✅ Sequential | Preferred in prompts |
| ` ```json ` block | 2 | ✅ Sequential | Alternative markdown |
| Raw JSON | 3 | ❌ Single only | Whole response is JSON |
| Embedded JSON | 4 (Lowest) | ❌ Single only | Heuristic fallback |

**Multi-Tool Execution**:
- Multiple ` ```tool ` or ` ```json ` blocks → All parsed & executed sequentially
- JSON array `[{...}, {...}]` → **Not supported** (use multiple blocks instead)

**Retry Strategy** (for tool execution failures):
- **Retryable errors**: `INVALID_ARGUMENT`, `NOT_FOUND`
- **Max retries**: 2 attempts with LLM correction
- **Non-retryable**: Execution errors returned immediately

See `application/src/use_cases/run_agent.rs` for implementation details.
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

**Tools**: `read_file`, `write_file`, `run_command`, `glob_search`, `grep_search`, `web_fetch`, `web_search` (web-tools feature)
- Low-risk (read-only): Direct execution
- High-risk (write/command): Requires quorum review before execution

**Key Components**:
- `domain/agent/`: AgentState, Plan, Task, AgentConfig
- `domain/tool/`: ToolDefinition, ToolCall, ToolResult, RiskLevel
- `application/use_cases/run_agent.rs`: RunAgentUseCase orchestrates the flow
- `infrastructure/tools/`: LocalToolExecutor implements ToolExecutorPort

詳細は [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) を参照。

## Feature Documentation

機能別の詳細ドキュメント（[docs/features/](docs/features/README.md)）:

| Document | Description |
|----------|-------------|
| [quorum.md](docs/features/quorum.md) | Quorum Discussion & Consensus |
| [agent-system.md](docs/features/agent-system.md) | Agent System + HiL |
| [ensemble-mode.md](docs/features/ensemble-mode.md) | Ensemble Mode（研究エビデンス付き） |
| [tool-system.md](docs/features/tool-system.md) | Tool System（プラグイン、リスク分類） |
| [cli-and-configuration.md](docs/features/cli-and-configuration.md) | REPL、設定、コンテキスト管理 |
