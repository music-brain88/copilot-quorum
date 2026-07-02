# Interaction Model / インタラクションモデル

> Unified peer-form architecture for Agent, Ask, and Discuss interactions
>
> Agent / Ask / Discuss を対等な peer form として統一するアーキテクチャ

---

## Overview / 概要

copilot-quorum では、ユーザーとシステムの対話を **Interaction** という単一の抽象で表現します。
従来の「Agent が主で Ask / Discuss が従属」というモデルではなく、
3 つの form が **対等な peer**（Vim の `buftype` パターン）として扱われます。

```
Vim:
  Buffer(buftype="")          ← 普通のバッファ
  Buffer(buftype="help")      ← ヘルプバッファ
  Buffer(buftype="terminal")  ← ターミナルバッファ
  → 全て Buffer の type。「普通のバッファ」が他の親ではない。

copilot-quorum:
  Interaction(form=Agent)    ← 自律実行
  Interaction(form=Ask)      ← 問い合わせ
  Interaction(form=Discuss)  ← 合議
  → 全て Interaction の form。Agent が他の親ではない。
```

> **設計背景**: Discussion [#138](https://github.com/music-brain88/copilot-quorum/discussions/138) で提案された
> "Unified Interaction Architecture" に基づく。

---

## InteractionForm — 3 つの対等な form

`InteractionForm` は対話の形式を表す enum で、各 form は独立した peer です。

```rust
// domain/src/interaction/mod.rs
enum InteractionForm {
    Agent,    // 自律タスク実行（計画 → ツール使用 → レビュー）
    Ask,      // 単一質問 → 回答（read-only ツール）
    Discuss,  // 多モデル議論 / Quorum council
}
```

### 各 form の特性

| 特性 | Agent | Ask | Discuss |
|------|-------|-----|---------|
| **説明** | 自律タスク実行 | 単一 Q&A | 多モデル議論 |
| **ツール** | 全て（risk-based review） | read-only | なし |
| **SessionMode** | ✓ | ✓（モデル選択のみ） | ✓ |
| **AgentPolicy** | ✓ | — | — |
| **ExecutionParams** | ✓ | ✓（`max_tool_turns`） | — |
| **デフォルト ContextMode** | `Full` | `Projected` | `Full` |

### Config 必要性の判定メソッド

各 form がどの設定型を使うかは、`InteractionForm` のメソッドで判定できます：

```rust
form.uses_session_mode()     // → true (全 form)
form.uses_agent_policy()     // → true (Agent のみ)
form.uses_execution_params() // → true (Agent, Ask)
```

### FromStr / Display

```rust
"agent".parse::<InteractionForm>()    // → Agent
"ask".parse::<InteractionForm>()      // → Ask
"discuss".parse::<InteractionForm>()  // → Discuss
"council".parse::<InteractionForm>()  // → Discuss（エイリアス）
```

---

## ContextMode — コンテキスト伝播量の制御

`ContextMode` は「親から子へどれだけコンテキストを渡すか」を制御する cross-cutting な概念です。
Agent Plan 内の Task レベルだけでなく、Interaction レベルでも同じセマンティクスで使用されます。

```rust
// domain/src/context/context_mode.rs
enum ContextMode {
    Full,       // 全コンテキストを渡す
    Projected,  // 要約・関連部分のみ渡す
    Fresh,      // コンテキストを引き継がず新しく始める
}
```

### Vim アナロジー

| ContextMode | Vim コマンド | 意味 |
|-------------|-------------|------|
| `Full` | `:split` | 同じバッファを共有（全プロジェクトコンテキスト） |
| `Projected` | `:edit` | 特定のファイルを開く（フォーカスされた context brief） |
| `Fresh` | `:enew` | 空のバッファで始める（コンテキスト継承なし） |

### デフォルト ContextMode マッピング

各 `InteractionForm` にはデフォルトの `ContextMode` が設定されます：

```rust
InteractionForm::Agent.default_context_mode()   // → ContextMode::Full
InteractionForm::Ask.default_context_mode()      // → ContextMode::Projected
InteractionForm::Discuss.default_context_mode()  // → ContextMode::Full
```

- **Agent → Full**: 計画立案にはプロジェクト全体の理解が必要
- **Ask → Projected**: 焦点を絞った質問には、焦点を絞ったコンテキストが適切
- **Discuss → Full**: 合議にはプロジェクト全体の俯瞰が必要

デフォルトは `with_context_mode()` で上書きできます。

---

## Interaction — 対話のインスタンス

`Interaction` は単一の対話インスタンスで、form・コンテキストモード・ネスティング情報を持ちます。

```rust
// domain/src/interaction/mod.rs
struct Interaction {
    id: InteractionId,
    form: InteractionForm,
    context_mode: ContextMode,
    parent: Option<InteractionId>,  // ネスト親
    depth: usize,                   // ネスト深度（0 = root）
}
```

### 生成パターン

```rust
// ルートレベル（深度 0、親なし）
let root = Interaction::root(InteractionId(1), InteractionForm::Agent);

// 子（親の深度 + 1）
let child = Interaction::child(InteractionId(2), InteractionForm::Ask, &root);

// ContextMode を上書き
let custom = Interaction::root(InteractionId(3), InteractionForm::Ask)
    .with_context_mode(ContextMode::Full);
```

---

## InteractionTree — 再帰的ネスティング

Interaction は再帰的にネストできます。Agent タスク中に Ask で明確化したり、
Discuss で多モデルの意見を集約したりする、**自然な思考の流れ** を表現します。

```
Ask("バグの原因は？")
└─ Agent(調査実行)             ← 聞いたら調査が必要だった
   └─ Discuss(設計判断)        ← 調査中に合議が必要に

Agent("認証システム実装")
└─ Discuss(設計合議)           ← 実装中に合議が必要に
   └─ Agent(PoC 調査)         ← 合議中に実証が必要に
```

### InteractionTree

`InteractionTree` は HashMap ベースのツリー構造で、自動 ID 採番とネスティング管理を行います。

```rust
let mut tree = InteractionTree::default();

// ルート Interaction を作成
let root_id = tree.create_root(InteractionForm::Agent);

// 子 Interaction を spawn
let child_id = tree.spawn_child(root_id, InteractionForm::Ask)?;

// ContextMode を指定して spawn
let discuss_id = tree.spawn_child_with_context(
    root_id,
    InteractionForm::Discuss,
    ContextMode::Fresh,
)?;

// ツリー操作
tree.get(child_id);              // → Option<&Interaction>
tree.parent_of(child_id);       // → Some(root_id)
tree.children_of(root_id);      // → &[child_id, discuss_id]
```

### 深度制限: `DEFAULT_MAX_NESTING_DEPTH = 3`

無制限な再帰を防ぐため、ネスティング深度は最大 3 に制限されます。
深度 0 がルートなので、合計 4 レベルまでのネストが可能です。

```
depth 0: root (Agent)       ← can_spawn() == true
depth 1: child (Ask)        ← can_spawn() == true
depth 2: grandchild (Discuss) ← can_spawn() == true
depth 3: great-grandchild   ← can_spawn() == false
```

制限超過時は `SpawnError::MaxDepthExceeded` が返ります。

---

## InteractionResult — 完了結果の伝播

子 Interaction の結果は `InteractionResult` として親のコンテキストに返ります。
`to_context_injection()` メソッドで、親に注入可能なテキスト形式に変換されます。

```rust
enum InteractionResult {
    AskResult { answer: String },
    DiscussResult { synthesis: String, participant_count: usize },
    AgentResult { summary: String, success: bool },
}
```

### コンテキスト注入の例

```rust
let result = InteractionResult::DiscussResult {
    synthesis: "Consensus reached on approach A.".into(),
    participant_count: 3,
};
result.to_context_injection()
// → "[Discuss Result (3 models)]: Consensus reached on approach A."

let result = InteractionResult::AgentResult {
    summary: "README updated successfully.".into(),
    success: true,
};
result.to_context_injection()
// → "[Agent Result (completed)]: README updated successfully."
```

---

## TUI 統合: Tab / Pane モデル

Presentation 層では、`Interaction` は `PaneKind::Interaction` として Tab / Pane モデルに統合されます。
Vim の Buffer / Window / Tab Page の 3 層に対応します。

```
Vim               copilot-quorum
─────────────────────────────────
Buffer          → Interaction (domain)
Window          → Pane (presentation)
Tab Page        → Tab (presentation)
```

### PaneKind

```rust
// presentation/src/tui/tab.rs
enum PaneKind {
    Interaction(InteractionForm, Option<InteractionId>),
}
```

- `Option<InteractionId>` は placeholder パターンを実現：
  - `None` — タブ作成直後（Interaction 生成前）
  - `Some(id)` — `bind_interaction_id()` で後から紐付け

### TabManager の操作

```
:tabnew ask      → create_tab(PaneKind::Interaction(Ask, None))
:tabnew discuss  → create_tab(PaneKind::Interaction(Discuss, None))
gt               → next_tab()
gT               → prev_tab()
:tabclose        → close_active()
:tabs            → tab_list_summary()
```

---

## Spawn メカニズム — 段階的アプローチ

子 Interaction の生成方法は段階的に拡張される設計です。

### Phase A: ユーザー起動（実装済み）

ユーザーが TUI コマンドで明示的に子 Interaction を生成します。

```
TUI Normal モード:
  a  → Ask タブにフォーカス
  d  → Discuss タブにフォーカス

TUI Command モード:
  :tabnew ask      → 新しい Ask タブを作成
  :tabnew discuss  → 新しい Discuss タブを作成
  :tabnew agent    → 新しい Agent タブを作成
```

最も安全で予測可能なアプローチ。TabManager と PaneKind の組み合わせで実現されています。

### Phase B: ツールベース spawn（計画中）

`spawn_ask`, `spawn_discuss`, `spawn_agent` を **ツールとして定義** し、
LLM が Native Tool Use パイプライン経由で子 Interaction を生成する構想です。

```rust
// 構想段階 — 未実装
ToolDefinition::new(
    "spawn_ask",
    "Spawn a child Ask interaction for a sub-question",
    RiskLevel::High,  // → HiL レビューを通る
)
```

既存の ToolSpec 登録 / RiskLevel / HiL レビュー / Native Tool Use ループがそのまま使えるため、
新しいインフラを追加せずに実現可能です。

### Phase C: ポリシーベース自動 spawn（将来構想）

`AgentPolicy` に spawn ルールを追加し、条件に基づいて自動的に子 Interaction を生成する構想です。

```
推奨順序:
Phase A (実装済み) → Phase B (計画中) → Phase C (将来)
  ユーザー起動        ツールベース       ポリシー自動化
  低リスク            中リスク           高リスク
```

---

## Source Files / ソースファイル

| File | Description |
|------|-------------|
| `domain/src/interaction/mod.rs` | InteractionForm, Interaction, InteractionTree, InteractionResult |
| `domain/src/context/context_mode.rs` | ContextMode (Full / Projected / Fresh) |
| `presentation/src/tui/tab.rs` | PaneKind, Pane, Tab, TabManager |
| `presentation/src/tui/state.rs` | TUI state integration |
| `presentation/src/tui/event.rs` | InteractionForm in event routing |

<!-- LLM Context: InteractionForm は Agent / Ask / Discuss の3つの対等な peer form。ContextMode (Full / Projected / Fresh) はコンテキスト伝播量を制御する cross-cutting 概念で、Vim のバッファコマンドにアナロジー。InteractionTree は HashMap ベースのツリー構造で再帰ネスティングを管理、DEFAULT_MAX_NESTING_DEPTH = 3。InteractionResult の to_context_injection() で子の結果を親に注入。TUI では PaneKind::Interaction として Tab/Pane モデルに統合。Spawn は Phase A（ユーザー起動）が実装済み、Phase B（ツールベース）/ Phase C（ポリシー自動化）は計画中。主要ファイルは domain/src/interaction/mod.rs。 -->
