# Documentation Hub / ドキュメントハブ

> Documentation for copilot-quorum
>
> copilot-quorum のドキュメント

---

## Documents / ドキュメント一覧

| Document | Description |
|----------|-------------|
| [README](../README.md) | プロジェクト概要・クイックスタート |
| [Architecture](./reference/architecture.md) | DDD + オニオンアーキテクチャの詳細 |

---

## Concepts / 概念

| Document | Description |
|----------|-------------|
| [Quorum Discussion & Consensus](./concepts/quorum.md) | 複数モデルによる議論と合意形成 |
| [Ensemble Mode](./concepts/ensemble-mode.md) | 研究に基づいたマルチモデル計画生成 |
| [Interaction Model](./concepts/interaction-model.md) | インタラクションモデルと会話フロー |

---

## Guides / ガイド

| Document | Description |
|----------|-------------|
| [CLI & Configuration](./guides/cli-and-configuration.md) | REPL コマンド、設定、コンテキスト管理 |
| [Modal TUI](./guides/tui.md) | Neovim ライクなモーダルインターフェース |

---

## Systems / システム

| Document | Description |
|----------|-------------|
| [Agent System](./systems/agent-system.md) | 自律タスク実行と Human-in-the-Loop |
| [Tool System](./systems/tool-system.md) | プラグインベースのツールアーキテクチャ |
| [Native Tool Use](./systems/native-tool-use.md) | Native Tool Use API による構造化ツール呼び出し |
| [Transport Demultiplexer](./systems/transport.md) | 並列セッションのメッセージルーティング |
| [Logging](./systems/logging.md) | JSONL 会話ログとデバッグ |

---

## Vision / ビジョン

| Document | Description |
|----------|-------------|
| [Vision Overview](./vision/README.md) | copilot-quorum の将来ビジョン |
| [Knowledge Architecture](./vision/knowledge-architecture.md) | 知識アーキテクチャ構想 |
| [Workflow Layer](./vision/workflow-layer.md) | ワークフローレイヤー構想 |
| [Extension Platform](./vision/extension-platform.md) | 拡張プラットフォーム構想 |

---

## Reading Guide / 読み順ガイド

### For Users / ユーザー向け

1. **[README](../README.md)** - インストールと基本的な使い方
2. **[CLI & Configuration](./guides/cli-and-configuration.md)** - 設定とコマンド
3. **[Modal TUI](./guides/tui.md)** - モーダル TUI の使い方
4. **[Quorum](./concepts/quorum.md)** - 合議の仕組み
5. **[Agent System](./systems/agent-system.md)** - エージェントの動作

### For Contributors / コントリビューター向け

1. **[README](../README.md)** - プロジェクト概要
2. **[Architecture](./reference/architecture.md)** - レイヤー構造とデータフロー
3. **[Quorum](./concepts/quorum.md)** - コアコンセプトの理解
4. **[Tool System](./systems/tool-system.md)** - ツール追加方法
5. **[Native Tool Use](./systems/native-tool-use.md)** - Native API によるツール呼び出し
6. **[Ensemble Mode](./concepts/ensemble-mode.md)** - 設計判断の背景
7. **[Transport Demultiplexer](./systems/transport.md)** - 並列セッションの仕組み
8. **[Agent System](./systems/agent-system.md)** - エージェントアーキテクチャ
9. **[TUI Design Philosophy](./reference/architecture.md#tui-design-philosophy--tui-設計思想)** - TUI 設計思想

---

## Project Structure / プロジェクト構造

```
copilot-quorum/
├── domain/             # ドメイン層 - ビジネスロジックの核心
│   ├── core/           # Model, Question, Error, NonEmptyString
│   ├── quorum/         # Vote, QuorumRule, ConsensusRound
│   ├── orchestration/  # ConsensusLevel, PhaseScope, OrchestrationStrategy, SessionMode, StrategyExecutor
│   ├── agent/          # AgentState, Plan, Task, ModelConfig, AgentPolicy
│   ├── tool/           # ToolDefinition, ToolCall, ToolSpec, ToolResult
│   ├── prompt/         # PromptTemplate, AgentPromptTemplate
│   ├── session/        # Message, LlmResponse, ContentBlock, StopReason
│   ├── context/        # ProjectContext, KnownContextFile
│   ├── config/         # OutputFormat
│   └── interaction/    # InteractionMode
│
├── application/        # アプリケーション層 - ユースケース
│   ├── ports/          # LlmGateway, LlmSession, ProgressNotifier, ToolExecutorPort, ToolSchemaPort
│   ├── use_cases/      # RunQuorumUseCase, RunAgentUseCase, AgentController, RunAskUseCase
│   └── config/         # ExecutionParams, QuorumConfig
│
├── infrastructure/     # インフラ層 - 外部システム連携
│   ├── copilot/        # CopilotLlmGateway, CopilotSession, MessageRouter
│   ├── tools/          # LocalToolExecutor, ToolRegistry, providers (builtin, cli, custom), schema
│   ├── config/         # FileConfig, ConfigLoader
│   ├── context/        # LocalContextLoader
│   ├── reference/      # GitHub reference resolver
│   └── logging/        # JSONL logger
│
├── presentation/       # プレゼンテーション層 - UI
│   ├── cli/            # CLI commands (CLAP)
│   ├── tui/            # Modal TUI (app, state, mode, widgets, editor, event)
│   ├── agent/          # AgentPresenter, progress, thought
│   ├── output/         # ConsoleFormatter
│   └── progress/       # ProgressReporter
│
├── cli/                # エントリポイント (DI構築)
├── docs/               # ドキュメント
├── examples/           # 使用例
└── tests/              # 統合テスト
```

---

## External Links / 外部リンク

- [Original copilot-council (Go)](https://github.com/openjny/copilot-council)
- [GitHub Copilot CLI](https://docs.github.com/en/copilot/github-copilot-in-the-cli)
- [Domain-Driven Design Reference](https://www.domainlanguage.com/ddd/reference/)
- [The Onion Architecture](https://jeffreypalermo.com/2008/07/the-onion-architecture-part-1/)
