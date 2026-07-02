# Architecture / アーキテクチャ

> Technical deep-dive into copilot-quorum
>
> copilot-quorumの技術的な詳細

---

## Overview / 概要

copilot-quorum は **DDD (Domain-Driven Design) + オニオンアーキテクチャ** を採用しています。
これにより、ビジネスロジックを外部依存から分離し、高い拡張性とテスト容易性を実現しています。

---

## Core Concepts / コア概念

コア概念の詳細は explanation 配下の各ドキュメントを参照してください。ここでは要点のみまとめます。

### Quorum

分散システムの Quorum System に着想を得たマルチ LLM オーケストレーション。
**Quorum Discussion**（意見収集）/ **Quorum Consensus**（投票承認）/ **Quorum Synthesis**（統合）の 3 側面を持ちます。
→ [Quorum Discussion & Consensus](../explanation/quorum-consensus.md)

### Solo / Ensemble モード

`ConsensusLevel` により単一モデル駆動（Solo、デフォルト）とマルチモデル駆動（Ensemble）を切り替えます。
ML のアンサンブル学習に相当する「独立生成 + 投票」方式です。
→ [Ensemble Mode](../explanation/ensemble-mode.md)

### 3 つの直交する設定軸

実行は `ConsensusLevel`（誰が参加するか）× `PhaseScope`（どこまで実行するか）×
`OrchestrationStrategy`（どう議論するか）の 3 つの独立した軸で制御されます。
→ [Orchestration Axes](../explanation/orchestration-axes.md)

### Interaction Model

`Agent` / `Ask` / `Discuss` は対等なインタラクション形式（`InteractionForm`）で、
`InteractionTree` により最大深度 3 までネスト可能です。
→ [Interaction Model](../explanation/interaction-model.md)

### Quorum Layers（将来ビジョン）

Decision Quorum（現在）→ Context Quorum → Knowledge Quorum の 3 層構想。
→ [Vision & Roadmap](../vision/README.md)

---


## Layer Structure / レイヤー構成

```
copilot-quorum/
├── domain/              # ドメイン層 - ビジネスロジックの核心
│   ├── core/            # 共通ドメイン概念 (Model, Question, Error)
│   ├── session/         # LLMセッションドメイン
│   ├── orchestration/   # Quorumオーケストレーションドメイン
│   ├── agent/           # エージェント自律実行ドメイン
│   ├── tool/            # ツール定義・実行ドメイン
│   ├── interaction/     # インタラクション形式・ネスト管理
│   ├── context/         # プロジェクトコンテキスト・リソース参照
│   ├── quorum/          # 合意形成（Vote, QuorumRule）
│   ├── prompt/          # プロンプトドメイン
│   └── config/          # 設定ドメイン
│
├── application/         # アプリケーション層 - ユースケース
│   ├── ports/           # ポート定義（11トレイト）
│   ├── use_cases/       # ユースケース
│   └── config/          # QuorumConfig, ExecutionParams
│
├── infrastructure/      # インフラ層 - 技術的実装
│   ├── copilot/         # Copilot CLIアダプター (Gateway, Router, Session)
│   ├── tools/           # ToolRegistry, プロバイダー群, Schema変換
│   ├── context/         # LocalContextLoader
│   ├── logging/         # JsonlConversationLogger
│   ├── reference/       # GitHubReferenceResolver
│   └── config/          # FileConfig, ConfigLoader
│
├── presentation/        # プレゼンテーション層 - UI
│   ├── cli/             # CLIコマンド定義
│   ├── tui/             # モーダル TUI (ratatui)
│   ├── agent/           # Agent UI コンポーネント
│   ├── output/          # 出力フォーマッター
│   ├── progress/        # プログレス表示
│   └── config/          # OutputConfig, ReplConfig
│
└── cli/                 # エントリポイント (DI構築)
```

### Dependency Flow (Onion Structure) / 依存の方向

```
                cli/
                  |
           presentation/
                  |
    infrastructure/ --> application/
            |                |
            +----> domain/ <-+
```

- **domain/** : 依存なし（純粋なビジネスロジック）
- **application/** : domainのみに依存
- **infrastructure/** : domain + applicationのトレイトを実装
- **presentation/** : domain + applicationに依存
- **cli/** : 全てに依存（DI構築）

---

## Domain Layer / ドメイン層

ビジネスロジックの核心。外部依存は一切なし。

### Core Module

`domain/src/core/`

| Type | Kind | Description |
|------|------|-------------|
| `Model` | Value Object | 利用可能なAIモデル（Claude, GPT, Gemini等） |
| `Question` | Value Object | Quorumに投げかける質問 |
| `DomainError` | Error | ドメインレベルのエラー |

### Quorum Module

`domain/src/quorum/`

Quorum（合意形成）に関する型を定義します。

| Type | Kind | Description |
|------|------|-------------|
| `Vote` | Value Object | モデルからの投票（承認/却下 + 理由） |
| `VoteResult` | Value Object | 投票結果の集計 |
| `QuorumRule` | Value Object | 合意ルール（過半数、全会一致など） |
| `ConsensusRound` | Entity | 投票ラウンドの記録 |
| `ConsensusOutcome` | Value Object | 結果（Approved, Rejected, Pending） |

### Session Module

`domain/src/session/`

| Type | Kind | Description |
|------|------|-------------|
| `Session` | Entity | LLMとの会話セッション |
| `Message` | Entity | 会話内のメッセージ |
| `LlmSessionRepository` | Trait | セッション管理の抽象化 |
| `LlmResponse` | Value Object | LLM からの構造化レスポンス（ContentBlock のリスト） |
| `ContentBlock` | Enum | Text / ToolUse / ToolResult |
| `StopReason` | Enum | EndTurn / ToolUse / MaxTokens / StopSequence |
| `StreamEvent` | Enum | Delta / Completed / Error / ToolCallDelta / CompletedResponse |

### Orchestration Module

`domain/src/orchestration/`

#### 3つの直交する設定軸

| 軸 | 型 | バリアント | 説明 |
|----|------|-----------|------|
| **ConsensusLevel** | Enum | `Solo` (default), `Ensemble` | 参加モデル数を制御（単一 or 複数） |
| **PhaseScope** | Enum | `Full` (default), `Fast`, `PlanOnly` | 実行フェーズの範囲を制御 |
| **OrchestrationStrategy** | Enum | `Quorum(QuorumConfig)`, `Debate(DebateConfig)` | 議論の進め方を選択 |

これらは直交しており、任意の組み合わせが可能です（例: `Solo + Fast + Debate`）。

#### 設定コンテナ

| Type | Kind | Description |
|------|------|-------------|
| `SessionMode` | Value Object | ランタイム可変設定（consensus_level, phase_scope, strategy） |
| `PlanningApproach` | Enum (派生) | `ConsensusLevel` から自動導出（Solo→Single, Ensemble→Ensemble） |

#### Quorum Discussion 型

| Type | Kind | Description |
|------|------|-------------|
| `Phase` | Value Object | フェーズ（Initial, Review, Synthesis） |
| `QuorumRun` | Entity | 実行中のQuorumセッション |
| `ModelResponse` | Value Object | モデルからの回答 |
| `PeerReview` | Value Object | ピアレビュー結果 |
| `SynthesisResult` | Value Object | 最終統合結果 |
| `QuorumResult` | Value Object | 全フェーズの結果 |
| `StrategyExecutor` | Trait | オーケストレーション戦略の実行インターフェース |

### Interaction Module

`domain/src/interaction/`

ユーザーとシステムの対話を3つの対等な形式で表現するモジュールです。

| Type | Kind | Description |
|------|------|-------------|
| `InteractionForm` | Enum | Agent / Ask / Discuss — 対話形式 |
| `InteractionId` | Value Object | インタラクションの一意識別子 |
| `Interaction` | Entity | 対話インスタンス（form, context_mode, depth） |
| `InteractionTree` | Entity | ネスト管理のツリー構造（ID自動採番） |
| `InteractionResult` | Enum | AskResult / DiscussResult / AgentResult |
| `SpawnError` | Error | 子インタラクション生成エラー（ParentNotFound, MaxDepthExceeded） |
| `DEFAULT_MAX_NESTING_DEPTH` | Const | 最大ネスト深度（= 3） |

`InteractionForm` は各形式がどの設定型を使うかを決定します：
- `uses_agent_policy()` — Agent のみ true
- `uses_execution_params()` — Agent と Ask が true
- `default_context_mode()` — Agent/Discuss → Full, Ask → Projected

### Agent Module

`domain/src/agent/`

| Type | Kind | Description |
|------|------|-------------|
| `AgentState` | Entity | エージェント実行の現在状態 |
| `AgentPhase` | Enum | ContextGathering / Planning / PlanReview / Executing / FinalReview / Completed |
| `SessionMode` | Value Object | ランタイム可変オーケストレーション設定 |
| `ModelConfig` | Value Object | ロールベースモデル選択（exploration, decision, review） |
| `AgentPolicy` | Value Object | ドメイン動作制約（HiL、レビュー設定） |
| `HilMode` | Enum | Interactive / AutoApprove / AutoReject |
| `HumanDecision` | Enum | Approve / Reject / Edit(Plan) |
| `Plan` | Value Object | タスク計画（目的、理由付け、タスクリスト） |
| `Task` | Value Object | 単一タスク（ツール呼び出し、依存関係） |
| `EnsemblePlanResult` | Entity | Ensemble 計画の結果（候補リスト + 選択） |
| `PlanCandidate` | Value Object | Ensemble 候補プラン（モデル + スコア） |
| `ReviewRound` | Entity | レビューラウンドの記録（投票リスト + 承認結果） |
| `ModelVote` | Value Object | モデルからの投票（モデル名、承認/却下、フィードバック） |
| `ToolExecution` | Entity | ツール実行のライフサイクル追跡（Pending → Running → Completed/Error） |
| `ToolExecutionState` | Enum | ツール実行の状態マシン |
| `ConfigIssue` | Value Object | 設定バリデーション問題 |

#### ToolExecution State Machine

```
Pending ──> Running ──> Completed
                   └──> Error
```

`ToolExecution` は各ツール呼び出しのライフサイクルを追跡します。
状態遷移は enum ベースで、各状態に有効なフィールドのみを持ちます。

### Tool Module

`domain/src/tool/`

| Type | Kind | Description |
|------|------|-------------|
| `ToolDefinition` | Entity | ツールのメタデータ（名前、説明、リスクレベル） |
| `ToolParameter` | Value Object | ツールパラメータの定義 |
| `ToolCall` | Value Object | ツール呼び出し（引数付き、`native_id` でAPI相関） |
| `ToolResult` | Value Object | 実行結果（成功/失敗、出力） |
| `ToolResultMetadata` | Value Object | 実行メタデータ（duration_ms, bytes, path, exit_code, match_count） |
| `ToolSpec` | Entity | 利用可能なツールのレジストリ |
| `RiskLevel` | Enum | Low（読み取り専用）/ High（変更あり） |
| `ToolValidator` | Trait | ツール呼び出しのバリデーションロジック |
| `ToolProvider` | Trait | 外部ツールプロバイダーの抽象化 |

### Context Module

`domain/src/context/`

| Type | Kind | Description |
|------|------|-------------|
| `ProjectContext` | Entity | プロジェクトの統合コンテキスト |
| `KnownContextFile` | Value Object | 既知のコンテキストファイル種別（CLAUDE.md, README.md等） |
| `LoadedContextFile` | Value Object | 読み込まれたファイルの内容 |
| `ContextMode` | Enum | Full / Projected / Fresh — コンテキスト投影モード |
| `ResourceReference` | Enum | GitHubIssue / GitHubPullRequest — テキスト中のリソース参照 |
| `extract_references()` | Function | テキストからリソース参照を抽出 |

#### ContextMode

タスクやインタラクションに渡すコンテキスト量を制御します。Vim のバッファコマンドのアナロジーです：

| Mode | 渡すコンテキスト | Vim アナロジー |
|------|-----------------|----------------|
| `Full` | 全 `AgentContext` | `:split` — 同じバッファを共有 |
| `Projected` | タスクの `context_brief` のみ | `:edit` — 特定ファイルを開く |
| `Fresh` | なし | `:enew` — 空バッファで開始 |

#### ResourceReference

テキスト中の GitHub Issue/PR 参照を自動検出します：

- GitHub URL: `github.com/{owner}/{repo}/(issues|pull)/{N}`
- クロスリポ参照: `{owner}/{repo}#{N}`
- 型付き参照: `Issue #N`, `PR #N`, `Pull Request #N`
- 範囲参照: `#N-M`（M-N <= 10）
- ベア参照: `#N`

### Prompt Module

`domain/src/prompt/`

| Type | Kind | Description |
|------|------|-------------|
| `PromptTemplate` | Service | 各フェーズのプロンプトテンプレート |
| `AgentPromptTemplate` | Service | エージェント専用プロンプト（system, plan, review） |

### Config Module

`domain/src/config/`

| Type | Kind | Description |
|------|------|-------------|
| `OutputFormat` | Enum | Full / Synthesis / Json |

---

## Application Layer / アプリケーション層

ユースケースとポート（外部インターフェース）を定義。

### Ports (Interfaces) / ポート

| Trait | Module | Description |
|-------|--------|-------------|
| `LlmGateway` | `llm_gateway` | LLMプロバイダーへのゲートウェイ |
| `LlmSession` | `llm_gateway` | アクティブなLLMセッション（send, send_with_tools, send_tool_results） |
| `ToolExecutorPort` | `tool_executor` | ツール実行の抽象化 |
| `ToolSchemaPort` | `tool_schema` | ツール定義 → JSON Schema 変換の抽象化 |
| `ContextLoaderPort` | `context_loader` | コンテキストファイル読み込みの抽象化 |
| `ProgressNotifier` | `progress` | Quorum 進捗通知コールバック |
| `AgentProgressNotifier` | `agent_progress` | エージェント進捗通知コールバック |
| `HumanInterventionPort` | `human_intervention` | 人間介入の抽象化（プラン承認/却下/編集、実行確認） |
| `ActionReviewer` | `action_reviewer` | 高リスクツール呼び出しのレビュー抽象化 |
| `ConversationLogger` | `conversation_logger` | 構造化会話ログの記録（JSONL） |
| `ReferenceResolverPort` | `reference_resolver` | リソース参照の解決（GitHub Issue/PR → コンテンツ） |
| `UiEvent` | `ui_event` | アプリケーション → プレゼンテーション層への出力イベント |

#### UiEvent（出力ポート）

`UiEvent` は `AgentController` からプレゼンテーション層へのイベントチャネルです。
Welcome, ModeChanged, AgentResult, QuorumResult, AskResult, InteractionSpawned, InteractionCompleted 等の
バリアントで、UI の種類（CLI, TUI, 将来の Web）に依存しない形でイベントを伝達します。

### Use Cases / ユースケース

| Type | Module | Description |
|------|--------|-------------|
| `RunAgentUseCase` | `run_agent/` | エージェント自律実行（Phase 1-5 全体オーケストレーション） |
| `RunQuorumUseCase` | `run_quorum` | Quorum（合議）実行 |
| `RunAskUseCase` | `run_ask` | Ask インタラクション（読み取り専用ツールでの Q&A） |
| `GatherContextUseCase` | `gather_context` | Phase 1: コンテキスト収集（3段階フォールバック） |
| `ExecuteTaskUseCase` | `execute_task` | Phase 4: タスク実行（動的モデル選択 + アクションレビュー） |
| `InitContextUseCase` | `init_context` | コンテキストファイル（.quorum/context.md）の初期化 |
| `AgentController` | `agent_controller` | REPL/TUI のビジネスロジック。コマンド処理、UiEvent 発信 |

#### run_agent/ ファイル分割

`RunAgentUseCase` は責務ごとに4ファイルに分割されています：

| File | Description |
|------|-------------|
| `mod.rs` | メインオーケストレーション（Phase 1-5 フロー） |
| `types.rs` | `RunAgentInput`, `RunAgentOutput`, `RunAgentError` |
| `planning.rs` | Solo/Ensemble 計画生成ロジック |
| `review.rs` | Quorum プランレビュー + アクションレビュー |
| `hil.rs` | 人間介入ハンドリング（execution confirmation 含む） |

### Config / 設定

| Type | Module | Description |
|------|--------|-------------|
| `QuorumConfig` | `config/quorum_config` | 4型コンテナ（SessionMode, ModelConfig, AgentPolicy, ExecutionParams） |
| `ExecutionParams` | `config/execution_params` | max_iterations, max_tool_turns, max_tool_retries, working_dir, ensemble_session_timeout |

`QuorumConfig` はバッファ伝搬のための統合コンテナで、`mode_mut()` でランタイム変更可能。
`to_agent_input()`, `to_quorum_input()` でユースケース入力を生成します。

---

## Infrastructure Layer / インフラ層

アプリケーション層のポートを実装するアダプター。

### Copilot Adapter

`infrastructure/src/copilot/`

| Type | Implements | Description |
|------|------------|-------------|
| `CopilotLlmGateway` | `LlmGateway` | Copilot CLI経由のLLMゲートウェイ |
| `CopilotSession` | `LlmSession` | Copilotセッション（send_with_tools, send_tool_results） |
| `MessageRouter` | - | TCP demultiplexer（セッション間メッセージルーティング） |
| `SessionChannel` | - | セッション専用の受信チャネル（Drop時に自動登録解除） |
| `CopilotError` | - | Copilot通信エラー（RouterStopped含む） |

> 詳細は [transport.md](./transport.md) を参照してください。

### Tools Adapter

`infrastructure/src/tools/`

ツールシステムはプラグインベースのアーキテクチャを採用しています（詳細は [Tool Provider System](#tool-provider-system--ツールプロバイダーシステム) を参照）。

| Type | Implements | Description |
|------|------------|-------------|
| `ToolRegistry` | `ToolExecutorPort` | プロバイダーを集約、優先度でルーティング |
| `BuiltinProvider` | `ToolProvider` | 最小限の組み込みツール（priority: -100） |
| `CliToolProvider` | `ToolProvider` | システムCLIツールのラッパー（priority: 50） |
| `CustomToolProvider` | `ToolProvider` | ユーザー定義カスタムツール（priority: 75） |
| `JsonSchemaToolConverter` | `ToolSchemaPort` | ツール定義 → JSON Schema 変換（Port パターン） |
| `LocalToolExecutor` | `ToolExecutorPort` | ファイル操作、コマンド実行、検索、Web ツール |

#### 利用可能なツール

**Builtin Provider (priority: -100):**
- `read_file` - ファイル内容の読み取り（Low risk）
- `write_file` - ファイルの書き込み/作成（High risk）
- `run_command` - シェルコマンド実行（High risk）
- `glob_search` - パターンによるファイル検索（Low risk）
- `grep_search` - ファイル内容の検索（Low risk）

**CLI Provider (priority: 50):**
- `grep_search` - grep/rg によるファイル内容検索（Low risk）
- `glob_search` - find/fd によるファイルパターン検索（Low risk）

**Web Tools (`web-tools` feature, default in CLI):**
- `web_fetch` - URL からコンテンツを取得（Low risk）
- `web_search` - Web 検索（Low risk）

**Custom Tools (priority: 75, `init.lua` の `quorum.tools.register` で設定):**
- ユーザー定義のシェルコマンドテンプレート（default risk: High）

### Context Adapter

`infrastructure/src/context/`

| Type | Implements | Description |
|------|------------|-------------|
| `LocalContextLoader` | `ContextLoaderPort` | ローカルファイルシステムからのコンテキスト読み込み |

読み込み対象ファイル（優先度順）:
1. `.quorum/context.md` - 生成されたQuorumコンテキスト
2. `CLAUDE.md` - ローカルプロジェクト指示
3. `~/.claude/CLAUDE.md` - グローバルClaude設定
4. `README.md` - プロジェクトREADME
5. `docs/**/*.md` - docsディレクトリ内の全Markdown
6. `Cargo.toml`, `package.json`, `pyproject.toml` - ビルド設定

### Logging Adapter

`infrastructure/src/logging/`

| Type | Implements | Description |
|------|------------|-------------|
| `JsonlConversationLogger` | `ConversationLogger` | JSONL ファイルへの構造化会話ログ記録 |

各 `ConversationEvent` を `type` + `timestamp` 付きの JSON 行として追記。
`Mutex<BufWriter<File>>` でスレッドセーフ。Drop 時に flush。
`tracing` の診断ログとは分離された、会話トランスクリプト専用のログです。

### Reference Adapter

`infrastructure/src/reference/`

| Type | Implements | Description |
|------|------------|-------------|
| `GitHubReferenceResolver` | `ReferenceResolverPort` | `gh` CLI 経由で GitHub Issue/PR を解決 |

`try_new()` で `gh` CLI の存在と認証状態をチェックし、不在時は `None` で graceful degradation。
`gh issue view --json title,body` で Issue と PR の両方を解決します。
`resolve_all()` は `futures::future::join_all` で並列解決。

### Config Adapter

`infrastructure/src/config/`

| Type | Description |
|------|-------------|
| （TOML 基盤は撤去済み） | 設定は Lua スクリプティング（`infrastructure/src/scripting/`）で管理。[Configuration Reference](./configuration.md) を参照 |

---

## Presentation Layer / プレゼンテーション層

ユーザーインターフェースと出力フォーマット。

### CLI Module

`presentation/src/cli/`

| Type | Description |
|------|-------------|
| `Cli` | CLAPコマンド定義（--ensemble, --solo, --model, etc.） |
| `CliOutputFormat` | CLI用出力形式 |

### TUI Module

`presentation/src/tui/`

モーダルインターフェースの実装。ratatui ベース。

| Type | Description |
|------|-------------|
| `TuiApp` | メインアプリケーション（イベントループ、レンダリング） |
| `TuiState` | TUI 全体の状態（タブ、入力バッファ、モード） |
| `TuiPresenter` | `UiEvent` → `TuiEvent` 変換 + `RoutedTuiEvent` 送信 |
| `TuiProgressBridge` | `AgentProgressNotifier` → `TuiEvent` ブリッジ |
| `TuiHumanIntervention` | `HumanInterventionPort` の TUI 実装 |
| `InputMode` | Normal / Insert / Command |
| `KeyAction` | キーバインドアクション enum |
| `TuiInputConfig` | 入力設定（submit_key, max_height, dynamic_height 等） |

#### Tab + Pane

| Type | Description |
|------|-------------|
| `TabManager` | タブの作成・切り替え・インタラクションバインド管理 |
| `Tab` | タブ（TabId, ペインリスト） |
| `Pane` | ペイン（PaneId, PaneKind, messages, streaming_text, progress） |
| `PaneKind` | Interaction(InteractionForm, Option<InteractionId>) |

#### Events

| Type | Description |
|------|-------------|
| `TuiCommand` | TUI → Controller のコマンド（ProcessRequest, HandleCommand, SpawnInteraction, etc.） |
| `TuiEvent` | Controller → TUI のイベント（Welcome, ModeChanged, AgentResult, etc.） |
| `RoutedTuiEvent` | `TuiEvent` + ルーティング用 `interaction_id` |

#### Widgets

| Type | Description |
|------|-------------|
| `MainLayout` | メインレイアウト計算（Header + TabBar + Conversation + Progress + Input + StatusBar） |
| `conversation` | 会話表示ウィジェット |
| `header` | ヘッダーウィジェット |
| `input` | 入力エリアウィジェット（動的高さ対応） |
| `progress_panel` | プログレスパネルウィジェット |
| `status_bar` | ステータスバーウィジェット |
| `tab_bar` | タブバーウィジェット |

### Agent Module

`presentation/src/agent/`

エージェント実行の UI コンポーネント。

| Type | Description |
|------|-------------|
| `AgentProgressReporter` | `AgentProgressNotifier` の CLI 実装（indicatif） |
| `SimpleAgentProgress` | シンプルなテキスト進捗表示 |
| `InteractiveHumanIntervention` | `HumanInterventionPort` の CLI 実装（対話的承認/却下） |
| `ReplPresenter` | `UiEvent` の CLI レンダリング |
| `ThoughtStream` | エージェント思考の表示 |

### Output Module

`presentation/src/output/`

| Type | Description |
|------|-------------|
| `OutputFormatter` | 出力フォーマッターのトレイト |
| `ConsoleFormatter` | コンソール向け色付き出力 |

### Progress Module

`presentation/src/progress/`

| Type | Description |
|------|-------------|
| `ProgressReporter` | indicatifによるプログレスバー |
| `SimpleProgress` | シンプルなテキスト進捗表示 |

### Config Module

`presentation/src/config/`

| Type | Description |
|------|-------------|
| `OutputConfig` | 出力設定（format, color） |
| `ReplConfig` | REPL設定（show_progress, history_file） |

---

## Data Flow / データフロー

```
+===========================================================================+
|                                  cli/                                      |
|  +-------------+                                       +----------------+  |
|  | CLI Parser  |                                       | DI Container   |  |
|  +------+------+                                       +--------+-------+  |
|         |                                                       |          |
+=========|=======================================================|==========+
          |                                                       |
          v                                                       v
+===========================================================================+
|                            application/                                    |
|                                                                            |
|  +---------------------------------------------------------------------+   |
|  |                    AgentController (TUI/REPL)                       |   |
|  |                                                                     |   |
|  |  UiEvent channel  ←─────────────────────────────── TuiPresenter    |   |
|  |                                                                     |   |
|  |  ┌─────────────────────────────────────────────────────────────┐   |   |
|  |  │  RunAgentUseCase (Phase 1-5)                                │   |   |
|  |  │    ├── GatherContextUseCase (Phase 1)                       │   |   |
|  |  │    ├── Planning + Review   (Phase 2-3)                      │   |   |
|  |  │    └── ExecuteTaskUseCase  (Phase 4)                        │   |   |
|  |  └─────────────────────────────────────────────────────────────┘   |   |
|  |                                                                     |   |
|  |  ┌─────────────────────────────────────────────────────────────┐   |   |
|  |  │  RunQuorumUseCase                                           │   |   |
|  |  │    Phase 1: Initial Query (parallel)                        │   |   |
|  |  │    Phase 2: Peer Review (parallel)                          │   |   |
|  |  │    Phase 3: Synthesis (moderator)                           │   |   |
|  |  └─────────────────────────────────────────────────────────────┘   |   |
|  |                                                                     |   |
|  |  ┌─────────────────────────────────────────────────────────────┐   |   |
|  |  │  RunAskUseCase                                              │   |   |
|  |  │    Low-risk tool loop → direct answer                       │   |   |
|  |  └─────────────────────────────────────────────────────────────┘   |   |
|  +---------------------------------------------------------------------+   |
|                                                                            |
+==================================+=========================================+
                                   |
                                   v
+===========================================================================+
|                          infrastructure/                                   |
|                                                                            |
|  +------------------+    +------------------+    +---------------------+   |
|  | CopilotLlmGateway|----> MessageRouter    |----> copilot CLI (JSON) |   |
|  +------------------+    +-------+----------+    +---------------------+   |
|                                  |                                         |
|                          +-------+-------+                                 |
|                          | SessionChannel | (per session)                  |
|                          +---------------+                                 |
|                                                                            |
|  +------------------+    +------------------+    +---------------------+   |
|  | ToolRegistry     |    | JsonlConv.Logger |    | GitHubRefResolver  |   |
|  | (providers)      |    | (.jsonl file)    |    | (gh CLI)           |   |
|  +------------------+    +------------------+    +---------------------+   |
|                                                                            |
+===========================================================================+
```

---

## Copilot CLI Protocol / Copilot CLIプロトコル

`infrastructure/copilot/` は GitHub Copilot CLI と JSON-RPC 経由で通信します。

```
+------------------+         JSON-RPC          +------------------+
| copilot-quorum   |<------------------------->|  copilot CLI     |
|                  |   TCP (localhost:PORT)    |                  |
+------------------+                           +------------------+
```

### Communication Flow / 通信フロー

1. `copilot --server` を起動
2. stdout から "CLI server listening on port XXXXX" を読み取り
3. TCP接続を確立
4. **MessageRouter** が背景タスクで TCP reader を占有、session_id でルーティング
5. LSP形式のヘッダー + JSON-RPCでメッセージ交換

### Transport Demultiplexer / トランスポート多重分離

> 詳細は [transport.md](./transport.md) を参照してください。

単一の TCP 接続上で複数セッションを並列運用するため、`MessageRouter` がメッセージを
session_id ベースで各 `SessionChannel` にルーティングします。

```
Session A ← channel_a ←┐
Session B ← channel_b ←┤── MessageRouter (background reader task)
Session C ← channel_c ←┘        │
                                 └── TCP reader (single owner, no Mutex)
```

### Message Format / メッセージ形式

```
Content-Length: 123\r\n
\r\n
{"jsonrpc":"2.0","method":"session.create","params":{"model":"claude-sonnet-4.5"},"id":1}
```

---

## Concurrency Model / 並行処理モデル

```rust
// Phase 1: All models queried in parallel
let mut join_set = JoinSet::new();
for model in &models {
    join_set.spawn(query_model(model, question));
}
let responses = join_set.join_all().await;

// Phase 2: All reviews in parallel
let mut join_set = JoinSet::new();
for model in &models {
    join_set.spawn(do_peer_review(model, other_responses));
}
let reviews = join_set.join_all().await;

// Phase 3: Single moderator call
let synthesis = synthesize(moderator, responses, reviews).await;
```

非同期処理は `tokio` ランタイム上で実行。各フェーズ内のモデル呼び出しは `JoinSet` で並列化されており、レイテンシを最小化しています。

各 `JoinSet::spawn` 内で `gateway.create_session()` が呼ばれ、`MessageRouter` が
session_id 毎に独立した `SessionChannel` を払い出すため、並列セッション間でメッセージが
混線することはありません（詳細は [transport.md](./transport.md)）。

---

## Agent System / エージェントシステム

> 詳細は [agent-system.md](./agent-system.md) を参照してください。

エージェントシステムは、Quorumの概念を自律タスク実行に拡張したものです。
Solo モードで動作し、重要なポイントでは Quorum Consensus によるレビューを行います。

### Agent Flow / エージェントフロー

```
User Request
    │
    ▼
┌───────────────────┐
│ Context Gathering │  ← GatherContextUseCase: 3段階フォールバック
│  (Phase 1)        │    1. 既知ファイル直接読み込み
└───────────────────┘    2. 探索エージェント (tool use)
    │                    3. 最小コンテキストで続行
    ▼
┌───────────────────┐
│     Planning      │  ← Solo: decision_model が計画作成
│  (Phase 2)        │    Ensemble: review_models が並列計画 + 投票
└───────────────────┘
    │
    ▼
┌───────────────────────────┐
│ Quorum Consensus #1       │  ← review_models が計画をレビュー
│ Plan Review (Phase 3)     │     却下時: フィードバック付き再計画
│                           │     max_plan_revisions 超過: HiL 介入
└───────────────────────────┘
    │
    ▼
┌───────────────────────────┐
│ Execution Confirmation    │  ← PhaseScope::Full のみ
│ (Phase 3b)                │     HilMode に応じて自動/対話的承認
└───────────────────────────┘
    │
    ▼
┌───────────────────┐
│  Task Execution   │  ← ExecuteTaskUseCase
│  (Phase 4)        │    Low-risk: 直接並列実行
│                   │    High-risk: ActionReviewer 経由
└───────────────────┘
    │
    ▼
┌───────────────────────────┐
│ Final Review (Phase 5)    │  ← オプション (require_final_review: true)
│                           │     実行結果全体をレビュー
└───────────────────────────┘
```

### PhaseScope による制御

| Phase | Full | Fast | PlanOnly |
|-------|------|------|----------|
| 1. Context Gathering | yes | yes | yes |
| 2. Planning | yes | yes | yes |
| 3. Plan Review (Quorum) | yes | skip | skip |
| 3b. Execution Confirmation | yes | skip | skip |
| 4. Task Execution | yes | yes | skip+return |
| 4a. Action Review | yes | skip | N/A |
| 5. Final Review | opt | skip | N/A |

### Quorum Consensus / 合意形成

Quorum Consensus は複数モデルの投票によって安全性を確保します：

1. **Plan Review（必須）**: 設定された全 review_models が提案された計画をレビュー
2. **Action Review（高リスク操作）**: `write_file` と `run_command` の実行前にレビュー
3. **Final Review（オプション）**: 実行結果全体をレビュー

承認には過半数（または設定された QuorumRule）の投票が必要。却下された計画/アクションには集約されたフィードバックが含まれます。

### Risk Levels / リスクレベル

| Risk Level | Tools | Behavior |
|------------|-------|----------|
| Low | `read_file`, `glob_search`, `grep_search`, `web_fetch`, `web_search` | 直接並列実行（レビューなし） |
| High | `write_file`, `run_command` | ActionReviewer によるレビュー後に実行 |

### Progress Notification Pattern / 進捗通知パターン

エージェントシステムは「アクションとUI通知の分離」パターンを採用しています。
これはVuex/Fluxのような単方向データフローに似た設計です。

#### データフロー

```
UseCase (Application層)
│
├── review_plan() ──→ QuorumReviewResult
│                          │
│                          ▼
├── execute_with_progress() ─→ progress.on_quorum_complete_with_votes()
│                                   │
│                                   ▼
└── AgentProgressNotifier (Presentation層) ──→ UI表示
```

---

## Tool Provider System / ツールプロバイダーシステム

> 詳細は [tool-system.md](./tool-system.md) を参照してください。

ツールプロバイダーシステムは、**プラグインベースのオーケストレーション**アーキテクチャを採用しています。

### Architecture / アーキテクチャ

```
┌─────────────────────────────────────────────────────────────┐
│                     ToolRegistry                            │
│  (プロバイダーを集約、優先度でルーティング)                 │
└─────────────────────────────────────────────────────────────┘
          │              │              │              │
          ▼              ▼              ▼              ▼
   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
   │ Builtin  │   │   CLI    │   │  Custom  │   │   MCP    │
   │ Provider │   │ Provider │   │ Provider │   │ Provider │
   └──────────┘   └──────────┘   └──────────┘   └──────────┘
   最小限の        rg, fd, gh     init.lua        MCP サーバー
   フォールバック   等をラップ    tools.register   を統合
   (優先度: -100)  (優先度: 50)  (優先度: 75)    (優先度: 100)
```

### Provider Types / プロバイダーの種類

| Provider | Priority | Description | Use Case |
|----------|----------|-------------|----------|
| **MCP** | 100 | MCP サーバー経由のツール | 外部サーバーとの連携、豊富な機能 |
| **Custom** | 75 | ユーザー定義シェルコマンド | カスタム処理、プロジェクト固有ツール |
| **CLI** | 50 | システムCLIツールのラッパー | grep/rg, find/fd, cat/bat |
| **Builtin** | -100 | 最小限の組み込みツール | フォールバック、常に利用可能 |

### Module Structure / モジュール構造

```
infrastructure/src/tools/
├── mod.rs              # 全体エクスポート
├── registry.rs         # ToolRegistry (ToolExecutorPort 実装)
├── executor.rs         # LocalToolExecutor
├── schema.rs           # JsonSchemaToolConverter (ToolSchemaPort 実装)
├── custom_provider.rs  # CustomToolProvider (priority: 75)
├── builtin/
│   ├── mod.rs
│   └── provider.rs     # BuiltinProvider (priority: -100)
├── cli/
│   ├── mod.rs
│   ├── provider.rs     # CliToolProvider (priority: 50)
│   └── discovery.rs    # 推奨ツール検知 & 提案
├── web/
│   ├── mod.rs
│   ├── fetch.rs        # web_fetch (feature-gated: web-tools)
│   └── search.rs       # web_search (feature-gated: web-tools)
├── file.rs             # read_file, write_file 実装
├── command.rs          # run_command 実装
└── search.rs           # glob_search, grep_search 実装
```

---

## Error Handling / エラーハンドリング

| Error Type | Location | Handling |
|------------|----------|----------|
| `DomainError` | `domain/` | ドメインルール違反 |
| `GatewayError` | `application/` | LLMゲートウェイエラー |
| `RunAgentError` | `application/` | エージェント実行エラー |
| `RunAskError` | `application/` | Ask実行エラー |
| `RunQuorumError` | `application/` | Quorum実行エラー |
| `CopilotError` | `infrastructure/` | Copilot CLI通信エラー（RouterStopped含む） |
| `ReferenceError` | `application/` | リソース参照解決エラー |
| `HumanInterventionError` | `application/` | 人間介入エラー（Cancelled, IoError） |
| `SpawnError` | `domain/` | インタラクション生成エラー |

部分的な失敗（一部のモデルがエラーを返す）は許容され、成功したモデルの結果のみで処理を続行します。

---

## Extension Points / 拡張ポイント

拡張ポイント（LLM プロバイダー / オーケストレーション戦略 / ツール / インタラクション形式 /
コンテキストファイル種別の追加）の実践手順は
[How to Extend the Codebase](../how-to/extend-the-codebase.md) を参照してください。

---

## Testing Strategy / テスト戦略

オニオンアーキテクチャにより、各層を独立してテスト可能：

| Layer | Test Type | Description |
|-------|-----------|-------------|
| domain | Unit | ドメインロジックの単体テスト（InteractionTree, ContextMode, ToolExecution等） |
| application | Unit + Integration | ScriptedGateway/MockToolExecutor でフローテスト |
| infrastructure | Integration | 実際のCopilot CLIとの結合テスト |
| presentation | Unit | フォーマッターの出力テスト |

### Flow Test Infrastructure

`application/src/use_cases/run_agent/` のテストでは以下のテストインフラを使用：

| Type | Description |
|------|-------------|
| `ScriptedGateway` | モデル名ごとにスクリプト化されたレスポンスを返すモックゲートウェイ |
| `ScriptedSession` | 順序付きレスポンスを返すモックセッション |
| `FlowTestBuilder` | Solo/Fast/PlanOnly/Ensemble のフローテスト構築用ビルダー |
| `TrackingProgress` | フェーズ遷移を記録するモックプログレス |
| `MockToolExecutor` | 呼び出しを記録して成功を返すモックツール実行器 |
| `MockHumanIntervention` | 設定可能な HiL 決定を返すモック |

```bash
# Run all tests
cargo test --workspace

# Run domain tests only
cargo test -p quorum-domain

# Run with coverage
cargo llvm-cov --workspace
```

<!--
LLM Context: Architecture Reference

Key types and locations:
- Domain: InteractionForm, InteractionTree, Interaction, InteractionResult, ContextMode, ResourceReference (domain/src/interaction/, domain/src/context/)
- Domain: ToolExecution, ToolExecutionState, ToolExecutionId (domain/src/agent/tool_execution.rs)
- Domain: ConsensusLevel, PhaseScope, OrchestrationStrategy, SessionMode (domain/src/orchestration/)
- Domain: AgentState, Plan, Task, ModelConfig, AgentPolicy, HilMode, HumanDecision (domain/src/agent/)
- Domain: ToolDefinition, ToolCall, ToolResult, ToolSpec, RiskLevel, ToolProvider (domain/src/tool/)
- Domain: LlmResponse, ContentBlock, StopReason, StreamEvent (domain/src/session/)
- Application Ports: LlmGateway, LlmSession, ToolExecutorPort, ToolSchemaPort, ContextLoaderPort, ProgressNotifier, AgentProgressNotifier, HumanInterventionPort, ActionReviewer, ConversationLogger, ReferenceResolverPort, UiEvent (application/src/ports/)
- Application Use Cases: RunAgentUseCase (run_agent/), RunQuorumUseCase, RunAskUseCase, GatherContextUseCase, ExecuteTaskUseCase, AgentController, InitContextUseCase (application/src/use_cases/)
- Application Config: QuorumConfig, ExecutionParams (application/src/config/)
- Infrastructure: CopilotLlmGateway, CopilotSession, MessageRouter, SessionChannel (infrastructure/src/copilot/)
- Infrastructure: ToolRegistry, LocalToolExecutor, BuiltinProvider, CliToolProvider, CustomToolProvider, JsonSchemaToolConverter (infrastructure/src/tools/)
- Infrastructure: JsonlConversationLogger (infrastructure/src/logging/)
- Infrastructure: GitHubReferenceResolver (infrastructure/src/reference/)
- Infrastructure: ConfigLoader, FileConfig (infrastructure/src/config/)
- Presentation: TuiApp, TuiState, TuiPresenter, TuiProgressBridge, TuiHumanIntervention (presentation/src/tui/)
- Presentation: TabManager, Tab, Pane, PaneKind (presentation/src/tui/tab.rs)
- Presentation: TuiCommand, TuiEvent, RoutedTuiEvent (presentation/src/tui/event.rs)
- Presentation: MainLayout, widgets/* (presentation/src/tui/widgets/)
- Presentation: AgentProgressReporter, InteractiveHumanIntervention, ReplPresenter, ThoughtStream (presentation/src/agent/)
- Presentation: ConsoleFormatter (presentation/src/output/)
- Presentation: ProgressReporter, SimpleProgress (presentation/src/progress/)
- Presentation: Cli (presentation/src/cli/)
- CLI: main.rs DI assembly (cli/src/main.rs)

Module structure:
- domain/src/: core/, quorum/, session/, orchestration/, agent/, tool/, interaction/, context/, prompt/, config/, util.rs
- application/src/: ports/ (11 modules), use_cases/ (run_agent/ [mod,types,planning,review,hil], run_quorum, run_ask, gather_context, execute_task, agent_controller, init_context, shared), config/
- infrastructure/src/: copilot/ (gateway, session, router, transport, protocol, error), tools/ (registry, executor, schema, custom_provider, builtin/, cli/, web/), context/, logging/, reference/, config/
- presentation/src/: cli/, tui/ (app, state, presenter, progress, human_intervention, editor, mode, event, tab, widgets/), agent/ (progress, thought, human_intervention, presenter), output/, progress/, config/
- cli/src/: main.rs
-->
