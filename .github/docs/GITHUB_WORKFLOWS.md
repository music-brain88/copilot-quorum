# GitHub Workflows ドキュメント

このドキュメントでは、`.github` ディレクトリ内のワークフローと設定ファイルについて説明します。

## ディレクトリ構成

```
.github/
├── docs/
│   └── GITHUB_WORKFLOWS.md  # このドキュメント
├── workflows/
│   ├── labels.yml           # ラベル同期ワークフロー
│   └── release-drafter.yml  # リリースドラフト作成ワークフロー
├── labels.yml               # ラベル定義ファイル
└── release-drafter.yml      # リリースドラフト設定
```

---

## Release Drafter

### 概要

PR がマージされると自動的にリリースノートのドラフトを作成・更新します。

### 仕組み

1. PR が main ブランチにマージされる
2. PR のラベルに基づいてカテゴリ分類
3. GitHub Releases にドラフトとして追加
4. セマンティックバージョンを自動計算

### カテゴリ分類

| カテゴリ | 対応ラベル |
|---------|-----------|
| Features | `feature`, `enhancement` |
| Bug Fixes | `fix`, `bugfix`, `bug` |
| Performance | `performance`, `perf` |
| Refactoring | `refactor`, `refactoring` |
| Documentation | `documentation`, `docs` |
| CI/CD | `ci`, `ci/cd` |
| Dependencies | `dependencies`, `deps` |
| Chores | `chore`, `maintenance` |

### 自動ラベリング (Autolabeler)

PR タイトルが Conventional Commits 形式の場合、自動でラベルが付与されます。

| PR タイトル例 | 付与されるラベル |
|--------------|----------------|
| `feat: 新機能追加` | `feature` |
| `fix: バグ修正` | `fix` |
| `docs: READMEを更新` | `docs` |
| `refactor: コード整理` | `refactor` |
| `perf: パフォーマンス改善` | `perf` |
| `ci: ワークフロー更新` | `ci` |
| `chore: 依存関係更新` | `chore` |

### バージョン自動計算

| ラベル | バージョン変更 |
|-------|--------------|
| `major`, `breaking` | メジャー (x.0.0) |
| `feature`, `enhancement` | マイナー (0.x.0) |
| その他 | パッチ (0.0.x) |

### リリース手順

1. GitHub Releases ページを開く
2. 自動生成されたドラフトを確認
3. 必要に応じて編集
4. 「Publish release」をクリック

---

## Label Syncer

### 概要

`labels.yml` の内容を GitHub リポジトリのラベル設定に自動同期します。

### 仕組み

1. `labels.yml` を編集して main にプッシュ
2. GitHub Actions が起動
3. リポジトリのラベルが `labels.yml` の内容に更新される

### 手動実行

GitHub Actions ページから「Sync Labels」ワークフローを手動実行することも可能です。

### ラベルの追加方法

`labels.yml` に以下の形式で追加:

```yaml
- name: new-label
  color: "ff0000"  # 6桁の16進数カラーコード
  description: "ラベルの説明"
```

### 注意事項

- `labels.yml` に存在しないラベルは削除されません（安全のため）
- 色や説明文の変更は反映されます
- ラベル名の変更は「削除 + 新規作成」として扱われます

---

## ラベル一覧

### タイプ系

| ラベル | 色 | 用途 |
|-------|-----|------|
| `feature` | 水色 | 新機能 |
| `enhancement` | 青 | 既存機能の改善 |
| `bug` / `fix` / `bugfix` | 赤 | バグ関連 |
| `refactor` | 黄 | リファクタリング |
| `performance` / `perf` | ピンク | パフォーマンス改善 |
| `documentation` / `docs` | 青 | ドキュメント |
| `ci` / `ci/cd` | 水色 | CI/CD |
| `chore` / `maintenance` | クリーム | メンテナンス |
| `dependencies` / `deps` | 青 | 依存関係 |

### バージョン系

| ラベル | 色 | 用途 |
|-------|-----|------|
| `major` / `breaking` | 赤 | 破壊的変更 |
| `minor` | オレンジ | マイナーバージョン |
| `patch` | 緑 | パッチバージョン |

### ワークフロー系

| ラベル | 用途 |
|-------|------|
| `skip-changelog` | チェンジログから除外 |
| `wip` | 作業中 |
| `help wanted` | ヘルプ募集 |
| `good first issue` | 初心者向け |
| `question` | 質問 |
| `duplicate` | 重複 |
| `invalid` | 無効 |
| `wontfix` | 対応しない |

### ドメイン系 (このプロジェクト固有)

| ラベル | 用途 |
|-------|------|
| `domain` | ドメイン層の変更 |
| `application` | アプリケーション層の変更 |
| `infrastructure` | インフラストラクチャ層の変更 |
| `presentation` | プレゼンテーション層の変更 |
