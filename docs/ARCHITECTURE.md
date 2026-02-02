# Architecture / アーキテクチャ

> Technical deep-dive into copilot-quorum
>
> copilot-quorumの技術的な詳細

---

## Overview / 概要

copilot-quorum は **DDD (Domain-Driven Design) + オニオンアーキテクチャ** を採用しています。
これにより、ビジネスロジックを外部依存から分離し、高い拡張性とテスト容易性を実現しています。

---

## Design Philosophy / 設計思想

### Why DDD + Onion Architecture? / なぜDDD + オニオンアーキテクチャか

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

### Vertical Domain Slicing / 垂直ドメイン分割

copilot-quorum のドメイン層は**垂直に分割**されています。
これは「機能」ではなく「ビジネス概念」でコードを分割するアプローチです。

#### 核心: 全ての層で同じドメイン分割を繰り返す

垂直ドメイン分割の最も重要なポイントは、**ドメイン層だけでなく、全ての層で同じ分割構造を維持する**ことです：

```
copilot-quorum/
│
├── domain/                    # ドメイン層
│   ├── core/                  #   共通概念 (Model, Question, Error)
│   ├── session/               #   [セッション] エンティティ + リポジトリtrait
│   ├── orchestration/         #   [オーケストレーション] フェーズ、結果、戦略trait
│   ├── agent/                 #   [エージェント] 自律実行の状態管理
│   ├── tool/                  #   [ツール] ツール定義、呼び出し、リスクレベル
│   ├── context/               #   [コンテキスト] プロジェクト情報の読み込み
│   ├── prompt/                #   [プロンプト] テンプレート
│   └── config/                #   [設定] 出力形式など
│
├── application/               # アプリケーション層
│   ├── ports/                 #   共通ポート定義
│   └── use_cases/             #   ユースケース実装
│       ├── run_quorum.rs      #     合議実行
│       └── run_agent.rs       #     エージェント実行
│
├── infrastructure/            # インフラ層
│   ├── copilot/               #   [Copilot] LlmGateway実装
│   ├── tools/                 #   [Tools] LocalToolExecutor実装
│   └── context/               #   [Context] LocalContextLoader実装
│
└── presentation/              # プレゼンテーション層
    ├── cli/                   #   [CLI] コマンド定義
    ├── chat/                  #   [Chat] REPL実装
    ├── output/                #   [出力] フォーマッター
    └── progress/              #   [進捗] レポーター
```

#### なぜ全層で同じ分割か？

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

#### Horizontal vs Vertical / 水平分割と垂直分割の違い

```
水平分割（機能で分割）:          垂直分割（ドメインで分割）:

├── entities/                   ├── session/
│   ├── Session.rs              │   ├── entities.rs
│   ├── Message.rs              │   └── repository.rs
│   ├── QuorumRun.rs            │
│   └── ...                     ├── orchestration/
│                               │   ├── entities.rs
├── repositories/               │   ├── value_objects.rs
│   ├── SessionRepo.rs          │   └── strategy.rs
│   └── ...                     │
│                               └── prompt/
├── services/                       └── template.rs
│   ├── QuorumService.rs
│   └── ...                     (関連するものが近くにある)

(同じ概念が散らばる)
```

**垂直分割のメリット:**

1. **凝集度** - 関連するコードが同じディレクトリにまとまる
2. **プラグイン性** - 新しいドメインをディレクトリ追加で実現
3. **理解しやすさ** - 1つのドメインを理解するために見るファイルが限定される
4. **独立した進化** - 各ドメインを独立して拡張・修正可能
5. **削除容易性** - 機能を削除する際、関連ファイルが一箇所にまとまっている

### Plugin Architecture / プラグインアーキテクチャ

垂直分割とトレイトの組み合わせにより、**プラグイン的に機能を追加**できます。

#### 新機能追加の具体的フロー

例：「ディベート戦略」という新しいオーケストレーション方式を追加する場合

```
Step 1: ドメイン層に戦略を追加
domain/src/orchestration/strategies/
└── debate.rs                    # DebateStrategy 実装

Step 2: アプリケーション層にユースケースを追加（必要なら）
application/src/use_cases/
└── run_debate.rs                # RunDebateUseCase

Step 3: プレゼンテーション層にCLIオプションを追加
presentation/src/cli/commands.rs # --strategy debate オプション

Step 4: cli/main.rs でDI設定を追加
cli/src/main.rs                  # 戦略の選択ロジック

既存コードの変更: 最小限（DIの登録部分のみ）
```

#### 拡張パターン別の追加場所

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
domain/src/orchestration/
├── strategy.rs     # OrchestrationStrategy trait（既存）
└── strategies/     # 新規ディレクトリ
    ├── mod.rs
    ├── three_phase.rs  # 既存: Initial → Review → Synthesis
    ├── fast.rs         # 新規: Initial → Synthesis
    └── debate.rs       # 新規: モデル同士が議論

新しいプレゼンテーション追加（例: HTTP API）:
presentation/
├── cli/            # 既存: CLI
└── server/         # 新規追加
    ├── mod.rs
    ├── http.rs     # Actix-web ハンドラ
    ├── grpc.rs     # tonic gRPC
    └── dto.rs      # リクエスト/レスポンス型
```

#### プラグイン性を支える設計原則

| 原則 | 実装 | 効果 |
|------|------|------|
| **依存性逆転** | ドメイン層でtrait定義、インフラ層で実装 | 実装を差し替え可能 |
| **統一インターフェース** | `LlmGateway`, `OrchestrationStrategy` | 新実装が既存コードと自動統合 |
| **DIによる疎結合** | `cli/main.rs` で組み立て | 実装の選択を1箇所に集約 |
| **型によるコンパイル時検証** | ジェネリクス `RunQuorumUseCase<G>` | 不正な組み合わせをコンパイルエラーに |

### Key Design Decisions / 主要な設計判断

| 判断 | 理由 |
|------|------|
| ドメイン層に `async-trait` のみ依存 | 非同期トレイトは本質的にドメインの一部（LLM呼び出しは非同期） |
| `Model` を Value Object として定義 | 不変で、同一性ではなく値で比較される |
| `Question` にバリデーションを内包 | 不正な状態を作れないようにする（空の質問を防ぐ） |
| ユースケースにジェネリクス使用 | 実行時DI（Box<dyn>）ではなくコンパイル時DI |
| インフラ層でプロトコル詳細を隠蔽 | JSON-RPC, LSPヘッダーなどの詳細はドメインに漏れない |

---

## Layer Structure / レイヤー構成

```
copilot-quorum/
├── domain/          # ドメイン層 - ビジネスロジックの核心
│   ├── core/        # 共通ドメイン概念 (Model, Question, Error)
│   ├── session/     # LLMセッションドメイン
│   ├── orchestration/  # Quorumオーケストレーションドメイン
│   ├── agent/       # エージェント自律実行ドメイン
│   ├── tool/        # ツール定義・実行ドメイン
│   ├── context/     # プロジェクトコンテキストドメイン
│   ├── prompt/      # プロンプトドメイン
│   └── config/      # 設定ドメイン
│
├── application/     # アプリケーション層 - ユースケース
│   ├── ports/       # ポート定義 (LlmGateway, ProgressNotifier, ToolExecutorPort, ContextLoaderPort)
│   └── use_cases/   # ユースケース (RunQuorumUseCase, RunAgentUseCase)
│
├── infrastructure/  # インフラ層 - 技術的実装
│   ├── copilot/     # Copilot CLIアダプター
│   ├── tools/       # LocalToolExecutor
│   └── context/     # LocalContextLoader
│
├── presentation/    # プレゼンテーション層 - UI
│   ├── cli/         # CLIコマンド定義
│   ├── chat/        # ChatRepl
│   ├── output/      # 出力フォーマッター
│   └── progress/    # プログレス表示
│
└── cli/             # エントリポイント (DI構築)
```

### Dependency Flow (Onion Structure) / 依存の方向

```
                cli/
                  |
           presentation/
                  |
    infrastructure/ --> application/
            |                |
            +----> domain/ <-+
```

- **domain/** : 依存なし（純粋なビジネスロジック）
- **application/** : domainのみに依存
- **infrastructure/** : domain + applicationのトレイトを実装
- **presentation/** : domain + applicationに依存
- **cli/** : 全てに依存（DI構築）

---

## Domain Layer / ドメイン層

ビジネスロジックの核心。外部依存は一切なし。

### Core Module

| Type | Kind | Description |
|------|------|-------------|
| `Model` | Value Object | 利用可能なAIモデル（Claude, GPT, Gemini等） |
| `Question` | Value Object | Quorumに投げかける質問 |
| `DomainError` | Error | ドメインレベルのエラー |

### Session Module

| Type | Kind | Description |
|------|------|-------------|
| `Session` | Entity | LLMとの会話セッション |
| `Message` | Entity | 会話内のメッセージ |
| `LlmSessionRepository` | Trait | セッション管理の抽象化 |

### Orchestration Module

| Type | Kind | Description |
|------|------|-------------|
| `Phase` | Value Object | フェーズ（Initial, Review, Synthesis） |
| `QuorumConfig` | Entity | Quorum設定（モデル、モデレーター等） |
| `QuorumRun` | Entity | 実行中のQuorumセッション |
| `ModelResponse` | Value Object | モデルからの回答 |
| `PeerReview` | Value Object | ピアレビュー結果 |
| `SynthesisResult` | Value Object | 最終統合結果 |
| `QuorumResult` | Value Object | 全フェーズの結果 |
| `OrchestrationStrategy` | Trait | オーケストレーション戦略の抽象化 |

### Prompt Module

| Type | Kind | Description |
|------|------|-------------|
| `PromptTemplate` | Service | 各フェーズのプロンプトテンプレート |

### Agent Module

| Type | Kind | Description |
|------|------|-------------|
| `AgentState` | Entity | エージェント実行の現在状態 |
| `AgentConfig` | Entity | エージェント設定（プライマリモデル、合議モデル等） |
| `Plan` | Value Object | タスク計画（目的、理由付け、タスクリスト） |
| `Task` | Value Object | 単一タスク（ツール呼び出し、依存関係） |
| `AgentContext` | Value Object | 収集されたプロジェクトコンテキスト |
| `Thought` | Value Object | エージェントの思考記録 |

### Tool Module

| Type | Kind | Description |
|------|------|-------------|
| `ToolDefinition` | Entity | ツールのメタデータ（名前、パラメータ、リスクレベル） |
| `ToolCall` | Value Object | ツール呼び出し（引数付き） |
| `ToolResult` | Value Object | 実行結果（成功/失敗、出力） |
| `ToolSpec` | Entity | 利用可能なツールのレジストリ |
| `RiskLevel` | Value Object | Low（読み取り専用）または High（変更あり） |
| `ToolValidator` | Trait | ツール呼び出しのバリデーションロジック |

### Context Module

| Type | Kind | Description |
|------|------|-------------|
| `ProjectContext` | Entity | プロジェクトの統合コンテキスト |
| `KnownContextFile` | Value Object | 既知のコンテキストファイル種別（CLAUDE.md, README.md等） |
| `LoadedContextFile` | Value Object | 読み込まれたファイルの内容 |

---

## Application Layer / アプリケーション層

ユースケースとポート（外部インターフェース）を定義。

### Ports (Interfaces) / ポート

| Trait | Description |
|-------|-------------|
| `LlmGateway` | LLMプロバイダーへのゲートウェイ |
| `LlmSession` | アクティブなLLMセッション |
| `ProgressNotifier` | 進捗通知コールバック |
| `ToolExecutorPort` | ツール実行の抽象化 |
| `ContextLoaderPort` | コンテキストファイル読み込みの抽象化 |
| `AgentProgressNotifier` | エージェント進捗通知コールバック |

### Use Cases / ユースケース

| Type | Description |
|------|-------------|
| `RunQuorumUseCase` | Quorum（合議）実行のユースケース |
| `RunAgentUseCase` | エージェント自律実行のユースケース |
| `RunQuorumInput` | Quorumユースケースへの入力 |
| `RunQuorumError` | Quorumユースケースのエラー |

---

## Infrastructure Layer / インフラ層

アプリケーション層のポートを実装するアダプター。

### Copilot Adapter

| Type | Implements | Description |
|------|------------|-------------|
| `CopilotLlmGateway` | `LlmGateway` | Copilot CLI経由のLLMゲートウェイ |
| `CopilotSession` | `LlmSession` | Copilotセッション |
| `StdioTransport` | - | TCP/JSON-RPC通信層 |

### Tools Adapter

| Type | Implements | Description |
|------|------------|-------------|
| `LocalToolExecutor` | `ToolExecutorPort` | ローカルマシンでのツール実行 |

利用可能なツール:
- `read_file` - ファイル内容の読み取り（Low risk）
- `write_file` - ファイルの書き込み/作成（High risk）
- `run_command` - シェルコマンド実行（High risk）
- `glob_search` - パターンによるファイル検索（Low risk）
- `grep_search` - ファイル内容の検索（Low risk）

### Context Adapter

| Type | Implements | Description |
|------|------------|-------------|
| `LocalContextLoader` | `ContextLoaderPort` | ローカルファイルシステムからのコンテキスト読み込み |

読み込み対象ファイル（優先度順）:
1. `.quorum/context.md` - 生成されたQuorumコンテキスト
2. `CLAUDE.md` - ローカルプロジェクト指示
3. `~/.claude/CLAUDE.md` - グローバルClaude設定
4. `README.md` - プロジェクトREADME
5. `docs/**/*.md` - docsディレクトリ内の全Markdown
6. `Cargo.toml`, `package.json`, `pyproject.toml` - ビルド設定

---

## Presentation Layer / プレゼンテーション層

ユーザーインターフェースと出力フォーマット。

### CLI Module

| Type | Description |
|------|-------------|
| `Cli` | CLAPコマンド定義 |
| `OutputFormat` | 出力形式（Full, Synthesis, Json） |

### Chat Module

| Type | Description |
|------|-------------|
| `ChatRepl` | インタラクティブなREPL実装 |
| `ChatCommand` | `/init`, `/council` などのスラッシュコマンド |

### Output Module

| Type | Description |
|------|-------------|
| `OutputFormatter` | 出力フォーマッターのトレイト |
| `ConsoleFormatter` | コンソール向け色付き出力 |

### Progress Module

| Type | Description |
|------|-------------|
| `ProgressReporter` | indicatifによるプログレスバー |
| `SimpleProgress` | シンプルなテキスト進捗表示 |

---

## Data Flow / データフロー

```
+===========================================================================+
|                                  cli/                                      |
|  +-------------+                                       +----------------+  |
|  | CLI Parser  |                                       | DI Container   |  |
|  +------+------+                                       +--------+-------+  |
|         |                                                       |          |
+=========|=======================================================|==========+
          |                                                       |
          v                                                       v
+===========================================================================+
|                            application/                                    |
|                                                                            |
|  +---------------------------------------------------------------------+   |
|  |                       RunQuorumUseCase                              |   |
|  |                                                                     |   |
|  |  Phase 1: Initial Query                                            |   |
|  |           +-- Model A (parallel)  --> Response A                   |   |
|  |           +-- Model B (parallel)  --> Response B                   |   |
|  |           +-- Model C (parallel)  --> Response C                   |   |
|  |                                                                     |   |
|  |  Phase 2: Peer Review                                              |   |
|  |           +-- A reviews [B, C] (anonymized)                        |   |
|  |           +-- B reviews [A, C] (anonymized)                        |   |
|  |           +-- C reviews [A, B] (anonymized)                        |   |
|  |                                                                     |   |
|  |  Phase 3: Synthesis                                                |   |
|  |           +-- Moderator synthesizes all responses + reviews        |   |
|  |                                                                     |   |
|  +---------------------------------------------------------------------+   |
|                                                                            |
+==================================+=========================================+
                                   |
                                   v
+===========================================================================+
|                          infrastructure/                                   |
|                                                                            |
|  +------------------+    +------------------+    +---------------------+   |
|  | CopilotLlmGateway|----> StdioTransport   |----> copilot CLI (JSON) |   |
|  +------------------+    +------------------+    +---------------------+   |
|                                                                            |
+===========================================================================+
```

---

## Copilot CLI Protocol / Copilot CLIプロトコル

`infrastructure/copilot/` は GitHub Copilot CLI と JSON-RPC 経由で通信します。

```
+------------------+         JSON-RPC          +------------------+
| copilot-quorum   |<------------------------->|  copilot CLI     |
|                  |   TCP (localhost:PORT)    |                  |
+------------------+                           +------------------+
```

### Communication Flow / 通信フロー

1. `copilot --server` を起動
2. stdout から "CLI server listening on port XXXXX" を読み取り
3. TCP接続を確立
4. LSP形式のヘッダー + JSON-RPCでメッセージ交換

### Message Format / メッセージ形式

```
Content-Length: 123\r\n
\r\n
{"jsonrpc":"2.0","method":"session.create","params":{"model":"claude-sonnet-4.5"},"id":1}
```

---

## Concurrency Model / 並行処理モデル

```rust
// Phase 1: All models queried in parallel
let mut join_set = JoinSet::new();
for model in &models {
    join_set.spawn(query_model(model, question));
}
let responses = join_set.join_all().await;

// Phase 2: All reviews in parallel
let mut join_set = JoinSet::new();
for model in &models {
    join_set.spawn(do_peer_review(model, other_responses));
}
let reviews = join_set.join_all().await;

// Phase 3: Single moderator call
let synthesis = synthesize(moderator, responses, reviews).await;
```

非同期処理は `tokio` ランタイム上で実行。各フェーズ内のモデル呼び出しは `JoinSet` で並列化されており、レイテンシを最小化しています。

---

## Agent System / エージェントシステム

エージェントシステムは、Quorumの概念を自律タスク実行に拡張したものです。
重要なポイントでは合議によるレビューを維持しつつ、ルーチンタスクは単一モデルで実行します。

### Agent Flow / エージェントフロー

```
User Request
    │
    ▼
┌───────────────────┐
│ Context Gathering │  ← プロジェクト情報収集 (glob, read_file)
└───────────────────┘
    │
    ▼
┌───────────────────┐
│     Planning      │  ← 単一モデルがタスク計画を作成
└───────────────────┘
    │
    ▼
┌───────────────────┐
│ 🗳️ QUORUM #1     │  ← 全モデルが計画をレビュー（必須）
│   Plan Review     │     過半数の投票で承認/却下
└───────────────────┘
    │
    ▼
┌───────────────────┐
│  Task Execution   │
│   ├─ Low-risk  ────▶ 直接実行
│   │
│   └─ High-risk ────▶ 🗳️ QUORUM #2 (Action Review)
│                        write_file, run_command 実行前にレビュー
└───────────────────┘
    │
    ▼
┌───────────────────┐
│ 🗳️ QUORUM #3     │  ← オプションの最終レビュー
│  Final Review     │     (require_final_review: true)
└───────────────────┘
```

### Quorum Review / 合議レビュー

合議システムは複数モデルの合意によって安全性を確保します：

1. **Plan Review（必須）**: 設定された全合議モデルが提案された計画をレビュー
2. **Action Review（高リスク操作）**: `write_file` と `run_command` の実行前にレビュー
3. **Final Review（オプション）**: 実行結果全体をレビュー

承認には過半数の投票が必要。却下された計画/アクションには集約されたフィードバックが含まれます。

### Risk Levels / リスクレベル

| Risk Level | Tools | Behavior |
|------------|-------|----------|
| Low | `read_file`, `glob_search`, `grep_search` | 直接実行（レビューなし） |
| High | `write_file`, `run_command` | 合議レビュー後に実行 |

### Progress Notification Pattern / 進捗通知パターン

エージェントシステムは「アクションとUI通知の分離」パターンを採用しています。
これはVuex/Fluxのような単方向データフローに似た設計です。

#### 原則

| 層 | 責任 | やらないこと |
|---|---|---|
| **低レベル関数** (`review_plan`, `review_action`, `final_review`) | ビジネスロジック実行、結果を返す | UI通知 |
| **メインループ** (`execute_with_progress`) | 結果に基づきUI通知を発火 | - |
| **ProgressNotifier** (Presentation層) | UIの更新、フィードバック表示 | ビジネスロジック |

#### データフロー

```
UseCase (Application層)
│
├── review_plan() ──→ QuorumReviewResult
│                          │
│                          ▼
├── execute_with_progress() ─→ progress.on_quorum_complete_with_votes()
│                                   │
│                                   ▼
└── ProgressNotifier (Presentation層) ──→ UI表示
```

#### なぜこの設計か

1. **責任の分離**: ビジネスロジックがUI詳細を知らない
2. **テスト容易性**: 低レベル関数はUI依存なしでテスト可能
3. **柔軟性**: 異なるUI (CLI, TUI, Web) に同じロジックを再利用
4. **バグ防止**: UI通知の重複呼び出しを構造的に防ぐ

---

## Error Handling / エラーハンドリング

| Error Type | Location | Handling |
|------------|----------|----------|
| `DomainError` | `domain/` | ドメインルール違反 |
| `GatewayError` | `application/` | LLMゲートウェイエラー |
| `RunQuorumError` | `application/` | ユースケース実行エラー |
| `CopilotError` | `infrastructure/` | Copilot CLI通信エラー |

部分的な失敗（一部のモデルがエラーを返す）は許容され、成功したモデルの結果のみで処理を続行します。

---

## Extension Points / 拡張ポイント

### Adding New LLM Provider / 新しいLLMプロバイダーの追加

`infrastructure/` に新しいアダプターを追加：

```rust
// infrastructure/src/ollama/gateway.rs
pub struct OllamaLlmGateway { ... }

#[async_trait]
impl LlmGateway for OllamaLlmGateway {
    async fn create_session(&self, model: &Model) -> Result<Box<dyn LlmSession>, GatewayError> {
        // Ollama API implementation
    }
    // ...
}
```

### Adding New Orchestration Strategy / 新しいオーケストレーション戦略の追加

`domain/orchestration/` に新しい戦略を追加：

```rust
// domain/src/orchestration/strategies/debate.rs
pub struct DebateStrategy { ... }

#[async_trait]
impl OrchestrationStrategy for DebateStrategy {
    fn name(&self) -> &'static str { "debate" }
    fn phases(&self) -> Vec<Phase> { /* ... */ }
    async fn execute(&self, /* ... */) -> Result<QuorumResult, DomainError> {
        // Models debate with each other
    }
}
```

### Adding HTTP/gRPC API / サーバー化

`presentation/` にサーバーモジュールを追加：

```rust
// presentation/src/server/http.rs
async fn run_quorum_handler(
    use_case: web::Data<RunQuorumUseCase<CopilotLlmGateway>>,
    req: web::Json<RunQuorumRequest>,
) -> HttpResponse {
    // Same UseCase, different interface
    match use_case.execute(req.into_inner().into()).await {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => HttpResponse::InternalServerError().json(e),
    }
}
```

### Adding New Models / 新しいモデルの追加

`domain/src/core/model.rs` の `Model` enum に追加：

```rust
pub enum Model {
    // ...
    NewModel,  // Add here
}

impl Model {
    pub fn as_str(&self) -> &str {
        match self {
            // ...
            Model::NewModel => "new-model-id",
        }
    }
}
```

### Custom Output Formats / カスタム出力形式

`presentation/output/` に新しいフォーマッターを追加：

```rust
pub struct MarkdownFormatter;

impl OutputFormatter for MarkdownFormatter {
    fn format(&self, result: &QuorumResult) -> String {
        // Markdown format
    }
}
```

### Custom Progress Reporters / カスタム進捗表示

`ProgressNotifier` トレイトを実装：

```rust
pub struct WebSocketProgress { /* ... */ }

impl ProgressNotifier for WebSocketProgress {
    fn on_phase_start(&self, phase: &Phase, total_tasks: usize) {
        // Send WebSocket message
    }
    // ...
}
```

### Adding New Tools / 新しいツールの追加

`infrastructure/tools/` に新しいツールを追加し、`default_tool_spec()` に登録：

```rust
// infrastructure/src/tools/my_tool.rs
pub fn execute_my_tool(args: &ToolCall) -> ToolResult {
    // Tool implementation
}

// infrastructure/src/tools/mod.rs の default_tool_spec() に追加
ToolDefinition::new("my_tool", "Description", RiskLevel::Low, params)
```

### Adding New Context File Types / 新しいコンテキストファイル種別の追加

`domain/context/` の `KnownContextFile` enum に新しいファイル種別を追加：

```rust
pub enum KnownContextFile {
    // ...
    MyConfigFile,  // 追加
}

impl KnownContextFile {
    pub fn relative_path(&self) -> &str {
        match self {
            // ...
            Self::MyConfigFile => "my-config.yaml",
        }
    }
}
```

---

## Testing Strategy / テスト戦略

オニオンアーキテクチャにより、各層を独立してテスト可能：

| Layer | Test Type | Description |
|-------|-----------|-------------|
| domain | Unit | ドメインロジックの単体テスト |
| application | Unit + Integration | モックゲートウェイでユースケーステスト |
| infrastructure | Integration | 実際のCopilot CLIとの結合テスト |
| presentation | Unit | フォーマッターの出力テスト |

```bash
# Run all tests
cargo test --workspace

# Run domain tests only
cargo test -p quorum-domain

# Run with coverage
cargo llvm-cov --workspace
```
