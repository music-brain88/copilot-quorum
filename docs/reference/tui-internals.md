# TUI Internals / TUI 内部構造

> Actor pattern, event routing and widget structure of the modal TUI
>
> モーダル TUI の Actor パターン・イベントルーティング・ウィジェット構造

---

## Actor パターン

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

## RoutedTuiEvent（interaction_id ベースのルーティング）

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

## TuiCommand

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

## Tab Spawn Flow / タブ生成フロー

`:ask`, `:discuss`, `:agent` コマンドは即座にプレースホルダータブを作成し、
バックグラウンドで `SpawnInteraction` を送信します。
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

## Key Files / 主要ファイル

| File | Description |
|------|-------------|
| `presentation/src/tui/app.rs` | メインループ + Actor |
| `presentation/src/tui/event.rs` | `RoutedTuiEvent`, `TuiCommand`, `TuiEvent` |
| `presentation/src/tui/tab.rs` | `Tab`, `Pane`, `TabManager` |
| `presentation/src/tui/mode.rs` | `InputMode`, `KeyAction`, キーマップ |
| `presentation/src/tui/presenter.rs` | `UiEvent` → `TuiState` 反映 |
| `presentation/src/tui/state.rs` | `TuiState` |
| `presentation/src/tui/progress.rs` | `TuiProgressBridge` |
| `presentation/src/tui/editor.rs` | $EDITOR 連携 |
| `presentation/src/tui/human_intervention.rs` | `TuiHumanIntervention` |
| `presentation/src/tui/remote.rs` | Remote Control API サーバー |
| `presentation/src/tui/content.rs` / `route.rs` / `surface.rs` / `layout.rs` | Content / Route / Surface / レイアウト計算 |
| `presentation/src/tui/widgets/` | 各ウィジェット |

---

## Related / 関連

- [How to Use the TUI](../how-to/use-the-tui.md) - 使い方（モード・コマンド・タブ操作）
- [TUI Design](../explanation/tui-design.md) - 設計思想
- [TUI Remote Control API](./tui-remote-control.md) - 外部プロセスからの操作
- [Transport Reference](./transport.md) - 並列セッションのメッセージルーティング

<!-- LLM Context: TUI 内部構造。Actor パターン: TuiApp の select! ループが crossterm EventStream、ui_rx (UiEvent)、tui_event_rx (RoutedTuiEvent)、hil_rx (HilRequest)、tick を多重化。controller_task が cmd_rx 経由で TuiCommand を受信し AgentController を操作。RoutedTuiEvent: interaction_id ベースのイベントルーティング。select! biased で ui_rx 優先。:ask/:discuss/:agent はプレースホルダータブ即時作成→SpawnInteraction→InteractionSpawned で bind_interaction_id。主要ファイル: presentation/src/tui/ (app.rs, event.rs, tab.rs, mode.rs, presenter.rs, state.rs, progress.rs, editor.rs, human_intervention.rs, remote.rs, content.rs/route.rs/surface.rs/layout.rs, widgets/)。 -->
