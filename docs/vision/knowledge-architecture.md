# Knowledge-Driven Architecture / 知識駆動型アーキテクチャ

> 🔴 **Status**: Not implemented — Concept phase
>
> Based on [Discussion #43](https://github.com/music-brain88/copilot-quorum/discussions/43)

---

## Overview / 概要

copilot-quorum を「複数 LLM の合議ツール」から **「知識駆動型エージェント基盤」** へ進化させる構想。
3 層アーキテクチャ（Knowledge Layer / Context Layer / Workflow Layer）により、
プロジェクト固有の知識を蓄積・活用し、コンテキストを構造化して、
ワークフローを動的に制御できるようにします。

> **Note**: これは将来ビジョンであり、現時点では設計段階です。
> 実装には複数のフェーズを要し、既存の Agent System / Quorum Discussion を段階的に拡張していく想定です。

---

## Motivation / 動機

### 現状の課題

```
現在の実装:
┌─────────────────────┐
│ Phase-based 実行    │ ← 線形フロー
│ (Context→Plan→Exec) │
├─────────────────────┤
│ Quorum Review       │ ← チェックポイント
│ (Plan/Action/Final) │
├─────────────────────┤
│ Tool Execution      │ ← 単発実行
└─────────────────────┘
```

| Layer | Current State | Ideal State |
|-------|--------------|-------------|
| **Workflow** | Phase 列挙 + match（線形） | グラフノードで動的遷移、並列 Agent |
| **Context** | AgentState.thoughts + session 履歴 | LLM 間の関係（同意/反論/補足）を構造化 |
| **Knowledge** | なし 🔴 | プロジェクト構造、推論キャッシュ、学習 |

---

## Proposed Architecture / 提案アーキテクチャ

### 3-Layer Model

```
┌─────────────────────────────────────────────────────────────────┐
│                     GitHub Discussions                          │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐            │
│  │ RFC: 設計A   │ │ HiL: Task X  │ │ 学習: login  │            │
│  │ 議論→決定   │ │ Plan→承認   │ │ 修正パターン │            │
│  └──────────────┘ └──────────────┘ └──────────────┘            │
└─────────────────────────────────────────────────────────────────┘
         ↑ write                    ↓ read
┌─────────────────────────────────────────────────────────────────┐
│                     Knowledge Layer                             │
│  - 設計決定の履歴                                               │
│  - 過去の Plan/Review 結果                                      │
│  - プロジェクト固有のパターン                                   │
│  - HiL State                                                    │
└─────────────────────────────────────────────────────────────────┘
         ↑↓
┌─────────────────────────────────────────────────────────────────┐
│                     Context Layer                               │
│  - 議論グラフ（同意/反論/補足の関係性）                        │
│  - LLM 間の関係性の構造化                                       │
│  - セッション履歴                                               │
└─────────────────────────────────────────────────────────────────┘
         ↑↓
┌─────────────────────────────────────────────────────────────────┐
│                     Workflow Layer                              │
│  - グラフベースの状態遷移                                       │
│  - 並列 Agent 実行                                              │
│  - 動的フロー制御                                               │
└─────────────────────────────────────────────────────────────────┘
```

### UX と内部の対応

```
ユーザー視点                    内部で動くもの
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
You: ファイル構成教えて
                              → Knowledge Graph 参照
                              → なければ Workflow: 探索

You: /council この設計どう？
                              → Workflow: 並列 LLM 起動
                              → Context: 議論グラフ構築
                              → Knowledge: 関連情報注入

You: login.rs 直して
                              → Workflow: Plan→Review→Exec
                              → Context: 各ステップの関係性
                              → Knowledge: 過去の修正パターン
```

---

## Knowledge Layer Design / Knowledge Layer 設計案

### KnowledgeStore trait

```rust
// ⚠️ 未実装 — 設計案
#[async_trait]
pub trait KnowledgeStore: Send + Sync {
    /// 知識を保存
    async fn store(&self, entry: &KnowledgeEntry) -> Result<KnowledgeId>;

    /// ID で取得
    async fn get(&self, id: &KnowledgeId) -> Result<Option<KnowledgeEntry>>;

    /// クエリで検索（セマンティック検索対応）
    async fn query(&self, query: &KnowledgeQuery) -> Result<Vec<KnowledgeEntry>>;

    /// 関連知識を取得
    async fn get_related(&self, id: &KnowledgeId) -> Result<Vec<KnowledgeEntry>>;
}
```

### KnowledgeEntry の種類

```rust
// ⚠️ 未実装 — 設計案
pub enum KnowledgeEntry {
    /// HiL の状態（承認待ち、完了など）
    HilState(HilState),

    /// 設計決定
    DesignDecision {
        title: String,
        context: String,
        decision: String,
        consequences: Vec<String>,
        discussion_url: Option<String>,
    },

    /// 学習したパターン
    LearnedPattern {
        trigger: String,      // "login.rs の認証エラー"
        pattern: String,      // "トークン有効期限チェック"
        confidence: f32,
        examples: Vec<String>,
    },

    /// Quorum Review 結果
    ReviewResult {
        plan_summary: String,
        votes: Vec<Vote>,
        consensus: ConsensusType,
    },
}
```

### Storage 実装の想定

| Implementation | Use Case | Features |
|----------------|----------|----------|
| `LocalFileStore` | デフォルト | `~/.quorum/knowledge/`、依存なし |
| `GitHubDiscussionStore` | チーム共有 | Discussion をストレージとして活用 |
| `SQLiteStore` | 高度な検索 | 全文検索、関係クエリ |
| `CompositeStore` | 組み合わせ | ローカル + GitHub 同期 |

---

## Context Layer Design / Context Layer 設計案

### 議論グラフ

LLM 間の議論を構造化して、同意/反論/補足の関係を追跡する：

```rust
// ⚠️ 未実装 — 設計案
pub struct DiscussionGraph {
    nodes: Vec<DiscussionNode>,
    edges: Vec<DiscussionEdge>,
}

pub struct DiscussionNode {
    pub id: NodeId,
    pub model: Model,
    pub content: String,
    pub stance: Stance,  // Support, Oppose, Neutral, Question
}

pub enum Relation {
    Agrees,
    Disagrees,
    Extends,
    Questions,
}
```

### 可視化イメージ

```
Claude: "認証はJWTがいい"
    │
    ├──[Agrees]── GPT: "同意、ステートレスで良い"
    │
    └──[Disagrees]── Gemini: "セッションの方が安全"
                        │
                        └──[Questions]── Claude: "具体的なリスクは？"
```

---

## GitHub Discussions Integration / GitHub Discussions 連携構想

### カテゴリ設計

| Category | Purpose | Author |
|----------|---------|--------|
| `RFC` | 設計議論 | ユーザー |
| `HiL Reviews` | Plan 承認待ち | Agent 自動 |
| `Knowledge Base` | 学習した知識 | Agent 自動 |
| `Retrospective` | 完了タスクの振り返り | Agent 自動 |

### 設定の想定

```toml
# ⚠️ 未実装 — 構想
[knowledge]
storage = "local"  # "local", "sqlite", "github"
local_path = "~/.quorum/knowledge"

[knowledge.github]
enabled = true
repo = "music-brain88/copilot-quorum"
```

---

## Context Gathering 拡張 — 段階的アプローチ

Discussion #43 Comment 3 で提案された、Knowledge Layer の **プロトタイプ** としての参照グラフ自動追跡：

```
Phase 1 (近い将来): Context Gathering 拡張 — 参照抽出 + 自動 fetch
    ↓
Phase 2: BufferType ネスティングで Sub-agent が自律的に深堀り
    ↓
Phase 3 (本構想): Knowledge Layer で設計決定・学習パターンが蓄積
    → 参照グラフを辿らなくても既に知識として持ってる状態
```

Phase 1 は `KnowledgeStore::get_related()` の **手続き的プロトタイプ** として位置づけられている。

---

## Implementation Roadmap / 実装ロードマップ（構想）

> ⚠️ 以下はすべて未実装。Discussion #43 の提案に基づく想定ロードマップです。

### Phase 1: Knowledge Layer 基盤

- `KnowledgeStore` trait 定義
- `LocalFileStore` 実装
- `HilState` を `KnowledgeEntry` として統合
- `quorum knowledge` CLI コマンド

### Phase 2: GitHub Discussions 連携

- `GitHubDiscussionStore` 実装
- HiL 発生時の自動投稿
- Discussion からの応答検出
- `quorum knowledge sync` コマンド

### Phase 3: Context Layer 強化

- `DiscussionGraph` 実装
- LLM 間関係の構造化
- 議論可視化（CLI / TUI）

### Phase 4: Workflow Layer 進化

- `WorkflowGraph` 実装（→ [workflow-layer.md](workflow-layer.md) で詳述）
- グラフベース状態遷移
- 並列 Agent 実行
- カスタムワークフロー定義

---

## Open Questions / 未解決の論点

1. **Knowledge の粒度**: どこまで自動学習する？ノイズにならない？
2. **Discussion カテゴリ**: 専用カテゴリを最初から作る？
3. **Context Graph の永続化**: セッション跨ぎで保持する？
4. **Workflow 定義**: YAML? TOML? Rust DSL?
5. **Phase 1 の優先順位**: HiL Storage から始める？Context Gathering 拡張から始める？

---

## Related

- [Discussion #43](https://github.com/music-brain88/copilot-quorum/discussions/43): RFC: Quorum v2 — Knowledge-Driven Architecture（本ドキュメントのソース）
- [Discussion #42](https://github.com/music-brain88/copilot-quorum/discussions/42): HiL Storage RFC（#43 に統合）
- [Discussion #138](https://github.com/music-brain88/copilot-quorum/discussions/138): Unified Interaction Architecture RFC
- [workflow-layer.md](workflow-layer.md): Workflow Layer 設計（3 層構想の一部）
- [extension-platform.md](extension-platform.md): Extension Platform（スクリプティング構想）
