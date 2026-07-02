# How to Use the Modal TUI / モーダル TUI を使う

> Neovim ライクなモーダルインターフェースの使い方
>
> 設計思想は [TUI Design](../explanation/tui-design.md) を参照

---

## Overview / 概要

copilot-quorum の TUI は 3 つのモード（Normal, Insert, Command）を持つモーダルインターフェースです。
**3 つの入力手段が 3 つのニーズ粒度に対応**しており、状況に応じて最適な方法で LLM に指示を送れます。

```
一言で済む           対話的に書く          がっつり書く
:ask Fix the bug     i で入力モード       I で $EDITOR 起動
    ↓                    ↓                    ↓
COMMAND モード        INSERT モード         $EDITOR (vim/neovim)
```

タブは Vim のタブページに相当し、Agent / Ask / Discuss の各インタラクションが
独立したタブで動作します。各タブは独立した入力バッファを持つため、
**タブ切り替え時に下書きが保持**されます。
タブタイトルは最初のユーザーメッセージから自動生成されます（30 文字で切り詰め）。

---

## Modes / モード

### Normal モード

起動後のホームポジション。オーケストレーション操作を行うモードです。

| キー | アクション |
|------|-----------|
| `i` | INSERT モードへ |
| `I` | $EDITOR を起動（INSERT バッファの内容を引き継ぎ） |
| `:` | COMMAND モードへ |
| `s` | Solo モードに切り替え |
| `e` | Ensemble モードに切り替え |
| `f` | Fast/Full スコープトグル |
| `a` | Ask（`:ask ` プリフィル） |
| `d` | Discuss（`:discuss ` プリフィル） |
| `j` / `k` / `↓` / `↑` | 会話バッファスクロール |
| `gg` | バッファ先頭 |
| `G` | バッファ末尾 |
| `gt` | 次のタブ |
| `gT` | 前のタブ |
| `?` | ヘルプ表示 |
| `Ctrl+C` | 終了 |

### Insert モード

LLM の応答パネルを見ながらプロンプトを入力するモード。**マルチライン入力に対応**しており、
複雑な指示を構造的に書けます。

| キー | アクション |
|------|-----------|
| `Enter` | 入力を送信（Agent 実行） |
| `Shift+Enter` | 改行を挿入（マルチライン入力） |
| `Alt+Enter` | 改行を挿入（フォールバック） |
| `Esc` | NORMAL モードに戻る（送信せず） |
| `Backspace` | 1文字削除（改行も削除可能） |
| `←` / `→` | カーソル移動（行をまたぐ） |
| `Home` / `End` | 行頭/行末 |

> **Note:** `Shift+Enter` は kitty keyboard protocol 対応ターミナル（Alacritty, kitty, WezTerm, foot）で動作します。
> 非対応ターミナルでは `Alt+Enter` をフォールバックとして使用してください。

#### マルチライン入力

入力エリアは内容に応じて **動的にリサイズ** します（1行 → 最大 `max_height` 行）。
長い入力はスクロールされ、カーソル行が常に見える状態を維持します。

```
solo> 以下の要件でリファクタリングしてほしい:
  1. エラーハンドリングを Result 型に統一
  2. 重複したバリデーションロジックを共通化
  3. テストカバレッジ 80% 以上
```

#### エスカレーション（INSERT → $EDITOR）

INSERT モードで書き始めて「長くなるな」と思ったら:

1. `Esc` で NORMAL に戻る
2. `I` で $EDITOR を起動
3. INSERT バッファの書きかけ内容が **初期テキスト**として $EDITOR に渡される

### Command モード

`:` で起動する ex コマンドモード。

| キー | アクション |
|------|-----------|
| `Enter` | コマンドを実行 |
| `Esc` | NORMAL モードに戻る（実行せず） |

---

## Commands / コマンド

COMMAND モード（`:`）で使用できるコマンド一覧:

### LLM への入力

| コマンド | 説明 |
|---------|------|
| `:ask <prompt>` | 新しいタブで Ask インタラクションを開始 |
| `:discuss <question>` | 新しいタブで Quorum Discussion を開始 |
| `:agent <prompt>` | 新しいタブで Agent インタラクションを開始 |

### モード・設定変更

| コマンド | エイリアス | 説明 |
|---------|-----------|------|
| `:solo` | | Solo モードに切り替え |
| `:ens` | `:ensemble` | Ensemble モードに切り替え |
| `:fast` | | PhaseScope の Fast/Full トグル |
| `:scope <scope>` | | PhaseScope を変更 (full, fast, plan-only) |
| `:strategy <strategy>` | | OrchestrationStrategy を変更 (quorum, debate) |

### タブ管理

| コマンド | 説明 |
|---------|------|
| `:tabs` | 開いているタブの一覧を表示 |
| `:tabnew [form]` | 新しいタブを作成 (agent/ask/discuss、デフォルト: agent) |
| `:tabclose` | アクティブタブを閉じる（最後の 1 つは閉じられない） |

### セッション管理

| コマンド | 説明 |
|---------|------|
| `:config [section]` | 現在の設定を表示（全キー、セクション絞り込み可: `:config models`） |
| `:clear` | 会話履歴をクリア |
| `:init [--force]` | プロジェクトコンテキストを初期化 |
| `:help` | ヘルプを表示 |
| `:q` / `:quit` / `:exit` | 終了 |

---

## Tab System / タブシステム

`:ask`, `:discuss`, `:agent` コマンドでクエリ付きのタブを作成できます
（内部の生成フローは [TUI Internals](../reference/tui-internals.md) を参照）。

### タブ切り替え

| 操作 | 説明 |
|------|------|
| `gt` (Normal モード) | 次のタブ（循環） |
| `gT` (Normal モード) | 前のタブ（循環） |

### タブ一覧

`:tabs` コマンドでタブ一覧を表示:

```
  1: [Fix the auth bug]     ← Agent タブ（タイトル自動生成）
> 2: [What is DDD?]         ← Ask タブ（アクティブ）
  3: [Discuss]               ← Discuss タブ（未使用、デフォルトタイトル）
```

---

## $EDITOR Integration / $EDITOR 連携

NORMAL モードで `I`（Shift+i）を押すと `$EDITOR`（vim/neovim 等）を全画面起動します。
`git commit` が `$EDITOR` を呼ぶのと同じパターンです。

- 保存して終了（`:wq`）→ 内容が INSERT バッファに入り、INSERT モードに遷移
- 保存せず終了（`:q!`）→ キャンセル、NORMAL モードのまま

### エディタ検出順序

1. `$VISUAL` 環境変数
2. `$EDITOR` 環境変数
3. `vi`（フォールバック）

### コンテキストヘッダー

起動時にコンテキスト情報がコメント行（`#`）で表示されます。
`#` で始まる行はプロンプト送信時に自動除去されます。

```
# --- Quorum Prompt ---
# Mode: Solo | Scope: Full | Strategy: Quorum
# Write your prompt below. Lines starting with # are ignored.
# Save and quit to send, quit without saving to cancel.
# ---------------------

```

コンテキストヘッダーは `quorum.config.set("tui.input.context_header", false)` で非表示にできます。

### TUI サスペンド/レジューム

$EDITOR 起動中は TUI が一時停止します（raw mode 解除、alternate screen 退出）。
エディタ終了後に TUI が自動復帰し、バックグラウンドで受信した LLM 応答が反映されます。

---

## Input Configuration / 入力設定

`~/.config/copilot-quorum/init.lua` の `tui.input.*` キーで入力動作をカスタマイズできます:

```lua
quorum.config.set("tui.input.submit_key", "enter")           -- メッセージ送信キー
quorum.config.set("tui.input.newline_key", "shift+enter")    -- 改行挿入キー（マルチライン）
quorum.config.set("tui.input.editor_key", "I")               -- $EDITOR 起動キー（Normal モード）
quorum.config.set("tui.input.editor_action", "return_to_insert")  -- エディタ後: "return_to_insert" or "submit"
quorum.config.set("tui.input.max_height", 10)                -- 入力エリアの最大行数
quorum.config.set("tui.input.dynamic_height", true)          -- 内容に応じた動的リサイズ
quorum.config.set("tui.input.context_header", true)          -- $EDITOR でコンテキストヘッダーを表示
```

レイアウトのカスタマイズ（`tui.layout.*`）を含む全キーは
[Configuration Reference](../reference/configuration.md) を参照してください。

---

## Related / 関連

- [TUI Design](../explanation/tui-design.md) — 設計思想の詳細
- [TUI Internals](../reference/tui-internals.md) — Actor パターン・イベントルーティング
- [TUI Remote Control API](../reference/tui-remote-control.md) — 外部プロセスからの操作
- [Discussion #58: Neovim-Style Extensible TUI](https://github.com/music-brain88/copilot-quorum/discussions/58) — 元の提案
- [Configuration Reference](../reference/configuration.md) — 設定オプション

<!-- LLM Context: TUI の使い方。3 モード (Normal, Insert, Command)。入力 3 粒度 (:ask=COMMAND即時, i=INSERT対話的マルチライン, I=$EDITOR全画面)。NORMAL キー: i/I/:/s(solo)/e(ensemble)/f(fast)/a(ask)/d(discuss)/j/k/gg/G/gt/gT/?/Ctrl+C。INSERT: Enter送信, Shift+Enter改行(kitty protocol), Alt+Enterフォールバック。COMMAND: :ask/:discuss/:agent(タブ生成), :solo/:ens/:fast/:scope/:strategy, :tabs/:tabnew/:tabclose, :config/:clear/:init/:help/:q。$EDITOR は $VISUAL→$EDITOR→vi 検出、TUI サスペンド→レジューム。設定は tui.input.* Lua キー。内部構造は reference/tui-internals.md、設計思想は explanation/tui-design.md、Remote Control API は reference/tui-remote-control.md。 -->
