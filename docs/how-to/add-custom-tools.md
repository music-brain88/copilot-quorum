# How to Add Custom Tools / カスタムツールを追加する

> Register external CLI commands as first-class agent tools from init.lua
>
> 外部 CLI コマンドを init.lua からファーストクラスのエージェントツールとして登録する

---

## Register a custom tool / カスタムツールを登録する

`~/.config/copilot-quorum/init.lua` で `quorum.tools.register()` を呼ぶだけで、
コードを書くことなくエージェントが使えるツールを追加できます。

```lua
quorum.tools.register("gh_create_issue", {
    description = "Create a GitHub issue",
    command = "gh issue create --title {title} --body {body}",
    risk_level = "high",
    parameters = {
        title = { type = "string", description = "Issue title", required = true },
        body  = { type = "string", description = "Issue body content", required = true },
    }
})
```

低リスク（読み取り専用）ツールの例:

```lua
quorum.tools.register("list_branches", {
    description = "List git branches",
    command = "git branch --list {pattern}",
    risk_level = "low",
    parameters = {
        pattern = { type = "string", description = "Branch name pattern (e.g., 'feat/*')", required = false },
    }
})
```

登録したツールはエージェントのツール一覧に自動的に追加され、
LLM は組み込みツールと同じように呼び出せます。

### 仕様のポイント

- **コマンドテンプレート**: `{param_name}` プレースホルダーでパラメータを埋め込み
- **シェルエスケープ**: パラメータ値は自動的にエスケープされ、コマンドインジェクションを防止
- **安全デフォルト**: `risk_level` 未指定の場合は `"high"`（Quorum レビュー必須になる）
- **優先度 75**: CLI プロバイダー（50）より高く、同名の組み込みツールを上書きできる

---

## Use enhanced CLI tools / 強化 CLI ツールを使う

`rg` (ripgrep) や `fd` がインストールされていれば自動検出され、
標準の `grep` / `find` より高速な実装が提案されます。特別な設定は不要です。

```bash
# エージェントがツールを使ってタスクを実行
copilot-quorum "Find all TODO comments and create a summary"
# → rg が検出されていれば自動的に使用、無ければ grep にフォールバック
```

---

## Enable web tools / Web ツールを有効化する

`web_fetch` / `web_search` は `web-tools` feature で提供されます（CLI crate ではデフォルト有効）:

```bash
# web-tools feature 付きでビルド（CLI はデフォルト有効）
cargo build -p copilot-quorum

# feature なしでビルド
cargo build -p quorum-infrastructure --no-default-features
```

---

## Related / 関連

- [Tool System Reference](../reference/tool-system.md) - プロバイダー優先度・組み込みツール一覧
- [How to Extend the Codebase](./extend-the-codebase.md) - Rust でツールプロバイダーを実装する
- [Configuration Reference](../reference/configuration.md) - `quorum.tools.register` の API
- [Agent Behavior](../explanation/agent-behavior.md) - リスクレベルと Quorum レビューの関係
