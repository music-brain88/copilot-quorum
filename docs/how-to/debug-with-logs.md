# How to Debug with Logs / ログでデバッグする

> Enable operation logs, conversation transcripts and transport dumps
>
> 操作ログ・会話トランスクリプト・Transport dump を有効化して調査する

---

## Enable debug logging / デバッグログを有効化する

操作ログは `RUST_LOG` 環境変数、または `-v` フラグで制御します:

```bash
# デバッグログ有効化
RUST_LOG=debug cargo run -p copilot-quorum -- "Your question"

# 特定クレートのみ
RUST_LOG=quorum_infrastructure=trace cargo run -p copilot-quorum -- "Your question"

# CLI フラグで段階的に増加
copilot-quorum -v "Your question"      # verbose
copilot-quorum -vv "Your question"     # more verbose
copilot-quorum -vvv "Your question"    # trace 相当
```

---

## Control conversation transcripts / 会話ログの出力先を変える

会話トランスクリプト（JSONL）はデフォルトで自動生成されます:

```bash
# 出力先ディレクトリを指定
copilot-quorum --log-dir ./my-logs "Your task"

# 会話ログファイルを無効化
copilot-quorum --no-log-file "Your task"
```

---

## Which log to look at / どのログを見るべきか

| シナリオ | 使うべきログ |
|---------|------------|
| 「セッション作成に失敗した」 | `tracing::warn!` (操作ログ) |
| 「LLM が 500 トークン返した」 | `ConversationLogger` (会話ログ) |
| 「TCP 上の raw メッセージ内容」 | `tracing::debug!` (transport dump) |
| 「ツール実行に 3 秒かかった」 | 両方（操作ログ + 会話ログの metadata） |

JSONL スキーマとログの内部構造は [Logging Reference](../reference/logging.md) を参照してください。

---

## Related / 関連

- [Logging Reference](../reference/logging.md) - 3 種類のログの仕組みと JSONL スキーマ
- [CLI Reference](../reference/cli.md) - `-v` / `--log-dir` / `--no-log-file` フラグ
