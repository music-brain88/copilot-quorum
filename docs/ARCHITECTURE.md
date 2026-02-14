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
├── strategy.rs     # OrchestrationStrategy enum + StrategyExecutor trait（既存）
├── mode.rs         # ConsensusLevel enum（既存）
├── scope.rs        # PhaseScope enum（既存）
└── strategies/     # 新規ディレクトリ
    ├── mod.rs
    └── new_strategy.rs  # 新規: impl StrategyExecutor

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

他のツールとの差別化:

| ツール | テキスト入力 | 本業 |
|--------|-------------|------|
| Claude Code | 内蔵エディタ | 会話 |
| OpenCode | 内蔵 vim 風 | 会話 |
| **copilot-quorum** | **$EDITOR 委譲** | **オーケストレーション** |

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

**なぜこの 3 段階か:**

- **`:ask`** — ex コマンドの即時性。`:w` で保存するように `:ask Fix the bug` で質問。INSERT モードへの遷移不要
- **`i`** — 応答パネルが表示されたまま入力。LLM の出力を参照しながら追加質問や修正指示を出す対話フロー
- **`I`** — `$EDITOR`（vim/neovim）を子プロセスとして起動。`git commit` が `$EDITOR` を呼ぶのと同じ Unix の伝統的パターン。ユーザーの vim 設定・プラグイン・スニペットが全て使える

#### First-Class COMMAND Mode Commands / COMMAND モードのファーストクラスコマンド

`:ask` と `:discuss` は COMMAND モードのファーストクラスコマンドです。
「コマンドの種類（何をするか）」と「入力手段（どれくらい書くか）」は直交する 2 軸として設計されています。

```
                    :command (即時)    i (対話)     I (がっつり)
                    ─────────────────────────────────────────
Solo 質問            :ask              i で入力     I で起動
Quorum Discussion    :discuss          ─            I で起動
```

`:ask` = Solo Agent 実行、`:discuss` = Quorum Discussion。
同じ「LLM にテキストを送る」行為でも、vim のモーダル文法で粒度が自然に分かれます。

#### $EDITOR Delegation / $EDITOR 委譲

`I` キーで `$EDITOR` を全画面起動します。`git commit` が `$EDITOR` を呼ぶのと同じパターンです。

```
[NORMAL] ← ホームポジション
    │
    I → $EDITOR 起動 → プロンプトを書く → :wq で送信 / :q! でキャンセル
    │
    ▼
[NORMAL] に戻る（応答表示後）
```

起動時にコンテキスト情報をコメント行で表示:

```
# --- Quorum Prompt ---
# Mode: Ensemble | Strategy: Quorum
# Buffers: src/auth.rs, README.md
#
# Write your prompt below. Lines starting with # are ignored.
# :wq to send, :q! to cancel
# ---------------------

```

この設計により:
- **実装コスト**: エディタ再実装不要（子プロセス起動のみ）
- **ユーザー体験**: 使い慣れた本物のエディタでプロンプトを書ける
- **責務分離**: copilot-quorum はオーケストレーションに全力集中

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

VISUAL モードは Phase 2 以降。Normal + Insert + Command で十分な初期体験を提供した後に追加。

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

### Quorum Module

Quorum（合意形成）に関する型を定義します。

| Type | Kind | Description |
|------|------|-------------|
| `Vote` | Value Object | モデルからの投票（承認/却下 + 理由） |
| `VoteResult` | Value Object | 投票結果の集計 |
| `QuorumRule` | Value Object | 合意ルール（過半数、全会一致など） |
| `ConsensusRound` | Entity | 投票ラウンドの記録 |
| `ConsensusOutcome` | Value Object | 結果（Approved, Rejected, Pending） |

### Session Module

| Type | Kind | Description |
|------|------|-------------|
| `Session` | Entity | LLMとの会話セッション |
| `Message` | Entity | 会話内のメッセージ |
| `LlmSessionRepository` | Trait | セッション管理の抽象化 |

### Orchestration Module

#### 3つの直交する設定軸

| 軸 | 型 | バリアント | 説明 |
|----|------|-----------|------|
| **ConsensusLevel** | Enum | `Solo` (default), `Ensemble` | 参加モデル数を制御（単一 or 複数） |
| **PhaseScope** | Enum | `Full` (default), `Fast`, `PlanOnly` | 実行フェーズの範囲を制御 |
| **OrchestrationStrategy** | Enum | `Quorum(QuorumConfig)`, `Debate(DebateConfig)` | 議論の進め方を選択 |

これらは直交しており、任意の組み合わせが可能です（例: `Solo + Fast + Debate`）。

#### 派生型

| Type | Kind | Description |
|------|------|-------------|
| `PlanningApproach` | Enum (派生) | `ConsensusLevel` から自動導出（Solo→Single, Ensemble→Ensemble） |

#### Quorum Discussion 型

| Type | Kind | Description |
|------|------|-------------|
| `Phase` | Value Object | フェーズ（Initial, Review, Synthesis） |
| `QuorumConfig` | Entity | Quorum設定（モデル、モデレーター等） |
| `QuorumRun` | Entity | 実行中のQuorumセッション |
| `ModelResponse` | Value Object | モデルからの回答 |
| `PeerReview` | Value Object | ピアレビュー結果 |
| `SynthesisResult` | Value Object | 最終統合結果 |
| `QuorumResult` | Value Object | 全フェーズの結果 |
| `StrategyExecutor` | Trait | オーケストレーション戦略の実行インターフェース |

### Prompt Module

| Type | Kind | Description |
|------|------|-------------|
| `PromptTemplate` | Service | 各フェーズのプロンプトテンプレート |

### Agent Module

| Type | Kind | Description |
|------|------|-------------|
| `AgentState` | Entity | エージェント実行の現在状態 |
| `SessionMode` | Value Object | ランタイム可変オーケストレーション設定 |
| `ModelConfig` | Value Object | ロールベースモデル選択 |
| `AgentPolicy` | Value Object | ドメイン動作制約（HiL、レビュー設定） |
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
| `MessageRouter` | - | TCP demultiplexer（セッション間メッセージルーティング） |
| `SessionChannel` | - | セッション専用の受信チャネル |

> 詳細は [features/transport.md](./features/transport.md) を参照してください。

### Tools Adapter

ツールシステムはプラグインベースのアーキテクチャを採用しています（詳細は [Tool Provider System](#tool-provider-system--ツールプロバイダーシステム) を参照）。

| Type | Implements | Description |
|------|------------|-------------|
| `ToolRegistry` | `ToolExecutorPort` | プロバイダーを集約、優先度でルーティング |
| `BuiltinProvider` | `ToolProvider` | 最小限の組み込みツール（priority: -100） |
| `CliToolProvider` | `ToolProvider` | システムCLIツールのラッパー（priority: 50） |

#### 利用可能なツール

**Builtin Provider:**
- `read_file` - ファイル内容の読み取り（Low risk）
- `write_file` - ファイルの書き込み/作成（High risk）
- `run_command` - シェルコマンド実行（High risk）
- `glob_search` - パターンによるファイル検索（Low risk）
- `grep_search` - ファイル内容の検索（Low risk）

**CLI Provider:**
- `grep_search` - grep/rg によるファイル内容検索（Low risk）
- `glob_search` - find/fd によるファイルパターン検索（Low risk）

CLI Provider は Builtin Provider より高い優先度を持つため、同じ名前のツールは CLI 版が優先されます。

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
|  | CopilotLlmGateway|----> MessageRouter    |----> copilot CLI (JSON) |   |
|  +------------------+    +-------+----------+    +---------------------+   |
|                                  |                                         |
|                          +-------+-------+                                 |
|                          | SessionChannel | (per session)                  |
|                          +---------------+                                 |
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

> 詳細は [features/transport.md](./features/transport.md) を参照してください。

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
混線することはありません（詳細は [features/transport.md](./features/transport.md)）。

---

## Agent System / エージェントシステム

> 詳細は [features/agent-system.md](./features/agent-system.md) を参照してください。

エージェントシステムは、Quorumの概念を自律タスク実行に拡張したものです。
Solo モードで動作し、重要なポイントでは Quorum Consensus によるレビューを行います。

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
┌───────────────────────────┐
│ 🗳️ Quorum Consensus #1   │  ← 全モデルが計画をレビュー（必須）
│   Plan Review             │     過半数の投票で承認/却下
└───────────────────────────┘
    │
    ▼
┌───────────────────┐
│  Task Execution   │
│   ├─ Low-risk  ────▶ 直接実行
│   │
│   └─ High-risk ────▶ 🗳️ Quorum Consensus #2 (Action Review)
│                        write_file, run_command 実行前にレビュー
└───────────────────┘
    │
    ▼
┌───────────────────────────┐
│ 🗳️ Quorum Consensus #3   │  ← オプションの最終レビュー
│  Final Review             │     (require_final_review: true)
└───────────────────────────┘
```

### Quorum Consensus / 合意形成

Quorum Consensus は複数モデルの投票によって安全性を確保します：

1. **Plan Review（必須）**: 設定された全 review_models が提案された計画をレビュー
2. **Action Review（高リスク操作）**: `write_file` と `run_command` の実行前にレビュー
3. **Final Review（オプション）**: 実行結果全体をレビュー

承認には過半数（または設定された QuorumRule）の投票が必要。却下された計画/アクションには集約されたフィードバックが含まれます。

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

## Tool Provider System / ツールプロバイダーシステム

> 詳細は [features/tool-system.md](./features/tool-system.md) を参照してください。

ツールプロバイダーシステムは、**プラグインベースのオーケストレーション**アーキテクチャを採用しています。
Quorum はツールの呼び出し・連携に専念し、実際のツール実装は外部プロバイダーに委譲します。

### Design Philosophy / 設計思想

| 原則 | 説明 |
|------|------|
| **オーケストレーション専念** | Quorum はツールの呼び出し・連携に注力、実装は外部に委譲 |
| **外部ツール追従** | CLI ツール（rg, gh, fd 等）や MCP サーバーが進化しても自動追従 |
| **ユーザー選択可能** | 設定ファイルでツールプロバイダーを切り替え |
| **プラグイン拡張** | コード変更なしで新しいツールを追加可能 |
| **標準ツールがデフォルト** | grep, find, cat など標準ツールをデフォルトに（どこでも動く） |
| **推奨ツール提案** | 高速ツール（rg, fd, bat）検知時はユーザーに切り替えを提案 |

### Architecture / アーキテクチャ

```
┌─────────────────────────────────────────────────────────────┐
│                     ToolRegistry                            │
│  (プロバイダーを集約、優先度でルーティング)                 │
└─────────────────────────────────────────────────────────────┘
          │              │              │              │
          ▼              ▼              ▼              ▼
   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
   │ Builtin  │   │   CLI    │   │   MCP    │   │  Script  │
   │ Provider │   │ Provider │   │ Provider │   │ Provider │
   └──────────┘   └──────────┘   └──────────┘   └──────────┘
   最小限の        rg, fd, gh     MCP サーバー    ユーザー
   フォールバック   等をラップ    を統合         スクリプト
   (優先度: -100)  (優先度: 50)  (優先度: 100)  (優先度: 75)
```

### Provider Types / プロバイダーの種類

| Provider | Priority | Description | Use Case |
|----------|----------|-------------|----------|
| **MCP** | 100 | MCP サーバー経由のツール | 外部サーバーとの連携、豊富な機能 |
| **Script** | 75 | ユーザー定義スクリプト | カスタム処理、プロジェクト固有ツール |
| **CLI** | 50 | システムCLIツールのラッパー | grep/rg, find/fd, cat/bat |
| **Builtin** | -100 | 最小限の組み込みツール | フォールバック、常に利用可能 |

優先度が高いプロバイダーが同じ名前のツールを提供している場合、そちらが優先されます。

### ToolProvider Trait

```rust
#[async_trait]
pub trait ToolProvider: Send + Sync {
    /// 一意な識別子 (e.g., "builtin", "cli", "mcp:filesystem")
    fn id(&self) -> &str;

    /// 表示名
    fn display_name(&self) -> &str;

    /// 優先度 (高い方が優先)
    fn priority(&self) -> i32 { 0 }

    /// プロバイダーが利用可能か確認
    async fn is_available(&self) -> bool;

    /// 利用可能なツールを検出
    async fn discover_tools(&self) -> Result<Vec<ToolDefinition>, ProviderError>;

    /// ツール実行
    async fn execute(&self, call: &ToolCall) -> ToolResult;
}
```

### CLI Tool Discovery / CLI ツール検知

CLI プロバイダーは標準ツールをデフォルトとしつつ、高速な代替ツールを検知して提案します。

#### Tool Mapping / ツールマッピング

| Tool | Standard (Default) | Enhanced (Recommended) | Improvement |
|------|-------------------|------------------------|-------------|
| `grep_search` | `grep` | `rg` (ripgrep) | ~10x faster, .gitignore support |
| `glob_search` | `find` | `fd` | ~5x faster, simpler syntax |
| `read_file` | `cat` | `bat` | Syntax highlighting |

#### Discovery Flow / 検知フロー

```
$ quorum init
📦 Tool configuration...

Default tools (always available):
  ✓ grep  → file content search
  ✓ find  → file pattern search

🔍 Enhanced tools detected on your system:
  • rg (ripgrep) - 10x faster than grep
  • fd           - 5x faster than find

Would you like to use these enhanced tools? [Y/n]: y

✨ Configuration updated!
```

### Configuration / 設定

`quorum.toml` でプロバイダーとツールを設定できます：

```toml
[tools]
providers = ["cli", "builtin"]  # 有効化するプロバイダー
suggest_enhanced_tools = true   # 推奨ツール検知時に提案するか

[tools.builtin]
enabled = true

[tools.cli]
enabled = true

# ツールのエイリアス設定（標準ツールがデフォルト）
[tools.cli.aliases]
grep_search = "grep"    # デフォルト: grep, 推奨: rg
glob_search = "find"    # デフォルト: find, 推奨: fd

# MCP サーバー設定
[tools.mcp]
enabled = true

[[tools.mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@anthropic/mcp-server-filesystem", "/workspace"]
```

### ToolRegistry / ツールレジストリ

`ToolRegistry` は複数のプロバイダーを集約し、`ToolExecutorPort` を実装します：

```rust
// レジストリの初期化
let mut registry = ToolRegistry::new()
    .register(CliToolProvider::new())      // priority: 50
    .register(BuiltinProvider::new());     // priority: -100

// ツール検出（優先度順に処理）
registry.discover().await?;

// ツール実行（適切なプロバイダーにルーティング）
let call = ToolCall::new("grep_search").with_arg("pattern", "TODO");
let result = registry.execute(&call).await;
```

### Module Structure / モジュール構造

```
infrastructure/src/tools/
├── mod.rs              # 全体エクスポート
├── registry.rs         # ToolRegistry 実装
├── builtin/
│   ├── mod.rs
│   ├── provider.rs     # BuiltinProvider (priority: -100)
│   └── *.rs            # read_file, write_file, etc.
├── cli/
│   ├── mod.rs
│   ├── provider.rs     # CliToolProvider (priority: 50)
│   └── discovery.rs    # 推奨ツール検知 & 提案
├── mcp/                # (Future: MCP integration)
│   ├── mod.rs
│   ├── provider.rs     # McpToolProvider (priority: 100)
│   └── client.rs       # MCP クライアント
└── script/             # (Future: User scripts)
    └── provider.rs     # ScriptToolProvider (priority: 75)
```

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

新しい戦略の追加は 2 ステップで行います：

**Step 1**: `OrchestrationStrategy` enum にバリアントを追加（`domain/src/orchestration/strategy.rs`）：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrchestrationStrategy {
    Quorum(QuorumConfig),
    Debate(DebateConfig),
    NewStrategy(NewStrategyConfig),  // ← 新規バリアント追加
}
```

**Step 2**: `StrategyExecutor` trait を実装する実行者を追加：

```rust
// domain/src/orchestration/strategies/new_strategy.rs
pub struct NewStrategyExecutor { ... }

#[async_trait]
impl StrategyExecutor for NewStrategyExecutor {
    fn name(&self) -> &'static str { "new-strategy" }
    fn phases(&self) -> Vec<Phase> { /* ... */ }
    async fn execute<G: LlmGateway>(
        &self, question: &Question, models: &[Model],
        moderator: &Model, gateway: &G, notifier: &dyn ProgressNotifier,
    ) -> Result<QuorumResult, DomainError> {
        // Strategy-specific execution logic
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

ツールプロバイダーシステムでは、複数の方法でツールを追加できます：

#### Option 1: CLI ツールのラッピング（推奨）

既存の CLI ツールを Quorum で利用可能にする最も簡単な方法：

```toml
# quorum.toml
[tools.cli.aliases]
my_tool = "external-cli-command"
```

#### Option 2: BuiltinProvider への追加

`infrastructure/tools/builtin/` に新しいツールを追加：

```rust
// infrastructure/src/tools/builtin/my_tool.rs
pub fn execute_my_tool(call: &ToolCall) -> ToolResult {
    // Tool implementation
}

// infrastructure/src/tools/builtin/provider.rs の build_default_spec() に追加
ToolDefinition::new("my_tool", "Description", RiskLevel::Low)
    .with_parameter(ToolParameter::new("arg", "Description", true))
```

#### Option 3: 新しい ToolProvider の実装

完全なカスタムプロバイダーを作成：

```rust
// infrastructure/src/tools/custom/provider.rs
pub struct CustomToolProvider { /* ... */ }

#[async_trait]
impl ToolProvider for CustomToolProvider {
    fn id(&self) -> &str { "custom" }
    fn display_name(&self) -> &str { "Custom Tools" }
    fn priority(&self) -> i32 { 60 }  // CLI より高く、Script より低い

    async fn is_available(&self) -> bool { true }

    async fn discover_tools(&self) -> Result<Vec<ToolDefinition>, ProviderError> {
        Ok(vec![
            ToolDefinition::new("my_tool", "Description", RiskLevel::Low)
        ])
    }

    async fn execute(&self, call: &ToolCall) -> ToolResult {
        match call.tool_name.as_str() {
            "my_tool" => execute_my_tool(call),
            _ => ToolResult::failure(&call.tool_name, ToolError::not_found(&call.tool_name)),
        }
    }
}
```

レジストリへの登録：

```rust
// cli/src/main.rs
let mut registry = ToolRegistry::new()
    .register(CustomToolProvider::new())  // priority: 60
    .register(CliToolProvider::new())     // priority: 50
    .register(BuiltinProvider::new());    // priority: -100
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
