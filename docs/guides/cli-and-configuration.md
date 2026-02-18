# CLI & Configuration / CLI とコンフィグレーション

> REPL commands, configuration options, and context management
>
> REPL コマンド、設定オプション、コンテキスト管理

---

## Overview / 概要

copilot-quorum は CLI ツールとして動作し、ワンショット実行と対話的な REPL の 2 つのモードを提供します。
設定は TOML ファイル（`quorum.toml`）で管理され、プロジェクトレベルとグローバルレベルの
2 段階で設定できます。`/init` コマンドによるプロジェクトコンテキストの自動生成も可能です。

エージェントの動作は 3 つの直交する軸（`ConsensusLevel`, `PhaseScope`, `OrchestrationStrategy`）で構成され、
`SessionMode` に集約されて TUI から runtime で切り替え可能です。モデル設定は `[models]` セクションで一元管理されます。
設定全体は `QuorumConfig`（4型コンテナ）で管理されます。

---

## Quick Start / クイックスタート

```bash
# ワンショット実行
copilot-quorum "What's the best way to handle errors in Rust?"

# 対話モード (REPL)
copilot-quorum --chat

# モデル指定
copilot-quorum -m claude-sonnet-4.5 -m gpt-5.2-codex "Compare patterns"

# Ensemble モード
copilot-quorum --ensemble "Design the auth system"

# プロジェクトコンテキスト生成
copilot-quorum /init
```

---

## How It Works / 仕組み

### CLI Options / CLI オプション

| Option | Short | Description |
|--------|-------|-------------|
| `--model <MODEL>` | `-m` | モデル指定（複数可） |
| `--moderator <MODEL>` | | シンセシス用モデル |
| `--no-review` | | ピアレビューをスキップ |
| `--output <FORMAT>` | `-o` | 出力形式 (`full` / `synthesis` / `json`) |
| `--verbose` | `-v` | 詳細ログを表示 |
| `--quiet` | `-q` | プログレス表示を抑制 |
| `--chat` | | 対話モード (REPL) で起動 |
| `--ensemble` | | Ensemble モードで実行 |
| `--config <PATH>` | | 設定ファイルのパスを指定 |

### REPL Commands / REPL コマンド

REPL（対話モード）で使用できるスラッシュコマンド一覧:

| Command | Aliases | Description |
|---------|---------|-------------|
| `/help` | `/h`, `/?` | ヘルプを表示 |
| `/mode <mode>` | | 合意レベルを変更 (solo, ensemble) |
| `/solo` | | Solo モードに切り替え（単一モデル、高速実行） |
| `/ens` | `/ensemble` | Ensemble モードに切り替え（マルチモデル計画生成） |
| `/fast` | | PhaseScope を Fast に切り替え（レビュースキップ） |
| `/scope <scope>` | | フェーズスコープを変更 (full, fast, plan-only) |
| `/strategy <strategy>` | | 戦略を変更 (quorum, debate) |
| `/ask` | | (再設計予定 — Issue #119) |
| `/discuss` | | (再設計予定 — Issue #119、`/council <question>` を使用) |
| `/council <question>` | | Quorum Discussion を実行（複数モデルに相談） |
| `/init [--force]` | | プロジェクトコンテキストを初期化 |
| `/config` | | 現在の設定を表示 |
| `/clear` | | 会話履歴をクリア |
| `/verbose` | | Verbose モードの状態を表示 |
| `/quit` | `/exit`, `/q` | 終了 |

### Consensus Level / 合意レベル

REPL では 2 つの合意レベルが利用可能です。`/mode <level>` または各モードのエイリアスコマンドで切り替えられます。

| Level | Aliases | Description |
|-------|---------|-------------|
| **Solo** (default) | `/solo`, `/mode solo` | 単一モデルによる自律タスク実行（Plan → Review → Execute） |
| **Ensemble** | `/ens`, `/mode ensemble` | マルチモデル Quorum Discussion（複数モデルで計画生成 + 投票） |

定義ファイル: `domain/src/orchestration/mode.rs`（`ConsensusLevel` enum）

### Phase Scope / フェーズスコープ

合意レベルとは直交するオプションで、実行範囲を制御します。

| Scope | Command | Description |
|-------|---------|-------------|
| **Full** (default) | `/scope full` | 全フェーズ実行（レビュー含む） |
| **Fast** | `/fast`, `/scope fast` | レビューフェーズをスキップ（高速実行） |
| **PlanOnly** | `/scope plan-only` | 計画のみ生成、実行は行わない |

定義ファイル: `domain/src/orchestration/scope.rs`（`PhaseScope` enum）

### Orchestration Strategy / オーケストレーション戦略

議論の進め方を選択します。

| Strategy | Command | Description |
|----------|---------|-------------|
| **Quorum** (default) | `/strategy quorum` | 対等な議論 → レビュー → 統合 |
| **Debate** | `/strategy debate` | 対立的議論 → 合意形成 |

定義ファイル: `domain/src/orchestration/strategy.rs`（`OrchestrationStrategy` enum）

### Combination Validation / 組み合わせバリデーション

上記 3 軸の一部の組み合わせは無効・未サポートです。起動時に自動検出され、Warning または Error が表示されます。

| 組み合わせ | Severity | 理由 |
|------------|----------|------|
| Solo + Debate | **Error** | 1モデルで対立的議論は不可能 |
| Ensemble + Debate | Warning | StrategyExecutor 未実装 |
| Ensemble + Fast | Warning | レビュースキップで Ensemble の価値が減少 |

Error の場合は実行が中断されます。詳細は [Agent System](../systems/agent-system.md) を参照。

バリデーション: `SessionMode::validate_combination()` → `Vec<ConfigIssue>`
定義ファイル: `domain/src/agent/validation.rs`（`Severity`, `ConfigIssueCode`, `ConfigIssue`）、`domain/src/orchestration/session_mode.rs`

### Prompt Display / プロンプト表示

REPL のプロンプトは現在の ConsensusLevel に応じて変わります:

| ConsensusLevel | Prompt |
|----------------|--------|
| Solo | `solo>` |
| Ensemble | `ens>` |

### Context Management / コンテキスト管理

`/init` コマンドはプロジェクトの情報を収集し、`.quorum/context.md` を生成します。
このファイルはエージェントがプロジェクトを理解するためのコンテキストとして使用されます。

読み込み対象ファイル（優先度順）:

| Priority | File | Description |
|----------|------|-------------|
| 1 | `.quorum/context.md` | 生成された Quorum コンテキスト |
| 2 | `CLAUDE.md` | ローカルプロジェクト指示 |
| 3 | `~/.claude/CLAUDE.md` | グローバル Claude 設定 |
| 4 | `README.md` | プロジェクト README |
| 5 | `docs/**/*.md` | docs ディレクトリ内の全 Markdown |
| 6 | `Cargo.toml` / `package.json` / `pyproject.toml` | ビルド設定 |

---

## Configuration / 設定

### Configuration File Priority / 設定ファイルの優先順位

| Priority | Location | Description |
|----------|----------|-------------|
| 1 | `--config <path>` | CLI で明示指定 |
| 2 | `./quorum.toml` or `./.quorum.toml` | プロジェクトレベル |
| 3 | `$XDG_CONFIG_HOME/copilot-quorum/config.toml` | XDG 設定 |
| 4 | `~/.config/copilot-quorum/config.toml` | グローバル（フォールバック） |
| 5 | Built-in defaults | デフォルト値 |

上位の設定が下位を上書きします。

### Full Configuration Reference / 全設定項目

```toml
# ============================================================
# Model Settings / モデル設定
# ============================================================
[models]
exploration = "gpt-5.2-codex"           # コンテキスト収集用（高速・低コスト）
decision = "claude-sonnet-4.5"          # 計画作成・高リスクツール判断用
review = ["claude-opus-4.5", "gpt-5.2-codex", "gemini-3-pro-preview"]  # Quorum レビュー用

# ============================================================
# Quorum Settings / 合議設定
# ============================================================
[quorum]
rule = "majority"        # 合意ルール: "majority", "unanimous", "atleast:N", "N%"
min_models = 2           # 有効な合意に必要な最小モデル数
moderator = "claude-opus-4.5"    # シンセシスモデル
enable_peer_review = true        # Phase 2 (Peer Review) の有効化

# ============================================================
# Agent Settings / エージェント設定
# ============================================================
[agent]
consensus_level = "solo"                  # "solo" or "ensemble"
phase_scope = "full"                      # "full", "fast", "plan-only"
strategy = "quorum"                       # "quorum" or "debate"
hil_mode = "interactive"                  # "interactive", "auto_reject", "auto_approve"
max_plan_revisions = 3                    # 人間介入までの最大計画修正回数

# ============================================================
# Output Settings / 出力設定
# ============================================================
[output]
format = "synthesis"       # "full", "synthesis", "json"
color = true               # カラー出力の有効化

# ============================================================
# REPL Settings / REPL 設定
# ============================================================
[repl]
show_progress = true       # プログレス表示の有効化
history_file = null        # 履歴ファイルのパス（null = デフォルト）

# ============================================================
# Tool Settings / ツール設定
# ============================================================
# [tools.custom.my_tool]
# description = "My custom tool"
# command = "echo {input}"
# risk_level = "high"
# [tools.custom.my_tool.parameters.input]
# type = "string"
# description = "Input text"
# required = true

# ============================================================
# TUI Settings / TUI 設定
# ============================================================
[tui.input]
submit_key = "enter"           # 送信キー
newline_key = "shift+enter"    # 改行挿入キー（マルチライン入力）
editor_key = "I"               # $EDITOR 起動キー（NORMAL モード）
editor_action = "return_to_insert"  # $EDITOR 保存後の動作: "return_to_insert" or "submit"
max_height = 10                # INSERT モード入力エリアの最大行数
dynamic_height = true          # 入力内容に応じた動的リサイズ
context_header = true          # $EDITOR 起動時のコンテキストヘッダー表示
```

---

## Architecture / アーキテクチャ

### Key Files / 主要ファイル

| File | Description |
|------|-------------|
| `presentation/src/cli/commands.rs` | CLAP CLI コマンド定義 |
| `presentation/src/agent/repl.rs` | REPL 実装（コマンド処理、プロンプト表示） |
| `infrastructure/src/config/file_config.rs` | TOML 設定構造定義 |
| `infrastructure/src/config/loader.rs` | 設定ローダー（優先順位処理） |
| `domain/src/config/` | `OutputFormat` など設定ドメイン型 |
| `domain/src/context/` | `ProjectContext`, `KnownContextFile` |
| `infrastructure/src/context/` | `LocalContextLoader` 実装 |

### Data Flow / データフロー

```
CLI Arguments / REPL Input
    │
    ├── Config Loading
    │   ├── --config flag → explicit path
    │   ├── ./quorum.toml → project config
    │   ├── XDG/home config → global config
    │   └── Built-in defaults
    │
    ├── Context Loading (/init)
    │   ├── .quorum/context.md
    │   ├── CLAUDE.md
    │   ├── README.md
    │   ├── docs/**/*.md
    │   └── Cargo.toml / package.json
    │
    └── Command Dispatch
        ├── One-shot → RunQuorumUseCase / RunAgentUseCase
        └── REPL → ChatRepl → Command loop
```

---

## Related Features / 関連機能

- [Quorum Discussion & Consensus](../concepts/quorum.md) - `/council` コマンドで実行
- [Agent System](../systems/agent-system.md) - エージェント設定の詳細
- [Ensemble Mode](../concepts/ensemble-mode.md) - `/ens` コマンドと Ensemble 設定
- [Tool System](../systems/tool-system.md) - ツール設定の詳細

<!-- LLM Context: CLI & Configuration は copilot-quorum のユーザーインターフェース。REPL コマンド（/help, /solo, /ens, /fast, /scope, /strategy, /council, /init, /config, /clear, /quit 等）と quorum.toml による設定管理。設定は4型に分割: SessionMode(domain, runtime-mutable: consensus_level/phase_scope/strategy)、ModelConfig(domain: exploration/decision/review)、AgentPolicy(domain: hil_mode等)、ExecutionParams(application: max_iterations等)。QuorumConfig(application)が4型コンテナとしてAgentControllerで使用。組み合わせバリデーション: SessionMode::validate_combination()。Solo+Debate=Error、Debate全般=Warning(未実装)、Ensemble+Fast=Warning。設定優先順位は CLI > project > global > defaults。[tui.input] セクションで TUI の入力設定を管理。主要ファイルは application/src/use_cases/agent_controller.rs と application/src/config/ と infrastructure/src/config/。 -->
