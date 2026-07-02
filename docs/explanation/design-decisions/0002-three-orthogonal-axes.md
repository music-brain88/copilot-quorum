# 0002: 5モード enum → 3 直交軸 / Five Modes → Three Orthogonal Axes

> **Status**: Implemented
> **Date**: 2026-02-02 提案 → 2026-02-06 決着
> **Source**: [Discussion #38 — RFC: オーケストレーションモード切り替え設計](https://github.com/music-brain88/copilot-quorum/discussions/38)

---

## 背景 / Context

当時の copilot-quorum には Agent Mode（`RunAgentUseCase`）と Council Mode
（`RunQuorumUseCase`、`/council` コマンド）が独立して存在し、
ユーザーが「今は合議モードで使いたい」と思っても毎回 `/council` を打つ必要があった。

## 議論 / Discussion

当初案は単一の `OrchestrationMode` enum に 5 モードを持たせる設計だった:

| Mode | 実行フロー |
|------|-----------|
| Agent | Context → Plan → Review → Exec |
| Quorum | 並列 LLM → Synthesis |
| Fast | 単一 LLM 即答（レビュー省略） |
| Debate | モデル間議論 |
| Plan | 計画作成のみ |

議論の過程で以下が明らかになった:

- **概念の混在** — 「体制（何モデルか）」「範囲（どこまで実行するか）」
  「戦略（どう議論するか）」という独立した概念が 1 つの enum に混ざっている
- **バリアント爆発** — 組み合わせが増えるたびに `EnsembleFast` のような
  合成バリアントが必要になる
- 実際に 50+ ターンかけても Quorum レビューが合意に至らない UX 問題も発覚し、
  並列レビュー・リトライ上限・ユーザー介入（後の HiL）の優先度が上がった

## 決定 / Decision

5 モード設計を却下し、**3 つの直交軸**に再設計:

```
ConsensusLevel (Solo/Ensemble) × PhaseScope (Full/Fast/PlanOnly) × OrchestrationStrategy (Quorum/Debate)
```

旧モードは軸の組み合わせで表現される（例: Fast = Solo + PhaseScope::Fast、
Plan = PhaseScope::PlanOnly）。無効な組み合わせ（Solo + Debate 等）は
バリデーションで検出する。

## 理由 / Rationale

- **組み合わせの自由度** — N × M × K 通りの構成を少ないバリアントで表現できる
- **拡張容易性** — 新しい Scope や Strategy を追加しても他の軸に影響しない
- **設定の明確性** — 各軸が「何を制御するか」が一目瞭然

## Related / 関連

- 実装コミット: `4ca46d7` (orthogonal axes), `53cd28a` (Quorum consensus + Solo/Ensemble)
- 現行ドキュメント: [Orchestration Axes](../orchestration-axes.md)
- 関連 ADR: [0003 (ConsensusLevel の導入)](./0003-restore-quorum-consensus-level.md)
- HiL の優先度見直しはこの議論の UX 問題フィードバックが発端
