# TUI Design / TUI 設計思想

> Why the TUI is a modal orchestration cockpit, not an editor with vim keybindings
>
> TUI が「Vim キーバインド付き REPL」ではなく、モーダルなオーケストレーション操作盤である理由

---

## TUI Design Philosophy / TUI 設計思想

> See also: [Discussion #58: Neovim-Style Extensible TUI](https://github.com/music-brain88/copilot-quorum/discussions/58)

copilot-quorum の TUI は「Vim キーバインド付きの REPL」ではなく、
**LLM オーケストレーションに最適化されたモーダルインターフェース**として設計されています。

### Core Principle: Orchestrator, Not Editor / 核心原則: オーケストレーター、エディタではない

copilot-quorum の本質は **LLM 群を指揮するオーケストレーター**です。
テキスト編集は本業ではありません。

この原則から導かれる設計判断:

| 判断 | 理由 |
|------|------|
| NORMAL モードがホームポジション | 「指揮者の操作盤」= オーケストレーション操作が主 |
| $EDITOR 委譲（`I`） | エディタを再実装せず、ユーザーの本物の vim/neovim を呼ぶ |
| INSERT は対話的入力に特化 | 内蔵エディタの完成度を競わない |
| NORMAL キーバインドはオーケストレーション操作 | `d` = Discuss, `s` = Solo, `e` = Ensemble（vim の delete/substitute ではない） |

### Input Granularity Model / 入力粒度モデル

LLM への入力を **3 つのニーズ粒度** に分類し、それぞれを **vim の自然な操作** にマッピングします。

```
操作コスト:  低 ──────────────────────────────── 高

             :ask            i (INSERT)       I ($EDITOR)
             ↓               ↓                ↓
ニーズ:      一言で済む       対話的            複雑なプロンプト
             "Fix the bug"   応答を見ながら    システム設計の依頼
                             追加質問          コード片を含む指示
```

| キー | モード | 用途 | vim との対応 |
|------|--------|------|-------------|
| `:ask <prompt>` | COMMAND | 最速の一言質問。入力して即実行 | `:!command` と同じ即時性 |
| `i` | INSERT | 応答パネルを見ながらの対話的入力 | `i` = INSERT モードに入る |
| `I` | $EDITOR | がっつり書く。本物の vim/neovim で編集 | `I` = "大きい" INSERT |

### Tab + Pane Architecture / タブ・ペインアーキテクチャ

TUI は Vim のバッファ/ウィンドウ/タブページモデルを踏襲しています：

| Vim | copilot-quorum | 説明 |
|-----|----------------|------|
| Buffer | `Interaction` (domain) | 対話データ（Agent/Ask/Discuss） |
| Window | `Pane` (presentation) | 表示ユニット（会話、プログレス等を保持） |
| Tab Page | `Tab` (presentation) | 1つ以上のペインを含むタブ |

`TabManager` がタブの作成・切り替え・インタラクションとのバインドを管理します。

各 Pane は独立した入力バッファを持つため、**タブ切り替え時に下書きが保持**されます。
新しい対話を「新しいバッファを開く」操作として扱うこのモデルの経緯は
[ADR 0005: Unified Interaction Architecture](./design-decisions/0005-unified-interaction-architecture.md) を参照してください。

### Modal Architecture / モーダルアーキテクチャ

```
┌───────────┐    Esc    ┌───────────┐    :     ┌──────────────┐
│  INSERT   │ ────────► │  NORMAL   │ ──────► │   COMMAND    │
│           │ ◄──────── │           │ ◄────── │              │
└───────────┘   i / a   └───────────┘   Esc   └──────────────┘
                              │
                              │ v (将来)
                              ▼
                        ┌───────────┐
                        │  VISUAL   │
                        └───────────┘
```

---

## Remote Control Design / リモート操作の設計

`--listen` で公開される Remote Control API（Neovim の `nvim --listen` に相当）の設計判断:

- **ワイヤ形式**: LSP スタイルの `Content-Length` フレーミング + JSON-RPC 2.0
  （`copilot --server` と同一形式）
- **実行モデル**: 各リクエストは `remote_rx` チャネル経由でメインの `select!`
  ループ内で処理される。`&mut TuiState` へのアクセスはキーボード入力と完全に
  同一のコードパス（`input.send` = `KeyAction::SubmitInput` 相当）
- **セキュリティ**: ソケットは `0600` パーミッション、TCP リスナーなし
- **HiL 連携**: `state.get` で保留中プランを確認 → `hil.respond` で承認/却下。
  エージェントが TUI を運転しつつ承認だけ人間（または別エージェント）が返す
  非同期 HiL（Discussion #42 の方向性）の土台

表示層を Content / Route / Surface の 3 プリミティブに分離した経緯は
[ADR 0006: TUI Content/Route/Surface](./design-decisions/0006-tui-content-route-surface.md) を参照してください。

---

## Related / 関連

- [How to Use the TUI](../how-to/use-the-tui.md) - モード・コマンド・タブ操作の使い方
- [TUI Internals](../reference/tui-internals.md) - Actor パターン・イベントルーティングの実装
- [TUI Remote Control API](../reference/tui-remote-control.md) - JSON-RPC メソッドリファレンス
- [Interaction Model](./interaction-model.md) - Interaction と Tab/Pane の対応
- [Discussion #58: Neovim-Style Extensible TUI](https://github.com/music-brain88/copilot-quorum/discussions/58) - 元の提案と概念ロードマップ

<!-- LLM Context: TUI 設計思想。核心原則「オーケストレーター、エディタではない」。入力3粒度 (:ask=即時 / i=INSERT対話 / I=$EDITOR委譲)。Vim 3層モデル: Buffer→Interaction(domain), Window→Pane(presentation), Tab Page→Tab(presentation)。NORMAL がホームポジション。Remote Control API は LSP フレーミング + JSON-RPC 2.0、キーボードと同一コードパス、socket 0600。 -->
