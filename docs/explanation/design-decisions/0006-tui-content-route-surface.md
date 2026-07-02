# 0006: TUI Content/Route/Surface 分離 / TUI Content/Route/Surface Separation

> **Status**: Implemented (Phase 1: #208, Phase 2: #209)
> **Date**: 2026-02-22
> **Source**: [Discussion #207 — RFC: TUI Display Architecture](https://github.com/music-brain88/copilot-quorum/discussions/207)

---

## 背景 / Context

従来の TUI は Conversation / Progress パネルの「何を表示するか」と「どこに表示するか」が
一体化していた:

- `widgets/mod.rs` で **70/30 分割がハードコード**
- `Pane` が `messages`, `streaming_text`, `progress` を直接保持 —
  レイアウトとコンテンツが密結合
- Progress パネルの表示/非表示すらユーザーが選べない
- Ensemble 時の複数モデル並列表示など、表示先のバリエーションに対応困難

## 議論 / Discussion

Neovim エコシステムの 2 つの先行例を比較検討した:

- **noice.nvim** — Vim の固定 UI を `Source → Route → View` の 3 層に分解
- **ddu.vim** — `Source → Filter → UI → Column → Kind` の 5 関心完全分離。
  設定解決チェーン（グローバル → 型別 → ローカルのパッチ適用）も参考にした
- **Telescope** — 対照的に Picker が全部を束ねる合成アプローチ

追記の議論で ContentRenderer（ddu の Column 相当）や Preset 機構も提案されたが、
「Neovim 本体は buffer/window という**土台**だけ提供し、その上にプラグインが乗る」
という構造に倣い、**最初のスコープから外す**判断をした。

## 決定 / Decision

1. TUI のディスプレイ層を **Content（何を）/ Route（どこへ）/ Surface（どう表示）**
   の 3 プリミティブに分離する
2. **最小限の土台のみ先に作る** — Renderer / Preset などの上物は、土台を使ってみて
   機能界面が見えてから追加する
3. レイアウトプリセット（default / minimal / wide / stacked）と
   `quorum.tui.routes.set(content, surface)` によるルーティング変更を Lua に公開

## 理由 / Rationale

- **カスタマイズと拡張の界面** — 表示先の差し替え（Floating Window、Notification、
  Merge View 等の将来拡張）は、コンテンツとレイアウトが分離されていて初めて可能になる
- **土台先行** — 抽象を先に完成させようとするより、最小プリミティブを置いて
  実際の使用から必要な上物を発見する方が、誤った抽象化を避けられる
- noice.nvim / ddu.vim という実績あるパターンの流用で設計リスクを下げる

## Related / 関連

- 実装 Issue: #208 (Phase 1: 土台), #209 (Phase 2: レイアウトカスタマイズ)
- 現行ドキュメント: [TUI Design](../tui-design.md), [TUI Internals](../../reference/tui-internals.md),
  [Configuration Reference](../../reference/configuration.md)（`tui.layout.*` キー・`quorum.tui.*` API）
- 関連 Discussion: [#58 (Neovim TUI) Layer 4/5](https://github.com/music-brain88/copilot-quorum/discussions/58),
  [#43 (Knowledge-Driven) の UX 側面](https://github.com/music-brain88/copilot-quorum/discussions/43)
- 後続: Remote Control API の `layout.set` / `route.set` はこの土台の上に実装された
