# Orchestration Axes / オーケストレーションの 3 直交軸

> Why orchestration is three independent axes instead of one mode enum
>
> オーケストレーション設定が単一のモード enum ではなく 3 つの独立した軸である理由

---

## Overview / 概要

エージェントのオーケストレーション設定は、3 つの独立した軸で構成されています（`SessionMode` に集約）：

| 軸 | 型 | 役割 |
|----|------|------|
| **ConsensusLevel** | Enum (`Solo`, `Ensemble`) | 参加モデル数を制御 |
| **PhaseScope** | Enum (`Full`, `Fast`, `PlanOnly`) | 実行フェーズの範囲を制御 |
| **OrchestrationStrategy** | Enum (`Quorum(QuorumConfig)`, `Debate(DebateConfig)`) | 議論の進め方を選択 |

当初は Agent / Quorum / Fast / Debate / Plan という 5 つのモードを持つ単一 enum として提案されましたが、
議論の結果「体制（何モデルか）」「範囲（どこまで実行するか）」「戦略（どう議論するか）」という
**独立した概念が 1 つの enum に混在している** ことが判明し、直交軸へ再設計されました。
経緯は [ADR 0002: Three Orthogonal Axes](./design-decisions/0002-three-orthogonal-axes.md) を参照してください。

**なぜ直交軸か？** 単一 enum ではモードの組み合わせが増えるたびにバリアントが爆発します
（例: `EnsembleFast` の次は `EnsemblePlanOnly` …）。3 つの独立した軸に分解することで：

- **組み合わせの自由度** — N × M × K 通りの構成を少ないバリアントで表現
- **拡張容易性** — 新しい PhaseScope や Strategy を追加しても他の軸に影響しない
- **設定の明確性** — 各軸が「何を制御するか」が一目瞭然

`OrchestrationStrategy` はバリアントごとに設定を保持する **enum** です。
一方、`StrategyExecutor` は戦略の実行ロジックを定義する **trait** です。
enum が「何を使うか」を、trait が「どう実行するか」を担います。

---

## The Three Axes / 3 つの軸

### Consensus Level / 合意レベル

| Level | Commands | Description |
|-------|----------|-------------|
| **Solo** (default) | `/solo`, `/mode solo` | 単一モデルによる自律タスク実行（Plan → Review → Execute） |
| **Ensemble** | `/ens`, `/mode ensemble` | マルチモデル計画生成 + 投票 |

定義ファイル: `domain/src/orchestration/mode.rs`（`ConsensusLevel` enum）

### Phase Scope / フェーズスコープ

合意レベルとは直交するオプションで、実行範囲を制御します。

| Scope | Commands | Description |
|-------|----------|-------------|
| **Full** (default) | `/scope full` | 全フェーズ実行（レビュー含む） |
| **Fast** | `/fast`, `/scope fast` | レビューフェーズをスキップ（高速実行） |
| **PlanOnly** | `/scope plan-only` | 計画のみ生成、実行は行わない |

定義ファイル: `domain/src/orchestration/scope.rs`（`PhaseScope` enum）

### Orchestration Strategy / オーケストレーション戦略

議論の進め方を選択します。

| Strategy | Commands | Description |
|----------|----------|-------------|
| **Quorum** (default) | `/strategy quorum` | 対等な議論 → レビュー → 統合 |
| **Debate** | `/strategy debate` | 対立的議論 → 合意形成 |

定義ファイル: `domain/src/orchestration/strategy.rs`（`OrchestrationStrategy` enum）

---

## Combination Validation / 組み合わせバリデーション

3 軸の組み合わせのうち、無効・未サポートなものは起動時にバリデーションされます。
`SessionMode::validate_combination()` が `Vec<ConfigIssue>` を返し、CLI 層で Warning 表示 or Error 中断します。

| ConsensusLevel | PhaseScope | Strategy | Severity | Code | 理由 |
|---|---|---|---|---|---|
| Solo | * | Debate | **Error** | `SoloWithDebate` | ソロでは議論不可能（1モデルで対立的議論は成立しない） |
| Ensemble | * | Debate | Warning | `DebateNotImplemented` | StrategyExecutor が未実装 |
| Ensemble | Fast | * | Warning | `EnsembleWithFast` | レビュースキップにより Ensemble の価値が減少 |

- Solo + Debate は **Error**（実行不可）として即座に `bail!` します
- Ensemble + Debate は Warning のみ（将来の実装に備えて設定自体は受け付ける）
- Ensemble + Fast は Warning（動作はするが、マルチモデル合議のメリットが薄れる）
- Solo + Debate の場合は `DebateNotImplemented` Warning は省略されます（Error が優先）

定義ファイル: `domain/src/agent/validation.rs`（`Severity`, `ConfigIssueCode`, `ConfigIssue`）、
`domain/src/orchestration/session_mode.rs`

---

## Runtime Switching / 実行中の切り替え

3 軸は `SessionMode` として runtime-mutable であり、REPL/TUI のモードコマンド
（`/solo` `/ens` `/fast` `/scope` `/strategy`）または Lua
（`quorum.config.set("agent.consensus_level", ...)` 等）で切り替えられます。

---

## Related / 関連

- [ADR 0002: Three Orthogonal Axes](./design-decisions/0002-three-orthogonal-axes.md) - 5 モード案から再設計された経緯
- [Agent Behavior](./agent-behavior.md) - PhaseScope が制御するライフサイクル
- [Configuration Reference](../reference/configuration.md) - `agent.*` キー
- [CLI Reference](../reference/cli.md) - モード切り替えコマンド

<!-- LLM Context: オーケストレーションは 3 直交軸: ConsensusLevel (Solo/Ensemble) × PhaseScope (Full/Fast/PlanOnly) × OrchestrationStrategy (Quorum/Debate)。SessionMode (domain/src/orchestration/session_mode.rs) に集約、runtime-mutable。組み合わせバリデーション SessionMode::validate_combination(): Solo+Debate=Error(SoloWithDebate), Ensemble+Debate=Warning(DebateNotImplemented), Ensemble+Fast=Warning(EnsembleWithFast)。旧 5 モード enum (Agent/Quorum/Fast/Debate/Plan) は Discussion #38 で概念混在と判明し再設計 (commits 4ca46d7, 53cd28a)。 -->
