# Transport Demultiplexer / トランスポート多重分離

> Message routing architecture for concurrent Copilot CLI sessions
>
> 並列 Copilot CLI セッションのためのメッセージルーティングアーキテクチャ

---

## Overview / 概要

copilot-quorum は GitHub Copilot CLI と **単一の TCP 接続** で通信しますが、
Quorum Discussion や Ensemble Planning では **複数セッションが同時に** 動きます。

`MessageRouter` はこの TCP 接続上のメッセージを `session_id` ベースで正しいセッションに
ルーティングする **demultiplexer**（多重分離器）です。

---

## Problem / 課題

Copilot CLI の JSON-RPC プロトコルでは、全イベントに `sessionId` が含まれています。
しかし、単一の TCP reader を複数セッションが Mutex で共有すると：

1. **混線**: Session A 向けのイベントを Session B が読んでしまう
2. **デッドロック**: 1 つのセッションが reader lock を保持すると他が待機
3. **スケーラビリティ**: セッション数に比例して lock 競合が増加

```
Before:
  Session A ──┐
  Session B ──┼─→ Arc<Mutex<reader>> ← 混線 + lock 競合
  Session C ──┘
```

---

## Solution / 解決策

**1 つの背景タスク** が TCP reader を専有し、受信メッセージを session_id で分類して
各セッション専用のチャネルに配送します。

```
After:
  Session A ← channel_a ←┐
  Session B ← channel_b ←┤── MessageRouter (background reader task)
  Session C ← channel_c ←┘        │
                                   └── TCP reader (single owner, no Mutex)
```

### Design Principles / 設計原則

| 原則 | 実現方法 |
|------|---------|
| **Reader は 1 タスクが専有** | 背景タスクが `OwnedReadHalf` を所有、Mutex 不要 |
| **Writer は独立** | `Mutex<BufWriter<OwnedWriteHalf>>` で書き込みをシリアライズ |
| **Session 間は完全分離** | 各セッションが専用の `mpsc::UnboundedReceiver` を持つ |
| **自動クリーンアップ** | `SessionChannel` の Drop で routes から自動削除 |

---

## Architecture / アーキテクチャ

### Component Diagram / コンポーネント図

```
┌─────────────────────────────────────────────────────┐
│                   MessageRouter                      │
│                                                      │
│  ┌──────────────┐    routes: HashMap<session_id, tx> │
│  │ Background   │    ┌─────────────────────┐         │
│  │ Reader Task  │───>│ Session A: tx_a     │──→ rx_a ──→ SessionChannel A
│  │              │    │ Session B: tx_b     │──→ rx_b ──→ SessionChannel B
│  │ (TCP reader) │    │ Session C: tx_c     │──→ rx_c ──→ SessionChannel C
│  └──────────────┘    └─────────────────────┘         │
│                                                      │
│  ┌──────────────┐    pending_responses:               │
│  │   Writer     │    HashMap<request_id, oneshot::tx> │
│  │ (TCP writer) │                                    │
│  └──────────────┘    session_start_rx (for create)   │
│                      create_lock (serialized create)  │
└─────────────────────────────────────────────────────┘
```

### Message Classification / メッセージ分類

背景 reader タスクが受信した JSON-RPC メッセージを 3 種類に分類します：

| 分類 | 条件 | ルーティング先 |
|------|------|--------------|
| **Response** | `id` あり, `method` なし | `pending_responses[id]` (oneshot) |
| **IncomingRequest** | `id` + `method` あり | `routes[params.session_id]` (tool.call) |
| **Notification** | `method` あり, `id` なし | `routes[session_id]` or `session_start_tx` |

#### Notification の詳細ルーティング

```
session.event
├── event_type == "session.start" → session_start_tx (create_session 用)
└── event_type == other           → routes[session_id] → SessionChannel
```

---

## Key Types / 主要な型

### MessageRouter

```rust
pub struct MessageRouter {
    _reader_handle: JoinHandle<()>,           // 背景 reader タスク
    routes: Arc<RwLock<HashMap<String, tx>>>,  // session_id → sender
    pending_responses: Arc<RwLock<HashMap<u64, oneshot::Sender>>>,  // request correlation
    session_start_rx: Mutex<UnboundedReceiver>,  // session.start イベント
    create_lock: Mutex<()>,                    // session 作成のシリアライズ
    writer: Mutex<BufWriter<OwnedWriteHalf>>,  // TCP 書き込み
    _child: Child,                             // Copilot CLI プロセス
}
```

**Public API:**

| メソッド | 説明 |
|---------|------|
| `spawn()` | Copilot CLI を起動して router を構築 |
| `create_session(params)` | セッション作成 → `(session_id, SessionChannel)` |
| `request(req)` | JSON-RPC リクエスト送信 + レスポンス待機 |
| `send_request(req)` | Fire-and-forget リクエスト送信 |
| `send_response(resp)` | JSON-RPC レスポンス送信（tool.call 結果返送用） |
| `deregister_session(id)` | セッション登録解除 |

### SessionChannel

```rust
pub struct SessionChannel {
    rx: mpsc::UnboundedReceiver<RoutedMessage>,
    session_id: String,
    router: Arc<MessageRouter>,  // Drop 時に deregister
}
```

**Public API:**

| メソッド | 説明 |
|---------|------|
| `recv()` | 次のメッセージを受信 |
| `read_streaming(on_chunk)` | session.idle まで streaming 読み取り |
| `read_streaming_for_tools(on_chunk)` | session.idle or tool.call まで読み取り |
| `read_streaming_with_cancellation(on_chunk, token)` | キャンセル対応 streaming |

**Drop で自動 deregister:**

```rust
impl Drop for SessionChannel {
    fn drop(&mut self) {
        self.router.deregister_session(&self.session_id);
    }
}
```

### RoutedMessage

```rust
pub enum RoutedMessage {
    SessionEvent { event_type: String, event: serde_json::Value },
    ToolCall { request_id: u64, params: ToolCallParams },
}
```

---

## Session Creation Flow / セッション作成フロー

セッション作成は `create_lock` でシリアライズされ、`session.start` イベントの
混同を防止します。

```
Thread A: create_session("claude-sonnet")
│
├── 1. create_lock.lock().await           ← シリアライズ
├── 2. send_request("session.create")
├── 3. session_start_rx.recv()            ← session.start イベントを待つ
├── 4. (tx, rx) = unbounded_channel()
├── 5. routes.insert(session_id, tx)      ← ルーティングテーブルに登録
├── 6. drop(create_lock)                  ← ロック解放
└── 7. return (session_id, SessionChannel { rx })
```

`request()` は oneshot パターンでレスポンスを待ちます：

```
request(req)
├── 1. oneshot::channel() → (tx, rx)
├── 2. pending_responses.insert(req.id, tx)
├── 3. send_request(req)
└── 4. rx.await                            ← 背景タスクが tx.send() するのを待つ
```

---

## Concurrency Patterns / 並行処理パターン

### Solo + /discuss (7 sessions)

```
/discuss "REST APIの設計"
├── Phase 1: Initial Query
│   ├── create_session(Claude)  → SessionChannel#1
│   ├── create_session(GPT)     → SessionChannel#2
│   └── create_session(Gemini)  → SessionChannel#3
│
├── Phase 2: Peer Review
│   ├── create_session(Claude)  → SessionChannel#4  (reviews GPT + Gemini)
│   ├── create_session(GPT)     → SessionChannel#5  (reviews Claude + Gemini)
│   └── create_session(Gemini)  → SessionChannel#6  (reviews Claude + GPT)
│
└── Phase 3: Synthesis
    └── create_session(Moderator) → SessionChannel#7
```

### Ensemble Planning (N² sessions)

3 モデルの場合:

```
Plan Generation: 3 sessions (1 per model)
Voting:          6 sessions (each plan × 2 other models)
Total:           9 concurrent sessions at peak
```

### Solo + Tool Use (2 sessions)

```
Main session:        SessionChannel#1  (ask/send)
Tool-enabled session: SessionChannel#2  (send_with_tools → tool.call loop)
```

---

## Integration / 統合

### Layer Mapping

```
CopilotLlmGateway (gateway.rs)
  └── Arc<MessageRouter>
        ├── create_session() → CopilotSession
        │     ├── Arc<MessageRouter>
        │     ├── Mutex<SessionChannel>      ← main session
        │     └── Mutex<Option<ToolSessionState>>
        │           └── SessionChannel       ← tool session
        └── background reader task
              └── routes HashMap → per-session channels
```

### Error Propagation

| シナリオ | エラー |
|---------|--------|
| TCP reader 切断 | `RouterStopped` (全 sender drop → receiver が None) |
| session.start タイムアウト | `RouterStopped` (create_lock 内で rx.recv() が None) |
| request の response 未到着 | `RouterStopped` (oneshot receiver が RecvError) |
| Copilot CLI クラッシュ | `TransportClosed` → reader loop 終了 → 上記パス |

---

## Source Files / ソースファイル

| File | Description |
|------|-------------|
| `infrastructure/src/copilot/router.rs` | MessageRouter, SessionChannel, RoutedMessage |
| `infrastructure/src/copilot/transport.rs` | classify_message, MessageKind, StreamingOutcome |
| `infrastructure/src/copilot/session.rs` | CopilotSession (uses MessageRouter + SessionChannel) |
| `infrastructure/src/copilot/gateway.rs` | CopilotLlmGateway (owns Arc\<MessageRouter\>) |
| `infrastructure/src/copilot/error.rs` | CopilotError (RouterStopped variant) |

<!-- LLM Context: MessageRouter は単一 TCP 接続上で複数並列セッションを demultiplex する。背景 reader タスクが session_id ベースで SessionChannel にルーティング。create_session は create_lock でシリアライズ。request は oneshot で response を correlation。SessionChannel は Drop で自動 deregister。主要ファイルは infrastructure/src/copilot/router.rs。 -->
