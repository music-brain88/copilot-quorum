# Ensemble Architecture / アンサンブルアーキテクチャ

> Research-backed design for multi-model planning
>
> 研究に基づいた複数モデル計画生成の設計

---

## Overview / 概要

Ensemble モードは、複数の LLM が **独立して計画を生成**し、その後 **投票で最良の計画を選択** するアーキテクチャを採用しています。
このアプローチは、最新の研究から得られた知見に基づいています。

---

## Research Evidence / 研究エビデンス

### 1. Debate vs Voting の比較

**"Debate or Vote: Which Yields Better Decisions"** (ACL 2025)
- [論文リンク](https://arxiv.org/pdf/2508.17536)
- **発見**: Majority voting だけでも、Debate を含むアプローチと同等のパフォーマンス
- **示唆**: 複数モデルで長々と議論するより、**独立生成 + 投票** が効率的

### 2. Multi-Agent Debate (MAD) の限界

**ICLR Blogposts 2025** - Multi-LLM-Agents Debate
- [ブログポスト](https://d2jud02ci9yv69.cloudfront.net/2025-04-28-mad-159/blog/mad/)
- **発見**: 現状の MAD は「単純な単一エージェント戦略を一貫して上回れていない」
- **問題点**:
  - **Degeneration of thought** - 議論が収束して多様性が失われる
  - **Majority herding** - 多数派に引きずられる
  - **Overconfident consensus** - 誤った合意に過度な自信

### 3. より賢い集約方法

**"Beyond Majority Voting"** (NeurIPS 2024)
- [論文リンク](https://arxiv.org/abs/2510.01499)
- 単純な多数決は「モデル間の異質性や相関を考慮しない」
- Optimal Weight (OW)、Inverse Surprising Popularity (ISP) などの高度な集約アルゴリズム

### 4. Ensemble パラダイム

**"Harnessing Multiple LLMs: A Survey on LLM Ensemble"** (2025)
- [論文リンク](https://arxiv.org/html/2502.18036v1)

| パラダイム | 説明 | 適用タイミング |
|-----------|------|---------------|
| **ensemble-before-inference** | 適切なモデルにルーティング | クエリ受信時 |
| **ensemble-during-inference** | デコード中に集約 | 生成中 |
| **ensemble-after-inference** | 完全な応答を集約 | 生成後 |

copilot-quorum は **ensemble-after-inference** パラダイムを採用。

### 5. Multi-Agent Collaboration のメリット

**"Multi-Agent Collaboration Mechanisms: A Survey"** (2025)
- [論文リンク](https://arxiv.org/html/2501.06322v1)
- 複雑なマルチステップタスクでは協調的アプローチが独立生成を大幅に上回る
- ただしコーディネーションのオーバーヘッドあり

---

## Design Decision / 設計判断

### なぜ Independent Generation + Voting か

研究結果に基づき、**ensemble-after-inference** パラダイムを採用した理由:

1. **研究で効果が実証済み** - Debate より Voting の方が効率的
2. **Degeneration of thought を回避** - 独立生成で多様性を維持
3. **実装がシンプル** - 既存の RunQuorumUseCase を再利用できる
4. **コスト効率** - 議論のラウンドが少ない（1往復のみ）

### 不採用としたアプローチ

| アプローチ | 不採用理由 |
|-----------|-----------|
| Multi-Agent Debate | 議論が収束し多様性喪失、コスト高 |
| Sequential Refinement | 後のモデルが前のモデルに引きずられる |
| Real-time Collaboration | 実装複雑、API の制約 |

---

## Architecture / アーキテクチャ

### Ensemble Planning Flow

```
┌─────────────────────────────────────────────────────────────────┐
│  Ensemble Planning Flow                                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Step 1: Independent Plan Generation (並列)                      │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐           │
│  │   Model A    │  │   Model B    │  │   Model C    │           │
│  │   (Claude)   │  │   (GPT)      │  │   (Gemini)   │           │
│  │              │  │              │  │              │           │
│  │  Plan A:     │  │  Plan B:     │  │  Plan C:     │           │
│  │  - Task 1    │  │  - Task 1    │  │  - Task 1    │           │
│  │  - Task 2    │  │  - Task 2    │  │  - Task 2    │           │
│  │  - Task 3    │  │  - Task 3    │  │  - Task 3    │           │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘           │
│         │                 │                 │                    │
│         └─────────────────┼─────────────────┘                    │
│                           ▼                                      │
│  Step 2: Plan Comparison & Voting                                │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │  各モデルが他の計画をレビュー                             │  │
│  │  - Plan A の評価: B→7/10, C→8/10  → 平均 7.5              │  │
│  │  - Plan B の評価: A→6/10, C→7/10  → 平均 6.5              │  │
│  │  - Plan C の評価: A→8/10, B→6/10  → 平均 7.0              │  │
│  │                                                           │  │
│  │  結果: Plan A が最高スコア → 採用                         │  │
│  └───────────────────────────────────────────────────────────┘  │
│                           ▼                                      │
│  Step 3: Execute Selected Plan                                   │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │  Solo と同じ実行フロー（decision_model が実行）           │  │
│  │  - High-risk 操作は Quorum Consensus でレビュー           │  │
│  └───────────────────────────────────────────────────────────┘  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Solo vs Ensemble 比較

| フェーズ | Solo | Ensemble |
|---------|------|----------|
| Context Gathering | exploration_model | exploration_model |
| **Planning** | decision_model (1回) | **review_models (並列生成 + 投票)** |
| Plan Review | review_models (Consensus) | (Planning に含まれる) |
| Execution | decision_model | decision_model |
| Action Review | review_models (Consensus) | review_models (Consensus) |

| 特性 | Solo | Ensemble |
|------|------|----------|
| 計画生成コスト | 1x | Nx（N = モデル数） |
| 計画の多様性 | 低 | 高 |
| 精度 | 標準 | 高（多角的視点） |
| 実行速度 | 速い | 計画段階は遅い |
| 適用シーン | シンプルなタスク | 複雑な設計・判断 |

---

## Data Structures / データ構造

### PlanningMode

```rust
/// Ensemble planning mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanningMode {
    /// Single model generates the plan (Solo mode)
    #[default]
    Single,
    /// Multiple models generate plans independently, then vote (Ensemble mode)
    Ensemble,
}
```

### PlanCandidate

```rust
/// A plan candidate from ensemble planning
#[derive(Debug, Clone)]
pub struct PlanCandidate {
    /// Model that generated this plan
    pub model: Model,
    /// The generated plan
    pub plan: Plan,
    /// Votes received from other models (model -> score)
    pub votes: HashMap<Model, f64>,
}

impl PlanCandidate {
    /// Calculate the average score from all votes
    pub fn average_score(&self) -> f64 {
        if self.votes.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.votes.values().sum();
        sum / self.votes.len() as f64
    }
}
```

### EnsemblePlanResult

```rust
/// Result of ensemble planning
#[derive(Debug, Clone)]
pub struct EnsemblePlanResult {
    /// All plan candidates with their votes
    pub candidates: Vec<PlanCandidate>,
    /// The selected plan (highest score)
    pub selected: Plan,
    /// Model that generated the selected plan
    pub selected_by: Model,
}
```

---

## Implementation Plan / 実装計画

### Phase 1.5 Tasks

1. **Domain Layer** (`domain/src/agent/`)
   - `PlanningMode` enum 追加
   - `PlanCandidate` struct 追加
   - `EnsemblePlanResult` struct 追加

2. **Application Layer** (`application/src/use_cases/run_agent.rs`)
   - `generate_ensemble_plans()` - 複数モデルで並列に計画生成
   - `vote_on_plans()` - 各モデルが他の計画をレビュー・採点
   - `select_best_plan()` - 最高スコアの計画を選択

3. **Presentation Layer** (`presentation/src/agent/`)
   - Ensemble モード時のヘッダー表示更新
   - 計画生成・投票の進捗表示

### Voting Prompt Template

```
あなたは計画レビュアーです。
以下の計画を評価し、1-10のスコアと理由を提供してください。

## 評価基準
- 完全性: タスクの要件をすべてカバーしているか
- 実現可能性: 各ステップが技術的に実現可能か
- 効率性: 無駄なステップがないか
- リスク管理: エラーハンドリングを考慮しているか

## 計画
{plan}

## 出力形式
{
  "score": <1-10>,
  "reasoning": "<評価理由>"
}
```

---

## Future Enhancements / 将来の拡張

### 高度な集約アルゴリズム

研究（"Beyond Majority Voting"）で示された高度な手法:

- **Optimal Weight (OW)** - モデルの信頼性に基づく重み付け
- **Inverse Surprising Popularity (ISP)** - 意外な合意に高いウェイト
- **Confidence-weighted voting** - 各モデルの自信度を考慮

### ハイブリッドアプローチ

特定の条件下では Debate も有効:

- 初期計画に大きな不一致がある場合 → 短い Debate フェーズを追加
- 投票結果が僅差の場合 → 追加の議論を実施

---

## References / 参考文献

1. "Debate or Vote: Which Yields Better Decisions" (ACL 2025)
2. "Multi-LLM-Agents Debate" (ICLR Blogposts 2025)
3. "Beyond Majority Voting" (NeurIPS 2024)
4. "Harnessing Multiple LLMs: A Survey on LLM Ensemble" (2025)
5. "Multi-Agent Collaboration Mechanisms: A Survey" (2025)
