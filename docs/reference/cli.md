# CLI Reference / CLI リファレンス

> Command-line flags, REPL slash commands and TUI command-mode commands
>
> CLI フラグ、REPL スラッシュコマンド、TUI コマンドモードの一覧

---

## CLI Options / CLI オプション

`copilot-quorum [OPTIONS] [QUESTION]` — QUESTION を与えるとワンショット実行、省略すると対話モード（TUI）で起動します。

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

定義ファイル: `presentation/src/cli/commands.rs`

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
| `:q` | 終了 |

モード操作やキーバインドの全体像は [How to Use the TUI](../how-to/use-the-tui.md) を参照してください。

定義ファイル: `presentation/src/tui/app_tab_command.rs`, `presentation/src/tui/mode.rs`

---

## Related / 関連

- [Configuration Reference](./configuration.md) - init.lua による設定
- [How to Use the TUI](../how-to/use-the-tui.md) - モーダル操作の使い方
- [Orchestration Axes](../explanation/orchestration-axes.md) - `/solo` `/scope` `/strategy` が変更する 3 軸の意味

<!-- LLM Context: CLI フラグは presentation/src/cli/commands.rs で定義。--solo/--ensemble(排他), --no-quorum, -m/--model(複数可), --final-review, -w/--working-dir, -o/--output, -v(count), --show-votes, -q/--quiet, --log-dir, --no-log-file, --show-config, --listen(Remote Control API)。--chat/--config/--moderator/--no-review フラグは存在しない(旧ドキュメントの残骸)。REPL: /solo /ens /fast /scope /strategy /council /init /config /clear /quit。TUI command mode: agent/ask/discuss <query>, tabnew, tabs, tabclose。 -->
