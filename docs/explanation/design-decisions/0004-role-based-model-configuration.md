# 0004: ロールベースモデル設定 / Role-Based Model Configuration

> **Status**: Implemented
> **Date**: 2026-02-05 提案（#54）→ 2026-02-06 再構成（#63）
> **Source**: [Discussion #54](https://github.com/music-brain88/copilot-quorum/discussions/54) / [Discussion #63](https://github.com/music-brain88/copilot-quorum/discussions/63) — RFC: Role-based Model Configuration for Cost Optimization

---

## 背景 / Context

エージェントの全フェーズを単一の高性能モデルで実行するとコストが高い。
フェーズごとに要求される能力は異なる:

- **軽量モデルで十分**: Context Gathering（情報収集）、低リスクツールの呼び出し判断
  （read_file, glob_search, grep_search）
- **高性能モデルが必要**: Planning（計画立案）、Review（Plan/Action Review）、
  高リスクツール（write_file, run_command）の呼び出し判断

## 議論 / Discussion

フェーズ/リスクレベルに応じたロール別設定を導入し、各ロールにコスト最適な
デフォルトを埋め込む案。#54 で提案され、読みやすく再構成した #63 に引き継がれた。

## 決定 / Decision

モデル設定をロール別に分割する:

| Role | 用途 | 特性 |
|------|------|------|
| `exploration` | Context Gathering + 低リスクツール | 高速・低コスト |
| `decision` | Planning + 高リスクツール判断 | 高性能 |
| `review` | Plan / Action Review（複数指定可） | 高性能・多様性 |

後に Interaction ロール（`participants` / `moderator` / `ask`）も同じ
`ModelConfig` に統合され、Agent ロールと Interaction ロールの 2 系統になった。

現在は Lua で設定する:

```lua
quorum.config.set("models.exploration", "gpt-5.3-codex")
quorum.config.set("models.decision", "claude-sonnet-4.5")
quorum.config.set("models.review", { "claude-opus-4.5", "gpt-5.3-codex", "gemini-3.1-pro-preview" })
```

## 理由 / Rationale

- **コスト効率** — 呼び出し回数が最も多い Context Gathering / 低リスクツールを
  軽量モデルに割り当てることで、品質を落とさずコストを大幅に削減できる
- **リスク整合** — 「元に戻すのが困難な操作ほど高性能モデルに判断させる」という
  リスクベースの原則と一致する
- **デフォルトで最適** — 各ロールにコスト最適なデフォルトを埋め込み、
  設定なしでも合理的に動く

## Related / 関連

- 現行ドキュメント: [Configuration Reference](../../reference/configuration.md)（`models.*` キー）,
  [Agent System Reference](../../reference/agent-system.md)（`ModelConfig` 型）
- 関連: リスク分類は [Agent Behavior](../agent-behavior.md) を参照
