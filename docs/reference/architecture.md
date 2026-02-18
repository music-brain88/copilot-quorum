# Architecture / アーキテクチャ

> Technical deep-dive into copilot-quorum
>
> copilot-quorumの技術的な詳細

---

## Overview / 概要

copilot-quorum は **DDD (Domain-Driven Design) + オニオンアーキテクチャ** を採用しています。
これにより、ビジネスロジックを外部依存から分離し、高い拡張性とテスト容易性を実現しています。

---

## Core Concepts / コア概念

### Quorum とは

copilot-quorum は **分散システムの Quorum System** に着想を得たマルチ LLM オーケストレーションツールです。

分散データベースでは、複数ノードの過半数（quorum）が合意して初めて操作が確定します。
copilot-quorum はこの仕組みを LLM に適用し、**複数モデルの合意によって判断の信頼性を高める**
というアプローチを取っています。

#### 分散システムとの概念マッピング

| 分散システム | copilot-quorum | 対応関係 |
|------------|----------------|----------|
| Node (ノード) | Model (LLM) | 処理を担う個々のエンティティ |
| Replication Factor | `QuorumConfig.models` | 参加するノード（モデル）の数 |
| Quorum (定足数) | Quorum Consensus | 過半数の合意で操作を確定 |
| Read/Write 操作 | Plan/Action Review | データ操作 → タスク操作 |
| Consistency Level | `ConsensusLevel` | 何ノード（モデル）の応答を要求するか |

#### Cassandra アナロジー

分散データベース Cassandra の `ConsistencyLevel` との具体的な対応：

| Cassandra | copilot-quorum | 意味 |
|-----------|----------------|------|
| `ConsistencyLevel.ONE` | `ConsensusLevel::Solo` | 1 ノード（モデル）の応答で十分 |
| `ConsistencyLevel.QUORUM` | `ConsensusLevel::Ensemble` | 過半数のノード（モデル）が合意必要 |
| Replication Factor | `QuorumConfig.models` | 参加するノード（モデル）の数 |
| Read/Write | Plan/Action Review | データ操作 → タスク操作 |

Cassandra が `ConsistencyLevel` を変えるだけで一貫性と可用性のトレードオフを制御できるように、
copilot-quorum も `ConsensusLevel` を `Solo` ↔ `Ensemble` に切り替えるだけで
速度と信頼性のトレードオフを制御できます。

#### Quorum の 3 つの側面

この概念を LLM の文脈で具体化すると、3 つの機能になります：

- **Quorum Discussion**: 複数モデルによる対等な議論（意見収集）— Read Quorum に相当
- **Quorum Consensus**: 投票による合意形成（承認/却下の判定）— Write Quorum に相当
- **Quorum Synthesis**: 複数意見の統合・矛盾解決 — Conflict Resolution に相当

### Solo / Ensemble モード

```
┌─────────────────────────────────────────────────────────────────┐
│  モード切り替え                                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  【Solo モード】(default)          【Ensemble モード】          │
│  - 単一モデル（Agent）主導         - 複数モデル（Quorum）主導   │
│  - 素早く実行                      - 多角的な視点で議論         │
│  - 必要時のみ /discuss             - 常に複数モデルで合議       │
│  - シンプルなタスク向け            - 複雑な設計・判断向け       │
│                                                                 │
│  CLI: --solo (default)             CLI: --ensemble              │
│  REPL: /solo                       REPL: /ens or /ensemble      │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

**ML 的アナロジー**:
- Solo = 単一モデルの予測
- Ensemble = 複数モデルを組み合わせて精度・信頼性を向上（アンサンブル学習）

### 3 つの直交する設定軸

Solo / Ensemble（`ConsensusLevel`）は、実行を制御する **3 つの独立した軸** のうちの 1 つです。
これらは直交しており、任意の組み合わせが可能です。

```
┌──────────────────────────────────────────────────────────────────┐
│  3 Orthogonal Axes / 3 つの直交する設定軸                        │
├──────────────────────────────────────────────────────────────────┤
│                                                                  │
│  軸 1: ConsensusLevel ─ 「誰が参加するか」                      │
│  ┌────────────┐   ┌──────────────┐                              │
│  │    Solo     │   │   Ensemble   │                              │
│  │  単一モデル │   │  複数モデル  │                              │
│  └────────────┘   └──────────────┘                              │
│                                                                  │
│  軸 2: PhaseScope ─ 「どこまで実行するか」                      │
│  ┌────────┐   ┌────────┐   ┌──────────┐                        │
│  │  Full  │   │  Fast  │   │ PlanOnly │                        │
│  │ 全工程 │   │ 高速   │   │ 計画のみ │                        │
│  └────────┘   └────────┘   └──────────┘                        │
│                                                                  │
│  軸 3: OrchestrationStrategy ─ 「どう議論するか」               │
│  ┌─────────────────────┐   ┌─────────────────────┐             │
│  │  Quorum(QuorumConfig)│   │  Debate(DebateConfig)│             │
│  │  対等な議論→統合     │   │  対立的議論→合意    │             │
│  └─────────────────────┘   └─────────────────────┘             │
│                                                                  │
│  組み合わせ例:                                                   │
│   Solo + Full + Quorum  = デフォルト（単一モデル、全工程）       │
│   Ensemble + Fast + Debate = 複数モデルで高速ディベート          │
│   Solo + PlanOnly + Quorum = 計画だけ生成して確認                │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

**なぜ直交軸か？**

当初は `Standard`, `Fast`, `PlanOnly`, `Ensemble`, `EnsembleFast` の 5 モード（`OrchestrationMode` enum）でしたが、
モードの組み合わせが増えるたびにバリアントが爆発する問題がありました（例: `EnsemblePlanOnly` を追加するとさらに増える）。

3 つの独立した軸に分解することで：
- **組み合わせの自由度** — N × M × K 通りの構成を少ないバリアントで表現
- **拡張容易性** — 新しい PhaseScope や Strategy を追加しても他の軸に影響しない
- **設定の明確性** — 各軸が「何を制御するか」が一目瞭然

### Interaction Model / インタラクションモデル

copilot-quorum のユーザー対話は **3 つの対等なインタラクション形式** で構成されています。
どれが「メイン」で他が「サブ」ということはなく、全てが第一級市民です。

| Form | Description | Context Default | 使う設定 |
|------|-------------|-----------------|----------|
| `Agent` | 自律タスク実行（計画→レビュー→実行） | `Full` | SessionMode, AgentPolicy, ExecutionParams |
| `Ask` | 単一 Q&A（読み取り専用ツール） | `Projected` | SessionMode, ExecutionParams |
| `Discuss` | 複数モデル議論 / Quorum Council | `Full` | SessionMode |

インタラクションは **ネスト可能** で、最大深度 `DEFAULT_MAX_NESTING_DEPTH`（= 3）まで子インタラクションを生成できます。
例：Agent が設計判断のために Discuss を子として生成し、その結果を親の実行に反映する。

`InteractionTree` が ID 自動採番とネスト管理を担当し、`InteractionResult` が完了時の結果を型安全に運搬します。

### Quorum Layers（将来ビジョン）

```
┌─────────────────────────────────────────────────────────────────┐
│  Quorum Layers                                                   │
├─────────────────────────────────────────────────────────────────┤
│  ┌───────────────────────────────────────────────────────────┐  │
│  │  Knowledge Quorum (知識層) - Phase 3                       │  │
│  │  - 永続化された知識からの合意形成                         │  │
│  └───────────────────────────────────────────────────────────┘  │
│                          ↓                                       │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │  Context Quorum (コンテキスト層) - Phase 2                 │  │
│  │  - セッション間でコンテキスト共有（常駐）                 │  │
│  │  - 議論履歴、決定事項、パターンを保持                     │  │
│  └───────────────────────────────────────────────────────────┘  │
│                          ↓                                       │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │  Decision Quorum (決定層) - Phase 1 (Current)              │  │
│  │  - 複数モデルによる意思決定の合意                         │  │
│  │  - Quorum Discussion, Quorum Consensus                     │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

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
│   ├── quorum/                #   [Quorum] 合意形成 (Vote, QuorumRule, ConsensusRound)
│   ├── session/               #   [セッション] エンティティ + リポジトリtrait
│   ├── orchestration/         #   [オーケストレーション] フェーズ、結果、戦略trait
│   ├── agent/                 #   [エージェント] 自律実行の状態管理
│   ├── tool/                  #   [ツール] ツール定義、呼び出し、リスクレベル
│   ├── interaction/           #   [インタラクション] 対話形式、ネスト管理
│   ├── context/               #   [コンテキスト] プロジェクト情報、リソース参照
│   ├── prompt/                #   [プロンプト] テンプレート
│   └── config/                #   [設定] 出力形式など
│
├── application/               # アプリケーション層
│   ├── ports/                 #   ポート定義（11トレイト）
│   ├── use_cases/             #   ユースケース実装
│   │   ├── run_agent/         #     エージェント実行（4ファイル分割）
│   │   ├── run_quorum.rs      #     合議実行
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
│   ├── context/               #   [Context] LocalContextLoader
│   ├── logging/               #   [Logging] JsonlConversationLogger
│   ├── reference/             #   [Reference] GitHubReferenceResolver
│   └── config/                #   [Config] FileConfig, ConfigLoader
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

### Plugin Architecture / プラグインアーキテクチャ

垂直分割とトレイトの組み合わせにより、**プラグイン的に機能を追加**できます。

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
├── strategy.rs     # OrchestrationStrategy enum + StrategyExecutor trait（既存）
├── mode.rs         # ConsensusLevel enum（既存）
├── scope.rs        # PhaseScope enum（既存）
└── session_mode.rs # SessionMode（既存）

新しいプレゼンテーション追加（例: HTTP API）:
presentation/
├── cli/            # 既存: CLI
├── tui/            # 既存: TUI
└── server/         # 新規追加
    ├── mod.rs
    ├── http.rs     # Actix-web ハンドラ
    └── dto.rs      # リクエスト/レスポンス型
```

#### プラグイン性を支える設計原則

| 原則 | 実装 | 効果 |
|------|------|------|
| **依存性逆転** | ドメイン層でtrait定義、インフラ層で実装 | 実装を差し替え可能 |
| **統一インターフェース** | `LlmGateway`, `StrategyExecutor` | 新実装が既存コードと自動統合 |
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
| JSON Schema 変換を Port パターンで分離 | domain 層はツールの定義・フィルタリングのみ担当し、LLM API フォーマット（JSON Schema）への変換は `ToolSchemaPort` 経由で infrastructure 層が実装 |
| インタラクション形式を対等な peer に | Agent / Ask / Discuss を階層化せず、全て `InteractionForm` enum の対等なバリアント |

### TUI Design Philosophy / TUI 設計思想

> See also: [Discussion #58: Neovim-Style Extensible TUI](https://github.com/music-brain88/copilot-quorum/discussions/58)

copilot-quorum の TUI は「Vim キーバインド付きの REPL」ではなく、
**LLM オーケストレーションに最適化されたモーダルインターフェース**として設計されています。

#### Core Principle: Orchestrator, Not Editor / 核心原則: オーケストレーター、エディタではない

copilot-quorum の本質は **LLM 群を指揮するオーケストレーター**です。
テキスト編集は本業ではありません。

この原則から導かれる設計判断:

| 判断 | 理由 |
|------|------|
| NORMAL モードがホームポジション | 「指揮者の操作盤」= オーケストレーション操作が主 |
| $EDITOR 委譲（`I`） | エディタを再実装せず、ユーザーの本物の vim/neovim を呼ぶ |
| INSERT は対話的入力に特化 | 内蔵エディタの完成度を競わない |
| NORMAL キーバインドはオーケストレーション操作 | `d` = Discuss, `s` = Solo, `e` = Ensemble（vim の delete/substitute ではない） |

#### Input Granularity Model / 入力粒度モデル

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

#### Tab + Pane Architecture / タブ・ペインアーキテクチャ

TUI は Vim のバッファ/ウィンドウ/タブページモデルを踏襲しています：

| Vim | copilot-quorum | 説明 |
|-----|----------------|------|
| Buffer | `Interaction` (domain) | 対話データ（Agent/Ask/Discuss） |
| Window | `Pane` (presentation) | 表示ユニット（会話、プログレス等を保持） |
| Tab Page | `Tab` (presentation) | 1つ以上のペインを含むタブ |

`TabManager` がタブの作成・切り替え・インタラクションとのバインドを管理します。

#### Modal Architecture / モーダルアーキテクチャ

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

## Layer Structure / レイヤー構成

```
copilot-quorum/
├── domain/              # ドメイン層 - ビジネスロジックの核心
│   ├── core/            # 共通ドメイン概念 (Model, Question, Error)
│   ├── session/         # LLMセッションドメイン
│   ├── orchestration/   # Quorumオーケストレーションドメイン
│   ├── agent/           # エージェント自律実行ドメイン
│   ├── tool/            # ツール定義・実行ドメイン
│   ├── interaction/     # インタラクション形式・ネスト管理
│   ├── context/         # プロジェクトコンテキスト・リソース参照
│   ├── quorum/          # 合意形成（Vote, QuorumRule）
│   ├── prompt/          # プロンプトドメイン
│   └── config/          # 設定ドメイン
│
├── application/         # アプリケーション層 - ユースケース
│   ├── ports/           # ポート定義（11トレイト）
│   ├── use_cases/       # ユースケース
│   └── config/          # QuorumConfig, ExecutionParams
│
├── infrastructure/      # インフラ層 - 技術的実装
│   ├── copilot/         # Copilot CLIアダプター (Gateway, Router, Session)
│   ├── tools/           # ToolRegistry, プロバイダー群, Schema変換
│   ├── context/         # LocalContextLoader
│   ├── logging/         # JsonlConversationLogger
│   ├── reference/       # GitHubReferenceResolver
│   └── config/          # FileConfig, ConfigLoader
│
├── presentation/        # プレゼンテーション層 - UI
│   ├── cli/             # CLIコマンド定義
│   ├── tui/             # モーダル TUI (ratatui)
│   ├── agent/           # Agent UI コンポーネント
│   ├── output/          # 出力フォーマッター
│   ├── progress/        # プログレス表示
│   └── config/          # OutputConfig, ReplConfig
│
└── cli/                 # エントリポイント (DI構築)
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

`domain/src/core/`

| Type | Kind | Description |
|------|------|-------------|
| `Model` | Value Object | 利用可能なAIモデル（Claude, GPT, Gemini等） |
| `Question` | Value Object | Quorumに投げかける質問 |
| `DomainError` | Error | ドメインレベルのエラー |

### Quorum Module

`domain/src/quorum/`

Quorum（合意形成）に関する型を定義します。

| Type | Kind | Description |
|------|------|-------------|
| `Vote` | Value Object | モデルからの投票（承認/却下 + 理由） |
| `VoteResult` | Value Object | 投票結果の集計 |
| `QuorumRule` | Value Object | 合意ルール（過半数、全会一致など） |
| `ConsensusRound` | Entity | 投票ラウンドの記録 |
| `ConsensusOutcome` | Value Object | 結果（Approved, Rejected, Pending） |

### Session Module

`domain/src/session/`

| Type | Kind | Description |
|------|------|-------------|
| `Session` | Entity | LLMとの会話セッション |
| `Message` | Entity | 会話内のメッセージ |
| `LlmSessionRepository` | Trait | セッション管理の抽象化 |
| `LlmResponse` | Value Object | LLM からの構造化レスポンス（ContentBlock のリスト） |
| `ContentBlock` | Enum | Text / ToolUse / ToolResult |
| `StopReason` | Enum | EndTurn / ToolUse / MaxTokens / StopSequence |
| `StreamEvent` | Enum | Delta / Completed / Error / ToolCallDelta / CompletedResponse |

### Orchestration Module

`domain/src/orchestration/`

#### 3つの直交する設定軸

| 軸 | 型 | バリアント | 説明 |
|----|------|-----------|------|
| **ConsensusLevel** | Enum | `Solo` (default), `Ensemble` | 参加モデル数を制御（単一 or 複数） |
| **PhaseScope** | Enum | `Full` (default), `Fast`, `PlanOnly` | 実行フェーズの範囲を制御 |
| **OrchestrationStrategy** | Enum | `Quorum(QuorumConfig)`, `Debate(DebateConfig)` | 議論の進め方を選択 |

これらは直交しており、任意の組み合わせが可能です（例: `Solo + Fast + Debate`）。

#### 設定コンテナ

| Type | Kind | Description |
|------|------|-------------|
| `SessionMode` | Value Object | ランタイム可変設定（consensus_level, phase_scope, strategy） |
| `PlanningApproach` | Enum (派生) | `ConsensusLevel` から自動導出（Solo→Single, Ensemble→Ensemble） |

#### Quorum Discussion 型

| Type | Kind | Description |
|------|------|-------------|
| `Phase` | Value Object | フェーズ（Initial, Review, Synthesis） |
| `QuorumRun` | Entity | 実行中のQuorumセッション |
| `ModelResponse` | Value Object | モデルからの回答 |
| `PeerReview` | Value Object | ピアレビュー結果 |
| `SynthesisResult` | Value Object | 最終統合結果 |
| `QuorumResult` | Value Object | 全フェーズの結果 |
| `StrategyExecutor` | Trait | オーケストレーション戦略の実行インターフェース |

### Interaction Module

`domain/src/interaction/`

ユーザーとシステムの対話を3つの対等な形式で表現するモジュールです。

| Type | Kind | Description |
|------|------|-------------|
| `InteractionForm` | Enum | Agent / Ask / Discuss — 対話形式 |
| `InteractionId` | Value Object | インタラクションの一意識別子 |
| `Interaction` | Entity | 対話インスタンス（form, context_mode, depth） |
| `InteractionTree` | Entity | ネスト管理のツリー構造（ID自動採番） |
| `InteractionResult` | Enum | AskResult / DiscussResult / AgentResult |
| `SpawnError` | Error | 子インタラクション生成エラー（ParentNotFound, MaxDepthExceeded） |
| `DEFAULT_MAX_NESTING_DEPTH` | Const | 最大ネスト深度（= 3） |

`InteractionForm` は各形式がどの設定型を使うかを決定します：
- `uses_agent_policy()` — Agent のみ true
- `uses_execution_params()` — Agent と Ask が true
- `default_context_mode()` — Agent/Discuss → Full, Ask → Projected

### Agent Module

`domain/src/agent/`

| Type | Kind | Description |
|------|------|-------------|
| `AgentState` | Entity | エージェント実行の現在状態 |
| `AgentPhase` | Enum | ContextGathering / Planning / PlanReview / Executing / FinalReview / Completed |
| `SessionMode` | Value Object | ランタイム可変オーケストレーション設定 |
| `ModelConfig` | Value Object | ロールベースモデル選択（exploration, decision, review） |
| `AgentPolicy` | Value Object | ドメイン動作制約（HiL、レビュー設定） |
| `HilMode` | Enum | Interactive / AutoApprove / AutoReject |
| `HumanDecision` | Enum | Approve / Reject / Edit(Plan) |
| `Plan` | Value Object | タスク計画（目的、理由付け、タスクリスト） |
| `Task` | Value Object | 単一タスク（ツール呼び出し、依存関係） |
| `EnsemblePlanResult` | Entity | Ensemble 計画の結果（候補リスト + 選択） |
| `PlanCandidate` | Value Object | Ensemble 候補プラン（モデル + スコア） |
| `ReviewRound` | Entity | レビューラウンドの記録（投票リスト + 承認結果） |
| `ModelVote` | Value Object | モデルからの投票（モデル名、承認/却下、フィードバック） |
| `ToolExecution` | Entity | ツール実行のライフサイクル追跡（Pending → Running → Completed/Error） |
| `ToolExecutionState` | Enum | ツール実行の状態マシン |
| `ConfigIssue` | Value Object | 設定バリデーション問題 |

#### ToolExecution State Machine

```
Pending ──> Running ──> Completed
                   └──> Error
```

`ToolExecution` は各ツール呼び出しのライフサイクルを追跡します。
状態遷移は enum ベースで、各状態に有効なフィールドのみを持ちます。

### Tool Module

`domain/src/tool/`

| Type | Kind | Description |
|------|------|-------------|
| `ToolDefinition` | Entity | ツールのメタデータ（名前、説明、リスクレベル） |
| `ToolParameter` | Value Object | ツールパラメータの定義 |
| `ToolCall` | Value Object | ツール呼び出し（引数付き、`native_id` でAPI相関） |
| `ToolResult` | Value Object | 実行結果（成功/失敗、出力） |
| `ToolResultMetadata` | Value Object | 実行メタデータ（duration_ms, bytes, path, exit_code, match_count） |
| `ToolSpec` | Entity | 利用可能なツールのレジストリ |
| `RiskLevel` | Enum | Low（読み取り専用）/ High（変更あり） |
| `ToolValidator` | Trait | ツール呼び出しのバリデーションロジック |
| `ToolProvider` | Trait | 外部ツールプロバイダーの抽象化 |

### Context Module

`domain/src/context/`

| Type | Kind | Description |
|------|------|-------------|
| `ProjectContext` | Entity | プロジェクトの統合コンテキスト |
| `KnownContextFile` | Value Object | 既知のコンテキストファイル種別（CLAUDE.md, README.md等） |
| `LoadedContextFile` | Value Object | 読み込まれたファイルの内容 |
| `ContextMode` | Enum | Full / Projected / Fresh — コンテキスト投影モード |
| `ResourceReference` | Enum | GitHubIssue / GitHubPullRequest — テキスト中のリソース参照 |
| `extract_references()` | Function | テキストからリソース参照を抽出 |

#### ContextMode

タスクやインタラクションに渡すコンテキスト量を制御します。Vim のバッファコマンドのアナロジーです：

| Mode | 渡すコンテキスト | Vim アナロジー |
|------|-----------------|----------------|
| `Full` | 全 `AgentContext` | `:split` — 同じバッファを共有 |
| `Projected` | タスクの `context_brief` のみ | `:edit` — 特定ファイルを開く |
| `Fresh` | なし | `:enew` — 空バッファで開始 |

#### ResourceReference

テキスト中の GitHub Issue/PR 参照を自動検出します：

- GitHub URL: `github.com/{owner}/{repo}/(issues|pull)/{N}`
- クロスリポ参照: `{owner}/{repo}#{N}`
- 型付き参照: `Issue #N`, `PR #N`, `Pull Request #N`
- 範囲参照: `#N-M`（M-N <= 10）
- ベア参照: `#N`

### Prompt Module

`domain/src/prompt/`

| Type | Kind | Description |
|------|------|-------------|
| `PromptTemplate` | Service | 各フェーズのプロンプトテンプレート |
| `AgentPromptTemplate` | Service | エージェント専用プロンプト（system, plan, review） |

### Config Module

`domain/src/config/`

| Type | Kind | Description |
|------|------|-------------|
| `OutputFormat` | Enum | Full / Synthesis / Json |

---

## Application Layer / アプリケーション層

ユースケースとポート（外部インターフェース）を定義。

### Ports (Interfaces) / ポート

| Trait | Module | Description |
|-------|--------|-------------|
| `LlmGateway` | `llm_gateway` | LLMプロバイダーへのゲートウェイ |
| `LlmSession` | `llm_gateway` | アクティブなLLMセッション（send, send_with_tools, send_tool_results） |
| `ToolExecutorPort` | `tool_executor` | ツール実行の抽象化 |
| `ToolSchemaPort` | `tool_schema` | ツール定義 → JSON Schema 変換の抽象化 |
| `ContextLoaderPort` | `context_loader` | コンテキストファイル読み込みの抽象化 |
| `ProgressNotifier` | `progress` | Quorum 進捗通知コールバック |
| `AgentProgressNotifier` | `agent_progress` | エージェント進捗通知コールバック |
| `HumanInterventionPort` | `human_intervention` | 人間介入の抽象化（プラン承認/却下/編集、実行確認） |
| `ActionReviewer` | `action_reviewer` | 高リスクツール呼び出しのレビュー抽象化 |
| `ConversationLogger` | `conversation_logger` | 構造化会話ログの記録（JSONL） |
| `ReferenceResolverPort` | `reference_resolver` | リソース参照の解決（GitHub Issue/PR → コンテンツ） |
| `UiEvent` | `ui_event` | アプリケーション → プレゼンテーション層への出力イベント |

#### UiEvent（出力ポート）

`UiEvent` は `AgentController` からプレゼンテーション層へのイベントチャネルです。
Welcome, ModeChanged, AgentResult, QuorumResult, AskResult, InteractionSpawned, InteractionCompleted 等の
バリアントで、UI の種類（CLI, TUI, 将来の Web）に依存しない形でイベントを伝達します。

### Use Cases / ユースケース

| Type | Module | Description |
|------|--------|-------------|
| `RunAgentUseCase` | `run_agent/` | エージェント自律実行（Phase 1-5 全体オーケストレーション） |
| `RunQuorumUseCase` | `run_quorum` | Quorum（合議）実行 |
| `RunAskUseCase` | `run_ask` | Ask インタラクション（読み取り専用ツールでの Q&A） |
| `GatherContextUseCase` | `gather_context` | Phase 1: コンテキスト収集（3段階フォールバック） |
| `ExecuteTaskUseCase` | `execute_task` | Phase 4: タスク実行（動的モデル選択 + アクションレビュー） |
| `InitContextUseCase` | `init_context` | コンテキストファイル（.quorum/context.md）の初期化 |
| `AgentController` | `agent_controller` | REPL/TUI のビジネスロジック。コマンド処理、UiEvent 発信 |

#### run_agent/ ファイル分割

`RunAgentUseCase` は責務ごとに4ファイルに分割されています：

| File | Description |
|------|-------------|
| `mod.rs` | メインオーケストレーション（Phase 1-5 フロー） |
| `types.rs` | `RunAgentInput`, `RunAgentOutput`, `RunAgentError` |
| `planning.rs` | Solo/Ensemble 計画生成ロジック |
| `review.rs` | Quorum プランレビュー + アクションレビュー |
| `hil.rs` | 人間介入ハンドリング（execution confirmation 含む） |

### Config / 設定

| Type | Module | Description |
|------|--------|-------------|
| `QuorumConfig` | `config/quorum_config` | 4型コンテナ（SessionMode, ModelConfig, AgentPolicy, ExecutionParams） |
| `ExecutionParams` | `config/execution_params` | max_iterations, max_tool_turns, max_tool_retries, working_dir, ensemble_session_timeout |

`QuorumConfig` はバッファ伝搬のための統合コンテナで、`mode_mut()` でランタイム変更可能。
`to_agent_input()`, `to_quorum_input()` でユースケース入力を生成します。

---

## Infrastructure Layer / インフラ層

アプリケーション層のポートを実装するアダプター。

### Copilot Adapter

`infrastructure/src/copilot/`

| Type | Implements | Description |
|------|------------|-------------|
| `CopilotLlmGateway` | `LlmGateway` | Copilot CLI経由のLLMゲートウェイ |
| `CopilotSession` | `LlmSession` | Copilotセッション（send_with_tools, send_tool_results） |
| `MessageRouter` | - | TCP demultiplexer（セッション間メッセージルーティング） |
| `SessionChannel` | - | セッション専用の受信チャネル（Drop時に自動登録解除） |
| `CopilotError` | - | Copilot通信エラー（RouterStopped含む） |

> 詳細は [systems/transport.md](../systems/transport.md) を参照してください。

### Tools Adapter

`infrastructure/src/tools/`

ツールシステムはプラグインベースのアーキテクチャを採用しています（詳細は [Tool Provider System](#tool-provider-system--ツールプロバイダーシステム) を参照）。

| Type | Implements | Description |
|------|------------|-------------|
| `ToolRegistry` | `ToolExecutorPort` | プロバイダーを集約、優先度でルーティング |
| `BuiltinProvider` | `ToolProvider` | 最小限の組み込みツール（priority: -100） |
| `CliToolProvider` | `ToolProvider` | システムCLIツールのラッパー（priority: 50） |
| `CustomToolProvider` | `ToolProvider` | ユーザー定義カスタムツール（priority: 75） |
| `JsonSchemaToolConverter` | `ToolSchemaPort` | ツール定義 → JSON Schema 変換（Port パターン） |
| `LocalToolExecutor` | `ToolExecutorPort` | ファイル操作、コマンド実行、検索、Web ツール |

#### 利用可能なツール

**Builtin Provider (priority: -100):**
- `read_file` - ファイル内容の読み取り（Low risk）
- `write_file` - ファイルの書き込み/作成（High risk）
- `run_command` - シェルコマンド実行（High risk）
- `glob_search` - パターンによるファイル検索（Low risk）
- `grep_search` - ファイル内容の検索（Low risk）

**CLI Provider (priority: 50):**
- `grep_search` - grep/rg によるファイル内容検索（Low risk）
- `glob_search` - find/fd によるファイルパターン検索（Low risk）

**Web Tools (`web-tools` feature, default in CLI):**
- `web_fetch` - URL からコンテンツを取得（Low risk）
- `web_search` - Web 検索（Low risk）

**Custom Tools (priority: 75, `quorum.toml` で設定):**
- ユーザー定義のシェルコマンドテンプレート（default risk: High）

### Context Adapter

`infrastructure/src/context/`

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

### Logging Adapter

`infrastructure/src/logging/`

| Type | Implements | Description |
|------|------------|-------------|
| `JsonlConversationLogger` | `ConversationLogger` | JSONL ファイルへの構造化会話ログ記録 |

各 `ConversationEvent` を `type` + `timestamp` 付きの JSON 行として追記。
`Mutex<BufWriter<File>>` でスレッドセーフ。Drop 時に flush。
`tracing` の診断ログとは分離された、会話トランスクリプト専用のログです。

### Reference Adapter

`infrastructure/src/reference/`

| Type | Implements | Description |
|------|------------|-------------|
| `GitHubReferenceResolver` | `ReferenceResolverPort` | `gh` CLI 経由で GitHub Issue/PR を解決 |

`try_new()` で `gh` CLI の存在と認証状態をチェックし、不在時は `None` で graceful degradation。
`gh issue view --json title,body` で Issue と PR の両方を解決します。
`resolve_all()` は `futures::future::join_all` で並列解決。

### Config Adapter

`infrastructure/src/config/`

| Type | Description |
|------|-------------|
| `ConfigLoader` | `quorum.toml` / `~/.config/copilot-quorum/config.toml` の読み込み |
| `FileConfig` | ファイルベースの設定構造体（TOML デシリアライズ） |
| `FileCustomToolConfig` | カスタムツール設定（command テンプレート + parameters） |

---

## Presentation Layer / プレゼンテーション層

ユーザーインターフェースと出力フォーマット。

### CLI Module

`presentation/src/cli/`

| Type | Description |
|------|-------------|
| `Cli` | CLAPコマンド定義（--ensemble, --chat, --model, etc.） |
| `CliOutputFormat` | CLI用出力形式 |

### TUI Module

`presentation/src/tui/`

モーダルインターフェースの実装。ratatui ベース。

| Type | Description |
|------|-------------|
| `TuiApp` | メインアプリケーション（イベントループ、レンダリング） |
| `TuiState` | TUI 全体の状態（タブ、入力バッファ、モード） |
| `TuiPresenter` | `UiEvent` → `TuiEvent` 変換 + `RoutedTuiEvent` 送信 |
| `TuiProgressBridge` | `AgentProgressNotifier` → `TuiEvent` ブリッジ |
| `TuiHumanIntervention` | `HumanInterventionPort` の TUI 実装 |
| `InputMode` | Normal / Insert / Command |
| `KeyAction` | キーバインドアクション enum |
| `TuiInputConfig` | 入力設定（submit_key, max_height, dynamic_height 等） |

#### Tab + Pane

| Type | Description |
|------|-------------|
| `TabManager` | タブの作成・切り替え・インタラクションバインド管理 |
| `Tab` | タブ（TabId, ペインリスト） |
| `Pane` | ペイン（PaneId, PaneKind, messages, streaming_text, progress） |
| `PaneKind` | Interaction(InteractionForm, Option<InteractionId>) |

#### Events

| Type | Description |
|------|-------------|
| `TuiCommand` | TUI → Controller のコマンド（ProcessRequest, HandleCommand, SpawnInteraction, etc.） |
| `TuiEvent` | Controller → TUI のイベント（Welcome, ModeChanged, AgentResult, etc.） |
| `RoutedTuiEvent` | `TuiEvent` + ルーティング用 `interaction_id` |

#### Widgets

| Type | Description |
|------|-------------|
| `MainLayout` | メインレイアウト計算（Header + TabBar + Conversation + Progress + Input + StatusBar） |
| `conversation` | 会話表示ウィジェット |
| `header` | ヘッダーウィジェット |
| `input` | 入力エリアウィジェット（動的高さ対応） |
| `progress_panel` | プログレスパネルウィジェット |
| `status_bar` | ステータスバーウィジェット |
| `tab_bar` | タブバーウィジェット |

### Agent Module

`presentation/src/agent/`

エージェント実行の UI コンポーネント。

| Type | Description |
|------|-------------|
| `AgentProgressReporter` | `AgentProgressNotifier` の CLI 実装（indicatif） |
| `SimpleAgentProgress` | シンプルなテキスト進捗表示 |
| `InteractiveHumanIntervention` | `HumanInterventionPort` の CLI 実装（対話的承認/却下） |
| `ReplPresenter` | `UiEvent` の CLI レンダリング |
| `ThoughtStream` | エージェント思考の表示 |

### Output Module

`presentation/src/output/`

| Type | Description |
|------|-------------|
| `OutputFormatter` | 出力フォーマッターのトレイト |
| `ConsoleFormatter` | コンソール向け色付き出力 |

### Progress Module

`presentation/src/progress/`

| Type | Description |
|------|-------------|
| `ProgressReporter` | indicatifによるプログレスバー |
| `SimpleProgress` | シンプルなテキスト進捗表示 |

### Config Module

`presentation/src/config/`

| Type | Description |
|------|-------------|
| `OutputConfig` | 出力設定（format, color） |
| `ReplConfig` | REPL設定（show_progress, history_file） |

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
|  |                    AgentController (TUI/REPL)                       |   |
|  |                                                                     |   |
|  |  UiEvent channel  ←─────────────────────────────── TuiPresenter    |   |
|  |                                                                     |   |
|  |  ┌─────────────────────────────────────────────────────────────┐   |   |
|  |  │  RunAgentUseCase (Phase 1-5)                                │   |   |
|  |  │    ├── GatherContextUseCase (Phase 1)                       │   |   |
|  |  │    ├── Planning + Review   (Phase 2-3)                      │   |   |
|  |  │    └── ExecuteTaskUseCase  (Phase 4)                        │   |   |
|  |  └─────────────────────────────────────────────────────────────┘   |   |
|  |                                                                     |   |
|  |  ┌─────────────────────────────────────────────────────────────┐   |   |
|  |  │  RunQuorumUseCase                                           │   |   |
|  |  │    Phase 1: Initial Query (parallel)                        │   |   |
|  |  │    Phase 2: Peer Review (parallel)                          │   |   |
|  |  │    Phase 3: Synthesis (moderator)                           │   |   |
|  |  └─────────────────────────────────────────────────────────────┘   |   |
|  |                                                                     |   |
|  |  ┌─────────────────────────────────────────────────────────────┐   |   |
|  |  │  RunAskUseCase                                              │   |   |
|  |  │    Low-risk tool loop → direct answer                       │   |   |
|  |  └─────────────────────────────────────────────────────────────┘   |   |
|  +---------------------------------------------------------------------+   |
|                                                                            |
+==================================+=========================================+
                                   |
                                   v
+===========================================================================+
|                          infrastructure/                                   |
|                                                                            |
|  +------------------+    +------------------+    +---------------------+   |
|  | CopilotLlmGateway|----> MessageRouter    |----> copilot CLI (JSON) |   |
|  +------------------+    +-------+----------+    +---------------------+   |
|                                  |                                         |
|                          +-------+-------+                                 |
|                          | SessionChannel | (per session)                  |
|                          +---------------+                                 |
|                                                                            |
|  +------------------+    +------------------+    +---------------------+   |
|  | ToolRegistry     |    | JsonlConv.Logger |    | GitHubRefResolver  |   |
|  | (providers)      |    | (.jsonl file)    |    | (gh CLI)           |   |
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
4. **MessageRouter** が背景タスクで TCP reader を占有、session_id でルーティング
5. LSP形式のヘッダー + JSON-RPCでメッセージ交換

### Transport Demultiplexer / トランスポート多重分離

> 詳細は [systems/transport.md](../systems/transport.md) を参照してください。

単一の TCP 接続上で複数セッションを並列運用するため、`MessageRouter` がメッセージを
session_id ベースで各 `SessionChannel` にルーティングします。

```
Session A ← channel_a ←┐
Session B ← channel_b ←┤── MessageRouter (background reader task)
Session C ← channel_c ←┘        │
                                 └── TCP reader (single owner, no Mutex)
```

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

各 `JoinSet::spawn` 内で `gateway.create_session()` が呼ばれ、`MessageRouter` が
session_id 毎に独立した `SessionChannel` を払い出すため、並列セッション間でメッセージが
混線することはありません（詳細は [systems/transport.md](../systems/transport.md)）。

---

## Agent System / エージェントシステム

> 詳細は [systems/agent-system.md](../systems/agent-system.md) を参照してください。

エージェントシステムは、Quorumの概念を自律タスク実行に拡張したものです。
Solo モードで動作し、重要なポイントでは Quorum Consensus によるレビューを行います。

### Agent Flow / エージェントフロー

```
User Request
    │
    ▼
┌───────────────────┐
│ Context Gathering │  ← GatherContextUseCase: 3段階フォールバック
│  (Phase 1)        │    1. 既知ファイル直接読み込み
└───────────────────┘    2. 探索エージェント (tool use)
    │                    3. 最小コンテキストで続行
    ▼
┌───────────────────┐
│     Planning      │  ← Solo: decision_model が計画作成
│  (Phase 2)        │    Ensemble: review_models が並列計画 + 投票
└───────────────────┘
    │
    ▼
┌───────────────────────────┐
│ Quorum Consensus #1       │  ← review_models が計画をレビュー
│ Plan Review (Phase 3)     │     却下時: フィードバック付き再計画
│                           │     max_plan_revisions 超過: HiL 介入
└───────────────────────────┘
    │
    ▼
┌───────────────────────────┐
│ Execution Confirmation    │  ← PhaseScope::Full のみ
│ (Phase 3b)                │     HilMode に応じて自動/対話的承認
└───────────────────────────┘
    │
    ▼
┌───────────────────┐
│  Task Execution   │  ← ExecuteTaskUseCase
│  (Phase 4)        │    Low-risk: 直接並列実行
│                   │    High-risk: ActionReviewer 経由
└───────────────────┘
    │
    ▼
┌───────────────────────────┐
│ Final Review (Phase 5)    │  ← オプション (require_final_review: true)
│                           │     実行結果全体をレビュー
└───────────────────────────┘
```

### PhaseScope による制御

| Phase | Full | Fast | PlanOnly |
|-------|------|------|----------|
| 1. Context Gathering | yes | yes | yes |
| 2. Planning | yes | yes | yes |
| 3. Plan Review (Quorum) | yes | skip | skip |
| 3b. Execution Confirmation | yes | skip | skip |
| 4. Task Execution | yes | yes | skip+return |
| 4a. Action Review | yes | skip | N/A |
| 5. Final Review | opt | skip | N/A |

### Quorum Consensus / 合意形成

Quorum Consensus は複数モデルの投票によって安全性を確保します：

1. **Plan Review（必須）**: 設定された全 review_models が提案された計画をレビュー
2. **Action Review（高リスク操作）**: `write_file` と `run_command` の実行前にレビュー
3. **Final Review（オプション）**: 実行結果全体をレビュー

承認には過半数（または設定された QuorumRule）の投票が必要。却下された計画/アクションには集約されたフィードバックが含まれます。

### Risk Levels / リスクレベル

| Risk Level | Tools | Behavior |
|------------|-------|----------|
| Low | `read_file`, `glob_search`, `grep_search`, `web_fetch`, `web_search` | 直接並列実行（レビューなし） |
| High | `write_file`, `run_command` | ActionReviewer によるレビュー後に実行 |

### Progress Notification Pattern / 進捗通知パターン

エージェントシステムは「アクションとUI通知の分離」パターンを採用しています。
これはVuex/Fluxのような単方向データフローに似た設計です。

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
└── AgentProgressNotifier (Presentation層) ──→ UI表示
```

---

## Tool Provider System / ツールプロバイダーシステム

> 詳細は [systems/tool-system.md](../systems/tool-system.md) を参照してください。

ツールプロバイダーシステムは、**プラグインベースのオーケストレーション**アーキテクチャを採用しています。

### Architecture / アーキテクチャ

```
┌─────────────────────────────────────────────────────────────┐
│                     ToolRegistry                            │
│  (プロバイダーを集約、優先度でルーティング)                 │
└─────────────────────────────────────────────────────────────┘
          │              │              │              │
          ▼              ▼              ▼              ▼
   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
   │ Builtin  │   │   CLI    │   │  Custom  │   │   MCP    │
   │ Provider │   │ Provider │   │ Provider │   │ Provider │
   └──────────┘   └──────────┘   └──────────┘   └──────────┘
   最小限の        rg, fd, gh     quorum.toml     MCP サーバー
   フォールバック   等をラップ    [tools.custom]   を統合
   (優先度: -100)  (優先度: 50)  (優先度: 75)    (優先度: 100)
```

### Provider Types / プロバイダーの種類

| Provider | Priority | Description | Use Case |
|----------|----------|-------------|----------|
| **MCP** | 100 | MCP サーバー経由のツール | 外部サーバーとの連携、豊富な機能 |
| **Custom** | 75 | ユーザー定義シェルコマンド | カスタム処理、プロジェクト固有ツール |
| **CLI** | 50 | システムCLIツールのラッパー | grep/rg, find/fd, cat/bat |
| **Builtin** | -100 | 最小限の組み込みツール | フォールバック、常に利用可能 |

### Module Structure / モジュール構造

```
infrastructure/src/tools/
├── mod.rs              # 全体エクスポート
├── registry.rs         # ToolRegistry (ToolExecutorPort 実装)
├── executor.rs         # LocalToolExecutor
├── schema.rs           # JsonSchemaToolConverter (ToolSchemaPort 実装)
├── custom_provider.rs  # CustomToolProvider (priority: 75)
├── builtin/
│   ├── mod.rs
│   └── provider.rs     # BuiltinProvider (priority: -100)
├── cli/
│   ├── mod.rs
│   ├── provider.rs     # CliToolProvider (priority: 50)
│   └── discovery.rs    # 推奨ツール検知 & 提案
├── web/
│   ├── mod.rs
│   ├── fetch.rs        # web_fetch (feature-gated: web-tools)
│   └── search.rs       # web_search (feature-gated: web-tools)
├── file.rs             # read_file, write_file 実装
├── command.rs          # run_command 実装
└── search.rs           # glob_search, grep_search 実装
```

---

## Error Handling / エラーハンドリング

| Error Type | Location | Handling |
|------------|----------|----------|
| `DomainError` | `domain/` | ドメインルール違反 |
| `GatewayError` | `application/` | LLMゲートウェイエラー |
| `RunAgentError` | `application/` | エージェント実行エラー |
| `RunAskError` | `application/` | Ask実行エラー |
| `RunQuorumError` | `application/` | Quorum実行エラー |
| `CopilotError` | `infrastructure/` | Copilot CLI通信エラー（RouterStopped含む） |
| `ReferenceError` | `application/` | リソース参照解決エラー |
| `HumanInterventionError` | `application/` | 人間介入エラー（Cancelled, IoError） |
| `SpawnError` | `domain/` | インタラクション生成エラー |

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

**Step 1**: `OrchestrationStrategy` enum にバリアントを追加（`domain/src/orchestration/strategy.rs`）：

```rust
pub enum OrchestrationStrategy {
    Quorum(QuorumConfig),
    Debate(DebateConfig),
    NewStrategy(NewStrategyConfig),  // ← 新規バリアント追加
}
```

**Step 2**: `StrategyExecutor` trait を実装する実行者を追加。

### Adding New Tools / 新しいツールの追加

#### Option 1: カスタムツール（設定のみ）

`quorum.toml` に追加するだけ：

```toml
[tools.custom.my_tool]
description = "Run my custom tool"
command = "my-command {input}"
risk_level = "low"

[tools.custom.my_tool.parameters.input]
description = "Input to process"
required = true
```

#### Option 2: BuiltinProvider への追加

`infrastructure/tools/builtin/` に新しいツール実装を追加し、`default_tool_spec()` に登録。

#### Option 3: 新しい ToolProvider の実装

`ToolProvider` trait を実装し、`ToolRegistry` に登録。

### Adding New Interaction Forms / 新しいインタラクション形式の追加

`domain/src/interaction/mod.rs` の `InteractionForm` にバリアントを追加し、
対応するユースケースとプレゼンテーションを実装。

### Adding New Context File Types / 新しいコンテキストファイル種別の追加

`domain/context/value_objects.rs` の `KnownContextFile` enum に新しいファイル種別を追加。

---

## Testing Strategy / テスト戦略

オニオンアーキテクチャにより、各層を独立してテスト可能：

| Layer | Test Type | Description |
|-------|-----------|-------------|
| domain | Unit | ドメインロジックの単体テスト（InteractionTree, ContextMode, ToolExecution等） |
| application | Unit + Integration | ScriptedGateway/MockToolExecutor でフローテスト |
| infrastructure | Integration | 実際のCopilot CLIとの結合テスト |
| presentation | Unit | フォーマッターの出力テスト |

### Flow Test Infrastructure

`application/src/use_cases/run_agent/` のテストでは以下のテストインフラを使用：

| Type | Description |
|------|-------------|
| `ScriptedGateway` | モデル名ごとにスクリプト化されたレスポンスを返すモックゲートウェイ |
| `ScriptedSession` | 順序付きレスポンスを返すモックセッション |
| `FlowTestBuilder` | Solo/Fast/PlanOnly/Ensemble のフローテスト構築用ビルダー |
| `TrackingProgress` | フェーズ遷移を記録するモックプログレス |
| `MockToolExecutor` | 呼び出しを記録して成功を返すモックツール実行器 |
| `MockHumanIntervention` | 設定可能な HiL 決定を返すモック |

```bash
# Run all tests
cargo test --workspace

# Run domain tests only
cargo test -p quorum-domain

# Run with coverage
cargo llvm-cov --workspace
```

<!--
LLM Context: Architecture Reference

Key types and locations:
- Domain: InteractionForm, InteractionTree, Interaction, InteractionResult, ContextMode, ResourceReference (domain/src/interaction/, domain/src/context/)
- Domain: ToolExecution, ToolExecutionState, ToolExecutionId (domain/src/agent/tool_execution.rs)
- Domain: ConsensusLevel, PhaseScope, OrchestrationStrategy, SessionMode (domain/src/orchestration/)
- Domain: AgentState, Plan, Task, ModelConfig, AgentPolicy, HilMode, HumanDecision (domain/src/agent/)
- Domain: ToolDefinition, ToolCall, ToolResult, ToolSpec, RiskLevel, ToolProvider (domain/src/tool/)
- Domain: LlmResponse, ContentBlock, StopReason, StreamEvent (domain/src/session/)
- Application Ports: LlmGateway, LlmSession, ToolExecutorPort, ToolSchemaPort, ContextLoaderPort, ProgressNotifier, AgentProgressNotifier, HumanInterventionPort, ActionReviewer, ConversationLogger, ReferenceResolverPort, UiEvent (application/src/ports/)
- Application Use Cases: RunAgentUseCase (run_agent/), RunQuorumUseCase, RunAskUseCase, GatherContextUseCase, ExecuteTaskUseCase, AgentController, InitContextUseCase (application/src/use_cases/)
- Application Config: QuorumConfig, ExecutionParams (application/src/config/)
- Infrastructure: CopilotLlmGateway, CopilotSession, MessageRouter, SessionChannel (infrastructure/src/copilot/)
- Infrastructure: ToolRegistry, LocalToolExecutor, BuiltinProvider, CliToolProvider, CustomToolProvider, JsonSchemaToolConverter (infrastructure/src/tools/)
- Infrastructure: JsonlConversationLogger (infrastructure/src/logging/)
- Infrastructure: GitHubReferenceResolver (infrastructure/src/reference/)
- Infrastructure: ConfigLoader, FileConfig (infrastructure/src/config/)
- Presentation: TuiApp, TuiState, TuiPresenter, TuiProgressBridge, TuiHumanIntervention (presentation/src/tui/)
- Presentation: TabManager, Tab, Pane, PaneKind (presentation/src/tui/tab.rs)
- Presentation: TuiCommand, TuiEvent, RoutedTuiEvent (presentation/src/tui/event.rs)
- Presentation: MainLayout, widgets/* (presentation/src/tui/widgets/)
- Presentation: AgentProgressReporter, InteractiveHumanIntervention, ReplPresenter, ThoughtStream (presentation/src/agent/)
- Presentation: ConsoleFormatter (presentation/src/output/)
- Presentation: ProgressReporter, SimpleProgress (presentation/src/progress/)
- Presentation: Cli (presentation/src/cli/)
- CLI: main.rs DI assembly (cli/src/main.rs)

Module structure:
- domain/src/: core/, quorum/, session/, orchestration/, agent/, tool/, interaction/, context/, prompt/, config/, util.rs
- application/src/: ports/ (11 modules), use_cases/ (run_agent/ [mod,types,planning,review,hil], run_quorum, run_ask, gather_context, execute_task, agent_controller, init_context, shared), config/
- infrastructure/src/: copilot/ (gateway, session, router, transport, protocol, error), tools/ (registry, executor, schema, custom_provider, builtin/, cli/, web/), context/, logging/, reference/, config/
- presentation/src/: cli/, tui/ (app, state, presenter, progress, human_intervention, editor, mode, event, tab, widgets/), agent/ (progress, thought, human_intervention, presenter), output/, progress/, config/
- cli/src/: main.rs
-->
