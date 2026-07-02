# Tool System / ツールシステム

> Plugin-based tool architecture with risk classification and provider routing
>
> リスク分類とプロバイダールーティングを備えたプラグインベースのツールアーキテクチャ

---

## Overview / 概要

ツールシステムは、エージェントがファイル操作やコマンド実行などのアクションを行うための仕組みです。
**プラグインベースのオーケストレーション** アーキテクチャを採用しており、
Quorum はツールの呼び出し・連携に専念し、実際のツール実装は外部プロバイダーに委譲します。

ツールはリスクレベルで分類され、高リスクツール（書き込み・コマンド実行）は
Quorum Consensus によるレビューを経てから実行されます。

---

## How It Works / 仕組み

### Built-in Tools / 組み込みツール

| Tool | Risk Level | Description | Parameters |
|------|-----------|-------------|------------|
| `read_file` | Low | ファイル内容の読み取り | `path` (必須), `offset`, `limit` |
| `write_file` | **High** | ファイルの書き込み/作成 | `path` (必須), `content` (必須), `create_dirs` |
| `run_command` | **High** | シェルコマンド実行 | `command` (必須), `working_dir`, `timeout_secs` |
| `glob_search` | Low | パターンによるファイル検索 | `pattern` (必須), `base_dir`, `max_results` |
| `grep_search` | Low | ファイル内容の正規表現検索 | `pattern` (必須), `path` (必須), `file_pattern`, `context_lines`, `case_insensitive` |
| `web_fetch` | Low | Web ページ取得・テキスト抽出 | `url` (必須), `max_length` |
| `web_search` | Low | DuckDuckGo で Web 検索 | `query` (必須) |

> **Note**: `web_fetch` と `web_search` は `web-tools` feature flag が有効な場合のみ利用可能です。

### Risk Classification / リスク分類

| Risk Level | Behavior | Examples |
|------------|----------|----------|
| **Low** | 直接実行（レビューなし） | `read_file`, `glob_search`, `grep_search`, `web_fetch`, `web_search` |
| **High** | Quorum Consensus レビュー後に実行 | `write_file`, `run_command` |

高リスクツールの特性:
- ファイルシステムを変更する可能性がある
- 外部コマンドを実行する
- 元に戻すのが困難な操作

### Provider Architecture / プロバイダーアーキテクチャ

```
┌─────────────────────────────────────────────────────────────┐
│                     ToolRegistry                            │
│  (プロバイダーを集約、優先度でルーティング)                 │
└─────────────────────────────────────────────────────────────┘
          │              │              │              │              │
          ▼              ▼              ▼              ▼              ▼
   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
   │ Builtin  │   │   CLI    │   │  Custom  │   │  Script  │   │   MCP    │
   │ Provider │   │ Provider │   │ Provider │   │ Provider │   │ Provider │
   └──────────┘   └──────────┘   └──────────┘   └──────────┘   └──────────┘
   最小限の        rg, fd, gh     init.lua        ユーザー        MCP サーバー
   フォールバック   等をラップ    で定義した       スクリプト      を統合
   (優先度: -100)  (優先度: 50)  カスタムツール   (優先度: 75)   (優先度: 100)
                                 (優先度: 75)
```

優先度が高いプロバイダーが同じ名前のツールを提供している場合、そちらが優先されます。
例えば、CLI Provider の `grep_search`（rg ベース）が Builtin Provider の `grep_search` より優先されます。

### Provider Types / プロバイダーの種類

| Provider | Priority | Status | Description |
|----------|----------|--------|-------------|
| **Builtin** | -100 | 実装済み | 最小限の組み込みツール（フォールバック） |
| **CLI** | 50 | 実装済み | システム CLI ツールのラッパー（grep/rg, find/fd） |
| **Custom** | 75 | 実装済み | init.lua (`quorum.tools.register`) で定義するユーザーカスタムツール |
| **Script** | 75 | 将来 | ユーザー定義スクリプト |
| **MCP** | 100 | 将来 | MCP サーバー経由のツール |

### Custom Tools / カスタムツール

カスタムツールは、CLI コマンドを `init.lua` の `quorum.tools.register()` で定義するだけで
ファーストクラスのツールとして登録できる仕組みです。

- **コマンドテンプレート**: `{param_name}` プレースホルダーでパラメータを埋め込み
- **シェルエスケープ**: パラメータ値は自動的にシェルエスケープされ、インジェクション攻撃を防止
- **安全デフォルト**: リスクレベルは未指定の場合 `high`（safe by default）
- **優先度 75**: CLI プロバイダー（50）より高く、MCP（100）より低い位置

登録手順は [How to Add Custom Tools](../how-to/add-custom-tools.md) を参照してください。

### Web Tools / Web ツール (`web-tools` feature)

Web ツールは `web-tools` feature flag で有効化されます（CLI crate ではデフォルト有効）。

**`web_fetch`**: URL からページを取得し、HTML をテキストに変換して返します。
- `script`, `style`, `noscript`, `svg` タグは除外
- デフォルト最大 50KB のテキストを返却（`max_length` パラメータで変更可能）
- レスポンスの最大サイズは 5MB

**`web_search`**: DuckDuckGo Instant Answer API を使用した Web 検索。API Key 不要。
- 即座に使える検索結果（Abstract, Answer, Definition, Related Topics）を返却
- より詳細な情報が必要な場合は `web_fetch` で直接 URL にアクセス

ビルド方法は [How to Add Custom Tools](../how-to/add-custom-tools.md#enable-web-tools--web-ツールを有効化する) を参照してください。

### CLI Tool Enhancement / CLI ツールの強化

CLI プロバイダーは標準ツールをデフォルトとしつつ、高速な代替ツールを検知して提案します。

| Tool | Standard (Default) | Enhanced (Recommended) | Improvement |
|------|-------------------|------------------------|-------------|
| `grep_search` | `grep` | `rg` (ripgrep) | ~10x faster, .gitignore support |
| `glob_search` | `find` | `fd` | ~5x faster, simpler syntax |
| `read_file` | `cat` | `bat` | Syntax highlighting |

---

## Configuration / 設定

カスタムツールの登録は `quorum.tools.register()`（[Configuration Reference](./configuration.md) 参照）。
CLI エイリアス（`grep_search` → `rg` 等）は現在 infrastructure 内蔵のデフォルトで管理され、
強化ツールは自動検出されます。MCP サーバー統合は将来の構想です。

## Architecture / アーキテクチャ

### Key Files / 主要ファイル

| File | Description |
|------|-------------|
| `domain/src/tool/entities.rs` | `ToolDefinition`, `ToolCall`, `ToolSpec`, `RiskLevel` |
| `domain/src/tool/value_objects.rs` | `ToolResult`, `ToolError` |
| `domain/src/tool/traits.rs` | `ToolValidator` trait |
| `application/src/ports/tool_executor.rs` | `ToolExecutorPort` trait |
| `infrastructure/src/tools/mod.rs` | `default_tool_spec()`, `read_only_spec()` |
| `infrastructure/src/tools/registry.rs` | `ToolRegistry` 実装 |
| `infrastructure/src/tools/builtin/provider.rs` | `BuiltinProvider` (priority: -100) |
| `infrastructure/src/tools/cli/provider.rs` | `CliToolProvider` (priority: 50) |
| `infrastructure/src/tools/cli/discovery.rs` | 強化ツール検知ロジック |
| `infrastructure/src/tools/custom_provider.rs` | `CustomToolProvider` (priority: 75) |
| `infrastructure/src/scripting/tools_api.rs` | `quorum.tools.register`（Lua カスタムツール登録 API） |
| `infrastructure/src/tools/cli/config.rs` | `CliToolsConfig`（エイリアス・強化ツール定義） |
| `infrastructure/src/tools/file.rs` | `read_file`, `write_file` 実装 |
| `infrastructure/src/tools/command.rs` | `run_command` 実装 |
| `infrastructure/src/tools/search.rs` | `glob_search`, `grep_search` 実装 |
| `infrastructure/src/tools/web/mod.rs` | Web ツールモジュール (`web-tools` feature) |
| `infrastructure/src/tools/web/fetch.rs` | `web_fetch` 実装 |
| `infrastructure/src/tools/web/search.rs` | `web_search` 実装 |

### Data Flow / データフロー

```
Agent (RunAgentUseCase)
    │
    ▼
ToolExecutorPort.execute(ToolCall)
    │
    ▼
ToolRegistry
    │
    ├── 1. ToolCall のバリデーション (ToolValidator)
    │
    ├── 2. プロバイダー選択（優先度順）
    │   ├── Custom Provider (75) ← init.lua 定義のカスタムツール
    │   ├── CLI Provider (50)    ← 同名ツールがあればこちら優先
    │   └── Builtin (-100)       ← フォールバック
    │
    └── 3. ToolResult を返却
```

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

### ToolExecutorPort

```rust
#[async_trait]
pub trait ToolExecutorPort: Send + Sync {
    fn tool_spec(&self) -> &ToolSpec;
    fn has_tool(&self, name: &str) -> bool;
    fn get_tool(&self, name: &str) -> Option<&ToolDefinition>;
    fn available_tools(&self) -> Vec<&str>;
    async fn execute(&self, call: &ToolCall) -> ToolResult;
    fn execute_sync(&self, call: &ToolCall) -> ToolResult;
}
```

### Adding New Tools / ツール追加ガイド

- 設定のみで追加: [How to Add Custom Tools](../how-to/add-custom-tools.md)
- Rust での実装（BuiltinProvider / 新規 ToolProvider）: [How to Extend the Codebase](../how-to/extend-the-codebase.md)

---

## Related Features / 関連機能

- [How to Add Custom Tools](../how-to/add-custom-tools.md) - カスタムツールの登録手順
- [Agent System Reference](./agent-system.md) - ツールを使って自律タスク実行
- [Agent Behavior](../explanation/agent-behavior.md) - 高リスクツールの Consensus レビュー
- [Configuration Reference](./configuration.md) - `quorum.tools.register` API

<!-- LLM Context: Tool System はプラグインベースのアーキテクチャ。5つの組み込みツール（read_file, write_file, run_command, glob_search, grep_search）+ 2つの Web ツール（web_fetch, web_search、web-tools feature flag）。RiskLevel で Low/High に分類。ToolRegistry が優先度ベースでプロバイダーをルーティング（Builtin:-100, CLI:50, Custom:75, MCP:100）。Custom Provider（infrastructure/src/tools/custom_provider.rs）は init.lua の quorum.tools.register でユーザー定義の CLI コマンドをファーストクラスのツールとして登録可能。コマンドテンプレートは {param_name} プレースホルダーを使い、パラメータはシェルエスケープされる。リスクレベルはデフォルト high（safe by default）。ToolResultMetadata フィールド: duration_ms, bytes, path, exit_code, match_count（domain/src/tool/value_objects.rs）。ToolSchemaPort（application/src/ports/tool_schema.rs）が JSON Schema 変換を担当。主要ファイルは domain/src/tool/（entities.rs, value_objects.rs, traits.rs）、application/src/ports/tool_executor.rs、application/src/ports/tool_schema.rs、infrastructure/src/tools/（registry.rs, custom_provider.rs, schema.rs）、infrastructure/src/scripting/tools_api.rs。 -->
