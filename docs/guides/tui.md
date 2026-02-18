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

### Tab/Pane アーキテクチャ

TUI は Vim の 3 層モデル（Buffer / Window / Tab Page）にインスパイアされたタブ構造を持ちます。

```
Vim の概念           copilot-quorum の対応
───────────         ──────────────────────
Buffer     →        Interaction (domain)
Window     →        Pane (presentation)
Tab Page   →        Tab (presentation)
```

| 概念 | 型 | 説明 |
|------|------|------|
| `Tab` | `Tab` | タブページ — Phase 1 では 1 Tab = 1 Pane |
| `Pane` | `Pane` | 最小描画単位。会話バッファ、入力バッファ、進捗状態を保持 |
| `PaneKind` | `PaneKind::Interaction(form, id)` | Agent / Ask / Discuss の 3 種類 |
| `TabManager` | `TabManager` | 全タブの管理、アクティブタブの追跡 |

各 Pane は独立した入力バッファを持つため、**タブ切り替え時に下書きが保持**されます。
タブタイトルは最初のユーザーメッセージから自動生成されます（30 文字で切り詰め）。

---

## Architecture / アーキテクチャ

### Actor パターン

TUI は Actor パターンで設計されています。メインの select! ループと、バックグラウンドの controller タスクが非同期チャネルで通信します。

```
TuiApp (select! loop)                 controller_task (tokio::spawn)
  ├─ crossterm EventStream              ├─ cmd_rx.recv()
  ├─ ui_rx (UiEvent from controller)    ├─ controller.handle_command()
  ├─ tui_rx (TuiEvent from progress)    └─ controller.process_request()
  ├─ hil_rx (HilRequest)
  └─ tick_interval
       └── cmd_tx ──────────────────>──┘
```

| チャネル | 方向 | 型 | 用途 |
|---------|------|------|------|
| `cmd_tx` → `cmd_rx` | TUI → Controller | `TuiCommand` | ユーザー入力（テキスト送信、コマンド実行） |
| `ui_tx` → `ui_rx` | Controller → TUI | `UiEvent` | アプリケーション層からの構造化イベント |
| `tui_event_tx` → `tui_event_rx` | Progress → TUI | `RoutedTuiEvent` | 進捗通知（ルーティング付き） |
| `hil_tx` → `hil_rx` | HiL Port → TUI | `HilRequest` | 人間介入リクエスト |

### RoutedTuiEvent（interaction_id ベースのルーティング）

複数タブが並行して動作する場合、進捗イベントを正しいタブに配送する必要があります。
`RoutedTuiEvent` はイベントに `interaction_id` を付与し、対象の Pane にルーティングします。

```rust
pub struct RoutedTuiEvent {
    pub interaction_id: Option<InteractionId>,  // None = グローバル（アクティブ Pane へ）
    pub event: TuiEvent,
}
```

- `for_interaction(id, event)` — 特定のインタラクションに紐づくイベント
- `global(event)` — グローバルイベント（アクティブ Pane にフォールバック）

select! ループは `biased` で `ui_rx` を優先します。これにより `InteractionSpawned` が
進捗イベントより先に処理され、タブが存在する状態で進捗がルーティングされます。

### TuiCommand

Controller タスクへ送信されるコマンド:

| コマンド | 説明 |
|---------|------|
| `ProcessRequest { interaction_id, request }` | INSERT モードからのテキスト送信 |
| `HandleCommand { interaction_id, command }` | COMMAND モードからのコマンド実行 |
| `SpawnInteraction { form, query, context_mode_override }` | 新しいインタラクションを生成 |
| `ActivateInteraction(id)` | タブ切り替え時にアクティブインタラクションを同期 |
| `SetVerbose(bool)` | Verbose モード設定 |
| `SetCancellation(token)` | キャンセルトークン設定 |
| `SetReferenceResolver(resolver)` | リファレンスリゾルバ設定 |
| `Quit` | 終了 |

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
| `:config` | 現在の設定を表示 |
| `:clear` | 会話履歴をクリア |
| `:init [--force]` | プロジェクトコンテキストを初期化 |
| `:help` | ヘルプを表示 |
| `:q` / `:quit` / `:exit` | 終了 |

---

## Tab System / タブシステム

### タブの作成と切り替え

`:ask`, `:discuss`, `:agent` コマンドでクエリ付きのタブを作成できます。
各コマンドは即座にプレースホルダータブを作成し、バックグラウンドで `SpawnInteraction` を送信します。
`InteractionSpawned` イベント到着時にプレースホルダーと実際の `InteractionId` がバインドされます。

```
ユーザー: :ask What is DDD?
    ↓
1. handle_tab_command() → プレースホルダータブ作成 (Ask, None)
2. cmd_tx.send(SpawnInteraction { form: Ask, query: "What is DDD?" })
    ↓
3. controller_task → controller.prepare_spawn() → InteractionId(42) 割当
4. UiEvent::InteractionSpawned → presenter.apply() → bind_interaction_id(Ask, 42)
    ↓
5. 以降の RoutedTuiEvent(interaction_id=42) は正しいタブに配送
```

### タブ切り替え

| 操作 | 説明 |
|------|------|
| `gt` (Normal モード) | 次のタブ（循環） |
| `gT` (Normal モード) | 前のタブ（循環） |

タブ切り替え時に `ActivateInteraction` コマンドが送信され、controller のアクティブインタラクションが同期されます。

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

コンテキストヘッダーは `[tui.input]` の `context_header = false` で非表示にできます。

### TUI サスペンド/レジューム

$EDITOR 起動中は TUI が一時停止します（raw mode 解除、alternate screen 退出）。
エディタ終了後に TUI が自動復帰し、バックグラウンドで受信した LLM 応答が反映されます。

---

## Input Configuration / 入力設定

`[tui.input]` セクションで入力動作をカスタマイズできます:

```toml
[tui.input]
submit_key = "enter"           # メッセージ送信キー
newline_key = "shift+enter"    # 改行挿入キー（マルチライン）
editor_key = "I"               # $EDITOR 起動キー（Normal モード）
editor_action = "return_to_insert"  # エディタ後の動作: "return_to_insert" or "submit"
max_height = 10                # 入力エリアの最大行数
dynamic_height = true          # 内容に応じた動的リサイズ
context_header = true          # $EDITOR でコンテキストヘッダーを表示
```

---

## Widget Structure / ウィジェット構造

TUI は以下のウィジェットで構成されます:

```
┌─────────────────── Header ──────────────────┐
│ Model: claude-sonnet-4.5 | Solo | Full      │
├──── Tab Bar (2+ タブ時のみ表示) ────────────┤
│ [Fix the auth bug] [What is DDD?]           │
├─────────────── Conversation ────────────────┤
│ > Fix the auth bug                          │
│ Agent completed                             │
│ ...                                         │
├──────────── Progress Panel ─────────────────┤
│ Phase: Executing | Task 2/3                 │
│ ✓ read_file (120ms)                         │
│ … write_file (running)                      │
├─────────────── Input ───────────────────────┤
│ solo>                                       │
├────────────── Status Bar ───────────────────┤
│ -- INSERT -- | Flash: Agent completed       │
└─────────────────────────────────────────────┘
```

---

## Related / 関連

- [architecture.md - TUI Design Philosophy](../reference/architecture.md#tui-design-philosophy--tui-設計思想) — 設計思想の詳細
- [Discussion #58: Neovim-Style Extensible TUI](https://github.com/music-brain88/copilot-quorum/discussions/58) — 元の提案
- [CLI & Configuration](./cli-and-configuration.md) — 設定オプション

<!-- LLM Context: TUI は 3 つのモード (Normal, Insert, Command) を持つモーダルインターフェース。Actor パターン: TuiApp の select! ループが crossterm EventStream、ui_rx (UiEvent)、tui_event_rx (RoutedTuiEvent)、hil_rx (HilRequest)、tick を多重化。controller_task が cmd_rx 経由で TuiCommand を受信し AgentController を操作。Tab/Pane アーキテクチャ: Vim の Buffer→Interaction、Window→Pane、Tab Page→Tab マッピング。PaneKind::Interaction(form, Option<InteractionId>) で Agent/Ask/Discuss を区別。TabManager が全タブ管理。各 Pane が独立した input バッファ・messages・progress を保持、タブ切り替え時に下書き保持。RoutedTuiEvent: interaction_id ベースのイベントルーティング。for_interaction(id, event) で特定 Pane に配送、global(event) でアクティブ Pane にフォールバック。select! biased で ui_rx を優先（InteractionSpawned がタブ作成より先に来るのを防ぐ）。TuiCommand: ProcessRequest, HandleCommand, SpawnInteraction, ActivateInteraction, SetVerbose, SetCancellation, SetReferenceResolver, Quit。:ask/:discuss/:agent コマンドはプレースホルダータブを即時作成→SpawnInteraction→InteractionSpawned で bind_interaction_id。タブ操作: gt/gT (Normal)、:tabs, :tabnew, :tabclose (Command)。入力: 3 粒度 (:ask=COMMAND即時, i=INSERT対話的マルチライン, I=$EDITOR全画面)。INSERT は Shift+Enter/Alt+Enter で改行、Enter で送信。動的リサイズ（1行〜max_height行）。$EDITOR は TUI サスペンド→subprocess→レジューム。[tui.input] 設定: submit_key, newline_key, editor_key, editor_action, max_height, dynamic_height, context_header。主要ファイル: presentation/src/tui/ (app.rs=メインループ+Actor, event.rs=RoutedTuiEvent+TuiCommand+TuiEvent, tab.rs=Tab/Pane/TabManager, mode.rs=InputMode+KeyAction, presenter.rs=UiEvent→TuiState, state.rs=TuiState, progress.rs=TuiProgressBridge, editor.rs=$EDITOR連携, human_intervention.rs=TuiHumanIntervention, widgets/=各ウィジェット)。 -->
