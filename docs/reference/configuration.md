# Configuration Reference / 設定リファレンス

> Canonical reference for all configuration keys and the Lua configuration API
>
> 全設定キーと Lua 設定 API の正規リファレンス

---

## Overview / 概要

copilot-quorum の設定は **Lua スクリプト**（`~/.config/copilot-quorum/init.lua` +
`~/.config/copilot-quorum/plugins/*.lua`）で管理されます。
旧 TOML（`quorum.toml`）ベースの設定基盤は撤去済みです。

> **Note**: Lua スクリプティングは `scripting` feature（デフォルト ON）で提供されます。
> `init.lua` が存在しない場合はサイレントにスキップされます（エラーにはなりません）。

リポジトリ直下の [`quorum.example.lua`](../../quorum.example.lua) が全設定のテンプレートです。

### Boot Sequence / 起動時の設定解決順序

```
Rust defaults  →  init.lua  →  plugins/*.lua (アルファベット順)  →  CLI フラグ
```

後段が前段を上書きします。CLI フラグ（`--ensemble` 等）が最優先です。
設定全体は `QuorumConfig`（4型コンテナ: SessionMode / ModelConfig / AgentPolicy / ExecutionParams）で管理され、
`AgentController` と `LuaScriptingEngine` が `Arc<Mutex<QuorumConfig>>` を共有するため、
Lua からの変更は runtime でエージェントに伝播します。

---

## Configuration Keys / 設定キー一覧

`quorum.config.set(key, value)` / `quorum.config.get(key)` で読み書きできる全 29 キー。
すべて runtime で変更可能です。

### `agent.*` — エージェント動作

| キー | 型 | 値 | デフォルト |
|------|-----|-----|-----------|
| `agent.consensus_level` | String | `"solo"`, `"ensemble"` | `"solo"` |
| `agent.phase_scope` | String | `"full"`, `"fast"`, `"plan-only"` | `"full"` |
| `agent.strategy` | String | `"quorum"`, `"debate"` | `"quorum"` |
| `agent.hil_mode` | String | `"interactive"`, `"auto_reject"`, `"auto_approve"` | `"interactive"` |
| `agent.max_plan_revisions` | Integer | 人間介入までの最大計画修正回数 | `3` |

3 軸（consensus_level / phase_scope / strategy）の意味と組み合わせ制約は
[Orchestration Axes](../explanation/orchestration-axes.md) を参照してください。

`agent.hil_mode` は Plan Review の人間介入だけでなく、`agent.strategy = "debate"`
（Debate 戦略）のエスカレーションにも効きます。moderator が critical/major な
反論が未解決のまま settle しようとするたびに（最終ラウンドに限らず、途中の
settle チェックポイントでも）このモードでゲートされます: `auto_reject` は
最終ラウンドでは討議を中止しますが、途中ラウンドでは settle を却下して
討議を続行します（討議自体を止めるのではなく、未解決のまま決着させないための
fail-secure）。`auto_approve` は未解決のまま強制的に settle し、`interactive`
は `HumanInterventionPort::request_debate_escalation` 経由で都度ユーザーに
判断を委ねます。

### `models.*` — ロール別モデル設定

| キー | 型 | 用途 |
|------|-----|------|
| `models.exploration` | String | コンテキスト収集 + 低リスクツール実行（高速・低コスト） |
| `models.decision` | String | 計画作成 + 高リスクツール判断 |
| `models.review` | StringList | Quorum レビュー（Plan / Action Review） |
| `models.participants` | StringList | Quorum Discussion / Ensemble 計画生成の参加モデル |
| `models.moderator` | String | Quorum Synthesis（Phase 3 統合役） |
| `models.ask` | String | Ask（Q&A）インタラクション |

ロール分割の設計経緯は [ADR 0004](../explanation/design-decisions/0004-role-based-model-configuration.md) を参照。

### `execution.*` — 実行ループ制御

| キー | 型 | 説明 | デフォルト |
|------|-----|------|-----------|
| `execution.max_iterations` | Integer | 最大計画イテレーション数 | `20` |
| `execution.max_tool_turns` | Integer | タスクあたり最大ツールターン数 | `10` |

### `output.*` — 出力

| キー | 型 | 値 | デフォルト |
|------|-----|-----|-----------|
| `output.format` | String | `"full"`, `"synthesis"`, `"json"` | `"synthesis"` |
| `output.color` | Boolean | カラー出力の有効化 | `true` |

### `repl.*` — REPL

| キー | 型 | 説明 | デフォルト |
|------|-----|------|-----------|
| `repl.show_progress` | Boolean | プログレス表示 | `true` |
| `repl.history_file` | String | 履歴ファイルパス | `~/.local/share/copilot-quorum/history.txt` |

### `context_budget.*` — コンテキスト予算

タスク実行間で保持する結果コンテキストの量を制御し、プロンプト肥大化を防ぎます。

| キー | 型 | 説明 | デフォルト |
|------|-----|------|-----------|
| `context_budget.max_entry_bytes` | Integer | 単一タスク結果の最大バイト数 | `20000` |
| `context_budget.max_total_bytes` | Integer | 全過去結果の合計最大バイト数 | `60000` |
| `context_budget.recent_full_count` | Integer | 完全保持する直近結果数 | `3` |

### `tui.input.*` — TUI 入力

| キー | 型 | 説明 | デフォルト |
|------|-----|------|-----------|
| `tui.input.submit_key` | String | 送信キー | `"enter"` |
| `tui.input.newline_key` | String | 改行挿入キー（マルチライン入力） | `"shift+enter"` |
| `tui.input.editor_key` | String | $EDITOR 起動キー（NORMAL モード） | `"I"` |
| `tui.input.editor_action` | String | $EDITOR 保存後の動作: `"return_to_insert"`, `"submit"` | `"return_to_insert"` |
| `tui.input.max_height` | Integer | 入力エリアの最大行数 | `10` |
| `tui.input.dynamic_height` | Boolean | 入力内容に応じた動的リサイズ | `true` |
| `tui.input.context_header` | Boolean | $EDITOR 起動時のコンテキストヘッダー表示 | `true` |

### `tui.layout.*` — TUI レイアウト

| キー | 型 | 説明 | デフォルト |
|------|-----|------|-----------|
| `tui.layout.preset` | String | `"default"`, `"minimal"`, `"wide"`, `"stacked"` | `"default"` |
| `tui.layout.flex_threshold` | Integer | Minimal フォールバックの端末幅閾値（0 で無効） | `120` |

| Preset | Layout |
|--------|--------|
| `default` | 70/30 横分割（conversation + sidebar） |
| `minimal` | 全幅 conversation、sidebar なし |
| `wide` | 60/20/20 三分割（conversation + progress + tools） |
| `stacked` | 70/30 縦分割（conversation 上、progress 下） |

### `supervisor.*` — 現地司令塔の状態自己申告（#309）

| キー | 型 | 説明 | デフォルト |
|------|-----|------|-----------|
| `supervisor.reporter` | String | `"auto"`（herdr 環境検出時のみ有効化）, `"none"`（常に無効） | `"auto"` |

working/blocked/idle を上位コックピット（herdr 等）へ自己申告する仕組み。`auto` でも
`HERDR_ENV` / `HERDR_PANE_ID` / `HERDR_SOCKET_PATH` が揃っていなければ完全 no-op
（スレッドもソケットも作られない）。詳細は
[architecture.md の Supervisor Reporting](architecture.md#supervisor-reporting-309) を参照。

---

## Lua Configuration API / Lua 設定 API

### `quorum.config` — 設定アクセス

```lua
-- 関数形式
quorum.config.set("agent.consensus_level", "ensemble")
local level = quorum.config.get("agent.consensus_level")

-- メタテーブルショートカット（読み書き両対応）
local strategy = quorum.config["agent.strategy"]     -- 読み取り
quorum.config["agent.strategy"] = "debate"           -- 書き込み

-- 全キー一覧を取得
local keys = quorum.config.keys()
```

### `quorum.providers` — プロバイダー設定

デフォルトでは全モデルが Copilot CLI バックエンドにルーティングされます。

```lua
quorum.providers.set_default("copilot")

-- モデル → プロバイダーの明示ルーティング ("copilot", "anthropic", "openai", "bedrock", "azure")
quorum.providers.route("claude-sonnet-4.5", "bedrock")

-- AWS Bedrock（`bedrock` feature 必須: cargo build --features bedrock。IAM 認証）
quorum.providers.bedrock({ region = "us-east-1", profile = "default", max_tokens = 8192, cross_region = false })

-- Direct API（API キー必須）
quorum.providers.anthropic({ api_key = os.getenv("ANTHROPIC_API_KEY") })
quorum.providers.openai({ api_key = os.getenv("OPENAI_API_KEY") })
```

### `quorum.tools.register` — カスタムツール登録

外部 CLI コマンドをツールとして登録します。パラメータ値はシェルエスケープされ、
コマンドインジェクションを防ぎます。`risk_level` のデフォルトは `"high"`（安全側）です。

```lua
quorum.tools.register("my_tool", {
    description = "My custom tool",
    command = "echo {input}",
    risk_level = "high",     -- "low" or "high"
    parameters = {
        input = { type = "string", description = "Input text", required = true },
    }
})
```

手順とサンプルは [How to Add Custom Tools](../how-to/add-custom-tools.md) を参照。

### `quorum.keymap.set` — キーバインド

```lua
-- ビルトインアクションにマッピング
quorum.keymap.set("normal", "Ctrl+s", "submit_input")

-- Lua コールバックにバインド
quorum.keymap.set("normal", "Ctrl+d", function()
    quorum.config.set("agent.strategy", "debate")
end)
```

| モード | 説明 |
|--------|------|
| `"normal"` | Normal モード（Vim-like） |
| `"insert"` | Insert モード（テキスト入力） |
| `"command"` | Command モード（`:` コマンド） |

終了系のビルトインアクションは 2 種類ある:

| アクション | 相当コマンド | 挙動 |
|-----------|-------------|------|
| `"close_tab_or_quit"` | `:q` | 複数タブ時は現在のタブを閉じる（実行中エージェントはキャンセル）。最後の 1 枚でアプリを終了 |
| `"quit"` | `:qa` | タブ数に関係なくアプリ全体を即終了 |

### `quorum.command.register` — ユーザー定義コマンド

```lua
quorum.command.register("hello", {
    description = "Say hello",
    fn = function(args) print("Hello, " .. (args or "world") .. "!") end,
})
```

### `quorum.on` — イベントフック

```lua
quorum.on("SessionStarted", function(data)
    quorum.config["agent.consensus_level"] = "ensemble"
end)

quorum.on("ConfigChanged", function(data)
    -- data.key, data.old_value, data.new_value が参照可能
end)

quorum.on("ToolCallBefore", function(event)
    return true  -- false を返すとツール実行をキャンセル
end)
```

全イベント一覧は [Scripting Reference](./scripting.md) を参照。

### `quorum.tui.*` — TUI ルート/レイアウト/コンテンツ

```lua
-- Content → Surface のルーティング
quorum.tui.routes.set("progress", "main_pane")
quorum.tui.routes.get("progress")            --> "sidebar"
quorum.tui.routes.list()

-- レイアウト操作
quorum.tui.layout.current()                  --> "default"
quorum.tui.layout.switch("wide")
quorum.tui.layout.register_preset("my_layout", { direction = "horizontal", ... })
quorum.tui.layout.presets()

-- カスタムコンテンツスロット
quorum.tui.content.register("my_panel")
quorum.tui.content.set_text("my_panel", "Hello from Lua!")
quorum.tui.content.slots()
```

### Sandbox / サンドボックス

セキュリティのため、以下の制限が適用されます：

- C モジュールのロードをブロック（`package.loadlib = nil`, `package.cpath = ""`）
- 標準 Lua ライブラリ（`io`, `os`, `string`, `table` 等）は利用可能
- `os.getenv()` で環境変数を参照可能

---

## Plugin System / プラグインシステム

`~/.config/copilot-quorum/plugins/*.lua` は init.lua の後にアルファベット順で自動ロードされます。

- 番号プレフィックスで順序制御: `01_core.lua`, `02_lsp.lua`（Vim native packages スタイル）
- ディレクトリが存在しない場合: サイレントスキップ
- 個別ファイルの失敗: `eprintln!` 警告を出して続行

---

## Key Files / 主要ファイル

| File | Description |
|------|-------------|
| `quorum.example.lua` | 全設定のテンプレート（リポジトリ直下） |
| `application/src/config/quorum_config.rs` | `QuorumConfig`（4型コンテナ）+ 全キーの get/set 実装 |
| `application/src/ports/config_accessor.rs` | `ConfigAccessorPort` trait |
| `infrastructure/src/scripting/lua_engine.rs` | `LuaScriptingEngine`（mlua, Lua 5.4） |
| `infrastructure/src/scripting/config_api.rs` | `quorum.config` API |
| `infrastructure/src/scripting/providers_api.rs` | `quorum.providers` API |
| `infrastructure/src/scripting/tools_api.rs` | `quorum.tools` API |
| `infrastructure/src/scripting/keymap_api.rs` | `quorum.keymap` API |
| `infrastructure/src/scripting/command_api.rs` | `quorum.command` API |
| `infrastructure/src/scripting/tui_api.rs` | `quorum.tui` API |
| `infrastructure/src/scripting/sandbox.rs` | サンドボックス |
| `cli/src/main.rs` | DI 構築（defaults → Lua → CLI フラグの解決） |

---

## Related / 関連

- [CLI Reference](./cli.md) - CLI フラグと REPL コマンド
- [Orchestration Axes](../explanation/orchestration-axes.md) - 3 直交軸の意味と組み合わせ制約
- [Scripting Reference](./scripting.md) - イベント一覧・プラグインアーキテクチャ
- [How to Write Lua Plugins](../how-to/write-lua-plugins.md) - プラグイン作成手順
- [Tutorial: Customizing with Lua](../tutorials/customizing-with-lua.md) - 入門チュートリアル

<!-- LLM Context: 設定は Lua (init.lua + plugins/*.lua) のみ。TOML (quorum.toml) 基盤は撤去済み。Boot: Rust defaults → init.lua → plugins → CLI flags。全30キー runtime 変更可能: agent.*(5), models.*(6), execution.*(2), output.*(2), repl.*(2), context_budget.*(3), tui.input.*(7), tui.layout.*(2), supervisor.*(1)。QuorumConfig(application/src/config/quorum_config.rs) が4型コンテナ(SessionMode/ModelConfig/AgentPolicy/ExecutionParams)。AgentController と LuaScriptingEngine が Arc<Mutex<QuorumConfig>> を共有し runtime 伝播。Lua API: quorum.config/{get,set,keys}+metatable proxy, quorum.providers.{set_default,route,bedrock,anthropic,openai}, quorum.tools.register, quorum.keymap.set, quorum.command.register, quorum.on, quorum.tui.{routes,layout,content}。 -->
