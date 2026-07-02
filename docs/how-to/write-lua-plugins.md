# How to Write Lua Plugins / Lua プラグインを書く

> Customize copilot-quorum with init.lua, plugins, user commands and event hooks
>
> init.lua・プラグイン・ユーザーコマンド・イベントフックで copilot-quorum をカスタマイズする

---

## Start with init.lua / init.lua から始める

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
```

リポジトリ直下の `quorum.example.lua` が全 API のテンプレートです。

---

## Split into plugins / プラグインに分割する

`~/.config/copilot-quorum/plugins/*.lua` は init.lua の後にアルファベット順で自動ロードされます。
番号プレフィックスで順序を制御できます（Vim native packages 方式）:

```
~/.config/copilot-quorum/
├── init.lua              ← 最初にロード（グローバル設定）
└── plugins/
    ├── 01_core.lua       ← アルファベット順にロード
    ├── 02_lsp.lua
    └── 99_experimental.lua
```

個別ファイルの失敗は Warning を出して続行するため、1 つのプラグインの
エラーが他をブロックすることはありません。

---

## Register a user command / ユーザーコマンドを登録する

```lua
quorum.command.register("deploy", {
    fn = function(args)
        print("Deploying to: " .. args)
    end,
    description = "Deploy to environment",
    usage = "/deploy <env>"
})
```

登録したコマンドは REPL / TUI で `/deploy staging` のように呼び出せます。
名前の先頭 `/` は自動でストリップされ、同名の再登録は上書き（last-write-wins）です。

---

## Hook agent events / エージェントイベントをフックする

```lua
-- 計画作成を監視
quorum.on("PlanCreated", function(event)
    print("Plan: " .. event.objective .. " (" .. event.task_count .. " tasks)")
end)

-- ツール実行後のログ
quorum.on("ToolCallAfter", function(event)
    print(event.tool_name .. " took " .. event.duration_ms .. "ms")
end)
```

全 11 イベントとデータフィールドは
[Scripting Reference](../reference/scripting.md) の Event Reference を参照してください。

---

## Related / 関連

- [Scripting Reference](../reference/scripting.md) - アーキテクチャ・全イベント一覧・サンドボックス
- [Configuration Reference](../reference/configuration.md) - `quorum.config` / `quorum.keymap` 等の API
- [Tutorial: Customizing with Lua](../tutorials/customizing-with-lua.md) - 手を動かして学ぶ入門
- [How to Add Custom Tools](./add-custom-tools.md) - `quorum.tools.register` でツール追加
