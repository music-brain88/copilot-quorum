# Documentation Hub / ドキュメントハブ

> Documentation for copilot-quorum, organized by the [Diátaxis](https://diataxis.fr/) framework
>
> copilot-quorum のドキュメント（[Diátaxis](https://diataxis.fr/) フレームワークに準拠）

ドキュメントは「読者が何をしたいか」で 4 象限に分かれています:

|  | **学ぶ** (Study) | **作業する** (Work) |
|--|------------------|---------------------|
| **実践** (Practical) | [Tutorials](#tutorials--チュートリアル) — 手を動かして学ぶ | [How-to Guides](#how-to-guides--ハウツーガイド) — タスクを達成する |
| **理解** (Theoretical) | [Explanation](#explanation--解説) — 仕組みと理由を理解する | [Reference](#reference--リファレンス) — 正確な情報を調べる |

---

## Tutorials / チュートリアル

> 学習向け。順番に手を動かして copilot-quorum を身につける

| Document | Description |
|----------|-------------|
| [Getting Started](./tutorials/getting-started.md) | ビルド → 最初の質問 → `/council` で 3 フェーズ議論体験 |
| [Your First Agent Task](./tutorials/first-agent-task.md) | エージェントのライフサイクルと HiL を体験、Ensemble と比較 |
| [Customizing with Lua](./tutorials/customizing-with-lua.md) | init.lua 作成 → モデル設定 → プラグイン → カスタムツール |

## How-to Guides / ハウツーガイド

> タスク指向。特定の目的を達成する手順

| Document | Description |
|----------|-------------|
| [Run a Quorum Discussion](./how-to/run-a-quorum-discussion.md) | 複数モデルの議論を CLI / TUI から実行する |
| [Use Ensemble Mode](./how-to/use-ensemble-mode.md) | マルチモデル計画生成に切り替える |
| [Run Agent Tasks](./how-to/run-agent-tasks.md) | タスク実行と HiL ゲートの操作 |
| [Use the Modal TUI](./how-to/use-the-tui.md) | モード・コマンド・タブ・$EDITOR 連携 |
| [Manage Project Context](./how-to/manage-project-context.md) | `/init` とコンテキスト予算の制御 |
| [Add Custom Tools](./how-to/add-custom-tools.md) | `quorum.tools.register` で CLI コマンドをツール化 |
| [Write Lua Plugins](./how-to/write-lua-plugins.md) | プラグイン・ユーザーコマンド・イベントフック |
| [Debug with Logs](./how-to/debug-with-logs.md) | 操作ログ・会話ログ・transport dump の使い分け |
| [Extend the Codebase](./how-to/extend-the-codebase.md) | プロバイダー・戦略・ツールの追加（Rust） |

## Reference / リファレンス

> 情報指向。正確な仕様・型・キーの一覧

| Document | Description |
|----------|-------------|
| [Architecture](./reference/architecture.md) | レイヤー構造・データフロー・プロトコル・テスト戦略 |
| [CLI](./reference/cli.md) | CLI フラグ・REPL コマンド・TUI コマンドモード |
| [Configuration](./reference/configuration.md) | 全 29 設定キーと Lua API（init.lua） |
| [Agent System](./reference/agent-system.md) | 設定 4 型・ポート・run_agent/ モジュール構成 |
| [Tool System](./reference/tool-system.md) | 組み込みツール・プロバイダー優先度・trait |
| [Native Tool Use](./reference/native-tool-use.md) | 構造化ツール呼び出し API・ワイヤーフォーマット |
| [Orchestration Internals](./reference/orchestration-internals.md) | Quorum / Ensemble の型・データフロー・StrategyExecutor |
| [Transport](./reference/transport.md) | MessageRouter・SessionChannel・メッセージ分類 |
| [Logging](./reference/logging.md) | 3 種類のログと JSONL スキーマ |
| [Scripting](./reference/scripting.md) | Lua ランタイム・全イベント一覧・サンドボックス |
| [TUI Internals](./reference/tui-internals.md) | Actor パターン・イベントルーティング・ウィジェット |
| [TUI Remote Control](./reference/tui-remote-control.md) | `--listen` JSON-RPC API |

## Explanation / 解説

> 理解指向。仕組み・設計・その理由

| Document | Description |
|----------|-------------|
| [Quorum Discussion & Consensus](./explanation/quorum-consensus.md) | 合議の仕組みと分散システムアナロジー |
| [Ensemble Mode](./explanation/ensemble-mode.md) | 独立生成+投票の設計判断と研究エビデンス |
| [Orchestration Axes](./explanation/orchestration-axes.md) | 3 直交軸（ConsensusLevel × PhaseScope × Strategy） |
| [Agent Behavior](./explanation/agent-behavior.md) | エージェントのライフサイクル・レビュー・HiL |
| [Interaction Model](./explanation/interaction-model.md) | Agent/Ask/Discuss の対等な関係とネスティング |
| [Design Philosophy](./explanation/design-philosophy.md) | DDD + オニオン + 垂直ドメイン分割の理由 |
| [TUI Design](./explanation/tui-design.md) | 「オーケストレーター、エディタではない」設計思想 |
| [Transport & Concurrency](./explanation/transport-and-concurrency.md) | 多重分離が必要な理由と並行パターン |

### Design Decisions / 設計決定記録

GitHub Discussions で決着した設計判断の ADR: [design-decisions/](./explanation/design-decisions/README.md)

| # | Decision | Source |
|---|----------|--------|
| [0001](./explanation/design-decisions/0001-tool-executor-port-layering.md) | ToolExecutorPort のレイヤリング | #10 |
| [0002](./explanation/design-decisions/0002-three-orthogonal-axes.md) | 5モード enum → 3 直交軸 | #38 |
| [0003](./explanation/design-decisions/0003-restore-quorum-consensus-level.md) | Quorum モード復活と ConsensusLevel | #55 |
| [0004](./explanation/design-decisions/0004-role-based-model-configuration.md) | ロールベースモデル設定 | #54/#63 |
| [0005](./explanation/design-decisions/0005-unified-interaction-architecture.md) | Agent/Ask/Discuss の対等化 | #138 |
| [0006](./explanation/design-decisions/0006-tui-content-route-surface.md) | TUI Content/Route/Surface 分離 | #207 |

## Vision / ビジョン

> 将来構想。進行中の RFC と設計段階のアイデア

| Document | Description |
|----------|-------------|
| [Vision Overview](./vision/README.md) | 現在地・ロードマップ・ステータス一覧 |
| [Unified Architecture](./vision/unified-architecture.md) | 4 つの RFC の統合整理 |
| [Knowledge Architecture](./vision/knowledge-architecture.md) | 知識駆動型アーキテクチャ構想 |
| [Workflow Layer](./vision/workflow-layer.md) | DAG ベース並列タスク実行構想 |
| [Extension Platform](./vision/extension-platform.md) | 拡張プラットフォーム構想 |

---

## Reading Paths / 読み順ガイド

### New Users / 新規ユーザー

1. [Getting Started](./tutorials/getting-started.md)
2. [Your First Agent Task](./tutorials/first-agent-task.md)
3. [Use the Modal TUI](./how-to/use-the-tui.md)
4. [Quorum Discussion & Consensus](./explanation/quorum-consensus.md)

### Daily Users / 日常ユーザー

- 作業レシピ: [How-to Guides](#how-to-guides--ハウツーガイド) 一覧
- 調べもの: [CLI](./reference/cli.md) / [Configuration](./reference/configuration.md)

### Contributors / コントリビューター

1. [Design Philosophy](./explanation/design-philosophy.md) — なぜこの構造か
2. [Architecture](./reference/architecture.md) — レイヤーとデータフロー
3. [Design Decisions](./explanation/design-decisions/README.md) — 過去の判断の経緯
4. [Extend the Codebase](./how-to/extend-the-codebase.md) — 拡張ポイントの実践

### TUI Deep-dive / TUI 深掘り

1. [TUI Design](./explanation/tui-design.md)
2. [Use the Modal TUI](./how-to/use-the-tui.md)
3. [TUI Internals](./reference/tui-internals.md) → [TUI Remote Control](./reference/tui-remote-control.md)

---

## External Links / 外部リンク

- [Diátaxis Framework](https://diataxis.fr/) — このドキュメント構成の元になったフレームワーク
- [Original copilot-council (Go)](https://github.com/openjny/copilot-council)
- [GitHub Copilot CLI](https://docs.github.com/en/copilot/github-copilot-in-the-cli)
- [Domain-Driven Design Reference](https://www.domainlanguage.com/ddd/reference/)
- [The Onion Architecture](https://jeffreypalermo.com/2008/07/the-onion-architecture-part-1/)
