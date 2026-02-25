# Vision & Roadmap / ビジョンとロードマップ

> The evolution from "multi-LLM consensus tool" to "LLM orchestration platform"
>
> 「合議ツール」から「LLM オーケストレーションプラットフォーム」への進化

---

## Where We Are / 現在地

copilot-quorum v0.11 は **Copilot CLI 上で動く多モデル合議ツール** として、
以下の基盤を確立しています：

- Solo / Ensemble モードによる柔軟なモデル構成
- Quorum Discussion & Consensus（投票ベースの合意形成）
- Agent System（Plan → Review → Execute の自律実行）
- Native Tool Use API（構造化ツール呼び出し）
- Modal TUI（Neovim ライクなモーダルインターフェース）
- Transport Demultiplexer（並列セッションルーティング）

## Where We're Going / これからの方向

3 つの大きな進化軸があります：

```
Knowledge Layer    知識を蓄え、学習し、コンテキストを自動提供
     ↕
Workflow Layer     タスクを DAG で表現し、並列に実行
     ↕
Extension Platform ユーザーがスクリプトやプラグインで拡張
```

---

## Status Tracker / ステータス一覧

### Implemented / 実装済み ✅

| Feature | Description | Reference |
|---------|-------------|-----------|
| Modal TUI (Layer 0-1) | Normal/Insert/Command モード、Actor パターン | [tui.md](../guides/tui.md) |
| Agent System | Plan → Review → Execute フロー、HiL | [agent-system.md](../systems/agent-system.md) |
| Native Tool Use | 構造化 JSON Schema ツール呼び出し | [native-tool-use.md](../systems/native-tool-use.md) |
| Transport Demux | 並列セッションルーティング | [transport.md](../systems/transport.md) |
| Quorum Discussion | 多モデル合議 + 投票ベース合意 | [quorum.md](../concepts/quorum.md) |
| Custom Tools | TOML 設定ベースのカスタムツール登録 | [tool-system.md](../systems/tool-system.md) |
| `Task::depends_on` | タスク間の依存関係表現 | `domain/src/agent/entities.rs` |

### In Progress / 進行中 🟡

| Feature | Description | Reference |
|---------|-------------|-----------|
| Input Diversification (Layer 2) | $EDITOR 委譲、追加キーバインド | [Discussion #58](https://github.com/music-brain88/copilot-quorum/discussions/58) |
| Buffer/Tab System (Layer 3) | Agent/Ask/Discuss バッファの分離 | [Discussion #58](https://github.com/music-brain88/copilot-quorum/discussions/58) |

### Design Phase / 設計段階 🟠

| Feature | Description | Reference |
|---------|-------------|-----------|
| Workflow Layer | DAG ベース並列タスク実行 | [workflow-layer.md](workflow-layer.md), [Discussion #157](https://github.com/music-brain88/copilot-quorum/discussions/157) |

### Partially Implemented / 一部実装 🟡

| Feature | Description | Reference |
|---------|-------------|-----------|
| Extension Platform (Phase 1) | Lua ランタイム + Config/Keymap API | [extension-platform.md](extension-platform.md), [#193](https://github.com/music-brain88/copilot-quorum/issues/193) |

### Concept Phase / 構想段階 🔴

| Feature | Description | Reference |
|---------|-------------|-----------|
| Knowledge-Driven Architecture | 3 層構想（Knowledge / Context / Workflow） | [knowledge-architecture.md](knowledge-architecture.md), [Discussion #43](https://github.com/music-brain88/copilot-quorum/discussions/43) |
| Extension Platform (Phase 2+) | TUI API + Plugin + TOML→Lua 一本化 | [extension-platform.md](extension-platform.md), [#230](https://github.com/music-brain88/copilot-quorum/issues/230), [#231](https://github.com/music-brain88/copilot-quorum/issues/231), [#233](https://github.com/music-brain88/copilot-quorum/issues/233) |

---

## Evolution Map / 進化の全体像

```
v0.6  ─── Modal TUI 基盤 ──────────────────── ✅ Done
v0.7  ─── Agent System + Native Tool Use ───── ✅ Done
v0.8  ─── Transport Demux ─────────────────── ✅ Done
v0.11 ─── Custom Tools + Config 4-Type Split ─ ✅ Done (current)
          │
          ├─ Input Diversification (Layer 2) ── 🟡 In progress
          ├─ Buffer/Tab System (Layer 3) ────── 🟡 In progress
          │
          ├─ Workflow Layer ────────────────── 🟠 Design phase
          │   └─ DAG parallel task execution
          │
          ├─ Knowledge Layer ──────────────── 🔴 Concept
          │   ├─ KnowledgeStore trait
          │   ├─ GitHub Discussions 連携
          │   └─ Context Gathering 参照グラフ
          │
          └─ Extension Platform
              ├─ Phase 1: Lua Runtime + Config/Keymap ── ✅ Done (#193)
              ├─ Phase 2: TUI API ───────────────────── 🔴 Planned (#230)
              ├─ Phase 3: Plugin + Tools ─────────────── 🔴 Planned (#231)
              ├─ TOML → Lua 一本化 ──────────────────── 🔴 Planned (#233)
              └─ Protocol-Based extensions ──────────── 🔴 Concept
```

---

## Vision Documents / ビジョンドキュメント

| Document | Description |
|----------|-------------|
| [knowledge-architecture.md](knowledge-architecture.md) | Knowledge-Driven Architecture — 3 層構想 |
| [workflow-layer.md](workflow-layer.md) | Workflow Layer — DAG ベース並列タスク実行 |
| [extension-platform.md](extension-platform.md) | Extension Platform — スクリプティング + プラグイン |

---

## Related Discussions

- [Discussion #43](https://github.com/music-brain88/copilot-quorum/discussions/43): RFC: Quorum v2 — Knowledge-Driven Architecture
- [Discussion #58](https://github.com/music-brain88/copilot-quorum/discussions/58): Neovim-Style Extensible TUI
- [Discussion #98](https://github.com/music-brain88/copilot-quorum/discussions/98): Protocol-Based Extension Architecture — 詳細設計 (Layer 5)
- [Discussion #157](https://github.com/music-brain88/copilot-quorum/discussions/157): RFC: Workflow Layer — Graph-Based Task Execution & Parallel Dispatch
