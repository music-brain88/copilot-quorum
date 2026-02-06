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

```
User Request
    │
    ▼
┌───────────────────┐
│ Context Gathering │  ← プロジェクト情報収集 (glob, read_file)
└───────────────────┘    exploration_model を使用
    │
    ▼
┌───────────────────┐
│     Planning      │  ← 単一モデルがタスク計画を作成
└───────────────────┘    decision_model を使用
    │
    ▼
╔═══════════════════════════╗
║  Quorum Consensus #1     ║  ← 全モデルが計画をレビュー（必須）
║  Plan Review              ║    QuorumRule に基づく投票で承認/却下
╚═══════════════════════════╝
    │
    ▼
┌───────────────────┐
│  Task Execution   │
│   ├─ Low-risk  ────▶ 直接実行
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
| **Plan Review** | 計画作成後 | 必須 | 計画の安全性・正確性を検証 |
| **Action Review** | 高リスクツール実行前 | 高リスク操作時 | 書き込み・コマンド実行の安全性検証 |
| **Final Review** | 全タスク完了後 | オプション | 実行結果全体の品質検証 |

### Risk-Based Tool Classification / リスクベースのツール分類

| Tool | Risk Level | Quorum Review | Rationale |
|------|------------|---------------|-----------|
| `read_file` | Low | No | 読み取り専用 |
| `glob_search` | Low | No | 読み取り専用 |
| `grep_search` | Low | No | 読み取り専用 |
| `write_file` | **High** | **Yes** | ファイルシステムを変更 |
| `run_command` | **High** | **Yes** | 外部コマンド実行、元に戻すのが困難 |

### Plan Review Details / 計画レビューの詳細

計画レビューは **常に必須** です（`require_plan_review: true` は変更不可）。

1. 全ての review_models に計画を並列送信
2. 各モデルが APPROVE / REJECT を投票
3. QuorumRule（デフォルト: 過半数）で判定
4. 却下時は全モデルのフィードバックを集約し、計画を修正 → 再投票

### Action Review Details / アクションレビューの詳細

高リスクツール（`write_file`, `run_command`）の実行前に自動発動します。

判断基準:
- 操作は必要か？
- 引数は正しいか？
- より安全な代替手段はないか？

却下されたアクションはスキップされます（エラーではない）。

---

## Human-in-the-Loop (HiL) / 人間介入

### Overview / 概要

Quorum が合意に至らない場合、エージェントは無限にリトライする代わりに、
ユーザーに判断を委ねることができます。

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

## Configuration / 設定

### AgentConfig

```rust
pub struct AgentConfig {
    // ---- Role-based Model Configuration ----
    pub exploration_model: Model,      // コンテキスト収集用（デフォルト: Haiku）
    pub decision_model: Model,         // 計画作成・高リスクツール判断用（デフォルト: Sonnet）
    pub review_models: Vec<Model>,     // Quorum レビュー投票用

    // ---- Orchestration Configuration ----
    pub consensus_level: ConsensusLevel,             // Solo or Ensemble
    pub phase_scope: PhaseScope,                     // Full, Fast, PlanOnly
    pub orchestration_strategy: OrchestrationStrategy, // Quorum or Debate

    // ---- Behavior Configuration ----
    pub require_plan_review: bool,     // 常に true（計画レビューは必須）
    pub require_final_review: bool,    // 最終レビューを有効化
    pub max_iterations: usize,         // 最大実行イテレーション数
    pub working_dir: Option<String>,   // ツール実行の作業ディレクトリ
    pub max_tool_retries: usize,       // ツールバリデーションエラー時の最大リトライ数
    pub max_plan_revisions: usize,     // 人間介入までの最大計画修正回数
    pub hil_mode: HilMode,            // 人間介入モード
}
```

### Role-Based Model Configuration / ロールベースモデル設定

モデルはタスクの性質に応じて使い分けられます：

| Role | Config Key | Default | Purpose |
|------|-----------|---------|---------|
| **Exploration** | `exploration_model` | `claude-haiku-4.5` | コンテキスト収集（高速・低コスト） |
| **Decision** | `decision_model` | `claude-sonnet-4.5` | 計画作成・高リスクツール判断 |
| **Review** | `review_models` | `[claude-sonnet-4.5, gpt-5.2-codex]` | Quorum レビュー投票 |

### TOML Configuration / TOML 設定

```toml
[agent]
max_plan_revisions = 3         # 人間介入までの最大修正回数（デフォルト: 3）
hil_mode = "interactive"       # "interactive", "auto_reject", "auto_approve"
consensus_level = "solo"       # "solo" or "ensemble"
phase_scope = "full"           # "full", "fast", "plan-only"
strategy = "quorum"            # "quorum" or "debate"
exploration_model = "claude-haiku-4.5"
decision_model = "claude-sonnet-4.5"
review_models = ["claude-sonnet-4.5", "gpt-5.2-codex"]
```

---

## Architecture / アーキテクチャ

### Key Files / 主要ファイル

| File | Description |
|------|-------------|
| `domain/src/agent/entities.rs` | `AgentState`, `AgentConfig`, `Plan`, `Task`, `AgentPhase` |
| `domain/src/agent/value_objects.rs` | `AgentId`, `AgentContext`, `TaskResult`, `Thought` |
| `domain/src/tool/entities.rs` | `ToolDefinition`, `ToolCall`, `ToolSpec`, `RiskLevel` |
| `domain/src/tool/value_objects.rs` | `ToolResult`, `ToolError` |
| `domain/src/tool/traits.rs` | `ToolValidator` |
| `domain/src/prompt/agent.rs` | `AgentPromptTemplate` |
| `application/src/ports/tool_executor.rs` | `ToolExecutorPort` trait |
| `application/src/use_cases/run_agent.rs` | `RunAgentUseCase`, `RunAgentInput`, `RunAgentOutput`, `AgentProgressNotifier` |
| `infrastructure/src/tools/` | `LocalToolExecutor` 実装 |
| `presentation/src/agent/repl.rs` | エージェント REPL UI |

### Data Flow / データフロー

```
RunAgentUseCase (Application層)
│
├── gather_context()
│   └── ToolExecutorPort.execute(glob_search, read_file)
│
├── create_plan()
│   └── LlmSession.chat() → Plan
│
├── review_plan() → QuorumReviewResult
│   ├── Model A: Vote (APPROVE/REJECT)
│   ├── Model B: Vote
│   └── Model C: Vote
│
├── execute_tasks()
│   ├── Low-risk tool → ToolExecutorPort.execute()
│   └── High-risk tool → review_action() → ToolExecutorPort.execute()
│
└── final_review() → QuorumReviewResult (optional)
```

### Progress Notification / 進捗通知

エージェントは「アクションと UI 通知の分離」パターンを採用しています。

> **Note**: 進捗通知には 2 つの trait があります：
> - `ProgressNotifier`（`domain/src/orchestration/strategy.rs`）— Quorum Discussion のフェーズ進行通知（ドメイン層）
> - `AgentProgressNotifier`（`application/src/use_cases/run_agent.rs`）— エージェント実行全体の進捗通知（アプリケーション層）
>
> 両者は別の trait であり、`ProgressNotifier` は Quorum Discussion の各フェーズ（Initial Query → Peer Review → Synthesis）を、`AgentProgressNotifier` はエージェントの各フェーズ（Planning → Execution → Review 等）を通知します。

| Layer | Responsibility |
|-------|---------------|
| **低レベル関数** (`review_plan`, `review_action`) | ビジネスロジック実行、結果を返す |
| **メインループ** (`execute_with_progress`) | 結果に基づき UI 通知を発火 |
| **AgentProgressNotifier** (Presentation 層) | UI 更新、フィードバック表示 |

```rust
pub trait AgentProgressNotifier: Send + Sync {
    fn on_phase_change(&self, phase: &AgentPhase);
    fn on_thought(&self, thought: &Thought);
    fn on_task_start(&self, task: &Task);
    fn on_task_complete(&self, task: &Task, success: bool);
    fn on_tool_call(&self, tool_name: &str, args: &str);
    fn on_tool_result(&self, tool_name: &str, success: bool);
    fn on_quorum_start(&self, phase: &str, model_count: usize);
    fn on_quorum_model_complete(&self, model: &Model, approved: bool);
    fn on_quorum_complete(&self, phase: &str, approved: bool, feedback: Option<&str>);
    // ... tool error/retry, plan revision, action retry,
    //     quorum_complete_with_votes, human_intervention_required,
    //     ensemble start/plan_generated/voting_start/complete
    //     (全 18 メソッド — 詳細は application/src/use_cases/run_agent.rs 参照)
}
```

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

- [Quorum Discussion & Consensus](./quorum.md) - エージェントが使用する合議メカニズム
- [Ensemble Mode](./ensemble-mode.md) - マルチモデル計画生成モード
- [Tool System](./tool-system.md) - エージェントが使用するツールの詳細
- [CLI & Configuration](./cli-and-configuration.md) - エージェントの設定と REPL コマンド

<!-- LLM Context: Agent System は Solo モードでの自律タスク実行。Context Gathering → Planning → Plan Review (Quorum Consensus) → Execution → Final Review のフロー。高リスクツールは Action Review が必須。HiL で人間介入も可能。AgentConfig は ConsensusLevel（Solo/Ensemble）、PhaseScope（Full/Fast/PlanOnly）、OrchestrationStrategy（Quorum/Debate）の3つの直交軸で設定。主要ファイルは domain/src/agent/、application/src/use_cases/run_agent.rs、infrastructure/src/tools/。 -->
