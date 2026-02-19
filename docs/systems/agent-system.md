# Agent System / エージェントシステム

> Autonomous task execution with quorum-based safety and human-in-the-loop
>
> 合議ベースの安全性と人間介入を備えた自律タスク実行システム

---

## Overview / 概要

エージェントシステムは、copilot-quorum の合議（Quorum）コンセプトを自律的なタスク実行に拡張したものです。
ルーチンタスクは単一モデルで高速実行しつつ、重要な決定ポイントでは複数モデルによる合議を行うことで、
**効率性**と**安全性**を両立しています。

従来のエージェントシステムは単一モデルの判断に依存しますが、
ハルシネーション、盲点、過信といったリスクがあります。
copilot-quorum のエージェントは、**3 つの重要ポイント**で Quorum Consensus を挟むことで、
これらのリスクを構造的に軽減します。

---

## Quick Start / クイックスタート

```bash
# Solo モードでエージェント実行（デフォルト）
copilot-quorum "Fix the login bug in auth.rs"

# Ensemble モードで計画生成（複雑なタスク向け）
copilot-quorum --ensemble "Design the authentication system"

# REPL で対話的に実行
copilot-quorum --chat
> Fix the broken test in user_service.rs
> /solo    # Solo モードに切り替え
> /ens     # Ensemble モードに切り替え
```

---

## How It Works / 仕組み

### Agent Lifecycle / エージェントライフサイクル

エージェントの実行フローは `PhaseScope` によって制御されます。
各フェーズの実行可否は以下の表の通りです：

| Phase                  | Full | Fast  | PlanOnly    |
|------------------------|------|-------|-------------|
| 1. Context Gathering   | yes  | yes   | yes         |
| 2. Planning            | yes  | yes   | yes         |
| 3. Plan Review (Quorum)| yes  | skip  | skip        |
| 3b. Execution Confirm  | yes  | skip  | skip        |
| 4. Executing           | yes  | yes   | skip+return |
|    - Action Review     | yes  | skip  | N/A         |
| 5. Final Review        | opt  | skip  | N/A         |

定義ファイル: `application/src/use_cases/run_agent/mod.rs`

```
User Request
    │
    ▼
┌───────────────────┐
│ Context Gathering │  ← プロジェクト情報収集 (glob, read_file)
└───────────────────┘    exploration_model を使用
    │                    ResourceReference 自動解決（GitHub Issue/PR）
    ▼
┌───────────────────┐
│     Planning      │  ← Solo: decision_model が計画作成
└───────────────────┘    Ensemble: review_models が並列生成 → 投票
    │
    ▼
╔═══════════════════════════╗
║  Quorum Consensus #1     ║  ← 全モデルが計画をレビュー（PhaseScope::Full のみ）
║  Plan Review              ║    QuorumRule に基づく投票で承認/却下
╚═══════════════════════════╝
    │
    ▼
┌─── Execution Confirm ────┐  ← PhaseScope::Full + Interactive のみ
│  "本当に実行しますか？"     │    HilMode に応じて自動/手動判断
└───────────────────────────┘
    │
    ▼
┌───────────────────┐
│  Task Execution   │   ← Native Tool Use マルチターンループ
│   ├─ Low-risk  ────▶ 直接実行（並列）
│   │
│   └─ High-risk ────▶ ╔═══════════════════════════╗
│                       ║  Quorum Consensus #2     ║
│                       ║  Action Review            ║
│                       ╚═══════════════════════════╝
└───────────────────┘
    │
    ▼
╔═══════════════════════════╗
║  Quorum Consensus #3     ║  ← オプションの最終レビュー
║  Final Review             ║    (require_final_review: true)
╚═══════════════════════════╝
```

### Quorum Review Points / 合議レビューポイント

| Review | Timing | Required | Purpose |
|--------|--------|----------|---------|
| **Plan Review** | 計画作成後 | PhaseScope::Full | 計画の安全性・正確性を検証 |
| **Execution Confirm** | 計画承認後 | PhaseScope::Full + Interactive | 実行前の最終確認 |
| **Action Review** | 高リスクツール実行前 | PhaseScope::Full | 書き込み・コマンド実行の安全性検証 |
| **Final Review** | 全タスク完了後 | オプション | 実行結果全体の品質検証 |

### Risk-Based Tool Classification / リスクベースのツール分類

| Tool | Risk Level | Quorum Review | Rationale |
|------|------------|---------------|-----------|
| `read_file` | Low | No | 読み取り専用 |
| `glob_search` | Low | No | 読み取り専用 |
| `grep_search` | Low | No | 読み取り専用 |
| `web_fetch` | Low | No | 読み取り専用（`web-tools` feature） |
| `web_search` | Low | No | 読み取り専用（`web-tools` feature） |
| `write_file` | **High** | **Yes** | ファイルシステムを変更 |
| `run_command` | **High** | **Yes** | 外部コマンド実行、元に戻すのが困難 |

### Plan Review Details / 計画レビューの詳細

計画レビューは `PhaseScope::Full` で実行されます（Fast/PlanOnly ではスキップ）。

1. 全ての review_models に計画を並列送信（`JoinSet` で並行実行）
2. 各モデルが APPROVE / REJECT を投票
3. QuorumRule（デフォルト: 過半数）で判定
4. 却下時は全モデルのフィードバックを集約し、計画を修正 → 再投票

実装: `application/src/use_cases/run_agent/review.rs` — `review_plan()`, `query_model_for_review()`

### Action Review Details / アクションレビューの詳細

高リスクツール（`write_file`, `run_command`）の実行前に自動発動します。
`QuorumActionReviewer` が `ActionReviewer` trait を実装し、マルチモデル投票で判断します。

判断基準:
- 操作は必要か？
- 引数は正しいか？
- より安全な代替手段はないか？

却下されたアクションはスキップされます（エラーではない）。

実装: `application/src/use_cases/run_agent/review.rs` — `QuorumActionReviewer`

---

## InteractionForm / インタラクション形式

エージェントは `InteractionForm` の一形態として実行されます。
TUI では各インタラクションが独立したタブで実行されます。

| Form | Description | 使用する設定 |
|------|-------------|------------|
| `Agent` | 自律タスク実行（計画・ツール使用・レビュー） | SessionMode, AgentPolicy, ExecutionParams |
| `Ask` | 単発の質問→回答（読み取り専用ツールのみ） | SessionMode (モデル選択), ExecutionParams (max_tool_turns) |
| `Discuss` | マルチモデル議論 / Quorum council | SessionMode (consensus + strategy) |

```
TUI                          Application
───                          ───────────
:ask "question"  ──────────→ RunAskUseCase
:agent "task"    ──────────→ RunAgentUseCase
:discuss "topic" ──────────→ RunQuorumUseCase
```

定義ファイル: `domain/src/interaction/mod.rs`

---

## Human-in-the-Loop (HiL) / 人間介入

### Overview / 概要

Quorum が合意に至らない場合、エージェントは無限にリトライする代わりに、
ユーザーに判断を委ねることができます。

HiL には 2 つのゲートがあります：
1. **Plan Review HiL**: `max_plan_revisions` 到達時
2. **Execution Confirmation**: 計画承認後、タスク実行前（`PhaseScope::Full` のみ）

```
Planning → Review REJECTED → Revision 1
                ↓
         Review REJECTED → Revision 2
                ↓
         Review REJECTED → Revision 3
                ↓
         max_plan_revisions 到達
                ↓
    ┌─── HiL Mode ───┐
    │                │
    ▼                ▼
Interactive      Auto*
    │                │
    ▼                ▼
User Prompt    自動決定
/approve          │
/reject           │
/edit             │
    │             │
    ▼             ▼
Continue or Abort
```

### HiL Modes / HiL モード

| Mode | Description |
|------|-------------|
| `Interactive` | ユーザーにプロンプトを表示して判断を求める（デフォルト） |
| `AutoReject` | 自動的に中止する |
| `AutoApprove` | 自動的に最後の計画を承認する（危険！） |

### Execution Confirmation Gate / 実行確認ゲート

`PhaseScope::Full` の場合、計画が承認された後でもタスク実行前に追加の確認ゲートがあります。
`HilMode` に応じた決定ソース：

| HilMode | 動作 |
|---------|------|
| `Interactive` | `HumanInterventionPort::request_execution_confirmation()` |
| `AutoApprove` | 自動承認 |
| `AutoReject` | 自動拒否（計画は作成されるが実行されない） |

実装: `application/src/use_cases/run_agent/hil.rs` — `handle_execution_confirmation()`

### Interactive Mode UI / インタラクティブモード UI

```
═══════════════════════════════════════════════════════════════
  ⚠️  Plan Requires Human Intervention
═══════════════════════════════════════════════════════════════

Revision limit (3) exceeded. Quorum could not reach consensus.

Request:
  Update the README file

Plan Objective:
  Add installation instructions to README

Tasks:
  1. Read README.md
  2. Append installation section

Review History:
  Rev 1: REJECTED [○●○]
    └─ gpt-5.2-codex: Missing error handling
  Rev 2: REJECTED [●○○]
    └─ gemini-3-pro-preview: Unclear objective
  Rev 3: REJECTED [○○●]
    └─ claude-sonnet-4.5: Inconsistent approach

Commands:
  /approve  - Execute this plan as-is
  /reject   - Abort the agent
  /edit     - Edit plan manually (未実装)

agent-hil>
```

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

## Configuration / 設定

### Three Orthogonal Axes / 3つの直交する設定軸

エージェントのオーケストレーション設定は、3 つの独立した軸で構成されています（`SessionMode` に集約）：

| 軸 | 型 | 役割 |
|----|------|------|
| **ConsensusLevel** | Enum (`Solo`, `Ensemble`) | 参加モデル数を制御 |
| **PhaseScope** | Enum (`Full`, `Fast`, `PlanOnly`) | 実行フェーズの範囲を制御 |
| **OrchestrationStrategy** | Enum (`Quorum(QuorumConfig)`, `Debate(DebateConfig)`) | 議論の進め方を選択 |

`OrchestrationStrategy` はバリアントごとに設定を保持する **enum** です。
一方、`StrategyExecutor` は戦略の実行ロジックを定義する **trait** です。
enum が「何を使うか」を、trait が「どう実行するか」を担います。

### Combination Validation / 組み合わせバリデーション

3 軸の組み合わせのうち、無効・未サポートなものは起動時にバリデーションされます。
`SessionMode::validate_combination()` が `Vec<ConfigIssue>` を返し、CLI 層で Warning 表示 or Error 中断します。

| ConsensusLevel | PhaseScope | Strategy | Severity | Code | 理由 |
|---|---|---|---|---|---|
| Solo | * | Debate | **Error** | `SoloWithDebate` | ソロでは議論不可能（1モデルで対立的議論は成立しない） |
| Ensemble | * | Debate | Warning | `DebateNotImplemented` | StrategyExecutor が未実装 |
| Ensemble | Fast | * | Warning | `EnsembleWithFast` | レビュースキップにより Ensemble の価値が減少 |

- Solo + Debate は **Error**（実行不可）として即座に `bail!` します
- Ensemble + Debate は Warning のみ（将来の実装に備えて設定自体は受け付ける）
- Ensemble + Fast は Warning（動作はするが、マルチモデル合議のメリットが薄れる）
- Solo + Debate の場合は `DebateNotImplemented` Warning は省略されます（Error が優先）

定義ファイル: `domain/src/agent/validation.rs`（`Severity`, `ConfigIssueCode`, `ConfigIssue`）

### Configuration Types / 設定型（4型分割）

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

#### Buffer 必要性マップ

| 型 | Agent | Ask | Discuss |
|----|-------|-----|---------|
| `SessionMode` | Yes | No (Solo固定) | Yes |
| `ModelConfig` | Yes | Yes | Yes |
| `AgentPolicy` | Yes | No | No |
| `ExecutionParams` | Yes | Yes | No |

### Role-Based Model Configuration / ロールベースモデル設定

モデルはタスクの性質に応じて使い分けられます：

| Role | Config Key | Default | Purpose |
|------|-----------|---------|---------|
| **Exploration** | `exploration_model` | `claude-haiku-4.5` | コンテキスト収集（高速・低コスト） |
| **Decision** | `decision_model` | `claude-sonnet-4.5` | 計画作成・高リスクツール判断 |
| **Review** | `review_models` | `[claude-sonnet-4.5, gpt-5.2-codex]` | Quorum レビュー投票 |

### TOML Configuration / TOML 設定

```toml
[models]
exploration = "gpt-5.2-codex"           # コンテキスト収集用
decision = "claude-sonnet-4.5"          # 計画作成・高リスクツール判断用
review = ["claude-opus-4.5", "gpt-5.2-codex", "gemini-3-pro-preview"]

[agent]
max_plan_revisions = 3         # 人間介入までの最大修正回数（デフォルト: 3）
hil_mode = "interactive"       # "interactive", "auto_reject", "auto_approve"
consensus_level = "solo"       # "solo" or "ensemble"
phase_scope = "full"           # "full", "fast", "plan-only"
strategy = "quorum"            # "quorum" or "debate"
```

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

- [Quorum Discussion & Consensus](../concepts/quorum.md) - エージェントが使用する合議メカニズム
- [Ensemble Mode](../concepts/ensemble-mode.md) - マルチモデル計画生成モード
- [Tool System](./tool-system.md) - エージェントが使用するツールの詳細
- [Native Tool Use](./native-tool-use.md) - 構造化ツール呼び出し API
- [TUI](../guides/tui.md) - エージェントの TUI インターフェース
- [CLI & Configuration](../guides/cli-and-configuration.md) - エージェントの設定と REPL コマンド

<!-- LLM Context: Agent System は Solo/Ensemble モードでの自律タスク実行。Context Gathering → Planning → Plan Review (Quorum) → Execution Confirm → Task Execution → Final Review のフロー。PhaseScope (Full/Fast/PlanOnly) でフェーズ範囲を制御。高リスクツールは Action Review (QuorumActionReviewer) が必須。HiL は 2 ゲート: Plan Review HiL (max_plan_revisions 到達時) と Execution Confirmation (PhaseScope::Full のみ)。run_agent/ は 5 モジュール分割: mod.rs (メインフロー), types.rs, hil.rs, planning.rs, review.rs。ToolExecution ステートマシン (Pending→Running→Completed/Error) が domain/src/agent/tool_execution.rs。UiEvent 出力ポートが application/src/ports/ui_event.rs で Application→Presentation の構造化イベント伝達。AgentProgressNotifier (application/src/ports/agent_progress.rs) は 6 カテゴリ・26+ コールバック。ResourceReference (domain/src/context/reference.rs) と GitHubReferenceResolver (infrastructure/src/reference/github.rs) で GitHub Issue/PR 自動解決。InteractionForm (Agent/Ask/Discuss) が domain/src/interaction/mod.rs。設定は 4 型分割: SessionMode, ModelConfig (Agent: exploration/decision/review + Interaction: participants/moderator/ask), AgentPolicy, ExecutionParams。QuorumConfig (application) が 4 型コンテナ。組み合わせバリデーション: SessionMode::validate_combination()。 -->
