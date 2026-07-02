# Logging / ログシステム

> Structured conversation logging with JSONL and tracing-based operation logs
>
> JSONL 構造化会話ログと tracing ベース操作ログの分離設計

---

## Overview / 概要

copilot-quorum のログシステムは **3 種類のログ** を分離して管理します。
それぞれが異なる目的・フォーマット・消費者を持ちます。

| ログ種別 | 目的 | フォーマット | 実装 |
|---------|------|-----------|------|
| **操作ログ** | 診断・デバッグ | 人間可読テキスト | `tracing` + `tracing-subscriber` |
| **会話トランスクリプト** | 会話記録・分析 | JSONL | `ConversationLogger` port |
| **Transport dump** | 通信デバッグ | Raw JSON-RPC | `tracing::debug!` in transport layer |

---

## 操作ログ（tracing ベース）

標準的な Rust の `tracing` エコシステムを使用した操作ログです。
人間が読むための診断メッセージで、`RUST_LOG` 環境変数で制御されます。

```bash
# デバッグログ有効化
RUST_LOG=debug cargo run -p copilot-quorum -- "Your question"

# 特定クレートのみ
RUST_LOG=quorum_infrastructure=trace cargo run -p copilot-quorum -- "Your question"
```

### 依存関係

```toml
# Cargo.toml (workspace)
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-appender = "0.2"
```

### 使い分けの指針

| シナリオ | 使うべきログ |
|---------|------------|
| 「セッション作成に失敗した」 | `tracing::warn!` (操作ログ) |
| 「LLM が 500 トークン返した」 | `ConversationLogger` (会話ログ) |
| 「TCP 上の raw メッセージ内容」 | `tracing::debug!` (transport dump) |
| 「ツール実行に 3 秒かかった」 | 両方（操作ログ + 会話ログの metadata） |

---

## 会話トランスクリプト（ConversationLogger）

### Port 定義

`ConversationLogger` は application 層の port として定義されています。
会話イベント（LLM プロンプト/レスポンス、ツール呼び出し、計画投票など）を
機械可読な構造化ログに記録します。

```rust
// application/src/ports/conversation_logger.rs

struct ConversationEvent {
    event_type: &'static str,  // "llm_response", "tool_call", "plan_generated" など
    payload: serde_json::Value, // イベント固有のデータ
}

trait ConversationLogger: Send + Sync {
    fn log(&self, event: ConversationEvent);
}
```

**設計判断**:
- `log()` は **同期的かつ non-fallible** — ロギング失敗がメイン処理を中断しない
- `Send + Sync` — 複数スレッドからの同時書き込みに対応

### NoConversationLogger（NOP 実装）

テスト用やロギング無効時の no-op 実装です。

```rust
// application/src/ports/conversation_logger.rs
struct NoConversationLogger;

impl ConversationLogger for NoConversationLogger {
    fn log(&self, _event: ConversationEvent) {}
}
```

---

## JsonlConversationLogger — JSONL 実装

`ConversationLogger` の本番実装で、1 イベントを 1 行の JSON として書き出します。

```rust
// infrastructure/src/logging/jsonl_logger.rs
struct JsonlConversationLogger {
    writer: Mutex<BufWriter<File>>,
    path: PathBuf,
}
```

### スレッド安全性

- `Mutex<BufWriter<File>>` でスレッド安全な書き込み
- **Poisoned mutex recovery**: `unwrap_or_else(|e| e.into_inner())` で、
  別スレッドの panic 後もロギングを継続（best-effort 設計）
- `Drop` で最終 flush を実行

### JSONL スキーマ

各行は以下の構造を持つ JSON オブジェクトです：

```json
{"type":"llm_response","timestamp":"2026-02-19T10:30:00.123Z","model":"claude-sonnet-4.5","bytes":42,"text":"Hello world"}
{"type":"tool_call","timestamp":"2026-02-19T10:30:01.456Z","tool":"read_file","args":{"path":"foo.rs"}}
```

| フィールド | 型 | 説明 |
|-----------|------|------|
| `type` | string | イベント種別（`event_type` から自動付与） |
| `timestamp` | string | RFC 3339 UTC タイムスタンプ（ミリ秒精度） |
| *(その他)* | any | `payload` のフィールドがフラットに展開される |

#### Payload の展開ルール

- `payload` が **JSON Object** の場合: `type` と `timestamp` がマージされ、フラットに展開
- `payload` が **非 Object** の場合: `data` フィールドに格納

```json
// Object payload → フラット展開
{"type":"llm_response","timestamp":"...","model":"test","bytes":42}

// 非 Object payload → data フィールド
{"type":"simple_event","timestamp":"...","data":"just a string"}
```

### ファイル生成

```rust
let logger = JsonlConversationLogger::new("path/to/conversation.jsonl");
// → 親ディレクトリを自動作成
// → ファイル作成失敗時は None を返す（panic しない）
```

---

## Architecture / アーキテクチャ

### レイヤーマッピング

```
┌──────────────────────────────────────────────────┐
│  application (port)                               │
│                                                   │
│  ConversationLogger trait                         │
│  ConversationEvent struct                         │
│  NoConversationLogger (NOP)                       │
│                                                   │
├──────────────────────────────────────────────────┤
│  infrastructure (adapter)                         │
│                                                   │
│  JsonlConversationLogger                          │
│    └── Mutex<BufWriter<File>>                     │
│         └── .conversation.jsonl                   │
│                                                   │
├──────────────────────────────────────────────────┤
│  tracing (orthogonal)                             │
│                                                   │
│  tracing::info!/warn!/debug!                      │
│    └── tracing-subscriber (env-filter)            │
│         └── stderr / file                         │
│                                                   │
└──────────────────────────────────────────────────┘
```

### 2 つのログの位置づけ

```
                    ConversationLogger          tracing
                    ──────────────────         ────────
目的:               会話の完全な記録            診断・デバッグ
フォーマット:       JSONL (機械可読)            テキスト (人間可読)
消費者:             分析ツール、再生            開発者
制御:               コード内の log() 呼び出し   RUST_LOG 環境変数
失敗時:             サイレント無視              サイレント無視
レイヤー:           application port            横断的関心事
```

---

## Source Files / ソースファイル

| File | Description |
|------|-------------|
| `application/src/ports/conversation_logger.rs` | ConversationLogger trait, ConversationEvent, NoConversationLogger |
| `infrastructure/src/logging/mod.rs` | Module re-exports |
| `infrastructure/src/logging/jsonl_logger.rs` | JsonlConversationLogger (JSONL 実装) |

<!-- LLM Context: ログシステムは 3 分割設計: (1) tracing ベース操作ログ (RUST_LOG 制御), (2) ConversationLogger port による JSONL 会話トランスクリプト, (3) transport dump。ConversationLogger は application 層の port で、log() は同期・non-fallible（best-effort）。JsonlConversationLogger が infrastructure 層の実装で、Mutex<BufWriter<File>> によるスレッド安全な JSONL 書き出し。各行は type + timestamp + フラット展開された payload。NoConversationLogger はテスト用 NOP。主要ファイルは application/src/ports/conversation_logger.rs と infrastructure/src/logging/jsonl_logger.rs。 -->
