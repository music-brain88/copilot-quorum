# Vision & Roadmap / ビジョンとロードマップ

> The evolution from "multi-LLM consensus tool" to "LLM orchestration platform"
>
> 「合議ツール」から「LLM オーケストレーションプラットフォーム」への進化

---

## Survival Strategy / 生存戦略

機能ロードマップとは別に、copilot-quorum が Claude Code + herdr という強力なライバルスタックの隣でどう生き残るかを整理したのが [Organ Strategy（臓器戦略）](organ-strategy.md) です。nix/mise の配役、Claude Code が構造的に持てない層、3 層スタック、3 トラックの現在地をまとめています。

---

## Where We Are / 現在地

copilot-quorum v0.12 は **Copilot CLI 上で動く多モデル合議ツール** として、
以下の基盤を確立しています：

- Solo / Ensemble モードによる柔軟なモデル構成
- Quorum Discussion & Consensus（投票ベースの合意形成）
- Agent System（Plan → Review → Execute の自律実行）
- Native Tool Use API（構造化ツール呼び出し）
- Modal TUI（Neovim ライクなモーダルインターフェース）
- Transport Demultiplexer（並列セッションルーティング）
- Content / Route / Surface TUI 表示基盤
- Tab / Pane バッファ管理基盤
- Lua スクリプティング Phase 1 + 1.5（Config / Keymap API）

## Where We're Going / これからの方向

4 つの大きな進化軸があります（[統合アーキテクチャビジョン](unified-architecture.md) 参照）：

```
TUI Display        Content → Route → Surface の柔軟な表示制御
     ↕
Interaction        Agent/Ask/Discuss を対等な peer form として統一
     ↕
Knowledge Layer    知識を蓄え、学習し、コンテキストを自動提供
     ↕
Extension Platform ユーザーがスクリプトやプラグインで拡張
```

---

## Status Tracker / ステータス一覧

### Implemented / 実装済み ✅

| Feature | Description | Reference |
|---------|-------------|-----------|
| Modal TUI (Layer 0-1) | Normal/Insert/Command モード、Actor パターン | [use-the-tui.md](../how-to/use-the-tui.md) |
| Agent System | Plan → Review → Execute フロー、HiL | [agent-system.md](../reference/agent-system.md) |
| Native Tool Use | 構造化 JSON Schema ツール呼び出し | [native-tool-use.md](../reference/native-tool-use.md) |
| Transport Demux | 並列セッションルーティング | [transport.md](../reference/transport.md) |
| Quorum Discussion | 多モデル合議 + 投票ベース合意 | [quorum-consensus.md](../explanation/quorum-consensus.md) |
| Custom Tools | Lua (`quorum.tools.register`) ベースのカスタムツール登録 | [tool-system.md](../reference/tool-system.md) |
| `Task::depends_on` | タスク間の依存関係表現 | `domain/src/agent/entities.rs` |
| Content/Route/Surface 基盤 | ContentSlot → RouteTable → SurfaceId | [Discussion #207](https://github.com/music-brain88/copilot-quorum/discussions/207) |
| Tab/Pane 基盤 | TabManager, Pane, PaneKind, `g` prefix key | [Discussion #58](https://github.com/music-brain88/copilot-quorum/discussions/58) |
| Ensemble Streaming | ModelStreamRenderer, 動的 ContentSlot | `presentation/src/tui/widgets/model_stream.rs` |
| Config 4-Type Split | SessionMode / ModelConfig / AgentPolicy / ExecutionParams | [unified-architecture.md](unified-architecture.md) |
| Lua Phase 1 + 1.5 | init.lua + Config/Keymap API, 全20キー mutable | [extension-platform.md](extension-platform.md), [#193](https://github.com/music-brain88/copilot-quorum/issues/193), [#235](https://github.com/music-brain88/copilot-quorum/issues/235) |
| Interaction 型（部分） | InteractionForm, InteractionId, InteractionTree | `domain/src/interaction/` |

### In Progress / 進行中 🟡

| Feature | Description | Reference |
|---------|-------------|-----------|
| Lua Phase 2 (TUI API) | quorum.tui.{routes,layout,content} API | [#230](https://github.com/music-brain88/copilot-quorum/issues/230) |

### Next / 次の優先事項 🔜

| Feature | Description | Reference |
|---------|-------------|-----------|
| Input Diversification (Layer 2) | $EDITOR 委譲、追加キーバインド | [Discussion #58](https://github.com/music-brain88/copilot-quorum/discussions/58) |
| #138 Phase A | ContextMode ワイヤリング + Ask/Discuss アクション化 | [Discussion #138](https://github.com/music-brain88/copilot-quorum/discussions/138) |
| Context Layer 強化 | BoundedResultBuffer, ConversationMemory | [#183](https://github.com/music-brain88/copilot-quorum/issues/183)-[#186](https://github.com/music-brain88/copilot-quorum/issues/186) |

### Design Phase / 設計段階 🟠

| Feature | Description | Reference |
|---------|-------------|-----------|
| Workflow Layer | DAG ベース並列タスク実行 | [workflow-layer.md](workflow-layer.md), [Discussion #157](https://github.com/music-brain88/copilot-quorum/discussions/157) |
| #138 Phase B | ツールベース spawn (spawn_ask/discuss/agent) | [Discussion #138](https://github.com/music-brain88/copilot-quorum/discussions/138) |
| Lua Phase 3 | Plugin + Tools + Commands API | [extension-platform.md](extension-platform.md), [#231](https://github.com/music-brain88/copilot-quorum/issues/231) |

### Concept Phase / 構想段階 🔴

| Feature | Description | Reference |
|---------|-------------|-----------|
| Knowledge Layer | KnowledgeStore trait, GitHub Discussions 連携 | [knowledge-architecture.md](knowledge-architecture.md), [Discussion #43](https://github.com/music-brain88/copilot-quorum/discussions/43) |
| TOML → Lua 一本化 | 設定ファイルを init.lua に統合 | [#233](https://github.com/music-brain88/copilot-quorum/issues/233) |
| Protocol-Based 拡張 | LSP/denops 的外部プロセス拡張 | [Discussion #98](https://github.com/music-brain88/copilot-quorum/discussions/98) |

---

## Evolution Map / 進化の全体像

```
v0.6  ─── Modal TUI 基盤 ──────────────────────── ✅ Done
v0.7  ─── Agent System + Native Tool Use ───────── ✅ Done
v0.8  ─── Transport Demux ─────────────────────── ✅ Done
v0.11 ─── Custom Tools + Config 4-Type Split ───── ✅ Done
v0.12 ─── Content/Route/Surface + Tab/Pane ─────── ✅ Done
      ─── Lua Phase 1 + 1.5 ───────────────────── ✅ Done
      ─── Ensemble Streaming ──────────────────── ✅ Done (current)
          │
          ├─ Lua Phase 2: TUI API ─────────────── 🟡 In progress (#230)
          │
          ├─ Input Diversification (Layer 2) ──── 🔜 Next
          ├─ #138 Phase A: ContextMode wiring ─── 🔜 Next
          ├─ Context Layer 強化 ───────────────── 🔜 Next (#183-186)
          │
          ├─ Workflow Layer ───────────────────── 🟠 Design (#157)
          ├─ #138 Phase B: Tool-based spawn ──── 🟠 Design
          ├─ Lua Phase 3: Plugin + Tools ─────── 🟠 Design (#231)
          │
          ├─ Knowledge Layer ─────────────────── 🔴 Concept (#43)
          ├─ TOML → Lua 一本化 ───────────────── 🔴 Concept (#233)
          └─ Protocol-Based extensions ────────── 🔴 Concept (#98)
```

> 詳細な依存関係と統合ビジョンは [unified-architecture.md](unified-architecture.md) を参照

---

## Vision Documents / ビジョンドキュメント

| Document | Description |
|----------|-------------|
| [**unified-architecture.md**](unified-architecture.md) | **統合アーキテクチャビジョン — 4つの RFC を統合整理** |
| [organ-strategy.md](organ-strategy.md) | 臓器戦略 — Claude Code + herdr と共存するための生存戦略 |
| [knowledge-architecture.md](knowledge-architecture.md) | Knowledge-Driven Architecture — 3 層構想 |
| [workflow-layer.md](workflow-layer.md) | Workflow Layer — DAG ベース並列タスク実行 |
| [extension-platform.md](extension-platform.md) | Extension Platform — スクリプティング + プラグイン |

---

## Related Discussions

- [Discussion #43](https://github.com/music-brain88/copilot-quorum/discussions/43): RFC: Quorum v2 — Knowledge-Driven Architecture
- [Discussion #58](https://github.com/music-brain88/copilot-quorum/discussions/58): Neovim-Style Extensible TUI
- [Discussion #98](https://github.com/music-brain88/copilot-quorum/discussions/98): Protocol-Based Extension Architecture — 詳細設計 (Layer 5)
- [Discussion #138](https://github.com/music-brain88/copilot-quorum/discussions/138): RFC: Unified Interaction Architecture — Agent/Ask/Discuss as Peer Forms
- [Discussion #157](https://github.com/music-brain88/copilot-quorum/discussions/157): RFC: Workflow Layer — Graph-Based Task Execution & Parallel Dispatch
- [Discussion #207](https://github.com/music-brain88/copilot-quorum/discussions/207): RFC: TUI Display Architecture — Content / Route / Surface 分離
- [Discussion #42](https://github.com/music-brain88/copilot-quorum/discussions/42): RFC: GitHub Discussions Integration for HiL — #43 に統合済み（HiL Storage は Knowledge Layer の一部として設計）

決着済みの設計判断は [Design Decisions (ADR)](../explanation/design-decisions/README.md) に記録されています。
