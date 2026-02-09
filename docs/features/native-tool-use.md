# Native Tool Use API / ネイティブツール呼び出し API

> Structured tool calling via LLM provider APIs, eliminating text parsing and tool name hallucinations
>
> LLM プロバイダー API による構造化ツール呼び出し — テキストパースとツール名ハルシネーションの排除

---

## Overview / 概要

Native Tool Use API は、LLM にツール定義を **API パラメータとして構造化データで渡し**、
ツール呼び出しを **構造化レスポンスから直接抽出する** 仕組みです。

従来の **プロンプトベース方式** では、ツール定義をシステムプロンプトにテキスト埋め込みし、
LLM レスポンスから `` ```tool `` ブロックをパースしてツール呼び出しを抽出していました。
この方式には以下の問題がありました：

| 問題 | プロンプトベース | Native Tool Use |
|------|:---------------:|:---------------:|
| ツール名ハルシネーション（`bash` → `run_command` 等） | エイリアスで対応 | API が名前を強制 → **発生しない** |
| テキストパースの不安定さ | 4 段階フォールバック | パース不要 → **構造化データ** |
| ツール定義のトークン消費 | プロンプトに埋め込み | API パラメータ → **プロンプト不要** |
| マルチターン対話 | 1 ターンのみ | **ループで継続的にツール実行** |

Native Tool Use API は、Anthropic の `tool_use`、OpenAI の `function_calling` に対応した
業界標準のツール呼び出しプロトコルです。copilot-quorum はこの両方を同一の `LlmSession` trait で
抽象化し、プロバイダーに依存しないツール呼び出しを実現します。

---

## Quick Start / クイックスタート

Native Tool Use は、`LlmSession` の `tool_mode()` が `Native` を返す場合に自動的に使用されます。
ユーザーが明示的に設定する必要はありません。

```bash
# Anthropic API 直接呼び出し（将来）→ 自動的に Native mode
copilot-quorum --provider anthropic "List all Rust files"

# Copilot CLI 経由 → プロバイダーの対応状況で自動判定
copilot-quorum "Fix the failing test"
```

REPL での確認:

```
> /config
Provider: copilot
Tool Mode: native    ← API がツール定義を受け付ける場合
```

---

## How It Works / 仕組み

### Two-Path Architecture / 二経路アーキテクチャ

copilot-quorum は **PromptBased** と **Native** の 2 つのツール呼び出し経路を持ち、
`LlmSession::tool_mode()` の返り値で自動的に切り替えます。

```
LlmSession::tool_mode()
    │
    ├── PromptBased (従来方式)
    │   │
    │   ├── ツール定義 → システムプロンプトにテキスト埋め込み
    │   ├── LLM レスポンス → テキストパース (```tool / ```json)
    │   ├── エイリアス解決 → bash → run_command
    │   └── 1 ターンのみ
    │
    └── Native (Native Tool Use API)
        │
        ├── ツール定義 → API パラメータとして JSON Schema で送信
        ├── LLM レスポンス → ContentBlock::ToolUse から直接抽出
        ├── エイリアス不要 → API がツール名を保証
        └── マルチターンループ → ToolUse stop → 実行 → 結果送信 → ...
```

### Decision Logic / 経路決定ロジック

```rust
// LlmSession trait のデフォルト実装
fn tool_mode(&self) -> ToolMode {
    ToolMode::PromptBased  // 既存実装は何も変更不要
}
```

| 判定ポイント | 決定方法 |
|------------|---------|
| **どの経路を使うか** | `session.tool_mode()` の返り値（各 `LlmSession` 実装が決定） |
| **いつ決定されるか** | セッション作成時（`LlmGateway::create_session()` 内部） |
| **誰が決定するか** | Infrastructure 層の各 Session 実装（`AnthropicSession`, `CopilotSession` 等） |
| **ユーザーの介入** | 不要 — プロバイダーの能力に基づいて自動判定 |

### 各プロバイダーの対応状況

| Provider | `tool_mode()` | 実装方式 | Status |
|----------|:-------------:|---------|--------|
| **Copilot CLI** | `PromptBased` → `Native` | JSON-RPC `session.create` に `tools` パラメータ追加 | 将来 |
| **Anthropic API** | `Native` | Messages API の `tools` パラメータ | 将来 |
| **OpenAI API** | `Native` | Chat Completions の `function_calling` | 将来 |
| **Fallback** | `PromptBased` | 現行方式（エイリアス + テキストパース） | 実装済み |

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

#### `ToolDefinition::to_json_schema()` — JSON Schema 変換

```rust
impl ToolDefinition {
    pub fn to_json_schema(&self) -> serde_json::Value;
    // → {"name", "description", "input_schema": {"type": "object", ...}}
}

impl ToolSpec {
    pub fn to_api_tools(&self) -> Vec<serde_json::Value>;
    // → 全ツールの JSON Schema 配列
}
```

`param_type` → JSON Schema 変換:

| `param_type` | JSON Schema `type` |
|:----------:|:------------------:|
| `"string"`, `"path"` | `"string"` |
| `"number"` | `"number"` |
| `"integer"` | `"integer"` |
| `"boolean"` | `"boolean"` |

### Application Layer / アプリケーション層

#### `ToolMode` — ツール通信モード

```rust
pub enum ToolMode {
    PromptBased,  // プロンプト埋め込み + テキストパース
    Native,       // API パラメータ + 構造化レスポンス
}
```

#### `ToolResultMessage` — ツール実行結果

```rust
pub struct ToolResultMessage {
    pub tool_use_id: String,   // ContentBlock::ToolUse の id
    pub tool_name: String,     // ツール名（ログ用）
    pub output: String,        // 実行結果 or エラーメッセージ
    pub is_error: bool,        // エラーかどうか
}
```

#### `LlmSession` trait 拡張

```rust
#[async_trait]
pub trait LlmSession: Send + Sync {
    // === 既存メソッド（変更なし） ===
    fn model(&self) -> &Model;
    async fn send(&self, content: &str) -> Result<String, GatewayError>;
    async fn send_streaming(&self, content: &str) -> Result<StreamHandle, GatewayError>;

    // === 新メソッド（デフォルト実装あり → 既存 impl は変更不要） ===
    fn tool_mode(&self) -> ToolMode { ToolMode::PromptBased }

    async fn send_with_tools(
        &self, content: &str, tools: &[serde_json::Value],
    ) -> Result<LlmResponse, GatewayError> {
        // デフォルト: tools を無視して send() → LlmResponse::from_text()
    }

    async fn send_tool_results(
        &self, results: &[ToolResultMessage],
    ) -> Result<LlmResponse, GatewayError> {
        // デフォルト: テキストにフォーマットして send()
    }
}
```

**設計判断**: 別トレイト（`NativeToolSession`）にしなかった理由：
- `RunAgentUseCase` が `dyn LlmSession` で動作 → ダウンキャスト不要
- デフォルト実装により、既存の `CopilotSession` は一切変更不要
- 将来 Native 対応する際は `tool_mode()` のオーバーライドだけで OK

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

## Compatibility / 互換性

### 既存コードへの影響

| コンポーネント | 影響 | 理由 |
|-------------|------|------|
| `CopilotSession` | **変更不要** | デフォルト実装で `PromptBased` として動作 |
| `AgentPromptTemplate` | **変更不要** | 既存メソッドはそのまま動作 |
| `ToolSpec::aliases` | **引き続き有効** | PromptBased モードでは依然として必要 |
| `parse_tool_calls()` | **引き続き有効** | PromptBased モードでは依然として使用 |
| `resolve_tool_call()` | **Native では不要** | API がツール名を保証するため |
| テスト | **全テスト通過** | 破壊変更なし |

### エイリアスシステムとの関係

| 仕組み | Native mode | PromptBased mode |
|--------|:-----------:|:----------------:|
| Alias (`bash` → `run_command`) | 不要（API が名前強制） | 引き続き有効 |
| ToolRegistry (provider routing) | 必要 | 必要 |
| ToolProvider priority | 必要 | 必要 |

---

## Configuration / 設定

```toml
# quorum.toml
[agent]
max_tool_turns = 10    # Native ループの最大ターン数（デフォルト: 10）
```

```rust
// プログラム的に設定
let config = AgentConfig::default()
    .with_max_tool_turns(15);
```

---

## Architecture / アーキテクチャ

### Key Files / 主要ファイル

| File | Description |
|------|-------------|
| `domain/src/session/response.rs` | `LlmResponse`, `ContentBlock`, `StopReason` |
| `domain/src/session/stream.rs` | `StreamEvent`（`ToolCallDelta`, `CompletedResponse` バリアント） |
| `domain/src/tool/entities.rs` | `ToolCall::native_id`, `ToolDefinition::to_json_schema()`, `ToolSpec::to_api_tools()` |
| `domain/src/prompt/agent.rs` | `PromptToolMode`, `agent_system_native()` |
| `domain/src/agent/entities.rs` | `AgentConfig::max_tool_turns` |
| `application/src/ports/llm_gateway.rs` | `ToolMode`, `ToolResultMessage`, `LlmSession` 拡張 |
| `application/src/use_cases/run_agent.rs` | `execute_task_native()`, `execute_task_prompt_based()`, `send_with_tools_cancellable()` |

### Data Flow (Native Path) / データフロー

```
RunAgentUseCase::execute_single_task()
    │
    ├── session.tool_mode() == Native?
    │       │
    │       ▼
    │   execute_task_native()
    │       │
    │       ├── tool_spec.to_api_tools() → JSON Schema 配列
    │       │
    │       ├── session.send_with_tools(prompt, tools) → LlmResponse
    │       │
    │       ├── response.tool_calls() → Vec<ToolCall> (直接抽出)
    │       │
    │       ├── Low-risk → futures::join_all() 並列実行
    │       │
    │       ├── High-risk → Quorum Review → 順次実行
    │       │
    │       ├── session.send_tool_results(results) → 次の LlmResponse
    │       │
    │       └── turn_count < max_tool_turns? → ループ
    │
    └── session.tool_mode() == PromptBased?
            │
            ▼
        execute_task_prompt_based()
            │
            ├── session.send(prompt) → String
            │
            ├── parse_tool_calls(response) → Vec<ToolCall> (テキストパース)
            │
            ├── resolve_tool_call() → エイリアス解決
            │
            └── execute_tool_with_retry() → リトライ付き実行
```

---

## Related Features / 関連機能

- [Tool System](./tool-system.md) - ツールの定義、リスク分類、プロバイダーアーキテクチャ
- [Agent System](./agent-system.md) - エージェントライフサイクルと Quorum Review
- [Ensemble Mode](./ensemble-mode.md) - マルチモデル計画生成
- [CLI & Configuration](./cli-and-configuration.md) - 設定オプション

<!-- LLM Context: Native Tool Use API は LLM プロバイダーの構造化ツール呼び出し対応。LlmSession::tool_mode() で PromptBased/Native を自動判定し、Native 時は send_with_tools() → LlmResponse → ContentBlock::ToolUse から直接 ToolCall 抽出（テキストパース・エイリアス解決不要）。マルチターンループで StopReason::ToolUse の間ツール実行を繰り返す。Low-risk ツールは並列実行、High-risk は Quorum Review 後に順次実行。max_tool_turns（デフォルト10）でループ制限。主要ファイルは domain/src/session/response.rs、application/src/ports/llm_gateway.rs、application/src/use_cases/run_agent.rs。 -->
