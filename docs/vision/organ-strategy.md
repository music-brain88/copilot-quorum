# Organ Strategy / 臓器戦略

> How copilot-quorum survives next to Claude Code + herdr
>
> Claude Code + herdr という強力なライバルスタックの隣で、copilot-quorum がどう生き残るかの戦略

---

## きっかけ — 「26 本の PR を直したのは quorum ではなかった」

2026-07-03、[Zenn 記事](#出典)2 本（herdr + Claude Code の並列 agent コックピット、1 日 54 PR マージの体験記）を題材に、作者自身が直視した現実がある。

> **copilot-quorum の 26 本の PR を直したのは、copilot-quorum ではなく Claude Code + herdr のコックピットだった**

作者自身がライバルスタックに日常的に手を伸ばしている——これは最強の市場調査データである。「オーケストレーション体験」レイヤーでは、ディスパッチ・状態計器盤・worktree 隔離・割り込み駆動の上りチャネル・permission 3 層設計など、quorum が TUI で目指した景色のかなりの部分が、すでに既製品の組み合わせで動いている。

この現実を起点に、「Claude Code に勝つには」ではなく「**Claude Code には原理的に持てない層はどこか**」を問い直した結果が、以下の「臓器」戦略である。

## nix / mise の配役

quorum と Claude Code + herdr の関係は、**nix と mise** の関係に近い。

| | mise 側 = Claude Code + herdr | nix 側 = copilot-quorum |
|---|---|---|
| 哲学 | プラグマティズム。既存部品の合成、体験の速さ | 原理主義。「合議」がドメインの第一級概念 |
| 強み | エルゴノミクス、エコシステム、改善速度 | 構造的に相手が真似できない原理 |
| 弱点 | 単一ベンダー・単一モデル系列に縛られる | 日常の使い勝手で永遠に追いかける側 |

**教訓**: nix は mise とエルゴノミクスで戦って生き残ったのではない。「再現性」という、mise には原理的に持てない層を握っているから、同じマシンの上で共存できている。

quorum も同型を狙う。Claude Code のエルゴノミクスを日常使いで追い抜こうとするのではなく、Claude Code が構造的に持てない層を握る。

## Claude Code が構造的に持てない 3 つ

1. **異種モデルの合議** — Anthropic は GPT / Gemini を対等な合議メンバーとしてオーケストレーションする製品を作らない（作れないのではなく動機がない）。Zenn 記事に書いた「AI レビューの診断ミス・処方箋正解」は単一モデルレビューの相関エラーそのものであり、quorum の匿名相互レビュー + Synthesis がこれを直撃する。
2. **承認モデルの第 4 段階 = per-quorum** — per-action → per-launch → per-policy に続く自然な次の段階。「人間はポリシーに署名し、個別の高リスク操作は異種モデルの合議が承認する」。`QuorumRule` / `Vote` / `RiskLevel` としてすでにドメインモデル化されている。
3. **プロバイダー中立** — Copilot ライセンスで動く企業配布という優位はあるが、現状は Copilot CLI 依存が逆に信頼性リスクになっている（1.0.65 系の互換性問題）。この弱点を埋めるのが Track C（後述）。

## 3 層スタック

「herdr → quorum で quorum が複数いるならそれは強い」——各リポジトリに quorum が受付（現地司令塔）として常駐する絵。

```
herdr           … 艦隊管制。横の可視性（全リポジトリ・全部屋の working/idle/blocked）
copilot-quorum  … 現地司令塔。縦の可視性（1 リポジトリの Plan/投票/Phase/interaction tree）+ 合議による判断
Claude Code 等  … 作業者。部屋の中の実装ハンズ
```

3 層は競合しない。herdr は横に広いが浅い（計器は working/idle/blocked 程度の状態のみ）。quorum は 1 リポジトリに深く潜る。TUI が戦う軸は「幅の広さ」ではなく「深さ」——herdr が絶対作らないもの（Plan のタスク進捗、投票の内訳、Phase 遷移、interaction tree の可視化）に全振りする。

## worktree コモディティ化と herdr 非依存の設計原則

「Claude Code も Codex も worktree あるじゃん？」という指摘の通り、**worktree 隔離はコモディティ化が始まっている**。1 タスク = 1 隔離ツリーは、遠からず全ハーネスの標準装備になる。

これは戦略への反証ではなく補強材料として読む。

- コモディティ化の波は「ハコ」（worktree・隔離・並列起動・状態計器）を食う → だからハコで戦わない判断が正しい。
- 波が食えないのは**利害相反があってベンダーが作れない層** = 異種モデル合議。
- herdr 自体も squeeze されるリスクがある → **quorum は herdr にも依存しない設計にする**。
  - Track A（臓器）は元から無傷: 刺さっているのは **PR という普遍インターフェース**。コックピットが何に入れ替わっても生き続ける。
  - Track B は「herdr 統合」ではなく「**supervisor 報告ポート + herdr アダプタ**」として設計する（`application/ports` に trait、`infrastructure` に herdr アダプタ。オニオンアーキテクチャ的にも自然な置き場所）。

nix の生存戦略と同型: 「どのディストロが勝っても上に乗れる」→ quorum は「**どのコックピットが勝っても刺さる臓器**」を目指す。

worktree 管理そのものは quorum で再実装しない（受付のディスパッチは Lua の `quorum.tools.register` で `herdr worktree create` を叩けば済む）。再発明した瞬間に nix が mise の真似を始める構図になってしまう。

## 3 トラックと現在地（2026-07-09 時点）

| トラック | 中身 | 状態 |
|---|---|---|
| **A. 臓器（最短の楔）** | headless `quorum review <PR/diff>` — 合議 + 投票 + Synthesis を JSON/Markdown で返す。司令塔が `gh pr create` の後に呼ぶ | ✅ **完走**。[#300](https://github.com/music-brain88/copilot-quorum/issues/300)（headless review）・[#302](https://github.com/music-brain88/copilot-quorum/issues/302)（Remote Control API 完全化）・[#303](https://github.com/music-brain88/copilot-quorum/issues/303)（ヘッドレス基盤）起票 → [PR #305](https://github.com/music-brain88/copilot-quorum/pull/305)〜[#308](https://github.com/music-brain88/copilot-quorum/pull/308)、[#310](https://github.com/music-brain88/copilot-quorum/pull/310) マージ済み。headless review の CI ゲートと `rpc` サブコマンドが稼働中 |
| **B. 現地司令塔（TUI の新しい顔）** | supervisor 報告ポート + herdr アダプタ、HiL=blocked マッピング、1 リポジトリ深堀り計器盤 | 🟡 [Issue #309](https://github.com/music-brain88/copilot-quorum/issues/309) オープン |
| **C. 堀** | [#218](https://github.com/music-brain88/copilot-quorum/issues/218) / [#219](https://github.com/music-brain88/copilot-quorum/issues/219) 直接 API プロバイダー（Anthropic / OpenAI）。合議の多様性を Copilot CLI の人質にしない | 🟡 オープン。Copilot CLI 1.0.65 系の互換性問題が直接の動機となり、「便利機能」から「堀そのもの」に格上げ済み |

### Track A を最初に選んだ理由

実運用（1 日 54 PR マージという体験記の水準）で、今いちばん細いパイプは人間のレビュー帯域。headless review を作った瞬間に、作者自身が毎日それを使うことになる。ドッグフーディングが「頑張って使う」から「使わないと困る」に変わる、という見立てが実際に当たった——Track A 完走時のドッグフーディングで、少数派 REJECT（gpt-5.3-codex）が `cli.command.take()` + `if let` 連鎖のバグ（2 個目のサブコマンド variant を握りつぶす）を本当に発見し、単一モデルレビューの相関エラーを異種合議が拾う構図が自分のリポジトリで実証された。

また、Track A の JSON 出力（投票内訳）はそのまま Track B の計器盤のデータになるため、投資が無駄にならない。

## 出典

- Obsidian ResearchNotes: `ClaudeCodeSession-20260703-CopilotQuorum.md`（戦略決定セッション — nix/mise 配役・3 層スタック・3 トラックの合意、Issue #300 起票まで）
- Obsidian ResearchNotes: `ClaudeCodeSession-20260704-QuorumRfc304Sprint.md`（Track A 完走の記録 — RFC Discussion #304、PR #305〜#310 のマージ、ドッグフーディング結果）

## Related / 関連

- [Vision Overview](README.md) — 現在地・ロードマップ全体
- [design-philosophy.md](../explanation/design-philosophy.md) — DDD + オニオンアーキテクチャの理由（Track B の「herdr アダプタを infrastructure に置く」設計はこの原則に従う）
- [review-a-pr.md](../how-to/review-a-pr.md) — Track A の実体である `review` サブコマンドの使い方
- [tui-remote-control.md](../reference/tui-remote-control.md) — Track A/B が共有する Remote Control API
