# Modal TUI / モーダル TUI

> Neovim ライクなモーダルインターフェースの使い方
>
> 設計思想は [architecture.md](../reference/architecture.md#tui-design-philosophy--tui-設計思想) を参照

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

---

## Modes / モード

### Normal モード

起動後のホームポジション。オーケストレーション操作を行うモードです。

| キー | アクション |
|------|-----------|
| `i` / `a` | INSERT モードへ |
| `I` | $EDITOR を起動（INSERT バッファの内容を引き継ぎ） |
| `:` | COMMAND モードへ |
| `s` | Solo モードに切り替え |
| `e` | Ensemble モードに切り替え |
| `f` | Fast/Full スコープトグル |
| `d` | Quorum Discussion 開始（`:discuss ` プリフィル） |
| `j` / `k` / `↓` / `↑` | 会話バッファスクロール |
| `g` | バッファ先頭 |
| `G` | バッファ末尾 |
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
solo:ask> 以下の要件でリファクタリングしてほしい:
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

`:` で起動する ex コマンドモード。ファーストクラスコマンドとして `:ask` と `:discuss` を提供します。

| キー | アクション |
|------|-----------|
| `Enter` | コマンドを実行 |
| `Esc` | NORMAL モードに戻る（実行せず） |

---

## Commands / コマンド

COMMAND モード（`:`）で使用できるコマンド一覧:

### LLM への入力

| コマンド | 説明 | 状態 |
|---------|------|------|
| `:ask <prompt>` | Solo で即座に質問（Agent 実行） | 未実装 |
| `:discuss <question>` | Quorum Discussion を開始 | 実装済 |

### モード・設定変更

| コマンド | エイリアス | 説明 |
|---------|-----------|------|
| `:solo` | | Solo モードに切り替え |
| `:ens` | `:ensemble` | Ensemble モードに切り替え |
| `:fast` | | PhaseScope の Fast/Full トグル |
| `:scope <scope>` | | PhaseScope を変更 (full, fast, plan-only) |
| `:strategy <strategy>` | | OrchestrationStrategy を変更 (quorum, debate) |

### セッション管理

| コマンド | 説明 |
|---------|------|
| `:config` | 現在の設定を表示 |
| `:clear` | 会話履歴をクリア |
| `:init [--force]` | プロジェクトコンテキストを初期化 |
| `:help` | ヘルプを表示 |
| `:q` | 終了 |
| `:quit` / `:exit` | 終了 |

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

コンテキストヘッダーは `[tui.input]` の `context_header = false` で非表示にできます。

### TUI サスペンド/レジューム

$EDITOR 起動中は TUI が一時停止します（raw mode 解除、alternate screen 退出）。
エディタ終了後に TUI が自動復帰し、バックグラウンドで受信した LLM 応答が反映されます。

### INSERT → $EDITOR エスカレーション

INSERT モードで書き始めて「長くなるな」と思ったら:

1. `Esc` で NORMAL に戻る
2. `I` で $EDITOR を起動
3. INSERT バッファの書きかけ内容が **初期テキスト**として $EDITOR に渡される

---

## Implementation Status / 実装状況

| 機能 | 状態 |
|------|------|
| Normal / Insert / Command モード | 実装済 |
| モード遷移 (i, :, Esc) | 実装済 |
| INSERT → Enter 送信 | 実装済 |
| INSERT マルチライン入力 (Shift+Enter / Alt+Enter) | 実装済 |
| INSERT 動的入力エリアリサイズ | 実装済 |
| COMMAND → コマンド実行 | 実装済 |
| `:discuss` | 実装済 |
| j/k スクロール | 実装済 |
| g/G 先頭/末尾 | 実装済 |
| ? ヘルプ | 実装済 |
| NORMAL キーバインド (s, e, f, d) | 実装済 |
| `I` ($EDITOR 委譲) | 実装済 |
| `[tui.input]` 設定セクション | 実装済 |
| `:ask` | 未実装 (#78) |
| NORMAL キーバインド (/, y, .) | 未実装 (#78) |
| キーバインド設定反映 (`[tui.input]` → key dispatch) | 未実装 |
| VISUAL モード | 未実装（将来） |

---

## Related / 関連

- [architecture.md - TUI Design Philosophy](../reference/architecture.md#tui-design-philosophy--tui-設計思想) — 設計思想の詳細
- [Discussion #58: Neovim-Style Extensible TUI](https://github.com/music-brain88/copilot-quorum/discussions/58) — 元の提案
- [CLI & Configuration](./cli-and-configuration.md) — 設定オプション

<!-- LLM Context: TUI は 3 つのモード (Normal, Insert, Command) を持つモーダルインターフェース。3 つの入力粒度: :ask (COMMAND, 即時質問, 未実装), i (INSERT, 対話的マルチライン), I ($EDITOR, 全画面エディタ委譲)。INSERT モードは Shift+Enter / Alt+Enter で改行挿入、Enter で送信。入力エリアは動的リサイズ（1行〜max_height行）。$EDITOR は TUI サスペンド→subprocess→レジュームで実装。INSERT→$EDITOR エスカレーション対応（書きかけ内容を初期テキストとして渡す）。NORMAL がホームポジション。NORMAL モードクイックキー: s (Solo), e (Ensemble), f (Fast トグル), d (Discuss プリフィル), I ($EDITOR)。:discuss (実装済) は COMMAND モードのファーストクラスコマンド。[tui.input] 設定セクションで max_height, context_header 等を設定可能（キーバインド設定反映は未実装）。Follow-up: #78 (:ask, /, y, .)。主要ファイルは presentation/src/tui/、editor.rs が $EDITOR 連携を担当。 -->
