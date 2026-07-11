# Design Philosophy / 設計思想

> Why copilot-quorum is built with DDD + Onion Architecture and vertical domain slicing
>
> copilot-quorum が DDD + オニオンアーキテクチャと垂直ドメイン分割を採用する理由

---

## Why DDD + Onion Architecture? / なぜDDD + オニオンアーキテクチャか

従来の層構造（Presentation → Business → Data）では、ビジネスロジックがインフラ層に依存しがちです。
オニオンアーキテクチャでは**依存の方向を逆転**させ、ドメイン層を中心に据えることで：

1. **ドメインの純粋性** - ビジネスロジックが外部技術（DB、API、フレームワーク）に汚染されない
2. **テスト容易性** - ドメイン層は依存がないため、モックなしでテスト可能
3. **技術選択の自由** - インフラ層を差し替えるだけでLLMプロバイダーを変更可能
4. **長期保守性** - 技術トレンドが変わってもドメインロジックは不変

```
従来の層構造:                    オニオンアーキテクチャ:

  Presentation                        cli/
       |                               |
       v                        presentation/
    Business  -----> DB               |
       |                     infrastructure/ --> application/
       v                              |                |
      Data                            +----> domain/ <-+

  (外側が内側に依存)              (内側は何にも依存しない)
```

## Vertical Domain Slicing / 垂直ドメイン分割

copilot-quorum のドメイン層は**垂直に分割**されています。
これは「機能」ではなく「ビジネス概念」でコードを分割するアプローチです。

### 核心: 全ての層で同じドメイン分割を繰り返す

垂直ドメイン分割の最も重要なポイントは、**ドメイン層だけでなく、全ての層で同じ分割構造を維持する**ことです：

```
copilot-quorum/
│
├── domain/                    # ドメイン層
│   ├── core/                  #   共通概念 (Model, Question, Error)
│   ├── quorum/                #   [Quorum] 合意形成 (Vote, QuorumRule, ConsensusRound)
│   ├── session/               #   [セッション] エンティティ + リポジトリtrait
│   ├── orchestration/         #   [オーケストレーション] フェーズ、結果、戦略enum（実行traitはapplication層）
│   ├── agent/                 #   [エージェント] 自律実行の状態管理
│   ├── tool/                  #   [ツール] ツール定義、呼び出し、リスクレベル
│   ├── interaction/           #   [インタラクション] 対話形式、ネスト管理
│   ├── context/               #   [コンテキスト] プロジェクト情報、リソース参照
│   ├── prompt/                #   [プロンプト] テンプレート
│   ├── scripting/             #   [スクリプティング] イベント型、値型
│   └── config/                #   [設定] 出力形式など
│
├── application/               # アプリケーション層
│   ├── ports/                 #   ポート定義（11トレイト）
│   ├── use_cases/             #   ユースケース実装
│   │   ├── run_agent/         #     エージェント実行（5ファイル分割）
│   │   ├── run_quorum/        #     合議実行（StrategyExecutor: Quorum/Debate）
│   │   ├── run_ask.rs         #     Ask インタラクション実行
│   │   ├── gather_context.rs  #     コンテキスト収集
│   │   ├── execute_task.rs    #     タスク実行
│   │   ├── agent_controller.rs #    REPL/TUI コントローラー
│   │   └── init_context.rs    #     コンテキスト初期化
│   └── config/                #   QuorumConfig, ExecutionParams
│
├── infrastructure/            # インフラ層
│   ├── copilot/               #   [Copilot] LlmGateway実装, MessageRouter
│   ├── tools/                 #   [Tools] ToolRegistry, プロバイダー群
│   ├── scripting/             #   [Scripting] LuaScriptingEngine, Lua API 群
│   ├── context/               #   [Context] LocalContextLoader
│   ├── logging/               #   [Logging] JsonlConversationLogger
│   ├── reference/             #   [Reference] GitHubReferenceResolver
│   └── config/                #   [Config] （設定は Lua スクリプティングへ移行済み）
│
├── presentation/              # プレゼンテーション層
│   ├── cli/                   #   [CLI] コマンド定義
│   ├── tui/                   #   [TUI] モーダルインターフェース（tab, widgets, event）
│   ├── agent/                 #   [Agent UI] プログレス、思考表示、HiL UI
│   ├── output/                #   [出力] フォーマッター
│   ├── progress/              #   [進捗] レポーター
│   └── config/                #   [設定] OutputConfig, ReplConfig
│
└── cli/                       # エントリポイント (DI構築)
```

### なぜ全層で同じ分割か？

```
機能「テンプレート管理」を追加する例（他プロジェクトの場合）:

domain/template/           → エンティティ、リポジトリtrait定義
application/template/      → ユースケース実装
infrastructure/template/   → DB実装
presentation/template/     → ハンドラ、DTO

全ての層に「template」が現れる = 縦に一貫性がある
```

この構造により：
- **新機能追加時**: 4つの層に同名ディレクトリを追加するだけ
- **機能削除時**: 4つのディレクトリを削除するだけ
- **機能理解時**: 1つのドメイン名で全層を追跡可能

## Plugin Architecture / プラグインアーキテクチャ

垂直分割とトレイトの組み合わせにより、**プラグイン的に機能を追加**できます。

### 拡張パターン別の追加場所

```
新しいLLMプロバイダー追加（例: Ollama）:
infrastructure/
├── copilot/        # 既存: Copilot CLI
└── ollama/         # 新規追加
    ├── mod.rs
    ├── gateway.rs  # impl LlmGateway for OllamaGateway
    ├── session.rs  # impl LlmSession for OllamaSession
    └── client.rs   # Ollama API クライアント

新しいオーケストレーション戦略追加:
domain/src/orchestration/          # データモデル（enum に新バリアント追加）
├── strategy.rs     # OrchestrationStrategy enum（既存）
├── mode.rs         # ConsensusLevel enum（既存）
├── scope.rs        # PhaseScope enum（既存）
└── session_mode.rs # SessionMode（既存）
application/src/use_cases/run_quorum/  # 実行ロジック（LlmGateway/ProgressNotifier に依存するため application 層）
├── strategy_executor.rs  # StrategyExecutor trait（既存）
├── quorum_strategy.rs    # QuorumStrategyExecutor（既存）
└── debate_strategy.rs    # DebateStrategyExecutor（既存。新戦略はここに実装を追加）

新しいプレゼンテーション追加（例: HTTP API）:
presentation/
├── cli/            # 既存: CLI
├── tui/            # 既存: TUI
└── server/         # 新規追加
    ├── mod.rs
    ├── http.rs     # Actix-web ハンドラ
    └── dto.rs      # リクエスト/レスポンス型
```

具体的な拡張手順は [How to Extend the Codebase](../how-to/extend-the-codebase.md) を参照してください。

### プラグイン性を支える設計原則

| 原則 | 実装 | 効果 |
|------|------|------|
| **依存性逆転** | ドメイン層でtrait定義、インフラ層で実装 | 実装を差し替え可能 |
| **統一インターフェース** | `LlmGateway`, `StrategyExecutor` | 新実装が既存コードと自動統合 |
| **DIによる疎結合** | `cli/main.rs` で組み立て | 実装の選択を1箇所に集約 |
| **型によるコンパイル時検証** | ジェネリクス `RunQuorumUseCase<G>` | 不正な組み合わせをコンパイルエラーに |

## Key Design Decisions / 主要な設計判断

| 判断 | 理由 |
|------|------|
| ドメイン層に `async-trait` のみ依存 | 非同期トレイトは本質的にドメインの一部（LLM呼び出しは非同期） |
| `Model` を Value Object として定義 | 不変で、同一性ではなく値で比較される |
| `Question` にバリデーションを内包 | 不正な状態を作れないようにする（空の質問を防ぐ） |
| ユースケースにジェネリクス使用 | 実行時DI（Box<dyn>）ではなくコンパイル時DI |
| インフラ層でプロトコル詳細を隠蔽 | JSON-RPC, LSPヘッダーなどの詳細はドメインに漏れない |
| JSON Schema 変換を Port パターンで分離 | domain 層はツールの定義・フィルタリングのみ担当し、LLM API フォーマット（JSON Schema）への変換は `ToolSchemaPort` 経由で infrastructure 層が実装 |
| インタラクション形式を対等な peer に | Agent / Ask / Discuss を階層化せず、全て `InteractionForm` enum の対等なバリアント |

個々の決定の経緯は [Design Decisions](./design-decisions/README.md)（ADR）に記録されています。

---

## Related / 関連

- [Architecture Reference](../reference/architecture.md) - レイヤー構造・データフローの詳細
- [Design Decisions](./design-decisions/README.md) - GitHub Discussions で決着した設計判断の記録
- [TUI Design](./tui-design.md) - TUI の設計思想
- [How to Extend the Codebase](../how-to/extend-the-codebase.md) - 拡張ポイントの実践ガイド

<!-- LLM Context: DDD + オニオンアーキテクチャ + 垂直ドメイン分割。依存方向: cli → presentation → infrastructure → application → domain（内側は依存なし）。全層で同じドメイン分割（quorum, orchestration, agent, tool, interaction, context 等）を繰り返す。プラグイン原則: trait 定義は domain/application、実装は infrastructure、DI は cli/main.rs。 -->
