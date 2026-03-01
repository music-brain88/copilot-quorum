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

Configuration via Lua: `~/.config/copilot-quorum/init.lua` (plugins in `plugins/*.lua`)

Boot sequence: Rust defaults → Lua (init.lua + plugins) → CLI arg overrides

```lua
-- ~/.config/copilot-quorum/init.lua

-- Role-based model selection
quorum.config.set("models.exploration", "gpt-5.2-codex")       -- Context gathering + low-risk tools
quorum.config.set("models.decision", "claude-sonnet-4.5")      -- Planning + high-risk tools
quorum.config.set("models.review", { "claude-opus-4.5", "gpt-5.2-codex", "gemini-3-pro-preview" })
-- quorum.config.set("models.participants", { "claude-opus-4.5", "gpt-5.2-codex" })  -- Quorum Discussion
-- quorum.config.set("models.moderator", "claude-opus-4.5")    -- Quorum Synthesis
-- quorum.config.set("models.ask", "claude-sonnet-4.5")        -- Ask (Q&A) interaction

-- Agent settings
quorum.config.set("agent.consensus_level", "solo")     -- "solo" or "ensemble"
quorum.config.set("agent.phase_scope", "full")          -- "full", "fast", "plan-only"
quorum.config.set("agent.strategy", "quorum")           -- "quorum" or "debate"
quorum.config.set("agent.hil_mode", "interactive")      -- "interactive", "auto_reject", "auto_approve"

-- Output
quorum.config.set("output.format", "synthesis")         -- "full", "synthesis", or "json"

-- TUI input
quorum.config.set("tui.input.submit_key", "enter")
quorum.config.set("tui.input.newline_key", "shift+enter")
quorum.config.set("tui.input.editor_key", "I")
quorum.config.set("tui.input.editor_action", "return_to_insert")  -- or "submit"
quorum.config.set("tui.input.max_height", 10)
quorum.config.set("tui.input.dynamic_height", true)
quorum.config.set("tui.input.context_header", true)

-- TUI layout
quorum.config.set("tui.layout.preset", "default")      -- "default", "minimal", "wide", "stacked"
-- quorum.config.set("tui.layout.flex_threshold", 120)

-- Provider configuration
quorum.providers.set_default("copilot")
-- quorum.providers.route("claude-sonnet-4.6", "bedrock")
-- quorum.providers.bedrock({ region = "us-west-2", profile = "dev-ai" })
-- quorum.providers.anthropic({ api_key = os.getenv("ANTHROPIC_API_KEY") })
-- quorum.providers.openai({ api_key = os.getenv("OPENAI_API_KEY") })

-- Custom tools
-- quorum.tools.register("gh_issue", {
--     description = "Create a GitHub issue",
--     command = "gh issue create --title {title} --body {body}",
--     risk_level = "high",
--     parameters = {
--         title = { type = "string", description = "Issue title", required = true },
--         body = { type = "string", description = "Issue body", required = true },
--     }
-- })

-- TUI routes and surfaces (via quorum.tui.* API)
-- quorum.tui.routes.set("tool_log", "sidebar")
-- quorum.tui.routes.set("notification", "status_bar")
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
- `ScriptingEnginePort` (application/ports) - Lua scripting engine interface

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
├── config/         # OutputFormat
└── scripting/      # ScriptEventType, ScriptEventData, ScriptValue
```

### Adding New Features

- New LLM provider: Add to `infrastructure/` implementing `LlmGateway`
- New orchestration strategy: Add to `domain/orchestration/`
- New output format: Add to `presentation/output/`
- New model: Add variant to `domain/src/core/model.rs` Model enum
- New tool: Add to `infrastructure/tools/`, register in `default_tool_spec()`
- Custom tool: Register via `quorum.tools.register()` in `init.lua`
- New agent capability: Extend `domain/agent/` and `RunAgentUseCase`
- New context file type: Add to `domain/context/` KnownContextFile enum
- New Lua API: Add to `infrastructure/scripting/`, register in `LuaScriptingEngine::new()`

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
- `domain/agent/`: AgentState, Plan, Task, ModelConfig (exploration/decision/review + participants/moderator/ask), AgentPolicy, HilAction
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

## Lua Scripting (Phase 1–3)

User config via `~/.config/copilot-quorum/init.lua`, loaded at startup. Feature-gated behind `scripting` (default ON).

**Lua APIs**:
- `quorum.on(event, callback)` — Event subscription (ScriptLoading, ScriptLoaded, ConfigChanged, ModeChanged, SessionStarted, ToolCallBefore, ToolCallAfter, PhaseChanged, PlanCreated)
- `quorum.config.get(key)` / `quorum.config.set(key, value)` / `quorum.config.keys()` — Runtime config access via `ConfigAccessorPort` (20 keys, all read-write)
- `quorum.config["key"]` — Metatable proxy (`__index`/`__newindex`)
- `quorum.keymap.set(mode, key, action)` — Custom keybindings (mode: normal/insert/command, action: string or Lua callback)
- `quorum.command.register(name, opts)` — User-defined slash commands (opts: fn, description, usage)

**Plugin System** (Phase 3):
- `~/.config/copilot-quorum/plugins/*.lua` auto-loaded in alphabetical order after init.lua
- Number prefix for ordering: `01_core.lua`, `02_lsp.lua` (Vim native packages style)
- Directory absence: silent skip. Individual file failure: `eprintln!` warning, continue loading

**Agent Events** (Phase 3):
- `ToolCallBefore` (cancellable) — Lua returns false to skip tool execution (before HiL)
- `ToolCallAfter` — tool_name, success, duration_ms, output_preview/error
- `PhaseChanged` — phase name as string
- `PlanCreated` — objective, task_count
- `CompositeProgressNotifier<'a>` — borrowed refs, delegates to TUI + ScriptProgressBridge
- `ScriptProgressBridge` — maps AgentProgressNotifier → ScriptingEnginePort::emit_event()
- ToolCallBefore: Vim BufWritePre timing + BufWriteCmd cancel = hybrid design

**Config Key Sections** (all mutable at runtime):
- `agent.*` — consensus_level, phase_scope, strategy, hil_mode, max_plan_revisions
- `models.*` — exploration, decision, review, participants, moderator, ask
- `execution.*` — max_iterations, max_tool_turns
- `output.*` — format, color
- `repl.*` — show_progress, history_file
- `context_budget.*` — max_entry_bytes, max_total_bytes, recent_full_count

**Key Components**:
- `domain/scripting/`: ScriptEventType (11 events), ScriptEventData, ScriptValue
- `application/ports/scripting_engine.rs`: ScriptingEnginePort trait, NoScriptingEngine, EventOutcome, KeymapAction
- `application/ports/composite_progress.rs`: CompositeProgressNotifier<'a> (borrowed delegate pattern)
- `application/ports/script_progress_bridge.rs`: ScriptProgressBridge (progress → scripting events)
- `infrastructure/scripting/`: LuaScriptingEngine (mlua), EventBus, ConfigAPI, KeymapAPI, CommandAPI, Sandbox
- `presentation/tui/mode.rs`: KeyAction::LuaCallback, CustomKeymap, `parse_key_descriptor()`
- `cli/main.rs`: DI wiring with `Arc<Mutex<QuorumConfig>>` shared between AgentController and LuaScriptingEngine

**Sandbox**: C module loading blocked (`package.loadlib = nil`, `package.cpath = ""`), standard Lua libs available.

**Shared Config**: `AgentController` and `LuaScriptingEngine` share `Arc<Mutex<QuorumConfig>>` — Lua config changes propagate to agent at runtime.

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
| [scripting-system.md](docs/systems/scripting-system.md) | Lua Scripting（Plugin、Commands、Agent Events） |
| [vision/](docs/vision/README.md) | 将来ビジョン（Knowledge、Workflow、Extension） |
