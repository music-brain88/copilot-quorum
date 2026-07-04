# TUI Remote Control API / TUI リモート操作 API

> JSON-RPC API for driving the TUI from external processes
>
> 外部プロセスから TUI を操作する JSON-RPC API

---

## Overview / 概要

`--listen <PATH>` で起動すると、Unix ドメインソケット上に JSON-RPC サーバーが公開され、
外部プロセス（コーディングエージェント等）がキーボードと対等に TUI を操作できます。
Neovim の `nvim --listen` に相当する機能です。

```bash
# TUI をソケット付きで起動（実ターミナルで表示もしたい場合）
copilot-quorum --listen /tmp/quorum.sock

# ヘッドレス起動（TTY 不要。--listen 必須 — #303）
copilot-quorum --headless --listen /tmp/quorum.sock

# 別プロセスから操作（scripts/tui-rpc.py はリファレンスクライアント）
scripts/tui-rpc.py /tmp/quorum.sock state.get
scripts/tui-rpc.py /tmp/quorum.sock input.send '{"text": "Fix the bug in login.rs"}'
scripts/tui-rpc.py /tmp/quorum.sock pane.read '{"last": 5}'
scripts/tui-rpc.py /tmp/quorum.sock hil.respond '{"decision": "approve"}'

# 画面を「見る」・レイアウトを調整する (Phase 2)
scripts/tui-rpc.py /tmp/quorum.sock screen.capture '{"width": 120, "height": 40}'
scripts/tui-rpc.py /tmp/quorum.sock layout.get
scripts/tui-rpc.py /tmp/quorum.sock layout.set '{"preset": "stacked"}'
scripts/tui-rpc.py /tmp/quorum.sock keys.feed '{"keys": ["i", "h", "i", "Esc"]}'
```

ワイヤ形式は LSP スタイルの `Content-Length` フレーミング + JSON-RPC 2.0 です。
設計判断の背景は [TUI Design](../explanation/tui-design.md) を参照してください。

---

## Headless Mode / ヘッドレスモード (#303)

`--headless` は `TuiApp` のイベントループから実ターミナルへの依存（raw mode /
alternate screen / crossterm `EventStream` / `terminal.draw`）を丸ごと外して起動します。
`nvim --headless --listen` と同型で、「UI なし」ではなく「UI の出力先がソケットになる」動作です。

- **必須**: `--headless` は `--listen` なしでは起動できません（操作不能になるため、
  clap の `requires` で起動時に弾かれます）
- **状態・観測面は無改修で同一**: `TuiState` は元々 TTY 非依存の純粋データなので、
  `state.get` / `pane.read` / `screen.capture` はヘッドレスでもそのまま動きます
- **`screen.capture` の既定サイズ**: 実ターミナルがないため、通常モードでの
  `terminal.size()` 取得失敗時と同じ 80x24 フォールバックが既定値になります
  (`width`/`height` を渡せば任意サイズで描画可能)
- **終了**: `:q!` / `:qa`（`command.exec` 経由）に加え、SIGINT / SIGTERM でも
  graceful shutdown します（キーボードがないため、外部からの唯一の中断経路）
- **端末前提機能の degrade**: clipboard は `NoClipboard` にフォールバックし、
  kitty keyboard enhancement 等は呼び出し自体をスキップします

```bash
copilot-quorum --headless --listen /tmp/quorum.sock &
scripts/tui-rpc.py /tmp/quorum.sock state.get
scripts/tui-rpc.py /tmp/quorum.sock screen.capture '{"width": 100, "height": 30}'
scripts/tui-rpc.py /tmp/quorum.sock command.exec '{"command": "q!"}'
```

実装: `TuiApp::run_headless`（`presentation/src/tui/app.rs`）。
デバッグ手順としての使い方は [CLAUDE.md](../../CLAUDE.md) の「Debugging the TUI」を参照。

---

## Methods

| Method | Params | 説明 |
|--------|--------|------|
| `state.get` | — | モード、モデル、タブ数、保留中 HiL、flash、フォーカス、入力下書き、レイアウト |
| `panes.list` | — | 全タブ/ペインのメタデータ(form, title, message_count, streaming, scroll) |
| `pane.read` | `{tab?, last?}` | 会話メッセージを構造化 JSON で取得(画面スクレイピング不要) |
| `input.send` | `{text}` | アクティブペインへプロンプト送信(`SubmitInput` と同一経路) |
| `command.exec` | `{command}` | `:` コマンド実行(`solo`, `tabnew ask`, `q` 等)。`q` はタブが複数あれば `{quit: false, flash}` でタブを閉じ、最後の 1 枚で `{quit: true}`。全体終了は `qa` |
| `interaction.spawn` | `{form, query}` | Agent/Ask/Discuss タブを生成 |
| `interaction.activate` | `{interaction_id}` | 指定インタラクションのタブへフォーカス |
| `hil.respond` | `{decision}` | 保留中の HiL モーダルに approve/reject を返す |
| `screen.capture` | `{width?, height?, styles?}` | オフスクリーン描画した画面をテキスト行(+スタイルラン)で取得 |
| `layout.get` | `{width?, height?}` | Surface ごとの Rect、preset、splits、route table、overlay 位置 |
| `layout.set` | `{preset}` | レイアウトプリセットをライブ切替(default/minimal/wide/stacked/カスタム) |
| `route.set` | `{content, surface}` | content slot → surface のルーティングをライブ変更 |
| `keys.feed` | `{keys: [...]}` | 生キー注入(`"j"`, `"Esc"`, `"Ctrl+w"` — Lua keymap と同じ記法) |

## Screen Visibility / 画面の可視化 (Phase 2)

AI エージェントが「見る → 変更する → 結果を確認する」ループを回すための API 群:

- **`screen.capture`** は現在の `TuiState` を `TestBackend` でオフスクリーン描画するため、
  ユーザーが見ている画面と同じ内容を任意のサイズで検証できる(実端末に影響なし)。
  行末の空白は `trim_end()` 済み。`styles: true` で行ごとのスタイルラン
  (`{start, end, fg, bg, mods}` — 列座標・end-exclusive、トリム前のグリッド基準)を返す。
  Help や HiL モーダル等のオーバーレイも描画結果に含まれる。
- **`layout.get`** はレイアウトジオメトリを計算だけして返す(描画なし)。
  `flex_fallback_active` で狭い端末での Minimal フォールバック発動が分かる。
- **`layout.set`** は Lua の `quorum.tui` と同じライブ変更パスを通るが、
  不明な preset 名は明示的にエラーを返す(Lua はサイレントにカスタム扱い)。
- **`keys.feed`** はキーボードと同一のディスパッチ経路(HiL モーダル、Lua keymap、
  組み込みバインド)。descriptor が 1 つでも不正ならバッチ全体を拒否。
  `$EDITOR` 起動(`I`)はリモートでは抑制され `editor_suppressed: true` が返る。
  `input.send` 同様、送信系の効果は非同期(RPC 応答は効果より先に返る)。

実装: `presentation/src/tui/remote.rs`

---

## Related / 関連

- [TUI Design](../explanation/tui-design.md) - ワイヤ形式・実行モデル・セキュリティの設計判断
- [TUI Internals](./tui-internals.md) - select! ループとチャネル構造
- [How to Use the TUI](../how-to/use-the-tui.md) - キーボードでの操作方法
- [CLI Reference](./cli.md) - `--listen` / `--headless` フラグ

<!-- LLM Context: Remote Control API。--listen <socket> で JSON-RPC 2.0 (LSP Content-Length フレーミング)。Methods: state.get, panes.list, pane.read, input.send, command.exec, interaction.spawn, interaction.activate, hil.respond, screen.capture(TestBackend オフスクリーン描画), layout.get/set, route.set, keys.feed(キーボードと同一ディスパッチ、$EDITOR 抑制)。socket 0600、TCP なし。実装: presentation/src/tui/remote.rs。リファレンスクライアント: scripts/tui-rpc.py。--headless（#303）: raw mode/alternate screen/EventStream/terminal.draw なしで同じイベントループ・TuiState を --listen 経由のみで提供。--listen 必須（clap requires）。screen.capture 既定 80x24。:q!/:qa/SIGINT/SIGTERM で終了。実装: TuiApp::run_headless (presentation/src/tui/app.rs)。 -->
