# Unified Architecture Vision / 統合アーキテクチャビジョン

> Discussions #58, #207, #43, #138 の統合整理
>
> 最終更新: 2026-02-28

---

## TL;DR

copilot-quorum は **「合議ツール」→「LLM オーケストレーションプラットフォーム」** に進化する。4つの RFC を統合すると、**Interaction を中心に Backend 3層と TUI 3層が同じ3つの問い (what/how/where) に異なる領域で答える** 全体像になる:

```
┌──────────────────── copilot-quorum ──────────────────────────────┐
│                                                                   │
│  Extension Platform (Lua init.lua)                                │
│  config · keymap · tui · on() · command · tools                   │
│                                                                   │
│                      ┌── Interaction ──┐                          │
│                      │ Agent|Ask|Discuss│                          │
│                      │ spawn·nest·cycle │                          │
│                      └────────┬────────┘                          │
│                               │                                   │
│  Backend (処理側)             │               TUI (提示側)        │
│                               │                                   │
│  ┏━━━━━━━━━━━━━━━━━━┓ inform │      what    ┌─────────────────┐  │
│  ┃ Knowledge        ┃───────→│ · · · · · · │ Content          │  │
│  ┃ 何を知っているか  ┃        │              │ 何を見せるか      │  │
│  ┃ ≈ 楽譜 (Score)   ┃        │              │ Buffer · Slot    │  │
│  ┗━━━━━━━━━━━━━━━━━━┛        │              └─────────────────┘  │
│                               │                                   │
│  ┊╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┊  emit │      how     ┊╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┊ │
│  ┊ Context           ┊←─────│ · · · · · · ┊ Route            ┊ │
│  ┊ どう伝搬するか    ┊        │              ┊ どう流すか        ┊ │
│  ┊ ≈ 共鳴 (Resonance)┊        │              ┊ Mapping · Config ┊ │
│  ┊╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┊        │              ┊╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┊ │
│                               │                                   │
│  ┌──────────────────┐ drive  │      where   ┌─────────────────┐  │
│  │ Workflow          │←─────→│ · · · · · · │ Surface          │  │
│  │ どこで実行するか  │        │              │ どこに出すか      │  │
│  │ ≈ 演奏 (Perform.) │        │              │ Pane · Tab       │  │
│  └──────────────────┘        │              └─────────────────┘  │
│                               │                                   │
│  ※ 全 Backend 層の出力が Interaction を通じて全 TUI 層に届く       │
│    what/how/where は構造の同型性であり、1:1のデータ写像ではない     │
└───────────────────────────────┴───────────────────────────────────┘
```

### 図の読み方

**罫線スタイル = レイヤー性質**:

| 罫線 | レイヤー | 比喩 | 性質 | ML 補注 |
|------|---------|------|------|---------|
| `┏━━┓` 太線 | Knowledge | **楽譜** (Score) | 演奏前に書かれ、開いて読む | Hyperparameters |
| `┊╌╌┊` 破線 | Context | **共鳴** (Resonance) | 演奏中に生まれ、奏者間を伝搬する | Hidden State |
| `┌──┐` 通常線 | Workflow | **演奏** (Performance) | 楽譜を解釈し、実際に音にする | Forward Pass |

**矢印 = Backend と Interaction の関係** (実データフロー):

| 矢印 | Backend → Interaction | 意味 |
|-------|----------------------|------|
| `───→` 右向き | Knowledge → inform | 蓄積が Interaction に方向づけを与える |
| `←───` 左向き | Context ← emit | Interaction の中で発生・伝搬する |
| `←──→` 双方向 | Workflow ↔ drive | 実行を駆動し、フローを制御する |

**`· · ·` 点線 = 構造の同型性 (Structural Isomorphism)**:

Backend と TUI は同じ3つの問いに、**それぞれの領域で** 答える:

| 問い | Backend (処理側) | TUI (提示側) |
|------|-----------------|-------------|
| **what** | Knowledge — 何を知っているか | Content — 何を見せるか |
| **how** | Context — どう伝搬するか | Route — どう流すか |
| **where** | Workflow — どこで実行するか | Surface — どこに出すか |

> **音楽の比喩**: 楽譜を読んで演奏を始めると、奏者の間に共鳴が生まれる。
> その共鳴がまた演奏に影響し、楽譜だけでは予測できない音楽になる。
> 楽譜にない音が鳴る = セレンディピティ。共鳴が強すぎるとハウリング = コンテキスト膨張。
>
> これは「鏡像」でも1:1のデータ写像でもない。Content は Knowledge だけでなく
> Context や Workflow の出力も表示する（例: ToolLog は Workflow の産物、
> DiscussionGraph は Context の産物）。Backend 全層の出力が Interaction を通じて
> TUI 全層に届く。what/how/where は **同じ問いへの別領域の回答** である。
>
> TUI の Content/Route/Surface を制御することは、Backend の
> Knowledge/Context/Workflow の3層構造を整理する手段にもなる。

---

## 1. 全体像: 4つの RFC が描くもの

### Discussion 間の関係

```
#58 Neovim-Style TUI (マスターロードマップ)
 │
 ├── Layer 3 Buffer/Tab ──→ #138 Unified Interaction Architecture
 │                              └── domain: Interaction(Agent|Ask|Discuss)
 │                              └── presentation: Tab + Pane + PaneKind
 │
 ├── Layer 4-5 ────────→ #207 Content/Route/Surface
 │                          └── Content(何を) → Route(どこに) → Surface(どう表示)
 │
 └── Backend Vision ──→ #43 Knowledge-Driven Architecture
                           └── Knowledge Layer + Context Layer + Workflow Layer
```

各 RFC の責務:

| RFC | 領域 | 核心 |
|-----|------|------|
| **#58** | TUI 全体 | Neovim ライクなモーダル + スクリプト拡張プラットフォーム |
| **#207** | 表示層 | Content/Route/Surface の3層分離（noice.nvim + ddu パターン） |
| **#43** | バックエンド | Knowledge/Context/Workflow の3層（知識駆動型エージェント基盤） |
| **#138** | ドメインモデル | Agent/Ask/Discuss を対等な peer form として統一 |

---

## 2. 現在のアーキテクチャ (v0.12 時点)

### DDD + Onion Architecture

```
           cli/                  # Entrypoint, DI assembly
             │
      presentation/              # TUI (ratatui, Actor pattern)
             │
infrastructure/  ──→ application/   # Adapters ──→ Use cases + ports
        │                │
        └───→  domain/  ←┘         # Pure business logic
```

### 実装済み機能

| 機能 | 状態 | 参照 |
|------|------|------|
| Modal TUI (Layer 0-1) | ✅ Done | Normal/Insert/Command モード |
| Content/Route/Surface 基盤 | ✅ Done | ContentSlot, RouteTable, SurfaceId |
| Agent System | ✅ Done | Plan → Review → Execute + HiL |
| Native Tool Use | ✅ Done | JSON Schema ベース構造化ツール呼び出し |
| Transport Demux | ✅ Done | 並列セッションルーティング |
| Quorum Discussion | ✅ Done | 多モデル合議 + 投票ベース合意 |
| Custom Tools | ✅ Done | TOML 設定ベースカスタムツール |
| Config 4-Type Split | ✅ Done | SessionMode / ModelConfig / AgentPolicy / ExecutionParams |
| Lua Phase 1 | ✅ Done | init.lua + Config/Keymap API |
| Lua Phase 1.5 | ✅ Done | ConfigAccessorPort 全20キー mutable |
| Ensemble Streaming | ✅ Done | ModelStreamRenderer, 動的 ContentSlot |
| Tab/Pane 基盤 | ✅ Done | TabManager, Pane, PaneKind, `g` prefix key |
| Lua Phase 2 (TUI API) | 🟡 WIP | quorum.tui.{routes,layout,content} |
| Interaction 型 | 🟡 Partial | InteractionForm, InteractionId, InteractionTree (domain) |

### クレート依存グラフ

```
copilot-quorum (cli)
    ├── quorum-presentation ──→ quorum-application ──→ quorum-domain
    └── quorum-infrastructure ──→ quorum-application ──→ quorum-domain

※ presentation ⊥ infrastructure（DI は cli で解決）
```

### 主要 Port/Adapter

| Port (application) | Adapter (infrastructure) | 用途 |
|----|----|----|
| `LlmGateway` / `LlmSession` | `CopilotLlmGateway` / `CopilotSession` | LLM 通信 |
| `ToolExecutorPort` | `LocalToolExecutor` | ツール実行 |
| `ToolSchemaPort` | `JsonSchemaToolConverter` | JSON Schema 変換 |
| `ScriptingEnginePort` | `LuaScriptingEngine` | Lua スクリプティング |
| `ConfigAccessorPort` | `QuorumConfig` (impl) | ランタイム config |
| `TuiAccessorPort` | `TuiAccessorState` | Lua → TUI 変更伝播 |
| `HumanInterventionPort` | TUI overlay | HiL 介入 |
| `ConversationLogger` | `JsonlConversationLogger` | 会話ログ永続化 |

### DI 共有構造

```
Arc<Mutex<QuorumConfig>>               Arc<Mutex<dyn TuiAccessorPort>>
    ├── LuaScriptingEngine                 ├── LuaScriptingEngine
    │   (config get/set)                   │   (tui.routes/layout/content 書込)
    └── AgentController                    └── TuiApp
        (runtime config 読取)                  (take_pending_changes() 毎フレーム)
```

---

## 3. TUI Display Architecture (#207)

### 設計思想

Neovim の `buffer / window` 分離と [noice.nvim](https://github.com/folke/noice.nvim) の `Source → Route → View` パターンに倣い、「何を表示するか」と「どこに表示するか」を分離する。

### 3層モデル

```
Content (何を表示するか)  → ddu の Source パターン（独立バッファ）
Route   (どこに流すか)    → noice.nvim の Route パターン（設定可能マッピング）
Surface (器の配置)        → tmux プリセット + Telescope の動的計算
```

### 現行実装の型

**ContentSlot** — 表示すべきデータの論理単位:

| ContentSlot | 用途 | 動的? |
|-------------|------|-------|
| `Conversation` | メッセージ履歴 + ストリーミング | No |
| `Progress` | フェーズ・タスク進捗 | No |
| `ToolLog` | ツール実行ログ | No |
| `HilPrompt` | 人間介入プロンプト | No |
| `Help` | キーバインドヘルプ | No |
| `Notification` | 一時的通知 | No |
| `ModelStream(name)` | Ensemble 個別モデル出力 | Yes |
| `LuaSlot(name)` | Lua 登録カスタムスロット | Yes |

**SurfaceId** — レンダリング先の物理領域:

```
MainPane | Sidebar | Overlay | Header | Input | StatusBar | TabBar
ToolPane | ToolFloat | DynamicPane(name)
```

**RouteTable** — Content → Surface のマッピング:

```rust
// デフォルトルーティング
Conversation → MainPane
Progress     → Sidebar
HilPrompt   → Overlay
Help         → Overlay
Notification → StatusBar
```

**ContentRegistry** — `HashMap<ContentSlot, Box<dyn ContentRenderer>>`:
- `.register()` — 静的登録（ビルトインレンダラー）
- `.register_mut()` — 動的登録（Ensemble ModelStream, Lua カスタムスロット）

### LayoutPreset — tmux 的プリセット

```toml
[tui.layout]
preset = "default"   # "default" | "minimal" | "wide" | "stacked"
```

- `default`: 2ペイン（Conversation 70% / Progress 30%）
- `minimal`: 1ペイン（Conversation のみ）
- `wide`: 3ペイン
- Lua から `quorum.tui.layout.register_preset()` でカスタムプリセット登録可能

### 設定解決チェーン（ddu パターン）

```
[tui.routes]           (デフォルト — patch_global 相当)
  ↓ override
[tui.presets.xxx]      (モード別 — patch_local 相当)
  ↓ override
runtime keybind/Lua    (アドホック — ddu#start 相当)
  ↓ resolve
Content → Route → Surface  (最終描画)
```

### 未実装（将来）

- ContentRenderer の分離（Content ごとの描画オプション）
- Preset システム（Solo/Ensemble 切替時に自動適用）
- Float / Popup Surface
- z-index / フォーカス管理

---

## 4. Neovim-Style Extensible TUI (#58)

### Layer 構成

```
┌──────────────────────────────────────────────────────────────────┐
│ Layer 5: Scripting Platform                 🟡 PHASE 1+1.5 DONE │
│   ✅ init.lua · quorum.config · quorum.keymap · quorum.on()     │
│   🟡 quorum.tui.* (Phase 2 WIP)                                 │
│   🔴 quorum.command() · quorum.tools.*                           │
├──────────────────────────────────────────────────────────────────┤
│ Layer 4: Advanced UX                                 🔮 FUTURE  │
│   VISUAL Mode · Merge View · Pane Management                    │
├─────────────────────────────┬────────────────────────────────────┤
│ Layer 2: Input              │ Layer 3: Buffer/Tab    🟡 PARTIAL │
│ Diversification  🔜 NEXT   │   Tab/Pane 基盤 ✅                │
│   $EDITOR · / · y · .      │   Interaction 型 🟡               │
├─────────────────────────────┴────────────────────────────────────┤
│ Layer 1: Modal Foundation                       ✅ DONE (v0.6)  │
│   Normal/Insert/Command · Keybindings · :commands · HiL UI      │
├──────────────────────────────────────────────────────────────────┤
│ Layer 0: TUI Infrastructure                     ✅ DONE (v0.6)  │
│   ratatui · Actor Pattern · Streaming · AgentController          │
└──────────────────────────────────────────────────────────────────┘
```

### 3つの入力粒度

```
一言で済む            対話的に書く           がっつり書く
:ask Fix the bug      i で INSERT モード     I で $EDITOR 起動
    ↓                     ↓                     ↓
COMMAND モード         INSERT モード          $EDITOR (vim/neovim)
```

**copilot-quorum はエディタを再実装しない。ユーザーの使い慣れた本物のエディタに委譲する。**

### NORMAL モード — オーケストレーション操作盤

| キー | アクション | 対応概念 |
|------|-----------|----------|
| `s` | Solo モード | ConsensusLevel |
| `e` | Ensemble モード | ConsensusLevel |
| `f` | Fast/Full トグル | PhaseScope |
| `d` | `:discuss` プリフィル | InteractionForm |
| `j/k` | スクロール | — |
| `gg/G` | 先頭/末尾 | — |
| `gt/gT` | 次/前のタブ | Tab/Pane |
| `i` | INSERT モード | — |
| `I` | $EDITOR 起動 | — |
| `:` | COMMAND モード | — |

### 競合との差別化

| | Copilot CLI | OpenCode | Claude Code | **copilot-quorum** |
|---|---|---|---|---|
| UI パラダイム | 会話型 REPL | Vim TUI | 会話型 REPL | **Modal + Scripting** |
| 拡張性 | なし | キーバインド | MCP サーバー | **ユーザースクリプト + プラグイン** |
| 入門コスト | 低 | 中 | 低 | **高** |
| 天井 | 低 | 中 | 中 | **高** |

---

## 5. Unified Interaction Architecture (#138)

### 核心: Agent / Ask / Discuss は対等な peer

```
Vim:
  Buffer(buftype="")          ← 普通のバッファ
  Buffer(buftype="help")      ← ヘルプバッファ
  Buffer(buftype="terminal")  ← ターミナルバッファ
  → 全て Buffer の type。「普通のバッファ」が他の親ではない。

copilot-quorum:
  Interaction(form=Agent)     ← 自律実行
  Interaction(form=Ask)       ← 問い合わせ
  Interaction(form=Discuss)   ← 合議
  → 全て Interaction の form。Agent が他の親ではない。
```

### Domain Model

```rust
// domain/src/interaction/
struct Interaction {
    id: InteractionId,
    form: InteractionForm,          // Agent | Ask | Discuss
    context_mode: ContextMode,      // Full | Projected | Fresh
    model_config: ModelConfig,
    parent: Option<InteractionId>,  // ネスト親
    depth: usize,                   // ネスト深度
}

enum InteractionForm {
    Agent(AgentInteraction),     // PhaseScope, AgentPolicy, Plan, ...
    Ask(AskInteraction),         // 単一モデル, read-only tools
    Discuss(DiscussInteraction), // Strategy, 複数モデル
}
```

### 各 form の特性

| 特性 | Ask | Discuss | Agent |
|------|-----|---------|-------|
| ライフサイクル | Query → Response | Collect → Review → Synthesize | Context → Plan → Execute |
| モデル数 | 単一 | 複数 | ロールベース |
| ツール | read-only | なし | 全て (risk-based) |
| ContextMode default | Fresh | Fresh | Full |
| spawn | 全 form | 全 form | 全 form |

### 再帰ネスティング

```
Ask("バグの原因は？")
└─ Agent(調査実行)              ← 聞いたら調査が必要だった
   └─ Discuss(設計判断)         ← 調査中に合議が必要に

Agent("認証システム実装")
└─ Discuss(設計合議)            ← 実装中に合議が必要に
   └─ Agent(PoC 調査)          ← 合議中に実証が必要に
```

### Spawn メカニズム（段階的）

| Phase | 方式 | リスク |
|-------|------|--------|
| A | ユーザー起動（`:ask`, `:discuss`, `:agent`） | 低 |
| B | ツールベース（`spawn_ask` etc. = RiskLevel::High → HiL レビュー） | 中 |
| C | ポリシー自動化（`AgentPolicy.auto_discuss_on_high_risk`） | 高 |

### Presentation 層: Vim 3層モデル

```
Vim:                    copilot-quorum:
Buffer (データ)     →   Interaction (domain — 会話の論理単位)
Window (ビュー)     →   Pane (presentation — 表示ビューポート)
Tab Page (グループ) →   Tab (presentation — Pane のレイアウトグループ)
```

```rust
// presentation 層
struct Tab {
    id: TabId,
    panes: Vec<Pane>,
    layout: PaneLayout,       // Single | VSplit | HSplit
    active_pane: usize,
    display_name: String,
}

struct Pane {
    id: PaneId,
    kind: PaneKind,
    messages: Vec<DisplayMessage>,
    scroll_offset: usize,
    progress: ProgressState,
    input: String,            // per-pane input buffer
}

enum PaneKind {
    Interaction(InteractionId),
    Knowledge(KnowledgeQuery),   // :help 相当
    Log(LogFilter),              // :messages 相当
}
```

### OrchestrationStrategy とアンサンブル学習の対応

| ML 手法 | やること | Strategy | 現在の実装 |
|---------|---------|----------|-----------|
| **Stacking** | メタモデル統合 | Stacking (旧 Quorum) | RunQuorumUseCase 3フェーズ |
| **Voting** | 多数決/最良選択 | Voting (旧 Ensemble Planning) | 並列計画生成→投票 |
| **Boosting** | 逐次的改善 | Boosting (旧 Debate) | 反論→改善の繰り返し |

### Config 必要性マップ

| Config | Agent | Ask | Discuss |
|--------|-------|-----|---------|
| `SessionMode` | ✓ (全3軸) | — (固定) | Strategy のみ |
| `ModelConfig` | ✓ (role-based) | ✓ (単一) | ✓ (複数) |
| `AgentPolicy` | ✓ | — | — |
| `ExecutionParams` | ✓ | ✓ (一部) | — |

---

## 6. Knowledge-Driven Architecture (#43)

### 3層構想

```
┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃              Knowledge Layer ≈ 楽譜 (Score)                      ┃
┃  - 設計決定の履歴           KnowledgeStore trait                  ┃
┃  - 過去の Plan/Review 結果  KnowledgeEntry enum                  ┃
┃  - プロジェクト固有パターン  LocalFileStore / SQLiteStore          ┃
┃  - HiL State                GitHub Discussions 連携              ┃
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
         ↑↓
┊╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┊
┊              Context Layer ≈ 共鳴 (Resonance)                    ┊
┊  - 議論グラフ（同意/反論/補足）  DiscussionGraph                   ┊
┊  - LLM 間の関係性               DiscussionNode + DiscussionEdge  ┊
┊  - セッション履歴               ConversationMemory                ┊
┊  - コンテキスト膨張制御         BoundedResultBuffer (#183)        ┊
┊╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┊
         ↑↓
┌──────────────────────────────────────────────────────────────────┐
│              Workflow Layer ≈ 演奏 (Performance)                  │
│  - グラフベースの状態遷移   WorkflowGraph                         │
│  - 並列 Agent 実行          Parallel node type                    │
│  - 動的フロー制御           Conditional branching                 │
└──────────────────────────────────────────────────────────────────┘
```

### #138 との統合 — 同じ3問、異なる領域

Interaction が中心軸として Backend 全層と関わり、その出力が TUI 全層に届く。
Backend と TUI は1:1のデータ写像ではなく、**同じ3つの問いに別領域で答える** 構造的同型性を持つ:

```
Backend (処理側)         Interaction          TUI (提示側)
──────────────          ──────────          ──────────────
Knowledge ──inform─→    Agent|Ask|Discuss  ←─ Content
Context   ←──emit──     spawn·nest·cycle   ←─ Route
Workflow  ←─drive──→                       ←─ Surface
        ↓                                       ↓
what: 何を知っているか              what: 何を見せるか
how:  どう伝搬するか               how:  どう流すか
where: どこで実行するか             where: どこに出すか
```

**具体例: Agent("認証システム実装")**
- **楽譜を開く** — Knowledge が過去の設計パターンで Plan 生成を inform
- **共鳴が生まれる** — Context で DiscussionGraph が emit、HiL 状態が奏者間を伝搬
- **演奏する** — Workflow が Task DAG を実行、動的に分岐・合流
- **客席に届く (TUI)** — Content が全 Backend 層の出力を表示（Plan も ToolLog も DiscussionGraph も）、Route が Solo/Ensemble で流れを切替、Surface が Pane/Tab に配置

### Context Gathering 拡張（参照グラフ自動追跡）

Knowledge Layer の段階的プロトタイプとして、コンテキスト収集時にテキスト中の参照（`#NNN`, URL 等）を自動追跡する:

```
ユーザー: "Issue #127 をレビューして"
├─ depth 0: Issue #127 を取得
│   └─ Related: Discussion #58, Issue #119, #120, #121
├─ depth 1: 各参照を取得
│   └─ depth 2 で停止
└─ 全コンテキストを context_brief に統合
```

### Context Layer 強化ロードマップ

| Phase | Issue | 概要 |
|-------|-------|------|
| 1 | #183 | `previous_results` にサイズ上限 (BoundedResultBuffer) |
| 2 | #184 | `HistoryEntry` → `ConversationMemory` 構造化 |
| 3 | #185 | `ConversationMemoryStore`（2層メモリ + 自動圧縮） |
| 4 | #186 | JSONL ログからの会話コンテキスト復元 |

---

## 7. Extension Platform (Layer 5)

### Lua API 全体像

```lua
-- ✅ Phase 1 (Done) — Config + Keymap + Events
quorum.on(event, callback)              -- 7 events
quorum.config.get(key)                  -- 20 keys, all read-write
quorum.config.set(key, value)
quorum.config["key"] = value            -- metatable proxy
quorum.keymap.set(mode, key, action)    -- string action or Lua callback

-- 🟡 Phase 2 (WIP) — TUI Route/Layout/Content API
quorum.tui.routes.set(content, surface)
quorum.tui.routes.get(content)
quorum.tui.routes.list()
quorum.tui.layout.current()
quorum.tui.layout.switch(preset)
quorum.tui.layout.register_preset(name, config)
quorum.tui.content.register(slot_name)  -- カスタムスロット登録
quorum.tui.content.set_text(slot, text)

-- 🔴 Phase 3 (Planned) — Plugin + Tools + Commands
quorum.command(name, callback)          -- ユーザーコマンド定義
quorum.tools.register(name, config)     -- カスタムツール登録

-- 🔴 TOML → Lua 一本化 (Planned)
quorum.providers.anthropic = { ... }    -- プロバイダ設定
```

### 変更伝播フロー (Phase 2)

```
Lua: quorum.tui.routes.set("progress", "main_pane")
  → tui_api.rs: TuiAccessorPort::route_set()
  → TuiAccessorState: pending.route_changes.push(...)
  [次フレーム]
  → TuiApp::tick(): take_pending_changes()
  → RouteTable::set_route() 反映
```

### ロードマップ

```
Phase 1:   Lua Runtime + Config/Keymap API      ── ✅ Done (#193)
Phase 1.5: ConfigAccessorPort 全キー mutable     ── ✅ Done (#235)
Phase 2:   TUI Route/Layout/Content API          ── 🟡 WIP (#230)
Phase 3:   Plugin + Tools + Commands API         ── 🔴 Planned (#231)
TOML→Lua:  設定ファイル一本化                     ── 🔴 Planned (#233)
Protocol:  Protocol-Based 拡張 (LSP/denops 的)   ── 🔴 Concept
```

---

## 8. 統合ロードマップ

### Phase マッピング

```
                   TUI (#58/#207)      Domain (#138)      Backend (#43)      Lua (#58 L5)
                   ─────────────       ──────────────     ─────────────      ───────────
✅ Done            Layer 0-1           Interaction 型     Agent System       Phase 1+1.5
                   Content/Route/      (partial)          Context Gathering  Config/Keymap
                   Surface 基盤                           /init
                   Tab/Pane 基盤

🟡 In Progress     Lua TUI API         InteractionTree    —                  Phase 2
                   (Phase 2 WIP)                                             TUI API

🔜 Next            Layer 2 Input       Phase A:           Context Layer      —
                   $EDITOR 委譲        ContextMode        強化 (#183-186)
                                       ワイヤリング

💡 Design          Layer 3 完成        Phase B:           Workflow Layer     Phase 3
                   Buffer/Tab UI       ツールベース       DAG 並列実行       Plugin/Tools
                                       spawn

🔮 Future          Layer 4             Phase C:           Knowledge Layer    TOML→Lua
                   VISUAL / Merge      ポリシー自動       KnowledgeStore     Protocol拡張
                   Pane 管理           spawn              GitHub 連携
```

### 依存関係

```
Layer 0-1 ✅
    │
    ├─→ Layer 2 (Input) ──── 独立実装可能
    │
    ├─→ Layer 3 (Buffer/Tab)
    │       │
    │       └─→ #138 Phase A (ContextMode ワイヤリング)
    │               │
    │               └─→ #138 Phase B (ツールベース spawn)
    │                       │
    │                       └─→ #43 Knowledge Layer
    │
    ├─→ #207 Content/Route/Surface ✅ 基盤
    │       │
    │       └─→ Layer 5 Phase 2 (Lua TUI API) 🟡 WIP
    │               │
    │               └─→ Layer 5 Phase 3 (Plugin/Tools)
    │
    └─→ #43 Context Layer 強化 (#183-186) ──── 独立実装可能
```

---

## 9. 設計原則

### これまでの成功パターン

1. **直交軸分解**: 旧 `OrchestrationMode` の enum 爆発を `ConsensusLevel × PhaseScope × Strategy` に分解した成功体験を、全設計に適用する

2. **Config 4型分割**: `AgentConfig` の16フィールド一枚岩を、性質別に4型（SessionMode / ModelConfig / AgentPolicy / ExecutionParams）に分割。型シグネチャが「何を使うか」を正直に宣言する

3. **Port/Adapter パターン**: infrastructure 固有の実装（Lua, Copilot CLI）を application のポートで抽象化し、presentation からは一切見えない

4. **段階的土台構築**: #207 の「Content/Route/Surface を最小限の土台として作る → Renderer/Preset は需要が明確になってから」というアプローチ

### 守るべき制約

- **domain は外部依存ゼロ**: serde, thiserror 以外の外部クレートに依存しない
- **presentation ⊥ infrastructure**: DI は cli で解決。presentation は infrastructure を直接参照しない
- **Neovim を再実装しない**: テキスト編集は $EDITOR に委譲する。copilot-quorum はオーケストレーションに専念
- **設定のデフォルトで動く**: ユーザーが何も設定しなくても現状と同じ動作。カスタマイズは「変えたい人だけ」

---

## 10. Open Questions

### TUI Display (#207)

1. Content のライフサイクル管理（Notification は自動消滅、ToolLog は？）
2. 複数 Float の z-index / フォーカス管理
3. Pane 間の Content 移動（Vim の `:buf N` 的操作）

### Interaction (#138)

4. Ask のツール制約: read-only のみか、ツールなしか
5. ネスト時の ModelConfig 伝播: 親の config を継承 vs form ごとのデフォルト
6. Interaction の永続化: プロセス内完結 vs シリアライズ可能

### Knowledge (#43)

7. 自動学習の粒度: ノイズにならない範囲は？
8. Context Graph の永続化: セッション跨ぎで保持するか？
9. Workflow 定義フォーマット: YAML / TOML / Rust DSL / Lua？

### Extension (#58)

10. MCP 互換性: プラグインプロトコルを MCP と互換にするか独自にするか
11. プラグイン配布モデル: Git リポジトリ / レジストリ / ファイル配置
12. TOML → Lua 一本化の移行パス

---

## References

| Discussion | Title |
|---|---|
| [#58](https://github.com/music-brain88/copilot-quorum/discussions/58) | Neovim-Style Extensible TUI |
| [#207](https://github.com/music-brain88/copilot-quorum/discussions/207) | RFC: TUI Display Architecture — Content / Route / Surface 分離 |
| [#43](https://github.com/music-brain88/copilot-quorum/discussions/43) | RFC: Quorum v2 — Knowledge-Driven Architecture |
| [#138](https://github.com/music-brain88/copilot-quorum/discussions/138) | RFC: Unified Interaction Architecture — Agent/Ask/Discuss as Peer Forms |
| [#157](https://github.com/music-brain88/copilot-quorum/discussions/157) | RFC: Workflow Layer — Graph-Based Task Execution & Parallel Dispatch |

| Document | Path |
|---|---|
| Architecture Reference | [docs/reference/architecture.md](../reference/architecture.md) |
| TUI Guide | [docs/guides/tui.md](../guides/tui.md) |
| Agent System | [docs/systems/agent-system.md](../systems/agent-system.md) |
| Extension Platform | [docs/vision/extension-platform.md](extension-platform.md) |
| Knowledge Architecture | [docs/vision/knowledge-architecture.md](knowledge-architecture.md) |
| Workflow Layer | [docs/vision/workflow-layer.md](workflow-layer.md) |

<!-- LLM Context
## Summary
Consolidated architecture vision document merging 4 RFCs:
- #58: Neovim-Style TUI master roadmap (Layer 0-5)
- #207: Content/Route/Surface TUI display architecture
- #43: Knowledge/Context/Workflow 3-layer backend evolution
- #138: Unified Interaction model (Agent|Ask|Discuss as peer forms)

## Core Insight: Structural Isomorphism via 3 Questions
Interaction (Agent|Ask|Discuss) is the central axis. Backend and TUI both decompose into 3 layers
answering the SAME 3 questions (what/how/where) in different domains:
- Backend: Knowledge=what(know), Context=how(propagate), Workflow=where(execute)
- TUI: Content=what(show), Route=how(flow), Surface=where(render)
- This is NOT 1:1 data mapping — Content shows output from ALL backend layers (e.g. ToolLog=Workflow, DiscussionGraph=Context)
- All backend output flows through Interaction to all TUI layers

Music metaphors (primary), ML analogies (secondary):
- Knowledge ≈ Score (楽譜) — written before, read at start [ML: Hyperparameters]
- Context ≈ Resonance (共鳴) — emerges during play, propagates between performers [ML: Hidden State]
- Workflow ≈ Performance (演奏) — interprets score, produces sound [ML: Forward Pass]
Narrative: read the score → start performing → resonance emerges → influences the performance

Diagram notation:
- Arrow directions encode Backend↔Interaction relationships: Knowledge→inform, Context←emit, Workflow↔drive
- Dotted lines (· · ·) show structural correspondence, not data flow
- Border styles encode layer properties: ┏━━┓ permanent, ┊╌╌┊ emergent, ┌──┐ active

Key architectural decisions:
- DDD + Onion with 5 crates (domain→application→infrastructure, presentation, cli)
- Config 4-type split: SessionMode, ModelConfig, AgentPolicy, ExecutionParams
- TUI: ContentSlot → RouteTable → SurfaceId pipeline, LayoutPreset system
- Interaction: Vim buftype pattern, recursive nesting, ContextMode propagation
- Extension: Lua (mlua) in-process scripting, Phase 1-3 roadmap
- Shared state: Arc<Mutex<QuorumConfig>> + Arc<Mutex<TuiAccessorPort>>

Current status (v0.12):
- Layer 0-1 ✅, Content/Route/Surface base ✅, Tab/Pane base ✅
- Lua Phase 1+1.5 ✅, Phase 2 (TUI API) WIP
- Interaction types defined in domain (partial)
- Knowledge/Workflow layers: concept phase
-->
