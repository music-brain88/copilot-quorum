# How to Run a Headless PR/Diff Review / ヘッドレスで PR/diff レビューを実行する

> Run a multi-model Quorum review of a PR or diff from the CLI, CI, or another cockpit — no TUI required
>
> CLI・CI・他のコックピットから、TUI なしで多モデル Quorum レビューを実行する

---

## Basic usage / 基本の使い方

```bash
# PR 番号を指定（gh CLI で diff + title を取得）
copilot-quorum review --pr 123

# 標準入力から diff を渡す（gh 非依存）
git diff main...feature | copilot-quorum review

# ファイルで diff を渡し、レビュー観点を指示
copilot-quorum review --diff changes.patch --focus "並行処理の安全性"
```

デフォルト出力（`--output synthesis`）は、モデレーターが投票内訳を統合した Markdown レビューをそのまま stdout に出します。`gh pr comment 123 --body-file -` のようにパイプして人間向けコメントとして使えます。

---

## CI ゲートとして使う / Use as a CI gate

```bash
# --output json で構造化結果を取得
copilot-quorum review --pr "$PR_NUMBER" --output json > review.json
```

終了コードで合否を判定できます:

| Exit code | 意味 |
|-----------|------|
| `0` | 合議承認（過半数の cast 票が approve） |
| `1` | 合議否認 |
| `2` | 実行エラー（diff 取得失敗、全モデル不達など） |

```yaml
# 例: GitHub Actions での使用イメージ
- run: copilot-quorum review --pr ${{ github.event.pull_request.number }} --output json > review.json
- run: gh pr comment ${{ github.event.pull_request.number }} --body-file <(jq -r '.synthesis.conclusion' review.json)
```

`--output json` の形状は `quorum_result` v1 契約（[logging.md](../reference/logging.md) 参照）と同一です:

```jsonc
{
  "type": "quorum_result",
  "api_version": 1,
  "topic": "pr_review",
  "target": {"pr": 123, "title": "..."},
  "approved": true,
  "rule": "majority",
  "votes": [
    {"model": "claude-opus-4.5", "verdict": "approve", "reasoning": "...", "confidence": null}
  ],
  "synthesis": {"moderator": "...", "conclusion": "...", "key_points": [], "consensus": [], "disagreements": []},
  "timestamp": "2026-07-04T..."
}
```

---

## Configure review models / レビューに使うモデルの設定

投票（`models.review`）と統合（`models.moderator`）に使うモデルは、通常の Quorum レビューと共通の設定キーです:

```lua
quorum.config.set("models.review", { "claude-opus-4.5", "gpt-5.3-codex", "gemini-3.1-pro-preview" })
quorum.config.set("models.moderator", "claude-opus-4.5")
```

全設定キーは [Configuration Reference](../reference/configuration.md) を参照してください。

---

## 観測方法 / Observability

`review` はヘッドレス App のラッパーとして動くため、他の interaction form と同じ配管が使えます:

- 実行結果は会話ログ（JSONL）に `quorum_result`（`topic: "pr_review"`）として記録されます（`--log-dir` / `--no-log-file` で制御）
- Lua の `quorum.on("QuorumResult", fn)` でも観測できます
- 内部的には `InteractionForm::Review` の spawn なので、将来 Remote Control API の `interaction.spawn {"form": "review", "query": "<diff>"}` からも同じ経路で呼び出せます（PR メタデータ・focus 込みのヘッドレス起動は `review` サブコマンドが提供）

---

## Related / 関連

- [CLI Reference](../reference/cli.md) - `review` サブコマンドの全オプション
- [Logging Reference](../reference/logging.md) - `quorum_result` イベントと JSONL 会話ログ
- [How to Run a Quorum Discussion](./run-a-quorum-discussion.md) - 対話的な Quorum Discussion（`/council`, `:discuss`）
- [TUI Remote Control API](../reference/tui-remote-control.md) - `--headless --listen` と `interaction.spawn`
