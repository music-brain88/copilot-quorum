# Workflow Layer — Graph-Based Task Execution / DAG ベース並列タスク実行

> 🔴 **Status**: Not implemented — Design phase (Draft)
>
> Based on [Discussion #157](https://github.com/music-brain88/copilot-quorum/discussions/157)

---

## Overview / 概要

現在の線形タスク実行（`Plan::next_task()` → 1 タスクずつ順番に実行）を
**DAG ベースの並列ディスパッチ** に進化させる Workflow Layer の設計案。

既存の `Task::depends_on` フィールドと `Task::is_ready()` メソッドを拡張して、
新しい大きな概念を導入するのではなく、**最小変更で並列化を実現** します。

> **Note**: これは将来ビジョンであり、現時点では未実装です。

---

## Motivation / 動機

### 現在の実行モデル

```
Plan::next_task() → 1タスク取得 → 実行 → 完了 → next_task() → ...（直列ループ）
```

- `Plan::next_task()` は **1 つしか返さない**
- `ExecuteTaskUseCase::execute()` は sequential ループ
- 29 タスクのプランでも 1 個ずつ実行 → 時間がかかる

### 既に揃っている部品

| Component | Location | Status |
|-----------|----------|--------|
| `Task::depends_on: Vec<TaskId>` | `domain/src/agent/entities.rs` | **Implemented** |
| `Task::is_ready(&resolved)` | `domain/src/agent/entities.rs` | **Implemented** |
| `create_plan` スキーマに `depends_on` | `domain/src/prompt/agent.rs` | **LLM に公開済み** |
| Transport Demux（並列セッション） | `infrastructure/src/copilot/router.rs` | **Implemented** |
| `futures::join_all` パターン | `application/src/use_cases/execute_task.rs` | **低リスクツールで使用済み** |

**結論**: 新しい大きな概念を導入するのではなく、既存の `depends_on` + `is_ready()` を拡張して並列実行を実現する。

---

## Architecture Design / アーキテクチャ設計

### Layer 配置

```
domain/
  └─ workflow/
       ├─ graph.rs          # WorkflowGraph 構造体 + DAG メソッド
       └─ mod.rs

application/
  └─ use_cases/
       └─ workflow_executor.rs  # 並列ディスパッチ + スケジューリング
```

**設計原則**: 構造と不変条件（DAG 検証、トポロジカルソート）は domain、
実行制御（並列ディスパッチ、セッション管理）は application。

---

## Domain Layer: WorkflowGraph / ドメイン層設計案

### 構造体

```rust
// ⚠️ 未実装 — 設計案
// domain/src/workflow/graph.rs

/// DAG-based workflow representation built from a Plan
pub struct WorkflowGraph {
    /// task → tasks that depend on it (forward edges)
    dependents: HashMap<TaskId, Vec<TaskId>>,
    /// task → tasks it depends on (reverse edges)
    dependencies: HashMap<TaskId, Vec<TaskId>>,
    /// All task IDs in the graph
    task_ids: Vec<TaskId>,
}
```

### ドメインメソッド

```rust
// ⚠️ 未実装 — 設計案
impl WorkflowGraph {
    /// Plan から WorkflowGraph を構築
    pub fn from_plan(plan: &Plan) -> Result<Self, WorkflowError>;

    /// 実行可能なタスク（依存が全て解決済み + Pending）を全て返す
    pub fn ready_tasks<'a>(&self, plan: &'a Plan) -> Vec<&'a Task>;

    /// DAG にサイクルがないか検証
    pub fn validate(&self) -> Result<(), WorkflowError>;

    /// デッドロック検出（ready なタスクがないが未完了タスクがある）
    pub fn is_deadlocked(&self, plan: &Plan) -> bool;

    /// トポロジカルソートされたレベル（並列実行グループ）を返す
    pub fn execution_levels(&self) -> Vec<Vec<TaskId>>;

    /// タスク完了時に新たに ready になるタスクを返す
    pub fn unblocked_by(&self, completed: &TaskId, plan: &Plan) -> Vec<TaskId>;

    /// クリティカルパス（最長依存チェーン）を算出
    pub fn critical_path(&self) -> Vec<TaskId>;
}
```

### Plan への追加メソッド案

```rust
// ⚠️ 未実装 — 設計案
impl Plan {
    /// 実行可能なタスクを全て返す（next_task の複数版）
    pub fn next_ready_tasks(&self) -> Vec<&Task>;

    /// 全タスクが完了または失敗か
    pub fn is_complete(&self) -> bool;
}
```

---

## Application Layer: WorkflowExecutor / アプリケーション層設計案

### 実行ループの概略

```rust
// ⚠️ 未実装 — 設計案
impl WorkflowExecutor {
    pub async fn execute(&self, plan: &mut Plan, graph: &WorkflowGraph) -> Result<WorkflowResult> {
        graph.validate()?;

        loop {
            let ready = plan.next_ready_tasks();
            if ready.is_empty() {
                if plan.is_complete() { break; }
                return Err(AgentError::WorkflowDeadlock);
            }

            // 並列ディスパッチ
            let futures = ready.iter().map(|task| self.execute_single_task(task));
            let results = futures::future::join_all(futures).await;

            // 結果を Plan に反映
            for (task_id, result) in results { /* ... */ }
        }
        Ok(WorkflowResult { plan: plan.clone() })
    }
}
```

### Quorum Review の設計案

```
Ready: [Task A, Task B, Task C]  (独立)
  │
  ├─ Task A → execute → ⚠ high-risk tool → Quorum Review (個別)
  │                                           ├─ Approve → continue
  │                                           └─ Reject → mark failed
  │
  ├─ Task B → execute → low-risk only → complete (レビュー不要)
  │
  └─ Task C → execute → ⚠ high-risk tool → Quorum Review (個別)
```

既存の `ActionReviewer`（ツールレベル Quorum Review）はそのまま活用。
WorkflowExecutor は「タスク単位」の並列制御のみ担当し、
ツールレベルのレビューは各タスク内で従来通り動く。

---

## Configuration / 設定案

```toml
# ⚠️ 未実装 — 構想
[workflow]
review_mode = "per_task"     # "per_task" | "per_batch" | "none"
max_parallel_tasks = 4       # 並列実行の最大同時タスク数
on_task_failure = "continue" # "continue" | "abort_group" | "abort_all"
```

---

## Phased Roadmap / 段階的ロードマップ（構想）

> ⚠️ 以下はすべて未実装。Discussion #157 の提案に基づく想定ロードマップです。

### Phase 1: Domain 基盤 + Plan 拡張

**Goal**: WorkflowGraph を domain に追加、Plan に並列対応メソッドを追加

- `domain/src/workflow/graph.rs` — WorkflowGraph 構造体
- `from_plan()`, `validate()`, `ready_tasks()`, `is_deadlocked()`
- `execution_levels()` — トポロジカルソートのレベル分け
- `Plan::next_ready_tasks()`, `Plan::is_complete()`
- ユニットテスト: サイクル検出、ready tasks 抽出、レベル分け

**Impact**: domain のみ（既存コード変更なし、追加のみ）

### Phase 2: WorkflowExecutor + 並列ディスパッチ

**Goal**: application 層に WorkflowExecutor を追加、タスク実行を並列化

- `application/src/use_cases/workflow_executor.rs`
- `join_all` ベースの並列タスクディスパッチ
- `max_parallel_tasks` 制御（セマフォ）
- `RunAgentUseCase` から WorkflowExecutor への接続

**Impact**: application（execute_task.rs のループを WorkflowExecutor に委譲）

### Phase 3: 設定 + TUI 可視化

**Goal**: 設定ファイルでの制御、TUI でのワークフロー進捗表示

- `quorum.toml` に `[workflow]` セクション追加
- TUI に DAG ベースの進捗表示（どのタスクが並列実行中か可視化）

**Impact**: infrastructure (config), presentation (TUI)

### Phase 4: 高度なフロー制御

**Goal**: 条件分岐、動的タスク追加

- 条件付きエッジ（タスク結果に基づく分岐）
- 動的タスク追加（実行中に LLM が追加タスクを提案）
- `on_task_failure` ポリシー
- クリティカルパス表示

### Phase 5: Knowledge Layer 統合 (Discussion #43)

**Goal**: ワークフロー実行結果の知識化

- `WorkflowResult` → `KnowledgeEntry` への変換
- 過去のワークフローパターンの学習・再利用

---

## Impact Map / 影響マップ（想定）

| File | Phase | Change |
|------|-------|--------|
| `domain/src/workflow/` (NEW) | 1 | WorkflowGraph, WorkflowError |
| `domain/src/agent/entities.rs` | 1 | `Plan::next_ready_tasks()`, `Plan::is_complete()` |
| `application/src/use_cases/workflow_executor.rs` (NEW) | 2 | WorkflowExecutor |
| `application/src/use_cases/execute_task.rs` | 2 | ループを WorkflowExecutor に委譲 |
| `application/src/use_cases/run_agent/mod.rs` | 2 | WorkflowExecutor の呼び出し |
| `application/src/config/execution_params.rs` | 2-3 | `max_parallel_tasks`, `review_mode` |
| `infrastructure/src/config/file_config.rs` | 3 | `[workflow]` セクション |
| `presentation/src/tui/` | 3 | ワークフロー進捗表示 |

---

## Open Questions / 未解決の論点

1. **`max_parallel_tasks` のデフォルト値** — Copilot CLI のレート制限次第。4? 8?
2. **失敗タスクの依存先** — Failed も解決済み扱い？並列実行でもこのセマンティクスを維持する？
3. **Ensemble × Workflow** — Ensemble モードで各モデルが別々の WorkflowGraph を提案した場合の扱い
4. **タスク間の結果参照** — `context_brief` 経由の参照で十分か？より構造化された参照が必要か？

---

## Related

- [Discussion #157](https://github.com/music-brain88/copilot-quorum/discussions/157): RFC: Workflow Layer（本ドキュメントのソース）
- [Discussion #43](https://github.com/music-brain88/copilot-quorum/discussions/43): Knowledge-Driven Architecture — 3 層構想
- [knowledge-architecture.md](knowledge-architecture.md): Knowledge Layer 設計
- [extension-platform.md](extension-platform.md): Extension Platform 構想
- `domain/src/agent/entities.rs`: 既存の `Task::depends_on`, `Task::is_ready()`
- `infrastructure/src/copilot/router.rs`: Transport Demux（並列セッション基盤）
