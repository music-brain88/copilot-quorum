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

なぜこの設計が必要か・どんな並行パターンが動くかは
[Transport & Concurrency](../explanation/transport-and-concurrency.md) を参照してください。

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


## Related / 関連

- [Transport & Concurrency](../explanation/transport-and-concurrency.md) - 設計原則と並行処理パターン
- [Native Tool Use](./native-tool-use.md) - ツールセッションの作成フロー
- [TUI Internals](./tui-internals.md) - TUI 側のイベントルーティング

<!-- LLM Context: MessageRouter は単一 TCP 接続上で複数並列セッションを demultiplex する。背景 reader タスクが session_id ベースで SessionChannel にルーティング。create_session は create_lock でシリアライズ。request は oneshot で response を correlation。SessionChannel は Drop で自動 deregister。主要ファイルは infrastructure/src/copilot/router.rs。 -->
