# Agent Behavior / エージェントの動作原理

> How the agent combines autonomous execution with quorum-based safety
>
> 自律実行と合議ベースの安全性をエージェントがどう両立しているか

---

## Overview / 概要

エージェントシステムは、copilot-quorum の合議（Quorum）コンセプトを自律的なタスク実行に拡張したものです。
ルーチンタスクは単一モデルで高速実行しつつ、重要な決定ポイントでは複数モデルによる合議を行うことで、
**効率性**と**安全性**を両立しています。

従来のエージェントシステムは単一モデルの判断に依存しますが、
ハルシネーション、盲点、過信といったリスクがあります。
copilot-quorum のエージェントは、**3 つの重要ポイント**で Quorum Consensus を挟むことで、
これらのリスクを構造的に軽減します。

実行手順は [How to Run Agent Tasks](../how-to/run-agent-tasks.md) を参照してください。

---

## Agent Lifecycle / エージェントライフサイクル

エージェントの実行フローは `PhaseScope` によって制御されます。
各フェーズの実行可否は以下の表の通りです：

| Phase                  | Full | Fast  | PlanOnly    |
|------------------------|------|-------|-------------|
| 1. Context Gathering   | yes  | yes   | yes         |
| 2. Planning            | yes  | yes   | yes         |
| 3. Plan Review (Quorum)| yes  | skip  | skip        |
| 3b. Execution Confirm  | yes  | skip  | skip        |
| 4. Executing           | yes  | yes   | skip+return |
|    - Action Review     | yes  | skip  | N/A         |
| 5. Final Review        | opt  | skip  | N/A         |

定義ファイル: `application/src/use_cases/run_agent/mod.rs`

```
User Request
    │
    ▼
┌───────────────────┐
│ Context Gathering │  ← プロジェクト情報収集 (glob, read_file)
└───────────────────┘    exploration_model を使用
    │                    ResourceReference 自動解決（GitHub Issue/PR）
    ▼
┌───────────────────┐
│     Planning      │  ← Solo: decision_model が計画作成
└───────────────────┘    Ensemble: review_models が並列生成 → 投票
    │
    ▼
╔═══════════════════════════╗
║  Quorum Consensus #1     ║  ← 全モデルが計画をレビュー（PhaseScope::Full のみ）
║  Plan Review              ║    QuorumRule に基づく投票で承認/却下
╚═══════════════════════════╝
    │
    ▼
┌─── Execution Confirm ────┐  ← PhaseScope::Full + Interactive のみ
│  "本当に実行しますか？"     │    HilMode に応じて自動/手動判断
└───────────────────────────┘
    │
    ▼
┌───────────────────┐
│  Task Execution   │   ← Native Tool Use マルチターンループ
│   ├─ Low-risk  ────▶ 直接実行（並列）
│   │
│   └─ High-risk ────▶ ╔═══════════════════════════╗
│                       ║  Quorum Consensus #2     ║
│                       ║  Action Review            ║
│                       ╚═══════════════════════════╝
└───────────────────┘
    │
    ▼
╔═══════════════════════════╗
║  Quorum Consensus #3     ║  ← オプションの最終レビュー
║  Final Review             ║    (require_final_review: true)
╚═══════════════════════════╝
```

## Quorum Review Points / 合議レビューポイント

| Review | Timing | Required | Purpose |
|--------|--------|----------|---------|
| **Plan Review** | 計画作成後 | PhaseScope::Full | 計画の安全性・正確性を検証 |
| **Execution Confirm** | 計画承認後 | PhaseScope::Full + Interactive | 実行前の最終確認 |
| **Action Review** | 高リスクツール実行前 | PhaseScope::Full | 書き込み・コマンド実行の安全性検証 |
| **Final Review** | 全タスク完了後 | オプション | 実行結果全体の品質検証 |

## Risk-Based Tool Classification / リスクベースのツール分類

「元に戻せるか」を基準に、ツールを 2 段階のリスクに分類しています。
読み取り専用ツールは即時・並列実行し、システムを変更するツールだけに合議コストを払う設計です。

| Tool | Risk Level | Quorum Review | Rationale |
|------|------------|---------------|-----------|
| `read_file` | Low | No | 読み取り専用 |
| `glob_search` | Low | No | 読み取り専用 |
| `grep_search` | Low | No | 読み取り専用 |
| `web_fetch` | Low | No | 読み取り専用（`web-tools` feature） |
| `web_search` | Low | No | 読み取り専用（`web-tools` feature） |
| `write_file` | **High** | **Yes** | ファイルシステムを変更 |
| `run_command` | **High** | **Yes** | 外部コマンド実行、元に戻すのが困難 |

カスタムツールのリスクレベルは登録時に指定でき、デフォルトは `"high"`（安全側）です。
詳細は [Tool System Reference](../reference/tool-system.md) を参照。

## Plan Review Details / 計画レビューの詳細

計画レビューは `PhaseScope::Full` で実行されます（Fast/PlanOnly ではスキップ）。

1. 全ての review_models に計画を並列送信（`JoinSet` で並行実行）
2. 各モデルが APPROVE / REJECT を投票
3. QuorumRule（デフォルト: 過半数）で判定
4. 却下時は全モデルのフィードバックを集約し、計画を修正 → 再投票

実装: `application/src/use_cases/run_agent/review.rs` — `review_plan()`, `query_model_for_review()`

## Action Review Details / アクションレビューの詳細

高リスクツール（`write_file`, `run_command`）の実行前に自動発動します。
`QuorumActionReviewer` が `ActionReviewer` trait を実装し、マルチモデル投票で判断します。

判断基準:
- 操作は必要か？
- 引数は正しいか？
- より安全な代替手段はないか？

却下されたアクションはスキップされます（エラーではない）。

実装: `application/src/use_cases/run_agent/review.rs` — `QuorumActionReviewer`

---

## Human-in-the-Loop (HiL) / 人間介入

Quorum が合意に至らない場合、エージェントは無限にリトライする代わりに、
ユーザーに判断を委ねることができます。

HiL には 2 つのゲートがあります：
1. **Plan Review HiL**: `max_plan_revisions` 到達時
2. **Execution Confirmation**: 計画承認後、タスク実行前（`PhaseScope::Full` のみ）

```
Planning → Review REJECTED → Revision 1
                ↓
         Review REJECTED → Revision 2
                ↓
         Review REJECTED → Revision 3
                ↓
         max_plan_revisions 到達
                ↓
    ┌─── HiL Mode ───┐
    │                │
    ▼                ▼
Interactive      Auto*
    │                │
    ▼                ▼
User Prompt    自動決定
/approve          │
/reject           │
/edit             │
    │             │
    ▼             ▼
Continue or Abort
```

### HiL Modes / HiL モード

| Mode | Description |
|------|-------------|
| `Interactive` | ユーザーにプロンプトを表示して判断を求める（デフォルト） |
| `AutoReject` | 自動的に中止する |
| `AutoApprove` | 自動的に最後の計画を承認する（危険！） |

### Execution Confirmation Gate / 実行確認ゲート

`PhaseScope::Full` の場合、計画が承認された後でもタスク実行前に追加の確認ゲートがあります。
`HilMode` に応じた決定ソース：

| HilMode | 動作 |
|---------|------|
| `Interactive` | `HumanInterventionPort::request_execution_confirmation()` |
| `AutoApprove` | 自動承認 |
| `AutoReject` | 自動拒否（計画は作成されるが実行されない） |

実装: `application/src/use_cases/run_agent/hil.rs` — `handle_execution_confirmation()`

介入プロンプトの実際の操作方法は [How to Run Agent Tasks](../how-to/run-agent-tasks.md) を参照。

---

## Related / 関連

- [How to Run Agent Tasks](../how-to/run-agent-tasks.md) - 実行手順と HiL の操作
- [Quorum Discussion & Consensus](./quorum-consensus.md) - レビューに使われる合議メカニズム
- [Ensemble Mode](./ensemble-mode.md) - マルチモデル計画生成の設計判断
- [Orchestration Axes](./orchestration-axes.md) - PhaseScope 等の 3 設定軸
- [Interaction Model](./interaction-model.md) - Agent/Ask/Discuss の対等な関係
- [Agent System Reference](../reference/agent-system.md) - 実装の詳細（型・ポート・データフロー）
- [ADR 0001: ToolExecutorPort Layering](./design-decisions/0001-tool-executor-port-layering.md) - エージェント導入時のレイヤリング判断

<!-- LLM Context: Agent の動作原理。Context Gathering → Planning → Plan Review (Quorum) → Execution Confirm → Task Execution (Low-risk 並列 / High-risk Action Review) → Final Review。PhaseScope (Full/Fast/PlanOnly) でフェーズ範囲制御。HiL 2 ゲート: Plan Review HiL (max_plan_revisions 到達時) + Execution Confirmation (PhaseScope::Full のみ)。HilMode: Interactive/AutoReject/AutoApprove。リスク分類: read/glob/grep/web=Low(直接実行), write_file/run_command=High(Quorum Action Review 必須)。 -->
