# 0003: Quorum モード復活と ConsensusLevel / Restore Quorum Mode as ConsensusLevel

> **Status**: Implemented
> **Date**: 2026-02-05 提案 → 2026-02-06 決着
> **Source**: [Discussion #55 — RFC: Restore Quorum (Council) Mode in CLI](https://github.com/music-brain88/copilot-quorum/discussions/55)

---

## 背景 / Context

CLI のデフォルトが Agent モードになった結果、README に記載されていた合議（Quorum）モードの
3 フェーズフロー（Initial Query → Peer Review → Synthesis）が CLI から使えない状態になっていた。
`RunQuorumUseCase` は存在するが、CLI から呼び出されていなかった。

## 議論 / Discussion

- 短期案: `--council` フラグまたは REPL の `/council` コマンドで合議モードを有効化
- 長期案: vim の buffer のようなコンテキスト管理 — 各モード（Agent / Council）を
  別々の「buffer」として保持し、明示的に切り替えてコンテキストを受け渡す
  （後の Buffer/Tab System・[#43 Knowledge-Driven Architecture](https://github.com/music-brain88/copilot-quorum/discussions/43) につながる構想）

## 決定 / Decision

- **`ConsensusLevel`（Solo / Ensemble）** をユーザー向けのモード軸として導入し、
  CLI フラグ（`--solo` / `--ensemble`）と REPL コマンド（`/solo` / `/ens`）で切り替え可能に
- Quorum の核心概念を **`domain/src/quorum/` モジュール**として独立
  （`Vote`, `QuorumRule`, `ConsensusRound`）
- 命名体系を整理: Quorum Discussion（議論）/ Quorum Consensus（投票承認）/
  Quorum Synthesis（統合）

## 理由 / Rationale

- 「単一モデルで速く」と「複数モデルで多角的に」は日常的に往復する使い分けであり、
  ワンショットコマンドではなく**永続的なモード**として表現すべき
- Quorum のドメイン概念（投票・ルール・ラウンド）はエージェントのレビューでも
  Discussion でも共用されるため、独立モジュールにすることで再利用可能になる

## Related / 関連

- 実装コミット: `53cd28a` (Solo/Ensemble modes), `4ca46d7` (orthogonal axes), PR #56/#57
- 現行ドキュメント: [Quorum Discussion & Consensus](../quorum-consensus.md), [Ensemble Mode](../ensemble-mode.md)
- 関連 ADR: [0002 (3 直交軸)](./0002-three-orthogonal-axes.md)
- 長期構想の buffer 案は [0005 (Unified Interaction Architecture)](./0005-unified-interaction-architecture.md) で結実
