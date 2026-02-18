# Native Tool Use API / ネイティブツール呼び出し API

> Structured tool calling via LLM provider APIs — the sole tool execution path
>
> LLM プロバイダー API による構造化ツール呼び出し — 唯一のツール実行パス

---

## Overview / 概要

Native Tool Use API は、LLM にツール定義を **API パラメータとして構造化データで渡し**、
ツール呼び出しを **構造化レスポンスから直接抽出する** 仕組みです。
copilot-quorum の**唯一のツール実行パス**として機能します。

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

## How It Works / 仕組み

### Architecture / アーキテクチャ

copilot-quorum は **Native Tool Use API** を唯一のツール呼び出しパスとして使用します。

```
LlmSession
    │
    └── Native Tool Use API (唯一のパス)
        │
        ├── ToolSchemaPort  → ツール定義を JSON Schema に変換
        ├── send_with_tools → API パラメータとして JSON Schema で送信
        ├── ContentBlock::ToolUse → レスポンスから直接抽出
        ├── execute tools   → Low-risk 並列 / High-risk 順次+Review
        └── send_tool_results → ToolResultMessage で結果返送
```

### CopilotSession 実装

Copilot CLI プロバイダーでの Native Tool Use 実装は、内部的に**別セッション**を作成する仕組みです：

```
CopilotSession
├── session_id: "main-session"       ← テキスト Q&A 用
├── channel: SessionChannel          ← メインセッションの受信チャネル
└── tool_session: Option<ToolSessionState>
    ├── session_id: "tool-session"   ← ツール対話専用（tools パラメータ付き）
    ├── channel: SessionChannel      ← ツールセッションの専用チャネル
    └── pending_tool_call: Option<PendingToolCall>
        └── request_id: u64         ← JSON-RPC リクエスト ID（結果返送時に使用）
```

**ライフサイクル**：

1. `send_with_tools()` → `create_tool_session_and_send()` が呼ばれる
2. `router.create_session(params_with_tools)` で tools 付きの新セッションを作成
3. `router.request(session.send)` でプロンプトを送信
4. `tool_channel.read_streaming_for_tools()` でレスポンスを読み取り
   - `StreamingOutcome::Idle` → テキストレスポンス（`StopReason::EndTurn`）
   - `StreamingOutcome::ToolCall` → ToolSessionState を保存、`StopReason::ToolUse` を返却
5. `send_tool_results()` → 保存された `pending_tool_call` の `request_id` で JSON-RPC レスポンスを返送
6. 次の `read_streaming_for_tools()` で再びツール呼び出しまたは終了を待機

定義ファイル: `infrastructure/src/copilot/session.rs`

---

## ToolSchemaPort / JSON Schema 変換 (Port パターン)

JSON Schema 変換は domain 層のツールフィルタリングから分離されています。

```
Domain Layer                  Application Layer               Infrastructure Layer
─────────────                 ──────────────────               ────────────────────
ToolSpec                      ToolSchemaPort (trait)           JsonSchemaToolConverter
├── all()                     ├── tool_to_schema()            └── impl ToolSchemaPort
├── low_risk_tools()          ├── all_tools_schema()
└── high_risk_tools()         └── low_risk_tools_schema()
```

**Domain** は「どのツールを使うか」（フィルタリング）を担当し、
**Infrastructure** は「API にどう渡すか」（JSON Schema 変換）を担当します。

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

### DI Chain / 依存注入チェーン

```
cli/main.rs
  └── JsonSchemaToolConverter → Arc<dyn ToolSchemaPort>
        ├── → RunAgentUseCase::new(gateway, executor, tool_schema)
        ├── → GatherContextUseCase::new(executor, tool_schema, ...)
        ├── → ExecuteTaskUseCase::new(..., tool_schema, ...)
        ├── → RunAskUseCase::new(gateway, executor, tool_schema)
        └── → AgentController → TuiApp
```

### param_type → JSON Schema 変換

| `param_type` | JSON Schema `type` |
|:----------:|:------------------:|
| `"string"`, `"path"` | `"string"` |
| `"number"` | `"number"` |
| `"integer"` | `"integer"` |
| `"boolean"` | `"boolean"` |

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

## Copilot CLI Wire Format / ワイヤーフォーマット

Copilot CLI プロバイダーでのツール定義の変換チェーン：

```
ToolDefinition (domain)
  → JsonSchemaToolConverter::tool_to_schema() → { "input_schema": {...} }
    → CopilotToolDefinition::from_api_tool() → { "parameters": {...} }
      → JSON-RPC session.create の tools パラメータとして送信
```

| 段階 | フィールド名 | 説明 |
|------|------------|------|
| `JsonSchemaToolConverter` 出力 | `"input_schema"` | プロバイダー中立 |
| `CopilotToolDefinition` | `"parameters"` | Copilot SDK 公式フォーマット |

> **Note**: Copilot SDK の公式ツールフィールドは `"parameters"`（`"inputSchema"` や `"input_schema"` ではない）。
> `CopilotToolDefinition::from_api_tool()` がマッピングを行います。

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
| `application/src/ports/llm_gateway.rs` | `ToolResultMessage`, `LlmSession` trait |
| `application/src/config/execution_params.rs` | `ExecutionParams::max_tool_turns` |
| `application/src/use_cases/run_agent/mod.rs` | `execute_with_progress()` のマルチターンループ |
| `infrastructure/src/tools/schema.rs` | `JsonSchemaToolConverter` — JSON Schema 変換実装 |
| `infrastructure/src/copilot/session.rs` | `CopilotSession` — `send_with_tools` / `send_tool_results` 実装 |
| `infrastructure/src/copilot/protocol.rs` | `CopilotToolDefinition` — Copilot SDK ワイヤーフォーマット |

### Data Flow / データフロー

```
ExecuteTaskUseCase::execute_single_task()
    │
    ▼
Native Tool Use multi-turn loop
    │
    ├── tool_schema.all_tools_schema(tool_spec) → JSON Schema 配列
    │
    ├── session.send_with_tools(prompt, tools) → LlmResponse
    │   └── CopilotSession: create_tool_session_and_send()
    │       ├── CopilotToolDefinition::from_api_tool() (input_schema → parameters)
    │       ├── router.create_session(params_with_tools)
    │       ├── router.request(session.send)
    │       └── tool_channel.read_streaming_for_tools()
    │           ├── Idle → LlmResponse (EndTurn)
    │           └── ToolCall → stash ToolSessionState → LlmResponse (ToolUse)
    │
    ├── response.tool_calls() → Vec<ToolCall> (直接抽出)
    │
    ├── Low-risk → futures::join_all() 並列実行
    │
    ├── High-risk → QuorumActionReviewer → 順次実行
    │
    ├── session.send_tool_results(results) → 次の LlmResponse
    │   └── CopilotSession: pending_tool_call.request_id で JSON-RPC レスポンス返送
    │
    └── turn_count < max_tool_turns? → ループ
```

---

## Related Features / 関連機能

- [Tool System](./tool-system.md) - ツールの定義、リスク分類、プロバイダーアーキテクチャ
- [Agent System](./agent-system.md) - エージェントライフサイクルと Quorum Review
- [Transport Demultiplexer](./transport.md) - MessageRouter による並列セッションルーティング
- [Ensemble Mode](../concepts/ensemble-mode.md) - マルチモデル計画生成
- [CLI & Configuration](../guides/cli-and-configuration.md) - 設定オプション

<!-- LLM Context: Native Tool Use API は LLM プロバイダーの構造化ツール呼び出しで、copilot-quorum の唯一のツール実行パス（フォールバック経路なし）。ToolSchemaPort (application/src/ports/tool_schema.rs) が Port パターンで JSON Schema 変換を分離し、JsonSchemaToolConverter (infrastructure/src/tools/schema.rs) が実装。DI チェーン: cli → RunAgentUseCase/GatherContextUseCase/ExecuteTaskUseCase/RunAskUseCase → AgentController → TuiApp。CopilotSession (infrastructure/src/copilot/session.rs) は send_with_tools() で内部 ToolSessionState (別セッション ID + SessionChannel + PendingToolCall) を作成し、send_tool_results() で pending_tool_call.request_id を使って JSON-RPC レスポンスを返送。StreamingOutcome::Idle → EndTurn、ToolCall → StopReason::ToolUse + ToolSessionState 保存。Copilot SDK ワイヤーフォーマット: CopilotToolDefinition が input_schema → parameters にマッピング。マルチターンループで StopReason::ToolUse の間ツール実行を繰り返す。Low-risk は futures::join_all() で並列、High-risk は QuorumActionReviewer + 順次。max_tool_turns (デフォルト 10) でループ制限。主要ファイルは domain/src/session/response.rs、application/src/ports/llm_gateway.rs、application/src/ports/tool_schema.rs、infrastructure/src/tools/schema.rs、infrastructure/src/copilot/session.rs。 -->
