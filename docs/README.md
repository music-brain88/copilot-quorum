# Documentation Hub / ドキュメントハブ

> Documentation for copilot-quorum
>
> copilot-quorumのドキュメント

---

## Documents / ドキュメント一覧

| Document | Description |
|----------|-------------|
| [README](../README.md) | プロジェクト概要・クイックスタート |
| [ARCHITECTURE](./ARCHITECTURE.md) | DDD + オニオンアーキテクチャの詳細 |
| [Features](./features/README.md) | 機能別ドキュメント |

---

## Features / 機能ドキュメント

機能ごとの詳細ドキュメントは [features/](./features/README.md) にまとまっています：

| Document | Description |
|----------|-------------|
| [Quorum Discussion & Consensus](./features/quorum.md) | 複数モデルによる議論と合意形成 |
| [Agent System](./features/agent-system.md) | 自律タスク実行と Human-in-the-Loop |
| [Ensemble Mode](./features/ensemble-mode.md) | 研究に基づいたマルチモデル計画生成 |
| [Tool System](./features/tool-system.md) | プラグインベースのツールアーキテクチャ |
| [CLI & Configuration](./features/cli-and-configuration.md) | REPL コマンド、設定、コンテキスト管理 |

---

## Reading Guide / 読み順ガイド

### For Users / ユーザー向け

1. **[README](../README.md)** - インストールと基本的な使い方
2. **[CLI & Configuration](./features/cli-and-configuration.md)** - REPL コマンドと設定
3. **[Features](./features/README.md)** - 各機能の詳細

### For Contributors / コントリビューター向け

1. **[README](../README.md)** - プロジェクト概要
2. **[ARCHITECTURE](./ARCHITECTURE.md)** - レイヤー構造とデータフロー
3. **[Features](./features/README.md)** - 機能別の設計と実装ガイド
4. **crate-level docs** - `cargo doc --open` で各crateのAPIドキュメント

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
