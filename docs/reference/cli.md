# CLI Reference / CLI リファレンス

> Command-line flags, REPL slash commands and TUI command-mode commands
>
> CLI フラグ、REPL スラッシュコマンド、TUI コマンドモードの一覧

---

## CLI Options / CLI オプション

`copilot-quorum [OPTIONS] [QUESTION]` — QUESTION を与えるとワンショット実行、省略すると対話モード（TUI）で起動します。
`copilot-quorum <COMMAND>` — サブコマンド（現在は `review` のみ、[後述](#review-subcommand--review-サブコマンド-300)）。グローバルフラグはサブコマンド名の**前**に置きます（例: `copilot-quorum -v review --pr 123`）。

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

TUI の Command モード（`:`）で使えるコマンド（抜粋）:

| Command | Description |
|---------|-------------|
| `:agent <query>` | 新しい Agent タブを開いてタスク実行 |
| `:ask <query>` | 新しい Ask（Q&A）タブを開く |
| `:discuss <query>` | 新しい Discuss（Quorum Discussion）タブを開く |
| `:tabnew [form]` | 新規タブ作成（form: agent / ask / discuss） |
| `:tabs` | タブ一覧を表示 |
| `:tabclose` | アクティブタブを閉じる |
| `:q` / `:quit` | 複数タブ時はアクティブタブを閉じる（`:tabclose` 相当）。最後の 1 枚ではアプリ終了 |
| `:qa` / `:qall` | 全タブを閉じてアプリ終了（Vim 準拠） |

モード操作やキーバインドの全体像は [How to Use the TUI](../how-to/use-the-tui.md) を参照してください。

定義ファイル: `presentation/src/tui/app_tab_command.rs`, `presentation/src/tui/mode.rs`

---

## Related / 関連

- [Configuration Reference](./configuration.md) - init.lua による設定
- [How to Use the TUI](../how-to/use-the-tui.md) - モーダル操作の使い方
- [Orchestration Axes](../explanation/orchestration-axes.md) - `/solo` `/scope` `/strategy` が変更する 3 軸の意味
- [TUI Remote Control API](./tui-remote-control.md) - `--headless --listen` と `interaction.spawn`（`review` form 含む）

<!-- LLM Context: CLI フラグは presentation/src/cli/commands.rs で定義。--solo/--ensemble(排他), --no-quorum, -m/--model(複数可), --final-review, -w/--working-dir, -o/--output, -v(count), --show-votes, -q/--quiet, --log-dir, --no-log-file, --show-config, --listen(Remote Control API), --headless(--listen 必須、TTY なしでイベントループのみ起動 — #303)。--chat/--config/--moderator/--no-review フラグは存在しない(旧ドキュメントの残骸)。サブコマンド: review(#300, --pr|--diff|stdin, --focus, --output synthesis|json, exit 0/1/2)。グローバルフラグはサブコマンド名より前に置く必要がある(args_conflicts_with_subcommands は使っていない — 使うとサブコマンド名がグローバルフラグの後で認識されなくなる罠あり)。REPL: /solo /ens /fast /scope /strategy /council /init /config /clear /quit。TUI command mode: agent/ask/discuss <query>, tabnew, tabs, tabclose, q/quit(タブ数>1 で tabclose 相当・最後の1枚で終了), qa/qall(全体終了)。 -->
