# Design Decisions / 設計決定記録

> ADR-style records of design decisions settled in GitHub Discussions
>
> GitHub Discussions で決着した設計判断の記録（ADR スタイル）

---

## What is this? / これは何?

copilot-quorum の大きな設計判断は [GitHub Discussions](https://github.com/music-brain88/copilot-quorum/discussions) の
RFC で議論され、決定されてきました。このディレクトリはその **決着済みの判断** を
「背景 → 議論 → 決定 → 理由」の形式で記録したものです。

- **現行仕様**（どう動くか）は各 explanation / reference ドキュメントが正
- **経緯**（なぜそうなったか）はこの ADR が正。元 Discussion へのリンク付き
- 進行中・構想段階の議論（#58 TUI, #43 Knowledge, #157 Workflow, #98 Extension）は
  [Vision](../../vision/README.md) 配下で扱い、ここには記録しない

## Format / フォーマット

各レコードは以下の構成です:

```markdown
# NNNN: タイトル

Status / Date / Source Discussion

## 背景 / Context      ← 何が問題だったか
## 議論 / Discussion   ← どんな案が出て、何が争点だったか
## 決定 / Decision     ← 何に決めたか
## 理由 / Rationale    ← なぜそれを選んだか
## Related / 関連      ← コミット・現行ドキュメント・後続 Discussion
```

番号は元 Discussion の時系列順です。

## Records / 記録一覧

| # | Title | Source | Status |
|---|-------|--------|--------|
| [0001](./0001-tool-executor-port-layering.md) | ToolExecutorPort のレイヤリング | [#10](https://github.com/music-brain88/copilot-quorum/discussions/10) | Implemented |
| [0002](./0002-three-orthogonal-axes.md) | 5モード enum → 3 直交軸 | [#38](https://github.com/music-brain88/copilot-quorum/discussions/38) | Implemented |
| [0003](./0003-restore-quorum-consensus-level.md) | Quorum モード復活と ConsensusLevel | [#55](https://github.com/music-brain88/copilot-quorum/discussions/55) | Implemented |
| [0004](./0004-role-based-model-configuration.md) | ロールベースモデル設定 | [#54](https://github.com/music-brain88/copilot-quorum/discussions/54) / [#63](https://github.com/music-brain88/copilot-quorum/discussions/63) | Implemented |
| [0005](./0005-unified-interaction-architecture.md) | Agent/Ask/Discuss の対等化 | [#138](https://github.com/music-brain88/copilot-quorum/discussions/138) | Partially Implemented |
| [0006](./0006-tui-content-route-surface.md) | TUI Content/Route/Surface 分離 | [#207](https://github.com/music-brain88/copilot-quorum/discussions/207) | Implemented (Phase 1-2) |
