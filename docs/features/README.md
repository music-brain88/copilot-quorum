# Feature Documentation / 機能ドキュメント

> Per-feature documentation for copilot-quorum
>
> copilot-quorum の機能別ドキュメント

---

## Features / 機能一覧

| Document | Description |
|----------|-------------|
| [Quorum Discussion & Consensus](./quorum.md) | 複数モデルによる議論と合意形成 |
| [Agent System](./agent-system.md) | 自律タスク実行と Human-in-the-Loop |
| [Ensemble Mode](./ensemble-mode.md) | 研究に基づいたマルチモデル計画生成 |
| [Tool System](./tool-system.md) | プラグインベースのツールアーキテクチャ |
| [Native Tool Use](./native-tool-use.md) | Native Tool Use API による構造化ツール呼び出し |
| [Modal TUI](./tui.md) | Neovim ライクなモーダルインターフェース |
| [Transport Demultiplexer](./transport.md) | 並列セッションのメッセージルーティング |
| [CLI & Configuration](./cli-and-configuration.md) | REPL コマンド、設定、コンテキスト管理 |

---

## Reading Guide / 読み順ガイド

### For Users / ユーザー向け

1. **[Modal TUI](./tui.md)** - モーダル TUI の使い方
2. **[CLI & Configuration](./cli-and-configuration.md)** - 設定とコマンド
3. **[Quorum](./quorum.md)** - 合議の仕組み
4. **[Agent System](./agent-system.md)** - エージェントの動作

### For Contributors / コントリビューター向け

1. **[Quorum](./quorum.md)** - コアコンセプトの理解
2. **[Tool System](./tool-system.md)** - ツール追加方法
3. **[Native Tool Use](./native-tool-use.md)** - Native API によるツール呼び出し
4. **[Ensemble Mode](./ensemble-mode.md)** - 設計判断の背景
5. **[Transport Demultiplexer](./transport.md)** - 並列セッションの仕組み
6. **[Agent System](./agent-system.md)** - エージェントアーキテクチャ
6. **[ARCHITECTURE.md](../ARCHITECTURE.md#tui-design-philosophy--tui-設計思想)** - TUI 設計思想
