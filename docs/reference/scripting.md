# Scripting System / スクリプティングシステム

> Lua-based extensibility for configuration, keybindings, plugins, commands, and agent event hooks
>
> 設定・キーバインド・プラグイン・コマンド・エージェントイベントフックのための Lua 拡張基盤

---

## Overview / 概要

スクリプティングシステムは、copilot-quorum を Lua スクリプトでカスタマイズ可能にする拡張基盤です。
Vim/Neovim のプラグインエコシステムに倣い、段階的に構築されています。

**3 フェーズ構成**:
1. **Phase 1**: `init.lua` + Config/Keymap API（基本設定）
2. **Phase 2**: TUI Route/Layout/Content API（UI カスタマイズ）
3. **Phase 3**: Plugin System + Agent Events + User Commands（拡張性）

---

## Quick Start / クイックスタート

```lua
-- ~/.config/copilot-quorum/init.lua

-- 設定変更
quorum.config.set("agent.hil_mode", "auto_approve")
quorum.config["models.exploration"] = "gpt-5.2-codex"

-- キーバインド
quorum.keymap.set("normal", "q", "quit")
quorum.keymap.set("normal", "r", function()
    quorum.config.set("agent.hil_mode", "interactive")
end)

-- イベント購読
quorum.on("PhaseChanged", function(data)
    print("Phase: " .. data.phase)
end)

-- ツール実行の制御（false でキャンセル）
quorum.on("ToolCallBefore", function(data)
    if data.tool_name == "run_command" then
        return false  -- コマンド実行をブロック
    end
    return true
end)

-- ユーザー定義コマンド
quorum.command.register("greet", {
    fn = function(args)
        print("Hello, " .. args .. "!")
    end,
    description = "Greet someone",
    usage = "/greet <name>"
})
```

---

## Architecture / アーキテクチャ

```
┌─────────────────────────────────────────────────────────┐
│                    CLI (main.rs)                         │
│  init.lua ロード → plugins/*.lua ロード → DI 組み立て     │
└────────────────────────┬────────────────────────────────┘
                         │ Arc<dyn ScriptingEnginePort>
                         ▼
┌─────────────────────────────────────────────────────────┐
│              Application Layer (Ports)                    │
│                                                          │
│  ScriptingEnginePort        AgentProgressNotifier        │
│  ├── emit_event()           ├── on_phase_change()        │
│  ├── on_tool_call_before()  ├── on_plan_created()        │
│  ├── registered_commands()  ├── on_tool_execution_*()    │
│  └── execute_command_cb()   └── ...                      │
│                                                          │
│  CompositeProgressNotifier<'a>   ScriptProgressBridge    │
│  └── [TuiProgress, ScriptBridge] └── progress → events   │
└────────────────────────┬────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│           Infrastructure Layer (Lua Runtime)              │
│                                                          │
│  LuaScriptingEngine (mlua)                               │
│  ├── EventBus      (quorum.on)                           │
│  ├── ConfigAPI     (quorum.config)                       │
│  ├── KeymapAPI     (quorum.keymap)                       │
│  ├── CommandAPI    (quorum.command)                       │
│  └── Sandbox       (C module blocking)                   │
└─────────────────────────────────────────────────────────┘
```

---

## Plugin System

### ロード順序

```
~/.config/copilot-quorum/
├── init.lua              ← 最初にロード（グローバル設定）
└── plugins/
    ├── 01_core.lua       ← アルファベット順にロード
    ├── 02_lsp.lua
    └── 99_experimental.lua
```

- Vim native packages 方式（アルファベット順、数字プレフィックスで順序制御）
- ディレクトリ不在は silent skip
- 個別ファイルの失敗は `eprintln!` Warning で続行（他プラグインをブロックしない）
- `load_script()` を再利用（新トレイトメソッド不要）

---

## User Commands / ユーザー定義コマンド

```lua
quorum.command.register("deploy", {
    fn = function(args)
        print("Deploying to: " .. args)
    end,
    description = "Deploy to environment",
    usage = "/deploy <env>"
})
```

### 設計

- `CommandRegistry` が名前 → callback_id マッピングを管理
- `AgentController.handle_command()` で未知コマンド → Lua コマンド検索 → 見つかれば実行
- 同名の再登録は last-write-wins（上書き）
- 名前の先頭 `/` は自動ストリップ

---

## Agent Events / エージェントイベント

### イベント一覧

| Event | Cancellable | Data Fields |
|-------|-------------|-------------|
| `ToolCallBefore` | **Yes** | tool_name, args (JSON) |
| `ToolCallAfter` | No | tool_name, success, duration_ms, output_preview/error |
| `PhaseChanged` | No | phase (string) |
| `PlanCreated` | No | objective, task_count |

### ToolCallBefore キャンセルフロー

```
ToolCall 受信
  → ScriptingEnginePort::on_tool_call_before(tool_name, args_json)
    → false: ToolResultMessage { is_rejected: true } で即返却、HiL スキップ
    → true: 通常フロー継続
      → Low-risk: parallel execute → ToolCallAfter (via ScriptProgressBridge)
      → High-risk: HiL review → execute → ToolCallAfter
```

**設計根拠**: Vim の `BufWritePre`（実行前に割り込むタイミング）と `BufWriteCmd`（本体処理をキャンセルできる能力）のハイブリッド。Lua フィルタ先 → HiL 後方式により、プログラマティックポリシーレイヤーとして機能する。

### CompositeProgressNotifier

```
RunAgentUseCase.execute_with_progress(input, &composite_progress)
                                                |
                    +---------------------------+---------------------------+
                    |                                                       |
        TuiProgressBridge (既存)                        ScriptProgressBridge (新規)
        → TuiEvent channel                              → ScriptingEnginePort::emit_event()
```

- `CompositeProgressNotifier<'a>` は借用参照ベース（`Vec<&'a dyn AgentProgressNotifier>`）
- `ScriptProgressBridge` が `AgentProgressNotifier` コールバックを `ScriptEventType` に変換
- `ToolCallBefore` は戻り値が必要なため、`ScriptProgressBridge` 経由ではなく `ExecuteTaskUseCase` が直接呼び出し

---

## Event Reference / 全イベント一覧

| Event | Phase | Cancellable | Description |
|-------|-------|-------------|-------------|
| `ScriptLoading` | 1 | No | スクリプトロード開始 |
| `ScriptLoaded` | 1 | No | スクリプトロード完了 |
| `ConfigChanged` | 1 | No | 設定変更 |
| `ModeChanged` | 1 | No | モード変更 |
| `SessionStarted` | 1 | No | セッション開始 |
| `RouteChanged` | 2 | No | ルート変更 |
| `ToolCallBefore` | 3 | **Yes** | ツール実行前（false でキャンセル） |
| `ToolCallAfter` | 3 | No | ツール実行後 |
| `PhaseChanged` | 3 | No | エージェントフェーズ変更 |
| `PlanCreated` | 3 | No | プラン作成 |
| `ContentRegistered` | 2 | No | コンテンツ登録 |

---

## Sandbox / サンドボックス

- `package.loadlib = nil` — C モジュール読み込みをブロック
- `package.cpath = ""` — C パス検索を無効化
- 標準 Lua ライブラリ（string, table, math, io, os）は利用可能
- `require()` は Lua モジュールのみ対応

---

## Key Files / 主要ファイル

| File | Description |
|------|-------------|
| `domain/src/scripting/mod.rs` | ScriptEventType (11 variants), ScriptEventData, ScriptValue |
| `application/src/ports/scripting_engine.rs` | ScriptingEnginePort trait, NoScriptingEngine |
| `application/src/ports/composite_progress.rs` | CompositeProgressNotifier<'a> |
| `application/src/ports/script_progress_bridge.rs` | ScriptProgressBridge |
| `infrastructure/src/scripting/lua_engine.rs` | LuaScriptingEngine (mlua) |
| `infrastructure/src/scripting/event_bus.rs` | EventBus (event → callback dispatch) |
| `infrastructure/src/scripting/config_api.rs` | ConfigAPI (quorum.config) |
| `infrastructure/src/scripting/keymap_api.rs` | KeymapAPI (quorum.keymap) |
| `infrastructure/src/scripting/command_api.rs` | CommandAPI (quorum.command) |
| `infrastructure/src/scripting/sandbox.rs` | Sandbox (C module blocking) |
| `cli/src/main.rs` | DI wiring, init.lua + plugins/ loading |

<!-- LLM Context: Scripting system Phase 1-3. Events: 11 types (ScriptLoading, ScriptLoaded, ConfigChanged, ModeChanged, SessionStarted, RouteChanged, ToolCallBefore, ToolCallAfter, PhaseChanged, PlanCreated, ContentRegistered). ToolCallBefore is cancellable. Plugin loading: alphabetical order in ~/.config/copilot-quorum/plugins/. Commands: quorum.command.register(name, opts). CompositeProgressNotifier delegates to TUI + ScriptProgressBridge. -->
