## Summary

<!-- 変更内容を簡潔に説明してください -->

## Type of Change

<!--
PR タイトルを Conventional Commits 形式にすると自動でラベルが付きます。
例: feat: 新機能追加, fix: バグ修正, docs: ドキュメント更新

該当するものにチェックを入れてください（複数可）
-->

| Type | Label | Version | Description |
|------|-------|---------|-------------|
| - [ ] feat | `feature` | minor | 新機能 |
| - [ ] fix | `fix` | patch | バグ修正 |
| - [ ] docs | `docs` | patch | ドキュメントのみの変更 |
| - [ ] refactor | `refactor` | patch | リファクタリング |
| - [ ] perf | `perf` | patch | パフォーマンス改善 |
| - [ ] ci | `ci` | patch | CI/CD の変更 |
| - [ ] chore | `chore` | patch | その他の変更 |
| - [ ] deps | `dependencies` | patch | 依存関係の更新 |
| - [ ] breaking | `breaking` | **major** | 破壊的変更 |

## Related Issues

<!-- 関連する Issue があればリンクしてください -->
<!-- Closes #123 -->

## Checklist

- [ ] `cargo build` が成功する
- [ ] `cargo test --workspace` が成功する
- [ ] `cargo clippy` で警告がない
- [ ] 破壊的変更がある場合は `breaking` ラベルを付けた

## Additional Notes

<!-- レビュアーへの補足事項があれば記載してください -->
