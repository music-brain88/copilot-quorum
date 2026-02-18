# Documentation Hub / ドキュメントハブ

> Documentation for copilot-quorum
>
> copilot-quorumのドキュメント

---

## Documents / ドキュメント一覧

| Document | Description |
|----------|-------------|
| [README](../README.md) | プロジェクト概要・クイックスタート |
| [Architecture](./reference/architecture.md) | DDD + オニオンアーキテクチャの詳細 |

---

## Concepts / 概念

| Document | Description |
|----------|-------------|
| [Quorum Discussion & Consensus](./features/quorum.md) | 複数モデルによる議論と合意形成 |
| [Ensemble Mode](./features/ensemble-mode.md) | 研究に基づいたマルチモデル計画生成 |

---

## Guides / ガイド

| Document | Description |
|----------|-------------|
| [CLI & Configuration](./features/cli-and-configuration.md) | REPL コマンド、設定、コンテキスト管理 |
| [Modal TUI](./features/tui.md) | Neovim ライクなモーダルインターフェース |

---

## Systems / システム

| Document | Description |
|----------|-------------|
| [Agent System](./features/agent-system.md) | 自律タスク実行と Human-in-the-Loop |
| [Tool System](./features/tool-system.md) | プラグインベースのツールアーキテクチャ |
| [Native Tool Use](./features/native-tool-use.md) | Native Tool Use API による構造化ツール呼び出し |
| [Transport Demultiplexer](./features/transport.md) | 並列セッションのメッセージルーティング |

---

## Reading Guide / 読み順ガイド

### For Users / ユーザー向け

1. **[README](../README.md)** - インストールと基本的な使い方
2. **[CLI & Configuration](./features/cli-and-configuration.md)** - 設定とコマンド
3. **[Modal TUI](./features/tui.md)** - モーダル TUI の使い方
4. **[Quorum](./features/quorum.md)** - 合議の仕組み
5. **[Agent System](./features/agent-system.md)** - エージェントの動作

### For Contributors / コントリビューター向け

1. **[README](../README.md)** - プロジェクト概要
2. **[Architecture](./reference/architecture.md)** - レイヤー構造とデータフロー
3. **[Quorum](./features/quorum.md)** - コアコンセプトの理解
4. **[Tool System](./features/tool-system.md)** - ツール追加方法
5. **[Native Tool Use](./features/native-tool-use.md)** - Native API によるツール呼び出し
6. **[Ensemble Mode](./features/ensemble-mode.md)** - 設計判断の背景
7. **[Transport Demultiplexer](./features/transport.md)** - 並列セッションの仕組み
8. **[Agent System](./features/agent-system.md)** - エージェントアーキテクチャ
9. **[TUI Design Philosophy](./reference/architecture.md#tui-design-philosophy--tui-設計思想)** - TUI 設計思想

---

## Project Structure / プロジェクト構造

```
copilot-quorum/
├── domain/          # ドメイン層 - ビジネスロジックの核心
│   ├── core/        # Model, Question, Error
│   ├── session/     # Session, Message, Repository trait
│   ├── orchestration/  # Phase, Config, Result, Strategy trait
│   └── prompt/      # PromptTemplate
│
├── application/     # アプリケーション層 - ユースケース
│   ├── ports/       # LlmGateway, ProgressNotifier traits
│   └── use_cases/   # RunQuorumUseCase
│
├── infrastructure/  # インフラ層 - 外部システム連携
│   └── copilot/     # Copilot CLI adapter
│
├── presentation/    # プレゼンテーション層 - UI
│   ├── cli/         # CLI commands (CLAP)
│   ├── output/      # Output formatters
│   └── progress/    # Progress reporters
│
├── cli/             # エントリポイント (DI構築)
├── docs/            # ドキュメント
├── examples/        # 使用例
└── tests/           # 統合テスト
```

---

## External Links / 外部リンク

- [Original copilot-council (Go)](https://github.com/openjny/copilot-council)
- [GitHub Copilot CLI](https://docs.github.com/en/copilot/github-copilot-in-the-cli)
- [Domain-Driven Design Reference](https://www.domainlanguage.com/ddd/reference/)
- [The Onion Architecture](https://jeffreypalermo.com/2008/07/the-onion-architecture-part-1/)
