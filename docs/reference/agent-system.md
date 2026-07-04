# Agent System Reference / エージェントシステムリファレンス

> Implementation reference: types, ports, module layout and data flow of the agent system
>
> エージェントシステムの型・ポート・モジュール構成・データフローの実装リファレンス

---

## Overview / 概要

エージェントの実行フロー・レビューポイント・HiL の仕組みは
[Agent Behavior](../explanation/agent-behavior.md) を、
実行手順は [How to Run Agent Tasks](../how-to/run-agent-tasks.md) を参照してください。
このドキュメントは実装の詳細（型・ポート・データフロー）を扱います。

エージェントは `InteractionForm`（`Agent` / `Ask` / `Discuss`）の一形態として実行されます。
詳細は [Interaction Model](../explanation/interaction-model.md) を参照してください。

---

## Resource Reference Resolution / リソース参照の自動解決

Context Gathering フェーズで、ユーザーリクエスト中の GitHub Issue/PR 参照を自動検出・解決します。

### ResourceReference 抽出

`extract_references()` がテキストから以下のパターンを認識します（優先度順）：

| パターン | 例 | 結果 |
|---------|-----|------|
| GitHub URL | `github.com/owner/repo/issues/123` | `GitHubIssue { repo: Some("owner/repo"), number: 123 }` |
| クロスリポジトリ | `owner/repo#123` | `GitHubIssue { repo: Some("owner/repo"), number: 123 }` |
| 型付き明示 | `PR #123`, `Issue #42` | 対応する型 |
| 範囲参照 | `#10-15` (差が≤10) | `GitHubIssue` × 6 |
| ベア参照 | `#123` | `GitHubIssue { repo: None, number: 123 }` |

定義ファイル: `domain/src/context/reference.rs`

### GitHubReferenceResolver

`gh` CLI を使用して Issue/PR の内容を取得します。

- `try_new()`: `gh` のインストール・認証を確認、失敗時は `None`（graceful degradation）
- `gh issue view`: Issues と PR の両方に対応（GitHub API は同一エンドポイント）
- `resolve_all()`: `futures::join_all()` で並列解決

定義ファイル: `infrastructure/src/reference/github.rs`

---

## Configuration Types / 設定型（4型分割）

設定は 4 つの focused な型に分割されています：

| 型 | 層 | 性質 | 用途 |
|----|-----|------|------|
| `SessionMode` | domain | runtime-mutable | TUI で切り替え可能なオーケストレーション設定 |
| `ModelConfig` | domain | static | ロールベースモデル選択 |
| `AgentPolicy` | domain | static | ドメイン動作制約（HiL、レビュー設定） |
| `ExecutionParams` | application | static | Use case ループ制御パラメータ |

```rust
// domain/src/orchestration/session_mode.rs — TUI /solo, /ens, /fast で切替
pub struct SessionMode {
    pub consensus_level: ConsensusLevel,       // Solo or Ensemble
    pub phase_scope: PhaseScope,               // Full, Fast, PlanOnly
    pub strategy: OrchestrationStrategy,       // Quorum or Debate
}

// domain/src/agent/model_config.rs — ロールベースモデル設定
pub struct ModelConfig {
    // Agent Roles
    pub exploration: Model,       // コンテキスト収集用（デフォルト: Haiku）
    pub decision: Model,          // 計画作成・高リスクツール判断用（デフォルト: Sonnet）
    pub review: Vec<Model>,       // Quorum レビュー投票用
    // Interaction Roles
    pub participants: Vec<Model>, // Quorum Discussion 参加者
    pub moderator: Model,         // Quorum Synthesis 担当
    pub ask: Model,               // Ask (Q&A) 応答用
}

// domain/src/agent/agent_policy.rs — ドメインポリシー
pub struct AgentPolicy {
    pub hil_mode: HilMode,
    pub require_plan_review: bool,     // 常に true
    pub require_final_review: bool,
    pub max_plan_revisions: usize,
}

// application/src/config/execution_params.rs — 実行制御
pub struct ExecutionParams {
    pub max_iterations: usize,
    pub max_tool_turns: usize,
    pub max_tool_retries: usize,
    pub working_dir: Option<String>,
    pub ensemble_session_timeout: Option<Duration>,
}
```

`QuorumConfig`（application 層）が 4 型をまとめるコンテナとして機能し、Buffer Controller 間の伝播を担います：

```rust
// application/src/config/quorum_config.rs
pub struct QuorumConfig { /* SessionMode, ModelConfig, AgentPolicy, ExecutionParams */ }
impl QuorumConfig {
    pub fn mode_mut(&mut self) -> &mut SessionMode; // runtime-mutable
    pub fn to_agent_input(&self, request: impl Into<String>) -> RunAgentInput;
    pub fn to_quorum_input(&self, question: impl Into<String>) -> RunQuorumInput;
}
```

### Buffer 必要性マップ

| 型 | Agent | Ask | Discuss |
|----|-------|-----|---------|
| `SessionMode` | Yes | No (Solo固定) | Yes |
| `ModelConfig` | Yes | Yes | Yes |
| `AgentPolicy` | Yes | No | No |
| `ExecutionParams` | Yes | Yes | No |

3 軸（`SessionMode`）の意味と組み合わせバリデーションは
[Orchestration Axes](../explanation/orchestration-axes.md) を、
設定キー（`agent.*`, `models.*` 等）は
[Configuration Reference](./configuration.md) を参照してください。

---

## Architecture / アーキテクチャ

### run_agent/ ディレクトリ構造

`RunAgentUseCase` の実装は責務ごとにモジュール分割されています：

| File | Description |
|------|-------------|
| `mod.rs` | メインフロー（`execute_with_progress`）、`RunAgentUseCase` 定義、Flow テスト基盤 |
| `types.rs` | `RunAgentInput`, `RunAgentOutput`, `RunAgentError`, `PlanningResult`, `QuorumReviewResult` |
| `hil.rs` | `handle_human_intervention()`, `handle_execution_confirmation()` |
| `planning.rs` | `create_plan()`, `create_ensemble_plans()`（並列生成 + 投票 + 選択） |
| `review.rs` | `review_plan()`, `final_review()`, `QuorumActionReviewer`（`ActionReviewer` 実装） |

### ToolExecution State Machine / ツール実行ステートマシン

個々のツール呼び出しのライフサイクルを追跡するステートマシンです。

```
Pending ──> Running ──> Completed
                   └──> Error
```

各状態は tagged union として実装され、その状態でのみ有効なフィールドを持ちます：

| State | Fields | Description |
|-------|--------|-------------|
| `Pending` | tool_name, arguments, native_id | ツール呼び出し受信、実行待ち |
| `Running` | + started_at | 実行中 |
| `Completed` | + completed_at, output_preview, metadata | 正常完了 |
| `Error` | + failed_at, error_message | 実行失敗 |

遷移メソッド: `mark_running()`, `mark_completed(&ToolResult)`, `mark_error(message)`
不正な遷移（例: Pending→Completed）は no-op として安全に処理されます。

定義ファイル: `domain/src/agent/tool_execution.rs`

### Key Files / 主要ファイル

| File | Description |
|------|-------------|
| `domain/src/agent/entities.rs` | `AgentState`, `Plan`, `Task`, `AgentPhase`, `ReviewRound`, `ModelVote` |
| `domain/src/agent/model_config.rs` | `ModelConfig`（ロールベースモデル設定） |
| `domain/src/agent/agent_policy.rs` | `AgentPolicy`, `HilAction`（ドメインポリシー） |
| `domain/src/agent/tool_execution.rs` | `ToolExecution`, `ToolExecutionState`（ステートマシン） |
| `domain/src/agent/validation.rs` | `ConfigIssue`, `ConfigIssueCode`, `Severity` |
| `domain/src/orchestration/session_mode.rs` | `SessionMode`（runtime-mutable オーケストレーション設定） |
| `domain/src/context/reference.rs` | `ResourceReference`, `extract_references()` |
| `domain/src/interaction/mod.rs` | `InteractionForm` (`Agent`, `Ask`, `Discuss`) |
| `domain/src/tool/entities.rs` | `ToolDefinition`, `ToolCall`, `ToolSpec`, `RiskLevel` |
| `application/src/use_cases/run_agent/` | `RunAgentUseCase` — メインフロー（5モジュール分割） |
| `application/src/config/execution_params.rs` | `ExecutionParams`（実行ループ制御） |
| `application/src/config/quorum_config.rs` | `QuorumConfig`（4型コンテナ） |
| `application/src/ports/agent_progress.rs` | `AgentProgressNotifier`（進捗通知ポート） |
| `application/src/ports/ui_event.rs` | `UiEvent`（Application→Presentation 出力ポート） |
| `application/src/ports/event_publisher.rs` | `EventPublisher`（typed イベントの継ぎ目） |
| `infrastructure/src/reference/github.rs` | `GitHubReferenceResolver`（`gh` CLI 解決） |
| `infrastructure/src/tools/` | `LocalToolExecutor` 実装 |

### Data Flow / データフロー

```
RunAgentUseCase::execute_with_progress() (Application層)
│
├── Phase 1: GatherContextUseCase.execute()
│   ├── ToolExecutorPort.execute(glob_search, read_file)
│   ├── extract_references(request) → Vec<ResourceReference>
│   └── ReferenceResolverPort.resolve_all(refs) → GitHub Issue/PR 内容
│
├── Phase 2: create_plan() or create_ensemble_plans()
│   ├── Solo: LlmSession.send_with_tools() → Plan (Native Tool Use)
│   └── Ensemble: JoinSet で並列生成 → 投票 → 選択
│
├── Phase 3: review_plan() → QuorumReviewResult
│   ├── Model A: Vote (APPROVE/REJECT)
│   ├── Model B: Vote
│   └── Model C: Vote
│   └── Rejected → plan_feedback → loop back to Phase 2
│
├── Phase 3b: handle_execution_confirmation()
│   └── PhaseScope::Full + Interactive → HumanInterventionPort
│
├── Phase 4: ExecuteTaskUseCase.execute()
│   ├── Native Tool Use multi-turn loop
│   ├── Low-risk tool → ToolExecutorPort.execute() (並列)
│   └── High-risk tool → QuorumActionReviewer → ToolExecutorPort.execute()
│
└── Phase 5: final_review() → QuorumReviewResult (optional)
```

### UiEvent Output Port / UI イベント出力ポート

`UiEvent` は Application 層から Presentation 層への出力ポートです。
`AgentController` がユースケースの結果を `UiEvent` に変換し、
`TuiPresenter` / `ReplPresenter` が受け取ってレンダリングします。

```
AgentController ──→ UiEvent ──→ TuiPresenter ──→ TuiState mutations
                                ReplPresenter ──→ Console output
```

主なイベント：

| Category | Events |
|----------|--------|
| Welcome & Info | `Welcome(WelcomeInfo)`, `ConfigDisplay(ConfigSnapshot)` |
| Mode Changes | `ModeChanged`, `ScopeChanged`, `StrategyChanged` |
| Agent Execution | `AgentStarting`, `AgentResult`, `AgentError` |
| Interaction | `InteractionSpawned`, `InteractionCompleted`, `InteractionSpawnError` |
| Ask | `AskStarting`, `AskResult`, `AskError` |
| Quorum | `QuorumStarting`, `QuorumResult`, `QuorumError` |
| Context Init | `ContextInitStarting`, `ContextInitResult`, `ContextInitError` |
| Errors | `CommandError`, `UnknownCommand` |

定義ファイル: `application/src/ports/ui_event.rs`

### EventPublisher — typed イベントの継ぎ目

合議結果（`quorum_result`）のような**外部消費される typed イベント**は、
`EventPublisher` port（`application/src/ports/event_publisher.rs`）を通る単一の発行点を持ちます。

```rust
pub enum AppEvent { QuorumResult(QuorumResultEnvelope) }  // variant は今後増える
pub trait EventPublisher: Send + Sync {
    fn publish(&self, event: AppEvent);  // sync・fire-and-forget
}
```

- 購読者: `ConversationLogEventPublisher`（JSONL）、`ScriptEventPublisher`（Lua `QuorumResult` イベント）。`CompositeEventPublisher` でファンアウト
- これは意図的に「バス」ではなく**バスに後で差し替えられる継ぎ目**（RFC Discussion #304）。将来の Application / Interaction Event Bus は impl 差し替え + `AppEvent` variant 追加で導入され、呼び出し側は変わらない
- エンベロープの JSON 契約は [Logging](logging.md) の `quorum_result` v1 を参照

### Progress Notification / 進捗通知

エージェントは「アクションと UI 通知の分離」パターンを採用しています。

> **Note**: 進捗通知には 2 つの trait があります：
> - `ProgressNotifier`（`domain/src/orchestration/strategy.rs`）— Quorum Discussion のフェーズ進行通知（ドメイン層）
> - `AgentProgressNotifier`（`application/src/ports/agent_progress.rs`）— エージェント実行全体の進捗通知（アプリケーション層）
>
> 両者は別の trait であり、`ProgressNotifier` は Quorum Discussion の各フェーズ（Initial Query → Peer Review → Synthesis）を、`AgentProgressNotifier` はエージェントの各フェーズ（Planning → Execution → Review 等）を通知します。

| Layer | Responsibility |
|-------|---------------|
| **低レベル関数** (`review_plan`, `review_action`) | ビジネスロジック実行、結果を返す |
| **メインループ** (`execute_with_progress`) | 結果に基づき UI 通知を発火 |
| **AgentProgressNotifier** (Presentation 層) | UI 更新、フィードバック表示 |

`AgentProgressNotifier` のコールバックカテゴリ：

| Category | Callbacks |
|----------|-----------|
| Phase | `on_phase_change` |
| Reasoning | `on_thought` |
| Task | `on_task_start`, `on_task_complete` |
| Tool | `on_tool_call`, `on_tool_result`, `on_tool_error`, `on_tool_retry`, `on_tool_not_found`, `on_tool_resolved` |
| Tool Execution Lifecycle | `on_tool_execution_created`, `on_tool_execution_started`, `on_tool_execution_completed`, `on_tool_execution_failed` |
| LLM Streaming | `on_llm_chunk`, `on_llm_stream_start`, `on_llm_stream_end` |
| Plan Revision | `on_plan_revision`, `on_action_retry` |
| Quorum | `on_quorum_start`, `on_quorum_model_complete`, `on_quorum_complete`, `on_quorum_complete_with_votes` |
| HiL | `on_human_intervention_required`, `on_execution_confirmation_required` |
| Ensemble | `on_ensemble_start`, `on_ensemble_plan_generated`, `on_ensemble_voting_start`, `on_ensemble_model_failed`, `on_ensemble_complete`, `on_ensemble_fallback` |

全メソッドにはデフォルトの no-op 実装があり、必要なコールバックのみオーバーライドできます。

### ReviewRound & ModelVote

`HumanInterventionPort::request_intervention` の `review_history` 引数や、HiL UI の `Review History` 表示に使われるデータ構造です。

```rust
/// A single model's vote in a quorum review
pub struct ModelVote {
    pub model: String,       // モデル識別子
    pub approved: bool,      // 承認/拒否
    pub reasoning: String,   // 理由・フィードバック
}

/// A record of a single review round in quorum voting
pub struct ReviewRound {
    pub round: usize,            // ラウンド番号（1-indexed）
    pub approved: bool,          // このラウンドの結果
    pub votes: Vec<ModelVote>,   // 個別投票
    pub timestamp: u64,          // タイムスタンプ
}

impl ReviewRound {
    pub fn vote_summary(&self) -> String; // "[●●○]" 形式の視覚的サマリー
}
```

定義ファイル: `domain/src/agent/entities.rs`

### HumanInterventionPort

```rust
#[async_trait]
pub trait HumanInterventionPort: Send + Sync {
    async fn request_intervention(
        &self,
        request: &str,
        plan: &Plan,
        review_history: &[ReviewRound],
    ) -> Result<HumanDecision, HumanInterventionError>;

    async fn request_execution_confirmation(
        &self,
        request: &str,
        plan: &Plan,
    ) -> Result<HumanDecision, HumanInterventionError> {
        // デフォルト実装: 自動承認（後方互換性）
        Ok(HumanDecision::Approve)
    }
}

pub enum HumanDecision {
    Approve,        // 現在の計画を実行
    Reject,         // エージェントを中止
    Edit(Plan),     // 編集した計画を使用（将来）
}
```

| Implementation | Description |
|----------------|-------------|
| `InteractiveHumanIntervention` | CLI 対話 UI（presentation 層） |
| `AutoRejectIntervention` | 常に Reject を返す（application 層） |
| `AutoApproveIntervention` | 常に Approve を返す（application 層） |

---

## Related Features / 関連機能

- [Agent Behavior](../explanation/agent-behavior.md) - ライフサイクル・レビュー・HiL の仕組み
- [How to Run Agent Tasks](../how-to/run-agent-tasks.md) - 実行手順
- [Interaction Model](../explanation/interaction-model.md) - Agent/Ask/Discuss の対等な関係
- [Tool System](./tool-system.md) - エージェントが使用するツールの詳細
- [Native Tool Use](./native-tool-use.md) - 構造化ツール呼び出し API
- [Configuration Reference](./configuration.md) - `agent.*` / `models.*` キー

<!-- LLM Context: Agent System の実装リファレンス。run_agent/ は 5 モジュール分割: mod.rs (メインフロー), types.rs, hil.rs, planning.rs, review.rs。ToolExecution ステートマシン (Pending→Running→Completed/Error) が domain/src/agent/tool_execution.rs。UiEvent 出力ポートが application/src/ports/ui_event.rs で Application→Presentation の構造化イベント伝達。AgentProgressNotifier (application/src/ports/agent_progress.rs) は 6 カテゴリ・26+ コールバック。ResourceReference (domain/src/context/reference.rs) と GitHubReferenceResolver (infrastructure/src/reference/github.rs) で GitHub Issue/PR 自動解決。設定は 4 型分割: SessionMode, ModelConfig (Agent: exploration/decision/review + Interaction: participants/moderator/ask), AgentPolicy, ExecutionParams。QuorumConfig (application) が 4 型コンテナ。動作原理・HiL は explanation/agent-behavior.md、3軸は explanation/orchestration-axes.md を参照。 -->
