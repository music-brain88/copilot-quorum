# Extension Platform / 拡張プラットフォーム

> 🟡 **Status**: Phase 1 implemented (#193) — Phase 2+ in progress
>
> Based on [Discussion #58](https://github.com/music-brain88/copilot-quorum/discussions/58) Layer 5
> and [Discussion #98](https://github.com/music-brain88/copilot-quorum/discussions/98)

---

## Overview / 概要

copilot-quorum をユーザーが **スクリプトやプラグインで拡張できるプラットフォーム** にする構想。
2 つの補完的な拡張モデル（In-Process スクリプティング + Protocol-Based 拡張）を提供する。

> **Note**: Phase 1（Lua ランタイム + Config/Keymap API）は実装済みです（#193）。
> Phase 2（TUI API: #230）、Phase 3（Plugin + Tools: #231）、TOML → Lua 一本化（#233）は計画中です。

---

## Motivation / 動機

### 競合との比較

| | Copilot CLI | OpenCode | Claude Code | **copilot-quorum** |
|---|---|---|---|---|
| UI パラダイム | 会話型 REPL | Vim TUI | 会話型 REPL | **Neovim-like modal + scripting** |
| 拡張性 | なし | キーバインド設定 | MCP サーバー | **ユーザースクリプト + プラグイン** |
| 入門コスト | 低 | 中 | 低 | **高** |
| 天井 | 低 | 中 | 中 | **高** |

### 差別化の核心

Neovim が Vim から `init.lua` で差別化したように、
copilot-quorum もスクリプト拡張で差別化する：

1. **キーマップ** — ユーザーが独自のキーバインドを定義
2. **コマンド** — ユーザーが独自の `:` コマンドを作成
3. **イベントフック** — `on_message`, `on_tool_call`, `on_phase_change` 等
4. **プラグイン** — 再利用可能なスクリプトパッケージの配布

---

## Two Extension Models / 2 つの拡張モデル

### Model 1: In-Process Scripting (Lua / mlua)

Neovim の `init.lua` と同じアプローチ。Rust プロセス内で Lua VM を動かす。

| Aspect | Detail |
|--------|--------|
| Language | **Lua**（mlua crate 経由） |
| Latency | 最低（同一プロセス） |
| Safety | Lua VM 内サンドボックス |
| Binary impact | +500KB |
| Prior art | WezTerm, Neovim |

#### Neovim との対比

**Phase 1 実装済み API** (`~/.config/copilot-quorum/init.lua`):

```lua
-- ✅ 実装済み — Phase 1 (#193)

-- キーマップ設定（ビルトインアクション or Lua コールバック）
quorum.keymap.set("normal", "Ctrl+s", "submit_input")
quorum.keymap.set("normal", "Ctrl+p", function()
    quorum.config.set("agent.strategy", "debate")
end)

-- イベントフック
-- 対応イベント: ScriptLoading, ScriptLoaded, ConfigChanged, ModeChanged, SessionStarted
quorum.on("SessionStarted", function(data)
    print("Session started in mode: " .. data.mode)
end)

quorum.on("ConfigChanged", function(data)
    print("Config changed: " .. data.key .. " = " .. data.new_value)
end)

-- 設定アクセス（関数形式 + メタテーブルショートカット）
quorum.config.get("agent.strategy")          -- 関数形式
quorum.config["agent.strategy"]              -- メタテーブル読み取り
quorum.config.set("agent.strategy", "debate")
quorum.config["agent.strategy"] = "debate"   -- メタテーブル書き込み
quorum.config.keys()                         -- 全キー一覧
```

**Phase 2+ 構想 API**:

```lua
-- ⚠️ 未実装 — 構想レベルの API イメージ

-- ユーザーコマンド定義 (Phase 3: #231)
quorum.command("review", function(args)
    quorum.ask("Review this code: " .. args.input)
end)

-- TUI レイアウト制御 (Phase 2: #230)
quorum.tui.layout.preset = "wide"
quorum.tui.input.submit_key = "ctrl+enter"

-- カスタムツール登録 (Phase 3: #231)
quorum.tools.register("my_tool", {
    command = "echo {message}",
    risk_level = "low",
    parameters = { { name = "message", required = true } }
})

-- プロバイダ設定 (Phase 3-4: #233)
quorum.providers.anthropic = {
    api_key = os.getenv("ANTHROPIC_API_KEY"),
    base_url = "https://api.anthropic.com",
}
```

| Neovim | copilot-quorum | Status | Description |
|--------|---------------|--------|-------------|
| `vim.keymap.set()` | `quorum.keymap.set()` | ✅ Phase 1 | キーマップ設定 |
| `vim.api.nvim_create_autocmd()` | `quorum.on()` | ✅ Phase 1 | イベントフック |
| `vim.opt` | `quorum.config` | ✅ Phase 1 | 設定アクセス（メタテーブル proxy） |
| `init.lua` | `~/.config/copilot-quorum/init.lua` | ✅ Phase 1 | ユーザー設定ファイル |
| `vim.api.nvim_create_user_command()` | `quorum.command()` | 🔴 Phase 3 | ユーザーコマンド定義 |

### Model 2: Protocol-Based Extension (denops-like)

denops（Vim + Deno）パターンに着想を得た、プロセス分離型の拡張モデル。

| Aspect | Detail |
|--------|--------|
| Language | **何でも OK**（Python, TypeScript, Go, Rust 等） |
| Latency | IPC オーバーヘッドあり |
| Safety | **プロセス分離**（プラグインクラッシュがホストに影響しない） |
| Protocol | JSON-RPC ベース（MCP 互換を検討中） |
| Prior art | denops (Vim + Deno), LSP |

#### プラグインホストモデルの選択肢

```
Option A: 各プラグインが独立プロセス
  copilot-quorum ←→ plugin-a (Python)
                 ←→ plugin-b (TypeScript)

Option B: 共通ランタイムがプラグインをホスト (denops 型)
  copilot-quorum ←→ plugin-host (Deno) ←→ plugin-a.ts
                                       ←→ plugin-b.ts

Option C: ハイブリッド
  copilot-quorum ←→ plugin-host (Deno) ←→ TS plugins
                 ←→ standalone-plugin (Rust binary)
```

### In-Process vs Protocol-Based の比較

| | In-Process (mlua) | Protocol-Based |
|---|---|---|
| レイテンシ | 最低（同一プロセス） | IPC オーバーヘッドあり |
| 言語 | Lua のみ | **何でも OK** |
| サンドボックス | Lua VM 内 | **プロセス分離（安全）** |
| コミュニティ参入障壁 | Lua を書ける人 | **何語でも書ける** |
| 先行事例 | WezTerm, Neovim | denops, LSP |

**結論**: 2 つは補完し合う可能性がある。In-Process は高頻度・低レイテンシの拡張（キーマップ、イベントフック）、
Protocol-Based はヘビーな拡張（カスタム LLM プロバイダ、外部ツール統合）に適する。

---

## MCP (Model Context Protocol) との関係

AI ツール界隈で MCP が JSON-RPC ベースのプロトコルとして普及しつつある。
copilot-quorum の拡張プロトコルとの関係は未決定：

| Option | Pros | Cons |
|--------|------|------|
| **A: MCP を拡張プロトコルとして採用** | 既存 MCP サーバーがそのまま使える | TUI 拡張には不向きな面も |
| **B: 独自プロトコル + MCP ブリッジ** | TUI 拡張に最適化された API 設計 | プロトコル設計・維持コスト |
| **C: MCP スーパーセット** | 互換維持しつつ TUI 拡張機能を追加 | MCP の進化に追従するコスト |

---

## ScriptingEngine Port / ScriptingEngine ポート

```rust
// ✅ 実装済み — application/src/ports/scripting_engine.rs

pub trait ScriptingEnginePort: Send + Sync {
    fn emit_event(&self, event: ScriptEventType, data: ScriptEventData)
        -> Result<EventOutcome, ScriptError>;
    fn load_script(&self, path: &Path) -> Result<(), ScriptError>;
    fn is_available(&self) -> bool;
    fn registered_keymaps(&self) -> Vec<(String, String, KeymapAction)>;
    fn execute_callback(&self, callback_id: u64) -> Result<(), ScriptError>;
}
```

WezTerm パターンでモジュラー API 実装（`infrastructure/src/scripting/`）：

| モジュール | 状態 | 内容 |
|-----------|------|------|
| `lua_engine.rs` | ✅ 実装済み | メイン Lua 5.4 エンジン（mlua） |
| `config_api.rs` | ✅ 実装済み | `quorum.config` API（メタテーブル proxy） |
| `keymap_api.rs` | ✅ 実装済み | `quorum.keymap` API（string-based key descriptors） |
| `event_bus.rs` | ✅ 実装済み | イベント登録・発火 |
| `sandbox.rs` | ✅ 実装済み | C モジュールブロック |
| `tui_api.rs` | 🔴 Phase 2 | `quorum.tui.*` API |
| `tools_api.rs` | 🔴 Phase 3 | `quorum.tools.*` API |
| `command_api.rs` | 🔴 Phase 3 | `quorum.command()` API |

---

## copilot-quorum 固有の考慮事項

Protocol-Based 拡張で検討が必要な copilot-quorum 固有の機能：

| Capability | Description |
|------------|-------------|
| **LLM セッション管理** | プラグインが独自の LLM セッションを開ける？コスト管理は？ |
| **ツール登録** | プラグインが新しい `ToolDefinition` を登録する API |
| **バッファ操作** | プラグインが会話バッファの作成・読み取りを行える API |
| **イベントフック** | `on_message`, `on_tool_call`, `on_phase_change` 等 |

---

## Prerequisites & Roadmap / 前提条件・ロードマップ

```
Phase 1: Lua Runtime + Config/Keymap API (#193)  ── ✅ Done
  └─ quorum.on(), quorum.config, quorum.keymap.set()

Phase 1.5: ConfigAccessorPort 拡張 (#233 Step 2)  ── ✅ Done
  └─ 全 20 キー mutable 化 (models, output, repl, context_budget)

Phase 2: TUI Route/Layout API (#230)               ── 🔴 Planned
  └─ quorum.tui.* で TUI セクション全体を Lua 化

Phase 3: Plugin + Tools API (#231)                  ── 🔴 Planned
  └─ quorum.tools.*, quorum.command()

TOML → Lua 一本化 (#233)                            ── ✅ 完了
  └─ quorum.toml 撤去済み（設定は init.lua のみ）
```

---

## Scripting Language Comparison / スクリプト言語の比較

| | Lua (mlua) | Rhai | JS (deno_core) | WASM |
|---|---|---|---|---|
| Binary impact | +500KB | +200KB | +30-50MB | +5-10MB |
| Ecosystem | 巨大 | 小 | 巨大 | 言語依存 |
| Async support | mlua で可能 | 不可 | ネイティブ | ホスト経由 |
| Sandbox | 良 | 優秀 | 良 | 最高 |
| Neovim 親和性 | **最高** | 低 | 中 | 低 |
| Prior art | WezTerm, Neovim | — | Deno | Zed |

**推奨**: Lua (mlua) — Neovim ユーザーとの親和性が最高。WezTerm が Rust + Lua 統合を実証済み。

---

## Open Questions / 未解決の論点

1. ~~**拡張モデル**: In-Process (mlua) vs Protocol-Based vs ハイブリッド~~ → **Phase 1 で In-Process (Lua/mlua) を採用**
2. **MCP 互換性**: プラグインプロトコルを MCP と互換にするか独自にするか
3. ~~**スクリプト言語**: Lua vs Rhai vs 他~~ → **Lua (mlua) に決定**
4. **プラグイン配布**: Git リポジトリ / レジストリ / ファイル配置
5. **API 安定性**: セマンティックバージョニング？Capability negotiation？
6. **プラグインのライフサイクル**: 起動/停止/再起動の管理
7. **プラグイン間通信**: 許可するか？
8. **パフォーマンスバジェット**: IPC レイテンシの許容範囲

---

## Related

- [#193](https://github.com/music-brain88/copilot-quorum/issues/193): Phase 1 — Lua Config Adapter (✅ Done)
- [#230](https://github.com/music-brain88/copilot-quorum/issues/230): Phase 2 — TUI Route/Layout API
- [#231](https://github.com/music-brain88/copilot-quorum/issues/231): Phase 3 — Plugin + Tools API
- [#233](https://github.com/music-brain88/copilot-quorum/issues/233): TOML → Lua 一本化ロードマップ
- [Discussion #58](https://github.com/music-brain88/copilot-quorum/discussions/58): Neovim-Style Extensible TUI（Layer 5 が本構想に対応）
- [Discussion #98](https://github.com/music-brain88/copilot-quorum/discussions/98): Protocol-Based Extension Architecture — 詳細設計
- [knowledge-architecture.md](knowledge-architecture.md): Knowledge Layer 構想
- [workflow-layer.md](workflow-layer.md): Workflow Layer 設計
- [Discussion #43](https://github.com/music-brain88/copilot-quorum/discussions/43): Knowledge-Driven Architecture — 3 層構想の全体像
