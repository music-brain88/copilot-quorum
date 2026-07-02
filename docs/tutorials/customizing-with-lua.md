# Customizing with Lua / Lua でカスタマイズ

> Create your init.lua, tune models and behavior, then write your first plugin and custom tool
>
> init.lua を作り、モデルと動作を調整し、最初のプラグインとカスタムツールを書く

[Getting Started](./getting-started.md) を終えていることが前提です。
copilot-quorum の設定はすべて Lua（Neovim の init.lua と同じ発想）で行います。

---

## 1. Create your init.lua / init.lua を作る

リポジトリ直下のテンプレートをコピーします:

```bash
mkdir -p ~/.config/copilot-quorum
cp quorum.example.lua ~/.config/copilot-quorum/init.lua
```

読み込まれているか確認:

```bash
cargo run -p copilot-quorum -- --show-config
```

## 2. Set role-based models / ロール別モデルを設定する

init.lua を開いて、役割ごとにモデルを割り当てます:

```lua
-- 情報収集は速くて安いモデル、計画とレビューは高性能モデル
quorum.config.set("models.exploration", "claude-haiku-4.5")
quorum.config.set("models.decision", "claude-sonnet-4.5")
quorum.config.set("models.review", { "claude-opus-4.5", "gpt-5.2-codex" })

-- Discussion / Ensemble の参加モデルとモデレーター
quorum.config.set("models.participants", { "claude-opus-4.5", "gpt-5.2-codex", "gemini-3-pro-preview" })
quorum.config.set("models.moderator", "claude-opus-4.5")
```

対話モードの `/config` で反映を確認できます。

## 3. Tune agent behavior / エージェント動作を調整する

```lua
-- 介入までの計画修正回数を増やす
quorum.config.set("agent.max_plan_revisions", 5)

-- 計画だけ作らせるのをデフォルトに（実行はしない）
quorum.config.set("agent.phase_scope", "plan-only")
```

タスクを 1 つ実行して、動作の違いを確かめてみてください。
確認できたら `phase_scope` は `"full"` に戻しておきましょう。

## 4. Write your first plugin / 最初のプラグインを書く

プラグインは `plugins/` に置くと init.lua の後に自動ロードされます:

```bash
mkdir -p ~/.config/copilot-quorum/plugins
```

`~/.config/copilot-quorum/plugins/hello.lua`:

```lua
-- ユーザー定義コマンド: /hello で挨拶
quorum.command.register("hello", {
    description = "Say hello",
    fn = function(args)
        print("Hello, " .. (args ~= "" and args or "world") .. "!")
    end,
})

-- イベントフック: ツール実行を監視
quorum.on("ToolCallAfter", function(event)
    print("[hook] " .. event.tool_name .. " finished in " .. event.duration_ms .. "ms")
end)
```

再起動して `/hello quorum` と打ってみてください。
エージェントにタスクを投げると、ツール実行のたびにフックが発火します。

## 5. Register a custom tool / カスタムツールを登録する

外部 CLI コマンドをエージェントのツールにできます。
`~/.config/copilot-quorum/plugins/tools.lua`:

```lua
quorum.tools.register("word_count", {
    description = "Count lines, words and characters in a file",
    command = "wc {path}",
    risk_level = "low",
    parameters = {
        path = { type = "string", description = "File path", required = true },
    }
})
```

エージェントに「README の行数を数えて」と頼むと、
LLM が組み込みツールと同じように `word_count` を発見して使います。

## 6. Next / 次のステップ

- [How to Write Lua Plugins](../how-to/write-lua-plugins.md) — イベントフックの実践レシピ
- [Configuration Reference](../reference/configuration.md) — 全 29 設定キーと Lua API
- [Scripting Reference](../reference/scripting.md) — 全イベント一覧とサンドボックスの仕様
