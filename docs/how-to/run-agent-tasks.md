# How to Run Agent Tasks / エージェントにタスクを実行させる

> Run autonomous tasks and interact with the human-in-the-loop gates
>
> 自律タスクを実行し、Human-in-the-Loop ゲートを操作する

---

## Run a task / タスクを実行する

```bash
# Solo モードでエージェント実行（デフォルト）
copilot-quorum "Fix the login bug in auth.rs"

# Ensemble モードで計画生成（複雑なタスク向け）
copilot-quorum --ensemble "Design the authentication system"

# 作業ディレクトリを指定
copilot-quorum -w ~/projects/my-app "Add a usage section to the README"

# レビューをスキップして高速実行
copilot-quorum --no-quorum "Rename the helper function"
```

対話モードではそのままタスクを入力し、モードコマンドで切り替えられます:

```
> Fix the broken test in user_service.rs
> /solo               # Solo モードに切り替え
> /ens                # Ensemble モードに切り替え
> /scope plan-only    # 計画だけ作らせて実行しない
> /fast               # レビューをスキップ
```

---

## Respond to the HiL prompt / HiL プロンプトに応答する

Quorum が `max_plan_revisions`（デフォルト 3 回）以内に合意できないと、
ユーザーに判断が委ねられます:

```
═══════════════════════════════════════════════════════════════
  ⚠️  Plan Requires Human Intervention
═══════════════════════════════════════════════════════════════

Revision limit (3) exceeded. Quorum could not reach consensus.

Request:
  Update the README file

Plan Objective:
  Add installation instructions to README

Tasks:
  1. Read README.md
  2. Append installation section

Review History:
  Rev 1: REJECTED [○●○]
    └─ gpt-5.3-codex: Missing error handling
  Rev 2: REJECTED [●○○]
    └─ gemini-3.1-pro-preview: Unclear objective
  Rev 3: REJECTED [○○●]
    └─ claude-sonnet-4.5: Inconsistent approach

Commands:
  /approve  - Execute this plan as-is
  /reject   - Abort the agent
  /edit     - Edit plan manually (未実装)

agent-hil>
```

| Command | 動作 |
|---------|------|
| `/approve` | 最後の計画をそのまま実行 |
| `/reject` | エージェントを中止 |
| `/edit` | 計画を手動編集（未実装） |

---

## Confirm before execution / 実行前の確認ゲート

`PhaseScope::Full`（デフォルト）では、計画承認後・実行開始前にもう一度確認を求められます。
確認をスキップしたい場合は HiL モードを変更します:

```lua
-- ~/.config/copilot-quorum/init.lua
quorum.config.set("agent.hil_mode", "auto_approve")  -- 自動承認（注意して使用）
quorum.config.set("agent.hil_mode", "auto_reject")   -- 計画作成のみ、実行しない
quorum.config.set("agent.max_plan_revisions", 5)     -- 介入までの修正回数を変更
```

---

## Reference GitHub Issues in requests / リクエストで GitHub Issue を参照する

リクエストに Issue/PR 参照を含めると、Context Gathering フェーズで内容が自動解決されます
（`gh` CLI のインストールと認証が必要）:

```bash
copilot-quorum "Fix #123"
copilot-quorum "Implement owner/repo#42 as designed"
copilot-quorum "Apply the review feedback in PR #57"
```

---

## Related / 関連

- [Agent Behavior](../explanation/agent-behavior.md) - ライフサイクルとレビューポイントの仕組み
- [How to Use Ensemble Mode](./use-ensemble-mode.md) - マルチモデル計画生成
- [Configuration Reference](../reference/configuration.md) - `agent.*` キーの詳細
- [CLI Reference](../reference/cli.md) - 全フラグとコマンド
