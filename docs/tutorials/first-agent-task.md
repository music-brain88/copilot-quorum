# Your First Agent Task / はじめてのエージェントタスク

> Watch the agent plan, get reviewed by a quorum, and execute — then take the controls yourself
>
> エージェントの計画 → 合議レビュー → 実行を観察し、HiL ゲートを自分で操作する

[Getting Started](./getting-started.md) を終えていることが前提です。
このチュートリアルではエージェントのライフサイクル全体と Human-in-the-Loop を体験します。

---

## 1. Set up a sandbox / サンドボックスを用意する

エージェントはファイル書き込みやコマンド実行を行うので、練習用のプロジェクトを作りましょう:

```bash
mkdir ~/quorum-sandbox && cd ~/quorum-sandbox
git init
echo "# My Sandbox" > README.md
git add . && git commit -m "init"
```

`-w` フラグでエージェントの作業ディレクトリを指定できます。

## 2. Run a task in Solo mode / Solo モードでタスクを実行

```bash
cargo run -p copilot-quorum -- -w ~/quorum-sandbox "README に使い方セクションを追加して"
```

実行の流れを観察してください:

1. **Context Gathering** — glob / read_file でプロジェクト情報を収集
2. **Planning** — 計画（タスクリスト）を作成
3. **Plan Review** — レビューモデル群が計画に投票（`[●●○]` のような表示）
4. **Execution Confirm** — 実行前の確認ゲート
5. **Task Execution** — 読み取りは即実行、`write_file` は Action Review を経て実行

## 3. Interact with the HiL gate / HiL ゲートを操作する

Execution Confirm では実行してよいか確認を求められます。
また、Quorum が 3 回のリビジョンで合意できないと介入プロンプトが出ます:

```
Commands:
  /approve  - Execute this plan as-is
  /reject   - Abort the agent
```

わざと曖昧なリクエスト（例: 「いい感じにして」）を投げて、
レビューが却下 → 計画修正のループを観察してみるのも学びになります。

## 4. Compare with Ensemble planning / Ensemble 計画と比較する

同じタスクを Ensemble モードで実行してみます:

```bash
cargo run -p copilot-quorum -- --ensemble -w ~/quorum-sandbox "README にインストール手順を追加して"
```

Solo との違い: Planning フェーズで **複数モデルが独立に計画を生成**し、
相互にスコアを付けて **最高評価の計画が選ばれます**（`GPT:8/10, Gemini:7/10` のような表示）。

## 5. Drive the axes from the REPL / 3 軸を切り替える

対話モードで、実行を制御する 3 つの軸を触ってみましょう:

```
/solo               ← ConsensusLevel: 単一モデル
/ens                ← ConsensusLevel: マルチモデル
/fast               ← PhaseScope: レビューをスキップ（速い・要注意）
/scope plan-only    ← PhaseScope: 計画だけ作って実行しない
/scope full         ← PhaseScope: 全フェーズ（デフォルト）
```

`/scope plan-only` は「まず計画だけ見たい」ときに便利です。

## 6. Understand what happened / 何が起きていたかを理解する

- [Agent Behavior](../explanation/agent-behavior.md) — ライフサイクルと 3 つの合議レビューポイント
- [Orchestration Axes](../explanation/orchestration-axes.md) — 3 軸がなぜ直交しているか
- [How to Run Agent Tasks](../how-to/run-agent-tasks.md) — 日常使いのレシピ集
