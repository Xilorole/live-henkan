# AI Agent Workflow Guide

## Overview

このリポジトリはAIエージェント（GitHub Copilot Agent Mode / Claude Code）が
自律的にfeatureを実装し、PRを作成できるように設計されている。

## Workflow Diagram

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  GitHub Issue │────▶│  Agent Work  │────▶│  Pull Request │
│  (work order) │     │  (branch)    │     │  (review)     │
└──────────────┘     └──────────────┘     └──────────────┘
       │                    │                      │
       │              ┌─────┴─────┐                │
       │              │  CI Check │                │
       │              │ (fmt,clippy│                │
       │              │  test)    │                │
       │              └─────┬─────┘                │
       │                    │ pass                  │
       │                    ▼                      │
       └──────────── Squash Merge ◀────────────────┘
```

## Copilot Agent Mode での作業手順

### 1. Issue を選ぶ

VS Code の Copilot Chat (Agent Mode) で以下のように指示する:

```
@workspace Implement the feature described in GitHub Issue #1.
Read CLAUDE.md and .github/copilot/instructions.md for conventions.
Create a feature branch, implement with tests, and prepare a commit.
```

### 2. Copilot が自律的に行うこと

1. `CLAUDE.md` と `instructions.md` を読む
2. 該当crateの `lib.rs` にある型定義とテストを確認
3. feature branch を作成: `git checkout -b feat/romaji-trie-state-machine`
4. 実装 → `cargo test -p <crate>` でテスト → `cargo clippy` で lint
5. Conventional Commit でコミット

### 3. PR を作成

```
@workspace Create a PR for this branch. Use the PR template.
Reference "Closes #1" in the description.
```

あるいはターミナルで:
```bash
gh pr create --fill --base main
```

### 4. レビューとマージ

- CI が通ることを確認
- コード品質を確認（型の設計、エッジケース、ドキュメント）
- Squash merge

## Claude Code での作業手順

Claude Code は `CLAUDE.md` を自動的に読む。

```bash
# Claude Code に Issue を渡す
claude "Implement GitHub Issue #1 (romaji trie state machine). \
  Create a feature branch, implement, test, and commit."
```

Claude Code は以下を自律的に行う:
1. `CLAUDE.md` を読んで規約を理解
2. 既存のコードを読んで型定義・テストを把握
3. 実装 → テスト → clippy → fmt
4. Conventional Commit でコミット

## 複数エージェントの並列作業

依存関係のないMilestoneは並列に進められる:

```
M1 (romaji) ──────────────────────────┐
                                       ├──▶ M5 (engine) ──▶ M6 (TUI) ──▶ M7 (TSF)
M2 (dictionary) ──▶ M3+M4 (converter) ┘
```

- M1 と M2 は並列実行可能
- M3/M4 は M2 に依存
- M5 は M1 と M3/M4 の完了を待つ

## Quality Gates

PRがマージされるための条件:

| Check | Command | Must Pass |
|-------|---------|-----------|
| Format | `cargo fmt --all -- --check` | Yes |
| Lint | `cargo clippy --workspace -- -D warnings` | Yes |
| Test | `cargo test --workspace` | Yes |
| Doc | All `pub` items have `///` comments | Yes |

## Tips for Effective Agent Dispatch

1. **スコープを明確にする**: 1つのIssue = 1つのcrate内の1機能
2. **型を先に定義する**: エージェントはシグネチャから実装を推測する
3. **テストを先に書く**: テストがあればエージェントはそれをパスする実装を書く
4. **コンテキストを渡す**: `@workspace` で関連ファイルを読ませる
5. **段階的に進める**: 大きな機能は複数Issueに分割する
