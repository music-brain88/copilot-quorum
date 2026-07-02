# How to Use Ensemble Mode / Ensemble モードを使う

> Generate plans with multiple models in parallel and let them vote for the best one
>
> 複数モデルが並列で計画を生成し、投票で最良の計画を選択させる

---

## Run a task in Ensemble mode / Ensemble モードでタスクを実行

```bash
# Ensemble モードで実行
copilot-quorum --ensemble "Design the authentication system"

# Ensemble モードで REPL を起動
copilot-quorum --ensemble
```

---

## Switch modes at runtime / 実行中にモードを切り替える

REPL / TUI 内でモードコマンドを使います:

```
/ens     # Ensemble モードに切り替え
/solo    # Solo モードに戻す
```

プロンプト表示（例: `ensemble>`）で現在のモードを確認できます。

---

## Make Ensemble the default / Ensemble をデフォルトにする

`~/.config/copilot-quorum/init.lua`:

```lua
quorum.config.set("agent.consensus_level", "ensemble")  -- "solo" or "ensemble"
```

計画を生成・相互投票するモデル群は `models.participants` で設定します:

```lua
quorum.config.set("models.participants", { "claude-opus-4.5", "gpt-5.2-codex", "gemini-3-pro-preview" })
```

全設定キーは [Configuration Reference](../reference/configuration.md) を参照してください。

---

## When to use / 使いどころ

| タスク | 推奨モード |
|--------|-----------|
| シンプルなタスク、バグ修正 | Solo（デフォルト） |
| 複雑な設計、アーキテクチャ決定 | **Ensemble** |

計画生成コストはモデル数に比例して増えるため、多角的な検討が価値を持つタスクで使うのが効果的です。

---

## Related / 関連

- [Ensemble Mode](../explanation/ensemble-mode.md) - 独立生成+投票を採用した理由と研究エビデンス
- [How to Run a Quorum Discussion](./run-a-quorum-discussion.md) - タスク実行を伴わない議論だけを行う
- [Orchestration Axes](../explanation/orchestration-axes.md) - ConsensusLevel と他の設定軸の関係
