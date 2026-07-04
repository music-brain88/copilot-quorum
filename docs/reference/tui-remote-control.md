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

# 別プロセスから操作（copilot-quorum rpc がビルトインクライアント — #302）
copilot-quorum rpc --socket /tmp/quorum.sock state.get
copilot-quorum rpc --socket /tmp/quorum.sock input.send '{"text": "Fix the bug in login.rs"}'
copilot-quorum rpc --socket /tmp/quorum.sock pane.read '{"last": 5}'
copilot-quorum rpc --socket /tmp/quorum.sock hil.respond '{"decision": "approve"}'

# 画面を「見る」・レイアウトを調整する (Phase 2)
copilot-quorum rpc --socket /tmp/quorum.sock screen.capture '{"width": 120, "height": 40}'
copilot-quorum rpc --socket /tmp/quorum.sock layout.get
copilot-quorum rpc --socket /tmp/quorum.sock layout.set '{"preset": "stacked"}'
copilot-quorum rpc --socket /tmp/quorum.sock keys.feed '{"keys": ["i", "h", "i", "Esc"]}'

# 何が呼べるか分からないときは discover から (Phase 3 — #302)
copilot-quorum rpc --socket /tmp/quorum.sock rpc.discover
```

ワイヤ形式は LSP スタイルの `Content-Length` フレーミング + JSON-RPC 2.0 です。
設計判断の背景は [TUI Design](../explanation/tui-design.md) を参照してください。

`copilot-quorum rpc` はリポジトリ checkout も Python も不要な、インストール済みバイナリ
1 個で完結するビルトインクライアント（#302）。ワイヤ形式は `scripts/tui-rpc.py`
（プロトコル参照実装として引き続き残っている）と完全互換なので、どちらを使っても同じ
ソケットを操作できます:

```bash
# 上と同じ操作を tui-rpc.py（参照実装）で行う場合
scripts/tui-rpc.py /tmp/quorum.sock state.get
```

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
copilot-quorum rpc --socket /tmp/quorum.sock state.get
copilot-quorum rpc --socket /tmp/quorum.sock screen.capture '{"width": 100, "height": 30}'
copilot-quorum rpc --socket /tmp/quorum.sock command.exec '{"command": "q!"}'
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
| `interaction.spawn` | `{form, query}` | Agent/Ask/Discuss/Review タブを生成（`form: "review"` の `query` は diff テキストそのもの。PR メタデータ/focus 込みのヘッドレス実行は `copilot-quorum review` サブコマンド — #300） |
| `interaction.activate` | `{interaction_id}` | 指定インタラクションのタブへフォーカス |
| `hil.respond` | `{decision}` | 保留中の HiL モーダルに approve/reject を返す |
| `screen.capture` | `{width?, height?, styles?}` | オフスクリーン描画した画面をテキスト行(+スタイルラン)で取得 |
| `layout.get` | `{width?, height?}` | Surface ごとの Rect、preset、splits、route table、overlay 位置 |
| `layout.set` | `{preset}` | レイアウトプリセットをライブ切替(default/minimal/wide/stacked/カスタム) |
| `route.set` | `{content, surface}` | content slot → surface のルーティングをライブ変更 |
| `keys.feed` | `{keys: [...]}` | 生キー注入(`"j"`, `"Esc"`, `"Ctrl+w"` — Lua keymap と同じ記法) |
| `rpc.discover` | — | 全メソッドの一覧(params スキーマ + 説明 + `api_version`) |
| `commands.list` | — | `:` コマンド一覧(builtin + Lua `quorum.command.register` 登録分) |
| `config.keys` | — | 全設定キー(description/mutable/valid_values) |
| `config.get` | `{key}` | 設定キーの現在値 |
| `config.set` | `{key, value}` | 設定キーを変更(Lua `quorum.config.set` / `:config` と同じ `ConfigAccessorPort` を経由) |
| `keymaps.list` | — | キーバインド一覧(builtin + Lua `quorum.keymap.set` 登録分) |

## Introspection & Config / 内省と設定操作 (Phase 3, #302)

「人間が TUI で触れる領域・設定できる領域は、全て API からも可読・可設定」という原則を
仕上げるメソッド群。`dispatch()` と `rpc.discover` は同じメソッドメタデータテーブル
(`remote.rs` の `METHODS`)を読むため、メソッドを追加してどちらかを更新し忘れる、という
ズレが構造的に起きません。

- **`rpc.discover`** — LSP の `capabilities` / MCP の `tools/list` / `nvim_get_api_info`
  と同じ発想のケーパビリティ発見。各メソッドの `params_schema` は手書きの JSON Schema
  断片(厳密なバリデーション用ではなく発見用)。`api_version` はレスポンス形状に破壊的
  変更が入ったときだけ上げる(メソッド追加は破壊的変更に数えない)。
- **`commands.list`** — `:` コマンドは builtin(`command_registry.rs`)と Lua 登録分
  (`quorum.command.register` 経由、`ScriptingEnginePort::registered_commands()`)を
  合成して返す。Lua コマンドは静的ドキュメントに原理的に載らないため、ランタイム自己
  記述だけが真実を語れる。Help オーバーレイ(`?`)の "Commands" セクションも同じ
  `command_registry` から生成されるため、画面と API が乖離しない。
- **`config.keys` / `config.get` / `config.set`** — Lua `quorum.config.*` API や TUI の
  `:config` コマンドと同じ `Arc<Mutex<QuorumConfig>>` / `ConfigAccessorPort` を経由する
  ため、バリデーション(例: Solo + Debate の組み合わせ警告)も含めて 3 面で挙動が同一。
  `value` は JSON の string/bool/number/array(文字列限定)を `ConfigValue` に変換する
  (Lua 側の変換規則と同じ)。
- **`keymaps.list`** — builtin キーバインド(`keymap_registry.rs`)と Lua
  `quorum.keymap.set` 登録分(`ScriptingEnginePort::registered_keymaps()`)を合成。
  `key` は Lua keymap 記法の記述子だが、`"gg"` / `"yy"` のような複数キーの prefix
  chord は単体の `keys.feed` エントリとしては解釈できない点に注意(ドキュメント目的の
  表記)。
- 未知の method / config key のエラーには "see rpc.discover" / "see config.keys" の
  ヒントが付く(#278 の親切エラーの流儀)。

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
- [CLI Reference](./cli.md) - `--listen` / `--headless` / `rpc` サブコマンド

<!-- LLM Context: Remote Control API。--listen <socket> で JSON-RPC 2.0 (LSP Content-Length フレーミング)。Methods: state.get, panes.list, pane.read, input.send, command.exec, interaction.spawn(form: agent|ask|discuss|review), interaction.activate, hil.respond, screen.capture(TestBackend オフスクリーン描画), layout.get/set, route.set, keys.feed(キーボードと同一ディスパッチ、$EDITOR 抑制), rpc.discover(全メソッド一覧+params_schema+api_version), commands.list(builtin+Lua quorum.command.register), config.keys/get/set(ConfigAccessorPort経由、Lua/`:config`と挙動同一), keymaps.list(builtin+Lua quorum.keymap.set)。socket 0600、TCP なし。実装: presentation/src/tui/remote.rs(METHODS テーブルが dispatch と rpc.discover の single source of truth)、command_registry.rs、keymap_registry.rs。クライアント: `copilot-quorum rpc --socket PATH method [params]`(ビルトイン、#302)。scripts/tui-rpc.py はプロトコル参照実装として引き続き存在(同じワイヤ形式)。--headless（#303）: raw mode/alternate screen/EventStream/terminal.draw なしで同じイベントループ・TuiState を --listen 経由のみで提供。--listen 必須（clap requires）。screen.capture 既定 80x24。:q!/:qa/SIGINT/SIGTERM で終了。実装: TuiApp::run_headless (presentation/src/tui/app.rs)。TuiApp::run_headless_until(interaction_id)（#300）: --listen 任意、特定の interaction の完了(InteractionCompletedEvent.result)または失敗(*Error UiEvent)を待って InteractionOutcome を返す。copilot-quorum review サブコマンド（cli/src/review.rs）が使用。 -->
