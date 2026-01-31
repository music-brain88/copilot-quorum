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

---

## Reading Guide / 読み順ガイド

### For Users / ユーザー向け

1. **[README](../README.md)** - インストールと基本的な使い方
2. **[ARCHITECTURE](./ARCHITECTURE.md)** - 仕組みを理解したい場合

### For Contributors / コントリビューター向け

1. **[README](../README.md)** - プロジェクト概要
2. **[ARCHITECTURE](./ARCHITECTURE.md)** - レイヤー構造とデータフロー
3. **crate-level docs** - `cargo doc --open` で各crateのAPIドキュメント

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
