# CLI & Configuration / CLI とコンフィグレーション

> REPL commands, configuration options, and context management
>
> REPL コマンド、設定オプション、コンテキスト管理

---

## Overview / 概要

copilot-quorum は CLI ツールとして動作し、ワンショット実行と対話的な REPL の 2 つのモードを提供します。
設定は TOML ファイル（`quorum.toml`）で管理され、プロジェクトレベルとグローバルレベルの
2 段階で設定できます。`/init` コマンドによるプロジェクトコンテキストの自動生成も可能です。

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
| `/mode <mode>` | | モードを変更 (solo, ensemble, fast, debate, plan) |
| `/solo` | | Solo モードに切り替え（単一モデル、高速実行） |
| `/ens` | `/ensemble` | Ensemble モードに切り替え（マルチモデル計画生成） |
| `/discuss <question>` | `/council` | Quorum Discussion を実行（複数モデルに相談） |
| `/init [--force]` | | プロジェクトコンテキストを初期化 |
| `/config` | | 現在の設定を表示 |
| `/clear` | | 会話履歴をクリア |
| `/verbose` | | Verbose モードの状態を表示 |
| `/quit` | `/exit`, `/q` | 終了 |

### Prompt Display / プロンプト表示

REPL のプロンプトは現在のモードに応じて色が変わります:

| Mode | Prompt | Color |
|------|--------|-------|
| Solo | `solo>` | Green |
| Ensemble | `ensemble>` | Magenta |
| Fast | `fast>` | Yellow |
| Debate | `debate>` | Blue |
| Plan | `plan>` | Cyan |

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
# Quorum Settings / 合議設定
# ============================================================
[quorum]
rule = "majority"        # 合意ルール: "majority", "unanimous", "atleast:N", "N%"
min_models = 2           # 有効な合意に必要な最小モデル数

[quorum.discussion]
models = ["claude-sonnet-4.5", "gpt-5.2-codex", "gemini-3-pro-preview"]
moderator = "claude-opus-4.5"
enable_peer_review = true   # Phase 2 (Peer Review) の有効化

# ============================================================
# Legacy Council Settings（後方互換、quorum.discussion に移行推奨）
# ============================================================
[council]
models = ["claude-sonnet-4.5", "gpt-5.2-codex"]
moderator = "claude-sonnet-4.5"

# ============================================================
# Agent Settings / エージェント設定
# ============================================================
[agent]
planning_mode = "single"                  # "single" (Solo) or "ensemble"
hil_mode = "interactive"                  # "interactive", "auto_reject", "auto_approve"
max_plan_revisions = 3                    # 人間介入までの最大計画修正回数
exploration_model = "claude-haiku-4.5"    # コンテキスト収集用（高速・低コスト）
decision_model = "claude-sonnet-4.5"      # 計画作成・高リスクツール判断用
review_models = ["claude-sonnet-4.5", "gpt-5.2-codex"]  # Quorum レビュー用

# ============================================================
# Behavior Settings / 動作設定
# ============================================================
[behavior]
enable_review = true       # ピアレビューをデフォルトで有効化
timeout_seconds = null     # タイムアウト秒数（null = 無制限）

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
[tools]
providers = ["cli", "builtin"]    # 有効化するプロバイダー
suggest_enhanced_tools = true     # 強化ツール検知時の提案

[tools.builtin]
enabled = true

[tools.cli]
enabled = true

[tools.cli.aliases]
grep_search = "grep"    # "grep" or "rg" (ripgrep)
glob_search = "find"    # "find" or "fd"

[tools.mcp]
enabled = false

[[tools.mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@anthropic/mcp-server-filesystem", "/workspace"]
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

- [Quorum Discussion & Consensus](./quorum.md) - `/discuss` コマンドで実行
- [Agent System](./agent-system.md) - エージェント設定の詳細
- [Ensemble Mode](./ensemble-mode.md) - `/ens` コマンドと Ensemble 設定
- [Tool System](./tool-system.md) - ツール設定の詳細

<!-- LLM Context: CLI & Configuration は copilot-quorum のユーザーインターフェース。REPL コマンド（/help, /solo, /ens, /discuss, /init, /config, /clear, /quit 等）と quorum.toml による設定管理。設定優先順位は CLI > project > global > defaults。主要ファイルは presentation/src/agent/repl.rs と infrastructure/src/config/。 -->
