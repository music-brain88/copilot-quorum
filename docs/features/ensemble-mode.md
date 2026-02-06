# Ensemble Mode / アンサンブルモード

> Research-backed multi-model planning with independent generation and voting
>
> 研究に基づいた独立計画生成と投票によるマルチモデル計画生成

---

## Overview / 概要

Ensemble モードは、複数の LLM が **独立して計画を生成** し、
その後 **投票で最良の計画を選択** するアーキテクチャを採用しています。
機械学習のアンサンブル学習に着想を得ており、単一モデルよりも多角的な視点で
計画の品質を向上させます。

Solo モードが単一モデルの素早い実行に適しているのに対し、
Ensemble モードは複雑な設計判断やアーキテクチャ決定など、
多角的な検討が重要なタスクに最適です。

---

## Quick Start / クイックスタート

```bash
# Ensemble モードで実行
copilot-quorum --ensemble "Design the authentication system"

# REPL で切り替え
copilot-quorum --chat
> /ens                                    # Ensemble モードに切り替え
ensemble> Design the payment integration  # 複数モデルで計画生成
> /solo                                   # Solo モードに戻す
```

```toml
# quorum.toml でデフォルトを Ensemble に
[agent]
consensus_level = "ensemble"
```

---

## How It Works / 仕組み

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

| Phase | Solo | Ensemble |
|-------|------|----------|
| Context Gathering | exploration_model | exploration_model |
| **Planning** | decision_model (1 回) | **review_models (並列生成 + 投票)** |
| Plan Review | review_models (Consensus) | (Planning に含まれる) |
| Execution | decision_model | decision_model |
| Action Review | review_models (Consensus) | review_models (Consensus) |

| Property | Solo | Ensemble |
|----------|------|----------|
| 計画生成コスト | 1x | Nx（N = モデル数） |
| 計画の多様性 | 低 | 高 |
| 精度 | 標準 | 高（多角的視点） |
| 実行速度 | 速い | 計画段階は遅い |
| 適用シーン | シンプルなタスク、バグ修正 | 複雑な設計、アーキテクチャ決定 |

### ML Analogy / ML 的アナロジー

- **Solo** = 単一モデルの予測
- **Ensemble** = 複数モデルを組み合わせて精度・信頼性を向上（アンサンブル学習）

copilot-quorum は **ensemble-after-inference** パラダイムを採用：
各モデルが完全な応答（計画）を生成した後に集約します。

---

## Research Evidence / 研究エビデンス

Ensemble モードの設計は、以下の最新研究に基づいています。

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
- Optimal Weight (OW)、Inverse Surprising Popularity (ISP) などの高度な集約アルゴリズムを提案

### 4. Ensemble パラダイム

**"Harnessing Multiple LLMs: A Survey on LLM Ensemble"** (2025)
- [論文リンク](https://arxiv.org/html/2502.18036v1)

| Paradigm | Description | Timing |
|----------|-------------|--------|
| **ensemble-before-inference** | 適切なモデルにルーティング | クエリ受信時 |
| **ensemble-during-inference** | デコード中に集約 | 生成中 |
| **ensemble-after-inference** | 完全な応答を集約 | 生成後 |

copilot-quorum は **ensemble-after-inference** を採用。

### 5. Multi-Agent Collaboration のメリット

**"Multi-Agent Collaboration Mechanisms: A Survey"** (2025)
- [論文リンク](https://arxiv.org/html/2501.06322v1)
- 複雑なマルチステップタスクでは協調的アプローチが独立生成を大幅に上回る
- ただしコーディネーションのオーバーヘッドあり

---

## Design Decision / 設計判断

### なぜ Independent Generation + Voting か

| Reason | Detail |
|--------|--------|
| 研究で効果が実証済み | Debate より Voting の方が効率的 |
| Degeneration of thought を回避 | 独立生成で多様性を維持 |
| 実装がシンプル | 既存の RunQuorumUseCase を再利用 |
| コスト効率 | 議論のラウンドが少ない（1 往復のみ） |

### 不採用としたアプローチ

| Approach | Reason |
|----------|--------|
| Multi-Agent Debate | 議論が収束し多様性喪失、コスト高 |
| Sequential Refinement | 後のモデルが前のモデルに引きずられる |
| Real-time Collaboration | 実装複雑、API の制約 |

---

## Configuration / 設定

```toml
# quorum.toml
[agent]
consensus_level = "ensemble"   # "solo" or "ensemble"

[quorum.discussion]
models = ["claude-sonnet-4.5", "gpt-5.2-codex", "gemini-3-pro-preview"]
```

CLI フラグ:

```bash
copilot-quorum --ensemble "Your task"   # Ensemble モードで実行
```

REPL コマンド:

```
/ens       # Ensemble モードに切り替え
/solo      # Solo モードに戻す
```

---

## Architecture / アーキテクチャ

### Key Files / 主要ファイル

| File | Description |
|------|-------------|
| `domain/src/orchestration/mode.rs` | `ConsensusLevel` enum (`Solo`, `Ensemble`) + `PlanningApproach` |
| `domain/src/agent/entities.rs` | `PlanCandidate` struct（候補計画 + 投票スコア） |
| `domain/src/agent/entities.rs` | `EnsemblePlanResult` struct（選択結果） |
| `application/src/use_cases/run_agent.rs` | `generate_ensemble_plans()`, `vote_on_plans()`, `select_best_plan()` |
| `presentation/src/agent/repl.rs` | REPL でのモード切り替え UI |

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

## Future Enhancements / 将来の拡張

### 高度な集約アルゴリズム

研究（"Beyond Majority Voting"）で示された高度な手法の導入:

- **Optimal Weight (OW)** - モデルの信頼性に基づく重み付け
- **Inverse Surprising Popularity (ISP)** - 意外な合意に高いウェイト
- **Confidence-weighted voting** - 各モデルの自信度を考慮

### ハイブリッドアプローチ

特定の条件下では Debate も有効:

- 初期計画に大きな不一致がある場合 → 短い Debate フェーズを追加
- 投票結果が僅差の場合 → 追加の議論を実施

---

## Related Features / 関連機能

- [Quorum Discussion & Consensus](./quorum.md) - Ensemble が活用する合議メカニズム
- [Agent System](./agent-system.md) - Ensemble 計画の実行フロー
- [CLI & Configuration](./cli-and-configuration.md) - `/ens` コマンドと設定

## References / 参考文献

1. "Debate or Vote: Which Yields Better Decisions" (ACL 2025)
2. "Multi-LLM-Agents Debate" (ICLR Blogposts 2025)
3. "Beyond Majority Voting" (NeurIPS 2024)
4. "Harnessing Multiple LLMs: A Survey on LLM Ensemble" (2025)
5. "Multi-Agent Collaboration Mechanisms: A Survey" (2025)

<!-- LLM Context: Ensemble モードは複数モデルが独立して計画を生成し、投票で最良の計画を選択する。ensemble-after-inference パラダイム。Solo モードとは Planning フェーズだけが異なり、実行フローは同じ。ConsensusLevel enum（Solo/Ensemble）で切り替え、PlanningApproach は ConsensusLevel から自動導出。主要ファイルは domain/src/orchestration/mode.rs と application/src/use_cases/run_agent.rs。 -->
