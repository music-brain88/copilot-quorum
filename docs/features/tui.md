# Modal TUI / モーダル TUI

> Neovim ライクなモーダルインターフェースの使い方
>
> 設計思想は [ARCHITECTURE.md](../ARCHITECTURE.md#tui-design-philosophy--tui-設計思想) を参照

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
| `I` | $EDITOR を起動（未実装） |
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

LLM の応答パネルを見ながらプロンプトを入力するモード。

| キー | アクション |
|------|-----------|
| `Enter` | 入力を送信（Agent 実行） |
| `Esc` | NORMAL モードに戻る（送信せず） |
| `Backspace` | 1文字削除 |
| `←` / `→` | カーソル移動 |
| `Home` / `End` | 行頭/行末 |

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

> **Status:** 未実装

NORMAL モードで `I` を押すと `$EDITOR`（vim/neovim 等）を全画面起動します。
`git commit` が `$EDITOR` を呼ぶのと同じパターンです。

- `:wq` でプロンプトを送信
- `:q!` でキャンセル

起動時にコンテキスト情報がコメント行で表示されます:

```
# --- Quorum Prompt ---
# Mode: Ensemble | Strategy: Quorum
# Buffers: src/auth.rs, README.md
#
# Write your prompt below. Lines starting with # are ignored.
# :wq to send, :q! to cancel
# ---------------------

```

---

## Implementation Status / 実装状況

| 機能 | 状態 |
|------|------|
| Normal / Insert / Command モード | 実装済 |
| モード遷移 (i, :, Esc) | 実装済 |
| INSERT → Enter 送信 | 実装済 |
| COMMAND → コマンド実行 | 実装済 |
| `:discuss` | 実装済 |
| j/k スクロール | 実装済 |
| g/G 先頭/末尾 | 実装済 |
| ? ヘルプ | 実装済 |
| NORMAL キーバインド (s, e, f, d) | 実装済 |
| `:ask` | 未実装 (#78) |
| `I` ($EDITOR 委譲) | 未実装 (#79) |
| NORMAL キーバインド (/, y, .) | 未実装 (#78) |
| VISUAL モード | 未実装（将来） |

---

## Related / 関連

- [ARCHITECTURE.md - TUI Design Philosophy](../ARCHITECTURE.md#tui-design-philosophy--tui-設計思想) — 設計思想の詳細
- [Discussion #58: Neovim-Style Extensible TUI](https://github.com/music-brain88/copilot-quorum/discussions/58) — 元の提案
- [CLI & Configuration](./cli-and-configuration.md) — 設定オプション

<!-- LLM Context: TUI は 3 つのモード (Normal, Insert, Command) を持つモーダルインターフェース。3 つの入力粒度: :ask (COMMAND, 即時質問, 未実装), i (INSERT, 対話的), I ($EDITOR, がっつり, 未実装)。NORMAL がホームポジション。NORMAL モードクイックキー: s (Solo), e (Ensemble), f (Fast トグル), d (Discuss プリフィル)。:discuss (実装済) は COMMAND モードのファーストクラスコマンド。Follow-up: #78 (/, y, .), #79 ($EDITOR)。主要ファイルは presentation/src/tui/。 -->
