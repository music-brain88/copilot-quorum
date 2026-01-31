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
│   ├── core/                  #   共通概念
│   ├── session/               #   [セッション] エンティティ + リポジトリtrait
│   ├── orchestration/         #   [オーケストレーション] エンティティ + 戦略trait
│   └── prompt/                #   [プロンプト] テンプレート
│
├── application/               # アプリケーション層
│   ├── ports/                 #   共通ポート定義
│   └── use_cases/             #   [全ドメイン共通] ユースケース実装
│       └── run_quorum.rs      #     オーケストレーションのユースケース
│
├── infrastructure/            # インフラ層
│   └── copilot/               #   [Copilot] LlmGateway実装
│       ├── gateway.rs         #     ゲートウェイ
│       ├── session.rs         #     セッション実装
│       └── transport.rs       #     通信層
│
└── presentation/              # プレゼンテーション層
    ├── cli/                   #   [CLI] コマンド定義
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
│   └── prompt/      # プロンプトドメイン
│
├── application/     # アプリケーション層 - ユースケース
│   ├── ports/       # ポート定義 (LlmGateway, ProgressNotifier)
│   └── use_cases/   # ユースケース (RunQuorumUseCase)
│
├── infrastructure/  # インフラ層 - 技術的実装
│   └── copilot/     # Copilot CLIアダプター
│
├── presentation/    # プレゼンテーション層 - UI
│   ├── cli/         # CLIコマンド定義
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

---

## Application Layer / アプリケーション層

ユースケースとポート（外部インターフェース）を定義。

### Ports (Interfaces) / ポート

| Trait | Description |
|-------|-------------|
| `LlmGateway` | LLMプロバイダーへのゲートウェイ |
| `LlmSession` | アクティブなLLMセッション |
| `ProgressNotifier` | 進捗通知コールバック |

### Use Cases / ユースケース

| Type | Description |
|------|-------------|
| `RunQuorumUseCase` | Quorum実行のメインユースケース |
| `RunQuorumInput` | ユースケースへの入力 |
| `RunQuorumError` | ユースケースのエラー |

---

## Infrastructure Layer / インフラ層

アプリケーション層のポートを実装するアダプター。

### Copilot Adapter

| Type | Implements | Description |
|------|------------|-------------|
| `CopilotLlmGateway` | `LlmGateway` | Copilot CLI経由のLLMゲートウェイ |
| `CopilotSession` | `LlmSession` | Copilotセッション |
| `StdioTransport` | - | TCP/JSON-RPC通信層 |

---

## Presentation Layer / プレゼンテーション層

ユーザーインターフェースと出力フォーマット。

### CLI Module

| Type | Description |
|------|-------------|
| `Cli` | CLAPコマンド定義 |
| `OutputFormat` | 出力形式（Full, Synthesis, Json） |

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
