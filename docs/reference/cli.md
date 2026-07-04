# CLI Reference / CLI リファレンス

> Command-line flags, REPL slash commands and TUI command-mode commands
>
> CLI フラグ、REPL スラッシュコマンド、TUI コマンドモードの一覧

---

## CLI Options / CLI オプション

`copilot-quorum [OPTIONS] [QUESTION]` — QUESTION を与えるとワンショット実行、省略すると対話モード（TUI）で起動します。

`copilot-quorum <SUBCOMMAND>` — サブコマンド（RFC Discussion #304 D4）。グローバルフラグは
サブコマンド名の**前**に置きます（例: `copilot-quorum -v review --pr 123`）。clap の
`args_conflicts_with_subcommands` は使っていません — この属性を付けると、サブコマンド名より
前に他のトップレベルフラグ/値が1つでもあった場合に clap がサブコマンドとして認識しなくなり
（例: `--log-dir X review --pr 123` が `review` を文字通りの QUESTION として実行してしまう）、
`presentation/src/cli/commands.rs` の回帰テストで検証済みです。現状のサブコマンド:

| Subcommand | Description |
|------------|-------------|
| `review` | PR/diff の多モデル Quorum レビューをヘッドレスで実行（下記参照、#300） |
| `rpc` | Remote Control API のビルトイン JSON-RPC クライアント（下記参照、#302） |

| Option | Short | Description |
|--------|-------|-------------|
| `--solo` | | Solo モードで起動（`--ensemble` と排他） |
| `--ensemble` | | Ensemble モードで起動（`--solo` と排他） |
| `--no-quorum` | | Quorum レビューをスキップ（高速実行） |
| `--model <MODEL>` | `-m` | モデル指定（複数可） |
| `--final-review` | | 実行後の Final Review を有効化 |
| `--working-dir <PATH>` | `-w` | エージェントの作業ディレクトリ |
| `--output <FORMAT>` | `-o` | 出力形式 (`full` / `synthesis` / `json`) |
| `--verbose` | `-v` | 詳細ログ（`-vv`, `-vvv` で段階的に増加） |
| `--show-votes` | | 投票の詳細を表示 |
| `--quiet` | `-q` | プログレス表示を抑制 |
| `--log-dir <PATH>` | | 会話ログの出力先ディレクトリ |
| `--no-log-file` | | 会話ログファイルを無効化 |
| `--show-config` | | 解決された設定と init.lua パスを表示して終了 |
| `--listen <PATH>` | | Remote Control API のソケットを開いて TUI を起動 |
| `--headless` | | 実ターミナルなしでイベントループを起動（`--listen` 必須。詳細は [tui-remote-control.md](./tui-remote-control.md#headless-mode--ヘッドレスモード-303)） |

定義ファイル: `presentation/src/cli/commands.rs`

---

## `review` Subcommand / `review` サブコマンド (#300)

PR/diff を入力に、`models.review` の複数モデルで多数決レビューし、`models.moderator` が投票+フィードバックを 1 本の統合レビューに合成する、非対話・読み取り専用（ツール実行なし・HiL なし）のヘッドレスコマンドです。TUI を経由しない外部消費（CI ゲート、他コックピットからの呼び出し）向けの第一歩（RFC Discussion #304 D2）。

```bash
copilot-quorum review --pr 123                          # gh CLI で diff + title を取得
git diff main...feature | copilot-quorum review          # stdin から diff を読む(gh 非依存)
copilot-quorum review --diff changes.patch --focus "並行処理の安全性"
copilot-quorum review --pr 123 --output json              # quorum_result v1 JSON を stdout に
```

| Option | Description |
|--------|-------------|
| `--pr <NUMBER>` | PR 番号（`gh pr diff` / `gh pr view --json title` で diff + title を取得。`--diff` と排他） |
| `--diff <PATH>` | diff/patch ファイルのパス（`--pr` も `--diff` も省略すると stdin から読む） |
| `--focus <TEXT>` | レビューの観点をモデルに指示（例: `"並行処理の安全性"`） |
| `--output <FORMAT>` | `synthesis`（デフォルト。moderator の統合レビュー Markdown をそのまま出力）または `json`（`quorum_result` v1 の構造化 JSON。`votes` / `synthesis` / `target: {pr, title}` を含む） |

**終了コード**: `0` = 合議承認 / `1` = 合議否認 / `2` = 実行エラー（diff 取得失敗、全モデル不達など）。CI ゲートとして使えます。

実行結果は既存の JSONL 会話ログにも `quorum_result`（`topic: "pr_review"`）として記録され、Lua の `quorum.on("QuorumResult", fn)` でも観測できます（`--log-dir` / `--no-log-file` は他のモードと共通）。

内部的には `InteractionForm::Review`（Agent/Ask/Discuss と対等な第 4 の interaction form）としてヘッドレス App 上で実行されるため、`interaction.spawn {"form": "review", "query": "<diff>"}`（Remote Control API）や将来の `:review` コマンドからも同じ経路で呼び出せます。

定義ファイル: `presentation/src/cli/commands.rs`（`ReviewArgs`）, `cli/src/review.rs`, `application/src/use_cases/run_review.rs`

---

## `rpc` Subcommand / `rpc` サブコマンド (#302)

`--listen`（または `--headless --listen`）で起動した TUI/ヘッドレスインスタンスの
ソケットを、リポジトリ checkout も Python も要らないビルトインクライアントで操作します。
ワイヤ形式は `scripts/tui-rpc.py`（プロトコル参照実装として引き続き存在）と完全互換
（LSP スタイル `Content-Length` フレーミング + JSON-RPC 2.0）。

```bash
copilot-quorum rpc --socket PATH <method> [params-json]

# 例
copilot-quorum rpc --socket /tmp/quorum.sock rpc.discover
copilot-quorum rpc --socket /tmp/quorum.sock state.get
copilot-quorum rpc --socket /tmp/quorum.sock config.get '{"key": "agent.strategy"}'
copilot-quorum rpc --socket /tmp/quorum.sock config.set '{"key": "agent.strategy", "value": "debate"}'
copilot-quorum rpc --socket /tmp/quorum.sock command.exec '{"command": "qa!"}'
```

`params-json` を省略すると `{}` として送信されます。結果は整形 JSON で stdout に、
エラー（JSON-RPC の `error` オブジェクト）は stderr に出力され、終了コードは
成功時 `0` / エラー時 `1` です。呼べるメソッドの全量は `rpc.discover` を参照してください
（[tui-remote-control.md](./tui-remote-control.md) にメソッド一覧あり）。

定義ファイル: `presentation/src/cli/commands.rs`（`RpcArgs`）, `presentation/src/cli/rpc_client.rs`

---

## REPL Commands / REPL コマンド

対話モードで使用できるスラッシュコマンド一覧:

| Command | Aliases | Description |
|---------|---------|-------------|
| `/help` | `/h`, `/?` | ヘルプを表示 |
| `/mode <mode>` | | 合意レベルを変更 (solo, ensemble) |
| `/solo` | | Solo モードに切り替え（単一モデル、高速実行） |
| `/ens` | `/ensemble` | Ensemble モードに切り替え（マルチモデル計画生成） |
| `/fast` | | PhaseScope を Fast に切り替え（レビュースキップ） |
| `/scope <scope>` | | フェーズスコープを変更 (full, fast, plan-only) |
| `/strategy <strategy>` | | 戦略を変更 (quorum, debate) |
| `/ask` | | (再設計予定 — Issue #119) |
| `/discuss` | | (再設計予定 — Issue #119、`/council <question>` を使用) |
| `/council <question>` | | Quorum Discussion を実行（複数モデルに相談） |
| `/init [--force]` | | プロジェクトコンテキストを初期化 |
| `/config [section]` | | 現在の設定を表示（全キー、セクション絞り込み可: `/config models`） |
| `/clear` | | 会話履歴をクリア |
| `/verbose` | | Verbose モードの状態を表示 |
| `/quit` | `/exit`, `/q` | 終了 |

### Prompt Display / プロンプト表示

プロンプトは現在の ConsensusLevel に応じて変わります:

| ConsensusLevel | Prompt |
|----------------|--------|
| Solo | `solo>` |
| Ensemble | `ens>` |

---

## TUI Command Mode / TUI コマンドモード

TUI の Command モード（`:`）で使えるコマンド全量。この一覧は
`presentation/src/tui/command_registry.rs` が single source of truth — Help
オーバーレイ（`?`）の "Commands" セクションと Remote Control API の
`commands.list`（#302）はどちらもここから生成されるため、この表と実際の挙動が
構造的に乖離しません（Lua `quorum.command.register` で追加したコマンドは
`commands.list` にのみ現れます — 静的ドキュメントには原理的に載らないため）。

| Command | Aliases | Description |
|---------|---------|-------------|
| `:q` | `:quit` | 複数タブ時はアクティブタブを閉じる。最後の 1 枚ではアプリ終了 |
| `:qa` | `:qall`, `:quitall`, `:exit` | 全タブを閉じてアプリ終了（Vim 準拠） |
| `:help` | `:h`, `:?` | ヘルプオーバーレイを表示 |
| `:solo` | | Solo モードに切り替え |
| `:ens` | `:ensemble` | Ensemble モードに切り替え |
| `:fast` | | Fast phase scope をトグル |
| `:mode <solo\|ensemble>` | | 合意レベルを明示的に変更 |
| `:scope <full\|fast\|plan-only>` | | フェーズスコープを明示的に変更 |
| `:strategy <quorum\|debate>` | | オーケストレーション戦略を変更 |
| `:agent <task>` | | 新しい Agent タブを開いてタスク実行 |
| `:ask <question>` | | 新しい Ask（Q&A）タブを開く |
| `:discuss <question>` | | 新しい Discuss（Quorum Discussion）タブを開く |
| `:council <question>` | | アクティブタブ内で Quorum Discussion を実行（新規タブなし） |
| `:tabnew [agent\|ask\|discuss]` | | 新規タブ作成（既定 agent） |
| `:tabclose` | | アクティブタブを閉じる |
| `:tabs` | | タブ一覧を表示 |
| `:config [section]` | | 現在の設定を表示（セクション絞り込み可: `:config models`） |
| `:clear` | | 会話履歴をクリア |
| `:init[!]` | | プロジェクトコンテキストを初期化（`!` で強制再実行） |
| `:verbose` | | Verbose モードの状態を表示 |

モード操作やキーバインドの全体像は [How to Use the TUI](../how-to/use-the-tui.md) を参照してください。

定義ファイル: `presentation/src/tui/command_registry.rs`（メタデータ）,
`presentation/src/tui/app_tab_command.rs`（quit/tab 系のディスパッチ）,
`application/src/use_cases/agent_controller.rs`（mode/config/interaction 系のディスパッチ）

---

## Related / 関連

- [Configuration Reference](./configuration.md) - init.lua による設定
- [How to Use the TUI](../how-to/use-the-tui.md) - モーダル操作の使い方
- [Orchestration Axes](../explanation/orchestration-axes.md) - `/solo` `/scope` `/strategy` が変更する 3 軸の意味
- [TUI Remote Control API](./tui-remote-control.md) - `--headless --listen`、`interaction.spawn`（`review` form 含む）、`rpc.discover` / `commands.list` / `config.*` / `keymaps.list`

<!-- LLM Context: CLI フラグは presentation/src/cli/commands.rs で定義。--solo/--ensemble(排他), --no-quorum, -m/--model(複数可), --final-review, -w/--working-dir, -o/--output, -v(count), --show-votes, -q/--quiet, --log-dir, --no-log-file, --show-config, --listen(Remote Control API), --headless(--listen 必須、TTY なしでイベントループのみ起動 — #303)。--chat/--config/--moderator/--no-review フラグは存在しない(旧ドキュメントの残骸)。サブコマンド(RFC #304 D4。args_conflicts_with_subcommands は使っていない — 使うとサブコマンド名より前に他のトップレベルフラグ/値があるとサブコマンドとして認識されなくなる罠があり、回帰テストで検証済み。グローバルフラグはサブコマンド名より前に置ける): `review`(#300, --pr|--diff|stdin, --focus, --output synthesis|json, exit 0/1/2), `rpc --socket PATH <method> [params-json]`(#302, Remote Control API のビルトイン JSON-RPC クライアント, presentation/src/cli/rpc_client.rs)。REPL: /solo /ens /fast /scope /strategy /council /init /config /clear /quit。TUI command mode(command_registry.rs が single source of truth、Help オーバーレイと commands.list RPC の両方がここから生成): q/quit, qa/qall/quitall/exit, help/h/?, solo, ens/ensemble, fast, mode, scope, strategy, agent/ask/discuss <query>, council, tabnew, tabclose, tabs, config, clear, init, verbose。 -->
