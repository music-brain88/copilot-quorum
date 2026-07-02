# How to Run a Quorum Discussion / Quorum Discussion を実行する

> Run a multi-model discussion (Initial Query → Peer Review → Synthesis) from the CLI or TUI
>
> CLI / TUI から複数モデルの議論（初期回答 → 相互レビュー → 統合）を実行する

---

## One-shot from the CLI / CLI からワンショット実行

```bash
# Quorum Discussion（3フェーズの議論）
copilot-quorum "What's the best way to handle errors in Rust?"

# モデルを指定して Discussion
copilot-quorum -m claude-sonnet-4.5 -m gpt-5.3-codex "Compare async/await patterns"

# 全フェーズの出力を表示（デフォルトは統合結果のみ）
copilot-quorum -o full "Explain the actor model"
```

---

## From the TUI / TUI から実行

TUI のコマンドモード（`:`）から `discuss` コマンドで新しい Discussion タブを開けます:

```
:discuss What are the tradeoffs of microservices?
```

REPL では `/council <question>` でアドホックに Quorum Discussion を実行できます。

---

## Configure participants and moderator / 参加モデルとモデレーターの設定

参加モデル（Phase 1-2）とモデレーター（Phase 3 の統合役）は
`~/.config/copilot-quorum/init.lua` で設定します:

```lua
quorum.config.set("models.participants", { "claude-opus-4.5", "gpt-5.3-codex", "gemini-3.1-pro-preview" })
quorum.config.set("models.moderator", "claude-opus-4.5")
```

出力形式の既定値も変更できます:

```lua
quorum.config.set("output.format", "synthesis")  -- "full", "synthesis", "json"
```

全設定キーは [Configuration Reference](../reference/configuration.md) を参照してください。

---

## Related / 関連

- [Quorum Discussion & Consensus](../explanation/quorum-consensus.md) - 3フェーズ議論の仕組みと設計意図
- [How to Use Ensemble Mode](./use-ensemble-mode.md) - エージェントの計画生成を複数モデル化する
- [CLI Reference](../reference/cli.md) - 全 CLI フラグと REPL コマンド
