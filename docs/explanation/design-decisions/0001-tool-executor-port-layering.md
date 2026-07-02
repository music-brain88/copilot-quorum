# 0001: ToolExecutorPort のレイヤリング / ToolExecutorPort Layering

> **Status**: Implemented
> **Date**: 2026-02-01
> **Source**: [Discussion #10 — RFC: エージェント機能の追加](https://github.com/music-brain88/copilot-quorum/discussions/10)

---

## 背景 / Context

合議（Quorum）を保ちつつ Claude Code のようなエージェント機能
（ツールシステム、コンテキスト収集、計画→実行ループ）を追加する RFC。
初期設計では `ToolExecutor` trait を `domain/tool/traits.rs` に置く案だった。

## 議論 / Discussion

レビューで以下の指摘があった:

1. **Domain 層の責務が曖昧** — `ToolExecutor` が domain にあると、domain 層が
   I/O（ファイル操作・コマンド実行）に依存する可能性がある
2. **既存 port と異なるレイヤー** — `LlmGateway` や `ProgressNotifier` は
   `application/ports/` にあるのに、`ToolExecutor` だけ `domain/tool/` に置くのは一貫性がない
3. 複数ツール対応（read, write, command, glob, grep）のインターフェースが不明確

## 決定 / Decision

- `application/ports/tool_executor.rs` に **`ToolExecutorPort` trait** を新設
  （`supported_tools()` / `tool_spec()` を含む）
- `domain/tool/traits.rs` には純粋なドメインロジックである **`ToolValidator` のみ残す**
- `ToolExecutor` trait は domain から削除し、実装は `infrastructure/tools/` に置く
- 合議ポイントは 3 箇所: Plan Review（必須・スキップ不可）、Action Review（高リスクツール）、Final Review（オプション）
- 初期ツール 5 種（read, write, command, glob, grep）を一括実装

## 理由 / Rationale

- **I/O 境界は port** — LLM 呼び出しと同様、ツール実行も外部世界との境界であり、
  application 層の port として抽象化するのがオニオンアーキテクチャとして一貫する
- **domain は検証のみ** — 「この ToolCall は妥当か」は純粋ロジックなので domain に残せる
- 計画レビュー必須（スキップ不可）は、単一モデル暴走の構造的リスクを防ぐエージェントの根幹

## Related / 関連

- 実装: PR #11
- 現行ドキュメント: [Agent Behavior](../agent-behavior.md), [Tool System Reference](../../reference/tool-system.md), [Design Philosophy](../design-philosophy.md)
- 後続: `ToolSchemaPort`（JSON Schema 変換の Port 分離）も同じ原則で設計された
