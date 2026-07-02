# Getting Started / はじめる

> Build copilot-quorum, ask your first question, and hold your first multi-model council
>
> copilot-quorum をビルドして、最初の質問と最初のマルチモデル合議を体験する

このチュートリアルでは、インストールから Quorum Discussion（複数モデルの 3 フェーズ議論）の
体験までを順に進めます。所要時間は 15 分程度です。

---

## 1. Prerequisites / 前提条件

- **GitHub Copilot CLI** がインストール・認証済みであること
  （copilot-quorum はデフォルトで Copilot CLI をバックエンドに使います）
- **Rust toolchain**（`rustup` で stable を推奨）

## 2. Build & verify / ビルドと動作確認

```bash
git clone https://github.com/music-brain88/copilot-quorum.git
cd copilot-quorum
cargo build --release
```

設定の解決が動いているか確認します:

```bash
cargo run -p copilot-quorum -- --show-config
```

現在の設定と `init.lua` の探索パス（`~/.config/copilot-quorum/init.lua`）が表示されます。
まだ init.lua が無くても問題ありません — デフォルト設定で動きます。

## 3. Ask your first question / 最初の質問

まずは最速の形で 1 問聞いてみましょう。`--no-quorum` はレビューをスキップして
単一モデルで即答させるフラグです:

```bash
cargo run -p copilot-quorum -- --no-quorum "このリポジトリの構造を説明して"
```

エージェントが Context Gathering（プロジェクト情報収集）を行ってから回答します。

## 4. Start the interactive mode / 対話モードを起動する

引数なしで起動すると対話モード（TUI）に入ります:

```bash
cargo run -p copilot-quorum
```

プロンプトに現在のモードが表示されます（例: `solo>`）。試しに:

```
/help      ← コマンド一覧
/config    ← 現在の設定を表示
```

## 5. Hold your first council / 最初の合議を開く

いよいよ本題です。`/council` で複数モデルによる Quorum Discussion を実行します:

```
/council Rust のエラーハンドリングは thiserror と anyhow をどう使い分けるべき?
```

3 つのフェーズが順に走るのを観察してください:

1. **Initial Query** — 全モデルが並列で独立に回答
2. **Peer Review** — 各モデルが他のモデルの回答を匿名でレビュー
3. **Synthesis** — モデレーターが全回答とレビューを統合して結論を出す

デフォルトでは統合結果（synthesis）だけが表示されます。
全フェーズの出力を見たい場合はワンショットで:

```bash
cargo run -p copilot-quorum -- -o full "比較: tokio vs async-std"
```

投票の詳細を見たい場合は `--show-votes` を付けます。

## 6. Where to go next / 次のステップ

- [Your First Agent Task](./first-agent-task.md) — エージェントにコードを書かせる
- [Customizing with Lua](./customizing-with-lua.md) — init.lua で好みの設定にする
- [Quorum Discussion & Consensus](../explanation/quorum-consensus.md) — 合議の仕組みを理解する
- [How to Run a Quorum Discussion](../how-to/run-a-quorum-discussion.md) — Discussion の実行レシピ集
