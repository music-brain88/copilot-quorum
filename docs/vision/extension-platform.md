# Extension Platform / 拡張プラットフォーム

> 🔴 **Status**: Not implemented — Concept phase
>
> Based on [Discussion #58](https://github.com/music-brain88/copilot-quorum/discussions/58) Layer 5
> and [Discussion #98](https://github.com/music-brain88/copilot-quorum/discussions/98)

---

## Overview / 概要

copilot-quorum をユーザーが **スクリプトやプラグインで拡張できるプラットフォーム** にする構想。
2 つの補完的な拡張モデル（In-Process スクリプティング + Protocol-Based 拡張）を検討中。

> **Note**: これは将来ビジョンであり、現時点では構想段階です。
> Layer 2（Input Diversification）と Layer 3（Buffer/Tab System）の実装が先決条件です。

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

#### Neovim との対比（構想）

```lua
-- ⚠️ 未実装 — 構想レベルの API イメージ

-- キーマップ設定
quorum.keymap.set("normal", "s", ":solo<CR>")
quorum.keymap.set("normal", "e", ":ens<CR>")

-- ユーザーコマンド定義
quorum.command("review", function(args)
  quorum.ask("Review this code: " .. args.input)
end)

-- イベントフック
quorum.on("tool_call", function(event)
  if event.tool == "write_file" then
    quorum.notify("Writing to " .. event.args.path)
  end
end)

-- 設定アクセス
quorum.config.set("agent.hil_mode", "interactive")
```

| Neovim | copilot-quorum (構想) | Description |
|--------|----------------------|-------------|
| `vim.keymap.set()` | `quorum.keymap.set()` | キーマップ設定 |
| `vim.api.nvim_create_user_command()` | `quorum.command()` | ユーザーコマンド定義 |
| `vim.api.nvim_create_autocmd()` | `quorum.on()` | イベントフック |
| `vim.opt` | `quorum.config` | 設定アクセス |
| `init.lua` | `init.lua` (or `quorum.lua`) | ユーザー設定ファイル |

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

## ScriptingEngine Port / ScriptingEngine ポート（設計案）

```rust
// ⚠️ 未実装 — 設計案
// application 層

/// Port for scripting engine integration
#[async_trait]
pub trait ScriptingEngine: Send + Sync {
    fn load_config(&mut self, path: &Path) -> Result<(), ScriptError>;
    fn get_keymaps(&self, mode: &InputMode) -> Vec<KeyMapping>;
    fn get_commands(&self) -> Vec<UserCommand>;
    async fn emit_event(&self, event: ReplEvent) -> Result<(), ScriptError>;
}
```

WezTerm パターンでモジュラー API 実装：
`api_quorum.rs`, `api_keymap.rs`, `api_command.rs`, `api_buffer.rs`, `api_event.rs`

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

## Prerequisites / 前提条件

Extension Platform は以下の実装が先決条件：

```
Layer 2: Input Diversification     ── 🟡 In progress
  └─ $EDITOR 委譲、追加キーバインド

Layer 3: Buffer/Tab System         ── 🟡 In progress
  └─ Buffer API がスクリプティング API の前提

Extension Platform (Layer 5)       ── 🔴 Concept
  └─ Buffer API + キーバインド基盤の上に構築
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

1. **拡張モデル**: In-Process (mlua) vs Protocol-Based vs ハイブリッド — どの組み合わせで始める？
2. **MCP 互換性**: プラグインプロトコルを MCP と互換にするか独自にするか
3. **スクリプト言語**: Lua vs Rhai vs 他（バイナリサイズ vs エコシステム）
4. **プラグイン配布**: Git リポジトリ / レジストリ / ファイル配置
5. **API 安定性**: セマンティックバージョニング？Capability negotiation？
6. **プラグインのライフサイクル**: 起動/停止/再起動の管理
7. **プラグイン間通信**: 許可するか？
8. **パフォーマンスバジェット**: IPC レイテンシの許容範囲

---

## Related

- [Discussion #58](https://github.com/music-brain88/copilot-quorum/discussions/58): Neovim-Style Extensible TUI（Layer 5 が本構想に対応）
- [Discussion #98](https://github.com/music-brain88/copilot-quorum/discussions/98): Protocol-Based Extension Architecture — 詳細設計
- [knowledge-architecture.md](knowledge-architecture.md): Knowledge Layer 構想
- [workflow-layer.md](workflow-layer.md): Workflow Layer 設計
- [Discussion #43](https://github.com/music-brain88/copilot-quorum/discussions/43): Knowledge-Driven Architecture — 3 層構想の全体像
