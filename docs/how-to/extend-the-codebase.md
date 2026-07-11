# How to Extend the Codebase / コードベースを拡張する

> Add new LLM providers, orchestration strategies, tools and interaction forms
>
> 新しい LLM プロバイダー、オーケストレーション戦略、ツール、インタラクション形式の追加手順

前提となる設計原則（依存性逆転、垂直ドメイン分割）は
[Design Philosophy](../explanation/design-philosophy.md) を参照してください。

---

## Adding a New LLM Provider / 新しいLLMプロバイダーの追加

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

## Adding a New Orchestration Strategy / 新しいオーケストレーション戦略の追加

**Step 1**: `OrchestrationStrategy` enum にバリアントを追加（`domain/src/orchestration/strategy.rs`）：

```rust
pub enum OrchestrationStrategy {
    Quorum(QuorumConfig),
    Debate(DebateConfig),
    NewStrategy(NewStrategyConfig),  // ← 新規バリアント追加
}
```

**Step 2**: `application/src/use_cases/run_quorum/` に `StrategyExecutor` trait
（`strategy_executor.rs`）を実装する executor を追加（例: `new_strategy.rs`）。
`QuorumStrategyExecutor`/`DebateStrategyExecutor` が参考実装。executor は domain の
`LlmGateway` ではなく application の `Arc<dyn LlmGateway>` ポートを使う（domain は
I/O ポートに依存できないため、trait 自体も application 層にある）。

**Step 3**: `run_quorum/mod.rs` の `RunQuorumUseCase::execute_with_progress` にある
`OrchestrationStrategy` の exhaustive match に新しいバリアントの腕を追加。
`if let`/`matches!` チェーンではなく match の網羅性チェックに乗せることで、
バリアント追加の対応漏れをコンパイルエラーで検知できる。

## Adding New Tools / 新しいツールの追加

### Option 1: カスタムツール（設定のみ、最も簡単）

`init.lua` で `quorum.tools.register()` を呼ぶだけです。
手順は [How to Add Custom Tools](./add-custom-tools.md) を参照。

### Option 2: BuiltinProvider への追加

`infrastructure/tools/builtin/` に新しいツール実装を追加し、`default_tool_spec()` に登録：

```rust
// infrastructure/src/tools/builtin/my_tool.rs
pub fn execute_my_tool(call: &ToolCall) -> ToolResult {
    // Tool implementation
}

// infrastructure/src/tools/builtin/provider.rs の build_default_spec() に追加
ToolDefinition::new("my_tool", "Description", RiskLevel::Low)
    .with_parameter(ToolParameter::new("arg", "Description", true))
```

### Option 3: 新しい ToolProvider の実装

`ToolProvider` trait を実装し、`ToolRegistry` に登録：

```rust
pub struct CustomToolProvider { /* ... */ }

#[async_trait]
impl ToolProvider for CustomToolProvider {
    fn id(&self) -> &str { "custom" }
    fn display_name(&self) -> &str { "Custom Tools" }
    fn priority(&self) -> i32 { 60 }

    async fn is_available(&self) -> bool { true }
    async fn discover_tools(&self) -> Result<Vec<ToolDefinition>, ProviderError> { /* ... */ }
    async fn execute(&self, call: &ToolCall) -> ToolResult { /* ... */ }
}

// cli/src/main.rs でレジストリに登録
let registry = ToolRegistry::new()
    .register(CustomToolProvider::new())  // priority: 60
    .register(CliToolProvider::new())     // priority: 50
    .register(BuiltinProvider::new());    // priority: -100
```

## Adding New Interaction Forms / 新しいインタラクション形式の追加

`domain/src/interaction/mod.rs` の `InteractionForm` にバリアントを追加し、
対応するユースケースとプレゼンテーションを実装。

## Adding New Context File Types / 新しいコンテキストファイル種別の追加

`domain/context/value_objects.rs` の `KnownContextFile` enum に新しいファイル種別を追加。

## Adding a New Lua API / 新しい Lua API の追加

`infrastructure/scripting/` に API モジュールを追加し、`LuaScriptingEngine::new()` で登録。

---

## Related / 関連

- [Design Philosophy](../explanation/design-philosophy.md) - 拡張性を支える設計原則
- [Architecture Reference](../reference/architecture.md) - レイヤー構造の全体像
- [Tool System Reference](../reference/tool-system.md) - `ToolProvider` trait とレジストリ
- [Scripting Reference](../reference/scripting.md) - Lua API のアーキテクチャ
