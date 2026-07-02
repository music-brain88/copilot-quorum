# 0005: Agent/Ask/Discuss の対等化 / Unified Interaction Architecture

> **Status**: Partially Implemented（決定済み部分のみ記録。spawn の Phase B/C は未実装）
> **Date**: 2026-02-16 提案 → 2026-02-17 以降段階的に実装
> **Source**: [Discussion #138 — RFC: Unified Interaction Architecture](https://github.com/music-brain88/copilot-quorum/discussions/138)

---

## 背景 / Context

従来は Agent が暗黙の「親」概念で、Ask / Discuss は Agent の中の「アクション」として
位置づけられていた。独立した実行コンテキストとして扱われず、以下の問題があった:

- プロンプト表示 `solo:ask>` が `ConsensusLevel`（永続的なモード）と
  `InteractionType`（今から何をするかのアクション）を同列に並べていて概念的に混乱する
- ContextMode が Task レベルにしか存在せず、対話単位のコンテキスト制御ができない

コード検証により「`InteractionType` は実行フローに影響しない」
（`run_agent` に条件分岐ゼロ）ことが確認され、対等な形式として再定義する道が開けた。

## 議論 / Discussion

- **Vim の `buftype` アナロジー** — Vim がバッファの種類（normal/help/terminal）を
  対等な `buftype` として扱うように、対話の種類も対等な form として扱える
- **Presentation 層の設計** — 当初は `Buffer` + `BufferKind` 案だったが、
  Vim の 3 層モデル（Buffer / Window / Tab Page）を完全適用する方が自然と判明
- **命名** — `Session` は既存概念と衝突するため `Interaction` を採用
- Dog fooding で「複数モデルのメカニズムが 2 系統ある」
  （Discuss の 3 フェーズ討議と Ensemble の独立生成+投票）ことも整理された

## 決定 / Decision

1. **`InteractionForm`（Agent | Ask | Discuss）** をドメイン型として新設し、
   3 形式を対等な peer form として扱う
2. **ContextMode を Interaction レベルに昇格** — 対話単位でコンテキスト伝播量を制御
3. **Vim 3 層モデルの完全適用** — Buffer→`Interaction`(domain)、
   Window→`Pane`(presentation)、Tab Page→`Tab`(presentation)
4. **入力バッファは Pane ごとに分離** — タブ切り替え時に書きかけの下書きが保持される
5. **Spawn は段階的に導入** — Phase A: ユーザー起動（実装済み）→
   Phase B: ツールベース spawn（計画中）→ Phase C: ポリシー自動化（将来）

## 理由 / Rationale

- **概念の階層をなくす** — 「Agent の中の Ask」ではなく「Ask という対話を開く」方が、
  再帰ネスト（Agent が Discuss を子として生成する等）を自然に表現できる
- **Vim ユーザーのメンタルモデルに一致** — 新しい対話 = 新しいバッファ、という
  既知の操作感覚をそのまま持ち込める
- **実行フローと直交** — InteractionForm は「何を開くか」であり「どう実行するか」
  （3 直交軸）とは独立している

## Related / 関連

- 現行ドキュメント: [Interaction Model](../interaction-model.md)（現行仕様の正）,
  [TUI Design](../tui-design.md)
- 関連 Discussion: [#43 (Knowledge-Driven)](https://github.com/music-brain88/copilot-quorum/discussions/43),
  [#58 (Neovim TUI) Layer 3](https://github.com/music-brain88/copilot-quorum/discussions/58)
- 関連 Issue: #119/#120 (Buffer/Tab Phases), #127 (BufferType Design), #142 (Phase 1 実装), #143 (Interaction Nesting)
- 設定 4 型分割（SessionMode/ModelConfig/AgentPolicy/ExecutionParams）はこの議論の
  前提整理として #116/#122/#123 で決定された
