# Native Tool Use API / ネイティブツール呼び出し API

> Structured tool calling via LLM provider APIs, eliminating text parsing and tool name hallucinations
>
> LLM プロバイダー API による構造化ツール呼び出し — テキストパースとツール名ハルシネーションの排除

---

## Overview / 概要

Native Tool Use API は、LLM にツール定義を **API パラメータとして構造化データで渡し**、
ツール呼び出しを **構造化レスポンスから直接抽出する** 仕組みです。

| 特徴 | 説明 |
|------|------|
| ツール名の正確性 | API がツール名を強制 → ハルシネーション**不発生** |
| 構造化データ | テキストパース不要 → `ContentBlock::ToolUse` から直接抽出 |
| トークン効率 | ツール定義は API パラメータ → プロンプト埋め込み不要 |
| マルチターン対話 | ループで継続的にツール実行（`send_with_tools` → 実行 → `send_tool_results` → ...） |

Native Tool Use API は、Anthropic の `tool_use`、OpenAI の `function_calling` に対応した
業界標準のツール呼び出しプロトコルです。copilot-quorum はこの両方を同一の `LlmSession` trait で
抽象化し、プロバイダーに依存しないツール呼び出しを実現します。

---

## Quick Start / クイックスタート

Native Tool Use の型とマルチターンループの枠組みは整備済みです。
各プロバイダーの `send_with_tools()` / `send_tool_results()` 実装は今後対応予定で、
現時点では `send()` → `LlmResponse::from_text()` によるフォールバック経路で動作します。

```bash
# 現在はフォールバック経路で動作
copilot-quorum --provider anthropic "List all Rust files"
copilot-quorum "Fix the failing test"
```

---

## How It Works / 仕組み

### Architecture / アーキテクチャ

copilot-quorum は **Native Tool Use API** を通じてツール呼び出しを行います。

```
LlmSession
    │
    └── Native Tool Use API
        │
        ├── ツール定義 → API パラメータとして JSON Schema で送信
        ├── LLM レスポンス → ContentBlock::ToolUse から直接抽出
        ├── ツール名の正確性 → API がツール名を保証
        └── マルチターンループ → ToolUse stop → 実行 → 結果送信 → ...
```

### 各プロバイダーの対応状況

| Provider | 実装方式 | Status |
|----------|---------|--------|
| **Copilot CLI** | JSON-RPC `session.create` に `tools` パラメータ追加 | 将来 |
| **Anthropic API** | Messages API の `tools` パラメータ | 将来 |
| **OpenAI API** | Chat Completions の `function_calling` | 将来 |

> **Note**: 各プロバイダーの Native 実装が未対応の間、`LlmSession` のデフォルト実装
> （`send()` → `LlmResponse::from_text()`）がフォールバックとして機能します。
> マルチターンループの枠組み（型、ツール定義の JSON Schema 変換、並列実行）は既に完成しています。

---

## Multi-Turn Tool Use Loop / マルチターンツール呼び出しループ

Native Tool Use の最大の利点は **マルチターン対話** です。
LLM がツールを呼び出す → 結果を受け取る → さらにツールを呼ぶ...というループが
API レベルでサポートされます。

### Loop Flow / ループフロー

```
                    ┌──────────────────────────────────────┐
                    │    send_with_tools(prompt, tools)     │
                    └──────────────┬───────────────────────┘
                                   │
                                   ▼
                          ┌─────────────────┐
                          │  LlmResponse    │
                          │  stop_reason?   │
                          └────────┬────────┘
                                   │
                    ┌──────────────┼──────────────┐
                    │              │              │
                    ▼              ▼              ▼
              EndTurn        ToolUse        MaxTokens
              (完了)        (ツール実行)     (トークン上限)
                │              │              │
                ▼              ▼              ▼
             return      ┌──────────┐      return
             text        │ execute  │      text
                         │ tools    │
                         └────┬─────┘
                              │
                    ┌─────────┴─────────┐
                    │ Low-risk (並列)   │ High-risk (順次)
                    │ read_file ─┐      │ write_file → Quorum Review → execute
                    │ grep ──────┤      │ run_command → Quorum Review → execute
                    │ glob ──────┘      │
                    └─────────┬─────────┘
                              │
                              ▼
                    ┌──────────────────┐
                    │ send_tool_results│
                    │ (results)        │  ← ToolResultMessage で結果返送
                    └────────┬─────────┘
                             │
                             ▼
                     turn_count < max?  ───no──→ return (上限到達)
                             │
                            yes
                             │
                             ▼
                      次の LlmResponse へ
                      (ループ先頭に戻る)
```

### Turn Limit / ターン数制限

```toml
# quorum.toml
[agent]
max_tool_turns = 10    # デフォルト: 10
```

| 設定 | 型 | デフォルト | 説明 |
|------|----|-----------|------|
| `max_tool_turns` | `usize` | `10` | 1 タスク内の最大ツール呼び出しターン数 |

各ターンは「LLM レスポンス受信 → ツール実行 → 結果送信」の 1 サイクルです。
1 ターンで複数のツールが呼び出される場合もあり、それらは 1 ターンとしてカウントされます。

### Parallel Execution / 並列実行

1 ターン内で複数のツール呼び出しがある場合、**リスクレベルに応じて実行戦略が異なります**：

| Risk Level | 実行方式 | 理由 |
|-----------|---------|------|
| **Low** (read_file, grep, glob 等) | `futures::join_all()` で**並列**実行 | 読み取り専用 → 副作用なし、高速化可能 |
| **High** (write_file, run_command) | **順次**実行 + Quorum Review | 書き込み操作 → 順序重要、レビュー必要 |

```
Turn 1 の LLM レスポンス:
  ToolUse: read_file("/src/main.rs")     ← Low risk
  ToolUse: grep_search("TODO", "/src")   ← Low risk
  ToolUse: write_file("/out.txt", "...")  ← High risk

実行順序:
  1. read_file + grep_search → 並列実行 (futures::join_all)
  2. write_file → Quorum Review → 順次実行
  3. 全結果を send_tool_results() で返送
```

---

## Key Types / 主要な型

### Domain Layer / ドメイン層

#### `LlmResponse` — 構造化レスポンス

```rust
pub struct LlmResponse {
    pub content: Vec<ContentBlock>,        // テキスト + ツール呼び出しブロック
    pub stop_reason: Option<StopReason>,   // 停止理由
    pub model: Option<String>,             // モデル識別子
}

impl LlmResponse {
    fn from_text(text) -> Self;     // テキストのみ（フォールバック用）
    fn text_content() -> String;    // テキストブロックを結合
    fn tool_calls() -> Vec<ToolCall>;  // ToolUse ブロックを ToolCall に変換
    fn has_tool_calls() -> bool;    // ツール呼び出しの有無
}
```

#### `ContentBlock` — レスポンス内の個別ブロック

```rust
pub enum ContentBlock {
    Text(String),
    ToolUse {
        id: String,      // API 割当 ID ("toolu_abc123")
        name: String,    // 正規ツール名（API が保証）
        input: HashMap<String, serde_json::Value>,
    },
}
```

#### `StopReason` — 停止理由

```rust
pub enum StopReason {
    EndTurn,        // 自然終了 → ループ終了
    ToolUse,        // ツール呼び出し待ち → 結果を返して続行
    MaxTokens,      // トークン上限 → ループ終了
    Other(String),  // プロバイダー固有
}
```

#### `ToolCall::native_id` — API 割当 ID

```rust
pub struct ToolCall {
    pub tool_name: String,
    pub arguments: HashMap<String, serde_json::Value>,
    pub reasoning: Option<String>,
    pub native_id: Option<String>,  // API 割当 ID（結果返送時に使用）
}
```

`native_id` は Native Tool Use レスポンスの `ContentBlock::ToolUse` の `id` に対応します。
`send_tool_results()` 時に `ToolResultMessage::tool_use_id` として使用し、
リクエストと結果の対応付けを行います。

#### `ToolSchemaPort` — JSON Schema 変換 (Port パターン)

JSON Schema 変換は `ToolSchemaPort` trait として application 層に定義され、
infrastructure 層の `JsonSchemaToolConverter` が実装します。
domain 層はツールのフィルタリング（`low_risk_tools()`, `high_risk_tools()`）のみを担当し、
API フォーマットの関心事から分離されています。

```rust
// application/src/ports/tool_schema.rs
pub trait ToolSchemaPort: Send + Sync {
    fn tool_to_schema(&self, tool: &ToolDefinition) -> serde_json::Value;
    fn all_tools_schema(&self, spec: &ToolSpec) -> Vec<serde_json::Value>;
    fn low_risk_tools_schema(&self, spec: &ToolSpec) -> Vec<serde_json::Value>;
}

// infrastructure/src/tools/schema.rs
pub struct JsonSchemaToolConverter;
impl ToolSchemaPort for JsonSchemaToolConverter { ... }
```

`param_type` → JSON Schema 変換:

| `param_type` | JSON Schema `type` |
|:----------:|:------------------:|
| `"string"`, `"path"` | `"string"` |
| `"number"` | `"number"` |
| `"integer"` | `"integer"` |
| `"boolean"` | `"boolean"` |

### Application Layer / アプリケーション層

#### `ToolResultMessage` — ツール実行結果

```rust
pub struct ToolResultMessage {
    pub tool_use_id: String,   // ContentBlock::ToolUse の id
    pub tool_name: String,     // ツール名（ログ用）
    pub output: String,        // 実行結果 or エラーメッセージ
    pub is_error: bool,        // エラーかどうか
    pub is_rejected: bool,     // HiL/action review で拒否されたか
}
```

#### `LlmSession` trait

```rust
#[async_trait]
pub trait LlmSession: Send + Sync {
    // === 基本メソッド ===
    fn model(&self) -> &Model;
    async fn send(&self, content: &str) -> Result<String, GatewayError>;
    async fn send_streaming(&self, content: &str) -> Result<StreamHandle, GatewayError>;

    // === Native Tool Use メソッド ===
    async fn send_with_tools(
        &self, content: &str, tools: &[serde_json::Value],
    ) -> Result<LlmResponse, GatewayError>;

    async fn send_tool_results(
        &self, results: &[ToolResultMessage],
    ) -> Result<LlmResponse, GatewayError>;
}
```

---

## Streaming Support / ストリーミング対応

`StreamEvent` に Native Tool Use 用のバリアントが追加されています：

```rust
pub enum StreamEvent {
    Delta(String),          // テキストチャンク（既存）
    Completed(String),      // テキスト完了（既存）
    Error(String),          // エラー（既存）

    // Native Tool Use 用
    ToolCallDelta {         // ツール呼び出しの増分データ
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: Option<String>,
    },
    CompletedResponse(LlmResponse),  // 構造化レスポンス完了
}
```

`ToolCallDelta` は、ストリーミング中にツール呼び出しの各フィールドが
増分的に到着する場合に使用されます（Anthropic SSE 等）。

---

## Configuration / 設定

```toml
# quorum.toml
[agent]
max_tool_turns = 10    # Native ループの最大ターン数（デフォルト: 10）
```

```rust
// プログラム的に設定
let execution = ExecutionParams {
    max_tool_turns: 15,
    ..Default::default()
};
```

---

## Architecture / アーキテクチャ

### Key Files / 主要ファイル

| File | Description |
|------|-------------|
| `domain/src/session/response.rs` | `LlmResponse`, `ContentBlock`, `StopReason` |
| `domain/src/session/stream.rs` | `StreamEvent`（`ToolCallDelta`, `CompletedResponse` バリアント） |
| `domain/src/tool/entities.rs` | `ToolCall::native_id`, `ToolSpec::low_risk_tools()`, `ToolSpec::all()` |
| `application/src/ports/tool_schema.rs` | `ToolSchemaPort` trait — JSON Schema 変換ポート |
| `infrastructure/src/tools/schema.rs` | `JsonSchemaToolConverter` — JSON Schema 変換実装 |
| `domain/src/prompt/agent.rs` | `agent_system()` — エージェントシステムプロンプト生成 |
| `application/src/config/execution_params.rs` | `ExecutionParams::max_tool_turns` |
| `application/src/ports/llm_gateway.rs` | `ToolResultMessage`, `LlmSession` trait |
| `application/src/use_cases/run_agent.rs` | `execute_task_native()`, `send_with_tools_cancellable()` |

### Data Flow / データフロー

```
RunAgentUseCase::execute_single_task()
    │
    ▼
execute_task_native()
    │
    ├── tool_schema.all_tools_schema(tool_spec) → JSON Schema 配列
    │
    ├── session.send_with_tools(prompt, tools) → LlmResponse
    │
    ├── response.tool_calls() → Vec<ToolCall> (直接抽出)
    │
    ├── Low-risk → futures::join_all() 並列実行
    │
    ├── High-risk → Quorum Review → 順次実行
    │
    ├── session.send_tool_results(results) → 次の LlmResponse
    │
    └── turn_count < max_tool_turns? → ループ
```

---

## Related Features / 関連機能

- [Tool System](./tool-system.md) - ツールの定義、リスク分類、プロバイダーアーキテクチャ
- [Agent System](./agent-system.md) - エージェントライフサイクルと Quorum Review
- [Ensemble Mode](../concepts/ensemble-mode.md) - マルチモデル計画生成
- [CLI & Configuration](../guides/cli-and-configuration.md) - 設定オプション

<!-- LLM Context: Native Tool Use API は LLM プロバイダーの構造化ツール呼び出し。LlmSession の send_with_tools() で API パラメータとしてツール定義を送信し、LlmResponse の ContentBlock::ToolUse から直接 ToolCall を抽出（テキストパース不要）。マルチターンループで StopReason::ToolUse の間ツール実行を繰り返す。Low-risk ツールは futures::join_all() で並列実行、High-risk は Quorum Review 後に順次実行。max_tool_turns（デフォルト10）でループ制限。主要ファイルは domain/src/session/response.rs、application/src/ports/llm_gateway.rs、application/src/use_cases/run_agent.rs。 -->
