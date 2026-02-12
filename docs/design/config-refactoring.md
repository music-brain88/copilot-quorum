# Config Refactoring Analysis

Discussion #58 Layer 5（Scripting Platform）の土台づくりに向けた設定系リファクタリング要素の洗い出し。

## 現状の課題マップ

### 1. quorum.example.toml と FileConfig の乖離（解消済み）

`quorum.example.toml` を `FileConfig` の全セクションに対応する形に更新済み。
以前は `[council]`, `[behavior]`, `[output]`, `[repl]`, `[agent]`（一部）, `[integrations]` のみだったが、
以下のセクションが追加された：

- `[quorum]` / `[quorum.discussion]`
- `[agent]` の role-based model 設定 + orchestration axes
- `[tui.input]`
- `[tools]` / `[tools.builtin]` / `[tools.cli]` / `[tools.mcp]` / `[tools.custom]`

### 2. 「定義済みだが未配線」な設定項目

| 設定項目 | 定義場所 | 状態 | 詳細 |
|----------|---------|------|------|
| `agent.strategy` | `file_config.rs:121` | **パース済み・未使用** | `parse_strategy()` で "quorum"/"debate" を返すが、main.rs で `OrchestrationStrategy` の切り替えに使われていない |
| `agent.interaction_type` | `file_config.rs:123` | **UIラベルのみ** | AgentConfig に設定されるが、実行フローに影響なし。プロンプト表示（`solo:ask>`）のみ |
| `agent.context_mode` | `file_config.rs:125` | **完全未使用** | AgentConfig に設定されるが、どのコードもこの値を参照して動作を変えない |
| `quorum.rule` | `file_config.rs:300` | **パース済み・未使用** | `parse_rule()` は機能するが、main.rs の実行パスで使われていない |
| `quorum.min_models` | `file_config.rs:302` | **保存のみ** | フィールドに格納されるが参照なし |
| `quorum.discussion.models` | `file_config.rs:254` | **council フォールバック** | 主に `council.models` が使われ、こちらは優先されない |
| `tools.providers` | `file_config.rs:600` | **未使用** | プロバイダ選択ロジックがない |
| `tools.suggest_enhanced_tools` | `file_config.rs:606` | **未使用** | 提案ロジックが未実装 |
| `tools.builtin.enabled` | `file_config.rs:470` | **未使用** | 常に有効 |
| `tools.cli.enabled` | `file_config.rs:386` | **未使用** | 常に有効 |
| `integrations.github.*` | `file_config.rs:219-226` | **完全未使用** | main.rs で一切参照されない |
| `tui.input.submit_key` | `file_config.rs:660` | **未使用** | TUI でハードコード |
| `tui.input.newline_key` | `file_config.rs:662` | **未使用** | TUI でハードコード |
| `tui.input.editor_key` | `file_config.rs:664` | **未使用** | TUI でハードコード |
| `tui.input.editor_action` | `file_config.rs:666` | **未使用** | TUI でハードコード |
| `tui.input.dynamic_height` | `file_config.rs:670` | **未使用** | TUI でハードコード |

### 3. Legacy `[council]` と `[quorum.discussion]` の重複

**問題**: 同じ概念（参加モデル、モデレーター）が2箇所で定義可能。

```
council.models         ←→  quorum.discussion.models
council.moderator      ←→  quorum.discussion.moderator
behavior.enable_review ←→  quorum.discussion.enable_peer_review
```

**現在の動作**: main.rs は `council.models` → `config.council.models` を参照。
`quorum.discussion` は定義されているがフォールバック先として使われていない。

---

## Layer 5 に向けたリファクタリング要素

Discussion #58 の Layer アーキテクチャ：

```
Layer 5: Scripting Platform       - Lua/Rhai, init.lua, Plugin Distribution
Layer 4: Advanced UX              - VISUAL Mode, Merge View, Pane Management
Layer 3: Buffer/Tab               - Agent/Ask/Discuss Buffers, Tab Bar
Layer 2: Input Diversification    - $EDITOR, configurable keybinds
Layer 1: Modal Foundation         (DONE)
Layer 0: TUI Infrastructure       (DONE)
```

### R1: 設定の階層構造を Domain Config に集約する

**現状**: 設定変換が main.rs に集中し、各レイヤーが独自の Config 型を持つ。

```
FileConfig (infrastructure)
    ├→ BehaviorConfig (application) — timeout のみ
    ├→ OutputConfig (presentation)  — format + color
    ├→ ReplConfig (presentation)    — progress + history
    ├→ AgentConfig (domain)         — 大部分の設定を保持
    └→ TuiInputConfig (presentation) — max_height + context_header のみ
```

**問題点**:
- `AgentConfig` が config の大部分を保持し肥大化（orchestration, model, behavior, HiL すべて）
- `BehaviorConfig` が timeout だけで貧弱
- TUI 設定の大半が `FileTuiInputConfig` → `TuiInputConfig` の変換で落ちている
- Domain 層の config モジュールが `OutputFormat` しか持たない

**提案**: Domain Config を設定の「意味」で再設計

```
domain/src/config/
├── mod.rs
├── output_format.rs       # 既存
├── quorum_config.rs       # NEW: QuorumRule + min_models（現在 orchestration 散在）
├── model_config.rs        # NEW: Role-based model selection
└── orchestration_config.rs # NEW: ConsensusLevel + PhaseScope + Strategy + InteractionType + ContextMode
```

**理由**: Layer 5 の Scripting Platform は `domain` の設定を直接操作する。
infrastructure の `FileConfig` を経由せず、domain の型を scripting API に公開する必要がある。

### R2: `[council]` → `[quorum.discussion]` への統一マイグレーション

**対応内容**:
1. `[council]` を deprecated とし、`[quorum.discussion]` を正式な設定パスにする
2. main.rs のモデル解決ロジックを `quorum.discussion.models` → `council.models` → defaults の優先順位に変更
3. マイグレーション警告の出力（`[council]` 使用時に deprecated メッセージ）

**影響範囲**:
- `cli/src/main.rs:201-212` — モデル解決ロジック
- `infrastructure/src/config/file_config.rs` — FileCouncilConfig のdeprecation注釈
- テスト更新

### R3: 未配線設定の「Wire or Remove」判断

Discussion #58 の Layer 3（Buffer/Tab）に必要な設定：

| 設定 | 判断 | 理由 |
|------|------|------|
| `interaction_type` | **Wire（Layer 3 Phase A）** | Buffer タイプの決定に使用。モード切替 → バッファ生成アクションに変更 |
| `context_mode` | **Wire（Layer 3 Phase A）** | Shared/Fresh がバッファのコンテキスト分離に直結 |
| `strategy` | **Wire** | OrchestrationStrategy::Debate の実行パス実装とセット |
| `quorum.rule` | **Wire** | Consensus Round の投票ルールとして使用 |
| `quorum.min_models` | **Wire** | Consensus 有効性チェックに使用 |
| `tools.providers` | **Wire or Remove** | provider 選択ロジック実装するか、常に全プロバイダ有効にするか |
| `tools.suggest_enhanced_tools` | **Defer** | UX 改善。Layer 5 のプラグイン提案機構にまとめてもよい |
| `integrations.github` | **Defer** | 独立機能。設定は残すが実装は後回し |
| `tui.input.*`（未使用5項目） | **Wire（Layer 2）** | Layer 2 Input Diversification で configurable keybinds を実装する際に配線 |

### R4: TUI 設定の拡張設計（Layer 2-5 対応）

**現状の TuiInputConfig**:

```rust
pub struct TuiInputConfig {
    pub max_input_height: u16,
    pub context_header: bool,
}
```

**Layer 2 で必要な拡張**:

```toml
[tui.input]
submit_key = "enter"
newline_key = "shift+enter"
editor_key = "I"
editor_action = "return_to_insert"
max_height = 10
dynamic_height = true
context_header = true
```

→ `FileTuiInputConfig` の全フィールドを `TuiInputConfig` に透過させる。

**Layer 3 で必要な追加**:

```toml
[tui.buffers]
max_agent = 1          # Agent バッファ上限
max_ask = 3            # Ask バッファ上限
max_discuss = 2        # Discuss バッファ上限
session_timeout = 1800 # LLM セッション非活性タイムアウト（秒）
```

**Layer 5 で必要な追加**:

```toml
[tui.keymap]
# Neovim-like keymap configuration
# Layer 5 では Lua/Rhai でオーバーライド可能
normal_mode = "default"   # or path to keymap file
insert_mode = "default"
command_mode = "default"

[tui.statusline]
# ステータスライン表示設定
format = "{mode} | {model} | {consensus} | {buffer}"
```

### R5: Scripting Platform 基盤としての設定ローダー拡張

**現在の設定ソース（figment）**:

```
CLI flags > project quorum.toml > XDG config.toml > defaults
```

**Layer 5 で追加が必要なソース**:

```
CLI flags > init.lua/init.rhai > project quorum.toml > XDG config.toml > defaults
```

**必要な変更**:
1. `ConfigLoader` にスクリプト設定ソースを追加するためのプラグインポイント
2. 設定のランタイム変更を可能にする（`:set` コマンド対応）
3. 設定変更のイベント通知機構（Observer パターン）

```toml
[scripting]
# Scripting engine selection (future)
engine = "lua"           # "lua" or "rhai"
init_file = "init.lua"   # Auto-loaded on startup
plugin_dirs = ["~/.config/copilot-quorum/plugins"]
```

### R6: `AgentConfig` の責務分割

**現状**: `AgentConfig` が以下をすべて保持

```
- Role-based models (exploration, decision, review)
- Orchestration axes (consensus, phase, strategy, interaction, context)
- Behavior (max_iterations, max_tool_turns, max_plan_revisions)
- HiL settings (hil_mode)
- Working dir
- Ensemble timeout
```

**提案**: 設定の関心事で分割

```rust
// domain/src/config/
pub struct ModelConfig {
    pub exploration_model: Model,
    pub decision_model: Model,
    pub review_models: Vec<Model>,
}

pub struct OrchestrationConfig {
    pub consensus_level: ConsensusLevel,
    pub phase_scope: PhaseScope,
    pub strategy: OrchestrationStrategy,
    pub interaction_type: InteractionType,
    pub context_mode: ContextMode,
}

pub struct AgentBehaviorConfig {
    pub max_iterations: usize,
    pub max_tool_turns: usize,
    pub max_plan_revisions: usize,
    pub max_tool_retries: usize,
    pub hil_mode: HilMode,
    pub require_plan_review: bool,
    pub require_final_review: bool,
    pub ensemble_session_timeout: Option<Duration>,
}

// AgentConfig は上記を組み合わせる
pub struct AgentConfig {
    pub models: ModelConfig,
    pub orchestration: OrchestrationConfig,
    pub behavior: AgentBehaviorConfig,
    pub working_dir: Option<String>,
}
```

**メリット**:
- Layer 5 のスクリプトから `orchestration.consensus_level = "ensemble"` のように直感的にアクセス
- 設定の validation を関心事ごとに分離可能
- Buffer/Tab が `OrchestrationConfig` のみを参照すればよい

---

## 優先順位と依存関係

```
Phase A（独立実装可能、高優先度）
├── R2: council → quorum.discussion 統一
├── R3: interaction_type / context_mode の Wire（Layer 3 Phase A）
└── R3: quorum.rule / min_models の Wire

Phase B（R6 が前提）
├── R6: AgentConfig 責務分割
└── R1: Domain Config 再設計

Phase C（Layer 2 実装時）
└── R4: TUI 設定の全フィールド配線

Phase D（Layer 5 実装時）
├── R4: tui.buffers / tui.keymap / tui.statusline 追加
└── R5: ConfigLoader のスクリプトソース対応
```

## 関連 Issue / Discussion

- Discussion #58: Neovim-Style Extensible TUI（Layer アーキテクチャ全体設計）
- Discussion #58 Comment 4: InteractionType Redesign
- Discussion #58 Comment 5: Layer 3 Critical Design Analysis
