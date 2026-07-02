# How to Manage Project Context / プロジェクトコンテキストを管理する

> Generate and control the project context the agent reads before working
>
> エージェントが作業前に読み込むプロジェクトコンテキストの生成と制御

---

## Generate project context with /init / `/init` でコンテキストを生成する

```bash
# ワンショットで生成
copilot-quorum /init

# REPL / TUI 内から（--force で再生成）
/init
/init --force
```

`/init` はプロジェクトの情報を収集し、`.quorum/context.md` を生成します。
このファイルはエージェントがプロジェクトを理解するためのコンテキストとして使用されます。

---

## What gets loaded / 読み込まれるファイル

読み込み対象ファイル（優先度順）:

| Priority | File | Description |
|----------|------|-------------|
| 1 | `.quorum/context.md` | 生成された Quorum コンテキスト |
| 2 | `CLAUDE.md` | ローカルプロジェクト指示 |
| 3 | `~/.claude/CLAUDE.md` | グローバル Claude 設定 |
| 4 | `README.md` | プロジェクト README |
| 5 | `docs/**/*.md` | docs ディレクトリ内の全 Markdown |
| 6 | `Cargo.toml` / `package.json` / `pyproject.toml` | ビルド設定 |

定義ファイル: `domain/src/context/`（`ProjectContext`, `KnownContextFile`）、
`infrastructure/src/context/`（`LocalContextLoader`）

---

## Control context size / コンテキスト量を制御する

タスク実行間で持ち越す結果コンテキストの量は `context_budget.*` キーで調整できます:

```lua
-- ~/.config/copilot-quorum/init.lua
quorum.config.set("context_budget.max_entry_bytes", 20000)   -- 単一タスク結果の上限
quorum.config.set("context_budget.max_total_bytes", 60000)   -- 全過去結果の合計上限
quorum.config.set("context_budget.recent_full_count", 3)     -- 完全保持する直近結果数
```

全キーは [Configuration Reference](../reference/configuration.md) を参照してください。

---

## Related / 関連

- [Configuration Reference](../reference/configuration.md) - `context_budget.*` キーの詳細
- [Agent Behavior](../explanation/agent-behavior.md) - Context Gathering フェーズの位置づけ
- [CLI Reference](../reference/cli.md) - `/init` コマンド
