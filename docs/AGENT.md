# Agent System / エージェントシステム

> Autonomous task execution with quorum-based safety
>
> 合議ベースの安全性を持つ自律タスク実行システム

---

## Overview / 概要

エージェントシステムは、copilot-quorum の合議（Quorum）コンセプトを自律的なタスク実行に拡張したものです。
ルーチンタスクは単一モデルで高速実行しつつ、重要な決定ポイントでは複数モデルによる合議を行うことで、
**効率性**と**安全性**を両立しています。

The agent system extends copilot-quorum's quorum concept to autonomous task execution.
It achieves both **efficiency** and **safety** by using single-model execution for routine tasks
while employing multi-model consensus at critical decision points.

---

## Design Philosophy / 設計思想

### Quorum at Critical Points / 重要ポイントでの合議

従来のエージェントシステムは単一モデルの判断に依存しますが、これには以下のリスクがあります：

Traditional agent systems rely on single-model judgment, which has these risks:

1. **ハルシネーション** - 誤った計画や危険なコマンドを生成する可能性
2. **盲点** - 単一の視点では見落としが生じやすい
3. **過信** - モデルが自身の判断を疑わない

copilot-quorum のエージェントは、**3つの重要ポイント**で合議を挟みます：

```
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│   User Request                                              │
│        │                                                    │
│        ▼                                                    │
│   ┌─────────────────┐                                       │
│   │ Context Gather  │  Single model (fast)                  │
│   └─────────────────┘                                       │
│        │                                                    │
│        ▼                                                    │
│   ┌─────────────────┐                                       │
│   │    Planning     │  Single model (creative)              │
│   └─────────────────┘                                       │
│        │                                                    │
│        ▼                                                    │
│   ╔═════════════════╗                                       │
│   ║  🗳️ QUORUM #1  ║  ← "Is this plan safe and correct?"  │
│   ║  Plan Review    ║    「この計画は安全で正しい？」       │
│   ╚═════════════════╝                                       │
│        │                                                    │
│        ▼                                                    │
│   ┌─────────────────┐                                       │
│   │ Task Execution  │                                       │
│   │   │             │                                       │
│   │   ├─ read_file ─────▶ Direct (low risk)                │
│   │   │             │                                       │
│   │   ├─ glob_search ───▶ Direct (low risk)                │
│   │   │             │                                       │
│   │   └─ write_file ────▶ ╔═════════════════╗              │
│   │                       ║  🗳️ QUORUM #2  ║              │
│   │                       ║ Action Review   ║              │
│   │                       ╚═════════════════╝              │
│   └─────────────────┘                                       │
│        │                                                    │
│        ▼                                                    │
│   ╔═════════════════╗                                       │
│   ║  🗳️ QUORUM #3  ║  ← Optional final review              │
│   ║  Final Review   ║    (require_final_review: true)      │
│   ╚═════════════════╝                                       │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Risk-Based Tool Classification / リスクベースのツール分類

ツールはリスクレベルによって分類され、高リスクツールは合議対象となります：

| Tool | Risk Level | Quorum Review |
|------|------------|---------------|
| `read_file` | Low | No |
| `glob_search` | Low | No |
| `grep_search` | Low | No |
| `write_file` | **High** | **Yes** |
| `run_command` | **High** | **Yes** |

高リスクツールは以下の特性を持ちます：
- ファイルシステムを変更する可能性がある
- 外部コマンドを実行する
- 元に戻すのが困難な操作

---

## Architecture / アーキテクチャ

### Layer Structure / レイヤー構造

エージェントシステムは既存のオニオンアーキテクチャに沿って実装されています：

```
┌─────────────────────────────────────────────────────────────┐
│ Domain Layer (quorum-domain)                                │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  agent/                        tool/                        │
│  ├── entities.rs               ├── entities.rs              │
│  │   ├── AgentState            │   ├── ToolDefinition       │
│  │   ├── AgentConfig           │   ├── ToolCall             │
│  │   ├── Plan                  │   ├── ToolSpec             │
│  │   ├── Task                  │   └── RiskLevel            │
│  │   └── AgentPhase            │                            │
│  │                             ├── value_objects.rs         │
│  └── value_objects.rs          │   ├── ToolResult           │
│      ├── AgentId               │   └── ToolError            │
│      ├── AgentContext          │                            │
│      ├── TaskResult            └── traits.rs                │
│      └── Thought                   └── ToolValidator        │
│                                                             │
│  prompt/                                                    │
│  └── agent.rs                                               │
│      └── AgentPromptTemplate                                │
│                                                             │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ Application Layer (quorum-application)                      │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ports/                        use_cases/                   │
│  └── tool_executor.rs          └── run_agent.rs             │
│      └── ToolExecutorPort          ├── RunAgentUseCase      │
│                                    ├── RunAgentInput        │
│                                    ├── RunAgentOutput       │
│                                    ├── QuorumReviewResult   │
│                                    └── AgentProgressNotifier│
│                                                             │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ Infrastructure Layer (quorum-infrastructure)                │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  tools/                                                     │
│  ├── mod.rs           (default_tool_spec, read_only_spec)   │
│  ├── executor.rs      (LocalToolExecutor)                   │
│  ├── file.rs          (read_file, write_file)               │
│  ├── command.rs       (run_command)                         │
│  └── search.rs        (glob_search, grep_search)            │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Key Interfaces / 主要インターフェース

#### ToolExecutorPort (application/ports)

```rust
#[async_trait]
pub trait ToolExecutorPort: Send + Sync {
    fn tool_spec(&self) -> &ToolSpec;
    fn has_tool(&self, name: &str) -> bool;
    fn get_tool(&self, name: &str) -> Option<&ToolDefinition>;
    fn available_tools(&self) -> Vec<&str>;
    async fn execute(&self, call: &ToolCall) -> ToolResult;
    fn execute_sync(&self, call: &ToolCall) -> ToolResult;
}
```

この設計により、ツール実行の実装を差し替え可能にしています。
例えば、リモートサーバーでツールを実行する `RemoteToolExecutor` を追加することも可能です。

#### AgentProgressNotifier (application/use_cases)

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
}
```

プレゼンテーション層でこのトレイトを実装することで、
思考過程のストリーミング表示や進捗バーを実現できます。

---

## Quorum Review Details / 合議レビューの詳細

### Plan Review / 計画レビュー

計画レビューは**常に必須**です（`require_plan_review: true` は変更不可）。

**プロセス：**
1. 全ての `quorum_models` に計画を並列送信
2. 各モデルが APPROVE / REJECT を投票
3. **過半数**が APPROVE で承認
4. 却下時は全モデルのフィードバックを集約

**プロンプト内容：**
- ユーザーの元リクエスト
- プロジェクトコンテキスト
- 提案された計画（目的、理由、タスク一覧）

### Action Review / アクションレビュー

高リスクツール（`write_file`, `run_command`）実行前に自動発動します。

**プロセス：**
1. 実行しようとしているツールコールを全モデルに送信
2. 各モデルが APPROVE / REJECT を投票
3. 却下されたアクションはスキップ（エラーではない）

**判断基準：**
- 操作は必要か？
- 引数は正しいか？
- より安全な代替手段はないか？

### Final Review / 最終レビュー

オプション機能です（`require_final_review: true` で有効化）。

**評価内容：**
- 元のリクエストが達成されたか
- 変更の品質
- 残課題や推奨事項

---

## Configuration / 設定

### AgentConfig

```rust
pub struct AgentConfig {
    /// Primary model for agent execution
    /// エージェント実行の主モデル
    pub primary_model: Model,

    /// Models for quorum voting
    /// 合議投票に使用するモデル群
    pub quorum_models: Vec<Model>,

    /// Always true (plan review is required)
    /// 常にtrue（計画レビューは必須）
    pub require_plan_review: bool,

    /// Enable final review
    /// 最終レビューを有効化
    pub require_final_review: bool,

    /// Maximum execution iterations
    /// 最大実行イテレーション数
    pub max_iterations: usize,

    /// Working directory for tools
    /// ツール実行の作業ディレクトリ
    pub working_dir: Option<String>,
}
```

### Default Configuration / デフォルト設定

```rust
AgentConfig {
    primary_model: Model::ClaudeSonnet45,
    quorum_models: Model::default_models(),  // [GPT, Claude, Gemini]
    require_plan_review: true,
    require_final_review: false,
    max_iterations: 50,
    working_dir: None,
}
```

---

## Adding New Tools / 新しいツールの追加

### 1. Define Tool (infrastructure/tools/)

```rust
// infrastructure/tools/my_tool.rs

pub const MY_TOOL: &str = "my_tool";

pub fn my_tool_definition() -> ToolDefinition {
    ToolDefinition::new(
        MY_TOOL,
        "Description of what this tool does",
        RiskLevel::Low,  // or High
    )
    .with_parameter(ToolParameter::new("param1", "Parameter description", true))
}

pub fn execute_my_tool(call: &ToolCall) -> ToolResult {
    let param1 = call.require_string("param1")?;
    // ... implementation
    ToolResult::success(MY_TOOL, "output")
}
```

### 2. Register Tool (infrastructure/tools/mod.rs)

```rust
pub fn default_tool_spec() -> ToolSpec {
    ToolSpec::new()
        .register(file::read_file_definition())
        .register(file::write_file_definition())
        // ... existing tools
        .register(my_tool::my_tool_definition())  // Add here
}
```

### 3. Add Execution (infrastructure/tools/executor.rs)

```rust
fn execute_internal(&self, call: &ToolCall) -> ToolResult {
    match call.tool_name.as_str() {
        // ... existing matches
        my_tool::MY_TOOL => my_tool::execute_my_tool(call),
        // ...
    }
}
```

---

## Future Enhancements / 今後の拡張予定

### Phase 4: Presentation & UX

- `ThoughtStream` - 思考過程のリアルタイム表示
- `AgentProgressReporter` - 進捗バーとステータス表示
- `AgentRepl` - `/agent` モード対応のREPL
- CLI `--agent` フラグ

### Potential Extensions / 将来的な拡張案

1. **Tool Chains** - 複数ツールの連携パターン
2. **Memory** - 過去の実行結果の記憶
3. **Rollback** - 変更の自動ロールバック機能
4. **Sandbox** - 隔離環境でのプレビュー実行
