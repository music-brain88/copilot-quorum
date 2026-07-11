# Orchestration Internals / オーケストレーション内部構造

> Key files, data flows and types behind Quorum Discussion, Consensus and Ensemble planning
>
> Quorum Discussion / Consensus / Ensemble 計画生成を支える主要ファイル・データフロー・型

---

## Quorum Discussion

### Key Files / 主要ファイル

| File | Description |
|------|-------------|
| `domain/src/quorum/vote.rs` | `Vote`, `VoteResult` 型の定義 |
| `domain/src/quorum/rule.rs` | `QuorumRule` 合意ルール定義 |
| `domain/src/quorum/consensus.rs` | `ConsensusRound`, `ConsensusOutcome` 定義 |
| `domain/src/orchestration/` | `Phase`, `QuorumRun`, `QuorumResult`, `OrchestrationStrategy` |
| `application/src/use_cases/run_quorum/` | `RunQuorumUseCase`（ディスパッチ）+ `StrategyExecutor` 実装群 |
| `domain/src/prompt/template.rs` | 各フェーズのプロンプトテンプレート |

### Data Flow / データフロー

```
User Question
    │
    ▼
RunQuorumUseCase
    │
    ├── Phase 1: Initial Query
    │   ├── Model A (parallel) → Response A
    │   ├── Model B (parallel) → Response B
    │   └── Model C (parallel) → Response C
    │
    ├── Phase 2: Peer Review
    │   ├── A reviews [B, C] (anonymized)
    │   ├── B reviews [A, C] (anonymized)
    │   └── C reviews [A, B] (anonymized)
    │
    └── Phase 3: Synthesis
        └── Moderator synthesizes all → Final Consensus
```

非同期処理は `tokio` ランタイム上で `JoinSet` を使って並列化されています。

### StrategyExecutor

Quorum Discussion の実行フローはプラグイン可能な戦略パターンで実装されています。
`StrategyExecutor` trait を実装することで、新しい議論戦略を追加できます。実行には
`LlmGateway`/`ProgressNotifier` という I/O ポートへの依存が必須なため、trait 自体は
domain ではなく application 層（`application/src/use_cases/run_quorum/`）にあります。
`OrchestrationStrategy` enum（domain, 2 バリアント）は `RunQuorumUseCase` が
exhaustive match でディスパッチします — `dyn StrategyExecutor` によるトレイト
オブジェクトディスパッチではありません（#314）。

```rust
/// Trait for executing orchestration strategies
/// (application/src/use_cases/run_quorum/strategy_executor.rs)
#[async_trait]
pub trait StrategyExecutor: Send + Sync {
    fn name(&self) -> &'static str;
    fn phases(&self) -> Vec<Phase>;
    async fn execute(
        &self,
        input: &RunQuorumInput,
        gateway: Arc<dyn LlmGateway>,
        progress: &dyn ProgressNotifier,
    ) -> Result<QuorumResult, RunQuorumError>;
}
```

| Strategy | Phases | Description |
|----------|--------|-------------|
| `QuorumStrategyExecutor` | Initial → Review → Synthesis | 対等な議論（デフォルト）。旧 `RunQuorumUseCase` 本体を抽出したもの |
| `DebateStrategyExecutor` | Initial（立場割り当て+オープニング）→ Review（攻撃/防衛ラウンド×`max_rounds`、モデレーター早期決着判定）→ Synthesis | 敵対的討議。提案側/批判側の固定ロール + 任意の第三者乱入（`allow_interjection`） |

定義ファイル: `application/src/use_cases/run_quorum/`
（`strategy_executor.rs`, `quorum_strategy.rs`, `debate_strategy.rs`）

---

## Quorum Consensus Types / 合意形成の型

### QuorumRule / 合意ルール

| Rule | Description | Example |
|------|-------------|---------|
| `Majority` | 過半数の承認で可決（デフォルト） | 3 モデル中 2 以上で承認 |
| `Unanimous` | 全員一致で可決 | 3 モデル全員が承認 |
| `AtLeast(n)` | 最低 n 票の承認で可決 | `atleast:2` → 2 票以上で承認 |
| `Percentage(p)` | p% 以上の承認で可決 | `75%` → 75% 以上で承認 |

### Vote / 投票

```rust
pub struct Vote {
    pub model: String,           // モデル識別子
    pub approved: bool,          // 承認/拒否
    pub reasoning: String,       // 理由・フィードバック
    pub confidence: Option<f64>, // 信頼度 (0.0-1.0)
}
```

投票結果は `VoteResult` に集約され、視覚的なサマリー（例: `[●●○]`）で表示されます。
`●` は承認、`○` は却下を表します。

### ConsensusRound / 合意ラウンド

合意形成の 1 ラウンドを記録するエンティティです。
エージェントのプランレビューで却下された場合、修正 → 再投票のサイクルを複数ラウンド実行できます。

```rust
pub struct ConsensusRound {
    pub round: usize,              // ラウンド番号（1から）
    pub outcome: ConsensusOutcome, // Approved / Rejected / Pending
    pub rule: QuorumRule,          // 使用されたルール
    pub votes: Vec<Vote>,         // 個別投票
    pub result: VoteResult,       // 集約結果
}
```

---

## Ensemble Planning

### Key Files / 主要ファイル

| File | Description |
|------|-------------|
| `domain/src/orchestration/mode.rs` | `ConsensusLevel` enum (`Solo`, `Ensemble`) + `PlanningApproach` |
| `domain/src/agent/entities.rs` | `PlanCandidate` struct（候補計画 + 投票スコア） |
| `domain/src/agent/entities.rs` | `EnsemblePlanResult` struct（選択結果） |
| `application/src/use_cases/run_agent.rs` | `generate_ensemble_plans()`, `vote_on_plans()`, `select_best_plan()` |
| `presentation/src/tui/app.rs` | TUI でのモード切り替え UI |

### Data Flow / データフロー

```
RunAgentUseCase (Ensemble mode)
│
├── gather_context()     ← Solo と同じ
│
├── generate_ensemble_plans()
│   ├── Model A → Plan A (parallel)
│   ├── Model B → Plan B (parallel)
│   └── Model C → Plan C (parallel)
│
├── vote_on_plans()
│   ├── Model A reviews [Plan B, Plan C]
│   ├── Model B reviews [Plan A, Plan C]
│   └── Model C reviews [Plan A, Plan B]
│
├── select_best_plan()
│   └── Highest average score → Selected Plan
│
└── execute_tasks()      ← Solo と同じ
```

### Key Data Structures / 主要データ構造

```rust
/// Consensus level — the single user-facing mode axis
pub enum ConsensusLevel {
    Solo,      // Single model driven (default)
    Ensemble,  // Multi-model driven
}

/// Planning approach — derived from ConsensusLevel
pub enum PlanningApproach {
    Single,    // Derived from Solo
    Ensemble,  // Derived from Ensemble
}

/// A plan candidate from ensemble planning
pub struct PlanCandidate {
    pub model: Model,                    // 生成したモデル
    pub plan: Plan,                      // 生成された計画
    pub votes: HashMap<String, f64>,     // 他モデルからのスコア (model name -> 1-10)
}

impl PlanCandidate {
    pub fn average_score(&self) -> f64;  // 全投票の平均スコアを計算
    pub fn vote_count(&self) -> usize;   // 投票数を取得
    pub fn vote_summary(&self) -> String; // "GPT:8/10, Gemini:7/10" 形式の要約
}

/// Result of ensemble planning
pub struct EnsemblePlanResult {
    pub candidates: Vec<PlanCandidate>,  // 全候補計画
    pub selected_index: usize,           // 選択された計画のインデックス
}

impl EnsemblePlanResult {
    pub fn select_best(candidates: Vec<PlanCandidate>) -> Self; // 最高平均スコアの計画を選択
    pub fn selected(&self) -> Option<&PlanCandidate>;           // 選択された候補への参照
    pub fn into_selected(self) -> Option<PlanCandidate>;        // 選択された候補を所有権ごと取得
}
```

---

## Related / 関連

- [Quorum Discussion & Consensus](../explanation/quorum-consensus.md) - 合議の仕組みと設計意図
- [Ensemble Mode](../explanation/ensemble-mode.md) - 独立生成+投票の設計判断と研究エビデンス
- [Agent System Reference](./agent-system.md) - Consensus を使う計画・アクションレビューの実装
- [Architecture](./architecture.md) - レイヤー構造全体
