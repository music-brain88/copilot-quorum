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

# Run in interactive mode (TUI)
cargo run -p copilot-quorum -- -m claude-haiku-4.5

# Initialize project context (generates .quorum/context.md)
cargo run -p copilot-quorum -- /init
```

## Debugging the TUI (Remote Control API)

Claude Code (や他のコーディングエージェント) から実際に TUI を起動して「見て・操作して・検証する」ためのワークフロー。
TUI のバグ再現・修正確認はコードリーディングだけで済ませず、この方法で実機確認すること。

```bash
# 1. Build first, then launch headless with a JSON-RPC socket (TTY 不要 — #303)
#    ソケットパスは短く (Unix socket の SUN_LEN ~108 文字制限。長いと "path must be shorter than SUN_LEN" で落ちる)
cargo build
./target/debug/copilot-quorum --headless --listen /tmp/quorum-dbg.sock &

# 2. Drive it with the built-in client (method 一覧は rpc.discover か docs/reference/tui-remote-control.md — #302)
./target/debug/copilot-quorum rpc --socket /tmp/quorum-dbg.sock state.get                          # モード・モデル・flash 等
./target/debug/copilot-quorum rpc --socket /tmp/quorum-dbg.sock command.exec '{"command": "init!"}' # `:` コマンド実行
./target/debug/copilot-quorum rpc --socket /tmp/quorum-dbg.sock input.send '{"text": "Fix the bug"}'
./target/debug/copilot-quorum rpc --socket /tmp/quorum-dbg.sock hil.respond '{"decision": "approve"}'

# 3. "See" the screen — ユーザーが見ているものと同じ画面をオフスクリーン描画で取得
./target/debug/copilot-quorum rpc --socket /tmp/quorum-dbg.sock screen.capture '{"width": 140, "height": 40}' \
  | python3 -c "import json,sys; print('\n'.join(json.load(sys.stdin)['lines']))"
./target/debug/copilot-quorum rpc --socket /tmp/quorum-dbg.sock pane.read '{"last": 5}'            # 会話ログを構造化 JSON で

# 4. Clean up (:qa! は常にアプリ終了。:q! はタブが複数あるとタブクローズになるので注意)
./target/debug/copilot-quorum rpc --socket /tmp/quorum-dbg.sock command.exec '{"command": "qa!"}'
```

`scripts/tui-rpc.py` は同じワイヤ形式(LSP Content-Length + JSON-RPC 2.0)を話す
プロトコル参照実装として引き続き存在する。`copilot-quorum rpc` が使えない環境
(ビルド前など)ではそちらでも同じソケットを操作できる。

Tips:
- `rpc.discover` で今呼べる全メソッド(params スキーマ + 説明)、`commands.list` /
  `keymaps.list` で `:` コマンド・キーバインド一覧、`config.keys` / `config.get` /
  `config.set` で設定の読み書きができる(TUI の `:config` や Lua と同じ挙動 — #302)
- `screen.capture` を実行前後で取れば「修正が画面にどう反映されたか」を diff で検証できる
- LLM 呼び出しを伴う操作 (`init!` 等) は非同期。完了は `pane.read` のメッセージ (例: "Context saved to") をポーリングして検知する
- `keys.feed` はキーボードと同一のディスパッチ経路なので、キーバインドや HiL モーダルのデバッグにも使える
- `--headless` は `--listen` 必須（付けないと起動時に clap がエラーで止める）。`:q!`/`:qa` に加え SIGINT/SIGTERM でも graceful shutdown する

## Configuration

Configuration via Lua: `~/.config/copilot-quorum/init.lua` (plugins in `plugins/*.lua`)

Boot sequence: Rust defaults → Lua (init.lua + plugins) → CLI arg overrides

```lua
-- ~/.config/copilot-quorum/init.lua

-- Role-based model selection
quorum.config.set("models.exploration", "gpt-5.3-codex")       -- Context gathering + low-risk tools
quorum.config.set("models.decision", "claude-sonnet-4.5")      -- Planning + high-risk tools
quorum.config.set("models.review", { "claude-opus-4.5", "gpt-5.3-codex", "gemini-3.1-pro-preview" })
-- quorum.config.set("models.participants", { "claude-opus-4.5", "gpt-5.3-codex" })  -- Quorum Discussion
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
      presentation/             # CLI, TUI, output formatters
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
| presentation | `quorum-presentation` | CLI commands, TuiApp, ConsoleFormatter, ProgressReporter |
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
- `quorum.on(event, callback)` — Event subscription (ScriptLoading, ScriptLoaded, ConfigChanged, ModeChanged, SessionStarted, ToolCallBefore, ToolCallAfter, PhaseChanged, PlanCreated, QuorumResult)
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
- `QuorumResult` — topic, approved, approve_count, reject_count, api_version, rule, task_id?, tool?, feedback?, votes_json（`EventPublisher` 継ぎ目経由。progress bridge ではない）
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
- `domain/scripting/`: ScriptEventType (12 events), ScriptEventData, ScriptValue
- `application/ports/scripting_engine.rs`: ScriptingEnginePort trait, NoScriptingEngine, EventOutcome, KeymapAction
- `application/ports/composite_progress.rs`: CompositeProgressNotifier<'a> (borrowed delegate pattern)
- `application/ports/script_progress_bridge.rs`: ScriptProgressBridge (progress → scripting events)
- `application/ports/event_publisher.rs`: EventPublisher（typed イベントの継ぎ目 — `AppEvent::QuorumResult` を JSONL + Lua にファンアウト。将来の Application/Interaction Event Bus はこの port の impl 差し替えで導入）
- `infrastructure/scripting/`: LuaScriptingEngine (mlua), EventBus, ConfigAPI, KeymapAPI, CommandAPI, Sandbox
- `presentation/tui/mode.rs`: KeyAction::LuaCallback, CustomKeymap, `parse_key_descriptor()`
- `cli/main.rs`: DI wiring with `Arc<Mutex<QuorumConfig>>` shared between AgentController and LuaScriptingEngine

**Sandbox**: C module loading blocked (`package.loadlib = nil`, `package.cpath = ""`), standard Lua libs available.

**Shared Config**: `AgentController` and `LuaScriptingEngine` share `Arc<Mutex<QuorumConfig>>` — Lua config changes propagate to agent at runtime.

## Feature Documentation

ドキュメントは [Diátaxis](https://diataxis.fr/) 構成（[docs/README.md](docs/README.md) がハブ）:

**Tutorials（学習向け）**

| Document | Description |
|----------|-------------|
| [getting-started.md](docs/tutorials/getting-started.md) | ビルド → 最初の質問 → `/council` 体験 |
| [first-agent-task.md](docs/tutorials/first-agent-task.md) | Agent ライフサイクル + HiL 体験 |
| [customizing-with-lua.md](docs/tutorials/customizing-with-lua.md) | init.lua・プラグイン・カスタムツール入門 |

**How-to Guides（タスク指向）**

| Document | Description |
|----------|-------------|
| [run-a-quorum-discussion.md](docs/how-to/run-a-quorum-discussion.md) | Quorum Discussion の実行 |
| [review-a-pr.md](docs/how-to/review-a-pr.md) | `review` サブコマンドで PR/diff をヘッドレスレビュー（CI ゲート、#300） |
| [use-ensemble-mode.md](docs/how-to/use-ensemble-mode.md) | Ensemble モードへの切り替え |
| [run-agent-tasks.md](docs/how-to/run-agent-tasks.md) | エージェント実行と HiL 操作 |
| [use-the-tui.md](docs/how-to/use-the-tui.md) | Modal TUI の使い方 |
| [manage-project-context.md](docs/how-to/manage-project-context.md) | `/init` とコンテキスト予算 |
| [add-custom-tools.md](docs/how-to/add-custom-tools.md) | `quorum.tools.register` でツール追加 |
| [write-lua-plugins.md](docs/how-to/write-lua-plugins.md) | Lua プラグイン・コマンド・フック |
| [debug-with-logs.md](docs/how-to/debug-with-logs.md) | ログの有効化と使い分け |
| [extend-the-codebase.md](docs/how-to/extend-the-codebase.md) | プロバイダー・戦略・ツールの追加（Rust） |

**Reference（情報指向）**

| Document | Description |
|----------|-------------|
| [architecture.md](docs/reference/architecture.md) | レイヤー構造・データフロー・プロトコル |
| [cli.md](docs/reference/cli.md) | CLI フラグ・REPL/TUI コマンド・`review` サブコマンド（headless 多モデル PR/diff レビュー、#300） |
| [configuration.md](docs/reference/configuration.md) | 全設定キーと Lua API |
| [agent-system.md](docs/reference/agent-system.md) | Agent の型・ポート・モジュール構成 |
| [tool-system.md](docs/reference/tool-system.md) | ツール・プロバイダー・trait |
| [native-tool-use.md](docs/reference/native-tool-use.md) | Native Tool Use API |
| [orchestration-internals.md](docs/reference/orchestration-internals.md) | Quorum / Ensemble の内部構造 |
| [transport.md](docs/reference/transport.md) | Transport Demultiplexer |
| [logging.md](docs/reference/logging.md) | ログシステム（ConversationLogger、JSONL） |
| [scripting.md](docs/reference/scripting.md) | Lua Scripting（イベント一覧、Sandbox） |
| [tui-internals.md](docs/reference/tui-internals.md) | TUI の Actor パターン・ルーティング |
| [tui-remote-control.md](docs/reference/tui-remote-control.md) | Remote Control API（`--listen`） |

**Explanation（理解指向）**

| Document | Description |
|----------|-------------|
| [quorum-consensus.md](docs/explanation/quorum-consensus.md) | Quorum Discussion & Consensus の仕組み |
| [ensemble-mode.md](docs/explanation/ensemble-mode.md) | Ensemble Mode（研究エビデンス付き） |
| [orchestration-axes.md](docs/explanation/orchestration-axes.md) | 3 直交軸の設計 |
| [agent-behavior.md](docs/explanation/agent-behavior.md) | Agent ライフサイクル + HiL |
| [interaction-model.md](docs/explanation/interaction-model.md) | InteractionForm、ContextMode、ネスティング |
| [design-philosophy.md](docs/explanation/design-philosophy.md) | DDD + オニオン + 垂直分割の理由 |
| [tui-design.md](docs/explanation/tui-design.md) | TUI 設計思想 |
| [transport-and-concurrency.md](docs/explanation/transport-and-concurrency.md) | 多重分離の理由と並行パターン |
| [design-decisions/](docs/explanation/design-decisions/README.md) | 設計決定記録（ADR、Discussions 由来） |
| [vision/](docs/vision/README.md) | 将来ビジョン（Knowledge、Workflow、Extension） |
