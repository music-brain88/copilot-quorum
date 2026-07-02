# Transport & Concurrency / トランスポートと並行処理

> Why a single TCP connection needs a demultiplexer, and how concurrent sessions behave
>
> なぜ単一 TCP 接続に多重分離器が必要か、並列セッションはどう振る舞うか

---

## Problem / 課題

copilot-quorum は GitHub Copilot CLI と **単一の TCP 接続** で通信しますが、
Quorum Discussion や Ensemble Planning では **複数セッションが同時に** 動きます。

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

## Solution / 解決策

**1 つの背景タスク** が TCP reader を専有し、受信メッセージを session_id で分類して
各セッション専用のチャネルに配送します。これが `MessageRouter`（demultiplexer）です。

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

## Concurrency Patterns / 並行処理パターン

実際のワークロードでどれだけのセッションが並走するかの実例です。

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

## Related / 関連

- [Transport Reference](../reference/transport.md) - MessageRouter・SessionChannel の型とルーティング詳細
- [Native Tool Use](../reference/native-tool-use.md) - ツールセッションの仕組み
- [Quorum Discussion & Consensus](./quorum-consensus.md) - 並列セッションを使う 3 フェーズ議論

<!-- LLM Context: 単一 TCP 接続で複数並列セッション（Discussion 7、Ensemble N²、Tool Use 2）を動かすため、MessageRouter が背景 reader タスクで session_id ベースの demultiplex を行う。設計原則: reader 専有(Mutex なし)、writer 独立、session 完全分離、Drop 自動クリーンアップ。 -->
