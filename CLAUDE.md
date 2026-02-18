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
/solo      # Switch to Solo mode
/ens       # Switch to Ensemble mode
/council <question>  # Run ad-hoc Quorum Discussion
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
# Role-based model selection
[models]
exploration = "gpt-5.2-codex"           # Context gathering + low-risk tools
decision = "claude-sonnet-4.5"          # Planning + high-risk tools
review = ["claude-opus-4.5", "gpt-5.2-codex", "gemini-3-pro-preview"]

# Quorum consensus rules
[quorum]
rule = "majority"        # "majority", "unanimous", "atleast:2", "75%"
min_models = 2           # Minimum models for valid consensus
moderator = "claude-opus-4.5"
enable_peer_review = true

[output]
format = "synthesis"  # "full", "synthesis", or "json"

[agent]
consensus_level = "solo"  # "solo" or "ensemble"
phase_scope = "full"      # "full", "fast", "plan-only"
strategy = "quorum"       # "quorum" or "debate"
hil_mode = "interactive"  # "interactive", "auto_reject", "auto_approve"

[tui.input]
submit_key = "enter"           # Key to send message
newline_key = "shift+enter"    # Key to insert newline (multiline)
editor_key = "I"               # Key to launch $EDITOR (Normal mode)
editor_action = "return_to_insert"  # "return_to_insert" or "submit"
max_height = 10                # Max input area height in lines
dynamic_height = true          # Input area grows with content
context_header = true          # Show context header in $EDITOR
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
| domain | `quorum-domain` | Entities, value objects, traits (Model, Question, Phase, QuorumResult, AgentState, Plan, Task, ToolCall, ConsensusLevel, PhaseScope, OrchestrationStrategy, LlmResponse, ContentBlock, StopReason) |
| application | `quorum-application` | Use cases (RunQuorumUseCase, RunAgentUseCase), port traits (LlmGateway, ProgressNotifier, ToolExecutorPort, ToolResultMessage) |
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
├── orchestration/  # ConsensusLevel, PhaseScope, OrchestrationStrategy, SessionMode, StrategyExecutor, Phase, QuorumRun, QuorumResult (オーケストレーション)
├── agent/          # AgentState, Plan, Task, ModelConfig, AgentPolicy (エージェント)
├── tool/           # ToolDefinition, ToolCall, ToolSpec, ToolResult (ツール)
├── prompt/         # PromptTemplate, AgentPromptTemplate
├── session/        # Message, LlmSessionRepository, LlmResponse, ContentBlock, StopReason
├── context/        # ProjectContext, KnownContextFile (/init用)
└── config/         # OutputFormat
```

### Adding New Features

- New LLM provider: Add to `infrastructure/` implementing `LlmGateway`
- New orchestration strategy: Add to `domain/orchestration/`
- New output format: Add to `presentation/output/`
- New model: Add variant to `domain/src/core/model.rs` Model enum
- New tool: Add to `infrastructure/tools/`, register in `default_tool_spec()`
- Custom tool: Add to `[tools.custom]` in `quorum.toml`
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
- `domain/agent/`: AgentState, Plan, Task, ModelConfig, AgentPolicy, HilAction
- `domain/orchestration/session_mode.rs`: SessionMode (runtime-mutable: consensus_level, phase_scope, strategy)
- `domain/tool/`: ToolDefinition, ToolCall (native_id), ToolResult, RiskLevel
- `domain/session/response.rs`: LlmResponse, ContentBlock, StopReason
- `application/config/`: ExecutionParams (use case loop control), QuorumConfig (4-type container for buffer propagation)
- `application/use_cases/run_agent.rs`: RunAgentUseCase orchestrates the flow (Native Tool Use)
- `application/ports/llm_gateway.rs`: LlmSession trait (send_with_tools, send_tool_results)
- `infrastructure/tools/`: LocalToolExecutor implements ToolExecutorPort

**Native Tool Use**: LLM の構造化 Tool Use API を使用してツールを呼び出す（唯一のツール実行パス）。
- ツール定義は `ToolSchemaPort::all_tools_schema()` で JSON Schema に変換（Port パターン）
- Multi-turn loop: `send_with_tools()` → ToolUse stop → execute → `send_tool_results()` → repeat
- Low-risk ツールは `futures::join_all()` で並列実行、High-risk は順次 + Quorum Review

詳細は [docs/reference/architecture.md](docs/reference/architecture.md) を参照。

## Feature Documentation

機能別の詳細ドキュメント（[docs/README.md](docs/README.md)）:

| Document | Description |
|----------|-------------|
| [tui.md](docs/guides/tui.md) | Modal TUI（Tab/Pane、Actorパターン） |
| [cli-and-configuration.md](docs/guides/cli-and-configuration.md) | REPL、設定、コンテキスト管理 |
| [quorum.md](docs/concepts/quorum.md) | Quorum Discussion & Consensus |
| [ensemble-mode.md](docs/concepts/ensemble-mode.md) | Ensemble Mode（研究エビデンス付き） |
| [interaction-model.md](docs/concepts/interaction-model.md) | InteractionForm、ContextMode、ネスティング |
| [agent-system.md](docs/systems/agent-system.md) | Agent System + HiL + ToolExecution |
| [tool-system.md](docs/systems/tool-system.md) | Tool System（プラグイン、リスク分類） |
| [native-tool-use.md](docs/systems/native-tool-use.md) | Native Tool Use API（構造化ツール呼び出し） |
| [transport.md](docs/systems/transport.md) | Transport Demultiplexer（並列セッションルーティング） |
| [logging.md](docs/systems/logging.md) | ログシステム（ConversationLogger、JSONL） |
| [vision/](docs/vision/README.md) | 将来ビジョン（Knowledge、Workflow、Extension） |
